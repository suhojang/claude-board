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
const BROKEN_PREFIX: &str = "accounts.json.broken-";

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

    // 5) 로그인할 때 자동 실행 — 설정 > 일반에서 끈 사람(표지 파일 login-launch-off)은 등록하지 않는다(mac user-setup.sh 6)
    #[cfg(windows)]
    println!("  {}", register_login_launch(app, &paths::login_launch_off_file(), paths::overridden(), &registry::Real)?);
    println!("설정을 마쳤습니다.");
    Ok(())
}

/// 로그인할 때 자동 실행을 등록해야 하는가 — 표지 파일 `login-launch-off` 가 없을 때만(처음 설치는 켠 채로 시작한다).
#[cfg_attr(not(windows), allow(dead_code))]
fn login_launch_wanted(off_mark: &Path) -> bool {
    !off_mark.exists()
}

/// 시작 프로그램 값(HKCU\…\Run)을 쓰고 지우는 곳 — 시험은 가짜를 넣는다.
#[cfg_attr(not(windows), allow(dead_code))]
trait RunSink {
    fn set(&self, name: &str, value: &str) -> io::Result<()>;
    /// 값을 지운다. 없으면 성공.
    fn delete(&self, name: &str) -> io::Result<()>;
}

/// setup 5단계 — 등록하거나 지우고, 화면에 낼 한 줄을 돌려준다.
/// `redirected`(CLAUDE_BOARD_HOME·CLAUDE_BOARD_NOTIFIER_DIR 로 폴더를 바꾼 실행)면 표지 파일은 바뀐 폴더에서 읽되
/// 진짜 Run 값은 쓰지도 지우지도 않는다 — 시험·격리 실행이 설치된 보드의 자동 실행을 건드리지 않게.
#[cfg_attr(not(windows), allow(dead_code))]
fn register_login_launch(app: &Path, off_mark: &Path, redirected: bool, run: &dyn RunSink) -> Result<String, String> {
    if redirected {
        return Ok("자동 실행: 건너뜀(폴더를 바꿔 띄운 실행은 시작 프로그램을 건드리지 않음)".to_string());
    }
    if login_launch_wanted(off_mark) {
        let value = format!("\"{}\" --background", app.display());
        run.set(RUN_VALUE, &value).map_err(|e| format!("자동 실행을 등록하지 못했습니다: {e}"))?;
        Ok(format!("자동 실행: HKCU 시작 프로그램 {RUN_VALUE}"))
    } else {
        // 이미 있는 값도 지운다 — 끄기가 값을 못 지웠어도 업데이트 뒤에는 꺼진 채로 남는다
        run.delete(RUN_VALUE).map_err(|e| format!("자동 실행 값을 지우지 못했습니다: {e}"))?;
        Ok("자동 실행: 건너뜀(설정에서 로그인할 때 열기를 꺼 둠)".to_string())
    }
}

/// uninstall 2단계의 Run 값 지우기 — 폴더를 바꿔 띄운 실행은 진짜 Run 값을 지우지 않는다.
#[cfg_attr(not(windows), allow(dead_code))]
fn unregister_login_launch(redirected: bool, run: &dyn RunSink) -> Result<(), String> {
    if redirected {
        return Ok(());
    }
    run.delete(RUN_VALUE).map_err(|e| format!("자동 실행 값을 지우지 못했습니다: {e}"))
}

/// 자율주행이 작업 스케줄러에 등록한 작업의 자리 — 작업 폴더와 그 폴더 작업 이름의 앞머리.
/// 이름은 `com.claude.board[.dev].autopilot.<실행 id>.<경로 해시 8자>` 이다(autopilot 크레이트 `adapters::supervisor`·`cli::label_for_dir`).
/// hook 크레이트는 autopilot 크레이트에 기대지 않으므로(의존 방향) 폴더·이름 꼴을 여기에 따로 둔다.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy)]
struct TaskScope {
    folder: &'static str,
    prefix: &'static str,
}

#[cfg_attr(not(windows), allow(dead_code))]
const AUTOPILOT_SCOPES: [TaskScope; 2] = [
    TaskScope { folder: r"\ClaudeBoard\autopilot\", prefix: "com.claude.board.autopilot." },
    TaskScope { folder: r"\ClaudeBoard\autopilot-dev\", prefix: "com.claude.board.dev.autopilot." },
];

/// 작업 스케줄러를 읽고 지우는 곳 — 시험은 가짜를 넣는다. 작업은 폴더를 포함한 전체 경로(`\ClaudeBoard\autopilot\<이름>`)로 다룬다.
#[cfg_attr(not(windows), allow(dead_code))]
trait TaskSink {
    fn list(&self) -> Result<Vec<String>, String>;
    fn delete(&self, task_path: &str) -> Result<(), String>;
}

/// `<실행 id>.<해시 8자>` 꼴인가 — 실행 id 는 autopilot 의 `valid_run_id`(1~120자, `[A-Za-z0-9._-]`, 점만이거나 점으로 끝나지 않음)와 같다.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_run_label_tail(tail: &str) -> bool {
    let Some((id, hash)) = tail.rsplit_once('.') else { return false };
    let hash_ok = hash.len() == 8 && hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'));
    let id_ok = (1..=120).contains(&id.len())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && id.chars().any(|c| c != '.')
        && !id.ends_with('.');
    hash_ok && id_ok
}

/// 작업 목록(전체 경로) 중 지울 것만 고른다 — 폴더 **바로 아래**이고(대소문자 무시) 이름이 그 폴더의 앞머리 + `<실행 id>.<해시>` 인 것.
/// 다른 폴더·하위 폴더·꼴이 다른 이름은 고르지 않는다. 같은 경로는 한 번만.
#[cfg_attr(not(windows), allow(dead_code))]
fn select_autopilot_tasks(listing: &[String], scopes: &[TaskScope]) -> Vec<String> {
    let mut picked: Vec<String> = Vec::new();
    for path in listing {
        for scope in scopes {
            let Some(head) = path.get(..scope.folder.len()) else { continue };
            if !head.eq_ignore_ascii_case(scope.folder) {
                continue;
            }
            let name = &path[scope.folder.len()..];
            let Some(tail) = name.strip_prefix(scope.prefix) else { continue };
            if !name.contains('\\') && is_run_label_tail(tail) && !picked.contains(path) {
                picked.push(path.clone());
            }
        }
    }
    picked
}

/// 한 번의 정리 결과
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Default, PartialEq, Eq)]
struct TaskCleanup {
    /// 격리 실행이라 건드리지 않았다
    skipped: bool,
    removed: Vec<String>,
    /// (작업 경로, 이유) — 하나가 실패해도 나머지는 계속 지운다
    failed: Vec<(String, String)>,
    /// 목록을 못 읽었다
    list_error: Option<String>,
}

/// uninstall 의 자율주행 작업 스케줄러 등록 지우기. 작업만 지운다 — 돌고 있는 엔진은 끝내지 않는다
/// (엔진은 시작 때 실행 파일 사본을 따로 쥐고 있어 설치 폴더를 지워도 마저 돈다. 등록이 사라지면 죽어도 다시 뜨지 않을 뿐이다).
/// `redirected`(폴더를 바꿔 띄운 실행)면 진짜 스케줄러를 읽지도 지우지도 않는다 — Run 값과 같은 규칙.
/// 실패는 uninstall 의 종료 코드를 바꾸지 않는다(호출 쪽이 알린다) — 남은 등록은 `claude-autopilot unregister --all` 로 지운다.
#[cfg_attr(not(windows), allow(dead_code))]
fn remove_autopilot_tasks(redirected: bool, scopes: &[TaskScope], sink: &dyn TaskSink) -> TaskCleanup {
    let mut out = TaskCleanup::default();
    if redirected {
        out.skipped = true;
        return out;
    }
    let listing = match sink.list() {
        Ok(l) => l,
        Err(e) => {
            out.list_error = Some(e);
            return out;
        }
    };
    for path in select_autopilot_tasks(&listing, scopes) {
        match sink.delete(&path) {
            Ok(()) => out.removed.push(path),
            Err(e) => out.failed.push((path, e)),
        }
    }
    out
}

/// 화면에 낼 줄들
#[cfg_attr(not(windows), allow(dead_code))]
fn describe_cleanup(c: &TaskCleanup) -> Vec<String> {
    if c.skipped {
        return vec!["  자율주행 작업 스케줄러: 건너뜀(폴더를 바꿔 띄운 실행은 진짜 스케줄러를 건드리지 않음)".to_string()];
    }
    if let Some(e) = &c.list_error {
        return vec![format!("  자율주행 작업 스케줄러 목록을 읽지 못했습니다: {e}")];
    }
    let mut lines = vec![format!("  자율주행 작업 스케줄러: {}개 지움", c.removed.len())];
    lines.extend(c.failed.iter().map(|(p, e)| format!("  {p} 을 지우지 못했습니다: {e}")));
    lines
}

/// `schtasks /query /fo csv /nh` 출력 → 작업 전체 경로 목록(줄의 첫 칸)
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_task_paths(csv: &str) -> Vec<String> {
    csv.lines().filter_map(|line| first_csv_cell(line.trim())).filter(|c| c.starts_with('\\')).collect()
}

/// 큰따옴표로 감싼 첫 칸(`""` 는 `"`). 따옴표로 시작하지 않거나 닫히지 않으면 None
#[cfg_attr(not(windows), allow(dead_code))]
fn first_csv_cell(line: &str) -> Option<String> {
    let mut chars = line.strip_prefix('"')?.chars().peekable();
    let mut cell = String::new();
    while let Some(c) = chars.next() {
        if c != '"' {
            cell.push(c);
        } else if chars.peek() == Some(&'"') {
            chars.next();
            cell.push('"');
        } else {
            return Some(cell);
        }
    }
    None
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
        if let Err(e) = unregister_login_launch(paths::overridden(), &registry::Real) {
            eprintln!("  {e}");
            ok = false;
        }
        // 자율주행이 남긴 작업 스케줄러 등록 — 실패해도 제거를 실패로 치지 않는다(돌던 엔진은 끝내지 않는다)
        for line in describe_cleanup(&remove_autopilot_tasks(paths::overridden(), &AUTOPILOT_SCOPES, &scheduler::Real)) {
            println!("{line}");
        }
        let table = claude_board_core::proc::ProcessTable::capture();
        for pid in table.pids_named("ClaudeBoard.exe") {
            claude_board_core::proc::terminate(pid);
        }
    }

    println!("3) hook 사본·세션 기록·저장한 계정·답변 꾸미기 사본 제거");
    let notifier = paths::notifier_dir();
    ok &= remove_notifier_files(&notifier);
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

/// notifier 에서 이름이 정해진 우리 파일들 — hook 사본·앱 경로·알림 위치·로그인할 때 열기를 끈 표지(mac `Uninstall.command` 가 `login-launch-off` 를 지운다)·저장 계정 목록.
/// 경로는 넘겨받은 notifier 기준이다(시험이 임시 폴더로 부른다).
fn fixed_files(notifier: &Path) -> Vec<PathBuf> {
    vec![
        notifier.join(paths::HOOK_EXE),
        notifier.join("app-path"),
        notifier.join("notify-position"),
        notifier.join("login-launch-off"),
        notifier.join("accounts.json"),
    ]
}

/// notifier 바로 아래의 보드 자기 파일들을 지운다 — 이름이 정해진 파일(`fixed_files`)과
/// 깨진 목록 백업(`accounts.json.broken-<시각>`, mac `Uninstall.command` 과 같다), 지난 setup 이 비켜 둔 사본.
/// 그 밖의 파일은 건드리지 않는다.
fn remove_notifier_files(notifier: &Path) -> bool {
    let mut files = fixed_files(notifier);
    for entry in fs::read_dir(notifier).into_iter().flatten().flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with(OLD_PREFIX) || file_name.starts_with(BROKEN_PREFIX) {
            files.push(entry.path());
        }
    }
    let mut ok = true;
    for f in &files {
        ok &= remove(f, fs::remove_file(f));
    }
    ok
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

/// 진짜 작업 스케줄러 — `schtasks.exe` 를 부른다(COM 을 쓰지 않는다. autopilot 의 어댑터와 같은 길).
#[cfg(windows)]
mod scheduler {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// 콘솔 창이 깜빡이지 않게
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub struct Real;

    /// 종료 코드와 stdout·stderr 를 합친 글(OEM 코드 페이지라 글은 판정에 쓰지 않는다)
    fn run(args: &[&str]) -> Result<(i32, String), String> {
        let exe = std::env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("schtasks.exe"))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("schtasks.exe"));
        let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.code().unwrap_or(-1), text))
    }

    pub fn query_csv() -> Result<String, String> {
        let (code, text) = run(&["/query", "/fo", "csv", "/nh"])?;
        if code == 0 {
            Ok(text)
        } else {
            Err(format!("schtasks /query 실패(종료 코드 {code}): {}", text.trim().chars().take(200).collect::<String>()))
        }
    }

    pub fn delete(task_path: &str) -> Result<(), String> {
        let (code, text) = run(&["/delete", "/tn", task_path, "/f"])?;
        if code == 0 {
            Ok(())
        } else {
            Err(format!("schtasks /delete 실패(종료 코드 {code}): {}", text.trim().chars().take(200).collect::<String>()))
        }
    }

    #[cfg(test)]
    pub fn create_throwaway(task_path: &str) -> Result<(), String> {
        let (code, text) = run(&["/create", "/tn", task_path, "/tr", "cmd.exe /c exit 0", "/sc", "once", "/st", "23:59"])?;
        if code == 0 {
            Ok(())
        } else {
            Err(format!("schtasks /create 실패(종료 코드 {code}): {}", text.trim()))
        }
    }

    impl super::TaskSink for Real {
        fn list(&self) -> Result<Vec<String>, String> {
            query_csv().map(|csv| super::parse_task_paths(&csv))
        }
        fn delete(&self, task_path: &str) -> Result<(), String> {
            delete(task_path)
        }
    }
}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 진짜 HKCU\…\Run
    pub struct Real;

    impl super::RunSink for Real {
        fn set(&self, name: &str, value: &str) -> std::io::Result<()> {
            set_run(name, value)
        }
        fn delete(&self, name: &str) -> std::io::Result<()> {
            delete_run(name)
        }
    }

    /// HKCU\…\Run 에 문자열 값을 쓴다.
    fn set_run(name: &str, value: &str) -> std::io::Result<()> {
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
    fn delete_run(name: &str) -> std::io::Result<()> {
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
    fn uninstall_removes_broken_accounts_backups() {
        let notifier = scratch("broken-accounts");
        // 깨진 accounts.json 은 accounts.json.broken-<시각>(-2) 으로 옮겨진다. 제거 때 이 백업들도 지운다(mac Uninstall.command 와 같다)
        touch(&notifier.join("accounts.json.broken-20261029-101010"));
        touch(&notifier.join("accounts.json.broken-20261029-101010-2"));
        touch(&notifier.join("keep.txt"));
        assert!(remove_notifier_files(&notifier));
        assert!(!notifier.join("accounts.json.broken-20261029-101010").exists(), "broken 백업(-1)은 지워진다");
        assert!(!notifier.join("accounts.json.broken-20261029-101010-2").exists(), "broken 백업(-2)은 지워진다");
        assert!(notifier.join("keep.txt").exists(), "관계 없는 파일은 남아야 한다");
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
    fn login_launch_is_registered_only_without_the_off_mark() {
        let dir = scratch("login-mark");
        let mark = dir.join("login-launch-off");
        assert!(login_launch_wanted(&mark), "표지가 없으면(처음 설치) 등록한다");
        touch(&mark);
        assert!(!login_launch_wanted(&mark), "끈 사람은 업데이트가 다시 등록하지 않는다");
        fs::remove_file(&mark).unwrap();
        assert!(login_launch_wanted(&mark));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// 가짜 Run 자리 — 진짜 레지스트리를 건드리지 않는다. 쓴 것과 지운 것의 이름을 센다
    #[derive(Default)]
    struct FakeRun {
        sets: std::cell::RefCell<Vec<(String, String)>>,
        deletes: std::cell::RefCell<Vec<String>>,
    }

    impl RunSink for FakeRun {
        fn set(&self, name: &str, value: &str) -> io::Result<()> {
            self.sets.borrow_mut().push((name.to_string(), value.to_string()));
            Ok(())
        }
        fn delete(&self, name: &str) -> io::Result<()> {
            self.deletes.borrow_mut().push(name.to_string());
            Ok(())
        }
    }

    #[test]
    fn setup_writes_or_deletes_the_run_value_by_the_off_mark() {
        let dir = scratch("run-sink");
        let mark = dir.join("login-launch-off");
        let app = Path::new(r"C:\Apps\ClaudeBoard.exe");
        let run = FakeRun::default();
        register_login_launch(app, &mark, false, &run).unwrap();
        assert_eq!(*run.sets.borrow(), [(RUN_VALUE.to_string(), r#""C:\Apps\ClaudeBoard.exe" --background"#.to_string())]);
        assert!(run.deletes.borrow().is_empty());
        touch(&mark);
        register_login_launch(app, &mark, false, &run).unwrap();
        assert_eq!(*run.deletes.borrow(), [RUN_VALUE]);
        assert_eq!(run.sets.borrow().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// 폴더를 바꿔 띄운 setup·uninstall 은 표지가 어떻든 진짜 Run 값을 쓰지도 지우지도 않는다
    #[test]
    fn redirected_setup_and_uninstall_never_touch_the_run_value() {
        let dir = scratch("run-redirected");
        let mark = dir.join("login-launch-off");
        let app = Path::new(r"C:\Apps\ClaudeBoard.exe");
        let run = FakeRun::default();
        for marked in [false, true] {
            if marked {
                touch(&mark);
            }
            register_login_launch(app, &mark, true, &run).unwrap();
            unregister_login_launch(true, &run).unwrap();
        }
        assert!(run.sets.borrow().is_empty() && run.deletes.borrow().is_empty());
        unregister_login_launch(false, &run).unwrap();
        assert_eq!(*run.deletes.borrow(), [RUN_VALUE]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn uninstall_removes_the_off_mark_with_the_other_fixed_files() {
        let files = fixed_files(&paths::notifier_dir());
        for expected in [paths::login_launch_off_file(), paths::notify_position_file(), paths::app_path_file(), paths::hook_exe()] {
            assert!(files.contains(&expected), "{}", expected.display());
        }
        // 지우는 일은 있는 파일만 지우고 없는 파일은 성공으로 본다
        let dir = scratch("fixed");
        let mark = dir.join("login-launch-off");
        touch(&mark);
        assert!(remove(&mark, fs::remove_file(&mark)));
        assert!(!mark.exists());
        assert!(remove(&mark, fs::remove_file(&mark)));
        fs::remove_dir_all(&dir).unwrap();
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

    // ── 자율주행 작업 스케줄러 정리 ──

    const HASH: &str = "0123abcd";

    fn full(folder: &str, prefix: &str, id: &str) -> String {
        format!("{folder}{prefix}{id}.{HASH}")
    }

    /// 가짜 작업 스케줄러 — 목록을 주고, 지우려 한 경로를 기록한다. `fail` 에 든 경로는 지우기가 실패한다
    struct FakeTasks {
        tasks: Vec<String>,
        fail: Vec<String>,
        list_fails: bool,
        deleted: std::cell::RefCell<Vec<String>>,
    }

    impl FakeTasks {
        fn new(tasks: Vec<String>) -> Self {
            Self { tasks, fail: Vec::new(), list_fails: false, deleted: Default::default() }
        }
    }

    impl TaskSink for FakeTasks {
        fn list(&self) -> Result<Vec<String>, String> {
            if self.list_fails {
                Err("목록 실패".to_string())
            } else {
                Ok(self.tasks.clone())
            }
        }
        fn delete(&self, task_path: &str) -> Result<(), String> {
            self.deleted.borrow_mut().push(task_path.to_string());
            if self.fail.iter().any(|f| f == task_path) {
                Err("거부".to_string())
            } else {
                Ok(())
            }
        }
    }

    const NON_DEV_FOLDER: &str = r"\ClaudeBoard\autopilot\";
    const DEV_FOLDER: &str = r"\ClaudeBoard\autopilot-dev\";
    const NON_DEV_PREFIX: &str = "com.claude.board.autopilot.";
    const DEV_PREFIX: &str = "com.claude.board.dev.autopilot.";

    #[test]
    fn selects_matching_dev_and_non_dev_tasks_only() {
        let unrelated = [
            r"\Microsoft\Windows\Defrag\ScheduledDefrag".to_string(),
            r"\ClaudeBoard\autopilot\other-task".to_string(),
            format!(r"\ClaudeBoard\autopilot-dev\{NON_DEV_PREFIX}run-3.{HASH}"), // 폴더와 앞머리가 어긋남
            format!(r"\ClaudeBoard\autopilot\{DEV_PREFIX}run-4.{HASH}"),
            format!(r"\Other\autopilot\{NON_DEV_PREFIX}run-5.{HASH}"), // 다른 폴더
            format!(r"\ClaudeBoard\autopilot\sub\{NON_DEV_PREFIX}run-6.{HASH}"), // 하위 폴더
            format!(r"\ClaudeBoard\autopilot\{NON_DEV_PREFIX}run-7"), // 해시 없음
            format!(r"\ClaudeBoard\autopilot\{NON_DEV_PREFIX}run-8.0123ABCD"), // 대문자 해시
            format!(r"\ClaudeBoard\autopilot\{NON_DEV_PREFIX}run-9.0123abc"), // 7자
            format!(r"\ClaudeBoard\autopilot\{NON_DEV_PREFIX}.{HASH}"), // 실행 id 없음
            format!(r"\ClaudeBoard\autopilot\{NON_DEV_PREFIX}...{HASH}"), // 점만인 id
            format!(r"\ClaudeBoard\autopilot\x{NON_DEV_PREFIX}run-1.{HASH}"), // 앞머리 앞에 글자
            format!(r"\ClaudeBoard\autopilot\{NON_DEV_PREFIX}run 1.{HASH}"), // 허용하지 않는 글자
        ];
        let want = vec![full(NON_DEV_FOLDER, NON_DEV_PREFIX, "run-1"), full(DEV_FOLDER, DEV_PREFIX, "run.2_b")];
        let mut listing = unrelated.to_vec();
        listing.insert(1, want[0].clone());
        listing.push(want[1].clone());
        listing.push(want[0].clone()); // 같은 경로가 둘이어도 한 번
        assert_eq!(select_autopilot_tasks(&listing, &AUTOPILOT_SCOPES), want);
        // 폴더는 대소문자를 가리지 않는다(작업 스케줄러가 그렇다)
        let upper = full(r"\CLAUDEBOARD\AUTOPILOT\", NON_DEV_PREFIX, "run-1");
        assert_eq!(select_autopilot_tasks(std::slice::from_ref(&upper), &AUTOPILOT_SCOPES), [upper]);
    }

    #[test]
    fn empty_listing_removes_nothing() {
        let fake = FakeTasks::new(Vec::new());
        let r = remove_autopilot_tasks(false, &AUTOPILOT_SCOPES, &fake);
        assert_eq!(r, TaskCleanup::default());
        assert!(fake.deleted.borrow().is_empty());
        assert_eq!(describe_cleanup(&r), ["  자율주행 작업 스케줄러: 0개 지움"]);
    }

    #[test]
    fn one_delete_failure_continues_with_the_rest() {
        let (a, b, c) = (full(NON_DEV_FOLDER, NON_DEV_PREFIX, "a"), full(DEV_FOLDER, DEV_PREFIX, "b"), full(NON_DEV_FOLDER, NON_DEV_PREFIX, "c"));
        let mut fake = FakeTasks::new(vec![a.clone(), r"\Unrelated\x".to_string(), b.clone(), c.clone()]);
        fake.fail = vec![b.clone()];
        let r = remove_autopilot_tasks(false, &AUTOPILOT_SCOPES, &fake);
        assert_eq!(*fake.deleted.borrow(), [a.clone(), b.clone(), c.clone()], "관계없는 작업은 지우기를 시도하지도 않는다");
        assert_eq!(r.removed, [a, c]);
        assert_eq!(r.failed, [(b, "거부".to_string())]);
        assert!(r.list_error.is_none());
        assert_eq!(describe_cleanup(&r).len(), 2);
    }

    #[test]
    fn list_failure_deletes_nothing_and_is_reported() {
        let mut fake = FakeTasks::new(vec![full(NON_DEV_FOLDER, NON_DEV_PREFIX, "a")]);
        fake.list_fails = true;
        let r = remove_autopilot_tasks(false, &AUTOPILOT_SCOPES, &fake);
        assert!(fake.deleted.borrow().is_empty());
        assert_eq!(r.list_error.as_deref(), Some("목록 실패"));
        assert!(describe_cleanup(&r)[0].contains("목록 실패"));
    }

    /// 폴더를 바꿔 띄운 uninstall 은 진짜 스케줄러를 읽지도 지우지도 않는다
    #[test]
    fn redirected_uninstall_never_touches_the_scheduler() {
        let fake = FakeTasks::new(vec![full(NON_DEV_FOLDER, NON_DEV_PREFIX, "a")]);
        let r = remove_autopilot_tasks(true, &AUTOPILOT_SCOPES, &fake);
        assert!(r.skipped && r.removed.is_empty());
        assert!(fake.deleted.borrow().is_empty());
        assert!(describe_cleanup(&r)[0].contains("건너뜀"));
    }

    #[test]
    fn parses_schtasks_csv_first_column() {
        let csv = "\"\\ClaudeBoard\\autopilot\\com.claude.board.autopilot.r.0123abcd\",\"N/A\",\"Ready\"\r\n\
                   \"TaskName\",\"Next Run Time\",\"Status\"\r\n\
                   \r\n\
                   \"\\Microsoft\\X\",\"2026-01-01\",\"Ready\"\r\n";
        assert_eq!(parse_task_paths(csv), [r"\ClaudeBoard\autopilot\com.claude.board.autopilot.r.0123abcd", r"\Microsoft\X"]);
    }

    /// 진짜 작업 스케줄러 — 시험 폴더 아래에 같은 꼴의 작업 둘(일반·dev)과 관계없는 하나를 만들고 정리한 뒤 사라졌는지 본다.
    /// 사용자의 진짜 `\ClaudeBoard\autopilot*` 폴더는 건드리지 않는다.
    #[cfg(windows)]
    #[test]
    fn real_scheduler_removes_only_the_matching_throwaway_tasks() {
        use super::TaskSink as _;
        let scopes = [
            TaskScope { folder: r"\ClaudeBoardHookTest\w14a\", prefix: NON_DEV_PREFIX },
            TaskScope { folder: r"\ClaudeBoardHookTest\w14b\", prefix: DEV_PREFIX },
        ];
        let mine = [full(scopes[0].folder, NON_DEV_PREFIX, "hooktest-1"), full(scopes[1].folder, DEV_PREFIX, "hooktest-2")];
        let unrelated = format!("{}unrelated-hooktest", scopes[0].folder);
        let cleanup = || {
            for p in mine.iter().chain(std::iter::once(&unrelated)) {
                let _ = scheduler::delete(p);
            }
        };
        cleanup();
        for p in mine.iter().chain(std::iter::once(&unrelated)) {
            scheduler::create_throwaway(p).unwrap_or_else(|e| panic!("{p}: {e}"));
        }
        let before = scheduler::Real.list().unwrap();
        for p in mine.iter().chain(std::iter::once(&unrelated)) {
            assert!(before.iter().any(|b| b.eq_ignore_ascii_case(p)), "{p} 가 만들어지지 않았다");
        }
        let r = remove_autopilot_tasks(false, &scopes, &scheduler::Real);
        let after = scheduler::Real.list().unwrap();
        let still: Vec<&String> = mine.iter().filter(|p| after.iter().any(|a| a.eq_ignore_ascii_case(p))).collect();
        let kept = after.iter().any(|a| a.eq_ignore_ascii_case(&unrelated));
        cleanup();
        assert!(r.failed.is_empty() && r.list_error.is_none(), "{r:?}");
        assert!(still.is_empty(), "남았다: {still:?}");
        assert!(kept, "관계없는 작업까지 지웠다");
        assert_eq!(r.removed.len(), 2);
    }
}
