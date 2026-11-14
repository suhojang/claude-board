//! claude-autopilot.exe — 사람이 PowerShell 에서 부르는 명령줄(콘솔 앱이라 끝날 때까지 기다리고 종료 코드를 받는다).
//! 하위 명령 다섯은 모두 `cli` 모듈이 소유한다.
fn main() {
    std::process::exit(claude_board_autopilot::cli::main(std::env::args().skip(1).collect()));
}
