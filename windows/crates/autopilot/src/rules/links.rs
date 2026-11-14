//! 주제로 시작의 자료 링크 줄 규칙. 원본: `Sources/AutopilotRules/Links.swift`.
//! 붙여 넣은 주소 하나의 종류(Figma·Confluence·Notion·Jira·웹)와 읽기 상태를 정하고, 줄 목록에 더해도 되는지 본다.
//! 링크 읽기는 바깥 연결과 함께라 여기서는 주소 꼴과 종류만 본다(읽지 않는다) — 꼴이 맞는 주소는 모두 `needs_connection`.
//! 원본의 `apLinkInfo` 가 `link_info`, `apLinkAddProblem` 이 `link_add_problem`, `APLink` 가 `ApLink` 다.
//!
//! 원본은 Foundation 의 `URLComponents(string:)` 로 주소를 쪼갠다. 여기는 같은 RFC 3986 쪼개기(꾸러미 `://` 권한 경로 `?` 질의 `#` 조각)를
//! 손으로 쓴다. `prose::safe_link` 는 https 만·ASCII 만·글자 목록 거름이라 이 규칙(http 도 받음)과 맞지 않아 쓰지 않는다.
//! 같은 점: 주소에 올 수 없는 글자(빈칸·한글·꺾쇠·따옴표 `"`·역슬래시…)나 `%` 뒤 16진수 두 자리가 아닌 글은 주소로 보지 않는다(원본은 nil).
//! 같은 점: `host`·`password`·`path` 는 퍼센트 부호를 풀어 쓴다(Foundation 의 getter 와 같다).

/// 자료 링크 한 줄. 원본: `APLink`. `kind` 는 Figma·Confluence·Notion·Jira·웹, `state` 는 needs_connection·unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApLink {
    pub url: String,
    pub kind: String,
    pub state: String,
    pub why: String,
}

impl ApLink {
    pub fn new(url: &str, kind: &str, state: &str, why: &str) -> Self {
        Self { url: url.to_string(), kind: kind.to_string(), state: state.to_string(), why: why.to_string() }
    }

    /// 원본의 `id`(주소 자체)
    pub fn id(&self) -> &str {
        &self.url
    }
}

/// `URLComponents` 로 쪼갠 결과 가운데 이 규칙이 쓰는 것. 이 꼴이 안 되면 원본은 nil(또는 http 가 아니거나 호스트 없음)이다.
struct Parts {
    user: Option<String>,
    password: Option<String>,
    host: String,
    path: String,
}

fn is_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}

fn is_sub_delim(c: char) -> bool {
    matches!(c, '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '=')
}

/// `allowed` 글자이거나 `%` + 16진수 두 자리로만 이루어졌는가
fn pct_ok(s: &str, allowed: impl Fn(char) -> bool) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c == '%' {
            if i + 2 >= b.len() || !(b[i + 1] as char).is_ascii_hexdigit() || !(b[i + 2] as char).is_ascii_hexdigit() {
                return false;
            }
            i += 3;
        } else {
            if !b[i].is_ascii() || !allowed(c) {
                return false;
            }
            i += 1;
        }
    }
    true
}

/// 퍼센트 부호를 푼다(`pct_ok` 를 통과한 글에만 부른다). UTF-8 이 아닌 바이트는 U+FFFD 로 바뀐다.
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `URLComponents(string:)` 쪼개기. 꾸러미가 없거나 `//` 권한이 없거나 호스트가 비면 None(원본은 그 경우도 "웹 주소 꼴이 아닙니다").
fn split_url(s: &str) -> Option<(String, Parts)> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    let mut sc = scheme.chars();
    if !sc.next().is_some_and(|c| c.is_ascii_alphabetic()) || !sc.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let rest = s[colon + 1..].strip_prefix("//")?;
    let (before_frag, frag) = match rest.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (rest, None),
    };
    let (before_query, query) = match before_frag.split_once('?') {
        Some((a, q)) => (a, Some(q)),
        None => (before_frag, None),
    };
    let (auth, path) = match before_query.find('/') {
        Some(i) => (&before_query[..i], &before_query[i..]),
        None => (before_query, ""),
    };
    let pchar = |c: char| is_unreserved(c) || is_sub_delim(c) || c == ':' || c == '@';
    if !pct_ok(path, |c| pchar(c) || c == '/') {
        return None;
    }
    if query.is_some_and(|q| !pct_ok(q, |c| pchar(c) || c == '/' || c == '?')) {
        return None;
    }
    if frag.is_some_and(|f| !pct_ok(f, |c| pchar(c) || c == '/' || c == '?')) {
        return None;
    }
    // 권한 = [사용자 정보 "@"] 호스트 [":" 포트]. `@` 가 둘 이상이면 꼴이 틀린 것
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
            if !pct_ok(u, |c| is_unreserved(c) || is_sub_delim(c) || c == ':') {
                return None;
            }
            match u.split_once(':') {
                Some((n, p)) => (Some(pct_decode(n)), Some(pct_decode(p))),
                None => (Some(pct_decode(u)), None),
            }
        }
    };
    let (host_raw, port) = if let Some(after) = hostport.strip_prefix('[') {
        let end = after.find(']')?;
        let lit = &after[..end];
        if lit.is_empty() || !lit.chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '.') {
            return None;
        }
        let tail = &after[end + 1..];
        let port = match tail {
            "" => None,
            t => Some(t.strip_prefix(':')?),
        };
        (lit.to_string(), port)
    } else {
        let (h, p) = match hostport.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (hostport, None),
        };
        if !pct_ok(h, |c| is_unreserved(c) || is_sub_delim(c)) {
            return None;
        }
        (pct_decode(h), p)
    };
    if port.is_some_and(|p| !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some((scheme.to_string(), Parts { user, password, host: host_raw, path: pct_decode(path) }))
}

const NOT_URL: &str = "웹 주소 꼴이 아닙니다(https:// 로 시작하는 주소를 붙여 넣습니다)";

/// 붙여 넣은 주소 하나 — 앞뒤 빈칸을 자르고, http(s) 와 호스트(점이 든 것)가 있는 주소만 받는다. 종류는 호스트와 경로로.
/// 사용자·비밀번호가 든 주소(`https://u:p@…`)는 받지 않는다(근거 묶음과 화면에 비밀이 남지 않게): `unreadable` 로 하고
/// 주소에서 `:<비밀번호>@` 를 `@` 로 바꾼다. 원본: `apLinkInfo`
pub fn link_info(raw: &str) -> ApLink {
    let s = raw.trim_matches(char::is_whitespace);
    let parsed = split_url(s).filter(|(scheme, _)| {
        let l = scheme.to_lowercase();
        l == "http" || l == "https"
    });
    let Some((_, u)) = parsed else {
        return ApLink::new(s, "웹", "unreadable", NOT_URL);
    };
    let host = u.host.to_lowercase();
    if host.is_empty() || !host.contains('.') {
        return ApLink::new(s, "웹", "unreadable", NOT_URL);
    }
    if u.user.is_some() || u.password.is_some() {
        let needle = format!("{}@", u.password.as_ref().map(|p| format!(":{p}")).unwrap_or_default());
        return ApLink::new(&s.replace(&needle, "@"), "웹", "unreadable", "사용자 이름·비밀번호가 든 주소는 받지 않습니다");
    }
    let path = u.path.to_lowercase();
    let kind = if host == "figma.com" || host.ends_with(".figma.com") {
        "Figma"
    } else if host.ends_with(".atlassian.net") {
        if path.starts_with("/wiki") {
            "Confluence"
        } else if path.starts_with("/browse") || path.starts_with("/jira") {
            "Jira"
        } else {
            "웹"
        }
    } else if host == "notion.so" || host.ends_with(".notion.so") || host.ends_with(".notion.site") {
        "Notion"
    } else {
        "웹"
    };
    let why = if kind == "웹" {
        "그 밖 웹 주소는 사용자가 등록한 MCP 읽기 세션으로 읽습니다(바깥 연결, 4단계)".to_string()
    } else {
        format!("{kind} 연결(설정 › 연결, 4단계) 뒤에 메타 정보만 읽습니다")
    };
    ApLink::new(s, kind, "needs_connection", &why)
}

/// 줄 목록에 더해도 되는가 — 같은 주소는 한 번만, 20줄까지. 되면 None. 원본: `apLinkAddProblem(_:to:)`
pub fn link_add_problem(link: &ApLink, list: &[ApLink]) -> Option<&'static str> {
    if link.url.is_empty() {
        return Some("주소를 붙여 넣어 주세요");
    }
    if list.iter().any(|l| l.url == link.url) {
        return Some("이미 있는 주소입니다");
    }
    if list.len() >= 20 {
        return Some("자료 링크는 20줄까지입니다");
    }
    None
}
