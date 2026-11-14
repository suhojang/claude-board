//! 실행 보고서 틀. 원본: `Sources/AutopilotRules/Report.swift`(568줄).
//! 이 구간은 결과 기록(`result.json`, 모델이 쓴 글)을 값으로 읽는 일과
//! 보고서가 쓰는 값 꼴이다. 모델이 쓴 글은 믿지 않는다: 칸의 꼴이 틀리면 그 칸만 비우고, 글·목록 길이는 상한에서 자른다(`rules::formats`·`rules::prose` 와 같은 태도).
//! 긴 줄표 바꾸기(`rules::prose::plain`)와 HTML 이스케이프(`rules::prose::escape_html`)는 그릴 때(뒤 구간) 한다. 여기서는 글자를 그대로 둔다.
//!
//! 이식됨(원본 순서): `ResultTest`(APResultTest) · `ResultChange`(APResultChange) · `ResultDecision`(APResultDecision) · `ResultFlow`(APResultFlow) ·
//! `ResultCarry`(APResultCarry) · `ResultTicket`(APResultTicket) · `ResultRef`(APResultRef) · `ResultRecord`(APResultRecord) ·
//! `REPORT_TEXT_MAX`·`REPORT_LIST_MAX`·`REPORT_FLOW_MAX`·`REPORT_TAIL_MAX`(apReportTextMax·ListMax·FlowMax·TailMax) · `cut`(apCut) ·
//! `report_text_n`·`report_text`(apReportText) · `parse_result`(apParseResult) · `ResultState`(APResultState) · `ReportCheck`(APReportCheck) ·
//! `ReportUnit`(APReportUnit) · `ReportRun`(APReportRun) · `ReportOrigin`(APReportOrigin, 원본은 `Ports.swift` 에 있다).
//! 이식됨(2/4, 원본 143~251줄): `ReportSource`(APReportSource, 원본은 `Ports.swift`) · `num`·`int`·`str`(`str_of`)·`lastPart`(`last_part`) · `report_run`(apReportRun, 기록 → 보고서 값) · 이름표 사전 일곱(`unit_label`·`unit_class`·`run_label`·`kind_label`·`outcome_label`·`test_label`·`merge_label`) · `report_dur`(apReportDur) · `kind_text`(비공개 `kind`).
//! 이식됨(3/4, 원본 253~334줄): `Writer`(`Writer`·`t`·`c`·`short`·`date`·`add`, 시간대는 `FixedZone`·`CivilTime`) · `chip`(`chip`·`chip_plain`) · `css`(`CSS`).
//! 이식됨(d, 원본 337~539줄): `report_html`(apReportHTML, `only` 의 기본값 `nil` 은 `Option<i64>` 로) · `unit_section`(unitSection) · `section_list`(비공개 `list`,
//! 이 파일의 목록 읽기 도우미 `list` 와 이름이 겹쳐 다르게 둠) · `flow`(비공개, Swift 의 `unit:` 인자는 원본 본문에서 안 쓰므로 `_unit` 으로 받기만 한다) ·
//! `timeline`(비공개). Swift 시험 `ReportHTMLTests`(견본·단위·악성골든·악성·견본안전·결정적·바탕)도 `tests/rules_report.rs` 의 `html_` 로 시작하는 이름으로 옮겼다.
//! 이식됨(e, 원본 540~565줄): `ReportDraw`(APReportDraw) · `draw_reports`(apDrawReports, 그리는 유스케이스). 쓸 곳 포트(`ReportSink`·`ReportFile`, 원본
//! `APReportSink`·`APReportFile` 은 `Ports.swift` 에 있다)를 함께 정한다. 빠진 단위 보고서(`apReportEndedStatuses`·`apMissingUnitReports`, 원본 565~568줄)는
//! 이미 `unit_commits.rs`(임시 조각 3)에 있어 옮기지 않고 `pub use` 로 다시 내놓는다. Swift 시험 `ReportDrawTests`(쓰기·실패·없음)도 `tests/rules_report.rs` 의 `draw_` 로 시작하는 이름으로 옮겼다.
//! 원본 시험 중 앞 구간의 것: `ReportTests.swift` 의 `ResultParseTests` 셋(`아님`·`꼴`·`상한`)과 `ReportRunTests` 셋(`견본`·`칸`·`없음`) → `tests/rules_report.rs`.
//! 3/4 구간(`Writer`·`chip`·`css` 는 Swift 에서 비공개)을 겨냥한 Swift 시험은 없고 골든 HTML(`Report/*.html`)과 `ReportTests.swift` 의 `결정적`(시간대만 바꾸면 시각만 다름)이 덮는다.
//! 이 구간은 골든의 `<style>` 안(`CSS`)·때 글자(`Writer::date`)를 직접 맞춰 보고, `결정적` 의 시각 부분을 옮겼다. HTML 전체 골든(`견본`·`단위`·`악성골든`)은 d 구간에서 `html_견본` 등으로 옮겼다.
//!
//! Swift ↔ Rust 로 달라진 점:
//! - 기본 인자(`apReportText(v, n = 4000)`)가 없다: `report_text`(상한 4000)와 `report_text_n`(상한을 준다)으로 나눈다.
//! - 사전 `[String: Any]` 는 `serde_json::Value`·`Map`(키 차례 보존)이다. Swift `NSNumber`(불리언 아님)는 `Value::Number`, 불리언은 `Value::Bool` 이라 글이 되지 않는다.
//! - 수를 글로 바꾸는 `NSNumber.stringValue` 는 정수는 그대로, 실수는 `7.0` → `7`, `1.5` → `1.5` 이다. 아주 크거나(지수 17 이상) 아주 작은(-5 아래) 실수는
//!   `e+XX`·`e-XX` 꼴로 쓰는데 Foundation 이 그 경계에서 쓰는 자릿수와 같은지 이 머신(Windows)에서는 확인하지 못했다. 결과 기록 칸에 그런 수가 오는 일은 모델이 틀리게 쓴 때뿐이다.
//! - `String.count`(Character 수)는 `formats::grapheme_len` 과 같은 근사다(CR LF 는 한 글자). `cut` 의 앞부분 자르기도 같은 덩어리 경계에서 한다.
//! - `JSONSerialization` 대신 `serde_json`: 깊이 한도(128)가 달라 아주 깊은 중첩은 nil 이 된다. 같은 키가 둘이면 둘 다 마지막 것을 쓴다.
//! - `refs` 키 정렬은 Swift `String` 비교(유니코드 정규 동등)이고 여기는 UTF-8 바이트 순이다. 키가 정규화되지 않은 글자를 섞어 쓰면 차례가 다를 수 있다.
//! - `last_part` 는 `/` 와 `\` 를 모두 구분자로 본다(Swift `lastPathComponent` 는 `/` 만). 엔진이 Windows 경로(`C:\work\todo-api`)를 기록해도 저장소 이름이 `todo-api` 가 되게 하려는 것이고, `/` 만 쓰는 입력은 같다.
//! - `as? Bool`(`same`·`timed_out`·`interrupted`)은 JSON 불리언만 받는다. Foundation 은 수 0·1 도 받을 수 있는데 이 머신에서 확인하지 못했다(엔진이 쓴 기록은 불리언이다).
//! - `as? [[String: Any]]` 는 원소 하나라도 객체가 아니면 통째로 빈 배열이 된다(`moved`·`changed`·`checks`·`denials`). 같게 옮겼다.
//! - `apReportDur` 의 `Int(sec)` 는 범위 밖에서 멈추고 Rust `as i64` 는 끝값에 붙는다.
//! - 사전 일곱은 Swift 에서 파일 안 비공개 상수이고 Rust 에서는 `Option` 을 돌려주는 함수다(뒤 구간이 쓰므로 공개).
//! - 시간대: Foundation `TimeZone` 대신 `FixedZone`(UTC 에서 앞선 초). `Asia/Seoul`(`FixedZone::SEOUL`)·UTC 뿐이고 서머타임 시간대는 못 쓴다. 날짜 계산은 `rules::stopline::utc_time` 과 같은 civil_from_days 이고 stopline.rs 는 건드리지 않았다.
//! - `Writer`·`chip`·`css` 는 Swift 에서 비공개이고 Rust 에서는 뒤 구간(같은 파일)이 쓰므로 공개다. `chip` 의 기본 인자는 `chip_plain` 으로 나눴다. `Writer` 는 값 복사 대신 `&Writer`(`timeline` 은 읽기만 한다)로 건넨다.
//! - `Writer::date` 의 무늬는 보고서가 쓰는 글자(`y M d H m s` 와 따옴표 글)만 푼다. Foundation 의 나머지 무늬 글자는 지원하지 않는다.
//! - 비용 글자 `String(format: "%.2f USD")` 는 `format!("{:.2} USD")` 로: 둘 다 가장 가까운 자리로 반올림해 같은 글자를 낸다(JSON 수에는 NaN·∞ 이 없고, 기록의 합이
//!   넘칠 만큼 큰 때는 기록이 틀린 때뿐이다).
//! - 판정 끝부분의 `last.exit != 0` 은 Optional 비교라 종료 코드가 없을 때도 참이다: `last.exit != Some(0)` 으로 그대로 옮겼다.
//! - `timeline` 의 `String(Int(v.rounded()))` 는 `v.round() as i64` 로: 반올림 규칙(절반은 멀어진다)은 같고, 그림의 수는 다 작아 범위 밖이 없다.
//!
//! Windows 판단: 글 칸에 CRLF 가 들어와도(`\r\n` 은 한 글자) 자르는 자리·글자 수는 Swift 와 같다. 경로 가운데 `ResultChange::place` 는 받은 글자 그대로 두고,
//! 저장소 이름만 `last_part` 가 `/` 와 `\` 로 끊는다(`C:\work\todo-api` → `todo-api`). 이 변환은 홈 폴더 줄이기(`~/`)와 달리 그리는 쪽이 아니라 값을 만들 때 한다.
//! 홈 폴더 줄이기(`Writer::short`)는 홈 뒤에 `/` 하나를 붙여 찾는다(Swift 와 같다). 홈이 `C:\Users\x` 이고 경로가 `C:\Users\x\proj` 처럼 `\` 로 이어지면 줄이지 않는다(1:1 로 두었다).

use crate::ports::Row;
use crate::rules::formats::grapheme_len;
use crate::rules::prose::{escape_html, plain, safe_link};
use crate::rules::reading::ReadClock;
use crate::rules::sequence::{parse_sequence, sequence_svg, SeqParse};
use serde_json::{Map, Number, Value};

// ── 결과 기록 (result.json, 설계 5장 3절) ─────────────

/// 시험 한 줄. 원본: `APResultTest`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultTest {
    pub name: String,
    pub result: String,
    pub note: String,
}

/// 바뀐 것 한 줄. 원본: `APResultChange`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultChange {
    pub what: String,
    pub place: String,
}

/// 적용한 결정 한 줄. 원본: `APResultDecision`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultDecision {
    pub what: String,
    pub basis: String,
}

/// 흐름 그림(mermaid 원문). 원본: `APResultFlow`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultFlow {
    pub title: String,
    pub source: String,
}

/// 다음에 이어 할 일·새로 생긴 일 한 줄. 원본: `APResultCarry`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultCarry {
    pub title: String,
    pub why: String,
}

/// 실행 중에 새로 필요해진 티켓(4단계 ⑩-3) — 모델이 임시 번호(`TMP-<단위>-<차례>`)로 쓰고 결과 기록 `new_tickets` 에 남긴 것. 보드가 검수 뒤에 만든다. 원본: `APResultTicket`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultTicket {
    pub temp: String,
    pub title: String,
    pub why: String,
}

/// 참고 한 줄(키 차례). 원본: `APResultRef`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultRef {
    pub key: String,
    pub value: String,
}

/// 결과 기록 한 건. 원본: `APResultRecord`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultRecord {
    // unit 칸
    pub title: String,
    pub kind: String,
    pub r#ref: String,
    pub outcome: String,
    pub headline: String,
    pub next: String,
    pub summary: Vec<String>,
    pub risks: Vec<String>,
    pub tests: Vec<ResultTest>,
    pub changes: Vec<ResultChange>,
    pub decisions: Vec<ResultDecision>,
    pub flows: Vec<ResultFlow>,
    pub next_start: Vec<ResultCarry>,
    pub new_work: Vec<ResultCarry>,
    pub new_tickets: Vec<ResultTicket>,
    /// 키 차례
    pub refs: Vec<ResultRef>,
}

/// 글 한 칸의 글자 수 상한(넘으면 자르고 …). 원본: `apReportTextMax`
pub const REPORT_TEXT_MAX: usize = 4000;
/// 목록 한 칸의 줄 수 상한. 원본: `apReportListMax`
pub const REPORT_LIST_MAX: usize = 200;
/// 흐름 그림 원문 글자 수 상한. 원본: `apReportFlowMax`
pub const REPORT_FLOW_MAX: usize = 20_000;
/// 판정 출력 끝부분 글자 수 상한. 원본: `apReportTailMax`
pub const REPORT_TAIL_MAX: usize = 2000;

// 글자(Character) 수가 `n` 이하인 가장 긴 앞 조각의 끝 바이트 위치(글자 수가 `n` 이하면 글 전체). 앞 조각의 글자 수는 길어질수록 줄지 않는다:
// 그런 가장 긴 앞 조각이 앞 `n` 글자다(다음 글자가 새 덩어리를 열면 n+1 이 된다)
fn prefix_end(s: &str, n: usize) -> usize {
    // 글자 하나는 바이트 하나 이상이다
    if s.len() <= n || grapheme_len(s) <= n {
        return s.len();
    }
    let bounds: Vec<usize> = s.char_indices().map(|(i, _)| i).chain(std::iter::once(s.len())).collect();
    let (mut lo, mut hi) = (0, bounds.len() - 1);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if grapheme_len(&s[..bounds[mid]]) <= n {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    bounds[lo]
}

/// 글자(Character) 수가 `n` 을 넘으면 앞 `n` 글자에서 자르고 `…` 를 붙인다. 원본: `apCut`
pub fn cut(s: &str, n: usize) -> String {
    let end = prefix_end(s, n);
    if end == s.len() {
        s.to_string()
    } else {
        format!("{}…", &s[..end])
    }
}

// 수 → 글. 정수는 그대로, 실수는 짧은 왕복 꼴(정수 값이면 소수점 없이). 지수가 -5 아래거나 17 이상이면 `1e+20` 꼴
fn number_text(x: &Number) -> String {
    if let Some(i) = x.as_i64() {
        return i.to_string();
    }
    if let Some(u) = x.as_u64() {
        return u.to_string();
    }
    let f = x.as_f64().unwrap_or(0.0);
    let e = format!("{f:e}");
    if let Some((mant, exp)) = e.split_once('e') {
        if let Ok(exp) = exp.parse::<i32>() {
            if !(-5..17).contains(&exp) {
                return format!("{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
            }
        }
    }
    format!("{f}")
}

/// 글 칸 하나(상한 4000) — 글자는 그대로, 숫자는 글로, 그 밖(배열·객체·null·불리언)은 빈 글자. 원본: `apReportText`
pub fn report_text(v: Option<&Value>) -> String {
    report_text_n(v, REPORT_TEXT_MAX)
}

/// 글 칸 하나 — 상한 `n` 을 준다. 원본: `apReportText`
pub fn report_text_n(v: Option<&Value>, n: usize) -> String {
    match v {
        Some(Value::String(s)) => cut(s, n),
        Some(Value::Number(x)) => number_text(x),
        _ => String::new(),
    }
}

// 목록 칸: 배열의 앞 200 개 가운데 객체인 것만(상한을 먼저 자르고 객체를 고른다)
fn list<'a>(j: &'a Map<String, Value>, k: &str) -> Vec<&'a Map<String, Value>> {
    j.get(k).and_then(Value::as_array).map(|a| a.iter().take(REPORT_LIST_MAX).filter_map(Value::as_object).collect()).unwrap_or_default()
}

// 글 목록 칸: 배열의 앞 200 개를 글로 바꾸고 빈 것은 뺀다
fn text_list(j: &Map<String, Value>, k: &str) -> Vec<String> {
    j.get(k)
        .and_then(Value::as_array)
        .map(|a| a.iter().take(REPORT_LIST_MAX).map(|v| report_text(Some(v))).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// 결과 기록 글 → 값. JSON 객체가 아니면 `None`. 칸의 꼴이 틀리면(글 자리에 객체 등) 그 칸만 비운다. 긴 줄표는 그릴 때(`prose::plain`) 바꾼다. 원본: `apParseResult`
pub fn parse_result(text: Option<&str>) -> Option<ResultRecord> {
    let v: Value = serde_json::from_str(text?).ok()?;
    let j = v.as_object()?;
    let empty = Map::new();
    let u = j.get("unit").and_then(Value::as_object).unwrap_or(&empty);
    let mut r = ResultRecord {
        title: report_text_n(u.get("title"), 300),
        kind: report_text_n(u.get("kind"), 40),
        r#ref: report_text_n(u.get("ref"), 300),
        outcome: report_text_n(j.get("outcome"), 40),
        headline: report_text(j.get("headline")),
        next: report_text(j.get("next")),
        summary: text_list(j, "summary"),
        risks: text_list(j, "risks"),
        ..ResultRecord::default()
    };
    r.tests = list(j, "tests")
        .iter()
        .map(|m| ResultTest { name: report_text_n(m.get("name"), 500), result: report_text_n(m.get("result"), 40), note: report_text(m.get("note")) })
        .collect();
    r.changes = list(j, "changes").iter().map(|m| ResultChange { what: report_text(m.get("what")), place: report_text_n(m.get("where"), 500) }).collect();
    r.decisions = list(j, "decisions_applied").iter().map(|m| ResultDecision { what: report_text(m.get("what")), basis: report_text(m.get("basis")) }).collect();
    r.flows = list(j, "sequence")
        .iter()
        .map(|m| ResultFlow { title: report_text_n(m.get("title"), 300), source: report_text_n(m.get("mermaid"), REPORT_FLOW_MAX) })
        .collect();
    r.next_start = list(j, "next_start").iter().map(|m| ResultCarry { title: report_text(m.get("title")), why: report_text(m.get("why")) }).collect();
    r.new_work = list(j, "new_work").iter().map(|m| ResultCarry { title: report_text(m.get("title")), why: report_text(m.get("why")) }).collect();
    r.new_tickets = list(j, "new_tickets")
        .iter()
        .take(20)
        .map(|m| ResultTicket { temp: report_text_n(m.get("temp"), 20), title: report_text_n(m.get("title"), 300), why: report_text_n(m.get("why"), 1000) })
        .collect();
    if let Some(refs) = j.get("refs").and_then(Value::as_object) {
        let mut keys: Vec<&String> = refs.keys().collect();
        keys.sort();
        r.refs = keys
            .into_iter()
            .take(50)
            .filter_map(|k| {
                let v = report_text_n(refs.get(k), 1000);
                (!v.is_empty()).then(|| ResultRef { key: cut(k, 60), value: v })
            })
            .collect();
    }
    Some(r)
}

// ── 보고서 값 ────────────────────────────────────────

/// 결과 기록의 상태. 원본: `APResultState`(rawValue `ok`·`missing`·`broken`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultState {
    Ok,
    #[default]
    Missing,
    Broken,
}

impl ResultState {
    /// 원본 `rawValue`
    pub fn raw(self) -> &'static str {
        match self {
            ResultState::Ok => "ok",
            ResultState::Missing => "missing",
            ResultState::Broken => "broken",
        }
    }
}

/// 판정 한 번. 원본: `APReportCheck`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportCheck {
    pub n: i64,
    pub exit: Option<i64>,
    pub sec: f64,
    pub tail: String,
    pub timed_out: bool,
    pub interrupted: bool,
}

/// 단위 하나. `status` 의 처음 값은 `pending`, `result_state` 는 `Missing`. 원본: `APReportUnit`
#[derive(Debug, Clone, PartialEq)]
pub struct ReportUnit {
    pub n: i64,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub reason: String,
    pub task: String,
    pub story: String,
    pub branch: String,
    pub merge: String,
    pub active_sec: f64,
    pub cost: f64,
    pub turns: i64,
    pub retries: i64,
    pub started: Option<f64>,
    pub ended: Option<f64>,
    pub checks: Vec<ReportCheck>,
    pub last_exit: Option<i64>,
    pub denials: Vec<String>,
    pub result: Option<ResultRecord>,
    pub result_state: ResultState,
}

impl Default for ReportUnit {
    fn default() -> Self {
        ReportUnit {
            n: 0,
            title: String::new(),
            kind: String::new(),
            status: "pending".to_string(),
            reason: String::new(),
            task: String::new(),
            story: String::new(),
            branch: String::new(),
            merge: String::new(),
            active_sec: 0.0,
            cost: 0.0,
            turns: 0,
            retries: 0,
            started: None,
            ended: None,
            checks: Vec::new(),
            last_exit: None,
            denials: Vec::new(),
            result: None,
            result_state: ResultState::Missing,
        }
    }
}

/// 기록의 바탕: 실행 폴더 · 기록 저장소. 원본: `APReportOrigin`(Ports.swift, rawValue `folder`·`records`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportOrigin {
    #[default]
    Folder,
    Records,
}

/// 실행 하나의 보고서 값. 원본: `APReportRun`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportRun {
    pub id: String,
    pub title: String,
    pub project: String,
    pub model: String,
    pub state: String,
    pub reason: String,
    pub started: Option<f64>,
    pub ended: Option<f64>,
    /// worktree · in_place · 빈 글자(기록 없음)
    pub mode: String,
    /// 자율주행 가지
    pub work: String,
    /// 저장소 이름(마지막 경로 조각)
    pub repos: Vec<String>,
    /// "저장소: 출발 가지"
    pub bases: Vec<String>,
    pub harness_same: Option<bool>,
    pub harness_files: i64,
    pub harness_changed: Vec<String>,
    /// 움직인 기존 가지(0 이어야 함)
    pub moved: Vec<String>,
    pub stashed: i64,
    pub epic: String,
    pub units: Vec<ReportUnit>,
    pub origin: ReportOrigin,
}

// 쓸 곳 포트(`ReportSink`·`ReportFile`)는 파일 끝의 그리는 유스케이스 자리에 있다(원본 `APReportSink`·`APReportFile`, `Ports.swift`)

// ── 기록 → 보고서 값 ──────────────────────────────────

/// 보고서가 읽을 곳(원본 `APReportSource`, `Ports.swift`). 실행 폴더든 기록 저장소든 사전(JSON 객체)으로 돌려준다.
/// 포트 모음(`ports.rs`)에 아직 없어 이 파일에 둔다(옮기면 `use` 만 바뀐다).
pub trait ReportSource {
    fn origin(&self) -> ReportOrigin;
    fn run(&self) -> Option<Row>;
    fn queue(&self) -> Vec<Row>;
    fn engine(&self, n: i64) -> Option<Row>;
    /// 모델이 쓴 결과 기록 글(보통 파일이고 크기 상한 아래일 때만, 어댑터가 본다). 없으면 `None`
    fn result(&self, n: i64) -> Option<String>;
}

// `v as? NSNumber`(불리언 제외) → Double
fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(x)) => x.as_f64(),
        _ => None,
    }
}

// 유한하고 절댓값이 1e15 아래인 수만 정수로(소수는 0 쪽으로 버림)
fn int(v: Option<&Value>) -> Option<i64> {
    num(v).filter(|x| x.is_finite() && x.abs() < 1e15).map(|x| x as i64)
}

// `v as? String ?? ""`
fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

// `v as? String`
fn as_string(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

// `v as? [String: Any]`
fn object(v: Option<&Value>) -> Option<&Map<String, Value>> {
    v.and_then(Value::as_object)
}

// `v as? [[String: Any]] ?? []` — Swift 의 배열 형 바꾸기는 모두 객체여야 하고, 하나라도 아니면 통째로 실패해 빈 배열이 된다
fn objects(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    let Some(Value::Array(a)) = v else { return Vec::new() };
    a.iter().map(Value::as_object).collect::<Option<Vec<_>>>().unwrap_or_default()
}

/// `NSString.lastPathComponent`: 끝의 구분자를 떼고 마지막 구분자 뒤. 구분자만 있으면 그 첫 글자(`/`). 빈 글은 빈 글. 원본: `lastPart`(비공개)
/// Windows 판은 `\` 도 구분자로 본다(저장소 경로가 `C:\work\todo-api` 꼴로 와도 `todo-api` 가 되게).
pub fn last_part(p: &str) -> String {
    let t = p.trim_end_matches(['/', '\\']);
    if t.is_empty() {
        return p.chars().next().map(String::from).unwrap_or_default();
    }
    t.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

// 글자 수가 `n` 을 넘을 때 끝 `n` 글자(`String.suffix(n)`)
fn suffix_chars(s: &str, n: usize) -> &str {
    let total = grapheme_len(s);
    if total <= n {
        return s;
    }
    &s[prefix_end(s, total - n)..]
}

/// 기록을 읽어 보고서 값으로. run 이 없으면 `None`. 단위는 queue 의 차례(1부터)이고, 상태는 queue 의 status(엔진 판정)가 먼저다. 원본: `apReportRun`
pub fn report_run(id: &str, source: &dyn ReportSource) -> Option<ReportRun> {
    let run = source.run()?;
    let mut r = ReportRun { origin: source.origin(), id: id.to_string(), ..ReportRun::default() };
    r.title = report_text_n(run.get("title"), 300);
    r.project = report_text_n(run.get("project"), 120);
    r.model = report_text_n(run.get("model"), 80);
    r.state = report_text_n(run.get("state"), 40);
    r.reason = report_text_n(run.get("reason"), 1000);
    r.started = num(run.get("started"));
    r.ended = num(run.get("ended"));
    r.epic = report_text_n(run.get("epic"), 80);
    let empty = Map::new();
    let wt = object(run.get("worktree")).unwrap_or(&empty);
    r.mode = str_of(wt.get("mode"));
    r.work = str_of(wt.get("branch"));
    let br = object(run.get("branches")).unwrap_or(&empty);
    let repos = object(br.get("repos")).unwrap_or(&empty);
    let mut keys: Vec<&String> = repos.keys().collect();
    keys.sort();
    for k in keys {
        r.repos.push(last_part(k));
        let e = object(repos.get(k)).unwrap_or(&empty);
        if let Some(b) = as_string(object(e.get("base")).and_then(|b| b.get("branch"))) {
            r.bases.push(format!("{}: {b}", last_part(k)));
        }
        if r.work.is_empty() {
            if let Some(w) = as_string(e.get("work")) {
                r.work = w.to_string();
            }
        }
    }
    if r.repos.is_empty() {
        if let Some(rp) = as_string(wt.get("repo")) {
            r.repos = vec![last_part(rp)];
        }
    }
    if r.repos.is_empty() {
        if let Some(Value::Array(a)) = wt.get("repos") {
            r.repos = a.iter().filter_map(|v| as_string(Some(v))).map(last_part).collect();
            r.repos.sort();
        }
    }
    r.moved = objects(br.get("moved")).iter().map(|m| format!("{} {}", last_part(&str_of(m.get("repo"))), str_of(m.get("branch")))).collect();
    r.stashed = br.get("stashed").and_then(Value::as_array).map_or(0, |a| a.len() as i64);
    if let Some(h) = object(run.get("harness")) {
        r.harness_files = int(h.get("files")).unwrap_or(0);
        r.harness_same = h.get("same").and_then(Value::as_bool);
        r.harness_changed = objects(h.get("changed"))
            .iter()
            .take(30)
            .map(|c| {
                let change = str_of(c.get("change"));
                let what = match change.as_str() {
                    "changed" => "바뀜",
                    "added" => "더함",
                    "removed" => "없어짐",
                    other => other,
                };
                format!("{} ({what})", str_of(c.get("path")))
            })
            .collect();
    }
    for (i, q) in source.queue().iter().enumerate() {
        let n = i as i64 + 1;
        let e = source.engine(n).unwrap_or_default();
        let mut u = ReportUnit { n, ..ReportUnit::default() };
        u.title = report_text_n(q.get("title").or_else(|| e.get("title")), 300);
        u.kind = report_text_n(q.get("kind"), 40);
        let (qs, es) = (str_of(q.get("status")), str_of(e.get("status")));
        u.status = if !qs.is_empty() {
            qs
        } else if es.is_empty() {
            "pending".to_string()
        } else {
            es
        };
        u.reason = report_text_n(e.get("reason"), 1000);
        u.story = report_text_n(q.get("story"), 80);
        if !u.story.is_empty() {
            if let Some(t) = q.get("tickets").and_then(Value::as_array).and_then(|a| a.first()) {
                u.task = report_text_n(Some(t), 80);
            }
        }
        u.branch = if str_of(e.get("branch")).is_empty() { str_of(q.get("branch")) } else { str_of(e.get("branch")) };
        u.merge = str_of(object(e.get("merge")).and_then(|m| m.get("state")));
        u.active_sec = num(e.get("active_sec")).unwrap_or(0.0);
        u.cost = num(e.get("cost_usd")).unwrap_or(0.0);
        u.turns = int(e.get("turns")).unwrap_or(0);
        u.retries = int(e.get("try")).unwrap_or(0);
        u.started = num(e.get("started"));
        u.ended = num(e.get("ended"));
        u.checks = objects(e.get("checks"))
            .iter()
            .take(50)
            .enumerate()
            .map(|(k, c)| {
                let tail = report_text_n(c.get("tail"), 100_000);
                ReportCheck {
                    n: int(c.get("try")).map_or(k as i64 + 1, |t| t + 1),
                    exit: int(c.get("exit")),
                    sec: num(c.get("sec")).unwrap_or(0.0),
                    tail: if grapheme_len(&tail) > REPORT_TAIL_MAX { format!("…{}", suffix_chars(&tail, REPORT_TAIL_MAX)) } else { tail },
                    timed_out: c.get("timed_out").and_then(Value::as_bool).unwrap_or(false),
                    interrupted: c.get("interrupted").and_then(Value::as_bool).unwrap_or(false),
                }
            })
            .collect();
        u.last_exit = u.checks.last().and_then(|c| c.exit).or_else(|| int(e.get("check_exit")));
        u.denials = objects(e.get("denials"))
            .iter()
            .take(30)
            .map(|d| [str_of(d.get("tool")), report_text_n(d.get("what"), 300)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": "))
            .collect();
        if let Some(t) = source.result(n) {
            match parse_result(Some(&t)) {
                Some(p) => {
                    u.result = Some(p);
                    u.result_state = ResultState::Ok;
                }
                None => u.result_state = ResultState::Broken,
            }
        }
        if u.kind.is_empty() {
            if let Some(k) = u.result.as_ref().map(|p| p.kind.clone()) {
                u.kind = k;
            }
        }
        r.units.push(u);
    }
    Some(r)
}

// ── 보고서 HTML 의 이름표(원본 234~241줄, Swift 에서는 파일 안 비공개 사전) ──────────

/// 단위 상태 이름표. 원본: `unitLabel`
pub fn unit_label(k: &str) -> Option<&'static str> {
    Some(match k {
        "pending" => "대기",
        "running" => "진행 중",
        "done" => "통과",
        "failed" => "미통과",
        "capped" => "상한에서 멈춤",
        "stopped" => "중단",
        "skipped" => "건너뜀",
        _ => return None,
    })
}

/// 단위 상태 칩의 클래스. 원본: `unitClass`
pub fn unit_class(k: &str) -> Option<&'static str> {
    Some(match k {
        "done" => "ok",
        "failed" => "fail",
        "capped" => "cap",
        "running" => "run",
        _ => return None,
    })
}

/// 실행 상태 이름표. 원본: `runLabel`
pub fn run_label(k: &str) -> Option<&'static str> {
    Some(match k {
        "starting" => "출발 중",
        "running" => "도는 중",
        "waiting_limit" => "한도 대기",
        "waiting_stopline" => "멈출 선 대기",
        "waiting_precheck" => "환경 확인 대기",
        "paused" => "일시정지",
        "stopped" => "중단됨",
        "done" => "끝남",
        "error" => "오류로 멈춤",
        _ => return None,
    })
}

/// 종류 이름표(키는 소문자로 접은 글). 원본: `kindLabel`
pub fn kind_label(k: &str) -> Option<&'static str> {
    Some(match k {
        "fix" => "고침",
        "dev" | "feature" | "story" => "개발",
        "test" => "시험",
        "doc" | "docs" => "문서",
        "env" => "환경",
        _ => return None,
    })
}

/// 결과 기록의 끝맺음 이름표. 원본: `outcomeLabel`
pub fn outcome_label(k: &str) -> Option<&'static str> {
    Some(match k {
        "done" => "끝냄",
        "failed" => "끝내지 못함",
        "parked" => "보류",
        "capped" => "상한",
        _ => return None,
    })
}

/// 시험 결과 이름표와 클래스. 원본: `testLabel`
pub fn test_label(k: &str) -> Option<(&'static str, &'static str)> {
    Some(match k {
        "pass" => ("통과", "ok"),
        "fail" => ("실패", "fail"),
        "skip" => ("건너뜀", ""),
        _ => return None,
    })
}

/// 합치기 상태 이름표. 원본: `mergeLabel`
pub fn merge_label(k: &str) -> Option<&'static str> {
    Some(match k {
        "merged" => "스토리 가지에 합침",
        "nothing" => "합칠 커밋 없음",
        "not_merged" => "스토리 가지에 합치지 않음",
        _ => return None,
    })
}

/// 걸린 시간 — 1분 안이면 초, 한 시간 안이면 분, 그 위는 시간·분(보드 `apDur` 과 같은 꼴). 원본: `apReportDur`
pub fn report_dur(sec: f64) -> String {
    // Swift `Int(sec)` 는 범위 밖이면 멈추지만 Rust `as` 는 끝값으로 붙는다(아주 큰 수는 시간 칸이 아주 커질 뿐)
    let s = (if sec.is_finite() { sec } else { 0.0 } as i64).max(0);
    if s == 0 {
        return "0분".to_string();
    }
    if s < 60 {
        return format!("{s}초");
    }
    if s >= 3600 {
        format!("{}시간 {}분", s / 3600, s % 3600 / 60)
    } else {
        format!("{}분", s / 60)
    }
}

/// 종류 글자 → 이름표(없으면 그대로). 원본: `kind`(비공개 함수)
pub fn kind_text(k: &str) -> String {
    kind_label(&k.to_lowercase()).map_or_else(|| k.to_string(), str::to_string)
}

// ── 보고서 HTML: 글 만들기 (원본 253~334줄) ─────────────

/// 고정 시간대(UTC 에서 몇 초 앞서나). 원본은 Foundation `TimeZone` 이고 보고서는 한국(`Asia/Seoul`, UTC+9, 서머타임 없음)이나 UTC 로만 그린다.
/// 서머타임이 있는 시간대는 표현하지 못한다(`rules::stopline::Tz` 는 UTC 와 이 머신 시간대 둘뿐이라 이 파일 안에서 따로 둔다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedZone {
    pub offset_secs: i32,
}

impl FixedZone {
    /// `Asia/Seoul` — UTC+9, 서머타임 없음.
    pub const SEOUL: FixedZone = FixedZone { offset_secs: 9 * 3600 };
    /// `UTC`
    pub const UTC: FixedZone = FixedZone { offset_secs: 0 };

    /// 유닉스 초 → 이 시간대의 달력 날짜·시각. 초의 소수는 버린다(음수는 내림, Foundation 이 초 단위로 끊는 것과 같다).
    /// 아주 큰 수는 `i64` 끝값에 붙는다(원본은 범위 밖 `Date` 를 그대로 둔다, 결과 기록 칸에 그런 때가 오는 것은 모델이 틀리게 쓴 때뿐이다).
    pub fn civil(&self, ts: f64) -> CivilTime {
        let secs = (ts.floor() as i64).saturating_add(i64::from(self.offset_secs));
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        // 그레고리력 날짜 계산(Howard Hinnant 의 civil_from_days, `rules::stopline::utc_time` 과 같다)
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        CivilTime {
            year: yoe + era * 400 + i64::from(month <= 2),
            month,
            day: (doy - (153 * mp + 2) / 5 + 1) as u32,
            hour: (rem / 3600) as u32,
            minute: (rem % 3600 / 60) as u32,
            second: (rem % 60) as u32,
        }
    }
}

/// 달력 날짜·시각(그레고리력). `FixedZone::civil` 이 만든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// 글 만들기. 원본: `Writer`(비공개 구조체). `tz` 는 보일 시간대, `home` 은 `~` 로 줄일 홈 폴더, 만든 글은 `output` 으로 읽는다.
#[derive(Debug, Clone)]
pub struct Writer {
    pub tz: FixedZone,
    pub home: Option<String>,
    o: String,
}

impl Writer {
    pub fn new(tz: FixedZone, home: Option<&str>) -> Self {
        Writer { tz, home: home.map(str::to_string), o: String::new() }
    }

    /// 모델·사람이 쓴 글(긴 줄표 바꿈 + 이스케이프). 원본: `t`
    pub fn t(&self, s: &str) -> String {
        escape_html(&plain(s))
    }

    /// 경로·가지 같은 이름(그대로, 홈 줄이기 + 이스케이프). 원본: `c`
    pub fn c(&self, s: &str) -> String {
        escape_html(&self.short(s))
    }

    /// 홈 폴더를 `~` 로(보고서가 바깥에 첨부될 수 있어 사용자 이름이 든 경로를 줄인다). 홈이 한 글자 이하(`/` 만)면 그대로. 원본: `short`
    /// 홈 뒤에 붙이는 것은 `/` 하나다(Swift 와 같다): 엔진이 Windows 경로(`C:\Users\x\proj`)를 기록하면 줄이지 못한다.
    pub fn short(&self, s: &str) -> String {
        match &self.home {
            Some(h) if grapheme_len(h) > 1 => s.replace(&format!("{h}/"), "~/"),
            _ => s.to_string(),
        }
    }

    /// 때 글자. 원본: `date`. 시각이 없거나 유한하지 않으면 `때 모름`.
    /// 무늬는 Foundation `DateFormatter`(`ko_KR`·그레고리력) 글자 중 보고서가 쓰는 것만 푼다: `y`(한 글자는 그대로, 둘은 끝 두 자리, 셋 이상은 그 자리 수까지 0 채움) ·
    /// `M`·`d`·`H`·`m`·`s`(한 글자는 그대로, 둘 이상은 두 자리 0 채움) · `'…'` 따옴표 글(`''` 은 따옴표 하나). 다른 글자는 그대로 쓴다(영문자 가운데 쓰이지 않는 것을
    /// Foundation 은 다르게 풀 수 있다). 서기 1년 아래는 Foundation 이 시대를 따로 쓰는데 여기는 부호 있는 연도를 그대로 쓴다.
    pub fn date(&self, ts: Option<f64>, f: &str) -> String {
        let Some(ts) = ts.filter(|t| t.is_finite()) else {
            return "때 모름".to_string();
        };
        let t = self.tz.civil(ts);
        let chars: Vec<char> = f.chars().collect();
        let mut o = String::new();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if ch == '\'' {
                // 따옴표 글: `''` 은 따옴표 하나, 그 밖은 닫는 따옴표까지 그대로(`''` 은 안에서도 따옴표 하나)
                if chars.get(i + 1) == Some(&'\'') {
                    o.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            o.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    o.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            let run = chars[i..].iter().take_while(|c| **c == ch).count();
            let v = match ch {
                'y' => Some(t.year),
                'M' => Some(i64::from(t.month)),
                'd' => Some(i64::from(t.day)),
                'H' => Some(i64::from(t.hour)),
                'm' => Some(i64::from(t.minute)),
                's' => Some(i64::from(t.second)),
                _ => None,
            };
            match v {
                Some(v) if ch == 'y' && run == 2 => o.push_str(&format!("{:02}", v.rem_euclid(100))),
                Some(v) if ch == 'y' => o.push_str(&format!("{:0w$}", v, w = if run >= 3 { run } else { 1 })),
                Some(v) => o.push_str(&format!("{:0w$}", v, w = if run >= 2 { 2 } else { 1 })),
                None => o.push_str(&ch.to_string().repeat(run)),
            }
            i += run;
        }
        o
    }

    /// 만든 글 뒤에 이어 붙인다. 원본: `add`
    pub fn add(&mut self, s: &str) {
        self.o.push_str(s);
    }

    /// 지금까지 만든 글. 원본: `w.o`
    pub fn output(&self) -> &str {
        &self.o
    }

    /// 만든 글을 넘겨 준다.
    pub fn into_output(self) -> String {
        self.o
    }
}

/// 칩(알약 모양 이름표). 글은 이미 이스케이프된 것을 받는다. 원본: `chip`(비공개 함수, 둘째 인자 기본값 `""` 은 `chip_plain`)
pub fn chip(text: &str, cls: &str) -> String {
    format!("<span class=\"chip{}\">{}</span>", if cls.is_empty() { String::new() } else { format!(" {cls}") }, text)
}

/// 칩, 클래스 없음. 원본: `chip(text)`
pub fn chip_plain(text: &str) -> String {
    chip(text, "")
}

/// 보고서에 그대로 넣는 스타일(바깥 요청 없음). 원본: `css`(Swift 여러 줄 글자의 줄 끝 `\` 는 줄 이음이라 지금은 한 줄로 이어 붙인 값이다).
/// 시험이 골든 HTML 의 `<style>` 안과 바이트로 맞춰 본다.
pub const CSS: &str = r##":root{color-scheme:light dark;--bg:#f6f7f8;--card:#ffffff;--fg:#1f2328;--muted:#57606a;--line:#d8dee4;--soft:#eef1f4;--code:#f1f3f5;--accent:#0b62d6;--ok:#1a7f37;--ok-bg:#dcf5e3;--fail:#c4252f;--fail-bg:#ffe9e8;--cap:#8a5a00;--cap-bg:#fff3c4;--run:#0b62d6;--run-bg:#e2edff;--chip:#e9edf1;--note:#fff6cc;--note-line:#c9a227;--frame:#8c959f;--tag:#e9edf1}
@media (prefers-color-scheme:dark){:root{--bg:#101215;--card:#181b20;--fg:#e6edf3;--muted:#9aa4af;--line:#30363d;--soft:#20252c;--code:#1c2027;--accent:#6aa8ff;--ok:#56d37f;--ok-bg:#132a1b;--fail:#ff8078;--fail-bg:#311719;--cap:#e3a93a;--cap-bg:#2e2410;--run:#6aa8ff;--run-bg:#14243d;--chip:#232830;--note:#2d2712;--note-line:#8f7425;--frame:#76808b;--tag:#232830}}
*{box-sizing:border-box}
html{background:var(--bg)}
body{margin:0 auto;max-width:960px;padding:24px 16px 40px;background:var(--bg);color:var(--fg);font:14px/1.6 -apple-system,BlinkMacSystemFont,"Apple SD Gothic Neo","Malgun Gothic","Segoe UI",sans-serif;word-break:keep-all;overflow-wrap:anywhere}
header,nav,section{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:18px 20px;margin:0 0 14px}
h1{font-size:21px;line-height:1.35;margin:2px 0 4px}
h2{font-size:16.5px;line-height:1.4;margin:0 0 4px}
h3{font-size:12.5px;color:var(--muted);font-weight:600;margin:16px 0 4px}h3.top{margin-top:0}
p{margin:4px 0}
.mark{color:var(--muted);font-size:12px;margin:0}
.meta{color:var(--muted);font-size:12.5px}
.chips{display:flex;flex-wrap:wrap;gap:6px;margin:10px 0 4px}
.chip{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12px;line-height:1.7;background:var(--chip);white-space:nowrap}
.chip.ok{background:var(--ok-bg);color:var(--ok)}.chip.fail{background:var(--fail-bg);color:var(--fail)}
.chip.cap{background:var(--cap-bg);color:var(--cap)}.chip.run{background:var(--run-bg);color:var(--run)}
dl{display:grid;grid-template-columns:max-content 1fr;gap:3px 14px;margin:10px 0 0;font-size:13px}
dt{color:var(--muted)}dd{margin:0}
nav ol{margin:4px 0 0;padding-left:24px}nav li{margin:2px 0}
a{color:var(--accent)}nav a{text-decoration:none}
.headline{font-size:15px;font-weight:600;margin:10px 0 2px}
ul{margin:4px 0;padding-left:20px}li{margin:2px 0}
ul.risk li::marker{color:var(--cap)}
table{border-collapse:collapse;width:100%;font-size:13px;margin:2px 0}
th,td{border-bottom:1px solid var(--line);padding:5px 8px;text-align:left;vertical-align:top}
th{color:var(--muted);font-weight:600}
code,pre{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px}
code{background:var(--code);border-radius:5px;padding:1px 5px}
pre{background:var(--code);border-radius:8px;padding:10px 12px;margin:6px 0;overflow:auto;white-space:pre-wrap}
details{margin:6px 0}summary{color:var(--muted);font-size:12.5px;cursor:pointer}
figure{margin:8px 0 12px}figcaption{color:var(--muted);font-size:12.5px;margin:0 0 4px}
.pic{overflow-x:auto;border:1px solid var(--line);border-radius:8px;padding:8px;background:var(--bg)}
.pic svg{display:block;max-width:none}.pic svg.tl{width:100%;height:auto}
svg text{font-family:-apple-system,BlinkMacSystemFont,"Apple SD Gothic Neo","Malgun Gothic","Segoe UI",sans-serif}
.sq .pb{fill:var(--soft);stroke:var(--frame)}.sq .pt{fill:var(--fg);font-size:13px}
.sq .ll{stroke:var(--frame);stroke-dasharray:3 4}
.sq .ml{stroke:var(--fg);stroke-width:1.3;fill:none}.sq .ml.d{stroke-dasharray:5 4}
.sq .ah{fill:var(--fg)}.sq .mt{fill:var(--fg);font-size:12.5px}
.sq .nb{fill:var(--note);stroke:var(--note-line)}.sq .nt{fill:var(--fg);font-size:12.5px}
.sq .fr{fill:none;stroke:var(--frame)}.sq .fr.rc{fill:var(--soft);fill-opacity:.5;stroke-dasharray:2 3}
.sq .ft{fill:var(--tag);stroke:var(--frame)}.sq .fl{fill:var(--fg);font-size:11.5px;font-weight:600}
.sq .fc{fill:var(--muted);font-size:11.5px}.sq .fs{stroke:var(--frame);stroke-dasharray:4 3}
.tl .ax{stroke:var(--line)}.tl .lb{fill:var(--muted);font-size:11px}.tl .bar{fill:var(--frame)}
.tl .ok{fill:var(--ok)}.tl .fail{fill:var(--fail)}.tl .cap{fill:var(--cap)}.tl .run{fill:var(--run)}
.empty{color:var(--muted)}
footer{color:var(--muted);font-size:12px;margin:16px 4px 0}
@media (max-width:560px){header,nav,section{padding:14px}dl{grid-template-columns:1fr}dt{margin-top:6px}}"##;

// ── 보고서 HTML 그리기 (원본 337~539줄) ─────────────

/// 실행 보고서(`only` 가 `None`) 또는 단위 하나의 보고서. `at` 은 그린 때(유닉스 초), `tz` 는 보일 시간대, `home` 은 ~ 로 줄일 홈 폴더.
/// 원본: `apReportHTML`(`only` 의 기본값 `nil` 은 `Option<i64>` 로 건넨다). 그리는 유스케이스(`draw_reports`)는 파일 끝에 있다.
pub fn report_html(r: &ReportRun, only: Option<i64>, at: f64, tz: &FixedZone, home: Option<&str>) -> String {
    let mut w = Writer::new(*tz, home);
    let units: Vec<&ReportUnit> = match only {
        Some(k) => r.units.iter().filter(|u| u.n == k).collect(),
        None => r.units.iter().collect(),
    };
    let name: &str = if r.title.is_empty() { if r.project.is_empty() { &r.id } else { &r.project } } else { &r.title };
    let doc_title = match only {
        Some(k) => format!("자율주행 단위 보고서: {name} 단위 {k}"),
        None => format!("자율주행 보고서: {name}"),
    };
    w.add("<!doctype html>\n<html lang=\"ko\">\n<head>\n<meta charset=\"utf-8\">\n");
    w.add("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    // 바깥 요청을 모두 막는다(스크립트·글꼴·그림·연결·폼·base). 이 파일만 받아 다른 브라우저로 열어도 같다
    w.add("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\">\n");
    w.add("<meta name=\"referrer\" content=\"no-referrer\">\n");
    w.add("<meta name=\"generator\" content=\"Claude Board 자율주행 보고서 틀 1\">\n");
    w.add(&format!("<title>{}</title>\n<style>\n{CSS}\n</style>\n</head>\n<body>\n", w.t(&doc_title)));

    // 머리 — 실행 이름, 프로젝트·때·모델, 결과 칩, 사실 목록
    let all = &r.units;
    let count = |s: &str| all.iter().filter(|u| u.status == s).count();
    // 사실 한 줄. 원본은 지역 함수 `fact`
    fn fact(w: &mut Writer, k: &str, v: &str) {
        w.add(&format!("<dt>{k}</dt><dd>{v}</dd>\n"));
    }
    w.add(&format!(
        "<header>\n<p class=\"mark\">{}</p>\n<h1>{}</h1>\n",
        if only.is_none() { "자율주행 보고서" } else { "자율주행 단위 보고서" },
        w.t(name)
    ));
    let mut meta: Vec<String> = Vec::new();
    if !r.project.is_empty() && !r.title.is_empty() {
        meta.push(w.t(&r.project));
    }
    let end_part = match r.ended {
        Some(e) => format!(" ~ {}", w.date(Some(e), "M월 d일 HH:mm")),
        None => String::new(),
    };
    meta.push(format!("{}{end_part}", w.date(r.started, "yyyy년 M월 d일 HH:mm")));
    if !r.model.is_empty() {
        meta.push(format!("모델 {}", w.t(&r.model)));
    }
    meta.push(format!("실행 <code>{}</code>", w.c(&r.id)));
    w.add(&format!("<p class=\"meta\">{}</p>\n<div class=\"chips\">", meta.join(" · ")));
    let st = r.state.as_str();
    let run_txt: String = match run_label(st) {
        Some(l) => l.to_string(),
        None => {
            if st.is_empty() {
                "상태 모름".to_string()
            } else {
                st.to_string()
            }
        }
    };
    let run_cls = if st == "done" {
        "ok"
    } else if st == "error" {
        "fail"
    } else if st == "stopped" || st.is_empty() {
        ""
    } else {
        "run"
    };
    w.add(&chip(&w.t(&run_txt), run_cls));
    let done = count("done");
    w.add(&chip(&format!("{}개 중 {done}개 통과", all.len()), if done == all.len() && !all.is_empty() { "ok" } else { "" }));
    for (s, label, cls) in [
        ("failed", "미통과", "fail"),
        ("capped", "상한에서 멈춤", "cap"),
        ("skipped", "건너뜀", ""),
        ("stopped", "중단", ""),
        ("pending", "대기", ""),
        ("running", "진행 중", "run"),
    ] {
        // Swift `where count(s) > 0` — 세는 것만 칩을 넣는다
        let n = count(s);
        if n > 0 {
            w.add(&chip(&format!("{label} {n}"), cls));
        }
    }
    w.add("</div>\n");
    if !r.reason.is_empty() {
        w.add(&format!("<p>{}</p>\n", w.t(&r.reason)));
    }
    w.add("<dl>\n");
    let active: f64 = all.iter().map(|u| u.active_sec).sum();
    let cost: f64 = all.iter().map(|u| u.cost).sum();
    if let Some(s) = r.started {
        let dur = report_dur(r.ended.unwrap_or(at) - s);
        let suffix = if r.ended.is_none() { " (아직 도는 중)" } else { "" };
        fact(&mut w, "걸린 시간", &format!("{dur}{suffix}"));
    }
    fact(&mut w, "일한 시간", &format!("{} (단위 시계 합, 대기·일시정지 뺌)", report_dur(active)));
    fact(&mut w, "비용", &format!("{cost:.2} USD"));
    if !r.mode.is_empty() {
        let place = if r.mode == "worktree" {
            "따로 작업 트리"
        } else if r.mode == "in_place" {
            "원래 작업 트리"
        } else {
            r.mode.as_str()
        };
        let repos_part = if r.repos.is_empty() {
            String::new()
        } else {
            format!(" · 저장소 {}", r.repos.iter().map(|rp| format!("<code>{}</code>", w.c(rp))).collect::<Vec<_>>().join(" "))
        };
        // fact 가 w 를 고쳐 쓰는 동시에 읽지 않게, 인자 글자를 먼저 만든다
        let place_txt = w.t(place);
        fact(&mut w, "어디서", &format!("{}{repos_part}", place_txt));
    }
    if !r.work.is_empty() {
        let bases_part = if r.bases.is_empty() {
            String::new()
        } else {
            format!(" (출발 {})", r.bases.iter().map(|b| w.c(b)).collect::<Vec<_>>().join(", "))
        };
        let work = format!("<code>{}</code>", w.c(&r.work));
        fact(&mut w, "자율주행 가지", &format!("{work}{bases_part}"));
    }
    if !r.epic.is_empty() {
        let epic_txt = w.t(&r.epic);
        fact(&mut w, "에픽", &epic_txt);
    }
    let harness_part = match r.harness_same {
        Some(true) => format!("출발 때와 같음 (파일 {}개)", r.harness_files),
        Some(false) => format!("출발 때와 다름 {}곳", r.harness_changed.len()),
        None => "<span class=\"empty\">끝 검사 기록 없음</span>".to_string(),
    };
    fact(&mut w, "하네스", &harness_part);
    if !r.moved.is_empty() {
        let moved_txt = r.moved.iter().map(|m| format!("<code>{}</code>", w.c(m))).collect::<Vec<_>>().join(" ");
        fact(&mut w, "움직인 기존 가지", &moved_txt);
    }
    if r.stashed > 0 {
        fact(&mut w, "따로 보관한 변경", &format!("{}번 (읽기 전용 자리에 남은 변경, git stash list 로 봅니다)", r.stashed));
    }
    w.add("</dl>\n");
    if r.harness_same == Some(false) && !r.harness_changed.is_empty() {
        w.add(&format!(
            "<details><summary>바뀐 하네스 파일</summary><ul>{}</ul></details>\n",
            r.harness_changed.iter().map(|f| format!("<li><code>{}</code></li>", w.c(f))).collect::<String>()
        ));
    }
    w.add("</header>\n");

    if only.is_none() {
        // 차례와 단위가 돈 때(그림)
        if !units.is_empty() {
            w.add("<nav aria-label=\"단위 차례\">\n<h3 class=\"top\">단위</h3>\n<ol>\n");
            for u in units.iter().copied() {
                let link_title = if u.title.is_empty() { format!("단위 {}", u.n) } else { u.title.clone() };
                let ul: String = match unit_label(&u.status) {
                    Some(l) => l.to_string(),
                    None => u.status.clone(),
                };
                let uc = unit_class(&u.status).unwrap_or("");
                w.add(&format!("<li><a href=\"#unit-{}\">{}</a> {}</li>\n", u.n, w.t(&link_title), chip(&w.t(&ul), uc)));
            }
            w.add("</ol>\n");
            let tl = timeline(r, at, &w);
            if let Some(tl) = tl {
                w.add(&format!("<figure><figcaption>단위가 돈 때</figcaption><div class=\"pic\">{tl}</div></figure>\n"));
            }
            w.add("</nav>\n");
        }
    }
    for u in units.iter().copied() {
        unit_section(u, &mut w);
    }
    if units.is_empty() {
        w.add("<section><p class=\"empty\">단위 기록이 없습니다.</p></section>\n");
    }

    w.add("<footer>Claude Board 가 결과 기록(모델이 씀)과 엔진 기록으로 그렸습니다. 모델을 부르지 않았고, 바깥 글꼴·스크립트·그림 없이 이 파일 하나로 보입니다. ");
    w.add("단위의 통과·미통과는 엔진의 판정입니다. 모델 글의 긴 줄표는 쉼표로 바꿔 옮겼습니다. ");
    let base = if r.origin == ReportOrigin::Folder { "실행 폴더" } else { "기록 저장소(실행 폴더를 지워 판정 출력은 없습니다)" };
    w.add(&format!("그린 때 {} · 바탕 {base}.</footer>\n", w.date(Some(at), "yyyy년 M월 d일 HH:mm")));
    w.add("</body>\n</html>\n");
    w.into_output()
}

fn unit_section(u: &ReportUnit, w: &mut Writer) {
    w.add(&format!("<section id=\"unit-{}\">\n", u.n));
    let title: &str = if u.title.is_empty() { "제목 없음" } else { &u.title };
    let ul: String = match unit_label(&u.status) {
        Some(l) => l.to_string(),
        None => u.status.clone(),
    };
    let uc = unit_class(&u.status).unwrap_or("");
    w.add(&format!("<h2>단위 {}. {} {}</h2>\n", u.n, w.t(title), chip(&w.t(&ul), uc)));
    let mut meta: Vec<String> = Vec::new();
    if !u.kind.is_empty() {
        meta.push(w.t(&kind_text(&u.kind)));
    }
    if !u.task.is_empty() {
        let story = if u.story.is_empty() { String::new() } else { format!(" · 스토리 {}", w.t(&u.story)) };
        meta.push(format!("작업 {}{story}", w.t(&u.task)));
    }
    if !u.branch.is_empty() {
        meta.push(format!("가지 <code>{}</code>", w.c(&u.branch)));
    }
    if u.started.is_some() || u.active_sec > 0.0 {
        meta.push(format!("일한 시간 {}", report_dur(u.active_sec)));
    }
    meta.push(format!("{:.2} USD", u.cost));
    if u.turns > 0 {
        meta.push(format!("턴 {}", u.turns));
    }
    if let Some(s) = u.started {
        let end_part = match u.ended {
            Some(e) => format!(" ~ {}", w.date(Some(e), "HH:mm")),
            None => String::new(),
        };
        meta.push(format!("{}{end_part}", w.date(Some(s), "HH:mm")));
    }
    w.add(&format!("<p class=\"meta\">{}</p>\n", meta.join(" · ")));
    if !u.reason.is_empty() && u.status != "done" {
        w.add(&format!("<p class=\"meta\">엔진: {}</p>\n", w.t(&u.reason)));
    }
    if let Some(m) = merge_label(&u.merge) {
        w.add(&format!("<p class=\"meta\">{m}</p>\n"));
    }

    match u.result_state {
        ResultState::Missing => {
            let why = if u.status == "skipped" || u.status == "pending" { "(단위를 시작하지 않았습니다)" } else { "(모델이 남기지 않았습니다)" };
            w.add(&format!("<p class=\"empty\">결과 기록이 없습니다{why}.</p>\n"));
        }
        ResultState::Broken => w.add("<p class=\"empty\">결과 기록을 읽지 못했습니다(JSON 객체가 아닙니다).</p>\n"),
        ResultState::Ok => {}
    }
    if let Some(rec) = &u.result {
        // 원본은 `r` 로 둔다(이 함수 안에서만 쓰는 이름)
        if !rec.headline.is_empty() {
            w.add(&format!("<p class=\"headline\">{}</p>\n", w.t(&rec.headline)));
        }
        if let Some(o) = outcome_label(&rec.outcome) {
            w.add(&format!("<p class=\"meta\">모델이 적은 결론: {o}</p>\n"));
        }
        let summary: Vec<String> = rec.summary.iter().map(|s| w.t(s)).collect();
        section_list("한 일", &summary, w);
        if !rec.tests.is_empty() {
            w.add("<h3>시험</h3>\n<table><thead><tr><th>시험</th><th>결과</th><th>메모</th></tr></thead><tbody>\n");
            for x in &rec.tests {
                // Swift `testLabel[x.result] ?? (x.result, "")`
                let (l, cls) = test_label(&x.result).unwrap_or((x.result.as_str(), ""));
                w.add(&format!("<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n", w.t(&x.name), chip(&w.t(l), cls), w.t(&x.note)));
            }
            w.add("</tbody></table>\n");
        }
        let changes: Vec<String> = rec
            .changes
            .iter()
            .map(|c| {
                let place = if c.place.is_empty() { String::new() } else { format!(" <code>{}</code>", w.c(&plain(&c.place))) };
                format!("{}{place}", w.t(&c.what))
            })
            .collect();
        section_list("바꾼 것", &changes, w);
        let decisions: Vec<String> = rec
            .decisions
            .iter()
            .map(|d| {
                let basis = if d.basis.is_empty() { String::new() } else { format!(" <span class=\"meta\">(근거: {})</span>", w.t(&d.basis)) };
                format!("{}{basis}", w.t(&d.what))
            })
            .collect();
        section_list("따른 결정", &decisions, w);
        if !rec.flows.is_empty() {
            w.add("<h3>흐름 그림</h3>\n");
            for (k, f) in rec.flows.iter().enumerate() {
                flow(f, u.n, k + 1, w);
            }
        }
        if !rec.risks.is_empty() {
            w.add(&format!(
                "<h3>알아 둘 위험</h3>\n<ul class=\"risk\">{}</ul>\n",
                rec.risks.iter().map(|ri| format!("<li>{}</li>", w.t(ri))).collect::<String>()
            ));
        }
        let next_start: Vec<String> = rec
            .next_start
            .iter()
            .map(|c| {
                let why = if c.why.is_empty() { String::new() } else { format!(": {}", w.t(&c.why)) };
                format!("{}{why}", w.t(&c.title))
            })
            .collect();
        section_list("사람 몫이라 미룬 것(다음 출발 카드)", &next_start, w);
        let new_work: Vec<String> = rec
            .new_work
            .iter()
            .map(|c| {
                let why = if c.why.is_empty() { String::new() } else { format!(": {}", w.t(&c.why)) };
                format!("{}{why}", w.t(&c.title))
            })
            .collect();
        section_list("새로 찾은 일(다음 출발 대기열 후보)", &new_work, w);
        if !rec.next.is_empty() {
            w.add(&format!("<h3>다음에 할 일</h3>\n<p>{}</p>\n", w.t(&rec.next)));
        }
        if !rec.refs.is_empty() {
            w.add("<h3>참고</h3>\n<ul>");
            for rf in &rec.refs {
                let v = plain(&rf.value);
                match safe_link(&rf.value) {
                    // 링크는 safe_link 이 통과시킨 글만 낸다. 화면 글자는 홈 줄이기 + 이스케이프
                    Some(link) => w.add(&format!(
                        "<li>{}: <a href=\"{}\" rel=\"noopener noreferrer\">{}</a></li>",
                        w.t(&rf.key),
                        escape_html(&link),
                        w.c(&link)
                    )),
                    None => w.add(&format!("<li>{}: {}</li>", w.t(&rf.key), w.t(&v))),
                }
            }
            w.add("</ul>\n");
        }
    }

    // 판정(엔진) — 차례마다 종료 코드·시간, 통과하지 못한 마지막 판정의 출력 끝부분
    if !u.checks.is_empty() || u.last_exit.is_some() {
        w.add("<h3>판정</h3>\n");
        match (u.checks.is_empty(), u.last_exit) {
            // Swift `if checks.isEmpty, let e = lastExit`
            (true, Some(e)) => {
                let retries = if u.retries > 0 { format!(" · 다시 한 횟수 {}", u.retries) } else { String::new() };
                w.add(&format!("<p>마지막 판정 종료 코드 {e}{retries}</p>\n"));
            }
            // 목록이 있거나(빈 데 없음), 빈 데 종료 코드도 없을 때 — 둘 다 목록 쪽을 그린다
            _ => {
                w.add("<ul>");
                for c in &u.checks {
                    let exit = match c.exit {
                        // Swift `c.exit.map { $0 == 0 ? "통과(종료 코드 0)" : "종료 코드 \($0)" } ?? "끝나지 않음"`
                        Some(0) => "통과(종료 코드 0)".to_string(),
                        Some(e) => format!("종료 코드 {e}"),
                        None => "끝나지 않음".to_string(),
                    };
                    let dur = if c.sec < 1.0 { "1초 안".to_string() } else { report_dur(c.sec) };
                    let mut s = format!("{}번째: {exit} · {dur}", c.n);
                    if c.timed_out {
                        s += " · 시간 상한에서 멈춤";
                    }
                    if c.interrupted {
                        s += " · 도중에 끊김";
                    }
                    w.add(&format!("<li>{s}</li>"));
                }
                w.add("</ul>\n");
                if let Some(last) = u.checks.last() {
                    // Swift `last.exit != 0` 은 Optional 비교라 종료 코드가 없을 때도 참이다
                    if last.exit != Some(0) && !last.tail.is_empty() {
                        w.add(&format!(
                            "<details><summary>마지막 판정 출력 끝부분</summary><pre>{}</pre></details>\n",
                            w.c(&plain(&last.tail))
                        ));
                    }
                }
            }
        }
    }
    if !u.denials.is_empty() {
        w.add(&format!(
            "<h3>허용 범위 밖이라 거부한 일</h3>\n<ul>{}</ul>\n",
            u.denials.iter().map(|d| format!("<li>{}</li>", w.t(d))).collect::<String>()
        ));
    }
    w.add("</section>\n");
}

/// 결과 기록의 목록 칸 하나 — 빈 목록은 아무것도 넣지 않는다. 원본: `list`(비공개 함수, 원본 497줄).
/// Swift 로는 `list` 인데 이 파일의 목록 읽기 도우미 `list` 와 이름이 겹쳐 Rust 에서는 `section_list` 로 둔다.
fn section_list(title: &str, items: &[String], w: &mut Writer) {
    if items.is_empty() {
        return;
    }
    let lis: String = items.iter().map(|it| format!("<li>{it}</li>")).collect();
    w.add(&format!("<h3>{title}</h3>\n<ul>{lis}</ul>\n"));
}

/// 흐름 그림 하나 — 읽히면 SVG 와 접힌 원문, 아니면 원문과 까닭. 원본: `flow`(비공개 함수, 원본 503줄).
/// Swift 의 `unit:` 인자는 원본 본문에서 쓰지 않는다(그대로 받기만 하는 `_unit`).
fn flow(f: &ResultFlow, _unit: i64, k: usize, w: &mut Writer) {
    let title = if f.title.is_empty() { format!("흐름 그림 {k}") } else { f.title.clone() };
    w.add(&format!("<figure>\n<figcaption>{}</figcaption>\n", w.t(&title)));
    match parse_sequence(&f.source) {
        SeqParse::Ok(d) => {
            w.add(&format!("<div class=\"pic\">{}</div>\n", sequence_svg(&d, &format!("흐름 그림: {title}"))));
            w.add(&format!("<details><summary>원문 보기</summary><pre>{}</pre></details>\n", w.t(&f.source)));
        }
        SeqParse::Unsupported { line, why } => {
            w.add(&format!(
                "<p class=\"empty\">그리지 못해 원문을 보입니다({line}번째 줄: {}).</p>\n<pre>{}</pre>\n",
                w.t(&why),
                w.t(&f.source)
            ));
        }
    }
    w.add("</figure>\n");
}

/// 단위가 돈 때 — 출발부터 끝(도는 중이면 그린 때)까지 가로 막대. 시작 시각이 있는 단위만. 원본: `timeline`(비공개 함수, 원본 517줄).
/// 지역 이름은 원본의 W·L·R·row·h 를 긴 이름으로 옮겼다: 너비·왼쪽 여백·오른쪽 여백·한 줄 높이·그림 높이.
fn timeline(r: &ReportRun, at: f64, w: &Writer) -> Option<String> {
    let us: Vec<&ReportUnit> = r.units.iter().filter(|u| u.started.is_some()).collect();
    // Iterator 의 min·max 는 f64(NaN 때문에 Ord 가 없다)에 못 쓰므로 inherent f64::min 로 같은 뜻을 낸다
    let t0 = r.started.or_else(|| us.iter().filter_map(|u| u.started).reduce(f64::min))?;
    let unit_end_max: Option<f64> = us.iter().filter_map(|u| u.ended).fold(None, |a: Option<f64>, v| Some(a.map_or(v, |m| m.max(v))));
    let t1 = r.ended.or(unit_end_max).unwrap_or(at).max(t0 + 1.0);
    if us.is_empty() || t1 <= t0 {
        return None;
    }
    let picture_width = 640.0;
    let left_margin = 70.0;
    let right_margin = 12.0;
    let row = 20.0;
    let picture_height = us.len() as f64 * row + 34.0;
    // 때 → 가로 자리(원본 지역 함수 `x`). 시작~끝 밖의 때는 가장자리에 붙인다
    let x_of = |t: f64| left_margin + ((t.clamp(t0, t1) - t0) / (t1 - t0) * (picture_width - left_margin - right_margin));
    // 수 → 글(원본 지역 함수 `n`. Swift `rounded()` 는 절반을 멀리로, Rust `round()` 도 같다)
    let num = |v: f64| -> String { format!("{}", v.round() as i64) };
    let mut s = format!(
        "<svg class=\"tl\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"단위가 돈 때\">",
        num(picture_width),
        num(picture_height),
        num(picture_width),
        num(picture_height)
    );
    for (k, u) in us.iter().enumerate() {
        let y = 6.0 + k as f64 * row;
        // `us` 는 시작 시각이 있는 단위만 골랐다(원본의 `?? t0` 가 있어도 여기서는 쓰지 않는다)
        let a = x_of(u.started.unwrap_or(t0));
        let b_end = match u.ended {
            Some(e) => e,
            None => {
                if u.status == "running" {
                    at
                } else {
                    u.started.unwrap_or(t0)
                }
            }
        };
        let b = x_of(b_end).max(a + 3.0);
        s += &format!("<text class=\"lb\" x=\"0\" y=\"{}\">단위 {}</text>", num(y + 11.0), u.n);
        s += &format!(
            "<rect class=\"bar {}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"12\" rx=\"3\"/>",
            unit_class(&u.status).unwrap_or(""),
            num(a),
            num(y + 2.0),
            num(b - a)
        );
    }
    let axis_y = us.len() as f64 * row + 10.0;
    s += &format!(
        "<line class=\"ax\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
        num(left_margin),
        num(axis_y),
        num(picture_width - right_margin),
        num(axis_y)
    );
    s += &format!("<text class=\"lb\" x=\"{}\" y=\"{}\">{}</text>", num(left_margin), num(axis_y + 16.0), w.date(Some(t0), "HH:mm"));
    s += &format!(
        "<text class=\"lb\" x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>",
        num(picture_width - right_margin),
        num(axis_y + 16.0),
        w.date(Some(t1), "HH:mm")
    );
    Some(s + "</svg>")
}

// ── 유스케이스: 보고서 그리기 (원본 540~565줄, 쓸 곳 포트는 Ports.swift) ──

/// 그린 결과. 원본: `APReportDraw`(`Equatable, Sendable` — 파생이 같은 뜻이다)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportDraw {
    pub written: Vec<ReportFile>,
    pub errors: Vec<String>,
}

/// 보고서 파일 — 실행 보고서(`runs/<실행 id>/report.html`)와 단위 보고서(`runs/<실행 id>/units/<n>/report.html`). 원본: `APReportFile`(`Ports.swift`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReportFile {
    Run,
    Unit(i64),
}

/// 보고서 쓸 곳 — 되면 `None`, 아니면 까닭. 링크를 따라가지 않게 쓰는 것은 어댑터 몫(엔진: 엔진 전용 폴더에 쓰고 rename).
/// 원본: `APReportSink`(`Ports.swift`, `func write(_ file: APReportFile, _ html: String) -> String?`)
pub trait ReportSink {
    fn write(&self, file: ReportFile, html: &str) -> Option<String>;
}

/// 기록을 읽어 실행 보고서와 고른 단위의 보고서를 쓴다. run.json 이 없으면 아무것도 쓰지 않는다.
/// 엔진이 단위 판정 뒤(그 단위와 실행)·건너뛴 뒤·끝 정리 때 부르고, 엔진 report 하위 명령이 끝난 실행을 다시 그릴 때 부른다.
///
/// 원본: `apDrawReports`. Swift `clock: APClock` 의 `clock.now.timeIntervalSince1970` 은 오늘의 시각(`ReadClock`)을 `clock.now()` 로 건넨다.
pub fn draw_reports(id: &str, source: &dyn ReportSource, sink: &dyn ReportSink, units: &[i64], clock: &dyn ReadClock, tz: &FixedZone, home: Option<&str>) -> ReportDraw {
    let mut d = ReportDraw::default();
    let Some(r) = report_run(id, source) else {
        d.errors.push("실행 기록(run.json)을 읽지 못해 보고서를 그리지 않았습니다".to_string());
        return d;
    };
    let at = clock.now();
    for n in units {
        // `r.units.contains(where: { $0.n == n })`
        if !r.units.iter().any(|u| u.n == *n) {
            d.errors.push(format!("대기열에 없는 단위({n})라 그 단위 보고서를 그리지 않았습니다"));
            continue;
        }
        match sink.write(ReportFile::Unit(*n), &report_html(&r, Some(*n), at, tz, home)) {
            Some(e) => d.errors.push(format!("단위 {n} 보고서: {e}")),
            None => d.written.push(ReportFile::Unit(*n)),
        }
    }
    match sink.write(ReportFile::Run, &report_html(&r, None, at, tz, home)) {
        Some(e) => d.errors.push(format!("실행 보고서: {e}")),
        None => d.written.push(ReportFile::Run),
    }
    d
}

// 빠진 단위 보고서(`apReportEndedStatuses`·`apMissingUnitReports`, 원본 565~568줄)는 이미 `unit_commits.rs`(임시 조각 3)에 있어 이 파일에서 다시 내놓는다.
pub use crate::rules::unit_commits::{missing_unit_reports, REPORT_ENDED_STATUSES};
