//! 출발 때 한꺼번에 만드는 Jira 티켓 나무 — 계획(보드가 씀)·상태(엔진이 씀)·요청 꼴. 원본: `Sources/AutopilotRules/Tickets.swift` 전체
//! (계획 12–103 · 상태 105–203 · 요청 꼴 205–293 · 만들기와 행·실행 중 새 티켓 295–528), 시험 make_all·retry_left·uncertain·blocked·waits_and_guards·types_and_keys·outbox·post_ticket.
//! 4단계 ⑩-3(2026-10-30, 설계 2장 끝·5장 6절). 출발 확인의 "만들 티켓"(에픽 아래 새 스토리·새 작업, 옮긴 작업의 부모 바꾸기)은
//! 보드가 계획 파일(tickets-plan.json)로 쓰고, 출발을 누르면 보드가 앱 안 엔진의 tickets 명령을 불러 만든다(보드는 HTTP 를 하지 않는다, ⑩-1).
//! 만든 키는 엔진이 상태 파일(tickets.json)에 남기고 보드가 그 키를 대기열(queue.json 의 story·tickets)에 넣은 뒤 출발한다.
//! 같은 것을 두 번 만들지 않게: 요청을 보내기 전에 항목을 "만드는 중"(creating)으로 쓰고, 받은 키를 바로 적고, 잠금 안에서 creating 을 보면
//! 앞 만들기가 끝나지 못하고 죽은 것이라 "확인 필요"(uncertain)로 바꾸고 사람이 확인하기 전에는 다시 만들지 않는다(⑩-1 올릴 글과 같은 꼴).
//! 실행 중에 새로 필요해진 티켓은 출발 카드 답대로 임시 번호(TMP-<단위>-<차례>)로 진행하고, 단위 판정 뒤 엔진이 올릴 글(kind ticket)로 남겨
//! 검수 뒤에 사람이 만든다(게시 방식이 자동으로 올림이어도 늘 검수 뒤 — 실행 중 티켓 만들기 요청 0).
//!
//! 이식된 선언(원본 순서): `TicketKind`(APTicketKind) · `TicketEntry`(APTicketEntry) · `TicketPlan`(APTicketPlan)·`project` · 상수 넷 ·
//! `ticket_entry_id_ok`(apTicketEntryIDOK) · `ticket_plan_json`(apTicketPlanJSON) · `ticket_plan_parse`(apTicketPlanParse) ·
//! `TicketState`(APTicketState) · `Attempt`(APAttempt) · `TicketRecord`(APTicketRecord) · `TicketBook`(APTicketBook) ·
//! `ticket_book_json`(apTicketBookJSON) · `ticket_book_parse`(apTicketBookParse) · `ticket_keys`(apTicketKeys) ·
//! `ticket_placeholder`(`apTicketPlaceholderPattern` 규칙 + `apTicketPlaceholder`) · `is_ticket_placeholder`(apIsTicketPlaceholder) ·
//! `unmade_keys`(apUnmadeKeys) · `tickets_left`(apTicketsLeft) · `IssueType`(APIssueType) · `jira_project_request`(apJiraProjectRequest) ·
//! `jira_issue_types`(apJiraIssueTypes) · `pick_issue_type`(apPickIssueType) · `ticket_doc`(apTicketDoc) ·
//! `jira_create_issue_request`(apJiraCreateIssueRequest) · `jira_parent_request`(apJiraParentRequest) ·
//! `created_issue_key`(apCreatedIssueKey) · `ticket_fail_note`(apTicketFailNote).
//!
//! Swift ↔ Rust 로 달라진 점:
//! - 정규식(`apMatch`)은 판별자로 옮겼다: `issue_key_ok` 는 Outbox.swift 38줄 `apIssueKeyPattern`, `temp_ticket_ok` 는 이 원본의
//!   455줄 `apTempTicketPattern`(선언은 b 청이지만 a 청의 `unmade_keys` 가 쓴다)을 옮겼다. Outbox 는 아직 이식 전이라 `fail_note`
//!   (Outbox.swift 422–435 줄 `apFailNote`)도 여기에 둔다 — R-게시 청이 outbox.rs 를 심을 때 같은 것을 쓰게 한다.
//! - 제목·단위 제목은 모델·바깥이 쓴 글일 수 있다(`Notify` apNotifyClean 원칙) — 비밀 가리기·다듬기·멘션 무력화(`notify_clean_n`), 상한.
//! - Swift `prefix(40)` 는 글자(Character) 수다 — grapheme 으로 센 `take` 근사(`grapheme_len`). Swift `Int(...)` 파싱은 `i64`
//!   (부호 표도 받는다).
//! - 키 순서: serde_json 은 `preserve_order` 라 — Swift 사전은 순서가 없다. 요청 본문은 `body` 에서 키를 정렬한다
//!   (JSONSerialization `[.sortedKeys, .withoutEscapingSlashes]`, `/` 는 이스케이프하지 않는다).
//! - `trimmingCharacters(in: .whitespaces)` 는 Zs 범주와 탭(`trim_ws`)이다 — `char::is_whitespace` 가 아니다.
//! - 상태 파일의 수 칸은 `NSNumber` 규칙(불리언 1·0)으로 읽는다(`num_f64`·`int_value`·`int_opt`).
//! - `TicketBook.entries` 는 순서가 없는 Swift Dictionary — BTreeMap 으로 둔다(시험은 키를 정렬해 견준다).
//! - 기본 인자(`apTicketDoc` 의 temp·why·task)는 Rust 에 없다 — 모두 받는다.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::rules::connections::{
    ap_expiry, auth_for, auth_header, same_site, service_base, Auth, AuthHeader, ConnHttp,
    ConnSecrets, Connection, Expiry, HttpRequest, HttpResponse, Service,
};
use crate::rules::formats::{
    adf, batches, doc_clamp, doc_json, doc_parse, doc_text_n, grapheme_len, notion_blocks, storage,
    Block, Doc, DOC_MARKER, Span,
};
use crate::rules::notify_rules::{
    notify_clean_n, NoticeClock, NoticeWaiter, OutboxState, PublishMode,
};
use crate::rules::report::{ReportRun, ReportUnit};
use crate::rules::stopline::Tz;
use serde_json::{json, Map, Value};

// 원본 상수 — `apNotionAPI`(Outbox.swift 469 사용) · `apNotionVersion`(Connections.swift 278)
use crate::rules::connections::{NOTION_API, NOTION_VERSION};

// MARK: - 계획(보드가 씀)

/// 티켓 종류. 원본: `APTicketKind`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketKind {
    Story,
    Task,
    Move,
}

impl TicketKind {
    /// Swift rawValue
    pub fn raw_value(&self) -> &'static str {
        match self {
            TicketKind::Story => "story",
            TicketKind::Task => "task",
            TicketKind::Move => "move",
        }
    }

    /// Swift `APTicketKind(rawValue:)`
    pub fn from_raw(s: &str) -> Option<TicketKind> {
        match s {
            "story" => Some(TicketKind::Story),
            "task" => Some(TicketKind::Task),
            "move" => Some(TicketKind::Move),
            _ => None,
        }
    }
}

/// 만들 티켓 하나 — id 는 출발 준비 안에서 바뀌지 않는 이름(단위 위치가 아니라):
/// story:<보드 스토리 id>·task:<단위 id>·move:<작업 키>. 원본: `APTicketEntry`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketEntry {
    pub id: String,
    pub kind: TicketKind,
    pub title: String,
    /// 에픽 키·이미 있는 스토리 키, 또는 같은 계획의 story:… 항목
    pub parent: String,
    /// move: 부모를 바꿀 이미 있는 작업 키
    pub key: String,
    /// 본문에 넣을 단위 제목(스토리는 그 스토리의 단위들, 작업은 그 단위 하나)
    pub units: Vec<String>,
}

impl Default for TicketEntry {
    /// Swift 기본값(kind 는 .task)
    fn default() -> Self {
        TicketEntry { id: String::new(), kind: TicketKind::Task, title: String::new(), parent: String::new(), key: String::new(), units: Vec::new() }
    }
}

/// 만들 티켓 계획(보드가 씀). 원본: `APTicketPlan`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketPlan {
    /// 출발 준비 id(출발하면 실행 id)
    pub run: String,
    pub epic: String,
    /// 프로젝트 설정 tickets.issue_types.story(비우면 응답의 이슈 종류에서 고름)
    pub story_type: String,
    /// 프로젝트 설정 tickets.issue_types.task(비우면 하위 작업 종류에서 고름)
    pub task_type: String,
    pub entries: Vec<TicketEntry>,
}

impl TicketPlan {
    /// 새 티켓의 프로젝트 — 에픽 키 앞(SHOP-10 → SHOP). Swift `split(separator:)` 는 빈 조각을 버린다
    pub fn project(&self) -> String {
        self.epic.split('-').find(|x| !x.is_empty()).unwrap_or("").to_string()
    }
}

/// 원본: `apTicketPlanFormat`
pub const TICKET_PLAN_FORMAT: &str = "claude-board-tickets-plan";
/// 원본: `apTicketBookFormat`
pub const TICKET_BOOK_FORMAT: &str = "claude-board-tickets";
/// Jira 요약 칸 255자 안. 원본: `apTicketTitleMax`
pub const TICKET_TITLE_MAX: usize = 240;
/// 한 번에 만들 티켓 수 상한. 원본: `apTicketEntryMax`
pub const TICKET_ENTRY_MAX: usize = 200;

/// 티켓 항목 id 꼴 — `^(story:<…>|task:<…>|move:<이슈 키>)$` 를 판별자로 옮긴 것. 원본: `apTicketEntryIDOK`
pub fn ticket_entry_id_ok(id: &str) -> bool {
    let Some((head, tail)) = id.split_once(':') else { return false };
    match head {
        "story" | "task" => {
            (1..=64).contains(&tail.chars().count()) && tail.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        }
        "move" => issue_key_ok(tail),
        _ => false,
    }
}

/// 계획 파일 값(보드가 쓴다). Swift 사전은 순서가 없다 — 요청 본문은 `body` 에서 키를 정렬해 내보낸다. 원본: `apTicketPlanJSON`
pub fn ticket_plan_json(p: &TicketPlan) -> Value {
    let entries: Vec<Value> = p
        .entries
        .iter()
        .map(|e| {
            let mut o = json!({"id": e.id, "kind": e.kind.raw_value(), "title": e.title, "parent": e.parent, "units": e.units});
            if !e.key.is_empty() {
                o["key"] = json!(e.key);
            }
            o
        })
        .collect();
    let mut d = json!({"format": TICKET_PLAN_FORMAT, "version": 1, "run": p.run, "epic": p.epic, "entries": entries});
    if !p.story_type.is_empty() || !p.task_type.is_empty() {
        d["issue_types"] = json!({"story": p.story_type, "task": p.task_type});
    }
    d
}

/// 계획 파일 → 계획과 고칠 것. 고칠 것이 하나라도 있으면 아무것도 만들지 않는다(요청 0). 원본: `apTicketPlanParse`
pub fn ticket_plan_parse(data: Option<&[u8]>) -> (TicketPlan, Vec<String>) {
    let mut p = TicketPlan::default();
    let mut probs: Vec<String> = Vec::new();
    // 읽는다: JSONSerialization, 1MiB, 그리고 format 표지. 어느 하나라도 아니면 "읽지 못했다" 하나만
    let o: Value = match data {
        Some(d) if d.len() <= 1 << 20 => serde_json::from_slice(d).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    if o.get("format").and_then(Value::as_str) != Some(TICKET_PLAN_FORMAT) {
        return (p, vec!["만들 티켓 계획 파일을 읽지 못했습니다".to_string()]);
    }
    p.run = o.get("run").and_then(Value::as_str).unwrap_or("").to_string();
    p.epic = trim_ws(o.get("epic").and_then(Value::as_str).unwrap_or("")).to_string();
    if let Some(t) = o.get("issue_types").and_then(Value::as_object) {
        p.story_type = doc_text_n(t.get("story").and_then(Value::as_str).unwrap_or(""), 80, true);
        p.task_type = doc_text_n(t.get("task").and_then(Value::as_str).unwrap_or(""), 80, true);
    }
    if !issue_key_ok(&p.epic) {
        probs.push(format!("에픽 번호 꼴이 아닙니다: {}", take(&p.epic, 40)));
    }
    let list: Vec<Value> = o.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.len() > TICKET_ENTRY_MAX {
        probs.push(format!("한 번에 만들 티켓은 {}개까지입니다({}개)", TICKET_ENTRY_MAX, list.len()));
    }
    let mut seen: HashSet<String> = HashSet::new();
    for x in list.iter().take(TICKET_ENTRY_MAX) {
        let Some(e) = x.as_object() else {
            probs.push("꼴이 틀린 항목이 있습니다".to_string());
            continue;
        };
        let Some(id) = e.get("id").and_then(Value::as_str).filter(|s| ticket_entry_id_ok(s)) else {
            probs.push("꼴이 틀린 항목이 있습니다".to_string());
            continue;
        };
        let Some(k) = e.get("kind").and_then(Value::as_str).and_then(TicketKind::from_raw) else {
            probs.push("꼴이 틀린 항목이 있습니다".to_string());
            continue;
        };
        if !seen.insert(id.to_string()) {
            probs.push(format!("같은 항목이 둘입니다: {id}"));
            continue;
        }
        let t = TicketEntry {
            id: id.to_string(),
            kind: k,
            title: notify_clean_n(e.get("title").and_then(Value::as_str).unwrap_or(""), TICKET_TITLE_MAX),
            parent: trim_ws(e.get("parent").and_then(Value::as_str).unwrap_or("")).to_string(),
            key: trim_ws(e.get("key").and_then(Value::as_str).unwrap_or("")).to_string(),
            units: e
                .get("units")
                .and_then(Value::as_array)
                .map(|a| a.iter().take(50).filter_map(Value::as_str).map(|s| notify_clean_n(s, 200)).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default(),
        };
        if id.split(':').find(|x| !x.is_empty()).unwrap_or("") != k.raw_value() {
            probs.push(format!("항목 {id} 의 종류가 이름과 다릅니다"));
        }
        if k != TicketKind::Move && t.title.is_empty() {
            probs.push(format!("항목 {id} 의 제목이 비었습니다"));
        }
        if k == TicketKind::Move && (!issue_key_ok(&t.key) || id != format!("move:{}", t.key)) {
            probs.push(format!("옮길 작업 키 꼴이 아닙니다: {}", take(&t.key, 40)));
        }
        p.entries.push(t);
    }
    // 부모: 스토리는 에픽 아래, 작업·옮기기는 이미 있는 스토리 키 또는 같은 계획의 새 스토리
    let stories: HashSet<String> = p.entries.iter().filter(|e| e.kind == TicketKind::Story).map(|e| e.id.clone()).collect();
    for e in &p.entries {
        match e.kind {
            TicketKind::Story => {
                if e.parent != p.epic {
                    probs.push(format!("스토리 {} 의 부모가 에픽({})이 아닙니다", e.id, p.epic));
                }
            }
            TicketKind::Task | TicketKind::Move => {
                if e.parent.starts_with("story:") {
                    if !stories.contains(&e.parent) {
                        probs.push(format!("항목 {} 의 부모 {} 가 계획에 없습니다", e.id, e.parent));
                    }
                } else if !issue_key_ok(&e.parent) {
                    probs.push(format!("항목 {} 의 부모 꼴이 아닙니다: {}", e.id, take(&e.parent, 40)));
                }
            }
        }
    }
    (p, probs)
}

// MARK: - 상태(엔진이 쓴다)

/// 티켓 하나의 상태. 원본: `APTicketState`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TicketState {
    /// 아직 시작 전(waits: 항목이 있지만 만들기 요청이 아직 없다)
    #[default]
    Waiting,
    /// 요청을 보내는 중(보내기 전 상태, 답을 받으면 바꾼다)
    Creating,
    Made,
    Failed,
    /// 잠금에서 creating 이 남아 앞 만들기가 끝났는지 알 수 없다 — 사람이 확인해야 다시 만든다
    Uncertain,
}

impl TicketState {
    /// Swift rawValue
    pub fn raw_value(&self) -> &'static str {
        match self {
            TicketState::Waiting => "waiting",
            TicketState::Creating => "creating",
            TicketState::Made => "made",
            TicketState::Failed => "failed",
            TicketState::Uncertain => "uncertain",
        }
    }

    /// Swift `APTicketState(rawValue:)`
    pub fn from_raw(s: &str) -> Option<TicketState> {
        match s {
            "waiting" => Some(TicketState::Waiting),
            "creating" => Some(TicketState::Creating),
            "made" => Some(TicketState::Made),
            "failed" => Some(TicketState::Failed),
            "uncertain" => Some(TicketState::Uncertain),
            _ => None,
        }
    }
}

/// 요청 한 번의 기록. 원본 Outbox.swift 141–145 줄 `APAttempt`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Attempt {
    pub at: f64,
    pub what: String,
    pub code: i32,
    pub note: String,
}

/// 티켓 하나의 상태 기록. 원본: `APTicketRecord`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TicketRecord {
    pub state: TicketState,
    /// 만든 키(move 는 옮긴 작업 키)
    pub key: String,
    pub url: String,
    pub last: Option<Attempt>,
    pub tries: i32,
    pub updated: f64,
}

/// 상태 파일(tickets.json, 엔진이 쓴다 — 보드는 읽기만). 원본: `APTicketBook`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TicketBook {
    /// 만들기 시작한 사이트(만든 키는 이 사이트의 것)
    pub site: String,
    /// Swift Dictionary 는 순서가 없다 — BTreeMap 으로 둔다(시험은 키를 정렬해 견준다)
    pub entries: BTreeMap<String, TicketRecord>,
}

/// Swift `NSNumber` 수 꺼내기 — JSONSerialization 은 true·false 도 NSNumber(1·0)로 준다
fn num_f64(v: &Value) -> f64 {
    match v {
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Swift `(x as? NSNumber)?.intValue`
fn int_value(v: &Value) -> i32 {
    match v {
        Value::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        Value::Number(n) => n.as_i64().map(|x| x as i32).or_else(|| n.as_f64().map(|f| f as i32)).unwrap_or(0),
        _ => 0,
    }
}

/// Swift `(x as? NSNumber)` — 칸이 없거나 문자·사전이면 None(불리언·수만)
fn int_opt(v: Option<&Value>) -> Option<i32> {
    match v {
        Some(Value::Bool(_)) | Some(Value::Number(_)) => Some(int_value(v.unwrap())),
        _ => None,
    }
}

/// 상태 파일 값(엔진이 쓴다). 원본: `apTicketBookJSON`
pub fn ticket_book_json(b: &TicketBook) -> Value {
    let mut es = Map::new();
    for (k, r) in &b.entries {
        let mut d = json!({"state": r.state.raw_value(), "tries": r.tries, "updated": r.updated});
        if !r.key.is_empty() {
            d["key"] = json!(r.key);
        }
        if !r.url.is_empty() {
            d["url"] = json!(r.url);
        }
        if let Some(a) = &r.last {
            d["last"] = json!({"at": a.at, "what": a.what, "code": a.code, "note": a.note});
        }
        es.insert(k.clone(), d);
    }
    let mut o = json!({"format": TICKET_BOOK_FORMAT, "version": 1, "entries": Value::Object(es)});
    if !b.site.is_empty() {
        o["site"] = json!(b.site);
    }
    o
}

/// 상태 파일 → 책. 꼴이 틀린 칸은 버린다(상태 없음으로). 원본: `apTicketBookParse`
pub fn ticket_book_parse(data: Option<&[u8]>) -> TicketBook {
    let mut b = TicketBook::default();
    let o: Value = match data {
        Some(d) if d.len() <= 1 << 20 => serde_json::from_slice(d).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    if o.get("format").and_then(Value::as_str) != Some(TICKET_BOOK_FORMAT) {
        return b;
    }
    b.site = o.get("site").and_then(Value::as_str).unwrap_or("").to_string();
    if let Some(es) = o.get("entries").and_then(Value::as_object) {
        for (k, v) in es {
            if !ticket_entry_id_ok(k) {
                continue;
            }
            let Some(d) = v.as_object() else { continue };
            let Some(st) = d.get("state").and_then(Value::as_str).and_then(TicketState::from_raw) else { continue };
            let mut r = TicketRecord { state: st, ..Default::default() };
            let key = d.get("key").and_then(Value::as_str).unwrap_or("");
            r.key = if issue_key_ok(key) { key.to_string() } else { String::new() };
            r.url = doc_text_n(d.get("url").and_then(Value::as_str).unwrap_or(""), 500, true);
            if let Some(a) = d.get("last").and_then(Value::as_object) {
                r.last = Some(Attempt {
                    at: num_f64(a.get("at").unwrap_or(&Value::Null)),
                    what: a.get("what").and_then(Value::as_str).unwrap_or("").to_string(),
                    code: int_value(a.get("code").unwrap_or(&Value::Null)),
                    note: doc_text_n(a.get("note").and_then(Value::as_str).unwrap_or(""), 500, true),
                });
            }
            r.tries = int_value(d.get("tries").unwrap_or(&Value::Null));
            r.updated = num_f64(d.get("updated").unwrap_or(&Value::Null));
            if r.state == TicketState::Made && r.key.is_empty() {
                r.state = TicketState::Uncertain;
            }
            b.entries.insert(k.clone(), r);
        }
    }
    b
}

/// 만든 키 — 보드가 대기열에 넣는다: 보드 스토리 id → 스토리 키, 단위 id → 작업 키. 원본: `apTicketKeys`
pub fn ticket_keys(p: &TicketPlan, b: &TicketBook) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut s: BTreeMap<String, String> = BTreeMap::new();
    let mut t: BTreeMap<String, String> = BTreeMap::new();
    for e in &p.entries {
        let Some(r) = b.entries.get(&e.id) else { continue };
        if r.state != TicketState::Made || r.key.is_empty() {
            continue;
        }
        let tail = e.id.split_once(':').map(|x| x.1).unwrap_or("");
        match e.kind {
            TicketKind::Story => {
                s.insert(tail.to_string(), r.key.clone());
            }
            TicketKind::Task => {
                t.insert(tail.to_string(), r.key.clone());
            }
            TicketKind::Move => {}
        }
    }
    (s, t)
}

// MARK: - 키 가림막·임시 번호(이 심부름이 쓰는 것)

/// 이슈 키 꼴 — `^[A-Z][A-Z0-9_]*-[0-9]{1,9}$` 를 판별자로 옮긴 것. 원본: Outbox.swift 38줄 `apIssueKeyPattern`
/// (Outbox 는 아직 이식 전이라 여기에 두고, R-게시 청이 outbox.rs 를 심으면 그쪽에서 같은 것을 쓰게 한다)
pub fn issue_key_ok(s: &str) -> bool {
    let Some((head, digits)) = s.split_once('-') else { return false };
    let mut hs = head.chars();
    if !matches!(hs.next(), Some(c) if c.is_ascii_uppercase()) {
        return false;
    }
    if !hs.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return false;
    }
    (1..=9).contains(&digits.chars().count()) && digits.chars().all(|c| c.is_ascii_digit())
}

/// 임시 티켓 번호 꼴 — `^TMP-[0-9]{1,4}-[0-9]{1,2}$` 를 판별자로 옮긴 것. 원본: Tickets.swift 455줄 `apTempTicketPattern`
/// (선언은 R-Tickets-b 자리지만 a 청의 `unmade_keys` 가 쓴다)
pub fn temp_ticket_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("TMP-") else { return false };
    let Some((unit, seq)) = rest.split_once('-') else { return false };
    (1..=4).contains(&unit.chars().count()) && unit.chars().all(|c| c.is_ascii_digit()) && (1..=2).contains(&seq.chars().count()) && seq.chars().all(|c| c.is_ascii_digit())
}

/// 자리표가 될 수 없는 이슈 키 꼭 꼴 — `^[A-Z][A-Z0-9_]*-NEW([0-9]{1,4}|-[A-Za-z0-9._]{1,64})$` 를 판별자로 옮긴 것.
/// (이슈 키 꼴은 숫자 부분이 있어 NEW 로 시작할 수 없다) 원본: `apIsTicketPlaceholder`
pub fn is_ticket_placeholder(s: &str) -> bool {
    let Some((head, tail)) = s.split_once('-') else { return false };
    let mut hs = head.chars();
    if !matches!(hs.next(), Some(c) if c.is_ascii_uppercase()) {
        return false;
    }
    if !hs.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return false;
    }
    let Some(rest) = tail.strip_prefix("NEW") else { return false };
    let Some(rest) = rest.strip_prefix('-') else {
        return (1..=4).contains(&rest.chars().count()) && rest.chars().all(|c| c.is_ascii_digit());
    };
    (1..=64).contains(&rest.chars().count()) && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// 대기열에 넣을 키 자리표 — story:new-N 은 SHOP-NEW{N}, 그 밖은 SHOP-NEW-<id 꼬리>. Swift 기본 인자(`project=""`)는 없다.
/// 이슈 키 꼭 꼴(숫자 부분)과 임시 번호 꼴은 출발을 막는다(엔진 start 는 자리표가 남은 대기열로 출발하지 않는다).
/// 원본: `apTicketPlaceholderPattern`(규칙) · `apTicketPlaceholder`(값)
pub fn ticket_placeholder(project: &str, entry: &str) -> String {
    let tail = entry.split_once(':').map(|x| x.1).unwrap_or("");
    if entry.starts_with("story:") && tail.starts_with("new-") {
        if let Ok(k) = tail[4..].parse::<i64>() {
            if (0..=9999).contains(&k) {
                return format!("{project}-NEW{k}");
            }
        }
    }
    let safe: String = tail
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' { c } else { '_' })
        .take(64)
        .collect();
    format!("{project}-NEW-{safe}")
}

/// 출발하면 안 되는 키 — 대기열 단위의 story·tickets 에 자리표나 임시 번호(TMP-…)가 남았으면 그 값들(처음 본 순서). 원본: `apUnmadeKeys`
pub fn unmade_keys(units: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for u in units {
        let mut vals: Vec<String> = Vec::new();
        if let Some(x) = u.get("story").and_then(Value::as_str) {
            vals.push(x.to_string());
        }
        if let Some(a) = u.get("tickets").and_then(Value::as_array) {
            vals.extend(a.iter().filter_map(Value::as_str).map(str::to_string));
        }
        for v in vals {
            if (is_ticket_placeholder(&v) || temp_ticket_ok(&v)) && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// 남은 항목(만들지 않았거나 실패·확인 필요) — 출발은 이것이 없을 때만. 원본: `apTicketsLeft`
pub fn tickets_left(p: &TicketPlan, b: &TicketBook) -> Vec<TicketEntry> {
    p.entries
        .iter()
        .filter(|e| !b.entries.get(&e.id).map(|r| r.state == TicketState::Made).unwrap_or(false))
        .cloned()
        .collect()
}

// MARK: - 이 파일 안에서만 쓰는 것들

/// `trimmingCharacters(in: .whitespaces)` — Zs 범주와 탭(formats.rs `ws` 와 같은 판별)
fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'))
}

/// Swift `String.prefix(n)` 근사 — grapheme 으로 센 `n` 개 덩어리까지의 접두어. (formats.rs 의 cluster 함수는 비공개라
/// 접두어마다 grapheme_len 을 다시 센다 — 여기는 짧은 까닭 문장만 자른다) `len_utf8` 으로 자리를 옮겨가며 다음 덩어리가
/// 시작하는 자리(그 앞이 n 개)를 찾는다
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

// MARK: - 요청 꼴(보드가 HTTP 를 하지 않으므로 꼴만 준다)

/// Jira 이슈 종류 응답 한 줄. 원본: `APIssueType`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueType {
    pub id: String,
    pub name: String,
    pub subtask: bool,
    /// hierarchyLevel: 에픽 1, 스토리·작업 0, 하위 작업 -1
    pub level: Option<i32>,
}

/// Swift JSONSerialization `[.sortedKeys, .withoutEscapingSlashes]` — 키를 자순(deep)으로 정렬해 내보낸다(`/` 는 이스케이프하지
/// 않는다, serde_json 도 같다). 순서 있는 Map(preserve_order)이므로 BTreeMap 순서로 다시 싣는다
fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let keys: BTreeMap<String, &Value> = m.iter().map(|(k, val)| (k.clone(), val)).collect();
            let mut sorted = Map::new();
            for (k, val) in keys {
                sorted.insert(k, sort_keys(val));
            }
            Value::Object(sorted)
        }
        Value::Array(a) => Value::Array(a.iter().map(sort_keys).collect()),
        other => other.clone(),
    }
}

/// 본문 인코딩(Swift `body` 와 같다 — 실패는 0바이트). 요청 본문의 키는 항상 정렬해 답이 견주기 쉽게
fn body(o: &Value) -> Vec<u8> {
    serde_json::to_vec(&sort_keys(o)).unwrap_or_default()
}

fn json_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Accept".to_string(), "application/json".to_string()),
    ])
}

/// 프로젝트의 이슈 종류 — 종류 이름은 사이트·언어마다 달라(Story·스토리, Subtask·Sub-task·하위 작업) 이름을 박지 않고 응답에서 고른다.
/// 원본: `apJiraProjectRequest`
pub fn jira_project_request(project: &str, base: &str, auth: Auth) -> HttpRequest {
    let mut headers = BTreeMap::new();
    headers.insert("Accept".to_string(), "application/json".to_string());
    HttpRequest { method: "GET".to_string(), url: format!("{base}/rest/api/3/project/{project}"), headers, body: None, auth }
}

/// GET /rest/api/3/project/<키> 의 응답에서 이슈 종류를 읽는다(믿지 않는 값). 원본: `apJiraIssueTypes`
pub fn jira_issue_types(r: &HttpResponse) -> Vec<IssueType> {
    if r.error.is_some() || r.status != 200 || r.body.len() > 4 << 20 {
        return Vec::new();
    }
    let Ok(o) = serde_json::from_slice::<Value>(&r.body) else { return Vec::new() };
    let list: Vec<Value> = o.get("issueTypes").and_then(Value::as_array).cloned().unwrap_or_default();
    list.iter()
        .take(100)
        .filter_map(|x| {
            let t = x.as_object()?;
            let id = t.get("id").and_then(Value::as_str)?;
            if !((1..=20).contains(&id.chars().count()) && id.chars().all(|c| c.is_ascii_digit())) {
                return None;
            }
            Some(IssueType {
                id: id.to_string(),
                name: doc_text_n(t.get("name").and_then(Value::as_str).unwrap_or(""), 80, true),
                subtask: t.get("subtask").and_then(Value::as_bool).unwrap_or(false),
                level: int_opt(t.get("hierarchyLevel")),
            })
        })
        .collect()
}

/// 이슈 종류 고르기 — 프로젝트 설정에 이름이 있으면 그 이름(대소문자 무시), 없으면 스토리는 하위 작업이 아닌 수준 0 의
/// Story·스토리, 작업·옮기기는 하위 작업 종류(subtask 또는 수준 -1)의 Subtask·Sub-task·하위 작업, 그것도 없으면 그 무리의
/// 첫 종류. 프로젝트 설정 이름이 없는 무리이면 없다. 원본: `apPickIssueType`
pub fn pick_issue_type(types: &[IssueType], kind: TicketKind, name: &str) -> Option<IssueType> {
    let want = trim_ws(name).to_lowercase();
    if !want.is_empty() {
        return types.iter().find(|t| t.name.to_lowercase() == want).cloned();
    }
    match kind {
        TicketKind::Story => {
            let pool: Vec<&IssueType> = types.iter().filter(|t| !t.subtask && t.level.unwrap_or(0) == 0).collect();
            pool.iter()
                .find(|t| t.name.to_lowercase() == "story" || t.name == "스토리")
                .or_else(|| {
                    pool.iter().find(|t| {
                        let n = t.name.to_lowercase();
                        !(n == "task" || n == "bug" || t.name == "작업" || t.name == "버그")
                    })
                })
                .or_else(|| pool.first())
                .map(|t| (*t).clone())
        }
        TicketKind::Task | TicketKind::Move => {
            let pool: Vec<&IssueType> = types.iter().filter(|t| t.subtask || t.level == Some(-1)).collect();
            pool.iter()
                .find(|t| {
                    let n = t.name.to_lowercase();
                    n == "subtask" || n == "sub-task" || t.name == "하위 작업"
                })
                .or_else(|| pool.first())
                .map(|t| (*t).clone())
        }
    }
}

/// 티켓 본문 — 첫 줄 표지, 무엇을 위한 티켓인지, 단위 제목, 보드 티켓 번호(사람이 Jira 에서 찾게).
/// Swift 기본 인자(`temp=""`·`why=""`·`task=""`)는 Rust 에 없어 모두 받는다. 원본: `apTicketDoc`
pub fn ticket_doc(e: &TicketEntry, run: &str, temp: &str, why: &str, task: &str) -> Doc {
    let mut b: Vec<Block> = vec![Block::Paragraph(vec![Span::Text(DOC_MARKER.to_string(), true)])];
    if !temp.is_empty() {
        // 실행 중에 새로 필요해진 티켓 — 가지는 이 번호로 만들지 않았다(임시 번호로 진행)
        let tail_m = if task.is_empty() { String::new() } else { format!("(작업 {task})") };
        b.push(Block::Paragraph(vec![Span::Text(
            format!("자율주행 실행 {run} 의 단위{tail_m}가 일하다가 새로 필요해진 티켓입니다. 실행 중에는 임시 번호 {temp} 로 진행했고, 사람이 검수한 뒤 만든 티켓입니다."),
            false,
        )]));
        if !why.is_empty() {
            b.push(Block::Paragraph(vec![Span::Text(format!("까닭: {why}"), false)]));
        }
    } else {
        match e.kind {
            TicketKind::Story => b.push(Block::Paragraph(vec![Span::Text(
                format!("자율주행 실행 {run} 의 출발 확인에서 승인한 스토리입니다. 아래 작업 티켓의 단위를 묶습니다."),
                false,
            )])),
            TicketKind::Task | TicketKind::Move => b.push(Block::Paragraph(vec![Span::Text(
                format!("자율주행 실행 {run} 의 단위 하나(작업 티켓)입니다. 엔진이 이 번호로 작업 가지를 만듭니다."),
                false,
            )])),
        }
    }
    if !e.units.is_empty() {
        b.push(Block::Bullets(e.units.iter().take(50).map(|u| vec![Span::Text(u.clone(), false)]).collect()));
    }
    b.push(Block::Paragraph(vec![Span::Text(format!("보드 티켓 번호 {run}/{}", e.id), false)]));
    doc_clamp(&Doc::new(b))
}

/// 만들기 — POST /rest/api/3/issue. 스토리는 parent 에 에픽, 작업(하위 작업 종류)은 parent 에 스토리.
/// 제목은 모델·바깥이 쓴 글일 수 있다 — 상한(TICKET_TITLE_MAX)·다듬기, 본문은 문서 모형(계획 꼴). 원본: `apJiraCreateIssueRequest`
pub fn jira_create_issue_request(project: &str, type_id: &str, parent: &str, summary: &str, doc: &Doc, base: &str, auth: Auth) -> HttpRequest {
    let fields = json!({
        "project": {"key": project},
        "issuetype": {"id": type_id},
        "parent": {"key": parent},
        "summary": doc_text_n(summary, TICKET_TITLE_MAX, true),
        "description": adf(doc),
    });
    HttpRequest { method: "POST".to_string(), url: format!("{base}/rest/api/3/issue"), headers: json_headers(), body: Some(body(&json!({"fields": fields}))), auth }
}

/// 부모 바꾸기 — PUT /rest/api/3/issue/<키>(다른 스토리로 옮긴 이미 있는 작업). 하위 작업의 부모 바꾸기는 사이트 설정에 따라
/// 거부될 수 있다(그때 실패 줄). 원본: `apJiraParentRequest`
pub fn jira_parent_request(key: &str, parent: &str, base: &str, auth: Auth) -> HttpRequest {
    let o = json!({"fields": {"parent": {"key": parent}}});
    HttpRequest { method: "PUT".to_string(), url: format!("{base}/rest/api/3/issue/{key}"), headers: json_headers(), body: Some(body(&o)), auth }
}

/// 만든 키 — 응답(믿지 않는 값)의 key 칸이 이슈 키 꼴이고 그 프로젝트의 것일 때만. 원본: `apCreatedIssueKey`
pub fn created_issue_key(r: &HttpResponse, project: &str) -> Option<String> {
    if r.error.is_some() || !(200..=299).contains(&r.status) || r.body.len() > 1 << 20 {
        return None;
    }
    let o: Value = serde_json::from_slice(&r.body).ok()?;
    let k = o.get("key").and_then(Value::as_str)?;
    if !issue_key_ok(k) || !k.starts_with(&format!("{project}-")) {
        return None;
    }
    Some(k.to_string())
}

/// 실패 까닭(사람 말) — 받는 쪽 본문은 믿지 않아 상태 번호로만. 원본: Outbox.swift 422–435 줄 `apFailNote`
/// (Outbox 는 아직 이식 전이라 여기에 둔다 — R-게시 청이 outbox.rs 를 심을 때 같은 것을 쓰게 한다)
pub fn fail_note(r: &HttpResponse) -> String {
    if let Some(e) = &r.error {
        return e.clone();
    }
    match r.status {
        401 | 403 => format!("거부됨({}): 토큰이 만료·취소됐거나 그 자리에 쓸 권한이 없습니다", r.status),
        404 => "받을 곳을 찾지 못했습니다(404): 이슈·스페이스·페이지 id 를 확인합니다".to_string(),
        400 => "받는 쪽이 글 꼴을 받지 않았습니다(400): 같은 제목의 페이지가 있거나 받을 곳 값이 틀렸을 수 있습니다".to_string(),
        409 => "받는 쪽에서 부딪혔습니다(409)".to_string(),
        413 => "글이 너무 큽니다(413)".to_string(),
        429 => "요청이 많다며 거절했습니다(429). 잠시 뒤 다시 올립니다".to_string(),
        300..=399 => format!("다른 주소로 보내려 해 따라가지 않았습니다({})", r.status),
        500..=599 => format!("받는 쪽 오류({})", r.status),
        _ => format!("받는 쪽이 {} 로 답했습니다", r.status),
    }
}

/// 만들기 실패 까닭 — 받는 쪽 본문은 믿지 않아 상태 번호로만(Jira 티켓에 맞춘 말, 그 밖은 `fail_note`). 원본: `apTicketFailNote`
pub fn ticket_fail_note(r: &HttpResponse) -> String {
    if r.error.is_some() {
        return fail_note(r);
    }
    match r.status {
        400 => "Jira 가 요청 꼴을 받지 않았습니다(400): 이슈 종류·부모·필수 칸을 확인합니다(프로젝트 설정 tickets.issue_types)".to_string(),
        401 | 403 => format!("거부됨({}): 토큰이 만료·취소됐거나 그 프로젝트에 티켓을 만들 권한이 없습니다", r.status),
        404 => "프로젝트나 부모 이슈를 찾지 못했습니다(404)".to_string(),
        _ => fail_note(r),
    }
}

// MARK: - 만들어도 되는가

/// Atlassian 연결로 티켓을 만들 수 없는 까닭(요청 0) — 연결·범위 jira·사이트·계정 메일·토큰·만료. None 이면 만들 수 있다. 원본: `apTicketBlocker`
pub fn ticket_blocker(c: Option<&Connection>, has_secret: bool, dev: bool, now: f64, tz: Tz) -> Option<String> {
    let Some(c) = c.filter(|c| c.service == Service::Atlassian) else {
        return Some("Atlassian 연결이 없습니다(설정 › 연결)".to_string());
    };
    if !c.scopes.contains(&"jira".to_string()) {
        return Some("Atlassian 연결의 범위에 'Jira 쓰기'가 없습니다".to_string());
    }
    if service_base(c, dev).is_none() {
        return Some("Atlassian 사이트 주소가 규칙에 맞지 않습니다".to_string());
    }
    if c.account.is_empty() {
        return Some("Atlassian 계정 메일이 없습니다".to_string());
    }
    if !has_secret {
        return Some("Atlassian 토큰이 키체인에 없습니다".to_string());
    }
    if let Expiry::Expired(_) = ap_expiry(&c.expires, now, tz) {
        return Some(format!("Atlassian 토큰 만료일({})이 지났습니다", c.expires));
    }
    None
}

// MARK: - 만들기 유스케이스

/// 계획 파일 자리의 상태 파일(tickets.json)과 그 잠금 — 쓰기는 임시 파일 + rename, 잠금은 lock(프로세스가 죽으면 풀림). 원본: `APTicketStore`
pub trait TicketStore {
    fn read(&self) -> TicketBook;
    fn write(&self, b: &TicketBook) -> bool;
    /// None 이면 다른 곳에서 잠금을 잡고 있다(원본은 generic <T> — dyn 가능하도록 이 안에서 쓰는 결과 타입으로 고쳤다)
    fn with_lock(&self, body: &mut dyn FnMut() -> TicketsResult) -> Option<TicketsResult>;
}

/// 원본: `apTicketsResult`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TicketsResult {
    pub book: TicketBook,
    pub requests: i64,
    /// 이번에 만든(옮긴) 항목
    pub made: Vec<String>,
    /// 이번에 실패한 항목
    pub failed: Vec<String>,
    pub note: String,
    pub busy: bool,
    /// 끝난 뒤 남은 항목 수(0 이면 출발할 수 있다)
    pub left: i64,
}

/// 만들기 한 바퀴의 상태 — book 과 보낸 요청 수 등을 함께 두면 Swift 의 중첩 함수(send·mark)를 메서드로 1:1 옮긴다
struct MakeRun<'a> {
    p: &'a TicketPlan,
    store: &'a dyn TicketStore,
    http: &'a dyn ConnHttp,
    waiter: &'a dyn NoticeWaiter,
    clock: &'a dyn NoticeClock,
    base: String,
    header: Option<AuthHeader>,
    book: TicketBook,
    start: f64,
    budget: f64,
    requests: i64,
    last_wait: Option<f64>,
    made: Vec<String>,
    failed: Vec<String>,
}

impl MakeRun<'_> {
    /// 요청 하나 보내기 — 429 다시 보내기(retry-after 의 `w` 초, 시간 상한 안에서), 사이트 밖 주소는 보내지 않는다
    fn send(&mut self, req: &HttpRequest) -> HttpResponse {
        if !same_site(&req.url, &self.base) {
            return HttpResponse { error: Some("연결한 사이트 밖 주소라 보내지 않았습니다".to_string()), ..HttpResponse::default() };
        }
        let mut attempt: i32 = 0;
        loop {
            self.requests += 1;
            let resp = self.http.send(req, self.header.as_ref());
            let Some(w) = retry_wait(resp.status, resp.headers.get("retry-after").map(String::as_str), attempt, self.clock.now()) else { return resp };
            if self.clock.now() + w > self.start + self.budget {
                self.last_wait = Some(w);
                return resp;
            }
            self.waiter.wait(w);
            attempt += 1;
        }
    }

    /// 상태 기록 한 줄(쓰기까지). 원본의 중첩 함수 `mark`
    fn mark(&mut self, id: &str, st: TicketState, key: &str, code: i32, note: &str) {
        let t = self.clock.now();
        let rec = self.book.entries.entry(id.to_string()).or_default();
        rec.state = st;
        rec.updated = t;
        if !key.is_empty() {
            rec.key = key.to_string();
            rec.url = format!("{}/browse/{}", self.base, key);
        }
        rec.last = Some(Attempt { at: t, what: st.raw_value().to_string(), code, note: note.to_string() });
        self.store.write(&self.book);
    }

    /// 부모 키 — 이미 있는 키이거나(story: 로 시작하지 않음), 같은 계획의 스토리면 만든 키
    fn parent_key(&self, e: &TicketEntry) -> Option<String> {
        if !e.parent.starts_with("story:") {
            return Some(e.parent.clone());
        }
        let r = self.book.entries.get(&e.parent)?;
        if r.state != TicketState::Made || r.key.is_empty() {
            return None;
        }
        Some(r.key.clone())
    }

    fn finish(&self, note: &str) -> TicketsResult {
        TicketsResult {
            book: self.book.clone(),
            requests: self.requests,
            made: self.made.clone(),
            failed: self.failed.clone(),
            note: note.to_string(),
            busy: false,
            left: tickets_left(self.p, &self.book).len() as i64,
        }
    }
}

/// 남은 것만 차례로(스토리 → 작업 → 옮기기). confirm 은 사람이 "그래도 만들기"로 확인한 확인 필요 항목.
/// 스토리를 만들지 못하면 그 스토리의 작업·옮기기는 보내지 않고 기다린다. budget 은 429 기다림까지 합친 초 상한
#[allow(clippy::too_many_arguments)]
pub fn make_tickets(
    p: &TicketPlan,
    store: &dyn TicketStore,
    connections: &[Connection],
    dev: bool,
    secrets: &dyn ConnSecrets,
    http: &dyn ConnHttp,
    clock: &dyn NoticeClock,
    waiter: &dyn NoticeWaiter,
    tz: Tz,
    confirm: &BTreeSet<String>,
    budget: f64,
) -> TicketsResult {
    let r: Option<TicketsResult> = store.with_lock(&mut || make_locked(p, store, connections, dev, secrets, http, clock, waiter, tz, confirm, budget));    if let Some(r) = r {
        return r;
    }
    let book = store.read();
    let left_busy = tickets_left(p, &book).len() as i64;
    TicketsResult {
        book,
        busy: true,
        note: "다른 곳에서 이 티켓들을 만드는 중입니다".to_string(),
        left: left_busy,
        ..TicketsResult::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn make_locked(p: &TicketPlan, store: &dyn TicketStore, connections: &[Connection], dev: bool, secrets: &dyn ConnSecrets, http: &dyn ConnHttp, clock: &dyn NoticeClock, waiter: &dyn NoticeWaiter, tz: Tz, confirm: &BTreeSet<String>, budget: f64) -> TicketsResult {
    let mut book = store.read();
    let start = clock.now();
    // 앞 만들기가 끝나지 못하고 죽었다 — 확인 필요로(사람이 Jira 에서 생겼는지 본 뒤 그래도 만들기)
    let creating: Vec<String> = book.entries.iter().filter(|(_, v)| v.state == TicketState::Creating).map(|(k, _)| k.clone()).collect();
    for k in &creating {
        let rec = book.entries.get_mut(k).unwrap();
        rec.state = TicketState::Uncertain;
        rec.last = Some(Attempt {
            at: start,
            what: "uncertain".to_string(),
            code: 0,
            note: "앞 만들기가 끝나지 못했습니다. Jira 에 이미 생겼는지 본 뒤 그래도 만들기를 누릅니다".to_string(),
        });
    }
    if !creating.is_empty() {
        store.write(&book);
    }
    let todo = tickets_left(p, &book);
    if todo.is_empty() {
        return TicketsResult { book, note: "만들 티켓이 모두 있습니다(요청 0)".to_string(), left: 0, ..TicketsResult::default() };
    }
    // Swift 의 중첩 함수들(send·mark·finish)을 MakeRun 으로 옮긴다 — base·header 는 아래 검사 뒤에 채운다
    let mut run = MakeRun {
        p,
        store,
        http,
        waiter,
        clock,
        base: String::new(),
        header: None,
        book,
        start,
        budget,
        requests: 0,
        last_wait: None,
        made: Vec::new(),
        failed: Vec::new(),
    };
    let conn = connections.iter().find(|c| c.id == "atlassian");
    if let Some(why) = ticket_blocker(conn, conn.map(|c| secrets.exists(&c.id)).unwrap_or(false), dev, clock.now(), tz) {
        return run.finish(&format!("{why}. 요청하지 않았습니다"));
    }
    let c = conn.unwrap(); // ticket_blocker 가 통과했으므로 연결이 있다
    let Some(base) = service_base(c, dev) else {
        return run.finish("Atlassian 사이트 주소가 규칙에 맞지 않습니다. 요청하지 않았습니다");
    };
    if !run.book.site.is_empty() && run.book.site != base && run.book.entries.values().any(|r| r.state == TicketState::Made) {
        let note = format!("이 계획을 만들기 시작한 사이트({})와 지금 연결의 사이트({})가 달라 요청하지 않았습니다", run.book.site, base);
        return run.finish(&note);
    }
    let held = todo.iter().filter(|e| run.book.entries.get(&e.id).map(|r| r.state == TicketState::Uncertain).unwrap_or(false) && !confirm.contains(&e.id)).count();
    if held > 0 {
        return run.finish(&format!("확인 필요 {held}개: 앞 만들기가 끝났는지 모릅니다. Jira 에서 확인한 뒤 그래도 만들기를 누릅니다(요청 0)"));
    }
    let Some(token) = secrets.read(&c.id).filter(|t| !t.is_empty()) else {
        return run.finish("Atlassian 토큰이 키체인에 없습니다. 요청하지 않았습니다");
    };
    let auth = auth_for(c);
    run.base = base.clone();
    run.header = auth_header(&auth, &token);
    run.book.site = base.clone();
    // 이슈 종류는 만들 것이 있을 때 한 번 읽는다(읽기 요청)
    let story_type: Option<IssueType>;
    let task_type: Option<IssueType>;
    if todo.iter().any(|e| e.kind != TicketKind::Move) {
        let resp = run.send(&jira_project_request(&p.project(), &base, auth.clone()));
        let types = jira_issue_types(&resp);
        if types.is_empty() {
            let why = if resp.status == 200 {
                "프로젝트의 이슈 종류를 읽지 못했습니다".to_string()
            } else {
                format!("프로젝트 {} 를 읽지 못했습니다: {}", p.project(), ticket_fail_note(&resp))
            };
            for e in todo.iter().filter(|e| e.kind != TicketKind::Move).map(|e| e.id.clone()).collect::<Vec<String>>() {
                run.mark(&e, TicketState::Failed, "", resp.status, &why);
                run.failed.push(e);
            }
            return run.finish(&why);
        }
        story_type = pick_issue_type(&types, TicketKind::Story, &p.story_type);
        task_type = pick_issue_type(&types, TicketKind::Task, &p.task_type);
    } else {
        story_type = None;
        task_type = None;
    }
    for kind in [TicketKind::Story, TicketKind::Task, TicketKind::Move] {
        for e in todo.iter().filter(|e| e.kind == kind) {
            if clock.now() >= run.start + run.budget {
                let left = tickets_left(p, &run.book).len();
                let note = format!("시간 상한({}초)이 지나 남은 {}개는 다시 누를 때 만듭니다", budget as i64, left);
                return run.finish(&note);
            }
            let Some(parent) = run.parent_key(e) else {
                run.mark(&e.id, TicketState::Waiting, "", 0, &format!("스토리({})를 먼저 만들어야 합니다(요청 0)", e.parent));
                continue;
            };
            let ty = if kind == TicketKind::Story { &story_type } else { &task_type };
            if kind != TicketKind::Move && ty.is_none() {
                let why = if kind == TicketKind::Story {
                    format!("프로젝트 {} 에 스토리로 쓸 이슈 종류가 없습니다(프로젝트 설정 tickets.issue_types.story 로 이름을 정합니다)", p.project())
                } else {
                    format!("프로젝트 {} 에 하위 작업 종류가 없습니다(프로젝트 설정 tickets.issue_types.task 로 이름을 정합니다)", p.project())
                };
                run.mark(&e.id, TicketState::Failed, "", 0, &why);
                run.failed.push(e.id.clone());
                continue;
            }
            {
                let rec = run.book.entries.entry(e.id.clone()).or_default();
                rec.state = TicketState::Creating;
                rec.tries += 1;
                rec.updated = clock.now();
            }
            store.write(&run.book); // 보내기 전에 만드는 중으로(죽으면 다음에 잠금을 잡은 쪽이 확인 필요로 본다)
            run.last_wait = None;
            if kind == TicketKind::Move {
                let resp = run.send(&jira_parent_request(&e.key, &parent, &base, auth.clone()));
                if (200..=299).contains(&resp.status) && resp.error.is_none() {
                    run.mark(&e.id, TicketState::Made, &e.key, resp.status, &format!("부모를 {parent} 로 바꿨습니다"));
                    run.made.push(e.id.clone());
                } else {
                    let tail = if run.last_wait.is_some() { " 다시 누르면 남은 것만 만듭니다" } else { "" };
                    let note = format!("부모 바꾸기: {}{tail}", ticket_fail_note(&resp));
                    run.mark(&e.id, TicketState::Failed, "", resp.status, &note);
                    run.failed.push(e.id.clone());
                }
                continue;
            }
            let resp = run.send(&jira_create_issue_request(&p.project(), &ty.as_ref().unwrap().id, &parent, &e.title, &ticket_doc(e, &p.run, "", "", ""), &base, auth.clone()));
            if let Some(key) = created_issue_key(&resp, &p.project()) {
                run.mark(&e.id, TicketState::Made, &key, resp.status, "만들었습니다");
                run.made.push(e.id.clone());
            } else {
                let why = if (200..=299).contains(&resp.status) {
                    format!("응답에 만든 키가 없습니다({}). Jira 에 생겼는지 확인합니다", resp.status)
                } else {
                    ticket_fail_note(&resp)
                };
                let st = if (200..=299).contains(&resp.status) { TicketState::Uncertain } else { TicketState::Failed };
                run.mark(&e.id, st, "", resp.status, &why);
                run.failed.push(e.id.clone());
            }
        }
    }
    let left = tickets_left(p, &run.book).len();
    let note = if left == 0 {
        format!("티켓 {}개를 만들었습니다", run.made.len())
    } else {
        format!("티켓 {}개를 만들었고 {left}개가 남았습니다. 다시 누르면 남은 것만 만듭니다", run.made.len())
    };
    run.finish(&note)
}

/// 올릴 글의 받을 곳 종류. 원본: `Outbox.swift` 의 `APTargetService`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetService {
    Jira,
    Confluence,
    Notion,
}

impl TargetService {
    /// Swift rawValue
    pub fn raw_value(&self) -> &'static str {
        match self {
            TargetService::Jira => "jira",
            TargetService::Confluence => "confluence",
            TargetService::Notion => "notion",
        }
    }

    /// Swift `APTargetService(rawValue:)`
    pub fn from_raw(s: &str) -> Option<TargetService> {
        match s {
            "jira" => Some(TargetService::Jira),
            "confluence" => Some(TargetService::Confluence),
            "notion" => Some(TargetService::Notion),
            _ => None,
        }
    }
}

/// 받을 곳 — Jira 는 이슈, Confluence 는 스페이스(와 상위 페이지, 없으면 스페이스 맨 위), Notion 은 상위 페이지. 원본: `Outbox.swift` 의 `APOutboxTarget`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutboxTarget {
    pub service: TargetService,
    pub issue: String,
    pub space: String,
    pub parent: String,
}

impl OutboxTarget {
    /// Swift 기본값(service 는 confluence)
    pub fn new(service: TargetService, issue: &str, space: &str, parent: &str) -> OutboxTarget {
        OutboxTarget { service, issue: issue.to_string(), space: space.to_string(), parent: parent.to_string() }
    }
}

/// 받은 곳의 이름. 원본: `Outbox.swift` 의 `APRemote`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remote {
    pub id: String,
    pub url: String,
}

/// 올릴 글 하나 — 엔진이 만들고 검수 화면·자동 올림이 보낸다. notify_rules 의 최소 이식(상태만)과 달리 온전한 꼴이며, Outbox 이식 뒤에는 notify_rules 축약형과 통합한다. 원본: `Outbox.swift` 의 `APOutboxItem`
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OutboxItem {
    // 정의 — 엔진이 만들 때 정하고 바꾸지 않는다
    pub id: String,
    pub run: String,
    pub unit: Option<i64>,
    /// report · comment · ticket
    pub kind: String,
    pub target: OutboxTarget,
    pub connection: String,
    pub title: String,
    pub doc: Doc,
    /// 실행 폴더 기준 파일 이름(report.html), Confluence 보고서만
    pub attachment: Option<String>,
    /// 만들 때의 게시 방식
    pub mode: PublishMode,
    pub created: f64,
    // 상태
    pub state: OutboxState,
    /// "나중에"를 누른 시각(목록에 남김, 자동으로 올리지 않음)
    pub later: Option<f64>,
    /// 올리기 시작한 사이트(받은 곳 id 가 이 사이트의 것)
    pub site: String,
    pub remote: Option<Remote>,
    /// 끝난 단계: comment · page · attachment · blocks:<보낸 블록 수>
    pub steps: Vec<String>,
    pub last: Option<Attempt>,
    pub retry_after: Option<f64>,
    pub tries: i32,
    pub updated: f64,
}

impl OutboxItem {
    /// Swift 기본값(kind 는 report, mode 는 review, state 는 waiting)
    fn default() -> Self {
        OutboxItem {
            id: String::new(),
            run: String::new(),
            unit: None,
            kind: "report".to_string(),
            target: OutboxTarget::default(),
            connection: String::new(),
            title: String::new(),
            doc: Doc::default(),
            attachment: None,
            mode: PublishMode::Review,
            created: 0.0,
            state: OutboxState::default(),
            later: None,
            site: String::new(),
            remote: None,
            steps: Vec::new(),
            last: None,
            retry_after: None,
            tries: 0,
            updated: 0.0,
        }
    }
}

/// 상태·방식의 raw 값 — notify_rules 의 두 낱말은 raw 문자열을 밖으로 주지 않아 여기서 원본 값과 맞춘다(Outbox 이식 뒤에는 그쪽으로 옮긴다)
fn outbox_state_raw(s: &OutboxState) -> &'static str {
    match s {
        OutboxState::Waiting => "waiting",
        OutboxState::Posting => "posting",
        OutboxState::Posted => "posted",
        OutboxState::Skipped => "skipped",
        OutboxState::Failed => "failed",
        OutboxState::Uncertain => "uncertain",
    }
}

fn outbox_state_from_raw(s: &str) -> Option<OutboxState> {
    match s {
        "waiting" => Some(OutboxState::Waiting),
        "posting" => Some(OutboxState::Posting),
        "posted" => Some(OutboxState::Posted),
        "skipped" => Some(OutboxState::Skipped),
        "failed" => Some(OutboxState::Failed),
        "uncertain" => Some(OutboxState::Uncertain),
        _ => None,
    }
}

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

impl Default for TargetService {
    /// Swift `APTargetService` 는 기본값이 없어 OutboxTarget 의 기본값(confluence)이 대신한다
    fn default() -> Self {
        TargetService::Confluence
    }
}

/// 원본: `Outbox.swift` 의 `apOutboxFormat`
pub const OUTBOX_FORMAT: &str = "claude-board-outbox";
/// 원본: `Outbox.swift` 의 `apOutboxMaxBytes`
pub const OUTBOX_MAX_BYTES: usize = 2 << 20;

/// 글 id — 파일 이름이 되므로 좁게(엔진이 만든 id 만 이 꼴). 꼬리는 이슈 키(u3-jira-SHOP-13) 또는 임시 티켓(u3-ticket-TMP-3-1).
/// `^(run|u[0-9]{1,4})-[a-z]{1,12}(-[A-Z][A-Z0-9_]*-[0-9]{1,9}(-[0-9]{1,2})?)?$` 를 판별자로 옮긴 것. 원본: `Outbox.swift` 의 `apOutboxIDOK`
pub fn outbox_id_ok(id: &str) -> bool {
    let mut segs = id.split('-');
    let head = match segs.next() {
        Some(h) => h,
        None => return false,
    };
    let body = match segs.next() {
        Some(b) => b,
        None => return false,
    };
    let u_tail = head.strip_prefix('u');
    let head_ok = head == "run"
        || match u_tail {
            Some(d) => (1..=4).contains(&d.len()) && d.chars().all(|c| c.is_ascii_digit()),
            None => false,
        };
    let body_ok = (1..=12).contains(&body.len()) && body.chars().all(|c| c.is_ascii_lowercase());
    if !head_ok || !body_ok {
        return false;
    }
    match (segs.next(), segs.next(), segs.next(), segs.next()) {
        (None, None, None, None) => true,
        (Some(project), Some(number), None, None) => outbox_id_key_ok(project, number, None),
        (Some(project), Some(number), Some(step), None) if (1..=2).contains(&step.len()) && step.chars().all(|c| c.is_ascii_digit()) => {
            outbox_id_key_ok(project, number, Some(step))
        }
        _ => false,
    }
}

/// 꼬리의 `-<프로젝트>-<번호>(-<step>)` 네 조각. `step` 은 None 이거나 정한 두 조각 뒤의 1~2 자리
fn outbox_id_key_ok(project: &str, number: &str, step: Option<&str>) -> bool {
    let mut cs = project.chars();
    let first_ok = matches!(cs.next(), Some(c) if c.is_ascii_uppercase());
    first_ok
        && cs.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && (1..=9).contains(&number.len())
        && number.chars().all(|c| c.is_ascii_digit())
        && step.is_none_or(|s| (1..=2).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit()))
}

/// 원본: `Outbox.swift` 의 `apOutboxJSON`
pub fn outbox_json(it: &OutboxItem) -> Value {
    let mut t = Map::new();
    t.insert("service".to_string(), json!(it.target.service.raw_value()));
    if !it.target.issue.is_empty() {
        t.insert("issue".to_string(), json!(it.target.issue));
    }
    if !it.target.space.is_empty() {
        t.insert("space".to_string(), json!(it.target.space));
    }
    if !it.target.parent.is_empty() {
        t.insert("parent".to_string(), json!(it.target.parent));
    }
    let mut d = Map::new();
    d.insert("format".to_string(), json!(OUTBOX_FORMAT));
    d.insert("version".to_string(), json!(1));
    d.insert("id".to_string(), json!(it.id));
    d.insert("run".to_string(), json!(it.run));
    d.insert("kind".to_string(), json!(it.kind));
    d.insert("target".to_string(), Value::Object(t));
    d.insert("connection".to_string(), json!(it.connection));
    d.insert("title".to_string(), json!(it.title));
    d.insert("doc".to_string(), json!(doc_json(&it.doc)));
    d.insert("mode".to_string(), json!(publish_mode_raw(&it.mode)));
    d.insert("created".to_string(), json!(it.created));
    d.insert("state".to_string(), json!(outbox_state_raw(&it.state)));
    if let Some(u) = it.unit {
        d.insert("unit".to_string(), json!(u));
    }
    if let Some(a) = &it.attachment {
        d.insert("attachment".to_string(), json!(a));
    }
    if let Some(l) = it.later {
        d.insert("later".to_string(), json!(l));
    }
    if !it.site.is_empty() {
        d.insert("site".to_string(), json!(it.site));
    }
    if let Some(r) = &it.remote {
        d.insert("remote".to_string(), json!({"id": r.id, "url": r.url}));
    }
    d.insert("steps".to_string(), json!(it.steps));
    if let Some(a) = &it.last {
        d.insert("last".to_string(), json!({"at": a.at, "what": a.what, "code": a.code, "note": a.note}));
    }
    if let Some(r) = it.retry_after {
        d.insert("retry_after".to_string(), json!(r));
    }
    d.insert("tries".to_string(), json!(it.tries));
    d.insert("updated".to_string(), json!(it.updated));
    Value::Object(d)
}

/// 원본: `Outbox.swift` 의 `apOutboxData`(prettyPrinted · sortedKeys · withoutEscapingSlashes)
pub fn outbox_data(it: &OutboxItem) -> Vec<u8> {
    serde_json::to_vec_pretty(&sort_keys(&outbox_json(it))).unwrap_or_default()
}

/// Swift `NSNumber` 수 가운데 Bool 이 아닌 것만 — `Outbox.swift` 의 비공개 `dbl`(Bool 은 받지 않는다)
fn outbox_num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// Swift `(v as? NSNumber)?.intValue` — Bool 도 NSNumber 로 1·0 이 된다
fn outbox_int(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Bool(b)) => Some(i64::from(*b)),
        Some(Value::Number(n)) => n.as_i64(),
        _ => None,
    }
}

/// 파일 → 글. 꼴이 틀리면 None(보드 검수 화면은 그 파일을 넘긴다). 원본: `Outbox.swift` 의 `apOutboxParse`
pub fn outbox_parse(data: &[u8]) -> Option<OutboxItem> {
    if data.len() > OUTBOX_MAX_BYTES {
        return None;
    }
    let o = match serde_json::from_slice::<Value>(data) {
        Ok(Value::Object(o)) => o,
        _ => return None,
    };
    if o.get("format").and_then(Value::as_str) != Some(OUTBOX_FORMAT) {
        return None;
    }
    let id = o.get("id").and_then(Value::as_str)?;
    if !outbox_id_ok(id) {
        return None;
    }
    let tg = o.get("target")?.as_object()?;
    let svc = TargetService::from_raw(tg.get("service").and_then(Value::as_str).unwrap_or(""))?;
    let st = outbox_state_from_raw(o.get("state").and_then(Value::as_str).unwrap_or(""))?;
    let mut it = OutboxItem::default();
    it.id = id.to_string();
    it.run = o.get("run").and_then(Value::as_str).unwrap_or("").to_string();
    it.unit = outbox_int(o.get("unit"));
    let kind = o.get("kind").and_then(Value::as_str).unwrap_or("");
    it.kind = if kind == "comment" || kind == "ticket" { kind.to_string() } else { "report".to_string() };
    if it.kind == "ticket" {
        let issue = tg.get("issue").and_then(Value::as_str).unwrap_or("");
        let parent = tg.get("parent").and_then(Value::as_str).unwrap_or("");
        if svc != TargetService::Jira || !temp_ticket_ok(issue) || !issue_key_ok(parent) {
            return None;
        }
    }
    it.target = OutboxTarget {
        service: svc,
        issue: tg.get("issue").and_then(Value::as_str).unwrap_or("").to_string(),
        space: tg.get("space").and_then(Value::as_str).unwrap_or("").to_string(),
        parent: tg.get("parent").and_then(Value::as_str).unwrap_or("").to_string(),
    };
    it.connection = o.get("connection").and_then(Value::as_str).unwrap_or("").to_string();
    it.title = doc_text_n(o.get("title").and_then(Value::as_str).unwrap_or(""), 300, true);
    it.doc = doc_parse(o.get("doc"));
    it.attachment = o.get("attachment").and_then(Value::as_str).filter(|a| *a == "report.html").map(str::to_string);
    it.mode = publish_mode_from_raw(o.get("mode").and_then(Value::as_str).unwrap_or("")).unwrap_or(PublishMode::Review);
    it.created = outbox_num(o.get("created")).unwrap_or(0.0);
    it.state = st;
    it.later = outbox_num(o.get("later"));
    it.site = o.get("site").and_then(Value::as_str).unwrap_or("").to_string();
    if let Some(Value::Object(r)) = o.get("remote") {
        it.remote = Some(Remote {
            id: r.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            url: r.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
        });
    }
    it.steps = o.get("steps").and_then(Value::as_array).unwrap_or(&Vec::new()).iter().filter_map(Value::as_str).map(str::to_string).collect();
    if let Some(Value::Object(a)) = o.get("last") {
        it.last = Some(Attempt {
            at: outbox_num(a.get("at")).unwrap_or(0.0),
            what: a.get("what").and_then(Value::as_str).unwrap_or("").to_string(),
            code: outbox_int(a.get("code")).unwrap_or(0) as i32,
            note: doc_text_n(a.get("note").and_then(Value::as_str).unwrap_or(""), 500, true),
        });
    }
    it.retry_after = outbox_num(o.get("retry_after"));
    it.tries = outbox_int(o.get("tries")).unwrap_or(0) as i32;
    it.updated = outbox_num(o.get("updated")).unwrap_or(0.0);
    Some(it)
}

/// 받을 곳을 쓰는 연결과 그 범위. 원본: `Outbox.swift` 의 `apTargetConnection`
pub fn target_connection(s: TargetService) -> (&'static str, &'static str) {
    match s {
        TargetService::Jira => ("atlassian", "jira"),
        TargetService::Confluence => ("atlassian", "confluence"),
        TargetService::Notion => ("notion", "write"),
    }
}

/// 글 하나의 받을 곳 줄 — 티켓 만들기(실행 중에 새로 필요해진 티켓)는 그 꼴로, 나머지는 `target_label`. 원본: `Outbox.swift` 의 `apItemLabel`
pub fn item_label(it: &OutboxItem) -> String {
    if it.kind == "ticket" {
        return format!("Jira 티켓 만들기 · 임시 번호 {} → {} 아래 하위 작업", it.target.issue, it.target.parent);
    }
    target_label(&it.target)
}

/// 원본: `Outbox.swift` 의 `apTargetLabel`
pub fn target_label(t: &OutboxTarget) -> String {
    match t.service {
        TargetService::Jira => format!("Jira 댓글 · {}", t.issue),
        TargetService::Confluence => format!(
            "Confluence · 스페이스 {}{} › 새 페이지",
            t.space,
            if t.parent.is_empty() { " 맨 위".to_string() } else { format!(" › 상위 페이지 {}", t.parent) }
        ),
        TargetService::Notion => format!("Notion · 상위 페이지 {}… › 새 페이지", take(&t.parent, 8)),
    }
}

/// 출발 카드 id — 답 temp(임시 번호로 진행, 추천)·defer(보류하고 다음 출발로). 원본: `Outbox.swift` 쪽 `apNewTicketCard`
pub const NEW_TICKET_CARD: &str = "jira-new-ticket";

/// 단위 판정 뒤 만들 올릴 글 — 결과 기록의 new_tickets 가운데 이 단위의 임시 번호 꼴인 것만, 그 스토리 아래 하위 작업으로.
/// 게시 방식과 상관없이 늘 검수 뒤 올림(실행 중 티켓 만들기 요청 0). 카드 답이 temp 가 아니면 만들지 않는다. 원본: `apNewTicketOutbox`
pub fn new_ticket_outbox(r: &ReportRun, unit: i64, story: Option<&str>, task: Option<&str>, answer: Option<&str>, now: f64) -> Vec<OutboxItem> {
    if answer != Some("temp") {
        return Vec::new();
    }
    let Some(st) = story else { return Vec::new() };
    if !issue_key_ok(st) {
        return Vec::new();
    }
    let Some(u) = r.units.iter().find(|u| u.n == unit) else { return Vec::new() };
    let Some(res) = &u.result else { return Vec::new() };
    if !["done", "failed", "capped"].contains(&u.status.as_str()) {
        return Vec::new();
    }
    let project = st.split('-').find(|x| !x.is_empty()).unwrap_or("");
    let mut out: Vec<OutboxItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for t in res.new_tickets.iter().take(20) {
        if !temp_ticket_ok(&t.temp) || !t.temp.starts_with(&format!("TMP-{unit}-")) || !seen.insert(t.temp.clone()) {
            continue;
        }
        let title = notify_clean_n(&t.title, TICKET_TITLE_MAX);
        if title.is_empty() {
            continue;
        }
        let mut it = OutboxItem::default();
        it.id = format!("u{unit}-ticket-{}", t.temp);
        it.run = r.id.clone();
        it.unit = Some(unit);
        it.kind = "ticket".to_string();
        it.target = OutboxTarget::new(TargetService::Jira, &t.temp, project, st);
        it.connection = "atlassian".to_string();
        it.title = title.clone();
        let e = TicketEntry {
            id: format!("task:{}", t.temp),
            kind: TicketKind::Task,
            title: title.clone(),
            parent: st.to_string(),
            key: String::new(),
            units: vec![notify_clean_n(&task_title_fallback(u), 200)],
        };
        it.doc = ticket_doc(&e, &r.id, &t.temp, &notify_clean_n(&t.why, 500), task.unwrap_or(""));
        it.mode = PublishMode::Review; // 늘 검수 뒤
        it.created = now;
        it.updated = now;
        out.push(it);
    }
    out
}

/// 단위 제목 비었는지의 원본 식(`u.title.isEmpty ? "단위 \(n)" : u.title`)을 그대로 — ReportUnit 은 기본 제목이 없어 여기서 맞춘다
fn task_title_fallback(u: &ReportUnit) -> String {
    if u.title.is_empty() {
        format!("단위 {}", u.n)
    } else {
        u.title.clone()
    }
}

/// 만든 티켓 한 줄 — 키·종류(story·task)·부모 키·제목·주소·원천 키·만든 실행·만든 때. 같은 키는 다시 쓰지 않는다(엔진 ON CONFLICT DO NOTHING). 원본: `APTicketRow`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TicketRow {
    pub key: String,
    pub kind: String,
    pub parent: String,
    pub title: String,
    pub url: String,
    pub source_key: Option<String>,
    pub run: String,
    pub created: f64,
}

/// 출발 확인의 티켓 나무(tickets-plan.json + tickets.json)에서 만든 것(made)만. 부모 바꾸기(move)는 만든 것이 아니라 넣지 않는다.
/// 작업의 원천 키는 그 단위(task:<단위 id>)의 source_key, 스토리는 없음. 부모가 같은 계획의 스토리(story:…)면 그 스토리의 만든 키. 원본: `apTicketRows`
pub fn ticket_rows(plan: &TicketPlan, book: &TicketBook, unit_sources: &BTreeMap<String, String>, run: &str) -> Vec<TicketRow> {
    let mut out: Vec<TicketRow> = Vec::new();
    for e in plan.entries.iter().filter(|e| e.kind != TicketKind::Move) {
        let Some(r) = book.entries.get(&e.id) else { continue };
        if r.state != TicketState::Made || !issue_key_ok(&r.key) || out.iter().any(|w| w.key == r.key) {
            continue;
        }
        let parent = if e.parent.starts_with("story:") {
            let Some(p) = book.entries.get(&e.parent) else { continue };
            if p.state != TicketState::Made || !issue_key_ok(&p.key) {
                continue;
            }
            p.key.clone()
        } else if issue_key_ok(&e.parent) {
            e.parent.clone()
        } else {
            String::new()
        };
        let src = if e.kind == TicketKind::Task && e.id.starts_with("task:") {
            unit_sources.get(&e.id[5..]).cloned()
        } else {
            None
        };
        out.push(TicketRow {
            key: r.key.clone(),
            kind: e.kind.raw_value().to_string(),
            parent,
            title: take(&e.title, TICKET_TITLE_MAX),
            url: r.url.clone(),
            source_key: src,
            run: run.to_string(),
            created: r.updated,
        });
    }
    out
}

/// 실행 중에 새로 필요해진 티켓(올릴 글 kind ticket)에서 올린 것(posted, 받은 곳 id 가 이슈 키)만. 종류는 하위 작업(task), 원천 키는 임시 번호(TMP-<단위>-<차례>). 원본: `apOutboxTicketRows`
pub fn outbox_ticket_rows(items: &[OutboxItem], run: &str) -> Vec<TicketRow> {
    let mut out: Vec<TicketRow> = Vec::new();
    for it in items.iter().filter(|it| it.kind == "ticket" && it.state == OutboxState::Posted) {
        let Some(rm) = &it.remote else { continue };
        if !issue_key_ok(&rm.id) || out.iter().any(|w| w.key == rm.id) {
            continue;
        }
        out.push(TicketRow {
            key: rm.id.clone(),
            kind: "task".to_string(),
            parent: if issue_key_ok(&it.target.parent) { it.target.parent.clone() } else { String::new() },
            title: take(&it.title, TICKET_TITLE_MAX),
            url: rm.url.clone(),
            source_key: if temp_ticket_ok(&it.target.issue) { Some(it.target.issue.clone()) } else { None },
            run: run.to_string(),
            created: it.updated,
        });
    }
    out
}

// MARK: - 올릴지 판정

/// 올림에 누가 부쳤는지. 원본: `Outbox.swift` 의 `APPostTrigger`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostTrigger {
    /// 엔진이 스스로(자동으로 올림)
    Auto,
    /// 사람이 검수 화면에서 누름
    Press,
}

/// 올릴지 판정. 원본: `Outbox.swift` 의 `APPostDecision`
#[derive(Debug, Clone, PartialEq)]
pub enum PostDecision {
    /// 보낸다(그 바탕 주소로만)
    Go(String),
    /// 요청 없음, 상태 그대로
    Nothing(String),
    /// 요청 없음, 마지막 시도에 "held" 와 까닭을 남김
    Hold(String),
}

/// 글 하나를 올릴지 판정 — 요청은 Go 만 내고, 나머지는 까닭과 함께 상태만 바꾼다. 원본: `apPostDecide`
#[allow(clippy::too_many_arguments)]
pub fn post_decide(it: &OutboxItem, trigger: PostTrigger, connection: Option<&Connection>, has_secret: bool, dev: bool, now: f64, tz: &Tz) -> PostDecision {
    match it.state {
        OutboxState::Posted => return PostDecision::Nothing("이미 올렸습니다".to_string()),
        OutboxState::Posting => return PostDecision::Nothing("올리는 중입니다".to_string()),
        OutboxState::Skipped if trigger == PostTrigger::Auto => return PostDecision::Nothing("올리지 않기로 했습니다(로컬에만)".to_string()),
        OutboxState::Uncertain if trigger == PostTrigger::Auto => return PostDecision::Nothing("앞 올리기가 끝났는지 몰라 사람이 확인한 뒤에만 올립니다".to_string()),
        _ => {}
    }
    if trigger == PostTrigger::Auto {
        if it.mode == PublishMode::Review {
            return PostDecision::Nothing("검수 뒤 올림: 사람이 올리기를 눌러야 올라갑니다".to_string());
        }
        if it.state == OutboxState::Failed && it.retry_after.is_none() {
            return PostDecision::Nothing("올리지 못한 까닭을 사람이 본 뒤에 다시 올립니다".to_string());
        }
        if let Some(retry_after) = it.retry_after {
            if retry_after > now {
                return PostDecision::Nothing("받는 쪽이 잠시 뒤에 보내라고 해 기다립니다".to_string());
            }
        }
    }
    let need = target_connection(it.target.service);
    let Some(c) = connection else { return PostDecision::Hold("연결이 없어 요청하지 않았습니다. 글은 로컬에 남습니다".to_string()) };
    if c.id != it.connection || c.id != need.0 {
        return PostDecision::Hold("연결이 없어 요청하지 않았습니다. 글은 로컬에 남습니다".to_string());
    }
    if !c.scopes.contains(&need.1.to_string()) {
        return PostDecision::Hold(format!("연결의 범위에 '{}' 가 없어 요청하지 않았습니다", need.1));
    }
    let Some(base) = service_base(c, dev) else { return PostDecision::Hold("사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다".to_string()) };
    if !it.site.is_empty() && it.site != base && (it.remote.is_some() || !it.steps.is_empty()) {
        return PostDecision::Hold(format!("이 글을 올리기 시작한 사이트({})와 지금 연결의 사이트({})가 달라 요청하지 않았습니다", it.site, base));
    }
    if c.service == Service::Atlassian && c.account.is_empty() {
        return PostDecision::Hold("Atlassian 계정 메일이 없어 요청하지 않았습니다".to_string());
    }
    if !has_secret {
        return PostDecision::Hold("토큰이 키체인에 없어 요청하지 않았습니다".to_string());
    }
    if matches!(ap_expiry(&c.expires, now, *tz), Expiry::Expired(_)) {
        return PostDecision::Hold(format!("토큰 만료일({})이 지나 요청하지 않았습니다. 새 토큰을 넣고 올립니다", c.expires));
    }
    PostDecision::Go(base)
}

// MARK: - 다시 보내기

/// 댓글 바디 — 문서를 아타시안 문서 꼴로. 원본: `Outbox.swift` 의 `apJiraCommentRequest`
pub fn jira_comment_request(it: &OutboxItem, base: &str, auth: Auth) -> HttpRequest {
    HttpRequest {
        method: "POST".to_string(),
        url: format!("{base}/rest/api/3/issue/{}/comment", it.target.issue),
        headers: json_headers(),
        body: Some(body(&serde_json::json!({ "body": adf(&it.doc) }))),
        auth,
    }
}

/// 받을 곳이 Confluence 일 때 — 상위 페이지가 있으면 그 아래로. 원본: `apConfluencePageRequest`
pub fn confluence_page_request(it: &OutboxItem, base: &str, auth: Auth) -> HttpRequest {
    let mut o = serde_json::json!({
        "type": "page",
        "title": it.title,
        "space": { "key": it.target.space },
        "body": { "storage": { "value": storage(&it.doc), "representation": "storage" } },
    });
    if !it.target.parent.is_empty() {
        o["ancestors"] = serde_json::json!([{ "id": it.target.parent }]);
    }
    HttpRequest { method: "POST".to_string(), url: format!("{base}/wiki/rest/api/content"), headers: json_headers(), body: Some(body(&o)), auth }
}

// 첨부 — v1 API 의 "만들거나 바꾸기"(PUT): 같은 이름이 이미 있으면 새 판으로 올라가 다시 눌러도 첨부가 둘이 되지 않는다
pub fn confluence_attachment_request(it: &OutboxItem, base: &str, page: &str, file: &[u8], auth: Auth) -> HttpRequest {
    let boundary = format!(
        "claude-board-{}",
        it.id.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect::<String>()
    );
    let mut d = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: text/html; charset=utf-8\r\n\r\n",
        it.attachment.as_deref().unwrap_or("report.html")
    )
    .into_bytes();
    d.extend_from_slice(file);
    d.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"minorEdit\"\r\n\r\ntrue\r\n--{boundary}--\r\n").as_bytes());
    HttpRequest {
        method: "PUT".to_string(),
        url: format!("{base}/wiki/rest/api/content/{page}/child/attachment"),
        headers: BTreeMap::from([
            ("Content-Type".to_string(), format!("multipart/form-data; boundary={boundary}")),
            ("X-Atlassian-Token".to_string(), "no-check".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ]),
        body: Some(d),
        auth,
    }
}

// MARK: - 받은 곳 id 와 주소

// MARK: - 올리기 유스케이스

/// 올림 결과 — item 은 바뀐 글(실패·보류·다른 곳일 때도 현재 상태로. 글이 없으면 nil). 원본: `Outbox.swift` 의 `APPostResult`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PostResult {
    pub item: Option<OutboxItem>,
    /// 이번에 보낸 요청 수(다시 보낸 것 포함)
    pub requests: i64,
    pub note: String,
    /// 잠금을 잡지 못함(다른 곳에서 올리는 중)
    pub busy: bool,
}

/// 올림 글 저장소 — 만들기·읽기·쓰기·긴 잠금·첨부 파일. `ids()`(포트 `APOutboxStore`)는 쓰는 곳이 없어 두지 않았다. 원본: `Outbox.swift` 의 `APOutboxStore`
pub trait OutboxStore {
    /// 같은 id 가 없을 때만 넣는다
    fn create(&self, it: &OutboxItem) -> bool;
    fn read(&self, id: &str) -> Option<OutboxItem>;
    /// 잠금 안에서의 바꾸기 — `f` 는 이미 텍스처로 온전히 보존돼 한 번만 부른다
    fn write(&self, it: &OutboxItem);
    fn with_lock(&self, id: &str, f: &mut dyn FnMut() -> PostResult) -> Option<PostResult>;
    /// 첨부 파일(실행 폴더의 첨부)
    fn attachment(&self, name: &str) -> Option<Vec<u8>>;
}

/// 글 하나를 올리는 흐름의 임시 상태 — Store 구조체 대신 도우미로 문서를 만들고 쓴다. 원본 `apPostItem` 의 지역 함수들(send·fail·done)
struct PostRun<'a> {
    http: &'a dyn ConnHttp,
    waiter: &'a dyn NoticeWaiter,
    clock: &'a dyn NoticeClock,
    base: String,
    header: Option<AuthHeader>,
    start: f64,
    budget: f64,
    requests: i64,
    last_wait: Option<f64>,
}

impl PostRun<'_> {
    /// 요청 하나 보내기 — 사이트 밖 주소는 보내지 않고, 429 는 budget 안에서만 다시 보낸다
    fn send(&mut self, req: &HttpRequest) -> HttpResponse {
        if !same_site(&req.url, &self.base) {
            return HttpResponse { error: Some("연결한 사이트 밖 주소라 보내지 않았습니다".to_string()), ..HttpResponse::default() };
        }
        let mut attempt: i32 = 0;
        loop {
            self.requests += 1;
            let resp = self.http.send(req, self.header.as_ref());
            let Some(w) = retry_wait(resp.status, resp.headers.get("retry-after").map(String::as_str), attempt, self.clock.now()) else { return resp };
            if self.clock.now() + w > self.start + self.budget {
                self.last_wait = Some(w);
                return resp;
            }
            self.waiter.wait(w);
            attempt += 1;
        }
    }
}

/// 실패로 마친다 — 잠깐의 실패(429·5xx·응답 없음)만 자동으로 다시 보낼 때를 남기고, 사람이 보게 기록을 남긴다. `apPostItem` 의 `fail`
fn fail_post(it: &mut OutboxItem, store: &dyn OutboxStore, run: &PostRun, resp: HttpResponse, step: &str) -> PostResult {
    let t = run.clock.now();
    it.state = OutboxState::Failed;
    if transient(&resp) {
        it.retry_after = Some(t + run.last_wait.unwrap_or(60.0));
    }
    let note = format!("{step}: {}", fail_note(&resp));
    it.last = Some(Attempt { at: t, what: "failed".to_string(), code: resp.status, note });
    it.updated = t;
    store.write(it);
    PostResult {
        item: Some(it.clone()),
        requests: run.requests,
        note: it.last.as_ref().map(|l| l.note.clone()).unwrap_or_default(),
        busy: false,
    }
}

/// 단계 하나를 다 쳤다고 남긴다(받은 곳·블록 묶음·첨부). blocks: 단계는 새 번호로 곂쳐 쓴다. `apPostItem` 의 `done`
fn done_post(it: &mut OutboxItem, store: &dyn OutboxStore, run: &PostRun, step: &str, remote: Option<Remote>) {
    if let Some(remote) = remote {
        it.remote = Some(remote);
    }
    if step.starts_with("blocks:") {
        it.steps.retain(|s| !s.starts_with("blocks:"));
    }
    if !it.steps.iter().any(|s| s == step) {
        it.steps.push(step.to_string());
    }
    it.updated = run.clock.now();
    store.write(it);
}

/// 받은 곳 id — 응답 본문(믿지 않는 값)의 id 칸이 정한 꼴일 때만. Confluence·Jira 는 숫자(20자리까지), Notion 은 32자리 16진수(소문자로). 원본: `apCreatedID`
pub fn created_id(s: TargetService, r: &HttpResponse) -> Option<String> {
    if r.error.is_some() || !(200..=299).contains(&r.status) {
        return None;
    }
    let bytes = &r.body;
    if bytes.len() > 4 << 20 {
        return None;
    }
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let id = v.get("id")?.as_str()?;
    match s {
        TargetService::Jira | TargetService::Confluence => confluence_id_key_ok(id).then(|| id.to_string()),
        TargetService::Notion => notion_id_key_ok(id).then(|| id.to_lowercase()),
    }
}

/// Confluence·Jira id 꼴 — 숫자만, 1 자리 이상 20 자리 이하. 원본: `apMatch(id, apConfluenceIDPattern)`
fn confluence_id_key_ok(id: &str) -> bool {
    (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// Notion id 꼴 — 8-4-4-4-12 자리 16진수, 붙임표(-)는 그 네 경계마다 있어도 없어도. 원본: `apNotionIDPattern`(Outbox.swift 41 줄:
/// `^[0-9a-fA-F]{8}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{12}$`)
fn notion_id_key_ok(id: &str) -> bool {
    /// 붙임표가 허용되는 자리(붙임표를 지운 꼴에서의 글자 수) — 여덟·열두·열여섯·스물
    fn boundary_ok(seen: usize) -> bool {
        matches!(seen, 8 | 12 | 16 | 20)
    }
    let hex = id.replace('-', "");
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let mut seen = 0usize;
    let mut prev_dash = false;
    for c in id.bytes() {
        if c == b'-' {
            // 붙임표는 경계마다 하나까지 — `-?` 이므로 연달아 나오거나 경계 밖이면 틀린 꼴이다
            if prev_dash || !boundary_ok(seen) {
                return false;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
            seen += 1;
        }
    }
    true
}

/// 받은 곳 주소 — 응답의 주소 칸은 믿지 않고 사이트와 id 로 만든다. Notion 은 정한 API 주소면 notion.so, 시험 빌드의 가짜 서버면 그 서버 아래. 원본: `apRemoteURL`
pub fn remote_url(it: &OutboxItem, base: &str, id: &str) -> String {
    match it.target.service {
        TargetService::Jira => format!("{base}/browse/{}?focusedCommentId={id}", it.target.issue),
        TargetService::Confluence => format!("{base}/wiki/pages/viewpage.action?pageId={id}"),
        TargetService::Notion => {
            let hex = id.replace('-', "");
            if base == NOTION_API {
                format!("https://www.notion.so/{hex}")
            } else {
                format!("{base}/p/{hex}")
            }
        }
    }
}

/// Notion 페이지 만들기 — 상위 페이지 아위로 첫 묶음 블록과 함께. 원본: `apNotionPageRequest`
pub fn notion_page_request(it: &OutboxItem, base: &str, children: &[Value]) -> HttpRequest {
    let o = serde_json::json!({
        "parent": { "page_id": it.target.parent },
        "properties": { "title": { "title": [{ "type": "text", "text": { "content": it.title } }] } },
        "children": children,
    });
    HttpRequest {
        method: "POST".to_string(),
        url: format!("{base}/v1/pages"),
        headers: BTreeMap::from([
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
            ("Notion-Version".to_string(), NOTION_VERSION.to_string()),
        ]),
        body: Some(body(&o)),
        auth: Auth::Bearer,
    }
}

/// Notion 덧붙이기 — 만든 페이지 아위에 남은 묶음 블록을 차례로. 원본: `apNotionAppendRequest`
pub fn notion_append_request(base: &str, page: &str, children: &[Value]) -> HttpRequest {
    HttpRequest {
        method: "PATCH".to_string(),
        url: format!("{base}/v1/blocks/{page}/children"),
        headers: BTreeMap::from([
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
            ("Notion-Version".to_string(), NOTION_VERSION.to_string()),
        ]),
        body: Some(body(&serde_json::json!({ "children": children }))),
        auth: Auth::Bearer,
    }
}

/// 글 하나를 올린다(잠금 안). budget 은 429 기다림까지 합친 초 상한 — 넘으면 "올리지 못함"과 다시 보낼 때(retry_after)를 남기고 끝낸다. 원본: `apPostItem`
#[allow(clippy::too_many_arguments)]
pub fn post_item(
    id: &str,
    store: &dyn OutboxStore,
    trigger: PostTrigger,
    connections: &[Connection],
    dev: bool,
    secrets: &dyn ConnSecrets,
    http: &dyn ConnHttp,
    clock: &dyn NoticeClock,
    waiter: &dyn NoticeWaiter,
    tz: &Tz,
    budget: f64,
) -> PostResult {
    let r = store.with_lock(id, &mut || post_locked(id, store, trigger, connections, dev, secrets, http, clock, waiter, tz, budget));
    if let Some(r) = r {
        return r;
    }
    // 잠금이 걸려 있지 않으면 — 다른 곳에서 이 글을 올리는 중
    PostResult { item: store.read(id), note: "다른 곳에서 이 글을 올리는 중입니다".to_string(), busy: true, ..PostResult::default() }
}

/// 잠금 안의 올림 몸통 — `apPostItem` 의 `store.withLock(id) { … }` 클로저를 1:1
#[allow(clippy::too_many_arguments)]
fn post_locked(
    id: &str,
    store: &dyn OutboxStore,
    trigger: PostTrigger,
    connections: &[Connection],
    dev: bool,
    secrets: &dyn ConnSecrets,
    http: &dyn ConnHttp,
    clock: &dyn NoticeClock,
    waiter: &dyn NoticeWaiter,
    tz: &Tz,
    budget: f64,
) -> PostResult {
    let Some(mut it) = store.read(id) else { return PostResult { note: "글을 찾지 못했습니다".to_string(), ..PostResult::default() } };
    let start = clock.now();
    if it.state == OutboxState::Posting {
        it.state = OutboxState::Uncertain;
        it.last = Some(Attempt { at: start, what: "uncertain".to_string(), code: 0, note: "앞 올리기가 끝나지 못했습니다. 받을 곳에 이미 생겼는지 확인한 뒤 올립니다".to_string() });
        it.updated = start;
        store.write(&it);
    }
    let conn = connections.iter().find(|c| c.id == it.connection);
    let decision = post_decide(
        &it,
        trigger,
        conn,
        conn.map(|c| secrets.exists(&c.id)).unwrap_or(false),
        dev,
        clock.now(),
        tz,
    );
    let base = match decision {
        PostDecision::Nothing(why) => return PostResult { item: Some(it), note: why, ..PostResult::default() },
        PostDecision::Hold(why) => {
            it.last = Some(Attempt { at: start, what: "held".to_string(), ..Attempt::default() });
            it.updated = start;
            store.write(&it);
            return PostResult { item: Some(it), note: why, ..PostResult::default() };
        }
        PostDecision::Go(b) => b,
    };
    let Some(c) = conn else { return PostResult { item: Some(it), note: "연결이 없어 요청하지 않았습니다".to_string(), ..PostResult::default() } };
    let Some(token) = secrets.read(&c.id).filter(|tok| !tok.is_empty()) else {
        let note = "토큰이 키체인에 없어 요청하지 않았습니다";
        it.last = Some(Attempt { at: start, what: "held".to_string(), ..Attempt::default() });
        it.updated = start;
        store.write(&it);
        return PostResult { item: Some(it), note: note.to_string(), ..PostResult::default() };
    };
    // 보내기 전에 올리는 중으로(죽으면 다음에 잠금을 잡은 쪽이 "확인 필요"로 본다)
    it.state = OutboxState::Posting;
    it.site = base.clone();
    it.tries += 1;
    it.retry_after = None;
    it.later = None;
    it.updated = start;
    store.write(&it);
    let mut run = PostRun {
        http,
        waiter,
        clock,
        base: base.clone(),
        header: auth_header(&auth_for(c), &token),
        start,
        budget,
        requests: 0,
        last_wait: None,
    };
    // 아래 블록은 send·fail·done 의 내부 함수를 만들지 않고 순서대로 풀어 쓴다
    match it.target.service {
        // 실행 중에 새로 필요해진 티켓(⑩-3) — 그 프로젝트의 하위 작업 종류를 읽고, 스토리 아위로 만든다
        TargetService::Jira if it.kind == "ticket" => {
            if it.remote.is_none() {
                let project = it.target.space.clone();
                let tr = run.send(&jira_project_request(&project, &base, auth_for(c)));
                let Some(ty) = pick_issue_type(&jira_issue_types(&tr), TicketKind::Task, "") else {
                    let resp = if tr.status == 200 {
                        HttpResponse { error: Some(format!("프로젝트 {project} 에 하위 작업 종류가 없습니다")), ..HttpResponse::default() }
                    } else {
                        tr.clone()
                    };
                    return fail_post(&mut it, store, &run, resp, "이슈 종류");
                };
                let resp = run.send(&jira_create_issue_request(
                    &project,
                    &ty.id,
                    &it.target.parent,
                    &it.title,
                    &it.doc,
                    &base,
                    auth_for(c),
                ));
                let Some(key) = created_issue_key(&resp, &project) else { return fail_post(&mut it, store, &run, resp, "티켓") };
                done_post(&mut it, store, &run, "issue", Some(Remote { id: key.clone(), url: format!("{base}/browse/{key}") }));
            }
        }
        TargetService::Jira => {
            if it.remote.is_none() {
                let resp = run.send(&jira_comment_request(&it, &base, auth_for(c)));
                let Some(rid) = created_id(TargetService::Jira, &resp) else { return fail_post(&mut it, store, &run, resp, "댓글") };
                let url_r = remote_url(&it, &base, &rid);
                done_post(&mut it, store, &run, "comment", Some(Remote { id: rid.clone(), url: url_r }));
            }
        }
        TargetService::Confluence => {
            if it.remote.is_none() {
                let resp = run.send(&confluence_page_request(&it, &base, auth_for(c)));
                let Some(rid) = created_id(TargetService::Confluence, &resp) else { return fail_post(&mut it, store, &run, resp, "페이지") };
                let url_r = remote_url(&it, &base, &rid);
                done_post(&mut it, store, &run, "page", Some(Remote { id: rid.clone(), url: url_r }));
            }
            if let Some(name) = it.attachment.clone() {
                if !it.steps.iter().any(|s| s == "attachment") {
                    if let Some(page) = it.remote.as_ref().map(|r| r.id.clone()) {
                        let Some(file) = store.attachment(&name) else {
                            return fail_post(
                                &mut it,
                                store,
                                &run,
                                HttpResponse { error: Some(format!("첨부할 {name} 이 실행 폴더에 없습니다")), ..HttpResponse::default() },
                                "첨부",
                            );
                        };
                        let resp = run.send(&confluence_attachment_request(&it, &base, &page, &file, auth_for(c)));
                        if !(200..=299).contains(&resp.status) || resp.error.is_some() {
                            return fail_post(&mut it, store, &run, resp, "첨부");
                        }
                        done_post(&mut it, store, &run, "attachment", None);
                    }
                }
            }
        }
        TargetService::Notion => {
            let batched = batches(&notion_blocks(&it.doc));
            let mut sent = it
                .steps
                .iter()
                .find(|s| s.starts_with("blocks:"))
                .and_then(|s| s[7..].parse::<usize>().ok())
                .unwrap_or(0);
            if it.remote.is_none() {
                let first = batched.first().cloned().unwrap_or_default();
                let resp = run.send(&notion_page_request(&it, &base, &first));
                let Some(rid) = created_id(TargetService::Notion, &resp) else { return fail_post(&mut it, store, &run, resp, "페이지") };
                sent = first.len();
                let url_r = remote_url(&it, &base, &rid);
                done_post(&mut it, store, &run, "page", Some(Remote { id: rid.clone(), url: url_r }));
                done_post(&mut it, store, &run, &format!("blocks:{sent}"), None);
            }
            let all: Vec<Value> = batched.iter().flatten().cloned().collect();
            for batch in batches(&all[sent.min(all.len())..]) {
                let page = it.remote.as_ref().map(|r| r.id.clone()).unwrap_or_default();
                let resp = run.send(&notion_append_request(&base, &page, &batch));
                if !(200..=299).contains(&resp.status) || resp.error.is_some() {
                    return fail_post(&mut it, store, &run, resp, "블록 덧붙이기");
                }
                sent += batch.len();
                done_post(&mut it, store, &run, &format!("blocks:{sent}"), None);
            }
        }
    }
    let now = clock.now();
    it.state = OutboxState::Posted;
    it.last = Some(Attempt { at: now, what: "posted".to_string(), code: 0, note: "올렸습니다".to_string() });
    it.updated = now;
    store.write(&it);
    PostResult { item: Some(it), requests: run.requests, note: "올렸습니다".to_string(), busy: false }
}


// ── 아래는 원본 `Outbox.swift` 를 임시로 얹는 곳(시험 outbox·post_ticket 이 쓰는 것만) — R-게시 청이 Outbox.swift 전체를 통합하면 이 얹음을 뗀다 ──

/// 한 번 올리기에서 429·503 뒤 다시 보내는 횟수. 원본: `Outbox.swift` 의 `apRetryMax`(notify_rules.rs 의 비공개 사본과도 같다)
pub const RETRY_MAX: i32 = 3;
/// 한 번 기다리는 초 상한. 원본: `Outbox.swift` 의 `apRetryWaitMax`
pub const RETRY_WAIT_MAX: f64 = 20.0;

/// 다시 보낼지와 기다릴 초 — 429·503 만, Retry-After(초 또는 HTTP 날짜)를 따르되 1~20초, 없으면 1·2·4초. 횟수를 넘으면 None. 원본: `Outbox.swift` 의 `apRetryWait`
pub fn retry_wait(status: i32, retry_after: Option<&str>, attempt: i32, now: f64) -> Option<f64> {
    if (status != 429 && status != 503) || attempt >= RETRY_MAX {
        return None;
    }
    let mut w = 2f64.powi(attempt);
    let h = retry_after.map(trim_ws).unwrap_or("");
    if !h.is_empty() {
        if let Ok(s) = h.parse::<f64>() {
            if s.is_finite() {
                w = s;
            }
        } else if let Some(d) = http_date(h) {
            w = d - now;
        }
    }
    Some(w.clamp(1.0, RETRY_WAIT_MAX))
}

/// `EEE, dd MMM yyyy HH:mm:ss GMT` — Swift `DateFormatter` en_US_POSIX 는 요일 이름 대소문자를 느슨하게 받는다. 원본: `Outbox.swift` 406–411 줄의 날짜 해석(notify_rules.rs 비공개 사본과 같다)
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

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 잠깐의 실패인가 — 자동으로 올림에서 다음 단위 끝·끝 정리 때 다시 보낸다. 원본: `Outbox.swift` 의 `apTransient`
pub fn transient(r: &HttpResponse) -> bool {
    if r.status == 0 {
        let e = r.error.as_deref().unwrap_or("");
        return !e.contains("사이트 밖") && !e.contains("실행 폴더에 없습니다");
    }
    r.status == 429 || (500..=599).contains(&r.status)
}

