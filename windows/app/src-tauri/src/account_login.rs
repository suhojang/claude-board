//! 계정 추가 로그인(설정 > 계정) — 보드가 ConPTY 로 `claude auth login --claudeai` 를 돌리고(`CLAUDE_CONFIG_DIR=<슬롯>`, 1000열),
//! 출력에서 주소를 찾아(`login_output.rs`) 프론트에 알리고, 사용자가 붙여 넣은 코드를 그 ConPTY 에 넣는다.
//! 끝났는지는 보드가 그 슬롯을 지켜보는 `accounts::tick`(15분 한도)이 가르고, 이 모듈은 그 결과(`Host::pending`)를 따라 프로세스를 거둔다.
//!
//! - `machine.rs` — 순수 규칙. 출력·종료·코드·끝남을 받아 할 일(`Effect`)만 돌려준다.
//! - `login_window.rs` — 로그인 주소를 여는 보드 소유 창(닫으면 취소).
//! - 여기 — 프로세스(ConPTY)·스레드·명령. 보드와는 `Host` 로만 닿는다(시험은 가짜 `Host`).
//!
//! 프론트와는 이벤트 `board://account-login`(`AccountLoginEvent`)과 명령 `begin_add_account`·`submit_login_code`·`open_login_url`·
//! `delete_account`, 그리고 board.rs 의 `cancel_add_account` 로 말한다.
//!
//! 어느 프로그램을 띄울지는 `test_claude::login_program` 이 가른다 — 시험 모드면 가짜(`windows/ci/fake-claude-login.cjs`), 설정이 어긋나면 거절,
//! 꺼져 있을 때만 진짜 claude. 시험은 가짜만 쓴다.
//!
//! `BROWSER` 는 슬롯의 `open-browser.cmd` 로 정한다 — 받은 주소를 슬롯의 `url.txt` 에만 적고 브라우저를 열지 않는 도우미다. 진짜 claude 가
//! 지금 로그인된 계정이 이미 들어 있는 기본 브라우저를 열어 거기서 로그인을 마쳐 버리면 "다른 계정 추가"가 안 되기 때문이다(mac 도 같은 방식).
//! 주소는 출력에서 찾는 것이 먼저이고, `url.txt` 는 보드가 같은 감시 반복(`watch`)에서 읽는 두 번째 길이다.
//! 계정 추가를 접을 때는 로그인 프로세스를 먼저 죽이고 끝난 것을 확인한 뒤에 슬롯을 지운다(`stop_then_remove_slot`) —
//! 안 그러면 죽기 전의 claude 가 지운 슬롯에 평문 토큰 파일을 다시 만들 수 있다.
//!
//! unchecked: 진짜 Windows `claude auth login` 의 ConPTY 출력 모양, 진짜 Windows Claude Code 가 `BROWSER` 를 따르는지(따르지 않으면 기본 브라우저가
//! 열리고 주소는 출력에서만 얻는다), 로그인 창 자체(GUI).

mod machine;
mod login_window;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use claude_board_core::paths;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::accounts::{self, Account};
use crate::board::Board;
use crate::pty::Pty;
use crate::test_claude::{self, Decision};
use crate::util::board_log;
use machine::{Effect, Machine};

/// 프론트로 보내는 로그인 진행 이벤트.
pub const ACCOUNT_LOGIN_EVENT: &str = "board://account-login";

/// 로그인 주소가 한 줄로 나오게 넓은 가짜 콘솔로 띄운다(mac 은 1000열 × 50줄).
const COLUMNS: u16 = 1000;
const ROWS: u16 = 50;

/// 보드가 이 로그인을 아직 기다리는지 보는 간격
const WATCH_INTERVAL: Duration = Duration::from_millis(300);
/// 로그인 프로세스를 죽이고 끝난 것을 기다리는 한도 — 넘으면 로그에 남기고 슬롯을 지운다
const STOP_WAIT: Duration = Duration::from_secs(3);
/// `BROWSER` 도우미 파일 이름 — 슬롯 안에 쓴다
const BROWSER_HELPER: &str = "open-browser.cmd";
/// 도우미가 받은 주소를 적는 파일 이름 — 슬롯 안
const URL_FILE: &str = "url.txt";
/// 프로세스가 끝난 뒤 남은 출력을 기다리는 한도 — 콘솔을 닫아도 파이프가 안 끊기는 경우에도 종료 처리는 한다
const OUTPUT_DRAIN_LIMIT: Duration = Duration::from_secs(3);

/// 이벤트 내용 — 없는 칸은 보내지 않는다. `url` 은 열 로그인 주소, `note` 는 안내 한 줄, `done` 은 끝남, `error` 는 사유.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 프론트에 로그인 진행을 알린다.
pub fn emit(app: &AppHandle, event: &AccountLoginEvent) {
    if let Err(e) = app.emit(ACCOUNT_LOGIN_EVENT, event) {
        board_log(&format!("계정 로그인 이벤트를 보내지 못했습니다: {e}"));
    }
}

// ── 띄우기 ───────────────────────────────────────────────────

/// 로그인 명령 하나를 띄우는 방법.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// 이 프로세스에만 더하는 환경변수
    pub vars: Vec<(String, String)>,
    /// `BROWSER` 도우미가 주소를 적는 파일 — 있으면 출력 말고 이 파일에서도 주소를 찾는다
    pub url_file: Option<PathBuf>,
}

/// `BROWSER` 도우미(`open-browser.cmd`) 내용 — 첫 인자(주소)를 같은 폴더의 `url.txt` 에 덧붙이기만 한다. 브라우저·네트워크·`start` 를 쓰지 않는다.
/// 주소는 `set "…"` 의 큰따옴표 안에 받고 지연 확장(`!address!`)으로 쓴다 — 주소의 `&`·`>`·`%` 가 명령으로 읽히지 않게.
/// 셸이 한글 코드 페이지로 읽어도 깨지지 않게 ASCII 만 쓴다.
pub fn browser_helper_script() -> &'static str {
    "@echo off\r\n\
rem Claude Board: write the login address next to this file. Never opens a browser.\r\n\
setlocal EnableDelayedExpansion\r\n\
set \"address=%~1\"\r\n\
>>\"%~dp0url.txt\" echo(!address!\r\n\
exit /b 0\r\n"
}

/// 슬롯에 도우미를 쓴다. 경로(`BROWSER` 값)와 주소 파일 경로를 돌려준다.
pub fn write_browser_helper(slot: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let helper = slot.join(BROWSER_HELPER);
    std::fs::write(&helper, browser_helper_script())?;
    Ok((helper, slot.join(URL_FILE)))
}

/// 로그인 프로세스에 더할 환경 — `CLAUDE_CONFIG_DIR`(슬롯)·`BROWSER`(슬롯의 도우미)는 늘, 시험 모드(가짜 claude)에서만 보드 환경의 `FAKE_LOGIN_*` 을 그대로 넘긴다
/// (탭 셸처럼 이 프로세스의 환경도 보드 환경을 물려받지 않으므로 가짜의 시험 손잡이는 이름으로 골라 넘겨야 닿는다).
pub fn launch_vars(slot: &Path, test_mode: bool, env: impl Iterator<Item = (OsString, OsString)>) -> Vec<(String, String)> {
    let mut vars = vec![
        ("CLAUDE_CONFIG_DIR".to_string(), slot.to_string_lossy().into_owned()),
        ("BROWSER".to_string(), slot.join(BROWSER_HELPER).to_string_lossy().into_owned()),
    ];
    if test_mode {
        for (name, value) in env {
            if let (Some(name), Some(value)) = (name.to_str(), value.to_str()) {
                if name.starts_with("FAKE_LOGIN_") {
                    vars.push((name.to_string(), value.to_string()));
                }
            }
        }
    }
    vars
}

// ── 보드와의 경계 ────────────────────────────────────────────

/// 로그인 세션이 바깥(보드·프론트)과 닿는 곳.
pub trait Host: Send + Sync + 'static {
    /// 프론트에 알린다
    fn emit(&self, event: AccountLoginEvent);
    /// 보드가 이 로그인을 아직 기다린다(저장·취소·시간 초과로 끝나면 false)
    fn pending(&self) -> bool;
    /// 계정 추가를 접고 이 안내를 오류로 띄운다
    fn fail(&self, message: &str);
}

/// 실행 중인 로그인 하나 — ConPTY 와 상태 기계.
pub struct Session {
    pty: Arc<Pty>,
    machine: Mutex<Machine>,
    host: Arc<dyn Host>,
    /// 알려 준 로그인 주소 — "다른 브라우저에서 열기"가 읽는다
    url: Mutex<Option<String>>,
    /// 프로세스가 끝났다(출력을 다 읽기 전일 수 있다) — 슬롯을 지워도 되는 때
    exited: AtomicBool,
    /// 프로세스가 끝나고 출력도 읽었다
    finished: AtomicBool,
    /// `BROWSER` 도우미가 주소를 적는 파일
    url_file: Option<PathBuf>,
}

impl Session {
    /// 사용자가 붙여 넣은 코드를 명령에 써 넣는다.
    pub fn submit_code(&self, code: &str) {
        let effects = self.machine.lock().unwrap().on_submit(code);
        self.apply(effects);
    }

    pub fn url(&self) -> Option<String> {
        self.url.lock().unwrap().clone()
    }

    /// 프로세스를 죽이고 끝난 것을 `limit` 까지 기다린다. 끝났으면 true — 이미 끝났어도 true
    pub fn stop_and_wait(&self, limit: Duration) -> bool {
        self.pty.kill();
        let deadline = Instant::now() + limit;
        while !self.exited.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    /// 프로세스가 끝났다
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// 아직 주소를 모르면 `BROWSER` 도우미의 주소 파일을 읽어 본다. 파일이 없거나 아직 온전한 주소가 없으면 아무 일도 없다.
    fn poll_url_file(&self) {
        let Some(path) = &self.url_file else { return };
        if self.url.lock().unwrap().is_some() {
            return;
        }
        let Ok(bytes) = std::fs::read(path) else { return };
        let effects = self.machine.lock().unwrap().on_url_file(&String::from_utf8_lossy(&bytes));
        self.apply(effects);
    }

    /// 프로세스가 끝나고 종료 처리까지 마쳤다(시험이 기다린다)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// 할 일을 한다. 상태 기계의 잠금은 이미 풀린 뒤라 `Host` 가 보드 잠금을 잡아도 얽히지 않는다.
    fn apply(&self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                // 코드는 로그에 남기지 않는다
                Effect::Write(bytes) => {
                    if let Err(e) = self.pty.write_all(&bytes) {
                        board_log(&format!("로그인 명령에 쓰지 못했습니다: {e}"));
                    }
                }
                Effect::Emit(event) => {
                    if let Some(url) = &event.url {
                        *self.url.lock().unwrap() = Some(url.clone());
                    }
                    self.host.emit(event);
                }
                Effect::Fail(message) => self.host.fail(&message),
            }
        }
    }
}

/// 로그인 명령을 가짜 콘솔에 띄우고 지켜보는 스레드 둘을 시작한다.
/// 하나는 출력을 읽다가 프로세스가 끝나면 종료를 처리하고, 하나는 보드가 이 로그인을 접었는지 보다가 프로세스를 거두고 끝났다고 알린다.
pub fn start(host: Arc<dyn Host>, launch: &Launch, watch_interval: Duration) -> io::Result<Arc<Session>> {
    let pty = Pty::spawn_env(&launch.program, &launch.args, &launch.cwd, COLUMNS, ROWS, &launch.vars)?;
    let session = Arc::new(Session {
        pty: Arc::new(pty),
        machine: Mutex::new(Machine::default()),
        host,
        url: Mutex::new(None),
        exited: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        url_file: launch.url_file.clone(),
    });
    let supervised = Arc::clone(&session);
    std::thread::Builder::new().name("account-login".into()).spawn(move || supervise(&supervised))?;
    let watched = Arc::clone(&session);
    std::thread::Builder::new().name("account-login-watch".into()).spawn(move || watch(&watched, watch_interval))?;
    Ok(session)
}

/// 출력을 읽고, 프로세스가 끝나면 콘솔을 닫아 남은 출력을 다 읽은 뒤 종료를 상태 기계에 알린다.
fn supervise(session: &Arc<Session>) {
    let (drained_tx, drained_rx) = mpsc::channel();
    let reader = Arc::clone(session);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 8192];
        while let Ok(n @ 1..) = reader.pty.read(&mut buf) {
            let effects = reader.machine.lock().unwrap().on_output(&buf[..n]);
            reader.apply(effects);
        }
        reader.pty.close_output();
        let _ = drained_tx.send(());
    });
    let code = session.pty.wait_exit();
    session.exited.store(true, Ordering::SeqCst);
    // 콘솔을 닫아야 출력 파이프가 끊긴다. 닫기가 오래 걸려도 읽기는 계속된다
    let closer = Arc::clone(&session.pty);
    std::thread::spawn(move || closer.close_console());
    session.pty.release();
    let _ = drained_rx.recv_timeout(OUTPUT_DRAIN_LIMIT);
    let pending = session.host.pending();
    let effects = session.machine.lock().unwrap().on_exit(code, pending);
    session.apply(effects);
    session.finished.store(true, Ordering::SeqCst);
}

/// 보드가 이 로그인을 접을 때까지 지켜보다가, 접으면 프로세스를 거두고 끝났다고 알린다.
fn watch(session: &Arc<Session>, interval: Duration) {
    while session.host.pending() {
        session.poll_url_file();
        std::thread::sleep(interval);
    }
    session.pty.kill();
    let effects = session.machine.lock().unwrap().on_ended();
    session.apply(effects);
}

// ── 보드에 붙이기 ────────────────────────────────────────────

/// 지금 도는 로그인 — `submit_login_code`·`open_login_url` 이 찾는다.
static CURRENT: Mutex<Option<Arc<Session>>> = Mutex::new(None);

fn current() -> Option<Arc<Session>> {
    CURRENT.lock().unwrap().clone()
}

/// 로그인 프로세스를 먼저 죽이고 끝난 것을 확인한 뒤에 슬롯을 지운다. 프로세스가 `limit` 안에 안 끝나도 슬롯은 지우되 로그에 남긴다.
pub fn stop_then_remove_slot(session: Option<&Session>, slot: &Path, limit: Duration) {
    if let Some(session) = session {
        if !session.stop_and_wait(limit) {
            board_log("로그인 명령이 제때 끝나지 않았는데 슬롯을 지웠습니다(다음 시작 때 남은 것을 치웁니다)");
        }
    }
    let _ = std::fs::remove_dir_all(slot);
}

/// 지금 도는 로그인 — 계정 추가를 접는 곳(`accounts`)이 계정 잠금을 쥔 채 집어 둔다. 그 뒤 새 로그인이 시작돼도 새 것을 죽이지 않게
/// 접는 쪽은 자기가 집은 것만 `stop_session_and_remove_slot` 으로 거둔다.
pub fn current_session() -> Option<Arc<Session>> {
    current()
}

/// 집어 둔 로그인(있으면)을 죽이고 끝난 것을 확인한 뒤 그 슬롯을 지운다.
pub fn stop_session_and_remove_slot(session: Option<Arc<Session>>, slot: &Path) {
    stop_then_remove_slot(session.as_deref(), slot, STOP_WAIT);
}

struct BoardHost {
    board: Arc<Board>,
    slot: PathBuf,
    /// 가짜 claude — 로그인 창을 열지 않는다(가짜 주소는 어디에도 닿지 않는다)
    test_mode: bool,
    window: login_window::LoginWindow,
}

impl BoardHost {
    fn user_closed(board: &Arc<Board>, slot: &Path) {
        let still_waiting = board.accounts.lock().unwrap().login_slot() == Some(slot);
        if !still_waiting {
            return;
        }
        accounts::cancel_add(board);
        board.accounts.lock().unwrap().flash("로그인 창을 닫아 계정 추가를 취소했습니다", false);
        board.publish();
    }
}

impl Host for BoardHost {
    fn emit(&self, event: AccountLoginEvent) {
        if let Some(url) = event.url.as_deref().filter(|_| !self.test_mode) {
            let (board, slot) = (self.board.clone(), self.slot.clone());
            self.window.open(url, move || Self::user_closed(&board, &slot));
        }
        if event.done == Some(true) {
            self.window.close();
        }
        emit(&self.board.app, &event);
    }

    fn pending(&self) -> bool {
        self.board.accounts.lock().unwrap().login_slot() == Some(self.slot.as_path())
    }

    fn fail(&self, message: &str) {
        accounts::cancel_add(&self.board);
        self.board.accounts.lock().unwrap().flash(message, true);
        self.board.publish();
    }
}

/// `accounts::begin_add` 가 부른다 — 슬롯에서 로그인 명령을 띄우고 현재 로그인으로 등록한다.
pub fn start_for_board(board: &Arc<Board>, program: &Path, args: Vec<String>, slot: &Path) -> io::Result<()> {
    let test_mode = matches!(test_claude::decision_from_process(), Decision::On(_));
    let (_, url_file) = write_browser_helper(slot)?;
    let launch = Launch {
        program: program.to_path_buf(),
        args,
        cwd: paths::home(),
        vars: launch_vars(slot, test_mode, std::env::vars_os()),
        url_file: Some(url_file),
    };
    let host = Arc::new(BoardHost { board: board.clone(), slot: slot.to_path_buf(), test_mode, window: login_window::LoginWindow::new(board.app.clone()) });
    let session = start(host, &launch, WATCH_INTERVAL)?;
    *CURRENT.lock().unwrap() = Some(session);
    Ok(())
}

// ── 계정 지우기 ──────────────────────────────────────────────

/// 지우기 전에 물을 말.
#[derive(Debug, PartialEq)]
pub struct DeleteAsk {
    pub title: String,
    pub text: String,
}

/// 지울 수 있는 계정이면 확인 문구. 저장 목록에 없거나 지금 로그인 계정이면 None(지금 계정은 보드가 지우지 않는다).
pub fn delete_ask(current_id: Option<&str>, saved: &[Account], id: &str) -> Option<DeleteAsk> {
    if current_id == Some(id) {
        return None;
    }
    let account = saved.iter().find(|a| a.id == id)?;
    Some(DeleteAsk {
        title: format!("{} 계정을 지울까요?", account.email()),
        text: "보드에 저장한 이 계정의 로그인 정보를 지웁니다. 다시 쓰려면 계정을 다시 추가해야 합니다.".to_string(),
    })
}

/// 확인을 받은 뒤에만 지운다. 확인 창과 지우기는 바깥에서 받는다 — 시험은 가짜를 넣는다.
pub fn delete_after_confirm(ask: &DeleteAsk, confirm: impl FnOnce(&DeleteAsk) -> bool, remove: impl FnOnce()) -> bool {
    if !confirm(ask) {
        return false;
    }
    remove();
    true
}

// ── 명령 ─────────────────────────────────────────────────────

/// 프론트의 `beginAddAccount(email?)`.
#[tauri::command(async)]
pub fn begin_add_account(board: State<'_, Arc<Board>>, email: Option<String>) {
    accounts::begin_add(&board, email);
}

/// 프론트의 `submitLoginCode(code)` — 로그인 창에 붙여 넣은 코드를 로그인 ConPTY 에 넣는다.
#[tauri::command(async)]
pub fn submit_login_code(code: String) {
    match current() {
        Some(session) => session.submit_code(&code),
        None => board_log("받을 로그인이 없어 코드를 버렸습니다"),
    }
}

/// 프론트의 `openLoginUrl()` — 지금 로그인 주소를 기본 브라우저로 연다. 주소는 프론트가 아니라 로그인 명령의 출력에서 온 것만 연다.
#[tauri::command(async)]
pub fn open_login_url(app: AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    let Some(url) = current().and_then(|s| s.url()).filter(|u| login_window::login_url(u).is_some()) else {
        board_log("열 로그인 주소가 없습니다");
        return;
    };
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        board_log(&format!("로그인 주소를 브라우저로 열지 못했습니다: {e}"));
    }
}

/// 프론트의 `deleteAccount(id)` — 지우기 전에 확인을 받는다(mac `confirmRemove`).
#[tauri::command(async)]
pub fn delete_account(board: State<'_, Arc<Board>>, id: String) {
    let ask = {
        let acc = board.accounts.lock().unwrap();
        delete_ask(acc.current.as_ref().map(|a| a.id.as_str()), &acc.saved, &id)
    };
    let Some(ask) = ask else {
        board_log("지울 수 없는 계정이라 무시했습니다");
        return;
    };
    delete_after_confirm(&ask, |ask| board.confirm(&ask.title, &ask.text, "지우기", "취소"), || accounts::remove_account(&board, id));
}

#[cfg(test)]
mod tests;
