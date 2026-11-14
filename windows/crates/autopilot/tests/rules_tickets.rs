//! Tickets 시험 — `Tests/AutopilotRulesTests/TicketsTests.swift` 의 여덟 가지
//! (make_all·retry_left·uncertain·blocked·waits_and_guards·types_and_keys·outbox·post_ticket)과
//! 청 a 의 여섯 가지(계획 읽기·이슈 종류 고르기·요청 꼴·제목 다듬기·키 자리표·상태 파일).
//! outbox·post_ticket 은 임시로 얹은 `Outbox.swift` 슬라이스의 시험이기도 하다.
//! 요청 꼴 골든(Tests/AutopilotRulesTests/Publish/ticket-*.json)은 만들기·부모바꾸기 요청 본문 그대로다

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use claude_board_autopilot::rules::connections::{
    Auth, AuthHeader, ConnHttp, ConnSecrets, Connection, HttpRequest, HttpResponse,
};
use claude_board_autopilot::rules::formats::{doc_plain_lines, grapheme_len, DOC_MARKER};
use claude_board_autopilot::rules::notify_rules::{NoticeClock, NoticeWaiter, OutboxState, PublishMode};
use claude_board_autopilot::rules::stopline::Tz;
use claude_board_autopilot::rules::tickets::*;
use serde_json::{json, Value};

const SITE: &str = "https://zz-toy.atlassian.net";
const RUN: &str = "20261030-1500-zz";

// ── 골든·견본 도우미(원본의 golden·pretty) ─────────────────────────────────

/// 골든과 비교한다 — `AP_REPORT_GOLDEN=write` 면 다시 쓴다. 윈도우 체크아웃은 줄 끝이 CRLF 일 수 있다(글: LF 만)
fn golden(name: &str, text: &str) {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Tests/AutopilotRulesTests/Publish").join(name);
    if std::env::var("AP_REPORT_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(p, text).expect("골든 쓰기");
        return;
    }
    let want = std::fs::read_to_string(p).map(|s| s.replace("\r\n", "\n")).unwrap_or(format!("(골든 {name} 없음)"));
    assert_eq!(text, want, "골든 {name} 와 다름");
}

/// Swift JSONSerialization 의 [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes] 꼴 — 2칸 들여쓰기, 키 뒤 " : "
fn pretty(v: &Value) -> String {
    fn out(v: &Value, depth: usize, buf: &mut String) {
        match v {
            Value::Object(o) if o.is_empty() => buf.push_str("{}"),
            Value::Object(o) => {
                let mut kv: Vec<(&String, &Value)> = o.iter().collect();
                kv.sort_by(|a, b| a.0.cmp(b.0));
                buf.push_str("{\n");
                for (i, (k, val)) in kv.iter().enumerate() {
                    buf.push_str(&" ".repeat(depth * 2 + 2));
                    out(&Value::String((*k).clone()), depth + 1, buf);
                    buf.push_str(" : ");
                    out(val, depth + 1, buf);
                    if i + 1 < kv.len() {
                        buf.push(',');
                    }
                    buf.push('\n');
                }
                buf.push_str(&" ".repeat(depth * 2));
                buf.push('}');
            }
            Value::Array(a) if a.is_empty() => buf.push_str("[]"),
            Value::Array(a) => {
                buf.push_str("[\n");
                for (i, val) in a.iter().enumerate() {
                    buf.push_str(&" ".repeat(depth * 2 + 2));
                    out(val, depth + 1, buf);
                    if i + 1 < a.len() {
                        buf.push(',');
                    }
                    buf.push('\n');
                }
                buf.push_str(&" ".repeat(depth * 2));
                buf.push(']');
            }
            other => buf.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    out(v, 0, &mut s);
    s.push('\n');
    s
}

/// 요청 본문(Option) — 없으면 "(본문 없음)"
fn pretty_body(b: Option<&[u8]>) -> String {
    match b {
        None => "(본문 없음)\n".to_string(),
        Some(d) => {
            let v: Value = serde_json::from_slice(d).expect("요청 본문 JSON");
            pretty(&v)
        }
    }
}

/// Swift `APAuth.basic(email:)`
fn auth() -> Auth {
    Auth::Basic { user: "kim@example.com".to_string() }
}

// ── 견본(Swift 시험과 같은 계획) ───────────────────────────────────────────

/// 출발 확인에서 승인한 만들 티켓 계획 — 에픽 아래 새 스토리·작업 둘, 이미 있는 스토리로 옮길 작업 하나
fn plan() -> TicketPlan {
    TicketPlan {
        run: RUN.to_string(),
        epic: "SHOP-10".to_string(),
        story_type: String::new(),
        task_type: String::new(),
        entries: vec![
            TicketEntry {
                id: "story:new-1".to_string(),
                kind: TicketKind::Story,
                title: "쿠폰 만료 묶음".to_string(),
                parent: "SHOP-10".to_string(),
                key: String::new(),
                units: vec!["쿠폰 만료일 검사".to_string(), "만료 쿠폰 안내".to_string()],
            },
            TicketEntry {
                id: "task:u2".to_string(),
                kind: TicketKind::Task,
                title: "쿠폰 만료일 검사".to_string(),
                parent: "story:new-1".to_string(),
                key: String::new(),
                units: vec!["쿠폰 만료일 검사".to_string()],
            },
            TicketEntry {
                id: "task:u4".to_string(),
                kind: TicketKind::Task,
                title: "장바구니 합계 고침".to_string(),
                parent: "SHOP-12".to_string(),
                key: String::new(),
                units: vec!["장바구니 합계 고침".to_string()],
            },
            TicketEntry {
                id: "move:SHOP-13".to_string(),
                kind: TicketKind::Move,
                title: String::new(),
                parent: "story:new-1".to_string(),
                key: "SHOP-13".to_string(),
                units: Vec::new(),
            },
        ],
    }
}

// ── 시험 ───────────────────────────────────────────────────────────────────

/// 계획을 읽는다 — 꼴이 틀린 곳의 까닭을 준다. 고칠 것이 하나라도 있으면 만들기를 막는다(요청 0)
#[test]
fn plan_parse() {
    // 맞음
    let bytes = serde_json::to_vec(&ticket_plan_json(&plan())).expect("계획 JSON");
    let (p, broken) = ticket_plan_parse(Some(&bytes));
    assert_eq!(broken, Vec::<String>::new());
    assert_eq!(p, plan());
    assert_eq!(p.project(), "SHOP");

    // 꼴이 틀린 견본 여섯+하나(곤란이 있는 자리) — 각각 대답에 까닭이 있다
    let broken_of = |patch: &dyn Fn(&mut Value)| -> Vec<String> {
        let mut o = ticket_plan_json(&plan());
        patch(&mut o);
        ticket_plan_parse(Some(o.to_string().as_bytes())).1
    };

    // 에픽 번호 꼴
    let broken = broken_of(&|o| o["epic"] = json!("shop 10"));
    assert!(broken.iter().any(|s| s.contains("에픽 번호 꼴이 아닙니다: shop 10")));
    // 모르는 부모
    let broken = broken_of(&|o| o["entries"][1]["parent"] = json!("story:new-9"));
    assert!(broken.iter().any(|s| s.contains("항목 task:u2 의 부모 story:new-9 가 계획에 없습니다")));
    // 스토리는 에픽 아래
    let broken = broken_of(&|o| o["entries"][0]["parent"] = json!("SHOP-11"));
    assert!(broken.iter().any(|s| s.contains("스토리 story:new-1 의 부모가 에픽(SHOP-10)이 아닙니다")));
    // 옮길 키와 이름이 다름
    let broken = broken_of(&|o| o["entries"][3]["key"] = json!("SHOP-14"));
    assert!(broken.iter().any(|s| s.contains("옮길 작업 키 꼴이 아닙니다: SHOP-14")));
    // 같은 항목이 둘
    let broken = broken_of(&|o| {
        let e = o["entries"][1].clone();
        o["entries"].as_array_mut().expect("entries").push(e);
    });
    assert!(broken.iter().any(|s| s.contains("같은 항목이 둘입니다: task:u2")));
    // 빈 제목(비인쇄 뿐)
    let broken = broken_of(&|o| o["entries"][1]["title"] = json!(" \u{202E} "));
    assert!(broken.iter().any(|s| s.contains("항목 task:u2 의 제목이 비었습니다")));
    // 종류와 이름이 다름
    let broken = broken_of(&|o| o["entries"][1]["kind"] = json!("story"));
    assert!(broken.iter().any(|s| s.contains("항목 task:u2 의 종류가 이름과 다릅니다")));
}

/// 이슈 종류 고르기 — 프로젝트 설정 이름이 있으면 그 이름(대소문자 무시·없으면 없다), 없으면 종류 무리에서 고른다
#[test]
fn pick_type() {
    let it = |id: &str, name: &str, subtask: bool, level: i32| IssueType { id: id.to_string(), name: name.to_string(), subtask, level: Some(level) };

    // 영문 기본 — Story·Task·Subtask 이름
    let t = vec![it("1", "Story", false, 0), it("3", "Subtask", true, -1)];
    assert_eq!(pick_issue_type(&t, TicketKind::Story, "").as_ref().map(|x| x.id.as_str()), Some("1"));
    assert_eq!(pick_issue_type(&t, TicketKind::Task, "").as_ref().map(|x| x.id.as_str()), Some("3"));
    // 한국어 — 프로젝트 설정 이름이 없어도 종류 이름으로 고른다
    let t = vec![it("1", "스토리", false, 0), it("3", "하위 작업", true, -1)];
    assert_eq!(pick_issue_type(&t, TicketKind::Story, "").as_ref().map(|x| x.id.as_str()), Some("1"));
    assert_eq!(pick_issue_type(&t, TicketKind::Task, "").as_ref().map(|x| x.id.as_str()), Some("3"));
    // 옛 하위 작업 이름(Sub-task)
    let t = vec![it("1", "Story", false, 0), it("3", "Sub-task", true, -1)];
    assert_eq!(pick_issue_type(&t, TicketKind::Task, "").as_ref().map(|x| x.id.as_str()), Some("3"));
    // 이름이 없어도 수준(에픽 1, 글 0, 하위 작업 -1)으로 고른다
    let t = vec![it("1", "Feature", false, 0), it("3", "Child", true, -1)];
    assert_eq!(pick_issue_type(&t, TicketKind::Story, "").as_ref().map(|x| x.id.as_str()), Some("1"));
    assert_eq!(pick_issue_type(&t, TicketKind::Task, "").as_ref().map(|x| x.id.as_str()), Some("3"));
    // 프로젝트 설정 이름(대소문자 무시) — 설정에 있으면 그 종류
    let t = vec![it("1", "Story", false, 0), it("2", "User Story", false, 0), it("3", "개발", false, 0), it("4", "개발 하위", true, -1)];
    assert_eq!(pick_issue_type(&t, TicketKind::Story, "user story").as_ref().map(|x| x.id.as_str()), Some("2"));
    assert_eq!(pick_issue_type(&t, TicketKind::Task, "개발 하위").as_ref().map(|x| x.id.as_str()), Some("4"));
    // 설정 이름이 무리에 없으면 고르지 않는다(만들기를 막는다)
    let t = vec![it("1", "Story", false, 0), it("2", "Task", false, 0), it("3", "Subtask", true, -1)];
    assert!(pick_issue_type(&t, TicketKind::Story, "Feature").is_none());
    // 임의 이름이 아니라 정한 이름만
    assert!(pick_issue_type(&t, TicketKind::Story, "스토리").is_none());
    // 하위 작업 종류가 없으면 없다 — 보통 작업으로 만들지 않는다
    let t = vec![it("1", "Story", false, 0), it("2", "Task", false, 0)];
    assert_eq!(pick_issue_type(&t, TicketKind::Story, "").as_ref().map(|x| x.id.as_str()), Some("1"));
    assert!(pick_issue_type(&t, TicketKind::Task, "").is_none());
}

/// 만들기·부모바꾸기 요청 꼴 — 골든(Publish/ticket-*.json). 요약 칸 상한·제어문자가 다듬어졌는지 함께 견준다
#[test]
fn request_golden() {
    // 제목: 긴 줄표·제어문자·한글 300자 — Jira 요약 255자 안으로 다듬는다
    let dirty = format!("쿠폰 — 만료\u{202E}묶음 {}", "가".repeat(300));
    let e0 = plan().entries[0].clone();
    let doc = ticket_doc(&e0, RUN, "", "", "");
    let create = jira_create_issue_request("SHOP", "10001", "SHOP-10", &dirty, &doc, SITE, auth());
    golden("ticket-create-story.json", &pretty_body(create.body.as_deref()));
    let fields = &serde_json::from_slice::<Value>(create.body.as_ref().expect("본문 있음")).expect("본문 JSON")["fields"];
    let summary = fields["summary"].as_str().expect("요약 있음");
    assert!(grapheme_len(summary) <= TICKET_TITLE_MAX + 1);
    assert!(!summary.contains('—'));
    assert!(!summary.contains('\u{202E}'));
    assert_eq!(create.method, "POST");
    assert_eq!(create.url, format!("{SITE}/rest/api/3/issue"));

    let task = jira_create_issue_request("SHOP", "10003", "SHOP-101", "쿠폰 만료일 검사", &ticket_doc(&plan().entries[1], RUN, "", "", ""), SITE, auth());
    golden("ticket-create-task.json", &pretty_body(task.body.as_deref()));

    let move_req = jira_parent_request("SHOP-13", "SHOP-101", SITE, auth());
    golden("ticket-move.json", &pretty_body(move_req.body.as_deref()));
    assert_eq!(move_req.method, "PUT");
    assert_eq!(move_req.url, format!("{SITE}/rest/api/3/issue/SHOP-13"));
}

/// 계획 제목·단위 제목은 모델이 쓴 글일 수 있다 — 비밀·멘션·줄표를 다듬는다(요청 꼴). 실패 까닭도 사람 말
#[test]
fn plan_clean() {
    let mut p = plan();
    p.entries[0].title = format!("쿠폰 — @here <!channel> https://hooks.example.com/services/T0/B0/{}", "z".repeat(24));
    p.entries[0].units = vec!["@everyone 단위".to_string()];
    let parsed = ticket_plan_parse(Some(serde_json::to_vec(&ticket_plan_json(&p)).expect("계획 JSON").as_slice())).0;
    let title = &parsed.entries[0].title;
    assert!(!title.contains('@'));
    assert!(!title.contains("—"));
    assert!(!title.contains("hooks.example.com/services"));
    assert_eq!(parsed.entries[0].units, vec!["＠everyone 단위".to_string()]);

    // 실패 까닭 — 받는 쪽 본문은 믿지 않아 상태 번호로만(Jira 티켓에 맞춘 말)
    let bad = |code: i32| HttpResponse { status: code, ..Default::default() };
    assert!(ticket_fail_note(&bad(400)).contains("이슈 종류·부모·필수 칸"));
    assert!(ticket_fail_note(&bad(404)).contains("프로젝트나 부모 이슈를 찾지 못했습니다"));
    assert!(ticket_fail_note(&bad(302)).contains("따라가지 않았습니다"));
}

/// 키 자리표 — story:new-N 과 단위 id. 자리표는 이슈 키 꼭 꼴이지만 이슈 키 꼴이 아니라, 출발을 막는 값이다
#[test]
fn placeholders() {
    assert_eq!(ticket_placeholder("SHOP", "story:new-3"), "SHOP-NEW3");
    assert_eq!(ticket_placeholder("SHOP", "task:u2"), "SHOP-NEW-u2");
    assert_eq!(ticket_placeholder("SHOP", "task:a/b c"), "SHOP-NEW-a_b_c");
    assert!(is_ticket_placeholder("SHOP-NEW3"));
    assert!(!issue_key_ok("SHOP-NEW3"));
    // 이슈 키 꼭 꼴의 NEW 뒤는 1–4자리 수 또는 이름 한 조각
    assert!(!is_ticket_placeholder("SHOP-NEW3.5"));
    assert!(!is_ticket_placeholder("SHOP-12"));
    assert!(!is_ticket_placeholder("NEW-1"));

    // 출발하면 안 되는 키 — 자리표·임시 번호(TMP-…)가 남은 대기열 단위
    let units = vec![
        json!({"story": "SHOP-NEW1", "tickets": ["SHOP-NEW-u2"]}),
        json!({"story": "SHOP-12", "tickets": ["SHOP-13"]}),
        json!({"story": "SHOP-12", "tickets": ["TMP-3-1"]}),
        json!({"title": "티켓 없는 단위"}),
    ];
    assert_eq!(unmade_keys(&units), vec!["SHOP-NEW1".to_string(), "SHOP-NEW-u2".to_string(), "TMP-3-1".to_string()]);
    // 모두 만들었고 임시 번호가 없다 — 아무것도 막지 않는다
    assert!(unmade_keys(&[units[1].clone(), units[3].clone()]).is_empty());
}

/// 상태 파일을 읽는다 — 만듦·믿지 않음(만들었다는데 키가 없으면)·꼴이 틀린 칸은 버린다. 그리고 돌려 쓸 수 있나
#[test]
fn book_parse() {
    let o = json!({
        "format": TICKET_BOOK_FORMAT,
        "version": 1,
        "site": SITE,
        "entries": {
            "story:new-1": {"state": "made", "key": "SHOP-101", "tries": 1, "updated": 1},
            "task:u2": {"state": "made", "key": "<b>x</b>"},
            "../evil": {"state": "made", "key": "SHOP-1"},
            "task:u4": {"state": "zz"}
        }
    });
    let bytes = serde_json::to_vec(&o).expect("상태 JSON");
    let b = ticket_book_parse(Some(&bytes));
    let keys: Vec<String> = b.entries.keys().cloned().collect();
    assert_eq!(keys, vec!["story:new-1".to_string(), "task:u2".to_string()]);
    assert_eq!(b.site, SITE);
    let ok = &b.entries["story:new-1"];
    assert_eq!(ok.state, TicketState::Made);
    assert_eq!(ok.key, "SHOP-101");
    assert_eq!(ok.tries, 1);
    // 만들었다는데 키가 없으면 믿지 않는다 — 사람이 확인해야 다시 만든다
    assert_eq!(b.entries["task:u2"].state, TicketState::Uncertain);

    // 돌려 써도 같다
    let back = ticket_book_parse(Some(serde_json::to_vec(&ticket_book_json(&b)).expect("상태 JSON").as_slice()));
    assert_eq!(back, b);
}

// ── 만들기·올림 시험의 가짜 어댑터(원본 PublishTests·TicketsTests 와 같다) ─────────

/// 서울 시각(TicketsTests.swift 의 `seoul`)
const SEOUL: Tz = Tz::Local;
const FAKE_TOKEN: &str = "fake-token-ZZ9-not-real";
const NOW: f64 = 1_793_300_000.0;

/// 가짜 시각 — 숫자 하나를 갖는다(시험 사이에 바꾼다)
struct PubClock {
    t: std::cell::Cell<f64>,
}
impl PubClock {
    fn new(t: f64) -> Self {
        PubClock { t: std::cell::Cell::new(t) }
    }
}
impl NoticeClock for PubClock {
    fn now(&self) -> f64 {
        self.t.get()
    }
}

/// 기다림을 기록하고 시계를 넘긴다(PubWaiter)
struct PubWaiter<'a> {
    clock: &'a PubClock,
    waits: RefCell<Vec<f64>>,
}
impl NoticeWaiter for PubWaiter<'_> {
    fn wait(&self, s: f64) {
        self.waits.borrow_mut().push(s);
        self.clock.t.set(self.clock.t.get() + s);
    }
}

/// 가짜 키체인 — 읽은 수를 센다
struct PubSecrets {
    items: BTreeMap<String, String>,
    reads: std::cell::Cell<usize>,
}
impl ConnSecrets for PubSecrets {
    fn read(&self, id: &str) -> Option<String> {
        self.reads.set(self.reads.get() + 1);
        self.items.get(id).cloned()
    }
    fn exists(&self, id: &str) -> bool {
        self.items.contains_key(id)
    }
}

/// 가짜 HTTP — 차례대로 정한 답을 돌려주고 받은 요청과 인증 머리를 남긴다(남는 답이 없으면 200 {})
struct PubHTTP {
    answers: RefCell<std::collections::VecDeque<HttpResponse>>,
    got: RefCell<Vec<HttpRequest>>,
    auths: RefCell<Vec<String>>,
}
impl ConnHttp for PubHTTP {
    fn send(&self, request: &HttpRequest, auth: Option<&AuthHeader>) -> HttpResponse {
        self.got.borrow_mut().push(request.clone());
        self.auths
            .borrow_mut()
            .push(auth.map(|a| format!("{}: {}", a.name, a.value)).unwrap_or_default());
        match self.answers.borrow_mut().pop_front() {
            Some(r) => r,
            None => HttpResponse { status: 200, body: "{}".as_bytes().to_vec(), ..HttpResponse::default() },
        }
    }
}
/// 올린 답 — `ok(id)`({"id":id}, 200)
fn ok_auth(id: &str, status: i32) -> HttpResponse {
    HttpResponse { status, body: format!("{{\"id\":\"{id}\"}}").into_bytes(), ..HttpResponse::default() }
}

/// 가짜 상태 파일 — 메모리, 쓴 차례의 상태를 남긴다. locked 면 잠금을 못 잡는다
struct MemTicketStore {
    book: RefCell<TicketBook>,
    writes: RefCell<Vec<TicketBook>>,
    locked: bool,
}
impl MemTicketStore {
    fn new() -> Self {
        MemTicketStore { book: RefCell::new(TicketBook::default()), writes: RefCell::new(Vec::new()), locked: false }
    }
}
impl TicketStore for MemTicketStore {
    fn read(&self) -> TicketBook {
        self.book.borrow().clone()
    }
    fn write(&self, b: &TicketBook) -> bool {
        self.writes.borrow_mut().push(b.clone());
        *self.book.borrow_mut() = b.clone();
        true
    }
    fn with_lock(&self, body: &mut dyn FnMut() -> TicketsResult) -> Option<TicketsResult> {
        if self.locked {
            None
        } else {
            Some(body())
        }
    }
}

/// Atlassian 연결(TicketsTests 의 `conn`)
fn conn_scopes(scopes: &[&str], expires: &str) -> Connection {
    Connection {
        id: "atlassian".to_string(),
        service: claude_board_autopilot::rules::connections::Service::Atlassian,
        site: SITE.to_string(),
        account: "kim@example.com".to_string(),
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        expires: expires.to_string(),
        ..Connection::default()
    }
}
fn conn() -> Connection {
    conn_scopes(&["jira", "read"], "")
}

/// 이슈 종류 읽는 답(`types()`)
fn types_resp() -> HttpResponse {
    let body = "{\"key\":\"SHOP\",\"issueTypes\":[{\"id\":\"10000\",\"name\":\"Epic\",\"subtask\":false,\"hierarchyLevel\":1},\
{\"id\":\"10001\",\"name\":\"Story\",\"subtask\":false,\"hierarchyLevel\":0},\
{\"id\":\"10002\",\"name\":\"Task\",\"subtask\":false,\"hierarchyLevel\":0},\
{\"id\":\"10003\",\"name\":\"Subtask\",\"subtask\":true,\"hierarchyLevel\":-1}]}";
    HttpResponse { status: 200, body: body.as_bytes().to_vec(), ..HttpResponse::default() }
}
/// 만든 답(`made(key)`)
fn made_resp(key: &str) -> HttpResponse {
    ok_auth(&format!("1\", \"key\":\"{key}"), 201)
}

// ── 시험(원본 TicketsTests 의 makeAll·retryLeft·uncertain·blocked·waitsAndGuards·typesAndKeys) ──

/// 한꺼번에 만들기: 이슈 종류 읽기 하나 → 스토리 → 작업 → 옮기기, 만든 키가 대기열 값으로
#[test]
fn make_all() {
    let store = MemTicketStore::new();
    let clock = PubClock::new(NOW);
    let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
    let http = PubHTTP {
        answers: RefCell::new([types_resp(), made_resp("SHOP-101"), made_resp("SHOP-102"), made_resp("SHOP-103"), HttpResponse { status: 204, ..Default::default() }].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let r = make_tickets(
        &plan(),
        &store,
        &[conn()],
        false,
        &PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) },
        &http,
        &clock,
        &waiter,
        SEOUL,
        &std::collections::BTreeSet::new(),
        30.0,
    );
    assert_eq!(r.requests, 5);
    assert_eq!(r.left, 0);
    assert_eq!(r.made, vec!["story:new-1", "task:u2", "task:u4", "move:SHOP-13"]);
    let got = http.got.borrow();
    let paths: Vec<String> = got.iter().map(|g| format!("{} {}", g.method, g.url.replace(SITE, ""))).collect();
    assert_eq!(
        paths,
        vec![
            "GET /rest/api/3/project/SHOP",
            "POST /rest/api/3/issue",
            "POST /rest/api/3/issue",
            "POST /rest/api/3/issue",
            "PUT /rest/api/3/issue/SHOP-13"
        ]
    );
    // 작업은 만든 스토리 키를 부모로, 옮기기도 만든 스토리 키로
    let fields = |i: usize| got[i].body.as_ref().and_then(|b| serde_json::from_slice::<Value>(b).ok()).unwrap_or(json!({}))["fields"].clone();
    assert_eq!(fields(1)["parent"]["key"], json!("SHOP-10"));
    assert_eq!(fields(1)["issuetype"]["id"], json!("10001"));
    assert_eq!(fields(2)["parent"]["key"], json!("SHOP-101"));
    assert_eq!(fields(2)["issuetype"]["id"], json!("10003"));
    assert_eq!(fields(3)["parent"]["key"], json!("SHOP-12"));
    assert_eq!(fields(4)["parent"]["key"], json!("SHOP-101"));
    let keys = ticket_keys(&plan(), &r.book);
    assert_eq!(keys.0.get("new-1").map(String::as_str), Some("SHOP-101"));
    assert_eq!(keys.1.get("u2").map(String::as_str), Some("SHOP-102"));
    assert_eq!(keys.1.get("u4").map(String::as_str), Some("SHOP-103"));
    assert_eq!(r.book.entries["story:new-1"].url, format!("{SITE}/browse/SHOP-101"));
    assert_eq!(r.book.site, SITE);
    // 보내기 전에 만드는 중으로 썼다(죽으면 다음에 확인 필요)
    assert!(store.writes.borrow().iter().any(|b| b.entries["story:new-1"].state == TicketState::Creating));
    // 토큰은 인증 머리로만(본문·주소·상태 파일에 없음)
    for a in http.auths.borrow().iter() {
        assert!(a.starts_with("Authorization: Basic "));
    }
    for g in got.iter() {
        assert!(!g.url.contains(FAKE_TOKEN));
        assert!(!String::from_utf8_lossy(g.body.as_deref().unwrap_or(&[])).contains(FAKE_TOKEN));
    }
    let book_bytes = serde_json::to_vec(&ticket_book_json(&r.book)).expect("상태 JSON");
    assert!(!String::from_utf8_lossy(&book_bytes).contains(FAKE_TOKEN));
    // 다시 누르면 요청 0
    drop(got);
    let again_http = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
    let again = make_tickets(&plan(), &store, &[conn()], false, &PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) }, &again_http, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert_eq!(again.requests, 0);
    assert!(again.note.contains("모두 있습니다"));
}

/// 실패하면 남기고 다시 누르면 남은 것만, 스토리를 못 만들면 그 아래는 보내지 않음
#[test]
fn retry_left() {
    let store = MemTicketStore::new();
    let clock = PubClock::new(NOW);
    let sec = PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) };
    // 첫 번: 스토리 400 → 그 스토리의 작업·옮기기는 보내지 않고, 이미 있는 스토리 아래 작업은 만든다
    let http = PubHTTP {
        answers: RefCell::new([
            types_resp(),
            HttpResponse { status: 400, body: "{\"errors\":{\"summary\":\"x\"}}".as_bytes().to_vec(), ..Default::default() },
            made_resp("SHOP-103"),
        ].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
    let r1 = make_tickets(&plan(), &store, &[conn()], false, &sec, &http, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert_eq!(r1.requests, 3);
    assert_eq!(r1.made, vec!["task:u4"]);
    assert_eq!(r1.failed, vec!["story:new-1"]);
    assert_eq!(r1.left, 3);
    assert_eq!(r1.book.entries["task:u2"].state, TicketState::Waiting);
    assert!(r1.book.entries["task:u2"].last.as_ref().map(|l| l.note.contains("먼저 만들어야")).unwrap_or(false));
    assert!(r1.book.entries["story:new-1"].last.as_ref().map(|l| l.note.contains("400")).unwrap_or(false));
    // 둘째 번: 남은 셋만(이미 만든 task:u4 는 요청 0)
    let http2 = PubHTTP {
        answers: RefCell::new([types_resp(), made_resp("SHOP-104"), made_resp("SHOP-105"), HttpResponse { status: 204, ..Default::default() }].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let r2 = make_tickets(&plan(), &store, &[conn()], false, &sec, &http2, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert_eq!(r2.requests, 4);
    assert_eq!(r2.left, 0);
    let keys = ticket_keys(&plan(), &r2.book);
    assert_eq!(keys.1.get("u2").map(String::as_str), Some("SHOP-105"));
    assert_eq!(keys.1.get("u4").map(String::as_str), Some("SHOP-103"));
    assert_eq!(http2.got.borrow().iter().filter(|g| g.method == "POST").count(), 2);
}

/// 만드는 중에 죽었으면 확인 필요(요청 0), 사람이 확인하면 그것만 다시
#[test]
fn uncertain() {
    let store = MemTicketStore::new();
    let clock = PubClock::new(NOW);
    let sec = PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) };
    let mut b = TicketBook { site: SITE.to_string(), ..TicketBook::default() };
    b.entries.insert("story:new-1".to_string(), TicketRecord { state: TicketState::Creating, ..TicketRecord::default() });
    *store.book.borrow_mut() = b;
    let http = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
    let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
    let r = make_tickets(&plan(), &store, &[conn()], false, &sec, &http, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert_eq!(r.requests, 0);
    assert_eq!(r.book.entries["story:new-1"].state, TicketState::Uncertain);
    assert!(r.note.contains("확인 필요 1개"));
    let http2 = PubHTTP {
        answers: RefCell::new([types_resp(), made_resp("SHOP-201"), made_resp("SHOP-202"), made_resp("SHOP-203"), HttpResponse { status: 204, ..Default::default() }].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let confirm = std::collections::BTreeSet::from(["story:new-1".to_string()]);
    let r2 = make_tickets(&plan(), &store, &[conn()], false, &sec, &http2, &clock, &waiter, SEOUL, &confirm, 30.0);
    assert_eq!(r2.left, 0);
    assert_eq!(r2.requests, 5);
    drop(waiter);
}

/// 연결로 만들 수 없으면 요청 0 — (이름, 연결, 토큰 둠, 기대 까닭)
#[test]
fn blocked() {
    struct Case(&'static str, Option<Connection>, bool, &'static str);
    let cases = vec![
        Case("연결 없음", None, true, "Atlassian 연결이 없습니다"),
        Case("Jira 쓰기 범위 없음", Some(conn_scopes(&["confluence", "read"], "")), true, "'Jira 쓰기'가 없습니다"),
        Case("토큰 없음", Some(conn()), false, "토큰이 키체인에 없습니다"),
        Case("만료", Some(conn_scopes(&["jira", "read"], "2026-10-01")), true, "만료일(2026-10-01)이 지났습니다"),
        Case("http 사이트", Some(Connection { site: "http://zz.example".to_string(), ..conn_scopes(&["jira"], "") }), true, "사이트 주소가 규칙에"),
        Case("계정 메일 없음", Some(Connection { account: String::new(), ..conn_scopes(&["jira"], "") }), true, "계정 메일이 없습니다"),
    ];
    for Case(name, c, token, want) in cases {
        let clock = PubClock::new(NOW);
        let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
        let http = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
        let conns: Vec<Connection> = c.into_iter().collect();
        let sec = PubSecrets {
            items: if token { BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]) } else { BTreeMap::new() },
            reads: std::cell::Cell::new(0),
        };
        let r = make_tickets(&plan(), &MemTicketStore::new(), &conns, false, &sec, &http, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
        assert_eq!(r.requests, 0, "{name}");
        assert!(http.got.borrow().is_empty(), "{name}");
        assert!(r.note.contains(want), "{name}: {}", r.note);
        assert_eq!(r.left, 4, "{name}");
    }
}

/// 429 는 기다렸다 다시, 사이트가 바뀌면 요청 0, 잠금을 못 잡으면 busy
#[test]
fn waits_and_guards() {
    let clock = PubClock::new(NOW);
    let sec = PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) };
    let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
    let one = TicketPlan {
        run: "r".to_string(),
        epic: "SHOP-10".to_string(),
        story_type: String::new(),
        task_type: String::new(),
        entries: vec![TicketEntry { id: "task:u4".to_string(), kind: TicketKind::Task, title: "합계".to_string(), parent: "SHOP-12".to_string(), key: String::new(), units: Vec::new() }],
    };
    let store = MemTicketStore::new();
    let http = PubHTTP {
        answers: RefCell::new([
            types_resp(),
            HttpResponse { status: 429, headers: BTreeMap::from([("retry-after".to_string(), "2".to_string())]), ..Default::default() },
            made_resp("SHOP-300"),
        ].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let r = make_tickets(&one, &store, &[conn()], false, &sec, &http, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert_eq!(r.left, 0);
    assert_eq!(*waiter.waits.borrow(), vec![2.0]);
    // 사이트가 바뀜: 만든 것이 있는 계획을 다른 사이트로 이어 만들지 않는다
    let mut entries = one.entries.clone();
    entries.push(TicketEntry { id: "task:u5".to_string(), kind: TicketKind::Task, title: "둘".to_string(), parent: "SHOP-12".to_string(), key: String::new(), units: Vec::new() });
    let two = TicketPlan { run: "r".to_string(), epic: "SHOP-10".to_string(), story_type: String::new(), task_type: String::new(), entries };
    let other = Connection { site: "https://zz-other.atlassian.net".to_string(), ..conn() };
    let http2 = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
    let r2 = make_tickets(&two, &store, &[other], false, &sec, &http2, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert!(http2.got.borrow().is_empty());
    assert!(r2.note.contains("사이트"));
    assert_eq!(r2.left, 1);
    let mut locked = MemTicketStore::new();
    locked.locked = true;
    let http3 = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
    let r3 = make_tickets(&one, &locked, &[conn()], false, &sec, &http3, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert!(r3.busy);
    assert_eq!(r3.requests, 0);
}

/// 이슈 종류를 못 읽거나 고를 수 없으면 실패 줄, 다른 프로젝트 키가 오면 받지 않음
#[test]
fn types_and_keys() {
    let clock = PubClock::new(NOW);
    let sec = PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) };
    let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
    let http = PubHTTP {
        answers: RefCell::new([HttpResponse { status: 404, ..Default::default() }].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let r = make_tickets(&plan(), &MemTicketStore::new(), &[conn()], false, &sec, &http, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert_eq!(r.requests, 1);
    assert_eq!(r.book.entries["story:new-1"].state, TicketState::Failed);
    assert!(r.note.contains("프로젝트 SHOP 를 읽지 못했습니다"));
    // 하위 작업 종류가 없는 프로젝트
    let no_sub = HttpResponse {
        status: 200,
        body: "{\"issueTypes\":[{\"id\":\"1\",\"name\":\"Story\",\"hierarchyLevel\":0}]}".as_bytes().to_vec(),
        ..Default::default()
    };
    let http2 = PubHTTP {
        answers: RefCell::new([no_sub, made_resp("SHOP-101")].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let r2 = make_tickets(&plan(), &MemTicketStore::new(), &[conn()], false, &sec, &http2, waiter.clock, &waiter, SEOUL, &std::collections::BTreeSet::new(), 30.0);
    assert!(r2.book.entries["task:u4"].last.as_ref().map(|l| l.note.contains("하위 작업 종류가 없습니다")).unwrap_or(false));
    assert!(created_issue_key(&ok_auth("1\", \"key\":\"OTHER-1", 201), "SHOP").is_none());
    assert!(created_issue_key(&ok_auth("1\", \"key\":\"SHOP-1x", 201), "SHOP").is_none());
    assert!(created_issue_key(&HttpResponse { status: 400, body: "{\"key\":\"SHOP-1\"}".as_bytes().to_vec(), ..Default::default() }, "SHOP").is_none());
    assert_eq!(created_issue_key(&ok_auth("1\", \"key\":\"SHOP-77", 201), "SHOP").as_deref(), Some("SHOP-77"));
}

// ── 실행 중에 새로 필요해진 티켓(임시 번호 → 검수 뒤 만들기) ─────────────────────

/// 결과 기록에 new_tickets 가 담긴 단위 하나(`runWithTickets`)
fn run_with_tickets(tickets: &[(&str, &str, &str)], status: &str) -> claude_board_autopilot::rules::report::ReportRun {
    use claude_board_autopilot::rules::report::{ReportRun, ReportUnit, ResultRecord, ResultTicket};
    let mut r = ReportRun { id: RUN.to_string(), project: "zz-shop".to_string(), ..ReportRun::default() };
    let mut u = ReportUnit {
        n: 3,
        title: "쿠폰 만료일 검사".to_string(),
        status: status.to_string(),
        story: "SHOP-12".to_string(),
        task: "SHOP-13".to_string(),
        ..ReportUnit::default()
    };
    let rec = ResultRecord {
        new_tickets: tickets
            .iter()
            .map(|(temp, title, why)| ResultTicket { temp: (*temp).to_string(), title: (*title).to_string(), why: (*why).to_string() })
            .collect(),
        ..ResultRecord::default()
    };
    u.result = Some(rec);
    r.units = vec![u];
    r
}

/// 가짜 outbox — 메모리, 쓴 글자(JSON)를 모두 남긴다. locked 면 잠금을 못 잡는다
struct MemOutbox {
    items: RefCell<BTreeMap<String, OutboxItem>>,
    writes: RefCell<Vec<String>>,
    locked: bool,
    files: BTreeMap<String, Vec<u8>>,
}
impl MemOutbox {
    fn new() -> Self {
        MemOutbox {
            items: RefCell::new(BTreeMap::new()),
            writes: RefCell::new(Vec::new()),
            locked: false,
            files: BTreeMap::from([("report.html".to_string(), "<html>보고서</html>".as_bytes().to_vec())]),
        }
    }
}
impl OutboxStore for MemOutbox {
    fn create(&self, it: &OutboxItem) -> bool {
        if self.items.borrow().contains_key(&it.id) {
            return false;
        }
        self.write(it);
        true
    }
    fn read(&self, id: &str) -> Option<OutboxItem> {
        self.items.borrow().get(id).cloned()
    }
    fn write(&self, it: &OutboxItem) {
        self.writes.borrow_mut().push(String::from_utf8_lossy(&outbox_data(it)).to_string());
        self.items.borrow_mut().insert(it.id.clone(), it.clone());
    }
    fn with_lock(&self, _id: &str, f: &mut dyn FnMut() -> PostResult) -> Option<PostResult> {
        if self.locked {
            None
        } else {
            Some(f())
        }
    }
    fn attachment(&self, name: &str) -> Option<Vec<u8>> {
        self.files.get(name).cloned()
    }
}

/// 카드 답이 temp 일 때만, 이 단위의 임시 번호만, 늘 검수 뒤
#[test]
fn outbox_test() {
    let r = run_with_tickets(
        &[("TMP-3-1", "만료 알림 메일 문구", "만료 안내가 따로 필요"), ("TMP-4-1", "다른 단위 번호", ""), ("SHOP-99", "진짜 키 꼴", ""), ("TMP-3-1", "같은 번호 둘", ""), ("TMP-3-2", " ", "")],
        "done",
    );
    let items = new_ticket_outbox(&r, 3, Some("SHOP-12"), Some("SHOP-13"), Some("temp"), 1.0);
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, vec!["u3-ticket-TMP-3-1"]);
    let it = &items[0];
    assert!(outbox_id_ok(&it.id));
    assert_eq!(it.kind, "ticket");
    assert_eq!(it.mode, PublishMode::Review);
    assert_eq!(it.target, OutboxTarget::new(TargetService::Jira, "TMP-3-1", "SHOP", "SHOP-12"));
    assert_eq!(doc_plain_lines(&it.doc).first().map(String::as_str), Some(DOC_MARKER));
    assert!(doc_plain_lines(&it.doc).iter().any(|l| l.contains("임시 번호 TMP-3-1") && l.contains("SHOP-13")));
    assert!(item_label(it).contains("TMP-3-1 → SHOP-12"));
    assert!(new_ticket_outbox(&r, 3, Some("SHOP-12"), Some("SHOP-13"), Some("defer"), 1.0).is_empty());
    assert!(new_ticket_outbox(&r, 3, None, None, Some("temp"), 1.0).is_empty());
    assert!(new_ticket_outbox(&run_with_tickets(&[("TMP-3-1", "x", "")], "skipped"), 3, Some("SHOP-12"), None, Some("temp"), 1.0).is_empty());
    // 파일 꼴로 돌아와도 같다. 티켓 글의 받을 곳이 임시 번호·이슈 키 꼴이 아니면 받지 않는다
    assert_eq!(outbox_parse(&outbox_data(it)), Some(it.clone()));
    let mut bad = it.clone();
    bad.target.issue = "SHOP-14".to_string();
    assert!(outbox_parse(&outbox_data(&bad)).is_none());
}

/// 검수 뒤 사람이 누르면 하위 작업 종류를 읽고 그 스토리 아래 만든다, 엔진 자동은 요청 0
#[test]
fn post_ticket() {
    let r = run_with_tickets(&[("TMP-3-1", "만료 알림 메일 문구", "")], "done");
    let mut it = new_ticket_outbox(&r, 3, Some("SHOP-12"), Some("SHOP-13"), Some("temp"), 1.0).swap_remove(0);
    it.mode = PublishMode::Review; // 만들 때 늘 review — 파일을 바꿔도 아래 자동 판정은 review 로 막힘을 본다
    let box_ = MemOutbox::new();
    box_.items.borrow_mut().insert(it.id.clone(), it.clone());
    let clock = PubClock::new(NOW);
    let sec = Box::new(PubSecrets { items: BTreeMap::from([("atlassian".to_string(), FAKE_TOKEN.to_string())]), reads: std::cell::Cell::new(0) });
    let waiter = PubWaiter { clock: &clock, waits: RefCell::new(Vec::new()) };
    let http0 = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
    // 자동으로는 검수 뒤 올림이라 요청 0
    let auto = post_item(&it.id, &box_, PostTrigger::Auto, &[conn()], false, sec.as_ref(), &http0, &clock, &waiter, &SEOUL, 30.0);
    assert_eq!(auto.requests, 0);
    let http = PubHTTP {
        answers: RefCell::new([types_resp(), made_resp("SHOP-140"), ok_auth("1", 200), ok_auth("2", 200)].into()),
        got: RefCell::new(Vec::new()),
        auths: RefCell::new(Vec::new()),
    };
    let p = post_item(&it.id, &box_, PostTrigger::Press, &[conn()], false, sec.as_ref(), &http, &clock, &waiter, &SEOUL, 30.0);
    assert_eq!(p.requests, 2);
    assert_eq!(p.item.as_ref().map(|i| i.state), Some(OutboxState::Posted));
    let remote = p.item.and_then(|i| i.remote);
    assert_eq!(remote.as_ref().map(|r| (r.id.clone(), r.url.clone())), Some(("SHOP-140".to_string(), format!("{SITE}/browse/SHOP-140"))));
    let got = http.got.borrow();
    let fields = &serde_json::from_slice::<Value>(got[1].body.as_ref().expect("본문 있음")).expect("본문 JSON")["fields"];
    assert_eq!(fields["parent"]["key"], json!("SHOP-12"));
    assert_eq!(fields["issuetype"]["id"], json!("10003"));
    drop(got);
    // 두 번째 누름은 요청 0
    let http_again = PubHTTP { answers: RefCell::new(std::collections::VecDeque::new()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) };
    let again = post_item(&it.id, &box_, PostTrigger::Press, &[conn()], false, sec.as_ref(), &http_again, &clock, &waiter, &SEOUL, 30.0);
    assert_eq!(again.requests, 0);
}
