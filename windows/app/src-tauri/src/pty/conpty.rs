//! ConPTY 래퍼 — 가짜 콘솔을 만들고 그 안에 셸 프로세스를 띄운다.
//!
//! `CreatePseudoConsole`·`ResizePseudoConsole`·`ClosePseudoConsole` 은 [`ConptyApi`] 함수 표로 부른다.
//! 지금 표는 시스템(kernel32)의 것 하나뿐이고, 따로 싣는 conpty.dll 을 쓰게 되면 표만 갈아 끼운다.
//! 파이프 끝은 모두 상속 불가(널 보안 속성)로 만들고 `CreateProcessW` 도 핸들을 상속시키지 않는다 —
//! 보드가 PowerShell 을 띄울 때 핸들 상속을 쓰므로(updater) 우리 파이프가 거기 새어 들어가면 안 된다.

use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
#[cfg(test)]
use std::sync::Arc;

use claude_board_core::quote;
use windows_sys::core::HRESULT;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF, FALSE, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// 가짜 콘솔을 다루는 세 함수. 시스템 것과 따로 싣는 conpty.dll 의 것이 같은 모양이라 표로 갈아 끼운다.
pub struct ConptyApi {
    pub create: unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> HRESULT,
    pub resize: unsafe extern "system" fn(HPCON, COORD) -> HRESULT,
    pub close: unsafe extern "system" fn(HPCON),
}

/// 시스템(kernel32)의 ConPTY. 플래그는 0 으로 쓴다.
pub static SYSTEM_API: ConptyApi =
    ConptyApi { create: CreatePseudoConsole, resize: ResizePseudoConsole, close: ClosePseudoConsole };

/// 닫기를 잊지 않게 하는 핸들 상자(0 = 비어 있음).
struct Owned(isize);

impl Owned {
    fn raw(&self) -> HANDLE {
        self.0 as HANDLE
    }

    /// 소유를 넘긴다 — 이 상자는 더 닫지 않는다.
    fn into_raw(mut self) -> isize {
        std::mem::take(&mut self.0)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: 이 상자가 소유한 핸들을 한 번 닫는다
            unsafe { CloseHandle(self.0 as HANDLE) };
        }
    }
}

fn pipe() -> io::Result<(Owned, Owned)> {
    let (mut r, mut w): (HANDLE, HANDLE) = (null_mut(), null_mut());
    // SAFETY: 널 보안 속성 = 상속 불가 핸들. 돌려받은 둘은 바로 상자에 담는다
    if unsafe { CreatePipe(&mut r, &mut w, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((Owned(r as isize), Owned(w as isize)))
}

fn hresult(hr: HRESULT) -> io::Result<()> {
    if hr < 0 {
        Err(io::Error::from_raw_os_error(hr))
    } else {
        Ok(())
    }
}

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

fn coord(cols: u16, rows: u16) -> COORD {
    COORD { X: cols.min(i16::MAX as u16) as i16, Y: rows.min(i16::MAX as u16) as i16 }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 가짜 콘솔 하나와 그 안의 프로세스 하나. 여러 스레드가 함께 쓴다 —
/// 읽기 스레드는 `read`·`close_output`, 웨이터는 `wait_exit`·`release`, 그 밖은 입력·크기·종료.
pub struct ConPty {
    api: &'static ConptyApi,
    hpc: HPCON,
    /// 가짜 콘솔이 열려 있나. 크기 바꾸기와 닫기가 같은 뮤텍스를 쥐고 부르므로 닫힌(해제된) 핸들에 resize 가 닿지 않는다
    console_open: Mutex<bool>,
    /// 입력 파이프 쓰기 끝(0 = 닫힘). 쓰기를 한 줄로 세우려고 뮤텍스에 둔다
    input: Mutex<isize>,
    /// 출력 파이프 읽기 끝(0 = 닫힘). 읽기 스레드만 읽고 닫는다
    output: AtomicIsize,
    /// 프로세스 핸들(0 = 닫힘). 웨이터만 닫는다
    process: Mutex<isize>,
    pid: u32,
}

// SAFETY: 핸들은 정수로 들고 있고 접근은 뮤텍스·원자 값을 거친다. 가짜 콘솔 핸들은 스레드에 묶이지 않는다
unsafe impl Send for ConPty {}
// SAFETY: 위와 같다
unsafe impl Sync for ConPty {}

impl ConPty {
    /// `program` 을 `args` 와 함께 `cwd` 에서 `cols`×`rows` 가짜 콘솔에 띄운다. 환경은 사용자 계정 기준으로 새로 만든다.
    pub fn spawn(program: &Path, args: &[String], cwd: &Path, cols: u16, rows: u16) -> io::Result<ConPty> {
        Self::spawn_with(&SYSTEM_API, program, args, cwd, cols, rows)
    }

    pub fn spawn_with(
        api: &'static ConptyApi,
        program: &Path,
        args: &[String],
        cwd: &Path,
        cols: u16,
        rows: u16,
    ) -> io::Result<ConPty> {
        let env = super::env::new_block()?;
        let (in_read, in_write) = pipe()?;
        let (out_read, out_write) = pipe()?;
        let mut hpc: HPCON = 0;
        // SAFETY: 파이프 끝 둘은 이 함수가 만든 유효한 핸들. 플래그 0
        hresult(unsafe { (api.create)(coord(cols, rows), in_read.raw(), out_write.raw(), 0, &mut hpc) })?;
        // 가짜 콘솔이 자기 몫을 복제해 갔으므로 우리 쪽 끝은 닫는다(안 그러면 EOF 가 안 온다)
        drop((in_read, out_write));
        let mut pty = ConPty {
            api,
            hpc,
            console_open: Mutex::new(true),
            input: Mutex::new(in_write.into_raw()),
            output: AtomicIsize::new(out_read.into_raw()),
            process: Mutex::new(0),
            pid: 0,
        };
        let (process, pid) = create_process(hpc, program, args, cwd, &env)?;
        *lock(&pty.process) = process;
        pty.pid = pid;
        Ok(pty)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 출력을 읽는다. 0 바이트 = 끝(가짜 콘솔이 닫혀 파이프가 끊겼다).
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let h = self.output.load(Ordering::Acquire) as HANDLE;
        if h.is_null() {
            return Ok(0);
        }
        let mut n = 0u32;
        // SAFETY: buf 는 길이만큼 쓸 수 있고, 핸들은 읽기 스레드만 닫는다
        let ok = unsafe { ReadFile(h, buf.as_mut_ptr(), buf.len().min(u32::MAX as usize) as u32, &mut n, null_mut()) };
        if ok != 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error().map(|c| c as u32) {
            Some(ERROR_BROKEN_PIPE | ERROR_HANDLE_EOF) => Ok(0),
            _ => Err(e),
        }
    }

    /// 읽기 스레드가 EOF 를 만난 뒤 읽기 끝을 닫는다.
    pub fn close_output(&self) {
        let h = self.output.swap(0, Ordering::AcqRel);
        if h != 0 {
            // SAFETY: swap 으로 소유를 가져온 핸들
            unsafe { CloseHandle(h as HANDLE) };
        }
    }

    /// 입력 파이프에 통째로 쓴다(셸이 읽을 때까지 파이프가 차면 기다린다).
    pub fn write_all(&self, mut data: &[u8]) -> io::Result<()> {
        let g = lock(&self.input);
        if *g == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "입력 파이프가 닫혔다"));
        }
        while !data.is_empty() {
            let mut n = 0u32;
            // SAFETY: data 는 길이만큼 읽을 수 있고, 핸들은 같은 뮤텍스 아래서만 닫힌다
            let ok = unsafe { WriteFile(*g as HANDLE, data.as_ptr(), data.len().min(u32::MAX as usize) as u32, &mut n, null_mut()) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            data = &data[n as usize..];
        }
        Ok(())
    }

    /// 크기를 바꿨으면 `Ok(true)`. 가짜 콘솔이 이미 닫혔으면 아무 일도 하지 않고 `Ok(false)` 를 돌려준다(닫힌 핸들은 해제된 메모리다).
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<bool> {
        let open = lock(&self.console_open);
        if !*open {
            return Ok(false);
        }
        // SAFETY: 뮤텍스를 쥔 동안은 close_console 이 핸들을 해제하지 못한다
        hresult(unsafe { (self.api.resize)(self.hpc, coord(cols, rows)) })?;
        Ok(true)
    }

    /// 가짜 콘솔을 닫는다(한 번만). 붙어 있던 프로세스에는 콘솔이 닫혔다고 알려진다. 오래 걸릴 수 있어
    /// 출력을 읽는 스레드가 따로 살아 있는 동안 별도 스레드에서 부른다.
    pub fn close_console(&self) {
        let mut open = lock(&self.console_open);
        if *open {
            *open = false;
            // SAFETY: 뮤텍스 아래서 한 번만 닫고, 이후 resize 는 닫힘을 보고 돌아간다
            unsafe { (self.api.close)(self.hpc) };
        }
    }

    /// 프로세스가 끝날 때까지 기다려 종료 코드를 돌려준다. 웨이터 스레드 하나만 부른다.
    pub fn wait_exit(&self) -> u32 {
        let h = *lock(&self.process) as HANDLE;
        if h.is_null() {
            return 0;
        }
        let mut code = 0u32;
        // SAFETY: 프로세스 핸들은 웨이터(이 호출자)만 닫는다
        unsafe {
            WaitForSingleObject(h, INFINITE);
            GetExitCodeProcess(h, &mut code);
        }
        code
    }

    /// 프로세스가 끝난 뒤 프로세스·입력 핸들을 닫는다. 웨이터가 부른다.
    pub fn release(&self) {
        for slot in [&self.process, &self.input] {
            let mut g = lock(slot);
            if *g != 0 {
                // SAFETY: 뮤텍스 아래서 한 번만 닫고 0 으로 표시한다
                unsafe { CloseHandle(*g as HANDLE) };
                *g = 0;
            }
        }
    }

    /// 프로세스를 바로 끝낸다(이미 끝났으면 아무 일도 없다).
    pub fn kill(&self) {
        let g = lock(&self.process);
        if *g != 0 {
            // SAFETY: 뮤텍스 아래라 웨이터가 닫지 못한다
            unsafe { TerminateProcess(*g as HANDLE, 1) };
        }
    }

    /// 시험용: 입력 쓰기 끝·출력 읽기 끝의 원시 핸들.
    #[cfg(test)]
    pub fn raw_pipe_handles(&self) -> (isize, isize) {
        (*lock(&self.input), self.output.load(Ordering::Acquire))
    }
}

impl Drop for ConPty {
    fn drop(&mut self) {
        self.close_console();
        self.close_output();
        self.release();
    }
}

/// 가짜 콘솔에 붙인 프로세스를 만든다. 돌려주는 것은 프로세스 핸들과 pid(스레드 핸들은 닫는다).
fn create_process(hpc: HPCON, program: &Path, args: &[String], cwd: &Path, env: &[u16]) -> io::Result<(isize, u32)> {
    let app = wide(program);
    let mut line = wide(
        std::iter::once(quote::windows(&program.to_string_lossy()))
            .chain(args.iter().map(|a| quote::windows(a)))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let dir = wide(cwd);

    let mut size = 0usize;
    // SAFETY: 크기만 묻는 호출 — 목록 포인터는 널. 일부러 실패(ERROR_INSUFFICIENT_BUFFER)하고 size 를 채운다
    unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    // usize 단위로 잡아 정렬을 맞춘다
    let mut list_buf = vec![0usize; size.div_ceil(size_of::<usize>())];
    let list = list_buf.as_mut_ptr() as *mut c_void;
    // SAFETY: list_buf 는 요구 크기 이상이고 이 함수 끝(DeleteProcThreadAttributeList)까지 산다
    if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: PSEUDOCONSOLE 속성의 값은 HPCON 자체(값 복사)이고 크기는 그 크기
    let set = unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            hpc as *const c_void,
            size_of::<HPCON>(),
            null_mut(),
            null(),
        )
    };
    let result = if set == 0 {
        Err(io::Error::last_os_error())
    } else {
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        // 표준 핸들을 쓰겠다고 하되 모두 널 — 콘솔에서 띄운 보드의 표준 핸들을 자식이 물려받지 않게 한다
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.lpAttributeList = list;
        let mut pi = PROCESS_INFORMATION::default();
        // SAFETY: 문자열은 모두 NUL 로 끝나고 호출 동안 산다. 핸들 상속 없음(FALSE). env 는 이중 NUL 블록
        let ok = unsafe {
            CreateProcessW(
                app.as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                FALSE,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                env.as_ptr().cast(),
                dir.as_ptr(),
                &si.StartupInfo,
                &mut pi,
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: CreateProcessW 가 준 스레드 핸들은 쓸 일이 없다
            unsafe { CloseHandle(pi.hThread) };
            Ok((pi.hProcess as isize, pi.dwProcessId))
        }
    };
    // SAFETY: 위에서 초기화한 목록
    unsafe { DeleteProcThreadAttributeList(list) };
    result
}

/// Windows 빌드 번호(레지스트리 `CurrentBuildNumber`). xterm 의 `windowsPty` 설정에 쓴다. 못 읽으면 None.
pub fn windows_build() -> Option<u32> {
    static BUILD: OnceLock<Option<u32>> = OnceLock::new();
    *BUILD.get_or_init(|| {
        let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        let name = wide("CurrentBuildNumber");
        let mut buf = [0u16; 32];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: buf 는 bytes 바이트만큼 쓸 수 있고, 키·이름은 NUL 로 끝난다
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if rc != 0 {
            return None;
        }
        let chars = (bytes as usize / 2).min(buf.len());
        String::from_utf16_lossy(&buf[..chars]).trim_end_matches('\0').trim().parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static CLOSES: AtomicUsize = AtomicUsize::new(0);
    static USED_AFTER_CLOSE: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "system" fn fake_create(_: COORD, _: HANDLE, _: HANDLE, _: u32, _: *mut HPCON) -> HRESULT {
        0
    }

    unsafe extern "system" fn fake_resize(_: HPCON, _: COORD) -> HRESULT {
        // 닫힌 뒤에 불리면 해제된 메모리를 만진 것
        if CLOSED.load(Ordering::SeqCst) {
            USED_AFTER_CLOSE.fetch_add(1, Ordering::SeqCst);
        }
        std::thread::yield_now();
        if CLOSED.load(Ordering::SeqCst) {
            USED_AFTER_CLOSE.fetch_add(1, Ordering::SeqCst);
        }
        0
    }

    unsafe extern "system" fn fake_close(_: HPCON) {
        CLOSES.fetch_add(1, Ordering::SeqCst);
        CLOSED.store(true, Ordering::SeqCst);
    }

    static FAKE: ConptyApi = ConptyApi { create: fake_create, resize: fake_resize, close: fake_close };

    fn bare() -> ConPty {
        ConPty {
            api: &FAKE,
            hpc: 1,
            console_open: Mutex::new(true),
            input: Mutex::new(0),
            output: AtomicIsize::new(0),
            process: Mutex::new(0),
            pid: 0,
        }
    }

    /// resize 여럿이 close_console 과 겹쳐도 닫힌 뒤에 resize 가 핸들을 쓰지 않고, 닫기는 한 번이다.
    #[test]
    fn resize_never_touches_the_console_after_close() {
        for _ in 0..300 {
            CLOSED.store(false, Ordering::SeqCst);
            CLOSES.store(0, Ordering::SeqCst);
            USED_AFTER_CLOSE.store(0, Ordering::SeqCst);
            let pty = Arc::new(bare());
            let mut hs = Vec::new();
            for i in 0..3 {
                let p = Arc::clone(&pty);
                hs.push(std::thread::spawn(move || {
                    for k in 0..50 {
                        assert!(p.resize(80 + k + i, 24).is_ok());
                    }
                }));
            }
            let (a, b) = (Arc::clone(&pty), Arc::clone(&pty));
            hs.push(std::thread::spawn(move || a.close_console()));
            hs.push(std::thread::spawn(move || b.close_console()));
            for h in hs {
                h.join().unwrap();
            }
            assert_eq!(USED_AFTER_CLOSE.load(Ordering::SeqCst), 0);
            assert_eq!(CLOSES.load(Ordering::SeqCst), 1);
            assert!(!pty.resize(100, 30).unwrap(), "닫힌 뒤에는 적용하지 않았다고 알린다");
        }
    }
}
