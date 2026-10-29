#!/usr/bin/env python3
"""Claude Code Stop·Notification hook 에서 알림 패널(claude-notifier)을 띄운다.

사용법: python3 notify.py stop | notification   (hook 입력 JSON 은 stdin)
패널은 분리된 프로세스로 띄우고 곧바로 종료하므로 Claude Code 를 기다리게 하지 않는다.
"""
import datetime
import json
import os
import re
import subprocess
import sys

BIN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "claude-notifier")
MAX_BODY = 140
SESSIONS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sessions")
# 승인 대기로 알릴 Notification 종류 — state.py 의 WAIT_NOTIFICATIONS 와 같다
WAIT_NOTIFICATIONS = {"permission_prompt", "worker_permission_prompt", "elicitation_dialog", "elicitation_url_dialog"}


def last_assistant_text(transcript_path):
    """transcript JSONL 에서 마지막 assistant 텍스트를 찾는다."""
    try:
        with open(transcript_path, "rb") as f:
            f.seek(0, os.SEEK_END)
            size = f.tell()
            f.seek(max(0, size - 400_000))
            lines = f.read().decode("utf-8", "ignore").splitlines()
    except OSError:
        return ""
    for line in reversed(lines):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if entry.get("type") != "assistant":
            continue
        content = (entry.get("message") or {}).get("content") or []
        texts = [c.get("text", "") for c in content if isinstance(c, dict) and c.get("type") == "text"]
        text = "\n".join(t for t in texts if t.strip())
        if text:
            return text
    return ""


def interrupted_at(transcript):
    """Esc 로 끊긴 세션이면 끊은 시각(epoch 초), 아니면 None.
    Claude Code 는 사용자가 응답을 끊으면 Stop hook 을 부르지 않아 세션 파일이 작업 중·승인 대기로 남는다.
    대화 기록 끝의 마지막 대화 줄이 중단 표시([Request interrupted by user], … for tool use])인지 본다 — 보드(InterruptReader)와 같은 규칙."""
    try:
        with open(transcript, "rb") as f:
            f.seek(0, os.SEEK_END)
            f.seek(max(0, f.tell() - 256_000))
            data = f.read()
    except (OSError, TypeError):
        return None
    if b"[Request interrupted by user" not in data:
        return None
    # 중단 뒤에는 cost-state·last-prompt 같은 대화 아닌 줄이 붙고, ! 명령·슬래시 명령 출력은 < 로 시작하는 사용자 줄로 남는다
    for line in reversed(data.splitlines()):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        kind = entry.get("type") if isinstance(entry, dict) else None
        if kind not in ("user", "assistant") or entry.get("isSidechain") or entry.get("isMeta"):
            continue
        content = (entry.get("message") or {}).get("content")
        if isinstance(content, list):
            content = content[0].get("text") if content and isinstance(content[0], dict) else None
        text = content if isinstance(content, str) else ""
        if kind == "user" and text.startswith("<"):
            continue
        if kind == "user" and text.startswith("[Request interrupted by user"):
            try:
                return datetime.datetime.fromisoformat(entry["timestamp"].replace("Z", "+00:00")).timestamp()
            except (KeyError, AttributeError, ValueError):
                return os.path.getmtime(transcript)
        return None
    return None


def claude_status(pid, sid, since):
    """Claude Code 의 status(idle·busy·waiting)가 since 뒤에 바뀌었으면 (status, 바뀐 시각 epoch 초), 아니면 (None, None).
    hook 이 오지 않는 때를 메운다 — 첫 응답 전에 Esc 로 끊으면 대화 기록에 중단 표시가 남지 않고(idle), 승인한 도구가 도는 동안은
    PostToolUse 까지 hook 이 없다(busy). Claude Code 가 ~/.claude/sessions/<pid>.json 에 쓰는 값을 그 파일의 sessionId 가
    이 세션일 때만 본다. 보드(ClaudeNames.statusChange)와 같은 규칙."""
    try:
        with open(os.path.expanduser(f"~/.claude/sessions/{int(pid)}.json")) as f:
            j = json.load(f)
    except (OSError, TypeError, ValueError):
        return None, None
    if not isinstance(j, dict) or j.get("sessionId") != sid:
        return None, None
    at = j.get("statusUpdatedAt")
    if not isinstance(at, (int, float)) or at / 1000 <= (since or 0):
        return None, None
    return j.get("status"), at / 1000


def plain(text):
    """마크다운 기호를 덜어내고 한 문단으로 줄인다."""
    text = re.sub(r"```.*?```", " ", text, flags=re.S)
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"\*\*([^*]*)\*\*", r"\1", text)
    text = re.sub(r"^\s*(#+|[-*]|\d+\.|\|)\s*", "", text, flags=re.M)
    text = re.sub(r"\s+", " ", text).strip()
    return text[:MAX_BODY - 1] + "…" if len(text) > MAX_BODY else text


def session_summary(current_id, current_state):
    """전체 세션의 상태별 개수 — 이 hook 이 막 바꾼 세션은 state.py 보다 먼저 실행될 수 있어 직접 반영한다."""
    counts = {"waiting": 0, "working": 0, "idle": 0}
    try:
        names = os.listdir(SESSIONS)
    except OSError:
        names = []
    seen = False
    for name in names:
        if not name.endswith(".json"):
            continue
        try:
            with open(os.path.join(SESSIONS, name)) as f:
                rec = json.load(f)
        except (OSError, ValueError):
            continue
        state = rec.get("state")
        if rec.get("session_id") == current_id:
            state, seen = current_state, True
        elif rec.get("pid"):
            try:
                os.kill(rec["pid"], 0)
            except ProcessLookupError:
                continue
            except PermissionError:
                pass
        # Esc 로 끊긴 다른 세션은 입력을 기다리는 중이다(첫 응답 전에 끊었으면 Claude Code 의 status 로 안다).
        # 승인한 도구가 도는 세션은 승인 대기로 남으므로 Claude Code 가 다시 busy 면 작업 중이다
        if rec.get("session_id") != current_id and state in ("working", "waiting"):
            status, _ = claude_status(rec.get("pid"), rec.get("session_id"), rec.get("since"))
            if interrupted_at(rec.get("transcript_path")) is not None or status == "idle":
                state = "idle"
            elif state == "waiting" and status == "busy":
                state = "working"
        if state in counts:
            counts[state] += 1
    if not seen and current_id:
        counts[current_state] += 1
    return ",".join(f"{k}={v}" for k, v in counts.items())


def first_sentence(text):
    """요약 배너용으로 첫 문장(또는 첫 줄)만 남긴다."""
    text = plain(text)
    m = re.search(r"(.+?[.!?。])(\s|$)", text)
    return m.group(1) if m else text


def main():
    kind = sys.argv[1] if len(sys.argv) > 1 else "stop"
    try:
        data = json.load(sys.stdin)
    except ValueError:
        data = {}
    project = os.path.basename(data.get("cwd") or os.getcwd())

    # 보드 윗줄 종 단추로 전체 알림을 끄면 ~/.claude/notifier/mute-all 이 생긴다 — 세션별 설정보다 먼저 본다
    if os.path.exists(os.path.join(os.path.dirname(SESSIONS), "mute-all")):
        return
    # 보드에서 알림을 끈 세션은 작업 완료·승인 대기 모두 띄우지 않는다
    sid = data.get("session_id")
    if sid and os.path.exists(os.path.join(SESSIONS, f"{sid}.mute")):
        return

    # 응답을 마친 뒤 입력이 없을 때 오는 알림(idle_prompt)은 이미 '작업 완료' 로 알렸고, 다른 백그라운드 작업이 끝남
    # 같은 승인과 상관없는 알림은 승인 대기가 아니므로 띄우지 않는다(종류가 없는 예전 Claude Code 의 알림은 띄운다)
    noti = data.get("notification_type")
    if kind == "notification" and noti and noti not in WAIT_NOTIFICATIONS:
        return

    if kind == "notification":
        title, sound, accent = "승인 대기", "Ping", "wait"
        body = data.get("message") or "Claude 가 입력을 기다리고 있습니다"
    else:
        title, sound, accent = "작업 완료", "Glass", "done"
        body = data.get("last_assistant_message") or last_assistant_text(data.get("transcript_path", ""))

    if not os.path.exists(BIN):
        return
    # 보드 메뉴 Claude Board > 알림 위치 에서 모서리를 고르면 ~/.claude/notifier/notify-position 이 생긴다 — 없으면 가운데
    try:
        with open(os.path.join(os.path.dirname(SESSIONS), "notify-position")) as f:
            position = f.read().strip() or "center"
    except OSError:
        position = "center"
    subprocess.Popen(
        [BIN, "--title", title, "--subtitle", project, "--body", first_sentence(body),
         "--summary", session_summary(data.get("session_id"), "waiting" if accent == "wait" else "idle"),
         "--sound", sound, "--kind", accent, "--duration", "10",
         "--activate", "com.claude.board", "--position", position, "--session", data.get("session_id") or ""],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
