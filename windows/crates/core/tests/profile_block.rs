use claude_board_core::profile_block::{insert_block, remove_block, BEGIN, END};

const BODY: &str = "if (-not $env:CLAUDE_BOARD_TAB) { claude-board.cmd --background }";

fn block(eol: &str, body: &str) -> String {
    format!("{BEGIN}{eol}{body}{eol}{END}{eol}")
}

#[test]
fn insert_into_empty_file() {
    assert_eq!(insert_block("", BODY), block("\r\n", BODY));
}

#[test]
fn insert_appends_after_blank_line_in_lf_file() {
    let out = insert_block("Set-Alias ll ls\n", BODY);
    assert_eq!(out, format!("Set-Alias ll ls\n\n{}", block("\n", BODY)));
}

#[test]
fn insert_follows_crlf_file() {
    let out = insert_block("a\r\nb\r\n", BODY);
    assert_eq!(out, format!("a\r\nb\r\n\r\n{}", block("\r\n", BODY)));
}

#[test]
fn insert_uses_last_eol_in_mixed_file_and_keeps_user_lines() {
    let src = "a\r\nb\nc\r\nd\n";
    let out = insert_block(src, BODY);
    assert!(out.starts_with(src));
    assert!(out.ends_with(&block("\n", BODY)));
}

#[test]
fn insert_without_trailing_newline_only_adds_the_missing_newline() {
    let out = insert_block("last line", BODY);
    assert_eq!(out, format!("last line\r\n\r\n{}", block("\r\n", BODY)));
    let out = insert_block("last line\n", BODY);
    assert_eq!(out, format!("last line\n\n{}", block("\n", BODY)));
}

#[test]
fn insert_twice_equals_once() {
    for src in ["", "a\n", "a", "a\r\nb", "\u{FEFF}x\r\n", "\n\n"] {
        let once = insert_block(src, BODY);
        assert_eq!(insert_block(&once, BODY), once, "src={src:?}");
    }
}

#[test]
fn insert_replaces_outdated_body_in_place() {
    let src = format!("top\n\n{}bottom\n", block("\n", "old line 1\nold line 2"));
    let out = insert_block(&src, BODY);
    assert_eq!(out, format!("top\n\n{}bottom\n", block("\n", BODY)));
}

#[test]
fn replace_keeps_the_blocks_own_line_endings_and_missing_final_newline() {
    let src = format!("top\r\n\r\n{BEGIN}\r\nold\r\n{END}");
    let out = insert_block(&src, "new1\nnew2");
    assert_eq!(out, format!("top\r\n\r\n{BEGIN}\r\nnew1\r\nnew2\r\n{END}"));
}

#[test]
fn insert_collapses_duplicate_blocks_to_the_first() {
    let src = format!("a\n\n{}x\n\n{}y\n", block("\n", "one"), block("\n", "two"));
    let out = insert_block(&src, BODY);
    assert_eq!(out, format!("a\n\n{}x\ny\n", block("\n", BODY)));
}

#[test]
fn bom_is_kept_by_insert_and_remove() {
    let src = "\u{FEFF}Set-Alias ll ls\r\n";
    let out = insert_block(src, BODY);
    assert!(out.starts_with("\u{FEFF}Set-Alias ll ls\r\n"));
    assert_eq!(out.matches('\u{FEFF}').count(), 1);
    assert_eq!(remove_block(&out), src);
}

#[test]
fn bom_only_file_gets_block_after_bom() {
    let out = insert_block("\u{FEFF}", BODY);
    assert_eq!(out, format!("\u{FEFF}{}", block("\r\n", BODY)));
    assert_eq!(remove_block(&out), "\u{FEFF}");
}

#[test]
fn remove_roundtrips_files_that_end_with_newline() {
    for src in ["", "a\n", "a\r\nb\r\n", "a\r\nb\nc\r\n", "\n", "\n\n", "\u{FEFF}가나다\r\n", "x\n\n\n"] {
        assert_eq!(remove_block(&insert_block(src, BODY)), src, "src={src:?}");
    }
}

#[test]
fn remove_after_insert_into_file_without_final_newline_keeps_every_line() {
    assert_eq!(remove_block(&insert_block("a\r\nb", BODY)), "a\r\nb\r\n");
}

#[test]
fn remove_without_block_returns_input_unchanged() {
    for src in ["", "a", "a\r\nb\n", "\u{FEFF}x"] {
        assert_eq!(remove_block(src), src);
    }
}

#[test]
fn remove_deletes_every_block() {
    let src = format!("a\n\n{}b\n\n{}c\n", block("\n", "one"), block("\n", "two"));
    assert_eq!(remove_block(&src), "a\nb\nc\n");
}

#[test]
fn remove_block_at_start_and_at_eof_without_newline() {
    assert_eq!(remove_block(&format!("{}after\n", block("\n", BODY))), "after\n");
    let src = format!("before\n\n{BEGIN}\nx\n{END}");
    assert_eq!(remove_block(&src), "before\n");
}

#[test]
fn remove_keeps_user_blank_line_before_hand_written_block_at_file_start() {
    let src = format!("\n{}", block("\n", BODY));
    assert_eq!(remove_block(&src), "\n");
}

#[test]
fn remove_preserves_mixed_line_endings_around_the_block() {
    let src = format!("a\r\nb\n\n{}c\r\nd", block("\n", BODY));
    assert_eq!(remove_block(&src), "a\r\nb\nc\r\nd");
}

#[test]
fn begin_without_end_is_left_alone_and_never_eats_user_text() {
    let src = format!("one\n{BEGIN}\nuser line 1\nuser line 2\n");
    assert_eq!(remove_block(&src), src);
    let out = insert_block(&src, BODY);
    assert!(out.starts_with(&src));
    assert!(out.ends_with(&block("\n", BODY)));
    // 다시 넣어도 짝 없는 줄을 건드리지 않고, 새 블록 하나만 바뀐다
    assert_eq!(insert_block(&out, "other"), format!("{src}\n{}", block("\n", "other")));
    assert_eq!(remove_block(&out), src);
}

#[test]
fn end_without_begin_is_user_text() {
    let src = format!("a\n{END}\nb\n");
    assert_eq!(remove_block(&src), src);
    let out = insert_block(&src, BODY);
    assert!(out.starts_with(&src));
    assert_eq!(remove_block(&out), src);
}

#[test]
fn end_before_begin_makes_no_block() {
    let src = format!("{END}\nx\n{BEGIN}\n");
    assert_eq!(remove_block(&src), src);
}

#[test]
fn marker_text_inside_a_quoted_string_is_not_a_marker() {
    let src = format!("Write-Host \"{BEGIN}\"\nWrite-Host '{END}'\n$x = \"{BEGIN} {END}\"\n");
    assert_eq!(remove_block(&src), src);
    let out = insert_block(&src, BODY);
    assert!(out.starts_with(&src));
    assert_eq!(remove_block(&out), src);
}

#[test]
fn indented_marker_lines_count_as_markers() {
    let src = format!("a\n\n  {BEGIN}  \nold\n\t{END}\nz\n");
    assert_eq!(insert_block(&src, "new"), format!("a\n\n  {BEGIN}  \nnew\n\t{END}\nz\n"));
}

#[test]
fn marker_lines_inside_the_body_are_dropped_not_nested() {
    let body = format!("keep\n{BEGIN}\n{END}\nalso keep");
    let out = insert_block("", &body);
    assert_eq!(out, block("\r\n", "keep\r\nalso keep"));
    assert_eq!(remove_block(&out), "");
}

#[test]
fn body_with_trailing_newline_and_empty_body() {
    assert_eq!(insert_block("", "x\n"), block("\r\n", "x"));
    assert_eq!(insert_block("", ""), format!("{BEGIN}\r\n{END}\r\n"));
    assert_eq!(insert_block(&insert_block("", ""), BODY), block("\r\n", BODY));
}

#[test]
fn unicode_user_lines_and_body_survive() {
    let src = "# 한글 주석 🙂\r\n$이름 = '가나다'\r\n";
    let body = "Write-Host '보드 🙂'";
    let out = insert_block(src, body);
    assert!(out.starts_with(src));
    assert!(out.contains(body));
    assert_eq!(remove_block(&out), src);
}

#[test]
fn lone_cr_is_not_a_line_break() {
    let src = "a\rb\n";
    let out = insert_block(src, BODY);
    assert!(out.starts_with(src));
    assert_eq!(remove_block(&out), src);
}

#[test]
fn very_large_file_is_handled_and_roundtrips() {
    let mut src = String::new();
    for i in 0..200_000 {
        src.push_str(&format!("$v{i} = {i}{}", if i % 3 == 0 { "\r\n" } else { "\n" }));
    }
    let out = insert_block(&src, BODY);
    assert!(out.starts_with(&src));
    assert_eq!(insert_block(&out, BODY), out);
    assert_eq!(remove_block(&out), src);
}
