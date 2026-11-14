//! `board_running` 판정 시험 — 보드 본체와 알림 패널(`--notify`)을 프로세스 표 가짜로 가른다.
//!
//! `claude-board-hook` 은 실행 파일 전용 crate 라(lib 가 없다) 모듈 소스를 `#[path]` 로 넣어 시험한다.
//! 진짜 시스템 조회는 넘겨받는다(`board_running_with` 의 인자 조회 가짜) — 표는 `ProcessTable::from_entries`,
//! 인자는 가짜 상태표로 보드·패널 구별을 본다. 환경변수 건너뛰기(`CLAUDE_BOARD_NOTIFY_ALWAYS`)는 값이 `1` 일 때만이다.

#[path = "../src/board_running.rs"]
mod board_running;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;

use claude_board_core::notify::Panel;
use claude_board_core::proc::ProcessTable;

use board_running::{board_running_with, notify_always, panel_to_spawn, NOTIFY_ALWAYS_ENV};

/// 보드 본체의 실행 인자 모양(자동 실행은 `--background` 를 붙인다).
fn board_args(pid: u32) -> (u32, Vec<&'static str>) {
    (pid, vec!["ClaudeBoard.exe", "--background"])
}

/// 알림 패널의 실행 인자 모양(core `notify::panel_args` 가 만드는 것과 같다).
fn panel_args(pid: u32) -> (u32, Vec<&'static str>) {
    (pid, vec!["ClaudeBoard.exe", "--notify", "--title", "작업 완료", "--duration", "10"])
}

fn entry(pid: u32, name: &str) -> (u32, u32, String) {
    (pid, 1, name.to_string())
}

/// pid → 실행 인자 조회 가짜. 표에 없는 pid 는 None — 인자를 못 읽은 프로세스로 본다.
fn args_map(args: impl IntoIterator<Item = (u32, Vec<&'static str>)>) -> impl Fn(u32) -> Option<Vec<String>> {
    let map: HashMap<u32, Vec<String>> =
        args.into_iter().map(|(pid, argv)| (pid, argv.into_iter().map(String::from).collect())).collect();
    move |pid| map.get(&pid).cloned()
}

fn panel() -> Panel {
    Panel { app: PathBuf::from(r"C:\x\ClaudeBoard.exe"), args: vec!["--notify".to_string(), "--title".to_string()] }
}

/// 보드본체로 인정되는 프로세스가 하나 있으면 띄운다 — 대소문자 무시로 이름을 걸른다.
#[test]
fn board_process_makes_running() {
    let table = ProcessTable::from_entries([entry(10, "node.exe"), entry(20, "ClaudeBoard.exe"), entry(30, "CLAUDEBOARD.EXE")]);
    assert!(board_running_with(&table, args_map([board_args(20), board_args(30)])));
}

/// 이 기능이 막을 것 — 떠 있는 것이 알림 패널뿐이면 보드가 꺼져 있으므로 띄우지 않는다.
#[test]
fn notify_panel_alone_is_not_running() {
    let table = ProcessTable::from_entries([entry(10, "ClaudeBoard.exe")]);
    assert!(!board_running_with(&table, args_map([panel_args(10)])));
}

/// 보드 본체가 하나라도 있으면 패널이 같이 떠 있어도 띄운다(`any` 의 경계 — 0 이 아니라 1 부터).
#[test]
fn one_board_among_panels_is_running() {
    let table = ProcessTable::from_entries([entry(10, "ClaudeBoard.exe"), entry(20, "ClaudeBoard.exe")]);
    assert!(board_running_with(&table, args_map([panel_args(10), board_args(20)])));
}

/// 보드 이름의 프로세스가 없으면 띄우지 않는다 — 다른 이름의 프로세스는 본다 뿐이다.
#[test]
fn no_board_named_process_is_not_running() {
    let table = ProcessTable::from_entries([entry(10, "node.exe"), entry(11, "explorer.exe")]);
    assert!(!board_running_with(&table, args_map([board_args(10)])));
    assert!(!board_running_with(&ProcessTable::from_entries([]), args_map([])));
}

/// 실행 인자를 못 읽은 것(None)은 본체로 본다 — 모르는 것을 패널로 몰면 알림이 통째로 사라진다.
/// (못 읽음 = 0 개 아님: 못 읽은 것을 패널로 몰아 띄우기를 건너뛰면 눈치채지 못하므로 띄우는 쪽으로 논다)
#[test]
fn unreadable_args_count_as_board() {
    let table = ProcessTable::from_entries([entry(10, "ClaudeBoard.exe")]);
    assert!(board_running_with(&table, |_| None));
}

/// 패널 표지는 낱인자 `--notify` 여야 한다 — `--notifications`·`--notify=true` 는 본체다.
#[test]
fn panel_flag_must_be_a_whole_argument() {
    let table = ProcessTable::from_entries([entry(10, "ClaudeBoard.exe"), entry(11, "ClaudeBoard.exe")]);
    let imposter = args_map([(10, vec!["ClaudeBoard.exe", "--notifications", "off"]), (11, vec!["ClaudeBoard.exe", "--notify=true"])]);
    assert!(board_running_with(&table, imposter));
}

/// 이름 걸름을 지나지 못한 pid 는 실행 인자를 묻지 않는다 — 표 걸름이 먼저다.
#[test]
fn lookup_runs_only_for_board_named_pids() {
    let asked = Cell::new(0u32);
    let table = ProcessTable::from_entries([entry(10, "node.exe"), entry(11, "CLAUDEBOARD.EXE"), entry(20, "ClaudeBoard.exe")]);
    let answer = |_pid: u32| {
        asked.set(asked.get() + 1);
        Some(vec!["ClaudeBoard.exe".to_string()])
    };
    assert!(board_running_with(&table, answer));
    assert_eq!(asked.get(), 1, "이름을 걸러 찾은 것만 인자를 묻는다");
}

/// 건너뛰기 환경변수는 값이 정확히 `1` 일 때만 — `0`·빈 값·다른 값은 검사를 건너뛰지 않는다.
/// 환경변수는 프로세스 하나를 함께 쓰므로 이 시험 하나로만 만지고, 건너뛰기일 때는 표를 보지 않고 참이다.
#[test]
fn notify_always_env_bypasses_only_when_exactly_one() {
    std::env::remove_var(NOTIFY_ALWAYS_ENV);
    assert!(!notify_always());
    std::env::set_var(NOTIFY_ALWAYS_ENV, "1");
    assert!(notify_always());
    // 건너뛰기일 때는 프로세스 표를 보지 않고 참 — 이 기기에 보드가 없어도(CI 연기) 패널이 뜬다
    assert!(board_running::board_running());
    for value in ["0", "", "yes", "1 "] {
        std::env::set_var(NOTIFY_ALWAYS_ENV, value);
        assert!(!notify_always(), "{value:?} 는 건너뛰기가 아니다");
    }
    std::env::remove_var(NOTIFY_ALWAYS_ENV);
}

/// 띄우기 문 — 보드 검사가 참일 때만 패널을 돌려주고, 패널이 없으면 검사를 부르지 않는다.
#[test]
fn panel_spawns_only_when_the_board_check_passes() {
    let panel = panel();
    assert_eq!(panel_to_spawn(Some(panel.clone()), || true), Some(panel.clone()));
    assert_eq!(panel_to_spawn(Some(panel.clone()), || false), None);
    let asked = Cell::new(false);
    let check = || {
        asked.set(true);
        true
    };
    assert_eq!(panel_to_spawn(None, check), None);
    assert!(!asked.get(), "패널이 없는 낙방에서 보드 검사를 부르지 않는다");
}
