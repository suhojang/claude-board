//! 바깥 연결(Jira·Confluence·Notion·Figma·알림 webhook) 규칙. 원본: `Sources/AutopilotRules/Connections.swift`(357줄, 전체 이식).
//! 연결 목록(connections.json, 비밀 없음)의 꼴, 사이트 주소 규칙(설치판은 https 만, 시험 빌드만 http://127.0.0.1), 요청 바탕 주소.
//! 토큰·webhook 주소 전체는 키체인(어댑터 `secrets`)에만 있고 이 파일의 값(`Connection`)에는 들어가지 않는다 — `Connection` 에는 비밀 칸이 없어
//! `Debug` 출력과 `connections_json` 에도 나올 수 없다(파일에 낯선 `token` 칸이 있어도 읽을 때 버린다).
//!
//! 이식된 선언(원본 순서): `Service`(APService, `ALL`·`raw`·`from_raw`·`label`·`scope_choices`) · `CheckRecord`(APCheckRecord) ·
//! `Connection`(APConnection) · `NotifyFormat`(APNotifyFormat, 아래 설명) · `CONNECTIONS_FORMAT`(apConnectionsFormat) · `connection_id_ok`(apConnectionIDOK) ·
//! `secret_service`(apSecretService) · 비공개 `clean` · `parse_connections`(apParseConnections) · `connections_json`(apConnectionsJSON) ·
//! `SiteCheck`(APSiteCheck) · 비공개 `base_components` · `site_check`(apSiteCheck) · `webhook_check`(apWebhookCheck) · `url_host`(apHost) ·
//! `NOTION_API`(apNotionAPI) · `FIGMA_API`(apFigmaAPI) · `service_base`(apServiceBase) ·
//! `Expiry`(APExpiry) · `EXPIRY_SOON_DAYS`(apExpirySoonDays) · `Day`·`ap_day`(apDay) · `ap_expiry`(apExpiry) · `expiry_label`(apExpiryLabel) · `Auth`(APAuth) ·
//! `HttpRequest`(APHTTPRequest) · `HttpResponse`(APHTTPResponse) · `AuthHeader`·`auth_header`(apAuthHeader) · `NOTION_VERSION`(apNotionVersion) ·
//! `same_site`(apSameSite) · `auth_for`(apAuthFor) · `check_request`(apCheckRequest) · `CheckOutcome`(APCheckOutcome) · 비공개 `who` ·
//! `check_outcome`(apCheckOutcome) · `check_connection`(apCheckConnection). 포트 둘: `ConnHttp`(APHTTP)·`ConnSecrets`(APSecrets).
//! 공개 보조: `UrlParts`·`parse_url`(Swift `URLComponents(string:)` 를 손으로 옮긴 것). 남은 선언 없음(원본 전체가 이식됐다).
//!
//! 포트 둘의 자리: 원본은 `Ports.swift` 지만 `ports.rs` 는 통합자 몫이라 이 파일에 둔다. 엔진의 `ports::Http`·`ports::Secrets` 는 꼴이 다르다
//! (`Vec<(String,String)>` 머리·`PortResult`·바이트 비밀) — 어댑터가 `ConnHttp`·`ConnSecrets` 로 맞춘다. 트레이트는 원본처럼 `&self` 다(가짜는 `RefCell` 로 기록한다).
//!
//! 비밀(인증 머리): 토큰은 `HttpRequest` 에 들어가지 않는다(`auth` 는 어떤 꼴의 인증인지만). 인증 머리의 값은 `auth_header` 가 보내기 직전에만 만들고
//! `AuthHeader` 의 `Debug` 가 값을 `<가림>` 으로 찍는다(원본에는 없는 가림 — 파생 `Debug` 가 토큰을 찍을 자리라서). `HttpRequest` 의 `Debug` 도
//! `Authorization`·`X-Figma-Token` 머리의 값을 가린다(요청에는 안 들어가는 값이라 방어일 뿐이다). `CheckOutcome`·`HttpResponse` 에는 토큰이 들어갈 칸이 없다.
//!
//! Swift ↔ Rust 로 달라진 점:
//! - 주소 읽기: 원본 `URLComponents(string:)`(Foundation). 여기는 RFC 3986 꼴 `scheme "://" [userinfo "@"] host [":" port] path ["?" query] ["#" fragment]` 를 손으로 읽는다.
//!   틀린 꼴은 모두 같은 글(「주소 꼴이 아닙니다」)로 가는 길이라 어느 쪽이든 겉으로 같지만, Foundation 이 받는 이상한 입력 중 아래는 여기서 받지 않는다(더 엄격).
//!   · 호스트는 `A-Za-z0-9._~-` 만(Foundation 은 `,;!$&*+=`·`%XX` 도 받는다). · IP 리터럴 `[::1]` 은 받지 않는다. · 빈 포트(`host:`)와 65535 를 넘는 포트는 받지 않는다.
//!   · 글자 틀 밖의 `[` `]` 는 어느 칸에서도 받지 않는다. · `@` 가 둘 이상인 칸은 받지 않는다. · userinfo 칸(`@` 앞)은 비어 있어도 「아이디·암호를 넣지 않습니다」로 거절한다.
//!   안 받는 입력의 목록은 Foundation 쪽에서 직접 잰 것이 아니라(이 머신에 Swift 가 없다) 글자 규칙에서 정한 것이다.
//! - `path` 는 Foundation 처럼 퍼센트 풀이를 한 뒤 견준다(`/%77iki` 는 `/wiki`). 질의·조각은 `?`·`#` 가 있으면 있는 것이다(빈 `?` 도 있음).
//! - 정규식 `^(atlassian|notion|figma|webhook-[1-9][0-9]{0,2})$`·`^[0-9]{4}-[0-9]{2}-[0-9]{2}$` 는 손으로 옮겼다. ICU 의 `$` 는 끝의 줄바꿈 하나 앞에서도 맞으므로
//!   원본은 `"figma\n"` 을 받을 수 있다고 알고 있다(직접 재지 못함). 여기는 받지 않는다(더 엄격). 키체인 계정 이름이라 엄격한 쪽이 안전하다.
//! - `clean` 의 길이 자르기는 원본이 자소 묶음(`Character`) 단위, 여기는 `char` 단위다. 결합 글자가 든 긴 글에서만 갈린다.
//!   공백 깎기는 원본 `.whitespaces`(Zs 와 탭), 주소 깎기는 `.whitespacesAndNewlines`(거기에 U+000A~000D·U+0085·U+2028·U+2029)다.
//! - `parse_connections`: 원본 `JSONSerialization` 은 여기 `serde_json`(엄격한 JSON). `checked.at` 은 원본 `as? NSNumber` 라 참/거짓도 1/0 으로 읽고,
//!   `checked.ok` 의 `as? Bool` 은 수 0·1 도 거짓·참으로 받는다(브리지 규칙을 기억에서 옮김, 직접 재지 못함). `connections` 안에 객체가 아닌 원소가 하나라도 있으면 원본 캐스팅이 통째로 실패해 빈 목록이다.
//! - `connections_json` 은 원본 `[String: Any]` 대신 `serde_json::Map`(넣은 차례 유지). `checked.at` 이 정수 값이면 정수로 쓴다(JSONSerialization 과 같은 모양). NaN·무한대는 0 으로 쓴다(원본은 던져서 저장이 실패).
//! - `ap_expiry`: 원본 `now: Date`·`tz: TimeZone` 은 `now: f64`(유닉스 초)·`tz: stopline::Tz`(Utc·Local)다. 「Asia/Seoul」 같은 임의 시간대는 표현하지 못한다
//!   (시험은 `Utc` — 시험 시각 서울 13:40 은 UTC 04:40 이라 같은 날이다). 날짜 계산은 달력 글자 계산이고 시계를 부르지 않는다.
//! - `same_site`: `parse_url` 을 쓰므로 위 「더 엄격」한 입력(공백·비 ASCII·IP 리터럴 등)은 `false` 다. 원본은 Foundation 이 받는 글자에서 갈릴 수 있다(직접 재지 못함).
//! - `who`: `JSONSerialization` 대신 `serde_json`. JSON `null` 은 원본에서 `NSNull`(있는 값)이라 `a ?? b` 가 `b` 로 넘어가지 않는다 — 여기도 `get(a).or(get(b))` 로 같다.
//! - `HttpRequest.headers`·`HttpResponse.headers` 는 `BTreeMap`(원본 `[String: String]`, 차례 없음), `HttpResponse.status` 는 `i32`(0 = 응답 없음), 본문은 `Vec<u8>`(`Data`).
//!
//! Windows 판단:
//! - 이 규칙은 경로를 다루지 않는다. 키체인 항목 이름(`secret_service`)과 연결 id 는 글자일 뿐이다. 토큰이 실제로 어디에 사는지(Windows 자격 증명 보관소, `~/.claude/…` 아래 `\` 경로)는
//!   어댑터(`adapters::secrets`)가 소유하고, 이 규칙에 들어오는 경로·주소는 `/` 로 맞춘 글자라고 본다. 주소에서 `\` 는 글자 틀에서 거절되므로
//!   Windows 경로(`C:\x`)가 주소 칸으로 들어오면 「쓸 수 없는 글자」로 걸린다.

use claude_board_core::clock;
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;

use crate::rules::stopline::Tz;

// ── 서비스 ───────────────────────────────────────────

/// 서비스 — Jira·Confluence 는 Atlassian 연결 하나를 함께 쓴다(같은 사이트·같은 API 토큰). 원본: `APService`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Atlassian,
    Notion,
    Figma,
    Webhook,
}

impl Service {
    /// 원본 `CaseIterable.allCases`(선언 차례)
    pub const ALL: [Service; 4] = [Service::Atlassian, Service::Notion, Service::Figma, Service::Webhook];

    /// 원본 `rawValue`
    pub fn raw(self) -> &'static str {
        match self {
            Service::Atlassian => "atlassian",
            Service::Notion => "notion",
            Service::Figma => "figma",
            Service::Webhook => "webhook",
        }
    }

    /// 원본 `init?(rawValue:)`
    pub fn from_raw(s: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|v| v.raw() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Service::Atlassian => "Atlassian",
            Service::Notion => "Notion",
            Service::Figma => "Figma",
            Service::Webhook => "알림 webhook",
        }
    }

    /// 설정 화면에 보일 범위(쓰임). 값은 connections.json 의 scopes 에 그대로 남는다. 원본: `scopeChoices`(`(key:, label:)`)
    pub fn scope_choices(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Service::Atlassian => &[("jira", "Jira 쓰기(티켓 만들기·댓글)"), ("confluence", "Confluence 쓰기(보고서)"), ("read", "읽기")],
            Service::Notion => &[("write", "Notion 쓰기(보고서)"), ("read", "읽기")],
            Service::Figma => &[("read", "읽기")],
            Service::Webhook => &[("notify", "알림")],
        }
    }
}

/// webhook 글 꼴. 원본: `APNotifyFormat`(`Notify.swift` — 이 청크의 `parse_connections` 가 필요해 `label` 까지 미리 옮겼다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyFormat {
    /// 일반 JSON — {text, event, run, …}
    Json,
    /// Slack 꼴 — {text}(mrkdwn, & < > 는 엔티티), 미리보기 끔
    Slack,
    /// Google Chat 꼴 — {text}
    Gchat,
}

impl NotifyFormat {
    pub const ALL: [NotifyFormat; 3] = [NotifyFormat::Json, NotifyFormat::Slack, NotifyFormat::Gchat];

    pub fn raw(self) -> &'static str {
        match self {
            NotifyFormat::Json => "json",
            NotifyFormat::Slack => "slack",
            NotifyFormat::Gchat => "gchat",
        }
    }

    pub fn from_raw(s: &str) -> Option<NotifyFormat> {
        NotifyFormat::ALL.into_iter().find(|v| v.raw() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            NotifyFormat::Json => "일반 JSON",
            NotifyFormat::Slack => "Slack 꼴",
            NotifyFormat::Gchat => "Google Chat 꼴",
        }
    }
}

/// 마지막 "연결 확인" — 보드가 남긴다(엔진 연결 확인 명령의 결과). 원본: `APCheckRecord`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CheckRecord {
    pub at: f64,
    pub ok: bool,
    /// 받는 쪽이 알려 준 계정 이름(믿지 않는 값, 자르고 다듬어 둠)
    pub who: String,
    pub note: String,
}

/// 연결 하나(비밀 없음). id 는 키체인 항목의 계정 이름이기도 하다. 원본: `APConnection`
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub id: String,
    pub service: Service,
    /// Atlassian 사이트(https://<이름>.atlassian.net). Notion·Figma 는 빈 글자 = 기본 API 주소(시험 빌드만 127.0.0.1 로 바꿈)
    pub site: String,
    /// Atlassian·Figma 계정 메일, Notion 은 요청자 이름(본문에 씀: Notion 작성자는 통합 이름이라서)
    pub account: String,
    /// webhook 이름(대화방)
    pub name: String,
    /// webhook 주소의 호스트(보이기만 함, 주소 전체는 키체인)
    pub host: String,
    pub scopes: Vec<String>,
    /// 토큰 만료일 yyyy-MM-dd(사람이 넣음, 없으면 빈 글자)
    pub expires: String,
    pub checked: Option<CheckRecord>,
    /// webhook 글 꼴 — 받는 쪽마다 정한다. 다른 서비스는 쓰지 않는다
    pub format: NotifyFormat,
}

impl Default for Connection {
    /// 원본 `APConnection()` 의 기본값(`id` 빈 글·`.atlassian`·`.json`)
    fn default() -> Self {
        Connection {
            id: String::new(),
            service: Service::Atlassian,
            site: String::new(),
            account: String::new(),
            name: String::new(),
            host: String::new(),
            scopes: Vec::new(),
            expires: String::new(),
            checked: None,
            format: NotifyFormat::Json,
        }
    }
}

pub const CONNECTIONS_FORMAT: &str = "claude-board-connections";

/// 연결 id 꼴 — 키체인 계정 이름과 파일 칸으로 쓰므로 좁게. 원본 정규식 `^(atlassian|notion|figma|webhook-[1-9][0-9]{0,2})$`
pub fn connection_id_ok(id: &str) -> bool {
    if matches!(id, "atlassian" | "notion" | "figma") {
        return true;
    }
    let Some(n) = id.strip_prefix("webhook-") else { return false };
    let b = n.as_bytes();
    (1..=3).contains(&b.len()) && b[0] != b'0' && b.iter().all(u8::is_ascii_digit)
}

/// 키체인 항목(일반 암호) — 서비스 이름은 설치판·시험 빌드가 달라 시험이 설치판 토큰을 건드리지 않는다. 계정 이름은 연결 id
pub fn secret_service(dev: bool) -> &'static str {
    if dev {
        "Claude Board Dev-connection"
    } else {
        "Claude Board-connection"
    }
}

/// Swift `CharacterSet.whitespaces` — 일반 범주 Zs 와 탭
fn is_blank(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// Swift `CharacterSet.whitespacesAndNewlines`
fn is_blank_or_newline(c: char) -> bool {
    is_blank(c) || matches!(c, '\u{A}'..='\u{D}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// 믿지 않는 글 한 칸 — 글이 아니면 빈 글, 제어·방향 글자를 지우고 앞뒤 빈칸을 깎아 n 글자로 자른다
fn clean(v: Option<&Value>, n: usize) -> String {
    let Some(Value::String(s)) = v else { return String::new() };
    let o: String = s
        .chars()
        .filter(|&c| {
            let x = c as u32;
            !(x < 0x20
                || (0x7F..=0x9F).contains(&x)
                || (0x202A..=0x202E).contains(&x)
                || (0x2066..=0x2069).contains(&x)
                || x == 0x2028
                || x == 0x2029)
        })
        .collect();
    let t = o.trim_matches(is_blank);
    if t.chars().count() > n {
        t.chars().take(n).collect()
    } else {
        t.to_string()
    }
}

/// 원본 `as? NSNumber` 의 `doubleValue`(JSONSerialization 은 참/거짓도 NSNumber)
fn ns_double(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// 원본 `as? Bool` — 수 0·1 도 받는다(NSNumber 브리지)
fn ns_bool(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        _ => false,
    }
}

/// connections.json → 연결 목록. 꼴이 틀린 줄과 같은 id 의 두 번째 줄은 버린다. 원본: `apParseConnections`
pub fn parse_connections(data: Option<&[u8]>) -> Vec<Connection> {
    let Some(data) = data else { return Vec::new() };
    if data.len() > 256 << 10 {
        return Vec::new();
    }
    let Ok(Value::Object(o)) = serde_json::from_slice::<Value>(data) else { return Vec::new() };
    if o.get("format").and_then(Value::as_str) != Some(CONNECTIONS_FORMAT) {
        return Vec::new();
    }
    let Some(Value::Array(list)) = o.get("connections") else { return Vec::new() };
    // `as? [[String: Any]]` — 객체가 아닌 원소가 하나라도 있으면 통째로 실패
    let Some(list) = list.iter().map(Value::as_object).collect::<Option<Vec<_>>>() else { return Vec::new() };
    let mut out: Vec<Connection> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for e in list.into_iter().take(50) {
        let id = e.get("id").and_then(Value::as_str).unwrap_or("");
        if !connection_id_ok(id) {
            continue;
        }
        let Some(s) = Service::from_raw(e.get("service").and_then(Value::as_str).unwrap_or("")) else { continue };
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        if (s == Service::Webhook) != id.starts_with("webhook-") || !(s == Service::Webhook || id == s.raw()) {
            continue;
        }
        let mut c = Connection { id: id.to_string(), service: s, ..Connection::default() };
        c.site = clean(e.get("site"), 300);
        c.account = clean(e.get("account"), 200);
        c.name = clean(e.get("name"), 80);
        c.host = clean(e.get("host"), 200);
        let allowed = s.scope_choices();
        c.scopes = match e.get("scopes") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(Value::as_str)
                .filter(|k| allowed.iter().any(|(key, _)| key == k))
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        let ex = clean(e.get("expires"), 10);
        c.expires = if ap_day(&ex).is_some() { ex } else { String::new() };
        if let Some(Value::Object(k)) = e.get("checked") {
            c.checked = Some(CheckRecord {
                at: ns_double(k.get("at")),
                ok: ns_bool(k.get("ok")),
                who: clean(k.get("who"), 120),
                note: clean(k.get("note"), 300),
            });
        }
        if s == Service::Webhook {
            c.format = NotifyFormat::from_raw(e.get("format").and_then(Value::as_str).unwrap_or("")).unwrap_or(NotifyFormat::Json);
        }
        out.push(c);
    }
    out
}

/// 연결 목록 → connections.json 값. 비밀 칸은 만들지 않는다. 원본: `apConnectionsJSON`
pub fn connections_json(list: &[Connection]) -> Map<String, Value> {
    let rows: Vec<Value> = list
        .iter()
        .map(|c| {
            let mut d = Map::new();
            d.insert("id".into(), Value::from(c.id.as_str()));
            d.insert("service".into(), Value::from(c.service.raw()));
            d.insert("scopes".into(), Value::from(c.scopes.clone()));
            for (key, v) in [("site", &c.site), ("account", &c.account), ("name", &c.name), ("host", &c.host), ("expires", &c.expires)] {
                if !v.is_empty() {
                    d.insert(key.into(), Value::from(v.as_str()));
                }
            }
            if let Some(k) = &c.checked {
                let mut m = Map::new();
                m.insert("at".into(), json_number(k.at));
                m.insert("ok".into(), Value::from(k.ok));
                m.insert("who".into(), Value::from(k.who.as_str()));
                m.insert("note".into(), Value::from(k.note.as_str()));
                d.insert("checked".into(), Value::Object(m));
            }
            if c.service == Service::Webhook {
                d.insert("format".into(), Value::from(c.format.raw()));
            }
            Value::Object(d)
        })
        .collect();
    let mut o = Map::new();
    o.insert("format".into(), Value::from(CONNECTIONS_FORMAT));
    o.insert("version".into(), Value::from(1));
    o.insert("connections".into(), Value::Array(rows));
    o
}

/// 정수 값이면 정수로(JSONSerialization 과 같은 모양), NaN·무한대는 0
fn json_number(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e15 {
        Value::from(x as i64)
    } else {
        Number::from_f64(x).map_or(Value::from(0), Value::Number)
    }
}

// ── 사이트 주소 규칙 ─────────────────────────────────

/// 주소 확인 결과. 원본: `APSiteCheck`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteCheck {
    /// 다듬은 주소
    Ok(String),
    /// 받지 않는 까닭
    Bad(String),
}

/// 주소를 칸으로 나눈 것. 원본 `URLComponents` 에서 규칙이 쓰는 칸만. `path` 는 퍼센트를 푼 글자, `host` 는 소문자가 아니라 받은 그대로(비교하는 쪽이 소문자로 한다 — 원본과 같다)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlParts {
    pub scheme: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<u32>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn is_sub_delim(b: u8) -> bool {
    matches!(b, b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=')
}

/// `extra` 글자와 unreserved·sub-delims·`%XX` 만으로 된 칸인가
fn comp_ok(s: &str, extra: &[u8]) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' {
            if i + 2 >= b.len() || !b[i + 1].is_ascii_hexdigit() || !b[i + 2].is_ascii_hexdigit() {
                return false;
            }
            i += 3;
            continue;
        }
        if !(is_unreserved(c) || is_sub_delim(c) || extra.contains(&c)) {
            return false;
        }
        i += 1;
    }
    true
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = |x: u8| (x as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (h(b[i + 1]), h(b[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 원본 `URLComponents(string:)` 의 손으로 옮긴 판. 꼴이 틀리면 None(위 머리 설명의 차이를 본다). 스킴 없는 글(`example.com`)은 칸이 path 뿐인 값이 아니라 None 이다 —
/// 규칙은 어차피 스킴·호스트가 없으면 「꼴이 아니다」로 보내므로 겉으로 같다
pub fn parse_url(s: &str) -> Option<UrlParts> {
    if !s.is_ascii() {
        return None;
    }
    let (scheme, rest) = s.split_once(':')?;
    let sb = scheme.as_bytes();
    if sb.is_empty() || !sb[0].is_ascii_alphabetic() || !sb.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.')) {
        return None;
    }
    let rest = rest.strip_prefix("//")?;
    // 조각 → 질의 → 칸(authority)·경로 순으로 뗀다
    let (rest, fragment) = match rest.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((a, q)) => (a, Some(q)),
        None => (rest, None),
    };
    let (auth, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    if !comp_ok(path, b":@/") || !query.is_none_or(|q| comp_ok(q, b":@/?")) || !fragment.is_none_or(|f| comp_ok(f, b":@/?")) {
        return None;
    }
    if auth.matches('@').count() > 1 {
        return None;
    }
    let (userinfo, hostport) = match auth.split_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, auth),
    };
    let (user, password) = match userinfo {
        None => (None, None),
        Some(u) => {
            if !comp_ok(u, b":") {
                return None;
            }
            match u.split_once(':') {
                Some((a, p)) => (Some(pct_decode(a)), Some(pct_decode(p))),
                None => (Some(pct_decode(u)), None),
            }
        }
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => {
            if p.is_empty() || p.len() > 5 || !p.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let n: u32 = p.parse().ok()?;
            if n > 65535 {
                return None;
            }
            (h, Some(n))
        }
        None => (hostport, None),
    };
    if !host.bytes().all(is_unreserved) {
        return None;
    }
    Some(UrlParts {
        scheme: Some(scheme.to_string()),
        user,
        password,
        host: Some(host.to_string()),
        port,
        path: pct_decode(path),
        query: query.map(str::to_string),
        fragment: fragment.map(str::to_string),
    })
}

/// 주소 글자의 공통 확인 — 받으면 칸 나눈 값. 설치판은 https 만, 시험 빌드(dev)만 http://127.0.0.1(포트 아무것). localhost·다른 http 는 늘 거절. 원본: `baseComponents`
fn base_components(raw: &str, dev: bool) -> Result<UrlParts, String> {
    let t = raw.trim_matches(is_blank_or_newline);
    if t.is_empty() {
        return Err("주소가 비었습니다".to_string());
    }
    let bad_char = |c: char| !(c > ' ' && c < '\u{7F}') || "\"'<>\\`{}|^".contains(c);
    if t.chars().count() > 2000 || t.chars().any(bad_char) {
        return Err("주소에 쓸 수 없는 글자가 있습니다".to_string());
    }
    let shape = || "주소 꼴이 아닙니다(https://… 로 씁니다)".to_string();
    let c = parse_url(t).ok_or_else(shape)?;
    let scheme = c.scheme.as_deref().ok_or_else(shape)?.to_lowercase();
    let host = c.host.as_deref().ok_or_else(shape)?.to_lowercase();
    if host.is_empty() {
        return Err(shape());
    }
    if c.user.is_some() || c.password.is_some() {
        return Err("주소에 아이디·암호를 넣지 않습니다".to_string());
    }
    if scheme == "https" {
        return Ok(c);
    }
    if scheme == "http" {
        if dev && host == "127.0.0.1" {
            return Ok(c);
        }
        return Err(if dev { "http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다" } else { "https 주소만 받습니다" }.to_string());
    }
    Err("https 주소만 받습니다".to_string())
}

/// Atlassian 사이트(그리고 시험 빌드에서 Notion·Figma 를 가짜 서버로 바꿀 때) — 호스트(와 포트)까지만, 경로·물음·조각은 받지 않는다. 결과는 scheme://host[:port]. 원본: `apSiteCheck`
pub fn site_check(raw: &str, dev: bool) -> SiteCheck {
    let c = match base_components(raw, dev) {
        Ok(c) => c,
        Err(why) => return SiteCheck::Bad(why),
    };
    if !(c.path.is_empty() || c.path == "/" || c.path == "/wiki" || c.path == "/wiki/") {
        return SiteCheck::Bad("사이트 주소는 호스트까지만 씁니다(경로 없이)".to_string());
    }
    if c.query.is_some() || c.fragment.is_some() {
        return SiteCheck::Bad("사이트 주소에 ? 나 # 을 넣지 않습니다".to_string());
    }
    let mut out = format!("{}://{}", c.scheme.as_deref().unwrap_or("").to_lowercase(), c.host.as_deref().unwrap_or("").to_lowercase());
    if let Some(p) = c.port {
        out.push_str(&format!(":{p}"));
    }
    SiteCheck::Ok(out)
}

/// 알림 webhook 주소 — 경로·물음은 받는다(대화방 주소에 열쇠가 들어 있어 주소 전체가 비밀). 조각(#)은 받지 않는다. 원본: `apWebhookCheck`
pub fn webhook_check(raw: &str, dev: bool) -> SiteCheck {
    let c = match base_components(raw, dev) {
        Ok(c) => c,
        Err(why) => return SiteCheck::Bad(why),
    };
    if c.fragment.is_some() {
        return SiteCheck::Bad("webhook 주소에 # 을 넣지 않습니다".to_string());
    }
    if c.path.is_empty() || c.path == "/" {
        return SiteCheck::Bad("webhook 주소에 경로가 없습니다".to_string());
    }
    SiteCheck::Ok(raw.trim_matches(is_blank_or_newline).to_string())
}

/// 주소의 호스트(보이기만). 읽지 못하면 빈 글자. 원본: `apHost`
pub fn url_host(url: &str) -> String {
    parse_url(url).and_then(|c| c.host).map(|h| h.to_lowercase()).unwrap_or_default()
}

pub const NOTION_API: &str = "https://api.notion.com";
pub const FIGMA_API: &str = "https://api.figma.com";

/// 요청 바탕 주소 — Atlassian 은 사이트(규칙을 다시 봄), Notion·Figma 는 정한 API 주소이고 시험 빌드만 사이트 칸의 http://127.0.0.1 로 바꾼다.
/// webhook 은 주소가 비밀이라 None. 규칙에 맞지 않으면 None(요청하지 않음). 원본: `apServiceBase`
pub fn service_base(c: &Connection, dev: bool) -> Option<String> {
    match c.service {
        Service::Atlassian => match site_check(&c.site, dev) {
            SiteCheck::Ok(s) => Some(s),
            SiteCheck::Bad(_) => None,
        },
        Service::Notion | Service::Figma => {
            let fixed = if c.service == Service::Notion { NOTION_API } else { FIGMA_API };
            if !dev || c.site.is_empty() {
                return Some(fixed.to_string());
            }
            match site_check(&c.site, true) {
                SiteCheck::Ok(s) if s.starts_with("http://127.0.0.1") => Some(s),
                _ => Some(fixed.to_string()),
            }
        }
        Service::Webhook => None,
    }
}

// ── 만료일 ───────────────────────────────────────────

/// 만료일 판정. 원본: `APExpiry`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    /// 만료일을 넣지 않음
    None,
    /// 남은 날
    Ok(i64),
    /// `EXPIRY_SOON_DAYS` 안(0 이면 오늘이 만료일)
    Soon(i64),
    /// 지난 날
    Expired(i64),
}

/// 원본 `apExpirySoonDays`
pub const EXPIRY_SOON_DAYS: i64 = 14;

/// 그레고리력 날짜(원본 `DateComponents` 의 년·월·일)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// yyyy-MM-dd → 그 날(그레고리력, 2000~2200년). 꼴이 틀리거나 없는 날(2월 30일)이면 None. 원본: `apDay`
pub fn ap_day(s: &str) -> Option<Day> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s[r].parse::<u32>().unwrap_or(0);
    let (y, m, d) = (num(0..4), num(5..7), num(8..10));
    if !(2000..=2200).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let in_month = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (d <= in_month).then_some(Day { year: y as i32, month: m, day: d })
}

/// 1970-01-01 부터 센 날 수(Howard Hinnant 의 days_from_civil)
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 그 시각의 시간대 기준 오늘(년·월·일)
fn civil_today(now: f64, tz: Tz) -> (i64, i64, i64) {
    match tz {
        Tz::Local => {
            let l = clock::local(now);
            (i64::from(l.year), i64::from(l.month), i64::from(l.day))
        }
        Tz::Utc => {
            let z = (now.floor() as i64).div_euclid(86_400) + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z.rem_euclid(146_097);
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let month = if mp < 10 { mp + 3 } else { mp - 9 };
            (yoe + era * 400 + i64::from(month <= 2), month, doy - (153 * mp + 2) / 5 + 1)
        }
    }
}

/// 만료일까지 남은 날(그 사람의 시간대로 오늘을 센다). 만료일 당일은 `Soon(0)`. 원본: `apExpiry`
pub fn ap_expiry(expires: &str, now: f64, tz: Tz) -> Expiry {
    let Some(e) = ap_day(expires) else { return Expiry::None };
    let (ty, tm, td) = civil_today(now, tz);
    let days = days_from_civil(i64::from(e.year), i64::from(e.month), i64::from(e.day)) - days_from_civil(ty, tm, td);
    if days < 0 {
        return Expiry::Expired(-days);
    }
    if days <= EXPIRY_SOON_DAYS {
        return Expiry::Soon(days);
    }
    Expiry::Ok(days)
}

/// 원본: `apExpiryLabel`
pub fn expiry_label(e: Expiry, expires: &str) -> String {
    match e {
        Expiry::None => "만료일 없음".to_string(),
        Expiry::Ok(d) => format!("만료 {expires} · {d}일 남음"),
        Expiry::Soon(0) => format!("오늘 만료({expires})"),
        Expiry::Soon(d) => format!("만료 임박 · {d}일 남음({expires})"),
        Expiry::Expired(d) => format!("만료됨 · {d}일 지남({expires})"),
    }
}

// ── 요청 값과 인증 머리 ──────────────────────────────

/// 보낼 요청의 인증 꼴 — 토큰은 들어가지 않는다(어떤 꼴의 인증인지만). 어댑터는 리다이렉트를 따르지 않는다. 원본: `APAuth`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    /// Atlassian: 메일:토큰
    Basic { user: String },
    /// Notion: Bearer 토큰
    Bearer,
    /// Figma: X-Figma-Token
    Figma,
}

/// 보낼 요청. 토큰은 들어가지 않는다. 원본: `APHTTPRequest`
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub auth: Auth,
}

impl Default for HttpRequest {
    /// 원본 `APHTTPRequest()` 의 기본값(`GET`·빈 주소·인증 없음)
    fn default() -> Self {
        HttpRequest { method: "GET".to_string(), url: String::new(), headers: BTreeMap::new(), body: None, auth: Auth::None }
    }
}

const REDACTED: &str = "<가림>";

impl std::fmt::Debug for HttpRequest {
    /// 인증 머리 이름의 값은 가린다(요청에는 들어가지 않는 값이라 방어일 뿐이다)
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: BTreeMap<&str, &str> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let secret = k.eq_ignore_ascii_case("authorization") || k.eq_ignore_ascii_case("x-figma-token");
                (k.as_str(), if secret { REDACTED } else { v.as_str() })
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body", &self.body.as_ref().map(Vec::len))
            .field("auth", &self.auth)
            .finish()
    }
}

/// 받은 응답. 원본: `APHTTPResponse`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    /// 0 = 응답 없음(`error` 에 까닭)
    pub status: i32,
    /// 이름은 소문자
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub error: Option<String>,
}

/// 보내기 직전에 만든 인증 머리(이름·값). 값이 비밀이라 `Debug` 가 가린다. 원본은 `(name:, value:)` 튜플
#[derive(Clone, PartialEq, Eq)]
pub struct AuthHeader {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for AuthHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthHeader").field("name", &self.name).field("value", &REDACTED).finish()
    }
}

/// 표준 base64(패딩 있음) — `Data.base64EncodedString()` 과 같다
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 인증 머리 — 보내기 직전에만 만든다(값을 남기지 않는다). 원본: `apAuthHeader`
pub fn auth_header(a: &Auth, token: &str) -> Option<AuthHeader> {
    let (name, value) = match a {
        Auth::None => return None,
        Auth::Basic { user } => ("Authorization", format!("Basic {}", base64(format!("{user}:{token}").as_bytes()))),
        Auth::Bearer => ("Authorization", format!("Bearer {token}")),
        Auth::Figma => ("X-Figma-Token", token.to_string()),
    };
    Some(AuthHeader { name: name.to_string(), value })
}

/// 원본: `apNotionVersion`
pub const NOTION_VERSION: &str = "2022-06-28";

/// 요청의 주소가 그 연결의 바탕 주소 아래인가 — 아니면 보내지 않는다(받는 쪽 응답이나 글에 든 주소로 토큰을 보내지 않게). 원본: `apSameSite`
pub fn same_site(url: &str, base: &str) -> bool {
    let (Some(u), Some(b)) = (parse_url(url), parse_url(base)) else { return false };
    let lower = |s: &Option<String>| s.as_deref().map(str::to_lowercase);
    lower(&u.scheme) == lower(&b.scheme) && lower(&u.host) == lower(&b.host) && u.port == b.port && u.user.is_none() && u.password.is_none()
}

/// 서비스마다 인증 꼴. 원본: `apAuthFor`
pub fn auth_for(c: &Connection) -> Auth {
    match c.service {
        Service::Atlassian => Auth::Basic { user: c.account.clone() },
        Service::Notion => Auth::Bearer,
        Service::Figma => Auth::Figma,
        Service::Webhook => Auth::None,
    }
}

fn header_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// 「연결 확인」 읽기 요청 하나(설계 5장 6절). webhook 은 읽는 길이 없어 None(주소 꼴만 본다). 원본: `apCheckRequest`
pub fn check_request(c: &Connection, dev: bool) -> Option<HttpRequest> {
    let base = service_base(c, dev)?;
    match c.service {
        Service::Atlassian => Some(HttpRequest {
            url: base + "/rest/api/3/myself",
            headers: header_map(&[("Accept", "application/json")]),
            auth: auth_for(c),
            ..HttpRequest::default()
        }),
        Service::Notion => Some(HttpRequest {
            url: base + "/v1/users/me",
            headers: header_map(&[("Accept", "application/json"), ("Notion-Version", NOTION_VERSION)]),
            auth: Auth::Bearer,
            ..HttpRequest::default()
        }),
        Service::Figma => Some(HttpRequest {
            url: base + "/v1/me",
            headers: header_map(&[("Accept", "application/json")]),
            auth: Auth::Figma,
            ..HttpRequest::default()
        }),
        Service::Webhook => None,
    }
}

/// 연결 확인 결과. 원본: `APCheckOutcome`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// 읽기 요청이 됨(받는 쪽이 알려 준 이름)
    Ok { who: String },
    /// 401·403 — 토큰이 틀렸거나 만료·취소됨
    Rejected(String),
    /// 그 밖(응답 없음·다른 곳으로 보냄·꼴이 틀린 응답)
    Failed(String),
    /// 요청 없이 끝남(토큰 없음·주소 규칙·webhook)
    Local(String),
}

/// 응답 본문(믿지 않는 값)에서 이름 한 칸 — JSON 객체의 정한 칸만, 자르고 다듬는다
fn who(c: &Connection, body: &[u8]) -> String {
    if body.len() > 1 << 20 {
        return String::new();
    }
    let Ok(Value::Object(o)) = serde_json::from_slice::<Value>(body) else { return String::new() };
    match c.service {
        Service::Atlassian => clean(o.get("displayName").or(o.get("emailAddress")), 120),
        Service::Notion => {
            let owner = o.get("bot").and_then(|b| b.get("owner")).and_then(|w| w.get("user"));
            let n = clean(o.get("name"), 120);
            let u = clean(owner.and_then(|w| w.get("name")), 120);
            if u.is_empty() {
                n
            } else if n.is_empty() {
                u
            } else {
                format!("{n} · {u}")
            }
        }
        Service::Figma => clean(o.get("email").or(o.get("handle")), 120),
        Service::Webhook => String::new(),
    }
}

/// 응답 → 결과. 원본: `apCheckOutcome`
pub fn check_outcome(c: &Connection, r: &HttpResponse) -> CheckOutcome {
    if let Some(e) = &r.error {
        return CheckOutcome::Failed(e.clone());
    }
    match r.status {
        200 => {
            let w = who(c, &r.body);
            if w.is_empty() {
                CheckOutcome::Failed("응답 꼴이 예상과 다릅니다".to_string())
            } else {
                CheckOutcome::Ok { who: w }
            }
        }
        401 | 403 => CheckOutcome::Rejected(format!("거부됨({}): 토큰이 틀렸거나 만료·취소됐습니다", r.status)),
        429 => CheckOutcome::Failed("요청이 많다며 거절했습니다(429). 잠시 뒤 다시 확인합니다".to_string()),
        300..=399 => CheckOutcome::Failed(format!("다른 주소로 보내려 해 따라가지 않았습니다({})", r.status)),
        s => CheckOutcome::Failed(format!("받는 쪽이 {s} 로 답했습니다")),
    }
}

/// HTTP 요청 하나 — 어댑터는 리다이렉트를 따르지 않고(3xx 는 그대로 돌려줌), 쿠키·캐시를 쓰지 않고, 응답 본문에 크기 상한을 둔다.
/// `auth` 는 보내기 직전에 만든 인증 머리이고 어댑터는 이것을 남기지 않는다(기록·파일·환경 변수 어디에도). 원본: 포트 `APHTTP`
pub trait ConnHttp {
    fn send(&self, request: &HttpRequest, auth: Option<&AuthHeader>) -> HttpResponse;
}

/// 키체인의 연결 비밀(API 토큰·webhook 주소) — 항목 이름은 `secret_service(dev)`, 계정 이름은 연결 id. 읽기만. 원본: 포트 `APSecrets`
pub trait ConnSecrets {
    fn read(&self, id: &str) -> Option<String>;
    fn exists(&self, id: &str) -> bool;
}

/// 연결 확인 유스케이스 — 토큰이 없거나 주소가 규칙 밖이면 요청 0. 원본: `apCheckConnection`
pub fn check_connection(c: &Connection, dev: bool, secrets: &dyn ConnSecrets, http: &dyn ConnHttp) -> CheckOutcome {
    if c.service == Service::Webhook {
        if !secrets.exists(&c.id) {
            return CheckOutcome::Local("webhook 주소가 키체인에 없습니다".to_string());
        }
        return CheckOutcome::Local("webhook 은 읽는 요청이 없어 주소 꼴만 봤습니다. 대화방에 닿는지는 '시험 글 보내기'로 봅니다".to_string());
    }
    let bad_site = || CheckOutcome::Local("사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다".to_string());
    let (Some(req), Some(base)) = (check_request(c, dev), service_base(c, dev)) else { return bad_site() };
    if !same_site(&req.url, &base) {
        return bad_site();
    }
    if let Auth::Basic { user } = &req.auth {
        if user.is_empty() {
            return CheckOutcome::Local("계정 메일이 없어 요청하지 않았습니다".to_string());
        }
    }
    let Some(token) = secrets.read(&c.id).filter(|t| !t.is_empty()) else {
        return CheckOutcome::Local("토큰이 키체인에 없어 요청하지 않았습니다".to_string());
    };
    let header = auth_header(&req.auth, &token);
    check_outcome(c, &http.send(&req, header.as_ref()))
}
