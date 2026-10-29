//! 로그인할 때 열기(설정 > 일반, mac `LoginItem`) — 설치가 등록하는 시작 프로그램(HKCU Run 값 `ClaudeBoard`)을 켜고 끈다.
//!
//! 끄면 등록을 지우고 표지 파일 `login-launch-off`(`claude_board_core::paths::login_launch_off_file`)를 남겨 업데이트의 `setup` 이 다시
//! 등록하지 않게 한다. 지금 도는 보드는 그대로 둔다. 켜면 등록을 다시 쓰고 표지를 지운다 — 다음 로그인부터.
//!
//! 개발 식별자(`CLAUDE_BOARD_HOME`·`CLAUDE_BOARD_NOTIFIER_DIR` 로 띄운 보드)는 설치된 보드의 Run 값을 건드리지 않는다
//! (mac 시험 빌드가 번들 id 가 달라 설치판 LaunchAgent 를 못 건드리는 것과 같다) — 자기 notifier 폴더의 표지 파일만 바꾼다.
//! 레지스트리는 `RunKey` 뒤에 둬서 시험이 진짜 Run 키를 쓰지 않는다.

use std::io;
use std::path::Path;

use claude_board_core::paths;

/// 시작 프로그램 값 이름 — `crates/hook/src/setup.rs` 의 `RUN_VALUE` 와 같아야 한다(설치가 쓰고 제거가 지우는 그 값).
pub const RUN_VALUE: &str = "ClaudeBoard";

/// 시작 프로그램 등록을 쓰고 지우는 곳(HKCU\…\Run).
pub trait RunKey {
    fn set(&self, name: &str, value: &str) -> io::Result<()>;
    /// 값을 지운다. 없으면 성공.
    fn delete(&self, name: &str) -> io::Result<()>;
    /// 값이 있는가 — 켜 있는지 실제 상태로 다시 읽는 데 쓴다. 읽지 못하면 Err
    fn exists(&self, name: &str) -> io::Result<bool>;
}

/// 표지 파일이 없으면 켠 것(처음 설치는 켠 채로 시작한다).
fn enabled_at(off_mark: &Path) -> bool {
    !off_mark.exists()
}

/// Run 값 내용 — 설치(`setup`)가 쓰는 것과 같은 모양: 큰따옴표로 감싼 앱 경로와 `--background`.
pub fn run_command(app: &Path) -> String {
    format!("\"{}\" --background", app.display())
}

/// 켜거나 끈다. `run` 이 `None` 이면 레지스트리는 건드리지 않고 표지 파일만 바꾼다(개발 식별자).
///
/// 순서는 어느 쪽이 실패해도 "끈 것으로 보이는 쪽"으로 남게 한다: 끌 때는 표지를 먼저 쓰고(업데이트가 다시 등록하지 않게) 값을 지우고,
/// 켤 때는 값을 먼저 쓰고 성공해야 표지를 지운다.
pub fn apply(on: bool, run: Option<&dyn RunKey>, app: &Path, off_mark: &Path) -> io::Result<()> {
    if on {
        if let Some(run) = run {
            run.set(RUN_VALUE, &run_command(app))?;
        }
        match std::fs::remove_file(off_mark) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        if let Some(dir) = off_mark.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(off_mark, b"")?;
        match run {
            Some(run) => run.delete(RUN_VALUE),
            None => Ok(()),
        }
    }
}

/// 지금 켜 있는가 — Run 값이 있으면 켠 것(표지 파일이 아니라 실제로 로그인 때 열리는지). 레지스트리가 없거나(개발 식별자·Windows 가 아닌 기기)
/// 읽지 못하면 표지 파일을 따른다. 끄는 쪽이 값을 못 지웠으면 켜 있는 그대로 보여 준다.
fn read_state(run: Option<&dyn RunKey>, off_mark: &Path) -> bool {
    let from_mark = enabled_at(off_mark);
    let Some(run) = run else { return from_mark };
    match run.exists(RUN_VALUE) {
        Ok(present) => present,
        Err(e) => {
            crate::util::board_log(&format!("로그인할 때 열기: 시작 프로그램 값을 읽지 못했습니다(표지 파일을 따릅니다): {e}"));
            from_mark
        }
    }
}

/// `apply` 의 결과를 프론트에 보일 글로 — 켜기·끄기 어느 쪽이 안 됐는지 말한다.
fn set_state(on: bool, run: Option<&dyn RunKey>, app: &Path, off_mark: &Path) -> Result<(), String> {
    apply(on, run, app, off_mark).map_err(|e| {
        let msg = format!("로그인할 때 열기를 {}지 못했습니다: {e}", if on { "켜" } else { "끄" });
        crate::util::board_log(&msg);
        msg
    })
}

/// 진짜 HKCU\…\Run — 개발 식별자에서는 쓰지 않는다.
#[cfg(windows)]
pub struct Registry {
    subkey: &'static str,
}

#[cfg(windows)]
impl Registry {
    pub const RUN_KEY: &'static str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn real() -> Registry {
        Registry { subkey: Self::RUN_KEY }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
impl RunKey for Registry {
    fn set(&self, name: &str, value: &str) -> io::Result<()> {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
        let (key, name, data) = (Self::wide(self.subkey), Self::wide(name), Self::wide(value));
        // SAFETY: NUL 로 끝나는 문자열과 바이트 길이(NUL 포함)를 넘긴다
        let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
        if r == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(r as i32))
        }
    }

    fn delete(&self, name: &str) -> io::Result<()> {
        use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
        use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
        let (key, name) = (Self::wide(self.subkey), Self::wide(name));
        // SAFETY: NUL 로 끝나는 문자열
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(r as i32))
        }
    }

    fn exists(&self, name: &str) -> io::Result<bool> {
        use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
        use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let (key, name) = (Self::wide(self.subkey), Self::wide(name));
        let mut len = 0u32;
        // SAFETY: 버퍼 없이 길이만 묻는다. NUL 로 끝나는 문자열
        let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut len) };
        match r {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
            other => Err(io::Error::from_raw_os_error(other as i32)),
        }
    }
}

/// 지금 시작 프로그램에 쓸 Run 자리 — 개발 식별자에서는 없다(레지스트리를 건드리지 않는다)
#[cfg(windows)]
fn real_run() -> Option<Registry> {
    (!paths::overridden()).then(Registry::real)
}

/// 프론트의 `loginLaunch()` — 지금 로그인할 때 열기가 켜져 있는가. Run 값을 실제로 읽어 답한다.
#[tauri::command(async)]
pub fn login_launch() -> bool {
    #[cfg(windows)]
    let real = real_run();
    #[cfg(windows)]
    let run: Option<&dyn RunKey> = real.as_ref().map(|r| r as &dyn RunKey);
    #[cfg(not(windows))]
    let run: Option<&dyn RunKey> = None;
    read_state(run, &paths::login_launch_off_file())
}

/// 프론트의 `setLoginLaunch(on)` — 레지스트리나 표지 파일을 쓰지 못하면 사유를 돌려준다(프론트가 실제 상태를 다시 읽는다).
#[tauri::command(async)]
pub fn set_login_launch(on: bool) -> Result<(), String> {
    let app = std::env::current_exe().map_err(|e| {
        let msg = format!("로그인할 때 열기: 실행 파일 경로를 알 수 없습니다: {e}");
        crate::util::board_log(&msg);
        msg
    })?;
    #[cfg(windows)]
    let real = real_run();
    #[cfg(windows)]
    let run: Option<&dyn RunKey> = real.as_ref().map(|r| r as &dyn RunKey);
    // Windows 가 아닌 개발 기기에서는 레지스트리가 없다
    #[cfg(not(windows))]
    let run: Option<&dyn RunKey> = None;
    set_state(on, run, &app, &paths::login_launch_off_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    /// 레지스트리 자리에 놓는 메모리 값 — (이름, 값) 목록.
    #[derive(Default)]
    struct Fake {
        values: RefCell<Vec<(String, String)>>,
        fail_set: bool,
        fail_delete: bool,
        fail_read: bool,
    }

    impl RunKey for Fake {
        fn set(&self, name: &str, value: &str) -> io::Result<()> {
            if self.fail_set {
                return Err(io::Error::other("set"));
            }
            let mut v = self.values.borrow_mut();
            v.retain(|(n, _)| n != name);
            v.push((name.to_string(), value.to_string()));
            Ok(())
        }
        fn delete(&self, name: &str) -> io::Result<()> {
            if self.fail_delete {
                return Err(io::Error::other("delete"));
            }
            self.values.borrow_mut().retain(|(n, _)| n != name);
            Ok(())
        }
        fn exists(&self, name: &str) -> io::Result<bool> {
            if self.fail_read {
                return Err(io::Error::other("read"));
            }
            Ok(self.values.borrow().iter().any(|(n, _)| n == name))
        }
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-login-item-{}", crate::util::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const APP: &str = r"C:\Apps\Claude Board\ClaudeBoard.exe";

    #[test]
    fn on_unless_the_off_mark_exists() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        assert!(enabled_at(&mark));
        std::fs::write(&mark, b"").unwrap();
        assert!(!enabled_at(&mark));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_value_is_the_quoted_path_with_background_flag() {
        assert_eq!(run_command(Path::new(APP)), r#""C:\Apps\Claude Board\ClaudeBoard.exe" --background"#);
    }

    #[test]
    fn off_removes_the_value_and_writes_the_mark() {
        let dir = scratch();
        let mark = dir.join("notifier").join("login-launch-off");
        let run = Fake::default();
        run.set(RUN_VALUE, "old").unwrap();
        apply(false, Some(&run), Path::new(APP), &mark).unwrap();
        assert!(run.values.borrow().is_empty());
        assert!(mark.is_file(), "setup·업데이트가 다시 등록하지 않게 표지를 남긴다(폴더가 없어도)");
        assert!(!enabled_at(&mark));
        // 이미 꺼 둔 것을 또 꺼도 같다
        apply(false, Some(&run), Path::new(APP), &mark).unwrap();
        assert!(mark.is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn on_writes_the_value_and_removes_the_mark() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        std::fs::write(&mark, b"").unwrap();
        let run = Fake::default();
        apply(true, Some(&run), Path::new(APP), &mark).unwrap();
        assert_eq!(*run.values.borrow(), vec![(RUN_VALUE.to_string(), run_command(Path::new(APP)))]);
        assert!(!mark.exists());
        assert!(enabled_at(&mark));
        // 표지가 없는 채로 또 켜도 오류가 아니다
        apply(true, Some(&run), Path::new(APP), &mark).unwrap();
        assert_eq!(run.values.borrow().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_a_run_key_only_the_mark_changes() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        apply(false, None, Path::new(APP), &mark).unwrap();
        assert!(mark.is_file());
        apply(true, None, Path::new(APP), &mark).unwrap();
        assert!(!mark.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_set_keeps_the_off_mark() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        std::fs::write(&mark, b"").unwrap();
        let run = Fake { fail_set: true, ..Fake::default() };
        assert!(apply(true, Some(&run), Path::new(APP), &mark).is_err());
        assert!(mark.is_file(), "등록이 안 됐는데 켠 것으로 보이면 안 된다");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_delete_still_leaves_the_mark() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        let run = Fake { fail_delete: true, ..Fake::default() };
        assert!(apply(false, Some(&run), Path::new(APP), &mark).is_err());
        assert!(mark.is_file(), "값을 못 지워도 업데이트가 다시 등록하지 않게 표지는 남는다");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_comes_back_as_an_error_message() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        let run = Fake { fail_set: true, ..Fake::default() };
        let err = set_state(true, Some(&run), Path::new(APP), &mark).unwrap_err();
        assert!(err.starts_with("로그인할 때 열기를 켜지 못했습니다") && err.contains("set"), "{err}");
        let run = Fake { fail_delete: true, ..Fake::default() };
        let err = set_state(false, Some(&run), Path::new(APP), &mark).unwrap_err();
        assert!(err.starts_with("로그인할 때 열기를 끄지 못했습니다") && err.contains("delete"), "{err}");
        assert!(set_state(true, Some(&Fake::default()), Path::new(APP), &mark).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn state_is_read_back_from_the_run_value_not_the_mark() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        let run = Fake::default();
        // 값이 없으면 표지가 없어도 꺼진 것
        assert!(!read_state(Some(&run), &mark));
        set_state(true, Some(&run), Path::new(APP), &mark).unwrap();
        assert!(read_state(Some(&run), &mark));
        // 끄다가 값을 못 지웠다: 표지는 있지만 실제로는 로그인 때 열리므로 켜 있다고 답한다
        let stuck = Fake { fail_delete: true, ..Fake::default() };
        stuck.set(RUN_VALUE, "x").unwrap();
        assert!(set_state(false, Some(&stuck), Path::new(APP), &mark).is_err());
        assert!(mark.is_file());
        assert!(read_state(Some(&stuck), &mark));
        // 지워지면 꺼짐
        assert!(set_state(false, Some(&run), Path::new(APP), &mark).is_ok());
        assert!(!read_state(Some(&run), &mark));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn state_follows_the_mark_without_a_run_key_or_when_it_cannot_be_read() {
        let dir = scratch();
        let mark = dir.join("login-launch-off");
        assert!(read_state(None, &mark));
        std::fs::write(&mark, b"").unwrap();
        assert!(!read_state(None, &mark));
        let unreadable = Fake { fail_read: true, ..Fake::default() };
        assert!(!read_state(Some(&unreadable), &mark));
        std::fs::remove_file(&mark).unwrap();
        assert!(read_state(Some(&unreadable), &mark));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 진짜 Registry 어댑터를 임시 키(HKCU\Software\ClaudeBoardTest-…)에서만 시험한다 — 진짜 Run 키는 쓰지 않는다.
    #[cfg(windows)]
    #[test]
    fn registry_adapter_round_trips_on_a_temp_key() {
        use windows_sys::Win32::System::Registry::{RegDeleteTreeW, RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let sub: &'static str = Box::leak(format!(r"Software\ClaudeBoardTest-{}\Run", crate::util::short_id()).into_boxed_str());
        assert_ne!(sub, Registry::RUN_KEY);
        let reg = Registry { subkey: sub };
        let read = || -> Option<String> {
            let (key, name) = (Registry::wide(sub), Registry::wide(RUN_VALUE));
            let mut buf = [0u16; 512];
            let mut len = std::mem::size_of_val(&buf) as u32;
            // SAFETY: 버퍼와 그 바이트 길이를 넘기고, NUL 로 끝나는 문자열을 준다
            let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len) };
            (r == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
        };
        assert_eq!(read(), None);
        assert!(!reg.exists(RUN_VALUE).unwrap(), "키가 아직 없어도 오류가 아니라 없음");
        reg.set(RUN_VALUE, &run_command(Path::new(APP))).unwrap();
        assert!(reg.exists(RUN_VALUE).unwrap());
        assert_eq!(read().as_deref(), Some(run_command(Path::new(APP)).as_str()));
        reg.delete(RUN_VALUE).unwrap();
        assert!(!reg.exists(RUN_VALUE).unwrap());
        assert_eq!(read(), None);
        reg.delete(RUN_VALUE).unwrap(); // 없어도 성공
        let root = Registry::wide(sub.split('\\').take(2).collect::<Vec<_>>().join("\\").as_str());
        // SAFETY: NUL 로 끝나는 문자열 — 시험이 만든 임시 키 가지만 지운다
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, root.as_ptr()) };
    }
}
