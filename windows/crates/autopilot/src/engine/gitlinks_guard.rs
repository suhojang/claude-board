//! 하위 모듈(gitlink) 막기: 출발 때 막고, 실행 중 새로 생기면 그 단위를 멈춘다(macOS 판 M-709).
//! 판정은 `rules::gitlinks`(순수)가 하고, 여기는 git 에 묻고 그 답을 엔진이 쓸 꼴로 바꾼다.
//! 원본: `Sources/ClaudeAutopilot/main.swift` 의 출발 확인(`problems` 의 하위 모듈 줄)과 `submoduleReason`·`unitGitOK`.
//!
//! 약속:
//! - git 은 모두 `ports::Git`(기본 `adapters::git::ProcessGit`, `git.exe`, 보호 환경)로만 부르고 저장소는 늘 호출의 `repo` 인자로 넘긴다(`-C <저장소>`).
//! - 색인은 `-c core.quotePath=false ls-files -s -z`(filter 를 돌리지 않는다)로 읽는다. 읽지 못하면 닫힌 쪽(멈춤).
//! - `.gitmodules` 는 작업 트리·색인·HEAD(출발 때는 출발 가지도) 가운데 하나라도 있으면 있는 것으로 본다. 출발 가지의 gitlink 는 `ls-tree -r -z`.
//!   macOS 출발 확인은 작업 트리·색인·출발 가지를, 단위 확인은 작업 트리·색인·HEAD 를 보는데 여기는 둘을 합쳐 본다(닫힌 쪽으로 더 넓다).
//! - 실행 중 확인은 출발 때 잰 기준(`Snapshot`)과 견줘 새로 생긴 gitlink 나 새로 생긴 `.gitmodules` 가 있을 때만 멈춘다.
//!   출발 확인이 하위 모듈을 막으므로 기준은 늘 비어 있고, 그러면 macOS 의 "하나라도 있으면 멈춤"과 같다.

use crate::adapters::git::ProcessGit;
use crate::ports::Git;
use crate::rules::gitlinks::{gitlink_paths, has_submodule, submodule_reason};
use std::path::Path;

/// 출발 때 잰 하위 모듈 자리(실행 중 확인의 기준)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub gitlinks: Vec<String>,
    pub gitmodules: bool,
}

/// 확인 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 하위 모듈 없음(실행 중 확인에서는 기준에 없던 새 것이 없음)
    Clear,
    /// 하위 모듈이 있음(실행 중 확인에서는 새로 생김). 글은 규칙 `submodule_reason` 이 만든다
    Submodule { gitlinks: Vec<String>, gitmodules: bool },
    /// git 이 답하지 못해 확인할 수 없음 — 닫힌 쪽. `what` 은 실패한 git 명령 이름
    Unreadable { what: String },
}

impl Verdict {
    pub fn is_clear(&self) -> bool {
        matches!(self, Verdict::Clear)
    }

    /// 멈추는 까닭의 본문(`<저장소> 의 …` 뒤에 붙는다). Clear 면 None
    pub fn why(&self) -> Option<String> {
        match self {
            Verdict::Clear => None,
            Verdict::Submodule { gitlinks, gitmodules } => Some(submodule_reason(gitlinks, *gitmodules)),
            Verdict::Unreadable { what } => Some(format!("색인을 읽지 못해({what} 실패)")),
        }
    }
}

/// 출발 확인의 결과. `Clear` 일 때만 `baseline` 이 있다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCheck {
    pub verdict: Verdict,
    pub baseline: Option<Snapshot>,
}

/// 엔진 단위 반복이 쓸 결과: 그 단위를 `failed` 로 끝내는 까닭. 단위 기록(`engine.json`)의 `reason` 이자 진행 기록(`events`)에 남길 글
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitStop {
    pub reason: String,
}

fn out(git: &dyn Git, repo: &Path, args: &[&str]) -> Option<String> {
    let o = git.run(repo, args).ok()?;
    (o.code == 0).then_some(o.stdout)
}

/// 색인의 gitlink. 읽지 못하면 None
fn index_gitlinks(git: &dyn Git, repo: &Path) -> Option<Vec<String>> {
    out(git, repo, &["-c", "core.quotePath=false", "ls-files", "-s", "-z"]).map(|t| gitlink_paths(&t))
}

/// `.gitmodules` 가 작업 트리·색인·HEAD(·`base` 가지) 가운데 하나라도 있는가
fn has_gitmodules(git: &dyn Git, repo: &Path, base: Option<&str>) -> bool {
    std::fs::symlink_metadata(repo.join(".gitmodules")).is_ok()
        || out(git, repo, &["ls-files", "-s", "-z", "--", ".gitmodules"]).is_some_and(|t| !t.is_empty())
        || out(git, repo, &["ls-tree", "HEAD", "--", ".gitmodules"]).is_some_and(|t| !t.is_empty())
        || base.is_some_and(|b| out(git, repo, &["cat-file", "-e", &format!("{b}:.gitmodules")]).is_some())
}

/// 지금의 하위 모듈 자리. 색인을 읽지 못하면 `Err(실패한 명령)`
fn read_now(git: &dyn Git, repo: &Path, base: Option<&str>) -> Result<Snapshot, String> {
    let mut gitlinks = index_gitlinks(git, repo).ok_or_else(|| "git ls-files".to_string())?;
    if let Some(b) = base {
        // ls-tree 는 filter 를 돌리지 않는다. 읽지 못해도 멈추지 않는다(출발 가지가 없는 문제는 다른 확인이 낸다)
        if let Some(t) = out(git, repo, &["-c", "core.quotePath=false", "ls-tree", "-r", "-z", b]) {
            for l in gitlink_paths(&t) {
                if !gitlinks.contains(&l) {
                    gitlinks.push(l);
                }
            }
        }
    }
    Ok(Snapshot { gitlinks, gitmodules: has_gitmodules(git, repo, base) })
}

/// 출발 확인. `base` 는 출발 가지(run.json 의 `base_branch`), 없으면 색인·`.gitmodules` 만 본다.
/// 가지 이름이 `-` 로 시작하면 git 옵션으로 읽힐 수 있어 확인할 수 없는 것으로 닫는다.
pub fn check_start_with(git: &dyn Git, repo: &Path, base: Option<&str>) -> StartCheck {
    if base.is_some_and(|b| b.starts_with('-')) {
        return StartCheck { verdict: Verdict::Unreadable { what: "git ls-tree".to_string() }, baseline: None };
    }
    match read_now(git, repo, base) {
        Err(what) => StartCheck { verdict: Verdict::Unreadable { what }, baseline: None },
        Ok(s) if has_submodule(&s.gitlinks, s.gitmodules) => {
            StartCheck { verdict: Verdict::Submodule { gitlinks: s.gitlinks, gitmodules: s.gitmodules }, baseline: None }
        }
        Ok(s) => StartCheck { verdict: Verdict::Clear, baseline: Some(s) },
    }
}

/// 기본 길(보호 환경 `ProcessGit`)로 하는 출발 확인
pub fn check_start(repo: &Path, base: Option<&str>) -> StartCheck {
    check_start_with(&ProcessGit::new(), repo, base)
}

/// 실행 중 확인: 기준 뒤 새로 생긴 gitlink 나 `.gitmodules` 가 있으면 `Submodule`(까닭 글에는 지금의 모두를 적는다), 못 읽으면 `Unreadable`
pub fn check_unit_with(git: &dyn Git, repo: &Path, baseline: &Snapshot) -> Verdict {
    let now = match read_now(git, repo, None) {
        Ok(s) => s,
        Err(what) => return Verdict::Unreadable { what },
    };
    let new_link = now.gitlinks.iter().any(|l| !baseline.gitlinks.contains(l));
    let new_gitmodules = now.gitmodules && !baseline.gitmodules;
    if new_link || new_gitmodules {
        Verdict::Submodule { gitlinks: now.gitlinks, gitmodules: now.gitmodules }
    } else {
        Verdict::Clear
    }
}

/// 기본 길(보호 환경 `ProcessGit`)로 하는 실행 중 확인
pub fn check_unit(repo: &Path, baseline: &Snapshot) -> Verdict {
    check_unit_with(&ProcessGit::new(), repo, baseline)
}

/// 저장소 이름(마지막 폴더 이름, `\`·`/` 모두 가른다). macOS `repoName`
pub fn repo_name(repo: &Path) -> String {
    let s = repo.to_string_lossy();
    let t = s.trim_end_matches(['/', '\\']);
    t.rsplit(['/', '\\']).next().unwrap_or(t).to_string()
}

/// 출발을 막는 글(`start` 가 문제 목록에 넣는다). Clear 면 None
pub fn start_message(repo: &Path, v: &Verdict) -> Option<String> {
    let name = repo_name(repo);
    match v {
        Verdict::Clear => None,
        Verdict::Unreadable { what } => Some(format!("{name} 의 색인을 읽지 못했습니다({what} 실패)")),
        Verdict::Submodule { .. } => Some(format!(
            "{name} 의 {} 출발하지 않습니다. 자율주행은 git 하위 모듈이 있는 저장소를 아직 다루지 않습니다",
            v.why()?
        )),
    }
}

/// 단위 반복이 쓸 것: Clear 면 None(계속), 아니면 그 단위를 `failed` 로 끝내는 까닭과 기록 글
pub fn unit_stop(n: usize, repo: &Path, v: &Verdict) -> Option<UnitStop> {
    let why = v.why()?;
    let name = repo_name(repo);
    Some(UnitStop { reason: format!("보안: 단위 {n} 의 {name} 의 {why} 그 자리에서 git(모델·판정·기록)을 돌리지 않습니다. 사람이 확인하세요.") })
}
