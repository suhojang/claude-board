//! 바깥 탭 닫기의 보드 쪽 수함 — 명령줄 도구(`close_cli.rs`)가 쓰는 요청 폴더를 훑어 닫기 판정(`close.rs`)대로 답하는 조각.
//! mac 원본은 `OpenTab.swift` `CloseTabInbox` — 폴더 훑기·답 쓰기·`--stop` 의 끝나기 기다림. 규칙은 `close.rs`(순수)가
//! 젖고, 파일·시각·화면·동작을 물리는 부분만 여기 본다.
//!
//! 요청 `*.req.json` 을 읽어 `plan_close_tab` 으로 판정하고 — 닫기면 [`CloseIo::close`] 로 화면을 닫고,
//! 끝내고 닫기면 [`CloseIo::send_exit`] 로 claude 에 /exit 를 넣은 뒤 화면이 셸 프롬프트로 돌아오길
//! `CLOSE_TAB_STOP_WAIT` 안까지 기다렸다 닫는다(강제 종료는 하지 않는다 — `close.rs` `CLOSE_TAB_STOP_WAIT` 주의).
//! 답은 `<요청 id>.reply.json` 으로 쓰고 요청 파일은 지운다(답을 못 썼을 때도 지운다 — 기다림이 기한 넘기로
//! 끝나고 탭은 닫히지 않은 채로 그렇게 끝난다). 훑기는 열기(`handle.rs`)와 같은 틱 스레드 하나에서만 한다.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;

use claude_board_core::clock;

use super::args::open_tab_stale;
use super::close::{
    close_tab_decide, close_tab_reply, close_tab_target, is_session_id, plan_close_tab, CloseTabPlan, CloseTabReply,
    CloseTabRequest, CloseTabScreen, CLOSE_TAB_MAX_BYTES,
};
use crate::mdrender::is_link;
use crate::terminal::screens::PaneApp;
use crate::util::mtime;

/// 요청 파일 이름 뒷부분 — 닫기 요청 폴더의 약속(`<요청 id>.req.json`, `close_cli.rs` 도 같다)
const REQ_SUFFIX: &str = ".req.json";

/// 요청 폴더에 쌓인 옛 잡파일(엎어진 답·쓰다 남은 임시 파일)을 지우는 나이 — 열기(`handle.rs`)의 600초와 같다
const LEFTOVER_MAX_AGE: f64 = 600.0;

/// /exit 를 넣은 뒤 화면을 다시 보는 주기 — 끝남을 빨리 보려는 폴(mac 의 settle 폴과 같은 뜻)
const CLOSE_POLL: Duration = Duration::from_millis(200);

/// 프론트가 알려 온 탭 화면 값(`close_tab_panes` 명령, `terminal::screens::PaneApp` 의 웹뷰 꼴) —
/// 앱(웹뷰) 쪽에만 있는 값(탭 이름·바깥 열기 표지·이어받기·나눔·막 뜸·이름들)이다. 닫기 판정에 `PaneApp` 꼴로 더한다.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PaneView {
    pub id: String,
    pub names: Vec<String>,
    pub session_id: Option<String>,
    pub tab: String,
    pub external: bool,
    pub takeover: bool,
    pub split: bool,
    pub starting: bool,
}

/// 웹뷰 꼴을 닫기 판정의 `PaneApp` 꼴로 바꾼다 — 칸 이름이 같은지는 시험에서 대조한다
pub(crate) fn pane_app(view: PaneView) -> PaneApp {
    PaneApp {
        id: view.id,
        names: view.names,
        session_id: view.session_id,
        tab: view.tab,
        external: view.external,
        takeover: view.takeover,
        split: view.split,
        starting: view.starting,
    }
}

/// 닫기 판정의 동작 몸통 — 앱의 실물(`inbox.rs` `CloseActions`)과 시험의 가짜가 같이 간다
pub trait CloseIo {
    /// 화면(백엔드 터미널)을 닫는다. 못 닫으면 문구로 실패한다 — 닫지 않고 busy 답을 쓴다
    fn close(&mut self, term: &str) -> Result<(), String>;
    /// 그 화면의 claude 에 끝내기(/exit)를 넣는다. 넣지 못했으면 false — 탭은 그대로 두고 busy 답을 쓴다
    fn send_exit(&mut self, term: &str) -> bool;
}

/// 요청 폴더를 훑어 요청을 처리하고 `(<요청 id>, 답)` 쌍을 온 순서(`at` 빠른 것부터)로 돌려준다.
/// 답은 파일로도 쓰고 요청 파일은 지운다. 폴더가 없으면(아직 요청이 없으므로) 아무것도 하지 않는다.
pub fn drain_close(
    dir: &Path,
    screens: &dyn Fn(&[String]) -> Vec<CloseTabScreen>,
    io: &mut dyn CloseIo,
    stop_wait: Duration,
) -> Vec<(String, CloseTabReply)> {
    let mut pairs: Vec<(String, CloseTabReply)> = vec![];
    for request in read_requests(dir) {
        let reply = handle_request(&request, screens, io, stop_wait);
        write_reply(dir, &request.id, &reply);
        let _ = fs::remove_file(request_file(dir, &request.id));
        pairs.push((request.id, reply));
    }
    pairs
}

/// 요청 파일 하나의 경로
fn request_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}{REQ_SUFFIX}"))
}

/// 요청 폴더 훑기 — 닫기 요청 파일만 남겨 `at` 오름차순으로 돌려준다. 못 받을 요청(이름이 어긋남·링크·
/// 너무 큼·못 읽음·못 풀음·id 어긋남·오래됨)은 지우고 버려 다음 틱에 다시 읽히지 않게 한다(열기와 같다).
fn read_requests(dir: &Path) -> Vec<CloseTabRequest> {
    let entries = match fs::read_dir(dir) {
        Ok(iter) => iter,
        Err(_) => return vec![],
    };
    let now = clock::now();
    let mut found: Vec<CloseTabRequest> = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().into_string().ok() else { continue };
        if !name.ends_with(REQ_SUFFIX) {
            remove_old_stray(&path, now);
            continue;
        }
        let Some(id) = name.strip_suffix(REQ_SUFFIX) else { unreachable!("뒷부분을 이미 걸렀다") };
        // 명령줄 도구가 UUID 꼴 id 로 남기므로(`close_cli.rs` `new_request_id`) 꼴이 어긋나면 닫기 요청이 아니다
        if !is_session_id(id) {
            discard_request_file(&path);
            continue;
        }
        // 폴더·링크는 지우지 않고 지나간다
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(true) || is_link(&path) {
            continue;
        }
        let too_big = fs::metadata(&path).map(|meta| meta.len() as usize > CLOSE_TAB_MAX_BYTES).unwrap_or(true);
        if too_big {
            discard_request_file(&path);
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                discard_request_file(&path);
                continue;
            }
        };
        let request: CloseTabRequest = match serde_json::from_slice(&bytes) {
            Ok(request) => request,
            Err(_) => {
                discard_request_file(&path);
                continue;
            }
        };
        // 파일 안의 id 가 이름과 어긋나면 부른 쪽이 기다리지 않는 요청이다
        if request.id != id {
            discard_request_file(&path);
            continue;
        }
        // 명령줄 도구가 기다림을 끝내고 떠난 요청은 답을 쓸 곳이 없다 — 지우고 버린다(열기와 같다)
        if open_tab_stale(request.at, now) {
            discard_request_file(&path);
            continue;
        }
        found.push(request);
    }
    found.sort_by(|a, b| a.at.total_cmp(&b.at));
    found
}

/// 요청 폴더의 옛 잡파일(엎어진 답·쓰다 남은 임시 파일)을 지운다 — 최근 것은 건드리지 않는다(열기와 같다)
fn remove_old_stray(path: &Path, now: f64) {
    match mtime(path) {
        Some(t) if now - t <= LEFTOVER_MAX_AGE => {}
        _ => {
            let _ = fs::remove_file(path);
        }
    }
}

/// 몇 가지 죄 가운데 하나라 걸린 요청 파일을 지워 다음 틱의 반복 읽기를 끊는다
fn discard_request_file(path: &Path) {
    let _ = fs::remove_file(path);
}

/// 답을 임시 파일에 쓰고 바꿔 끼워 답 파일을 만든다 — 열기(`handle.rs`)와 같은 임시 꼴.
/// 실패는 조용히 두는데, 답이 없으면 명령줄 도구의 기다림이 그렇게 끝나기 때문이다. 요청은 drain 이 어쨌든 지운다.
fn write_reply(dir: &Path, id: &str, reply: &CloseTabReply) {
    let at = dir.join(format!("{id}.reply.json"));
    let partial = at.with_extension("cb-partial");
    if fs::write(&partial, reply.line().as_bytes()).is_err() {
        return;
    }
    if fs::rename(&partial, &at).is_err() {
        let _ = fs::remove_file(&partial);
    }
}

/// 요청 하나를 검사·판정해 답을 꾸민다 — 보드도 요청 파일을 뜯어 보므로 도구(`close_cli.rs`)와 같은 검사
/// (`close_tab_target`)를 다시 본다. 닫기·끝내고 닫기는 동작(`io`)으로 물리고, 나머지는 규칙의 답(`close_tab_reply`)으로.
fn handle_request(
    request: &CloseTabRequest,
    screens: &dyn Fn(&[String]) -> Vec<CloseTabScreen>,
    io: &mut dyn CloseIo,
    stop_wait: Duration,
) -> CloseTabReply {
    let target = match close_tab_target(request.session.as_deref(), request.title.as_deref()) {
        Ok(target) => target,
        Err(failure) => return failure.reply(),
    };
    // 끝내는 중 표지는 판정 중인 곳만 본다 — 훑기가 겹치지 않는 한(한 틱 스레드) 곪은 표지가 아직 없다
    let shot = screens(&[]);
    let plan = plan_close_tab(&target, request.stop, &shot);
    match &plan {
        CloseTabPlan::Close(id) => close_now(id, &shot, io),
        CloseTabPlan::StopThenClose(id) => stop_then_close(id, screens, io, stop_wait),
        CloseTabPlan::NotFound(_) | CloseTabPlan::Busy(_, _) | CloseTabPlan::Refuse(_, _) => {
            match close_tab_reply(&plan, &shot) {
                Some(reply) => reply,
                // 규칙의 약속(닫기·끝내고 닫기만 None)이 깨진 것 — 닫을 근거가 없는 답으로 골라 낸다
                None => failed_reply("busy", None, "닫기 판정을 답으로 만들지 못했습니다"),
            }
        }
    }
}

/// 판정이 닫기면 화면을 닫는다 — 닫으면 closed 답(맞은 화면의 이름·세션을 싣는다), 못 닫으면 busy 답.
/// 화면 닫기에 성공하면 이름·세션을 되찾지 못해도 답 자체는 closed 다(닫힌 사실은 변하지 않는다)
fn close_now(id: &str, shot: &[CloseTabScreen], io: &mut dyn CloseIo) -> CloseTabReply {
    let screen = shot.iter().find(|s| s.id == id);
    match io.close(id) {
        Ok(()) => {
            let (tab, session) = screen
                .map(|s| (Some(s.tab.clone()), s.session_id.clone()))
                .unwrap_or((None, None));
            CloseTabReply { ok: true, pid: None, session, status: "closed".to_string(), tab, why: None }
        }
        Err(why) => failed_reply("busy", screen, format!("탭을 닫지 못했습니다: {why}")),
    }
}

/// 실패 꼴 답 — 맞은 화면을 알면 그 이름·세션을 싣는다(규칙 `close_tab_reply` 의 어깨 붙잡기와 같다)
fn failed_reply(status: &str, screen: Option<&CloseTabScreen>, why: impl Into<String>) -> CloseTabReply {
    let (tab, session) = screen
        .map(|s| (Some(s.tab.clone()), s.session_id.clone()))
        .unwrap_or((None, None));
    CloseTabReply { ok: false, pid: None, session, status: status.to_string(), tab, why: Some(why.into()) }
}

/// --stop 의 끝내고 닫기 — claude 에 /exit 를 넣고(`CloseIo::send_exit`), 끝나서 셸 프롬프트로 돌아오는지
/// (`close_tab_decide` 의 닫기 판정) `stop_wait` 안까지 `screens` 를 다시 보며 기다린다. 못 넣거나 기다림이
/// 넘으면 탭은 그대로 두고 busy 답, 끝내는 사이 화면이 사라지면(탭이 통째로 닫혔다) not-found 답.
/// 넣기 실패 답의 이름·세션은 다시 본 화면 값에서 되찾는다(탭이 그 사이 변했을 수 있으므로).
fn stop_then_close(
    id: &str,
    screens: &dyn Fn(&[String]) -> Vec<CloseTabScreen>,
    io: &mut dyn CloseIo,
    stop_wait: Duration,
) -> CloseTabReply {
    if !io.send_exit(id) {
        let shot = screens(&[]);
        let screen = shot.iter().find(|s| s.id == id);
        return failed_reply("busy", screen, "claude 를 끝낼 수 없었습니다(탭은 그대로 둡니다)");
    }
    let started = Instant::now();
    let stopping = [id.to_string()];
    loop {
        std::thread::sleep(CLOSE_POLL);
        let shot = screens(&stopping);
        let Some(fresh) = shot.iter().find(|s| s.id == id) else {
            // 끝내는 사이 탭이 닫혔다(셸이 통째로 끝났거나 손이 먼저 닫았다) — 닫을 것도 없다
            return failed_reply("not-found", None, "claude 를 끝내는 사이 탭이 닫혔습니다");
        };
        if let CloseTabPlan::Close(close_id) = close_tab_decide(fresh, false) {
            return close_now(&close_id, &shot, io);
        }
        if started.elapsed() >= stop_wait {
            return failed_reply(
                "busy",
                Some(fresh),
                format!("claude 가 {:.0}초 안에 끝나지 않아 닫지 않았습니다(탭은 그대로 둡니다)", stop_wait.as_secs_f64()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// 닫기 요청의 세션 id(원본 close.rs 시험과 같은 자리표)
    const SID: &str = "0f0e1d2c-3b4a-5968-7766-554433221100";

    /// 시험용 임시 폴더(repo 관례 — cb-접두어와 short_id)
    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cb-closetab-{tag}-{}", crate::util::short_id()))
    }

    /// 기본 화면 — tab "역할", 바깥 열기 표지, 셸 프롬프트(원본 close.rs 시험과 같다)
    fn screen(id: &str, set: impl FnOnce(&mut CloseTabScreen)) -> CloseTabScreen {
        let mut s = CloseTabScreen {
            id: id.to_string(),
            tab: "역할".to_string(),
            session_id: Some(SID.to_string()),
            names: vec!["역할".to_string()],
            external: true,
            takeover: false,
            split: false,
            stopping: false,
            foreground: crate::open_tab::close::CloseTabForeground::Prompt,
        };
        set(&mut s);
        s
    }

    /// 요청 파일을 폴더에 남긴다
    fn put_request(dir: &Path, request: &CloseTabRequest) {
        fs::create_dir_all(dir).unwrap();
        fs::write(request_file(dir, &request.id), serde_json::to_vec(request).unwrap()).unwrap();
    }

    fn request(id: &str, at: f64, stop: bool) -> CloseTabRequest {
        CloseTabRequest { at, id: id.to_string(), session: Some(SID.to_string()), stop, title: None }
    }

    /// 닫기 동작의 가짜 — 닫은 화면과 /exit 를 넣은 화면을 기록한다
    struct FakeIo {
        closed: Vec<String>,
        exits: Vec<String>,
        close_err: Option<String>,
        exit_ok: bool,
    }

    impl FakeIo {
        fn fresh() -> FakeIo {
            FakeIo { closed: vec![], exits: vec![], close_err: None, exit_ok: true }
        }
    }

    impl CloseIo for FakeIo {
        fn close(&mut self, term: &str) -> Result<(), String> {
            self.closed.push(term.to_string());
            match &self.close_err {
                Some(why) => Err(why.clone()),
                None => Ok(()),
            }
        }
        fn send_exit(&mut self, term: &str) -> bool {
            self.exits.push(term.to_string());
            self.exit_ok
        }
    }

    /// 화면 값의 가짜 — 부른 차례마다 곱을 하나씩 뱉고, 끝내는 중 표지를 기록한다(곱이 다 소진되면 빈 곱)
    struct FakeScreens {
        shots: RefCell<VecDeque<Vec<CloseTabScreen>>>,
        seen_stopping: RefCell<Vec<Vec<String>>>,
    }

    impl FakeScreens {
        fn new(shots: Vec<Vec<CloseTabScreen>>) -> FakeScreens {
            FakeScreens { shots: RefCell::new(shots.into()), seen_stopping: RefCell::new(vec![]) }
        }

        fn take(&self, stopping: &[String]) -> Vec<CloseTabScreen> {
            self.seen_stopping.borrow_mut().push(stopping.to_vec());
            self.shots.borrow_mut().pop_front().unwrap_or_default()
        }
    }

    fn drain(dir: &Path, shots: &FakeScreens, io: &mut FakeIo, stop_wait: Duration) -> Vec<(String, CloseTabReply)> {
        drain_close(dir, &|stopping: &[String]| shots.take(stopping), io, stop_wait)
    }

    #[test]
    fn prompt_tab_closes_and_writes_closed_reply() {
        let dir = temp_dir("closed");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        put_request(&dir, &request(SID, now - 5.0, false));
        let shots = FakeScreens::new(vec![vec![screen("t1", |_| {})]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        assert_eq!(pairs.len(), 1);
        assert!(io.closed.iter().eq(["t1"].iter()), "닫기는 맞은 화면 id 로 한다");
        assert!(io.exits.is_empty(), "셸 프롬프트라 /exit 를 넣지 않는다");
        assert_eq!(io.closed[0], "t1");
        let reply = &pairs[0].1;
        assert!(reply.ok && reply.status == "closed");
        assert_eq!(reply.tab.as_deref(), Some("역할"));
        assert_eq!(reply.session.as_deref(), Some(SID));
        assert_eq!(reply.why, None);
        // 답 파일의 키 차례가 고정(원본 sortedKeys) — 빈 칸(pid·why)은 뺀 꼴
        let written = fs::read_to_string(dir.join(format!("{SID}.reply.json"))).unwrap();
        assert_eq!(written, format!(r#"{{"ok":true,"session":"{SID}","status":"closed","tab":"역할"}}"#));
        // 요청은 답을 쓴 뒤 지운다
        assert!(!request_file(&dir, SID).exists(), "처리된 요청 파일은 지운다");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn close_failure_replies_busy_and_keeps_tab() {
        let dir = temp_dir("close-err");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        put_request(&dir, &request(SID, now - 5.0, false));
        let shots = FakeScreens::new(vec![vec![screen("t1", |_| {})]]);
        let mut io = FakeIo { close_err: Some(String::from("터미널이 없다")), ..FakeIo::fresh() };
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        let reply = &pairs[0].1;
        assert!(!reply.ok && reply.status == "busy");
        assert_eq!(reply.why.as_deref(), Some("탭을 닫지 못했습니다: 터미널이 없다"));
        assert!(!request_file(&dir, SID).exists(), "요청은 답을 못 썼을 때도 지워 다음 틱의 반복 처리를 끊는다");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stop_then_close_exits_then_closes_resumed_shell() {
        let dir = temp_dir("stop-closed");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        let t1 = screen("t1", |s| s.foreground = crate::open_tab::close::CloseTabForeground::Claude);
        put_request(&dir, &request(SID, now - 5.0, true));
        let shots = FakeScreens::new(vec![vec![t1], vec![screen("t1", |_| {})]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        assert!(io.exits.iter().eq(["t1"].iter()), "/exit 는 맞은 화면에 넣는다");
        assert!(io.closed.iter().eq(["t1"].iter()), "프롬프트로 돌아온 뒤에 닫는다");
        let reply = &pairs[0].1;
        assert!(reply.ok && reply.status == "closed");
        // 판정 화면(끝내는 중 표지 없음) 이후 폴은 [id] 표지를 심는다
        assert!(shots.seen_stopping.borrow().last().unwrap().iter().eq(["t1"].iter()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stop_send_exit_failure_replies_busy_without_closing() {
        let dir = temp_dir("exit-fail");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        let t1 = screen("t1", |s| s.foreground = crate::open_tab::close::CloseTabForeground::Claude);
        put_request(&dir, &request(SID, now - 5.0, true));
        // 넣기 실패 답은 화면 값을 다시 보며 이름을 되찾는다 → 곱이 하나 더 필요하다
        let shots = FakeScreens::new(vec![vec![t1], vec![screen("t1", |_| {})]]);
        let mut io = FakeIo { exit_ok: false, ..FakeIo::fresh() };
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        let reply = &pairs[0].1;
        assert!(!reply.ok && reply.status == "busy");
        assert_eq!(reply.why.as_deref(), Some("claude 를 끝낼 수 없었습니다(탭은 그대로 둡니다)"));
        assert_eq!(reply.tab.as_deref(), Some("역할"), "넣기 실패 답에도 맞은 화면의 이름을 싣는다");
        assert!(io.closed.is_empty(), "못 끝냈으면 닫지 않는다");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stop_screen_vanishing_replies_not_found_without_closing() {
        let dir = temp_dir("stop-vanish");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        let t1 = screen("t1", |s| s.foreground = crate::open_tab::close::CloseTabForeground::Claude);
        put_request(&dir, &request(SID, now - 5.0, true));
        let shots = FakeScreens::new(vec![vec![t1], vec![]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        let reply = &pairs[0].1;
        assert!(!reply.ok && reply.status == "not-found");
        assert_eq!(reply.why.as_deref(), Some("claude 를 끝내는 사이 탭이 닫혔습니다"));
        assert!(io.closed.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stop_wait_timeout_replies_busy_and_keeps_tab() {
        let dir = temp_dir("stop-timeout");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        let claude = |s: &mut CloseTabScreen| {
            s.foreground = crate::open_tab::close::CloseTabForeground::Claude;
        };
        put_request(&dir, &request(SID, now - 5.0, true));
        // 폴을 다시 볼 때마다 claude 가 그대로다 → 기다림이 넘어 busy 답(폴 주기 0.2초, 두세 폴)
        let shots = FakeScreens::new((0..5).map(|_| vec![screen("t1", claude)]).collect());
        let mut io = FakeIo::fresh();
        // 폴을 다시 볼 때마다 claude 가 그대로다 → 기다림이 넘어 busy 답(폴 주기 0.2초, 두 세 폴)
        let pairs = drain(&dir, &shots, &mut io, Duration::from_millis(500));
        let reply = &pairs[0].1;
        assert!(!reply.ok && reply.status == "busy");
        assert!(reply.why.as_deref().is_some_and(|why| why.contains("닫지 않았습니다")));
        assert!(io.closed.is_empty(), "기다림이 넘으면 닫지 않는다(강제 종료는 하지 않는다)");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// 닫지 않는 판정 표 — 규칙의 자리표(원본 close.rs 판정과 같은 곱)를 drain 을 통과한 답으로 못박는다.
    /// 열기·닫기 어느 쪽이든 답은 drain 을 나가니 표도 drain 앞뒤로 본다
    #[test]
    fn rule_reply_table() {
        use crate::open_tab::close::CloseTabForeground;
        /// 판정 표의 한 줄 — (자리표, 화면 고침, --stop 표지, 기대 상태, 기대 이유 일부)
        type RuleRow = (&'static str, Box<dyn Fn(&mut CloseTabScreen)>, bool, &'static str, &'static str);
        let rows: Vec<RuleRow> = vec![
            ("claude, --stop 없음", Box::new(|s: &mut CloseTabScreen| s.foreground = CloseTabForeground::Claude), false, "busy", "claude 가 돌고"),
            ("다른 프로그램", Box::new(|s| s.foreground = CloseTabForeground::Other), false, "busy", "다른 프로그램"),
            ("막 뜨는 중", Box::new(|s| s.foreground = CloseTabForeground::Starting), false, "busy", "막 뜨는 중"),
            ("끝내는 중 표지", Box::new(|s| s.stopping = true), false, "busy", "끝내는 중"),
            ("사람이 연 탭", Box::new(|s| s.external = false), false, "refused", "바깥 열기"),
            ("이어받은 탭", Box::new(|s| s.takeover = true), false, "refused", "이어받은"),
            ("나눈 탭", Box::new(|s| s.split = true), false, "refused", "나눈"),
        ];
        for (note, set, stop, want_status, want_why) in rows {
            let dir = temp_dir("table");
            let _ = fs::remove_dir_all(&dir);
            let now = clock::now();
            put_request(&dir, &request(SID, now - 5.0, stop));
            let shots = FakeScreens::new(vec![vec![screen("t1", set)]]);
            let mut io = FakeIo::fresh();
            let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
            assert_eq!(pairs.len(), 1, "한 요청당 한 답({note})");
            let reply = &pairs[0].1;
            assert_eq!(reply.status, want_status, "{note}: 상태");
            assert!(reply.why.as_deref().is_some_and(|why| why.contains(want_why)), "{note}: 이유");
            assert!(!reply.ok, "{note}: 닫지 않는 답은 ok false");
            assert!(io.closed.is_empty(), "{note}: 닫지 않는 판정은 아무것도 닫지 않는다");
            assert!(io.exits.is_empty(), "{note}: /exit 넣기도 없다");
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn no_target_is_bad_args() {
        let dir = temp_dir("bad-args");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        // session·title 둘 다 없는 요청은 명령줄 도구가 만들지 않지만, 폴더에 생겼으면 거절로 답한다
        put_request(&dir, &CloseTabRequest { at: now - 5.0, id: SID.to_string(), session: None, stop: false, title: None });
        let shots = FakeScreens::new(vec![vec![screen("t1", |_| {})]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        assert_eq!(pairs[0].1.status, "bad-args");
        assert!(pairs[0].1.why.as_deref().is_some_and(|why| why.contains("하나가")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unmatched_session_is_not_found() {
        let dir = temp_dir("not-found");
        let _ = fs::remove_dir_all(&dir);
        let other = "11111111-2222-3333-4444-555555555555";
        let now = clock::now();
        // 보조 함수는 항상 SID 를 겨냥하므로, 어긋난 세션 겨냥은 곧이곧대로 적는다
        let unmatched = CloseTabRequest { at: now - 5.0, id: other.to_string(), session: Some(other.to_string()), stop: false, title: None };
        put_request(&dir, &unmatched);
        let shots = FakeScreens::new(vec![vec![screen("t1", |_| {})]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        assert_eq!(pairs[0].1.status, "not-found");
        assert!(pairs[0].1.tab.is_none(), "맞은 탭이 없으니 이름도 없다");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn matching_two_tabs_is_refused_without_closing_any() {
        let dir = temp_dir("many");
        let _ = fs::remove_dir_all(&dir);
        let now = clock::now();
        let title_req = CloseTabRequest {
            at: now - 5.0,
            id: SID.to_string(),
            session: None,
            stop: false,
            title: Some("역할".to_string()),
        };
        put_request(&dir, &title_req);
        let shots = FakeScreens::new(vec![vec![screen("t1", |_| {}), screen("t2", |_| {})]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        let reply = &pairs[0].1;
        assert_eq!(reply.status, "refused");
        assert!(reply.why.as_deref().is_some_and(|why| why.contains("2개")));
        assert!(reply.tab.is_none(), "여러 탭이 맞으면 이름을 싣지 않는다");
        assert!(io.closed.is_empty(), "아무것도 닫지 않는다");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// 못 받을 요청 표 — 지워 다음 틱에 같은 요청이 다시 답을 만들지 않게 한다(열기의 폴과 같다)
    #[test]
    fn unreadable_requests_are_discarded() {
        let dir = temp_dir("unreadable");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let now = clock::now();
        // id 꼴이 어긋난 파일 — 닫기 요청이 아니다
        fs::write(dir.join("역할.req.json"), b"{}").unwrap();
        // 파일 안의 id 가 이름과 어긋나는 요청
        let mismatch_id = CloseTabRequest { at: now - 5.0, id: "11111111-2222-3333-4444-555555555555".to_string(), session: Some(SID.to_string()), stop: false, title: None };
        fs::write(dir.join(format!("{SID}.req.json")), serde_json::to_vec(&mismatch_id).unwrap()).unwrap();
        // 못 풀 꼴 JSON
        fs::write(dir.join("00000000-0000-0000-0000-00000000000a.req.json"), b"{").unwrap();
        // 너무 큰 요청
        let big = request("00000000-0000-0000-0000-00000000000b", now, false);
        let mut bytes = serde_json::to_vec(&big).unwrap();
        bytes.resize(CLOSE_TAB_MAX_BYTES + 1, b' ');
        fs::write(request_file(&dir, "00000000-0000-0000-0000-00000000000b"), bytes).unwrap();
        // 오래된 요청 — 명령줄 도구가 기다림을 그만둔 뒤다
        put_request(&dir, &request("00000000-0000-0000-0000-00000000000c", now - 700.0, false));
        // 최근 잡파일(.req.json 이 아닌 것)은 지우지 않는다
        fs::write(dir.join("00000000-0000-0000-0000-00000000000a.reply.json"), "기다린 답").unwrap();

        let pairs = drain(&dir, &FakeScreens::new(vec![]), &mut FakeIo::fresh(), Duration::from_secs(10));
        assert!(pairs.is_empty(), "받을 요청이 없다({pairs:?})");
        // id 꼴이 어긋난 파일·못 풀 꼴·너무 큰 것·오래된 것·id 어긋난 것은 다 지워졌다
        assert!(!dir.join("역할.req.json").exists());
        assert!(!dir.join(format!("{SID}.req.json")).exists());
        assert!(!dir.join("00000000-0000-0000-0000-00000000000a.req.json").exists());
        assert!(!dir.join("00000000-0000-0000-0000-00000000000b.req.json").exists());
        assert!(!dir.join("00000000-0000-0000-0000-00000000000c.req.json").exists());
        // 최근 잡파일은 남는다
        assert!(dir.join("00000000-0000-0000-0000-00000000000a.reply.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn drain_without_folder_is_empty() {
        let dir = temp_dir("no-folder");
        let _ = fs::remove_dir_all(&dir);
        let pairs = drain(&dir, &FakeScreens::new(vec![]), &mut FakeIo::fresh(), Duration::from_secs(10));
        assert!(pairs.is_empty());
    }

    #[test]
    fn older_request_is_replied_first() {
        let dir = temp_dir("order");
        let _ = fs::remove_dir_all(&dir);
        let old = "00000000-0000-0000-0000-00000000000a";
        let new = "00000000-0000-0000-0000-00000000000b";
        let now = clock::now();
        put_request(&dir, &CloseTabRequest { at: now - 100.0, id: old.to_string(), session: Some(SID.to_string()), stop: false, title: None });
        put_request(&dir, &CloseTabRequest { at: now - 1.0, id: new.to_string(), session: Some(SID.to_string()), stop: false, title: None });
        let shots = FakeScreens::new(vec![vec![screen("t1", |_| {})], vec![screen("t2", |_| {})]]);
        let mut io = FakeIo::fresh();
        let pairs = drain(&dir, &shots, &mut io, Duration::from_secs(10));
        assert_eq!(pairs.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec![old, new], "at 오름차순");
        assert_eq!(io.closed, vec!["t1", "t2"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// 웹뷰 꼴 화면 값 — camelCase 키와 빠진 칸(기본값)을 읽고 `PaneApp` 꼴로 옮긴다
    #[test]
    fn pane_view_reads_camel_case_and_defaults() {
        let view: PaneView = serde_json::from_str(
            r#"{"id":"t1","names":["역할"],"sessionId":"11111111-2222-3333-4444-555555555555","tab":"역할","external":true,"takeover":false,"split":false,"starting":true}"#,
        )
        .unwrap();
        let app = pane_app(view);
        assert_eq!(app.id, "t1");
        assert_eq!(app.session_id.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
        assert!(app.starting);
        // 기본값 — 빠진 칸은 false(가리지 않은 것), names 는 빈 곱
        let lean: PaneView = serde_json::from_str(r#"{"id":"t2"}"#).unwrap();
        let app = pane_app(lean);
        assert!(app.names.is_empty() && !app.external && !app.takeover && !app.split && !app.starting);
        assert_eq!(app.session_id, None);
    }
}
