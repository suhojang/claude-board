//! 가짜 콘솔(pty) — Windows 는 ConPTY, 그 밖은 `Unsupported` 를 돌려주는 스텁(macOS 개발 빌드가 컴파일되게).
//! 셸을 고르는 규칙(pwsh → powershell → ComSpec, 그리고 키로 고른 WSL·Git Bash)도 여기 둔다.

mod env;

use std::path::{Path, PathBuf};

use claude_board_core::envs::{self, EnvChoice, EnvEntry, EnvKind};

#[cfg(windows)]
mod conpty;
#[cfg(all(test, windows))]
mod capture;
#[cfg(windows)]
pub use conpty::{windows_build, ConPty as Pty};

/// 터미널 한 칸이 띄울 셸.
#[derive(Debug, Clone)]
pub struct Shell {
    /// 실행 환경 키 — `pwsh` · `powershell` · `cmd` · `git-bash` · `wsl:<배포판>` · `wsl`
    pub kind: String,
    pub path: PathBuf,
    pub args: Vec<String>,
}

/// 기본 셸: pwsh → powershell → `%ComSpec%`(없으면 cmd.exe 를 찾는다). pwsh·powershell 은 `-NoLogo` 만 주고 프로필은 로드한다.
pub fn resolve_shell() -> Option<Shell> {
    ["pwsh", "powershell", "cmd"].into_iter().find_map(native_shell)
}

/// pwsh · powershell · cmd 중 하나를 찾는다. 그 밖의 키는 None.
fn native_shell(kind: &str) -> Option<Shell> {
    let (path, args) = match kind {
        "pwsh" | "powershell" => (crate::terminal::locate(kind)?, vec!["-NoLogo".to_string()]),
        "cmd" => {
            let comspec = std::env::var_os("ComSpec").map(PathBuf::from).filter(|p| p.is_file());
            (comspec.or_else(|| crate::terminal::locate("cmd"))?, Vec::new())
        }
        _ => return None,
    };
    Some(Shell { kind: kind.to_string(), path, args })
}

/// 실행 환경 키가 가리키는 셸. 키 모양이 틀리거나 이 PC 에 없으면 None(부르는 쪽이 기본 셸로 돌아간다).
/// `cwd` 는 WSL 의 `--cd` 에 쓴다.
pub fn resolve_shell_for(key: &str, cwd: &Path) -> Option<Shell> {
    let choice = EnvChoice::parse(key)?;
    match choice.kind {
        EnvKind::Pwsh | EnvKind::PowerShell | EnvKind::Cmd => native_shell(choice.kind.id()),
        // detect 는 wsl.exe 를 한 번 돌린다(배포판을 띄우지 않는다)
        EnvKind::Wsl | EnvKind::GitBash => shell_from_entries(&choice, &envs::detect(), &cwd.to_string_lossy()),
    }
}

/// 찾아 둔 환경 목록에서 WSL·Git Bash 셸을 만든다(순수 — 목록만 본다).
/// WSL 은 `wsl.exe -d <배포판> --cd <폴더>` 를 인자 배열로 만들고, Git Bash 는 `bash.exe --login -i` 다.
fn shell_from_entries(choice: &EnvChoice, entries: &[EnvEntry], cwd: &str) -> Option<Shell> {
    match choice.kind {
        EnvKind::GitBash => {
            let entry = entries.iter().find(|e| e.choice.kind == EnvKind::GitBash)?;
            Some(Shell { kind: choice.key(), path: entry.program.clone(), args: vec!["--login".to_string(), "-i".to_string()] })
        }
        EnvKind::Wsl => {
            // 배포판을 골랐으면 목록에 있어야 하고, 안 골랐으면(`wsl`) 기본 배포판이라 wsl.exe 위치만 필요하다
            let entry = entries.iter().find(|e| e.choice.kind == EnvKind::Wsl && (choice.distro.is_none() || e.choice == *choice))?;
            let launch = wsl_launch(choice.distro.as_deref(), cwd);
            let kind = match &launch.distro {
                Some(_) => EnvChoice { kind: EnvKind::Wsl, distro: launch.distro.clone() }.key(),
                None => "wsl".to_string(),
            };
            Some(Shell { kind, path: entry.program.clone(), args: launch.args })
        }
        _ => None,
    }
}

/// WSL 탭을 띄우는 방법 — 쓸 배포판과 `wsl.exe` 인자.
#[derive(Debug, PartialEq, Eq)]
pub struct WslLaunch {
    pub distro: Option<String>,
    pub args: Vec<String>,
}

/// `wsl.exe -d <배포판> --cd <폴더>` 인자를 만든다. 폴더가 `\\wsl.localhost\<배포판>\…` · `\\wsl$\<배포판>\…` 이면
/// 그 배포판을 쓰고 폴더는 리눅스 경로(`/…`)로 바꾼다. 그 밖은 Windows 경로를 그대로 준다(`wsl.exe` 가 바꿔 읽는다).
/// 배포판 이름이 규칙(`EnvChoice::parse`)에 어긋나면 그 배포판은 쓰지 않고 `--cd` 도 빼서 기본 위치로 간다.
pub fn wsl_launch(distro: Option<&str>, cwd: &str) -> WslLaunch {
    let mut distro = distro.map(str::to_string);
    let mut cd = (!cwd.is_empty()).then(|| cwd.to_string());
    if let Some((unc_distro, linux)) = wsl_unc_path(cwd) {
        if EnvChoice::parse(&format!("wsl:{unc_distro}")).is_some() {
            distro = Some(unc_distro);
            cd = Some(linux);
        } else {
            cd = None;
        }
    }
    let mut args = Vec::new();
    if let Some(d) = &distro {
        args.extend(["-d".to_string(), d.clone()]);
    }
    if let Some(c) = cd {
        args.extend(["--cd".to_string(), c]);
    }
    WslLaunch { distro, args }
}

/// `\\wsl.localhost\<배포판>\a\b` · `\\wsl$\<배포판>\a\b` → (`<배포판>`, `/a/b`). 대소문자는 가리지 않고 `/` 로 쓴 꼴과 `\\?\UNC\` 꼴도 받는다.
/// WSL 경로가 아니면 None. 배포판 이름 검사는 하지 않는다(부르는 쪽이 한다).
pub fn wsl_unc_path(path: &str) -> Option<(String, String)> {
    let flat = path.replace('/', "\\");
    let rest = match flat.strip_prefix(r"\\?\UNC\") {
        Some(rest) => rest,
        None => flat.strip_prefix(r"\\")?,
    };
    let (host, rest) = rest.split_once('\\')?;
    if !host.eq_ignore_ascii_case("wsl.localhost") && !host.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    let (distro, tail) = rest.split_once('\\').unwrap_or((rest, ""));
    if distro.is_empty() {
        return None;
    }
    let parts: Vec<&str> = tail.split('\\').filter(|p| !p.is_empty()).collect();
    Some((distro.to_string(), format!("/{}", parts.join("/"))))
}

/// 프로세스를 띄울 때의 작업 폴더. WSL 은 `\\wsl.localhost\…` 를 작업 폴더로 받지 못할 수 있어 홈으로 두고 `--cd` 가 폴더를 정한다.
pub fn process_cwd(shell_kind: &str, cwd: &Path) -> PathBuf {
    if shell_kind.starts_with("wsl") && wsl_unc_path(&cwd.to_string_lossy()).is_some() {
        claude_board_core::paths::home()
    } else {
        cwd.to_path_buf()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: EnvKind, distro: Option<&str>, program: &str) -> EnvEntry {
        EnvEntry { choice: EnvChoice { kind, distro: distro.map(str::to_string) }, label: String::new(), program: program.into() }
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unc_wsl_paths_become_linux_paths() {
        let got = wsl_unc_path;
        assert_eq!(got(r"\\wsl.localhost\Ubuntu\home\me\proj"), Some(("Ubuntu".into(), "/home/me/proj".into())));
        assert_eq!(got(r"\\wsl$\Debian\tmp\"), Some(("Debian".into(), "/tmp".into())));
        assert_eq!(got(r"\\WSL.LOCALHOST\Ubuntu-22.04"), Some(("Ubuntu-22.04".into(), "/".into())));
        assert_eq!(got("//wsl.localhost/Ubuntu/a/b"), Some(("Ubuntu".into(), "/a/b".into())));
        assert_eq!(got(r"\\?\UNC\wsl.localhost\Ubuntu\a"), Some(("Ubuntu".into(), "/a".into())));
        assert_eq!(got(r"\\server\share\a"), None);
        assert_eq!(got(r"C:\Users\me"), None);
        assert_eq!(got(r"\\wsl.localhost\"), None);
        assert_eq!(got(""), None);
    }

    #[test]
    fn wsl_launch_args() {
        // 배포판을 골랐고 Windows 폴더 — 폴더는 그대로 넘긴다
        assert_eq!(wsl_launch(Some("Ubuntu"), r"C:\work\a b").args, strings(&["-d", "Ubuntu", "--cd", r"C:\work\a b"]));
        // 기본 배포판
        assert_eq!(wsl_launch(None, r"C:\work").args, strings(&["--cd", r"C:\work"]));
        // UNC 폴더 — 그 배포판과 리눅스 경로를 쓰고, 고른 배포판은 덮어쓴다
        let l = wsl_launch(Some("Ubuntu"), r"\\wsl.localhost\Debian\home\me");
        assert_eq!((l.distro.as_deref(), l.args), (Some("Debian"), strings(&["-d", "Debian", "--cd", "/home/me"])));
        // 규칙에 어긋난 배포판 이름 — 쓰지 않고 --cd 도 뺀다
        let bad = wsl_launch(Some("Ubuntu"), r"\\wsl$\bad name\x");
        assert_eq!((bad.distro.as_deref(), bad.args), (Some("Ubuntu"), strings(&["-d", "Ubuntu"])));
        assert!(wsl_launch(None, "").args.is_empty());
    }

    #[test]
    fn shells_from_entries() {
        let entries = [
            entry(EnvKind::Wsl, Some("Ubuntu"), r"C:\Windows\System32\wsl.exe"),
            entry(EnvKind::GitBash, None, r"C:\Git\bin\bash.exe"),
        ];
        let parse = |k: &str| EnvChoice::parse(k).unwrap();

        let ubuntu = shell_from_entries(&parse("wsl:Ubuntu"), &entries, r"C:\w").unwrap();
        assert_eq!((ubuntu.kind.as_str(), ubuntu.args), ("wsl:Ubuntu", strings(&["-d", "Ubuntu", "--cd", r"C:\w"])));
        assert_eq!(ubuntu.path, PathBuf::from(r"C:\Windows\System32\wsl.exe"));
        // 목록에 없는 배포판은 None
        assert!(shell_from_entries(&parse("wsl:Gone"), &entries, r"C:\w").is_none());
        // 기본 배포판 `wsl`
        let default = shell_from_entries(&parse("wsl"), &entries, r"C:\w").unwrap();
        assert_eq!((default.kind.as_str(), default.args), ("wsl", strings(&["--cd", r"C:\w"])));
        // UNC 폴더는 키의 배포판을 덮어쓴다
        let unc = shell_from_entries(&parse("wsl"), &entries, r"\\wsl.localhost\Ubuntu\home\me").unwrap();
        assert_eq!((unc.kind.as_str(), unc.args), ("wsl:Ubuntu", strings(&["-d", "Ubuntu", "--cd", "/home/me"])));

        let bash = shell_from_entries(&parse("git-bash"), &entries, r"C:\w").unwrap();
        assert_eq!((bash.kind.as_str(), bash.args), ("git-bash", strings(&["--login", "-i"])));
        assert!(shell_from_entries(&parse("git-bash"), &[], r"C:\w").is_none());
        assert!(shell_from_entries(&parse("wsl"), &[], r"C:\w").is_none());
    }

    #[test]
    fn invalid_keys_resolve_to_nothing() {
        assert!(resolve_shell_for("wsl:bad name", Path::new(".")).is_none());
        assert!(resolve_shell_for("zsh", Path::new(".")).is_none());
    }

    #[test]
    fn process_cwd_only_moves_off_wsl_unc() {
        let home = claude_board_core::paths::home();
        assert_eq!(process_cwd("wsl:Ubuntu", Path::new(r"\\wsl.localhost\Ubuntu\a")), home);
        assert_eq!(process_cwd("wsl", Path::new(r"C:\w")), PathBuf::from(r"C:\w"));
        assert_eq!(process_cwd("pwsh", Path::new(r"\\wsl.localhost\Ubuntu\a")), PathBuf::from(r"\\wsl.localhost\Ubuntu\a"));
    }
}
