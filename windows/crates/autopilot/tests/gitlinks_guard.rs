//! 하위 모듈 막기 시험(`engine::gitlinks_guard`). `real-git` 표시가 붙은 것은 이 PC 의 진짜 git.exe 와 새 임시 폴더로 돈다.
//! 사용자의 ~/.gitconfig·실제 저장소·네트워크는 건드리지 않는다(시험 준비용 git 은 시스템·전역 설정을 끄고 로컬 경로만 쓴다).

use claude_board_autopilot::engine::gitlinks_guard::{
    check_start, check_start_with, check_unit, check_unit_with, repo_name, start_message, unit_stop, Snapshot, Verdict,
};
use claude_board_autopilot::ports::{Git, GitOutput, PortError, PortErrorKind, PortResult};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

// ── 진짜 git 준비 도우미(시험 자신의 git 호출) ─────────

#[cfg(windows)]
const NULL: &str = "NUL";
#[cfg(not(windows))]
const NULL: &str = "/dev/null";

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", NULL)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git 가 있어야 한다");
    assert!(out.status.success(), "git {args:?} 실패: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    sh(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    sh(dir, &["add", "a.txt"]);
    sh(dir, &["commit", "-q", "-m", "base"]);
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        // 확장 길이 접두는 git 의 하위 모듈 주소로 못 쓴다
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

/// 로컬 경로의 저장소를 하위 모듈로 더한다(시험 자신의 호출에만 file 프로토콜을 연다)
fn add_submodule(repo: &Path, source: &Path, at: &str) {
    let url = source.to_string_lossy().replace('\\', "/");
    sh(repo, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &url, at]);
}

const SHA: &str = "0123456789012345678901234567890123456789";

/// 색인에 gitlink 만 넣는다(커밋하지 않음)
fn add_gitlink(repo: &Path, at: &str) {
    sh(repo, &["update-index", "--add", "--cacheinfo", &format!("160000,{SHA},{at}")]);
}

// ── 진짜 git ─────────────────────────────────────────

#[test]
fn real_git_plain_repo_passes_start() {
    let e = Env::new();
    let r = e.repo("plain");
    let c = check_start(&r, Some("main"));
    assert_eq!(c.verdict, Verdict::Clear);
    assert_eq!(c.baseline, Some(Snapshot::default()));
    assert!(start_message(&r, &c.verdict).is_none());
    assert!(unit_stop(1, &r, &c.verdict).is_none());
}

#[test]
fn real_git_repo_with_submodule_is_blocked_at_start() {
    let e = Env::new();
    let src = e.repo("src");
    let r = e.repo("host");
    add_submodule(&r, &src, "libs/dep");
    let c = check_start(&r, Some("main"));
    assert_eq!(c.baseline, None);
    match &c.verdict {
        Verdict::Submodule { gitlinks, gitmodules } => {
            assert_eq!(gitlinks, &vec!["libs/dep".to_string()]);
            assert!(*gitmodules);
        }
        v => panic!("하위 모듈로 막혀야 한다: {v:?}"),
    }
    let m = start_message(&r, &c.verdict).unwrap();
    assert_eq!(
        m,
        "host 의 작업 폴더에 하위 모듈(색인에 중첩 git 저장소(libs/dep), .gitmodules 파일)이 있어 출발하지 않습니다. 자율주행은 git 하위 모듈이 있는 저장소를 아직 다루지 않습니다"
    );
}

#[test]
fn real_git_submodule_added_after_baseline_stops_the_unit() {
    let e = Env::new();
    let src = e.repo("src");
    let r = e.repo("host");
    let start = check_start(&r, Some("main"));
    let baseline = start.baseline.expect("통과하면 기준이 있다");
    assert_eq!(check_unit(&r, &baseline), Verdict::Clear);
    add_submodule(&r, &src, "vendor/x");
    let v = check_unit(&r, &baseline);
    assert!(matches!(&v, Verdict::Submodule { gitlinks, .. } if gitlinks == &["vendor/x".to_string()]), "{v:?}");
    let s = unit_stop(3, &r, &v).unwrap();
    assert_eq!(
        s.reason,
        "보안: 단위 3 의 host 의 작업 폴더에 하위 모듈(색인에 중첩 git 저장소(vendor/x), .gitmodules 파일)이 있어 그 자리에서 git(모델·판정·기록)을 돌리지 않습니다. 사람이 확인하세요."
    );
}

#[test]
fn real_git_gitmodules_file_without_any_gitlink_blocks() {
    let e = Env::new();
    let r = e.repo("host");
    std::fs::write(r.join(".gitmodules"), "[submodule \"a\"]\n\tpath = a\n\turl = ../a\n").unwrap();
    let c = check_start(&r, None);
    assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec![], gitmodules: true });
    assert_eq!(c.verdict.why().unwrap(), "작업 폴더에 하위 모듈(.gitmodules 파일)이 있어");
    // 실행 중: 기준에 없던 .gitmodules 가 생기면 멈춘다
    let v = check_unit(&r, &Snapshot::default());
    assert_eq!(v, Verdict::Submodule { gitlinks: vec![], gitmodules: true });
    // 기준에 이미 .gitmodules 가 있으면 새 것이 아니다
    assert_eq!(check_unit(&r, &Snapshot { gitlinks: vec![], gitmodules: true }), Verdict::Clear);
}

#[test]
fn real_git_nested_gitmodules_in_subfolder_is_not_a_submodule_marker() {
    let e = Env::new();
    let r = e.repo("host");
    std::fs::create_dir_all(r.join("docs")).unwrap();
    std::fs::write(r.join("docs/.gitmodules"), "x").unwrap();
    sh(&r, &["add", "docs/.gitmodules"]);
    assert_eq!(check_start(&r, Some("main")).verdict, Verdict::Clear);
}

#[test]
fn real_git_gitlink_only_in_index() {
    let e = Env::new();
    let r = e.repo("host");
    add_gitlink(&r, "only-index");
    for base in [None, Some("main")] {
        let c = check_start(&r, base);
        assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec!["only-index".into()], gitmodules: false }, "base={base:?}");
    }
}

#[test]
fn real_git_gitlink_only_in_head_is_seen_only_through_the_base_branch() {
    let e = Env::new();
    let r = e.repo("host");
    add_gitlink(&r, "in-head");
    sh(&r, &["commit", "-q", "-m", "gitlink"]);
    sh(&r, &["rm", "-q", "--cached", "in-head"]);
    // 색인에는 없다 → 색인만 보는 확인은 통과, 출발 가지를 함께 보면 막힌다
    assert_eq!(check_start(&r, None).verdict, Verdict::Clear);
    assert_eq!(check_start(&r, Some("main")).verdict, Verdict::Submodule { gitlinks: vec!["in-head".into()], gitmodules: false });
    // 실행 중 확인은 색인과 .gitmodules 만 본다
    assert_eq!(check_unit(&r, &Snapshot::default()), Verdict::Clear);
}

#[test]
fn real_git_gitmodules_only_in_head_blocks() {
    let e = Env::new();
    let r = e.repo("host");
    std::fs::write(r.join(".gitmodules"), "x").unwrap();
    sh(&r, &["add", ".gitmodules"]);
    sh(&r, &["commit", "-q", "-m", "gm"]);
    sh(&r, &["rm", "-q", ".gitmodules"]); // 작업 트리·색인에서 지움, HEAD 에만 남음
    assert!(!r.join(".gitmodules").exists());
    let c = check_start(&r, None);
    assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec![], gitmodules: true });
}

#[test]
fn real_git_submodule_path_with_spaces_and_non_ascii_name() {
    let e = Env::new();
    let src = e.repo("src");
    let r = e.repo("host");
    add_submodule(&r, &src, "공백 이름/서브 모듈");
    add_gitlink(&r, "탭 폴더/가나다");
    let c = check_start(&r, Some("main"));
    match c.verdict {
        Verdict::Submodule { gitlinks, .. } => {
            assert!(gitlinks.contains(&"공백 이름/서브 모듈".to_string()), "{gitlinks:?}");
            assert!(gitlinks.contains(&"탭 폴더/가나다".to_string()), "{gitlinks:?}");
        }
        v => panic!("{v:?}"),
    }
}

#[test]
fn real_git_windows_backslash_and_drive_letter_spellings_give_the_same_verdict() {
    let e = Env::new();
    let plain = e.repo("plain");
    let withsub = e.repo("with");
    add_gitlink(&withsub, "dep");
    let spellings = |p: &Path| -> Vec<PathBuf> {
        let s = p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let fwd = s.replace('\\', "/");
        let back = s.replace('/', "\\");
        let mut v = vec![PathBuf::from(&fwd), PathBuf::from(&back)];
        if s.as_bytes().get(1) == Some(&b':') {
            let lower = format!("{}{}", s[..1].to_ascii_lowercase(), &fwd[1..]);
            let upper = format!("{}{}", s[..1].to_ascii_uppercase(), &back[1..]);
            v.push(PathBuf::from(lower));
            v.push(PathBuf::from(upper));
        }
        v
    };
    for p in spellings(&plain) {
        assert_eq!(check_start(&p, Some("main")).verdict, Verdict::Clear, "{p:?}");
    }
    for p in spellings(&withsub) {
        let c = check_start(&p, Some("main"));
        assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec!["dep".into()], gitmodules: false }, "{p:?}");
        assert_eq!(start_message(&p, &c.verdict).unwrap().split(' ').next().unwrap(), "with", "{p:?}");
    }
}

#[test]
fn real_git_not_a_repository_is_closed_not_clear() {
    let e = Env::new();
    let d = e.root.join("nogit");
    std::fs::create_dir_all(&d).unwrap();
    // 위 폴더에 저장소가 없어야 한다(임시 폴더는 저장소 밖)
    let c = check_start(&d, None);
    assert!(matches!(c.verdict, Verdict::Unreadable { .. }), "{:?}", c.verdict);
    assert_eq!(c.baseline, None);
    let v = check_unit(&d, &Snapshot::default());
    assert!(matches!(v, Verdict::Unreadable { .. }));
    assert!(unit_stop(1, &d, &v).unwrap().reason.contains("색인을 읽지 못해(git ls-files 실패)"));
}

// ── 가짜 git: 모든 판정에 닿는다 ──────────────────────

/// 부른 호출을 적고 명령 앞머리에 맞는 답을 준다
struct Fake {
    calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
    answers: Vec<(&'static str, PortResult<GitOutput>)>,
}

fn ok(s: &str) -> PortResult<GitOutput> {
    Ok(GitOutput { code: 0, stdout: s.to_string(), stderr: String::new() })
}
fn fail() -> PortResult<GitOutput> {
    Ok(GitOutput { code: 128, stdout: String::new(), stderr: "fatal".into() })
}
fn link(p: &str) -> String {
    format!("160000 {SHA} 0\t{p}\0")
}

impl Fake {
    /// `answers` 는 (명령 이름, 답). 명령 이름은 `ls-files-all`(색인 전체), `ls-files-gm`(.gitmodules), `ls-tree-gm`, `ls-tree-base`, `cat-file`
    fn new(answers: Vec<(&'static str, PortResult<GitOutput>)>) -> Fake {
        Fake { calls: Mutex::new(Vec::new()), answers }
    }
    fn names(&self) -> Vec<String> {
        self.calls.lock().unwrap().iter().map(|(_, a)| kind_of(a).to_string()).collect()
    }
}

fn kind_of(a: &[String]) -> &'static str {
    let has = |s: &str| a.iter().any(|x| x == s);
    if has("ls-files") && has("--") {
        "ls-files-gm"
    } else if has("ls-files") {
        "ls-files-all"
    } else if has("ls-tree") && has("HEAD") {
        "ls-tree-gm"
    } else if has("ls-tree") {
        "ls-tree-base"
    } else if has("cat-file") {
        "cat-file"
    } else {
        "other"
    }
}

impl Git for Fake {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let k = kind_of(&a);
        self.calls.lock().unwrap().push((repo.to_path_buf(), a));
        match self.answers.iter().find(|(n, _)| *n == k) {
            Some((_, r)) => r.clone(),
            None => fail(),
        }
    }
}

/// 색인은 읽히고 `.gitmodules` 는 어디에도 없는 기본 답
fn base_answers(index: &str) -> Vec<(&'static str, PortResult<GitOutput>)> {
    vec![("ls-files-all", ok(index)), ("ls-files-gm", ok("")), ("ls-tree-gm", ok("")), ("ls-tree-base", ok("")), ("cat-file", fail())]
}

fn repo_path() -> PathBuf {
    PathBuf::from("Z:\\no\\such\\repo")
}

#[test]
fn fake_clear_when_nothing_found() {
    let f = Fake::new(base_answers(""));
    let c = check_start_with(&f, &repo_path(), Some("main"));
    assert_eq!(c.verdict, Verdict::Clear);
    assert_eq!(c.baseline, Some(Snapshot::default()));
}

#[test]
fn fake_gitlink_in_index_blocks() {
    let f = Fake::new(base_answers(&format!("100644 {SHA} 0\ta.txt\0{}", link("m"))));
    let c = check_start_with(&f, &repo_path(), None);
    assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec!["m".into()], gitmodules: false });
}

#[test]
fn fake_gitlink_in_base_tree_blocks_and_is_merged_without_duplicates() {
    let mut a = base_answers(&link("m"));
    a.retain(|(n, _)| *n != "ls-tree-base");
    a.push(("ls-tree-base", ok(&format!("{}{}", link("m"), link("n")))));
    let c = check_start_with(&Fake::new(a), &repo_path(), Some("main"));
    assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec!["m".into(), "n".into()], gitmodules: false });
}

#[test]
fn fake_base_tree_read_failure_is_ignored() {
    let mut a = base_answers("");
    a.retain(|(n, _)| *n != "ls-tree-base");
    a.push(("ls-tree-base", fail()));
    assert_eq!(check_start_with(&Fake::new(a), &repo_path(), Some("main")).verdict, Verdict::Clear);
}

#[test]
fn fake_gitmodules_in_index_head_or_base_each_blocks() {
    for which in ["ls-files-gm", "ls-tree-gm", "cat-file"] {
        let mut a = base_answers("");
        a.retain(|(n, _)| *n != which);
        a.push((which, ok(if which == "cat-file" { "" } else { "100644 abc 0\t.gitmodules\0" })));
        let c = check_start_with(&Fake::new(a), &repo_path(), Some("main"));
        assert_eq!(c.verdict, Verdict::Submodule { gitlinks: vec![], gitmodules: true }, "{which}");
    }
}

#[test]
fn fake_cat_file_on_base_is_not_asked_without_a_base() {
    let f = Fake::new(base_answers(""));
    check_start_with(&f, &repo_path(), None);
    let n = f.names();
    assert!(!n.contains(&"cat-file".to_string()) && !n.contains(&"ls-tree-base".to_string()), "{n:?}");
}

#[test]
fn fake_index_read_failure_and_adapter_error_are_closed() {
    let f = Fake::new(vec![("ls-files-all", fail())]);
    let c = check_start_with(&f, &repo_path(), None);
    assert_eq!(c.verdict, Verdict::Unreadable { what: "git ls-files".into() });
    assert_eq!(c.baseline, None);
    assert_eq!(start_message(&repo_path(), &c.verdict).unwrap(), "repo 의 색인을 읽지 못했습니다(git ls-files 실패)");

    let f = Fake::new(vec![("ls-files-all", Err(PortError::new(PortErrorKind::Unavailable, "git.exe 없음")))]);
    assert!(matches!(check_start_with(&f, &repo_path(), None).verdict, Verdict::Unreadable { .. }));
    assert!(matches!(check_unit_with(&f, &repo_path(), &Snapshot::default()), Verdict::Unreadable { .. }));
}

#[test]
fn fake_base_name_starting_with_dash_is_closed_without_calling_git() {
    let f = Fake::new(base_answers(""));
    let c = check_start_with(&f, &repo_path(), Some("--output=x"));
    assert!(matches!(c.verdict, Verdict::Unreadable { .. }));
    assert!(f.names().is_empty());
}

#[test]
fn fake_more_than_three_gitlinks_are_summarised() {
    let idx: String = ["a", "b", "c", "d", "e"].iter().map(|p| link(p)).collect();
    let c = check_start_with(&Fake::new(base_answers(&idx)), &repo_path(), None);
    assert_eq!(c.verdict.why().unwrap(), "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b, c 외 2개))이 있어");
}

#[test]
fn fake_unit_check_compares_against_the_baseline() {
    let base = Snapshot { gitlinks: vec!["old".into()], gitmodules: false };
    // 기준에 있던 것만 → 통과
    let f = Fake::new(base_answers(&link("old")));
    assert_eq!(check_unit_with(&f, &repo_path(), &base), Verdict::Clear);
    // 기준에 없던 것이 하나라도 → 멈춤, 까닭에는 지금의 모두
    let f = Fake::new(base_answers(&format!("{}{}", link("old"), link("new"))));
    assert_eq!(
        check_unit_with(&f, &repo_path(), &base),
        Verdict::Submodule { gitlinks: vec!["old".into(), "new".into()], gitmodules: false }
    );
    // 기준에 있던 것이 사라진 것은 멈추지 않는다
    let f = Fake::new(base_answers(""));
    assert_eq!(check_unit_with(&f, &repo_path(), &base), Verdict::Clear);
    // 기준에 .gitmodules 가 있었으면 새 것이 아니고, 없었는데 생기면 멈춘다
    let mut a = base_answers("");
    a.retain(|(n, _)| *n != "ls-files-gm");
    a.push(("ls-files-gm", ok("100644 abc 0\t.gitmodules\0")));
    let with_gm = Snapshot { gitlinks: vec![], gitmodules: true };
    assert_eq!(check_unit_with(&Fake::new(a.clone()), &repo_path(), &with_gm), Verdict::Clear);
    assert!(matches!(check_unit_with(&Fake::new(a), &repo_path(), &Snapshot::default()), Verdict::Submodule { gitmodules: true, .. }));
}

#[test]
fn fake_unit_stop_text_and_clear_is_none() {
    let v = Verdict::Submodule { gitlinks: vec!["x".into()], gitmodules: false };
    let s = unit_stop(7, Path::new("C:\\work\\my repo\\"), &v).unwrap();
    assert_eq!(
        s.reason,
        "보안: 단위 7 의 my repo 의 작업 폴더에 하위 모듈(색인에 중첩 git 저장소(x))이 있어 그 자리에서 git(모델·판정·기록)을 돌리지 않습니다. 사람이 확인하세요."
    );
    let u = unit_stop(2, &repo_path(), &Verdict::Unreadable { what: "git ls-files".into() }).unwrap();
    assert!(u.reason.starts_with("보안: 단위 2 의 repo 의 색인을 읽지 못해(git ls-files 실패) 그 자리에서"));
    assert!(unit_stop(1, &repo_path(), &Verdict::Clear).is_none());
    assert!(Verdict::Clear.is_clear() && !v.is_clear());
    assert!(Verdict::Clear.why().is_none());
}

#[test]
fn repo_name_handles_both_separators_and_trailing_ones() {
    assert_eq!(repo_name(Path::new("C:\\a\\b")), "b");
    assert_eq!(repo_name(Path::new("C:/a/b/")), "b");
    assert_eq!(repo_name(Path::new("C:\\a\\b\\")), "b");
    assert_eq!(repo_name(Path::new("solo")), "solo");
}

#[test]
fn spy_every_git_call_goes_through_the_port_with_the_repo_argument() {
    let repo = repo_path();
    let f = Fake::new(base_answers(""));
    check_start_with(&f, &repo, Some("main"));
    check_unit_with(&f, &repo, &Snapshot::default());
    let calls = f.calls.lock().unwrap();
    assert!(calls.len() >= 8, "{} 번 불렀다", calls.len());
    for (r, args) in calls.iter() {
        assert_eq!(r, &repo, "{args:?}");
        assert_ne!(kind_of(args), "other", "{args:?}");
        // 읽기 명령만 부른다: 색인·트리를 읽고 파일 존재를 묻는다
        assert!(matches!(args.iter().find(|a| !a.starts_with('-') && *a != "core.quotePath=false").map(String::as_str), Some("ls-files" | "ls-tree" | "cat-file")), "{args:?}");
    }
    // 색인 읽기는 따옴표 끄기와 -z 를 건다
    let idx = calls.iter().find(|(_, a)| kind_of(a) == "ls-files-all").unwrap();
    assert_eq!(idx.1, ["-c", "core.quotePath=false", "ls-files", "-s", "-z"]);
    let tree = calls.iter().find(|(_, a)| kind_of(a) == "ls-tree-base").unwrap();
    assert_eq!(tree.1, ["-c", "core.quotePath=false", "ls-tree", "-r", "-z", "main"]);
}

#[test]
fn real_git_gitmodules_only_in_the_base_branch_blocks_only_with_that_base() {
    let e = Env::new();
    let r = e.repo("host");
    sh(&r, &["switch", "-q", "-c", "side"]);
    std::fs::write(r.join(".gitmodules"), "x").unwrap();
    sh(&r, &["add", ".gitmodules"]);
    sh(&r, &["commit", "-q", "-m", "gm"]);
    sh(&r, &["switch", "-q", "main"]);
    assert!(!r.join(".gitmodules").exists());
    assert_eq!(check_start(&r, None).verdict, Verdict::Clear);
    assert_eq!(check_start(&r, Some("main")).verdict, Verdict::Clear);
    assert_eq!(check_start(&r, Some("side")).verdict, Verdict::Submodule { gitlinks: vec![], gitmodules: true });
}
