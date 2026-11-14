//! 자율주행 읽기(4단계 ⑩-2): 링크 → 읽을 대상, 읽기 요청 꼴(GET 만), 그림 주소 허용, 응답 → 메타, 읽기 유스케이스와 에픽 칸. 원본: `Sources/AutopilotRules/Reading.swift` 1~650줄.
//!
//! Swift ↔ Rust 로 달라진 점:
//! - `URLComponents(string:)` 는 `connections.rs` 의 `parse_url`(RFC 3986 손으로 옮긴 판)을 확장한 비공개 `parse_link_url` 로 읽는다. 차이:
//!   Foundation 은 경로·질의·조각·사용자 정보의 비 ASCII 글자(…/회의실-예약-0123…, ?p=…)를 받고 ReadingTests 의 견본 줄이 그렇다. 여기도 받는다. 호스트는 ASCII 만
//!   (유니코드 호스트는 IDNA 변환을 옮기지 않았다 — 그런 링크는 꼴이 틀림으로 간다), `[::1]` IP 표기·빈 포트·65535 넘는 포트는 connections.rs 와 같이 받지 않는다.
//! - 정규식 다섯(`^[A-Z][A-Z0-9_]*-[0-9]{1,9}$`(Outbox 의 apIssueKeyPattern — `tickets::issue_key_ok` 을 쓴다)·`^[0-9]{1,20}$`·
//!   `^[A-Za-z0-9]{10,64}$`·`^[0-9]{1,9}:[0-9]{1,9}$`·`^[A-Za-z0-9_-]{1,64}$`(MCPRead 의 apMCPServerNameOK — 아직 이식 전이라 비공개 `mcp_server_name_ok`)) 는
//!   손으로 옮긴 판별자다. ICU 의 `$` 는 끝의 줄바꿈 하나 앞에서도 맞으므로 원본은 `"TODO-7\n"` 도 받을 수 있다(직접 재지 못함). 여기는 받지 않는다(더 엄격).
//! - `now: Date`·`tz: TimeZone` 은 `now: f64`(유닉스 초)·`tz: stopline::Tz` 로 바꿨다(connections.rs 의 `ap_expiry` 와 같은 바꿈).
//! - `secrets: Set<String>` 은 `&HashSet<String>`(carry.rs 의 같은 바꿈).
//! - 빈칸 깎기는 `.whitespacesAndNewlines` 대신 Rust `char::is_whitespace`(유니코드 White_Space — 같은 목록)를 썼다.
//! - `addingPercentEncoding` 의 16진수는 원본처럼 대문자 두 자리다. `fields` 의 기본 인자는 부르는 쪽이 `JIRA_READ_FIELDS` 를 넣는다.
//! - 요청의 `auth` 는 꼴만 담아 비밀값이 남지 않는다. 비밀 가리기(`apRedact` 18 규칙)는 meta.rs 로 옮겼다(`redact`·`meta_redact` 공개, `word_boundary`·`ci_starts` 는 그 규칙 표와 이 파일의 태그 스캔이 나눠 쓴다) —
//!   `NSRegularExpression` 를 손으로 찾는 판으로 바꿨고(돌려 쓰는 탐욕·되감기는 원본 순서 대로), goldens 가 그 결과를 글자 그대로 묶는다.
//! - `NSRegularExpression` 여섯(저장 형식 링·CDATA·태그 떼기와 닫는 꼬리 `</script\s*>` 따위)은 손으로 옮긴 찾기로 갔다. ICU 의 `.` 가 막는 줄은
//!   LF·CR·U+0085(LNL·NEL)·U+2028(LS)·U+2029(PS) 다(Java 와 같이 0085 도 넣었다 — 실제 응답에 드물어 차이가 실리지 않는다). `\b` 는 왼쪽과 오른쪽의
//!   낱말 글자 여부가 다른 자리로, 낱말 글자는 ASCII 영숫자·_ 에 유니코드 글자·숫자까지(ICU `\w`). `\s` 는 `char::is_whitespace` 로 본다(ICU `\s` 는
//!   U+0085 를 빼고 나머지가 같다). `[^^]*` 탐욕 — 같은 이름의 속성이 태그 안에 둘 이상이면 마지막 것을 잡는다(정규식 되감기와 같다).
//! - `sv`(Swift 에서 `"\(NSNumber)"` 꼴)는 정수는 그대로, 실수는 정수 값이면 정수 꼴로 넣는다. 그 밖의 실수는 Rust `f64` Display — 10²¹ 넘는 자리의
//!   지수 표기가 Swift `e+21` 대신 `e21` 로 나오나 실제 응답 값에서 그런 자리를 못 만난다(차이 기록: `META_SWIFT_RUST.md`).
//! - Meta.swift 393~470 의 도우미(`apStripMarkup`·`apMarkupChunks`·`apChunked`·`apSectionWanted`)는 이 청크가 대기자로 같이 옮겼다가, meta.rs 를 맺는 R-Meta-d2 청크에서 옮겨 갔다 —
//!   `strip_markup`·`markup_chunks`·`chunked`·`section_wanted` 와 도우미(`swift_ws` 등)는 `use crate::rules::meta::{…}` 로 가져다 쓴다.
//! - Notion 머리 수준은 원본 `Int(t.suffix(1))` 대신 마지막 글자의 숫자로 본다(뒤 ① 이모지는 실제 블록 이름에 없다).

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::rules::connections::{
    ap_expiry, auth_for, auth_header, same_site, service_base, url_host, Auth, AuthHeader, Connection, ConnHttp, ConnSecrets, Expiry,
    HttpRequest, HttpResponse, Service, UrlParts, NOTION_VERSION,
};
use crate::rules::formats::grapheme_len;
use crate::rules::links::{link_info, ApLink};
use crate::rules::meta::{a_tail, attr_matches, chunked, decode_entities, limit, markup_chunks, meta_check, meta_clamp, meta_data, meta_redact, meta_text, quote_tail, strip_markup, swift_ws, Frame, Meta, MetaField, MetaLink, TextChunk, TocItem};
use crate::rules::stopline::Tz;
use crate::rules::tickets::issue_key_ok;

// ── 링크 → 읽을 대상 ─────────────────────────────────

/// 링크 하나를 무엇으로 읽을지. 원본: `APReadTarget`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadTarget {
    Jira { key: String },
    Confluence { page: String },
    /// 8-4-4-4-12 꼴
    Notion { page: String },
    /// node 는 API 꼴(12:4)
    Figma { file: String, node: Option<String> },
    /// 내장하지 않음 — MCP 읽기 세션으로
    Web,
    /// 주소 꼴이 틀림
    Bad(String),
}

impl ReadTarget {
    /// 읽을 서비스. Web·Bad 는 없다. 원본: `APReadTarget.service`
    pub fn service(&self) -> Option<Service> {
        match self {
            ReadTarget::Jira { .. } | ReadTarget::Confluence { .. } => Some(Service::Atlassian),
            ReadTarget::Notion { .. } => Some(Service::Notion),
            ReadTarget::Figma { .. } => Some(Service::Figma),
            ReadTarget::Web | ReadTarget::Bad(_) => None,
        }
    }

    /// 사람이 보는 종류 이름. 원본: `APReadTarget.label`
    pub fn label(&self) -> &'static str {
        match self {
            ReadTarget::Jira { .. } => "Jira",
            ReadTarget::Confluence { .. } => "Confluence",
            ReadTarget::Notion { .. } => "Notion",
            ReadTarget::Figma { .. } => "Figma",
            ReadTarget::Web | ReadTarget::Bad(_) => "웹",
        }
    }

    /// 근거 묶음의 자료 번호 앞 글자. Web·Bad 는 `web`. 원본: `APReadTarget.sourceKey`
    pub fn source_key(&self) -> &'static str {
        match self {
            ReadTarget::Jira { .. } => "jira",
            ReadTarget::Confluence { .. } => "confluence",
            ReadTarget::Notion { .. } => "notion",
            ReadTarget::Figma { .. } => "figma",
            ReadTarget::Web | ReadTarget::Bad(_) => "web",
        }
    }
}

/// Notion 주소 끝(또는 ?p=)의 32자리 16진수 → 8-4-4-4-12. 원본: `apNotionID`
pub fn notion_id(s: &str) -> Option<String> {
    let hex: String = s.to_lowercase().chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() != 32 {
        return None;
    }
    let a = hex.as_bytes();
    let f = |r: std::ops::Range<usize>| std::str::from_utf8(&a[r]).unwrap_or_default();
    Some([f(0..8), f(8..12), f(12..16), f(16..20), f(20..32)].join("-"))
}

// ── URLComponents(string:) 의 손으로 옮긴 판 ─────────

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn is_sub_delim(b: u8) -> bool {
    matches!(b, b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=')
}

/// `allowed` 글자·비 ASCII 글자·`%XX` 만으로 된 칸인가(Foundation 은 비 ASCII 경로·질의를 받는다)
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
        if !(c >= 0x80 || is_unreserved(c) || is_sub_delim(c) || extra.contains(&c)) {
            return false;
        }
        i += 1;
    }
    true
}

/// 퍼센트 부호를 푼다(말은 UTF-8 이라 되살아난 바이트만 U+FFFD 로 바뀐다)
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

/// 원본 `URLComponents(string:)` — `connections::parse_url` 의 확장판(머리 설명). 꼴이 틀리면 None. 스킴 없는 글도 None 이다
fn parse_link_url(s: &str) -> Option<UrlParts> {
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
    // 호스트는 ASCII 만(유니코드 호스트의 IDNA 는 옮기지 않았다)
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

/// 원본 `URLComponents.queryItems` — 질의 (이름, 값) 목록(퍼센트 풀이, `+` 는 빈칸으로 바꾸지 않는다). 이름만 있고 값이 없는 칸도 남긴다
fn query_items(q: &str) -> Vec<(String, Option<String>)> {
    q.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((n, v)) => (pct_decode(n), Some(pct_decode(v))),
            None => (pct_decode(pair), None),
        })
        .collect()
}

/// 질의 항목 중 그 이름의 첫 값. 값이 없는 칸(None)은 건너뛴 것이 아니라 None 이다(원본의 `?.value` 와 같다)
fn query_first<'a>(items: &'a [(String, Option<String>)], name: &str) -> Option<&'a str> {
    items.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
}

const NOT_URL: &str = "웹 주소 꼴이 아닙니다(https:// 로 시작하는 주소를 붙여 넣습니다)";
const WITH_PASSWORD: &str = "사용자 이름·비밀번호가 든 주소는 받지 않습니다";

/// `^[0-9]{1,20}$` — Confluence 페이지 번호
fn page_num_ok(s: &str) -> bool {
    (1..=20).contains(&s.chars().count()) && s.chars().all(|c| c.is_ascii_digit())
}

/// `^[A-Za-z0-9]{10,64}$` — Figma 파일 키
fn figma_key_ok(s: &str) -> bool {
    (10..=64).contains(&s.chars().count()) && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `^[0-9]{1,9}:[0-9]{1,9}$` — node-id 를 API 꼴(12:4)로 바꾼 것
fn node_api_ok(s: &str) -> bool {
    match s.split_once(':') {
        Some((a, b)) => {
            (1..=9).contains(&a.chars().count())
                && (1..=9).contains(&b.chars().count())
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// `^[A-Za-z0-9_-]{1,64}$` — 골라 읽을 MCP 서버의 이름. 원본: `apMCPServerNameOK`(`MCPRead.swift` — 아직 이식 전이라 이 청크가 비공개로 옮겼다)
fn mcp_server_name_ok(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 링크 하나를 읽을 대상으로. Atlassian 링크는 연결한 사이트(시험 빌드는 http://127.0.0.1:<포트> 도)와 같은 호스트일 때만 Confluence·Jira 로 본다 —
/// 다른 사이트의 Atlassian 링크는 종류만 알리고(연결 필요) 그 사이트로 요청하지 않는다. 원본: `apReadTarget`
pub fn read_target(raw: &str, atlassian_site: Option<&str>) -> ReadTarget {
    let s = raw.trim_matches(char::is_whitespace);
    let Some(u) = parse_link_url(s) else { return ReadTarget::Bad(NOT_URL.to_string()) };
    let Some(scheme) = u.scheme.as_deref().map(str::to_lowercase) else { return ReadTarget::Bad(NOT_URL.to_string()) };
    if scheme != "https" && scheme != "http" {
        return ReadTarget::Bad(NOT_URL.to_string());
    }
    let Some(host0) = u.host.as_deref() else { return ReadTarget::Bad(NOT_URL.to_string()) };
    let host = host0.to_lowercase();
    if host.is_empty() {
        return ReadTarget::Bad(NOT_URL.to_string());
    }
    if u.user.is_some() || u.password.is_some() {
        return ReadTarget::Bad(WITH_PASSWORD.to_string());
    }
    let path = u.path;
    let atl = host.ends_with(".atlassian.net") || atlassian_site.is_some_and(|b| same_site(s, b));
    let q = query_items(u.query.as_deref().unwrap_or(""));
    if atl {
        let p: Vec<&str> = path.split('/').filter(|x| !x.is_empty()).collect();
        if let Some(i) = p.iter().position(|x| *x == "browse") {
            if i + 1 < p.len() && issue_key_ok(p[i + 1]) {
                return ReadTarget::Jira { key: p[i + 1].to_string() };
            }
        }
        if let Some(k) = query_first(&q, "selectedIssue").filter(|k| issue_key_ok(k)) {
            return ReadTarget::Jira { key: k.to_string() };
        }
        if path.starts_with("/wiki") {
            if let Some(i) = p.iter().position(|x| *x == "pages") {
                if i + 1 < p.len() && page_num_ok(p[i + 1]) {
                    return ReadTarget::Confluence { page: p[i + 1].to_string() };
                }
            }
            if let Some(id) = query_first(&q, "pageId").filter(|id| page_num_ok(id)) {
                return ReadTarget::Confluence { page: id.to_string() };
            }
            return ReadTarget::Bad("Confluence 페이지 주소에서 페이지 번호를 찾지 못했습니다(…/pages/<번호>/… 꼴로 붙여 넣습니다. 짧은 주소 /wiki/x/… 는 아직 읽지 않습니다)".to_string());
        }
        return ReadTarget::Bad("Jira 이슈(/browse/<키>)나 Confluence 페이지(/wiki/…/pages/<번호>) 주소가 아닙니다".to_string());
    }
    if host == "notion.so" || host.ends_with(".notion.so") || host.ends_with(".notion.site") {
        if let Some(id) = query_first(&q, "p").and_then(notion_id) {
            return ReadTarget::Notion { page: id };
        }
        // 끝의 슬래시는 칸만 더하니 자르고 마지막 칸 — 원본 path.split(/).filter { !$0.isEmpty }.last 와 같다
        let last = path.trim_end_matches('/').split('/').next_back().unwrap_or("");
        let tail = last.rsplit('-').next().unwrap_or("");
        if let Some(id) = notion_id(tail) {
            return ReadTarget::Notion { page: id };
        }
        return ReadTarget::Bad("Notion 페이지 주소 끝의 32자리 id 를 찾지 못했습니다".to_string());
    }
    if host == "figma.com" || host.ends_with(".figma.com") {
        let p: Vec<&str> = path.split('/').filter(|x| !x.is_empty()).collect();
        if !(p.len() >= 2 && matches!(p[0], "design" | "file" | "proto" | "board")) {
            return ReadTarget::Bad("Figma 파일 주소(/design/<키>/…)가 아닙니다".to_string());
        }
        let mut key = p[1];
        if p.len() >= 4 && p[2] == "branch" {
            key = p[3];
        }
        if !figma_key_ok(key) {
            return ReadTarget::Bad("Figma 파일 키 꼴이 아닙니다".to_string());
        }
        let mut node = None;
        if let Some(n) = query_first(&q, "node-id") {
            let api = n.replace('-', ":");
            if node_api_ok(&api) {
                node = Some(api);
            }
        }
        return ReadTarget::Figma { file: key.to_string(), node };
    }
    ReadTarget::Web
}

/// 읽을 수 있는 연결인가 — 연결이 있고 범위에 "읽기"가 있고 토큰이 키체인에 있고 만료되지 않고 바탕 주소가 규칙 안일 때 None, 아니면 까닭(요청 0). 원본: `apReadBlocker`
pub fn read_blocker(service: Service, connection: Option<&Connection>, has_secret: bool, dev: bool, now: f64, tz: Tz) -> Option<String> {
    let Some(c) = connection else { return Some(format!("{} 연결이 없습니다(설정 › 연결)", service.label())) };
    if !c.scopes.iter().any(|s| s == "read") {
        return Some(format!("{} 연결의 범위에 '읽기'가 없습니다(설정 › 연결)", service.label()));
    }
    if !has_secret {
        return Some(format!("{} 토큰이 키체인에 없습니다(설정 › 연결)", service.label()));
    }
    if matches!(ap_expiry(&c.expires, now, tz), Expiry::Expired(_)) {
        return Some(format!("{} 토큰 만료일({})이 지났습니다", service.label(), c.expires));
    }
    if service_base(c, dev).is_none() {
        return Some(format!("{} 사이트 주소가 규칙에 맞지 않습니다", service.label()));
    }
    if let Auth::Basic { user } = auth_for(c) {
        if user.is_empty() {
            return Some("Atlassian 계정 메일이 없습니다".to_string());
        }
    }
    None
}

/// 한 줄의 상태를 매기는 데 필요한 것(`apLinkState` 의 매개변수 모음 — 흐름 함수의 매개변수가 늘어나지 않게)
struct LineCtx<'a> {
    connections: &'a [Connection],
    secrets: &'a HashSet<String>,
    dev: bool,
    now: f64,
    tz: Tz,
}

/// 읽을 대상(Jira·Confluence·Notion·Figma) 한 줄의 상태 — 종류와 연결로 매긴다. 원본 `apLinkState` 의 default 면
fn linked(s: &str, label: &str, svc: Service, ctx: &LineCtx) -> ApLink {
    let c = ctx.connections.iter().find(|c| c.service == svc);
    if svc == Service::Atlassian {
        if let Some(c) = c {
            if let Some(base) = service_base(c, ctx.dev) {
                if !same_site(s, &base) {
                    return ApLink::new(s, label, "needs_connection", &format!("연결한 Atlassian 사이트({})와 다른 사이트의 주소입니다", url_host(&base)));
                }
            }
        }
    }
    let has_secret = c.is_some_and(|c| ctx.secrets.contains(&c.id));
    if let Some(why) = read_blocker(svc, c, has_secret, ctx.dev, ctx.now, ctx.tz) {
        return ApLink::new(s, label, "needs_connection", &why);
    }
    ApLink::new(s, label, "connected", &format!("{label} 연결로 메타 정보만 읽습니다"))
}

/// 보드의 자료 링크 줄 — 종류와 읽기 상태(connected·needs_connection·unreadable·mcp)와 까닭. 연결 목록·키체인 있음 여부·고른 MCP 서버로 매긴다. 원본: `apLinkState`
pub fn link_state(
    raw: &str, connections: &[Connection], secrets: &HashSet<String>, mcp: Option<&str>, dev: bool, now: f64, tz: Tz,
) -> ApLink {
    let ctx = LineCtx { connections, secrets, dev, now, tz };
    let s = raw.trim_matches(char::is_whitespace);
    let site = ctx.connections.iter().find(|c| c.service == Service::Atlassian).and_then(|c| service_base(c, dev));
    let t = read_target(s, site.as_deref());
    match &t {
        ReadTarget::Bad(why) => ApLink::new(s, &link_info(s).kind, "unreadable", why),
        ReadTarget::Web => {
            if let Some(m) = mcp.filter(|m| mcp_server_name_ok(m)) {
                ApLink::new(s, "웹", "mcp", &format!("MCP 서버 {} 의 읽기 도구로 읽습니다(그 서버 하나만 켠 짧은 세션)", m))
            } else {
                ApLink::new(s, "웹", "needs_connection", "내장하지 않은 주소라 MCP 서버를 골라 읽습니다(읽기 도구만 켠 짧은 세션)")
            }
        }
        ReadTarget::Jira { .. } | ReadTarget::Confluence { .. } => linked(s, t.label(), Service::Atlassian, &ctx),
        ReadTarget::Notion { .. } => linked(s, t.label(), Service::Notion, &ctx),
        ReadTarget::Figma { .. } => linked(s, t.label(), Service::Figma, &ctx),
    }
}

// ── 읽기 요청 꼴(GET 만) ─────────────────────────────

/// 원본 `readHeaders`(`[Accept: application/json]`) — GET 이라 본문은 없다
fn read_headers() -> BTreeMap<String, String> {
    [("Accept", "application/json")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// 원본 `[Accept: …, Notion-Version: apNotionVersion]`
fn notion_headers() -> BTreeMap<String, String> {
    let mut h = read_headers();
    h.insert("Notion-Version".to_string(), NOTION_VERSION.to_string());
    h
}

/// 주소 한 칸(경로 조각·물음 값) — UTF-8 바이트마다 unreserved(A-Za-z0-9-._~) 밖은 %XX(대문자). 원본 `q`(`addingPercentEncoding`)
fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if is_unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 원본 `addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed)` — unreserved 에 `!$&'()*+,-./:;=?@` 도 그대로
fn query_allowed(s: &str) -> String {
    let allowed = |b: u8| {
        is_unreserved(b)
            || matches!(b, b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b'-' | b'.' | b'/' | b':' | b';' | b'=' | b'?' | b'@')
    };
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if allowed(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Jira 읽기 때 요청할 칸. 원본: `apJiraReadFields`
pub const JIRA_READ_FIELDS: &str =
    "summary,status,issuetype,assignee,reporter,priority,labels,updated,created,description,attachment,issuelinks,parent,fixVersions,components";

/// 원본: `apJiraIssueRequest`(`fields` 는 Swift 기본 인자 `apJiraReadFields` — 부르는 쪽이 `JIRA_READ_FIELDS` 를 넣는다)
pub fn jira_issue_request(key: &str, base: &str, auth: Auth, fields: &str) -> HttpRequest {
    HttpRequest {
        url: format!("{base}/rest/api/3/issue/{}?fields={}", q(key), query_allowed(fields)),
        headers: read_headers(),
        auth,
        ..HttpRequest::default()
    }
}

/// 원본: `apConfluenceReadRequest`
pub fn confluence_read_request(page: &str, base: &str, auth: Auth) -> HttpRequest {
    HttpRequest {
        url: format!(
            "{base}/wiki/rest/api/content/{}?expand=body.storage,version,space,history,children.attachment,metadata.labels",
            q(page)
        ),
        headers: read_headers(),
        auth,
        ..HttpRequest::default()
    }
}

/// 원본: `apNotionPageGetRequest`
pub fn notion_page_get_request(page: &str, base: &str) -> HttpRequest {
    HttpRequest { url: format!("{base}/v1/pages/{}", q(page)), headers: notion_headers(), auth: Auth::Bearer, ..HttpRequest::default() }
}

/// 원본: `apNotionChildrenRequest` — 이어지는 페이지는 `&start_cursor=<커서>`
pub fn notion_children_request(block: &str, base: &str, cursor: Option<&str>) -> HttpRequest {
    let mut url = format!("{base}/v1/blocks/{}/children?page_size=100", q(block));
    if let Some(c) = cursor {
        url.push_str(&format!("&start_cursor={}", q(c)));
    }
    HttpRequest { url, headers: notion_headers(), auth: Auth::Bearer, ..HttpRequest::default() }
}

/// 원본: `apFigmaReadRequest` — node 가 있으면 골라 읽기(`/nodes?ids=`)
pub fn figma_read_request(file: &str, node: Option<&str>, base: &str) -> HttpRequest {
    let url = match node {
        Some(n) => format!("{base}/v1/files/{}/nodes?ids={}", q(file), q(n)),
        None => format!("{base}/v1/files/{}?depth=2", q(file)),
    };
    HttpRequest { url, headers: read_headers(), auth: Auth::Figma, ..HttpRequest::default() }
}

/// 원본: `apFigmaImageRequest`
pub fn figma_image_request(file: &str, node: &str, base: &str) -> HttpRequest {
    HttpRequest {
        url: format!("{base}/v1/images/{}?ids={}&format=png&scale=1", q(file), q(node)),
        headers: read_headers(),
        auth: Auth::Figma,
        ..HttpRequest::default()
    }
}

// ── Figma 그림 주소 허용 ─────────────────────────────

/// /v1/images 가 돌려주는 내보낸 그림 자리의 정한 호스트. 원본: `apFigmaImageHosts`
pub const FIGMA_IMAGE_HOSTS: [&str; 1] = ["figma-alpha-api.s3.us-west-2.amazonaws.com"];

/// 원본: `apFigmaImageHostSuffix`
pub const FIGMA_IMAGE_HOST_SUFFIX: &str = ".figma.com";

/// Figma 그림 주소 허용 — 내보낸 그림 자리(https 의 정한 호스트: FIGMA_IMAGE_HOSTS 와 *.figma.com 꼬리)만. 시험 빌드는 가짜 서버
/// http://127.0.0.1(포트 아무것)도. 사용자 정보·다른 포트(https 는 443)·다른 호스트는 받지 않는다. 이 요청에는 인증 머리를 붙이지 않는다. 원본: `apImageURLOK`
pub fn image_url_ok(url: &str, dev: bool) -> bool {
    if grapheme_len(url) > 4000 {
        return false;
    }
    let quoted = "\"'<>\\`{}|^";
    if !url.chars().all(|c| {
        let v = c as u32;
        (0x20 < v) && (v < 0x7F) && !quoted.contains(c)
    }) {
        return false;
    }
    let Some(c) = parse_link_url(url) else { return false };
    let Some(scheme) = c.scheme.as_deref().map(str::to_lowercase) else { return false };
    let Some(host) = c.host.as_deref().map(str::to_lowercase) else { return false };
    if host.is_empty() || c.user.is_some() || c.password.is_some() {
        return false;
    }
    if scheme == "https" {
        if !(c.port.is_none() || c.port == Some(443)) {
            return false;
        }
        return FIGMA_IMAGE_HOSTS.contains(&host.as_str())
            || (host.ends_with(FIGMA_IMAGE_HOST_SUFFIX) && host.len() > FIGMA_IMAGE_HOST_SUFFIX.len());
    }
    dev && scheme == "http" && host == "127.0.0.1"
}

/// 받은 그림의 크기 상한. 원본: `apImageMaxBytes`
pub const IMAGE_MAX_BYTES: usize = 8 << 20;

/// 그림 첫 8바이트의 PNG 서명(89 50 4E 47 0D 0A 1A 0A). 원본: `apIsPNG`(`d.count > 8` — 8바이트만은 아니다)
pub fn is_png(d: &[u8]) -> bool {
    d.len() > 8 && d.get(0..8) == Some(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A][..])
}

// MARK - 응답 → 메타

// 원본: `Sources/AutopilotRules/Reading.swift` 181~477줄(MARK 응답 → 메타). Meta.swift 393~470줄의 도우미(apStripMarkup·
// apMarkupChunks·apChunked·apSectionWanted)는 R-Meta-d2 에서 meta.rs 로 옮겨져 `use crate::rules::meta::{…}` 로 온다. 정규식 여섯은 meta.rs 의 손으로 옮긴 찾기로 갔다.
// 앞 청크(a)의 `page_num_ok`·`notion_id`와 요청 꼴을 그대로 쓴다.

/// JSON 문자열 꾸러미. Swift 에서 `"꾸러미"`(`\(v)`) — 수는 `NSNumber` 설명을 본뜬다.
fn sv(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => number_string(n),
        Some(value_bool @ Value::Bool(_)) => match value_bool {
            Value::Bool(true) => "1".to_string(),
            _ => "0".to_string(),
        },
        _ => String::new(),
    }
}

/// `NSNumber description` 을 본뜬 숫자 꼴. 정수(i64·u64)는 그대로, 정수 값 실수(2⁵³ 안)는 정수 꼴,
/// 그 밖의 실수는 Rust `f64` Display — Swift 지수 표기(`e+21`)와 다를 수 있어 헤더 기록에 남겼다.
fn number_string(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let f = n.as_f64().unwrap_or(0.0);
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
        return format!("{f:.0}");
    }
    format!("{f}")
}

static EMPTY_OBJECT: OnceLock<Map<String, Value>> = OnceLock::new();

fn empty_object() -> &'static Map<String, Value> {
    EMPTY_OBJECT.get_or_init(Map::new)
}

/// 개체 꾸러미 — Swift `v as? [String: Any] ?? [:]`. 빈 개체를 늘 공유해 대여를 단순히 둔다.
fn dict(v: Option<&Value>) -> &Map<String, Value> {
    v.and_then(Value::as_object).unwrap_or_else(|| empty_object())
}

/// 배열 꾸러미의 개체들만 — Swift `(v as? [[String: Any]])?.compactMap { $0 as? [String: Any] } ?? []`
fn arr(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    match v {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_object).collect(),
        _ => Vec::new(),
    }
}

/// 숫자 꾸러미. Swift `(v as? NSNumber)?.intValue` — 참·거짓은 1·0, 실수는 잘라서.
fn num_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i
            } else if let Some(u) = n.as_u64() {
                if u <= i64::MAX as u64 {
                    u as i64
                } else {
                    i64::MAX
                }
            } else {
                n.as_f64().map(|f| f.trunc() as i64).unwrap_or(0)
            }
        }
        Some(Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// 실수 꾸러미. Swift `(v as? NSNumber)?.doubleValue`
fn num_f64(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// 사람 이름 — `displayName ?? name`
fn name_of(o: &Map<String, Value>) -> String {
    let shown = sv(o.get("displayName"));
    if shown.is_empty() {
        sv(o.get("name"))
    } else {
        shown
    }
}

/// `^~?[A-Za-z0-9_-]{1,100}$` — Confluence 스페이스 키(Outbox.swift 의 apSpaceKeyPattern)
fn space_key_ok(s: &str) -> bool {
    let after = s.strip_prefix('~').unwrap_or(s);
    let ok_len = !after.is_empty() && after.len() <= 100;
    ok_len && after.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Confluence 저장 형식에서 링크·첨부 이름. 원본: `storageLinks`(`Reading.swift` 266-282).
/// 찾기 넷은 원본 정규식 그대로다(맞은 자리는 300 개까지). `/` 로 시작하는 주소에는 사이트를 붙인다.
fn storage_links(s: &str, base: &str) -> Vec<MetaLink> {
    let mut out: Vec<MetaLink> = Vec::new();
    for h in attr_matches(s, "a", "href", 300, a_tail) {
        out.push(MetaLink::new(&strip_markup(&s[h.inner.0..h.inner.1]), &decode_entities(&s[h.v0..h.vend]), "link"));
    }
    for h in attr_matches(s, "ri:url", "ri:value", 300, quote_tail) {
        let u = decode_entities(&s[h.v0..h.vend]);
        out.push(MetaLink::new(&u, &u, "link"));
    }
    for h in attr_matches(s, "ri:page", "ri:content-title", 300, quote_tail) {
        out.push(MetaLink::new(&decode_entities(&s[h.v0..h.vend]), "", "page"));
    }
    for h in attr_matches(s, "ri:attachment", "ri:filename", 300, quote_tail) {
        out.push(MetaLink::new(&decode_entities(&s[h.v0..h.vend]), "", "attachment"));
    }
    for l in out.iter_mut() {
        if l.url.starts_with('/') {
            l.url = format!("{base}{}", l.url);
        }
    }
    out
}

/// ADF 문서 훑기 상태 — 지금 덩어리(수준·제목·글), 아직 매끈하지 않은 줄들, 링크, 남은 마디 자리(~20000).
struct AdfChunking {
    chunks: Vec<TextChunk>,
    links: Vec<MetaLink>,
    cur: TextChunk,
    lines: Vec<String>,
    budget: i64,
}

impl AdfChunking {
    /// 인라인 글 담기(AADF) — 글 마디·멘션·이모지·카드 주소·날짜·상태. 링크 매무새와 카드 주소는 링크에도 넣는다.
    fn adf_inline(&mut self, n: &Map<String, Value>) -> String {
        self.budget -= 1;
        if self.budget <= 0 {
            return String::new();
        }
        match sv(n.get("type")).as_str() {
            "text" => {
                let t = sv(n.get("text"));
                for m in arr(n.get("marks")) {
                    if sv(m.get("type")) == "link" {
                        self.links.push(MetaLink::new(&t, &sv(dict(m.get("attrs")).get("href")), "link"));
                    }
                }
                t
            }
            "hardBreak" => "\n".to_string(),
            "mention" => sv(dict(n.get("attrs")).get("text")),
            "emoji" => {
                let shown = sv(dict(n.get("attrs")).get("text"));
                if shown.is_empty() {
                    sv(dict(n.get("attrs")).get("shortName"))
                } else {
                    shown
                }
            }
            "inlineCard" | "blockCard" | "embedCard" => {
                let u = sv(dict(n.get("attrs")).get("url"));
                if !u.is_empty() {
                    self.links.push(MetaLink::new(&u, &u, "link"));
                }
                u
            }
            "date" => sv(dict(n.get("attrs")).get("timestamp")),
            "status" => sv(dict(n.get("attrs")).get("text")),
            _ => arr(n.get("content")).iter().map(|c| self.adf_inline(c)).collect::<Vec<_>>().concat(),
        }
    }

    /// 덩이 담기(AADF) — heading 은 지금 덩어리를 매끈하게 마치고 새 덩어리를, 문단·코드는 문단 줄을 쌓는다.
    /// 목록·표·인용·패널·확장 칸은 안의 덩이를 차례로, 그림 칸·구분선는 흘리고, 블록 카드는 링크만 남긴다.
    fn adf_block(&mut self, n: &Map<String, Value>) {
        self.budget -= 1;
        if self.budget <= 0 {
            return;
        }
        match sv(n.get("type")).as_str() {
            "heading" => {
                self.cur.text = self.lines.join("\n");
                self.lines = Vec::new();
                if self.cur.level > 0 || !self.cur.text.is_empty() {
                    self.chunks.push(self.cur.clone());
                }
                let title = arr(n.get("content")).iter().map(|c| self.adf_inline(c)).collect::<Vec<_>>().concat().replace('\n', " ");
                let attrs = dict(n.get("attrs"));
                // 수준 — `(attrs["level"] as? NSNumber)?.intValue ?? 1`(글이나 참·거짓도 수로 본다, 그 밖은 1)
                let level = match attrs.get("level") {
                    Some(Value::Number(_)) => num_i64(attrs.get("level")),
                    Some(Value::Bool(b)) => i64::from(*b),
                    _ => 1,
                };
                self.cur = TextChunk::new(level, &title, "");
            }
            "paragraph" | "codeBlock" => {
                let t = arr(n.get("content")).iter().map(|c| self.adf_inline(c)).collect::<Vec<_>>().concat();
                for piece in t.split('\n') {
                    let piece = piece.trim_matches(swift_ws);
                    if !piece.is_empty() {
                        self.lines.push(piece.to_string());
                    }
                }
            }
            "mediaSingle" | "mediaGroup" | "media" | "rule" => {}
            "blockCard" | "embedCard" => {
                self.adf_inline(n);
            }
            _ => {
                for c in arr(n.get("content")) {
                    self.adf_block(c);
                }
            }
        }
    }
}

/// Jira 문서 형식(ADF) → 글 덩어리와 링크. 원본: `apADFChunks`(`Reading.swift` 186-231)
pub fn adf_chunks(doc: &Map<String, Value>) -> (Vec<TextChunk>, Vec<MetaLink>) {
    let mut a = AdfChunking { chunks: Vec::new(), links: Vec::new(), cur: TextChunk::new(0, "", ""), lines: Vec::new(), budget: 20000 };
    for c in arr(doc.get("content")) {
        a.adf_block(c);
    }
    a.cur.text = a.lines.join("\n");
    if a.cur.level > 0 || !a.cur.text.is_empty() {
        a.chunks.push(std::mem::take(&mut a.cur));
    }
    (a.chunks, a.links)
}

/// Notion 글 칸(rich_text 배열) → 글과 링크 — 빈 plain_text 는 안의 text.content 로, 주소 있으면 링크에도 넣는다.
/// 원본: `notionRich`(`Reading.swift` 322-330)
fn notion_rich(v: Option<&Value>, links: &mut Vec<MetaLink>) -> String {
    arr(v).iter().map(|r| {
        let plain = sv(r.get("plain_text"));
        let t = if plain.is_empty() { sv(dict(r.get("text")).get("content")) } else { plain };
        let h = sv(r.get("href"));
        if !h.is_empty() {
            links.push(MetaLink::new(&t, &h, "link"));
        }
        t
    }).collect::<Vec<_>>().concat()
}

/// Notion 속성 하나 → 보일 글(제목 칸은 따로). 원본: `notionProp`(`Reading.swift` 332-348). rich_text 의 링크는 버린다.
fn notion_prop(p: &Map<String, Value>) -> String {
    let mut scratch: Vec<MetaLink> = Vec::new();
    let kind = sv(p.get("type"));
    match kind.as_str() {
        "select" | "status" => sv(dict(p.get(kind.as_str())).get("name")),
        "multi_select" => arr(p.get("multi_select")).iter().map(|o| sv(o.get("name"))).collect::<Vec<_>>().join(", "),
        "people" => arr(p.get("people")).iter()
            .map(|o| sv(o.get("name"))).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", "),
        "date" => {
            let d = dict(p.get("date"));
            [sv(d.get("start")), sv(d.get("end"))].iter()
                .filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ~ ")
        }
        "checkbox" => {
            if matches!(p.get("checkbox"), Some(Value::Bool(true))) {
                "예".to_string()
            } else {
                "아니요".to_string()
            }
        }
        "number" => sv(p.get("number")),
        "url" | "email" | "phone_number" => sv(p.get(kind.as_str())),
        "rich_text" => notion_rich(p.get("rich_text"), &mut scratch),
        "created_time" | "last_edited_time" => sv(p.get(kind.as_str())),
        _ => String::new(),
    }
}

/// Jira 응답 → 메타. 원본: `apMetaFromJira`(`Reading.swift` 233-283)
pub fn meta_from_jira(
    o: &Map<String, Value>, key: &str, base: &str, want: &[String], prompt: &str, dev: bool,
) -> Meta {
    let f = dict(o.get("fields"));
    let mut m = Meta::default();
    m.source = "jira".to_string();
    m.id = if issue_key_ok(&sv(o.get("key"))) { sv(o.get("key")) } else { key.to_string() };
    m.url = format!("{}/browse/{}", base, m.id);
    m.title = sv(f.get("summary"));
    m.kind = name_of(dict(f.get("issuetype")));
    m.modified = sv(f.get("updated"));
    m.author = name_of(dict(f.get("reporter")));
    let (chunks, links) = adf_chunks(dict(f.get("description")));
    let c = chunked(&chunks, want, prompt);
    m.toc = c.toc;
    m.length = c.length;
    m.summary = c.summary;
    m.sections = c.sections;
    let mut fields = vec![
        MetaField::new("상태", &name_of(dict(f.get("status")))),
        MetaField::new("담당", &name_of(dict(f.get("assignee")))),
        MetaField::new("우선순위", &name_of(dict(f.get("priority")))),
    ];
    if let Some(p) = f.get("parent").and_then(Value::as_object) {
        fields.push(MetaField::new("상위", &format!("{} {}", sv(p.get("key")), sv(dict(p.get("fields")).get("summary")))));
    }
    let labels: Vec<&str> = f.get("labels").and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
    if !labels.is_empty() {
        fields.push(MetaField::new("레이블", &labels.join(", ")));
    }
    let comps: Vec<String> = arr(f.get("components")).iter()
        .map(|x| sv(x.get("name"))).filter(|x| !x.is_empty()).collect();
    if !comps.is_empty() {
        fields.push(MetaField::new("구성 요소", &comps.join(", ")));
    }
    let fix: Vec<String> = arr(f.get("fixVersions")).iter()
        .map(|x| sv(x.get("name"))).filter(|x| !x.is_empty()).collect();
    if !fix.is_empty() {
        fields.push(MetaField::new("수정 버전", &fix.join(", ")));
    }
    if !sv(f.get("created")).is_empty() {
        fields.push(MetaField::new("만든 때", &sv(f.get("created"))));
    }
    fields.retain(|x| !x.value.trim_matches(swift_ws).is_empty());
    m.fields = fields;

    let mut ls: Vec<MetaLink> = arr(f.get("attachment")).iter()
        .map(|x| MetaLink::new(&sv(x.get("filename")), &sv(x.get("content")), "attachment")).collect();
    for l in arr(f.get("issuelinks")) {
        let t = dict(l.get("type"));
        if let Some(x) = l.get("outwardIssue").and_then(Value::as_object) {
            let title = format!("{} {} {}", sv(t.get("outward")), sv(x.get("key")), sv(dict(x.get("fields")).get("summary")));
            ls.push(MetaLink::new(&title, &format!("{}/browse/{}", base, sv(x.get("key"))), "issue"));
        }
        if let Some(x) = l.get("inwardIssue").and_then(Value::as_object) {
            let title = format!("{} {} {}", sv(t.get("inward")), sv(x.get("key")), sv(dict(x.get("fields")).get("summary")));
            ls.push(MetaLink::new(&title, &format!("{}/browse/{}", base, sv(x.get("key"))), "issue"));
        }
    }
    m.links = [ls, links].concat();
    meta_clamp(&m, dev)
}

/// Confluence 저장 형식 응답 → 메타. 원본: `apMetaFromConfluence`(`Reading.swift` 287-324)
pub fn meta_from_confluence(
    o: &Map<String, Value>, page: &str, base: &str, want: &[String], prompt: &str, dev: bool,
) -> Meta {
    // 주소는 응답의 주소 칸을 믿지 않고 사이트와 번호로 만든다(⑩-1 과 같은 꼴)
    let id = if page_num_ok(&sv(o.get("id"))) { sv(o.get("id")) } else { page.to_string() };
    let space = dict(o.get("space"));
    let sk = sv(space.get("key"));
    let url = if space_key_ok(&sk) {
        format!("{}/wiki/spaces/{}/pages/{}", base, sk, id)
    } else {
        format!("{}/wiki/pages/viewpage.action?pageId={}", base, id)
    };
    let ver = dict(o.get("version"));
    let mut m = Meta {
        source: "confluence".to_string(),
        id,
        url,
        title: sv(o.get("title")),
        kind: if sv(o.get("type")) == "blogpost" {
            "Confluence 블로그 글".to_string()
        } else {
            "Confluence 페이지".to_string()
        },
        modified: sv(ver.get("when")),
        author: name_of(dict(dict(o.get("history")).get("createdBy"))),
        ..Default::default()
    };
    let storage = sv(dict(dict(o.get("body")).get("storage")).get("value"));
    let c = chunked(&markup_chunks(&storage), want, prompt);
    m.toc = c.toc;
    m.length = c.length;
    m.summary = c.summary;
    m.sections = c.sections;
    let spacing = [sv(space.get("key")), sv(space.get("name"))].iter()
        .filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
    let mut fields = vec![
        MetaField::new("스페이스", &spacing),
        MetaField::new("상태", &sv(o.get("status"))),
        MetaField::new("판", &sv(ver.get("number"))),
        MetaField::new("마지막 고친 사람", &name_of(dict(ver.get("by")))),
    ];
    let labels: Vec<String> = arr(dict(dict(o.get("metadata")).get("labels")).get("results")).iter()
        .map(|x| sv(x.get("name"))).filter(|x| !x.is_empty()).collect();
    if !labels.is_empty() {
        fields.push(MetaField::new("레이블", &labels.join(", ")));
    }
    fields.retain(|x| !x.value.is_empty());
    m.fields = fields;
    let atts: Vec<MetaLink> = arr(dict(dict(o.get("children")).get("attachment")).get("results")).iter()
        .map(|a| {
            let download = sv(dict(a.get("_links")).get("download"));
            let url = if download.starts_with('/') { format!("{}/wiki{}", base, download) } else { download };
            MetaLink::new(&sv(a.get("title")), &url, "attachment")
        })
        .collect();
    m.links = [atts, storage_links(&storage, base)].concat();
    meta_clamp(&m, dev)
}

/// Notion 페이지 응답 → 메타(칸들(blocks)은 응답의 자식 칸들). 원본: `apMetaFromNotion`(`Reading.swift` 343-391)
pub fn meta_from_notion(
    o: &Map<String, Value>, blocks: &[&Map<String, Value>], id: &str, link: &str,
    want: &[String], prompt: &str, dev: bool,
) -> Meta {
    // 주소는 www.notion.so·notion.so·*.notion.site 의 링크면 그대로 쓰고, 아니면 사람이 볼 링크로
    let pu = sv(o.get("url"));
    let host = url_host(&pu);
    let notion_url = if host == "www.notion.so" || host == "notion.so" || host.ends_with(".notion.site") {
        pu
    } else {
        link.to_string()
    };
    let mut m = Meta {
        source: "notion".to_string(),
        id: notion_id(&sv(o.get("id"))).unwrap_or_else(|| id.to_string()),
        url: notion_url,
        kind: if sv(o.get("object")) == "page" {
            "Notion 페이지".to_string()
        } else {
            format!("Notion {}", sv(o.get("object")))
        },
        modified: sv(o.get("last_edited_time")),
        author: sv(dict(o.get("created_by")).get("name")),
        ..Default::default()
    };
    let mut links: Vec<MetaLink> = Vec::new();
    let mut fields: Vec<MetaField> = Vec::new();
    // 속성은 이름 순(원본의 sorted — 바뀌지 않는 보임 순서). 제목 칸은 m.title 로, 나머지는 비어 있지 않을 때만 칸으로
    let mut props: Vec<(&String, &Value)> = dict(o.get("properties")).iter().collect();
    props.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in &props {
        let p = dict(Some(*v));
        if sv(p.get("type")) == "title" {
            m.title = notion_rich(p.get("title"), &mut links);
            continue;
        }
        let x = notion_prop(p);
        if !x.is_empty() {
            fields.push(MetaField::new(k, &x));
        }
    }
    m.fields = fields;
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut cur = TextChunk::new(0, "", "");
    let mut lines: Vec<String> = Vec::new();
    for b in blocks {
        let t = sv(b.get("type"));
        let body = dict(b.get(t.as_str()));
        match t.as_str() {
            "heading_1" | "heading_2" | "heading_3" => {
                cur.text = lines.join("\n");
                lines = Vec::new();
                if cur.level > 0 || !cur.text.is_empty() {
                    chunks.push(cur.clone());
                }
                // 수준 — 헤딩 종류의 끝 글자(heading_1~3)가 곧 수준, 모르면 1
                let level = match t.chars().next_back().and_then(|c| c.to_digit(10)) {
                    Some(d) => i64::from(d),
                    None => 1,
                };
                cur = TextChunk::new(level, &notion_rich(body.get("rich_text"), &mut links), "");
            }
            "child_page" | "child_database" => links.push(MetaLink::new(&sv(body.get("title")), "", "page")),
            "bookmark" | "link_preview" | "embed" => {
                let u = sv(body.get("url"));
                let cap = notion_rich(body.get("caption"), &mut links);
                let shown = if cap.is_empty() { u.clone() } else { cap };
                links.push(MetaLink::new(&shown, &u, "link"));
            }
            "image" | "file" | "pdf" | "video" => {
                let external = sv(dict(body.get("external")).get("url"));
                let u = if external.is_empty() { sv(dict(body.get("file")).get("url")) } else { external };
                let cap = notion_rich(body.get("caption"), &mut links);
                let name_ = sv(body.get("name"));
                let shown = if cap.is_empty() {
                    if name_.is_empty() { t.clone() } else { name_ }
                } else {
                    cap
                };
                let kind = if t == "image" { "image" } else { "attachment" };
                links.push(MetaLink::new(&shown, &u, kind));
            }
            _ => {
                let x = notion_rich(body.get("rich_text"), &mut links);
                if !x.is_empty() {
                    for piece in x.split('\n') {
                        if !piece.trim_matches(swift_ws).is_empty() {
                            lines.push(piece.to_string());
                        }
                    }
                }
            }
        }
    }
    cur.text = lines.join("\n");
    if cur.level > 0 || !cur.text.is_empty() {
        chunks.push(std::mem::take(&mut cur));
    }
    let c = chunked(&chunks, want, prompt);
    m.toc = c.toc;
    m.length = c.length;
    m.summary = c.summary;
    m.sections = c.sections;
    m.links = links;
    meta_clamp(&m, dev)
}

/// Figma 색 → #RRGGBB(스칼라 1 이 아니면 AA 를 덧대어 붙인다). r·g·b 는 수여야 한다. 원본: `hex`(`Reading.swift` 393-400)
fn figma_hex(c: &Map<String, Value>, opacity: f64) -> Option<String> {
    let need = |v: Option<&Value>| match v {
        Some(Value::Number(_)) => Some(num_f64(v)),
        Some(Value::Bool(true)) => Some(1.0),
        Some(Value::Bool(false)) => Some(0.0),
        _ => None,
    };
    let r = need(c.get("r"))?;
    let g = need(c.get("g"))?;
    let b = need(c.get("b"))?;
    let a0 = match c.get("a") {
        Some(Value::Number(_)) => num_f64(c.get("a")),
        Some(Value::Bool(true)) => 1.0,
        Some(Value::Bool(false)) => 0.0,
        _ => 1.0,
    };
    let a = a0 * opacity;
    let h = |x: f64| format!("{:02X}", (x.clamp(0.0, 1.0) * 255.0).round() as i64);
    Some(format!("#{}{}{}{}", h(r), h(g), h(b), if a < 0.999 { h(a) } else { String::new() }))
}

/// 프레임 겸 자리 종류들 — 원본 `frameTypes`(`Reading.swift` 402-404)
const FRAME_TYPES: [&str; 4] = ["FRAME", "COMPONENT", "COMPONENT_SET", "INSTANCE"];

/// 프레임 훑기 상태 — 마디 센 수(5,000 까지), 색·글꼴·부품(겹치지 않게).
struct FrameWalk {
    seen: i64,
    colors: Vec<String>,
    fonts: Vec<String>,
    comps: Vec<String>,
}

impl FrameWalk {
    /// 프레임 훑기 — TEXT 의 글과 글꼴, INSTANCE 의 부품 이름(맨 겉은 빼고), 견고한 칠 색, 아래 칸을 차례로. 원본: `frameOf`의 `walk`
    fn frame_walk(&mut self, x: &Map<String, Value>, components: &Map<String, Value>, top: bool, f: &mut Frame) {
        self.seen += 1;
        if self.seen > 5000 || matches!(x.get("visible"), Some(Value::Bool(false))) {
            return;
        }
        let t = sv(x.get("type"));
        if t == "TEXT" {
            let characters = sv(x.get("characters"));
            if !characters.is_empty() {
                f.texts.push(characters);
            }
            let style = dict(x.get("style"));
            let font = [sv(style.get("fontFamily")), sv(style.get("fontWeight"))].iter()
                .filter(|piece| !piece.is_empty()).cloned().collect::<Vec<_>>().join(" ");
            if !font.is_empty() && !self.fonts.contains(&font) {
                self.fonts.push(font);
            }
        }
        if t == "INSTANCE" && !top {
            let component_name = sv(dict(components.get(sv(x.get("componentId")).as_str())).get("name"));
            let label = if component_name.is_empty() { sv(x.get("name")) } else { component_name };
            if !label.is_empty() && !self.comps.contains(&label) {
                self.comps.push(label);
            }
        }
        for fill in arr(x.get("fills")) {
            if sv(fill.get("type")) != "SOLID" || matches!(fill.get("visible"), Some(Value::Bool(false))) {
                continue;
            }
            let opacity = match fill.get("opacity") {
                Some(Value::Bool(true)) => 1.0,
                Some(Value::Bool(false)) => 0.0,
                Some(_) => num_f64(fill.get("opacity")),
                None => 1.0,
            };
            if let Some(h) = figma_hex(dict(fill.get("color")), opacity) {
                if !self.colors.contains(&h) {
                    self.colors.push(h);
                }
            }
        }
        for c in arr(x.get("children")) {
            self.frame_walk(c, components, false, f);
        }
    }
}

/// 프레임 하나의 표 — 크기, 안의 글(TEXT), 부품(INSTANCE 의 이름), 칠한 색(SOLID), 글꼴. 마디는 5,000개까지만 본다.
/// 원본: `frameOf`(`Reading.swift` 406-433)
fn frame_of(n: &Map<String, Value>, components: &Map<String, Value>) -> Frame {
    let bb = dict(n.get("absoluteBoundingBox"));
    let mut f = Frame {
        id: sv(n.get("id")),
        name: sv(n.get("name")),
        width: num_f64(bb.get("width")),
        height: num_f64(bb.get("height")),
        ..Default::default()
    };
    let mut walk = FrameWalk { seen: 0, colors: Vec::new(), fonts: Vec::new(), comps: Vec::new() };
    walk.frame_walk(n, components, true, &mut f);
    f.colors = walk.colors;
    f.fonts = walk.fonts;
    f.components = walk.comps;
    f
}

/// Figma 응답(nodes 또는 파일 depth=2) → 메타와 그림을 받을 프레임 id. 원본: `apMetaFromFigma`(`Reading.swift` 436-472)
pub fn meta_from_figma(
    o: &Map<String, Value>, file: &str, node: Option<&str>, link: &str, dev: bool,
) -> (Meta, Option<String>) {
    let mut m = Meta {
        source: "figma".to_string(),
        url: link.to_string(),
        modified: sv(o.get("lastModified")),
        kind: "Figma 프레임".to_string(),
        ..Default::default()
    };
    let mut frames: Vec<Frame> = Vec::new();
    let mut page = String::new();
    if let Some(node) = node {
        m.id = format!("{file}/{node}");
        let entry = dict(dict(o.get("nodes")).get(node));
        let doc = dict(entry.get("document"));
        let comps = dict(entry.get("components"));
        m.title = [sv(o.get("name")), sv(doc.get("name"))].iter()
            .filter(|piece| !piece.is_empty()).cloned().collect::<Vec<_>>().join(" › ");
        if FRAME_TYPES.contains(&sv(doc.get("type")).as_str()) {
            frames.push(frame_of(doc, comps));
        } else {
            page = sv(doc.get("name"));
            frames = arr(doc.get("children")).iter()
                .filter(|c| FRAME_TYPES.contains(&sv(c.get("type")).as_str()))
                .take(limit::FRAMES)
                .map(|c| frame_of(c, comps))
                .collect();
        }
    } else {
        m.id = file.to_string();
        m.title = sv(o.get("name"));
        m.kind = "Figma 파일".to_string();
        let pages = arr(dict(o.get("document")).get("children"));
        page = pages.iter().map(|p| sv(p.get("name"))).collect::<Vec<_>>().join(", ");
        for p in &pages {
            for c in arr(p.get("children")) {
                // 프레임 겸 자리들만, 표 수(50)까지만
                if FRAME_TYPES.contains(&sv(c.get("type")).as_str()) && frames.len() < limit::FRAMES {
                    frames.push(frame_of(c, empty_object()));
                }
            }
        }
    }
    m.frames = frames;
    m.toc = m.frames.iter().map(|f| TocItem::new(1, &f.name)).collect();
    m.length = m.frames.iter().map(|f| f.texts.iter().map(|t| grapheme_len(t) as i64).sum::<i64>()).sum::<i64>();
    let mut fields = vec![MetaField::new("프레임", &format!("{}개", m.frames.len()))];
    if !page.is_empty() {
        fields.insert(0, MetaField::new(if node.is_none() { "페이지" } else { "자리" }, &page));
    }
    if !sv(o.get("version")).is_empty() {
        fields.push(MetaField::new("판", &sv(o.get("version"))));
    }
    m.fields = fields;
    let texts_total = m.frames.iter().map(|f| f.texts.len()).sum::<usize>();
    let frames_text = if m.frames.is_empty() {
        String::new()
    } else {
        m.frames.iter().take(5).map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
    };
    m.summary = vec![format!(
        "프레임 {}개 · 글 {}개{}",
        m.frames.len(),
        texts_total,
        if frames_text.is_empty() { String::new() } else { format!(" · {}", frames_text) }
    )];
    let image_node = m.frames.first().map(|f| f.id.clone()).filter(|id| !id.is_empty());
    (meta_clamp(&m, dev), image_node)
}

// ── 읽기 유스케이스(원본 Reading.swift 478~560줄) ───────────────────

/// 지금 시각 — 유스케이스가 어댑터의 실제 시계를 몰라도 되게 하는 포트. 원본: 포트 `APClock`
pub trait ReadClock {
    fn now(&self) -> f64;
}

/// 기다림(429 뒤에 다시 읽기까지). 원본: 포트 `APWaiter`
pub trait ReadWaiter {
    fn wait(&self, seconds: f64);
}

/// 링크 하나를 읽은 결과. 원본: `APReadResult`
#[derive(Debug, Clone)]
pub struct ReadResult {
    pub target: ReadTarget,
    pub meta: Option<Meta>,
    /// 피그마 그림(PNG 본문)
    pub image: Option<Vec<u8>>,
    /// 그림을 받은 호스트
    pub image_host: String,
    pub image_note: String,
    pub requests: usize,
    /// 가린 비밀 수
    pub redacted: usize,
    pub note: String,
    /// 쓴 연결 id(요청 0 이면 빈 글자)
    pub connection: String,
}

impl ReadResult {
    /// 메타를 받았는가
    pub fn ok(&self) -> bool {
        self.meta.is_some()
    }
}

impl Default for ReadResult {
    fn default() -> Self {
        ReadResult {
            target: ReadTarget::Web,
            meta: None,
            image: None,
            image_host: String::new(),
            image_note: String::new(),
            requests: 0,
            redacted: 0,
            note: String::new(),
            connection: String::new(),
        }
    }
}

/// 실패한 답을 사람 말로 — 받은 쪽의 오류가 있으면 그것이 먼저다. 원본: `apReadFailNote`
pub fn read_fail_note(r: &HttpResponse) -> String {
    if let Some(e) = &r.error {
        return e.clone();
    }
    match r.status {
        401 => "읽을 수 없음: 거부됨(401). 토큰이 틀렸거나 만료·취소됐습니다".to_string(),
        403 => "읽을 수 없음: 권한 없음(403). 연결한 계정이 그 자리를 볼 수 없습니다".to_string(),
        404 => "읽을 수 없음: 찾지 못했습니다(404). 주소의 번호·키를 확인합니다".to_string(),
        410 => "읽을 수 없음: 지워진 글입니다(410)".to_string(),
        429 => "읽을 수 없음: 요청이 많다며 거절했습니다(429). 잠시 뒤 다시 읽습니다".to_string(),
        300..=399 => format!("읽을 수 없음: 다른 주소로 보내려 해 따라가지 않았습니다({})", r.status),
        500..=599 => format!("읽을 수 없음: 받는 쪽 오류({})", r.status),
        _ => format!("읽을 수 없음: 받는 쪽이 {} 로 답했습니다", r.status),
    }
}

/// 답을 JSON 객체로 — 오류 없이 200 이고 본문이 4MB 안일 때만. 원본: `json`
fn read_json(r: &HttpResponse) -> Option<Map<String, Value>> {
    if r.error.is_some() || r.status != 200 || r.body.len() > (4 << 20) {
        return None;
    }
    serde_json::from_slice::<Value>(&r.body).ok().and_then(|v| v.as_object().cloned())
}

/// 한 번 보내기에서 429·503 뒤 다시 보내는 횟수. 원본 `apRetryMax`
const RETRY_MAX: i32 = 3;
/// 한 번 기다리는 초 상한. 원본 `apRetryWaitMax`
const RETRY_WAIT_MAX: f64 = 20.0;

/// 429·503 에 응답 주인이 알려 준 기다림(초). 원본 `Outbox.swift` 의 `apRetryWait` — 아직 이식 전이라
/// `notify_rules.rs` 의 비공개 두 번째 부본과 같은 것(세 번째 부본)을 여기에도 둔다
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

/// HTTP 때 글자(EEE, dd MMM yyyy HH:mm:ss zzz)를 유닉스 때로. 몫은 GMT·UTC 만 — `notify_rules.rs` 의 부본과 같은 것
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

/// 1970-01-01 부터 센 날 수(Howard Hinnant 의 days_from_civil) — `connections.rs` 같은 것의 다섯 번째 부본
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 읽기의 `send` — GET 만, 연결의 바탕 주소 아래로만 보낸다. 429·503 은 retry-after 대로 기다렸다
/// (예산 안이면) 다시 보낸다. 횟수는 결과의 `requests` 에 세고, 기다림은 `waiter` 로 시계를 옮겨 재며,
/// 예산이 끝나면 마지막 답(실패)을 돌려준다. 원본: `apReadLink` 안의 `send`
#[allow(clippy::too_many_arguments)]
fn read_send(
    http: &dyn ConnHttp,
    clock: &dyn ReadClock,
    waiter: &dyn ReadWaiter,
    req: &HttpRequest,
    header: Option<&AuthHeader>,
    with_auth: bool,
    base: &str,
    start: f64,
    budget: f64,
    requests: &mut usize,
) -> HttpResponse {
    if req.method != "GET" {
        return HttpResponse { error: Some("읽기는 GET 만 보냅니다".to_string()), ..Default::default() };
    }
    if with_auth && !same_site(&req.url, base) {
        return HttpResponse { error: Some("연결한 사이트 밖 주소라 보내지 않았습니다".to_string()), ..Default::default() };
    }
    let mut attempt = 0i32;
    loop {
        *requests += 1;
        let r = http.send(req, if with_auth { header } else { None });
        let Some(w) = retry_wait(r.status, r.headers.get("retry-after").map(String::as_str), attempt, clock.now()) else {
            return r;
        };
        if clock.now() + w > start + budget {
            return r;
        }
        waiter.wait(w);
        attempt += 1;
    }
}

/// 링크 하나를 내장 REST 로 읽는다. 연결이 없거나 읽기 범위·토큰이 없거나 만료면 요청 0. 요청은 연결의 바탕 주소 아래로만
/// (apSameSite — GET 만). 메타는 상한 안으로 자르고 비밀을 가린 뒤(redacted) 형식 검사(apMetaCheck)를 지난 것만 돌려준다.
/// 원본: `apReadLink`
#[allow(clippy::too_many_arguments)]
pub fn read_link(
    link: &str,
    want: &[String],
    prompt: &str,
    connections: &[Connection],
    dev: bool,
    secrets: &dyn ConnSecrets,
    http: &dyn ConnHttp,
    clock: &dyn ReadClock,
    waiter: &dyn ReadWaiter,
    tz: Tz,
    budget: f64,
) -> ReadResult {
    let mut res = ReadResult::default();
    let site = connections.iter().find(|c| c.service == Service::Atlassian).and_then(|c| service_base(c, dev));
    let target = read_target(link, site.as_deref());
    res.target = target.clone();
    match &target {
        ReadTarget::Bad(why) => {
            res.note = why.clone();
            return res;
        }
        ReadTarget::Web => {
            res.note = "내장하지 않은 주소입니다. MCP 서버를 골라 읽습니다".to_string();
            return res;
        }
        _ => {}
    }
    let Some(svc) = target.service() else { return res };
    let Some(c) = connections.iter().find(|c| c.service == svc) else {
        res.note = format!("{} 연결이 없어 요청하지 않았습니다(설정 › 연결)", svc.label());
        return res;
    };
    if let Some(why) = read_blocker(svc, Some(c), secrets.exists(&c.id), dev, clock.now(), tz) {
        res.note = format!("{}. 요청하지 않았습니다", why);
        return res;
    }
    // blocker 가 사이트 주소를 이미 봤으므로 여기엔 항상 온다
    let Some(base) = service_base(c, dev) else { return res };
    if svc == Service::Atlassian && !same_site(link, &base) {
        res.note = "연결한 Atlassian 사이트와 다른 사이트의 주소라 요청하지 않았습니다".to_string();
        return res;
    }
    let Some(token) = secrets.read(&c.id).filter(|t| !t.is_empty()) else {
        res.note = format!("{} 토큰이 키체인에 없어 요청하지 않았습니다", svc.label());
        return res;
    };
    res.connection = c.id.clone();
    let auth = auth_for(c);
    let header = auth_header(&auth, &token);
    let start = clock.now();
    let meta;
    match &target {
        ReadTarget::Jira { key } => {
            let r = read_send(http, clock, waiter, &jira_issue_request(key, &base, auth_for(c), JIRA_READ_FIELDS), header.as_ref(), true, &base, start, budget, &mut res.requests);
            let Some(o) = read_json(&r) else {
                res.note = if r.status == 200 { "읽을 수 없음: 응답 꼴이 예상과 다릅니다".to_string() } else { read_fail_note(&r) };
                return res;
            };
            meta = meta_from_jira(&o, key, &base, want, prompt, dev);
        }
        ReadTarget::Confluence { page } => {
            let r = read_send(http, clock, waiter, &confluence_read_request(page, &base, auth_for(c)), header.as_ref(), true, &base, start, budget, &mut res.requests);
            let Some(o) = read_json(&r) else {
                res.note = if r.status == 200 { "읽을 수 없음: 응답 꼴이 예상과 다릅니다".to_string() } else { read_fail_note(&r) };
                return res;
            };
            meta = meta_from_confluence(&o, page, &base, want, prompt, dev);
        }
        ReadTarget::Notion { page } => {
            let r = read_send(http, clock, waiter, &notion_page_get_request(page, &base), header.as_ref(), true, &base, start, budget, &mut res.requests);
            let Some(o) = read_json(&r) else {
                res.note = if r.status == 404 {
                    "읽을 수 없음: 찾지 못했습니다(404). Notion 은 통합을 그 페이지에 초대해야 읽힙니다(데이터베이스 주소는 아직 읽지 않습니다)".to_string()
                } else {
                    read_fail_note(&r)
                };
                return res;
            };
            let mut blocks: Vec<Map<String, Value>> = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..5 {
                // 한 번에 100블록, 다섯 번(500블록)까지. 안쪽 블록(has_children)은 내려가지 않는다
                let rb = read_send(http, clock, waiter, &notion_children_request(page, &base, cursor.as_deref()), header.as_ref(), true, &base, start, budget, &mut res.requests);
                let Some(ob) = read_json(&rb) else {
                    res.note = format!("본문 블록을 읽지 못했습니다: {}", read_fail_note(&rb));
                    return res;
                };
                blocks.extend(arr(ob.get("results")).into_iter().cloned());
                let next = ob.get("next_cursor").and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_string);
                if !(ob.get("has_more").and_then(Value::as_bool) == Some(true) && next.is_some()) {
                    break;
                }
                cursor = next;
            }
            let refs: Vec<&Map<String, Value>> = blocks.iter().collect();
            meta = meta_from_notion(&o, &refs, page, link, want, prompt, dev);
        }
        ReadTarget::Figma { file, node } => {
            let r = read_send(http, clock, waiter, &figma_read_request(file, node.as_deref(), &base), header.as_ref(), true, &base, start, budget, &mut res.requests);
            let Some(o) = read_json(&r) else {
                res.note = if r.status == 200 { "읽을 수 없음: 응답 꼴이 예상과 다릅니다".to_string() } else { read_fail_note(&r) };
                return res;
            };
            if let Some(nd) = node {
                let f = dict(o.get("nodes")).get(nd);
                if f.is_none() || f.is_some_and(Value::is_null) {
                    res.note = format!("읽을 수 없음: 파일에 그 프레임({})이 없습니다", nd);
                    return res;
                }
            }
            let (m, frame) = meta_from_figma(&o, file, node.as_deref(), link, dev);
            meta = m;
            // 프레임 그림 한 장 — /v1/images 가 내보낸 그림 주소(다른 호스트)를 돌려준다. 허용 목록 안일 때만, 인증 머리 없이 받는다
            if let Some(frame) = frame {
                let ri = read_send(http, clock, waiter, &figma_image_request(file, &frame, &base), header.as_ref(), true, &base, start, budget, &mut res.requests);
                let images = read_json(&ri).and_then(|j| j.get("images").cloned());
                let url = sv(dict(images.as_ref()).get(&frame));
                if url.is_empty() {
                    res.image_note = if ri.status == 200 {
                        "그림 주소를 받지 못했습니다".to_string()
                    } else {
                        format!("그림 주소를 받지 못했습니다({})", read_fail_note(&ri))
                    };
                } else if !image_url_ok(&url, dev) {
                    res.image_note = format!("그림 주소가 허용한 호스트 밖이라 받지 않았습니다({})", url_host(&url));
                } else {
                    let mut g = HttpRequest { url: url.clone(), ..Default::default() };
                    g.headers = BTreeMap::from([("Accept".to_string(), "image/png".to_string())]);
                    let rg = read_send(http, clock, waiter, &g, None, false, &base, start, budget, &mut res.requests);
                    if rg.error.is_none() && rg.status == 200 && rg.body.len() <= IMAGE_MAX_BYTES && is_png(&rg.body) {
                        res.image = Some(rg.body.clone());
                        res.image_host = url_host(&url);
                    } else {
                        res.image_note = if rg.status == 200 {
                            "그림이 PNG 가 아니거나 너무 큽니다".to_string()
                        } else {
                            format!("그림을 받지 못했습니다: {}", read_fail_note(&rg))
                        };
                    }
                }
            }
        }
        _ => return res,
    }
    let (red, n) = meta_redact(&meta);
    let json_value = serde_json::from_slice::<Value>(&meta_data(&red)).ok();
    let checked = meta_check(json_value.as_ref(), false);
    match checked.meta {
        None => {
            res.note = format!("형식 검사에 걸렸습니다: {}", checked.problems.iter().take(3).cloned().collect::<Vec<_>>().join(" · "));
            res
        }
        Some(ok) => {
            res.meta = Some(ok);
            res.redacted = n;
            res.note = "읽었습니다".to_string();
            res
        }
    }
}

// ── 에픽 제목·상태(출발 준비의 에픽 칸, 5-2c 의 읽기 몫 — 원본 Reading.swift 606~649줄) ──

/// 에픽 하나의 읽은 값. 원본: `APEpicInfo`(`type` 은 예약어라 `kind` 로 바꿨다)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EpicInfo {
    pub key: String,
    pub found: bool,
    pub is_epic: bool,
    pub title: String,
    pub status: String,
    /// Swift 원본의 `type`
    pub kind: String,
    pub note: String,
    pub requests: usize,
    /// 요청해서 답을 받았는가(연결이 없어 요청하지 않았으면 false)
    pub read: bool,
}

/// 에픽 판정 — 이슈 종류의 계층 수준이 1(에픽 자리)이면 에픽, 그 값이 없으면 종류 이름이 Epic·에픽.
/// 원본: `apIsEpicType`
pub fn is_epic_type(t: &Map<String, Value>) -> bool {
    if let Some(lv) = t.get("hierarchyLevel").and_then(Value::as_i64) {
        return lv == 1;
    }
    let n = sv(t.get("name")).to_lowercase();
    n == "epic" || n == "에픽"
}

/// 에픽 번호 하나를 읽어 제목·상태를 채운다. 원본: `apReadEpic`
pub fn read_epic(key: &str, connections: &[Connection], dev: bool, secrets: &dyn ConnSecrets, http: &dyn ConnHttp, clock: &dyn ReadClock, tz: Tz) -> EpicInfo {
    let mut e = EpicInfo { key: key.to_string(), ..Default::default() };
    if !issue_key_ok(key) {
        e.note = "에픽 번호 꼴이 아닙니다(예: SHOP-10)".to_string();
        return e;
    }
    let c = connections.iter().find(|c| c.service == Service::Atlassian);
    let has_secret = c.is_some_and(|x| secrets.exists(&x.id));
    if let Some(why) = read_blocker(Service::Atlassian, c, has_secret, dev, clock.now(), tz) {
        e.note = format!("{}. 번호 꼴만 봤습니다", why);
        return e;
    }
    let Some(c) = c else { return e };
    let Some(base) = service_base(c, dev) else { return e };
    let Some(token) = secrets.read(&c.id).filter(|t| !t.is_empty()) else {
        e.note = "Atlassian 토큰이 키체인에 없습니다. 번호 꼴만 봤습니다".to_string();
        return e;
    };
    let req = jira_issue_request(key, &base, auth_for(c), "summary,status,issuetype");
    if !same_site(&req.url, &base) {
        e.note = "사이트 주소가 규칙에 맞지 않습니다".to_string();
        return e;
    }
    e.requests = 1;
    let r = http.send(&req, auth_header(&auth_for(c), &token).as_ref());
    if r.status == 404 {
        e.read = true;
        e.note = format!("{} 이슈가 없습니다(404). 에픽 번호를 확인합니다", key);
        return e;
    }
    let Some(o) = read_json(&r) else {
        e.note = read_fail_note(&r);
        return e;
    };
    let f = dict(o.get("fields"));
    e.read = true;
    e.found = true;
    e.title = meta_text(&sv(f.get("summary")), 200);
    e.status = meta_text(&name_of(dict(f.get("status"))), 80);
    e.kind = meta_text(&name_of(dict(f.get("issuetype"))), 80);
    e.is_epic = is_epic_type(dict(f.get("issuetype")));
    e.note = if e.is_epic {
        "에픽입니다".to_string()
    } else {
        format!("{} 은 {}입니다. 에픽 번호를 넣어 주세요", key, if e.kind.is_empty() { "에픽이 아닌 이슈" } else { &e.kind })
    };
    e
}
