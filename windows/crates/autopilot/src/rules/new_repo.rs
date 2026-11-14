//! 주제로 시작의 "새 폴더 만들기" 규칙: 이름·자리 확인 → 계획, git 차례, 만들기 유스케이스. 원본: `Sources/AutopilotRules/NewRepo.swift`(전체).
//! 이름·자리 확인과 git 차례는 값으로 정하고, 폴더·git 은 트레이트(`Folders`·`GitRunner`)로만 부른다(순수 층: 파일·시각·환경 없음).
//!
//! 이식된 선언(원본 순서): `NewRepoPlan`(APNewRepoPlan) · `NEW_REPO_BRANCHES`(apNewRepoBranches) · `NEW_REPO_MESSAGE`(apNewRepoMessage) ·
//! `new_repo_check`(apNewRepoCheck) · `new_repo_steps`(apNewRepoSteps) · `CreateResult`(APCreateResult) · `create_repo`(apCreateRepo).
//! 같은 파일에서 쓰는 포트 선언은 원본 `Ports.swift` 의 몫이지만 `ports.rs` 가 통합자 소유라 여기에 둔다: `PathState`(APPathState) ·
//! `Folders`(APFolders) · `GitRunner`(APGitRunner). (엔진의 `ports::Git` 은 `&Path` 와 `GitOutput` 을 쓰는 다른 꼴이라 어댑터가 맞춘다.)
//!
//! Windows 전용 덧붙임(원본에 없음, 1:1 함수의 동작은 그대로다):
//! - `new_repo_name_problem_windows` · `new_repo_check_windows`. 원본의 이름 규칙(`^[A-Za-z0-9][A-Za-z0-9._-]*$`)은 Windows 가 거부하는 이름을 통과시킨다:
//!   끝이 `.` 인 이름(`todo.` — Windows 는 끝의 점을 떼어 다른 폴더가 된다), 예약 장치 이름(`CON`·`PRN`·`AUX`·`NUL`·`COM1`~`COM9`·`LPT1`~`LPT9`,
//!   `con.txt` 처럼 첫 점 앞이 예약 이름이어도 같다. 대소문자 무시). 끝 빈칸·`\`·`:`·`*`·`?`·`"`·`<`·`>`·`|` 는 원본 규칙이 이미 막는다(`$` 의 줄끝 예외만 아래).
//!   `new_repo_check_windows` 는 원본 확인이 계획을 낼 때만 이름을 한 번 더 본다(원본 문제가 있으면 그 문제를 그대로 낸다).
//!   위치(부모) 규칙의 `/` 시작·홈 아래 판정은 원본 그대로 POSIX 꼴이다. Windows 경로(`C:\…`)를 `/` 꼴로 바꾸는 일은 호출자 몫이다.
//!
//! Foundation 과 다른 점:
//! - 원본 이름 규칙은 `range(of:options:.regularExpression)`(ICU)이다. 바이트 스캐너로 옮겼다. ICU 의 `$` 는 입력 끝 앞의 줄 끝 글자 하나
//!   (`\n`·`\v`·`\f`·`\r`·U+0085·U+2028·U+2029, 또는 `\r\n`)도 허용하므로 `todo\n` 은 원본이 통과시킨다(공백 깎기는 `\n` 을 못 깎는다). 여기도 같이 통과시키고,
//!   `*_windows` 가 막는다. 이 줄끝 허용은 Mac 에서 돌려 본 값이 아니라 ICU 문서의 동작이다.
//! - `trimmingCharacters(in: .whitespaces)` 는 Zs 와 탭이다(`is_cs_whitespace`, stack.rs 와 같은 표).
//! - 글자 수(`n.count > 64`)는 원본이 자소 묶음(`Character`) 단위다. 여기는 `char` 에서 결합 글자(stack.rs 의 근사 목록 `is_extend`)를 빼고 `\r\n` 을 하나로 센다.
//!   한글 자모 조합 등 목록 밖 확장 글자는 모른다(64 근처의 비ASCII 이름에서만 어느 문제 문구가 나오는지 갈릴 수 있고, 어느 쪽이든 문제가 난다).
//! - `hasPrefix`·`hasSuffix`·`==` 는 원본이 정준 동치·자소 묶음 기준이다. 여기는 바이트 열 비교이고, 접두사 바로 뒤에 결합 글자가 붙으면 접두사로 보지 않는다.
//! - `args.first { … }`(실패한 git 명령 이름)의 `hasPrefix("-")`·`contains("=")` 는 자소 묶음 대신 바이트 열로 센다. `c.prefix(7)` 는 `char` 7 개다(커밋 해시는 16진 ASCII).

use std::collections::BTreeSet;

/// 새 폴더 계획 — 만들 자리, 가지(빈 첫 커밋에 모두), 처음 걸어 둘 가지, 출발 가지. 원본: `APNewRepoPlan`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRepoPlan {
    pub path: String,
    pub branches: Vec<String>,
    pub head: String,
    pub base: String,
    pub message: String,
}

/// 원본: `apNewRepoBranches`
pub const NEW_REPO_BRANCHES: [&str; 3] = ["dev", "live", "master"];
/// 원본: `apNewRepoMessage`
pub const NEW_REPO_MESSAGE: &str = "빈 첫 커밋(Claude Board 자율주행이 새 폴더로 만듦)";

/// 경로가 어떤 상태인가. 원본: `APPathState`(`Ports.swift`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
    /// 없음
    Missing,
    /// 빈 폴더(`.DS_Store`·`desktop.ini`·`Thumbs.db` 만 든 폴더도 빈 것으로 본다. 판정은 engine/new_repo.rs)
    EmptyDir,
    /// 무엇이 든 폴더
    Dir,
    /// 파일·링크 등 폴더가 아님
    Other,
}

/// 폴더 포트. 원본: `APFolders`(`Ports.swift`)
pub trait Folders: Send + Sync {
    fn state(&self, path: &str) -> PathState;
    /// 폴더 하나를 만든다(부모는 만들지 않는다). 되면 true
    fn make_dir(&self, path: &str) -> bool;
}

/// git 포트 — 표준 출력만 받는다. 원본: `APGitRunner`(`Ports.swift`)
pub trait GitRunner: Send + Sync {
    /// `(종료 코드, 표준 출력)`
    fn run(&self, args: &[String], dir: &str) -> (i32, String);
    /// 커밋에 적을 사람 — 사용자 전역 git 설정의 `(user.name, user.email)`. 없으면 None
    fn identity(&self) -> Option<(String, String)>;
}

// ---------- 글자 도우미 ----------

// Foundation `CharacterSet.whitespaces`: 일반 범주 Zs 와 탭
fn is_cs_whitespace(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

// 자소 묶음을 앞 글자에 붙이는 확장 글자(stack.rs 의 근사 목록)
fn is_extend(c: char) -> bool {
    matches!(
        c,
        '\u{300}'..='\u{36f}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{fe20}'..='\u{fe2f}'
            | '\u{200c}'
            | '\u{200d}'
            | '\u{1f3fb}'..='\u{1f3ff}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

// ICU 의 줄 끝 글자
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

// Swift `String.count`(자소 묶음 수)의 근사: 확장 글자는 앞 글자에 붙고 `\r\n` 은 하나다
fn grapheme_count(s: &str) -> usize {
    let mut n = 0;
    let mut prev = '\0';
    for c in s.chars() {
        if is_extend(c) || (prev == '\r' && c == '\n') {
            if n == 0 {
                n = 1;
            }
        } else {
            n += 1;
        }
        prev = c;
    }
    n
}

fn trim_whitespace(s: &str) -> &str {
    s.trim_matches(is_cs_whitespace)
}

// `hasPrefix` — 접두사 바로 뒤의 결합 글자는 앞 글자 묶음에 속하므로 접두사로 보지 않는다
fn has_prefix(s: &str, prefix: &str) -> bool {
    match s.strip_prefix(prefix) {
        Some(rest) => !rest.chars().next().is_some_and(is_extend),
        None => false,
    }
}

// `^[A-Za-z0-9][A-Za-z0-9._-]*$`(ICU: `$` 는 끝 앞의 줄 끝 글자 하나도 허용)
fn name_matches(n: &str) -> bool {
    let b = n.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphanumeric() {
        return false;
    }
    let mut i = 1;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'.' | b'_' | b'-')) {
        i += 1;
    }
    let rest = &n[i..];
    rest.is_empty() || rest == "\r\n" || (rest.chars().count() == 1 && rest.chars().all(is_line_terminator))
}

// ---------- 이름과 자리 확인 ----------

const NAME_PROBLEM: &str = "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)";

/// 이름과 자리를 보고 계획을 만든다. `parent` 는 위치(절대 경로), `name` 은 새 폴더 이름, `home` 은 홈 폴더.
/// 이름은 영문·숫자·`.-_` 로 64자까지, 점으로 시작하지 않는다. 위치는 홈 폴더 아래의 있는 폴더이고 앱 자리(`~/Library`)·Claude 자리(`~/.claude`·`~/.claude-board`) 밖이다.
/// 새 자리는 없거나 빈 폴더여야 한다. 처음 걸어 둘 가지는 dev, 출발 가지는 live. 원본: `apNewRepoCheck`
pub fn new_repo_check(parent: &str, name: &str, home: &str, parent_state: PathState, target_state: PathState) -> (Option<NewRepoPlan>, Vec<String>) {
    let mut p: Vec<String> = Vec::new();
    let n = trim_whitespace(name);
    if n.is_empty() {
        p.push("새 폴더 이름을 넣어 주세요".to_string());
    } else if grapheme_count(n) > 64 {
        p.push("새 폴더 이름은 64자까지입니다".to_string());
    } else if !name_matches(n) {
        p.push(NAME_PROBLEM.to_string());
    }
    let mut par = trim_whitespace(parent).to_string();
    while grapheme_count(&par) > 1 && par.ends_with('/') {
        par.pop();
    }
    let h = home.strip_suffix('/').unwrap_or(home);
    if !par.starts_with('/') {
        p.push("위치는 / 로 시작하는 경로로 씁니다".to_string());
    } else if par != h && !has_prefix(&par, &format!("{h}/")) {
        p.push("위치는 홈 폴더 아래여야 합니다".to_string());
    } else if ["/Library", "/.claude", "/.claude-board", "/.Trash"].iter().any(|s| {
        let base = format!("{h}{s}");
        par == base || has_prefix(&par, &format!("{base}/"))
    }) {
        p.push("위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다".to_string());
    } else if par.split('/').any(|x| x == "." || x == "..") {
        p.push("위치에 . 이나 .. 을 쓰지 않습니다".to_string());
    } else if parent_state != PathState::EmptyDir && parent_state != PathState::Dir {
        p.push(if parent_state == PathState::Missing { "위치 폴더가 없습니다" } else { "위치가 폴더가 아닙니다" }.to_string());
    }
    let path = format!("{par}/{n}");
    if p.is_empty() {
        match target_state {
            PathState::Missing | PathState::EmptyDir => {}
            PathState::Dir => p.push("그 자리에 이미 무엇이 든 폴더가 있습니다. 기존 저장소로 고르거나 다른 이름을 씁니다".to_string()),
            PathState::Other => p.push("그 자리에 폴더가 아닌 것이 있습니다".to_string()),
        }
    }
    if !p.is_empty() {
        return (None, p);
    }
    let plan = NewRepoPlan {
        path,
        branches: NEW_REPO_BRANCHES.iter().map(|s| s.to_string()).collect(),
        head: "dev".to_string(),
        base: "live".to_string(),
        message: NEW_REPO_MESSAGE.to_string(),
    };
    (Some(plan), Vec::new())
}

// Windows 예약 장치 이름(첫 점 앞, 대소문자 무시)
fn is_reserved_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s => s.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) && matches!(s.as_bytes()[3], b'1'..=b'9'),
    }
}

/// Windows 가 거부하는 새 폴더 이름의 문제 한 줄(없으면 None). 원본 이름 규칙에 더해 영문·숫자·`.-_` 밖 글자(줄끝 포함)·끝의 `.`·예약 장치 이름을 막는다.
/// 이름은 `new_repo_check` 처럼 앞뒤 빈칸(Zs·탭)을 깎고 본다. Windows 전용(원본에 없음)
pub fn new_repo_name_problem_windows(name: &str) -> Option<String> {
    let n = trim_whitespace(name);
    if n.is_empty() {
        return Some("새 폴더 이름을 넣어 주세요".to_string());
    }
    let b = n.as_bytes();
    if !b[0].is_ascii_alphanumeric() || !b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')) {
        return Some(NAME_PROBLEM.to_string());
    }
    if n.ends_with('.') {
        return Some("Windows 는 이름 끝의 점을 떼어 버립니다. 끝에 . 을 쓰지 않습니다".to_string());
    }
    if is_reserved_device(n) {
        return Some("Windows 가 장치 이름으로 쓰는 이름입니다(CON·PRN·AUX·NUL·COM1~9·LPT1~9). 다른 이름을 씁니다".to_string());
    }
    None
}

/// `new_repo_check` + Windows 이름 확인. 원본 확인이 문제를 내면 그대로 내고, 계획을 낼 때만 이름을 `new_repo_name_problem_windows` 로 한 번 더 본다. Windows 전용
pub fn new_repo_check_windows(parent: &str, name: &str, home: &str, parent_state: PathState, target_state: PathState) -> (Option<NewRepoPlan>, Vec<String>) {
    let (plan, problems) = new_repo_check(parent, name, home, parent_state, target_state);
    if plan.is_some() {
        if let Some(w) = new_repo_name_problem_windows(name) {
            return (None, vec![w]);
        }
    }
    (plan, problems)
}

// ---------- git 차례 ----------

/// git 차례 — 처음 가지 이름으로 init, 빈 첫 커밋(사람 이름은 사용자 git 설정, 없으면 보드 이름), 나머지 가지를 그 커밋에.
/// hooks·서명 설정은 쓰지 않는다(서명은 `-c` 로 끈다). 원본: `apNewRepoSteps`
pub fn new_repo_steps(plan: &NewRepoPlan, identity: Option<(&str, &str)>) -> Vec<Vec<String>> {
    let (name, email) = identity.unwrap_or(("Claude Board", "claude-board@localhost"));
    let v = |a: &[&str]| -> Vec<String> { a.iter().map(|s| s.to_string()).collect() };
    let mut s: Vec<Vec<String>> = vec![v(&["init", "-q", "-b", &plan.head])];
    s.push(v(&[
        "-c",
        &format!("user.name={name}"),
        "-c",
        &format!("user.email={email}"),
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "--allow-empty",
        "--no-verify",
        "-m",
        &plan.message,
    ]));
    for b in &plan.branches {
        if *b != plan.head {
            s.push(v(&["branch", b]));
        }
    }
    s
}

/// 만들기 결과. 원본: `APCreateResult`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CreateResult {
    pub ok: bool,
    /// 폴더를 새로 만들었나(빈 폴더였으면 false)
    pub made: bool,
    /// 빈 첫 커밋
    pub commit: Option<String>,
    /// 한 일(쉬운 말)
    pub done: Vec<String>,
    pub error: Option<String>,
}

/// 유스케이스: 새 폴더 만들기 — 계획대로 폴더(없을 때)·git init·빈 첫 커밋·가지. 하다 실패하면 거기서 멈추고 까닭을 돌려준다
/// (만든 것을 지우지 않는다). 끝에 가지가 모두 빈 첫 커밋을 가리키는지 본다. 원본: `apCreateRepo`
pub fn create_repo(plan: &NewRepoPlan, folders: &dyn Folders, git: &dyn GitRunner) -> CreateResult {
    let mut r = CreateResult::default();
    match folders.state(&plan.path) {
        PathState::Missing => {
            if !folders.make_dir(&plan.path) {
                r.error = Some(format!("폴더를 만들지 못했습니다: {}", plan.path));
                return r;
            }
            r.made = true;
            r.done.push("폴더를 만들었습니다".to_string());
        }
        PathState::EmptyDir => r.done.push("빈 폴더를 그대로 씁니다".to_string()),
        PathState::Dir | PathState::Other => {
            r.error = Some(format!("그 자리에 이미 무엇이 있어 만들지 않았습니다: {}", plan.path));
            return r;
        }
    }
    let identity = git.identity();
    for args in new_repo_steps(plan, identity.as_ref().map(|(a, b)| (a.as_str(), b.as_str()))) {
        let (code, out) = git.run(&args, &plan.path);
        if code != 0 {
            let verb = args.iter().find(|a| !a.starts_with('-') && !a.contains('=')).map(String::as_str).unwrap_or("");
            let tail = if out.is_empty() { String::new() } else { format!(": {out}") };
            r.error = Some(format!("git {verb} 이 실패했습니다(종료 코드 {code}){tail}"));
            return r;
        }
    }
    r.done.push(format!("git init(처음 가지 {})", plan.head));
    let mut heads: BTreeSet<String> = BTreeSet::new();
    for b in &plan.branches {
        let (code, out) = git.run(&["rev-parse".to_string(), "--verify".to_string(), "-q".to_string(), format!("refs/heads/{b}")], &plan.path);
        if code != 0 || out.is_empty() {
            r.error = Some(format!("가지 {b} 를 만들지 못했습니다"));
            return r;
        }
        heads.insert(out);
    }
    let c = match (heads.len(), heads.into_iter().next()) {
        (1, Some(c)) => c,
        _ => {
            r.error = Some("가지가 같은 첫 커밋을 가리키지 않습니다".to_string());
            return r;
        }
    };
    r.done.push(format!("빈 첫 커밋 {} 에 가지 {}", c.chars().take(7).collect::<String>(), plan.branches.join("·")));
    r.commit = Some(c);
    r.ok = true;
    r
}
