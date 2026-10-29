#!/usr/bin/env python3
"""hook 공유 시험 표를 만든다 — macOS hook(state.py·notify.py·hooks.py)의 main() 을 끝까지 돌리고 바깥만 가짜로 바꾼다.

실행(저장소 맨 위에서): python3 -I hook-table/gen.py [--out <폴더>]
결과: hook-table/state.json·notify.json·hooks.json. Windows 판 Rust 코어 시험(windows/crates/core/tests/hook_table.rs)이
같은 표를 읽어 자기 state::run·notify::prepare·hooks::add 결과와 견준다. 표시 규칙·뺀 것·유지 규칙은 hook-table/README.md.

가짜로 바꾸는 바깥:
- 시계: state.time.time → 시작 시각 + 단계 번호(초). 표에는 값 대신 "@<단계>" 로 적는다
- 프로세스: state.find_claude → 가짜 claude(pid 4242, 고정 tty), state.proc_args → 그 단계의 실행 인자,
  state.alive → 4242 는 산 것, 그 밖은 진짜 생사. state.set_title → 아무것도 쓰지 않음
- 경로: state.SESSIONS·notify.SESSIONS·notify.BIN·hooks.PATH 를 임시 폴더로, HOME(notify 의 ~/.claude/sessions)도 임시 폴더로
- 패널: notify.subprocess.Popen → 실행하지 않고 인자만 잡는다
- 그 밖: stdin(JSON 문자열), sys.argv, os.getcwd(입력에 cwd 가 없을 때), 환경 변수 CLAUDE_PROJECT_DIR
notify 의 다른 세션 생사(os.kill)와 대화 기록·Claude Code 세션 파일 읽기는 진짜로 한다 — 그래서 진짜 pid 를 "@LIVE_PID" 등으로 적는다.
"""
import contextlib
import io
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import types

sys.dont_write_bytecode = True  # 저장소에 __pycache__ 를 남기지 않는다
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, REPO)
import hooks  # noqa: E402
import notify  # noqa: E402
import state  # noqa: E402

CLAUDE_PID = 4242            # 가짜 claude 프로세스 — 늘 산 것으로 본다
TTY = "hook-table-tty"       # 가짜 tty — /dev 에 없는 이름이라 Rust 가 제목을 쓰려 해도 열지 못한다
FALLBACK_CWD = "/hook-table/cwd"   # 입력과 기록에 cwd 가 없을 때의 현재 폴더
SEED = 1000                  # "@seed" = 시작 시각 - 1000초
SID = "sess-1"
OTHER = "sess-2"
TRANSCRIPT = "@DIR/t/main.jsonl"
SESSIONS_REL = "notifier/sessions"
ORIGINAL_ALIVE = state.alive
ORIGINAL_POPEN = subprocess.Popen

ABOUT = ("hook-table/gen.py 가 만든 표. 손으로 고치지 않는다. macOS hook 의 main() 을 끝까지 돌려 얻은 결과이고 "
         "Windows 판 Rust 코어가 같은 표로 견준다(windows/crates/core/tests/hook_table.rs). 자세한 규칙은 hook-table/README.md")
MARKERS = {
    "@DIR": "표를 돌리는 쪽의 임시 폴더(경로 앞부분)",
    "@<k>": "k 번째 단계(0부터)에서 hook 이 쓴 시각",
    "@seed": "심어 둔 시각 = 시나리오 시작 시각 - 1000초. '@seed-<n>' 은 시작 시각 - n초, '@ms:seed-<n>' 은 그 밀리초",
    "@LIVE_PID": "표를 돌리는 프로세스 자신의 pid(산 것)",
    "@PARENT_PID": "그 부모의 pid(산 것)",
    "@DEAD_PID": "띄웠다가 거둔 자식의 pid(죽은 것)",
    "4242": "가짜 claude 프로세스 pid(가짜 생사 판정에서 늘 산 것)",
    "age": "심은 파일의 수정 시각 = 시작 시각 - age초",
}
EXCLUDED = [
    "프로세스 찾기·headless 판정·터미널 제목 쓰기: 플랫폼마다 다르게 구현한다. 표는 가짜 claude 를 넣고 그 값이 기록에 실리는 길만 본다",
    "resume_cmd 의 인용 방식: macOS 는 POSIX 셸(shlex.join), Windows 는 명령줄 규칙. 표에는 인용을 풀어 토큰 목록으로 적는다",
    "Rust 에만 있는 resume_args·pid_ctime: Windows 판이 더한 필드라 견주기 전에 뺀다(resume_args 는 resume_cmd 토큰과 같아야 한다)",
    "패널 인자의 순서·--sound 이름·--activate·--notify·실행 파일 경로: 패널 프로그램이 다르다. 표에는 이름별 값만 적는다",
    "패널의 --session 이 빈 값: macOS 는 빈 문자열로 늘 넘기고 Windows 는 빼지만, 패널은 둘 다 '세션 없음'으로 본다. 표에는 null",
    "경로 구분자가 든 session_id: Windows 판만 받지 않는다(sessions 밖에 쓰지 않게). 표에 넣지 않는다",
    "세션 파일의 키 순서: 표는 키를 정렬해 적는다(키 순서는 windows/crates/core/tests/state_sequence.rs 가 본다)",
    "hook 입력 앞의 UTF-8 BOM·notify 의 깨진 입력: Windows 는 hook 실행 파일(crates/hook main.rs)이 코어를 부르기 전에 다룬다. 표는 코어만 본다",
]


# ---- 표시 바꾸기 ---------------------------------------------------------------

def dead_pid():
    p = ORIGINAL_POPEN(["true"])
    p.wait()
    return p.pid


class Marks:
    """표의 표시(@DIR·@seed·@LIVE_PID …)를 이 실행의 값으로 채우고, 결과의 값을 다시 표시로 바꾼다."""

    def __init__(self, root, t0, pids):
        self.root, self.t0, self.pids = root, t0, pids
        self.times = {}   # 시각 값 → 표시
        self.pid_marks = {v: k for k, v in pids.items()}

    def seed_time(self, mark):
        n = SEED if mark == "@seed" else int(mark[len("@seed-"):])
        value = self.t0 - n
        assert self.times.setdefault(value, mark) == mark, (value, mark)
        return value

    def step_time(self, k):
        value = self.t0 + k
        self.times[value] = "@%d" % k
        return value

    def fill(self, v):
        if isinstance(v, dict):
            return {k: self.fill(x) for k, x in v.items()}
        if isinstance(v, list):
            return [self.fill(x) for x in v]
        if not isinstance(v, str):
            return v
        if v in self.pids:
            return self.pids[v]
        if v == "@seed" or v.startswith("@seed-"):
            return self.seed_time(v)
        if v.startswith("@ms:seed-"):
            return int(round((self.t0 - int(v[len("@ms:seed-"):])) * 1000))
        v = v.replace("@DIR", self.root)
        for mark, pid in self.pids.items():
            v = v.replace(mark, str(pid))
        return v

    def mark(self, v):
        if isinstance(v, dict):
            return {k: self.mark(x) for k, x in v.items()}
        if isinstance(v, list):
            return [self.mark(x) for x in v]
        if isinstance(v, bool) or v is None:
            return v
        if isinstance(v, (int, float)):
            if v in self.times:
                return self.times[v]
            if isinstance(v, int) and v in self.pid_marks:
                return self.pid_marks[v]
            return v
        if isinstance(v, str):
            return v.replace(self.root, "@DIR")
        return v


def reset(root):
    for name in os.listdir(root):
        p = os.path.join(root, name)
        if os.path.isdir(p) and not os.path.islink(p):
            shutil.rmtree(p)
        else:
            os.remove(p)
    os.makedirs(os.path.join(root, SESSIONS_REL))


def plant(seed, m):
    """심을 파일을 쓴다 — json(객체 하나)·lines(JSONL)·text, age 가 있으면 수정 시각을 그만큼 앞당긴다."""
    for f in seed:
        path = os.path.join(m.root, m.fill(f["path"]))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if "json" in f:
            body = json.dumps(m.fill(f["json"]), ensure_ascii=False)
        elif "lines" in f:
            body = "".join(json.dumps(m.fill(x), ensure_ascii=False) + "\n" for x in f["lines"])
        else:
            body = m.fill(f["text"])
        with open(path, "w", encoding="utf-8", newline="\n") as out:
            out.write(body)
    for f in seed:
        if "age" in f:
            t = m.t0 - f["age"]
            os.utime(os.path.join(m.root, m.fill(f["path"])), (t, t))


def normalize_record(rec):
    """세션 기록 — resume_cmd 는 인용을 풀어 토큰 목록으로(Windows 판은 resume_args 로 같은 목록을 만든다)."""
    if isinstance(rec, dict) and isinstance(rec.get("resume_cmd"), str):
        rec = dict(rec, resume_cmd=shlex.split(rec["resume_cmd"]))
    return rec


def snapshot(m):
    """sessions 폴더 — .json 은 내용(표시로 바꾼 것), 그 밖과 JSON 이 아닌 것은 있음(true)만."""
    sessions = os.path.join(m.root, SESSIONS_REL)
    out = {}
    if not os.path.isdir(sessions):
        return out
    for name in sorted(os.listdir(sessions)):
        value = True
        if name.endswith(".json"):
            try:
                with open(os.path.join(sessions, name), encoding="utf-8") as f:
                    value = m.mark(normalize_record(json.load(f)))
            except ValueError:
                value = True
        out[name] = value
    return out


@contextlib.contextmanager
def patched(argv, stdin_text, env):
    saved = (sys.argv, sys.stdin, os.getcwd, {k: os.environ.get(k) for k in env})
    sys.argv, sys.stdin = argv, io.StringIO(stdin_text)
    os.getcwd = lambda: FALLBACK_CWD
    for k, v in env.items():
        if v is None:
            os.environ.pop(k, None)
        else:
            os.environ[k] = v
    try:
        yield
    finally:
        sys.argv, sys.stdin, os.getcwd = saved[0], saved[1], saved[2]
        for k, v in saved[3].items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v


# ---- state.py ------------------------------------------------------------------

def run_state(sc, root, pids):
    reset(root)
    t0 = float(int(time.time()))
    m = Marks(root, t0, pids)
    plant(sc.get("seed", []), m)
    state.SESSIONS = os.path.join(root, SESSIONS_REL)
    state.set_title = lambda tty, text: None
    state.alive = lambda pid: True if pid == CLAUDE_PID else ORIGINAL_ALIVE(pid)
    steps, last_resume = [], None
    for k, (arg, data, opts) in enumerate(sc["steps"]):
        claude = opts.get("claude", sc.get("claude", ["claude"]))
        if "claude_from_resume" in opts:
            # 앞 단계의 resume_cmd 로 다시 연 claude — Claude Code 가 덧붙이는 인자까지
            claude = shlex.split(last_resume) + opts["claude_from_resume"]
            claude = m.mark(claude)
        env = dict(sc.get("env", {}), **opts.get("env", {}))
        argv = m.fill(claude) if claude is not None else None
        now = m.step_time(k)
        state.time = types.SimpleNamespace(time=lambda now=now: now)
        state.find_claude = (lambda: (CLAUDE_PID, TTY)) if argv is not None else (lambda: (None, None))
        state.proc_args = lambda pid, argv=argv: list(argv) if argv is not None and pid == CLAUDE_PID else []
        text = opts["input_text"] if "input_text" in opts else json.dumps(m.fill(data), ensure_ascii=False)
        raised = None
        with patched(["state.py", arg], text, {"CLAUDE_PROJECT_DIR": env.get("CLAUDE_PROJECT_DIR")}):
            try:
                state.main()
            except Exception as e:  # hook 진입점(__main__)이 모두 삼키는 예외 — 일부러 넣은 단계만 허용한다
                raised = e
        if bool(raised) != bool(opts.get("raises")):
            raise SystemExit("state 시나리오 %r 단계 %d: 예외 %r (기대 %r)" % (sc["name"], k, raised, opts.get("raises")))
        try:
            with open(os.path.join(state.SESSIONS, SID + ".json"), encoding="utf-8") as f:
                last_resume = json.load(f).get("resume_cmd")
        except (OSError, ValueError):
            pass
        step = {"arg": arg, "claude": claude, "expect": {"sessions": snapshot(m)}}
        if "input_text" in opts:
            step["input_text"] = opts["input_text"]
        else:
            step["input"] = data
        if env:
            step["env"] = env
        if opts.get("note"):
            step["note"] = opts["note"]
        steps.append(step)
    out = {"name": sc["name"], "steps": steps}
    if sc.get("seed"):
        out["seed"] = sc["seed"]
    return out


# ---- notify.py -----------------------------------------------------------------

def panel_fields(args):
    """패널 인자(--이름 값 …)를 이름별 값으로 — 순서·--sound·--activate 는 뺀다(README)."""
    assert len(args) % 2 == 0, args
    out = {}
    for i in range(0, len(args), 2):
        name, value = args[i], args[i + 1]
        assert name.startswith("--"), args
        out[name[2:]] = value
    for k in ("sound", "activate"):
        out.pop(k, None)
    if not out.get("session"):
        out["session"] = None
    return out


def run_notify(sc, root, pids):
    reset(root)
    t0 = float(int(time.time()))
    m = Marks(root, t0, pids)
    plant(sc.get("seed", []), m)
    notifier = os.path.join(root, "notifier")
    notify.SESSIONS = os.path.join(notifier, "sessions")
    notify.BIN = os.path.join(notifier, "claude-notifier")
    if sc.get("panel_app", True):
        open(notify.BIN, "w").close()
    calls = []
    notify.subprocess = types.SimpleNamespace(
        Popen=lambda args, **kw: calls.append(list(args)), DEVNULL=subprocess.DEVNULL)
    text = json.dumps(m.fill(sc["input"]), ensure_ascii=False)
    with patched(["notify.py", sc["kind"]], text, {"HOME": os.path.join(root, "home")}):
        notify.main()
    assert len(calls) <= 1, calls
    panel = None
    if calls:
        assert calls[0][0] == notify.BIN, calls
        panel = m.mark(panel_fields(calls[0][1:]))
    out = {"name": sc["name"], "kind": sc["kind"], "input": sc["input"], "expect": {"panel": panel}}
    for k in ("seed", "note"):
        if sc.get(k):
            out[k] = sc[k]
    if not sc.get("panel_app", True):
        out["panel_app"] = False
    return out


# ---- hooks.py ------------------------------------------------------------------

def run_hooks(root):
    """hooks.py install 이 settings.json 에 넣는 것 — 명령 문자열은 (스크립트 이름, 인자) 로 줄인다(실행 경로·모양은 판마다 다르다)."""
    reset(root)
    hooks.PATH = os.path.join(root, "home", ".claude", "settings.json")
    with patched(["hooks.py", "install"], "", {}), contextlib.redirect_stdout(io.StringIO()):
        hooks.main()
    with open(hooks.PATH, encoding="utf-8") as f:
        settings = json.load(f)
    assert set(settings) == {"hooks", "env"}, settings.keys()
    table = {}
    for event, groups in settings["hooks"].items():
        rows = []
        for g in groups:
            row = {"hooks": []}
            if "matcher" in g:
                row["matcher"] = g["matcher"]
            for h in g["hooks"]:
                words = shlex.split(h["command"])
                assert words[0] == "python3" and words[1].startswith("$HOME/.claude/notifier/"), words
                run = [os.path.basename(words[1])[:-len(".py")]] + words[2:]
                row["hooks"].append({"type": h["type"], "timeout": h["timeout"], "run": run})
            rows.append(row)
        table[event] = rows
    return {"about": ABOUT, "env": settings["env"], "hooks": table}


# ---- 시나리오 ------------------------------------------------------------------

def ev(event, **kw):
    d = {"session_id": SID, "transcript_path": TRANSCRIPT, "cwd": "/w/repo", "hook_event_name": event}
    d.update(kw)
    return d


def sub(event, aid, **kw):
    return ev(event, **dict({"agent_id": aid, "agent_type": "Explore"}, **kw))


def record(drop=(), **kw):
    """심어 둘 세션 기록 — state.py 가 쓰는 모양."""
    r = {"pid": CLAUDE_PID, "tty": TTY, "headless": False, "started": "@seed", "args": [],
         "transcript_path": TRANSCRIPT, "session_id": SID, "cwd": "/w/repo", "project": "repo",
         "updated": "@seed", "project_dir": "/w/repo", "state": "working", "since": "@seed"}
    r.update(kw)
    for k in drop:
        r.pop(k)
    assert "resume_cmd" not in r  # 인용을 푼 토큰으로 견주므로 심지 않는다
    return r


def sess(name, value=None, age=None, text=None):
    f = {"path": SESSIONS_REL + "/" + name}
    if text is not None:
        f["text"] = text
    else:
        f["json"] = value
    if age is not None:
        f["age"] = age
    return f


def user(text, **kw):
    d = {"type": "user", "message": {"role": "user", "content": text}}
    d.update(kw)
    return d


def assistant(*texts):
    return {"type": "assistant", "message": {"content": [{"type": "text", "text": t} for t in texts]}}


DAY = 86400
LONG = "가나다라마바사아자차카타파하 " * 20
PERM = {"message": "Claude needs your permission to use Bash", "notification_type": "permission_prompt"}


def S(name, steps, **kw):
    d = {"name": name, "steps": [(s[0], s[1], s[2] if len(s) > 2 else {}) for s in steps]}
    d.update(kw)
    return d


STATE = [
    S("보통 흐름: 시작 → 요청 → 도구 → 응답 끝 → 세션 끝", [
        ("idle", ev("SessionStart", source="startup", model="claude-opus-5-5")),
        ("working", ev("UserPromptSubmit", prompt="로그인 화면을 고쳐 주세요", permission_mode="default")),
        ("working", ev("PreToolUse", tool_name="Bash", tool_input={"command": "ls"}, permission_mode="default")),
        ("working", ev("PostToolUse", tool_name="Bash", permission_mode="default")),
        ("idle", ev("Stop", last_assistant_message="로그인 화면을 고쳤습니다.\n\n시험도   통과합니다.")),
        ("ended", ev("SessionEnd", reason="exit")),
    ], claude=["claude", "--model", "opus"], env={"CLAUDE_PROJECT_DIR": "/w/repo"}),
    S("승인 대기: PermissionRequest·승인 알림 뒤 PostToolUse 로 작업 중", [
        ("working", ev("UserPromptSubmit", prompt="배포 스크립트 돌려 줘")),
        ("working", ev("PreToolUse", tool_name="Bash")),
        ("waiting", ev("PermissionRequest", tool_name="Bash")),
        ("waiting", ev("Notification", **PERM)),
        ("working", ev("PostToolUse", tool_name="Bash")),
        ("idle", ev("Stop", last_assistant_message="돌렸습니다.")),
    ]),
    S("승인과 상관없는 Notification 은 기록을 그대로 둔다(223401d)", [
        ("working", ev("UserPromptSubmit", prompt="긴 작업")),
        ("waiting", ev("Notification", message="Background task finished", notification_type="agent_completed")),
        ("waiting", ev("Notification", message="Agent needs input", notification_type="agent_needs_input")),
        ("waiting", ev("Notification", message="Login successful", notification_type="auth_success")),
        ("waiting", ev("Notification", message="push", notification_type="push_notification")),
    ]),
    S("기록이 없을 때 온 승인과 상관없는 Notification 은 파일을 만들지 않는다", [
        ("waiting", ev("Notification", message="Background task finished", notification_type="agent_completed")),
    ]),
    S("승인 계열 알림 네 가지는 모두 승인 대기", [
        ("working", ev("UserPromptSubmit", prompt="여러 승인")),
        ("waiting", ev("Notification", message="팀원 승인", notification_type="worker_permission_prompt")),
        ("working", ev("PostToolUse", tool_name="Bash")),
        ("waiting", ev("Notification", message="MCP 입력", notification_type="elicitation_dialog")),
        ("working", ev("PostToolUse", tool_name="Bash")),
        ("waiting", ev("Notification", message="MCP 주소", notification_type="elicitation_url_dialog")),
        ("working", ev("PostToolUse", tool_name="Bash")),
        ("waiting", ev("Notification", **PERM)),
    ]),
    S("idle_prompt 알림은 입력 대기(idle) — 응답 끝 뒤에는 since 그대로, Esc 뒤에는 idle 로", [
        ("working", ev("UserPromptSubmit", prompt="질문")),
        ("idle", ev("Stop", last_assistant_message="답했습니다.")),
        ("waiting", ev("Notification", message="Claude is waiting for your input", notification_type="idle_prompt")),
        ("working", ev("UserPromptSubmit", prompt="다음 질문")),
        ("waiting", ev("Notification", message="Claude is waiting for your input", notification_type="idle_prompt"),
         {"note": "Esc 로 끊겨 working 으로 남은 세션"}),
    ]),
    S("종류 없는 예전 Notification 은 승인 대기 — message 앞 200자(공백 정리 없음)", [
        ("working", ev("UserPromptSubmit", prompt="옛 Claude Code")),
        ("waiting", ev("Notification", message="  허락이   필요합니다  " + LONG)),
    ]),
    S("서브에이전트 한 살이: 시작·도구·끝, 세션 상태는 그대로, 메타 파일의 설명·백그라운드", [
        ("working", ev("UserPromptSubmit", prompt="코드 찾아 줘", model="claude-opus-5-5", permission_mode="default")),
        ("subagent", sub("SubagentStart", "a1", model="sub-model", permission_mode="plan")),
        ("working", sub("PreToolUse", "a1", tool_name="Grep")),
        ("working", sub("PostToolUse", "a1", tool_name="Grep")),
        ("subagent", sub("SubagentStart", "a2", agent_type="Plan")),
        ("subagent", sub("SubagentStop", "a1", last_assistant_message="  찾았습니다.\n  세 곳입니다.  ")),
        ("idle", ev("Stop", last_assistant_message="찾았습니다.")),
    ], seed=[{"path": "t/main/subagents/agent-a1.meta.json",
              "json": {"description": "코드 찾기", "requestShape": "background"}}]),
    S("started_seen: 시작 hook 없이 생긴 기록은 거짓, 뒤늦은 시작 hook 부터 참", [
        ("working", ev("UserPromptSubmit", prompt="여럿 띄우기")),
        ("subagent", sub("SubagentStop", "orphan_stop")),
        ("working", sub("PreToolUse", "orphan_tool", tool_name="Read")),
        ("waiting", sub("PermissionRequest", "orphan_perm", tool_name="Bash")),
        ("working", sub("PostToolUse", "orphan_perm", tool_name="Bash")),
        ("subagent", sub("SubagentStart", "orphan_tool")),
        ("subagent", sub("SubagentStart", "orphan_tool"), {"note": "두 번째 시작 hook 은 시작 시각을 바꾸지 않는다"}),
    ]),
    S("서브에이전트 승인: 작업 중 → 승인 대기 → 승인 알림 → 승인 끝나면 작업 중", [
        ("working", ev("UserPromptSubmit", prompt="서브에이전트에게 맡기기")),
        ("subagent", sub("SubagentStart", "a1")),
        ("waiting", sub("PermissionRequest", "a1", tool_name="Bash")),
        ("waiting", ev("Notification", **PERM)),
        ("working", sub("PostToolUse", "a1", tool_name="Bash")),
        ("working", ev("PreToolUse", tool_name="Read")),
    ]),
    S("쉬는 세션의 승인 알림은 서브에이전트 것 — 서브에이전트가 움직이면 대기로", [
        ("idle", ev("SessionStart", source="startup")),
        ("waiting", ev("Notification", **PERM)),
        ("working", sub("PostToolUse", "bg", tool_name="Bash")),
        ("waiting", sub("PermissionRequest", "bg", tool_name="Bash")),
        ("subagent", sub("SubagentStop", "bg", last_assistant_message="끝")),
    ]),
    S("메인의 승인은 서브에이전트가 움직여도 풀리지 않는다", [
        ("working", ev("UserPromptSubmit", prompt="메인 승인")),
        ("waiting", ev("PermissionRequest", tool_name="Bash")),
        ("waiting", ev("Notification", **PERM)),
        ("working", sub("PostToolUse", "a1", tool_name="Read")),
        ("working", ev("PostToolUse", tool_name="Bash")),
    ]),
    S("세션이 끝나면 도는 서브에이전트를 모두 닫는다", [
        ("working", ev("UserPromptSubmit", prompt="둘 띄우기")),
        ("subagent", sub("SubagentStart", "a1")),
        ("subagent", sub("SubagentStart", "a2")),
        ("subagent", sub("SubagentStop", "a2")),
        ("ended", ev("SessionEnd", reason="exit")),
    ]),
    S("끝난 서브에이전트가 다시 깨어나면 ended 를 지우고 running", [
        ("working", ev("UserPromptSubmit", prompt="백그라운드")),
        ("subagent", sub("SubagentStart", "a1")),
        ("subagent", sub("SubagentStop", "a1", last_assistant_message="일단 끝")),
        ("working", sub("PreToolUse", "a1", tool_name="Bash")),
        ("waiting", sub("PermissionRequest", "a1", tool_name="Bash")),
    ]),
    S("끝난 지 1시간 넘은 서브에이전트는 지운다", [
        ("subagent", sub("SubagentStart", "new")),
    ], seed=[sess(SID + ".json", record(subagents={
        "old": {"type": "Explore", "started": "@seed-9000", "state": "done", "started_seen": True,
                "updated": "@seed-7200", "ended": "@seed-7200"},
        "recent": {"type": "Explore", "started": "@seed-3000", "state": "done", "started_seen": True,
                   "updated": "@seed-1800", "ended": "@seed-1800"},
        "running": {"type": "Plan", "started": "@seed-9000", "state": "running", "started_seen": True,
                    "updated": "@seed-9000"},
    }))]),
    S("서브에이전트 이벤트가 먼저 온 세션은 작업 중으로 시작", [
        ("subagent", sub("SubagentStart", "a1")),
        ("working", sub("PreToolUse", "a1", tool_name="Read")),
    ]),
    S("Esc 로 끊긴 뒤 새 요청은 working → working 이어도 since 를 새로 잡는다", [
        ("working", ev("UserPromptSubmit", prompt="새 요청")),
        ("working", ev("PreToolUse", tool_name="Read"), {"note": "도구 hook 은 since 그대로"}),
        ("working", ev("UserPromptSubmit", prompt="새 요청"), {"note": "같은 문구 — topic 은 그대로, since 는 새로"}),
    ], seed=[sess(SID + ".json", record(topic="끊긴 요청", since="@seed-300"))]),
    S("project_dir: cd·워크트리에도 띄운 폴더 그대로, 환경 변수가 비면 기록 값", [
        ("idle", ev("SessionStart", cwd="/w/repo")),
        ("working", ev("PreToolUse", cwd="/w/repo/sub")),
        ("working", ev("PreToolUse", cwd="/w/repo/.claude/worktrees/t1")),
        ("working", ev("PreToolUse", cwd="/w/repo/.claude/worktrees/t1"), {"env": {"CLAUDE_PROJECT_DIR": ""}}),
        ("working", ev("PreToolUse", cwd="/elsewhere"), {"env": {"CLAUDE_PROJECT_DIR": None}}),
        ("working", ev("PreToolUse", cwd="/elsewhere"), {"env": {"CLAUDE_PROJECT_DIR": "/w/other"}}),
    ], env={"CLAUDE_PROJECT_DIR": "/w/repo"}),
    S("project_dir 이 없는 예전 기록은 그 hook 의 cwd 부터, cwd 가 없으면 기록·현재 폴더", [
        ("working", ev("PreToolUse", cwd="/w/repo/sub")),
        ("working", {"session_id": SID, "hook_event_name": "PostToolUse"}),
    ], seed=[sess(SID + ".json", record(drop=("project_dir",), cwd="/w/old", project="old"))]),
    S("새 세션에 cwd 가 없으면 현재 폴더", [
        ("working", {"session_id": SID, "hook_event_name": "UserPromptSubmit", "prompt": "cwd 없음"}),
    ]),
    S("이어 열기: bypass 로 여러 번 이어 열어도 같은 플래그가 쌓이지 않는다", [
        ("idle", ev("SessionStart", source="startup", permission_mode="bypassPermissions", model="claude-opus-5-5")),
        ("ended", ev("SessionEnd", reason="exit")),
        ("idle", ev("SessionStart", source="resume", permission_mode="bypassPermissions"),
         {"claude_from_resume": ["--allow-dangerously-skip-permissions"]}),
        ("ended", ev("SessionEnd", reason="exit")),
        ("idle", ev("SessionStart", source="resume", permission_mode="bypassPermissions"),
         {"claude_from_resume": ["--allow-dangerously-skip-permissions"]}),
        ("ended", ev("SessionEnd", reason="exit")),
        ("idle", ev("SessionStart", source="resume", permission_mode="bypassPermissions"),
         {"claude_from_resume": ["--allow-dangerously-skip-permissions"]}),
    ], claude=["claude", "--dangerously-skip-permissions", "--model", "opus"]),
    S("넘길 실행 인자 고르기(carry_args)와 기록의 권한 모드·모델로 바꿔 끼우기", [
        ("idle", ev("SessionStart", source="startup", permission_mode="acceptEdits", model="claude-sonnet-5")),
    ], claude=["/opt/claude/bin/claude", "--model", "opus", "--model=sonnet", "--permission-mode=plan",
               "--add-dir", "a b", "c", "--add-dir", "a b", "--mcp-config", "m.json", "n.json",
               "--settings", "{\"x\": 1}", "--plugin-dir", "/p q", "--agent", "rev", "--chrome", "--chrome",
               "--ide", "--verbose", "-c", "--print", "--resume", "old", "--add-dir=x", "--add-dir=x",
               "--model", "opus", "--agent"]),
    S("권한 모드·모델: 메인 hook 만 바꾸고 빈 값·문자열 아닌 값은 버린다", [
        ("idle", ev("SessionStart", source="startup", permission_mode="default", model="m1")),
        ("working", ev("UserPromptSubmit", prompt="계획부터", permission_mode="plan")),
        ("working", sub("PreToolUse", "a1", tool_name="Read", permission_mode="acceptEdits", model="sub-model")),
        ("working", ev("PreToolUse", tool_name="Read", permission_mode=5, model="")),
        ("idle", ev("Stop", model="m2")),
    ], claude=["claude", "--permission-mode", "default", "--model=old"]),
    S("사용 한도(limit_hit): 한도 문구만 남기고 메인 hook 이 다시 오면 지운다", [
        ("working", ev("UserPromptSubmit", prompt="큰 작업")),
        ("idle", ev("StopFailure", error="rate_limit",
                    last_assistant_message="You've hit your weekly limit ·  resets 4pm")),
        ("working", sub("PreToolUse", "a1", tool_name="Read"), {"note": "서브에이전트 hook 은 지우지 않는다"}),
        ("idle", ev("StopFailure", error="rate_limit", last_assistant_message="API Error: Repeated 529 Overloaded"),
         {"note": "한도 문구가 아닌 rate_limit 은 새로 쓰지도 지우지도 않는다"}),
        ("idle", ev("StopFailure", error="server_error", last_assistant_message="usage limit"),
         {"note": "rate_limit 이 아닌 StopFailure 는 지우지 않는다"}),
        ("working", ev("UserPromptSubmit", prompt="다시")),
        ("idle", ev("StopFailure", error="rate_limit", last_assistant_message="Claude AI USAGE LIMIT reached " + LONG)),
        ("idle", ev("SessionStart", source="resume")),
    ]),
    S("topic: 공백 정리 뒤 앞 40자, < 로 시작하거나 빈 요청은 무시", [
        ("working", ev("UserPromptSubmit",
                       prompt="  여러   칸의\n공백과 줄바꿈을\t한 칸으로 줄이고 앞 마흔 글자만 주제로 남깁니다 뒤는 버립니다  ")),
        ("working", ev("UserPromptSubmit", prompt="<command-name>/clear</command-name>")),
        ("working", ev("UserPromptSubmit", prompt="")),
        ("working", ev("UserPromptSubmit", prompt="😀 이모지와　한글\x1c섞인 요청")),
    ]),
    S("last_message: idle 은 응답 공백 정리 200자, waiting 은 message 앞 200자, 없으면 그대로", [
        ("idle", ev("Stop", last_assistant_message="첫 줄\n\n둘째　줄\x1c셋째   " + LONG)),
        ("working", ev("PreToolUse", tool_name="Bash", message="도구 hook 의 message 는 쓰지 않는다")),
        ("waiting", ev("PermissionRequest", tool_name="Bash", message="  승인   " + LONG)),
        ("idle", ev("Stop")),
    ]),
    S("Claude 프로세스를 못 찾은 세션: pid 없음 → 다시 찾지 않다가 SessionStart 에서 찾는다", [
        ("working", ev("UserPromptSubmit", prompt="headless"), {"claude": None}),
        ("working", ev("PreToolUse", tool_name="Read"), {"note": "기록의 pid 가 null 이면 산 것으로 보고 다시 찾지 않는다"}),
        ("idle", ev("SessionStart", source="resume")),
    ], claude=["claude", "--chrome"]),
    S("죽은 pid 의 기록은 프로세스를 다시 잡는다", [
        ("working", ev("PreToolUse", tool_name="Read")),
    ], claude=["claude", "--verbose"], seed=[sess(SID + ".json", record(pid="@DEAD_PID", args=["--old"]))]),
    S("산 pid 인데 args 가 없는 예전 기록은 args 만 채운다", [
        ("working", ev("PreToolUse", tool_name="Read")),
    ], claude=["claude", "--ide"], seed=[sess(SID + ".json", record(drop=("args",)))]),
    S("SessionStart 는 산 기록도 프로세스 정보를 새로 잡는다(--resume)", [
        ("idle", ev("SessionStart", source="resume")),
    ], claude=["claude", "--chrome"], seed=[sess(SID + ".json", record(args=["--old"], state="idle"))]),
    S("2일 지난 기록 정리(prune)는 SessionStart 에만", [
        ("idle", ev("Stop", last_assistant_message="정리 전"), {"note": "SessionStart 가 아니면 정리하지 않는다"}),
        ("idle", ev("SessionStart", source="startup")),
    ], seed=[
        sess("old-dead.json", {"session_id": "old-dead", "pid": "@DEAD_PID", "state": "ended"}, age=3 * DAY),
        sess("old-nopid.json", {"session_id": "old-nopid", "pid": None, "state": "ended"}, age=3 * DAY),
        sess("old-live.json", {"session_id": "old-live", "pid": "@LIVE_PID", "state": "idle"}, age=3 * DAY),
        sess("old-parent.json", {"session_id": "old-parent", "pid": "@PARENT_PID", "state": "idle"}, age=3 * DAY),
        sess("fresh-dead.json", {"session_id": "fresh-dead", "pid": "@DEAD_PID", "state": "ended"}, age=1 * DAY),
        sess("old-broken.json", text="{", age=3 * DAY),
        sess("notes.txt", text="x", age=3 * DAY),
        sess(".old-live.lock", text=""),
        sess(".gone.lock", text=""),
        sess("gone.paused", {"paused_at": "@seed", "topic": "x"}),
        sess("gone.mute", text=""),
        sess("fresh-dead.mute", text=""),
        sess("old-live.paused", {"paused_at": "@seed", "topic": "x"}),
    ]),
    S("일시중지 표지(.paused)는 그 세션의 새 요청에만 지운다", [
        ("working", ev("PreToolUse", tool_name="Read")),
        ("idle", ev("Stop", last_assistant_message="쉼")),
        ("working", ev("UserPromptSubmit", prompt="재개")),
    ], seed=[
        sess(SID + ".json", record(state="idle")),
        sess(SID + ".paused", {"paused_at": "@seed", "topic": "끊긴 요청"}),
        sess(OTHER + ".json", record(session_id=OTHER, pid=None, state="idle")),
        sess(OTHER + ".paused", {"paused_at": "@seed", "topic": "다른 세션"}),
    ]),
    S("세션 id 가 없거나 입력이 깨졌으면 아무것도 쓰지 않는다", [
        ("working", {"hook_event_name": "UserPromptSubmit", "prompt": "id 없음", "cwd": "/w/repo"}),
        ("working", {"session_id": "", "hook_event_name": "UserPromptSubmit", "cwd": "/w/repo"}),
        ("working", None, {"input_text": "not json", "raises": True}),
        ("idle", None, {"input_text": "", "raises": True}),
    ]),
]


def N(name, kind, data, **kw):
    d = {"name": name, "kind": kind, "input": data}
    d.update(kw)
    return d


def nev(event, **kw):
    d = {"session_id": SID, "transcript_path": TRANSCRIPT, "cwd": "/w/repo", "hook_event_name": event}
    d.update(kw)
    return d


STOP_MSG = "로그인 화면을 고쳤습니다. 시험도 통과합니다."
OTHERS = [  # 다른 세션들 — 상태별 개수용
    sess("idle-a.json", {"session_id": "idle-a", "pid": None, "state": "idle"}),
    sess("work-b.json", {"session_id": "work-b", "state": "working", "since": "@seed"}),
    sess("wait-c.json", {"session_id": "wait-c", "pid": None, "state": "waiting", "since": "@seed"}),
    sess("ended-d.json", {"session_id": "ended-d", "pid": None, "state": "ended"}),
    sess("broken.json", text="{broken"),
    sess(".idle-a.lock", text=""),
    sess("idle-a.mute", text=""),
]

NOTIFY = [
    N("작업 완료: 응답의 첫 문장, 이 세션만 있으면 대기 1", "stop", nev("Stop", last_assistant_message=STOP_MSG)),
    N("작업 완료: 응답이 없으면 대화 기록의 마지막 assistant 글", "stop", nev("Stop"), seed=[
        {"path": "t/main.jsonl", "lines": [
            user("안녕"), assistant("이전 답"),
            assistant("  \n", "대화 기록에서 꺼낸 답입니다. 둘째 문장."),
            {"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Read"}]}},
            user([{"type": "tool_result", "content": "ok"}]),
        ]}]),
    N("작업 완료: 응답이 빈 문자열이어도 대화 기록에서", "stop", nev("Stop", last_assistant_message=""), seed=[
        {"path": "t/main.jsonl", "lines": [assistant("기록의 답!"), {"type": "summary"}]}]),
    N("작업 완료: 응답도 대화 기록도 없으면 빈 본문", "stop", nev("Stop")),
    N("승인 대기: message 가 본문", "notification", nev("Notification", **PERM)),
    N("승인 대기: message 가 없으면 기본 문구", "notification", nev("Notification", notification_type="permission_prompt")),
    N("idle_prompt 알림은 띄우지 않는다", "notification",
      nev("Notification", message="Claude is waiting for your input", notification_type="idle_prompt")),
    N("다른 작업이 끝남(agent_completed)은 띄우지 않는다", "notification",
      nev("Notification", message="Background task finished", notification_type="agent_completed")),
    N("로그인 성공 같은 알림은 띄우지 않는다", "notification",
      nev("Notification", message="Login successful", notification_type="auth_success")),
    N("팀원 승인(worker_permission_prompt)은 띄운다", "notification",
      nev("Notification", message="팀원이 승인을 기다립니다", notification_type="worker_permission_prompt")),
    N("MCP 입력 요청(elicitation_url_dialog)은 띄운다", "notification",
      nev("Notification", message="MCP 서버가 주소를 엽니다", notification_type="elicitation_url_dialog")),
    N("종류 없는 예전 알림은 띄운다", "notification", nev("Notification", message="Claude needs your permission")),
    N("전체 알림 끔(mute-all)이면 작업 완료도 띄우지 않는다", "stop", nev("Stop", last_assistant_message=STOP_MSG),
      seed=[{"path": "notifier/mute-all", "text": ""}]),
    N("전체 알림 끔이면 승인 대기도 띄우지 않는다", "notification", nev("Notification", **PERM),
      seed=[{"path": "notifier/mute-all", "text": ""}, sess(SID + ".json", record(state="working"))]),
    N("이 세션 알림 끔(.mute)이면 띄우지 않는다", "notification", nev("Notification", **PERM),
      seed=[sess(SID + ".mute", text="")]),
    N("다른 세션의 .mute 는 상관없다", "stop", nev("Stop", last_assistant_message=STOP_MSG),
      seed=[sess(OTHER + ".mute", text=""), sess(OTHER + ".json", record(session_id=OTHER, pid=None, state="idle"))]),
    N("알림 위치 파일(앞뒤 공백 정리)", "stop", nev("Stop", last_assistant_message=STOP_MSG),
      seed=[{"path": "notifier/notify-position", "text": "top-right\n"}]),
    N("알림 위치 파일이 비었으면 가운데", "notification", nev("Notification", **PERM),
      seed=[{"path": "notifier/notify-position", "text": "  \n"}]),
    N("상태별 개수: 이 세션은 막 바뀐 상태로, 끝난 세션·깨진 파일·표지 파일은 세지 않는다", "stop",
      nev("Stop", last_assistant_message=STOP_MSG),
      seed=OTHERS + [sess(SID + ".json", record(pid=None, state="working"))]),
    N("상태별 개수: 승인 대기는 이 세션을 waiting 으로", "notification", nev("Notification", **PERM),
      seed=OTHERS + [sess(SID + ".json", record(pid=None, state="idle"))]),
    N("상태별 개수: 이 세션 파일이 아직 없으면 그 상태에 1 을 더한다", "notification", nev("Notification", **PERM),
      seed=OTHERS),
    N("상태별 개수: pid 가 죽은 다른 세션은 빼고 산 것은 센다", "stop", nev("Stop", last_assistant_message=STOP_MSG), seed=[
        sess("live.json", {"session_id": "live", "pid": "@LIVE_PID", "state": "idle"}),
        sess("dead-w.json", {"session_id": "dead-w", "pid": "@DEAD_PID", "state": "working", "since": "@seed"}),
        sess("dead-i.json", {"session_id": "dead-i", "pid": "@DEAD_PID", "state": "idle"}),
    ]),
    N("상태별 개수: Esc 로 끊긴 다른 세션은 대기(대화 기록 끝의 중단 표시)", "stop",
      nev("Stop", last_assistant_message=STOP_MSG), seed=[
          sess("esc.json", {"session_id": "esc", "pid": None, "state": "working", "since": "@seed",
                            "transcript_path": "@DIR/t/esc.jsonl"}),
          {"path": "t/esc.jsonl", "lines": [
              user("요청"), assistant("하는 중"),
              user([{"type": "text", "text": "[Request interrupted by user for tool use]"}],
                   timestamp="2026-10-28T00:00:00.000Z"),
              {"type": "cost-state"}, user("<local-command-stdout>ok</local-command-stdout>"),
              user("메타", isMeta=True), user("곁가지", isSidechain=True)]},
          sess("esc-wait.json", {"session_id": "esc-wait", "pid": None, "state": "waiting", "since": "@seed",
                                 "transcript_path": "@DIR/t/esc-wait.jsonl"}),
          {"path": "t/esc-wait.jsonl", "lines": [user("요청"), user("[Request interrupted by user]")]},
          sess("resumed.json", {"session_id": "resumed", "pid": None, "state": "working", "since": "@seed",
                                "transcript_path": "@DIR/t/resumed.jsonl"}),
          {"path": "t/resumed.jsonl", "lines": [user("[Request interrupted by user]"), user("다시 해 줘"),
                                                assistant("다시 하는 중")]},
      ]),
    N("상태별 개수: Claude Code 의 status 로 덮기(busy 는 승인 대기를 작업 중으로, idle 은 대기로)", "notification",
      nev("Notification", **PERM), seed=[
          sess("busy.json", {"session_id": "busy", "pid": "@LIVE_PID", "state": "waiting", "since": "@seed-100"}),
          {"path": "home/.claude/sessions/@LIVE_PID.json",
           "json": {"pid": "@LIVE_PID", "sessionId": "busy", "status": "busy", "statusUpdatedAt": "@ms:seed-50"}},
          sess("idle-now.json", {"session_id": "idle-now", "pid": "@PARENT_PID", "state": "working",
                                 "since": "@seed-100"}),
          {"path": "home/.claude/sessions/@PARENT_PID.json",
           "json": {"pid": "@PARENT_PID", "sessionId": "idle-now", "status": "idle", "statusUpdatedAt": "@ms:seed-50"}},
      ]),
    N("상태별 개수: status 가 since 보다 앞이거나 다른 세션 id 면 덮지 않는다", "stop",
      nev("Stop", last_assistant_message=STOP_MSG), seed=[
          sess("old-busy.json", {"session_id": "old-busy", "pid": "@LIVE_PID", "state": "waiting",
                                 "since": "@seed-100"}),
          {"path": "home/.claude/sessions/@LIVE_PID.json",
           "json": {"pid": "@LIVE_PID", "sessionId": "old-busy", "status": "busy", "statusUpdatedAt": "@ms:seed-200"}},
          sess("cleared.json", {"session_id": "cleared", "pid": "@PARENT_PID", "state": "working",
                                "since": "@seed-100"}),
          {"path": "home/.claude/sessions/@PARENT_PID.json",
           "json": {"pid": "@PARENT_PID", "sessionId": "after-clear", "status": "idle", "statusUpdatedAt": "@ms:seed-50"}},
      ]),
    N("이 세션은 Claude Code status 로 덮지 않는다", "stop", nev("Stop", last_assistant_message=STOP_MSG), seed=[
        sess(SID + ".json", record(pid="@LIVE_PID", state="working", since="@seed-100")),
        {"path": "home/.claude/sessions/@LIVE_PID.json",
         "json": {"pid": "@LIVE_PID", "sessionId": SID, "status": "busy", "statusUpdatedAt": "@ms:seed-50"}},
    ]),
    N("세션 id 가 없으면 --session 없음, 이 세션을 더하지 않는다", "notification",
      {"hook_event_name": "Notification", "cwd": "/w/repo", "message": "id 없는 알림"}, seed=OTHERS),
    N("세션 id 가 빈 문자열이어도 --session 없음", "stop",
      {"session_id": "", "hook_event_name": "Stop", "cwd": "/w/repo", "last_assistant_message": STOP_MSG}),
    N("cwd 가 없으면 현재 폴더의 이름", "stop", {"session_id": SID, "hook_event_name": "Stop",
                                              "last_assistant_message": STOP_MSG}),
    N("cwd 가 / 로 끝나면 빈 이름", "stop", nev("Stop", cwd="/w/repo/", last_assistant_message=STOP_MSG)),
    N("본문 다듬기: 코드 블록·인라인 코드·굵게·줄머리 기호를 덜고 첫 문장", "stop", nev("Stop", last_assistant_message=(
        "## 요약\n\n```rust\nfn main() {}\n```\n- **굵게** 항목 `code` 입니다! 다음\n1. 둘째 줄\n| 표 | 칸 |"))),
    N("본문 다듬기: 140자를 넘으면 139자 + …, 문장 끝이 없으면 전부", "stop",
      nev("Stop", last_assistant_message="가나다라마바사아자차 " * 16)),
    N("첫 문장: 버전 번호의 점은 문장 끝이 아니다", "stop",
      nev("Stop", last_assistant_message="v1.2.3 버전을 올렸습니다. 확인해 주세요.")),
    N("첫 문장: 。·? 로도 끊는다", "notification",
      nev("Notification", notification_type="permission_prompt", message="이 파일을 지울까요? 되돌릴 수 없습니다。끝")),
    N("패널 실행 파일이 없으면 띄우지 않는다", "stop", nev("Stop", last_assistant_message=STOP_MSG), panel_app=False),
]


# ---- 실행 ----------------------------------------------------------------------

def write(path, obj):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(obj, ensure_ascii=False, sort_keys=True, indent=1) + "\n")


def main():
    out = HERE
    if sys.argv[1:2] == ["--out"]:
        out = sys.argv[2]
    elif sys.argv[1:]:
        raise SystemExit("사용법: python3 -I hook-table/gen.py [--out <폴더>]")
    repo_sessions = os.path.join(REPO, "sessions")
    had_sessions = os.path.exists(repo_sessions)
    pids = {"@LIVE_PID": os.getpid(), "@PARENT_PID": os.getppid(), "@DEAD_PID": dead_pid()}
    assert len(set(pids.values()) | {CLAUDE_PID}) == 4, pids
    root = tempfile.mkdtemp(prefix="hook-table-")
    try:
        state_table = [run_state(sc, root, pids) for sc in STATE]
        notify_table = [run_notify(sc, root, pids) for sc in NOTIFY]
        hooks_table = run_hooks(root)
    finally:
        shutil.rmtree(root, ignore_errors=True)
    assert had_sessions or not os.path.exists(repo_sessions), "저장소 안에 sessions/ 가 생겼다"
    head = {"about": ABOUT, "markers": MARKERS, "excluded": EXCLUDED,
            "claude": {"pid": CLAUDE_PID, "tty": TTY}, "fallback_cwd": FALLBACK_CWD}
    write(os.path.join(out, "state.json"), dict(head, scenarios=state_table))
    write(os.path.join(out, "notify.json"), dict(head, scenarios=notify_table))
    write(os.path.join(out, "hooks.json"), hooks_table)
    print("state %d · notify %d 시나리오, hooks %d 이벤트 → %s" % (len(state_table), len(notify_table),
                                                             len(hooks_table["hooks"]), out))


if __name__ == "__main__":
    main()
