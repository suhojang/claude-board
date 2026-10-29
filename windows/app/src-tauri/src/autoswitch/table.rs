//! 자동 전환 판단의 공유 규칙 시험 표 — `rules-table/autoswitch.json` 의 `decide`·`failed` 줄을 돌린다.
//!
//! 줄의 입력은 mac `AutoSwitchTests.swift` 의 DecideRow 와 같은 이름이다. 시각은 지금 = 0 기준 초라
//! 여기서 지금을 `NOW` 로 두고 더해 넣는다. 계정 이름 "A" 는 id "A|org" 다. 모르는 키는 실패로 모은다.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};

use super::{decide, failed, key, Move, Pause};
use crate::accounts::{Account, SwitchSide};
use crate::model::{Limit, PidPlace, Session};
use crate::rules_table::{self, Case};

/// 시험의 지금(유닉스 초) — mac 표와 같은 값
const NOW: f64 = 3_000_000.0;
/// 마지막 전환 기본값 — settle(40초)이 한참 지났다
const LONG_AGO: f64 = -1000.0;

/// 객체에서 쓸 수 있는 키만 있는지 본다. 모르는 키가 있으면 실패한다.
fn only_keys<'a>(what: &str, value: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let map = value.as_object().ok_or_else(|| format!("{what} 는 객체여야 한다"))?;
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{what} 에 모르는 키 '{key}' 가 있다"));
        }
    }
    Ok(map)
}

fn number(what: &str, value: &Value) -> Result<f64, String> {
    value.as_f64().ok_or_else(|| format!("{what} 는 숫자여야 한다"))
}

fn text<'a>(what: &str, value: &'a Value) -> Result<&'a str, String> {
    value.as_str().ok_or_else(|| format!("{what} 는 문자열이어야 한다"))
}

fn flag(map: &Map<String, Value>, name: &str) -> Result<bool, String> {
    match map.get(name) {
        None => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| format!("{name} 는 true/false 여야 한다")),
    }
}

/// 계정 이름 목록 필드(`expired`·`saved`·`resumed` 처럼 문자열 배열).
fn names(map: &Map<String, Value>, name: &str) -> Result<Option<Vec<String>>, String> {
    let Some(v) = map.get(name) else { return Ok(None) };
    let list = v.as_array().ok_or_else(|| format!("{name} 는 배열이어야 한다"))?;
    list.iter().map(|x| text(name, x).map(str::to_string)).collect::<Result<Vec<_>, _>>().map(Some)
}

/// 계정 이름 → id. 이름 하나("A")가 id "A|org".
fn account_id(name: &str) -> String {
    format!("{name}|org")
}

/// 계정 — `expired` 면 다시 로그인해야 한다(refresh_expires 가 1초 전).
fn account(name: &str, expired: bool) -> Account {
    let oauth = json!({"accountUuid": name, "organizationUuid": "org", "emailAddress": format!("{}@example.com", name.to_lowercase())});
    let expires = if expired { NOW - 1.0 } else { NOW + 86400.0 };
    Account::new(oauth.as_object().unwrap(), Some(expires)).expect("시험 계정은 accountUuid 가 있다")
}

/// 세션 — Default 가 없어 모든 자리를 적는다. `limit_at` 은 지금에서 초, 없으면 한도에 걸리지 않음.
fn session(value: &Value) -> Result<Session, String> {
    let map = only_keys("session", value, &["id", "state", "limit_at", "cwd", "message"])?;
    let id = text("session.id", map.get("id").ok_or("session.id 가 없다")?)?.to_string();
    let state = map.get("state").map_or(Ok("idle"), |v| text("session.state", v))?.to_string();
    let cwd = match map.get("cwd") {
        Some(v) => text("session.cwd", v)?.to_string(),
        None => format!("/p/{id}"),
    };
    let message = map.get("message").map_or(Ok("한도 메시지"), |v| text("session.message", v))?.to_string();
    let limit_s = match map.get("limit_at") {
        Some(v) => Some(NOW + number("session.limit_at", v)?),
        None => None,
    };
    Ok(Session {
        id,
        cwd,
        state,
        since: 0,
        updated: 0,
        headless: false,
        title: String::new(),
        topic: String::new(),
        message,
        name: None,
        ctx: None,
        brain: None,
        subagents: Vec::new(),
        shells: 0,
        paused: false,
        muted: false,
        limit_at: None,
        can_drive: true,
        place: PidPlace::default(),
        pid: 0,
        pid_ctime: None,
        file: Default::default(),
        project_dir: "/p".into(),
        resume_args: Vec::new(),
        since_s: NOW,
        updated_s: NOW,
        limit_s,
    })
}

/// 한도 — `resets` 는 지금에서 초, 없으면 모름. 라벨은 mac 표와 같다.
fn limit(value: &Value) -> Result<Limit, String> {
    let map = only_keys("limit", value, &["id", "pct", "resets"])?;
    let id = text("limit.id", map.get("id").ok_or("limit.id 가 없다")?)?.to_string();
    let label = match id.as_str() {
        "5h" => "5시간",
        "7d" => "주간 · 전체",
        "7d-Fable" => "주간 · Fable",
        other => other,
    }
    .to_string();
    let pct = number("limit.pct", map.get("pct").ok_or("limit.pct 가 없다")?)?;
    let resets = match map.get("resets") {
        Some(v) => Some(((NOW + number("limit.resets", v)?) * 1000.0) as i64),
        None => None,
    };
    Ok(Limit { id: id.clone(), label, short: id.clone(), tiny: id, pct, resets })
}

/// 쉬는 중 — 쉬기 시작한 계정 이름과 끝 시각(지금에서 초).
fn pause(value: &Value) -> Result<Pause, String> {
    let map = only_keys("pause", value, &["account", "until"])?;
    let account = text("pause.account", map.get("account").ok_or("pause.account 가 없다")?)?;
    let until = number("pause.until", map.get("until").ok_or("pause.until 가 없다")?)?;
    Ok(Pause { account: account_id(account), until: NOW + until })
}

/// 표시(계정 이름 → 지금에서 초)를 절대 시각 표(계정 id → 유닉스 초)로.
fn marks_in(name: &str, value: &Value) -> Result<BTreeMap<String, f64>, String> {
    let map = value.as_object().ok_or_else(|| format!("{name} 는 객체여야 한다"))?;
    map.iter().map(|(k, v)| Ok((account_id(k), NOW + number(name, v)?))).collect()
}

/// 판단 결과를 견주기 쉬운 글자로 — mac `describe` 와 같은 꼴.
fn describe(mv: &Move) -> String {
    let initial = |a: &Account| a.id.chars().next().map(String::from).unwrap_or_default();
    match mv {
        Move::Stay => "stay".to_string(),
        Move::SwitchTo { next, from, reason, stopped } => {
            format!("switch {}→{} [{reason}]{}", initial(from), initial(next), if *stopped { " stopped" } else { "" })
        }
        Move::NoCandidate(reason) => format!("none [{reason}]"),
        Move::UnknownCurrent(reason) => format!("unknown [{reason}]"),
    }
}

/// 표시 표를 글자로 — 실패 메시지에 보이기 쉽게 지금 기준 초로 되돌린다.
fn show_marks(marks: &BTreeMap<String, f64>) -> String {
    let shown: Vec<String> = marks.iter().map(|(k, v)| format!("{k}: {}", v - NOW)).collect();
    format!("{{{}}}", shown.join(", "))
}

fn decide_rule(case: &Case) -> Result<(), String> {
    let input = only_keys(
        "input",
        &case.input,
        &["sessions", "limits", "current", "saved", "expired", "full", "resumed", "last_switch", "pending", "switching", "pause"],
    )?;
    let expect = only_keys("expect", &case.expect, &["move", "marks", "resume"])?;

    let rows = |name: &str| -> Result<Vec<Value>, String> {
        match input.get(name) {
            None => Ok(Vec::new()),
            Some(v) => v.as_array().cloned().ok_or_else(|| format!("{name} 는 배열이어야 한다")),
        }
    };
    let sessions = rows("sessions")?.iter().map(session).collect::<Result<Vec<_>, _>>()?;
    let limits = rows("limits")?.iter().map(limit).collect::<Result<Vec<_>, _>>()?;

    let expired = names(input, "expired")?.unwrap_or_default();
    let build = |name: &str| account(name, expired.iter().any(|e| e == name));
    // current: 없으면 "A", null 이면 지금 계정을 모름
    let current = match input.get("current") {
        None => Some(build("A")),
        Some(Value::Null) => None,
        Some(v) => Some(build(text("current", v)?)),
    };
    let saved: Vec<Account> = names(input, "saved")?
        .unwrap_or_else(|| vec!["A".into(), "B".into(), "C".into()])
        .iter()
        .map(|n| build(n))
        .collect();
    let full = match input.get("full") {
        Some(v) => marks_in("full", v)?,
        None => BTreeMap::new(),
    };
    // resumed 는 세션 id 로 적고, 그 세션의 이어 가기 표지("세션 id@한도 시각")로 바꿔 넣는다
    let resumed_ids = names(input, "resumed")?.unwrap_or_default();
    let resumed: HashSet<String> = sessions.iter().filter(|s| s.limit_s.is_some() && resumed_ids.contains(&s.id)).map(key).collect();
    let last_switch = NOW + input.get("last_switch").map_or(Ok(LONG_AGO), |v| number("last_switch", v))?;
    let pending = flag(input, "pending")?;
    let switching = flag(input, "switching")?;
    let paused = match input.get("pause") {
        None | Some(Value::Null) => None,
        Some(v) => Some(pause(v)?),
    };

    let d = decide(&sessions, &limits, current.as_ref(), &saved, &full, &resumed, last_switch, pending, switching, paused.as_ref(), NOW);

    let mut wrong = Vec::new();
    let want_move = text("expect.move", expect.get("move").ok_or("expect.move 가 없다")?)?;
    let got_move = describe(&d.mv);
    if got_move != want_move {
        wrong.push(format!("move: 기대 '{want_move}' 실제 '{got_move}'"));
    }
    let want_marks = match expect.get("marks") {
        None | Some(Value::Null) => None,
        Some(v) => Some(marks_in("expect.marks", v)?),
    };
    if d.full != want_marks {
        let show = |m: &Option<BTreeMap<String, f64>>| m.as_ref().map_or("그대로(None)".to_string(), show_marks);
        wrong.push(format!("marks: 기대 {} 실제 {}", show(&want_marks), show(&d.full)));
    }
    let want_resume = names(expect, "resume")?.unwrap_or_default();
    let got_resume: Vec<String> = d.resume.iter().map(|s| s.id.clone()).collect();
    if got_resume != want_resume {
        wrong.push(format!("resume: 기대 {want_resume:?} 실제 {got_resume:?}"));
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join(" / "))
    }
}

fn failed_rule(case: &Case) -> Result<(), String> {
    let input = only_keys("input", &case.input, &["side", "target", "current", "full"])?;
    let expect = only_keys("expect", &case.expect, &["marks", "pause"])?;

    let side = match text("side", input.get("side").ok_or("side 가 없다")?)? {
        "target" => SwitchSide::Target,
        "other" => SwitchSide::Other,
        "busy" => SwitchSide::Busy,
        other => return Err(format!("모르는 side '{other}'")),
    };
    let target = account(text("target", input.get("target").ok_or("target 이 없다")?)?, false);
    let current = account(text("current", input.get("current").ok_or("current 가 없다")?)?, false);
    let full = match input.get("full") {
        Some(v) => marks_in("full", v)?,
        None => BTreeMap::new(),
    };

    let (marks, paused) = failed(side, &target, &current, &full, NOW);

    let mut wrong = Vec::new();
    let want_marks = marks_in("expect.marks", expect.get("marks").ok_or("expect.marks 가 없다")?)?;
    if marks != want_marks {
        wrong.push(format!("marks: 기대 {} 실제 {}", show_marks(&want_marks), show_marks(&marks)));
    }
    let want_pause = match expect.get("pause") {
        None | Some(Value::Null) => None,
        Some(v) => Some(pause(v)?),
    };
    let same = match (&paused, &want_pause) {
        (None, None) => true,
        (Some(a), Some(b)) => a.account == b.account && a.until == b.until,
        _ => false,
    };
    if !same {
        let show = |p: &Option<Pause>| p.as_ref().map_or("없음".to_string(), |p| format!("{} 를 {}초까지", p.account, p.until - NOW));
        wrong.push(format!("pause: 기대 {} 실제 {}", show(&want_pause), show(&paused)));
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join(" / "))
    }
}

#[test]
fn autoswitch_table() {
    rules_table::run("autoswitch", &[("decide", decide_rule), ("failed", failed_rule)]);
}
