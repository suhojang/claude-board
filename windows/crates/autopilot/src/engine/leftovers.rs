//! 단위가 끝난 자리에 남은 변경을 실제 git 으로 읽고, 읽기 전용 자리의 것은 `git stash` 로 따로 보관하는 엔진 쪽.
//! 판정은 `rules::leftovers`(순수)가 하고, 여기는 git 에 묻고 그 답을 규칙이 받는 꼴로 바꾸며 규칙이 정한 만큼만 보관한다.
//! 원본: `Sources/ClaudeAutopilot/main.swift` 의 `porcelain`·`stashReadOnly`.
//!
//! 약속:
//! - git 은 모두 `ports::Git`(보호 환경을 거는 `adapters::git::ProcessGit`, `git.exe`)로만 부르고 저장소는 호출의 `repo` 인자로 넘긴다(`-C <저장소>`).
//! - 상태는 `status --porcelain=v1 -z --ignore-submodules=all`(추적 안 하는 파일 포함, 무시 파일 제외)로 읽고
//!   규칙이 받는 줄 꼴(`git status --porcelain` 의 따옴표·` -> ` 꼴)로 바꾼다. 따옴표로 싸는 경로(공백·특수 글자·한글)는 규칙이 갈라 내지 않는다.
//!   읽지 못하면 빈 목록이 아니라 오류다(Swift 는 빈 목록이라 "깨끗함"으로 읽었다 — 여기는 닫힌 쪽으로 더 엄격하다).
//! - 보관은 `stash push --include-untracked -m <규칙의 글>`. 보관 앞뒤의 `refs/stash` 가 달라졌을 때만 보관한 것으로 센다.
//!   색인의 gitlink 는 pathspec 으로 뺀다(그 저장소의 filter 를 돌리지 않게). 색인을 읽지 못하면 보관하지 않는다.
//! - git 이 실패하면 오류를 돌려준다. 파일을 지우거나 되돌리는 일은 하지 않는다 — git 이 일부를 치운 뒤 실패했으면
//!   보관본(`stash_created`)과 남은 파일이 함께 있어 어느 쪽에도 변경이 남는다.
//! - CRLF 변환 경고(`warning: … LF will be replaced by CRLF`)는 stderr 에 나올 뿐 종료코드 0 이면 실패로 보지 않는다.
//! - 긴 경로·잠긴 파일은 git 이 실패로 답하면 오류로 돌려주고 파일은 그대로 둔다.

use crate::ports::{Git, PortError};
use crate::rules::gitlinks::gitlink_excludes;
use crate::rules::leftovers::{leftover, stash_message, Leftover};
use serde_json::{json, Value};
use std::path::Path;

/// 보관 기록(run.json `branches.stashed`)의 `paths` 에 남기는 경로 수 상한(원본 `prefix(20)`)
pub const RECORD_PATHS_MAX: usize = 20;
/// 사용자 git 신원이 없을 때 쓰는 값(원본 `gitIdentity` 의 기본)
pub const DEFAULT_IDENTITY: (&str, &str) = ("Claude Board autopilot", "autopilot@claude-board.invalid");

/// 읽거나 보관하지 못한 까닭. 어느 경우에도 작업 트리의 파일을 지우지 않았다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftoverError {
    /// git 을 띄우지 못했다(git.exe 없음·시간 초과 등)
    Port(String),
    /// git 이 0 이 아닌 종료코드로 답했다. `what` 은 명령 이름
    Git { what: String, code: i32, detail: String },
    /// 색인을 읽지 못해 gitlink 를 가릴 수 없다 — 닫힌 쪽(보관하지 않음)
    IndexUnreadable { detail: String },
    /// `git stash` 가 0 으로 끝났는데 새 보관본이 없다(보관할 변경이 없었음)
    NothingStashed,
    /// `git stash` 가 실패했다. `stash_created` 가 있으면 git 이 보관본을 만든 뒤 작업 트리를 치우다 실패한 것(보관본에 전부 있다)
    StashFailed { code: i32, detail: String, stash_created: Option<String> },
}

impl std::fmt::Display for LeftoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeftoverError::Port(m) => write!(f, "git 을 실행하지 못했습니다: {m}"),
            LeftoverError::Git { what, code, detail } => write!(f, "git {what} 실패({code}): {detail}"),
            LeftoverError::IndexUnreadable { detail } => write!(f, "색인을 읽지 못해 보관하지 않았습니다: {detail}"),
            LeftoverError::NothingStashed => write!(f, "보관할 변경이 없었습니다"),
            LeftoverError::StashFailed { code, detail, stash_created } => match stash_created {
                Some(s) => write!(f, "git stash 실패({code}), 보관본 {} 은 만들어졌습니다: {detail}", short_sha(s)),
                None => write!(f, "git stash 실패({code}): {detail}"),
            },
        }
    }
}

impl std::error::Error for LeftoverError {}

impl From<PortError> for LeftoverError {
    fn from(e: PortError) -> Self {
        LeftoverError::Port(e.to_string())
    }
}

/// 첫 줄만, 160자까지(원본 `firstLine`)
fn first_line(s: &str) -> String {
    let l = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    l.chars().take(160).collect()
}

fn short_sha(s: &str) -> &str {
    s.get(..7).unwrap_or(s)
}

// ── 상태 읽기 ────────────────────────────────────────

/// 경로를 `git status --porcelain`(-z 아님)이 내는 꼴로: 공백·`"`·`\`·제어 글자·ASCII 밖(core.quotePath 기본)이 있으면 큰따옴표로 싸고 C 식으로 이스케이프
fn quote_like_git(path: &str) -> String {
    let plain = path.bytes().all(|b| b > 0x20 && b < 0x7f && b != b'"' && b != b'\\');
    if plain {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for b in path.bytes() {
        match b {
            0x07 => out.push_str("\\a"),
            0x08 => out.push_str("\\b"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            0x0b => out.push_str("\\v"),
            0x0c => out.push_str("\\f"),
            b'\r' => out.push_str("\\r"),
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b' ' => out.push(' '),
            0x21..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push('"');
    out
}

/// `status --porcelain=v1 -z` 출력 → 규칙이 받는 줄. 항목은 `XY 경로\0`, 이름 바꿈·복사(R·C)는 `XY 새\0옛\0` → `XY 옛 -> 새`.
/// 짧거나 깨진 항목(경로가 빈 것)은 건너뛰지 않고 그대로 줄로 둔다 — 규칙이 그런 줄은 갈라 내지 않는다
pub fn parse_status_z(out: &str) -> Vec<String> {
    let mut it = out.split('\0').filter(|e| !e.is_empty());
    let mut lines = Vec::new();
    while let Some(entry) = it.next() {
        let Some(xy) = entry.get(..2) else {
            lines.push(entry.to_string());
            continue;
        };
        let Some(path) = entry.get(3..).filter(|p| !p.is_empty()) else {
            lines.push(entry.to_string());
            continue;
        };
        if xy.contains('R') || xy.contains('C') {
            let old = it.next().unwrap_or("");
            lines.push(format!("{xy} {} -> {}", quote_like_git(old), quote_like_git(path)));
        } else {
            lines.push(format!("{xy} {}", quote_like_git(path)));
        }
    }
    lines
}

fn run_ok(git: &dyn Git, repo: &Path, what: &str, args: &[&str]) -> Result<String, LeftoverError> {
    let o = git.run(repo, args)?;
    if o.code != 0 {
        let detail = if o.stderr.trim().is_empty() { first_line(&o.stdout) } else { first_line(&o.stderr) };
        return Err(LeftoverError::Git { what: what.to_string(), code: o.code, detail });
    }
    Ok(o.stdout)
}

/// 작업 트리 상태 줄(추적 안 하는 파일 포함, 무시 파일 제외, 하위 모듈 안은 보지 않음). 원본 `porcelain`
pub fn status_lines(git: &dyn Git, repo: &Path) -> Result<Vec<String>, LeftoverError> {
    let out = run_ok(git, repo, "status", &["-c", "core.longpaths=true", "status", "--porcelain=v1", "-z", "--ignore-submodules=all"])?;
    Ok(parse_status_z(&out))
}

/// `check_leftovers` 가 돌려주는 것: 읽은 줄과 규칙의 판정
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub dirty: Vec<String>,
    pub verdict: Leftover,
}

/// 남은 변경을 읽고 규칙 `leftover` 로 판정한다. `dirty_start` 는 출발 때 잰 상태 줄(run.json `branches.repos.<저장소>.dirty_start`),
/// `on_story_branch`·`made_by_run` 은 규칙 문서의 뜻 그대로다. 상태를 읽지 못하면 오류(판정하지 않는다)
pub fn check_leftovers(
    git: &dyn Git,
    repo: &Path,
    dirty_start: &[String],
    on_story_branch: bool,
    made_by_run: bool,
) -> Result<Checked, LeftoverError> {
    let dirty = status_lines(git, repo)?;
    let verdict = leftover(&dirty, dirty_start, on_story_branch, made_by_run);
    Ok(Checked { dirty, verdict })
}

// ── 보관 ─────────────────────────────────────────────

/// 보관 요청. 글(`message`)은 규칙 `stash_message(run, unit, repo_name, branch)` 가 만든다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashRequest<'a> {
    pub run: &'a str,
    pub unit: Option<i64>,
    /// 저장소 이름(마지막 폴더 이름)
    pub repo_name: &'a str,
    pub branch: &'a str,
    /// 판정 `Stash(paths)` 의 경로. `None` 이면 gitlink 를 뺀 전부
    pub paths: Option<&'a [String]>,
    /// 이번에 보관되는 상태 줄(출발 때 없던 줄) — 개수와 기록의 `paths` 가 된다
    pub new_lines: &'a [String],
}

/// 보관한 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stashed {
    /// `refs/stash` 의 커밋 해시
    pub sha: String,
    pub message: String,
    pub files: usize,
    pub paths: Vec<String>,
}

fn rev_stash(git: &dyn Git, repo: &Path) -> Option<String> {
    let o = git.run(repo, &["rev-parse", "-q", "--verify", "refs/stash"]).ok()?;
    let t = o.stdout.trim();
    (o.code == 0 && !t.is_empty()).then(|| t.to_string())
}

/// 저장소의 git 신원(값만 읽는다). 없으면 엔진 이름
fn identity(git: &dyn Git, repo: &Path) -> (String, String) {
    let get = |k: &str| {
        git.run(repo, &["config", "--get", k])
            .ok()
            .filter(|o| o.code == 0)
            .map(|o| o.stdout.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    (
        get("user.name").unwrap_or_else(|| DEFAULT_IDENTITY.0.to_string()),
        get("user.email").unwrap_or_else(|| DEFAULT_IDENTITY.1.to_string()),
    )
}

/// 읽기 전용 자리에 남은 변경을 `git stash push --include-untracked` 로 보관한다. 실패하면 오류이고 파일을 지우거나 되돌리지 않는다.
/// 부르기 전에 하위 모듈 확인(`gitlinks_guard`)·가지 확인·판정(`check_leftovers` 가 `Stash`)을 마쳐야 한다
pub fn stash_leftovers(git: &dyn Git, repo: &Path, req: &StashRequest) -> Result<Stashed, LeftoverError> {
    let message = stash_message(req.run, req.unit, req.repo_name, req.branch);
    let before = rev_stash(git, repo);
    let ls = git
        .run(repo, &["-c", "core.quotePath=false", "ls-files", "-s", "-z"])
        .map_err(LeftoverError::from)?;
    if ls.code != 0 {
        return Err(LeftoverError::IndexUnreadable { detail: first_line(&ls.stderr) });
    }
    let excludes = gitlink_excludes(&ls.stdout);
    let spec: Vec<String> = match req.paths {
        Some(ps) => std::iter::once("--".to_string()).chain(ps.iter().map(|p| format!(":(literal){p}"))).collect(),
        None if excludes.is_empty() => Vec::new(),
        None => ["--".to_string(), ":(literal).".to_string()].into_iter().chain(excludes).collect(),
    };
    let (name, email) = identity(git, repo);
    let (un, ue) = (format!("user.name={name}"), format!("user.email={email}"));
    let mut args: Vec<&str> = vec![
        "-c", "core.longpaths=true", "-c", &un, "-c", &ue, "stash", "push", "--include-untracked", "-m", &message,
    ];
    args.extend(spec.iter().map(String::as_str));
    let o = git.run(repo, &args)?;
    let after = rev_stash(git, repo);
    let created = after.clone().filter(|a| Some(a) != before.as_ref());
    if o.code != 0 {
        let detail = if o.stderr.trim().is_empty() { first_line(&o.stdout) } else { first_line(&o.stderr) };
        return Err(LeftoverError::StashFailed { code: o.code, detail, stash_created: created });
    }
    let Some(sha) = created else {
        return Err(LeftoverError::NothingStashed);
    };
    Ok(Stashed {
        sha,
        message,
        files: req.new_lines.len(),
        paths: req.new_lines.iter().take(RECORD_PATHS_MAX).cloned().collect(),
    })
}

// ── 기록 ─────────────────────────────────────────────

/// run.json `branches.stashed` 에 더할 한 줄(원본 `row`). 키: repo·branch·unit(없으면 null)·stash·message·files·paths(20개까지)·at(초, 소수 셋째 자리까지)
pub fn record_leftovers(repo: &str, branch: &str, unit: Option<i64>, s: &Stashed, at: f64) -> Value {
    json!({
        "repo": repo,
        "branch": branch,
        "unit": unit,
        "stash": s.sha,
        "message": s.message,
        "files": s.files,
        "paths": s.paths,
        "at": (at * 1000.0).round() / 1000.0,
    })
}

/// 보관한 뒤의 사람용 한 줄(원본 events 글)
pub fn stashed_event(unit: Option<i64>, repo_name: &str, branch: &str, s: &Stashed, still_dirty: bool) -> String {
    let u = unit.map_or_else(|| "?".to_string(), |n| n.to_string());
    format!(
        "단위 {u} 이 읽기 전용으로 걸어 둔 {repo_name} {branch} 에 남긴 변경 {}개를 엔진이 따로 보관했습니다(git stash {}, 판정·다음 단위에 넣지 않음){}.",
        s.files,
        short_sha(&s.sha),
        if still_dirty { ". 보관하지 못한 변경이 남았습니다" } else { "" }
    )
}

/// 보관하지 못했을 때의 한 줄(원본 events 글)
pub fn stash_failed_event(unit: Option<i64>, repo_name: &str, branch: &str, files: usize, err: &LeftoverError) -> String {
    let u = unit.map_or_else(|| "?".to_string(), |n| n.to_string());
    format!("단위 {u} 이 읽기 전용으로 걸어 둔 {repo_name} {branch} 에 남긴 변경 {files}개를 보관하지 못했습니다: {err}")
}

/// 새로 생긴 줄(출발 때 없던 줄)
pub fn new_lines(dirty: &[String], dirty_start: &[String]) -> Vec<String> {
    dirty.iter().filter(|l| !dirty_start.contains(l)).cloned().collect()
}
