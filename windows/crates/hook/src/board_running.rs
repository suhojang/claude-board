//! 알림 패널을 띄우기 전 보드가 떠 있는지 본다 (SPEC §3.6).
//!
//! macOS 는 패널(claude-notifier)이 보드가 떠 있는지 스스로 확인하고, Windows 는 hook 이 띄우기 전에 여기서 본다 —
//! 보드가 꺼져 있을 때 hook 이 알림 패널(`ClaudeBoard.exe --notify …`)만 띄우는 것을 막는다.

use claude_board_core::notify::Panel;
use claude_board_core::proc::{self, ProcessTable};

/// 보드 실행 파일 이름(uninstall 의 보드 종료 때 걸르는 것과 같은 이름).
const BOARD_EXE: &str = "ClaudeBoard.exe";

/// 알림 패널 표지 — 패널은 `ClaudeBoard.exe --notify …` 로 뜬다(core `notify::panel_args` 가 만드는 첫 인자).
const PANEL_FLAG: &str = "--notify";

/// 보드 검사를 건너뛰는 환경변수 — CI 연기(smoke)에서 보드 없이 패널을 띄울 때 쓴다. 값은 `1`.
pub const NOTIFY_ALWAYS_ENV: &str = "CLAUDE_BOARD_NOTIFY_ALWAYS";

/// 보드 검사를 건너뛴다(`CLAUDE_BOARD_NOTIFY_ALWAYS` 의 값이 정확히 `1` 일 때만 — `0`·빈 값은 건너뛰기가 아니다).
pub fn notify_always() -> bool {
    std::env::var(NOTIFY_ALWAYS_ENV).is_ok_and(|v| v == "1")
}

/// 패널을 띄울 것인가 — 패널이 있고 보드 검사가 참일 때만 그 패널을 돌려준다(건너뛸 때는 None).
/// 패널이 없으면 검사를 부르지 않는다(음소거·걸름에서 이미 낙방한 때에 프로세스 표를 보지 않는다).
/// 보드 검사는 넘겨받는다: 진짜는 [`board_running`], 시험은 가짜 함수로 바꿔 판단을 본다.
pub fn panel_to_spawn(panel: Option<Panel>, board_running: impl Fn() -> bool) -> Option<Panel> {
    panel.filter(|_| board_running())
}

/// 패널을 띄울 때의 보드 검사 — 보드 본체가 떠 있으면 참.
/// [`notify_always`] 면 프로세스 표를 보지 않고 참으로 검사를 건너뛴다.
pub fn board_running() -> bool {
    notify_always() || board_running_with(&ProcessTable::capture(), proc::command_line)
}

/// [`board_running`] 의 판정을 프로세스 표와 pid → 실행 인자 조회 가짜로 한다 — 시험은
/// [`ProcessTable::from_entries`] 표와 가짜 조회를 넘기고, 진짜는 `ProcessTable::capture()` 표와
/// `proc::command_line` 조회다(SPEC §2.1).
///
/// 떠 있는 `ClaudeBoard.exe` 가운데 `--notify` 인자가 없는 것이 하나라도 있으면 본체다.
/// 실행 인자를 못 읽은 것(`None`)은 본체로 본다 — 못 읽어 패널로 몰아 넣으면 알림이 통째로 사라지고,
/// 패널은 우리가 띄운 것이라 인자를 읽을 수 있다. 못 읽는 것은 권한 문제뿐이라 비용도 본체 뿐이다.
pub fn board_running_with(table: &ProcessTable, command_line: impl Fn(u32) -> Option<Vec<String>>) -> bool {
    table.pids_named(BOARD_EXE).iter().any(|&pid| {
        command_line(pid).is_none_or(|args| !args.iter().any(|a| a == PANEL_FLAG))
    })
}
