//! WSL 실행 환경 — `ports::ExecEnv` 의 둘째 구현(첫째는 `native_env`). claude 와 판정 명령을 WSL2 배포판 **안에서** 돌린다.
//!
//! - 늘 배포판 안의 claude 를 쓴다(`sh -lc 'exec claude …'`). Windows 쪽 `claude.exe` 는 쓰지 않는다. 로그인 셸(`-l`)로 띄워 PATH 가 터미널과 같다.
//! - 판정 호출 꼴은 `rules::env_policy::check_invocation` 이 정한다(`timeout -s INT -k 20 …`, `WSLENV`, `--cd`).
//!   이 파일은 그 꼴에 **꼬리표**(`CLAUDE_BOARD_AP_TAG`)를 더해 띄우고, 끝난 뒤 같은 꼬리표를 단 리눅스 프로세스를 쓸어낸다.
//! - 꼬리표: 띄우는 모든 리눅스 프로세스의 환경에 `CLAUDE_BOARD_AP_TAG=<꼬리표>` 가 든다. 자손은 환경을 물려받으므로 `/proc/*/environ` 으로
//!   찾을 수 있고, 프로세스 번호가 다른 프로세스에 재사용돼도 엉뚱한 것을 끝내지 않는다. 환경을 비우고 띄운 자손은 못 찾는다(정직한 한계).
//! - 리눅스 쪽 pid 는 `/tmp/<꼬리표>.pid` 에 `echo $$` 로 남긴다(`exec` 로 같은 pid 가 claude 가 된다). Windows 쪽 pid 는 wsl.exe 다.
//! - 값은 셸 글자에 끼워 넣지 않는다. wsl.exe 에는 argv 로 주고(`-e`), 스크립트에 들어가는 값은 위치 매개변수(`sh -c '…' sh a b`)로 준다.
//!   스크립트 글자는 모두 고정이고 한 줄이다.
//! - 프로세스 띄우기·Job Object 는 `winproc` 가 맡는다. 짧은 보조 명령(목록·탐침·쓸어내기)은 `WslRunner` 이음매로 부르므로 시험은 가짜로 갈아 끼운다.
//! - VM 을 깨우는 일은 최소로 한다: 배포판 목록(`wsl -l -v`)은 깨우지 않고, 쓸어내기·SIGINT 는 `wsl -l --running` 으로 돌고 있는지 먼저 본다.
//!   깨우는 것은 `check_ready` 의 탐침(출발 전 한 번)과 claude·판정을 띄울 때뿐이다.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use claude_board_core::envs::{parse_wsl_list, EnvEntry};
use serde_json::{json, Value};

use crate::adapters::native_env::{claude_args, permission_rules_with, read_tail, temp_log, TAIL_BYTES};
use crate::adapters::winproc::{strip_verbatim, Io, SpawnSpec, WinProcess};
use crate::ports::{
    CheckOutcome, CheckRun, ChildProcess, ClaudeLaunch, EnvChoice, EnvKind, EnvReport, ExecEnv, PortError, PortErrorKind,
    PortResult, SessionPolicy,
};
use crate::rules::env_policy::{check_invocation, sandbox_block, wsl_cd_arg, wsl_timed_out, Invocation, OutputCodec};

/// 리눅스 프로세스에 붙이는 꼬리표 환경 변수. 쓸어내기가 이 값으로 자기 프로세스를 알아본다
pub const TAG_ENV: &str = "CLAUDE_BOARD_AP_TAG";
/// 자동 마운트 뿌리의 기본값(`/etc/wsl.conf` 의 `[automount] root` 를 안 바꾼 배포판)
pub const DEFAULT_AUTOMOUNT_ROOT: &str = "/mnt";
/// 탐침(claude 확인·샌드박스 준비물 확인)을 기다리는 시간
const PROBE_LIMIT: Duration = Duration::from_secs(20);
/// 배포판 목록·돌고 있는지 확인을 기다리는 시간
const LIST_LIMIT: Duration = Duration::from_secs(10);
/// 쓸어내기를 기다리는 시간(스크립트 안의 TERM → KILL 대기가 최대 2초)
const SWEEP_LIMIT: Duration = Duration::from_secs(12);
/// SIGINT 보내기를 기다리는 시간
const SIGNAL_LIMIT: Duration = Duration::from_secs(10);

// ── 리눅스 쪽 스크립트(고정 글자, 한 줄) ─────────────

/// claude 띄우기. 위치 매개변수: `$1` 꼬리표, 나머지는 claude 인자. pid 를 `/tmp/<꼬리표>.pid` 에 적고 `exec` 한다
pub const LAUNCH_SCRIPT: &str = concat!(
    r#"tag=$1; shift; "#,
    r#"case $tag in ''|*[!A-Za-z0-9-]*) exit 64;; esac; "#,
    r#"echo $$ > "/tmp/$tag.pid" || exit 65; "#,
    r#"CLAUDE_BOARD_AP_TAG=$tag; export CLAUDE_BOARD_AP_TAG; "#,
    r#"exec claude "$@""#,
);

/// 표준 오류를 표준 출력과 **리눅스 안에서** 한 파일 설명자로 합친 뒤 나머지 인자를 그대로 `exec` 한다. 위치 매개변수: `$@` 가 띄울 명령.
/// wsl.exe 는 파일로 돌린 표준 출력·오류를 리눅스 쪽에서 각자 자기 쓰기 위치로 쓴다(Windows 쪽 핸들을 복제해 줘도 위치를 나누지 않는다) —
/// 둘을 같은 파일에 두면 나중에 쓴 쪽이 앞 글자를 덮는다. 합치는 일은 로그인 셸 프로필이 내는 글까지 덮도록 로그인 셸보다 바깥에서 한다
pub const MERGE_STDERR_SCRIPT: &str = r#"exec 2>&1; exec "$@""#;

/// 탐침. 출력 줄은 `CB_` 로 시작한다(로그인 셸이 내는 잡글과 섞여도 읽힌다)
pub const PROBE_SCRIPT: &str = concat!(
    r#"c=$(command -v claude 2>/dev/null); echo "CB_CLAUDE=$c"; "#,
    r#"if [ -n "$c" ]; then echo "CB_VERSION=$(timeout 10 claude --version 2>&1 | head -n 1)"; fi; "#,
    r#"echo "CB_BWRAP=$(command -v bwrap 2>/dev/null)"; "#,
    r#"echo "CB_SOCAT=$(command -v socat 2>/dev/null)"; "#,
    r#"if command -v bwrap >/dev/null 2>&1; then "#,
    r#"if bwrap --ro-bind / / --dev /dev --unshare-net true >/dev/null 2>&1; then echo CB_BWRAP_RUN=1; else echo CB_BWRAP_RUN=0; fi; fi; "#,
    r#"echo "CB_AUTOMOUNT=$(sed -n 's/^[[:space:]]*root[[:space:]]*=[[:space:]]*//p' /etc/wsl.conf 2>/dev/null | head -n 1)""#,
);

/// SIGINT 한 번. `$1` 꼬리표. pid 파일의 pid 가 이 꼬리표를 단 프로세스일 때만 보낸다. 종료 코드: 2 pid 없음 · 3 우리 것이 아님
pub const INTERRUPT_SCRIPT: &str = concat!(
    r#"tag=$1; "#,
    r#"case $tag in ''|*[!A-Za-z0-9-]*) exit 64;; esac; "#,
    r#"mine() { tr '\0' '\n' 2>/dev/null < "/proc/$1/environ" | grep -qxF "CLAUDE_BOARD_AP_TAG=$tag"; }; "#,
    r#"pid=$(cat "/tmp/$tag.pid" 2>/dev/null); "#,
    r#"case $pid in ''|*[!0-9]*) exit 2;; esac; "#,
    r#"mine "$pid" || exit 3; "#,
    r#"kill -s INT "$pid""#,
);

/// 쓸어내기. `$1` 꼬리표. 꼬리표를 단 프로세스를 모두 찾아 TERM, 2초 안에 안 끝나면 KILL. pid 파일이 있고 우리 것이면 그 프로세스 그룹도.
/// 끝에 pid 파일을 지운다. 찾을 것이 없어도 성공(0)이다
pub const SWEEP_SCRIPT: &str = concat!(
    r#"tag=$1; "#,
    r#"case $tag in ''|*[!A-Za-z0-9-]*) exit 64;; esac; "#,
    r#"pf="/tmp/$tag.pid"; "#,
    r#"mine() { tr '\0' '\n' 2>/dev/null < "/proc/$1/environ" | grep -qxF "CLAUDE_BOARD_AP_TAG=$tag"; }; "#,
    r#"all=; "#,
    r#"for d in /proc/[0-9]*; do p=${d#/proc/}; if [ "$p" != "$$" ] && mine "$p"; then all="$all $p"; fi; done; "#,
    r#"root=$(cat "$pf" 2>/dev/null); pg=; "#,
    r#"case $root in ''|*[!0-9]*) ;; *) if mine "$root"; then pg=$(ps -o pgid= -p "$root" 2>/dev/null | tr -d ' '); fi;; esac; "#,
    r#"case $pg in ''|*[!0-9]*) pg=;; esac; "#,
    r#"if [ -n "$pg" ] && [ "$pg" -le 1 ]; then pg=; fi; "#,
    r#"sig() { if [ -n "$pg" ]; then /bin/kill -s "$1" -- "-$pg" 2>/dev/null; fi; for p in $all; do kill -s "$1" "$p" 2>/dev/null; done; return 0; }; "#,
    r#"sig TERM; "#,
    r#"i=0; alive=1; "#,
    r#"while [ "$i" -lt 20 ] && [ -n "$alive" ]; do alive=; for p in $all; do kill -0 "$p" 2>/dev/null && alive=1; done; if [ -n "$alive" ]; then sleep 0.1; fi; i=$((i+1)); done; "#,
    r#"if [ -n "$alive" ]; then sig KILL; fi; "#,
    r#"rm -f -- "$pf"; exit 0"#,
);

// ── 이음매 ───────────────────────────────────────────

/// wsl.exe 로 띄울 보조 명령 하나. 어댑터가 만들어 `WslRunner` 에 넘긴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// 보조 명령의 결과. `exit` 가 None 이면 시간 안에 끝나지 않아 끊은 것이다. `output` 은 표준 출력과 오류를 합친 바이트
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub exit: Option<i32>,
    pub output: Vec<u8>,
}

/// 짧은 보조 명령을 돌리는 이음매. 실제는 `WinRunner`(winproc 로 띄움), 시험은 가짜
pub trait WslRunner: Send + Sync {
    fn run(&self, cmd: &WslCommand, limit: Duration) -> PortResult<RunOutput>;
}

/// winproc 로 wsl.exe 를 띄워 시간 안에 끝나기를 기다린다. 시간이 다 되면 Job 째로 끊는다
struct WinRunner;

impl WslRunner for WinRunner {
    fn run(&self, cmd: &WslCommand, limit: Duration) -> PortResult<RunOutput> {
        if !cmd.program.is_file() {
            return Err(PortError::new(PortErrorKind::NotFound, format!("wsl.exe 를 찾지 못했습니다: {}", cmd.program.display())));
        }
        let log = temp_log("wsl");
        let spec = SpawnSpec {
            program: cmd.program.clone(),
            args: cmd.args.clone(),
            raw_tail: None,
            cwd: std::env::temp_dir(),
            env: cmd.env.clone(),
            io: Io::MergedToFile { log: log.clone() },
        };
        let exit = WinProcess::spawn(&spec).and_then(|mut proc| {
            let waited = proc.wait_timeout(limit);
            let _ = proc.kill_tree();
            waited
        });
        let output = std::fs::read(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        Ok(RunOutput { exit: exit?, output })
    }
}

fn invalid(message: impl Into<String>) -> PortError {
    PortError::new(PortErrorKind::Invalid, message)
}

/// 이 엔진 프로세스 안에서 겹치지 않는 꼬리표. 글자는 `[A-Za-z0-9-]` 만이다(스크립트 검사와 같다)
fn new_tag() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("cbap-{}-{nanos:x}-{n}", std::process::id())
}

// ── 경로 바꾸기(순수) ────────────────────────────────

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// 자동 마운트 뿌리를 `/mnt` 꼴(끝 `/` 없음, 뿌리 `/` 는 빈 글)로 다듬는다
fn normalize_root(root: &str) -> PortResult<String> {
    if !root.starts_with('/') || root.contains('\0') || root.split('/').any(|c| c == "..") {
        return Err(invalid(format!("자동 마운트 뿌리는 `..` 없는 절대 경로여야 한다: {root}")));
    }
    Ok(root.trim_end_matches('/').to_string())
}

/// 경로 조각: 빈 조각과 `.` 은 버리고 `..` 은 거부한다
fn components<'a>(parts: impl Iterator<Item = &'a str>, whole: &str) -> PortResult<Vec<&'a str>> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            "" | "." => {}
            ".." => return Err(invalid(format!("`..` 가 든 경로는 바꾸지 않는다: {whole}"))),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Windows 절대 경로 → 배포판 안 경로. 드라이브 경로는 `<뿌리>/<소문자 드라이브>/…`, `\\wsl.localhost\<배포판>\…`·`\\wsl$\<배포판>\…` 는 `/…`
fn windows_to_linux(path: &Path, distro: &str, root: &str) -> PortResult<String> {
    let stripped = strip_verbatim(path).to_string_lossy().into_owned();
    if stripped.contains('\0') {
        return Err(invalid("경로에 NUL 글자가 있다"));
    }
    let text = match stripped.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => stripped,
    };
    let lead = text.chars().take_while(|c| is_sep(*c)).count();
    if lead >= 2 {
        let mut parts = text.trim_start_matches(is_sep).split(is_sep);
        let host = parts.next().unwrap_or("");
        if !(host.eq_ignore_ascii_case("wsl.localhost") || host.eq_ignore_ascii_case("wsl$")) {
            return Err(invalid(format!("네트워크 경로는 WSL 경로로 바꿀 수 없다: {}", path.display())));
        }
        let Some(name) = parts.next().filter(|d| !d.is_empty()) else {
            return Err(invalid(format!("배포판 이름이 없는 경로다: {}", path.display())));
        };
        if !name.eq_ignore_ascii_case(distro) {
            return Err(invalid(format!("다른 WSL 배포판({name})의 경로는 {distro} 에서 쓸 수 없다: {}", path.display())));
        }
        let comps = components(parts, &text)?;
        return Ok(if comps.is_empty() { "/".to_string() } else { comps.iter().map(|c| format!("/{c}")).collect() });
    }
    let mut chars = text.chars();
    let drive = match (chars.next(), chars.next(), chars.next()) {
        (Some(letter), Some(':'), Some(sep)) if letter.is_ascii_alphabetic() && is_sep(sep) => letter.to_ascii_lowercase(),
        _ => return Err(invalid(format!("드라이브 글자로 시작하는 절대 경로여야 한다: {}", path.display()))),
    };
    let comps = components(text[3..].split(is_sep), &text)?;
    let tail: String = comps.iter().map(|c| format!("/{c}")).collect();
    Ok(format!("{root}/{drive}{tail}"))
}

/// 배포판 안 절대 경로 → Windows 경로. 뿌리 아래 드라이브 글자 한 칸이면 `X:\…`, 그 밖은 `\\wsl.localhost\<배포판>\…`
fn linux_to_windows(path: &str, distro: &str, root: &str) -> PortResult<PathBuf> {
    if path.contains('\0') {
        return Err(invalid("경로에 NUL 글자가 있다"));
    }
    if !path.starts_with('/') {
        return Err(invalid(format!("배포판 경로는 `/` 로 시작해야 한다: {path}")));
    }
    let comps = components(path.split('/'), path)?;
    if comps.iter().any(|c| c.contains('\\')) {
        return Err(invalid(format!("역슬래시가 든 조각은 Windows 경로로 바꿀 수 없다: {path}")));
    }
    let root_parts: Vec<&str> = root.split('/').filter(|c| !c.is_empty()).collect();
    let n = root_parts.len();
    let on_drive = comps.len() > n
        && comps[..n] == root_parts[..]
        && comps[n].len() == 1
        && comps[n].as_bytes()[0].is_ascii_alphabetic();
    if on_drive {
        let drive = comps[n].to_ascii_uppercase();
        let tail = comps[n + 1..].join("\\");
        return Ok(PathBuf::from(format!("{drive}:\\{tail}")));
    }
    Ok(PathBuf::from(format!("\\\\wsl.localhost\\{distro}\\{}", comps.join("\\"))))
}

// ── 배포판 목록·탐침 읽기(순수) ──────────────────────

/// `wsl -l -v` 의 배포판 한 줄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroRow {
    pub name: String,
    /// 현지화된 글자 그대로(`Running`·`Stopped`·`실행 중` …). 판정에 쓰지 않는다
    pub state: String,
    pub version: u8,
    pub is_default: bool,
}

/// `wsl -l -v` 출력(UTF-16LE) → 배포판 줄들. 머리줄은 버전 칸이 숫자가 아니라 건너뛴다(현지화에 기대지 않는다)
pub fn parse_wsl_verbose(raw: &[u8]) -> Vec<DistroRow> {
    let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
    let text: String = String::from_utf16_lossy(&units).chars().filter(|&c| c != '\0' && c != '\u{feff}').collect();
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let is_default = line.starts_with('*');
        let tokens: Vec<&str> = line.trim_start_matches('*').split_whitespace().collect();
        if tokens.len() < 3 {
            continue;
        }
        let Ok(version) = tokens[tokens.len() - 1].parse::<u8>() else { continue };
        rows.push(DistroRow {
            name: tokens[0].to_string(),
            state: tokens[1..tokens.len() - 1].join(" "),
            version,
            is_default,
        });
    }
    rows
}

/// 탐침이 알아낸 것
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    /// `command -v claude`
    pub claude: Option<String>,
    /// `claude --version` 첫 줄
    pub version: Option<String>,
    pub bwrap: bool,
    pub socat: bool,
    /// bubblewrap 을 실제로 한 번 돌려 본 결과. 돌려 보지 못했으면 None
    pub bwrap_runs: Option<bool>,
    /// `/etc/wsl.conf` 의 `root`. 없으면 None(기본값 `/mnt`)
    pub automount: Option<String>,
}

/// 탐침 출력 → `Probe`. `CB_` 로 시작하는 줄만 읽는다
pub fn parse_probe(output: &str) -> Probe {
    let mut probe = Probe::default();
    for line in output.lines() {
        let Some((key, value)) = line.trim().split_once('=') else { continue };
        let value = value.trim();
        let some = || (!value.is_empty()).then(|| value.to_string());
        match key {
            "CB_CLAUDE" => probe.claude = some(),
            "CB_VERSION" => probe.version = some(),
            "CB_BWRAP" => probe.bwrap = !value.is_empty(),
            "CB_SOCAT" => probe.socat = !value.is_empty(),
            "CB_BWRAP_RUN" => probe.bwrap_runs = Some(value == "1"),
            "CB_AUTOMOUNT" => probe.automount = some(),
            _ => {}
        }
    }
    probe
}

/// 샌드박스 준비물이 모두 있는가: bubblewrap·socat 이 깔렸고 bubblewrap 이 실제로 돈다(WSL2 인지는 따로 본다)
pub fn sandbox_ready(probe: &Probe) -> bool {
    probe.bwrap && probe.socat && probe.bwrap_runs != Some(false)
}

/// 배포판 안 경로가 Windows 드라이브 마운트(`<뿌리>/<드라이브 글자>/…`) 아래인가
fn on_windows_drive(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|drive| drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic())
}

/// 탐침 결과의 문제 목록(쉬운 말). 비면 이 배포판에서 claude 를 샌드박스로 돌릴 수 있다
pub fn probe_problems(distro: &str, automount_root: &str, probe: &Probe) -> Vec<String> {
    let mut problems = Vec::new();
    match (&probe.claude, &probe.version) {
        (None, _) => problems.push(format!(
            "WSL 배포판({distro}) 안에 claude 가 없습니다. 배포판 안에서 Claude Code 를 설치하세요(Windows 쪽 claude.exe 는 쓰지 않습니다)."
        )),
        (Some(path), _) if on_windows_drive(path, automount_root) => problems.push(format!(
            "WSL 에서 찾은 claude 가 Windows 쪽 파일({path})입니다. 배포판({distro}) 안에 Claude Code 를 따로 설치하세요."
        )),
        (Some(_), None) => problems.push("claude --version 이 아무것도 내지 않았거나 10초 안에 답하지 않았습니다.".to_string()),
        (Some(_), Some(_)) => {}
    }
    let missing: Vec<&str> = [(!probe.bwrap).then_some("bubblewrap"), (!probe.socat).then_some("socat")].into_iter().flatten().collect();
    if !missing.is_empty() {
        problems.push(format!(
            "샌드박스에 필요한 {} 이(가) 배포판({distro})에 없습니다. 배포판 안에서 `sudo apt install bubblewrap socat` 을 실행하세요(Fedora 계열은 `sudo dnf install bubblewrap socat`).",
            missing.join("·")
        ));
    }
    if probe.bwrap && probe.bwrap_runs == Some(false) {
        problems.push(
            "bubblewrap 이 깔려 있지만 실제로 돌지 않습니다. Ubuntu 의 AppArmor 제한일 수 있습니다. Claude Code 문서(sandboxing)의 \"Add an AppArmor profile for bubblewrap\" 를 따르세요."
                .to_string(),
        );
    }
    let found = probe.automount.as_deref().map_or(Ok(DEFAULT_AUTOMOUNT_ROOT.to_string()), normalize_root);
    match found {
        Ok(found) if found == automount_root => {}
        Ok(found) => problems.push(format!(
            "배포판({distro})의 자동 마운트 뿌리는 {} 인데 자율주행은 {} 로 알고 있습니다. 자동 마운트 뿌리가 다른 배포판은 아직 지원하지 않습니다.",
            show_root(&found),
            show_root(automount_root)
        )),
        Err(why) => problems.push(format!("배포판({distro})의 /etc/wsl.conf 자동 마운트 뿌리를 읽지 못했습니다: {}", why.message)),
    }
    problems
}

fn show_root(root: &str) -> &str {
    if root.is_empty() {
        "/"
    } else {
        root
    }
}

// ── 배포판 쪽 일(명령 만들기 + 이음매) ───────────────

/// wsl.exe 와 배포판 하나에 대한 일. 명령 만들기는 순수 함수이고, 돌리는 것은 `WslRunner` 다.
/// 띄운 claude(`WslChild`)도 이것을 하나 쥐고 있어 끝낼 때 쓴다
#[derive(Clone)]
pub struct Remote {
    wsl: PathBuf,
    distro: String,
    runner: Arc<dyn WslRunner>,
}

impl Remote {
    fn command(&self, args: Vec<String>, env: Vec<(String, String)>) -> WslCommand {
        WslCommand { program: self.wsl.clone(), args, env }
    }

    /// `wsl.exe -d <배포판> <꼬리…>`
    fn in_distro(&self, tail: &[&str]) -> Vec<String> {
        ["-d", &self.distro].into_iter().chain(tail.iter().copied()).map(String::from).collect()
    }

    /// 배포판 목록과 WSL 버전. 배포판을 깨우지 않는다
    pub fn list_command(&self) -> WslCommand {
        self.command(["-l", "-v"].map(String::from).to_vec(), Vec::new())
    }

    /// 돌고 있는 배포판 이름만. 깨우지 않는다
    pub fn running_command(&self) -> WslCommand {
        self.command(["-l", "--running", "-q"].map(String::from).to_vec(), Vec::new())
    }

    /// claude 와 샌드박스 준비물 탐침. **배포판을 깨운다**
    pub fn probe_command(&self) -> WslCommand {
        self.command(self.in_distro(&["-e", "sh", "-c", MERGE_STDERR_SCRIPT, "sh", "sh", "-lc", PROBE_SCRIPT]), Vec::new())
    }

    /// 꼬리표를 단 프로세스 쓸어내기
    pub fn sweep_command(&self, tag: &str) -> WslCommand {
        self.command(self.in_distro(&["-e", "sh", "-c", SWEEP_SCRIPT, "sh", tag]), Vec::new())
    }

    /// claude 에게 SIGINT 한 번
    pub fn interrupt_command(&self, tag: &str) -> WslCommand {
        self.command(self.in_distro(&["-e", "sh", "-c", INTERRUPT_SCRIPT, "sh", tag]), Vec::new())
    }

    /// 이 배포판이 지금 돌고 있는가. 확인하지 못하면 false(깨우지 않는 쪽으로 기운다)
    pub fn is_running(&self) -> bool {
        match self.runner.run(&self.running_command(), LIST_LIMIT) {
            Ok(RunOutput { exit: Some(0), output }) => parse_wsl_list(&output).iter().any(|n| n == &self.distro),
            _ => false,
        }
    }

    /// 꼬리표를 단 리눅스 프로세스를 끝낸다. `only_if_running` 이면 배포판이 꺼져 있을 때 깨우지 않고 그대로 돌아온다
    pub fn sweep(&self, tag: &str, only_if_running: bool) -> PortResult<()> {
        if only_if_running && !self.is_running() {
            return Ok(());
        }
        let out = self.runner.run(&self.sweep_command(tag), SWEEP_LIMIT)?;
        match out.exit {
            Some(0) => Ok(()),
            Some(code) => Err(PortError::new(PortErrorKind::Io, format!("리눅스 프로세스 정리가 실패했다(종료 코드 {code})"))),
            None => Err(PortError::new(PortErrorKind::Timeout, "리눅스 프로세스 정리가 시간 안에 끝나지 않았다")),
        }
    }

    /// claude 에게 SIGINT 를 보낸다(stream-json interrupt 를 쓸 수 없을 때). 꺼져 있으면 보낼 곳이 없으므로 Unavailable
    pub fn interrupt(&self, tag: &str) -> PortResult<()> {
        if !self.is_running() {
            return Err(PortError::new(PortErrorKind::Unavailable, "배포판이 꺼져 있어 SIGINT 를 보낼 곳이 없다"));
        }
        let out = self.runner.run(&self.interrupt_command(tag), SIGNAL_LIMIT)?;
        match out.exit {
            Some(0) => Ok(()),
            Some(2) => Err(PortError::new(PortErrorKind::NotFound, "claude 의 pid 파일이 없다")),
            Some(3) => Err(PortError::new(PortErrorKind::NotFound, "pid 파일의 프로세스가 이 실행의 claude 가 아니다")),
            Some(code) => Err(PortError::new(PortErrorKind::Io, format!("SIGINT 보내기가 실패했다(종료 코드 {code})"))),
            None => Err(PortError::new(PortErrorKind::Timeout, "SIGINT 보내기가 시간 안에 끝나지 않았다")),
        }
    }
}

// ── WslEnv ───────────────────────────────────────────

pub struct WslEnv {
    choice: EnvChoice,
    distro: String,
    /// 정규화한 자동 마운트 뿌리(`/mnt`, 뿌리 `/` 는 빈 글)
    automount_root: String,
    remote: Remote,
}

impl WslEnv {
    /// `distro` 는 `[A-Za-z0-9._-]{1,64}`(인자로 넘기므로). `wsl` 은 `wsl.exe` 경로(`EnvEntry.program`)
    pub fn new(distro: &str, wsl: PathBuf) -> PortResult<Self> {
        let choice = EnvChoice::parse(&format!("wsl:{distro}")).ok_or_else(|| invalid(format!("WSL 배포판 이름이 규칙에 맞지 않는다: {distro}")))?;
        Ok(Self {
            choice,
            distro: distro.to_string(),
            automount_root: DEFAULT_AUTOMOUNT_ROOT.to_string(),
            remote: Remote { wsl, distro: distro.to_string(), runner: Arc::new(WinRunner) },
        })
    }

    /// 이 PC 의 환경 목록 항목(`envs::detect`)으로 만든다. WSL 이 아니거나 배포판 이름이 없으면 Invalid
    pub fn from_entry(entry: &EnvEntry) -> PortResult<Self> {
        match (&entry.choice.kind, &entry.choice.distro) {
            (EnvKind::Wsl, Some(distro)) => Self::new(distro, entry.program.clone()),
            _ => Err(invalid("WSL 배포판 항목이 아니다")),
        }
    }

    /// 자동 마운트 뿌리를 기본값 `/mnt` 가 아닌 것으로 둔다(`/etc/wsl.conf` 의 `[automount] root`)
    pub fn with_automount_root(mut self, root: &str) -> PortResult<Self> {
        self.automount_root = normalize_root(root)?;
        Ok(self)
    }

    /// 보조 명령을 돌리는 이음매를 바꾼다(시험)
    pub fn with_runner(mut self, runner: Arc<dyn WslRunner>) -> Self {
        self.remote.runner = runner;
        self
    }

    pub fn remote(&self) -> &Remote {
        &self.remote
    }

    fn cd_arg(&self, cwd: &Path) -> PortResult<String> {
        wsl_cd_arg(&strip_verbatim(cwd), &self.distro).map_err(invalid)
    }

    /// claude 를 띄울 wsl.exe 명령. 스크립트는 고정이고 꼬리표·claude 인자는 위치 매개변수로 간다.
    /// 인자 속 파일 경로는 배포판 경로로 바꾼 뒤 `native_env::claude_args` 에 맡긴다(인자 순서를 한 곳에 둔다)
    pub fn launch_command(&self, launch: &ClaudeLaunch, tag: &str) -> PortResult<WslCommand> {
        let linux = |p: &Path| self.to_env_path(p).map(PathBuf::from);
        let converted = ClaudeLaunch {
            settings_file: linux(&launch.settings_file)?,
            system_prompt_file: launch.system_prompt_file.as_deref().map(linux).transpose()?,
            add_dirs: launch.add_dirs.iter().map(|d| linux(d)).collect::<PortResult<_>>()?,
            plugin_dir: launch.plugin_dir.as_deref().map(linux).transpose()?,
            ..launch.clone()
        };
        let mut tail = vec!["sh".to_string(), tag.to_string()];
        tail.extend(claude_args(&converted));
        let cd = self.cd_arg(&launch.cwd)?;
        let mut args = self.remote.in_distro(&["--cd", &cd, "-e", "sh", "-lc", LAUNCH_SCRIPT]);
        args.extend(tail);
        let mut env: Vec<(String, String)> = launch.env.clone();
        if let Some(value) = launch_wslenv(launch)? {
            env.retain(|(n, _)| !n.eq_ignore_ascii_case("WSLENV"));
            env.push(("WSLENV".to_string(), value));
        }
        Ok(self.remote.command(args, env))
    }

    /// 판정 명령의 호출 꼴: `check_invocation` 의 결과에 꼬리표(환경 변수 + `WSLENV` 항목)를 더한다
    pub fn check_command(&self, run: &CheckRun, tag: &str, inherited_wslenv: Option<&str>) -> PortResult<Invocation> {
        let mut run = run.clone();
        run.cwd = strip_verbatim(&run.cwd);
        let mut inv = check_invocation(&self.choice, &self.remote.wsl, &run, inherited_wslenv).map_err(invalid)?;
        merge_stderr_in_distro(&mut inv.args)?;
        inv.env.retain(|(n, _)| !n.eq_ignore_ascii_case(TAG_ENV));
        inv.env.push((TAG_ENV.to_string(), tag.to_string()));
        match inv.env.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case("WSLENV")) {
            Some((_, value)) => {
                if !value.is_empty() {
                    value.push(':');
                }
                value.push_str(&format!("{TAG_ENV}/u"));
            }
            None => inv.env.push(("WSLENV".to_string(), format!("{TAG_ENV}/u"))),
        }
        Ok(inv)
    }

    /// WSL 버전과 탐침으로 `EnvReport` 를 만든다. 첫째 줄 문제가 있으면 탐침(=배포판 깨우기)을 하지 않는다
    fn probe_report(&self, problems: &mut Vec<String>) -> (bool, Option<String>) {
        match self.remote.runner.run(&self.remote.probe_command(), PROBE_LIMIT) {
            Err(e) => {
                problems.push(format!("WSL 배포판({})을 실행하지 못했습니다: {}", self.distro, e.message));
                (false, None)
            }
            Ok(RunOutput { exit: None, .. }) => {
                problems.push(format!("WSL 배포판({})이 {}초 안에 답하지 않았습니다.", self.distro, PROBE_LIMIT.as_secs()));
                (false, None)
            }
            Ok(RunOutput { exit: Some(code), output }) => {
                let probe = parse_probe(&String::from_utf8_lossy(&output));
                if code != 0 && probe == Probe::default() {
                    problems.push(format!("WSL 배포판({})에서 확인 명령이 실패했습니다(종료 코드 {code}).", self.distro));
                    return (false, None);
                }
                problems.extend(probe_problems(&self.distro, &self.automount_root, &probe));
                (sandbox_ready(&probe), probe.version)
            }
        }
    }
}

/// `wsl.exe … -e <명령…>` 의 `-e` 뒤를 `sh -c MERGE_STDERR_SCRIPT sh <명령…>` 으로 감싼다(`MERGE_STDERR_SCRIPT` 참고). `-e` 가 없으면 Invalid
fn merge_stderr_in_distro(args: &mut Vec<String>) -> PortResult<()> {
    let at = args.iter().position(|a| a == "-e").ok_or_else(|| invalid("판정 호출 꼴에 `-e` 가 없다"))?;
    let wrapper = ["sh", "-c", MERGE_STDERR_SCRIPT, "sh"].map(String::from);
    args.splice(at + 1..at + 1, wrapper);
    Ok(())
}

/// `launch.env` 이름 → `WSLENV` 값. 이름마다 경로면 `/up`, 아니면 `/u`. 환경 변수가 없으면 None
fn launch_wslenv(launch: &ClaudeLaunch) -> PortResult<Option<String>> {
    let mut names: Vec<&str> = Vec::new();
    for (name, _) in &launch.env {
        let mut chars = name.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return Err(invalid(format!("WSL 로 넘길 수 없는 환경 변수 이름입니다: {name}")));
        }
        if !name.eq_ignore_ascii_case("WSLENV") && !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return Ok(None);
    }
    let items: Vec<String> = names
        .iter()
        .map(|n| format!("{n}/{}", if launch.path_env.iter().any(|p| p.eq_ignore_ascii_case(n)) { "up" } else { "u" }))
        .collect();
    Ok(Some(items.join(":")))
}

fn dedup(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for item in list {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

impl ExecEnv for WslEnv {
    fn choice(&self) -> &EnvChoice {
        &self.choice
    }

    /// 순서: 작업 폴더 → `wsl -l -v`(깨우지 않음: 배포판이 있는지·WSL2 인지) → 문제가 없을 때만 탐침 한 번(**배포판을 깨운다**, 20초 상한).
    /// 탐침은 로그인 셸에서 claude 와 `--version`, bubblewrap·socat 과 bubblewrap 실행, `/etc/wsl.conf` 의 자동 마운트 뿌리를 본다.
    /// 샌드박스 준비물(Claude Code 문서 sandboxing: Linux·WSL2 는 bubblewrap 과 socat, WSL1 은 불가)이 없으면 `ready` 도 false 다 —
    /// 출발 문턱이 어차피 샌드박스 없는 WSL 을 막으므로, 무엇을 깔면 되는지가 문제 글에 들어간다
    fn check_ready(&self, cwd: &Path) -> EnvReport {
        let mut problems: Vec<String> = Vec::new();
        if !cwd.is_dir() {
            problems.push(format!("작업 폴더가 없습니다: {}", cwd.display()));
        } else if let Err(e) = self.to_env_path(cwd) {
            problems.push(format!("작업 폴더를 WSL 경로로 바꿀 수 없습니다: {}", e.message));
        }
        let mut version2 = false;
        match self.remote.runner.run(&self.remote.list_command(), LIST_LIMIT) {
            Err(e) => problems.push(format!("wsl.exe 를 실행하지 못했습니다: {}", e.message)),
            Ok(RunOutput { exit: Some(0), output }) => match parse_wsl_verbose(&output).into_iter().find(|r| r.name == self.distro) {
                None => problems.push(format!("WSL 배포판({})을 찾지 못했습니다. `wsl -l -v` 로 이름을 확인하세요.", self.distro)),
                Some(row) if row.version != 2 => problems.push(format!(
                    "WSL 배포판({})이 WSL{} 입니다. 샌드박스는 WSL2 에서만 걸립니다. PowerShell 에서 `wsl --set-version {} 2` 를 실행하세요.",
                    self.distro, row.version, self.distro
                )),
                Some(_) => version2 = true,
            },
            Ok(RunOutput { exit, .. }) => problems.push(format!(
                "`wsl -l -v` 가 {}",
                exit.map_or("시간 안에 끝나지 않았습니다.".to_string(), |c| format!("실패했습니다(종료 코드 {c})."))
            )),
        }
        let (mut sandbox, mut version) = (false, None);
        if problems.is_empty() {
            (sandbox, version) = self.probe_report(&mut problems);
        }
        EnvReport { ready: problems.is_empty(), sandbox: sandbox && version2, claude_version: version, problems }
    }

    fn to_env_path(&self, path: &Path) -> PortResult<String> {
        windows_to_linux(path, &self.distro, &self.automount_root)
    }

    fn env_path_to_windows(&self, path: &str) -> PortResult<PathBuf> {
        linux_to_windows(path, &self.distro, &self.automount_root)
    }

    /// 샌드박스 덩어리는 `sandbox_block(wsl)` 위에 파일 시스템·네트워크 칸을 더한다(macOS 엔진의 `sandboxSettings` 와 같은 칸 이름).
    /// 파일 시스템 칸은 배포판 경로(`/mnt/c/…`), 권한 규칙은 `//` 로 시작하는 뿌리 기준 경로(`Edit(//mnt/c/work/**)`)다.
    /// `denyWrite` 에는 읽기 금지 경로도 든다(macOS 와 같다)
    fn render_settings(&self, policy: &SessionPolicy) -> PortResult<Value> {
        let linux_all = |paths: &[PathBuf]| -> PortResult<Vec<String>> { paths.iter().map(|p| self.to_env_path(p)).collect() };
        let allow_write = linux_all(&policy.allow_write)?;
        let deny_read = linux_all(&policy.deny_read)?;
        let deny_write: Vec<String> = linux_all(&policy.deny_write)?.into_iter().chain(deny_read.iter().cloned()).collect();
        let mut sandbox = sandbox_block(self.choice.kind);
        if let Some(block) = sandbox.as_object_mut() {
            block.insert(
                "filesystem".to_string(),
                json!({ "allowWrite": dedup(allow_write), "denyWrite": dedup(deny_write), "denyRead": dedup(deny_read) }),
            );
            block.insert(
                "network".to_string(),
                json!({
                    "allowedDomains": policy.network_allow,
                    "strictAllowlist": true,
                    "allowLocalBinding": policy.local_binding,
                    "allowUnixSockets": [],
                }),
            );
        }
        let (allow, deny) = permission_rules_with(policy, &|p| self.to_env_path(p).map(|linux| format!("/{linux}")))?;
        Ok(json!({
            "sandbox": sandbox,
            "permissions": { "allow": allow, "deny": deny },
        }))
    }

    fn spawn_claude(&self, launch: &ClaudeLaunch) -> PortResult<Box<dyn ChildProcess>> {
        let tag = new_tag();
        let cmd = self.launch_command(launch, &tag)?;
        if let Some(dir) = launch.stderr_log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // wsl.exe 자신의 작업 폴더는 임시 폴더로 둔다 — 실행 중인 프로세스가 쥔 폴더는 작업 트리를 지울 때 막는다. 리눅스 쪽 폴더는 `--cd` 가 정한다
        let spec = SpawnSpec {
            program: cmd.program,
            args: cmd.args,
            raw_tail: None,
            cwd: std::env::temp_dir(),
            env: cmd.env,
            io: Io::Streaming { stderr_log: launch.stderr_log.clone() },
        };
        let proc = WinProcess::spawn(&spec)?;
        Ok(Box::new(WslChild { proc, remote: self.remote.clone(), tag, interrupts: 0 }))
    }

    /// 판정을 돌린 뒤 **끝났든 시간이 다 됐든** 같은 꼬리표의 리눅스 프로세스를 쓸어낸다(백그라운드로 남은 서버 등).
    /// 종료 코드 124·137 은 안쪽 `timeout` 이 끊은 것으로 본다(판정 명령 스스로 낸 같은 코드와 구별하지 못한다)
    fn run_check(&self, run: &CheckRun) -> PortResult<CheckOutcome> {
        let tag = new_tag();
        let inherited = std::env::var("WSLENV").ok();
        let inv = self.check_command(run, &tag, inherited.as_deref())?;
        if let Some(dir) = run.output_log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let spec = SpawnSpec {
            program: inv.program,
            args: inv.args,
            raw_tail: inv.raw_tail,
            cwd: std::env::temp_dir(),
            env: inv.env,
            io: Io::MergedToFile { log: run.output_log.clone() },
        };
        let started = Instant::now();
        let mut proc = WinProcess::spawn(&spec)?;
        let waited = proc.wait_timeout(inv.outer_timeout);
        let _ = proc.kill_tree();
        let _ = self.remote.sweep(&tag, false);
        let exit = waited?;
        let timed_out = exit.is_none_or(wsl_timed_out);
        Ok(CheckOutcome {
            exit: if timed_out { None } else { exit },
            timed_out,
            seconds: started.elapsed().as_secs_f64(),
            tail: read_tail(&run.output_log, TAIL_BYTES, OutputCodec::Utf8),
        })
    }
}

// ── 띄운 claude ──────────────────────────────────────

struct WslChild {
    proc: WinProcess,
    remote: Remote,
    tag: String,
    interrupts: u32,
}

impl ChildProcess for WslChild {
    /// Windows 쪽 pid(wsl.exe). 리눅스 쪽 pid 는 `/tmp/<꼬리표>.pid` 에 있고 어댑터 안에서만 쓴다
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

    /// stream-json 의 `control_request` interrupt 한 줄. 표준 입력에 쓰지 못할 때만(닫힘·깨진 파이프) 리눅스 쪽 SIGINT 로 넘어간다
    fn interrupt(&mut self) -> PortResult<()> {
        self.interrupts += 1;
        let line = json!({
            "type": "control_request",
            "request_id": format!("cb-interrupt-{}-{}", self.proc.pid(), self.interrupts),
            "request": { "subtype": "interrupt" },
        });
        match self.proc.write_line(&line.to_string()) {
            Ok(()) => Ok(()),
            Err(write_err) => self.remote.interrupt(&self.tag).map_err(|sig_err| {
                PortError::new(
                    write_err.kind,
                    format!("interrupt 줄을 쓰지 못했고({}) SIGINT 도 보내지 못했다({})", write_err.message, sig_err.message),
                )
            }),
        }
    }

    /// Windows 쪽은 Job 째로 끝내고, 배포판이 돌고 있으면 같은 꼬리표의 리눅스 프로세스도 쓸어낸다(최선 노력 — 오류는 삼킨다).
    /// 돌고 있지 않은 배포판은 깨우지 않는다
    fn kill_tree(&mut self) -> PortResult<()> {
        let windows = self.proc.kill_tree();
        let _ = self.remote.sweep(&self.tag, true);
        windows
    }
}

impl Drop for WslChild {
    /// 떨어뜨리면 자손까지 끝낸다(엔진이 죽지 않고 이 값만 버려도 리눅스 쪽에 남지 않게). Job 은 `WinProcess` 가 따로 닫는다
    fn drop(&mut self) {
        let _ = self.proc.kill_tree();
        let _ = self.remote.sweep(&self.tag, true);
    }
}
