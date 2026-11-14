//! 방향을 바꿀 말(`inbox/<시각>.md`)의 글 꼴·어느 단위에 붙일지·언제 넘길지·모델에게 넘길 글을 정하는 규칙. 원본: `Sources/AutopilotRules/Inbox.swift`.
//! 순수 함수와 포트(`InboxFiles`·`InboxOut`)뿐이다(파일·환경 없음, 시각은 `ports::Clock` 으로만).
//!
//! 이식된 선언(원본 순서, 전부): `InboxTo`(APInboxTo) · `InboxNote`(APInboxNote) · `INBOX_MAX_CHARS`·`INBOX_MAX_BYTES`(apInboxMaxChars·Bytes) ·
//! `compose`(apInboxCompose) · `name_ok`(apInboxNameOK) · `InboxParse`(APInboxParse) · `parse`(apInboxParse) · `InboxEntry`(APInboxEntry) ·
//! `INBOX_STATUSES`(apInboxStatuses) · `InboxBind`(APInboxBind) · `bind`(apInboxBind) · 비공개 `unit_word`(apInboxUnitWord) ·
//! `now_for`(apInboxNowFor) · `next_for`(apInboxNextFor) · 비공개 `quote`(apInboxQuote) · 비공개 `INBOX_SCOPE`(apInboxScope) · `now_text`(apInboxNowText) ·
//! `next_text`(apInboxNextText) · `INBOX_PASSED_TEXT`(apInboxPassedText) · `AfterTurn`·`after_turn`(APAfterTurn·apAfterTurn) ·
//! `AfterCheck`·`after_check`(APAfterCheck·apAfterCheck) · `close_unit`(apInboxCloseUnit) · `close_run`(apInboxCloseRun) ·
//! `InboxRead`·`InboxFiles`·`receive`(APInboxRead·APInboxFiles·apInboxReceive) · `InboxOut`·`InboxSend`·`send`(APInboxOut·APInboxSend·apInboxSend) ·
//! `label`(apInboxLabel) · `received_text`(apInboxReceivedText).
//! 원본 시험 파일 끝의 `RestartTests`(멈추고 다시 출발)가 시험하는 규칙은 `Control.swift` 몫이라 `rules::control` 에 있다. 시험만 `tests/rules_inbox.rs` 에 옮겼다.
//!
//! Foundation 과 다른 점(원본 Swift 는 `Character`=자소 묶음 단위, 여기는 유니코드 글자(`char`) 단위):
//! - 본문 길이 한도(`count` ≤ 2000)와 머리 오류 글의 `prefix(40)`·`prefix(20)` 은 `chars()` 로 센다. 결합 글자·ZWJ 그림 글자는 원본이 하나로 세는 것을 여기서는 여럿으로 센다.
//! - 머리 줄의 `firstIndex(of: ":")` 는 바이트로 찾는다(쌍점 바로 뒤에 결합 글자가 붙은 줄에서만 갈린다).
//! - `trimmingCharacters` 의 `.whitespaces`(Zs+탭)는 `is_ws`, `.whitespacesAndNewlines`(Zs·Zl·Zp·U+0009~000D·U+0085)는 `char::is_whitespace` 와 같은 집합이다.
//! - `apInboxNameOK` 의 정규식 `$` 는 ICU 에서 끝 줄바꿈 앞에서도 맞지만, 여기는 끝 줄바꿈이 붙은 이름을 받지 않는다(파일 이름에 줄바꿈은 들어갈 수 없다).
//! - `Double("…")` 은 16진 실수(`0x1p3`)도 읽지만 `f64::from_str` 은 읽지 않는다(그런 `at` 머리는 여기서 `bad`). 무한·NaN 은 둘 다 `bad`.
//! - `compose` 의 `at` 은 `{:.3}` 으로 쓴다. 유한한 값은 `%.3f` 와 같고, NaN 은 `NaN`(Swift 는 `nan`)으로 나온다.
//! - `apInboxBind` 에 `unit <= 0` 이 직접 들어오면 Swift 는 배열 범위 밖으로 죽는다. 여기는 「그 단위가 이 실행에 없음」으로 `late` 를 낸다(`parse` 는 1 이상만 내므로 닿지 않는다).
//! - `InboxEntry` 의 Codable 은 serde 로 옮겼다: 옵셔널은 비면 키를 쓰지 않고, 정수인 시각은 정수로 쓴다(Swift JSONEncoder 와 같게). 키 차례는 선언 차례다.
//! - `send` 의 글자 수 한도·`received_text` 의 80 자 끊기도 `chars()` 로 센다(`parse` 와 같은 차이). `received_text` 의 `prefix(80)` 은 글자 단위로 끊어 결합 글자 중간에서 끊길 수 있다.
//! - 시각은 Swift `Date`(`timeIntervalSince1970`) 대신 `ports::Clock::now()`(유닉스 초, `f64`)를 쓴다. `receive` 의 `received` 가 그 값이다.
//! - Swift 포트 `APInboxFiles`·`APInboxOut` 은 `InboxFiles`·`InboxOut` 트레이트다(`Data` 는 `&[u8]`, `tooBig(Int)` 는 `TooBig(i64)`). `names()` 가 같은 이름을 두 번 주면 Swift 처럼 두 줄이 나온다.
//! - `send` 가 쓰는 `apRunEnded` 는 `rules::control::run_ended` 다. 문자열 합치기(`joined`)와 `components(separatedBy:)` 는 `join`·`replace` 로 같은 결과다.

use serde::{Deserialize, Serialize, Serializer};

use crate::ports::Clock;

/// 말을 어디에 붙이는가. 원본: `APInboxTo`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxTo {
    /// 지금 도는 단위에
    Now,
    /// 다음 단위부터
    Next,
}

impl InboxTo {
    /// 원본 `rawValue`
    pub fn as_str(self) -> &'static str {
        match self {
            InboxTo::Now => "now",
            InboxTo::Next => "next",
        }
    }

    /// 원본 `APInboxTo(rawValue:)` — 정확히 `now`·`next` 만
    pub fn from_raw(s: &str) -> Option<InboxTo> {
        match s {
            "now" => Some(InboxTo::Now),
            "next" => Some(InboxTo::Next),
            _ => None,
        }
    }
}

/// 파일 하나의 글 — 머리(빈 줄 앞까지 "이름: 값" 줄)와 본문. 원본: `APInboxNote`
#[derive(Debug, Clone, PartialEq)]
pub struct InboxNote {
    pub to: InboxTo,
    /// 보드가 보낼 때 본 지금 단위(그 단위가 돌고 있을 때만 적는다)
    pub unit: Option<i64>,
    /// 보낸 시각(유닉스 초)
    pub at: Option<f64>,
    pub text: String,
}

impl InboxNote {
    /// 원본 `APInboxNote(to:text:)` — `unit`·`at` 은 비운다
    pub fn new(to: InboxTo, text: &str) -> InboxNote {
        InboxNote { to, unit: None, at: None, text: text.to_string() }
    }
}

/// 본문 글자 수 상한(보드 입력 칸과 같음). 원본: `apInboxMaxChars`
pub const INBOX_MAX_CHARS: usize = 2000;
/// 파일 크기 상한(엔진이 이보다 크면 읽지 않는다). 원본: `apInboxMaxBytes`
pub const INBOX_MAX_BYTES: usize = 16_384;

// Foundation `CharacterSet.whitespaces`: 일반 범주 Zs 와 탭(U+0009)
fn is_ws(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn trim_ws(s: &str) -> &str {
    s.trim_matches(is_ws)
}

// Foundation `CharacterSet.whitespacesAndNewlines` — 유니코드 White_Space 와 같은 집합
fn trim_all(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}

fn prefix_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// 보드가 쓰는 글. 본문은 앞뒤 빈칸을 다듬고 줄 끝은 `\n` 으로. 원본: `apInboxCompose`
pub fn compose(n: &InboxNote) -> String {
    let mut h = format!("to: {}\n", n.to.as_str());
    if let Some(u) = n.unit {
        h.push_str(&format!("unit: {u}\n"));
    }
    if let Some(a) = n.at {
        h.push_str(&format!("at: {a:.3}\n"));
    }
    let body = n.text.replace("\r\n", "\n");
    format!("{h}\n{}\n", trim_all(&body))
}

/// 파일 이름 꼴 — 보드는 `<yyyyMMdd-HHmmss-SSS>[-k].md` 로 쓴다. 엔진은 이 꼴만 읽는다
/// (임시 파일 `.tmp-…`·숨은 파일·다른 확장자는 넘김). 원본: `apInboxNameOK`, 정규식 `^[0-9A-Za-z][0-9A-Za-z._-]{0,80}\.md$`
pub fn name_ok(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    let b = stem.as_bytes();
    if b.is_empty() || b.len() > 81 || !b[0].is_ascii_alphanumeric() {
        return false;
    }
    b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// 엔진이 읽은 글. 원본: `APInboxParse`
#[derive(Debug, Clone, PartialEq)]
pub enum InboxParse {
    Note(InboxNote),
    /// 읽지 않는 까닭(쉬운 말)
    Bad(String),
}

/// 엔진이 읽는 글. 머리 `to` 는 꼭 있어야 하고(now·next), `unit` 은 1 이상의 정수, `at` 은 수.
/// 모르는 머리 칸은 넘긴다(뒤에 칸이 늘어도 읽게). 본문이 비었거나 상한보다 길면 읽지 않는다. 원본: `apInboxParse`
pub fn parse(raw: &str) -> InboxParse {
    let s = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = s.split('\n').collect();
    let Some(blank) = lines.iter().position(|l| trim_ws(l).is_empty()) else {
        return InboxParse::Bad("머리와 본문 사이 빈 줄이 없습니다".to_string());
    };
    let mut to: Option<InboxTo> = None;
    let mut unit: Option<i64> = None;
    let mut at: Option<f64> = None;
    for l in &lines[..blank] {
        let Some(c) = l.find(':') else {
            return InboxParse::Bad(format!("머리 줄 꼴이 아닙니다: {}", prefix_chars(l, 40)));
        };
        let k = trim_ws(&l[..c]).to_lowercase();
        let v = trim_ws(&l[c + 1..]);
        match k.as_str() {
            "to" => match InboxTo::from_raw(v) {
                Some(t) => to = Some(t),
                None => return InboxParse::Bad(format!("머리 to 는 now 또는 next 입니다({})", prefix_chars(v, 20))),
            },
            "unit" => match v.parse::<i64>() {
                Ok(u) if u >= 1 => unit = Some(u),
                _ => return InboxParse::Bad(format!("머리 unit 은 1 이상의 정수입니다({})", prefix_chars(v, 20))),
            },
            "at" => match v.parse::<f64>() {
                Ok(a) if a.is_finite() => at = Some(a),
                _ => return InboxParse::Bad(format!("머리 at 은 수입니다({})", prefix_chars(v, 20))),
            },
            _ => continue,
        }
    }
    let Some(to) = to else {
        return InboxParse::Bad("머리에 to: now 또는 to: next 가 없습니다".to_string());
    };
    let text = trim_all(&lines[blank + 1..].join("\n")).to_string();
    if text.is_empty() {
        return InboxParse::Bad("본문이 비었습니다".to_string());
    }
    let n = text.chars().count();
    if n > INBOX_MAX_CHARS {
        return InboxParse::Bad(format!("본문이 {INBOX_MAX_CHARS}자보다 깁니다({n}자)"));
    }
    InboxParse::Note(InboxNote { to, unit, at, text })
}

// 시각(유닉스 초)이 정수면 정수로 쓴다 — Swift JSONEncoder 가 1793300000 으로 쓰는 것과 같게(소수가 있으면 그대로)
fn secs_value(at: f64) -> serde_json::Value {
    if at.is_finite() && at.fract() == 0.0 && at.abs() < 9e15 {
        serde_json::Value::from(at as i64)
    } else {
        serde_json::Value::from(at)
    }
}

fn ser_secs<S: Serializer>(at: &f64, s: S) -> Result<S::Ok, S::Error> {
    secs_value(*at).serialize(s)
}

fn ser_secs_opt<S: Serializer>(at: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match at {
        Some(a) => secs_value(*a).serialize(s),
        None => s.serialize_none(),
    }
}

/// `run.json` 의 inbox 한 줄 — 받은 말 하나와 그 말이 어떻게 됐는지(보드가 그린다). 원본: `APInboxEntry`
/// (Swift 의 긴 `init` 은 기본값 인자가 있다 — 여기는 필수 다섯을 `new` 로 받고 나머지는 필드로 채운다)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxEntry {
    /// `inbox/` 안 파일 이름(같은 파일을 다시 읽지 않는 열쇠)
    pub file: String,
    /// now·next(읽지 못한 파일은 빈 글자)
    pub to: String,
    pub text: String,
    /// 머리 at
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "ser_secs_opt")]
    pub sent: Option<f64>,
    /// 머리 unit(보낼 때 본 단위)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<i64>,
    /// 엔진이 받은 시각
    #[serde(serialize_with = "ser_secs")]
    pub received: f64,
    /// `INBOX_STATUSES` 가운데 하나
    pub status: String,
    /// now: 넘길 단위
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<i64>,
    /// next: 붙이기 시작할 단위
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<i64>,
    /// 넘긴(now)·붙인(next) 단위
    pub units: Vec<i64>,
    /// 처음 넘긴·붙인 시각
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "ser_secs_opt")]
    pub done: Option<f64>,
    /// late·dropped·unused·bad 의 까닭
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// 없으면 사람이 보드에서 보낸 말, takeover 면 돌려줄 때 엔진이 만든 이어받기 지시(⑫-8, `Takeover.swift`). 없는 칸은 옛 꼴 그대로 읽힌다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl InboxEntry {
    /// 원본 `APInboxEntry(file:to:text:received:status:)` — 나머지 칸은 비워 둔다
    pub fn new(file: &str, to: &str, text: &str, received: f64, status: &str) -> InboxEntry {
        InboxEntry {
            file: file.to_string(),
            to: to.to_string(),
            text: text.to_string(),
            sent: None,
            seen: None,
            received,
            status: status.to_string(),
            unit: None,
            from: None,
            units: Vec::new(),
            done: None,
            why: None,
            source: None,
        }
    }
}

/// waiting: 넘길·붙일 차례를 기다림, delivered: now 를 세션에 넘김, applied: next 를 단위 첫 입력에 붙임(뒤 단위에도 이어 붙는다),
/// late: 받았을 때 이미 넘길 곳이 없음, dropped: now 를 넘기기 전에 그 단위가 끝남, unused: next 를 붙일 단위가 시작하지 않고 실행이 끝남,
/// bad: 읽지 못함. 원본: `apInboxStatuses`
pub const INBOX_STATUSES: [&str; 7] = ["waiting", "delivered", "applied", "late", "dropped", "unused", "bad"];

/// 받은 말이 어느 단위에 붙는지의 결과. 원본: `APInboxBind`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxBind {
    /// waiting 또는 late
    pub status: String,
    pub unit: Option<i64>,
    pub from: Option<i64>,
    pub why: Option<String>,
}

impl InboxBind {
    fn waiting_unit(unit: i64) -> InboxBind {
        InboxBind { status: "waiting".to_string(), unit: Some(unit), from: None, why: None }
    }

    fn waiting_from(from: i64) -> InboxBind {
        InboxBind { status: "waiting".to_string(), unit: None, from: Some(from), why: None }
    }

    fn late(why: String) -> InboxBind {
        InboxBind { status: "late".to_string(), unit: None, from: None, why: Some(why) }
    }
}

/// 받은 말을 어느 단위에 붙일지 — `statuses` 는 `queue.json` 차례의 단위 상태(running 은 일시정지·한도 대기 중인 단위도).
/// - now: 보낼 때 본 단위(`seen`)가 있으면 그 단위가 아직 돌거나 시작 전일 때만 그 단위, 끝났으면 넘기지 않는다(다른 단위의 일에 그 말이 섞이지 않게).
///   없으면 지금 도는 단위, 도는 단위가 없으면(멈출 선 대기·단위 사이) 다음에 시작할 단위의 첫 입력
/// - next: 지금 도는 단위 뒤(도는 단위가 없으면 처음부터)의 첫 대기 단위부터 남은 단위 모두
///
/// 원본: `apInboxBind`
pub fn bind<S: AsRef<str>>(n: &InboxNote, statuses: &[S]) -> InboxBind {
    let running = statuses.iter().position(|s| s.as_ref() == "running").map(|i| i as i64 + 1);
    let first_pending = |after: i64| -> Option<i64> {
        statuses.iter().enumerate().find(|(i, s)| *i as i64 + 1 > after && s.as_ref() == "pending").map(|(i, _)| i as i64 + 1)
    };
    match n.to {
        InboxTo::Now => {
            if let Some(s) = n.unit {
                if s < 1 || s > statuses.len() as i64 {
                    return InboxBind::late(format!("보낼 때 본 단위(단위 {s})가 이 실행에 없어 넘기지 않았습니다"));
                }
                let st = statuses[(s - 1) as usize].as_ref();
                if st == "running" || st == "pending" {
                    return InboxBind::waiting_unit(s);
                }
                return InboxBind::late(format!(
                    "보낼 때 본 단위(단위 {s})가 이미 끝나({}) 넘기지 않았습니다. 다음 단위부터로 다시 보내거나 멈추고 다시 출발합니다",
                    unit_word(st)
                ));
            }
            if let Some(r) = running {
                return InboxBind::waiting_unit(r);
            }
            if let Some(p) = first_pending(0) {
                return InboxBind::waiting_unit(p);
            }
            InboxBind::late("남은 단위가 없어 넘기지 않았습니다".to_string())
        }
        InboxTo::Next => {
            if let Some(p) = first_pending(running.unwrap_or(0)) {
                return InboxBind::waiting_from(p);
            }
            InboxBind::late("뒤에 남은 단위가 없어 붙일 곳이 없습니다".to_string())
        }
    }
}

// 끝난 단위 상태의 쉬운 말. 원본: `apInboxUnitWord`(비공개)
fn unit_word(s: &str) -> &str {
    match s {
        "done" => "통과",
        "failed" => "미통과",
        "capped" => "상한에서 멈춤",
        "stopped" => "중단",
        "skipped" => "건너뜀",
        other => other,
    }
}

/// 단위 n 의 세션에 넘길 말(now, 기다림, 그 단위) — 받은 차례대로 자리 번호. 원본: `apInboxNowFor`
pub fn now_for(entries: &[InboxEntry], unit: i64) -> Vec<usize> {
    (0..entries.len())
        .filter(|&i| entries[i].to == "now" && entries[i].status == "waiting" && entries[i].unit == Some(unit))
        .collect()
}

/// 단위 n 의 첫 입력에 붙일 방향(next, 기다림·붙임, 붙이기 시작할 단위가 n 이하) — 받은 차례대로. 원본: `apInboxNextFor`
pub fn next_for(entries: &[InboxEntry], unit: i64) -> Vec<usize> {
    (0..entries.len())
        .filter(|&i| {
            let e = &entries[i];
            e.to == "next" && (e.status == "waiting" || e.status == "applied") && e.from.unwrap_or(i64::MAX) <= unit && !e.units.contains(&unit)
        })
        .collect()
}


// 모델에게 넘기는 글 — 사람의 글은 줄마다 들여 쓴다. 원본: `apInboxQuote`(비공개)
fn quote<S: AsRef<str>>(texts: &[S]) -> String {
    texts.iter().map(|t| format!("- {}", t.as_ref().replace('\n', "\n  "))).collect::<Vec<_>>().join("\n")
}

// 원본: `apInboxScope`(비공개)
const INBOX_SCOPE: &str = "이 단위의 일·판정 명령·쓰기 범위·가지 규칙은 출발 때 고정한 그대로입니다. 그 밖의 일(다른 단위의 일, 다른 가지 맞추기, 판정 바꾸기 등)을 하라는 말이면 하지 않고 result.json 의 next_start 에 적습니다.";

/// 지금 도는 단위의 세션에 넘기는 글. 원본: `apInboxNowText`
pub fn now_text<S: AsRef<str>>(texts: &[S]) -> String {
    format!(
        "사람이 실행을 지켜보다 이 단위에 한마디를 보냈습니다(보드의 \"지금 단위에\"):\n{}\n이 말을 반영해 하던 일을 이어 가세요. {INBOX_SCOPE} 마치면 result.json 을 고쳐 쓰고 턴을 끝냅니다.",
        quote(texts)
    )
}

/// 단위의 첫 입력에 붙이는 방향 글. 원본: `apInboxNextText`
pub fn next_text<S: AsRef<str>>(texts: &[S]) -> String {
    format!(
        "사람이 실행 중에 보낸 방향(보드의 \"다음 단위부터\", 이 단위와 뒤 단위에 모두 붙습니다):\n{}\n이 방향은 일하는 방식만 바꿉니다. {INBOX_SCOPE}",
        quote(texts)
    )
}

/// 판정은 통과했는데 그 사이 지금 단위에 말이 온 경우 — 다시 맡기는 글의 머리(말은 `now_text` 로 뒤에 붙는다). 원본: `apInboxPassedText`
pub const INBOX_PASSED_TEXT: &str = "판정 명령은 통과했지만 그 사이 사람이 이 단위에 한마디를 보냈습니다. 아래 말을 반영한 뒤 판정 명령을 다시 돌려 보고 result.json 을 고쳐 쓰세요.";

/// 턴이 끝난 뒤의 다음 걸음. 원본: `APAfterTurn`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterTurn {
    Check,
    Again,
}

/// 턴이 끝난 뒤: 그 사이 이 단위에 말이 왔으면 판정 전에 같은 세션으로 한 번 더. 원본: `apAfterTurn`
pub fn after_turn(waiting_now: bool) -> AfterTurn {
    if waiting_now {
        AfterTurn::Again
    } else {
        AfterTurn::Check
    }
}

/// 판정 뒤의 다음 걸음. 원본: `APAfterCheck`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterCheck {
    Pass,
    Note,
    Retry,
    Fail,
}

/// 판정 뒤: 통과·말 없음 → 통과, 통과·말 있음 → 말을 넘기고 다시(시도 수는 세지 않음), 실패 → 시도가 남았거나 말이 있으면 다시, 아니면 미통과.
/// 시도가 다 찼어도 그 사이 말이 왔으면 한 번 더 맡긴다(사람이 끼어든 것이라). 원본: `apAfterCheck`
pub fn after_check(passed: bool, tries: i64, retries: i64, waiting_now: bool) -> AfterCheck {
    if passed {
        return if waiting_now { AfterCheck::Note } else { AfterCheck::Pass };
    }
    if tries < retries || waiting_now {
        AfterCheck::Retry
    } else {
        AfterCheck::Fail
    }
}

/// 단위가 끝날 때(통과·미통과·상한·중단·건너뜀) 그 단위에 아직 넘기지 못한 now 는 dropped. 바뀐 줄 수. 원본: `apInboxCloseUnit`
pub fn close_unit(entries: &mut [InboxEntry], unit: i64, ended: &str) -> usize {
    let mut k = 0;
    for i in now_for(entries, unit) {
        entries[i].status = "dropped".to_string();
        entries[i].why = Some(format!("넘기기 전에 그 단위(단위 {unit})가 끝나({}) 넘기지 못했습니다", unit_word(ended)));
        k += 1;
    }
    k
}

/// 실행이 끝날 때 남은 것: 기다리던 now 는 dropped, 한 번도 붙이지 못한 next 는 unused. 바뀐 줄 수. 원본: `apInboxCloseRun`
pub fn close_run(entries: &mut [InboxEntry]) -> usize {
    let mut k = 0;
    for e in entries.iter_mut().filter(|e| e.status == "waiting") {
        if e.to == "now" {
            e.status = "dropped".to_string();
            e.why = Some("넘기기 전에 실행이 끝났습니다".to_string());
        } else {
            e.status = "unused".to_string();
            e.why = Some("붙일 단위가 시작하지 않고 실행이 끝났습니다".to_string());
        }
        k += 1;
    }
    k
}

// ── 포트와 유스케이스 ────────────────────────────────

/// 엔진이 읽는 `inbox/` 의 파일 하나 — 폴더가 없거나 링크면 빈 목록, 파일은 링크가 아닌 보통 파일만 크기 상한 안에서만 글로. 원본: `APInboxRead`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxRead {
    Text(String),
    NotFile,
    TooBig(i64),
    Unreadable,
}

/// 엔진이 읽는 `inbox/`. 원본: `APInboxFiles`
pub trait InboxFiles: Send + Sync {
    fn names(&self) -> Vec<String>;
    fn read(&self, name: &str) -> InboxRead;
}

/// 유스케이스(엔진): 새 파일만 읽어 받은 줄을 만든다 — 이름 꼴이 맞고 `known` 에 없는 것, 이름 차례로.
/// 읽지 못한 파일도 bad 줄로 남겨 다시 읽지 않는다. 원본: `apInboxReceive`
pub fn receive<S: AsRef<str>>(files: &dyn InboxFiles, known: &[InboxEntry], statuses: &[S], clock: &dyn Clock) -> Vec<InboxEntry> {
    let seen: std::collections::HashSet<&str> = known.iter().map(|e| e.file.as_str()).collect();
    let now = clock.now();
    let mut names: Vec<String> = files.names().into_iter().filter(|n| name_ok(n) && !seen.contains(n.as_str())).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let bad = |why: String| {
            let mut e = InboxEntry::new(&name, "", "", now, "bad");
            e.why = Some(why);
            e
        };
        match files.read(&name) {
            InboxRead::NotFile => out.push(bad("보통 파일이 아니라(링크·폴더) 읽지 않았습니다".to_string())),
            InboxRead::TooBig(b) => out.push(bad(format!("파일이 {INBOX_MAX_BYTES}바이트보다 커서({b}바이트) 읽지 않았습니다"))),
            InboxRead::Unreadable => out.push(bad("글(UTF-8)로 읽지 못했습니다".to_string())),
            InboxRead::Text(t) => match parse(&t) {
                InboxParse::Bad(why) => out.push(bad(why)),
                InboxParse::Note(n) => {
                    let b = bind(&n, statuses);
                    let mut e = InboxEntry::new(&name, n.to.as_str(), &n.text, now, &b.status);
                    e.sent = n.at;
                    e.seen = n.unit;
                    e.unit = b.unit;
                    e.from = b.from;
                    e.why = b.why;
                    out.push(e);
                }
            },
        }
    }
    out
}

/// 보드가 쓰는 `inbox/` — 있는지와 쓰기(임시 파일에 쓰고 이름을 바꿔 엔진이 반쯤 쓴 파일을 읽지 않게). 원본: `APInboxOut`
pub trait InboxOut: Send + Sync {
    fn exists(&self, name: &str) -> bool;
    fn write(&self, name: &str, data: &[u8]) -> bool;
}

/// 보내기의 결과. 원본: `APInboxSend`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxSend {
    Sent(String),
    Refused(String),
}

/// 유스케이스(보드): 한마디 보내기 — 끝나지 않은(출발한) 실행에만, 빈 글·상한보다 긴 글은 보내지 않는다.
/// 이름은 `stamp`(.md), 있으면 -2, -3 … 원본: `apInboxSend`
pub fn send(n: &InboxNote, run_state: &str, stamp: &str, out: &dyn InboxOut) -> InboxSend {
    if run_state.is_empty() || crate::rules::control::run_ended(run_state) {
        return InboxSend::Refused("끝난 실행에는 보내지 않습니다".to_string());
    }
    let text = trim_all(&n.text);
    if text.is_empty() {
        return InboxSend::Refused("보낼 말을 써 주세요".to_string());
    }
    let count = text.chars().count();
    if count > INBOX_MAX_CHARS {
        return InboxSend::Refused(format!("{INBOX_MAX_CHARS}자까지 보낼 수 있습니다(지금 {count}자)"));
    }
    let mut name = format!("{stamp}.md");
    let mut k = 1;
    while out.exists(&name) {
        k += 1;
        name = format!("{stamp}-{k}.md");
    }
    if !name_ok(&name) {
        return InboxSend::Refused(format!("파일 이름 꼴이 아닙니다({name})"));
    }
    let mut m = n.clone();
    m.text = text.to_string();
    if out.write(&name, compose(&m).as_bytes()) {
        InboxSend::Sent(name)
    } else {
        InboxSend::Refused("inbox 에 쓰지 못했습니다".to_string())
    }
}

/// 보드에 보일 말의 한 줄 — (상태 이름, 자세한 말). 원본: `apInboxLabel`
pub fn label(e: &InboxEntry) -> (String, String) {
    let to = if e.to == "next" { "다음 단위부터" } else { "지금 단위에" };
    let q = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| "?".to_string());
    let joined = e.units.iter().map(|u| u.to_string()).collect::<Vec<_>>().join("·");
    let (s, d) = match e.status.as_str() {
        "waiting" => {
            if e.to == "next" {
                ("기다림".to_string(), format!("단위 {} 부터 첫 입력에 붙입니다", q(e.from)))
            } else {
                ("기다림".to_string(), format!("단위 {} 의 다음 차례에 넘깁니다", q(e.unit)))
            }
        }
        "delivered" => ("넘김".to_string(), format!("단위 {joined} 의 세션에 넘겼습니다")),
        "applied" => {
            let tail = if e.to == "next" { "(뒤 단위에도 붙습니다)" } else { "" };
            ("붙임".to_string(), format!("단위 {joined} 의 첫 입력에 붙였습니다{tail}"))
        }
        "late" => ("넘기지 않음".to_string(), e.why.clone().unwrap_or_else(|| format!("{to} 넘길 곳이 없었습니다"))),
        "dropped" => ("넘기지 못함".to_string(), e.why.clone().unwrap_or_default()),
        "unused" => ("쓰이지 않음".to_string(), e.why.clone().unwrap_or_default()),
        "bad" => ("읽지 못함".to_string(), e.why.clone().unwrap_or_default()),
        other => (other.to_string(), e.why.clone().unwrap_or_default()),
    };
    (s, d)
}

/// 엔진이 받았을 때 진행 기록(who board) 한 줄. `unit_running` 은 now 를 붙일 단위가 지금 도는 단위인가
/// (아니면 그 단위가 시작할 때 첫 입력에 붙는다). 원본: `apInboxReceivedText`
pub fn received_text(e: &InboxEntry, unit_running: bool) -> String {
    let t = e.text.replace('\n', " ");
    let q = if t.chars().count() > 80 { format!("«{}…»", prefix_chars(&t, 80)) } else { format!("«{t}»") };
    let why = e.why.clone().unwrap_or_default();
    match e.status.as_str() {
        "waiting" if e.to == "now" => format!(
            "사람이 보낸 말을 받았습니다(지금 단위에 · 단위 {}): {q}. {}",
            e.unit.unwrap_or(0),
            if unit_running { "이 세션의 다음 차례에 넘깁니다." } else { "그 단위가 시작할 때 첫 입력에 붙입니다." }
        ),
        "waiting" => format!(
            "사람이 보낸 방향을 받았습니다(다음 단위부터 · 단위 {} 부터): {q}. 대기열(범위)은 그대로이고 뒤 단위의 첫 입력에 붙입니다.",
            e.from.unwrap_or(0)
        ),
        "bad" => format!("inbox 의 {} 을 읽지 못해 넘겼습니다: {why}", e.file),
        _ => format!("사람이 보낸 말을 받았지만 넘기지 않습니다({}): {q}. {why}", if e.to == "next" { "다음 단위부터" } else { "지금 단위에" }),
    }
}
