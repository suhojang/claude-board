//! `ClaudeBoard --open-tab` 명령줄 길 — 규칙(`args.rs`)의 읽기·요청 꼴로 요청 파일을 놓고 답을 기다린다.
//! 원본(macOS): `Sources/ClaudeBoard/OpenTab.swift` 16-116 줄(`openTabCommandLine` · `prepareRequestDir` ·
//! `writeRequest` · `awaitReply`). `main.rs` 가 첫 인자 `--open-tab` 으로 이 길을 부른다.
//! 답 한 줄 JSON 을 표준 출력에 찍고 ok 면 0, 아니면 1 로 끝난다 — 부른 쪽(셸·자율주행 모델 세션)이 본다.
//! 켜기·깨우기 같은 바깥 이는 어댑터(`lib.rs` `open_tab_command_line`)가 넣는다 — `board_running` 은
//! 단일 실행 플러그인이 만드는 이름 있는 뮤텍스([`BOARD_INSTANCE_MUTEX`]), `launch_board` 는 지금 실행
//! 파일(`--no-focus` 요청이면 `--background` 를 얹어) 분리로 다시 띄우는 것, `wake` 는 침묵이다:
//! 떠 있는 보드를 두 번째 실행으로 깨우면 보드 창이 앞으로 나와 포커스를 뺏고(`--no-focus` 요청에 어긋난다),
//! 보드는 1초 틱에 요청 폴더를 스쳐 가면서 알아서 읽으므로 깨울 필요가 없다.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::args::{parse_open_tab_args, OpenTabFailure, OpenTabReply, OpenTabRequest};

/// 답 파일을 검사하는 곈 — 보드가 요청 폴더를 훑는 주기와 맞추듯 자주 본다(원본 `usleep(100_000)`)
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 답을 기다리는 동안 보드를 다시 깨우는 곈(초) — 보드가 막 뜨는 중이면 앞 깨움을 놓칠 수 있다(원본 OpenTab.swift:93)
const RE_WAKE_EVERY_SECS: f64 = 1.0;

/// 보드가 떠 있는지 보는 이름 있는 뮤텍스 — Windows 의 단일 실행 플러그인(`tauri-plugin-single-instance`
/// 2.5.4)은 식별자 뒤에 `-sim` 을 붙인 이름으로 뮤텍스를 만들고 보드가 끝나면 닫는다.
/// 시험·개발 실행에서 홈을 환경변수로 바꿔도 이 이름은 언제나 설치판과 같다.
pub const BOARD_INSTANCE_MUTEX: &str = "com.claude.board-sim";

/// 보드가 떠 있는지 — [`BOARD_INSTANCE_MUTEX`] 이름의 뮤텍스가 열리는지(`OpenMutexW`)로 안다.
pub fn board_instance_mutex_exists() -> bool {
    open_named_mutex_exists(BOARD_INSTANCE_MUTEX)
}

#[cfg(windows)]
fn open_named_mutex_exists(name: &str) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

    // 이름은 널 글자로 끝나는 UTF-16 이어야 한다(Windows API)
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, wide.as_ptr()) };
    if handle.is_null() {
        false
    } else {
        unsafe { CloseHandle(handle) };
        true
    }
}

/// Windows 밖(macOS 개발 기기의 `cargo check`)에서는 보드 뮤텍스가 없다고 본다 — 이 길은 Windows 전용이다.
#[cfg(not(windows))]
fn open_named_mutex_exists(_name: &str) -> bool {
    false
}

/// `--open-tab` 뒤의 인자로 탭을 연다. `args` 는 `--open-tab` 뒤의 인자, `dir` 은 요청 폴더
/// (`args::open_tab_dir`), 보드를 이는 부분은 진짜 어댑터가 넣는다: `board_running` · `launch_board` ·
/// `wake`, 시계는 `now`(1970 기준 초). 돌려주는 짝은 (답 한 줄 JSON, 끝 코드 — ok 면 0 아니면 1) 다.
pub fn run_open_tab(
    args: &[String],
    dir: &Path,
    board_running: &dyn Fn() -> bool,
    launch_board: &dyn Fn() -> bool,
    wake: &dyn Fn(&str),
    now: &dyn Fn() -> f64,
) -> (String, i32) {
    let options = match parse_open_tab_args(args, &caller_working_dir()) {
        Ok(options) => options,
        Err(failure) => return reply_exit(&failure.reply()),
    };
    if let Err(error) = std::fs::create_dir_all(dir) {
        let why = format!("요청 폴더를 만들지 못했습니다: {} {error}", dir.display());
        return reply_exit(&OpenTabFailure::Refused(why).reply());
    }
    // 보드가 요청 파일 이름을 is_session_id 로 걸러 읽으므로 id 는 UUID 꼴이어야 한다
    let id = super::close_cli::new_request_id();
    // --no-focus 는 켤 때만 칸을 싣는다(끄면 None — 3.3.3 까지의 요청 JSON 과 같다)
    let request = OpenTabRequest {
        id: id.clone(),
        cwd: options.cwd.clone(),
        title: options.title.clone(),
        argv: options.argv.clone(),
        at: now(),
        no_focus: options.no_focus.then_some(true),
    };
    let body = serde_json::to_string(&request).expect("요청 꼴은 언제나 JSON 이 된다");
    if !write_request_once(dir, &id, &body) {
        let why = format!("요청을 쓰지 못했습니다: {}", dir.display());
        return reply_exit(&OpenTabFailure::Refused(why).reply());
    }
    if !board_running() {
        if !options.launch {
            return board_off_taking_back(dir, &id, "보드가 꺼져 있습니다. --launch 를 주면 켜고 엽니다");
        }
        if !launch_board() {
            return board_off_taking_back(dir, &id, "보드를 켜지 못했습니다");
        }
    }
    reply_exit(&await_reply(dir, &id, options.timeout, wake, now))
}

/// 명령을 부른 폴더 — 상대 --cwd 를 붙일 기준(원본 parseOpenTabArgs 의 here: FileManager.currentDirectoryPath)
fn caller_working_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 답을 한 줄로, 끝 코드는 ok 면 0 아니면 1(원본 finish: OpenTab.swift:17-20)
fn reply_exit(reply: &OpenTabReply) -> (String, i32) {
    (reply.line(), i32::from(!reply.ok))
}

/// 요청을 `<id>.tmp` 에 새로 쓰고(`<id>.tmp` 가 있으면 실패 — 새 파일로만 쓴다) `<id>.req.json` 으로 바꾼다.
/// 보드가 쓰다 만 요청을 읽지 않게(원본 writeRequest: OpenTab.swift:80-88). 성공으로 바꿨으면 참
fn write_request_once(dir: &Path, id: &str, body: &str) -> bool {
    let tmp_path = dir.join(format!("{id}.tmp"));
    let req_path = dir.join(format!("{id}.req.json"));
    if !write_new_alone(&tmp_path, body) {
        return false;
    }
    match std::fs::rename(&tmp_path, &req_path) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&tmp_path);
            false
        }
    }
}

/// 새 파일로만 쓴다(있으면 실패, 덮어쓰지 않는다 — 원본 writeNew 는 O_EXCL). 임시 파일은 어떤 경우에도 남기지 않는다
fn write_new_alone(path: &Path, body: &str) -> bool {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(body.as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// 보드가 없어 답을 볼 뒤가 없다 — 이미 쓴 요청을 거둔(지운) 뒤 board-off 로 끝낸다.
/// 원본은 쓰기 전에 보고 온다(OpenTab.swift:38-41) — 요청을 남기지 않으려면 어느 쪽이든 거두는 일은 필요하다
fn board_off_taking_back(dir: &Path, id: &str, why: &str) -> (String, i32) {
    let _ = std::fs::remove_file(dir.join(format!("{id}.req.json")));
    reply_exit(&OpenTabFailure::BoardOff(why.to_string()).reply())
}

/// 답을 기다린다 — 먼저 보드를 깨우고 1초 간 다시 깨워 보드가 막 뜨는 중 놓친 깨움을 만회한다(원본 OpenTab.swift:93-108).
/// 시간 안에 답이 없으면 보드가 아직 가져가지 않은 요청을 거둬 나중에 처리되지 않게 한다.
/// `now` 는 루프마다 딱 한 번 불린다 — 가짜 시계(시험)가 답 검사와 다시 깨움 사이 두 번 흘러가지 않게
fn await_reply(
    dir: &Path,
    id: &str,
    timeout_secs: f64,
    wake: &dyn Fn(&str),
    now: &dyn Fn() -> f64,
) -> OpenTabReply {
    let reply_path = dir.join(format!("{id}.reply.json"));
    let req_path = dir.join(format!("{id}.req.json"));
    let started = now();
    let deadline = started + timeout_secs;
    let mut posted = started;
    wake(id);
    loop {
        let now_secs = now();
        if now_secs >= deadline {
            break;
        }
        if let Some(reply) = take_reply(&reply_path) {
            let _ = std::fs::remove_file(&reply_path);
            return reply;
        }
        if now_secs - posted >= RE_WAKE_EVERY_SECS {
            wake(id);
            posted = now_secs;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    timeout_reply(&req_path, timeout_secs)
}

/// 답 파일을 읽어 답 꼴로 — 없거나 꼴이 틀리면 아직 없는 것으로 본다(보드가 쓰는 중일 수 있다 — 원본 try? → nil)
fn take_reply(reply_path: &Path) -> Option<OpenTabReply> {
    let body = std::fs::read_to_string(reply_path).ok()?;
    serde_json::from_str(&body).ok()
}

/// 시간 안에 답이 없다 — 요청 파일이 남아 있으면(보드가 아직 안 가져갔다) 거둔다. 거둔 성패로 why 를 나눈다:
/// 가져갔는데 답이 늦은 건 탭이 열렸을 수 있다(원본 OpenTab.swift:112-116)
fn timeout_reply(req_path: &Path, timeout_secs: f64) -> OpenTabReply {
    let seconds = timeout_secs as i64;
    let why = if std::fs::remove_file(req_path).is_ok() {
        format!("보드가 {seconds}초 안에 요청을 가져가지 않았습니다(요청을 거뒀습니다)")
    } else {
        format!("보드가 요청을 가져갔지만 {seconds}초 안에 답하지 않았습니다(탭이 열렸을 수 있습니다)")
    };
    OpenTabFailure::Timeout(why).reply()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;

    use super::*;
    use crate::open_tab::plan::is_session_id;
    use crate::util::short_id;

    /// 시험용 요청 폴더 — 부를 때마다 이름이 담보된다(short_id 는 시각·pid·순번을 섞는다)
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-open-tab-{tag}-{}", short_id()));
        std::fs::create_dir_all(&dir).expect("임시 요청 폴더");
        dir
    }

    fn svec(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    /// 가짜 시계 — 부를 때마다 step 씩 흐른 논리 시각(벽시계와 무관). 부르는 횟수를 정해 둔다:
    /// run_open_tab 은 요청 at 에 한 번, 기다림 시작에 한 번, 답 검사 루프마다 한 번 불린다
    fn ticking_clock(clock: &Cell<f64>, step: f64) -> impl Fn() -> f64 + '_ {
        move || {
            let current = clock.get();
            clock.set(current + step);
            current
        }
    }

    /// 보드가 놓는 답 파일 — 깨움은 (요청 id, 답 몸통) 을 알아야 쓸 수 있다. 몸통은 답 한 줄 JSON 그대로
    fn write_reply_body(dir: &Path, id: &str, body: &str) {
        std::fs::write(dir.join(format!("{id}.reply.json")), body).expect("답 파일");
    }

    fn read_request(dir: &Path, id: &str) -> OpenTabRequest {
        let body = std::fs::read_to_string(dir.join(format!("{id}.req.json"))).expect("요청 파일");
        serde_json::from_str(&body).expect("요청 꼴")
    }

    /// 폴더에 남은 파일 수 — 거둔 뒤에는 아무것도 남지 않아야 한다
    fn files_left_in(dir: &Path) -> usize {
        std::fs::read_dir(dir).expect("요청 폴더").count()
    }

    #[test]
    fn board_answers_at_first_wake_for_all_statuses() {
        // opened·exists·reopened·refused — 보드가 첫 깨움 자리에서 답 파일을 놓으면 CLI 는 곧바로 읽어 끝낸다.
        // 요청 id 는 임의라 답 파일을 '미리' 쓰는 방법은 깨움 클로저다(첫 깨움은 첫 답 검사보다 앞선다)
        let rows: Vec<(&str, &str, i32)> = vec![
            ("opened", r#"{"ok":true,"status":"opened","tab":"역할/2"}"#, 0),
            ("exists", r#"{"ok":true,"pid":42,"status":"exists"}"#, 0),
            ("reopened", r#"{"ok":true,"session":"8-4-4-4-12","status":"reopened"}"#, 0),
            ("refused", r#"{"ok":false,"status":"refused","why":"폴더가 없습니다"}"#, 1),
        ];
        for (status, answer, exit) in rows {
            let dir = temp_dir(status);
            let clock = Cell::new(1000.0_f64);
            let now = ticking_clock(&clock, 0.1);
            let wakes = Cell::new(0_i32);
            let seen_id = Cell::new(String::new());
            let wake = |id: &str| {
                wakes.set(wakes.get() + 1);
                seen_id.set(id.to_string());
                write_reply_body(&dir, id, answer);
            };
            let args = svec(&["--cwd", "C:\\w", "--title", "역할", "--", "claude"]);
            let (line, exit_code) = run_open_tab(&args, &dir, &|| true, &|| true, &wake, &now);
            assert_eq!(exit_code, exit, "{status}: 끝 코드");
            assert_eq!(line, answer, "{status}: 답 한 줄은 그대로");
            assert_eq!(wakes.get(), 1, "{status}: 깨움은 처음 한 번");
            let id = seen_id.take();
            assert!(is_session_id(&id), "{status}: 요청 id 는 세션 id 꼴이다 — 보드가 파일 이름을 걸러 읽는다");
            assert!(
                !dir.join(format!("{id}.reply.json")).exists(),
                "{status}: 답 파일은 읽은 뒤 지운다"
            );
            assert!(!dir.join(format!("{id}.tmp")).exists(), "{status}: 임시 파일은 바꾼 뒤 없다");
            let request = read_request(&dir, &id);
            assert_eq!(request.id, id, "{status}: 요청 id");
            assert_eq!(request.cwd, "C:\\w", "{status}: 작업 폴더");
            assert_eq!(request.title.as_deref(), Some("역할"), "{status}: 탭 이름");
            assert_eq!(request.argv, svec(&["claude"]), "{status}: claude 인자");
            assert_eq!(request.at, 1000.0, "{status}: 만든 시각은 첫 시계 값");
            assert_eq!(request.no_focus, None, "{status}: --no-focus 없으면 칸도 없다");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn timeout_takes_back_the_request() {
        // --timeout 1 · 시계는 0.5초씩 — 벽시계로는 답 검사 두 번(0.2초) 만에 시간이 다 한다.
        // args.rs 가 --timeout 은 1~600 초만 받으므로 논리 시간만 흘려 보낸다
        let dir = temp_dir("timeout-withdrawn");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 0.5);
        let wakes = Cell::new(0_i32);
        let seen_id = Cell::new(String::new());
        let wake = |id: &str| {
            wakes.set(wakes.get() + 1);
            seen_id.set(id.to_string());
        };
        let args = svec(&["--cwd", "C:\\w", "--timeout", "1", "--", "claude"]);
        let (line, exit) = run_open_tab(&args, &dir, &|| true, &|| true, &wake, &now);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"timeout","why":"보드가 1초 안에 요청을 가져가지 않았습니다(요청을 거뒀습니다)"}"#,
            "거둔 답 한 줄"
        );
        assert_eq!(wakes.get(), 1, "다시 깨우지 않는다(1초 지점이 곧 한계다)");
        let id = seen_id.take();
        assert!(!dir.join(format!("{id}.req.json")).exists(), "요청을 거둔다");
        assert!(!dir.join(format!("{id}.reply.json")).exists(), "답 파일도 없다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_after_board_took_request() {
        // 보드가 요청을 읽어 지웠지만 답하지 않았다 — 탭이 열렸을 수 있다고 말한다(원본 taken 별지)
        let dir = temp_dir("timeout-taken");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 0.5);
        let dir_for_wake = dir.clone();
        let wake = move |id: &str| {
            let _ = std::fs::remove_file(dir_for_wake.join(format!("{id}.req.json")));
        };
        let args = svec(&["--cwd", "C:\\w", "--timeout", "1", "--", "claude"]);
        let (line, exit) = run_open_tab(&args, &dir, &|| true, &|| true, &wake, &now);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"timeout","why":"보드가 요청을 가져갔지만 1초 안에 답하지 않았습니다(탭이 열렸을 수 있습니다)"}"#,
            "가져간 뒤 늦은 답 한 줄"
        );
        assert_eq!(files_left_in(&dir), 0, "남은 것도 없다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wakes_again_every_second_until_answer() {
        // 두 번째 깨움(1초 지점)에 보드가 답을 놓는다 — CLI 는 그 답을 받아 끝낸다.
        // 시계 1초씩 · --timeout 3: 벽시계로는 답 검사 두 번(0.2초) 안에 끝난다
        let dir = temp_dir("rewake");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 1.0);
        let wakes = Cell::new(0_i32);
        let wake = {
            let dir = dir.clone();
            let wakes = &wakes;
            move |id: &str| {
                wakes.set(wakes.get() + 1);
                if wakes.get() == 2 {
                    write_reply_body(&dir, id, r#"{"ok":true,"status":"opened"}"#);
                }
            }
        };
        let args = svec(&["--cwd", "C:\\w", "--timeout", "3", "--", "claude"]);
        let (line, exit) = run_open_tab(&args, &dir, &|| true, &|| true, &wake, &now);
        assert_eq!(exit, 0, "끝 코드");
        assert_eq!(line, r#"{"ok":true,"status":"opened"}"#, "두 번째 깨움의 답");
        assert_eq!(wakes.get(), 2, "다시 깨우는 곈은 1초다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn board_off_without_launch() {
        // 보드가 꺼져 있고 --launch 가 없다 — 보드를 이지 않고, 이미 쓴 요청을 거둔 뒤 board-off 면서 끝난다
        let dir = temp_dir("board-off");
        let launches = Cell::new(0_i32);
        let launch = || {
            launches.set(launches.get() + 1);
            true
        };
        let (line, exit) = run_open_tab(&svec(&["--cwd", "C:\\w", "--", "claude"]), &dir, &|| false, &launch, &|_| {}, &|| 1000.0);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"board-off","why":"보드가 꺼져 있습니다. --launch 를 주면 켜고 엽니다"}"#,
            "보드 꺼짐 답 한 줄"
        );
        assert_eq!(launches.get(), 0, "켜지 않는다");
        assert_eq!(files_left_in(&dir), 0, "요청을 거둔다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_failure_is_board_off() {
        // --launch 로 켜 보다가 실패 — 요청을 거둔 뒤 board-off 면서 끝난다
        let dir = temp_dir("launch-fail");
        let (line, exit) = run_open_tab(
            &svec(&["--cwd", "C:\\w", "--launch", "--", "claude"]),
            &dir,
            &|| false,
            &|| false,
            &|_| {},
            &|| 1000.0,
        );
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"board-off","why":"보드를 켜지 못했습니다"}"#,
            "켜기 실패 답 한 줄"
        );
        assert_eq!(files_left_in(&dir), 0, "요청을 거둔다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launches_board_then_opens() {
        // --launch 로 보드를 켜고 답을 받는다 — 켜기는 한 번만 이른다
        let dir = temp_dir("launch-open");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 0.1);
        let launches = Cell::new(0_i32);
        let launch = || {
            launches.set(launches.get() + 1);
            true
        };
        let wake = |id: &str| write_reply_body(&dir, id, r#"{"ok":true,"status":"opened"}"#);
        let (line, exit) = run_open_tab(&svec(&["--cwd", "C:\\w", "--launch", "--", "claude"]), &dir, &|| false, &launch, &wake, &now);
        assert_eq!(exit, 0, "끝 코드");
        assert_eq!(line, r#"{"ok":true,"status":"opened"}"#, "답 한 줄");
        assert_eq!(launches.get(), 1, "켜기는 한 번");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_args_keeps_nothing() {
        // 띄울 명령 앞에 -- 가(모르는 옵션이) 없다 — 요청 폴더를 만들지도 않고 끝난다
        let dir = std::env::temp_dir().join(format!("cb-open-tab-badargs-{}", short_id()));
        let (line, exit) = run_open_tab(&svec(&["--cwd", "C:\\w", "claude"]), &dir, &|| true, &|| true, &|_| {}, &|| 1000.0);
        assert_eq!(exit, 1, "끝 코드");
        assert!(
            line.starts_with(r#"{"ok":false,"status":"bad-args","why":"모르는 옵션입니다: claude"#),
            "나쁜 인자 답 한 줄: {line}"
        );
        assert!(line.contains("쓰는 법:"), "쓰는 법 안내가 붙는다: {line}");
        assert!(!dir.exists(), "요청 폴더도 만들지 않는다");
    }

    #[test]
    fn refused_when_request_folder_cannot_be_made() {
        // 폴더 자리가 파일이면 요청 폴더를 못 만든다 — 요청 없이 refused 면서 끝난다
        let base = temp_dir("refused-parent");
        let occupying_file = base.join("file");
        std::fs::write(&occupying_file, "폴더가 아니라 파일이다").expect("가림 파일");
        let dir = occupying_file.join("open-tab");
        let (line, exit) = run_open_tab(&svec(&["--cwd", "C:\\w", "--", "claude"]), &dir, &|| true, &|| true, &|_| {}, &|| 1000.0);
        assert_eq!(exit, 1, "끝 코드");
        assert!(
            line.starts_with(r#"{"ok":false,"status":"refused","why":"요청 폴더를 만들지 못했습니다: "#),
            "거절 답 한 줄: {line}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn no_focus_marks_the_request() {
        // --no-focus 면 요청에 noFocus: true 가 들어가고, -- 뒤 인자는 claude 로 그대로 간다
        let dir = temp_dir("no-focus");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 0.1);
        let seen_id = Cell::new(String::new());
        let wake = |id: &str| {
            seen_id.set(id.to_string());
            write_reply_body(&dir, id, r#"{"ok":true,"status":"opened","tab":"뒤에서"}"#);
        };
        let args = svec(&["--cwd", "C:\\w", "--no-focus", "--", "claude", "--resume", "x"]);
        let (line, exit) = run_open_tab(&args, &dir, &|| true, &|| true, &wake, &now);
        assert_eq!(exit, 0, "끝 코드");
        assert_eq!(line, r#"{"ok":true,"status":"opened","tab":"뒤에서"}"#, "답 한 줄");
        let request = read_request(&dir, &seen_id.take());
        assert_eq!(request.no_focus, Some(true), "--no-focus 칸");
        assert!(!request.reveals(), "뒤에서 여는 것은 앞으로 가져오지 않는다");
        assert_eq!(request.argv, svec(&["claude", "--resume", "x"]), "claude 인자");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unparseable_reply_keeps_waiting() {
        // 깨진 답 파일은 아직 답이 아니다(보드가 쓰는 중일 수 있다) — 계속 기다려 시간이 다 하면 거둔다.
        // 깨진 답 파일은 건드리지 않은 채 남는다(원본 try? → nil 도 지우지 않는다)
        let dir = temp_dir("broken-reply");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 0.5);
        let seen_id = Cell::new(String::new());
        let wake = |id: &str| {
            seen_id.set(id.to_string());
            write_reply_body(&dir, id, "아직 답이 아니다");
        };
        let args = svec(&["--cwd", "C:\\w", "--timeout", "1", "--", "claude"]);
        let (line, exit) = run_open_tab(&args, &dir, &|| true, &|| true, &wake, &now);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"timeout","why":"보드가 1초 안에 요청을 가져가지 않았습니다(요청을 거뒀습니다)"}"#,
            "거둔 답 한 줄"
        );
        let id = seen_id.take();
        assert!(!dir.join(format!("{id}.req.json")).exists(), "요청을 거둔다");
        let broken_body = std::fs::read_to_string(dir.join(format!("{id}.reply.json"))).expect("깨진 답 파일");
        assert_eq!(broken_body, "아직 답이 아니다", "깨진 답 파일은 그대로 둔다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn board_mutex_name_matches_the_single_instance_plugin() {
        // 단일 실행 플러그인은 Windows 에서 `{식별자}-sim` 이름으로 뮤텍스를 만든다 — 이름이 어긋나면
        // 떠 있는 보드를 못 알아보고 --launch 일 때도 이중으로 띄우려 든다
        let config: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tauri.conf.json"
        )))
        .expect("tauri.conf.json");
        let identifier = config["identifier"].as_str().expect("식별자");
        assert_eq!(BOARD_INSTANCE_MUTEX, format!("{identifier}-sim"));
    }

    #[cfg(windows)]
    #[test]
    fn named_mutex_probe_sees_a_made_mutex_only() {
        // 없는 이름은 거짓 — 보드가 꺼져 있을 때의 상태
        let gone = format!("com.claude.board-test-{}-gone", std::process::id());
        assert!(!open_named_mutex_exists(&gone), "없는 뮤텍스는 없다고 본다");
        // 만들어 둔 이름은 참 — 보드가 켜져 있을 때 단일 실행 플러그인이 만든 뮤텍스와 같은 상태.
        // 다른 시험·실행과 부딪히지 않게 pid 로 이름을 담보한다
        let made = format!("com.claude.board-test-{}-made", std::process::id());
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let wide: Vec<u16> = made.encode_utf16().chain([0]).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide.as_ptr()) };
        assert!(!handle.is_null(), "시험용 뮤텍스를 만든다");
        assert!(open_named_mutex_exists(&made), "만든 뮤텍스는 있다고 본다");
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        // 핸들을 닫으면(마지막 핸들이므로) 뮤텍스 사라짐 — 정리는 여기서 끝난다
    }
}

