//! 실행 환경 목록 — 내장 터미널 새 탭의 셸과 자율주행 실행 환경이 함께 쓰는 하나의 목록.
//! id 는 터미널 독(`terminal.rs DOCK`)과 같은 글자를 쓰고 WSL 만 더한다.
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvKind {
    Wsl,
    Pwsh,
    PowerShell,
    Cmd,
    GitBash,
}

impl EnvKind {
    pub const ALL: [EnvKind; 5] = [EnvKind::Wsl, EnvKind::Pwsh, EnvKind::PowerShell, EnvKind::Cmd, EnvKind::GitBash];

    pub fn id(self) -> &'static str {
        match self {
            EnvKind::Wsl => "wsl",
            EnvKind::Pwsh => "pwsh",
            EnvKind::PowerShell => "powershell",
            EnvKind::Cmd => "cmd",
            EnvKind::GitBash => "git-bash",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            EnvKind::Wsl => "WSL",
            EnvKind::Pwsh => "PowerShell 7",
            EnvKind::PowerShell => "Windows PowerShell",
            EnvKind::Cmd => "명령 프롬프트",
            EnvKind::GitBash => "Git Bash",
        }
    }

    /// Claude Code 샌드박스를 걸 수 있는 환경인가 — 공식 문서상 WSL2 뿐(네이티브 Windows 는 샌드박스 없이 돈다).
    pub fn sandbox_capable(self) -> bool {
        matches!(self, EnvKind::Wsl)
    }
}

/// 고른 환경. 키 꼴: `wsl:<배포판>` · `wsl`(기본 배포판) · `pwsh` · `powershell` · `cmd` · `git-bash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvChoice {
    pub kind: EnvKind,
    pub distro: Option<String>,
}

impl EnvChoice {
    pub fn key(&self) -> String {
        match (self.kind, &self.distro) {
            (EnvKind::Wsl, Some(d)) => format!("wsl:{d}"),
            (k, _) => k.id().to_string(),
        }
    }

    /// 배포판 이름은 `[A-Za-z0-9._-]{1,64}` 만 받는다(인자로 넘기므로). 그 밖은 None.
    pub fn parse(key: &str) -> Option<Self> {
        if let Some(d) = key.strip_prefix("wsl:") {
            return valid_distro_name(d).then(|| EnvChoice { kind: EnvKind::Wsl, distro: Some(d.to_string()) });
        }
        EnvKind::from_id(key).map(|kind| EnvChoice { kind, distro: None })
    }
}

/// 이 PC 에 있는 환경 하나. `program` 은 띄울 실행 파일
/// (Git Bash 는 mintty 가 아니라 `Git\bin\bash.exe`, WSL 은 `%SystemRoot%\System32\wsl.exe`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvEntry {
    pub choice: EnvChoice,
    pub label: String,
    pub program: PathBuf,
}

/// 배포판 이름 규칙 `[A-Za-z0-9._-]{1,64}` — 인자로 넘기므로 이 밖의 글자는 받지 않는다.
fn valid_distro_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// `wsl.exe -l -q` 출력(UTF-16LE, BOM 이 있을 수도 없을 수도) → 배포판 이름.
/// 빈 줄, `docker-desktop` 으로 시작하는 이름, 위 이름 규칙에 어긋나는 이름은 뺀다. 순서는 지키고 겹친 이름은 한 번만 낸다.
pub fn parse_wsl_list(raw: &[u8]) -> Vec<String> {
    let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
    // BOM(U+FEFF)과 NUL 찌꺼기는 이름의 일부가 아니다
    let text: String = String::from_utf16_lossy(&units).chars().filter(|&c| c != '\0' && c != '\u{feff}').collect();
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        let name = line.trim();
        if name.starts_with("docker-desktop") || !valid_distro_name(name) || names.iter().any(|n| n == name) {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

/// 있는 환경을 `EnvKind::ALL` 차례로 돌려준다(Windows 가 아니면 빈 목록).
/// WSL 은 wsl.exe 가 있고 `-l -q` 가 5초 안에 종료 코드 0 과 배포판을 하나 이상 줄 때만 넣는다(배포판마다 한 줄).
/// 기능이 꺼진 PC 의 wsl.exe 는 설치 안내만 내므로, 파일이 있는지만으로는 판정하지 않는다.
pub fn detect() -> Vec<EnvEntry> {
    if !cfg!(windows) {
        return Vec::new();
    }
    EnvKind::ALL.into_iter().flat_map(entries_of).collect()
}

fn entries_of(kind: EnvKind) -> Vec<EnvEntry> {
    if kind == EnvKind::Wsl {
        return wsl_entries();
    }
    let Some(program) = locate(kind) else { return Vec::new() };
    vec![EnvEntry { choice: EnvChoice { kind, distro: None }, label: kind.label().to_string(), program }]
}

/// 배포판은 목록만 보고 띄우지 않는다(`-l -q` 는 배포판을 시작하지 않는다).
fn wsl_entries() -> Vec<EnvEntry> {
    let Some(program) = env_dir("SystemRoot").map(|d| d.join("System32").join("wsl.exe")).filter(|p| p.is_file()) else {
        return Vec::new();
    };
    let Some(raw) = run_with_timeout(&program, &["-l", "-q"], Duration::from_secs(5)) else { return Vec::new() };
    parse_wsl_list(&raw)
        .into_iter()
        .map(|name| EnvEntry {
            label: format!("WSL · {name}"),
            choice: EnvChoice { kind: EnvKind::Wsl, distro: Some(name) },
            program: program.clone(),
        })
        .collect()
}

/// 프로그램을 돌려 종료 코드 0 일 때만 표준 출력을 준다. 제한 시간을 넘기면 죽이고 None.
fn run_with_timeout(program: &Path, args: &[&str], limit: Duration) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    // 출력이 파이프 용량을 넘어도 자식이 막히지 않게 따로 읽는다
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                // 자식이 죽으면 파이프가 닫혀 읽는 스레드도 끝난다
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()?;
    status.success().then_some(out)
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

fn first_file(dirs: Vec<PathBuf>, exe: &str) -> Option<PathBuf> {
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

/// 실행 파일 위치. 탐색 순서는 `terminal.rs locate`(PATH → 알려진 설치 위치)를 따른다.
fn locate(kind: EnvKind) -> Option<PathBuf> {
    let system32 = env_dir("SystemRoot").map(|d| d.join("System32"));
    match kind {
        EnvKind::Wsl => None,
        EnvKind::Pwsh => {
            let mut dirs = path_dirs();
            dirs.extend(env_dir("ProgramFiles").map(|d| d.join("PowerShell").join("7")));
            first_file(dirs, "pwsh.exe")
        }
        EnvKind::PowerShell => {
            system32.map(|d| d.join("WindowsPowerShell").join("v1.0").join("powershell.exe")).filter(|p| p.is_file())
        }
        EnvKind::Cmd => env_dir("ComSpec")
            .filter(|p| p.is_file())
            .or_else(|| system32.map(|d| d.join("cmd.exe")).filter(|p| p.is_file())),
        EnvKind::GitBash => locate_git_bash(),
    }
}

/// Git for Windows 의 `bin\bash.exe`. PATH 의 `bash.exe` 는 System32 의 WSL 발사기일 수 있어 그대로 쓰지 않고,
/// PATH 의 `Git\cmd`(git.exe 가 있는 곳)에서 설치 루트를 거슬러 올라간 뒤 알려진 설치 위치를 본다.
fn locate_git_bash() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = path_dirs()
        .into_iter()
        .filter(|d| d.join("git.exe").is_file())
        .filter_map(|d| d.parent().map(Path::to_path_buf))
        .collect();
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        roots.extend(env_dir(key).map(|d| d.join("Git")));
    }
    roots.extend(env_dir("LOCALAPPDATA").map(|d| d.join("Programs").join("Git")));
    roots.into_iter().map(|r| r.join("bin").join("bash.exe")).find(|p| p.is_file())
}
