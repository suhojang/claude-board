//! 자율주행 지시 고치기·지난 판 차이 규칙(`rules::instruction`) 시험.
//! 원본: `Tests/AutopilotRulesTests/StartTests.swift` 의 `InstructionTests`(표 7줄을 도는 `표`, `머리`, `큰글`). 전용 시험 파일은 없다.
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 경계(정규 등가·CRLF·한계값·Windows 줄바꿈)를 이 이식이 따로 지킨다.

use claude_board_autopilot::rules::instruction::{
    diff_count, instruction_head, instruction_save_problem, line_diff, DiffCount, DiffKind, DiffLine,
};

struct DiffRow {
    note: &'static str,
    old: &'static str,
    new: &'static str,
    want: &'static str, // 줄마다 " a" "+b" "-c" 를 | 로
}

const DIFF_ROWS: [DiffRow; 7] = [
    DiffRow { note: "같음", old: "a\nb", new: "a\nb", want: " a| b" },
    DiffRow { note: "앞뒤 빈 줄은 견주지 않는다(엔진이 자른다)", old: "\n\na\nb\n", new: "a\nb", want: " a| b" },
    DiffRow { note: "한 줄 바꿈: 뺀 줄이 더한 줄보다 앞", old: "a\nb\nc", new: "a\nB\nc", want: " a|-b|+B| c" },
    DiffRow { note: "끝에 더함", old: "a", new: "a\nb", want: " a|+b" },
    DiffRow { note: "처음 판(지난 판이 빔)", old: "", new: "a\nb", want: "+a|+b" },
    DiffRow {
        note: "와이어프레임 예: 40행을 바꾸고 21행 규칙을 더함",
        old: "앞선다\n14행\n40행 파일로\n47행",
        new: "앞선다\n14행\n40행 result.json\n47행\n21행 느린 시험",
        want: " 앞선다| 14행|-40행 파일로|+40행 result.json| 47행|+21행 느린 시험",
    },
    DiffRow { note: "CRLF 는 같은 줄로", old: "a\r\nb", new: "a\nb", want: " a| b" },
];

fn render(d: &[DiffLine]) -> String {
    d.iter()
        .map(|l| format!("{}{}", if l.kind == DiffKind::Add { "+" } else if l.kind == DiffKind::Del { "-" } else { " " }, l.text))
        .collect::<Vec<_>>()
        .join("|")
}

// ── 줄 차이 (Swift `표`) ─────────────────────
#[test]
fn 표() {
    assert_eq!(DIFF_ROWS.len(), 7, "Swift diffRows 는 7줄이다");
    for r in &DIFF_ROWS {
        assert_eq!(render(&line_diff(r.old, r.new)), r.want, "{}", r.note);
    }
}

// ── 머리 줄과 저장 확인 (Swift `머리`) ─────────────────────
#[test]
fn 머리() {
    assert_eq!(instruction_head(Some("a\nb"), Some("a\nc\nd"), "10월 20일"), "지난 판(10월 20일)과 차이 +2 −1");
    assert_eq!(instruction_head(Some("a"), Some("a\n"), "10월 20일"), "지난 판(10월 20일)과 같음");
    assert_eq!(instruction_head(None, Some("a"), ""), "지난 판 없음 · 이 프로젝트에서 처음 쓰는 지시");
    assert_eq!(instruction_head(None, None, ""), "지시 없음 · 공통 규칙만으로 돕니다");
    assert_eq!(instruction_save_problem("  \n ").as_deref(), Some("빈 지시는 저장하지 않습니다. 고친 글을 넣어 주세요"));
    assert_eq!(instruction_save_problem("이 글이 앞선다"), None);
    assert_eq!(
        instruction_save_problem(&"가".repeat(100_001)).as_deref(),
        Some("지시가 너무 깁니다(100001자, 10만 자까지)")
    );
}

// ── 줄이 아주 많으면 모두 빼고 모두 더한 것으로 (Swift `큰글`) ─────────────────────
#[test]
fn 큰글() {
    let a = (0..2100).map(|i| format!("a{i}")).collect::<Vec<_>>().join("\n");
    let b = (0..2100).map(|i| format!("b{i}")).collect::<Vec<_>>().join("\n");
    let c = diff_count(&line_diff(&a, &b));
    assert!(c.add == 2100 && c.del == 2100);
}

// ── 추가: Swift 시험에 없는 경계 ─────────────────────

#[test]
fn extra_literals_match_swift_bytes() {
    // 빼기 기호는 U+2212(−), 가운뎃점은 U+00B7(·)
    let h = instruction_head(Some("a"), Some("b"), "d");
    assert_eq!(h, "지난 판(d)과 차이 +1 \u{2212}1");
    assert!(h.contains('\u{2212}') && !h.contains('-'));
    assert!(instruction_head(None, None, "").contains(" \u{b7} "));
    assert_eq!(instruction_head(Some("a"), None, "10월 20일"), "지난 판(10월 20일)이 있으나 지금 세트에 지시가 없습니다");
}

#[test]
fn extra_diff_kinds_and_tie_order() {
    assert_eq!(line_diff("a", "a"), vec![DiffLine::new(DiffKind::Same, "a")]);
    // 뺀 줄이 더한 줄보다 앞: 완전히 다른 두 줄
    assert_eq!(
        line_diff("x", "y"),
        vec![DiffLine::new(DiffKind::Del, "x"), DiffLine::new(DiffKind::Add, "y")]
    );
    // 양쪽이 비면 차이 없음
    assert!(line_diff("", "  \n\t\n").is_empty());
    assert_eq!(diff_count(&[]), DiffCount { add: 0, del: 0 });
    // 모두 지움
    assert_eq!(render(&line_diff("a\nb", "")), "-a|-b");
}

#[test]
fn extra_blank_lines_inside_are_kept_and_edges_trimmed() {
    assert_eq!(render(&line_diff("  \n  a\n\nb  \n\n", "a\n\nb")), " a| | b");
    // 안쪽 빈 줄을 더하면 더한 줄로 보인다
    assert_eq!(render(&line_diff("a\nb", "a\n\nb")), " a|+| b");
}

#[test]
fn extra_windows_line_endings_and_lone_cr() {
    // 줄마다 CRLF 여도 LF 와 같은 줄
    assert_eq!(render(&line_diff("a\r\nb\r\nc\r\n", "a\nb\nc")), " a| b| c");
    // 줄 끝 `\r` 는 하나만 뗀다
    let d = line_diff("a\r\r\nb", "a\nb");
    assert_eq!(render(&d), "-a\r|+a| b");
    // 빈 줄(CRLF)도 같은 빈 줄
    assert_eq!(render(&line_diff("a\r\n\r\nb", "a\n\nb")), " a| | b");
}

#[test]
fn extra_unicode_whitespace_trim_matches_foundation() {
    // U+00A0·U+3000·U+2028 은 앞뒤에서 잘린다(Foundation .whitespacesAndNewlines)
    assert_eq!(render(&line_diff("\u{a0}\u{3000}a\u{2028}", "a")), " a");
    assert!(instruction_save_problem("\u{a0}\u{3000}\u{2028}\u{85}").is_some());
}

#[test]
fn extra_canonical_equivalence_lines_are_same() {
    // 한글 완성형 "가"(U+AC00) 와 조합형 ㄱ+ㅏ(U+1100 U+1161) 는 Swift String 에서 같은 글이다
    let composed = "\u{ac00}";
    let decomposed = "\u{1100}\u{1161}";
    let d = line_diff(composed, decomposed);
    assert_eq!(d, vec![DiffLine::new(DiffKind::Same, composed)]);
    // 같은 줄은 앞 글(지난 판)의 모양을 그대로 돌려준다
    assert_eq!(instruction_head(Some(composed), Some(decomposed), "d"), "지난 판(d)과 같음");
}

#[test]
fn extra_big_limit_boundary() {
    // 곱이 정확히 4,000,000 이면 견주고, 넘으면 모두 빼고 모두 더한다
    let mk = |n: usize, p: &str| (0..n).map(|i| format!("{p}{i}")).collect::<Vec<_>>().join("\n");
    let same_a = mk(2000, "s");
    let c = diff_count(&line_diff(&same_a, &same_a));
    assert_eq!(c, DiffCount { add: 0, del: 0 }, "2000 x 2000 = 400만 은 견준다");
    let over = mk(2001, "s");
    let c2 = diff_count(&line_diff(&over, &mk(2000, "s")));
    assert_eq!(c2, DiffCount { add: 2000, del: 2001 }, "2001 x 2000 > 400만 은 전부 다름으로 본다");
    // 곱이 400만 + 1(41 × 97561)이면 견주지 않는다: 앞 41줄이 같아도 모두 뺀 것으로 본다
    let c3 = diff_count(&line_diff(&mk(41, "s"), &mk(97_561, "s")));
    assert_eq!(c3, DiffCount { add: 97_561, del: 41 });
    // 한쪽이 비면 곱이 0 이라 어떤 크기여도 견준다
    let big = mk(10_000, "z");
    assert_eq!(diff_count(&line_diff("", &big)), DiffCount { add: 10_000, del: 0 });
}

#[test]
fn extra_save_problem_length_edges() {
    // 정확히 10만 자는 통과, 앞뒤 빈칸은 세지 않는다
    assert_eq!(instruction_save_problem(&"가".repeat(100_000)), None);
    assert_eq!(instruction_save_problem(&format!("  {}\n\n", "a".repeat(100_000))), None);
    assert_eq!(
        instruction_save_problem(&format!("  {}\n", "a".repeat(100_001))).as_deref(),
        Some("지시가 너무 깁니다(100001자, 10만 자까지)")
    );
    // `\r\n` 은 Swift 에서 한 글자(Character)다
    // 글자(스칼라)로는 120001 이지만 Swift 로는 80001 이다
    assert_eq!(instruction_save_problem(&format!("a{}", "\r\nb".repeat(40_000))), None);
    // 안쪽 CRLF 가 많아도 Swift 로 10만 1 자를 넘으면 걸린다(1 + 2 × 50000 = 100001)
    assert_eq!(
        instruction_save_problem(&format!("a{}", "\r\nb".repeat(50_000))).as_deref(),
        Some("지시가 너무 깁니다(100001자, 10만 자까지)")
    );
}
