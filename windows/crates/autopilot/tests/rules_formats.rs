//! 올릴 글의 형식 변환(`rules::formats`) 시험.
//! 원본: `Tests/AutopilotRulesTests/PublishTests.swift` 의 `FormatTests`(`@Suite("자율주행 올릴 글: 형식 변환")`, Connections·Outbox 시험과 한 파일).
//! `FormatTests` 의 시험 8개 가운데 이 파일이 옮긴 것은 `다듬기`(글 다듬기, 표 5줄)·`노션글`·`묶음`·`상한`(문서 상한) 넷이다. 같은 이름의 Rust 시험이다.
//! 남은 넷은 Report·Outbox 이식이 먼저 와야 옮길 수 있다:
//!   `골든`·`표지`·`악성`·`왕복` — `item(…)`·`apReportDoc`·`run(…)`·`apOutboxParse` 와 골든 파일(`Tests/AutopilotRulesTests/Publish/*`)이 필요하다
//!   (`악성` 의 `apStorage`·`apADF`·`apNotionBlocks` 단언은 이 파일의 `extra_` 시험이 손으로 만든 문서로 미리 덮는다).
//! 표(`DOC_TEXT_ROWS`)는 Swift 시험 파일에서 기계로 옮긴 것이다(손으로 다시 치지 않았다): 문자열 이스케이프(`\n`·`\t`·`\u{…}`)는 Rust 와 같아 글자 그대로.
//! 끝의 `extra_` 시험은 Swift 시험에 없는 것(덩어리 세기·링크 걸러내기·ADF·저장 형식·Notion 블록·파일 칸 왕복 직접 호출)이다.
//! JSON 은 값으로 견준다(`serde_json::Value` 같음 — Swift 사전은 키 순서가 없다).

use claude_board_autopilot::rules::formats::{
    adf, batches, batches_n, doc_clamp, doc_code, doc_code_n, doc_json, doc_parse, doc_plain_lines, doc_text, doc_text_n, grapheme_len,
    notion_blocks, notion_rich, storage, Block, Doc, Span, DOC_MARKER, NOTION_BATCH_MAX, NOTION_TEXT_MAX,
};
use serde_json::{json, Value};

const DOC_TEXT_ROWS: [(&str, &str); 5] = [
    ("A — B", "A, B"),
    ("줄\n바꿈\t탭", "줄 바꿈 탭"),
    ("거\u{202E}꾸로", "거꾸로"),
    ("  빈칸   여럿  ", "빈칸 여럿"),
    ("\u{0007}종", "종"),
];

fn text(s: &str) -> Span {
    Span::Text(s.to_string(), false)
}

#[test]
fn 다듬기() {
    for (input, want) in DOC_TEXT_ROWS {
        assert_eq!(doc_text(input), want, "{input:?}");
    }
}

#[test]
fn 노션글() {
    assert_eq!(notion_rich(&"가".repeat(2000), false, None).len(), 1);
    let two = notion_rich(&"가".repeat(2001), false, None);
    assert_eq!(two.len(), 2);
    assert_eq!(grapheme_len(two[1]["text"]["content"].as_str().expect("content")), 1);
    assert!(notion_rich("", false, None).is_empty());
}

#[test]
fn 묶음() {
    let a: Vec<i32> = (0..100).collect();
    let b: Vec<i32> = (0..101).collect();
    assert_eq!(batches(&a).iter().map(Vec::len).collect::<Vec<_>>(), [100]);
    assert_eq!(batches(&b).iter().map(Vec::len).collect::<Vec<_>>(), [100, 1]);
    assert!(batches::<i32>(&[]).is_empty());
}

#[test]
fn 상한() {
    let mut blocks: Vec<Block> = (0..450).map(|_| Block::Paragraph(vec![Span::Text("x".repeat(5000), false)])).collect();
    blocks.push(Block::Bullets((0..300).map(|_| vec![text("줄")]).collect()));
    let c = doc_clamp(&Doc::new(blocks));
    assert_eq!(c.blocks.len(), 400);
    match &c.blocks[0] {
        Block::Paragraph(s) => match &s[0] {
            Span::Text(t, _) => assert_eq!(t.chars().count(), 4001),
            other => panic!("글 조각: {other:?}"),
        },
        other => panic!("문단: {other:?}"),
    }
    let b = doc_clamp(&Doc::new(vec![Block::Bullets((0..300).map(|_| vec![text("줄")]).collect())]));
    match &b.blocks[0] {
        Block::Bullets(rows) => assert_eq!(rows.len(), 200),
        other => panic!("목록: {other:?}"),
    }
}

// ---- 추가: Swift 시험에 없는 것 ----

#[test]
fn extra_상수() {
    assert_eq!(DOC_MARKER, "오토파일럿 작성");
    assert_eq!((NOTION_TEXT_MAX, NOTION_BATCH_MAX), (2000, 100));
}

#[test]
fn extra_덩어리_세기() {
    for (s, n) in [
        ("", 0),
        ("abc", 3),
        ("가나다", 3),
        ("e\u{301}", 1),
        ("a\u{301}b", 2),
        ("\r\n", 1),
        ("\n\n", 2),
        ("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", 1),
        ("\u{1F1F0}\u{1F1F7}\u{1F1FA}\u{1F1F8}", 2),
        ("\u{1F1F0}\u{1F1F7}\u{1F1FA}", 2),
        ("\u{1100}\u{1161}\u{11A8}", 1),
        ("\u{1F44D}\u{1F3FD}", 1),
    ] {
        assert_eq!(grapheme_len(s), n, "{s:?}");
    }
}

#[test]
fn extra_상한에서_자르기() {
    assert_eq!(doc_text(&"x".repeat(4000)), "x".repeat(4000));
    assert_eq!(doc_text(&"x".repeat(4001)), format!("{}…", "x".repeat(4000)));
    assert_eq!(doc_text_n("abc", 2, true), "ab…");
    assert_eq!(doc_text_n("abc", 3, true), "abc");
    assert_eq!(doc_text_n("가나다", 2, true), "가나…");
    // 결합 글자는 앞 글자와 한 덩어리 — 잘리지 않는다
    assert_eq!(doc_text_n("e\u{301}e\u{301}", 1, true), "e\u{301}…");
    assert_eq!(doc_text_n("\u{1F468}\u{200D}\u{1F469}x", 1, true), "\u{1F468}\u{200D}\u{1F469}…");
}

#[test]
fn extra_다듬기_가장자리() {
    assert_eq!(doc_text_n("  a  b  ", 10, false), " a b ");
    assert_eq!(doc_text_n("  a  ", 10, true), "a");
    assert_eq!(doc_text("\u{A0}\u{3000}a\u{A0}"), "a");
    assert_eq!(doc_text("a\u{85}b\u{FEFF}c\u{2066}d\u{2069}e"), "abcde");
    assert_eq!(doc_text("줄\u{2028}둘\u{2029}셋\r끝"), "줄 둘 셋 끝");
    assert_eq!(doc_text(""), "");
}

#[test]
fn extra_코드_칸() {
    assert_eq!(doc_code("a\n\n b  \tc"), "a\n\nb c");
    assert_eq!(doc_code_n("abcdef\nxyz", 8), "abcdef\nx");
    // 줄마다 `…` 까지 붙여 자른 뒤 전체를 다시 n 글자로 자르므로 `…` 는 남지 않는다
    assert_eq!(doc_code_n("abcdef", 3), "abc");
    assert_eq!(doc_code_n("abcdef\nxy", 4), "abcd");
    assert_eq!(doc_code(""), "");
}

#[test]
fn extra_다듬기_문서() {
    let d = Doc::new(vec![
        Block::Heading(7, "큰 제목".into()),
        Block::Heading(-2, "작은 제목".into()),
        Block::Heading(2, "  ".into()),
        Block::Paragraph(vec![text("   "), Span::Text("굵게".into(), true), text(" 일반")]),
        Block::Paragraph(vec![text(" ")]),
        Block::Paragraph(vec![
            Span::Link("나쁜".into(), "javascript:alert(1)".into()),
            Span::Link("".into(), "javascript:alert(1)".into()),
            Span::Link("".into(), "https://example.com/a".into()),
            Span::Link("좋은".into(), "https://example.com/b".into()),
        ]),
        Block::Bullets(vec![vec![text("  ")], vec![text("줄")]]),
        Block::Bullets(vec![vec![text("")]]),
        Block::Code(String::new()),
        Block::Code(" x ".into()),
        Block::Rule,
    ]);
    let want = Doc::new(vec![
        Block::Heading(3, "큰 제목".into()),
        Block::Heading(1, "작은 제목".into()),
        Block::Paragraph(vec![Span::Text("굵게".into(), true), text(" 일반")]),
        Block::Paragraph(vec![
            text("나쁜"),
            Span::Link("https://example.com/a".into(), "https://example.com/a".into()),
            Span::Link("좋은".into(), "https://example.com/b".into()),
        ]),
        Block::Bullets(vec![vec![text("줄")]]),
        Block::Code("x".into()),
        Block::Rule,
    ]);
    assert_eq!(doc_clamp(&d), want);
    assert_eq!(doc_clamp(&want), want, "두 번 다듬어도 같다");
}

#[test]
fn extra_adf_꼴() {
    let d = Doc::new(vec![
        Block::Heading(2, "제목".into()),
        Block::Paragraph(vec![Span::Text("굵게".into(), true), text(" 일반"), Span::Link("링크".into(), "https://example.com/a?b=1&c=2".into())]),
        Block::Bullets(vec![vec![text("줄")]]),
        Block::Code("x".into()),
        Block::Rule,
    ]);
    let want = json!({"type": "doc", "version": 1, "content": [
        {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "제목"}]},
        {"type": "paragraph", "content": [
            {"type": "text", "text": "굵게", "marks": [{"type": "strong"}]},
            {"type": "text", "text": " 일반"},
            {"type": "text", "text": "링크", "marks": [{"type": "link", "attrs": {"href": "https://example.com/a?b=1&c=2"}}]}]},
        {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "줄"}]}]}]},
        {"type": "codeBlock", "content": [{"type": "text", "text": "x"}]},
        {"type": "rule"}]});
    assert_eq!(adf(&d), want);
    assert_eq!(adf(&Doc::default()), json!({"type": "doc", "version": 1, "content": []}));
    // 위험한 주소는 링크 표시가 아니라 글로만 남는다
    let evil = adf(&Doc::new(vec![Block::Paragraph(vec![Span::Link("나쁜".into(), "javascript:alert(1)".into())])]));
    assert!(!evil.to_string().contains("javascript:") && !evil.to_string().contains("\"link\""));
}

#[test]
fn extra_저장_형식_꼴() {
    let d = Doc::new(vec![
        Block::Heading(1, "<b>t</b>".into()),
        Block::Paragraph(vec![Span::Text("a&b".into(), true), Span::Link("L".into(), "https://example.com/x?y=1&z=2".into())]),
        Block::Bullets(vec![vec![text("1")], vec![text("2")]]),
        Block::Code("]]><ac:x>".into()),
        Block::Rule,
    ]);
    assert_eq!(
        storage(&d),
        "<h1>&lt;b&gt;t&lt;/b&gt;</h1><p><strong>a&amp;b</strong><a href=\"https://example.com/x?y=1&amp;z=2\">L</a></p><ul><li>1</li><li>2</li></ul><pre>]]&gt;&lt;ac:x&gt;</pre><hr />"
    );
    assert_eq!(storage(&Doc::default()), "");
    let evil = storage(&Doc::new(vec![Block::Paragraph(vec![
        Span::Link("나쁜".into(), "javascript:alert(1)".into()),
        Span::Link("데이터".into(), "data:text/html,x".into()),
    ])]));
    assert_eq!(evil, "<p>나쁜데이터</p>");
}

// ---- 추가: Notion 블록·묶음·파일 칸 ----

fn sample_doc() -> Doc {
    Doc::new(vec![
        Block::Heading(2, "제목".into()),
        Block::Paragraph(vec![Span::Text("굵게".into(), true), text(" 일반"), Span::Link("링크".into(), "https://example.com/a?b=1&c=2".into())]),
        Block::Bullets(vec![vec![text("하나")], vec![text("둘"), Span::Text("굵".into(), true)]]),
        Block::Code("x\ny".into()),
        Block::Rule,
    ])
}

#[test]
fn extra_노션_글_칸_꼴() {
    assert_eq!(notion_rich("a", false, None), vec![json!({"type": "text", "text": {"content": "a"}})]);
    assert_eq!(
        notion_rich("a", true, None),
        vec![json!({"type": "text", "text": {"content": "a"}, "annotations": {"bold": true}})]
    );
    assert_eq!(
        notion_rich("a", false, Some("https://e.com/")),
        vec![json!({"type": "text", "text": {"content": "a", "link": {"url": "https://e.com/"}}})]
    );
    // 칸마다 같은 굵기·링크를 단다
    let long = notion_rich(&"x".repeat(2001), true, Some("https://e.com/"));
    assert_eq!(long.len(), 2);
    assert!(long.iter().all(|i| i["annotations"]["bold"] == true && i["text"]["link"]["url"] == "https://e.com/"));
    let sizes: Vec<usize> = long.iter().map(|i| i["text"]["content"].as_str().unwrap().len()).collect();
    assert_eq!(sizes, [NOTION_TEXT_MAX, 1]);
}

#[test]
fn extra_노션_글_칸_글자_경계() {
    // 1999 글자 뒤 `e` + 결합 악센트는 한 덩어리 — 2000 번째 덩어리가 갈라지지 않는다
    let s = format!("{}e\u{301}z", "x".repeat(1999));
    let r = notion_rich(&s, false, None);
    let parts: Vec<&str> = r.iter().map(|i| i["text"]["content"].as_str().unwrap()).collect();
    assert_eq!(parts, [format!("{}e\u{301}", "x".repeat(1999)), "z".to_string()]);
    // 가 2001 개를 이어 붙이면 원문이 그대로다
    let joined: String = notion_rich(&"가".repeat(4001), false, None).iter().map(|i| i["text"]["content"].as_str().unwrap().to_string()).collect();
    assert_eq!(joined, "가".repeat(4001));
    assert_eq!(notion_rich(&"가".repeat(4001), false, None).len(), 3);
}

#[test]
fn extra_노션_블록_꼴() {
    let want = vec![
        json!({"object": "block", "type": "heading_2", "heading_2": {"rich_text": [{"type": "text", "text": {"content": "제목"}}]}}),
        json!({"object": "block", "type": "paragraph", "paragraph": {"rich_text": [
            {"type": "text", "text": {"content": "굵게"}, "annotations": {"bold": true}},
            {"type": "text", "text": {"content": " 일반"}},
            {"type": "text", "text": {"content": "링크", "link": {"url": "https://example.com/a?b=1&c=2"}}}]}}),
        json!({"object": "block", "type": "bulleted_list_item", "bulleted_list_item": {"rich_text": [{"type": "text", "text": {"content": "하나"}}]}}),
        json!({"object": "block", "type": "bulleted_list_item", "bulleted_list_item": {"rich_text": [
            {"type": "text", "text": {"content": "둘"}},
            {"type": "text", "text": {"content": "굵"}, "annotations": {"bold": true}}]}}),
        json!({"object": "block", "type": "code", "code": {"rich_text": [{"type": "text", "text": {"content": "x\ny"}}], "language": "plain text"}}),
        json!({"object": "block", "type": "divider", "divider": {}}),
    ];
    assert_eq!(notion_blocks(&sample_doc()), want);
    assert!(notion_blocks(&Doc::default()).is_empty());
}

#[test]
fn extra_노션_블록_상한() {
    // 한 블록의 rich_text 는 100 칸까지
    let spans: Vec<Span> = (0..150).map(|i| Span::Text(format!("{}", i % 10), i % 2 == 0)).collect();
    let b = notion_blocks(&Doc::new(vec![Block::Paragraph(spans)]));
    assert_eq!(b.len(), 1);
    assert_eq!(b[0]["paragraph"]["rich_text"].as_array().unwrap().len(), 100);
    // 위험한 링크는 글로만 남고, 제목 수준은 1~3 으로 맞춘다
    let e = notion_blocks(&Doc::new(vec![Block::Heading(9, "h".into()), Block::Paragraph(vec![Span::Link("나쁜".into(), "javascript:alert(1)".into())])]));
    assert_eq!(e[0]["type"], "heading_3");
    assert!(!Value::Array(e).to_string().contains("javascript:"));
    // 목록 줄은 블록 하나씩 — 150 블록이면 100 + 50 묶음이다
    let rows = Block::Bullets((0..150).map(|_| vec![text("줄")]).collect());
    let nb = notion_blocks(&Doc::new(vec![rows]));
    assert_eq!(batches(&nb).iter().map(Vec::len).collect::<Vec<_>>(), [100, 50]);
}

#[test]
fn extra_묶음_경계() {
    let a: Vec<i32> = (0..250).collect();
    assert_eq!(batches(&a).iter().map(Vec::len).collect::<Vec<_>>(), [100, 100, 50]);
    assert_eq!(batches_n(&a, 250).len(), 1);
    assert_eq!(batches_n(&a, 7).len(), 36);
    assert_eq!(batches_n(&a, 1)[3], [3]);
    // n 이 0 이면 통째로 한 묶음(빈 입력도 빈 묶음 하나)
    assert_eq!(batches_n(&[1, 2, 3], 0), vec![vec![1, 2, 3]]);
    assert_eq!(batches_n::<i32>(&[], 0), vec![Vec::<i32>::new()]);
    assert!(batches_n::<i32>(&[], 5).is_empty());
    // 묶음을 이으면 원본 순서 그대로
    assert_eq!(batches(&a).concat(), a);
}

#[test]
fn extra_파일_칸_꼴() {
    let want = vec![
        json!({"t": "h", "n": 2, "x": "제목"}),
        json!({"t": "p", "s": [{"x": "굵게", "b": true}, {"x": " 일반"}, {"x": "링크", "l": "https://example.com/a?b=1&c=2"}]}),
        json!({"t": "ul", "i": [[{"x": "하나"}], [{"x": "둘"}, {"x": "굵", "b": true}]]}),
        json!({"t": "code", "x": "x\ny"}),
        json!({"t": "hr"}),
    ];
    assert_eq!(doc_json(&sample_doc()), want);
    assert!(doc_json(&Doc::default()).is_empty());
}

#[test]
fn extra_파일_칸_왕복() {
    let d = sample_doc();
    assert_eq!(doc_parse(Some(&Value::Array(doc_json(&d)))), d);
    // 글 하나를 거쳐도(문자열로) 같다
    let text_form = Value::Array(doc_json(&d)).to_string();
    let back: Value = serde_json::from_str(&text_form).unwrap();
    assert_eq!(doc_parse(Some(&back)), d);
}

#[test]
fn extra_파일_칸_읽기_너그러움() {
    // 칸이 없거나 모양이 틀리면 빈 문서
    assert_eq!(doc_parse(None), Doc::default());
    assert_eq!(doc_parse(Some(&json!("글"))), Doc::default());
    assert_eq!(doc_parse(Some(&json!({"t": "hr"}))), Doc::default());
    // 원소 하나라도 사전이 아니면 Swift `as? [[String: Any]]` 가 통째로 실패한다
    assert_eq!(doc_parse(Some(&json!([{"t": "hr"}, 3]))), Doc::default());
    // 모르는 블록은 버린다
    assert_eq!(doc_parse(Some(&json!([{"t": "zzz"}, {"x": "t 없음"}, {"t": 5}, {"t": "hr"}]))), Doc::new(vec![Block::Rule]));
    // 빠진 칸의 기본값: 제목 수준 2, 글 빈칸(다듬으면 사라진다)
    assert_eq!(doc_parse(Some(&json!([{"t": "h", "x": "머리"}]))), Doc::new(vec![Block::Heading(2, "머리".into())]));
    assert_eq!(doc_parse(Some(&json!([{"t": "h"}, {"t": "code"}, {"t": "p"}, {"t": "ul"}]))), Doc::default());
    // 제목 수준은 다듬어서 1~3. 소수는 버리고, 불리언은 1·0
    assert_eq!(doc_parse(Some(&json!([{"t": "h", "n": 9, "x": "a"}, {"t": "h", "n": -4, "x": "b"}, {"t": "h", "n": 2.9, "x": "c"}]))),
        Doc::new(vec![Block::Heading(3, "a".into()), Block::Heading(1, "b".into()), Block::Heading(2, "c".into())]));
    assert_eq!(doc_parse(Some(&json!([{"t": "h", "n": "3", "x": "d"}]))), Doc::new(vec![Block::Heading(2, "d".into())]));
    // 글 조각: `l` 이 글이면 링크, 아니면 글(`b` 가 불리언일 때만 굵게)
    let spans = json!([{"x": "a", "b": true}, {"x": "b", "b": "yes"}, {"x": "c", "l": "https://e.com/"}, {"x": "d", "l": 7}, {"b": true}]);
    assert_eq!(
        doc_parse(Some(&json!([{"t": "p", "s": spans}]))),
        Doc::new(vec![Block::Paragraph(vec![Span::Text("a".into(), true), text("b"), Span::Link("c".into(), "https://e.com/".into()), text("d")])])
    );
    // 목록: 줄 하나가 틀리면 그 줄만 빈 줄이 되어 사라진다
    assert_eq!(
        doc_parse(Some(&json!([{"t": "ul", "i": [[{"x": "a"}], "나쁨", [{"x": "b"}, 4]]}]))),
        Doc::new(vec![Block::Bullets(vec![vec![text("a")]])])
    );
}

#[test]
fn extra_파일_칸_읽기_다듬기() {
    // 읽을 때 다시 다듬는다: 위험한 링크·긴 줄표·화면을 속이는 글자·가로줄 보존
    let v = json!([
        {"t": "p", "s": [{"x": "나쁜 — 링크\u{202E}", "l": "javascript:alert(1)"}]},
        {"t": "code", "x": "  a\u{7}b  "},
        {"t": "hr"}
    ]);
    assert_eq!(
        doc_parse(Some(&v)),
        Doc::new(vec![Block::Paragraph(vec![text("나쁜, 링크")]), Block::Code("ab".into()), Block::Rule])
    );
}

#[test]
fn extra_쉬운_글_줄() {
    assert_eq!(doc_plain_lines(&sample_doc()), ["제목", "굵게 일반링크", "· 하나", "· 둘굵", "x\ny", "(가로줄)"]);
    assert!(doc_plain_lines(&Doc::default()).is_empty());
    assert_eq!(doc_plain_lines(&Doc::new(vec![Block::Paragraph(vec![]), Block::Bullets(vec![])])), [""]);
    let marker = Doc::new(vec![Block::Paragraph(vec![text(DOC_MARKER)])]);
    assert_eq!(doc_plain_lines(&marker).first().map(String::as_str), Some(DOC_MARKER));
}

#[test]
fn extra_코드_칸_crlf() {
    // Swift 시험은 CRLF 를 못박지 않았다. `\n` 에서만 가르고 `\r` 은 `doc_text` 가 빈칸으로 바꿔 줄 끝에서 지운다
    assert_eq!(doc_code("a\r\nb\r\n"), "a\nb\n");
    assert_eq!(doc_code("a\rb"), "a b");
    assert_eq!(doc_code("\r\n"), "\n");
}
