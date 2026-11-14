//! 따로 작업 트리 출발 시험(`engine::worktree_run`). `real-git` 표시가 붙은 것은 이 PC 의 진짜 git.exe 와 새 임시 폴더로 돈다.
//! 사용자의 ~/.gitconfig·실제 저장소·네트워크는 건드리지 않는다(git 은 보호 환경 `ProcessGit`).

use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::adapters::worktree::add_existing;
use claude_board_autopilot::engine::worktree_run::{
    remap_to_worktree, rollback_worktree, start_worktree, worktree_files_of, WorktreeFile,
};
use claude_board_autopilot::ports::{Git, PortErrorKind};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// ── 도우미(다른 시험과 같은 꼴) ───────────────────────

struct Env {
    _tmp: tempfile::TempDir,
    /// `C:/…` 꼴(구분자 `/`, 확장 길이 접두 없음)
    root: String,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let canon = std::fs::canonicalize(tmp.path()).unwrap();
        let root = canon.to_string_lossy().replace('\\', "/").trim_start_matches("//?/").to_string();
        std::fs::create_dir_all(format!("{root}/home")).unwrap();
        Env { _tmp: tmp, root }
    }
    fn p(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }
    fn home(&self) -> String {
        self.p("home")
    }
}

fn g(dir: &Path, args: &[&str]) -> String {
    let out = ProcessGit::new().run(dir, args).expect("git.exe 가 있어야 한다");
    assert_eq!(out.code, 0, "git {args:?} 실패: {}", out.stderr);
    out.stdout
}

/// 저장소 + 첫 커밋(가지 main), 함께 두고 볼 파일을 같이 놓는다:
/// .gitignore(무시 파일)·loose.txt(추적 안 하는 파일)·secrets.env(무시 파일)·data/db.sqlite(무시 폴더)
fn env_repo() -> (Env, String) {
    let env = Env::new();
    let repo = env.p("repo");
    let d = init_repo(&repo);
    std::fs::write(d.join(".gitignore"), "*.env\ndata/\n").unwrap();
    std::fs::write(d.join("loose.txt"), "나만 본다").unwrap();
    std::fs::write(d.join("secrets.env"), "TOKEN=1").unwrap();
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::write(d.join("data/db.sqlite"), "db").unwrap();
    g(&d, &["add", ".gitignore"]);
    g(&d, &["commit", "-q", "-m", "무시 규칙"]);
    (env, repo)
}

fn init_repo(dir: &str) -> PathBuf {
    let d = PathBuf::from(dir);
    std::fs::create_dir_all(&d).unwrap();
    g(&d, &["init", "-q", "-b", "main"]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        g(&d, &["config", k, v]);
    }
    std::fs::write(d.join("a.txt"), "a").unwrap();
    g(&d, &["add", "a.txt"]);
    g(&d, &["commit", "-q", "-m", "base"]);
    d
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase().replace('\\', "/") == b.to_lowercase().replace('\\', "/")
}

/// 시작과 되돌리기에 함께 쓸 파일 목록
fn sample_files() -> Vec<WorktreeFile> {
    vec![
        WorktreeFile { path: "secrets.env".to_string(), link: true },
        WorktreeFile { path: "data/db.sqlite".to_string(), link: false },
        WorktreeFile { path: "loose.txt".to_string(), link: false },
    ]
}

// ── worktree_files 해석(순수) ────────────────────────

#[test]
fn worktree_files_문자열과_객체를_항목으로_갈라_낸다() {
    let frozen = json!({
        "worktree_files": [".env", {"path": "data/db.sqlite", "link": true}, {"path": "no-link"}, 42, null]
    });
    let files = worktree_files_of(&frozen);
    assert_eq!(files.len(), 3, "문자열·객체만 항목이다");
    assert_eq!(files[0].path, ".env");
    assert!(!files[0].link);
    assert_eq!(files[1].path, "data/db.sqlite");
    assert!(files[1].link);
    assert_eq!(files[2].path, "no-link");
    assert!(!files[2].link, "link 칸이 없으면 복사다");
    assert!(worktree_files_of(&json!({})).is_empty(), "칸이 없으면 비었다");
    assert!(worktree_files_of(&json!({"worktree_files": "문자열"})).is_empty(), "목록이 아니면 비었다");
}

// ── 출발(실 git) ─────────────────────────────────────

/// 정상 출발: 작업 가지·작업 트리·함께 둘 파일·기록·worktree 값. 끝에 되돌리기(만든 가지는 지운다)
#[test]
fn start_worktree_새_가지와_작업_트리를_만들고_함께_둘_파일을_옮긴다() {
    let (env, repo) = env_repo();
    let main_head = head(&repo, "main");
    let files = sample_files();
    let started = start_worktree(
        &ProcessGit::new(),
        &repo,
        "run1",
        "main",
        false,
        &files,
        &PathBuf::from(env.p("wts")),
        &env.home(),
    )
    .expect("출발에 성공해야 한다");
    let wt = started.path.clone();
    assert!(same(&wt, &env.p("wts/repo")), "{wt}");
    // 가지 기록: 출발 가지·작업 가지·원래 걸려 있던 가지
    let record = &started.record;
    assert!(same(&record.repo, &repo));
    assert_eq!(record.base_branch, "main");
    assert_eq!(record.base_head, main_head);
    assert_eq!(record.work, "autopilot/run1");
    assert_eq!(record.original.as_deref(), Some("main"), "원래 걸려 있던 가지");
    assert_eq!(record.original_head, main_head);
    assert_eq!(record.dirty_start, vec!["?? loose.txt".to_string()], "무시 파일·추적 파일이 변하면 안 된다");
    assert_eq!(started.start_heads.get("main").map(String::as_str), Some(main_head.as_str()));
    assert!(!started.start_heads.contains_key("autopilot/run1"), "작업 가지는 만든 뒤 머리에 없다");
    // worktree 값(원본과 같은 꼴)
    assert_eq!(started.worktree["mode"], "worktree");
    assert_eq!(started.worktree["branch"], "autopilot/run1");
    assert_eq!(started.worktree["base"], main_head.as_str());
    assert!(started.worktree["reason"].as_str().unwrap_or_default().contains("작업 트리"));
    // 원본 저장소는 그대로 main 이고 작업 가지가 걸려 있지 않다
    assert_eq!(g(Path::new(&repo), &["branch", "--show-current"]).trim(), "main");
    // 작업 트리는 작업 가지에 걸려 있다
    assert_eq!(g(Path::new(&wt), &["branch", "--show-current"]).trim(), "autopilot/run1");
    // 함께 둔 파일이 복사됐다(원본은 남는다) — 바로 가기는 읽기 전용
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    assert_eq!(read(&env.p("wts/repo/secrets.env")), "TOKEN=1");
    assert!(std::fs::metadata(env.p("wts/repo/secrets.env")).unwrap().permissions().readonly(), "바로 가기는 읽기 전용");
    assert_eq!(read(&env.p("wts/repo/data/db.sqlite")), "db");
    assert_eq!(read(&env.p("wts/repo/loose.txt")), "나만 본다");
    assert!(!std::fs::metadata(env.p("wts/repo/loose.txt")).unwrap().permissions().readonly(), "복사는 쓸 수 있다");
    assert!(std::fs::metadata(env.p("repo/secrets.env")).is_ok(), "원본 파일이 남아야 한다(복사다)");
    // 되돌리기: 작업 트리·만든 가지·부모 폴더 자리까지 치운다
    rollback_worktree(&ProcessGit::new(), &repo, &wt, "autopilot/run1", false);
    assert!(!std::fs::symlink_metadata(&wt).is_ok(), "작업 트리가 치워진다");
    assert!(!std::fs::symlink_metadata(env.p("wts")).is_ok(), "빈 부모 폴더도 치워진다");
    let gone = ProcessGit::new().run(Path::new(&repo), &["rev-parse", "--verify", "-q", "refs/heads/autopilot/run1"]).unwrap();
    assert_ne!(gone.code, 0, "만든 가지도 지워진다");
}

fn head(repo: &str, what: &str) -> String {
    let out = ProcessGit::new().run(Path::new(&repo), &["rev-parse", "-q", "--verify", what]).unwrap();
    assert_eq!(out.code, 0, "git rev-parse {what} 실패: {}", out.stderr);
    out.stdout.trim().to_string()
}

/// 이어받은 출발(⑪-2): 가지를 새로 만들지 않고, 되돌리기에 가지를 남긴다
#[test]
fn start_worktree_이어받은_가지는_만들지_않고_쓰고_되돌기에는_남는다() {
    let (env, repo) = env_repo();
    g(Path::new(&repo), &["branch", "autopilot/run1"]);
    let main_head = head(&repo, "main");
    let started = start_worktree(
        &ProcessGit::new(),
        &repo,
        "run1",
        "main",
        true,
        &[],
        &PathBuf::from(env.p("wts")),
        &env.home(),
    )
    .expect("이어받은 출발에 성공해야 한다");
    assert_eq!(started.record.work, "autopilot/run1");
    assert_eq!(started.worktree["branch"], "autopilot/run1");
    assert_eq!(started.record.base_head, main_head, "이어받기 전 가지의 머리");
    assert_eq!(started.record.original.as_deref(), Some("main"));
    assert_eq!(started.record.original_head, main_head);
    assert!(
        !started.start_heads.contains_key("autopilot/run1"),
        "이어받은 가지는 출발 전 머리 목록에서 빠진다(움직임이 아니다)"
    );
    assert!(started.start_heads.contains_key("main"));
    rollback_worktree(&ProcessGit::new(), &repo, &started.path.clone(), "autopilot/run1", true);
    assert!(std::fs::symlink_metadata(&started.path).is_err(), "작업 트리는 치워진다");
    let kept = ProcessGit::new().run(Path::new(&repo), &["rev-parse", "--verify", "-q", "refs/heads/autopilot/run1"]).unwrap();
    assert_eq!(kept.code, 0, "이어받은 가지는 남는다");
}

/// 출발 가지가 없으면 거부하고 만들던 자리를 치운다(부모 자리는 남는다 — 원본과 같다)
#[test]
fn start_worktree_가지_만들기가_실패하면_자리를_치운다() {
    let (env, repo) = env_repo();
    let why = start_worktree(
        &ProcessGit::new(),
        &repo,
        "run1",
        "absent",
        false,
        &[],
        &PathBuf::from(env.p("wts")),
        &env.home(),
    )
    .expect_err("없는 출발 가지는 거부다");
    assert!(why.contains("만들지 못했습니다"), "{why}");
    assert!(why.contains("absent"), "{why}");
    assert!(!std::fs::symlink_metadata(env.p("wts")).is_ok(), "만들던 자리를 치운다");
    assert!(std::fs::symlink_metadata(env.p("home/.claude-board/worktrees")).is_ok(), "보드 자리는 남는다");
    let gone = ProcessGit::new().run(Path::new(&repo), &["rev-parse", "--verify", "-q", "refs/heads/autopilot/run1"]).unwrap();
    assert_ne!(gone.code, 0, "작업 가지를 남기지 않는다");
}

/// git 설정 확인(작업 트리에서 다시 보기)이 막히면 출발을 거부하고 되돌린다
#[test]
fn start_worktree_설정_확인에_막히면_되돌린다() {
    let (env, repo) = env_repo();
    let alarm = env.p("repo/.git/alarm.inc");
    std::fs::write(&alarm, "[user]\n\tname = t\n").unwrap();
    g(Path::new(&repo), &["config", "include.path", &alarm]);
    let why = start_worktree(
        &ProcessGit::new(),
        &repo,
        "run1",
        "main",
        false,
        &[],
        &PathBuf::from(env.p("wts")),
        &env.home(),
    )
    .expect_err("저장소 안을 가리키는 include 는 막혀야 한다");
    assert!(why.contains("보안"), "{why}");
    assert!(why.contains("지웠습니다"), "{why}");
    assert!(!std::fs::symlink_metadata(env.p("wts/repo")).is_ok(), "작업 트리를 지웁니다");
    let gone = ProcessGit::new().run(Path::new(&repo), &["rev-parse", "--verify", "-q", "refs/heads/autopilot/run1"]).unwrap();
    assert_ne!(gone.code, 0, "막힌 출발의 가지도 지운다");
}

/// 함께 둘 파일 가운데 작업 트리의 git 자리를 건드리는 것은 잘못된 입력이라 터뜨리고 되돌린다(Windows 보완)
#[test]
fn start_worktree_git_자리를_건드리는_파일은_거부하고_되돌린다() {
    let (env, repo) = env_repo();
    let files = [WorktreeFile { path: ".git".to_string(), link: false }];
    let why = start_worktree(
        &ProcessGit::new(),
        &repo,
        "run1",
        "main",
        false,
        &files,
        &PathBuf::from(env.p("wts")),
        &env.home(),
    )
    .expect_err(".git 을 덮어쓰는 worktree_files 는 거부다");
    assert!(why.contains("git 자리"), "{why}");
    assert!(!std::fs::symlink_metadata(env.p("wts/repo")).is_ok(), "작업 트리를 되돌린다");
    let gone = ProcessGit::new().run(Path::new(&repo), &["rev-parse", "--verify", "-q", "refs/heads/autopilot/run1"]).unwrap();
    assert_ne!(gone.code, 0, "되돌린 출발의 가지도 지운다");
}

/// 잘못된 상대 경로(빈 길·`..` 성분·절대 꼴)와 원본이 없는 파일은 원본과 같이 넘긴다
#[test]
fn start_worktree_잘못된_파일_항목은_넘기고_진행한다() {
    let (env, repo) = env_repo();
    std::fs::create_dir_all(env.p("victim")).unwrap();
    let victim = env.p("victim/keep.txt");
    std::fs::write(&victim, "남의 파일").unwrap();
    let files = vec![
        WorktreeFile { path: String::new(), link: false },
        WorktreeFile { path: "a/../../outside.txt".to_string(), link: false },
        WorktreeFile { path: "/abs.txt".to_string(), link: false },
        WorktreeFile { path: "C:".to_string(), link: false },
        WorktreeFile { path: "nope.txt".to_string(), link: false },
        // 드라이브 글자가 붙은 절대 경로 — `join` 이 앞 경로를 바꿔 작업 트리 밖의 파일을 지우던 구멍
        WorktreeFile { path: victim.clone(), link: false },
        WorktreeFile { path: victim.replace('/', "\\"), link: true },
    ];
    let started = start_worktree(
        &ProcessGit::new(),
        &repo,
        "run1",
        "main",
        false,
        &files,
        &PathBuf::from(env.p("wts")),
        &env.home(),
    )
    .expect("잘못된 항목은 넘기고 진행한다");
    assert!(std::fs::symlink_metadata(&started.path).is_ok(), "작업 트리는 만들어 진다");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "남의 파일", "작업 트리 밖의 파일은 그대로다");
    rollback_worktree(&ProcessGit::new(), &repo, &started.path, "autopilot/run1", false);
}

// ── add_existing(실 git) ─────────────────────────────

#[test]
fn add_existing_있는_가지로_작업_트리를_만든다() {
    let (env, repo) = env_repo();
    g(Path::new(&repo), &["branch", "autopilot/run1"]);
    let added = add_existing(&ProcessGit::new(), Path::new(&repo), &env.p("wt"), "autopilot/run1")
        .expect("있는 가지라 만든다");
    assert!(same(&added.path, &env.p("wt")), "{}", added.path);
    let in_wt = ProcessGit::new().run(Path::new(&env.p("wt")), &["branch", "--show-current"]).unwrap();
    assert_eq!(in_wt.stdout.trim(), "autopilot/run1");
    rollback_worktree(&ProcessGit::new(), &repo, &added.path, "autopilot/run1", false);
}

#[test]
fn add_existing_없는_가지는_거부한다() {
    let (env, repo) = env_repo();
    let why = add_existing(&ProcessGit::new(), Path::new(&repo), &env.p("wt"), "autopilot/none")
        .expect_err("없는 가지는 거부다");
    assert_eq!(why.kind, PortErrorKind::NotFound, "{}", why.message);
    assert!(why.message.contains("이어받을 가지가 없다"), "{}", why.message);
    assert!(!std::fs::symlink_metadata(env.p("wt")).is_ok(), "거부된 자리는 남지 않는다");
}

// ── 고정 설정 고치기(순수) ───────────────────────────

/// 자신(frozen)을 만들어 고쳐 본다. `units` 는 cwd 접두사·경계 바늘을 함께 본다
fn frozen_units(orig: &str) -> Value {
    json!({
        "write_roots": [orig, format!("{orig}/하위"), "C:/other"],
        "context": {"task_prompt": format!("{orig} 저장소에서만 고친다")},
        "units": [
            {"cwd": format!("{orig}\\단위"), "check": format!("git -C {orig} 상태를 본다"), "prompt": ""},
            {"cwd": "C:/other", "check": format!("{orig}toy 는 남긴다"), "prompt": format!("{orig}2 도 남긴다 toy")}
        ]
    })
}

#[test]
fn remap_원본_저장소_자리를_작업_트리로_옮긴다() {
    let (env, repo) = env_repo();
    let wt = env.p("wts/repo");
    let mut frozen = frozen_units(&repo);
    remap_to_worktree(&mut frozen, &repo, &wt);
    assert_eq!(frozen["write_roots"][0], wt.as_str(), "원본 자신");
    assert_eq!(frozen["write_roots"][1], format!("{wt}/하위"), "원본 아래 자리");
    assert_eq!(frozen["write_roots"][2], "C:/other", "남의 자리는 그대로");
    // context 는 그대로다(원본과 같다)
    assert_eq!(frozen["context"]["task_prompt"], format!("{repo} 저장소에서만 고친다"));
    // cwd 는 접두사로, `\` 꼴도 옮긴다
    assert_eq!(frozen["units"][0]["cwd"], format!("{wt}/단위"));
    assert_eq!(frozen["units"][1]["cwd"], "C:/other");
    // check·prompt 는 경계로만 — 뒤에 글자가 붙으면 바꾸지 않는다
    assert_eq!(frozen["units"][0]["check"], format!("git -C {wt} 상태를 본다"));
    assert_eq!(frozen["units"][1]["check"], format!("{repo}toy 는 남긴다"), "뒤에 붙은 toy 사이는 바꾸지 않는다");
    assert_eq!(frozen["units"][1]["prompt"], format!("{repo}2 도 남긴다 toy"), "숫자가 붙은 것도 바꾸지 않는다");
}

/// 글의 끝에 닿은 바늘도 경계다(뒤에 아무 글도 없다). 빈 바늘이면 아무것도 바꾸지 않는다
#[test]
fn remap_글_끝과_빈_바늘을_본다() {
    let (env, repo) = env_repo();
    let wt = env.p("wts/repo");
    let mut frozen = json!({
        "write_roots": [],
        "units": [{"prompt": format!("끝에 닿은 {repo}"), "action": format!("[{repo}] 닫은 괄호"), "precheck": format!("{repo}\n뒤에 줄바꿈")}]
    });
    remap_to_worktree(&mut frozen, &repo, &wt);
    assert_eq!(frozen["units"][0]["prompt"], format!("끝에 닿은 {wt}"), "글의 끝도 경계다");
    assert_eq!(frozen["units"][0]["action"], format!("[{repo}] 닫은 괄호"), "경계 글이 아닌 ) 는 바꾸지 않는다");
    assert_eq!(frozen["units"][0]["precheck"], format!("{wt}\n뒤에 줄바꿈"), "줄바꿈 앞도 경계다");

    let mut intact = json!({"write_roots": ["C:/x"], "units": [{"cwd": "C:/x"}]});
    let untouched = intact.clone();
    remap_to_worktree(&mut intact, "", &wt);
    assert_eq!(intact, untouched, "빈 바늘이면 바꾸지 않는다");
}



