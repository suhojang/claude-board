//! 자료 링크 줄 규칙(`rules::links`) 시험.
//! 원본: `Tests/AutopilotRulesTests/TopicTests.swift` 의 `LinkTests`(표 12 줄·`더하기` 1개). `LinksTests.swift` 는 없다.
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 경계(비밀번호 가리기·꼴 거절·Windows 경로)를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::links::{link_add_problem, link_info, ApLink};

const TABLE: [(&str, &str); 12] = [
    ("https://www.figma.com/design/Rm8k2Tq/room?node-id=12-4", "Figma needs_connection"),
    ("  https://example.atlassian.net/wiki/spaces/OPS/pages/48213  ", "Confluence needs_connection"),
    ("https://example.atlassian.net/browse/SHOP-12", "Jira needs_connection"),
    ("https://www.notion.so/team/abc123", "Notion needs_connection"),
    ("https://team.notion.site/page", "Notion needs_connection"),
    ("https://docs.example.com/policy", "웹 needs_connection"),
    ("http://intranet.example.com/a", "웹 needs_connection"),
    ("figma.com/design/x", "웹 unreadable"),
    ("ftp://files.example.com/a", "웹 unreadable"),
    ("https://localhost/a", "웹 unreadable"),
    ("회의실 정책 문서", "웹 unreadable"),
    ("https://u:secret@example.com/a", "웹 unreadable"),
];

#[test]
fn 표() {
    for (input, want) in TABLE {
        let l = link_info(input);
        assert_eq!(format!("{} {}", l.kind, l.state), want, "입력 {input:?}");
        assert!(!l.url.contains("secret"), "입력 {input:?} -> {:?}", l.url);
    }
}

#[test]
fn 더하기() {
    let a = link_info("https://www.figma.com/design/x");
    assert_eq!(link_add_problem(&a, &[]), None);
    assert_eq!(link_add_problem(&a, std::slice::from_ref(&a)), Some("이미 있는 주소입니다"));
    assert_eq!(link_add_problem(&link_info(""), &[]), Some("주소를 붙여 넣어 주세요"));
    let many: Vec<ApLink> = (0..20).map(|i| link_info(&format!("https://example.com/{i}"))).collect();
    assert_eq!(link_add_problem(&link_info("https://example.com/x"), &many), Some("자료 링크는 20줄까지입니다"));
}

// ---- 추가: Swift 시험에 없는 경계 ----

#[test]
fn 추가_줄_내용_그대로() {
    let l = link_info("  https://example.atlassian.net/wiki/spaces/OPS/pages/48213  ");
    assert_eq!(l.url, "https://example.atlassian.net/wiki/spaces/OPS/pages/48213");
    assert_eq!(l.id(), l.url);
    assert_eq!(l.why, "Confluence 연결(설정 › 연결, 4단계) 뒤에 메타 정보만 읽습니다");
    let w = link_info("https://docs.example.com/policy");
    assert_eq!(w.why, "그 밖 웹 주소는 사용자가 등록한 MCP 읽기 세션으로 읽습니다(바깥 연결, 4단계)");
    let bad = link_info("회의실 정책 문서");
    assert_eq!(bad.url, "회의실 정책 문서");
    assert_eq!(bad.why, "웹 주소 꼴이 아닙니다(https:// 로 시작하는 주소를 붙여 넣습니다)");
}

#[test]
fn 추가_비밀번호_가리기() {
    let l = link_info("https://u:secret@example.com/a");
    assert_eq!(l.url, "https://u@example.com/a");
    assert_eq!(l.why, "사용자 이름·비밀번호가 든 주소는 받지 않습니다");
    // 비밀번호 없이 사용자 이름만: 주소는 그대로
    let u = link_info("https://user@example.com/a");
    assert_eq!((u.kind.as_str(), u.state.as_str(), u.url.as_str()), ("웹", "unreadable", "https://user@example.com/a"));
}

#[test]
fn 추가_종류_경계() {
    let k = |s: &str| link_info(s).kind;
    assert_eq!(k("https://figma.com/file/x"), "Figma");
    assert_eq!(k("https://notfigma.com/file/x"), "웹");
    assert_eq!(k("https://notion.so/x"), "Notion");
    assert_eq!(k("https://HOST.Atlassian.NET/WIKI/x"), "Confluence");
    assert_eq!(k("https://x.atlassian.net/jira/software"), "Jira");
    assert_eq!(k("https://x.atlassian.net/other"), "웹");
    assert_eq!(k("https://x.atlassian.net"), "웹");
    assert_eq!(k("HTTPS://WWW.FIGMA.COM/x"), "Figma");
    // 경로의 퍼센트 부호는 풀어서 본다(URLComponents.path 와 같다)
    assert_eq!(k("https://x.atlassian.net/%77iki/x"), "Confluence");
    // 포트·질의·조각
    assert_eq!(k("https://x.figma.com:8443/a?b=c#d"), "Figma");
}

#[test]
fn 추가_꼴이_틀린_주소() {
    for s in [
        "",
        "   ",
        "https://",
        "https:///a",
        "https:example.com/a",
        "https://example.com:abc/a",
        "https://exa mple.com/a",
        "https://example.com/한글",
        "https://example.com/%zz",
        "https://example.com/%4",
        "https://a@b@example.com/",
        "https://example.com/a#b#c",
        "https://example.com/<x>",
        "1http://example.com/a",
        "mailto:a@example.com",
        "https://nodot/a",
    ] {
        let l = link_info(s);
        assert_eq!((l.kind.as_str(), l.state.as_str()), ("웹", "unreadable"), "{s:?}");
        assert_eq!(l.url, s.trim(), "{s:?}");
    }
}

#[test]
fn 추가_windows_경로와_파일_주소는_받지_않는다() {
    for s in [r"C:\Users\u\doc.txt", r"\\server\share\doc", "file:///C:/Users/u/doc.txt", r"C:/Users/u/doc.txt"] {
        let l = link_info(s);
        assert_eq!((l.kind.as_str(), l.state.as_str()), ("웹", "unreadable"), "{s:?}");
    }
}

#[test]
fn 추가_ip_주소와_점이_든_호스트() {
    assert_eq!(link_info("http://192.168.0.1/a").state, "needs_connection");
    assert_eq!(link_info("http://[::1]/a").state, "unreadable");
    assert_eq!(link_info("http://a.b/").state, "needs_connection");
}

#[test]
fn 추가_더하기_경계() {
    let one: Vec<ApLink> = (0..19).map(|i| link_info(&format!("https://example.com/{i}"))).collect();
    let next = link_info("https://example.com/new");
    assert_eq!(link_add_problem(&next, &one), None);
    // 20줄이고 같은 주소면 "이미 있는" 쪽이 먼저
    let mut full = one.clone();
    full.push(next.clone());
    assert_eq!(link_add_problem(&next, &full), Some("이미 있는 주소입니다"));
    // 빈 주소가 가장 먼저
    assert_eq!(link_add_problem(&link_info(""), &full), Some("주소를 붙여 넣어 주세요"));
    // 꼴이 틀린 줄도 같은 글이면 이미 있는 주소다
    let bad = link_info("메모");
    assert_eq!(link_add_problem(&bad, std::slice::from_ref(&bad)), Some("이미 있는 주소입니다"));
}
