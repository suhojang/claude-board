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
#       FAKE RESULT <JSON 파일 절대 경로>    결과 기록을 그 파일의 객체로(견본 기록, 보고서 시험. NEXT·NEW 는 쓰지 않음)
#     SIGINT(엔진의 멈춤)를 받으면 진짜 claude 처럼(2026-10-20 실측) result subtype error_during_execution·is_error true·
#     terminal_reason aborted_tools 를 내고 종료 코드 0 으로 끝난다.
#
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
if stop.is_set(): interrupted()
res = {"unit": {"title": "가짜 단위"}, "outcome": "done", "headline": "가짜 claude 가 줄대로 했습니다.", "summary": ["줄 %d개" % len(todo)],
       "next_start": nexts, "new_work": news}
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
