//! 여러 모듈이 함께 쓰는 작은 도구 — 시각·파일 수정 시각·파일 끝 읽기·로그·HTTP·분리 실행.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use claude_board_core::{clock, Record};
use serde_json::Value;

/// 유닉스 초를 프론트가 쓰는 유닉스 밀리초로.
pub fn ms(secs: f64) -> i64 {
    (secs * 1000.0).round() as i64
}

/// 파일 수정 시각(유닉스 초). 없으면 None.
pub fn mtime(path: &Path) -> Option<f64> {
    let m = fs::metadata(path).ok()?.modified().ok()?;
    Some(m.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0))
}

/// 캐시 비교용 수정 시각 — 없는 파일끼리도 같다고 본다(Swift `mtime ?? .distantPast`).
pub fn mtime_key(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// 파일 끝 `n` 바이트. 없거나 못 읽으면 빈 값.
pub fn read_tail(path: &Path, n: u64) -> Vec<u8> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > n && f.seek(SeekFrom::Start(len - n)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    buf
}

/// `haystack` 안에 `needle` 이 있는지(바이트).
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// JSON 값을 객체로 — 아니면 None.
pub fn obj(v: Option<&Value>) -> Option<&Record> {
    v.and_then(Value::as_object)
}

/// 숫자 값(정수·소수 모두)을 f64 로.
pub fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

/// `<지역 시각> <내용>` 한 줄을 로그 파일 끝에 붙인다(update.log·auto-switch.log·board.log). 실패는 무시.
pub fn append_log(path: &Path, text: &str) {
    let t = clock::local(clock::now());
    let line = format!(
        "{:04}-{:02}-{:02} {} {text}\n",
        t.year,
        t.month,
        t.day,
        t.hms()
    );
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `notifier/board.log` 에 남긴다 — 전역 단축키 등록 실패처럼 화면에 띄울 곳이 없는 일.
pub fn board_log(text: &str) {
    append_log(&claude_board_core::paths::notifier_dir().join("board.log"), text);
}

/// 개발 실행 확인용 — 환경변수 `CLAUDE_BOARD_TRACE` 가 있을 때만 표준 오류에 한 줄 남긴다.
pub fn trace(text: impl FnOnce() -> String) {
    if std::env::var_os("CLAUDE_BOARD_TRACE").is_some() {
        eprintln!("[claude-board {:.3}] {}", clock::now(), text());
    }
}

/// 짧은 임의 16진수(표지 제목·로그인 자리 이름) — 시각·pid·순번을 섞는다.
pub fn short_id() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let seed = format!("{nanos}-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let hex: String = Sha256::digest(seed.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    hex[..8].to_string()
}

/// HTTP GET — (상태 코드, 본문). OS 의 TLS(native-tls)를 쓴다. 실패하면 Err.
pub fn http_get(url: &str, headers: &[(&str, &str)], timeout: Duration) -> Result<(u16, Vec<u8>), String> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        // 기본값(RootCerts::WebPki)은 OS 저장소를 끄고 ureq 에 든 Mozilla 목록만 믿어, 사슬이 그 목록으로 끝나지 않는 서버(api.anthropic.com)에서 실패한다(native-tls: unable to find any user-specified roots in the final cert chain). OS 저장소를 쓴다
        .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build())
        .build()
        .into();
    let mut req = agent.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_vec().map_err(|e| e.to_string())?;
    Ok((status, body))
}

/// `CREATE_BREAKAWAY_FROM_JOB` (Win32 상수와 같은 값 — 시험이 다른 OS 에서도 돌게 직접 둔다)
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
/// `CREATE_NO_WINDOW`
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `ERROR_ACCESS_DENIED` — 작업 개체가 떼기를 허락하지 않으면 CreateProcess 가 이것으로 실패한다
const ERROR_ACCESS_DENIED: i32 = 5;

/// 분리 실행의 생성 플래그 — 처음은 작업 개체에서 떼어(`breakaway`) 창 없는 새 콘솔로.
/// `DETACHED_PROCESS` 는 쓰지 않는다: `CREATE_NO_WINDOW` 와 함께 주면 `CREATE_NO_WINDOW` 가 무시되어 콘솔이 아예 없고,
/// 그러면 업데이트 스크립트(powershell)가 띄우는 콘솔 프로그램마다 새 콘솔 창이 뜬다. 숨은 콘솔은 그 자식들이 함께 쓴다.
/// 또 보드가 잠깐 세션 콘솔에 붙어 있는 동안 띄워도 그 콘솔을 물려받지 않는다.
#[cfg_attr(not(windows), allow(dead_code))]
fn detached_flags(breakaway: bool) -> u32 {
    if breakaway {
        CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW
    } else {
        CREATE_NO_WINDOW
    }
}

/// 떼기를 빼고 다시 띄울 실패인지 — 작업 개체가 떼기를 거부한 경우만.
#[cfg_attr(not(windows), allow(dead_code))]
fn retry_without_breakaway(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(ERROR_ACCESS_DENIED)
}

/// 보드와 상관없이 살아야 하는 프로세스를 띄운다 — 알림 패널의 `--show`, 터미널 독, 업데이트 스크립트.
/// Windows: 작업 개체에서 떼어 창 없는 콘솔로 띄우고(`detached_flags`), 떼기가 거부되면 그 플래그 없이 다시 띄운다.
pub fn spawn_detached(mut cmd: Command) -> std::io::Result<Child> {
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(detached_flags(true));
        match cmd.spawn() {
            Err(e) if retry_without_breakaway(&e) => {
                cmd.creation_flags(detached_flags(false));
                cmd.spawn()
            }
            r => r,
        }
    }
    #[cfg(not(windows))]
    {
        cmd.spawn()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 보드가 쓰는 두 서버(사용량·업데이트)에 진짜로 닿는지 본다(OS 인증서 저장소). 어떤 상태 코드든 받으면 TLS 가 통과한 것이다 —
    /// 토큰이 없는 사용량 요청은 401 이 와도 성공이다. 네트워크가 필요해 기본으로는 돌리지 않는다: `cargo test -- --ignored https_reaches_a_real_server`
    #[test]
    #[ignore]
    fn https_reaches_a_real_server() {
        let hosts = [
            ("사용량", "https://api.anthropic.com/api/oauth/usage"),
            ("업데이트", "https://api.github.com/repos/suhojang/claude-board/releases?per_page=1"),
        ];
        for (name, url) in hosts {
            let r = http_get(url, &[("User-Agent", "ClaudeBoard-Windows")], Duration::from_secs(10));
            assert!(r.is_ok(), "{name} 서버({url}) TLS 연결: {r:?}");
        }
    }

    #[test]
    fn tail_and_contains() {
        let dir = std::env::temp_dir().join(format!("cb-util-{}", short_id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.txt");
        fs::write(&p, b"0123456789").unwrap();
        assert_eq!(read_tail(&p, 4), b"6789");
        assert_eq!(read_tail(&p, 100), b"0123456789");
        assert!(read_tail(&dir.join("none"), 4).is_empty());
        assert!(contains(b"{\"type\":\"assistant\"}", b"\"assistant\""));
        assert!(!contains(b"user", b"\"assistant\""));
        assert_eq!(ms(1.2345), 1235);
        assert_ne!(short_id(), short_id());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detached_spawn_flags() {
        // DETACHED_PROCESS(0x8) 는 CREATE_NO_WINDOW 를 무시하게 하므로 섞지 않는다
        assert_eq!(detached_flags(true), 0x0100_0000 | 0x0800_0000);
        assert_eq!(detached_flags(false), 0x0800_0000);
        assert_eq!(detached_flags(true) & 0x8, 0);
        assert!(retry_without_breakaway(&std::io::Error::from_raw_os_error(5)));
        assert!(!retry_without_breakaway(&std::io::Error::from_raw_os_error(2)));
        assert!(!retry_without_breakaway(&std::io::Error::other("x")));
    }
}
