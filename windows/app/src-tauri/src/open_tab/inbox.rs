//! 바깥 탭 열기의 보드 쪽 연결 — 요청 폴더를 1초 틱으로 훑어(`handle::drain`) 요청을 프론트에 흘려 보내고 곱을 기다린다.
//! 프론트는 `open-tab-request` 이벤트를 받아(`hub.svelte.ts`) 탭을 열거나 다시 띄우거나 가리고, 끝나면
//! `open_tab_done` 명령으로 곱(탭·세션)을 알려 온다. mac `OpenTab.swift` drain 의 Windows 판이다.
//!
//! 흐름 — 명령줄 도구(`--open-tab`)가 요청 파일을 남긴다 → 이 모듈의 틱 스레드가 요청을 읽어 검사·판정을
//! `handle` 에 맡기고 동작(새 탭·다시 띄움·가리기)을 프론트에 보낸다 → 프론트의 곱을 기다린다 → 답은
//! `handle::drain` 이 파일로 쓰고 요청 파일은 drain 이 지운다. 이 훑기는 겹치면 곱이 어긋나므로 이 스레드 하나에서만 한다.

use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use claude_board_core::claude_live::read_claude_session;
use claude_board_core::paths;
use claude_board_core::place::SessionProc;
use claude_board_core::proc::{self, ProcessTable};

use crate::board::Board;
use crate::drive::{self, Purpose, Target};
use crate::open_tab::args::open_tab_dir;
use crate::open_tab::close::{CloseTabScreen, CLOSE_TAB_STOP_WAIT};
use crate::open_tab::close_cli::close_tab_request_dir;
use crate::open_tab::close_handle::{self, PaneView};
use crate::open_tab::handle::{self, OpenTabAction};
use crate::open_tab::plan::{claude_arg_info, is_session_id, open_tab_command, LiveClaude, OpenTabScreen};
use crate::readers::ClaudeNames;
use crate::session_action::SessionAction;
use crate::store;
use crate::terminal::screens::{close_tab_screens, open_tab_screens, PaneApp};
use crate::terms::{pairing, TermInfo, TermHub};
use crate::util::{board_log, short_id};

/// 프론트가 바깥 열기 요청을 듣는 이벤트 이름(`hub.svelte.ts` `startOpenTabFlow`)
const OPEN_TAB_REQUEST_EVENT: &str = "open-tab-request";

/// 곱을 기다리는 시간 — 명령줄 도구의 기다림(20초, `--launch` 40초)보다 짧아야 답을 마친다.
/// 답이 이 안에 오지 않으면 거절로 끝낸다(mac 은 탭을 여는 곳이 같은 프로세스라 기다림이 없다 — Windows 판의 결정).
const OPEN_TAB_WAIT_SECS: f64 = 10.0;

/// 프론트가 알려 온 바깥 열기 곱 — `tab` 은 곱이 닫힌 탭의 보이는 이름, 알아 내지 못했으면 None.
struct DoneNotice {
    tab: Option<String>,
    session: Option<String>,
}

/// 바깥 탭 열기 수함 — `ready` 는 프론트 준비 표지, `waiters` 는 곱을 기다리는 자리.
/// Tauri 명령(`open_tab_ready`·`open_tab_done`)과 틱 스레드가 나눠 쓴다.
#[derive(Default)]
pub struct Inbox {
    /// 프론트가 듣기를 켜고 탭 되살리기를 끝냈는가 — 이 표지가 참이 되기 전 요청은 훑지 않는다
    /// (되살린 탭이 아직 없어 닫힌 화면으로 답하는 것을 막는다 — mac `drain`의 `isRestored` 장치와 같다).
    pub ready: bool,
    /// 곱을 기다리는 자리 `<요청 id, 흘려 보낼 곳>`
    waiters: Vec<(String, mpsc::Sender<DoneNotice>)>,
    /// 프론트가 알려 온 탭 화면 값 — 닫기 판정에 쓴다(`close_tab_panes` 명령으로 갱신한다).
    /// 프론트가 보내기 전엔 비어 있어 바깥 열기 표지가 없다 → 닫기 요청은 표지 거절로 답한다(닫을 근거가 없으므로)
    panes: Vec<PaneApp>,
}

impl Inbox {
    /// 곱을 기다리는 자리를 등록한다.
    fn add_waiter(&mut self, id: String, sender: mpsc::Sender<DoneNotice>) {
        self.waiters.push((id, sender));
    }

    /// 기다리던 자리를 치운다 — 요청을 못 보냈거나 답이 늦어 곱을 받지 못할 때. 치우지 않으면 자리가 쌓인다.
    fn drop_waiter(&mut self, id: &str) {
        self.waiters.retain(|(waiter_id, _)| waiter_id != id);
    }

    /// 프론트의 곱을 기다리는 자리에 흘려 보낸다 — 기다리는 자리가 없으면(답이 늦어 이미 끝났으면) 그냥 잊는다.
    fn send_done(&mut self, id: &str, tab: Option<String>, session: Option<String>) {
        if let Some(pos) = self.waiters.iter().position(|(waiter_id, _)| waiter_id == id) {
            let (_, sender) = self.waiters.remove(pos);
            let _ = sender.send(DoneNotice { tab, session });
        }
    }
}

/// 프론트가 바깥 열기를 받을 준비가 끝났다고 알린다(`hub.svelte.ts` `notifyOpenTabReady`) — 듣기를 켜고
/// 탭 되살리기를 끝낸 뒤 딱 한 번. 이 표지가 참이 되기 전 요청은 틱 스레드가 읽지 않는다.
#[tauri::command]
pub fn open_tab_ready(board: State<'_, Arc<Board>>) {
    board.inbox.lock().unwrap().ready = true;
}

/// 프론트가 바깥 열기 하나의 곱을 알려 온다(`hub.svelte.ts` `reportOpenTabDone`) — 탭·세션을 알아 내지
/// 못했으면 None 이 온다. 기다리는 자리가 없으면(답이 늦어 이미 끝났으면) 그냥 잊는다(프론트 약속과 같다).
#[tauri::command]
pub fn open_tab_done(board: State<'_, Arc<Board>>, id: String, tab: Option<String>, session: Option<String>) {
    board.inbox.lock().unwrap().send_done(&id, tab, session);
}

/// 프론트가 닫기 판정에 쓸 탭 화면 값을 알려 온다 — 웹뷰에만 있는 값(탭 이름·바깥 열기 표지·이어받기·
/// 나눔·막 뜸)을 백엔드 판정(`close_handle::drain_close`)에 더한다. 요청이 있을 때마다 갱신해 보낸다.
#[tauri::command]
pub fn close_tab_panes(board: State<'_, Arc<Board>>, panes: Vec<PaneView>) {
    let panes = panes.into_iter().map(close_handle::pane_app).collect();
    board.inbox.lock().unwrap().panes = panes;
}

/// 프론트에 보내는 바깥 열기 요청 — `openTab.ts` `OpenTabRequest` 꼴(camelCase 키로 보낸다).
struct FrontRequest {
    id: String,
    cwd: String,
    /// 새 탭의 이름·한 화면 탭에 붙일 이름
    title: Option<String>,
    /// 셸에 칠 명령 — 가리기는 빈 목록
    command: Vec<String>,
    /// 이어 열 화면의 백엔드 터미널 id("t1") — 새 탭이면 None
    reuse_tab: Option<String>,
    /// 뒤에서 열기(가리키지 않기)
    no_focus: bool,
}

/// 요청 꼴을 이벤트 값으로 바꾼다 — 키는 프론트가 읽는 camelCase(`openTab.ts` `parseOpenTabEvent`).
fn request_value(request: &FrontRequest) -> Value {
    json!({
        "id": request.id,
        "cwd": request.cwd,
        "title": request.title,
        "command": request.command,
        "reuseTab": request.reuse_tab,
        "noFocus": request.no_focus,
    })
}

/// 요청을 프론트에 보내고 곱을 기다린다 — 이행하면 None, 아니면 문구(거절답 `why`).
/// 곱 `tab` 이 있으면 프론트가 탭을 열거나 가린 것 — 이행. 곱이 없거나 답이 늦으면 문구로 거절한다.
/// 이벤트 보내기는 `emit` 이 맡는다(시험에서는 보낸 값을 모으는 가짜로 바꾼다). `wait_secs` 은 기다림(시험에서 짧게).
fn send_and_wait(
    inbox: &Mutex<Inbox>,
    wait_secs: f64,
    request: &FrontRequest,
    emit: &dyn Fn(&str, &Value) -> Result<(), String>,
) -> Option<String> {
    let (sender, receiver) = mpsc::channel();
    inbox.lock().unwrap().add_waiter(request.id.clone(), sender);
    if let Err(e) = emit(OPEN_TAB_REQUEST_EVENT, &request_value(request)) {
        inbox.lock().unwrap().drop_waiter(&request.id);
        return Some(format!("바깥 탭 요청을 보내지 못했습니다: {e}"));
    }
    match receiver.recv_timeout(Duration::from_secs_f64(wait_secs)) {
        Ok(done) => match done.tab {
            Some(tab) => {
                let session = done.session.as_deref().unwrap_or("없음");
                board_log(&format!("바깥 탭 열림: {tab} — 세션 {session}"));
                None
            }
            None => Some("탭을 열지 못했습니다".to_string()),
        },
        Err(_) => {
            inbox.lock().unwrap().drop_waiter(&request.id);
            Some(format!(
                "바깥 탭 답이 {}초 안에 오지 않았습니다(탭이 열렸을 수 있습니다)",
                wait_secs as u64
            ))
        }
    }
}

/// 이어 열기의 대화 기록 검사 — claude `--resume` 은 대화 기록이 없으면 바로 끝나므로 띄우지 않고
/// 이유를 돌려준다(mac `OpenTab.swift` `noTranscript`). 이어 여는 세션(`--resume <id>`, fork 아님)만 검사하고,
/// 기록은 `~/.claude/projects/<요청 폴더의 영숫자 밖 문자를 -로>/<세션 id>.jsonl` 에서 본다
/// (mac 은 세션 기록의 `transcript_path` 를 먼저 봤는데, Windows 판은 act 가 세션 기록을 읽지 않아 폴더만 본다).
fn missing_transcript(claude_dir: &Path, cwd: &str, command: &[String]) -> Option<String> {
    let info = claude_arg_info(command);
    let resume = info.resume.filter(|_| !info.fork)?;
    if !is_session_id(&resume) {
        return None;
    }
    // 사용자가 대문자를 섞어 보내도 claude 가 쓰는 파일 이름은 소문자다(mac prep 도 소문자로 맞춘다)
    let sid = resume.to_lowercase();
    let file = store::transcript_path(None, cwd, &sid, claude_dir);
    if !file.exists() {
        return Some(format!(
            "이어 열 대화 기록이 없습니다(요청을 한 번도 보내지 않았거나 지워진 세션): {sid}"
        ));
    }
    None
}

/// 열기 동작의 몸통 — 새 탭·다시 띄움·가리기. 이행하면 None, 못 이행하면 문구(거절답 `why`).
/// `snapshot` 은 판정에 쓴 화면 값 — 다시 띄움·가리기의 화면을 여기서 찾는다.
/// `claude_dir` 은 대화 기록 검사에 쓴다(시험에서 임시 폴더), `emit` 은 실제로는 board 이벤트(`perform_action`).
fn act_with(
    inbox: &Mutex<Inbox>,
    wait_secs: f64,
    claude_dir: &Path,
    snapshot: &[OpenTabScreen],
    action: &OpenTabAction,
    emit: &dyn Fn(&str, &Value) -> Result<(), String>,
) -> Option<String> {
    match action {
        OpenTabAction::Open { cwd, title, command, no_focus } => {
            let cwd = handle::real_dir(cwd);
            if let Some(why) = missing_transcript(claude_dir, &cwd, command) {
                return Some(why);
            }
            let request = FrontRequest {
                id: short_id(),
                cwd,
                title: title.clone(),
                command: open_tab_command(command.clone(), None),
                reuse_tab: None,
                no_focus: *no_focus,
            };
            send_and_wait(inbox, wait_secs, &request, emit)
        }
        OpenTabAction::Reuse { id, command, no_focus } => {
            let Some(screen) = snapshot.iter().find(|screen| screen.id == *id) else {
                return Some("탭이 그 사이 닫혔습니다".to_string());
            };
            let cwd = handle::real_dir(&screen.cwd);
            if let Some(why) = missing_transcript(claude_dir, &cwd, command) {
                return Some(why);
            }
            // 새 탭과 같은 인자 argv 를 셸에 친다 — 탭 이름은 명령의 `--name` 으로만 맞춘다(제목 인자는 argv 에 없다)
            let request = FrontRequest {
                id: short_id(),
                cwd,
                title: claude_arg_info(command).name,
                command: open_tab_command(command.clone(), None),
                reuse_tab: Some(id.clone()),
                no_focus: *no_focus,
            };
            send_and_wait(inbox, wait_secs, &request, emit)
        }
        OpenTabAction::Focus { id } => {
            let Some(screen) = snapshot.iter().find(|screen| screen.id == *id) else {
                return Some("탭이 그 사이 닫혔습니다".to_string());
            };
            let request = FrontRequest {
                id: short_id(),
                cwd: handle::real_dir(&screen.cwd),
                title: None,
                // 가리기 — 명령 없이 화면을 앞으로 갖다 놓는다(프론트는 빈 명령을 가리기로 본다)
                command: vec![],
                reuse_tab: Some(id.clone()),
                no_focus: false,
            };
            send_and_wait(inbox, wait_secs, &request, emit)
        }
    }
}

/// 열기 동작 — board 의 수함·이벤트를 물린 `act_with`. drain 의 `act` 로 물린다.
fn perform_action(board: &Board, wait_secs: f64, snapshot: &[OpenTabScreen], action: &OpenTabAction) -> Option<String> {
    let emit = |event: &str, payload: &Value| board.app.emit(event, payload).map_err(|e| e.to_string());
    act_with(&board.inbox, wait_secs, &paths::claude_dir(), snapshot, action, &emit)
}

/// 열기 판정에 넘길 화면 값 — 도는 보드 터미널만 들여 짝짓기를 새로 읽는다(`board.rs` 확인의 같은 흐름).
/// 앱(웹뷰) 쪽 값(탭 이름·바깥 열기 표지 따위)은 백엔드가 모르므로 비운 채로 둔다(`PaneApp`).
fn screen_list(board: &Board) -> Vec<OpenTabScreen> {
    let hub = board.app.state::<TermHub>();
    // 끝난 터미널은 claude 징후가 빈 값이 되므로 도는 것만 남긴다(`open_tab_screens`의 주의와 같다)
    let terms: Vec<TermInfo> = hub.list().into_iter().filter(|term| term.status == "running").collect();
    let sessions = board.sessions();
    let procs: Vec<SessionProc> = sessions
        .iter()
        .filter(|session| session.pid > 0 && session.state != "ended")
        .map(|session| SessionProc { pid: session.pid, headless: session.headless, ended: false })
        .collect();
    let table = ProcessTable::capture();
    let screens = pairing::pair_all(&hub.hosts(), &procs, &table);
    // 화면 쪽 cwd 도 요청 쪽(`handle`)과 같은 도구로 풀어야 폴더 견주기가 성립한다(real_dir 주의 문구)
    let out: Vec<OpenTabScreen> = open_tab_screens(&terms, &sessions, &screens, &[]);
    out.iter().map(|screen| OpenTabScreen { cwd: handle::real_dir(&screen.cwd), ..screen.clone() }).collect()
}

/// 닫기 판정에 넘길 화면 값 — 도는 보드 터미널만(끝난 터미널은 claude 징후가 빈 값이 되므로, `screen_list`의
/// 주의와 같다), 프론트가 알려 온 탭 화면 값(`Inbox::panes`)을 더하고, 끝내는 중 표지는 판정 중이면 심는다.
/// cwd 견주기가 없어(닫기 판정에 cwd 가 없다) 열기와 달리 real_dir 로 풀 필요가 없다.
fn close_screen_list(board: &Board, stopping: &[String]) -> Vec<CloseTabScreen> {
    let panes: Vec<PaneApp> = board.inbox.lock().unwrap().panes.clone();
    let hub = board.app.state::<TermHub>();
    let terms: Vec<TermInfo> = hub.list().into_iter().filter(|term| term.status == "running").collect();
    let sessions = board.sessions();
    let procs: Vec<SessionProc> = sessions
        .iter()
        .filter(|session| session.pid > 0 && session.state != "ended")
        .map(|session| SessionProc { pid: session.pid, headless: session.headless, ended: false })
        .collect();
    let table = ProcessTable::capture();
    let pairing = pairing::pair_all(&hub.hosts(), &procs, &table);
    close_tab_screens(&terms, &sessions, &pairing, &panes, stopping)
}

/// 끝내기(/exit)만 쓰는 `drive` 실물 — 화면(터미널)에 단계를 친다. 콘솔·프로세스 끝내기는 닫기가 아니므로 안 쓴다.
struct SurfaceIo<'a> {
    hub: &'a TermHub,
}

impl drive::Io for SurfaceIo<'_> {
    fn surface(&self, id: &str, bytes: &[u8]) -> bool {
        self.hub.write(id, bytes).is_ok()
    }
    fn console(&self, _pid: u32, _inputs: &[claude_board_core::console::Input]) -> bool {
        false
    }
    fn kill(&self, _pid: u32) -> bool {
        false
    }
    fn sleep(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// 닫기 동작의 몸통 — 화면 닫기는 터미널 허브의 끄기(`kill`), 끝내기는 drive 의 `/exit` 단계를 그 화면에 친다.
/// 끝내기를 넣지 못했으면(화면이 이미 끝났거나 쓰기가 막혔다) false — 탭은 그대로 두고 busy 답을 쓴다.
/// 콘솔·프로세스 경로는 닫기가 아니므로(`drive::Io` 약속의 남는 조각) false 로 막아 둔다.
struct CloseActions<'a> {
    hub: &'a TermHub,
}

impl close_handle::CloseIo for CloseActions<'_> {
    fn close(&mut self, term: &str) -> Result<(), String> {
        self.hub.kill(term)
    }
    fn send_exit(&mut self, term: &str) -> bool {
        let io = SurfaceIo { hub: self.hub };
        drive::run(&Target::Surface(term.to_string()), &drive::steps_for(SessionAction::Stop), Purpose::Stop, &io)
    }
}

/// 닫기 요청 훑기 — 열기 요청과 폴더가 다르고(`close-tab` 폴더) 이 틱 스레드에서만 훑는다.
/// 답을 기다리는 동안(--stop 의 끝나기 기다림) 막히므로 `drain_tick` 에서만 부른다.
fn drain_close_tick(board: &Arc<Board>) {
    let dir = close_tab_request_dir(&paths::local_app_data(), false);
    let hub = board.app.state::<TermHub>();
    let mut actions = CloseActions { hub: hub.inner() };
    let screens = |stopping: &[String]| close_screen_list(board, stopping);
    // 답의 요청 id 는 열기와 같이 기록하지 않는다 — 상태·탭·세션·이유만 남긴다
    for (_id, reply) in
        close_handle::drain_close(&dir, &screens, &mut actions, Duration::from_secs_f64(CLOSE_TAB_STOP_WAIT))
    {
        board_log(&format!(
            "바깥 탭 닫기: {} {} {} {}",
            reply.status,
            reply.tab.as_deref().unwrap_or(""),
            reply.session.as_deref().unwrap_or(""),
            reply.why.as_deref().unwrap_or("")
        ));
    }
}

/// `~/.claude/sessions` 폴더의 `<pid>.json` 에서 (pid, 이름) — 이름 없거나 빈 것은 뺀다
/// (mac `ClaudeNames.live` 의 이름 부분. 살아 도는지는 부르는 쪽에서 본다).
fn named_live_claudes(claude_dir: &Path) -> Vec<(u32, String)> {
    let sessions_dir = claude_dir.join("sessions");
    let Some(entries) = std::fs::read_dir(&sessions_dir).ok() else { return vec![] };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.strip_suffix(".json")?.parse().ok()?;
            if pid == 0 {
                return None;
            }
            let name = read_claude_session(claude_dir, pid)?.name;
            let name = name.filter(|name| !name.is_empty())?;
            Some((pid, name))
        })
        .collect()
}

/// 살아 있는 claude 목록(mac `OpenTab.swift` `liveClaudes`) — `~/.claude/sessions` 의 `<pid>.json` 에서
/// 이름을 읽되 프로세스가 살아 도는지 확인한다. 요청한 세션은 보드 세션 기록의 pid 와 같은 이름 지도의
/// sessionId 색인으로도 본다(세션 기록에 pid 가 없는 세션을 위한 것 — `ClaudeNames::pid_for_session`).
fn live_claudes(board: &Board, session: Option<&str>) -> Vec<LiveClaude> {
    let claude_dir = paths::claude_dir();
    let table = ProcessTable::capture();
    // 살아 도는 claude 인지 — 창 이름이 claude 인지 실행 인자로 확인한다(`terms/pairing.rs` 와 같은 확인)
    let running = |pid: u32| table.name(pid).is_some_and(|name| proc::is_claude_exe(name, || proc::command_line(pid)));
    let named: Vec<(u32, String)> = named_live_claudes(&claude_dir).into_iter().filter(|&(pid, _)| running(pid)).collect();
    let mut claudes: Vec<LiveClaude> = named
        .iter()
        .map(|&(pid, ref name)| LiveClaude { pid: i32::try_from(pid).unwrap_or(0), session: None, name: Some(name.clone()) })
        .collect();
    let Some(sid) = session else { return claudes };
    let known_pid = board.session(sid).filter(|record| record.state != "ended").map(|record| record.pid);
    let mut named_lookup = ClaudeNames::new(claude_dir);
    let push = |pid: Option<u32>, claudes: &mut Vec<LiveClaude>| {
        let Some(pid) = pid.filter(|&pid| pid > 0 && running(pid)) else { return };
        let pid = i32::try_from(pid).unwrap_or(0);
        if !claudes.iter().any(|claude| claude.pid == pid) {
            claudes.push(LiveClaude { pid, session: Some(sid.to_string()), name: None });
        }
    };
    push(known_pid, &mut claudes);
    push(named_lookup.pid_for_session(sid), &mut claudes);
    claudes
}

/// 훑을 요청 폴더 — 명령줄 도구(`lib.rs` `open_tab_command_line`)가 쓰는 같은 폴더다.
fn request_dir() -> std::path::PathBuf {
    open_tab_dir(&paths::local_app_data(), false)
}

/// 1초 틱 한 번 — 준비가 끝난 뒤에만 요청 폴더를 훑는다. 답을 기다리는 동안 막히므로 이 함수는 스레드에서만 부른다.
fn drain_tick(board: &Arc<Board>, wait_secs: f64) {
    if !board.inbox.lock().unwrap().ready {
        return;
    }
    // 화면 값은 요청이 있을 때만 새로 읽는다 — act 가 판정에 쓴 것과 같은 값을 보게 남겨 둔다
    let shot: RefCell<Vec<OpenTabScreen>> = RefCell::new(Vec::new());
    let screens = || {
        let fresh = screen_list(board);
        *shot.borrow_mut() = fresh.clone();
        fresh
    };
    let live = |session: Option<&str>| live_claudes(board, session);
    let mut act = |action: OpenTabAction| perform_action(board, wait_secs, &shot.borrow(), &action);
    // 답의 요청 id 는 mac 처럼 기록하지 않는다 — 상태·탭·세션·이유만 남긴다
    for (_id, reply) in handle::drain(&request_dir(), &screens, &live, &mut act) {
        // mac drain 의 답 기록 그대로 — 빈 칸은 그 사이(탭·세션·이유 없음)다
        board_log(&format!(
            "바깥 탭 열기: {} {} {} {}",
            reply.status,
            reply.tab.as_deref().unwrap_or(""),
            reply.session.as_deref().unwrap_or(""),
            reply.why.as_deref().unwrap_or("")
        ));
    }
    // 닫기 요청 훑기 — 열기 요청과 폴더가 다르고(닫기는 `close-tab` 폴더) 같은 틱 스레드에서만 훑는다
    drain_close_tick(board);
}

/// 바깥 탭 열기 틱 스레드 — 1초마다 요청 폴더를 본다(보드의 다른 틱들과 같은 간격).
pub fn start(board: Arc<Board>) {
    std::thread::spawn(move || loop {
        drain_tick(&board, OPEN_TAB_WAIT_SECS);
        std::thread::sleep(Duration::from_secs(1));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험용 임시 폴더(repo 관례 — cb-접두어와 short_id)
    fn temp_claude_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cb-opentab-inbox-{tag}-{}", short_id()))
    }

    /// 대화 기록 파일을 만들어 놓는다 — `--resume` 을 거르지 않으려면 이 파일이 있어야 한다
    fn put_transcript(claude_dir: &Path, cwd: &str, sid: &str) {
        let file = store::transcript_path(None, cwd, sid, claude_dir);
        std::fs::create_dir_all(file.parent().expect("기록 파일의 폴더")).expect("기록 폴더 만들기");
        std::fs::write(&file, b"{\"type\":\"user\"}\n").expect("기록 파일 쓰기");
    }

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    /// 화면 값 하나 — 다시 띄움·가리기 시험에서 snapshot 으로 넘긴다
    fn screen(id: &str, cwd: &str) -> OpenTabScreen {
        OpenTabScreen { id: id.to_string(), session_id: None, cwd: cwd.to_string(), pid: None, names: vec![], busy: false, takeover: false }
    }

    /// 보내기를 죽이는 가짜 — 보낸 이벤트 이름·값을 RefCell 에 쌓아 단언한다
    fn capture_emit(seen: &RefCell<Vec<(String, Value)>>) -> impl Fn(&str, &Value) -> Result<(), String> + '_ {
        |event, value| {
            seen.borrow_mut().push((event.to_string(), value.clone()));
            Err("가짜 보내기 실패".to_string())
        }
    }

    /// 프론트처럼 곱을 곧바로 흘려 보내는 가짜 — 값을 보고 나서(`id` 는 payload 에서 읽는다) 성공한다
    fn done_emit<'a>(
        inbox: &'a Mutex<Inbox>,
        tab: Option<&'a str>,
        session: Option<&'a str>,
    ) -> impl Fn(&str, &Value) -> Result<(), String> + 'a {
        move |_, value| {
            let id = value["id"].as_str().unwrap_or_default().to_string();
            inbox
                .lock()
                .unwrap()
                .send_done(&id, tab.map(|tab| tab.to_string()), session.map(|session| session.to_string()));
            Ok(())
        }
    }

    /// 아무 곱도 보내지 않는 가짜 — 기다림이 끝나 거절이 나온다
    fn silent_emit() -> impl Fn(&str, &Value) -> Result<(), String> {
        |_, _| Ok(())
    }

    const CWD: &str = "C:\\work";
    const SID_UPPER: &str = "0F0E1D2C-3B4A-5968-7766-554433221100";
    const SID: &str = "0f0e1d2c-3b4a-5968-7766-554433221100";

    #[test]
    fn open_event_carries_front_end_keys() {
        // 이 시험이 빨개지는 길 — 이벤트 이름·camelCase 키 하나라도 바뀌면 프론트가 읽지 못해 곱이 없다
        let inbox = Mutex::new(Inbox::default());
        let seen = RefCell::new(Vec::new());
        let action = OpenTabAction::Open {
            cwd: CWD.to_string(),
            title: Some("역할".to_string()),
            command: argv(&["claude", "--plugin-dir", "C:\\md\\plugins\\md-render", "--name", "역할"]),
            no_focus: true,
        };
        let why = act_with(&inbox, 0.2, &temp_claude_dir("keys"), &[], &action, &capture_emit(&seen));
        let (event, value) = seen.borrow().first().expect("보낸 값이 있어야 한다").clone();
        assert_eq!(event, "open-tab-request");
        assert_eq!(value["cwd"], json!(CWD));
        assert_eq!(value["title"], json!("역할"));
        // 답 꾸미기용 md-render 플러그인 폴더 인자는 지우고 프론트가 자기 폴더를 맨 뒤에 다시 넣는다
        assert_eq!(value["command"], json!(["claude", "--name", "역할"]));
        assert_eq!(value["reuseTab"], json!(null));
        assert_eq!(value["noFocus"], json!(true));
        assert!(!value["id"].as_str().expect("요청 id").is_empty());
        // 보내기가 실패하면 곱을 기다리지 않고 바로 거절 문구로 끝낸다
        assert!(why.unwrap().contains("보내지 못했습니다"));
    }

    #[test]
    fn focus_event_is_a_bare_reveal() {
        // 가리기 — 명령 없이 화면을 앞으로 갖다 놓는다(프론트는 빈 명령을 가리기로 본다)
        let inbox = Mutex::new(Inbox::default());
        let seen = RefCell::new(Vec::new());
        let snapshot = vec![screen("t1", CWD)];
        let action = OpenTabAction::Focus { id: "t1".to_string() };
        let _ = act_with(&inbox, 0.2, &temp_claude_dir("focus"), &snapshot, &action, &capture_emit(&seen));
        let value = seen.borrow().first().expect("보낸 값이 있어야 한다").1.clone();
        assert_eq!(value["command"], json!([]));
        assert_eq!(value["title"], json!(null));
        assert_eq!(value["reuseTab"], json!("t1"));
        assert_eq!(value["noFocus"], json!(false));
        assert_eq!(value["cwd"], json!(CWD));
    }

    #[test]
    fn done_with_tab_fulfills_the_opening() {
        // 곱에 탭이 있으면 프론트가 열거나 가린 것 — 이행(None), 기다리는 자리는 치워진다
        let claude_dir = temp_claude_dir("fulfill");
        put_transcript(&claude_dir, CWD, SID);
        let inbox = Mutex::new(Inbox::default());
        let snapshot = vec![screen("t1", CWD)];
        let action = OpenTabAction::Reuse {
            id: "t1".to_string(),
            command: argv(&["claude", "--resume", SID]),
            no_focus: false,
        };
        let why = act_with(&inbox, 0.2, &claude_dir, &snapshot, &action, &done_emit(&inbox, Some("역할"), Some("abc")));
        assert_eq!(why, None);
        assert!(inbox.lock().unwrap().waiters.is_empty());
    }

    #[test]
    fn done_without_tab_refuses() {
        // 곱에 탭이 없으면 프론트가 못 연 것 — 탭을 열지 못했다고 거절한다
        let inbox = Mutex::new(Inbox::default());
        let action = OpenTabAction::Open {
            cwd: CWD.to_string(),
            title: Some("역할".to_string()),
            command: argv(&["claude", "--continue"]),
            no_focus: false,
        };
        let why = act_with(&inbox, 0.2, &temp_claude_dir("no-tab"), &[], &action, &done_emit(&inbox, None, None));
        assert_eq!(why, Some("탭을 열지 못했습니다".to_string()));
        assert!(inbox.lock().unwrap().waiters.is_empty());
    }

    #[test]
    fn late_done_refuses_after_wait() {
        // 답이 늦으면 기다림이 끝나 거절 문구로 끝나고, 기다리는 자리는 치워져 뒤늦은 곱을 잊는다
        let inbox = Mutex::new(Inbox::default());
        let action = OpenTabAction::Open {
            cwd: CWD.to_string(),
            title: None,
            command: argv(&["claude"]),
            no_focus: false,
        };
        let why = act_with(&inbox, 0.05, &temp_claude_dir("late"), &[], &action, &silent_emit());
        let why = why.expect("기다림이 끝나면 거절 문구가 나온다");
        assert!(why.contains("안에 오지 않았습니다"));
        // 자리가 치워졌는지 — 뒤늦은 곱을 보내도 무시되고 기다리는 자리는 그대로 비어 있다
        inbox.lock().unwrap().send_done("no-such-id", Some("늦은 탭".to_string()), None);
        assert!(inbox.lock().unwrap().waiters.is_empty());
    }

    #[test]
    fn missing_transcript_refuses_resume() {
        // 이어 열지만 대화 기록이 없으면 — claude 가 바로 끝나므로 띄우지 않고 이유를 돌려준다(mac noTranscript)
        let claude_dir = temp_claude_dir("no-transcript");
        let inbox = Mutex::new(Inbox::default());
        let action = OpenTabAction::Open {
            cwd: CWD.to_string(),
            title: None,
            command: argv(&["claude", "--resume", SID]),
            no_focus: false,
        };
        // 거절은 프론트에 요청을 보내기 전에 끝난다 — 보낸 값이 비어 있어야 한다
        let seen = RefCell::new(Vec::new());
        let why = act_with(&inbox, 0.2, &claude_dir, &[], &action, &capture_emit(&seen));
        let why = why.expect("대화 기록이 없으면 거절 문구가 나온다");
        assert!(why.contains("이어 열 대화 기록이 없습니다"));
        assert!(why.contains(SID));
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn missing_transcript_leaves_fork_alone() {
        // fork 세션은 기록이 없어도 새로 잡는다(mac과 같다) — 검사를 뛰어 넘어 기다림까지 간다
        let claude_dir = temp_claude_dir("fork");
        let inbox = Mutex::new(Inbox::default());
        let action = OpenTabAction::Open {
            cwd: CWD.to_string(),
            title: None,
            command: argv(&["claude", "--resume", SID, "--fork-session"]),
            no_focus: false,
        };
        let why = act_with(&inbox, 0.05, &claude_dir, &[], &action, &silent_emit());
        let why = why.expect("기다림이 끝나면 거절 문구가 나온다");
        assert!(why.contains("안에 오지 않았습니다"));
        assert!(!why.contains("이어 열 대화 기록이 없습니다"));
    }

    #[test]
    fn transcript_lookup_uses_lowercase_sid() {
        // 사용자가 대문자로 세션 id 를 보내도 claude 가 쓰는 파일 이름은 소문자다 — 소문자 파일로 찾는다
        let claude_dir = temp_claude_dir("lowercase");
        put_transcript(&claude_dir, CWD, SID);
        let inbox = Mutex::new(Inbox::default());
        let action = OpenTabAction::Open {
            cwd: CWD.to_string(),
            title: None,
            command: argv(&["claude", "--resume", SID_UPPER]),
            no_focus: false,
        };
        let why = act_with(&inbox, 0.2, &claude_dir, &[], &action, &done_emit(&inbox, Some("탭"), None));
        assert_eq!(why, None);
    }
}
