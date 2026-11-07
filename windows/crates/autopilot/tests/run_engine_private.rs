//! 엔진 전용 단위 진행 파일(`private/<id>/unit-<n>.json`)과 모델이 쓸 수 있는 공개 사본(`units/<n>/engine.json`)의 경계 시험.
//! 엔진의 판단은 전용 파일만 보고, 공개 사본은 보드가 그리도록 매번 전용 파일에서 다시 덮어쓴다는 약속을 깨뜨려 본다.

mod fakes;

use claude_board_autopilot::engine::decide::RunState;
use claude_board_autopilot::engine::run::{RunExit, MODEL_FILE_MAX};
use claude_board_autopilot::ports::{RunFiles, SessionStart};
use fakes::*;
use serde_json::{json, Value};

const SID: &str = "5b0a1c7e-0a67-4f0e-9f5c-5d3f2b6a8e11";
const PRIVATE_UNIT1: &str = "/private/r1/unit-1.json";
const PUBLIC_UNIT1: &str = "units/1/engine.json";

fn engine_json(sid: &str) -> Value {
    json!({
        "unit": 1, "id": "u1", "title": "첫째", "status": "running", "phase": "model",
        "session_id": sid, "session_started": true, "sessions": [sid],
        "first_message": "처음 글", "next_message": "처음 글", "try": 0, "errors": 0,
        "started": NOW - 100.0, "active_sec": 50, "cost_usd": 0.2, "cost_done": 0.2, "turns": 3,
        "claude_runs": 1, "checks": [], "denials": [],
        "procs": {"claude_pid": 48250, "ctime": 123.5},
    })
}

/// 도는 단위 하나가 있는 실행. 전용 사본이 `private_text`(None 이면 파일 없음)
fn scenario_with_private(private_text: Option<&str>) -> Scenario {
    let mut s = Scenario::new(1);
    s.run["state"] = json!("running");
    s.run["unit"] = json!(1);
    s.queue[0]["status"] = json!("running");
    if let Some(t) = private_text {
        s.file(PRIVATE_UNIT1, t);
    }
    s
}

fn running_with_valid_private() -> Scenario {
    scenario_with_private(Some(&serde_json::to_string_pretty(&engine_json(SID)).unwrap()))
}

/// 글자 수가 정확히 `len` 바이트가 되게 JSON 뒤를 공백으로 채운다
fn padded_to(v: &Value, len: usize) -> String {
    let mut t = serde_json::to_string(v).unwrap();
    assert!(t.len() <= len);
    t.push_str(&" ".repeat(len - t.len()));
    t
}

// ── 전용 사본이 깨졌을 때 ──

/// 깨진 전용 사본으로는 시작하지 않고, 이름을 댄 LoadFailed 로 멈춘다(공개 사본·세션은 건드리지 않는다)
#[test]
fn unreadable_private_unit_file_is_load_failed_naming_unit_file() {
    let cases: [(&str, &str); 7] = [
        ("잘린 JSON", "{\"unit\": 1, \"status\": \"run"),
        ("빈 파일", ""),
        ("NUL 바이트", "\0\0\0\0"),
        ("배열", "[1, 2]"),
        ("글자 하나", "\"running\""),
        ("숫자", "42"),
        ("BOM 이 붙은 JSON", "\u{feff}{\"unit\": 1}"),
    ];
    for (label, text) in cases {
        let mut s = scenario_with_private(Some(text));
        // 공개 사본은 멀쩡하다 — 그래도 전용 사본이 깨졌으면 공개 사본으로 대신하지 않는다
        s.file(PUBLIC_UNIT1, &serde_json::to_string(&engine_json("public-session")).unwrap());
        let w = s.build();
        w.env.push_spawn(ChildScript::ok_turn(0.5));
        let exit = w.run();
        match &exit {
            RunExit::LoadFailed(why) => {
                assert!(why.contains("unit-1.json"), "{label}: 메시지가 전용 파일 이름을 대지 않는다: {why}");
                assert!(!why.contains("units/1"), "{label}: 공개 사본 이름을 댄다: {why}");
            }
            other => panic!("{label}: LoadFailed 여야 하는데 {other:?}"),
        }
        assert_eq!(w.env.spawn_count(), 0, "{label}: 깨진 상태로 claude 를 띄웠다");
        assert!(w.reaped.lock().unwrap().is_empty(), "{label}: 프로세스를 끝냈다");
        assert_eq!(w.files.text(PRIVATE_UNIT1).as_deref(), Some(text), "{label}: 깨진 파일을 덮어썼다");
        assert!(
            !w.files.ops().iter().any(|o| o == &format!("write:/runs/r1/{PUBLIC_UNIT1}")),
            "{label}: 공개 사본을 썼다: {:?}",
            w.files.ops()
        );
    }
}

/// 한도 경계: 정확히 MODEL_FILE_MAX 바이트는 읽고, 1바이트 넘으면 읽지 않고 멈춘다
#[test]
fn private_unit_file_size_limit_boundary() {
    let at_limit = padded_to(&engine_json(SID), MODEL_FILE_MAX as usize);
    let w = scenario_with_private(Some(&at_limit)).build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done), "한도 바로 위가 아닌 파일은 읽는다");
    assert_eq!(w.env.launches()[0].session, SessionStart::Resume(SID.to_string()));

    let over = padded_to(&engine_json(SID), MODEL_FILE_MAX as usize + 1);
    let w = scenario_with_private(Some(&over)).build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    match w.run() {
        RunExit::LoadFailed(why) => {
            assert!(why.contains("unit-1.json"), "{why}");
            assert!(why.contains(&MODEL_FILE_MAX.to_string()), "한도를 알려 준다: {why}");
        }
        other => panic!("한도를 넘었는데 {other:?}"),
    }
    assert_eq!(w.env.spawn_count(), 0);
    assert!(w.reaped.lock().unwrap().is_empty());
}

/// 도는 단위가 아닌 단위의 전용 사본이 깨져 있어도 실행은 간다 — 그 단위를 처음 값으로 열고 올바른 파일로 바꿔 쓴다
#[test]
fn corrupt_private_file_of_a_pending_unit_is_replaced_not_fatal() {
    let mut s = Scenario::new(2);
    s.file("/private/r1/unit-2.json", "{깨짐");
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let fixed = w.files.json("/private/r1/unit-2.json").expect("깨진 파일이 올바른 JSON 으로 바뀐다");
    assert_eq!(fixed["status"], "done");
    assert_eq!(fixed["unit"], 2);
    assert_eq!(w.queue_statuses(), ["done", "done"]);
}

// ── 전용 사본이 없을 때 ──

/// 전용 사본이 없으면(공개 사본만 남아 있어도) 단위를 처음 값으로 잇는다 — 공개 사본의 pid 를 끝내거나 세션을 잇지 않는다
#[test]
fn missing_private_file_ignores_public_copy_pid_and_session() {
    let mut s = scenario_with_private(None);
    let mut forged = engine_json("forged-session");
    forged["procs"] = json!({"claude_pid": 31337, "ctime": 9.0});
    s.file(PUBLIC_UNIT1, &serde_json::to_string(&forged).unwrap());
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert!(w.reaped.lock().unwrap().is_empty(), "공개 사본의 pid 를 끝냈다: {:?}", w.reaped.lock().unwrap());
    let launches = w.env.launches();
    assert_eq!(launches.len(), 1);
    assert!(
        !matches!(&launches[0].session, SessionStart::Resume(s) if s == "forged-session"),
        "공개 사본의 세션을 이었다: {:?}",
        launches[0].session
    );
    let private = w.files.json(PRIVATE_UNIT1).expect("엔진 전용 사본이 새로 생긴다");
    assert_ne!(private["session_id"], "forged-session");
    assert_eq!(private["status"], "done");
}

// ── 공개 사본은 늘 전용 사본에서 다시 덮어쓴다 ──

/// 시작 때 공개 사본이 없거나 쓰레기여도, 끝나면 공개 사본은 전용 사본과 같은 글이다
#[test]
fn public_copy_is_rewritten_from_private_state() {
    let huge = "x".repeat(MODEL_FILE_MAX as usize + 5);
    let cases: [(&str, Option<String>); 5] = [
        ("없음", None),
        ("쓰레기 글", Some("이건 JSON 이 아니다".to_string())),
        ("빈 파일", Some(String::new())),
        ("위조 JSON", Some(json!({"status": "done", "session_id": "x", "procs": {"claude_pid": 5}}).to_string())),
        ("한도 초과", Some(huge)),
    ];
    for (label, public) in cases {
        let mut s = running_with_valid_private();
        if let Some(t) = &public {
            s.file(PUBLIC_UNIT1, t);
        }
        let w = s.build();
        w.env.push_spawn(ChildScript::ok_turn(0.5));
        assert_eq!(w.run(), RunExit::Finished(RunState::Done), "{label}");
        let private = w.files.json(PRIVATE_UNIT1).expect("전용 사본");
        assert_eq!(private["status"], "done", "{label}");
        assert_eq!(w.files.json(PUBLIC_UNIT1), Some(private), "{label}: 공개 사본이 전용 사본과 다르다");
    }
}

/// 모델이 도는 도중(턴 한가운데) 공개 사본을 지우거나 바꿔도 엔진의 결정과 전용 사본의 결과는 달라지지 않는다
#[test]
fn model_tampering_public_copy_mid_turn_changes_no_decision() {
    type Tamper = Box<dyn FnOnce(&MemFiles) + Send>;
    fn run_with(tamper: Option<Tamper>) -> (Value, Vec<String>, Vec<String>, Value) {
        let w = running_with_valid_private().build();
        let mut steps = vec![Step::Line(init_line())];
        if let Some(f) = tamper {
            let files = w.files.clone();
            steps.push(Step::Hook(Box::new(move || f(&files))));
        }
        steps.push(Step::Line(assistant_text("일합니다")));
        steps.push(Step::Line(result_line(0.5)));
        w.env.push_spawn(ChildScript::new(steps));
        assert_eq!(w.run(), RunExit::Finished(RunState::Done));
        (
            w.files.json(PRIVATE_UNIT1).unwrap(),
            w.queue_statuses(),
            w.event_texts(),
            w.files.json(PUBLIC_UNIT1).unwrap(),
        )
    }
    let (base_private, base_queue, base_events, base_public) = run_with(None);
    assert_eq!(base_public, base_private);

    let tampers: Vec<(&str, Tamper)> = vec![
        ("지움", Box::new(|f| f.remove(std::path::Path::new("/runs/r1/units/1/engine.json")).unwrap())),
        ("쓰레기", Box::new(|f| f.put_text(PUBLIC_UNIT1, "\u{0}\u{0}garbage"))),
        (
            "위조: 끝난 것처럼 + 남의 pid",
            Box::new(|f| {
                f.put_json(
                    PUBLIC_UNIT1,
                    &json!({"status": "done", "phase": "done", "session_id": "evil", "procs": {"claude_pid": 31337, "ctime": 1.0}, "cost_usd": 0, "turns": 999}),
                )
            }),
        ),
        ("한도 초과", Box::new(|f| f.put_text(PUBLIC_UNIT1, &"y".repeat(MODEL_FILE_MAX as usize + 1)))),
    ];
    for (label, tamper) in tampers {
        let (private, queue, events, public) = run_with(Some(tamper));
        assert_eq!(private, base_private, "{label}: 전용 사본의 결과가 달라졌다");
        assert_eq!(queue, base_queue, "{label}");
        assert_eq!(events, base_events, "{label}: 엔진 사건이 달라졌다");
        assert_eq!(public, private, "{label}: 공개 사본이 다시 덮어써지지 않았다");
    }
}

/// 저장은 전용 사본을 먼저, 공개 사본을 그 뒤에 쓴다 — 그 사이에서 죽어도 판단의 근거(전용)가 더 앞선다
#[test]
fn save_writes_private_before_public() {
    let w = running_with_valid_private().build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let private_op = format!("write:{PRIVATE_UNIT1}");
    let public_op = format!("write:/runs/r1/{PUBLIC_UNIT1}");
    let ops = w.files.ops();
    let public_at: Vec<usize> = ops.iter().enumerate().filter(|(_, o)| **o == public_op).map(|(i, _)| i).collect();
    assert!(!public_at.is_empty(), "공개 사본을 한 번도 쓰지 않았다: {ops:?}");
    for i in public_at {
        assert!(i > 0 && ops[i - 1] == private_op, "공개 사본 쓰기({i}) 바로 앞이 전용 사본 쓰기가 아니다: {:?}", &ops[i.saturating_sub(2)..=i]);
    }
    assert_eq!(
        ops.iter().filter(|o| **o == private_op).count(),
        ops.iter().filter(|o| **o == public_op).count(),
        "두 사본을 쓴 횟수가 같아야 한다"
    );
}

// ── 직전 엔진의 claude 를 끝내는 판단은 전용 사본의 pid 만 본다 ──

#[test]
fn kill_leftovers_pid_boundaries() {
    // (claude_pid 값, 끝내야 하는가) — 0·1·음수는 자기 자신/시스템 프로세스일 수 있어 건드리지 않는다
    let cases: [(Value, bool); 5] = [
        (json!(0), false),
        (json!(1), false),
        (json!(2), true),
        (json!(-5), false),
        (Value::Null, false),
    ];
    for (pid, expect) in cases {
        let mut e = engine_json(SID);
        e["procs"] = json!({"claude_pid": pid, "ctime": 123.5});
        let w = scenario_with_private(Some(&serde_json::to_string_pretty(&e).unwrap())).build();
        w.env.push_spawn(ChildScript::ok_turn(0.5));
        assert_eq!(w.run(), RunExit::Finished(RunState::Done), "pid={pid}");
        let reaped = w.reaped.lock().unwrap().clone();
        assert_eq!(!reaped.is_empty(), expect, "pid={pid}: {reaped:?}");
        if expect {
            assert_eq!(reaped.len(), 1, "pid={pid}");
        }
    }
}

/// procs 가 아예 없거나 모양이 틀려도 죽지 않고 이어 간다
#[test]
fn kill_leftovers_tolerates_missing_or_malformed_procs() {
    for procs in [None, Some(json!("문자열")), Some(json!([1, 2])), Some(json!({"claude_pid": {"nested": 1}}))] {
        let mut e = engine_json(SID);
        match &procs {
            Some(p) => e["procs"] = p.clone(),
            None => {
                e.as_object_mut().unwrap().remove("procs");
            }
        }
        let w = scenario_with_private(Some(&serde_json::to_string_pretty(&e).unwrap())).build();
        w.env.push_spawn(ChildScript::ok_turn(0.5));
        assert_eq!(w.run(), RunExit::Finished(RunState::Done), "{procs:?}");
        assert!(w.reaped.lock().unwrap().is_empty(), "{procs:?}: {:?}", w.reaped.lock().unwrap());
    }
}
