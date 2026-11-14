//! 자율주행 MCP 읽기 세션 — 원본 전체(R-MCPRead a + b)의 시험.
//! 원본: `Tests/AutopilotRulesTests/ReadingTests.swift` 의 `MCPReadTests`(618줄~, 도구 표 581줄).
//! 옮긴 시험(같은 이름): `도구`(표 33줄) · `인자` · `서버` · `결과`(664줄) · `가리기`(680줄, `add_evidence` 로 수).
//! R-MCPRead-a 청크는 원본 1~145줄을, R-MCPRead-b 청크는 146~213줄(질문·출력 해석·메타)를 싣는다.
//! 끝의 `extra_` 시험은 Swift 시험에 없는 것(경계)이다.

use claude_board_autopilot::rules::evidence::{add_evidence, manifest_parse, EvidenceStore, MANIFEST_NAME};
use claude_board_autopilot::rules::formats::grapheme_len;
use claude_board_autopilot::rules::mcp_read::{
    mcp_allow_prefixes, mcp_allow_rules, mcp_allow_summary, mcp_config_json, mcp_deny_rules, mcp_glob, mcp_meta, mcp_output, mcp_prompt, mcp_read_args,
    mcp_server_name_ok, mcp_servers, mcp_settings_json, mcp_tool_allowed, McpServer, MCP_BUILTIN_DENY, MCP_READ_BUDGET_USD, MCP_READ_MODEL, MCP_READ_MAX_SEC,
    MCP_READ_VERBS, MCP_WRITE_VERBS,
};
use claude_board_autopilot::rules::reading::ReadClock;
use serde_json::{json, Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

/// 원본 `body(Any)` 의 옮김 — 값 그대로 직렬화한다(규칙은 이름 순을 스스로 정하므로 입력 순서는 상관 없다)
fn body(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap()
}

/// 원본 도움 함수 `after` — 인자 목록에서 `k` 뒤의 값(원본은 없으면 터지지만, 여기는 None 으로 단언이 잡는다)
fn after<'a>(args: &'a [String], k: &str) -> Option<&'a str> {
    args.iter().position(|x| x.as_str() == k).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// (원본 `ToolRow`) 도구 이름을 정한 서버("wiki")의 이 세션에서 쓸 수 있는가
const TOOL_ROWS: [(&str, &str, bool); 33] = [
    ("Atlassian 읽기", "getJiraIssue", true),
    ("Atlassian 찾기", "searchJiraIssuesUsingJql", true),
    ("Confluence 읽기", "getConfluencePage", true),
    ("Confluence 댓글 읽기", "getConfluencePageFooterComments", true),
    ("만들기", "createJiraIssue", false),
    ("고치기", "editJiraIssue", false),
    ("댓글 달기", "addCommentToJiraIssue", false),
    ("상태 바꾸기", "transitionJiraIssue", false),
    ("페이지 고치기", "updateConfluencePage", false),
    ("읽기 꼴이 아닌 이름은 허용 밖", "atlassianUserInfo", false),
    ("Notion 읽기(이름 앞 붙임)", "API-retrieve-a-page", true),
    ("Notion 블록 읽기", "API-get-block-children", true),
    ("POST 로 찾기도 막는 쪽", "API-post-search", false),
    ("Notion 고치기", "API-patch-page", false),
    ("Notion 지우기", "API-delete-a-block", false),
    ("GitHub 파일 읽기", "get_file_contents", true),
    ("GitHub 목록", "list_issues", true),
    ("GitHub 찾기", "search_code", true),
    ("GitHub 합치기", "merge_pull_request", false),
    ("GitHub 올리기", "push_files", false),
    ("읽고 만들기", "get_or_create_page", false),
    ("대문자 꼴", "DELETE_ROW", false),
    ("다른 서버 도구", "mcp__other__get_page", false),
    ("가운데 낀 get(쓰기 도구)", "delete_and_get_x", false),
    ("거부 목록에 없는 쓰기 동사 뒤 get(전에는 *_get* 으로 허용됐음)", "purge_and_get_status", false),
    ("가운데 낀 read(쓰기 꼴 없음이라도)", "cleanup_read_cache", false),
    ("모르는 접두어 뒤 get", "x_get_page", false),
    ("낙타 꼴 가운데 Get", "notionGetPage", false),
    ("서버 이름 접두어", "wiki_get_page", true),
    ("서버 이름 접두어(-)", "wiki-search", true),
    ("다른 서버 이름 접두어", "other_get_page", false),
    ("읽고 지우기", "get_then_delete", false),
    ("내장 Bash", "Bash", false),
];

#[test]
fn 도구() {
    for (i, (note, tool, ok)) in TOOL_ROWS.iter().enumerate() {
        assert_eq!(mcp_tool_allowed(tool, "wiki"), *ok, "행 {i} — {note}: 도구 {tool}");
    }
}

#[test]
fn 인자() {
    let a = mcp_read_args("wiki", "/p/mcp.json", "/p/settings.json", MCP_READ_MODEL, MCP_READ_BUDGET_USD);
    assert!(a.iter().any(|x| x.as_str() == "--strict-mcp-config"));
    assert_eq!(after(&a, "--mcp-config"), Some("/p/mcp.json"));
    assert_eq!(after(&a, "--tools"), Some(""));
    assert_eq!(after(&a, "--permission-mode"), Some("dontAsk"));
    assert_eq!(after(&a, "--output-format"), Some("json"));
    assert_eq!(after(&a, "--max-budget-usd"), Some("0.30"));
    // 사용자·저장소·local 설정은 읽지 않는다(--restricted), 엔진이 쓴 --settings 만
    assert!(a.iter().any(|x| x.as_str() == "--no-session-persistence"));
    assert!(a.iter().any(|x| x.as_str() == "--restricted"));
    assert!(!a.iter().any(|x| x.as_str() == "--setting-sources"));
    assert_eq!(after(&a, "--settings"), Some("/p/settings.json"));
    let allow: Vec<&str> = after(&a, "--allowedTools").unwrap().split(' ').collect();
    let deny: Vec<&str> = after(&a, "--disallowedTools").unwrap().split(' ').collect();
    assert!(allow.iter().all(|r| r.starts_with("mcp__wiki__")));
    assert!(!allow.contains(&"Bash"));
    assert!(["Bash", "Write", "Edit", "Read", "WebFetch"].iter().all(|t| deny.contains(t)));
    assert!(deny.contains(&"mcp__wiki__create*"));
    assert!(deny.contains(&"mcp__wiki__*Delete*"));
    assert!(mcp_prompt("https://wiki.example.com/p", "wiki", &[]).contains("주소: https://wiki.example.com/p"));
    let s = mcp_settings_json("wiki");
    assert_eq!(s["disableAllHooks"], json!(true));
    assert_eq!(s["permissions"]["defaultMode"], json!("dontAsk"));
    let deny_rules: Vec<&str> = s["permissions"]["deny"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(deny_rules.contains(&"Bash"));
    for r in &allow {
        let hit = mcp_allow_prefixes("wiki").iter().any(|p| MCP_READ_VERBS.iter().any(|v| *r == format!("mcp__wiki__{p}{v}*")));
        assert!(hit, "허용 꼴이 접두어·동사 조합이 아니다(가운데 낀 꼴 *_get* 이 없어야 한다): {r}");
    }
    assert!(mcp_allow_summary("wiki").contains("로 시작하는 것만"));
}

#[test]
fn 서버() {
    let claude = body(json!({
        "mcpServers": {
            "wiki": {"command": "/usr/bin/false", "args": ["--user"], "env": {"WIKI_TOKEN": "fake-x"}},
            "remote": {"type": "http", "url": "https://mcp.example.com/mcp", "headers": {"Authorization": "Bearer fake-x"}},
            "plain-http": {"type": "http", "url": "http://mcp.example.com"},
            "bad name": {"command": "x"}, "nothing": {"type": "stdio"}
        },
        "projects": {"/w/app": {
            "mcpServers": {"wiki": {"command": "/usr/bin/true", "args": ["--local"]}},
            "enabledMcpjsonServers": ["docs", "off"], "disabledMcpjsonServers": ["off"]
        }}
    }));
    let project = body(json!({
        "mcpServers": {
            "docs": {"type": "sse", "url": "https://docs.example.com/sse"},
            "wiki": {"command": "/bin/echo"},
            "repo-tool": {"command": "./scripts/evil.sh"}, "off": {"command": "/bin/echo"}
        }
    }));
    let list = mcp_servers(Some(&claude), Some(&project), Some("/w/app"));
    let fmt: Vec<String> = list.iter().map(|s| format!("{} {} {} {}", s.name, s.transport, s.scope, s.approved)).collect();
    assert_eq!(fmt, ["wiki stdio local true", "docs sse project true", "off stdio project false", "repo-tool stdio project false", "remote http user true"]);
    let repo = list.iter().find(|s| s.name == "repo-tool").unwrap();
    assert!(repo.note.contains("승인한 뒤에만"));
    assert_eq!(repo.target, "./scripts/evil.sh");
    let off = list.iter().find(|s| s.name == "off").unwrap();
    assert!(off.note.contains("disabledMcpjsonServers"));
    let docs = list.iter().find(|s| s.name == "docs").unwrap();
    assert_eq!(docs.target, "docs.example.com");
    // 모든 프로젝트 서버를 켠 프로젝트
    let all = body(json!({"projects": {"/w/app": {"enableAllProjectMcpServers": true}}}));
    let all_list = mcp_servers(Some(&all), Some(&project), Some("/w/app"));
    assert!(all_list.iter().all(|s| s.approved));
    let one = mcp_config_json(&list[0]);
    let keys: Vec<&str> = one["mcpServers"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["wiki"]);
    assert_eq!(one["mcpServers"]["wiki"]["args"], json!(["--local"]));
    let names: Vec<String> = mcp_servers(Some(&claude), None, None).iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["remote", "wiki"]);
}

// ── 원본 `결과`(664줄) · `가리기`(680줄) — R-MCPRead-b ──

#[test]
fn 결과() {
    let good = r#"{"format":"claude-board-meta","version":1,"source":"x","url":"https://evil.example/","title":"위키 쪽","summary":["한 줄"]}"#;
    let a = mcp_meta(good, "wiki", "https://wiki.example.com/p");
    let am = a.meta.expect("메타");
    assert_eq!(am.source, "mcp:wiki");
    assert_eq!(am.url, "https://wiki.example.com/p");
    assert_eq!(am.title, "위키 쪽");
    assert!(mcp_meta(&format!("```json\n{good}\n```"), "wiki", "https://wiki.example.com/p").meta.is_some());
    assert!(mcp_meta(&format!("여기 있습니다:\n{good}"), "wiki", "x").problems.first().is_some_and(|p| p.contains("JSON 객체 하나가 아닙니다")));
    assert_eq!(
        mcp_meta(r#"{"format":"claude-board-meta","version":1,"source":"x","title":"<h1>x</h1>"}"#, "w", "x").problems,
        ["HTML 태그가 있습니다(title)"]
    );
    assert_eq!(
        mcp_meta(r#"{"format":"claude-board-meta","version":1,"source":"x","title":"t","body":"전문"}"#, "w", "x").problems,
        ["형식 밖 칸입니다(body)"]
    );
    let out = mcp_output(br#"{"type":"result","subtype":"success","is_error":false,"result":"{}","total_cost_usd":0.0123}"#);
    assert_eq!(out.text.as_deref(), Some("{}"));
    assert_eq!(out.cost, Some(0.0123));
    assert!(out.error.is_none());
    let err = mcp_output(br#"{"type":"result","subtype":"error_max_budget_usd","is_error":true}"#);
    assert!(err.error.as_deref().is_some_and(|e| e.contains("error_max_budget_usd")));
    assert_eq!(mcp_output(b"not json").error.as_deref(), Some("세션이 결과를 내지 않았습니다"));
}

/// 가짜 근거 저장소 — 메모리, 해시 대신 `sha-<글자 수>`(`tests/rules_reading.rs` 의 MemEvidence, `tests/rules_import.rs` 가 다시 둔 것과 같다)
struct MemEvidence {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    removed: RefCell<Vec<String>>,
}

impl MemEvidence {
    fn new() -> Self {
        Self { files: RefCell::new(BTreeMap::new()), removed: RefCell::new(Vec::new()) }
    }
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        self.files.borrow().clone()
    }
}

impl EvidenceStore for MemEvidence {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(name).cloned()
    }
    fn write(&self, name: &str, data: &[u8]) -> Option<String> {
        self.files.borrow_mut().insert(name.to_string(), data.to_vec());
        Some(format!("sha-{}", data.len()))
    }
    fn remove(&self, name: &str) {
        self.files.borrow_mut().remove(name);
        self.removed.borrow_mut().push(name.to_string());
    }
}

/// 가짜 시각 — 숫자 하나(`tests/rules_reading.rs` 의 PubClock 와 같다)
struct PubClock {
    t: Cell<f64>,
}

impl PubClock {
    fn new(t: f64) -> Self {
        PubClock { t: Cell::new(t) }
    }
}

impl ReadClock for PubClock {
    fn now(&self) -> f64 {
        self.t.get()
    }
}

/// 서일의 기준 시각(`tests/rules_reading.rs` 의 T0 와 같다)
const T0: f64 = 1_793_340_000.0;

/// 토큰 꼴 견본(가짜) — 글자 그대로 저장소에 남기지 않는다(`tests/rules_meta.rs` 의 FAKE_SECRET_GITHUB 와 같은 부분)
const FAKE_SECRET_GITHUB: &str = concat!("gh", "p_", "Zz9Zz9Zz9Zz9Zz9Zz9Zz9Zz9Zz9Zz9Zz9Zz9");

/// MCP 길의 비밀도 근거 묶음에 넣는 문에서 가리고 MANIFEST 에 수를 적는다(가린 뒤 다시 검사)
#[test]
fn 가리기() {
    let raw = [
        r#"{"format":"claude-board-meta","version":1,"source":"mcp","title":"위키","summary":["배포 키 "#,
        FAKE_SECRET_GITHUB,
        r#""],"sections":[{"title":"설정","text":"password=hunter2-fake"}]}"#,
    ]
    .concat();
    let m = mcp_meta(&raw, "wiki", "https://wiki.example.com/p").meta.expect("메타");
    let store = MemEvidence::new();
    let w = add_evidence(&m, None, "", "https://wiki.example.com/p", "mcp", 0, 0, Some(0.01), &store, &PubClock::new(T0));
    let w_entry = w.entry.expect(&w.note);
    assert_eq!(w_entry.redacted, 2);
    assert_eq!(w_entry.source, "mcp:wiki");
    assert_eq!(w_entry.cost_usd, Some(0.01));
    let all = store.files().values().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<String>();
    assert!(!all.contains(FAKE_SECRET_GITHUB) && !all.contains("hunter2") && all.contains("[가림:GitHub 토큰]"), "근거 파일에 비밀이 남는다: {all}");
    let man = manifest_parse(store.read(MANIFEST_NAME).as_deref());
    assert_eq!(man.first().map(|e| e.redacted), Some(2));
}

fn 한_서버(name: &str, server: Value) -> Vec<McpServer> {
    let mut inner = Map::new();
    inner.insert(name.to_string(), server);
    let mut root = Map::new();
    root.insert("mcpServers".to_string(), Value::Object(inner));
    let bytes = serde_json::to_vec(&root).unwrap();
    mcp_servers(Some(&bytes), None, None)
}

#[test]
fn extra_글롭_경계() {
    assert!(mcp_glob("", ""));
    assert!(!mcp_glob("", "x"));
    assert!(mcp_glob("a.b", "a.b")); // 별표 없는 꼴은 메타 문자도 낱글자로
    assert!(!mcp_glob("a.b", "aXb"));
    assert!(!mcp_glob("*Get*", "notionGETpage")); // 대소문자를 가린다 — 원본 주석의 "가린다" 와 정반대다
    assert!(mcp_glob("*Get*", "notionGetpage"));
    assert!(mcp_glob("*", "아무 글자나") && mcp_glob("**", "빈 글자도"));
    assert!(mcp_glob("a**b", "ab") && mcp_glob("a**b", "aXb")); // 이웃한 별표는 하나와 같다
    assert!(mcp_glob("a*a", "aa")); // 앞·뒤 절이 겹치는 문자열은 못 맞추는 것(원본 정규식과 같다)
    assert!(!mcp_glob("a*a", "a"));
    assert!(mcp_glob("*a*", "xax") && !mcp_glob("*b*", "xax"));
}

#[test]
fn extra_서버_경계() {
    assert_eq!(한_서버(&"a".repeat(64), json!({"command": "cmd"})).len(), 1); // 이름은 64 글자까지
    assert!(한_서버(&"a".repeat(65), json!({"command": "cmd"})).is_empty());
    assert!(한_서버("a.b", json!({"command": "cmd"})).is_empty()); // 점은 이름이 아니다
    assert_eq!(한_서버("a-b_c", json!({"command": "cmd"})).len(), 1);
    assert!(한_서버("n1", json!({"command": 3})).is_empty()); // command 가 글이 아니면 버린다
    let typed = 한_서버("n2", json!({"type": 3, "command": "cmd"})).remove(0); // type 이 글이 아니어도 command 로 stdio
    assert_eq!(typed.transport, "stdio");
    assert!(한_서버("n3", json!({"type": "http", "url": 3})).is_empty()); // url 이 글이 아니면 버린다
    let long = 한_서버("n4", json!({"command": "x".repeat(250)})).remove(0);
    assert_eq!(grapheme_len(&long.target), 200); // 화면에 보일 띄울 것은 앞 200 글자만
    // 활성·비활성 낱목의 배열에 글이 아닌 것이 섞여도 글자만 걸러 센다
    let claude = body(json!({"projects": {"/w/app": {"enabledMcpjsonServers": ["docs", 1, true], "disabledMcpjsonServers": [7, "off"]}}}));
    let project = body(json!({"mcpServers": {
        "docs": {"command": "/bin/x"}, "off": {"command": "/bin/y"}, "never": {"command": "/bin/z"}
    }}));
    let list = mcp_servers(Some(&claude), Some(&project), Some("/w/app"));
    let docs = list.iter().find(|s| s.name == "docs").unwrap();
    let off = list.iter().find(|s| s.name == "off").unwrap();
    let never = list.iter().find(|s| s.name == "never").unwrap();
    assert!(docs.approved && !off.approved && !never.approved);
    assert!(off.note.contains("disabledMcpjsonServers") && never.note.contains("승인한 뒤에만"));
    // 입력이 없으면 서버가 없다
    assert!(mcp_servers(None, None, None).is_empty());
    assert!(mcp_servers(Some(b"not json".as_ref()), None, None).is_empty());
    assert!(mcp_servers(Some(b"[]".as_ref()), None, None).is_empty());
    // 32MiB 넘는 설정은 읽지 않는다(읽기 실패를 읽어도 서버가 없다 — 조용히 빈 객체)
    let mut big = String::from(r#"{"key":""#);
    big.push_str(&"a".repeat(34 << 20));
    big.push_str(r#""}"#);
    assert!(mcp_servers(Some(&big.into_bytes()), None, None).is_empty());
}

#[test]
fn extra_목록_차수() {
    assert_eq!(MCP_READ_VERBS.len(), 12);
    assert_eq!(MCP_WRITE_VERBS.len(), 72);
    assert_eq!(MCP_BUILTIN_DENY.len(), 22);
    assert_eq!(mcp_allow_rules("wiki").len(), 12 * 5);
    assert_eq!(mcp_deny_rules("wiki").len(), 22 + 72 * 5 + 4);
    assert_eq!(mcp_allow_rules("wiki").first(), Some(&"mcp__wiki__get*".to_string()));
    assert_eq!(mcp_deny_rules("wiki").last(), Some(&"mcp__wiki__*WRITE*".to_string()));
    assert!(mcp_deny_rules("wiki").iter().any(|r| r == "mcp__wiki__*Create*")); // 낙타 꼴 거부 꼴이 있다
    assert_eq!(MCP_READ_MODEL, "haiku");
    assert_eq!(MCP_READ_BUDGET_USD, 0.30);
    assert_eq!(MCP_READ_MAX_SEC, 180.0);
    // 이름 꼴 경계를 상수 함수에서 다시 잠근다
    assert!(mcp_server_name_ok(&"a".repeat(64)) && !mcp_server_name_ok(&"a".repeat(65)));
    assert!(mcp_server_name_ok("a-b_c") && !mcp_server_name_ok("a.b"));
    assert!(!mcp_server_name_ok(""));
}

