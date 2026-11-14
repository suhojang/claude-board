//! 엔진 루프의 순수 부품(4.2a) 시험 — 파일 → 상태, 모델 글, 실행 기록 행. 포트를 부르지 않는다.

use std::collections::BTreeSet;
use std::path::Path;

use claude_board_autopilot::adapters::records_sqlite::SqliteRecords;
use claude_board_autopilot::engine::decide::*;
use claude_board_autopilot::engine::run::*;
use claude_board_autopilot::model::{FrozenFile, QueueFile, RunFile, UnitProgressFile};
use claude_board_autopilot::ports::{Records, Row};
use claude_board_autopilot::rules::records::RECORDS_SCHEMA_V1;
use claude_board_autopilot::rules::stopline::Tz;
use claude_board_core::paths;
use serde_json::{json, Value};

const NOW: f64 = 1_800_000_000.0;

// ── 도우미 ──

fn run_file(v: Value) -> RunFile {
    serde_json::from_value(v).expect("run.json 꼴")
}

fn queue_file(v: Value) -> QueueFile {
    serde_json::from_value(v).expect("queue.json 꼴")
}

fn frozen_file(v: Value) -> FrozenFile {
    serde_json::from_value(v).expect("frozen.json 꼴")
}

fn progress(v: Value) -> UnitProgressFile {
    serde_json::from_value(v).expect("engine.json 꼴")
}

fn two_units() -> Value {
    json!([
        {"id": "a", "title": "첫째", "check": "true", "cwd": "C:\\w", "prompt": "p1"},
        {"id": "b", "title": "둘째", "check": "test -f x", "precheck": "ping", "action": "make", "caps": {"usd": 3}}
    ])
}

fn pending_queue(n: usize) -> QueueFile {
    queue_file(Value::Array((0..n).map(|i| json!({"id": format!("u{i}"), "status": "pending"})).collect()))
}

/// 표 정의(RECORDS_SCHEMA_V1)에서 표 하나의 칸 이름
fn columns(table: &str) -> BTreeSet<String> {
    let head = format!("CREATE TABLE {table} (");
    let start = RECORDS_SCHEMA_V1.find(&head).expect("표 정의") + head.len();
    let (mut depth, mut end) = (1, start);
    for (i, c) in RECORDS_SCHEMA_V1[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let (mut parts, mut depth, mut cur) = (Vec::new(), 0, String::new());
    for c in RECORDS_SCHEMA_V1[start..end].chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.starts_with("PRIMARY"))
        .map(|p| p.split_whitespace().next().unwrap().to_string())
        .collect()
}

fn keys(r: &Row) -> BTreeSet<String> {
    r.keys().cloned().collect()
}

// ── load_state ──

#[test]
fn load_fresh_run() {
    let run = run_file(json!({"state": "starting", "model": "run-model", "reason": ""}));
    let queue = queue_file(json!([{"id": "a", "status": "pending"}, {"id": "b"}]));
    let frozen = frozen_file(json!({
        "model": "sonnet",
        "units": two_units(),
        "caps": {"minutes": 30},
        "check_retries": 3,
        "precheck_retry_min": 7,
        "stopline": {"five_hour": 0.9}
    }));
    let loaded = load_state(&run, &queue, &frozen, None, Tz::Utc).expect("읽힘");
    let s = &loaded.state;
    assert_eq!(s.run, RunState::Starting);
    assert_eq!(s.model, "sonnet");
    assert_eq!(s.statuses, vec![UnitStatus::Pending, UnitStatus::Pending]);
    assert_eq!(s.current, None);
    assert_eq!((s.retries, s.precheck_retry_min), (3, 7.0));
    assert_eq!(s.stopline.get("five_hour"), Some(&json!(0.9)));
    assert!(!s.restart_asked);
    assert_eq!(s.restart_unit, None);
    assert_eq!(
        s.units[0],
        UnitPlan {
            title: "첫째".into(),
            check: "true".into(),
            has_action: false,
            has_precheck: false,
            caps: Caps { minutes: 30.0, usd: 100.0, weekly_pp: 5.0 },
        }
    );
    assert_eq!(
        s.units[1],
        UnitPlan {
            title: "둘째".into(),
            check: "test -f x".into(),
            has_action: true,
            has_precheck: true,
            caps: Caps { minutes: 30.0, usd: 3.0, weekly_pp: 5.0 },
        }
    );
    assert_eq!(loaded.weekly, WeeklyMeter::default());
    assert_eq!(loaded.session_id, None);
}

#[test]
fn load_without_settings_uses_defaults_and_queue_units() {
    // run.state 가 없으면 Starting, frozen 에 units 가 없으면 queue 가 정의, 제목이 없으면 "단위 n"
    let run = run_file(json!({}));
    let queue = queue_file(json!([{"check": "c"}]));
    let loaded = load_state(&run, &queue, &frozen_file(json!({})), None, Tz::Utc).expect("읽힘");
    assert_eq!(loaded.state.run, RunState::Starting);
    assert_eq!(loaded.state.units[0].title, "단위 1");
    assert_eq!(loaded.state.units[0].check, "c");
    assert_eq!((loaded.state.retries, loaded.state.precheck_retry_min), (2, 5.0));
    assert_eq!(loaded.state.model, "");
}

#[test]
fn load_running_unit_restores_current() {
    let run = run_file(json!({"state": "waiting_limit", "reason": "쉬는 중", "unit": 2}));
    let queue = queue_file(json!([{"status": "done"}, {"status": "running"}, {"status": "pending"}]));
    let frozen = frozen_file(json!({"units": [{"title": "a"}, {"title": "b"}, {"title": "c"}]}));
    let e = progress(json!({
        "unit": 2, "phase": "limit_wait", "try": 1, "errors": 2, "limits": 3, "session_started": true,
        "session_id": "sid-1", "first_message": "첫 입력", "next_message": MSG_LIMIT, "wait_until": 1_800_000_500.5,
        "weekly_base": 0.5, "weekly_last": 0.55, "weekly_pp_done": 1.5, "weekly_pp": 2.0
    }));
    let loaded = load_state(&run, &queue, &frozen, Some(&e), Tz::Utc).expect("읽힘");
    let s = &loaded.state;
    assert_eq!(s.run, RunState::WaitingLimit);
    assert_eq!(s.reason, "쉬는 중");
    assert_eq!(s.statuses, vec![UnitStatus::Done, UnitStatus::Running, UnitStatus::Pending]);
    assert_eq!(
        s.current,
        Some(Current {
            n: 2,
            phase: UnitPhase::LimitWait,
            tries: 1,
            errors: 2,
            limits: 3,
            resume: true,
            next: Next::Limit,
            wait_until: Some(1_800_000_500.5),
        })
    );
    assert_eq!(s.inflight, Inflight::Idle);
    assert_eq!(loaded.weekly, WeeklyMeter { base: Some(0.5), last: Some(0.55), pp_done: 1.5, pp: 2.0 });
    assert_eq!(loaded.session_id.as_deref(), Some("sid-1"));
}

#[test]
fn load_running_unit_without_engine_json_resumes_with_restart_message() {
    let queue = queue_file(json!([{"status": "running"}]));
    let frozen = frozen_file(json!({"units": [{"title": "a", "action": "make"}]}));
    let loaded = load_state(&run_file(json!({"state": "running"})), &queue, &frozen, None, Tz::Utc).expect("읽힘");
    let c = loaded.state.current.expect("current");
    assert_eq!((c.n, c.phase, c.next.clone()), (1, UnitPhase::Action, Next::Restart));
}

#[test]
fn load_restart_flag() {
    let queue = pending_queue(2);
    let frozen = frozen_file(json!({}));
    let with = run_file(json!({"state": "running", "restart": {"asked": 1.5, "unit": 2, "plan": "after_unit"}}));
    let s = load_state(&with, &queue, &frozen, None, Tz::Utc).unwrap().state;
    assert!(s.restart_asked);
    assert_eq!(s.restart_unit, Some(2));

    let now_plan = run_file(json!({"state": "running", "restart": {"asked": 1.5, "unit": null, "plan": "now"}}));
    let s = load_state(&now_plan, &queue, &frozen, None, Tz::Utc).unwrap().state;
    assert!(s.restart_asked);
    assert_eq!(s.restart_unit, None);

    let without = run_file(json!({"state": "running"}));
    let s = load_state(&without, &queue, &frozen, None, Tz::Utc).unwrap().state;
    assert!(!s.restart_asked);
    assert_eq!(s.restart_unit, None);
}

#[test]
fn load_length_mismatch_errors() {
    let frozen = frozen_file(json!({"units": two_units()}));
    let err = load_state(&run_file(json!({})), &pending_queue(1), &frozen, None, Tz::Utc).unwrap_err();
    assert!(err.contains('2') && err.contains('1'), "{err}");
}

#[test]
fn load_zero_units_errors() {
    let err = load_state(&run_file(json!({})), &pending_queue(0), &frozen_file(json!({})), None, Tz::Utc).unwrap_err();
    assert!(err.contains("단위"), "{err}");
}

#[test]
fn load_two_running_errors() {
    let queue = queue_file(json!([{"status": "running"}, {"status": "running"}]));
    let err = load_state(&run_file(json!({"state": "running"})), &queue, &frozen_file(json!({})), None, Tz::Utc).unwrap_err();
    assert!(err.contains("둘 이상"), "{err}");
}

#[test]
fn load_unknown_run_state_errors() {
    let err = load_state(&run_file(json!({"state": "zzz"})), &pending_queue(1), &frozen_file(json!({})), None, Tz::Utc).unwrap_err();
    assert!(err.contains("zzz"), "{err}");
}

// ── next / caps / 해시 ──

#[test]
fn next_from_table() {
    let first = Some("FIRST 글");
    assert_eq!(next_from(Some(MSG_PAUSED), first), Next::Paused);
    assert_eq!(next_from(Some(MSG_LIMIT), first), Next::Limit);
    assert_eq!(next_from(Some(MSG_RESTART), first), Next::Restart);
    assert_eq!(next_from(Some(MSG_ERROR), first), Next::Error);
    assert_eq!(next_from(Some("FIRST 글"), first), Next::First);
    assert_eq!(next_from(Some(""), first), Next::First);
    assert_eq!(next_from(None, first), Next::First);
    assert_eq!(next_from(Some(""), None), Next::First);
    assert_eq!(next_from(Some("사람이 덧붙인 말"), first), Next::Text("사람이 덧붙인 말".into()));
    assert_eq!(next_from(Some(MSG_LIMIT), None), Next::Limit);
}

#[test]
fn next_text_roundtrip() {
    for next in [Next::First, Next::Restart, Next::Paused, Next::Limit, Next::Error, Next::Text("그 밖의 글".into())] {
        let text = next_text(&next, "FIRST");
        assert_eq!(next_from(Some(&text), Some("FIRST")), next, "{text}");
    }
    assert_eq!(next_text(&Next::First, "FIRST"), "FIRST");
    assert_eq!(next_text(&Next::Error, "FIRST"), MSG_ERROR);
}

#[test]
fn caps_of_table() {
    let d = Caps::default();
    assert_eq!(caps_of(&json!({}), &Value::Null), d);
    // 단위 > 실행 > 기본, 일부 칸만
    let unit = json!({"caps": {"minutes": 10}});
    let run = json!({"minutes": 20, "usd": 50});
    assert_eq!(caps_of(&unit, &run), Caps { minutes: 10.0, usd: 50.0, weekly_pp: d.weekly_pp });
    assert_eq!(caps_of(&json!({}), &run), Caps { minutes: 20.0, usd: 50.0, weekly_pp: d.weekly_pp });
    assert_eq!(caps_of(&json!({"caps": {"weekly_pp": 1.5, "usd": "7"}}), &run), Caps { minutes: 20.0, usd: 7.0, weekly_pp: 1.5 });
    // 숫자가 아닌 칸은 다음 층으로 넘어간다
    assert_eq!(caps_of(&json!({"caps": {"minutes": "많이"}}), &run).minutes, 20.0);
}

#[test]
fn frozen_digest_stable() {
    use sha2::{Digest, Sha256};
    let a: Value = serde_json::from_str(r#"{"a": 1, "b": [1, 2], "c": "한글"}"#).unwrap();
    let again: Value = serde_json::from_str("{\"a\":1,\"b\":[1,2],\"c\":\"한글\"}").unwrap();
    let swapped: Value = serde_json::from_str(r#"{"b": [1, 2], "a": 1, "c": "한글"}"#).unwrap();
    let d = frozen_digest(&a);
    assert_eq!(d, frozen_digest(&a));
    assert_eq!(d, frozen_digest(&again), "줄 띄움이 달라도 같은 글이다");
    assert_ne!(d, frozen_digest(&swapped), "키 순서가 다르면 다르다");
    assert_eq!(d.len(), 64);
    assert!(d.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    let text = format!("{}\n", serde_json::to_string_pretty(&a).unwrap());
    let want: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(d, want);
}

// ── starts.log ──

#[test]
fn starts_line_format() {
    assert_eq!(
        starts_line(1_800_000_000.9, "2027-01-15 08:00:00", true, 4242, "running", "C:\\bin\\claude-autopilot-engine.exe"),
        "1800000000\t2027-01-15 08:00:00\tabnormal\tpid=4242\tprev=running\texe=C:\\bin\\claude-autopilot-engine.exe"
    );
    assert_eq!(
        starts_line(5.0, "t", false, 1, "", "x"),
        "5\tt\tnormal\tpid=1\tprev=\texe=x"
    );
}

#[test]
fn abnormal_recent_window() {
    let now = NOW;
    let line = |t: f64, kind: &str| starts_line(t, "t", kind == "abnormal", 1, "running", "x");
    let log = [
        line(now - 601.0, "abnormal"), // 창 밖
        line(now - 600.0, "abnormal"), // 경계는 안
        line(now - 300.0, "normal"),   // normal 은 세지 않는다
        line(now - 10.0, "abnormal"),
        "깨진 줄".to_string(),
        "abc\tt\tabnormal\tpid=1".to_string(), // 시각이 숫자가 아님
        "1800000000".to_string(),             // 칸이 모자람
        String::new(),
        line(now, "abnormal"),
    ]
    .join("\n");
    assert_eq!(abnormal_recent(&log, now), 3);
    assert_eq!(abnormal_recent("", now), 0);
    // 이번 줄을 덧붙인 글을 넘기면 이번 줄도 센다
    let appended = format!("{log}\n{}\n", line(now + 5.0, "abnormal"));
    assert_eq!(abnormal_recent(&appended, now + 5.0), 3);
}

#[test]
fn is_abnormal_table() {
    assert!(!is_abnormal(""));
    assert!(!is_abnormal("starting"));
    for prev in ["running", "waiting_limit", "waiting_stopline", "waiting_precheck", "paused", "taken_over", "stopped", "done", "error"] {
        assert!(is_abnormal(prev), "{prev}");
    }
}

#[test]
fn session_uuid_shape() {
    let id = session_uuid(b"run-1:1:1800000000123456789:4242");
    let p: Vec<&str> = id.split('-').collect();
    assert_eq!(p.iter().map(|x| x.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12], "{id}");
    assert!(id.bytes().all(|b| b == b'-' || b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{id}");
    assert!(p[2].starts_with('4'), "{id}");
    assert!(matches!(p[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'), "{id}");
    assert_eq!(id, session_uuid(b"run-1:1:1800000000123456789:4242"));
    assert_ne!(id, session_uuid(b"run-1:2:1800000000123456789:4242"));
}

// ── 모델 글 ──

#[test]
fn system_prompt_verbatim() {
    let lines: Vec<&str> = SYSTEM_PROMPT.lines().collect();
    assert_eq!(lines.len(), 10);
    assert_eq!(lines[0], "이 세션은 Claude Board 자율주행 모드로 돈다.");
    assert_eq!(lines[9], "- 일을 마치면 $CLAUDE_BOARD_UNIT_DIR/result.json 을 쓰고 턴을 끝낸다.");
    assert!(!SYSTEM_PROMPT.contains('\r') && !SYSTEM_PROMPT.ends_with('\n'));
    let rf: Vec<&str> = RESULT_FORMAT.lines().collect();
    assert_eq!((rf.len(), rf[0], rf[15]), (16, "{", "}"));
    assert!(!RESULT_FORMAT.contains('\r') && !RESULT_FORMAT.ends_with('\n'));
    assert_eq!(serde_json::from_str::<Value>(RESULT_FORMAT).map(|v| v["outcome"].clone()).ok(), Some(json!("done|failed|parked|capped")));
}

/// 원본 파일이 있는 작업 트리에서는 두 상수가 원본 줄(603~612, 616~631)과 글자 그대로인지 직접 견준다
#[test]
fn prompt_constants_match_swift_source() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Sources/ClaudeAutopilot/main.swift");
    let swift = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n");
    let lines: Vec<&str> = swift.lines().collect();
    assert_eq!(lines[601], "let systemPromptText = \"\"\"");
    assert_eq!(lines[614], "let resultFormat = \"\"\"");
    assert_eq!(SYSTEM_PROMPT, lines[602..612].join("\n"));
    assert_eq!(RESULT_FORMAT, lines[615..631].join("\n"));
}

fn unit<'a>(roots: &'a [String]) -> PromptUnit<'a> {
    PromptUnit {
        prompt: "로그인 화면을 고친다.",
        title: "로그인 고치기",
        cwd: "C:\\work\\app",
        check: "npm test",
        unit_dir: "C:\\runs\\r1\\units\\1",
        write_roots: roots,
        work_branch: None,
        sandbox: false,
        answers: None,
    }
}

#[test]
fn first_message_native_and_wsl() {
    let no_roots: Vec<String> = Vec::new();
    // 네이티브(샌드박스 없음): 전체 글을 견준다
    let native = first_message(&unit(&no_roots));
    let want = format!(
        "로그인 화면을 고친다.\n\n---\n\
이 일은 Claude Board 자율주행의 작업 단위 하나입니다(「로그인 고치기」).\n\
- 작업 폴더: C:\\work\\app\n\
- 판정 명령: `npm test` (일을 마치면 엔진이 작업 폴더에서 돌려 종료 코드 0 이면 통과로 봅니다. 마치기 전에 직접 돌려 보세요.)\n\
- 이 단위에 쓸 Jira 티켓 번호는 없습니다. 새 티켓이 있어야 하는 부분은 하지 말고 next_start 에 적으세요.\n\
- 쓰기는 작업 폴더, 단위 폴더, 임시 폴더만 됩니다(이 실행은 샌드박스 없이 돕니다. 권한 규칙으로만 막습니다).\n\
- 일을 마치면 결과 기록을 `C:\\runs\\r1\\units\\1\\result.json` 에 아래 형식의 JSON 하나로 쓰세요. 사람이 읽는 글은 모두 쉬운 한국어로 쓰고, 내부 번호(티켓·결정 번호 등)는 문장에 쓰지 말고 refs 에만 둡니다. 비용은 엔진이 채우므로 쓰지 않습니다. 해당 없는 칸은 빈 배열이나 빈 글자로 둡니다.\n\n\
```json\n{RESULT_FORMAT}\n```\n"
    );
    assert_eq!(native, want);

    // WSL(샌드박스 있음): 환경 쪽 경로와 쓰기 범위 둘
    let roots = vec!["/mnt/c/work/lib".to_string(), "/mnt/c/work/docs".to_string()];
    let wsl = first_message(&PromptUnit {
        cwd: "/mnt/c/work/app",
        unit_dir: "/mnt/c/runs/r1/units/1",
        sandbox: true,
        ..unit(&roots)
    });
    assert!(wsl.contains("- 작업 폴더: /mnt/c/work/app\n"));
    assert!(wsl.contains(
        "- 쓰기는 작업 폴더와 /mnt/c/work/lib, /mnt/c/work/docs, 단위 폴더, 임시 폴더만 됩니다(샌드박스). 네트워크는 허용한 곳만 나갑니다.\n"
    ));
    assert!(wsl.contains("`/mnt/c/runs/r1/units/1/result.json`"));
    assert!(!wsl.contains("샌드박스 없이"));
    assert!(wsl.ends_with("}\n```\n"));
}

#[test]
fn first_message_with_answers_and_branch() {
    let roots: Vec<String> = Vec::new();
    let msg = first_message(&PromptUnit {
        answers: Some("C:\\runs\\r1\\answers.json"),
        work_branch: Some("autopilot/r1"),
        ..unit(&roots)
    });
    let at = |needle: &str| msg.find(needle).unwrap_or_else(|| panic!("없음: {needle}"));
    assert!(msg.contains(
        "- 출발 때 고른 답: C:\\runs\\r1\\answers.json (카드: C:\\runs\\r1\\cards.json). 따를 답이 있으면 따르고 decisions_applied 에 적으세요.\n"
    ));
    assert!(msg.contains("- 작업 가지: `autopilot/r1`. 커밋은 이 가지에만 합니다. 가지를 바꾸거나 다른 가지를 옮기지 않습니다(다른 가지 맞추기는 끝난 뒤 사람이 보드에서 합니다).\n"));
    // 차례: 티켓 없음 → 고른 답 → 쓰기 범위 → 작업 가지 → result.json 안내
    let order = [at("Jira 티켓 번호는 없습니다"), at("고른 답"), at("- 쓰기는"), at("- 작업 가지"), at("- 일을 마치면")];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

#[test]
fn first_message_claude_dir_bash_note() {
    let roots: Vec<String> = Vec::new();
    let under_claude = paths::claude_dir().join("notifier").join("autopilot").join("r1").join("units").join("1");
    let dir = under_claude.to_string_lossy().to_string();
    let msg = first_message(&PromptUnit { unit_dir: &dir, ..unit(&roots) });
    assert!(msg.contains(&format!("- 단위 폴더(`{dir}`)는 Write·Edit 도구로 쓸 수 없습니다.")));
    // 안내는 result.json 안내 앞에 온다
    assert!(msg.find("Write·Edit 도구로 쓸 수 없습니다").unwrap() < msg.find("- 일을 마치면").unwrap());
    // 그 밖의 폴더에는 없다
    assert!(!first_message(&unit(&roots)).contains("Write·Edit"));
}

#[test]
fn is_under_table() {
    assert!(is_under("C:\\Users\\a\\.claude\\x", "C:\\Users\\a\\.claude"));
    assert!(is_under("c:/users/A/.CLAUDE/x/y", "C:\\Users\\a\\.claude\\"));
    assert!(!is_under("C:\\Users\\a\\.claude", "C:\\Users\\a\\.claude"), "폴더 자신은 아래가 아니다");
    assert!(!is_under("C:\\Users\\a\\.claude2\\x", "C:\\Users\\a\\.claude"));
    assert!(!is_under("D:\\Users\\a\\.claude\\x", "C:\\Users\\a\\.claude"));
}

// ── 실행 기록 행 ──

fn sample_run() -> (RunFile, QueueFile, FrozenFile, Vec<Option<UnitProgressFile>>) {
    let run = run_file(json!({
        "state": "done", "reason": "끝", "started": 1_799_999_000, "ended": 1_799_999_900.25, "dev": true,
        "title": "run 제목", "project": "run-project", "model": "run-model",
        "branches": {"repos": {"/r/b": {"on": "x/y"}, "/r/a": {}}}, "epic": "EP-1", "source": {"kind": "topic"},
        "worktree": {"mode": "in_place"}
    }));
    let queue = queue_file(json!([
        {"id": "a", "status": "done"}, {"id": "b", "status": "failed"}, {"id": "c", "status": "capped"},
        {"id": "d", "status": "stopped"}, {"id": "e"}
    ]));
    let frozen = frozen_file(json!({"title": "고정 제목", "model": "sonnet", "effort": "high", "project": "proj"}));
    let units = vec![
        Some(progress(json!({"active_sec": 10.5, "cost_usd": 0.25, "weekly_pp": 1.0}))),
        None,
        Some(progress(json!({"active_sec": 20, "cost_usd": 0.5, "weekly_pp": 0.5}))),
    ];
    (run, queue, frozen, units)
}

#[test]
fn run_row_shape() {
    let (run, queue, frozen, units) = sample_run();
    let row = run_row("r1", "C:\\runs\\r1", &run, &queue, &frozen, &units, NOW);
    // 표 정의의 칸을 하나도 빠뜨리거나 더하지 않는다
    assert_eq!(keys(&row), columns("runs"));
    assert_eq!(row["id"], "r1");
    assert_eq!(row["project_id"], "proj", "고정 설정이 run.json 보다 앞선다");
    assert_eq!(row["title"], "고정 제목");
    assert_eq!(row["model"], "sonnet");
    assert_eq!(row["effort"], "high");
    assert_eq!(row["state"], "done");
    assert_eq!(row["reason"], "끝");
    assert_eq!(row["dev"], 1);
    assert_eq!((row["started"].as_f64(), row["ended"].as_f64()), (Some(1_799_999_000.0), Some(1_799_999_900.25)));
    assert_eq!(
        [&row["units_total"], &row["units_done"], &row["units_failed"], &row["units_capped"], &row["units_stopped"]],
        [&json!(5), &json!(1), &json!(1), &json!(1), &json!(1)]
    );
    assert_eq!(row["active_sec"].as_f64(), Some(30.5));
    assert_eq!(row["cost_usd"].as_f64(), Some(0.75));
    assert_eq!(row["weekly_pp"].as_f64(), Some(1.5));
    // JSON 칸은 키를 정렬한 한 줄 글, 글 칸은 그대로
    assert_eq!(row["branches"], r#"{"repos":{"/r/a":{},"/r/b":{"on":"x/y"}}}"#);
    assert_eq!(row["worktree"], r#"{"mode":"in_place"}"#);
    assert_eq!(row["source"], r#"{"kind":"topic"}"#);
    assert_eq!(row["epic"], "EP-1");
    assert_eq!(row["continues"], Value::Null);
    assert_eq!(row["harness"], Value::Null);
    assert_eq!(row["set_id"], Value::Null);
    assert_eq!(row["run_dir"], "C:\\runs\\r1");
    assert_eq!(row["origin"], "engine");
    assert_eq!(row["updated"].as_f64(), Some(NOW));
}

#[test]
fn run_row_falls_back_to_run_json_and_nulls() {
    let run = run_file(json!({"title": "t", "project": "p", "model": "m"}));
    let row = run_row("r2", "d", &run, &pending_queue(1), &frozen_file(json!({})), &[], NOW);
    assert_eq!(keys(&row), columns("runs"));
    assert_eq!((&row["title"], &row["project_id"], &row["model"]), (&json!("t"), &json!("p"), &json!("m")));
    assert_eq!((&row["state"], &row["dev"], &row["started"], &row["effort"]), (&Value::Null, &Value::Null, &Value::Null, &Value::Null));
    assert_eq!(row["active_sec"].as_f64(), Some(0.0));
}

#[test]
fn unit_row_shape() {
    let queue = queue_file(json!([{
        "id": "u1", "title": "첫 단위", "source_key": "K-1", "branch": "def-branch", "est_min": 30, "est_basis": "비슷한 일"
    }]));
    let e = progress(json!({
        "branch": "eng-branch", "active_sec": 12.5, "cost_usd": 0.125, "weekly_pp": 0.5, "turns": 7, "try": 2,
        "checks": [{"try": 0, "exit": 1}, {"try": 1, "exit": 0}], "denials": [{"tool_name": "Bash"}],
        "started": 1_799_999_000, "ended": 1_799_999_500.5
    }));
    let result = r#"{"unit": {"kind": "fix"}, "outcome": "done"}"#;
    let row = unit_row("r1", 3, &queue[0], "done", Some(&e), Some(result), Some("sonnet"));
    assert_eq!(keys(&row), columns("units"));
    assert_eq!((&row["run_id"], &row["n"], &row["unit_id"], &row["title"]), (&json!("r1"), &json!(3), &json!("u1"), &json!("첫 단위")));
    assert_eq!(row["kind"], "fix", "정의에 없으면 결과 기록의 unit.kind");
    assert_eq!((&row["source_key"], &row["story_key"], &row["task_key"], &row["merged"]), (&json!("K-1"), &Value::Null, &Value::Null, &Value::Null));
    assert_eq!(row["branch"], "eng-branch");
    assert_eq!(row["status"], "done");
    assert_eq!((row["est_min"].as_f64(), &row["est_basis"]), (Some(30.0), &json!("비슷한 일")));
    assert_eq!((row["active_sec"].as_f64(), row["cost_usd"].as_f64(), row["weekly_pp"].as_f64()), (Some(12.5), Some(0.125), Some(0.5)));
    assert_eq!((&row["turns"], &row["tries"], &row["check_exit"]), (&json!(7), &json!(2), &json!(0)));
    assert_eq!(row["denials"], r#"[{"tool_name":"Bash"}]"#);
    assert_eq!(row["result"], result, "result.json 글 그대로");
    assert_eq!(row["model"], "sonnet");
    assert_eq!((row["started"].as_f64(), row["ended"].as_f64()), (Some(1_799_999_000.0), Some(1_799_999_500.5)));

    // 정의의 가지·종류가 우선하고, engine.json 도 결과 기록도 없으면 비운다
    let def = queue_file(json!([{"id": "u2", "kind": "dev", "branch": "def-branch"}]));
    let bare = unit_row("r1", 1, &def[0], "pending", None, None, None);
    assert_eq!(keys(&bare), columns("units"));
    assert_eq!((&bare["kind"], &bare["branch"]), (&json!("dev"), &json!("def-branch")));
    for k in ["active_sec", "cost_usd", "weekly_pp", "turns", "tries", "check_exit", "denials", "result", "model", "started", "ended", "est_min"] {
        assert_eq!(bare[k], Value::Null, "{k}");
    }
}

#[test]
fn project_row_created_only_new() {
    let repos = vec!["C:\\r\\a".to_string(), "C:\\r\\b".to_string()];
    let fresh = project_row("proj", Some("C:\\r"), &repos, false, NOW);
    assert_eq!(keys(&fresh), columns("projects"));
    assert_eq!(fresh["created"].as_f64(), Some(NOW));
    assert_eq!(fresh["updated"].as_f64(), Some(NOW));
    assert_eq!((&fresh["id"], &fresh["name"], &fresh["root"], &fresh["kind"]), (&json!("proj"), &json!("proj"), &json!("C:\\r"), &json!("bundle")));
    assert_eq!(fresh["repos"], r#"["C:\\r\\a","C:\\r\\b"]"#);

    let old = project_row("proj", None, &repos[..1], true, NOW);
    assert!(!old.contains_key("created"));
    assert_eq!(old["kind"], "single");
    assert_eq!(old["root"], Value::Null);
    assert!(keys(&old).is_subset(&columns("projects")));
}

#[test]
fn rows_accepted_by_sqlite() {
    // 어댑터는 표 정의 밖의 칸과 빈 기본키를 거절한다 — 만든 행이 그대로 들어가는지 본다
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("records.db")).expect("저장소 열기");
    let (run, queue, frozen, units) = sample_run();
    let defs = queue_file(json!([{"id": "a", "title": "t"}]));
    db.upsert("projects", &project_row("proj", Some("C:\\r"), &["C:\\r".to_string()], false, NOW)).unwrap();
    db.upsert("projects", &project_row("proj", Some("C:\\r"), &["C:\\r".to_string()], true, NOW + 60.0)).unwrap();
    db.upsert("runs", &run_row("r1", "C:\\runs\\r1", &run, &queue, &frozen, &units, NOW)).unwrap();
    db.upsert("units", &unit_row("r1", 1, &defs[0], "done", units[0].as_ref(), Some("{}"), Some("sonnet"))).unwrap();

    let p = db.select("SELECT created, updated FROM projects WHERE id=?", &[json!("proj")]).unwrap();
    assert_eq!((p[0]["created"].as_f64(), p[0]["updated"].as_f64()), (Some(NOW), Some(NOW + 60.0)));
    let r = db.select("SELECT units_done, cost_usd, origin FROM runs WHERE id=?", &[json!("r1")]).unwrap();
    assert_eq!((r[0]["units_done"].as_i64(), r[0]["cost_usd"].as_f64(), r[0]["origin"].as_str()), (Some(1), Some(0.75), Some("engine")));
    let u = db.select("SELECT unit_id, status, active_sec FROM units WHERE run_id=? AND n=1", &[json!("r1")]).unwrap();
    assert_eq!((u[0]["unit_id"].as_str(), u[0]["status"].as_str(), u[0]["active_sec"].as_f64()), (Some("a"), Some("done"), Some(10.5)));
}

#[test]
fn normalize_root_forms() {
    for (raw, want) in [
        (r"C:\repo", r"C:\repo"),
        (r"c:\repo", r"C:\repo"),
        (r"C:\repo\", r"C:\repo"),
        (r"C:\repo\sub", r"C:\repo\sub"),
        ("C:/repo/sub/", r"C:\repo\sub"),
        (r"\\?\C:\repo", r"C:\repo"),
        (r"\\?\UNC\srv\share\x", r"\\srv\share\x"),
        (r"C:\", r"C:\"),
        ("C:", r"C:\"),
        (r"\\srv\share\", r"\\srv\share"),
        ("/w/p/", "/w/p/"),
    ] {
        assert_eq!(normalize_root(raw), want, "{raw}");
    }
    assert_eq!(root_key(r"C:\Work\Repo"), root_key("c:/work/repo/"));
    assert_ne!(root_key(r"C:\work\repo"), root_key(r"C:\work\repo2"));
    assert_ne!(root_key(r"C:\a"), root_key(r"D:\a"));
}

#[test]
fn claim_root_rules() {
    let others = vec![("x".to_string(), r"c:\repo\".to_string())];
    // 다른 프로젝트가 갖고 있으면 이 프로젝트의 이전 root(없으면 None)를 둔다
    assert_eq!(claim_root(Some(r"C:\Repo"), None, &others), None);
    assert_eq!(claim_root(Some(r"C:\Repo"), Some(r"C:\old"), &others), Some(r"C:\old".to_string()));
    // 비어 있으면 정리한 글로 차지한다
    assert_eq!(claim_root(Some("C:/new/"), None, &others), Some(r"C:\new".to_string()));
    // 정하지 않았으면 이전 root 를 잇는다
    assert_eq!(claim_root(None, Some(r"C:\old"), &others), Some(r"C:\old".to_string()));
    assert_eq!(claim_root(None, None, &others), None);
}
