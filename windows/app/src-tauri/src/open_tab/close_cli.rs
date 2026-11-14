//! `ClaudeBoard --close-tab` 명령줄 길 — 닫기 규칙(`close.rs`)의 요청 꼴로 닫기 폴더에 요청 파일을 놓고 답을 기다린다.
//! 원본(macOS): `Sources/ClaudeBoard/OpenTab.swift` 262~296 줄(`closeTabCommandLine` · `closeTabDir` ·
//! `prepareRequestDir` · `writeRequest` · `awaitReply`). `main.rs` 가 첫 인자 `--close-tab` 으로 이 길을 부른다.
//! 답 한 줄 JSON 을 표준 출력에 찍고 ok 면 0, 아니면 1 로 끝난다 — 부른 쪽(셸·자율주행 모델 세션)이 본다.
//! 여는 길(`cli.rs`)과 닮았지만 갈래가 다르다 — 닫으려고 보드를 켜지지 않는다(꺼져 있으면 곧바로 board-off,
//! 요청 폴더도 만들지 않는다 — 원본 OpenTab.swift:276), 깨움도 없다(보드가 1초 틱에 닫기 폴더를 스쳐 읽는다).
//! 답 파일은 여는 길과 같은 `OpenTabReply` 꼴로 읽는다 — 원본이 닫기에도 그 꼴을 재활용한다.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use claude_board_core::clock;

use super::args::{OpenTabFailure, OpenTabReply};
use super::close::{parse_close_tab_args, CloseTabRequest};

/// 답 파일을 검사하는 주기 — 보드가 요청 폴더를 훑는 주기와 맞추듯 자주 본다(원본 `usleep(100_000)`, `cli.rs` 와 같다)
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 닫기 요청 폴더 — 여는 폴더(`args::open_tab_dir`) 옆의 close-tab. 설치판·시험 빌드가 따로다.
/// 열기 처리부는 폴더의 `*.req.json` 을 모두 열기 요청으로 읽으므로 폴더를 섞으면 닫기 요청이 먹혀 탭이 열린다
pub fn close_tab_request_dir(local_app_data: &Path, dev: bool) -> PathBuf {
    let board_dir = if dev { "claude-board-dev" } else { "claude-board" };
    local_app_data.join(board_dir).join("close-tab")
}

/// `--close-tab` 뒤의 인자로 탭을 닫는다. `args` 는 `--close-tab` 뒤의 인자, `dir` 은 닫기 요청 폴더
/// (`close_tab_request_dir`), 보드가 떠 있는지 보는 부분은 진짜 어댑터가 넣는다: `board_running`,
/// 시계는 `now`(1970 기준 초). 돌려주는 짝은 (답 한 줄 JSON, 끝 코드 — ok 면 0 아니면 1) 다.
pub fn run_close_tab(
    args: &[String],
    dir: &Path,
    board_running: &dyn Fn() -> bool,
    now: &dyn Fn() -> f64,
) -> (String, i32) {
    let options = match parse_close_tab_args(args) {
        Ok(options) => options,
        Err(failure) => {
            let reply = failure.reply();
            return reply_exit(reply.ok, reply.line());
        }
    };
    // 닫으려고 보드를 켜지지 않는다 — 꺼져 있으면 요청 폴더도 만들지 않고 곧바로 거절한다(원본 OpenTab.swift:276)
    if !board_running() {
        let reply = OpenTabFailure::BoardOff("보드가 꺼져 있습니다(닫으려고 보드를 켜지 않습니다)".to_string()).reply();
        return reply_exit(reply.ok, reply.line());
    }
    if let Err(error) = std::fs::create_dir_all(dir) {
        let why = format!("요청 폴더를 만들지 못했습니다: {} {error}", dir.display());
        let reply = OpenTabFailure::Refused(why).reply();
        return reply_exit(reply.ok, reply.line());
    }
    let id = new_request_id();
    let request = CloseTabRequest::new(id.as_str(), &options.target, options.stop, now());
    let body = serde_json::to_string(&request).expect("요청 꼴은 언제나 JSON 이 된다");
    if !write_request_once(dir, &id, &body) {
        let why = format!("요청을 쓰지 못했습니다: {}", dir.display());
        let reply = OpenTabFailure::Refused(why).reply();
        return reply_exit(reply.ok, reply.line());
    }
    let reply = await_reply(dir, &id, options.timeout, now);
    reply_exit(reply.ok, reply.line())
}

/// 답을 한 줄로, 끝 코드는 ok 면 0 아니면 1(원본 finish: OpenTab.swift:17-20). 완성된 한 줄을 받는다 —
/// 인자 읽기의 실패 답(`CloseTabReply`)과 요청 답(`OpenTabReply`)이 꼴이 다르기 때문이다
fn reply_exit(ok: bool, line: String) -> (String, i32) {
    (line, i32::from(!ok))
}

/// 새 요청 id(UUID 꼴) — uuid 의존성을 더하지 않고 시각·프로세스·차례를 섞어 해시로 만든다(handle.rs 의
/// new_session_id 와 같다). mac 은 `UUID().uuidString.lowercased()` 로 만든다(OpenTab.swift). 열기(`cli.rs`)의
/// `short_id` 8글자를 쓰지 않는다 — 보드 쪽이 요청 파일 이름을 `is_session_id` 로 걸러 읽기 때문이다
pub(super) fn new_request_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(clock::now().to_bits().to_be_bytes());
    hasher.update(std::process::id().to_be_bytes());
    hasher.update(seq.to_be_bytes());
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// 요청을 `<id>.tmp` 에 새로 쓰고(`<id>.tmp` 가 있으면 실패 — 새 파일로만 쓴다) `<id>.req.json` 으로 바꾼다.
/// 보드가 쓰다 만 요청을 읽지 않게(원본 writeRequest: OpenTab.swift:280-288). 성공으로 바꿨으면 참
fn write_request_once(dir: &Path, id: &str, body: &str) -> bool {
    let tmp_path = dir.join(format!("{id}.tmp"));
    let req_path = dir.join(format!("{id}.req.json"));
    if !write_new_alone(&tmp_path, body) {
        return false;
    }
    match std::fs::rename(&tmp_path, &req_path) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&tmp_path);
            false
        }
    }
}

/// 새 파일로만 쓴다(있으면 실패, 덮어쓰지 않는다 — 원본 writeNew 는 O_EXCL). 임시 파일은 어떤 경우에도 남기지 않는다
fn write_new_alone(path: &Path, body: &str) -> bool {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(body.as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// 답을 기다린다 — 여는 길과 다르게 깨움이 없다: 보드가 1초 틱에 닫기 폴더를 스쳐 읽는다. 시간 안에 답이 없으면
/// 보드가 아직 가져가지 않은 요청을 거둬 나중에 닫히지 않게 한다. `now` 는 한계를 잡을 때와 루프마다 딱 한 번
/// 불린다 — 가짜 시계(시험)가 흘러가는 자리를 정해 두기 위함이다
fn await_reply(dir: &Path, id: &str, timeout_secs: f64, now: &dyn Fn() -> f64) -> OpenTabReply {
    let reply_path = dir.join(format!("{id}.reply.json"));
    let req_path = dir.join(format!("{id}.req.json"));
    let deadline = now() + timeout_secs;
    loop {
        let now_secs = now();
        if now_secs >= deadline {
            break;
        }
        if let Some(reply) = take_reply(&reply_path) {
            let _ = std::fs::remove_file(&reply_path);
            return reply;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    timeout_reply(&req_path, timeout_secs)
}

/// 답 파일을 읽어 답 꼴로 — 없거나 꼴이 틀리면 아직 없는 것으로 본다(보드가 쓰는 중일 수 있다 — 원본 try? → nil)
fn take_reply(reply_path: &Path) -> Option<OpenTabReply> {
    let body = std::fs::read_to_string(reply_path).ok()?;
    serde_json::from_str(&body).ok()
}

/// 시간 안에 답이 없다 — 요청 파일이 남아 있으면(보드가 아직 안 가져갔다) 거둔다. 거둔 성패로 why 를 나눈다:
/// 가져갔는데 답이 늦은 건 탭이 닫혔을 수 있다(원본 OpenTab.swift:283, `cli.rs` timeout_reply 의 taken 갈래)
fn timeout_reply(req_path: &Path, timeout_secs: f64) -> OpenTabReply {
    let seconds = timeout_secs as i64;
    let why = if std::fs::remove_file(req_path).is_ok() {
        format!("보드가 {seconds}초 안에 요청을 가져가지 않았습니다(요청을 거뒀습니다)")
    } else {
        format!("보드가 요청을 가져갔지만 {seconds}초 안에 답하지 않았습니다(탭이 닫혔을 수 있습니다)")
    };
    OpenTabFailure::Timeout(why).reply()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;

    use super::super::close::{is_session_id, CloseTabReply};
    use crate::util::short_id;

    use super::*;

    /// 원본 CloseTabTests 의 자리표
    const SID: &str = "0f0e1d2c-3b4a-5968-7766-554433221100";
    const SID2: &str = "11111111-2222-3333-4444-555555555555";

    /// 시험용 닫기 요청 폴더 — 부를 때마다 이름이 겹치지 않는다(short_id 는 시각·pid·순번을 섞는다)
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-close-tab-{tag}-{}", short_id()));
        std::fs::create_dir_all(&dir).expect("임시 요청 폴더");
        dir
    }

    fn svec(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    /// 보드가 놓을 답 꼴 — 답 몸통은 답 한 줄 JSON 그대로
    fn reply(ok: bool, status: &str, tab: Option<&str>, session: Option<&str>, why: Option<&str>) -> CloseTabReply {
        CloseTabReply {
            ok,
            pid: None,
            session: session.map(Into::into),
            status: status.to_string(),
            tab: tab.map(Into::into),
            why: why.map(Into::into),
        }
    }

    /// 가짜 시계 — 부르는 곳의 Cell 을 흘려 논리 시각(벽시계와 무관)을 만든다
    fn ticking_clock(clock: &Cell<f64>, step: f64) -> impl Fn() -> f64 + '_ {
        move || {
            let current = clock.get();
            clock.set(current + step);
            current
        }
    }

    /// 요청 파일을 찾는다 — 요청 id 는 임의라 폴더를 스쳐 찾는다
    fn first_request_file(dir: &Path) -> Option<PathBuf> {
        std::fs::read_dir(dir).ok()?.find_map(|entry| {
            let path = entry.ok()?.path();
            let is_request = path.file_name()?.to_string_lossy().ends_with(".req.json");
            is_request.then_some(path)
        })
    }

    fn request_id_of(dir: &Path) -> String {
        let path = first_request_file(dir).expect("요청 파일");
        let name = path.file_name().expect("파일 이름").to_string_lossy().into_owned();
        name.strip_suffix(".req.json").expect("요청 파일 이름 꼴").to_string()
    }

    fn read_request(dir: &Path) -> CloseTabRequest {
        let path = first_request_file(dir).expect("요청 파일");
        let body = std::fs::read_to_string(path).expect("요청 파일 읽기");
        serde_json::from_str(&body).expect("요청 꼴")
    }

    /// 폴더에 남은 파일 수 — 답을 읽은 뒤에는 요청 하나만, 거둔 뒤에는 아무것도 남지 않아야 한다
    fn files_left_in(dir: &Path) -> usize {
        std::fs::read_dir(dir).expect("요청 폴더").count()
    }

    /// 보드를 흉내 내는 시계 하나 — 틱 뒤에 아직 읽히지 않은 닫기 요청 파일이 있으면 곁에 답 파일을 한 번
    /// 놓는다. 닫기 길에는 깨움 훅이 없어 여는 길(`cli.rs`)의 wake 흉내를 시계로 대신한다: 요청 at(첫 부름) →
    /// 한계 잡기(둘째 부름) 사이에 요청이 생기므로 둘째 부름에 시사해 첫 루프 검사(셋째 부름) 앞에 답이 있다
    fn answering_clock<'a>(dir: &'a Path, step: f64, body: &'a str) -> impl Fn() -> f64 + 'a {
        let clock = Cell::new(1000.0_f64);
        let served = Cell::new(false);
        move || {
            let current = clock.get();
            clock.set(current + step);
            if !served.get() && first_request_file(dir).is_some() {
                served.set(true);
                let id = request_id_of(dir);
                std::fs::write(dir.join(format!("{id}.reply.json")), body).expect("답 파일");
            }
            current
        }
    }

    /// 보드가 요청을 읽어 가는 것을 흉내 내는 시계 — 틱 뒤에 요청 파일이 있으면 지운다(답은 놓지 않는다)
    fn taking_clock(dir: &Path, step: f64) -> impl Fn() -> f64 + '_ {
        let clock = Cell::new(1000.0_f64);
        move || {
            let current = clock.get();
            clock.set(current + step);
            if let Some(request) = first_request_file(dir) {
                let _ = std::fs::remove_file(request);
            }
            current
        }
    }

    #[test]
    fn pre_written_replies_serve_for_all_statuses() {
        // closed·not-found·busy·refused — 보드가 첫 답 검사 전에 답 파일을 놓으면 CLI 는 곧바로 읽어 끝낸다.
        // 요청 id 는 임의라 답 파일을 미리 쓰는 방법은 시계 클로저다. 답 파일은 읽은 뒤 지워 요청 하나만 남는다
        type Row = (&'static str, Vec<String>, CloseTabReply, i32, Option<&'static str>, Option<&'static str>, bool);
        let rows: Vec<Row> = vec![
            ("closed", svec(&["--title", "역할/2"]),
                reply(true, "closed", Some("역할/2"), Some(SID), None), 0, None, Some("역할/2"), false),
            ("not-found", svec(&["--session", SID]),
                reply(false, "not-found", None, None, Some("이 세션의 보드 탭이 없습니다")), 1, Some(SID), None, false),
            ("busy", svec(&["--title", "역할/2", "--stop"]),
                reply(false, "busy", None, None, Some("claude 가 돌고 있습니다")), 1, None, Some("역할/2"), true),
            ("refused", svec(&["--session", SID2]),
                reply(false, "refused", None, None, Some("여러 탭이 맞습니다")), 1, Some(SID2), None, false),
        ];
        for (note, args, want, exit, session, title, stop) in rows {
            let dir = temp_dir(note);
            let body = want.line();
            let now = answering_clock(&dir, 0.1, &body);
            let (line, exit_code) = run_close_tab(&args, &dir, &|| true, &now);
            assert_eq!(exit_code, exit, "{note}: 끝 코드");
            assert_eq!(line, body, "{note}: 답 한 줄은 그대로");
            assert_eq!(files_left_in(&dir), 1, "{note}: 답 파일은 읽은 뒤 지우고, 임시 파일도 없고, 요청만 남는다");
            let request = read_request(&dir);
            let id = request_id_of(&dir);
            assert_eq!(request.id, id, "{note}: 요청 id 는 파일 이름과 같다");
            assert!(is_session_id(&request.id), "{note}: 요청 id 는 세션 id 꼴이다 — 보드가 파일 이름을 걸러 읽는다");
            assert_eq!(request.session.as_deref(), session, "{note}: 요청의 세션");
            assert_eq!(request.title.as_deref(), title, "{note}: 요청의 탭 이름");
            assert_eq!(request.stop, stop, "{note}: 요청의 끝내기");
            assert_eq!(request.at, 1000.0, "{note}: 만든 시각은 첫 시계 값");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn bad_args_keeps_nothing() {
        // 대상 뒤에 낱말 인자(모르는 옵션)가 있다 — 요청 폴더를 만들지도 않고 끝난다
        let dir = std::env::temp_dir().join(format!("cb-close-tab-badargs-{}", short_id()));
        let (line, exit) = run_close_tab(&svec(&["--title", "역할", "b"]), &dir, &|| true, &|| 1000.0);
        assert_eq!(exit, 1, "끝 코드");
        assert!(
            line.starts_with(r#"{"ok":false,"status":"bad-args","why":"모르는 옵션입니다: b"#),
            "나쁜 인자 답 한 줄: {line}"
        );
        assert!(line.contains("쓰는 법:"), "쓰는 법 안내가 붙는다: {line}");
        assert!(!dir.exists(), "요청 폴더도 만들지 않는다");
    }

    #[test]
    fn board_off_writes_nothing() {
        // 보드가 꺼져 있으면 닫으려고 켜지지 않는다 — 요청 폴더도 만들지 않고 곧바로 board-off 로 끝난다(원본 OpenTab.swift:276)
        let dir = std::env::temp_dir().join(format!("cb-close-tab-boardoff-{}", short_id()));
        let (line, exit) = run_close_tab(&svec(&["--session", SID]), &dir, &|| false, &|| 1000.0);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"board-off","why":"보드가 꺼져 있습니다(닫으려고 보드를 켜지 않습니다)"}"#,
            "보드 꺼짐 답 한 줄"
        );
        assert!(!dir.exists(), "요청 폴더도 만들지 않는다");
    }

    #[test]
    fn timeout_takes_back_the_request() {
        // --timeout 1 · 시계는 0.3초씩 — 벽시계로는 답 검사 세 번(0.3초) 안에 시간이 다 한다.
        // close.rs 가 --timeout 은 1~600 초만 받으므로 논리 시간만 흘려 보낸다
        let dir = temp_dir("timeout-withdrawn");
        let clock = Cell::new(1000.0_f64);
        let now = ticking_clock(&clock, 0.3);
        let (line, exit) = run_close_tab(&svec(&["--title", "역할/2", "--timeout", "1"]), &dir, &|| true, &now);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"timeout","why":"보드가 1초 안에 요청을 가져가지 않았습니다(요청을 거뒀습니다)"}"#,
            "거둔 답 한 줄"
        );
        assert_eq!(files_left_in(&dir), 0, "요청을 거둔다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_after_board_took_request() {
        // 보드가 요청을 읽어 지웠지만 답하지 않았다 — 탭이 닫혔을 수 있다고 말한다(원본 taken 갈래)
        let dir = temp_dir("timeout-taken");
        let now = taking_clock(&dir, 0.3);
        let (line, exit) = run_close_tab(&svec(&["--title", "역할/2", "--timeout", "1"]), &dir, &|| true, &now);
        assert_eq!(exit, 1, "끝 코드");
        assert_eq!(
            line,
            r#"{"ok":false,"status":"timeout","why":"보드가 요청을 가져갔지만 1초 안에 답하지 않았습니다(탭이 닫혔을 수 있습니다)"}"#,
            "가져간 뒤 늦은 답 한 줄"
        );
        assert_eq!(files_left_in(&dir), 0, "남은 것도 없다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refused_when_request_folder_cannot_be_made() {
        // 폴더 자리가 파일이면 요청 폴더를 못 만든다 — 요청 없이 refused 면서 끝난다
        let base = temp_dir("refused-parent");
        let occupying_file = base.join("file");
        std::fs::write(&occupying_file, "폴더가 아니라 파일이다").expect("가림 파일");
        let dir = occupying_file.join("close-tab");
        let (line, exit) = run_close_tab(&svec(&["--title", "역할/2"]), &dir, &|| true, &|| 1000.0);
        assert_eq!(exit, 1, "끝 코드");
        assert!(
            line.starts_with(r#"{"ok":false,"status":"refused","why":"요청 폴더를 만들지 못했습니다: "#),
            "거절 답 한 줄: {line}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn request_id_is_session_id_shaped_and_unique() {
        // 보드 쪽은 요청 파일 이름을 is_session_id 로 걸러 읽는다(handle.rs read_requests) — 짧은 id 는 버려진다.
        // 두 요청의 id 가 겹치면 앞 요청의 답을 뒤 요청이 기다리므로 겹치지도 않아야 한다
        let first = new_request_id();
        let second = new_request_id();
        assert!(is_session_id(&first), "첫 id 꼴: {first}");
        assert!(is_session_id(&second), "둘째 id 꼴: {second}");
        assert_ne!(first, second, "이어지는 두 id 는 다르다");
    }

    #[test]
    fn close_tab_request_dir_sits_beside_open_tab() {
        // 닫기 폴더는 여는 폴더 옆의 close-tab 이다 — 열기 처리부는 폴더의 *.req.json 을 모두 열기 요청으로
        // 읽으므로(`open_tab::handle`) 한 폴더를 섞으면 열기 요청과 닫기 요청이 서로 먹는다
        assert_eq!(
            close_tab_request_dir(Path::new("C:\\Local\\AppData"), false),
            PathBuf::from("C:\\Local\\AppData\\claude-board\\close-tab")
        );
        assert_eq!(
            close_tab_request_dir(Path::new("C:\\Local\\AppData"), true),
            PathBuf::from("C:\\Local\\AppData\\claude-board-dev\\close-tab")
        );
        for dev in [false, true] {
            assert_ne!(
                close_tab_request_dir(Path::new("C:\\Local\\AppData"), dev),
                crate::open_tab::args::open_tab_dir(Path::new("C:\\Local\\AppData"), dev),
                "여는 폴더와 섞이지 않는다"
            );
        }
    }
}
