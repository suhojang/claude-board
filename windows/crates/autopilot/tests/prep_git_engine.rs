//! 출발 전 git 자리 확인 어댑터 시험(`engine::prep_git`). `real-git` 표시가 붙은 것은 이 PC 의 진짜 git.exe 와 임시 폴더로 돈다.
//! 홈 폴더는 임시 폴더 안의 가짜 홈(`<뿌리>/home`)을 넘긴다 — 규칙의 "임시 폴더" 자리가 가짜 홈 아래라 시험 저장소가 막히지 않는다.
//! 사용자의 ~/.gitconfig·실제 저장소·네트워크는 건드리지 않는다(git 은 보호 환경 `ProcessGit`).

use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::engine::prep_git::{block_message, file_id, format_file_id, parse_config_list, run_prep_git, RealPrepGitWorld};
use claude_board_autopilot::ports::{Git, GitOutput, PortError, PortErrorKind, PortResult};
use claude_board_autopilot::rules::board_git::GitRecord;
use claude_board_autopilot::rules::prep_git::{prep_git_check, ConfigEntry, GitEntry, PrepGit, PrepGitWorld};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ── 도우미 ───────────────────────────────────────────

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("work")).unwrap();
        Env { _tmp: tmp, root }
    }
    fn home(&self) -> String {
        self.root.join("home").to_string_lossy().into_owned()
    }
    fn work(&self, name: &str) -> PathBuf {
        self.root.join("work").join(name)
    }
    fn outside(&self, name: &str) -> PathBuf {
        let d = self.root.join("outside");
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }
}

fn g(dir: &Path, args: &[&str]) -> String {
    let out = ProcessGit::new().run(dir, args).expect("git.exe 가 있어야 한다");
    assert_eq!(out.code, 0, "git {args:?} 실패: {}", out.stderr);
    out.stdout
}

/// 저장소 + 첫 커밋
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    g(dir, &["init", "-q", "-b", "main"]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        g(dir, &["config", k, v]);
    }
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    g(dir, &["add", "a.txt"]);
    g(dir, &["commit", "-q", "-m", "base"]);
}

fn check(e: &Env, p: &str) -> PrepGit {
    run_prep_git(&ProcessGit::new(), p, &BTreeMap::new(), &e.home())
}

fn check_path(e: &Env, p: &Path) -> PrepGit {
    check(e, &p.to_string_lossy())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_start_matches("//?/").to_string()
}

fn passes(r: &PrepGit) {
    assert!(r.top.is_some() && r.block.is_none(), "통과해야 한다: {r:?}");
}

fn blocked_with(r: &PrepGit, needle: &str) {
    let b = r.block.as_deref().unwrap_or_else(|| panic!("막혀야 한다: {r:?}"));
    assert!(b.contains(needle), "사유에 {needle:?} 가 있어야 한다: {b}");
}

fn config_path(repo: &Path) -> PathBuf {
    repo.join(".git").join("config")
}

fn append(path: &Path, text: &str) {
    let mut cur = std::fs::read_to_string(path).unwrap_or_default();
    cur.push_str(text);
    std::fs::write(path, cur).unwrap();
}

// ── (1) 보통 저장소 · (7) 저장소 아님 ─────────────────

#[test]
fn real_git_보통_저장소는_통과하고_경로와_git_폴더를_돌려준다() {
    let e = Env::new();
    let repo = e.work("plain");
    init_repo(&repo);
    let r = check_path(&e, &repo);
    passes(&r);
    assert_eq!(r.top.as_deref(), Some(s(&repo).as_str()));
    assert_eq!(r.git_dir.as_deref(), Some(format!("{}/.git", s(&repo)).as_str()));
    assert_eq!(r.common_dir, r.git_dir);
    assert!(r.pin().iter().any(|x| x.starts_with("GIT_DIR=")));
}

#[test]
fn real_git_저장소_안쪽_폴더도_뿌리를_찾는다() {
    let e = Env::new();
    let repo = e.work("plain");
    init_repo(&repo);
    std::fs::create_dir_all(repo.join("sub/deeper")).unwrap();
    let r = check_path(&e, &repo.join("sub/deeper"));
    passes(&r);
    assert_eq!(r.top.as_deref(), Some(s(&repo).as_str()));
}

#[test]
fn real_git_저장소가_아닌_폴더와_없는_폴더() {
    let e = Env::new();
    let plain = e.work("not-a-repo");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(check_path(&e, &plain), PrepGit::default());
    assert_eq!(check_path(&e, &e.work("missing")), PrepGit::default());
}

// ── (2) 중첩 git ─────────────────────────────────────

#[test]
fn real_git_바깥_저장소_안에_든_중첩_git_은_막고_바깥은_통과한다() {
    let e = Env::new();
    let outer = e.work("outer");
    init_repo(&outer);
    let inner = outer.join("inner");
    init_repo(&inner);
    blocked_with(&check_path(&e, &inner), "다른 저장소");
    // 중첩은 untracked 폴더일 뿐 gitlink 가 아니다
    passes(&check_path(&e, &outer));
}

// ── (3) .git 파일 바로 가기 ──────────────────────────

#[test]
fn real_git_따로_작업_트리는_통과하고_공통_git_폴더를_돌려준다() {
    let e = Env::new();
    let main = e.work("main");
    init_repo(&main);
    let wt = e.work("wt");
    g(&main, &["worktree", "add", "-q", &s(&wt), "-b", "wt"]);
    let r = check_path(&e, &wt);
    passes(&r);
    assert_eq!(r.common_dir.as_deref(), Some(format!("{}/.git", s(&main)).as_str()));
    assert!(r.git_dir.as_deref().unwrap().contains("/.git/worktrees/"));
    // 따로 작업 트리 폴더는 그 저장소의 작업 트리 안이 아니다 — 본 저장소도 그대로 통과
    passes(&check_path(&e, &main));
}

#[test]
fn real_git_밖을_가리키는_git_파일은_막는다() {
    let e = Env::new();
    // 떨어진 git 폴더(--separate-git-dir 꼴)
    let sep = e.outside("sep.git");
    let dir = e.work("separate");
    std::fs::create_dir_all(&dir).unwrap();
    g(&dir, &["init", "-q", "--separate-git-dir", &sep.to_string_lossy(), "."]);
    assert!(dir.join(".git").is_file());
    blocked_with(&check_path(&e, &dir), "떨어진 git 폴더");

    // 없는 자리
    let ghost = e.work("ghost");
    std::fs::create_dir_all(&ghost).unwrap();
    std::fs::write(ghost.join(".git"), format!("gitdir: {}\n", s(&e.outside("nope.git")))).unwrap();
    blocked_with(&check_path(&e, &ghost), "가 없어");

    // 작업 트리 안(상대 경로)
    let inside = e.work("inside");
    std::fs::create_dir_all(inside.join("hid/.git")).unwrap();
    std::fs::write(inside.join("hid/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(inside.join(".git"), "gitdir: hid/.git\n").unwrap();
    blocked_with(&check_path(&e, &inside), "그 작업 트리 안");
}

#[test]
fn real_git_되짚는_자리가_다른_따로_작업_트리는_막는다() {
    let e = Env::new();
    let main = e.work("main");
    init_repo(&main);
    let wt = e.work("wt");
    g(&main, &["worktree", "add", "-q", &s(&wt), "-b", "wt"]);
    let admin = main.join(".git/worktrees/wt");
    std::fs::write(admin.join("gitdir"), format!("{}\n", s(&e.outside("elsewhere/.git")))).unwrap();
    blocked_with(&check_path(&e, &wt), "되짚는 자리");
    // commondir 가 다른 자리
    std::fs::write(admin.join("gitdir"), format!("{}/.git\n", s(&wt))).unwrap();
    std::fs::write(admin.join("commondir"), format!("{}\n", s(&e.outside("c.git")))).unwrap();
    blocked_with(&check_path(&e, &wt), "commondir");
}

#[test]
fn real_git_git_파일_내용이_이상하면_막는다() {
    let e = Env::new();
    for (name, body) in [("garbage", b"hello world".to_vec()), ("badutf8", b"gitdir: \xff\xfe".to_vec()), ("empty", Vec::new())] {
        let d = e.work(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".git"), body).unwrap();
        blocked_with(&check_path(&e, &d), ".git 파일을 읽지 못해");
    }
    // 읽기 상한(64KiB)을 넘는 파일
    let big = e.work("big");
    std::fs::create_dir_all(&big).unwrap();
    let mut body = b"gitdir: ".to_vec();
    body.extend(std::iter::repeat_n(b'x', 70_000));
    std::fs::write(big.join(".git"), body).unwrap();
    blocked_with(&check_path(&e, &big), ".git 파일을 읽지 못해");
}

// ── (4) commondir ────────────────────────────────────

#[test]
fn real_git_git_폴더의_commondir_는_막는다() {
    let e = Env::new();
    let repo = e.work("cd");
    init_repo(&repo);
    std::fs::write(repo.join(".git/commondir"), "../../elsewhere\n").unwrap();
    blocked_with(&check_path(&e, &repo), "commondir");
}

// ── .git 꼴 ──────────────────────────────────────────

#[test]
fn real_git_head_가_폴더이면_막고_head_없는_git_폴더는_지나친다() {
    let e = Env::new();
    let odd = e.work("odd");
    std::fs::create_dir_all(odd.join(".git/HEAD")).unwrap();
    blocked_with(&check_path(&e, &odd), "HEAD 가 보통 파일이 아니라");
    // HEAD 가 없는 .git 폴더는 git 이 저장소로 보지 않으니 지나친다 → 위에도 없으면 저장소 아님
    let empty = e.work("emptygit");
    std::fs::create_dir_all(empty.join(".git")).unwrap();
    assert_eq!(check_path(&e, &empty), PrepGit::default());
}

#[cfg(windows)]
#[test]
fn real_git_git_이_정션이면_막고_정션_자리는_가리키는_쪽과_다르다() {
    let e = Env::new();
    let real = e.outside("realgit");
    std::fs::create_dir_all(&real).unwrap();
    let dir = e.work("junc");
    std::fs::create_dir_all(&dir).unwrap();
    let st = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J", &dir.join(".git").to_string_lossy(), &real.to_string_lossy()])
        .output()
        .unwrap();
    assert!(st.status.success(), "정션을 만들지 못했다: {}", String::from_utf8_lossy(&st.stdout));
    blocked_with(&check_path(&e, &dir), ".git 이 바로 가기라");
    // 바로 가기를 따라가지 않는다: 정션 자체의 자리는 가리키는 폴더의 자리와 다르다
    let a = file_id(&dir.join(".git").to_string_lossy()).unwrap();
    let b = file_id(&real.to_string_lossy()).unwrap();
    assert_ne!(a, b);
}

// ── (5) git 설정 ─────────────────────────────────────

#[test]
fn real_git_설정에_명령이_있으면_막는다() {
    let e = Env::new();
    let cases: [(&str, &str, &str); 3] = [
        ("fsm", "[core]\n\tfsmonitor = C:/evil/hook.cmd\n", "core.fsmonitor"),
        ("flt", "[filter \"x\"]\n\tclean = evil.exe %f\n", "filter 명령"),
        ("flp", "[filter \"x\"]\n\tprocess = evil.exe\n", "filter 명령"),
    ];
    for (name, body, why) in cases {
        let repo = e.work(name);
        init_repo(&repo);
        append(&config_path(&repo), body);
        blocked_with(&check_path(&e, &repo), why);
    }
}

#[test]
fn real_git_설정의_불리언_fsmonitor_와_값_없는_filter_와_lfs_표준은_통과한다() {
    let e = Env::new();
    let repo = e.work("benign");
    init_repo(&repo);
    // 값 없는 키(`키\0` 꼴)도 읽혀야 한다
    append(
        &config_path(&repo),
        "[core]\n\tfsmonitor = false\n[filter \"lfs\"]\n\tclean = git-lfs clean -- %f\n\tsmudge = git-lfs smudge -- %f\n\tprocess = git-lfs filter-process\n[filter \"y\"]\n\tclean\n",
    );
    passes(&check_path(&e, &repo));
    let nokey = e.work("benign2");
    init_repo(&nokey);
    append(&config_path(&nokey), "[core]\n\tfsmonitor\n");
    passes(&check_path(&e, &nokey));
}

#[test]
fn real_git_include_는_범위_안이면_막고_밖의_깨끗한_파일은_통과하며_밖의_filter_는_막는다() {
    let e = Env::new();
    // 저장소 안을 가리킴
    let a = e.work("inc-in");
    init_repo(&a);
    std::fs::write(a.join("extra.cfg"), "[user]\n\tname = x\n").unwrap();
    append(&config_path(&a), &format!("[include]\n\tpath = {}\n", s(&a.join("extra.cfg"))));
    blocked_with(&check_path(&e, &a), "저장소·임시 폴더 안");
    // 밖의 깨끗한 파일
    let clean = e.outside("clean.cfg");
    std::fs::write(&clean, "[user]\n\tname = x\n").unwrap();
    let b = e.work("inc-out");
    init_repo(&b);
    append(&config_path(&b), &format!("[include]\n\tpath = {}\n", s(&clean)));
    passes(&check_path(&e, &b));
    // includeIf 도 같다
    let c = e.work("inc-if");
    init_repo(&c);
    append(&config_path(&c), &format!("[includeIf \"gitdir:**\"]\n\tpath = {}\n", s(&clean)));
    passes(&check_path(&e, &c));
    // 밖의 파일에 든 filter 명령
    let evil = e.outside("evil.cfg");
    std::fs::write(&evil, "[filter \"z\"]\n\tsmudge = evil.exe\n").unwrap();
    let d = e.work("inc-evil");
    init_repo(&d);
    append(&config_path(&d), &format!("[include]\n\tpath = {}\n", s(&evil)));
    blocked_with(&check_path(&e, &d), "filter 명령");
    // 읽을 수 없는 include(폴더)
    let dir_inc = e.outside("a-dir.cfg");
    std::fs::create_dir_all(&dir_inc).unwrap();
    let f = e.work("inc-dir");
    init_repo(&f);
    append(&config_path(&f), &format!("[include]\n\tpath = {}\n", s(&dir_inc)));
    blocked_with(&check_path(&e, &f), "읽지 못해");
    // 안 쓰는 ~<사용자> 꼴은 git 과 같게 풀 수 없어 닫는다
    let h = e.work("inc-user");
    init_repo(&h);
    append(&config_path(&h), "[include]\n\tpath = ~bob/x.cfg\n");
    blocked_with(&check_path(&e, &h), "같게 풀 수 없어");
}

#[test]
fn real_git_설정_파일이_폴더이면_읽지_못해_막는다() {
    let e = Env::new();
    let repo = e.work("cfgdir");
    init_repo(&repo);
    // config.worktree 자리가 폴더
    std::fs::create_dir_all(repo.join(".git/config.worktree")).unwrap();
    blocked_with(&check_path(&e, &repo), "읽지 못해");
}

#[test]
fn 설정_출력_읽기는_키_값_값없음_줄바꿈_값을_가른다() {
    let got = parse_config_list("core.a\nv1\0core.b\0core.c\nline1\nline2\0\0");
    assert_eq!(
        got,
        vec![
            ConfigEntry::new("core.a", Some("v1")),
            ConfigEntry::new("core.b", None),
            ConfigEntry::new("core.c", Some("line1\nline2")),
        ]
    );
    assert!(parse_config_list("").is_empty());
}

// ── 하위 모듈 · 색인 ─────────────────────────────────

#[test]
fn real_git_하위_모듈_흔적은_막는다() {
    let e = Env::new();
    // 작업 트리의 .gitmodules
    let a = e.work("sm-wt");
    init_repo(&a);
    std::fs::write(a.join(".gitmodules"), "[submodule \"s\"]\n").unwrap();
    blocked_with(&check_path(&e, &a), "gitmodules");
    // 색인에만 있음
    let b = e.work("sm-idx");
    init_repo(&b);
    std::fs::write(b.join(".gitmodules"), "[submodule \"s\"]\n").unwrap();
    g(&b, &["add", ".gitmodules"]);
    std::fs::remove_file(b.join(".gitmodules")).unwrap();
    blocked_with(&check_path(&e, &b), "gitmodules");
    // HEAD 에만 있음
    let c = e.work("sm-head");
    init_repo(&c);
    std::fs::write(c.join(".gitmodules"), "[submodule \"s\"]\n").unwrap();
    g(&c, &["add", ".gitmodules"]);
    g(&c, &["commit", "-q", "-m", "sm"]);
    g(&c, &["rm", "-q", "--cached", ".gitmodules"]);
    std::fs::remove_file(c.join(".gitmodules")).unwrap();
    blocked_with(&check_path(&e, &c), "gitmodules");
    // 색인의 gitlink
    let d = e.work("sm-link");
    init_repo(&d);
    let sha = g(&d, &["rev-parse", "HEAD"]).trim().to_string();
    g(&d, &["update-index", "--add", "--cacheinfo", &format!("160000,{sha},sub")]);
    let r = check_path(&e, &d);
    assert!(r.block.is_some(), "gitlink 는 막는다: {r:?}");
}

#[test]
fn real_git_색인을_읽지_못하면_막는다() {
    let e = Env::new();
    let repo = e.work("badindex");
    init_repo(&repo);
    std::fs::write(repo.join(".git/index"), b"not an index at all").unwrap();
    blocked_with(&check_path(&e, &repo), "색인을 읽지 못해");
}

// ── 지난 실행 기록(③) ────────────────────────────────

#[test]
fn real_git_기록의_자리와_같으면_통과하고_다르면_막는다() {
    let e = Env::new();
    let repo = e.work("rec");
    init_repo(&repo);
    let common = format!("{}/.git", s(&repo));
    let rec = |ino: String| {
        let mut m = BTreeMap::new();
        m.insert(s(&repo), GitRecord { common_dir: Some(common.clone()), common_ino: Some(ino), work_git_dir: None, work_git_ino: None });
        m
    };
    let live = file_id(&common).unwrap();
    let ok = run_prep_git(&ProcessGit::new(), &repo.to_string_lossy(), &rec(live), &e.home());
    passes(&ok);
    let bad = run_prep_git(&ProcessGit::new(), &repo.to_string_lossy(), &rec("1:1".to_string()), &e.home());
    blocked_with(&bad, "지난 실행이 출발할 때");
}

// ── (6) 경로 표기 ────────────────────────────────────

#[cfg(windows)]
fn spellings(p: &Path) -> Vec<String> {
    let verbatim = p.to_string_lossy().into_owned(); // canonicalize → `\\?\C:\…`
    assert!(verbatim.starts_with(r"\\?\"), "{verbatim}");
    let plain = verbatim.trim_start_matches(r"\\?\").to_string();
    let slash = plain.replace('\\', "/");
    let mut lower = plain.clone();
    lower.replace_range(..1, &lower[..1].to_lowercase());
    vec![verbatim, plain, slash, lower, format!("{}\\.\\sub\\..", p.to_string_lossy().trim_start_matches(r"\\?\"))]
}

#[cfg(windows)]
#[test]
fn real_git_드라이브_글자_표기와_verbatim_표기는_슬래시_표기와_같은_판정을_낸다() {
    let e = Env::new();
    let pass_repo = e.work("sp-pass");
    init_repo(&pass_repo);
    let bad_repo = e.work("sp-bad");
    init_repo(&bad_repo);
    append(&config_path(&bad_repo), "[core]\n\tfsmonitor = evil.exe\n");
    let nested = e.work("sp-pass").join("inner");
    init_repo(&nested);
    let not_repo = e.work("sp-none");
    std::fs::create_dir_all(&not_repo).unwrap();
    for p in [&pass_repo, &bad_repo, &nested, &not_repo] {
        let want = check_path(&e, p);
        let forms = spellings(p);
        assert!(forms.len() >= 5);
        for f in forms {
            assert_eq!(check(&e, &f), want, "표기 {f}");
        }
    }
    passes(&check_path(&e, &pass_repo));
    assert!(check_path(&e, &bad_repo).block.is_some());
    assert!(check_path(&e, &nested).block.is_some());
    assert!(check_path(&e, &not_repo).top.is_none());
}

// ── (8) 파일 자리 ────────────────────────────────────

#[test]
fn 파일_자리는_표기가_달라도_같은_폴더면_같고_다른_폴더면_다르다() {
    let e = Env::new();
    let a = e.work("ida");
    let b = e.work("idb");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let ids = file_id(&a.to_string_lossy()).expect("자리가 나와야 한다");
    assert!(ids.contains(':'));
    assert_eq!(Some(ids.clone()), file_id(&s(&a)));
    assert_eq!(Some(ids.clone()), file_id(&format!("{}/./", s(&a))));
    #[cfg(windows)]
    {
        assert_eq!(Some(ids.clone()), file_id(a.to_string_lossy().trim_start_matches(r"\\?\")));
        assert!(a.to_string_lossy().starts_with(r"\\?\"));
    }
    assert_ne!(Some(ids), file_id(&b.to_string_lossy()));
    // 같은 폴더의 두 파일, 없는 자리
    std::fs::write(a.join("f1"), "1").unwrap();
    std::fs::write(a.join("f2"), "2").unwrap();
    assert_ne!(file_id(&a.join("f1").to_string_lossy()), file_id(&a.join("f2").to_string_lossy()));
    assert_eq!(file_id(&a.join("nope").to_string_lossy()), None);
}

// ── 세계 질문 하나하나 ───────────────────────────────

#[test]
fn real_git_세계_질문은_바로_가기를_따라가지_않고_종류를_가른다() {
    let e = Env::new();
    let git = ProcessGit::new();
    let w = RealPrepGitWorld::new(&git);
    let d = e.work("w");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("file.txt"), "hello").unwrap();
    let dp = s(&d);
    assert!(w.is_dir(&format!("{dp}/sub")) && !w.is_file(&format!("{dp}/sub")));
    assert!(w.is_file(&format!("{dp}/file.txt")) && !w.is_dir(&format!("{dp}/file.txt")));
    assert!(!w.is_file(&format!("{dp}/none")) && !w.is_dir(&format!("{dp}/none")));
    assert_eq!(w.text(&format!("{dp}/file.txt")).as_deref(), Some("hello"));
    assert_eq!(w.text(&format!("{dp}/sub")), None);
    assert_eq!(w.text(&format!("{dp}/none")), None);
    assert_eq!(w.entry(&dp), None);
    std::fs::create_dir_all(d.join(".git")).unwrap();
    assert_eq!(w.entry(&dp), Some(GitEntry::Dir));
    // 끝 구분자·뿌리 꼴
    assert_eq!(w.entry(&format!("{dp}/")), Some(GitEntry::Dir));
    assert_eq!(w.config(&format!("{dp}/none.cfg")), Some(Vec::new()));
    assert_eq!(w.config(&format!("{dp}/sub")), None);
    assert!(w.real(&format!("{dp}/none")).is_none());
    assert!(w.real(&format!("{dp}/sub/../file.txt")).unwrap().ends_with("file.txt"));
    assert_eq!(w.stamp(&format!("{dp}/file.txt")), file_id(&format!("{dp}/file.txt")));
    // `.git` 파일: gitdir 접두가 있으면 그 뒤 글(앞 공백 포함 그대로), 없으면 None
    let f = e.work("gf");
    std::fs::create_dir_all(&f).unwrap();
    std::fs::write(f.join(".git"), "gitdir: C:/x/y\n").unwrap();
    assert_eq!(w.entry(&s(&f)), Some(GitEntry::File(Some(" C:/x/y\n".to_string()))));
    std::fs::write(f.join(".git"), "GITDIR: C:/x/y\n").unwrap();
    assert_eq!(w.entry(&s(&f)), Some(GitEntry::File(None)));
}

// ── 어댑터가 git 에 넘기는 명령을 엿본다 ──────────────

struct Spy {
    inner: ProcessGit,
    calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
}

impl Git for Spy {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        self.calls.lock().unwrap().push((repo.to_path_buf(), args.iter().map(|a| a.to_string()).collect()));
        self.inner.run(repo, args)
    }
}

#[test]
fn real_git_설정은_저장소_밖_뿌리에서_읽고_하위_모듈은_확인한_git_폴더로_박아_본다() {
    let e = Env::new();
    let repo = e.work("spy");
    init_repo(&repo);
    let spy = Spy { inner: ProcessGit::new(), calls: Mutex::new(Vec::new()) };
    let r = run_prep_git(&spy, &repo.to_string_lossy(), &BTreeMap::new(), &e.home());
    passes(&r);
    let calls = spy.calls.lock().unwrap();
    let cfg: Vec<_> = calls.iter().filter(|(_, a)| a.first().map(String::as_str) == Some("config")).collect();
    assert!(!cfg.is_empty());
    for (cwd, a) in &cfg {
        assert!(!cwd.starts_with(&repo), "설정 읽기는 저장소 안에서 돌면 안 된다: {cwd:?}");
        assert!(a.iter().any(|x| x == "--no-includes") && a.iter().any(|x| x == "--null") && a.iter().any(|x| x == "--list"));
        assert!(a.windows(2).any(|w| w[0] == "--file"));
    }
    let ls: Vec<_> = calls.iter().filter(|(_, a)| a.iter().any(|x| x == "ls-files")).collect();
    assert!(!ls.is_empty());
    let want_gd = format!("--git-dir={}", r.git_dir.as_deref().unwrap());
    let want_wt = format!("--work-tree={}", r.top.as_deref().unwrap());
    for (cwd, a) in &ls {
        assert_eq!(cwd, &PathBuf::from(r.top.as_deref().unwrap()));
        assert_eq!(a[0], want_gd);
        assert_eq!(a[1], want_wt);
    }
    assert!(calls.iter().any(|(_, a)| a.iter().any(|x| x == "ls-tree") && a.iter().any(|x| x == "HEAD")));
    assert!(calls.iter().any(|(_, a)| a.iter().any(|x| x == "--") && a.iter().any(|x| x == ".gitmodules") && a.iter().any(|x| x == "ls-files")));
}

type Reply = Box<dyn Fn(&[&str]) -> PortResult<GitOutput> + Send + Sync>;

struct Scripted {
    reply: Reply,
}

impl Git for Scripted {
    fn run(&self, _repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        (self.reply)(args)
    }
}

fn out(code: i32, stdout: &str) -> PortResult<GitOutput> {
    Ok(GitOutput { code, stdout: stdout.to_string(), stderr: String::new() })
}

#[test]
fn 가짜_git_설정과_하위_모듈_답에_따른_세계의_답() {
    let pin = vec!["GIT_DIR=C:/r/.git".to_string(), "GIT_COMMON_DIR=C:/r/.git".to_string(), "GIT_WORK_TREE=C:/r".to_string()];
    let e = Env::new();
    let cfgfile = e.work("fake.cfg");
    std::fs::write(&cfgfile, "x").unwrap();
    let cf = s(&cfgfile);
    // config: 코드 0 이면 읽고, 0 이 아니거나 git 이 없으면 None
    let ok = Scripted { reply: Box::new(|_| out(0, "a.b\nv\0")) };
    assert_eq!(RealPrepGitWorld::new(&ok).config(&cf), Some(vec![ConfigEntry::new("a.b", Some("v"))]));
    let bad = Scripted { reply: Box::new(|_| out(1, "a.b\nv\0")) };
    assert_eq!(RealPrepGitWorld::new(&bad).config(&cf), None);
    let none = Scripted { reply: Box::new(|_| Err(PortError::new(PortErrorKind::Unavailable, "git 없음"))) };
    assert_eq!(RealPrepGitWorld::new(&none).config(&cf), None);
    assert_eq!(RealPrepGitWorld::new(&none).submodules("C:/r", &pin), None);
    // submodules: ls-files 실패는 None, 빈 답은 (없음, false)
    let fail = Scripted { reply: Box::new(|_| out(128, "")) };
    assert_eq!(RealPrepGitWorld::new(&fail).submodules("C:/r", &pin), None);
    let empty = Scripted { reply: Box::new(|_| out(0, "")) };
    assert_eq!(RealPrepGitWorld::new(&empty).submodules("C:/r", &pin), Some((Vec::new(), false)));
    // ls-files 만 성공하고 ls-tree 가 실패해도(HEAD 없음) 닫지 않는다
    let no_head = Scripted { reply: Box::new(|a| if a.contains(&"ls-tree") { out(128, "") } else { out(0, "") }) };
    assert_eq!(RealPrepGitWorld::new(&no_head).submodules("C:/r", &pin), Some((Vec::new(), false)));
    // 색인에 gitlink
    let link = Scripted { reply: Box::new(|a| if a.contains(&"-s") && !a.contains(&"--") { out(0, "160000 0123456789012345678901234567890123456789 0\tsub\0") } else { out(0, "") }) };
    let (links, gm) = RealPrepGitWorld::new(&link).submodules("C:/r", &pin).unwrap();
    assert_eq!(links, vec!["sub".to_string()]);
    assert!(!gm);
    // HEAD 의 .gitmodules 만
    let head_gm = Scripted { reply: Box::new(|a| if a.contains(&"ls-tree") { out(0, "100644 blob abc\t.gitmodules\n") } else { out(0, "") }) };
    assert!(RealPrepGitWorld::new(&head_gm).submodules("C:/r", &pin).unwrap().1);
    // 박는 값에 GIT_DIR·GIT_WORK_TREE 가 없으면 git 이 스스로 찾게 두지 않고 닫는다
    assert_eq!(RealPrepGitWorld::new(&empty).submodules("C:/r", &[]), None);
    assert_eq!(RealPrepGitWorld::new(&empty).submodules("C:/r", &["GIT_DIR=C:/r/.git".to_string()]), None);
    assert_eq!(RealPrepGitWorld::new(&empty).submodules("C:/r", &["GIT_WORK_TREE=C:/r".to_string()]), None);
    // 이름이 비슷한 변수(GIT_DIR_X)는 GIT_DIR 로 보지 않는다
    assert_eq!(RealPrepGitWorld::new(&empty).submodules("C:/r", &["GIT_DIR_X=C:/r/.git".to_string(), "GIT_WORK_TREE=C:/r".to_string()]), None);
}

// ── 규칙 흐름(가짜 세계) ─────────────────────────────

#[derive(Default)]
struct Fake {
    entries: BTreeMap<String, GitEntry>,
    head_stamp: bool,
    config: Option<Vec<ConfigEntry>>,
    subs: Option<(Vec<String>, bool)>,
}

impl PrepGitWorld for Fake {
    fn entry(&self, dir: &str) -> Option<GitEntry> {
        self.entries.get(dir).cloned()
    }
    fn real(&self, path: &str) -> Option<String> {
        Some(path.to_string())
    }
    fn text(&self, _: &str) -> Option<String> {
        None
    }
    fn is_file(&self, _: &str) -> bool {
        false
    }
    fn is_dir(&self, _: &str) -> bool {
        false
    }
    fn stamp(&self, p: &str) -> Option<String> {
        (self.head_stamp && p.ends_with("/HEAD")).then(|| "1:1".to_string())
    }
    fn config(&self, _: &str) -> Option<Vec<ConfigEntry>> {
        self.config.clone()
    }
    fn submodules(&self, _: &str, _: &[String]) -> Option<(Vec<String>, bool)> {
        self.subs.clone()
    }
}

fn fake_check(f: &Fake) -> PrepGit {
    prep_git_check("C:/h/work/repo", f, &BTreeMap::new(), "C:/h")
}

fn fake_with(entry: GitEntry) -> Fake {
    let mut f = Fake { config: Some(Vec::new()), subs: Some((Vec::new(), false)), ..Fake::default() };
    f.entries.insert("C:/h/work/repo".to_string(), entry);
    f
}

#[test]
fn 가짜_세계로_규칙의_막는_흐름을_모두_닿는다() {
    blocked_with(&fake_check(&fake_with(GitEntry::Link)), "바로 가기");
    blocked_with(&fake_check(&fake_with(GitEntry::Other)), "폴더도 파일도 아니라");
    blocked_with(&fake_check(&fake_with(GitEntry::File(None))), ".git 파일을 읽지 못해");
    let mut odd = fake_with(GitEntry::Dir);
    odd.head_stamp = true;
    blocked_with(&fake_check(&odd), "HEAD 가 보통 파일이 아니라");
    // 설정을 읽지 못함 · 색인을 읽지 못함 · 하위 모듈 · 통과
    let mut unreadable = fake_with(GitEntry::Dir);
    unreadable.config = None;
    unreadable.head_stamp = false;
    // HEAD 가 보통 파일이 아니고 stamp 도 없으면 지나쳐 저장소가 아니게 된다(가짜 세계는 is_file 이 늘 거짓)
    assert_eq!(fake_check(&unreadable), PrepGit::default());
    assert!(PrepGit::default().pin().is_empty());
    assert_eq!(block_message(&PrepGit::default(), "C:/h"), None);
    let b = PrepGit { top: Some("C:/h/work/repo".into()), block: Some("까닭이라".into()), ..PrepGit::default() };
    let m = block_message(&b, "C:/h").unwrap();
    assert!(m.contains("~/work/repo") || m.contains("~\\work\\repo") || m.contains("C:/h/work/repo"), "{m}");
    assert!(m.contains("까닭이라"));
}

#[test]
fn 파일_자리_글은_볼륨과_64비트_인덱스를_16진수로_적는다() {
    assert_eq!(format_file_id(0xABCD, 0x1, 0x2), "abcd:100000002");
    assert_eq!(format_file_id(0, 0, 0xff), "0:ff");
    assert_ne!(format_file_id(1, 0, 5), format_file_id(2, 0, 5));
    assert_ne!(format_file_id(1, 0, 5), format_file_id(1, 1, 5));
}
