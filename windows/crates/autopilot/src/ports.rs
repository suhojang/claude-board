//! 포트 — 엔진(engine::run·cli)이 바깥(파일·실행 환경·git·SQLite·시각·비밀·HTTP·작업 스케줄러)과 말하는 유일한 길.
//! 규칙과 상태 기계는 이 트레이트를 부르지 않는다. 시험은 같은 트레이트의 가짜 구현으로 한다.
//! 시각은 macOS 와 같이 유닉스 초(f64). 경로 인자는 늘 Windows 경로이고, 실행 환경 쪽 경로로 바꾸는 일은 ExecEnv 만 한다.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use claude_board_core::envs::{EnvChoice, EnvKind};

/// 포트 오류의 종류. 엔진은 종류로만 갈래를 정하고 `message` 는 진행 기록·로그에만 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortErrorKind {
    /// 대상이 없다(파일·배포판·실행 파일·작업)
    NotFound,
    /// 남이 쥐고 있다(잠금·SQLite busy). 잠시 뒤 다시 하면 될 수 있다
    Busy,
    /// 정한 시간 안에 끝나지 않았다
    Timeout,
    /// 이 PC·환경에서 쓸 수 없다(WSL 없음, 샌드박스를 걸 수 없음). 출발 거부 사유
    Unavailable,
    /// 권한·보호 규칙이 막았다
    Denied,
    /// 입력 꼴이 틀렸다(경로 변환 불가, JSON 꼴)
    Invalid,
    /// 그 밖의 입출력 실패
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortError {
    pub kind: PortErrorKind,
    pub message: String,
}

impl PortError {
    pub fn new(kind: PortErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for PortError {}

impl From<std::io::Error> for PortError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        let kind = match e.kind() {
            K::NotFound => PortErrorKind::NotFound,
            K::PermissionDenied => PortErrorKind::Denied,
            K::TimedOut => PortErrorKind::Timeout,
            K::WouldBlock => PortErrorKind::Busy,
            K::InvalidInput | K::InvalidData => PortErrorKind::Invalid,
            _ => PortErrorKind::Io,
        };
        PortError::new(kind, e.to_string())
    }
}

pub type PortResult<T> = Result<T, PortError>;

/// SQLite 한 행, 또는 JSON 객체 하나.
pub type Row = serde_json::Map<String, serde_json::Value>;

// ── 시각 ─────────────────────────────────────────────

pub trait Clock: Send + Sync {
    /// 유닉스 초
    fn now(&self) -> f64;
    fn sleep(&self, d: Duration);
}

// ── 실행 폴더 파일 ───────────────────────────────────

/// 잠금을 쥔 동안 살아 있는 값. 떨어뜨리면 풀린다.
pub trait Held: Send {}

pub trait RunFiles: Send + Sync {
    /// 없으면 Ok(None). JSON 이 아니면 Invalid.
    fn read_json(&self, path: &Path) -> PortResult<Option<serde_json::Value>>;
    /// 같은 폴더의 임시 파일에 쓰고 이름을 바꾼다(반쯤 쓴 파일을 보드가 읽지 않게).
    fn write_json(&self, path: &Path, value: &serde_json::Value) -> PortResult<()>;
    /// 한 줄 덧붙이기(`events.jsonl`·`context.jsonl`·`starts.log`). 줄 끝 `\n` 은 어댑터가 붙인다.
    fn append_line(&self, path: &Path, line: &str) -> PortResult<()>;
    /// 없어도 Ok.
    fn remove(&self, path: &Path) -> PortResult<()>;
    /// 폴더 안 이름(정렬). 폴더가 없으면 빈 목록.
    fn list(&self, dir: &Path) -> PortResult<Vec<String>>;
    /// 상속을 끄고 지금 사용자만 접근하는 폴더(macOS 0700 대응). 이미 있으면 권한만 맞춘다.
    fn make_private_dir(&self, dir: &Path) -> PortResult<()>;
    /// 배타 잠금(`.engine.lock`). 남이 쥐었으면 Ok(None).
    fn try_lock(&self, path: &Path) -> PortResult<Option<Box<dyn Held>>>;
    /// 파일 크기와 수정 시각(유닉스 초). 없으면 Ok(None)
    fn stat(&self, path: &Path) -> PortResult<Option<FileStat>>;
    /// `offset` 부터 끝까지 읽되 마지막 `max` 바이트만. 없으면 Ok(None). 글자 중간에서 잘릴 수 있다(부르는 쪽이 lossy 로 푼다)
    fn read_from(&self, path: &Path, offset: u64, max: usize) -> PortResult<Option<Vec<u8>>>;
}

/// `RunFiles::stat` 의 결과.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileStat {
    pub len: u64,
    pub modified: f64,
}

// ── 실행 환경 ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStart {
    /// `--session-id <uuid>`
    New(String),
    /// `--resume <uuid>`
    Resume(String),
}

/// 모델 세션이 할 수 있는 일의 범위. 경로는 Windows 경로로 주고,
/// 환경 쪽 경로와 권한 규칙 꼴로 바꾸는 일은 `ExecEnv::render_settings` 가 한다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPolicy {
    pub allow_write: Vec<PathBuf>,
    pub deny_write: Vec<PathBuf>,
    /// 파일 도구와(샌드박스가 있으면) Bash 모두에서 읽기 금지
    pub deny_read: Vec<PathBuf>,
    /// 샌드박스가 있는 환경에서만 의미가 있다
    pub network_allow: Vec<String>,
    pub local_binding: bool,
    /// 경로가 없는 규칙만(`Bash(npm test:*)` 등). 경로 규칙은 위 칸으로
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
}

/// claude 한 번 띄우기. 긴 글은 파일로 넘긴다(Windows 명령줄은 32,767자 한도).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeLaunch {
    pub session: SessionStart,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    /// `render_settings` 결과를 엔진이 엔진 전용 폴더에 쓴 파일
    pub settings_file: PathBuf,
    /// `--append-system-prompt-file`
    pub system_prompt_file: Option<PathBuf>,
    /// `--add-dir`
    pub add_dirs: Vec<PathBuf>,
    /// `--plugin-dir`
    pub plugin_dir: Option<PathBuf>,
    /// 넘길 환경 변수(고른 것만). 값이 경로인 변수는 이름을 `path_env` 에도 넣는다
    pub env: Vec<(String, String)>,
    /// 환경 쪽 경로로 바꿔 넘길 변수 이름(WSL 은 WSLENV 의 `/p`)
    pub path_env: Vec<String>,
    /// 경로가 없는 추가 인자(시험 빌드의 `claude_args`)
    pub extra_args: Vec<String>,
    pub stderr_log: PathBuf,
}

/// 판정 명령 한 번. `command` 는 사용자가 쓴 글자 그대로이고, 그 환경의 셸 문법으로 돈다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub command: String,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub path_env: Vec<String>,
    pub timeout: Duration,
    /// 표준 출력·오류를 함께 쓸 파일(`evidence/check-<k>/output.log`)
    pub output_log: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    /// 시간 상한으로 끊었으면 None
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub seconds: f64,
    /// 출력 끝부분(UTF-8 로 바꾼 것)
    pub tail: String,
}

/// 출발 전 확인 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvReport {
    pub ready: bool,
    /// 이 환경에서 Claude Code 샌드박스가 걸리는가(WSL 에서 실제로 잰 값. 네이티브는 늘 false)
    pub sandbox: bool,
    pub claude_version: Option<String>,
    /// 쉬운 말 문제 목록(ready 가 false 면 하나 이상)
    pub problems: Vec<String>,
}

/// 실행 환경이 띄운 claude 하나.
pub trait ChildProcess: Send {
    /// Windows 쪽 pid(WSL 이면 wsl.exe)
    fn pid(&self) -> u32;
    fn write_line(&mut self, line: &str) -> PortResult<()>;
    fn close_stdin(&mut self);
    /// stdout 한 줄. Ok(None) = 끝(EOF). 시간 안에 줄이 없으면 Timeout.
    fn read_line(&mut self, timeout: Duration) -> PortResult<Option<String>>;
    fn try_wait(&mut self) -> PortResult<Option<i32>>;
    /// 점잖게 멈추기(stream-json interrupt, WSL 은 그 안의 SIGINT)
    fn interrupt(&mut self) -> PortResult<()>;
    /// 자손까지 끝내기(Job Object, WSL 은 그 안의 프로세스 그룹까지)
    fn kill_tree(&mut self) -> PortResult<()>;
}

/// 고른 실행 환경. claude 띄우기·판정 돌리기·경로 바꾸기·프로세스 끝내기를 맡는다.
/// 어댑터는 둘: `native_env`(pwsh·powershell·cmd·git-bash, claude 는 claude.exe)와 `wsl_env`.
pub trait ExecEnv: Send + Sync {
    fn choice(&self) -> &EnvChoice;
    /// 출발 전에만 부른다(WSL 은 VM 을 띄운다). claude 가 있는지, 샌드박스가 걸리는지.
    fn check_ready(&self, cwd: &Path) -> EnvReport;
    /// Windows 경로 → 환경 쪽 경로(네이티브는 그대로, WSL 은 `/mnt/c/...` 또는 `\\wsl.localhost\<배포판>\...` → `/...`).
    fn to_env_path(&self, path: &Path) -> PortResult<String>;
    /// 환경 쪽 경로 → Windows 경로(stream 의 도구 입력을 기록할 때).
    fn env_path_to_windows(&self, path: &str) -> PortResult<PathBuf>;
    /// `--settings` 파일 내용. WSL 은 `sandbox.enabled`·`failIfUnavailable`·`allowUnsandboxedCommands: false`.
    /// 네이티브는 `sandbox.enabled: false` 와 권한 규칙만(failIfUnavailable 을 넣으면 claude 가 시작하자마자 끝난다).
    fn render_settings(&self, policy: &SessionPolicy) -> PortResult<serde_json::Value>;
    fn spawn_claude(&self, launch: &ClaudeLaunch) -> PortResult<Box<dyn ChildProcess>>;
    /// 판정 명령을 그 환경의 셸로 돌린다. 시간 상한에 닿으면 자손까지 끝내고 `timed_out`.
    fn run_check(&self, run: &CheckRun) -> PortResult<CheckOutcome>;
}

// ── git(엔진 쪽, 늘 Windows git.exe) ─────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait Git: Send + Sync {
    /// 보호 환경을 늘 건다: `core.fsmonitor=false`, `core.hooksPath` 비움, `GIT_CONFIG_NOSYSTEM=1`, 전역 설정 끔.
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput>;
}

// ── 실행 기록 저장소(SQLite, 판 4) ────────────────────

pub trait Records: Send + Sync {
    /// `PRAGMA user_version`
    fn version(&self) -> PortResult<i32>;
    /// 키가 같으면 덮어쓴다. 표·칸 이름은 `rules::records` 의 표 정의를 벗어나면 Invalid.
    fn upsert(&self, table: &str, row: &Row) -> PortResult<()>;
    fn select(&self, sql: &str, args: &[serde_json::Value]) -> PortResult<Vec<Row>>;
}

// ── 계정 사용률(stream 에 없을 때의 대체) ─────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageFrom {
    Stream,
    Cache,
    Api,
    Toy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageSnapshot {
    /// 0~1
    pub five_hour: f64,
    pub five_hour_resets: Option<f64>,
    pub seven_day: f64,
    pub seven_day_resets: Option<f64>,
    pub at: f64,
    pub from: UsageFrom,
}

pub trait Usage: Send + Sync {
    /// 캐시(15분 안) → API 차례. 못 알면 Ok(None).
    fn fallback(&self, now: f64) -> PortResult<Option<UsageSnapshot>>;
}

// ── 다시 띄우기(작업 스케줄러) ───────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepAlive {
    /// 작업 이름. 실행 id 는 `[A-Za-z0-9._-]` 만
    pub label: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

pub trait Supervisor: Send + Sync {
    /// 로그온 + 1분 반복, 이미 돌고 있으면 새로 띄우지 않음. 관리자 권한 없이.
    fn register(&self, job: &KeepAlive) -> PortResult<()>;
    /// 없어도 Ok.
    fn unregister(&self, label: &str) -> PortResult<()>;
    fn is_registered(&self, label: &str) -> PortResult<bool>;
    fn list(&self) -> PortResult<Vec<String>>;
}

// ── 비밀(DPAPI 금고, 보드 accounts 와 같은 꼴) ────────

pub trait Secrets: Send + Sync {
    fn get(&self, name: &str) -> PortResult<Option<Vec<u8>>>;
    fn put(&self, name: &str, secret: &[u8]) -> PortResult<()>;
    fn delete(&self, name: &str) -> PortResult<()>;
}

// ── HTTP ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub trait Http: Send + Sync {
    fn send(&self, req: &HttpRequest) -> PortResult<HttpResponse>;
}
