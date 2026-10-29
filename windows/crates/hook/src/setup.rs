//! 설치 도구 — `setup`·`uninstall`·`install-hooks`·`uninstall-hooks` (SPEC §3.1, §3.7).
//! macOS `user-setup.sh`·`Uninstall.command`·`hooks.py` 에 해당한다.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use claude_board_core::{clock, hooks, paths};

/// 로그인 자동 실행 값 이름(HKCU\…\Run).
#[cfg_attr(not(windows), allow(dead_code))]
const RUN_VALUE: &str = "ClaudeBoard";
const BOARD_CMD: &str = "claude-board.cmd";
const TUI_CMD: &str = "claude-board-tui.cmd";
const OLD_PREFIX: &str = "claude-board-hook.old-";

/// macOS 개발 기기에서는 macOS 판이 쓰는 ~/.claude 를 덮어쓰지 않도록 환경변수로 폴더를 바꿨을 때만 허락한다.
fn guard() -> Result<(), ExitCode> {
    if cfg!(windows) || paths::overridden() {
        return Ok(());
    }
    eprintln!(
        "Windows 가 아닌 곳에서는 macOS 판의 ~/.claude 를 덮어쓰므로 {} 이나 {} 를 지정해야 합니다.",
        paths::HOME_ENV,
        paths::NOTIFIER_ENV
    );
    Err(ExitCode::FAILURE)
}

fn stamp() -> String {
    clock::local(clock::now()).stamp()
}

fn do_install_hooks() -> Result<(), String> {
    let settings = paths::settings_file();
    let exe = paths::hook_exe();
    if !exe.is_file() {
        println!("  주의: {} 가 아직 없습니다 — setup 을 실행해야 hook 이 동작합니다", exe.display());
    }
    let report = hooks::install(&settings, &hooks::command_path(&exe), &stamp()).map_err(|e| e.to_string())?;
    if let Some(b) = report.backup {
        println!("  백업: {}", b.display());
    }
    println!("  등록: {}", settings.display());
    Ok(())
}

fn do_uninstall_hooks() -> Result<(), String> {
    let settings = paths::settings_file();
    let report = hooks::uninstall(&settings, &stamp()).map_err(|e| e.to_string())?;
    if let Some(b) = report.backup {
        println!("  백업: {}", b.display());
    }
    println!("  제거: {}", settings.display());
    Ok(())
}

fn finish(r: Result<(), String>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("  {e}");
            ExitCode::FAILURE
        }
    }
}

/// `install-hooks` — settings.json 에 hook 11개(exec form)와 env 키를 넣는다.
pub fn install_hooks() -> ExitCode {
    if let Err(c) = guard() {
        return c;
    }
    finish(do_install_hooks())
}

/// `uninstall-hooks` — 우리 hook 과 env 키만 뺀다.
pub fn uninstall_hooks() -> ExitCode {
    if let Err(c) = guard() {
        return c;
    }
    finish(do_uninstall_hooks())
}

/// `setup --app <ClaudeBoard.exe>`
pub fn setup(args: &[String]) -> ExitCode {
    let app = match args {
        [flag, path, ..] if flag == "--app" => PathBuf::from(path),
        [arg, ..] if arg.starts_with("--app=") => PathBuf::from(&arg["--app=".len()..]),
        _ => {
            eprintln!("사용법: claude-board-hook setup --app <ClaudeBoard.exe 경로>");
            return ExitCode::from(2);
        }
    };
    if let Err(c) = guard() {
        return c;
    }
    let app = if app.is_absolute() { app } else { env::current_dir().map(|d| d.join(&app)).unwrap_or(app) };
    if !app.is_file() {
        eprintln!("  앱을 찾지 못했습니다: {}", app.display());
        return ExitCode::FAILURE;
    }
    println!("Claude Board 설정");
    finish(run_setup(&app))
}

fn run_setup(app: &Path) -> Result<(), String> {
    let notifier = paths::notifier_dir();
    fs::create_dir_all(paths::sessions_dir()).map_err(|e| format!("{} 을 만들지 못했습니다: {e}", notifier.display()))?;

    // 1) hook 사본 — 실행 중인 exe 는 덮어쓸 수 없으므로 이름을 바꿔 비키고 새로 복사한다
    let dest = paths::hook_exe();
    remove_old_copies(&notifier);
    let me = env::current_exe().map_err(|e| format!("실행 파일 경로를 알 수 없습니다: {e}"))?;
    if same_file(&me, &dest) {
        println!("  hook: {} (지금 실행 중인 파일)", dest.display());
    } else {
        if dest.exists() {
            let old = notifier.join(format!(
                "{OLD_PREFIX}{}{}",
                (clock::now() * 1000.0) as u64,
                env::consts::EXE_SUFFIX
            ));
            if fs::rename(&dest, &old).is_err() {
                fs::remove_file(&dest).map_err(|e| format!("{} 을 비우지 못했습니다: {e}", dest.display()))?;
            }
        }
        fs::copy(&me, &dest).map_err(|e| format!("{} 에 복사하지 못했습니다: {e}", dest.display()))?;
        println!("  hook: {}", dest.display());
    }

    // 2) 알림 패널이 띄울 앱 경로
    let app_path = paths::app_path_file();
    fs::write(&app_path, format!("{}\n", app.display())).map_err(|e| format!("{} 을 쓰지 못했습니다: {e}", app_path.display()))?;
    println!("  앱 경로: {}", app_path.display());

    // 3) 전역 settings.json hook (백업 후 Claude Board hook 만 다시 넣는다)
    do_install_hooks()?;

    // 4) 명령 — Claude Code 네이티브 설치가 ~/.local/bin 을 PATH 에 넣는다
    let bin = paths::local_bin();
    fs::create_dir_all(&bin).map_err(|e| format!("{} 을 만들지 못했습니다: {e}", bin.display()))?;
    let board = format!("@start \"\" \"{}\" %*\r\n", cmd_path(app));
    let tui = format!("@\"{}\" tui %*\r\n", cmd_path(&dest));
    for (name, body) in [(BOARD_CMD, board), (TUI_CMD, tui)] {
        let p = bin.join(name);
        fs::write(&p, body).map_err(|e| format!("{} 을 쓰지 못했습니다: {e}", p.display()))?;
        println!("  명령: {}", p.display());
    }

    // 5) 로그인할 때 자동 실행
    #[cfg(windows)]
    {
        let value = format!("\"{}\" --background", app.display());
        registry::set_run(RUN_VALUE, &value).map_err(|e| format!("자동 실행을 등록하지 못했습니다: {e}"))?;
        println!("  자동 실행: HKCU\\{}\\{RUN_VALUE}", registry::RUN_KEY);
    }
    println!("설정을 마쳤습니다.");
    Ok(())
}

/// 지난 setup 이 비켜 둔 `claude-board-hook.old-*` 를 지운다(아직 도는 것은 지워지지 않으니 다음에 다시).
fn remove_old_copies(notifier: &Path) {
    for entry in fs::read_dir(notifier).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(OLD_PREFIX) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// .cmd 안에 쓸 경로 — 사용자 폴더 부분을 `%LOCALAPPDATA%`·`%USERPROFILE%` 로 바꾼다.
/// cmd.exe 는 배치 파일을 OEM 코드 페이지로 읽으므로 한글 사용자 이름이 경로에 그대로 있으면 깨진다.
fn cmd_path(p: &Path) -> String {
    let vars = ["LOCALAPPDATA", "USERPROFILE"].map(|v| (v, env::var(v).ok()));
    with_env_vars(&p.display().to_string(), &vars)
}

fn with_env_vars(s: &str, vars: &[(&str, Option<String>)]) -> String {
    for (var, value) in vars {
        let Some(v) = value.as_deref().map(|v| v.trim_end_matches('\\')).filter(|v| !v.is_empty()) else { continue };
        if s.len() > v.len() && s.is_char_boundary(v.len()) && s[..v.len()].eq_ignore_ascii_case(v) && s[v.len()..].starts_with('\\') {
            return format!("%{var}%{}", &s[v.len()..]);
        }
    }
    s.to_string()
}

/// `uninstall` — hook 제거 → 자동 실행 제거 → 보드 종료 → notifier 의 우리 파일 → 명령.
/// 지금 Claude Code 로그인(`.credentials.json`)은 건드리지 않는다. 앱 폴더는 uninstall.ps1 이 지운다.
pub fn uninstall() -> ExitCode {
    if let Err(c) = guard() {
        return c;
    }
    println!("Claude Board 제거");
    let mut ok = true;

    println!("1) 전역 settings.json 에서 hook 제거");
    if let Err(e) = do_uninstall_hooks() {
        eprintln!("  {e}");
        eprintln!("  (hook 제거 실패 — ~/.claude/settings.json 에서 .claude/notifier/ 가 들어간 hook 을 직접 지우세요)");
        ok = false;
    }

    println!("2) 로그인 자동 실행 제거·보드 종료");
    #[cfg(not(windows))]
    println!("  (Windows 가 아니라 건너뜀)");
    #[cfg(windows)]
    {
        if let Err(e) = registry::delete_run(RUN_VALUE) {
            eprintln!("  자동 실행 값을 지우지 못했습니다: {e}");
            ok = false;
        }
        let table = claude_board_core::proc::ProcessTable::capture();
        for pid in table.pids_named("ClaudeBoard.exe") {
            claude_board_core::proc::terminate(pid);
        }
    }

    println!("3) hook 사본·세션 기록·저장한 계정·답변 꾸미기 사본 제거");
    let notifier = paths::notifier_dir();
    let mut files = vec![paths::hook_exe(), paths::app_path_file(), paths::notify_position_file(), notifier.join("accounts.json")];
    for entry in fs::read_dir(&notifier).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(OLD_PREFIX) {
            files.push(entry.path());
        }
    }
    for f in &files {
        ok &= remove(f, fs::remove_file(f));
    }
    ok &= remove_notifier_dirs(&notifier, notifier_redirected());
    let _ = fs::remove_dir(&notifier);

    println!("4) 명령 제거");
    let bin = paths::local_bin();
    for name in [BOARD_CMD, TUI_CMD] {
        let p = bin.join(name);
        ok &= remove(&p, fs::remove_file(&p));
    }

    println!();
    println!("제거를 마쳤습니다. 실행 중인 Claude Code 세션은 다시 시작해야 hook 제거가 적용됩니다.");
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// notifier 아래에서 통째로 지우는 폴더들 — 세션 기록, 저장 계정(accounts\ 에는 DPAPI 로 암호화한 토큰(vault)도 있다),
/// 앱이 옮겨 쓴 md-render 사본(plugins\ — Claude Code 가 안에 만든 tsconfig.json·.claude-plugin\types 포함, macOS `Uninstall.command` 가 plugins 를 지우는 것과 같다).
/// 그 밖 파일은 건드리지 않는다. `plugins\md-render` 가 링크(junction)여도 가리키는 곳은 지우지 않고 링크만 뗀다(`remove_dir_all` 이 링크를 따라가지 않는다).
/// notifier 폴더를 `CLAUDE_BOARD_NOTIFIER_DIR` 로 옮겨 둔 실행(시험·격리)에서는 `plugins` 를 지우지 않는다 — 그 폴더에는 이 보드가 옮긴 것 밖의 것이 있을 수 있다.
/// `packaging/uninstall.ps1` 도 같은 조건으로 같은 범위를 지운다.
fn remove_notifier_dirs(notifier: &Path, redirected: bool) -> bool {
    let names: &[&str] = if redirected { &["sessions", "accounts"] } else { &["sessions", "accounts", "plugins"] };
    names.iter().fold(true, |ok, name| {
        let dir = notifier.join(name);
        remove(&dir, fs::remove_dir_all(&dir)) && ok
    })
}

/// notifier 폴더가 환경변수로 옮겨졌는가.
fn notifier_redirected() -> bool {
    env::var_os(paths::NOTIFIER_ENV).is_some_and(|v| !v.is_empty())
}

/// 지우기 결과 — 없던 것은 성공으로 본다.
fn remove(path: &Path, r: io::Result<()>) -> bool {
    match r {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            eprintln!("  {} 을 지우지 못했습니다: {e}", path.display());
            false
        }
    }
}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

    pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// HKCU\…\Run 에 문자열 값을 쓴다.
    pub fn set_run(name: &str, value: &str) -> std::io::Result<()> {
        let (key, name, data) = (wide(RUN_KEY), wide(name), wide(value));
        // SAFETY: NUL 로 끝나는 문자열과 바이트 길이(NUL 포함)를 넘긴다
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
        };
        if r == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(r as i32))
        }
    }

    /// HKCU\…\Run 의 값을 지운다(없으면 성공).
    pub fn delete_run(name: &str) -> std::io::Result<()> {
        let (key, name) = (wide(RUN_KEY), wide(name));
        // SAFETY: NUL 로 끝나는 문자열
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(r as i32))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = env::temp_dir().join(format!("cb-hook-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    #[test]
    fn uninstall_removes_md_render_copy_with_files_claude_wrote() {
        let notifier = scratch("plugins");
        for f in ["LICENSE", "hooks/register.tsx", ".claude-plugin/plugin.json", "tsconfig.json", ".claude-plugin/types/index.d.ts"] {
            touch(&notifier.join("plugins").join("md-render").join(f));
        }
        touch(&notifier.join("sessions").join("a.json"));
        touch(&notifier.join("accounts").join("vault").join("b.bin"));
        touch(&notifier.join("claude-board-hook.exe"));
        assert!(remove_notifier_dirs(&notifier, false));
        for gone in ["plugins", "sessions", "accounts"] {
            assert!(!notifier.join(gone).exists(), "{gone}");
        }
        // 폴더만 지운다 — 파일(hook 사본 등)은 따로 지운다
        assert!(notifier.join("claude-board-hook.exe").exists());
        fs::remove_dir_all(&notifier).unwrap();
    }

    #[test]
    fn uninstall_leaves_plugins_alone_when_notifier_is_redirected() {
        let notifier = scratch("redirected");
        touch(&notifier.join("plugins").join("md-render").join("LICENSE"));
        touch(&notifier.join("plugins").join("other").join("keep.txt"));
        touch(&notifier.join("sessions").join("a.json"));
        touch(&notifier.join("accounts").join("b.json"));
        assert!(remove_notifier_dirs(&notifier, true));
        assert!(notifier.join("plugins").join("md-render").join("LICENSE").exists());
        assert!(notifier.join("plugins").join("other").join("keep.txt").exists());
        assert!(!notifier.join("sessions").exists());
        assert!(!notifier.join("accounts").exists());
        fs::remove_dir_all(&notifier).unwrap();
    }

    #[test]
    fn uninstall_dirs_missing_is_success() {
        let notifier = scratch("empty");
        assert!(remove_notifier_dirs(&notifier, false));
        assert!(remove_notifier_dirs(&notifier.join("none"), false));
        fs::remove_dir_all(&notifier).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn uninstall_removes_only_the_link_when_md_render_is_a_junction() {
        let notifier = scratch("junction");
        let elsewhere = scratch("elsewhere");
        touch(&elsewhere.join("keep.txt"));
        fs::create_dir_all(notifier.join("plugins")).unwrap();
        let link = notifier.join("plugins").join("md-render");
        // mklink /J 는 관리자 권한이 필요 없다. 못 만들면(정책) 이 시험은 의미가 없어 건너뛴다
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&elsewhere).output().map(|o| o.status.success()).unwrap_or(false);
        if made {
            assert!(remove_notifier_dirs(&notifier, false));
            assert!(!link.exists());
            assert!(elsewhere.join("keep.txt").exists(), "링크가 가리키는 곳은 남아야 한다");
        }
        fs::remove_dir_all(&notifier).unwrap();
        fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[test]
    fn cmd_paths_use_env_vars() {
        let vars = [("LOCALAPPDATA", Some(r"C:\Users\홍길동\AppData\Local".to_string())), ("USERPROFILE", Some(r"C:\Users\홍길동\".to_string()))];
        assert_eq!(
            with_env_vars(r"C:\Users\홍길동\AppData\Local\Programs\Claude Board\ClaudeBoard.exe", &vars),
            r"%LOCALAPPDATA%\Programs\Claude Board\ClaudeBoard.exe"
        );
        assert_eq!(
            with_env_vars(r"c:\users\홍길동\.claude\notifier\claude-board-hook.exe", &vars),
            r"%USERPROFILE%\.claude\notifier\claude-board-hook.exe"
        );
        assert_eq!(with_env_vars(r"D:\Apps\ClaudeBoard.exe", &vars), r"D:\Apps\ClaudeBoard.exe");
        assert_eq!(with_env_vars(r"C:\Users\홍길동2\x.exe", &vars), r"C:\Users\홍길동2\x.exe");
        assert_eq!(with_env_vars("/opt/board/ClaudeBoard", &[("USERPROFILE", None)]), "/opt/board/ClaudeBoard");
    }
}
