//! 지난 실행 이어 가기의 미룬 사람 몫(next_start) 규칙. 원본: `Sources/AutopilotRules/Carry.swift`.
//! 이어 가기는 고른 실행과 그 실행이 이어 간 줄(continues 를 거슬러)의 미룬 몫을 출발 카드로 가져온다. 그 줄 밖의 실행
//! (이어 가지 않은 옛 실행, 같은 프로젝트의 다른 갈래)에서 미룬 몫도 아직 열려 있으면 보인다(`open_parked`).
//! 기록 저장소는 바깥이 읽고 값으로 넘긴다. 여기는 I/O·시계·환경변수 없이 값만 다루는 순수 규칙이다.
//! Windows: 경로·프로세스 인자를 다루지 않는다. 실행 id·제목은 글 그대로(대소문자·`\`·`/` 구분) 견준다.

use std::collections::{HashMap, HashSet};

/// 미룬 몫 한 줄(기록 저장소 carry) — 어느 실행의 몇째 단위가 남겼나. 원본: `APCarryRow`
#[derive(Debug, Clone, PartialEq)]
pub struct CarryRow {
    pub id: String,
    pub run: String,
    pub unit: i64,
    pub kind: String,
    pub title: String,
    pub why: String,
    /// 그 실행의 출발 시각(최근 것부터 보이려고). 없으면 0 으로 센다
    pub started: Option<f64>,
}

/// 이어 간 줄 밖의 열린 미룬 몫. 같은 프로젝트의 next_start 가운데 줄(`chain`: 고른 실행과 그것이 이어 간 실행들) 밖의
/// 실행이 남긴 것, 어느 실행의 출발 카드에서도 정함(decide)·닫음(close)으로 답하지 않은 것(다시 미룸 park 은 열린 채다),
/// 이미 카드로 가져온 것(`shown` 의 (id, 제목))과 제목이 같은 것은 뺀다. 같은 제목이 여럿이면 최근 실행의 것 하나.
/// 최근 실행부터. 원본: `apOpenParked(rows:answers:chain:shown:)`
///
/// 정렬은 `(started 없으면 0, run, -unit)` 내림차순이다. 원본의 `run` 견줌은 Swift 문자열(유니코드 정규 동치)이고
/// 여기는 UTF-8 바이트 순서다 — 실행 id 는 ASCII 라 같다. `started` 가 NaN 이면 원본은 견줌이 거짓이라 자리가 정해지지
/// 않고 여기는 `total_cmp` 로 가장 앞에 둔다(기록에는 NaN 이 오지 않는다).
pub fn open_parked(
    rows: &[CarryRow],
    answers: &HashMap<String, String>,
    chain: &HashSet<String>,
    shown: &[(String, String)],
) -> Vec<CarryRow> {
    let shown_ids: HashSet<&str> = shown.iter().map(|(id, _)| id.as_str()).collect();
    let shown_titles: HashSet<&str> = shown.iter().map(|(_, title)| title.as_str()).collect();
    let mut sorted: Vec<&CarryRow> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        b.started
            .unwrap_or(0.0)
            .total_cmp(&a.started.unwrap_or(0.0))
            .then_with(|| b.run.cmp(&a.run))
            .then_with(|| a.unit.cmp(&b.unit))
    });
    let mut out: Vec<CarryRow> = Vec::new();
    let mut titles: HashSet<&str> = HashSet::new();
    for r in sorted {
        let answered = matches!(answers.get(&r.id).map(String::as_str), Some("decide" | "close"));
        if r.kind == "next_start"
            && !chain.contains(&r.run)
            && !answered
            && !shown_ids.contains(r.id.as_str())
            && !shown_titles.contains(r.title.as_str())
            && titles.insert(r.title.as_str())
        {
            out.push(r.clone());
        }
    }
    out
}
