//! 엔진 상태 기계(순수). 원본 `main.swift` 의 판단(main·runUnit·turn·pause·stop·게이트·다시 출발)만 옮겼다.
//! `decide(state, event)` 는 state 를 바꾸고 해야 할 일을 `Action` 목록으로 돌려준다.
//! 파일·프로세스·시계는 부르지 않는다. 그 일은 `Action` 을 받은 `engine::run` 이 한다.

use serde_json::{Map, Value};

use crate::rules::checks;
use crate::rules::control::{self, ControlMode, ControlStep, RestartPlan};
use crate::rules::stopline::{self, Tz};
use crate::stream::StreamState;

/// 모델에게 넘기는 이어 가는 말(원본 main.swift 634~637 글자 그대로)
pub const MSG_PAUSED: &str = "일시정지됐던 작업을 이어서 하세요. 이미 끝낸 일은 다시 하지 말고, 멈춘 자리의 상태(파일, git)를 확인한 뒤 계속하세요.";
pub const MSG_LIMIT: &str = "사용량 한도로 끊겼던 작업을 이어서 하세요. 이미 끝낸 일은 다시 하지 말고, 멈춘 자리의 상태(파일, git)를 확인한 뒤 계속하세요.";
pub const MSG_RESTART: &str = "엔진이 다시 떠서 이어 갑니다. 하던 일이 중간에 끊겼을 수 있으니, 이미 끝낸 일은 다시 하지 말고 멈춘 자리의 상태(파일, git, 돌던 명령)를 확인한 뒤 이어서 하세요.";
pub const MSG_ERROR: &str = "오류로 끊겼던 작업을 이어서 하세요. 이미 끝낸 일은 다시 하지 말고, 멈춘 자리의 상태(파일, git)를 확인한 뒤 계속하세요.";
/// claude 오류로 끝난 뒤 다시 열기까지(초)와 그 횟수 상한
pub const ERROR_RETRY_SEC: f64 = 60.0;
pub const ERROR_LIMIT: i64 = 3;
/// 한도 초기화 시각을 모를 때 기다리는 초, 초기화 뒤 더 기다리는 초
pub const LIMIT_UNKNOWN_SEC: f64 = 1800.0;
pub const LIMIT_AFTER_RESET_SEC: f64 = 120.0;

/// run.json `state`. `as_str`/`parse` 글자: starting·running·waiting_limit·waiting_stopline·waiting_precheck·paused·taken_over·stopped·done·error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Starting,
    Running,
    WaitingLimit,
    WaitingStopline,
    WaitingPrecheck,
    Paused,
    TakenOver,
    Stopped,
    Done,
    Error,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Starting => "starting",
            RunState::Running => "running",
            RunState::WaitingLimit => "waiting_limit",
            RunState::WaitingStopline => "waiting_stopline",
            RunState::WaitingPrecheck => "waiting_precheck",
            RunState::Paused => "paused",
            RunState::TakenOver => "taken_over",
            RunState::Stopped => "stopped",
            RunState::Done => "done",
            RunState::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "starting" => RunState::Starting,
            "running" => RunState::Running,
            "waiting_limit" => RunState::WaitingLimit,
            "waiting_stopline" => RunState::WaitingStopline,
            "waiting_precheck" => RunState::WaitingPrecheck,
            "paused" => RunState::Paused,
            "taken_over" => RunState::TakenOver,
            "stopped" => RunState::Stopped,
            "done" => RunState::Done,
            "error" => RunState::Error,
            _ => return None,
        })
    }

    pub fn ended(self) -> bool {
        control::run_ended(self.as_str())
    }
}

/// queue.json·engine.json `status`. 글자: pending·running·done·failed·capped·skipped·stopped.
/// `label()`(원본 unitLabel): 대기·진행 중·통과·미통과·상한에서 멈춤·건너뜀·중단
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitStatus {
    Pending,
    Running,
    Done,
    Failed,
    Capped,
    Skipped,
    Stopped,
}

impl UnitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitStatus::Pending => "pending",
            UnitStatus::Running => "running",
            UnitStatus::Done => "done",
            UnitStatus::Failed => "failed",
            UnitStatus::Capped => "capped",
            UnitStatus::Skipped => "skipped",
            UnitStatus::Stopped => "stopped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => UnitStatus::Pending,
            "running" => UnitStatus::Running,
            "done" => UnitStatus::Done,
            "failed" => UnitStatus::Failed,
            "capped" => UnitStatus::Capped,
            "skipped" => UnitStatus::Skipped,
            "stopped" => UnitStatus::Stopped,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            UnitStatus::Pending => "대기",
            UnitStatus::Running => "진행 중",
            UnitStatus::Done => "통과",
            UnitStatus::Failed => "미통과",
            UnitStatus::Capped => "상한에서 멈춤",
            UnitStatus::Skipped => "건너뜀",
            UnitStatus::Stopped => "중단",
        }
    }
}

/// engine.json `phase`. 글자: action·model·check·limit_wait
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitPhase {
    Action,
    Model,
    Check,
    LimitWait,
}

impl UnitPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitPhase::Action => "action",
            UnitPhase::Model => "model",
            UnitPhase::Check => "check",
            UnitPhase::LimitWait => "limit_wait",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "action" => UnitPhase::Action,
            "model" => UnitPhase::Model,
            "check" => UnitPhase::Check,
            "limit_wait" => UnitPhase::LimitWait,
            _ => return None,
        })
    }
}

/// 상한 종류. 글자 minutes·usd·weekly, `label()`(원본 capLabel): 시간·비용·주간 사용률
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapKind {
    Minutes,
    Usd,
    Weekly,
}

impl CapKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CapKind::Minutes => "minutes",
            CapKind::Usd => "usd",
            CapKind::Weekly => "weekly",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CapKind::Minutes => "시간",
            CapKind::Usd => "비용",
            CapKind::Weekly => "주간 사용률",
        }
    }
}

/// 단위 상한. 기본 120분·100 USD·5%p(설계 3장)
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    pub minutes: f64,
    pub usd: f64,
    pub weekly_pp: f64,
}

impl Default for Caps {
    fn default() -> Self {
        Caps { minutes: 120.0, usd: 100.0, weekly_pp: 5.0 }
    }
}

/// 고정 설정에서 읽은 단위 하나의 계획(바뀌지 않음)
#[derive(Debug, Clone, PartialEq)]
pub struct UnitPlan {
    pub title: String,
    /// 판정 글에 쓰는 이름(판정 명령 글자)
    pub check: String,
    pub has_action: bool,
    pub has_precheck: bool,
    pub caps: Caps,
}

/// 다음에 claude 에 넘길 말
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    First,
    Restart,
    Paused,
    Limit,
    Error,
    Text(String),
}

impl Next {
    /// First 는 None(run.rs 가 engine.json `first_message` 를 쓴다), 나머지는 MSG_* 또는 그 글
    pub fn text(&self) -> Option<&str> {
        match self {
            Next::First => None,
            Next::Restart => Some(MSG_RESTART),
            Next::Paused => Some(MSG_PAUSED),
            Next::Limit => Some(MSG_LIMIT),
            Next::Error => Some(MSG_ERROR),
            Next::Text(t) => Some(t),
        }
    }
}

/// 도는 단위의 판단 상태(engine.json 에 이어 쓰는 칸의 부분)
#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    /// 1부터
    pub n: usize,
    pub phase: UnitPhase,
    /// 판정 실패 뒤 다시 맡긴 횟수(engine.json `try`)
    pub tries: i64,
    /// claude 오류 횟수
    pub errors: i64,
    /// 한도 대기 횟수
    pub limits: i64,
    /// 세션이 만들어졌다(engine.json `session_started`) → 다음 띄우기는 --resume
    pub resume: bool,
    pub next: Next,
    /// limit_wait 의 끝 시각
    pub wait_until: Option<f64>,
}

/// 바깥 일이 도는 중인가
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inflight {
    Idle,
    Precheck(usize),
    Action,
    Claude,
    Check,
    Wait(WaitWhy),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitWhy {
    Stopline,
    Precheck(usize),
    Limit,
    ClaudeError,
}

/// 도는 일을 멈추게 한 까닭(멈춘 일이 끝났다는 이벤트가 오면 이 까닭대로 간다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopWhy {
    Pause,
    Stop,
    Restart,
    Capped(CapKind),
}

/// 엔진 한 실행의 판단 상태. run.rs 가 파일(run.json·queue.json·frozen.json·engine.json)에서 채우고 Boot 를 보낸다
#[derive(Debug, Clone, PartialEq)]
pub struct EngineState {
    pub run: RunState,
    pub reason: String,
    pub model: String,
    pub units: Vec<UnitPlan>,
    /// units 와 길이가 같다
    pub statuses: Vec<UnitStatus>,
    /// status 가 running 인 단위
    pub current: Option<Current>,
    pub inflight: Inflight,
    pub pending_stop: Option<StopWhy>,
    /// frozen `check_retries`, 기본 2
    pub retries: i64,
    /// frozen `precheck_retry_min`, 기본 5
    pub precheck_retry_min: f64,
    /// frozen `stopline`
    pub stopline: Map<String, Value>,
    pub tz: Tz,
    /// run.json `restart` 칸이 있다
    pub restart_asked: bool,
    /// run.json `restart.unit`
    pub restart_unit: Option<usize>,
}

impl EngineState {
    /// 새 실행: run Starting, 모든 단위 Pending, current None, inflight Idle, pending_stop None, restart 없음, reason 빈 글
    pub fn new(
        model: &str,
        units: Vec<UnitPlan>,
        retries: i64,
        precheck_retry_min: f64,
        stopline: Map<String, Value>,
        tz: Tz,
    ) -> Self {
        EngineState {
            run: RunState::Starting,
            reason: String::new(),
            model: model.to_string(),
            statuses: vec![UnitStatus::Pending; units.len()],
            units,
            current: None,
            inflight: Inflight::Idle,
            pending_stop: None,
            retries,
            precheck_retry_min,
            stopline,
            tz,
            restart_asked: false,
            restart_unit: None,
        }
    }

    pub fn total(&self) -> usize {
        self.units.len()
    }
}

/// claude·판정·고정 단계가 도는 동안 run.rs 가 센 값
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UnitMeter {
    pub active_sec: f64,
    pub cost_usd: f64,
    pub weekly_pp: f64,
}

/// claude 한 프로세스가 끝났을 때의 셈(StreamState 와 종료 코드에서)
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeOutcome {
    pub session_started: bool,
    pub result_seen: bool,
    pub last_error: bool,
    pub limit: bool,
    pub limit_resets: Option<f64>,
    pub budget: bool,
    pub err_text: String,
    pub exit_code: Option<i32>,
    /// 이번 띄우기가 stderr.log 에 쓴 것의 끝(앞뒤 공백 뺌)
    pub stderr_tail: String,
}

impl ClaudeOutcome {
    pub fn from_stream(s: &StreamState, exit_code: Option<i32>, stderr_tail: &str) -> Self {
        ClaudeOutcome {
            session_started: s.session_started,
            result_seen: s.result_seen,
            last_error: s.last_error,
            limit: s.limit,
            limit_resets: s.limit_resets,
            budget: s.budget,
            err_text: s.err_text.clone(),
            exit_code,
            stderr_tail: stderr_tail.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckVerdict {
    Passed,
    Failed { code: i32, tail: String, timed_out: Option<f64> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionVerdict {
    Ok,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// 엔진이 떴다. state 는 파일에서 채운 뒤(run = 직전 상태, 비었으면 Starting).
    /// abnormal_recent = starts.log 에서 이번 줄을 포함한 최근 600초 안 `abnormal` 줄 수(run.rs 가 이번 줄을 먼저 적고 센다)
    Boot { now: f64, abnormal_recent: u32 },
    /// 메인 루프 한 바퀴. inflight 가 Idle 일 때만 run.rs 가 보낸다. usage = run.json `usage`(없으면 None)
    Next { now: f64, usage: Option<Map<String, Value>> },
    PrecheckDone { now: f64, ok: bool, code: i32, last_line: String },
    /// 단위를 시작·진행할 수 없다(가지·git 폴더 확인 실패 등, run.rs 가 판정)
    UnitBlocked { now: f64, n: usize, why: String },
    ActionDone { now: f64, verdict: ActionVerdict },
    /// Claude·Check·Action 이 도는 동안 1초마다. limit_seen = 이번 claude 프로세스가 한도 거절을 받았나
    Tick { now: f64, meter: UnitMeter, limit_seen: bool },
    ClaudeExited { now: f64, outcome: ClaudeOutcome },
    CheckDone { now: f64, verdict: CheckVerdict },
    /// control.json 새 요청(run.rs 가 `control::control_new` 로 거른 action)
    Control { now: f64, action: String },
    /// WaitUntil 시각이 지났다
    WaitElapsed { now: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Board,
    Model,
    Engine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Claude,
    Check,
    Action,
    Precheck,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    SetState { state: RunState, reason: String, wait_until: Option<f64> },
    Log { who: Who, text: String },
    /// 단위 폴더·engine.json(status running, phase, started, weekly_base)·queue status·run.json unit·unit_caps
    BeginUnit { n: usize },
    RunPrecheck { n: usize },
    RunAction { n: usize },
    StartClaude { n: usize, resume: bool, next: Next },
    RunCheck { n: usize, attempt: i64 },
    /// claude 는 stream interrupt → 20초 → kill_tree, 나머지는 kill_tree. 끝나면 run.rs 가 그 Done/Exited 이벤트를 보낸다
    Interrupt { target: Target },
    WaitUntil { until: f64 },
    /// current 를 engine.json 에 적는다(phase·try·errors·limits·next_message·wait_until·session_started)
    SaveUnit { n: usize },
    /// engine.json·queue 를 끝 상태로. Capped 면 git status·diff stat 을 남긴다. 진행 기록 줄은 run.rs 가 쓴다
    EndUnit { n: usize, status: UnitStatus, reason: String },
    SkipUnit { n: usize, why: String },
    /// 직전 엔진이 남긴 claude·판정 프로세스를 끝낸다(engine.json `procs`)
    KillLeftovers { n: usize },
    /// run.json `restart` 칸을 쓴다(`asked`·`ts`·`unit`·`plan`)
    MarkRestart { plan: RestartPlan },
    /// run.json `restart.stopped` 를 쓴다
    MarkRestartStopped,
    /// ended 기록 → 상태 → 끝 정리 → 다시 띄우기 해제 → 엔진 끝. 이미 끝난 실행이 다시 떴으면(Boot) 끝 정리만 다시 한다
    Finish { state: RunState, reason: String },
}

// ── 순수 도우미 ──

/// 턴을 어디까지 기다릴지(원본 turn 3829~3840). run.rs 가 200ms 마다 부른다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnProgress {
    Continue,
    ClearBackground,
    Done,
}

/// exited = 프로세스가 끝났다, at_eof = stdout 끝까지 읽었다. bg_quiet = frozen `background_quiet_sec`(기본 300)
pub fn turn_progress(
    s: &StreamState,
    now: f64,
    bg_quiet: f64,
    bg_cleared: Option<f64>,
    exited: bool,
    at_eof: bool,
) -> TurnProgress {
    let quiet = now - s.last_line;
    let settled = s.result_seen && !s.open;
    if settled && (s.bg == 0 || s.last_error) && quiet >= 2.0 {
        return TurnProgress::Done;
    }
    if settled && s.bg > 0 && quiet >= bg_quiet && bg_cleared.is_none() {
        return TurnProgress::ClearBackground;
    }
    if let Some(k) = bg_cleared {
        if settled && now - k.max(s.last_line) >= 10.0 {
            return TurnProgress::Done;
        }
    }
    if exited && (at_eof || quiet >= 3.0) {
        return TurnProgress::Done;
    }
    TurnProgress::Continue
}

/// 단위 시계 더하기: prev + min(max(now - last_tick, 0), 5) (잠자기로 시계가 튀면 5초까지만)
pub fn account_active(prev: f64, last_tick: f64, now: f64) -> f64 {
    prev + (now - last_tick).clamp(0.0, 5.0)
}

/// 단위 주간 %p 셈(원본 turn 3860~3869)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeeklyMeter {
    pub base: Option<f64>,
    pub last: Option<f64>,
    pub pp_done: f64,
    pub pp: f64,
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

pub fn account_weekly(m: WeeklyMeter, seven_day: Option<f64>, seven_day_resets: Option<f64>, now: f64) -> WeeklyMeter {
    if seven_day_resets.is_some_and(|r| r <= now) {
        return m;
    }
    let Some(cur) = seven_day else { return m };
    let mut base = m.base.unwrap_or(cur);
    let last = m.last.unwrap_or(base);
    let mut pp_done = m.pp_done;
    // 창이 초기화돼 사용률이 줄었다: 그 창에서 쌓은 만큼을 굳히고 새로 센다
    if cur < last {
        pp_done += (last - base).max(0.0) * 100.0;
        base = cur;
    }
    WeeklyMeter {
        base: Some(base),
        last: Some(cur),
        pp_done: r3(pp_done),
        pp: r3(pp_done + (cur - base).max(0.0) * 100.0),
    }
}

// ── 상태 기계 ──

type Out = Vec<Action>;

fn log(out: &mut Out, who: Who, text: impl Into<String>) {
    out.push(Action::Log { who, text: text.into() });
}

fn label(s: &EngineState, n: usize) -> String {
    format!("{n}/{}", s.total())
}

fn title(s: &EngineState, n: usize) -> &str {
    &s.units[n - 1].title
}

fn when(s: &EngineState, t: f64) -> String {
    stopline::when_text(t, s.tz)
}

fn cur_n(s: &EngineState) -> Option<usize> {
    s.current.as_ref().map(|c| c.n)
}

fn working_text(s: &EngineState, n: usize) -> String {
    format!("단위 {} 하는 중: {}", label(s, n), title(s, n))
}

fn set_state(s: &mut EngineState, out: &mut Out, run: RunState, reason: String, wait_until: Option<f64>) {
    s.run = run;
    s.reason = reason.clone();
    out.push(Action::SetState { state: run, reason, wait_until });
}

fn finish(s: &mut EngineState, out: &mut Out, run: RunState, reason: &str) {
    s.run = run;
    s.reason = reason.to_string();
    s.inflight = Inflight::Idle;
    s.pending_stop = None;
    out.push(Action::Finish { state: run, reason: reason.to_string() });
}

fn begin(s: &mut EngineState, out: &mut Out, n: usize, now: f64) {
    s.statuses[n - 1] = UnitStatus::Running;
    let phase = if s.units[n - 1].has_action { UnitPhase::Action } else { UnitPhase::Model };
    s.current = Some(Current {
        n,
        phase,
        tries: 0,
        errors: 0,
        limits: 0,
        resume: false,
        next: Next::First,
        wait_until: None,
    });
    out.push(Action::BeginUnit { n });
    log(out, Who::Board, format!("단위 {} 시작: {}", label(s, n), title(s, n)));
    let reason = working_text(s, n);
    set_state(s, out, RunState::Running, reason, None);
    start_phase(s, out, now);
}

fn start_phase(s: &mut EngineState, out: &mut Out, now: f64) {
    let Some(c) = s.current.clone() else { return };
    match c.phase {
        UnitPhase::Action => {
            out.push(Action::RunAction { n: c.n });
            s.inflight = Inflight::Action;
        }
        UnitPhase::Model => {
            out.push(Action::StartClaude { n: c.n, resume: c.resume, next: c.next });
            s.inflight = Inflight::Claude;
        }
        UnitPhase::Check => {
            out.push(Action::RunCheck { n: c.n, attempt: c.tries });
            s.inflight = Inflight::Check;
        }
        UnitPhase::LimitWait => enter_limit_wait(s, out, now),
    }
}

fn enter_limit_wait(s: &mut EngineState, out: &mut Out, now: f64) {
    let until = s.current.as_ref().and_then(|c| c.wait_until).unwrap_or(now);
    let reason = format!("사용량 한도로 쉬는 중입니다. {} 에 같은 자리에서 다시 시작합니다.", when(s, until));
    set_state(s, out, RunState::WaitingLimit, reason, Some(until));
    out.push(Action::WaitUntil { until });
    s.inflight = Inflight::Wait(WaitWhy::Limit);
}

fn end_unit(s: &mut EngineState, out: &mut Out, n: usize, status: UnitStatus, reason: String) {
    s.statuses[n - 1] = status;
    out.push(Action::EndUnit { n, status, reason });
    s.current = None;
    s.inflight = Inflight::Idle;
    s.pending_stop = None;
}

fn pause_now(s: &mut EngineState, out: &mut Out) {
    log(out, Who::Board, "일시정지했습니다. 재개하면 같은 세션으로 이어 갑니다.");
    set_state(s, out, RunState::Paused, "사람이 일시정지했습니다. 재개하면 이어 갑니다.".to_string(), None);
    s.inflight = Inflight::Idle;
    s.pending_stop = None;
}

fn stop_run(s: &mut EngineState, out: &mut Out) {
    if let Some(n) = cur_n(s) {
        end_unit(s, out, n, UnitStatus::Stopped, "사람이 중단했습니다.".to_string());
    }
    log(out, Who::Board, "사람이 중단해 실행을 마칩니다. 다시 띄우지 않습니다.");
    finish(s, out, RunState::Stopped, "사람이 중단했습니다.");
}

fn restart_stop(s: &mut EngineState, out: &mut Out) {
    let left = s.statuses.iter().filter(|x| **x == UnitStatus::Pending).count();
    out.push(Action::MarkRestartStopped);
    log(
        out,
        Who::Board,
        format!("멈추고 다시 출발: 사람이 요청해 여기서 멈춥니다. 남은 단위 {left}개는 출발 준비에서 다시 고릅니다."),
    );
    finish(s, out, RunState::Stopped, control::RESTART_REASON);
}

fn capped_text(k: CapKind) -> String {
    format!("{} 상한에 닿아 멈췄습니다. 바뀐 파일 목록을 남기고 다음 단위로 갑니다.", k.label())
}

/// 원본 `short(s, max)`: max 자(char)를 넘으면 앞 max-1 자 + `…`
fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max - 1).chain(std::iter::once('…')).collect()
}

fn last_chars(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    let at = s.char_indices().nth(count - n).map_or(s.len(), |(i, _)| i);
    &s[at..]
}

pub fn decide(state: &mut EngineState, event: Event) -> Vec<Action> {
    let mut out = Vec::new();
    if let Event::Boot { abnormal_recent, .. } = event {
        boot(state, &mut out, abnormal_recent);
        return out;
    }
    // 끝난 실행은 Boot 의 끝 정리 말고는 건드리지 않는다
    if state.run.ended() {
        return out;
    }
    match event {
        Event::Boot { .. } => {}
        Event::Next { now, usage } => next(state, &mut out, now, usage.as_ref()),
        Event::PrecheckDone { now, ok, code, last_line } => precheck_done(state, &mut out, now, ok, code, &last_line),
        Event::UnitBlocked { n, why, .. } => unit_blocked(state, &mut out, n, why),
        Event::ActionDone { now, verdict } => action_done(state, &mut out, now, verdict),
        Event::Tick { meter, limit_seen, .. } => tick(state, &mut out, meter, limit_seen),
        Event::ClaudeExited { now, outcome } => claude_exited(state, &mut out, now, outcome),
        Event::CheckDone { now, verdict } => check_done(state, &mut out, now, verdict),
        Event::Control { action, .. } => control_request(state, &mut out, &action),
        Event::WaitElapsed { now } => wait_elapsed(state, &mut out, now),
    }
    out
}

fn boot(s: &mut EngineState, out: &mut Out, abnormal_recent: u32) {
    s.inflight = Inflight::Idle;
    s.pending_stop = None;
    if s.run.ended() {
        let (run, reason) = (s.run, s.reason.clone());
        finish(s, out, run, &reason);
        return;
    }
    let leftover = cur_n(s);
    if s.run != RunState::Starting && abnormal_recent >= 3 {
        if let Some(n) = leftover {
            out.push(Action::KillLeftovers { n });
        }
        log(out, Who::Engine, "엔진 오류로 멈춥니다: 10분 안에 세 번 비정상으로 다시 떴습니다.");
        if let Some(n) = leftover {
            end_unit(s, out, n, UnitStatus::Stopped, "엔진 오류로 멈췄습니다.".to_string());
        }
        finish(s, out, RunState::Error, "엔진 오류로 멈춤: 10분 안에 세 번 비정상으로 다시 떴습니다.");
        return;
    }
    if s.run == RunState::Starting {
        log(out, Who::Engine, format!("자율주행을 시작합니다. 단위 {}개를 차례로 합니다(모델 {}).", s.total(), s.model));
        set_state(s, out, RunState::Running, "단위를 시작하는 중입니다.".to_string(), None);
        return;
    }
    if let Some(n) = leftover {
        out.push(Action::KillLeftovers { n });
    }
    match s.run {
        RunState::Paused => log(out, Who::Engine, "엔진이 다시 떴습니다. 사람이 일시정지한 실행이라 그대로 쉽니다."),
        RunState::TakenOver => {
            log(out, Who::Engine, "엔진이 다시 떴습니다. 사람이 이어받은 실행이라 돌려주기를 그대로 기다립니다.")
        }
        _ => {
            log(out, Who::Engine, "엔진이 다시 떴습니다. 직전에 비정상으로 끝나 하던 자리에서 잇습니다.");
            // 모델이 돌던 중이면 끊긴 자리를 확인하게 하고, 판정 중이면 판정을 다시 돌린다
            if let Some(c) = s.current.as_mut().filter(|c| c.phase == UnitPhase::Model) {
                c.next = Next::Restart;
                out.push(Action::SaveUnit { n: c.n });
            }
        }
    }
}

fn next(s: &mut EngineState, out: &mut Out, now: f64, usage: Option<&Map<String, Value>>) {
    let runs = matches!(
        s.run,
        RunState::Running | RunState::WaitingStopline | RunState::WaitingPrecheck | RunState::WaitingLimit
    );
    if s.inflight != Inflight::Idle || !runs {
        return;
    }
    if s.current.is_some() {
        start_phase(s, out, now);
        return;
    }
    let Some(idx) = s.statuses.iter().position(|x| *x == UnitStatus::Pending) else {
        let count = |k: UnitStatus| s.statuses.iter().filter(|x| **x == k).count();
        let skipped = count(UnitStatus::Skipped);
        let skipped_text = if skipped > 0 { format!(", 건너뜀 {skipped}") } else { String::new() };
        let text = format!(
            "자율주행을 마쳤습니다. 통과 {}, 미통과 {}, 상한에서 멈춤 {}{skipped_text}.",
            count(UnitStatus::Done),
            count(UnitStatus::Failed),
            count(UnitStatus::Capped)
        );
        log(out, Who::Engine, text);
        finish(s, out, RunState::Done, "대기열을 모두 마쳤습니다.");
        return;
    };
    let n = idx + 1;
    if control::restart_stops_before(s.restart_asked, true) {
        restart_stop(s, out);
        return;
    }
    if let Some(w) = stopline::stopline_wait(usage, &s.stopline, now, s.tz) {
        if w.reason != s.reason {
            log(out, Who::Board, w.reason.clone());
        }
        set_state(s, out, RunState::WaitingStopline, w.reason, Some(w.until));
        out.push(Action::WaitUntil { until: w.until });
        s.inflight = Inflight::Wait(WaitWhy::Stopline);
        return;
    }
    if s.units[idx].has_precheck {
        out.push(Action::RunPrecheck { n });
        s.inflight = Inflight::Precheck(n);
    } else {
        begin(s, out, n, now);
    }
}

fn wait_elapsed(s: &mut EngineState, out: &mut Out, now: f64) {
    let Inflight::Wait(why) = s.inflight else { return };
    match why {
        WaitWhy::Stopline => {
            if s.run != RunState::Running {
                set_state(s, out, RunState::Running, "멈출 선 대기를 마치고 다시 확인합니다.".to_string(), None);
            }
            s.inflight = Inflight::Idle;
        }
        WaitWhy::Precheck(n) => {
            out.push(Action::RunPrecheck { n });
            s.inflight = Inflight::Precheck(n);
        }
        WaitWhy::Limit => {
            let Some(c) = s.current.as_mut() else {
                s.inflight = Inflight::Idle;
                return;
            };
            c.phase = UnitPhase::Model;
            c.wait_until = None;
            let n = c.n;
            out.push(Action::SaveUnit { n });
            log(out, Who::Board, "한도 대기를 마치고 같은 세션으로 이어 갑니다.");
            let reason = working_text(s, n);
            set_state(s, out, RunState::Running, reason, None);
            start_phase(s, out, now);
        }
        WaitWhy::ClaudeError => start_phase(s, out, now),
    }
}

fn precheck_done(s: &mut EngineState, out: &mut Out, now: f64, ok: bool, code: i32, last_line: &str) {
    let Inflight::Precheck(n) = s.inflight else { return };
    s.inflight = Inflight::Idle;
    match s.pending_stop {
        Some(StopWhy::Pause) => return pause_now(s, out),
        Some(StopWhy::Stop) => return stop_run(s, out),
        Some(StopWhy::Restart) => return restart_stop(s, out),
        _ => {}
    }
    if ok {
        if s.run == RunState::WaitingPrecheck {
            log(out, Who::Board, "시험 환경 확인을 통과해 단위를 시작합니다.");
        }
        begin(s, out, n, now);
        return;
    }
    let until = now + s.precheck_retry_min * 60.0;
    let mut reason = format!(
        "단위 {} 의 시험 환경 확인이 실패해(종료 코드 {code}) 시작하지 않고 {} 에 다시 확인합니다",
        label(s, n),
        when(s, until)
    );
    if !last_line.is_empty() {
        reason.push_str(&format!(": {last_line}"));
    }
    if s.run != RunState::WaitingPrecheck {
        log(out, Who::Board, reason.clone());
    }
    set_state(s, out, RunState::WaitingPrecheck, reason, Some(until));
    out.push(Action::WaitUntil { until });
    s.inflight = Inflight::Wait(WaitWhy::Precheck(n));
}

fn unit_blocked(s: &mut EngineState, out: &mut Out, n: usize, why: String) {
    if n == 0 || n > s.total() {
        return;
    }
    match cur_n(s) {
        Some(c) if c == n => end_unit(s, out, n, UnitStatus::Failed, why),
        Some(_) => {}
        None => {
            s.statuses[n - 1] = UnitStatus::Skipped;
            out.push(Action::SkipUnit { n, why });
            s.inflight = Inflight::Idle;
        }
    }
}

fn tick(s: &mut EngineState, out: &mut Out, meter: UnitMeter, limit_seen: bool) {
    let target = match s.inflight {
        Inflight::Claude => Target::Claude,
        Inflight::Check => Target::Check,
        Inflight::Action => Target::Action,
        _ => return,
    };
    let Some(n) = cur_n(s) else { return };
    if s.pending_stop.is_some() {
        return;
    }
    let cap = &s.units[n - 1].caps;
    let claude = target == Target::Claude;
    let hit = if meter.active_sec >= cap.minutes * 60.0 {
        Some(CapKind::Minutes)
    } else if claude && !limit_seen && meter.weekly_pp >= cap.weekly_pp {
        // 한도 거절을 받은 턴은 사용률이 튀므로 보지 않는다
        Some(CapKind::Weekly)
    } else if claude && meter.cost_usd >= cap.usd {
        Some(CapKind::Usd)
    } else {
        None
    };
    if let Some(k) = hit {
        s.pending_stop = Some(StopWhy::Capped(k));
        out.push(Action::Interrupt { target });
    }
}

/// claude 가 오류로 끝났다: 세 번까지 1분 뒤 같은 세션으로 다시 열고, 넘으면 미통과
fn claude_failed(s: &mut EngineState, out: &mut Out, now: f64, msg: &str) {
    let Some(c) = s.current.as_mut() else { return };
    c.errors += 1;
    c.next = Next::Error;
    let (n, errors) = (c.n, c.errors);
    out.push(Action::SaveUnit { n });
    if errors >= ERROR_LIMIT {
        end_unit(s, out, n, UnitStatus::Failed, format!("claude 가 세 번 오류로 끝나 미통과로 기록합니다: {msg}"));
        return;
    }
    log(out, Who::Board, format!("claude 가 오류로 끝났습니다({msg}). 1분 뒤 같은 세션으로 다시 엽니다({errors}/{ERROR_LIMIT})."));
    out.push(Action::WaitUntil { until: now + ERROR_RETRY_SEC });
    s.inflight = Inflight::Wait(WaitWhy::ClaudeError);
}

fn claude_exited(s: &mut EngineState, out: &mut Out, now: f64, o: ClaudeOutcome) {
    if s.inflight != Inflight::Claude {
        return;
    }
    s.inflight = Inflight::Idle;
    let Some(c) = s.current.as_mut() else { return };
    c.resume |= o.session_started;
    let n = c.n;
    match s.pending_stop {
        Some(StopWhy::Pause) => {
            c.next = Next::Paused;
            out.push(Action::SaveUnit { n });
            return pause_now(s, out);
        }
        Some(StopWhy::Stop) => return stop_run(s, out),
        Some(StopWhy::Capped(k)) => return end_unit(s, out, n, UnitStatus::Capped, capped_text(k)),
        _ => {}
    }
    if o.limit {
        let until = match o.limit_resets {
            Some(r) if r > now => r,
            _ => now + LIMIT_UNKNOWN_SEC,
        } + LIMIT_AFTER_RESET_SEC;
        c.phase = UnitPhase::LimitWait;
        c.wait_until = Some(until);
        c.next = Next::Limit;
        c.limits += 1;
        out.push(Action::SaveUnit { n });
        let unknown = if o.limit_resets.is_none() { "(초기화 시각을 몰라 30분 뒤)" } else { "" };
        log(out, Who::Board, format!("사용량 한도에 막혔습니다. {} 에 같은 세션으로 다시 시작합니다{unknown}.", when(s, until)));
        enter_limit_wait(s, out, now);
        return;
    }
    if o.budget {
        return end_unit(s, out, n, UnitStatus::Capped, capped_text(CapKind::Usd));
    }
    if !o.result_seen {
        let mut msg = format!("결과 없이 끝남, 종료 코드 {}", o.exit_code.unwrap_or(-1));
        if !o.stderr_tail.is_empty() {
            msg.push_str(&format!(": {}", short(last_chars(&o.stderr_tail, 300), 120)));
        }
        return claude_failed(s, out, now, &msg);
    }
    if o.last_error {
        let msg = if o.err_text.is_empty() { "claude 오류" } else { &o.err_text };
        return claude_failed(s, out, now, msg);
    }
    c.phase = UnitPhase::Check;
    out.push(Action::SaveUnit { n });
    start_phase(s, out, now);
}

fn check_done(s: &mut EngineState, out: &mut Out, now: f64, verdict: CheckVerdict) {
    if s.inflight != Inflight::Check {
        return;
    }
    s.inflight = Inflight::Idle;
    let Some(n) = cur_n(s) else { return };
    match s.pending_stop {
        Some(StopWhy::Pause) => return pause_now(s, out),
        Some(StopWhy::Stop) => return stop_run(s, out),
        Some(StopWhy::Capped(k)) => return end_unit(s, out, n, UnitStatus::Capped, capped_text(k)),
        _ => {}
    }
    let (code, tail, timed_out) = match verdict {
        CheckVerdict::Passed => return end_unit(s, out, n, UnitStatus::Done, "판정 명령을 통과했습니다.".to_string()),
        CheckVerdict::Failed { code, tail, timed_out } => (code, tail, timed_out),
    };
    let retries = s.retries;
    let check = s.units[n - 1].check.clone();
    let Some(c) = s.current.as_mut() else { return };
    if c.tries >= retries {
        let tries = c.tries;
        return end_unit(s, out, n, UnitStatus::Failed, checks::check_fail_reason(tries, timed_out));
    }
    let k = c.tries;
    c.tries = k + 1;
    c.phase = UnitPhase::Model;
    c.next = Next::Text(checks::check_fail_message(code, &check, &tail, timed_out));
    out.push(Action::SaveUnit { n });
    log(out, Who::Board, checks::check_fail_event(code, k, retries, timed_out));
    start_phase(s, out, now);
}

fn action_done(s: &mut EngineState, out: &mut Out, now: f64, verdict: ActionVerdict) {
    if s.inflight != Inflight::Action {
        return;
    }
    s.inflight = Inflight::Idle;
    let Some(n) = cur_n(s) else { return };
    match s.pending_stop {
        Some(StopWhy::Pause) => return pause_now(s, out),
        Some(StopWhy::Stop) => return stop_run(s, out),
        Some(StopWhy::Capped(k)) => {
            return end_unit(s, out, n, UnitStatus::Capped, format!("{} 상한에 닿아 멈췄습니다.", k.label()))
        }
        _ => {}
    }
    match verdict {
        ActionVerdict::Failed(why) => end_unit(s, out, n, UnitStatus::Failed, why),
        ActionVerdict::Ok => {
            if let Some(c) = s.current.as_mut() {
                c.phase = UnitPhase::Model;
            }
            out.push(Action::SaveUnit { n });
            start_phase(s, out, now);
        }
    }
}

fn busy_target(s: &EngineState) -> Option<Target> {
    match s.inflight {
        Inflight::Claude => Some(Target::Claude),
        Inflight::Check => Some(Target::Check),
        Inflight::Action => Some(Target::Action),
        Inflight::Precheck(_) => Some(Target::Precheck),
        Inflight::Idle | Inflight::Wait(_) => None,
    }
}

/// 도는 일을 멈추라고 한다. 이미 멈추는 중이면 새로 끊지 않고, 중단만 먼저 걸린 까닭을 덮는다
fn request_stop(s: &mut EngineState, out: &mut Out, why: StopWhy, target: Target) {
    match s.pending_stop {
        None => {
            s.pending_stop = Some(why);
            out.push(Action::Interrupt { target });
        }
        Some(StopWhy::Stop) => {}
        Some(_) if why == StopWhy::Stop => s.pending_stop = Some(StopWhy::Stop),
        Some(_) => {}
    }
}

fn control_request(s: &mut EngineState, out: &mut Out, action: &str) {
    let mode = match s.run {
        RunState::Paused => ControlMode::Paused,
        RunState::TakenOver => ControlMode::TakenOver,
        _ => ControlMode::Running,
    };
    let o = control::control_outcome_mode(action, mode);
    if let Some(text) = o.text {
        log(out, Who::Board, text);
    }
    let busy = busy_target(s);
    match o.step {
        ControlStep::Pause => match busy {
            Some(t) => request_stop(s, out, StopWhy::Pause, t),
            None => pause_now(s, out),
        },
        ControlStep::Stop => match busy.filter(|_| mode == ControlMode::Running) {
            Some(t) => request_stop(s, out, StopWhy::Stop, t),
            None => stop_run(s, out),
        },
        ControlStep::Restart => request_restart(s, out, mode == ControlMode::Paused, busy),
        ControlStep::Takeover => log(out, Who::Board, "Windows 판은 아직 터미널로 이어받기를 하지 않아 넘깁니다."),
        ControlStep::Resume => {
            set_state(s, out, RunState::Running, "재개했습니다.".to_string(), None);
            s.inflight = Inflight::Idle;
        }
        ControlStep::Giveback | ControlStep::None => {}
    }
}

fn request_restart(s: &mut EngineState, out: &mut Out, paused: bool, busy: Option<Target>) {
    let names: Vec<&str> = s.statuses.iter().map(|x| x.as_str()).collect();
    let plan = control::restart_plan(&names, s.restart_asked);
    log(out, Who::Board, control::restart_text(plan, paused, s.restart_unit));
    match plan {
        RestartPlan::AfterUnit(u) => {
            s.restart_asked = true;
            s.restart_unit = Some(u);
            out.push(Action::MarkRestart { plan });
        }
        RestartPlan::Now => {
            s.restart_asked = true;
            out.push(Action::MarkRestart { plan });
            match busy {
                Some(t) => request_stop(s, out, StopWhy::Restart, t),
                None => restart_stop(s, out),
            }
        }
        RestartPlan::Already => {}
    }
}
