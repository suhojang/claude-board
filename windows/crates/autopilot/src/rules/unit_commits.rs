//! 단위 커밋 기록과 승격 뒤 되돌림 신호. 원본: `Sources/AutopilotRules/UnitCommits.swift`(291줄, 1:1).
//!
//! - 단위 커밋: 단위 기록(engine/unit-<n>.json 꼴)의 commits·merge 와 run.json `branches.promoted` 에서 `unit_commits` 표 줄로
//!   (기록 저장소 판 5, 엔진이 이 트레이트의 짝지은 결과를 `judge_misses`(signal revert) 에 쓴다).
//! - 되돌림 신호: 승격한 가지를 그 승격 머리 뒤로만 훑어 "This reverts commit <sha>" 줄의 이름이 기록된 커밋과 같을 때만 짝짓는다(시각 어림 없음).
//!   되돌림을 다시 되돌린 것(다시 살림)은 신호를 거둔다. 커밋 메시지는 믿지 않는 자료다 — 이름 꼴(7~40자 16진수)만 받고, 짝지은 결과만 남긴다.
//!
//! 원본 대응 규칙(이 판의 다른 청들과 같다):
//! - `$` 앵커는 글자 끝만 본다(ICU `$` 는 마지막 줄바꿈 앞도 맞지만, 여기가 더 엄격한 쪽 — 자료는 늘 한 줄 글이다).
//! - 소수`Double` 은 그대로 `f64`, 16진수 이름 꼴은 손으로 쓴 판별자(위 3개 — 바위놀이 정규식 주석).
//! - 글자 수(`count`·`prefix`)는 `formats::grapheme_len`(Swift `Character` 수 근사, UAX #29 주요 규칙만)이다.
//! - 훑기 알고리즘은 원본과 같은 재귀+메모 구조다. 한 가지 다름: `memo` 는 (깊이·경로를 무시한 캐시라) Swift 사전 순서에 따라
//!   결과가 갈릴 수 있는데 — Swift 사전 순서는 매번 달라져 결정이 아니므로, 여기서는 `BTreeMap` 으로 고정 순서를 둔다.
//!
//! 승격 뒤 되돌림 짝지은 결과는 `judge_misses`(signal `revert`) 로. git 은 `ApRevertWorld` 포트로만 — 진짜 어댑터는 엔진, 가짜는 시험에만.
//!
//! ⚠️ 임시 함께 둔 조각(이 파일이 잡는 시험을 돌리기 위해 1:1 로 옮긴 것) — 각 원본 청이 그 모듈로 옮겨 갈 자리다:
//! - `Takeover.swift` 의 `takeover_clean` 은 `takeover.rs` 의 것을 쓴다.
//! - `Report.swift` 꼬리(565~568줄): `report_ended_statuses`·`missing_unit_reports`(R-보고서 청이 report.rs 로 옮길 때 옮겨 간다).
//! - `Tickets.swift`·`Outbox.swift` 조각: 티켓 표 줄 규칙(`ApTicket*`·`ticket_rows`·`ApOutboxItem` 축약형·`outbox_ticket_rows`)과
//!   `ISSUE_KEY_PATTERN`·`TEMP_TICKET_PATTERN`(R-티켓 청이 이 모듈을 지우게).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::rules::formats::grapheme_len;
use crate::rules::meta::defang_markers;
use crate::rules::takeover::takeover_clean;

/// JSON 객체 하나(`[String: Any]`).
pub type Obj = Map<String, Value>;

/// 커밋 종류. 원본: `APCommitKind`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ApCommitKind {
    /// 단위가 만든 커밋(단위 시작 머리 → 끝 머리)
    #[serde(rename = "unit")]
    Unit,
    /// 판정을 통과한 작업 가지를 스토리 가지로 합친 병합 커밋(엔진, 단위 하나)
    #[serde(rename = "story_merge")]
    StoryMerge,
    /// finish merge 가 합칠 쪽을 받는 가지로 합친 병합 커밋(실행 또는 스토리)
    #[serde(rename = "finish_merge")]
    FinishMerge,
    /// finish promote 가 기존 가지를 다음 가지로 올린 병합 커밋
    #[serde(rename = "promote")]
    Promote,
}

impl ApCommitKind {
    /// Swift `rawValue`
    pub fn raw_value(self) -> &'static str {
        match self {
            ApCommitKind::Unit => "unit",
            ApCommitKind::StoryMerge => "story_merge",
            ApCommitKind::FinishMerge => "finish_merge",
            ApCommitKind::Promote => "promote",
        }
    }
}

/// `unit_commits` 한 줄. 원본: `APUnitCommit`
#[derive(Debug, Clone, PartialEq)]
pub struct ApUnitCommit {
    pub run: String,
    /// finish_merge·promote 는 None(실행·스토리 단위의 병합)
    pub unit_n: Option<i64>,
    pub repo: String,
    /// 소문자 16진수 40자(SHA-256 저장소는 64자)
    pub sha: String,
    pub kind: ApCommitKind,
    /// 그 커밋이 든 가지: 단위 → 작업 가지, story_merge → 스토리 가지, finish_merge·promote → 받는 가지
    pub r#ref: String,
    /// story_merge → story:<키>, finish_merge·promote → 합칠 쪽(autopilot·story:<키>·branch:<가지>)
    pub source: Option<String>,
    pub at: f64,
}

impl ApUnitCommit {
    #[allow(clippy::too_many_arguments)] // Swift init 에도 인자 8개다(1:1 형태 유지)
    pub fn new(
        run: &str, unit_n: Option<i64>, repo: &str, sha: &str, kind: ApCommitKind, r#ref: &str, source: Option<&str>, at: f64,
    ) -> Self {
        ApUnitCommit { run: run.to_string(), unit_n, repo: repo.to_string(), sha: sha.to_string(), kind, r#ref: r#ref.to_string(), source: source.map(str::to_string), at }
    }
}

// ---- 기록할 커밋 이름 꼴(엔진이 git 에서 받은 전체 이름) ----

/// 정규식 `^[0-9a-f]{40}([0-9a-f]{24})?$` 을 판별자로 옮긴 것(40자 또는 SHA-256 64자).
fn is_full_sha_shape(t: &str) -> bool {
    t.len() == 40 || t.len() == 64
}

/// 정규식 `^[0-9a-f]{7,40}$` 을 판별자로 옮긴 것.
fn is_revert_sha_shape(t: &str) -> bool {
    (7..=40).contains(&t.len())
}

/// 기록할 커밋 이름 꼴. 원본: `apFullSHA` — `lowercased()` 뒤 전체 이름 판별자를 지난 값만(소문자로 고쳐 돌려준다)
pub fn full_sha(s: &str) -> Option<String> {
    let t = s.to_lowercase();
    (is_full_sha_shape(&t) && t.chars().all(|c| c.is_ascii_hexdigit())).then_some(t)
}

/// 되돌림 줄의 커밋 이름 꼴(사람이 고쳐 쓸 수 있는 자리): 16진수 7~40자만. 원본: `apRevertSHA`
pub fn revert_sha(s: &str) -> Option<String> {
    let t = s.to_lowercase();
    (is_revert_sha_shape(&t) && t.chars().all(|c| c.is_ascii_hexdigit())).then_some(t)
}

/// 가지 이름 꼴(기록에서 읽은 받는 가지) — git 명령줄에 넘기므로 좁게. 원본: `apRefNameOK` (`^[A-Za-z0-9][A-Za-z0-9._/-]{0,199}$`)
pub fn ref_name_ok(b: &str) -> bool {
    let chars: Vec<char> = b.chars().collect();
    let head_ok = matches!(chars.first(), Some(c) if c.is_ascii_alphanumeric());
    let tail_ok = chars.len() <= 200
        && chars.iter().skip(1).all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    head_ok && tail_ok && !b.contains("..") && !b.contains("//") && !b.ends_with('/') && !b.ends_with(".lock") && !b.ends_with('.')
}

// ---- JSON 다루기(branches.rs 의 도움들과 같다. 세 군째라 공용으로 뺄 자리 — branches.rs 청과 합칠 때 묶는다) ----

/// JSON 숫자만 읽는다(원본 `as? NSNumber`; JSON `true`·`false` 도 1·0 으로 읽는다)
fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `as? [Any] ?? []`
fn rows(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// `as? [String: Any] ?? [:]`
fn obj(v: Option<&Value>) -> &Obj {
    static EMPTY: OnceLock<Obj> = OnceLock::new();
    v.and_then(Value::as_object).unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}

/// 단위 하나에 남길 커밋 수 상한(넘으면 엔진이 more 를 적음). 원본: `apUnitCommitMax`
pub const UNIT_COMMIT_MAX: usize = 500;

/// 단위 기록 하나(엔진 비공개 사본 engine/unit-<n>.json 꼴)에서 줄로: commits(저장소·가지·커밋들)와 merge(스토리로 합친 병합 커밋).
/// 원본: `apUnitCommitRows`
pub fn unit_commit_rows(run: &str, n: i64, engine: &Obj) -> Vec<ApUnitCommit> {
    let mut out: Vec<ApUnitCommit> = Vec::new();
    let mut seen = HashSet::new();
    let at = num(engine.get("ended")).or_else(|| num(engine.get("started"))).unwrap_or(0.0);
    let commits: Vec<&Obj> = rows(engine.get("commits")).iter().filter_map(Value::as_object).collect();
    for c in &commits {
        let Some(repo) = c.get("repo").and_then(Value::as_str) else { continue };
        if !repo.starts_with('/') {
            continue;
        }
        let branch = c.get("branch").and_then(Value::as_str).unwrap_or("");
        for s in rows(c.get("shas")).iter().take(UNIT_COMMIT_MAX) {
            let Some(sha) = s.as_str().and_then(full_sha) else { continue };
            if !seen.insert(repo.to_string() + "\u{0}" + &sha) {
                continue;
            }
            out.push(ApUnitCommit::new(run, Some(n), repo, &sha, ApCommitKind::Unit, branch, None, at));
        }
    }
    // 스토리로 합친 병합 커밋(앞으로 감기는 새 커밋이 없어 남기지 않는다). 저장소는 merge.repo, 옛 기록(⑬-2 전)은 단위 커밋의 저장소가 하나일 때 그것
    let m = obj(engine.get("merge"));
    if m.get("state").and_then(Value::as_str) == Some("merged") && m.get("how").and_then(Value::as_str) == Some("merge") {
        if let Some(sha) = m.get("head").and_then(Value::as_str).and_then(full_sha) {
            let repos: HashSet<&str> = commits.iter().filter_map(|c| c.get("repo").and_then(Value::as_str)).collect();
            let repo = m.get("repo").and_then(Value::as_str).or_else(|| (repos.len() == 1).then(|| *repos.iter().next().unwrap()));
            if let Some(repo) = repo.filter(|r| r.starts_with('/')) {
                let branch = m.get("branch").and_then(Value::as_str).unwrap_or("");
                let source = m.get("story").and_then(Value::as_str).map(|s| format!("story:{s}"));
                let at = num(m.get("at")).unwrap_or(at);
                out.push(ApUnitCommit::new(run, Some(n), repo, &sha, ApCommitKind::StoryMerge, branch, source.as_deref(), at));
            }
        }
    }
    out
}

/// run.json branches.promoted 에서 finish 가 만든 병합 커밋 줄(how merge 의 to). 앞으로 감기·그대로는 새 커밋이 없다. 원본: `apFinishCommitRows`
pub fn finish_commit_rows(run: &str, promoted: &[Obj]) -> Vec<ApUnitCommit> {
    let mut out: Vec<ApUnitCommit> = Vec::new();
    for p in promoted {
        if p.get("how").and_then(Value::as_str) != Some("merge") {
            continue;
        }
        let Some(repo) = p.get("repo").and_then(Value::as_str) else { continue };
        if !repo.starts_with('/') {
            continue;
        }
        let Some(sha) = p.get("to").and_then(Value::as_str).and_then(full_sha) else { continue };
        let Some(into) = p.get("into").and_then(Value::as_str) else { continue };
        let src = p.get("source").and_then(Value::as_str).unwrap_or("");
        let kind = if src.starts_with("branch:") { ApCommitKind::Promote } else { ApCommitKind::FinishMerge };
        let at = num(p.get("at")).unwrap_or(0.0);
        out.push(ApUnitCommit::new(run, None, repo, &sha, kind, into, Some(src), at));
    }
    out
}

// ---- 되돌림 줄 ----

/// 커밋 메시지에서 되돌린 커밋 이름들 — 줄 앞(빈칸 뺌)이 "This reverts commit " 이고 바로 뒤가 16진수 7~40자, 그 뒤는 끝·마침표·쉼표·빈칸·괄호일 때만.
/// git revert 는 "This reverts commit <sha>."(병합은 "<sha>, reversing" 뒤 줄바꿈), revert.reference 는 "<짧은 이름> (<제목>, <날짜>)." 꼴로 쓴다.
/// 메시지는 앞 64 KB·200줄만 본다(믿지 않는 자료). 원본: `apRevertTargets`
pub fn revert_targets(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = message.as_bytes();
    // Swift `String(decoding:prefix(64·1024), as: UTF8.self)` — 잘린 자리의 나머지 반 짝 글자는 대체 문자(U+FFFD)로
    let head = if bytes.len() > 65_536 { String::from_utf8_lossy(&bytes[..65_536]).into_owned() } else { message.to_string() };
    for raw in head.split('\n').take(200) {
        let line = raw.trim_start_matches([' ', '\t']);
        let lead = "this reverts commit ";
        if !(grapheme_len(line) > lead.len() && line.chars().take(lead.len()).map(|c| c.to_ascii_lowercase()).collect::<String>() == lead) {
            continue;
        }
        let rest = &line[lead.len()..];
        let hex_len = rest.chars().take_while(|c| c.is_ascii_hexdigit()).map(char::len_utf8).sum::<usize>();
        let hex = &rest[..hex_len];
        let after = rest[hex_len..].chars().next();
        if !(after.is_none() || matches!(after, Some('.') | Some(',') | Some(' ') | Some('\t') | Some('(') | Some('\r'))) {
            continue;
        }
        let Some(sha) = revert_sha(hex) else { continue };
        if !out.contains(&sha) {
            out.push(sha);
        }
    }
    out
}

/// 훑은 커밋 하나(포트가 준 것). 원본: `APScannedCommit`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApScannedCommit {
    pub sha: String,
    pub message: String,
}

impl ApScannedCommit {
    pub fn new(sha: &str, message: &str) -> Self {
        ApScannedCommit { sha: sha.to_string(), message: message.to_string() }
    }
    /// 되돌림 커밋 첫 줄(다듬기 전 글, 줄 글은 `revert_misses` 가 다듬음) — Swift `subject`
    pub fn subject(&self) -> String {
        self.message.split('\n').next().unwrap_or("").to_string()
    }
}

/// 훑을 자리: 저장소 repo 의 받는 가지 into 를 승격 머리 from 뒤로만(from..refs/heads/into). 원본: `APRevertRange`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApRevertRange {
    pub repo: String,
    pub into: String,
    pub from: String,
}

impl ApRevertRange {
    pub fn new(repo: &str, into: &str, from: &str) -> Self {
        ApRevertRange { repo: repo.to_string(), into: into.to_string(), from: from.to_string() }
    }
}

/// 실행들의 승격 기록(branches.promoted)에서 훑을 자리 — 꼴이 맞는 줄만(저장소 절대 경로·가지 이름 꼴·머리 전체 이름), 같은 자리는 한 번.
/// 원본: `apRevertRanges`
pub fn revert_ranges(promoted: &[Obj]) -> Vec<ApRevertRange> {
    let mut out: Vec<ApRevertRange> = Vec::new();
    for p in promoted {
        let Some(repo) = p.get("repo").and_then(Value::as_str) else { continue };
        if !(repo.starts_with('/') && !repo.contains('\n')) {
            continue;
        }
        let Some(into) = p.get("into").and_then(Value::as_str) else { continue };
        if !ref_name_ok(into) {
            continue;
        }
        let Some(from) = p.get("to").and_then(Value::as_str).and_then(full_sha) else { continue };
        let r = ApRevertRange::new(repo, into, &from);
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

// ---- 훑기 ----

/// git 포트 — 진짜 어댑터는 엔진(EngineRevertWorld, 출발 준비와 같은 git 자리 확인을 지난 저장소만 박은 env 로), 가짜는 시험에만. 원본: `APRevertWorld`
pub trait ApRevertWorld {
    /// repo 의 from..refs/heads/<into> 커밋(새것부터 max 개까지, 이름은 전체 이름). 읽지 못하면 None
    fn commits(&self, repo: &str, into: &str, from: &str, max: usize) -> Option<Vec<ApScannedCommit>>;
    /// 병합 커밋 merge 가 들여온 커밋(merge^1..merge, max 개까지, merge 자신 포함). 읽지 못하면 None
    fn introduced(&self, repo: &str, merge: &str, max: usize) -> Option<Vec<String>>;
}

/// 훑을 자리 하나에서 읽는 커밋 수 상한(새것부터). 원본: `apRevertScanMax`
pub const REVERT_SCAN_MAX: usize = 2000;
/// 원본: `apRevertIntroducedMax`
pub const REVERT_INTRODUCED_MAX: usize = 5000;

/// 짝지은 되돌림 — 단위 하나가 되돌림 커밋 하나에 걸린 것. 원본: `APRevertHit`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApRevertHit {
    pub run: String,
    pub unit_n: i64,
    pub repo: String,
    /// 되돌린 기록 커밋(전체 이름) — 단위 커밋이거나 그 단위를 담은 병합 커밋
    pub target: String,
    pub target_kind: ApCommitKind,
    /// 되돌림 커밋(전체 이름)
    pub revert: String,
    /// 되돌림 커밋 첫 줄(다듬기 전 글, 줄 글은 `revert_misses` 가 다듬음)
    pub subject: String,
}

impl ApRevertHit {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.run, self.unit_n, self.target)
    }
}

/// 되돌림이 다시 되돌려져 거둘 것(그 단위·대상에 산 되돌림이 없음) — Swift `(run: String, unitN: Int, target: String)` 쌍
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApRevertWithdrawn {
    pub run: String,
    pub unit_n: i64,
    pub target: String,
}

/// 훑은 결과. 원본: `APRevertScan`
#[derive(Debug, Clone, Default)]
pub struct ApRevertScan {
    pub hits: Vec<ApRevertHit>,
    pub withdrawn: Vec<ApRevertWithdrawn>,
    pub notes: Vec<String>,
    pub scanned: usize,
}

impl PartialEq for ApRevertScan {
    /// Swift `==` — withdrawn 는 run/unitN/target 셋으로 비교한다
    fn eq(&self, other: &Self) -> bool {
        self.hits == other.hits
            && self.notes == other.notes
            && self.scanned == other.scanned
            && self.withdrawn.iter().map(row_key).eq(other.withdrawn.iter().map(row_key))
    }
}

/// 거둘 쌍 비교용 키 — Swift `"\(run)/\(unitN)/\(target)"`
fn row_key(w: &ApRevertWithdrawn) -> String {
    format!("{}/{}/{}", w.run, w.unit_n, w.target)
}

/// 짧은 이름 풀기 — 기록된 커밋 둘 이상에 맞으면 None(닫힌 쪽). Swift `resolve`
fn resolve(names: &HashSet<String>, p: &str) -> Option<String> {
    let mut found: Option<&String> = None;
    for n in names.iter().filter(|n| n.starts_with(p)) {
        if found.is_some() {
            return None;
        }
        found = Some(n);
    }
    found.cloned()
}

/// 산 되돌림인가: 그것을 되돌린 산 되돌림이 없으면(되돌림의 되돌림은 다시 살림). 고리·깊이 32 를 넘으면 산 것으로 본다. Swift `live`
fn live(memo: &mut HashMap<String, bool>, reverts: &BTreeMap<String, Vec<String>>, r: &str, depth: usize, path: &HashSet<String>) -> bool {
    if let Some(&v) = memo.get(r) {
        return v;
    }
    if depth > 32 || path.contains(r) {
        return true;
    }
    let next: HashSet<String> = path.iter().cloned().chain([r.to_string()]).collect();
    let undone = reverts.iter().any(|(k, v)| v.iter().any(|x| x == r) && live(memo, reverts, k, depth + 1, &next));
    memo.insert(r.to_string(), !undone);
    !undone
}

/// 기록된 커밋 → 그 커밋에 걸린 단위들. Swift `units(of:)`
fn recorded_units(
    scan: &mut ApRevertScan, introduced_cache: &mut HashMap<String, Option<Vec<String>>>, mine: &[&ApUnitCommit], repo: &str, sha: &str, world: &dyn ApRevertWorld,
) -> Vec<(String, i64, ApCommitKind)> {
    let mut out: Vec<(String, i64, ApCommitKind)> = Vec::new();
    for rec in mine {
        if rec.sha != sha {
            continue;
        }
        match rec.kind {
            ApCommitKind::Unit | ApCommitKind::StoryMerge => {
                if let Some(n) = rec.unit_n {
                    if !out.iter().any(|(run, n0, _)| *run == rec.run && *n0 == n) {
                        out.push((rec.run.clone(), n, rec.kind));
                    }
                }
            }
            ApCommitKind::FinishMerge | ApCommitKind::Promote => {
                // 병합이 들여온 커밋에 든 단위 모두(단위 커밋·스토리 병합 커밋으로)
                let inn = match introduced_cache.get(sha) {
                    Some(v @ Some(_)) => v.clone(),
                    _ => {
                        let v = world.introduced(repo, sha, REVERT_INTRODUCED_MAX);
                        introduced_cache.insert(sha.to_string(), v.clone());
                        v
                    }
                };
                let Some(inn) = inn else {
                    scan.notes.push(format!("{repo}: 병합 커밋 {} 이 들여온 커밋을 읽지 못했습니다", short_sha(sha)));
                    continue;
                };
                let set: HashSet<String> = inn.iter().filter_map(|s| full_sha(s)).collect();
                for u in mine {
                    if !matches!(u.kind, ApCommitKind::Unit | ApCommitKind::StoryMerge) || !set.contains(&u.sha) {
                        continue;
                    }
                    if let Some(n) = u.unit_n {
                        if !out.iter().any(|(run, n0, _)| *run == u.run && *n0 == n) {
                            out.push((u.run.clone(), n, rec.kind));
                        }
                    }
                }
            }
        }
    }
    out
}

/// 이름 앞 12자(표시용)
fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// 유스케이스: 훑을 자리마다 커밋을 읽어 되돌림 줄을 기록된 커밋과 짝짓는다. `blocked`: git 자리 확인에 걸린 저장소(→ 까닭) — 그 저장소는 git 을 부르지 않는다.
/// 저장소가 다르면 같은 이름이어도 짝짓지 않는다(복제·포크). 짧은 이름이 기록된 커밋 둘 이상에 맞으면 짝짓지 않는다(닫힌 쪽). 원본: `apScanReverts`
pub fn scan_reverts(ranges: &[ApRevertRange], recorded: &[ApUnitCommit], world: &dyn ApRevertWorld, blocked: &HashMap<String, String>) -> ApRevertScan {
    let mut scan = ApRevertScan::default();
    let mut order: Vec<&str> = Vec::new();
    for r in ranges {
        if !order.contains(&r.repo.as_str()) {
            order.push(&r.repo);
        }
    }
    for repo in order {
        if let Some(why) = blocked.get(repo) {
            scan.notes.push(format!("{repo}: {why} 되돌림을 훑지 않았습니다"));
            continue;
        }
        // 이 저장소의 훑은 커밋(여러 자리에서 겹치면 한 번)
        let mut commits: Vec<ApScannedCommit> = Vec::new();
        let mut seen = HashSet::new();
        for r in ranges.iter().filter(|r| r.repo == repo) {
            match world.commits(repo, &r.into, &r.from, REVERT_SCAN_MAX) {
                None => scan.notes.push(format!("{repo} {}: 커밋을 읽지 못해 되돌림을 훑지 않았습니다", r.into)),
                Some(cs) => {
                    if cs.len() >= REVERT_SCAN_MAX {
                        scan.notes.push(format!("{repo} {}: 승격 뒤 커밋이 {REVERT_SCAN_MAX}개를 넘어 새것부터 그만큼만 봤습니다", r.into));
                    }
                    for c in cs {
                        if let Some(s) = full_sha(&c.sha) {
                            if seen.insert(s.clone()) {
                                commits.push(ApScannedCommit::new(&s, &c.message));
                            }
                        }
                    }
                }
            }
        }
        scan.scanned += commits.len();
        let mine: Vec<&ApUnitCommit> = recorded.iter().filter(|r| r.repo == repo).collect();
        let mut names: HashSet<String> = mine.iter().map(|r| r.sha.clone()).collect();
        names.extend(commits.iter().map(|c| c.sha.clone()));
        // 되돌림 커밋 → 되돌린 이름들(풀린 것만)
        let mut reverts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut subject: BTreeMap<String, String> = BTreeMap::new();
        for c in &commits {
            let ts = revert_targets(&c.message);
            if ts.is_empty() {
                continue;
            }
            let mut full: Vec<String> = Vec::new();
            for t in &ts {
                match resolve(&names, t) {
                    Some(f) => {
                        if f != c.sha {
                            full.push(f);
                        }
                    }
                    None => {
                        if names.iter().any(|n| n.starts_with(t)) {
                            scan.notes.push(format!("{repo}: 되돌림 줄의 짧은 이름 {t} 가 여러 커밋에 맞아 짝짓지 않았습니다"));
                        }
                    }
                }
            }
            if !full.is_empty() {
                reverts.insert(c.sha.clone(), full);
                subject.insert(c.sha.clone(), c.subject());
            }
        }
        let mut introduced_cache: HashMap<String, Option<Vec<String>>> = HashMap::new();
        let mut live_memo: HashMap<String, bool> = HashMap::new();
        let mut hit_keys: HashSet<String> = HashSet::new();
        let mut withdrawn: Vec<ApRevertWithdrawn> = Vec::new();
        for c in &commits {
            let Some(ts) = reverts.get(&c.sha) else { continue };
            let alive = live(&mut live_memo, &reverts, &c.sha, 0, &HashSet::new());
            for t in ts {
                if !mine.iter().any(|m| m.sha == *t) {
                    continue;
                }
                for (run, n, kind) in recorded_units(&mut scan, &mut introduced_cache, &mine, repo, t, world) {
                    if alive {
                        let h = ApRevertHit {
                            run, unit_n: n, repo: repo.to_string(), target: t.clone(), target_kind: kind, revert: c.sha.clone(),
                            subject: subject.get(&c.sha).cloned().unwrap_or_default(),
                        };
                        if hit_keys.insert(h.key()) {
                            scan.hits.push(h);
                        }
                    } else if !withdrawn.iter().any(|w| w.run == run && w.unit_n == n && w.target == *t) {
                        withdrawn.push(ApRevertWithdrawn { run, unit_n: n, target: t.clone() });
                    }
                }
            }
        }
        for w in withdrawn {
            if !hit_keys.contains(&row_key(&w)) {
                scan.withdrawn.push(w);
            }
        }
    }
    scan
}

// ---- 판정 놓침 줄(Misses.swift 의 형은 아래에서 misses.rs 의 것을 옛 이름으로 다시 내놓는다) ----

/// 판정 놓침 줄의 id — <실행 id>/<단위 번호>/revert/<되돌린 기록 커밋 앞 12자>. 원본: `apRevertMissId`
pub fn revert_miss_id(run: &str, unit_n: i64, target: &str) -> String {
    format!("{run}/{unit_n}/revert/{}", short_sha(target))
}

/// 단위 하나의 기록(units 표와 고정 설정에서). 원본: `APRevertUnit`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApRevertUnit {
    pub status: String,
    pub unit_id: Option<String>,
    pub source_key: Option<String>,
    pub title: String,
    pub judge: Option<String>,
    pub judge_digest: Option<String>,
    pub set_id: Option<String>,
}

impl ApRevertUnit {
    pub fn new(status: &str) -> Self {
        ApRevertUnit { status: status.to_string(), unit_id: None, source_key: None, title: String::new(), judge: None, judge_digest: None, set_id: None }
    }
    pub fn with_unit_id(mut self, unit_id: &str) -> Self {
        self.unit_id = Some(unit_id.to_string());
        self
    }
    pub fn with_source_key(mut self, source_key: &str) -> Self {
        self.source_key = Some(source_key.to_string());
        self
    }
    pub fn with_title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }
    pub fn with_judge(mut self, judge: &str, judge_digest: &str) -> Self {
        self.judge = Some(judge.to_string());
        self.judge_digest = Some(judge_digest.to_string());
        self
    }
    pub fn with_set_id(mut self, set_id: &str) -> Self {
        self.set_id = Some(set_id.to_string());
        self
    }
}

impl Default for ApRevertUnit {
    /// Swift init 의 status 는 인자 — 기본 빈 상태
    fn default() -> Self {
        ApRevertUnit::new("")
    }
}

/// 짝지은 되돌림 → 판정 놓침 줄(판정을 통과한 단위만 — 판정 쪽 신호).
/// 돌려줌: (넣을 줄, 지울 것) — 지울 것은 거둔 되돌림의 id(열린 줄만 지운다, 엔진). 원본: `apRevertMisses`
pub fn revert_misses(
    scan: &ApRevertScan, project: &str, units: &HashMap<String, ApRevertUnit>, repo_name: impl Fn(&str) -> String, at: f64,
) -> (Vec<ApMiss>, Vec<String>) {
    let mut put: Vec<ApMiss> = Vec::new();
    for h in &scan.hits {
        let Some(u) = units.get(&format!("{}/{}", h.run, h.unit_n)) else { continue };
        if u.status != "done" {
            continue;
        }
        let subj = takeover_clean(&h.subject.replace('\r', " "), TAKEN_OVER_SUBJECT_MAX);
        let what = match h.target_kind {
            ApCommitKind::Unit => format!("이 단위가 만든 커밋 {}", short_sha(&h.target)),
            ApCommitKind::StoryMerge => format!("이 단위를 스토리 가지로 합친 병합 커밋 {}", short_sha(&h.target)),
            ApCommitKind::FinishMerge => format!("이 단위가 든 합치기 병합 커밋 {}", short_sha(&h.target)),
            ApCommitKind::Promote => format!("이 단위가 든 승격 병합 커밋 {}", short_sha(&h.target)),
        };
        let why = format!(
            "승격 뒤 되돌림: {} 의 커밋 {}(⟦{subj}⟧)가 {what} 을 되돌렸습니다(판정은 통과시켰음)",
            defang_markers(&repo_name(&h.repo)),
            short_sha(&h.revert),
        );
        put.push(ApMiss::new(
            &revert_miss_id(&h.run, h.unit_n, &h.target),
            project,
            &h.run,
            Some(h.unit_n),
            u.unit_id.as_deref(),
            u.source_key.as_deref(),
            ApMissSignal::Revert,
            u.judge.as_deref(),
            if u.judge.is_none() { None } else { u.judge_digest.as_deref() },
            &clip_miss_why(&why),
            u.set_id.as_deref(),
            at,
            &clip_miss_why_at(&u.title, MISS_TITLE_MAX),
        ));
    }
    let drop = scan
        .withdrawn
        .iter()
        .map(|w| revert_miss_id(&w.run, w.unit_n, &w.target))
        .filter(|id| !put.iter().any(|m| m.id == *id))
        .collect();
    (put, drop)
}

// ── 판정 놓침 형: Misses.swift 청이 misses.rs 로 옮겨 갔다 — 시험이 쓰는 옛 이름으로 다시 내놓는다 ──

pub use crate::rules::misses::{clip_miss_why, clip_miss_why_at, miss_side as ap_miss_side, Miss as ApMiss, MissSignal as ApMissSignal, MISS_WHY_MAX};

/// 놓침 줄 제목 상한. 원본: Misses.swift `apUnitMisses` 의 `clip(title, 120)`
const MISS_TITLE_MAX: usize = 120;
/// 되돌림 커밋 첫 줄 상한. 원본: UnitCommits.swift `apRevertMisses` 의 `apTakeoverClean(..., max: 80)`
const TAKEN_OVER_SUBJECT_MAX: usize = 80;

// ── ⚠️ 임시 조각 3: Report.swift 꼬리(565~568줄) — 빠진 단위 보고서. 원본 청(R-보고서-d)이 report.rs 에서 옮겨 간다 ──

/// 끝난 단위의 상태. 원본: Report.swift `apReportEndedStatuses`
pub const REPORT_ENDED_STATUSES: [&str; 5] = ["done", "failed", "capped", "stopped", "skipped"];

/// 빠진 단위 보고서 — 끝난 단위 가운데 단위 폴더는 있는데 보고서(units/<n>/report.html, 보통 파일)가 없는 것(단위 번호, 1부터).
/// `statuses`: 대기열 차례의 단위 상태, `has_dir`·`has_report`: 단위 번호 → 폴더·보고서가 있는가. 원본: Report.swift `apMissingUnitReports`
pub fn missing_unit_reports(statuses: &[impl AsRef<str>], has_dir: impl Fn(usize) -> bool, has_report: impl Fn(usize) -> bool) -> Vec<usize> {
    (1..=statuses.len())
        .filter(|&n| REPORT_ENDED_STATUSES.contains(&statuses[n - 1].as_ref()) && has_dir(n) && !has_report(n))
        .collect()
}

// ── ⚠️ 임시 조각 4: Tickets.swift·Outbox.swift 조각 — 티켓 표 줄 규칙(R-티켓·R-게시 청 porting 때 이 자리를 지운다). 형은 시험에 쓰는 축약형이다 ──

/// 티켓 종류. 원본: Tickets.swift `APTicketKind`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApTicketKind {
    Story,
    Task,
    Move,
}

impl ApTicketKind {
    /// Swift `rawValue`
    pub fn raw_value(self) -> &'static str {
        match self {
            ApTicketKind::Story => "story",
            ApTicketKind::Task => "task",
            ApTicketKind::Move => "move",
        }
    }
}

/// 만들 티켓 하나 — id 는 출발 준비 안에서 바뀌지 않는 이름. 시험에 쓰는 칸만. 원본: Tickets.swift `APTicketEntry`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApTicketEntry {
    pub id: String,
    pub kind: ApTicketKind,
    pub title: String,
    pub parent: String,
}

/// 만들 티켓 계획. 시험에 쓰는 칸만. 원본: Tickets.swift `APTicketPlan`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApTicketPlan {
    pub entries: Vec<ApTicketEntry>,
}

/// 티켓 항목 상태 — 만듦(made)만 표 줄로. 원본: Tickets.swift `APTicketState`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApTicketState {
    #[default]
    Waiting,
    Creating,
    Made,
    Failed,
    Uncertain,
}

impl ApTicketState {
    /// Swift `rawValue`
    pub fn raw_value(self) -> &'static str {
        match self {
            ApTicketState::Waiting => "waiting",
            ApTicketState::Creating => "creating",
            ApTicketState::Made => "made",
            ApTicketState::Failed => "failed",
            ApTicketState::Uncertain => "uncertain",
        }
    }
}

/// 상태 파일(tickets.json)의 항목 기록 하나. 시험에 쓰는 칸만(기본 상태는 waiting). 원본: Tickets.swift `APTicketRecord`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ApTicketRecord {
    pub state: ApTicketState,
    pub key: String,
    pub url: String,
    pub updated: f64,
}

/// 상태 파일. 원본: Tickets.swift `APTicketBook`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ApTicketBook {
    pub entries: BTreeMap<String, ApTicketRecord>,
}

/// 만든 티켓 한 줄 — 키·종류(story·task)·부모 키·제목·주소·원천 키·만든 실행·만든 때.
/// 같은 키는 다시 쓰지 않는다(엔진 ON CONFLICT DO NOTHING). 원본: Tickets.swift `APTicketRow`
#[derive(Debug, Clone, PartialEq)]
pub struct ApTicketRow {
    pub key: String,
    pub kind: String,
    pub parent: String,
    pub title: String,
    pub url: String,
    pub source_key: Option<String>,
    pub run: String,
    pub created: f64,
}

/// Jira 요약 칸 255자 안(240). 원본: Tickets.swift `apTicketTitleMax`
pub const TICKET_TITLE_MAX: usize = 240;

/// 이슈 키 꼴 — `^[A-Z][A-Z0-9_]*-[0-9]{1,9}$` 를 판별자로 옮긴 것. 원본: Outbox.swift `apIssueKeyPattern`
pub fn issue_key_ok(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let Some(dash) = b.iter().position(|c| *c == '-') else { return false };
    // 앞부분(식별자): 첫 글자는 대문자, 그 뒤 대문자·숫자·_
    b[0].is_ascii_uppercase()
        && b[1..dash].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
        && (1..=9).contains(&(b.len() - dash - 1))
        && b[dash + 1..].iter().all(|c| c.is_ascii_digit())
}

/// 임시 티켓 번호 꼴 — `^TMP-[0-9]{1,4}-[0-9]{1,2}$` 를 판별자로 옮긴 것. 원본: Tickets.swift `apTempTicketPattern`
pub fn temp_ticket_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("TMP-") else { return false };
    let Some((n, seq)) = rest.split_once('-') else { return false };
    let n_ok = (1..=4).contains(&n.chars().count()) && n.chars().all(|c| c.is_ascii_digit());
    let seq_ok = (1..=2).contains(&seq.chars().count()) && seq.chars().all(|c| c.is_ascii_digit());
    n_ok && seq_ok
}

/// 출발 확인의 티켓 나무(tickets-plan.json + tickets.json)에서 만든 것(made)만. 부모 바꾸기(move)는 만든 것이 아니라 넣지 않는다.
/// 작업의 원천 키는 그 단위(task:<단위 id>)의 source_key, 스토리는 없음. 부모가 같은 계획의 스토리(story:…)면 그 스토리의 만든 키.
/// 원본: Tickets.swift `apTicketRows`
pub fn ticket_rows(plan: &ApTicketPlan, book: &ApTicketBook, unit_sources: &HashMap<String, String>, run: &str) -> Vec<ApTicketRow> {
    let mut out: Vec<ApTicketRow> = Vec::new();
    for e in &plan.entries {
        if e.kind == ApTicketKind::Move {
            continue;
        }
        let Some(r) = book.entries.get(&e.id) else { continue };
        if r.state != ApTicketState::Made || !issue_key_ok(&r.key) || out.iter().any(|o| o.key == r.key) {
            continue;
        }
        let parent = if e.parent.starts_with("story:") {
            let Some(p) = book.entries.get(e.parent.as_str()) else { continue };
            if p.state != ApTicketState::Made || !issue_key_ok(&p.key) {
                continue;
            }
            p.key.clone()
        } else if issue_key_ok(&e.parent) {
            e.parent.clone()
        } else {
            String::new()
        };
        let src = if e.kind == ApTicketKind::Task {
            e.id.strip_prefix("task:").and_then(|unit_id| unit_sources.get(unit_id)).cloned()
        } else {
            None
        };
        out.push(ApTicketRow {
            key: r.key.clone(),
            kind: e.kind.raw_value().to_string(),
            parent,
            title: e.title.chars().take(TICKET_TITLE_MAX).collect(),
            url: r.url.clone(),
            source_key: src,
            run: run.to_string(),
            created: r.updated,
        });
    }
    out
}

/// 게시 상태. 원본: Outbox.swift `APOutboxState`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApOutboxState {
    #[default]
    Waiting,
    Posting,
    Posted,
    Skipped,
    Failed,
    Uncertain,
}

/// 받을 곳 — 시험에 쓰는 칸만(이슈·부모). 원본: Outbox.swift `APOutboxTarget`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApOutboxTarget {
    pub issue: String,
    pub parent: String,
}

impl ApOutboxTarget {
    pub fn jira(issue: &str, parent: &str) -> Self {
        ApOutboxTarget { issue: issue.to_string(), parent: parent.to_string() }
    }
}

/// 받은 곳(올려서 받은 id·url). 원본: Outbox.swift `APRemote`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApRemote {
    pub id: String,
    pub url: String,
}

/// 올릴 글 하나 — 시험에 쓰는 칸만. 원본: Outbox.swift `APOutboxItem`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ApOutboxItem {
    pub kind: String,
    pub state: ApOutboxState,
    pub target: ApOutboxTarget,
    pub remote: Option<ApRemote>,
    pub title: String,
    pub updated: f64,
}

/// 실행 중에 새로 필요해진 티켓(올릴 글 kind ticket)에서 올린 것(posted, 받은 곳 id 가 이슈 키)만.
/// 종류는 하위 작업(task), 원천 키는 임시 번호(TMP-<단위>-<차례>). 원본: Tickets.swift `apOutboxTicketRows`
pub fn outbox_ticket_rows(items: &[ApOutboxItem], run: &str) -> Vec<ApTicketRow> {
    let mut out: Vec<ApTicketRow> = Vec::new();
    for it in items {
        if it.kind != "ticket" || it.state != ApOutboxState::Posted {
            continue;
        }
        let Some(rm) = it.remote.as_ref() else { continue };
        if !issue_key_ok(&rm.id) || out.iter().any(|o| o.key == rm.id) {
            continue;
        }
        out.push(ApTicketRow {
            key: rm.id.clone(),
            kind: "task".to_string(),
            parent: if issue_key_ok(&it.target.parent) { it.target.parent.clone() } else { String::new() },
            title: it.title.chars().take(TICKET_TITLE_MAX).collect(),
            url: rm.url.clone(),
            source_key: if temp_ticket_ok(&it.target.issue) { Some(it.target.issue.clone()) } else { None },
            run: run.to_string(),
            created: it.updated,
        });
    }
    out
}
