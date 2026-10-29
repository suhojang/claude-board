# state.py 의 track_subagent 로 subagent-started-seen.json 을 만든다 — 시작 hook 을 본 서브에이전트만 started_seen 이 참이다
# 실행(windows/ 에서): python3 -I crates/core/tests/fixtures/gen-started-seen.py .. crates/core/tests/fixtures
import json, sys
repo, out = sys.argv[1], sys.argv[2]
sys.dont_write_bytecode = True  # 저장소에 __pycache__ 를 남기지 않는다
sys.path.insert(0, repo)
import state


def ev(name, aid, tool=None):
    d = {"session_id": "s", "hook_event_name": name, "agent_id": aid, "agent_type": "Explore"}
    if tool:
        d["tool_name"] = tool
    return d


# (hook 입력, hook 인자 state, 시각)
script = [
    (ev("SubagentStart", "seen"), "working", 100.0),
    (ev("PreToolUse", "seen", "Bash"), "working", 110.0),
    (ev("SubagentStop", "seen"), "working", 120.0),
    (ev("SubagentStop", "orphan_stop"), "working", 130.0),
    (ev("PreToolUse", "orphan_tool", "Read"), "working", 131.0),
    (ev("PermissionRequest", "orphan_perm", "Bash"), "waiting", 132.0),
    (ev("SubagentStart", "orphan_tool"), "working", 140.0),
]
rec = {}
steps = []
for data, st, now in script:
    state.track_subagent(rec, data, st, now)
    steps.append({"input": data, "state": st, "now": now, "subagents": json.loads(json.dumps(rec["subagents"]))})
json.dump({"source": "state.py track_subagent (3.2.0) 로 만든 기대값 — gen-started-seen.py", "steps": steps},
          open(out + "/subagent-started-seen.json", "w", encoding="utf-8"), ensure_ascii=False, indent=2)
