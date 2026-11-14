//! 값이 있을 수도 있는 옵션(`--remote-control`·`--rc`) 넘기기 — macOS `state.py` 의 CARRY_OPT 이식.
//! 시나리오 6개는 upstream `hook-table/gen.py` 의 `check_mac_only` 표에 그대로 옮긴 것으로,
//! Windows 판에 시나리오 표가 없어(표에 넣지 않는 macOS 만의 규칙, gen.py EXCLUDED 비고) 여기서 바로 견준다.

use claude_board_core::state::{carry_args, resume_args};
use claude_board_core::Record;

/// gen.py check_mac_only 의 행 하나를 그대로 견준다.
fn assert_row(argv: &[&str], want: &[&str]) {
    let got = carry_args(argv);
    assert_eq!(got, want, "carry_args({argv:?})");
}

#[test]
fn bare_remote_control_is_carried_as_flag() {
    // 값이 없는 --remote-control 은 플래그 그대로 넘긴다
    assert_row(&["claude", "--remote-control"], &["--remote-control"]);
}

#[test]
fn remote_control_takes_next_arg_as_value() {
    // 다음 인자가 - 로 시작하지 않으면 그 인자가 값이다 — Claude Code 와 같은 읽기
    assert_row(&["claude", "--remote-control", "내 폰", "--model", "opus"], &["--remote-control", "내 폰", "--model", "opus"]);
}

#[test]
fn remote_control_eq_form_is_carried_whole() {
    // --x=v 형은 그대로 한 인자로 넘긴다
    assert_row(&["claude", "--remote-control=내 폰"], &["--remote-control=내 폰"]);
}

#[test]
fn rc_flag_is_carried() {
    // 짧은 이름 --rc 도 같은 옵션이고, 뒤의 플래그는 값을 훔치지 않는다
    assert_row(&["claude", "--rc", "--verbose"], &["--rc", "--verbose"]);
}

#[test]
fn rc_dedups_pairs_flags_and_eq_forms() {
    // 같은 (이름, 값) 쌍은 한 번만, 플래그 --rc 도 한 번만, --rc=a 형도 한 번만 — 꼴이 다르면 각각 넘긴다
    assert_row(
        &["claude", "--rc", "a", "--rc", "a", "--rc", "--rc=a", "--rc=a"],
        &["--rc", "a", "--rc", "--rc=a"],
    );
}

#[test]
fn similar_named_option_is_not_carried() {
    // --remote-control 로 시작하는 다른 옵션은 건드리지 않는다(정확히 같은 이름만)
    assert_row(&["claude", "--remote-control-session-name-prefix", "p"], &[]);
}

#[test]
fn carried_rc_args_reach_resume_cmd() {
    // carry_args 결과가 기록의 args 로 남고, resume_cmd 는 그 인자를 그대로 이어 받는다(중복 지움·플래그 보존 포함)
    let args: Vec<String> = carry_args(&["claude", "--rc", "a", "--rc", "a", "--rc", "--rc=a", "--rc=a"])
        .into_iter()
        .chain(carry_args(&["claude", "--remote-control"]))
        .collect();
    let mut rec = Record::new();
    rec.insert("session_id".into(), "sid-1".into());
    rec.insert("args".into(), args.into());
    assert_eq!(
        resume_args(&rec),
        ["--rc", "a", "--rc", "--rc=a", "--remote-control", "--resume", "sid-1"]
    );
}
