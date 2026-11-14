//! git 작업 트리 어댑터 시험(`adapters::worktree`). `real-git` 표시가 붙은 것은 이 PC 의 진짜 git.exe 와 새 임시 폴더로 돈다.
//! 사용자의 ~/.gitconfig·실제 저장소·네트워크는 건드리지 않는다(git 은 보호 환경 `ProcessGit`).

use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::adapters::worktree::{
    add, canonical_loose, canonical_path, list, parse_porcelain, remove, Entry, Removed, MAX_PATH_LEN,
};
use claude_board_autopilot::engine::prep_git::{file_id, run_prep_git};
use claude_board_autopilot::ports::{Git, GitOutput, PortErrorKind, PortResult};
use claude_board_autopilot::rules::board_git::{board_git_changed, GitRecord};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ── 도우미 ───────────────────────────────────────────

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

/// 저장소 + 첫 커밋(가지 main)
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

fn real_add(repo: &Path, path: &str, branch: &str) -> PortResult<claude_board_autopilot::adapters::worktree::Added> {
    add(&ProcessGit::new(), repo, path, branch, "main")
}

fn kind<T: std::fmt::Debug>(r: PortResult<T>) -> PortErrorKind {
    r.expect_err("거부돼야 한다").kind
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase().replace('\\', "/") == b.to_lowercase().replace('\\', "/")
}

// ── 경로 한 가지 꼴(순수) ─────────────────────────────

#[test]
fn canonical_한_경로의_여러_글자_꼴은_한_결과() {
    let want = "C:/x/y";
    for p in [r"C:\x\y", "c:/x/y", r"\\?\C:\x\y", r"\\?\c:\x\y", "//?/C:/x/y", r"C:\x\\y\.", r"c:\x\.\y\", "C:/x//y/"] {
        assert_eq!(canonical_path(p).unwrap(), want, "{p}");
    }
    assert_eq!(canonical_path(r"\\?\UNC\srv\sh\x").unwrap(), "//srv/sh/x");
    assert_eq!(canonical_path(r"\\srv\sh\x").unwrap(), "//srv/sh/x");
    assert_eq!(canonical_path(r"C:\한글 폴더\작업 트리").unwrap(), "C:/한글 폴더/작업 트리");
    // 비슷해 보이지만 예약이 아닌 이름
    for p in [r"C:\x\console", r"C:\x\auxx", r"C:\x\com10", r"C:\x\nulx", r"C:\x\a.b", r"C:\x\.gitx", r"C:\x\x.git"] {
        assert!(canonical_path(p).is_ok(), "{p}");
    }
    assert_eq!(canonical_loose(r"c:\a\..\b"), "C:/b");
}

#[test]
fn canonical_거부_까닭은_글에_드러난다() {
    // 같은 거부라도 까닭이 맞게 적혀야 한다(`..` 는 끝 점 규칙에도 걸려 종류만으로는 가려지지 않는다)
    let why = |p: &str| canonical_path(p).unwrap_err().message;
    assert!(why(r"C:\x\..\y").contains("`..`") || why(r"C:\x\..\y").contains(".. 가"), "{}", why(r"C:\x\..\y"));
    assert!(why(r"\\.\C:\x").contains("장치 이름 공간"), "{}", why(r"\\.\C:\x"));
    assert!(why(r"C:\x\a.").contains("점이나 공백"));
    assert!(why(r"C:\x\a:b").contains("쓸 수 없는 글자"));
    assert!(why(r"C:\x\con").contains("예약 장치"));
    assert!(why(r"C:\x\PROGRA~1").contains("8.3"));
    assert!(why(r"C:\x\.git").contains(".git"));
    assert!(why("relative").contains("절대 경로"));
}

#[test]
fn canonical_거부_표() {
    let long = format!("C:/x/{}", "a".repeat(MAX_PATH_LEN));
    let cases: Vec<(String, PortErrorKind)> = vec![
        (r"C:\x\con".into(), PortErrorKind::Invalid),
        (r"C:\x\CON.txt".into(), PortErrorKind::Invalid),
        (r"C:\x\nul".into(), PortErrorKind::Invalid),
        (r"C:\x\Aux".into(), PortErrorKind::Invalid),
        (r"C:\x\prn.tar.gz".into(), PortErrorKind::Invalid),
        (r"C:\x\com1".into(), PortErrorKind::Invalid),
        (r"C:\x\lpt9.x".into(), PortErrorKind::Invalid),
        (r"C:\x\conin$".into(), PortErrorKind::Invalid),
        (r"C:\x\a.".into(), PortErrorKind::Invalid),
        (r"C:\x\a ".into(), PortErrorKind::Invalid),
        (r"C:\x\a.\b".into(), PortErrorKind::Invalid),
        (r"C:\x\a:b".into(), PortErrorKind::Invalid),
        (r"C:\x\a:stream".into(), PortErrorKind::Invalid),
        (r"C:\x\a?b".into(), PortErrorKind::Invalid),
        (r"C:\x\a*b".into(), PortErrorKind::Invalid),
        (r"C:\x\a|b".into(), PortErrorKind::Invalid),
        (r"C:\x\a<b".into(), PortErrorKind::Invalid),
        ("C:\\x\\a\"b".into(), PortErrorKind::Invalid),
        (r"C:\x\..\y".into(), PortErrorKind::Invalid),
        (r"C:\x\PROGRA~1".into(), PortErrorKind::Invalid),
        (r"C:\x\.git\y".into(), PortErrorKind::Denied),
        (r"C:\x\.GIT".into(), PortErrorKind::Denied),
        (r"\\.\C:\x".into(), PortErrorKind::Invalid),
        (r"\\?\C:\x\con".into(), PortErrorKind::Invalid),
        ("relative\\x".into(), PortErrorKind::Invalid),
        ("x".into(), PortErrorKind::Invalid),
        (r"\x".into(), PortErrorKind::Invalid),
        ("/x".into(), PortErrorKind::Invalid),
        ("C:x".into(), PortErrorKind::Invalid),
        ("C:".into(), PortErrorKind::Invalid),
        (r"C:\".into(), PortErrorKind::Invalid),
        (r"\\srv\sh".into(), PortErrorKind::Invalid),
        ("".into(), PortErrorKind::Invalid),
        ("C:\\x\\tab\there".into(), PortErrorKind::Invalid),
        ("C:\\x\\nul\0".into(), PortErrorKind::Invalid),
        (long, PortErrorKind::Invalid),
    ];
    for (p, want) in cases {
        let r = canonical_path(&p);
        let k = r.as_ref().err().map(|e| e.kind);
        // `/x` 는 윈도에서만 거부(다른 OS 에서는 절대 경로)
        if p == "/x" && !cfg!(windows) {
            assert!(r.is_ok());
            continue;
        }
        assert_eq!(k, Some(want), "{p:?} → {r:?}");
    }
}

// ── 목록 파서 표(순수) ────────────────────────────────

fn rec(path: &str, extra: &[&str]) -> String {
    let mut s = format!("worktree {path}\0HEAD 1111111111111111111111111111111111111111\0");
    for e in extra {
        s.push_str(e);
        s.push('\0');
    }
    s.push('\0');
    s
}

const H: &str = "1111111111111111111111111111111111111111";

#[test]
fn porcelain_표() {
    // 공백·한글 경로, 가지
    let out = format!("{}{}", rec("C:/r", &["branch refs/heads/main"]), rec("C:/r wts/한글 폴더", &["branch refs/heads/autopilot/x"]));
    assert_eq!(
        parse_porcelain(&out),
        vec![
            Entry { path: "C:/r".into(), head: Some(H.into()), branch: Some("main".into()), ..Entry::default() },
            Entry { path: "C:/r wts/한글 폴더".into(), head: Some(H.into()), branch: Some("autopilot/x".into()), ..Entry::default() },
        ]
    );
    // detached
    let d = parse_porcelain(&rec("C:/d", &["detached"]));
    assert!(d[0].detached && d[0].branch.is_none() && !d[0].bare && d[0].locked.is_none() && d[0].prunable.is_none());
    // bare(HEAD 줄 없음)
    let b = parse_porcelain("worktree C:/b.git\0bare\0\0");
    assert_eq!(b, vec![Entry { path: "C:/b.git".into(), bare: true, ..Entry::default() }]);
    // locked: 까닭 없음·있음(한글·줄바꿈)
    let l0 = parse_porcelain(&rec("C:/l", &["detached", "locked"]));
    assert_eq!(l0[0].locked, Some(String::new()));
    let l1 = parse_porcelain(&rec("C:/l", &["detached", "locked 사유 있음\n둘째 줄"]));
    assert_eq!(l1[0].locked.as_deref(), Some("사유 있음\n둘째 줄"));
    // prunable: 까닭 없음·있음
    let p0 = parse_porcelain(&rec("C:/p", &["detached", "prunable"]));
    assert_eq!(p0[0].prunable, Some(String::new()));
    let p1 = parse_porcelain(&rec("C:/p", &["detached", "prunable gitdir file points to non-existent location"]));
    assert_eq!(p1[0].prunable.as_deref(), Some("gitdir file points to non-existent location"));
    // 잠기고 지울 수 있는 것
    let both = parse_porcelain(&rec("C:/q", &["detached", "locked r", "prunable p"]));
    assert_eq!((both[0].locked.as_deref(), both[0].prunable.as_deref()), (Some("r"), Some("p")));
    // ref 가 가지가 아니면 전체 이름
    assert_eq!(parse_porcelain(&rec("C:/t", &["branch refs/tags/v1"]))[0].branch.as_deref(), Some("refs/tags/v1"));
    // 마지막 빈 필드가 없어도 마지막 항목이 남고, 빈 입력·모르는 줄은 무해
    assert_eq!(parse_porcelain("worktree C:/z\0HEAD abc").len(), 1);
    assert!(parse_porcelain("").is_empty());
    assert!(parse_porcelain("\0\0").is_empty());
    assert_eq!(parse_porcelain("junk\0worktree C:/z\0future thing\0\0").len(), 1);
}

// ── 가짜 git ─────────────────────────────────────────

type Handler = Box<dyn Fn(&[&str]) -> GitOutput + Send + Sync>;

struct Fake {
    calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
    handler: Handler,
}

impl Fake {
    fn new(handler: impl Fn(&[&str]) -> GitOutput + Send + Sync + 'static) -> Fake {
        Fake { calls: Mutex::new(Vec::new()), handler: Box::new(handler) }
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().iter().map(|(_, a)| a.join(" ")).collect()
    }
    fn has(&self, needle: &str) -> bool {
        self.calls().iter().any(|c| c.contains(needle))
    }
    fn all_for(&self, repo: &Path) {
        let calls = self.calls.lock().unwrap();
        assert!(!calls.is_empty());
        for (r, a) in calls.iter() {
            assert_eq!(r, repo, "저장소가 다른 호출 {a:?}");
        }
    }
}

impl Git for Fake {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        self.calls.lock().unwrap().push((repo.to_path_buf(), args.iter().map(|s| s.to_string()).collect()));
        Ok((self.handler)(args))
    }
}

fn ok(stdout: &str) -> GitOutput {
    GitOutput { code: 0, stdout: stdout.into(), stderr: String::new() }
}

fn fail(code: i32, stderr: &str) -> GitOutput {
    GitOutput { code, stdout: String::new(), stderr: stderr.into() }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wire {
    Good,
    NoDotGit,
    DotGitNotGitdir,
    DotGitOutsideWorktrees,
    DotGitNested,
    NoCommondir,
    BadCommondir,
    NoBack,
    BadBack,
}

#[derive(Clone, Copy)]
struct Script {
    bad_ref: bool,
    branch_exists: bool,
    base_ok: bool,
    add_code: i32,
    wire: Wire,
}

const GOOD: Script = Script { bad_ref: false, branch_exists: false, base_ok: true, add_code: 0, wire: Wire::Good };

/// `worktree add` 를 흉내 내는 가짜. 저장소 공통 git 폴더는 `<root>/fake/.git`
fn fake_add(root: &str, s: Script) -> Fake {
    let common = format!("{root}/fake/.git");
    let root = root.to_string();
    Fake::new(move |a| match a {
        ["rev-parse", "--path-format=absolute", "--git-common-dir"] => ok(&format!("{common}\n")),
        ["check-ref-format", ..] => {
            if s.bad_ref {
                fail(1, "bad")
            } else {
                ok("")
            }
        }
        ["rev-parse", "--verify", "--quiet", r] if r.starts_with("refs/heads/") => {
            if s.branch_exists {
                ok("sha\n")
            } else {
                fail(1, "")
            }
        }
        ["rev-parse", "--verify", "--quiet", _] => {
            if s.base_ok {
                ok("sha\n")
            } else {
                fail(1, "")
            }
        }
        ["worktree", "add", "-b", _, path, _] => {
            if s.add_code != 0 {
                return fail(s.add_code, "fatal: boom\nsecond line\n");
            }
            let gd = format!("{common}/worktrees/wt");
            std::fs::create_dir_all(path).unwrap();
            std::fs::create_dir_all(&gd).unwrap();
            let gitdir_line = match s.wire {
                Wire::DotGitOutsideWorktrees => format!("gitdir: {root}/elsewhere/wt"),
                Wire::DotGitNested => format!("gitdir: {gd}/deeper"),
                _ => format!("gitdir: {gd}"),
            };
            match s.wire {
                Wire::NoDotGit => {}
                Wire::DotGitNotGitdir => std::fs::write(format!("{path}/.git"), "nonsense").unwrap(),
                _ => std::fs::write(format!("{path}/.git"), format!("{gitdir_line}\n")).unwrap(),
            }
            let gd_eff = match s.wire {
                Wire::DotGitOutsideWorktrees => format!("{root}/elsewhere/wt"),
                Wire::DotGitNested => format!("{gd}/deeper"),
                _ => gd.clone(),
            };
            std::fs::create_dir_all(&gd_eff).unwrap();
            match s.wire {
                Wire::NoCommondir => {}
                Wire::BadCommondir => std::fs::write(format!("{gd_eff}/commondir"), "../../other").unwrap(),
                _ => {
                    let rel = if s.wire == Wire::DotGitOutsideWorktrees || s.wire == Wire::DotGitNested { common.clone() } else { "../..".to_string() };
                    std::fs::write(format!("{gd_eff}/commondir"), format!("{rel}\n")).unwrap();
                }
            }
            match s.wire {
                Wire::NoBack => {}
                Wire::BadBack => std::fs::write(format!("{gd_eff}/gitdir"), format!("{root}/other/.git\n")).unwrap(),
                _ => std::fs::write(format!("{gd_eff}/gitdir"), format!("{path}/.git\n")).unwrap(),
            }
            ok("")
        }
        _ => ok(""),
    })
}

fn fake_repo(e: &Env) -> PathBuf {
    let r = PathBuf::from(e.p("fake"));
    std::fs::create_dir_all(r.join(".git")).unwrap();
    r
}

#[test]
fn 가짜_add_통과는_정리한_경로와_git_폴더를_돌려준다() {
    let e = Env::new();
    let repo = fake_repo(&e);
    let f = fake_add(&e.root, GOOD);
    let want_path = e.p("wts/one");
    let r = add(&f, &repo, &format!("{}\\wts\\one", e.root.replace('/', "\\")), "autopilot/r1", "main").unwrap();
    assert!(same(&r.path, &want_path) && r.path.contains('/') && !r.path.contains('\\'), "{r:?}");
    assert!(same(&r.git_dir, &e.p("fake/.git/worktrees/wt")));
    assert!(same(&r.common_dir, &e.p("fake/.git")));
    // git 에 넘기는 경로는 정리한 꼴
    assert!(f.has(&format!("worktree add -b autopilot/r1 {} main", r.path)), "{:?}", f.calls());
    f.all_for(&repo);
}

#[test]
fn 가짜_add_모든_거부와_실패_결말() {
    let e = Env::new();
    let repo = fake_repo(&e);
    let path = e.p("wts/two");
    let table: Vec<(&str, Script, PortErrorKind)> = vec![
        ("bad ref", Script { bad_ref: true, ..GOOD }, PortErrorKind::Invalid),
        ("branch exists", Script { branch_exists: true, ..GOOD }, PortErrorKind::Denied),
        ("base missing", Script { base_ok: false, ..GOOD }, PortErrorKind::NotFound),
        ("git fails", Script { add_code: 128, ..GOOD }, PortErrorKind::Io),
        ("no .git", Script { wire: Wire::NoDotGit, ..GOOD }, PortErrorKind::Denied),
        ("not gitdir", Script { wire: Wire::DotGitNotGitdir, ..GOOD }, PortErrorKind::Denied),
        ("outside worktrees", Script { wire: Wire::DotGitOutsideWorktrees, ..GOOD }, PortErrorKind::Denied),
        ("nested", Script { wire: Wire::DotGitNested, ..GOOD }, PortErrorKind::Denied),
        ("no commondir", Script { wire: Wire::NoCommondir, ..GOOD }, PortErrorKind::Denied),
        ("bad commondir", Script { wire: Wire::BadCommondir, ..GOOD }, PortErrorKind::Denied),
        ("no back", Script { wire: Wire::NoBack, ..GOOD }, PortErrorKind::Denied),
        ("bad back", Script { wire: Wire::BadBack, ..GOOD }, PortErrorKind::Denied),
    ];
    for (name, s, want) in table {
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_dir_all(e.p("fake/.git/worktrees"));
        let f = fake_add(&e.root, s);
        let err = add(&f, &repo, &path, "autopilot/r1", "main").expect_err(name);
        assert_eq!(err.kind, want, "{name}: {err}");
        f.all_for(&repo);
        let wired_wrong = matches!(
            s.wire,
            Wire::NoDotGit | Wire::DotGitNotGitdir | Wire::DotGitOutsideWorktrees | Wire::DotGitNested | Wire::NoCommondir | Wire::BadCommondir | Wire::NoBack | Wire::BadBack
        ) && s.add_code == 0
            && !s.bad_ref
            && !s.branch_exists
            && s.base_ok;
        if wired_wrong {
            // 만든 것만 되돌린다: 작업 트리 등록·가지
            assert!(f.has(&format!("worktree remove --force {path}")), "{name}: {:?}", f.calls());
            assert!(f.has("worktree prune") && f.has("branch -D autopilot/r1"), "{name}: {:?}", f.calls());
        } else {
            // 만들기 전에 거부했거나 git 이 실패 — 되돌릴 것이 없다
            assert!(!f.has("worktree remove") && !f.has("branch -D"), "{name}: {:?}", f.calls());
            if want != PortErrorKind::Io {
                assert!(!f.has("worktree add"), "{name}: {:?}", f.calls());
            }
        }
    }
    // git 실패 메시지는 첫 줄만
    let f = fake_add(&e.root, Script { add_code: 128, ..GOOD });
    let _ = std::fs::remove_dir_all(&path);
    let m = add(&f, &repo, &path, "b1", "main").unwrap_err().message;
    assert!(m.contains("fatal: boom") && !m.contains("second line"), "{m}");
}

#[test]
fn 가짜_add_경로_거부는_git_을_부르기_전에() {
    let e = Env::new();
    let repo = fake_repo(&e);
    let f = fake_add(&e.root, GOOD);
    for p in [r"C:\x\con", r"C:\x\a.", "relative", r"C:\x\..\y"] {
        assert_eq!(kind(add(&f, &repo, p, "b", "main")), PortErrorKind::Invalid, "{p}");
    }
    assert!(f.calls().is_empty(), "{:?}", f.calls());
    // 저장소 .git 안(가짜 저장소 기준)·같은 폴더의 다른 철자
    for p in [e.p("fake/.git/wt"), e.p("fake/.git"), e.p("FAKE/.GIT/worktrees/x").replace('/', "\\")] {
        assert_eq!(kind(add(&f, &repo, &p, "b", "main")), PortErrorKind::Denied, "{p}");
    }
    // `.git` 성분은 경로 검사에서 먼저 막혀 git 도 부르지 않는다
    assert!(f.calls().is_empty(), "{:?}", f.calls());
}

/// 공통 git 폴더가 `.git` 이름이 아닌 저장소(`--separate-git-dir`·bare 꼴)도 그 안은 막는다
#[test]
fn 가짜_add_이름이_다른_공통_git_폴더_안도_막는다() {
    let e = Env::new();
    let repo = PathBuf::from(e.p("work"));
    let common = e.p("store/common");
    let c2 = common.clone();
    let f = Fake::new(move |a| match a {
        ["rev-parse", "--path-format=absolute", "--git-common-dir"] => ok(&format!("{c2}\n")),
        _ => ok(""),
    });
    for p in [common.clone(), format!("{common}/worktrees/x"), common.to_uppercase().replace('/', "\\") + r"\y"] {
        assert_eq!(kind(add(&f, &repo, &p, "b", "main")), PortErrorKind::Denied, "{p}");
    }
    assert!(!f.has("worktree add") && !f.has("check-ref-format"), "{:?}", f.calls());
    f.all_for(&repo);
}

#[test]
fn 가짜_add_저장소가_아니면_거부() {
    let e = Env::new();
    let f = Fake::new(|_| fail(128, "fatal: not a git repository"));
    let repo = PathBuf::from(e.p("none"));
    assert_eq!(kind(add(&f, &repo, &e.p("w"), "b", "main")), PortErrorKind::Invalid);
    assert_eq!(f.calls().len(), 1);
}

#[test]
fn 가짜_add_자리_검사_파일_비지않은_폴더_빈_폴더() {
    let e = Env::new();
    let repo = fake_repo(&e);
    // 파일
    std::fs::create_dir_all(e.p("wts")).unwrap();
    std::fs::write(e.p("wts/file"), "x").unwrap();
    let f = fake_add(&e.root, GOOD);
    assert_eq!(kind(add(&f, &repo, &e.p("wts/file"), "b", "main")), PortErrorKind::Denied);
    // 안이 있는 폴더
    std::fs::create_dir_all(e.p("wts/full")).unwrap();
    std::fs::write(e.p("wts/full/x"), "x").unwrap();
    assert_eq!(kind(add(&f, &repo, &e.p("wts/full"), "b", "main")), PortErrorKind::Denied);
    assert!(!f.has("worktree add"));
    // 빈 폴더는 쓴다
    std::fs::create_dir_all(e.p("wts/empty")).unwrap();
    assert!(add(&f, &repo, &e.p("wts/empty"), "b", "main").is_ok());
}

#[test]
fn 가짜_remove_결말_표() {
    let e = Env::new();
    let repo = PathBuf::from(e.p("main"));
    let main = e.p("main");
    let wt = e.p("wts/w1");
    std::fs::create_dir_all(&wt).unwrap();
    let gone = e.p("wts/gone");

    struct Case {
        name: &'static str,
        entries: Vec<(String, Vec<&'static str>)>,
        target: String,
        force: bool,
        remove_code: i32,
        sticky: bool,
    }
    let plain = || vec![(main.clone(), vec!["branch refs/heads/main"]), (wt.clone(), vec!["branch refs/heads/a"])];
    let locked = |extra: &'static str| vec![(main.clone(), vec!["branch refs/heads/main"]), (wt.clone(), vec!["detached", extra])];
    let missing = |extra: Vec<&'static str>| {
        let mut v = vec!["detached", "prunable gitdir file points to non-existent location"];
        v.extend(extra);
        vec![(main.clone(), vec!["branch refs/heads/main"]), (gone.clone(), v)]
    };
    let cases = vec![
        Case { name: "clean", entries: plain(), target: wt.clone(), force: false, remove_code: 0, sticky: false },
        Case { name: "force", entries: plain(), target: wt.clone(), force: true, remove_code: 0, sticky: false },
        Case { name: "dirty", entries: plain(), target: wt.clone(), force: false, remove_code: 128, sticky: false },
        Case { name: "locked", entries: locked("locked r"), target: wt.clone(), force: false, remove_code: 0, sticky: false },
        Case { name: "locked force", entries: locked("locked r"), target: wt.clone(), force: true, remove_code: 0, sticky: false },
        Case { name: "main", entries: plain(), target: main.clone(), force: true, remove_code: 0, sticky: false },
        Case { name: "unregistered", entries: plain(), target: e.p("wts/other"), force: true, remove_code: 0, sticky: false },
        Case { name: "still listed", entries: plain(), target: wt.clone(), force: false, remove_code: 0, sticky: true },
        Case { name: "missing", entries: missing(vec![]), target: gone.clone(), force: false, remove_code: 0, sticky: false },
        Case { name: "missing sticky", entries: missing(vec![]), target: gone.clone(), force: false, remove_code: 0, sticky: true },
        Case { name: "missing locked", entries: missing(vec!["locked x"]), target: gone.clone(), force: false, remove_code: 0, sticky: false },
        Case { name: "missing locked force", entries: missing(vec!["locked x"]), target: gone.clone(), force: true, remove_code: 0, sticky: false },
        Case { name: "bare", entries: vec![(main.clone(), vec!["branch refs/heads/main"]), (wt.clone(), vec!["bare"])], target: wt.clone(), force: true, remove_code: 0, sticky: false },
    ];
    for c in cases {
        let listing = Mutex::new(c.entries.iter().map(|(p, x)| rec(p, x)).collect::<String>());
        let main_only = rec(&main, &["branch refs/heads/main"]);
        let (code, sticky) = (c.remove_code, c.sticky);
        let f = Fake::new(move |a| match a {
            ["worktree", "list", "--porcelain", "-z"] => ok(&listing.lock().unwrap()),
            ["worktree", "remove", ..] | ["worktree", "prune"] => {
                if code != 0 {
                    return fail(code, "fatal: contains modified or untracked files, use --force to delete it\n");
                }
                if !sticky {
                    *listing.lock().unwrap() = main_only.clone();
                }
                ok("")
            }
            _ => ok(""),
        });
        let r = remove(&f, &repo, &c.target, c.force);
        let calls = f.calls();
        let removed = |flags: &str| calls.iter().any(|x| x == &format!("worktree remove {flags}{}", c.target));
        match c.name {
            "clean" => {
                assert_eq!(r.unwrap(), Removed::Removed);
                assert!(removed(""), "{calls:?}");
            }
            "force" => {
                assert_eq!(r.unwrap(), Removed::Removed);
                assert!(removed("--force "), "{calls:?}");
            }
            "locked force" => {
                assert_eq!(r.unwrap(), Removed::Removed);
                assert!(removed("--force --force "), "{calls:?}");
            }
            "missing" => {
                assert_eq!(r.unwrap(), Removed::Pruned);
                assert!(f.has("worktree prune") && !f.has("worktree remove") && !f.has("unlock"), "{calls:?}");
            }
            "missing locked force" => {
                assert_eq!(r.unwrap(), Removed::Pruned);
                let (u, p) = (calls.iter().position(|x| x.starts_with("worktree unlock")), calls.iter().position(|x| x == "worktree prune"));
                assert!(u.is_some() && p.is_some() && u < p, "{calls:?}");
            }
            "dirty" => {
                assert_eq!(r.unwrap_err().kind, PortErrorKind::Denied);
            }
            "locked" | "main" | "bare" | "missing locked" => {
                assert_eq!(r.unwrap_err().kind, PortErrorKind::Denied, "{}", c.name);
                assert!(!f.has("worktree remove") && !f.has("prune") && !f.has("unlock"), "{}: {calls:?}", c.name);
            }
            "unregistered" => {
                assert_eq!(r.unwrap_err().kind, PortErrorKind::NotFound);
                assert!(!f.has("worktree remove") && !f.has("prune"), "{calls:?}");
            }
            "still listed" | "missing sticky" => {
                assert_eq!(r.unwrap_err().kind, PortErrorKind::Io, "{}", c.name);
            }
            other => panic!("{other}"),
        }
        f.all_for(&repo);
    }
}

/// 폴더 바로 가기(Windows 는 정션, 그 밖은 심볼릭 링크)를 만든다
fn make_link(link: &str, target: &str) {
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", &link.replace('/', "\\"), &target.replace('/', "\\")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(target, link).is_ok();
    assert!(made, "바로 가기를 만들 수 없다");
}

#[test]
fn 가짜_remove_바로가기_자리는_지우지_않는다() {
    // 정션/심볼릭 링크를 만들 수 있는 환경에서만(Windows 는 정션을 mklink /J 로)
    let e = Env::new();
    let repo = PathBuf::from(e.p("main"));
    let target = e.p("real");
    let link = e.p("wts/link");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(format!("{target}/keep.txt"), "keep").unwrap();
    std::fs::create_dir_all(e.p("wts")).unwrap();
    make_link(&link, &target);
    let listing = format!("{}{}", rec(&e.p("main"), &["branch refs/heads/main"]), rec(&link, &["detached"]));
    let f = Fake::new(move |a| if a.first() == Some(&"worktree") && a.get(1) == Some(&"list") { ok(&listing) } else { ok("") });
    assert_eq!(kind(remove(&f, &repo, &link, true)), PortErrorKind::Denied);
    assert!(!f.has("worktree remove"));
    assert!(Path::new(&format!("{target}/keep.txt")).exists());
    // 자리가 바로 가기인 곳에는 add 도 하지 않는다
    // (가리키는 곳이 빈 폴더여도 — 비어 있다는 이유가 아니라 바로 가기라는 이유로 막는다)
    std::fs::create_dir_all(e.p("emptytarget")).unwrap();
    let link2 = e.p("wts/link2");
    make_link(&link2, &e.p("emptytarget"));
    let fa = fake_add(&e.root, GOOD);
    let err = add(&fa, &fake_repo(&e), &link2, "b", "main").unwrap_err();
    assert_eq!(err.kind, PortErrorKind::Denied);
    assert!(err.message.contains("바로 가기"), "{}", err.message);
    assert!(!fa.has("worktree add"));
}

#[test]
fn 가짜_list_git_실패는_오류() {
    let f = Fake::new(|_| fail(129, "error: unknown option `z'"));
    let err = list(&f, Path::new("C:/x")).unwrap_err();
    assert_eq!(err.kind, PortErrorKind::Io);
    assert!(err.message.contains("unknown option"));
}

// ── 진짜 git ─────────────────────────────────────────

/// real-git: add 뒤 list 에 보이고 원래 작업 트리가 먼저
#[test]
fn real_git_add_뒤_list_에_보인다() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let wt = e.p("wts/one");
    let a = real_add(&repo, &wt, "autopilot/r1").unwrap();
    assert!(same(&a.path, &wt));
    assert!(Path::new(&wt).join("a.txt").exists());
    let l = list(&ProcessGit::new(), &repo).unwrap();
    assert_eq!(l.len(), 2);
    assert!(same(&canonical_loose(&l[0].path), &e.p("repo")) && l[0].branch.as_deref() == Some("main"), "{l:?}");
    assert!(same(&canonical_loose(&l[1].path), &wt), "{l:?}");
    assert_eq!(l[1].branch.as_deref(), Some("autopilot/r1"));
    assert!(!l[1].detached && !l[1].bare && l[1].locked.is_none() && l[1].prunable.is_none());
    assert_eq!(l[1].head.as_ref().map(String::len), Some(40));
}

/// real-git: 새 작업 트리의 `.git` 파일·commondir 가 맞고 출발 전 확인 규칙·보드 git 규칙이 통과시킨다
#[test]
fn real_git_git_폴더_되짚기와_규칙_통과() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let wt = e.p("wts/two");
    let a = real_add(&repo, &wt, "autopilot/r2").unwrap();
    let dot_git = std::fs::read_to_string(format!("{wt}/.git")).unwrap();
    assert!(dot_git.starts_with("gitdir:"), "{dot_git}");
    assert!(same(&a.git_dir, &e.p("repo/.git/worktrees/two")), "{a:?}");
    assert!(same(&a.common_dir, &e.p("repo/.git")));
    let cd = std::fs::read_to_string(format!("{}/commondir", a.git_dir)).unwrap();
    assert!(same(&canonical_loose(&format!("{}/{}", a.git_dir, cd.trim())), &a.common_dir), "{cd}");

    // 규칙 prep_git: 이 작업 트리는 따로 작업 트리로 통과하고 git 폴더가 어댑터가 준 것과 같다
    let pg = run_prep_git(&ProcessGit::new(), &wt, &BTreeMap::new(), &e.home());
    assert!(pg.block.is_none(), "{pg:?}");
    assert!(same(pg.top.as_deref().unwrap(), &wt), "{pg:?}");
    assert!(same(pg.git_dir.as_deref().unwrap(), &a.git_dir), "{pg:?}");
    assert!(same(pg.common_dir.as_deref().unwrap(), &a.common_dir), "{pg:?}");

    // 규칙 board_git: 출발 때 기록과 지금이 같다. 작업 트리 git 폴더가 바뀌면 잡고, 지운 뒤(work_tree_gone)에는 공통만 본다
    let rec = GitRecord {
        common_dir: pg.common_dir.clone(),
        common_ino: file_id(pg.common_dir.as_deref().unwrap()),
        work_git_dir: pg.git_dir.clone(),
        work_git_ino: file_id(pg.git_dir.as_deref().unwrap()),
    };
    let (cino, wino) = (rec.common_ino.clone(), rec.work_git_ino.clone());
    assert!(cino.is_some() && wino.is_some());
    assert_eq!(board_git_changed(&rec, pg.common_dir.as_deref(), cino.as_deref(), wino.as_deref(), false), None);
    assert!(board_git_changed(&rec, pg.common_dir.as_deref(), cino.as_deref(), Some("0:0"), false).is_some());
    remove(&ProcessGit::new(), &repo, &wt, false).unwrap();
    assert_eq!(board_git_changed(&rec, pg.common_dir.as_deref(), cino.as_deref(), None, true), None);
    // 지운 뒤 같은 자리를 다시 만들어도 어댑터 검사를 통과한다
    real_add(&repo, &wt, "autopilot/r2b").unwrap();
}

/// real-git: 거부 — 비지 않은 폴더·파일·.git 안·나쁜 이름·이미 있는 가지·없는 출발 가지
#[test]
fn real_git_add_거부들() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let heads_before = g(&repo, &["branch", "--list"]);

    // 비지 않은 폴더, 파일
    std::fs::create_dir_all(e.p("wts/full")).unwrap();
    std::fs::write(e.p("wts/full/keep.txt"), "keep").unwrap();
    assert_eq!(kind(real_add(&repo, &e.p("wts/full"), "b1")), PortErrorKind::Denied);
    assert_eq!(std::fs::read_to_string(e.p("wts/full/keep.txt")).unwrap(), "keep");
    std::fs::write(e.p("wts/afile"), "x").unwrap();
    assert_eq!(kind(real_add(&repo, &e.p("wts/afile"), "b1")), PortErrorKind::Denied);

    // 저장소 .git 안 — 바로 아래·worktrees 아래·다른 철자. 연결된 작업 트리를 저장소로 줘도 원래 .git 안은 막는다
    for p in [e.p("repo/.git/wt"), e.p("repo/.git"), e.p("repo/.git/worktrees/x"), e.p("REPO/.GIT/y").replace('/', "\\")] {
        assert_eq!(kind(real_add(&repo, &p, "b1")), PortErrorKind::Denied, "{p}");
    }
    let linked = real_add(&repo, &e.p("wts/linked"), "autopilot/linked").unwrap();
    assert_eq!(kind(real_add(Path::new(&linked.path), &e.p("repo/.git/worktrees/zz"), "b9")), PortErrorKind::Denied);
    assert!(!Path::new(&e.p("repo/.git/worktrees/zz")).exists());

    // 나쁜 이름
    for bad in [r"con", r"wts\a.", r"wts\a:b", r"wts\NUL.txt", r"wts\a b "] {
        let p = format!("{}\\{bad}", e.root.replace('/', "\\"));
        assert_eq!(kind(real_add(&repo, &p, "b1")), PortErrorKind::Invalid, "{p}");
    }

    // 가지: 이미 있음·나쁜 이름·없는 출발 가지
    g(&repo, &["branch", "exists-already"]);
    assert_eq!(kind(real_add(&repo, &e.p("wts/b"), "exists-already")), PortErrorKind::Denied);
    assert_eq!(kind(real_add(&repo, &e.p("wts/b"), "main")), PortErrorKind::Denied);
    for bad in ["a b", "-x", "a..b", "", "x.lock", "a~b"] {
        assert_eq!(kind(real_add(&repo, &e.p("wts/b"), bad)), PortErrorKind::Invalid, "{bad:?}");
    }
    assert_eq!(kind(add(&ProcessGit::new(), &repo, &e.p("wts/b"), "nb", "no-such-base")), PortErrorKind::NotFound);
    assert_eq!(kind(add(&ProcessGit::new(), &repo, &e.p("wts/b"), "nb", "--help")), PortErrorKind::Invalid);
    // 저장소가 아닌 곳
    std::fs::create_dir_all(e.p("plain")).unwrap();
    assert_eq!(kind(real_add(Path::new(&e.p("plain")), &e.p("wts/b"), "nb")), PortErrorKind::Invalid);

    assert!(!Path::new(&e.p("wts/b")).exists());
    // 거부한 것들은 가지를 만들지 않았다(연결 작업 트리 가지 하나만 늘었다)
    let after = g(&repo, &["branch", "--list"]);
    assert_eq!(after.lines().count(), heads_before.lines().count() + 2, "{after}");
}

/// real-git: 지우기 — 깨끗한 것
#[test]
fn real_git_remove_깨끗한_것() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let wt = e.p("wts/c");
    real_add(&repo, &wt, "autopilot/c").unwrap();
    assert_eq!(remove(&ProcessGit::new(), &repo, &wt, false).unwrap(), Removed::Removed);
    assert!(!Path::new(&wt).exists());
    assert!(!Path::new(&e.p("repo/.git/worktrees/c")).exists());
    assert_eq!(list(&ProcessGit::new(), &repo).unwrap().len(), 1);
    // 가지는 남는다(합치기·버리기는 호출자 몫)
    assert!(g(&repo, &["branch", "--list", "autopilot/c"]).contains("autopilot/c"));
    // 다시 지우면 등록이 없다
    assert_eq!(kind(remove(&ProcessGit::new(), &repo, &wt, false)), PortErrorKind::NotFound);
}

/// real-git: 남은 변경이 있으면 force 없이 거부, force 로 지운다
#[test]
fn real_git_remove_남은_변경은_force_만() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    for (name, dirty) in [("untracked", true), ("modified", false)] {
        let wt = e.p(&format!("wts/{name}"));
        real_add(&repo, &wt, &format!("autopilot/{name}")).unwrap();
        if dirty {
            std::fs::write(format!("{wt}/new.txt"), "n").unwrap();
        } else {
            std::fs::write(format!("{wt}/a.txt"), "changed").unwrap();
        }
        assert_eq!(kind(remove(&ProcessGit::new(), &repo, &wt, false)), PortErrorKind::Denied, "{name}");
        assert!(Path::new(&wt).exists(), "{name}");
        assert_eq!(remove(&ProcessGit::new(), &repo, &wt, true).unwrap(), Removed::Removed, "{name}");
        assert!(!Path::new(&wt).exists(), "{name}");
    }
}

/// real-git: 손으로 지운 폴더는 prune 으로 등록만 치운다
#[test]
fn real_git_remove_손으로_지운_폴더() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let wt = e.p("wts/gone");
    real_add(&repo, &wt, "autopilot/gone").unwrap();
    std::fs::remove_dir_all(&wt).unwrap();
    let l = list(&ProcessGit::new(), &repo).unwrap();
    assert!(l[1].prunable.is_some(), "{l:?}");
    assert_eq!(remove(&ProcessGit::new(), &repo, &wt, false).unwrap(), Removed::Pruned);
    assert_eq!(list(&ProcessGit::new(), &repo).unwrap().len(), 1);
    assert!(!Path::new(&e.p("repo/.git/worktrees/gone")).exists());
}

/// real-git: 잠긴 작업 트리(까닭 포함) — force 없이 거부, force 로 지운다. 폴더가 없어진 잠긴 것도 force 로 prune
#[test]
fn real_git_remove_잠긴_것() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let wt = e.p("wts/lk");
    real_add(&repo, &wt, "autopilot/lk").unwrap();
    g(&repo, &["worktree", "lock", "--reason", "쓰는 중 사유", &wt]);
    let l = list(&ProcessGit::new(), &repo).unwrap();
    assert_eq!(l[1].locked.as_deref(), Some("쓰는 중 사유"), "{l:?}");
    assert_eq!(kind(remove(&ProcessGit::new(), &repo, &wt, false)), PortErrorKind::Denied);
    assert!(Path::new(&wt).exists());
    assert_eq!(remove(&ProcessGit::new(), &repo, &wt, true).unwrap(), Removed::Removed);
    assert!(!Path::new(&wt).exists());

    let wt2 = e.p("wts/lk2");
    real_add(&repo, &wt2, "autopilot/lk2").unwrap();
    g(&repo, &["worktree", "lock", &wt2]);
    std::fs::remove_dir_all(&wt2).unwrap();
    assert_eq!(kind(remove(&ProcessGit::new(), &repo, &wt2, false)), PortErrorKind::Denied);
    assert_eq!(remove(&ProcessGit::new(), &repo, &wt2, true).unwrap(), Removed::Pruned);
    assert_eq!(list(&ProcessGit::new(), &repo).unwrap().len(), 1);
}

/// real-git: 등록되지 않은 폴더·원래 작업 트리는 force 여도 지우지 않는다
#[test]
fn real_git_remove_등록_밖은_건드리지_않는다() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    std::fs::create_dir_all(e.p("outside/inner")).unwrap();
    std::fs::write(e.p("outside/inner/keep.txt"), "keep").unwrap();
    for p in [e.p("outside"), e.p("outside/inner"), e.p("wts/never")] {
        assert_eq!(kind(remove(&ProcessGit::new(), &repo, &p, true)), PortErrorKind::NotFound, "{p}");
    }
    assert!(Path::new(&e.p("outside/inner/keep.txt")).exists());
    // 원래 작업 트리
    assert_eq!(kind(remove(&ProcessGit::new(), &repo, &e.p("repo"), true)), PortErrorKind::Denied);
    assert!(Path::new(&e.p("repo/a.txt")).exists() && Path::new(&e.p("repo/.git")).exists());
    // 등록된 작업 트리의 안쪽 폴더는 등록 경로가 아니다
    let wt = e.p("wts/x");
    real_add(&repo, &wt, "autopilot/x").unwrap();
    assert_eq!(kind(remove(&ProcessGit::new(), &repo, &format!("{wt}/.."), true)), PortErrorKind::Invalid);
    assert!(Path::new(&wt).exists());
}

/// real-git: 한 경로의 여러 철자(`C:\x`·`c:/x`·`\\?\C:\x`)로 만들고 지우고 목록에서 찾는다
#[test]
#[cfg(windows)]
fn real_git_철자가_달라도_같은_작업_트리() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let back = e.root.replace('/', "\\");
    let lower = format!("{}{}", back[..1].to_lowercase(), &back[1..]);
    let spellings = [
        format!("{back}\\wts\\sp"),
        format!("{lower}/wts/sp"),
        format!(r"\\?\{back}\wts\sp"),
        format!("{back}\\wts\\.\\sp\\"),
    ];
    let canon = canonical_path(&spellings[0]).unwrap();
    for s in &spellings {
        assert_eq!(canonical_path(s).unwrap(), canon, "{s}");
    }
    for (i, add_with) in spellings.iter().enumerate() {
        let a = real_add(&repo, add_with, &format!("autopilot/sp{i}")).unwrap();
        assert_eq!(a.path, canon);
        let l = list(&ProcessGit::new(), &repo).unwrap();
        assert_eq!(l.iter().filter(|x| same(&x.path, &canon)).count(), 1, "{l:?}");
        // 같은 자리에 다른 철자로 다시 만들면 거부(이미 비지 않음)
        let again = &spellings[(i + 1) % spellings.len()];
        assert_eq!(kind(real_add(&repo, again, &format!("autopilot/again{i}"))), PortErrorKind::Denied);
        let remove_with = &spellings[(i + 2) % spellings.len()];
        assert_eq!(remove(&ProcessGit::new(), &repo, remove_with, false).unwrap(), Removed::Removed, "{remove_with}");
        assert_eq!(list(&ProcessGit::new(), &repo).unwrap().len(), 1);
    }
}

/// real-git: 공백·한글이 든 경로
#[test]
fn real_git_공백_한글_경로() {
    let e = Env::new();
    let repo = init_repo(&e.p("저장소 하나"));
    let wt = e.p("작업 트리들/실행 1/저장소 하나");
    let a = real_add(&repo, &wt, "autopilot/한글").unwrap();
    assert!(same(&a.path, &wt));
    let l = list(&ProcessGit::new(), &repo).unwrap();
    assert!(l.iter().any(|x| same(&canonical_loose(&x.path), &wt) && x.branch.as_deref() == Some("autopilot/한글")), "{l:?}");
    assert_eq!(remove(&ProcessGit::new(), &repo, &wt, false).unwrap(), Removed::Removed);
}

// ── 어댑터 경유 확인 ─────────────────────────────────

struct Spy {
    inner: ProcessGit,
    calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
}

impl Git for Spy {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        self.calls.lock().unwrap().push((repo.to_path_buf(), args.iter().map(|s| s.to_string()).collect()));
        self.inner.run(repo, args)
    }
}

/// real-git: 만들기·목록·지우기의 모든 호출이 같은 저장소를 `-C` 로 받는다(어댑터가 `-C` 를 건다) — 호출마다 저장소 인자가 같다
#[test]
fn real_git_모든_호출이_저장소를_받는다() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let spy = Spy { inner: ProcessGit::new(), calls: Mutex::new(Vec::new()) };
    let wt = e.p("wts/spy");
    add(&spy, &repo, &wt, "autopilot/spy", "main").unwrap();
    list(&spy, &repo).unwrap();
    remove(&spy, &repo, &wt, false).unwrap();
    let calls = spy.calls.lock().unwrap();
    assert!(calls.len() >= 8, "{calls:?}");
    for (r, a) in calls.iter() {
        assert_eq!(r, &repo, "{a:?}");
        // 작업 트리 쪽 경로를 저장소 자리로 쓴 호출이 없다
        assert!(!a.iter().any(|x| x == "-C"));
    }
    // 만들기 호출은 정리한 경로를 쓴다
    assert!(calls.iter().any(|(_, a)| a.iter().any(|x| same(x, &wt)) && a[0] == "worktree" && a[1] == "add"));
}

/// 등록 목록이 적은 글자(드라이브 소문자·`\` 구분자·대소문자 다름)가 부른 경로의 정리한 꼴과 달라도,
/// git 에는 등록된 글자 그대로 넘긴다(부른 쪽 글자가 아니라)
#[test]
fn 가짜_remove_는_등록된_글자_그대로_git_에_넘긴다() {
    let e = Env::new();
    let repo = PathBuf::from(e.p("main"));
    let wt = e.p("wts/w1");
    std::fs::create_dir_all(&wt).unwrap();
    let lower_drive = format!("{}{}", wt[..1].to_lowercase(), &wt[1..]);
    let backslash = wt.replace('/', "\\");
    let mut variants = vec![lower_drive, backslash];
    if cfg!(windows) {
        variants.push(wt.to_uppercase());
    }
    for registered in variants {
        let listing = format!("{}{}", rec(&e.p("main"), &["branch refs/heads/main"]), rec(&registered, &["detached"]));
        let f = Fake::new(move |a| if a.first() == Some(&"worktree") && a.get(1) == Some(&"list") { ok(&listing) } else { ok("") });
        // 정리한 꼴과 다른 철자임을 먼저 확인한다
        assert_ne!(registered, canonical_path(&wt).unwrap());
        // 부르는 쪽은 또 다른 철자
        let called = wt.replace('/', "\\");
        let _ = remove(&f, &repo, &called, false);
        let calls = f.calls();
        assert!(calls.iter().any(|c| c == &format!("worktree remove {registered}")), "{registered}: {calls:?}");
        assert!(!calls.iter().any(|c| c.contains("remove") && c.ends_with(&canonical_path(&wt).unwrap()) && registered != canonical_path(&wt).unwrap()), "{calls:?}");
    }
}

/// real-git: git 에 소문자 드라이브로 등록된 작업 트리를 다른 철자로 불러도 지워지고, 목록의 글자는 등록된 그대로다
#[test]
#[cfg(windows)]
fn real_git_등록된_철자와_다른_철자로_지운다() {
    let e = Env::new();
    let repo = init_repo(&e.p("repo"));
    let wt = e.p("wts/reg");
    let lower = format!("{}{}", wt[..1].to_lowercase(), &wt[1..]);
    g(&repo, &["worktree", "add", "-b", "autopilot/reg", &lower, "main"]);
    let l = list(&ProcessGit::new(), &repo).unwrap();
    assert!(l.iter().any(|x| same(&x.path, &wt)), "{l:?}");
    assert_eq!(remove(&ProcessGit::new(), &repo, &wt.replace('/', "\\"), false).unwrap(), Removed::Removed);
    assert!(!Path::new(&wt).exists());
}

/// 가지 이름 검사는 git 의 `check-ref-format` 이 받아 줘도 스스로 거른다(앞의 `-`·빈칸)
#[test]
fn 가짜_add_가지_이름은_git_이_받아도_스스로_거른다() {
    let e = Env::new();
    let repo = fake_repo(&e);
    let f = fake_add(&e.root, GOOD); // check-ref-format 은 늘 통과
    for bad in ["-x", "--detach", "a b", "a\tb", ""] {
        assert_eq!(kind(add(&f, &repo, &e.p("wts/nb"), bad, "main")), PortErrorKind::Invalid, "{bad:?}");
        assert_eq!(kind(add(&f, &repo, &e.p("wts/nb"), "ok", bad)), PortErrorKind::Invalid, "base {bad:?}");
    }
    assert!(!f.has("worktree add") && !f.has("check-ref-format -- "), "{:?}", f.calls());
}
