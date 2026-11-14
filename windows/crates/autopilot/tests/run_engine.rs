//! 엔진 루프(4.2b) 시험 — 가짜 포트(`tests/fakes`)만으로 열기·턴·판정·control·대기·기록·끝을 돌린다.
//! 가짜 시계가 시각을 밀므로 실제로 기다리지 않는다. 진짜 claude·셸·git·SQLite·작업 스케줄러는 이 시험에 없다.

mod fakes;

use std::time::Duration;

use claude_board_autopilot::engine::decide::{RunState, MSG_ERROR, MSG_LIMIT, MSG_PAUSED, MSG_RESTART};
use claude_board_autopilot::engine::run::{RunExit, FROZEN_CHANGED_REASON, SYSTEM_PROMPT};
use claude_board_autopilot::ports::{ClaudeLaunch, SessionStart};
use claude_board_autopilot::rules::checks::{check_fail_event, check_fail_message, check_fail_reason};
use fakes::*;
use serde_json::{json, Value};

// ── 도우미 ──

/// 단위 하나, 정상 턴 한 번, 판정 통과
fn one_unit() -> World {
    let w = Scenario::new(1).build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w
}

fn sid_of(l: &ClaudeLaunch) -> String {
    match &l.session {
        SessionStart::New(s) | SessionStart::Resume(s) => s.clone(),
    }
}

/// 보낸 첫 줄({"type":"user",...})의 글
fn content_of(line: &str) -> String {
    let v: Value = serde_json::from_str(line).expect("보낸 줄은 JSON");
    assert_eq!(v["type"], "user");
    assert_eq!(v["message"]["role"], "user");
    v["message"]["content"].as_str().expect("글").to_string()
}

/// `needles` 가 이 차례로(사이에 다른 줄이 있어도) 나온다
fn assert_in_order(texts: &[String], needles: &[&str]) {
    let mut from = 0;
    for n in needles {
        match texts[from..].iter().position(|t| t.contains(n)) {
            Some(i) => from += i + 1,
            None => panic!("`{n}` 이(가) 차례대로 나오지 않는다.\n앞에서 {from}줄을 지났고, 줄 전체:\n{}", texts.join("\n")),
        }
    }
}

fn count(texts: &[String], part: &str) -> usize {
    texts.iter().filter(|t| t.contains(part)).count()
}

fn starts_line(ts: f64, kind: &str) -> String {
    format!("{}\t2026-01-01 00:00:00\t{kind}\tpid=1\tprev=running\texe=x", ts as i64)
}

/// 도는 단위 하나(모델 단계, 세션이 열린 적 있음)가 있는 실행 — 엔진이 다시 떴을 때의 파일들
fn running_scenario(sid: &str) -> Scenario {
    let mut s = Scenario::new(1);
    s.run["state"] = json!("running");
    s.run["unit"] = json!(1);
    s.queue[0]["status"] = json!("running");
    let engine = json!({
        "unit": 1, "id": "u1", "title": "첫째", "status": "running", "phase": "model",
        "session_id": sid, "session_started": true, "sessions": [sid],
        "first_message": "처음 글", "next_message": "처음 글", "try": 0, "errors": 0,
        "started": NOW - 100.0, "active_sec": 50, "cost_usd": 0.2, "cost_done": 0.2, "turns": 3,
        "claude_runs": 1, "checks": [], "denials": [],
        "procs": {"claude_pid": 48250, "ctime": 123.5},
    });
    s.file("/private/r1/unit-1.json", &serde_json::to_string_pretty(&engine).unwrap());
    s
}

const SID: &str = "5b0a1c7e-0a67-4f0e-9f5c-5d3f2b6a8e11";

fn with_branches(s: &mut Scenario) {
    s.run["branches"] = json!({"repos": {"C:\\repo": {
        "original": "main", "work": "autopilot/r1", "on": "autopilot/r1", "dirty_start": [], "restored": null
    }}});
}

// ── 열기 ──

#[test]
fn open_lock_held_writes_nothing() {
    let w = Scenario::new(1).build();
    w.files.hold_lock(".engine.lock");
    assert_eq!(w.open().err(), Some(RunExit::LockHeld));
    assert!(w.files.ops().is_empty(), "잠금을 못 잡았는데 쓴 것이 있다: {:?}", w.files.ops());
    assert!(w.supervisor.unregistered.lock().unwrap().is_empty());
}

#[test]
fn open_frozen_changed_errors_and_unregisters() {
    let mut w = Scenario::new(1).build();
    w.frozen_hash = "0".repeat(64);
    assert_eq!(w.open().err(), Some(RunExit::FrozenChanged));
    let run = w.run_json();
    assert_eq!(run["state"], "error");
    assert_eq!(run["reason"], "고정 설정(pinned/frozen.json)이 출발 뒤에 바뀌어 멈춥니다.");
    assert_eq!(run["reason"], FROZEN_CHANGED_REASON);
    assert!(run["ended"].is_number());
    assert!(w.has_event("고정 설정(pinned/frozen.json)이 출발 뒤에 바뀌어 멈춥니다."));
    assert_eq!(*w.supervisor.unregistered.lock().unwrap(), vec!["lbl-r1".to_string()]);
    assert!(!w.files.exists("starts.log"), "해시가 틀리면 시작 줄도 남기지 않는다");
}

#[test]
fn open_appends_starts_line_normal_first() {
    let w = Scenario::new(1).build();
    let e = w.open().expect("열림");
    let lines = w.files.lines("starts.log");
    assert_eq!(lines.len(), 1);
    let f: Vec<&str> = lines[0].split('\t').collect();
    assert_eq!(f.len(), 6, "{lines:?}");
    assert_eq!(f[0], "1800000000");
    assert_eq!(f[2], "normal");
    assert_eq!(f[3], "pid=77");
    assert_eq!(f[4], "prev=starting");
    assert_eq!(f[5], "exe=claude-autopilot-engine.exe");
    assert_eq!(e.abnormal_recent(), 0);
    assert!(w.files.ops().contains(&"mkdir:/private/r1".to_string()), "엔진 전용 폴더를 만든다");
}

#[test]
fn open_abnormal_counts_window() {
    let mut s = Scenario::new(1);
    s.run["state"] = json!("running");
    // 600초 밖 · 600초 안 · 정상 · 깨진 줄: 이번 시작(비정상)과 600초 안의 비정상 하나만 센다
    let old = starts_line(NOW - 700.0, "abnormal");
    let recent = starts_line(NOW - 100.0, "abnormal");
    let normal = starts_line(NOW - 50.0, "normal");
    s.file("starts.log", &format!("{old}\n{recent}\n{normal}\n깨진 줄\n"));
    let w = s.build();
    let e = w.open().expect("열림");
    assert_eq!(e.abnormal_recent(), 2);
    let last = w.files.lines("starts.log").pop().unwrap();
    assert!(last.contains("\tabnormal\t") && last.contains("prev=running"), "{last}");
}

// ── 턴 ──

#[test]
fn turn_writes_first_message_line() {
    let w = one_unit();
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let launches = w.env.launches();
    assert_eq!(launches.len(), 1);
    let sid = match &launches[0].session {
        SessionStart::New(s) => s.clone(),
        other => panic!("처음엔 새 세션이어야 한다: {other:?}"),
    };
    assert_eq!(sid.len(), 36);
    assert_eq!(sid.matches('-').count(), 4);
    let sent = w.env.sent(0);
    assert_eq!(sent.len(), 1, "첫 줄 하나만 보낸다");
    let text = content_of(&sent[0]);
    let unit = w.unit_json(1);
    assert_eq!(unit["first_message"], text.as_str());
    assert!(text.starts_with("p1\n\n---\n"), "{text}");
    assert!(text.contains("- 작업 폴더: C:\\work"));
    assert_eq!(unit["session_id"], sid.as_str());
    assert_eq!(unit["claude_runs"], 1);
    // 시작 파일들
    assert_eq!(w.files.json("/private/r1/settings-1.json").unwrap()["scripted"], true);
    let prompt = w.files.text("/private/r1/system-prompt.md").expect("시스템 프롬프트 파일");
    assert!(prompt.starts_with(SYSTEM_PROMPT.lines().next().unwrap()));
    // 띄우기 인자
    let l = &launches[0];
    assert_eq!(l.model, "m1");
    assert_eq!(l.cwd, std::path::PathBuf::from("C:\\work"));
    assert!(l.env.contains(&("CLAUDE_BOARD_AUTOPILOT".to_string(), "r1".to_string())));
    assert!(l.env.iter().any(|(k, v)| k == "CLAUDE_BOARD_UNIT_DIR" && v.replace('\\', "/").ends_with("units/1")));
    assert!(l.env.iter().any(|(k, _)| k == "CLAUDE_BOARD_AUTOPILOT_DIR"));
    assert!(l.extra_args.windows(2).any(|p| p == ["--permission-mode", "dontAsk"]));
    // 쓰기 범위: 작업 폴더와 단위 폴더는 허용, 상태 파일은 거부
    let policy = w.env.policies.lock().unwrap()[0].clone();
    assert!(policy.allow_write.iter().any(|p| p == std::path::Path::new("C:\\work")));
    assert!(policy.deny_write.iter().any(|p| p.to_string_lossy().replace('\\', "/").ends_with("r1/run.json")));
    assert!(policy.deny_read.iter().any(|p| p.to_string_lossy().replace('\\', "/") == "/private/r1"));
}

#[test]
fn turn_stream_to_events_and_context() {
    let w = Scenario::new(1).build();
    w.env.push_spawn(ChildScript::new(vec![
        Step::Line(init_line()),
        Step::Line(assistant_text("안녕")),
        Step::Line(assistant_tool("t1", "Read", json!({"file_path": "a.txt"}))),
        Step::Line(tool_result("t1", "내용")),
        Step::Line(assistant_tool("t2", "Bash", json!({"command": "ls"}))),
        Step::Line(tool_result("t2", "ok")),
        Step::Line(result_line(0.1)),
    ]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(&texts, &["안녕", "파일 1곳을 살펴봤습니다."]);
    assert_eq!(count(&texts, "파일 1곳을 살펴봤습니다."), 1, "읽기·찾기는 모아 한 줄로");
    assert!(w.events().iter().any(|(who, t)| who == "model" && t == "안녕"));

    let ctx = w.files.json_lines("context.jsonl");
    assert_eq!(ctx.len(), 4, "{ctx:?}");
    assert_eq!(ctx[0]["kind"], "tool_use");
    assert_eq!(ctx[0]["tool"], "Read");
    assert_eq!(ctx[0]["id"], "t1");
    assert_eq!(ctx[0]["agent"], false);
    assert_eq!(ctx[0]["unit"], 1);
    assert!(ctx[0]["input"].as_str().unwrap().contains("a.txt"));
    assert!(ctx[0]["ts"].is_number());
    assert_eq!(ctx[1]["kind"], "tool_result");
    assert_eq!(ctx[1]["id"], "t1");
    assert_eq!(ctx[1]["is_error"], false);
    assert_eq!(ctx[1]["output"], "내용");
    assert_eq!(ctx[2]["tool"], "Bash");
    // 받은 줄은 그대로 stream.jsonl 에
    let stream = w.files.lines("units/1/stream.jsonl");
    assert_eq!(stream.len(), 7);
    assert!(stream[0].contains("\"init\"") && stream[0].contains(&sid_of(&w.env.launches()[0])));
}

#[test]
fn turn_session_started_saved_once() {
    let w = Scenario::new(1).build();
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line()), Step::Line(init_line()), Step::Line(result_line(0.1))]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let sid = sid_of(&w.env.launches()[0]);
    let unit = w.unit_json(1);
    assert_eq!(unit["session_started"], true);
    assert_eq!(unit["sessions"], json!([sid]));
    assert_eq!(w.run_json()["sessions"], json!([sid]), "run.json sessions 에 한 번만");
}

#[test]
fn turn_denial_logged() {
    let w = Scenario::new(1).build();
    let result = json!({
        "type": "result", "subtype": "success", "is_error": false, "num_turns": 1, "result": "ok", "total_cost_usd": 0.1,
        "session_id": "{SID}",
        "permission_denials": [{"tool_name": "Bash", "tool_input": {"command": "rm -rf x"}}],
    })
    .to_string();
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line()), Step::Line(result)]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let events = w.events();
    let hit = events.iter().find(|(_, t)| t.contains("허용 범위 밖이라 거부했습니다: Bash")).expect("거부 줄");
    assert_eq!(hit.0, "board");
    assert!(hit.1.contains("rm -rf x"), "{}", hit.1);
    let denials = w.unit_json(1)["denials"].clone();
    assert_eq!(denials.as_array().unwrap().len(), 1);
    assert_eq!(denials[0]["tool"], "Bash");
}

#[test]
fn turn_usage_from_stream() {
    let w = Scenario::new(1).build();
    let rate = json!({"type": "rate_limit_event", "rate_limit_info": {"unifiedWindows": {
        "five_hour": {"utilization": 0.42, "resetsAt": NOW + 500.0},
        "seven_day": {"utilization": 0.33, "resetsAt": NOW + 9000.0},
    }}})
    .to_string();
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line()), Step::Line(rate), Step::Line(result_line(0.1))]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let u = w.run_json()["usage"].clone();
    assert_eq!(u["from"], "stream");
    assert_eq!(u["five_hour"].as_f64(), Some(0.42));
    assert_eq!(u["seven_day"].as_f64(), Some(0.33));
    assert_eq!(u["seven_day_resets"].as_f64(), Some(NOW + 9000.0));
    assert!(u["at"].as_f64().unwrap() >= NOW);
}

#[test]
fn turn_done_closes_stdin() {
    let w = one_unit();
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(w.env.child_events(0), vec!["close_stdin"]);
    assert_eq!(w.queue_statuses(), vec!["done"]);
}

#[test]
fn turn_kill_when_not_exiting() {
    let w = Scenario::new(1).build();
    let mut script = ChildScript::ok_turn(0.1);
    script.on_close = None;
    w.env.push_spawn(script);
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(w.env.child_events(0), vec!["close_stdin", "kill_tree"]);
    // 결과가 있었으니 판정까지 간다
    assert_eq!(w.queue_statuses(), vec!["done"]);
    assert!(w.clock.get() - NOW >= 10.0, "입력을 닫고 10초는 기다린다");
}

#[test]
fn turn_crash_no_result_stderr_tail() {
    let mut s = Scenario::new(1);
    s.file("units/1/stderr.log", "old\n");
    let w = s.build();
    let mut crash = ChildScript::new(vec![Step::Line(init_line()), Step::Exit(3)]);
    crash.stderr = "fatal: x".to_string();
    w.env.push_spawn(crash);
    w.env.push_spawn(ChildScript::ok_turn(0.1));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let events = w.event_texts();
    let hit = events.iter().find(|t| t.contains("claude 가 오류로 끝났습니다")).expect("오류 줄");
    assert!(hit.contains("결과 없이 끝남, 종료 코드 3: fatal: x"), "{hit}");
    assert!(!hit.contains("old"), "앞 띄우기가 남긴 글은 넣지 않는다: {hit}");
    assert!(hit.contains("1분 뒤 같은 세션으로 다시 엽니다(1/3)"));
    // 1분 뒤 같은 세션으로
    let launches = w.env.launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].session, SessionStart::Resume(sid_of(&launches[0])));
    assert_eq!(content_of(&w.env.sent(1)[0]), MSG_ERROR);
    assert!(w.clock.get() - NOW >= 60.0);
}

#[test]
fn turn_tick_minutes_cap_interrupts() {
    let mut s = Scenario::new(1);
    s.unit(1, "caps", json!({"minutes": 1}));
    let w = s.build();
    w.git.reply("status --porcelain --ignore-submodules=all", 0, " M a.txt\n");
    w.git.reply("diff --stat --ignore-submodules=all", 0, " a.txt | 1 +\n");
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line())]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(w.env.child_events(0), vec!["interrupt"]);
    assert_eq!(w.queue_statuses(), vec!["capped"]);
    let texts = w.event_texts();
    assert_in_order(&texts, &["claude 를 멈춥니다.", "시간 상한에 닿아 멈췄습니다."]);
    let unit = w.unit_json(1);
    assert_eq!(unit["status"], "capped");
    assert_eq!(unit["git"]["status"], " M a.txt\n");
    assert_eq!(unit["git"]["diff_stat"], " a.txt | 1 +\n");
    assert!(unit["active_sec"].as_f64().unwrap() >= 60.0);
    assert!(w.clock.get() - NOW >= 60.0);
}

#[test]
fn turn_pause_interrupts_then_paused() {
    let w = Scenario::new(1).build();
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line())]));
    w.control_at(NOW + 3.0, "pause", Some(NOW + 3.0));
    w.control_at(NOW + 200.0, "stop", Some(NOW + 200.0));
    assert_eq!(w.run(), RunExit::Finished(RunState::Stopped));
    assert_eq!(w.env.child_events(0), vec!["interrupt"]);
    let texts = w.event_texts();
    assert_in_order(&texts, &["일시정지 요청을 받았습니다.", "claude 를 멈춥니다.", "일시정지했습니다.", "중단했습니다"]);
    let unit = w.unit_json(1);
    assert_eq!(unit["next_message"], MSG_PAUSED, "재개하면 이 말로 같은 세션을 잇는다");
    assert_eq!(unit["session_started"], true);
    assert_eq!(w.queue_statuses(), vec!["stopped"]);
    let seen: Vec<String> = w.journal.all().into_iter().filter(|e| e.starts_with("run:")).collect();
    assert!(seen.contains(&"run:paused:-:-".to_string()), "일시정지 상태가 run.json 에 한 번은 적힌다: {seen:?}");
    let run = w.run_json();
    assert_eq!(run["control_last"], "stop");
}

/// 진짜 claude 는 interrupt 에 `result`(error_during_execution)로 답하고도 입력을 기다리며 산다.
/// 엔진이 그 `result` 를 보고 입력을 닫아야 곧바로 끝난다(안 그러면 20초를 기다렸다가 자손째 끝낸다)
#[test]
fn turn_pause_closes_input_when_claude_answers_the_interrupt() {
    let w = Scenario::new(1).build();
    let mut script = ChildScript::new(vec![Step::Line(init_line())]);
    script.on_interrupt = None;
    script.on_close = Some(0);
    script.lines_on_interrupt = vec![json!({
        "type": "result", "subtype": "error_during_execution", "is_error": true, "num_turns": 1,
        "result": "", "terminal_reason": "aborted_streaming", "total_cost_usd": 0.01, "session_id": "{SID}"
    })
    .to_string()];
    w.env.push_spawn(script);
    w.control_at(NOW + 3.0, "pause", Some(NOW + 3.0));
    w.control_at(NOW + 200.0, "stop", Some(NOW + 200.0));
    assert_eq!(w.run(), RunExit::Finished(RunState::Stopped));
    assert_eq!(w.env.child_events(0), vec!["interrupt", "close_stdin"], "result 를 읽고 입력을 닫아 스스로 끝나게 한다(자손째 끝내지 않는다)");
    let texts = w.event_texts();
    assert_in_order(&texts, &["일시정지 요청을 받았습니다.", "claude 를 멈춥니다.", "일시정지했습니다.", "중단했습니다"]);
    assert_eq!(w.unit_json(1)["next_message"], MSG_PAUSED);
}

#[test]
fn turn_clear_background() {
    let mut s = Scenario::new(1);
    s.frozen["background_quiet_sec"] = json!(10);
    let w = s.build();
    let bg = json!({"type": "system", "subtype": "background_tasks_changed", "tasks": [{"id": "b1"}]}).to_string();
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line()), Step::Line(bg), Step::Line(result_line(0.1))]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert!(w.has_event("결과를 보낸 뒤 백그라운드 작업만 남아 10초 동안 새 출력이 없어, 입력을 닫아 판정으로 넘어갑니다."));
    // 입력을 닫은 것은 두 번: 백그라운드 정리, 그리고 턴 끝맺음
    assert_eq!(w.env.child_events(0), vec!["close_stdin", "close_stdin"]);
    assert_eq!(w.queue_statuses(), vec!["done"]);
}

// ── 판정 ──

#[test]
fn check_evidence_files_and_engine_checks() {
    let mut s = Scenario::new(1);
    s.file("units/1/evidence/check-0/old.txt", "옛것");
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w.env.push_check(CheckScript::exit(0, "ok"));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert!(!w.files.exists("units/1/evidence/check-0/old.txt"), "증거 폴더는 비우고 새로 쓴다");
    let summary = w.files.json("units/1/evidence/check-0/summary.json").expect("summary.json");
    assert_eq!(summary["k"], 0);
    assert_eq!(summary["exit"], 0);
    assert_eq!(summary["timed_out"], false);
    assert_eq!(summary["command"], "true");
    assert_eq!(summary["env"], "pwsh");
    assert!(summary["at"].is_number() && summary["sec"].is_number());

    let checks = w.unit_json(1)["checks"].clone();
    assert_eq!(checks.as_array().unwrap().len(), 1);
    let c = &checks[0];
    assert_eq!(c["try"], 0);
    assert_eq!(c["exit"], 0);
    assert_eq!(c["tail"], "ok");
    assert_eq!(c["interrupted"], false);
    assert_eq!(c["timed_out"], false);
    assert_eq!(c["evidence"], "evidence/check-0");
    assert_eq!(c["files"], json!([]));
    assert_eq!(c["sandbox"], false);

    let run = &w.env.check_runs()[0];
    assert_eq!(run.command, "true");
    assert_eq!(run.cwd, std::path::PathBuf::from("C:\\work"));
    assert_eq!(run.timeout, Duration::from_secs(600), "check_timeout_min 기본 10분");
    assert!(run.output_log.to_string_lossy().replace('\\', "/").ends_with("units/1/evidence/check-0/output.log"));
    for name in ["CLAUDE_BOARD_AUTOPILOT", "CLAUDE_BOARD_AUTOPILOT_DIR", "CLAUDE_BOARD_UNIT_DIR", "CLAUDE_BOARD_EVIDENCE_DIR"] {
        assert!(run.env.iter().any(|(k, _)| k == name), "{name}");
    }
    let mut p = run.path_env.clone();
    p.sort();
    assert_eq!(p, ["CLAUDE_BOARD_AUTOPILOT_DIR", "CLAUDE_BOARD_EVIDENCE_DIR", "CLAUDE_BOARD_UNIT_DIR"]);
    let texts = w.event_texts();
    assert_in_order(&texts, &["판정 명령을 돌립니다.", "판정 통과(", "단위 1/1 통과: 판정 명령을 통과했습니다."]);
}

#[test]
fn check_fail_then_retry_message() {
    let w = Scenario::new(1).build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w.env.push_spawn(ChildScript::ok_turn(0.25));
    w.env.push_check(CheckScript::exit(1, "1 failing"));
    w.env.push_check(CheckScript::pass());
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(
        &texts,
        &[
            "판정 명령을 돌립니다.",
            &check_fail_event(1, 0, 2, None),
            "판정 명령을 돌립니다(다시 1번째).",
            "판정 통과(",
        ],
    );
    assert!(texts.contains(&"판정 실패(종료 코드 1). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(1/2).".to_string()));
    let launches = w.env.launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].session, SessionStart::Resume(sid_of(&launches[0])));
    assert_eq!(content_of(&w.env.sent(1)[0]), check_fail_message(1, "true", "1 failing", None));
    let unit = w.unit_json(1);
    assert_eq!(unit["try"], 1);
    assert_eq!(unit["checks"].as_array().unwrap().len(), 2);
    assert_eq!(unit["checks"][0]["exit"], 1);
    assert_eq!(unit["checks"][1]["try"], 1);
    assert!(w.files.exists("units/1/evidence/check-1/summary.json"));
    assert_eq!(w.queue_statuses(), vec!["done"]);
}

#[test]
fn check_timed_out_maps() {
    let mut s = Scenario::new(1);
    s.frozen["check_retries"] = json!(0);
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w.env.push_check(CheckScript::timed_out());
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let unit = w.unit_json(1);
    assert_eq!(unit["checks"][0]["timed_out"], true);
    assert_eq!(unit["checks"][0]["exit"], -1);
    assert_eq!(unit["status"], "failed");
    assert_eq!(unit["reason"], check_fail_reason(0, Some(600.0)).as_str());
    assert!(unit["reason"].as_str().unwrap().contains("시간 상한 10분"));
    assert_eq!(w.queue_statuses(), vec!["failed"]);
}

#[test]
fn check_pause_applies_after_end() {
    let mut w = Scenario::new(1).build();
    // 판정 중에 control 을 가져가야 판정이 끝나게 되어 있다 — 본 스레드가 tick 마다 control 을 본다는 증거
    w.tick = Duration::from_millis(20);
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w.env.push_check(CheckScript::pass().with_hook(w.control_during_check("pause")));
    w.control_at(NOW + 300.0, "stop", Some(NOW + 300.0));
    assert_eq!(w.run(), RunExit::Finished(RunState::Stopped));
    let texts = w.event_texts();
    assert_in_order(&texts, &["일시정지 요청을 받았습니다.", "판정이 끝나면 멈춥니다.", "판정 통과(", "일시정지했습니다."]);
    assert_eq!(count(&texts, "판정이 끝나면 멈춥니다."), 1);
    assert!(!texts.iter().any(|t| t.contains("단위 1/1 통과")), "일시정지가 먼저 적용된다");
    // 판정 결과는 증거에 남는다
    assert!(w.files.exists("units/1/evidence/check-0/summary.json"));
    assert_eq!(w.queue_statuses(), vec!["stopped"]);
}

#[test]
fn precheck_fail_waits_then_retries() {
    let mut s = Scenario::new(1);
    s.unit(1, "precheck", json!("ping"));
    let w = s.build();
    w.env.push_check(CheckScript::exit(1, "no server\n"));
    w.env.push_check(CheckScript::pass());
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(
        &texts,
        &[
            "단위 1/1 의 시험 환경 확인이 실패해(종료 코드 1) 시작하지 않고",
            "시험 환경 확인을 통과해 단위를 시작합니다.",
            "단위 1/1 시작: 첫째",
        ],
    );
    assert!(texts.iter().any(|t| t.contains("에 다시 확인합니다: no server")), "{texts:?}");
    let runs = w.env.check_runs();
    assert_eq!(runs[0].command, "ping");
    assert_eq!(runs[1].command, "ping");
    assert_eq!(runs[0].timeout, Duration::from_secs(120));
    assert!(runs[0].output_log.to_string_lossy().replace('\\', "/").ends_with("private/r1/precheck-1.log"));
    assert!(runs[0].env.iter().any(|(k, v)| k == "CLAUDE_BOARD_PRIVATE_DIR" && v.replace('\\', "/") == "/private/r1"));
    assert!(!runs[0].env.iter().any(|(k, _)| k == "CLAUDE_BOARD_EVIDENCE_DIR"));
    assert_eq!(runs[2].command, "true", "시작한 뒤 판정");
    assert!(w.clock.get() - NOW >= 300.0, "5분 뒤 다시 확인한다");
    let states: Vec<String> = w.journal.all().into_iter().filter(|e| e.starts_with("run:")).collect();
    assert!(states.contains(&"run:waiting_precheck:-:-".to_string()), "{states:?}");
}

#[test]
fn action_ok_then_claude() {
    let mut s = Scenario::new(1);
    s.unit(1, "action", json!("do it"));
    let w = s.build();
    w.env.push_check(CheckScript::pass()); // 고정 단계
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let runs = w.env.check_runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].command, "do it");
    assert!(runs[0].output_log.to_string_lossy().replace('\\', "/").ends_with("units/1/action.log"));
    assert_eq!(runs[0].timeout, Duration::from_secs(120 * 60));
    assert_eq!(runs[1].command, "true");
    let unit = w.unit_json(1);
    assert_eq!(unit["action_result"]["exit"], 0);
    let first = unit["first_message"].as_str().unwrap();
    assert!(first.contains("고정 단계(action)"), "모델이 고정 단계가 돌았음을 안다");
    assert_eq!(content_of(&w.env.sent(0)[0]), first);
    assert_eq!(w.queue_statuses(), vec!["done"]);
}

#[test]
fn action_fail_fails_unit() {
    let mut s = Scenario::new(1);
    s.unit(1, "action", json!("do it"));
    let w = s.build();
    w.env.push_check(CheckScript::exit(2, "bad"));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(w.env.spawn_count(), 0, "고정 단계가 실패하면 모델을 띄우지 않는다");
    let unit = w.unit_json(1);
    assert_eq!(unit["status"], "failed");
    assert_eq!(unit["reason"], "고정 단계가 실패했습니다(종료 코드 2).");
    assert_eq!(w.queue_statuses(), vec!["failed"]);
}

// ── control ──

/// 사용률을 모르는 채 시작해 멈출 선 대기(10분)에 들어간 실행 — control 을 받을 틈이 길다
fn stopline_waiting() -> Scenario {
    let mut s = Scenario::new(1);
    s.run.as_object_mut().unwrap().remove("usage");
    s
}

#[test]
fn control_new_consumed_and_file_removed() {
    let w = stopline_waiting().build();
    w.usage.set(None);
    w.control_at(NOW + 30.0, "stop", Some(NOW + 30.0));
    assert_eq!(w.run(), RunExit::Finished(RunState::Stopped));
    assert!(!w.files.exists("control.json"), "받은 요청 파일은 지운다");
    let run = w.run_json();
    assert_eq!(run["control_seen"].as_f64(), Some(NOW + 30.0));
    assert_eq!(run["control_last"], "stop");
    assert_eq!(run["state"], "stopped");
    let texts = w.event_texts();
    assert_in_order(&texts, &["중단 요청을 받았습니다.", "사람이 중단해 실행을 마칩니다."]);
    assert_eq!(w.env.spawn_count(), 0);
}

#[test]
fn control_old_ts_ignored_file_removed() {
    let mut s = stopline_waiting();
    s.run["control_seen"] = json!(NOW + 100.0);
    s.run["control_last"] = json!("pause");
    let w = s.build();
    w.usage.set(None);
    let usage = w.usage.clone();
    w.clock.at(NOW + 60.0, move || usage.set(Some((0.1, 0.1))));
    w.control_at(NOW + 50.0, "stop", Some(NOW + 50.0)); // 이미 받은 ts 보다 옛것
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert!(!w.files.exists("control.json"), "옛 요청도 파일은 지운다");
    let run = w.run_json();
    assert_eq!(run["control_seen"].as_f64(), Some(NOW + 100.0));
    assert_eq!(run["control_last"], "pause", "옛 요청은 기록하지 않는다");
    assert!(!w.has_event("중단 요청을 받았습니다."));
    assert_eq!(w.queue_statuses(), vec!["done"]);
}

#[test]
fn control_mtime_when_no_ts() {
    let w = stopline_waiting().build();
    w.usage.set(None);
    w.control_at(NOW + 30.0, "stop", None);
    assert_eq!(w.run(), RunExit::Finished(RunState::Stopped));
    // ts 가 없으면 파일 수정 시각(가짜 시계)이 ts 다
    assert_eq!(w.run_json()["control_seen"].as_f64(), Some(NOW + 30.0));
    assert!(!w.files.exists("control.json"));
}

#[test]
fn control_resume_from_paused() {
    let mut s = Scenario::new(1);
    s.run["state"] = json!("paused");
    s.run["reason"] = json!("사람이 일시정지했습니다. 재개하면 이어 갑니다.");
    let w = s.build();
    w.control_at(NOW + 30.0, "resume", Some(NOW + 30.0));
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(
        &texts,
        &[
            "엔진이 다시 떴습니다. 사람이 일시정지한 실행이라 그대로 쉽니다.",
            "재개합니다.",
            "단위 1/1 시작: 첫째",
            "단위 1/1 통과",
        ],
    );
    assert!(w.clock.get() - NOW >= 30.0);
    assert_eq!(w.run_json()["control_last"], "resume");
}

// ── 대기 ──

#[test]
fn limit_wait_elapses_on_fake_clock() {
    let w = Scenario::new(1).build();
    let limited = json!({
        "type": "result", "subtype": "success", "is_error": true, "api_error_status": 429,
        "num_turns": 1, "result": "limit", "session_id": "{SID}",
    })
    .to_string();
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line()), Step::Line(limited)]));
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(&texts, &["사용량 한도에 막혔습니다.", "한도 대기를 마치고 같은 세션으로 이어 갑니다.", "단위 1/1 통과"]);
    assert!(texts.iter().any(|t| t.contains("(초기화 시각을 몰라 30분 뒤)")));
    // 30분 + 2분을 가짜 시계로 건넜다
    assert!(w.clock.get() - NOW >= 1920.0, "{}", w.clock.get() - NOW);
    let launches = w.env.launches();
    assert_eq!(launches[1].session, SessionStart::Resume(sid_of(&launches[0])));
    assert_eq!(content_of(&w.env.sent(1)[0]), MSG_LIMIT);
    assert_eq!(w.unit_json(1)["limits"], 1);
    let states: Vec<String> = w.journal.all().into_iter().filter(|e| e.starts_with("run:")).collect();
    assert!(states.contains(&"run:waiting_limit:-:-".to_string()), "{states:?}");
}

#[test]
fn usage_fallback_when_stale() {
    // 낡은 사용률(15분 넘음) → 포트로 다시 읽어 run.json 에 적는다
    let mut s = Scenario::new(1);
    s.run["usage"]["at"] = json!(NOW - 1000.0);
    let w = s.build();
    w.usage.set(Some((0.2, 0.3)));
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert!(w.usage.calls() >= 1);
    let u = w.run_json()["usage"].clone();
    assert_eq!(u["from"], "cache");
    assert_eq!(u["seven_day"].as_f64(), Some(0.3));
    assert_eq!(u["five_hour"].as_f64(), Some(0.2));

    // 방금 읽은 값이면 부르지 않는다
    let fresh = one_unit();
    assert_eq!(fresh.run(), RunExit::Finished(RunState::Done));
    assert_eq!(fresh.usage.calls(), 0);
}

#[test]
fn stopline_wait_then_running() {
    let mut s = Scenario::new(1);
    s.run["usage"]["seven_day"] = json!(0.9);
    s.run["usage"]["seven_day_resets"] = json!(NOW + 300.0);
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(&texts, &["계정 주간 사용률 90% 가 멈출 선 80% 에 닿아", "단위 1/1 시작: 첫째", "단위 1/1 통과"]);
    // 초기화 시각 + 2분까지 기다린 뒤에야 시작한다
    assert!(w.clock.get() - NOW >= 420.0, "{}", w.clock.get() - NOW);
    let states: Vec<String> = w.journal.all().into_iter().filter(|e| e.starts_with("run:")).collect();
    assert_in_order(
        &states,
        &["run:waiting_stopline:-:-", "run:running:-:-", "run:done:ended"],
    );
}

// ── 기록 · 끝 ──

#[test]
fn records_rows_after_unit() {
    let mut s = Scenario::new(1);
    s.run["project"] = json!("toy");
    s.run["project_root"] = json!("C:\\proj");
    s.file("units/1/result.json", "{\"outcome\":\"done\"}");
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    // 출발 · 단위 판정 뒤 · 끝
    let runs = w.records.rows("runs");
    assert_eq!(runs.len(), 3, "{runs:?}");
    let last = runs.last().unwrap();
    assert_eq!(last["id"], "r1");
    assert_eq!(last["project_id"], "toy");
    assert_eq!(last["state"], "done");
    assert_eq!(last["units_total"], 1);
    assert_eq!(last["units_done"], 1);
    assert_eq!(last["origin"], "engine");
    assert_eq!(last["run_dir"], "/runs/r1");
    let units = w.records.rows("units");
    assert_eq!(units.len(), 3);
    let u = units.last().unwrap();
    assert_eq!(u["unit_id"], "u1");
    assert_eq!(u["status"], "done");
    assert_eq!(u["check_exit"], 0);
    assert_eq!(u["tries"], 0);
    assert_eq!(u["result"], "{\"outcome\":\"done\"}");
    assert_eq!(u["model"], "m1");
    let projects = w.records.rows("projects");
    assert_eq!(projects.len(), 3);
    assert_eq!(projects[0]["root"], "C:\\proj");
    assert_eq!(projects[0]["kind"], "single");
    assert_eq!(projects[0]["repos"], "[\"C:\\\\work\"]");
    assert!(projects[0].contains_key("created"), "처음 만드는 행에는 created 가 있다");
    // 한 번의 기록은 projects → runs → units 차례
    let j = w.journal.all();
    let recs: Vec<&String> = j.iter().filter(|e| e.starts_with("records:")).collect();
    assert_eq!(recs[..3], ["records:projects", "records:runs", "records:units"]);
}

#[test]
fn records_error_then_recovered() {
    let w = one_unit();
    w.records.set_fail(Some("db locked"));
    let records = w.records.clone();
    w.clock.at(NOW + 0.5, move || records.set_fail(None));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let texts = w.event_texts();
    assert_in_order(
        &texts,
        &[
            "실행 기록 저장소에 쓰지 못했습니다(출발: Busy: db locked). 실행은 그대로 가고, 다음 기록 때 실행 폴더에서 다시 옮깁니다.",
            "실행 기록 저장소에 다시 옮겨 적었습니다(단위 1 판정 뒤).",
        ],
    );
    assert!(w.run_json().get("records_error").is_none(), "다시 옮기면 records_error 를 지운다");
    assert_eq!(w.queue_statuses(), vec!["done"], "기록이 실패해도 실행은 간다");
}

#[test]
fn records_none_skips() {
    let w = one_unit();
    let exit = match w.open_without_records() {
        Ok(e) => e.run(),
        Err(x) => x,
    };
    assert_eq!(exit, RunExit::Finished(RunState::Done));
    assert!(w.records.upserts.lock().unwrap().is_empty());
    assert!(w.run_json().get("records_error").is_none());
    assert!(!w.has_event("실행 기록 저장소"));
}

#[test]
fn finish_order() {
    let mut s = Scenario::new(1);
    with_branches(&mut s);
    let w = s.build();
    w.git.reply("symbolic-ref -q --short HEAD", 0, "autopilot/r1\n");
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let j = w.journal.all();
    let at = |from: usize, what: &str| from + j[from..].iter().position(|e| e == what).unwrap_or_else(|| panic!("{what} 없음: {j:?}"));
    // ended 와 state 는 run.json 에 한 번에 적힌다(원본 setState 와 같다)
    let state = at(0, "run:done:ended:-");
    let record = at(state, "records:runs");
    let restore = at(record, "git:switch main");
    let wrapped = at(restore, "run:done:ended:wrapped");
    let unregister = at(wrapped, "unregister:lbl-r1");
    assert!(state < record && record < restore && restore < wrapped && wrapped < unregister);
}

#[test]
fn finish_restores_branch_when_clean() {
    let mut s = Scenario::new(1);
    with_branches(&mut s);
    let w = s.build();
    w.git.reply("symbolic-ref -q --short HEAD", 0, "autopilot/r1\n");
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let repo = w.run_json()["branches"]["repos"]["C:\\repo"].clone();
    assert_eq!(repo["restored"], true);
    assert_eq!(repo["on"], "main");
    assert!(repo["restore_note"].as_str().unwrap().contains("원래 작업 트리를 출발 때 가지(main)로 되돌렸습니다"));
    assert!(w.git.args().contains(&"switch main".to_string()));
    assert!(w.has_event("원래 작업 트리를 출발 때 가지(main)로 되돌렸습니다."));
    assert!(w.run_json()["wrapped"].is_number());
}

#[test]
fn finish_keeps_branch_when_dirty() {
    let mut s = Scenario::new(1);
    with_branches(&mut s);
    let w = s.build();
    w.git.reply("symbolic-ref -q --short HEAD", 0, "autopilot/r1\n");
    w.git.reply("status --porcelain", 0, " M a.txt\n");
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let repo = w.run_json()["branches"]["repos"]["C:\\repo"].clone();
    assert_eq!(repo["restored"], false);
    assert_eq!(
        repo["restore_note"],
        "커밋하지 않은 변경 1개가 있어 출발 때 가지로 되돌리지 않았습니다(작업 트리는 autopilot/r1 에 있습니다)"
    );
    assert!(!w.git.args().iter().any(|a| a.starts_with("switch")), "{:?}", w.git.args());
    assert!(w.run_json()["wrapped"].is_number(), "되돌리지 못해도 끝 정리는 마친다");
}

#[test]
fn boot_ended_rewraps() {
    let mut s = Scenario::new(1);
    s.run["state"] = json!("done");
    s.run["reason"] = json!("대기열을 모두 마쳤습니다.");
    s.run["ended"] = json!(NOW - 50.0);
    s.queue[0]["status"] = json!("done");
    let w = s.build();
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let run = w.run_json();
    assert_eq!(run["ended"].as_f64(), Some(NOW - 50.0), "이미 있는 ended 는 건드리지 않는다");
    assert!(run["wrapped"].is_number());
    assert_eq!(*w.supervisor.unregistered.lock().unwrap(), vec!["lbl-r1".to_string()]);
    assert_eq!(w.env.spawn_count(), 0);
    assert_eq!(w.records.rows("runs").len(), 1, "끝 정리의 기록");
}

#[test]
fn crash_resume_restart_message() {
    let w = running_scenario(SID).build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    // 직전 엔진이 남긴 claude 를 먼저 끝낸다
    assert_eq!(*w.reaped.lock().unwrap(), vec![(48250, Some(123.5))]);
    let texts = w.event_texts();
    assert_in_order(&texts, &["엔진이 다시 떴습니다. 직전에 비정상으로 끝나 하던 자리에서 잇습니다.", "단위 1/1 통과"]);
    assert!(!texts.iter().any(|t| t.contains("단위 1/1 시작")), "하던 단위를 새로 시작하지 않는다");
    // 같은 세션으로 이어 열며 "엔진이 다시 떠서 이어 갑니다" 를 넘긴다
    let launches = w.env.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].session, SessionStart::Resume(SID.to_string()));
    assert_eq!(content_of(&w.env.sent(0)[0]), MSG_RESTART);
    let last = w.files.lines("starts.log").pop().unwrap();
    assert!(last.contains("\tabnormal\t") && last.contains("prev=running"), "{last}");
    // 시계는 이어서 센다
    assert!(w.unit_json(1)["active_sec"].as_f64().unwrap() >= 50.0);
}

/// 모델이 쓸 수 있는 공개 사본(`units/1/engine.json`)을 바꿔도 엔진은 읽지 않는다. 엔진 전용 사본의 pid 만 끝낸다
#[test]
fn tampered_public_unit_file_is_ignored() {
    let mut s = running_scenario(SID);
    let forged = json!({
        "unit": 1, "id": "u1", "title": "첫째", "status": "running", "phase": "model",
        "session_id": "forged", "session_started": true, "procs": {"claude_pid": 1, "ctime": 1.0},
    });
    s.file("units/1/engine.json", &serde_json::to_string_pretty(&forged).unwrap());
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(*w.reaped.lock().unwrap(), vec![(48250, Some(123.5))], "공개 사본의 pid 를 끝내지 않는다");
    assert_eq!(w.env.launches()[0].session, SessionStart::Resume(SID.to_string()));
    assert_eq!(w.files.json("/private/r1/unit-1.json").unwrap()["status"], "done", "엔진 전용 사본이 끝 상태");
    assert_eq!(w.unit_json(1)["status"], "done", "공개 사본은 엔진이 덮어쓴 보드용 사본");
}

#[test]
fn three_abnormal_errors() {
    let mut s = running_scenario(SID);
    s.file("starts.log", &format!("{}\n{}\n", starts_line(NOW - 100.0, "abnormal"), starts_line(NOW - 50.0, "abnormal")));
    let w = s.build();
    assert_eq!(w.run(), RunExit::Finished(RunState::Error));
    assert_eq!(w.env.spawn_count(), 0);
    assert_eq!(*w.reaped.lock().unwrap(), vec![(48250, Some(123.5))]);
    let run = w.run_json();
    assert_eq!(run["state"], "error");
    assert_eq!(run["reason"], "엔진 오류로 멈춤: 10분 안에 세 번 비정상으로 다시 떴습니다.");
    assert_eq!(w.queue_statuses(), vec!["stopped"]);
    assert_eq!(w.unit_json(1)["reason"], "엔진 오류로 멈췄습니다.");
    assert!(w.has_event("엔진 오류로 멈춥니다: 10분 안에 세 번 비정상으로 다시 떴습니다."));
    assert_eq!(*w.supervisor.unregistered.lock().unwrap(), vec!["lbl-r1".to_string()]);
}

// ── 흐름 ──

#[test]
fn flow_two_units_pass_and_cap() {
    let mut s = Scenario::new(2);
    s.unit(2, "caps", json!({"minutes": 1}));
    s.file("units/1/result.json", "{\"outcome\":\"done\"}");
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w.env.push_spawn(ChildScript::new(vec![Step::Line(init_line())]));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));

    assert_eq!(w.queue_statuses(), vec!["done", "capped"]);
    let run = w.run_json();
    assert_eq!(run["state"], "done");
    assert_eq!(run["reason"], "대기열을 모두 마쳤습니다.");
    assert!(run["ended"].is_number() && run["wrapped"].is_number());
    assert_eq!(run["unit"], 2);
    assert_eq!(run["unit_caps"]["minutes"], 1);
    assert_eq!(run["engine_pid"], 77);
    assert!(run["started"].is_number());

    let u1 = w.unit_json(1);
    assert_eq!(u1["status"], "done");
    assert_eq!(u1["phase"], "end");
    assert_eq!(u1["result_ok"], true);
    assert_eq!(u1["checks"].as_array().unwrap().len(), 1);
    assert_eq!(u1["reason"], "판정 명령을 통과했습니다.");
    let u2 = w.unit_json(2);
    assert_eq!(u2["status"], "capped");
    assert_eq!(u2["result_ok"], false);
    assert!(u2["git"].is_object());
    assert!(u2["reason"].as_str().unwrap().contains("시간 상한에 닿아 멈췄습니다."));

    let texts = w.event_texts();
    assert_in_order(
        &texts,
        &[
            "자율주행을 시작합니다. 단위 2개를 차례로 합니다(모델 m1).",
            "단위 1/2 시작: 첫째",
            "일합니다",
            "판정 명령을 돌립니다.",
            "판정 통과(",
            "단위 1/2 통과: 판정 명령을 통과했습니다.",
            "단위 2/2 시작: 둘째",
            "claude 를 멈춥니다.",
            "단위 2/2 상한에서 멈춤: 시간 상한에 닿아 멈췄습니다.",
            "모델이 결과 기록(result.json)을 남기지 않았습니다.",
            "자율주행을 마쳤습니다. 통과 1, 미통과 0, 상한에서 멈춤 1.",
        ],
    );
    assert!(texts.iter().any(|t| t.contains("0.500 USD")), "단위 1 의 비용");
    assert_eq!(count(&texts, "모델이 결과 기록(result.json)을 남기지 않았습니다."), 1, "단위 1 은 결과 기록이 있다");
    let launches = w.env.launches();
    assert_eq!(launches.len(), 2);
    assert_ne!(sid_of(&launches[0]), sid_of(&launches[1]), "단위마다 세션이 다르다");
    assert_eq!(w.env.child_events(1), vec!["interrupt"]);
    assert_eq!(w.supervisor.unregistered.lock().unwrap().len(), 1);
}

// ── 모델이 쓰는 파일의 크기 한도 ──

#[test]
fn oversize_result_json_is_not_read_and_counts_as_missing() {
    let mut s = Scenario::new(1);
    // 모델이 쓸 수 있는 폴더의 result.json 이 한도(1 MiB)를 넘는다. 내용은 올바른 JSON 이어도 읽지 않는다
    let big = format!("{{\"outcome\":\"done\",\"pad\":\"{}\"}}", "x".repeat((1 << 20) + 10));
    s.file("units/1/result.json", &big);
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(w.unit_json(1)["result_ok"], false);
    let texts = w.event_texts();
    assert_eq!(count(&texts, "너무 커서"), 1, "{texts:?}");
    assert_eq!(count(&texts, "모델이 결과 기록(result.json)을 남기지 않았습니다."), 1);
}

#[test]
fn result_json_at_the_limit_is_still_read() {
    let mut s = Scenario::new(1);
    let head = "{\"outcome\":\"done\",\"pad\":\"";
    let tail = "\"}";
    let pad = "x".repeat((1 << 20) - head.len() - tail.len());
    s.file("units/1/result.json", &format!("{head}{pad}{tail}"));
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    assert_eq!(w.unit_json(1)["result_ok"], true);
    assert_eq!(count(&w.event_texts(), "너무 커서"), 0);
}

// ── 늘 거는 거부 목록 ──

#[test]
fn session_policy_carries_the_always_on_deny_list() {
    let mut s = Scenario::new(1);
    s.frozen["write_roots"] = json!(["D:\\shared"]);
    s.frozen["disallowed_tools"] = json!(["Bash(rm *)"]);
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let policy = w.env.policies.lock().unwrap()[0].clone();
    let writes: Vec<String> = policy.deny_write.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
    for root in ["C:/work", "D:/shared"] {
        for spot in ["CLAUDE.md", ".claude", ".git/config", ".git/hooks", ".mcp.json", "**/CLAUDE.md"] {
            let want = format!("{root}/{spot}");
            assert!(writes.contains(&want), "{want} 가 쓰기 금지에 없다");
        }
    }
    // 사용자 영역: 로그인 정보는 읽기도, 설정·시작 프로그램은 쓰기를
    let creds = claude_board_core::paths::claude_dir().join(".credentials.json").to_string_lossy().replace('\\', "/");
    assert!(policy.deny_read.iter().any(|p| p.to_string_lossy().replace('\\', "/") == creds), "{:?}", policy.deny_read);
    assert!(writes.iter().any(|w| w.ends_with("/.claude/settings*.json")), "{writes:?}");
    assert!(writes.iter().any(|w| w.ends_with("Start Menu/Programs/Startup")), "{writes:?}");
    // 단위 설정의 거부 도구는 그대로 두고 늘 거는 규칙이 붙는다
    assert_eq!(policy.disallowed_tools[0], "Bash(rm *)");
    for t in ["Bash(git push *)", "Bash(git -C * push *)", "Edit(**/CLAUDE.md)", "mcp__atlassian"] {
        assert!(policy.disallowed_tools.contains(&t.to_string()), "{t}");
    }
    // 같은 규칙이 두 번 들지 않는다
    let mut seen = std::collections::HashSet::new();
    assert!(policy.disallowed_tools.iter().all(|t| seen.insert(t.clone())));
}

// ── 허용 도구 기본값: 환경이 정한다 ──

/// 시나리오를 한 턴 돌려 모델 세션에 넘어간 허용 도구를 받는다
fn allowed_tools_of(mut s: Scenario, sandbox: Option<bool>, frozen_allowed: Option<Value>) -> Vec<String> {
    if let Some(b) = sandbox {
        s.frozen["exec_env"] = json!({"key": "x", "kind": if b { "wsl" } else { "pwsh" }, "sandbox": b});
    }
    if let Some(a) = frozen_allowed {
        s.frozen["allowed_tools"] = a;
    }
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let policy = w.env.policies.lock().unwrap()[0].clone();
    policy.allowed_tools
}

#[test]
fn native_default_allowed_tools_has_no_bare_bash() {
    for sandbox in [Some(false), None] {
        let tools = allowed_tools_of(Scenario::new(1), sandbox, None);
        for t in ["Read", "Glob", "Grep", "Task", "TaskStop", "SendMessage", "Skill", "ToolSearch", "Monitor", "TodoWrite"] {
            assert!(tools.contains(&t.to_string()), "{t} 가 기본 목록에 없다: {tools:?}");
        }
        assert!(!tools.contains(&"Bash".to_string()), "{sandbox:?}: {tools:?}");
        assert!(!tools.iter().any(|t| t.starts_with("Write") || t == "Edit"), "{tools:?}");
    }
}

#[test]
fn sandboxed_default_allowed_tools_has_bash() {
    let tools = allowed_tools_of(Scenario::new(1), Some(true), None);
    assert_eq!(tools, ["Read", "Glob", "Grep", "Bash", "Task", "TaskStop", "SendMessage", "Skill", "ToolSearch", "Monitor", "TodoWrite"]);
}

#[test]
fn frozen_allowed_tools_always_win_in_both_envs() {
    // 네이티브: 명시한 접두 규칙만 그대로 넘어가고 맨 Bash 는 생기지 않는다
    for sandbox in [Some(false), None] {
        let tools = allowed_tools_of(Scenario::new(1), sandbox, Some(json!(["Bash(npm test *)"])));
        assert_eq!(tools, ["Bash(npm test *)"], "{sandbox:?}");
    }
    // 샌드박스 WSL: 목록이 기본값을 대신하고, 파일 쓰기 도구는 여전히 걸러진다
    let tools = allowed_tools_of(Scenario::new(1), Some(true), Some(json!(["Read", "Write", "Bash(npm test *)"])));
    assert_eq!(tools, ["Read", "Bash(npm test *)"]);
    // 빈 목록도 명시한 것이다 — 기본값으로 되돌아가지 않는다
    assert!(allowed_tools_of(Scenario::new(1), Some(true), Some(json!([]))).is_empty());
}
