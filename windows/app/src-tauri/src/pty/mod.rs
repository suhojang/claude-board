//! 가짜 콘솔(pty) — Windows 는 ConPTY, 그 밖은 `Unsupported` 를 돌려주는 스텁(macOS 개발 빌드가 컴파일되게).
//! 셸을 고르는 규칙(pwsh → powershell → ComSpec)도 여기 둔다.

mod env;

use std::path::PathBuf;

#[cfg(windows)]
mod conpty;
#[cfg(all(test, windows))]
mod capture;
#[cfg(windows)]
pub use conpty::{windows_build, ConPty as Pty};

/// 터미널 한 칸이 띄울 셸.
#[derive(Debug, Clone)]
pub struct Shell {
    /// `pwsh` · `powershell` · `cmd`
    pub kind: &'static str,
    pub path: PathBuf,
    pub args: Vec<String>,
}

/// 기본 셸: pwsh → powershell → `%ComSpec%`(없으면 cmd.exe 를 찾는다). pwsh·powershell 은 `-NoLogo` 만 주고 프로필은 로드한다.
pub fn resolve_shell() -> Option<Shell> {
    let nologo = || vec!["-NoLogo".to_string()];
    for kind in ["pwsh", "powershell"] {
        if let Some(path) = crate::terminal::locate(kind) {
            return Some(Shell { kind, path, args: nologo() });
        }
    }
    let comspec = std::env::var_os("ComSpec").map(PathBuf::from).filter(|p| p.is_file());
    comspec.or_else(|| crate::terminal::locate("cmd")).map(|path| Shell { kind: "cmd", path, args: Vec::new() })
}

#[cfg(not(windows))]
pub use stub::{windows_build, Pty};

/// Windows 가 아니면 터미널을 열 수 없다. 같은 모양의 함수만 두고 모두 `Unsupported`.
#[cfg(not(windows))]
mod stub {
    use std::io;
    use std::path::Path;

    pub struct Pty;

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "내장 터미널은 Windows 에서만 쓸 수 있다"))
    }

    pub fn windows_build() -> Option<u32> {
        None
    }

    impl Pty {
        pub fn spawn(_program: &Path, _args: &[String], _cwd: &Path, _cols: u16, _rows: u16) -> io::Result<Pty> {
            unsupported()
        }

        pub fn spawn_env(_program: &Path, _args: &[String], _cwd: &Path, _cols: u16, _rows: u16, _vars: &[(String, String)]) -> io::Result<Pty> {
            unsupported()
        }

        pub fn pid(&self) -> u32 {
            0
        }

        pub fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }

        pub fn close_output(&self) {}

        pub fn write_all(&self, _data: &[u8]) -> io::Result<()> {
            unsupported()
        }

        pub fn resize(&self, _cols: u16, _rows: u16) -> io::Result<bool> {
            unsupported()
        }

        pub fn close_console(&self) {}

        pub fn wait_exit(&self) -> u32 {
            0
        }

        pub fn release(&self) {}

        pub fn kill(&self) {}
    }
}
