//! Windows 프로세스 띄우기 — 자율주행이 claude 와 판정 명령을 띄우는 유일한 길(`native_env` 가 쓴다).
//!
//! - 창을 만들지 않고(`CREATE_NO_WINDOW`), 환경 변수는 부모 것을 허용 목록(`ENV_ALLOW`·`ENV_ALLOW_PREFIXES`)만 물려받은 뒤 호출자 쌍을 얹는다.
//!   이름에 비밀 낱말이 든 변수(`is_secret_name`)는 허용 목록에 걸려도 물려주지 않는다(호출자가 `spec.env` 로 직접 준 것만 간다).
//! - `CREATE_SUSPENDED` 로 만들어 Job Object(`KILL_ON_JOB_CLOSE`)에 넣은 **뒤에** 첫 스레드를 깨운다 — 자손이 Job 밖으로 새는 틈이 없다.
//!   그래서 `WinProcess` 를 떨어뜨리면(엔진이 죽어도) 자손까지 끝난다.
//! - Store 로 깐 PowerShell 7(MSIX 패키지 앱)이 띄운 자식은 Job 밖으로 빠져나간다(실측). 그래서 `kill_tree` 는 Job 을 끝내기 전에
//!   프로세스 표에서 후손을 모아 두었다가 남은 것을 직접 끝낸다. 엔진 프로세스가 갑자기 죽는 경우는 Job 만 믿는다(정직한 한계).
//! - 자식이 물려받는 핸들은 `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` 로 표준 입출력 셋뿐이다. 다른 스레드가 동시에 만든 파이프 끝이
//!   자식에게 새어 EOF 가 오지 않는 일이 없다.
//! - Windows 가 아니면 같은 이름의 껍데기가 컴파일되고 `spawn` 은 `Unavailable` 을 낸다(순수 함수는 어디서나 돈다).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::ports::{PortError, PortErrorKind, PortResult};

/// 부모 환경에서 자식에게 물려주는 이름(대소문자 무시). 이 밖의 변수(토큰·`PSModulePath`·`GIT_DIR` …)는 물려주지 않는다.
/// 자격 증명·ssh 에이전트 같은 것은 엔진이 `launch.env` 로 고른 것만 넘긴다.
pub const ENV_ALLOW: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATH",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "HOME",
    "HOMEDRIVE",
    "HOMEPATH",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "GIT_INSTALL_ROOT",
    "GIT_EXEC_PATH",
    // macOS 엔진의 `envKeep` 과 같은 이름(프록시·인증서·도구 위치는 Windows 빌드도 기댄다)
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "TMPDIR",
    "TERM",
    "JAVA_HOME",
    "DOCKER_HOST",
    "DOCKER_CONFIG",
    "DOCKER_CONTEXT",
    "DOCKER_CERT_PATH",
    "DOCKER_TLS_VERIFY",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "CLAUDE_CONFIG_DIR",
];

/// 이름이 이 머리말로 시작하면 물려준다(macOS `envKeepPrefixes`). 비밀 낱말이 든 이름은 여기서도 걸러진다
pub const ENV_ALLOW_PREFIXES: &[&str] = &["LC_", "GRADLE_", "SDKMAN_"];

/// 명령줄 길이 한도(글자 수, 끝 NUL 제외) — `CreateProcessW` 는 32,767 글자까지
const COMMAND_LINE_MAX: usize = 32_766;

/// 자식의 표준 입출력을 어떻게 이을까
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Io {
    /// 표준 입력·출력은 파이프(줄 읽기·쓰기), 표준 오류는 `stderr_log` 에 덧붙인다(claude)
    Streaming { stderr_log: PathBuf },
    /// 표준 입력은 NUL, 표준 출력과 오류를 같은 파일 `log` 에 함께 쓴다(판정 명령 — `> log 2>&1` 과 같다)
    MergedToFile { log: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    /// 실행 파일 절대 경로(PATH 를 뒤지지 않고 이 파일을 그대로 띄운다. cmd.exe 를 거치지 않는다)
    pub program: PathBuf,
    /// 명령줄 인용 규칙(`CommandLineToArgvW`)으로 이어 붙는다
    pub args: Vec<String>,
    /// 인용하지 않고 맨 끝에 그대로 붙일 글(`cmd /s /c "…"` 의 따옴표 덩어리)
    pub raw_tail: Option<String>,
    pub cwd: PathBuf,
    /// 호출자가 정한 환경 변수. 허용 목록으로 걸러 물려받은 부모 환경 위에 얹는다(같은 이름은 이쪽이 이긴다)
    pub env: Vec<(String, String)>,
    pub io: Io,
}

// ── 순수 함수 ────────────────────────────────────────

/// 이름에 비밀 낱말이 들었는가(macOS `secretName` 정규식과 같은 뜻):
/// `TOKEN|SECRET|PASSW|CREDENTIAL|PRIVATE|API_?KEY|(^|_)KEY($|_)|AUTH`, 대소문자 무시
pub fn is_secret_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSW", "CREDENTIAL", "PRIVATE", "APIKEY", "API_KEY", "AUTH"].iter().any(|w| up.contains(w))
        || up.split('_').any(|part| part == "KEY")
}

fn in_allow_list(name: &str) -> bool {
    if is_secret_name(name) {
        return false;
    }
    let has_prefix = |p: &&str| name.get(..p.len()).is_some_and(|head| head.eq_ignore_ascii_case(p));
    ENV_ALLOW.iter().any(|a| a.eq_ignore_ascii_case(name)) || ENV_ALLOW_PREFIXES.iter().any(has_prefix)
}

/// 부모 환경(`parent`) 가운데 허용 목록만 남기고 `extra` 를 얹는다. `HOME` 이 없으면 `USERPROFILE` 에서 만든다.
/// 같은 이름(대소문자 무시)은 하나만 남는다 — `extra` 가 이긴다.
pub fn build_env(parent: &[(String, String)], extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, value) in parent.iter().filter(|(n, _)| in_allow_list(n)) {
        put_env(&mut out, name, value);
    }
    let has_home = out.iter().any(|(n, _)| n.eq_ignore_ascii_case("HOME"));
    let profile = out.iter().find(|(n, _)| n.eq_ignore_ascii_case("USERPROFILE")).map(|(_, v)| v.clone());
    if let (false, Some(profile)) = (has_home, profile) {
        put_env(&mut out, "HOME", &profile);
    }
    for (name, value) in extra {
        put_env(&mut out, name, value);
    }
    out
}

fn put_env(env: &mut Vec<(String, String)>, name: &str, value: &str) {
    env.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    env.push((name.to_string(), value.to_string()));
}

/// 이 프로세스의 환경에서 `build_env` 의 `parent` 로 쓸 쌍(유니코드가 아닌 것은 뺀다)
pub fn current_env() -> Vec<(String, String)> {
    std::env::vars_os().filter_map(|(n, v)| Some((n.into_string().ok()?, v.into_string().ok()?))).collect()
}

/// `CreateProcessW` 의 환경 블록(UTF-16, 이름 대문자 순, 각 `이름=값\0`, 끝 `\0\0`). 이름이 비었거나 `=`·NUL 이 있으면 Invalid
pub fn env_block(env: &[(String, String)]) -> PortResult<Vec<u16>> {
    let mut sorted: Vec<&(String, String)> = env.iter().collect();
    sorted.sort_by_key(|(n, _)| n.to_uppercase());
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in sorted {
        if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
            return Err(PortError::new(PortErrorKind::Invalid, format!("환경 변수 꼴이 틀렸다: {name:?}")));
        }
        block.extend(format!("{name}={value}").encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

/// 명령줄 한 줄. 실행 파일은 늘 큰따옴표로 감싸고, 인자는 인용 규칙으로, `raw_tail` 은 그대로 뒤에 붙인다
pub fn command_line(spec: &SpawnSpec) -> PortResult<String> {
    let mut line = format!("\"{}\"", spec.program.display());
    if !spec.args.is_empty() {
        line.push(' ');
        line.push_str(&claude_board_core::quote::join_windows(&spec.args));
    }
    if let Some(tail) = &spec.raw_tail {
        line.push(' ');
        line.push_str(tail);
    }
    if line.contains('\0') {
        return Err(PortError::new(PortErrorKind::Invalid, "명령줄에 NUL 글자가 있다"));
    }
    if line.encode_utf16().count() > COMMAND_LINE_MAX {
        return Err(PortError::new(PortErrorKind::Invalid, "명령줄이 32,767자를 넘는다(긴 글은 파일로 넘긴다)"));
    }
    Ok(line)
}

/// `\\?\C:\…` 의 `\\?\` 를 떼어 낸다(`\\?\UNC\…` 와 그 밖은 그대로)
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") && rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// 바이트 흐름을 `\n` 으로 갈라 줄마다 보낸다(줄 끝 `\r` 은 뗀다). 끝에 줄바꿈 없는 조각이 남으면 그것도 한 줄이다.
/// 읽기가 끝나거나(`Ok(0)`·오류) 받는 쪽이 사라지면 돌려준다 — 보내는 쪽(`tx`)이 떨어지는 것이 EOF 신호다.
pub fn pump_lines<R: Read>(mut reader: R, tx: Sender<Vec<u8>>) {
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        pending.extend_from_slice(&chunk[..n]);
        while let Some(at) = pending.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = pending.drain(..=at).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if tx.send(line).is_err() {
                return;
            }
        }
    }
    if !pending.is_empty() {
        if pending.last() == Some(&b'\r') {
            pending.pop();
        }
        let _ = tx.send(pending);
    }
}

/// 콘솔 OEM 코드 페이지(cmd 의 출력) → 글. Windows 가 아니면 UTF-8 로 푼다
pub fn decode_oem(bytes: &[u8]) -> String {
    sys::decode_oem(bytes)
}

/// `root` 의 후손 pid(자식·손자 …, root 자신은 뺀다). 프로세스 표의 부모 관계만 따른다
pub fn descendants(table: &claude_board_core::proc::ProcessTable, root: u32) -> Vec<u32> {
    let mut found: Vec<u32> = Vec::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        for child in table.children(pid) {
            if child != root && !found.contains(&child) {
                found.push(child);
                queue.push(child);
            }
        }
    }
    found
}

/// `descendants` 와 같되 **생성 시각**도 본다: 자식은 부모보다 나중에 만들어졌어야 한다.
/// 부모가 죽고 그 pid 를 다른 프로세스가 물려받으면 표에는 옛 자식이 새 pid 의 자식으로 남는데, 그것을 후손으로 세어 끝내면
/// 남의 프로세스를 죽인다. `ctime_of(root)` 가 없으면 시각을 견줄 수 없으니 부모 관계만 따른다(`descendants`).
/// 후보의 시각을 모르면 후손으로 세지 않는다 — 시각을 못 읽는 프로세스는 `terminate` 도 열지 못한다
pub fn descendants_since(
    table: &claude_board_core::proc::ProcessTable,
    root: u32,
    ctime_of: &dyn Fn(u32) -> Option<f64>,
) -> Vec<u32> {
    let Some(root_time) = ctime_of(root) else { return descendants(table, root) };
    let mut found: Vec<u32> = Vec::new();
    let mut queue = vec![(root, root_time)];
    while let Some((pid, born)) = queue.pop() {
        for child in table.children(pid) {
            if child == root || found.contains(&child) {
                continue;
            }
            if let Some(t) = ctime_of(child).filter(|t| *t >= born) {
                found.push(child);
                queue.push((child, t));
            }
        }
    }
    found
}

// ── 프로세스 ─────────────────────────────────────────

/// 띄운 프로세스 하나와 그 Job. 떨어뜨리면 Job 이 닫혀 자손까지 끝난다.
pub struct WinProcess {
    inner: sys::Inner,
    lines: Option<Receiver<Vec<u8>>>,
    /// 표준 입력 쓰기 스레드로 가는 길. None 이면 닫힌 것(스레드는 맡은 줄을 다 쓴 뒤 입력을 닫는다)
    stdin: Option<Sender<Vec<u8>>>,
}

/// 표준 입력을 쥔 스레드. 자식이 안 읽어 파이프(64 KB)가 차면 이 스레드만 막히고 부르는 쪽(엔진 루프)은 계속 돈다.
/// 쓰기가 실패하면(자식이 끝났다) 스레드가 끝나 보내는 길이 닫히고, 다음 `write_line` 이 오류를 낸다
fn spawn_stdin_writer(mut file: std::fs::File) -> PortResult<Sender<Vec<u8>>> {
    use std::io::Write;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::Builder::new().name("winproc-stdin".into()).spawn(move || {
        for bytes in rx {
            if file.write_all(&bytes).and_then(|()| file.flush()).is_err() {
                break;
            }
        }
    })?;
    Ok(tx)
}

impl WinProcess {
    /// 만들고 Job 에 넣은 뒤 깨운다. `Io::Streaming` 이면 줄 읽기·쓰기가 되고, `MergedToFile` 이면 읽을 것이 없다(`read_line` 은 곧바로 EOF)
    pub fn spawn(spec: &SpawnSpec) -> PortResult<WinProcess> {
        let (mut inner, stdout) = sys::spawn(spec)?;
        let stdin = inner.take_stdin().map(spawn_stdin_writer).transpose()?;
        let lines = stdout.map(|reader| {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("winproc-stdout".into())
                .spawn(move || pump_lines(reader, tx))
                .map(|_| rx)
                .map_err(PortError::from)
        });
        let lines = lines.transpose()?;
        Ok(WinProcess { inner, lines, stdin })
    }

    pub fn pid(&self) -> u32 {
        self.inner.pid()
    }

    /// 한 줄(`\n` 을 붙여)을 쓰기 스레드에 맡기고 곧바로 돌아온다. 자식이 안 읽어 파이프(64 KB)가 차도 막히지 않는다.
    /// 줄은 맡긴 차례대로 쓰인다. 입력을 닫았거나 자식이 끝나 쓰기 스레드가 멈췄으면 오류
    pub fn write_line(&mut self, line: &str) -> PortResult<()> {
        let tx = self.stdin.as_ref().ok_or_else(|| PortError::new(PortErrorKind::Io, "표준 입력이 닫혀 있다"))?;
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        tx.send(bytes).map_err(|_| PortError::new(PortErrorKind::Io, "표준 입력에 쓰지 못했다(자식이 끝났거나 입력이 닫혔다)"))
    }

    /// 표준 입력을 닫는다(자식에게 EOF). 맡겨 둔 줄은 다 쓴 뒤에 닫힌다. 이미 닫았으면 아무 일 없다
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// 표준 출력 한 줄(UTF-8 로 푼 것, 줄 끝 `\r\n`·`\n` 뗌). Ok(None) = EOF. 시간 안에 없으면 Timeout
    pub fn read_line(&mut self, timeout: Duration) -> PortResult<Option<String>> {
        let Some(rx) = &self.lines else { return Ok(None) };
        match rx.recv_timeout(timeout) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
            Err(RecvTimeoutError::Timeout) => Err(PortError::new(PortErrorKind::Timeout, "표준 출력에 줄이 오지 않았다")),
        }
    }

    pub fn try_wait(&mut self) -> PortResult<Option<i32>> {
        self.inner.wait(Duration::ZERO)
    }

    /// 끝나면 종료 코드, 시간이 다 되면 None
    pub fn wait_timeout(&mut self, timeout: Duration) -> PortResult<Option<i32>> {
        self.inner.wait(timeout)
    }

    /// Job 째로 끝낸다(자손 포함). 이미 끝났어도 된다
    pub fn kill_tree(&mut self) -> PortResult<()> {
        self.inner.kill_tree()
    }
}

impl Drop for WinProcess {
    /// 떨어뜨리면 자손까지 끝낸다(엔진이 죽어도 남지 않게). 이미 끝났으면 아무 일 없다
    fn drop(&mut self) {
        let _ = self.inner.kill_tree();
    }
}

#[cfg(windows)]
mod sys {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::fs::{File, OpenOptions};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList, ResumeThread,
        TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
        CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };

    pub struct Inner {
        pid: u32,
        process: OwnedHandle,
        job: OwnedHandle,
        stdin: Option<File>,
        exit: Option<i32>,
    }

    fn os_error(what: &str) -> PortError {
        let e = std::io::Error::last_os_error();
        let kind = PortError::from(std::io::Error::from_raw_os_error(e.raw_os_error().unwrap_or(0))).kind;
        PortError::new(kind, format!("{what}: {e}"))
    }

    fn wide(s: &OsStr) -> PortResult<Vec<u16>> {
        let mut v: Vec<u16> = s.encode_wide().collect();
        if v.contains(&0) {
            return Err(PortError::new(PortErrorKind::Invalid, "경로나 명령줄에 NUL 글자가 있다"));
        }
        v.push(0);
        Ok(v)
    }

    /// (자식 쪽 끝, 우리 쪽 끝). 자식 쪽만 상속 가능하다. `child_reads` 면 자식이 읽는 파이프(표준 입력)
    fn pipe(child_reads: bool) -> PortResult<(OwnedHandle, OwnedHandle)> {
        let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: null_mut(), bInheritHandle: 1 };
        let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
        // SAFETY: 출력 인자는 지역 변수이고 성공했을 때만 쓴다
        if unsafe { CreatePipe(&mut read, &mut write, &sa, 1 << 16) } == 0 {
            return Err(os_error("파이프를 만들지 못했다"));
        }
        // SAFETY: CreatePipe 가 방금 만든 소유 핸들이다
        let (read, write) = unsafe { (OwnedHandle::from_raw_handle(read), OwnedHandle::from_raw_handle(write)) };
        let (child, ours) = if child_reads { (read, write) } else { (write, read) };
        // SAFETY: ours 는 살아 있는 핸들이다
        if unsafe { SetHandleInformation(ours.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(os_error("파이프 끝을 상속 불가로 바꾸지 못했다"));
        }
        Ok((child, ours))
    }

    fn make_inheritable(handle: &impl AsRawHandle) -> PortResult<()> {
        // SAFETY: 살아 있는 핸들이다
        if unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(os_error("핸들을 상속 가능으로 바꾸지 못했다"));
        }
        Ok(())
    }

    fn create_job() -> PortResult<OwnedHandle> {
        // SAFETY: 인자 없는 익명 Job 을 만든다
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        if raw.is_null() {
            return Err(os_error("Job Object 를 만들지 못했다"));
        }
        // SAFETY: 방금 만든 소유 핸들이다
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };
        // SAFETY: 0 으로 채운 구조체는 모든 제한이 꺼진 유효한 값이다
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: info 는 지정한 크기의 유효한 구조체이다
        let ok = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(os_error("Job Object 에 닫으면 끝내기를 걸지 못했다"));
        }
        Ok(job)
    }

    /// 프로세스 속성 목록(핸들 상속 목록 하나). 떨어지면 지운다
    struct AttributeList {
        // 포인터 크기로 정렬되게 u64 로 잡는다
        buf: Vec<u64>,
        initialized: bool,
    }

    impl AttributeList {
        fn with_inherited(handles: &[HANDLE]) -> PortResult<Self> {
            let mut size = 0usize;
            // SAFETY: 크기만 묻는다(실패가 정상 — ERROR_INSUFFICIENT_BUFFER)
            unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size) };
            let mut list = AttributeList { buf: vec![0u64; size.div_ceil(8).max(1)], initialized: false };
            // SAFETY: buf 는 요구한 크기 이상이고 정렬되어 있다
            if unsafe { InitializeProcThreadAttributeList(list.ptr(), 1, 0, &mut size) } == 0 {
                return Err(os_error("프로세스 속성 목록을 만들지 못했다"));
            }
            list.initialized = true;
            // SAFETY: 초기화된 목록에 핸들 배열을 가리킨다. 배열은 CreateProcessW 가 끝날 때까지 호출자가 쥔다
            let ok = unsafe {
                UpdateProcThreadAttribute(
                    list.ptr(),
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handles.as_ptr().cast::<c_void>(),
                    std::mem::size_of_val(handles),
                    null_mut(),
                    null(),
                )
            };
            if ok == 0 {
                return Err(os_error("상속할 핸들 목록을 걸지 못했다"));
            }
            Ok(list)
        }

        fn ptr(&mut self) -> *mut c_void {
            self.buf.as_mut_ptr().cast::<c_void>()
        }
    }

    impl Drop for AttributeList {
        fn drop(&mut self) {
            if self.initialized {
                // SAFETY: 초기화에 성공한 목록이다
                unsafe { DeleteProcThreadAttributeList(self.ptr()) };
            }
        }
    }

    pub fn spawn(spec: &SpawnSpec) -> PortResult<(Inner, Option<File>)> {
        let env = env_block(&build_env(&current_env(), &spec.env))?;
        let mut command = wide(OsStr::new(&command_line(spec)?))?;
        let program = wide(strip_verbatim(&spec.program).as_os_str())?;
        let cwd = wide(strip_verbatim(&spec.cwd).as_os_str())?;

        // 표준 입출력: (자식 stdin, 자식 stdout, 자식 stderr, 우리 stdin, 우리 stdout)
        let (child_in, child_out, child_err, our_in, our_out): (OwnedHandle, OwnedHandle, OwnedHandle, Option<File>, Option<File>) =
            match &spec.io {
                Io::Streaming { stderr_log } => {
                    let (cin, oin) = pipe(true)?;
                    let (cout, oout) = pipe(false)?;
                    let (cerr, oerr) = pipe(false)?;
                    let mut log = OpenOptions::new().create(true).append(true).open(stderr_log)?;
                    let mut err_reader = File::from(oerr);
                    std::thread::Builder::new().name("winproc-stderr".into()).spawn(move || {
                        let _ = std::io::copy(&mut err_reader, &mut log);
                    })?;
                    (cin, cout, cerr, Some(File::from(oin)), Some(File::from(oout)))
                }
                Io::MergedToFile { log } => {
                    let nul = File::open("NUL")?;
                    let out = OpenOptions::new().create(true).write(true).truncate(true).open(log)?;
                    let err = out.try_clone()?;
                    make_inheritable(&nul)?;
                    make_inheritable(&out)?;
                    make_inheritable(&err)?;
                    (nul.into(), out.into(), err.into(), None, None)
                }
            };

        let mut inherit: Vec<HANDLE> = Vec::new();
        for h in [&child_in, &child_out, &child_err] {
            if !inherit.contains(&h.as_raw_handle()) {
                inherit.push(h.as_raw_handle());
            }
        }
        let mut attributes = AttributeList::with_inherited(&inherit)?;

        // SAFETY: 0 으로 채운 시작 정보는 유효하고, 아래에서 쓰는 칸을 채운다
        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = child_in.as_raw_handle();
        startup.StartupInfo.hStdOutput = child_out.as_raw_handle();
        startup.StartupInfo.hStdError = child_err.as_raw_handle();
        startup.lpAttributeList = attributes.ptr();

        let job = create_job()?;
        // SAFETY: 0 으로 채운 값은 출력 자리로 유효하다
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: 모든 포인터는 이 호출 동안 살아 있는 버퍼를 가리키고, 문자열은 NUL 로 끝난다
        let created = unsafe {
            CreateProcessW(
                program.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                env.as_ptr().cast::<c_void>(),
                cwd.as_ptr(),
                (&raw const startup).cast(),
                &mut info,
            )
        };
        if created == 0 {
            return Err(os_error(&format!("프로세스를 띄우지 못했다({})", spec.program.display())));
        }
        // SAFETY: CreateProcessW 가 돌려준 소유 핸들이다
        let (process, thread) = unsafe { (OwnedHandle::from_raw_handle(info.hProcess), OwnedHandle::from_raw_handle(info.hThread)) };
        // 자식 쪽 끝은 자식이 쥐었다 — 우리 것을 닫아야 자식이 끝났을 때 읽기가 EOF 를 본다
        drop((child_in, child_out, child_err, attributes));

        // SAFETY: 살아 있는 두 핸들이다
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
            let err = os_error("프로세스를 Job 에 넣지 못했다");
            // SAFETY: 아직 깨우지 않은 프로세스를 끝낸다
            unsafe { TerminateProcess(process.as_raw_handle(), 1) };
            return Err(err);
        }
        // SAFETY: 막 만든 일시 정지 스레드이다
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            let err = os_error("프로세스를 깨우지 못했다");
            // SAFETY: Job 째로 끝낸다
            unsafe { TerminateJobObject(job.as_raw_handle(), 1) };
            return Err(err);
        }
        drop(thread);

        Ok((Inner { pid: info.dwProcessId, process, job, stdin: our_in, exit: None }, our_out))
    }

    impl Inner {
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// 표준 입력을 쥘 몫을 넘긴다(쓰기 스레드가 쥔다). 처음 한 번만 Some
        pub fn take_stdin(&mut self) -> Option<File> {
            self.stdin.take()
        }

        pub fn wait(&mut self, timeout: Duration) -> PortResult<Option<i32>> {
            if self.exit.is_some() {
                return Ok(self.exit);
            }
            let ms = timeout.as_millis().min(0xFFFF_FFFE) as u32;
            // SAFETY: 살아 있는 프로세스 핸들이다
            let waited = unsafe { WaitForSingleObject(self.process.as_raw_handle(), ms) };
            if waited == WAIT_TIMEOUT {
                return Ok(None);
            }
            if waited != WAIT_OBJECT_0 {
                return Err(os_error("프로세스를 기다리지 못했다"));
            }
            let mut code = 0u32;
            // SAFETY: 끝난 프로세스의 종료 코드를 지역 변수로 받는다
            if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                return Err(os_error("종료 코드를 읽지 못했다"));
            }
            self.exit = Some(code as i32);
            Ok(self.exit)
        }

        pub fn kill_tree(&mut self) -> PortResult<()> {
            // 패키지 앱(Store 의 PowerShell 7)이 띄운 자식은 Job 밖으로 빠져나간다(실측). 루트가 아직 살아 있을 때
            // 프로세스 표에서 후손을 미리 모아 두었다가 Job 을 끝낸 뒤 남은 것을 직접 끝낸다
            let root_alive = matches!(self.wait(Duration::ZERO), Ok(None));
            let escaped = if root_alive { descendants_since(&claude_board_core::proc::ProcessTable::capture(), self.pid, &claude_board_core::proc::creation_time) } else { Vec::new() };
            // SAFETY: 살아 있는 Job 핸들이다
            let job_ended = unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } != 0;
            let job_error = (!job_ended).then(|| os_error("Job 을 끝내지 못했다"));
            for pid in escaped {
                claude_board_core::proc::terminate(pid);
            }
            job_error.map_or(Ok(()), Err)
        }
    }

    pub fn decode_oem(bytes: &[u8]) -> String {
        if bytes.is_empty() {
            return String::new();
        }
        let len = bytes.len() as i32;
        // SAFETY: 크기만 묻는다
        let needed = unsafe { MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), len, null_mut(), 0) };
        if needed <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; needed as usize];
        // SAFETY: wide 는 요구한 크기이다
        let written = unsafe { MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), needed) };
        String::from_utf16_lossy(&wide[..written.max(0) as usize])
    }
}

#[cfg(not(windows))]
mod sys {
    use super::*;
    use std::fs::File;

    pub struct Inner;

    pub fn spawn(_spec: &SpawnSpec) -> PortResult<(Inner, Option<File>)> {
        Err(PortError::new(PortErrorKind::Unavailable, "Windows 에서만 프로세스를 띄울 수 있다"))
    }

    pub fn decode_oem(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    impl Inner {
        pub fn pid(&self) -> u32 {
            0
        }
        pub fn take_stdin(&mut self) -> Option<File> {
            None
        }
        pub fn wait(&mut self, _timeout: Duration) -> PortResult<Option<i32>> {
            Err(PortError::new(PortErrorKind::Unavailable, "Windows 에서만 쓸 수 있다"))
        }
        pub fn kill_tree(&mut self) -> PortResult<()> {
            Err(PortError::new(PortErrorKind::Unavailable, "Windows 에서만 쓸 수 있다"))
        }
    }
}
