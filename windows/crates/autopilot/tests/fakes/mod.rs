//! 엔진 루프 시험용 가짜 포트 모음(4.3·5.1 시험도 같이 쓴다).
//! 실제로 기다리지 않는다: `FakeClock::sleep` 은 시각을 밀 뿐이고, `FakeChild::read_line` 은 줄이 없으면 기다리는 만큼 시각을 민다.
//! 시험이 실행 중간에 끼어들 자리는 둘이다 — 시각이 되면 도는 `FakeClock::at`, 줄 차례에 끼우는 `Step::Hook`.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use claude_board_autopilot::engine::run::{frozen_digest, Engine, Ports, RunConfig, RunExit};
use claude_board_autopilot::model::to_pretty_json;
use claude_board_autopilot::ports::*;
use claude_board_autopilot::rules::stopline::Tz;
use serde_json::{json, Value};

pub const NOW: f64 = 1_800_000_000.0;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ── 일지 ──

/// 여러 가짜가 한 줄로 적는 사건 일지 — 차례를 견줄 때 쓴다
#[derive(Default)]
pub struct Journal(Mutex<Vec<String>>);

impl Journal {
    pub fn push(&self, s: impl Into<String>) {
        lock(&self.0).push(s.into());
    }
    pub fn all(&self) -> Vec<String> {
        lock(&self.0).clone()
    }
}

// ── 시계 ──

/// 시각이 이 값에 닿으면 한 번 부르는 훅
type ClockHook = (f64, Box<dyn FnOnce() + Send>);

pub struct FakeClock {
    t: Mutex<f64>,
    hooks: Mutex<Vec<ClockHook>>,
}

impl FakeClock {
    pub fn new(t: f64) -> Arc<Self> {
        Arc::new(FakeClock { t: Mutex::new(t), hooks: Mutex::new(Vec::new()) })
    }
    pub fn get(&self) -> f64 {
        *lock(&self.t)
    }
    /// 시각이 `t` 에 닿으면(밀려 넘어도) 한 번 돈다
    pub fn at(&self, t: f64, f: impl FnOnce() + Send + 'static) {
        lock(&self.hooks).push((t, Box::new(f)));
    }
    pub fn advance(&self, secs: f64) {
        *lock(&self.t) += secs;
        loop {
            let now = self.get();
            let due = {
                let mut h = lock(&self.hooks);
                h.iter().position(|(t, _)| *t <= now).map(|i| h.remove(i))
            };
            match due {
                Some((_, f)) => f(),
                None => break,
            }
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.get()
    }
    fn sleep(&self, d: Duration) {
        self.advance(d.as_secs_f64());
        std::thread::yield_now();
    }
}

// ── 파일 ──

struct FileEntry {
    data: Vec<u8>,
    modified: f64,
}

#[derive(Default)]
struct MemInner {
    files: BTreeMap<String, FileEntry>,
    held: HashSet<String>,
    ops: Vec<String>,
}

/// 메모리 실행 폴더. 경로는 `/`·`\` 를 가리지 않는다. `stat` 의 수정 시각은 가짜 시계다
pub struct MemFiles {
    base: String,
    clock: Arc<FakeClock>,
    journal: Arc<Journal>,
    inner: Mutex<MemInner>,
}

fn key(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

impl MemFiles {
    fn abs(&self, rel: &str) -> String {
        if rel.starts_with('/') {
            rel.to_string()
        } else {
            format!("{}/{rel}", self.base)
        }
    }
    pub fn put_text(&self, rel: &str, text: &str) {
        let modified = self.clock.get();
        lock(&self.inner).files.insert(self.abs(rel), FileEntry { data: text.as_bytes().to_vec(), modified });
    }
    pub fn put_json(&self, rel: &str, v: &Value) {
        self.put_text(rel, &to_pretty_json(v).expect("JSON"));
    }
    pub fn text(&self, rel: &str) -> Option<String> {
        lock(&self.inner).files.get(&self.abs(rel)).map(|f| String::from_utf8_lossy(&f.data).into_owned())
    }
    pub fn json(&self, rel: &str) -> Option<Value> {
        self.text(rel).map(|t| serde_json::from_str(&t).unwrap_or_else(|e| panic!("{rel}: {e}")))
    }
    pub fn lines(&self, rel: &str) -> Vec<String> {
        self.text(rel).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
    }
    pub fn json_lines(&self, rel: &str) -> Vec<Value> {
        self.lines(rel).iter().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{rel}: {l}: {e}"))).collect()
    }
    pub fn exists(&self, rel: &str) -> bool {
        lock(&self.inner).files.contains_key(&self.abs(rel))
    }
    /// 이 이름 아래에 있는 파일 경로(실행 폴더 기준)
    pub fn under(&self, rel: &str) -> Vec<String> {
        let prefix = format!("{}/", self.abs(rel));
        lock(&self.inner).files.keys().filter(|k| k.starts_with(&prefix)).map(|k| k[self.base.len() + 1..].to_string()).collect()
    }
    /// 이 폴더에 잠금을 쥔 다른 엔진이 있는 것처럼 한다
    pub fn hold_lock(&self, rel: &str) {
        lock(&self.inner).held.insert(self.abs(rel));
    }
    /// 쓰기·지우기 기록(만들기 뒤부터)
    pub fn ops(&self) -> Vec<String> {
        lock(&self.inner).ops.clone()
    }
    pub fn clear_ops(&self) {
        lock(&self.inner).ops.clear();
    }
}

struct LockGuard;
impl Held for LockGuard {}

impl RunFiles for MemFiles {
    fn read_json(&self, path: &Path) -> PortResult<Option<Value>> {
        let inner = lock(&self.inner);
        let Some(f) = inner.files.get(&key(path)) else { return Ok(None) };
        serde_json::from_slice(&f.data)
            .map(Some)
            .map_err(|e| PortError::new(PortErrorKind::Invalid, format!("{}: {e}", path.display())))
    }

    fn write_json(&self, path: &Path, value: &Value) -> PortResult<()> {
        let k = key(path);
        let modified = self.clock.get();
        let text = to_pretty_json(value).map_err(|e| PortError::new(PortErrorKind::Invalid, e.to_string()))?;
        {
            let mut inner = lock(&self.inner);
            inner.ops.push(format!("write:{k}"));
            inner.files.insert(k.clone(), FileEntry { data: text.into_bytes(), modified });
        }
        if k.ends_with("/run.json") {
            let s = |n: &str| value.get(n).and_then(Value::as_str).unwrap_or("").to_string();
            let has = |n: &str| value.get(n).is_some_and(|v| !v.is_null());
            self.journal.push(format!(
                "run:{}:{}:{}",
                s("state"),
                if has("ended") { "ended" } else { "-" },
                if has("wrapped") { "wrapped" } else { "-" }
            ));
        }
        Ok(())
    }

    fn append_line(&self, path: &Path, line: &str) -> PortResult<()> {
        let k = key(path);
        let modified = self.clock.get();
        let mut inner = lock(&self.inner);
        inner.ops.push(format!("append:{k}"));
        let f = inner.files.entry(k).or_insert_with(|| FileEntry { data: Vec::new(), modified });
        f.data.extend_from_slice(line.as_bytes());
        f.data.push(b'\n');
        f.modified = modified;
        Ok(())
    }

    fn remove(&self, path: &Path) -> PortResult<()> {
        let k = key(path);
        let mut inner = lock(&self.inner);
        inner.ops.push(format!("remove:{k}"));
        inner.files.remove(&k);
        Ok(())
    }

    fn list(&self, dir: &Path) -> PortResult<Vec<String>> {
        let prefix = format!("{}/", key(dir));
        let inner = lock(&self.inner);
        let mut names: Vec<String> = inner
            .files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|rest| rest.split('/').next().unwrap_or("").to_string()))
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn make_private_dir(&self, dir: &Path) -> PortResult<()> {
        lock(&self.inner).ops.push(format!("mkdir:{}", key(dir)));
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> PortResult<Option<Box<dyn Held>>> {
        if lock(&self.inner).held.contains(&key(path)) {
            Ok(None)
        } else {
            Ok(Some(Box::new(LockGuard)))
        }
    }

    fn stat(&self, path: &Path) -> PortResult<Option<FileStat>> {
        Ok(lock(&self.inner).files.get(&key(path)).map(|f| FileStat { len: f.data.len() as u64, modified: f.modified }))
    }

    fn read_from(&self, path: &Path, offset: u64, max: usize) -> PortResult<Option<Vec<u8>>> {
        let inner = lock(&self.inner);
        let Some(f) = inner.files.get(&key(path)) else { return Ok(None) };
        let len = f.data.len();
        let start = (offset as usize).min(len).max(len.saturating_sub(max));
        Ok(Some(f.data[start..].to_vec()))
    }
}

// ── 실행 환경 ──

pub enum Step {
    /// stdout 한 줄. `{SID}` 는 띄울 때 정한 세션 id 로 바뀐다
    Line(String),
    /// 줄이 없는 채로 이만큼(초) 지난다
    Wait(f64),
    /// 이 차례에 한 번 도는 일(예: control.json 쓰기)
    Hook(Box<dyn FnOnce() + Send>),
    /// 프로세스가 이 차례에 끝난다(남은 줄은 마저 읽힌다)
    Exit(i32),
}

pub struct ChildScript {
    pub steps: Vec<Step>,
    /// 입력을 닫으면 이 코드로 끝난다(None 이면 안 끝난다)
    pub on_close: Option<i32>,
    /// 점잖게 멈추라고 하면 이 코드로 끝난다(None 이면 안 끝난다)
    pub on_interrupt: Option<i32>,
    /// 점잖게 멈추라고 하면 이 줄들이 곧바로 나온다(진짜 claude 는 `result` 로 답하고 입력을 기다리며 산다)
    pub lines_on_interrupt: Vec<String>,
    /// 띄울 때 stderr.log 에 덧붙일 글
    pub stderr: String,
}

impl ChildScript {
    pub fn new(steps: Vec<Step>) -> Self {
        ChildScript { steps, on_close: Some(0), on_interrupt: Some(130), lines_on_interrupt: Vec::new(), stderr: String::new() }
    }
    /// 세션을 열고 한마디 한 뒤 결과를 보내는 정상 턴
    pub fn ok_turn(cost: f64) -> Self {
        Self::new(vec![Step::Line(init_line()), Step::Line(assistant_text("일합니다")), Step::Line(result_line(cost))])
    }
}

pub fn init_line() -> String {
    json!({"type": "system", "subtype": "init", "session_id": "{SID}"}).to_string()
}

pub fn assistant_text(text: &str) -> String {
    json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}}).to_string()
}

pub fn assistant_tool(id: &str, name: &str, input: Value) -> String {
    json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
        .to_string()
}

pub fn tool_result(id: &str, text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": text}]}}).to_string()
}

pub fn result_line(cost: f64) -> String {
    json!({"type": "result", "subtype": "success", "is_error": false, "num_turns": 1, "result": "ok", "total_cost_usd": cost, "session_id": "{SID}"})
        .to_string()
}

pub struct CheckScript {
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub tail: String,
    /// 결과를 돌려주기 전에 도는 일(옆 스레드에서)
    pub hook: Option<Box<dyn FnOnce() + Send>>,
    /// Some 이면 명령을 돌리지 못한 오류
    pub err: Option<String>,
}

impl CheckScript {
    pub fn exit(code: i32, tail: &str) -> Self {
        CheckScript { exit: Some(code), timed_out: false, tail: tail.to_string(), hook: None, err: None }
    }
    pub fn pass() -> Self {
        Self::exit(0, "")
    }
    pub fn timed_out() -> Self {
        CheckScript { exit: None, timed_out: true, tail: "끝나지 않음".to_string(), hook: None, err: None }
    }
    pub fn with_hook(mut self, f: impl FnOnce() + Send + 'static) -> Self {
        self.hook = Some(Box::new(f));
        self
    }
}

struct ChildState {
    steps: VecDeque<Step>,
    exited: Option<i32>,
    on_close: Option<i32>,
    on_interrupt: Option<i32>,
    lines_on_interrupt: Vec<String>,
    sid: String,
}

pub struct FakeChild {
    state: Arc<Mutex<ChildState>>,
    clock: Arc<FakeClock>,
    pid: u32,
    sent: Arc<Mutex<Vec<String>>>,
    events: Arc<Mutex<Vec<String>>>,
}

impl ChildProcess for FakeChild {
    fn pid(&self) -> u32 {
        self.pid
    }
    fn write_line(&mut self, line: &str) -> PortResult<()> {
        lock(&self.sent).push(line.to_string());
        Ok(())
    }
    fn close_stdin(&mut self) {
        lock(&self.events).push("close_stdin".into());
        let mut s = lock(&self.state);
        if s.exited.is_none() {
            s.exited = s.on_close;
        }
    }
    fn read_line(&mut self, timeout: Duration) -> PortResult<Option<String>> {
        loop {
            let step = lock(&self.state).steps.pop_front();
            match step {
                Some(Step::Line(l)) => {
                    let sid = lock(&self.state).sid.clone();
                    return Ok(Some(l.replace("{SID}", &sid)));
                }
                Some(Step::Wait(secs)) => {
                    self.clock.advance(secs);
                    return Err(PortError::new(PortErrorKind::Timeout, "줄 없음"));
                }
                Some(Step::Hook(f)) => f(),
                Some(Step::Exit(code)) => lock(&self.state).exited = Some(code),
                None => {
                    if lock(&self.state).exited.is_some() {
                        return Ok(None);
                    }
                    self.clock.advance(timeout.as_secs_f64());
                    return Err(PortError::new(PortErrorKind::Timeout, "줄 없음"));
                }
            }
        }
    }
    fn try_wait(&mut self) -> PortResult<Option<i32>> {
        Ok(lock(&self.state).exited)
    }
    fn interrupt(&mut self) -> PortResult<()> {
        lock(&self.events).push("interrupt".into());
        let mut s = lock(&self.state);
        for line in std::mem::take(&mut s.lines_on_interrupt).into_iter().rev() {
            s.steps.push_front(Step::Line(line));
        }
        if s.exited.is_none() {
            s.exited = s.on_interrupt;
        }
        Ok(())
    }
    fn kill_tree(&mut self) -> PortResult<()> {
        lock(&self.events).push("kill_tree".into());
        let mut s = lock(&self.state);
        if s.exited.is_none() {
            s.exited = Some(137);
        }
        Ok(())
    }
}

/// 띄울 때마다·판정마다 미리 정한 차례대로 답하는 실행 환경
pub struct ScriptedEnv {
    files: Arc<MemFiles>,
    clock: Arc<FakeClock>,
    spawns: Mutex<VecDeque<Result<ChildScript, String>>>,
    checks: Mutex<VecDeque<CheckScript>>,
    pub launches: Mutex<Vec<ClaudeLaunch>>,
    pub policies: Mutex<Vec<SessionPolicy>>,
    pub check_runs: Mutex<Vec<CheckRun>>,
    /// 띄운 순서대로, 그 claude 에 쓴 줄
    pub sent: Mutex<Vec<Arc<Mutex<Vec<String>>>>>,
    /// 띄운 순서대로, 그 claude 에 한 일(close_stdin·interrupt·kill_tree)
    pub child_events: Mutex<Vec<Arc<Mutex<Vec<String>>>>>,
    choice: EnvChoice,
}

impl ScriptedEnv {
    pub fn push_spawn(&self, s: ChildScript) {
        lock(&self.spawns).push_back(Ok(s));
    }
    pub fn push_spawn_error(&self, why: &str) {
        lock(&self.spawns).push_back(Err(why.to_string()));
    }
    pub fn push_check(&self, c: CheckScript) {
        lock(&self.checks).push_back(c);
    }
    pub fn launches(&self) -> Vec<ClaudeLaunch> {
        lock(&self.launches).clone()
    }
    pub fn check_runs(&self) -> Vec<CheckRun> {
        lock(&self.check_runs).clone()
    }
    /// n 번째(0부터) 띄운 claude 에 쓴 줄
    pub fn sent(&self, n: usize) -> Vec<String> {
        lock(&lock(&self.sent)[n]).clone()
    }
    pub fn child_events(&self, n: usize) -> Vec<String> {
        lock(&lock(&self.child_events)[n]).clone()
    }
    pub fn spawn_count(&self) -> usize {
        lock(&self.launches).len()
    }
}

impl ExecEnv for ScriptedEnv {
    fn choice(&self) -> &EnvChoice {
        &self.choice
    }
    fn check_ready(&self, _cwd: &Path) -> EnvReport {
        EnvReport { ready: true, sandbox: false, claude_version: Some("test".into()), problems: Vec::new() }
    }
    fn to_env_path(&self, path: &Path) -> PortResult<String> {
        Ok(path.to_string_lossy().into_owned())
    }
    fn env_path_to_windows(&self, path: &str) -> PortResult<PathBuf> {
        Ok(PathBuf::from(path))
    }
    fn render_settings(&self, policy: &SessionPolicy) -> PortResult<Value> {
        lock(&self.policies).push(policy.clone());
        Ok(json!({"scripted": true, "allow_write": policy.allow_write.iter().map(|p| key(p)).collect::<Vec<_>>()}))
    }
    fn spawn_claude(&self, launch: &ClaudeLaunch) -> PortResult<Box<dyn ChildProcess>> {
        lock(&self.launches).push(launch.clone());
        let script = match lock(&self.spawns).pop_front() {
            Some(Ok(s)) => s,
            Some(Err(why)) => return Err(PortError::new(PortErrorKind::Unavailable, why)),
            None => panic!("띄울 claude 대본이 없다(시험이 대본을 덜 넣었다)"),
        };
        if !script.stderr.is_empty() {
            self.files.append_line(&launch.stderr_log, &script.stderr)?;
        }
        let sid = match &launch.session {
            SessionStart::New(s) | SessionStart::Resume(s) => s.clone(),
        };
        let state = Arc::new(Mutex::new(ChildState {
            steps: script.steps.into(),
            exited: None,
            on_close: script.on_close,
            on_interrupt: script.on_interrupt,
            lines_on_interrupt: script.lines_on_interrupt,
            sid,
        }));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        lock(&self.sent).push(sent.clone());
        lock(&self.child_events).push(events.clone());
        let pid = 4000 + lock(&self.launches).len() as u32;
        Ok(Box::new(FakeChild { state, clock: self.clock.clone(), pid, sent, events }))
    }
    fn run_check(&self, run: &CheckRun) -> PortResult<CheckOutcome> {
        lock(&self.check_runs).push(run.clone());
        let script = lock(&self.checks).pop_front().unwrap_or_else(CheckScript::pass);
        if let Some(h) = script.hook {
            h();
        }
        if let Some(why) = script.err {
            return Err(PortError::new(PortErrorKind::Io, why));
        }
        Ok(CheckOutcome { exit: script.exit, timed_out: script.timed_out, seconds: 1.5, tail: script.tail })
    }
}

// ── git ──

pub struct FakeGit {
    journal: Arc<Journal>,
    pub calls: Mutex<Vec<(String, String)>>,
    replies: Mutex<Vec<(String, GitOutput)>>,
}

impl FakeGit {
    /// 인자를 공백으로 이은 글이 같은 호출에 답한다. 정하지 않은 호출은 성공(빈 출력)
    pub fn reply(&self, args: &str, code: i32, stdout: &str) {
        lock(&self.replies).push((args.to_string(), GitOutput { code, stdout: stdout.to_string(), stderr: String::new() }));
    }
    pub fn args(&self) -> Vec<String> {
        lock(&self.calls).iter().map(|(_, a)| a.clone()).collect()
    }
}

impl Git for FakeGit {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        let a = args.join(" ");
        lock(&self.calls).push((repo.to_string_lossy().into_owned(), a.clone()));
        self.journal.push(format!("git:{a}"));
        let found = lock(&self.replies).iter().find(|(k, _)| *k == a).map(|(_, o)| o.clone());
        Ok(found.unwrap_or(GitOutput { code: 0, stdout: String::new(), stderr: String::new() }))
    }
}

// ── 기록 저장소 ──

pub struct MemRecords {
    journal: Arc<Journal>,
    pub upserts: Mutex<Vec<(String, Row)>>,
    fail: Mutex<Option<String>>,
    /// `select` 가 돌려줄 행(projects 에 이미 있는 행 흉내)
    pub existing: Mutex<Vec<Row>>,
}

impl MemRecords {
    pub fn set_fail(&self, why: Option<&str>) {
        *lock(&self.fail) = why.map(str::to_string);
    }
    pub fn rows(&self, table: &str) -> Vec<Row> {
        lock(&self.upserts).iter().filter(|(t, _)| t == table).map(|(_, r)| r.clone()).collect()
    }
}

impl Records for MemRecords {
    fn version(&self) -> PortResult<i32> {
        Ok(4)
    }
    fn upsert(&self, table: &str, row: &Row) -> PortResult<()> {
        if let Some(why) = lock(&self.fail).clone() {
            return Err(PortError::new(PortErrorKind::Busy, why));
        }
        self.journal.push(format!("records:{table}"));
        lock(&self.upserts).push((table.to_string(), row.clone()));
        Ok(())
    }
    fn select(&self, sql: &str, args: &[Value]) -> PortResult<Vec<Row>> {
        let all = lock(&self.existing).clone();
        // 프로젝트 행을 묻는 두 질의만 흉내 낸다: `id = ?`(이 프로젝트의 행) 와 `id <> ?`(다른 프로젝트의 행)
        let id_of = |r: &Row| r.get("id").and_then(Value::as_str).map(str::to_string);
        let arg = args.first().and_then(Value::as_str).map(str::to_string);
        Ok(match (sql.contains("id <> ?"), sql.contains("id = ?")) {
            (true, _) => all.into_iter().filter(|r| id_of(r) != arg).collect(),
            (_, true) => all.into_iter().filter(|r| id_of(r) == arg).collect(),
            _ => all,
        })
    }
}

// ── 사용률 · 작업 스케줄러 ──

pub struct FakeUsage {
    /// (5시간, 주간). None 이면 알 수 없음
    snap: Mutex<Option<(f64, f64)>>,
    pub calls: AtomicU32,
}

impl FakeUsage {
    pub fn set(&self, s: Option<(f64, f64)>) {
        *lock(&self.snap) = s;
    }
    pub fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Usage for FakeUsage {
    fn fallback(&self, now: f64) -> PortResult<Option<UsageSnapshot>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(lock(&self.snap).map(|(five, seven)| UsageSnapshot {
            five_hour: five,
            five_hour_resets: Some(now + 10_000.0),
            seven_day: seven,
            seven_day_resets: Some(now + 100_000.0),
            at: now,
            from: UsageFrom::Cache,
        }))
    }
}

pub struct FakeSupervisor {
    journal: Arc<Journal>,
    pub unregistered: Mutex<Vec<String>>,
}

impl Supervisor for FakeSupervisor {
    fn register(&self, _job: &KeepAlive) -> PortResult<()> {
        Ok(())
    }
    fn unregister(&self, label: &str) -> PortResult<()> {
        self.journal.push(format!("unregister:{label}"));
        lock(&self.unregistered).push(label.to_string());
        Ok(())
    }
    fn is_registered(&self, _label: &str) -> PortResult<bool> {
        Ok(true)
    }
    fn list(&self) -> PortResult<Vec<String>> {
        Ok(Vec::new())
    }
}

// ── 한 세계 ──

/// 끝내라고 부른 프로세스 하나: (pid, 생성 시각)
pub type ReapedPid = (u32, Option<f64>);

/// 시험 한 건이 쓰는 가짜 전부와 실행 폴더 내용. `Scenario::build` 로 만든다
pub struct World {
    pub clock: Arc<FakeClock>,
    pub files: Arc<MemFiles>,
    pub env: Arc<ScriptedEnv>,
    pub git: FakeGit,
    pub records: Arc<MemRecords>,
    pub usage: Arc<FakeUsage>,
    pub supervisor: FakeSupervisor,
    pub journal: Arc<Journal>,
    pub reaped: Arc<Mutex<Vec<ReapedPid>>>,
    reap: Box<dyn Fn(u32, Option<f64>) -> bool + Send + Sync>,
    pub dir: PathBuf,
    pub private: PathBuf,
    pub frozen_hash: String,
    pub tick: Duration,
    pub pid: u32,
}

impl World {
    pub fn cfg(&self) -> RunConfig {
        RunConfig {
            dir: self.dir.clone(),
            private_dir: self.private.clone(),
            frozen_hash: self.frozen_hash.clone(),
            tz: Tz::Utc,
            tick: self.tick,
            poll: Duration::from_millis(200),
            claude_env_extra: Vec::new(),
            pid: self.pid,
            exe: "claude-autopilot-engine.exe".to_string(),
        }
    }

    pub fn ports(&self, with_records: bool) -> Ports<'_> {
        Ports {
            files: &*self.files,
            env: &*self.env,
            git: &self.git,
            records: with_records.then_some(&*self.records as &dyn Records),
            usage: &*self.usage,
            clock: &*self.clock,
            supervisor: &self.supervisor,
            reap: &*self.reap,
        }
    }

    pub fn open(&self) -> Result<Engine<'_>, RunExit> {
        Engine::open(self.ports(true), self.cfg())
    }

    pub fn open_without_records(&self) -> Result<Engine<'_>, RunExit> {
        Engine::open(self.ports(false), self.cfg())
    }

    /// 가짜 기록 저장소 대신 `records`(진짜 SQLite 어댑터 등)를 끼워 끝까지 돌린다
    pub fn run_with(&self, records: &dyn Records) -> RunExit {
        let mut ports = self.ports(false);
        ports.records = Some(records);
        match Engine::open(ports, self.cfg()) {
            Ok(e) => e.run(),
            Err(x) => x,
        }
    }

    /// 열어서 끝까지 돌린다(기록 저장소 있음)
    pub fn run(&self) -> RunExit {
        match self.open() {
            Ok(e) => e.run(),
            Err(x) => x,
        }
    }

    /// events.jsonl 의 (who, text)
    pub fn events(&self) -> Vec<(String, String)> {
        self.files
            .json_lines("events.jsonl")
            .iter()
            .map(|v| (v["who"].as_str().unwrap_or("").to_string(), v["text"].as_str().unwrap_or("").to_string()))
            .collect()
    }

    pub fn event_texts(&self) -> Vec<String> {
        self.events().into_iter().map(|(_, t)| t).collect()
    }

    /// 어느 한 줄에 `part` 가 들어 있나
    pub fn has_event(&self, part: &str) -> bool {
        self.event_texts().iter().any(|t| t.contains(part))
    }

    pub fn run_json(&self) -> Value {
        self.files.json("run.json").expect("run.json")
    }

    pub fn queue_json(&self) -> Value {
        self.files.json("queue.json").expect("queue.json")
    }

    pub fn unit_json(&self, n: usize) -> Value {
        self.files.json(&format!("units/{n}/engine.json")).unwrap_or_else(|| panic!("units/{n}/engine.json"))
    }

    pub fn queue_statuses(&self) -> Vec<String> {
        self.queue_json().as_array().unwrap().iter().map(|q| q["status"].as_str().unwrap_or("").to_string()).collect()
    }

    /// 시각이 `t` 가 되면 control.json 을 쓴다. ts 를 안 주면 파일 수정 시각을 쓰게 된다
    pub fn control_at(&self, t: f64, action: &str, ts: Option<f64>) {
        let files = self.files.clone();
        let action = action.to_string();
        self.clock.at(t, move || {
            let body = match ts {
                Some(ts) => json!({"action": action, "ts": ts}),
                None => json!({"action": action}),
            };
            files.put_json("control.json", &body);
        });
    }

    /// 판정 안에서 control.json 을 쓰고, 엔진이 가져가(지워)야 판정이 끝나는 대본 조각
    pub fn control_during_check(&self, action: &str) -> impl FnOnce() + Send + 'static {
        let files = self.files.clone();
        let action = action.to_string();
        move || {
            files.put_json("control.json", &json!({"action": action, "ts": NOW + 10.0}));
            let start = std::time::Instant::now();
            while files.exists("control.json") {
                assert!(start.elapsed() < Duration::from_secs(10), "엔진이 판정 중에 control.json 을 가져가지 않았다");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// 실행 폴더에 처음 놓일 파일 셋. 필드를 고친 뒤 `build`
pub struct Scenario {
    pub run: Value,
    pub queue: Value,
    pub frozen: Value,
    pub extra_files: Vec<(String, String)>,
    /// 실행 폴더 이름 = 실행 id (기본 `r1`)
    pub run_id: String,
}

impl Scenario {
    /// 단위 `n` 개(첫째·둘째 …, 판정 `true`), 방금 읽은 낮은 사용률, 작업 스케줄러 이름 `lbl-r1`
    pub fn new(n: usize) -> Self {
        let units: Vec<Value> = (1..=n)
            .map(|i| {
                let title = ["첫째", "둘째", "셋째"].get(i - 1).copied().unwrap_or("넷째");
                json!({
                    "id": format!("u{i}"),
                    "title": title,
                    "prompt": format!("p{i}"),
                    "cwd": "C:\\work",
                    "check": "true",
                })
            })
            .collect();
        Scenario {
            run: json!({
                "state": "starting",
                "launchd_label": "lbl-r1",
                "usage": {"five_hour": 0.1, "five_hour_resets": NOW + 10_000.0, "seven_day": 0.1, "seven_day_resets": NOW + 100_000.0, "at": NOW, "from": "stream"},
            }),
            queue: Value::Array((1..=n).map(|i| json!({"id": format!("u{i}"), "status": "pending"})).collect()),
            frozen: json!({
                "id": "r1",
                "title": "시험 실행",
                "model": "m1",
                "caps": {"minutes": 120, "usd": 100, "weekly_pp": 5},
                "check_retries": 2,
                "precheck_retry_min": 5,
                "stopline": {"seven_day": 0.8, "five_hour": 0.95},
                "write_roots": [],
                "units": units,
            }),
            extra_files: Vec::new(),
            run_id: "r1".to_string(),
        }
    }

    /// 단위 i(1부터) 정의에 칸을 더한다
    pub fn unit(&mut self, i: usize, key: &str, v: Value) {
        self.frozen["units"][i - 1][key] = v;
    }

    pub fn file(&mut self, rel: &str, text: &str) {
        self.extra_files.push((rel.to_string(), text.to_string()));
    }

    pub fn build(self) -> World {
        let journal = Arc::new(Journal::default());
        let clock = FakeClock::new(NOW);
        let base = format!("/runs/{}", self.run_id);
        let files = Arc::new(MemFiles {
            base: base.clone(),
            clock: clock.clone(),
            journal: journal.clone(),
            inner: Mutex::new(MemInner::default()),
        });
        files.put_json("run.json", &self.run);
        files.put_json("queue.json", &self.queue);
        files.put_json("pinned/frozen.json", &self.frozen);
        for (rel, text) in &self.extra_files {
            files.put_text(rel, text);
        }
        files.clear_ops();
        let frozen_hash = frozen_digest(&serde_json::from_str::<Value>(&files.text("pinned/frozen.json").unwrap()).unwrap());
        let env = Arc::new(ScriptedEnv {
            files: files.clone(),
            clock: clock.clone(),
            spawns: Mutex::new(VecDeque::new()),
            checks: Mutex::new(VecDeque::new()),
            launches: Mutex::new(Vec::new()),
            policies: Mutex::new(Vec::new()),
            check_runs: Mutex::new(Vec::new()),
            sent: Mutex::new(Vec::new()),
            child_events: Mutex::new(Vec::new()),
            choice: EnvChoice { kind: EnvKind::Pwsh, distro: None },
        });
        let reaped = Arc::new(Mutex::new(Vec::new()));
        let reap_log = reaped.clone();
        World {
            clock,
            files,
            env,
            git: FakeGit { journal: journal.clone(), calls: Mutex::new(Vec::new()), replies: Mutex::new(Vec::new()) },
            records: Arc::new(MemRecords {
                journal: journal.clone(),
                upserts: Mutex::new(Vec::new()),
                fail: Mutex::new(None),
                existing: Mutex::new(Vec::new()),
            }),
            usage: Arc::new(FakeUsage { snap: Mutex::new(Some((0.1, 0.1))), calls: AtomicU32::new(0) }),
            supervisor: FakeSupervisor { journal: journal.clone(), unregistered: Mutex::new(Vec::new()) },
            journal,
            reaped,
            reap: Box::new(move |pid, ctime| {
                lock(&reap_log).push((pid, ctime));
                true
            }),
            dir: PathBuf::from(base),
            private: PathBuf::from(format!("/private/{}", self.run_id)),
            frozen_hash,
            tick: Duration::from_secs(1),
            pid: 77,
        }
    }
}
