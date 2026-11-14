//! 엔진 쪽 시험: `engine::new_repo` 가 실제 파일 시스템과 git 으로 규칙 `rules::new_repo` 의 계획을 끝까지 돌리는가.
//! 규칙 자리의 시험(`tests/rules_new_repo.rs`)은 가짜 폴더·git 으로 판정만 본다. 여기는 진짜로 만들어 본다:
//! 가지 세 개 · 커밋 하나 · 빈 폴더 재사용 · 무엇이 든 폴더·정션 거부 · git 실패 보고 · 전역 설정 읽기.
//! 환경을 고쳐 쓰는 시험(`GIT_CONFIG_GLOBAL`·PATH)과 git 을 띄우는 시험은 같은 프로세스에서 서로 밟지 않게 하나씩 돌린다.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use tempfile::TempDir;

use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::engine::new_repo::{create_repo_at, short_message, BoardGit, FolderFiles};
use claude_board_autopilot::ports::Git;
use claude_board_autopilot::rules::new_repo::{Folders, GitRunner, NewRepoPlan, PathState, NEW_REPO_BRANCHES, NEW_REPO_MESSAGE};

/// 환경 고치기·git 띄우기가 겹치지 않게(잘라 낸 값이 남아 다른 시험을 망치지 않게) 하나씩
static SERIAL: Mutex<()> = Mutex::new(());

fn to_s(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn as_strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn plan_at(path: &Path) -> NewRepoPlan {
    NewRepoPlan {
        path: to_s(path),
        branches: NEW_REPO_BRANCHES.iter().map(|s| s.to_string()).collect(),
        head: "dev".to_string(),
        base: "live".to_string(),
        message: NEW_REPO_MESSAGE.to_string(),
    }
}

fn git_out(repo: &Path, args: &[&str]) -> (i32, String) {
    let out = ProcessGit::new().run(repo, args).expect("git 을 띄우지 못했다");
    (out.code, out.stdout)
}

/// 고쳐 쓴 환경 값을 떨어뜨릴 때 원래대로 돌려놓는다
struct EnvSnapshot {
    saved: Vec<(String, Option<OsString>)>,
}

impl EnvSnapshot {
    fn new(vars: &[&str]) -> EnvSnapshot {
        let saved = vars.iter().map(|k| (k.to_string(), std::env::var_os(k))).collect();
        EnvSnapshot { saved }
    }
}

impl Drop for EnvSnapshot {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

/// 정션 하나를 만든다(mklink /J 는 관리자 권한이 필요 없다). Windows 전용
#[cfg(windows)]
fn make_junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("mklink /J 를 띄우지 못했다");
    assert!(out.status.success(), "정션 만들기 실패: {}", String::from_utf8_lossy(&out.stderr));
}

// ── 오류 글 줄이기 ───────────────────────────────────

#[test]
fn short_message_keeps_under_the_limit_and_cuts_over() {
    let full = "가".repeat(300);
    assert_eq!(short_message(&full, 300), full); // 경계: 딱 300 은 그대로
    let cut = short_message(&"나".repeat(301), 300);
    assert_eq!(cut.chars().count(), 301);
    assert!(cut.starts_with(&"나".repeat(300)));
    assert!(cut.ends_with('…'));
    // 여러 바이트 글자 중간에서도 글자 단위로 자른다(바이트로 자르면 깨진다)
    assert_eq!(short_message("한글", 1), "한…");
    assert_eq!(short_message("한글", 2), "한글");
    assert_eq!(short_message("abc", 0), "…");
}

// ── 폴더 상태 ────────────────────────────────────────

#[test]
fn path_state_sorts_missing_file_empty_junk_and_full() {
    let dir = TempDir::new().expect("임시 폴더");
    let root = dir.path();
    assert_eq!(FolderFiles.state(&to_s(&root.join("nothing"))), PathState::Missing);

    fs::write(root.join("plain.txt"), "x").expect("쓰기");
    assert_eq!(FolderFiles.state(&to_s(&root.join("plain.txt"))), PathState::Other);

    fs::create_dir(root.join("empty")).expect("만들기");
    assert_eq!(FolderFiles.state(&to_s(&root.join("empty"))), PathState::EmptyDir);

    // 찌거기뿐인 폴더는 빈 폴더다. Windows 는 대소문자를 가리지 않는다(THUMBS.DB)
    fs::create_dir(root.join("junk")).expect("만들기");
    fs::write(root.join("junk/.DS_Store"), "").expect("쓰기");
    fs::write(root.join("junk/desktop.ini"), "").expect("쓰기");
    fs::write(root.join("junk/THUMBS.DB"), "").expect("쓰기");
    assert_eq!(FolderFiles.state(&to_s(&root.join("junk"))), PathState::EmptyDir);

    // 찌거기 밖에 무엇이 있으면 무엇이 든 폴더다
    fs::create_dir(root.join("kept")).expect("만들기");
    fs::write(root.join("kept/.DS_Store"), "").expect("쓰기");
    fs::write(root.join("kept/keep.txt"), "x").expect("쓰기");
    assert_eq!(FolderFiles.state(&to_s(&root.join("kept"))), PathState::Dir);
}

#[test]
#[cfg(windows)]
fn path_state_reads_through_a_junction() {
    let dir = TempDir::new().expect("임시 폴더");
    let root = dir.path();
    fs::create_dir(root.join("real")).expect("만들기");
    fs::write(root.join("real/keep.txt"), "x").expect("쓰기");
    fs::create_dir(root.join("junk")).expect("만들기");
    fs::write(root.join("junk/.DS_Store"), "").expect("쓰기");

    // 끊긴 정션을 만들려고 폴더를 잠시 두었다 지운다
    fs::create_dir(root.join("gone")).expect("만들기");
    make_junction(&root.join("link-dir"), &root.join("real"));
    make_junction(&root.join("link-junk"), &root.join("junk"));
    make_junction(&root.join("link-gone"), &root.join("gone"));
    fs::remove_dir(root.join("gone")).expect("지우기");

    fs::write(root.join("plain.txt"), "x").expect("쓰기");
    make_junction(&root.join("link-file"), &root.join("plain.txt"));

    // 가리킨 자리가 폴더면 폴더다. 그 안이 찌거기뿐이어도 빈 폴더로 보지 않는다(원본은 링크의 이름들을 읽지 않는다)
    assert_eq!(FolderFiles.state(&to_s(&root.join("link-dir"))), PathState::Dir);
    assert_eq!(FolderFiles.state(&to_s(&root.join("link-junk"))), PathState::Dir);
    // 가리킨 자리가 파일이거나 끊기면 폴더가 아니다(이런 자리는 만들지 않는다)
    assert_eq!(FolderFiles.state(&to_s(&root.join("link-file"))), PathState::Other);
    assert_eq!(FolderFiles.state(&to_s(&root.join("link-gone"))), PathState::Other);
}

#[test]
fn make_dir_makes_one_folder_and_fails_without_parents() {
    let dir = TempDir::new().expect("임시 폴더");
    let made = dir.path().join("one");
    assert!(FolderFiles.make_dir(&to_s(&made)));
    assert!(made.is_dir());
    // 부모가 없으면 만들지 않는다
    assert!(!FolderFiles.make_dir(&to_s(&dir.path().join("no-parent").join("child"))));
    assert!(!dir.path().join("no-parent").exists());
    // 이미 있는 자리는 false(Swift mkdir 의 EEXIST → false 1:1)
    assert!(!FolderFiles.make_dir(&to_s(&made)));
}

// ── 보드 git ─────────────────────────────────────────

#[test]
fn identity_reads_user_global_config_and_needs_both_name_and_email() {
    let _serial = SERIAL.lock().expect("잠금");
    let _env = EnvSnapshot::new(&["GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM"]);
    let dir = TempDir::new().expect("임시 폴더");
    let git = ProcessGit::new();
    let board = BoardGit { git: &git };

    // 보호 환경 git 은 전역 설정을 막으므로 여기는 시험 전용 전역 설정 파일을 환경으로 건다
    let cfg = dir.path().join("gitconfig");
    fs::write(&cfg, "[user]\n\tname = 테스터 사람\n\temail = tester@example.com\n").expect("설정 쓰기");
    std::env::set_var("GIT_CONFIG_GLOBAL", cfg.as_os_str());
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    assert_eq!(board.identity(), Some(("테스터 사람".to_string(), "tester@example.com".to_string())));

    // 이름만 있으면 안 된다
    fs::write(&cfg, "[user]\n\tname = 테스터 사람\n").expect("설정 쓰기");
    assert_eq!(board.identity(), None);

    // 메일만 있어도 안 된다
    fs::write(&cfg, "[user]\n\temail = tester@example.com\n").expect("설정 쓰기");
    assert_eq!(board.identity(), None);

    // 빈 파일이면 없다
    fs::write(&cfg, "").expect("설정 쓰기");
    assert_eq!(board.identity(), None);

    // 가리킨 파일이 없어도 없다(시스템 설정은 끈 상태)
    std::env::set_var("GIT_CONFIG_GLOBAL", dir.path().join("no-such.cfg").as_os_str());
    assert_eq!(board.identity(), None);
}

#[test]
fn board_git_run_trims_stdout_and_reports_failure_stderr() {
    let _serial = SERIAL.lock().expect("잠금");
    let dir = TempDir::new().expect("임시 폴더");
    let git = ProcessGit::new();
    let board = BoardGit { git: &git };

    // 저장소가 아니면 실패: 종료 코드와 표준 오류(앞뒤 빈칸 없이)를 돌려준다
    let (code, err) = board.run(&as_strings(&["rev-parse", "HEAD"]), &to_s(dir.path()));
    assert_ne!(code, 0, "{err}");
    assert!(!err.is_empty(), "실패한 git 은 표준 오류를 보여야 한다");
    assert_eq!(err.trim(), err, "앞뒤 빈칸을 깎는다");
    assert!(!err.contains('\n'), "여러 줄을 한 줄의 글로 돌려준다: {err:?}");

    // 저장소를 만들고 빈 커밋을 하나 두면 표준 출력은 앞뒤 빈칸을 깎아 돌려준다
    let repo = dir.path().join("repo");
    fs::create_dir(&repo).expect("만들기");
    let (code, _) = board.run(&as_strings(&["init", "-q"]), &to_s(&repo));
    assert_eq!(code, 0);
    let args = [
        "-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "--allow-empty", "-m", "msg",
    ];
    let (code, _) = board.run(&as_strings(&args), &to_s(&repo));
    assert_eq!(code, 0);
    let (code, head) = board.run(&as_strings(&["rev-parse", "HEAD"]), &to_s(&repo));
    assert_eq!(code, 0);
    assert_eq!(head.chars().count(), 40, "끝의 줄바꿈이 깎였다: {head:?}");
    let (code, head_by_port) = git_out(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(code, 0);
    assert_eq!(head, head_by_port.trim());
}

#[test]
fn board_git_run_reports_when_git_cannot_be_spawned() {
    let _serial = SERIAL.lock().expect("잠금");
    let _env = EnvSnapshot::new(&["PATH", "ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]);
    std::env::remove_var("PATH");
    std::env::remove_var("ProgramFiles");
    std::env::remove_var("ProgramW6432");
    std::env::remove_var("ProgramFiles(x86)");
    let dir = TempDir::new().expect("임시 폴더");
    let git = ProcessGit::new();
    let board = BoardGit { git: &git };
    let (code, err) = board.run(&as_strings(&["status"]), &to_s(dir.path()));
    assert_eq!(code, -1, "{err}");
    assert!(err.starts_with("git 을 띄우지 못했습니다"), "{err}");
}

// ── 만들기 유스케이스 ────────────────────────────────

#[test]
fn create_repo_at_makes_three_branches_on_one_empty_commit() {
    let _serial = SERIAL.lock().expect("잠금");
    let _env = EnvSnapshot::new(&["GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM"]);
    let dir = TempDir::new().expect("임시 폴더");
    // 전역 설정을 시험 파일로 고정해 커밋의 사람 정보가 정해진 값으로 나오게 한다
    let cfg = dir.path().join("gitconfig");
    fs::write(&cfg, "[user]\n\tname = 테스터 사람\n\temail = tester@example.com\n").expect("설정 쓰기");
    std::env::set_var("GIT_CONFIG_GLOBAL", cfg.as_os_str());
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");

    let repo = dir.path().join("fresh");
    let r = create_repo_at(&plan_at(&repo), &ProcessGit::new());
    assert!(r.error.is_none(), "{r:?}");
    assert!(r.ok, "{r:?}");
    assert!(r.made, "{r:?}");
    assert_eq!(r.done[0], "폴더를 만들었습니다");
    assert_eq!(r.done[1], "git init(처음 가지 dev)");
    assert!(
        r.done[2].starts_with("빈 첫 커밋 ") && r.done[2].ends_with(" 에 가지 dev·live·master"),
        "{:?}",
        r.done
    );
    let commit = r.commit.expect("첫 커밋");

    // 가지 세 개가 같은 첫 커밋을 가리킨다
    let (code, refs) = git_out(&repo, &["for-each-ref", "refs/heads", "--format=%(refname:short) %(objectname)"]);
    assert_eq!(code, 0, "{refs}");
    let heads: Vec<(String, String)> = refs
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (n, h) = l.rsplit_once(' ').expect("ref 줄 꼴");
            (n.to_string(), h.to_string())
        })
        .collect();
    assert_eq!(
        heads,
        vec![
            ("dev".to_string(), commit.clone()),
            ("live".to_string(), commit.clone()),
            ("master".to_string(), commit.clone())
        ]
    );

    // 커밋은 하나뿐이고 부모가 없다
    let (code, count) = git_out(&repo, &["rev-list", "--count", "--all"]);
    assert_eq!(code, 0);
    assert_eq!(count.trim(), "1");
    let (code, author) = git_out(&repo, &["show", "-q", "--format=%P|%an|%ae|%s", "HEAD"]);
    assert_eq!(code, 0, "{author}");
    // 부모는 없다(%P 가 빈 것) · 사람은 전역 설정의 값 · 글은 계획의 것
    assert_eq!(author.trim(), format!("|테스터 사람|tester@example.com|{NEW_REPO_MESSAGE}"));
}

#[test]
fn create_repo_at_reuses_empty_but_refuses_full_folder_or_junction() {
    let _serial = SERIAL.lock().expect("잠금");
    let dir = TempDir::new().expect("임시 폴더");

    let empty = dir.path().join("empty");
    fs::create_dir(&empty).expect("만들기");
    let r = create_repo_at(&plan_at(&empty), &ProcessGit::new());
    assert!(r.ok, "{r:?}");
    assert!(!r.made);
    assert_eq!(r.done[0], "빈 폴더를 그대로 씁니다");

    // 찌거기뿐인 폴더도 빈 폴더로 재사용한다
    let junk = dir.path().join("junk");
    fs::create_dir(&junk).expect("만들기");
    fs::write(junk.join("desktop.ini"), "").expect("쓰기");
    let r_j = create_repo_at(&plan_at(&junk), &ProcessGit::new());
    assert!(r_j.ok, "{r_j:?}");
    assert!(!r_j.made);
    assert_eq!(r_j.done[0], "빈 폴더를 그대로 씁니다");

    // 무엇이 든 폴더는 만들지 않는다
    let full = dir.path().join("full");
    fs::create_dir(&full).expect("만들기");
    fs::write(full.join("some.txt"), "x").expect("쓰기");
    let r_f = create_repo_at(&plan_at(&full), &ProcessGit::new());
    assert!(!r_f.ok);
    assert_eq!(
        r_f.error.as_deref(),
        Some(format!("그 자리에 이미 무엇이 있어 만들지 않았습니다: {}", to_s(&full)).as_str())
    );

    // 정션이 가리킨 폴더가 비어 있어도 정션 자체는 무엇이 든 폴더다: 만들지 않는다
    #[cfg(windows)]
    {
        let link = dir.path().join("link-empty");
        make_junction(&link, &empty);
        let r_l = create_repo_at(&plan_at(&link), &ProcessGit::new());
        assert!(
            r_l.error
                .as_deref()
                .unwrap_or_default()
                .starts_with("그 자리에 이미 무엇이 있어 만들지 않았습니다"),
            "{r_l:?}"
        );
    }
}

#[test]
fn create_repo_at_reports_when_it_cannot_make_the_folder() {
    let _serial = SERIAL.lock().expect("잠금");
    let dir = TempDir::new().expect("임시 폴더");
    let missing = dir.path().join("no-parent").join("child");
    let r = create_repo_at(&plan_at(&missing), &ProcessGit::new());
    assert!(!r.ok, "{r:?}");
    assert!(!r.made);
    assert_eq!(
        r.error.as_deref(),
        Some(format!("폴더를 만들지 못했습니다: {}", to_s(&missing)).as_str())
    );
}

#[test]
fn create_repo_at_reports_git_failure_and_keeps_the_folder() {
    let _serial = SERIAL.lock().expect("잠금");
    let dir = TempDir::new().expect("임시 폴더");
    let repo = dir.path().join("bad-head");
    let mut plan = plan_at(&repo);
    plan.head = "a..b".to_string(); // git 이 거부하는 가지 이름
    let r = create_repo_at(&plan, &ProcessGit::new());
    assert!(!r.ok, "{r:?}");
    assert!(r.made, "{r:?}");
    assert_eq!(r.done[0], "폴더를 만들었습니다"); // 만든 것을 지우지 않는다
    assert!(repo.exists());
    assert!(r.commit.is_none());
    let err = r.error.expect("오류");
    assert!(err.starts_with("git init 이 실패했습니다(종료 코드 "), "{err}");
    assert!(err.contains("): "), "표준 오류도 함께 보인다: {err}");
}
