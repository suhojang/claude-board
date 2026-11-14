//! `run` — 작업 스케줄러가 1분마다 띄우는 엔진 본체. 고정 설정에서 실행 환경을 되살려 실제 어댑터를 조립하고 `Engine` 에 넘긴다.
//! 아무것도 출력하지 않는다. 어긋난 끝은 `<실행 폴더>/engine.log` 에 남기고, 패닉도 종료 코드 3 으로 닫는다.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_core::envs::{detect, EnvChoice, EnvEntry, EnvKind};
use claude_board_core::paths;
use serde_json::Value;

use super::{native_env, panic_text, run_id_of, ENGINE_EXE};
use crate::adapters::files::{FsRunFiles, SystemClock};
use crate::adapters::git::ProcessGit;
use crate::adapters::http::UreqHttp;
use crate::adapters::records_sqlite::SqliteRecords;
use crate::adapters::supervisor::TaskScheduler;
use crate::adapters::usage::FallbackUsage;
use crate::adapters::wsl_env::WslEnv;
use crate::engine::run::{Engine, Ports, RunConfig, RunExit, FROZEN_CHANGED_REASON};
use crate::ports::{
    CheckOutcome, CheckRun, ChildProcess, ClaudeLaunch, Clock, EnvReport, ExecEnv, PortError, PortErrorKind,
    PortResult, Records, RunFiles, SessionPolicy,
};
use crate::rules::stopline::Tz;

/// 실행 환경을 만들지 못했을 때의 자리. 엔진이 claude 를 띄우는 자리에서 까닭과 함께 실패해 run.json 에 남게 한다
struct BrokenEnv {
    choice: EnvChoice,
    why: String,
}

impl BrokenEnv {
    fn err<T>(&self) -> PortResult<T> {
        Err(PortError::new(PortErrorKind::Unavailable, self.why.clone()))
    }
}

impl ExecEnv for BrokenEnv {
    fn choice(&self) -> &EnvChoice {
        &self.choice
    }
    fn check_ready(&self, _cwd: &Path) -> EnvReport {
        EnvReport { ready: false, sandbox: false, claude_version: None, problems: vec![self.why.clone()] }
    }
    fn to_env_path(&self, path: &Path) -> PortResult<String> {
        Ok(path.to_string_lossy().into_owned())
    }
    fn env_path_to_windows(&self, path: &str) -> PortResult<PathBuf> {
        Ok(PathBuf::from(path))
    }
    fn render_settings(&self, _policy: &SessionPolicy) -> PortResult<Value> {
        self.err()
    }
    fn spawn_claude(&self, _launch: &ClaudeLaunch) -> PortResult<Box<dyn ChildProcess>> {
        self.err()
    }
    fn run_check(&self, _run: &CheckRun) -> PortResult<CheckOutcome> {
        self.err()
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// 고정 설정의 `exec_env`·`claude_cmd` 로 환경 어댑터를 만든다. 못 만들면 까닭을 품은 `BrokenEnv`
fn engine_env(frozen: &Value, entries: &[EnvEntry]) -> Box<dyn ExecEnv> {
    let broken = |choice: Option<EnvChoice>, why: &str| -> Box<dyn ExecEnv> {
        Box::new(BrokenEnv { choice: choice.unwrap_or(EnvChoice { kind: EnvKind::Cmd, distro: None }), why: why.to_string() })
    };
    let key = frozen.get("exec_env").and_then(|e| e.get("key")).and_then(Value::as_str).unwrap_or("");
    let Some(choice) = EnvChoice::parse(key) else { return broken(None, &format!("고정 설정의 실행 환경({key})을 알 수 없습니다.")) };
    let Some(entry) = entries.iter().find(|e| e.choice == choice) else {
        return broken(Some(choice.clone()), &format!("이 PC 에 없는 실행 환경({})입니다.", choice.key()));
    };
    if choice.kind == EnvKind::Wsl {
        return match WslEnv::from_entry(entry) {
            Ok(env) => Box::new(env),
            Err(e) => broken(Some(choice), &e.message),
        };
    }
    let cmd = strings(frozen.get("claude_cmd"));
    match native_env(entry, (!cmd.is_empty()).then_some(cmd.as_slice())) {
        Ok(env) => Box::new(env),
        Err(e) => broken(Some(choice), &e.message),
    }
}

fn log_stamp(now: f64) -> String {
    let t = claude_board_core::clock::local(now);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

fn engine_log(dir: &Path, text: &str) {
    let line = format!("{}\t{text}", log_stamp(SystemClock.now()));
    let _ = FsRunFiles::new().append_line(&dir.join("engine.log"), &line);
}

/// 엔진 종료 코드: 끝났거나 이미 다른 엔진이 돌면 0, 고정 설정이 바뀌었거나 읽지 못하면 3
pub fn exit_code(exit: &RunExit) -> i32 {
    match exit {
        RunExit::Finished(_) | RunExit::LockHeld => 0,
        RunExit::FrozenChanged | RunExit::LoadFailed(_) => 3,
    }
}

fn run_inner(dir: &Path, hash: &str) -> RunExit {
    let files = FsRunFiles::new();
    let frozen = files.read_json(&dir.join("pinned").join("frozen.json")).ok().flatten().unwrap_or(Value::Null);
    let dev = frozen.get("dev").and_then(Value::as_bool).unwrap_or(false);
    let root = paths::autopilot_root(dev);
    let env = engine_env(&frozen, &detect());
    let git = ProcessGit::new();
    let records = SqliteRecords::open(&root.join("private").join("records.sqlite")).ok();
    let usage = FallbackUsage::system(UreqHttp::new());
    let clock = SystemClock;
    let supervisor = TaskScheduler::new(dev);
    // 직전 엔진이 남긴 claude 를 끝낸다. 생성 시각이 없으면 같은 pid 의 다른 프로세스일 수 있어 건드리지 않는다
    let reap = |pid: u32, ctime: Option<f64>| ctime.is_some() && claude_board_core::proc::alive(pid, ctime) && claude_board_core::proc::terminate(pid);
    let ports = Ports {
        files: &files,
        env: &*env,
        git: &git,
        records: records.as_ref().map(|r| r as &dyn Records),
        usage: &usage,
        clock: &clock,
        supervisor: &supervisor,
        reap: &reap,
    };
    let cfg = RunConfig {
        dir: dir.to_path_buf(),
        private_dir: root.join("private").join(run_id_of(dir)),
        frozen_hash: hash.to_string(),
        tz: Tz::Local,
        tick: Duration::from_secs(1),
        poll: Duration::from_millis(200),
        claude_env_extra: frozen
            .get("claude_env")
            .and_then(Value::as_object)
            .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default(),
        pid: std::process::id(),
        exe: std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| ENGINE_EXE.to_string()),
    };
    match Engine::open(ports, cfg) {
        Ok(engine) => engine.run(),
        Err(exit) => exit,
    }
}

/// 엔진 본체. 아무것도 출력하지 않고 어긋난 끝은 `engine.log` 에 남긴다. 패닉도 종료 코드 3 이다
/// (작업 스케줄러는 종료 코드와 상관없이 1분 뒤 다시 띄우고, 다시 뜬 엔진이 starts.log 로 비정상을 센다)
pub(super) fn run_engine(dir: &Path, hash: &str) -> i32 {
    if !dir.is_dir() {
        return 3;
    }
    match catch_unwind(AssertUnwindSafe(|| run_inner(dir, hash))) {
        Ok(exit) => {
            match &exit {
                RunExit::LoadFailed(why) => engine_log(dir, &format!("시작하지 못했습니다: {why}")),
                RunExit::FrozenChanged => engine_log(dir, FROZEN_CHANGED_REASON),
                RunExit::Finished(_) | RunExit::LockHeld => {}
            }
            exit_code(&exit)
        }
        Err(p) => {
            engine_log(dir, &format!("내부 오류(패닉): {}", panic_text(p.as_ref())));
            3
        }
    }
}
