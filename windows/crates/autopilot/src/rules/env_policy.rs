//! 실행 환경(WSL·PowerShell 7·Windows PowerShell·명령 프롬프트·Git Bash)에서 파생되는 규칙. Windows 전용(Swift 원본 없음).
//! 동의 규칙 · 출발 문턱 · 판정 명령 호출 꼴 · 세션 설정의 `sandbox` 덩어리를 순수 함수로 둔다.
//! 실행은 하지 않는다. 호출 꼴(`Invocation`)을 만들 뿐이고 띄우는 일은 어댑터가 한다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_core::envs::{EnvChoice, EnvKind};
use serde_json::{json, Value};

use crate::model::{ExecEnv as ExecEnvRecord, UnsandboxedOk};
use crate::ports::{CheckRun, EnvReport};

/// 판정 명령 글자를 셸에 넘기는 환경 변수 이름(PowerShell·Git Bash·WSL 은 인자가 아니라 이 변수로 넘긴다 — 따옴표 문제를 없앤다)
pub const CHECK_ENV: &str = "CLAUDE_BOARD_CHECK";
/// WSL 판정의 안쪽 `timeout -k` 초(SIGINT 뒤 이만큼 기다렸다 SIGKILL)
pub const WSL_KILL_AFTER_SEC: u64 = 20;
/// WSL 판정의 바깥 시간 상한 여유 — 안쪽 timeout 이 먼저 끊고, 그래도 안 끝나면 어댑터가 Job 째로 끊는다
pub const WSL_OUTER_GRACE: Duration = Duration::from_secs(30);
/// PowerShell 판정 감싸개. 통과 규칙: 잡히지 않은 오류면 1, 마지막 네이티브 명령의 종료 코드가 0 이 아니면 그 값,
/// 오류 기록(비종료 오류 포함)이 하나라도 있으면 1, 아니면 0. 줄 끝은 LF 로 고정한다(`concat!` — 저장소 줄바꿈 설정에 기대지 않는다)
///
/// 맨 앞에서 `Microsoft.PowerShell.Utility`·`Management` 를 이름으로 불러 둔다. Windows PowerShell 5.1 은 아직 안 불러온 모듈의 cmdlet(`Write-Output`·`Test-Path` …)을
/// 처음 쓸 때 모듈 경로의 모든 모듈을 훑어 찾는데(자동 불러오기), 엔진은 `PSModulePath`·`PSModuleAnalysisCachePath` 를 물려주지 않으므로
/// 모듈이 많고 분석 캐시가 없는 PC 에서는 이 훑기가 수십 초 걸린다(GitHub 러너 windows-latest 실측: `Write-Output hello` 한 줄이 18~60초).
/// 이름으로 불러오면 훑지 않고 첫 일치를 바로 쓴다(0.5초). 실패해도 무시하고 오류 기록을 비운다(`$Error.Clear()`)
pub const POWERSHELL_WRAPPER: &str = concat!(
    "$ErrorActionPreference = 'Continue'\n",
    "$ProgressPreference = 'SilentlyContinue'\n",
    "try { Import-Module Microsoft.PowerShell.Utility, Microsoft.PowerShell.Management -ErrorAction Stop } catch { }\n",
    "try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }\n",
    "$global:LASTEXITCODE = 0\n",
    "$Error.Clear()\n",
    "try {\n",
    "    & ([ScriptBlock]::Create($env:CLAUDE_BOARD_CHECK))\n",
    "} catch {\n",
    "    [Console]::Error.WriteLine(($_ | Out-String))\n",
    "    exit 1\n",
    "}\n",
    "if ($LASTEXITCODE -is [int] -and $LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n",
    "if ($Error.Count -gt 0) { exit 1 }\n",
    "exit 0\n",
);

/// 판정 출력 바이트를 글로 풀 방법. cmd 는 콘솔 코드 페이지(OEM)로 낸다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCodec {
    Utf8,
    Oem,
}

/// 판정 명령 한 번을 띄울 꼴. 어댑터는 `program` 을 `args` 로(std `Command::args`) 띄우고,
/// `raw_tail` 이 있으면 그 뒤에 `CommandExt::raw_arg` 로 그대로 붙인다. 작업 폴더는 `cwd`, 환경은 고른 환경 + `env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub raw_tail: Option<String>,
    /// `run.env` 차례 그대로 + 이 함수가 더한 변수(뒤에). 같은 이름(대소문자 무시)은 뒤의 것이 이긴다 — 같은 이름이 둘 남지 않는다
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub codec: OutputCodec,
    /// 어댑터가 자손째로 끊을 시각(판정 시작부터). 네이티브는 `run.timeout`, WSL 은 `run.timeout + WSL_OUTER_GRACE`
    pub outer_timeout: Duration,
}

/// 글 안에 쓰는 환경 이름. WSL 은 배포판까지, 그 밖은 `EnvKind::label()`
fn env_label(choice: &EnvChoice) -> String {
    match (choice.kind, &choice.distro) {
        (EnvKind::Wsl, Some(d)) => format!("WSL · {d}"),
        (kind, _) => kind.label().to_string(),
    }
}

/// 샌드박스를 못 거는 환경이면 출발 확인의 동의가 있어야 한다.
pub fn needs_unsandboxed_consent(kind: EnvKind) -> bool {
    !kind.sandbox_capable()
}

/// 출발 문턱 1 — 기록(run.json `exec_env`)과 이 PC 의 환경 목록만 본다(준비 확인 전, VM 을 띄우지 않는다).
/// 통과하면 고른 환경. 막히면 쉬운 말 한 문장.
pub fn choose(rec: Option<&ExecEnvRecord>, available: &[EnvChoice]) -> Result<EnvChoice, String> {
    let rec = rec
        .filter(|r| !r.key.is_empty())
        .ok_or_else(|| "실행 환경을 고르지 않았습니다. 출발 확인에서 실행 환경을 고르세요.".to_string())?;
    let choice = EnvChoice::parse(&rec.key).ok_or_else(|| format!("알 수 없는 실행 환경({})입니다.", rec.key))?;
    if rec.kind != choice.kind.id() {
        return Err(format!("실행 환경 기록이 어긋납니다(key {}, kind {}).", rec.key, rec.kind));
    }
    if choice.kind == EnvKind::Wsl && choice.distro.is_none() {
        return Err("WSL 배포판을 고르세요.".to_string());
    }
    if let Some(distro) = &rec.distro {
        if choice.distro.as_ref() != Some(distro) {
            return Err(format!("실행 환경 기록이 어긋납니다(key {}, distro {}).", rec.key, distro));
        }
    }
    if !available.contains(&choice) {
        return Err(format!("이 PC 에 없는 실행 환경({})입니다.", env_label(&choice)));
    }
    if needs_unsandboxed_consent(choice.kind) {
        let consented = rec.unsandboxed_ok.as_ref().and_then(|ok| ok.at.as_f64()).is_some_and(|at| at > 0.0);
        if !consented {
            return Err(format!(
                "{} 은(는) 샌드박스 없이 돕니다. 출발 확인에서 동의해야 출발합니다.",
                env_label(&choice)
            ));
        }
    }
    Ok(choice)
}

/// 출발 문턱 2 — 어댑터 `check_ready` 결과를 본다. 통과하면 이 실행의 `sandbox` 값(WSL 은 늘 true, 네이티브는 늘 false)
pub fn confirm_ready(choice: &EnvChoice, report: &EnvReport) -> Result<bool, String> {
    if !report.ready {
        let why = if report.problems.is_empty() { "까닭 모름".to_string() } else { report.problems.join("; ") };
        return Err(format!("실행 환경을 쓸 수 없습니다: {why}"));
    }
    if choice.kind != EnvKind::Wsl {
        // 네이티브 Windows 에는 샌드박스가 없다. 어댑터가 true 를 줘도 따르지 않는다
        return Ok(false);
    }
    if report.sandbox {
        Ok(true)
    } else {
        Err("WSL 에서 Claude Code 샌드박스를 걸 수 없어 출발하지 않습니다(WSL2 인지, bubblewrap·socat 이 깔렸는지 확인하세요).".to_string())
    }
}

/// 문턱을 통과한 뒤 run.json·frozen.json 에 고정할 `exec_env`. `unsandboxed_ok` 는 네이티브일 때만 `consent` 그대로
pub fn record_for(choice: &EnvChoice, sandbox: bool, consent: Option<UnsandboxedOk>) -> ExecEnvRecord {
    ExecEnvRecord {
        distro: choice.distro.clone(),
        key: choice.key(),
        kind: choice.kind.id().to_string(),
        sandbox: Some(sandbox),
        unsandboxed_ok: if needs_unsandboxed_consent(choice.kind) { consent } else { None },
        extra: Default::default(),
    }
}

/// `--settings` 의 `sandbox` 덩어리 중 환경이 정하는 칸(filesystem·network 칸은 어댑터가 더한다).
/// 네이티브에 `failIfUnavailable` 을 넣으면 Windows 의 claude 가 시작하자마자 끝난다.
pub fn sandbox_block(kind: EnvKind) -> Value {
    if kind.sandbox_capable() {
        json!({
            "enabled": true,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": false,
            "autoAllowBashIfSandboxed": true,
        })
    } else {
        json!({ "enabled": false })
    }
}

/// 판정 명령 글자가 그 환경에서 돌 수 있는 꼴인가(출발 전 기계 검사)
pub fn validate_check(kind: EnvKind, command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("판정 명령이 비었습니다.".to_string());
    }
    if command.contains('\0') {
        return Err("판정 명령에 NUL 글자가 있습니다.".to_string());
    }
    if kind == EnvKind::Cmd && command.contains(['\r', '\n']) {
        return Err("명령 프롬프트 판정 명령은 한 줄이어야 합니다.".to_string());
    }
    Ok(())
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// `wsl.exe --cd` 에 줄 값. 일반 Windows 절대 경로는 그대로(wsl 이 automount 뿌리로 바꾼다),
/// `\\wsl.localhost\<배포판>\…`·`\\wsl$\<배포판>\…` 은 그 배포판이 `distro` 와 같으면 `/…` 로 바꾼다
pub fn wsl_cd_arg(cwd: &Path, distro: &str) -> Result<String, String> {
    let lossy = cwd.to_string_lossy();
    // `fs::canonicalize` 가 `\\wsl.localhost\…` 를 `\\?\UNC\wsl.localhost\…` 로 돌려준다. 같은 경로이니 앞 `\\?\UNC\` 를 `\\` 로 되돌린다
    let path: std::borrow::Cow<str> = match lossy.strip_prefix(r"\\?\UNC\") {
        Some(rest) => std::borrow::Cow::Owned(format!(r"\\{rest}")),
        None => lossy,
    };
    let rooted = path.trim_start_matches(is_sep);
    if rooted.len() < path.len() {
        let mut parts = rooted.split(is_sep);
        let host = parts.next().unwrap_or("");
        let is_wsl_host = host.eq_ignore_ascii_case("wsl.localhost") || host.eq_ignore_ascii_case("wsl$");
        if let (true, Some(d)) = (is_wsl_host, parts.next().filter(|d| !d.is_empty())) {
            if !d.eq_ignore_ascii_case(distro) {
                return Err(format!("작업 폴더가 다른 WSL 배포판({d})에 있습니다."));
            }
            let rest: Vec<&str> = parts.filter(|s| !s.is_empty()).collect();
            return Ok(format!("/{}", rest.join("/")));
        }
    }
    let mut chars = path.chars();
    let is_drive_abs = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some(sep)) if letter.is_ascii_alphabetic() && is_sep(sep)
    );
    if is_drive_abs {
        Ok(path.into_owned())
    } else {
        Err(format!("WSL 작업 폴더는 절대 경로여야 합니다: {path}"))
    }
}

/// PowerShell `-EncodedCommand` 값: UTF-16LE 바이트의 표준 base64(패딩 `=` 있음)
pub fn powershell_encoded(script: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| TABLE[((n >> shift) & 0x3f) as usize] as char;
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// 같은 이름(대소문자 무시)이 이미 있으면 앞의 것을 지우고 뒤에 붙인다
fn push_env(env: &mut Vec<(String, String)>, name: &str, value: &str) {
    env.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    env.push((name.to_string(), value.to_string()));
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `run.timeout` 을 초로 올림(최소 1)
fn timeout_seconds(timeout: Duration) -> u64 {
    (timeout.as_secs() + u64::from(timeout.subsec_nanos() > 0)).max(1)
}

/// WSL 로 넘길 환경 변수 목록 값. 이름마다 경로면 `/up`, 아니면 `/u`. 엔진 자신의 `WSLENV` 조각은 앞에 둔다
fn wslenv_value(run: &CheckRun, inherited: Option<&str>) -> Result<String, String> {
    let mut names: Vec<&str> = vec![CHECK_ENV];
    for (name, _) in &run.env {
        if !valid_env_name(name) {
            return Err(format!("WSL 로 넘길 수 없는 환경 변수 이름입니다: {name}"));
        }
        let skip = name.eq_ignore_ascii_case("WSLENV") || names.iter().any(|n| n.eq_ignore_ascii_case(name));
        if !skip {
            names.push(name);
        }
    }
    let ours: Vec<String> = names
        .iter()
        .map(|n| {
            let is_path = run.path_env.iter().any(|p| p.eq_ignore_ascii_case(n));
            format!("{n}/{}", if is_path { "up" } else { "u" })
        })
        .collect();
    let kept = inherited.unwrap_or("").split(':').filter(|piece| {
        let name = piece.split('/').next().unwrap_or("");
        !piece.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(name))
    });
    Ok(kept.map(str::to_string).chain(ours).collect::<Vec<_>>().join(":"))
}

/// 판정 명령 한 번의 호출 꼴. `program` 은 그 환경의 실행 파일(`EnvEntry.program`).
/// `inherited_wslenv` 는 엔진 자신의 `WSLENV` 값(없으면 None) — WSL 일 때만 쓴다.
/// 엔진은 판정 명령 글자를 바꾸지 않는다(사람이 그 환경의 문법으로 쓴다). 문법은 판정 미리 돌리기가 잡는다.
pub fn check_invocation(
    choice: &EnvChoice,
    program: &Path,
    run: &CheckRun,
    inherited_wslenv: Option<&str>,
) -> Result<Invocation, String> {
    validate_check(choice.kind, &run.command)?;
    let mut env: Vec<(String, String)> = Vec::new();
    for (name, value) in &run.env {
        push_env(&mut env, name, value);
    }
    let mut inv = Invocation {
        program: program.to_path_buf(),
        args: Vec::new(),
        raw_tail: None,
        env: Vec::new(),
        cwd: run.cwd.clone(),
        codec: OutputCodec::Utf8,
        outer_timeout: run.timeout,
    };
    let eval_args = || "eval \"$CLAUDE_BOARD_CHECK\"".to_string();
    match choice.kind {
        EnvKind::Cmd => {
            inv.args = ["/d", "/s", "/c"].map(String::from).to_vec();
            inv.raw_tail = Some(format!("\"{}\"", run.command));
            inv.codec = OutputCodec::Oem;
        }
        EnvKind::PowerShell | EnvKind::Pwsh => {
            inv.args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"]
                .map(String::from)
                .into_iter()
                .chain([powershell_encoded(POWERSHELL_WRAPPER)])
                .collect();
            push_env(&mut env, CHECK_ENV, &run.command);
        }
        EnvKind::GitBash => {
            inv.args = vec!["-lc".to_string(), eval_args()];
            push_env(&mut env, CHECK_ENV, &run.command);
            // Git for Windows 의 /etc/profile 이 홈으로 cd 하지 않게 한다
            push_env(&mut env, "CHERE_INVOKING", "1");
        }
        EnvKind::Wsl => {
            let distro = choice.distro.as_deref().ok_or_else(|| "WSL 배포판을 고르세요.".to_string())?;
            let cd = wsl_cd_arg(&run.cwd, distro)?;
            let wslenv = wslenv_value(run, inherited_wslenv)?;
            inv.args = ["-d", distro, "--cd", &cd, "-e", "timeout", "-s", "INT", "-k"]
                .map(String::from)
                .into_iter()
                .chain([
                    WSL_KILL_AFTER_SEC.to_string(),
                    format!("{}s", timeout_seconds(run.timeout)),
                    "bash".to_string(),
                    "-lc".to_string(),
                    eval_args(),
                ])
                .collect();
            push_env(&mut env, CHECK_ENV, &run.command);
            push_env(&mut env, "WSLENV", &wslenv);
            inv.outer_timeout = run.timeout + WSL_OUTER_GRACE;
        }
    }
    inv.env = env;
    Ok(inv)
}

/// WSL 판정의 종료 코드가 안쪽 `timeout` 이 끊은 것인가 — 124(SIGINT 로 끝남) 또는 137(-k 로 SIGKILL)
pub fn wsl_timed_out(exit: i32) -> bool {
    matches!(exit, 124 | 137)
}

/// 판정 미리 돌리기를 한 환경과 지금 고른 환경이 같은가. `prerun_key` 가 None(기록 없음, macOS 가 만든 준비)이면 Ok
pub fn prerun_matches(prerun_key: Option<&str>, choice: &EnvChoice) -> Result<(), String> {
    match prerun_key {
        Some(k) if k != choice.key() => {
            let was = EnvChoice::parse(k).map_or_else(|| k.to_string(), |c| env_label(&c));
            Err(format!("판정 미리 돌리기는 {was} 에서 했습니다. 지금 고른 {} 으로 다시 돌리세요.", env_label(choice)))
        }
        _ => Ok(()),
    }
}
