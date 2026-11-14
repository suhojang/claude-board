//! 자율주행 규칙(AutopilotRules) — 알림 webhook 의 상태 알림·3시간 요약과 끝 알림 흐름(4단계 ⑩-3, 2026-10-30, 설계 5장 6절 "알림").
//! 원본: `Sources/AutopilotRules/Notify.swift`(326줄, 전체 이식). 보내는 쪽은 엔진이다(보드 창이 꺼져 있어도): 상태 알림은 그 실행의 엔진이
//! 상태가 바뀔 때, 3시간 요약은 도는 실행의 엔진이 뿌리의 요약 기록(잠금)으로 하나만 보낸다. 정한 때에만: 출발(처음 도는 때), 대기에 들어감,
//! 끝(실행마다 한 번), 3시간 요약. 글 꼴은 받는 쪽(webhook 연결)마다: 일반 JSON·Slack 꼴·Google Chat 꼴. 바깥·모델이 쓴 글은 비밀 가리기 →
//! 다듬기 → 멘션 무력화(@ → 전각 ＠) → 받는 쪽 꼴의 이스케이프 → 길이 상한을 거친다.
//! webhook 주소는 비밀(키체인)이라 이 파일의 값·기록·글 어디에도 넣지 않는다 — 보낸 결과는 연결 id·이름으로만 남긴다.
//!
//! 이식 선언(원본 순서): `NOTIFY_LINE_MAX`·`NOTIFY_TEXT_MAX`·`NOTIFY_LINES_MAX`(apNotifyLineMax·TextMax·LinesMax) ·
//! `notify_clean`(+`_n`)(apNotifyClean) · `notify_escape`(apNotifyEscape) · `NoticeKind`(APNoticeKind) · `Notice`(APNotice) ·
//! `notice_text`(apNoticeText) · `notify_body`(apNotifyBody) · `NoticeRun`(APNoticeRun) · `notice_run_facts`(apNoticeRunFacts) ·
//! 비공개 `result`·`facts_fields`·`waiting_line` · `start_notice`(apStartNotice) · `wait_notice`(apWaitNotice) · `end_notice`(apEndNotice) ·
//! `MergeWait`(APMergeWait) · 비공개 `clock`·`expiry_at`·`days_from_civil` · `summary_notice`(apSummaryNotice) · `test_notice`(apTestNotice) ·
//! `expiring_lines`(apExpiringLines) · `SUMMARY_EVERY`(apSummaryEvery) · `state_notice`(apStateNotice) · `end_webhook_due`(apEndWebhookDue) ·
//! `summary_due`(apSummaryDue) · `notify_targets`(apNotifyTargets) · `NotifySent`(APNotifySent) · `notify_fail_note`(apNotifyFailNote) ·
//! 포트 `NoticeClock`(APClock)·`NoticeWaiter`(APWaiter) · `send_notice`(+`_budget`, apSendNotice) · `EndOpen`(APEndOpen)·`end_open`(apEndOpen) ·
//! `OutboxState`·`OutboxItem`·`outbox_waiting`(apOutboxWaiting) · `end_notice_due`(apEndNoticeDue) · `merge_reminder_due`(apMergeReminderDue) ·
//! `run_merge_open`(apRunMergeOpen) · `PublishMode`·`Publish`·`publish_parse`·`publish_json`(apPublishParse·apPublishJSON). 남은 선언 없음.
//!
//! 차이(원본 대비, 이 포트의 자리):
//! - 시간대: Foundation `TimeZone` 대신 `rules::report::FixedZone`(Asia/Seoul·UTC 만, 서머타임 시간대는 못 쓴다). `clock` 은 `Writer::date` 로
//!   "M월 d일 HH:mm" 을 그린다.
//! - 가리기: 원본과 같이 Meta.swift `apRedact`(열아홉 꼴) 전부를 쓴다 — 이식된 규칙 `meta::redact` 를 `use` 로 가져온다(R-Redact-b).
//! - 재시도 기다림: 원본은 Outbox.swift `apRetryWait`를 부른다. outbox 포트도 이식 전이라 같은 비공개 `retry_wait`·`http_date` 를 둔다.
//! - 게시 설정: 원본 `APPublish` 는 Outbox.swift 에 있으나 이 파일이 쓰는 칸(mode·notify)만 `Publish` 로 최소 이식했다(`publish_parse`·`publish_json`
//!   은 mode·notify 만 본다). Outbox 이식 뒤에는 `use` 로 바꾼다.
//! - 검수 대기 글: `OutboxState`(아홉 종류 가운데 여섯)·`OutboxItem`·`outbox_waiting` 을 최소 이식했다.
//! - `NoticeClock`·`NoticeWaiter` 포트는 `ports.rs` 가 아직 통합 전이라 여기 둔다(가짜는 `RefCell`). 이후 `use` 로 바꾼다.

use crate::rules::branches::{merge_open, Obj, Step};
use crate::rules::connections::{
    ap_day, connection_id_ok, webhook_check, Auth, ConnHttp, ConnSecrets, Connection, Expiry,
    HttpRequest, HttpResponse, NotifyFormat, Service, SiteCheck, EXPIRY_SOON_DAYS,
};
use crate::rules::control::run_ended;
use crate::rules::formats::{doc_text_n, grapheme_len, DOC_MARKER};
use crate::rules::meta::redact;
use crate::rules::report::{FixedZone, ReportRun, Writer};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

// MARK: - 받는 쪽 꼴

/// 한 줄 상한. 원본 `apNotifyLineMax`
pub const NOTIFY_LINE_MAX: usize = 200;
/// 글 전체 상한. 원본 `apNotifyTextMax`
pub const NOTIFY_TEXT_MAX: usize = 1800;
/// 줄 수 상한(머리 아래). 원본 `apNotifyLinesMax`
pub const NOTIFY_LINES_MAX: usize = 12;

// MARK: - 다듬기와 이스케이프

/// 바깥·모델 글 다듬기(기본 상한) — 가리기 → 다듬기 → 멘션 무력화. 꼴의 이스케이프는 글을 만들 때(`notify_escape`)다
pub fn notify_clean(s: &str) -> String {
    notify_clean_n(s, NOTIFY_LINE_MAX)
}

/// 바깥·모델 글 다듬기 — 가리기 → 다듬기 → 멘션 무력화. 꼴의 이스케이프는 글을 만들 때(`notify_escape`)다
pub fn notify_clean_n(s: &str, n: usize) -> String {
    let red = redact(s).0;
    // 너비 없는 글자(멘션을 쪼개 숨기는 데 쓰임)도 지운다. @ 를 전각으로 바꾸면 @channel·@here·@everyone·@all 이 멘션으로 읽히지 않는다
    let z: String = red
        .chars()
        .filter(|c| !matches!(u32::from(*c), 0x200B | 0x200C | 0x200D | 0x2060))
        .collect();
    doc_text_n(&z, n, true).replace('@', "＠")
}

/// 받는 쪽 꼴의 이스케이프 — Slack 은 공식 엔티티(<!here>·<@U…>·<#C…> 같은 꼴을 죽인다), Google Chat·일반 JSON 은 꺾쇠를 전각으로(<users/all> 등)
pub fn notify_escape(s: &str, f: NotifyFormat) -> String {
    match f {
        NotifyFormat::Slack => s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"),
        NotifyFormat::Gchat | NotifyFormat::Json => s.replace('<', "＜").replace('>', "＞"),
    }
}

// MARK: - 알림 하나

/// 알림 종류. 원본 `APNoticeKind`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoticeKind {
    /// 출발
    Start,
    /// 대기에 들어감
    Wait,
    /// 끝
    End,
    /// 3시간 요약
    Summary,
    /// 시험 글(기본값)
    #[default]
    Test,
}

impl NoticeKind {
    /// 원본의 원시 값(소문자)
    pub fn raw(self) -> &'static str {
        match self {
            NoticeKind::Start => "start",
            NoticeKind::Wait => "wait",
            NoticeKind::End => "end",
            NoticeKind::Summary => "summary",
            NoticeKind::Test => "test",
        }
    }
}

/// 알림 하나. 원본 `APNotice`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notice {
    pub kind: NoticeKind,
    /// 다시 보내지 않으려는 이름(start·end·wait-<k>·summary-<k>·test)
    pub id: String,
    /// 표지 다음 줄(다듬은 글)
    pub headline: String,
    /// 그 아래 줄(다듬은 글)
    pub lines: Vec<String>,
    /// 실행 id(요약·시험은 빈 글자)
    pub run: String,
    /// 일반 JSON 꼴에 더 넣는 칸(다듬은 글)
    pub fields: BTreeMap<String, String>,
}

impl Notice {
    /// 원본의 겸용 생성자 — 비친 칸은 빈 값(`Notice::default()`)으로 채워 넘긴다
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: NoticeKind,
        id: &str,
        headline: &str,
        lines: Vec<String>,
        run: &str,
        fields: BTreeMap<String, String>,
    ) -> Notice {
        Notice { kind, id: id.to_string(), headline: headline.to_string(), lines, run: run.to_string(), fields }
    }
}

/// 글 — 첫 줄 표지(오토파일럿 작성), 머리, 줄마다 "- ". 전체 상한에서 줄 단위로 자른다. 원본 `apNoticeText`
pub fn notice_text(n: &Notice) -> String {
    let mut out = format!("{DOC_MARKER}\n{}", n.headline);
    for l in n.lines.iter().take(NOTIFY_LINES_MAX) {
        let add = format!("\n- {l}");
        // 글자 수는 겹내기(그래프) 단위다(Swift `String.count`)
        if grapheme_len(&out) + grapheme_len(&add) > NOTIFY_TEXT_MAX {
            out.push_str("\n- …");
            break;
        }
        out.push_str(&add);
    }
    out
}

/// 꼬리표 꼴의 JSON — 오름차순 키. 원본은 `JSONSerialization(.sortedKeys, .withoutEscapingSlashes)` 다
fn json_data(o: &BTreeMap<String, Value>) -> Vec<u8> {
    // 글자·참거짓만 넣은 꼬리표 꼴이라 빠지지 않는다
    serde_json::to_vec(o).expect("알림 본문 JSON 만들기")
}

/// 글자 꼬리표 꼴 — 일반 JSON 본문의 기본 칸. 원본은 `[String: Any]` 다
fn json_text(o: &mut BTreeMap<String, Value>, k: &str, v: &str) {
    o.insert(k.to_string(), Value::from(v.to_string()));
}

/// 보낼 본문 — 받는 쪽 꼴마다. 원본 `apNotifyBody`
pub fn notify_body(n: &Notice, f: NotifyFormat) -> Vec<u8> {
    let text = notify_escape(&notice_text(n), f);
    let mut o: BTreeMap<String, Value> = BTreeMap::new();
    match f {
        NotifyFormat::Slack => {
            json_text(&mut o, "text", &text);
            o.insert("unfurl_links".to_string(), Value::from(false));
            o.insert("unfurl_media".to_string(), Value::from(false));
        }
        NotifyFormat::Gchat => {
            json_text(&mut o, "text", &text);
        }
        NotifyFormat::Json => {
            json_text(&mut o, "source", "claude-board-autopilot");
            json_text(&mut o, "marker", DOC_MARKER);
            json_text(&mut o, "event", n.kind.raw());
            json_text(&mut o, "id", &n.id);
            json_text(&mut o, "text", &text);
            json_text(&mut o, "title", &notify_escape(&n.headline, NotifyFormat::Json));
            let lines: Vec<Value> =
                n.lines.iter().take(NOTIFY_LINES_MAX).map(|l| Value::from(notify_escape(l, NotifyFormat::Json))).collect();
            o.insert("lines".to_string(), Value::from(lines));
            if !n.run.is_empty() {
                json_text(&mut o, "run", &n.run);
            }
            for (k, v) in &n.fields {
                if !o.contains_key(k) {
                    json_text(&mut o, k, &notify_escape(v, NotifyFormat::Json));
                }
            }
        }
    }
    json_data(&o)
}

// MARK: - 알림 글 만들기

/// 실행 하나의 알림 재료 — 보고서 값에서(엔진은 메모리의 run·queue 로 만든다). 원본 `APNoticeRun`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoticeRun {
    pub id: String,
    pub name: String,
    pub state: String,
    pub reason: String,
    pub model: String,
    pub total: i64,
    pub done: i64,
    pub failed: i64,
    pub capped: i64,
    pub skipped: i64,
    pub stopped: i64,
    pub finished: i64,
    /// 지금 도는 단위 제목
    pub current: String,
    pub started: Option<f64>,
}

/// 실행 하나의 알림 재료를 보고서 값에서 만든다. 원본 `apNoticeRunFacts`
pub fn notice_run_facts(r: &ReportRun) -> NoticeRun {
    let mut f = NoticeRun {
        id: r.id.clone(),
        name: notify_clean_n(if !r.project.is_empty() { &r.project } else if !r.title.is_empty() { &r.title } else { &r.id }, 80),
        state: r.state.clone(),
        reason: notify_clean_n(&r.reason, 160),
        model: notify_clean_n(&r.model, 40),
        total: r.units.len() as i64,
        ..NoticeRun::default()
    };
    let count = |s: &str| r.units.iter().filter(|u| u.status == s).count() as i64;
    f.done = count("done");
    f.failed = count("failed");
    f.capped = count("capped");
    f.skipped = count("skipped");
    f.stopped = count("stopped");
    f.finished = f.done + f.failed + f.capped + f.skipped + f.stopped;
    if let Some(u) = r.units.iter().find(|u| u.status == "running") {
        let raw = if u.title.is_empty() { format!("단위 {}", u.n) } else { u.title.clone() };
        f.current = notify_clean_n(&raw, 80);
    }
    f.started = r.started;
    f
}

/// 단위 결과 한 줄
fn result(f: &NoticeRun) -> String {
    let mut s = format!("단위 {}개 가운데 통과 {}", f.total, f.done);
    if f.failed > 0 {
        s += &format!(" · 미통과 {}", f.failed);
    }
    if f.capped > 0 {
        s += &format!(" · 상한 {}", f.capped);
    }
    if f.skipped > 0 {
        s += &format!(" · 건너뜀 {}", f.skipped);
    }
    if f.stopped > 0 {
        s += &format!(" · 중단 {}", f.stopped);
    }
    s
}

/// 일반 JSON 꼴의 실행 칸
fn facts_fields(f: &NoticeRun, waiting: i64) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("state".to_string(), f.state.clone()),
        ("name".to_string(), f.name.clone()),
        ("units_total".to_string(), f.total.to_string()),
        ("units_done".to_string(), f.done.to_string()),
        ("units_failed".to_string(), f.failed.to_string()),
        ("units_capped".to_string(), f.capped.to_string()),
        ("outbox_waiting".to_string(), waiting.to_string()),
    ])
}

/// 검수를 기다리는 올릴 글 줄 — 없으면 줄 자체를 넣지 않는다
fn waiting_line(n: i64) -> Option<String> {
    (n > 0).then(|| format!("검수를 기다리는 올릴 글 {n}개(보드의 '올릴 글'에서 봅니다)"))
}

/// 출발 알림. 원본 `apStartNotice`
pub fn start_notice(f: &NoticeRun, waiting: i64) -> Notice {
    let model = if f.model.is_empty() { String::new() } else { format!(" · 모델 {}", f.model) };
    let mut l = vec![format!("단위 {}개를 차례로 합니다{model}", f.total)];
    if let Some(w) = waiting_line(waiting) {
        l.push(w);
    }
    l.push(format!("실행 {}", f.id));
    Notice::new(NoticeKind::Start, "start", &format!("[자율주행] {} 출발했습니다", f.name), l, &f.id, facts_fields(f, waiting))
}

/// 대기 알림 — 단위를 시작한 뒤 첫 대기만 보낸다. 원본 `apWaitNotice`
pub fn wait_notice(f: &NoticeRun, waiting: i64, k: i64) -> Notice {
    let first = if f.reason.is_empty() { "새 단위를 시작하지 않고 기다립니다".to_string() } else { f.reason.clone() };
    let mut l = vec![first, format!("{} · 끝난 단위 {}", result(f), f.finished)];
    if let Some(w) = waiting_line(waiting) {
        l.push(w);
    }
    l.push(format!("실행 {}", f.id));
    Notice::new(NoticeKind::Wait, &format!("wait-{k}"), &format!("[자율주행] {} 쉬는 중", f.name), l, &f.id, facts_fields(f, waiting))
}

/// 끝 알림 — 실행마다 한 번. 원본 `apEndNotice`
pub fn end_notice(f: &NoticeRun, waiting: i64, merge_waiting: bool) -> Notice {
    let how = match f.state.as_str() {
        "done" => "끝났습니다",
        "error" => "멈췄습니다(엔진 오류)",
        _ => "멈췄습니다(사람이 중단)",
    };
    let mut l = vec![result(f)];
    if f.state != "done" && !f.reason.is_empty() {
        l.push(f.reason.clone());
    }
    if let Some(w) = waiting_line(waiting) {
        l.push(w);
    }
    if merge_waiting {
        l.push("합치기·승격을 기다립니다(보드에서 바뀐 줄을 보고 합칩니다)".to_string());
    }
    l.push(format!("실행 {}", f.id));
    Notice::new(NoticeKind::End, "end", &format!("[자율주행] {} {how}", f.name), l, &f.id, facts_fields(f, waiting))
}

/// 합치기를 기다리는 끝난 실행(요약 줄) — 이름과 끝난 지 며칠. 원본 `APMergeWait`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeWait {
    pub name: String,
    pub days: i64,
}

/// 때 글자 — "M월 d일 HH:mm". 원본은 Foundation `DateFormatter`(ko_KR)다
fn clock(ts: f64, tz: FixedZone) -> String {
    Writer::new(tz, None).date(Some(ts), "M월 d일 HH:mm")
}

/// 3시간 요약 — 도는 실행마다 한 줄, 기다리는 올릴 글 수, 합치기 기다림 n일, 토큰 만료 임박.
/// 도는 실행이 없으면 None(보내지 않음). 원본 `apSummaryNotice`
pub fn summary_notice(
    runs: &[NoticeRun],
    waiting: i64,
    merge: &[MergeWait],
    expiring: &[String],
    k: i64,
    at: f64,
    tz: FixedZone,
) -> Option<Notice> {
    if runs.is_empty() {
        return None;
    }
    let mut l: Vec<String> = Vec::new();
    for f in runs.iter().take(6) {
        let st = match f.state.as_str() {
            "running" => {
                if f.current.is_empty() { "도는 중".to_string() } else { format!("도는 중 · 지금 {}", f.current) }
            }
            "paused" => "일시정지".to_string(),
            "waiting_limit" => "사용량 한도로 쉬는 중".to_string(),
            "waiting_stopline" => "멈출 선에서 쉬는 중".to_string(),
            "waiting_precheck" => "시험 환경을 기다리는 중".to_string(),
            other => other.to_string(),
        };
        l.push(format!("{}: {} · 끝난 단위 {}/{}(통과 {})", f.name, st, f.finished, f.total, f.done));
    }
    if let Some(w) = waiting_line(waiting) {
        l.push(w);
    }
    for m in merge.iter().take(3) {
        l.push(format!("합치기 기다림: {} {}일", notify_clean_n(&m.name, 80), m.days));
    }
    for e in expiring.iter().take(3) {
        l.push(notify_clean_n(e, 120));
    }
    let mut fields = BTreeMap::new();
    fields.insert("runs".to_string(), runs.len().to_string());
    fields.insert("outbox_waiting".to_string(), waiting.to_string());
    fields.insert("merge_waiting".to_string(), merge.len().to_string());
    Some(Notice::new(
        NoticeKind::Summary,
        &format!("summary-{k}"),
        &format!("[자율주행] 3시간 요약 · {}", clock(at, tz)),
        l,
        "",
        fields,
    ))
}

/// 시험 글 — 설정 화면의 '시험 글 보내기'가 부르는 알림. 원본 `apTestNotice`
pub fn test_notice(name: &str, at: f64, tz: FixedZone) -> Notice {
    let lines = vec![
        format!("{} 에 보드의 알림이 닿는지 보는 시험 글입니다(설정 › 연결에서 보냄)", notify_clean_n(name, 80)),
        format!("보낸 때 {}", clock(at, tz)),
    ];
    Notice::new(NoticeKind::Test, "test", "[자율주행] 시험 글", lines, "", BTreeMap::new())
}

/// 1970-01-01 부터 센 날 수(Howard Hinnant 의 days_from_civil) — `connections.rs` 와 같은 것의 세 번째 부본
/// (세 파일이 각각 쓴다: report.rs·connections.rs·이곳)
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 만료일까지 남은 날(fixed 시간대의 오늘로 센다) — `connections::ap_expiry` 와 같은 판정이지만 시간대 꼴만 다르다
/// (`stopline::Tz` 는 UTC·이 머신 시간대 둘뿐이라 Asia/Seoul 을 못 쓴다). 당일은 `Soon(0)`
fn expiry_at(expires: &str, now: f64, tz: FixedZone) -> Expiry {
    let Some(e) = ap_day(expires) else { return Expiry::None };
    let c = tz.civil(now);
    let days =
        days_from_civil(i64::from(e.year), i64::from(e.month), i64::from(e.day)) - days_from_civil(c.year, i64::from(c.month), i64::from(c.day));
    if days < 0 {
        return Expiry::Expired(-days);
    }
    if days <= EXPIRY_SOON_DAYS {
        return Expiry::Soon(days);
    }
    Expiry::Ok(days)
}

/// 토큰 만료 임박 줄(요약) — 쓰기·읽기 연결의 만료일이 14일 안이거나 지났을 때. webhook 은 토큰이 없다. 원본 `apExpiringLines`
pub fn expiring_lines(connections: &[Connection], now: f64, tz: FixedZone) -> Vec<String> {
    connections
        .iter()
        .filter(|c| c.service != Service::Webhook)
        .filter_map(|c| match expiry_at(&c.expires, now, tz) {
            Expiry::Soon(d) => Some(format!(
                "{} 토큰 만료 임박: {}",
                c.service.label(),
                if d == 0 { "오늘 만료".to_string() } else { format!("{d}일 남음") }
            )),
            Expiry::Expired(d) => Some(format!("{} 토큰 만료됨: {d}일 지남(바깥 글은 쌓이고 일은 계속합니다)", c.service.label())),
            _ => None,
        })
        .collect()
}

// MARK: - 정한 때

/// 3시간 요약 간격(초). 원본 `apSummaryEvery`
pub const SUMMARY_EVERY: f64 = 3.0 * 3600.0;

/// 상태가 바뀔 때 보낼 알림 — 출발(처음 도는 때), 대기에 들어감(armed: 단위를 시작한 뒤 아직 대기 알림을 안 보냄).
/// 끝은 `end_notice_due` 가 본다. 원본 `apStateNotice`
pub fn state_notice(old: &str, new: &str, armed: bool, sent: &[String]) -> Option<NoticeKind> {
    let was = |k: &str| sent.iter().any(|s| s == k);
    if new == "running" && (old == "starting" || old.is_empty()) && !was("start") {
        return Some(NoticeKind::Start);
    }
    if new.starts_with("waiting_") && !old.starts_with("waiting_") && armed {
        return Some(NoticeKind::Wait);
    }
    None
}

/// 끝 알림(webhook) — 끝난 상태이고 아직 보내지 않았을 때 한 번. 원본 `apEndWebhookDue`
pub fn end_webhook_due(state: &str, sent: &[String]) -> bool {
    run_ended(state) && !sent.iter().any(|s| s == "end")
}

/// 3시간 요약을 보낼 때인가 — since 는 지금 도는 실행 가운데 가장 이른 출발(없으면 보내지 않음), last 는 마지막 요약.
/// 첫 요약은 출발 3시간 뒤, 그 뒤로는 3시간마다(앞 요약이 지금 도는 실행들보다 앞이면 출발부터 다시 센다). 원본 `apSummaryDue`
pub fn summary_due(last: Option<f64>, since: Option<f64>, now: f64) -> bool {
    let Some(since) = since else { return false };
    now - since.max(last.unwrap_or(0.0)) >= SUMMARY_EVERY
}

/// 받을 webhook — 게시 설정의 notify 가운데 알림 범위가 있는 webhook 연결만(없는 id 는 빼고). 원본 `apNotifyTargets`
pub fn notify_targets(ids: Option<&[String]>, connections: &[Connection]) -> Vec<Connection> {
    ids.unwrap_or(&[])
        .iter()
        .filter_map(|id| {
            connections
                .iter()
                .find(|c| c.id == *id && c.service == Service::Webhook && c.scopes.iter().any(|s| s == "notify"))
                .cloned()
        })
        .collect()
}

// MARK: - 보내기 유스케이스

/// webhook 하나에 보낸 결과. 원본 `APNotifySent`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotifySent {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub code: i32,
    pub note: String,
    pub requests: i64,
}

/// 지금의 때 — 시계 포트. 원본 `APClock`(`ports.rs` 통합 전까지 이 파일에 둔다)
pub trait NoticeClock {
    /// 유닉스 때(초)
    fn now(&self) -> f64;
}

/// 잠깐 기다림 — 재시도 앞의 기다림 포트. 원본 `APWaiter`
pub trait NoticeWaiter {
    fn wait(&self, seconds: f64);
}

/// 실패 까닭 — 상태 번호로만(받는 쪽 본문·주소는 남기지 않는다). 원본 `apNotifyFailNote`
pub fn notify_fail_note(r: &HttpResponse) -> String {
    if let Some(e) = &r.error {
        return e.clone();
    }
    match r.status {
        300..=399 => format!("다른 주소로 보내려 해 따라가지 않았습니다({})", r.status),
        400 => "받는 쪽이 글 꼴을 받지 않았습니다(400): 연결의 글 꼴(일반 JSON·Slack·Google Chat)을 확인합니다".to_string(),
        401 | 403 => format!("거부됨({}): webhook 주소가 취소됐거나 권한이 없습니다", r.status),
        404 => "받는 쪽에 webhook 이 없습니다(404)".to_string(),
        429 => "요청이 많다며 거절했습니다(429)".to_string(),
        500..=599 => format!("받는 쪽 오류({})", r.status),
        other => format!("받는 쪽이 {other} 로 답했습니다"),
    }
}

// MARK: - 재시도 기다림(Outbox.swift 의 `apRetryWait` 를 최소 이식했다)

/// 한 번 보내기에서 429·503 뒤 다시 보내는 횟수. 원본 `apRetryMax`
const RETRY_MAX: i32 = 3;
/// 한 번 기다리는 초 상한. 원본 `apRetryWaitMax`
const RETRY_WAIT_MAX: f64 = 20.0;

/// 429·503 에 응답 주인이 알려 준 기다림(초) — retry-after 를 재며, 숫자가 아니면 HTTP 때 글자로 본다. 기다림은 1 초~상한 안으로 벌린다
fn retry_wait(status: i32, retry_after: Option<&str>, attempt: i32, now: f64) -> Option<f64> {
    if !(status == 429 || status == 503) || attempt >= RETRY_MAX {
        return None;
    }
    let mut w = 2f64.powi(attempt);
    if let Some(h) = retry_after.map(str::trim).filter(|h| !h.is_empty()) {
        if let Some(sec) = h.parse::<f64>().ok().filter(|sec| sec.is_finite()) {
            w = sec;
        } else if let Some(at) = http_date(h) {
            w = at - now;
        }
    }
    Some(w.clamp(1.0, RETRY_WAIT_MAX))
}

/// HTTP 때 글자(EEE, dd MMM yyyy HH:mm:ss zzz)를 유닉스 때로. 몫은 GMT·UTC 만 — 원본 DateFormatter(en_US_POSIX·GMT) 의 보증 범위다
fn http_date(s: &str) -> Option<f64> {
    const WEEKS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let mut it = s.split_whitespace();
    let day_name = it.next()?.trim_end_matches(',');
    let day = it.next()?.parse::<i64>().ok()?;
    let mon = it.next()?;
    let year = it.next()?.parse::<i64>().ok()?;
    let time = it.next()?;
    let zone = it.next()?;
    if !WEEKS.iter().any(|w| day_name.eq_ignore_ascii_case(w))
        || !MONTHS.iter().any(|m| mon.eq_ignore_ascii_case(m))
        || !(zone.eq_ignore_ascii_case("gmt") || zone.eq_ignore_ascii_case("utc"))
    {
        return None;
    }
    let mut tm = time.split(':');
    let hour = tm.next()?.parse::<i64>().ok()?;
    let minute = tm.next()?.parse::<i64>().ok()?;
    let second = tm.next()?.parse::<i64>().ok()?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    let month = MONTHS.iter().position(|m| mon.eq_ignore_ascii_case(m))?;
    let epoch = days_from_civil(year, month as i64 + 1, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(epoch as f64)
}

// MARK: - 보내기

/// 알림 하나를 webhook 마다 보낸다(429·503 은 ⑩-1 과 같은 기다림, 모두 합쳐 budget 초 안).
/// 주소는 보내기 직전에 키체인에서 읽고 규칙을 다시 본다. 원본 `apSendNotice`
#[allow(clippy::too_many_arguments)]
pub fn send_notice_budget(
    n: &Notice,
    targets: &[Connection],
    dev: bool,
    secrets: &dyn ConnSecrets,
    http: &dyn ConnHttp,
    clock: &dyn NoticeClock,
    waiter: &dyn NoticeWaiter,
    budget: f64,
) -> Vec<NotifySent> {
    let start = clock.now();
    let mut out: Vec<NotifySent> = Vec::new();
    for c in targets {
        let mut s = NotifySent {
            id: c.id.clone(),
            name: if c.name.is_empty() { c.id.clone() } else { c.name.clone() },
            ..NotifySent::default()
        };
        let Some(url) = secrets.read(&c.id).filter(|u| !u.is_empty()) else {
            s.note = "webhook 주소가 키체인에 없어 보내지 않았습니다(요청 0)".to_string();
            out.push(s);
            continue;
        };
        if !matches!(webhook_check(&url, dev), SiteCheck::Ok(_)) {
            s.note = "webhook 주소가 규칙에 맞지 않아 보내지 않았습니다(요청 0)".to_string();
            out.push(s);
            continue;
        }
        if clock.now() >= start + budget {
            s.note = format!("시간 상한({}초)이 지나 보내지 않았습니다(요청 0)", budget as i64);
            out.push(s);
            continue;
        }
        let mut headers = BTreeMap::new();
        headers.insert("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
        let req = HttpRequest {
            method: "POST".to_string(),
            url: url.clone(),
            headers,
            body: Some(notify_body(n, c.format)),
            auth: Auth::None,
        };
        let mut attempt: i32 = 0;
        let resp = loop {
            s.requests += 1;
            let resp = http.send(&req, None);
            let Some(w) = retry_wait(resp.status, resp.headers.get("retry-after").map(String::as_str), attempt, clock.now()) else {
                break resp;
            };
            if clock.now() + w > start + budget {
                break resp;
            }
            waiter.wait(w);
            attempt += 1;
        };
        s.code = resp.status;
        if resp.error.is_none() && (200..=299).contains(&resp.status) {
            s.ok = true;
            s.note = "보냈습니다".to_string();
        } else {
            s.note = notify_fail_note(&resp);
        }
        out.push(s);
    }
    out
}

/// 기본 시간 상한(20초). 원본은 매개변수 기본값 `budget: Double = 20` 이다
pub fn send_notice(
    n: &Notice,
    targets: &[Connection],
    dev: bool,
    secrets: &dyn ConnSecrets,
    http: &dyn ConnHttp,
    clock: &dyn NoticeClock,
    waiter: &dyn NoticeWaiter,
) -> Vec<NotifySent> {
    send_notice_budget(n, targets, dev, secrets, http, clock, waiter, 20.0)
}

// MARK: - 끝 알림 흐름(보드)

/// 끝 알림을 누르면 무엇을 여나 — 기다리는 올릴 글이 있으면 검수(칸 위 실행 줄의 "합치기·승격 →"으로 잇는다), 없고 합치기가 남았으면 승격 화면,
/// 둘 다 없으면 지난 실행 상세(설계 5장 6절, 2026-10-22 사용자 결정과 설계 판단). 원본 `APEndOpen`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOpen {
    /// 검수 화면
    Review,
    /// 승격 화면
    Merge,
    /// 지난 실행 상세
    History,
}

/// 원본 `apEndOpen`
pub fn end_open(waiting: i64, merge_waiting: bool) -> EndOpen {
    if waiting > 0 {
        EndOpen::Review
    } else if merge_waiting {
        EndOpen::Merge
    } else {
        EndOpen::History
    }
}

/// 올릴 글의 상태 — 원본 `APOutboxState` 의 여섯(이 파일이 검수 화면 기준으로 쓰는 것. Outbox 이식 뒤에는 `use` 로 바꾼다)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutboxState {
    /// 검수 기다림
    #[default]
    Waiting,
    /// 올리는 중
    Posting,
    /// 올림
    Posted,
    /// 넘어감
    Skipped,
    /// 올리지 못함
    Failed,
    /// 올렸는지 모름(응답을 못 읽음)
    Uncertain,
}

/// 올릴 글 — 검수 화면이 쓰는 칸 가운데 이 파일이 보는 상태만(최소 이식). 원본 `APOutboxItem`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutboxItem {
    pub state: OutboxState,
}

/// 기다리는 올릴 글 — 검수 기다림·올리지 못함·확인 필요(보드 검수 화면의 "기다림"과 같은 기준). 원본 `apOutboxWaiting`
pub fn outbox_waiting(items: &[OutboxItem]) -> i64 {
    items
        .iter()
        .filter(|it| matches!(it.state, OutboxState::Waiting | OutboxState::Failed | OutboxState::Uncertain))
        .count() as i64
}

/// 끝 알림을 띄울 때인가 — 끝 정리를 마친 실행마다 한 번. 이 판의 보드가 처음 뜬 때(seeded) 앞에 끝난 실행은 띄우지 않는다
/// (처음 띄울 때 지난 실행 수십 개의 알림이 한꺼번에 뜨지 않게). 보드가 꺼져 있던 동안 끝난 실행은 다음에 뜰 때 한 번. 원본 `apEndNoticeDue`
pub fn end_notice_due(ended: Option<f64>, wrapped: bool, notified: bool, seeded: f64) -> bool {
    if !wrapped || notified {
        return false;
    }
    let Some(e) = ended else { return false };
    e >= seeded
}

/// "나중에" 뒤 하루 지나 다시 오는 합치기 알림 — 한 번만, 그때도 합치기가 남았을 때. 원본 `apMergeReminderDue`
pub fn merge_reminder_due(later: Option<f64>, reminded: bool, merge_waiting: bool, now: f64) -> bool {
    if reminded || !merge_waiting {
        return false;
    }
    let Some(l) = later else { return false };
    now >= l
}

/// 끝난 실행이 합치기·승격을 기다리는가(엔진의 3시간 요약 줄) — 보드 APRun.parse 와 같은 기준: 끝 정리를 마쳤고 가지 기록이 있는 실행에
/// `branches::merge_open`(승격 차례는 프로젝트 설정 promotion, Jira 실행은 스토리 없는 단위가 있을 때만 자율주행 가지도 합칠 쪽).
/// 원본 `apRunMergeOpen`
pub fn run_merge_open(run: &Obj, queue: &[Obj], promotion: &Value) -> bool {
    let empty = Map::new();
    let state = run.get("state").and_then(Value::as_str).unwrap_or("");
    if !run_ended(state) || !run.contains_key("wrapped") {
        return false;
    }
    let br = run.get("branches").and_then(Value::as_object).unwrap_or(&empty);
    let repos = br.get("repos").and_then(Value::as_object).unwrap_or(&empty);
    if repos.is_empty() {
        return false;
    }
    let steps: Vec<Step> = promotion
        .as_array()
        .filter(|list| list.iter().all(Value::is_object))
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    let src = s.as_object()?;
                    let from = src.get("from")?.as_str()?;
                    let into = src.get("into")?.as_str()?;
                    Some(Step {
                        from: from.to_string(),
                        into: into.to_string(),
                        after: src.get("after").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let jira = queue.iter().any(|q| q.contains_key("story"));
    let stories = if jira { queue.iter().any(|q| !q.contains_key("story")) } else { true };
    merge_open(br, &steps, stories)
}

// MARK: - 게시 설정(Outbox.swift 의 APPublish 를 최소 이식했다)

/// 게시 방식 — 새 프로젝트 기본은 검수 뒤 올림(사용자 결정). 원본 `APPublishMode`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PublishMode {
    /// 검수 뒤 올림
    #[default]
    Review,
    /// 곧바로 올림
    Auto,
}

impl PublishMode {
    fn raw(self) -> &'static str {
        match self {
            PublishMode::Review => "review",
            PublishMode::Auto => "auto",
        }
    }

    fn from_raw(s: &str) -> Option<PublishMode> {
        match s {
            "review" => Some(PublishMode::Review),
            "auto" => Some(PublishMode::Auto),
            _ => None,
        }
    }
}

/// 게시 설정 — 보드의 게시 설정 객체(`publish`) 가운데 이 판이 다루는 mode·notify 만
/// (jira_comments·report 와 그 검사는 Outbox 이식에 남아 있다: `apSpaceKeyPattern` 등이 아직 없다). 원본 `APPublish`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Publish {
    pub mode: PublishMode,
    pub notify: Option<Vec<String>>,
}

/// 게시 설정을 읽는다 — mode·notify 만. 잘못된 mode·notify 문제는 `apPublishParse` 와 같은 말로 적는다. 원본 `apPublishParse`
pub fn publish_parse(v: Option<&Value>) -> (Publish, Vec<String>) {
    let mut p = Publish::default();
    let mut probs: Vec<String> = Vec::new();
    let Some(v) = v.filter(|v| !v.is_null()) else { return (p, probs) };
    let Some(o) = v.as_object() else { return (p, vec!["publish 는 객체여야 합니다".to_string()]) };
    if let Some(m) = o.get("mode") {
        match m.as_str().and_then(PublishMode::from_raw) {
            Some(m) => p.mode = m,
            None => probs.push("publish.mode 는 review 또는 auto 입니다".to_string()),
        }
    }
    if let Some(n) = o.get("notify").filter(|n| !n.is_null()) {
        let ok = n.as_array().filter(|a| {
            a.len() <= 50 && a.iter().all(|x| x.as_str().is_some_and(|s| connection_id_ok(s) && s.starts_with("webhook-")))
        });
        match ok {
            Some(a) => {
                let mut seen = HashSet::new();
                p.notify = Some(
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .filter(|s| seen.insert(s.clone()))
                        .collect(),
                );
            }
            None => probs.push("publish.notify 는 알림 webhook 연결 id(webhook-n)의 배열입니다".to_string()),
        }
    }
    (p, probs)
}

/// 게시 설정을 JSON 으로 — mode·notify 만 내보낸다(`publish_parse` 의 최소 이식과 짝). 원본 `apPublishJSON`
pub fn publish_json(p: &Publish) -> Value {
    let mut d = Map::new();
    d.insert("mode".to_string(), Value::String(p.mode.raw().to_string()));
    if let Some(n) = &p.notify {
        d.insert("notify".to_string(), serde_json::to_value(n).expect("게시 notify JSON 만들기"));
    }
    Value::Object(d)
}
