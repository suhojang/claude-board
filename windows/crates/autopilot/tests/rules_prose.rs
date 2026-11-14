//! 글 다듬기·이스케이프·링크 규칙(`rules::prose`) 시험.
//! 원본: `Tests/AutopilotRulesTests/ReportTests.swift` 의 `PlainTests`(표 14 줄) · `EscapeTests`(이스케이프 10 줄·링크 17 줄).
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 경계(줄 끝 글자·주소 쪼개기)를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::prose::{escape_html, plain, safe_link};

struct Row {
    note: &'static str,
    s: &'static str,
    want: &'static str,
}

const PLAIN_ROWS: [Row; 14] = [
    Row { note: "빈 글", s: "", want: "" },
    Row { note: "줄표 없음", s: "그냥 글", want: "그냥 글" },
    Row { note: "앞뒤 빈칸 있는 줄표", s: "A \u{2014} B", want: "A, B" },
    Row { note: "붙은 줄표", s: "A\u{2014}B", want: "A, B" },
    Row { note: "앞에만 빈칸", s: "A \u{2014}B", want: "A, B" },
    Row { note: "뒤에만 빈칸", s: "A\u{2014} B", want: "A, B" },
    Row { note: "줄 맨 앞 줄표는 지움", s: "\u{2014} 앞", want: "앞" },
    Row { note: "줄 끝 줄표는 지움", s: "끝 \u{2014}", want: "끝" },
    Row { note: "이어 쓴 줄표는 하나로", s: "A \u{2014}\u{2014} B", want: "A, B" },
    Row { note: "둘째 줄 맨 앞", s: "줄 하나\n\u{2014} 둘째 줄", want: "줄 하나\n둘째 줄" },
    Row { note: "여러 개", s: "x \u{2014} y \u{2014} z", want: "x, y, z" },
    Row { note: "줄표 하나뿐", s: "\u{2014}", want: "" },
    Row { note: "엔 대시(범위)는 그대로", s: "1\u{2013}3 단계", want: "1\u{2013}3 단계" },
    Row { note: "가로 막대(U+2015)는 그대로", s: "A \u{2015} B", want: "A \u{2015} B" },
];

#[test]
fn 표() {
    for r in &PLAIN_ROWS {
        assert_eq!(plain(r.s), r.want, "{} / 입력 {:?}", r.note, r.s);
    }
}

const ESCAPE_ROWS: [Row; 10] = [
    Row { note: "스크립트 태그", s: "<script>alert('x')</script>", want: "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;" },
    Row { note: "속성 탈출(큰따옴표)", s: "\" onmouseover=\"x", want: "&quot; onmouseover=&quot;x" },
    Row { note: "속성 탈출(작은따옴표)", s: "' onfocus='x", want: "&#39; onfocus=&#39;x" },
    Row { note: "& 는 먼저", s: "a & b &amp;", want: "a &amp; b &amp;amp;" },
    Row { note: "탭·줄바꿈은 그대로", s: "가\t나\n다", want: "가\t나\n다" },
    Row { note: "방향 바꾸기 글자는 지움", s: "\u{202E}txt.exe\u{2066}x\u{2069}", want: "txt.exex" },
    Row { note: "제어 문자·DEL·C1 은 지움", s: "\u{07}벨\u{7F}\u{85}끝", want: "벨끝" },
    Row { note: "CR 은 지우고 LF 만", s: "a\r\nb", want: "a\nb" },
    Row { note: "줄·문단 구분자는 줄바꿈", s: "a\u{2028}b\u{2029}c", want: "a\nb\nc" },
    Row { note: "그림 글자·한글은 그대로", s: "\u{1F44D} 좋음", want: "\u{1F44D} 좋음" },
];

#[test]
fn 이스케이프() {
    for r in &ESCAPE_ROWS {
        assert_eq!(escape_html(r.s), r.want, "{} / 입력 {:?}", r.note, r.s);
    }
}

struct LinkRow {
    note: &'static str,
    s: &'static str,
    want: Option<&'static str>,
}

const LINK_ROWS: [LinkRow; 17] = [
    LinkRow { note: "https 주소", s: "https://example.com/a?b=1&c=2#x", want: Some("https://example.com/a?b=1&c=2#x") },
    LinkRow { note: "앞뒤 빈칸은 다듬음", s: "  https://example.com/  ", want: Some("https://example.com/") },
    LinkRow { note: "대문자 꼴", s: "HTTPS://Example.com", want: Some("HTTPS://Example.com") },
    LinkRow { note: "http 는 글로만", s: "http://example.com", want: None },
    LinkRow { note: "javascript:", s: "javascript:alert(1)", want: None },
    LinkRow { note: "대소문자 섞은 javascript:", s: " JaVaScRiPt:alert(1)", want: None },
    LinkRow { note: "data:", s: "data:text/html,<script>alert(1)</script>", want: None },
    LinkRow { note: "file:", s: "file:///etc/passwd", want: None },
    LinkRow { note: "꾸러미 없는 상대 주소", s: "//example.com/x", want: None },
    LinkRow { note: "호스트 없음", s: "https://", want: None },
    LinkRow { note: "사용자 정보", s: "https://user:pw@example.com/", want: None },
    LinkRow { note: "빈칸", s: "https://exa mple.com/", want: None },
    LinkRow { note: "속성 탈출 따옴표", s: "https://example.com/\"onmouseover=\"x", want: None },
    LinkRow { note: "역슬래시", s: "https://example.com\\@evil.example/", want: None },
    LinkRow { note: "꺾쇠", s: "https://example.com/<x>", want: None },
    LinkRow { note: "ASCII 밖 글자", s: "https://예시.com/", want: None },
    LinkRow { note: "빈 글", s: "", want: None },
];

#[test]
fn 링크() {
    for r in &LINK_ROWS {
        assert_eq!(safe_link(r.s).as_deref(), r.want, "{} / 입력 {:?}", r.note, r.s);
    }
}

#[test]
fn 표_길이() {
    // 줄이 빠지면 실패한다(Swift 표: 14·10·17)
    assert_eq!((PLAIN_ROWS.len(), ESCAPE_ROWS.len(), LINK_ROWS.len()), (14, 10, 17));
}

// ── 추가: Swift 시험에 없는 경계 ──────────────────────

#[test]
fn 추가_줄_끝_글자와_줄표() {
    // 원본 정규식 `[ \t]*—+[ \t]*$` 의 `$` 는 글 끝의 줄 끝 글자(\r·\u{85}·\u{2028}…) 앞에서도 맞는다.
    // 줄은 "\n" 으로만 나누므로 CRLF 글의 줄은 "\r" 로 끝난다
    assert_eq!(plain("끝 \u{2014}\r\n다음"), "끝\r\n다음");
    assert_eq!(plain("끝 \u{2014}\u{2028}"), "끝\u{2028}");
    assert_eq!(plain("끝 \u{2014} \r"), "끝\r");
    // 줄 끝 글자가 둘이면 `$` 가 맞지 않아 쉼표 하나로 바뀐다
    assert_eq!(plain("끝 \u{2014}\r\r"), "끝, \r\r");
    // 줄 맨 앞 줄표 뒤가 줄 끝이면 앞 규칙이 지운다
    assert_eq!(plain("\u{2014}\r"), "\r");
    // 맨 앞은 지우고 중간은 쉼표, 맨 끝은 지운다
    assert_eq!(plain("\u{2014} a \u{2014} b \u{2014}"), "a, b");
    // 맨 앞 규칙은 한 번만 — 줄표가 빈칸으로 떨어져 이어지면 남은 것은 쉼표로
    assert_eq!(plain("\u{2014} \u{2014} x"), ", x");
    // 탭도 빈칸이다
    assert_eq!(plain("A\t\u{2014}\tB"), "A, B");
    // 줄표 없는 글은 그대로
    assert_eq!(plain("a\r\nb"), "a\r\nb");
}

#[test]
fn 추가_링크_주소_쪼개기() {
    let ok = |s: &str| assert_eq!(safe_link(s).as_deref(), Some(s), "{s}");
    let no = |s: &str| assert_eq!(safe_link(s), None, "{s}");
    ok("https://example.com");
    ok("https://example.com:8443/a/b");
    ok("https://example.com/a%20b?q=%41#f%2F");
    ok("https://[::1]/x");
    ok("https://[::1]:8080/x");
    ok("https://example.com?x=a@b");
    ok("https://example.com/a@b");
    no("https://example.com/%zz");
    no("https://example.com/%4");
    no("https://example.com/%z4");
    no("https://example.com/%4z");
    no("https://user@example.com/");
    no("https://example.com:abc/");
    no("https://:80/");
    no("https://@example.com/");
    no("https://example.com/[x]");
    no("https://[::1/x");
    no("https:///x");
    no("https://exam[ple.com/");
    // 빈칸 뿐이거나 2000 자를 넘으면 None
    no("   ");
    let long_ok = format!("https://example.com/{}", "a".repeat(2000 - "https://example.com/".len()));
    assert_eq!(safe_link(&long_ok).as_deref(), Some(long_ok.as_str()));
    no(&format!("{long_ok}a"));
    // 줄바꿈·탭도 앞뒤에서는 다듬는다
    assert_eq!(safe_link("\t\n https://example.com/ \r\n").as_deref(), Some("https://example.com/"));
    // 가운데 빈칸·줄바꿈은 거부
    no("https://example.com/a\nb");
}

#[test]
fn 추가_이스케이프_경계() {
    // 경계값: 0x1F 는 지우고 0x20 은 둔다, 0x7E 는 두고 0x7F~0x9F 는 지우고 0xA0 은 둔다
    assert_eq!(escape_html("\u{1F}a \u{7E}\u{7F}\u{9F}\u{A0}"), "a ~\u{A0}");
    // 방향 글자 범위 끝 — 202A~202E·2066~2069 는 지우고 202F·2065·206A 는 둔다
    assert_eq!(escape_html("\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{202F}\u{2065}\u{2066}\u{2067}\u{2068}\u{2069}\u{206A}"), "\u{202F}\u{2065}\u{206A}");
    // 문자 참조 안의 ; 는 그대로, 한 번만 바꾼다
    assert_eq!(escape_html("&#39;"), "&amp;#39;");
}
