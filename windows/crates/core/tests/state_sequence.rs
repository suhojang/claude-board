//! 기준 데이터 시험 — macOS `state.py` 에 hook 이벤트 14개를 차례로 넣은 REA 캡처와 [`state::apply`] 결과를 맞춘다.
//! 출처·가린 필드는 `tests/fixtures/state-sequence/README.md` 참고.

use std::fs;
use std::path::PathBuf;

use claude_board_core::state::{self, ClaudeProcess, Env};
use claude_board_core::Record;
use serde_json::{Map, Value};

struct Step {
    name: String,
    state: String,
    input: Record,
    expected: Record,
}

fn steps() -> Vec<Step> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/state-sequence");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".in.json").map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(names.len(), 14, "단계 14개");
    names
        .into_iter()
        .map(|n| {
            let input: Value = serde_json::from_str(&fs::read_to_string(dir.join(format!("{n}.in.json"))).unwrap()).unwrap();
            let expected: Value = serde_json::from_str(&fs::read_to_string(dir.join(format!("{n}.out.json"))).unwrap()).unwrap();
            Step {
                name: n,
                state: input["state"].as_str().unwrap().to_string(),
                input: input["input"].as_object().unwrap().clone(),
                expected: expected.as_object().unwrap().clone(),
            }
        })
        .collect()
}

/// 가짜 프로세스 정보 — 늘 같은 Claude 프로세스를 찾고, 그 프로세스는 살아 있다.
struct Fake(ClaudeProcess);

impl Env for Fake {
    fn find_claude(&self) -> Option<ClaudeProcess> {
        Some(self.0.clone())
    }
    fn alive(&self, pid: u32, _ctime: Option<f64>) -> bool {
        pid == self.0.pid
    }
    fn argv(&self, _pid: u32) -> Vec<String> {
        self.0.argv.clone()
    }
    fn subagent_meta(&self, _transcript: &str, _agent_id: &str) -> Record {
        Map::new()
    }
    fn cwd(&self) -> String {
        "/somewhere/else".into()
    }
    fn project_dir(&self) -> Option<String> {
        None
    }
}

/// 캡처한 기기의 프로세스·시계에서 온 필드를 지운다(값을 바꾸지 않고 키째 지운다 — pid_ctime 은 macOS 기록에 없다).
/// project_dir 은 캡처 뒤 state.py 에 더한 필드라 캡처에 없다 — replay 시험에서 따로 본다.
fn mask(rec: &Record) -> Record {
    let mut r = rec.clone();
    for k in ["pid", "pid_ctime", "tty", "headless", "args", "resume_cmd", "resume_args", "started", "updated", "since", "transcript_path", "project_dir"] {
        r.shift_remove(k);
    }
    if let Some(Value::Object(subs)) = r.get_mut("subagents") {
        for sub in subs.values_mut().filter_map(Value::as_object_mut) {
            // started_seen 은 캡처 뒤 state.py 에 더한 필드(시작 hook 을 본 표지)라 캡처에 없다
            for k in ["started", "updated", "ended", "started_seen"] {
                sub.shift_remove(k);
            }
        }
    }
    if let Some(Value::Object(hit)) = r.get_mut("limit_hit") {
        hit.shift_remove("at");
    }
    r
}

#[test]
fn masked_sequence_matches_state_py() {
    // Windows 처럼 생성 시각이 있고 tty 가 없는 가짜 프로세스 — 이런 차이는 가린 필드에만 나타나야 한다
    let env = Fake(ClaudeProcess {
        pid: 4242,
        ctime: Some(1_700_000_000.25),
        tty: None,
        headless: false,
        argv: vec!["C:\\Users\\u\\.local\\bin\\claude.exe".into(), "--model".into(), "claude-opus-5-5".into()],
    });
    let mut rec = Map::new();
    for (i, step) in steps().iter().enumerate() {
        let out = state::apply(rec, &step.state, &step.input, 1000.0 + i as f64, &env);
        assert_eq!(Value::Object(mask(&out.record)), Value::Object(mask(&step.expected)), "단계 {}", step.name);
        rec = out.record;
    }
}

#[test]
fn replay_with_captured_clock_and_process_is_exact() {
    // 캡처한 기기와 같은 프로세스 정보와, 단계마다 캡처의 updated 를 시계로 쓰면 가린 필드까지 모두 같다
    let env = Fake(ClaudeProcess {
        pid: 46137,
        ctime: None,
        tty: Some("ttys000".into()),
        headless: false,
        argv: vec!["claude".into(), "--allow-dangerously-skip-permissions".into()],
    });
    let topic_title = " · 윈도우 포팅 작업을 시작해 주세요 아…";
    let titles = [
        Some("🟢 demo-proj".to_string()),
        Some(format!("🔵 demo-proj{topic_title}")),
        None,
        None,
        None,
        Some(format!("🟠 demo-proj{topic_title}")),
        Some(format!("🔵 demo-proj{topic_title}")),
        None,
        Some(format!("🟠 demo-proj{topic_title}")),
        Some(format!("🟢 demo-proj{topic_title}")),
        None,
        None,
        Some(format!("🔵 demo-proj{topic_title}")),
        Some(format!("⚫ demo-proj{topic_title}")),
    ];
    let first_cwd = steps()[0].input["cwd"].clone();
    let mut rec = Map::new();
    for (i, step) in steps().iter().enumerate() {
        let now = step.expected["updated"].as_f64().unwrap();
        let out = state::apply(rec, &step.state, &step.input, now, &env);
        // resume_args 는 Windows 판이 더한 필드(macOS 기록에 없다) — 내용을 따로 보고 뺀 나머지를 비교한다
        let mut got = out.record.clone();
        let args = got.shift_remove("resume_args").expect("resume_args");
        assert_eq!(
            args,
            serde_json::json!(["--allow-dangerously-skip-permissions", "--permission-mode", "default", "--model", "claude-opus-5-5",
                               "--resume", "11111111-2222-3333-4444-555555555555"]),
            "resume_args — 단계 {}", step.name
        );
        // project_dir 은 캡처 뒤 state.py 에 더한 필드(캡처에 없다) — 환경 변수가 없으면 첫 hook 의 cwd 를 지킨다
        let dir = got.shift_remove("project_dir").expect("project_dir");
        assert_eq!(dir, first_cwd, "project_dir — 단계 {}", step.name);
        // started_seen 도 캡처 뒤에 더한 필드 — 이 시나리오의 서브에이전트는 시작 hook 을 거쳤으므로 참이다
        if let Some(Value::Object(subs)) = got.get_mut("subagents") {
            for sub in subs.values_mut().filter_map(Value::as_object_mut) {
                assert_eq!(sub.shift_remove("started_seen"), Some(Value::Bool(true)), "started_seen — 단계 {}", step.name);
            }
        }
        assert_eq!(Value::Object(got.clone()), Value::Object(step.expected.clone()), "단계 {}", step.name);
        assert_eq!(out.title, titles[i], "제목 — 단계 {}", step.name);
        assert_eq!(out.remove_paused, step.input["hook_event_name"] == "UserPromptSubmit", "단계 {}", step.name);
        // state.py 와 같은 키 순서(json.dump 는 삽입 순서를 지킨다)
        let keys: Vec<&String> = got.keys().collect();
        let want: Vec<&String> = step.expected.keys().collect();
        assert_eq!(keys, want, "키 순서 — 단계 {}", step.name);
        rec = out.record;
    }
}

#[test]
fn subagent_started_seen_matches_state_py() {
    // state.py 3.2.0 의 track_subagent 로 만든 기대값(gen-started-seen.py) — 필드 이름·값·키 순서가 같다
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/subagent-started-seen.json");
    let fixture: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let env = Fake(ClaudeProcess { pid: 4242, ctime: Some(1_700_000_000.25), tty: None, headless: false, argv: vec!["claude".into()] });
    let mut rec = Map::new();
    let steps = fixture["steps"].as_array().unwrap();
    assert!(steps.len() >= 7, "단계가 비었다");
    for (i, step) in steps.iter().enumerate() {
        let input = step["input"].as_object().unwrap();
        let out = state::apply(rec, step["state"].as_str().unwrap(), input, step["now"].as_f64().unwrap(), &env);
        let got = &out.record["subagents"];
        let want = &step["subagents"];
        assert_eq!(got, want, "단계 {i}");
        for (id, sub) in want.as_object().unwrap() {
            let got_keys: Vec<&String> = got[id].as_object().unwrap().keys().collect();
            let want_keys: Vec<&String> = sub.as_object().unwrap().keys().collect();
            assert_eq!(got_keys, want_keys, "키 순서 — 단계 {i} {id}");
        }
        rec = out.record;
    }
}
