#!/usr/bin/env python3
# 가짜 claude: 실제 stream 기록(2026-10-14 exec-25)의 한도 거절 모양을 흉내 낸다
#  새 세션(--session-id): 첫 입력을 받으면 rate_limit_event(rejected) → assistant(error rate_limit) → result(subtype success, is_error true, 429)
#  이어 열기(--resume): init → 빈 result(num_turns 0) → 3초 뒤 일을 하고(limit-ok.txt) 진짜 result
# 입력이 닫히면 끝난다. 받은 인자는 단위 폴더($CLAUDE_BOARD_UNIT_DIR)의 fake-args.log 에 남긴다
# 쓰는 법: 시험 실행 폴더의 run.json 에 "claude_path": "<이 파일 절대 경로>" — 진행 상황.md "시험 방법"
import json, os, sys, time
args = sys.argv[1:]
open(os.path.join(os.environ.get("CLAUDE_BOARD_UNIT_DIR", "."), "fake-args.log"), "a").write(json.dumps(args, ensure_ascii=False) + "\n")
sid = args[args.index("--resume") + 1] if "--resume" in args else args[args.index("--session-id") + 1]
resume = "--resume" in args
def out(o):
    sys.stdout.write(json.dumps(o, ensure_ascii=False) + "\n"); sys.stdout.flush()
line = sys.stdin.readline()
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
for _ in sys.stdin:
    pass
