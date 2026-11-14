//! 모델이 쓴 글 다듬기와 HTML 로 옮길 때의 이스케이프. 원본: `Sources/AutopilotRules/Prose.swift`.
//! 보드 화면과 보고서 틀이 함께 쓴다. 순수 함수만 있다(파일·시각·환경 없음).
//! 원본의 `apPlain`·`apEscapeHTML`·`apSafeLink` 가 `plain`·`escape_html`·`safe_link` 이다.
//! 정규식 크레이트 없이 원본 정규식 세 개와 같은 뜻을 손으로 옮겼다(ICU `$` 의 줄 끝 규칙 포함).

const EM_DASH: char = '\u{2014}';

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

// ICU 정규식 `$`(여러 줄 아님)가 글 맨 끝 앞에서도 맞는 줄 끝 글자: \n \u{b} \u{c} \r \u{85} \u{2028} \u{2029}
fn is_icu_line_end(c: char) -> bool {
    matches!(c, '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

// `^[ \t]*—+[ \t]*` 를 지운다
fn strip_leading(l: &str) -> &str {
    let rest = l.trim_start_matches(is_blank);
    let after = rest.trim_start_matches(EM_DASH);
    if after.len() == rest.len() {
        return l;
    }
    after.trim_start_matches(is_blank)
}

// `[ \t]*—+[ \t]*$` 를 지운다. `$` 는 글 끝이거나 글 끝의 줄 끝 글자(\r\n 은 한 덩어리) 앞이다
fn strip_trailing(l: &str) -> String {
    let body_end = if let Some(b) = l.strip_suffix("\r\n") {
        b.len()
    } else if l.chars().next_back().is_some_and(is_icu_line_end) {
        l.len() - l.chars().next_back().map_or(0, char::len_utf8)
    } else {
        l.len()
    };
    let body = &l[..body_end];
    let no_tail = body.trim_end_matches(is_blank);
    let no_dash = no_tail.trim_end_matches(EM_DASH);
    if no_dash.len() == no_tail.len() {
        return l.to_string();
    }
    let start = no_dash.trim_end_matches(is_blank).len();
    format!("{}{}", &l[..start], &l[body_end..])
}

// `[ \t]*—+[ \t]*` 를 ", " 하나로. 왼쪽부터 겹치지 않게 가장 길게
fn replace_runs(l: &str) -> String {
    let cs: Vec<char> = l.chars().collect();
    let mut out = String::with_capacity(l.len());
    let mut i = 0;
    while i < cs.len() {
        if is_blank(cs[i]) || cs[i] == EM_DASH {
            let mut j = i;
            while j < cs.len() && is_blank(cs[j]) {
                j += 1;
            }
            let d = j;
            while j < cs.len() && cs[j] == EM_DASH {
                j += 1;
            }
            if j > d {
                while j < cs.len() && is_blank(cs[j]) {
                    j += 1;
                }
                out.push_str(", ");
                i = j;
                continue;
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

/// 모델이 쓴 글의 긴 줄표는 그릴 때 쉼표로 바꾼다. 줄마다 줄 맨 앞의 줄표와 줄 끝의 줄표는 지우고,
/// 그 밖은 앞뒤 빈칸과 함께 ", " 하나로(이어 쓴 줄표도 하나로). 원본: `apPlain`
pub fn plain(s: &str) -> String {
    if !s.contains(EM_DASH) {
        return s.to_string();
    }
    s.split('\n')
        .map(|line| {
            let l = strip_trailing(strip_leading(line));
            replace_runs(&l)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// HTML 글·속성 값 이스케이프 — & < > " ' 를 문자 참조로. 제어 문자(탭·줄바꿈 말고)·DEL·C1·방향 바꾸기 글자는 지우고,
/// 줄·문단 구분자(U+2028·2029)는 줄바꿈으로 바꾼다. 원본: `apEscapeHTML`
pub fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 16);
    for u in s.chars() {
        match u {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            '\n' | '\t' => o.push(u),
            '\u{2028}' | '\u{2029}' => o.push('\n'),
            _ => {
                let v = u as u32;
                if v < 0x20 || (0x7F..=0x9F).contains(&v) || (0x202A..=0x202E).contains(&v) || (0x2066..=0x2069).contains(&v) {
                    continue;
                }
                o.push(u);
            }
        }
    }
    o
}

// RFC 3986 의 글자 묶음. `%` 는 뒤에 16진수 둘이 있어야 한다
fn is_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}

fn is_sub_delim(c: char) -> bool {
    matches!(c, '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '=')
}

fn pct_ok(s: &str, allowed: impl Fn(char) -> bool) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c == '%' {
            if !(b.get(i + 1).is_some_and(u8::is_ascii_hexdigit) && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit)) {
                return false;
            }
            i += 3;
        } else if allowed(c) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

// 호스트(또는 `[IPv6]`)와 포트 — `URLComponents` 가 host 를 비어 있지 않게 내는 꼴만
fn host_port_ok(auth: &str) -> bool {
    let port = if let Some(rest) = auth.strip_prefix('[') {
        let Some(end) = rest.find(']') else { return false };
        let inner = &rest[..end];
        if inner.is_empty() || !inner.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') {
            return false;
        }
        match &rest[end + 1..] {
            "" => None,
            tail => match tail.strip_prefix(':') {
                Some(p) => Some(p),
                None => return false,
            },
        }
    } else {
        let (host, port) = match auth.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (auth, None),
        };
        if host.is_empty() || !pct_ok(host, |c| is_unreserved(c) || is_sub_delim(c)) {
            return false;
        }
        port
    };
    port.is_none_or(|p| p.bytes().all(|b| b.is_ascii_digit()))
}

/// 보고서에 링크로 걸 주소인가 — https 만, 호스트가 있고, 사용자 정보가 없고, 빈칸·따옴표·꺾쇠·역슬래시 같은
/// 주소에 올 수 없는 글자가 없을 때만 그 글자 그대로 돌려준다(아니면 None). 원본: `apSafeLink`
pub fn safe_link(s: &str) -> Option<String> {
    let t = s.trim_matches(char::is_whitespace);
    if t.is_empty() || t.chars().count() > 2000 {
        return None;
    }
    let bad = ['"', '\'', '<', '>', '\\', '`', '{', '}', '|', '^'];
    if !t.chars().all(|c| (c as u32) > 0x20 && (c as u32) < 0x7F && !bad.contains(&c)) {
        return None;
    }
    if !t.to_lowercase().starts_with("https://") {
        return None;
    }
    // URLComponents(string:) 와 같은 RFC 3986 쪼개기: 꾸러미 // 권한 / 경로 ? 질의 # 조각
    let rest = &t["https://".len()..];
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
    if auth.contains('@') || !host_port_ok(auth) {
        return None;
    }
    let pchar = |c: char| is_unreserved(c) || is_sub_delim(c) || c == ':' || c == '@';
    if !pct_ok(path, |c| pchar(c) || c == '/') {
        return None;
    }
    if let Some(q) = query {
        if !pct_ok(q, |c| pchar(c) || c == '/' || c == '?') {
            return None;
        }
    }
    if let Some(f) = frag {
        if !pct_ok(f, |c| pchar(c) || c == '/' || c == '?') {
            return None;
        }
    }
    Some(t.to_string())
}
