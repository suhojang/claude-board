//! 네이티브 Windows 실행 환경 — PowerShell 7·Windows PowerShell·명령 프롬프트·Git Bash. `ports::ExecEnv` 의 한 구현이다(다른 하나는 `wsl_env`).
//!
//! - claude 는 늘 `claude.exe` 를 직접 띄운다(cmd.exe 를 거치지 않는다). npm 이 만드는 `claude.cmd` 는 받지 않는다 —
//!   셸을 한 겹 더 거치면 따옴표·종료 코드·자손 끝내기가 어긋난다.
//! - 판정 명령은 고른 셸로 돌린다. 호출 꼴은 `rules::env_policy::check_invocation` 이 정하고 이 파일은 띄우기만 한다.
//! - 네이티브 Windows 에는 Claude Code 샌드박스가 없다(`sandbox` 는 늘 false). 파일 도구 권한 규칙만 건다.
//! - 프로세스 띄우기·Job Object 는 `winproc` 가 맡는다. Windows 가 아니면 `spawn_claude`·`run_check`·`check_ready` 가 `Unavailable` 이다.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::adapters::winproc::{decode_oem, strip_verbatim, Io, SpawnSpec, WinProcess};
use crate::ports::{
    ChildProcess, CheckOutcome, CheckRun, ClaudeLaunch, EnvChoice, EnvKind, EnvReport, ExecEnv, PortError, PortErrorKind, PortResult,
    SessionPolicy, SessionStart,
};
use crate::rules::env_policy::{check_invocation, sandbox_block, OutputCodec};

/// `claude --version` 을 기다리는 시간
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
/// 판정 출력 끝부분으로 돌려주는 바이트 수
pub const TAIL_BYTES: u64 = 4000;

/// claude 를 어디서 찾을까
#[derive(Debug, Clone)]
enum ClaudeSource {
    /// PATH 에서 `claude.exe`, 없으면 `<home>\.local\bin\claude.exe`. None 이면 이 프로세스의 환경
    Search { path: Option<OsString>, home: Option<PathBuf> },
    /// 시험·특수 빌드용 — 이 실행 파일에 앞 인자를 붙여 띄운다(예: `node.exe` + `fake-claude-stream.cjs`)
    Fixed { program: PathBuf, prefix: Vec<String> },
}

pub struct NativeEnv {
    choice: EnvChoice,
    /// 판정 명령을 돌릴 셸 실행 파일(`EnvEntry.program`)
    shell: PathBuf,
    claude: ClaudeSource,
}

impl NativeEnv {
    /// `shell` 은 그 환경의 실행 파일(`claude_board_core::envs::EnvEntry.program`). WSL 이면 Invalid
    pub fn new(choice: EnvChoice, shell: PathBuf) -> PortResult<Self> {
        if choice.kind == EnvKind::Wsl {
            return Err(PortError::new(PortErrorKind::Invalid, "WSL 은 wsl_env 가 맡는다"));
        }
        Ok(Self { choice, shell, claude: ClaudeSource::Search { path: None, home: None } })
    }

    /// 이 PC 의 환경 목록(`envs::detect`)에서 셸 실행 파일을 찾아 만든다. 목록을 만들 때 WSL 확인(최대 5초)이 끼므로 자주 부르지 않는다
    pub fn for_choice(choice: &EnvChoice) -> PortResult<Self> {
        let entry = claude_board_core::envs::detect()
            .into_iter()
            .find(|e| &e.choice == choice)
            .ok_or_else(|| PortError::new(PortErrorKind::NotFound, format!("이 PC 에 없는 실행 환경이다: {}", choice.key())))?;
        Self::new(choice.clone(), entry.program)
    }

    /// claude 자리에 다른 프로그램을 둔다. `args_prefix` 는 그 프로그램에 앞서 줄 인자(시험: `node.exe` + `fake-claude-stream.cjs`)
    pub fn with_claude_program(mut self, program: PathBuf, args_prefix: Vec<String>) -> Self {
        self.claude = ClaudeSource::Fixed { program, prefix: args_prefix };
        self
    }

    /// claude 찾기에 쓸 PATH 와 홈을 이 프로세스 환경 대신 준다(시험)
    pub fn with_claude_search(mut self, path: OsString, home: PathBuf) -> Self {
        self.claude = ClaudeSource::Search { path: Some(path), home: Some(home) };
        self
    }

    /// 띄울 claude 와 앞 인자. 못 찾으면 쉬운 말 한 문장
    fn resolve_claude(&self) -> Result<(PathBuf, Vec<String>), String> {
        match &self.claude {
            ClaudeSource::Fixed { program, prefix } => {
                if program.is_file() {
                    Ok((program.clone(), prefix.clone()))
                } else {
                    Err(format!("claude 자리의 실행 파일이 없습니다: {}", program.display()))
                }
            }
            ClaudeSource::Search { path, home } => {
                let path = path.clone().or_else(|| std::env::var_os("PATH")).unwrap_or_default();
                let home = home.clone().or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from));
                let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
                locate_claude(&dirs, home.as_deref()).map(|p| (p, Vec::new()))
            }
        }
    }
}

// ── claude 찾기 ──────────────────────────────────────

/// `claude.exe` 위치: PATH 차례대로, 없으면 `<home>\.local\bin\claude.exe`.
/// `claude.cmd`·`.bat`·`.ps1` 만 있으면(npm 설치본) 받지 않고 그렇게 알린다.
pub fn locate_claude(path_dirs: &[PathBuf], home: Option<&Path>) -> Result<PathBuf, String> {
    let home_bin = home.map(|h| h.join(".local").join("bin"));
    let dirs = path_dirs.iter().chain(home_bin.iter());
    let mut shim: Option<PathBuf> = None;
    let mut exe: Option<PathBuf> = None;
    for dir in dirs {
        let candidate = dir.join("claude.exe");
        if candidate.is_file() {
            exe = Some(candidate);
            break;
        }
        if shim.is_none() {
            shim = ["claude.cmd", "claude.bat", "claude.ps1"].iter().map(|n| dir.join(n)).find(|p| p.is_file());
        }
    }
    match (exe, shim) {
        (Some(exe), _) => Ok(exe),
        (None, Some(shim)) => Err(format!(
            "claude 가 {} 로만 깔려 있습니다. 자율주행은 claude.exe 를 직접 띄우므로 .cmd 로는 쓸 수 없습니다. 공식 설치본(claude.exe)을 설치하세요.",
            shim.display()
        )),
        (None, None) => Err("claude.exe 를 찾지 못했습니다. PATH 와 사용자 폴더의 .local\\bin 에 없습니다. Claude Code 를 설치하세요.".to_string()),
    }
}

// ── claude 인자 ──────────────────────────────────────

/// `claude` 에 줄 인자(앞 인자 제외). 긴 글은 파일 경로로만 넘긴다
pub fn claude_args(launch: &ClaudeLaunch) -> Vec<String> {
    let path = |p: &Path| strip_verbatim(p).to_string_lossy().into_owned();
    let mut args: Vec<String> = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"]
        .map(String::from)
        .to_vec();
    match &launch.session {
        SessionStart::New(id) => args.extend(["--session-id".to_string(), id.clone()]),
        SessionStart::Resume(id) => args.extend(["--resume".to_string(), id.clone()]),
    }
    if !launch.model.is_empty() {
        args.extend(["--model".to_string(), launch.model.clone()]);
    }
    if let Some(effort) = &launch.effort {
        args.extend(["--effort".to_string(), effort.clone()]);
    }
    args.extend(["--settings".to_string(), path(&launch.settings_file)]);
    if let Some(file) = &launch.system_prompt_file {
        args.extend(["--append-system-prompt-file".to_string(), path(file)]);
    }
    for dir in &launch.add_dirs {
        args.extend(["--add-dir".to_string(), path(dir)]);
    }
    if let Some(dir) = &launch.plugin_dir {
        args.extend(["--plugin-dir".to_string(), path(dir)]);
    }
    args.extend(launch.extra_args.iter().cloned());
    args
}

// ── 권한 규칙 ────────────────────────────────────────

/// Claude Code 권한 규칙에 쓰는 절대 경로 꼴. Windows 에서는 경로를 POSIX 꼴(`C:\Users\a` → `/c/Users/a`)로 바꿔 맞추고,
/// 파일 시스템 뿌리 기준은 `//` 로 시작한다(공식 문서 permissions 의 Read & Edit 절). 그래서 `C:\work\toy` → `//c/work/toy`.
/// 드라이브 글자로 시작하는 절대 경로만 받는다(상대 경로·네트워크 경로는 Invalid).
pub fn permission_path(path: &Path) -> PortResult<String> {
    let text = strip_verbatim(path).to_string_lossy().replace('\\', "/");
    let bytes = text.as_bytes();
    let drive_abs = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    if !drive_abs && !(bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':') {
        let why = if text.starts_with("//") { "네트워크 경로는 권한 규칙에 쓸 수 없다" } else { "드라이브 글자로 시작하는 절대 경로여야 한다" };
        return Err(PortError::new(PortErrorKind::Invalid, format!("{why}: {}", path.display())));
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest = text[2..].trim_end_matches('/');
    Ok(format!("//{drive}{rest}"))
}

fn has_glob(path: &str) -> bool {
    path.contains(['*', '?', '['])
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// 정책 → `permissions` 덩어리. 허용 목록(경로 없는 도구 규칙 + 쓰기 범위마다 `Edit(<경로>/**)`)과
/// 거부 목록(거부 도구 + 쓰기 금지 `Edit(...)` + 읽기 금지 `Read(...)`). 거부는 폴더 자신과 그 아래 둘 다 적는다
pub fn permission_rules(policy: &SessionPolicy) -> PortResult<(Vec<String>, Vec<String>)> {
    permission_rules_with(policy, &permission_path)
}

/// `permission_rules` 와 같되 경로를 규칙 꼴로 바꾸는 함수를 받는다(WSL 어댑터가 리눅스 경로로 쓴다)
pub fn permission_rules_with(
    policy: &SessionPolicy,
    to_rule_path: &dyn Fn(&Path) -> PortResult<String>,
) -> PortResult<(Vec<String>, Vec<String>)> {
    let mut allow: Vec<String> = Vec::new();
    let mut deny: Vec<String> = Vec::new();
    policy.allowed_tools.iter().for_each(|t| push_unique(&mut allow, t.clone()));
    policy.disallowed_tools.iter().for_each(|t| push_unique(&mut deny, t.clone()));
    for path in &policy.allow_write {
        let p = to_rule_path(path)?;
        push_unique(&mut allow, if has_glob(&p) { format!("Edit({p})") } else { format!("Edit({p}/**)") });
    }
    let guarded = [("Edit", &policy.deny_write), ("Read", &policy.deny_read)];
    for (tool, paths) in guarded {
        for path in paths {
            let p = to_rule_path(path)?;
            push_unique(&mut deny, format!("{tool}({p})"));
            if !has_glob(&p) {
                push_unique(&mut deny, format!("{tool}({p}/**)"));
            }
        }
    }
    Ok((allow, deny))
}

// ── 출력 끝부분 ──────────────────────────────────────

/// 파일 끝 `max` 바이트를 글로 푼다. 자르면서 UTF-8 글자 중간이 앞에 남으면 버린다
pub(crate) fn read_tail(path: &Path, max: u64, codec: OutputCodec) -> String {
    let Ok(mut file) = File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let cut = len > max;
    if cut && file.seek(SeekFrom::Start(len - max)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    match codec {
        OutputCodec::Utf8 => {
            let skip = if cut { bytes.iter().take_while(|b| (**b & 0xC0) == 0x80).count() } else { 0 };
            String::from_utf8_lossy(&bytes[skip..]).into_owned()
        }
        OutputCodec::Oem => decode_oem(&bytes),
    }
}

pub(crate) fn temp_log(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("claude-board-{tag}-{}-{n}.log", std::process::id()))
}

// ── ExecEnv ──────────────────────────────────────────

impl NativeEnv {
    /// `claude --version` 한 줄. 실패하면 쉬운 말 사유
    fn claude_version(&self, program: PathBuf, prefix: Vec<String>, cwd: &Path) -> Result<String, String> {
        let log = temp_log("version");
        let mut args = prefix;
        args.push("--version".to_string());
        let spec = SpawnSpec { program, args, raw_tail: None, cwd: cwd.to_path_buf(), env: Vec::new(), io: Io::MergedToFile { log: log.clone() } };
        let outcome = (|| {
            let mut proc = WinProcess::spawn(&spec).map_err(|e| format!("claude 를 띄우지 못했습니다: {}", e.message))?;
            let waited = proc.wait_timeout(VERSION_TIMEOUT).map_err(|e| e.message);
            let _ = proc.kill_tree();
            match waited? {
                None => Err(format!("claude --version 이 {}초 안에 답하지 않습니다.", VERSION_TIMEOUT.as_secs())),
                Some(0) => Ok(()),
                Some(code) => Err(format!("claude --version 이 실패했습니다(종료 코드 {code}).")),
            }
        })();
        let text = read_tail(&log, TAIL_BYTES, OutputCodec::Utf8);
        let _ = std::fs::remove_file(&log);
        outcome?;
        text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string).ok_or_else(|| "claude --version 이 아무것도 내지 않았습니다.".to_string())
    }
}

impl ExecEnv for NativeEnv {
    fn choice(&self) -> &EnvChoice {
        &self.choice
    }

    fn check_ready(&self, cwd: &Path) -> EnvReport {
        let mut problems: Vec<String> = Vec::new();
        if !cwd.is_dir() {
            problems.push(format!("작업 폴더가 없습니다: {}", cwd.display()));
        }
        if !self.shell.is_file() {
            problems.push(format!("{} 실행 파일을 찾지 못했습니다: {}", self.choice.kind.label(), self.shell.display()));
        }
        let claude = match self.resolve_claude() {
            Ok(found) => Some(found),
            Err(why) => {
                problems.push(why);
                None
            }
        };
        let mut version = None;
        if let (true, Some((program, prefix))) = (problems.is_empty(), claude) {
            match self.claude_version(program, prefix, cwd) {
                Ok(v) => version = Some(v),
                Err(why) => problems.push(why),
            }
        }
        EnvReport { ready: problems.is_empty(), sandbox: false, claude_version: version, problems }
    }

    fn to_env_path(&self, path: &Path) -> PortResult<String> {
        Ok(strip_verbatim(path).to_string_lossy().into_owned())
    }

    fn env_path_to_windows(&self, path: &str) -> PortResult<PathBuf> {
        Ok(strip_verbatim(Path::new(path)))
    }

    fn render_settings(&self, policy: &SessionPolicy) -> PortResult<Value> {
        let (allow, deny) = permission_rules(policy)?;
        Ok(json!({
            "sandbox": sandbox_block(self.choice.kind),
            "permissions": { "allow": allow, "deny": deny },
        }))
    }

    fn spawn_claude(&self, launch: &ClaudeLaunch) -> PortResult<Box<dyn ChildProcess>> {
        let (program, mut args) = self.resolve_claude().map_err(|why| PortError::new(PortErrorKind::Unavailable, why))?;
        args.extend(claude_args(launch));
        if let Some(dir) = launch.stderr_log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let spec = SpawnSpec {
            program,
            args,
            raw_tail: None,
            cwd: launch.cwd.clone(),
            env: launch.env.clone(),
            io: Io::Streaming { stderr_log: launch.stderr_log.clone() },
        };
        Ok(Box::new(NativeChild { proc: WinProcess::spawn(&spec)?, interrupts: 0 }))
    }

    fn run_check(&self, run: &CheckRun) -> PortResult<CheckOutcome> {
        let inv = check_invocation(&self.choice, &self.shell, run, None).map_err(|why| PortError::new(PortErrorKind::Invalid, why))?;
        if let Some(dir) = run.output_log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let spec = SpawnSpec {
            program: inv.program,
            args: inv.args,
            raw_tail: inv.raw_tail,
            cwd: inv.cwd,
            env: inv.env,
            io: Io::MergedToFile { log: run.output_log.clone() },
        };
        let started = Instant::now();
        let mut proc = WinProcess::spawn(&spec)?;
        let waited = proc.wait_timeout(inv.outer_timeout);
        // 끝났든 시간이 다 됐든 Job 에 남은 자손은 모두 끝낸다
        let _ = proc.kill_tree();
        let exit = waited?;
        Ok(CheckOutcome {
            exit,
            timed_out: exit.is_none(),
            seconds: started.elapsed().as_secs_f64(),
            tail: read_tail(&run.output_log, TAIL_BYTES, inv.codec),
        })
    }
}

// ── 띄운 claude ──────────────────────────────────────

struct NativeChild {
    proc: WinProcess,
    interrupts: u32,
}

impl ChildProcess for NativeChild {
    fn pid(&self) -> u32 {
        self.proc.pid()
    }

    fn write_line(&mut self, line: &str) -> PortResult<()> {
        self.proc.write_line(line)
    }

    fn close_stdin(&mut self) {
        self.proc.close_stdin();
    }

    fn read_line(&mut self, timeout: Duration) -> PortResult<Option<String>> {
        self.proc.read_line(timeout)
    }

    fn try_wait(&mut self) -> PortResult<Option<i32>> {
        self.proc.try_wait()
    }

    /// stream-json 의 `control_request` interrupt 한 줄. 자손까지 끝내는 일은 `kill_tree` 가 따로 한다
    fn interrupt(&mut self) -> PortResult<()> {
        self.interrupts += 1;
        let line = json!({
            "type": "control_request",
            "request_id": format!("cb-interrupt-{}-{}", self.proc.pid(), self.interrupts),
            "request": { "subtype": "interrupt" },
        });
        self.proc.write_line(&line.to_string())
    }

    fn kill_tree(&mut self) -> PortResult<()> {
        self.proc.kill_tree()
    }
}
