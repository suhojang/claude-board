#!/usr/bin/env python3
"""Claude Code 세션 상태를 기록하고 Ghostty 탭 제목에 상태 아이콘을 붙인다.

사용법: python3 state.py <working|waiting|idle|ended|subagent>   (hook 입력 JSON 은 stdin)
서브에이전트 안에서 온 hook(agent_id 가 있는 입력)은 세션 파일의 subagents 에만 기록한다.
상태 파일: ~/.claude/notifier/sessions/<session_id>.json — 메뉴 막대 앱(claude-status)이 읽는다.
"""
import json
import ctypes
import fcntl
import os
import re
import shlex
import struct
import subprocess
import sys
import tempfile
import time

SESSIONS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sessions")
ICONS = {"working": "🔵", "waiting": "🟠", "idle": "🟢", "ended": "⚫"}
# 사람의 답을 기다리게 하는 Notification(notification_type) — 승인 창(서브에이전트·팀원 것 포함)과 MCP 의 입력 요청.
# 다른 백그라운드 작업이 끝남(agent_completed)·입력 필요(agent_needs_input), 로그인 성공 같은 알림도 같은 hook 으로 오므로
# (2.1.312 에 17가지) 이 밖의 알림은 이 세션의 상태가 아니다. notify.py 에 같은 목록이 있다
WAIT_NOTIFICATIONS = {"permission_prompt", "worker_permission_prompt", "elicitation_dialog", "elicitation_url_dialog"}


def find_claude():
    """hook 프로세스의 조상 가운데 claude 세션 프로세스의 pid 와 tty 를 찾는다.

    Claude Code 의 백그라운드 작업은 데몬(claude daemon run) → pty 호스트(--bg-pty-host) → 세션 프로세스로 뜨고,
    세션 프로세스는 이름을 "claude bg-spare" 로 바꿔 쓰거나 버전 경로(…/claude/versions/2.1.312)로 뜬다(2.1.312 실측).
    이름이 claude 인 것만 찾으면 그 위의 pty 호스트나 데몬을 세션으로 잡는다.
    """
    pid = os.getppid()
    for _ in range(6):
        try:
            out = subprocess.run(["ps", "-o", "ppid=,tty=,comm=", "-p", str(pid)],
                                 capture_output=True, text=True, timeout=2).stdout.split(None, 2)
        except (OSError, subprocess.SubprocessError):
            return None, None
        if len(out) < 3:
            return None, None
        ppid, tty, comm = out
        comm = comm.strip()
        words = os.path.basename(comm).split()
        if "/claude/versions/" in comm or words[:1] == ["claude"]:
            argv = proc_args(pid)
            if "--bg-pty-host" in argv or argv[1:2] == ["daemon"] or words[1:] == ["bg-pty-host"]:
                return None, None
            # 백그라운드 작업의 tty 는 pty 호스트가 만든 것이라 사람이 보는 터미널이 아니다
            if tty.startswith("?") or "--bg-pty-host" in proc_args(int(ppid)):
                tty = None
            return pid, tty
        pid = int(ppid)
    return None, None


def proc_args(pid):
    """macOS sysctl KERN_PROCARGS2 로 프로세스 실행 인자를 공백 손실 없이 읽는다."""
    try:
        libc = ctypes.CDLL(None)
        mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2
        size = ctypes.c_size_t(1 << 18)
        buf = ctypes.create_string_buffer(size.value)
        if libc.sysctl(mib, 3, buf, ctypes.byref(size), None, 0) != 0:
            return []
        raw = buf.raw[:size.value]
        argc = struct.unpack("i", raw[:4])[0]
        rest = raw[4:].split(b"\0", 1)[1].lstrip(b"\0")  # 실행 파일 경로를 건너뛴다
        return [a.decode("utf-8", "replace") for a in rest.split(b"\0")[:argc]]
    except Exception:
        return []


# 세션을 다시 열 때 그대로 넘길 실행 인자 — 값을 받는 것, 값을 여러 개 받는 것, 값이 없는 것
CARRY_ONE = {"--model", "--permission-mode", "--agent", "--settings", "--plugin-dir"}
CARRY_MANY = {"--add-dir", "--mcp-config"}
CARRY_FLAG = {"--dangerously-skip-permissions", "--allow-dangerously-skip-permissions", "--chrome", "--ide", "--verbose"}


def carry_args(argv):
    # 같은 플래그·같은 (이름, 값) 은 한 번만 넘긴다 — Claude Code 는 bypass 권한 모드로 이어 열 때
    # --allow-dangerously-skip-permissions 를 스스로 하나 더 붙여 띄우므로, 그대로 넘기면 이어 열 때마다 하나씩 늘어난다
    out, i, seen = [], 1, set()
    while i < len(argv):
        a = argv[i]
        name = a.split("=", 1)[0]
        if "=" in a and name in CARRY_ONE | CARRY_MANY:
            if a not in seen:
                seen.add(a)
                out.append(a)
        elif a in CARRY_FLAG:
            if a not in seen:
                seen.add(a)
                out.append(a)
        elif a in CARRY_ONE and i + 1 < len(argv):
            if (a, argv[i + 1]) not in seen:
                seen.add((a, argv[i + 1]))
                out += [a, argv[i + 1]]
            i += 1
        elif a in CARRY_MANY:
            out.append(a)
            while i + 1 < len(argv) and not argv[i + 1].startswith("-"):
                out.append(argv[i + 1])
                i += 1
        i += 1
    return out


def resume_cmd(rec):
    """종료된 세션을 같은 인자와 마지막 권한 모드·모델로 다시 여는 명령."""
    args = list(rec.get("args", []))
    for flag, key in (("--permission-mode", "permission_mode"), ("--model", "model")):
        if rec.get(key):
            j = next((k for k, a in enumerate(args) if a == flag or a.startswith(flag + "=")), None)
            if j is not None:
                del args[j:j + (1 if "=" in args[j] else 2)]
            args += [flag, rec[key]]
    return shlex.join(["claude", *args, "--resume", rec["session_id"]])


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except (OSError, TypeError):
        return True


def set_title(tty, text):
    try:
        with open(f"/dev/{tty}", "w") as t:
            t.write(f"\033]2;{text}\007")
    except OSError:
        pass


def prune():
    """종료된 지 2일이 넘은 세션 파일을 지운다 — 보드를 띄우지 않는 동안 쌓이는 것을 막는다."""
    limit = time.time() - 2 * 86400
    for name in os.listdir(SESSIONS):
        path = os.path.join(SESSIONS, name)
        if name.endswith((".lock", ".paused", ".mute")):
            base = name[1:-5] if name.endswith(".lock") else name.rsplit(".", 1)[0]
            try:
                if not os.path.exists(os.path.join(SESSIONS, base + ".json")):
                    os.remove(path)
            except OSError:
                pass
            continue
        try:
            if os.path.getmtime(path) < limit:
                with open(path) as f:
                    pid = json.load(f).get("pid")
                if not pid or not alive(pid):
                    os.remove(path)
        except (OSError, ValueError):
            pass


def subagent_meta(transcript, agent_id):
    """Claude Code 가 남기는 subagents/agent-<id>.meta.json 에서 호출 설명과 실행 방식을 읽는다."""
    if not transcript:
        return {}
    meta = os.path.join(transcript[:-6] if transcript.endswith(".jsonl") else transcript,
                        "subagents", f"agent-{agent_id}.meta.json")
    try:
        with open(meta) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def track_subagent(rec, data, state, now):
    """서브에이전트 시작·도구 호출·승인 대기·종료를 세션 기록의 subagents 에 남긴다."""
    subs = rec.setdefault("subagents", {})
    aid = data["agent_id"]
    event = data.get("hook_event_name")
    sub = subs.setdefault(aid, {"type": data.get("agent_type") or "", "started": now, "state": "running", "started_seen": False})
    # 시작 hook 을 본 서브에이전트만 시작 시각을 믿는다 — 시작 없이 끝 hook 만 왔으면 시작 시각은 끝 시각을 흉내 낸 값이다
    if event == "SubagentStart" and not sub.get("started_seen"):
        sub.update(started=now, started_seen=True)
    if not sub.get("description"):
        meta = subagent_meta(data.get("transcript_path"), aid)
        if meta:
            sub["description"] = meta.get("description", "")
            sub["background"] = meta.get("requestShape") == "background"
    if event == "SubagentStop":
        sub.update(state="done", ended=now)
        if data.get("last_assistant_message"):
            sub["last_message"] = " ".join(data["last_assistant_message"].split())[:200]
    else:
        # 끝난 서브에이전트도 남은 백그라운드 작업 때문에 다시 깨어나 도구를 쓸 수 있다
        sub.pop("ended", None)
        sub["state"] = "waiting" if state == "waiting" else "running"
        if data.get("tool_name"):
            sub["tool"] = data["tool_name"]
    sub["updated"] = now
    # 끝난 지 1시간이 넘은 서브에이전트는 기록에서 지운다
    for k in [k for k, v in subs.items() if v.get("state") == "done" and now - v.get("ended", now) > 3600]:
        del subs[k]


def main():
    state = sys.argv[1] if len(sys.argv) > 1 else "idle"
    data = json.load(sys.stdin)
    sub_event = bool(data.get("agent_id"))
    # 응답을 마친 뒤 입력을 오래 기다릴 때 오는 Notification 은 승인 대기가 아니라 (입력) 대기다
    kind = data.get("notification_type")
    if state == "waiting" and kind == "idle_prompt":
        state = "idle"
    # 승인과 상관없는 알림은 세션 기록을 건드리지 않는다(종류가 없는 예전 Claude Code 의 알림은 승인으로 본다)
    elif state == "waiting" and kind and kind not in WAIT_NOTIFICATIONS:
        return
    sid = data.get("session_id")
    if not sid:
        return
    os.makedirs(SESSIONS, exist_ok=True)
    if state == "idle" and data.get("hook_event_name") == "SessionStart":
        prune()
    path = os.path.join(SESSIONS, f"{sid}.json")
    # 서브에이전트가 여럿 동시에 돌면 hook 도 동시에 돌므로, 읽고 고쳐 쓰는 동안 세션 파일을 잠근다
    with open(os.path.join(SESSIONS, f".{sid}.lock"), "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        update(path, sid, state, data, sub_event)


def update(path, sid, state, data, sub_event):
    try:
        with open(path) as f:
            rec = json.load(f)
    except (OSError, ValueError):
        rec = {}

    now = time.time()
    # 처음 보는 세션이거나 --resume 으로 다시 열린 세션이면 프로세스 정보를 새로 잡는다
    if "pid" not in rec or data.get("hook_event_name") == "SessionStart" or not alive(rec.get("pid")):
        pid, tty = find_claude()
        rec.update(pid=pid, tty=tty, headless=tty is None, started=now)
        if pid:
            rec["args"] = carry_args(proc_args(pid))
    elif "args" not in rec and rec.get("pid"):
        rec["args"] = carry_args(proc_args(rec["pid"]))
    if data.get("transcript_path"):
        rec["transcript_path"] = data["transcript_path"]
    for key in ("permission_mode", "model"):
        if not sub_event and isinstance(data.get(key), str) and data[key]:
            rec[key] = data[key]
    cwd = data.get("cwd") or rec.get("cwd") or os.getcwd()
    rec.update(session_id=sid, cwd=cwd, project=os.path.basename(cwd), updated=now)
    # 프로젝트 경로 — claude 를 띄운 폴더. cwd 는 Claude 가 하위 폴더로 cd 하거나 워크트리에 들어가면 따라 바뀌지만
    # Claude Code 가 hook 에 넘기는 CLAUDE_PROJECT_DIR 은 그대로다(2.1.304 실측). 보드는 이 경로로 카드를 묶는다
    rec["project_dir"] = os.environ.get("CLAUDE_PROJECT_DIR") or rec.get("project_dir") or cwd

    # 같은 프로젝트에서 여러 세션을 띄워도 구분되도록 마지막 요청 문구를 주제로 남긴다
    prompt = " ".join((data.get("prompt") or "").split())
    topic_changed = bool(prompt) and not prompt.startswith("<") and prompt[:40] != rec.get("topic")
    if topic_changed:
        rec["topic"] = prompt[:40]

    notification = data.get("hook_event_name") == "Notification"
    if sub_event:
        track_subagent(rec, data, state, now)
        # 서브에이전트의 도구 호출은 세션 상태를 바꾸지 않는다 — 승인 요청만 세션도 승인 대기로 올리고,
        # 승인이 끝나 서브에이전트가 다시 움직이면 세션을 승인 요청 전 상태로 돌려놓는다
        if state == "waiting":
            if rec.get("state") != "waiting":
                rec["state_before_sub_wait"] = rec.get("state", "working")
        elif rec.get("state") == "waiting" and "state_before_sub_wait" in rec:
            state = rec.pop("state_before_sub_wait")
        else:
            state = rec.get("state", "working")
    elif state == "ended":
        for sub in rec.get("subagents", {}).values():
            if sub.get("state") != "done":
                sub.update(state="done", ended=now)
    elif notification and data.get("notification_type") == "permission_prompt" and rec.get("state") == "idle":
        # 쉬고 있는 세션은 스스로 승인을 묻지 않으므로 서브에이전트의 승인 요청이다 — 서브에이전트가 다시 움직이면 대기로 돌린다
        rec["state_before_sub_wait"] = "idle"

    # 승인 알림(Notification)은 서브에이전트의 승인 요청이어도 agent_id 없이 오므로 되돌릴 상태를 지우지 않는다
    if not sub_event and not notification:
        rec.pop("state_before_sub_wait", None)
    # 보드에서 일시중지한 세션에 새 요청이 들어오면 재개된 것이다
    if data.get("hook_event_name") == "UserPromptSubmit":
        try:
            os.remove(os.path.join(SESSIONS, f"{sid}.paused"))
        except OSError:
            pass
    # 사용 한도에 걸려 응답이 끊긴 세션 — 보드가 계정을 자동 전환한 뒤 이어서 진행시킨다.
    # rate_limit 은 서버의 일시적인 요청 제한일 수도 있어 "You've hit your weekly limit · resets 4pm" 같은 한도 문구일 때만 남긴다.
    # 새 요청·도구 사용·응답 완료·다시 열기가 오면 세션이 다시 움직인 것이므로 지운다(Claude Code 가 한도가 풀린 뒤 스스로 이어 간 경우 포함)
    event = data.get("hook_event_name")
    if event == "StopFailure" and data.get("error") == "rate_limit":
        msg = " ".join((data.get("last_assistant_message") or "").split())
        if re.search(r"hit your|usage limit", msg, re.I):
            rec["limit_hit"] = {"at": now, "message": msg[:200]}
    elif not sub_event and event in ("UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SessionStart"):
        rec.pop("limit_hit", None)
    changed = rec.get("state") != state
    # Esc 로 끊으면 hook 이 오지 않아 다음 요청은 작업 중 → 작업 중이다 — 새 요청이면 시작 시각을 새로 잡는다
    if changed or event == "UserPromptSubmit":
        rec.update(state=state, since=now)
    if state == "idle" and data.get("last_assistant_message"):
        rec["last_message"] = " ".join(data["last_assistant_message"].split())[:200]
    if state == "waiting" and data.get("message"):
        rec["last_message"] = data["message"][:200]

    rec["resume_cmd"] = resume_cmd(rec)

    # 동시에 도는 hook 끼리 같은 임시 파일을 덮어쓰지 않도록 프로세스마다 다른 이름을 쓴다
    fd, tmp = tempfile.mkstemp(dir=SESSIONS, prefix=f"{sid}.", suffix=".tmp")
    with os.fdopen(fd, "w") as f:
        json.dump(rec, f, ensure_ascii=False)
    os.replace(tmp, path)

    if (changed or topic_changed) and rec.get("tty"):
        topic = rec.get("topic", "")
        topic = f" · {topic[:20]}…" if len(topic) > 20 else (f" · {topic}" if topic else "")
        set_title(rec["tty"], f"{ICONS.get(state, '')} {rec['project']}{topic}")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
