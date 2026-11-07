//! 명령줄 — `claude-autopilot` 의 하위 명령 다섯(`start`·`run`·`control`·`status`·`unregister`). 설계 원본 macOS `main.swift` 의 같은 이름 명령의 Windows 판(MVP).
//!
//! 종료 코드: 0 됨 · 1 거부(문턱·규칙에 걸림) · 2 인자 오류 · 3 내부 오류. `--json` 이면 표준 출력에 JSON 한 줄만, 아니면 쉬운 한국어 줄(오류 글은 표준 오류).
//! 이 파일은 인자 읽기·결과 내기·포트 묶음(`Deps`)·실제 어댑터 조립(`execute`)을 맡고, 명령 본체는 나눠 둔다.
//! - `start` — 문턱(실행 환경·준비)과 가지·고정 설정·작업 스케줄러 등록. 걸리면 이미 바꾼 것만 되돌린다
//! - `control`·`status`·`unregister` — 파일과 작업 스케줄러만 만진다(`inspect`)
//! - `run` — 작업 스케줄러가 1분마다 띄우는 본체. 실제 어댑터를 조립해 `Engine` 에 넘기고 아무것도 출력하지 않는다(로그는 `<실행 폴더>\engine.log`)
//!
//! `start`·`control`·`status`·`unregister` 는 `Deps` 만 만지므로 시험은 가짜 작업 스케줄러·환경 목록을 넣을 수 있다.

mod inspect;
pub mod roots;
mod run;
mod start;

pub use run::exit_code;
pub use start::build_frozen;

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use claude_board_core::envs::{detect, EnvEntry};
use claude_board_core::paths;
use claude_board_core::test_claude::{decide_from_process, Decision};
use serde_json::{json, Value};

use crate::adapters::files::{FsRunFiles, SystemClock};
use crate::adapters::git::ProcessGit;
use crate::adapters::native_env::NativeEnv;
use crate::adapters::supervisor::TaskScheduler;
use crate::ports::{Clock, Git, PortError, PortErrorKind, PortResult, RunFiles, Supervisor};

/// 사용법(표준 오류)
pub const USAGE: &str = "사용법:
  claude-autopilot start <실행 폴더> [--dev] [--accept-unsandboxed] [--no-schedule] [--json]
  claude-autopilot run <실행 폴더> <고정 설정 해시>
  claude-autopilot control <실행 폴더> <pause|resume|stop|restart_from_intake> [--json]
  claude-autopilot status <실행 폴더> [--json]
  claude-autopilot unregister (<실행 폴더> | --all) [--dev] [--json]";

/// 엔진 실행 파일 이름. `start` 가 이 파일을 `pinned\` 로 복사해 작업 스케줄러가 그 사본을 띄운다
pub const ENGINE_EXE: &str = if cfg!(windows) { "claude-autopilot-engine.exe" } else { "claude-autopilot-engine" };

/// 시험 모드에서 `start` 가 frozen `claude_env` 에 고정하는 환경 변수 이름의 머리
const FAKE_ENV_PREFIX: &str = "FAKE_CLAUDE_";

// ── 인자 ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Start { dir: PathBuf, dev: bool, accept_unsandboxed: bool, no_schedule: bool, json: bool },
    Run { dir: PathBuf, hash: String },
    Control { dir: PathBuf, action: String, json: bool },
    Status { dir: PathBuf, json: bool },
    Unregister { target: Target, dev: bool, json: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Folder(PathBuf),
    All,
}

/// 보드가 쓰는 요청 가운데 이 판이 받는 것. `takeover`·`giveback`(터미널로 이어받기)은 Windows 판에 없다
const CONTROL_ACTIONS: [&str; 4] = ["pause", "resume", "stop", "restart_from_intake"];

/// 인자(실행 파일 이름 뒤) → 명령. 틀리면 쉬운 말 한 문장(사용법은 부르는 쪽이 붙인다)
pub fn parse(args: &[String]) -> Result<Command, String> {
    let (name, rest) = args.split_first().ok_or_else(|| "명령이 없습니다.".to_string())?;
    let allowed: &[&str] = match name.as_str() {
        "start" => &["--dev", "--accept-unsandboxed", "--no-schedule", "--json"],
        "run" => &[],
        "control" | "status" => &["--json"],
        "unregister" => &["--dev", "--json", "--all"],
        other => return Err(format!("알 수 없는 명령입니다: {other}")),
    };
    let (flags, pos): (Vec<&String>, Vec<&String>) = rest.iter().partition(|a| a.starts_with("--"));
    if let Some(bad) = flags.iter().find(|f| !allowed.contains(&f.as_str())) {
        return Err(format!("{name} 이(가) 모르는 옵션입니다: {bad}"));
    }
    let has = |f: &str| flags.iter().any(|x| x.as_str() == f);
    let json = has("--json");
    let want = |n: usize, what: &str| {
        if pos.len() == n {
            Ok(())
        } else {
            Err(format!("{name} 에는 {what} 가 필요합니다."))
        }
    };
    match name.as_str() {
        "start" => {
            want(1, "실행 폴더 하나")?;
            Ok(Command::Start {
                dir: PathBuf::from(pos[0]),
                dev: has("--dev"),
                accept_unsandboxed: has("--accept-unsandboxed"),
                no_schedule: has("--no-schedule"),
                json,
            })
        }
        "run" => {
            want(2, "실행 폴더와 고정 설정 해시")?;
            Ok(Command::Run { dir: PathBuf::from(pos[0]), hash: pos[1].clone() })
        }
        "control" => {
            want(2, "실행 폴더와 요청")?;
            let action = pos[1].as_str();
            if !CONTROL_ACTIONS.contains(&action) {
                return Err(format!("받지 않는 요청입니다: {action}(pause·resume·stop·restart_from_intake 중 하나)"));
            }
            Ok(Command::Control { dir: PathBuf::from(pos[0]), action: action.to_string(), json })
        }
        "status" => {
            want(1, "실행 폴더 하나")?;
            Ok(Command::Status { dir: PathBuf::from(pos[0]), json })
        }
        _ => {
            let target = match (has("--all"), pos.as_slice()) {
                (true, []) => Target::All,
                (false, [one]) => Target::Folder(PathBuf::from(one)),
                _ => return Err("unregister 에는 실행 폴더 하나, 또는 --all 이 필요합니다.".to_string()),
            };
            Ok(Command::Unregister { target, dev: has("--dev"), json })
        }
    }
}

// ── 결과 ─────────────────────────────────────────────

/// 한 번의 실행 결과. `main` 이 그대로 내보낸다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn silent(code: i32) -> Self {
        Self { code, stdout: String::new(), stderr: String::new() }
    }
}

struct Reply {
    json: Value,
    text: String,
}

enum Fail {
    /// 인자 오류(2)
    Usage(String),
    /// 문턱·규칙에 걸림(1) — 쉬운 말 목록
    Refused(Vec<String>),
    /// 입출력 등(3)
    Internal(String),
}

fn refuse(msg: impl Into<String>) -> Fail {
    Fail::Refused(vec![msg.into()])
}

/// 실패를 `--json` 으로 낼 때의 꼴
#[derive(Clone, Copy)]
enum Shape {
    Problems,
    Why,
}

fn render(json: bool, shape: Shape, result: Result<Reply, Fail>) -> Outcome {
    let failed = |code: i32, problems: Vec<String>| {
        if json {
            let body = match shape {
                Shape::Problems => json!({"ok": false, "problems": problems}),
                Shape::Why => json!({"ok": false, "why": problems.join(" ")}),
            };
            Outcome { code, stdout: format!("{body}\n"), stderr: String::new() }
        } else {
            Outcome { code, stdout: String::new(), stderr: problems.iter().map(|p| format!("{p}\n")).collect() }
        }
    };
    match result {
        Ok(r) if json => Outcome { code: 0, stdout: format!("{}\n", r.json), stderr: String::new() },
        Ok(r) => Outcome { code: 0, stdout: format!("{}\n", r.text), stderr: String::new() },
        Err(Fail::Usage(m)) => Outcome { code: 2, stdout: String::new(), stderr: format!("{m}\n{USAGE}\n") },
        Err(Fail::Refused(p)) => failed(1, p),
        Err(Fail::Internal(m)) => failed(3, vec![m]),
    }
}

// ── 포트 묶음 ────────────────────────────────────────

/// `start`·`control`·`status`·`unregister` 가 바깥과 말하는 길. 실제는 `execute` 가 조립한다
pub struct Deps<'a> {
    pub files: &'a dyn RunFiles,
    pub git: &'a dyn Git,
    pub clock: &'a dyn Clock,
    /// 개발 판(`--dev`)이면 true 를 받아 그 판의 작업 스케줄러를 낸다
    pub supervisor: &'a dyn Fn(bool) -> Box<dyn Supervisor>,
    /// 이 PC 의 실행 환경 목록
    pub detect: &'a dyn Fn() -> Vec<EnvEntry>,
    /// 시험용 claude 판정
    pub test_claude: &'a dyn Fn() -> Decision,
    /// 시험 모드에서 고정할 환경 변수(`FAKE_CLAUDE_` 로 시작하는 것)
    pub fake_env: &'a dyn Fn() -> Vec<(String, String)>,
    /// `pinned\` 로 복사할 엔진 실행 파일
    pub engine_exe: PathBuf,
    /// 자율주행 데이터 뿌리(`dev` 면 개발 판). 엔진 전용 폴더가 그 아래 `private\<실행 id>` 에 선다
    pub autopilot_root: &'a dyn Fn(bool) -> PathBuf,
    /// `CLAUDE_BOARD_HOME` 이 있는가 — `--no-schedule` 은 이때만 받는다
    pub home_set: bool,
}

fn engine_exe_path() -> PathBuf {
    std::env::current_exe().map_or_else(|_| PathBuf::from(ENGINE_EXE), |p| p.with_file_name(ENGINE_EXE))
}

fn fake_claude_env() -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> =
        std::env::vars().filter(|(k, _)| k.starts_with(FAKE_ENV_PREFIX)).collect();
    vars.sort();
    vars
}

/// 인자(실행 파일 이름 뒤)를 실제 어댑터로 실행한다
pub fn execute(args: &[String]) -> Outcome {
    let command = match parse(args) {
        Ok(c) => c,
        Err(m) => return render(false, Shape::Problems, Err(Fail::Usage(m))),
    };
    if let Command::Run { dir, hash } = command {
        return Outcome::silent(run::run_engine(&absolute(&dir), &hash));
    }
    let files = FsRunFiles::new();
    let git = ProcessGit::new();
    let clock = SystemClock;
    let deps = Deps {
        files: &files,
        git: &git,
        clock: &clock,
        supervisor: &|dev: bool| -> Box<dyn Supervisor> { Box::new(TaskScheduler::new(dev)) },
        detect: &detect,
        test_claude: &decide_from_process,
        fake_env: &fake_claude_env,
        engine_exe: engine_exe_path(),
        autopilot_root: &paths::autopilot_root,
        home_set: std::env::var_os(paths::HOME_ENV).is_some_and(|v| !v.is_empty()),
    };
    execute_with(command, &deps)
}

/// `Run` 이 아닌 명령을 주어진 포트로 실행한다
pub fn execute_with(command: Command, d: &Deps) -> Outcome {
    match command {
        Command::Start { dir, dev, accept_unsandboxed, no_schedule, json } => {
            render(json, Shape::Problems, start::start(d, &absolute(&dir), dev, accept_unsandboxed, no_schedule))
        }
        Command::Control { dir, action, json } => render(json, Shape::Why, inspect::control(d, &absolute(&dir), &action)),
        Command::Status { dir, json } => render(json, Shape::Problems, inspect::status(d, &absolute(&dir))),
        Command::Unregister { target, dev, json } => render(json, Shape::Problems, inspect::unregister(d, &target, dev)),
        Command::Run { .. } => Outcome::silent(2),
    }
}

/// 실행 파일의 `main` — 인자(실행 파일 이름 뒤)를 받아 출력하고 종료 코드를 돌려준다
pub fn main(args: Vec<String>) -> i32 {
    let out = catch_unwind(AssertUnwindSafe(|| execute(&args))).unwrap_or_else(|p| Outcome {
        code: 3,
        stdout: String::new(),
        stderr: format!("내부 오류: {}\n", panic_text(p.as_ref())),
    });
    use std::io::Write;
    let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    out.code
}

fn panic_text(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "알 수 없는 패닉".to_string())
}

// ── 이름 ─────────────────────────────────────────────

/// 실행 id 규칙 `[A-Za-z0-9._-]{1,120}`(점으로만 된 이름과 점으로 끝나는 이름 제외: Windows 가 끝점을 떼어 폴더 이름이 달라진다)
pub fn valid_run_id(id: &str) -> bool {
    (1..=120).contains(&id.len())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && id.chars().any(|c| c != '.')
        && !id.ends_with('.')
}

/// 작업 이름: `com.claude.board[.dev].autopilot.<실행 id>`. run.json `launchd_label` 칸에 둔다
pub fn label_for(dev: bool, id: &str) -> String {
    format!("com.claude.board{}.autopilot.{id}", if dev { ".dev" } else { "" })
}

/// 작업 이름: `com.claude.board[.dev].autopilot.<실행 id>.<경로 해시 8자>`. 해시는 실행 폴더의 정규 전체 경로(소문자)의 sha256 앞 4바이트다.
/// 부모 폴더만 다른 같은 이름의 실행 폴더 둘이 작업 이름을 나눠 갖지 않게 한다(`start` 가 쓰고 run.json `launchd_label` 에 둔다)
pub fn label_for_dir(dev: bool, dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let canon = std::fs::canonicalize(dir).map(|p| adapters_strip(&p)).unwrap_or_else(|_| absolute(dir));
    let digest = Sha256::digest(canon.to_string_lossy().to_lowercase().as_bytes());
    let short: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{}.{short}", label_for(dev, &run_id_of(dir)))
}

fn adapters_strip(p: &Path) -> PathBuf {
    crate::adapters::winproc::strip_verbatim(p)
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn run_id_of(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// 읽기 실패 → 꼴이 틀리면 거부(1), 입출력이면 내부 오류(3)
fn read_fail(name: &str, e: PortError) -> Fail {
    match e.kind {
        PortErrorKind::Invalid => refuse(format!("{name} 을(를) 읽지 못했습니다: JSON 이 아닙니다.")),
        _ => Fail::Internal(format!("{name} 을(를) 읽지 못했습니다: {}", e.message)),
    }
}

fn read_required(d: &Deps, path: &Path, name: &str) -> Result<Value, Fail> {
    match d.files.read_json(path) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(refuse(format!("{name} 이(가) 없습니다."))),
        Err(e) => Err(read_fail(name, e)),
    }
}

/// `claude_cmd`(조각 배열)로 네이티브 환경 어댑터를 만든다. 없으면 실제 claude.exe 를 찾는다
pub(super) fn native_env(entry: &EnvEntry, claude_cmd: Option<&[String]>) -> PortResult<NativeEnv> {
    let env = NativeEnv::new(entry.choice.clone(), entry.program.clone())?;
    Ok(match claude_cmd.and_then(|c| c.split_first()) {
        Some((program, prefix)) => env.with_claude_program(PathBuf::from(program), prefix.to_vec()),
        None => env,
    })
}
