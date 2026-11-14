//! 바깥 연결 규칙(`rules::connections`) 시험.
//! 원본: `Tests/AutopilotRulesTests/PublishTests.swift` 의 `ConnectionRuleTests`(`@Suite("자율주행 연결: 주소·만료·키체인 이름")`)와
//! `ConnectionCheckTests`(`@Suite("자율주행 연결: 연결 확인(읽기 요청 하나)")`) — 둘 다 이 파일에 옮겼다.
//! 옮긴 시험(같은 이름): `사이트`(표 16줄) · `웹훅`(표 6줄) · `바탕` · `같은사이트` · `만료`(표 9줄) · `만료글` · `키체인` · `파일` · `인증` ·
//! `요청없음` · `확인`(표 3줄) · `실패`. 표는 Swift 시험 파일에서 변환 스크립트로 기계가 옮긴 것이다(글자 그대로).
//! 가짜 어댑터 `PubHTTP`·`PubSecrets` 는 Swift 시험의 같은 이름을 옮긴 것이다(`RefCell` 로 기록한다).
//! `NotifyTests.swift` 의 `settings` 는 게시 설정(`apPublishParse`)이 섞여 이 청크 밖이다. 그 가운데 연결의 글 꼴 왕복만 `extra_연결의_글_꼴_왕복` 으로 따로 옮겼다.
//! 끝의 `extra_` 시험은 Swift 시험에 없는 것(경계·비밀이 안 새는지·Windows 판단)이다.

use claude_board_autopilot::rules::connections::{
    ap_day, ap_expiry, auth_for, auth_header, check_connection, check_outcome, check_request, connection_id_ok, connections_json, expiry_label, parse_connections,
    parse_url, same_site, secret_service, service_base, site_check, url_host, webhook_check, Auth, AuthHeader, CheckOutcome, CheckRecord, ConnHttp, ConnSecrets,
    Connection, Day, Expiry, HttpRequest, HttpResponse, NotifyFormat, Service, SiteCheck, CONNECTIONS_FORMAT, FIGMA_API, NOTION_API, NOTION_VERSION,
};
use claude_board_autopilot::rules::stopline::Tz;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;

const FAKE_TOKEN: &str = "fake-token-ZZ9-not-real";

/// 가짜 키체인(원본 `PubSecrets`)
struct PubSecrets {
    items: BTreeMap<String, String>,
}

impl PubSecrets {
    fn new(items: &[(&str, &str)]) -> Self {
        PubSecrets { items: items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }
}

impl ConnSecrets for PubSecrets {
    fn read(&self, id: &str) -> Option<String> {
        self.items.get(id).cloned()
    }
    fn exists(&self, id: &str) -> bool {
        self.items.contains_key(id)
    }
}

/// 가짜 HTTP(원본 `PubHTTP`) — 차례대로 정한 답을 돌려주고 받은 요청과 인증 머리를 남긴다(남는 답이 없으면 200 `{}`)
#[derive(Default)]
struct PubHttp {
    answers: RefCell<Vec<HttpResponse>>,
    got: RefCell<Vec<HttpRequest>>,
    auths: RefCell<Vec<String>>,
}

impl PubHttp {
    fn new(answers: Vec<HttpResponse>) -> Self {
        PubHttp { answers: RefCell::new(answers), ..PubHttp::default() }
    }
}

impl ConnHttp for PubHttp {
    fn send(&self, request: &HttpRequest, auth: Option<&AuthHeader>) -> HttpResponse {
        self.got.borrow_mut().push(request.clone());
        self.auths.borrow_mut().push(auth.map(|a| format!("{}: {}", a.name, a.value)).unwrap_or_default());
        let mut answers = self.answers.borrow_mut();
        if answers.is_empty() {
            HttpResponse { status: 200, body: b"{}".to_vec(), ..HttpResponse::default() }
        } else {
            answers.remove(0)
        }
    }
}

fn reply(status: i32, body: &str) -> HttpResponse {
    HttpResponse { status, body: body.as_bytes().to_vec(), ..HttpResponse::default() }
}

const SITE_ROWS: &[(&str, &str, bool, &str)] = &[
    ("Atlassian 클라우드", "https://example.atlassian.net", false, "ok https://example.atlassian.net"),
    ("끝 빗금·대문자는 다듬음", " HTTPS://Example.Atlassian.net/ ", false, "ok https://example.atlassian.net"),
    ("/wiki 까지 넣어도 호스트만", "https://example.atlassian.net/wiki", false, "ok https://example.atlassian.net"),
    ("포트는 남김", "https://jira.example.com:8443", false, "ok https://jira.example.com:8443"),
    ("설치판 http 는 거절", "http://example.atlassian.net", false, "bad https 주소만 받습니다"),
    ("설치판 127.0.0.1 도 거절", "http://127.0.0.1:8080", false, "bad https 주소만 받습니다"),
    ("시험 빌드는 http://127.0.0.1 만", "http://127.0.0.1:8080", true, "ok http://127.0.0.1:8080"),
    ("시험 빌드라도 localhost 는 거절", "http://localhost:8080", true, "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다"),
    ("시험 빌드라도 다른 http 는 거절", "http://10.0.0.1", true, "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다"),
    ("아이디·암호 든 주소", "https://user:pw@example.atlassian.net", false, "bad 주소에 아이디·암호를 넣지 않습니다"),
    ("경로", "https://example.atlassian.net/jira/x", false, "bad 사이트 주소는 호스트까지만 씁니다(경로 없이)"),
    ("물음", "https://example.atlassian.net/?a=1", false, "bad 사이트 주소에 ? 나 # 을 넣지 않습니다"),
    ("다른 꼴", "ftp://example.com", true, "bad https 주소만 받습니다"),
    ("빈칸 든 주소", "https://exa mple.com", false, "bad 주소에 쓸 수 없는 글자가 있습니다"),
    ("빈 글자", "  ", false, "bad 주소가 비었습니다"),
    ("꼴 아님", "example.atlassian.net", false, "bad 주소 꼴이 아닙니다(https://… 로 씁니다)"),
];
const WEBHOOK_ROWS: &[(&str, &str, bool, bool)] = &[
    ("Slack 꼴", "https://hooks.slack.com/services/T0/B0/xyz", false, true),
    ("물음에 열쇠", "https://chat.googleapis.com/v1/spaces/x/messages?key=a&token=b", false, true),
    ("경로 없음", "https://hooks.example.com", false, false),
    ("설치판 http", "http://127.0.0.1:9/hook", false, false),
    ("시험 빌드 127.0.0.1", "http://127.0.0.1:9/hook", true, true),
    ("조각", "https://hooks.example.com/a#b", false, false),
];

fn site_text(c: &SiteCheck) -> String {
    match c {
        SiteCheck::Ok(s) => format!("ok {s}"),
        SiteCheck::Bad(s) => format!("bad {s}"),
    }
}

fn atl() -> Connection {
    Connection {
        id: "atlassian".into(),
        service: Service::Atlassian,
        site: "https://example.atlassian.net".into(),
        account: "kim@example.com".into(),
        scopes: vec!["jira".into(), "confluence".into()],
        expires: "2027-01-31".into(),
        ..Connection::default()
    }
}

fn conn(id: &str, service: Service, site: &str) -> Connection {
    Connection { id: id.into(), service, site: site.into(), ..Connection::default() }
}

fn pretty(m: &serde_json::Map<String, Value>) -> String {
    serde_json::to_string_pretty(m).unwrap() + "\n"
}

#[test]
fn 사이트() {
    for (note, raw, dev, want) in SITE_ROWS {
        assert_eq!(site_text(&site_check(raw, *dev)), *want, "{note}: {raw}");
    }
}

#[test]
fn 웹훅() {
    for (note, raw, dev, ok) in WEBHOOK_ROWS {
        assert_eq!(matches!(webhook_check(raw, *dev), SiteCheck::Ok(_)), *ok, "{note}: {raw}");
    }
}

#[test]
fn 바탕() {
    let notion = Connection { id: "notion".into(), service: Service::Notion, account: "김요청".into(), scopes: vec!["write".into()], ..Connection::default() };
    assert_eq!(service_base(&atl(), false).as_deref(), Some("https://example.atlassian.net"));
    assert_eq!(service_base(&conn("atlassian", Service::Atlassian, "http://127.0.0.1:9"), false), None);
    assert_eq!(service_base(&conn("atlassian", Service::Atlassian, "http://127.0.0.1:9"), true).as_deref(), Some("http://127.0.0.1:9"));
    assert_eq!((NOTION_API, FIGMA_API), ("https://api.notion.com", "https://api.figma.com")); // 요청이 가는 곳이라 글자로 못박는다
    assert_eq!(service_base(&notion, false).as_deref(), Some(NOTION_API));
    assert_eq!(service_base(&conn("notion", Service::Notion, "http://127.0.0.1:9"), false).as_deref(), Some(NOTION_API));
    assert_eq!(service_base(&conn("notion", Service::Notion, "http://127.0.0.1:9"), true).as_deref(), Some("http://127.0.0.1:9"));
    assert_eq!(service_base(&conn("figma", Service::Figma, "https://evil.example"), true).as_deref(), Some(FIGMA_API));
    assert_eq!(service_base(&conn("webhook-1", Service::Webhook, ""), true), None);
}

#[test]
fn 같은사이트() {
    assert!(same_site("https://example.atlassian.net/rest/api/3/myself", "https://example.atlassian.net"));
    assert!(!same_site("https://evil.example/rest", "https://example.atlassian.net"));
    assert!(!same_site("http://example.atlassian.net/rest", "https://example.atlassian.net"));
    assert!(!same_site("http://127.0.0.1:9/x", "http://127.0.0.1:8"));
    assert!(!same_site("https://u:p@example.atlassian.net/x", "https://example.atlassian.net"));
}

#[test]
fn 만료() {
    // 시험 시각: 2026-10-30 13:40 서울(= 04:40 UTC, 같은 날이라 `Tz::Utc` 로 센다)
    let now = 1_793_340_000.0;
    for (ex, want) in [
        ("", "none"),
        ("2026-13-01", "none"),
        ("2027-02-30", "none"),
        ("2027-01-31", "ok 93"),
        ("2026-11-13", "soon 14"),
        ("2026-11-14", "ok 15"),
        ("2026-10-30", "soon 0"),
        ("2026-10-29", "expired 1"),
        ("2026-01-01", "expired 302"),
    ] {
        let got = match ap_expiry(ex, now, Tz::Utc) {
            Expiry::None => "none".to_string(),
            Expiry::Ok(d) => format!("ok {d}"),
            Expiry::Soon(d) => format!("soon {d}"),
            Expiry::Expired(d) => format!("expired {d}"),
        };
        assert_eq!(got, want, "{ex}");
    }
}

#[test]
fn 만료글() {
    assert_eq!(expiry_label(Expiry::Soon(0), "2026-10-30"), "오늘 만료(2026-10-30)");
    assert_eq!(expiry_label(Expiry::Expired(2), "2026-10-28"), "만료됨 · 2일 지남(2026-10-28)");
    assert!(!expiry_label(Expiry::Ok(3), "x").contains('—'));
}

#[test]
fn 키체인() {
    assert_eq!(secret_service(false), "Claude Board-connection");
    assert_eq!(secret_service(true), "Claude Board Dev-connection");
    assert_ne!(secret_service(false), "Claude Board-account"); // 계정 전환 저장 계정과 다른 항목
    for (id, okk) in [
        ("atlassian", true),
        ("notion", true),
        ("figma", true),
        ("webhook-1", true),
        ("webhook-120", true),
        ("webhook-0", false),
        ("webhook-1000", false),
        ("jira", false),
        ("Atlassian", false),
        ("../x", false),
    ] {
        assert_eq!(connection_id_ok(id), okk, "{id}");
    }
}

const FILE_RAW: &str = r##"{"format":"claude-board-connections","version":1,"connections":[
 {"id":"atlassian","service":"atlassian","site":"https://example.atlassian.net","account":"kim@example.com","scopes":["jira","bogus"],"expires":"2027-01-31","token":"새면 안 됨"},
 {"id":"atlassian","service":"atlassian","site":"https://second.example"},
 {"id":"notion","service":"figma"},
 {"id":"webhook-1","service":"webhook","name":"팀\u202e방","host":"hooks.example.com","expires":"내일"},
 {"id":"../x","service":"notion"}]}"##;

#[test]
fn 파일() {
    let l = parse_connections(Some(FILE_RAW.as_bytes()));
    assert_eq!(l.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["atlassian", "webhook-1"]);
    assert!(l[0].scopes == ["jira"] && l[0].site == "https://example.atlassian.net");
    assert!(l[1].name == "팀방" && l[1].expires.is_empty());
    let back = pretty(&connections_json(&l));
    assert!(!back.contains("token") && !back.contains("새면"));
    assert_eq!(parse_connections(Some(back.as_bytes())), l);
    assert!(parse_connections(Some(br#"{"format":"x"}"#)).is_empty());
}

fn header(h: Option<AuthHeader>) -> Option<(String, String)> {
    h.map(|h| (h.name, h.value))
}

#[test]
fn 인증() {
    // 원본 `Data("kim@example.com:t".utf8).base64EncodedString()` 의 값(base64 "kim@example.com:t")
    let basic = "Basic a2ltQGV4YW1wbGUuY29tOnQ=";
    assert_eq!(header(auth_header(&Auth::Basic { user: "kim@example.com".into() }, "t")), Some(("Authorization".to_string(), basic.to_string())));
    assert_eq!(header(auth_header(&Auth::Bearer, "t")), Some(("Authorization".to_string(), "Bearer t".to_string())));
    assert_eq!(header(auth_header(&Auth::Figma, "t")), Some(("X-Figma-Token".to_string(), "t".to_string())));
    assert_eq!(auth_header(&Auth::None, "t"), None);
}

#[test]
fn 요청없음() {
    let h = PubHttp::default();
    assert_eq!(
        check_connection(&atl(), false, &PubSecrets::new(&[]), &h),
        CheckOutcome::Local("토큰이 키체인에 없어 요청하지 않았습니다".to_string())
    );
    let local = Connection { account: "a@b".into(), ..conn("atlassian", Service::Atlassian, "http://127.0.0.1:9") };
    assert_eq!(
        check_connection(&local, false, &PubSecrets::new(&[("atlassian", FAKE_TOKEN)]), &h),
        CheckOutcome::Local("사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다".to_string())
    );
    let hook = conn("webhook-1", Service::Webhook, "");
    assert!(
        matches!(check_connection(&hook, true, &PubSecrets::new(&[("webhook-1", "https://x/y")]), &h), CheckOutcome::Local(_)),
        "webhook 은 요청 없이"
    );
    assert!(h.got.borrow().is_empty());
}

const CHECK_ROWS: &[(&str, &str, &str, &str)] = &[
    ("atlassian", "/rest/api/3/myself", "{\"displayName\":\"김 시험\"}", "김 시험"),
    ("notion", "/v1/users/me", "{\"name\":\"Claude Board\",\"bot\":{\"owner\":{\"user\":{\"name\":\"김\"}}}}", "Claude Board · 김"),
    ("figma", "/v1/me", "{\"email\":\"kim@example.com\"}", "kim@example.com"),
];

#[test]
fn 확인() {
    for (id, suffix, body, who) in CHECK_ROWS {
        let c = Connection {
            account: "kim@example.com".into(),
            ..conn(id, Service::from_raw(id).unwrap(), if *id == "atlassian" { "https://example.atlassian.net" } else { "" })
        };
        let h = PubHttp::new(vec![reply(200, body)]);
        assert_eq!(check_connection(&c, false, &PubSecrets::new(&[(id, FAKE_TOKEN)]), &h), CheckOutcome::Ok { who: who.to_string() }, "{id}");
        let got = h.got.borrow();
        assert!(got.len() == 1 && got[0].method == "GET" && got[0].url.ends_with(suffix), "{id}");
        assert!(!got[0].headers.values().any(|v| v.contains(FAKE_TOKEN)), "토큰은 요청 값이 아니라 인증 머리로만");
        let want = if *id == "atlassian" { "a2ltQGV4YW1wbGUuY29tOmZha2UtdG9rZW4tWlo5LW5vdC1yZWFs" } else { FAKE_TOKEN };
        assert!(h.auths.borrow()[0].contains(want), "{id}: {}", h.auths.borrow()[0]);
    }
}

#[test]
fn 실패() {
    for (st, want) in [(401, "rejected"), (403, "rejected"), (302, "failed"), (429, "failed"), (500, "failed")] {
        let h = PubHttp::new(vec![reply(st, "")]);
        let o = check_connection(&atl(), false, &PubSecrets::new(&[("atlassian", FAKE_TOKEN)]), &h);
        match o {
            CheckOutcome::Rejected(_) => assert_eq!(want, "rejected", "{st}"),
            CheckOutcome::Failed(_) => assert_eq!(want, "failed", "{st}"),
            other => panic!("{st}: {other:?}"),
        }
    }
}

// ── extra_: Swift 시험에 없는 것 ─────────────────────

#[test]
fn extra_연결의_글_꼴_왕복() {
    // 원본 `NotifyTests.settings` 의 연결 쪽 줄
    let hook = |id: &str, f: NotifyFormat| Connection { id: id.into(), service: Service::Webhook, format: f, ..Connection::default() };
    let list = vec![hook("webhook-1", NotifyFormat::Slack), hook("webhook-2", NotifyFormat::Gchat), conn("atlassian", Service::Atlassian, "")];
    let json = serde_json::to_vec(&connections_json(&list)).unwrap();
    let back = parse_connections(Some(&json));
    assert_eq!(back.iter().map(|c| c.format).collect::<Vec<_>>(), [NotifyFormat::Slack, NotifyFormat::Gchat, NotifyFormat::Json]);
    let mut raw = connections_json(&[hook("webhook-1", NotifyFormat::Json)]);
    let Some(Value::Array(cs)) = raw.get_mut("connections") else { panic!("connections") };
    cs[0]["format"] = Value::from("teams");
    let json = serde_json::to_vec(&raw).unwrap();
    assert_eq!(parse_connections(Some(&json)).first().map(|c| c.format), Some(NotifyFormat::Json));
}

#[test]
fn extra_비밀은_값에도_출력에도_없다() {
    // `Connection` 에는 비밀 칸이 없다: 파일에 `token`·`secret`·`url` 칸이 있어도 값·Debug·JSON 어디에도 남지 않는다
    let raw = r##"{"format":"claude-board-connections","version":1,"connections":[
     {"id":"webhook-1","service":"webhook","name":"방","host":"hooks.example.com","url":"https://hooks.example.com/SECRET-PATH?key=SECRET-KEY","token":"SECRET-TOKEN","password":"SECRET-PW"},
     {"id":"notion","service":"notion","secret":"SECRET-TOKEN","checked":{"at":5,"ok":true,"who":"x","note":"y","token":"SECRET-TOKEN"}}]}"##;
    let l = parse_connections(Some(raw.as_bytes()));
    assert_eq!(l.len(), 2);
    let shown = format!("{l:?}");
    let json = pretty(&connections_json(&l));
    for out in [&shown, &json] {
        assert!(!out.contains("SECRET"), "{out}");
    }
    assert_eq!(l[0].host, "hooks.example.com");
    assert_eq!(l[1].checked, Some(CheckRecord { at: 5.0, ok: true, who: "x".into(), note: "y".into() }));
    // 결과 값은 같은 입력에서 같은 모양으로 다시 읽힌다
    assert_eq!(parse_connections(Some(json.as_bytes())), l);
}

#[test]
fn extra_연결_json_모양() {
    let c = Connection { checked: Some(CheckRecord { at: 1_793_000_000.0, ok: false, who: "w".into(), note: "n".into() }), ..atl() };
    let m = connections_json(&[c, conn("webhook-1", Service::Webhook, "")]);
    assert_eq!(m["format"], CONNECTIONS_FORMAT);
    assert_eq!(m["version"], 1);
    let Value::Array(rows) = &m["connections"] else { panic!("connections") };
    // 빈 칸은 쓰지 않고, webhook 만 format 을 쓴다
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&rows[0]), ["id", "service", "scopes", "site", "account", "expires", "checked"]);
    assert_eq!(keys(&rows[1]), ["id", "service", "scopes", "format"]);
    assert_eq!(rows[0]["checked"]["at"], 1_793_000_000_i64);
    assert_eq!(rows[1]["format"], "json");
}

#[test]
fn extra_파일_경계() {
    let wrap = |list: &str| format!(r#"{{"format":"claude-board-connections","connections":{list}}}"#);
    // 객체 아닌 원소가 하나라도 있으면 통째로 빈 목록
    assert!(parse_connections(Some(wrap(r#"[{"id":"notion","service":"notion"}, 3]"#).as_bytes())).is_empty());
    assert_eq!(parse_connections(Some(wrap(r#"[{"id":"notion","service":"notion"}]"#).as_bytes())).len(), 1);
    // 읽지 못하는 입력
    assert!(parse_connections(None).is_empty());
    assert!(parse_connections(Some(b"")).is_empty());
    assert!(parse_connections(Some(b"[1]")).is_empty());
    assert!(parse_connections(Some(wrap("{}").as_bytes())).is_empty());
    // 256 KiB 를 넘으면 빈 목록
    let pad = " ".repeat(256 << 10);
    assert!(parse_connections(Some(format!("{}{pad}", wrap(r#"[{"id":"notion","service":"notion"}]"#)).as_bytes())).is_empty());
    // 서비스와 id 가 맞아야 한다
    let l = parse_connections(Some(
        wrap(r#"[{"id":"webhook-2","service":"notion"},{"id":"notion","service":"webhook"},{"id":"figma","service":"figma"}]"#).as_bytes(),
    ));
    assert_eq!(l.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["figma"]);
    // 같은 id 는 첫 줄만(서비스가 어긋나 버려진 줄의 id 도 쓴 것으로 센다)
    let l = parse_connections(Some(wrap(r#"[{"id":"notion","service":"figma"},{"id":"notion","service":"notion"}]"#).as_bytes()));
    assert!(l.is_empty());
    // 줄 수 50 까지만
    let many: Vec<String> = (1..=60).map(|i| format!(r#"{{"id":"webhook-{i}","service":"webhook"}}"#)).collect();
    assert_eq!(parse_connections(Some(wrap(&format!("[{}]", many.join(","))).as_bytes())).len(), 50);
    // 범위 밖 scopes 는 걸러내되 차례를 지킨다. 칸 길이는 자른다. 없는 날은 빈 만료일
    let long = "가".repeat(400);
    let l = parse_connections(Some(
        wrap(&format!(r#"[{{"id":"atlassian","service":"atlassian","scopes":["read","x",7,"jira"],"site":"{long}","expires":"2027-02-30"}}]"#)).as_bytes(),
    ));
    assert_eq!(l[0].scopes, ["read", "jira"]);
    assert_eq!(l[0].site.chars().count(), 300);
    assert_eq!(l[0].expires, "");
    // webhook 이 아닌 연결은 format 을 읽지 않는다
    let l = parse_connections(Some(wrap(r#"[{"id":"notion","service":"notion","format":"slack"}]"#).as_bytes()));
    assert_eq!(l[0].format, NotifyFormat::Json);
    // 앞뒤 빈칸·제어·방향 글자
    let l = parse_connections(Some(wrap(r#"[{"id":"notion","service":"notion","account":"\u00a0\u3000 김\u0007요\u2066청 \t"}]"#).as_bytes()));
    assert_eq!(l[0].account, "김요청");
    // 지우는 제어 글자의 양 끝: U+001F·U+007F·U+009F 는 지우고 U+00A1 은 남는다
    let l = parse_connections(Some(wrap(r#"[{"id":"notion","service":"notion","account":"a\u001fb\u007fc\u009fd¡e"}]"#).as_bytes()));
    assert_eq!(l[0].account, "abcd\u{a1}e");
    // 이름은 80 글자, 계정은 200 글자, 만료일은 10 글자까지
    let l = parse_connections(Some(wrap(&format!(r#"[{{"id":"webhook-1","service":"webhook","name":"{}","account":"{}"}}]"#, "n".repeat(81), "a".repeat(201))).as_bytes()));
    assert_eq!((l[0].name.len(), l[0].account.len()), (80, 200));
    // 만료일의 달력: 윤년만 2월 29일(400 으로 나눠지는 해는 윤년, 100 으로만 나눠지는 해는 아니다)
    for (day, want) in [
        ("2000-02-29", "2000-02-29"),
        ("2100-02-29", ""),
        ("2028-02-29", "2028-02-29"),
        ("2027-02-29", ""),
        ("2027-04-31", ""),
        ("2027-12-31", "2027-12-31"),
        ("1999-12-31", ""),
        ("2201-01-01", ""),
        ("2027-1-31", ""),
    ] {
        let l = parse_connections(Some(wrap(&format!(r#"[{{"id":"notion","service":"notion","expires":"{day}"}}]"#)).as_bytes()));
        assert_eq!(l[0].expires, want, "{day}");
    }
    // checked 의 수·참거짓 읽기(Foundation NSNumber 규칙)
    let l = parse_connections(Some(
        wrap(r#"[{"id":"notion","service":"notion","checked":{"at":true,"ok":1}},{"id":"figma","service":"figma","checked":{"at":"x","ok":2}}]"#).as_bytes(),
    ));
    assert_eq!(l[0].checked, Some(CheckRecord { at: 1.0, ok: true, who: String::new(), note: String::new() }));
    assert_eq!(l[1].checked, Some(CheckRecord::default()));
}

#[test]
fn extra_연결_id_경계() {
    for id in ["", "webhook-", "webhook-01", "webhook-1 ", " webhook-1", "figma\n", "atlassian\r\n", "notion/", "webhook-١", "Webhook-1", "webhook-9999"] {
        assert!(!connection_id_ok(id), "{id:?}");
    }
    for id in ["webhook-999", "webhook-10", "webhook-9"] {
        assert!(connection_id_ok(id), "{id}");
    }
}

#[test]
fn extra_서비스_표() {
    let names: Vec<_> = Service::ALL.iter().map(|s| (s.raw(), s.label())).collect();
    assert_eq!(names, [("atlassian", "Atlassian"), ("notion", "Notion"), ("figma", "Figma"), ("webhook", "알림 webhook")]);
    for s in Service::ALL {
        assert_eq!(Service::from_raw(s.raw()), Some(s));
    }
    assert_eq!(Service::from_raw("Atlassian"), None);
    assert_eq!(Service::Atlassian.scope_choices().iter().map(|c| c.0).collect::<Vec<_>>(), ["jira", "confluence", "read"]);
    assert_eq!(Service::Notion.scope_choices()[0], ("write", "Notion 쓰기(보고서)"));
    assert_eq!(Service::Figma.scope_choices(), [("read", "읽기")]);
    assert_eq!(Service::Webhook.scope_choices(), [("notify", "알림")]);
    assert_eq!(NotifyFormat::ALL.map(|f| (f.raw(), f.label())), [("json", "일반 JSON"), ("slack", "Slack 꼴"), ("gchat", "Google Chat 꼴")]);
    assert_eq!(Connection::default().service, Service::Atlassian);
    assert_eq!(Connection::default().format, NotifyFormat::Json);
}

#[test]
fn extra_사이트_경계() {
    let t = |raw: &str, dev: bool| site_text(&site_check(raw, dev));
    // 경로: /wiki 는 받고 /wikis·/wiki/x 는 거절, 퍼센트로 쓴 같은 글자는 풀어서 본다
    assert_eq!(t("https://a.atlassian.net/wiki/", false), "ok https://a.atlassian.net");
    assert_eq!(t("https://a.atlassian.net/%77iki", false), "ok https://a.atlassian.net");
    assert_eq!(t("https://a.atlassian.net/wikis", false), "bad 사이트 주소는 호스트까지만 씁니다(경로 없이)");
    assert_eq!(t("https://a.atlassian.net/wiki/x", false), "bad 사이트 주소는 호스트까지만 씁니다(경로 없이)");
    // 질의·조각(빈 것도)
    assert_eq!(t("https://a.atlassian.net?", false), "bad 사이트 주소에 ? 나 # 을 넣지 않습니다");
    assert_eq!(t("https://a.atlassian.net#x", false), "bad 사이트 주소에 ? 나 # 을 넣지 않습니다");
    assert_eq!(t("https://a.atlassian.net/?a#b", false), "bad 사이트 주소에 ? 나 # 을 넣지 않습니다");
    // 아이디·암호는 빈 것도 거절
    for raw in ["https://u@a.atlassian.net", "https://:p@a.atlassian.net", "https://@a.atlassian.net", "https://u:p@127.0.0.1"] {
        assert_eq!(t(raw, true), "bad 주소에 아이디·암호를 넣지 않습니다", "{raw}");
    }
    // 글자 틀: 따옴표·꺾쇠·역슬래시·중괄호·구분선·^·백틱·제어·비ASCII 는 모두 같은 글로
    for raw in [
        "https://a.net/\"",
        "https://a.net/'",
        "https://a.net/<",
        "https://a.net\\x",
        "https://a.net/`",
        "https://a.net/{",
        "https://a.net/|",
        "https://a.net/^",
        "https://a.net/\u{7f}",
        "https://한글.net",
        "https://a.net/\tb",
        "https://a.net/\u{0}",
    ] {
        assert_eq!(t(raw, false), "bad 주소에 쓸 수 없는 글자가 있습니다", "{raw:?}");
    }
    // 길이: 2000 글자까지
    let host = format!("https://{}.example.com", "a".repeat(1900));
    assert_eq!(t(&host, false), format!("ok {host}"));
    assert_eq!(t(&format!("https://{}.example.com", "a".repeat(2000)), false), "bad 주소에 쓸 수 없는 글자가 있습니다");
    let exact = format!("https://{}.net", "a".repeat(1988)); // 정확히 2000 글자
    assert_eq!(exact.len(), 2000);
    assert_eq!(t(&exact, false), format!("ok {exact}"));
    assert_eq!(t(&format!("{exact}a"), false), "bad 주소에 쓸 수 없는 글자가 있습니다");
    // 앞뒤: Zs·줄바꿈은 다듬는다
    assert_eq!(t("\u{a0}\n https://a.net\r\n\u{2028}", false), "ok https://a.net");
    // 대소문자: 스킴·호스트는 소문자로, 포트 앞자리 0 은 수로
    assert_eq!(t("HTTP://127.0.0.1:08080", true), "ok http://127.0.0.1:8080");
    assert_eq!(t("https://A.NET:443", false), "ok https://a.net:443");
    // 꼴: 빈 호스트·스킴만·IP 리터럴·틀린 포트·퍼센트
    for raw in [
        "https:///x",
        "https://",
        "https:a.net",
        "https:/a.net",
        "https://[::1]",
        "https://a.net:",
        "https://a.net:x",
        "https://a.net:65536",
        "https://a.net:123456",
        "https://a%41.net",
        "https://a.net/%zz",
        "https://a.net/%4",
        "1http://a.net",
        "://a.net",
        "https://a@b@c.net",
        "https://a.net/[",
        "https://a.net/?[",
        "https://a.net:80:90",
    ] {
        assert_eq!(t(raw, false), "bad 주소 꼴이 아닙니다(https://… 로 씁니다)", "{raw}");
    }
    // 시험 빌드의 127.0.0.1 은 정확히 그 글자만(접두 속임수 거절)
    assert_eq!(t("http://127.0.0.1.evil.com", true), "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다");
    assert_eq!(t("http://127.0.0.10", true), "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다");
    assert_eq!(t("http://[::1]:9", true), "bad 주소 꼴이 아닙니다(https://… 로 씁니다)");
}

#[test]
fn extra_웹훅_경계() {
    let w = |raw: &str, dev: bool| site_text(&webhook_check(raw, dev));
    // 받은 글자를 앞뒤만 다듬어 그대로 돌려준다(대소문자·경로·물음 그대로)
    assert_eq!(w(" HTTPS://Hooks.Example.com/Services/T0?Key=A \n", false), "ok HTTPS://Hooks.Example.com/Services/T0?Key=A");
    assert_eq!(w("https://hooks.example.com/", false), "bad webhook 주소에 경로가 없습니다");
    assert_eq!(w("https://hooks.example.com%2F", false), "bad 주소 꼴이 아닙니다(https://… 로 씁니다)");
    assert_eq!(w("https://hooks.example.com?key=a", false), "bad webhook 주소에 경로가 없습니다");
    assert_eq!(w("https://hooks.example.com/a?b#c", false), "bad webhook 주소에 # 을 넣지 않습니다");
    assert_eq!(w("https://hooks.example.com/a#", false), "bad webhook 주소에 # 을 넣지 않습니다");
    assert_eq!(w("https://u:p@hooks.example.com/a", false), "bad 주소에 아이디·암호를 넣지 않습니다");
    assert_eq!(w("http://hooks.example.com/a", true), "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다");
    assert_eq!(w("", false), "bad 주소가 비었습니다");
    // 경로는 퍼센트를 풀어 본다: `/%2F` 는 `//` 라 경로가 있는 것이다(받은 글자는 그대로 돌려준다)
    assert_eq!(w("https://hooks.example.com/%2F", false), "ok https://hooks.example.com/%2F");
}

#[test]
fn extra_주소_칸_읽기와_호스트() {
    let p = parse_url("https://User:Pw@Example.com:8443/a%20b/c?x=1&y#frag").unwrap();
    assert_eq!(p.scheme.as_deref(), Some("https"));
    assert_eq!((p.user.as_deref(), p.password.as_deref()), (Some("User"), Some("Pw")));
    assert_eq!((p.host.as_deref(), p.port), (Some("Example.com"), Some(8443)));
    assert_eq!(p.path, "/a b/c");
    assert_eq!((p.query.as_deref(), p.fragment.as_deref()), (Some("x=1&y"), Some("frag")));
    let q = parse_url("http://127.0.0.1").unwrap();
    assert_eq!((q.user, q.password, q.port, q.path.as_str(), q.query, q.fragment), (None, None, None, "", None, None));
    assert_eq!(parse_url("https://u@h").unwrap().password, None);
    assert_eq!(url_host("HTTPS://Hooks.Example.COM/x?y"), "hooks.example.com");
    assert_eq!(url_host("https://a.net:9/x"), "a.net");
    assert_eq!(url_host("not a url"), "");
    assert_eq!(url_host(""), "");
    assert_eq!(url_host("example.com"), "");
}

#[test]
fn extra_windows_경로는_주소로_받지_않는다() {
    // 이 규칙은 `/` 로 맞춘 글자만 본다. 자격 증명·보관소 경로(`C:\Users\x\.claude\…`)가 주소 칸에 들어오면 틀린 글자로 걸린다
    for raw in [r"C:\Users\me\.claude\notifier\x", r"\\server\share\x", r"https://a.net\x"] {
        assert_eq!(site_text(&site_check(raw, false)), "bad 주소에 쓸 수 없는 글자가 있습니다", "{raw}");
        assert_eq!(site_text(&webhook_check(raw, true)), "bad 주소에 쓸 수 없는 글자가 있습니다", "{raw}");
    }
    // `/` 로 바꾼 같은 경로는 스킴(`c`)만 있는 꼴이라 「주소 꼴이 아님」이다
    assert_eq!(site_text(&site_check("C:/Users/me/.claude/notifier/x", false)), "bad 주소 꼴이 아닙니다(https://… 로 씁니다)");
    assert_eq!(url_host(r"C:\Users\me"), "");
}

// ── extra_: 후반(만료일·요청 값·인증 머리·연결 확인) ────

#[test]
fn extra_날짜_읽기() {
    assert_eq!(ap_day("2026-10-30"), Some(Day { year: 2026, month: 10, day: 30 }));
    for ok in ["2000-01-01", "2200-12-31", "2028-02-29", "2000-02-29", "2200-02-28", "2026-04-30", "2026-12-31"] {
        assert!(ap_day(ok).is_some(), "{ok}");
    }
    for bad in [
        "",
        "1999-12-31",
        "2201-01-01",
        "2026-00-10",
        "2026-13-01",
        "2026-01-00",
        "2026-01-32",
        "2027-02-29",
        "2100-02-29",
        "2200-02-29",
        "2026-04-31",
        "2026-06-31",
        "2026-09-31",
        "2026-11-31",
        "2026-1-01",
        "2026-01-1",
        "2026/01/01",
        "2026-01-01\n",
        " 2026-01-01",
        "2026-01-011",
        "２０２６-01-01",
        "2026-0a-01",
        "+026-01-01",
    ] {
        assert_eq!(ap_day(bad), None, "{bad:?}");
    }
}

#[test]
fn extra_만료_경계() {
    let t = |s: &str, now: f64| ap_expiry(s, now, Tz::Utc);
    let midnight = 1_793_318_400.0; // 2026-10-30 00:00:00 UTC
    assert_eq!(t("2026-10-30", midnight), Expiry::Soon(0));
    assert_eq!(t("2026-10-30", midnight + 86_399.0), Expiry::Soon(0));
    assert_eq!(t("2026-10-30", midnight + 86_400.0), Expiry::Expired(1));
    assert_eq!(t("2026-10-30", midnight - 1.0), Expiry::Soon(1));
    // 14일은 임박, 15일은 아직(EXPIRY_SOON_DAYS 의 경계)
    assert_eq!(t("2026-11-13", midnight), Expiry::Soon(14));
    assert_eq!(t("2026-11-14", midnight), Expiry::Ok(15));
    // 해를 건너는 날 수, 윤년의 2월 29일(2024)과 없는 해(2023)와 100 으로만 나눠지는 해(2100)
    assert_eq!(t("2027-01-01", 1_798_588_800.0), Expiry::Soon(2)); // 2026-12-30
    assert_eq!(t("2024-03-01", 1_709_078_400.0), Expiry::Soon(2)); // 2024-02-28
    assert_eq!(t("2023-03-01", 1_677_542_400.0), Expiry::Soon(1)); // 2023-02-28
    assert_eq!(t("2100-03-01", 4_107_456_000.0), Expiry::Soon(1)); // 2100-02-28
    // 틀린 꼴·없는 날은 none
    assert_eq!(t("내일", midnight), Expiry::None);
    assert_eq!(t("2026-02-30", midnight), Expiry::None);
    // Local 도 같은 날 수를 낸다(이 기계의 시간대로 오늘을 센다: 값은 시간대에 따라 하루 어긋날 수 있으나 만료일 당일 근처의 판정은 같은 종류다)
    assert!(matches!(ap_expiry("2026-10-30", midnight + 43_200.0, Tz::Local), Expiry::Soon(_) | Expiry::Expired(_)));
}

#[test]
fn extra_인증_머리_base64() {
    let b = |user: &str, token: &str| auth_header(&Auth::Basic { user: user.into() }, token).unwrap().value;
    // 조각 길이 0·1·2 (3 으로 나눈 나머지)별 패딩 — 표준 base64 와 같은 값
    for (user, token, want) in [
        ("", "", "Og=="),
        ("a", "b", "YTpi"),
        ("ab", "c", "YWI6Yw=="),
        ("a", "bc", "YTpiYw=="),
        ("a", "bcd", "YTpiY2Q="),
        ("a", "bcde", "YTpiY2Rl"),
        ("김", "토", "6rmAOu2GoA=="),
    ] {
        assert_eq!(b(user, token), format!("Basic {want}"), "{user}:{token}");
    }
}

#[test]
fn extra_비밀은_출력에_없다() {
    // 인증 머리의 값은 Debug 에 나오지 않고(이름은 나옴), 요청 값에는 비밀이 들어가지 않는다
    let h = auth_header(&Auth::Bearer, "SECRET-TOKEN").unwrap();
    assert_eq!(h.value, "Bearer SECRET-TOKEN");
    let shown = format!("{h:?}");
    assert!(shown.contains("Authorization") && !shown.contains("SECRET"), "{shown}");
    for a in [Auth::Basic { user: "kim@example.com".into() }, Auth::Figma, Auth::Bearer] {
        let shown = format!("{:?}", auth_header(&a, "SECRET-TOKEN").unwrap());
        assert!(!shown.contains("SECRET"), "{shown}");
    }
    // 머리에 인증 값이 들어 있는 요청도 Debug 는 가린다(대소문자 무시). 다른 머리는 그대로
    let mut req = HttpRequest { url: "https://x.example/a".into(), ..HttpRequest::default() };
    for (k, v) in [("Authorization", "SECRET-A"), ("x-figma-token", "SECRET-F"), ("Accept", "application/json")] {
        req.headers.insert(k.into(), v.into());
    }
    let shown = format!("{req:?}");
    assert!(!shown.contains("SECRET") && shown.contains("application/json"), "{shown}");
    // 확인 흐름 전체: 가짜 토큰이 요청·결과·Debug 어디에도 없고 가짜 HTTP 의 인증 머리에만 있다
    for id in ["atlassian", "notion", "figma"] {
        let c = Connection { account: "kim@example.com".into(), ..conn(id, Service::from_raw(id).unwrap(), if id == "atlassian" { "https://e.atlassian.net" } else { "" }) };
        let http = PubHttp::new(vec![reply(200, r#"{"displayName":"x","name":"x","email":"x"}"#)]);
        let out = check_connection(&c, false, &PubSecrets::new(&[(id, "SECRET-TOKEN")]), &http);
        let shown = format!("{out:?} {:?}", http.got.borrow());
        assert!(!shown.contains("SECRET"), "{id}: {shown}");
        assert_eq!(http.auths.borrow().len(), 1);
        assert!(http.auths.borrow()[0].contains("SECRET-TOKEN") || id == "atlassian", "{id}");
    }
    // 키체인의 토큰이 응답 거절 글에도 새지 않는다
    let http = PubHttp::new(vec![reply(401, "SECRET-BODY")]);
    let out = check_connection(&atl(), false, &PubSecrets::new(&[("atlassian", "SECRET-TOKEN")]), &http);
    assert!(!format!("{out:?}").contains("SECRET"));
}

#[test]
fn extra_같은사이트_경계() {
    let base = "https://example.atlassian.net";
    // 스킴·호스트는 대소문자를 가리지 않는다
    assert!(same_site("HTTPS://Example.Atlassian.NET/x", base));
    assert!(same_site(base, base));
    // 포트가 다르면(있고 없고도) 다른 사이트
    assert!(!same_site("https://example.atlassian.net:8443/x", base));
    assert!(!same_site("https://example.atlassian.net:443/x", base));
    assert!(same_site("https://example.atlassian.net:8443/x", "https://example.atlassian.net:8443"));
    // 사이트 이름을 접두로 속이는 호스트
    assert!(!same_site("https://example.atlassian.net.evil.example/x", base));
    assert!(!same_site("https://evilexample.atlassian.net/x", base));
    // 아이디만 있어도 거절(암호 없이)
    assert!(!same_site("https://u@example.atlassian.net/x", base));
    // 읽지 못하는 주소는 어느 쪽이든 거절
    assert!(!same_site("example.atlassian.net/x", base));
    assert!(!same_site(base, "not a url"));
    assert!(!same_site("", ""));
}

#[test]
fn extra_연결_확인_요청_모양() {
    let json = BTreeMap::from([("Accept".to_string(), "application/json".to_string())]);
    let r = check_request(&atl(), false).unwrap();
    assert_eq!((r.method.as_str(), r.url.as_str(), r.body), ("GET", "https://example.atlassian.net/rest/api/3/myself", None));
    assert_eq!(r.headers, json);
    assert_eq!(r.auth, Auth::Basic { user: "kim@example.com".into() });
    let n = check_request(&conn("notion", Service::Notion, ""), false).unwrap();
    assert_eq!(n.url, "https://api.notion.com/v1/users/me");
    assert_eq!(n.headers, BTreeMap::from([("Accept".to_string(), "application/json".to_string()), ("Notion-Version".to_string(), NOTION_VERSION.to_string())]));
    assert_eq!((n.auth, NOTION_VERSION), (Auth::Bearer, "2022-06-28"));
    let f = check_request(&conn("figma", Service::Figma, ""), false).unwrap();
    assert_eq!((f.url.as_str(), f.auth), ("https://api.figma.com/v1/me", Auth::Figma));
    assert_eq!(f.headers, json);
    // 시험 빌드의 가짜 서버 주소
    assert_eq!(check_request(&conn("figma", Service::Figma, "http://127.0.0.1:9"), true).unwrap().url, "http://127.0.0.1:9/v1/me");
    // 읽는 길이 없는 webhook 과 규칙 밖 주소는 요청 없음
    assert_eq!(check_request(&conn("webhook-1", Service::Webhook, ""), true), None);
    assert_eq!(check_request(&conn("atlassian", Service::Atlassian, "http://example.atlassian.net"), false), None);
    // 서비스마다 인증 꼴
    assert_eq!(auth_for(&atl()), Auth::Basic { user: "kim@example.com".into() });
    assert_eq!(auth_for(&conn("notion", Service::Notion, "")), Auth::Bearer);
    assert_eq!(auth_for(&conn("figma", Service::Figma, "")), Auth::Figma);
    assert_eq!(auth_for(&conn("webhook-1", Service::Webhook, "")), Auth::None);
    // 기본 요청: GET·빈 주소·머리 없음·본문 없음·인증 없음
    assert_eq!(HttpRequest::default(), HttpRequest { method: "GET".into(), url: String::new(), headers: BTreeMap::new(), body: None, auth: Auth::None });
    assert_eq!(HttpResponse::default().status, 0);
}

#[test]
fn extra_연결_확인_결과_글() {
    let o = |c: &Connection, r: HttpResponse| check_outcome(c, &r);
    let a = atl();
    // 응답 없음은 까닭 그대로, 상태 코드마다 글
    let none = HttpResponse { error: Some("시간이 지났습니다".into()), ..HttpResponse::default() };
    assert_eq!(o(&a, none), CheckOutcome::Failed("시간이 지났습니다".into()));
    assert_eq!(o(&a, reply(401, "")), CheckOutcome::Rejected("거부됨(401): 토큰이 틀렸거나 만료·취소됐습니다".into()));
    assert_eq!(o(&a, reply(403, "")), CheckOutcome::Rejected("거부됨(403): 토큰이 틀렸거나 만료·취소됐습니다".into()));
    assert_eq!(o(&a, reply(429, "")), CheckOutcome::Failed("요청이 많다며 거절했습니다(429). 잠시 뒤 다시 확인합니다".into()));
    assert_eq!(o(&a, reply(300, "")), CheckOutcome::Failed("다른 주소로 보내려 해 따라가지 않았습니다(300)".into()));
    assert_eq!(o(&a, reply(399, "")), CheckOutcome::Failed("다른 주소로 보내려 해 따라가지 않았습니다(399)".into()));
    assert_eq!(o(&a, reply(400, "")), CheckOutcome::Failed("받는 쪽이 400 로 답했습니다".into()));
    assert_eq!(o(&a, reply(500, "")), CheckOutcome::Failed("받는 쪽이 500 로 답했습니다".into()));
    assert_eq!(o(&a, reply(0, "")), CheckOutcome::Failed("받는 쪽이 0 로 답했습니다".into()));
    assert_eq!(o(&a, reply(201, r#"{"displayName":"x"}"#)), CheckOutcome::Failed("받는 쪽이 201 로 답했습니다".into()));
    // 200 이라도 이름을 못 읽으면 실패
    let shape = CheckOutcome::Failed("응답 꼴이 예상과 다릅니다".into());
    for body in ["", "{}", "[]", "\"x\"", "null", "{\"displayName\":3}", "{\"displayName\":\"  \"}", "not json", "{\"displayName\":null,\"emailAddress\":\"e@x\"}"] {
        assert_eq!(o(&a, reply(200, body)), shape, "{body}");
    }
    // Atlassian: 이름이 없을 때만 메일. 다듬고 120 글자까지
    assert_eq!(o(&a, reply(200, r#"{"emailAddress":"e@x"}"#)), CheckOutcome::Ok { who: "e@x".into() });
    assert_eq!(o(&a, reply(200, r#"{"displayName":"김","emailAddress":"e@x"}"#)), CheckOutcome::Ok { who: "김".into() });
    assert_eq!(o(&a, reply(200, "{\"displayName\":\" 김\\u202e시\\u0007험 \"}")), CheckOutcome::Ok { who: "김시험".into() });
    let long = format!("{{\"displayName\":\"{}\"}}", "가".repeat(121));
    assert_eq!(o(&a, reply(200, &long)), CheckOutcome::Ok { who: "가".repeat(120) });
    // Notion: 봇 이름·소유자 이름 — 둘이면 「봇 · 소유자」, 하나면 그것
    let n = conn("notion", Service::Notion, "");
    let owner = r#""bot":{"owner":{"user":{"name":"김"}}}"#;
    assert_eq!(o(&n, reply(200, &format!("{{\"name\":\"B\",{owner}}}"))), CheckOutcome::Ok { who: "B · 김".into() });
    assert_eq!(o(&n, reply(200, &format!("{{{owner}}}"))), CheckOutcome::Ok { who: "김".into() });
    assert_eq!(o(&n, reply(200, r#"{"name":"B","bot":{}}"#)), CheckOutcome::Ok { who: "B".into() });
    assert_eq!(o(&n, reply(200, r#"{"name":"B","bot":{"owner":"x"}}"#)), CheckOutcome::Ok { who: "B".into() });
    assert_eq!(o(&n, reply(200, r#"{"bot":{"owner":{"user":{}}}}"#)), shape);
    // Figma: 메일이 없을 때만 handle
    let f = conn("figma", Service::Figma, "");
    assert_eq!(o(&f, reply(200, r#"{"handle":"h"}"#)), CheckOutcome::Ok { who: "h".into() });
    assert_eq!(o(&f, reply(200, r#"{"email":"e@x","handle":"h"}"#)), CheckOutcome::Ok { who: "e@x".into() });
    // 서비스마다 자기 칸만 읽는다(Atlassian 칸을 Figma 응답에서 읽지 않는다)
    assert_eq!(o(&f, reply(200, r#"{"displayName":"x"}"#)), shape);
    assert_eq!(o(&a, reply(200, r#"{"email":"x"}"#)), shape);
    // webhook 은 읽을 칸이 없다
    assert_eq!(o(&conn("webhook-1", Service::Webhook, ""), reply(200, r#"{"displayName":"x"}"#)), shape);
    // 본문이 1 MiB 를 넘으면 읽지 않는다(경계 1 MiB 는 읽는다)
    let pad = |n: usize| format!("{{\"displayName\":\"x\",\"pad\":\"{}\"}}", "a".repeat(n - 28));
    assert_eq!(pad(1 << 20).len(), 1 << 20);
    assert_eq!(o(&a, reply(200, &pad(1 << 20))), CheckOutcome::Ok { who: "x".into() });
    assert_eq!(o(&a, reply(200, &pad((1 << 20) + 1))), shape);
}

#[test]
fn extra_연결_확인_요청_전_걸러내기() {
    let h = PubHttp::default();
    let tok = PubSecrets::new(&[("atlassian", FAKE_TOKEN), ("notion", FAKE_TOKEN), ("webhook-1", "https://x/y")]);
    // 계정 메일이 없는 Atlassian 은 요청하지 않는다
    let no_mail = Connection { account: String::new(), ..atl() };
    assert_eq!(check_connection(&no_mail, false, &tok, &h), CheckOutcome::Local("계정 메일이 없어 요청하지 않았습니다".into()));
    // 빈 토큰은 없는 것과 같다
    let empty = PubSecrets::new(&[("atlassian", "")]);
    assert_eq!(check_connection(&atl(), false, &empty, &h), CheckOutcome::Local("토큰이 키체인에 없어 요청하지 않았습니다".into()));
    // Notion·Figma 는 토큰이 없을 때 요청하지 않는다
    assert_eq!(check_connection(&conn("notion", Service::Notion, ""), false, &PubSecrets::new(&[]), &h), CheckOutcome::Local("토큰이 키체인에 없어 요청하지 않았습니다".into()));
    // webhook: 주소가 있느냐에 따라 글이 다르고 요청은 0
    let hook = conn("webhook-1", Service::Webhook, "");
    assert_eq!(check_connection(&hook, false, &PubSecrets::new(&[]), &h), CheckOutcome::Local("webhook 주소가 키체인에 없습니다".into()));
    assert_eq!(
        check_connection(&hook, false, &tok, &h),
        CheckOutcome::Local("webhook 은 읽는 요청이 없어 주소 꼴만 봤습니다. 대화방에 닿는지는 '시험 글 보내기'로 봅니다".into())
    );
    assert!(h.got.borrow().is_empty());
    // 시험 빌드의 가짜 서버로 가는 Notion 확인
    let fake = conn("notion", Service::Notion, "http://127.0.0.1:9");
    let h2 = PubHttp::new(vec![reply(200, r#"{"name":"B"}"#)]);
    assert_eq!(check_connection(&fake, true, &tok, &h2), CheckOutcome::Ok { who: "B".into() });
    assert_eq!(h2.got.borrow()[0].url, "http://127.0.0.1:9/v1/users/me");
    assert_eq!(h2.auths.borrow()[0], format!("Authorization: Bearer {FAKE_TOKEN}"));
}

#[test]
fn extra_만료글_전체() {
    assert_eq!(expiry_label(Expiry::None, ""), "만료일 없음");
    assert_eq!(expiry_label(Expiry::Ok(93), "2027-01-31"), "만료 2027-01-31 · 93일 남음");
    assert_eq!(expiry_label(Expiry::Soon(14), "2026-11-13"), "만료 임박 · 14일 남음(2026-11-13)");
    assert_eq!(expiry_label(Expiry::Soon(1), "2026-10-31"), "만료 임박 · 1일 남음(2026-10-31)");
}
