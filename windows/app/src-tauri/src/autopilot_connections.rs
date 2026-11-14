//! 자율주행 바깥 연결(Jira·Confluence·Notion·Figma·알림 webhook)의 보드 쪽 뒷단 — 목록 읽기와 「연결 확인」.
//! 원본: `Sources/ClaudeBoard/AutopilotPublish.swift`(`APConnections.load`·`check`)와 `Sources/ClaudeAutopilot/main.swift`(`cmdConnection`·`EngineHTTP`).
//! 규칙(`parse_connections`·`check_connection` …)과 비밀 저장소(`DpapiSecrets`)는 `claude-board-autopilot` 크레이트의 것을 그대로 쓴다. 여기가 더하는 것은
//! ① 보드 화면에 보낼 목록 꼴(`ConnectionView` — 비밀 칸이 없다) ② 실제 HTTP 어댑터(`UreqConn`) ③ 확인 결과를 화면 글로 옮기는 것뿐이다.
//!
//! 자리: 연결 목록 `<뿌리>\connections.json`(비밀 없음, 256 KiB 까지·일반 파일만), 비밀 `<뿌리>\private\connection-secrets\<id>.dpapi`.
//! 뿌리는 `claude_board_core::paths::autopilot_root(dev)`.
//!
//! HTTP 어댑터 정책(원본 `EngineHTTP` 와 같다): 리다이렉트는 따르지 않는다(3xx 는 응답으로 돌려주고 규칙이 「따라가지 않았습니다」로 옮긴다 — 인증 머리가
//! 다른 곳으로 다시 나가지 않는다), 응답 본문은 4 MiB 까지, 요청 20초, https 만(시험 빌드(`dev`)는 http://127.0.0.1 도), 쿠키·캐시 없음.
//! 인증 머리는 이 어댑터 안에서만 머리 값으로 만들어지고 민감 표시를 달며, 오류 글은 늘 고정 글이라 비밀·주소·본문이 들어갈 수 없다. 기록(log)은 하지 않는다.
//!
//! 아직 하지 않는 것: 확인 결과를 `connections.json` 의 `checked` 칸에 적는 일(원본은 보드가 적는다) — 쓰기 길은 설정 화면 청크가 소유한다.

// Tauri 명령 둘은 보드의 `invoke_handler` 에 걸어야 쓰인다(통합자 몫). 걸기 전에는 비공개 모듈 안에서 안 쓰여 경고가 난다
#![allow(dead_code)]

use claude_board_autopilot::adapters::secrets::DpapiSecrets;
use claude_board_autopilot::rules::connections::{
    ap_expiry, check_connection, connection_id_ok, connections_json, expiry_label, parse_connections, parse_url, AuthHeader, CheckOutcome, CheckRecord, ConnHttp,
    ConnSecrets, Connection, Expiry, HttpRequest, HttpResponse, Service,
};
use claude_board_autopilot::rules::stopline::Tz;
use claude_board_core::{clock, paths};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// 응답 본문 상한(바이트)
pub const MAX_BODY: usize = 4 << 20;
/// 요청 하나의 전체 제한 시간
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// 연결 목록 파일 크기 상한
const MAX_LIST_FILE: u64 = 256 << 10;
/// 받는 쪽이 보는 이름
const USER_AGENT: &str = "Claude Board autopilot";
/// 화면 글 한 칸의 길이 상한(원본 `apDocText(…, 300)`)
const NOTE_MAX: usize = 300;

/// 어디에서 읽고 어떤 규칙으로 보는가 — 시험은 임시 폴더와 `dev` 를 직접 준다
#[derive(Debug, Clone)]
pub struct ConnCtx {
    root: PathBuf,
    dev: bool,
}

impl ConnCtx {
    pub fn new(root: impl Into<PathBuf>, dev: bool) -> Self {
        Self { root: root.into(), dev }
    }

    /// 실제 보드 — 파일은 보드의 자율주행 읽기(`board.rs` 의 `autopilot_root(false)`)와 엔진이 보는 같은 뿌리에 둔다.
    /// 시험 빌드(`updater::is_dev`)는 사이트 규칙만 넓힌다(http://127.0.0.1 허용)
    pub fn live() -> Self {
        Self::new(paths::autopilot_root(false), crate::updater::is_dev())
    }

    fn list_path(&self) -> PathBuf {
        self.root.join("connections.json")
    }

    fn secrets_dir(&self) -> PathBuf {
        self.root.join("private").join("connection-secrets")
    }
}

// ── 목록 ─────────────────────────────────────────────

/// 마지막 「연결 확인」 기록(비밀 없음)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    pub at: f64,
    pub ok: bool,
    pub who: String,
    pub note: String,
}

/// 설정 › 연결 화면에 보내는 연결 한 줄. 비밀(토큰·webhook 주소 전체)을 담을 칸이 없다 — 토큰이 키 보관소에 있는지(`token_saved`)만 알린다
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub id: String,
    /// atlassian | notion | figma | webhook
    pub service: String,
    pub service_label: String,
    /// 화면 이름 — webhook 은 사람이 붙인 이름, 나머지는 서비스 이름
    pub display_name: String,
    /// Atlassian 사이트(시험 빌드의 Notion·Figma 바탕 주소)
    pub site: String,
    /// webhook 주소의 호스트(주소 전체는 비밀 저장소)
    pub host: String,
    pub account: String,
    pub scopes: Vec<String>,
    /// webhook 글 꼴(json | slack | gchat), 다른 서비스는 빈 글자
    pub format: String,
    pub token_saved: bool,
    pub checked: Option<CheckView>,
    /// 만료일 yyyy-MM-dd(없으면 빈 글자)
    pub expires: String,
    /// none | ok | soon | expired
    pub expiry_state: String,
    pub expiry_label: String,
}

fn display_name(c: &Connection) -> String {
    if c.service == Service::Webhook && !c.name.is_empty() {
        c.name.clone()
    } else {
        c.service.label().to_string()
    }
}

fn expiry_state(e: Expiry) -> &'static str {
    match e {
        Expiry::None => "none",
        Expiry::Ok(_) => "ok",
        Expiry::Soon(_) => "soon",
        Expiry::Expired(_) => "expired",
    }
}

fn view_of(c: &Connection, token_saved: bool, now: f64) -> ConnectionView {
    let e = ap_expiry(&c.expires, now, Tz::Local);
    ConnectionView {
        id: c.id.clone(),
        service: c.service.raw().to_string(),
        service_label: c.service.label().to_string(),
        display_name: display_name(c),
        site: c.site.clone(),
        host: c.host.clone(),
        account: c.account.clone(),
        scopes: c.scopes.clone(),
        format: if c.service == Service::Webhook { c.format.raw().to_string() } else { String::new() },
        token_saved,
        checked: c.checked.as_ref().map(|k| CheckView { at: k.at, ok: k.ok, who: k.who.clone(), note: k.note.clone() }),
        expires: c.expires.clone(),
        expiry_state: expiry_state(e).to_string(),
        expiry_label: expiry_label(e, &c.expires),
    }
}

/// 읽어도 되는 목록 파일인가 — 일반 파일(폴더·링크 아님), 256 KiB 까지
fn list_file_ok(meta: &fs::Metadata) -> bool {
    meta.is_file() && meta.len() <= MAX_LIST_FILE
}

/// `connections.json` — 일반 파일만(링크를 따라가지 않는다), 256 KiB 까지. 없거나 틀리면 빈 목록
fn load_connections(ctx: &ConnCtx) -> Vec<Connection> {
    let path = ctx.list_path();
    let Ok(meta) = fs::symlink_metadata(&path) else { return Vec::new() };
    if !list_file_ok(&meta) {
        return Vec::new();
    }
    match fs::read(&path) {
        Ok(bytes) => parse_connections(Some(&bytes)),
        Err(_) => Vec::new(),
    }
}

fn list_in(ctx: &ConnCtx, secrets: &dyn ConnSecrets, now: f64) -> Vec<ConnectionView> {
    load_connections(ctx).iter().map(|c| view_of(c, secrets.exists(&c.id), now)).collect()
}

/// 연결 목록(비밀 없음)
pub fn connections_list(ctx: &ConnCtx) -> Vec<ConnectionView> {
    list_in(ctx, &DpapiSecrets::new(ctx.secrets_dir()), clock::now())
}

// ── 연결 확인 ────────────────────────────────────────

/// 연결 확인 결과 — 엔진 `connection check` 의 JSON 한 줄과 같은 칸(`kind`·`ok`·`who`·`note`)에 화면에 쓸 글(`text`)을 더한 것
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    /// ok | rejected | failed | local
    pub kind: String,
    pub ok: bool,
    pub who: String,
    pub note: String,
    /// 사람이 읽는 시험 글: 성공이면 「연결됨 · 이름」, 아니면 까닭
    pub text: String,
}

/// 원본 `apDocText(s, n)` 중 쓰는 만큼 — 줄바꿈·제어 글자를 공백으로, 앞뒤를 깎고, n 글자로 자른다
fn doc_text(s: &str, n: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flat.trim().chars().take(n).collect()
}

fn report_of(outcome: CheckOutcome) -> CheckReport {
    let (kind, ok, who, note) = match outcome {
        CheckOutcome::Ok { who } => ("ok", true, who, "읽기 요청이 됐습니다".to_string()),
        CheckOutcome::Rejected(n) => ("rejected", false, String::new(), n),
        CheckOutcome::Failed(n) => ("failed", false, String::new(), n),
        CheckOutcome::Local(n) => ("local", false, String::new(), n),
    };
    let who = doc_text(&who, 120);
    let note = doc_text(&note, NOTE_MAX);
    let text = if ok { format!("연결됨 · {who}") } else { note.clone() };
    CheckReport { kind: kind.to_string(), ok, who, note, text }
}

/// 확인 한 번. id 꼴이 틀리면 파일 경로도 요청도 만들기 전에 거절한다
fn outcome_in(ctx: &ConnCtx, id: &str, secrets: &dyn ConnSecrets, http: &dyn ConnHttp) -> Result<CheckOutcome, String> {
    if !connection_id_ok(id) {
        return Err("연결 id 꼴이 아닙니다".to_string());
    }
    let Some(c) = load_connections(ctx).into_iter().find(|c| c.id == id) else {
        return Ok(CheckOutcome::Local(format!("연결 목록에 {id} 가 없습니다(먼저 저장합니다)")));
    };
    Ok(check_connection(&c, ctx.dev, secrets, http))
}

fn check_in(ctx: &ConnCtx, id: &str, secrets: &dyn ConnSecrets, http: &dyn ConnHttp) -> Result<CheckReport, String> {
    outcome_in(ctx, id, secrets, http).map(report_of)
}

/// 확인하고 결과를 목록에 적는다(원본 `APConnections.check` 와 같다). 적지 못해도 확인 결과는 돌려준다
fn check_and_record_in(ctx: &ConnCtx, id: &str, secrets: &dyn ConnSecrets, http: &dyn ConnHttp, now: f64) -> Result<CheckReport, String> {
    let outcome = outcome_in(ctx, id, secrets, http)?;
    let _ = record_check(&ctx.root, id, &outcome, now);
    Ok(report_of(outcome))
}

/// 연결 확인 — 읽기 요청 하나(webhook·토큰 없음·주소 규칙 밖이면 요청 0). 요청이 갔으면 결과를 `checked` 칸에 적는다
pub fn connection_check(ctx: &ConnCtx, id: &str) -> Result<CheckReport, String> {
    check_and_record_in(ctx, id, &DpapiSecrets::new(ctx.secrets_dir()), &UreqConn::new(ctx.dev), clock::now())
}

// ── 확인 결과 적기 ───────────────────────────────────

static LIST_WRITE: Mutex<()> = Mutex::new(());
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 이름 바꾸기로 덮어쓰는 쓰기 — 같은 폴더 임시 파일에 쓰고 디스크까지 내린 뒤 바꾼다. 실패하면 옛 파일이 그대로고 임시 파일은 지운다
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), TEMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let written = fs::OpenOptions::new().write(true).create_new(true).open(&temp).and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()));
    let done = written.and_then(|()| fs::rename(&temp, path));
    if done.is_err() {
        let _ = fs::remove_file(&temp);
    }
    done
}

/// 그 연결의 `checked` 칸만 고쳐 쓴다(원본: 연결 확인 뒤 보드가 적는 `APCheckRecord`). 규칙 `parse_connections`→`connections_json` 을 거치므로 비밀 칸이 생길 수 없다.
/// 요청 없이 끝난 결과(`Local`)는 지난 기록을 덮지 않는다 → `Ok(false)`. 적었으면 `Ok(true)`. 목록이 없거나 깨졌거나 그 id 가 없으면 파일을 건드리지 않고 `Err`.
/// 같은 프로세스의 동시 쓰기는 잠금 하나로 줄 세운다(읽고-고쳐-쓰기가 서로 덮지 않게)
pub fn record_check(dir: &Path, id: &str, outcome: &CheckOutcome, now: f64) -> Result<bool, String> {
    if !connection_id_ok(id) {
        return Err("연결 id 꼴이 아닙니다".to_string());
    }
    if matches!(outcome, CheckOutcome::Local(_)) {
        return Ok(false);
    }
    let report = report_of(outcome.clone());
    let _guard = LIST_WRITE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let ctx = ConnCtx::new(dir, false);
    let mut list = load_connections(&ctx);
    let Some(c) = list.iter_mut().find(|c| c.id == id) else { return Err(format!("연결 목록에 {id} 가 없어 기록하지 않았습니다")) };
    c.checked = Some(CheckRecord { at: now, ok: report.ok, who: report.who, note: report.note });
    let bytes = serde_json::to_vec_pretty(&connections_json(&list)).map_err(|_| "연결 목록을 만들지 못했습니다".to_string())?;
    write_atomic(&ctx.list_path(), &bytes).map_err(|_| "연결 목록 파일을 쓰지 못했습니다".to_string())?;
    Ok(true)
}

/// 같은 확인의 사람이 읽는 글(원본 설정 화면 줄과 같은 글)
pub fn connection_test_text(ctx: &ConnCtx, id: &str) -> Result<String, String> {
    connection_check(ctx, id).map(|r| r.text)
}

// ── HTTP 어댑터 ──────────────────────────────────────

/// `ConnHttp` 의 실제 구현 — ureq 3. 위 머리 설명의 정책을 지킨다
#[derive(Debug, Clone, Copy)]
pub struct UreqConn {
    dev: bool,
    timeout: Duration,
}

impl UreqConn {
    pub fn new(dev: bool) -> Self {
        Self { dev, timeout: REQUEST_TIMEOUT }
    }

    pub fn with_timeout(dev: bool, timeout: Duration) -> Self {
        Self { dev, timeout }
    }

    /// 규칙이 이미 본 주소지만 어댑터도 한 번 더 본다: https, 시험 빌드면 http://127.0.0.1 도. 아이디·암호가 든 주소는 늘 거절
    fn url_allowed(&self, url: &str) -> bool {
        let Some(u) = parse_url(url) else { return false };
        if u.user.is_some() || u.password.is_some() {
            return false;
        }
        match u.scheme.as_deref().map(str::to_lowercase).as_deref() {
            Some("https") => true,
            Some("http") => self.dev && u.host.as_deref() == Some("127.0.0.1"),
            _ => false,
        }
    }
}

/// 에이전트 설정(순수) — 리다이렉트를 따르지 않고(3xx 를 따라가면 인증 머리가 다시 나간다. 0 이면 오류도 아니고 응답으로 돌려준다),
/// 3xx·4xx·5xx 를 오류가 아닌 응답으로 받고, 제한 시간을 걸고, OS 인증서 저장소를 쓴다. 시험용 가짜 서버(loopback)로 가는 길은 환경 프록시를 타지 않는다
fn agent_config(timeout: Duration, loopback: bool) -> ureq::config::Config {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let mut cfg = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .https_only(false)
        .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build());
    if loopback {
        cfg = cfg.proxy(None);
    }
    cfg.build()
}

/// 인증 머리 값(순수) — 민감 표시를 달아 라이브러리의 Debug·기록에 값이 안 찍히게 한다. 줄바꿈 따위가 있으면 None
fn auth_value(a: &AuthHeader) -> Option<ureq::http::HeaderValue> {
    let mut value = ureq::http::HeaderValue::from_str(&a.value).ok()?;
    value.set_sensitive(true);
    Some(value)
}

fn failure(msg: &str) -> HttpResponse {
    HttpResponse { error: Some(msg.to_string()), ..HttpResponse::default() }
}

/// ureq 오류 → 고정 글(주소·머리·본문이 들어갈 수 없다)
fn error_text(e: &ureq::Error) -> &'static str {
    use ureq::Error as E;
    match e {
        E::Timeout(_) => "응답이 없습니다(시간 초과)",
        E::Io(io) if matches!(io.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => "응답이 없습니다(시간 초과)",
        E::BodyExceedsLimit(_) => "응답이 너무 큽니다",
        E::HostNotFound | E::ConnectionFailed | E::Io(_) => "응답이 없습니다(연결 실패)",
        _ => "응답이 없습니다",
    }
}

impl ConnHttp for UreqConn {
    fn send(&self, r: &HttpRequest, auth: Option<&AuthHeader>) -> HttpResponse {
        if !self.url_allowed(&r.url) {
            return failure("보낼 수 없는 주소입니다");
        }
        let loopback = parse_url(&r.url).and_then(|u| u.host).as_deref() == Some("127.0.0.1");
        let agent: ureq::Agent = agent_config(self.timeout, loopback).into();

        let mut builder = ureq::http::Request::builder().method(r.method.as_str()).uri(r.url.as_str());
        for (k, v) in &r.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        builder = builder.header("User-Agent", USER_AGENT);
        if let Some(a) = auth {
            let Some(value) = auth_value(a) else { return failure("보낼 수 없는 요청입니다") };
            builder = builder.header(a.name.as_str(), value);
        }
        let sent = match &r.body {
            Some(body) => builder.body(body.clone()).map(|req| agent.run(req)),
            None => builder.body(()).map(|req| agent.run(req)),
        };
        let Ok(sent) = sent else { return failure("보낼 수 없는 요청입니다") };
        let mut resp = match sent {
            Ok(resp) => resp,
            Err(e) => return failure(error_text(&e)),
        };
        let status = i32::from(resp.status().as_u16());
        let headers: BTreeMap<String, String> =
            resp.headers().iter().map(|(k, v)| (k.as_str().to_lowercase(), String::from_utf8_lossy(v.as_bytes()).into_owned())).collect();
        match resp.body_mut().with_config().limit(MAX_BODY as u64).read_to_vec() {
            Ok(body) => HttpResponse { status, headers, body, error: None },
            Err(e) => failure(error_text(&e)),
        }
    }
}

// ── Tauri 명령 ───────────────────────────────────────

/// 설정 › 연결의 목록(비밀 없음)
#[tauri::command(async)]
pub fn autopilot_connections_list() -> Vec<ConnectionView> {
    connections_list(&ConnCtx::live())
}

/// 설정 › 연결의 「연결 확인」 — 결과 칸(`kind`·`ok`·`who`·`note`)과 화면 글(`text`)
#[tauri::command(async)]
pub fn autopilot_connection_check(id: String) -> Result<CheckReport, String> {
    connection_check(&ConnCtx::live(), &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_board_autopilot::rules::connections::{auth_header, NotifyFormat, NOTION_VERSION};
    use std::cell::Cell;
    use std::io::Read;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    const MARK: &str = "SECRET-MARK-9f3a";

    // ── 가짜 서버 ────────────────────────────────────
    #[derive(Debug, Clone)]
    struct Seen {
        method: String,
        target: String,
        headers: BTreeMap<String, String>,
    }

    enum Reply {
        Raw(Vec<u8>),
        /// 받고도 오래 답하지 않는다
        Hang,
    }

    struct Fake {
        addr: SocketAddr,
        seen: Arc<Mutex<Vec<Seen>>>,
        stop: Arc<AtomicBool>,
    }

    impl Fake {
        fn start(reply: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Fake {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let reply = Arc::new(reply);
            let (seen2, stop2) = (seen.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop2.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let (seen3, reply3) = (seen2.clone(), reply.clone());
                            std::thread::spawn(move || serve(stream, &seen3, &*reply3));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            });
            Fake { addr, seen, stop }
        }

        fn base(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn serve(mut stream: TcpStream, seen: &Mutex<Vec<Seen>>, reply: &(dyn Fn(&Seen) -> Reply + Send + Sync)) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 64 << 10 {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let text = String::from_utf8_lossy(&buf).into_owned();
        let mut lines = text.split("\r\n");
        let first = lines.next().unwrap_or("");
        let mut parts = first.split(' ');
        let method = parts.next().unwrap_or("").to_string();
        let target = parts.next().unwrap_or("").to_string();
        let headers = lines
            .take_while(|l| !l.is_empty())
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
            .collect();
        let s = Seen { method, target, headers };
        seen.lock().unwrap().push(s.clone());
        match reply(&s) {
            Reply::Raw(bytes) => {
                let _ = stream.write_all(&bytes);
                let _ = stream.flush();
            }
            Reply::Hang => std::thread::sleep(Duration::from_millis(2500)),
        }
    }

    fn resp(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Reply {
        let mut out = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len()).into_bytes();
        for (k, v) in headers {
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(body);
        Reply::Raw(out)
    }

    fn json(status: u16, body: &str) -> Reply {
        resp(status, &[("Content-Type", "application/json")], body.as_bytes())
    }

    /// 닫힌 포트(연결 거부)
    fn dead_base() -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        format!("http://{a}")
    }

    // ── 임시 뿌리·가짜 비밀 ──────────────────────────
    struct Tmp(PathBuf);

    impl Tmp {
        fn new() -> Tmp {
            let p = std::env::temp_dir().join(format!("cb-ap-conn-{}", crate::util::short_id()));
            fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }

        fn ctx(&self, dev: bool) -> ConnCtx {
            ConnCtx::new(&self.0, dev)
        }

        fn write(&self, list: &[Connection]) {
            fs::write(self.0.join("connections.json"), serde_json::to_vec(&connections_json(list)).unwrap()).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Vault {
        items: BTreeMap<String, String>,
        reads: Cell<usize>,
    }

    impl Vault {
        fn with(items: &[(&str, &str)]) -> Vault {
            Vault { items: items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), reads: Cell::new(0) }
        }
    }

    impl ConnSecrets for Vault {
        fn read(&self, id: &str) -> Option<String> {
            self.reads.set(self.reads.get() + 1);
            self.items.get(id).cloned()
        }

        fn exists(&self, id: &str) -> bool {
            self.items.contains_key(id)
        }
    }

    fn conn(id: &str, service: Service, site: &str) -> Connection {
        Connection { id: id.into(), service, site: site.into(), account: "kim@example.com".into(), ..Connection::default() }
    }

    fn fast(dev: bool) -> UreqConn {
        UreqConn::with_timeout(dev, Duration::from_millis(400))
    }

    fn run(site_conn: Connection, secret: &str, dev: bool, fake_http: &UreqConn) -> CheckReport {
        let t = Tmp::new();
        let id = site_conn.id.clone();
        t.write(&[site_conn]);
        check_in(&t.ctx(dev), &id, &Vault::with(&[(&id, secret)]), fake_http).unwrap()
    }

    fn atlassian_ok_body() -> &'static str {
        r#"{"displayName":"Kim Tester","emailAddress":"kim@example.com"}"#
    }

    // ── 목록 ─────────────────────────────────────────
    #[test]
    fn 목록에는_비밀이_없다() {
        let t = Tmp::new();
        // 파일에 낯선 token·secret 칸이 있어도, webhook 의 주소 전체가 어딘가에 있어도 목록 값에 안 나온다
        let raw = format!(
            r#"{{"format":"claude-board-connections","version":1,"connections":[
              {{"id":"atlassian","service":"atlassian","site":"https://x.atlassian.net","account":"kim@example.com","token":"{MARK}","secret":"{MARK}","expires":"2999-01-31","checked":{{"at":1700000000,"ok":true,"who":"Kim","note":"ok"}}}},
              {{"id":"webhook-1","service":"webhook","name":"팀 대화방","host":"hooks.example.com","url":"https://hooks.example.com/{MARK}","format":"slack","scopes":["notify"]}},
              {{"id":"notion","service":"notion","password":"{MARK}"}}
            ]}}"#
        );
        fs::write(t.0.join("connections.json"), raw).unwrap();
        let vault = Vault::with(&[("atlassian", MARK), ("webhook-1", &format!("https://hooks.example.com/{MARK}"))]);
        let list = list_in(&t.ctx(false), &vault, 1_700_000_000.0);
        assert_eq!(list.len(), 3);
        let text = serde_json::to_string(&list).unwrap();
        assert!(!text.contains(MARK), "직렬화 값에 비밀 표지가 있다: {text}");
        assert!(!format!("{list:?}").contains(MARK), "Debug 에 비밀 표지가 있다");
        assert!(!text.to_lowercase().contains("\"token\"") && !text.contains("\"secret\"") && !text.contains("\"password\""));
        let (a, w, n) = (&list[0], &list[1], &list[2]);
        assert_eq!((a.id.as_str(), a.service.as_str(), a.display_name.as_str(), a.site.as_str()), ("atlassian", "atlassian", "Atlassian", "https://x.atlassian.net"));
        assert!(a.token_saved && w.token_saved && !n.token_saved);
        assert_eq!(a.checked, Some(CheckView { at: 1_700_000_000.0, ok: true, who: "Kim".into(), note: "ok".into() }));
        assert_eq!((w.display_name.as_str(), w.host.as_str(), w.format.as_str()), ("팀 대화방", "hooks.example.com", "slack"));
        assert_eq!(a.format, "");
        assert!(n.checked.is_none());
    }

    #[test]
    fn 목록은_만료_글과_상태를_보인다() {
        let t = Tmp::new();
        let mut a = conn("atlassian", Service::Atlassian, "https://x.atlassian.net");
        a.expires = "2000-01-01".into();
        let mut f = conn("figma", Service::Figma, "");
        f.expires = "2199-12-31".into();
        let n = conn("notion", Service::Notion, "");
        t.write(&[a, f, n]);
        let list = list_in(&t.ctx(false), &Vault::with(&[]), clock::now());
        assert_eq!(list[0].expiry_state, "expired");
        assert!(list[0].expiry_label.starts_with("만료됨 · "), "{}", list[0].expiry_label);
        assert_eq!(list[1].expiry_state, "ok");
        assert!(list[1].expiry_label.contains("2199-12-31"));
        assert_eq!((list[2].expiry_state.as_str(), list[2].expiry_label.as_str()), ("none", "만료일 없음"));
        assert!(!list[0].token_saved);
    }

    #[test]
    fn 목록_파일이_없거나_크거나_링크면_빈_목록이다() {
        let t = Tmp::new();
        assert!(list_in(&t.ctx(false), &Vault::with(&[]), 0.0).is_empty());
        fs::write(t.0.join("connections.json"), vec![b' '; (MAX_LIST_FILE + 1) as usize]).unwrap();
        assert!(list_in(&t.ctx(false), &Vault::with(&[]), 0.0).is_empty());
        // 상한 안의 정상 파일은 읽힌다(상한 경계)
        t.write(&[conn("figma", Service::Figma, "")]);
        assert_eq!(list_in(&t.ctx(false), &Vault::with(&[]), 0.0).len(), 1);
        fs::write(t.0.join("connections.json"), "not json").unwrap();
        assert!(list_in(&t.ctx(false), &Vault::with(&[]), 0.0).is_empty());
    }

    #[test]
    fn 목록_파일이_디렉터리면_읽지_않는다() {
        let t = Tmp::new();
        fs::create_dir(t.0.join("connections.json")).unwrap();
        assert!(list_in(&t.ctx(false), &Vault::with(&[]), 0.0).is_empty());
    }

    // ── 요청 꼴 ──────────────────────────────────────
    fn assert_matches_rule(c: &Connection, fake: &Fake, dev: bool) {
        use claude_board_autopilot::rules::connections::check_request;
        let want = check_request(c, dev).unwrap();
        let seen = fake.seen();
        assert_eq!(seen.len(), 1, "요청은 정확히 한 번");
        let s = &seen[0];
        assert_eq!(s.method, want.method);
        assert_eq!(format!("{}{}", fake.base(), s.target), want.url, "주소");
        for (k, v) in &want.headers {
            assert_eq!(s.headers.get(&k.to_lowercase()), Some(v), "머리 {k}");
        }
        assert_eq!(s.headers.get("user-agent").map(String::as_str), Some("Claude Board autopilot"));
    }

    #[test]
    fn atlassian_은_기본_인증으로_읽는다() {
        let fake = Fake::start(|_| json(200, atlassian_ok_body()));
        let c = conn("atlassian", Service::Atlassian, &fake.base());
        let r = run(c.clone(), MARK, true, &fast(true));
        assert_eq!((r.kind.as_str(), r.ok, r.who.as_str(), r.text.as_str()), ("ok", true, "Kim Tester", "연결됨 · Kim Tester"));
        assert_matches_rule(&c, &fake, true);
        let s = &fake.seen()[0];
        let want = format!("Basic {}", b64(format!("kim@example.com:{MARK}").as_bytes()));
        assert_eq!(s.headers.get("authorization"), Some(&want));
        assert_eq!(s.target, "/rest/api/3/myself");
        assert!(!s.headers.contains_key("x-figma-token") && !s.headers.contains_key("notion-version"));
    }

    #[test]
    fn notion_은_bearer_와_버전_머리를_보낸다() {
        let fake = Fake::start(|_| json(200, r#"{"name":"Bot","bot":{"owner":{"user":{"name":"Lee"}}}}"#));
        let c = conn("notion", Service::Notion, &fake.base());
        let r = run(c.clone(), MARK, true, &fast(true));
        assert_eq!((r.kind.as_str(), r.who.as_str()), ("ok", "Bot · Lee"));
        assert_matches_rule(&c, &fake, true);
        let s = &fake.seen()[0];
        assert_eq!(s.headers.get("authorization"), Some(&format!("Bearer {MARK}")));
        assert_eq!(s.headers.get("notion-version").map(String::as_str), Some(NOTION_VERSION));
        assert_eq!(s.target, "/v1/users/me");
        assert!(!s.headers.contains_key("x-figma-token"));
    }

    #[test]
    fn figma_는_x_figma_token_만_보낸다() {
        let fake = Fake::start(|_| json(200, r#"{"email":"kim@example.com","handle":"kim"}"#));
        let c = conn("figma", Service::Figma, &fake.base());
        let r = run(c.clone(), MARK, true, &fast(true));
        assert_eq!((r.kind.as_str(), r.who.as_str()), ("ok", "kim@example.com"));
        assert_matches_rule(&c, &fake, true);
        let s = &fake.seen()[0];
        assert_eq!(s.headers.get("x-figma-token").map(String::as_str), Some(MARK));
        assert!(!s.headers.contains_key("authorization") && !s.headers.contains_key("notion-version"));
        assert_eq!(s.target, "/v1/me");
    }

    fn b64(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in data.chunks(3) {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            out.push(T[(n >> 18) as usize & 63] as char);
            out.push(T[(n >> 12) as usize & 63] as char);
            out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
        }
        out
    }

    // ── 응답 → 결과 ──────────────────────────────────
    #[test]
    fn 상태_코드마다_규칙의_결과로_간다() {
        let cases: [(u16, &str, &str); 6] = [
            (401, "rejected", "거부됨(401): 토큰이 틀렸거나 만료·취소됐습니다"),
            (403, "rejected", "거부됨(403): 토큰이 틀렸거나 만료·취소됐습니다"),
            (404, "failed", "받는 쪽이 404 로 답했습니다"),
            (500, "failed", "받는 쪽이 500 로 답했습니다"),
            (429, "failed", "요청이 많다며 거절했습니다(429). 잠시 뒤 다시 확인합니다"),
            (200, "failed", "응답 꼴이 예상과 다릅니다"),
        ];
        for (status, kind, note) in cases {
            let body = if status == 200 { "<html>garbage</html>" } else { "{}" };
            let fake = Fake::start(move |_| json(status, body));
            let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, true, &fast(true));
            assert_eq!((r.kind.as_str(), r.ok, r.note.as_str(), r.text.as_str()), (kind, false, note, note), "{status}");
            assert!(!format!("{r:?}").contains(MARK));
        }
    }

    #[test]
    fn 시간_초과는_고정_글이다() {
        let fake = Fake::start(|_| Reply::Hang);
        let started = std::time::Instant::now();
        let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, true, &fast(true));
        assert!(started.elapsed() < Duration::from_millis(2300), "제한 시간이 안 걸렸다: {:?}", started.elapsed());
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("failed", "응답이 없습니다(시간 초과)"));
        assert!(!format!("{r:?}").contains(MARK));
    }

    #[test]
    fn 연결_거부는_고정_글이다() {
        let base = dead_base();
        // Windows 는 거부된 연결도 재시도로 2초쯤 걸린다 — 시간 초과와 가르려고 넉넉히 준다
        let r = run(conn("figma", Service::Figma, &base), MARK, true, &UreqConn::with_timeout(true, Duration::from_secs(10)));
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("failed", "응답이 없습니다(연결 실패)"));
        assert!(!format!("{r:?}").contains(MARK) && !r.note.contains("127.0.0.1"));
    }

    #[test]
    fn 쓰레기_본문과_빈_본문은_꼴_오류다() {
        for body in ["", "\u{0}\u{1}garbage", "[1,2,3]", "{\"displayName\":123}"] {
            let fake = Fake::start(move |_| resp(200, &[], body.as_bytes()));
            let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, true, &fast(true));
            assert_eq!((r.kind.as_str(), r.note.as_str()), ("failed", "응답 꼴이 예상과 다릅니다"), "{body:?}");
        }
    }

    #[test]
    fn 거대한_본문은_상한에서_끊긴다() {
        let big = vec![b'a'; MAX_BODY + (1 << 20)];
        let fake = Fake::start(move |_| resp(200, &[], &big));
        let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, true, &UreqConn::with_timeout(true, Duration::from_secs(10)));
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("failed", "응답이 너무 큽니다"));
    }

    #[test]
    fn 상한_안의_본문은_읽는다() {
        // 4 MiB 바로 아래의 JSON(이름 칸 뒤에 공백을 채운다)
        let mut body = atlassian_ok_body().as_bytes().to_vec();
        body.resize(MAX_BODY - 1, b' ');
        let fake = Fake::start(move |_| resp(200, &[], &body));
        let adapter = UreqConn::with_timeout(true, Duration::from_secs(10));
        let http = adapter.send(
            &HttpRequest { url: format!("{}/x", fake.base()), ..HttpRequest::default() },
            None,
        );
        assert_eq!((http.status, http.error.clone(), http.body.len()), (200, None, MAX_BODY - 1));
    }

    // ── 리다이렉트 ───────────────────────────────────
    #[test]
    fn 다른_호스트로_보내는_3xx_는_따르지_않고_비밀도_안_간다() {
        for status in [301u16, 302, 303, 307, 308] {
            let other = Fake::start(|_| json(200, atlassian_ok_body()));
            let target = format!("{}/steal", other.base());
            let fake = Fake::start(move |_| resp(status, &[("Location", &target)], b""));
            let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, true, &fast(true));
            assert_eq!(r.kind, "failed", "{status}");
            assert_eq!(r.note, format!("다른 주소로 보내려 해 따라가지 않았습니다({status})"));
            assert_eq!(fake.seen().len(), 1);
            std::thread::sleep(Duration::from_millis(150));
            assert!(other.seen().is_empty(), "{status}: 다른 서버가 요청을 받았다: {:?}", other.seen());
            assert!(!format!("{r:?}").contains(MARK));
        }
    }

    #[test]
    fn 같은_사이트로의_3xx_도_규칙이_따르지_않는다() {
        let fake = Fake::start(|s| {
            if s.target == "/rest/api/3/myself" {
                resp(302, &[("Location", "/rest/api/3/myself2")], b"")
            } else {
                json(200, atlassian_ok_body())
            }
        });
        let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, true, &fast(true));
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("failed", "다른 주소로 보내려 해 따라가지 않았습니다(302)"));
        assert_eq!(fake.seen().len(), 1, "규칙에 따라가기가 없다");
    }

    // ── 요청이 없어야 하는 경우 ──────────────────────
    #[test]
    fn 토큰이_없으면_요청_없이_규칙의_글이다() {
        let fake = Fake::start(|_| json(200, atlassian_ok_body()));
        let t = Tmp::new();
        t.write(&[conn("atlassian", Service::Atlassian, &fake.base())]);
        let r = check_in(&t.ctx(true), "atlassian", &Vault::with(&[]), &fast(true)).unwrap();
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("local", "토큰이 키체인에 없어 요청하지 않았습니다"));
        assert!(fake.seen().is_empty());
    }

    #[test]
    fn 계정_메일이_없으면_요청_없이_끝난다() {
        let fake = Fake::start(|_| json(200, atlassian_ok_body()));
        let t = Tmp::new();
        let mut c = conn("atlassian", Service::Atlassian, &fake.base());
        c.account = String::new();
        t.write(&[c]);
        let r = check_in(&t.ctx(true), "atlassian", &Vault::with(&[("atlassian", MARK)]), &fast(true)).unwrap();
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("local", "계정 메일이 없어 요청하지 않았습니다"));
        assert!(fake.seen().is_empty());
    }

    #[test]
    fn 시험_빌드가_아니면_http_127_사이트에_요청하지_않는다() {
        let fake = Fake::start(|_| json(200, atlassian_ok_body()));
        let r = run(conn("atlassian", Service::Atlassian, &fake.base()), MARK, false, &fast(false));
        assert_eq!((r.kind.as_str(), r.note.as_str()), ("local", "사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다"));
        assert!(fake.seen().is_empty());
    }

    #[test]
    fn 시험_빌드여도_http_127_밖의_주소는_요청하지_않는다() {
        for site in ["http://localhost:9", "http://example.com", "ftp://127.0.0.1", "https://u:p@x.atlassian.net"] {
            let r = run(conn("atlassian", Service::Atlassian, site), MARK, true, &fast(true));
            assert_eq!(r.kind, "local", "{site}");
            assert!(!format!("{r:?}").contains(MARK));
        }
    }

    #[test]
    fn 시험_빌드가_아니면_notion_figma_는_고정_주소로_간다() {
        // 사이트 칸에 가짜 서버를 적어도 설치판은 api.notion.com 으로만 간다 — 규칙의 바탕 주소가 이를 정한다
        use claude_board_autopilot::rules::connections::{check_request, FIGMA_API, NOTION_API};
        let fake = Fake::start(|_| json(200, "{}"));
        let n = conn("notion", Service::Notion, &fake.base());
        let f = conn("figma", Service::Figma, &fake.base());
        assert!(check_request(&n, false).unwrap().url.starts_with(NOTION_API));
        assert!(check_request(&f, false).unwrap().url.starts_with(FIGMA_API));
        assert!(check_request(&n, true).unwrap().url.starts_with(&fake.base()));
    }

    #[test]
    fn webhook_은_요청_없이_주소_꼴만_본다() {
        let fake = Fake::start(|_| json(200, "{}"));
        let t = Tmp::new();
        let mut w = conn("webhook-1", Service::Webhook, "");
        w.host = "hooks.example.com".into();
        t.write(&[w]);
        let with = check_in(&t.ctx(true), "webhook-1", &Vault::with(&[("webhook-1", &format!("https://hooks.example.com/{MARK}"))]), &fast(true)).unwrap();
        assert_eq!(with.kind, "local");
        assert!(with.note.starts_with("webhook 은 읽는 요청이 없어"), "{}", with.note);
        let without = check_in(&t.ctx(true), "webhook-1", &Vault::with(&[]), &fast(true)).unwrap();
        assert_eq!(without.note, "webhook 주소가 키체인에 없습니다");
        assert!(fake.seen().is_empty());
        assert!(!format!("{with:?}{without:?}").contains(MARK));
    }

    #[test]
    fn 목록에_없는_id_는_요청_없이_안내한다() {
        let t = Tmp::new();
        t.write(&[conn("figma", Service::Figma, "")]);
        let v = Vault::with(&[("notion", MARK)]);
        let r = check_in(&t.ctx(true), "notion", &v, &fast(true)).unwrap();
        assert_eq!((r.kind.as_str(), r.ok), ("local", false));
        assert_eq!(r.note, "연결 목록에 notion 가 없습니다(먼저 저장합니다)");
        assert_eq!(v.reads.get(), 0, "목록에 없으면 비밀을 읽지 않는다");
    }

    // ── id 거절 ──────────────────────────────────────
    #[test]
    fn 틀린_id_는_경로도_요청도_만들기_전에_거절한다() {
        let fake = Fake::start(|_| json(200, atlassian_ok_body()));
        let gone = std::env::temp_dir().join(format!("cb-ap-conn-none-{}", crate::util::short_id()));
        let ctx = ConnCtx::new(&gone, true);
        let v = Vault::with(&[("atlassian", MARK)]);
        for id in ["", "../atlassian", "atlassian/../x", "atlassian\\x", "ATLASSIAN", "webhook-0", "webhook-1000", "atlassian\n", "con", "C:\\x", "a b", "atlassian "] {
            let e = check_in(&ctx, id, &v, &fast(true)).unwrap_err();
            assert_eq!(e, "연결 id 꼴이 아닙니다", "{id:?}");
            assert!(connection_check(&ctx, id).is_err() && connection_test_text(&ctx, id).is_err(), "{id:?}");
        }
        assert!(!gone.exists(), "거절 전에 폴더가 만들어졌다");
        assert_eq!(v.reads.get(), 0);
        assert!(fake.seen().is_empty());
    }

    // ── 비밀이 안 샌다 ───────────────────────────────
    #[test]
    fn 어댑터의_모든_오류_글에_비밀이_없다() {
        let auth = AuthHeader { name: "Authorization".into(), value: format!("Bearer {MARK}") };
        let dead = dead_base();
        let hang = Fake::start(|_| Reply::Hang);
        let big = Fake::start(|_| resp(200, &[], &vec![b'a'; MAX_BODY + 10]));
        let bad_header = AuthHeader { name: "Authorization".into(), value: format!("Bearer {MARK}\r\nX-Evil: 1") };
        let a = UreqConn::with_timeout(true, Duration::from_millis(400));
        let mut outs = vec![
            a.send(&HttpRequest { url: format!("{dead}/x"), ..HttpRequest::default() }, Some(&auth)),
            a.send(&HttpRequest { url: format!("{}/x", hang.base()), ..HttpRequest::default() }, Some(&auth)),
            UreqConn::with_timeout(true, Duration::from_secs(10)).send(&HttpRequest { url: format!("{}/x", big.base()), ..HttpRequest::default() }, Some(&auth)),
            a.send(&HttpRequest { url: "http://example.com/x".into(), ..HttpRequest::default() }, Some(&auth)),
            a.send(&HttpRequest { url: format!("{dead}/x"), ..HttpRequest::default() }, Some(&bad_header)),
            a.send(&HttpRequest { url: "not a url".into(), ..HttpRequest::default() }, Some(&auth)),
            a.send(&HttpRequest { method: "BAD METHOD".into(), url: format!("{dead}/x"), ..HttpRequest::default() }, Some(&auth)),
        ];
        outs.push(UreqConn::new(false).send(&HttpRequest { url: format!("{dead}/x"), ..HttpRequest::default() }, Some(&auth)));
        outs.push(a.send(&HttpRequest { url: "http://user:pw@127.0.0.1:9/x".into(), ..HttpRequest::default() }, Some(&auth)));
        for o in &outs {
            assert!(o.error.is_some(), "{o:?}");
            assert_eq!(o.status, 0);
            let s = format!("{o:?}");
            assert!(!s.contains(MARK) && !s.contains("127.0.0.1") && !s.contains("Bearer"), "{s}");
        }
        assert_eq!(outs[4].error.as_deref(), Some("보낼 수 없는 요청입니다"));
        assert_eq!(outs[3].error.as_deref(), Some("보낼 수 없는 주소입니다"));
        assert_eq!(outs[7].error.as_deref(), Some("보낼 수 없는 주소입니다"), "시험 빌드가 아니면 http 어댑터가 거절");
        assert_eq!(outs[8].error.as_deref(), Some("보낼 수 없는 주소입니다"), "아이디·암호가 든 주소는 시험 빌드에서도 거절");
        assert!(hang.seen().len() == 1 && format!("{a:?}").contains("timeout") && !format!("{a:?}").contains(MARK));
    }

    #[test]
    fn 서버가_받은_인증_머리가_민감_표시로_가려도_값은_맞다() {
        // 보내는 값은 평문이어야 하고(서버가 받는다), 우리 쪽 값 타입은 Debug 로 가려진다
        let h = auth_header(&claude_board_autopilot::rules::connections::Auth::Bearer, MARK).unwrap();
        assert!(!format!("{h:?}").contains(MARK));
        let fake = Fake::start(|_| json(200, "{}"));
        let o = fast(true).send(&HttpRequest { url: format!("{}/x", fake.base()), ..HttpRequest::default() }, Some(&h));
        assert_eq!(o.status, 200);
        assert_eq!(fake.seen()[0].headers.get("authorization"), Some(&format!("Bearer {MARK}")));
        assert_eq!(o.headers.get("content-type").map(String::as_str), Some("application/json"), "응답 머리 이름은 소문자");
    }

    // ── 시험 글 ──────────────────────────────────────
    #[test]
    fn 시험_글은_성공이면_이름_실패면_까닭이다() {
        let t = Tmp::new();
        let ok = Fake::start(|_| json(200, atlassian_ok_body()));
        let rejected = Fake::start(|_| json(401, "{}"));
        t.write(&[conn("atlassian", Service::Atlassian, &ok.base())]);
        let vault = Vault::with(&[("atlassian", MARK)]);
        let r = check_in(&t.ctx(true), "atlassian", &vault, &fast(true)).unwrap();
        assert_eq!((r.text.as_str(), r.note.as_str()), ("연결됨 · Kim Tester", "읽기 요청이 됐습니다"));
        t.write(&[conn("atlassian", Service::Atlassian, &rejected.base())]);
        let r = check_in(&t.ctx(true), "atlassian", &vault, &fast(true)).unwrap();
        assert_eq!(r.text, "거부됨(401): 토큰이 틀렸거나 만료·취소됐습니다");
        assert_eq!(r.text, r.note);
    }

    #[test]
    fn 화면_글은_줄바꿈을_펴고_300자로_자른다() {
        assert_eq!(doc_text("  a\nb\r\tc  ", 300), "a b  c");
        let long = "가".repeat(400);
        assert_eq!(doc_text(&long, NOTE_MAX).chars().count(), 300);
        let r = report_of(CheckOutcome::Failed(long));
        assert_eq!(r.note.chars().count(), 300);
        assert_eq!(r.text, r.note);
        let r = report_of(CheckOutcome::Ok { who: "나".repeat(200) });
        assert_eq!(r.who.chars().count(), 120);
        assert_eq!(r.kind, "ok");
    }

    #[test]
    fn 보고_꼴은_엔진_json_칸과_같다() {
        let r = report_of(CheckOutcome::Rejected("거부".into()));
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v["kind"], "rejected");
        assert_eq!(v["ok"], false);
        assert_eq!(v["who"], "");
        assert_eq!(v["note"], "거부");
        assert_eq!(v["text"], "거부");
        let l = report_of(CheckOutcome::Local("x".into()));
        assert_eq!(l.kind, "local");
        let f = report_of(CheckOutcome::Failed("x".into()));
        assert_eq!(f.kind, "failed");
    }

    #[test]
    fn 상한_상수는_글자_그대로의_값이다() {
        // 시험이 상수로 만든 입력은 상수가 커져도 같이 커져 못 잡는다 — 값을 따로 못박는다
        assert_eq!(MAX_BODY, 4 * 1024 * 1024);
        assert_eq!(MAX_LIST_FILE, 256 * 1024);
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(20));
        assert_eq!(NOTE_MAX, 300);
    }

    #[test]
    fn 만료가_가까우면_soon_멀면_ok() {
        let day = |plus: f64| {
            let l = clock::local(clock::now() + plus * 86400.0);
            format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
        };
        let t = Tmp::new();
        let mut a = conn("atlassian", Service::Atlassian, "https://x.atlassian.net");
        a.expires = day(5.0);
        let mut f = conn("figma", Service::Figma, "");
        f.expires = day(60.0);
        t.write(&[a, f]);
        let list = list_in(&t.ctx(false), &Vault::with(&[]), clock::now());
        assert_eq!(list[0].expiry_state, "soon");
        assert!(list[0].expiry_label.starts_with("만료 임박 · "), "{}", list[0].expiry_label);
        assert_eq!(list[1].expiry_state, "ok");
    }

    #[test]
    fn 실제_보드_자리는_엔진과_같은_뿌리다() {
        let live = ConnCtx::live();
        let sep = std::path::MAIN_SEPARATOR;
        let list = live.list_path().to_string_lossy().into_owned();
        let vault = live.secrets_dir().to_string_lossy().into_owned();
        assert!(list.ends_with(&format!("autopilot{sep}connections.json")), "{list}");
        assert!(vault.ends_with(&format!("autopilot{sep}private{sep}connection-secrets")), "{vault}");
        // 보드의 자율주행 읽기(`board.rs`)가 쓰는 뿌리와 같다
        assert_eq!(live.root, paths::autopilot_root(false));
        assert!(!list.contains("Claude Board Dev"), "{list}");
    }

    // ── 순수 설정 ────────────────────────────────────
    struct EnvGuard(Vec<&'static str>);

    impl EnvGuard {
        fn set(keys: &[&'static str], value: &str) -> EnvGuard {
            for k in keys {
                std::env::set_var(k, value);
            }
            EnvGuard(keys.to_vec())
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for k in &self.0 {
                std::env::remove_var(k);
            }
        }
    }

    #[test]
    fn 에이전트_설정은_따라가지_않고_응답으로_받는다() {
        let t = Duration::from_millis(1234);
        let c = agent_config(t, true);
        assert_eq!(c.max_redirects(), 0);
        assert_eq!(c.timeouts().global, Some(t));
        assert!(!c.http_status_as_error());
        assert!(!c.https_only());
    }

    #[test]
    fn loopback_길은_환경_프록시를_타지_않는다() {
        let _env = EnvGuard::set(&["ALL_PROXY", "HTTPS_PROXY", "HTTP_PROXY", "all_proxy", "https_proxy", "http_proxy"], "http://127.0.0.1:9");
        // 전제: 환경에 프록시가 있으면 바깥 길은 그것을 쓴다
        assert!(agent_config(Duration::from_secs(1), false).proxy().is_some(), "전제가 안 선다(환경 프록시를 읽지 못함)");
        assert!(agent_config(Duration::from_secs(1), true).proxy().is_none());
    }

    #[test]
    fn 인증_머리_값은_민감_표시가_붙고_줄바꿈은_거절한다() {
        let ok = auth_value(&AuthHeader { name: "Authorization".into(), value: format!("Bearer {MARK}") }).unwrap();
        assert!(ok.is_sensitive());
        assert_eq!(ok.to_str().unwrap(), format!("Bearer {MARK}"));
        assert!(!format!("{ok:?}").contains(MARK), "민감 값은 Debug 에 안 찍힌다");
        assert!(auth_value(&AuthHeader { name: "Authorization".into(), value: format!("Bearer {MARK}\r\nX: 1") }).is_none());
    }

    #[test]
    fn 목록_파일_판정은_일반_파일만_받는다() {
        let t = Tmp::new();
        let dir = t.0.join("connections.json");
        fs::create_dir(&dir).unwrap();
        assert!(!list_file_ok(&fs::symlink_metadata(&dir).unwrap()), "같은 이름의 폴더");
        let file = t.0.join("ok.json");
        fs::write(&file, "{}").unwrap();
        assert!(list_file_ok(&fs::symlink_metadata(&file).unwrap()));
        let big = t.0.join("big.json");
        fs::write(&big, vec![b' '; (MAX_LIST_FILE + 1) as usize]).unwrap();
        assert!(!list_file_ok(&fs::symlink_metadata(&big).unwrap()));
        let edge = t.0.join("edge.json");
        fs::write(&edge, vec![b' '; MAX_LIST_FILE as usize]).unwrap();
        assert!(list_file_ok(&fs::symlink_metadata(&edge).unwrap()), "상한 바로 그 크기는 받는다");
    }

    // ── 확인 결과 적기 ───────────────────────────────
    fn base_list() -> Vec<Connection> {
        let mut f = conn("figma", Service::Figma, "");
        f.checked = Some(CheckRecord { at: 1.0, ok: false, who: String::new(), note: "지난 기록".into() });
        let mut w = conn("webhook-1", Service::Webhook, "");
        w.host = "hooks.example.com".into();
        w.name = "대화방".into();
        w.scopes = vec!["notify".into()];
        vec![conn("atlassian", Service::Atlassian, "https://x.atlassian.net"), conn("notion", Service::Notion, ""), f, w]
    }

    fn names(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn 적으면_그_연결의_checked_만_바뀌고_나머지는_그대로다() {
        let t = Tmp::new();
        t.write(&base_list());
        let before = parse_connections(Some(&fs::read(t.0.join("connections.json")).unwrap()));
        let done = record_check(&t.0, "atlassian", &CheckOutcome::Ok { who: "Kim Tester".into() }, 1_700_000_000.0).unwrap();
        assert!(done);
        let after = parse_connections(Some(&fs::read(t.0.join("connections.json")).unwrap()));
        assert_eq!(after.len(), 4);
        let a = after.iter().find(|c| c.id == "atlassian").unwrap();
        assert_eq!(a.checked, Some(CheckRecord { at: 1_700_000_000.0, ok: true, who: "Kim Tester".into(), note: "읽기 요청이 됐습니다".into() }));
        for (b, c) in before.iter().zip(&after) {
            if b.id != "atlassian" {
                assert_eq!(b, c, "{} 가 바뀌었다", b.id);
            } else {
                assert_eq!(Connection { checked: None, ..b.clone() }, Connection { checked: None, ..c.clone() });
            }
        }
        assert_eq!(names(&t.0), ["connections.json"], "임시 파일이 남았다");
        // 목록으로 읽으면 화면 값에도 실린다
        let list = list_in(&t.ctx(false), &Vault::with(&[]), 0.0);
        assert_eq!(list[0].checked.as_ref().map(|k| (k.ok, k.who.as_str())), Some((true, "Kim Tester")));
    }

    #[test]
    fn 실패와_거부도_적고_지난_기록을_덮는다() {
        let t = Tmp::new();
        t.write(&base_list());
        assert!(record_check(&t.0, "figma", &CheckOutcome::Rejected("거부됨(401): 토큰이 틀렸거나 만료·취소됐습니다".into()), 5.0).unwrap());
        assert!(record_check(&t.0, "notion", &CheckOutcome::Failed("받는 쪽이 500 로 답했습니다".into()), 6.0).unwrap());
        let list = list_in(&t.ctx(false), &Vault::with(&[]), 0.0);
        let f = list.iter().find(|c| c.id == "figma").unwrap().checked.clone().unwrap();
        assert_eq!((f.at, f.ok, f.who.as_str(), f.note.as_str()), (5.0, false, "", "거부됨(401): 토큰이 틀렸거나 만료·취소됐습니다"));
        let n = list.iter().find(|c| c.id == "notion").unwrap().checked.clone().unwrap();
        assert_eq!((n.at, n.ok, n.note.as_str()), (6.0, false, "받는 쪽이 500 로 답했습니다"));
    }

    #[test]
    fn 요청_없이_끝난_결과는_지난_기록을_덮지_않는다() {
        let t = Tmp::new();
        t.write(&base_list());
        let bytes = fs::read(t.0.join("connections.json")).unwrap();
        let done = record_check(&t.0, "figma", &CheckOutcome::Local("토큰이 키체인에 없어 요청하지 않았습니다".into()), 9.0).unwrap();
        assert!(!done);
        assert_eq!(fs::read(t.0.join("connections.json")).unwrap(), bytes, "파일이 바뀌었다");
    }

    #[test]
    fn 모르는_id_와_틀린_id_는_파일을_건드리지_않고_거절한다() {
        let t = Tmp::new();
        t.write(&[conn("figma", Service::Figma, "")]);
        let bytes = fs::read(t.0.join("connections.json")).unwrap();
        let ok = CheckOutcome::Ok { who: "x".into() };
        let e = record_check(&t.0, "notion", &ok, 1.0).unwrap_err();
        assert!(e.contains("notion") && e.contains("기록하지 않았습니다"), "{e}");
        assert_eq!(fs::read(t.0.join("connections.json")).unwrap(), bytes);
        for id in ["", "../figma", "webhook-0", "FIGMA"] {
            assert_eq!(record_check(&t.0, id, &ok, 1.0).unwrap_err(), "연결 id 꼴이 아닙니다", "{id:?}");
        }
        assert_eq!(fs::read(t.0.join("connections.json")).unwrap(), bytes);
        assert_eq!(names(&t.0), ["connections.json"]);
        // 틀린 id 는 Local 결과여도 먼저 거절한다
        assert!(record_check(&t.0, "../x", &CheckOutcome::Local("x".into()), 1.0).is_err());
    }

    #[test]
    fn 깨진_목록_큰_목록_없는_목록은_그대로_두고_거절한다() {
        let ok = CheckOutcome::Ok { who: "x".into() };
        let t = Tmp::new();
        for body in [b"not json".to_vec(), b"{\"format\":\"claude-board-connections\",\"connections\":[1,2]}".to_vec(), vec![b' '; (MAX_LIST_FILE + 1) as usize]] {
            fs::write(t.0.join("connections.json"), &body).unwrap();
            assert!(record_check(&t.0, "figma", &ok, 1.0).is_err());
            assert_eq!(fs::read(t.0.join("connections.json")).unwrap(), body, "깨진 파일을 건드렸다");
        }
        let empty = Tmp::new();
        assert!(record_check(&empty.0, "figma", &ok, 1.0).is_err());
        assert!(names(&empty.0).is_empty(), "없던 목록을 만들었다");
    }

    #[test]
    fn 쓰기가_실패하면_옛_파일이_남고_임시_파일은_지운다() {
        let t = Tmp::new();
        // 이름 바꾸기 실패: 대상이 비어 있지 않은 폴더
        let target = t.0.join("list.json");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "x").unwrap();
        assert!(write_atomic(&target, b"new").is_err());
        assert_eq!(names(&t.0), ["list.json"], "임시 파일이 남았다");
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"x");
        // 임시 파일 만들기 실패: 폴더가 없다
        assert!(write_atomic(&t.0.join("nope").join("a.json"), b"x").is_err());
        // 정상: 옛 내용을 통째로 바꾼다
        let f = t.0.join("a.json");
        fs::write(&f, "old old old old old").unwrap();
        write_atomic(&f, b"new").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"new");
        assert_eq!(names(&t.0), ["a.json", "list.json"]);
    }

    #[test]
    fn 확인하면_목록에_적히고_비밀은_파일에_없다() {
        let fake = Fake::start(|_| json(200, atlassian_ok_body()));
        let t = Tmp::new();
        t.write(&[conn("atlassian", Service::Atlassian, &fake.base()), conn("figma", Service::Figma, "")]);
        let vault = Vault::with(&[("atlassian", MARK)]);
        let r = check_and_record_in(&t.ctx(true), "atlassian", &vault, &fast(true), 42.0).unwrap();
        assert_eq!(r.kind, "ok");
        let raw = fs::read_to_string(t.0.join("connections.json")).unwrap();
        assert!(!raw.contains(MARK) && !raw.to_lowercase().contains("authorization"), "{raw}");
        let list = list_in(&t.ctx(true), &vault, 0.0);
        assert_eq!(list[0].checked, Some(CheckView { at: 42.0, ok: true, who: "Kim Tester".into(), note: "읽기 요청이 됐습니다".into() }));
        // 토큰이 없어 요청 없이 끝난 확인은 기록하지 않는다(figma 는 지난 기록도 없다)
        let r = check_and_record_in(&t.ctx(true), "figma", &Vault::with(&[]), &fast(true), 43.0).unwrap();
        assert_eq!(r.kind, "local");
        assert!(list_in(&t.ctx(true), &vault, 0.0)[1].checked.is_none());
        assert_eq!(fake.seen().len(), 1);
    }

    #[test]
    fn 동시에_적어도_파일이_깨지거나_기록이_사라지지_않는다() {
        let t = Tmp::new();
        t.write(&base_list());
        let path = t.0.join("connections.json");
        let dir = t.0.clone();
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (path, done) = (path.clone(), done.clone());
            std::thread::spawn(move || {
                let mut reads = 0;
                while !done.load(Ordering::Relaxed) {
                    // 옮기는 순간의 접근 거부는 건너뛴다. 읽힌 것은 늘 온전한 목록이어야 한다
                    if let Ok(bytes) = fs::read(&path) {
                        reads += 1;
                        assert_eq!(parse_connections(Some(&bytes)).len(), 4, "반쯤 쓴 목록이 읽혔다: {} 바이트", bytes.len());
                    }
                }
                reads
            })
        };
        let ids = ["atlassian", "notion", "figma", "webhook-1"];
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for i in 0..40 {
                        let id = ids[n % 4];
                        let o = CheckOutcome::Ok { who: format!("w{n}-{i}") };
                        assert_eq!(record_check(&dir, id, &o, f64::from(i)), Ok(true), "{id} {n}-{i}");
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        done.store(true, Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0, "읽는 쪽이 한 번도 못 읽었다");
        let list = parse_connections(Some(&fs::read(&path).unwrap()));
        assert_eq!(list.len(), 4);
        for c in &list {
            let k = c.checked.as_ref().unwrap_or_else(|| panic!("{} 의 기록이 사라졌다", c.id));
            assert!(k.ok && k.who.starts_with('w'), "{k:?}");
            assert_eq!(k.at, 39.0, "{} 의 마지막 기록이 낡은 값으로 덮였다: {k:?}", c.id);
        }
        assert_eq!(names(&t.0), ["connections.json"], "임시 파일이 남았다");
    }

    // ── 실제 DPAPI 비밀 저장소(Windows) ──────────────
    #[cfg(windows)]
    #[test]
    fn 실제_저장소로_저장한_토큰이_확인_요청에_실린다() {
        use claude_board_autopilot::ports::Secrets;
        let fake = Fake::start(|_| json(200, r#"{"email":"kim@example.com"}"#));
        let t = Tmp::new();
        t.write(&[conn("figma", Service::Figma, &fake.base())]);
        let ctx = t.ctx(true);
        let store = DpapiSecrets::new(ctx.secrets_dir());
        // 저장 전 — 토큰 없음
        assert!(!connections_list(&ctx)[0].token_saved);
        let before = connection_check(&ctx, "figma").unwrap();
        assert_eq!(before.kind, "local");
        assert!(fake.seen().is_empty());
        store.put("figma", format!("{MARK}\n").as_bytes()).unwrap();
        let list = connections_list(&ctx);
        assert!(list[0].token_saved);
        assert!(!serde_json::to_string(&list).unwrap().contains(MARK));
        let r = connection_check(&ctx, "figma").unwrap();
        assert_eq!((r.kind.as_str(), r.who.as_str()), ("ok", "kim@example.com"));
        assert_eq!(fake.seen()[0].headers.get("x-figma-token").map(String::as_str), Some(MARK));
        assert_eq!(connection_test_text(&ctx, "figma").unwrap(), "연결됨 · kim@example.com");
        assert!(!format!("{r:?}").contains(MARK));
        // 저장 파일은 평문이 아니다
        let sealed = fs::read(ctx.secrets_dir().join("figma.dpapi")).unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains(MARK));
    }

    #[test]
    fn 확인_기록은_목록_값으로_옮겨진다() {
        let t = Tmp::new();
        let mut c = conn("notion", Service::Notion, "");
        c.checked = Some(CheckRecord { at: 5.0, ok: false, who: String::new(), note: "거부됨".into() });
        t.write(&[c]);
        let list = list_in(&t.ctx(false), &Vault::with(&[]), 0.0);
        assert_eq!(list[0].checked, Some(CheckView { at: 5.0, ok: false, who: String::new(), note: "거부됨".into() }));
        let v = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(v["tokenSaved"], false);
        assert_eq!(v["displayName"], "Notion");
        assert_eq!(v["expiryState"], "none");
        assert_eq!(list[0].format, "");
        let _ = NotifyFormat::Json;
    }
}
