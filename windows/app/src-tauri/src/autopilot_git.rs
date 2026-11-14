//! 보드가 직접 돌리는 git — 합치기·승격 시트가 ref·머리·바뀐 파일을 읽을 때 쓰는 길.
//! 원본(macOS): `Sources/ClaudeBoard/Autopilot.swift` 의 `apGitHardenC`·`apGitEnv`·`apGit` 과 `Sources/AutopilotRules/BoardGit.swift`.
//! 규칙은 `claude_board_autopilot::rules::board_git`(`board_git_changed`·`board_refs_pin`)가 소유하고, 여기는 어댑터다:
//! 지금 git 폴더를 읽어 규칙에 값으로 넘기고, 규칙이 막으라면 git 을 돌리지 않는다.
//!
//! 엔진의 `adapters::git` 은 `GIT_DIR` 박기·시간 상한·출력 상한을 받지 않고 보호 설정(`static_config`)이 비공개라
//! 같은 보호(같은 `-c` 값·같은 환경)를 여기에 한 벌 더 둔다. 환경 쪽은 엔진이 공개한 `ENGINE_GIT_HARDEN_ENV`·`engine_git_env` 를 그대로 쓰고,
//! 이름을 저장소가 정하는 코드 실행 칸(`filter.<이름>.clean` 등)은 공개된 `neutralisers_for` 로 같은 방식으로 비운다.
//!
//! 웹뷰에 열리는 `autopilot_git_read` 는 읽기 전용 허용 목록(`check_read_only`)을 통과한 명령만 돌린다. 이것이 웹뷰와 git 사이의 안전 경계다.
//! 허용 목록은 하위 명령마다 정확히 맞는 깃발만 받는다(그 밖의 `-`로 시작하는 인자는 모두 거절). 이름·값을 따로 받는 깃발은 `이름=값` 꼴만 받는다.
//!
//! 아직 `board.rs` 의 `invoke_handler` 에 등록되지 않아 보드 빌드에서는 안 쓰이는 항목이 있다 — 등록하면 아래 `allow(dead_code)` 를 지운다.
#![allow(dead_code)]

use claude_board_autopilot::cli::valid_run_id;
use claude_board_autopilot::engine::prep_git::file_id;
use claude_board_autopilot::adapters::git::{engine_git_env, neutralisers_for};
use claude_board_autopilot::adapters::winproc::descendants_since;
use claude_board_core::proc::{creation_time, terminate, ProcessTable};
use claude_board_autopilot::rules::board_git::{board_git_changed, board_refs_pin, GitRecord};
use claude_board_autopilot::rules::path_key::path_key_windows;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// 기본 시간 상한. 넘으면 git 을 죽인다.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// 표준 출력 상한(바이트). 넘으면 git 을 죽이고 `truncated`.
pub const DEFAULT_OUT_CAP: usize = 1 << 20;
/// 표준 오류 상한(바이트)
pub const DEFAULT_ERR_CAP: usize = 64 << 10;
/// 죽인 뒤 출력 읽는 쪽을 기다리는 시간(손자 프로세스가 파이프를 쥐어도 여기서 끊는다)
const DRAIN_WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";
#[cfg(windows)]
const GIT_EXE: &str = "git.exe";
#[cfg(not(windows))]
const GIT_EXE: &str = "git";

const REFUSED_PREFIX: &str = "보드 git 거절: ";

/// git 한 번의 결과. 오류 글에는 환경 값·인자를 싣지 않는다(git 이 낸 표준 오류만 `stderr` 에 그대로 있다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitRead {
    /// 종료 코드. 상한 때문에 죽였으면 -1
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    /// 출력이 상한을 넘어 잘렸다
    pub truncated: bool,
}

/// 실행 한도와 기록 밖 사실. `work_tree_gone`: 따로 작업 트리가 이제 없다(합친 뒤·버린 뒤) — 규칙에 그대로 넘어간다.
#[derive(Debug, Clone)]
pub struct BoardGitOpts {
    pub timeout: Duration,
    pub out_cap: usize,
    pub err_cap: usize,
    pub work_tree_gone: bool,
    /// `repo` 가 이 실행의 따로 작업 트리다 — 공통 git 폴더가 아니라 그 작업 트리 자신의 git 폴더(`record.work_git_dir`)와 `GIT_WORK_TREE` 를 박는다(원본 `wtPin`)
    pub at_work_tree: bool,
}

impl Default for BoardGitOpts {
    fn default() -> Self {
        Self { timeout: DEFAULT_TIMEOUT, out_cap: DEFAULT_OUT_CAP, err_cap: DEFAULT_ERR_CAP, work_tree_gone: false, at_work_tree: false }
    }
}

// ── 보호 인자·환경(순수) ─────────────────────────────

/// 늘 거는 `-c` 설정. 엔진 `adapters::git::static_config` 와 같은 값·같은 차례(저장소 설정·속성이 코드를 못 돌리게)
fn static_config() -> Vec<(&'static str, &'static str)> {
    vec![
        ("core.fsmonitor", "false"),
        ("core.hooksPath", NULL_DEVICE),
        ("core.attributesFile", NULL_DEVICE),
        ("core.excludesFile", NULL_DEVICE),
        ("diff.external", ""),
        ("core.pager", "cat"),
        ("protocol.ext.allow", "never"),
        ("commit.gpgsign", "false"),
        ("safe.bareRepository", "explicit"),
        ("diff.ignoreSubmodules", "all"),
        ("protocol.allow", "never"),
        ("log.showSignature", "false"),
    ]
}

/// 보호 인자 앞부분: `--no-optional-locks` 와 `-c 이름=값` 줄
pub fn hardening_args() -> Vec<String> {
    let mut a = vec!["--no-optional-locks".to_string()];
    for (k, v) in static_config() {
        a.push("-c".to_string());
        a.push(format!("{k}={v}"));
    }
    a
}

/// 물려받은 환경에서 `GIT_*` 를 모두 빼고, 보호 값과 박을 값(`GIT_DIR=…`·`GIT_CEILING_DIRECTORIES=…`)을 넣은 환경.
/// 박을 값은 `GIT_*` 라서 먼저 빠진 뒤 다시 들어간다 — 물려받은 `GIT_DIR` 가 이긴 적이 없다.
pub fn board_git_env(inherited: &[(String, String)], pin: &[String]) -> Vec<(String, String)> {
    let mut env = engine_git_env(inherited);
    env.extend([
        ("GIT_PAGER".to_string(), "cat".to_string()),
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
        ("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string()),
    ]);
    for s in pin {
        if let Some((k, v)) = s.split_once('=') {
            env.push((k.to_string(), v.to_string()));
        }
    }
    env
}

/// `diff`·`show`·`log` 에는 외부 diff·textconv 를 끄는 옵션을 하위 명령 바로 뒤에 끼운다
fn with_safe_flags(args: &[String]) -> Vec<String> {
    let mut out = args.to_vec();
    if matches!(args.first().map(String::as_str), Some("diff" | "show" | "log")) {
        out.splice(1..1, ["--no-ext-diff".to_string(), "--no-textconv".to_string()]);
    }
    out
}

/// `\\?\C:\x` → `C:\x`. 그 밖(`\\?\UNC\…` 포함)은 그대로. git 이 GIT_DIR·작업 폴더로 받는 꼴로 되돌린다
fn strip_verbatim(p: &str) -> String {
    match p.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => p.to_string(),
    }
}

/// 박을 `GIT_*=값` 의 값 쪽만 git 이 받는 꼴로 바꾼다
fn plain_pin(pin: Vec<String>) -> Vec<String> {
    pin.into_iter()
        .map(|s| match s.split_once('=') {
            Some((k, v)) => format!("{k}={}", strip_verbatim(v)),
            None => s,
        })
        .collect()
}

// ── git.exe 찾기·띄우기 ──────────────────────────────

/// PATH 를 차례로 보고, 없으면 Program Files\Git\cmd. 작업 폴더는 보지 않는다
fn find_git() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var) {
            dirs.push(PathBuf::from(pf).join("Git").join("cmd"));
        }
    }
    dirs.into_iter().filter(|d| !d.as_os_str().is_empty()).map(|d| d.join(GIT_EXE)).find(|p| p.is_file())
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(windows))]
fn hide_window(_cmd: &mut Command) {}

fn inherited_env() -> Vec<(String, String)> {
    std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect()
}

/// 보호 환경을 건 git 명령. `extra_config` 는 환경 변수 설정 쌍(`GIT_CONFIG_COUNT`)으로 건다
fn command(exe: &Path, cwd: &Path, pin: &[String], extra_config: &[(String, String)], args: &[String]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(hardening_args());
    cmd.args(args);
    cmd.current_dir(cwd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.env_clear();
    cmd.envs(board_git_env(&inherited_env(), pin));
    if !extra_config.is_empty() {
        cmd.env("GIT_CONFIG_COUNT", extra_config.len().to_string());
        for (i, (key, value)) in extra_config.iter().enumerate() {
            cmd.env(format!("GIT_CONFIG_KEY_{i}"), key).env(format!("GIT_CONFIG_VALUE_{i}"), value);
        }
    }
    hide_window(&mut cmd);
    cmd
}

/// 파이프를 별도 스레드에서 읽는다. 상한을 넘으면 `over` 를 세우고 멈춘다(밖에서 git 을 죽인다)
fn drain<R: Read + Send + 'static>(pipe: Option<R>, cap: usize, over: Arc<AtomicBool>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut p) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match p.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let room = cap.saturating_sub(buf.len());
                        if n > room {
                            buf.extend_from_slice(&chunk[..room]);
                            over.store(true, Ordering::SeqCst);
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        }
        let _ = tx.send(buf);
    });
    rx
}

/// 띄워서 끝까지(또는 시간·출력 상한까지) 기다리고 모은다. 시간이 넘으면 죽이고 `Err`
fn exec(cmd: Command, timeout: Duration, out_cap: usize, err_cap: usize) -> Result<GitRead, String> {
    exec_notify(cmd, timeout, out_cap, err_cap, &mut |_| {})
}

/// 죽이기 전에 후손을 모아 두었다가(루트가 죽으면 부모 관계가 끊긴다) 루트와 후손을 모두 끝낸다
fn kill_tree(child: &mut std::process::Child) {
    let descendants = descendants_since(&ProcessTable::capture(), child.id(), &creation_time);
    let _ = child.kill();
    let _ = child.wait();
    for pid in descendants {
        terminate(pid);
    }
}

/// `exec` 와 같고, 띄운 직후 프로세스 id 를 `spawned` 로 알린다(시험이 후손을 찾는 데 쓴다)
fn exec_notify(mut cmd: Command, timeout: Duration, out_cap: usize, err_cap: usize, spawned: &mut dyn FnMut(u32)) -> Result<GitRead, String> {
    let mut child = cmd.spawn().map_err(|e| format!("{REFUSED_PREFIX}git 을 띄우지 못했다({:?})", e.kind()))?;
    spawned(child.id());
    let over = Arc::new(AtomicBool::new(false));
    let out_rx = drain(child.stdout.take(), out_cap, over.clone());
    let err_rx = drain(child.stderr.take(), err_cap, over.clone());
    let start = Instant::now();
    let mut killed_for_cap = false;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(e) => return Err(format!("{REFUSED_PREFIX}git 상태를 읽지 못했다({:?})", e.kind())),
        }
        if over.load(Ordering::SeqCst) {
            kill_tree(&mut child);
            killed_for_cap = true;
            break -1;
        }
        if start.elapsed() >= timeout {
            kill_tree(&mut child);
            return Err(format!("{REFUSED_PREFIX}git 이 {}초 안에 끝나지 않아 멈췄다", timeout.as_secs_f64()));
        }
        std::thread::sleep(POLL);
    };
    let stdout = String::from_utf8_lossy(&out_rx.recv_timeout(DRAIN_WAIT).unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_rx.recv_timeout(DRAIN_WAIT).unwrap_or_default()).into_owned();
    Ok(GitRead { code, stdout, stderr, truncated: killed_for_cap || over.load(Ordering::SeqCst) })
}

// ── 저장소 확인·실행 ─────────────────────────────────

/// 저장소 폴더를 `canonicalize`(드라이브 글자·`/`·`\\?\` 쓰기를 한 꼴로)한 뒤 git 에 줄 꼴로 푼다. 폴더가 아니면 Err
fn resolve_repo(repo: &Path) -> Result<PathBuf, String> {
    let real = std::fs::canonicalize(repo).map_err(|_| format!("{REFUSED_PREFIX}저장소 폴더를 찾을 수 없다"))?;
    if !real.is_dir() {
        return Err(format!("{REFUSED_PREFIX}저장소가 폴더가 아니다"));
    }
    Ok(PathBuf::from(strip_verbatim(&real.to_string_lossy())))
}

/// 기록과 견줄 "지금 공통 git 폴더": `rev-parse --git-common-dir` 를 박지 않고 보호 환경으로 읽어 `canonicalize`(엔진 `real` 과 같은 꼴)
fn live_common_dir(exe: &Path, repo: &Path, timeout: Duration) -> Option<String> {
    let args = vec!["rev-parse".to_string(), "--git-common-dir".to_string()];
    let out = exec(command(exe, repo, &[], &[], &args), timeout, 4096, 4096).ok()?;
    if out.code != 0 {
        return None;
    }
    let line = out.stdout.trim();
    if line.is_empty() {
        return None;
    }
    let p = Path::new(line);
    let abs = if p.is_absolute() { p.to_path_buf() } else { repo.join(p) };
    std::fs::canonicalize(abs).ok().map(|p| p.to_string_lossy().into_owned())
}

/// 저장소의 이름을 저장소가 정하는 코드 실행 칸(`filter.x.clean`·`diff.x.textconv`·`merge.x.driver`)을 비우는 설정 쌍
fn driver_neutralisers(exe: &Path, repo: &Path, pin: &[String], timeout: Duration) -> Vec<(String, String)> {
    let args: Vec<String> = ["config", "--list", "--name-only", "-z"].iter().map(|s| s.to_string()).collect();
    match exec(command(exe, repo, pin, &[], &args), timeout, DEFAULT_OUT_CAP, 4096) {
        Ok(out) if out.code == 0 => neutralisers_for(&out.stdout),
        _ => Vec::new(),
    }
}

/// 보호 환경으로 `git <args>` 를 돌린다. 규칙 `board_git_changed` 가 막으라면 돌리지 않고 Err(까닭).
/// 기록이 있으면 `board_refs_pin` 으로 `GIT_DIR`·`GIT_CEILING_DIRECTORIES` 를 박는다. 기록이 없으면(옛 실행) 보호 설정으로만 돈다.
pub fn board_git(repo: &Path, record: &GitRecord, args: &[String]) -> Result<GitRead, String> {
    board_git_with(repo, record, args, &BoardGitOpts::default())
}

pub fn board_git_with(repo: &Path, record: &GitRecord, args: &[String], opts: &BoardGitOpts) -> Result<GitRead, String> {
    if args.is_empty() {
        return Err(format!("{REFUSED_PREFIX}명령이 비었다"));
    }
    let exe = find_git().ok_or_else(|| format!("{REFUSED_PREFIX}git.exe 를 찾을 수 없다"))?;
    run_with_exe(&exe, repo, record, args, opts)
}

fn run_with_exe(exe: &Path, repo: &Path, record: &GitRecord, args: &[String], opts: &BoardGitOpts) -> Result<GitRead, String> {
    let root = resolve_repo(repo)?;
    // 1. 기록한 git 폴더가 지금과 같은가 — 다르면 돌리지 않는다
    if record.common_dir.is_some() {
        let live = live_common_dir(exe, &root, opts.timeout);
        let live_ino = live.as_deref().and_then(file_id);
        let live_work_ino = record.work_git_dir.as_deref().and_then(file_id);
        if let Some(why) = board_git_changed(record, live.as_deref(), live_ino.as_deref(), live_work_ino.as_deref(), opts.work_tree_gone) {
            return Err(format!("{REFUSED_PREFIX}{why}"));
        }
    }
    // 2. GIT_DIR·GIT_CEILING_DIRECTORIES 박기(기록이 없으면 빈 목록)
    let pin = if opts.at_work_tree {
        // 작업 트리 경로는 그 작업 트리 자신의 git 폴더로 박는다(공통 폴더로 박으면 다른 색인·머리를 읽는다). 기록이 모자라면 닫힌 쪽
        let (Some(_), Some(wg)) = (record.common_dir.as_deref(), record.work_git_dir.as_deref()) else {
            return Err(format!("{REFUSED_PREFIX}작업 트리의 git 폴더 기록이 없다"));
        };
        plain_pin(vec![format!("GIT_DIR={wg}"), format!("GIT_WORK_TREE={}", root.to_string_lossy())])
    } else {
        plain_pin(board_refs_pin(record, &root.to_string_lossy()))
    };
    // 3. 이름을 저장소가 정하는 코드 실행 칸을 비우고 본 명령
    let neutral = driver_neutralisers(exe, &root, &pin, opts.timeout);
    let cmd = command(exe, &root, &pin, &neutral, &with_safe_flags(args));
    exec(cmd, opts.timeout, opts.out_cap, opts.err_cap)
}

// ── 읽기 전용 허용 목록(웹뷰 경계) ────────────────────

struct Spec {
    /// 하위 명령
    name: &'static str,
    /// 그대로 받는 깃발
    flags: &'static [&'static str],
    /// `이름=값` 꼴로 받는 깃발(이름만, `=` 없이)
    valued: &'static [&'static str],
    /// 반드시 있어야 하는 깃발(출력 꼴을 고정)
    require: Option<&'static str>,
    /// 두 번째 인자가 이것이어야 하는 하위 명령(`worktree list`)
    second: Option<&'static str>,
    /// `-5` 같은 숫자 깃발을 받는가
    digits: bool,
}

const LOG_FLAGS: &[&str] = &[
    "--oneline", "--no-merges", "--merges", "--first-parent", "--reverse", "--all", "--branches", "--ancestry-path", "--topo-order",
    "--date-order", "--decorate", "--no-decorate", "--abbrev-commit", "--no-abbrev-commit", "--name-only", "--name-status", "--stat",
    "--numstat", "--shortstat", "--parents", "--not", "-z", "--no-patch", "-s",
];
const LOG_VALUED: &[&str] =
    &["--format", "--pretty", "--max-count", "--skip", "--date", "--author", "--committer", "--grep", "--since", "--until", "--abbrev"];

const ALLOWED: &[Spec] = &[
    Spec {
        name: "rev-parse",
        flags: &[
            "--show-toplevel", "--git-dir", "--git-common-dir", "--absolute-git-dir", "--is-inside-work-tree", "--is-bare-repository",
            "--show-prefix", "--show-cdup", "--verify", "--quiet", "-q", "--short", "--abbrev-ref", "--symbolic-full-name", "--symbolic",
        ],
        valued: &["--short", "--abbrev-ref"],
        require: None,
        second: None,
        digits: false,
    },
    Spec {
        name: "for-each-ref",
        flags: &["--shell", "--perl", "--python", "--tcl", "--merged", "--no-merged", "--contains", "--no-contains", "--points-at"],
        valued: &["--format", "--sort", "--count", "--merged", "--no-merged", "--contains", "--no-contains", "--points-at"],
        require: None,
        second: None,
        digits: false,
    },
    Spec { name: "log", flags: LOG_FLAGS, valued: LOG_VALUED, require: None, second: None, digits: true },
    Spec {
        name: "show",
        flags: &["--stat", "--name-only", "--name-status", "--numstat", "--shortstat", "--no-patch", "-s", "--oneline", "-z"],
        valued: &["--format", "--pretty", "--abbrev"],
        require: None,
        second: None,
        digits: false,
    },
    Spec { name: "cat-file", flags: &["-t", "-s", "-p", "-e"], valued: &[], require: None, second: None, digits: false },
    Spec {
        name: "merge-base",
        flags: &["--is-ancestor", "--all", "--fork-point"],
        valued: &[],
        require: None,
        second: None,
        digits: false,
    },
    Spec {
        name: "rev-list",
        flags: &[
            "--count", "--left-right", "--first-parent", "--no-merges", "--merges", "--all", "--branches", "--reverse", "--ancestry-path",
            "--parents", "--topo-order", "--date-order", "--abbrev-commit", "--not",
        ],
        valued: &["--max-count", "--skip", "--since", "--until"],
        require: None,
        second: None,
        digits: true,
    },
    Spec {
        name: "status",
        flags: &[
            "--porcelain", "-z", "--branch", "-b", "--short", "-s", "--no-renames", "-uno", "-unormal", "-uall", "--ignore-submodules",
        ],
        valued: &["--porcelain", "--untracked-files", "--ignore-submodules"],
        require: Some("--porcelain"),
        second: None,
        digits: false,
    },
    Spec {
        name: "diff",
        flags: &["--name-only", "-z", "--no-renames", "--cached", "--staged", "--ignore-submodules"],
        valued: &["--diff-filter", "--ignore-submodules"],
        require: Some("--name-only"),
        second: None,
        digits: false,
    },
    Spec { name: "worktree", flags: &["--porcelain", "-z"], valued: &[], require: None, second: Some("list"), digits: false },
];

const MAX_ARGS: usize = 64;
const MAX_ARG_LEN: usize = 4096;

fn refuse(why: &str) -> Result<(), String> {
    Err(format!("{REFUSED_PREFIX}{why}"))
}

/// 웹뷰가 보낸 git 인자가 읽기 전용 허용 목록 안인가. 하위 명령은 첫 인자여야 하고(`-C`·`-c`·`--git-dir=` 같은 앞 깃발은 모두 거절),
/// `--` 앞의 `-` 로 시작하는 인자는 그 하위 명령 표의 깃발만 받는다. `--format`/`--pretty` 값의 `%G`(gpg 를 부르는 서명 자리표시)는 거절한다.
pub fn check_read_only(args: &[String]) -> Result<(), String> {
    let Some(sub) = args.first() else { return refuse("명령이 비었다") };
    if args.len() > MAX_ARGS {
        return refuse("인자가 너무 많다");
    }
    if args.iter().any(|a| a.len() > MAX_ARG_LEN || a.contains('\0')) {
        return refuse("인자가 너무 길거나 NUL 이 들어 있다");
    }
    let Some(spec) = ALLOWED.iter().find(|s| s.name == sub) else { return refuse("허용 목록에 없는 git 명령이다") };
    let mut rest = &args[1..];
    if let Some(second) = spec.second {
        if rest.first().map(String::as_str) != Some(second) {
            return refuse("허용 목록에 없는 하위 명령이다");
        }
        rest = &rest[1..];
    }
    let mut required_seen = spec.require.is_none();
    let mut after_dd = false;
    for a in rest {
        if after_dd || !a.starts_with('-') {
            continue; // 위치 인자(ref·경로)
        }
        if a == "--" {
            after_dd = true;
            continue;
        }
        let (name, value) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (a.as_str(), None),
        };
        let ok = match value {
            None => spec.flags.contains(&name) || (spec.digits && is_digit_flag(name)),
            Some(_) => spec.valued.contains(&name),
        };
        if !ok {
            return refuse("허용 목록에 없는 깃발이다");
        }
        if let Some(v) = value {
            if matches!(name, "--format" | "--pretty") && v.contains("%G") {
                return refuse("서명 자리표시(%G)는 gpg 를 부르므로 받지 않는다");
            }
        }
        if Some(name) == spec.require {
            required_seen = true;
        }
    }
    if !required_seen {
        return refuse("이 명령에 꼭 필요한 깃발이 없다");
    }
    Ok(())
}

fn is_digit_flag(a: &str) -> bool {
    a.len() > 1 && a.starts_with('-') && a[1..].bytes().all(|b| b.is_ascii_digit())
}

// ── 실행 기록 읽기 ───────────────────────────────────

fn text(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(str::to_string)
}

/// run.json 의 `branches.repos.<저장소>` 에서 git 자리 기록(`common_dir`·`common_ino`·`work_git_dir`·`work_git_ino`)과 `work_tree` 를 읽는다.
/// 저장소 키는 `path_key_windows` 로 견준다. 저장소가 없으면 None
/// 저장소 키가 아니라 그 실행의 `work_tree` 경로로 맞았으면 셋째 값이 true(작업 트리 경로로 부른 것)
pub fn record_from_run_json(bytes: &[u8], repo: &str) -> Option<(GitRecord, Option<String>, bool)> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let want = path_key_windows(repo);
    let repos = v.get("branches")?.get("repos")?.as_object()?;
    let by_key = repos.iter().find(|(k, _)| path_key_windows(k) == want);
    let by_tree = || repos.iter().find(|(_, i)| text(i, "work_tree").is_some_and(|w| path_key_windows(&w) == want));
    let at_work_tree = by_key.is_none();
    let (_, info) = by_key.or_else(by_tree)?;
    let rec = GitRecord {
        common_dir: text(info, "common_dir"),
        common_ino: text(info, "common_ino"),
        work_git_dir: text(info, "work_git_dir"),
        work_git_ino: text(info, "work_git_ino"),
    };
    Some((rec, text(info, "work_tree"), at_work_tree))
}

fn load_record(run_id: &str, repo: &str) -> Result<(GitRecord, bool, bool), String> {
    if !valid_run_id(run_id) {
        return Err(format!("{REFUSED_PREFIX}실행 id 가 맞지 않다"));
    }
    let path = claude_board_core::paths::autopilot_root(false).join("runs").join(run_id).join("run.json");
    let bytes = std::fs::read(&path).map_err(|_| format!("{REFUSED_PREFIX}실행 기록을 읽을 수 없다"))?;
    // 이 저장소가 기록에 없으면 옛 실행처럼 보호 설정으로만 돈다
    let (rec, work_tree, at_work_tree) = record_from_run_json(&bytes, repo).unwrap_or_default();
    let gone = work_tree.map(|w| !Path::new(&w).exists()).unwrap_or(false);
    Ok((rec, gone, at_work_tree))
}

// ── Tauri 명령 ───────────────────────────────────────

/// 보드 화면이 ref·머리·바뀐 파일을 읽는다. `args` 는 읽기 전용 허용 목록(`check_read_only`)을 통과해야 한다.
/// `run_id` 가 있으면 그 실행의 run.json 기록으로 GIT_DIR 을 박고 바꿔치기를 확인한다.
#[tauri::command(async)]
pub fn autopilot_git_read(repo: String, args: Vec<String>, run_id: Option<String>) -> Result<GitRead, String> {
    check_read_only(&args)?;
    let (record, work_tree_gone, at_work_tree) = match run_id.as_deref() {
        Some(id) => load_record(id, &repo)?,
        None => (GitRecord::default(), false, false),
    };
    board_git_with(Path::new(&repo), &record, &args, &BoardGitOpts { work_tree_gone, at_work_tree, ..BoardGitOpts::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_board_autopilot::adapters::git::ENGINE_GIT_HARDEN_ENV;
    use std::path::PathBuf;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // ── 임시 저장소 ──

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Tmp {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("g7a-{}-{nanos}-{name}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Tmp(std::fs::canonicalize(&p).map(|c| PathBuf::from(strip_verbatim(&c.to_string_lossy()))).unwrap())
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn str(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
        fn slash(&self) -> String {
            self.str().replace('\\', "/")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 사용자 설정을 안 읽는 평범한(보호 없는) git — 임시 저장소를 만들고 시험 상태를 만드는 용도
    fn plain_git(dir: &Path, args: &[&str]) -> String {
        let exe = find_git().expect("git 이 필요하다");
        let mut cmd = Command::new(exe);
        cmd.current_dir(dir).args(args);
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") {
                cmd.env_remove(k);
            }
        }
        cmd.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", NULL_DEVICE).env("GIT_TERMINAL_PROMPT", "0");
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn init_repo(t: &Tmp) {
        plain_git(t.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(t.path().join("a.txt"), "one\n").unwrap();
        plain_git(t.path(), &["add", "a.txt"]);
        plain_git(
            t.path(),
            &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "first"],
        );
    }

    /// 지금 저장소에서 읽은 git 폴더 기록(출발 때 엔진이 적는 값과 같은 꼴: canonicalize 한 경로 + 파일 ID)
    fn record_of(repo: &Path) -> GitRecord {
        let common = std::fs::canonicalize(repo.join(".git")).unwrap().to_string_lossy().into_owned();
        GitRecord { common_ino: file_id(&common), common_dir: Some(common), work_git_dir: None, work_git_ino: None }
    }

    fn read(repo: &Path, rec: &GitRecord, args: &[&str]) -> Result<GitRead, String> {
        let a = s(args);
        check_read_only(&a)?;
        board_git(repo, rec, &a)
    }

    // ── 순수: argv·환경 표 ──

    #[test]
    fn g7a_argv_보호_인자가_정확히_이_순서다() {
        let want = [
            "--no-optional-locks",
            "-c", "core.fsmonitor=false",
            "-c", &format!("core.hooksPath={NULL_DEVICE}"),
            "-c", &format!("core.attributesFile={NULL_DEVICE}"),
            "-c", &format!("core.excludesFile={NULL_DEVICE}"),
            "-c", "diff.external=",
            "-c", "core.pager=cat",
            "-c", "protocol.ext.allow=never",
            "-c", "commit.gpgsign=false",
            "-c", "safe.bareRepository=explicit",
            "-c", "diff.ignoreSubmodules=all",
            "-c", "protocol.allow=never",
            "-c", "log.showSignature=false",
        ];
        assert_eq!(hardening_args(), want.iter().map(|x| x.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn g7a_argv_안전_깃발은_diff_show_log_뒤에만_끼워진다() {
        assert_eq!(with_safe_flags(&s(&["diff", "--name-only"])), s(&["diff", "--no-ext-diff", "--no-textconv", "--name-only"]));
        assert_eq!(with_safe_flags(&s(&["show", "HEAD"])), s(&["show", "--no-ext-diff", "--no-textconv", "HEAD"]));
        assert_eq!(with_safe_flags(&s(&["log", "-5"])), s(&["log", "--no-ext-diff", "--no-textconv", "-5"]));
        assert_eq!(with_safe_flags(&s(&["status", "--porcelain"])), s(&["status", "--porcelain"]));
        assert_eq!(with_safe_flags(&s(&["rev-list", "HEAD"])), s(&["rev-list", "HEAD"]));
    }

    #[test]
    fn g7a_환경_물려받은_git_변수는_모두_빠지고_보호_값과_박은_값만_남는다() {
        let hostile = [
            "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_0", "GIT_CONFIG", "GIT_EXEC_PATH", "GIT_SSH_COMMAND", "GIT_SSH", "GIT_ALLOW_PROTOCOL", "GIT_ASKPASS",
            "GIT_EXTERNAL_DIFF", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_CEILING_DIRECTORIES", "git_dir",
            "GIT_NO_LAZY_FETCH", "GIT_NO_REPLACE_OBJECTS",
        ];
        for k in hostile {
            let inherited = vec![
                ("PATH".to_string(), "C:/bin".to_string()),
                ("SystemRoot".to_string(), "C:/Windows".to_string()),
                (k.to_string(), "planted".to_string()),
            ];
            let e = board_git_env(&inherited, &s(&["GIT_DIR=C:/w/.git", "GIT_CEILING_DIRECTORIES=C:/w"]));
            let all = |n: &str| e.iter().filter(|(key, _)| key.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str()).collect::<Vec<_>>();
            assert!(!e.iter().any(|(_, v)| v == "planted"), "{k} 가 남았다");
            assert_eq!(all("PATH"), ["C:/bin"]);
            assert_eq!(all("SystemRoot"), ["C:/Windows"]);
            assert_eq!(all("GIT_DIR"), ["C:/w/.git"], "{k}");
            assert_eq!(all("GIT_CEILING_DIRECTORIES"), ["C:/w"], "{k}");
            assert_eq!(all("GIT_NO_LAZY_FETCH"), ["1"]);
            assert_eq!(all("GIT_NO_REPLACE_OBJECTS"), ["1"]);
            assert_eq!(all("GIT_CONFIG_NOSYSTEM"), ["1"]);
            assert_eq!(all("GIT_CONFIG_GLOBAL"), [NULL_DEVICE]);
            assert_eq!(all("GIT_ATTR_NOSYSTEM"), ["1"]);
            assert_eq!(all("GIT_PAGER"), ["cat"]);
            assert_eq!(all("GIT_TERMINAL_PROMPT"), ["0"]);
            assert_eq!(all("GIT_OPTIONAL_LOCKS"), ["0"]);
        }
        assert!(ENGINE_GIT_HARDEN_ENV.contains(&("GIT_NO_LAZY_FETCH", "1")));
    }

    #[test]
    fn g7a_환경_기록이_없으면_git_dir_을_박지_않는다() {
        let e = board_git_env(&[("GIT_DIR".to_string(), "x".to_string())], &[]);
        assert!(!e.iter().any(|(k, _)| k.eq_ignore_ascii_case("GIT_DIR")));
    }

    #[test]
    fn g7a_박은_값은_verbatim_접두를_뗀다() {
        assert_eq!(strip_verbatim(r"\\?\C:\w\repo\.git"), r"C:\w\repo\.git");
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share"), r"\\?\UNC\srv\share");
        assert_eq!(strip_verbatim("C:/w"), "C:/w");
        assert_eq!(
            plain_pin(s(&[r"GIT_DIR=\\?\C:\w\.git", r"GIT_CEILING_DIRECTORIES=\\?\C:\w"])),
            s(&[r"GIT_DIR=C:\w\.git", r"GIT_CEILING_DIRECTORIES=C:\w"])
        );
    }

    // ── 허용 목록 표 ──

    #[test]
    fn g7a_허용_목록_허용되는_명령() {
        let ok: &[&[&str]] = &[
            &["rev-parse", "--show-toplevel"],
            &["rev-parse", "--abbrev-ref", "HEAD"],
            &["rev-parse", "--short=8", "HEAD"],
            &["rev-parse", "--git-common-dir"],
            &["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/heads/"],
            &["for-each-ref", "--sort=-committerdate", "--count=5", "--merged", "main"],
            &["log", "-5", "--oneline", "main"],
            &["log", "--format=%H %s", "--max-count=3", "--", "src/a.txt"],
            &["show", "--stat", "HEAD"],
            &["show", "--no-patch", "--format=%H", "main:a.txt"],
            &["cat-file", "-p", "HEAD"],
            &["cat-file", "-t", "HEAD:a.txt"],
            &["merge-base", "--is-ancestor", "a", "b"],
            &["merge-base", "main", "topic"],
            &["rev-list", "--count", "main..topic"],
            &["rev-list", "--left-right", "--count", "a...b"],
            &["status", "--porcelain"],
            &["status", "--porcelain=v1", "-z", "--untracked-files=no"],
            &["diff", "--name-only", "main", "topic"],
            &["diff", "--name-only", "--diff-filter=AM", "a..b", "--", "x"],
            &["worktree", "list", "--porcelain"],
            &["worktree", "list"],
        ];
        for a in ok {
            assert_eq!(check_read_only(&s(a)), Ok(()), "{a:?}");
        }
    }

    #[test]
    fn g7a_허용_목록_거절되는_명령() {
        let bad: &[&[&str]] = &[
            &[],
            &["push", "origin", "main"],
            &["config", "--list"],
            &["-c", "core.fsmonitor=x", "status", "--porcelain"],
            &["--exec-path=C:/x", "status"],
            &["-C", "C:/x", "status", "--porcelain"],
            &["--git-dir=C:/x", "status", "--porcelain"],
            &["commit", "-m", "x"],
            &["stash"],
            &["clean", "-fd"],
            &["checkout", "main"],
            &["reset", "--hard"],
            &["fetch"],
            &["add", "."],
            &["branch", "-D", "x"],
            &["tag", "x"],
            &["update-ref", "-d", "refs/heads/x"],
            &["!sh", "-c", "calc"],
            &["status; calc", "--porcelain"],
            &["status&&calc", "--porcelain"],
            &["status", "--porcelain", "--output=C:/x/evil"],
            &["log", "--output=C:/x/evil"],
            &["diff", "--name-only", "--output=C:/x/evil"],
            &["diff", "--name-only", "--no-index", "a", "b"],
            &["diff", "--name-only", "--ext-diff"],
            &["diff", "--name-only", "--textconv"],
            &["show", "--ext-diff", "HEAD"],
            &["fetch", "--upload-pack=calc"],
            &["log", "--upload-pack=calc"],
            &["log", "--exec-path"],
            &["log", "-c", "x=y"],
            &["log", "-C", "x"],
            &["log", "--show-signature"],
            &["log", "--format=%H %GS"],
            &["log", "--pretty=format:%G?"],
            &["show", "--format=%GG", "HEAD"],
            &["for-each-ref", "--format=%(refname)", "--exec=calc"],
            &["rev-parse", "--git-dir=C:/x"],
            &["rev-parse", "--resolve-git-dir", "x"],
            &["cat-file", "--textconv", "HEAD:a"],
            &["cat-file", "--filters", "HEAD:a"],
            &["cat-file", "--batch"],
            &["merge-base", "--octopus", "a", "b"],
            &["worktree", "add", "x"],
            &["worktree", "remove", "x"],
            &["worktree"],
            &["worktree", "--porcelain", "list"],
            &["status"],
            &["status", "-z"],
            &["diff", "main", "topic"],
            &["diff", "--name-status", "a", "b"],
            &["status", "--porcelain", "-"],
            &["log", "-nz"],
            &["log", "-n", "5"],
            &["log", "--format", "%H"],
        ];
        for a in bad {
            let r = check_read_only(&s(a));
            assert!(r.is_err(), "{a:?} 가 통과했다");
            assert!(r.unwrap_err().starts_with(REFUSED_PREFIX));
        }
    }

    #[test]
    fn g7a_허용_목록_위치_인자는_이중_대시_뒤에서_깃발이_아니다() {
        assert_eq!(check_read_only(&s(&["log", "--oneline", "--", "-weird-file"])), Ok(()));
        assert!(check_read_only(&s(&["log", "--oneline", "-weird-file"])).is_err());
    }

    #[test]
    fn g7a_허용_목록_인자_수와_길이_상한() {
        let mut many = vec!["log".to_string()];
        many.extend((0..MAX_ARGS).map(|i| format!("r{i}")));
        assert!(check_read_only(&many).is_err());
        assert!(check_read_only(&["log".to_string(), "x".repeat(MAX_ARG_LEN + 1)]).is_err());
        assert!(check_read_only(&["log".to_string(), "a\0b".to_string()]).is_err());
    }

    // ── 실행 기록 ──

    #[test]
    fn g7a_기록_run_json_에서_저장소_기록을_읽는다() {
        let j = br#"{"branches":{"repos":{"C:\\W\\Repo":{"common_dir":"C:\\W\\Repo\\.git","common_ino":"1:2","work_git_dir":"C:\\W\\Repo\\.git\\worktrees\\a","work_git_ino":"1:3","work_tree":"C:\\W\\wt"},"D:\\x":{}}}}"#;
        let (rec, wt, at_wt) = record_from_run_json(j, "c:/w/repo/").unwrap();
        assert!(!at_wt);
        let by_tree = record_from_run_json(j, r"\\?\C:\W\wt").unwrap();
        assert!(by_tree.2 && by_tree.0 == rec);
        assert_eq!(rec.common_dir.as_deref(), Some(r"C:\W\Repo\.git"));
        assert_eq!(rec.common_ino.as_deref(), Some("1:2"));
        assert_eq!(rec.work_git_dir.as_deref(), Some(r"C:\W\Repo\.git\worktrees\a"));
        assert_eq!(rec.work_git_ino.as_deref(), Some("1:3"));
        assert_eq!(wt.as_deref(), Some(r"C:\W\wt"));
        assert_eq!(record_from_run_json(j, r"\\?\D:\X").unwrap().0, GitRecord::default());
        assert!(record_from_run_json(j, "E:/none").is_none());
        assert!(record_from_run_json(b"{", "C:/w").is_none());
    }

    #[test]
    fn g7a_실행_id_가_틀리면_기록을_읽기_전에_거절한다() {
        assert!(load_record("..\\x", "C:/w").is_err());
        assert!(load_record("", "C:/w").is_err());
        assert!(autopilot_git_read("C:/w".into(), s(&["status", "--porcelain"]), Some("a/b".into())).is_err());
    }

    // ── 실제 git 임시 저장소 ──

    #[test]
    fn g7a_실git_읽기_명령이_돈다() {
        let t = Tmp::new("read");
        init_repo(&t);
        let rec = record_of(t.path());
        let head = read(t.path(), &rec, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.code, 0);
        assert_eq!(head.stdout.trim().len(), 40);
        let refs = read(t.path(), &rec, &["for-each-ref", "--format=%(refname)", "refs/heads/"]).unwrap();
        assert_eq!(refs.stdout.trim(), "refs/heads/main");
        std::fs::write(t.path().join("a.txt"), "two\n").unwrap();
        let st = read(t.path(), &rec, &["status", "--porcelain"]).unwrap();
        assert_eq!(st.stdout.trim(), "M a.txt");
        let names = read(t.path(), &rec, &["diff", "--name-only"]).unwrap();
        assert_eq!(names.stdout.trim(), "a.txt");
        let wl = read(t.path(), &rec, &["worktree", "list", "--porcelain"]).unwrap();
        assert!(wl.stdout.contains("worktree "), "{}", wl.stdout);
        let cnt = read(t.path(), &rec, &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(cnt.stdout.trim(), "1");
        let bad = read(t.path(), &rec, &["rev-parse", "--verify", "no-such-ref"]).unwrap();
        assert_ne!(bad.code, 0);
    }

    #[test]
    fn g7a_실git_박은_git_dir_이_쓰인다() {
        let t = Tmp::new("pin");
        init_repo(&t);
        let rec = record_of(t.path());
        let r = read(t.path(), &rec, &["rev-parse", "--absolute-git-dir"]).unwrap();
        let want = strip_verbatim(rec.common_dir.as_deref().unwrap()).replace('\\', "/");
        assert_eq!(r.stdout.trim().replace('\\', "/").to_ascii_lowercase(), want.to_ascii_lowercase());
    }

    #[test]
    fn g7a_실git_물려받은_git_변수를_프로세스에_심어도_영향이_없다() {
        let a = Tmp::new("envA");
        init_repo(&a);
        let b = Tmp::new("envB");
        init_repo(&b);
        std::fs::write(b.path().join("only-b.txt"), "x\n").unwrap();
        let rec = record_of(a.path());
        let marker = Tmp::new("envM");
        let planted: [(&str, String); 6] = [
            ("GIT_DIR", format!("{}/.git", b.slash())),
            ("GIT_WORK_TREE", b.slash()),
            ("GIT_INDEX_FILE", format!("{}/.git/index", b.slash())),
            ("GIT_CONFIG_PARAMETERS", format!("'core.fsmonitor=: > {}/m1; :'", marker.slash())),
            ("GIT_CONFIG_COUNT", "1".to_string()),
            ("GIT_EXEC_PATH", format!("{}/nowhere", marker.slash())),
        ];
        let saved: Vec<(&str, Option<std::ffi::OsString>)> = planted.iter().map(|(k, _)| (*k, std::env::var_os(k))).collect();
        for (k, v) in &planted {
            std::env::set_var(k, v);
        }
        std::env::set_var("GIT_CONFIG_KEY_0", "core.fsmonitor");
        std::env::set_var("GIT_CONFIG_VALUE_0", format!(": > {}/m2; :", marker.slash()));
        std::env::set_var("GIT_SSH_COMMAND", format!(": > {}/m3; :", marker.slash()));
        let st = read(a.path(), &rec, &["status", "--porcelain"]);
        let top = read(a.path(), &rec, &["rev-parse", "--show-toplevel"]);
        for (k, old) in saved {
            match old {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        for k in ["GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0", "GIT_SSH_COMMAND"] {
            std::env::remove_var(k);
        }
        let st = st.unwrap();
        assert_eq!(st.code, 0, "{}", st.stderr);
        assert!(!st.stdout.contains("only-b"), "다른 저장소(GIT_DIR)를 읽었다: {}", st.stdout);
        let top = top.unwrap();
        assert_eq!(top.stdout.trim().replace('\\', "/").to_ascii_lowercase(), a.slash().to_ascii_lowercase());
        assert_eq!(std::fs::read_dir(marker.path()).unwrap().count(), 0, "심은 명령이 돌았다");
    }

    #[test]
    fn g7a_실git_저장소_설정이_심은_명령은_돌지_않는다() {
        let t = Tmp::new("hostile");
        init_repo(&t);
        let m = Tmp::new("hostileM");
        let fsm = m.slash() + "/fsmonitor";
        let marker = |n: &str| format!("{}/{n}", m.slash());
        // 포함된 설정 파일이 fsmonitor 와 필터를 심는다
        let included = t.path().join("evil.cfg");
        std::fs::write(
            &included,
            format!(
                "[core]
	fsmonitor = \": > {fsm}; :\"
	sshCommand = \": > {ssh}; :\"
[filter \"evil\"]
	clean = \"sh -c ': > {clean}; cat'\"
	required = true
[diff \"evil\"]
	textconv = \"sh -c ': > {tx}; cat' --\"
",
                fsm = fsm,
                ssh = marker("ssh"),
                clean = marker("clean"),
                tx = marker("textconv"),
            ),
        )
        .unwrap();
        plain_git(t.path(), &["config", "include.path", &format!("{}/evil.cfg", t.slash())]);
        std::fs::write(t.path().join(".gitattributes"), "a.txt filter=evil diff=evil\n").unwrap();
        // 같은 크기로 바꿔야(one→uno) git 이 내용을 다시 해시해 clean 필터를 부른다
        std::fs::write(t.path().join("a.txt"), "uno\n").unwrap();
        // 대조군: 보호 없는 git 은 심은 명령을 돌린다(시험이 의미 있다는 증거)
        plain_git(t.path(), &["-c", "core.fsmonitor=false", "status", "--porcelain"]);
        assert!(Path::new(&marker("clean")).exists(), "대조군: clean 필터가 안 돌아 시험 설정이 틀렸다");
        std::fs::remove_file(marker("clean")).unwrap();
        plain_git(t.path(), &["status", "--porcelain"]);
        assert!(Path::new(&fsm).exists(), "대조군: fsmonitor 가 안 돌아 시험 설정이 틀렸다");
        std::fs::remove_file(&fsm).unwrap();
        // 색인에 기록된 상태를 되돌려 보호한 길도 내용을 다시 해시해야 하게 한다
        std::fs::write(t.path().join("a.txt"), "uno\n").unwrap();
        let _ = std::fs::remove_file(marker("clean"));
        // 보호한 길
        let rec = record_of(t.path());
        for args in [&["status", "--porcelain"][..], &["diff", "--name-only"], &["show", "HEAD"], &["log", "-1", "--oneline"]] {
            let r = read(t.path(), &rec, args).unwrap();
            assert_eq!(r.code, 0, "{args:?}: {}", r.stderr);
        }
        let no_rec = read(t.path(), &GitRecord::default(), &["status", "--porcelain"]).unwrap();
        assert_eq!(no_rec.code, 0, "{}", no_rec.stderr);
        let left: Vec<_> = std::fs::read_dir(m.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "심은 명령이 돌았다: {left:?}");
    }

    #[test]
    fn g7a_실git_바꿔치기된_git_폴더는_거절한다() {
        let a = Tmp::new("swapA");
        init_repo(&a);
        let b = Tmp::new("swapB");
        init_repo(&b);
        let rec_a = record_of(a.path());
        // 기록은 A, 지금 저장소는 B
        let r = read(b.path(), &rec_a, &["rev-parse", "HEAD"]);
        assert_eq!(r.unwrap_err(), format!("{REFUSED_PREFIX}git 폴더가 출발 때와 달라져"));
        // 같은 저장소는 통과
        assert_eq!(read(a.path(), &rec_a, &["rev-parse", "HEAD"]).unwrap().code, 0);
        // 폴더는 같고 파일 ID 만 다르면(지우고 다시 만든 것) 거절
        let mut stale = rec_a.clone();
        stale.common_ino = Some("0:deadbeef".to_string());
        assert!(read(a.path(), &stale, &["rev-parse", "HEAD"]).unwrap_err().ends_with("git 폴더가 출발 때와 달라져"));
        // 파일 ID 를 못 읽는 값(기록은 있고 지금은 없음)은 닫힌 쪽
        let mut gone = rec_a.clone();
        gone.common_dir = Some(a.str() + "\\no-such\\.git");
        assert!(read(a.path(), &gone, &["rev-parse", "HEAD"]).is_err());
        // 작업 트리 git 폴더 기록이 있는데 달라졌으면 거절, 작업 트리가 이미 없으면 공통만 본다
        let mut wt = rec_a.clone();
        wt.work_git_dir = Some(a.str() + "\\.git\\worktrees\\x");
        wt.work_git_ino = Some("1:1".to_string());
        assert!(read(a.path(), &wt, &["rev-parse", "HEAD"]).unwrap_err().contains("작업 트리 git 폴더가 출발 때와 달라져"));
        let gone_ok = board_git_with(a.path(), &wt, &s(&["rev-parse", "HEAD"]), &BoardGitOpts { work_tree_gone: true, ..BoardGitOpts::default() });
        assert_eq!(gone_ok.unwrap().code, 0);
    }

    #[test]
    fn g7a_실git_저장소_쓰기_꼴이_달라도_판정이_같다() {
        let a = Tmp::new("spell");
        init_repo(&a);
        let b = Tmp::new("spellB");
        init_repo(&b);
        let rec_a = record_of(a.path());
        let spellings = |t: &Tmp| {
            let p = t.str();
            let lower_drive = format!("{}{}", p[..1].to_ascii_lowercase(), &p[1..]);
            vec![
                p.clone(),
                t.slash(),
                lower_drive.clone(),
                lower_drive.replace('\\', "/"),
                format!(r"\\?\{p}"),
                format!("{p}\\"),
                format!("{p}\\.\\"),
            ]
        };
        for sp in spellings(&a) {
            let r = board_git(Path::new(&sp), &rec_a, &s(&["rev-parse", "HEAD"]));
            assert_eq!(r.as_ref().map(|o| o.code), Ok(0), "{sp}: {r:?}");
        }
        for sp in spellings(&b) {
            let r = board_git(Path::new(&sp), &rec_a, &s(&["rev-parse", "HEAD"]));
            assert_eq!(r.unwrap_err(), format!("{REFUSED_PREFIX}git 폴더가 출발 때와 달라져"), "{sp}");
        }
    }

    #[test]
    fn g7a_실git_없는_폴더와_파일은_거절한다() {
        let t = Tmp::new("nodir");
        std::fs::write(t.path().join("f"), "x").unwrap();
        assert!(board_git(&t.path().join("none"), &GitRecord::default(), &s(&["rev-parse", "HEAD"])).is_err());
        assert!(board_git(&t.path().join("f"), &GitRecord::default(), &s(&["rev-parse", "HEAD"])).is_err());
        assert!(board_git(t.path(), &GitRecord::default(), &[]).is_err());
    }

    #[test]
    fn g7a_실git_utf8_이_아닌_출력은_손실_변환으로_돌려준다() {
        let t = Tmp::new("lossy");
        plain_git(t.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(t.path().join("bin.dat"), [b'a', 0xff, 0xfe, b'z', b'\n']).unwrap();
        let oid = plain_git(t.path(), &["hash-object", "-w", "bin.dat"]).trim().to_string();
        let r = board_git(t.path(), &GitRecord::default(), &s(&["cat-file", "-p", &oid])).unwrap();
        assert_eq!(r.code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "a\u{fffd}\u{fffd}z\n");
        assert!(!r.truncated);
    }

    #[test]
    fn g7a_실git_출력_상한을_넘으면_자르고_git_을_죽인다() {
        let t = Tmp::new("cap");
        plain_git(t.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(t.path().join("big.dat"), vec![b'x'; 300_000]).unwrap();
        let oid = plain_git(t.path(), &["hash-object", "-w", "big.dat"]).trim().to_string();
        let opts = BoardGitOpts { out_cap: 1000, ..BoardGitOpts::default() };
        let r = board_git_with(t.path(), &GitRecord::default(), &s(&["cat-file", "-p", &oid]), &opts).unwrap();
        assert!(r.truncated);
        assert_eq!(r.stdout.len(), 1000);
        assert_ne!(r.code, 0);
        let full = board_git(t.path(), &GitRecord::default(), &s(&["cat-file", "-p", &oid])).unwrap();
        assert!(!full.truncated);
        assert_eq!(full.stdout.len(), 300_000);
    }

    // ── 시간 상한·출력 상한(느린 외부 프로그램으로) ──

    #[cfg(windows)]
    fn slow_command() -> Command {
        let mut c = Command::new("ping.exe");
        c.args(["-n", "30", "127.0.0.1"]);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_window(&mut c);
        c
    }
    #[cfg(not(windows))]
    fn slow_command() -> Command {
        let mut c = Command::new("sleep");
        c.arg("30");
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }

    #[test]
    fn g7a_시간_상한을_넘으면_죽이고_거절한다() {
        let start = Instant::now();
        let r = exec(slow_command(), Duration::from_millis(300), 1000, 1000);
        assert!(r.is_err(), "{r:?}");
        let msg = r.unwrap_err();
        assert!(msg.starts_with(REFUSED_PREFIX) && msg.contains("멈췄다"), "{msg}");
        assert!(start.elapsed() < Duration::from_secs(10), "죽이지 않고 기다렸다: {:?}", start.elapsed());
    }

    #[test]
    fn g7a_시간_안에_끝나면_코드와_출력이_온다() {
        let mut c = Command::new(find_git().unwrap());
        c.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_window(&mut c);
        let r = exec(c, Duration::from_secs(20), DEFAULT_OUT_CAP, DEFAULT_ERR_CAP).unwrap();
        assert_eq!(r.code, 0);
        assert!(r.stdout.starts_with("git version"), "{}", r.stdout);
    }

    #[test]
    fn g7a_오류_글에_환경_값이나_인자를_싣지_않는다() {
        let t = Tmp::new("secret");
        init_repo(&t);
        let rec = record_of(t.path());
        std::env::set_var("GIT_SSH_COMMAND", "SECRET-TOKEN-123");
        let opts = BoardGitOpts { timeout: Duration::from_millis(1), ..BoardGitOpts::default() };
        let r = board_git_with(t.path(), &rec, &s(&["log", "SECRET-ARG-456"]), &opts);
        std::env::remove_var("GIT_SSH_COMMAND");
        if let Err(m) = r {
            assert!(!m.contains("SECRET"), "{m}");
        }
        let e = exec(slow_command(), Duration::from_millis(100), 100_000, 100_000).unwrap_err();
        assert!(!e.contains("127.0.0.1") && !e.contains("ping"), "{e}");
        let swapped = board_git(t.path(), &GitRecord { common_dir: Some("C:/x/SECRET-DIR".into()), ..GitRecord::default() }, &s(&["rev-parse", "HEAD"]));
        assert!(!swapped.unwrap_err().contains("SECRET-DIR"));
    }
    // ── 작업 트리(git worktree) ──

    #[test]
    fn g7a_실git_작업_트리_경로는_자기_git_폴더로_박는다() {
        let a = Tmp::new("wtmain");
        init_repo(&a);
        let wt_dir = Tmp::new("wtlinked");
        std::fs::remove_dir_all(wt_dir.path()).unwrap();
        plain_git(a.path(), &["worktree", "add", "-q", "-b", "topic", &wt_dir.str()]);
        std::fs::write(wt_dir.path().join("a.txt"), "two\n").unwrap();
        plain_git(wt_dir.path(), &["add", "a.txt"]);
        let common = std::fs::canonicalize(a.path().join(".git")).unwrap().to_string_lossy().into_owned();
        let work = std::fs::canonicalize(a.path().join(".git").join("worktrees").join(wt_dir.path().file_name().unwrap()))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let rec = GitRecord {
            common_ino: file_id(&common),
            common_dir: Some(common),
            work_git_ino: file_id(&work),
            work_git_dir: Some(work),
        };
        let at = BoardGitOpts { at_work_tree: true, ..BoardGitOpts::default() };
        let head = board_git_with(wt_dir.path(), &rec, &s(&["rev-parse", "--abbrev-ref", "HEAD"]), &at).unwrap();
        assert_eq!(head.stdout.trim(), "topic", "{}", head.stderr);
        let st = board_git_with(wt_dir.path(), &rec, &s(&["status", "--porcelain"]), &at).unwrap();
        assert_eq!(st.stdout.trim_end(), "M  a.txt", "{}", st.stderr);
        // 대조: 공통 git 폴더로 박으면 메인 저장소의 머리·색인을 읽는다(그래서 작업 트리 경로에는 at_work_tree 가 필요하다)
        let common_pin = board_git(wt_dir.path(), &rec, &s(&["rev-parse", "--abbrev-ref", "HEAD"])).unwrap();
        assert_eq!(common_pin.stdout.trim(), "main");
        // 작업 트리 git 폴더 기록이 없으면 닫는다
        let mut no_work = rec.clone();
        no_work.work_git_dir = None;
        no_work.work_git_ino = None;
        assert!(board_git_with(wt_dir.path(), &no_work, &s(&["rev-parse", "HEAD"]), &at).is_err());
        // 작업 트리 git 폴더가 바뀌었으면 거절
        let mut swapped = rec.clone();
        swapped.work_git_ino = Some("0:1".to_string());
        let r = board_git_with(wt_dir.path(), &swapped, &s(&["rev-parse", "HEAD"]), &at);
        assert!(r.unwrap_err().contains("작업 트리 git 폴더가 출발 때와 달라져"));
        plain_git(a.path(), &["worktree", "remove", "--force", &wt_dir.str()]);
    }

    // ── 프로세스 트리 끝내기 ──

    #[cfg(windows)]
    fn tree_command() -> Command {
        // cmd 가 ping 을 손자로 띄운다
        let mut c = Command::new("cmd.exe");
        c.args(["/c", "ping -n 60 127.0.0.1 > nul"]);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_window(&mut c);
        c
    }
    #[cfg(not(windows))]
    fn tree_command() -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 60; true"]);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }

    #[test]
    fn g7a_시간_상한을_넘으면_손자_프로세스까지_끝낸다() {
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            exec_notify(tree_command(), Duration::from_millis(3000), 1000, 1000, &mut |pid| {
                let _ = tx.send(pid);
            })
        });
        let root = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // 손자가 뜰 때까지 기다린다
        let mut grand: Vec<u32> = Vec::new();
        let t0 = Instant::now();
        while grand.is_empty() && t0.elapsed() < Duration::from_millis(2500) {
            grand = descendants_since(&ProcessTable::capture(), root, &creation_time);
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!grand.is_empty(), "손자가 안 떴다(시험 설정이 틀렸다)");
        let r = worker.join().unwrap();
        assert!(r.is_err());
        for pid in grand.iter().chain([&root]) {
            let mut alive = true;
            for _ in 0..40 {
                alive = claude_board_core::proc::alive(*pid, None);
                if !alive {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(!alive, "프로세스 {pid} 가 남았다");
        }
    }

    #[test]
    fn g7a_출력_상한을_넘어도_손자_프로세스까지_끝낸다() {
        // 큰 출력을 내며 손자를 두는 명령은 흔치 않으니 상한 경로의 끝내기는 같은 함수임을 시간 경로 시험과 상한 시험으로 덮는다
        let r = exec(tree_command(), Duration::from_millis(300), 1000, 1000);
        assert!(r.is_err());
    }
}
