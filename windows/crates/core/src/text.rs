//! Python 문자열 규칙 흉내 — `" ".join(s.split())`, `s[:n]`, JSON 값의 참·거짓.

use serde_json::Value;

/// Python `str.isspace()` 와 같은 공백 판정. Rust `char::is_whitespace` 는 `\x1c`-`\x1f` 를 공백으로 보지 않는다.
pub fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// `" ".join(s.split())` — 공백 묶음을 한 칸으로 줄이고 앞뒤 공백을 없앤다.
pub fn collapse_ws(s: &str) -> String {
    s.split(is_py_space).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// `s[:n]` — 코드포인트 기준 앞 n 글자.
pub fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// 앞의 UTF-8 BOM(EF BB BF) 한 개를 뗀다. 가운데·뒤의 BOM 바이트는 건드리지 않는다.
/// Windows 전용 관용이다 — PowerShell 5.1 의 `|` 등이 stdin 앞에 BOM 을 붙이는데 python 의 `json.load` 는 BOM 을 거부한다.
pub fn strip_utf8_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
}

/// Python 참·거짓 — None·null·false·0·빈 문자열·빈 목록·빈 객체는 거짓.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// 비어 있지 않은 문자열 값만 꺼낸다(`data.get(k) or ...` 에서 문자열만 쓰는 곳).
pub fn non_empty_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collapse_like_python_split() {
        assert_eq!(collapse_ws("  윈도우 포팅   작업을\n시작 "), "윈도우 포팅 작업을 시작");
        assert_eq!(collapse_ws("a\x1cb\u{3000}c\u{a0}d"), "a b c d");
        assert_eq!(collapse_ws(""), "");
    }

    #[test]
    fn take_counts_code_points() {
        assert_eq!(take_chars("가나다라", 2), "가나");
        assert_eq!(take_chars("ab", 5), "ab");
    }

    #[test]
    fn bom_is_stripped_only_at_the_front() {
        assert_eq!(strip_utf8_bom(b"\xEF\xBB\xBF{\"a\":1}"), b"{\"a\":1}");
        assert_eq!(strip_utf8_bom(b"{\"a\":1}"), b"{\"a\":1}");
        assert_eq!(strip_utf8_bom(b""), b"");
        assert_eq!(strip_utf8_bom(b"\xEF\xBB\xBF"), b"");
        // 가운데의 BOM 은 그대로, 앞의 것은 한 개만
        assert_eq!(strip_utf8_bom(b"a\xEF\xBB\xBFb"), b"a\xEF\xBB\xBFb");
        assert_eq!(strip_utf8_bom(b"\xEF\xBB\xBF\xEF\xBB\xBFx"), b"\xEF\xBB\xBFx");
        // 일부만 맞는 앞머리는 BOM 이 아니다
        assert_eq!(strip_utf8_bom(b"\xEF\xBB{"), b"\xEF\xBB{");
    }

    #[test]
    fn truthiness() {
        assert!(!truthy(None));
        assert!(!truthy(Some(&json!(null))));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!([]))));
        assert!(truthy(Some(&json!(0.5))));
        assert!(truthy(Some(&json!("x"))));
        assert!(truthy(Some(&json!({"a": 1}))));
    }
}
