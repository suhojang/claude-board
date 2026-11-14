//! 엔진 쪽 가지(`engine::branches`) 시험: 가지 이름 표, 출발 가지, run.json 기록. `real-git` 표시가 붙은 것은 이 PC 의 진짜 git.exe 와 새 임시 폴더로 돈다.
//! 사용자의 ~/.gitconfig·실제 저장소·네트워크는 건드리지 않는다(git 은 보호 환경 `ProcessGit`). 임시 폴더는 시험이 끝나면 지워진다.

use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::engine::branches::{
    branch_name, check_branch_name, create_work_branch, current_branch, list_branches, normalise_id, record_branch, record_story_branch, start_branch,
    unit_branch_name, BranchError, NameRefusal, StartBranch, StartSource, WorkRecord, MAX_BRANCH_LEN,
};
use claude_board_autopilot::model::{from_slice, to_pretty_json, RunFile};
use claude_board_autopilot::ports::{Git, GitOutput, PortError, PortErrorKind, PortResult};
use claude_board_autopilot::rules::branches::{
    failed_rows, merge_mark, merge_open, merge_sources, promoted_rows, story_repos, MergeMark, Step,
};
use serde_json::{json, Map, Value};
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
        Env { _tmp: tmp, root }
    }
    fn repo(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

fn g(dir: &Path, args: &[&str]) -> String {
    let out = ProcessGit::new().run(dir, args).expect("git.exe 가 있어야 한다");
    assert_eq!(out.code, 0, "git {args:?} 실패: {}", out.stderr);
    out.stdout
}

fn gs(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

/// 저장소(첫 가지 `first`) + 첫 커밋. `extra` 가지는 그 머리에서 만든다
fn init_repo(dir: &Path, first: &str, extra: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    g(dir, &["init", "-q", "-b", first]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        g(dir, &["config", k, v]);
    }
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    g(dir, &["add", "a.txt"]);
    g(dir, &["commit", "-q", "-m", "base"]);
    for b in extra {
        g(dir, &["branch", b]);
    }
}

fn start(dir: &Path) -> Result<StartBranch, BranchError> {
    start_branch(&ProcessGit::new(), &gs(dir), None)
}

fn name_of(r: Result<StartBranch, BranchError>) -> (String, StartSource) {
    let s = r.expect("출발 가지가 골라져야 한다");
    (s.name, s.source)
}

// ── 이름 표 ──────────────────────────────────────────

/// (입력, 거절 까닭 — None 이면 받는다, git 이 `autopilot/<입력>` 을 받는가)
type Row = (&'static str, Option<NameRefusal>, bool);

fn id_table() -> Vec<Row> {
    use NameRefusal::*;
    vec![
        // 받는 이름
        ("20261110-abc", None, true),
        ("run_1.2-x", None, true),
        ("a", None, true),
        ("A.B", None, true),
        ("a.b.c", None, true),
        ("v1..", Some(DotDot), false),
        ("conx", None, true),
        ("com10", None, true),
        ("lpt0", Some(ReservedDevice("lpt0".into())), true),
        ("COM0", Some(ReservedDevice("com0".into())), true),
        ("console", None, true),
        ("auxiliary", None, true),
        ("x.locky", None, true),
        // 거절: git 도 거부
        ("", Some(Empty), false),
        ("a..b", Some(DotDot), false),
        ("a b", Some(BadChar(' ')), false),
        ("a~1", Some(BadChar('~')), false),
        ("a^b", Some(BadChar('^')), false),
        ("a:b", Some(BadChar(':')), false),
        ("a?b", Some(BadChar('?')), false),
        ("a*b", Some(BadChar('*')), false),
        ("a[b", Some(BadChar('[')), false),
        ("a\\b", Some(BadChar('\\')), false),
        ("a/b", Some(BadChar('/')), true),
        ("a\tb", Some(BadChar('\t')), false),
        ("a@{b", Some(BadChar('@')), false),
        (".hidden", Some(LeadingDot), false),
        ("end.", Some(TrailingDot), false),
        ("x.lock", Some(LockSuffix), false),
        // 거절: git 은 받지만 안전하지 않다
        ("x.LOCK", Some(LockSuffix), true),
        ("x.Lock", Some(LockSuffix), true),
        ("-dash", Some(LeadingDash), true),
        ("con", Some(ReservedDevice("con".into())), true),
        ("CON", Some(ReservedDevice("con".into())), true),
        ("Nul", Some(ReservedDevice("nul".into())), true),
        ("aux", Some(ReservedDevice("aux".into())), true),
        ("prn", Some(ReservedDevice("prn".into())), true),
        ("com1", Some(ReservedDevice("com1".into())), true),
        ("COM9", Some(ReservedDevice("com9".into())), true),
        ("lpt1", Some(ReservedDevice("lpt1".into())), true),
        ("LPT9", Some(ReservedDevice("lpt9".into())), true),
        ("con.txt", Some(ReservedDevice("con".into())), true),
        ("nul.tar.gz", Some(ReservedDevice("nul".into())), true),
        ("한글", Some(BadChar('한')), true),
        ("run-é", Some(BadChar('é')), true),
        ("a\u{202e}b", Some(BadChar('\u{202e}')), true),
    ]
}

#[test]
fn branch_name_표_하나씩() {
    for (input, want, _) in id_table() {
        let got = branch_name(input);
        match want {
            None => assert_eq!(got.as_deref(), Ok(format!("autopilot/{input}").as_str()), "{input:?}"),
            Some(w) => assert_eq!(got, Err(w), "{input:?}"),
        }
    }
}

#[test]
fn branch_name_길이_경계() {
    assert!(branch_name(&"a".repeat(64)).is_ok());
    assert_eq!(branch_name(&"a".repeat(65)), Err(NameRefusal::TooLong));
    assert_eq!(branch_name(&"a".repeat(300)), Err(NameRefusal::TooLong));
    // 한글이 섞여도 길이 거절보다 글자 거절이 말해 준다(앞 칸부터 본다)
    assert_eq!(branch_name(&format!("{}한", "a".repeat(10))), Err(NameRefusal::BadChar('한')));
}

#[test]
fn real_git_표의_git_판정이_실제와_같다() {
    // 표의 세 번째 칸(git 이 받는가)이 이 PC 의 git 과 맞아야 한다 — 표가 지어낸 값이 아니라는 증거
    let e = Env::new();
    let dir = e.repo("chk");
    init_repo(&dir, "master", &[]);
    for (input, _, git_ok) in id_table() {
        if input.is_empty() || input.contains(['\t']) {
            continue;
        }
        let full = format!("autopilot/{input}");
        let out = ProcessGit::new().run(&dir, &["check-ref-format", "--branch", &full]).unwrap();
        assert_eq!(out.code == 0, git_ok, "git check-ref-format {full:?} → {} {}", out.code, out.stderr);
    }
    // 300자: git 은 받지만(받는 쪽이 규칙)
    let long = format!("autopilot/{}", "a".repeat(300));
    assert_eq!(ProcessGit::new().run(&dir, &["check-ref-format", "--branch", &long]).unwrap().code, 0);
}

#[test]
fn real_git_받은_이름은_모두_실제_가지가_된다() {
    let e = Env::new();
    for (i, (input, want, _)) in id_table().into_iter().enumerate() {
        if want.is_some() {
            continue;
        }
        let dir = e.repo(&format!("ok{i}"));
        init_repo(&dir, "master", &[]);
        let work = branch_name(input).unwrap();
        let git = ProcessGit::new();
        create_work_branch(&git, &gs(&dir), &work, "master").unwrap_or_else(|er| panic!("{work}: {er}"));
        assert_eq!(current_branch(&git, &gs(&dir)).unwrap().as_deref(), Some(work.as_str()));
        assert!(list_branches(&git, &gs(&dir)).unwrap().contains(&work));
        g(&dir, &["check-ref-format", "--branch", &work]);
    }
    // 64자 경계
    let dir = e.repo("l64");
    init_repo(&dir, "master", &[]);
    let work = branch_name(&"b".repeat(64)).unwrap();
    create_work_branch(&ProcessGit::new(), &gs(&dir), &work, "master").unwrap();
}

#[test]
fn real_git_거절한_이름은_git_이_못_만들거나_안전하지_않은_것뿐() {
    // git 이 거부하는 이름은 만들기도 실패한다. git 이 받는데 거절한 것은 표에 그 까닭(Windows 예약·옵션 꼴·비 ASCII·대소문자 잠금)이 있다
    let e = Env::new();
    let dir = e.repo("bad");
    init_repo(&dir, "master", &[]);
    for (input, want, git_ok) in id_table() {
        if want.is_none() || input.is_empty() || input.contains('\t') {
            continue;
        }
        let full = format!("autopilot/{input}");
        if git_ok {
            // git 이 만들어 주는 이름 — 거절은 우리 안전 규칙이다
            let reason = want.unwrap();
            let unsafe_kind = matches!(
                reason,
                NameRefusal::LockSuffix | NameRefusal::LeadingDash | NameRefusal::ReservedDevice(_) | NameRefusal::BadChar(_)
            );
            assert!(unsafe_kind, "{input:?}: git 은 받는데 거절한 까닭이 안전 규칙이 아니다: {reason:?}");
        } else {
            let out = ProcessGit::new().run(&dir, &["branch", &full]).unwrap();
            assert_ne!(out.code, 0, "git 이 받았는데 우리가 이미 거절할 일: {full:?}");
        }
    }
}

#[test]
fn create_work_branch_는_이름_공간과_칸_규칙을_다시_본다() {
    let e = Env::new();
    let dir = e.repo("cw");
    init_repo(&dir, "master", &[]);
    let git = ProcessGit::new();
    let r = gs(&dir);
    assert_eq!(create_work_branch(&git, &r, "feature/x", "master"), Err(BranchError::Name(NameRefusal::ReservedNamespace)));
    assert_eq!(create_work_branch(&git, &r, "autopilot", "master"), Err(BranchError::Name(NameRefusal::ReservedNamespace)));
    assert_eq!(create_work_branch(&git, &r, "autopilot/a/b", "master"), Err(BranchError::Name(NameRefusal::ReservedNamespace)));
    assert_eq!(create_work_branch(&git, &r, "autopilot/con", "master"), Err(BranchError::Name(NameRefusal::ReservedDevice("con".into()))));
    // 아직 아무 가지도 안 바뀌었다
    assert_eq!(list_branches(&git, &r).unwrap(), vec!["master".to_string()]);
    create_work_branch(&git, &r, "autopilot/run1", "master").unwrap();
    // 같은 이름, 대소문자만 다른 이름은 거절한다
    g(&dir, &["switch", "-q", "master"]);
    assert_eq!(create_work_branch(&git, &r, "autopilot/run1", "master"), Err(BranchError::Exists("autopilot/run1".into())));
    assert_eq!(create_work_branch(&git, &r, "autopilot/RUN1", "master"), Err(BranchError::Exists("autopilot/run1".into())));
    // 없는 출발 가지는 git 이 거부한다
    assert!(matches!(create_work_branch(&git, &r, "autopilot/run2", "nope"), Err(BranchError::Git(_))));
    assert_eq!(current_branch(&git, &r).unwrap().as_deref(), Some("master"));
}

#[test]
fn unit_branch_name_표() {
    use NameRefusal::*;
    let story = "features/parent/{story}";
    let task = "features/{task}";
    let typed = "{type}/_{story}/{task}";
    assert_eq!(unit_branch_name(story, None, Some("ABC-12"), None).as_deref(), Ok("features/parent/ABC-12"));
    assert_eq!(unit_branch_name(task, None, None, Some("t1")).as_deref(), Ok("features/t1"));
    assert_eq!(unit_branch_name(typed, Some("feature"), Some("ABC-12"), Some("ABC-13")).as_deref(), Ok("feature/_ABC-12/ABC-13"));
    // 값 하나가 칸을 넘으면 거절(경로 나누기·탈출)
    assert_eq!(unit_branch_name(task, None, None, Some("a/b")), Err(BadChar('/')));
    assert_eq!(unit_branch_name(task, None, None, Some("../x")), Err(BadChar('/')));
    assert_eq!(unit_branch_name(task, None, None, Some("..")), Err(DotDot));
    assert_eq!(unit_branch_name(task, None, None, Some("")), Err(Empty));
    assert_eq!(unit_branch_name(task, None, None, Some("a b")), Err(BadChar(' ')));
    assert_eq!(unit_branch_name(task, None, None, Some("한글")), Err(BadChar('한')));
    assert_eq!(unit_branch_name(task, None, None, Some("con")), Err(ReservedDevice("con".into())));
    assert_eq!(unit_branch_name(task, None, None, Some("x.lock")), Err(LockSuffix));
    assert_eq!(unit_branch_name(task, None, None, Some(&"a".repeat(65))), Err(TooLong));
    // 값이 안 채워진 자리표는 글자 거절로 걸린다
    assert_eq!(unit_branch_name(task, None, None, None), Err(BadChar('{')));
    assert_eq!(unit_branch_name(typed, None, Some("S"), Some("T")), Err(BadChar('{')));
    // 패턴 자체의 결함: 빈 칸·이름 공간·긴 이름·끝 점·`.lock`
    assert_eq!(unit_branch_name("/{task}", None, None, Some("a")), Err(EmptySegment));
    assert_eq!(unit_branch_name("{task}/", None, None, Some("a")), Err(EmptySegment));
    assert_eq!(unit_branch_name("a//{task}", None, None, Some("a")), Err(EmptySegment));
    assert_eq!(unit_branch_name("autopilot/{task}", None, None, Some("a")), Err(ReservedNamespace));
    assert_eq!(unit_branch_name("autopilot", None, None, None), Err(ReservedNamespace));
    assert_eq!(unit_branch_name("{task}.", None, None, Some("a")), Err(TrailingDot));
    assert_eq!(unit_branch_name("{task}.lock", None, None, Some("a")), Err(LockSuffix));
    assert_eq!(unit_branch_name("", None, None, None), Err(Empty));
    // 전체 길이: 칸마다 64 이하여도 합이 200 을 넘으면 거절
    let seg = "a".repeat(60);
    let long = format!("{seg}/{seg}/{seg}/{seg}");
    assert_eq!(long.len(), 243);
    assert!(long.len() > MAX_BRANCH_LEN);
    assert_eq!(check_branch_name(&long), Err(TooLong));
    let ok = format!("{seg}/{seg}/{seg}");
    assert_eq!(ok.len(), 182);
    assert_eq!(check_branch_name(&ok), Ok(()));
    // 전체 길이 경계: 200 은 받고 201 은 거절
    let at200 = format!("{}/{}/{}/{}", "a".repeat(60), "b".repeat(60), "c".repeat(60), "d".repeat(17));
    assert_eq!(at200.len(), MAX_BRANCH_LEN);
    assert_eq!(check_branch_name(&at200), Ok(()));
    assert_eq!(check_branch_name(&format!("{at200}d")), Err(NameRefusal::TooLong));
    // autopilot 로 시작하지만 다른 칸이면 이름 공간이 아니다
    assert_eq!(check_branch_name("autopilots/x"), Ok(()));
    assert_eq!(check_branch_name("x/autopilot/y"), Ok(()));
}

#[test]
fn real_git_단위_가지_이름은_git_이_받고_실제로_만들어진다() {
    let e = Env::new();
    let dir = e.repo("unit");
    init_repo(&dir, "master", &[]);
    for name in [
        unit_branch_name("features/parent/{story}", None, Some("ABC-12"), None).unwrap(),
        unit_branch_name("features/{task}", None, None, Some("ABC-13")).unwrap(),
        unit_branch_name("{type}/_{story}/{task}", Some("bugfix"), Some("S-1"), Some("T_2")).unwrap(),
    ] {
        g(&dir, &["check-ref-format", "--branch", &name]);
        g(&dir, &["branch", &name]);
    }
    // 같은 스토리 아래 작업 가지는 스토리 가지와 디렉터리·파일 충돌이 없다(`features/parent/<키>` 와 `features/<작업>`)
    assert_eq!(list_branches(&ProcessGit::new(), &gs(&dir)).unwrap().len(), 4);
}

#[test]
fn real_git_autopilot_run_가지에_단위_가지를_겹치면_git_이_거부한다() {
    // 이름 공간을 거절하는 까닭: `autopilot/<id>` 가 있으면 `autopilot/<id>/<단위>` 는 git 이 만들 수 없다
    let e = Env::new();
    let dir = e.repo("df");
    init_repo(&dir, "master", &[]);
    g(&dir, &["branch", "autopilot/run1"]);
    let out = ProcessGit::new().run(&dir, &["branch", "autopilot/run1/unit1"]).unwrap();
    assert_ne!(out.code, 0);
    assert_eq!(unit_branch_name("autopilot/{run}/{task}", None, None, Some("u")), Err(NameRefusal::BadChar('{')));
    assert_eq!(unit_branch_name("autopilot/run1/{task}", None, None, Some("u")), Err(NameRefusal::ReservedNamespace));
}

#[test]
fn normalise_id_결과는_늘_안전하고_같은_입력은_같은_id() {
    let inputs = [
        "my project", "한글 폴더", "새 폴더 (2)", "a..b", "..", "con", "CON.txt", "nul", "x.lock", "x.LOCK.lock", "-rf", ".git", "end.", "a:b*c?d", "C:\\w\\a",
        "한글", "漢字", "😀", "", "   ", "ok-name_1.2", "a/b", &"x".repeat(300), &"한".repeat(100),
    ];
    let e = Env::new();
    let dir = e.repo("norm");
    init_repo(&dir, "master", &[]);
    let mut seen = std::collections::BTreeMap::new();
    for raw in inputs {
        let id = normalise_id(raw).unwrap_or_else(|er| panic!("{raw:?}: {er}"));
        let work = branch_name(&id).unwrap_or_else(|er| panic!("{raw:?} → {id:?} 이 안전하지 않다: {er}"));
        g(&dir, &["check-ref-format", "--branch", &work]);
        assert!(id.len() <= 64, "{id}");
        assert_eq!(normalise_id(raw).unwrap(), id, "같은 입력은 같은 id");
        if let Some(prev) = seen.insert(id.clone(), raw) {
            assert_eq!(prev, raw, "서로 다른 입력 {prev:?}·{raw:?} 이 같은 id {id:?} 로 겹쳤다");
        }
    }
    // 이미 안전한 이름은 그대로, 바뀐 이름은 해시가 붙어 서로 다르다
    assert_eq!(normalise_id("ok-name_1.2").unwrap(), "ok-name_1.2");
    assert_eq!(normalise_id("한글").unwrap(), normalise_id("한글").unwrap());
    assert_ne!(normalise_id("한글").unwrap(), normalise_id("漢字").unwrap());
    assert_ne!(normalise_id("a b").unwrap(), normalise_id("a-b").unwrap());
    // 안전한 id 를 다시 넣어도 변하지 않는다(멱등)
    for raw in inputs {
        let id = normalise_id(raw).unwrap();
        assert_eq!(normalise_id(&id).unwrap(), id, "{raw:?}");
    }
}

// ── 출발 가지(real-git) ──────────────────────────────

#[test]
fn real_git_출발_가지_master_만_있는_저장소() {
    let e = Env::new();
    let dir = e.repo("m");
    init_repo(&dir, "master", &[]);
    assert_eq!(name_of(start(&dir)), ("master".into(), StartSource::Role("master")));
}

#[test]
fn real_git_출발_가지_main_과_dev() {
    let e = Env::new();
    let dir = e.repo("md");
    init_repo(&dir, "main", &["dev"]);
    // dev 가 master 자리(main)보다 앞이다
    assert_eq!(name_of(start(&dir)), ("dev".into(), StartSource::Role("dev")));
    let dir2 = e.repo("md2");
    init_repo(&dir2, "main", &["develop"]);
    assert_eq!(name_of(start(&dir2)), ("develop".into(), StartSource::Role("dev")));
}

#[test]
fn real_git_출발_가지_live_dev_master_는_live() {
    let e = Env::new();
    let dir = e.repo("ldm");
    init_repo(&dir, "master", &["dev", "live"]);
    assert_eq!(name_of(start(&dir)), ("live".into(), StartSource::Role("live")));
    // 현재 걸린 가지가 master 여도 live 가 이긴다
    assert_eq!(current_branch(&ProcessGit::new(), &gs(&dir)).unwrap().as_deref(), Some("master"));
    let dir2 = e.repo("ldm2");
    init_repo(&dir2, "main", &["staging", "develop"]);
    assert_eq!(name_of(start(&dir2)), ("staging".into(), StartSource::Role("live")));
}

#[test]
fn real_git_출발_가지_자리_가지가_없으면_걸린_가지() {
    let e = Env::new();
    let dir = e.repo("cur");
    init_repo(&dir, "trunk", &["feature"]);
    assert_eq!(name_of(start(&dir)), ("trunk".into(), StartSource::Current));
    g(&dir, &["switch", "-q", "feature"]);
    assert_eq!(name_of(start(&dir)), ("feature".into(), StartSource::Current));
}

#[test]
fn real_git_출발_가지_떨어진_head() {
    let e = Env::new();
    // 떨어진 HEAD + 자리 가지 있음 → 자리 가지
    let a = e.repo("d1");
    init_repo(&a, "master", &[]);
    g(&a, &["checkout", "-q", "--detach"]);
    assert_eq!(current_branch(&ProcessGit::new(), &gs(&a)).unwrap(), None);
    assert_eq!(name_of(start(&a)), ("master".into(), StartSource::Role("master")));
    // 떨어진 HEAD + 자리 가지도 없음 → 고를 수 없다
    let b = e.repo("d2");
    init_repo(&b, "trunk", &[]);
    g(&b, &["checkout", "-q", "--detach"]);
    assert_eq!(start(&b), Err(BranchError::NoStartBranch));
}

#[test]
fn real_git_출발_가지_커밋_없는_빈_저장소() {
    let e = Env::new();
    let dir = e.repo("empty");
    std::fs::create_dir_all(&dir).unwrap();
    g(&dir, &["init", "-q", "-b", "master"]);
    assert_eq!(start(&dir), Err(BranchError::NoCommits));
    assert_eq!(start_branch(&ProcessGit::new(), &gs(&dir), Some("master")), Err(BranchError::NoCommits));
    assert!(list_branches(&ProcessGit::new(), &gs(&dir)).unwrap().is_empty());
}

#[test]
fn real_git_출발_가지_정해_준_가지() {
    let e = Env::new();
    let dir = e.repo("req");
    init_repo(&dir, "master", &["dev", "feature"]);
    let git = ProcessGit::new();
    let s = start_branch(&git, &gs(&dir), Some("feature")).unwrap();
    assert_eq!((s.name.as_str(), s.source), ("feature", StartSource::Requested));
    assert_eq!(start_branch(&git, &gs(&dir), Some("nope")), Err(BranchError::Missing("nope".into())));
    // 가지 이름은 글 그대로 견준다(대소문자·끝 공백이 다르면 다른 이름)
    assert_eq!(start_branch(&git, &gs(&dir), Some("Dev")), Err(BranchError::Missing("Dev".into())));
    assert_eq!(start_branch(&git, &gs(&dir), Some("dev ")), Err(BranchError::Missing("dev ".into())));
    // 태그·원격 추적 이름은 가지가 아니다
    g(&dir, &["tag", "v1"]);
    assert_eq!(start_branch(&git, &gs(&dir), Some("v1")), Err(BranchError::Missing("v1".into())));
}

#[test]
fn real_git_출발_가지_이름_공간이_있는_가지와_같은_이름_태그() {
    let e = Env::new();
    let dir = e.repo("tag");
    init_repo(&dir, "master", &["dev"]);
    g(&dir, &["tag", "dev"]); // 같은 이름 태그가 있어도 가지 이름을 그대로 읽는다(refname:short 의 heads/ 접두 함정)
    assert_eq!(name_of(start(&dir)), ("dev".into(), StartSource::Role("dev")));
    assert_eq!(list_branches(&ProcessGit::new(), &gs(&dir)).unwrap(), vec!["dev".to_string(), "master".to_string()]);
}

#[test]
fn real_git_저장소_경로_표기가_달라도_같은_판정() {
    let e = Env::new();
    let dir = e.repo("spell");
    init_repo(&dir, "master", &["dev"]);
    let back = gs(&dir); // 윈도우 기본 표기(`\`)
    let fwd = back.replace('\\', "/");
    let mut spellings = vec![back.clone(), fwd.clone(), format!("{back}\\"), format!("{fwd}/"), back.replace('\\', "\\\\").replace("\\\\", "\\")];
    if back.len() > 2 && back.as_bytes()[1] == b':' {
        let mut up = back.clone();
        up.replace_range(0..1, &back[0..1].to_ascii_uppercase());
        let mut low = back.clone();
        low.replace_range(0..1, &back[0..1].to_ascii_lowercase());
        spellings.extend([up.clone(), low.clone(), up.replace('\\', "/"), low.replace('\\', "/")]);
    }
    let git = ProcessGit::new();
    for sp in &spellings {
        assert_eq!(start_branch(&git, sp, None).unwrap().name, "dev", "{sp}");
        assert_eq!(list_branches(&git, sp).unwrap(), vec!["dev".to_string(), "master".to_string()], "{sp}");
        assert_eq!(current_branch(&git, sp).unwrap().as_deref(), Some("master"), "{sp}");
    }
}

#[test]
fn real_git_저장소가_아닌_폴더는_git_오류() {
    let e = Env::new();
    let dir = e.repo("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let err = start(&dir).unwrap_err();
    assert!(matches!(err, BranchError::Git(_)), "{err:?}");
    assert!(matches!(start_branch(&ProcessGit::new(), &gs(&e.repo("absent")), None), Err(BranchError::Git(_))));
}

// ── 가짜 git: 모든 판정에 닿는다 ─────────────────────

type Reply = Box<dyn Fn(&[&str]) -> PortResult<GitOutput> + Send + Sync>;

struct FakeGit {
    reply: Reply,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl FakeGit {
    fn new(f: impl Fn(&[&str]) -> PortResult<GitOutput> + Send + Sync + 'static) -> FakeGit {
        FakeGit { reply: Box::new(f), calls: Mutex::new(Vec::new()) }
    }
}

impl Git for FakeGit {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        self.calls.lock().unwrap().push((repo.to_string_lossy().into_owned(), args.iter().map(|s| s.to_string()).collect()));
        (self.reply)(args)
    }
}

fn ok(out: &str) -> PortResult<GitOutput> {
    Ok(GitOutput { code: 0, stdout: out.to_string(), stderr: String::new() })
}

fn fail(code: i32, err: &str) -> PortResult<GitOutput> {
    Ok(GitOutput { code, stdout: String::new(), stderr: err.to_string() })
}

/// 가지 목록·걸린 가지·커밋 유무를 정해 주는 가짜
fn repo_of(branches: &'static [&'static str], current: Option<&'static str>, commits: bool) -> FakeGit {
    FakeGit::new(move |a| match a[0] {
        "for-each-ref" => ok(&branches.iter().map(|b| format!("refs/heads/{b}\r\n")).collect::<String>()),
        "symbolic-ref" => match current {
            Some(c) => ok(&format!("{c}\n")),
            None => fail(1, ""),
        },
        "rev-parse" => if commits { ok("abc\n") } else { fail(128, "fatal: Needed a single revision") },
        other => fail(2, &format!("예상 밖 명령 {other}")),
    })
}

fn pick(f: &FakeGit, req: Option<&str>) -> Result<(String, StartSource), BranchError> {
    start_branch(f, "C:\\w\\a", req).map(|s| (s.name, s.source))
}

#[test]
fn fake_출발_가지_모든_판정에_닿는다() {
    use StartSource::*;
    // live 가 dev 보다, dev 가 master 보다 앞
    assert_eq!(pick(&repo_of(&["master", "dev", "live"], Some("master"), true), None), Ok(("live".into(), Role("live"))));
    assert_eq!(pick(&repo_of(&["staging", "live", "dev"], None, true), None), Ok(("live".into(), Role("live"))));
    assert_eq!(pick(&repo_of(&["staging", "develop"], None, true), None), Ok(("staging".into(), Role("live"))));
    assert_eq!(pick(&repo_of(&["main", "dev"], Some("main"), true), None), Ok(("dev".into(), Role("dev"))));
    assert_eq!(pick(&repo_of(&["develop", "master", "main"], None, true), None), Ok(("develop".into(), Role("dev"))));
    assert_eq!(pick(&repo_of(&["main"], Some("main"), true), None), Ok(("main".into(), Role("master"))));
    assert_eq!(pick(&repo_of(&["main", "master"], Some("main"), true), None), Ok(("master".into(), Role("master"))));
    assert_eq!(pick(&repo_of(&["trunk", "x"], Some("x"), true), None), Ok(("x".into(), Current)));
    // 걸린 가지가 목록에 없다(막 만든 빈 가지·이름 어긋남)면 고르지 않는다
    assert_eq!(pick(&repo_of(&["trunk"], Some("other"), true), None), Err(BranchError::NoStartBranch));
    assert_eq!(pick(&repo_of(&["trunk"], None, true), None), Err(BranchError::NoStartBranch));
    // 가지 없음: 커밋이 없으면 NoCommits, 있는데 가지가 없으면(떨어진 HEAD 뿐) NoStartBranch
    assert_eq!(pick(&repo_of(&[], Some("master"), false), None), Err(BranchError::NoCommits));
    assert_eq!(pick(&repo_of(&[], None, true), None), Err(BranchError::NoStartBranch));
    // 정해 준 가지
    assert_eq!(pick(&repo_of(&["a", "live"], None, true), Some("a")), Ok(("a".into(), Requested)));
    assert_eq!(pick(&repo_of(&["a", "live"], None, true), Some("b")), Err(BranchError::Missing("b".into())));
    assert_eq!(pick(&repo_of(&[], None, false), Some("a")), Err(BranchError::NoCommits));
    // 줄 끝 `\r` 은 가지 이름에 붙지 않는다
    assert_eq!(list_branches(&repo_of(&["a", "b"], None, true), "x").unwrap(), vec!["a".to_string(), "b".to_string()]);
    // 이름순
    assert_eq!(list_branches(&repo_of(&["z", "a/b", "m"], None, true), "x").unwrap(), vec!["a/b".to_string(), "m".to_string(), "z".to_string()]);
}

#[test]
fn fake_git_실패는_숨기지_않고_그대로_올라온다() {
    let broken = FakeGit::new(|_| Err(PortError::new(PortErrorKind::Unavailable, "git.exe 없음")));
    assert_eq!(start_branch(&broken, "C:\\x", None), Err(BranchError::Git("git.exe 없음".into())));
    assert_eq!(current_branch(&broken, "C:\\x"), Err(BranchError::Git("git.exe 없음".into())));
    let nonzero = FakeGit::new(|_| fail(128, "fatal: not a git repository\n"));
    assert_eq!(list_branches(&nonzero, "C:\\x"), Err(BranchError::Git("fatal: not a git repository".into())));
    // 명령에 쓴 인자와 저장소 자리가 그대로 git 으로 간다(셸을 거치지 않는다)
    let f = repo_of(&["master"], Some("master"), true);
    start_branch(&f, "C:\\w\\a b", None).unwrap();
    let calls = f.calls.lock().unwrap();
    assert!(calls.iter().all(|(r, _)| r == "C:\\w\\a b"));
    assert_eq!(calls[0].1, ["for-each-ref", "--format=%(refname)", "refs/heads/"]);
}

#[test]
fn fake_create_work_branch_는_switch_c_로_만든다() {
    let f = FakeGit::new(|a| match a[0] {
        "for-each-ref" => ok("refs/heads/master\n"),
        "switch" => ok(""),
        _ => fail(2, "?"),
    });
    create_work_branch(&f, "C:\\w\\a", "autopilot/r1", "master").unwrap();
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.last().unwrap().1, ["switch", "-c", "autopilot/r1", "master"]);
    drop(calls);
    let refused = FakeGit::new(|a| match a[0] {
        "for-each-ref" => ok(""),
        _ => fail(128, "fatal: invalid reference: nope\nmore"),
    });
    assert_eq!(create_work_branch(&refused, "C:\\w\\a", "autopilot/r1", "nope"), Err(BranchError::Git("fatal: invalid reference: nope".into())));
}

// ── run.json 기록 ────────────────────────────────────

fn work_rec(repo: &str) -> WorkRecord {
    WorkRecord {
        repo: repo.into(),
        base_branch: "live".into(),
        base_head: "a".repeat(40),
        work: "autopilot/r1".into(),
        original: Some("master".into()),
        original_head: "b".repeat(40),
        dirty_start: vec![" M a.txt".into(), "?? 한글.txt".into()],
    }
}

fn branches_of(extra: &Map<String, Value>) -> &Map<String, Value> {
    extra.get("branches").and_then(Value::as_object).expect("branches 가 객체")
}

#[test]
fn record_branch_는_저장소_키_아래_엔진이_읽는_꼴을_쓴다() {
    let mut extra = Map::new();
    record_branch(&mut extra, &work_rec("C:\\w\\a")).unwrap();
    let br = branches_of(&extra);
    let one = &br["repos"]["C:\\w\\a"];
    assert_eq!(one["base"], json!({"branch": "live", "head": "a".repeat(40)}));
    assert_eq!(one["work"], "autopilot/r1");
    assert_eq!(one["on"], "autopilot/r1");
    assert_eq!(one["original"], "master");
    assert_eq!(one["original_head"], "b".repeat(40));
    assert_eq!(one.get("restored"), Some(&Value::Null), "restored 칸이 있고 null 이다");
    assert_eq!(one["dirty_start"], json!([" M a.txt", "?? 한글.txt"]));
    assert_eq!(one["made"], json!(["autopilot/r1"]));
    assert_eq!(br["stories"], json!({}));
    // 떨어진 HEAD 에서 출발했으면 original 은 null
    let mut rec = work_rec("D:\\x");
    rec.original = None;
    record_branch(&mut extra, &rec).unwrap();
    assert_eq!(branches_of(&extra)["repos"]["D:\\x"].get("original"), Some(&Value::Null));
    // 저장소 둘이 함께 남는다
    assert_eq!(branches_of(&extra)["repos"].as_object().unwrap().len(), 2);
}

#[test]
fn record_branch_두_번째는_made_를_합치고_나머지는_새_값() {
    let mut extra = Map::new();
    record_branch(&mut extra, &work_rec("R")).unwrap();
    let mut rec = work_rec("R");
    rec.work = "autopilot/r2".into();
    record_branch(&mut extra, &rec).unwrap();
    let one = &branches_of(&extra)["repos"]["R"];
    assert_eq!(one["work"], "autopilot/r2");
    assert_eq!(one["made"], json!(["autopilot/r1", "autopilot/r2"]));
    record_branch(&mut extra, &rec).unwrap();
    assert_eq!(branches_of(&extra)["repos"]["R"]["made"], json!(["autopilot/r1", "autopilot/r2"]));
}

#[test]
fn record_story_branch_는_저장소별_기록을_쓰고_합친_줄을_지킨다() {
    let mut extra = Map::new();
    record_story_branch(&mut extra, "ABC-1", "R", "features/parent/ABC-1", "h1").unwrap();
    let st = branches_of(&extra)["stories"]["ABC-1"].clone();
    assert_eq!(st, json!({"repos": {"R": {"branch": "features/parent/ABC-1", "base": "h1", "head": "h1", "merged": [], "unmerged": []}}}));
    // 앞서 남긴 merged·unmerged 는 지킨다, 다른 저장소가 더해진다
    extra.get_mut("branches").unwrap()["stories"]["ABC-1"]["repos"]["R"]["merged"] = json!([{"task": "t"}]);
    extra.get_mut("branches").unwrap()["stories"]["ABC-1"]["repos"]["R"]["unmerged"] = json!([{"task": "u"}]);
    record_story_branch(&mut extra, "ABC-1", "R", "features/parent/ABC-1", "h2").unwrap();
    record_story_branch(&mut extra, "ABC-1", "S", "features/parent/ABC-1", "h3").unwrap();
    let rs = story_repos(branches_of(&extra)["stories"].get("ABC-1"));
    assert_eq!(rs.len(), 2);
    assert_eq!(rs["R"]["head"], "h2");
    assert_eq!(rs["R"]["merged"], json!([{"task": "t"}]));
    assert_eq!(rs["R"]["unmerged"], json!([{"task": "u"}]));
    assert_eq!(rs["S"]["head"], "h3");
    // 옛 꼴(스토리 하나에 저장소 하나)은 새 꼴로 옮겨진다
    let mut old = Map::new();
    old.insert("branches".into(), json!({"stories": {"K": {"repo": "R", "branch": "b", "base": "x", "head": "x", "merged": [1], "unmerged": []}}}));
    record_story_branch(&mut old, "K", "R", "b", "y").unwrap();
    assert_eq!(branches_of(&old)["stories"]["K"]["repos"]["R"]["merged"], json!([1]));
    assert_eq!(branches_of(&old)["stories"]["K"]["repos"]["R"]["head"], "y");
    assert!(branches_of(&old)["stories"]["K"].get("repo").is_none());
}

#[test]
fn 기록_꼴이_틀린_run_json_은_덮어쓰지_않고_오류() {
    let mut a = Map::new();
    a.insert("branches".into(), json!("망가짐"));
    assert!(matches!(record_branch(&mut a, &work_rec("R")), Err(BranchError::Git(_))));
    assert_eq!(a["branches"], "망가짐");
    let mut b = Map::new();
    b.insert("branches".into(), json!({"repos": []}));
    assert!(record_branch(&mut b, &work_rec("R")).is_err());
    assert_eq!(b["branches"], json!({"repos": []}));
    let mut c = Map::new();
    c.insert("branches".into(), json!({"stories": 3}));
    assert!(record_story_branch(&mut c, "K", "R", "b", "h").is_err());
    assert_eq!(c["branches"], json!({"stories": 3}));
}

const RUN_JSON: &str = r#"{"state":"starting","project":"p","unknown_field":{"a":1},"updated":1.5}"#;

/// run.json 글 → 기록 → 글 → 다시 읽기. 규칙이 읽는 것까지 한 바퀴
fn round_trip(f: impl FnOnce(&mut Map<String, Value>)) -> Map<String, Value> {
    let mut run: RunFile = from_slice(RUN_JSON.as_bytes()).unwrap();
    f(&mut run.extra);
    let text = to_pretty_json(&run).unwrap();
    let back: RunFile = from_slice(text.as_bytes()).unwrap();
    assert_eq!(back.extra.get("unknown_field"), Some(&json!({"a": 1})), "모르는 칸은 지켜진다");
    back.extra.get("branches").and_then(Value::as_object).cloned().expect("branches 가 남는다")
}

#[test]
fn 기록_한_바퀴_규칙이_읽는다() {
    let br = round_trip(|extra| {
        record_branch(extra, &work_rec("C:\\w\\a")).unwrap();
        record_story_branch(extra, "ABC-2", "C:\\w\\a", "features/parent/ABC-2", "h2").unwrap();
        record_story_branch(extra, "ABC-1", "C:\\w\\a", "features/parent/ABC-1", "h1").unwrap();
    });
    // 스토리 가지가 있는 스토리만 합칠 쪽이고, 키 차례다
    assert_eq!(merge_sources(&br, false), ["story:ABC-1", "story:ABC-2"]);
    assert_eq!(merge_sources(&br, true), ["autopilot", "story:ABC-1", "story:ABC-2"]);
    let rs = story_repos(br["stories"].get("ABC-1"));
    assert_eq!(rs["C:\\w\\a"]["branch"], "features/parent/ABC-1");
    assert!(rs["C:\\w\\a"].contains_key("head"));
    // 아직 아무것도 합치지 않았다: 기다림
    assert_eq!(merge_mark(&br, false), Some(MergeMark::Waiting));
    assert!(merge_open(&br, &[Step::new("autopilot", "dev", None)], false));
    assert!(promoted_rows(&br).is_empty());
    assert!(failed_rows(&br).is_empty());
}

#[test]
fn 기록_한_바퀴_스토리가_없으면_자율주행_가지가_합칠_쪽() {
    let br = round_trip(|extra| record_branch(extra, &work_rec("R")).unwrap());
    assert_eq!(merge_sources(&br, false), ["autopilot"]);
    assert_eq!(merge_mark(&br, false), Some(MergeMark::Waiting));
}

#[test]
fn 기록_한_바퀴_엔진_finish_의_합친_줄을_붙이면_합침으로_읽힌다() {
    let br = round_trip(|extra| {
        record_branch(extra, &work_rec("R")).unwrap();
        extra.get_mut("branches").unwrap().as_object_mut().unwrap().insert(
            "promoted".into(),
            json!([{"source": "autopilot", "repo": "R", "branch": "autopilot/r1", "into": "live", "how": "merge", "to": "c0ffee", "at": 5}]),
        );
    });
    assert_eq!(promoted_rows(&br).len(), 1);
    assert_eq!(merge_mark(&br, false), Some(MergeMark::Promoted("live".into())));
    assert!(!merge_open(&br, &[Step::new("autopilot", "live", None)], false));
}

#[test]
fn 기록_저장소_키는_글_그대로라_표기가_다르면_다른_저장소() {
    // 규칙(`story_repos`·`merge_open` 의 repo 견줌)이 글 그대로 견주므로 기록 쪽도 고치지 않는다 — 어긋나면 합치기가 안 맞으니 호출자가 한 표기를 쓴다
    let br = round_trip(|extra| {
        record_branch(extra, &work_rec("C:\\w\\a")).unwrap();
        record_branch(extra, &work_rec("C:/w/a")).unwrap();
    });
    let repos = br["repos"].as_object().unwrap();
    assert_eq!(repos.len(), 2);
    assert!(repos.contains_key("C:\\w\\a") && repos.contains_key("C:/w/a"));
}

#[test]
fn 기록_한_바퀴_저장소_경로는_한글과_공백도_그대로() {
    let key = "C:\\내 문서\\a b";
    let br = round_trip(|extra| {
        record_branch(extra, &work_rec(key)).unwrap();
        record_story_branch(extra, "S", key, "features/parent/S", "h").unwrap();
    });
    assert!(br["repos"].as_object().unwrap().contains_key(key));
    assert!(story_repos(br["stories"].get("S")).contains_key(key));
}

#[test]
fn real_git_출발에서_기록까지_이어_돈다() {
    // 출발 가지 고르기 → 작업 가지 만들기 → 기록 → 규칙이 읽기. 머리 커밋은 진짜 git 이 준 값
    let e = Env::new();
    let dir = e.repo("flow");
    init_repo(&dir, "master", &["dev", "live"]);
    let git = ProcessGit::new();
    let r = gs(&dir);
    let base = start_branch(&git, &r, None).unwrap();
    assert_eq!(base.name, "live");
    let original = current_branch(&git, &r).unwrap();
    let base_head = g(&dir, &["rev-parse", &format!("refs/heads/{}", base.name)]).trim().to_string();
    let original_head = g(&dir, &["rev-parse", "HEAD"]).trim().to_string();
    let work = branch_name("flow-1").unwrap();
    create_work_branch(&git, &r, &work, &base.name).unwrap();
    let br = round_trip(|extra| {
        record_branch(extra, &WorkRecord { repo: r.clone(), base_branch: base.name.clone(), base_head: base_head.clone(), work: work.clone(), original, original_head, dirty_start: vec![] })
            .unwrap();
    });
    assert_eq!(br["repos"][&r]["base"]["head"], Value::from(base_head.clone()));
    assert_eq!(br["repos"][&r]["on"], "autopilot/flow-1");
    assert_eq!(g(&dir, &["rev-parse", "HEAD"]).trim(), base_head);
    assert_eq!(merge_mark(&br, false), Some(MergeMark::Waiting));
}
