//! 판정 놓침 되먹임(5단계 ⑪-3, 설계 2장 "③ 판정 놓침 되먹임"·"세트 기록과 이어 쓰기", 5장 7절 judge_misses).
//! 판정이 통과시켰지만 틀렸던 것(판정 쪽)과 지시가 모델을 멈추게 하거나 헤매게 한 자리(지시 쪽)를 실행 기록 저장소에 기계로 짝지어 남긴다.
//! 신호(판정 쪽): 판정을 통과한 단위를 합치기·승격 화면에서 버림(discard), 판정을 통과했는데 미룬 몫이 남음(next_start, 판정이 보지 않은 일),
//!   ①② 에서 확인된 지적(probe·critic), 승격 뒤 되돌림(revert, 다듬기 ⑬-2 부터 — 단위 커밋 기록과 짝짓기는 unit_commits.rs).
//! 신호(지시 쪽): 권한 거부(denial), 묻기 도구 거부(asked), 판정을 지나지 못한 단위의 보류(parked), decisions_applied 근거가 지시·카드가 아닌 저장소 규칙(decision_basis).
//! 버리기 화면에서 까닭을 묻지 않는다(끝난 뒤 묻지 않는다는 원칙). 다음 가져오기·초안·비판·세트 이어 쓰기 세션에 요약해 근거로 넘기고(프로젝트마다 크기 상한),
//! 세트 이어 쓰기를 다시 돌리는 조건에 "판정 놓침이 있음"을 더한다(하네스가 그대로여도 같은 경로 다시 출발 때 판정 차이가 보이게).
//! 세트 이어 쓰기 세션(이어 가기 길): 지난 판정·놓친 기록·지난 지시만 읽고 판정 명령과 지시를 고쳐 쓴다(결과 꼴 검사·약하게 바꾸기 막기도 여기).
//! 사람이 읽는 글에는 긴 줄표를 쓰지 않는다
//!
//! 원본: `Sources/AutopilotRules/Misses.swift`. 이 청은 12~134줄(신호·놓침 줄 형, 자르기, 버린 단위 판별, 단위 신호)을 옮긴 것.
//! 그 뒤 청(prep_misses·miss_digest·set_rewrite·revise 등)은 R-Misses-2 이후가 파일 끝에 이어서 더한다.
//! (unit_commits.rs 의 ⚠️임시 조각 1 — `ApMiss`·`ApMissSignal`·자르기 — 는 Misses 청이 옮겨 간 이 형으로 대체된다.)
//!
//! 원본 대응 규칙(이 판의 다른 청들과 같다):
//! - Swift `[String: Any]`·`[[String: Any]]` 자료는 `serde_json::Value` 로(열쇠 이름은 원본과 같다).
//!   `next_start` 원소가 객체가 아닐 때의 `["title": "\(그 값)"]` 대응은 `json_text`(JSONSerialization 이 내주는 NSNumber·NSNull 글 꼴)다.
//! - 글자 수(`count`·`prefix`)는 `formats::grapheme_len`(Swift `Character` 수 근사, UAX #29 주요 규칙만)이다.
//! - 사람이 친 글 «» 감싸기에 앞서 표시 괄호 바꾸기는 `meta::defang_markers` 로.
//! - id 는 `"{run}/{n}/{신호.rawValue}/{k}"` 꼴 — 같은 신호면 같아 두 번 써도 늘지 않는다(원본과 같다).

use crate::rules::formats::grapheme_len;
use crate::rules::meta::defang_markers;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// 놓침 신호. 원본: Misses.swift `APMissSignal`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissSignal {
    #[serde(rename = "discard")]
    Discard,
    #[serde(rename = "revert")]
    Revert,
    #[serde(rename = "next_start")]
    NextStart,
    #[serde(rename = "probe")]
    Probe,
    #[serde(rename = "critic")]
    Critic,
    #[serde(rename = "denial")]
    Denial,
    #[serde(rename = "asked")]
    Asked,
    #[serde(rename = "parked")]
    Parked,
    #[serde(rename = "decision_basis")]
    DecisionBasis,
    #[serde(rename = "takeover")]
    Takeover,
}

impl MissSignal {
    /// Swift `rawValue`
    pub fn raw_value(self) -> &'static str {
        match self {
            MissSignal::Discard => "discard",
            MissSignal::Revert => "revert",
            MissSignal::NextStart => "next_start",
            MissSignal::Probe => "probe",
            MissSignal::Critic => "critic",
            MissSignal::Denial => "denial",
            MissSignal::Asked => "asked",
            MissSignal::Parked => "parked",
            MissSignal::DecisionBasis => "decision_basis",
            MissSignal::Takeover => "takeover",
        }
    }
}

/// 신호의 쪽 — 판정(judge)·지시(instruction). 원본: Misses.swift `apMissSide`
pub fn miss_side(s: MissSignal) -> &'static str {
    match s {
        MissSignal::Discard | MissSignal::Revert | MissSignal::NextStart | MissSignal::Probe | MissSignal::Critic => "judge",
        MissSignal::Denial | MissSignal::Asked | MissSignal::Parked | MissSignal::DecisionBasis | MissSignal::Takeover => "instruction",
    }
}

/// 신호의 한 단어 이름표. 원본: Misses.swift `apMissLabel`
pub fn miss_label(s: MissSignal) -> &'static str {
    match s {
        MissSignal::Discard => "통과했지만 버림",
        MissSignal::Revert => "승격 뒤 되돌림",
        MissSignal::NextStart => "통과했는데 미룬 몫",
        MissSignal::Probe => "반대 탐침",
        MissSignal::Critic => "비판 세션 반례",
        MissSignal::Denial => "권한 거부",
        MissSignal::Asked => "사람에게 물으려 함",
        MissSignal::Parked => "보류 사유",
        MissSignal::DecisionBasis => "지시 밖 근거",
        MissSignal::Takeover => "사람이 이어받음",
    }
}

/// 놓침 줄 하나. 원본: Misses.swift `APMiss`
#[derive(Debug, Clone, PartialEq)]
pub struct Miss {
    pub id: String,
    pub project: String,
    pub run: String,
    pub unit_n: Option<i64>,
    pub unit_id: Option<String>,
    pub source_key: Option<String>,
    pub signal: MissSignal,
    /// 판정 걸음(시나리오 경로 · 판정 명령 글)
    pub judge: Option<String>,
    /// 그 판정 글의 지문(엔진이 냄)
    pub judge_digest: Option<String>,
    pub why: String,
    pub set_id: Option<String>,
    /// open 열림 · used 세션에 넘겨 고친 판이 쓰임 · fixed 같은 준비에서 고침
    pub state: String,
    pub created: f64,
    pub title: String,
}

impl Miss {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str, project: &str, run: &str, unit_n: Option<i64>, unit_id: Option<&str>, source_key: Option<&str>, signal: MissSignal, judge: Option<&str>,
        judge_digest: Option<&str>, why: &str, set_id: Option<&str>, created: f64, title: &str,
    ) -> Self {
        Miss {
            id: id.to_string(),
            project: project.to_string(),
            run: run.to_string(),
            unit_n,
            unit_id: unit_id.map(str::to_string),
            source_key: source_key.map(str::to_string),
            signal,
            judge: judge.map(str::to_string),
            judge_digest: judge_digest.map(str::to_string),
            why: why.to_string(),
            set_id: set_id.map(str::to_string),
            state: "open".to_string(),
            created,
            title: title.to_string(),
        }
    }
    /// 신호의 쪽 — Swift `side`
    pub fn side(&self) -> &'static str {
        miss_side(self.signal)
    }
}

/// 까닭 글자 상한. 원본: Misses.swift `apMissWhyMax`
pub const MISS_WHY_MAX: usize = 300;
/// 놓침 줄 제목 상한. 원본: Misses.swift `apUnitMisses` 의 `clip(title, 120)`
const MISS_TITLE_MAX: usize = 120;

/// 같은 자르기를 다른 신호 규칙(되돌림, unit_commits.rs)도 쓴다. 원본: Misses.swift `clip`·`apClipMissWhy`
pub fn clip_miss_why(s: &str) -> String {
    clip(s, MISS_WHY_MAX)
}

/// 크기 지정 자르기. 원본: Misses.swift `apClipMissWhy(_:_:)`
pub fn clip_miss_why_at(s: &str, n: usize) -> String {
    clip(s, n)
}

/// 줄바꿈은 빈칸으로, 겉 잉여 빈글자는 벗기고, 글자(묶음) 수 상한. 원본: Misses.swift `clip`
fn clip(s: &str, n: usize) -> String {
    let t = s.replace('\n', " ");
    grapheme_clamp(t.trim_matches(swift_ws), n)
}

/// CharacterSet.whitespaces — Zs 범주와 탭(formats.rs 의 것과 같다, 거긴 비공개라 여기도 둔다 — 세 청째, R-Misses-5 와 묶는다)
fn swift_ws(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// 글 `t` 가 `n` 글자(묶음)를 넘으면 앞 `max(n-1, 0)` 글자와 `…` 로 줄인다 — unit_commits.rs 의 것과 같다(거긴 비공개라 여기도 둔다)
fn grapheme_clamp(t: &str, n: usize) -> String {
    if grapheme_len(t) <= n {
        return t.to_string();
    }
    let keep = n.saturating_sub(1);
    // 글자 경계 가운데 앞부분의 글자 수가 keep 이하인 가장 뒤쪽(글자 수는 경계가 뒤로 갈수록 늘지 않는다)
    let bounds: Vec<usize> = t.char_indices().map(|(i, _)| i).chain(std::iter::once(t.len())).collect();
    let (mut lo, mut hi) = (0, bounds.len() - 1);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if grapheme_len(&t[..bounds[mid]]) <= keep {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    format!("{}…", &t[..bounds[lo]])
}

/// 버린 합칠 쪽(run.json branches.discarded 의 source)에 그 단위가 들었나:
/// 스토리 없는 단위는 자율주행 가지(autopilot), 스토리 단위는 story:<키>. 원본: Misses.swift `apDiscardedUnit`
pub fn discarded_unit(story: Option<&str>, discarded_sources: &BTreeSet<String>) -> bool {
    match story {
        Some(s) => discarded_sources.contains(&format!("story:{s}")),
        None => discarded_sources.contains("autopilot"),
    }
}

/// 저장소 규칙을 가리키는 글 — 결정 근거가 이것을 담고 지시 쪽 글을 담지 않으면 지시 밖 근거 신호. 원본: Misses.swift `harnessWords`
const HARNESS_WORDS: [&str; 7] = ["CLAUDE.md", "CLAUDE.local.md", ".claude/", "AGENTS.md", ".mcp.json", "저장소 규칙", "하네스"];
/// 지시·출발 카드를 가리키는 글. 원본: Misses.swift `instructionWords`
const INSTRUCTION_WORDS: [&str; 5] = ["autopilot.md", "자율주행 지시", "출발 카드", "카드 답", "answers.json"];

/// Swift 문자열 보간 `"\(그 값)"` — JSONSerialization 이 내주는 NSNumber·NSNull 의 글 꼴 대응. 원본: `["title": "\($0)"]` 의 `$0`
fn json_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => (if *b { "1" } else { "0" }).to_string(), // NSNumber.description
        Value::Number(n) => n.to_string(),
        Value::Null => "<null>".to_string(),
        other => other.to_string(), // 배열·사전 — 실제 자료는 이 자리를 안 쓰지만 통과시킨다
    }
}

/// `[String]` 내려가기 — 값이 없거나 하나라도 글이 아니면 통째로 빈 목록. 원본: `t["turns"] as? [String] ?? []`
fn json_string_array(v: Option<&Value>) -> Vec<&str> {
    match v {
        Some(Value::Array(a)) => {
            if a.iter().all(Value::is_string) { a.iter().filter_map(Value::as_str).collect() } else { Vec::new() }
        }
        _ => Vec::new(),
    }
}

/// NSNumber intValue — 숫자는 소수점 버림, 참거짓은 1·0, 글은 내려놓는다. 원본: `($0 as? NSNumber)?.intValue`
fn num_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

/// 글 `t` 의 앞 `n` 글자(묶음)만 — Swift `String(t.prefix(n))`(grapheme_clamp 와 같은 뼈대)
fn take_graphemes(t: &str, n: usize) -> String {
    let bounds: Vec<usize> = t.char_indices().map(|(i, _)| i).chain(std::iter::once(t.len())).collect();
    // 앞부분의 글자 수가 n 이하인 가장 뒤쪽 경계 = 앞 n 글자(글자 수는 경계가 뒤로 갈수록 늘지 않는다)
    let (mut lo, mut hi) = (0, bounds.len() - 1);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if grapheme_len(&t[..bounds[mid]]) <= n {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    t[..bounds[lo]].to_string()
}

/// `unit_misses` 의 받는 대꾸 — 원본 `apUnitMisses` 매개바뀜과 이름·순서까지 같다. 원본: Misses.swift `apUnitMisses`
pub struct UnitMissIn<'a> {
    pub project: &'a str,
    pub run: &'a str,
    pub n: i64,
    /// 단위 번호(engine/unit-<n>)
    pub unit_id: Option<&'a str>,
    /// 합칠 쪽 쌍 표의 쌍 열쇠
    pub source_key: Option<&'a str>,
    pub title: &'a str,
    /// 단위 상태 — "done" 이면 판정 통과
    pub status: &'a str,
    pub story: Option<&'a str>,
    pub discarded_sources: &'a BTreeSet<String>,
    /// 단위 기록의 result 사전(next_start·decisions_applied 를 본다)
    pub result: Option<&'a Value>,
    /// 그 단위의 권한 거부 줄(tool 열쇠를 본다)
    pub denials: &'a [Value],
    pub judge: Option<&'a str>,
    pub judge_digest: Option<&'a str>,
    pub set_id: Option<&'a str>,
    /// 적은 시각(초)
    pub at: f64,
    /// 그 단위의 이어받기 기록 줄(back 열쇠가 있으면 읽는다)
    pub takeovers: &'a [Value],
}

/// 단위 하나의 신호(엔진이 단위 판정 뒤·끝·합치기 버리기 뒤에 기록 저장소에 쓴다). id 는 같은 신호면 같다 — 두 번 써도 늘지 않음.
/// 원본: Misses.swift `apUnitMisses`
pub fn unit_misses(i: &UnitMissIn) -> Vec<Miss> {
    let mut out: Vec<Miss> = Vec::new();
    let mut add = |s: MissSignal, k: i64, why: &str, j: Option<&str>| {
        out.push(Miss::new(
            &format!("{}/{}/{}/{}", i.run, i.n, s.raw_value(), k),
            i.project,
            i.run,
            Some(i.n),
            i.unit_id,
            i.source_key,
            s,
            j,
            if j.is_none() { None } else { i.judge_digest },
            &clip_miss_why(why),
            i.set_id,
            i.at,
            &clip_miss_why_at(i.title, MISS_TITLE_MAX),
        ));
    };
    let passed = i.status == "done";
    // 버림
    if passed && discarded_unit(i.story, i.discarded_sources) {
        add(MissSignal::Discard, 1, "판정을 통과한 단위를 합치기·승격 화면에서 버렸습니다(까닭은 묻지 않음)", i.judge);
    }
    // 미룬 몫 — 원소가 사전이 아니면 ["title": "\(그 값)"] 로
    let next: Vec<(String, String)> = match i.result.and_then(|r| r.get("next_start")) {
        Some(Value::Array(a)) => a.iter().map(|x| match x.as_object() {
            Some(o) => (o.get("title").and_then(Value::as_str).unwrap_or("").to_string(), o.get("why").and_then(Value::as_str).unwrap_or("").to_string()),
            None => (json_text(x), String::new()),
        }).collect(),
        _ => Vec::new(),
    };
    for (cnt, (t, w)) in next.iter().enumerate() {
        let why = if w.is_empty() { t.clone() } else { format!("{t}: {w}") };
        let k = i64::try_from(cnt + 1).expect("next_start 순번은 i64 에 들어간다");
        if passed {
            add(MissSignal::NextStart, k, &format!("판정이 통과했는데 미룬 몫이 남았습니다(판정이 보지 않은 일): {why}"), i.judge);
        } else {
            add(MissSignal::Parked, k, &format!("보류: {why}"), None);
        }
    }
    // 권한 거부 — 묻기 도구와 다른 도구로 가른다
    let asked: Vec<_> = i.denials.iter().filter_map(Value::as_object).filter(|o| o.get("tool").and_then(Value::as_str) == Some("AskUserQuestion")).collect();
    let other: Vec<_> = i.denials.iter().filter_map(Value::as_object).filter(|o| o.get("tool").and_then(Value::as_str) != Some("AskUserQuestion")).collect();
    if !asked.is_empty() {
        add(MissSignal::Asked, 1, &format!("사람에게 물으려 했다가 거부됐습니다({}번). 지시가 그 자리를 다루지 않았을 수 있습니다", asked.len()), None);
    }
    if !other.is_empty() {
        // 도구 이름 중복을 빼고 글자 순으로 앞 5 명만(Swift Set + sorted + prefix(5))
        let tools: BTreeSet<&str> = other.iter().filter_map(|o| o.get("tool").and_then(Value::as_str)).collect();
        let names: Vec<&str> = tools.iter().take(5).copied().collect();
        add(MissSignal::Denial, 1, &format!("권한 거부 {}번: {}", other.len(), names.join(", ")), None);
    }
    // 지시 밖 근거 — harnessWords 만 담고 instructionWords 는 담지 않은 첫 결정 하나
    let decs: Vec<_> = match i.result.and_then(|r| r.get("decisions_applied")) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_object).collect(),
        _ => Vec::new(),
    };
    let d = decs.into_iter().find(|o| {
        let b = o.get("basis").and_then(Value::as_str).unwrap_or("");
        HARNESS_WORDS.iter().any(|w| b.contains(w)) && !INSTRUCTION_WORDS.iter().any(|w| b.contains(w))
    });
    if let Some(d) = d {
        let what = d.get("what").and_then(Value::as_str).unwrap_or("");
        let basis = d.get("basis").and_then(Value::as_str).unwrap_or("");
        add(MissSignal::DecisionBasis, 1, &format!("결정 근거가 지시·출발 카드가 아닌 저장소 규칙입니다: {what} · 근거 {basis}"), None);
    }
    // 사람이 이어받아 남긴 것(⑫-8, 단위 기록 takeovers 의 돌려준 줄): 요청·커밋·바뀐 파일·하네스 — 다음 출발의 세트 이어 쓰기가 새 요구로 읽는다.
    // 사람이 친 글은 «» 로 감싸고 표시 괄호는 바꾼다(세트 이어 쓰기 세션 글은 이 기록을 지시가 아닌 자료로 넘긴다)
    for (cnt, row) in i.takeovers.iter().enumerate() {
        let Some(o) = row.as_object() else { continue };
        if o.get("back").is_none() {
            continue;
        }
        let turns: Vec<String> = json_string_array(o.get("turns")).iter().map(|s| defang_markers(&s.replace('\n', " "))).collect();
        let files = json_string_array(o.get("files"));
        let harness = json_string_array(o.get("harness"));
        let commits = o.get("commits").and_then(Value::as_object).map(|m| m.values().filter_map(num_int).sum::<i64>()).unwrap_or(0);
        let mut why = format!("사람이 터미널로 이어받아 요청 {}개·커밋 {}개·바뀐 파일 {}개를 남겼습니다", turns.len(), commits, files.len());
        if !turns.is_empty() {
            let heads: Vec<String> = turns.iter().take(3).map(|s| {
                let p = if grapheme_len(s) > 80 { format!("{}…", take_graphemes(s, 80)) } else { s.clone() };
                format!("«{p}»")
            }).collect();
            why.push_str(": ");
            why.push_str(&heads.join(" "));
        }
        if !harness.is_empty() {
            why.push_str(" · 하네스 바뀜 ");
            why.push_str(&harness.iter().take(3).copied().collect::<Vec<_>>().join(", "));
        }
        let k = i64::try_from(cnt + 1).expect("takeover 순번은 i64 에 들어간다");
        add(MissSignal::Takeover, k, &why, None);
    }
    out
}
