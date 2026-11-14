//! 엔진 상태 기계(`engine::decide`) 시험. 전이 표 줄마다 (시작 state, 이벤트) → (Action 목록 전체, 바뀐 state 칸)을 본다.
//! 이름의 b1·n4·k5 따위는 설계 전이 표의 줄 번호다.

use claude_board_autopilot::engine::decide::*;
use claude_board_autopilot::rules::checks;
use claude_board_autopilot::rules::control::{self, RestartPlan};
use claude_board_autopilot::rules::stopline::{self, Tz};
use claude_board_autopilot::stream::StreamState;
use serde_json::{json, Map, Value};

const NOW: f64 = 1_800_000_000.0;

// ── 도우미 ──

fn plan(title: &str, has_action: bool, has_precheck: bool) -> UnitPlan {
    UnitPlan { title: title.to_string(), check: format!("check {title}"), has_action, has_precheck, caps: Caps::default() }
}

fn cur(n: usize, phase: UnitPhase) -> Current {
    Current { n, phase, tries: 0, errors: 0, limits: 0, resume: false, next: Next::First, wait_until: None }
}

fn state_with(
    units: Vec<UnitPlan>,
    statuses: Vec<UnitStatus>,
    current: Option<Current>,
    inflight: Inflight,
    run: RunState,
) -> EngineState {
    let mut s = EngineState::new("sonnet", units, 2, 5.0, Map::new(), Tz::Utc);
    s.statuses = statuses;
    s.current = current;
    s.inflight = inflight;
    s.run = run;
    s
}

/// 단위 하나가 Running 이고 current 가 있는 state
fn running(phase: UnitPhase, inflight: Inflight) -> EngineState {
    state_with(vec![plan("작업", false, false)], vec![UnitStatus::Running], Some(cur(1, phase)), inflight, RunState::Running)
}

/// 단위 하나가 Pending 이고 시험 환경 확인이 도는 state
fn prechecking(run: RunState) -> EngineState {
    state_with(vec![plan("작업", false, true)], vec![UnitStatus::Pending], None, Inflight::Precheck(1), run)
}

fn outcome_ok() -> ClaudeOutcome {
    ClaudeOutcome {
        session_started: true,
        result_seen: true,
        last_error: false,
        limit: false,
        limit_resets: None,
        budget: false,
        err_text: String::new(),
        exit_code: Some(0),
        stderr_tail: String::new(),
    }
}

fn board(text: &str) -> Action {
    Action::Log { who: Who::Board, text: text.to_string() }
}

fn engine(text: &str) -> Action {
    Action::Log { who: Who::Engine, text: text.to_string() }
}

fn when(t: f64) -> String {
    stopline::when_text(t, Tz::Utc)
}

fn exited(o: ClaudeOutcome) -> Event {
    Event::ClaudeExited { now: NOW, outcome: o }
}

fn control_ev(action: &str) -> Event {
    Event::Control { now: NOW, action: action.to_string() }
}

fn next_ev() -> Event {
    Event::Next { now: NOW, usage: None }
}

fn check_passed() -> Event {
    Event::CheckDone { now: NOW, verdict: CheckVerdict::Passed }
}

fn failed_check(code: i32, tail: &str, timed_out: Option<f64>) -> Event {
    Event::CheckDone { now: NOW, verdict: CheckVerdict::Failed { code, tail: tail.to_string(), timed_out } }
}

fn paused_tail() -> Vec<Action> {
    vec![
        board("일시정지했습니다. 재개하면 같은 세션으로 이어 갑니다."),
        Action::SetState { state: RunState::Paused, reason: "사람이 일시정지했습니다. 재개하면 이어 갑니다.".into(), wait_until: None },
    ]
}

fn stop_tail() -> Vec<Action> {
    vec![
        board("사람이 중단해 실행을 마칩니다. 다시 띄우지 않습니다."),
        Action::Finish { state: RunState::Stopped, reason: "사람이 중단했습니다.".into() },
    ]
}

fn end_stopped(n: usize) -> Action {
    Action::EndUnit { n, status: UnitStatus::Stopped, reason: "사람이 중단했습니다.".into() }
}

fn capped_text(label: &str) -> String {
    format!("{label} 상한에 닿아 멈췄습니다. 바뀐 파일 목록을 남기고 다음 단위로 갑니다.")
}

fn begin_model(n: usize, total: usize, title: &str) -> Vec<Action> {
    vec![
        Action::BeginUnit { n },
        board(&format!("단위 {n}/{total} 시작: {title}")),
        Action::SetState { state: RunState::Running, reason: format!("단위 {n}/{total} 하는 중: {title}"), wait_until: None },
        Action::StartClaude { n, resume: false, next: Next::First },
    ]
}

fn usage_with(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn head(a: &Action) -> &'static str {
    match a {
        Action::SetState { .. } => "SetState",
        Action::Log { .. } => "Log",
        Action::BeginUnit { .. } => "BeginUnit",
        Action::RunPrecheck { .. } => "RunPrecheck",
        Action::RunAction { .. } => "RunAction",
        Action::StartClaude { .. } => "StartClaude",
        Action::RunCheck { .. } => "RunCheck",
        Action::Interrupt { .. } => "Interrupt",
        Action::WaitUntil { .. } => "WaitUntil",
        Action::SaveUnit { .. } => "SaveUnit",
        Action::EndUnit { .. } => "EndUnit",
        Action::SkipUnit { .. } => "SkipUnit",
        Action::KillLeftovers { .. } => "KillLeftovers",
        Action::MarkRestart { .. } => "MarkRestart",
        Action::MarkRestartStopped => "MarkRestartStopped",
        Action::Finish { .. } => "Finish",
    }
}

fn heads(v: &[Action]) -> Vec<&'static str> {
    v.iter().map(head).collect()
}

// ── Boot ──

#[test]
fn b1_ended_rewraps() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::Done;
    s.reason = "대기열을 모두 마쳤습니다.".into();
    let before = s.clone();
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 5 });
    assert_eq!(out, vec![Action::Finish { state: RunState::Done, reason: "대기열을 모두 마쳤습니다.".into() }]);
    assert_eq!(s, before);
}

#[test]
fn b2_three_abnormal_errors_with_unit() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    s.pending_stop = Some(StopWhy::Pause);
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 3 });
    assert_eq!(
        out,
        vec![
            Action::KillLeftovers { n: 1 },
            engine("엔진 오류로 멈춥니다: 10분 안에 세 번 비정상으로 다시 떴습니다."),
            Action::EndUnit { n: 1, status: UnitStatus::Stopped, reason: "엔진 오류로 멈췄습니다.".into() },
            Action::Finish { state: RunState::Error, reason: "엔진 오류로 멈춤: 10분 안에 세 번 비정상으로 다시 떴습니다.".into() },
        ]
    );
    assert_eq!(s.statuses, vec![UnitStatus::Stopped]);
    assert_eq!((s.run, s.current.is_none(), s.inflight, s.pending_stop), (RunState::Error, true, Inflight::Idle, None));
}

#[test]
fn b2_three_abnormal_errors_without_unit() {
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Idle, RunState::WaitingStopline);
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 4 });
    assert_eq!(
        out,
        vec![
            engine("엔진 오류로 멈춥니다: 10분 안에 세 번 비정상으로 다시 떴습니다."),
            Action::Finish { state: RunState::Error, reason: "엔진 오류로 멈춤: 10분 안에 세 번 비정상으로 다시 떴습니다.".into() },
        ]
    );
    assert_eq!(s.run, RunState::Error);
}

#[test]
fn b2_two_abnormal_resumes() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 2 });
    assert_eq!(
        out,
        vec![
            Action::KillLeftovers { n: 1 },
            engine("엔진이 다시 떴습니다. 직전에 비정상으로 끝나 하던 자리에서 잇습니다."),
            Action::SaveUnit { n: 1 },
        ]
    );
    assert_eq!(s.inflight, Inflight::Idle);
}

#[test]
fn b3_first_start() {
    let mut s = EngineState::new("sonnet", vec![plan("가", false, false), plan("나", false, false)], 2, 5.0, Map::new(), Tz::Utc);
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 9 });
    assert_eq!(
        out,
        vec![
            engine("자율주행을 시작합니다. 단위 2개를 차례로 합니다(모델 sonnet)."),
            Action::SetState { state: RunState::Running, reason: "단위를 시작하는 중입니다.".into(), wait_until: None },
        ]
    );
    assert_eq!(s.run, RunState::Running);
}

#[test]
fn b4_paused_stays() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::Paused;
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 1 });
    assert_eq!(
        out,
        vec![Action::KillLeftovers { n: 1 }, engine("엔진이 다시 떴습니다. 사람이 일시정지한 실행이라 그대로 쉽니다.")]
    );
    assert_eq!(s.run, RunState::Paused);
    assert_eq!(s.current.as_ref().unwrap().next, Next::First);
}

#[test]
fn b5_taken_over_stays() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::TakenOver;
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 0 });
    assert_eq!(
        out,
        vec![Action::KillLeftovers { n: 1 }, engine("엔진이 다시 떴습니다. 사람이 이어받은 실행이라 돌려주기를 그대로 기다립니다.")]
    );
    assert_eq!(s.run, RunState::TakenOver);
}

#[test]
fn b6_crash_in_model_sets_restart() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    s.current.as_mut().unwrap().resume = true;
    decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 1 });
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.next.clone(), c.phase, c.resume), (Next::Restart, UnitPhase::Model, true));
    // 이어서 Next 가 그 세션을 --resume 으로 다시 연다
    let out = decide(&mut s, next_ev());
    assert_eq!(out, vec![Action::StartClaude { n: 1, resume: true, next: Next::Restart }]);
}

#[test]
fn b6_crash_in_check_keeps_phase() {
    let mut s = running(UnitPhase::Check, Inflight::Check);
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 1 });
    assert_eq!(
        out,
        vec![Action::KillLeftovers { n: 1 }, engine("엔진이 다시 떴습니다. 직전에 비정상으로 끝나 하던 자리에서 잇습니다.")]
    );
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.phase, c.next.clone()), (UnitPhase::Check, Next::First));
    // 판정 중이었으면 판정을 다시 돌린다
    assert_eq!(decide(&mut s, next_ev()), vec![Action::RunCheck { n: 1, attempt: 0 }]);
}

#[test]
fn b6_crash_in_limit_wait() {
    let mut s = running(UnitPhase::LimitWait, Inflight::Wait(WaitWhy::Limit));
    s.run = RunState::WaitingLimit;
    let until = NOW + 100.0;
    s.current.as_mut().unwrap().wait_until = Some(until);
    let out = decide(&mut s, Event::Boot { now: NOW, abnormal_recent: 1 });
    assert_eq!(heads(&out), vec!["KillLeftovers", "Log"]);
    assert_eq!(s.inflight, Inflight::Idle);
    let out = decide(&mut s, next_ev());
    assert_eq!(
        out,
        vec![
            Action::SetState {
                state: RunState::WaitingLimit,
                reason: format!("사용량 한도로 쉬는 중입니다. {} 에 같은 자리에서 다시 시작합니다.", when(until)),
                wait_until: Some(until),
            },
            Action::WaitUntil { until },
        ]
    );
    assert_eq!(s.inflight, Inflight::Wait(WaitWhy::Limit));
}

// ── Next ──

#[test]
fn n1_continue_model_resume() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    {
        let c = s.current.as_mut().unwrap();
        c.resume = true;
        c.next = Next::Paused;
    }
    let out = decide(&mut s, next_ev());
    assert_eq!(out, vec![Action::StartClaude { n: 1, resume: true, next: Next::Paused }]);
    assert_eq!(s.inflight, Inflight::Claude);
}

#[test]
fn n1_continue_action() {
    let mut s = running(UnitPhase::Action, Inflight::Idle);
    assert_eq!(decide(&mut s, next_ev()), vec![Action::RunAction { n: 1 }]);
    assert_eq!(s.inflight, Inflight::Action);
}

#[test]
fn n1_continue_check_attempt() {
    let mut s = running(UnitPhase::Check, Inflight::Idle);
    s.current.as_mut().unwrap().tries = 1;
    assert_eq!(decide(&mut s, next_ev()), vec![Action::RunCheck { n: 1, attempt: 1 }]);
    assert_eq!(s.inflight, Inflight::Check);
}

#[test]
fn n2_all_done_counts() {
    let units = vec![plan("가", false, false), plan("나", false, false), plan("다", false, false)];
    let mut s = state_with(units, vec![UnitStatus::Done, UnitStatus::Failed, UnitStatus::Capped], None, Inflight::Idle, RunState::Running);
    let out = decide(&mut s, next_ev());
    assert_eq!(
        out,
        vec![
            engine("자율주행을 마쳤습니다. 통과 1, 미통과 1, 상한에서 멈춤 1."),
            Action::Finish { state: RunState::Done, reason: "대기열을 모두 마쳤습니다.".into() },
        ]
    );
    assert_eq!(s.run, RunState::Done);
}

#[test]
fn n2_all_done_with_skipped() {
    let units = vec![plan("가", false, false), plan("나", false, false)];
    let mut s = state_with(units, vec![UnitStatus::Done, UnitStatus::Skipped], None, Inflight::Idle, RunState::Running);
    let out = decide(&mut s, next_ev());
    assert_eq!(out[0], engine("자율주행을 마쳤습니다. 통과 1, 미통과 0, 상한에서 멈춤 0, 건너뜀 1."));
}

#[test]
fn n3_restart_stops_before_pending() {
    let units = vec![plan("가", false, false), plan("나", false, false), plan("다", false, false)];
    let mut s = state_with(units, vec![UnitStatus::Done, UnitStatus::Pending, UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    s.restart_asked = true;
    let out = decide(&mut s, next_ev());
    assert_eq!(
        out,
        vec![
            Action::MarkRestartStopped,
            board("멈추고 다시 출발: 사람이 요청해 여기서 멈춥니다. 남은 단위 2개는 출발 준비에서 다시 고릅니다."),
            Action::Finish { state: RunState::Stopped, reason: control::RESTART_REASON.into() },
        ]
    );
    assert_eq!(s.run, RunState::Stopped);
}

#[test]
fn n4_stopline_weekly() {
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    let usage = usage_with(json!({"seven_day": 0.9, "seven_day_resets": NOW + 7200.0}));
    let w = stopline::stopline_wait(Some(&usage), &s.stopline, NOW, Tz::Utc).unwrap();
    assert_eq!(w.until, NOW + 7320.0);
    assert!(w.reason.contains("주간"));
    let out = decide(&mut s, Event::Next { now: NOW, usage: Some(usage) });
    assert_eq!(
        out,
        vec![
            board(&w.reason),
            Action::SetState { state: RunState::WaitingStopline, reason: w.reason.clone(), wait_until: Some(w.until) },
            Action::WaitUntil { until: w.until },
        ]
    );
    assert_eq!((s.run, s.inflight), (RunState::WaitingStopline, Inflight::Wait(WaitWhy::Stopline)));
}

#[test]
fn n4_stopline_unknown_usage() {
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    let out = decide(&mut s, next_ev());
    let reason = "계정 사용률을 알 수 없어 새 단위를 시작하지 않고 10분 뒤 다시 확인합니다.";
    assert_eq!(
        out,
        vec![
            board(reason),
            Action::SetState { state: RunState::WaitingStopline, reason: reason.into(), wait_until: Some(NOW + 600.0) },
            Action::WaitUntil { until: NOW + 600.0 },
        ]
    );
}

#[test]
fn n4_same_reason_no_log() {
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Idle, RunState::WaitingStopline);
    let reason = "계정 사용률을 알 수 없어 새 단위를 시작하지 않고 10분 뒤 다시 확인합니다.";
    s.reason = reason.into();
    let out = decide(&mut s, next_ev());
    assert_eq!(
        out,
        vec![
            Action::SetState { state: RunState::WaitingStopline, reason: reason.into(), wait_until: Some(NOW + 600.0) },
            Action::WaitUntil { until: NOW + 600.0 },
        ]
    );
}

fn calm_usage() -> Option<Map<String, Value>> {
    Some(usage_with(json!({"seven_day": 0.1, "five_hour": 0.1})))
}

#[test]
fn n5_precheck_first() {
    let units = vec![plan("가", false, false), plan("나", false, true)];
    let mut s = state_with(units, vec![UnitStatus::Done, UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    let out = decide(&mut s, Event::Next { now: NOW, usage: calm_usage() });
    assert_eq!(out, vec![Action::RunPrecheck { n: 2 }]);
    assert_eq!(s.inflight, Inflight::Precheck(2));
    assert!(s.current.is_none());
}

#[test]
fn n6_begin_model() {
    let units = vec![plan("가", false, false), plan("나", false, false)];
    let mut s = state_with(units, vec![UnitStatus::Pending, UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    let out = decide(&mut s, Event::Next { now: NOW, usage: calm_usage() });
    assert_eq!(out, begin_model(1, 2, "가"));
    assert_eq!(s.statuses[0], UnitStatus::Running);
    assert_eq!(s.inflight, Inflight::Claude);
    assert_eq!(s.current, Some(cur(1, UnitPhase::Model)));
}

#[test]
fn n6_begin_action() {
    let mut s = state_with(vec![plan("가", true, false)], vec![UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    let out = decide(&mut s, Event::Next { now: NOW, usage: calm_usage() });
    assert_eq!(
        out,
        vec![
            Action::BeginUnit { n: 1 },
            board("단위 1/1 시작: 가"),
            Action::SetState { state: RunState::Running, reason: "단위 1/1 하는 중: 가".into(), wait_until: None },
            Action::RunAction { n: 1 },
        ]
    );
    assert_eq!((s.current.as_ref().unwrap().phase, s.inflight), (UnitPhase::Action, Inflight::Action));
}

#[test]
fn next_while_paused_noop() {
    for run in [RunState::Paused, RunState::TakenOver] {
        let mut s = running(UnitPhase::Model, Inflight::Idle);
        s.run = run;
        let before = s.clone();
        assert_eq!(decide(&mut s, next_ev()), vec![]);
        assert_eq!(s, before);
    }
}

// ── WaitElapsed ──

#[test]
fn w1_stopline_back_to_running() {
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Wait(WaitWhy::Stopline), RunState::WaitingStopline);
    let out = decide(&mut s, Event::WaitElapsed { now: NOW });
    assert_eq!(out, vec![Action::SetState { state: RunState::Running, reason: "멈출 선 대기를 마치고 다시 확인합니다.".into(), wait_until: None }]);
    assert_eq!((s.run, s.inflight), (RunState::Running, Inflight::Idle));
    // 이미 Running 이면 상태를 다시 쓰지 않는다
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Wait(WaitWhy::Stopline), RunState::Running);
    assert_eq!(decide(&mut s, Event::WaitElapsed { now: NOW }), vec![]);
    assert_eq!(s.inflight, Inflight::Idle);
}

#[test]
fn w2_precheck_again() {
    let mut s = prechecking(RunState::WaitingPrecheck);
    s.inflight = Inflight::Wait(WaitWhy::Precheck(1));
    let out = decide(&mut s, Event::WaitElapsed { now: NOW });
    assert_eq!(out, vec![Action::RunPrecheck { n: 1 }]);
    assert_eq!((s.run, s.inflight), (RunState::WaitingPrecheck, Inflight::Precheck(1)));
}

#[test]
fn w3_limit_resume() {
    let mut s = running(UnitPhase::LimitWait, Inflight::Wait(WaitWhy::Limit));
    s.run = RunState::WaitingLimit;
    {
        let c = s.current.as_mut().unwrap();
        c.wait_until = Some(NOW);
        c.next = Next::Limit;
        c.resume = true;
    }
    let out = decide(&mut s, Event::WaitElapsed { now: NOW });
    assert_eq!(
        out,
        vec![
            Action::SaveUnit { n: 1 },
            board("한도 대기를 마치고 같은 세션으로 이어 갑니다."),
            Action::SetState { state: RunState::Running, reason: "단위 1/1 하는 중: 작업".into(), wait_until: None },
            Action::StartClaude { n: 1, resume: true, next: Next::Limit },
        ]
    );
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.phase, c.wait_until), (UnitPhase::Model, None));
    assert_eq!((s.run, s.inflight), (RunState::Running, Inflight::Claude));
}

#[test]
fn w4_error_reopen() {
    let mut s = running(UnitPhase::Model, Inflight::Wait(WaitWhy::ClaudeError));
    {
        let c = s.current.as_mut().unwrap();
        c.resume = true;
        c.next = Next::Error;
    }
    let out = decide(&mut s, Event::WaitElapsed { now: NOW });
    assert_eq!(out, vec![Action::StartClaude { n: 1, resume: true, next: Next::Error }]);
    assert_eq!(s.inflight, Inflight::Claude);
}

// ── PrecheckDone ──

fn precheck_ok() -> Event {
    Event::PrecheckDone { now: NOW, ok: true, code: 0, last_line: String::new() }
}

#[test]
fn p1_pause_during_precheck() {
    let mut s = prechecking(RunState::Running);
    s.pending_stop = Some(StopWhy::Pause);
    assert_eq!(decide(&mut s, precheck_ok()), paused_tail());
    assert_eq!((s.run, s.inflight, s.pending_stop), (RunState::Paused, Inflight::Idle, None));
    assert!(s.current.is_none());
}

#[test]
fn p2_stop_during_precheck() {
    let mut s = prechecking(RunState::Running);
    s.pending_stop = Some(StopWhy::Stop);
    assert_eq!(decide(&mut s, precheck_ok()), stop_tail());
    assert_eq!(s.run, RunState::Stopped);
}

#[test]
fn p3_restart_during_precheck() {
    let mut s = prechecking(RunState::Running);
    s.pending_stop = Some(StopWhy::Restart);
    let out = decide(&mut s, precheck_ok());
    assert_eq!(
        out,
        vec![
            Action::MarkRestartStopped,
            board("멈추고 다시 출발: 사람이 요청해 여기서 멈춥니다. 남은 단위 1개는 출발 준비에서 다시 고릅니다."),
            Action::Finish { state: RunState::Stopped, reason: control::RESTART_REASON.into() },
        ]
    );
}

#[test]
fn p4_ok_after_waiting_logs() {
    let mut s = prechecking(RunState::WaitingPrecheck);
    let out = decide(&mut s, precheck_ok());
    let mut want = vec![board("시험 환경 확인을 통과해 단위를 시작합니다.")];
    want.extend(begin_model(1, 1, "작업"));
    assert_eq!(out, want);
    assert_eq!((s.run, s.inflight), (RunState::Running, Inflight::Claude));
}

#[test]
fn p4_ok_first_time_no_log() {
    let mut s = prechecking(RunState::Running);
    assert_eq!(decide(&mut s, precheck_ok()), begin_model(1, 1, "작업"));
}

fn precheck_fail_reason(last: &str) -> (String, f64) {
    let until = NOW + 300.0;
    let tail = if last.is_empty() { String::new() } else { format!(": {last}") };
    (format!("단위 1/1 의 시험 환경 확인이 실패해(종료 코드 2) 시작하지 않고 {} 에 다시 확인합니다{tail}", when(until)), until)
}

fn precheck_failed(last: &str) -> Event {
    Event::PrecheckDone { now: NOW, ok: false, code: 2, last_line: last.to_string() }
}

#[test]
fn p5_fail_first_logs() {
    let mut s = prechecking(RunState::Running);
    let (reason, until) = precheck_fail_reason("no docker");
    let out = decide(&mut s, precheck_failed("no docker"));
    assert_eq!(
        out,
        vec![
            board(&reason),
            Action::SetState { state: RunState::WaitingPrecheck, reason: reason.clone(), wait_until: Some(until) },
            Action::WaitUntil { until },
        ]
    );
    assert_eq!((s.run, s.inflight), (RunState::WaitingPrecheck, Inflight::Wait(WaitWhy::Precheck(1))));
}

#[test]
fn p5_fail_again_no_log() {
    let mut s = prechecking(RunState::WaitingPrecheck);
    let (reason, until) = precheck_fail_reason("no docker");
    let out = decide(&mut s, precheck_failed("no docker"));
    assert_eq!(
        out,
        vec![
            Action::SetState { state: RunState::WaitingPrecheck, reason, wait_until: Some(until) },
            Action::WaitUntil { until },
        ]
    );
}

#[test]
fn p5_fail_without_last_line() {
    let mut s = prechecking(RunState::Running);
    let (reason, _) = precheck_fail_reason("");
    assert!(reason.ends_with("에 다시 확인합니다"));
    let out = decide(&mut s, precheck_failed(""));
    assert_eq!(out[0], board(&reason));
}

// ── UnitBlocked ──

#[test]
fn u1_running_unit_fails() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    let out = decide(&mut s, Event::UnitBlocked { now: NOW, n: 1, why: "git 폴더가 없습니다".into() });
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Failed, reason: "git 폴더가 없습니다".into() }]);
    assert_eq!(s.statuses, vec![UnitStatus::Failed]);
    assert_eq!((s.current.is_none(), s.inflight), (true, Inflight::Idle));
}

#[test]
fn u2_skip_before_begin() {
    let mut s = prechecking(RunState::Running);
    let out = decide(&mut s, Event::UnitBlocked { now: NOW, n: 1, why: "가지가 없습니다".into() });
    assert_eq!(out, vec![Action::SkipUnit { n: 1, why: "가지가 없습니다".into() }]);
    assert_eq!((s.statuses[0], s.inflight), (UnitStatus::Skipped, Inflight::Idle));
}

// ── Tick ──

fn tick(active_sec: f64, cost_usd: f64, weekly_pp: f64, limit_seen: bool) -> Event {
    Event::Tick { now: NOW, meter: UnitMeter { active_sec, cost_usd, weekly_pp }, limit_seen }
}

#[test]
fn t1_minutes_cap_claude() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    assert_eq!(decide(&mut s, tick(7199.0, 0.0, 0.0, false)), vec![]);
    assert_eq!(decide(&mut s, tick(7200.0, 0.0, 0.0, false)), vec![Action::Interrupt { target: Target::Claude }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Minutes)));
}

#[test]
fn t1_minutes_cap_check() {
    let mut s = running(UnitPhase::Check, Inflight::Check);
    assert_eq!(decide(&mut s, tick(7200.0, 0.0, 0.0, false)), vec![Action::Interrupt { target: Target::Check }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Minutes)));
}

#[test]
fn t1_minutes_cap_action() {
    let mut s = running(UnitPhase::Action, Inflight::Action);
    assert_eq!(decide(&mut s, tick(7200.0, 0.0, 0.0, false)), vec![Action::Interrupt { target: Target::Action }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Minutes)));
}

#[test]
fn t2_weekly_cap() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    assert_eq!(decide(&mut s, tick(10.0, 0.0, 5.0, false)), vec![Action::Interrupt { target: Target::Claude }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Weekly)));
}

#[test]
fn t2_weekly_ignored_when_limit_seen() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    assert_eq!(decide(&mut s, tick(10.0, 0.0, 9.0, true)), vec![]);
    assert_eq!(s.pending_stop, None);
}

#[test]
fn t3_usd_cap() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    assert_eq!(decide(&mut s, tick(10.0, 100.0, 0.0, false)), vec![Action::Interrupt { target: Target::Claude }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Usd)));
}

#[test]
fn tick_check_ignores_cost_and_weekly() {
    let mut s = running(UnitPhase::Check, Inflight::Check);
    assert_eq!(decide(&mut s, tick(10.0, 500.0, 50.0, false)), vec![]);
}

#[test]
fn tick_below_caps_noop() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    let before = s.clone();
    assert_eq!(decide(&mut s, tick(100.0, 1.0, 1.0, false)), vec![]);
    assert_eq!(s, before);
}

#[test]
fn tick_while_pending_noop() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    s.pending_stop = Some(StopWhy::Pause);
    let before = s.clone();
    assert_eq!(decide(&mut s, tick(99999.0, 999.0, 99.0, false)), vec![]);
    assert_eq!(s, before);
}

#[test]
fn tick_cap_order_minutes_first() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    decide(&mut s, tick(7200.0, 200.0, 10.0, false));
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Minutes)));
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    decide(&mut s, tick(10.0, 200.0, 10.0, false));
    assert_eq!(s.pending_stop, Some(StopWhy::Capped(CapKind::Weekly)));
}

// ── ClaudeExited ──

fn claude_state() -> EngineState {
    running(UnitPhase::Model, Inflight::Claude)
}

#[test]
fn c1_paused() {
    let mut s = claude_state();
    s.pending_stop = Some(StopWhy::Pause);
    let mut o = outcome_ok();
    o.result_seen = false;
    let out = decide(&mut s, exited(o));
    let mut want = vec![Action::SaveUnit { n: 1 }];
    want.extend(paused_tail());
    assert_eq!(out, want);
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.next.clone(), c.phase, c.resume), (Next::Paused, UnitPhase::Model, true));
    assert_eq!((s.run, s.inflight, s.pending_stop), (RunState::Paused, Inflight::Idle, None));
}

#[test]
fn c2_stopped() {
    let mut s = claude_state();
    s.pending_stop = Some(StopWhy::Stop);
    let out = decide(&mut s, exited(outcome_ok()));
    let mut want = vec![end_stopped(1)];
    want.extend(stop_tail());
    assert_eq!(out, want);
    assert_eq!((s.run, s.statuses[0]), (RunState::Stopped, UnitStatus::Stopped));
}

#[test]
fn c3_capped() {
    let mut s = claude_state();
    s.pending_stop = Some(StopWhy::Capped(CapKind::Weekly));
    let out = decide(&mut s, exited(outcome_ok()));
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Capped, reason: capped_text("주간 사용률") }]);
    assert_eq!((s.statuses[0], s.current.is_none(), s.pending_stop), (UnitStatus::Capped, true, None));
}

fn limit_outcome(resets: Option<f64>) -> ClaudeOutcome {
    let mut o = outcome_ok();
    o.limit = true;
    o.limit_resets = resets;
    o
}

fn limit_actions(until: f64, tail: &str) -> Vec<Action> {
    vec![
        Action::SaveUnit { n: 1 },
        board(&format!("사용량 한도에 막혔습니다. {} 에 같은 세션으로 다시 시작합니다{tail}.", when(until))),
        Action::SetState {
            state: RunState::WaitingLimit,
            reason: format!("사용량 한도로 쉬는 중입니다. {} 에 같은 자리에서 다시 시작합니다.", when(until)),
            wait_until: Some(until),
        },
        Action::WaitUntil { until },
    ]
}

fn assert_limit_state(s: &EngineState, until: f64) {
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.phase, c.wait_until, c.next.clone(), c.limits), (UnitPhase::LimitWait, Some(until), Next::Limit, 1));
    assert_eq!((s.run, s.inflight), (RunState::WaitingLimit, Inflight::Wait(WaitWhy::Limit)));
}

#[test]
fn c4_limit_known_reset() {
    let mut s = claude_state();
    let until = NOW + 3600.0 + 120.0;
    let out = decide(&mut s, exited(limit_outcome(Some(NOW + 3600.0))));
    assert_eq!(out, limit_actions(until, ""));
    assert_limit_state(&s, until);
}

#[test]
fn c4_limit_past_reset() {
    let mut s = claude_state();
    let until = NOW + 1800.0 + 120.0;
    let out = decide(&mut s, exited(limit_outcome(Some(NOW - 10.0))));
    assert_eq!(out, limit_actions(until, ""));
    assert_limit_state(&s, until);
}

#[test]
fn c4_limit_unknown_reset() {
    let mut s = claude_state();
    let until = NOW + 1800.0 + 120.0;
    let out = decide(&mut s, exited(limit_outcome(None)));
    assert_eq!(out, limit_actions(until, "(초기화 시각을 몰라 30분 뒤)"));
    assert_limit_state(&s, until);
}

#[test]
fn c5_budget() {
    let mut s = claude_state();
    let mut o = outcome_ok();
    o.budget = true;
    let out = decide(&mut s, exited(o));
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Capped, reason: capped_text("비용") }]);
}

fn error_actions(msg: &str, count: i64) -> Vec<Action> {
    vec![
        Action::SaveUnit { n: 1 },
        board(&format!("claude 가 오류로 끝났습니다({msg}). 1분 뒤 같은 세션으로 다시 엽니다({count}/3).")),
        Action::WaitUntil { until: NOW + 60.0 },
    ]
}

fn assert_error_state(s: &EngineState, errors: i64) {
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.errors, c.next.clone(), c.phase), (errors, Next::Error, UnitPhase::Model));
    assert_eq!(s.inflight, Inflight::Wait(WaitWhy::ClaudeError));
}

#[test]
fn c6_no_result_with_stderr() {
    let mut s = claude_state();
    let mut o = outcome_ok();
    o.result_seen = false;
    o.exit_code = Some(1);
    o.stderr_tail = "boom".into();
    let out = decide(&mut s, exited(o));
    assert_eq!(out, error_actions("결과 없이 끝남, 종료 코드 1: boom", 1));
    assert_error_state(&s, 1);
}

#[test]
fn c6_long_stderr_is_shortened() {
    let mut s = claude_state();
    let mut o = outcome_ok();
    o.result_seen = false;
    o.exit_code = Some(2);
    o.stderr_tail = format!("{}{}", "a".repeat(200), "b".repeat(300));
    let out = decide(&mut s, exited(o));
    // 끝 300자(전부 b)를 120자로: 앞 119자 + …
    let msg = format!("결과 없이 끝남, 종료 코드 2: {}…", "b".repeat(119));
    assert_eq!(out, error_actions(&msg, 1));
}

#[test]
fn c6_no_result_no_exit_code() {
    let mut s = claude_state();
    let mut o = outcome_ok();
    o.result_seen = false;
    o.exit_code = None;
    let out = decide(&mut s, exited(o));
    assert_eq!(out, error_actions("결과 없이 끝남, 종료 코드 -1", 1));
}

#[test]
fn c7_error_text() {
    let mut s = claude_state();
    let mut o = outcome_ok();
    o.last_error = true;
    o.err_text = "overloaded".into();
    let out = decide(&mut s, exited(o));
    assert_eq!(out, error_actions("overloaded", 1));
    assert_error_state(&s, 1);
}

#[test]
fn c7_error_empty_text() {
    let mut s = claude_state();
    let mut o = outcome_ok();
    o.last_error = true;
    let out = decide(&mut s, exited(o));
    assert_eq!(out, error_actions("claude 오류", 1));
}

#[test]
fn claude_failed_third_time_fails() {
    let mut s = claude_state();
    s.current.as_mut().unwrap().errors = 2;
    let mut o = outcome_ok();
    o.last_error = true;
    let out = decide(&mut s, exited(o));
    assert_eq!(
        out,
        vec![
            Action::SaveUnit { n: 1 },
            Action::EndUnit { n: 1, status: UnitStatus::Failed, reason: "claude 가 세 번 오류로 끝나 미통과로 기록합니다: claude 오류".into() },
        ]
    );
    assert_eq!((s.statuses[0], s.current.is_none(), s.inflight), (UnitStatus::Failed, true, Inflight::Idle));
}

#[test]
fn c8_turn_done_to_check() {
    let mut s = claude_state();
    s.current.as_mut().unwrap().tries = 1;
    let out = decide(&mut s, exited(outcome_ok()));
    assert_eq!(out, vec![Action::SaveUnit { n: 1 }, Action::RunCheck { n: 1, attempt: 1 }]);
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.phase, c.resume), (UnitPhase::Check, true));
    assert_eq!(s.inflight, Inflight::Check);
}

#[test]
fn resume_flag_sticks() {
    let mut s = claude_state();
    s.current.as_mut().unwrap().resume = true;
    let mut o = outcome_ok();
    o.session_started = false;
    decide(&mut s, exited(o));
    assert!(s.current.as_ref().unwrap().resume);
    // 처음 만들어진 세션이면 켜진다
    let mut s = claude_state();
    decide(&mut s, exited(outcome_ok()));
    assert!(s.current.as_ref().unwrap().resume);
}

#[test]
fn outcome_from_stream_copies() {
    let mut st = StreamState::new("sid", NOW);
    st.session_started = true;
    st.result_seen = true;
    st.limit = true;
    st.limit_resets = Some(5.0);
    st.err_text = "x".into();
    let o = ClaudeOutcome::from_stream(&st, Some(3), "  tail \n");
    assert_eq!(
        o,
        ClaudeOutcome {
            session_started: true,
            result_seen: true,
            last_error: false,
            limit: true,
            limit_resets: Some(5.0),
            budget: false,
            err_text: "x".into(),
            exit_code: Some(3),
            stderr_tail: "tail".into(),
        }
    );
}

// ── CheckDone ──

fn check_state() -> EngineState {
    running(UnitPhase::Check, Inflight::Check)
}

#[test]
fn k1_pause_keeps_check() {
    let mut s = check_state();
    s.pending_stop = Some(StopWhy::Pause);
    assert_eq!(decide(&mut s, failed_check(1, "x", None)), paused_tail());
    assert_eq!(s.current.as_ref().unwrap().phase, UnitPhase::Check);
    assert_eq!((s.run, s.inflight), (RunState::Paused, Inflight::Idle));
}

#[test]
fn k2_stop() {
    let mut s = check_state();
    s.pending_stop = Some(StopWhy::Stop);
    let mut want = vec![end_stopped(1)];
    want.extend(stop_tail());
    assert_eq!(decide(&mut s, failed_check(1, "x", None)), want);
}

#[test]
fn k3_capped() {
    let mut s = check_state();
    s.pending_stop = Some(StopWhy::Capped(CapKind::Minutes));
    let out = decide(&mut s, check_passed());
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Capped, reason: capped_text("시간") }]);
}

#[test]
fn k4_passed() {
    let mut s = check_state();
    let out = decide(&mut s, check_passed());
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Done, reason: "판정 명령을 통과했습니다.".into() }]);
    assert_eq!((s.statuses[0], s.current.is_none()), (UnitStatus::Done, true));
}

#[test]
fn k5_retry_first() {
    let mut s = check_state();
    s.current.as_mut().unwrap().resume = true;
    let msg = checks::check_fail_message(1, "check 작업", "FAIL", None);
    let out = decide(&mut s, failed_check(1, "FAIL", None));
    assert_eq!(
        out,
        vec![
            Action::SaveUnit { n: 1 },
            board(&checks::check_fail_event(1, 0, 2, None)),
            Action::StartClaude { n: 1, resume: true, next: Next::Text(msg.clone()) },
        ]
    );
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.tries, c.phase, c.next.clone()), (1, UnitPhase::Model, Next::Text(msg)));
    assert_eq!(s.inflight, Inflight::Claude);
}

#[test]
fn k5_retry_timed_out() {
    let mut s = check_state();
    s.current.as_mut().unwrap().tries = 1;
    let msg = checks::check_fail_message(124, "check 작업", "slow", Some(600.0));
    let out = decide(&mut s, failed_check(124, "slow", Some(600.0)));
    assert_eq!(
        out,
        vec![
            Action::SaveUnit { n: 1 },
            board(&checks::check_fail_event(124, 1, 2, Some(600.0))),
            Action::StartClaude { n: 1, resume: false, next: Next::Text(msg) },
        ]
    );
    assert_eq!(s.current.as_ref().unwrap().tries, 2);
}

#[test]
fn k6_out_of_retries() {
    let mut s = check_state();
    s.current.as_mut().unwrap().tries = 2;
    let out = decide(&mut s, failed_check(1, "x", None));
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Failed, reason: checks::check_fail_reason(2, None) }]);
    assert_eq!(s.statuses[0], UnitStatus::Failed);
}

#[test]
fn k6_retries_zero() {
    let mut s = check_state();
    s.retries = 0;
    let out = decide(&mut s, failed_check(124, "x", Some(60.0)));
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Failed, reason: checks::check_fail_reason(0, Some(60.0)) }]);
}

// ── ActionDone ──

fn action_state() -> EngineState {
    running(UnitPhase::Action, Inflight::Action)
}

fn action_done(verdict: ActionVerdict) -> Event {
    Event::ActionDone { now: NOW, verdict }
}

#[test]
fn a1_pause() {
    let mut s = action_state();
    s.pending_stop = Some(StopWhy::Pause);
    assert_eq!(decide(&mut s, action_done(ActionVerdict::Ok)), paused_tail());
    assert_eq!(s.current.as_ref().unwrap().phase, UnitPhase::Action);
}

#[test]
fn a2_stop() {
    let mut s = action_state();
    s.pending_stop = Some(StopWhy::Stop);
    let mut want = vec![end_stopped(1)];
    want.extend(stop_tail());
    assert_eq!(decide(&mut s, action_done(ActionVerdict::Ok)), want);
}

#[test]
fn a3_capped() {
    let mut s = action_state();
    s.pending_stop = Some(StopWhy::Capped(CapKind::Minutes));
    let out = decide(&mut s, action_done(ActionVerdict::Ok));
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Capped, reason: "시간 상한에 닿아 멈췄습니다.".into() }]);
}

#[test]
fn a4_ok_starts_claude() {
    let mut s = action_state();
    let out = decide(&mut s, action_done(ActionVerdict::Ok));
    assert_eq!(out, vec![Action::SaveUnit { n: 1 }, Action::StartClaude { n: 1, resume: false, next: Next::First }]);
    assert_eq!((s.current.as_ref().unwrap().phase, s.inflight), (UnitPhase::Model, Inflight::Claude));
}

#[test]
fn a5_failed() {
    let mut s = action_state();
    let out = decide(&mut s, action_done(ActionVerdict::Failed("고정 단계 실패".into())));
    assert_eq!(out, vec![Action::EndUnit { n: 1, status: UnitStatus::Failed, reason: "고정 단계 실패".into() }]);
    assert_eq!(s.statuses[0], UnitStatus::Failed);
}

// ── Control ──

#[test]
fn r1_pause_claude_interrupts() {
    let mut s = claude_state();
    let out = decide(&mut s, control_ev("pause"));
    assert_eq!(out, vec![board("일시정지 요청을 받았습니다."), Action::Interrupt { target: Target::Claude }]);
    assert_eq!((s.pending_stop, s.inflight), (Some(StopWhy::Pause), Inflight::Claude));
}

#[test]
fn r1_pause_check_interrupts() {
    let mut s = check_state();
    let out = decide(&mut s, control_ev("pause"));
    assert_eq!(out, vec![board("일시정지 요청을 받았습니다."), Action::Interrupt { target: Target::Check }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Pause));
}

#[test]
fn r2_pause_in_limit_wait_keeps_phase() {
    let mut s = running(UnitPhase::LimitWait, Inflight::Wait(WaitWhy::Limit));
    s.run = RunState::WaitingLimit;
    s.current.as_mut().unwrap().wait_until = Some(NOW + 500.0);
    s.current.as_mut().unwrap().next = Next::Limit;
    let out = decide(&mut s, control_ev("pause"));
    let mut want = vec![board("일시정지 요청을 받았습니다.")];
    want.extend(paused_tail());
    assert_eq!(out, want);
    let c = s.current.as_ref().unwrap();
    assert_eq!((c.phase, c.wait_until, c.next.clone()), (UnitPhase::LimitWait, Some(NOW + 500.0), Next::Limit));
    assert_eq!((s.run, s.inflight), (RunState::Paused, Inflight::Idle));
}

#[test]
fn r2_pause_idle() {
    let mut s = state_with(vec![plan("작업", false, false)], vec![UnitStatus::Pending], None, Inflight::Wait(WaitWhy::Stopline), RunState::WaitingStopline);
    let out = decide(&mut s, control_ev("pause"));
    let mut want = vec![board("일시정지 요청을 받았습니다.")];
    want.extend(paused_tail());
    assert_eq!(out, want);
    assert_eq!((s.run, s.inflight), (RunState::Paused, Inflight::Idle));
}

#[test]
fn r3_stop_busy() {
    let mut s = claude_state();
    let out = decide(&mut s, control_ev("stop"));
    assert_eq!(out, vec![board("중단 요청을 받았습니다."), Action::Interrupt { target: Target::Claude }]);
    assert_eq!(s.pending_stop, Some(StopWhy::Stop));
    // 일시정지를 기다리는 중에 온 중단은 까닭만 바꾸고 다시 끊지 않는다
    let mut s = claude_state();
    s.pending_stop = Some(StopWhy::Pause);
    let out = decide(&mut s, control_ev("stop"));
    assert_eq!(out, vec![board("중단 요청을 받았습니다.")]);
    assert_eq!(s.pending_stop, Some(StopWhy::Stop));
}

#[test]
fn r4_stop_waiting_ends_unit() {
    let mut s = running(UnitPhase::Model, Inflight::Wait(WaitWhy::ClaudeError));
    let out = decide(&mut s, control_ev("stop"));
    let mut want = vec![board("중단 요청을 받았습니다."), end_stopped(1)];
    want.extend(stop_tail());
    assert_eq!(out, want);
    assert_eq!((s.run, s.statuses[0]), (RunState::Stopped, UnitStatus::Stopped));
}

fn restart_stop_tail(left: usize) -> Vec<Action> {
    vec![
        Action::MarkRestartStopped,
        board(&format!("멈추고 다시 출발: 사람이 요청해 여기서 멈춥니다. 남은 단위 {left}개는 출발 준비에서 다시 고릅니다.")),
        Action::Finish { state: RunState::Stopped, reason: control::RESTART_REASON.into() },
    ]
}

#[test]
fn r5_restart_after_unit() {
    let units = vec![plan("가", false, false), plan("나", false, false)];
    let mut s = state_with(units, vec![UnitStatus::Running, UnitStatus::Pending], Some(cur(1, UnitPhase::Model)), Inflight::Claude, RunState::Running);
    let p = RestartPlan::AfterUnit(1);
    let out = decide(&mut s, control_ev("restart_from_intake"));
    assert_eq!(out, vec![board(&control::restart_text(p, false, None)), Action::MarkRestart { plan: p }]);
    assert_eq!((s.restart_asked, s.restart_unit, s.pending_stop, s.run), (true, Some(1), None, RunState::Running));
}

#[test]
fn r5_restart_now_idle() {
    let units = vec![plan("가", false, false), plan("나", false, false)];
    let mut s = state_with(units, vec![UnitStatus::Done, UnitStatus::Pending], None, Inflight::Idle, RunState::Running);
    let mut want = vec![board(&control::restart_text(RestartPlan::Now, false, None)), Action::MarkRestart { plan: RestartPlan::Now }];
    want.extend(restart_stop_tail(1));
    assert_eq!(decide(&mut s, control_ev("restart_from_intake")), want);
    assert_eq!((s.restart_asked, s.run), (true, RunState::Stopped));
}

#[test]
fn r5_restart_now_in_stopline_wait() {
    let mut s = state_with(vec![plan("가", false, false)], vec![UnitStatus::Pending], None, Inflight::Wait(WaitWhy::Stopline), RunState::WaitingStopline);
    let mut want = vec![board(&control::restart_text(RestartPlan::Now, false, None)), Action::MarkRestart { plan: RestartPlan::Now }];
    want.extend(restart_stop_tail(1));
    assert_eq!(decide(&mut s, control_ev("restart_from_intake")), want);
    assert_eq!((s.run, s.inflight), (RunState::Stopped, Inflight::Idle));
}

#[test]
fn r5_restart_now_during_precheck() {
    let mut s = prechecking(RunState::WaitingPrecheck);
    let out = decide(&mut s, control_ev("restart_from_intake"));
    assert_eq!(
        out,
        vec![
            board(&control::restart_text(RestartPlan::Now, false, None)),
            Action::MarkRestart { plan: RestartPlan::Now },
            Action::Interrupt { target: Target::Precheck },
        ]
    );
    assert_eq!((s.restart_asked, s.pending_stop), (true, Some(StopWhy::Restart)));
    // 끊긴 확인이 끝나면 P3
    assert_eq!(decide(&mut s, precheck_failed("")), restart_stop_tail(1));
}

#[test]
fn r5_restart_already() {
    let mut s = running(UnitPhase::Model, Inflight::Claude);
    s.restart_asked = true;
    s.restart_unit = Some(1);
    let out = decide(&mut s, control_ev("restart_from_intake"));
    assert_eq!(out, vec![board(&control::restart_text(RestartPlan::Already, false, Some(1)))]);
    assert_eq!(s.pending_stop, None);
}

#[test]
fn r5_restart_while_paused_with_unit() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::Paused;
    let p = RestartPlan::AfterUnit(1);
    let out = decide(&mut s, control_ev("restart_from_intake"));
    assert_eq!(out, vec![board(&control::restart_text(p, true, None)), Action::MarkRestart { plan: p }]);
    assert_eq!((s.run, s.restart_unit), (RunState::Paused, Some(1)));
}

#[test]
fn r6_takeover_unsupported() {
    let mut s = claude_state();
    let out = decide(&mut s, control_ev("takeover"));
    assert_eq!(
        out,
        vec![board("터미널로 이어받기 요청을 받았습니다."), board("Windows 판은 아직 터미널로 이어받기를 하지 않아 넘깁니다.")]
    );
    assert_eq!((s.run, s.inflight, s.pending_stop), (RunState::Running, Inflight::Claude, None));
}

#[test]
fn r7_resume() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::Paused;
    s.current.as_mut().unwrap().next = Next::Paused;
    let before_current = s.current.clone();
    let out = decide(&mut s, control_ev("resume"));
    assert_eq!(
        out,
        vec![
            board("재개합니다. 멈춘 자리에서 같은 세션으로 이어 갑니다."),
            Action::SetState { state: RunState::Running, reason: "재개했습니다.".into(), wait_until: None },
        ]
    );
    assert_eq!((s.run, s.inflight, s.current.clone()), (RunState::Running, Inflight::Idle, before_current));
    assert_eq!(decide(&mut s, next_ev()), vec![Action::StartClaude { n: 1, resume: false, next: Next::Paused }]);
}

#[test]
fn r8_stop_while_paused() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::Paused;
    let mut want = vec![end_stopped(1)];
    want.extend(stop_tail());
    assert_eq!(decide(&mut s, control_ev("stop")), want);
    assert_eq!(s.run, RunState::Stopped);
}

#[test]
fn r9_stop_taken_over() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::TakenOver;
    let mut want = vec![end_stopped(1)];
    want.extend(stop_tail());
    assert_eq!(decide(&mut s, control_ev("stop")), want);
    assert_eq!(s.run, RunState::Stopped);
}

#[test]
fn r10_resume_while_running_text_only() {
    let mut s = claude_state();
    let before = s.clone();
    let out = decide(&mut s, control_ev("resume"));
    assert_eq!(out, vec![board("재개 요청을 받았지만 일시정지 상태가 아니라 그대로 갑니다.")]);
    assert_eq!(s, before);
    // 이미 일시정지면 일시정지도 글만
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    s.run = RunState::Paused;
    assert_eq!(decide(&mut s, control_ev("pause")), vec![board("이미 일시정지 상태입니다.")]);
}

#[test]
fn r10_unknown_action_text() {
    let mut s = claude_state();
    let before = s.clone();
    let out = decide(&mut s, control_ev("foo"));
    assert_eq!(out, vec![board("아직 엔진이 하지 못하는 요청(foo)이라 넘깁니다.")]);
    assert_eq!(s, before);
}

#[test]
fn control_after_end_noop() {
    let mut s = claude_state();
    s.run = RunState::Done;
    let before = s.clone();
    assert_eq!(decide(&mut s, control_ev("pause")), vec![]);
    assert_eq!(s, before);
}

// ── 맞지 않는 이벤트 ──

#[test]
fn exited_while_idle_noop() {
    let mut s = running(UnitPhase::Model, Inflight::Idle);
    let before = s.clone();
    assert_eq!(decide(&mut s, exited(outcome_ok())), vec![]);
    assert_eq!(s, before);
}

#[test]
fn check_done_while_claude_noop() {
    let mut s = claude_state();
    let before = s.clone();
    assert_eq!(decide(&mut s, check_passed()), vec![]);
    assert_eq!(s, before);
}

// ── 순수 도우미 ──

fn stream(result_seen: bool, open: bool, bg: usize, last_error: bool, last_line: f64) -> StreamState {
    let mut s = StreamState::new("sid", NOW);
    s.result_seen = result_seen;
    s.open = open;
    s.bg = bg;
    s.last_error = last_error;
    s.last_line = last_line;
    s
}

#[test]
fn turn_progress_table() {
    use TurnProgress::*;
    // ① 결과가 나왔고 열린 턴이 없고 백그라운드가 없으면 2초 조용한 뒤 끝
    let s = stream(true, false, 0, false, NOW);
    assert_eq!(turn_progress(&s, NOW + 1.9, 300.0, None, false, false), Continue);
    assert_eq!(turn_progress(&s, NOW + 2.0, 300.0, None, false, false), Done);
    // ① 백그라운드가 있어도 오류로 끝났으면 끝
    let s = stream(true, false, 2, true, NOW);
    assert_eq!(turn_progress(&s, NOW + 2.0, 300.0, None, false, false), Done);
    // 열린 턴이 있으면 끝이 아니다
    let s = stream(true, true, 0, false, NOW);
    assert_eq!(turn_progress(&s, NOW + 50.0, 300.0, None, false, false), Continue);
    // ② 백그라운드가 남았으면 bg_quiet 초 조용한 뒤 한 번 지운다
    let s = stream(true, false, 2, false, NOW);
    assert_eq!(turn_progress(&s, NOW + 299.0, 300.0, None, false, false), Continue);
    assert_eq!(turn_progress(&s, NOW + 300.0, 300.0, None, false, false), ClearBackground);
    // ③ 지운 뒤에는 지운 시각과 마지막 줄 중 늦은 쪽에서 10초
    assert_eq!(turn_progress(&s, NOW + 309.9, 300.0, Some(NOW + 300.0), false, false), Continue);
    assert_eq!(turn_progress(&s, NOW + 310.0, 300.0, Some(NOW + 300.0), false, false), Done);
    // 지운 뒤 마지막 줄이 더 늦으면 그 줄에서 센다
    let s = stream(true, false, 2, false, NOW + 305.0);
    assert_eq!(turn_progress(&s, NOW + 314.9, 300.0, Some(NOW + 300.0), false, false), Continue);
    assert_eq!(turn_progress(&s, NOW + 315.0, 300.0, Some(NOW + 300.0), false, false), Done);
    // ④ 프로세스가 끝났고 끝까지 읽었거나 3초 조용하면 끝
    let s = stream(false, false, 0, false, NOW);
    assert_eq!(turn_progress(&s, NOW, 300.0, None, true, true), Done);
    assert_eq!(turn_progress(&s, NOW + 2.9, 300.0, None, true, false), Continue);
    assert_eq!(turn_progress(&s, NOW + 3.0, 300.0, None, true, false), Done);
    // ⑤ 프로세스가 안 끝났으면 계속
    assert_eq!(turn_progress(&s, NOW + 100.0, 300.0, None, false, true), Continue);
}

#[test]
fn account_active_clamps() {
    assert_eq!(account_active(10.0, 100.0, 103.0), 13.0);
    assert_eq!(account_active(10.0, 100.0, 90.0), 10.0);
    assert_eq!(account_active(10.0, 100.0, 107.0), 15.0);
}

#[test]
fn account_weekly_table() {
    let m0 = WeeklyMeter::default();
    // 초기화 시각이 지났으면 그대로
    assert_eq!(account_weekly(m0, Some(0.3), Some(NOW), NOW), m0);
    assert_eq!(account_weekly(m0, Some(0.3), Some(NOW - 5.0), NOW), m0);
    // 사용률을 모르면 그대로
    assert_eq!(account_weekly(m0, None, Some(NOW + 100.0), NOW), m0);
    // 처음: 지금 값이 기준
    let m1 = account_weekly(m0, Some(0.20), Some(NOW + 100.0), NOW);
    assert_eq!(m1, WeeklyMeter { base: Some(0.20), last: Some(0.20), pp_done: 0.0, pp: 0.0 });
    // 오름: 기준에서 늘어난 %p
    let m2 = account_weekly(m1, Some(0.23), None, NOW);
    assert_eq!(m2, WeeklyMeter { base: Some(0.20), last: Some(0.23), pp_done: 0.0, pp: 3.0 });
    // 창이 초기화돼 줄었다: 쌓은 만큼을 굳히고 새 기준에서 센다
    let m3 = account_weekly(WeeklyMeter { base: Some(0.20), last: Some(0.50), pp_done: 0.0, pp: 30.0 }, Some(0.10), None, NOW);
    assert_eq!(m3, WeeklyMeter { base: Some(0.10), last: Some(0.10), pp_done: 30.0, pp: 30.0 });
    let m4 = account_weekly(m3, Some(0.12), None, NOW);
    assert_eq!(m4, WeeklyMeter { base: Some(0.10), last: Some(0.12), pp_done: 30.0, pp: 32.0 });
}

#[test]
fn next_text() {
    assert_eq!(Next::First.text(), None);
    assert_eq!(Next::Restart.text(), Some(MSG_RESTART));
    assert_eq!(Next::Paused.text(), Some(MSG_PAUSED));
    assert_eq!(Next::Limit.text(), Some(MSG_LIMIT));
    assert_eq!(Next::Error.text(), Some(MSG_ERROR));
    assert_eq!(Next::Text("고쳐 주세요".into()).text(), Some("고쳐 주세요"));
}

#[test]
fn enum_strings_roundtrip() {
    let runs = [
        (RunState::Starting, "starting"),
        (RunState::Running, "running"),
        (RunState::WaitingLimit, "waiting_limit"),
        (RunState::WaitingStopline, "waiting_stopline"),
        (RunState::WaitingPrecheck, "waiting_precheck"),
        (RunState::Paused, "paused"),
        (RunState::TakenOver, "taken_over"),
        (RunState::Stopped, "stopped"),
        (RunState::Done, "done"),
        (RunState::Error, "error"),
    ];
    for (v, text) in runs {
        assert_eq!((v.as_str(), RunState::parse(text)), (text, Some(v)));
        assert_eq!(v.ended(), control::run_ended(text));
    }
    assert_eq!(RunState::parse(""), None);
    assert_eq!(RunState::parse("nope"), None);

    let statuses = [
        (UnitStatus::Pending, "pending", "대기"),
        (UnitStatus::Running, "running", "진행 중"),
        (UnitStatus::Done, "done", "통과"),
        (UnitStatus::Failed, "failed", "미통과"),
        (UnitStatus::Capped, "capped", "상한에서 멈춤"),
        (UnitStatus::Skipped, "skipped", "건너뜀"),
        (UnitStatus::Stopped, "stopped", "중단"),
    ];
    for (v, text, label) in statuses {
        assert_eq!((v.as_str(), UnitStatus::parse(text), v.label()), (text, Some(v), label));
    }
    assert_eq!(UnitStatus::parse(""), None);

    let phases = [
        (UnitPhase::Action, "action"),
        (UnitPhase::Model, "model"),
        (UnitPhase::Check, "check"),
        (UnitPhase::LimitWait, "limit_wait"),
    ];
    for (v, text) in phases {
        assert_eq!((v.as_str(), UnitPhase::parse(text)), (text, Some(v)));
    }
    assert_eq!(UnitPhase::parse(""), None);

    let caps = [(CapKind::Minutes, "minutes", "시간"), (CapKind::Usd, "usd", "비용"), (CapKind::Weekly, "weekly", "주간 사용률")];
    for (v, text, label) in caps {
        assert_eq!((v.as_str(), v.label()), (text, label));
    }
    assert_eq!(Caps::default(), Caps { minutes: 120.0, usd: 100.0, weekly_pp: 5.0 });
}

#[test]
fn engine_state_new_defaults() {
    let s = EngineState::new("opus", vec![plan("가", false, false), plan("나", false, false)], 2, 5.0, Map::new(), Tz::Utc);
    assert_eq!((s.run, s.total(), s.inflight, s.pending_stop, s.restart_asked, s.restart_unit), (RunState::Starting, 2, Inflight::Idle, None, false, None));
    assert_eq!(s.statuses, vec![UnitStatus::Pending; 2]);
    assert!(s.current.is_none() && s.reason.is_empty());
}

// ── 이어 붙인 흐름 ──

#[test]
fn flow_two_units_retry_then_cap() {
    let mut second = plan("나", false, false);
    second.caps.minutes = 1.0;
    let mut s = EngineState::new("sonnet", vec![plan("가", false, false), second], 2, 5.0, Map::new(), Tz::Utc);
    let step = |s: &mut EngineState, e: Event| heads(&decide(s, e));

    assert_eq!(step(&mut s, Event::Boot { now: NOW, abnormal_recent: 1 }), ["Log", "SetState"]);
    assert_eq!(step(&mut s, Event::Next { now: NOW, usage: calm_usage() }), ["BeginUnit", "Log", "SetState", "StartClaude"]);
    assert_eq!(step(&mut s, exited(outcome_ok())), ["SaveUnit", "RunCheck"]);
    assert_eq!(step(&mut s, failed_check(1, "FAIL", None)), ["SaveUnit", "Log", "StartClaude"]);
    assert_eq!(step(&mut s, exited(outcome_ok())), ["SaveUnit", "RunCheck"]);
    assert_eq!(step(&mut s, check_passed()), ["EndUnit"]);
    assert_eq!(s.statuses, vec![UnitStatus::Done, UnitStatus::Pending]);
    assert_eq!(step(&mut s, Event::Next { now: NOW, usage: calm_usage() }), ["BeginUnit", "Log", "SetState", "StartClaude"]);
    assert_eq!(step(&mut s, tick(60.0, 0.0, 0.0, false)), ["Interrupt"]);
    assert_eq!(step(&mut s, exited(outcome_ok())), ["EndUnit"]);
    assert_eq!(s.statuses, vec![UnitStatus::Done, UnitStatus::Capped]);
    assert_eq!(step(&mut s, Event::Next { now: NOW, usage: calm_usage() }), ["Log", "Finish"]);
    assert_eq!(s.run, RunState::Done);
}
