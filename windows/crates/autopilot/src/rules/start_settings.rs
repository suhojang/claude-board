//! 출발 확인의 설정 줄(모델·멈출 선)을 그 자리에서 바꾸는 규칙. 원본: `Sources/AutopilotRules/StartSettings.swift`.
//! 고를 수 있는 값(객관식)과 바꾼 값의 확인, 출발 준비가 쓸 값의 차례(프로젝트 설정 → 지난 실행 → 기본)와 설정 줄 안내 글을 정한다.
//! 순수 규칙이다. `projects/<프로젝트>.json`·지난 실행의 `run.json` 을 읽고 쓰는 일은 바깥이 하고 이 규칙은 읽은 JSON 값만 받는다.
//! 모델은 `claude --model` 에 그대로 넘기는 값이고, 멈출 선은 백분율이 아니라 0~1 비율로 저장된다(주간 `seven_day`·5시간 `five_hour`).

use serde_json::Value;

/// 모델 선택지: 별명 셋. 원본: `apModelChoices`
pub const MODEL_CHOICES: [&str; 3] = ["opus", "sonnet", "haiku"];

/// 모델 고르기 목록. 지금 값이 목록 밖이면 앞에 둔다(빈 값이면 별명 셋만). 원본: `apModelOptions(current:)`
pub fn model_options(current: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(MODEL_CHOICES.len() + 1);
    if !current.is_empty() && !MODEL_CHOICES.contains(&current) {
        out.push(current.to_string());
    }
    out.extend(MODEL_CHOICES.iter().map(|m| m.to_string()));
    out
}

/// 멈출 선(백분율) 주간 선택지. 원본: `apStopWeekChoices`
pub const STOP_WEEK_CHOICES: [i64; 4] = [60, 70, 80, 90];
/// 멈출 선(백분율) 5시간 선택지. 원본: `apStopFiveChoices`
pub const STOP_FIVE_CHOICES: [i64; 3] = [80, 90, 95];

/// 멈출 선 고르기 목록: 정해 둔 값과 지금 값을 오름차순으로(지금 값이 이미 있으면 한 번). 원본: `apStopOptions(_:current:)`
pub fn stop_options(choices: &[i64], current: i64) -> Vec<i64> {
    let mut out = choices.to_vec();
    if !choices.contains(&current) {
        out.push(current);
    }
    out.sort();
    out
}

/// 모델 이름 꼴: 첫 글자는 영문 소문자·숫자, 나머지는 영문 소문자·숫자·`.`·`_`·`[`·`]`·`-`. 원본 정규식 `^[a-z0-9][a-z0-9._\[\]-]*$`
fn model_name_shape(model: &str) -> bool {
    let mut chars = model.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '[' | ']' | '-'))
}

/// 바꾼 설정의 고칠 것(없으면 빈 목록). 모델은 영문 소문자·숫자·`.-_[]` 꼴, 멈출 선은 0 초과 1 이하 비율(1~100%).
/// 순서: 모델, 주간, 5시간. 원본: `apSettingsProblems(model:week:five:)`
///
/// 원본의 정규식은 ICU 라 `$` 가 끝의 줄바꿈 앞에서도 맞는다(`"opus\n"` 통과). 여기는 끝까지 글자 하나하나 보므로 줄바꿈이 든 이름을 거절한다.
pub fn settings_problems(model: &str, week: f64, five: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if model.is_empty() {
        out.push("모델이 비었습니다".to_string());
    } else if !model_name_shape(model) {
        out.push(format!("모델 이름 꼴이 아닙니다: {model}"));
    }
    for (name, v) in [("주간", week), ("5시간", five)] {
        // NaN 은 어느 비교도 거짓이라 문제로 센다(원본과 같다)
        if !(v > 0.0 && v <= 1.0) {
            out.push(format!("멈출 선 {name} 값은 1~100% 입니다"));
        }
    }
    out
}

/// 출발 준비가 쓸 모델·멈출 선과 그 값이 어디서 왔는지. 원본: `APStartPick`
#[derive(Debug, Clone, PartialEq)]
pub struct StartPick {
    pub model: Option<String>,
    /// 주간 멈출 선(비율)
    pub week: Option<f64>,
    /// 5시간 멈출 선(비율)
    pub five: Option<f64>,
    /// `project` · `past` · `default`
    pub from: String,
}

/// JSON 숫자 읽기. 원본은 `as? NSNumber` 라 불리언도 숫자로 읽힌다(`true` → 1, `false` → 0); 글자·널·목록은 없음
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `stopline` 객체의 (`seven_day`, `five_hour`)
fn line(o: &Value) -> (Option<f64>, Option<f64>) {
    let s = &o["stopline"];
    (number(&s["seven_day"]), number(&s["five_hour"]))
}

fn string(o: &Value, key: &str) -> Option<String> {
    o[key].as_str().map(str::to_string)
}

fn non_empty_string(o: &Value, key: &str) -> Option<String> {
    string(o, key).filter(|s| !s.is_empty())
}

/// 출발 준비가 쓸 모델·멈출 선. 프로젝트 설정(`projects/<프로젝트>.json` 의 `model`·`stopline`, 출발 확인에서 바꾼 값)에
/// 하나라도 있으면 `project`(빠진 칸은 지난 실행 값으로 채운다), 없으면 지난 실행(이어 가기)에 하나라도 있을 때 `past`,
/// 그것도 없으면 모두 `None` 인 `default`(보드·엔진 기본값). 객체가 아닌 JSON 은 빈 객체로 본다. 원본: `apStartSettings(project:past:)`
///
/// 프로젝트에 값이 하나라도 있을 때 모델이 프로젝트에 없으면 지난 실행의 `model` 을 그대로 쓴다(빈 글자여도, 원본과 같다).
pub fn start_settings(project: &Value, past: &Value) -> StartPick {
    let pm = non_empty_string(project, "model");
    let (pw, pf) = line(project);
    let (qw, qf) = line(past);
    if pm.is_some() || pw.is_some() || pf.is_some() {
        return StartPick { model: pm.or_else(|| string(past, "model")), week: pw.or(qw), five: pf.or(qf), from: "project".to_string() };
    }
    let qm = non_empty_string(past, "model");
    let from = if qm.is_some() || qw.is_some() || qf.is_some() { "past" } else { "default" };
    StartPick { model: qm, week: qw, five: qf, from: from.to_string() }
}

/// 출발 확인 설정 줄의 안내 글. `from` 은 `start_settings` 의 값(`project`·`past`, 그 밖은 기본값으로 읽는다), `edited` 는 사람이 바꿨는가.
/// 원본: `apStartSettingsNote(from:edited:)`
pub fn start_settings_note(from: &str, edited: bool) -> &'static str {
    if edited {
        return "바꾼 값은 이 출발에 쓰고, 출발하면 프로젝트 설정에 남깁니다(다음 출발의 기본값).";
    }
    match from {
        "project" => "프로젝트 설정에 저장된 값입니다. 그 자리에서 바꿉니다.",
        "past" => "지난 실행의 실행 설정을 그대로 잇습니다. 그 자리에서 바꿉니다.",
        _ => "이어 갈 지난 실행도, 프로젝트 설정에 남긴 값도 없어 기본값입니다. 그 자리에서 바꿉니다.",
    }
}
