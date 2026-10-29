//! 끌 때 확인과 탭 목록 저장 — 트레이 "종료"·업데이트 직전에 도는 세션을 보여 주고 확인을 받는다(mac `confirmShutdown`),
//! 다시 뜰 때 이어 열 탭 목록을 설정에 남긴다(mac `saveTabs`·`restoreTabs`).
//!
//! 확인 창은 보드 창 안의 Svelte 모달이다. 백엔드가 `board://confirm-shutdown` 을 보내면 프론트가 목록을 만들어(규칙은 프론트
//! `terminal/shutdownList.ts`) 줄이 없으면 바로 `answer_shutdown` 으로 답하고, 줄이 있으면 `shutdown_dialog_shown` 으로 창을 띄웠다고 알린 뒤
//! 사용자가 고르면 `answer_shutdown` 으로 답한다. 백엔드는 답을 받아 `.paused` 표지를 쓰고(`terminal.rs paused_mark` 경로) 끝낸다.
//!
//! 기다림은 세 갈래로 끝난다. 이벤트를 보낸 뒤 프론트가 반응(답 또는 "창을 띄움")하기를 `REACTION_TIMEOUT` 동안 기다리고, 반응이 없으면
//! 프론트가 없거나 멎은 것이라 로그를 남기고 그대로 진행한다. "창을 띄움" 뒤에는 사용자가 고를 때까지 기다리되 `ANSWER_TIMEOUT` 를 넘기면
//! 취소하고 창을 닫게 알린다(화면이 죽었는데 영원히 기다리지 않게). 새 요청(트레이 "종료"를 또 누름)이 오면 기다리던 요청을 취소하고 새 요청이
//! 이어받는다. 이벤트를 다시 보내 창을 다시 띄우므로 화면이 새로 고쳐졌거나 죽었어도 사용자가 한 번 더 눌러 빠져나간다. 프론트가 시작할 때
//! (`lifecycle_started`) 기다리던 요청도 취소한다. 이벤트를 받을 화면이 바뀌었기 때문이다.
//! 프론트가 수명 주기를 아직 켜지 않았으면(`lifecycle_started` 를 한 번도 부르지 않음 = 이벤트를 듣는 화면이 없다) 묻지 않고 지나간다.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use claude_board_core::clock;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::board::Board;
use crate::util::board_log;

/// 끄기 전에 프론트에 확인 창을 띄우라고 보내는 이벤트 — 내용은 `types.ts ConfirmShutdownPayload`(목록은 프론트가 채운다).
/// `reason` 이 `cancel` 이면 떠 있는 확인 창을 닫으라는 뜻이다.
pub const CONFIRM_SHUTDOWN_EVENT: &str = "board://confirm-shutdown";

/// 이벤트를 보낸 뒤 프론트의 첫 반응을 기다리는 시간 — 넘으면 프론트가 없는 것으로 보고 진행한다(판단값).
const REACTION_TIMEOUT: Duration = Duration::from_secs(10);

/// 확인 창을 띄운 뒤 사용자의 답을 기다리는 시간의 상한 — 사람이 읽고 고르는 시간을 끊지 않을 만큼 길게 잡되 무한히 기다리지는 않는다(판단값).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// 기다림의 두 상한 — 반응(답 또는 "창을 띄움")까지, 창을 띄운 뒤 답까지.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub reaction: Duration,
    pub answer: Duration,
}

const TIMEOUTS: Timeouts = Timeouts { reaction: REACTION_TIMEOUT, answer: ANSWER_TIMEOUT };

/// 끄는 까닭 — 이벤트의 `reason` 이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Quit,
    Update,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Reason::Quit => "quit",
            Reason::Update => "update",
        }
    }
}

/// 프론트가 확인 창에서 고른 것.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub ok: bool,
    /// `.paused` 를 남길 세션 id
    pub pause: Vec<String>,
}

/// 확인 요청의 끝.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Answered(Answer),
    /// 프론트가 `reaction` 안에 반응하지 않았다 — 프론트가 없거나 멎었다
    TimedOut,
    /// 창은 떴지만 사용자가 `answer` 안에 고르지 않았다
    AnswerTimedOut,
    /// 새 요청이 이어받았거나 프론트가 다시 시작해 이 요청의 화면이 사라졌다
    Cancelled,
}

enum Message {
    Shown,
    Answer(Answer),
    /// 새 요청이 이어받음 또는 프론트가 다시 시작함
    Cancel,
}

struct Waiting {
    id: u64,
    sender: Sender<Message>,
}

/// 확인 요청과 프론트의 답을 이어 주는 한 칸짜리 우편함. 한 번에 요청 하나만 기다리고, 새 요청은 기다리던 요청을 취소하고 이어받는다.
pub struct Handshake {
    waiting: Mutex<Option<Waiting>>,
    next_id: AtomicU64,
}

impl Handshake {
    pub const fn new() -> Self {
        Handshake { waiting: Mutex::new(None), next_id: AtomicU64::new(0) }
    }

    /// `emit` 으로 요청을 보내고 답을 기다린다. 우편함은 `emit` 전에 열어 두므로 답이 먼저 와도 놓치지 않는다.
    /// `emit` 의 인자는 기다리던 요청을 이어받았는가다.
    pub fn ask(&self, emit: impl FnOnce(bool), timeouts: Timeouts) -> Outcome {
        let (sender, receiver) = channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let took_over = {
            let mut waiting = self.waiting.lock().unwrap();
            let old = waiting.replace(Waiting { id, sender });
            if let Some(old) = &old {
                let _ = old.sender.send(Message::Cancel);
            }
            old.is_some()
        };
        emit(took_over);
        let mut deadline = Instant::now() + timeouts.reaction;
        let mut shown = false;
        let outcome = loop {
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Message::Answer(answer)) => break Outcome::Answered(answer),
                Ok(Message::Shown) if !shown => {
                    shown = true;
                    deadline = Instant::now() + timeouts.answer;
                }
                Ok(Message::Shown) => {}
                Ok(Message::Cancel) | Err(RecvTimeoutError::Disconnected) => break Outcome::Cancelled,
                Err(RecvTimeoutError::Timeout) => break if shown { Outcome::AnswerTimedOut } else { Outcome::TimedOut },
            }
        };
        let mut waiting = self.waiting.lock().unwrap();
        if waiting.as_ref().is_some_and(|w| w.id == id) {
            *waiting = None;
        }
        outcome
    }

    fn send(&self, message: Message) -> bool {
        self.waiting.lock().unwrap().as_ref().is_some_and(|w| w.sender.send(message).is_ok())
    }

    /// 프론트의 답을 기다리는 요청에 전한다. 기다리는 요청이 없으면 false.
    pub fn answer(&self, answer: Answer) -> bool {
        self.send(Message::Answer(answer))
    }

    /// 프론트가 확인 창을 띄웠다고 알린다 — 이 뒤로는 사용자가 고를 때까지 기다린다.
    pub fn shown(&self) -> bool {
        self.send(Message::Shown)
    }

    /// 기다리는 요청을 취소한다 — 프론트가 다시 시작해 그 요청의 화면이 사라졌다. 기다리던 요청이 있었으면 true.
    pub fn cancel(&self) -> bool {
        match self.waiting.lock().unwrap().take() {
            Some(old) => old.sender.send(Message::Cancel).is_ok(),
            None => false,
        }
    }
}

static HANDSHAKE: Handshake = Handshake::new();

/// 프론트가 `lifecycle_started` 를 불렀다 = 확인 이벤트를 듣는 화면이 있다.
static LIFECYCLE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 끄거나 업데이트를 진행해도 되는지와 `.paused` 를 남길 세션 — 순수한 판단이다.
#[derive(Debug, PartialEq, Eq)]
pub struct Decision {
    pub proceed: bool,
    pub pause: Vec<String>,
    /// 사용자에게 보이지 않는 사정이면 board.log 에 남길 한 줄 — 진행하지 않거나 묻지 않고 진행한 까닭은 늘 남긴다
    pub log: Option<&'static str>,
    /// 떠 있을지 모르는 확인 창을 닫으라고 프론트에 알린다 — 답 없이 끝난 요청
    pub close_dialog: bool,
}

pub fn decide(outcome: Outcome) -> Decision {
    let plain = |proceed, log| Decision { proceed, pause: Vec::new(), log, close_dialog: false };
    match outcome {
        Outcome::Answered(Answer { ok: true, pause }) => Decision { proceed: true, pause, log: None, close_dialog: false },
        Outcome::Answered(Answer { ok: false, .. }) => plain(false, None),
        Outcome::TimedOut => plain(true, Some("끌 때 확인에 프론트가 답하지 않아 그대로 진행합니다")),
        Outcome::AnswerTimedOut => Decision {
            close_dialog: true,
            ..plain(false, Some("끌 때 확인 창에 오래 답이 없어 취소합니다(보드는 그대로 둡니다)"))
        },
        Outcome::Cancelled => plain(false, Some("끌 때 확인이 새 요청이나 화면 새로 고침으로 취소되었습니다")),
    }
}

/// 끄기 전에 확인을 받는다. 계속해도 되면 새로 남긴 `.paused` 표지 경로들(없으면 빈 목록), 취소하거나 답 없이 끝났으면 None.
fn confirm(app: &AppHandle, reason: Reason) -> Option<Vec<PathBuf>> {
    if !LIFECYCLE_ACTIVE.load(Ordering::SeqCst) {
        return Some(Vec::new());
    }
    let send = |reason: &str| {
        if let Err(e) = app.emit(CONFIRM_SHUTDOWN_EVENT, json!({"reason": reason, "items": []})) {
            board_log(&format!("끌 때 확인 이벤트를 보내지 못했습니다: {e}"));
        }
    };
    let outcome = HANDSHAKE.ask(
        |took_over| {
            if took_over {
                // 기다리던 확인이 있는데 또 눌렀다 — 숨어 있던 보드를 보이고 창을 다시 띄우게 한다
                board_log("끌 때 확인이 이미 열려 있어 새 요청이 이어받고 창을 다시 보입니다");
                crate::window::show(app, true);
            }
            send(reason.as_str());
        },
        TIMEOUTS,
    );
    let decision = decide(outcome);
    if let Some(line) = decision.log {
        board_log(line);
    }
    if decision.close_dialog {
        send("cancel");
    }
    if !decision.proceed {
        return None;
    }
    let marks = match app.try_state::<Arc<Board>>() {
        Some(board) if !decision.pause.is_empty() => write_marks(&board, &decision.pause),
        _ => Vec::new(),
    };
    Some(marks)
}

/// 트레이 "종료". 도는 세션이 있으면 확인을 받은 뒤 끝낸다. 메인 스레드를 막지 않게 따로 돈다.
pub fn quit(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if confirm(&app, Reason::Quit).is_some() {
            app.exit(0);
        }
    });
}

/// 업데이트를 시작해도 되는지. 도는 세션이 있으면 확인을 받아 취소하면 None, 계속이면 새로 남긴 `.paused` 표지들.
/// 업데이트 스크립트가 보드를 끄지 못하고 돌아오면 호출한 쪽이 `remove_marks` 로 그 표지를 지운다. 호출한 스레드가 답이 올 때까지 막힌다.
pub fn confirm_update(board: &Board) -> Option<Vec<PathBuf>> {
    confirm(&board.app, Reason::Update)
}

/// 남긴 표지 파일들을 지운다 — 보드를 끄지 못하고 끝난 업데이트의 일시중지 표지(세션은 그대로 도는 중이다). 없는 파일은 건너뛴다.
pub fn remove_marks(marks: &[PathBuf]) {
    for mark in marks {
        let _ = fs::remove_file(mark);
    }
}

/// 일시중지 표지를 남겨도 되는 세션인가 — 사람이 입력하는 대화형 주 세션만이다(끝난 세션·`-p`·`--bg` 는 입력 자리가 없다).
pub fn pausable(state: &str, headless: bool, mode: Option<&str>) -> bool {
    state != "ended" && !headless && mode.is_none()
}

/// 표지 파일 하나를 쓴다. 이미 있으면(사용자가 일시중지해 둔 세션) 덮지 않고 false.
pub fn write_mark(path: &Path, paused_at: f64, topic: &str) -> bool {
    use std::io::Write;
    let body = serde_json::to_vec(&json!({"paused_at": paused_at, "topic": topic})).unwrap_or_default();
    match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(&body).is_ok(),
        Err(_) => false,
    }
}

/// 프론트가 고른 세션들에 `.paused` 를 남긴다. 알려진 세션이 아니거나 `pausable` 이 아니면 건너뛴다. 새로 쓴 표지 경로들을 돌려준다.
fn write_marks(board: &Arc<Board>, ids: &[String]) -> Vec<PathBuf> {
    let now = clock::now();
    let mut written = Vec::new();
    for id in ids {
        let Some(session) = board.session(id) else { continue };
        if !pausable(&session.state, session.headless, session.place.mode) {
            continue;
        }
        let path = crate::terminal::paused_mark(board, &session);
        if write_mark(&path, now, &session.topic) {
            written.push(path);
        }
    }
    if !written.is_empty() {
        board.publish();
    }
    written
}

/// 프론트의 `answerShutdown(ok, pause)` — 확인 창의 답. `pause` 는 `.paused` 를 남길 세션 id(`ok` 일 때만 쓰인다).
#[tauri::command(async)]
pub fn answer_shutdown(ok: bool, pause: Vec<String>) {
    if !HANDSHAKE.answer(Answer { ok, pause }) {
        board_log("기다리는 끌 때 확인이 없는데 답이 왔습니다");
    }
}

/// 프론트의 `shutdownDialogShown()` — 확인 창을 띄웠다. 이 뒤로는 사용자가 고를 때까지 기다린다.
#[tauri::command(async)]
pub fn shutdown_dialog_shown() {
    HANDSHAKE.shown();
}

/// 프론트의 `lifecycleStarted()` — 화면이 (다시) 시작해 확인 이벤트 듣기를 켰다. 이 뒤부터 끌 때 확인을 건다.
/// 그 전에 보낸 확인 요청의 화면은 사라졌으므로 기다리던 요청은 취소한다.
#[tauri::command(async)]
pub fn lifecycle_started() {
    LIFECYCLE_ACTIVE.store(true, Ordering::SeqCst);
    if HANDSHAKE.cancel() {
        board_log("화면이 다시 시작되어 기다리던 끌 때 확인을 취소했습니다");
    }
}

/// 주어진 경로 가운데 실제로 있는 폴더(입력 문자열 그대로, 입력 순서). 상대 경로는 어느 폴더인지 모르므로 없는 것으로 본다.
pub fn dirs_among(paths: &[String], is_dir: impl Fn(&Path) -> bool) -> Vec<String> {
    paths.iter().filter(|p| Path::new(p.as_str()).is_absolute() && is_dir(Path::new(p.as_str()))).cloned().collect()
}

/// 프론트의 `existingDirs(paths)` — 탭을 되살릴 때 저장해 둔 작업 폴더가 아직 있는지. 없는 폴더에서는 셸이 홈 폴더로 떠 버려 claude 를 거기서 띄우게 된다.
#[tauri::command(async)]
pub fn existing_dirs(paths: Vec<String>) -> Vec<String> {
    dirs_among(&paths, Path::is_dir)
}

/// 프론트의 `saveTabs(entries)` — 다시 뜰 때 이어 열 탭 목록을 설정(`terminalTabs`)에 통째로 바꿔 넣는다. 항목 모양은 프론트가 쥔다.
#[tauri::command(async)]
pub fn save_tabs(board: State<'_, Arc<Board>>, entries: Vec<Value>) {
    board.update_settings(|s| s.terminal_tabs = entries);
}

/// 프론트의 `loadTabs()` — 저장해 둔 탭 목록. 없으면 빈 목록.
#[tauri::command(async)]
pub fn load_tabs(board: State<'_, Arc<Board>>) -> Vec<Value> {
    board.settings.lock().unwrap().terminal_tabs.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    const FAST: Duration = Duration::from_millis(50);
    const LONG: Duration = Duration::from_secs(5);

    fn limits(reaction: Duration, answer: Duration) -> Timeouts {
        Timeouts { reaction, answer }
    }

    fn answer(ok: bool, pause: &[&str]) -> Answer {
        Answer { ok, pause: pause.iter().map(|id| id.to_string()).collect() }
    }

    #[test]
    fn 답이_오면_그대로_돌려준다() {
        let handshake = Arc::new(Handshake::new());
        let sender = handshake.clone();
        let outcome = handshake.ask(
            move |_| {
                thread::spawn(move || {
                    assert!(sender.answer(answer(true, &["a"])));
                });
            },
            limits(LONG, LONG),
        );
        assert_eq!(outcome, Outcome::Answered(answer(true, &["a"])));
    }

    #[test]
    fn 이벤트를_보내는_중에_온_답도_놓치지_않는다() {
        let handshake = Arc::new(Handshake::new());
        let sender = handshake.clone();
        let outcome = handshake.ask(move |_| assert!(sender.answer(answer(false, &[]))), limits(LONG, LONG));
        assert_eq!(outcome, Outcome::Answered(answer(false, &[])));
    }

    #[test]
    fn 반응이_없으면_시간이_지난_뒤_시간초과() {
        let handshake = Handshake::new();
        let started = Instant::now();
        let outcome = handshake.ask(|_| {}, limits(FAST, LONG));
        assert_eq!(outcome, Outcome::TimedOut);
        assert!(started.elapsed() >= FAST);
        assert!(started.elapsed() < LONG, "답 상한까지 기다리면 안 된다");
    }

    #[test]
    fn 창을_띄운_뒤에는_반응_시간을_넘겨도_사용자의_답을_기다린다() {
        let handshake = Arc::new(Handshake::new());
        let sender = handshake.clone();
        let outcome = handshake.ask(
            move |_| {
                thread::spawn(move || {
                    assert!(sender.shown());
                    thread::sleep(FAST * 4);
                    assert!(sender.answer(answer(true, &["b"])));
                });
            },
            limits(FAST, LONG),
        );
        assert_eq!(outcome, Outcome::Answered(answer(true, &["b"])));
    }

    #[test]
    fn 창을_띄운_뒤_화면이_죽어_답이_없으면_답_상한에_취소된다() {
        // 렌더러가 죽은 경우 — "창을 띄움"까지만 오고 그 뒤로 아무 말이 없다
        let handshake = Arc::new(Handshake::new());
        let sender = handshake.clone();
        let started = Instant::now();
        let outcome = handshake.ask(move |_| assert!(sender.shown()), limits(FAST, FAST * 3));
        assert_eq!(outcome, Outcome::AnswerTimedOut);
        assert!(started.elapsed() >= FAST * 3, "반응 상한이 아니라 답 상한까지 기다린다");
        assert!(started.elapsed() < LONG);
        assert!(!handshake.answer(answer(true, &[])), "끝난 요청의 우편함은 비어 있다");
    }

    #[test]
    fn 창을_띄운_알림이_거듭_와도_답_상한은_다시_시작하지_않는다() {
        let handshake = Arc::new(Handshake::new());
        let sender = handshake.clone();
        let started = Instant::now();
        let outcome = handshake.ask(
            move |_| {
                thread::spawn(move || {
                    for _ in 0..8 {
                        sender.shown();
                        thread::sleep(FAST);
                    }
                });
            },
            limits(LONG, FAST * 3),
        );
        assert_eq!(outcome, Outcome::AnswerTimedOut);
        assert!(started.elapsed() < FAST * 7, "반복된 알림이 답 상한을 늘리면 안 된다");
    }

    #[test]
    fn 요청이_끝나면_우편함이_비어_다음_요청을_받는다() {
        let handshake = Handshake::new();
        assert_eq!(handshake.ask(|_| {}, limits(Duration::from_millis(10), LONG)), Outcome::TimedOut);
        assert!(!handshake.answer(answer(true, &[])), "기다리는 요청이 없으면 답을 받지 않는다");
        assert_eq!(handshake.ask(|_| {}, limits(Duration::from_millis(10), LONG)), Outcome::TimedOut);
    }

    #[test]
    fn 새_요청은_기다리던_요청을_취소하고_이어받는다() {
        let handshake = Arc::new(Handshake::new());
        let first_handshake = handshake.clone();
        let (first_started, first_ready) = channel();
        let first = thread::spawn(move || {
            first_handshake.ask(
                move |took_over| {
                    assert!(!took_over);
                    first_started.send(()).unwrap();
                },
                limits(LONG, LONG),
            )
        });
        first_ready.recv().unwrap();
        let second_handshake = handshake.clone();
        let mut took_over_seen = false;
        let second = handshake.ask(
            |took_over| {
                took_over_seen = took_over;
                let sender = second_handshake;
                thread::spawn(move || {
                    assert!(sender.answer(answer(true, &["c"])));
                });
            },
            limits(LONG, LONG),
        );
        assert!(took_over_seen, "둘째는 이어받았다고 알린다");
        assert_eq!(first.join().unwrap(), Outcome::Cancelled);
        assert_eq!(second, Outcome::Answered(answer(true, &["c"])), "이어받은 요청이 답을 받는다");
    }

    #[test]
    fn 이어받은_뒤_첫_요청이_끝나도_둘째의_우편함을_지우지_않는다() {
        let handshake = Arc::new(Handshake::new());
        let first_handshake = handshake.clone();
        let (first_started, first_ready) = channel();
        let first = thread::spawn(move || first_handshake.ask(move |_| first_started.send(()).unwrap(), limits(LONG, LONG)));
        first_ready.recv().unwrap();
        let second_handshake = handshake.clone();
        let second = thread::spawn(move || second_handshake.ask(|_| {}, limits(LONG, LONG)));
        assert_eq!(first.join().unwrap(), Outcome::Cancelled);
        // 첫 요청이 정리를 끝낸 뒤에도 둘째가 답을 받을 수 있어야 한다
        for _ in 0..200 {
            if handshake.answer(answer(false, &[])) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(second.join().unwrap(), Outcome::Answered(answer(false, &[])));
    }

    #[test]
    fn 화면이_다시_시작하면_기다리던_요청이_취소된다() {
        let handshake = Arc::new(Handshake::new());
        let resetter = handshake.clone();
        let outcome = handshake.ask(
            move |_| {
                thread::spawn(move || {
                    assert!(resetter.shown());
                    assert!(resetter.cancel());
                });
            },
            limits(LONG, LONG),
        );
        assert_eq!(outcome, Outcome::Cancelled);
        assert!(!handshake.cancel(), "기다리는 요청이 없으면 취소할 것도 없다");
        assert_eq!(handshake.ask(|_| {}, limits(Duration::from_millis(10), LONG)), Outcome::TimedOut, "취소 뒤에도 다음 요청을 받는다");
    }

    #[test]
    fn 판단_계속을_고르면_진행하고_표지_대상을_넘긴다() {
        let d = decide(Outcome::Answered(answer(true, &["a", "b"])));
        assert_eq!(d, Decision { proceed: true, pause: vec!["a".into(), "b".into()], log: None, close_dialog: false });
    }

    #[test]
    fn 판단_취소하면_표지도_남기지_않는다() {
        let d = decide(Outcome::Answered(answer(false, &["a"])));
        assert_eq!(d, Decision { proceed: false, pause: vec![], log: None, close_dialog: false });
    }

    #[test]
    fn 판단_반응이_없으면_로그를_남기고_진행한다() {
        let d = decide(Outcome::TimedOut);
        assert!(d.proceed);
        assert!(d.pause.is_empty());
        assert!(d.log.is_some());
        assert!(!d.close_dialog);
    }

    #[test]
    fn 판단_창을_띄운_뒤_답이_없으면_진행하지_않고_창을_닫는다() {
        let d = decide(Outcome::AnswerTimedOut);
        assert!(!d.proceed, "사용자가 고르지 않은 채 보드를 끄지 않는다");
        assert!(d.log.is_some());
        assert!(d.close_dialog);
    }

    #[test]
    fn 판단_취소된_요청은_진행하지_않고_로그를_남긴다() {
        let d = decide(Outcome::Cancelled);
        assert!(!d.proceed);
        assert!(d.log.is_some(), "조용히 무시하지 않는다");
    }

    #[test]
    fn 사람이_입력하는_주_세션만_표지를_받는다() {
        assert!(pausable("idle", false, None));
        assert!(pausable("working", false, None));
        assert!(pausable("waiting", false, None));
        assert!(!pausable("ended", false, None));
        assert!(!pausable("working", true, Some("-p")));
        assert!(!pausable("working", false, Some("-p")));
        assert!(!pausable("idle", false, Some("--bg")));
    }

    #[test]
    fn 표지_파일은_모양을_지키고_이미_있으면_덮지_않는다() {
        let dir = std::env::temp_dir().join(format!("cb-shutdown-{}", crate::util::short_id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s1.paused");
        assert!(write_mark(&path, 12.5, "주제"));
        let mark: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(mark, json!({"paused_at": 12.5, "topic": "주제"}));
        assert!(!write_mark(&path, 99.0, "다른"), "있는 표지는 덮지 않는다");
        let again: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(again["paused_at"], 12.5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 남긴_표지는_지워지고_없는_파일은_건너뛴다() {
        let dir = std::env::temp_dir().join(format!("cb-shutdown-rm-{}", crate::util::short_id()));
        fs::create_dir_all(&dir).unwrap();
        let (kept, gone) = (dir.join("a.paused"), dir.join("b.paused"));
        assert!(write_mark(&kept, 1.0, "a"));
        remove_marks(&[kept.clone(), gone, dir.join("missing.paused")]);
        assert!(!kept.exists(), "업데이트가 보드를 못 끄고 돌아오면 남긴 표지를 지운다");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 있는_폴더만_골라_돌려주고_상대_경로는_없는_것으로_본다() {
        let paths: Vec<String> = ["C:\\here", "C:\\gone", "rel\\here", "C:\\here"].iter().map(|p| p.to_string()).collect();
        let found = dirs_among(&paths, |p| p.to_string_lossy().ends_with("here"));
        if cfg!(windows) {
            assert_eq!(found, ["C:\\here", "C:\\here"]);
        }
        assert!(!found.iter().any(|p| p.starts_with("rel")), "상대 경로는 어느 폴더인지 모른다");
        assert!(dirs_among(&[], Path::is_dir).is_empty());
        let me = std::env::temp_dir().to_string_lossy().into_owned();
        assert_eq!(dirs_among(&[me.clone(), format!("{me}\\cb-nope-{}", crate::util::short_id())], Path::is_dir), [me]);
    }

    #[test]
    fn 이벤트_이름과_까닭_글자는_프론트와_같다() {
        assert_eq!(CONFIRM_SHUTDOWN_EVENT, "board://confirm-shutdown");
        assert_eq!(Reason::Quit.as_str(), "quit");
        assert_eq!(Reason::Update.as_str(), "update");
    }
}
