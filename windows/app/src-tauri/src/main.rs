// 릴리스 빌드에서 콘솔 창이 뜨지 않게 한다(GUI 앱)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--open-tab`·`--close-tab …` 은 보드·알림 패널과 다른 앱이라 단일 실행 검사를 치지 않는다 —
    // 보드(board.rs)가 띄우는 단일 실행 플러그인 앞에서 가로챈다(답 한 줄 JSON, 끝 코드로 끝난다)
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("--open-tab") {
        std::process::exit(app_lib::open_tab_command_line(&argv[2..]));
    }
    if argv.get(1).map(String::as_str) == Some("--close-tab") {
        std::process::exit(app_lib::close_tab_command_line(&argv[2..]));
    }
    app_lib::run()
}
