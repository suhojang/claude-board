//! 터미널 제어 (SPEC §5.7) — Ghostty AppleScript 대신 콘솔 입력·창 찾기·claude.exe 직접 실행.
//! 터미널 독 목록(§5.11)과 설치된 Claude Code 버전(§5.10)도 여기 둔다. 새 콘솔 실행은 Windows 실기 시험으로 확인했다(그 밖은 **Windows 실기 미확인**).
//!
//! 이어 열기·계정 추가는 셸(cmd.exe·PowerShell·wt.exe 명령줄)을 거치지 않는다 — 모델 이름 같은 값은
//! 저장소의 설정 파일에서 올 수 있어 셸 해석을 거치면 명령 주입이 된다.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use claude_board_core::console;
use claude_board_core::proc::{self, ProcessTable};
use claude_board_core::{clock, paths, quote};
use serde_json::json;

use crate::board::Board;
use crate::drive::{drive, steps_for, still_same_session, Purpose};
use crate::model::{Session, TerminalApp};
use crate::open_session;
use crate::session_action::SessionAction;
use crate::util::board_log;

/// claude.exe 위치 — PATH, 없으면 `~/.local/bin/claude.exe`. `claude.cmd`(npm 설치의 shim)는 cmd.exe 가 인자를
/// 한 줄로 다시 해석하므로 일부러 고르지 않는다.
pub fn find_claude() -> Option<PathBuf> {
    let name = if cfg!(windows) { "claude.exe" } else { "claude" };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.push(paths::local_bin());
    if !cfg!(windows) {
        // macOS 판 findCommand 와 같은 흔한 설치 위치(보드는 로그인 셸의 PATH 를 모르고 뜬다)
        let home = paths::home();
        dirs.extend([home.join(".claude/bin"), home.join(".cargo/bin"), home.join("bin"), "/opt/homebrew/bin".into(), "/usr/local/bin".into()]);
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// 실행 파일을 인자 배열 그대로 새 콘솔 창에서 띄운다(CREATE_NEW_CONSOLE — Windows 11 에서 기본 터미널이
/// Windows Terminal 이면 그 안에 열린다). macOS 개발 빌드에서는 띄우지 않고 기록만 남긴다.
///
/// Windows 는 std `Command` 대신 `CreateProcessW` 를 직접 부른다. std 는 보드의 표준 핸들 가운데 하나라도 있으면
/// `STARTF_USESTDHANDLES` 로 그것을 넘기는데, 새 콘솔은 그렇게 받은 핸들을 자기 콘솔 핸들로 바꾸지 않는다 — 알림 패널이
/// 띄운 보드(표준 핸들이 NUL)나 콘솔에서 띄운 보드에서 claude.exe 의 입출력이 새 창이 아닌 곳으로 간다.
/// 여기서는 표준 핸들을 넘기지 않고(새 콘솔이 채운다) 핸들도 물려주지 않는다.
pub fn spawn_console(exe: &Path, args: &[String], cwd: &Path, envs: &[(&str, &OsStr)]) -> std::io::Result<()> {
    let cwd = if cwd.is_dir() { cwd.to_path_buf() } else { paths::home() };
    #[cfg(windows)]
    {
        create_console_process(exe, args, &cwd, envs)
    }
    #[cfg(not(windows))]
    {
        let _ = envs;
        board_log(&format!("(개발 빌드) 새 콘솔 실행 생략: {} {:?} @ {}", exe.display(), args, cwd.display()));
        Ok(())
    }
}

/// 새 콘솔로 띄울 명령줄 — 실행 파일 경로와 인자를 Windows 인용 규칙(`CommandLineToArgvW`·CRT 가 원래 인자로 되돌리는 꼴)으로 잇는다.
#[cfg_attr(not(windows), allow(dead_code))]
fn console_command_line(exe: &Path, args: &[String]) -> String {
    let exe = exe.to_string_lossy();
    std::iter::once(quote::windows(&exe))
        .chain(args.iter().map(|a| quote::windows(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 이 프로세스의 환경에 `extra` 를 덮어쓴 목록 — 이름은 대소문자를 가리지 않고 같게 보고,
/// 환경 블록 규칙대로 이름(대문자 기준) 순으로 늘어놓는다.
#[cfg_attr(not(windows), allow(dead_code))]
fn merged_env(base: impl IntoIterator<Item = (OsString, OsString)>, extra: &[(&str, &OsStr)]) -> Vec<(OsString, OsString)> {
    let upper = |k: &OsStr| k.to_string_lossy().to_uppercase();
    let mut out: Vec<(OsString, OsString)> =
        base.into_iter().filter(|(k, _)| !extra.iter().any(|(e, _)| upper(k) == e.to_uppercase())).collect();
    out.extend(extra.iter().map(|(k, v)| (OsString::from(k), v.to_os_string())));
    out.sort_by_cached_key(|(k, _)| upper(k));
    out
}

#[cfg(windows)]
fn create_console_process(exe: &Path, args: &[String], cwd: &Path, envs: &[(&str, &OsStr)]) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let wide = |s: &OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let app = wide(exe.as_os_str());
    // CreateProcessW 는 명령줄 버퍼를 고쳐 쓸 수 있으므로 우리 버퍼를 넘긴다
    let mut line = wide(OsStr::new(&console_command_line(exe, args)));
    let dir = wide(cwd.as_os_str());
    // 더할 환경변수가 있을 때만 블록을 만든다(없으면 null = 보드의 환경 그대로). "이름=값\0…\0\0"
    let block: Option<Vec<u16>> = (!envs.is_empty()).then(|| {
        let mut b = Vec::new();
        for (k, v) in merged_env(std::env::vars_os(), envs) {
            b.extend(k.encode_wide());
            b.push(u16::from(b'='));
            b.extend(v.encode_wide());
            b.push(0);
        }
        b.push(0);
        b
    });
    // dwFlags 0 — 표준 핸들은 새 콘솔이 채운다
    let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: 모든 문자열은 NUL 로 끝나고 이 함수 동안 살아 있다. 핸들 상속 없음. 돌려받은 두 핸들은 바로 닫는다
    let ok = unsafe {
        CreateProcessW(
            app.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            FALSE,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            block.as_ref().map_or(null(), |b| b.as_ptr().cast()),
            dir.as_ptr(),
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: CreateProcessW 가 준 핸들을 한 번씩 닫는다(프로세스는 그대로 돈다)
    unsafe {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    Ok(())
}

/// 세션 동작 — 프론트의 session_action 과 `--action` 이 부른다(이름은 `SessionAction::parse`). 오래 걸리는 일이 있어 스레드에서 돈다.
pub fn perform(board: &Arc<Board>, action: &str, carry_on: Option<bool>, s: Session, confirm: bool) {
    let board = board.clone();
    let Some(parsed) = SessionAction::parse(action, carry_on) else {
        board_log(&format!("모르는 세션 동작: {action}"));
        return;
    };
    std::thread::spawn(move || match parsed {
        SessionAction::Activate => activate_owner(&s),
        // 명령줄 focus(confirm=false)는 보드 허브가 그 세션의 탭을 고르게 하고, 보드 안 클릭은 바로 그 터미널 창을 앞으로 가져온다
        SessionAction::Focus if confirm => focus_session(&s),
        SessionAction::Focus => open_session::request(&board, &s, "focus", false),
        SessionAction::Reopen { carry_on } => reopen_session(&board, &s, carry_on),
        SessionAction::Pause => pause_session(&board, &s),
        SessionAction::Resume => resume_paused(&board, &s),
        SessionAction::Stop => stop_session(&board, &s, confirm),
    });
}

pub(crate) fn paused_mark(board: &Board, s: &Session) -> PathBuf {
    board.sessions_dir().join(format!("{}.paused", s.id))
}

/// 일시중지: Esc 로 지금 응답을 멈춘다. 그때까지 한 작업은 대화 기록에 남아 있으므로 재개하면 바로 이어 간다.
/// 키가 들어간 뒤에만 일시중지 표시를 남긴다.
fn pause_session(board: &Arc<Board>, s: &Session) {
    if !drive(board, s, &steps_for(SessionAction::Pause), Purpose::Keys) {
        return;
    }
    let mark = json!({"paused_at": clock::now(), "topic": s.topic});
    let _ = fs::write(paused_mark(board, s), serde_json::to_vec(&mark).unwrap_or_default());
    board.publish();
}

/// 재개 판정 — `Drive` 는 살아 있는 세션(끝나지 않았고, pid 가 없거나 pid 가 살아 있음)에 키를 넣는다.
/// `Reopen` 은 그 외(끝났거나 pid 가 죽었거나) 보드 터미널에서 이어 연다.
#[derive(Debug, PartialEq)]
enum ResumePlan {
    Reopen,
    Drive,
}

/// 재개 판정을 순수 함수로 뗀다(규칙 4) — 살아 있음 = 끝나지 않았고, pid 가 없거나(pid == 0) pid 가 살아 있음.
fn resume_plan(state: &str, pid: u32, alive: bool) -> ResumePlan {
    if state != "ended" && (pid == 0 || alive) {
        ResumePlan::Drive
    } else {
        ResumePlan::Reopen
    }
}

/// 재개: 살아 있는 세션(끝나지 않았고, pid 가 없거나 pid 가 살아 있음)에는 이어서 진행하라는 요청을 넣는다.
/// 넣지 못하면 표지를 그대로 두고 알린다(같은 세션을 한 번 더 열면 claude 가 중복으로 뜬다). 꺼진 세션은 보드 터미널에서 이어 연다.
fn resume_paused(board: &Arc<Board>, s: &Session) {
    // pid 가 있을 때만 살아 있는지 본다 — pid 가 없으면 살아 있는 것으로 본다
    let alive = s.pid > 0 && proc::alive(s.pid, s.pid_ctime);
    match resume_plan(&s.state, s.pid, alive) {
        ResumePlan::Reopen => return reopen_session(board, s, true),
        ResumePlan::Drive => {}
    }
    if drive(board, s, &steps_for(SessionAction::Resume), Purpose::Keys) {
        let _ = fs::remove_file(paused_mark(board, s));
        board.publish();
    } else {
        // 일시중지 표지는 그대로 둔다 — 사용자가 그 터미널에서 이어 간다
        board.flash(
            format!("{}: 세션이 아직 돌고 있지만 그 터미널에 키를 넣지 못했습니다. 그 터미널에서 이어 가 주세요", s.project()),
            true,
        );
    }
}

/// 중단: 확인을 받은 뒤 세션을 끝낸다. 닿을 곳에 따라 Esc → /exit 를 넣거나 프로세스를 끝낸다(보드 밖에서 도는 background 는 건드리지 않는다).
/// 대화 기록은 남으므로 "세션 다시 열기"로 이어 열 수 있다.
fn stop_session(board: &Arc<Board>, s: &Session, confirm: bool) {
    if confirm {
        let what = if s.topic.is_empty() { s.id.clone() } else { s.topic.clone() };
        let text = format!("{what}

진행 중인 응답을 멈추고 세션을 끝냅니다. 대화 기록은 남아서 나중에 다시 열 수 있습니다.");
        if !board.confirm("세션을 중단할까요?", &text, "중단", "취소") {
            return;
        }
    }
    // 확인 창이 떠 있는 동안 세션이 끝났거나 pid 가 바뀌었을 수 있다 — 새로 읽어 같은 세션일 때만 끝낸다
    let Some(fresh) = still_same_session(s, board.session(&s.id)) else { return };
    let _ = fs::remove_file(paused_mark(board, &fresh));
    drive(board, &fresh, &steps_for(SessionAction::Stop), Purpose::Stop);
    board.publish();
}

/// 이어 열기: 보드 터미널 허브가 그 세션을 탭에서 `claude <resume_args…>` 로 이어 열게 요청한다.
/// 일시중지 표시는 여기서 지우지 않는다 — 허브가 열기를 마친 뒤 `reopen_done` 으로 알리면 그때 지운다.
fn reopen_session(board: &Arc<Board>, s: &Session, carry_on: bool) {
    open_session::request(board, s, "reopen", carry_on);
    board.publish();
}

/// 그 세션의 터미널 창(콘솔 창 → 소유 창 → 조상의 보이는 최상위 창).
fn session_window(s: &Session) -> Option<isize> {
    if s.headless || s.state == "ended" || s.pid == 0 {
        return None;
    }
    console::terminal_window(s.pid)
}

/// 한 번 클릭: 그 세션의 터미널 창만 앞으로. 모르면 Windows Terminal(없으면 아무것도 안 함).
fn activate_owner(s: &Session) {
    match session_window(s) {
        Some(h) => crate::win::bring_to_front(h),
        None => activate_windows_terminal(),
    }
}

fn activate_windows_terminal() {
    let pids = ProcessTable::capture().pids_named("WindowsTerminal.exe");
    if let Some(h) = crate::win::process_window(&pids) {
        crate::win::bring_to_front(h);
    }
}

/// 두 번 클릭: 그 세션의 창으로. Windows Terminal 이면 콘솔 제목을 잠깐 표지로 바꿔 UI Automation 으로 그 탭을 고른다.
fn focus_session(s: &Session) {
    let Some(h) = session_window(s) else { return activate_windows_terminal() };
    let host = ProcessTable::capture().name(crate::win::window_pid(h)).map(str::to_ascii_lowercase);
    if host.as_deref() == Some("windowsterminal.exe") {
        let old = console::title(s.pid);
        let marker = format!("CBFOCUS-{}", crate::util::short_id());
        if console::set_title(s.pid, &marker) {
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(50));
                if crate::win::select_wt_tab(h, &marker) {
                    break;
                }
            }
            console::set_title(s.pid, &old.unwrap_or_else(|| s.project()));
        }
    }
    crate::win::bring_to_front(h);
}

// ── 터미널 독 (§5.11) ────────────────────────────────────────

/// (id, 이름, 실행 중으로 볼 프로세스 이름들)
const DOCK: [(&str, &str, &[&str]); 5] = [
    ("wt", "Windows Terminal", &["WindowsTerminal.exe"]),
    ("pwsh", "PowerShell 7", &["pwsh.exe"]),
    ("powershell", "Windows PowerShell", &["powershell.exe"]),
    ("cmd", "명령 프롬프트", &["cmd.exe"]),
    ("git-bash", "Git Bash", &["git-bash.exe", "mintty.exe"]),
];

#[derive(Default)]
pub struct Terminals {
    installed: HashMap<String, PathBuf>,
    checked_at: f64,
    running: HashSet<String>,
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// 설치 위치 — PATH 와 알려진 위치. Windows 가 아니면 늘 없음.
pub(crate) fn locate(id: &str) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let exe = match id {
        "wt" => "wt.exe",
        "pwsh" => "pwsh.exe",
        "powershell" => "powershell.exe",
        "cmd" => "cmd.exe",
        "git-bash" => "git-bash.exe",
        _ => return None,
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(d) = env_dir("LOCALAPPDATA") {
        dirs.push(d.join("Microsoft").join("WindowsApps"));
    }
    if let Some(d) = env_dir("ProgramFiles") {
        dirs.push(d.join("PowerShell").join("7"));
        dirs.push(d.join("Git"));
    }
    if let Some(d) = env_dir("SystemRoot") {
        dirs.push(d.join("System32"));
        dirs.push(d.join("System32").join("WindowsPowerShell").join("v1.0"));
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

impl Terminals {
    /// 설치 여부는 1분마다, 실행 여부는 백그라운드 확인(3초)마다.
    pub fn refresh(&mut self, table: &ProcessTable, now: f64) {
        if now - self.checked_at >= 60.0 {
            self.checked_at = now;
            self.installed = DOCK.iter().filter_map(|(id, _, _)| locate(id).map(|p| (id.to_string(), p))).collect();
        }
        self.running = DOCK
            .iter()
            .filter(|(_, _, names)| names.iter().any(|n| !table.pids_named(n).is_empty()))
            .map(|(id, _, _)| id.to_string())
            .collect();
    }

    pub fn view(&self) -> Vec<TerminalApp> {
        DOCK.iter()
            .map(|(id, name, _)| TerminalApp {
                id: id.to_string(),
                name: name.to_string(),
                installed: self.installed.contains_key(*id),
                running: self.running.contains(*id),
            })
            .collect()
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        self.installed.get(id).cloned().or_else(|| locate(id))
    }
}

/// 독 단추: 실행 중이면 그 창으로, 아니면 새로 연다(셸 명령줄 없이 실행 파일만).
pub fn open_terminal(board: &Arc<Board>, id: String) {
    let exe = board.terminals.lock().unwrap().path(&id);
    let Some(exe) = exe else { return };
    std::thread::spawn(move || {
        let names = DOCK.iter().find(|(i, _, _)| *i == id).map(|(_, _, n)| *n).unwrap_or(&[]);
        let table = ProcessTable::capture();
        let pids: Vec<u32> = names.iter().flat_map(|n| table.pids_named(n)).collect();
        // 콘솔 셸은 자기 창이 없을 수 있어(ConPTY) 콘솔 창 → 소유 창 순으로 찾는다
        let window = crate::win::process_window(&pids).or_else(|| pids.iter().find_map(|&p| console::terminal_window(p)));
        if let Some(h) = window {
            crate::win::bring_to_front(h);
            return;
        }
        let r = match id.as_str() {
            "wt" | "git-bash" => {
                let mut cmd = Command::new(&exe);
                cmd.current_dir(paths::home());
                crate::util::spawn_detached(cmd).map(|_| ())
            }
            _ => spawn_console(&exe, &[], &paths::home(), &[]),
        };
        if let Err(e) = r {
            board_log(&format!("{} 실행 실패: {e}", exe.display()));
        }
    });
}

// ── 설치된 Claude Code 버전 (§5.10) ─────────────────────────

/// 숫자 비교 — 점으로 나눈 조각에서 앞의 0 을 떼고 길이로, 같은 길이면 글자 차례로 비교한다(u64 를 넘는 자리도 옳다).
pub fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<&str> = a.split('.').collect();
    let pb: Vec<&str> = b.split('.').collect();
    for i in 0..pa.len().max(pb.len()) {
        // 없는 조각은 0 과 같다
        let o = compare_digits(pa.get(i).copied().unwrap_or("0"), pb.get(i).copied().unwrap_or("0"));
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    std::cmp::Ordering::Equal
}

/// 두 숫자 조각을 견준다 — 앞의 0 을 떼고 길이가 큰 쪽이 크고, 같은 길이면 글자 차례(u64 를 넘는 자리도 옳게 견준다).
fn compare_digits(x: &str, y: &str) -> std::cmp::Ordering {
    let x = x.trim_start_matches('0');
    let y = y.trim_start_matches('0');
    x.len().cmp(&y.len()).then_with(|| x.cmp(y))
}

/// `~/.local/share/claude/versions` 안 가장 높은 버전 이름(`.exe` 는 뗀다).
pub fn read_claude_version(home: &Path) -> Option<String> {
    let dir = home.join(".local").join("share").join("claude").join("versions");
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|n| n.strip_suffix(".exe").map(str::to_string).unwrap_or(n))
        .filter(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .max_by(|a, b| cmp_version(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn versions() {
        assert_eq!(cmp_version("2.1.301", "2.1.30"), Ordering::Greater);
        assert_eq!(cmp_version("2.12", "2.12.0"), Ordering::Equal);
        assert_eq!(cmp_version("2.9", "2.12"), Ordering::Less);
        let home = std::env::temp_dir().join(format!("cb-ver-{}", crate::util::short_id()));
        let dir = home.join(".local/share/claude/versions");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_claude_version(&home), None);
        for n in ["2.1.30", "2.1.301.exe", "2.1.99", "latest", ".tmp"] {
            fs::write(dir.join(n), b"").unwrap();
        }
        assert_eq!(read_claude_version(&home).as_deref(), Some("2.1.301"));
        fs::remove_dir_all(&home).unwrap();
    }

    /// 재개 판정 표(규칙 4) — 살아 있는 세션(끝나지 않았고, pid 가 없거나 pid 가 살아 있음)은 다시 열지 않는다.
    /// pid 가 없으면 살아 있는 것으로 본다(중복 claude 를 열지 않는다), 끝난 세션은 pid 가 살아 남아 있어도 다시 연다.
    #[test]
    fn resume_plan_table() {
        let rows: [(&str, u32, bool, ResumePlan); 5] = [
            // 끝난 세션은 pid 가 살아 있는 것처럼 기록이 남아 있어도(잔송) 다시 연다
            ("ended", 123, true, ResumePlan::Reopen),
            ("ended", 0, false, ResumePlan::Reopen),
            // 돌고 있는 세션은 그 터미널에 키를 넣는다
            ("working", 123, true, ResumePlan::Drive),
            // 기록이 끝나지 않았지만 pid 가 죽었으면 다시 연다
            ("working", 123, false, ResumePlan::Reopen),
            // pid 가 없으면 살아 있는 것으로 본다
            ("working", 0, false, ResumePlan::Drive),
        ];
        for (state, pid, alive, want) in rows {
            assert_eq!(resume_plan(state, pid, alive), want, "state={state} pid={pid} alive={alive}");
        }
    }

    /// u64 를 넘는 자리도 길이로 견준다(A2), 앞의 0 은 떼며, 조각 수가 달라도 0 을 채운 것과 같다.
    #[test]
    fn cmp_version_digits_beyond_u64() {
        assert_eq!(cmp_version("18446744073709551616", "9"), Ordering::Greater);
        assert_eq!(cmp_version("007", "7"), Ordering::Equal);
        assert_eq!(cmp_version("3.2", "3.2.0"), Ordering::Equal);
        assert_eq!(cmp_version("3.10.0", "3.9.9"), Ordering::Greater);
    }

    #[test]
    fn console_launch_line_and_env() {
        let exe = Path::new(r"C:\Users\홍 길동\.local\bin\claude.exe");
        // 공백 없는 끝 \ 는 그대로, 따옴표로 감싸는 인자의 끝 \ 는 두 배로(닫는 따옴표가 글자로 읽히지 않게)
        let args: Vec<String> =
            ["--resume", "abc", "이어서 \"진행\" 해 주세요", "", r"C:\dir\", r"C:\my dir\"].map(String::from).to_vec();
        assert_eq!(
            console_command_line(exe, &args),
            r#""C:\Users\홍 길동\.local\bin\claude.exe" --resume abc "이어서 \"진행\" 해 주세요" "" C:\dir\ "C:\my dir\\""#
        );
        assert_eq!(console_command_line(Path::new(r"C:\bin\claude.exe"), &[]), r"C:\bin\claude.exe");

        let base = [("Path", "C:\\bin"), ("claude_config_dir", "old"), ("APPDATA", "x"), ("=C:", "C:\\")]
            .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let slot = OsString::from("D:\\slot");
        let got = merged_env(base, &[("CLAUDE_CONFIG_DIR", slot.as_os_str())]);
        let keys: Vec<_> = got.iter().map(|(k, v)| format!("{}={}", k.to_string_lossy(), v.to_string_lossy())).collect();
        assert_eq!(keys, ["=C:=C:\\", "APPDATA=x", "CLAUDE_CONFIG_DIR=D:\\slot", "Path=C:\\bin"]);
    }

    #[test]
    fn dock_list_shape() {
        let t = Terminals::default();
        let v = t.view();
        assert_eq!(v.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["wt", "pwsh", "powershell", "cmd", "git-bash"]);
        assert!(v.iter().all(|x| !x.installed && !x.running));
    }

    /// 실제 Windows 에서만 — `spawn_console` 로 node.exe 를 새 콘솔에 띄워 자식이 받은 인자·환경변수·작업 폴더를 비교한다.
    /// 위 `console_launch_line_and_env` 가 명령줄 문자열의 모양을 본다면, 이것은 그 문자열을 Windows 의 CRT(node.exe 는
    /// MSVC 빌드)가 같은 배열로 되돌리는지를 본다. 기본 터미널이 Windows Terminal 이면 자식은 그 탭으로 열린다 — 쓰고 바로 끝난다.
    #[cfg(windows)]
    mod real_spawn {
        use super::*;
        use serde_json::Value;
        use std::time::Instant;

        /// 자식 스크립트. 결과는 스크립트 옆 result.json 에 쓴다(인자·환경이 틀려도 스크립트만 돌면 보고가 남게).
        /// 작업 폴더는 경로 문자열 대신 표지 파일로 확인한다(임시 폴더가 8.3 짧은 이름이거나 대소문자가 다를 수 있다).
        const SCRIPT: &str = r#"
const fs = require('fs'), path = require('path');
const out = path.join(__dirname, 'result.json');
let r;
try {
  const names = ['CB_SPAWN_VALUE', 'USERPROFILE', 'SYSTEMROOT'];
  const env = Object.keys(process.env).filter(k => names.includes(k.toUpperCase())).map(k => [k, process.env[k]]);
  fs.writeFileSync(path.join(process.cwd(), 'cwd-marker'), String(process.pid));
  r = { pid: process.pid, argv0: process.argv0, args: process.argv.slice(2), cwd: process.cwd(), env };
} catch (e) {
  r = { pid: process.pid, error: String((e && e.stack) || e) };
}
fs.writeFileSync(out + '.tmp', JSON.stringify(r));
fs.renameSync(out + '.tmp', out);
"#;

        fn find_node() -> Option<PathBuf> {
            let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
            dirs.into_iter().map(|d| d.join("node.exe")).find(|p| p.is_file())
        }

        /// 끝나면 남은 자식(명령줄에 이 시험 폴더 이름이 든 node.exe)을 5초 기다렸다 끝내고 시험 폴더를 지운다.
        struct Cleanup(PathBuf);

        impl Drop for Cleanup {
            fn drop(&mut self) {
                let mark = self.0.file_name().unwrap().to_string_lossy().into_owned();
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let left: Vec<u32> = ProcessTable::capture()
                        .pids_named("node.exe")
                        .into_iter()
                        .filter(|&p| proc::command_line(p).is_some_and(|a| a.iter().any(|s| s.contains(&mark))))
                        .collect();
                    if left.is_empty() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        eprintln!("남은 자식을 끝낸다: {left:?}");
                        for p in left {
                            proc::terminate(p);
                        }
                        std::thread::sleep(Duration::from_millis(500));
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                // 실행 파일 이미지가 막 풀린 참이면 지우기가 잠깐 실패할 수 있다
                for _ in 0..10 {
                    if fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        }

        fn listing(dir: &Path) -> Vec<String> {
            fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
        }

        /// 한 번 띄우고 결과를 60초까지 기다린다. 못 받으면 명령줄과 폴더 내용을 실패 메시지에 넣는다.
        fn run(exe: &Path, args: &[String], cwd: &Path, envs: &[(&str, &OsStr)], result: &Path) -> Value {
            let _ = fs::remove_file(result);
            let _ = fs::remove_file(cwd.join("cwd-marker"));
            let line = console_command_line(exe, args);
            spawn_console(exe, args, cwd, envs).unwrap_or_else(|e| panic!("spawn_console 실패: {e}\n명령줄: {line}"));
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline {
                if let Ok(b) = fs::read(result) {
                    return serde_json::from_slice(&b)
                        .unwrap_or_else(|e| panic!("결과 파일이 JSON 이 아니다: {e}\n{}", String::from_utf8_lossy(&b)));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            let scripts = result.parent().unwrap();
            panic!(
                "60초 안에 자식의 결과 파일이 생기지 않았다 — 자식이 스크립트를 돌리지 못했다(실행 파일 경로나 인자가 쪼개졌을 수 있다)\n\
                 실행 파일: {}\n명령줄: {line}\n작업 폴더: {} {:?}\n스크립트 폴더: {:?}",
                exe.display(),
                cwd.display(),
                listing(cwd),
                listing(scripts)
            );
        }

        /// 인자·argv[0]·작업 폴더 — 두 번의 실행에 같이 쓴다.
        fn check_common(r: &Value, exe: &Path, args: &[String], cwd: &Path) {
            let line = console_command_line(exe, args);
            if let Some(e) = r.get("error") {
                panic!("자식 스크립트 오류: {e}\n명령줄: {line}");
            }
            let got: Vec<String> = r["args"]
                .as_array()
                .unwrap_or_else(|| panic!("결과에 args 가 없다: {r}"))
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect();
            // args[0] 은 스크립트 경로 — node 가 process.argv[1] 로 바꿔 넣으므로 그 뒤부터 비교한다
            let want = &args[1..];
            if got != want {
                let diff: Vec<String> = (0..got.len().max(want.len()))
                    .filter(|&i| got.get(i) != want.get(i))
                    .map(|i| format!("  [{i}] 보냄 {:?} / 받음 {:?}", want.get(i), got.get(i)))
                    .collect();
                panic!("자식이 받은 인자가 다르다 (보냄 {}개, 받음 {}개)\n{}\n명령줄: {line}", want.len(), got.len(), diff.join("\n"));
            }
            assert_eq!(r["argv0"].as_str(), Some(&*exe.to_string_lossy()), "argv[0] 이 실행 파일 경로와 다르다\n명령줄: {line}");
            assert!(
                cwd.join("cwd-marker").is_file(),
                "자식의 작업 폴더가 넘긴 폴더가 아니다 — 자식 cwd {}, 넘긴 폴더 {}",
                r["cwd"],
                cwd.display()
            );
        }

        /// 자식이 본 환경변수 가운데 이름(대소문자 무시)이 `name` 인 것들 — 같은 이름이 두 번 들어갔는지도 보인다.
        fn env_of(r: &Value, name: &str) -> Vec<(String, String)> {
            r["env"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|kv| Some((kv[0].as_str()?.to_string(), kv[1].as_str()?.to_string())))
                .filter(|(k, _)| k.eq_ignore_ascii_case(name))
                .collect()
        }

        #[test]
        fn spawn_console_delivers_args_env_and_cwd() {
            let Some(node) = find_node() else {
                if std::env::var_os("CI").is_some() {
                    panic!("CI 인데 PATH 에서 node.exe 를 찾지 못했다 — PATH: {:?}", std::env::var_os("PATH"));
                }
                eprintln!("node.exe 가 PATH 에 없어 spawn_console 실제 실행 시험을 건너뛴다");
                return;
            };
            let root = std::env::temp_dir().join(format!("cb-console-{}", crate::util::short_id()));
            let _cleanup = Cleanup(root.clone());
            // 실행 파일 경로에도 공백·한글을 넣는다 — argv[0] 이 쪼개지면 뒤 인자가 모두 밀린다. 같은 볼륨이면 하드 링크, 아니면 복사
            let (bin, scripts, cwd) = (root.join("노드 복사본"), root.join("자식 스크립트"), root.join("작업 폴더"));
            for d in [&bin, &scripts, &cwd] {
                fs::create_dir_all(d).unwrap();
            }
            let exe = bin.join("node.exe");
            if fs::hard_link(&node, &exe).is_err() {
                fs::copy(&node, &exe).unwrap_or_else(|e| panic!("{} 를 {} 로 복사하지 못했다: {e}", node.display(), exe.display()));
            }
            let script = scripts.join("child.js");
            fs::write(&script, SCRIPT).unwrap();
            let result = scripts.join("result.json");

            // 이어 열기 인자에는 저장소 설정 파일에서 온 모델 이름이 들어갈 수 있다 — 셸 문자·따옴표·역슬래시가 그대로 하나씩 도착해야 한다
            let cases = [
                "공백 있는 인자",
                r#"say "hi" now"#,
                "한글",
                "",
                r"C:\끝\",
                r"C:\my dir\",
                r"C:\두 개\\",
                r#"a\"b"#,
                r#"a\\"b c"#,
                r"\\server\share dir",
                "\\",
                "\"",
                "& | ^ % ! < >",
                "a&b|c^d%e!f<g>h(i)",
                "%PATH%",
                "!PATH!",
                "--model",
                "x; calc & echo",
                r#"x" & calc & echo ""#,
                "--resume",
                "탭\t문자",
                "  앞뒤 공백  ",
                crate::drive::CARRY_ON_PROMPT,
            ];
            let args: Vec<String> = std::iter::once(script.to_string_lossy().into_owned()).chain(cases.map(String::from)).collect();
            let system_root = std::env::var("SystemRoot").expect("SystemRoot 환경변수");

            // 이어 열기 꼴 — 더할 환경변수 없이(보드의 환경을 그대로 물려준다)
            let r = run(&exe, &args, &cwd, &[], &result);
            eprintln!("이어 열기 꼴 결과: {r}");
            check_common(&r, &exe, &args, &cwd);
            let extra = env_of(&r, "CB_SPAWN_VALUE");
            assert!(extra.is_empty(), "더하지 않은 환경변수가 보인다: {extra:?}");
            assert_eq!(env_of(&r, "SystemRoot").into_iter().map(|(_, v)| v).collect::<Vec<_>>(), [system_root.as_str()]);

            // 계정 추가 꼴 — 환경변수를 더한다. 이미 있는 이름(USERPROFILE)은 대소문자가 달라도 하나로 덮어써야 한다
            let value = "값 \"따옴표\" & | ^ < > %TEMP% !PATH! a=b ; 끝\\";
            let profile = r"D:\가짜 프로필 %USERPROFILE%";
            let envs = [("CB_SPAWN_VALUE", OsStr::new(value)), ("userprofile", OsStr::new(profile))];
            let r = run(&exe, &args, &cwd, &envs, &result);
            eprintln!("계정 추가 꼴 결과: {r}");
            check_common(&r, &exe, &args, &cwd);
            assert_eq!(env_of(&r, "CB_SPAWN_VALUE"), [("CB_SPAWN_VALUE".to_string(), value.to_string())]);
            let got = env_of(&r, "USERPROFILE");
            assert!(got.len() == 1 && got[0].1 == profile, "USERPROFILE 덮어쓰기가 틀렸다 — 자식이 본 값 {got:?}");
            // 더한 환경 블록에도 보드의 나머지 환경이 그대로 들어가야 한다
            assert_eq!(env_of(&r, "SystemRoot").into_iter().map(|(_, v)| v).collect::<Vec<_>>(), [system_root.as_str()]);
        }
    }
}
