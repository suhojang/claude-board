//! 사용량 규칙(parse·cache·settle)의 공유 규칙 시험 표를 돌린다.
//!
//! 줄은 저장소 맨 위 `rules-table/usage.json` 이 소유하고(plan §2.2·2.4), 이 모듈은 각 rule 의
//! input 을 사용량 판단에 옮겨 기대(expect) 와 견준다. 시각 입력은 지금=0 기준 초이고, 한도의
//! `resets` 는 Limit 과 같은 밀리초 단위로 견준다. (R) 단계는 U1 이 잡을 대상이라 지금은 빨갛다.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::model::Limit;
use crate::rules_table::{Case, RuleFn};
use crate::usage::{self, Usage};

/// `rules-table/usage.json` 의 모든 줄을 돌린다. 실패한 id 는 run 이 한 번에 모아 보인다.
#[test]
fn usage_table() {
    crate::rules_table::run(
        "usage",
        &[
            ("parse", run_parse as RuleFn),
            ("cache", run_cache as RuleFn),
            ("settle", run_settle as RuleFn),
        ],
    );
}

/// 입력·기대에 모르는 키가 있으면 표를 고쳐야 하니 실패로 본다.
fn check_keys(value: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(obj) = value.as_object() else {
        return Err(format!("{what} 는 객체여야 한다"));
    };
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{what} 에 모르는 키가 있다: {key}"));
        }
    }
    Ok(())
}

/// 숫자 입력(초·밀리초)을 읽는다. 없거나 null 이면 없음, 다른 타입이면 표 결함이다.
fn number(value: &Value, key: &str) -> Result<Option<f64>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("input 의 {key} 는 숫자여야 한다")),
    }
}

/// 본문 입력을 바이트로 옮긴다. body 는 JSON 값 그대로 직렬화하고, body_raw 는 날 문자열이다.
fn body_bytes(case: &Case) -> Result<Option<Vec<u8>>, String> {
    if case.input.get("body").is_some() && case.input.get("body_raw").is_some() {
        return Err("body 와 body_raw 를 함께 둘 수 없다".to_string());
    }
    if let Some(raw) = case.input.get("body_raw") {
        let Some(raw) = raw.as_str() else {
            return Err("body_raw 는 문자열이어야 한다".to_string());
        };
        return Ok(Some(raw.as_bytes().to_vec()));
    }
    match case.input.get("body") {
        None | Some(Value::Null) => Ok(None),
        Some(body) => Ok(Some(body.to_string().into_bytes())),
    }
}

/// `expect.limits` 를 Limit 목록으로 옮긴다. null 은 `빈 목록`이 아니라 `해석 실패`를 뜻하니
/// run_parse 에서 따로 본다. 표에서는 화면에 보이는 네 정보(id·label·pct·resets)만 견준다.
fn expected_limits(value: &Value) -> Result<Vec<Limit>, String> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(rows) => rows
            .iter()
            .map(|row| {
                let obj = row
                    .as_object()
                    .ok_or("expect.limits 의 원소는 객체여야 한다")?;
                let text = |key: &str| -> Result<String, String> {
                    obj.get(key)
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .ok_or_else(|| format!("expect.limits 의 {key} 는 문자열이어야 한다"))
                };
                let resets = match obj.get("resets") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(
                        value
                            .as_f64()
                            .map(|ms| ms as i64)
                            .ok_or("expect.limits 의 resets 는 밀리초 숫자여야 한다")?,
                    ),
                };
                let pct = obj
                    .get("pct")
                    .and_then(Value::as_f64)
                    .ok_or("expect.limits 의 pct 는 숫자여야 한다")?;
                Ok(Limit {
                    id: text("id")?,
                    label: text("label")?,
                    short: String::new(),
                    tiny: String::new(),
                    pct,
                    resets,
                })
            })
            .collect(),
        _ => Err("expect.limits 는 null 이거나 배열이어야 한다".to_string()),
    }
}

/// 보이는 네 정보(id·label·pct·resets)만 견준다. short·tiny 는 표가 검사하는 자리가 아니다.
fn limits_eq(actual: &[Limit], expected: &[Limit]) -> Result<(), String> {
    if actual.len() != expected.len() {
        return Err(format!(
            "한도 개수가 다르다: 기대 {} 개, 실제 {} 개({})",
            expected.len(),
            actual.len(),
            limit_ids(actual)
        ));
    }
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        let same = actual.id == expected.id
            && actual.label == expected.label
            && (actual.pct - expected.pct).abs() < f64::EPSILON
            && actual.resets == expected.resets;
        if !same {
            return Err(format!(
                "한도가 다르다: 기대 id {}·label {}·pct {}·resets {:?}, 실제 id {}·label {}·pct {}·resets {:?}",
                expected.id, expected.label, expected.pct, expected.resets,
                actual.id, actual.label, actual.pct, actual.resets
            ));
        }
    }
    Ok(())
}

fn limit_ids(limits: &[Limit]) -> String {
    limits
        .iter()
        .map(|limit| limit.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// settle 의 `지금 보이는 한도` 기준값. 표에서는 한 쪽만 쓰니 한 자리에 둔다.
fn shown_limit() -> Limit {
    Limit {
        id: "5h".to_string(),
        label: "5시간".to_string(),
        short: "5시간".to_string(),
        tiny: "5h".to_string(),
        pct: 1.0,
        resets: None,
    }
}

/// 캐시를 쓰는지 알아보려 파일 자리가 필요하다. 매번 새 임시 디렉터리를 쓴다.
fn temp_dir() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cb-usage-table-{}", crate::util::short_id()));
    dir
}

fn run_parse(case: &Case) -> Result<(), String> {
    check_keys(&case.input, &["body", "body_raw"], "input")?;
    check_keys(&case.expect, &["limits"], "expect")?;
    let limits_value = case
        .expect
        .get("limits")
        .ok_or("expect 에 limits 가 없다")?;
    let expected = expected_limits(limits_value)?;
    let parsed = body_bytes(case)?.and_then(|body| usage::parse(&body));
    let Some(actual) = parsed else {
        return if limits_value.is_null() {
            Ok(())
        } else {
            Err("본문을 풀지 못했는데 기대에는 한도가 있다".to_string())
        };
    };
    if limits_value.is_null() {
        return Err(format!(
            "본문을 풀었는데 기대는 실패다({})",
            limit_ids(&actual)
        ));
    }
    limits_eq(&actual, &expected)
}

fn run_cache(case: &Case) -> Result<(), String> {
    check_keys(
        &case.input,
        &["modified", "trust_after", "fetching", "body", "body_raw"],
        "input",
    )?;
    check_keys(&case.expect, &["cached", "fetch"], "expect")?;
    let fetch_expect = case
        .expect
        .get("fetch")
        .and_then(Value::as_bool)
        .ok_or("expect 에 fetch (참거짓) 가 없다")?;
    let cached_expect = case.expect.get("cached").ok_or("expect 에 cached 가 없다")?;
    // trust_after 의 기본값은 전부 믿는 자리다(Usage::new 의 NEG_INFINITY 와 같다). 120 초는
    // 캐시 나이의 한계지 trust_after 가 아니니 섞지 않는다
    let trust_after = number(&case.input, "trust_after")?.unwrap_or(f64::NEG_INFINITY);
    let modified = number(&case.input, "modified")?;
    let fetching = case
        .input
        .get("fetching")
        .map(|value| value.as_bool().ok_or("input 의 fetching 은 참거짓이어야 한다"))
        .transpose()?
        .unwrap_or(false);
    let body = body_bytes(case)?;

    let written = modified.filter(|modified| *modified > trust_after);
    let body_for_check = if written.is_some() {
        body.as_deref()
    } else {
        None
    };
    let (limits_actual, fetch_actual) = usage::check_cache(written, body_for_check, fetching, 0.0);

    match (cached_expect, limits_actual) {
        (Value::Null, None) => {}
        (Value::Array(expected_ids), Some(actual_limits)) => {
            let actual_ids = limit_ids(&actual_limits);
            let expected_ids = expected_ids
                .iter()
                .map(|value| value.as_str().ok_or("expect.cached 의 원소는 문자열이어야 한다"))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            if actual_ids != expected_ids {
                return Err(format!(
                    "적용된 한도가 다르다: 기대 {}, 실제 {}",
                    expected_ids, actual_ids
                ));
            }
        }
        (Value::Null, Some(actual_limits)) => {
            return Err(format!(
                "한도를 적용하면 안 되는데 적용했다({})",
                limit_ids(&actual_limits)
            ));
        }
        (Value::Array(_), None) => {
            return Err("한도를 적용해야 하는데 캐시를 적용하지 않았다".to_string());
        }
        _ => return Err("expect.cached 는 null 이거나 id 배열이어야 한다".to_string()),
    }
    if fetch_actual != fetch_expect {
        return Err(format!(
            "받기가 {} 여야 하는데 {} 다",
            fetch_expect, fetch_actual
        ));
    }
    Ok(())
}

/// settle 한 줄을 돌린다: 받은 응답을 Usage 에 넣고 화면 한도·캐시 쓰기·실패 표시를 견준다.
/// 캐시 자리는 매번 새 임시 디렉터리에 둔다. 청소가 실패해도 흔적만 남고 판정에 영향이 없다.
fn run_settle(case: &Case) -> Result<(), String> {
    check_keys(&case.input, &["body", "body_raw", "current_empty"], "input")?;
    check_keys(&case.expect, &["applied", "cache_written", "failed"], "expect")?;
    let current_empty = case
        .input
        .get("current_empty")
        .map(|value| value.as_bool().ok_or("input 의 current_empty 는 참거짓이어야 한다"))
        .transpose()?
        .unwrap_or(true);
    let applied_expect = case
        .expect
        .get("applied")
        .and_then(Value::as_bool)
        .ok_or("expect 에 applied (참거짓) 가 없다")?;
    let written_expect = case
        .expect
        .get("cache_written")
        .and_then(Value::as_bool)
        .ok_or("expect 에 cache_written (참거짓) 가 없다")?;
    let failed_expect = case.expect.get("failed").ok_or("expect 에 failed 가 없다")?;

    let dir = temp_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("임시 디렉터리를 못 만들었다: {e}"))?;
    let mut usage = Usage::new(dir.join("usage-cache.json"));
    usage.limits = current_limits(current_empty);
    // 성공하면 앞의 실패 표시를 푸는지 보려고, 기대가 false 인 줄은 실패 표시를 켠 채 시작한다
    usage.failed = failed_expect.as_bool() == Some(false);
    let body = body_bytes(case)?;
    let parsed = body.as_deref().and_then(usage::parse);
    usage.finish(0, body, false);

    let checked = settle_expect(
        &usage,
        parsed.as_deref(),
        body_bytes(case)?.as_deref(),
        applied_expect,
        written_expect,
        failed_expect,
        &current_limits(current_empty),
    );
    let _ = fs::remove_dir_all(&dir);
    checked
}

/// 표의 current_empty 로 정한 `지금 보이는 한도`. 비지 않았으면 한도 하나를 심는다.
fn current_limits(current_empty: bool) -> Vec<Limit> {
    if current_empty {
        Vec::new()
    } else {
        vec![shown_limit()]
    }
}

/// apply 여부는 푼 한도와 화면의 한도가 같은가, 캐시 쓰기는 파일이 생겼는가, 실패 표시는
/// 기대(true·false) 이거나 건드리지 않았다는 뜻의 null 이다.
fn settle_expect(
    usage: &Usage,
    parsed: Option<&[Limit]>,
    body: Option<&[u8]>,
    applied_expect: bool,
    written_expect: bool,
    failed_expect: &Value,
    current: &[Limit],
) -> Result<(), String> {
    if applied_expect {
        let Some(expected) = parsed else {
            return Err("한도를 적용해야 하는데 본문을 풀지 못했다".to_string());
        };
        limits_eq(&usage.limits, expected)?;
    } else {
        limits_eq(&usage.limits, current)?;
    }
    let written = usage.cache.exists();
    if written != written_expect {
        return Err(format!(
            "캐시 쓰기가 {} 여야 하는데 {} 다",
            written_expect, written
        ));
    }
    // 캐시에는 받은 본문을 그대로 쓴다(상태줄이 같은 파일을 읽는다)
    if written && fs::read(&usage.cache).ok().as_deref() != body {
        return Err("캐시에 받은 본문이 그대로 쓰이지 않았다".to_string());
    }
    let want_failed = match failed_expect {
        Value::Bool(value) => *value,
        // null 은 건드리지 않았다는 뜻이다. 새로 만든 Usage 의 실패 표시는 false 에서 시작한다
        Value::Null => false,
        _ => return Err("settle expect 의 failed 는 true·false·null 이어야 한다".to_string()),
    };
    if usage.failed != want_failed {
        return Err(format!(
            "실패 표시가 {} 여야 하는데 {} 다",
            want_failed, usage.failed
        ));
    }
    Ok(())
}