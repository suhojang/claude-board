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
const RELEASES_URL: &str = "https://api.github.com/repos/suhojang/claude-board/releases";
/// 한 쪽에 받는 릴리스 수(GitHub 최대) — macOS 판 릴리스가 앞에 많이 쌓여도 Windows 릴리스가 첫 쪽 밖으로 밀리지 않게 넉넉히 받는다.
const PER_PAGE: usize = 100;
/// 이만큼 쪽을 넘겨도 `windows-v` 정식 릴리스가 없으면 없는 것으로 본다(인증 없는 GitHub 요청은 시간당 60번이라 상한을 둔다).
const MAX_PAGES: usize = 5;
const TAG_PREFIX: &str = "windows-v";

/// 시험용 주소 바꿔치기 — `CLAUDE_BOARD_UPDATE_RELEASES_URL`(릴리스 목록 주소)·`CLAUDE_BOARD_UPDATE_RAW_BASE`(install.ps1 을 받는 원본 주소 앞부분,
/// 기본 `https://raw.githubusercontent.com/suhojang/claude-board`). `CLAUDE_BOARD_HOME` 도 있을 때만 켜진다 — 홈을 따로 정한 시험 실행에서만 쓰이고,
/// 사용자 보드에 남은 환경변수 하나가 업데이트 주소를 돌리지 못하게 한다(`CLAUDE_BOARD_CLAUDE` 와 같은 규칙).
pub const RELEASES_URL_ENV: &str = "CLAUDE_BOARD_UPDATE_RELEASES_URL";
pub const RAW_BASE_ENV: &str = "CLAUDE_BOARD_UPDATE_RAW_BASE";

/// 바꿔치기 값 — 홈이 따로 정해졌고 값이 비지 않을 때만. 순수 함수라 환경을 건드리지 않고 시험한다.
fn gated(home: Option<&str>, value: Option<String>) -> Option<String> {
    home.filter(|h| !h.is_empty())?;
    value.filter(|v| !v.is_empty())
}

fn override_env(name: &str) -> Option<String> {
    let home = std::env::var(paths::HOME_ENV).ok();
    gated(home.as_deref(), std::env::var(name).ok())
}

/// 릴리스 목록 `page` 쪽 주소 — 시험용 바꿔치기가 있으면 그 주소에 같은 질의를 붙인다.
fn releases_url(page: usize) -> String {
    page_url(&override_env(RELEASES_URL_ENV).unwrap_or_else(|| RELEASES_URL.to_string()), page)
}

fn page_url(base: &str, page: usize) -> String {
    format!("{base}?per_page={PER_PAGE}&page={page}")
}

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

/// 릴리스 목록 한 쪽의 분석 — `found`: draft·prerelease 가 아니고 태그가 `windows-v` 로 시작하는 첫 릴리스의 버전, `items`: 그 쪽의 항목 수.
#[derive(Debug, PartialEq)]
pub struct PageScan {
    pub found: Option<String>,
    pub items: usize,
}

/// 릴리스 목록 한 쪽(JSON 배열)을 읽는다. 배열이 아니면(깨진 JSON·오류 객체·`null`) None.
pub fn scan_page(json: &[u8]) -> Option<PageScan> {
    let Ok(Value::Array(list)) = serde_json::from_slice::<Value>(json) else { return None };
    let found = list.iter().filter_map(Value::as_object).find_map(|r| {
        let flag = |k: &str| r.get(k).and_then(Value::as_bool).unwrap_or(false);
        if flag("draft") || flag("prerelease") {
            return None;
        }
        let v = r.get("tag_name")?.as_str()?.strip_prefix(TAG_PREFIX)?;
        valid_version(v).then(|| v.to_string())
    });
    Some(PageScan { found, items: list.len() })
}

/// 쪽을 넘기며 `windows-v` 정식 릴리스를 찾는다 — 찾으면 거기서 멈추고, 쪽이 덜 찼거나(마지막 쪽) `MAX_PAGES` 에 닿으면 없는 것이다.
/// 받기에 실패하거나(오류·200 아님) 읽을 수 없는 쪽이 나오면 이전 쪽에 있었어도 모르므로 None(확인 실패)이다.
fn find_latest(mut fetch: impl FnMut(usize) -> Result<(u16, Vec<u8>), String>) -> Option<String> {
    for page in 1..=MAX_PAGES {
        let Ok((200, body)) = fetch(page) else { return None };
        let scan = scan_page(&body)?;
        if scan.found.is_some() {
            return scan.found;
        }
        if scan.items < PER_PAGE {
            return None;
        }
    }
    None
}

pub fn is_newer(v: &str, current: &str) -> bool {
    cmp_version(v, current) == std::cmp::Ordering::Greater
}

/// 태그 기준 install.ps1 원본 주소 — 릴리스 자산과 달리 UTF-8 로 내려와 update.log 의 한글이 깨지지 않는다.
pub fn install_script_url(version: &str) -> String {
    script_url(&override_env(RAW_BASE_ENV).unwrap_or_else(default_raw_base), version)
}

fn default_raw_base() -> String {
    format!("https://raw.githubusercontent.com/{REPO}")
}

fn script_url(base: &str, version: &str) -> String {
    format!("{}/{TAG_PREFIX}{version}/windows/packaging/install.ps1", base.trim_end_matches('/'))
}

/// powershell `-Command` 로 넘길 스크립트 — 버전은 숫자·점만 들어가므로(valid_version) 따옴표 안에서 안전하다.
/// 맨 앞에서 콘솔 출력 인코딩을 UTF-8 로 맞춘다. install.ps1 은 끝에서 인코딩을 처음 값으로 되돌리는데, 실패(throw)는 그 뒤에 PowerShell 이 오류 줄로
/// 찍으므로 처음 값이 OEM 코드 페이지(한국어 Windows: CP949)면 update.log(UTF-8)에서 오류 메시지가 깨진다. 처음 값이 UTF-8 이면 되돌려도 UTF-8 이다.
pub fn install_command(version: &str) -> String {
    format!(
        "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; & {{ $s = irm '{}'; & ([scriptblock]::Create($s)) -Version '{}' -Update }}",
        install_script_url(version),
        version
    )
}

/// 확인해야 하는가 — 손으로 누른 확인(단추·트레이 메뉴)은 늘, 자동 확인(켤 때·1시간마다)은 설정 > 업데이트 > 자동 확인을 켜 둔 때만.
fn wants_check(manual: bool, auto_update_check: bool) -> bool {
    manual || auto_update_check
}

/// 켤 때·1시간마다(manual = false), 업데이트 확인 단추·트레이 메뉴(manual = true).
/// 자동 확인은 설정을 끄면(`autoUpdateCheck`) 하지 않는다 — 손으로 누른 확인은 그대로 된다.
pub fn check(board: &Arc<Board>, manual: bool) {
    // 설정 잠금은 업데이트 잠금보다 먼저·따로 잡는다(둘을 겹쳐 잡지 않는다)
    let auto = board.settings.lock().unwrap().auto_update_check;
    if !wants_check(manual, auto) {
        return;
    }
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
        let found = find_latest(|page| http_get(&releases_url(page), &headers, Duration::from_secs(15)));
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

/// 업데이트할 것이 있고 이미 도는 중이 아닌가 — 확인 창을 띄우기 전에 거른다.
fn can_start(u: &Updater) -> bool {
    u.latest.is_some() && !u.running
}

/// 업데이트 실행 — 성공하면 스크립트가 보드를 끄고 새 보드를 띄우므로 돌아오지 않는다. 돌아오면 실패다.
/// 업데이트할 것이 없거나 이미 도는 중이면 확인 창도 띄우지 않는다. 확인에서 남긴 `.paused` 표지는 스크립트가 보드를 끄지 못하고
/// 돌아왔을 때 지운다(세션은 그대로 도는 중이므로 일시중지로 남으면 안 된다, mac Updater.run).
pub fn run(board: &Arc<Board>) {
    if !can_start(&board.updater.lock().unwrap()) {
        return;
    }
    let Some(marks) = crate::shutdown::confirm_update(board) else { return };
    let version = {
        let mut u = board.updater.lock().unwrap();
        // 확인을 기다리는 사이 다른 업데이트가 시작됐을 수 있다
        match u.latest.clone().filter(|_| can_start(&u)) {
            Some(v) => {
                u.running = true;
                u.failed = false;
                v
            }
            None => {
                drop(u);
                crate::shutdown::remove_marks(&marks);
                return;
            }
        }
    };
    board.publish();
    let log = paths::notifier_dir().join("update.log");
    let board = board.clone();
    std::thread::spawn(move || {
        let ok = launch(&log, &version);
        crate::shutdown::remove_marks(&marks);
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

    fn latest_windows(json: &[u8]) -> Option<String> {
        scan_page(json)?.found
    }

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
    fn auto_check_follows_the_setting_but_manual_check_does_not() {
        // (manual, autoUpdateCheck) -> 확인하는가
        assert!(wants_check(false, true));
        assert!(!wants_check(false, false), "끄면 켤 때·1시간마다 확인하지 않는다");
        assert!(wants_check(true, false), "손으로 누른 확인은 꺼 둬도 된다");
        assert!(wants_check(true, true));
    }

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
    fn starts_only_with_something_to_install_and_nothing_running() {
        let mut u = Updater::new();
        assert!(!can_start(&u), "새 버전이 없으면 시작하지 않는다(확인 창도 띄우지 않는다)");
        u.latest = Some("2.13".into());
        assert!(can_start(&u));
        u.running = true;
        assert!(!can_start(&u), "이미 도는 중이면 시작하지 않는다");
    }

    #[test]
    fn install_command_uses_tag_raw_url() {
        let c = install_command("2.13");
        assert!(c.contains("https://raw.githubusercontent.com/suhojang/claude-board/windows-v2.13/windows/packaging/install.ps1"));
        assert!(c.contains("-Version '2.13' -Update"));
        assert!(c.starts_with("[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; "), "오류 줄이 update.log 에서 깨지지 않게 먼저 UTF-8 로");
        assert!(!c.contains("/releases/latest"));
        assert!(!RELEASES_URL.contains("latest"));
    }

    fn page_of(n: usize, windows_at: Option<(usize, &str)>) -> Vec<u8> {
        let items: Vec<String> = (0..n)
            .map(|i| match windows_at {
                Some((at, v)) if at == i => format!(r#"{{"tag_name":"windows-v{v}","draft":false,"prerelease":false}}"#),
                _ => format!(r#"{{"tag_name":"v2.{i}","draft":false,"prerelease":false}}"#),
            })
            .collect();
        format!("[{}]", items.join(",")).into_bytes()
    }

    #[test]
    fn page_urls_ask_for_the_biggest_page() {
        assert_eq!(page_url(RELEASES_URL, 1), "https://api.github.com/repos/suhojang/claude-board/releases?per_page=100&page=1");
        assert_eq!(page_url("http://127.0.0.1:9/releases", 3), "http://127.0.0.1:9/releases?per_page=100&page=3");
    }

    #[test]
    fn scan_page_counts_items_and_rejects_non_arrays() {
        assert_eq!(scan_page(&page_of(3, Some((1, "2.13")))), Some(PageScan { found: Some("2.13".into()), items: 3 }));
        assert_eq!(scan_page(&page_of(2, None)), Some(PageScan { found: None, items: 2 }));
        assert_eq!(scan_page(b"[]"), Some(PageScan { found: None, items: 0 }));
        for bad in [&b""[..], b"null", b"{}", b"<html>", b"[{\"tag_name\":", b"\xEF\xBB\xBF[]"] {
            assert_eq!(scan_page(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
        // 항목이 객체가 아니어도 죽지 않는다
        assert_eq!(scan_page(br#"[null, 5, "x", {"tag_name": 7}]"#), Some(PageScan { found: None, items: 4 }));
    }

    #[test]
    fn finds_a_windows_release_pushed_past_the_first_page() {
        // 앞쪽 두 쪽(200개)이 전부 macOS 판, Windows 릴리스는 세 번째 쪽에 있다
        let mut asked = Vec::new();
        let got = find_latest(|page| {
            asked.push(page);
            Ok((200, if page < 3 { page_of(PER_PAGE, None) } else { page_of(4, Some((1, "3.0"))) }))
        });
        assert_eq!(got.as_deref(), Some("3.0"));
        assert_eq!(asked, [1, 2, 3]);
    }

    #[test]
    fn paging_stops_early_and_never_runs_away() {
        // 찾으면 첫 쪽에서 멈춘다
        let mut asked = 0;
        assert_eq!(find_latest(|_| { asked += 1; Ok((200, page_of(PER_PAGE, Some((0, "2.1"))))) }).as_deref(), Some("2.1"));
        assert_eq!(asked, 1);
        // 덜 찬 쪽은 마지막 쪽이다
        asked = 0;
        assert_eq!(find_latest(|_| { asked += 1; Ok((200, page_of(PER_PAGE - 1, None))) }), None);
        assert_eq!(asked, 1);
        // 계속 가득 차 있으면 MAX_PAGES 에서 그만둔다
        asked = 0;
        assert_eq!(find_latest(|_| { asked += 1; Ok((200, page_of(PER_PAGE, None))) }), None);
        assert_eq!(asked, MAX_PAGES);
    }

    #[test]
    fn a_failed_or_unreadable_page_is_a_failed_check() {
        assert_eq!(find_latest(|_| Err("timeout".into())), None);
        assert_eq!(find_latest(|_| Ok((403, br#"{"message":"API rate limit exceeded"}"#.to_vec()))), None);
        assert_eq!(find_latest(|_| Ok((200, b"<html>".to_vec()))), None);
        // 둘째 쪽이 깨지면 앞 쪽에 없었어도 확인 실패다(없다고 단정하지 않는다)
        let mut page = 0;
        assert_eq!(find_latest(|_| { page += 1; if page == 1 { Ok((200, page_of(PER_PAGE, None))) } else { Ok((500, vec![])) } }), None);
    }

    #[test]
    fn address_override_needs_a_separate_home() {
        let v = || Some("http://127.0.0.1:1/x".to_string());
        assert_eq!(gated(Some("iso-home"), v()).as_deref(), Some("http://127.0.0.1:1/x"));
        assert_eq!(gated(None, v()), None, "홈을 따로 정하지 않은 실행(사용자 보드)에서는 켜지지 않는다");
        assert_eq!(gated(Some(""), v()), None, "빈 홈도 따로 정한 것이 아니다");
        assert_eq!(gated(Some("iso-home"), Some(String::new())), None);
        assert_eq!(gated(Some("iso-home"), None), None);
        assert_eq!(script_url("http://127.0.0.1:1/", "2.13"), "http://127.0.0.1:1/windows-v2.13/windows/packaging/install.ps1");
        assert_eq!(
            script_url(&default_raw_base(), "2.13"),
            format!("https://raw.githubusercontent.com/{REPO}/windows-v2.13/windows/packaging/install.ps1")
        );
    }
}
