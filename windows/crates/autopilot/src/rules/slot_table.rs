//! 보드에 그리는 멈출 선 자리 표 규칙. 원본: `Sources/AutopilotRules/SlotTable.swift`(87줄).
//! 엔진들이 기록 저장소에 함께 쓰는 자리 표 둘(`stopline_slots`·`path_claims`, `rules::multirun`)을 보드가 읽기만 해서, 자율주행 중 화면에
//! 「실행 · 단위 · 도는 중/기다림 · 잡은 %p · 시작한 때」를 줄마다 보인다(`slot_table`). 15분 넘게 다시 적지 않은 자리(`slot_live` 가 아님)는
//! 죽은 엔진의 자리로 따로 센다. 표는 실행이 둘 이상이거나 기다리는 실행·죽은 엔진의 자리가 있을 때만 보인다.
//! 기다림의 차례는 엔진 `slot_decide` 와 같은 (기다리기 시작한 때, 실행 id) 차례다. 시각 글자는 보드가 그린다(값만 돌려준다).
//!
//! 선언 대응(원본 순서): `SlotRowState`(APSlotRowState) · `SlotRow`(APSlotRow) · `ClaimRow`(APClaimRow) · `SlotTable`(APSlotTable) ·
//! `slot_table`(apSlotTable) · `slot_state_text`(apSlotStateText) · `slot_pp_text`(apSlotPPText).
//!
//! Swift ↔ Rust 로 달라진 점:
//! - `Slot`·`Claim`·`slot_live`·`slot_left`·`pp_text` 는 `rules::multirun` 의 것을 쓴다(다시 옮기지 않았다).
//! - 원본 `APSlotRowState` 의 `rawValue`(`running`·`waiting`·`dead`)는 `SlotRowState::as_str`, `Identifiable.id`(= `run`)는 쓰는 곳이 없어 옮기지 않았다.
//! - 정렬 키 `($0.run, $0.path) < …`·`($0.since, $0.run) < …`·`($0.beat, $0.run) < …` 와 `keys.sorted()` 의 글 비교는 Swift `String`(유니코드 정규 동등)이고
//!   여기는 UTF-8 바이트 순이다. 실행 id 는 엔진이 만든 ASCII 라 같고, 경로(`paths` 안 순서)는 비 ASCII 가 섞여도 같은 정규형으로 적혀 있으면 같다.
//! - 수 정렬은 Swift 튜플 `<`(앞 칸이 `<` 이면 참, `==` 이면 뒤 칸)와 같고 NaN 은 `==` 로 센다(시각은 NaN 이 아니다).
//! - `viewing` 의 `Optional<String>` 비교(`s.run == viewing`)는 `Some(실행)` 일 때만 같다.
//! - 규칙은 순수하다 — 시계는 `now` 로 받는다.

use crate::rules::multirun::{pp_text, slot_left, slot_live, Claim, Slot, SlotKind};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// 줄의 상태. 원본: `APSlotRowState`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRowState {
    /// 자리를 잡고 단위가 도는 중(hold, 살아 있음)
    Running,
    /// 자리가 없어 기다림(wait, 살아 있음) — 차례가 있다
    Waiting,
    /// 죽은 엔진의 자리(15분 넘게 다시 적지 않음, 또는 시각이 앞서 있음)
    Dead,
}

impl SlotRowState {
    /// 원본 `rawValue`
    pub fn as_str(self) -> &'static str {
        match self {
            SlotRowState::Running => "running",
            SlotRowState::Waiting => "waiting",
            SlotRowState::Dead => "dead",
        }
    }
}

/// 표의 한 줄. 원본: `APSlotRow`
#[derive(Debug, Clone, PartialEq)]
pub struct SlotRow {
    pub run: String,
    pub unit: i64,
    pub state: SlotRowState,
    /// 기다림이면 몇 번째(1 부터)
    pub turn: Option<i64>,
    /// 잡은 %p(기다림이면 잡으려는 %p)
    pub pp: f64,
    /// 그 단위가 쓴 %p
    pub used: f64,
    /// 잡은 때(기다림이면 기다리기 시작한 때)
    pub since: f64,
    /// 엔진이 마지막으로 적은 때
    pub beat: f64,
    /// 같은 실행이 잡은 같은 경로 자리(실제 경로)
    pub paths: Vec<String>,
    /// 이 화면이 보는 실행
    pub mine: bool,
}

/// 멈출 선 자리 없이 같은 경로 자리만 쥔 실행(일시정지·이어받음·출발 중 등). 원본: `APClaimRow`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub run: String,
    pub paths: Vec<String>,
}

/// 표. 원본: `APSlotTable`(기본값: 줄 없음·`show` 거짓·`summary` 빈 글)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlotTable {
    pub rows: Vec<SlotRow>,
    pub claims_only: Vec<ClaimRow>,
    pub show: bool,
    pub summary: String,
}

/// (수, 글) 쌍 비교 — Swift 튜플 `<`
fn pair_cmp(a: (f64, &str), b: (f64, &str)) -> Ordering {
    if a.0 < b.0 {
        Ordering::Less
    } else if a.0 == b.0 {
        a.1.cmp(b.1)
    } else {
        Ordering::Greater
    }
}

/// 표 한 번 — `slots`·`claims` 는 기록 저장소의 줄 그대로, `active_runs` 는 보드가 보는 끝나지 않은 실행 수, `viewing` 은 이 화면의 실행. 원본: `apSlotTable`
pub fn slot_table(slots: &[Slot], claims: &[Claim], active_runs: i64, viewing: Option<&str>, now: f64) -> SlotTable {
    let mut paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut sorted_claims: Vec<&Claim> = claims.iter().collect();
    sorted_claims.sort_by(|a, b| (a.run.as_str(), a.path.as_str()).cmp(&(b.run.as_str(), b.path.as_str())));
    for c in sorted_claims {
        let list = paths.entry(c.run.clone()).or_default();
        if !list.contains(&c.path) {
            list.push(c.path.clone());
        }
    }
    let live: Vec<&Slot> = slots.iter().filter(|s| slot_live(s, now)).collect();
    let mut holds: Vec<&Slot> = live.iter().copied().filter(|s| s.kind == SlotKind::Hold).collect();
    holds.sort_by(|a, b| pair_cmp((a.since, &a.run), (b.since, &b.run)));
    let mut waits: Vec<&Slot> = live.iter().copied().filter(|s| s.kind == SlotKind::Wait).collect();
    waits.sort_by(|a, b| pair_cmp((a.since, &a.run), (b.since, &b.run)));
    let mut dead: Vec<&Slot> = slots.iter().filter(|s| !slot_live(s, now)).collect();
    dead.sort_by(|a, b| pair_cmp((a.beat, &a.run), (b.beat, &b.run)));

    let row = |s: &Slot, st: SlotRowState, turn: Option<i64>| SlotRow {
        run: s.run.clone(),
        unit: s.unit,
        state: st,
        turn,
        pp: s.pp,
        used: if st == SlotRowState::Waiting { 0.0 } else { s.used },
        since: s.since,
        beat: s.beat,
        paths: paths.get(&s.run).cloned().unwrap_or_default(),
        mine: viewing == Some(s.run.as_str()),
    };

    let mut out = SlotTable::default();
    out.rows.extend(holds.iter().map(|s| row(s, SlotRowState::Running, None)));
    out.rows.extend(waits.iter().enumerate().map(|(i, s)| row(s, SlotRowState::Waiting, Some(i as i64 + 1))));
    out.rows.extend(dead.iter().map(|s| row(s, SlotRowState::Dead, None)));
    let in_slots: BTreeSet<&str> = slots.iter().map(|s| s.run.as_str()).collect();
    out.claims_only = paths
        .iter()
        .filter(|(run, _)| !in_slots.contains(run.as_str()))
        .map(|(run, list)| ClaimRow { run: run.clone(), paths: list.clone() })
        .collect();
    out.show = active_runs >= 2 || !waits.is_empty() || !dead.is_empty();
    let mut parts: Vec<String> = Vec::new();
    if !holds.is_empty() {
        parts.push(format!("도는 중 {}", holds.len()));
    }
    if !waits.is_empty() {
        parts.push(format!("기다림 {}", waits.len()));
    }
    if !dead.is_empty() {
        parts.push(format!("죽은 엔진의 자리 {}", dead.len()));
    }
    let left: f64 = holds.iter().fold(0.0, |acc, s| acc + slot_left(s));
    if !holds.is_empty() {
        parts.push(format!("잡아 두고 아직 쓰지 않은 몫 {}%p", pp_text(left)));
    }
    out.summary = if parts.is_empty() { "잡은 자리 없음".to_string() } else { parts.join(" · ") };
    out
}

/// 줄의 상태 글자(보드 꼬리표). 원본: `apSlotStateText`
pub fn slot_state_text(r: &SlotRow) -> String {
    match r.state {
        SlotRowState::Running => "도는 중".to_string(),
        SlotRowState::Waiting => format!("기다림{}", r.turn.map(|t| format!(" {t}번째")).unwrap_or_default()),
        SlotRowState::Dead => "죽은 엔진의 자리".to_string(),
    }
}

/// 줄의 %p 글자 — 도는 중은 잡은 %p 와 쓴 %p, 기다림은 잡으려는 %p, 죽은 엔진의 자리는 남은 몫(엔진이 지울 때까지 다른 실행의 판정에 들지 않음). 원본: `apSlotPPText`
pub fn slot_pp_text(r: &SlotRow) -> String {
    match r.state {
        SlotRowState::Running => format!("{}%p 잡음(쓴 {}%p)", pp_text(r.pp), pp_text(r.used)),
        SlotRowState::Waiting => format!("{}%p 기다림", pp_text(r.pp)),
        SlotRowState::Dead => format!("{}%p(쓴 {}%p) · 판정에 들지 않음, 다음 판정이 지움", pp_text(r.pp), pp_text(r.used)),
    }
}
