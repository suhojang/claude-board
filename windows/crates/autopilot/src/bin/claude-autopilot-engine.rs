//! claude-autopilot-engine.exe — 작업 스케줄러가 1분마다 띄우는 엔진. 콘솔 창이 뜨지 않게 창 없는 하위 시스템이다.
//! `run <실행 폴더> <해시>` 한 가지만 받고, 본체는 `cli` 모듈이 소유한다. 다른 명령은 사용법 오류(2)다.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("run") {
        std::process::exit(2);
    }
    std::process::exit(claude_board_autopilot::cli::main(args));
}
