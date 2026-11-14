//! git 작업 트리 어댑터 — 따로 작업 트리에서 도는 자율주행 실행이 쓰는 만들기(`add`)·이어받기(`add_existing`)·지우기(`remove`)·목록(`list`).
//! 원본: `Sources/ClaudeAutopilot/main.swift` 의 `startWorktree`(`git worktree add -b <가지> <경로> <출발 가지>` 와 이미 만든 가지를 이어받는
//! `git worktree add <경로> <가지>`)·`rollbackBranches`(remove·prune)·`checkedOut`(list). `add_existing` 의 되돌리기는 원본과 같이 가지를 지우지 않는다.
//!
//! 약속:
//! - git 은 모두 `ports::Git`(보호 환경을 거는 `adapters::git::ProcessGit`)으로 `repo` 를 `-C` 로 줘서만 부른다. 설정 include 를 따로 열지 않는다.
//! - 경로는 쓰기 전에 한 가지 꼴(`canonical_path`: 구분자 `/`, 드라이브 글자 대문자, `\\?\` 접두 벗김)로 맞추고, git·윈도가 다르게 읽는 꼴은 거부한다:
//!   `..`·성분 끝의 점/공백·예약 장치 이름(`con`, `nul`, `com1`, `con.txt` 포함)·`<>:"|?*`·제어 문자·`~숫자`(8.3 짧은 이름)·`.git` 성분·`\\.\` 장치 이름 공간·
//!   절대 경로가 아닌 꼴·드라이브 뿌리 자체·`MAX_PATH_LEN` 글자 넘는 경로.
//! - `add` 는 저장소의 공통 git 폴더(`.git`)와 그 안, 비지 않은 폴더·파일·바로 가기, 이미 있는 가지, 없는 출발 가지를 거부한다.
//!   만든 뒤 새 작업 트리의 `.git` 파일이 `<공통 git 폴더>/worktrees/<이름>` 을 가리키고 `commondir`·`gitdir` 되짚기가 맞는지 확인하며, 어긋나면 되돌리고 거부한다.
//! - `remove` 는 `git worktree list` 에 등록된 경로만 지운다(그 목록의 글자를 git 에 넘긴다). 원래 작업 트리·bare·바로 가기·잠긴 것(force 없이)은 거부한다.
//!   폴더가 이미 없으면 `worktree prune` 으로 등록만 치운다. 만든 가지는 지우지 않는다(합치기·버리기는 호출자 몫).
//! - `list` 는 `git worktree list --porcelain -z`(NUL 구분, 공백·한글 경로가 안전)를 읽는다. git 2.36 미만은 `-z` 가 없어 실패로 돌려준다.

use crate::ports::{Git, GitOutput, PortError, PortErrorKind, PortResult};
use crate::rules::prep_git::lexical;
use std::path::Path;

/// 경로 전체 글자 수 상한(안에 파일이 더 깊이 들어가도 윈도 MAX_PATH 아래에 머물게 하는 보수적 값)
pub const MAX_PATH_LEN: usize = 200;
const MAX_COMPONENT_LEN: usize = 255;

const RESERVED: [&str; 26] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM0", "LPT0",
];

/// 만든 작업 트리
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// 한 가지 꼴로 맞춘 작업 트리 경로(`C:/x/y`)
    pub path: String,
    /// 그 작업 트리의 git 폴더(`<공통>/worktrees/<이름>`)
    pub git_dir: String,
    /// 공통 git 폴더
    pub common_dir: String,
}

/// `remove` 의 결과
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removed {
    /// `git worktree remove` 로 폴더와 등록을 지웠다
    Removed,
    /// 폴더가 이미 없어 `git worktree prune` 으로 등록만 치웠다
    Pruned,
}

/// `git worktree list --porcelain` 한 항목
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// git 이 적은 그대로의 경로(git 에 되넘길 때 쓴다)
    pub path: String,
    pub head: Option<String>,
    /// `refs/heads/` 를 뗀 가지 이름(다른 ref 면 전체)
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    /// 잠겼으면 Some(까닭). 까닭이 없으면 Some("")
    pub locked: Option<String>,
    /// 지울 수 있으면(폴더 없음 등) Some(까닭)
    pub prunable: Option<String>,
}

fn invalid(msg: impl Into<String>) -> PortError {
    PortError::new(PortErrorKind::Invalid, msg)
}

fn denied(msg: impl Into<String>) -> PortError {
    PortError::new(PortErrorKind::Denied, msg)
}

fn first_line(s: &str) -> &str {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

// ── 경로 ─────────────────────────────────────────────

fn is_drive_component(c: &str) -> bool {
    let b = c.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// 검사 없이 한 가지 꼴로만 맞춘다(`lexical` + 드라이브 글자 대문자). 이미 있는 저장소·git 폴더 경로에 쓴다
pub fn canonical_loose(p: &str) -> String {
    // `lexical` 은 `\` 로 시작하는 꼴만 윈도 경로로 본다 — `//?/…`·`//서버/…` 도 같은 꼴로 읽히게 구분자를 맞춘다
    let c = if p.starts_with("//") { lexical(&p.replace('/', "\\")) } else { lexical(p) };
    if is_drive_component(c.get(..2).unwrap_or("")) {
        let mut s = c[..1].to_ascii_uppercase();
        s.push_str(&c[1..]);
        s
    } else {
        c
    }
}

fn validate_component(c: &str) -> PortResult<()> {
    if c == ".." {
        return Err(invalid("경로에 .. 가 있다"));
    }
    if c.ends_with('.') || c.ends_with(' ') {
        return Err(invalid(format!("경로 성분이 점이나 공백으로 끝난다(윈도가 떼어 낸다): {c:?}")));
    }
    if let Some(bad) = c.chars().find(|ch| "<>:\"|?*".contains(*ch)) {
        return Err(invalid(format!("경로 성분에 쓸 수 없는 글자 {bad:?}: {c:?}")));
    }
    if c.chars().count() > MAX_COMPONENT_LEN {
        return Err(invalid("경로 성분이 너무 길다"));
    }
    let stem = c.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(invalid(format!("윈도 예약 장치 이름이다: {c:?}")));
    }
    if c.as_bytes().windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit()) {
        return Err(invalid(format!("8.3 짧은 이름 꼴(~숫자)이라 같은 자리를 다르게 쓸 수 있다: {c:?}")));
    }
    if c.eq_ignore_ascii_case(".git") {
        return Err(denied("경로 성분이 .git 이다"));
    }
    Ok(())
}

/// 작업 트리 자리로 쓸 경로를 검사하고 한 가지 꼴(`C:/x/y`, UNC 는 `//서버/공유/x`)로 돌려준다.
/// `C:\x`·`c:/x`·`\\?\C:\x` 는 모두 `C:/x`.
pub fn canonical_path(p: &str) -> PortResult<String> {
    if p.is_empty() || p.chars().any(char::is_control) {
        return Err(invalid("경로가 비었거나 제어 문자가 있다"));
    }
    let norm = p.replace('\\', "/");
    if norm.starts_with("//./") || norm == "//." {
        return Err(invalid("\\\\.\\ 장치 이름 공간 경로는 쓰지 않는다"));
    }
    let body = norm.strip_prefix("//?/").unwrap_or(&norm);
    for (i, c) in body.split('/').enumerate() {
        if c.is_empty() || c == "." || (i == 0 && is_drive_component(c)) {
            continue;
        }
        validate_component(c)?;
    }
    let canon = canonical_loose(p);
    let drive = is_drive_component(canon.get(..2).unwrap_or("")) && canon.len() > 3 && canon.as_bytes()[2] == b'/';
    let unc = canon.strip_prefix("//").is_some_and(|r| r.split('/').filter(|s| !s.is_empty()).count() >= 3);
    let posix = !cfg!(windows) && canon.starts_with('/') && canon.len() > 1;
    if !(drive || unc || posix) {
        return Err(invalid(format!("드라이브가 있는 절대 경로(또는 UNC 공유 아래)가 아니다: {p:?}")));
    }
    if canon.chars().count() > MAX_PATH_LEN {
        return Err(invalid(format!("경로가 {MAX_PATH_LEN}자를 넘는다")));
    }
    Ok(canon)
}

fn fold(s: &str) -> String {
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

fn same_path(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// `p` 가 `dir` 자신이거나 그 아래인가(둘 다 `canonical_loose` 꼴)
fn inside(p: &str, dir: &str) -> bool {
    let (p, d) = (fold(p), fold(dir));
    p == d || p.starts_with(&format!("{}/", d.trim_end_matches('/')))
}

fn is_absolute(p: &str) -> bool {
    p.starts_with('/') || p.starts_with('\\') || is_drive_component(p.get(..2).unwrap_or(""))
}

#[cfg(windows)]
fn reparse(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
}

#[cfg(not(windows))]
fn reparse(_m: &std::fs::Metadata) -> bool {
    false
}

fn is_link(m: &std::fs::Metadata) -> bool {
    m.file_type().is_symlink() || reparse(m)
}

// ── git 질문 ─────────────────────────────────────────

fn run(git: &dyn Git, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
    git.run(repo, args)
}

/// 저장소의 공통 git 폴더(작업 트리여도 원래 `.git`). 한 가지 꼴
fn common_git_dir(git: &dyn Git, repo: &Path) -> PortResult<String> {
    let out = run(git, repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let line = first_line(&out.stdout);
    if out.code != 0 || line.is_empty() {
        return Err(invalid(format!("git 저장소가 아니다: {}", first_line(&out.stderr))));
    }
    Ok(canonical_loose(line))
}

/// 가지 이름 자체의 규칙(빈 이름·대시로 시작·제어·공백 글자·`git check-ref-format --branch`)
fn check_branch_name(git: &dyn Git, repo: &Path, branch: &str) -> PortResult<()> {
    if branch.is_empty() || branch.starts_with('-') || branch.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(invalid(format!("가지 이름이 맞지 않다: {branch:?}")));
    }
    let fmt = run(git, repo, &["check-ref-format", "--branch", branch])?;
    if fmt.code != 0 {
        return Err(invalid(format!("가지 이름이 git 규칙에 맞지 않다: {branch:?}")));
    }
    Ok(())
}

fn check_branch(git: &dyn Git, repo: &Path, branch: &str) -> PortResult<()> {
    check_branch_name(git, repo, branch)?;
    let exists = run(git, repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])?;
    if exists.code == 0 {
        return Err(denied(format!("가지가 이미 있다: {branch}")));
    }
    Ok(())
}

fn check_base(git: &dyn Git, repo: &Path, base: &str) -> PortResult<()> {
    if base.is_empty() || base.starts_with('-') || base.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(invalid(format!("출발 가지 이름이 맞지 않다: {base:?}")));
    }
    let ok = run(git, repo, &["rev-parse", "--verify", "--quiet", &format!("{base}^{{commit}}")])?;
    if ok.code != 0 {
        return Err(PortError::new(PortErrorKind::NotFound, format!("출발 가지(커밋)를 찾을 수 없다: {base}")));
    }
    Ok(())
}

/// 자리가 없거나 빈 폴더여야 한다. 파일·바로 가기(정션 포함)·안이 있는 폴더는 거부
fn check_target_folder(canon: &str) -> PortResult<()> {
    match std::fs::symlink_metadata(canon) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(m) => {
            if is_link(&m) {
                return Err(denied(format!("그 자리가 바로 가기다: {canon}")));
            }
            if !m.is_dir() {
                return Err(denied(format!("그 자리에 파일이 있다: {canon}")));
            }
            let mut rd = std::fs::read_dir(canon)?;
            if rd.next().is_some() {
                return Err(denied(format!("그 폴더가 비어 있지 않다: {canon}")));
            }
            Ok(())
        }
    }
}

// ── add ──────────────────────────────────────────────

/// `git worktree add -b <branch> <path> <base>`. 경로는 `canonical_path` 꼴로 맞춰 쓴다.
pub fn add(git: &dyn Git, repo: &Path, path: &str, branch: &str, base: &str) -> PortResult<Added> {
    let canon = canonical_path(path)?;
    let common = common_git_dir(git, repo)?;
    let repo_git = format!("{}/.git", canonical_loose(&repo.to_string_lossy()).trim_end_matches('/'));
    if inside(&canon, &common) || inside(&canon, &repo_git) {
        return Err(denied(format!("작업 트리 자리가 저장소 .git 안이다: {canon}")));
    }
    check_branch(git, repo, branch)?;
    check_base(git, repo, base)?;
    check_target_folder(&canon)?;

    let out = run(git, repo, &["worktree", "add", "-b", branch, &canon, base])?;
    if out.code != 0 {
        return Err(PortError::new(PortErrorKind::Io, format!("git worktree add 실패: {}", first_line(&out.stderr))));
    }
    match verify_new(&canon, &common) {
        Ok(git_dir) => Ok(Added { path: canon, git_dir, common_dir: common }),
        Err(why) => {
            // 되돌린다: 방금 만든 것(등록·폴더·가지)만
            let _ = run(git, repo, &["worktree", "remove", "--force", &canon]);
            let _ = run(git, repo, &["worktree", "prune"]);
            let _ = run(git, repo, &["branch", "-D", branch]);
            Err(why)
        }
    }
}

/// `git worktree add <path> <branch>` — 출발 전에 보드가 이미 만든 가지를 작업 트리로 이어받는다.
/// 검사는 `add` 와 같은 길을 쓰되 가지가 있어야 하고(없으면 NotFound) 출발 가지를 묻지 않는다.
/// 확인에 어긋나면 되돌리는데, 만든 가지가 아니라 이어받은 가지라 `branch -D` 는 하지 않는다(가지는 그대로 둔다).
pub fn add_existing(git: &dyn Git, repo: &Path, path: &str, branch: &str) -> PortResult<Added> {
    let canon = canonical_path(path)?;
    let common = common_git_dir(git, repo)?;
    let repo_git = format!("{}/.git", canonical_loose(&repo.to_string_lossy()).trim_end_matches('/'));
    if inside(&canon, &common) || inside(&canon, &repo_git) {
        return Err(denied(format!("작업 트리 자리가 저장소 .git 안이다: {canon}")));
    }
    check_branch_name(git, repo, branch)?;
    let exists = run(git, repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])?;
    if exists.code != 0 {
        return Err(PortError::new(PortErrorKind::NotFound, format!("이어받을 가지가 없다: {branch}")));
    }
    check_target_folder(&canon)?;

    let out = run(git, repo, &["worktree", "add", &canon, branch])?;
    if out.code != 0 {
        return Err(PortError::new(PortErrorKind::Io, format!("git worktree add 실패: {}", first_line(&out.stderr))));
    }
    match verify_new(&canon, &common) {
        Ok(git_dir) => Ok(Added { path: canon, git_dir, common_dir: common }),
        Err(why) => {
            // 되돌린다: 방금 만든 것(등록·폴더)만 — 가지는 이어받은 것
            let _ = run(git, repo, &["worktree", "remove", "--force", &canon]);
            let _ = run(git, repo, &["worktree", "prune"]);
            Err(why)
        }
    }
}

fn read_trim(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn resolve(base_dir: &str, value: &str) -> String {
    if is_absolute(value) {
        canonical_loose(value)
    } else {
        canonical_loose(&format!("{base_dir}/{value}"))
    }
}

/// 새 작업 트리의 `.git` 파일·`commondir`·`gitdir` 되짚기가 `<공통>/worktrees/<이름>` 으로 이어지는지 본다. 맞으면 그 git 폴더
fn verify_new(canon: &str, common: &str) -> PortResult<String> {
    let bad = |m: &str| denied(format!("만든 작업 트리의 git 자리가 맞지 않아 되돌렸다: {m}"));
    let dot_git = read_trim(&format!("{canon}/.git")).ok_or_else(|| bad(".git 파일을 읽지 못함"))?;
    let g = dot_git.strip_prefix("gitdir:").ok_or_else(|| bad(".git 이 gitdir 파일이 아님"))?.trim();
    if g.is_empty() {
        return Err(bad(".git 파일이 비었음"));
    }
    let git_dir = resolve(canon, g);
    let prefix = format!("{}/worktrees/", fold(common).trim_end_matches('/'));
    let tail = fold(&git_dir);
    let Some(name) = tail.strip_prefix(&prefix) else { return Err(bad("git 폴더가 공통 .git/worktrees 아래가 아님")) };
    if name.is_empty() || name.contains('/') {
        return Err(bad("git 폴더가 .git/worktrees/<이름> 꼴이 아님"));
    }
    let cd = read_trim(&format!("{git_dir}/commondir")).ok_or_else(|| bad("commondir 를 읽지 못함"))?;
    if !same_path(&resolve(&git_dir, &cd), common) {
        return Err(bad("commondir 가 저장소 공통 .git 이 아님"));
    }
    let back = read_trim(&format!("{git_dir}/gitdir")).ok_or_else(|| bad("gitdir 되짚기를 읽지 못함"))?;
    if !same_path(&resolve(&git_dir, &back), &format!("{canon}/.git")) {
        return Err(bad("gitdir 되짚기가 이 작업 트리가 아님"));
    }
    Ok(git_dir)
}

// ── list ─────────────────────────────────────────────

/// `git worktree list --porcelain -z` 출력 → 항목들(원래 작업 트리가 먼저). NUL 이 필드 끝이고 빈 필드가 항목 끝이다.
pub fn parse_porcelain(out: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut cur: Option<Entry> = None;
    for field in out.split('\0') {
        if field.is_empty() {
            entries.extend(cur.take());
            continue;
        }
        if let Some(p) = field.strip_prefix("worktree ") {
            entries.extend(cur.take());
            cur = Some(Entry { path: p.to_string(), ..Entry::default() });
            continue;
        }
        let Some(e) = cur.as_mut() else { continue };
        if let Some(h) = field.strip_prefix("HEAD ") {
            e.head = Some(h.to_string());
        } else if let Some(b) = field.strip_prefix("branch ") {
            e.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        } else if field == "detached" {
            e.detached = true;
        } else if field == "bare" {
            e.bare = true;
        } else if field == "locked" {
            e.locked = Some(String::new());
        } else if let Some(r) = field.strip_prefix("locked ") {
            e.locked = Some(r.to_string());
        } else if field == "prunable" {
            e.prunable = Some(String::new());
        } else if let Some(r) = field.strip_prefix("prunable ") {
            e.prunable = Some(r.to_string());
        }
    }
    entries.extend(cur);
    entries
}

pub fn list(git: &dyn Git, repo: &Path) -> PortResult<Vec<Entry>> {
    let out = run(git, repo, &["worktree", "list", "--porcelain", "-z"])?;
    if out.code != 0 {
        return Err(PortError::new(PortErrorKind::Io, format!("git worktree list 실패: {}", first_line(&out.stderr))));
    }
    Ok(parse_porcelain(&out.stdout))
}

// ── remove ───────────────────────────────────────────

/// 등록된 작업 트리 하나를 지운다. `force` 는 바뀐 파일이 남았거나 잠긴 것도 지운다.
/// 등록 목록에 없는 경로·원래 작업 트리·bare·바로 가기는 `force` 여도 거부한다.
pub fn remove(git: &dyn Git, repo: &Path, path: &str, force: bool) -> PortResult<Removed> {
    let canon = canonical_path(path)?;
    let entries = list(git, repo)?;
    let idx = entries
        .iter()
        .position(|e| same_path(&canonical_loose(&e.path), &canon))
        .ok_or_else(|| PortError::new(PortErrorKind::NotFound, format!("등록된 작업 트리가 아니다: {canon}")))?;
    let e = &entries[idx];
    if idx == 0 || e.bare {
        return Err(denied(format!("원래 작업 트리(또는 bare 저장소)는 지우지 않는다: {}", e.path)));
    }
    if e.locked.is_some() && !force {
        return Err(denied(format!("잠긴 작업 트리다({}): {}", e.locked.as_deref().unwrap_or(""), e.path)));
    }
    let meta = std::fs::symlink_metadata(&e.path).ok();
    if meta.as_ref().is_some_and(is_link) {
        return Err(denied(format!("작업 트리 자리가 바로 가기라 지우지 않는다: {}", e.path)));
    }
    if meta.is_none() {
        // 폴더가 사람 손으로 지워졌다 — 등록만 치운다. 잠긴 것은 force 일 때만 여기 온다
        if e.locked.is_some() {
            let un = run(git, repo, &["worktree", "unlock", &e.path])?;
            if un.code != 0 {
                return Err(denied(format!("잠금을 풀지 못했다: {}", first_line(&un.stderr))));
            }
        }
        let pr = run(git, repo, &["worktree", "prune"])?;
        if pr.code != 0 {
            return Err(PortError::new(PortErrorKind::Io, format!("git worktree prune 실패: {}", first_line(&pr.stderr))));
        }
        if still_listed(git, repo, &canon)? {
            return Err(PortError::new(PortErrorKind::Io, format!("prune 뒤에도 등록이 남았다: {}", e.path)));
        }
        return Ok(Removed::Pruned);
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
        if e.locked.is_some() {
            args.push("--force"); // 잠긴 것은 --force 를 두 번 줘야 지운다
        }
    }
    args.push(&e.path);
    let out = run(git, repo, &args)?;
    if out.code != 0 {
        return Err(denied(format!("git worktree remove 거부: {}", first_line(&out.stderr))));
    }
    if still_listed(git, repo, &canon)? {
        return Err(PortError::new(PortErrorKind::Io, format!("remove 뒤에도 등록이 남았다: {}", e.path)));
    }
    Ok(Removed::Removed)
}

fn still_listed(git: &dyn Git, repo: &Path, canon: &str) -> PortResult<bool> {
    Ok(list(git, repo)?.iter().any(|e| same_path(&canonical_loose(&e.path), canon)))
}
