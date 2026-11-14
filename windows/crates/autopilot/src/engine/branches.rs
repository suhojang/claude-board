//! 엔진이 정하는 가지 — 실행·단위의 가지 이름, 출발 가지, run.json `branches` 기록. 원본: `Sources/ClaudeAutopilot/main.swift` 의
//! `startBranches`·`baseBranch`·`renderBranch`·`validBranch`·`ensureBranch`·`repoRecord`·`setStoryRepo` 와 보드 `pickDefaultBases`.
//! 합치기 상태를 읽는 규칙(`rules::branches`)·가지 자리(`rules::promotion`)는 순수 규칙이고, 여기는 그 규칙에 git 이 읽은 값을 먹이고
//! 규칙이 읽는 꼴로 run.json 에 쓴다.
//!
//! 약속:
//! - git 은 모두 `ports::Git`(보호 환경의 `adapters::git::ProcessGit`, 늘 git.exe)로만 부른다. PATH 의 `git` 을 직접 띄우지 않는다.
//! - 가지 이름은 폴더·실행 id 에서 오므로 **ASCII `[A-Za-z0-9._-]` 한 칸 64자 이하**만 받는다. git 이 거부하는 꼴(`..`·공백·`~^:?*[\`·끝 `.`·`.lock`·앞 `.`·앞 `-`)과
//!   git 은 받지만 Windows 에서 안전하지 않은 꼴(예약 장치 이름 `con`·`nul`·`com1`…, 대소문자만 다른 `.LOCK`, 비 ASCII, 너무 긴 이름)을 모두 거절한다.
//!   고치지 않고 거절하는 길(`branch_name`)과 폴더 이름을 안전한 id 로 바꾸는 길(`normalise_id`)을 따로 둔다.
//! - 실행 가지 `autopilot/<id>` 는 파일처럼 놓이므로 `autopilot/<id>/<단위>` 는 만들 수 없다(git 의 디렉터리·파일 충돌). 단위 가지는 패턴(`features/{task}`)에서 오고
//!   `autopilot/` 이름 공간은 거절한다.
//! - run.json 기록은 `serde_json` 맵(`Run.extra`)을 받아 `branches` 칸만 고친다. 엔진이 저장(`save_run`)한다.

use crate::ports::{Git, PortError};
use crate::rules::promotion::branch_roles;
use serde_json::{Map, Value};
use std::fmt;
use std::path::Path;

/// 실행 id·스토리 키·작업 id 같은 한 칸의 길이 상한
pub const MAX_SEGMENT_LEN: usize = 64;
/// 가지 이름 전체 길이 상한(`.git/refs/heads/…` 가 Windows 경로 한도에 닿지 않게)
pub const MAX_BRANCH_LEN: usize = 200;
/// 실행 가지의 이름 공간
pub const RUN_NAMESPACE: &str = "autopilot";

// ── 이름 ─────────────────────────────────────────────

/// 이름을 거절한 까닭. 규칙마다 하나라 시험이 하나씩 잡는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRefusal {
    Empty,
    TooLong,
    /// ASCII `[A-Za-z0-9._-]` 밖의 글자(공백·`~^:?*[\`·`/`·제어 문자·한글 …)
    BadChar(char),
    /// `..`
    DotDot,
    /// 앞이 `.`
    LeadingDot,
    /// 앞이 `-`(옵션으로 읽힌다)
    LeadingDash,
    /// 끝이 `.`
    TrailingDot,
    /// 끝이 `.lock`(대소문자 무관 — NTFS 는 `x.LOCK` 을 `x` 의 잠금 파일과 같은 파일로 본다)
    LockSuffix,
    /// Windows 예약 장치 이름(`con`·`prn`·`aux`·`nul`·`com0-9`·`lpt0-9`, 첫 `.` 앞 이름이 같으면 확장자가 붙어도)
    ReservedDevice(String),
    /// `a//b`·앞뒤 `/` 처럼 빈 칸
    EmptySegment,
    /// `autopilot/…` — 실행 가지 이름 공간
    ReservedNamespace,
}

impl fmt::Display for NameRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameRefusal::Empty => write!(f, "빈 이름"),
            NameRefusal::TooLong => write!(f, "너무 긴 이름"),
            NameRefusal::BadChar(c) => write!(f, "가지 이름에 쓸 수 없는 글자 {c:?}"),
            NameRefusal::DotDot => write!(f, "'..' 을 품은 이름"),
            NameRefusal::LeadingDot => write!(f, "'.' 으로 시작하는 이름"),
            NameRefusal::LeadingDash => write!(f, "'-' 로 시작하는 이름"),
            NameRefusal::TrailingDot => write!(f, "'.' 으로 끝나는 이름"),
            NameRefusal::LockSuffix => write!(f, "'.lock' 으로 끝나는 이름"),
            NameRefusal::ReservedDevice(n) => write!(f, "Windows 예약 장치 이름 {n}"),
            NameRefusal::EmptySegment => write!(f, "빈 칸이 있는 이름"),
            NameRefusal::ReservedNamespace => write!(f, "자율주행 가지 이름 공간({RUN_NAMESPACE}/)은 쓸 수 없다"),
        }
    }
}

impl std::error::Error for NameRefusal {}

fn is_reserved_device(seg: &str) -> Option<String> {
    let stem = seg.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|p| stem.strip_prefix(p).is_some_and(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_digit()));
    reserved.then_some(stem)
}

/// 한 칸(실행 id·스토리 키·작업 id·`/` 사이의 한 조각)이 가지 이름으로 안전한가.
fn check_segment(s: &str, max: usize) -> Result<(), NameRefusal> {
    if s.is_empty() {
        return Err(NameRefusal::Empty);
    }
    if s.len() > max {
        return Err(NameRefusal::TooLong);
    }
    if let Some(c) = s.chars().find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))) {
        return Err(NameRefusal::BadChar(c));
    }
    if s.contains("..") {
        return Err(NameRefusal::DotDot);
    }
    if s.starts_with('.') {
        return Err(NameRefusal::LeadingDot);
    }
    if s.starts_with('-') {
        return Err(NameRefusal::LeadingDash);
    }
    if s.ends_with('.') {
        return Err(NameRefusal::TrailingDot);
    }
    if s.to_ascii_lowercase().ends_with(".lock") {
        return Err(NameRefusal::LockSuffix);
    }
    if let Some(stem) = is_reserved_device(s) {
        return Err(NameRefusal::ReservedDevice(stem));
    }
    Ok(())
}

/// 이 실행의 가지 `autopilot/<실행 id>`. 고치지 않고 거절한다 — 실행 폴더 이름도 같은 id 라 가지만 다른 글자로 바뀌면 둘이 어긋난다.
pub fn branch_name(run_id: &str) -> Result<String, NameRefusal> {
    check_segment(run_id, MAX_SEGMENT_LEN)?;
    Ok(format!("{RUN_NAMESPACE}/{run_id}"))
}

/// 가지 이름 전체(`/` 로 이은 여러 칸)가 안전한가. `autopilot/…` 이름 공간·빈 칸·전체 길이를 함께 본다.
pub fn check_branch_name(name: &str) -> Result<(), NameRefusal> {
    if name.is_empty() {
        return Err(NameRefusal::Empty);
    }
    if name.len() > MAX_BRANCH_LEN {
        return Err(NameRefusal::TooLong);
    }
    let segs: Vec<&str> = name.split('/').collect();
    if segs.iter().any(|s| s.is_empty()) {
        return Err(NameRefusal::EmptySegment);
    }
    for s in &segs {
        check_segment(s, MAX_SEGMENT_LEN)?;
    }
    if segs[0] == RUN_NAMESPACE {
        return Err(NameRefusal::ReservedNamespace);
    }
    Ok(())
}

/// 단위(스토리·작업) 가지 이름: 패턴 `features/parent/{story}`·`features/{task}` 의 자리표 `{type}`·`{story}`·`{task}` 를 채워 안전한지 본다.
/// 채우는 값은 모두 한 칸(`/` 없음)이어야 한다. 값이 없는 자리표가 남으면 `{` 가 BadChar 로 걸린다. 원본: `renderBranch` + `validBranch`
pub fn unit_branch_name(pattern: &str, kind: Option<&str>, story: Option<&str>, task: Option<&str>) -> Result<String, NameRefusal> {
    let mut out = pattern.to_string();
    for (key, val) in [("{type}", kind), ("{story}", story), ("{task}", task)] {
        if let Some(v) = val {
            check_segment(v, MAX_SEGMENT_LEN)?;
            out = out.replace(key, v);
        }
    }
    check_branch_name(&out)?;
    Ok(out)
}

/// 폴더·표시 이름에서 안전한 id 를 만든다. 이미 안전하면 그대로, 바꿨으면 같은 이름이 같은 id 가 되도록 원래 글의 해시(8자리)를 뒤에 붙인다
/// (한글 이름 둘이 같은 id 로 겹치지 않게). 결과는 늘 `check_segment` 를 지난다.
pub fn normalise_id(raw: &str) -> Result<String, NameRefusal> {
    let mut s: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect();
    while s.contains("..") {
        s = s.replace("..", ".");
    }
    let mut s = s.trim_matches(['.', '-']).to_string();
    while s.to_ascii_lowercase().ends_with(".lock") {
        s.truncate(s.len() - 5);
        s = s.trim_matches(['.', '-']).to_string();
    }
    if is_reserved_device(&s).is_some() {
        s.insert(0, '_');
    }
    if s == raw && !s.is_empty() && s.len() <= MAX_SEGMENT_LEN {
        check_segment(&s, MAX_SEGMENT_LEN)?;
        return Ok(s);
    }
    let mut head: String = s.chars().take(MAX_SEGMENT_LEN - 9).collect();
    head = head.trim_end_matches(['.', '-']).to_string();
    if head.is_empty() {
        head.push_str("id");
    }
    let out = format!("{head}-{:08x}", fnv1a32(raw.as_bytes()));
    check_segment(&out, MAX_SEGMENT_LEN)?;
    Ok(out)
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193))
}

// ── 출발 가지 ────────────────────────────────────────

/// 출발 가지를 어디서 골랐나
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartSource {
    /// 호출자가 정한 가지(고정 설정의 `base_branch`, 지난 실행·프로젝트 설정의 마지막 출발 가지)
    Requested,
    /// 가지 자리: `live`·`dev`·`master` 순(`rules::promotion::branch_roles` 가 이름 짝을 정한다)
    Role(&'static str),
    /// 자리 가지가 없어 지금 걸린 가지
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartBranch {
    pub name: String,
    pub source: StartSource,
}

/// 출발 가지를 못 골랐거나 만들지 못한 까닭
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchError {
    /// 정해 준 출발 가지가 저장소에 없다
    Missing(String),
    /// 커밋이 하나도 없는 저장소(출발할 머리가 없다)
    NoCommits,
    /// 자리 가지(live·dev·master)도 없고 걸린 가지도 없다(떨어진 HEAD)
    NoStartBranch,
    /// 거절한 이름
    Name(NameRefusal),
    /// 같은 이름(대소문자만 달라도)의 가지가 이미 있다
    Exists(String),
    /// git 이 실패했다
    Git(String),
}

impl fmt::Display for BranchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BranchError::Missing(b) => write!(f, "출발 가지 {b} 가 저장소에 없습니다"),
            BranchError::NoCommits => write!(f, "커밋이 하나도 없는 저장소라 출발 가지가 없습니다"),
            BranchError::NoStartBranch => write!(f, "출발 가지를 고를 수 없습니다(live·dev·master 가 없고 걸린 가지도 없음)"),
            BranchError::Name(n) => write!(f, "{n}"),
            BranchError::Exists(b) => write!(f, "{b} 가지가 이미 있습니다"),
            BranchError::Git(m) => write!(f, "git: {m}"),
        }
    }
}

impl std::error::Error for BranchError {}

impl From<NameRefusal> for BranchError {
    fn from(n: NameRefusal) -> Self {
        BranchError::Name(n)
    }
}

fn git_fail(e: PortError) -> BranchError {
    BranchError::Git(e.message)
}

fn first_line(out: &crate::ports::GitOutput) -> String {
    format!("{}\n{}", out.stderr, out.stdout).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// 저장소의 가지 이름들(`refs/heads/`, 이름순). 원본: `branchHeads` 의 이름 쪽
pub fn list_branches(git: &dyn Git, repo: &str) -> Result<Vec<String>, BranchError> {
    let out = git.run(Path::new(repo), &["for-each-ref", "--format=%(refname)", "refs/heads/"]).map_err(git_fail)?;
    if out.code != 0 {
        return Err(BranchError::Git(first_line(&out)));
    }
    let mut names: Vec<String> = out.stdout.lines().filter_map(|l| l.trim_end_matches('\r').strip_prefix("refs/heads/")).map(str::to_string).collect();
    names.sort();
    Ok(names)
}

/// 지금 걸린 가지. 떨어진 HEAD 면 `None`. 원본: `currentBranch`
pub fn current_branch(git: &dyn Git, repo: &str) -> Result<Option<String>, BranchError> {
    let out = git.run(Path::new(repo), &["symbolic-ref", "-q", "--short", "HEAD"]).map_err(git_fail)?;
    if out.code != 0 {
        return Ok(None);
    }
    let name = out.stdout.trim();
    Ok((!name.is_empty()).then(|| name.to_string()))
}

fn has_commits(git: &dyn Git, repo: &str) -> Result<bool, BranchError> {
    let out = git.run(Path::new(repo), &["rev-parse", "-q", "--verify", "HEAD^{commit}"]).map_err(git_fail)?;
    Ok(out.code == 0)
}

/// 출발 가지. 정해 준 가지(`requested`)가 있으면 저장소에 있어야 하고, 없으면 가지 자리 `live` → `dev` → `master`(이름 짝은 `branch_roles`),
/// 그것도 없으면 지금 걸린 가지. 원본: 고정 설정 `baseBranch` 와 보드 `pickDefaultBases`(`n.live ?? n.dev ?? n.master ?? i.current`)
pub fn start_branch(git: &dyn Git, repo: &str, requested: Option<&str>) -> Result<StartBranch, BranchError> {
    let branches = list_branches(git, repo)?;
    if branches.is_empty() && !has_commits(git, repo)? {
        return Err(BranchError::NoCommits);
    }
    if let Some(r) = requested {
        return if branches.iter().any(|b| b == r) {
            Ok(StartBranch { name: r.to_string(), source: StartSource::Requested })
        } else {
            Err(BranchError::Missing(r.to_string()))
        };
    }
    let roles = branch_roles(&branches);
    if let Some(n) = roles.live {
        return Ok(StartBranch { name: n, source: StartSource::Role("live") });
    }
    if let Some(n) = roles.dev {
        return Ok(StartBranch { name: n, source: StartSource::Role("dev") });
    }
    if let Some(n) = roles.master {
        return Ok(StartBranch { name: n, source: StartSource::Role("master") });
    }
    match current_branch(git, repo)? {
        Some(c) if branches.contains(&c) => Ok(StartBranch { name: c, source: StartSource::Current }),
        _ => Err(BranchError::NoStartBranch),
    }
}

/// 출발 가지 머리에서 작업 가지를 만들고 원래 작업 트리를 그 가지로 바꾼다(`git switch -c`). 이름은 `check_branch_name` 이 아니라 호출자가
/// `branch_name` 으로 만든 것이어야 한다(여기서는 이름 공간 `autopilot/` 만 허용하고 나머지 칸 규칙을 다시 본다). 같은 이름이 이미 있으면(대소문자만 달라도) 거절한다.
pub fn create_work_branch(git: &dyn Git, repo: &str, work: &str, base: &str) -> Result<(), BranchError> {
    let segs: Vec<&str> = work.split('/').collect();
    if segs.len() != 2 || segs[0] != RUN_NAMESPACE {
        return Err(BranchError::Name(NameRefusal::ReservedNamespace));
    }
    check_segment(segs[1], MAX_SEGMENT_LEN)?;
    if let Some(same) = list_branches(git, repo)?.into_iter().find(|b| b.eq_ignore_ascii_case(work)) {
        return Err(BranchError::Exists(same));
    }
    let out = git.run(Path::new(repo), &["switch", "-c", work, base]).map_err(git_fail)?;
    if out.code != 0 {
        return Err(BranchError::Git(first_line(&out)));
    }
    Ok(())
}

// ── run.json `branches` 기록 ─────────────────────────

/// 저장소 하나의 작업 가지 기록(`branches.repos.<저장소>`). 꼴은 엔진 `repoRecord` 와 `engine::run` 의 읽는 쪽(`work`·`original`·`on`·`dirty_start`·`restored`)이 정한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRecord {
    pub repo: String,
    /// 출발 가지와 그 머리 커밋
    pub base_branch: String,
    pub base_head: String,
    /// 이 실행의 작업 가지(`autopilot/<id>`)
    pub work: String,
    /// 출발 때 걸려 있던 가지(떨어진 HEAD 면 `None` → `null`)와 그때 HEAD 커밋
    pub original: Option<String>,
    pub original_head: String,
    /// 출발 때 이미 있던 변경(porcelain 줄)
    pub dirty_start: Vec<String>,
}

fn branches_mut(extra: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, BranchError> {
    let slot = extra.entry("branches").or_insert_with(|| Value::Object(Map::new()));
    slot.as_object_mut().ok_or_else(|| BranchError::Git("run.json 의 branches 가 객체가 아니다".to_string()))
}

fn child_obj<'a>(m: &'a mut Map<String, Value>, key: &str) -> Result<&'a mut Map<String, Value>, BranchError> {
    let slot = m.entry(key).or_insert_with(|| Value::Object(Map::new()));
    slot.as_object_mut().ok_or_else(|| BranchError::Git(format!("run.json branches 의 {key} 가 객체가 아니다")))
}

/// 저장소의 작업 가지 기록을 `branches.repos.<저장소>` 에 쓴다(있으면 새로 쓴다). 저장소 키는 글 그대로(`C:\w\a` 와 `C:/w/a` 는 다른 키다 — 규칙이 글 그대로 견준다).
/// 같은 저장소를 두 번 쓰면 `made` 는 합치고 나머지는 새 값으로 바꾼다.
pub fn record_branch(extra: &mut Map<String, Value>, rec: &WorkRecord) -> Result<(), BranchError> {
    let br = branches_mut(extra)?;
    let repos = child_obj(br, "repos")?;
    let mut made: Vec<Value> = repos
        .get(&rec.repo)
        .and_then(|r| r.get("made"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !made.iter().any(|m| m.as_str() == Some(rec.work.as_str())) {
        made.push(Value::from(rec.work.as_str()));
    }
    let mut one = Map::new();
    one.insert("base".into(), serde_json::json!({ "branch": rec.base_branch, "head": rec.base_head }));
    one.insert("work".into(), Value::from(rec.work.as_str()));
    one.insert("original".into(), rec.original.as_deref().map_or(Value::Null, Value::from));
    one.insert("original_head".into(), Value::from(rec.original_head.as_str()));
    one.insert("restored".into(), Value::Null);
    one.insert("dirty_start".into(), Value::from(rec.dirty_start.clone()));
    one.insert("on".into(), Value::from(rec.work.as_str()));
    one.insert("made".into(), Value::Array(made));
    repos.insert(rec.repo.clone(), Value::Object(one));
    br.entry("stories").or_insert_with(|| Value::Object(Map::new()));
    Ok(())
}

/// 스토리 가지 기록 `branches.stories.<키>.repos.<저장소>` = `{branch, base, head, merged, unmerged}`(원본 `ensureBranch` 의 `setStoryRepo`).
/// 같은 저장소에 앞서 쓴 `merged`·`unmerged` 줄은 지키고, `story_repos` 가 새 꼴로 읽도록 항상 `repos` 아래에 쓴다(옛 꼴 `{repo, …}` 는 새 꼴로 옮긴다).
pub fn record_story_branch(extra: &mut Map<String, Value>, story: &str, repo: &str, branch: &str, head: &str) -> Result<(), BranchError> {
    let br = branches_mut(extra)?;
    let stories = child_obj(br, "stories")?;
    let old = crate::rules::branches::story_repos(stories.get(story));
    let mut repos: Map<String, Value> = old.into_iter().map(|(k, v)| (k, Value::Object(v))).collect();
    let mut e: Map<String, Value> = repos.get(repo).and_then(Value::as_object).cloned().unwrap_or_default();
    e.insert("branch".into(), Value::from(branch));
    e.insert("base".into(), Value::from(head));
    e.insert("head".into(), Value::from(head));
    e.entry("merged").or_insert_with(|| Value::Array(Vec::new()));
    e.entry("unmerged").or_insert_with(|| Value::Array(Vec::new()));
    repos.insert(repo.to_string(), Value::Object(e));
    let mut s = Map::new();
    s.insert("repos".into(), Value::Object(repos));
    stories.insert(story.to_string(), Value::Object(s));
    Ok(())
}
