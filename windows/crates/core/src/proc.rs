//! 프로세스 도구 — Claude 프로세스 찾기·살아 있는지·실행 인자·자식 프로세스 (SPEC §2.1, §3.4, §5.1).
//!
//! Windows 는 Toolhelp 스냅숏·`OpenProcess`·`NtQueryInformationProcess` 를 쓴다.
//! 그 밖(macOS 개발 기기)은 `state.py` 와 같이 `ps`·`kill(pid, 0)`·`sysctl(KERN_PROCARGS2)` 를 쓴다.

use std::collections::HashMap;

use serde_json::Value;

use crate::place::Host;
use crate::state::ClaudeProcess;
use crate::Record;

/// FILETIME(1601년부터 100ns) 과 유닉스 시각 사이의 차이.
const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;

/// FILETIME 값을 유닉스 초로 바꾼다.
pub fn filetime_to_unix(ticks: u64) -> f64 {
    (ticks as f64 - FILETIME_UNIX_DIFF as f64) / 1e7
}

/// 유닉스 초를 FILETIME 값으로 바꾼다(1601년 이전이면 None).
pub fn unix_to_filetime(ts: f64) -> Option<u64> {
    let ticks = ts * 1e7 + FILETIME_UNIX_DIFF as f64;
    (ticks.is_finite() && ticks >= 0.0).then_some(ticks as u64)
}

#[derive(Debug, Clone)]
struct Entry {
    ppid: u32,
    name: String,
    // macOS 만 쓴다(Windows 에는 tty 가 없다)
    #[cfg_attr(windows, allow(dead_code))]
    tty: Option<String>,
}

/// 한 시점의 프로세스 표(pid → 부모 pid, 실행 파일 이름).
#[derive(Debug, Clone, Default)]
pub struct ProcessTable {
    procs: HashMap<u32, Entry>,
}

impl ProcessTable {
    /// 지금 프로세스 표를 만든다(Windows `CreateToolhelp32Snapshot` 한 번, 그 밖 `ps` 한 번). 실패하면 빈 표.
    pub fn capture() -> Self {
        sys::capture()
    }

    /// (pid, 부모 pid, 실행 파일 이름) 목록으로 만든다 — 시험용.
    pub fn from_entries<I: IntoIterator<Item = (u32, u32, String)>>(entries: I) -> Self {
        let procs = entries.into_iter().map(|(pid, ppid, name)| (pid, Entry { ppid, name, tty: None })).collect();
        ProcessTable { procs }
    }

    pub fn parent(&self, pid: u32) -> Option<u32> {
        self.procs.get(&pid).map(|e| e.ppid)
    }

    /// 실행 파일 이름(Windows `claude.exe`, 그 밖 `ps` comm 의 마지막 이름).
    pub fn name(&self, pid: u32) -> Option<&str> {
        self.procs.get(&pid).map(|e| e.name.as_str())
    }

    /// 부모부터 차례로 최대 `max` 개의 조상 pid. 표에 없거나 0 이거나 되돌아오면 멈춘다.
    pub fn ancestors(&self, pid: u32, max: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = pid;
        while out.len() < max {
            match self.parent(cur) {
                Some(p) if p != 0 && p != cur && !out.contains(&p) && p != pid => {
                    out.push(p);
                    cur = p;
                }
                _ => break,
            }
        }
        out
    }

    pub fn children(&self, pid: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.procs.iter().filter(|(&c, e)| e.ppid == pid && c != pid).map(|(&c, _)| c).collect();
        out.sort_unstable();
        out
    }

    /// 실행 파일 이름이 `name` 인 프로세스들(대소문자 무시) — 예: 실행 중인 `ClaudeBoard.exe`.
    pub fn pids_named(&self, name: &str) -> Vec<u32> {
        let mut out: Vec<u32> =
            self.procs.iter().filter(|(_, e)| e.name.eq_ignore_ascii_case(name)).map(|(&p, _)| p).collect();
        out.sort_unstable();
        out
    }

    /// `start` 부터 위로 최대 `max` 개 프로세스를 보며 `is_claude(pid, 이름)` 이 참인 첫 pid.
    fn find_up(&self, start: u32, max: usize, is_claude: impl Fn(u32, &str) -> bool) -> Option<u32> {
        let mut pid = start;
        for _ in 0..max {
            let entry = self.procs.get(&pid)?;
            if is_claude(pid, &entry.name) {
                return Some(pid);
            }
            if entry.ppid == 0 || entry.ppid == pid {
                return None;
            }
            pid = entry.ppid;
        }
        None
    }
}

const CLAUDE_PACKAGE: &str = "@anthropic-ai/claude-code";

/// Windows 의 Claude Code 프로세스 판별 — `claude.exe`, 또는 명령줄에 Claude Code 패키지(`@anthropic-ai/claude-code`)가 든 `node.exe`·`bun.exe`.
/// 폴더 이름에 `claude` 가 든 것만으로는 세지 않는다(예: `claude-board` 아래의 node 도구).
pub fn is_claude_exe(name: &str, cmdline: impl FnOnce() -> Option<Vec<String>>) -> bool {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "claude.exe" => true,
        "node.exe" | "bun.exe" => cmdline().is_some_and(|a| a.iter().any(|s| s.to_lowercase().replace('\\', "/").contains(CLAUDE_PACKAGE))),
        _ => false,
    }
}

/// hook 프로세스의 조상 가운데 Claude Code 프로세스를 찾는다.
/// Windows: 부모부터 최대 8단계, pid·생성 시각·headless(§3.4)·실행 인자. 그 밖: `state.py` 처럼 6단계, tty 로 headless.
pub fn find_claude() -> Option<ClaudeProcess> {
    let table = ProcessTable::capture();
    #[cfg(windows)]
    {
        let start = table.parent(sys::current_pid())?;
        let pid = table.find_up(start, 8, |pid, name| is_claude_exe(name, || command_line(pid)))?;
        let argv = command_line(pid).unwrap_or_default();
        let headless = crate::console::is_headless(pid, &argv);
        Some(ClaudeProcess { pid, ctime: creation_time(pid), tty: None, headless, argv })
    }
    #[cfg(not(windows))]
    {
        let start = sys::parent_pid();
        let pid = table.find_up(start, 6, |_, name| name == "claude")?;
        let tty = table.procs.get(&pid).and_then(|e| e.tty.clone());
        Some(ClaudeProcess {
            pid,
            ctime: None,
            headless: tty.is_none(),
            tty,
            argv: command_line(pid).unwrap_or_default(),
        })
    }
}

/// 그 pid 가 살아 있는지. `ctime`(생성 시각)이 있으면 그것도 같아야 산 것으로 본다(Windows pid 재사용).
/// 열 권한이 없으면 산 것으로 본다.
pub fn alive(pid: u32, ctime: Option<f64>) -> bool {
    sys::alive(pid, ctime)
}

/// 세션 기록의 `pid`·`pid_ctime` 으로 본 생사. pid 가 없거나 양의 정수가 아니면 산 것으로 본다(`state.py` 의 `alive(None)`).
pub fn record_alive(rec: &Record) -> bool {
    match rec.get("pid").and_then(Value::as_u64) {
        Some(pid) if pid > 0 && pid <= u64::from(u32::MAX) => {
            alive(pid as u32, rec.get("pid_ctime").and_then(Value::as_f64))
        }
        _ => true,
    }
}

/// 프로세스 생성 시각(유닉스 초) — Windows 만. 그 밖은 None.
pub fn creation_time(pid: u32) -> Option<f64> {
    sys::creation_time(pid)
}

/// 다른 프로세스의 실행 인자 전체(argv[0] 포함). 읽지 못하면 None.
pub fn command_line(pid: u32) -> Option<Vec<String>> {
    sys::command_line(pid)
}

/// 그 프로세스가 쓰는 콘솔의 호스트 프로세스 — Windows 에 tty 가 없어 같은 터미널인지의 자료로 쓴다
/// (W2 설계 (a) 방안). 같은 ConPTY 의 클라이언트들은 같은 콘솔 호스트(conhost/OpenConsole)를 본다.
/// 콘솔이 없는 프로세스(분리로 뜬 `--bg`)는 `Host::Detached`, 읽지 못하면 `Host::Unknown` 이다.
pub fn console_host(pid: u32) -> Host {
    match sys::console_host(pid) {
        Some(0) => Host::Detached,
        Some(host_pid) => Host::Console(host_pid),
        None => Host::Unknown,
    }
}

/// 그 Claude 프로세스의 백그라운드 셸 수 — 자식 가운데 명령줄에 `shell-snapshots` 가 든 프로세스 (SPEC §5.1).
pub fn shell_count(table: &ProcessTable, pid: u32) -> usize {
    table
        .children(pid)
        .into_iter()
        .filter(|&c| command_line(c).is_some_and(|a| a.iter().any(|s| s.contains("shell-snapshots"))))
        .count()
}

/// 프로세스를 끝낸다(Windows `TerminateProcess`, 그 밖 SIGTERM). 대화 기록은 Claude Code 가 이미 남겨 두었다.
pub fn terminate(pid: u32) -> bool {
    sys::terminate(pid)
}

#[cfg(windows)]
mod sys {
    use super::{filetime_to_unix, Entry, ProcessTable};
    use std::collections::HashMap;
    use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_ACCESS_DENIED, FILETIME, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE,
        UNICODE_STRING, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, GetExitCodeProcess, GetProcessTimes, OpenProcess, TerminateProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

    /// ProcessCommandLineInformation — windows-sys 에 상수가 없어 직접 둔다.
    const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;

    /// ProcessConsoleHostProcess — windows-sys 에 상수가 없어 직접 둔다.
    const PROCESS_CONSOLE_HOST_PROCESS: i32 = 49;

    pub(crate) struct Handle(pub HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: 우리가 연 핸들을 한 번만 닫는다
            unsafe { CloseHandle(self.0) };
        }
    }

    pub(crate) fn open(pid: u32, access: u32) -> Result<Handle, u32> {
        // SAFETY: 실패하면 null 을 돌려줄 뿐이다
        let h = unsafe { OpenProcess(access, 0, pid) };
        if h.is_null() {
            Err(unsafe { GetLastError() })
        } else {
            Ok(Handle(h))
        }
    }

    pub fn current_pid() -> u32 {
        // SAFETY: 인자 없는 조회
        unsafe { GetCurrentProcessId() }
    }

    pub fn capture() -> ProcessTable {
        let mut procs = HashMap::new();
        // SAFETY: 스냅숏 핸들은 Handle 이 닫고, 구조체는 dwSize 를 채워 넘긴다
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return ProcessTable::default();
            }
            let snap = Handle(snap);
            let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap.0, &mut e) != 0;
            while ok {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                procs.insert(e.th32ProcessID, Entry { ppid: e.th32ParentProcessID, name, tty: None });
                ok = Process32NextW(snap.0, &mut e) != 0;
            }
        }
        ProcessTable { procs }
    }

    fn created(h: &Handle) -> Option<f64> {
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: 넘긴 구조체에만 쓴다
        let ok = unsafe { GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) } != 0;
        ok.then(|| filetime_to_unix((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime)))
    }

    pub fn alive(pid: u32, ctime: Option<f64>) -> bool {
        // 끝났는지는 대기 가능 여부로 본다 — 종료 코드 259(STILL_ACTIVE)로 끝난 프로세스를 산 것으로 잘못 보지 않게.
        // SYNCHRONIZE 를 못 받으면(다른 사용자·높은 권한) 조회 권한만으로 종료 코드를 본다
        let h = match open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE) {
            Ok(h) => h,
            Err(ERROR_ACCESS_DENIED) => match open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
                Ok(h) => h,
                Err(e) => return e == ERROR_ACCESS_DENIED,
            },
            Err(_) => return false,
        };
        // SAFETY: 연 핸들로 상태만 읽는다(0ms 대기는 막지 않는다)
        match unsafe { WaitForSingleObject(h.0, 0) } {
            WAIT_OBJECT_0 => return false,
            WAIT_TIMEOUT => {}
            _ => {
                let mut code = 0u32;
                // SAFETY: 연 핸들로 종료 코드만 읽는다
                if unsafe { GetExitCodeProcess(h.0, &mut code) } != 0 && code != STILL_ACTIVE as u32 {
                    return false;
                }
            }
        }
        match (ctime, created(&h)) {
            (Some(want), Some(got)) => (want - got).abs() < 0.001,
            _ => true,
        }
    }

    pub fn creation_time(pid: u32) -> Option<f64> {
        created(&open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?)
    }

    pub fn command_line(pid: u32) -> Option<Vec<String>> {
        let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
        // UNICODE_STRING 머리 + 문자열이 들어갈 버퍼(8바이트 정렬). 모자라면 알려 준 크기로 다시 부른다
        let mut buf: Vec<u64> = vec![0; 512];
        for _ in 0..3 {
            let cap = (buf.len() * 8) as u32;
            let mut need = 0u32;
            // SAFETY: buf 크기를 정확히 넘긴다
            let status = unsafe {
                NtQueryInformationProcess(h.0, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr().cast(), cap, &mut need)
            };
            if status >= 0 {
                // SAFETY: 성공하면 buf 앞에 UNICODE_STRING 이 있고 Buffer 는 buf 안을 가리킨다
                let units = unsafe {
                    let us = &*(buf.as_ptr() as *const UNICODE_STRING);
                    if us.Buffer.is_null() || us.Length == 0 {
                        return Some(Vec::new());
                    }
                    std::slice::from_raw_parts(us.Buffer, usize::from(us.Length) / 2)
                };
                return Some(split_command_line(units));
            }
            if need <= cap {
                return None;
            }
            buf = vec![0; (need as usize).div_ceil(8)];
        }
        None
    }

    /// 콘솔 호스트 프로세스 pid(class 49). 못 읽으면 None(콘솔이 없다면 0 을 준다).
    /// 하위 2 비트는 플래그라 가린다(System Informer 도 같이 본다).
    pub fn console_host(pid: u32) -> Option<u32> {
        let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
        let mut raw = 0usize;
        let mut need = 0u32;
        // SAFETY: 포인터 크기 버퍼(usize) 를 정확히 넘긴다
        let status = unsafe {
            NtQueryInformationProcess(
                h.0,
                PROCESS_CONSOLE_HOST_PROCESS,
                (&mut raw as *mut usize).cast(),
                size_of::<usize>() as u32,
                &mut need,
            )
        };
        (status >= 0).then_some((raw & !3) as u32)
    }

    /// `CommandLineToArgvW` 로 나눈다(빈 문자열이면 그 함수가 자기 경로를 돌려주므로 먼저 거른다).
    pub(crate) fn split_command_line(units: &[u16]) -> Vec<String> {
        if units.is_empty() {
            return Vec::new();
        }
        let mut z = units.to_vec();
        z.push(0);
        let mut n = 0i32;
        // SAFETY: NUL 로 끝나는 문자열을 넘기고, 돌려받은 배열은 LocalFree 로 놓는다
        unsafe {
            let argv = CommandLineToArgvW(z.as_ptr(), &mut n);
            if argv.is_null() {
                return Vec::new();
            }
            let mut out = Vec::with_capacity(n.max(0) as usize);
            for i in 0..n.max(0) as usize {
                let p = *argv.add(i);
                let mut len = 0;
                while *p.add(len) != 0 {
                    len += 1;
                }
                out.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
            }
            LocalFree(argv.cast());
            out
        }
    }

    pub fn terminate(pid: u32) -> bool {
        match open(pid, PROCESS_TERMINATE) {
            // SAFETY: 끝낼 권한으로 연 핸들
            Ok(h) => unsafe { TerminateProcess(h.0, 1) != 0 },
            Err(_) => false,
        }
    }
}

#[cfg(unix)]
mod sys {
    use super::{Entry, ProcessTable};
    use std::collections::HashMap;
    use std::process::{Command, Stdio};

    pub fn parent_pid() -> u32 {
        // SAFETY: 인자 없는 조회
        unsafe { libc::getppid() as u32 }
    }

    /// 앞쪽 공백을 건너뛰고 첫 낱말과 나머지로 나눈다.
    fn token(s: &str) -> (&str, &str) {
        let s = s.trim_start();
        match s.find(char::is_whitespace) {
            Some(i) => (&s[..i], &s[i..]),
            None => (s, ""),
        }
    }

    /// `ps -axo pid=,ppid=,tty=,comm=` 출력을 표로 — comm 은 공백이 들 수 있어 마지막에 둔다.
    pub(super) fn parse_ps(text: &str) -> ProcessTable {
        let mut procs = HashMap::new();
        for line in text.lines() {
            let (pid, rest) = token(line);
            let (ppid, rest) = token(rest);
            let (tty, rest) = token(rest);
            let comm = rest.trim();
            let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) else { continue };
            // state.py: basename(comm) — 로그인 셸의 "-/bin/zsh" 도 마지막 이름만 본다
            let name = comm.rsplit('/').next().unwrap_or(comm).to_string();
            let tty = (!tty.is_empty() && !tty.starts_with('?')).then(|| tty.to_string());
            procs.insert(pid, Entry { ppid, name, tty });
        }
        ProcessTable { procs }
    }

    pub fn capture() -> ProcessTable {
        let out = Command::new("ps")
            .args(["-axo", "pid=,ppid=,tty=,comm="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => parse_ps(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => ProcessTable::default(),
        }
    }

    pub fn alive(pid: u32, _ctime: Option<f64>) -> bool {
        let Ok(pid) = i32::try_from(pid) else { return false };
        // SAFETY: 신호 0 은 존재 여부만 확인한다
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    pub fn creation_time(_pid: u32) -> Option<f64> {
        None
    }

    pub fn console_host(_pid: u32) -> Option<u32> {
        None
    }

    /// macOS: sysctl KERN_PROCARGS2 로 실행 인자를 공백 손실 없이 읽는다(`state.py:proc_args`).
    #[cfg(target_os = "macos")]
    pub fn command_line(pid: u32) -> Option<Vec<String>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, i32::try_from(pid).ok()?];
        let mut size: libc::size_t = 0;
        // SAFETY: 먼저 크기만 묻고, 그 크기의 버퍼에 받는다
        unsafe {
            if libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0 {
                return None;
            }
        }
        let mut buf = vec![0u8; size];
        unsafe {
            if libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) != 0 {
                return None;
            }
        }
        buf.truncate(size);
        let argc = usize::try_from(i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?)).ok()?;
        // 실행 파일 경로와 뒤따르는 NUL 들을 건너뛴다
        let rest = &buf[4..];
        let rest = &rest[rest.iter().position(|&b| b == 0)? + 1..];
        let rest = &rest[rest.iter().position(|&b| b != 0).unwrap_or(rest.len())..];
        Some(rest.split(|&b| b == 0).take(argc).map(|a| String::from_utf8_lossy(a).into_owned()).collect())
    }

    #[cfg(target_os = "linux")]
    pub fn command_line(pid: u32) -> Option<Vec<String>> {
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let raw = raw.strip_suffix(&[0]).unwrap_or(&raw);
        Some(raw.split(|&b| b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn command_line(_pid: u32) -> Option<Vec<String>> {
        None
    }

    pub fn terminate(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else { return false };
        // SAFETY: 그 pid 에 SIGTERM 만 보낸다
        unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    use super::ProcessTable;
    pub fn capture() -> ProcessTable {
        ProcessTable::default()
    }
    pub fn parent_pid() -> u32 {
        0
    }
    pub fn alive(_pid: u32, _ctime: Option<f64>) -> bool {
        true
    }
    pub fn creation_time(_pid: u32) -> Option<f64> {
        None
    }
    pub fn console_host(_pid: u32) -> Option<u32> {
        None
    }
    pub fn command_line(_pid: u32) -> Option<Vec<String>> {
        None
    }
    pub fn terminate(_pid: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> ProcessTable {
        ProcessTable::from_entries([
            (1, 0, "System".into()),
            (10, 1, "explorer.exe".into()),
            (20, 10, "WindowsTerminal.exe".into()),
            (30, 20, "pwsh.exe".into()),
            (40, 30, "claude.exe".into()),
            (50, 40, "claude-board-hook.exe".into()),
            (60, 40, "bash.exe".into()),
            (61, 40, "bash.exe".into()),
            (70, 30, "node.exe".into()),
        ])
    }

    #[test]
    fn walks_ancestors_and_children() {
        let t = table();
        assert_eq!(t.ancestors(50, 8), [40, 30, 20, 10, 1]);
        assert_eq!(t.ancestors(50, 2), [40, 30]);
        assert_eq!(t.children(40), [50, 60, 61]);
        assert_eq!(t.pids_named("CLAUDE.EXE"), [40]);
        assert_eq!(t.find_up(40, 8, |_, n| n == "claude.exe"), Some(40));
        assert_eq!(t.find_up(30, 8, |_, n| n == "claude.exe"), None);
        // 되돌아오는 부모 고리에서 멈춘다
        let loopy = ProcessTable::from_entries([(5, 6, "a".into()), (6, 5, "b".into())]);
        assert_eq!(loopy.ancestors(5, 8), [6]);
    }

    #[test]
    fn claude_exe_rules() {
        assert!(is_claude_exe("Claude.EXE", || None));
        let cli = || Some(vec!["node.exe".to_string(), r"C:\npm\node_modules\@anthropic-ai\claude-code\cli.js".to_string()]);
        assert!(is_claude_exe("node.exe", cli));
        assert!(!is_claude_exe("node.exe", || Some(vec!["node".into(), "server.js".into()])));
        assert!(!is_claude_exe("bun.exe", || None));
        assert!(!is_claude_exe("claude", || None));
        // 폴더 이름에만 claude 가 든 node 도구는 아니다
        let board = || Some(vec!["node.exe".to_string(), r"C:\Users\me\work\claude-board\tools\build.js".to_string()]);
        assert!(!is_claude_exe("node.exe", board));
        assert!(!is_claude_exe("bun.exe", || Some(vec!["bun.exe".into(), "/home/me/claude/app.ts".into()])));
        // 실제 npm 전역 설치 모양
        let npm = || {
            Some(vec![
                r"C:\Program Files\nodejs\node.exe".to_string(),
                r"C:\Users\me\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ])
        };
        assert!(is_claude_exe("node.exe", npm));
    }

    #[test]
    fn filetime_round_trip() {
        assert_eq!(filetime_to_unix(FILETIME_UNIX_DIFF), 0.0);
        let ts = 1_792_146_248.374_68;
        let back = filetime_to_unix(unix_to_filetime(ts).unwrap());
        assert!((back - ts).abs() < 1e-6);
        assert_eq!(unix_to_filetime(-1e12), None);
    }

    #[test]
    fn record_alive_rules() {
        let rec = |v: serde_json::Value| v.as_object().unwrap().clone();
        assert!(record_alive(&rec(serde_json::json!({}))));
        assert!(record_alive(&rec(serde_json::json!({"pid": null}))));
        assert!(record_alive(&rec(serde_json::json!({"pid": 0}))));
        assert!(record_alive(&rec(serde_json::json!({"pid": std::process::id()}))));
    }

    #[test]
    fn console_host_of_unknown_pids() {
        // pid 0 · 존재하지 않는 pid — 어느 플랫폼에서나 모름으로 본다
        assert_eq!(console_host(0), Host::Unknown);
        assert_eq!(console_host(u32::MAX), Host::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn unix_ps_parsing_and_self() {
        let t = sys::parse_ps("  46137  3957 ttys000  claude\n89111 46137 ??       /bin/zsh\n 3957  3954 ttys000  -/bin/zsh\n 1 0 ?? /Applications/Google Chrome.app/x y\nbad line\n");
        assert_eq!(t.name(46137), Some("claude"));
        assert_eq!(t.procs[&46137].tty.as_deref(), Some("ttys000"));
        assert_eq!(t.procs[&89111].tty, None);
        assert_eq!(t.name(3957), Some("zsh"));
        assert_eq!(t.name(1), Some("x y"));
        assert_eq!(t.find_up(89111, 6, |_, n| n == "claude"), Some(46137));
        // 이 시험 프로세스는 살아 있고, 자기 실행 인자를 읽을 수 있다
        let me = std::process::id();
        assert!(alive(me, None));
        assert!(!command_line(me).unwrap_or_default().is_empty());
        assert!(ProcessTable::capture().name(me).is_some());
    }
}
