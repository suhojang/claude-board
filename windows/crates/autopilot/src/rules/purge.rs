//! 지난 실행의 "원문 지우기". 원본: `Sources/AutopilotRules/Purge.swift`.
//! 지난 실행 원문은 자동으로 지우지 않고 지난 실행 화면에서 사람이 지운다. 지울 수 있는지(`purge_check`)는 값으로만 정하고(순수),
//! 지우기·기록은 포트(`RawFiles`·`Clock`)로만 한다(`purge_raw`).
//! 지우는 것은 실행 폴더 `runs/<실행 id>/` 하나다(stream 원문·결과 기록·대기열 정의·진행 기록). 기록 저장소의 행(결과 기록 전문 포함)은 남아
//! 지난 실행 목록·단위 결과는 그대로 보이고, 엔진 전용 `private/<실행 id>/`(하네스 사본)도 남긴다.
//!
//! Windows: 경로는 해석하지 않고 글 그대로 견준다. `root`·`run_dir` 는 어댑터가 같은 방식으로 이어 붙여 넘겨야 한다
//! (`root` 는 `\` 로 와도 되고 규칙은 `root + "/runs/" + run` 으로 이어 붙인다). 대소문자·`\\?\` 접두·끝 점/공백·`\` 와 `/` 의 다름은
//! 모두 "다른 자리"로 보아 지우지 않는다(안전한 쪽). 지울 폴더가 링크(정션·심볼릭 링크)인지·읽기 전용 속성·열린 핸들 재시도는 `RawFiles` 어댑터 몫이다.

use serde::{Deserialize, Serialize, Serializer};

use crate::ports::Clock;

/// 지울 수 있는지 정하는 입력. 원본: `APPurgeInput`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeInput {
    /// 실행 id
    pub run: String,
    /// 자율주행 뿌리(…/autopilot)
    pub root: String,
    /// 기록 저장소 runs.run_dir
    pub run_dir: String,
    /// runs.state
    pub state: String,
    /// 엔진이 끝 정리를 마쳤나(run.json 의 wrapped)
    pub wrapped: bool,
    /// 합치기·승격이 남았나(규칙 apMergeOpen)
    pub merge_waiting: bool,
    /// 따로 작업 트리(~/.claude-board/worktrees/<실행 id>)가 남았나
    pub worktree_left: bool,
    /// 출발 준비가 이 실행을 이어 가는 중인가
    pub in_draft: bool,
    /// 실행 폴더가 아직 있나
    pub exists: bool,
}

/// 지울 수 있는가의 답. 원본: `APPurgeDecision`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeDecision {
    /// 지울 자리
    Ok(String),
    /// 이미 없다
    Gone,
    /// 지우지 않는 까닭
    Refuse(String),
}

// 실행 id 꼴: `^[A-Za-z0-9._-]+$` 이고 "." ".." 가 아님(ASCII 글자·숫자·점·밑줄·붙임표만).
// 원본은 ICU 정규식이라 `$` 가 끝 줄바꿈 앞에도 맞는다. 여기서는 끝 줄바꿈도 허용하지 않는다(더 엄격한 쪽, 시험 `extra_trailing_newline_id_is_refused`)
fn run_id_ok(run: &str) -> bool {
    !run.is_empty() && run != "." && run != ".." && run.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// 지울 수 있는가 — 끝나고 정리를 마친 실행, 합치기·승격이 남지 않음(시트가 실행 폴더의 run.json 을 쓴다), 따로 작업 트리가 남지 않음,
/// 출발 준비가 이어 가는 중이 아님. 자리는 뿌리 안 `runs/<실행 id>` 와 글자 그대로 같을 때만(기록의 경로가 다른 자리를 가리키면 지우지 않는다).
/// 원본: `apPurgeCheck`
pub fn purge_check(i: &PurgeInput) -> PurgeDecision {
    if !run_id_ok(&i.run) {
        return PurgeDecision::Refuse("실행 id 꼴이 아니라 지우지 않습니다".to_string());
    }
    let want = format!("{}/runs/{}", i.root, i.run);
    if i.run_dir != want {
        return PurgeDecision::Refuse(format!("실행 폴더가 이 보드의 자율주행 뿌리 밖({})이라 보드에서 지우지 않습니다", i.run_dir));
    }
    if !i.exists {
        return PurgeDecision::Gone;
    }
    if !["done", "stopped", "error"].contains(&i.state.as_str()) {
        return PurgeDecision::Refuse("끝나지 않은 실행입니다".to_string());
    }
    if !i.wrapped {
        return PurgeDecision::Refuse("엔진이 끝 정리를 마치지 않았습니다(하네스 검사·기록 저장소 쓰기가 실행 폴더를 씁니다)".to_string());
    }
    if i.merge_waiting {
        return PurgeDecision::Refuse("합치기·승격이 남았습니다. 합치기·승격이 이 실행 폴더의 기록을 쓰므로 합치거나 버린 뒤에 지웁니다".to_string());
    }
    if i.worktree_left {
        return PurgeDecision::Refuse(format!(
            "따로 작업 트리(~/.claude-board/worktrees/{})가 남아 있습니다. 합치기·승격에서 버리거나 합친 뒤에 지웁니다",
            i.run
        ));
    }
    if i.in_draft {
        return PurgeDecision::Refuse("출발 준비가 이 실행을 이어 가는 중입니다(남은 단위의 정의를 이 실행 폴더에서 읽습니다)".to_string());
    }
    PurgeDecision::Ok(want)
}

// 시각(유닉스 초)이 정수면 정수로 쓴다 — Swift JSONEncoder 가 1793300000 으로 쓰는 것과 같게(소수가 있으면 그대로)
fn ser_at<S: Serializer>(at: &f64, s: S) -> Result<S::Ok, S::Error> {
    if at.is_finite() && at.fract() == 0.0 && at.abs() < 9e15 {
        s.serialize_i64(*at as i64)
    } else {
        s.serialize_f64(*at)
    }
}

/// 지운 기록(`purged/<실행 id>.json`) — 언제, 어디를, 파일 몇 개·몇 바이트, 남긴 것, 지우다 생긴 오류.
/// 필드는 글자 차례(at·bytes·error·files·kept·path·run)로 둔다 — 원본이 키를 정렬해 쓰는 것과 같은 차례로 직렬화된다.
/// `error` 가 없으면 키를 쓰지 않는다(Swift Codable 의 옵셔널과 같다). 원본: `APPurgeRecord`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PurgeRecord {
    #[serde(serialize_with = "ser_at")]
    pub at: f64,
    pub bytes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub files: i64,
    pub kept: Vec<String>,
    pub path: String,
    pub run: String,
}

/// 지워도 남는 것. `<실행 id>` 자리는 `purge_raw` 가 실행 id 로 바꾼다. 원본: `apPurgeKept`
pub const PURGE_KEPT: [&str; 3] = [
    "기록 저장소의 실행·단위 행(결과 기록 전문)",
    "엔진 전용 private/<실행 id>/(하네스 사본)",
    "프로젝트 설정과 자율주행 세트",
];

/// 끝난 실행의 실행 폴더(`runs/<실행 id>/`)와 지운 기록(`purged/<실행 id>.json`)을 다루는 포트. 원본: `APRawFiles`
/// (진짜 어댑터는 `adapters/files.rs` 쪽, 가짜는 시험에만 둔다)
pub trait RawFiles: Send + Sync {
    /// 그 자리가 링크가 아닌 진짜 폴더인가
    fn is_real_dir(&self, path: &str) -> bool;
    /// 안의 파일 수와 바이트(링크는 따라가지 않는다)
    fn usage(&self, path: &str) -> (i64, i64);
    /// 폴더를 통째로 지운다. 되면 `None`, 아니면 까닭
    fn remove(&self, path: &str) -> Option<String>;
    /// 지운 기록을 쓴다(실행 id 마다 한 파일). 되면 true
    fn write_record(&self, run: &str, data: &[u8]) -> bool;
}

/// 원문 지우기의 결과. 원본: `APPurgeResult`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PurgeResult {
    pub ok: bool,
    pub record: Option<PurgeRecord>,
    pub error: Option<String>,
}

/// 유스케이스: 원문 지우기 — 다시 확인(`purge_check`)하고, 링크가 아닌 진짜 폴더일 때만 크기를 센 뒤 지우고 기록을 쓴다.
/// 지우다 실패해도 기록은 쓴다(무엇이 남았는지 사람이 볼 수 있게). 원본: `apPurgeRaw`
///
/// 기록은 키를 글자 차례로 정렬한 들여쓴 JSON 이고 `/` 를 이스케이프하지 않는다(원본의 `.sortedKeys, .prettyPrinted, .withoutEscapingSlashes` 와 같다).
/// 다른 점: 들여쓰기가 `"키": 값`(원본은 `"키" : 값`)이다. 읽는 쪽(JSON 해석기)에는 차이가 없다.
pub fn purge_raw(input: &PurgeInput, files: &dyn RawFiles, clock: &dyn Clock) -> PurgeResult {
    let mut r = PurgeResult::default();
    let target = match purge_check(input) {
        PurgeDecision::Ok(t) => t,
        PurgeDecision::Gone => {
            r.error = Some("실행 폴더가 이미 없습니다".to_string());
            return r;
        }
        PurgeDecision::Refuse(why) => {
            r.error = Some(why);
            return r;
        }
    };
    if !files.is_real_dir(&target) {
        r.error = Some("실행 폴더가 링크이거나 폴더가 아니라 지우지 않습니다".to_string());
        return r;
    }
    let (n_files, n_bytes) = files.usage(&target);
    let err = files.remove(&target);
    let mut rec = PurgeRecord {
        at: clock.now(),
        bytes: n_bytes,
        error: err.clone(),
        files: n_files,
        kept: PURGE_KEPT.iter().map(|k| k.replace("<실행 id>", &input.run)).collect(),
        path: target,
        run: input.run.clone(),
    };
    // 직렬화는 문자열 키·수·글자만이라 실패하지 않는다. 그래도 원본처럼 실패하면 빈 데이터를 쓴다
    let data = serde_json::to_vec_pretty(&rec).unwrap_or_default();
    if !files.write_record(&input.run, &data) {
        rec.error = Some(format!("{}지운 기록을 쓰지 못했습니다", err.as_ref().map(|e| format!("{e} · ")).unwrap_or_default()));
    }
    r.ok = err.is_none();
    r.error = rec.error.clone();
    r.record = Some(rec);
    r
}
