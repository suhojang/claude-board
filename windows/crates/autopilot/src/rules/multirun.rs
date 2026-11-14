//! 여러 실행 함께 돌리기 규칙. 원본: `Sources/AutopilotRules/Multirun.swift`(288줄).
//! ① 사용률을 어디서 읽는가(`usage_steps`). ② 계정 멈출 선을 실행들이 함께 본다: 새 단위를 시작하기 전에 기록 저장소의 자리 표
//! (`stopline_slots`, 판 3)에서 자리를 잡는다(`take_slot`). 지금 사용률에 다른 실행이 잡아 둔 자리(단위 추정 %p 가운데 아직 쓰지 않은 만큼)를
//! 더해 그 실행의 멈출 선과 견준다. 먼저 기다리던 실행에 차례를 넘긴다. 표 읽기·쓰기는 포트(`SlotBook`: 엔진의 SQLite `BEGIN IMMEDIATE` 한 transaction)로만 한다.
//! ③ 같은 경로에서는 한 번에 한 실행만 돈다: 출발할 때 경로 자리(`path_claims`)를 잡고(`claim_paths`) 끝 정리 뒤에 푼다(`release_claims`).
//! ④ 잠긴 저장소를 다시 하는 간격(`busy_retry_delay`).
//!
//! 선언 대응(원본 순서): `UsageStep`(APUsageStep) · `usage_steps`(apUsageSteps) · `SlotKind`(APSlotKind) · `Slot`(APSlot) ·
//! `SLOT_TTL`(apSlotTTL) · `SLOT_RECHECK`(apSlotRecheck) · `slot_live`(apSlotLive) · `slot_size`(apSlotSize) · `slot_left`(apSlotLeft) ·
//! `pp_text`(apPPText) · `SlotChange`(APSlotChange) · `SlotGate`(APSlotGate) · `slot_decide`(apSlotDecide) · `SlotBook`(APSlotBook) ·
//! `SlotTake`(APSlotTake) · `take_slot`(apTakeSlot) · `beat_slot`(apBeatSlot) · `release_slot`(apReleaseSlot) ·
//! `Claim`(APClaim) · `ClaimRunView`(APClaimRunView) · `ClaimProbe`(APClaimProbe) · `claim_alive`(apClaimAlive) · `path_overlap`(apPathOverlap) ·
//! `claim_keys`(apClaimKeys) · `ClaimBook`·`ClaimChange`(APClaimBook) · `ClaimResult`(APClaimResult) · `claim_paths`(apClaimPaths) ·
//! `release_claims`(apReleaseClaims) · `claim_busy_text`(apClaimBusyText) · `BUSY_RETRIES`(apBusyRetries) · `busy_retry_delay`(apBusyRetryDelay).
//!
//! Swift ↔ Rust 로 달라진 점:
//! - `usage`·`stopline` 은 `serde_json::Map`(Swift `[String: Any]`)이다. 값을 수로 읽는 `stop_num` 은 `rules::stopline` 의 것을 쓴다.
//! - 시간대는 `stopline::Tz`(Swift `TimeZone`). 시계는 `ports::Clock`(Swift `APClock`).
//! - 표를 쓰지 못한 까닭 글(`note`·`ClaimResult::Failed`)의 `\(error)` 는 Swift `NSError` 의 긴 설명이고 여기는 어댑터가 준 글 그대로다.
//! - Swift `Int(…)` 는 NaN·무한대에서 죽고 여기 `as i64` 는 0·끝값으로 포화한다(`pct`·`pp_text`). 사용률이 그런 값이면 글자만 다를 뿐 판정은 같다.
//! - 실행 id 순서(`(since, run) < (since, me)`)는 Swift `String` 비교(유니코드 정규 동등)이고 여기는 UTF-8 바이트 순이다. 실행 id 는 엔진이 만든 ASCII 라 같다.
//! - Swift `update { … }` 는 `(put:, drop:)` 튜플을 돌려주고 던질 수 있다(`throws`). 여기 `ClaimBook::update` 는 `ClaimChange` 구조체를 돌려받고 쓰지 못하면 `Err(까닭 글)` 이다.
//! - Swift `hasPrefix("/")` 는 글자 묶음(Character) 단위라 「/」 바로 뒤에 결합 부호가 오면 거짓이다. 여기는 코드 단위(`starts_with`)다.
//!
//! Windows 판단(경로를 견주는 ③ 쪽):
//! - 경로 키는 `path_key::path_key_windows` 다(Swift `apPathKey` 가 아니다). 대소문자·`\`·`/`·`\\?\` 앞붙이·`.`·`..`·끝 구분자·NTFS 스트림을 한 꼴로 모으므로
//!   `C:\Work\Toy-A` 와 `\\?\c:\work\toy-a\` 는 같은 자리다. `path_overlap` 은 **이미 접은 키 둘**을 받고, 홈(`home`)은 안에서 같은 Windows 키로 접는다
//!   (원본이 안에서 `apPathKey(home)` 를 부르는 자리). 홈이 끝에 `\` 를 달고 있어도 같은 홈이다.
//! - `claim_keys` 의 「절대 경로」는 원본이 `/` 로 시작하는 것뿐이다. 여기는 `/`·`\`(UNC·`\\?\`·`\??\` 포함) 로 시작하거나 드라이브 절대(`C:\`·`C:/`)이면 절대다.
//!   드라이브 상대(`C:foo`)·상대 경로·빈 글자는 뺀다. `/w/x` 같은 드라이브 없는 절대는 현재 드라이브의 경로로 받는다(키는 `/w/x`).
//!   자리 줄의 `path` 는 받은 글자 그대로(끝 `/` 도 그대로)이고 `key` 만 접힌다. 원본이 끝 `/` 하나를 벗기고 접는 일은 `path_key_windows` 가 모든 끝 구분자를 벗기는 것으로 대신한다.
//! - `path_overlap` 의 「안쪽」은 `b + "/"` 로 시작하는지로 본다. 키가 이미 `/` 로 끝나는 뿌리(`/`·UNC 뿌리 `//`)는 `/` 를 더 붙이지 않는다(원본은 `/` 만 그렇게 다룬다).
//!   드라이브 뿌리(`c:`)는 `c:/…` 아래가 모두 안쪽이다. 서로 다른 드라이브는 겹치지 않는다.
//! - `path_key_windows` 가 못 하는 것(8.3 짧은 이름·정션·`subst`)은 어댑터가 실제 경로로 먼저 풀어 `paths` 에 넘긴다 — `path_key` 머리 설명이 소유한다.
//! - `claim_paths` 가 보는 `Claim.key` 는 이 엔진의 `claim_keys` 가 쓴 것이다(저장소에서 읽은 키를 다시 접지 않는다).
//! - `claim_busy_text` 의 `~/` 줄임은 표시용이다. 홈 앞부분을 대소문자·`\`·`/` 를 가리지 않고 알아본다. 그때 보이는 나머지 경로는 `\` 를 `/` 로 바꾼 글자다.
//! - 실행 id 는 정확히 같은 글일 때만 같은 실행이다. 윈도의 대소문자 무시 파일 시스템에서 `A` 와 `a` 는 같은 폴더가 되지만 id 는 엔진이 만들므로(소문자 ASCII) 겹치지 않는다.

use crate::ports::Clock;
use crate::rules::control::run_ended;
use crate::rules::path_key::path_key_windows;
use crate::rules::stopline::{stop_num, stopline_wait, StoplineWait, Tz};
use serde_json::{Map, Value};

// ── ① 사용률 읽을 곳 ─────────────────────────────────

/// 사용률을 읽는 걸음 — 엔진이 차례로 보고 처음 값이 나온 것을 쓴다. 원본: `APUsageStep`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageStep {
    /// 시험 빌드만: 장난감 사용량 파일(캐시와 같은 퍼센트 꼴, 두 엔진이 함께 읽음). 쓴 시각을 따지지 않는다
    Toy(String),
    /// 이 엔진의 run.json usage(stream rate_limit_event·앞서 읽은 값)가 15분 안이면
    Stream,
    /// 보드·상태줄이 함께 쓰는 사용량 캐시가 15분 안에 쓰였으면(읽기만)
    Cache,
    /// 키체인 토큰으로 사용량 API(받으면 캐시에 씀) — 설치판만
    Api,
}

/// 설치판(`dev` false)은 지금 그대로(stream → 캐시 → API)이고 장난감 파일 덮어쓰기를 듣지 않는다.
/// 시험 빌드는 사용량 API 로 떨어지지 않는다: 장난감 파일이 있으면 그것 → stream, 없으면 stream → 캐시. 원본: `apUsageSteps`
pub fn usage_steps(dev: bool, toy_path: Option<&str>) -> Vec<UsageStep> {
    if !dev {
        return vec![UsageStep::Stream, UsageStep::Cache, UsageStep::Api];
    }
    match toy_path {
        Some(p) if !p.is_empty() => vec![UsageStep::Toy(p.to_string()), UsageStep::Stream],
        _ => vec![UsageStep::Stream, UsageStep::Cache],
    }
}

// ── ② 멈출 선 자리 ───────────────────────────────────

/// 자리를 잡고 단위가 도는 중 · 자리가 없어 기다리는 중(차례). 원본: `APSlotKind`(rawValue `hold`·`wait`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Hold,
    Wait,
}

impl SlotKind {
    /// 원본 `rawValue` — 자리 표 `kind` 칸의 글
    pub fn as_str(self) -> &'static str {
        match self {
            SlotKind::Hold => "hold",
            SlotKind::Wait => "wait",
        }
    }
}

/// 자리 표 한 줄(실행마다 하나) — `pp`·`used` 는 주간 %p. 원본: `APSlot`
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub run: String,
    pub project: Option<String>,
    pub unit: i64,
    pub kind: SlotKind,
    /// 잡은 자리(단위 추정 %p, `slot_size`)
    pub pp: f64,
    /// 그 단위가 지금까지 쓴 주간 %p(엔진이 1분마다 적음)
    pub used: f64,
    pub pid: Option<i32>,
    /// 잡은 때(기다림이면 기다리기 시작한 때 — 차례)
    pub since: f64,
    /// 엔진이 마지막으로 적은 때. `SLOT_TTL` 이 지나면 죽은 엔진의 자리로 보고 푼다
    pub beat: f64,
}

/// 엔진이 죽어 남은 자리가 풀리는 때(초) — 도는 엔진은 1분마다 자리를 다시 적는다. 사용률을 믿는 15분과 같게. 원본: `apSlotTTL`
pub const SLOT_TTL: f64 = 900.0;
/// 자리 때문에 기다릴 때 다시 보는 간격(초). 원본: `apSlotRecheck`
pub const SLOT_RECHECK: f64 = 60.0;

/// 원본: `apSlotLive`
pub fn slot_live(s: &Slot, now: f64) -> bool {
    now - s.beat < SLOT_TTL && s.beat <= now + 60.0
}

/// 잡을 자리 — 단위 추정 %p 가 있으면 그것(단위 상한을 넘지 않게), 없으면 단위 주간 %p 상한. 원본: `apSlotSize`
pub fn slot_size(est_pp: Option<f64>, cap_pp: f64) -> f64 {
    match est_pp {
        Some(e) if e > 0.0 => e.min(cap_pp),
        _ => cap_pp,
    }
}

/// 아직 쓰지 않은 자리 — 지금 사용률에 이미 든 만큼(`used`)은 빼고 더한다. 원본: `apSlotLeft`
pub fn slot_left(s: &Slot) -> f64 {
    (s.pp - s.used).max(0.0)
}

/// %p 글자 — 정수면 "5", 아니면 소수 한 자리 "2.5". 원본: `apPPText`
pub fn pp_text(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r == r.round() {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

/// 자리 표 바꿈 — 넣거나 바꿀 줄(실행마다 하나)과 지울 실행. 원본: `APSlotChange`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlotChange {
    pub put: Vec<Slot>,
    pub drop: Vec<String>,
}

/// 자리 표 판정 한 번 — 기다리면 그 글과 다시 볼 때, 바꿈은 같은 transaction 에서 쓴다. 원본: `APSlotGate`
#[derive(Debug, Clone, PartialEq)]
pub struct SlotGate {
    pub wait: Option<StoplineWait>,
    pub change: SlotChange,
    /// 견준 다른 실행의 자리 수(hold)
    pub others: i64,
    /// 그 자리의 아직 쓰지 않은 %p 합
    pub held: f64,
    /// 다른 실행의 자리·차례 때문에 기다림(1분마다 다시 봄)
    pub by_others: bool,
}

/// 새 단위를 시작해도 되는가(자리 표를 함께 봄). 원본: `apSlotDecide`
/// 1) 계정 사용률만으로 멈출 선에 닿거나 모르면 지금처럼(`stopline_wait`) 기다리고 내 기다림 줄도 지운다(모두 초기화까지 기다림).
/// 2) 지금 주간 사용률 + 다른 실행이 잡아 둔 자리(쓰지 않은 만큼)가 내 멈출 선에 닿으면 기다리고 기다림 줄(차례)을 남긴다.
/// 3) 나보다 먼저 기다리던 실행이 있고 그 자리까지 더하면 닿으면 차례를 넘기고 기다린다(한 실행이 단위를 이어 잡아 다른 실행이 굶지 않게).
/// 4) 아니면 내 자리(hold)를 잡는다. 죽은 엔진의 자리(`SLOT_TTL` 지남)는 늘 지운다. 다른 실행이 없으면 1) 과 같다.
#[allow(clippy::too_many_arguments)]
pub fn slot_decide(
    me: &str,
    project: Option<&str>,
    unit: i64,
    pp: f64,
    pid: Option<i32>,
    usage: Option<&Map<String, Value>>,
    stopline: &Map<String, Value>,
    slots: &[Slot],
    now: f64,
    tz: Tz,
) -> SlotGate {
    let t = now;
    let mut change = SlotChange {
        put: Vec::new(),
        drop: slots.iter().filter(|s| !slot_live(s, t) && s.run != me).map(|s| s.run.clone()).collect(),
    };
    let live: Vec<&Slot> = slots.iter().filter(|s| s.run != me && slot_live(s, t)).collect();
    let holds: Vec<&&Slot> = live.iter().filter(|s| s.kind == SlotKind::Hold).collect();
    let held: f64 = holds.iter().fold(0.0, |a, s| a + slot_left(s));
    let others = holds.len() as i64;
    let mine = slots.iter().find(|s| s.run == me);
    if let Some(w) = stopline_wait(usage, stopline, t, tz) {
        if mine.is_some() {
            change.drop.push(me.to_string());
        }
        return SlotGate { wait: Some(w), change, others, held, by_others: false };
    }
    let l7 = stop_num(stopline.get("seven_day")).unwrap_or(0.80);
    let r7 = stop_num(usage.and_then(|u| u.get("seven_day_resets"))).unwrap_or(0.0);
    let v7 = if r7 > 0.0 && r7 <= t { 0.0 } else { stop_num(usage.and_then(|u| u.get("seven_day"))).unwrap_or(0.0) };
    let since = match mine {
        Some(m) if m.kind == SlotKind::Wait => m.since,
        _ => t,
    };
    let wait_row = Slot {
        run: me.to_string(),
        project: project.map(str::to_string),
        unit,
        kind: SlotKind::Wait,
        pp,
        used: 0.0,
        pid,
        since,
        beat: t,
    };
    let pct = |x: f64| format!("{}%", (x * 100.0).round() as i64);
    if v7 + held / 100.0 >= l7 - 1e-9 {
        change.put.push(wait_row);
        let reason = format!(
            "계정 주간 사용률 {} 에 다른 실행 {}개가 잡아 둔 자리 {}%p 를 더하면 멈출 선 {} 에 닿아, 새 단위를 시작하지 않고 그 단위가 끝나기를 기다립니다(1분마다 다시 봅니다).",
            pct(v7),
            others,
            pp_text(held),
            pct(l7)
        );
        return SlotGate { wait: Some(StoplineWait { until: t + SLOT_RECHECK, reason }), change, others, held, by_others: true };
    }
    // (since, run) < (since, me) — 튜플 비교. NaN 이면 둘 다 거짓
    let ahead: Vec<&&Slot> =
        live.iter().filter(|s| s.kind == SlotKind::Wait && (s.since < since || (s.since == since && s.run.as_str() < me))).collect();
    let ahead_pp: f64 = ahead.iter().fold(0.0, |a, s| a + s.pp);
    if !ahead.is_empty() && v7 + (held + ahead_pp) / 100.0 >= l7 - 1e-9 {
        change.put.push(wait_row);
        let reason = format!(
            "먼저 기다리던 실행 {}개(자리 {}%p)에 차례를 넘겨, 새 단위를 시작하지 않고 기다립니다(1분마다 다시 봅니다).",
            ahead.len(),
            pp_text(ahead_pp)
        );
        return SlotGate { wait: Some(StoplineWait { until: t + SLOT_RECHECK, reason }), change, others, held, by_others: true };
    }
    change.put.push(Slot { kind: SlotKind::Hold, since: t, ..wait_row });
    SlotGate { wait: None, change, others, held, by_others: false }
}

/// 자리 표 — 다른 엔진과 한 번에 하나(엔진: 기록 저장소의 `BEGIN IMMEDIATE` 한 transaction). `body` 가 지금 줄을 보고 돌려준 바꿈을
/// 같은 잠금 안에서 쓴다. 쓰지 못하면 Err(까닭 글). 원본: `APSlotBook`
pub trait SlotBook: Send + Sync {
    fn update(&self, body: &mut dyn FnMut(&[Slot]) -> SlotChange) -> Result<(), String>;
}

/// 자리 잡기 결과 — 시작(`wait` 없음)·기다림, 자리 표를 쓰지 못했으면 사용률만으로 정한 결과와 까닭(`note`). 원본: `APSlotTake`
#[derive(Debug, Clone, PartialEq)]
pub struct SlotTake {
    pub wait: Option<StoplineWait>,
    pub others: i64,
    pub held: f64,
    pub note: Option<String>,
    pub by_others: bool,
}

/// 유스케이스: 새 단위 앞에서 자리를 잡는다. 표를 쓰지 못하면(저장소 잠김·깨짐) 6단계 전처럼 사용률만으로 정하고 까닭을 남긴다
/// (실행은 멈추지 않음). 원본: `apTakeSlot`
#[allow(clippy::too_many_arguments)]
pub fn take_slot(
    me: &str,
    project: Option<&str>,
    unit: i64,
    pp: f64,
    pid: Option<i32>,
    usage: Option<&Map<String, Value>>,
    stopline: &Map<String, Value>,
    book: &dyn SlotBook,
    clock: &dyn Clock,
    tz: Tz,
) -> SlotTake {
    let t = clock.now();
    let mut gate: Option<SlotGate> = None;
    let r = book.update(&mut |slots| {
        let g = slot_decide(me, project, unit, pp, pid, usage, stopline, slots, t, tz);
        let change = g.change.clone();
        gate = Some(g);
        change
    });
    if let Err(e) = r {
        return SlotTake {
            wait: stopline_wait(usage, stopline, t, tz),
            others: 0,
            held: 0.0,
            note: Some(format!("멈출 선 자리 표를 쓰지 못해({e}) 다른 실행의 자리 없이 사용률만으로 정했습니다.")),
            by_others: false,
        };
    }
    match gate {
        Some(g) => SlotTake { wait: g.wait, others: g.others, held: g.held, note: None, by_others: g.by_others },
        None => SlotTake { wait: stopline_wait(usage, stopline, t, tz), others: 0, held: 0.0, note: None, by_others: false },
    }
}

/// 유스케이스: 도는 단위의 자리를 다시 적는다(1분마다, 다시 뜬 엔진이 이어 가는 단위도 이것으로 자리를 되찾는다).
/// 죽은 엔진의 자리도 함께 지운다. 원본: `apBeatSlot`
#[allow(clippy::too_many_arguments)]
pub fn beat_slot(
    me: &str,
    project: Option<&str>,
    unit: i64,
    pp: f64,
    used: f64,
    pid: Option<i32>,
    book: &dyn SlotBook,
    clock: &dyn Clock,
) -> Result<(), String> {
    let t = clock.now();
    book.update(&mut |slots| {
        let since = slots.iter().find(|s| s.run == me && s.kind == SlotKind::Hold).map_or(t, |s| s.since);
        SlotChange {
            drop: slots.iter().filter(|s| !slot_live(s, t) && s.run != me).map(|s| s.run.clone()).collect(),
            put: vec![Slot {
                run: me.to_string(),
                project: project.map(str::to_string),
                unit,
                kind: SlotKind::Hold,
                pp,
                used,
                pid,
                since,
                beat: t,
            }],
        }
    })
}

/// 유스케이스: 자리를 푼다(단위 끝·일시정지·실행 끝). 원본: `apReleaseSlot`
pub fn release_slot(me: &str, book: &dyn SlotBook) -> Result<(), String> {
    book.update(&mut |slots| {
        if slots.iter().any(|s| s.run == me) {
            SlotChange { put: Vec::new(), drop: vec![me.to_string()] }
        } else {
            SlotChange::default()
        }
    })
}

// ── ③ 같은 경로 자리 ─────────────────────────────────

/// 경로 자리 한 줄(저장소 경로마다) — `key` 는 접은 경로(`path_key_windows`), `path` 는 실제 경로. 원본: `APClaim`
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub key: String,
    pub path: String,
    pub run: String,
    pub run_dir: String,
    pub label: String,
    /// 자리를 잡은 출발 명령(start)의 pid — 출발이 끝나기 전에 그 실행이 살아 있는지 보려고
    pub pid: Option<i32>,
    pub claimed: f64,
}

/// 자리를 잡은 실행의 지금 — 포트가 실행 폴더의 run.json·엔진 pid·작업 스케줄러·출발 명령 pid 를 보고 채운다. 원본: `APClaimRunView`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRunView {
    /// 실행 폴더의 run.json 을 읽었나
    pub exists: bool,
    /// run.json state(출발 명령이 쓰기 전에는 None)
    pub state: Option<String>,
    /// 끝 정리를 마쳤나
    pub wrapped: bool,
    /// run.json engine_pid 가 살아 있고 그 실행의 엔진인가
    pub engine_alive: bool,
    /// 그 실행의 작업(macOS LaunchAgent 자리)이 실려 있나(죽어도 다시 띄움)
    pub job_loaded: bool,
    /// 자리를 잡은 출발 명령이 아직 도는가
    pub starter_alive: bool,
}

/// 원본: `APClaimProbe`
pub trait ClaimProbe: Send + Sync {
    fn view(&self, c: &Claim) -> ClaimRunView;
}

/// 그 자리를 아직 쓰는가. 실행 폴더가 없으면 아님. 출발 명령이 아직 상태를 쓰지 않았으면 그 명령이 도는 동안만.
/// 끝났고 정리도 마쳤으면 아님. 끝났지만 정리 중(가지 되돌리기)이거나 도는 상태면 엔진이 살아 있거나 작업이 실려 있을 때만
/// (엔진이 죽어 작업도 없으면 남은 자리). 원본: `apClaimAlive`
pub fn claim_alive(v: &ClaimRunView) -> bool {
    if !v.exists {
        return false;
    }
    let s = match v.state.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => return v.starter_alive,
    };
    if run_ended(s) && v.wrapped {
        return false;
    }
    v.engine_alive || v.job_loaded || (s == "starting" && v.starter_alive)
}

/// 두 접은 경로(`path_key_windows` 키)가 겹치는가 — 같거나 한쪽이 다른 쪽 안(조각 단위). 홈 폴더(접은 꼴)는 같을 때만(홈을 품은 자리로 보지 않음).
/// 홈은 안에서 같은 Windows 키로 접는다. 원본: `apPathOverlap`
pub fn path_overlap(a: &str, b: &str, home: &str) -> bool {
    if a == b {
        return true;
    }
    let h = path_key_windows(home);
    if a == h || b == h {
        return false;
    }
    inside(a, b) || inside(b, a)
}

// `x` 가 `dir` 안(조각 단위)인가. 키가 이미 「/」 로 끝나는 뿌리면 「/」 를 더 붙이지 않는다
fn inside(x: &str, dir: &str) -> bool {
    if dir.ends_with('/') {
        x.starts_with(dir)
    } else {
        x.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
    }
}

// 절대 경로인가 — `/`·`\` 로 시작하거나 드라이브 절대(`C:\`·`C:/`). 드라이브 상대(`C:foo`)는 아님
fn is_absolute(p: &str) -> bool {
    if p.starts_with('/') || p.starts_with('\\') {
        return true;
    }
    let mut cs = p.chars();
    matches!((cs.next(), cs.next(), cs.next()), (Some(d), Some(':'), Some('/' | '\\')) if d.is_ascii_alphabetic())
}

/// 실제 경로 목록 → 자리 줄 `(key, path)`(접은 꼴로 겹친 것은 먼저 온 하나로). 빈 글자·상대 경로는 뺀다. 원본: `apClaimKeys`
pub fn claim_keys(paths: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for p in paths.iter().filter(|p| is_absolute(p)) {
        let k = path_key_windows(p);
        if !out.iter().any(|(ok, _)| *ok == k) {
            out.push((k, p.clone()));
        }
    }
    out
}

/// 경로 자리 표 바꿈 — 넣을 줄과 지울 실행 id(그 실행의 줄 모두). 원본: `update` 본문이 돌려주는 `(put:, drop:)`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClaimChange {
    pub put: Vec<Claim>,
    pub drop: Vec<String>,
}

/// 경로 자리 표 — 다른 엔진과 한 번에 하나(엔진: 기록 저장소의 `BEGIN IMMEDIATE` 한 transaction). 쓰지 못하면(같은 키 충돌 포함) Err(까닭 글). 원본: `APClaimBook`
pub trait ClaimBook: Send + Sync {
    fn update(&self, body: &mut dyn FnMut(&[Claim]) -> ClaimChange) -> Result<(), String>;
}

/// 경로 자리 잡기 결과. 원본: `APClaimResult`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    /// 잡았다. 치운 남은 자리의 실행 id
    Ok { cleared: Vec<String> },
    /// 그 실행이 겹치는 자리에서 돈다(그 실행의 경로, 내 경로)
    Busy { run: String, path: String, mine: String },
    /// 표를 쓰지 못함(출발하지 않는다 — 닫힌 쪽)
    Failed(String),
}

/// 유스케이스: 출발할 때 경로 자리를 잡는다(한 transaction: 겹치는 다른 실행의 자리를 보고, 살아 있으면 거부, 남은 자리면 그 실행의 줄을 지우고 잡는다).
/// 두 출발이 거의 같은 때 와도 잠금이 하나씩 지나가게 해, 뒤에 온 쪽은 앞 쪽의 자리를 보고 거부한다(앞 쪽 출발 명령이 아직 돌면 살아 있음).
/// 원본: `apClaimPaths`
#[allow(clippy::too_many_arguments)]
pub fn claim_paths(
    run: &str,
    run_dir: &str,
    label: &str,
    pid: Option<i32>,
    paths: &[String],
    home: &str,
    book: &dyn ClaimBook,
    probe: &dyn ClaimProbe,
    clock: &dyn Clock,
) -> ClaimResult {
    let t = clock.now();
    let mine = claim_keys(paths);
    let mut result = ClaimResult::Ok { cleared: Vec::new() };
    let r = book.update(&mut |claims| {
        let mut dead: Vec<String> = Vec::new();
        let mut alive_cache: Vec<(String, bool)> = Vec::new();
        for c in claims.iter().filter(|c| c.run != run) {
            let Some(m) = mine.iter().find(|m| path_overlap(&m.0, &c.key, home)) else { continue };
            let alive = match alive_cache.iter().find(|(r, _)| *r == c.run) {
                Some((_, a)) => *a,
                None => {
                    let a = claim_alive(&probe.view(c));
                    alive_cache.push((c.run.clone(), a));
                    a
                }
            };
            if alive {
                result = ClaimResult::Busy { run: c.run.clone(), path: c.path.clone(), mine: m.1.clone() };
                return ClaimChange::default();
            }
            if !dead.contains(&c.run) {
                dead.push(c.run.clone());
            }
        }
        result = ClaimResult::Ok { cleared: dead.clone() };
        let put = mine
            .iter()
            .map(|(key, path)| Claim {
                key: key.clone(),
                path: path.clone(),
                run: run.to_string(),
                run_dir: run_dir.to_string(),
                label: label.to_string(),
                pid,
                claimed: t,
            })
            .collect();
        let mut drop = dead;
        if claims.iter().any(|c| c.run == run) {
            drop.push(run.to_string());
        }
        ClaimChange { put, drop }
    });
    match r {
        Ok(()) => result,
        Err(e) => ClaimResult::Failed(e),
    }
}

/// 유스케이스: 경로 자리를 푼다(끝 정리를 마친 뒤, 출발하지 못했을 때). 원본: `apReleaseClaims`
pub fn release_claims(run: &str, book: &dyn ClaimBook) -> Result<(), String> {
    book.update(&mut |claims| {
        if claims.iter().any(|c| c.run == run) {
            ClaimChange { put: Vec::new(), drop: vec![run.to_string()] }
        } else {
            ClaimChange::default()
        }
    })
}

/// 출발을 거부할 때의 글(보드 출발 준비의 잠금 글과 같은 말). 거부가 아니면 None. 원본: `apClaimBusyText`
pub fn claim_busy_text(r: &ClaimResult, home: &str) -> Option<String> {
    let ClaimResult::Busy { run: other, path, .. } = r else { return None };
    let shown = under_home(path, home).map_or_else(|| path.clone(), |rest| format!("~/{rest}"));
    Some(format!("이 경로에서 자율주행이 돌고 있습니다({other}, {shown}). 그 실행이 끝난 뒤에 출발합니다"))
}

// `path` 가 `home` 아래면 홈 뒤의 나머지(`\` 는 `/` 로). 홈 앞부분은 대소문자·`\`·`/` 를 가리지 않고 알아본다(표시용)
fn under_home(path: &str, home: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    let h = home.replace('\\', "/");
    let h = h.trim_end_matches('/');
    if h.is_empty() {
        return None;
    }
    let mut pc = p.chars();
    for hc in h.chars() {
        let c = pc.next()?;
        if !c.to_lowercase().eq(hc.to_lowercase()) {
            return None;
        }
    }
    if pc.next()? != '/' {
        return None;
    }
    Some(pc.collect())
}

// ── ④ 여러 엔진이 같은 기록 저장소를 쓸 때 ───────────

/// 잠겨 있어(SQLITE_BUSY·LOCKED) 실패한 걸음을 다시 하는 최대 횟수. 원본: `apBusyRetries`
pub const BUSY_RETRIES: i64 = 8;

/// 잠겨 있어(SQLITE_BUSY·LOCKED) 실패한 걸음을 다시 할 때 쉬는 초 — `attempt` 는 1 부터. 여덟 번까지(0.1·0.2·0.4·0.8 뒤로는 1초), 그 뒤는 None(포기).
/// 엔진은 busy_timeout 5초로 한 번 더 기다리므로 다른 엔진이 오래 잡아도 몇십 초까지 다시 한다. 열기의 journal_mode·판 읽기와 쓰기 transaction 이 함께 쓴다.
/// 원본: `apBusyRetryDelay`
pub fn busy_retry_delay(attempt: i64) -> Option<f64> {
    if !(1..=BUSY_RETRIES).contains(&attempt) {
        return None;
    }
    Some((0.1 * f64::from(1u32 << (attempt - 1).min(4))).min(1.0))
}
