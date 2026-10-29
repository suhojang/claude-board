//! Claude Board for Windows — Tauri 백엔드.
//!
//! 인자 없음 = 보드(트레이·단일 실행·전역 단축키). `--notify …` = 알림 패널 하나만 띄우고 끝나는 별도 프로세스.
//! 경로는 모두 `claude_board_core::paths` 를 거친다 — `CLAUDE_BOARD_HOME`·`CLAUDE_BOARD_NOTIFIER_DIR` 로 가짜 홈을 쓸 수 있다.

mod account_login;
mod accounts;
mod autoswitch;
mod board;
mod drive;
mod hotkeys;
mod login_item;
mod login_output;
mod mdrender;
mod model;
mod notify;
mod open_session;
mod proc_wait;
mod pty;
mod readers;
mod ring;
#[cfg(test)]
mod rules_table;
mod session_action;
mod settings;
mod shutdown;
mod store;
mod terminal;
mod terms;
mod test_claude;
mod tray;
mod updater;
mod usage;
mod util;
mod win;
mod window;

pub fn run() {
    let argv: Vec<String> = std::env::args().collect();
    // 앱 자원(프론트 빌드)은 한 번만 넣는다 — 두 모드가 같은 context 를 쓴다
    let context = tauri::generate_context!();
    if argv.iter().any(|a| a == "--notify") {
        notify::run(&argv, context);
    } else {
        board::run(argv, context);
    }
}
