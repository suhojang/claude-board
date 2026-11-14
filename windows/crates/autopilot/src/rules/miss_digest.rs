//! 출발 준비 때 확인된 지적(①②)을 출발한 실행의 기록으로 옮기고(prep_misses), 세션에 넘길 요약(miss_digest)과
//! 세트 이어 쓰기를 다시 돌릴 조건(set_rewrite)을 둔다.
//!
//! 원본: `Sources/AutopilotRules/Misses.swift` 136~209줄(`apPrepMisses`·`apMissDigestRows`·`apMissDigestChars`·
//! `apMissKeepPerProject`·`apMissDigest`·`apSetRewrite`). 줄 형과 자르기(`Miss`·`MissSignal`·`miss_label`·
//! `clip_miss_why_at`·`MISS_WHY_MAX`)는 misses.rs(청크 1/5)의 것을 갖다 쓴다 — 다시 정의하지 않는다.
//!
//! 원본 대응 규칙(misses.rs 청 1/5 과 같다):
//! - Swift `[String: Any]` 자료는 `serde_json::Value` 로(열쇠 이름은 원본과 같다).
//!   `[[String: Any]]`·`[String]` 밑으로 내려가기는 배열 원소가 모두(사전·글)일 때만 목록을 내고,
//!   하나라도 아니면 통째로 빈 목록을 낸다(Swift 조건 캐스트가 그렇다).
//! - `answers` 매개바뀜은 원본에서도 읽지 않는다(시그니처를 그대로 두는 자리).
//! - 글자 수(`count`)는 `formats::grapheme_len` 이다.

use crate::rules::formats::grapheme_len;
use crate::rules::misses::{clip_miss_why_at, miss_label, Miss, MissSignal, MISS_WHY_MAX};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// 출발 준비 하나의 단위(이어 갈 단위 목록의 줄). 원본: Misses.swift `apPrepMisses` 의 튜플 `(n, id, sourceKey, title)`
#[derive(Debug, Clone, PartialEq)]
pub struct PrepUnit {
    /// 단위 번호(engine/unit-<n>)
    pub n: i64,
    /// 단위 id
    pub id: String,
    /// 합칠 쪽 쌍 표의 쌍 열쇠
    pub source_key: Option<String>,
    pub title: String,
}

/// `prep_misses` 의 받는 대꾸 — 원본 `apPrepMisses` 매개바뀜과 이름·순서까지 같다. 원본: Misses.swift `apPrepMisses`
#[derive(Debug, Clone)]
pub struct PrepMissIn<'a> {
    pub project: &'a str,
    pub run: &'a str,
    /// prerun.json(probe_history·probe_cards)
    pub prerun: Option<&'a Value>,
    /// critic.json(rounds[].results)
    pub critic: Option<&'a Value>,
    /// 출발 카드 답 — 원본에서도 읽지 않는다(시그니처 유지용)
    pub answers: &'a Value,
    pub units: &'a [PrepUnit],
    pub set_id: Option<&'a str>,
    /// 적은 시각(초)
    pub at: f64,
}

/// `[[String: Any]]` 밑으로 내려가기 — 배열이어야 하고 원소가 하나라도 객체가 아니면 통째로 빈 목록.
/// 원본: `as? [[String: Any]] ?? []`
fn dict_array(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) if a.iter().all(Value::is_object) => a.iter().collect(),
        _ => Vec::new(),
    }
}

/// `[String]` 밑으로 내려가기 — 배열이어야 하고 원소가 하나라도 글이 아니면 통째로 빈 목록.
/// 원본: `as? [String] ?? []`(misses.rs 청 1/5 의 비공개 도움이와 같은 꼴 — 거긴 비공개라 여기도 둔다)
fn string_array(v: Option<&Value>) -> Vec<&str> {
    match v {
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// 출발 준비 때 확인된 지적(①②)을 출발한 실행의 기록으로(엔진 start 뒤 syncRun). 같은 준비 안에서 고친 것은 fixed, 아니면 open.
/// prerun: prerun.json(probe_cards), critic: critic.json(rounds[].results), answers: 출발 카드 답.
/// 원본: Misses.swift `apPrepMisses`
pub fn prep_misses(i: &PrepMissIn) -> Vec<Miss> {
    let mut out: Vec<Miss> = Vec::new();
    let units = i.units;
    // 앞 차례(고치기 전)에 확인됐다가 마지막 차례에 사라진 지적은 고친 것
    let rounds = dict_array(i.critic.and_then(|c| c.get("rounds")));
    let last_results = rounds.last().and_then(|rd| rd.get("results"));
    let last_critic: BTreeSet<String> = dict_array(last_results)
        .iter()
        .filter(|r| r.get("verdict").and_then(Value::as_str) == Some("confirmed"))
        .filter_map(|r| {
            r.get("unit").and_then(Value::as_str)
                .map(|u| format!("{u}|{}", r.get("judge").and_then(Value::as_str).unwrap_or("")))
        })
        .collect();
    for (k, rd) in rounds.iter().enumerate() {
        for r in dict_array(rd.get("results")) {
            if r.get("verdict").and_then(Value::as_str) != Some("confirmed") {
                continue;
            }
            let Some(uid) = r.get("unit").and_then(Value::as_str) else { continue };
            let Some(u) = units.iter().find(|u| u.id == uid) else { continue };
            let Some(fid) = r.get("id").and_then(Value::as_str) else { continue };
            let judge = r.get("judge").and_then(Value::as_str);
            let key = format!("{uid}|{}", judge.unwrap_or(""));
            let last = k == rounds.len() - 1;
            if !last && last_critic.contains(&key) {
                continue; // 마지막 차례에도 남은 것은 그 줄로 한 번만
            }
            let why = clip_miss_why_at(
                &format!(
                    "비판 세션 반례가 판정을 통과했습니다: {}",
                    r.get("why").and_then(Value::as_str).unwrap_or("")
                ),
                MISS_WHY_MAX,
            );
            let mut m = Miss::new(
                &format!("{}/prep/critic/{}/{}", i.run, k, fid),
                i.project,
                i.run,
                Some(u.n),
                Some(uid),
                u.source_key.as_deref(),
                MissSignal::Critic,
                judge,
                None,
                &why,
                i.set_id,
                i.at,
                &u.title,
            );
            // state: (!last && !lastCritic.contains(key)) ? "fixed" : "open" — 위 continue 로 !last 면 !contains 가 보장된다(원본 식 그대로)
            m.state = if !last && !last_critic.contains(&key) { "fixed".to_string() } else { "open".to_string() };
            out.push(m);
        }
    }
    let history = dict_array(i.prerun.and_then(|p| p.get("probe_history")));
    let now = dict_array(i.prerun.and_then(|p| p.get("probe_cards")));
    let now_keys: BTreeSet<&str> = now.iter().filter_map(|c| c.get("id").and_then(Value::as_str)).collect();
    // history 의 각 차례 cards 에 지금 차례(now)를 이어 붙인 차례 목록
    let mut all: Vec<(usize, Vec<&Value>)> = history.iter().map(|rd| dict_array(rd.get("cards"))).enumerate().collect();
    all.push((history.len(), now));
    for (k, cs) in all {
        for c in cs {
            let Some(cid) = c.get("id").and_then(Value::as_str) else { continue };
            let Some(uid) = c.get("unit_id").and_then(Value::as_str) else { continue };
            let Some(u) = units.iter().find(|u| u.id == uid) else { continue };
            let in_history = k < history.len();
            let fixed = in_history && !now_keys.contains(cid);
            if in_history && now_keys.contains(cid) {
                continue; // 마지막 차례에도 남은 것은 그 줄로 한 번만
            }
            let why = clip_miss_why_at(
                &format!("반대 탐침({})이 판정을 통과했습니다", string_array(c.get("probes")).join("·")),
                MISS_WHY_MAX,
            );
            let mut m = Miss::new(
                &format!("{}/prep/probe/{}/{}", i.run, k, cid),
                i.project,
                i.run,
                Some(u.n),
                Some(uid),
                u.source_key.as_deref(),
                MissSignal::Probe,
                Some(c.get("file").and_then(Value::as_str).unwrap_or("check")),
                None,
                &why,
                i.set_id,
                i.at,
                &u.title,
            );
            if fixed {
                m.state = "fixed".to_string();
            }
            out.push(m);
        }
    }
    out
}

/// 세션에 넘길 요약의 줄 수 상한(프로젝트마다 크기 상한 안에서). 원본: Misses.swift `apMissDigestRows`
pub const MISS_DIGEST_ROWS: usize = 30;
/// 세션에 넘길 요약의 글자 수 상한. 원본: Misses.swift `apMissDigestChars`
pub const MISS_DIGEST_CHARS: usize = 6000;
/// 프로젝트마다 남겨 둘 놓침 줄 수(기록 저장소 쪽에서 쓴다). 원본: Misses.swift `apMissKeepPerProject`
pub const MISS_KEEP_PER_PROJECT: usize = 500;

/// 세션에 넘길 요약: 열린 것 먼저, 최근 것 먼저, 판정 쪽·지시 쪽을 나눠 줄마다 한 줄. 줄 수·글자 수 상한(프로젝트마다 크기 상한 안에서).
/// 원본: Misses.swift `apMissDigest(_:_:)`(기본 인자는 MISS_DIGEST_ROWS·MISS_DIGEST_CHARS)
pub fn miss_digest(rows: &[Miss], max_rows: usize, max_chars: usize) -> String {
    let mut order: Vec<&Miss> = rows.iter().collect();
    order.sort_by(|a, b| {
        let a_open = a.state == "open";
        let b_open = b.state == "open";
        if a_open != b_open {
            if a_open {
                Ordering::Less // 열린 것 먼저
            } else {
                Ordering::Greater
            }
        } else {
            // 최근 것 먼저, 같으면 id 차례로(Swift `a.created != b.created` 는 NaN 이 아니라 참이 아니면 같다 — partial_cmp 의 나머지는 같다로)
            b.created.partial_cmp(&a.created).unwrap_or(Ordering::Equal).then(a.id.cmp(&b.id))
        }
    });
    let mut out: Vec<String> = Vec::new();
    let mut chars = 0usize;
    let mut shown = 0usize;
    for side in ["judge", "instruction"] {
        let part: Vec<&Miss> = order.iter().copied().filter(|m| m.side() == side).collect();
        if part.is_empty() {
            continue;
        }
        let head = if side == "judge" { "판정 쪽:" } else { "지시 쪽:" };
        out.push(head.to_string());
        chars += grapheme_len(head) + 1;
        for m in part {
            if shown >= max_rows {
                break;
            }
            let state_tag =
                if m.state == "open" { String::new() } else if m.state == "fixed" { " · 고침".to_string() } else { " · 씀".to_string() };
            let shown_title = if m.title.is_empty() { m.unit_id.as_deref().unwrap_or("") } else { &m.title };
            let mut line = format!("- [{}{}] {}", miss_label(m.signal), state_tag, shown_title);
            if let Some(j) = &m.judge {
                line.push_str(&format!(" · {}", clip_miss_why_at(j, 120)));
            }
            line.push_str(&format!(" · {}", m.why));
            let line_len = grapheme_len(&line) + 1;
            if chars + line_len > max_chars {
                break;
            }
            out.push(line);
            chars += line_len;
            shown += 1;
        }
    }
    let rest = rows.len() - shown;
    if rest > 0 {
        out.push(format!("(그 밖에 {}건은 줄였습니다)", rest));
    }
    out.join("\n")
}

/// 세트 이어 쓰기를 다시 돌릴까(설계 2장 "세트 기록과 이어 쓰기"): 하네스가 그대로이고 새 요구·판정 놓침이 없으면 그대로 쓴다(모델 호출 0).
/// 원본: Misses.swift `apSetRewrite`
pub fn set_rewrite(harness_changed: bool, new_request: bool, open_misses: i64) -> (bool, Vec<String>) {
    let mut r: Vec<String> = Vec::new();
    if harness_changed {
        r.push("하네스가 출발 때와 달라졌습니다".to_string());
    }
    if new_request {
        r.push("새 요구가 있습니다".to_string());
    }
    if open_misses > 0 {
        r.push(format!("판정 놓침 {open_misses}건이 있습니다"));
    }
    (!r.is_empty(), r)
}
