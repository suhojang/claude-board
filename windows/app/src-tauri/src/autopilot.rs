//! 자율주행 실행 읽기 — 보드가 1초마다 실행 폴더(`runs\<id>\`)를 훑어 스냅샷의 `autopilot` 목록을 만들고,
//! 사람이 누른 일시정지·재개·중단을 `control.json` 으로 남긴다.
//!
//! 보드는 읽기와 `control.json` 쓰기만 한다. 실행 상태의 주인은 엔진이다.
//! 읽는 파일은 `run.json`·`queue.json`·지금 단위의 `units\<n>\engine.json` 셋뿐이다(`events.jsonl`·`stream.jsonl` 은 크고 화면에 없다).
//! 파일마다 `(수정 시각, 길이)` 가 같으면 다시 읽지 않고, 엔진이 쓰는 중이라 JSON 이 깨져 있으면 앞 값을 그대로 둔다.
//!
//! 엔진이 살아 있는지는 보지 않는다. 대기·일시정지 동안 엔진은 run.json 을 오래 쓰지 않으므로 `updated` 로는
//! 죽음과 대기를 가르지 못한다(작업 스케줄러가 1분 안에 다시 띄운다).

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use claude_board_autopilot::cli::valid_run_id;
use claude_board_autopilot::model::{from_slice, to_pretty_json, ControlFile, ControlRequest, Num, Ordered, QueueFile, QueueUnit, Run, RunFile, Slot, UnitProgress, UnitProgressFile};
use claude_board_autopilot::rules::control::{board_can_control, run_ended};
use claude_board_core::envs::{EnvChoice, EnvEntry, EnvKind};
use claude_board_core::{clock, fileio};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::board::Board;
use crate::util::{board_log, ms};

type B<'a> = State<'a, Arc<Board>>;

/// 끝난 실행을 목록에 두는 시간(초). 이보다 오래 지나면 뺀다(파일은 그대로).
const ENDED_KEEP_SEC: f64 = 600.0;
/// 보드가 쓸 수 있는 요청 셋 — 순서가 `can` 의 순서다.
const BOARD_ACTIONS: [&str; 3] = ["pause", "resume", "stop"];

/// 이 상태에서 이 요청이 뜻이 있는가 — 규칙(`board_can_control`)은 일시정지 중의 일시정지와 도는 중의 재개도 받아 주므로
/// (엔진이 "이미 일시정지"·"일시정지 상태가 아니라 그대로"라고 넘긴다) 단추로는 일시정지 중이 아닐 때 일시정지, 일시정지 중일 때만 재개를 낸다.
fn can_send(action: &str, state: &str) -> bool {
    let paused = state == "paused";
    board_can_control(action, state) && match action {
        "pause" => !paused,
        "resume" => paused,
        _ => true,
    }
}

// ── 뷰 (스냅샷 `autopilot`) ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutopilotRunView {
    pub id: String,
    /// 카드 짝짓기용 경로 — run.json `project_root`, 없으면 첫 단위 cwd. Windows 경로 글 그대로
    pub path: String,
    pub title: String,
    /// run.json state 글 그대로
    pub state: String,
    pub reason: String,
    /// 대기 끝 시각(ms)
    pub wait_until: Option<i64>,
    pub env: AutopilotEnvView,
    pub unit: Option<AutopilotUnitView>,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub capped: usize,
    pub skipped: usize,
    pub stopped: usize,
    /// pause·resume·stop 가운데 지금 상태에서 보드가 보낼 수 있는 것(`can_send`)
    pub can: Vec<String>,
    pub ended: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutopilotEnvView {
    pub key: String,
    pub label: String,
    pub sandbox: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutopilotUnitView {
    pub n: usize,
    pub title: String,
    pub phase: String,
    pub active_sec: f64,
    /// 이번 단위의 비용. 실행 전체 합계는 끝난 단위 파일을 다 읽어야 해서 싣지 않는다
    pub cost_usd: f64,
    pub tries: i64,
}

// ── 파일 캐시 ──────────────────────────────────────

/// 파일이 바뀌었는지 보는 표지 — 수정 시각과 길이
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp { modified: meta.modified().ok(), len: meta.len() })
}

fn unix_secs(time: Option<SystemTime>) -> f64 {
    time.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64())
}

/// 파일 하나의 읽은 값. 표지가 같으면 다시 읽지 않는다.
#[derive(Debug)]
struct Cached<T> {
    stamp: Option<Stamp>,
    value: Option<T>,
}

impl<T> Default for Cached<T> {
    fn default() -> Self {
        Self { stamp: None, value: None }
    }
}

impl<T: DeserializeOwned> Cached<T> {
    /// 파일을 본다. 없으면 값을 비우고, 바뀌었으면 읽는다(`reads` 를 하나 센다).
    /// 읽다가 JSON 이 깨졌으면 앞 값과 앞 표지를 둔다 — 다음 틱에 다시 읽는다.
    fn refresh(&mut self, path: &Path, reads: &mut usize) {
        let Some(now_stamp) = stamp_of(path) else {
            *self = Self::default();
            return;
        };
        if self.stamp == Some(now_stamp) {
            return;
        }
        *reads += 1;
        if let Some(value) = fs::read(path).ok().and_then(|bytes| from_slice::<T>(&bytes).ok()) {
            self.value = Some(value);
            self.stamp = Some(now_stamp);
        }
    }
}

/// 실행 폴더 하나의 캐시
#[derive(Default)]
struct RunCache {
    run: Cached<RunFile>,
    queue: Cached<QueueFile>,
    /// (단위 번호, engine.json). 단위가 바뀌면 새로 읽는다
    unit: (u32, Cached<UnitProgressFile>),
}

impl RunCache {
    fn run_state(&self) -> &str {
        self.run.value.as_ref().and_then(|r| r.state.get()).map_or("", String::as_str)
    }

    /// 목록에서 뺄 만큼 오래전에 끝났는가
    fn retired(&self, now: f64) -> bool {
        let Some(run) = self.run.value.as_ref() else { return false };
        if !run_ended(self.run_state()) {
            return false;
        }
        let modified = unix_secs(self.run.stamp.and_then(|s| s.modified));
        let ended = run.ended.get().and_then(Num::as_f64).unwrap_or(modified);
        now - ended > ENDED_KEEP_SEC
    }

    fn current_unit(&self) -> Option<u32> {
        self.run.value.as_ref()?.unit.get().copied().filter(|n| *n > 0)
    }

    fn refresh(&mut self, dir: &Path, now: f64, reads: &mut usize) {
        let run_path = dir.join("run.json");
        let before = self.run.stamp;
        self.run.refresh(&run_path, reads);
        // 오래전에 끝난 실행은 run.json 이 그대로인 동안 다른 파일을 보지 않는다
        if self.retired(now) && before == self.run.stamp {
            return;
        }
        self.queue.refresh(&dir.join("queue.json"), reads);
        match self.current_unit() {
            Some(n) => {
                if self.unit.0 != n {
                    self.unit = (n, Cached::default());
                }
                self.unit.1.refresh(&dir.join("units").join(n.to_string()).join("engine.json"), reads);
            }
            None => self.unit = (0, Cached::default()),
        }
    }
}

// ── 읽기 ──────────────────────────────────────────

pub struct AutopilotReader {
    runs_dir: PathBuf,
    caches: HashMap<String, RunCache>,
    /// 파일을 실제로 읽어 해석한 횟수 — 캐시 시험이 센다
    reads: usize,
}

impl AutopilotReader {
    /// `root` 는 `claude_board_core::paths::autopilot_root(false)`. 실행 폴더는 그 아래 `runs\<id>\`
    pub fn new(root: PathBuf) -> Self {
        Self { runs_dir: root.join("runs"), caches: HashMap::new(), reads: 0 }
    }

    /// 1초마다. `runs\` 아래 폴더를 훑고, 파일이 바뀐 것만 다시 읽는다. `now` 는 유닉스 초.
    pub fn refresh(&mut self, now: f64) {
        let ids = run_folder_ids(&self.runs_dir);
        self.caches.retain(|id, _| ids.contains(id));
        for id in ids {
            let dir = self.runs_dir.join(&id);
            self.caches.entry(id).or_default().refresh(&dir, now, &mut self.reads);
        }
    }

    /// 목록에 보일 실행들 — 새 실행이 앞에 온다.
    pub fn view(&self, now: f64) -> Vec<AutopilotRunView> {
        let mut shown: Vec<(f64, AutopilotRunView)> = self
            .caches
            .iter()
            .filter(|(_, cache)| !cache.run_state().is_empty() && !cache.retired(now))
            .filter_map(|(id, cache)| {
                let created = cache.run.value.as_ref()?.created.get().and_then(Num::as_f64).unwrap_or(0.0);
                Some((created, run_view(id, cache)?))
            })
            .collect();
        shown.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        shown.into_iter().map(|(_, view)| view).collect()
    }

    /// 읽은 실행의 폴더. 캐시에 있는 id 만 답한다(경로 조각이 섞인 id 를 걸러낸다).
    pub fn run_dir(&self, run_id: &str) -> Option<PathBuf> {
        self.caches.contains_key(run_id).then(|| self.runs_dir.join(run_id))
    }

    /// 지금 run.json 의 state. 아직 출발 준비 중이면(state 가 비면) None
    pub fn state_of(&self, run_id: &str) -> Option<String> {
        let state = self.caches.get(run_id)?.run_state();
        (!state.is_empty()).then(|| state.to_string())
    }

    /// 요청을 써도 되는지 본다 — 아는 요청이고 지금 상태에서 보낼 수 있으면 `control.json` 을 둘 실행 폴더.
    pub fn control_target(&self, run_id: &str, action: &str) -> Result<PathBuf, String> {
        if !BOARD_ACTIONS.contains(&action) {
            return Err("모르는 요청".to_string());
        }
        let state = self.state_of(run_id).ok_or("모르는 실행")?;
        if !can_send(action, &state) {
            return Err("지금 상태에서는 할 수 없습니다".to_string());
        }
        self.run_dir(run_id).ok_or_else(|| "모르는 실행".to_string())
    }
}

/// `runs\` 아래 폴더 이름들. 폴더가 없으면 빈 목록
fn run_folder_ids(runs_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(runs_dir) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

// ── 뷰 만들기 (순수) ────────────────────────────────

fn count_status(queue: &[Ordered<QueueUnit>], status: &str) -> usize {
    queue.iter().filter(|u| u.status.get().is_some_and(|s| s == status)).count()
}

fn text(slot: &Slot<String>) -> String {
    slot.get().cloned().unwrap_or_default()
}

/// 실행 환경 이름 — WSL 은 "WSL · 배포판". 키를 못 알아보면 kind, 그것도 못 알아보면 키 그대로
fn env_label(key: &str, kind: &str) -> String {
    match EnvChoice::parse(key) {
        Some(EnvChoice { kind: EnvKind::Wsl, distro: Some(distro) }) => format!("{} · {distro}", EnvKind::Wsl.label()),
        Some(choice) => choice.kind.label().to_string(),
        None => EnvKind::from_id(kind).map_or_else(|| key.to_string(), |k| k.label().to_string()),
    }
}

fn env_view(run: &Run) -> AutopilotEnvView {
    match run.exec_env.get() {
        Some(env) => AutopilotEnvView { key: env.key.clone(), label: env_label(&env.key, &env.kind), sandbox: env.sandbox.unwrap_or(false) },
        None => AutopilotEnvView { key: String::new(), label: "알 수 없음".to_string(), sandbox: false },
    }
}

fn unit_view(cache: &RunCache, queue: &[Ordered<QueueUnit>]) -> Option<AutopilotUnitView> {
    let n = cache.current_unit()?;
    let progress = cache.unit.1.value.as_ref();
    let number = |pick: fn(&UnitProgress) -> &Slot<Num>| -> f64 {
        progress.and_then(|p| pick(p).get()).and_then(Num::as_f64).unwrap_or(0.0)
    };
    let title = progress
        .map(|p| text(&p.title))
        .filter(|t| !t.is_empty())
        .or_else(|| queue.get(n as usize - 1).map(|u| text(&u.title)))
        .unwrap_or_default();
    Some(AutopilotUnitView {
        n: n as usize,
        title,
        phase: progress.map(|p| text(&p.phase)).unwrap_or_default(),
        active_sec: number(|p| &p.active_sec),
        cost_usd: number(|p| &p.cost_usd),
        tries: number(|p| &p.r#try) as i64,
    })
}

/// 카드 짝짓기용 경로 — project_root, 없으면 첫 단위 cwd
fn run_path(run: &Run, queue: &[Ordered<QueueUnit>]) -> String {
    Some(text(&run.project_root))
        .filter(|p| !p.is_empty())
        .or_else(|| queue.first().map(|u| text(&u.cwd)).filter(|p| !p.is_empty()))
        .unwrap_or_default()
}

fn run_view(id: &str, cache: &RunCache) -> Option<AutopilotRunView> {
    let run = cache.run.value.as_ref()?;
    let queue: &[Ordered<QueueUnit>] = cache.queue.value.as_deref().unwrap_or(&[]);
    let state = text(&run.state);
    let title = Some(text(&run.title)).filter(|t| !t.is_empty()).or_else(|| Some(text(&run.project)).filter(|p| !p.is_empty())).unwrap_or_else(|| id.to_string());
    Some(AutopilotRunView {
        id: id.to_string(),
        path: run_path(run, queue),
        title,
        reason: text(&run.reason),
        wait_until: run.wait_until.get().and_then(Num::as_f64).map(ms),
        env: env_view(run),
        unit: unit_view(cache, queue),
        total: queue.len(),
        done: count_status(queue, "done"),
        failed: count_status(queue, "failed"),
        capped: count_status(queue, "capped"),
        skipped: count_status(queue, "skipped"),
        stopped: count_status(queue, "stopped"),
        can: BOARD_ACTIONS.iter().filter(|a| can_send(a, &state)).map(|a| a.to_string()).collect(),
        ended: run_ended(&state),
        state,
    })
}

// ── control.json 쓰기 ──────────────────────────────

/// `{"action", "ts"}` 를 실행 폴더의 `control.json` 으로 쓴다. 엔진이 1초마다 보다가 받으면 지운다.
fn write_control(run_dir: &Path, action: &str, now: f64) -> Result<(), String> {
    let request = ControlRequest { action: action.to_string().into(), ts: Num::r3(now).into(), ..Default::default() };
    let body = to_pretty_json(&ControlFile::new(request)).map_err(|e| format!("요청을 만들지 못했습니다: {e}"))?;
    fileio::write_atomic(&run_dir.join("control.json"), "control", body.as_bytes()).map_err(|e| format!("요청을 쓰지 못했습니다: {e}"))
}

/// 자율주행 실행에 일시정지·재개·중단을 보낸다. 중단은 확인 창을 먼저 띄운다(취소면 아무것도 쓰지 않고 끝난다).
#[tauri::command(async)]
pub fn autopilot_control(board: B<'_>, run_id: String, action: String) -> Result<(), String> {
    // 잠금은 이 문장에서 풀린다 — 확인 창이 떠 있는 동안 1초 틱이 멈추지 않게
    let run_dir = board.autopilot.lock().unwrap().control_target(&run_id, &action)?;
    if action == "stop" && !board.confirm("자율주행 중단", "이 실행을 중단할까요? 지금 단위는 중단으로 기록되고 다시 띄우지 않습니다.", "중단", "취소") {
        return Ok(());
    }
    write_control(&run_dir, &action, clock::now())?;
    board_log(&format!("[자율주행] {run_id} {action}"));
    board.publish();
    Ok(())
}

// ── 시작 준비 ───────────────────────────────────────
//
// 시작 준비 화면이 보낸 출발 요청을 실행 폴더(`runs\<id>\run.json`·`queue.json`)로 쓰고 `claude-autopilot start <폴더> --json` 을 부른다.
// 출발 규칙(실행 환경 문턱·경로 확인·판정 명령 꼴·가지·작업 스케줄러 등록)은 명령줄이 소유한다. 여기서는 규칙을 다시 적지 않고,
// 파일을 쓰기 전에 입력 모양(실행 id·크기·동의 기록이 끼어들지 않았는지)만 보고, 명령줄이 거절하면 그 글을 그대로 돌려준다.

/// 명령줄 실행 파일 이름(`claude-autopilot` 크레이트의 `[[bin]]`)
const CLI_EXE: &str = if cfg!(windows) { "claude-autopilot.exe" } else { "claude-autopilot" };
/// 한 번에 담을 수 있는 단위 수와 요청 크기(바이트). 화면의 `MAX_UNITS` 와 같은 값
const MAX_UNITS: usize = 30;
const MAX_REQUEST_BYTES: usize = 1 << 20;
/// 출발 명령이 끝나기를 기다리는 시간. WSL 준비 확인이 배포판을 깨우므로 넉넉히 둔다
const START_TIMEOUT: Duration = Duration::from_secs(300);

/// 화면이 고를 수 있는 실행 환경 한 개
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutopilotEnvOption {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub distro: Option<String>,
    /// Claude Code 샌드박스를 걸 수 있는 환경(WSL)이면 참. 거짓이면 화면이 샌드박스 없음 동의를 받는다
    pub sandbox_capable: bool,
}

fn env_option(entry: &EnvEntry) -> AutopilotEnvOption {
    AutopilotEnvOption {
        key: entry.choice.key(),
        label: entry.label.clone(),
        kind: entry.choice.kind.id().to_string(),
        distro: entry.choice.distro.clone(),
        sandbox_capable: entry.choice.kind.sandbox_capable(),
    }
}

/// 이 PC 에서 자율주행 실행 환경으로 고를 수 있는 목록. 기본값은 없다(고르는 것은 사람)
#[tauri::command(async)]
pub fn autopilot_envs() -> Vec<AutopilotEnvOption> {
    claude_board_core::envs::detect().iter().map(env_option).collect()
}

/// 화면이 보낸 출발 요청
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    run_id: String,
    run: Value,
    queue: Vec<Value>,
    accept_unsandboxed: bool,
}

/// 출발한 결과
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDone {
    run_id: String,
    scheduled: bool,
}

/// 모델 이름 꼴(`claude --model` 에 넘기는 값). 명령줄이 보지 않으므로 여기서 본다
fn valid_model(model: &str) -> bool {
    let mut chars = model.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '[' | ']' | '-'))
}

/// 입력 모양 검사. 경로·환경·판정 명령은 보지 않는다(명령줄이 본다)
fn validate_request(req: &StartRequest) -> Result<(), String> {
    if !valid_run_id(&req.run_id) {
        return Err(format!("실행 id 규칙(영문·숫자·점·밑줄·붙임표)에 맞지 않습니다: {}", req.run_id));
    }
    let run = req.run.as_object().ok_or("run.json 내용이 객체가 아닙니다.")?;
    if req.queue.is_empty() || req.queue.len() > MAX_UNITS {
        return Err(format!("단위는 1개 이상 {MAX_UNITS}개 이하여야 합니다."));
    }
    if req.queue.iter().any(|u| !u.is_object()) {
        return Err("queue.json 의 단위가 객체가 아닙니다.".to_string());
    }
    if serde_json::to_vec(&req.run).map_or(0, |b| b.len()) + serde_json::to_vec(&req.queue).map_or(0, |b| b.len()) > MAX_REQUEST_BYTES {
        return Err("요청이 너무 큽니다.".to_string());
    }
    if !run.get("model").and_then(Value::as_str).is_some_and(valid_model) {
        return Err("모델 이름이 비었거나 꼴이 아닙니다.".to_string());
    }
    // 샌드박스 없음 동의는 `--accept-unsandboxed` 로만 남긴다. 요청 안에 동의 기록이 있으면 그 단추를 건너뛰는 길이 된다
    if run.get("exec_env").and_then(|e| e.get("unsandboxed_ok")).is_some() {
        return Err("동의 기록(unsandboxed_ok)은 요청에 담을 수 없습니다. 동의는 화면의 확인 칸으로만 받습니다.".to_string());
    }
    Ok(())
}

/// `claude-autopilot start <실행 폴더> --json [--accept-unsandboxed]`
fn start_args(dir: &Path, accept_unsandboxed: bool) -> Vec<String> {
    let mut args = vec!["start".to_string(), dir.to_string_lossy().into_owned(), "--json".to_string()];
    if accept_unsandboxed {
        args.push("--accept-unsandboxed".to_string());
    }
    args
}

/// 실행 폴더를 새로 만들고 `run.json`·`queue.json` 을 쓴다. 같은 이름의 폴더가 이미 있으면 거부한다(남의 실행을 덮지 않는다)
fn write_run_folder(runs_dir: &Path, req: &StartRequest) -> Result<PathBuf, String> {
    fs::create_dir_all(runs_dir).map_err(|e| format!("실행 폴더의 부모({})를 만들지 못했습니다: {e}", runs_dir.display()))?;
    let dir = runs_dir.join(&req.run_id);
    fs::create_dir(&dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("같은 이름의 실행 폴더가 이미 있습니다: {}", dir.display())
        } else {
            format!("실행 폴더를 만들지 못했습니다({}): {e}", dir.display())
        }
    })?;
    let written = write_json_file(&dir.join("queue.json"), &req.queue).and_then(|()| write_json_file(&dir.join("run.json"), &req.run));
    if let Err(why) = written {
        discard_run_folder(&dir);
        return Err(why);
    }
    Ok(dir)
}

fn write_json_file<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let mut body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    body.push('\n');
    fileio::write_atomic(path, "start", body.as_bytes()).map_err(|e| format!("{} 을(를) 쓰지 못했습니다: {e}", path.display()))
}

/// 출발하지 못한 실행 폴더를 치운다. 우리가 쓴 두 파일만 지우고 폴더가 비어 있을 때만 폴더를 지운다
fn discard_run_folder(dir: &Path) {
    for name in ["run.json", "queue.json"] {
        let _ = fs::remove_file(dir.join(name));
    }
    let _ = fs::remove_dir(dir);
}

struct CliOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}

/// 명령줄을 콘솔 창 없이 돌리고 끝나기를 기다린다. 시간이 지나면 죽이고 실패로 돌려준다
fn run_cli(exe: &Path, args: &[String], timeout: Duration) -> Result<CliOutput, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::util::CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{} 를 띄우지 못했습니다: {e}", exe.display()))?;
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("출발 명령이 {}초 안에 끝나지 않아 멈췄습니다.", timeout.as_secs()));
            }
            Err(e) => return Err(format!("출발 명령을 기다리지 못했습니다: {e}")),
        }
    };
    // 명령줄이 끝났는데 손자 프로세스가 파이프를 쥐고 있으면 read_to_end 가 끝나지 않는다. 2초만 기다리고 버린다
    let drain_by = Instant::now() + Duration::from_secs(2);
    let text = |h: JoinHandle<Vec<u8>>| {
        while !h.is_finished() && Instant::now() < drain_by {
            std::thread::sleep(Duration::from_millis(20));
        }
        if h.is_finished() { String::from_utf8_lossy(&h.join().unwrap_or_default()).into_owned() } else { String::new() }
    };
    Ok(CliOutput { code: status.code().unwrap_or(-1), stdout: text(out), stderr: text(err) })
}

/// 명령줄이 거절한 까닭 글 — `--json` 의 `problems` 를 줄마다, 없으면 표준 오류, 그것도 없으면 종료 코드
fn failure_text(out: &CliOutput) -> String {
    let problems: Vec<String> = serde_json::from_str::<Value>(out.stdout.trim())
        .ok()
        .and_then(|v| v.get("problems").and_then(Value::as_array).cloned())
        .map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if !problems.is_empty() {
        return problems.join("\n");
    }
    let err = out.stderr.trim();
    if !err.is_empty() {
        return err.to_string();
    }
    format!("출발 명령이 실패했습니다(종료 코드 {}).", out.code)
}

/// 출발의 몸통 — 실행 폴더를 쓰고 명령줄을 부른다. 명령줄이 거절하면 쓴 폴더를 치우고 그 글을 돌려준다
fn start_run(exe: &Path, runs_dir: &Path, req: &StartRequest, timeout: Duration) -> Result<StartDone, String> {
    validate_request(req)?;
    let dir = write_run_folder(runs_dir, req)?;
    let out = match run_cli(exe, &start_args(&dir, req.accept_unsandboxed), timeout) {
        Ok(out) => out,
        Err(why) => {
            discard_run_folder(&dir);
            return Err(why);
        }
    };
    if out.code != 0 {
        discard_run_folder(&dir);
        return Err(failure_text(&out));
    }
    let scheduled = serde_json::from_str::<Value>(out.stdout.trim()).ok().and_then(|v| v.get("scheduled").and_then(Value::as_bool)).unwrap_or(false);
    Ok(StartDone { run_id: req.run_id.clone(), scheduled })
}

/// 명령줄 실행 파일 — 보드 실행 파일과 같은 폴더(엔진 `claude-autopilot-engine` 을 찾는 방식과 같다)
fn cli_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("보드 실행 파일 위치를 알 수 없습니다: {e}"))?.with_file_name(CLI_EXE);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!("{CLI_EXE} 를 찾지 못했습니다: {}. 보드 실행 파일과 같은 폴더에 있어야 합니다.", exe.display()))
    }
}

/// 자율주행을 시작한다. 실패하면 명령줄의 거절 글(여러 줄일 수 있다)이 그대로 `Err` 로 간다
#[tauri::command(async)]
pub fn autopilot_start(board: B<'_>, request: StartRequest) -> Result<StartDone, String> {
    let exe = cli_exe()?;
    let runs_dir = claude_board_core::paths::autopilot_root(false).join("runs");
    let done = start_run(&exe, &runs_dir, &request, START_TIMEOUT).inspect_err(|why| board_log(&format!("[자율주행] 출발 실패 {}: {}", request.run_id, why.replace('\n', " / "))))?;
    board_log(&format!("[자율주행] 출발 {}", done.run_id));
    board.publish();
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const NOW: f64 = 2_000_000_000.0;

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cb-autopilot-{}", crate::util::short_id()));
            fs::create_dir_all(root.join("runs")).unwrap();
            Self { root }
        }

        fn run_dir(&self, id: &str) -> PathBuf {
            self.root.join("runs").join(id)
        }

        fn put(&self, id: &str, rel: &str, value: &Value) {
            let path = self.run_dir(id).join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value.to_string()).unwrap();
        }

        fn reader(&self) -> AutopilotReader {
            AutopilotReader::new(self.root.clone())
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn running_run() -> Value {
        json!({"state": "running", "title": "야간 작업", "project_root": r"C:\work\app", "unit": 2, "created": NOW - 100.0,
               "exec_env": {"key": "pwsh", "kind": "pwsh", "sandbox": false}})
    }

    fn queue_of(statuses: &[&str]) -> Value {
        Value::Array(statuses.iter().enumerate().map(|(i, s)| json!({"id": format!("u{i}"), "title": format!("단위 {}", i + 1), "cwd": r"C:\work\unit", "status": s})).collect())
    }

    fn view_of(sandbox: &Sandbox, id: &str) -> AutopilotRunView {
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        reader.view(NOW).into_iter().find(|v| v.id == id).expect("목록에 있어야 한다")
    }

    #[test]
    fn reads_running_run() {
        let sandbox = Sandbox::new();
        sandbox.put("r1", "run.json", &json!({"state": "waiting_limit", "title": "야간 작업", "reason": "한도", "wait_until": 1_999_999_999.5, "created": NOW - 5.0}));
        let view = view_of(&sandbox, "r1");
        assert_eq!((view.title.as_str(), view.state.as_str(), view.reason.as_str()), ("야간 작업", "waiting_limit", "한도"));
        assert_eq!(view.wait_until, Some(1_999_999_999_500));
        assert!(!view.ended);
        assert_eq!(view.unit, None);
    }

    #[test]
    fn path_from_project_root_then_first_cwd() {
        let sandbox = Sandbox::new();
        sandbox.put("a", "run.json", &running_run());
        sandbox.put("a", "queue.json", &queue_of(&["running"]));
        assert_eq!(view_of(&sandbox, "a").path, r"C:\work\app");
        sandbox.put("b", "run.json", &json!({"state": "running"}));
        sandbox.put("b", "queue.json", &queue_of(&["running"]));
        assert_eq!(view_of(&sandbox, "b").path, r"C:\work\unit");
    }

    #[test]
    fn counts_from_queue() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        sandbox.put("r", "queue.json", &queue_of(&["done", "done", "failed", "capped", "skipped", "stopped", "running", "pending"]));
        let v = view_of(&sandbox, "r");
        assert_eq!((v.total, v.done, v.failed, v.capped, v.skipped, v.stopped), (8, 2, 1, 1, 1, 1));
    }

    #[test]
    fn current_unit_from_engine_json() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        sandbox.put("r", "queue.json", &queue_of(&["done", "running"]));
        sandbox.put("r", "units/2/engine.json", &json!({"title": "엔진이 쓴 제목", "phase": "model", "active_sec": 12.5, "cost_usd": 0.42, "try": 1}));
        let unit = view_of(&sandbox, "r").unit.expect("지금 단위");
        assert_eq!((unit.n, unit.title.as_str(), unit.phase.as_str(), unit.tries), (2, "엔진이 쓴 제목", "model", 1));
        assert_eq!((unit.active_sec, unit.cost_usd), (12.5, 0.42));
    }

    #[test]
    fn current_unit_title_falls_back_to_queue() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        sandbox.put("r", "queue.json", &queue_of(&["done", "running"]));
        let unit = view_of(&sandbox, "r").unit.expect("지금 단위");
        assert_eq!((unit.title.as_str(), unit.phase.as_str(), unit.cost_usd), ("단위 2", "", 0.0));
    }

    #[test]
    fn skips_preparing_run() {
        let sandbox = Sandbox::new();
        fs::create_dir_all(sandbox.run_dir("empty")).unwrap();
        sandbox.put("blank", "run.json", &json!({"state": ""}));
        sandbox.put("none", "run.json", &json!({"title": "아직"}));
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        assert!(reader.view(NOW).is_empty());
        assert_eq!(reader.state_of("blank"), None);
    }

    #[test]
    fn drops_ended_after_600s() {
        let sandbox = Sandbox::new();
        sandbox.put("old", "run.json", &json!({"state": "done", "ended": NOW - 601.0}));
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        assert!(reader.view(NOW).is_empty());
        // 뺀 뒤에는 run.json 이 그대로인 동안 다른 파일을 다시 읽지 않는다
        let reads = reader.reads;
        reader.refresh(NOW + 5.0);
        assert_eq!(reader.reads, reads);
    }

    #[test]
    fn keeps_ended_within_600s() {
        let sandbox = Sandbox::new();
        sandbox.put("fresh", "run.json", &json!({"state": "stopped", "ended": NOW - 599.0}));
        let v = view_of(&sandbox, "fresh");
        assert!(v.ended);
        assert!(v.can.is_empty());
    }

    #[test]
    fn mtime_cache_skips_unchanged() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        sandbox.put("r", "queue.json", &queue_of(&["running", "running"]));
        sandbox.put("r", "units/2/engine.json", &json!({"phase": "action"}));
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        assert_eq!(reader.reads, 3);
        reader.refresh(NOW + 1.0);
        reader.refresh(NOW + 2.0);
        assert_eq!(reader.reads, 3, "바뀐 파일이 없으면 다시 읽지 않는다");
        // 길이가 달라지면 시각이 같아도 다시 읽는다
        sandbox.put("r", "units/2/engine.json", &json!({"phase": "check"}));
        reader.refresh(NOW + 3.0);
        assert_eq!(reader.reads, 4);
    }

    #[test]
    fn broken_json_keeps_previous() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        fs::write(sandbox.run_dir("r").join("run.json"), b"{\"state\": \"runn").unwrap();
        reader.refresh(NOW + 1.0);
        let views = reader.view(NOW + 1.0);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].state, "running");
    }

    #[test]
    fn removed_folder_drops() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        assert_eq!(reader.view(NOW).len(), 1);
        fs::remove_dir_all(sandbox.run_dir("r")).unwrap();
        reader.refresh(NOW + 1.0);
        assert!(reader.view(NOW + 1.0).is_empty());
        assert_eq!(reader.run_dir("r"), None);
    }

    #[test]
    fn env_view_native_and_wsl() {
        let sandbox = Sandbox::new();
        sandbox.put("n", "run.json", &running_run());
        sandbox.put("w", "run.json", &json!({"state": "running", "exec_env": {"key": "wsl:Ubuntu-24.04", "kind": "wsl", "distro": "Ubuntu-24.04", "sandbox": true}}));
        let native = view_of(&sandbox, "n").env;
        assert_eq!((native.key.as_str(), native.label.as_str(), native.sandbox), ("pwsh", "PowerShell 7", false));
        let wsl = view_of(&sandbox, "w").env;
        assert_eq!((wsl.key.as_str(), wsl.label.as_str(), wsl.sandbox), ("wsl:Ubuntu-24.04", "WSL · Ubuntu-24.04", true));
    }

    #[test]
    fn env_view_missing() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &json!({"state": "running"}));
        let env = view_of(&sandbox, "r").env;
        assert_eq!((env.key.as_str(), env.label.as_str(), env.sandbox), ("", "알 수 없음", false));
    }

    #[test]
    fn can_by_state() {
        let sandbox = Sandbox::new();
        for (id, state) in [("a", "running"), ("b", "paused"), ("c", "done")] {
            sandbox.put(id, "run.json", &json!({"state": state, "ended": NOW}));
        }
        assert_eq!(view_of(&sandbox, "a").can, ["pause", "stop"]);
        assert_eq!(view_of(&sandbox, "b").can, ["resume", "stop"]);
        assert!(view_of(&sandbox, "c").can.is_empty());
    }

    #[test]
    fn control_writes_file() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        let dir = reader.control_target("r", "pause").unwrap();
        write_control(&dir, "pause", 1_761_000_000.123_456).unwrap();
        let written: Value = serde_json::from_slice(&fs::read(sandbox.run_dir("r").join("control.json")).unwrap()).unwrap();
        assert_eq!(written, json!({"action": "pause", "ts": 1_761_000_000.123}));
    }

    #[test]
    fn control_refuses_unknown_action() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        assert_eq!(reader.control_target("r", "takeover"), Err("모르는 요청".to_string()));
        assert!(!sandbox.run_dir("r").join("control.json").exists());
    }

    #[test]
    fn control_refuses_by_state() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &json!({"state": "paused"}));
        sandbox.put("d", "run.json", &json!({"state": "done", "ended": NOW}));
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        let refused = Err("지금 상태에서는 할 수 없습니다".to_string());
        assert_eq!(reader.control_target("r", "pause"), refused);
        assert_eq!(reader.control_target("d", "stop"), refused);
        assert!(reader.control_target("r", "resume").is_ok());
    }

    #[test]
    fn run_dir_refuses_path_pieces() {
        let sandbox = Sandbox::new();
        sandbox.put("r", "run.json", &running_run());
        let mut reader = sandbox.reader();
        reader.refresh(NOW);
        assert_eq!(reader.run_dir(r"..\r"), None);
        assert!(reader.run_dir("r").is_some());
    }
}

#[cfg(test)]
mod start_tests {
    use super::*;
    use claude_board_core::envs::EnvChoice;
    use serde_json::json;

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("cb-ap-start-{}", crate::util::short_id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn runs(&self) -> PathBuf {
            self.0.join("runs")
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request() -> StartRequest {
        StartRequest {
            run_id: "20261106-090503-toy".to_string(),
            run: json!({"exec_env": {"key": "pwsh", "kind": "pwsh"}, "model": "claude-sonnet-5-5", "project": "toy", "project_root": r"C:\work\toy", "title": "toy"}),
            queue: vec![json!({"check": "exit 0", "cwd": r"C:\work\toy", "id": "u1", "prompt": "p", "title": "t"})],
            accept_unsandboxed: true,
        }
    }

    fn refused(req: &StartRequest, needle: &str) {
        let why = validate_request(req).expect_err("거절해야 한다");
        assert!(why.contains(needle), "{needle:?} 가 없다: {why}");
    }

    #[test]
    fn model_shape() {
        for ok in ["claude-sonnet-5-5", "sonnet", "opus", "sonnet[1m]", "a.b_c-1"] {
            assert!(valid_model(ok), "{ok}");
        }
        for bad in ["", "Sonnet", "-x", "a b", "a;b", "한글", "a/b"] {
            assert!(!valid_model(bad), "{bad}");
        }
    }

    #[test]
    fn accepts_a_well_formed_request() {
        assert_eq!(validate_request(&request()), Ok(()));
    }

    #[test]
    fn refuses_run_ids_that_could_leave_the_runs_folder() {
        for bad in ["", ".", "..", "a/b", r"a\b", "a b", "한글"] {
            refused(&StartRequest { run_id: bad.to_string(), ..request() }, "실행 id");
        }
    }

    #[test]
    fn refuses_bad_shapes_and_sizes() {
        refused(&StartRequest { run: json!([]), ..request() }, "객체가 아닙니다");
        refused(&StartRequest { queue: vec![], ..request() }, "단위는");
        refused(&StartRequest { queue: vec![json!({}); MAX_UNITS + 1], ..request() }, "단위는");
        refused(&StartRequest { queue: vec![json!("x")], ..request() }, "단위가 객체가 아닙니다");
        let huge = StartRequest { queue: vec![json!({"prompt": "x".repeat(MAX_REQUEST_BYTES)})], ..request() };
        refused(&huge, "너무 큽니다");
        let mut no_model = request();
        no_model.run["model"] = json!("Bad Model");
        refused(&no_model, "모델");
        no_model.run.as_object_mut().unwrap().remove("model");
        refused(&no_model, "모델");
    }

    #[test]
    fn consent_cannot_ride_inside_the_request() {
        let mut req = request();
        req.run["exec_env"]["unsandboxed_ok"] = json!({"at": 1, "by": "board"});
        refused(&req, "동의 기록");
    }

    #[test]
    fn start_args_are_exactly_the_cli_contract() {
        let dir = Path::new(r"C:\runs\r1");
        assert_eq!(start_args(dir, false), ["start", r"C:\runs\r1", "--json"]);
        assert_eq!(start_args(dir, true), ["start", r"C:\runs\r1", "--json", "--accept-unsandboxed"]);
        // 실행 환경 문턱의 주인이 명령줄이므로 --dev·--no-schedule 은 붙이지 않는다
        assert!(!start_args(dir, true).iter().any(|a| a == "--dev" || a == "--no-schedule"));
    }

    #[test]
    fn writes_both_files_and_refuses_an_existing_folder() {
        let t = Temp::new();
        let req = request();
        let dir = write_run_folder(&t.runs(), &req).unwrap();
        assert_eq!(dir, t.runs().join("20261106-090503-toy"));
        let run: Value = serde_json::from_slice(&fs::read(dir.join("run.json")).unwrap()).unwrap();
        let queue: Value = serde_json::from_slice(&fs::read(dir.join("queue.json")).unwrap()).unwrap();
        assert_eq!(run, req.run);
        assert_eq!(queue, Value::Array(req.queue.clone()));
        let again = write_run_folder(&t.runs(), &req).unwrap_err();
        assert!(again.contains("이미 있습니다"), "{again}");
        // 먼저 있던 폴더의 파일을 건드리지 않는다
        assert!(dir.join("run.json").is_file());
    }

    #[test]
    fn discard_removes_only_what_the_board_wrote() {
        let t = Temp::new();
        let dir = write_run_folder(&t.runs(), &request()).unwrap();
        discard_run_folder(&dir);
        assert!(!dir.exists());
        let kept = write_run_folder(&t.runs(), &StartRequest { run_id: "other".to_string(), ..request() }).unwrap();
        fs::write(kept.join("note.txt"), "keep").unwrap();
        discard_run_folder(&kept);
        assert!(kept.join("note.txt").is_file(), "다른 파일이 든 폴더는 지우지 않는다");
        assert!(!kept.join("run.json").exists());
    }

    #[test]
    fn failure_text_prefers_problems_then_stderr_then_code() {
        let out = |code, stdout: &str, stderr: &str| CliOutput { code, stdout: stdout.to_string(), stderr: stderr.to_string() };
        assert_eq!(failure_text(&out(1, "{\"ok\":false,\"problems\":[\"가\",\"나\"]}\n", "")), "가\n나");
        assert_eq!(failure_text(&out(2, "", "알 수 없는 명령입니다: x\n사용법\n")), "알 수 없는 명령입니다: x\n사용법");
        assert_eq!(failure_text(&out(3, "not json", "")), "출발 명령이 실패했습니다(종료 코드 3).");
        assert_eq!(failure_text(&out(1, "{\"ok\":false,\"problems\":[]}", "e")), "e");
    }

    #[test]
    fn env_option_marks_only_wsl_sandbox_capable() {
        let entry = |kind, distro: Option<&str>| EnvEntry { choice: EnvChoice { kind, distro: distro.map(str::to_string) }, label: "x".into(), program: "x.exe".into() };
        let wsl = env_option(&entry(EnvKind::Wsl, Some("Ubuntu")));
        assert_eq!((wsl.key.as_str(), wsl.kind.as_str(), wsl.distro.as_deref(), wsl.sandbox_capable), ("wsl:Ubuntu", "wsl", Some("Ubuntu"), true));
        for kind in [EnvKind::Pwsh, EnvKind::PowerShell, EnvKind::Cmd, EnvKind::GitBash] {
            let e = env_option(&entry(kind, None));
            assert_eq!((e.key.as_str(), e.distro, e.sandbox_capable), (kind.id(), None, false));
        }
    }

    /// 가짜 명령줄(배치 파일) — 받은 인자를 옆 파일에 적고 정해 둔 글과 종료 코드로 끝난다
    #[cfg(windows)]
    fn fake_cli(t: &Temp, stdout: &str, code: i32) -> PathBuf {
        let path = t.0.join("fake-cli.cmd");
        let body = format!("@echo off\r\necho %* > \"%~dp0args.txt\"\r\necho {stdout}\r\nexit /b {code}\r\n");
        fs::write(&path, body).unwrap();
        path
    }

    #[cfg(windows)]
    #[test]
    fn success_keeps_the_folder_and_passes_the_consent_flag() {
        let t = Temp::new();
        let cli = fake_cli(&t, r#"{"ok":true,"run":"20261106-090503-toy","scheduled":true}"#, 0);
        let done = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap();
        assert_eq!(done, StartDone { run_id: "20261106-090503-toy".to_string(), scheduled: true });
        let dir = t.runs().join("20261106-090503-toy");
        assert!(dir.join("run.json").is_file() && dir.join("queue.json").is_file());
        let args = fs::read_to_string(t.0.join("args.txt")).unwrap();
        assert!(args.starts_with("start "), "{args}");
        assert!(args.contains(&*dir.to_string_lossy()) && args.contains("--json") && args.contains("--accept-unsandboxed"), "{args}");
    }

    #[cfg(windows)]
    #[test]
    fn no_consent_flag_when_the_screen_did_not_ask_for_it() {
        let t = Temp::new();
        let cli = fake_cli(&t, r#"{"ok":true,"scheduled":false}"#, 0);
        let req = StartRequest { accept_unsandboxed: false, ..request() };
        let done = start_run(&cli, &t.runs(), &req, Duration::from_secs(20)).unwrap();
        assert!(!done.scheduled);
        assert!(!fs::read_to_string(t.0.join("args.txt")).unwrap().contains("--accept-unsandboxed"));
    }

    #[cfg(windows)]
    #[test]
    fn a_refusal_shows_the_cli_text_and_leaves_no_folder() {
        let t = Temp::new();
        let cli = fake_cli(&t, r#"{"ok":false,"problems":["unit 1 bad cwd","second line"]}"#, 1);
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
        assert_eq!(why, "unit 1 bad cwd\nsecond line");
        assert!(!t.runs().join("20261106-090503-toy").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_hung_cli_is_killed_at_the_timeout() {
        let t = Temp::new();
        let path = t.0.join("hang.cmd");
        fs::write(&path, "@echo off\r\nping -n 20 127.0.0.1 >nul\r\n").unwrap();
        let why = start_run(&path, &t.runs(), &request(), Duration::from_secs(1)).unwrap_err();
        assert!(why.contains("멈췄습니다"), "{why}");
        assert!(!t.runs().join("20261106-090503-toy").exists());
    }

    #[test]
    fn a_missing_exe_is_reported_and_leaves_no_folder() {
        let t = Temp::new();
        let why = start_run(&t.0.join("none.exe"), &t.runs(), &request(), Duration::from_secs(5)).unwrap_err();
        assert!(why.contains("띄우지 못했습니다"), "{why}");
        assert!(!t.runs().join("20261106-090503-toy").exists());
    }

    // ── 가장자리: 크기·실행 id·동의 기록·명령줄 실패·정리·동시 출발 ──

    fn with_prompt_bytes(n: usize) -> StartRequest {
        let mut req = request();
        req.queue[0]["prompt"] = json!("x".repeat(n));
        req
    }

    fn request_size(req: &StartRequest) -> usize {
        serde_json::to_vec(&req.run).unwrap().len() + serde_json::to_vec(&req.queue).unwrap().len()
    }

    /// 요청 크기 경계: run+queue 를 JSON 으로 쓴 바이트가 정확히 한도면 통과, 1바이트 넘으면 거절. 글자 수가 아니라 바이트로 센다
    #[test]
    fn request_size_limit_is_exact_and_counts_bytes() {
        let base = request_size(&with_prompt_bytes(0));
        assert_eq!(validate_request(&with_prompt_bytes(MAX_REQUEST_BYTES - base)), Ok(()), "한도 바이트 정확히");
        assert_eq!(request_size(&with_prompt_bytes(MAX_REQUEST_BYTES - base)), MAX_REQUEST_BYTES);
        refused(&with_prompt_bytes(MAX_REQUEST_BYTES - base + 1), "너무 큽니다");
        // 한글은 글자 하나가 3바이트다 — 글자 수로 세면 이 요청은 통과해 버린다
        let mut korean = request();
        korean.queue[0]["prompt"] = json!("가".repeat(MAX_REQUEST_BYTES / 3));
        refused(&korean, "너무 큽니다");
        // run 쪽이 커도 같다
        let mut big_run = request();
        big_run.run["title"] = json!("t".repeat(MAX_REQUEST_BYTES));
        refused(&big_run, "너무 큽니다");
        // 단위 30개가 각자 작아도 합이 한도를 넘으면 거절
        let mut spread = request();
        spread.queue = (0..MAX_UNITS).map(|i| json!({"id": format!("u{i}"), "prompt": "y".repeat(MAX_REQUEST_BYTES / MAX_UNITS)})).collect();
        refused(&spread, "너무 큽니다");
    }

    #[test]
    fn unit_count_boundaries() {
        for count in [1, MAX_UNITS] {
            let req = StartRequest { queue: vec![json!({"id": "u"}); count], ..request() };
            assert_eq!(validate_request(&req), Ok(()), "{count}개");
        }
        for count in [0, MAX_UNITS + 1, 1000] {
            let req = StartRequest { queue: vec![json!({"id": "u"}); count], ..request() };
            refused(&req, "단위는");
        }
        // 여럿 중 하나만 객체가 아니어도 거절
        let mut mixed = vec![json!({"id": "a"}), json!({"id": "b"}), json!(null)];
        refused(&StartRequest { queue: mixed.clone(), ..request() }, "객체가 아닙니다");
        mixed[2] = json!([]);
        refused(&StartRequest { queue: mixed, ..request() }, "객체가 아닙니다");
    }

    /// 실행 id 표: 폴더 밖으로 나가거나 윈도우가 다르게 읽는 모양은 파일을 쓰기 전에 거절된다(명령줄도 부르지 않는다)
    #[test]
    fn run_id_table_is_refused_before_anything_is_written() {
        let long_ok = "a".repeat(120);
        let too_long = "a".repeat(121);
        let bad = [
            "", ".", "..", "...", "../x", "..\\x", "x/..", "a/b", "a\\b", "/abs", "\\abs", "\\\\server\\share", "C:\\x", "C:x", "C:", "x:y",
            "a b", " a", "a ", "a\tb", "a\nb", "a\0b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "a:b", "%TEMP%", "$HOME", "~", "한글", "a\u{ff0e}\u{ff0e}", "\u{2024}\u{2024}",
            too_long.as_str(),
        ];
        for id in bad {
            let t = Temp::new();
            let req = StartRequest { run_id: id.to_string(), ..request() };
            refused(&req, "실행 id");
            // 존재하지 않는 명령줄을 줘서, 검사에 걸리지 않고 지나갔다면 "띄우지 못했습니다" 로 바뀐다
            let why = start_run(&t.0.join("none.exe"), &t.runs(), &req, Duration::from_secs(5)).unwrap_err();
            assert!(why.contains("실행 id") && !why.contains("띄우지"), "{id:?}: {why}");
            assert!(!t.runs().exists(), "{id:?}: 실행 폴더의 부모까지 만들어졌다");
        }
        assert_eq!(validate_request(&StartRequest { run_id: long_ok, ..request() }), Ok(()), "120자는 통과");
    }

    /// 규칙은 통과하지만 윈도우가 따로 읽는 이름(장치 이름·끝 점)은 실제 폴더를 만들 때 정직하게 갈린다:
    /// 실패하거나, 성공하면 그 이름 그대로의 폴더가 생겨야 한다(다른 이름의 폴더가 몰래 생기면 안 된다)
    #[cfg(windows)]
    #[test]
    fn windows_special_names_either_fail_or_create_exactly_that_folder() {
        for id in ["CON", "con", "NUL", "AUX", "PRN", "COM1", "LPT1", "NUL.txt", "con.json"] {
            let t = Temp::new();
            let req = StartRequest { run_id: id.to_string(), ..request() };
            assert_eq!(validate_request(&req), Ok(()), "{id} 는 id 규칙을 통과한다");
            match write_run_folder(&t.runs(), &req) {
                Err(why) => assert!(!why.is_empty(), "{id}"),
                Ok(dir) => {
                    let names: Vec<String> = fs::read_dir(t.runs()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
                    assert_eq!(names, [id.to_string()], "{id}: 실제로 생긴 폴더 이름이 요청과 다르다 ({dir:?})");
                    assert!(dir.join("run.json").is_file() && dir.join("queue.json").is_file(), "{id}");
                    discard_run_folder(&dir);
                }
            }
        }
    }

    /// KNOWN BUG(막아 둠): 끝이 점인 id("a.")는 `valid_run_id` 를 통과하지만 윈도우가 끝 점을 떼어 폴더 이름이 "a" 가 된다.
    /// 그러면 명령줄은 `runs\a.` 로 id "a." 를, 보드는 폴더 이름으로 id "a" 를 보게 된다. 화면이 만드는 id 는 끝 점이 없어(`slug`) 일상에서는 안 나온다.
    /// 고치려면 `crates/autopilot/src/cli.rs` 의 `valid_run_id` 가 끝 점(과 장치 이름)을 거절하면 된다. 확인: cargo test -- --ignored trailing_dot_run_id
    #[cfg(windows)]
    #[test]
    fn trailing_dot_run_id_is_refused_or_creates_exactly_that_folder() {
        for id in ["a.", "a..", "x.log."] {
            let t = Temp::new();
            let req = StartRequest { run_id: id.to_string(), ..request() };
            if validate_request(&req).is_err() {
                continue;
            }
            let dir = write_run_folder(&t.runs(), &req).unwrap();
            let names: Vec<String> = fs::read_dir(t.runs()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            assert_eq!(names, [id.to_string()], "{id}: 실제로 생긴 폴더 이름이 요청과 다르다 ({dir:?})");
        }
    }

    /// 끝 점만 다른 두 id 는 윈도우에서 같은 폴더가 되므로 둘째는 '이미 있다' 로 거절돼야 하고, 먼저 쓴 파일은 그대로다
    #[cfg(windows)]
    #[test]
    fn ids_differing_only_by_a_trailing_dot_cannot_both_start() {
        let t = Temp::new();
        let first = StartRequest { run_id: "same".to_string(), ..request() };
        let dir = write_run_folder(&t.runs(), &first).unwrap();
        let before = fs::read(dir.join("run.json")).unwrap();
        let mut other = request();
        other.run["title"] = json!("다른 제목");
        let second = StartRequest { run_id: "same.".to_string(), run: other.run, ..request() };
        match write_run_folder(&t.runs(), &second) {
            Err(_) => {}
            Ok(_) => panic!("같은 폴더로 읽히는 id 를 둘 다 받았다"),
        }
        assert_eq!(fs::read(dir.join("run.json")).unwrap(), before, "먼저 쓴 run.json 이 덮였다");
    }

    /// 동의 기록은 값이 무엇이든(null·false·0·빈 객체·진짜 기록) 있기만 하면 거절한다
    #[test]
    fn consent_marker_is_refused_whatever_its_value() {
        for value in [json!(null), json!(false), json!(true), json!(0), json!(""), json!({}), json!([]), json!({"at": 1.0, "by": "board"})] {
            let mut req = request();
            req.run["exec_env"]["unsandboxed_ok"] = value.clone();
            refused(&req, "동의 기록");
            let t = Temp::new();
            let why = start_run(&t.0.join("none.exe"), &t.runs(), &req, Duration::from_secs(5)).unwrap_err();
            assert!(why.contains("동의 기록") && !why.contains("띄우지"), "{value}: {why}");
            assert!(!t.runs().exists(), "{value}");
        }
        // 동의 표시가 없는 요청은 통과한다(거절이 exec_env 에 아무 키나 있다고 걸리는 것이 아니다)
        let mut req = request();
        req.run["exec_env"]["distro"] = json!("Ubuntu");
        assert_eq!(validate_request(&req), Ok(()));
        // exec_env 가 객체가 아니어도 이 검사가 죽지 않는다(그 모양은 명령줄이 거절한다)
        for shape in [json!("pwsh"), json!(null), json!([1]), json!(5)] {
            let mut req = request();
            req.run["exec_env"] = shape;
            assert_eq!(validate_request(&req), Ok(()));
        }
    }

    #[test]
    fn model_must_be_a_string_in_shape() {
        for bad in [json!(5), json!(null), json!(true), json!(["opus"]), json!({"a": 1}), json!(" opus"), json!("opus "), json!("opus\n"), json!("OPUS")] {
            let mut req = request();
            req.run["model"] = bad.clone();
            refused(&req, "모델");
        }
    }

    /// 화면(TypeScript)이 보내는 모양과 이 쪽 역직렬화가 이름까지 같다: runId·run·queue·acceptUnsandboxed(camelCase)
    #[test]
    fn request_wire_names_match_the_screen() {
        let wire = json!({"runId": "20261106-090503-toy", "run": {"model": "opus"}, "queue": [{"id": "u1"}], "acceptUnsandboxed": true});
        let req: StartRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!((req.run_id.as_str(), req.accept_unsandboxed, req.queue.len()), ("20261106-090503-toy", true, 1));
        // 한 칸이라도 빠지거나 이름이 snake_case 면 읽지 못한다 — 어느 쪽이 이름을 바꿔도 조용히 기본값으로 가지 않는다
        for key in ["runId", "run", "queue", "acceptUnsandboxed"] {
            let mut missing = wire.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<StartRequest>(missing).is_err(), "{key} 가 없는데 읽혔다");
        }
        let snake = json!({"run_id": "r", "run": {}, "queue": [], "accept_unsandboxed": true});
        assert!(serde_json::from_value::<StartRequest>(snake).is_err());
        let result = serde_json::to_value(StartDone { run_id: "r".into(), scheduled: true }).unwrap();
        assert_eq!(result, json!({"runId": "r", "scheduled": true}));
    }

    #[test]
    fn failure_text_edge_cases() {
        let out = |code, stdout: &str, stderr: &str| CliOutput { code, stdout: stdout.to_string(), stderr: stderr.to_string() };
        // 문자열이 아닌 항목은 건너뛴다
        assert_eq!(failure_text(&out(1, r#"{"problems":[1,"가",null,"나",{"a":1}]}"#, "")), "가\n나");
        // 전부 문자열이 아니면 표준 오류로
        assert_eq!(failure_text(&out(1, r#"{"problems":[1,2]}"#, "오류")), "오류");
        // problems 가 배열이 아니면 표준 오류로
        assert_eq!(failure_text(&out(1, r#"{"problems":"한 줄"}"#, "오류")), "오류");
        // 공백뿐인 표준 오류는 없는 것으로 보고 종료 코드를 쓴다
        assert_eq!(failure_text(&out(7, "", " \r\n\t ")), "출발 명령이 실패했습니다(종료 코드 7).");
        // 표준 출력 앞뒤 공백·줄바꿈은 무시하고 읽는다
        assert_eq!(failure_text(&out(1, "\r\n  {\"problems\":[\"x\"]}  \r\n", "")), "x");
        // 문제 글 안의 줄바꿈·따옴표·한글은 그대로 간다
        assert_eq!(failure_text(&out(1, r#"{"problems":["a\nb \"q\" 한글"]}"#, "")), "a\nb \"q\" 한글");
        // 음수·큰 종료 코드
        assert!(failure_text(&out(-1, "", "")).contains("종료 코드 -1"));
        assert!(failure_text(&out(i32::MAX, "", "")).contains(&i32::MAX.to_string()));
    }

    /// 가짜 명령줄(배치 파일) — 정해 둔 바이트를 표준 출력·표준 오류로 그대로 내보내고 정해 둔 종료 코드로 끝난다
    #[cfg(windows)]
    fn bytes_cli(t: &Temp, out: &[u8], err: &[u8], code: i32) -> PathBuf {
        fs::write(t.0.join("out.bin"), out).unwrap();
        fs::write(t.0.join("err.bin"), err).unwrap();
        let path = t.0.join("bytes-cli.cmd");
        fs::write(&path, format!("@echo off\r\ntype \"%~dp0out.bin\"\r\ntype \"%~dp0err.bin\" 1>&2\r\nexit /b {code}\r\n")).unwrap();
        path
    }

    /// 표준 출력·표준 오류가 모두 비고 종료 코드만 0 이 아닐 때: 종료 코드를 알리고, 만든 폴더는 남기지 않는다
    #[cfg(windows)]
    #[test]
    fn silent_failure_reports_the_exit_code_and_leaves_no_folder() {
        for code in [1, 2, 255, 3010] {
            let t = Temp::new();
            let cli = bytes_cli(&t, b"", b"", code);
            let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
            assert_eq!(why, format!("출발 명령이 실패했습니다(종료 코드 {code})."));
            assert!(!t.runs().join("20261106-090503-toy").exists(), "코드 {code}");
        }
    }

    /// 명령줄이 UTF-8 이 아닌 바이트를 내도 죽지 않고 글로 알린다(깨진 글자는 대체 문자로)
    #[cfg(windows)]
    #[test]
    fn non_utf8_output_is_shown_lossily_not_a_panic() {
        // 표준 오류만 깨짐
        let t = Temp::new();
        let cli = bytes_cli(&t, b"", b"bad \xff\xfe byte\r\n", 1);
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
        assert!(why.starts_with("bad ") && why.contains('\u{fffd}') && why.ends_with("byte"), "{why:?}");
        // 표준 출력의 JSON 문제 글 안에 깨진 바이트: 글은 살고 깨진 자리만 대체 문자
        let t = Temp::new();
        let cli = bytes_cli(&t, b"{\"problems\":[\"a\xffb\",\"ok\"]}", b"", 1);
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
        assert_eq!(why, "a\u{fffd}b\nok");
        // 표준 출력 전체가 JSON 이 아닌 바이트: 표준 오류도 없으면 종료 코드
        let t = Temp::new();
        let cli = bytes_cli(&t, b"\xff\xfe\xfd", b"", 4);
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
        assert_eq!(why, "출발 명령이 실패했습니다(종료 코드 4).");
        // 성공(0)인데 표준 출력이 깨졌거나 비었으면 예약 여부만 거짓으로 두고 성공으로 돌려준다
        for out in [&b"\xff\xfe"[..], b"", b"not json", b"{\"scheduled\":\"yes\"}", b"{\"scheduled\":true"] {
            let t = Temp::new();
            let cli = bytes_cli(&t, out, b"", 0);
            let done = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap();
            assert!(!done.scheduled, "{out:?}");
            assert!(t.runs().join("20261106-090503-toy").join("run.json").is_file(), "성공이면 폴더가 남는다");
        }
    }

    /// 표준 출력이 큰 명령줄(관을 비우지 않으면 멈추는 크기)도 끝난다
    #[cfg(windows)]
    #[test]
    fn large_output_does_not_deadlock_the_pipe() {
        let t = Temp::new();
        let big = vec![b'x'; 2 << 20];
        let cli = bytes_cli(&t, &big, &big, 1);
        let started = Instant::now();
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(60)).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(30), "관이 가득 차 멈췄다");
        assert!(why.len() >= 2 << 20, "표준 오류 전체가 글로 온다: {}", why.len());
    }

    /// 시간이 지나면 명령줄을 죽이고 곧바로 돌아온다 — 스크립트가 스스로 끝나기를 기다리지 않는다
    #[cfg(windows)]
    #[test]
    fn timeout_returns_at_the_deadline_not_when_the_script_ends() {
        let t = Temp::new();
        let path = t.0.join("slow.cmd");
        fs::write(&path, "@echo off\r\nping -n 7 127.0.0.1 >nul\r\necho reached > \"%~dp0reached.txt\"\r\n").unwrap();
        let started = Instant::now();
        let why = start_run(&path, &t.runs(), &request(), Duration::from_secs(1)).unwrap_err();
        let took = started.elapsed();
        assert!(took >= Duration::from_secs(1) && took < Duration::from_millis(4500), "{took:?}");
        assert!(why.contains("1초") && why.contains("멈췄습니다"), "{why}");
        assert!(!t.runs().join("20261106-090503-toy").exists(), "시간 초과면 쓴 폴더를 치운다");
        assert!(!t.0.join("reached.txt").exists());
    }

    /// 명령줄이 이미 끝났는데 그가 띄운 손자 프로세스가 표준 출력을 쥐고 있으면, 출발이 그 손자가 끝날 때까지 돌아오지 못한다.
    /// 시간 제한(START_TIMEOUT)은 `wait` 에만 걸려 있고 관을 비우는 `join` 에는 걸려 있지 않다.
    /// 지금 명령줄은 작업 스케줄러에 등록만 하고 손자를 남기지 않아 실제로 일어나지는 않는다. 그래서 막아 두고(ignore) 사유를 남긴다.
    /// 확인: cargo test -- --ignored grandchild_holding_the_pipe
    #[cfg(windows)]
    #[test]
    fn grandchild_holding_the_pipe_does_not_outlive_the_deadline() {
        let t = Temp::new();
        let path = t.0.join("spawner.cmd");
        fs::write(&path, "@echo off\r\nstart \"\" /b ping -n 9 127.0.0.1\r\nexit /b 0\r\n").unwrap();
        let started = Instant::now();
        let _ = start_run(&path, &t.runs(), &request(), Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_secs(5), "손자가 관을 쥐어 {:?} 동안 돌아오지 못했다", started.elapsed());
    }

    /// 실패해도 보드가 쓴 두 파일만 치운다: 명령줄이 폴더에 만든 것·같은 부모의 다른 실행·사용자 파일은 그대로다
    #[cfg(windows)]
    #[test]
    fn failure_cleanup_touches_only_the_two_files_the_board_wrote() {
        let t = Temp::new();
        // 부모 폴더에 이미 있던 것: 다른 실행 폴더와 사용자 파일
        let other = write_run_folder(&t.runs(), &StartRequest { run_id: "older-run".to_string(), ..request() }).unwrap();
        fs::write(other.join("events.jsonl"), "keep").unwrap();
        fs::write(t.runs().join("notes.txt"), "mine").unwrap();
        let older_run_json = fs::read(other.join("run.json")).unwrap();
        // 명령줄이 자기 폴더 안에 pinned\ 를 만들고 실패한다
        let cli = t.0.join("makes-files.cmd");
        // start_args 는 ["start", dir, ...] 라 첫 인자는 'start' 다 — 배치에서 %2 가 폴더
        fs::write(&cli, "@echo off\r\nmkdir \"%~2\\pinned\"\r\necho x > \"%~2\\pinned\\frozen.json\"\r\necho {\"problems\":[\"rejected\"]}\r\nexit /b 1\r\n").unwrap();
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
        assert_eq!(why, "rejected");
        let mine = t.runs().join("20261106-090503-toy");
        assert!(!mine.join("run.json").exists() && !mine.join("queue.json").exists(), "보드가 쓴 두 파일은 치운다");
        assert!(mine.join("pinned").join("frozen.json").is_file(), "명령줄이 만든 파일은 보드가 지우지 않는다");
        assert_eq!(fs::read(other.join("run.json")).unwrap(), older_run_json);
        assert_eq!(fs::read_to_string(other.join("events.jsonl")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(t.runs().join("notes.txt")).unwrap(), "mine");
    }

    /// 이미 있는 실행 폴더는 명령줄을 부르기 전에 거절하고, 그 폴더의 파일·정리는 건드리지 않는다
    #[cfg(windows)]
    #[test]
    fn existing_folder_is_refused_without_calling_the_cli_or_cleaning_it() {
        let t = Temp::new();
        let existing = t.runs().join("20261106-090503-toy");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("run.json"), "{\"user\":true}").unwrap();
        fs::write(existing.join("queue.json"), "[]").unwrap();
        let cli = fake_cli(&t, r#"{"ok":true}"#, 0);
        let why = start_run(&cli, &t.runs(), &request(), Duration::from_secs(20)).unwrap_err();
        assert!(why.contains("이미 있습니다"), "{why}");
        assert!(!t.0.join("args.txt").exists(), "명령줄을 불렀다");
        assert_eq!(fs::read_to_string(existing.join("run.json")).unwrap(), "{\"user\":true}");
        assert_eq!(fs::read_to_string(existing.join("queue.json")).unwrap(), "[]");
    }

    /// 같은 id 로 동시에 출발하면 하나만 폴더를 얻고, 진 쪽은 이긴 쪽의 파일을 지우거나 덮지 않는다
    #[test]
    fn concurrent_starts_with_the_same_run_id_have_exactly_one_winner() {
        for round in 0..5 {
            let t = Temp::new();
            let runs = t.runs();
            let barrier = Arc::new(std::sync::Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let (runs, barrier) = (runs.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        let mut req = request();
                        req.run["title"] = json!(format!("스레드 {i}"));
                        barrier.wait();
                        write_run_folder(&runs, &req).map(|dir| (i, dir))
                    })
                })
                .collect();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
            assert_eq!(winners.len(), 1, "라운드 {round}: 이긴 쪽이 하나여야 한다 ({} 개)", winners.len());
            for lost in results.iter().filter_map(|r| r.as_ref().err()) {
                assert!(lost.contains("이미 있습니다"), "{lost}");
            }
            let (i, dir) = winners[0];
            let run: Value = serde_json::from_slice(&fs::read(dir.join("run.json")).unwrap()).unwrap();
            assert_eq!(run["title"], json!(format!("스레드 {i}")), "이긴 쪽의 run.json 이 아니다");
            let queue: Value = serde_json::from_slice(&fs::read(dir.join("queue.json")).unwrap()).unwrap();
            assert!(queue.is_array());
        }
    }

    /// 다른 id 로 동시에 출발하면 모두 자기 폴더를 얻는다(서로 막지 않는다)
    #[test]
    fn concurrent_starts_with_different_run_ids_all_succeed() {
        let t = Temp::new();
        let runs = t.runs();
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let (runs, barrier) = (runs.clone(), barrier.clone());
                std::thread::spawn(move || {
                    let req = StartRequest { run_id: format!("r{i}"), ..request() };
                    barrier.wait();
                    write_run_folder(&runs, &req)
                })
            })
            .collect();
        for h in handles {
            let dir = h.join().unwrap().unwrap();
            assert!(dir.join("run.json").is_file() && dir.join("queue.json").is_file());
        }
    }

    /// 부모(runs)가 폴더가 아니라 파일이면 부모를 못 만든다고 알리고 아무것도 지우지 않는다
    #[test]
    fn runs_parent_that_is_a_file_is_reported() {
        let t = Temp::new();
        fs::write(t.runs(), "i am a file").unwrap();
        let why = write_run_folder(&t.runs(), &request()).unwrap_err();
        assert!(why.contains("부모"), "{why}");
        assert_eq!(fs::read_to_string(t.runs()).unwrap(), "i am a file");
    }

    /// 쓴 파일의 모양: 끝 줄바꿈이 있고, 임시 파일(.tmp 등)이 남지 않으며, 다시 읽으면 원래 값이다(한글·이모지·큰따옴표 포함)
    #[test]
    fn written_files_round_trip_unicode_and_leave_no_temp_files() {
        let t = Temp::new();
        let mut req = request();
        req.run["title"] = json!("한글 \u{1F600} \"따옴표\" \\ 역슬래시 \n 줄바꿈");
        req.queue[0]["prompt"] = json!("목표\r\n둘째 줄\t탭 \u{0007}");
        let dir = write_run_folder(&t.runs(), &req).unwrap();
        for name in ["run.json", "queue.json"] {
            let bytes = fs::read(dir.join(name)).unwrap();
            assert!(bytes.ends_with(b"\n"), "{name}");
            assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]), "{name}: BOM 이 붙었다");
        }
        let run: Value = serde_json::from_slice(&fs::read(dir.join("run.json")).unwrap()).unwrap();
        let queue: Value = serde_json::from_slice(&fs::read(dir.join("queue.json")).unwrap()).unwrap();
        assert_eq!(run, req.run);
        assert_eq!(queue, Value::Array(req.queue.clone()));
        let mut names: Vec<String> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["queue.json", "run.json"], "임시 파일이 남았다");
    }

    /// 실행 폴더 경로에 공백이 있어도 명령줄이 한 인자로 받는다
    #[cfg(windows)]
    #[test]
    fn run_folder_path_with_spaces_reaches_the_cli_as_one_argument() {
        let t = Temp::new();
        let cli = fake_cli(&t, r#"{"ok":true,"scheduled":true}"#, 0);
        let runs = t.0.join("my runs dir");
        start_run(&cli, &runs, &request(), Duration::from_secs(20)).unwrap();
        let args = fs::read_to_string(t.0.join("args.txt")).unwrap();
        let want = runs.join("20261106-090503-toy");
        assert!(args.contains(&format!("\"{}\"", want.display())), "공백이 든 경로가 한 인자로 가지 않았다: {args}");
    }

    #[test]
    fn invalid_requests_write_nothing() {
        let t = Temp::new();
        let bad = StartRequest { run_id: "..".to_string(), ..request() };
        assert!(start_run(&t.0.join("none.exe"), &t.runs(), &bad, Duration::from_secs(5)).is_err());
        assert!(!t.runs().exists());
    }
}
