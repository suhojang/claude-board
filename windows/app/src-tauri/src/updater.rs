//! 업데이트 — `Updater` (`ClaudeBoard.swift Updater`, SPEC §5.8).
//!
//! **`/releases/latest` 를 쓰지 않는다** — 그 자리는 macOS 판 릴리스가 쓴다. 릴리스 목록 API 에서
//! `windows-v` 로 시작하는 첫 정식 릴리스를 고른다. 업데이트는 그 태그의 install.ps1 을 받아 창 없이 분리 실행한다.

use std::fs::{File, OpenOptions};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use claude_board_core::{clock, paths};
use serde_json::Value;

use crate::board::Board;
use crate::model::{UpdaterNotice, UpdaterView};
use crate::terminal::cmp_version;
use crate::util::{append_log, http_get};

pub const REPO: &str = "suhojang/claude-board";
pub const CHANGELOG_URL: &str = "https://github.com/suhojang/claude-board/blob/main/windows/CHANGELOG.md";
const RELEASES_URL: &str = "https://api.github.com/repos/suhojang/claude-board/releases?per_page=30";
const TAG_PREFIX: &str = "windows-v";

/// 이 빌드의 버전 — windows/VERSION(빌드 때 build.rs 가 넣는다).
pub const VERSION: &str = env!("CLAUDE_BOARD_VERSION");

pub struct Updater {
    pub current: String,
    pub dev: bool,
    pub latest: Option<String>,
    pub running: bool,
    pub failed: bool,
    pub checking: bool,
    notice: Option<(UpdaterNotice, f64)>,
}

/// 개발 빌드 — VERSION 을 못 읽었거나, 디버그 빌드거나, `CLAUDE_BOARD_DEV` 가 있으면. 업데이트를 확인하지 않는다
/// (배포판으로 덮어쓰지 않게).
pub fn is_dev() -> bool {
    VERSION.is_empty() || cfg!(debug_assertions) || std::env::var_os("CLAUDE_BOARD_DEV").is_some_and(|v| !v.is_empty())
}

impl Updater {
    pub fn new() -> Self {
        Updater { current: VERSION.to_string(), dev: is_dev(), latest: None, running: false, failed: false, checking: false, notice: None }
    }

    fn show(&mut self, kind: &'static str, text: String) {
        self.notice = Some((UpdaterNotice { kind, text }, clock::now() + 4.0));
    }

    pub fn view(&self, now: f64) -> UpdaterView {
        UpdaterView {
            current: self.current.clone(),
            dev: self.dev,
            latest: self.latest.clone(),
            running: self.running,
            failed: self.failed,
            checking: self.checking,
            notice: self.notice.as_ref().filter(|(_, until)| *until > now).map(|(n, _)| n.clone()),
        }
    }
}

/// 버전 문자열 모양 — `^[0-9]+(\.[0-9]+)*$`
fn valid_version(v: &str) -> bool {
    !v.is_empty() && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// 릴리스 목록에서 draft·prerelease 가 아니고 태그가 `windows-v` 로 시작하는 첫 릴리스의 버전.
pub fn latest_windows(json: &[u8]) -> Option<String> {
    let Ok(Value::Array(list)) = serde_json::from_slice::<Value>(json) else { return None };
    list.iter().filter_map(Value::as_object).find_map(|r| {
        let flag = |k: &str| r.get(k).and_then(Value::as_bool).unwrap_or(false);
        if flag("draft") || flag("prerelease") {
            return None;
        }
        let v = r.get("tag_name")?.as_str()?.strip_prefix(TAG_PREFIX)?;
        valid_version(v).then(|| v.to_string())
    })
}

pub fn is_newer(v: &str, current: &str) -> bool {
    cmp_version(v, current) == std::cmp::Ordering::Greater
}

/// 태그 기준 install.ps1 원본 주소 — 릴리스 자산과 달리 UTF-8 로 내려와 update.log 의 한글이 깨지지 않는다.
pub fn install_script_url(version: &str) -> String {
    format!("https://raw.githubusercontent.com/{REPO}/{TAG_PREFIX}{version}/windows/packaging/install.ps1")
}

/// powershell `-Command` 로 넘길 스크립트 — 버전은 숫자·점만 들어가므로(valid_version) 따옴표 안에서 안전하다.
pub fn install_command(version: &str) -> String {
    format!(
        "& {{ $s = irm '{}'; & ([scriptblock]::Create($s)) -Version '{}' -Update }}",
        install_script_url(version),
        version
    )
}

/// 켤 때·1시간마다(manual = false), 업데이트 확인 단추·트레이 메뉴(manual = true).
pub fn check(board: &Arc<Board>, manual: bool) {
    {
        let mut u = board.updater.lock().unwrap();
        if manual {
            if u.checking {
                return;
            }
            if u.dev {
                let text = format!("개발용 빌드({})는 업데이트를 확인하지 않습니다", if u.current.is_empty() { "dev" } else { &u.current });
                u.show("dev", text);
                drop(u);
                board.publish();
                return;
            }
            u.checking = true;
        } else if u.dev {
            return;
        }
    }
    board.publish();
    let board = board.clone();
    std::thread::spawn(move || {
        let headers = [("User-Agent", "ClaudeBoard-Windows"), ("Accept", "application/vnd.github+json")];
        let found = match http_get(RELEASES_URL, &headers, Duration::from_secs(15)) {
            Ok((200, body)) => latest_windows(&body),
            _ => None,
        };
        {
            let mut u = board.updater.lock().unwrap();
            if let Some(v) = &found {
                u.latest = is_newer(v, &u.current).then(|| v.clone());
            }
            if manual {
                u.checking = false;
                if found.is_none() {
                    u.show("warn", "업데이트를 확인하지 못했습니다".into());
                } else if u.latest.is_none() {
                    let text = format!("최신 버전입니다 ({})", u.current);
                    u.show("ok", text);
                }
            }
        }
        board.publish();
    });
}

/// 업데이트 실행 — 성공하면 스크립트가 보드를 끄고 새 보드를 띄우므로 돌아오지 않는다. 돌아오면 실패다.
pub fn run(board: &Arc<Board>) {
    let version = {
        let mut u = board.updater.lock().unwrap();
        let Some(v) = u.latest.clone().filter(|_| !u.running) else { return };
        u.running = true;
        u.failed = false;
        v
    };
    board.publish();
    let log = paths::notifier_dir().join("update.log");
    let board = board.clone();
    std::thread::spawn(move || {
        let ok = launch(&log, &version);
        let mut u = board.updater.lock().unwrap();
        u.running = false;
        u.failed = !ok;
        drop(u);
        board.publish();
    });
}

/// 스크립트를 창 없이 분리 실행하고(보드가 꺼져도 살아 있게) 끝날 때까지 기다린다.
/// 성공하면 스크립트가 이 보드를 끄므로 여기로 돌아왔다면 실패다(SPEC §5.8) — 늘 false.
fn launch(log: &std::path::Path, version: &str) -> bool {
    let _ = std::fs::create_dir_all(log.parent().unwrap_or(log));
    if File::create(log).is_err() {
        return false;
    }
    append_log(log, &format!("Claude Board {version} 로 업데이트 — {}", install_script_url(version)));
    if !cfg!(windows) {
        append_log(log, "Windows 에서만 업데이트할 수 있습니다");
        return false;
    }
    // 스크립트 출력은 파일 끝에 이어 쓴다 — 도중에 이 보드가 꺼지므로 파이프로 받으면 쓰다가 끊긴다
    let open = || OpenOptions::new().append(true).open(log);
    let (Ok(out), Ok(err)) = (open(), open()) else { return false };
    let mut cmd = Command::new("powershell.exe");
    // PowerShell 7 에서 띄운 보드(install.ps1·claude-board 명령)는 PowerShell 7 의 PSModulePath 를 물려받는다.
    // 그대로 넘기면 Windows PowerShell 5.1 이 PowerShell 7 모듈을 읽으려다 Get-FileHash 같은 명령을 찾지 못하므로 지운다(5.1 이 기본값을 쓴다)
    cmd.env_remove("PSModulePath")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &install_command(version)])
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    match crate::util::spawn_detached(cmd) {
        Ok(mut child) => {
            let status = child.wait();
            append_log(log, &format!("업데이트 스크립트가 끝났지만 보드가 그대로입니다 ({status:?})"));
            false
        }
        Err(e) => {
            append_log(log, &format!("powershell 을 띄우지 못했습니다: {e}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASES: &str = r#"[
      {"tag_name": "windows-v2.14", "draft": true, "prerelease": false},
      {"tag_name": "windows-v2.13.1-rc1", "draft": false, "prerelease": true},
      {"tag_name": "v2.13", "draft": false, "prerelease": false, "name": "macOS 2.13"},
      {"tag_name": "windows-vnext", "draft": false, "prerelease": false},
      {"tag_name": "windows-v2.13", "draft": false, "prerelease": false},
      {"tag_name": "v2.12", "draft": false, "prerelease": false},
      {"tag_name": "windows-v2.12", "draft": false, "prerelease": false}
    ]"#;

    #[test]
    fn picks_first_windows_release() {
        assert_eq!(latest_windows(RELEASES.as_bytes()).as_deref(), Some("2.13"));
        // macOS 릴리스만 있으면 없음
        assert_eq!(latest_windows(br#"[{"tag_name": "v2.12"}, {"tag_name": "v2.11"}]"#), None);
        assert_eq!(latest_windows(b"[]"), None);
        assert_eq!(latest_windows(br#"{"message": "API rate limit exceeded"}"#), None);
        assert!(is_newer("2.13", "2.12") && is_newer("2.12.1", "2.12") && is_newer("2.100", "2.12"));
        assert!(!is_newer("2.12", "2.12") && !is_newer("2.9", "2.12"));
    }

    #[test]
    fn install_command_uses_tag_raw_url() {
        let c = install_command("2.13");
        assert!(c.contains("https://raw.githubusercontent.com/suhojang/claude-board/windows-v2.13/windows/packaging/install.ps1"));
        assert!(c.contains("-Version '2.13' -Update"));
        assert!(!c.contains("/releases/latest"));
        assert!(!RELEASES_URL.contains("latest"));
    }
}
