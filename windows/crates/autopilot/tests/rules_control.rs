//! control.json 규칙(`rules::control`)과 계정 멈출 선 규칙(`rules::stopline`) 시험.
//! 원본: `Tests/AutopilotRulesTests/ControlTests.swift`(표 9+13+15 줄과 이름 시험 1) · `StoplineTests.swift`(표 15+9+5 줄).
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 함수(`control_outcome_mode`·재출발 함수)를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::control::{
    board_can_control, control_name, control_new, control_outcome, control_outcome_mode, is_restart_stop, reopen_intake,
    restart_plan, restart_stops_before, restart_text, run_ended, ControlMode, ControlOutcome, ControlStep,
    RestartPlan, CONTROL_ACTIONS, RESTART_REASON, TAKEN_OVER_STATE,
};
use claude_board_autopilot::rules::stopline::{show_five_hour, show_five_hour_usage, stopline_wait, usage_from_percent, when_text, Tz};
use serde_json::{Map, Value};

// ── control.json ─────────────────────────────────────

struct ControlNewRow {
    note: &'static str,
    json: Option<&'static str>, // control.json 글(None 이면 읽지 못함)
    mtime: f64,
    seen: Option<f64>,
    want: &'static str, // "action ts" 또는 "nil"
}

const CONTROL_NEW_ROWS: [ControlNewRow; 9] = [
    ControlNewRow { note: "읽지 못한 파일은 요청이 아니다", json: None, mtime: 9.0, seen: None, want: "nil" },
    ControlNewRow { note: "JSON 물체가 아니면 요청이 아니다", json: Some(r#""pause""#), mtime: 9.0, seen: None, want: "nil" },
    ControlNewRow { note: "action 이 글자가 아니면 요청이 아니다", json: Some(r#"{"action":3}"#), mtime: 9.0, seen: None, want: "nil" },
    ControlNewRow { note: "ts 가 없으면 파일 시각", json: Some(r#"{"action":"pause"}"#), mtime: 9.0, seen: None, want: "pause 9.0" },
    ControlNewRow { note: "ts 가 받은 것보다 크면 새 요청", json: Some(r#"{"action":"pause","ts":5.5}"#), mtime: 1.0, seen: Some(5.0), want: "pause 5.5" },
    ControlNewRow { note: "이미 받은 ts 와 같으면 다시 하지 않는다", json: Some(r#"{"action":"pause","ts":5.5}"#), mtime: 9.0, seen: Some(5.5), want: "nil" },
    ControlNewRow { note: "글자 ts 도 수로 읽는다", json: Some(r#"{"action":"stop","ts":"7"}"#), mtime: 1.0, seen: Some(6.0), want: "stop 7.0" },
    ControlNewRow { note: "수가 아닌 글자 ts 는 파일 시각", json: Some(r#"{"action":"pause","ts":"x"}"#), mtime: 9.0, seen: Some(8.0), want: "pause 9.0" },
    ControlNewRow { note: "받은 ts 가 없으면 0 보다 크기만 하면 된다", json: Some(r#"{"action":"resume","ts":10}"#), mtime: 1.0, seen: None, want: "resume 10.0" },
];

#[test]
fn control_new_rows() {
    for r in &CONTROL_NEW_ROWS {
        let json: Option<Value> = r.json.and_then(|s| serde_json::from_str(s).ok());
        let got = control_new(json.as_ref(), r.mtime, r.seen)
            .map(|(a, ts)| format!("{a} {ts:?}"))
            .unwrap_or_else(|| "nil".to_string());
        assert_eq!(got, r.want, "{}", r.note);
    }
}

struct OutcomeRow {
    note: &'static str,
    action: &'static str,
    paused: bool,
    step: ControlStep,
    text: Option<&'static str>,
}

const OUTCOME_ROWS: [OutcomeRow; 13] = [
    OutcomeRow { note: "도는 중 일시정지", action: "pause", paused: false, step: ControlStep::Pause, text: Some("일시정지 요청을 받았습니다.") },
    OutcomeRow { note: "도는 중 중단", action: "stop", paused: false, step: ControlStep::Stop, text: Some("중단 요청을 받았습니다.") },
    OutcomeRow { note: "도는 중 재개는 그대로", action: "resume", paused: false, step: ControlStep::None, text: Some("재개 요청을 받았지만 일시정지 상태가 아니라 그대로 갑니다.") },
    OutcomeRow { note: "⑫-8: 도는 중 터미널로 이어받기(전에는 넘김)", action: "takeover", paused: false, step: ControlStep::Takeover, text: Some("터미널로 이어받기 요청을 받았습니다.") },
    OutcomeRow { note: "도는 중 모르는 요청은 넘긴다", action: "rewind", paused: false, step: ControlStep::None, text: Some("아직 엔진이 하지 못하는 요청(rewind)이라 넘깁니다.") },
    OutcomeRow { note: "일시정지 중 재개", action: "resume", paused: true, step: ControlStep::Resume, text: Some("재개합니다. 멈춘 자리에서 같은 세션으로 이어 갑니다.") },
    OutcomeRow { note: "일시정지 중 중단은 엔진이 마치며 적는다", action: "stop", paused: true, step: ControlStep::Stop, text: None },
    OutcomeRow { note: "일시정지 중 일시정지", action: "pause", paused: true, step: ControlStep::None, text: Some("이미 일시정지 상태입니다.") },
    OutcomeRow { note: "⑫-8: 일시정지 중 돌려주기는 이어받은 상태가 아니라 넘김(전에는 모르는 요청)", action: "giveback", paused: true, step: ControlStep::None, text: Some("이어받은 상태가 아니라 돌려주기를 넘깁니다.") },
    OutcomeRow { note: "⑫-8: 일시정지 중 터미널로 이어받기", action: "takeover", paused: true, step: ControlStep::Takeover, text: Some("터미널로 이어받기 요청을 받았습니다.") },
    OutcomeRow { note: "일시정지 중 모르는 요청", action: "rewind", paused: true, step: ControlStep::None, text: Some("아직 엔진이 하지 못하는 요청(rewind)이라 넘깁니다.") },
    OutcomeRow { note: "5-4: 멈추고 다시 출발(전에는 넘김), 언제 멈출지는 restart_plan", action: "restart_from_intake", paused: false, step: ControlStep::Restart, text: None },
    OutcomeRow { note: "5-4: 일시정지 중 멈추고 다시 출발", action: "restart_from_intake", paused: true, step: ControlStep::Restart, text: None },
];

#[test]
fn control_outcome_rows() {
    for r in &OUTCOME_ROWS {
        let want = ControlOutcome { step: r.step, text: r.text.map(str::to_string) };
        assert_eq!(control_outcome(r.action, r.paused), want, "{}", r.note);
    }
}

struct BoardControlRow {
    note: &'static str,
    action: &'static str,
    state: &'static str,
    want: bool,
}

const BOARD_CONTROL_ROWS: [BoardControlRow; 15] = [
    BoardControlRow { note: "도는 실행에 일시정지", action: "pause", state: "running", want: true },
    BoardControlRow { note: "한도 대기에도 중단", action: "stop", state: "waiting_limit", want: true },
    BoardControlRow { note: "일시정지한 실행에 재개", action: "resume", state: "paused", want: true },
    BoardControlRow { note: "출발하지 않은(상태 없는) 실행에는 쓰지 않는다", action: "pause", state: "", want: false },
    BoardControlRow { note: "끝난 실행에는 쓰지 않는다", action: "stop", state: "done", want: false },
    BoardControlRow { note: "중단된 실행에는 쓰지 않는다", action: "resume", state: "stopped", want: false },
    BoardControlRow { note: "⑫-8: 도는 실행에 터미널로 이어받기(전에는 쓰지 않음)", action: "takeover", state: "running", want: true },
    BoardControlRow { note: "⑫-8: 이어받지 않은 실행에는 돌려주기를 쓰지 않는다", action: "giveback", state: "paused", want: false },
    BoardControlRow { note: "⑫-8: 이어받은 실행에 돌려주기", action: "giveback", state: "taken_over", want: true },
    BoardControlRow { note: "⑫-8: 이어받은 실행에 중단", action: "stop", state: "taken_over", want: true },
    BoardControlRow { note: "⑫-8: 이어받은 실행에 일시정지·재개·다시 이어받기는 쓰지 않는다", action: "resume", state: "taken_over", want: false },
    BoardControlRow { note: "⑫-8: 이어받은 실행에 다시 이어받기", action: "takeover", state: "taken_over", want: false },
    BoardControlRow { note: "모르는 요청은 쓰지 않는다", action: "rewind", state: "running", want: false },
    BoardControlRow { note: "5-4: 멈추고 다시 출발(전에는 쓰지 않음)", action: "restart_from_intake", state: "waiting_stopline", want: true },
    BoardControlRow { note: "5-4: 끝난 실행에는 멈추고 다시 출발도 쓰지 않는다", action: "restart_from_intake", state: "stopped", want: false },
];

#[test]
fn board_can_control_rows() {
    for r in &BOARD_CONTROL_ROWS {
        assert_eq!(board_can_control(r.action, r.state), r.want, "{}", r.note);
    }
}

#[test]
fn ended_states_and_action_names() {
    assert!(["done", "stopped", "error"].iter().all(|s| run_ended(s)));
    assert!(!["", "starting", "running", "paused", "waiting_limit", "waiting_stopline", "waiting_precheck"]
        .iter()
        .any(|s| run_ended(s)));
    let names: Vec<&str> = CONTROL_ACTIONS.iter().map(|a| control_name(a).unwrap_or("")).collect();
    assert_eq!(
        names,
        ["일시정지", "재개", "중단", "멈추고 다시 출발", "터미널로 이어받기", "자율주행에 돌려주기"]
    ); // 5-4 에서 넷째, ⑫-8 에서 둘
    assert!(!run_ended(TAKEN_OVER_STATE));
}

// ── 계정 멈출 선 ─────────────────────────────────────

const T: f64 = 1_793_000_000.0; // 2026-10-26 07:33:20 UTC

fn obj(pairs: &[(&str, f64)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::from(*v))).collect()
}

fn num(x: f64) -> String {
    format!("{x:?}") // Swift 의 Double 글자와 같다(1793000600.0)
}

struct StopWaitRow {
    note: &'static str,
    usage: Option<&'static [(&'static str, f64)]>, // None 이면 사용률을 모름
    line: &'static [(&'static str, f64)],
    want: String, // "go" 또는 "<until> <글>"
}

fn stop_wait_rows() -> Vec<StopWaitRow> {
    let row = |note, usage, line, want: String| StopWaitRow { note, usage, line, want };
    vec![
        row("사용률을 모르면 10분 뒤 다시", None, &[],
            format!("{} 계정 사용률을 알 수 없어 새 단위를 시작하지 않고 10분 뒤 다시 확인합니다.", num(T + 600.0))),
        row("빈 사용량은 0 으로 보고 지나간다", Some(&[]), &[], "go".into()),
        row("기본 멈출 선(주간 80%) 아래는 지나간다", Some(&[("seven_day", 0.79), ("five_hour", 0.5)]), &[], "go".into()),
        row("주간이 멈출 선과 같으면 기다린다(초기화 시각 + 2분)", Some(&[("seven_day", 0.8), ("seven_day_resets", T + 86_400.0)]), &[],
            format!("{} 계정 주간 사용률 80% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 27일 07:35 까지 기다립니다.", num(T + 86_400.0 + 120.0))),
        row("초기화 시각이 없으면 10분 + 2분 뒤", Some(&[("seven_day", 0.95)]), &[],
            format!("{} 계정 주간 사용률 95% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 26일 07:45 까지 기다립니다.", num(T + 720.0))),
        row("초기화 시각이 지난 창은 0 으로 본다", Some(&[("seven_day", 0.95), ("seven_day_resets", T - 1.0)]), &[], "go".into()),
        row("초기화 시각이 지금이면 지난 것", Some(&[("seven_day", 1.0), ("seven_day_resets", T)]), &[], "go".into()),
        row("낮춘 멈출 선(30%)", Some(&[("seven_day", 0.29), ("seven_day_resets", T + 30.0)]), &[("seven_day", 0.3)], "go".into()),
        row("낮춘 멈출 선에 닿음", Some(&[("seven_day", 0.3), ("seven_day_resets", T + 30.0)]), &[("seven_day", 0.3)],
            format!("{} 계정 주간 사용률 30% 가 멈출 선 30% 에 닿아 새 단위를 시작하지 않고 10월 26일 07:35 까지 기다립니다.", num(T + 150.0))),
        row("5시간 멈출 선(기본 95%)", Some(&[("seven_day", 0.1), ("five_hour", 0.95), ("five_hour_resets", T + 3600.0)]), &[],
            format!("{} 계정 5시간 사용률 95% 가 멈출 선 95% 에 닿아 새 단위를 시작하지 않고 10월 26일 08:35 까지 기다립니다.", num(T + 3720.0))),
        row("주간이 먼저", Some(&[("seven_day", 0.9), ("five_hour", 0.97)]), &[],
            format!("{} 계정 주간 사용률 90% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 26일 07:45 까지 기다립니다.", num(T + 720.0))),
        row("5시간 창이 초기화됐으면 0", Some(&[("five_hour", 0.97), ("five_hour_resets", T - 5.0)]), &[], "go".into()),
        row("멈출 선 100% 는 사용률 100% 에서만", Some(&[("seven_day", 0.99)]), &[("seven_day", 1.0), ("five_hour", 1.0)], "go".into()),
        row("5시간 칸이 없는 계정(Ultra)은 주간 선만 본다: 선 아래면 지나간다(2026-11)",
            Some(&[("seven_day", 0.26), ("seven_day_resets", T + 86_400.0)]), &[("seven_day", 0.8), ("five_hour", 0.95)], "go".into()),
        row("5시간 칸이 없는 계정(Ultra)도 주간 선에 닿으면 기다린다(2026-11)",
            Some(&[("seven_day", 0.85), ("seven_day_resets", T + 86_400.0)]), &[("seven_day", 0.8), ("five_hour", 0.95)],
            format!("{} 계정 주간 사용률 85% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 27일 07:35 까지 기다립니다.", num(T + 86_400.0 + 120.0))),
    ]
}

#[test]
fn stopline_wait_rows() {
    let rows = stop_wait_rows();
    assert_eq!(rows.len(), 15, "Swift stopWaitRows 줄 수");
    for r in &rows {
        let usage = r.usage.map(obj);
        let got = stopline_wait(usage.as_ref(), &obj(r.line), T, Tz::Utc)
            .map(|w| format!("{} {}", num(w.until), w.reason))
            .unwrap_or_else(|| "go".to_string());
        assert_eq!(got, r.want, "{}", r.note);
    }
}

struct PercentRow {
    note: &'static str,
    json: &'static str,
    want: &'static str, // "nil" 이거나 Swift 가 쓴 JSON 글자 그대로
}

const PERCENT_ROWS: [PercentRow; 9] = [
    PercentRow {
        note: "five_hour 가 없으면 주간만 푼다(2026-11 5시간은 있을 때만, 그 전에는 nil)",
        json: r#"{"seven_day":{"utilization":50}}"#,
        want: r#"{"at":1793000000,"from":"cache","seven_day":0.5}"#,
    },
    PercentRow {
        note: "5시간 한도가 없는 요금제(Ultra) 꼴: five_hour null, 5시간 칸은 넣지 않는다(2026-11)",
        json: r#"{"five_hour":null,"seven_day":{"utilization":26,"resets_at":"2026-11-05T06:59:59.717902+00:00"},"seven_day_opus":null,"limits":[{"kind":"weekly_all","percent":26}]}"#,
        want: r#"{"at":1793000000,"from":"cache","seven_day":0.26000000000000001,"seven_day_resets":1793861999}"#,
    },
    PercentRow { note: "두 창 모두 null 이면 사용률을 모른다(2026-11)", json: r#"{"five_hour":null,"seven_day":null}"#, want: "nil" },
    PercentRow {
        note: "빈 five_hour 만 있으면 사용률을 모른다(2026-11, 그 전에는 시각과 출처만이라 0 으로 지나갔다)",
        json: r#"{"five_hour":{}}"#,
        want: "nil",
    },
    PercentRow {
        note: "사용률 없이 시각만 있는 창도 모른다(2026-11)",
        json: r#"{"seven_day":{"resets_at":"2026-11-02T00:00:00Z"}}"#,
        want: "nil",
    },
    PercentRow {
        note: "주간 사용률이 null 이고 5시간만 있으면 5시간만(2026-11)",
        json: r#"{"five_hour":{"utilization":40},"seven_day":{"utilization":null}}"#,
        want: r#"{"at":1793000000,"five_hour":0.40000000000000002,"from":"cache"}"#,
    },
    PercentRow {
        note: "퍼센트를 비율로, 소수 초가 긴 시각도 읽는다",
        json: r#"{"five_hour":{"utilization":84.0,"resets_at":"2026-10-30T15:00:00.318890+00:00"}}"#,
        want: r#"{"at":1793000000,"five_hour":0.83999999999999997,"five_hour_resets":1793372400,"from":"cache"}"#,
    },
    PercentRow {
        note: "글자 퍼센트·주간 창",
        json: r#"{"five_hour":{"utilization":"12"},"seven_day":{"utilization":29,"resets_at":"2026-11-02T00:00:00Z"}}"#,
        want: r#"{"at":1793000000,"five_hour":0.12,"from":"cache","seven_day":0.28999999999999998,"seven_day_resets":1793577600}"#,
    },
    PercentRow {
        note: "못 읽는 시각은 뺀다",
        json: r#"{"five_hour":{"utilization":3,"resets_at":"bad"}}"#,
        want: r#"{"at":1793000000,"five_hour":0.029999999999999999,"from":"cache"}"#,
    },
];

// 정수든 소수든 같은 값이면 같게 비교하려고 모든 수를 f64 로 맞춘다(serde_json 은 1 과 1.0 을 다르게 본다)
fn normalize(v: Value) -> Value {
    match v {
        Value::Number(n) => Value::from(n.as_f64().unwrap_or(f64::NAN)),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

#[test]
fn usage_from_percent_rows() {
    assert_eq!(PERCENT_ROWS.len(), 9, "Swift percentRows 줄 수");
    for r in &PERCENT_ROWS {
        let j: Map<String, Value> = match serde_json::from_str(r.json) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        let got = usage_from_percent(&j, "cache", T).map(normalize);
        let want = (r.want != "nil").then(|| normalize(serde_json::from_str(r.want).expect("기대값 JSON")));
        assert_eq!(got, want, "{}", r.note);
    }
}

struct ShowFiveRow {
    note: &'static str,
    usage: Option<&'static [(&'static str, f64)]>,
    want: bool,
}

const SHOW_FIVE_ROWS: [ShowFiveRow; 5] = [
    ShowFiveRow { note: "아직 사용률을 읽지 못했으면 보인다(모름)", usage: None, want: true },
    ShowFiveRow { note: "시각만 있는 빈 사용량도 모름이라 보인다", usage: Some(&[("at", T)]), want: true },
    ShowFiveRow { note: "5시간 값이 있으면 보인다", usage: Some(&[("five_hour", 0.1), ("seven_day", 0.3)]), want: true },
    ShowFiveRow { note: "5시간만 읽었어도 보인다", usage: Some(&[("five_hour", 0.0)]), want: true },
    ShowFiveRow {
        note: "주간은 읽었는데 5시간 값이 없으면(5시간 한도가 없는 요금제) 숨긴다",
        usage: Some(&[("seven_day", 0.26), ("seven_day_resets", T + 30.0)]),
        want: false,
    },
];

#[test]
fn 오시간칸() {
    for r in &SHOW_FIVE_ROWS {
        let usage = r.usage.map(obj);
        assert_eq!(show_five_hour_usage(usage.as_ref()), r.want, "{}", r.note);
    }
}

#[test]
fn 오시간칸_한도목록() {
    assert!(show_five_hour(false, false));
    assert!(show_five_hour(true, true));
    assert!(!show_five_hour(true, false));
}

// ── 추가: Swift 시험에 없는 함수 ─────────────────────

#[test]
fn extra_outcome_when_taken_over() {
    let o = |a| control_outcome_mode(a, ControlMode::TakenOver);
    assert_eq!(o("giveback"), ControlOutcome { step: ControlStep::Giveback, text: None });
    assert_eq!(o("stop"), ControlOutcome { step: ControlStep::Stop, text: None });
    assert_eq!(o("restart_from_intake").step, ControlStep::Restart);
    assert_eq!(o("takeover").text.as_deref(), Some("이미 사람이 이어받았습니다."));
    assert_eq!(o("pause").step, ControlStep::None);
    assert_eq!(o("resume").step, ControlStep::None);
    assert_eq!(o("rewind").text.as_deref(), Some("아직 엔진이 하지 못하는 요청(rewind)이라 넘깁니다."));
}

#[test]
fn extra_restart_plan_and_text() {
    assert_eq!(restart_plan(&["done", "running", "pending"], false), RestartPlan::AfterUnit(2));
    assert_eq!(restart_plan(&["done", "pending"], false), RestartPlan::Now);
    assert_eq!(restart_plan::<&str>(&[], false), RestartPlan::Now);
    assert_eq!(restart_plan(&["running"], true), RestartPlan::Already);
    assert!(restart_text(RestartPlan::AfterUnit(2), false, None).contains("지금 단위(단위 2)를 마친 뒤 멈추고"));
    assert!(restart_text(RestartPlan::AfterUnit(2), true, None).contains("일시정지 중이라 재개하면 그 단위(단위 2)"));
    assert!(restart_text(RestartPlan::Now, false, None).contains("도는 단위가 없어 바로 멈춥니다."));
    assert_eq!(
        restart_text(RestartPlan::Already, false, Some(3)),
        "멈추고 다시 출발 요청을 이미 받았습니다(그 단위(단위 3)를 마친 뒤 멈춤)."
    );
    assert_eq!(restart_text(RestartPlan::Already, false, None), "멈추고 다시 출발 요청을 이미 받았습니다.");
}

#[test]
fn extra_restart_stop_and_reopen() {
    assert!(restart_stops_before(true, true));
    assert!(!restart_stops_before(true, false));
    assert!(!restart_stops_before(false, true));
    assert!(is_restart_stop("stopped", RESTART_REASON));
    assert!(!is_restart_stop("done", RESTART_REASON));
    assert!(!is_restart_stop("stopped", "사람이 중단했습니다."));
    assert!(reopen_intake("stopped", true, true, true, false, Some("/p")));
    assert!(!reopen_intake("running", true, true, true, false, Some("/p")));
    assert!(!reopen_intake("stopped", true, true, true, true, Some("/p")));
    assert!(!reopen_intake("stopped", true, true, true, false, Some("")));
    assert!(!reopen_intake("stopped", true, true, true, false, None));
    assert!(!reopen_intake("stopped", false, true, true, false, Some("/p")));
}

#[test]
fn extra_when_text() {
    assert_eq!(when_text(T, Tz::Utc), "10월 26일 07:33");
    assert_eq!(when_text(0.0, Tz::Utc), "1월 1일 00:00");
}
