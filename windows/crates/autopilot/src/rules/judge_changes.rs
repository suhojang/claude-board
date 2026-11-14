//! 판정 차이(출발 확인). 원본: `Sources/AutopilotRules/Misses.swift` 375~422줄
//! (`APJudgeChange`·`apJudgeChanges`·`apLineChange`·`apScenarioTextChanges`).
//!
//! 지난 판(대기열 정의)과 지금의 판정 견주기 — 단위 id 로 짝지어 판정 명령과 시나리오 목록(경로·짝지은 수용 기준)을 비교하고,
//! 두 초안의 시나리오 글자 차이를 더한 줄·뺀 줄 수로 낸다. 시험: `tests/rules_misses_3.rs`(원본 `CritiqueTests.swift` 의 `차이`).
//!
//! Swift ↔ Rust 로 달라진 점:
//! - Swift `[String: Any]`(JSON 값)는 `serde_json::Value` 다. 배열 항목 가운데 사전이 아닌 것은 빠진다(Swift `[[String: Any]]` 에는 없는 꼴을 빠짐으로 흉내 낸다).
//!   시나리오 목록(`scenarios`) 안의 사전이 아닌 항목도 같은 규약으로 빠진다.
//! - 글자 수와 앞 글자 빼기(`count`·`dropFirst`)는 글자 묶음(Character) 수 근사 `formats::grapheme_len` 을 따른다.
//!   키는 "<단위 id>/<파일>" 꼴이라 한글 단위 id 도 글자 묶음 수로 센다.

use crate::rules::formats::grapheme_len;
use serde_json::Value;
use std::collections::BTreeMap;

/// 판정 차이 한 항목. 원본: `APJudgeChange`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeChange {
    pub unit: String,
    pub title: String,
    pub field: String, // check · scenarios
    pub old: String,
    pub new: String,
}

// 시나리오 목록을 한 줄 글자로 — "파일(짝지은 수용 기준)" 조각을 쉼표로 잇는다. 원본: `apJudgeChanges` 안의 `scen`
fn scen(d: &Value) -> String {
    let mut pieces: Vec<String> = Vec::new();
    for s in d.get("scenarios").and_then(Value::as_array).into_iter().flatten() {
        if !s.is_object() {
            continue; // 사전이 아닌 항목은 빠진다(Swift `[[String: Any]]` 규약)
        }
        let file = s.get("file").and_then(Value::as_str).unwrap_or("");
        let mut piece = file.to_string();
        if let Some(criterion) = s.get("criterion").and_then(Value::as_str) {
            piece.push('(');
            piece.push_str(criterion);
            piece.push(')');
        }
        pieces.push(piece);
    }
    pieces.join(", ")
}

// 지난 판(대기열 정의)과 지금의 판정 견주기 — 단위 id 로 짝지어 판정 명령과 시나리오 목록(경로·짝지은 수용 기준). 원본: `apJudgeChanges`
pub fn judge_changes(old: &[Value], new: &[Value]) -> Vec<JudgeChange> {
    let mut changes: Vec<JudgeChange> = Vec::new();
    for n in new {
        let Some(id) = n.get("id").and_then(Value::as_str) else { continue };
        let Some(o) = old.iter().find(|o| o.get("id").and_then(Value::as_str) == Some(id)) else { continue };
        let title = n.get("title").and_then(Value::as_str).unwrap_or(id);
        let old_check = o.get("check").and_then(Value::as_str).unwrap_or("");
        let new_check = n.get("check").and_then(Value::as_str).unwrap_or("");
        if old_check != new_check {
            changes.push(JudgeChange {
                unit: id.to_string(),
                title: title.to_string(),
                field: "check".to_string(),
                old: old_check.to_string(),
                new: new_check.to_string(),
            });
        }
        let old_scen = scen(o);
        let new_scen = scen(n);
        if old_scen != new_scen {
            changes.push(JudgeChange {
                unit: id.to_string(),
                title: title.to_string(),
                field: "scenarios".to_string(),
                old: old_scen,
                new: new_scen,
            });
        }
    }
    changes
}

// 시나리오 글이 바뀐 것 — 줄 단위로 더한 줄·뺀 줄 수(같은 줄은 짝지어 뺌). 원본: `apLineChange`
pub fn line_change(old: &str, new: &str) -> (usize, usize) {
    let mut pool: BTreeMap<&str, usize> = BTreeMap::new();
    for line in old.split('\n') {
        *pool.entry(line).or_default() += 1;
    }
    let mut add = 0usize;
    for line in new.split('\n') {
        match pool.get_mut(line) {
            Some(c) if *c > 0 => {
                *c -= 1;
            }
            _ => {
                add += 1;
            }
        }
    }
    let del: usize = pool.values().sum();
    (add, del)
}

// 앞 글자 빼기 — 글자 묶음 n 개를 앞에서 뺀다. 원본: `apScenarioTextChanges` 의 `dropFirst`
fn skip_graphemes(s: &str, n: usize) -> &str {
    if n == 0 {
        return s;
    }
    for (ix, _) in s.char_indices() {
        if grapheme_len(&s[..ix]) == n {
            return &s[ix..];
        }
    }
    ""
}

// 시나리오 글이 바뀐 것(판정 고치기 차례: 앞 초안과 지금 초안의 같은 시나리오 파일). 키 "<단위 id>/<파일>" → 글.
// 두 쪽에 다 있고 글이 다른 것만(없어진 시나리오는 apJudgeChanges 의 시나리오 목록·약해짐 검사가 본다). 원본: `apScenarioTextChanges`
pub fn scenario_text_changes(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
    titles: &BTreeMap<String, String>,
) -> Vec<JudgeChange> {
    let mut changes: Vec<JudgeChange> = Vec::new();
    for (key, now) in new {
        let Some(previous) = old.get(key) else { continue };
        if previous == now {
            continue;
        }
        let unit = key.split('/').find(|p| !p.is_empty()).unwrap_or("");
        let (add, del) = line_change(previous, now);
        changes.push(JudgeChange {
            unit: unit.to_string(),
            title: titles.get(unit).map(String::as_str).unwrap_or(unit).to_string(),
            field: format!("scenario:{}", skip_graphemes(key, grapheme_len(unit) + 1)),
            old: format!("{del}줄 뺌"),
            new: format!("{add}줄 더함"),
        });
    }
    changes
}
