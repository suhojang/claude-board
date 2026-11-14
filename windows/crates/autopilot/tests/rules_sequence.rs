//! 흐름 그림 규칙(`rules::sequence`) 시험. 원본: `Tests/AutopilotRulesTests/ReportTests.swift` 의 `SequenceTests`
//! (`describe`·`seqRows` 표는 같은 파일). Swift 시험 4개(`읽기`(표 25 줄)·`그리기`·`넓히기`·`폭`)를 모두 옮겼다.
//! 표의 줄은 Swift 에서 기계로 옮겼다(손으로 다시 치지 않았다). `그리기`·`넓히기` 는 표가 아니라 시험 안의 글자라 같은 글자를 그대로 옮겼다.
//! 글자 그대로 옮길 수 없는 두 줄(`참여자 25명`·`줄 301개`, 원본이 `map`·`String(repeating:)` 으로 만든 입력)만 시험 함수 안에서 같은 식으로 만든다.
//! 표 길이를 세어 줄이 빠지면 실패한다. 끝의 "추가" 시험은 Swift 시험에 없는 것(경계·Foundation 과의 차이·SVG 글자 골든)이다.

use claude_board_autopilot::rules::sequence::{num, parse_sequence, seq_lines, sequence_svg, text_width, SeqDiagram, SeqItem, SeqParse};

struct Row {
    note: &'static str,
    src: &'static str,
    want: &'static str,
}

// 아래 표는 Swift 시험 파일에서 기계로 옮긴 것이다: 글자 값은 그대로 복사했다(Swift 의 \n·\" 는 Rust 에서도 같은 뜻이다).
const SEQ_ROWS: [Row; 23] = [
    Row { note: "가장 작은 꼴", src: "sequenceDiagram\nA->>B: 안녕", want: "P:A,B | 0->>1:안녕" },
    Row { note: "선언한 차례와 이름, actor", src: "sequenceDiagram\n actor U as 사용자\n participant A as API 서버\n A->>U: 답", want: "P:@U=사용자,A=API 서버 | 1->>0:답" },
    Row { note: "선언 없이 나온 차례", src: "sequenceDiagram\nB->>A: x\nC-->>B: y", want: "P:B,A,C | 0->>1:x · 2-->>0:y" },
    Row { note: "화살표 열 가지", src: "sequenceDiagram\nA->B: 1\nA-->B: 2\nA->>B: 3\nA-->>B: 4\nA-xB: 5\nA--xB: 6\nA-)B: 7\nA--)B: 8\nA<<->>B: 9\nA<<-->>B: 10", want: "P:A,B | 0-1:1 · 0--1:2 · 0->>1:3 · 0-->>1:4 · 0-x1:5 · 0--x1:6 · 0-)1:7 · 0--)1:8 · 0<->>1:9 · 0<-->>1:10" },
    Row { note: "켜기 표지 +·- 는 무시", src: "sequenceDiagram\nA->>+B: 가\nB-->>-A: 나", want: "P:A,B | 0->>1:가 · 1-->>0:나" },
    Row { note: "쌍점 뒤의 화살표 꼴은 글", src: "sequenceDiagram\nA->>B: a->b 로", want: "P:A,B | 0->>1:a->b 로" },
    Row { note: "글 없는 메시지", src: "sequenceDiagram\nA->>B", want: "P:A,B | 0->>1:" },
    Row { note: "Note 셋", src: "sequenceDiagram\nparticipant A\nparticipant B\nNote left of A: 왼\nnote right of B: 오\nNote over A,B: 둘", want: "P:A,B | N left 0:왼 · N right 1:오 · N over 0,1:둘" },
    Row { note: "묶음과 갈래, 빈 줄·주석·끝 쌍반점", src: "sequenceDiagram\n%% 주석\n\nalt 맞음;\nA->>B: x\nelse 틀림\nA-xB: y\nend\nloop 매시\nB->>B: 다시\nend", want: "P:A,B | [alt 맞음 · 0->>1:x · |else 틀림 · 0-x1:y · ] · [loop 매시 · 1->>1:다시 · ]" },
    Row { note: "par/and·critical/option·break·opt", src: "sequenceDiagram\npar 가\nA->>B: 1\nand 나\nA->>B: 2\nend\ncritical 꼭\nA->>B: 3\noption 실패\nA->>B: 4\nend\nbreak 멈춤\nA->>B: 5\nend\nopt 골라\nA->>B: 6\nend", want: "P:A,B | [par 가 · 0->>1:1 · |and 나 · 0->>1:2 · ] · [critical 꼭 · 0->>1:3 · |option 실패 · 0->>1:4 · ] · [break 멈춤 · 0->>1:5 · ] · [opt 골라 · 0->>1:6 · ]" },
    Row { note: "rect·box 의 색 글자는 버림", src: "sequenceDiagram\nbox rgb(1,2,3) 묶음\nparticipant A\nend\nrect rgba(0,0,255,.1)\nA->>A: x\nend", want: "P:A | [box · ] · [rect · 0->>0:x · ]" },
    Row { note: "닫지 않은 묶음은 끝에서 닫음", src: "sequenceDiagram\nloop 계속\nA->>B: x", want: "P:A,B | [loop 계속 · 0->>1:x · ]" },
    Row { note: "autonumber·title·activate·destroy·create", src: "sequenceDiagram\ntitle: 제목\nautonumber\ncreate participant C as 새것\nactivate A\nA->>C: x\ndeactivate A\ndestroy C", want: "P:C=새것,A | 1->>0:x | # | 제목 제목" },
    Row { note: "다른 그림 꼴", src: "graph TD; A-->B; click A \"javascript:alert(1)\"", want: "못 그림 1: sequenceDiagram 으로 시작하지 않습니다" },
    Row { note: "빈 원문", src: "  \n", want: "못 그림 1: 비어 있습니다" },
    Row { note: "참여자 없음", src: "sequenceDiagram\nautonumber", want: "못 그림 1: 참여자가 없습니다" },
    Row { note: "읽지 못한 줄은 그리지 않음(말없이 빠지지 않게)", src: "sequenceDiagram\nA->>B: x\nclick A call()", want: "못 그림 3: 읽지 못한 줄입니다" },
    Row { note: "link 줄", src: "sequenceDiagram\nA->>B: x\nlink A: 대시보드 @ https://example.com", want: "못 그림 3: 읽지 못한 줄입니다" },
    Row { note: "alt 밖의 else", src: "sequenceDiagram\nA->>B: x\nelse 틀림", want: "못 그림 3: else 가 맞는 묶음 안에 있지 않습니다" },
    Row { note: "여는 묶음 없는 end", src: "sequenceDiagram\nA->>B: x\nend", want: "못 그림 3: 여는 묶음 없이 end 가 있습니다" },
    Row { note: "Note 대상 셋", src: "sequenceDiagram\nNote over A,B,C: x", want: "못 그림 2: Note 줄을 읽지 못했습니다" },
    Row { note: "Note 쌍점 없음", src: "sequenceDiagram\nNote over A x", want: "못 그림 2: Note 줄에 쌍점(:)이 없습니다" },
    Row { note: "참여자 이름에 꺾쇠·따옴표", src: "sequenceDiagram\nparticipant \"A\"", want: "못 그림 2: 참여자 이름을 읽지 못했습니다" },
];

// 그림 값을 견주기 쉬운 글로: "P:A,@U=사용자 | 0->>1:글 · N over 0,1:글 · [alt 라벨 · |else 라벨 · ] · #". 원본 `describe` 와 같다
fn describe(p: &SeqParse) -> String {
    match p {
        SeqParse::Unsupported { line, why } => format!("못 그림 {line}: {why}"),
        SeqParse::Ok(d) => {
            let ps = d
                .participants
                .iter()
                .map(|p| format!("{}{}{}", if p.actor { "@" } else { "" }, p.id, if p.label == p.id { String::new() } else { format!("={}", p.label) }))
                .collect::<Vec<_>>()
                .join(",");
            let items = d
                .items
                .iter()
                .map(|it| match it {
                    SeqItem::Message { from, to, text, dotted, head, both } => {
                        let tail = match head.as_str() {
                            "none" => "",
                            "arrow" => ">>",
                            "cross" => "x",
                            _ => ")",
                        };
                        format!("{from}{}{}{tail}{to}:{text}", if *both { "<" } else { "" }, if *dotted { "--" } else { "-" })
                    }
                    SeqItem::Note { at, side, text } => {
                        format!("N {} {}:{text}", side.as_str(), at.iter().map(usize::to_string).collect::<Vec<_>>().join(","))
                    }
                    SeqItem::Begin { kind, label } => format!("[{kind}{}", if label.is_empty() { String::new() } else { format!(" {label}") }),
                    SeqItem::Divide { kind, label } => format!("|{kind}{}", if label.is_empty() { String::new() } else { format!(" {label}") }),
                    SeqItem::End => "]".to_string(),
                })
                .collect::<Vec<_>>()
                .join(" · ");
            format!("P:{ps} | {items}{}{}", if d.autonumber { " | #" } else { "" }, if d.title.is_empty() { String::new() } else { format!(" | 제목 {}", d.title) })
        }
    }
}

fn read(src: &str) -> String {
    describe(&parse_sequence(src))
}

// Swift 시험 `읽기`(sequenceDiagram 의 작은 부분만 읽고 그 밖은 원문으로) — 표 23 줄 + 계산으로 만든 2 줄
#[test]
fn 읽기() {
    for r in SEQ_ROWS.iter() {
        assert_eq!(read(r.src), r.want, "{}", r.note);
    }
    // 원본: .init(note: "참여자 25명", src: "sequenceDiagram\n" + (1...25).map { "participant P\($0)" }.joined(separator: "\n"), want: "못 그림 26: 참여자가 너무 많습니다(24까지)")
    let src = format!("sequenceDiagram\n{}", (1..=25).map(|i| format!("participant P{i}")).collect::<Vec<_>>().join("\n"));
    assert_eq!(read(&src), "못 그림 26: 참여자가 너무 많습니다(24까지)", "참여자 25명");
    // 원본: .init(note: "줄 301개", src: "sequenceDiagram\n" + String(repeating: "A->>B: x\n", count: 301), want: "못 그림 302: 줄이 너무 많습니다(300줄까지 그립니다)")
    let src = format!("sequenceDiagram\n{}", "A->>B: x\n".repeat(301));
    assert_eq!(read(&src), "못 그림 302: 줄이 너무 많습니다(300줄까지 그립니다)", "줄 301개");
}

// Swift 시험 `폭`
#[test]
fn 폭() {
    assert_eq!(text_width("가나", 10.0), 20.0);
    assert_eq!(text_width("ab", 10.0), 12.0);
    assert_eq!(text_width("", 10.0), 0.0);
}

#[test]
fn 표_길이() {
    // Swift seqRows 는 25 줄: 글자 그대로 옮긴 23 줄 + 계산으로 만든 2 줄(읽기 안)
    assert_eq!(SEQ_ROWS.len(), 23);
}

// ---- 추가: Swift 시험에 없는 경계 ----

#[test]
fn 추가_줄바꿈_꼴과_키워드_대소문자() {
    assert_eq!(read("sequenceDiagram\r\nA->>B: x\r\n"), "P:A,B | 0->>1:x");
    assert_eq!(read("sequenceDiagram\rA->>B: x"), "P:A,B | 0->>1:x");
    assert_eq!(read("sequenceDiagram\nALT 갈래\nA->>B: x\nELSE 틀림\nEnd"), "P:A,B | [alt 갈래 · 0->>1:x · |else 틀림 · ]");
    assert_eq!(read("sequenceDiagram\nelse 틀림"), "못 그림 2: else 가 맞는 묶음 안에 있지 않습니다");
    assert_eq!(read("sequenceDiagram\nELSE 틀림"), "못 그림 2: ELSE 가 맞는 묶음 안에 있지 않습니다");
}

#[test]
fn 추가_참여자_다시_선언과_이름() {
    assert_eq!(read("sequenceDiagram\nparticipant A\nactor A as 에이\nA->>B: x"), "P:@A=에이,B | 0->>1:x");
    assert_eq!(read("sequenceDiagram\nparticipant A as 가 as 나\nA->>A: x"), "P:A=가 as 나 | 0->>0:x");
    assert_eq!(read("sequenceDiagram\ncreate note A"), "못 그림 2: 읽지 못한 줄입니다");
}

#[test]
fn 추가_note_대상_쪼개기() {
    // 빈 조각은 버리고 공백뿐인 조각은 남아 이름 검사에서 걸린다
    assert_eq!(read("sequenceDiagram\nNote over A,,B: x"), "P:A,B | N over 0,1:x");
    assert_eq!(read("sequenceDiagram\nNote over A, : x"), "못 그림 2: Note 줄을 읽지 못했습니다");
    assert_eq!(read("sequenceDiagram\nNote left of A,B: x"), "못 그림 2: Note 줄을 읽지 못했습니다");
    assert_eq!(read("sequenceDiagram\nNote left A: x"), "못 그림 2: Note 줄을 읽지 못했습니다");
    assert_eq!(read("sequenceDiagram\nNote under A: x"), "못 그림 2: Note 줄을 읽지 못했습니다");
}

#[test]
fn 추가_end_뒤에_글이_있으면_메시지로_읽는다() {
    assert_eq!(read("sequenceDiagram\nend A->>B: x"), "못 그림 2: 메시지 줄을 읽지 못했습니다");
    assert_eq!(read("sequenceDiagram\nend"), "못 그림 2: 여는 묶음 없이 end 가 있습니다");
}

#[test]
fn 추가_제목과_빈칸_깎기() {
    assert_eq!(read("sequenceDiagram\ntitle 제목\nA->B"), "P:A,B | 0-1: | 제목 제목");
    assert_eq!(read("sequenceDiagram\ntitle : 제목\nA->B"), "P:A,B | 0-1: | 제목 제목");
    // 전각 빈칸·NBSP 는 Foundation `.whitespaces`(Zs)라 깎인다
    assert_eq!(read("sequenceDiagram\n\u{3000}A->>B: x\u{a0}"), "P:A,B | 0->>1:x");
    let long = format!("sequenceDiagram\ntitle: {}\nA->B", "가".repeat(301));
    match parse_sequence(&long) {
        SeqParse::Ok(d) => assert_eq!(d.title.chars().count(), 300),
        other => panic!("읽지 못함: {other:?}"),
    }
}

#[test]
fn 추가_한도_경계() {
    let ok24 = format!("sequenceDiagram\n{}", (1..=24).map(|i| format!("participant P{i}")).collect::<Vec<_>>().join("\n"));
    assert!(matches!(parse_sequence(&ok24), SeqParse::Ok(d) if d.participants.len() == 24));
    let items300 = format!("sequenceDiagram\n{}", "A->>B: x\n".repeat(300));
    assert!(matches!(parse_sequence(&items300), SeqParse::Ok(d) if d.items.len() == 300));
    let label300 = format!("sequenceDiagram\nA->>B: {}", "가".repeat(300));
    assert!(matches!(parse_sequence(&label300), SeqParse::Ok(_)));
    let label301 = format!("sequenceDiagram\nA->>B: {}", "가".repeat(301));
    assert_eq!(read(&label301), "못 그림 2: 메시지 줄을 읽지 못했습니다");
    let id81 = format!("sequenceDiagram\nparticipant {}", "a".repeat(81));
    assert_eq!(read(&id81), "못 그림 2: 참여자 이름을 읽지 못했습니다");
    let id80 = format!("sequenceDiagram\nparticipant {}", "a".repeat(80));
    assert!(matches!(parse_sequence(&id80), SeqParse::Ok(_)));
}

#[test]
fn 추가_글자_폭_범위() {
    assert_eq!(text_width("a가", 10.0), 16.0);
    assert_eq!(text_width("\u{1F600}", 10.0), 10.0); // 그림 글자 1em
    assert_eq!(text_width("\u{FF21}", 10.0), 10.0); // 전각 A
    assert_eq!(text_width("\u{2014}", 10.0), 6.0); // 긴 줄표는 0.6em
    assert_eq!(text_width("a", 0.0), 0.0);
}

// ---- SVG: Swift 시험 `그리기`·`넓히기` ----

// Swift 시험 `그리기` — SVG: 모든 글은 이스케이프, 바깥 참조·id 없음, 같은 값이면 같은 글자
#[test]
fn 그리기() {
    let SeqParse::Ok(d) = parse_sequence("sequenceDiagram\nparticipant A as <script>x</script>\nA->>B: <img src=x onerror=alert(1)>\nNote over A: </text><b>\nB->>B: 혼자 — 다시\nalt 갈래\nA-xB: 끝\nend") else {
        panic!("읽지 못함");
    };
    let s = sequence_svg(&d, "그림 \"하나\"");
    assert_eq!(s, sequence_svg(&d, "그림 \"하나\""));
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 ") && s.ends_with("</svg>"));
    assert!(!s.contains("<script") && !s.contains("<img") && !s.contains("</text><b>") && !s.contains('—'));
    assert!(s.contains("&lt;script&gt;x&lt;/script&gt;") && s.contains("혼자, 다시") && s.contains("aria-label=\"그림 &quot;하나&quot;\""));
    assert!(!s.contains(" id=") && !s.contains("href") && !s.contains("url(") && !s.contains("marker"));
    assert!(s.contains("<polyline class=\"ml\" points=") && s.contains("class=\"fr\"") && s.contains("class=\"nb\""));
    assert_eq!(sequence_svg(&SeqDiagram::default(), "빈 그림"), "");
}

// Swift 시험 `넓히기` — 긴 메시지는 참여자 사이를 넓힌다
#[test]
fn 넓히기() {
    fn width(src: &str) -> i64 {
        let SeqParse::Ok(d) = parse_sequence(src) else { return -1 };
        let s = sequence_svg(&d, "");
        let Some(r) = s.find("width=\"") else { return -1 };
        let rest = &s[r + "width=\"".len()..];
        let Some(e) = rest.find('"') else { return -1 };
        rest[..e].parse().unwrap_or(-1)
    }
    let short = width("sequenceDiagram\nA->>B: 짧음");
    let long = width(&format!("sequenceDiagram\nA->>B: {}", "아주 긴 메시지 ".repeat(8)));
    assert!(short > 0 && long > short + 300, "short={short} long={long}");
}

// ---- 추가: SVG 글자 골든(손으로 계산: 상자 84x34, 간격 120, 첫 x=54, 화살 끝 x=172, 줄 y=84)·수 글자·줄 나누기 ----

fn svg(src: &str) -> String {
    match parse_sequence(src) {
        SeqParse::Ok(d) => sequence_svg(&d, "x"),
        other => panic!("읽지 못함: {other:?}"),
    }
}

#[test]
fn 추가_수_글자() {
    let table: [(f64, &str); 22] = [
        (0.0, "0"),
        (-0.0, "0"),
        (0.4, "0"),
        (0.5, "1"),
        (0.6, "1"),
        (1.5, "2"),
        (2.5, "3"),
        (3.5, "4"),
        (-0.4, "0"),
        (-0.5, "-1"),
        (-1.5, "-2"),
        (-2.5, "-3"),
        (12.0, "12"),
        (79.5, "80"),
        (88.5, "89"),
        (99.5, "100"),
        (-6.0, "-6"),
        (0.49999999999999994, "0"),
        (1e12, "1000000000000"),
        (-1e12, "-1000000000000"),
        (4503599627370497.0, "4503599627370497"),
        (34.7, "35"),
    ];
    for (x, want) in table {
        assert_eq!(num(x), want, "num({x})");
    }
}

#[test]
fn 추가_줄_나누기() {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(seq_lines("a<br>b"), v(&["a", "b"]));
    assert_eq!(seq_lines("a<BR/>b<Br />c"), v(&["a", "b", "c"]));
    assert_eq!(seq_lines("a<br\t/>b<br  >c"), v(&["a", "b", "c"]));
    assert_eq!(seq_lines(" a <br> b "), v(&["a", "b"]));
    assert_eq!(seq_lines("<br>"), v(&["", ""]));
    assert_eq!(seq_lines(""), v(&[""]));
    assert_eq!(seq_lines("a\nb"), v(&["a", "b"]));
    assert_eq!(seq_lines("가<br>나\u{3000}"), v(&["가", "나"]));
    // 표지가 아닌 것은 그대로 둔다
    assert_eq!(seq_lines("a<brx>b"), v(&["a<brx>b"]));
    assert_eq!(seq_lines("a<br//>b"), v(&["a<br//>b"]));
    assert_eq!(seq_lines("a<b r>b"), v(&["a<b r>b"]));
    assert_eq!(seq_lines("a<br"), v(&["a<br"]));
    assert_eq!(seq_lines("a<br/"), v(&["a<br/"]));
    // ICU `\s` 는 [\t\n\f\r\p{Z}]: 줄바꿈·전각 빈칸은 표지 안 빈칸이고 VT·U+0085 는 아니다
    assert_eq!(seq_lines("a<br\n>b"), v(&["a", "b"]));
    assert_eq!(seq_lines("a<br\u{3000}/>b"), v(&["a", "b"]));
    assert_eq!(seq_lines("a<br\u{b}>b"), v(&["a<br\u{b}>b"]));
    assert_eq!(seq_lines("a<br\u{85}>b"), v(&["a<br\u{85}>b"]));
    // `\r` 은 `.whitespaces` 가 아니라 남는다(원본 실행으로 확인하지 못한 가정: NSString 은 `\n` 에서만 자른다)
    assert_eq!(seq_lines("a\r\nb"), v(&["a\r", "b"]));
}

#[test]
fn 추가_골든_가장_작은_그림() {
    let want = concat!(
        "<svg class=\"sq\" viewBox=\"0 0 228 150\" width=\"228\" height=\"150\" role=\"img\" aria-label=\"x\">",
        "<line class=\"ll\" x1=\"54\" y1=\"44\" x2=\"54\" y2=\"106\"/><line class=\"ll\" x1=\"174\" y1=\"44\" x2=\"174\" y2=\"106\"/>",
        "<rect class=\"pb\" x=\"12\" y=\"10\" width=\"84\" height=\"34\" rx=\"4\"/><text class=\"pt\" x=\"54\" y=\"30\" text-anchor=\"middle\">A</text>",
        "<rect class=\"pb\" x=\"12\" y=\"106\" width=\"84\" height=\"34\" rx=\"4\"/><text class=\"pt\" x=\"54\" y=\"126\" text-anchor=\"middle\">A</text>",
        "<rect class=\"pb\" x=\"132\" y=\"10\" width=\"84\" height=\"34\" rx=\"4\"/><text class=\"pt\" x=\"174\" y=\"30\" text-anchor=\"middle\">B</text>",
        "<rect class=\"pb\" x=\"132\" y=\"106\" width=\"84\" height=\"34\" rx=\"4\"/><text class=\"pt\" x=\"174\" y=\"126\" text-anchor=\"middle\">B</text>",
        "<text class=\"mt\" x=\"114\" y=\"74\" text-anchor=\"middle\">안녕</text>",
        "<line class=\"ml\" x1=\"54\" y1=\"84\" x2=\"172\" y2=\"84\"/>",
        "<polygon class=\"ah\" points=\"172,84 163,80 163,89\"/></svg>"
    );
    assert_eq!(svg("sequenceDiagram\nA->>B: 안녕"), want);
}

#[test]
fn 추가_골든_화살_머리() {
    let head = |src: &str| {
        let s = svg(&format!("sequenceDiagram\nparticipant A\nparticipant B\n{src}"));
        s[s.find("<line class=\"ml").expect("message line")..s.len() - "</svg>".len()].to_string()
    };
    assert_eq!(head("A->B: 안녕"), "<line class=\"ml\" x1=\"54\" y1=\"84\" x2=\"172\" y2=\"84\"/>");
    assert_eq!(head("A-->B: 안녕"), "<line class=\"ml d\" x1=\"54\" y1=\"84\" x2=\"172\" y2=\"84\"/>");
    assert_eq!(head("A-)B: 안녕"), "<line class=\"ml\" x1=\"54\" y1=\"84\" x2=\"172\" y2=\"84\"/><polyline class=\"ml\" points=\"163,79 172,84 163,89\"/>");
    assert_eq!(
        head("A-xB: 안녕"),
        "<line class=\"ml\" x1=\"54\" y1=\"84\" x2=\"172\" y2=\"84\"/><line class=\"ml\" x1=\"163\" y1=\"80\" x2=\"171\" y2=\"88\"/><line class=\"ml\" x1=\"163\" y1=\"88\" x2=\"171\" y2=\"80\"/>"
    );
    // 왼쪽으로 가는 화살: 끝은 받는 쪽 x(54)+2, 머리는 오른쪽으로 벌어진다
    assert_eq!(head("B->>A: 안녕"), "<line class=\"ml\" x1=\"174\" y1=\"84\" x2=\"56\" y2=\"84\"/><polygon class=\"ah\" points=\"56,84 65,80 65,89\"/>");
    // 양쪽: 받는 쪽 머리 다음에 보내는 쪽 머리
    assert_eq!(
        head("A<<->>B: 안녕"),
        "<line class=\"ml\" x1=\"54\" y1=\"84\" x2=\"172\" y2=\"84\"/><polygon class=\"ah\" points=\"172,84 163,80 163,89\"/><polygon class=\"ah\" points=\"56,84 65,80 65,89\"/>"
    );
}

#[test]
fn 추가_골든_혼자_메시지와_번호() {
    let s = svg("sequenceDiagram\nA->>A: 안녕");
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 147 170\" width=\"147\" height=\"170\" role=\"img\" aria-label=\"x\">"), "{s}");
    assert!(s.contains("<text class=\"mt\" x=\"64\" y=\"74\" text-anchor=\"start\">안녕</text>"), "{s}");
    assert!(s.contains("<polyline class=\"ml\" points=\"54,84 94,84 94,104 56,104\"/><polygon class=\"ah\" points=\"56,104 65,100 65,109\"/></svg>"), "{s}");
    // 번호: 글이 없어도 번호 줄은 그린다
    let s = svg("sequenceDiagram\nautonumber\nA->>B\nB->>A: 답<br>둘째");
    assert!(s.contains("<text class=\"mt\" x=\"114\" y=\"74\" text-anchor=\"middle\">1. </text>"), "{s}");
    assert!(s.contains("middle\">2. 답</text>") && s.contains("middle\">둘째</text>"), "{s}");
}

#[test]
fn 추가_골든_묶음_틀() {
    let s = svg("sequenceDiagram\nalt 갈래\nA->>B: x\nelse 틀림\nA->>B: y\nend");
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"-6 0 240 248\" width=\"240\" height=\"248\" role=\"img\" aria-label=\"x\">"), "{s}");
    let frame = concat!(
        "<rect class=\"fr\" x=\"-2\" y=\"62\" width=\"232\" height=\"126\" rx=\"2\"/>",
        "<path class=\"ft\" d=\"M-2 62h35v12l-6 6h-29z\"/>",
        "<text class=\"fl\" x=\"4\" y=\"75\">alt</text>",
        "<text class=\"fc\" x=\"41\" y=\"76\">[갈래]</text>",
        "<line class=\"fs\" x1=\"-2\" y1=\"124\" x2=\"230\" y2=\"124\"/>",
        "<text class=\"fc\" x=\"6\" y=\"139\">[틀림]</text>"
    );
    assert!(s.contains(&format!("aria-label=\"x\">{frame}<line class=\"ll\"")), "{s}");
    // rect 는 이름표 없이 틀만, box 는 틀을 그리지 않는다
    let s = svg("sequenceDiagram\nrect rgb(1,2,3)\nA->>B: x\nend");
    assert!(s.contains("<rect class=\"fr rc\" ") && !s.contains("class=\"ft\"") && !s.contains("class=\"fl\""), "{s}");
    let s = svg("sequenceDiagram\nbox 묶음\nA->>B: x\nend");
    assert!(!s.contains("class=\"fr") && s.contains("class=\"ml\""), "{s}");
}

#[test]
fn 추가_note_자리와_바깥_여백() {
    // 맨 왼쪽 참여자의 왼쪽 Note·맨 오른쪽 참여자의 오른쪽 Note 는 viewBox 를 넓힌다
    let base = svg("sequenceDiagram\nparticipant A\nA->>A: x");
    let left = svg("sequenceDiagram\nparticipant A\nNote left of A: 아주 긴 왼쪽 글입니다");
    let right = svg("sequenceDiagram\nparticipant A\nNote right of A: 아주 긴 오른쪽 글입니다");
    let w = |s: &str| -> i64 {
        let r = s.find(" width=\"").expect("width") + " width=\"".len();
        s[r..r + s[r..].find('"').expect("quote")].parse().expect("number")
    };
    assert!(w(&left) > w(&base) - 40 && w(&right) > 150, "{} {} {}", w(&base), w(&left), w(&right));
    assert!(left.contains("class=\"nb\"") && right.contains("class=\"nt\""));
    let over = svg("sequenceDiagram\nNote over A,B: 둘<br>줄");
    assert!(over.contains("rx=\"3\"/>") && over.contains(">줄</text>"), "{over}");
    // 참여자 이름 바꾸기와 actor 의 둥근 상자
    let actor = svg("sequenceDiagram\nactor U as 사용자<br>둘째\nU->>U: x");
    assert!(actor.contains("class=\"pb ac\"") && actor.contains("rx=\"14\"") && actor.contains(">둘째</text>"), "{actor}");
}

// ---- 추가: 간격을 넓히는 규칙과 세로 쌓기를 손으로 계산한 값으로 지킨다(8글자 한글 = 12.5px x 8 = 100px) ----

const K7: &str = "가나다라마바사"; // 87.5px
const K8: &str = "가나다라마바사아"; // 100px
const K12: &str = "가나다라마바사아자차카타"; // 150px

#[test]
fn 추가_간격_참여자_상자가_크면_그만큼() {
    // 상자 너비 = 한글 12 글자 13px 156 + 24 = 180, 간격 = 90 + 90 + 28 = 208, 첫 x = 90 + 12 = 102
    let s = svg(&format!("sequenceDiagram\nparticipant A as {K12}\nparticipant B as {K12}"));
    assert!(s.contains("<line class=\"ll\" x1=\"102\" ") && s.contains("<line class=\"ll\" x1=\"310\" "), "{s}");
}

#[test]
fn 추가_간격_메시지와_번호() {
    // 메시지 간격 = 글 폭 + 32 = 132 → 둘째 x = 54 + 132 = 186
    let s = svg(&format!("sequenceDiagram\nA->>B: {K8}"));
    assert!(s.contains("<line class=\"ll\" x1=\"186\" "), "{s}");
    // 번호가 있으면 글 폭에 22 가 더해진다: 87.5 + 22 + 32 = 141.5 → 둘째 x = 195.5(→196), 화살 끝 193.5(→194), 가운데 124.75(→125), 틀 너비 249.5(→250)
    let s = svg(&format!("sequenceDiagram\nautonumber\nA->>B: {K7}"));
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 250 150\" "), "{s}");
    assert!(s.contains("<line class=\"ll\" x1=\"196\" ") && s.contains("x2=\"194\" y2=\"84\"/>"), "{s}");
    assert!(s.contains(&format!("<text class=\"mt\" x=\"125\" y=\"74\" text-anchor=\"middle\">1. {K7}</text>")), "{s}");
    // 번호가 없으면 87.5 + 32 = 119.5 < 120 이라 기본 간격이 이긴다
    assert!(svg(&format!("sequenceDiagram\nA->>B: {K7}")).contains("<line class=\"ll\" x1=\"174\" "));
}

#[test]
fn 추가_간격_혼자_메시지() {
    // 오른쪽에 참여자가 있으면 간격 = 87.5 + 56 = 143.5 → 둘째 x = 197.5(→198)
    let s = svg(&format!("sequenceDiagram\nparticipant A\nparticipant B\nA->>A: {K7}"));
    assert!(s.contains("<line class=\"ll\" x1=\"198\" "), "{s}");
    // 맨 오른쪽이면 바깥 여백 = 87.5 + 56 - 42 = 101.5 → 틀 너비 ceil(54 + 42 + 12 + 101.5) = 210, 높이 170
    let s = svg(&format!("sequenceDiagram\nA->>A: {K7}"));
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 210 170\" "), "{s}");
}

#[test]
fn 추가_간격_note() {
    // Note over A,B: 폭 170 → 간격 170 - 40 = 130 → 둘째 x = 184, 상자 너비 170, 왼쪽 (54+184)/2 - 85 = 34, 높이 26
    let s = svg(&format!("sequenceDiagram\nparticipant A\nparticipant B\nNote over A,B: {K12}"));
    assert!(s.contains("<line class=\"ll\" x1=\"184\" "), "{s}");
    assert!(s.contains("<rect class=\"nb\" x=\"34\" y=\"62\" width=\"170\" height=\"26\" rx=\"3\"/><text class=\"nt\" x=\"119\" y=\"77\" text-anchor=\"middle\">"), "{s}");
    // Note right of A / left of B: 간격 170 + 18 = 188 → 둘째 x = 242
    let s = svg(&format!("sequenceDiagram\nparticipant A\nparticipant B\nNote right of A: {K12}"));
    assert!(s.contains("<line class=\"ll\" x1=\"242\" ") && s.contains("<rect class=\"nb\" x=\"64\" y=\"62\" width=\"170\" "), "{s}");
    let s = svg(&format!("sequenceDiagram\nparticipant A\nparticipant B\nNote left of B: {K12}"));
    assert!(s.contains("<line class=\"ll\" x1=\"242\" ") && s.contains("<rect class=\"nb\" x=\"62\" y=\"62\" width=\"170\" "), "{s}");
    // 바깥 여백: 맨 오른쪽 Note = 170 + 12 - 42 = 140 → 틀 너비 54 + 42 + 12 + 140 = 248
    let s = svg(&format!("sequenceDiagram\nNote right of A: {K12}"));
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 248 150\" "), "{s}");
    // 맨 왼쪽 Note: 첫 x = 42 + 12 + 140 = 194
    let s = svg(&format!("sequenceDiagram\nNote left of A: {K12}"));
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 248 150\" ") && s.contains("<line class=\"ll\" x1=\"194\" "), "{s}");
    // 참여자 하나 위의 Note: 왼쪽 54 - 85 = -31, 틀이 왼쪽으로 나가 viewBox 가 -37 에서 시작한다(-31 - 6)
    let s = svg(&format!("sequenceDiagram\nNote over A: {K12}"));
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"-37 0 182 150\" "), "{s}");
    assert!(s.contains("<rect class=\"nb\" x=\"-31\" y=\"62\" width=\"170\" height=\"26\" rx=\"3\"/><text class=\"nt\" x=\"54\" y=\"77\" text-anchor=\"middle\">"), "{s}");
    // 영문 23 글자: 폭 172.5 + 20 → 왼쪽 -42.25, viewBox 시작 floor(-48.25) = -49, 너비 ceil(204.5) = 205
    let s = svg("sequenceDiagram\nNote over A: abcdefghijklmnopqrstuvw");
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"-49 0 205 150\" "), "{s}");
}

#[test]
fn 추가_세로_묶음_높이() {
    // 이름 없는 묶음은 14, box 는 0 을 쓴다: 62 + 14 + (메시지 38) + 2 + 10 → 아래 머리 126+6
    let s = svg("sequenceDiagram\nloop\nA->>B: x\nend");
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"-6 0 240 176\" "), "{s}");
    assert!(s.contains("<rect class=\"fr\" x=\"-2\" y=\"62\" width=\"232\" height=\"54\" rx=\"2\"/>"), "{s}");
    let s = svg("sequenceDiagram\nbox 묶음\nA->>B: x\nend");
    assert!(s.starts_with("<svg class=\"sq\" viewBox=\"0 0 228 150\" "), "{s}");
    // 안쪽 묶음은 여백이 8 줄어든다(깊이 1 → 48, 바깥 56)
    let s = svg("sequenceDiagram\nalt a\nloop b\nA->>B: x\nend\nend");
    assert!(s.contains("<rect class=\"fr\" x=\"6\" y=\"86\" width=\"216\" height=\"64\" rx=\"2\"/>"), "{s}");
    assert!(s.contains("<rect class=\"fr\" x=\"-2\" y=\"62\" width=\"232\" height=\"100\" rx=\"2\"/>"), "{s}");
}
