//! 남은 변경 읽기·보관 시험(`engine::leftovers`). `real-git` 표시가 붙은 것은 이 PC 의 진짜 git.exe 와 새 임시 폴더로 돈다.
//! 사용자의 ~/.gitconfig·실제 저장소·네트워크는 건드리지 않는다(시험 준비용 git 은 시스템·전역 설정을 끄고 로컬 경로만 쓴다).
//! 나머지는 가짜 git(`Fake`)으로 모든 판정과 오류 길을 돈다.

use claude_board_autopilot::engine::leftovers::{
    check_leftovers, new_lines, parse_status_z, record_leftovers, stash_failed_event, stash_leftovers, stashed_event,
    status_lines, LeftoverError, StashRequest, Stashed, DEFAULT_IDENTITY, RECORD_PATHS_MAX,
};
use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::ports::{Git, GitOutput, PortError, PortErrorKind, PortResult};
use claude_board_autopilot::rules::leftovers::{stash_message, Leftover, LeftoverWhy};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

// ── 진짜 git 준비 도우미(시험 자신의 git 호출) ─────────

#[cfg(windows)]
const NULL: &str = "NUL";
#[cfg(not(windows))]
const NULL: &str = "/dev/null";

fn sh_raw(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "-c", "core.longpaths=true"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", NULL)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git 가 있어야 한다")
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = sh_raw(dir, args);
    assert!(out.status.success(), "git {args:?} 실패: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    sh(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", "base"]);
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let canon = std::fs::canonicalize(tmp.path()).unwrap();
        let root = PathBuf::from(canon.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        Env { _tmp: tmp, root }
    }
    fn repo(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        init_repo(&d);
        d
    }
}

/// 작업 트리의 모든 파일 내용(.git 은 뺀다)
fn tree(repo: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if e.file_name() == ".git" {
                continue;
            }
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(repo, repo, &mut m);
    m
}

fn stash_count(repo: &Path) -> usize {
    sh(repo, &["stash", "list"]).lines().count()
}

fn git() -> ProcessGit {
    ProcessGit::new()
}

fn req<'a>(new: &'a [String], paths: Option<&'a [String]>) -> StashRequest<'a> {
    StashRequest { run: "r1", unit: Some(2), repo_name: "repo", branch: "story/x", paths, new_lines: new }
}

/// 읽고 → 판정이 `Stash(None)` 인지 확인하고 → 보관
fn stash_all(repo: &Path) -> Result<Stashed, LeftoverError> {
    let c = check_leftovers(&git(), repo, &[], true, true).unwrap();
    assert_eq!(c.verdict, Leftover::Stash(None), "{:?}", c.dirty);
    let nl = new_lines(&c.dirty, &[]);
    stash_leftovers(&git(), repo, &req(&nl, None))
}

fn v(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| (*s).to_string()).collect()
}

// ── 진짜 git ─────────────────────────────────────────

#[test]
fn real_git_clean_repo_has_nothing_to_stash() {
    let e = Env::new();
    let r = e.repo("clean");
    let c = check_leftovers(&git(), &r, &[], true, true).unwrap();
    assert!(c.dirty.is_empty());
    assert_eq!(c.verdict, Leftover::Clean);
    assert_eq!(stash_leftovers(&git(), &r, &req(&[], None)), Err(LeftoverError::NothingStashed));
    assert_eq!(stash_count(&r), 0);
}

#[test]
fn real_git_modified_tracked_file_is_stashed_and_restorable() {
    let e = Env::new();
    let r = e.repo("mod");
    std::fs::write(r.join("a.txt"), "changed\nbytes\n").unwrap();
    let before = tree(&r);
    let c = check_leftovers(&git(), &r, &[], true, true).unwrap();
    assert_eq!(c.dirty, v([" M a.txt"].as_slice()));
    let s = stash_all(&r).unwrap();
    assert_eq!(s.files, 1);
    assert_eq!(s.paths, v(&[" M a.txt"]));
    assert_eq!(s.message, stash_message("r1", Some(2), "repo", "story/x"));
    assert_eq!(std::fs::read(r.join("a.txt")).unwrap(), b"a");
    assert_eq!(status_lines(&git(), &r).unwrap(), Vec::<String>::new());
    assert_eq!(sh(&r, &["rev-parse", "refs/stash"]).trim(), s.sha);
    assert!(sh(&r, &["stash", "list"]).contains(&s.message));
    sh(&r, &["stash", "pop", "-q"]);
    assert_eq!(tree(&r), before);
}

#[test]
fn real_git_untracked_file_is_stashed_with_include_untracked() {
    let e = Env::new();
    let r = e.repo("untracked");
    std::fs::create_dir_all(r.join("sub")).unwrap();
    std::fs::write(r.join("sub").join("n.txt"), "new").unwrap();
    std::fs::write(r.join("u.txt"), "u").unwrap();
    let before = tree(&r);
    let c = check_leftovers(&git(), &r, &[], true, true).unwrap();
    assert_eq!(c.dirty, v(&["?? sub/", "?? u.txt"]));
    stash_all(&r).unwrap();
    assert!(!r.join("u.txt").exists() && !r.join("sub").join("n.txt").exists());
    sh(&r, &["stash", "pop", "-q"]);
    assert_eq!(tree(&r), before);
}

#[test]
fn real_git_ignored_file_is_neither_listed_nor_stashed() {
    let e = Env::new();
    let r = e.repo("ignored");
    std::fs::write(r.join(".gitignore"), "*.log\n").unwrap();
    sh(&r, &["add", ".gitignore"]);
    sh(&r, &["commit", "-q", "-m", "ignore"]);
    std::fs::write(r.join("keep.log"), "log").unwrap();
    assert!(status_lines(&git(), &r).unwrap().is_empty());
    std::fs::write(r.join("x.txt"), "x").unwrap();
    assert_eq!(status_lines(&git(), &r).unwrap(), v(&["?? x.txt"]));
    stash_all(&r).unwrap();
    assert_eq!(std::fs::read(r.join("keep.log")).unwrap(), b"log");
    assert!(!r.join("x.txt").exists());
}

#[test]
fn real_git_renamed_file_reads_as_old_arrow_new_and_restores_with_index() {
    let e = Env::new();
    let r = e.repo("rename");
    sh(&r, &["mv", "a.txt", "renamed.txt"]);
    let c = check_leftovers(&git(), &r, &[], true, true).unwrap();
    assert_eq!(c.dirty, v(&["R  a.txt -> renamed.txt"]));
    let before = tree(&r);
    stash_all(&r).unwrap();
    assert!(r.join("a.txt").exists() && !r.join("renamed.txt").exists());
    sh(&r, &["stash", "pop", "-q", "--index"]);
    assert_eq!(tree(&r), before);
    assert_eq!(status_lines(&git(), &r).unwrap(), v(&["R  a.txt -> renamed.txt"]));
}

#[test]
fn real_git_renamed_with_start_dirty_cannot_be_split() {
    let e = Env::new();
    let r = e.repo("rename-split");
    std::fs::write(r.join("b.txt"), "human").unwrap();
    let start = status_lines(&git(), &r).unwrap();
    assert_eq!(start, v(&[" M b.txt"]));
    sh(&r, &["mv", "a.txt", "z.txt"]);
    let c = check_leftovers(&git(), &r, &start, true, true).unwrap();
    assert_eq!(c.verdict, Leftover::Refuse(LeftoverWhy::ReadOnly));
}

#[test]
fn real_git_deleted_file_is_stashed_and_restored() {
    let e = Env::new();
    let r = e.repo("deleted");
    std::fs::remove_file(r.join("a.txt")).unwrap();
    let c = check_leftovers(&git(), &r, &[], true, true).unwrap();
    assert_eq!(c.dirty, v(&[" D a.txt"]));
    stash_all(&r).unwrap();
    assert_eq!(std::fs::read(r.join("a.txt")).unwrap(), b"a");
    sh(&r, &["stash", "pop", "-q"]);
    assert!(!r.join("a.txt").exists());
}

#[test]
fn real_git_staged_and_unstaged_same_file_restores_both_with_index() {
    let e = Env::new();
    let r = e.repo("mm");
    std::fs::write(r.join("a.txt"), "staged").unwrap();
    sh(&r, &["add", "a.txt"]);
    std::fs::write(r.join("a.txt"), "unstaged").unwrap();
    assert_eq!(status_lines(&git(), &r).unwrap(), v(&["MM a.txt"]));
    stash_all(&r).unwrap();
    assert_eq!(std::fs::read(r.join("a.txt")).unwrap(), b"a");
    sh(&r, &["stash", "pop", "-q", "--index"]);
    assert_eq!(std::fs::read(r.join("a.txt")).unwrap(), b"unstaged");
    assert_eq!(sh(&r, &["show", ":a.txt"]), "staged");
}

#[test]
fn real_git_special_paths_read_and_stash() {
    let e = Env::new();
    let r = e.repo("special");
    let names = ["with space.txt", "한글 파일.txt", "br[ack]et.txt", "한글.txt", "tab\tname.txt"];
    for n in names {
        if std::fs::write(r.join(n), n).is_err() {
            continue; // 이 OS 가 못 만드는 이름(Windows 의 탭 등)
        }
    }
    let made: Vec<&str> = names.iter().copied().filter(|n| r.join(n).exists()).collect();
    assert!(made.len() >= 4, "{made:?}");
    let before = tree(&r);
    let lines = status_lines(&git(), &r).unwrap();
    assert_eq!(lines.len(), made.len(), "{lines:?}");
    // 규칙이 받는 줄은 진짜 `git status --porcelain`(-z 아님)과 같다(한글·공백은 따옴표로 싼다)
    let mut want: Vec<String> =
        sh(&r, &["status", "--porcelain", "--ignore-submodules=all"]).lines().map(String::from).collect();
    let mut got = lines.clone();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    let korean = [r"\355\225\234\352\270\200", " ", r"\355\214\214\354\235\274", ".txt"].concat();
    assert!(lines.contains(&format!("?? \"{korean}\"")), "{lines:?}");
    stash_all(&r).unwrap();
    assert!(status_lines(&git(), &r).unwrap().is_empty());
    sh(&r, &["stash", "pop", "-q"]);
    assert_eq!(tree(&r), before);
}

#[cfg(not(windows))]
#[test]
fn real_git_newline_and_quote_paths_match_porcelain() {
    let e = Env::new();
    let r = e.repo("nl");
    std::fs::write(r.join("a\nb.txt"), "x").unwrap();
    std::fs::write(r.join("q\"uote.txt"), "x").unwrap();
    std::fs::write(r.join("back\\slash.txt"), "x").unwrap();
    let mut want: Vec<String> = sh(&r, &["status", "--porcelain"]).lines().map(String::from).collect();
    let mut got = status_lines(&git(), &r).unwrap();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}

#[test]
fn real_git_crlf_repo_warnings_are_not_failures() {
    let e = Env::new();
    let r = e.root.join("crlf");
    std::fs::create_dir_all(&r).unwrap();
    sh(&r, &["init", "-q", "-b", "main"]);
    sh(&r, &["config", "core.autocrlf", "true"]);
    std::fs::write(r.join("t.txt"), "one\ntwo\n").unwrap();
    sh(&r, &["add", "."]);
    sh(&r, &["commit", "-q", "-m", "base"]);
    std::fs::write(r.join("t.txt"), "one\r\ntwo\r\nthree\r\n").unwrap();
    // CRLF 저장소에서 추적 안 하는 파일은 보관할 때 LF 로 바뀌고 꺼낼 때 CRLF 로 돌아온다(git 의 autocrlf 왕복) — CRLF 로 써 두어야 바이트가 같다
    std::fs::write(r.join("new.txt"), "x\r\ny\r\n").unwrap();
    let before = tree(&r);
    let c = check_leftovers(&git(), &r, &[], true, true).unwrap();
    assert_eq!(c.dirty, v(&[" M t.txt", "?? new.txt"]));
    let s = stash_all(&r).unwrap();
    assert_eq!(s.files, 2);
    assert!(status_lines(&git(), &r).unwrap().is_empty());
    sh(&r, &["stash", "pop", "-q"]);
    assert_eq!(tree(&r), before);
}

#[test]
fn real_git_path_spellings_all_work() {
    let e = Env::new();
    let r = e.repo("spell");
    std::fs::write(r.join("x.txt"), "x").unwrap();
    let fwd = PathBuf::from(r.to_string_lossy().replace('\\', "/"));
    assert_eq!(status_lines(&git(), &fwd).unwrap(), v(&["?? x.txt"]));
    let back = PathBuf::from(r.to_string_lossy().replace('/', "\\"));
    assert_eq!(status_lines(&git(), &back).unwrap(), v(&["?? x.txt"]));
    #[cfg(windows)]
    {
        let verbatim = PathBuf::from(format!(r"\\?\{}", back.to_string_lossy()));
        // 확장 길이 접두는 git 이 못 받을 수 있다 — 받으면 같은 답, 못 받으면 패닉 없이 오류
        match status_lines(&git(), &verbatim) {
            Ok(l) => assert_eq!(l, v(&["?? x.txt"])),
            Err(e) => assert!(matches!(e, LeftoverError::Git { .. } | LeftoverError::Port(_)), "{e:?}"),
        }
        // 소문자 드라이브 글자
        let s = back.to_string_lossy().to_string();
        let lower = format!("{}{}", s[..1].to_lowercase(), &s[1..]);
        assert_eq!(status_lines(&git(), Path::new(&lower)).unwrap(), v(&["?? x.txt"]));
    }
    stash_leftovers(&git(), &fwd, &req(&v(&["?? x.txt"]), None)).unwrap();
    assert!(!r.join("x.txt").exists());
}

#[test]
fn real_git_long_path_is_stashed_or_fails_loudly_without_losing_it() {
    let e = Env::new();
    let r = e.repo("long");
    let mut rel = PathBuf::new();
    for _ in 0..6 {
        rel.push("d".repeat(50));
    }
    let dir = r.join(&rel);
    #[cfg(windows)]
    let mk = PathBuf::from(format!(r"\\?\{}", dir.to_string_lossy().replace('/', "\\")));
    #[cfg(not(windows))]
    let mk = dir.clone();
    std::fs::create_dir_all(&mk).unwrap();
    let file = mk.join("deep.txt");
    std::fs::write(&file, "deep").unwrap();
    assert!(dir.to_string_lossy().len() > 280);
    let st = status_lines(&git(), &r);
    let lines = match st {
        Ok(l) => l,
        Err(err) => {
            assert!(std::fs::read(&file).unwrap() == b"deep", "{err:?}");
            return;
        }
    };
    assert_eq!(lines.len(), 1, "{lines:?}");
    match stash_leftovers(&git(), &r, &req(&lines, None)) {
        Ok(_) => {
            sh(&r, &["stash", "pop", "-q"]);
            assert_eq!(std::fs::read(&file).unwrap(), b"deep");
        }
        Err(_) => assert_eq!(std::fs::read(&file).unwrap(), b"deep", "실패해도 파일은 그대로"),
    }
}

#[cfg(windows)]
fn lock(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(path).unwrap()
}

#[cfg(windows)]
#[test]
fn real_git_locked_modified_file_makes_stash_fail_and_keeps_every_byte() {
    let e = Env::new();
    let r = e.repo("locked");
    std::fs::write(r.join("a.txt"), "edited").unwrap();
    std::fs::write(r.join("free.txt"), "free").unwrap();
    let held = lock(&r.join("a.txt"));
    let before_stashes = stash_count(&r);
    let res = {
        let c = check_leftovers(&git(), &r, &[], true, true);
        match c {
            // 잠긴 파일을 status 가 읽지 못하면 읽기 단계에서 오류 — 이것도 보관하지 않는 쪽의 안전한 답
            Err(err) => {
                assert!(matches!(err, LeftoverError::Git { .. }), "{err:?}");
                Err(err)
            }
            Ok(c) => stash_leftovers(&git(), &r, &req(&new_lines(&c.dirty, &[]), None)),
        }
    };
    eprintln!("locked modified -> {res:?}");
    let err = res.expect_err("잠긴 파일이 있으면 보관은 성공하지 않는다");
    drop(held);
    // 변경은 어디든 남아 있다: 작업 트리에 그대로이거나 보관본에 있다
    let tree_now = tree(&r);
    let created = match &err {
        LeftoverError::StashFailed { stash_created, .. } => stash_created.clone(),
        _ => None,
    };
    if created.is_none() {
        assert_eq!(tree_now.get("a.txt").map(Vec::as_slice), Some(b"edited".as_slice()), "{err:?}");
        assert_eq!(tree_now.get("free.txt").map(Vec::as_slice), Some(b"free".as_slice()), "{err:?}");
        assert_eq!(stash_count(&r), before_stashes);
    } else {
        // 보관본이 만들어진 뒤 실패: 보관본에 변경이 모두 있다
        assert_eq!(sh(&r, &["show", "stash@{0}:a.txt"]), "edited");
    }
    assert!(!err.to_string().is_empty());
}

#[cfg(windows)]
#[test]
fn real_git_locked_untracked_file_never_vanishes_silently() {
    let e = Env::new();
    let r = e.repo("locked-untracked");
    std::fs::write(r.join("u.txt"), "untracked").unwrap();
    let held = lock(&r.join("u.txt"));
    let c = check_leftovers(&git(), &r, &[], true, true);
    let res = match c {
        Err(err) => Err(err),
        Ok(c) => stash_leftovers(&git(), &r, &req(&new_lines(&c.dirty, &[]), None)),
    };
    drop(held);
    eprintln!("locked untracked -> {res:?}");
    match res {
        Err(LeftoverError::StashFailed { stash_created: Some(_), .. }) => {
            // 보관본은 있고 잠긴 파일은 작업 트리에 남았다
            assert_eq!(std::fs::read(r.join("u.txt")).unwrap_or_default(), b"untracked");
        }
        Err(_) => assert_eq!(std::fs::read(r.join("u.txt")).unwrap(), b"untracked"),
        Ok(s) => {
            // git 이 잠긴 파일을 보관할 수 있었다면 되돌려 받을 수 있어야 한다
            assert!(!r.join("u.txt").exists());
            sh(&r, &["stash", "pop", "-q"]);
            assert_eq!(std::fs::read(r.join("u.txt")).unwrap(), b"untracked", "{s:?}");
        }
    }
}

fn add_submodule(repo: &Path, source: &Path, at: &str) {
    let url = source.to_string_lossy().replace('\\', "/");
    sh(repo, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &url, at]);
    sh(repo, &["commit", "-q", "-m", "sub"]);
}

#[test]
fn real_git_dirty_submodule_is_not_listed_and_never_stashed_into() {
    let e = Env::new();
    let src = e.repo("src");
    let r = e.repo("host");
    add_submodule(&r, &src, "libs/dep");
    // 하위 모듈 안의 변경(추적 파일 수정 + 안 쓰는 파일)
    std::fs::write(r.join("libs/dep/a.txt"), "dirty-in-sub").unwrap();
    std::fs::write(r.join("libs/dep/loose.txt"), "loose").unwrap();
    std::fs::write(r.join("top.txt"), "top").unwrap();
    let lines = status_lines(&git(), &r).unwrap();
    assert_eq!(lines, v(&["?? top.txt"]), "하위 모듈 안은 보지 않는다");
    let nl = new_lines(&lines, &[]);
    let s = stash_leftovers(&git(), &r, &req(&nl, None)).unwrap();
    assert_eq!(s.files, 1);
    assert!(!r.join("top.txt").exists());
    // 하위 모듈 안의 변경은 그대로
    assert_eq!(std::fs::read(r.join("libs/dep/a.txt")).unwrap(), b"dirty-in-sub");
    assert_eq!(std::fs::read(r.join("libs/dep/loose.txt")).unwrap(), b"loose");
    sh(&r, &["stash", "pop", "-q"]);
    assert_eq!(std::fs::read(r.join("top.txt")).unwrap(), b"top");
}

#[test]
fn real_git_start_dirty_human_change_stays_only_new_paths_are_stashed() {
    let e = Env::new();
    let r = e.repo("split");
    std::fs::write(r.join("a.txt"), "human").unwrap();
    let start = status_lines(&git(), &r).unwrap();
    std::fs::write(r.join("stray.txt"), "stray").unwrap();
    std::fs::write(r.join("b.txt"), "model").unwrap();
    let c = check_leftovers(&git(), &r, &start, true, true).unwrap();
    let paths = match &c.verdict {
        Leftover::Stash(Some(p)) => p.clone(),
        o => panic!("경로만 보관해야 한다: {o:?}"),
    };
    assert_eq!(paths, v(&["b.txt", "stray.txt"]));
    let nl = new_lines(&c.dirty, &start);
    let s = stash_leftovers(&git(), &r, &req(&nl, Some(&paths))).unwrap();
    assert_eq!(s.files, 2);
    assert_eq!(std::fs::read(r.join("a.txt")).unwrap(), b"human", "사람의 변경은 남는다");
    assert!(!r.join("stray.txt").exists());
    assert_eq!(std::fs::read(r.join("b.txt")).unwrap(), b"b");
    assert_eq!(status_lines(&git(), &r).unwrap(), start);
    sh(&r, &["stash", "pop", "-q"]);
    assert_eq!(std::fs::read(r.join("b.txt")).unwrap(), b"model");
    assert_eq!(std::fs::read(r.join("stray.txt")).unwrap(), b"stray");
}

#[test]
fn real_git_second_stash_gets_its_own_sha() {
    let e = Env::new();
    let r = e.repo("two");
    std::fs::write(r.join("one.txt"), "1").unwrap();
    let s1 = stash_all(&r).unwrap();
    std::fs::write(r.join("two.txt"), "2").unwrap();
    let s2 = stash_all(&r).unwrap();
    assert_ne!(s1.sha, s2.sha);
    assert_eq!(stash_count(&r), 2);
    assert_eq!(sh(&r, &["rev-parse", "refs/stash"]).trim(), s2.sha);
}

#[test]
fn real_git_not_a_repo_is_an_error_not_a_clean_answer() {
    let e = Env::new();
    let d = e.root.join("plain-dir");
    std::fs::create_dir_all(&d).unwrap();
    let err = status_lines(&git(), &d).unwrap_err();
    assert!(matches!(err, LeftoverError::Git { .. }), "{err:?}");
    let err = check_leftovers(&git(), &d, &[], true, true).unwrap_err();
    assert!(matches!(err, LeftoverError::Git { .. }));
}

#[test]
fn real_git_stash_works_without_any_configured_identity() {
    // 시험 준비 git 은 신원을 -c 로만 준다 → 저장소 설정에는 신원이 없다. 엔진은 기본 신원을 -c 로 건다
    let e = Env::new();
    let r = e.repo("noid");
    std::fs::write(r.join("x.txt"), "x").unwrap();
    let s = stash_all(&r).unwrap();
    let author = sh(&r, &["log", "-1", "--format=%an <%ae>", &s.sha]);
    assert_eq!(author.trim(), format!("{} <{}>", DEFAULT_IDENTITY.0, DEFAULT_IDENTITY.1));
}

#[test]
fn real_git_repo_configured_identity_is_used() {
    let e = Env::new();
    let r = e.repo("withid");
    sh(&r, &["config", "user.name", "Repo Owner"]);
    sh(&r, &["config", "user.email", "owner@example.com"]);
    std::fs::write(r.join("x.txt"), "x").unwrap();
    let s = stash_all(&r).unwrap();
    assert_eq!(sh(&r, &["log", "-1", "--format=%an <%ae>", &s.sha]).trim(), "Repo Owner <owner@example.com>");
}

// ── 가짜 git: 모든 판정·오류 길 ────────────────────────

type Handler = Box<dyn Fn(&[String]) -> PortResult<GitOutput> + Send + Sync>;

struct Fake {
    calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
    handler: Handler,
}

impl Fake {
    fn new(h: impl Fn(&[String]) -> PortResult<GitOutput> + Send + Sync + 'static) -> Fake {
        Fake { calls: Mutex::new(Vec::new()), handler: Box::new(h) }
    }
    fn calls(&self) -> Vec<(PathBuf, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
    fn stash_args(&self) -> Option<Vec<String>> {
        self.calls().into_iter().map(|c| c.1).find(|a| a.iter().any(|x| x == "stash"))
    }
}

impl Git for Fake {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.calls.lock().unwrap().push((repo.to_path_buf(), a.clone()));
        (self.handler)(&a)
    }
}

fn ok(s: &str) -> PortResult<GitOutput> {
    Ok(GitOutput { code: 0, stdout: s.to_string(), stderr: String::new() })
}

fn bad(code: i32, err: &str) -> PortResult<GitOutput> {
    Ok(GitOutput { code, stdout: String::new(), stderr: err.to_string() })
}

fn has(a: &[String], w: &str) -> bool {
    a.iter().any(|x| x == w)
}

fn repo_path() -> PathBuf {
    PathBuf::from("Z:\\no\\such\\repo")
}

/// 상태를 `status` 로 주는 가짜 + 나머지는 기본 답
fn fake_with_status(status: &'static str) -> Fake {
    Fake::new(move |a| {
        if has(a, "status") {
            ok(status)
        } else if has(a, "ls-files") {
            ok("")
        } else if has(a, "rev-parse") || has(a, "config") {
            bad(1, "")
        } else {
            ok("")
        }
    })
}

fn verdict_of(status: &'static str, start: &[&str], story: bool, made: bool) -> Leftover {
    check_leftovers(&fake_with_status(status), &repo_path(), &v(start), story, made).unwrap().verdict
}

#[test]
fn fake_every_verdict_is_reachable() {
    assert_eq!(verdict_of("", &[], false, true), Leftover::Clean);
    assert_eq!(verdict_of(" M a.txt\0", &[" M a.txt"], true, false), Leftover::Clean);
    assert_eq!(verdict_of("?? b.txt\0", &[], false, true), Leftover::Commit);
    assert_eq!(verdict_of("?? b.txt\0", &[], false, false), Leftover::Refuse(LeftoverWhy::Dirty));
    assert_eq!(verdict_of("?? b.txt\0", &[" M a.txt"], false, true), Leftover::Refuse(LeftoverWhy::Dirty));
    assert_eq!(verdict_of("?? b.txt\0", &[], true, false), Leftover::Stash(None));
    assert_eq!(
        verdict_of(" M a.txt\0?? b.txt\0", &[" M a.txt"], true, true),
        Leftover::Stash(Some(v(&["b.txt"])))
    );
    assert_eq!(verdict_of("MM a.txt\0", &[" M a.txt"], true, true), Leftover::Refuse(LeftoverWhy::ReadOnly));
    assert_eq!(verdict_of("?? b.txt\0", &[" M a.txt"], true, true), Leftover::Refuse(LeftoverWhy::ReadOnly));
}

#[test]
fn fake_status_failure_is_an_error_never_clean() {
    let f = Fake::new(|_| bad(128, "fatal: not a git repository\nsecond line"));
    let e = check_leftovers(&f, &repo_path(), &[], true, true).unwrap_err();
    assert_eq!(e, LeftoverError::Git { what: "status".into(), code: 128, detail: "fatal: not a git repository".into() });
    let f = Fake::new(|_| Err(PortError::new(PortErrorKind::Unavailable, "git.exe 없음")));
    let e = check_leftovers(&f, &repo_path(), &[], true, true).unwrap_err();
    assert!(matches!(&e, LeftoverError::Port(m) if m.contains("git.exe")), "{e:?}");
    assert!(e.to_string().contains("git.exe"));
}

#[test]
fn fake_status_stdout_used_when_stderr_empty() {
    let f = Fake::new(|_| Ok(GitOutput { code: 3, stdout: "from stdout".into(), stderr: "  \n".into() }));
    let e = status_lines(&f, &repo_path()).unwrap_err();
    assert_eq!(e, LeftoverError::Git { what: "status".into(), code: 3, detail: "from stdout".into() });
}

fn stash_fake(code: i32, stderr: &'static str, refs: Vec<&'static str>) -> Fake {
    // rev-parse 는 부를 때마다 refs 의 다음 값(빈 문자열 = 없음)
    let n = std::sync::atomic::AtomicUsize::new(0);
    Fake::new(move |a| {
        if has(a, "rev-parse") {
            let i = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let s = refs.get(i).copied().unwrap_or("");
            return if s.is_empty() { bad(1, "") } else { ok(&format!("{s}\n")) };
        }
        if has(a, "ls-files") {
            return ok("");
        }
        if has(a, "config") {
            return bad(1, "");
        }
        if has(a, "stash") {
            return Ok(GitOutput { code, stdout: String::new(), stderr: stderr.to_string() });
        }
        ok("")
    })
}

#[test]
fn fake_stash_success_needs_a_new_ref() {
    let nl = v(&[" M a.txt"]);
    let f = stash_fake(0, "", vec!["", "aaaaaaa1111"]);
    let s = stash_leftovers(&f, &repo_path(), &req(&nl, None)).unwrap();
    assert_eq!(s.sha, "aaaaaaa1111");
    assert_eq!(s.files, 1);
    // ref 가 그대로면 보관하지 않은 것
    let f = stash_fake(0, "", vec!["same", "same"]);
    assert_eq!(stash_leftovers(&f, &repo_path(), &req(&nl, None)), Err(LeftoverError::NothingStashed));
    let f = stash_fake(0, "", vec!["", ""]);
    assert_eq!(stash_leftovers(&f, &repo_path(), &req(&nl, None)), Err(LeftoverError::NothingStashed));
    // 앞에 있던 보관본이 있고 새로 생김
    let f = stash_fake(0, "", vec!["old", "new"]);
    assert_eq!(stash_leftovers(&f, &repo_path(), &req(&nl, None)).unwrap().sha, "new");
}

#[test]
fn fake_stash_failure_keeps_files_and_reports_whether_a_stash_was_made() {
    let nl = v(&[" M a.txt"]);
    let f = stash_fake(1, "error: unable to unlink old 'a.txt': Permission denied\nmore", vec!["", ""]);
    let e = stash_leftovers(&f, &repo_path(), &req(&nl, None)).unwrap_err();
    assert_eq!(
        e,
        LeftoverError::StashFailed { code: 1, detail: "error: unable to unlink old 'a.txt': Permission denied".into(), stash_created: None }
    );
    assert!(e.to_string().contains("Permission denied") && !e.to_string().contains("만들어졌습니다"));
    // 보관본이 만들어진 뒤 치우다 실패
    let f = stash_fake(1, "failed to remove", vec!["", "deadbeefcafe"]);
    let e = stash_leftovers(&f, &repo_path(), &req(&nl, None)).unwrap_err();
    assert!(matches!(&e, LeftoverError::StashFailed { stash_created: Some(s), .. } if s == "deadbeefcafe"), "{e:?}");
    assert!(e.to_string().contains("deadbee") && e.to_string().contains("만들어졌습니다"));
    // 앞에 있던 보관본이 그대로면 만든 것이 아니다
    let f = stash_fake(1, "x", vec!["old", "old"]);
    let e = stash_leftovers(&f, &repo_path(), &req(&nl, None)).unwrap_err();
    assert!(matches!(e, LeftoverError::StashFailed { stash_created: None, .. }));
}

#[test]
fn fake_stash_timeout_is_a_port_error() {
    let f = Fake::new(|a| {
        if has(a, "stash") {
            Err(PortError::new(PortErrorKind::Timeout, "git stash 가 60초 안에 끝나지 않았다"))
        } else if has(a, "ls-files") {
            ok("")
        } else {
            bad(1, "")
        }
    });
    let e = stash_leftovers(&f, &repo_path(), &req(&[], None)).unwrap_err();
    assert!(matches!(&e, LeftoverError::Port(m) if m.contains("60초")), "{e:?}");
}

#[test]
fn fake_unreadable_index_stops_before_stash() {
    let f = Fake::new(|a| if has(a, "ls-files") { bad(128, "index.lock exists\nx") } else { bad(1, "") });
    let e = stash_leftovers(&f, &repo_path(), &req(&v(&["?? x"]), None)).unwrap_err();
    assert_eq!(e, LeftoverError::IndexUnreadable { detail: "index.lock exists".into() });
    assert!(f.stash_args().is_none(), "색인을 못 읽으면 stash 를 부르지 않는다");
    let f = Fake::new(|a| if has(a, "ls-files") { Err(PortError::new(PortErrorKind::Unavailable, "x")) } else { bad(1, "") });
    let e = stash_leftovers(&f, &repo_path(), &req(&[], None)).unwrap_err();
    assert!(matches!(e, LeftoverError::Port(_)));
    assert!(f.stash_args().is_none());
}

#[test]
fn spy_every_call_goes_through_the_port_with_repo_and_rule_message() {
    let ls = "100644 aaa 0\ta.txt\x00160000 bbb 0\tlibs/dep\0";
    let n = std::sync::atomic::AtomicUsize::new(0);
    let f = Fake::new(move |a| {
        if has(a, "rev-parse") {
            let i = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return if i == 0 { bad(1, "") } else { ok("c0ffee0\n") };
        }
        if has(a, "ls-files") {
            return ok(ls);
        }
        if has(a, "config") {
            return bad(1, "");
        }
        ok("")
    });
    let repo = repo_path();
    let nl = v(&["?? x.txt"]);
    let s = stash_leftovers(&f, &repo, &req(&nl, None)).unwrap();
    let calls = f.calls();
    assert!(calls.iter().all(|(r, _)| *r == repo), "모든 호출이 repo 인자를 받는다");
    let first: Vec<&str> = calls.iter().map(|(_, a)| a.iter().find(|x| !x.starts_with('-') && !x.contains('=') && *x != "core.quotePath=false" && *x != "core.longpaths=true").map(String::as_str).unwrap_or("")).collect();
    assert!(first.iter().all(|c| ["rev-parse", "ls-files", "config", "stash"].contains(c)), "{first:?}");
    let stash = f.stash_args().unwrap();
    let expect = stash_message("r1", Some(2), "repo", "story/x");
    assert_eq!(s.message, expect);
    let pos = stash.iter().position(|x| x == "stash").unwrap();
    assert_eq!(&stash[pos..pos + 3], ["stash", "push", "--include-untracked"]);
    assert_eq!(stash[pos + 3], "-m");
    assert_eq!(stash[pos + 4], expect);
    // gitlink 는 pathspec 으로 뺀다
    assert_eq!(&stash[pos + 5..], ["--", ":(literal).", ":(exclude,literal)libs/dep"]);
    // 신원은 -c 로(전역 설정을 끈 git 이 stash 를 못 하는 일을 막는다)
    assert!(stash.contains(&format!("user.name={}", DEFAULT_IDENTITY.0)));
    assert!(stash.contains(&format!("user.email={}", DEFAULT_IDENTITY.1)));
    // 색인은 quotePath=false 로 읽는다
    assert!(calls.iter().any(|(_, a)| a == &v(&["-c", "core.quotePath=false", "ls-files", "-s", "-z"])));
}

#[test]
fn spy_paths_become_literal_pathspecs_without_exclude_when_no_gitlink() {
    let f = Fake::new(|a| if has(a, "rev-parse") { bad(1, "") } else if has(a, "ls-files") { ok("100644 a 0\tx\0") } else { bad(1, "") });
    let paths = v(&["b.txt", "src/c.txt"]);
    let _ = stash_leftovers(&f, &repo_path(), &req(&[], Some(&paths)));
    let stash = f.stash_args().unwrap();
    let pos = stash.iter().position(|x| x == "stash").unwrap();
    assert_eq!(&stash[pos + 5..], ["--", ":(literal)b.txt", ":(literal)src/c.txt"]);
    // 경로가 없고 gitlink 도 없으면 pathspec 자체가 없다
    let f = Fake::new(|a| if has(a, "rev-parse") { bad(1, "") } else if has(a, "ls-files") { ok("100644 a 0\tx\0") } else { bad(1, "") });
    let _ = stash_leftovers(&f, &repo_path(), &req(&[], None));
    let stash = f.stash_args().unwrap();
    let pos = stash.iter().position(|x| x == "stash").unwrap();
    assert_eq!(stash.len(), pos + 5, "{stash:?}");
}

#[test]
fn spy_status_command_is_exact() {
    let f = fake_with_status("");
    status_lines(&f, &repo_path()).unwrap();
    assert_eq!(f.calls(), vec![(repo_path(), v(&["-c", "core.longpaths=true", "status", "--porcelain=v1", "-z", "--ignore-submodules=all"]))]);
}

// ── 순수: 상태 -z 읽기 ─────────────────────────────────

#[test]
fn parse_status_z_covers_every_field() {
    assert_eq!(parse_status_z(""), Vec::<String>::new());
    assert_eq!(parse_status_z(" M a.txt\0"), v(&[" M a.txt"]));
    assert_eq!(parse_status_z("A  new.txt\0D  gone.txt\0?? u.txt\0UU c.txt\0"), v(&["A  new.txt", "D  gone.txt", "?? u.txt", "UU c.txt"]));
    // 이름 바꿈·복사: "새\0옛\0" → "옛 -> 새"
    assert_eq!(parse_status_z("R  new.txt\0old.txt\0 M z.txt\0"), v(&["R  old.txt -> new.txt", " M z.txt"]));
    assert_eq!(parse_status_z("C  copy.txt\0orig.txt\0"), v(&["C  orig.txt -> copy.txt"]));
    assert_eq!(parse_status_z(" R new.txt\0old.txt\0"), v(&[" R old.txt -> new.txt"]));
    // 공백·한글·`[`
    assert_eq!(parse_status_z("?? a b.txt\0"), v(&["?? \"a b.txt\""]));
    assert_eq!(parse_status_z("?? 한.txt\0"), v(&["?? \"\\355\\225\\234.txt\""]));
    assert_eq!(parse_status_z("?? br[a].txt\0"), v(&["?? br[a].txt"]));
    // 줄바꿈·탭·따옴표·역슬래시·제어 글자
    assert_eq!(parse_status_z("?? a\nb\0"), v(&["?? \"a\\nb\""]));
    assert_eq!(parse_status_z("?? a\tb\0"), v(&["?? \"a\\tb\""]));
    assert_eq!(parse_status_z("?? q\"x\0"), v(&["?? \"q\\\"x\""]));
    assert_eq!(parse_status_z("?? b\\s\0"), v(&["?? \"b\\\\s\""]));
    assert_eq!(parse_status_z("?? a\rb\x07\x08\x0b\x0c\x01\x7f\0"), v(&["?? \"a\\rb\\a\\b\\v\\f\\001\\177\""]));
    // 이름 바꿈의 한쪽만 특수해도 각각 싼다
    assert_eq!(parse_status_z("R  새 name.txt\0old.txt\0"), v(&["R  old.txt -> \"\\354\\203\\210 name.txt\""]));
    // 이름 바꿈에 옛 경로 항목이 빠진 깨진 출력도 패닉 없이
    assert_eq!(parse_status_z("R  only-new.txt\0"), v(&["R   -> only-new.txt"]));
    // 깨진(짧은) 항목은 그대로 둔다 — 규칙이 갈라 내지 않는다
    assert_eq!(parse_status_z("?\0"), v(&["?"]));
    assert_eq!(parse_status_z("??\0"), v(&["??"]));
}

#[test]
fn quoted_lines_are_never_split_by_the_rule_but_plain_ones_are() {
    use claude_board_autopilot::rules::leftovers::leftover;
    let start = parse_status_z(" M a.txt\0");
    let dirty = parse_status_z(" M a.txt\0?? 한.txt\0");
    assert_eq!(leftover(&dirty, &start, true, true), Leftover::Refuse(LeftoverWhy::ReadOnly));
    let dirty = parse_status_z(" M a.txt\0?? plain.txt\0");
    assert_eq!(leftover(&dirty, &start, true, true), Leftover::Stash(Some(v(&["plain.txt"]))));
    // 출발 때 깨끗하면 따옴표 경로도 모두 보관한다
    let dirty = parse_status_z("?? 한.txt\0");
    assert_eq!(leftover(&dirty, &[], true, true), Leftover::Stash(None));
}

// ── 기록 ─────────────────────────────────────────────

fn stashed(n: usize) -> Stashed {
    let paths: Vec<String> = (0..n).map(|i| format!("?? f{i}.txt")).collect();
    Stashed { sha: "0123456789abcdef".into(), message: "msg".into(), files: n, paths: paths.into_iter().take(RECORD_PATHS_MAX).collect() }
}

#[test]
fn record_has_exactly_the_run_json_keys() {
    let j = record_leftovers("C:/work/repo", "story/x", Some(3), &stashed(2), 1700000000.123456);
    let o = j.as_object().unwrap();
    let keys: Vec<&str> = o.keys().map(String::as_str).collect();
    assert_eq!(keys, ["repo", "branch", "unit", "stash", "message", "files", "paths", "at"]);
    assert_eq!(j["repo"], "C:/work/repo");
    assert_eq!(j["branch"], "story/x");
    assert_eq!(j["unit"], 3);
    assert_eq!(j["stash"], "0123456789abcdef");
    assert_eq!(j["message"], "msg");
    assert_eq!(j["files"], 2);
    assert_eq!(j["paths"], serde_json::json!(["?? f0.txt", "?? f1.txt"]));
    assert_eq!(j["at"], 1700000000.123);
    // 단위를 모르면 null
    let j = record_leftovers("r", "b", None, &stashed(0), 1.0);
    assert!(j["unit"].is_null());
    assert_eq!(j["paths"], serde_json::json!([]));
}

#[test]
fn stash_request_new_lines_cap_paths_at_twenty_but_count_all() {
    let all: Vec<String> = (0..25).map(|i| format!("?? f{i}.txt")).collect();
    let f = stash_fake(0, "", vec!["", "abc1234567"]);
    let s = stash_leftovers(&f, &repo_path(), &req(&all, None)).unwrap();
    assert_eq!(s.files, 25);
    assert_eq!(s.paths.len(), 20);
    assert_eq!(s.paths[19], "?? f19.txt");
}

#[test]
fn new_lines_drops_the_start_lines_in_order() {
    let dirty = v(&[" M a.txt", "?? n.txt", " M b.txt"]);
    assert_eq!(new_lines(&dirty, &v(&[" M a.txt"])), v(&["?? n.txt", " M b.txt"]));
    assert_eq!(new_lines(&dirty, &[]), dirty);
    assert!(new_lines(&[], &v(&[" M a.txt"])).is_empty());
}

#[test]
fn event_texts_match_the_original_wording() {
    let s = Stashed { sha: "0123456789abcdef".into(), message: "m".into(), files: 3, paths: vec![] };
    assert_eq!(
        stashed_event(Some(2), "repo", "story/x", &s, false),
        "단위 2 이 읽기 전용으로 걸어 둔 repo story/x 에 남긴 변경 3개를 엔진이 따로 보관했습니다(git stash 0123456, 판정·다음 단위에 넣지 않음)."
    );
    assert!(stashed_event(None, "repo", "b", &s, true).starts_with("단위 ? 이"));
    assert!(stashed_event(None, "repo", "b", &s, true).ends_with("판정·다음 단위에 넣지 않음). 보관하지 못한 변경이 남았습니다."));
    let e = LeftoverError::NothingStashed;
    assert_eq!(
        stash_failed_event(Some(1), "repo", "b", 4, &e),
        "단위 1 이 읽기 전용으로 걸어 둔 repo b 에 남긴 변경 4개를 보관하지 못했습니다: 보관할 변경이 없었습니다"
    );
}

#[test]
fn error_display_covers_every_variant() {
    let all = [
        LeftoverError::Port("p".into()),
        LeftoverError::Git { what: "status".into(), code: 2, detail: "d".into() },
        LeftoverError::IndexUnreadable { detail: "i".into() },
        LeftoverError::NothingStashed,
        LeftoverError::StashFailed { code: 1, detail: "s".into(), stash_created: None },
        LeftoverError::StashFailed { code: 1, detail: "s".into(), stash_created: Some("abcdef0123".into()) },
    ];
    let t: Vec<String> = all.iter().map(|e| e.to_string()).collect();
    assert!(t[0].contains("p") && t[1].contains("status") && t[1].contains("2") && t[2].contains('i') && t[4].contains("(1)") && t[5].contains("abcdef0"));
    assert!(!t[5].contains("abcdef01"), "일곱 글자까지만");
    let _: &dyn std::error::Error = &all[0];
}
