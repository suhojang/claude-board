//! 계정 멈출 선 규칙(설계 4장 "사용률"). 원본: `Sources/AutopilotRules/Stopline.swift`.
//! 사용률 판정(멈출 선 견주기, 기다릴 때와 그 글)과 퍼센트 꼴 사용량 풀기만 한다.
//! 사용률 읽기(stream·캐시·사용량 API)·상태 바꾸기·기다리기는 엔진에 남는다.

use claude_board_core::clock::{self, LocalTime};
use serde_json::{Map, Value};

// 사용량 값의 수 — 엔진 num() 과 같다(수, NSNumber 처럼 불리언, 수로 읽히는 글자). 원본: `apStopNum`
fn stop_num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// ISO 8601 시각(초). 소수 초 자릿수가 많으면(`…00.318890+00:00`) 떼고 읽는다. 원본: `apISODate`
pub fn iso_date(s: &str) -> Option<f64> {
    let dot = s
        .char_indices()
        .find_map(|(i, c)| (c == '.' && s[i + 1..].starts_with(|d: char| d.is_ascii_digit())).then_some(i));
    match dot {
        Some(dot) => {
            let digits = s[dot + 1..].bytes().take_while(u8::is_ascii_digit).count();
            clock::iso_date(&format!("{}{}", &s[..dot], &s[dot + 1 + digits..]))
        }
        None => clock::iso_date(s),
    }
}

/// 사용량 API 응답·보드와 상태줄이 함께 쓰는 캐시(`/tmp/.claude-usage-cache.json` 꼴
/// `{five_hour:{utilization 84.0, resets_at}, seven_day:{…}}`) → 엔진 run.json usage 꼴
/// (`{at, from, five_hour 0.84, five_hour_resets, seven_day, seven_day_resets}`). 퍼센트를 비율로 맞춘다.
/// 5시간 한도가 없는 요금제(Ultra)는 `five_hour` 가 null 로 온다. 객체인 창만 넣고 없는 창은 칸을 넣지 않는다.
/// 두 창 모두 사용률(`utilization`)을 읽지 못하면 None(사용률을 모름). 원본: `apUsageFromPercent`
pub fn usage_from_percent(j: &Map<String, Value>, from: &str, at: f64) -> Option<Value> {
    let mut u = Map::new();
    u.insert("at".into(), Value::from((at * 1000.0).round() / 1000.0));
    u.insert("from".into(), Value::from(from));
    let mut read = false;
    for key in ["five_hour", "seven_day"] {
        let Some(w) = j.get(key).and_then(Value::as_object) else { continue };
        if let Some(v) = stop_num(w.get("utilization")) {
            u.insert(key.into(), Value::from(v / 100.0));
            read = true;
        }
        if let Some(r) = w.get("resets_at").and_then(Value::as_str).and_then(iso_date) {
            u.insert(format!("{key}_resets"), Value::from(r));
        }
    }
    read.then_some(Value::Object(u))
}

/// 5시간 칸(사용률 막대·멈출 선 고르기·status 줄)을 보일까 — 사용률을 읽었는데(`known`) 5시간 값이 없으면
/// 그 계정은 5시간 한도가 없는 요금제라 숨긴다. 아직 읽지 못했으면 보인다(모름). 원본: `apShowFiveHour(known:hasFive:)`
pub fn show_five_hour(known: bool, has_five: bool) -> bool {
    !known || has_five
}

/// run.json usage 꼴로 — 주간이나 5시간 사용률 가운데 하나라도 수면 읽은 것. 원본: `apShowFiveHour(usage:)`
pub fn show_five_hour_usage(usage: Option<&Map<String, Value>>) -> bool {
    let five = stop_num(usage.and_then(|u| u.get("five_hour"))).is_some();
    show_five_hour(five || stop_num(usage.and_then(|u| u.get("seven_day"))).is_some(), five)
}

/// 글로 적는 시각의 시간대. 규칙은 시스템 시계를 직접 부르지 않으므로 호출자가 고른다(시험은 `Utc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tz {
    Utc,
    /// 이 기계의 지역 시간대
    Local,
}

fn utc_time(t: f64) -> LocalTime {
    let secs = t.floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // 그레고리력 날짜 계산(Howard Hinnant 의 civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    LocalTime {
        year: (yoe + era * 400 + i64::from(month <= 2)) as i32,
        month,
        day: (doy - (153 * mp + 2) / 5 + 1) as u32,
        hour: (rem / 3600) as u32,
        minute: (rem % 3600 / 60) as u32,
        second: (rem % 60) as u32,
    }
}

/// 엔진 when() 과 같은 꼴(`M월 d일 HH:mm`, 한국어). 원본: `apWhenText`
pub fn when_text(t: f64, tz: Tz) -> String {
    let l = match tz {
        Tz::Utc => utc_time(t),
        Tz::Local => clock::local(t),
    };
    format!("{}월 {}일 {:02}:{:02}", l.month, l.day, l.hour, l.minute)
}

/// 멈출 선에 걸려 기다릴 때 — 다시 볼 시각과 진행 기록·run.json reason 의 글. 원본: `APStoplineWait`
#[derive(Debug, Clone, PartialEq)]
pub struct StoplineWait {
    pub until: f64,
    pub reason: String,
}

/// 새 단위를 시작해도 되는가(엔진 stoplineGate 의 판정). usage 는 run.json usage 꼴(비율, None 이면 사용률을 모름),
/// stopline 은 고정 설정의 멈출 선(`{seven_day 0.80, five_hour 0.95}` 이 기본). 초기화 시각이 지난 창은 0 으로 본다.
/// 주간이 먼저, 그다음 5시간. 넘으면 초기화 시각(지났거나 없으면 10분 뒤) + 2분까지. 사용률을 모르면 10분 뒤 다시.
/// usage 에 5시간 칸이 없으면(5시간 한도가 없는 요금제, `usage_from_percent`) 5시간 선은 보지 않고 주간 선만 본다(판정은 그대로).
/// 시작해도 되면 None. 원본: `apStoplineWait`
pub fn stopline_wait(
    usage: Option<&Map<String, Value>>,
    stopline: &Map<String, Value>,
    now: f64,
    tz: Tz,
) -> Option<StoplineWait> {
    let l7 = stop_num(stopline.get("seven_day")).unwrap_or(0.80);
    let l5 = stop_num(stopline.get("five_hour")).unwrap_or(0.95);
    let Some(u) = usage else {
        return Some(StoplineWait {
            until: now + 600.0,
            reason: "계정 사용률을 알 수 없어 새 단위를 시작하지 않고 10분 뒤 다시 확인합니다.".to_string(),
        });
    };
    // 초기화 시각이 지났으면 그 창은 0 으로 본다
    let window = |rate: &str, resets: &str| {
        let r = stop_num(u.get(resets)).unwrap_or(0.0);
        let v = if r > 0.0 && r <= now { 0.0 } else { stop_num(u.get(rate)).unwrap_or(0.0) };
        (v, r)
    };
    let (v7, r7) = window("seven_day", "seven_day_resets");
    let (v5, r5) = window("five_hour", "five_hour_resets");
    let wait = |label: &str, v: f64, limit: f64, resets: f64| {
        let until = (if resets > now { resets } else { now + 600.0 }) + 120.0;
        StoplineWait {
            until,
            reason: format!(
                "계정 {label} 사용률 {}% 가 멈출 선 {}% 에 닿아 새 단위를 시작하지 않고 {} 까지 기다립니다.",
                (v * 100.0).round() as i64,
                (limit * 100.0).round() as i64,
                when_text(until, tz)
            ),
        }
    };
    if v7 >= l7 {
        return Some(wait("주간", v7, l7, r7));
    }
    if v5 >= l5 {
        return Some(wait("5시간", v5, l5, r5));
    }
    None
}
