//! 파일 → 판단 상태. run.json·queue.json·pinned/frozen.json·(도는 단위의) engine.json 을 읽은 값에서 `EngineState` 를 세운다.
//! 순수 함수다 — 파일을 읽지 않는다. 읽는 일은 `engine::run` 의 루프(4.2b)가 한다.
//! 함께 starts.log 한 줄, 비정상 재시작 세기, 세션 id, 고정 설정 해시처럼 파일 값만 가지고 만드는 작은 글도 둔다.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::engine::decide::{
    Caps, Current, EngineState, Next, RunState, UnitPhase, UnitPlan, UnitStatus, WeeklyMeter, MSG_ERROR, MSG_LIMIT, MSG_PAUSED,
    MSG_RESTART,
};
use crate::model::{to_pretty_json, FrozenFile, QueueFile, RunFile, UnitProgressFile};
use crate::rules::stopline::Tz;

/// 비정상 재시작을 세는 창(초). 원본 `main` 의 `t - 600`
const ABNORMAL_WINDOW_SEC: f64 = 600.0;

/// 파일에서 세운 엔진 시작 상태
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub state: EngineState,
    /// 도는 단위의 주간 %p 셈(도는 단위가 없으면 기본값)
    pub weekly: WeeklyMeter,
    /// 도는 단위 engine.json 의 `session_id`
    pub session_id: Option<String>,
}

/// 숫자, 또는 숫자로 읽히는 글자(원본 `num`)
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// frozen 의 units(없으면 queue)로 UnitPlan 을, queue 의 status 로 statuses 를, status running 단위의 engine.json 으로 Current 를 세운다.
/// run.state 가 비었거나 없으면 Starting. restart_asked = run.json extra 에 `restart` 객체가 있음, restart_unit = 그 `unit`.
/// 오류: 단위 0개 · 고정 설정 단위 수와 queue 길이가 다름 · running 단위가 둘 이상 · run.state 를 모름.
/// 원본: `main` 의 시작 상태 읽기와 `syncRun` 의 `defs`(길이가 다르면 macOS 는 queue 로 돌아가지만 Windows 는 멈춘다)
pub fn load_state(
    run: &RunFile,
    queue: &QueueFile,
    frozen: &FrozenFile,
    running: Option<&UnitProgressFile>,
    tz: Tz,
) -> Result<Loaded, String> {
    let defs: Vec<&crate::model::Ordered<crate::model::QueueUnit>> = match frozen.units.get() {
        Some(f) if f.len() != queue.len() => {
            return Err(format!("고정 설정의 단위 {}개와 대기열의 단위 {}개가 다릅니다.", f.len(), queue.len()));
        }
        Some(f) => f.iter().collect(),
        None => queue.iter().collect(),
    };
    if defs.is_empty() {
        return Err("단위가 하나도 없습니다.".to_string());
    }

    let run_caps = frozen.caps.get().cloned().unwrap_or(Value::Null);
    let units: Vec<UnitPlan> = defs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let non_empty = |k: &str| d.extra.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
            UnitPlan {
                title: d.title.get().cloned().unwrap_or_else(|| format!("단위 {}", i + 1)),
                check: d.check.get().cloned().unwrap_or_default(),
                has_action: non_empty("action"),
                has_precheck: non_empty("precheck"),
                caps: caps_of(&serde_json::to_value(d).unwrap_or(Value::Null), &run_caps),
            }
        })
        .collect();
    let statuses: Vec<UnitStatus> =
        queue.iter().map(|q| q.status.get().and_then(|s| UnitStatus::parse(s)).unwrap_or(UnitStatus::Pending)).collect();
    let running_ns: Vec<usize> = statuses.iter().enumerate().filter(|(_, s)| **s == UnitStatus::Running).map(|(i, _)| i + 1).collect();
    if running_ns.len() > 1 {
        return Err(format!("도는 단위가 둘 이상입니다(단위 {}).", running_ns.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")));
    }

    let setting = |k: &str| frozen.extra.get(k).and_then(num);
    let stopline: Map<String, Value> = frozen
        .stopline
        .get()
        .or_else(|| run.stopline.get())
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let model = frozen.model.get().or_else(|| run.model.get()).cloned().unwrap_or_default();
    let mut state = EngineState::new(
        &model,
        units,
        setting("check_retries").map_or(2, |x| x as i64),
        setting("precheck_retry_min").unwrap_or(5.0),
        stopline,
        tz,
    );

    state.run = match run.state.get().map(String::as_str) {
        None | Some("") => RunState::Starting,
        Some(s) => RunState::parse(s).ok_or_else(|| format!("run.json 의 state 를 알 수 없습니다: {s}"))?,
    };
    state.reason = run.reason.get().cloned().unwrap_or_default();
    state.statuses = statuses;
    let restart = run.extra.get("restart").and_then(Value::as_object);
    state.restart_asked = restart.is_some();
    state.restart_unit = restart.and_then(|r| r.get("unit")).and_then(num).map(|x| x as usize);

    let mut weekly = WeeklyMeter::default();
    let mut session_id = None;
    if let Some(&n) = running_ns.first() {
        state.current = Some(current_of(n, &state.units[n - 1], running));
        if let Some(e) = running {
            weekly = WeeklyMeter {
                base: e.weekly_base.get().and_then(|x| x.as_f64()),
                last: e.weekly_last.get().and_then(|x| x.as_f64()),
                pp_done: e.weekly_pp_done.get().and_then(|x| x.as_f64()).unwrap_or(0.0),
                pp: e.weekly_pp.get().and_then(|x| x.as_f64()).unwrap_or(0.0),
            };
            session_id = e.session_id.get().cloned();
        }
    }
    Ok(Loaded { state, weekly, session_id })
}

/// 도는 단위의 engine.json → Current. engine.json 이 아직 없으면(단위를 연 직후 끊김) 처음 값에 `Restart` 로 잇는다
fn current_of(n: usize, plan: &UnitPlan, e: Option<&UnitProgressFile>) -> Current {
    let Some(e) = e else {
        let phase = if plan.has_action { UnitPhase::Action } else { UnitPhase::Model };
        return Current { n, phase, tries: 0, errors: 0, limits: 0, resume: false, next: Next::Restart, wait_until: None };
    };
    let int = |x: Option<f64>| x.map_or(0, |v| v as i64);
    Current {
        n,
        phase: e.phase.get().and_then(|p| UnitPhase::parse(p)).unwrap_or(UnitPhase::Model),
        tries: int(e.r#try.get().and_then(|x| x.as_f64())),
        errors: int(e.errors.get().and_then(|x| x.as_f64())),
        limits: int(e.limits.get().and_then(num)),
        resume: e.session_started.get().copied().unwrap_or(false),
        next: next_from(e.next_message.get().map(String::as_str), e.first_message.get().map(String::as_str)),
        wait_until: e.wait_until.get().and_then(|x| x.as_f64()),
    }
}

/// engine.json next_message 글 → Next: first_message 와 같으면 First, MSG_* 와 같으면 그 variant, 빈 글·없음이면 First, 그 밖 Text
pub fn next_from(next_message: Option<&str>, first_message: Option<&str>) -> Next {
    let Some(t) = next_message else { return Next::First };
    if Some(t) == first_message {
        return Next::First;
    }
    match t {
        MSG_PAUSED => Next::Paused,
        MSG_LIMIT => Next::Limit,
        MSG_RESTART => Next::Restart,
        MSG_ERROR => Next::Error,
        "" => Next::First,
        _ => Next::Text(t.to_string()),
    }
}

/// Next → engine.json next_message 글(First 면 first_message)
pub fn next_text(next: &Next, first_message: &str) -> String {
    next.text().unwrap_or(first_message).to_string()
}

/// UnitPlan.caps: 단위 caps → frozen caps → Caps::default. `unit` 은 단위 정의 객체(그 안의 `caps` 객체를 본다),
/// `run_caps` 는 frozen `caps` 객체(없으면 `Null`). 칸 이름 minutes·usd·weekly_pp. 원본: `caps(_ n:)`
pub fn caps_of(unit: &Value, run_caps: &Value) -> Caps {
    let d = Caps::default();
    let pick = |k: &str, fallback: f64| {
        unit.get("caps")
            .and_then(|c| c.get(k))
            .and_then(num)
            .or_else(|| run_caps.get(k).and_then(num))
            .unwrap_or(fallback)
    };
    Caps { minutes: pick("minutes", d.minutes), usd: pick("usd", d.usd), weekly_pp: pick("weekly_pp", d.weekly_pp) }
}

/// `sha256(to_pretty_json(v))` 소문자 16진 64자. 파일 바이트가 아니라 정규화한 글의 해시라 줄 띄움·줄바꿈 차이에 흔들리지 않는다
pub fn frozen_digest(v: &Value) -> String {
    let text = to_pretty_json(v).expect("JSON 값은 늘 글로 쓸 수 있다");
    hex(&Sha256::digest(text.as_bytes()))
}

/// starts.log 한 줄: `{유닉스 초 정수}\t{YYYY-MM-DD HH:MM:SS 지역}\t{normal|abnormal}\tpid={pid}\tprev={직전 상태}\texe={실행 파일}`
pub fn starts_line(now: f64, local: &str, abnormal: bool, pid: u32, prev: &str, exe: &str) -> String {
    let kind = if abnormal { "abnormal" } else { "normal" };
    format!("{}\t{local}\t{kind}\tpid={pid}\tprev={prev}\texe={exe}", now as i64)
}

/// starts.log 글에서 `now - 600` 이후의 abnormal 줄 수(이번 줄 포함해 세려면 먼저 덧붙인 글을 넘긴다)
pub fn abnormal_recent(log: &str, now: f64) -> u32 {
    let count = log
        .lines()
        .filter(|line| {
            // 원본 `split(separator:)` 는 빈 칸을 건너뛴다
            let f: Vec<&str> = line.split('\t').filter(|x| !x.is_empty()).collect();
            f.len() > 2 && f[2] == "abnormal" && f[0].parse::<f64>().is_ok_and(|t| t >= now - ABNORMAL_WINDOW_SEC)
        })
        .count();
    count as u32
}

/// abnormal = 직전 상태가 빈 글·starting 이 아님
pub fn is_abnormal(prev: &str) -> bool {
    !(prev.is_empty() || prev == "starting")
}

/// 세션 id(UUID v4 꼴): seed 의 sha256 앞 16바이트에 버전 4·변형 비트를 넣어 8-4-4-4-12.
/// 실제 seed 는 실행 id·단위 번호·시각 나노초·pid
pub fn session_uuid(seed: &[u8]) -> String {
    let mut b = [0u8; 16];
    for (slot, x) in b.iter_mut().zip(Sha256::digest(seed).iter()) {
        *slot = *x;
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}
