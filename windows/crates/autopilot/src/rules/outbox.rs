//! 자율주행 올릴 글(outbox)과 게시 방식(4단계 ⑩-1, 2026-10-30, 설계 5장 2절 outbox/·5장 6절) — 원본 1~678줄 전체(청크 a + b).
//! 엔진이 단위 판정 뒤(Jira 댓글)·끝 정리 때(보고서)에 실행 폴더 outbox/<글 id>.json 으로 글을 만들고, 게시 방식이 "자동으로 올림"이면 바로 올린다.
//! "검수 뒤 올림"(기본)이면 글은 기다리고, 보드 검수 화면에서 사람이 누를 때만 올린다(보드는 앱 안 엔진의 outbox 명령을 부른다).
//! 여기에는 순수한 것만 둔다: 게시 설정 읽기, 어떤 글을 만들지, 글 본문(APDoc), 파일 꼴, 올릴지 판정, 요청 꼴과 응답 해석, 429 다시 보내기,
//! 그리고 포트(APOutboxStore·APHTTP·APSecrets·APClock·APWaiter)로만 바깥을 부르는 올리기 유스케이스 apPostItem.
//! 다시 눌러도 하나만 생기게: 요청을 보내기 전에 글을 "올리는 중"(posting)으로 쓰고, 단계마다 받은 곳 id 를 바로 적고, 끝나면 "올림"(posted).
//! 잠금 안에서 posting 을 보면 앞 올리기가 끝나지 못하고 죽은 것이라 "확인 필요"(uncertain)로 바꾸고, 그 글은 자동으로 다시 올리지 않는다
//! (판정·올리기와 그 시험도 파일 끝에 있다).
//!
//! 이식 선언(원본 순서): `PublishMode`(APPublishMode, notify_rules 최소 이식을 `use`) · `TargetService`(APTargetService, tickets `use`) ·
//! `OutboxTarget`(APOutboxTarget, tickets) · `Publish`(APPublish)·`makes_any` · `space_key_ok`·`confluence_id_ok`·`notion_id_ok`
//! (`apSpaceKeyPattern`·`apConfluenceIDPattern`·`apNotionIDPattern` 을 정규식 없이) · `publish_parse`(apPublishParse) ·
//! `publish_json`(apPublishJSON) · `target_connection`(tickets) · `item_label`·`target_label`(tickets) · `publish_notes`(apPublishNotes) ·
//! 글: `OutboxState`(APOutboxState, notify_rules) · `Attempt`(APAttempt)·`Remote`(APRemote)·`OutboxItem`(APOutboxItem, tickets) ·
//! `OUTBOX_FORMAT`·`OUTBOX_MAX_BYTES`·`outbox_id_ok`·`outbox_json`·`outbox_data`·`outbox_parse`(tickets, 원본 183–249줄) ·
//! `unit_status_label`(apUnitStatusLabel) · 비공개 `status_label`·`usd_text`·`day` · `report_doc`(apReportDoc) ·
//! `comment_doc`(apCommentDoc) · 비공개 `comment_states` · `unit_outbox`(apUnitOutbox) · `run_outbox`(apRunOutbox) ·
//! 판정·다시 보내기(356–435 줄, tickets.rs 가 옮긴 것을 `pub use` 로 내놓는다): `PostTrigger`(APPostTrigger)·`PostDecision`(APPostDecision)·
//! `post_decide`(apPostDecide) · `RETRY_MAX`·`RETRY_WAIT_MAX`(apRetryMax·apRetryWaitMax)·`retry_wait`(apRetryWait) ·
//! `transient`(apTransient)·`fail_note`(apFailNote) · 요청 꼴(437–496 줄, tickets.rs): `jira_comment_request`(apJiraCommentRequest)·
//! `confluence_page_request`(apConfluencePageRequest)·`confluence_attachment_request`(apConfluenceAttachmentRequest)·
//! `notion_page_request`(apNotionPageRequest)·`notion_append_request`(apNotionAppendRequest)·`created_id`(apCreatedID)·`remote_url`(apRemoteURL) ·
//! 응답 꼴 `PostResult`(APPostResult) · 포트 `OutboxStore`(APOutboxStore)·유스케이스 `post_item`(apPostItem) ·
//! 새로 옮긴 것(634–678 줄): `mark_item`(apMarkItem)·`create_items`(apCreateItems)·`outbox_state_label`(apOutboxStateLabel)·`outbox_counts`(apOutboxCounts).
//!
//! 차이(원본 대비, 이 포트의 자리):
//! - 임시 슬라이스 재사용: 원본 12–41·92–113·141–188줄의 값 꼴과 파일 꼴(`APTargetService`·`APOutboxTarget`·`APOutboxState`·`APAttempt`·
//!   `APRemote`·`APOutboxItem`·`apOutboxFormat`·`apOutboxMaxBytes`·`apOutboxIDOK`·`apOutboxJSON`·`apOutboxData`·`apOutboxParse`·
//!   `apTargetConnection`·`apItemLabel`·`apTargetLabel`)은 tickets.rs 가 이미 같은 원본에서 옮겼다 — 판정·올리기 356–632 줄도 그렇다.
//!   그 자리를 하나로 모은 것은 이 파일이다: tickets.rs 것을 `pub use` 로 내놓아 `rules::outbox` 가 이 원본의 공개 자리다(아래 슬라이스 참고).
//!   `APPublishMode`·`APOutboxState` 는 notify_rules.rs 의 최소 이식을 `use` 로 쓴다. 정규식 `apIssueKeyPattern` 은 tickets.rs 의 `issue_key_ok`.
//! - 정규식은 판별자로: `apSpaceKeyPattern`·`apConfluenceIDPattern`·`apNotionIDPattern` 세 꼴은 정규식 없이 비공개 `space_key_ok`·
//!   `confluence_id_ok`·`notion_id_ok` 로 옮겼다(lookaround 없어도 본문이 길어 정규식 크레이트를 더하지 않는다).
//! - Swift `trimmingCharacters(in: .whitespaces)`(Zs 범주와 탭)는 비공개 `trim_ws`, `prefix(40)·prefix(8)·prefix(10)` 은 grapheme 으로 센
//!   `take` 로 옮겼다(tickets.rs 의 것과 같은 근사).
//! - 시간대: Foundation `TimeZone` 대신 `rules::stopline::Tz`(Utc·Local). `day` 는 원본 `day(ts, "M월 d일 HH:mm", tz)` 하나만 쓰므로
//!   `stopline::when_text` 로 그린다(엔진 `apWhenText` 와 같은 꼴).
//! - `APPublish` 의 `makesAny` 는 메서드 `makes_any` 로.
//! - 포트 `APOutboxStore` 의 `ids()`(Ports.swift 121줄)는 tickets.rs 가 trait 에서 뺀 결정(이 규칙 크레이트의 쓰는 곳이 없다 — 부르는 곳은
//!   엔진 명령 어댑터 main.swift 쪽)을 잇는다: 시험의 MemOutbox 같은 저장소가 각자 필요하면 보통 메서드로 둔다.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use crate::rules::connections::{ap_expiry, connection_id_ok, service_base, Connection, Expiry};
use crate::rules::formats::{doc_clamp, doc_text_n, grapheme_len, Block, Doc, Span, DOC_MARKER};
use crate::rules::report::{report_dur, ReportRun, ReportUnit};
use crate::rules::stopline::{when_text, Tz};

// MARK: - 게시 설정(프로젝트 설정 publish 칸 → run.json publish → 고정 설정)

/// 슬라이스 — tickets.rs · notify_rules.rs 가 원본 Outbox.swift 12–249·356–632 줄을 이미 옮겼다(위 파일 머리의 차이 참고).
/// `pub use` 로 내놓아 이 원본의 공개 자리는 한 곳(rules::outbox)이다.
pub use crate::rules::notify_rules::{OutboxState, PublishMode};
use crate::rules::notify_rules::NoticeClock;
pub use crate::rules::tickets::{
    confluence_attachment_request, confluence_page_request, created_id, fail_note, issue_key_ok, item_label, jira_comment_request,
    notion_append_request, notion_page_request, outbox_data, outbox_id_ok, outbox_json, outbox_parse, post_decide, post_item,
    remote_url, retry_wait, target_connection, target_label, transient, Attempt, OutboxItem, OutboxTarget, PostDecision, PostResult,
    PostTrigger, Remote, TargetService, RETRY_MAX, RETRY_WAIT_MAX, OUTBOX_FORMAT, OUTBOX_MAX_BYTES, OutboxStore,
};

/// 게시 설정 — 프로젝트 설정 `publish` 칸 → run.json `publish` → 고정 설정. 원본: `Outbox.swift` 의 `APPublish`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Publish {
    /// 새 프로젝트 기본은 검수 뒤 올림(사용자 결정)
    pub mode: PublishMode,
    /// 보고서 받을 곳(confluence·notion), 없으면 보고서 글을 만들지 않음
    pub report: Option<OutboxTarget>,
    /// 단위 티켓에 결과 댓글
    pub jira_comments: bool,
    /// 알림 webhook(4단계 ⑩-3) — 이 프로젝트의 상태 알림·3시간 요약을 받을 webhook 연결 id.
    /// None 은 정한 적 없음(보드가 출발 확인에서 모든 webhook 으로 채움), 빈 배열은 끔
    pub notify: Option<Vec<String>>,
}

impl Publish {
    /// 받을 곳이 하나라도 있는가. 원본: `APPublish.makesAny`
    pub fn makes_any(&self) -> bool {
        self.report.is_some() || self.jira_comments
    }
}

/// 게시 방식의 raw 값 — notify_rules 의 `PublishMode` 는 raw 문자열을 밖으로 주지 않아 여기서 원본 값과 맞춘다.
/// (tickets.rs 의 비공개 것과 같다 — Outbox 전체 이식 뒤 `PublishMode` 자체로 옮긴다)
fn publish_mode_raw(m: &PublishMode) -> &'static str {
    match m {
        PublishMode::Review => "review",
        PublishMode::Auto => "auto",
    }
}

fn publish_mode_from_raw(s: &str) -> Option<PublishMode> {
    match s {
        "review" => Some(PublishMode::Review),
        "auto" => Some(PublishMode::Auto),
        _ => None,
    }
}

/// Swift `trimmingCharacters(in: .whitespaces)` — Zs 범주와 탭(tickets.rs 의 `trim_ws` 가 비공개라 같은 것을 둔다)
fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'))
}

/// Swift `String.prefix(n)` 근사 — grapheme 으로 센 `n` 개 덩어리까지의 접두어(tickets.rs 의 `take` 가 비공개라 같은 것을 둔다;
/// 짧은 까닭 문장만 자른다)
fn take(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    for (i, c) in s.char_indices() {
        if grapheme_len(&s[..i + c.len_utf8()]) == n + 1 {
            return s[..i].to_string();
        }
    }
    s.to_string()
}

/// Confluence 스페이스 키 꼴 — `^~?[A-Za-z0-9_-]{1,100}$` 를 정규식 없이 본 것. 원본: `apSpaceKeyPattern` · `apMatch`
fn space_key_ok(s: &str) -> bool {
    let t = s.strip_prefix('~').unwrap_or(s);
    !t.is_empty() && t.chars().count() <= 100 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Confluence 페이지 id 꼴 — `^[0-9]{1,20}$` 를 정규식 없이 본 것. 원본: `apConfluenceIDPattern`
fn confluence_id_ok(s: &str) -> bool {
    (1..=20).contains(&s.chars().count()) && s.chars().all(|c| c.is_ascii_digit())
}

/// Notion 페이지 id 꼴 — `^[0-9a-fA-F]{8}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{12}$` 를
/// 정규식 없이 본 것(5 덩이 가운데 각, 앞의 줄표 하나만 옵션). 원본: `apNotionIDPattern`
fn notion_id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let (mut at, mut grp) = (0usize, 0usize);
    for n in [8, 4, 4, 4, 12] {
        if grp > 0 && b.get(at) == Some(&b'-') {
            at += 1;
        }
        if at + n > b.len() || !b[at..at + n].iter().all(u8::is_ascii_hexdigit) {
            return false;
        }
        at += n;
        grp += 1;
    }
    at == b.len()
}

/// 게시 설정을 읽는다 — 없으면 기본(검수 뒤 올림, 받을 곳 없음). 꼴이 틀린 칸은 까닭을 남기고 그 칸은 쓰지 않는다. 원본: `apPublishParse`
pub fn publish_parse(v: Option<&Value>) -> (Publish, Vec<String>) {
    let mut p = Publish::default();
    let mut probs: Vec<String> = Vec::new();
    let Some(v) = v.filter(|v| !v.is_null()) else { return (p, probs) };
    let Some(o) = v.as_object() else { return (p, vec!["publish 는 객체여야 합니다".to_string()]) };
    if let Some(m) = o.get("mode") {
        match m.as_str().and_then(publish_mode_from_raw) {
            Some(m) => p.mode = m,
            None => probs.push("publish.mode 는 review 또는 auto 입니다".to_string()),
        }
    }
    if let Some(j) = o.get("jira_comments") {
        match j.as_bool() {
            Some(b) => p.jira_comments = b,
            None => probs.push("publish.jira_comments 는 true/false 입니다".to_string()),
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
    if let Some(r) = o.get("report").filter(|r| !r.is_null()) {
        let d = r.as_object().cloned().unwrap_or_default();
        let space = trim_ws(d.get("space").and_then(Value::as_str).unwrap_or(""));
        let parent = trim_ws(d.get("parent").and_then(Value::as_str).unwrap_or(""));
        match d.get("service").and_then(Value::as_str) {
            Some("confluence") => {
                if !space_key_ok(space) {
                    probs.push(format!("Confluence 스페이스 키 꼴이 아닙니다: {}", take(space, 40)));
                } else if !parent.is_empty() && !confluence_id_ok(parent) {
                    probs.push(format!("Confluence 상위 페이지 id 는 숫자입니다: {}", take(parent, 40)));
                } else {
                    p.report = Some(OutboxTarget::new(TargetService::Confluence, "", space, parent));
                }
            }
            Some("notion") => {
                if !notion_id_ok(parent) {
                    probs.push(format!("Notion 상위 페이지 id 꼴이 아닙니다(32자리 16진수): {}", take(parent, 40)));
                } else {
                    p.report = Some(OutboxTarget::new(TargetService::Notion, "", "", &parent.replace('-', "").to_lowercase()));
                }
            }
            _ => probs.push("publish.report.service 는 confluence 또는 notion 입니다".to_string()),
        }
    }
    (p, probs)
}

/// 게시 설정을 JSON 으로 — mode·jira_comments·notify·report 를 내보낸다(`publish_parse` 와 짝). 원본: `apPublishJSON`
pub fn publish_json(p: &Publish) -> Value {
    let mut d = Map::new();
    d.insert("mode".to_string(), json!(publish_mode_raw(&p.mode)));
    d.insert("jira_comments".to_string(), json!(p.jira_comments));
    if let Some(n) = &p.notify {
        d.insert("notify".to_string(), json!(n));
    }
    match &p.report {
        Some(r) => {
            let mut t = Map::new();
            t.insert("service".to_string(), json!(r.service.raw_value()));
            if !r.space.is_empty() {
                t.insert("space".to_string(), json!(r.space));
            }
            if !r.parent.is_empty() {
                t.insert("parent".to_string(), json!(r.parent));
            }
            d.insert("report".to_string(), Value::Object(t));
        }
        None => {
            d.insert("report".to_string(), Value::Null);
        }
    }
    Value::Object(d)
}

// MARK: - 출발 확인 알림

/// 출발 확인 "게시 방식" 줄의 알림 — 받을 곳이 있는데 연결·범위·토큰이 없거나 만료면 "글은 outbox 에 쌓입니다". 출발은 막지 않는다.
/// `secrets` 는 토큰이 있는 연결 id(엔진이 키체인에서 읽은 것의 이름만). 원본: `apPublishNotes`
pub fn publish_notes(p: &Publish, connections: &[Connection], secrets: &HashSet<String>, dev: bool, now: f64, tz: Tz) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut targets: Vec<TargetService> = Vec::new();
    if let Some(r) = &p.report {
        targets.push(r.service);
    }
    if p.jira_comments {
        targets.push(TargetService::Jira);
    }
    for s in targets {
        let need = target_connection(s);
        let name = match s {
            TargetService::Jira => "Jira 댓글",
            TargetService::Confluence => "Confluence 보고서",
            TargetService::Notion => "Notion 보고서",
        };
        let Some(c) = connections.iter().find(|c| c.id == need.0) else {
            out.push(format!(
                "{name}: {} 연결이 없습니다. 요청은 0건이고 글은 올리지 않은 채 쌓입니다(연결한 뒤 검수 화면에서 올림)",
                if need.0 == "notion" { "Notion" } else { "Atlassian" }
            ));
            continue;
        };
        if !c.scopes.iter().any(|x| x == need.1) {
            out.push(format!("{name}: 연결의 범위에 '{}' 가 없어 올리지 않습니다", need.1));
        }
        if !secrets.contains(&c.id) {
            out.push(format!("{name}: 키체인에 토큰이 없어 올리지 않습니다"));
        }
        if service_base(c, dev).is_none() {
            out.push(format!("{name}: 사이트 주소가 규칙에 맞지 않아 올리지 않습니다"));
        }
        match ap_expiry(&c.expires, now, tz) {
            Expiry::Expired(_) => out.push(format!("{name}: 토큰 만료일이 지났습니다. 글은 쌓이고 일은 계속합니다")),
            Expiry::Soon(d) => out.push(format!("{name}: 토큰 만료가 {d}일 남았습니다")),
            _ => {}
        }
    }
    out
}

// MARK: - 어떤 글을 만들지와 그 본문

/// 단위 상태의 이름표. 원본: `apUnitStatusLabel`
pub fn unit_status_label(s: &str) -> String {
    match s {
        "done" => "통과",
        "failed" => "미통과",
        "capped" => "상한",
        "skipped" => "건너뜀",
        "stopped" => "중단",
        "pending" => "대기",
        "running" => "진행 중",
        _ => s,
    }
    .to_string()
}

/// 원본 `usdText` — 달러 글(< 1 이면 소수 셋째 자리까지, 그 위는 둘째 자리까지)
fn usd_text(x: f64) -> String {
    if x < 1.0 {
        format!("{x:.3}")
    } else {
        format!("{x:.2}")
    }
}

/// 원본 `day(...)` — "M월 d일 HH:mm"(한국어, 엔진 `apWhenText` 와 같은 꼴). 유한하고 0 넘는 시각만, 아니면 빈 글
fn day(ts: Option<f64>, tz: Tz) -> String {
    let Some(t) = ts.filter(|t| t.is_finite() && *t > 0.0) else { return String::new() };
    when_text(t, tz)
}

/// 보고서 글 본문(Confluence·Notion) — 첫 줄 표지, Notion 이면 요청자, 요약·단위 목록·넘김, 첨부 안내, 글 번호.
/// `title` 은 원본 서명에 있으나 본문이 쓰지 않는다(제목은 받는 쪽 제목 칸으로). 원본: `apReportDoc`
pub fn report_doc(r: &ReportRun, notion: bool, attachment: bool, requester: &str, item_id: &str, title: &str) -> Doc {
    let _ = title;
    let mut b: Vec<Block> = vec![Block::Paragraph(vec![Span::Text(DOC_MARKER.to_string(), true)])];
    if notion {
        let who = doc_text_n(requester, 80, true);
        b.push(Block::Paragraph(vec![Span::Text(
            if who.is_empty() {
                "요청자: 연결에 이름이 없습니다(Notion 작성자 칸은 통합 이름입니다)".to_string()
            } else {
                format!("요청자: {who}")
            },
            false,
        )]));
    }
    let k = |s: &str| r.units.iter().filter(|u| u.status == s).count();
    let mut line = format!("대기열 {}개 가운데 {}개 통과", r.units.len(), k("done"));
    if k("failed") > 0 {
        line += &format!(", {}개 미통과", k("failed"));
    }
    if k("capped") > 0 {
        line += &format!(", 상한 {}", k("capped"));
    }
    if k("skipped") > 0 {
        line += &format!(", 건너뜀 {}", k("skipped"));
    }
    if k("stopped") > 0 {
        line += &format!(", 중단 {}", k("stopped"));
    }
    let sec = r.units.iter().map(|u| u.active_sec).sum::<f64>();
    let cost = r.units.iter().map(|u| u.cost).sum::<f64>();
    line += &format!(". 일한 시간 {}, {} USD.", report_dur(sec), usd_text(cost));
    b.push(Block::Paragraph(vec![Span::Text(line, false)]));
    let mut who = format!("실행 {}", r.id);
    if !r.project.is_empty() {
        who += &format!(" · 프로젝트 {}", r.project);
    }
    if !r.model.is_empty() {
        who += &format!(" · 모델 {}", r.model);
    }
    if !r.epic.is_empty() {
        who += &format!(" · 에픽 {}", r.epic);
    }
    b.push(Block::Paragraph(vec![Span::Text(who, false)]));
    if !r.units.is_empty() {
        let rows = r
            .units
            .iter()
            .map(|u| {
                let mut s = vec![
                    Span::Text(format!("{}: ", if u.title.is_empty() { format!("단위 {}", u.n) } else { u.title.clone() }), false),
                    Span::Text(unit_status_label(&u.status), true),
                ];
                if !u.task.is_empty() {
                    s.push(Span::Text(format!(" · {}", u.task), false));
                }
                s
            })
            .collect();
        b.push(Block::Bullets(rows));
    }
    let parked = r.units.iter().map(|u| u.result.as_ref().map(|res| res.next_start.len()).unwrap_or(0)).sum::<usize>();
    let found = r.units.iter().map(|u| u.result.as_ref().map(|res| res.new_work.len()).unwrap_or(0)).sum::<usize>();
    let harness = match r.harness_same {
        Some(true) => "하네스 출발 때와 같음",
        Some(false) => "하네스 출발 때와 다름",
        None => "하네스 확인 기록 없음",
    };
    b.push(Block::Paragraph(vec![Span::Text(
        format!("미룬 사람 몫 {parked} · 새로 찾은 일 {found} · {harness}"),
        false,
    )]));
    b.push(Block::Paragraph(vec![Span::Text(
        if attachment {
            "흐름 그림과 단위별 기록은 첨부한 보고서(report.html)에 있습니다.".to_string()
        } else {
            "흐름 그림과 단위별 기록은 보드의 실행 보고서에 있습니다(이 글에는 첨부하지 않았습니다).".to_string()
        },
        false,
    )]));
    b.push(Block::Paragraph(vec![Span::Text(format!("보드 글 번호 {}/{}", r.id, item_id), false)]));
    doc_clamp(&Doc::new(b))
}

/// 단위 티켓 댓글 본문(Jira) — 첫 줄 표지, 결과 한 줄, 모델이 쓴 요약 몇 줄, 글 번호. 원본: `apCommentDoc`
pub fn comment_doc(r: &ReportRun, unit: &ReportUnit, item_id: &str) -> Doc {
    let mut b: Vec<Block> = vec![Block::Paragraph(vec![Span::Text(DOC_MARKER.to_string(), true)])];
    let title = if unit.title.is_empty() { format!("단위 {}", unit.n) } else { unit.title.clone() };
    b.push(Block::Paragraph(vec![
        Span::Text(format!("{title}: "), false),
        Span::Text(unit_status_label(&unit.status), true),
        Span::Text(format!(" (단위 {}/{}, 일한 시간 {}, {} USD)", unit.n, r.units.len(), report_dur(unit.active_sec), usd_text(unit.cost)), false),
    ]));
    if let Some(res) = &unit.result {
        if !res.headline.is_empty() {
            b.push(Block::Paragraph(vec![Span::Text(res.headline.clone(), false)]));
        }
        let rows: Vec<Vec<Span>> = res.summary.iter().take(10).map(|s| vec![Span::Text(s.clone(), false)]).collect();
        if !rows.is_empty() {
            b.push(Block::Bullets(rows));
        }
    } else if !unit.reason.is_empty() {
        b.push(Block::Paragraph(vec![Span::Text(unit.reason.clone(), false)]));
    }
    b.push(Block::Paragraph(vec![Span::Text(format!("자율주행 실행 {} · 보드 글 번호 {}/{}", r.id, r.id, item_id), false)]));
    doc_clamp(&Doc::new(b))
}

/// 단위 댓글을 만드는 끝난 상태. 원본: `commentStates`
const COMMENT_STATES: [&str; 4] = ["done", "failed", "capped", "skipped"];

/// 단위 판정 뒤 만들 글 — 단위 티켓 댓글(게시 설정이 켰고 단위에 티켓 키가 있고 끝난 단위일 때). 원본: `apUnitOutbox`
pub fn unit_outbox(r: &ReportRun, unit: i64, ticket: Option<&str>, publish: &Publish, now: f64) -> Vec<OutboxItem> {
    let Some(key) = ticket.filter(|k| issue_key_ok(k)) else { return Vec::new() };
    if !publish.jira_comments {
        return Vec::new();
    }
    let Some(u) = r.units.iter().find(|u| u.n == unit) else {
        return Vec::new();
    };
    if !COMMENT_STATES.contains(&u.status.as_str()) {
        return Vec::new();
    }
    let id = format!("u{unit}-jira-{key}");
    let title_base = if u.title.is_empty() { format!("단위 {unit}") } else { u.title.clone() };
    let it = OutboxItem {
        id: id.clone(),
        run: r.id.clone(),
        unit: Some(unit),
        kind: "comment".to_string(),
        target: OutboxTarget::new(TargetService::Jira, key, "", ""),
        connection: "atlassian".to_string(),
        title: doc_text_n(&format!("{key} {}: {}", title_base, unit_status_label(&u.status)), 300, true),
        doc: comment_doc(r, u, &id),
        mode: publish.mode,
        created: now,
        updated: now,
        ..OutboxItem::default() // 나머지 칸(attachment·state·attempts)은 꼴 기본값(mode·state 의 enum 기본 = 원본 시작 값)
    };
    vec![it]
}

/// 끝 정리 때 만들 글 — 보고서(받을 곳이 있을 때). Confluence 는 report.html 을 첨부하고 Notion 은 요약만
/// (설계 판단: Notion 파일 올리기는 요청 세 단계라 다음 조각). 원본: `apRunOutbox`
pub fn run_outbox(r: &ReportRun, publish: &Publish, requester: &str, has_report: bool, now: f64, tz: Tz) -> Vec<OutboxItem> {
    let Some(t) = &publish.report else { return Vec::new() };
    let id = format!("run-{}", t.service.raw_value());
    let name = if r.project.is_empty() {
        if r.title.is_empty() { r.id.clone() } else { r.title.clone() }
    } else {
        r.project.clone()
    };
    let title = doc_text_n(&format!("자율주행 보고: {name} {}", day(r.started.or(r.ended).or(Some(now)), tz)), 300, true);
    let attach = t.service == TargetService::Confluence && has_report;
    let it = OutboxItem {
        id: id.clone(),
        run: r.id.clone(),
        kind: "report".to_string(),
        target: t.clone(),
        connection: target_connection(t.service).0.to_string(),
        title: title.clone(),
        attachment: if attach { Some("report.html".to_string()) } else { None },
        doc: report_doc(r, t.service == TargetService::Notion, attach, requester, &id, &title),
        mode: publish.mode,
        created: now,
        updated: now,
        ..OutboxItem::default() // 나머지 칸(unit·state·attempts)은 꼴 기본값(mode·state 의 enum 기본 = 원본 시작 값)
    };
    vec![it]
}

// MARK: - 상태 바꾸기와 검수 화면

/// 사람이 고른 상태 바꾸기(올리지 않기·나중에) — 잠금 안에서, 올린 글·올리는 중인 글은 바꾸지 않는다. 원본: `apMarkItem`
pub fn mark_item(id: &str, what: &str, store: &dyn OutboxStore, clock: &dyn NoticeClock) -> PostResult {
    let r = store.with_lock(id, &mut || {
        let Some(mut it) = store.read(id) else {
            return PostResult { note: "글을 찾지 못했습니다".to_string(), ..PostResult::default() };
        };
        if it.state == OutboxState::Posted || it.state == OutboxState::Posting {
            let note = if it.state == OutboxState::Posted { "이미 올렸습니다" } else { "올리는 중입니다" };
            return PostResult { item: Some(it), note: note.to_string(), ..PostResult::default() };
        }
        let t = clock.now();
        let note = match what {
            "skip" => {
                it.state = OutboxState::Skipped;
                it.later = None;
                it.last = Some(Attempt { at: t, what: "skipped".to_string(), code: 0, note: "올리지 않기(로컬에만)".to_string() });
                "올리지 않기(로컬에만)"
            }
            "later" => {
                it.later = Some(t);
                it.last = Some(Attempt { at: t, what: "later".to_string(), code: 0, note: "나중에(목록에 남김, 자동으로 올리지 않음)".to_string() });
                "나중에(목록에 남김, 자동으로 올리지 않음)"
            }
            "unskip" => {
                if it.state == OutboxState::Skipped {
                    it.state = OutboxState::Waiting;
                }
                it.last = Some(Attempt { at: t, what: "waiting".to_string(), code: 0, note: "다시 기다림".to_string() });
                "다시 기다림"
            }
            _ => return PostResult { item: Some(it), note: "모르는 바꾸기입니다".to_string(), ..PostResult::default() },
        };
        it.updated = t;
        store.write(&it);
        PostResult { item: Some(it), note: note.to_string(), ..PostResult::default() }
    });
    r.unwrap_or(PostResult {
        item: store.read(id),
        note: "다른 곳에서 이 글을 올리는 중입니다".to_string(),
        busy: true,
        ..PostResult::default()
    })
}

/// 글 만들기 — 같은 id 가 있으면 두지 않는다(다시 떠서 끝 정리를 다시 해도 하나). 원본: `apCreateItems`
pub fn create_items(items: Vec<OutboxItem>, store: &dyn OutboxStore) -> Vec<String> {
    items.into_iter().filter(|it| store.create(it)).map(|it| it.id).collect()
}

/// 검수 화면 한 줄 상태. 원본: `apOutboxStateLabel`
pub fn outbox_state_label(it: &OutboxItem) -> String {
    match it.state {
        OutboxState::Waiting => {
            if it.later.is_some() {
                "나중에".to_string()
            } else if it.mode == PublishMode::Review {
                "검수 기다림".to_string()
            } else if it.last.as_ref().is_some_and(|l| l.what == "held") {
                "올리지 못함(요청 0)".to_string()
            } else {
                "올리기 기다림".to_string()
            }
        }
        OutboxState::Posting => "올리는 중".to_string(),
        OutboxState::Posted => "올림".to_string(),
        OutboxState::Skipped => "올리지 않음(로컬에만)".to_string(),
        OutboxState::Failed => "올리지 못함".to_string(),
        OutboxState::Uncertain => "확인 필요".to_string(),
    }
}

/// "모두 올리기" 확인 글의 수 — 받을 곳 종류마다. 원본: `apOutboxCounts`
pub fn outbox_counts(items: &[OutboxItem]) -> String {
    let order = [(TargetService::Jira, "Jira 댓글"), (TargetService::Confluence, "Confluence"), (TargetService::Notion, "Notion")];
    let tickets = items.iter().filter(|it| it.kind == "ticket").count();
    let mut out: Vec<String> = Vec::new();
    if tickets > 0 {
        out.push(format!("Jira 티켓 {tickets}"));
    }
    for (service, name) in order {
        let n = items.iter().filter(|it| it.target.service == service && it.kind != "ticket").count();
        if n > 0 {
            out.push(format!("{name} {n}"));
        }
    }
    out.join(" · ")
}
