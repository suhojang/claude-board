//! 읽기 전용 모델 세션 실행기 — 단위가 아닌 짧은 claude 세션(출발 준비의 초안·점검·비평·보고)을 돌린다.
//! 단위 실행(`engine::run`)과 같은 길로 claude 를 띄운다: `ExecEnv::spawn_claude`(네이티브·WSL 둘 다), 줄 읽기는 `ChildProcess`,
//! 줄 해석은 `stream::parse_line`. 이 파일이 더하는 것은 읽기 전용 정책·시간 상한·취소·결과 꼴뿐이다.
//!
//! - 도구는 읽기 셋(`READ_TOOLS`)만 쓴다. `--tools`·`--allowedTools` 가 그 셋이고 `--disallowedTools` 가 쓰기·실행·웹·하위 에이전트를 막는다
//!   (`DENIED_TOOLS`). `--strict-mcp-config` 로 MCP 도 없다. 같은 규칙을 `--settings` 파일에도 적는다.
//! - 지시 글은 명령줄이 아니라 표준 입력의 stream-json 한 줄로 넘긴다(Windows 명령줄 한도·따옴표 문제).
//! - 작업 폴더에는 아무것도 쓰지 않는다. 설정·stderr 기록은 호출자가 준 `scratch_dir` 에만 쓴다.
//! - 환경 변수는 어댑터(`winproc::build_env`)가 허용 목록으로 걸러 물려준다. 여기서는 `env` 로 받은 쌍만 얹는다.
//! - 시간 상한·취소에 닿으면 `kill_tree` 로 자손까지 끝낸다(Job Object). 어떤 줄이 와도 멈추지 않는다.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::adapters::native_env::read_tail;
use crate::adapters::winproc::is_secret_name;
use crate::engine::run::session_uuid;
use crate::ports::{ClaudeLaunch, ExecEnv, PortErrorKind, SessionPolicy, SessionStart};
use crate::rules::env_policy::OutputCodec;
use crate::stream::{parse_line, ResultEvent, StreamEvent};

/// 이 세션이 쓸 수 있는 도구
pub const READ_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
/// 거부하는 도구 — 쓰기·실행·웹·하위 에이전트
pub const DENIED_TOOLS: &[&str] =
    &["Bash", "PowerShell", "Edit", "MultiEdit", "Write", "NotebookEdit", "WebFetch", "WebSearch", "Task", "Agent"];

/// 이보다 긴 줄이 오면 더 읽지 않고 세션을 끝낸다(바이트)
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// 줄 읽기를 한 번에 기다리는 시간. 취소·시간 상한을 이 간격으로 본다
const POLL: Duration = Duration::from_millis(100);
/// 출력이 끝난 뒤 종료 코드를 기다리는 시간
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// 실패 사유에 싣는 stderr 길이(글자 수)
const REASON_CHARS: usize = 400;
/// stderr 끝에서 읽는 바이트
const STDERR_TAIL_BYTES: u64 = 4000;

/// 세션 한 번의 요청
#[derive(Debug, Clone)]
pub struct SessionRequest {
    /// 모델에게 줄 지시 글(표준 입력으로 간다)
    pub prompt: String,
    /// 작업 폴더. 읽기만 한다
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    /// `--max-budget-usd`
    pub max_budget_usd: f64,
    /// `--max-turns`
    pub max_turns: u32,
    /// 벽시계 상한
    pub timeout: Duration,
    /// 설정 파일과 stderr 기록을 두는 엔진 전용 폴더(작업 폴더 밖). 없으면 만든다
    pub scratch_dir: PathBuf,
    /// 엔진이 고른 환경 변수(어댑터의 허용 목록 위에 얹는다)
    pub env: Vec<(String, String)>,
    /// 이보다 긴 출력 줄이 오면 세션을 끝낸다(바이트). 보통 `MAX_LINE_BYTES`
    pub max_line_bytes: usize,
}

/// 결과. 패닉 없이 이 넷 중 하나로 끝난다
#[derive(Debug, Clone, PartialEq)]
pub enum SessionOutcome {
    Done { text: String, cost: f64, turns: i64, session_id: String },
    TimedOut,
    Cancelled,
    Failed { reason: String },
}

/// 밖에서 세션을 멈추는 손잡이(복제해 나눠 쥔다)
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 읽기 전용 모델 세션 실행기. 실행 환경은 엔진이 고른 것(`ExecEnv`)을 그대로 쓴다
pub struct SessionRunner<'a> {
    env: &'a dyn ExecEnv,
}

/// claude 에 줄 읽기 전용 추가 인자(`ClaudeLaunch.extra_args`)
pub fn readonly_args(req: &SessionRequest) -> Vec<String> {
    let mut args: Vec<String> =
        ["--permission-mode", "dontAsk", "--permission-prompts", "none", "--strict-mcp-config"].map(String::from).to_vec();
    args.extend(["--tools".to_string(), READ_TOOLS.join(",")]);
    args.extend(["--allowedTools".to_string(), READ_TOOLS.join(" ")]);
    args.extend(["--disallowedTools".to_string(), DENIED_TOOLS.join(" ")]);
    args.extend(["--max-budget-usd".to_string(), format!("{:.2}", req.max_budget_usd.max(0.01))]);
    args.extend(["--max-turns".to_string(), req.max_turns.max(1).to_string()]);
    args
}

/// 표준 입력에 넣는 사용자 차례 한 줄
pub fn prompt_line(prompt: &str) -> String {
    json!({ "type": "user", "message": { "role": "user", "content": prompt } }).to_string()
}

/// 사유에 비밀이 실려 나가지 않게 가린다: 엔진이 준 비밀 값 그대로, `이름=값`(이름에 비밀 낱말), `sk-…`·`Bearer` 로 시작하는 낱말
pub fn redact(text: &str, env: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (_, value) in env.iter().filter(|(n, v)| is_secret_name(n) && v.chars().count() >= 4) {
        out = out.replace(value.as_str(), "***");
    }
    let words: Vec<String> = out
        .split(' ')
        .map(|w| match w.split_once('=') {
            Some((name, _)) if is_secret_name(name) => format!("{name}=***"),
            _ if w.starts_with("sk-") || w.starts_with("Bearer") => "***".to_string(),
            _ => w.to_string(),
        })
        .collect();
    words.join(" ")
}

pub(crate) fn fail(reason: impl Into<String>) -> SessionOutcome {
    SessionOutcome::Failed { reason: reason.into() }
}

pub(crate) fn unique_session_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let seed = format!("session-runner-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    session_uuid(seed.as_bytes())
}

impl<'a> SessionRunner<'a> {
    pub fn new(env: &'a dyn ExecEnv) -> Self {
        Self { env }
    }

    /// 세션 하나를 끝까지 돌린다. 끝나거나 시간·취소에 닿을 때까지 막힌다
    pub fn run(&self, req: &SessionRequest, cancel: &CancelToken) -> SessionOutcome {
        if cancel.is_cancelled() {
            return SessionOutcome::Cancelled;
        }
        let launch = match self.prepare(req) {
            Ok(l) => l,
            Err(why) => return fail(why),
        };
        run_launch(self.env, &launch, req, cancel)
    }

    /// 설정 파일을 엔진 전용 폴더에 쓰고 띄울 꼴을 만든다
    fn prepare(&self, req: &SessionRequest) -> Result<ClaudeLaunch, String> {
        std::fs::create_dir_all(&req.scratch_dir).map_err(|e| format!("엔진 폴더를 만들지 못했다: {e}"))?;
        let policy = SessionPolicy {
            allowed_tools: READ_TOOLS.iter().map(|s| s.to_string()).collect(),
            disallowed_tools: DENIED_TOOLS.iter().map(|s| s.to_string()).collect(),
            ..SessionPolicy::default()
        };
        let settings = self.env.render_settings(&policy).map_err(|e| format!("설정을 만들지 못했다: {}", e.message))?;
        let settings_file = req.scratch_dir.join("settings.json");
        std::fs::write(&settings_file, settings.to_string()).map_err(|e| format!("설정 파일을 쓰지 못했다: {e}"))?;
        Ok(ClaudeLaunch {
            session: SessionStart::New(unique_session_id()),
            cwd: req.cwd.clone(),
            model: req.model.clone(),
            effort: req.effort.clone(),
            settings_file,
            system_prompt_file: None,
            add_dirs: Vec::new(),
            plugin_dir: None,
            env: req.env.clone(),
            path_env: Vec::new(),
            extra_args: readonly_args(req),
            stderr_log: stderr_path(&req.scratch_dir),
        })
    }
}

pub(crate) fn stderr_path(dir: &Path) -> PathBuf {
    dir.join("stderr.log")
}

fn judge(r: ResultEvent, env: &[(String, String)]) -> SessionOutcome {
    if r.subtype == "success" && !r.is_error {
        return SessionOutcome::Done {
            text: r.text,
            cost: r.total_cost_usd.unwrap_or(0.0),
            turns: r.num_turns,
            session_id: r.session_id.unwrap_or_default(),
        };
    }
    let detail = r.api_error.filter(|s| !s.is_empty()).unwrap_or(r.text);
    let reason = if detail.is_empty() { format!("결과 {}", r.subtype) } else { format!("결과 {}: {detail}", r.subtype) };
    fail(redact(&reason, env))
}

/// 정해진 띄울 꼴로 claude 를 띄우고 끝까지 돌린다 — 읽기 세션(`SessionRunner::run`)과 결과 폴더 세션
/// (`session_limits::run_out_session`)이 같은 루프·시간 상한·취소·kill_tree 를 쓴다. 띄우기부터 판정까지 여기서 한다
pub(crate) fn run_launch(env: &dyn ExecEnv, launch: &ClaudeLaunch, req: &SessionRequest, cancel: &CancelToken) -> SessionOutcome {
    let mut child = match env.spawn_claude(launch) {
        Ok(c) => c,
        Err(e) => return fail(redact(&format!("claude 를 띄우지 못했다: {}", e.message), &req.env)),
    };
    let deadline = Instant::now() + req.timeout;
    if let Err(e) = child.write_line(&prompt_line(&req.prompt)) {
        let _ = child.kill_tree();
        return fail(format!("지시 글을 보내지 못했다: {}", e.message));
    }
    child.close_stdin();

    let mut result = None;
    let early = loop {
        if cancel.is_cancelled() {
            break Some(SessionOutcome::Cancelled);
        }
        let now = Instant::now();
        if now >= deadline {
            break Some(SessionOutcome::TimedOut);
        }
        match child.read_line((deadline - now).min(POLL)) {
            Ok(Some(line)) => {
                if line.len() > req.max_line_bytes {
                    break Some(fail(format!("출력 줄이 상한({} 바이트)을 넘어 세션을 끝냈다", req.max_line_bytes)));
                }
                if let StreamEvent::Result(r) = parse_line(&line) {
                    result = Some(r);
                }
            }
            Ok(None) => break None,
            Err(e) if e.kind == PortErrorKind::Timeout => {}
            Err(e) => break Some(fail(format!("출력을 읽지 못했다: {}", e.message))),
        }
    };
    if let Some(outcome) = early {
        let _ = child.kill_tree();
        return outcome;
    }
    // 출력이 끝났다 — 종료 코드를 잠깐 기다리고, 남은 자손까지 끝낸다
    let exit_deadline = Instant::now() + EXIT_WAIT;
    let mut exit = None;
    while Instant::now() < exit_deadline {
        match child.try_wait() {
            Ok(Some(code)) => {
                exit = Some(code);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    let _ = child.kill_tree();
    if let Some(r) = result {
        return judge(r, &req.env);
    }
    let tail = redact(read_tail(&launch.stderr_log, STDERR_TAIL_BYTES, OutputCodec::Utf8).trim(), &req.env);
    let skip = tail.chars().count().saturating_sub(REASON_CHARS);
    let tail: String = tail.chars().skip(skip).collect();
    let why = match exit {
        Some(0) => "claude 가 결과 없이 끝났다".to_string(),
        Some(code) => format!("claude 가 종료 코드 {code} 로 끝났다"),
        None => "claude 가 결과 없이 출력을 닫았다".to_string(),
    };
    fail(if tail.is_empty() { why } else { format!("{why}: {tail}") })
}
