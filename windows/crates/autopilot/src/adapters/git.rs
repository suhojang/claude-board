//! git 어댑터 — 엔진이 저장소를 만지는 유일한 길(`ports::Git`).
//! 늘 Windows `git.exe` 를 `-C <저장소>` 로 부르고, 저장소 안의 설정·훅이 코드를 돌리지 못하게 보호 환경을 건다.

use crate::ports::{Git, GitOutput, PortError, PortErrorKind, PortResult};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 기본 시간 상한. 넘으면 git 을 죽이고 `Timeout`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// 죽인 뒤 출력 읽는 쪽을 기다리는 시간. 손자 프로세스가 파이프를 쥐고 있어도 여기서 끊는다.
const DRAIN_WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

/// 설정·훅 파일 자리에 쓰는 빈 장치. 없는 파일처럼 읽힌다.
#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

#[cfg(windows)]
const GIT_EXE: &str = "git.exe";
#[cfg(not(windows))]
const GIT_EXE: &str = "git";

pub struct ProcessGit {
    timeout: Duration,
}

impl ProcessGit {
    pub fn new() -> Self {
        Self { timeout: DEFAULT_TIMEOUT }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for ProcessGit {
    fn default() -> Self {
        Self::new()
    }
}

impl Git for ProcessGit {
    fn run(&self, repo: &Path, args: &[&str]) -> PortResult<GitOutput> {
        let exe = find_git().ok_or_else(|| {
            PortError::new(PortErrorKind::Unavailable, "git.exe 를 찾을 수 없다(PATH 와 Program Files\\Git\\cmd 모두)")
        })?;
        run_protected(&exe, repo, args, self.timeout)
    }
}

// ── git.exe 찾기 ─────────────────────────────────────

/// PATH 를 차례로 보고, 없으면 Program Files\Git\cmd. 작업 폴더는 보지 않는다(Command::new 는 Windows 에서 작업 폴더부터 찾는다).
fn find_git() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var) {
            dirs.push(PathBuf::from(pf).join("Git").join("cmd"));
        }
    }
    dirs.into_iter()
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(GIT_EXE))
        .find(|p| p.is_file())
}

// ── 실행 ─────────────────────────────────────────────

/// 늘 거는 설정(`-c`). 저장소 안의 설정·속성 파일이 엔진 프로세스 권한으로 명령을 돌리지 못하게 한다.
/// 이름이 정해진 칸만 여기서 막고, 이름을 저장소가 정하는 칸(`filter.<이름>.*`·`diff.<이름>.*`·`merge.<이름>.*`)은
/// `driver_neutralisers` 가 저장소 설정을 읽어 찾아 막는다
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
        // macOS 엔진 git 보호(EngineGit.swift `apEngineGitHardenC`)와 같은 값, 같은 차례
        // 작업 폴더가 bare 저장소 꼴(HEAD·objects·refs)이어도 git 이 스스로 저장소로 쓰지 않게
        ("safe.bareRepository", "explicit"),
        // 모델이 심은 중첩 git(gitlink)으로 status·diff·switch 가 들어가 그 저장소의 filter 를 돌리지 않게
        ("diff.ignoreSubmodules", "all"),
        // 모든 전송 프로토콜을 끈다(명령줄 무게라 저장소 설정이 덮지 못함). 엔진은 fetch·push·clone 을 부르지 않는다
        ("protocol.allow", "never"),
        // git log 가 저장소 설정의 gpg.program 을 부르지 않게
        ("log.showSignature", "false"),
    ]
}

/// 엔진 git 에 박는 환경 값(EngineGit.swift `apEngineGitHardenEnv` 와 같은 이름·값, 설정 파일 자리는 Windows 빈 장치).
/// 모델이 저장소 설정·attributes·replace 객체에 명령을 심어도 밖의 git 이 실행하지 않게 하고,
/// 부분 복제 저장소에서 빠진 객체를 받으려고 원격 명령(`remote.*.uploadpack`·`core.sshCommand`)을 부르지 않게 한다
pub const ENGINE_GIT_HARDEN_ENV: [(&str, &str); 5] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", NULL_DEVICE),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
    ("GIT_NO_LAZY_FETCH", "1"),
];

/// `GIT_` 로 시작하는 환경 변수 이름인가(Windows 는 이름의 대소문자를 가리지 않는다)
fn is_git_var(name: &str) -> bool {
    name.to_ascii_uppercase().starts_with("GIT_")
}

/// 엔진 git 의 프로세스 환경(EngineGit.swift `apEngineGitEnv`): 물려받은 환경에서 `GIT_*` 를 모두 빼고 보호 값만 넣는다.
/// 물려받은 `GIT_CONFIG_PARAMETERS`·`GIT_CONFIG_COUNT/KEY/VALUE`·`GIT_SSH*`·`GIT_ALLOW_PROTOCOL`·`GIT_DIR`·`GIT_INDEX_FILE` 등이 남아 있으면 안 된다
pub fn engine_git_env(inherited: &[(String, String)]) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = inherited.iter().filter(|(k, _)| !is_git_var(k)).cloned().collect();
    env.extend(ENGINE_GIT_HARDEN_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    env
}

/// 저장소가 이름을 붙인 코드 실행 칸(`filter.x.clean` 처럼)을 찾아 비우는 설정 쌍.
/// 쓰는 일은 `git config --list` 읽기뿐이라 코드가 돌지 않는다. 설정을 못 읽으면 빈 목록(그 저장소에 대한 본 명령도 같은 까닭으로 실패한다)
fn driver_neutralisers(exe: &Path, repo: &Path, timeout: Duration) -> Vec<(String, String)> {
    let mut cmd = base_command(exe, repo, &[]);
    cmd.args(["config", "--list", "--name-only", "-z"]);
    match exec(cmd, "config", timeout) {
        Ok(out) if out.code == 0 => neutralisers_for(&out.stdout),
        _ => Vec::new(),
    }
}

/// `git config --list --name-only -z` 출력(NUL 로 끝나는 이름들) → 비울 칸 쌍. 순수 함수
pub fn neutralisers_for(names: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut put = |key: String, value: &str| {
        if !out.iter().any(|(k, _)| *k == key) {
            out.push((key, value.to_string()));
        }
    };
    for name in names.split('\0').filter(|n| !n.is_empty()) {
        let Some((section, rest)) = name.split_once('.') else { continue };
        let Some((sub, var)) = rest.rsplit_once('.') else { continue };
        match (section, var) {
            ("filter", "clean" | "smudge" | "process") => {
                put(name.to_string(), "");
                // 필수 필터가 비어 실패하면 status 가 죽는다 — 필수 표시도 끈다
                put(format!("filter.{sub}.required"), "false");
            }
            ("diff", "textconv" | "command") => put(name.to_string(), ""),
            ("merge", "driver") => put(name.to_string(), ""),
            _ => {}
        }
    }
    out
}

/// `diff`·`show`·`log` 에는 외부 diff·textconv 를 끄는 옵션을 앞에 끼운다(이미 있어도 무해)
fn with_safe_flags(args: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    if matches!(args.first().copied(), Some("diff" | "show" | "log")) {
        out.splice(1..1, ["--no-ext-diff".to_string(), "--no-textconv".to_string()]);
    }
    out
}

/// 보호 환경을 건 `git -C <저장소>` 명령. `extra_config` 는 환경 변수 설정 쌍(`GIT_CONFIG_COUNT`)으로 건다 —
/// 키에 `=` 가 있어도 `-c` 처럼 갈라지지 않는다
fn base_command(exe: &Path, repo: &Path, extra_config: &[(String, String)]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg("--no-optional-locks");
    cmd.arg("-C").arg(repo);
    // -c 는 저장소 설정보다 우선한다. 훅 폴더는 비워 훅을 못 찾게 한다
    for (key, value) in static_config() {
        cmd.arg("-c").arg(format!("{key}={value}"));
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_env(&mut cmd);
    if !extra_config.is_empty() {
        cmd.env("GIT_CONFIG_COUNT", extra_config.len().to_string());
        for (i, (key, value)) in extra_config.iter().enumerate() {
            cmd.env(format!("GIT_CONFIG_KEY_{i}"), key).env(format!("GIT_CONFIG_VALUE_{i}"), value);
        }
    }
    hide_window(&mut cmd);
    cmd
}

fn run_protected(exe: &Path, repo: &Path, args: &[&str], timeout: Duration) -> PortResult<GitOutput> {
    let neutral = driver_neutralisers(exe, repo, timeout);
    let mut cmd = base_command(exe, repo, &neutral);
    cmd.args(with_safe_flags(args));
    exec(cmd, args.first().copied().unwrap_or(""), timeout)
}

/// 띄워서 끝까지(또는 시간 상한까지) 기다리고 출력을 모은다
fn exec(mut cmd: Command, what: &str, timeout: Duration) -> PortResult<GitOutput> {
    let mut child = cmd.spawn()?;
    let out_rx = drain(child.stdout.take());
    let err_rx = drain(child.stderr.take());

    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait()? {
            // 신호로 끝나 코드가 없으면 -1
            break status.code().unwrap_or(-1);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PortError::new(
                PortErrorKind::Timeout,
                format!("git {what} 가 {}초 안에 끝나지 않았다", timeout.as_secs_f64()),
            ));
        }
        std::thread::sleep(POLL);
    };

    Ok(GitOutput {
        code,
        stdout: String::from_utf8_lossy(&out_rx.recv_timeout(DRAIN_WAIT).unwrap_or_default()).into_owned(),
        stderr: String::from_utf8_lossy(&err_rx.recv_timeout(DRAIN_WAIT).unwrap_or_default()).into_owned(),
    })
}

/// 상속된 git 환경을 걷어내고 보호 값을 건다.
fn apply_env(cmd: &mut Command) {
    // 물려받은 GIT_* 는 모두 뺀다 — GIT_DIR 등이 남아 있으면 -C 와 상관없이 엉뚱한 저장소를 만지고,
    // GIT_CONFIG_*(PARAMETERS·COUNT/KEY/VALUE 포함)·GIT_SSH*·GIT_ALLOW_PROTOCOL·GIT_EXTERNAL_DIFF·GIT_ATTR_SOURCE·GIT_PAGER·GIT_EDITOR·
    // GIT_ASKPASS·GIT_PROXY_COMMAND·GIT_EXEC_PATH 같은 변수는 코드를 돌리거나 보호를 풀 수 있다
    let stripped: Vec<OsString> =
        std::env::vars_os().map(|(k, _)| k).filter(|k| is_git_var(&k.to_string_lossy())).collect();
    for k in stripped {
        cmd.env_remove(k);
    }
    for (k, v) in ENGINE_GIT_HARDEN_ENV {
        cmd.env(k, v);
    }
    // Windows 어댑터가 더하는 값: 쪽수 넘김·입력 물음·선택적 잠금 끄기
    cmd.env("GIT_PAGER", "cat").env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0");
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(not(windows))]
fn hide_window(_cmd: &mut Command) {}

/// 파이프를 별도 스레드에서 끝까지 읽는다(가득 차 git 이 막히는 것을 막는다).
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

// ── 순수 도우미 ──────────────────────────────────────

/// `git status --porcelain=v1 -z` 출력 → (상태 두 글자, 경로). 바꾼 이름·복사는 새 경로만 남기고 옛 경로 항목은 건너뛴다.
pub fn parse_porcelain_status(out: &str) -> Vec<(String, String)> {
    let mut entries = out.split('\0').filter(|e| !e.is_empty());
    let mut result = Vec::new();
    while let Some(entry) = entries.next() {
        // "XY path": 상태 두 글자, 빈칸, 경로
        let Some(xy) = entry.get(..2) else { continue };
        let Some(path) = entry.get(3..) else { continue };
        if xy.contains('R') || xy.contains('C') {
            entries.next(); // 옛 경로
        }
        result.push((xy.to_string(), path.to_string()));
    }
    result
}

/// `git diff --shortstat`(또는 `--stat` 끝 줄) → (파일 수, 추가 줄, 지운 줄). 요약 줄이 없으면 (0, 0, 0).
pub fn parse_diff_stat(out: &str) -> (usize, usize, usize) {
    let Some(line) = out.lines().rev().find(|l| l.contains("changed")) else {
        return (0, 0, 0);
    };
    let mut files = 0;
    let mut ins = 0;
    let mut del = 0;
    for part in line.split(',') {
        let part = part.trim();
        let Some(n) = part.split_whitespace().next().and_then(|w| w.parse::<usize>().ok()) else { continue };
        if part.contains("changed") {
            files = n;
        } else if part.contains("insertion") {
            ins = n;
        } else if part.contains("deletion") {
            del = n;
        }
    }
    (files, ins, del)
}

/// 자율주행 가지 이름. 실행 id 는 `[A-Za-z0-9._-]{1,64}` 이고 `..` 을 품지 않아야 한다(가지 이름 규칙·경로 탈출 방지).
pub fn autopilot_branch_name(run_id: &str) -> PortResult<String> {
    let ok_chars = run_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let ok = ok_chars
        && !run_id.is_empty()
        && run_id.len() <= 64
        && !run_id.contains("..")
        // git 은 '.lock' 으로 끝나거나 '.' 으로 시작·끝나는 ref 이름을 거부한다
        && !run_id.starts_with('.')
        && !run_id.ends_with('.')
        && !run_id.ends_with(".lock");
    if ok {
        Ok(format!("autopilot/{run_id}"))
    } else {
        Err(PortError::new(PortErrorKind::Invalid, format!("실행 id 가 가지 이름으로 맞지 않다: {run_id:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 엔진 git 보호(EngineGitTests.swift 이름을 따른다) ──

    fn harden_c_args() -> Vec<String> {
        static_config().iter().flat_map(|(k, v)| ["-c".to_string(), format!("{k}={v}")]).collect()
    }

    #[test]
    fn engine_git_옮기기_macos_값과_같은_글자() {
        assert_eq!(
            ENGINE_GIT_HARDEN_ENV[..4],
            [
                ("GIT_CONFIG_NOSYSTEM", "1"),
                ("GIT_CONFIG_GLOBAL", NULL_DEVICE),
                ("GIT_ATTR_NOSYSTEM", "1"),
                ("GIT_NO_REPLACE_OBJECTS", "1")
            ]
        );
        let c = harden_c_args().join(" ");
        for want in [
            "-c core.fsmonitor=false",
            &format!("-c core.hooksPath={NULL_DEVICE}"),
            "-c commit.gpgsign=false",
            &format!("-c core.attributesFile={NULL_DEVICE}"),
            "-c safe.bareRepository=explicit",
            "-c diff.ignoreSubmodules=all",
        ] {
            assert!(c.contains(want), "{want} 가 없다: {c}");
        }
    }

    #[test]
    fn engine_git_늦은받기_전송프로토콜_서명확인을_끈다() {
        assert!(ENGINE_GIT_HARDEN_ENV.contains(&("GIT_NO_LAZY_FETCH", "1")));
        let c = harden_c_args();
        assert_eq!(c[c.len() - 4..], ["-c", "protocol.allow=never", "-c", "log.showSignature=false"]);
    }

    #[test]
    fn engine_git_물려받은_git_변수는_모두_빠지고_보호_값만_남는다() {
        for k in [
            "GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0", "GIT_SSH", "GIT_SSH_COMMAND",
            "GIT_ALLOW_PROTOCOL", "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_EXEC_PATH", "GIT_ASKPASS", "GIT_PROTOCOL_FROM_USER", "git_dir",
        ] {
            let inherited = vec![
                ("PATH".to_string(), "C:/bin".to_string()),
                ("USERPROFILE".to_string(), "C:/Users/toy".to_string()),
                (k.to_string(), "planted".to_string()),
                ("GIT_CONFIG_GLOBAL".to_string(), "C:/w/planted.cfg".to_string()),
            ];
            let e = engine_git_env(&inherited);
            let get = |n: &str| e.iter().find(|(key, _)| key == n).map(|(_, v)| v.as_str());
            assert_eq!(get(k), if k == "GIT_CONFIG_GLOBAL" { Some(NULL_DEVICE) } else { None }, "{k}");
            assert_eq!(get("PATH"), Some("C:/bin"));
            assert_eq!(get("USERPROFILE"), Some("C:/Users/toy"));
            assert_eq!(get("GIT_CONFIG_GLOBAL"), Some(NULL_DEVICE));
            assert_eq!(get("GIT_NO_LAZY_FETCH"), Some("1"));
            assert_eq!(get("GIT_NO_REPLACE_OBJECTS"), Some("1"));
            let mut git_keys: Vec<&str> = e.iter().map(|(key, _)| key.as_str()).filter(|n| is_git_var(n)).collect();
            git_keys.sort_unstable();
            assert_eq!(
                git_keys,
                ["GIT_ATTR_NOSYSTEM", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM", "GIT_NO_LAZY_FETCH", "GIT_NO_REPLACE_OBJECTS"],
                "{k}"
            );
        }
    }

    #[test]
    fn porcelain_basic_and_rename() {
        let out = " M a.txt\0?? b c.txt\0R  new.txt\0old.txt\0A  z\0";
        assert_eq!(
            parse_porcelain_status(out),
            vec![
                (" M".into(), "a.txt".into()),
                ("??".into(), "b c.txt".into()),
                ("R ".into(), "new.txt".into()),
                ("A ".into(), "z".into()),
            ]
        );
        assert!(parse_porcelain_status("").is_empty());
    }

    #[test]
    fn diff_stat_lines() {
        assert_eq!(parse_diff_stat(" 3 files changed, 10 insertions(+), 2 deletions(-)\n"), (3, 10, 2));
        assert_eq!(parse_diff_stat(" 1 file changed, 1 insertion(+)\n"), (1, 1, 0));
        assert_eq!(parse_diff_stat(" 1 file changed, 4 deletions(-)\n"), (1, 0, 4));
        assert_eq!(parse_diff_stat(" a | 2 +-\n 1 file changed, 1 insertion(+), 1 deletion(-)\n"), (1, 1, 1));
        assert_eq!(parse_diff_stat(""), (0, 0, 0));
    }
}
