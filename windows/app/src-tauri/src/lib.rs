//! Claude Board for Windows — Tauri 백엔드.
//!
//! 인자 없음 = 보드(트레이·단일 실행·전역 단축키). `--notify …` = 알림 패널 하나만 띄우고 끝나는 별도 프로세스.
//! `--open-tab …` = 탭 열기 명령줄 — 답 한 줄 JSON 과 끝 코드로 끝난다(`open_tab_command_line`).
//! `--close-tab …` = 탭 닫기 명령줄 — 같은 답 한 줄 JSON 과 끝 코드로 끝난다(`close_tab_command_line`,
//! 닫으려고 보드를 켜지는 않는다 — 꺼져 있으면 곧바로 실패다).
//! 경로는 모두 `claude_board_core::paths` 를 거친다 — `CLAUDE_BOARD_HOME`·`CLAUDE_BOARD_NOTIFIER_DIR` 로 가짜 홈을 쓸 수 있다.

mod account_login;
mod accounts;
mod autopilot;
mod autopilot_git;
mod autopilot_connections;
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
mod open_tab;
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

/// `ClaudeBoard --open-tab …` 명령줄 길 — 답 한 줄 JSON 을 표준 출력에 찍고 ok 면 0 아니면 1 로 끝난다.
/// 이 길은 `main.rs` 가 단일 실행 플러그인 앞에서 가로챈다 — 떠 있는 보드의 단일 실행 검사를 치지 않는다.
pub fn open_tab_command_line(args: &[String]) -> i32 {
    // 요청 폴더는 언제나 설치판 자리를 본다 — 시험·개발 실행이라도 열리는 보드는 하나다(dev=false)
    let dir = open_tab::args::open_tab_dir(&claude_board_core::paths::local_app_data(), false);
    // 떠 있는 보드는 단일 실행 플러그인의 이름 있는 뮤텍스로 안다
    let board_running = || open_tab::cli::board_instance_mutex_exists();
    // 띄울 때는 지금 실행 파일을 분리로 다시 띄운다 — `--no-focus` 요청이면 `--background` 를 함께 줘 보드 창도 앞으로 나오지 않게 한다
    let no_focus = no_focus_before_separator(args);
    let launch_board = move || spawn_board_detached(no_focus);
    // 깨움은 침묵 — 두 번째 실행으로 깨우면 보드가 포커스를 뺏고, 보드는 1초 틱에 요청 폴더를 읽는다
    let wake = |_: &str| {};
    let (line, exit) =
        open_tab::cli::run_open_tab(args, &dir, &board_running, &launch_board, &wake, &claude_board_core::clock::now);
    print_open_tab_reply(&line);
    exit
}

/// `ClaudeBoard --close-tab …` 명령줄 길 — 답 한 줄 JSON 을 표준 출력에 찍고 ok 면 0 아니면 1 로 끝난다.
/// 이 길도 `main.rs` 가 단일 실행 플러그인 앞에서 가로챈다. 닫으려고 보드를 켜지지는 않는다 —
/// 꺼져 있으면 곧바로 board-off 다.
pub fn close_tab_command_line(args: &[String]) -> i32 {
    // 요청 폴더는 언제나 설치판 자리를 본다 — 시험·개발 실행이라도 열리는 보드는 하나다(dev=false)
    let dir = open_tab::close_cli::close_tab_request_dir(&claude_board_core::paths::local_app_data(), false);
    // 떠 있는 보드는 단일 실행 플러그인의 이름 있는 뮤텍스로 안다 — 꺼져 있으면 곧바로 실패다(켜지 않는다)
    let board_running = || open_tab::cli::board_instance_mutex_exists();
    let (line, exit) = open_tab::close_cli::run_close_tab(args, &dir, &board_running, &claude_board_core::clock::now);
    print_open_tab_reply(&line);
    exit
}

/// `--no-focus` 가 `--`(claude 인자 가름)보다 앞에 있는가 — 띄우기에만 안다. `--` 뒤의 `--no-focus` 는
/// claude 인자라 세지 않고, `--` 가 없으면 세지 않는다.
fn no_focus_before_separator(args: &[String]) -> bool {
    let Some(end) = args.iter().position(|a| a == "--") else {
        return false;
    };
    args[..end].iter().any(|a| a == "--no-focus")
}

/// 보드를 띄운다 — 지금 실행 파일을 분리로 다시 띄운다(`util::spawn_detached`). 표준 입출력 핸들을
/// 물려받지 않는다(stdin 은 `spawn_detached` 가, stdout·stderr 는 여기서 널로 끊는다). 띄움 성패.
fn spawn_board_detached(no_focus: bool) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut command = std::process::Command::new(exe);
    if no_focus {
        command.arg("--background");
    }
    command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    util::spawn_detached(command).is_ok()
}

/// 답 한 줄을 표준 출력에 찍는다 — 릴리스(GUI, 콘솔 없음) 빌드에도 패닉하지 않고 그냥 둔다.
/// 답을 파이프로 물려받은 부른 쪽(셸·자율주행 모델 세션)은 이 한 줄과 끝 코드를 본다
fn print_open_tab_reply(line: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg_list(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn no_focus_counted_only_before_the_separator() {
        // `--` 앞의 --no-focus 만 보드 띄우기에 안다 — 뒤의 것은 claude 인자다
        assert!(no_focus_before_separator(&arg_list(&["--no-focus", "--", "claude"])), "가름 앞");
        assert!(
            no_focus_before_separator(&arg_list(&["--cwd", "C:\\w", "--no-focus", "--", "claude", "--no-focus"])),
            "가름 앞 여러 인자 사이"
        );
        // `--` 뒤는 claude 인자라 세지 않는다
        assert!(!no_focus_before_separator(&arg_list(&["--", "--no-focus", "claude"])), "가름 뒤");
        // `--` 가 없으면 세지 않는다
        assert!(!no_focus_before_separator(&arg_list(&["--no-focus", "claude"])), "가름 없음");
        assert!(!no_focus_before_separator(&arg_list(&[])), "인자 없음");
    }

    #[cfg(windows)]
    #[test]
    fn board_mutex_probe_follows_a_made_mutex() {
        // 실제 상수 이름으로 뮤텍스를 만들면 probe 가 본다 — 이름이 어긋나면(플러그인과 다르면) 이 시험이 어긋난다.
        // 보드가 떠 있으면 이미 있는 뮤텍스의 핸들을 돌려받는다(어느 쪽이든 참이다)
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let wide: Vec<u16> = open_tab::cli::BOARD_INSTANCE_MUTEX.encode_utf16().chain([0]).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        assert!(!handle.is_null(), "뮤텍스를 만든다");
        assert!(open_tab::cli::board_instance_mutex_exists(), "만든 동안은 떠 있다고 본다");
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        // 핸들을 닫는다 — 보드가 없었다면 여기서 뮤텍스도 사라진다
    }
}
