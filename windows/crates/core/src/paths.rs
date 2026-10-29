//! 경로 — `%USERPROFILE%\.claude\notifier\` 아래 파일 약속 (SPEC §1.2, §2).
//!
//! 시험·개발용으로 환경변수로 바꿀 수 있다.
//! - `CLAUDE_BOARD_HOME`: 홈 폴더(`~`) 대신 쓸 폴더 — `.claude`·`.claude.json`·`.local\bin` 이 모두 이 아래로 간다.
//! - `CLAUDE_BOARD_NOTIFIER_DIR`: notifier 폴더만 바꾼다. hook 표시(`.claude/notifier/`)가 경로에 없으면
//!   `install-hooks` 가 넣은 hook 을 나중에 알아보지 못하므로, hook 등록까지 시험할 때는 `CLAUDE_BOARD_HOME` 을 쓴다.

use std::env;
use std::path::PathBuf;

pub const HOME_ENV: &str = "CLAUDE_BOARD_HOME";
pub const NOTIFIER_ENV: &str = "CLAUDE_BOARD_NOTIFIER_DIR";

/// hook 이 실행하는 사본의 파일 이름(Windows 는 `.exe`).
pub const HOOK_EXE: &str = if cfg!(windows) { "claude-board-hook.exe" } else { "claude-board-hook" };

fn env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// 홈 폴더 — `CLAUDE_BOARD_HOME` > Windows `USERPROFILE` / 그 밖 `HOME`.
pub fn home() -> PathBuf {
    if let Some(p) = env_path(HOME_ENV) {
        return p;
    }
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env_path(key).unwrap_or_else(|| PathBuf::from("."))
}

/// 환경변수로 홈이나 notifier 폴더를 바꿨는지 — macOS 개발 기기에서 실제 ~/.claude 를 지키는 데 쓴다.
pub fn overridden() -> bool {
    env_path(HOME_ENV).is_some() || env_path(NOTIFIER_ENV).is_some()
}

/// `~/.claude`
pub fn claude_dir() -> PathBuf {
    home().join(".claude")
}

/// `~/.claude/notifier` (또는 `CLAUDE_BOARD_NOTIFIER_DIR`)
pub fn notifier_dir() -> PathBuf {
    env_path(NOTIFIER_ENV).unwrap_or_else(|| claude_dir().join("notifier"))
}

/// `notifier/sessions` — 세션 기록과 `.lock`·`.paused`·`.mute` 표지
pub fn sessions_dir() -> PathBuf {
    notifier_dir().join("sessions")
}

/// `~/.claude/settings.json` — hook 을 등록하는 전역 설정
pub fn settings_file() -> PathBuf {
    claude_dir().join("settings.json")
}

/// `~/.claude/.credentials.json` — 지금 Claude Code 로그인
pub fn credentials_file() -> PathBuf {
    claude_dir().join(".credentials.json")
}

/// `~/.claude.json` — `oauthAccount` 등 Claude Code 계정 정보
pub fn claude_json() -> PathBuf {
    home().join(".claude.json")
}

/// `notifier/claude-board-hook(.exe)` — settings.json 의 hook 이 실행하는 사본
pub fn hook_exe() -> PathBuf {
    notifier_dir().join(HOOK_EXE)
}

/// `notifier/app-path` — ClaudeBoard.exe 전체 경로 한 줄(setup 이 쓰고 notify 가 읽는다)
pub fn app_path_file() -> PathBuf {
    notifier_dir().join("app-path")
}

/// `notifier/mute-all` — 전체 알림 끔 표지
pub fn mute_all_file() -> PathBuf {
    notifier_dir().join("mute-all")
}

/// `notifier/notify-position` — 알림 위치(없으면 가운데)
pub fn notify_position_file() -> PathBuf {
    notifier_dir().join("notify-position")
}

/// `notifier/login-launch-off` — 로그인할 때 열기를 끈 표지(mac `login-launch-off`). 있으면 설치·업데이트(`setup`)가 시작 프로그램에 다시 등록하지 않고, 제거가 지운다
pub fn login_launch_off_file() -> PathBuf {
    notifier_dir().join("login-launch-off")
}

/// `notifier/board-theme` — TUI 색 테마 이름
pub fn board_theme_file() -> PathBuf {
    notifier_dir().join("board-theme")
}

/// `~/.local/bin` — `claude-board.cmd`·`claude-board-tui.cmd` 를 두는 곳(Claude Code 네이티브 설치가 PATH 에 넣는다)
pub fn local_bin() -> PathBuf {
    home().join(".local").join("bin")
}

/// Claude Code 실행 파일 이름(Windows 는 `claude.exe`).
pub const CLAUDE_EXE: &str = if cfg!(windows) { "claude.exe" } else { "claude" };

/// Claude Code 실행 파일 — PATH 에서 먼저 찾고, 없으면 네이티브 설치 위치 `~/.local/bin/claude(.exe)`.
/// 보드는 이 파일을 [`crate::state::resume_args`] 같은 인자 배열로 직접 실행한다(셸 문자열로 실행하지 않는다).
pub fn claude_exe() -> Option<PathBuf> {
    find_claude_exe(env::var_os("PATH").as_deref(), &local_bin())
}

fn find_claude_exe(path_var: Option<&std::ffi::OsStr>, local_bin: &std::path::Path) -> Option<PathBuf> {
    let from_path = path_var.into_iter().flat_map(env::split_paths).find_map(|dir| {
        // Windows PATH 항목은 큰따옴표로 감싸여 있을 수 있다
        let dir = PathBuf::from(dir.to_string_lossy().trim_matches('"'));
        let exe = dir.join(CLAUDE_EXE);
        (!dir.as_os_str().is_empty() && exe.is_file()).then_some(exe)
    });
    from_path.or_else(|| Some(local_bin.join(CLAUDE_EXE)).filter(|p| p.is_file()))
}

/// 경로의 마지막 이름 — Python `os.path.basename` 처럼 끝이 구분자면 빈 문자열.
/// Windows 는 `\`·`/` 를 모두 구분자로 본다(SPEC §2.1 `project`).
pub fn basename(path: &str) -> &str {
    let seps: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    path.rsplit(seps).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_exe_lookup_order() {
        let root = std::env::temp_dir().join(format!("cb-claude-exe-{}", std::process::id()));
        let (a, b, local) = (root.join("a"), root.join("b"), root.join("local"));
        for d in [&a, &b, &local] {
            std::fs::create_dir_all(d).unwrap();
        }
        let path = env::join_paths([&a, &b]).unwrap();
        // 아무 데도 없으면 None
        assert_eq!(find_claude_exe(Some(&path), &local), None);
        // ~/.local/bin 에만 있으면 그것
        std::fs::write(local.join(CLAUDE_EXE), "").unwrap();
        assert_eq!(find_claude_exe(Some(&path), &local), Some(local.join(CLAUDE_EXE)));
        assert_eq!(find_claude_exe(None, &local), Some(local.join(CLAUDE_EXE)));
        // PATH 에 있으면 PATH 가 먼저(앞 항목부터)
        std::fs::write(b.join(CLAUDE_EXE), "").unwrap();
        assert_eq!(find_claude_exe(Some(&path), &local), Some(b.join(CLAUDE_EXE)));
        // 같은 이름의 폴더는 실행 파일이 아니다
        std::fs::create_dir_all(a.join(CLAUDE_EXE)).unwrap();
        assert_eq!(find_claude_exe(Some(&path), &local), Some(b.join(CLAUDE_EXE)));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn login_launch_off_mark_sits_in_notifier_dir() {
        let mark = login_launch_off_file();
        assert_eq!(mark.file_name().and_then(|n| n.to_str()), Some("login-launch-off"));
        assert_eq!(mark.parent(), Some(notifier_dir().as_path()));
    }

    #[test]
    fn basename_like_python() {
        assert_eq!(basename("/tmp/demo-proj"), "demo-proj");
        assert_eq!(basename("/tmp/demo-proj/"), "");
        assert_eq!(basename("demo"), "demo");
        if cfg!(windows) {
            assert_eq!(basename(r"C:\work\proj"), "proj");
        }
    }
}
