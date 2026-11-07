//! 실행 폴더 파일 어댑터(`ports::RunFiles`)와 시각 어댑터(`ports::Clock`).
//!
//! - 쓰기: 같은 폴더 임시 파일 → 디스크까지 내림 → `MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)` 로 이름 바꿈.
//!   보드가 읽는 파일은 늘 "옛것 아니면 새것 전체"다. 읽는 쪽이 잠깐 쥐고 있어 거부되면(접근 거부·공유 위반) 짧게 다시 한다.
//! - 덧붙이기: 줄 하나를 `write` 한 번으로 보낸다. 덧붙이기 모드의 한 번 쓰기는 OS 가 끝 위치를 잡아 주므로 스레드·프로세스끼리 줄이 섞이지 않는다.
//! - 개인 폴더: `icacls` 자식 프로세스(창 없음)로 상속을 끄고 지금 사용자만 준다. 보안 기술자 API(`SetNamedSecurityInfoW`)는
//!   `windows-sys` 의 `Win32_Security_Authorization` 기능과 ACL 구조체 조립이 더 든다. `icacls` 는 한 줄이고 결과를 `icacls` 로 바로 볼 수 있다.
//! - 잠금: `LockFileEx` 배타·즉시 실패. 잠금은 핸들 단위라 같은 프로세스의 두 번째 시도도 거부된다.

use crate::model::to_pretty_json;
use crate::ports::{Clock, FileStat, Held, PortError, PortErrorKind, PortResult, RunFiles};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// `read_json` 이 읽는 파일 하나의 크기 한도. 모델이 쓸 수 있는 폴더의 파일(`result.json` 등)을 읽을 때의 마지막 방어선이다
/// (그 파일들의 더 낮은 한도 `engine::run::MODEL_FILE_MAX` 는 엔진이 `stat` 으로 먼저 본다)
pub const JSON_MAX_BYTES: u64 = 16 << 20;

/// 임시 파일 이름 표시. `list` 는 이 꼴을 숨긴다(쓰는 도중의 흔적이라 이름 목록에 낄 자리가 아니다).
const TEMP_PREFIX: &str = ".";
const TEMP_SUFFIX: &str = ".tmp";

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Clone, Copy)]
pub struct FsRunFiles;

impl FsRunFiles {
    pub fn new() -> Self {
        Self
    }
}

fn temp_sibling(path: &Path) -> PortResult<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| PortError::new(PortErrorKind::Invalid, format!("파일 이름이 없다: {}", path.display())))?
        .to_string_lossy();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = format!("{TEMP_PREFIX}{name}.{}.{seq}{TEMP_SUFFIX}", std::process::id());
    Ok(path.with_file_name(temp))
}

fn is_temp_name(name: &str) -> bool {
    name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX) && name.len() > TEMP_PREFIX.len() + TEMP_SUFFIX.len()
}

impl RunFiles for FsRunFiles {
    fn read_json(&self, path: &Path) -> PortResult<Option<serde_json::Value>> {
        use std::io::Read;
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        // 모델이 쓸 수 있는 폴더의 파일을 읽는다: 보통 파일만, 한도까지만(한도를 넘게 자라는 파일도 메모리를 다 쓰지 못한다)
        if !file.metadata()?.is_file() {
            return Err(PortError::new(PortErrorKind::Invalid, format!("{}: 보통 파일이 아니다", path.display())));
        }
        let mut bytes = Vec::new();
        file.take(JSON_MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > JSON_MAX_BYTES {
            return Err(PortError::new(PortErrorKind::Invalid, format!("{}: {} 바이트를 넘어 읽지 않는다", path.display(), JSON_MAX_BYTES)));
        }
        let body = bytes.strip_prefix(BOM).unwrap_or(&bytes);
        serde_json::from_slice(body)
            .map(Some)
            .map_err(|e| PortError::new(PortErrorKind::Invalid, format!("{}: {e}", path.display())))
    }

    fn write_json(&self, path: &Path, value: &serde_json::Value) -> PortResult<()> {
        let text = to_pretty_json(value).map_err(|e| PortError::new(PortErrorKind::Invalid, e.to_string()))?;
        let temp = temp_sibling(path)?;
        let result = write_temp(&temp, text.as_bytes()).and_then(|()| replace_file(&temp, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(PortError::from)
    }

    fn append_line(&self, path: &Path, line: &str) -> PortResult<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        // 한 번의 write 로 보내야 줄이 섞이지 않는다
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(buf.as_bytes())?;
        Ok(())
    }

    fn remove(&self, path: &Path) -> PortResult<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, dir: &Path) -> PortResult<Vec<String>> {
        let entries = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if !is_temp_name(&name) {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    fn make_private_dir(&self, dir: &Path) -> PortResult<()> {
        fs::create_dir_all(dir)?;
        restrict_to_current_user(dir)
    }

    fn try_lock(&self, path: &Path) -> PortResult<Option<Box<dyn Held>>> {
        lock_exclusive(path)
    }

    fn stat(&self, path: &Path) -> PortResult<Option<FileStat>> {
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_secs_f64());
        Ok(Some(FileStat { len: meta.len(), modified }))
    }

    fn read_from(&self, path: &Path, offset: u64, max: usize) -> PortResult<Option<Vec<u8>>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let len = file.metadata()?.len();
        // 끝에서 max 바이트만 읽는다. offset 이 파일보다 뒤면 빈 값이다
        let start = offset.min(len).max(len.saturating_sub(max as u64));
        file.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(Some(buf))
    }
}

fn write_temp(temp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    // 이름을 바꾸기 전에 내용이 디스크에 있어야 정전 뒤에도 빈 파일이 남지 않는다
    file.sync_all()
}

// ── 이름 바꿔 덮기 ───────────────────────────────────

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const TRIES: u32 = 40;
    const WAIT: Duration = Duration::from_millis(5);

    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(std::iter::once(0)).collect() };
    let (src, dst) = (wide(from), wide(to));
    let mut last = std::io::Error::from(std::io::ErrorKind::Other);
    for _ in 0..TRIES {
        // SAFETY: 두 인자 모두 NUL 로 끝나는 UTF-16 버퍼이고 호출 동안 살아 있다
        let ok = unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
        if ok != 0 {
            return Ok(());
        }
        last = std::io::Error::last_os_error();
        // 읽는 쪽이 대상을 잠깐 쥐었거나 백신이 훑는 중이다. 다른 오류는 기다려도 안 풀린다
        if !matches!(last.raw_os_error(), Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)) {
            break;
        }
        std::thread::sleep(WAIT);
    }
    Err(last)
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

// ── 개인 폴더 ────────────────────────────────────────

#[cfg(windows)]
fn restrict_to_current_user(dir: &Path) -> PortResult<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let name = std::env::var("USERNAME")
        .ok()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| PortError::new(PortErrorKind::Unavailable, "USERNAME 환경변수가 없다"))?;
    // 도메인·PC 이름을 붙이면 같은 이름의 다른 계정과 헷갈리지 않는다
    let account = match std::env::var("USERDOMAIN") {
        Ok(d) if !d.is_empty() => format!("{d}\\{name}"),
        _ => name,
    };
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let icacls = Path::new(&system_root).join("System32").join("icacls.exe");

    let out = Command::new(icacls)
        .arg(dir)
        // 상속을 끈다(끄면서 상속분은 지운다) → 이미 붙어 있던 흔한 그룹 허용을 걷는다 → 지금 사용자에게만 준다
        .args(["/inheritance:r", "/remove:g", "*S-1-1-0", "*S-1-5-32-545", "*S-1-5-11", "/grant:r"])
        .arg(format!("{account}:(OI)(CI)F"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Err(PortError::new(PortErrorKind::Denied, format!("icacls 실패({}): {}", dir.display(), text.trim())))
}

#[cfg(not(windows))]
fn restrict_to_current_user(dir: &Path) -> PortResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

// ── 배타 잠금 ────────────────────────────────────────

#[cfg(windows)]
struct FileLock {
    file: fs::File,
}

#[cfg(windows)]
impl Held for FileLock {}

#[cfg(windows)]
impl Drop for FileLock {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
        // 핸들을 닫아도 풀리지만 시점이 OS 몫이라, 풀린 뒤 곧바로 다시 잡는 쪽을 위해 명시한다
        let mut overlapped = lock_region();
        // SAFETY: 핸들은 self.file 이 쥐고 있고 overlapped 는 잠글 때와 같은 자리다
        unsafe { UnlockFileEx(self.file.as_raw_handle() as _, 0, 1, 0, &mut overlapped) };
    }
}

/// 잠그는 1바이트의 자리(2^40). 내용 바깥을 잠가야 한다 - Windows 잠금은 읽기도 막아서,
/// 앞 바이트를 잠그면 잠금 파일에 적어 둔 내용(pid 등)을 남이 못 읽는다.
#[cfg(windows)]
const LOCK_OFFSET_HIGH: u32 = 0x100;

#[cfg(windows)]
fn lock_region() -> windows_sys::Win32::System::IO::OVERLAPPED {
    // SAFETY: OVERLAPPED 는 0 으로 채워도 유효한 평범한 구조체다
    let mut overlapped: windows_sys::Win32::System::IO::OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.Anonymous.Anonymous.OffsetHigh = LOCK_OFFSET_HIGH;
    overlapped
}

#[cfg(windows)]
fn lock_exclusive(path: &Path) -> PortResult<Option<Box<dyn Held>>> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};

    const ERROR_LOCK_VIOLATION: i32 = 33;

    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    let mut overlapped = lock_region();
    // SAFETY: 핸들은 file 이 쥐고 있고 overlapped 는 `lock_region` 이 채운 자리의 1바이트를 건다
    let ok = unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    };
    if ok != 0 {
        return Ok(Some(Box::new(FileLock { file })));
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
        return Ok(None);
    }
    Err(err.into())
}

#[cfg(not(windows))]
fn lock_exclusive(_path: &Path) -> PortResult<Option<Box<dyn Held>>> {
    Err(PortError::new(PortErrorKind::Unavailable, "배타 잠금은 Windows 에서만 구현했다"))
}

// ── 시각 ─────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs_f64(),
            // 시계가 1970 이전이면 음수 초로 그대로 알린다
            Err(e) => -e.duration().as_secs_f64(),
        }
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}
