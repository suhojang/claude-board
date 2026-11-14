//! 새 폴더 만들기 규칙(`rules::new_repo`) 시험. 원본: `Tests/AutopilotRulesTests/TopicTests.swift` 의 `NewRepoTests` 전부
//! (NewRepo.swift 를 쓰는 시험 파일은 이것뿐이다. 같은 파일의 Stack·Link 시험은 다른 원본의 몫이다).
//! 옮긴 시험: `NewRepoTests.표`(`표`) · `차례` · `만들기` · `실패`. 표(16행)는 Swift 에서 변환 스크립트로 옮겼다(손으로 다시 치지 않았다).
//! 끝의 `extra_`·`windows_` 시험은 Swift 시험에 없는 것(ICU `$`·자소 수·위치 규칙·유스케이스 경계·Windows 이름 규칙)이다.

use claude_board_autopilot::rules::new_repo::{
    create_repo, new_repo_check, new_repo_check_windows, new_repo_name_problem_windows, new_repo_steps, Folders, GitRunner, NewRepoPlan, PathState,
    NEW_REPO_BRANCHES, NEW_REPO_MESSAGE,
};
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

struct Row {
    note: &'static str,
    parent: &'static str,
    name: &'static str,
    parent_state: PathState,
    target_state: PathState,
    want: &'static str, // 계획 경로 또는 첫 고칠 것
}

const NEW_REPO_ROWS: &[Row] = &[
    Row { note: "보통: ~/workspace 아래 새 이름", parent: "/Users/u/workspace", name: "room-booking", parent_state: PathState::Dir, target_state: PathState::Missing, want: "/Users/u/workspace/room-booking" },
    Row { note: "끝의 / 는 뗀다", parent: "/Users/u/workspace/", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "/Users/u/workspace/todo" },
    Row { note: "빈 폴더는 쓴다", parent: "/Users/u/workspace", name: "todo", parent_state: PathState::Dir, target_state: PathState::EmptyDir, want: "/Users/u/workspace/todo" },
    Row { note: "무엇이 든 폴더에는 만들지 않는다", parent: "/Users/u/workspace", name: "todo", parent_state: PathState::Dir, target_state: PathState::Dir, want: "그 자리에 이미 무엇이 든 폴더가 있습니다. 기존 저장소로 고르거나 다른 이름을 씁니다" },
    Row { note: "이름이 비었다", parent: "/Users/u/workspace", name: " ", parent_state: PathState::Dir, target_state: PathState::Missing, want: "새 폴더 이름을 넣어 주세요" },
    Row { note: "이름에 빈칸·한글", parent: "/Users/u/workspace", name: "회의실 예약", parent_state: PathState::Dir, target_state: PathState::Missing, want: "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)" },
    Row { note: "점으로 시작하는 이름", parent: "/Users/u/workspace", name: ".hidden", parent_state: PathState::Dir, target_state: PathState::Missing, want: "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)" },
    Row { note: "이름에 / (다른 자리로 빠지기)", parent: "/Users/u/workspace", name: "a/../../x", parent_state: PathState::Dir, target_state: PathState::Missing, want: "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)" },
    Row { note: "홈 밖", parent: "/tmp", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "위치는 홈 폴더 아래여야 합니다" },
    Row { note: "앱 자리 안", parent: "/Users/u/Library/Application Support", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다" },
    Row { note: "엔진 작업 트리 자리 안", parent: "/Users/u/.claude-board/worktrees", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다" },
    Row { note: "위치에 ..", parent: "/Users/u/workspace/../Library", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "위치에 . 이나 .. 을 쓰지 않습니다" },
    Row { note: "위치 폴더가 없다", parent: "/Users/u/nowhere", name: "todo", parent_state: PathState::Missing, target_state: PathState::Missing, want: "위치 폴더가 없습니다" },
    Row { note: "상대 경로 위치", parent: "workspace", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "위치는 / 로 시작하는 경로로 씁니다" },
    Row { note: "홈 바로 아래도 된다", parent: "/Users/u", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "/Users/u/todo" },
    Row { note: "비슷한 이름의 다른 홈은 홈 밖", parent: "/Users/u2/workspace", name: "todo", parent_state: PathState::Dir, target_state: PathState::Missing, want: "위치는 홈 폴더 아래여야 합니다" },
];

const HOME: &str = "/Users/u";
const NAME_PROBLEM: &str = "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)";

fn branches_ok(p: &NewRepoPlan) -> bool {
    p.branches == ["dev", "live", "master"] && p.head == "dev" && p.base == "live"
}

#[test]
fn 표() {
    for r in NEW_REPO_ROWS {
        let (plan, problems) = new_repo_check(r.parent, r.name, HOME, r.parent_state, r.target_state);
        let got = plan.as_ref().map(|p| p.path.clone()).or_else(|| problems.first().cloned()).unwrap_or_else(|| "?".to_string());
        assert_eq!(got, r.want, "{}", r.note);
        if let Some(p) = &plan {
            assert!(branches_ok(p), "{}", r.note);
            assert!(problems.is_empty(), "{}", r.note);
        }
    }
}

fn todo_plan() -> NewRepoPlan {
    new_repo_check("/Users/u/workspace", "todo", HOME, PathState::Dir, PathState::Missing).0.unwrap()
}

fn has(v: &[String], s: &str) -> bool {
    v.iter().any(|x| x == s)
}

#[test]
fn 차례() {
    let p = todo_plan();
    let s = new_repo_steps(&p, Some(("김 개발", "dev@example.com")));
    assert_eq!(s.len(), 4);
    assert_eq!(s[0], ["init", "-q", "-b", "dev"]);
    assert!(has(&s[1], "user.name=김 개발") && has(&s[1], "commit.gpgsign=false") && has(&s[1], "--allow-empty") && has(&s[1], "--no-verify"));
    assert_eq!(s[2], ["branch", "live"]);
    assert_eq!(s[3], ["branch", "master"]);
    assert!(has(&new_repo_steps(&p, None)[1], "user.name=Claude Board"));
}

struct FakeFolders {
    st: Mutex<HashMap<String, PathState>>,
    made: Mutex<Vec<String>>,
    can_make: bool,
}
impl FakeFolders {
    fn new(s: &[(&str, PathState)]) -> Self {
        FakeFolders { st: Mutex::new(s.iter().map(|(k, v)| (k.to_string(), *v)).collect()), made: Mutex::new(Vec::new()), can_make: true }
    }
}
impl Folders for FakeFolders {
    fn state(&self, path: &str) -> PathState {
        self.st.lock().unwrap().get(path).copied().unwrap_or(PathState::Missing)
    }
    fn make_dir(&self, path: &str) -> bool {
        if !self.can_make {
            return false;
        }
        self.made.lock().unwrap().push(path.to_string());
        self.st.lock().unwrap().insert(path.to_string(), PathState::EmptyDir);
        true
    }
}

struct FakeGit {
    ran: Mutex<Vec<Vec<String>>>,
    fail_at: Option<&'static str>,
    head: &'static str,
    branches: Mutex<BTreeSet<String>>,
}
impl FakeGit {
    fn new() -> Self {
        FakeGit { ran: Mutex::new(Vec::new()), fail_at: None, head: "c0ffee1234567890", branches: Mutex::new(BTreeSet::new()) }
    }
}
impl GitRunner for FakeGit {
    fn run(&self, args: &[String], _dir: &str) -> (i32, String) {
        self.ran.lock().unwrap().push(args.to_vec());
        let verb = args.iter().find(|a| !a.starts_with('-') && !a.contains('=')).map(String::as_str).unwrap_or("");
        if Some(verb) == self.fail_at {
            return (128, "fatal: 가짜 실패".to_string());
        }
        let mut br = self.branches.lock().unwrap();
        match verb {
            "init" => br.clear(),
            "commit" => {
                br.insert("dev".to_string());
            }
            "branch" => {
                br.insert(args.last().cloned().unwrap_or_default());
            }
            "rev-parse" => {
                let last = args.last().cloned().unwrap_or_default();
                let name: String = last.chars().skip(11).collect();
                return if br.contains(&name) { (0, self.head.to_string()) } else { (1, String::new()) };
            }
            _ => {}
        }
        (0, String::new())
    }
    fn identity(&self) -> Option<(String, String)> {
        None
    }
}

#[test]
fn 만들기() {
    let plan = todo_plan();
    let f = FakeFolders::new(&[]);
    let g = FakeGit::new();
    let r = create_repo(&plan, &f, &g);
    assert!(r.ok && r.made && r.commit.as_deref() == Some("c0ffee1234567890") && r.error.is_none());
    assert_eq!(*f.made.lock().unwrap(), ["/Users/u/workspace/todo"]);
    assert_eq!(g.ran.lock().unwrap().iter().filter(|a| a.first().map(String::as_str) != Some("rev-parse")).count(), 4);
    assert_eq!(r.done, ["폴더를 만들었습니다", "git init(처음 가지 dev)", "빈 첫 커밋 c0ffee1 에 가지 dev·live·master"]);
}

#[test]
fn 실패() {
    let plan = todo_plan();
    let f = FakeFolders::new(&[]);
    let mut g = FakeGit::new();
    g.fail_at = Some("commit");
    let r = create_repo(&plan, &f, &g);
    assert!(!r.ok && r.made);
    assert_eq!(r.error.as_deref(), Some("git commit 이 실패했습니다(종료 코드 128): fatal: 가짜 실패"));
    assert!(!g.ran.lock().unwrap().iter().any(|a| a.first().map(String::as_str) == Some("branch")));
    // 그사이 누가 그 자리에 파일을 두었으면 만들지 않는다
    let f2 = FakeFolders::new(&[("/Users/u/workspace/todo", PathState::Dir)]);
    let g2 = FakeGit::new();
    let r2 = create_repo(&plan, &f2, &g2);
    assert!(!r2.ok && g2.ran.lock().unwrap().is_empty() && f2.made.lock().unwrap().is_empty());
    // 폴더를 만들지 못함
    let mut f3 = FakeFolders::new(&[]);
    f3.can_make = false;
    assert_eq!(create_repo(&plan, &f3, &FakeGit::new()).error.as_deref(), Some("폴더를 만들지 못했습니다: /Users/u/workspace/todo"));
}

// ---------- Swift 시험에 없는 경계 ----------

#[test]
fn extra_constants() {
    assert_eq!(NEW_REPO_BRANCHES, ["dev", "live", "master"]);
    assert_eq!(NEW_REPO_MESSAGE, "빈 첫 커밋(Claude Board 자율주행이 새 폴더로 만듦)");
    assert_eq!(todo_plan().message, NEW_REPO_MESSAGE);
}

fn first_problem(name: &str) -> Option<String> {
    new_repo_check("/Users/u/workspace", name, HOME, PathState::Dir, PathState::Missing).1.into_iter().next()
}

fn plan_for(name: &str) -> Option<NewRepoPlan> {
    new_repo_check("/Users/u/workspace", name, HOME, PathState::Dir, PathState::Missing).0
}

#[test]
fn extra_name_length() {
    assert!(plan_for(&"a".repeat(64)).is_some());
    assert_eq!(first_problem(&"a".repeat(65)).as_deref(), Some("새 폴더 이름은 64자까지입니다"));
    // 앞뒤 빈칸·탭은 깎는다
    assert!(plan_for(" \t\u{3000}todo\u{a0}").is_some());
    // 결합 글자는 앞 글자와 한 묶음이다: 64 묶음(= 65 char)은 글자 수 문제가 아니라 이름 규칙 문제다
    assert_eq!(first_problem(&format!("{}\u{301}", "a".repeat(64))).as_deref(), Some(NAME_PROBLEM));
    // \r\n 은 한 묶음이다: a*63 + "\r\n" = 64 묶음(줄끝 하나는 ICU `$` 가 허용한다)
    assert!(plan_for(&format!("{}\r\n", "a".repeat(63))).is_some());
    assert_eq!(first_problem(&format!("{}\r\n", "a".repeat(64))).as_deref(), Some("새 폴더 이름은 64자까지입니다"));
}

#[test]
fn extra_icu_dollar_allows_one_trailing_line_terminator() {
    for t in ["\n", "\r", "\r\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"] {
        let n = format!("todo{t}");
        assert!(plan_for(&n).is_some(), "{n:?}");
        assert!(new_repo_check_windows("/Users/u/workspace", &n, HOME, PathState::Dir, PathState::Missing).0.is_none(), "{n:?}");
    }
    // 둘 이상이거나 줄끝 뒤에 글자가 오면 거절
    for n in ["todo\n\n", "todo\nx", "todo\r\n\n", "todo\n\r"] {
        assert_eq!(first_problem(n).as_deref(), Some(NAME_PROBLEM), "{n:?}");
    }
}

#[test]
fn extra_place_rules() {
    let chk = |parent: &str| new_repo_check(parent, "todo", HOME, PathState::Dir, PathState::Missing);
    // 끝의 슬래시 여럿·앞뒤 빈칸
    assert_eq!(chk("  /Users/u//  ").0.unwrap().path, "/Users/u/todo");
    // 홈 끝의 슬래시는 뗀다
    assert!(new_repo_check("/Users/u/work", "todo", "/Users/u/", PathState::Dir, PathState::Missing).0.is_some());
    // 접두사 바로 뒤의 결합 글자는 홈 아래가 아니다
    assert_eq!(chk("/Users/u/\u{301}x").1, ["위치는 홈 폴더 아래여야 합니다"]);
    // 막힌 자리 넷과 그 아래
    let blocked = "위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다";
    for d in ["/Users/u/Library", "/Users/u/.claude", "/Users/u/.claude-board", "/Users/u/.Trash", "/Users/u/.claude/x/y", "/Users/u/.Trash/a"] {
        assert_eq!(chk(d).1, [blocked], "{d}");
    }
    // 비슷한 이름은 막히지 않는다
    assert!(chk("/Users/u/.claude2").0.is_some());
    assert!(chk("/Users/u/Libraryx").0.is_some());
    // . 이나 .. 이 한 조각일 때만 거절한다
    assert_eq!(chk("/Users/u/./x").1, ["위치에 . 이나 .. 을 쓰지 않습니다"]);
    assert_eq!(chk("/Users/u/x/..").1, ["위치에 . 이나 .. 을 쓰지 않습니다"]);
    assert!(chk("/Users/u/.x/..y").0.is_some());
    // 위치 상태
    for (st, msg) in [(PathState::Missing, "위치 폴더가 없습니다"), (PathState::Other, "위치가 폴더가 아닙니다")] {
        assert_eq!(new_repo_check("/Users/u/w", "todo", HOME, st, PathState::Missing).1, [msg]);
    }
    assert!(new_repo_check("/Users/u/w", "todo", HOME, PathState::EmptyDir, PathState::Missing).0.is_some());
    // 자리가 파일이면 거절. 위치 문제가 있으면 자리 상태는 보지 않는다
    assert_eq!(new_repo_check("/Users/u/w", "todo", HOME, PathState::Dir, PathState::Other).1, ["그 자리에 폴더가 아닌 것이 있습니다"]);
    assert_eq!(
        new_repo_check("/tmp", " ", HOME, PathState::Dir, PathState::Dir).1,
        ["새 폴더 이름을 넣어 주세요", "위치는 홈 폴더 아래여야 합니다"]
    );
    // 홈이 "/" 여도 위치가 / 로 시작하면 아래다
    assert!(new_repo_check("/work", "todo", "/", PathState::Dir, PathState::Missing).0.is_some());
}

#[test]
fn extra_steps_without_extra_branches() {
    let mut p = todo_plan();
    p.branches = vec!["dev".to_string()];
    assert_eq!(new_repo_steps(&p, None).len(), 2);
    let s = new_repo_steps(&p, Some(("a", "b@c")));
    assert_eq!(s[1][..6], ["-c", "user.name=a", "-c", "user.email=b@c", "-c", "commit.gpgsign=false"]);
    assert_eq!(s[1][6..], ["commit", "-q", "--allow-empty", "--no-verify", "-m", NEW_REPO_MESSAGE]);
    assert_eq!(new_repo_steps(&p, None)[1][1], "user.name=Claude Board");
    assert_eq!(new_repo_steps(&p, None)[1][3], "user.email=claude-board@localhost");
}

struct Quiet;
impl GitRunner for Quiet {
    fn run(&self, args: &[String], _d: &str) -> (i32, String) {
        if args[0] == "init" {
            (1, String::new())
        } else {
            (0, String::new())
        }
    }
    fn identity(&self) -> Option<(String, String)> {
        Some(("n".to_string(), "e@x".to_string()))
    }
}

struct Gap(&'static str);
impl GitRunner for Gap {
    fn run(&self, args: &[String], _d: &str) -> (i32, String) {
        if args[0] == "rev-parse" {
            if args.last().unwrap().ends_with(self.0) {
                return (1, String::new());
            }
            return (0, "abc1234".to_string());
        }
        (0, String::new())
    }
    fn identity(&self) -> Option<(String, String)> {
        None
    }
}

struct Blank;
impl GitRunner for Blank {
    fn run(&self, _args: &[String], _d: &str) -> (i32, String) {
        (0, String::new())
    }
    fn identity(&self) -> Option<(String, String)> {
        None
    }
}

struct Split;
impl GitRunner for Split {
    fn run(&self, args: &[String], _d: &str) -> (i32, String) {
        if args[0] == "rev-parse" {
            return (0, format!("c{}", args.last().unwrap().len()));
        }
        (0, String::new())
    }
    fn identity(&self) -> Option<(String, String)> {
        None
    }
}

struct Spy(Mutex<Vec<Vec<String>>>);
impl GitRunner for Spy {
    fn run(&self, args: &[String], _d: &str) -> (i32, String) {
        self.0.lock().unwrap().push(args.to_vec());
        if args[0] == "rev-parse" {
            return (0, "abcdef0123".to_string());
        }
        (0, String::new())
    }
    fn identity(&self) -> Option<(String, String)> {
        Some(("김 개발".to_string(), "dev@example.com".to_string()))
    }
}

#[test]
fn extra_create_edges() {
    let plan = todo_plan();
    // 빈 폴더는 그대로 쓴다
    let f = FakeFolders::new(&[("/Users/u/workspace/todo", PathState::EmptyDir)]);
    let r = create_repo(&plan, &f, &FakeGit::new());
    assert!(r.ok && !r.made && f.made.lock().unwrap().is_empty());
    assert_eq!(r.done[0], "빈 폴더를 그대로 씁니다");
    // 파일이면 만들지 않는다(문구까지)
    let f2 = FakeFolders::new(&[("/Users/u/workspace/todo", PathState::Other)]);
    let r2 = create_repo(&plan, &f2, &FakeGit::new());
    assert_eq!(r2.error.as_deref(), Some("그 자리에 이미 무엇이 있어 만들지 않았습니다: /Users/u/workspace/todo"));
    assert!(!r2.made && r2.commit.is_none());
    // 실패 문구: 출력이 없으면 ": " 를 붙이지 않는다
    assert_eq!(create_repo(&plan, &FakeFolders::new(&[]), &Quiet).error.as_deref(), Some("git init 이 실패했습니다(종료 코드 1)"));
    // 가지 확인: 한 가지가 없으면 그 이름을 낸다
    let r3 = create_repo(&plan, &FakeFolders::new(&[]), &Gap("master"));
    assert!(!r3.ok && r3.commit.is_none());
    assert_eq!(r3.error.as_deref(), Some("가지 master 를 만들지 못했습니다"));
    // 종료 코드 0 이어도 커밋 해시가 비었으면 그 가지를 못 만든 것이다
    assert_eq!(create_repo(&plan, &FakeFolders::new(&[]), &Blank).error.as_deref(), Some("가지 dev 를 만들지 못했습니다"));
    // 가지가 다른 커밋을 가리키면 거절한다
    let r4 = create_repo(&plan, &FakeFolders::new(&[]), &Split);
    assert!(!r4.ok && r4.commit.is_none());
    assert_eq!(r4.error.as_deref(), Some("가지가 같은 첫 커밋을 가리키지 않습니다"));
    // 사람은 git 설정에서 받아 커밋 인자에 쓴다
    let spy = Spy(Mutex::new(Vec::new()));
    let r5 = create_repo(&plan, &FakeFolders::new(&[]), &spy);
    assert!(r5.ok && r5.commit.as_deref() == Some("abcdef0123"));
    let ran = spy.0.lock().unwrap();
    assert!(has(&ran[1], "user.email=dev@example.com") && has(&ran[1], "user.name=김 개발"));
    assert_eq!(ran[4], ["rev-parse", "--verify", "-q", "refs/heads/dev"]);
}

// ---------- Windows 전용 ----------

#[test]
fn windows_names() {
    let bad_dot = "Windows 는 이름 끝의 점을 떼어 버립니다. 끝에 . 을 쓰지 않습니다";
    let reserved = "Windows 가 장치 이름으로 쓰는 이름입니다(CON·PRN·AUX·NUL·COM1~9·LPT1~9). 다른 이름을 씁니다";
    // 원본 규칙이 통과시키는 Windows 불가 이름
    for n in ["todo.", "todo..", "a."] {
        assert!(first_problem(n).is_none(), "1:1 은 {n} 을 통과시킨다");
        assert_eq!(new_repo_name_problem_windows(n).as_deref(), Some(bad_dot), "{n}");
    }
    for n in ["CON", "con", "Nul", "PRN", "aux", "COM1", "com9", "LPT1", "lpt9", "con.txt", "NUL.tar.gz", "Com3.x"] {
        assert!(first_problem(n).is_none(), "1:1 은 {n} 을 통과시킨다");
        assert_eq!(new_repo_name_problem_windows(n).as_deref(), Some(reserved), "{n}");
    }
    // 예약이 아닌 비슷한 이름
    for n in ["COM0", "COM10", "LPT0", "CONSOLE", "console", "nulx", "aux1", "com", "todo", "a.b", "x-y_z.1", "con-x", "x.con"] {
        assert_eq!(new_repo_name_problem_windows(n), None, "{n}");
    }
    // 앞뒤 빈칸은 깎고 본다 / 빈 이름 / 규칙 밖 글자
    assert_eq!(new_repo_name_problem_windows(" todo "), None);
    assert_eq!(new_repo_name_problem_windows(" con ").as_deref(), Some(reserved));
    assert_eq!(new_repo_name_problem_windows("  ").as_deref(), Some("새 폴더 이름을 넣어 주세요"));
    for n in [".hidden", "a b", "a\\b", "a:b", "a*", "a?", "a\"", "a<", "a>", "a|", "회의실", "-x", "_x", "todo\n"] {
        assert_eq!(new_repo_name_problem_windows(n).as_deref(), Some(NAME_PROBLEM), "{n:?}");
    }
}

#[test]
fn windows_check() {
    let ck = |name: &str| new_repo_check_windows("/Users/u/workspace", name, HOME, PathState::Dir, PathState::Missing);
    // 정상 이름은 1:1 과 같은 계획
    let (p, pr) = ck("room-booking");
    assert_eq!(p, plan_for("room-booking"));
    assert!(pr.is_empty());
    // Windows 불가 이름은 계획 없이 문제 한 줄
    let (p, pr) = ck("con");
    assert!(p.is_none() && pr.len() == 1);
    assert!(ck("todo.").0.is_none());
    // 원본 문제가 있으면 그대로 낸다(이름이 예약어여도 위치 문제가 먼저)
    assert_eq!(
        new_repo_check_windows("/tmp", "con", HOME, PathState::Dir, PathState::Missing),
        new_repo_check("/tmp", "con", HOME, PathState::Dir, PathState::Missing)
    );
}
