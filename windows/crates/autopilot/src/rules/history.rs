//! 지난 실행 화면의 판정 규칙. 원본: `Sources/AutopilotRules/History.swift`.
//! 기록 저장소는 읽지 않는다. 보드가 읽은 실행 목록(최근 것부터)을 값으로 받는 순수 규칙이다.
//! 원본의 `String ==` 는 유니코드 정규 동치(NFC=NFD)로 견주고 여기는 바이트로 견준다 — 프로젝트 묶음 이름과 실행 id 를
//! 같은 곳(기록 저장소)에서 읽어 오므로 같은 글자가 다른 정규형으로 나뉘어 들어오지 않는다.

/// 지난 실행 목록의 실행 하나 — 판정에 쓰는 칸만(id, 프로젝트 묶음 이름). 원본: `APRunTag`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTag {
    pub id: String,
    pub project: String,
}

impl RunTag {
    pub fn new(id: impl Into<String>, project: impl Into<String>) -> Self {
        RunTag { id: id.into(), project: project.into() }
    }
}

/// 상세에 보일 실행. `selected` 는 사람이 고른 실행(`None` 이면 고르지 않음), `runs` 는 최근 것부터,
/// `project` 는 거른 프로젝트(`None` 이면 모든 프로젝트).
/// 거른 목록 안에서 고른다: 고른 실행이 거른 목록에 있으면 그것, 아니면 거른 목록의 첫 실행(거른 목록이 비면 `None`).
/// 원본: `apHistoryPick(selected:runs:project:)`
pub fn history_pick(selected: Option<&str>, runs: &[RunTag], project: Option<&str>) -> Option<String> {
    let shown: Vec<&RunTag> = runs.iter().filter(|r| project.is_none() || project == Some(r.project.as_str())).collect();
    if let Some(s) = selected {
        if shown.iter().any(|r| r.id == s) {
            return Some(s.to_string());
        }
    }
    shown.first().map(|r| r.id.clone())
}
