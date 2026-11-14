//! 올릴 글(`rules::outbox`) 시험 전체 몫 — 원본: `Tests/AutopilotRulesTests/PublishTests.swift`(635 줄).
//! 연결·형식 변환 조각(118–356 줄)은 rules_meta.rs·rules_formats.rs 가 이미 같은 것을 갖고 있고(원본 한 파일에 연결·형식·올릴 글 셋이 같이 있다),
//! 여기는 게시 설정·만들 글(PublishPlanTests 358–425 줄)과 판정·다시 보내기(DecideTests 427–477 줄), 올리기(PostTests 479–635 줄)의 몫이다.
//! 실제 Atlassian·Notion 으로는 요청하지 않는다. 견본 기록은 `Tests/AutopilotRulesTests/Report/sample-run.json` 을 그 자리에서
//! 읽는다(tests/rules_report.rs 의 FixtureSource 와 같은 것). 시간대는 원본 `seoul` 대신 `Tz::Utc` — 이 시험의 판정은 시간대에 안 탄다
//! (만료일 여부를 볼 뿐이다; `날짜` 시험의 GMT 문자열은 손으로 준다).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::Path;

use claude_board_autopilot::rules::connections::{
    AuthHeader, ConnHttp, ConnSecrets, Connection, HttpRequest, HttpResponse, NOTION_VERSION, Service,
};
use claude_board_autopilot::rules::formats::{notion_blocks, Block, Doc, Span};
use claude_board_autopilot::rules::notify_rules::{NoticeClock, NoticeWaiter};
#[allow(unused_imports)] // 시험마다 다른 것을 쓴다(만들기·판정·기다림·표시)
use claude_board_autopilot::rules::outbox::*;
use claude_board_autopilot::rules::report::{report_run, ReportOrigin, ReportRun, ReportSource};
use claude_board_autopilot::rules::stopline::Tz;
use serde_json::Value;

const T0: f64 = 1_793_340_000.0; // 2026-10-30 즈음(서울) — 원본 PublishTests.swift `t0`

// 가짜 읽을 곳 — 견본 기록의 사전을 그 자리에서. 원본: `FixtureSource`(tests/rules_report.rs 와 같은 것)
struct FixtureSource {
    j: Value,
}

impl FixtureSource {
    fn new(j: Value) -> Self {
        FixtureSource { j }
    }
}

impl ReportSource for FixtureSource {
    fn origin(&self) -> ReportOrigin {
        ReportOrigin::Folder
    }
    fn run(&self) -> Option<serde_json::Map<String, Value>> {
        self.j.get("run").and_then(Value::as_object).cloned()
    }
    fn queue(&self) -> Vec<serde_json::Map<String, Value>> {
        let Some(Value::Array(a)) = self.j.get("queue") else { return Vec::new() };
        a.iter().map(|v| v.as_object().cloned()).collect::<Option<Vec<_>>>().unwrap_or_default()
    }
    fn engine(&self, n: i64) -> Option<serde_json::Map<String, Value>> {
        self.j.get("engines")?.get(n.to_string())?.as_object().cloned()
    }
    fn result(&self, n: i64) -> Option<String> {
        match self.j.get("results")?.get(n.to_string())? {
            Value::String(s) => Some(s.clone()),
            o => Some(o.to_string()),
        }
    }
}

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Tests/AutopilotRulesTests/Report").join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// 견본 기록을 보고서 값으로. 원본 PublishTests.swift `run(_ name:)` — id 는 기록의 `id` 칸, 없으면 "?"
fn run(name: &str) -> ReportRun {
    let j = fixture(name);
    let id = j.get("id").and_then(Value::as_str).unwrap_or("?").to_string();
    report_run(&id, &FixtureSource::new(j)).unwrap_or_default()
}

/// 원본 `atl`(Atlassian 연결) — 만료일만 달리 다시 만든다(`var old = atl; old.expires = …`)
fn atlassian(expires: &str) -> Connection {
    Connection {
        id: "atlassian".to_string(),
        service: Service::Atlassian,
        site: "https://example.atlassian.net".to_string(),
        account: "kim@example.com".to_string(),
        scopes: vec!["jira".to_string(), "confluence".to_string()],
        expires: expires.to_string(),
        ..Connection::default()
    }
}

// ── 원본: 올리기 시험의 가짜 어댑터(PublishTests.swift · PublishPlanTests 아래) ──

/// 가짜 토큰 — 어느 요청 본문에도 나타나면 안 되는 값. 원본 `fakeToken`
const FAKE_TOKEN: &str = "fake-token-ZZ9-not-real";

/// 가짜 시각 — 숫자 하나를 갖고, 기다림이 그 값을 넘긴다. 원본 `PubClock`
struct PubClock {
    t: Cell<f64>,
}
impl PubClock {
    fn new(t: f64) -> Self {
        PubClock { t: Cell::new(t) }
    }
}
impl NoticeClock for PubClock {
    fn now(&self) -> f64 {
        self.t.get()
    }
}

/// 기다림을 기록하고 시계를 넘긴다. 원본 `PubWaiter`
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

/// 가짜 키체인. 원본 `PubSecrets` — 읽은 수를 세는 것은 시험에서 안 써 뺐다
struct PubSecrets {
    items: BTreeMap<String, String>,
}
impl ConnSecrets for PubSecrets {
    fn read(&self, id: &str) -> Option<String> {
        self.items.get(id).cloned()
    }
    fn exists(&self, id: &str) -> bool {
        self.items.contains_key(id)
    }
}

/// 가짜 HTTP — 차례대로 정한 답을 돌려주고, 받은 요청과 인증 머리를 남긴다(남는 답이 없으면 200 {}). 원본 `PubHTTP`
struct PubHTTP {
    answers: RefCell<VecDeque<HttpResponse>>,
    got: RefCell<Vec<HttpRequest>>,
    auths: RefCell<Vec<String>>,
}
impl PubHTTP {
    fn new(answers: Vec<HttpResponse>) -> Self {
        PubHTTP { answers: RefCell::new(answers.into()), got: RefCell::new(Vec::new()), auths: RefCell::new(Vec::new()) }
    }
}
impl ConnHttp for PubHTTP {
    fn send(&self, request: &HttpRequest, auth: Option<&AuthHeader>) -> HttpResponse {
        self.got.borrow_mut().push(request.clone());
        self.auths.borrow_mut().push(match auth {
            Some(a) => format!("{}: {}", a.name, a.value),
            None => String::new(),
        });
        match self.answers.borrow_mut().pop_front() {
            Some(r) => r,
            None => HttpResponse { status: 200, body: "{}".as_bytes().to_vec(), ..HttpResponse::default() },
        }
    }
}

/// 올린 답 — 상태와 `{"id":…}` 몸통. 원본 PublishTests.swift `ok(_:_:)`
fn ok_auth(id: &str, status: i32) -> HttpResponse {
    HttpResponse { status, body: format!("{{\"id\":\"{id}\"}}").into_bytes(), ..HttpResponse::default() }
}

/// Notion 연결 — 사이트는 API 고정, 임박 칸 없음. 원본 `notion`
fn notion() -> Connection {
    Connection { id: "notion".to_string(), service: Service::Notion, account: "김요청".to_string(), scopes: vec!["write".to_string()], ..Connection::default() }
}

/// 원본 `post(_:)` 가 기본으로 주는 연결 두 개
fn default_conns() -> Vec<Connection> {
    vec![atlassian("2027-01-31"), notion()]
}

/// 원본 `post(_:)` 가 기본으로 주는 토큰 두 개
fn default_secrets() -> PubSecrets {
    PubSecrets {
        items: BTreeMap::from([
            ("atlassian".to_string(), FAKE_TOKEN.to_string()),
            ("notion".to_string(), FAKE_TOKEN.to_string()),
        ]),
    }
}

/// 가짜 outbox — 메모리에 담고 쓴 글자(JSON)를 모두 남긴다. 잠긴 판을 흉내할 수 있다. 원본 PublishTests.swift `MemOutbox`
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
    /// 정돈된 글 id 차례 — 포트 `APOutboxStore` 의 `ids()`. trait 에서는 빠졌다(쓰는 곳이 없었다, outbox.rs 머리의 차이 참고)
    fn ids(&self) -> Vec<String> {
        self.items.borrow().keys().cloned().collect()
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
        let j = String::from_utf8_lossy(&outbox_data(it)).to_string();
        self.writes.borrow_mut().push(j);
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

/// 올리기 부르기(사람이 누름, 기본 연결·토큰·시각·예산) — 원본 PublishTests.swift `post(_:http:context:)` 의 기본 인자
fn post(store: &MemOutbox, id: &str, http: &PubHTTP) -> (PostResult, Vec<f64>) {
    post_n(store, id, PostTrigger::Press, &default_conns(), false, &default_secrets(), http, &PubClock::new(T0), 30.0)
}

/// 올리기 부르기(엔진이 스스로, 기본 연결·토큰·시각·예산)
fn post_auto(store: &MemOutbox, id: &str, http: &PubHTTP) -> (PostResult, Vec<f64>) {
    post_n(store, id, PostTrigger::Auto, &default_conns(), false, &default_secrets(), http, &PubClock::new(T0), 30.0)
}

/// 전부 정해서 올리기 부르기 — Rust 에 기본 인자가 없어 둘로 나눈다(`draft_args` 전례, rules::draft)
#[allow(clippy::too_many_arguments)]
fn post_n(
    store: &MemOutbox,
    id: &str,
    trigger: PostTrigger,
    conns: &[Connection],
    dev: bool,
    secrets: &PubSecrets,
    http: &PubHTTP,
    clock: &PubClock,
    budget: f64,
) -> (PostResult, Vec<f64>) {
    let waiter = PubWaiter { clock, waits: RefCell::new(Vec::new()) };
    let r = post_item(id, store, trigger, conns, dev, secrets, http, clock, &waiter, &Tz::Utc, budget);
    let waits = waiter.waits.borrow().clone();
    (r, waits)
}

// ── 원본: 게시 설정 읽기(칸마다 한 줄) ─────────────────────────

struct PublishRow {
    note: &'static str,
    json: &'static str,
    want: &'static str,
}

const PUBLISH_ROWS: [PublishRow; 7] = [
    PublishRow { note: "없음 = 검수 뒤 올림, 받을 곳 없음", json: "null", want: "review - false | " },
    PublishRow {
        note: "자동·Confluence",
        json: r#"{"mode":"auto","report":{"service":"confluence","space":"TODO","parent":"123"},"jira_comments":true}"#,
        want: "auto confluence TODO 123 true | ",
    },
    PublishRow {
        note: "Confluence 상위 페이지 없음 = 스페이스 맨 위",
        json: r#"{"report":{"service":"confluence","space":"~abc"}}"#,
        want: "review confluence ~abc  false | ",
    },
    PublishRow {
        note: "Notion id 는 줄표 빼고 소문자",
        json: r#"{"report":{"service":"notion","parent":"0123ABCD-89ab-cdef-0123-456789ABCDEF"}}"#,
        want: "review notion  0123abcd89abcdef0123456789abcdef false | ",
    },
    PublishRow {
        note: "틀린 꼴은 그 칸만 버림",
        json: r#"{"mode":"later","report":{"service":"confluence","space":"a b"},"jira_comments":"yes"}"#,
        want: "review - false | publish.mode 는 review 또는 auto 입니다; publish.jira_comments 는 true/false 입니다; Confluence 스페이스 키 꼴이 아닙니다: a b",
    },
    PublishRow {
        note: "상위 페이지 id 숫자만",
        json: r#"{"report":{"service":"confluence","space":"TODO","parent":"../1"}}"#,
        want: "review - false | Confluence 상위 페이지 id 는 숫자입니다: ../1",
    },
    PublishRow {
        note: "모르는 서비스",
        json: r#"{"report":{"service":"slack"}}"#,
        want: "review - false | publish.report.service 는 confluence 또는 notion 입니다",
    },
];

/// 게시 방식의 raw 값(시험이 원본 `rawValue` 처럼 표시하려고)
fn mode_text(m: &PublishMode) -> &'static str {
    match m {
        PublishMode::Review => "review",
        PublishMode::Auto => "auto",
    }
}

/// 검수 예시 글 — 원본 PublishTests.swift `item(_:mode:)`(다른 mode 가 필요한 올리기 시험이 함께 쓴다)
fn item(s: TargetService, mode: PublishMode) -> OutboxItem {
    let r = run("sample-run.json");
    match s {
        TargetService::Jira => unit_outbox(&r, 1, Some("TODO-41"), &Publish { mode, jira_comments: true, ..Publish::default() }, T0)
            .into_iter()
            .next()
            .unwrap(),
        TargetService::Confluence => run_outbox(
            &r,
            &Publish { mode, report: Some(OutboxTarget::new(s, "", "TODO", "12345")), ..Publish::default() },
            "",
            true,
            T0,
            Tz::Utc,
        )
        .into_iter()
        .next()
        .unwrap(),
        TargetService::Notion => run_outbox(
            &r,
            &Publish { mode, report: Some(OutboxTarget::new(s, "", "", "0123456789abcdef0123456789abcdef")), ..Publish::default() },
            "김요청",
            true,
            T0,
            Tz::Utc,
        )
        .into_iter()
        .next()
        .unwrap(),
    }
}

/// 게시 설정 읽기 — 칸마다 한 줄. 원본: PublishTests.swift `설정`
#[test]
fn 설정() {
    for r in &PUBLISH_ROWS {
        let v: Value = serde_json::from_str(r.json).unwrap_or(Value::Null);
        let (p, probs) = publish_parse(if v.is_null() { None } else { Some(&v) });
        let t = p
            .report
            .as_ref()
            .map(|x| format!("{} {} {}", x.service.raw_value(), x.space, x.parent))
            .unwrap_or_else(|| "-".to_string());
        let got = format!("{} {} {} | {}", mode_text(&p.mode), t, p.jira_comments, probs.join("; "));
        assert_eq!(got, r.want, "{}", r.note);
        let (back, _) = publish_parse(Some(&publish_json(&p)));
        assert_eq!(back, p, "{} — 온전히 읽은 설정은 다시 읽어도 같다", r.note);
    }
}

/// 단위 댓글 — 켰고 티켓 키가 있고 끝난 단위만. 원본: PublishTests.swift `댓글`
#[test]
fn 댓글() {
    let r = run("sample-run.json");
    let on = Publish { jira_comments: true, ..Publish::default() };
    let ids = |unit: i64, ticket: Option<&str>, p: &Publish| unit_outbox(&r, unit, ticket, p, T0).iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(1, Some("TODO-41"), &on), ["u1-jira-TODO-41"]);
    assert!(ids(1, Some("TODO-41"), &Publish::default()).is_empty(), "끄면 댓글 없음");
    assert!(ids(1, None, &on).is_empty(), "티켓 키 없으면 없음");
    assert!(ids(1, Some("../x"), &on).is_empty(), "티켓 키 꼴이 틀리면 없음");
    assert_eq!(ids(4, Some("TODO-44"), &on).len(), 1, "건너뜀도 댓글");
    assert!(outbox_id_ok("u1-jira-TODO-41") && outbox_id_ok("run-confluence") && !outbox_id_ok("run-x/../y"));
}

/// 보고서 글 — 받을 곳이 있을 때 하나, Confluence 만 첨부. 원본: PublishTests.swift `보고서`
#[test]
fn 보고서() {
    let r = run("sample-run.json");
    assert!(run_outbox(&r, &Publish::default(), "", true, T0, Tz::Utc).is_empty(), "받을 곳 없으면 글 없음");
    let c = item(TargetService::Confluence, PublishMode::Review);
    let n = item(TargetService::Notion, PublishMode::Review);
    assert_eq!(c.id, "run-confluence");
    assert_eq!(c.attachment.as_deref(), Some("report.html"));
    assert_eq!(c.connection, "atlassian");
    assert_eq!(n.id, "run-notion");
    assert_eq!(n.attachment, None);
    assert_eq!(n.connection, "notion");
    let no_file = run_outbox(
        &r,
        &Publish { report: Some(OutboxTarget::new(TargetService::Confluence, "", "T", "")), ..Publish::default() },
        "",
        false,
        T0,
        Tz::Utc,
    )
    .into_iter()
    .next()
    .unwrap();
    assert_eq!(no_file.attachment, None, "보고서 파일이 없으면 첨부 없음");
}

/// 출발 확인 알림 — 연결 없음·범위·토큰·만료. 원본: PublishTests.swift `알림`
#[test]
fn 알림() {
    let p = Publish { report: Some(OutboxTarget::new(TargetService::Confluence, "", "T", "")), jira_comments: true, ..Publish::default() };
    let none = HashSet::<String>::new();
    assert_eq!(publish_notes(&p, &[], &none, false, T0, Tz::Utc).len(), 2, "연결 없음 — 보고서·댓글 받을 곳 각각");
    let secret = HashSet::from(["atlassian".to_string()]);
    assert!(publish_notes(&p, &[atlassian("2027-01-31")], &secret, false, T0, Tz::Utc).is_empty());
    let old = [atlassian("2026-10-01")];
    let notes = publish_notes(&p, &old, &secret, false, T0, Tz::Utc);
    assert_eq!(notes.iter().filter(|n| n.contains("만료일이 지났습니다")).count(), 2, "{notes:?}");
    assert!(publish_notes(&Publish::default(), &[], &none, false, T0, Tz::Utc).is_empty());
}

// ── 원본: 올리기(판정과 다시 보내기, PublishTests.swift DecideTests 427–477 줄) ──

/// 만든 글은 다시 만들지 않는다(같은 id). 원본: PublishTests.swift `한번`
#[test]
fn 한번() {
    let s = MemOutbox::new();
    assert_eq!(
        create_items(vec![item(TargetService::Jira, PublishMode::Review), item(TargetService::Confluence, PublishMode::Review)], &s),
        ["u1-jira-TODO-41", "run-confluence"]
    );
    assert!(create_items(vec![item(TargetService::Jira, PublishMode::Review)], &s).is_empty(), "같은 id 는 다시 만들지 않는다");
    assert_eq!(s.items.borrow().len(), 2);
}

struct DecideRow {
    note: &'static str,
    state: OutboxState,
    mode: PublishMode,
    trigger: PostTrigger,
    conn: &'static str,
    want: &'static str,
}

/// 올릴지 판정 — 칸마다 한 줄. 원본 PublishTests.swift `decideRows`
const DECIDE_ROWS: [DecideRow; 14] = [
    DecideRow { note: "검수 뒤 올림 + 엔진 = 요청 0", state: OutboxState::Waiting, mode: PublishMode::Review, trigger: PostTrigger::Auto, conn: "ok", want: "nothing" },
    DecideRow { note: "검수 뒤 올림 + 사람이 누름 = 보냄", state: OutboxState::Waiting, mode: PublishMode::Review, trigger: PostTrigger::Press, conn: "ok", want: "go" },
    DecideRow { note: "자동으로 올림 + 엔진 = 보냄", state: OutboxState::Waiting, mode: PublishMode::Auto, trigger: PostTrigger::Auto, conn: "ok", want: "go" },
    DecideRow { note: "연결 없음 = 요청 0, 남김", state: OutboxState::Waiting, mode: PublishMode::Auto, trigger: PostTrigger::Auto, conn: "none", want: "hold" },
    DecideRow { note: "토큰 없음", state: OutboxState::Waiting, mode: PublishMode::Auto, trigger: PostTrigger::Press, conn: "nosecret", want: "hold" },
    DecideRow { note: "만료일 지남", state: OutboxState::Waiting, mode: PublishMode::Auto, trigger: PostTrigger::Auto, conn: "expired", want: "hold" },
    DecideRow { note: "범위 밖", state: OutboxState::Waiting, mode: PublishMode::Auto, trigger: PostTrigger::Press, conn: "noscope", want: "hold" },
    DecideRow { note: "설치판의 127.0.0.1 사이트", state: OutboxState::Waiting, mode: PublishMode::Auto, trigger: PostTrigger::Press, conn: "local", want: "hold" },
    DecideRow { note: "이미 올림 = 두 번째 누름도 요청 0", state: OutboxState::Posted, mode: PublishMode::Review, trigger: PostTrigger::Press, conn: "ok", want: "nothing" },
    DecideRow { note: "올리지 않기 + 엔진", state: OutboxState::Skipped, mode: PublishMode::Auto, trigger: PostTrigger::Auto, conn: "ok", want: "nothing" },
    DecideRow { note: "올리지 않기 + 사람이 다시 누름", state: OutboxState::Skipped, mode: PublishMode::Review, trigger: PostTrigger::Press, conn: "ok", want: "go" },
    DecideRow { note: "확인 필요 + 엔진 = 요청 0", state: OutboxState::Uncertain, mode: PublishMode::Auto, trigger: PostTrigger::Auto, conn: "ok", want: "nothing" },
    DecideRow { note: "확인 필요 + 사람", state: OutboxState::Uncertain, mode: PublishMode::Auto, trigger: PostTrigger::Press, conn: "ok", want: "go" },
    DecideRow { note: "거부된 실패 + 엔진 = 사람이 본 뒤", state: OutboxState::Failed, mode: PublishMode::Auto, trigger: PostTrigger::Auto, conn: "ok", want: "nothing" },
];

/// 올릴지 판정 — 글 상태·게시 방식·누름·연결 조件마다. 원본: PublishTests.swift `판정`
#[test]
fn 판정() {
    for r in &DECIDE_ROWS {
        let mut it = item(TargetService::Jira, r.mode);
        it.state = r.state;
        let mut c = Some(atlassian("2027-01-31"));
        let mut secret = true;
        match r.conn {
            "none" => c = None,
            "nosecret" => secret = false,
            "expired" => *c.as_mut().unwrap() = atlassian("2026-10-01"),
            "noscope" => {
                let mut x = atlassian("2027-01-31");
                x.scopes = vec!["confluence".to_string()];
                c = Some(x);
            }
            "local" => {
                let mut x = atlassian("2027-01-31");
                x.site = "http://127.0.0.1:9".to_string();
                c = Some(x);
            }
            _ => {}
        }
        let d = post_decide(&it, r.trigger, c.as_ref(), secret, false, T0, &Tz::Utc);
        let got = match d {
            PostDecision::Go(_) => "go",
            PostDecision::Nothing(_) => "nothing",
            PostDecision::Hold(_) => "hold",
        };
        assert_eq!(got, r.want, "{}", r.note);
    }
}

/// 다시 보낼 때 — 상태와 Retry-After·시도마다. 원본: PublishTests.swift `다시`
#[test]
fn 다시() {
    for (status, ra, attempt, want) in [(429, Some("3"), 0, 3.0), (429, None, 0, 1.0), (429, None, 2, 4.0), (429, Some("120"), 0, 20.0), (429, Some("0"), 1, 1.0),
        (503, None, 1, 2.0), (429, None, 3, -1.0), (500, Some("3"), 0, -1.0), (401, None, 0, -1.0)]
    {
        let w = retry_wait(status, ra, attempt, T0);
        assert_eq!(w.unwrap_or(-1.0), want, "({status}, {ra:?}, {attempt})");
    }
}

/// Retry-After 가 HTTP 날짜인 경우. 원본: PublishTests.swift `날짜`
#[test]
fn 날짜() {
    // 1_793_340_005 = "Fri, 30 Oct 2026 06:00:05 GMT" — t0 보다 5 초 뒤
    assert_eq!(retry_wait(429, Some("Fri, 30 Oct 2026 06:00:05 GMT"), 0, T0), Some(5.0));
}

// ── 원본: 실제로 올리기(PublishTests.swift PostTests 479–635 줄) ──

/// 연결이 없으면 요청 없이 글만 남는다. 원본: PublishTests.swift `연결없음`
#[test]
fn 연결없음() {
    let s = MemOutbox::new();
    s.create(&item(TargetService::Confluence, PublishMode::Auto));
    let h = PubHTTP::new(Vec::new());
    let (r, _) = post_n(&s, "run-confluence", PostTrigger::Auto, &[], false, &default_secrets(), &h, &PubClock::new(T0), 30.0);
    assert!(h.got.borrow().is_empty() && r.requests == 0, "연결이 없으면 요청 없음");
    let it = s.read("run-confluence").unwrap();
    assert_eq!(it.state, OutboxState::Waiting);
    assert_eq!(it.last.map(|l| l.what), Some("held".to_string()), "글은 남고, 마지막 시도에 'held' 를 남긴다");
}

/// 검수 뒤 올림 — 엔진이 불러도 요청 없음, 사람이 눌러야 하나 올라간다. 원본: PublishTests.swift `검수`
#[test]
fn 검수() {
    let s = MemOutbox::new();
    s.create(&item(TargetService::Jira, PublishMode::Review));
    let h = PubHTTP::new(vec![ok_auth("10001", 201)]);
    post_auto(&s, "u1-jira-TODO-41", &h);
    assert!(h.got.borrow().is_empty(), "검수 뒤 올림은 엔진이 불러도 요청 없음");
    let (r, _) = post(&s, "u1-jira-TODO-41", &h);
    assert_eq!(r.note, "올렸습니다");
    assert_eq!(h.got.borrow().len(), 1);
    {
        let got = h.got.borrow();
        assert_eq!(
            (got[0].method.as_str(), got[0].url.as_str()),
            ("POST", "https://example.atlassian.net/rest/api/3/issue/TODO-41/comment")
        );
    }
    let it = s.read("u1-jira-TODO-41").unwrap();
    assert_eq!(
        it.remote,
        Some(Remote {
            id: "10001".to_string(),
            url: "https://example.atlassian.net/browse/TODO-41?focusedCommentId=10001".to_string()
        })
    );
}

/// 같은 글을 두 번 누르면 두 번은 하나다. 원본: PublishTests.swift `두번`
#[test]
fn 두번() {
    let mut s = MemOutbox::new();
    s.create(&item(TargetService::Jira, PublishMode::Review));
    let h = PubHTTP::new(vec![ok_auth("10001", 201), ok_auth("10002", 201)]);
    let (r1, _) = post(&s, "u1-jira-TODO-41", &h);
    assert_eq!(h.got.borrow().len(), 1);
    assert_eq!(r1.note, "올렸습니다");
    let (r2, _) = post(&s, "u1-jira-TODO-41", &h);
    assert_eq!(h.got.borrow().len(), 1, "다시 눌러도 요청은 없다");
    assert_eq!(r2.requests, 0);
    assert_eq!(r2.note, "이미 올렸습니다");
    s.locked = true;
    let (r3, _) = post(&s, "u1-jira-TODO-41", &h);
    assert!(r3.busy, "다른 곳에서 올리는 중입니다");
    assert_eq!(h.got.borrow().len(), 1);
}

/// 올리는 중 죽으면 다음 누름이 마친다. 원본: PublishTests.swift `죽음`
#[test]
fn 죽음() {
    let s = MemOutbox::new();
    let mut it = item(TargetService::Jira, PublishMode::Auto);
    it.state = OutboxState::Posting;
    s.create(&it);
    let h = PubHTTP::new(vec![ok_auth("10001", 201)]);
    let (r, _) = post_auto(&s, &it.id, &h);
    assert!(h.got.borrow().is_empty() && r.requests == 0, "올리는 중이면 엔진은 요청 없이 본다");
    assert_eq!(s.read(&it.id).unwrap().state, OutboxState::Uncertain);
    post(&s, &it.id, &h);
    assert_eq!(h.got.borrow().len(), 1);
    assert_eq!(s.read(&it.id).unwrap().state, OutboxState::Posted);
    // 올리는 동안 쓴 판 가운데 "올리는 중"이 성공보다 먼저 왔다
    let states = s.writes.borrow().iter().filter_map(|w| outbox_parse(w.as_bytes())).map(|i| i.state).collect::<Vec<_>>();
    let last_two = states[states.len() - 2..].to_vec();
    assert_eq!(last_two, [OutboxState::Posting, OutboxState::Posted]);
}

/// Confluence — 페이지 만들고 첨부한다(첨부가 실패해도 페이지는 뒀다, 다시 누르면 첨부부터). 원본: PublishTests.swift `컨플`
#[test]
fn 컨플() {
    let s = MemOutbox::new();
    s.create(&item(TargetService::Confluence, PublishMode::Review));
    let h = PubHTTP::new(vec![
        ok_auth("98765", 200),
        HttpResponse { status: 500, ..HttpResponse::default() },
        HttpResponse { status: 500, ..HttpResponse::default() },
        HttpResponse { status: 500, ..HttpResponse::default() },
    ]);
    let (r, _) = post(&s, "run-confluence", &h);
    {
        let it = r.item.as_ref().unwrap();
        assert_eq!(it.state, OutboxState::Failed, "첨부가 계속 실패하면 실패로 남는다");
        assert_eq!(it.remote.as_ref().map(|x| x.id.as_str()), Some("98765"), "페이지는 만들어 뒀다");
        assert_eq!(it.steps, ["page"]);
    }
    {
        let got = h.got.borrow();
        assert_eq!(got.iter().map(|rq| rq.method.as_str()).collect::<Vec<_>>(), ["POST", "PUT"]);
        assert_eq!(got[1].url, "https://example.atlassian.net/wiki/rest/api/content/98765/child/attachment");
        assert!(
            got[1].headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("x-atlassian-token") && v == "no-check"),
            "첨부 요청은 X-Atlassian-Token: no-check 로 검사를 피한다"
        );
    }
    let h2 = PubHTTP::new(vec![HttpResponse { status: 200, body: "{}".as_bytes().to_vec(), ..HttpResponse::default() }]);
    let (r2, _) = post(&s, "run-confluence", &h2);
    assert_eq!(h2.got.borrow().len(), 1);
    assert_eq!(h2.got.borrow()[0].method, "PUT", "다시 누를 때는 첨부만");
    let it2 = r2.item.as_ref().unwrap();
    assert_eq!(it2.state, OutboxState::Posted);
    assert_eq!(it2.steps, ["page", "attachment"]);
    {
        let got = h.got.borrow();
        let page: Value = serde_json::from_slice(got[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(page["space"]["key"].as_str(), Some("TODO"));
        assert_eq!(page["ancestors"][0]["id"].as_str(), Some("12345"));
    }
    let att = {
        let bound = h2.got.borrow();
        String::from_utf8_lossy(bound[0].body.as_ref().unwrap()).into_owned()
    };
    assert!(att.contains("filename=\"report.html\"") && att.contains("<html>보고서</html>"), "첨부 본문은 파일과 html 이 담긴다");
}

/// Notion — 페이지 만들고 블록을 100 개씩 나눠 올린다. 원본: PublishTests.swift `노션`
#[test]
fn 노션() {
    let s = MemOutbox::new();
    let mut it = item(TargetService::Notion, PublishMode::Review);
    let base = notion_blocks(&it.doc).len();
    let mut blocks = it.doc.blocks.clone();
    if base < 150 {
        // 150 개를 채워 두 번(100 + 남은 수)으로 나눠 올리게 한다
        for _ in 0..(150 - base) {
            blocks.push(Block::Paragraph(vec![Span::Text("줄".to_string(), false)]));
        }
        it.doc = Doc::new(blocks);
    }
    s.create(&it);
    let h = PubHTTP::new(vec![
        ok_auth("0123456789abcdef0123456789abcdef", 200),
        HttpResponse { status: 200, body: "{}".as_bytes().to_vec(), ..HttpResponse::default() },
    ]);
    let (r, _) = post(&s, &it.id, &h);
    let it2 = r.item.as_ref().unwrap();
    assert_eq!(it2.state, OutboxState::Posted);
    {
        let got = h.got.borrow();
        assert_eq!(got.iter().map(|rq| rq.method.as_str()).collect::<Vec<_>>(), ["POST", "PATCH"]);
        assert_eq!(got[1].url, "https://api.notion.com/v1/blocks/0123456789abcdef0123456789abcdef/children");
        assert_eq!(got[0].headers.get("Notion-Version").map(String::as_str), Some(NOTION_VERSION));
        let first: Value = serde_json::from_slice(got[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(first["children"].as_array().map(Vec::len), Some(100));
        let rest: Value = serde_json::from_slice(got[1].body.as_ref().unwrap()).unwrap();
        assert_eq!(rest["children"].as_array().map(Vec::len), Some(50));
    }
    assert_eq!(it2.steps, ["page", "blocks:150"]);
    assert_eq!(it2.remote.as_ref().map(|x| x.url.as_str()), Some("https://www.notion.so/0123456789abcdef0123456789abcdef"));
}

/// 받는 쪽이 429 로 미루면 Retry-After 를 따른다(예산을 넘으면 그만). 원본: PublishTests.swift `기다림`
#[test]
fn 기다림() {
    let s = MemOutbox::new();
    s.create(&item(TargetService::Jira, PublishMode::Auto));
    let h = PubHTTP::new(vec![
        HttpResponse { status: 429, headers: BTreeMap::from([("retry-after".to_string(), "2".to_string())]), ..HttpResponse::default() },
        HttpResponse { status: 429, headers: BTreeMap::from([("retry-after".to_string(), "2".to_string())]), ..HttpResponse::default() },
        ok_auth("10001", 201),
    ]);
    let (r, w) = post_auto(&s, "u1-jira-TODO-41", &h);
    assert_eq!(h.got.borrow().len(), 3, "세 번에 성공");
    assert_eq!(w, [2.0, 2.0], "Retry-After 2 초씩 두 번 기다린다");
    assert_eq!(r.item.as_ref().unwrap().state, OutboxState::Posted);

    // 20 초 — 예산(30 초)이 세 번째 시도 전에 끝나므로 한 번 기다리고 그만둔다
    let s2 = MemOutbox::new();
    s2.create(&item(TargetService::Jira, PublishMode::Auto));
    let clock = PubClock::new(T0);
    let h2 = PubHTTP::new(vec![
        HttpResponse { status: 429, headers: BTreeMap::from([("retry-after".to_string(), "20".to_string())]), ..HttpResponse::default() },
        HttpResponse { status: 429, headers: BTreeMap::from([("retry-after".to_string(), "20".to_string())]), ..HttpResponse::default() },
    ]);
    let (r2, w2) = post_n(&s2, "u1-jira-TODO-41", PostTrigger::Auto, &default_conns(), false, &default_secrets(), &h2, &clock, 30.0);
    assert_eq!(h2.got.borrow().len(), 2);
    assert_eq!(w2, [20.0]);
    let it2 = r2.item.as_ref().unwrap();
    assert_eq!(it2.state, OutboxState::Failed);
    assert_eq!(it2.retry_after, Some(clock.t.get() + 20.0), "이따가 다시 보낼 시각");

    // 자동으로 다시 보낼 때 — 예정 전에는 요청 없이 보고, 지나면 다시 보낸다
    let h3 = PubHTTP::new(vec![ok_auth("10009", 201)]);
    post_n(&s2, "u1-jira-TODO-41", PostTrigger::Auto, &default_conns(), false, &default_secrets(), &h3, &PubClock::new(clock.t.get() + 5.0), 30.0);
    assert!(h3.got.borrow().is_empty(), "예정 전에는 요청하지 않는다");
    post_n(&s2, "u1-jira-TODO-41", PostTrigger::Auto, &default_conns(), false, &default_secrets(), &h3, &PubClock::new(clock.t.get() + 25.0), 30.0);
    assert_eq!(h3.got.borrow().len(), 1);
    assert_eq!(s2.read("u1-jira-TODO-41").unwrap().state, OutboxState::Posted);
}

/// 거꾸로 된 답(401·302)은 기다리지 않고 실패 — 사람이 본 뒤에만 다시. 원본: PublishTests.swift `거부`
#[test]
fn 거부() {
    for st in [401, 302] {
        let s = MemOutbox::new();
        s.create(&item(TargetService::Jira, PublishMode::Auto));
        let h = PubHTTP::new(vec![HttpResponse { status: st, ..HttpResponse::default() }]);
        let (r, _) = post_auto(&s, "u1-jira-TODO-41", &h);
        let it = r.item.as_ref().unwrap();
        assert_eq!(it.state, OutboxState::Failed, "{st}");
        assert_eq!(it.retry_after, None, "{st}");
        assert_eq!(it.last.as_ref().map(|l| l.code), Some(st), "{st}");
        post_auto(&s, "u1-jira-TODO-41", &h);
        assert_eq!(h.got.borrow().len(), 1, "{st} — 엔진은 다시 요청하지 않는다");
    }
}

/// 연결의 만료일이 지났으면 요청 없이 본다. 원본: PublishTests.swift `만료`
#[test]
fn 만료() {
    let old = atlassian("2026-10-01");
    let s = MemOutbox::new();
    s.create(&item(TargetService::Jira, PublishMode::Auto));
    let h = PubHTTP::new(Vec::new());
    let (r, _) = post_n(
        &s,
        "u1-jira-TODO-41",
        PostTrigger::Press,
        std::slice::from_ref(&old),
        false,
        &default_secrets(),
        &h,
        &PubClock::new(T0),
        30.0,
    );
    assert!(h.got.borrow().is_empty());
    assert!(r.note.contains("만료일"), "{}", r.note);
}

/// 올리기 시작한 사이트와 연결 사이트가 다르면 요청 없이 본다. 원본: PublishTests.swift `사이트바뀜`
#[test]
fn 사이트바뀜() {
    let s = MemOutbox::new();
    let mut it = item(TargetService::Confluence, PublishMode::Review);
    it.state = OutboxState::Failed;
    it.site = "https://old.atlassian.net".to_string();
    it.remote = Some(Remote { id: "1".to_string(), url: String::new() });
    it.steps = vec!["page".to_string()];
    s.create(&it);
    let h = PubHTTP::new(Vec::new());
    post(&s, &it.id, &h);
    assert!(h.got.borrow().is_empty());
    assert_eq!(s.read(&it.id).unwrap().last.map(|l| l.what), Some("held".to_string()));
}

/// 토큰은 어디에도 남지 않는다 — 기록·요청 값, 요청에는 인증 머리로만. 원본: PublishTests.swift `토큰`
#[test]
fn 토큰() {
    let s = MemOutbox::new();
    assert_eq!(
        create_items(
            vec![
                item(TargetService::Jira, PublishMode::Review),
                item(TargetService::Confluence, PublishMode::Review),
                item(TargetService::Notion, PublishMode::Review),
            ],
            &s
        ),
        ["u1-jira-TODO-41", "run-confluence", "run-notion"]
    );
    let h = PubHTTP::new(vec![
        ok_auth("98765", 200),
        HttpResponse { status: 200, body: "{}".as_bytes().to_vec(), ..HttpResponse::default() },
        ok_auth("0123456789abcdef0123456789abcdef", 200),
        ok_auth("10001", 201),
    ]);
    for id in s.ids() {
        post(&s, &id, &h);
    }
    assert!(s.items.borrow().values().all(|it| it.state == OutboxState::Posted), "셋 다 올림");
    // 토큰(과 그 HTTP 기본 옷)은 기록에 남지 않는다
    const AUTH_BASIC: &str = "a2ltQGV4YW1wbGUuY29tOmZha2UtdG9rZW4tWlo5LW5vdC1yZWFs";
    assert!(!s.writes.borrow().iter().any(|w| w.contains(FAKE_TOKEN) || w.contains(AUTH_BASIC)), "기록에 토큰 없음");
    {
        let got = h.got.borrow();
        assert!(
            got.iter().all(|rq| !rq.headers.values().any(|v| v.contains(FAKE_TOKEN))
                && !rq.body.as_ref().is_some_and(|b| String::from_utf8_lossy(b).contains(FAKE_TOKEN))),
            "요청 값에도 토큰이 없다"
        );
    }
    assert!(h.auths.borrow().iter().all(|a| !a.is_empty()), "모든 요청은 인증 머리로만 토큰을 실는다");
}

/// 글 표시 고치기(나중에·올리지 않기)와 확인 글의 수. 원본: PublishTests.swift `표시`
#[test]
fn 표시() {
    let s = MemOutbox::new();
    s.create(&item(TargetService::Jira, PublishMode::Review));
    let c = PubClock::new(T0);
    let m = mark_item("u1-jira-TODO-41", "later", &s, &c);
    assert_eq!(m.item.as_ref().unwrap().later, Some(T0));
    assert_eq!(outbox_state_label(&s.read("u1-jira-TODO-41").unwrap()), "나중에");
    assert_eq!(mark_item("u1-jira-TODO-41", "skip", &s, &c).item.as_ref().unwrap().state, OutboxState::Skipped);
    post_auto(&s, "u1-jira-TODO-41", &PubHTTP::new(Vec::new()));
    assert_eq!(s.read("u1-jira-TODO-41").unwrap().state, OutboxState::Skipped, "올리지 않게 한 글은 엔진이 다시 올리지 않는다");
    let mut p = item(TargetService::Confluence, PublishMode::Review);
    p.state = OutboxState::Posted;
    s.create(&p);
    assert_eq!(mark_item(&p.id, "skip", &s, &c).note, "이미 올렸습니다");
    assert_eq!(
        outbox_counts(&[
            item(TargetService::Jira, PublishMode::Review),
            item(TargetService::Jira, PublishMode::Review),
            item(TargetService::Confluence, PublishMode::Review),
            item(TargetService::Notion, PublishMode::Review),
        ]),
        "Jira 댓글 2 · Confluence 1 · Notion 1"
    );
}
