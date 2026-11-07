//! 엔진 본체 — `engine::decide` 를 구동하고 `Action` 을 포트 위에서 수행한다(설계 원본 macOS `main.swift` 의 `Engine`).
//! 순수 부품(모델 글·실행 기록 행·파일 → 상태)은 `load`·`prompt`·`rows` 에 있고 포트를 부르지 않는다.
//! 이 파일은 열기(잠금·해시·starts.log)와 1초 tick 루프, 파일 쓰기 도우미를 맡는다.
//! 턴(claude 한 번 띄워 줄을 읽는 일)은 `turn`, 수행(`Action` 하나하나)·판정 돌리기는 `perform` 에 있다.
//!
//! 층: 이 모듈만 바깥(포트)과 말한다. 바깥에서 오는 결과는 이벤트로 `decide` 에 넣고, 나온 `Action` 을 차례대로 수행한다.
//! 시간은 모두 `Clock` 포트에서 받는다(시험은 가짜 시계로 기다리지 않는다).

use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_core::clock;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::engine::decide::{
    account_active, account_weekly, decide, EngineState, Event, Inflight, RunState, UnitMeter, WeeklyMeter, Who,
};
use crate::model::{FrozenFile, Num, Ordered, QueueFile, QueueUnit, RunFile, Slot, UnitProgress, UnitProgressFile};
use crate::ports::{
    ChildProcess, Clock, ExecEnv, Git, Held, PortError, PortErrorKind, Records, Row, RunFiles, Supervisor, Usage, UsageFrom,
    UsageSnapshot,
};
use crate::rules::control;
use crate::rules::stopline::Tz;
use crate::stream::StreamState;

pub mod guard;
mod load;
mod perform;
mod prompt;
mod rows;
mod turn;

pub use load::{
    abnormal_recent, caps_of, frozen_digest, is_abnormal, load_state, next_from, next_text, session_uuid, starts_line, Loaded,
};
pub use prompt::{first_message, is_under, PromptUnit, RESULT_FORMAT, SYSTEM_PROMPT};
pub use rows::{project_row, run_row, unit_row};

/// 모델이 쓸 수 있는 폴더의 파일(`result.json`·단위 `engine.json`·`control.json`)을 엔진이 통째로 읽어도 되는 크기.
/// 넘으면 읽지 않고 "쓸 수 있는 내용이 없다"로 본다(모델이 거대한 파일을 두어 엔진의 메모리를 쓰게 하지 못하게)
pub const MODEL_FILE_MAX: u64 = 1 << 20;

/// 고정 설정이 출발 뒤에 바뀌었을 때 run.json 에 적는 까닭
pub const FROZEN_CHANGED_REASON: &str = "고정 설정(pinned/frozen.json)이 출발 뒤에 바뀌어 멈춥니다.";

/// 엔진이 바깥과 말하는 길 전부. 시험은 가짜 구현을 넣는다
pub struct Ports<'p> {
    pub files: &'p dyn RunFiles,
    pub env: &'p dyn ExecEnv,
    pub git: &'p dyn Git,
    /// 열지 못했으면 None — run.json `records_error` 에 까닭을 두고 실행은 이어 간다
    pub records: Option<&'p dyn Records>,
    pub usage: &'p dyn Usage,
    pub clock: &'p dyn Clock,
    pub supervisor: &'p dyn Supervisor,
    /// 직전 엔진이 남긴 프로세스(pid, 생성 시각)를 끝낸다. 실제는 core::proc::{alive, terminate}, 시험은 기록만
    pub reap: &'p dyn Fn(u32, Option<f64>) -> bool,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    /// 실행 폴더
    pub dir: PathBuf,
    /// `<뿌리>\private\<실행 id>` (엔진 전용, 0700 대응)
    pub private_dir: PathBuf,
    /// run 명령 인자
    pub frozen_hash: String,
    pub tz: Tz,
    /// 1초
    pub tick: Duration,
    /// claude 줄 읽기 기다림 200ms
    pub poll: Duration,
    /// 시험 모드에서 넘길 FAKE_CLAUDE_* (4.3 이 채움, 실제 실행은 빈 목록)
    pub claude_env_extra: Vec<(String, String)>,
    pub pid: u32,
    pub exe: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunExit {
    Finished(RunState),
    LockHeld,
    FrozenChanged,
    LoadFailed(String),
}

pub struct Engine<'p> {
    p: Ports<'p>,
    cfg: RunConfig,
    run: RunFile,
    queue: QueueFile,
    frozen: FrozenFile,
    state: EngineState,
    weekly: WeeklyMeter,
    /// 도는 단위의 engine.json (메모리 사본)
    unit: Option<(usize, UnitProgressFile)>,
    /// 같은 단위 안에서 claude 를 다시 띄워도 이어지는 셈
    stream: Option<StreamState>,
    stream_sid: String,
    turn: Option<turn::Turn>,
    child: Option<Box<dyn ChildProcess>>,
    job: Option<perform::Job>,
    job_last_tick: f64,
    interrupt_logged: bool,
    wait_until: Option<f64>,
    abnormal_recent: u32,
    finished: bool,
    _lock: Box<dyn Held>,
}

// ── 값 도우미 ──

/// 숫자, 또는 숫자로 읽히는 글자(원본 `num`)
fn jnum(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 소수 셋째 자리로 둥글린 JSON 숫자(정수로 떨어지면 정수)
fn r3v(x: f64) -> Value {
    Num::r3(x).raw().clone()
}

fn slot_f(n: &Slot<Num>) -> f64 {
    n.get().and_then(Num::as_f64).unwrap_or(0.0)
}

/// 초 → 사람이 읽는 길이(원본 `dur`)
fn dur(sec: f64) -> String {
    let s = sec.round() as i64;
    if s < 60 {
        format!("{s}초")
    } else if s < 3600 {
        if s % 60 == 0 {
            format!("{}분", s / 60)
        } else {
            format!("{}분 {}초", s / 60, s % 60)
        }
    } else {
        format!("{}시간 {}분", s / 3600, (s % 3600) / 60)
    }
}

/// 원본 `usd`: 1 미만은 소수 셋째, 이상은 둘째
fn usd(x: f64) -> String {
    if x < 1.0 {
        format!("{x:.3}")
    } else {
        format!("{x:.2}")
    }
}

fn who_str(w: Who) -> &'static str {
    match w {
        Who::Board => "board",
        Who::Model => "model",
        Who::Engine => "engine",
    }
}

fn read_required(files: &dyn RunFiles, path: &Path, name: &str) -> Result<Value, RunExit> {
    match files.read_json(path) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(RunExit::LoadFailed(format!("{name} 이(가) 없습니다."))),
        Err(e) => Err(RunExit::LoadFailed(format!("{name} 을(를) 읽지 못했습니다: {e}"))),
    }
}

fn parse<T: serde::de::DeserializeOwned>(v: Value, name: &str) -> Result<T, RunExit> {
    serde_json::from_value(v).map_err(|e| RunExit::LoadFailed(format!("{name} 꼴이 틀립니다: {e}")))
}

/// 사용률 스냅숏 → run.json `usage` 꼴
fn usage_value(s: &UsageSnapshot) -> Value {
    let mut m = Map::new();
    m.insert("five_hour".into(), Num::from_f64(s.five_hour).raw().clone());
    if let Some(r) = s.five_hour_resets {
        m.insert("five_hour_resets".into(), Num::from_f64(r).raw().clone());
    }
    m.insert("seven_day".into(), Num::from_f64(s.seven_day).raw().clone());
    if let Some(r) = s.seven_day_resets {
        m.insert("seven_day_resets".into(), Num::from_f64(r).raw().clone());
    }
    m.insert("at".into(), r3v(s.at));
    let from = match s.from {
        UsageFrom::Stream => "stream",
        UsageFrom::Cache => "cache",
        UsageFrom::Api => "api",
        UsageFrom::Toy => "toy",
    };
    m.insert("from".into(), Value::from(from));
    Value::Object(m)
}

impl<'p> Engine<'p> {
    /// 잠금 → 파일 읽기 → 해시 확인 → starts.log 줄 → load_state. 잠금을 못 잡으면 LockHeld(아무것도 쓰지 않는다)
    pub fn open(ports: Ports<'p>, cfg: RunConfig) -> Result<Self, RunExit> {
        let files = ports.files;
        let lock = match files.try_lock(&cfg.dir.join(".engine.lock")) {
            Ok(Some(l)) => l,
            Ok(None) => return Err(RunExit::LockHeld),
            Err(e) => return Err(RunExit::LoadFailed(format!("실행 잠금을 잡지 못했습니다: {e}"))),
        };
        let run_v = read_required(files, &cfg.dir.join("run.json"), "run.json")?;
        let queue_v = read_required(files, &cfg.dir.join("queue.json"), "queue.json")?;
        let frozen_v = read_required(files, &cfg.dir.join("pinned").join("frozen.json"), "pinned/frozen.json")?;
        let hash_ok = frozen_digest(&frozen_v) == cfg.frozen_hash;
        let run: RunFile = parse(run_v, "run.json")?;
        let queue: QueueFile = parse(queue_v, "queue.json")?;
        let frozen: FrozenFile = parse(frozen_v, "pinned/frozen.json")?;
        let tz = cfg.tz;
        let mut e = Engine {
            p: ports,
            cfg,
            run,
            queue,
            frozen,
            state: EngineState::new("", Vec::new(), 2, 5.0, Map::new(), tz),
            weekly: WeeklyMeter::default(),
            unit: None,
            stream: None,
            stream_sid: String::new(),
            turn: None,
            child: None,
            job: None,
            job_last_tick: 0.0,
            interrupt_logged: false,
            wait_until: None,
            abnormal_recent: 0,
            finished: false,
            _lock: lock,
        };
        if !hash_ok {
            return Err(e.frozen_changed());
        }
        e.append_starts_line();
        if let Err(err) = e.p.files.make_private_dir(&e.cfg.private_dir) {
            return Err(RunExit::LoadFailed(format!("엔진 전용 폴더를 만들지 못했습니다: {err}")));
        }
        let running = e.running_unit_file()?;
        let loaded = load_state(&e.run, &e.queue, &e.frozen, running.as_ref().map(|(_, f)| f), e.cfg.tz)
            .map_err(RunExit::LoadFailed)?;
        e.state = loaded.state;
        e.weekly = loaded.weekly;
        e.unit = running;
        Ok(e)
    }

    /// 이번 시작을 포함한 최근 10분 안 비정상 시작 수(시험·진단용)
    pub fn abnormal_recent(&self) -> u32 {
        self.abnormal_recent
    }

    /// Boot 부터 Finish 까지. Finish 를 수행하면 돌아온다
    pub fn run(mut self) -> RunExit {
        let now = self.now();
        self.run.engine_pid.set(i64::from(self.cfg.pid));
        let first = self.state.run == RunState::Starting;
        if first {
            self.run.started.set(Num::r3(now));
        }
        let abnormal_recent = self.abnormal_recent;
        self.dispatch(Event::Boot { now, abnormal_recent });
        if !self.finished {
            self.save_run();
            if first {
                self.record("출발");
            }
        }
        while !self.finished {
            self.step();
        }
        RunExit::Finished(self.state.run)
    }

    // ── 열기 ──

    fn frozen_changed(&mut self) -> RunExit {
        let now = self.now();
        if self.run.ended.get().is_none() {
            self.run.ended.set(Num::r3(now));
        }
        self.state.run = RunState::Error;
        self.state.reason = FROZEN_CHANGED_REASON.to_string();
        self.write_state(RunState::Error, FROZEN_CHANGED_REASON, None);
        self.event(Who::Engine, FROZEN_CHANGED_REASON);
        self.unregister();
        RunExit::FrozenChanged
    }

    fn append_starts_line(&mut self) {
        let prev = self.run.state.get().cloned().unwrap_or_default();
        let now = self.now();
        let l = clock::local(now);
        let local = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", l.year, l.month, l.day, l.hour, l.minute, l.second);
        let line = starts_line(now, &local, is_abnormal(&prev), self.cfg.pid, &prev, &self.cfg.exe);
        let path = self.path("starts.log");
        if let Err(e) = self.p.files.append_line(&path, &line) {
            self.warn("starts.log", &e);
        }
        let text = match self.p.files.read_from(&path, 0, 1 << 20) {
            Ok(Some(b)) => String::from_utf8_lossy(&b).into_owned(),
            _ => line,
        };
        self.abnormal_recent = abnormal_recent(&text, now);
    }

    /// status 가 running 인 단위의 engine.json. 단위가 없으면 None. engine.json 이 없으면 load_state 가 처음 값으로 잇는다
    fn running_unit_file(&self) -> Result<Option<(usize, UnitProgressFile)>, RunExit> {
        let Some(i) = self.queue.iter().position(|q| q.status.get().map(String::as_str) == Some("running")) else {
            return Ok(None);
        };
        let n = i + 1;
        match self.read_capped(&self.unit_state_path(n)) {
            Ok(Some(v)) => Ok(Some((n, parse(v, &format!("unit-{n}.json"))?))),
            Ok(None) => Ok(None),
            Err(e) => Err(RunExit::LoadFailed(format!("unit-{n}.json 을(를) 읽지 못했습니다: {e}"))),
        }
    }

    // ── 루프 ──

    /// 한 바퀴. 끝났으면 `finished` 가 선다
    fn step(&mut self) {
        if self.state.run.ended() {
            // Finish 가 이미 수행됐어야 하는 자리 — 헛돌지 않게 닫는다
            self.finished = true;
            return;
        }
        if matches!(self.state.run, RunState::Paused | RunState::TakenOver) {
            if !self.poll_control() {
                self.sleep_tick();
            }
            return;
        }
        match self.state.inflight {
            Inflight::Idle => {
                let (now, usage) = (self.now(), self.pick_usage());
                if self.dispatch(Event::Next { now, usage }) == 0 {
                    self.sleep_tick();
                }
            }
            Inflight::Wait(_) => {
                if self.poll_control() {
                    return;
                }
                let now = self.now();
                if self.wait_until.is_some_and(|u| now >= u) {
                    self.dispatch(Event::WaitElapsed { now });
                } else {
                    self.sleep_tick();
                }
            }
            Inflight::Claude => self.turn_step(),
            Inflight::Check | Inflight::Precheck(_) | Inflight::Action => self.run_pending_job(),
        }
    }

    /// 이벤트를 넣고 나온 Action 을 차례로 수행한다. 나온 Action 수
    fn dispatch(&mut self, ev: Event) -> usize {
        let actions = decide(&mut self.state, ev);
        let count = actions.len();
        for a in actions {
            if self.finished {
                break;
            }
            self.perform(a);
        }
        count
    }

    fn sleep_tick(&self) {
        self.p.clock.sleep(self.cfg.tick);
    }

    /// 새 control 요청을 보고 있으면 이벤트로 넣고 true. 파일은 늘 지운다(다시 뜬 엔진이 같은 요청을 또 하지 않게)
    fn poll_control(&mut self) -> bool {
        let path = self.path("control.json");
        let stat = match self.p.files.stat(&path) {
            Ok(Some(s)) => s,
            Ok(None) => return false,
            Err(e) => {
                self.warn("control.json", &e);
                return false;
            }
        };
        let json = match self.read_capped(&path) {
            Ok(v) => v,
            Err(e) => {
                self.warn("control.json", &e);
                None
            }
        };
        if let Err(e) = self.p.files.remove(&path) {
            self.warn("control.json 지우기", &e);
        }
        let seen = self.run.control_seen.get().and_then(Num::as_f64);
        let Some((action, ts)) = control::control_new(json.as_ref(), stat.modified, seen) else { return false };
        self.run.control_seen.set(Num::from_f64(ts));
        self.run.control_last.set(action.clone());
        self.save_run();
        let now = self.now();
        self.dispatch(Event::Control { now, action });
        true
    }

    /// run.json `usage` 가 900초보다 오래됐거나 없으면 포트로 다시 읽는다. 이벤트에 넘길 run.json `usage`(없으면 None)
    fn pick_usage(&mut self) -> Option<Map<String, Value>> {
        let now = self.now();
        let at = self.run.usage.get().and_then(|u| u.get("at")).and_then(jnum);
        if at.is_none_or(|a| now - a >= 900.0) {
            match self.p.usage.fallback(now) {
                Ok(Some(snap)) => {
                    self.run.usage.set(usage_value(&snap));
                    self.save_run();
                }
                Ok(None) => {}
                Err(e) => self.warn("사용률 읽기", &e),
            }
        }
        self.run.usage.get().and_then(Value::as_object).cloned()
    }

    // ── 경로 ──

    fn now(&self) -> f64 {
        self.p.clock.now()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.cfg.dir.join(name)
    }

    fn run_id(&self) -> String {
        self.cfg.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn unit_dir(&self, n: usize) -> PathBuf {
        self.cfg.dir.join("units").join(n.to_string())
    }

    /// 엔진이 판단에 쓰는 단위 진행 파일. 엔진 전용 폴더에 있어 모델 세션이 쓰지도 읽지도 못한다.
    /// `units/<n>/engine.json` 은 보드가 그리는 사본일 뿐이라 엔진은 그것을 읽지 않는다
    fn unit_state_path(&self, n: usize) -> PathBuf {
        self.cfg.private_dir.join(format!("unit-{n}.json"))
    }

    /// 고정 설정의 단위 정의(없으면 queue)
    fn def(&self, n: usize) -> &Ordered<QueueUnit> {
        self.frozen.units.get().and_then(|u| u.get(n - 1)).unwrap_or(&self.queue[n - 1])
    }

    fn def_text(&self, n: usize, key: &str) -> Option<String> {
        self.def(n).extra.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
    }

    // ── 모델이 쓸 수 있는 파일 읽기 ──

    /// 이 파일이 `MODEL_FILE_MAX` 를 넘는가(넘으면 그 크기). 없거나 재지 못하면 None
    fn oversize(&self, path: &Path) -> Option<u64> {
        self.p.files.stat(path).ok().flatten().map(|s| s.len).filter(|len| *len > MODEL_FILE_MAX)
    }

    /// `read_json` 인데 `MODEL_FILE_MAX` 를 넘는 파일은 읽지 않고 Invalid 로 돌려준다
    fn read_capped(&self, path: &Path) -> Result<Option<Value>, PortError> {
        if let Some(len) = self.oversize(path) {
            return Err(PortError::new(PortErrorKind::Invalid, format!("{}: {len} 바이트라 {MODEL_FILE_MAX} 바이트를 넘어 읽지 않는다", path.display())));
        }
        self.p.files.read_json(path)
    }

    // ── 쓰기 ──

    /// 쓰기 실패는 삼키지 않고 표준 오류에 남긴다(창 없는 실행 파일이면 버려진다)
    fn warn(&self, what: &str, e: &dyn std::fmt::Display) {
        eprintln!("claude-autopilot: {what}: {e}");
    }

    fn put(&self, path: &Path, v: &impl Serialize) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match serde_json::to_value(v) {
            Ok(value) => {
                if let Err(e) = self.p.files.write_json(path, &value) {
                    self.warn(&name, &e);
                }
            }
            Err(e) => self.warn(&name, &e),
        }
    }

    fn save_run(&mut self) {
        let now = self.now();
        self.run.updated.set(Num::r3(now));
        self.put(&self.path("run.json"), &self.run);
    }

    fn save_queue(&self) {
        self.put(&self.path("queue.json"), &self.queue);
    }

    fn write_state(&mut self, state: RunState, reason: &str, wait_until: Option<f64>) {
        self.run.state.set(state.as_str().to_string());
        self.run.reason.set(reason.to_string());
        match wait_until {
            Some(t) => self.run.wait_until.set(Num::r3(t)),
            None => self.run.wait_until.set_null(),
        }
        self.save_run();
    }

    /// events.jsonl 한 줄. 원본 `event`
    fn event(&mut self, who: Who, text: &str) {
        let now = self.now();
        let mut m = Map::new();
        m.insert("text".into(), Value::from(text));
        m.insert("ts".into(), r3v(now));
        m.insert("who".into(), Value::from(who_str(who)));
        if let Err(e) = self.p.files.append_line(&self.path("events.jsonl"), &Value::Object(m).to_string()) {
            self.warn("events.jsonl", &e);
        }
        self.run.last_activity.set(Num::r3(now));
    }

    /// context.jsonl 한 줄(키는 이름 순). 원본 `context`
    fn context(&mut self, n: usize, fields: Vec<(&str, Value)>) {
        let now = self.now();
        let mut all: Vec<(String, Value)> = fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        all.push(("ts".into(), r3v(now)));
        all.push(("unit".into(), Value::from(n)));
        all.sort_by(|a, b| a.0.cmp(&b.0));
        let line = Value::Object(all.into_iter().collect()).to_string();
        if let Err(e) = self.p.files.append_line(&self.path("context.jsonl"), &line) {
            self.warn("context.jsonl", &e);
        }
    }

    fn unregister(&mut self) {
        let Some(label) = self.run.launchd_label.get().cloned() else { return };
        if let Err(e) = self.p.supervisor.unregister(&label) {
            self.event(Who::Engine, &format!("작업 스케줄러에서 다시 띄우기를 해제하지 못했습니다({label}): {e}"));
        }
    }

    // ── 단위 engine.json (메모리 사본) ──

    fn load_unit_file(&self, n: usize) -> UnitProgressFile {
        let found = self.read_capped(&self.unit_state_path(n)).ok().flatten();
        found.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_else(|| {
            let mut e = UnitProgress::default();
            e.unit.set(n as u32);
            e.id.set(self.def(n).id.get().cloned().unwrap_or_else(|| format!("u{n}")));
            e.title.set(self.def(n).title.get().cloned().unwrap_or_else(|| format!("단위 {n}")));
            e.status.set("running".to_string());
            Ordered::new(e)
        })
    }

    fn ensure_unit(&mut self, n: usize) {
        if self.unit.as_ref().map(|(m, _)| *m) != Some(n) {
            let e = self.load_unit_file(n);
            self.unit = Some((n, e));
        }
    }

    fn unit_mut(&mut self, n: usize) -> &mut UnitProgressFile {
        self.ensure_unit(n);
        match self.unit.as_mut() {
            Some((_, e)) => e,
            None => unreachable!("ensure_unit 가 채웠다"),
        }
    }

    /// 메모리 사본을 엔진 전용 `unit-<n>.json` 에 쓰고, 보드가 그리는 `units/<n>/engine.json` 에 같은 글을 복사한다
    fn save_unit(&mut self, n: usize) {
        let now = self.now();
        let value = match self.unit.as_mut() {
            Some((m, e)) if *m == n => {
                e.updated.set(Num::r3(now));
                serde_json::to_value(&*e)
            }
            _ => return,
        };
        match value {
            Ok(v) => {
                if let Err(err) = self.p.files.write_json(&self.unit_state_path(n), &v) {
                    self.warn("unit-state", &err);
                }
                if let Err(err) = self.p.files.write_json(&self.unit_dir(n).join("engine.json"), &v) {
                    self.warn("engine.json", &err);
                }
            }
            Err(err) => self.warn("engine.json", &err),
        }
    }

    /// 단위 시계·비용·주간 %p 를 고쳐 engine.json 메모리 사본에 넣고 Tick 에 넘길 값을 돌려준다.
    /// `running_cost` 는 지금 도는 claude 프로세스의 누적 비용(판정·고정 단계는 0)
    fn account(&mut self, n: usize, last_tick: f64, now: f64, running_cost: f64) -> UnitMeter {
        let usage = self.run.usage.get().and_then(Value::as_object);
        let seven = usage.and_then(|u| u.get("seven_day")).and_then(jnum);
        let resets = usage.and_then(|u| u.get("seven_day_resets")).and_then(jnum);
        self.weekly = account_weekly(self.weekly, seven, resets, now);
        let w = self.weekly;
        let e = self.unit_mut(n);
        let active = account_active(slot_f(&e.active_sec), last_tick, now);
        e.active_sec.set(Num::r3(active));
        let cost = slot_f(&e.cost_done) + running_cost;
        e.cost_usd.set(Num::r3(cost));
        if let Some(b) = w.base {
            e.weekly_base.set(Num::from_f64(b));
        }
        if let Some(l) = w.last {
            e.weekly_last.set(Num::from_f64(l));
        }
        e.weekly_pp.set(Num::r3(w.pp));
        e.weekly_pp_done.set(Num::r3(w.pp_done));
        UnitMeter { active_sec: active, cost_usd: cost, weekly_pp: w.pp }
    }

    // ── 실행 기록 저장소 ──

    /// 기록 저장소에 이 실행을 옮겨 적는다(출발·단위 판정 뒤·끝). 실패해도 실행은 멈추지 않고, 다음 기록 때 실행 폴더에서 통째로 다시 옮긴다
    fn record(&mut self, when: &str) {
        let Some(records) = self.p.records else { return };
        match self.sync_records(records) {
            Err(why) => {
                self.run.extra.insert("records_error".into(), Value::String(why.clone()));
                self.save_run();
                self.event(
                    Who::Engine,
                    &format!(
                        "실행 기록 저장소에 쓰지 못했습니다({when}: {why}). 실행은 그대로 가고, 다음 기록 때 실행 폴더에서 다시 옮깁니다."
                    ),
                );
            }
            Ok(()) => {
                if self.run.extra.shift_remove("records_error").is_some() {
                    self.save_run();
                    self.event(Who::Engine, &format!("실행 기록 저장소에 다시 옮겨 적었습니다({when})."));
                }
            }
        }
    }

    fn sync_records(&mut self, records: &dyn Records) -> Result<(), String> {
        let now = self.now();
        if let Some((n, _)) = self.unit {
            self.save_unit(n);
        }
        if let Some(project) = self.frozen.project.get().or_else(|| self.run.project.get()).cloned() {
            let row = self.project_row_for(records, &project, now)?;
            records.upsert("projects", &row).map_err(|e| e.to_string())?;
        }
        let engs: Vec<Option<UnitProgressFile>> = (1..=self.queue.len())
            .map(|n| {
                let v = self.read_capped(&self.unit_state_path(n)).ok().flatten()?;
                serde_json::from_value(v).ok()
            })
            .collect();
        let dir = self.cfg.dir.to_string_lossy().into_owned();
        let run_id = self.run_id();
        let runs = run_row(&run_id, &dir, &self.run, &self.queue, &self.frozen, &engs, now);
        records.upsert("runs", &runs).map_err(|e| e.to_string())?;
        let model = self.frozen.model.get().or_else(|| self.run.model.get()).cloned();
        for (i, q) in self.queue.iter().enumerate() {
            let n = i + 1;
            let result_path = self.unit_dir(n).join("result.json");
            let result_text = match self.oversize(&result_path) {
                Some(_) => None,
                None => match self.p.files.read_from(&result_path, 0, MODEL_FILE_MAX as usize) {
                    Ok(Some(b)) => Some(String::from_utf8_lossy(&b).into_owned()),
                    _ => None,
                },
            };
            let status = q.status.get().map_or("pending", String::as_str);
            let row = unit_row(&run_id, n, self.def(n), status, engs[i].as_ref(), result_text.as_deref(), model.as_deref());
            records.upsert("units", &row).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 프로젝트 행. 저장소 목록은 가지 기록의 저장소(없으면 하네스 폴더, 그것도 없으면 단위 작업 폴더)이고,
    /// 이미 있는 행의 root·repos 는 이어받는다(root 가 None 이면 저장된 root 가 지워진다)
    fn project_row_for(&self, records: &dyn Records, project: &str, now: f64) -> Result<Row, String> {
        let old = records
            .select("SELECT root, repos FROM projects WHERE id = ?", &[Value::from(project)])
            .map_err(|e| e.to_string())?;
        let old = old.first();
        let old_root = old.and_then(|r| r.get("root")).and_then(Value::as_str).map(str::to_string);
        let mut repos: Vec<String> = old
            .and_then(|r| r.get("repos"))
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default();
        let strings = |v: Option<&Value>| -> Vec<String> {
            v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
        };
        let mut fresh: Vec<String> = self
            .run
            .extra
            .get("branches")
            .and_then(|b| b.get("repos"))
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        fresh.sort();
        if fresh.is_empty() {
            fresh = strings(self.run.extra.get("harness").and_then(|h| h.get("folders")));
        }
        if fresh.is_empty() {
            fresh = (1..=self.queue.len()).filter_map(|n| self.def(n).cwd.get().cloned()).collect();
        }
        for r in fresh {
            if !repos.contains(&r) {
                repos.push(r);
            }
        }
        let root = self.run.project_root.get().cloned().or(old_root);
        Ok(project_row(project, root.as_deref(), &repos, old.is_some(), now))
    }
}
