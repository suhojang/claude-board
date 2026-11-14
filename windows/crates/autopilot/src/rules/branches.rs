//! 끝난 실행의 가지 기록(엔진 run.json 의 `branches` — 기록 저장소 `runs.branches` 도 같은 꼴)을 읽고 합치기 상태를 정하는 규칙.
//! 원본: `Sources/AutopilotRules/Branches.swift`. 엔진 finish 가 남긴 promoted·failed·discarded 로만 보고 git 은 부르지 않는다.
//! 입력은 JSON 값(`serde_json`)이다. 원본의 `as? String`·`as? [[String: Any]]` 캐스트는 같은 뜻으로 옮겼다: 글이 아니면 빈 글자,
//! 배열 안에 객체가 아닌 칸이 하나라도 있으면 그 배열 전체를 빈 목록으로 본다.
//!
//! 선언(원본 순서, 이름은 `AP` 접두를 뗀 스네이크 꼴): `Step`(APStep) · `Promoted`(APPromoted) · `Failed`(APFailed) · `story_repos`(apStoryRepos) ·
//! `promoted_rows` · `failed_rows` · `unresolved` · `step_done` · `merge_sources` · 비공개 `discarded_sources`(apDiscardedSources) · `merge_open` ·
//! `MergeMark`(APMergeMark, `label`·`has_waiting`) · `merge_mark`(apMergeMark) · `RefFate`(APRefFate) · `discard_rows`(apDiscardRows) ·
//! `carries_base`(apCarriesBase) · `finish_root`(apFinishRoot). 비공개 `discarded_sources` 는 `merge_open` 과 `merge_mark` 가 함께 쓴다.
//!
//! Windows: 저장소 경로(`C:\w\a`·`/mnt/c/w/a`)는 JSON 키·글자 그대로 견주며 고치지 않는다.

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// JSON 객체 — 원본의 `[String: Any]`.
pub type Obj = Map<String, Value>;

/// JSON 숫자만 읽는다(원본 `apNum` = `as? NSNumber`). 글자로 적힌 수는 읽지 않는다.
/// Foundation 의 NSNumber 는 JSON `true`·`false` 도 받아 1·0 으로 읽으므로 같게 옮겼다.
fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `as? String ?? ""`
fn text(o: &Obj, key: &str) -> String {
    o.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// `as? [[String: Any]] ?? []` — 객체가 아닌 칸이 하나라도 있으면 전체가 빈 목록이다.
fn object_rows<'a>(br: &'a Obj, key: &str) -> Vec<&'a Obj> {
    let Some(Value::Array(items)) = br.get(key) else { return Vec::new() };
    let rows: Vec<&Obj> = items.iter().filter_map(Value::as_object).collect();
    if rows.len() == items.len() {
        rows
    } else {
        Vec::new()
    }
}

// ---- 가지 기록 읽기 ----

/// 승격 차례 한 단계(`projects/<프로젝트>.json` promotion: `from` 이 `autopilot` 이면 이 실행의 자율주행 가지). 원본: `APStep`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub from: String,
    pub into: String,
    pub after: Option<String>,
}

impl Step {
    pub fn new(from: &str, into: &str, after: Option<&str>) -> Self {
        Step { from: from.to_string(), into: into.to_string(), after: after.map(str::to_string) }
    }
}

/// 엔진 finish 가 합친 줄(`branches.promoted`) — `source` 는 `autopilot`·`story:<키>`·`branch:<가지>`. 원본: `APPromoted`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Promoted {
    pub source: String,
    pub repo: String,
    pub branch: String,
    pub into: String,
    pub how: String,
    pub to: String,
    pub at: Option<f64>,
}

/// 엔진 finish 가 저장소 하나에서 하지 못한 것(먼저 본 확인을 지난 뒤의 중간 실패, `branches.failed`). 원본: `APFailed`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Failed {
    pub op: String,
    pub source: String,
    pub repo: String,
    pub branch: String,
    pub into: String,
    pub why: String,
    pub at: Option<f64>,
}

/// 엔진 run.json 의 `branches.stories.<키>` 를 저장소별로 읽는다. 새 꼴 `{repos: {<저장소>: {branch, base, head, merged, unmerged}}}`,
/// 옛 꼴(스토리 하나에 저장소 하나 `{repo, branch, base, head, merged, unmerged}`)은 그 저장소 하나로 본다(`repo` 칸은 뗀다).
/// 저장소 기록에 `head` 가 없으면 그 저장소에는 스토리 가지를 만들지 않았다. 원본: `apStoryRepos`
pub fn story_repos(s: Option<&Value>) -> BTreeMap<String, Obj> {
    let Some(Value::Object(s)) = s else { return BTreeMap::new() };
    if let Some(Value::Object(rs)) = s.get("repos") {
        return rs.iter().filter_map(|(k, v)| v.as_object().map(|o| (k.clone(), o.clone()))).collect();
    }
    match s.get("repo").and_then(Value::as_str) {
        Some(r) if !r.is_empty() => {
            let mut o = s.clone();
            o.remove("repo");
            BTreeMap::from([(r.to_string(), o)])
        }
        _ => BTreeMap::new(),
    }
}

/// `branches.promoted` 줄. 원본: `apPromotedRows`
pub fn promoted_rows(br: &Obj) -> Vec<Promoted> {
    object_rows(br, "promoted")
        .into_iter()
        .map(|o| Promoted {
            source: text(o, "source"),
            repo: text(o, "repo"),
            branch: text(o, "branch"),
            into: text(o, "into"),
            how: text(o, "how"),
            to: text(o, "to"),
            at: num(o.get("at")),
        })
        .collect()
}

/// `branches.failed` 줄. 원본: `apFailedRows`
pub fn failed_rows(br: &Obj) -> Vec<Failed> {
    object_rows(br, "failed")
        .into_iter()
        .map(|o| Failed {
            op: text(o, "op"),
            source: text(o, "source"),
            repo: text(o, "repo"),
            branch: text(o, "branch"),
            into: text(o, "into"),
            why: text(o, "why"),
            at: num(o.get("at")),
        })
        .collect()
}

/// 합치기·올리기 가운데 하지 못하고 아직 다시 하지 않은 것 — 그 뒤 같은 합칠 쪽·받는 가지·저장소로 합친 줄(시각이 같거나 뒤)이 없으면 남았다.
/// 시각이 없으면 0 으로 견준다. `merge`·`promote` 가 아닌 못 한 일(cleanup 등)은 세지 않는다. 원본: `apUnresolved`
pub fn unresolved(promoted: &[Promoted], failed: &[Failed], source: Option<&str>, into: &str) -> Vec<Failed> {
    failed
        .iter()
        .filter(|f| {
            ["merge", "promote"].contains(&f.op.as_str())
                && f.into == into
                && source.is_none_or(|s| f.source == s)
                && !promoted.iter().any(|p| {
                    p.source == f.source && p.into == f.into && p.repo == f.repo && p.at.unwrap_or(0.0) >= f.at.unwrap_or(0.0)
                })
        })
        .cloned()
        .collect()
}

/// 단계가 끝났는가 — 엔진 finish 의 차례 규칙: 그 합칠 쪽(`source` 가 없으면 무엇이든)을 `into` 에 합친 줄이 있고, 일부 저장소만 하고 남긴 것이 없다.
/// 원본: `apStepDone`
pub fn step_done(promoted: &[Promoted], failed: &[Failed], source: Option<&str>, into: &str) -> bool {
    promoted.iter().any(|p| p.into == into && source.is_none_or(|s| p.source == s)) && unresolved(promoted, failed, source, into).is_empty()
}

// ---- 합치기 상태 ----

/// 합칠 쪽(엔진 finish 의 source) — 스토리마다 `story:<키>`(키 차례), 스토리가 없거나 `with_autopilot` 이면 자율주행 가지 `autopilot` 을 맨 앞에.
/// 어느 저장소에도 스토리 가지를 만들지 않은 스토리(저장소 기록에 `head` 칸이 없음)는 넣지 않는다. `stories` 가 객체가 아니면 스토리 없음.
/// 원본: `apMergeSources`
pub fn merge_sources(br: &Obj, with_autopilot: bool) -> Vec<String> {
    let empty = Obj::new();
    let st = br.get("stories").and_then(Value::as_object).unwrap_or(&empty);
    let mut keys: Vec<&String> = st.keys().collect();
    keys.sort();
    let mut sources: Vec<String> = keys
        .into_iter()
        .filter(|k| story_repos(st.get(k.as_str())).values().any(|r| r.contains_key("head")))
        .map(|k| format!("story:{k}"))
        .collect();
    if sources.is_empty() || with_autopilot {
        sources.insert(0, "autopilot".to_string());
    }
    sources
}

/// 버린 합칠 쪽(`branches.discarded` 의 `source`). 원본: `apDiscardedSources`(비공개)
fn discarded_sources(br: &Obj) -> BTreeSet<String> {
    object_rows(br, "discarded").into_iter().filter_map(|o| o.get("source").and_then(Value::as_str).map(str::to_string)).collect()
}

/// 합치기·승격에 남은 것이 있는가 — 보드 본체의 끝난 실행 줄 「합치기 기다림」. 엔진 finish 의 promoted·discarded 로만 본다.
/// 합칠 쪽(`merge_sources`)이 버려지지도 승격 차례의 마지막 자율주행 단계에 합쳐지지도 않았거나, 하나라도 그 단계에 합쳤는데 기존 가지끼리의
/// 마지막 단계를 아직 안 했으면 남았다. 자율주행 단계가 없는 차례는 합친 줄이 하나라도 있으면 닿은 것으로 본다.
/// 일부 저장소만 합친 단계(`failed` 가 남고 그 뒤 그 저장소를 합치지 않음)는 끝나지 않은 것이다. 원본: `apMergeOpen`
pub fn merge_open(br: &Obj, steps: &[Step], with_autopilot: bool) -> bool {
    let promoted = promoted_rows(br);
    let failed = failed_rows(br);
    let discarded = discarded_sources(br);
    let done = |src: Option<&str>, into: &str| step_done(&promoted, &failed, src, into);
    let sources = merge_sources(br, with_autopilot);
    let last_auto = steps.iter().rev().find(|s| s.from == "autopilot").map(|s| s.into.as_str());
    let reached = |s: &str| match last_auto {
        Some(into) => done(Some(s), into),
        None => promoted.iter().any(|p| p.source == s),
    };
    if sources.iter().any(|s| !discarded.contains(s) && !reached(s)) {
        return true;
    }
    if let Some(last) = steps.last() {
        if last.from != "autopilot" && sources.iter().any(|s| reached(s)) && !done(None, &last.into) {
            return true;
        }
    }
    false
}

// 지난 실행 목록의 합치기 상태 표지. 색은 보드가 정한다(버림 흐림·합침 초록·기다림 노랑, 섞임은 기다림이 있으면 노랑 아니면 초록)
/// 지난 실행 목록의 합치기 상태 표지. 원본: `APMergeMark`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeMark {
    /// 버림
    Discarded,
    /// 마지막으로 받은 가지
    Promoted(String),
    /// 합치기 기다림
    Waiting,
    /// 합칠 쪽(Jira 면 스토리)마다 결과가 다름 — 그 수
    Mixed { promoted: usize, discarded: usize, waiting: usize },
}

impl MergeMark {
    /// 보드 줄에 보이는 글자. `master`·`main` 으로 받았으면 「까지 올림」, 그 밖은 「에 합침」, 섞임은 0 이 아닌 수만 「 · 」로 잇는다(대소문자를 가린다).
    pub fn label(&self) -> String {
        match self {
            MergeMark::Discarded => "버림".to_string(),
            MergeMark::Promoted(last) => {
                if ["master", "main"].contains(&last.as_str()) {
                    format!("{last} 까지 올림")
                } else {
                    format!("{last} 에 합침")
                }
            }
            MergeMark::Waiting => "합치기 기다림".to_string(),
            MergeMark::Mixed { promoted, discarded, waiting } => [(*promoted, "합침"), (*discarded, "버림"), (*waiting, "기다림")]
                .iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, w)| format!("{w} {n}"))
                .collect::<Vec<_>>()
                .join(" · "),
        }
    }

    /// 아직 합치거나 버리지 않은 합칠 쪽이 있다(지난 실행의 「합치기·승격」 단추를 강조)
    pub fn has_waiting(&self) -> bool {
        match self {
            MergeMark::Waiting => true,
            MergeMark::Mixed { waiting, .. } => *waiting > 0,
            _ => false,
        }
    }
}

/// 합치기 상태: 합칠 쪽(`merge_sources`)마다 버림·합침(`into` 가 있는 합친 줄이 있음)·기다림을 정하고, 모두 버렸으면 버림, 모두 합쳤으면 마지막으로 받은 가지,
/// 모두 기다리면 합치기 기다림, 섞이면 섞인 수. 가지 기록(`repos`)이 없거나 객체가 아니거나 비었으면 `None`.
/// 마지막으로 받은 가지는 `promoted` 의 차례로 마지막이면서 `into` 가 글자인 줄이다(시각으로 고르지 않는다. 빈 글자 `into` 도 글자로 센다 — 원본 compactMap 과 같다).
/// `with_autopilot` 은 `merge_open` 과 같다. 원본: `apMergeMark`
pub fn merge_mark(br: &Obj, with_autopilot: bool) -> Option<MergeMark> {
    if br.get("repos").and_then(Value::as_object).is_none_or(|r| r.is_empty()) {
        return None;
    }
    let discarded = discarded_sources(br);
    let promoted: BTreeSet<String> = promoted_rows(br).into_iter().filter(|p| !p.into.is_empty()).map(|p| p.source).collect();
    let (mut p, mut d, mut w) = (0, 0, 0);
    for s in merge_sources(br, with_autopilot) {
        if discarded.contains(&s) {
            d += 1;
        } else if promoted.contains(&s) {
            p += 1;
        } else {
            w += 1;
        }
    }
    if p == 0 && w == 0 {
        return Some(MergeMark::Discarded);
    }
    if d == 0 && w == 0 {
        if let Some(last) = object_rows(br, "promoted").into_iter().filter_map(|o| o.get("into").and_then(Value::as_str)).next_back() {
            return Some(MergeMark::Promoted(last.to_string()));
        }
    }
    if p == 0 && d == 0 {
        return Some(MergeMark::Waiting);
    }
    Some(MergeMark::Mixed { promoted: p, discarded: d, waiting: w })
}

// ---- 버리기 기록 ----

/// 엔진 finish discard 가 가지 하나를 어떻게 했는가. 원본: `APRefFate`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefFate {
    /// 지웠다(지우기 전 머리)
    Deleted { head: String },
    /// 지우지 못했다(git 이 실패한 까닭)
    Failed(String),
    /// 이미 없다(보드 밖에서 지웠거나 앞서 버림)
    Missing,
    /// 이 실행이 끝난 뒤 같은 이름으로 다시 만들어진 가지라 남겼다
    Foreign,
}

/// 시각 값. Foundation 의 JSON 은 정수인 Double(`5.0`)을 `5` 로 적으므로 같게 정수로 넣는다(그 밖은 실수). 유한하지 않은 값은 `null`.
fn at_value(at: f64) -> Value {
    if at.is_finite() && at.fract() == 0.0 && at.abs() < 9.0e15 {
        Value::from(at as i64)
    } else {
        serde_json::Number::from_f64(at).map_or(Value::Null, Value::Number)
    }
}

fn obj_of(pairs: Vec<(&str, Value)>) -> Obj {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// 버리기 한 가지의 기록 줄(`(discarded, failed)`): 지운 가지는 `branches.discarded`, 지우지 못한 가지는 `branches.failed`.
/// `is_source` 는 그 가지가 합칠 쪽 자체(자율주행 가지, 스토리 가지)인가 — 작업 가지는 false. 합칠 쪽 가지가 이미 없거나(보드 밖에서 지움) 같은 이름으로
/// 다시 만들어져 남겼어도 버린 줄을 `left` 와 함께 적는다 — 그 합칠 쪽은 더 합칠 것이 없으니 끝난 것으로 본다. 작업 가지는 지운 것만 적는다.
/// `recorded` 는 그 가지(합칠 쪽·저장소·가지)에 버린 줄이 이미 있는가 — 있으면 없음·남김을 다시 적지 않는다(원본의 기본값 `false` 는 Rust 에 없어 늘 넘긴다).
/// 원본: `apDiscardRows`
pub fn discard_rows(source: &str, repo: &str, branch: &str, fate: &RefFate, is_source: bool, recorded: bool, at: f64) -> (Option<Obj>, Option<Obj>) {
    let base = |extra: Vec<(&str, Value)>| {
        let mut pairs = vec![("source", Value::from(source)), ("repo", Value::from(repo)), ("branch", Value::from(branch))];
        pairs.extend(extra);
        pairs.push(("at", at_value(at)));
        obj_of(pairs)
    };
    let left = |what: &str| (is_source && !recorded).then(|| base(vec![("left", Value::from(what))]));
    match fate {
        RefFate::Deleted { head } => (Some(base(vec![("head", Value::from(head.as_str()))])), None),
        RefFate::Failed(why) => {
            let mut pairs = vec![("op", Value::from("discard"))];
            pairs.extend([("source", Value::from(source)), ("repo", Value::from(repo)), ("branch", Value::from(branch)), ("why", Value::from(why.as_str()))]);
            pairs.push(("at", at_value(at)));
            (None, Some(obj_of(pairs)))
        }
        RefFate::Missing => (left("missing"), None),
        RefFate::Foreign => (left("foreign"), None),
    }
}

// ---- 합치기·승격 시트 ----

/// 단계의 「출발 가지에만 있던 커밋 n개가 함께 갑니다」 경고를 셀 단계인가. 이 실행의 가지(자율주행·스토리 가지)를 합치는 단계에 앞 단계(`after`)가 있고
/// 그 가지가 출발 가지(`base`)일 때만 센다. 기존 가지끼리 올리는 단계(`branch:<가지>`)는 그 가지 전부를 올리므로 세지 않는다. 원본: `apCarriesBase`
pub fn carries_base(source: &str, after: Option<&str>, base: Option<&str>) -> bool {
    !source.starts_with("branch:") && after.is_some() && base == after
}

// ---- finish 의 뿌리 ----

/// finish 가 쓰는 뿌리(`dev` 인가)와 `--dev` 거부 여부. 그 실행이 출발한 뿌리(run.json 의 dev)를 따르고 `--dev`(`flag`)는 맞는지만 본다: 설치판 뿌리에서 출발한
/// 실행에 `--dev` 면 거부한다. run.json 에 dev 가 없으면(옛 실행) `--dev` 를 따른다. 반환은 `(dev, refuse)`. 원본: `apFinishRoot`
pub fn finish_root(flag: bool, run_dev: Option<bool>) -> (bool, bool) {
    match run_dev {
        None => (flag, false),
        Some(r) => (r, flag && !r),
    }
}
