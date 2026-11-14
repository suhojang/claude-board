//! 알림 webhook·3시간 요약·끝 알림 흐름 시험(`notify_rules.rs`) — 원본: `Tests/AutopilotRulesTests/NotifyTests.swift`(242줄, 전체 옮김).
//! 실제 메신저로는 보내지 않는다: HTTP·키체인·시각은 가짜 어댑터(PubHTTP·PubSecrets·PubClock)다. 골든(Publish/notify-*.json)은
//! 악성 견본(Report/evil-run.json)과 멘션 꼴 제목으로 만든 본문 그대로다 — 받는 쪽 꼴의 이스케이프·멘션 무력화·길이 상한을 고정한다
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use claude_board_autopilot::rules::branches::Obj;
use claude_board_autopilot::rules::connections::{
    connections_json, parse_connections, Auth, AuthHeader, ConnHttp, ConnSecrets, Connection, HttpRequest, HttpResponse, NotifyFormat,
    Service,
};
use claude_board_autopilot::rules::formats::{grapheme_len, DOC_MARKER};
use claude_board_autopilot::rules::notify_rules::*;
use claude_board_autopilot::rules::report::{report_run, FixedZone, ReportOrigin, ReportRun, ReportSource, ReportUnit};
use serde_json::{json, Map, Value};

const T0: f64 = 1_793_340_000.0;
const HOOK_URL: &str = "https://hooks.example.com/services/T0FAKE/B0FAKE/zzzzzzzzzzzzzzzzzzzzzzzz";

// ── 골든·견본 도우미(원본의 golden·pretty·evil) ─────────────────────────────

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

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Tests/AutopilotRulesTests/Report").join(name);
    serde_json::from_str(&std::fs::read_to_string(p).expect("견본 읽기")).expect("견본 JSON")
}

// 가짜 읽을 곳 — 견본 기록의 사전을 그대로. 원본: `FixtureSource`(tests/rules_report.rs 와 같은 것)
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
    fn run(&self) -> Option<Map<String, Value>> {
        self.j.get("run").and_then(Value::as_object).cloned()
    }
    fn queue(&self) -> Vec<Map<String, Value>> {
        let Some(Value::Array(a)) = self.j.get("queue") else { return Vec::new() };
        a.iter().map(|v| v.as_object().cloned()).collect::<Option<Vec<_>>>().unwrap_or_default()
    }
    fn engine(&self, n: i64) -> Option<Map<String, Value>> {
        self.j.get("engines")?.get(n.to_string())?.as_object().cloned()
    }
    fn result(&self, n: i64) -> Option<String> {
        match self.j.get("results")?.get(n.to_string())? {
            Value::String(s) => Some(s.clone()),
            o => Some(o.to_string()),
        }
    }
}

/// 악성 견본 실행. 원본 evil()
fn evil() -> ReportRun {
    let j = fixture("evil-run.json");
    let id = j.get("id").and_then(Value::as_str).unwrap_or("?").to_string();
    report_run(&id, &FixtureSource::new(j)).unwrap_or_default()
}

/// 멘션 꼴과 비밀이 든 실행 — 제목에 <!channel>·@here·<users/all>·가짜 webhook 주소·너비 없는 글자
fn mention_run() -> ReportRun {
    let mut r = ReportRun {
        id: "20261030-1500-zz".to_string(),
        project: "zz <!channel> @here 팀\u{200B}".to_string(),
        state: "running".to_string(),
        model: "haiku".to_string(),
        started: Some(T0),
        ..Default::default()
    };
    let u = ReportUnit {
        n: 1,
        title: format!("쿠폰 <users/all> @everyone {HOOK_URL}"),
        status: "running".to_string(),
        ..Default::default()
    };
    let v = ReportUnit {
        n: 2,
        title: "합계".to_string(),
        status: "done".to_string(),
        ..Default::default()
    };
    r.units = vec![v, u];
    r
}

fn hook(id: &str, f: NotifyFormat, name: &str) -> Connection {
    Connection {
        id: id.to_string(),
        service: Service::Webhook,
        name: name.to_string(),
        host: "hooks.example.com".to_string(),
        scopes: vec!["notify".to_string()],
        format: f,
        ..Connection::default()
    }
}

// ── 가짜 어댑터(원본의 PubClock·PubWaiter·PubSecrets·PubHTTP) ────────────────

struct PubClock(f64);

impl NoticeClock for PubClock {
    fn now(&self) -> f64 {
        self.0
    }
}

struct PubWaiter {
    waits: RefCell<Vec<f64>>,
}

impl NoticeWaiter for PubWaiter {
    fn wait(&self, seconds: f64) {
        self.waits.borrow_mut().push(seconds);
    }
}

struct PubSecrets(BTreeMap<String, String>);

impl ConnSecrets for PubSecrets {
    fn read(&self, id: &str) -> Option<String> {
        self.0.get(id).cloned()
    }
    fn exists(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }
}

struct PubHTTP {
    /// 미리 정해 둔 응답 — 순서대로 하나씩
    rs: RefCell<Vec<HttpResponse>>,
    /// 실제로 잡혀 간 요청
    got: RefCell<Vec<HttpRequest>>,
}

impl PubHTTP {
    fn new(rs: Vec<HttpResponse>) -> Self {
        PubHTTP { rs: RefCell::new(rs), got: RefCell::new(Vec::new()) }
    }
}

impl ConnHttp for PubHTTP {
    fn send(&self, request: &HttpRequest, _auth: Option<&AuthHeader>) -> HttpResponse {
        self.got.borrow_mut().push(request.clone());
        let mut rs = self.rs.borrow_mut();
        if rs.is_empty() {
            HttpResponse::default()
        } else {
            rs.remove(0)
        }
    }
}

// ── 1/3 구간: 다듬기·이스케이프·본문 골든·글 상한 ────────────────────────────

// @Test("다듬기: 비밀 가리기·멘션 무력화·너비 없는 글자·긴 줄표·상한")
#[test]
fn 다듬기() {
    let s = notify_clean(&format!("@channel 보세요 — {HOOK_URL} \u{200B}@here {}", "가".repeat(400)));
    assert!(!s.contains('@'));
    assert!(s.contains("＠channel") && s.contains("＠here"));
    assert!(!s.contains("hooks.example.com/services"));
    assert!(!s.contains('—') && !s.contains('\u{200B}'));
    assert!(grapheme_len(&s) <= NOTIFY_LINE_MAX + 1);
}

// @Test("받는 쪽 꼴의 이스케이프")
#[test]
fn 이스케이프() {
    assert_eq!(notify_escape("<!here> & <@U1> <#C1|x>", NotifyFormat::Slack), "&lt;!here&gt; &amp; &lt;@U1&gt; &lt;#C1|x&gt;");
    assert_eq!(notify_escape("<users/all> <https://x|y>", NotifyFormat::Gchat), "＜users/all＞ ＜https://x|y＞");
    assert_eq!(notify_escape("<b>x</b> & y", NotifyFormat::Json), "＜b＞x＜/b＞ & y");
}

// @Test("본문 골든: 악성 견본 끝 알림·멘션 꼴 출발 알림을 꼴마다")
#[test]
fn 본문_골든() {
    let e = end_notice(&notice_run_facts(&evil()), 2, true);
    let s = start_notice(&notice_run_facts(&mention_run()), 0);
    for f in [NotifyFormat::Json, NotifyFormat::Slack, NotifyFormat::Gchat] {
        let a = notify_body(&e, f);
        let b = notify_body(&s, f);
        for (name, d) in [(format!("notify-end-{}.json", f.raw()), &a), (format!("notify-start-{}.json", f.raw()), &b)] {
            let v: Value = serde_json::from_slice(d).expect("본문 JSON");
            golden(&name, &pretty(&v));
            let t = String::from_utf8(d.clone()).expect("본문 UTF-8");
            assert!(!t.contains("<script") && !t.contains("<img") && !t.contains("<!channel") && !t.contains("<users/all"), "{f:?}");
            assert!(!t.contains("@here") && !t.contains("@everyone"), "{f:?}");
            assert!(!t.contains("hooks.example.com/services"), "{f:?}");
            assert!(t.contains(DOC_MARKER), "{f:?}");
            assert!(d.len() < 4000, "{f:?}");
        }
    }
    assert!(notice_text(&e).starts_with(&format!("{DOC_MARKER}\n[자율주행] ")));
}

// @Test("글 상한 — 줄이 많아도 전체 1,800자 안")
#[test]
fn 글_상한() {
    let n = Notice::new(
        NoticeKind::Summary,
        "summary-1",
        "머리",
        (0..30).map(|_| "가".repeat(200)).collect(),
        "",
        Default::default(),
    );
    assert!(grapheme_len(&notice_text(&n)) <= NOTIFY_TEXT_MAX + 4);
}

// ── 2/3 구간: 정한 때 · 요약 · 받을 webhook ─────────────────────────────────

// @Test("상태 알림의 정한 때" — 11개 견본)
#[test]
fn 상태_알림의_정한_때() {
    assert_eq!(state_notice("starting", "running", true, &[]), Some(NoticeKind::Start));
    assert_eq!(state_notice("", "running", true, &[]), Some(NoticeKind::Start));
    assert_eq!(state_notice("starting", "running", true, &["start".to_string()]), None); // 다시 뜬 엔진은 두 번 보내지 않음
    assert_eq!(state_notice("running", "running", true, &[]), None); // 단위가 바뀌어도
    assert_eq!(state_notice("running", "waiting_stopline", true, &["start".to_string()]), Some(NoticeKind::Wait));
    assert_eq!(state_notice("running", "waiting_limit", true, &["start".to_string()]), Some(NoticeKind::Wait));
    assert_eq!(state_notice("running", "waiting_precheck", true, &["start".to_string()]), Some(NoticeKind::Wait));
    assert_eq!(state_notice("running", "waiting_stopline", false, &["start".to_string()]), None); // 다시 확인하며 오가는 대기는 단위를 시작하기 전까지 한 번
    assert_eq!(state_notice("waiting_stopline", "waiting_stopline", true, &["start".to_string()]), None);
    assert_eq!(state_notice("running", "paused", true, &["start".to_string()]), None); // 사람이 한 일시정지는 알리지 않음
    assert_eq!(state_notice("paused", "running", true, &["start".to_string()]), None);
}

// @Test("끝 알림(webhook)은 실행마다 한 번")
#[test]
fn 끝_알림은_실행마다_한번() {
    assert!(end_webhook_due("done", &["start".to_string()]));
    assert!(end_webhook_due("error", &[]));
    assert!(!end_webhook_due("done", &["end".to_string()]));
    assert!(!end_webhook_due("running", &[]));
}

// @Test("3시간 요약의 정한 때" — 6개 견본)
#[test]
fn 요약의_정한_때() {
    assert!(!summary_due(None, None, 0.0)); // 도는 실행이 없음
    assert!(!summary_due(None, Some(T0), T0 + 3.0 * 3600.0 - 1.0));
    assert!(summary_due(None, Some(T0), T0 + 3.0 * 3600.0)); // 출발 3시간 뒤 첫 요약
    assert!(!summary_due(Some(T0 + 3.0 * 3600.0), Some(T0), T0 + 5.0 * 3600.0));
    assert!(summary_due(Some(T0 + 3.0 * 3600.0), Some(T0), T0 + 6.0 * 3600.0));
    assert!(!summary_due(Some(T0 - 10.0 * 3600.0), Some(T0), T0 + 1.0 * 3600.0)); // 앞 요약이 지금 실행들보다 앞이면 출발부터 다시 센다
}

// @Test("3시간 요약 글 — 도는 실행이 없으면 보내지 않음, 올릴 글·합치기 기다림·만료 임박")
#[test]
fn 요약_글() {
    assert_eq!(summary_notice(&[], 3, &[], &[], 1, T0, FixedZone::SEOUL), None);
    let exp = expiring_lines(
        &[
            Connection { id: "atlassian".to_string(), service: Service::Atlassian, expires: "2026-11-04".to_string(), ..Connection::default() },
            hook("webhook-1", NotifyFormat::Json, "팀 대화방"),
        ],
        T0,
        FixedZone::SEOUL,
    );
    assert_eq!(exp, ["Atlassian 토큰 만료 임박: 5일 남음"]);
    let n = summary_notice(
        &[notice_run_facts(&mention_run())],
        3,
        &[MergeWait { name: "todo-api <b>".to_string(), days: 2 }],
        &exp,
        4,
        T0,
        FixedZone::SEOUL,
    )
    .expect("요약 글");
    assert_eq!(n.id, "summary-4");
    assert!(n.lines.iter().any(|l| l == "검수를 기다리는 올릴 글 3개(보드의 '올릴 글'에서 봅니다)"));
    assert!(n.lines.iter().any(|l| l.starts_with("합치기 기다림: todo-api") && l.ends_with("2일")));
    assert!(n.lines.iter().any(|l| l == "Atlassian 토큰 만료 임박: 5일 남음"));
    assert!(n.lines[0].contains("도는 중 · 지금 쿠폰") && n.lines[0].contains("끝난 단위 1/2"));
    let v: Value = serde_json::from_slice(&notify_body(&n, NotifyFormat::Slack)).expect("요약 본문 JSON");
    golden("notify-summary-slack.json", &pretty(&v));
}

// @Test("받을 webhook — 게시 설정의 notify 가운데 알림 범위가 있는 webhook 연결만")
#[test]
fn 받을_webhook() {
    let mut no_scope = hook("webhook-3", NotifyFormat::Json, "팀 대화방");
    no_scope.scopes = Vec::new();
    let cs = vec![
        hook("webhook-1", NotifyFormat::Slack, "팀 대화방"),
        hook("webhook-2", NotifyFormat::Gchat, "팀 대화방"),
        no_scope,
        Connection { id: "atlassian".to_string(), service: Service::Atlassian, ..Connection::default() },
    ];
    let ids = vec!["webhook-2".to_string(), "webhook-3".to_string(), "webhook-9".to_string(), "atlassian".to_string()];
    let got: Vec<String> = notify_targets(Some(&ids), &cs).iter().map(|c| c.id.clone()).collect();
    assert_eq!(got, ["webhook-2"]);
    assert!(notify_targets(None, &cs).is_empty());
}

// ── 3/3 구간: 보내기 · 끝 알림 흐름 · 게시 설정 ─────────────────────────────

// @Test("보내기: 꼴마다 본문, 주소는 키체인에서 보내기 직전에, 인증 머리 없음, 결과에 주소 없음")
#[test]
fn 보내기() {
    let clock = PubClock(T0);
    let w = PubWaiter { waits: RefCell::new(Vec::new()) };
    let sec = PubSecrets(BTreeMap::from([
        ("webhook-1".to_string(), HOOK_URL.to_string()),
        ("webhook-2".to_string(), "http://127.0.0.1:9/hook/b".to_string()),
        ("webhook-3".to_string(), "http://example.com/hook".to_string()),
    ]));
    let h = PubHTTP::new(vec![
        HttpResponse { status: 429, headers: BTreeMap::from([("retry-after".to_string(), "1".to_string())]), ..HttpResponse::default() },
        HttpResponse { status: 200, ..HttpResponse::default() },
        HttpResponse { status: 302, headers: BTreeMap::from([("location".to_string(), "https://evil.example/x".to_string())]), ..HttpResponse::default() },
    ]);
    let n = test_notice("팀 대화방", T0, FixedZone::SEOUL);
    let r = send_notice(
        &n,
        &[
            hook("webhook-1", NotifyFormat::Slack, "팀 대화방"),
            hook("webhook-2", NotifyFormat::Gchat, "시험방"),
            hook("webhook-3", NotifyFormat::Json, "팀 대화방"),
            hook("webhook-4", NotifyFormat::Json, "팀 대화방"),
        ],
        true,
        &sec,
        &h,
        &clock,
        &w,
    );
    assert_eq!(r.iter().map(|s| s.ok).collect::<Vec<_>>(), [true, false, false, false]);
    assert_eq!(r[0].requests, 2);
    assert_eq!(w.waits.borrow().clone(), vec![1.0]);
    assert!(r[1].note.contains("따라가지 않았습니다(302)"));
    assert!(r[2].note.contains("규칙에 맞지 않아") && r[2].requests == 0); // 시험 빌드라도 http 는 127.0.0.1 만
    assert!(r[3].note.contains("키체인에 없어") && r[3].requests == 0);
    let got = h.got.borrow().clone();
    assert_eq!(got.len(), 3);
    assert!(got.iter().all(|q| q.method == "POST"));
    assert!(got.iter().all(|q| q.auth == Auth::None));
    let body = String::from_utf8(got[0].body.clone().expect("본문")).expect("본문 UTF-8");
    assert!(body.contains("\"unfurl_links\":false"));
    for s in &r {
        assert!(!s.note.contains("hooks.example.com") && !s.note.contains("127.0.0.1") && !s.note.contains("example.com"));
    }
    // 설치판은 http 를 늘 거절
    let r2 = send_notice(&n, &[hook("webhook-2", NotifyFormat::Gchat, "시험방")], false, &sec, &PubHTTP::new(vec![]), &clock, &w);
    assert_eq!(r2[0].requests, 0);
}

// @Test("끝 알림을 누르면 무엇을 여나" — 4개 견본)
#[test]
fn 끝_알림을_누르면_무엇을_여나() {
    assert_eq!(end_open(2, true), EndOpen::Review);
    assert_eq!(end_open(1, false), EndOpen::Review);
    assert_eq!(end_open(0, true), EndOpen::Merge);
    assert_eq!(end_open(0, false), EndOpen::History);
}

// @Test("끝 알림을 띄울 때 — 실행마다 한 번, 처음 띄울 때 이미 끝난 옛 실행은 아님" — 5개 견본)
#[test]
fn 끝_알림을_띄울_때() {
    assert!(end_notice_due(Some(T0 + 10.0), true, false, T0));
    assert!(!end_notice_due(Some(T0 + 10.0), true, true, T0));
    assert!(!end_notice_due(Some(T0 - 10.0), true, false, T0));
    assert!(!end_notice_due(Some(T0 + 10.0), false, false, T0)); // 끝 정리 전
    assert!(!end_notice_due(None, true, false, T0));
}

// @Test("나중에 뒤 하루 지나 다시 오는 합치기 알림" — 5개 견본)
#[test]
fn 나중에_합치기_알림() {
    assert!(merge_reminder_due(Some(T0), false, true, T0 + 1.0));
    assert!(!merge_reminder_due(Some(T0), false, true, T0 - 1.0));
    assert!(!merge_reminder_due(Some(T0), true, true, T0 + 1.0)); // 한 번만
    assert!(!merge_reminder_due(Some(T0), false, false, T0 + 1.0)); // 그사이 합침·버림
    assert!(!merge_reminder_due(None, false, true, T0 + 1.0));
}

// @Test("기다리는 올릴 글 수")
#[test]
fn 기다리는_올릴_글_수() {
    let a = OutboxItem { state: OutboxState::Waiting };
    let b = OutboxItem { state: OutboxState::Posted };
    let c = OutboxItem { state: OutboxState::Failed };
    let d = OutboxItem { state: OutboxState::Skipped };
    let e = OutboxItem { state: OutboxState::Uncertain };
    assert_eq!(outbox_waiting(&[a, b, c, d, e]), 3);
}

// @Test("끝난 실행이 합치기를 기다리는가(요약 줄)")
#[test]
fn 합치기를_기다리는가() {
    let ended = json!({"state": "done", "wrapped": T0, "branches": {"repos": {"/w/todo": {"work": "autopilot/x"}}}});
    let promo = json!([{"from": "autopilot", "into": "dev"}, {"from": "autopilot", "into": "live", "after": "dev"}]);
    let empty: Vec<Obj> = vec![Map::new()];
    assert!(run_merge_open(ended.as_object().expect("실행 객체"), &empty, &promo));
    let mut merged = ended.clone();
    merged["branches"] = json!({
        "repos": {"/w/todo": {"work": "autopilot/x"}},
        "promoted": [{"source": "autopilot", "into": "dev"}, {"source": "autopilot", "into": "live"}],
    });
    assert!(!run_merge_open(merged.as_object().expect("합친 객체"), &empty, &promo));
    let mut running = ended.clone();
    running["state"] = json!("running");
    assert!(!run_merge_open(running.as_object().expect("도는 객체"), &empty, &promo));
    let mut no_branch = ended.clone();
    no_branch["branches"] = json!({});
    assert!(!run_merge_open(no_branch.as_object().expect("가지 없는 객체"), &empty, &promo));
}

// @Test("게시 설정의 notify 와 연결의 글 꼴")
#[test]
fn 게시_설정의_notify() {
    let (p, probs) = publish_parse(Some(&json!({"mode": "review", "notify": ["webhook-2", "webhook-1", "webhook-2"]})));
    assert!(probs.is_empty() && p.notify == Some(vec!["webhook-2".to_string(), "webhook-1".to_string()]));
    assert_eq!(publish_parse(Some(&publish_json(&p))).0, p);
    assert_eq!(publish_parse(Some(&json!({"notify": ["atlassian"]}))).1.len(), 1);
    assert_eq!(publish_parse(Some(&json!({"notify": "webhook-1"}))).1.len(), 1);
    assert_eq!(publish_parse(Some(&json!({"mode": "auto"}))).0.notify, None);
    let list = vec![
        hook("webhook-1", NotifyFormat::Slack, "팀 대화방"),
        hook("webhook-2", NotifyFormat::Gchat, "팀 대화방"),
        Connection { id: "atlassian".to_string(), service: Service::Atlassian, ..Connection::default() },
    ];
    let back = parse_connections(Some(&serde_json::to_vec(&connections_json(&list)).expect("연결 JSON")));
    assert_eq!(back.iter().map(|c| c.format).collect::<Vec<_>>(), [NotifyFormat::Slack, NotifyFormat::Gchat, NotifyFormat::Json]);
    let mut raw = connections_json(&[hook("webhook-1", NotifyFormat::Json, "팀 대화방")]);
    if let Some(Value::Array(cs)) = raw.get_mut("connections") {
        if let Some(o) = cs[0].as_object_mut() {
            o.insert("format".to_string(), json!("teams"));
        }
    }
    let back = parse_connections(Some(&serde_json::to_vec(&raw).expect("연결 JSON")));
    assert_eq!(back[0].format, NotifyFormat::Json);
}

// ── R-Redact-b 보강: 가리기가 `meta::redact`(열아홉 꼴) 전부를 쓰는지 — 원본 NotifyTests 에 없는 시험이다 ──

// @Test("가리기(꼴 8): GitHub 토큰을 [가림:GitHub 토큰] 으로 — 20자 문턱(19자는 그대로)")
#[test]
fn extra_가린다_깃허브_토큰() {
    let token = format!("ghp_{}", "Qq7".repeat(12)); // 36자 — 20자 문턱을 넘는다
    let shortest = format!("ghp_{}", "Q8".repeat(10)); // 남는 글자 20자 — 가린다
    let short = format!("ghp_{}", "Q8".repeat(9) + "Q"); // 남는 글자 19자 — 가리지 않는다
    let s = notify_clean(&format!("열쇠는 {token} 과 {shortest} 이다"));
    assert!(s.contains("[가림:GitHub 토큰]"), "{s}");
    assert!(!s.contains("ghp_"), "{s}");
    assert!(s.contains("열쇠는") && s.contains("이다"), "{s}"); // 가린 뒤에도 앞뒤 문맥은 남는다
    let s2 = notify_clean(&format!("열쇠는 {short} 이다"));
    assert!(s2.contains("ghp_Q"), "{s2}");
    assert!(!s2.contains("[가림"), "{s2}");
}

// @Test("가리기(꼴 3·4): 알린 webhook 서비스·hooks?.호스트 — 옛 코드가 가리던 꼴 둘을 그대로 가린다")
#[test]
fn extra_가린다_webhook_둘_꼴() {
    let slack = "https://hooks.slack.com/services/T0AAA/B0BBB/cccccccccccccccccccccc";
    let any = format!("https://webhooks.ci.example.org/{}", "A9b8C7d6E5f4G3h2J1k9L0");
    let s = notify_clean(&format!("둘 다 {slack} 과 {any} 이다"));
    assert_eq!(s.matches("[가림:webhook 주소]").count(), 2, "{s}");
    assert!(!s.contains("services/T0AAA") && !s.contains("ci.example.org"), "{s}");
    assert!(s.contains("과") && s.contains("이다"), "{s}");
}

// @Test("가리기(꼴 5): 흔하지 않은 사이트의 hooks/ 긴 경로 — 옛 코드는 못 가렸다. 16자 문턱")
#[test]
fn extra_가린다_일반_hooks_경로() {
    let url = format!("https://ci.example.com/build/{}/{}", "hooks", "zZ9Y8X7W6V5U4T3S2R1Q0P9O8N7M6");
    let s = notify_clean(&format!("빌드 로그 {url} 에서 봤다"));
    assert!(s.contains("[가림:webhook 주소]"), "{s}");
    assert!(!s.contains("hooks/"), "{s}");
    let short = format!("https://ci.example.com/hooks/{}", "a".repeat(15)); // 문턱 아래 — 그대로
    let at = format!("https://ci.example.com/hooks/{}", "a".repeat(16)); // 문턱 — 가린다
    assert!(notify_clean(&format!("짧은 {short} 은 그대로")).contains("hooks/aaa"), "{short}");
    assert!(!notify_clean(&format!("긴 {at} 은 가린다")).contains("hooks/aaa"), "{at}");
}

// @Test("가리기: 비밀 아닌 평범한 주소는 그대로 둔다")
#[test]
fn extra_평범한_주소는_그대로() {
    let s = notify_clean("문서는 https://example.com/docs 이다");
    assert!(!s.contains("[가림"), "{s}");
    assert!(s.contains("https://example.com/docs"), "{s}");
}
