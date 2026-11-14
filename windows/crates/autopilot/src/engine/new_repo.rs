//! 주제로 시작의 "새 폴더 만들기"를 실제 파일 시스템과 git 으로 끝까지 돌리는 엔진 쪽.
//! 판정은 `rules::new_repo`(순수)가 하고, 여기는 그 규칙이 묻는 질문(폴더 상태·만들기·git)을 Windows 에서 답한다.
//! 원본: `Sources/ClaudeBoard/AutopilotTopic.swift` 의 `APFolderFiles`·`APBoardGit`·`createRepo` 의 `apCreateRepo` 부름,
//! 그리고 `apShort`(`Sources/ClaudeBoard/Autopilot.swift`).
//!
//! 약속:
//! - git 은 모두 `ports::Git`(보호 환경을 거는 `adapters::git::ProcessGit`)로만 부른다. 정션·심볼릭 링크(재분석 지점)는
//!   `FILE_ATTRIBUTE_REPARSE_POINT` 로 보고, 따라가 폴더면 폴더로 본다(원본 lstat → stat 1:1. 위치가 링크인 ~/workspace 등).
//!   새 자리가 링크면 무엇이 들든 폴더로 보아 만들지 않는다.
//! - 빈 폴더 판정은 원본의 `.DS_Store`(Mac 찾기 찌거기)에 Windows 찌거기 `desktop.ini`·`Thumbs.db` 를 더한 이름만 남은 폴더다.
//!   Windows 파일 체계가 이름의 대소문자를 가리지 않으므로 비교도 가리지 않는다(결정).
//! - 폴더 읽기가 실패하면 빈 폴더로 본다(원본 `try? contentsOfDirectory … ?? []` 1:1).
//! - `apShort` 의 글자 수는 원본이 자소 묶음(`Character`) 단위지만 여기는 `char` 를 센다(엔진 `leftovers::first_line` 과 같은 근사).
//!   300 자 컷은 ASCII 오류 글에서만 자소 묶음 수와 같은 값이다.
//! - 사용자 전역 git 설정 읽기(`identity`)만 git 을 직접 띄운다 — `ports::Git` 은 전역 설정을 빈 장치로 막아 보호하므로
//!   이 질문(원본 `git config --global --get` 읽기만의 명령)은 그 보호를 거치면 답을 못 얻는다. 찾기는
//!   `adapters::git::find_git` 이 private 라 같은 순서(PATH → Program Files\Git\cmd)의 짧은 사본을 둔다(2회째 중복).
//!   명령은 설정 글을 읽기만 하고 환경을 만지지 않는다.

use crate::ports::Git;
use crate::rules::new_repo::{create_repo, CreateResult, Folders, GitRunner, NewRepoPlan, PathState};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
const GIT_EXE: &str = "git.exe";
#[cfg(not(windows))]
const GIT_EXE: &str = "git";

// ── 폴더 상태 ────────────────────────────────────────

/// 실제 파일 시스템으로 답하는 `Folders`. 원본: `APFolderFiles`
pub struct FolderFiles;

#[cfg(windows)]
fn is_reparse(m: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
}

#[cfg(not(windows))]
fn is_reparse(_m: &Metadata) -> bool {
    false
}

/// 빈 폴더로 두어도 되는 찌거기 이름. 원본은 `.DS_Store` 만 본다
fn is_junk_name(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), ".ds_store" | "desktop.ini" | "thumbs.db")
}

/// 폴더 안의 이름들. 열기·읽기가 실패하면 None
fn read_dir_names(path: &str) -> Option<Vec<String>> {
    let rd = std::fs::read_dir(path).ok()?;
    let mut names = Vec::new();
    for e in rd {
        names.push(e.ok()?.file_name().to_string_lossy().into_owned());
    }
    Some(names)
}

impl Folders for FolderFiles {
    /// 링크(정션·심볼릭 링크)를 따라가 폴더인지만 본다. 폴더면 폴더다(그 안이 찌거기뿐이어도 빈 폴더로 보지 않는다 — 원본도 링크의 이름들을 읽지 않는다)
    fn state(&self, path: &str) -> PathState {
        let m = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => return PathState::Missing,
        };
        let ft = m.file_type();
        if ft.is_symlink() || is_reparse(&m) {
            return match std::fs::metadata(path) {
                Ok(t) if t.is_dir() => PathState::Dir,
                _ => PathState::Other,
            };
        }
        if !ft.is_dir() {
            return PathState::Other;
        }
        match read_dir_names(path) {
            None => PathState::EmptyDir, // 읽지 못하면 빈 폴더로 본다(원본 `?? []` 1:1)
            Some(names) if names.iter().all(|n| is_junk_name(n)) => PathState::EmptyDir,
            Some(_) => PathState::Dir,
        }
    }

    /// 폴더 하나만 만든다(부모는 만들지 않는다)
    fn make_dir(&self, path: &str) -> bool {
        std::fs::create_dir(path).is_ok()
    }
}

// ── 오류 글 줄이기 ───────────────────────────────────

/// 오류 글 상한(원본 `apShort` 의 기본값)
const SHORT_MAX: usize = 300;

/// 앞부분만 남기고 `…` 를 붙인다. 끝까지 남으면 그대로. 원본: `apShort`
pub fn short_message(s: &str, n: usize) -> String {
    let mut chars = s.chars();
    let taken: String = chars.by_ref().take(n).collect();
    if chars.next().is_none() {
        s.to_string()
    } else {
        format!("{taken}…")
    }
}

// ── 보드 git ─────────────────────────────────────────

/// 보드가 부르는 git(`ports::Git` 과 같은 보호: 저장소·전역·시스템 설정의 hooks·fsmonitor·attributes·replace 를 쓰지 않음).
/// 실패하면 오류 글도 함께 돌려준다. 원본: `APBoardGit`
pub struct BoardGit<'a> {
    pub git: &'a dyn Git,
}

impl GitRunner for BoardGit<'_> {
    /// 성공은 표준 출력(앞뒤 빈칸을 깎은 것), 실패는 표준 오류를 `SHORT_MAX` 로 줄인 것.
    /// git 을 띄우지 못했으면(-1, 까닭). 원본: `APBoardGit.run`
    fn run(&self, args: &[String], dir: &str) -> (i32, String) {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.git.run(Path::new(dir), &refs) {
            Ok(out) if out.code == 0 => (out.code, out.stdout.trim().to_string()),
            Ok(out) => (out.code, short_message(out.stderr.trim(), SHORT_MAX)),
            Err(e) => (-1, format!("git 을 띄우지 못했습니다: {e}")), // 원본은 까닭 없는 한 줄이지만 어댑터 오류가 있어 함께 보인다
        }
    }

    /// 커밋에 적을 사람 — 사용자 전역 git 설정의 (user.name, user.email), 둘 다 있어야 Some. 원본: `APBoardGit.identity`
    fn identity(&self) -> Option<(String, String)> {
        let exe = find_git_exe()?;
        let name = git_global_get(&exe, "user.name")?;
        let email = git_global_get(&exe, "user.email")?;
        Some((name, email))
    }
}

/// PATH 를 차례로 보고, 없으면 Program Files\Git\cmd(`adapters::git::find_git` 의 짧은 사본)
fn find_git_exe() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var) {
            dirs.push(PathBuf::from(pf).join("Git").join("cmd"));
        }
    }
    dirs.into_iter().filter(|d| !d.as_os_str().is_empty()).map(|d| d.join(GIT_EXE)).find(|p| p.is_file())
}

/// `git config --global --get <키>` — 설정 글을 읽기만 한다(명령·hook 이 돌지 않는 git config --get). 값이 없으면 None
fn git_global_get(exe: &Path, key: &str) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.args(["config", "--global", "--get", key]);
    cmd.current_dir(claude_board_core::paths::home());
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    hide_window(&mut cmd);
    let out = cmd.output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(not(windows))]
fn hide_window(_cmd: &mut Command) {}

// ── 엔진 진입 ────────────────────────────────────────

/// 엔진 진입: `createRepo` 의 `apCreateRepo(plan, APFolderFiles(), APBoardGit())` 의 Windows 길.
/// git 은 보호 환경을 늘 거는 `ports::Git` 을 건다
pub fn create_repo_at(plan: &NewRepoPlan, git: &dyn Git) -> CreateResult {
    create_repo(plan, &FolderFiles, &BoardGit { git })
}
