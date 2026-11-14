//! 출발 확인 설정 줄(모델·멈출 선) 규칙(`rules::start_settings`) 시험.
//! 원본: `Tests/AutopilotRulesTests/StartTests.swift` 의 `SettingsTests` 묶음(`@Suite("자율주행 출발 확인: 설정 줄 바꾸기")`, 시험 넷:
//! 표·고를것·차례·안내글). 같은 파일의 승격 차례·지시·원문 지우기·미룬 몫 시험은 각자의 규칙 WP 가 옮긴다.
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 경계를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::start_settings::{
    model_options, settings_problems, start_settings, start_settings_note, stop_options, STOP_FIVE_CHOICES, STOP_WEEK_CHOICES,
};
use serde_json::Value;

struct SettingsRow {
    #[allow(dead_code)]
    note: &'static str,
    model: &'static str,
    week: f64,
    five: f64,
    want: Vec<&'static str>,
}

fn settings_rows() -> Vec<SettingsRow> {
    vec![
        SettingsRow { note: "별명과 기본 멈출 선", model: "opus", week: 0.8, five: 0.95, want: vec![] },
        SettingsRow { note: "전체 모델 이름도 꼴이면 된다", model: "claude-sonnet-4-6[1m]", week: 0.6, five: 1.0, want: vec![] },
        SettingsRow { note: "빈 모델", model: "", week: 0.8, five: 0.95, want: vec!["모델이 비었습니다"] },
        SettingsRow { note: "빈칸이 든 모델", model: "opus 4", week: 0.8, five: 0.95, want: vec!["모델 이름 꼴이 아닙니다: opus 4"] },
        SettingsRow {
            note: "멈출 선 0%·101%",
            model: "haiku",
            week: 0.0,
            five: 1.01,
            want: vec!["멈출 선 주간 값은 1~100% 입니다", "멈출 선 5시간 값은 1~100% 입니다"],
        },
    ]
}

fn ints(v: &[i64]) -> Vec<i64> {
    v.to_vec()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Swift `o(_:)` — JSON 글을 객체로 읽고, 읽지 못하거나 객체가 아니면 빈 객체
fn o(s: &str) -> Value {
    match serde_json::from_str::<Value>(s) {
        Ok(v) if v.is_object() => v,
        _ => serde_json::json!({}),
    }
}

#[test]
fn 표_길이() {
    assert_eq!(settings_rows().len(), 5, "Swift 설정 표는 5줄이다");
}

/// Swift `표` (arguments: settingsRows)
#[test]
fn 표() {
    for r in settings_rows() {
        assert_eq!(settings_problems(r.model, r.week, r.five), r.want, "{}", r.note);
    }
}

/// Swift `고를것`
#[test]
fn 고를것() {
    assert_eq!(model_options("haiku"), strings(&["opus", "sonnet", "haiku"]));
    assert_eq!(model_options("claude-opus-4-7"), strings(&["claude-opus-4-7", "opus", "sonnet", "haiku"]));
    assert_eq!(stop_options(&STOP_WEEK_CHOICES, 80), ints(&[60, 70, 80, 90]));
    assert_eq!(stop_options(&STOP_WEEK_CHOICES, 85), ints(&[60, 70, 80, 85, 90]));
}

fn order_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (r#"{}"#, r#"{}"#, "default nil nil nil"),
        (r#"{}"#, r#"{"model":"haiku","stopline":{"seven_day":0.8,"five_hour":0.95}}"#, "past haiku 0.8 0.95"),
        (r#"{"model":"opus"}"#, r#"{"model":"haiku","stopline":{"seven_day":0.8,"five_hour":0.95}}"#, "project opus 0.8 0.95"),
        (r#"{"stopline":{"seven_day":0.7}}"#, r#"{"model":"haiku","stopline":{"seven_day":0.8,"five_hour":0.95}}"#, "project haiku 0.7 0.95"),
        (r#"{"model":"","promotion":[]}"#, r#"{"model":"sonnet"}"#, "past sonnet nil nil"),
    ]
}

/// Swift 시험이 값을 글로 찍는 꼴: `"\(from) \(model ?? "nil") \(week) \(five)"`
fn pick_text(project: &str, past: &str) -> String {
    let p = start_settings(&o(project), &o(past));
    let num = |v: Option<f64>| v.map_or("nil".to_string(), |x| format!("{x}"));
    format!("{} {} {} {}", p.from, p.model.as_deref().unwrap_or("nil"), num(p.week), num(p.five))
}

/// Swift `차례` (arguments: 다섯 줄)
#[test]
fn 차례() {
    assert_eq!(order_rows().len(), 5, "Swift 차례 표는 5줄이다");
    for (project, past, want) in order_rows() {
        assert_eq!(pick_text(project, past), want, "project={project} past={past}");
    }
}

/// Swift `안내글` (arguments: 네 줄)
#[test]
fn 안내글() {
    let rows = [
        ("default", false, "이어 갈 지난 실행도, 프로젝트 설정에 남긴 값도 없어 기본값입니다. 그 자리에서 바꿉니다."),
        ("past", false, "지난 실행의 실행 설정을 그대로 잇습니다. 그 자리에서 바꿉니다."),
        ("project", false, "프로젝트 설정에 저장된 값입니다. 그 자리에서 바꿉니다."),
        ("default", true, "바꾼 값은 이 출발에 쓰고, 출발하면 프로젝트 설정에 남깁니다(다음 출발의 기본값)."),
    ];
    assert_eq!(rows.len(), 4, "Swift 안내글 표는 4줄이다");
    for (from, edited, want) in rows {
        assert_eq!(start_settings_note(from, edited), want, "from={from} edited={edited}");
    }
}

// ---- 추가: Swift 시험에 없는 경계 ----

#[test]
fn extra_choices_constants_match_swift() {
    assert_eq!(STOP_WEEK_CHOICES, [60, 70, 80, 90]);
    assert_eq!(STOP_FIVE_CHOICES, [80, 90, 95]);
    assert_eq!(stop_options(&STOP_FIVE_CHOICES, 95), ints(&[80, 90, 95]));
    assert_eq!(stop_options(&STOP_FIVE_CHOICES, 85), ints(&[80, 85, 90, 95]));
    // 목록 밖의 지금 값이 가장 작거나 큰 경우도 오름차순
    assert_eq!(stop_options(&STOP_WEEK_CHOICES, 50), ints(&[50, 60, 70, 80, 90]));
    assert_eq!(stop_options(&STOP_WEEK_CHOICES, 100), ints(&[60, 70, 80, 90, 100]));
}

#[test]
fn extra_empty_current_model_shows_only_aliases() {
    assert_eq!(model_options(""), strings(&["opus", "sonnet", "haiku"]));
    assert_eq!(model_options("opus"), strings(&["opus", "sonnet", "haiku"]));
}

#[test]
fn extra_model_shape_boundaries() {
    let ok = |m: &str| settings_problems(m, 0.8, 0.95).is_empty();
    assert!(ok("a"));
    assert!(ok("0"));
    assert!(ok("claude-sonnet-5-5"));
    assert!(ok("a.b_c-d[1m]"));
    assert!(!ok("-opus"), "첫 글자가 -");
    assert!(!ok(".opus"), "첫 글자가 .");
    assert!(!ok("[1m]"), "첫 글자가 [");
    assert!(!ok("Opus"), "대문자");
    assert!(!ok("opus/4"), "슬래시");
    assert!(!ok("오퍼스"), "한글");
    assert!(!ok("opus\n"), "줄바꿈(원본 ICU 정규식은 끝의 줄바꿈을 통과시킨다 — 이 이식은 거절한다)");
}

#[test]
fn extra_stopline_edges() {
    let none = |w: f64, f: f64| settings_problems("opus", w, f).is_empty();
    assert!(none(1.0, 1.0), "100% 는 된다");
    assert!(none(f64::MIN_POSITIVE, 0.01));
    assert!(!none(-0.1, 0.95));
    assert!(!none(f64::NAN, 0.95), "NaN 은 문제");
    assert!(!none(0.8, f64::INFINITY));
    assert_eq!(
        settings_problems("", 2.0, 0.0),
        vec!["모델이 비었습니다", "멈출 선 주간 값은 1~100% 입니다", "멈출 선 5시간 값은 1~100% 입니다"],
        "순서: 모델, 주간, 5시간"
    );
}

#[test]
fn extra_start_settings_fills_missing_project_fields_from_past() {
    // 프로젝트에 5시간 선만 있으면 모델·주간은 지난 실행에서
    assert_eq!(
        pick_text(r#"{"stopline":{"five_hour":0.9}}"#, r#"{"model":"sonnet","stopline":{"seven_day":0.6,"five_hour":0.95}}"#),
        "project sonnet 0.6 0.9"
    );
    // 프로젝트 값이 있는데 지난 실행에도 없으면 비어 있다
    assert_eq!(pick_text(r#"{"model":"opus"}"#, r#"{}"#), "project opus nil nil");
    // 지난 실행에 stopline 만 있는 경우
    assert_eq!(pick_text(r#"{}"#, r#"{"stopline":{"five_hour":0.9}}"#), "past nil nil 0.9");
}

#[test]
fn extra_start_settings_empty_past_model_passes_through_when_project_has_value() {
    // 원본: 프로젝트에 값이 있고 모델이 없으면 지난 실행 model 을 비어 있어도 그대로 쓴다(past 갈래는 빈 글자를 없음으로 본다)
    let p = start_settings(&o(r#"{"stopline":{"seven_day":0.7}}"#), &o(r#"{"model":""}"#));
    assert_eq!((p.from.as_str(), p.model.as_deref(), p.week, p.five), ("project", Some(""), Some(0.7), None));
    let p = start_settings(&o(r#"{}"#), &o(r#"{"model":""}"#));
    assert_eq!((p.from.as_str(), p.model, p.week, p.five), ("default", None, None, None));
}

#[test]
fn extra_start_settings_loose_types() {
    // 객체가 아닌 JSON 은 빈 객체와 같다
    assert_eq!(start_settings(&Value::Null, &serde_json::json!([1, 2])).from, "default");
    // 숫자가 아닌 값·객체가 아닌 stopline 은 없음
    assert_eq!(pick_text(r#"{"stopline":{"seven_day":"0.7","five_hour":null}}"#, r#"{}"#), "default nil nil nil");
    assert_eq!(pick_text(r#"{"stopline":[0.7]}"#, r#"{}"#), "default nil nil nil");
    // model 이 글자가 아니면 없음
    assert_eq!(pick_text(r#"{"model":5}"#, r#"{}"#), "default nil nil nil");
    // 원본은 `as? NSNumber` 라 불리언도 숫자로 읽는다
    assert_eq!(pick_text(r#"{"stopline":{"seven_day":true}}"#, r#"{}"#), "project nil 1 nil");
    // 정수 JSON 도 비율로 읽는다(Swift 는 "1.0" 으로 찍고 Rust 는 "1" 로 찍는다 — 값은 같다)
    let p = start_settings(&o(r#"{"stopline":{"five_hour":1}}"#), &o("{}"));
    assert_eq!(p.five, Some(1.0));
}

#[test]
fn extra_note_unknown_from_reads_as_default() {
    assert_eq!(start_settings_note("", false), start_settings_note("default", false));
    assert_eq!(start_settings_note("anything", false), start_settings_note("default", false));
    assert_eq!(start_settings_note("project", true), start_settings_note("past", true));
}
