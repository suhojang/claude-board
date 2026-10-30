#!/usr/bin/env python3
# 가짜 claude: 자율주행 엔진 시험용(모델 비용 0). 엔진이 띄우는 `claude -p --input-format stream-json --output-format stream-json …` 자리에 둔다.
# 쓰는 법: 시험 실행 폴더의 run.json 에 "claude_path": "<이 파일 절대 경로>"(--dev 출발만) — 진행 상황.md "시험 방법"
#
# 두 가지 꼴
#  1) 한도 흐름(첫 입력에 FAKE 줄이 없을 때, 2026-10-14 exec-25 실제 stream 기록의 모양):
#     새 세션(--session-id): 첫 입력을 받으면 rate_limit_event(rejected) → assistant(error rate_limit) → result(subtype success, is_error true, 429)
#     이어 열기(--resume): init → 빈 result(num_turns 0) → 3초 뒤 일을 하고(limit-ok.txt) 진짜 result
#  2) 줄대로(새 세션의 첫 입력에 "FAKE …" 줄이 있을 때, 단위 폴더 fake-scenario.txt 에 남겨 이어 열 때도 같은 줄을 읽는다).
#     한 줄을 하면 fake-done.txt 에 그 번호를 남겨, 오류·멈춤 뒤 이어 열면 남은 줄부터 한다(진짜 claude 가 멈춘 자리에서 잇는 것처럼).
#     사람이 보낸 말(엔진이 "- " 로 들여 붙임)의 FAKE 줄은 그 차례에만 따른다. 줄(앞의 "- " 는 벗긴다):
#       FAKE WRITE <상대 경로> <글>      파일을 쓴다(작업 폴더 기준)
#       FAKE COMMIT <메시지>             쓴 파일만 add 해 커밋
#       FAKE SLEEP <초>                  턴 동안 기다린다(SIGINT 를 받으면 그 자리에서 멈춘다)
#       FAKE SH <셸 명령>                명령을 돌린다(모델 쪽 쓰기 흉내, 샌드박스 없음)
#       FAKE CHILD <초>                  도구 명령처럼 새 process group 의 자식(python sleep)을 띄워 두고 그 pid 를 fake-child.pid 에
#       FAKE ORPHAN                      SIGINT 를 받아도 CHILD 를 끄지 않는다(엔진이 process group 을 치우는지 보려고. 진짜 claude 는 도구 명령도 끝낸다)
#       FAKE ERROR <횟수> <꼴>           이 단위의 처음 <횟수>번 띄우기를 오류로 끝낸다. 꼴: exit:<코드>(init 뒤 결과 없이 그 코드로 끝남)
#                                        · result(init 뒤 is_error result, 입력이 닫히면 끝) · preinit:<코드>(init 도 내지 않고 끝남)
#       FAKE IGNORE_INT / FAKE IGNORE_TERM   그 신호를 무시한다(엔진의 SIGTERM·SIGKILL 단계를 보려고)
#       FAKE NEXT <제목>|<까닭> · FAKE NEW <제목>|<까닭>   결과 기록의 next_start·new_work
#       FAKE TICKET <임시 번호>|<제목>|<까닭>   결과 기록의 new_tickets(⑩-3 실행 중에 새로 필요해진 티켓, 임시 번호는 TMP-<단위>-<차례>)
#       FAKE RESULT <JSON 파일 절대 경로>    결과 기록을 그 파일의 객체로(견본 기록, 보고서 시험. NEXT·NEW 는 쓰지 않음)
#     SIGINT(엔진의 멈춤)를 받으면 진짜 claude 처럼(2026-10-20 실측) result subtype error_during_execution·is_error true·
#     terminal_reason aborted_tools 를 내고 종료 코드 0 으로 끝난다.
#
#  3) MCP 읽기 세션(인자에 --mcp-config 가 있을 때, 4단계 ⑩-2): `claude -p --output-format json` 처럼 표준 입력의 질문을 받고 결과 JSON 한 줄을 낸다.
#     받은 인자·--settings·--mcp-config 파일 내용·ENABLE_TOOL_SEARCH 를 지금 폴더(엔진이 만든 빈 임시 폴더)의 fake-read.json 에 남긴다.
#     MCP 서버는 띄우지 않고 바깥으로 아무것도 보내지 않는다. 질문의 "주소:" 줄 주소에 ?fake=<꼴> 이 있으면 그 꼴로 답한다:
#       ok(기본) · html(절 글에 태그) · extra(형식 밖 칸) · long(요약 400자) · secret(요약에 가짜 토큰) · text(JSON 앞에 다른 글) · fence(```json 울타리)
#       · write(쓰기 도구를 부르려 했다고 치고, 그 이름이 --disallowedTools 의 어느 꼴에 걸리는지 남김) · slow(600초 기다림, 엔진 시간 상한 시험) · error(오류 결과)
#  4) 초안 세션(인자에 --input-format 이 없고 --tools 가 있을 때, 5단계 ⑪-2 주제로 시작): `claude -p --output-format json` 처럼 표준 입력의 글을 받는다.
#     첫 줄이 "Claude Board 자율주행의 초안 세션입니다." 이고 글 안에 "FAKE DRAFT <견본 폴더 절대 경로>" 줄(주제 글에 넣음)이 있으면 그 폴더의
#     결과 파일(spec.md·units.json·cards.json·autopilot.md·scenarios/<단위 id>/…)을 지금 폴더(엔진이 정한 결과 폴더 draft/)에 그대로 복사한다.
#     "FAKE DRAFT_SLEEP <초>" 줄이 있으면 그만큼 기다린 뒤(시간 상한 시험), "FAKE DRAFT_ERROR" 면 오류 결과로 끝낸다.
#     받은 인자·--settings 파일 내용·글 앞부분을 결과 폴더의 숨은 파일 .fake-draft.json 에 남긴다(기계 검사는 숨은 파일을 보지 않음). 바깥으로 아무것도 보내지 않는다
#     판정 고치기 차례(5단계 ⑪-3, 글에 "판정 고치기 차례입니다." 가 있음)면 "FAKE DRAFT_FIX <견본 폴더>" 줄의 폴더를 복사한다(엔진이 먼저 넣어 둔 지금 초안 위에 덮어씀)
#  5) 판정 비판 세션(5단계 ⑪-3, 같은 인자 꼴, 첫 줄 "Claude Board 자율주행의 판정 비판 세션입니다."): 글 안의 "FAKE CRITIC <견본 폴더>" 줄(단위 글·주제 글에 넣음)의
#     findings.json·cases/ 를 결과 폴더에 복사한다. 받은 인자(--model 포함)·설정·글 앞부분은 .fake-critic.json 에
#  6) 세트 이어 쓰기 세션(5단계 ⑪-3, 첫 줄 "Claude Board 자율주행의 세트 이어 쓰기 세션입니다."): "FAKE REVISE <견본 폴더>" 줄의 units.json·autopilot.md 를 복사, 기록은 .fake-revise.json
#     5)·6) 은 견본 줄이 없으면 찾은 것이 없다고 끝낸다(비판은 빈 findings.json). 4)~6) 의 견본 폴더가 절대 경로가 아니면 이 파일이 있는 폴더(시험 도구/) 기준으로 찾는다(견본 단위 글에 "FAKE CRITIC 견본 비판" 처럼 쓰려고)
# 남기는 것(단위 폴더 $CLAUDE_BOARD_UNIT_DIR): fake-args.log(띄울 때 인자 한 줄), fake-in.log(표준 입력 줄 원문, 띄울 때마다 머리 줄),
# fake-signals.log(받은 신호와 시각), fake-runs.count(이 단위에서 띄운 횟수). 뜰 때 진짜 claude 처럼 작업 폴더에서 git status 를 한 번 부른다
# (샌드박스 밖에서 도는 claude 의 git, 2026-10-30 5-4 실측 흉내)
import json, os, signal, subprocess, sys, threading, time

UNIT = os.environ.get("CLAUDE_BOARD_UNIT_DIR", ".")
args = sys.argv[1:]
T0 = time.time()

def log(name, text):
    with open(os.path.join(UNIT, name), "a") as f:
        f.write(text)

log("fake-args.log", json.dumps(args, ensure_ascii=False) + "\n")
runs_path = os.path.join(UNIT, "fake-runs.count")
try:
    runs = int(open(runs_path).read().strip() or "0") + 1
except Exception:
    runs = 1
open(runs_path, "w").write(str(runs))
log("fake-in.log", "### run %d pid %d %s\n" % (runs, os.getpid(), "resume" if "--resume" in args else "new"))

if "--mcp-config" in args:
    import fnmatch, re as _re
    from urllib.parse import urlsplit, parse_qs
    def after(k):
        return args[args.index(k) + 1] if k in args and args.index(k) + 1 < len(args) else None
    files = {}
    for k in ("--mcp-config", "--settings"):
        try:
            files[k] = json.load(open(after(k)))
        except Exception as ex:
            files[k] = "읽지 못함: %s" % ex
    prompt = sys.stdin.read()
    m = _re.search(r"^주소: (\S+)", prompt, _re.M)
    link = m.group(1) if m else ""
    mode = (parse_qs(urlsplit(link).query).get("fake") or ["ok"])[0]
    servers = list((files["--mcp-config"] or {}).get("mcpServers", {}).keys()) if isinstance(files["--mcp-config"], dict) else []
    rec = {"argv": args, "mcp_config": files["--mcp-config"], "settings": files["--settings"], "env_tool_search": os.environ.get("ENABLE_TOOL_SEARCH"),
           "cwd": os.getcwd(), "servers": servers, "mode": mode, "prompt_head": prompt[:600]}
    if mode == "write":
        srv = servers[0] if servers else "x"
        deny = (after("--disallowedTools") or "").split(" ")
        allow = (after("--allowedTools") or "").split(" ")
        tried = {}
        for t in ("mcp__%s__createPage" % srv, "mcp__%s__update_page" % srv, "mcp__%s__purge_and_get_status" % srv, "mcp__%s__get_page" % srv, "Bash", "Write"):
            tried[t] = {"denied_by": [d for d in deny if d and fnmatch.fnmatchcase(t, d)][:3],
                        "allowed_by": [a for a in allow if a and fnmatch.fnmatchcase(t, a)][:3],
                        "builtin_tools_flag": after("--tools")}
        rec["tried"] = tried
    json.dump(rec, open("fake-read.json", "w"), ensure_ascii=False, indent=1)
    if mode == "slow":
        time.sleep(600)
    meta = {"format": "claude-board-meta", "version": 1, "source": "mcp", "id": "wiki-7", "url": link, "title": "위키 문서(가짜 MCP 읽기)",
            "kind": "위키 문서", "modified": "2026-10-29T09:00:00Z", "author": "가짜 작성자",
            "toc": [{"level": 1, "title": "개요"}, {"level": 2, "title": "운영 규칙"}],
            "fields": [{"name": "상태", "value": "게시됨"}], "links": [{"title": "첨부 1", "url": "https://wiki.example.com/a/1", "kind": "attachment"}],
            "length": 640, "summary": ["회의실 운영 규칙을 정리한 위키 문서입니다."], "sections": []}
    if mode == "html":
        meta["sections"] = [{"title": "개요", "text": "<p>본문을 통째로 옮김</p>"}]
    if mode == "extra":
        meta["body"] = "본문 전체"
    if mode == "long":
        meta["summary"] = ["가" * 400]
    if mode == "secret":
        meta["summary"].append("배포 토큰 " + "ATA" + "TT3x" + "Ff9" * 10 + "fakeZZ")
    text = json.dumps(meta, ensure_ascii=False)
    if mode == "text":
        text = "읽은 결과입니다:\n" + text
    if mode == "fence":
        text = "```json\n" + text + "\n```"
    res = {"type": "result", "subtype": "success", "is_error": False, "result": text, "total_cost_usd": 0, "num_turns": 1, "session_id": "fake-read"}
    if mode == "error":
        res = {"type": "result", "subtype": "error_during_execution", "is_error": True, "total_cost_usd": 0}
    sys.stdout.write(json.dumps(res, ensure_ascii=False) + "\n")
    sys.exit(0)

if "--input-format" not in args and "--tools" in args:
    import re as _re, shutil
    def after(k):
        return args[args.index(k) + 1] if k in args and args.index(k) + 1 < len(args) else None
    prompt = sys.stdin.read()
    try:
        settings = json.load(open(after("--settings")))
    except Exception as ex:
        settings = "읽지 못함: %s" % ex
    here = os.path.dirname(os.path.abspath(__file__))
    def sample(tag):
        mm = _re.search(r"^\s*(?:- )?FAKE " + tag + r" (.+?)\s*$", prompt, _re.M)   # 견본 폴더 경로에 빈칸이 있어도 줄 끝까지
        if not mm:
            return None
        p = mm.group(1)
        return p if p.startswith("/") else os.path.join(here, p)
    kind = "draft"
    if prompt.startswith("Claude Board 자율주행의 판정 비판 세션입니다."):
        kind = "critic"
    elif prompt.startswith("Claude Board 자율주행의 세트 이어 쓰기 세션입니다."):
        kind = "revise"
    fixing = kind == "draft" and "판정 고치기 차례입니다." in prompt.split("\n", 1)[0]
    rec = {"argv": args, "model": after("--model"), "settings": settings, "cwd": os.getcwd(), "kind": kind, "fix": fixing,
           "env_draft": os.environ.get("CLAUDE_BOARD_DRAFT"), "env_critic": os.environ.get("CLAUDE_BOARD_CRITIC"), "env_revise": os.environ.get("CLAUDE_BOARD_REVISE"),
           "prompt_head": prompt[:1500], "prompt_chars": len(prompt), "first_line": prompt.split("\n", 1)[0]}
    src_dir = sample({"critic": "CRITIC", "revise": "REVISE"}.get(kind, "DRAFT_FIX" if fixing else "DRAFT"))
    class _M:
        def __init__(self, g): self._g = g
        def group(self, i): return self._g
    m = _M(src_dir) if src_dir else None
    copied = []
    ok = m is not None and (kind != "draft" or prompt.startswith("Claude Board 자율주행의 초안 세션입니다."))
    sl = _re.search(r"^\s*(?:- )?FAKE DRAFT_SLEEP (\d+)", prompt, _re.M)
    if sl:
        time.sleep(int(sl.group(1)))
    if ok and os.path.isdir(m.group(1)):
        src = m.group(1)
        for root, dirs, names in os.walk(src):
            for n in names:
                a = os.path.join(root, n)
                rel = os.path.relpath(a, src)
                os.makedirs(os.path.dirname(rel) or ".", exist_ok=True)
                shutil.copyfile(a, rel)
                copied.append(rel)
    if kind in ("critic", "revise") and m is None:   # 견본이 없으면 "찾은 것 없음"으로 끝낸다(지적 0·고칠 것 0)
        if kind == "critic":
            open("findings.json", "w").write("[]\n"); copied.append("findings.json")
        ok = True
    rec["copied"] = sorted(copied)
    json.dump(rec, open(".fake-%s.json" % kind, "w"), ensure_ascii=False, indent=1)
    if _re.search(r"^\s*(?:- )?FAKE DRAFT_ERROR", prompt, _re.M) or not ok:
        res = {"type": "result", "subtype": "error_during_execution", "is_error": True, "total_cost_usd": 0}
    else:
        res = {"type": "result", "subtype": "success", "is_error": False, "result": "결과 파일 %d개를 썼습니다." % len(copied), "total_cost_usd": 0, "num_turns": 1, "session_id": "fake-draft"}
    sys.stdout.write(json.dumps(res, ensure_ascii=False) + "\n")
    sys.exit(0)

sid = args[args.index("--resume") + 1] if "--resume" in args else args[args.index("--session-id") + 1]
resume = "--resume" in args

out_lock = threading.Lock()
def out(o):
    with out_lock:
        sys.stdout.write(json.dumps(o, ensure_ascii=False) + "\n"); sys.stdout.flush()

# 표준 입력: 모든 줄을 원문 그대로 남긴다(엔진이 user 메시지 말고 무엇을 보내는지 보려고). 첫 줄은 본 흐름이 받는다
first = threading.Event()
closed = threading.Event()
lines = []
def reader():
    for raw in sys.stdin:
        log("fake-in.log", raw if raw.endswith("\n") else raw + "\n")
        lines.append(raw)
        first.set()
    log("fake-in.log", "### stdin closed %.1fs\n" % (time.time() - T0))
    closed.set(); first.set()
threading.Thread(target=reader, daemon=True).start()

# 신호
stop = threading.Event()
cfg = {"ignore_int": False, "ignore_term": False, "orphan": False}
children = []
def on_signal(sig, frame):
    name = signal.Signals(sig).name
    ignored = (sig == signal.SIGINT and cfg["ignore_int"]) or (sig == signal.SIGTERM and cfg["ignore_term"])
    log("fake-signals.log", "%s %.1fs run %d%s\n" % (name, time.time() - T0, runs, " ignored" if ignored else ""))
    if ignored:
        return
    if sig == signal.SIGINT:
        stop.set()
    else:
        os._exit(128 + sig)
signal.signal(signal.SIGINT, on_signal)
signal.signal(signal.SIGTERM, on_signal)

# 진짜 claude 는 세션을 열 때 샌드박스 밖에서 git status 를 부른다
try:
    subprocess.run(["git", "status", "--porcelain"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
except Exception:
    pass

first.wait()
msg = ""
if lines:
    try:
        o = json.loads(lines[0])
        c = o.get("message", {}).get("content", "")
        msg = c if isinstance(c, str) else "".join(b.get("text", "") for b in c if isinstance(b, dict))
    except Exception:
        msg = lines[0]

def fake_lines(text):
    got = []
    for l in text.splitlines():
        s = l.strip()
        while s.startswith("- "):
            s = s[2:].strip()
        if s.startswith("FAKE "):
            got.append(s[5:])
    return got

scen_path = os.path.join(UNIT, "fake-scenario.txt")
mine = fake_lines(msg)
if not resume and mine:
    open(scen_path, "w").write("\n".join(mine) + "\n")
scenario = [l for l in open(scen_path).read().splitlines() if l] if os.path.exists(scen_path) else []

def wait_closed():
    while not closed.is_set():
        closed.wait(0.2)

# ---- 1) 한도 흐름(옛 가짜 그대로) ----
if not scenario:
    out({"type": "system", "subtype": "init", "session_id": sid, "cwd": os.getcwd(), "model": "fake"})
    now = int(time.time())
    reset = now + 61
    win = {"five_hour": {"utilization": 0.03, "resetsAt": now + 3000}, "seven_day": {"utilization": 1, "resetsAt": reset}}
    if not resume:
        out({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": reset, "rateLimitType": "seven_day",
             "overageStatus": "rejected", "isUsingOverage": False, "unifiedWindows": win}, "session_id": sid})
        out({"type": "assistant", "message": {"role": "assistant", "model": "<synthetic>", "content": [{"type": "text", "text": "You've hit your weekly limit · resets 4pm (Asia/Seoul)"}]},
             "parent_tool_use_id": None, "session_id": sid, "error": "rate_limit", "is_api_error_message": True, "api_error_status": 429,
             "api_error": "usage_limit_reached", "api_error_params": {"rate_limit_info": {"status": "rejected", "resetsAt": reset, "rateLimitType": "seven_day", "unifiedWindows": win}}})
        out({"type": "result", "subtype": "success", "is_error": True, "num_turns": 3, "api_error_status": 429, "api_error": "usage_limit_reached",
             "terminal_reason": "api_error", "result": "You've hit your weekly limit · resets 4pm (Asia/Seoul)", "total_cost_usd": 0.01, "session_id": sid})
    else:
        out({"is_error": False, "duration_api_ms": 0, "num_turns": 0, "stop_reason": None, "session_id": sid, "total_cost_usd": 0.0,
             "subtype": "success", "result": "", "type": "result"})
        time.sleep(3)
        out({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "한도가 풀려 이어서 limit-ok.txt 를 만듭니다."}]}, "parent_tool_use_id": None, "session_id": sid})
        open("limit-ok.txt", "w").write("ok\n")
        out({"type": "result", "subtype": "success", "is_error": False, "num_turns": 2, "result": "만들었습니다", "total_cost_usd": 0.02, "session_id": sid})
    wait_closed()
    sys.exit(0)

# ---- 2) 줄대로 ----
for l in scenario:
    if l == "IGNORE_INT": cfg["ignore_int"] = True
    if l == "IGNORE_TERM": cfg["ignore_term"] = True
    if l == "ORPHAN": cfg["orphan"] = True
error = next((l.split()[1:] for l in scenario if l.startswith("ERROR ")), None)
if error and runs <= int(error[0]):
    kind = error[1] if len(error) > 1 else "exit:1"
    if kind.startswith("preinit:"):
        log("fake-signals.log", "preinit exit %s run %d\n" % (kind[8:], runs))
        sys.stdout.flush(); os._exit(int(kind[8:]))
    out({"type": "system", "subtype": "init", "session_id": sid, "cwd": os.getcwd(), "model": "fake"})
    if kind.startswith("exit:"):
        sys.stderr.write("fake claude: 일부러 오류로 끝냄(run %d, 코드 %s)\n" % (runs, kind[5:])); sys.stderr.flush()
        time.sleep(0.3)
        os._exit(int(kind[5:]))
    out({"type": "result", "subtype": "error_during_execution", "is_error": True, "num_turns": 1, "result": "API Error: 500 fake overloaded",
         "total_cost_usd": 0.001, "session_id": sid})
    wait_closed()
    sys.exit(0)

out({"type": "system", "subtype": "init", "session_id": sid, "cwd": os.getcwd(), "model": "fake"})
written, nexts, news, cost = [], [], [], 0.0
tickets = []   # FAKE TICKET(⑩-3)
custom = None   # FAKE RESULT 로 받은 결과 기록(견본)

def interrupted():
    # 진짜 claude 의 SIGINT 뒤 모양(진행 상황.md 1단계 실측): 도구 명령을 끝내고 error_during_execution result, 종료 코드 0
    if not cfg["orphan"]:
        for p in children:
            try: os.killpg(p.pid, signal.SIGTERM)
            except Exception: pass
    out({"type": "result", "subtype": "error_during_execution", "is_error": True, "num_turns": 1, "terminal_reason": "aborted_tools",
         "stop_reason": "tool_use", "result": "", "total_cost_usd": 0.002, "session_id": sid})
    time.sleep(0.3)
    sys.exit(0)

# 이번 차례에 따를 줄: 시나리오에서 아직 하지 않은 줄(fake-done.txt 에 한 줄 번호를 남긴다 — 오류·멈춤 뒤 이어 열면 남은 줄부터,
# 진짜 claude 가 멈춘 자리에서 잇는 것처럼) + 이어 열기면 이번 입력에 든 줄(사람이 보낸 말)
done_path = os.path.join(UNIT, "fake-done.txt")
done = set(open(done_path).read().split()) if os.path.exists(done_path) else set()
todo = [(str(i), l) for i, l in enumerate(scenario) if str(i) not in done] + ([("in", l) for l in mine] if resume else [])
def mark(k):
    if k != "in": log("fake-done.txt", k + "\n")
for k, l in todo:
    if stop.is_set(): interrupted()
    verb, _, rest = l.partition(" ")
    if verb not in ("SLEEP",): mark(k)
    if verb == "WRITE":
        path, _, text = rest.partition(" ")
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        open(path, "w").write(text + "\n"); written.append(path)
        out({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "%s 를 썼습니다." % path}]}, "parent_tool_use_id": None, "session_id": sid})
    elif verb == "COMMIT":
        if written:
            subprocess.run(["git", "add", "--"] + written, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["git", "commit", "-q", "-m", rest or "가짜 커밋"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif verb == "SLEEP":
        mark(k)   # 멈춰서 끊겨도 이어 열 때 다시 기다리지 않는다
        end = time.time() + float(rest)
        while time.time() < end:
            if stop.is_set(): interrupted()
            time.sleep(0.1)
    elif verb == "SH":
        subprocess.run(["/bin/sh", "-c", rest], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif verb == "CHILD":
        p = subprocess.Popen(["/usr/bin/python3", "-c", "import time; time.sleep(%s)" % rest], start_new_session=True)
        children.append(p)
        log("fake-child.pid", "%d\n" % p.pid)
        out({"type": "system", "subtype": "background_tasks_changed", "tasks": [], "session_id": sid})
    elif verb == "RESULT":
        try:
            custom = json.load(open(rest))
            open(os.path.join(UNIT, "fake-result.txt"), "w").write(rest)   # 판정을 다시 해 이어 열 때도 같은 견본을 쓴다
        except Exception as e:
            log("fake-errors.log", "RESULT %s: %s\n" % (rest, e))
    elif verb == "NEXT":
        t, _, w = rest.partition("|"); nexts.append({"title": t.strip(), "why": w.strip()})
    elif verb == "NEW":
        t, _, w = rest.partition("|"); news.append({"title": t.strip(), "why": w.strip()})
    elif verb == "TICKET":
        a = rest.split("|")
        tickets.append({"temp": a[0].strip(), "title": (a[1] if len(a) > 1 else "").strip(), "why": (a[2] if len(a) > 2 else "").strip()})
if stop.is_set(): interrupted()
res = {"unit": {"title": "가짜 단위"}, "outcome": "done", "headline": "가짜 claude 가 줄대로 했습니다.", "summary": ["줄 %d개" % len(todo)],
       "next_start": nexts, "new_work": news}
if tickets:
    res["new_tickets"] = tickets
if custom is None and os.path.exists(os.path.join(UNIT, "fake-result.txt")):
    try:
        custom = json.load(open(open(os.path.join(UNIT, "fake-result.txt")).read().strip()))
    except Exception:
        pass
if isinstance(custom, dict):
    res = custom   # 견본 결과 기록을 그대로(보고서 틀 시험, 3단계)
try:
    open(os.path.join(UNIT, "result.json"), "w").write(json.dumps(res, ensure_ascii=False))
except Exception:
    pass
out({"type": "result", "subtype": "success", "is_error": False, "num_turns": 2, "result": "끝냈습니다", "total_cost_usd": 0.01, "session_id": sid})
# 결과 뒤에도 SIGINT 를 받을 수 있게 입력이 닫힐 때까지 기다린다
while not closed.is_set():
    if stop.is_set(): interrupted()
    closed.wait(0.2)
sys.exit(0)
