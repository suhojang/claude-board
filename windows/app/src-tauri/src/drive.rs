//! 세션 구동 (W3 설계 §3.1) — 일시중지·재개·중단·이름 바꾸기가 세션 프로세스에 키를 넣는 길.
//! 위쪽은 순수 규칙(`drive_target`·`encode`·`steps_for`)과 실행 순서(`run`), 아래쪽은 Win32·터미널 허브에 잇는 어댑터다.
//!
//! 보드 터미널 탭의 세션은 `TermHub::write` 로 원시 바이트를 쓴다. 보드가 연 ConPTY 에 붙은 pid 에는 `AttachConsole` 하지
//! 않는다(자기 ConPTY 에 붙으면 보드가 죽을 수 있다 — W2 R2). 보드 밖 콘솔은 콘솔 입력 버퍼에, 키를 넣을 수 없는 세션은
//! 중단할 때만 프로세스를 끝낸다.

use std::sync::Arc;
use std::time::Duration;

use claude_board_core::console::{self, Input};
use claude_board_core::{paths, proc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Manager, State};

use crate::board::Board;
use crate::model::{PidPlace, Session};
use crate::session_action::SessionAction;
use crate::terms::TermHub;
use crate::util::board_log;

pub const CARRY_ON_PROMPT: &str = "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요.";

/// 한 번에 기다릴 수 있는 상한(ms) — 프론트가 보낸 단계가 보드를 오래 붙잡지 않게.
const MAX_WAIT_MS: u64 = 5000;
/// 한 요청의 기다림 합계 상한(ms).
const MAX_TOTAL_WAIT_MS: u64 = 10_000;

/// 입력 한 단계.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Escape,
    Enter,
    Text(String),
    /// 밀리초 쉰다.
    Wait(u64),
}

/// 무엇을 위해 구동하는가 — 중단만 키를 못 넣을 때 프로세스를 끝내는 길이 열린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Keys,
    Stop,
}

/// 입력이 닿을 곳.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// 보드 터미널(`TermId`)에 원시 바이트.
    Surface(String),
    /// 보드 밖 콘솔(pid)의 입력 버퍼.
    Console(u32),
    /// 키를 넣을 수 없어 프로세스를 끝낸다(중단만).
    Kill(u32),
    None,
}

/// 세션을 어디로 구동하나. 처음 맞는 줄이 이긴다:
/// 끝났거나 pid 0 → None · 보드 터미널 짝(`tty`)이 있으면 그 탭에서 claude 가 앞에서 도는 대화형일 때만 Surface, 아니면 중단만 Kill ·
/// headless 가 탭 조상 아래면 중단만 Kill · 그 밖 headless 는 None(보드가 신호를 보내지 않는다) ·
/// 콘솔에 붙을 수 있으면(`attachable`) Console · 아니면 중단만 Kill.
pub fn drive_target(s: &Session, runs_claude: &[String], attachable: bool, purpose: Purpose) -> Target {
    target(&s.state, s.pid, s.headless, &s.place, runs_claude, attachable, purpose)
}

/// 메뉴가 쓰는 `canDrive` — 키를 넣을 곳이 있다. 세션을 다 짓기 전에 쓰도록 낱개 사실을 받는다(규칙은 `drive_target` 하나).
pub fn can_drive(state: &str, pid: u32, headless: bool, place: &PidPlace, runs_claude: &[String], attachable: bool) -> bool {
    target(state, pid, headless, place, runs_claude, attachable, Purpose::Keys) != Target::None
}

fn target(state: &str, pid: u32, headless: bool, place: &PidPlace, runs_claude: &[String], attachable: bool, purpose: Purpose) -> Target {
    if state == "ended" || pid == 0 {
        return Target::None;
    }
    let stop_only = || if purpose == Purpose::Stop { Target::Kill(pid) } else { Target::None };
    if let Some(t) = &place.tty {
        let interactive = place.mode.is_none() && !headless;
        return if interactive && runs_claude.contains(t) { Target::Surface(t.clone()) } else { stop_only() };
    }
    if headless {
        return if place.tab_ancestor.is_some() { stop_only() } else { Target::None };
    }
    if attachable {
        Target::Console(pid)
    } else {
        stop_only()
    }
}

/// 한 단계의 바이트. 글에 제어 문자(`\r`·`\n`·Esc·`\x03` 같은 C0)가 있으면 거부한다 —
/// 키로 해석돼 다른 일을 하게 되므로 조용히 고치지 않고 멈춘다. `Wait` 는 바이트가 없다.
pub fn encode(step: &Step) -> Result<Vec<u8>, String> {
    match step {
        Step::Escape => Ok(vec![0x1b]),
        Step::Enter => Ok(vec![b'\r']),
        Step::Wait(_) => Ok(Vec::new()),
        Step::Text(t) => match t.chars().find(|c| c.is_control()) {
            Some(c) => Err(format!("글에 제어 문자가 있다: U+{:04X}", c as u32)),
            None => Ok(t.as_bytes().to_vec()),
        },
    }
}

/// 동작별 입력 단계(mac `performAction` 의 목록). 키를 넣지 않는 동작은 비어 있다.
pub fn steps_for(action: SessionAction) -> Vec<Step> {
    match action {
        SessionAction::Pause => vec![Step::Escape],
        SessionAction::Resume => vec![Step::Text(CARRY_ON_PROMPT.to_string()), Step::Wait(200), Step::Enter],
        SessionAction::Stop => vec![Step::Escape, Step::Wait(400), Step::Text("/exit".to_string()), Step::Wait(200), Step::Enter],
        _ => Vec::new(),
    }
}

/// 바깥 세계 — 터미널 쓰기·콘솔 입력·프로세스 끝내기·기다리기. 시험은 가짜로 바꾼다.
pub trait Io {
    fn surface(&self, id: &str, bytes: &[u8]) -> bool;
    fn console(&self, pid: u32, inputs: &[Input]) -> bool;
    fn kill(&self, pid: u32) -> bool;
    fn sleep(&self, ms: u64);
}

/// `steps` 를 `target` 에 넣는다. 성공했으면 true.
/// 단계를 먼저 모두 검사한다(제어 문자가 있으면 아무것도 보내지 않고 false). 중단에서 콘솔 입력이 실패하면 프로세스를 끝낸다.
pub fn run(target: &Target, steps: &[Step], purpose: Purpose, io: &impl Io) -> bool {
    let encoded: Result<Vec<Vec<u8>>, String> = steps.iter().map(encode).collect();
    let encoded = match encoded {
        Ok(e) => e,
        Err(e) => {
            board_log(&format!("구동 단계를 거부했습니다: {e}"));
            return false;
        }
    };
    match target {
        Target::None => false,
        Target::Kill(pid) => io.kill(*pid),
        Target::Surface(id) => steps.iter().zip(&encoded).all(|(step, bytes)| match step {
            Step::Wait(ms) => {
                io.sleep(*ms);
                true
            }
            _ => io.surface(id, bytes),
        }),
        Target::Console(pid) => {
            let inputs: Vec<Input> = steps
                .iter()
                .map(|step| match step {
                    Step::Escape => Input::Escape,
                    Step::Enter => Input::Enter,
                    Step::Text(t) => Input::Text(t),
                    Step::Wait(ms) => Input::Pause(Duration::from_millis(*ms)),
                })
                .collect();
            io.console(*pid, &inputs) || (purpose == Purpose::Stop && io.kill(*pid))
        }
    }
}

/// 확인 창·대기 사이에 세션이 바뀌었는지 가른다 — 같은 세션(같은 pid·생성 시각)이 아직 살아 있을 때만 새 기록을 돌려준다.
/// 끝났거나 사라졌거나 pid 가 바뀌었으면 None: 옛 스냅샷으로 다른 프로세스를 끝내지 않는다.
pub fn still_same_session(old: &Session, fresh: Option<Session>) -> Option<Session> {
    fresh.filter(|f| f.state != "ended" && f.pid != 0 && f.pid == old.pid && f.pid_ctime == old.pid_ctime)
}

/// `run` 에 중단의 안전망을 더한다 — 탭(Surface)에 /exit 를 쓰지 못했으면 프로세스를 끝낸다(mac 과 같다).
pub fn run_with_fallback(target: &Target, pid: u32, steps: &[Step], purpose: Purpose, io: &impl Io) -> bool {
    run(target, steps, purpose, io) || (purpose == Purpose::Stop && matches!(target, Target::Surface(_)) && io.kill(pid))
}

/// 합계 기다림이 상한 안인가.
fn total_wait_ok(steps: &[Step]) -> bool {
    steps.iter().map(|s| if let Step::Wait(ms) = s { *ms } else { 0 }).sum::<u64>() <= MAX_TOTAL_WAIT_MS
}

/// `kill` 때 pid 가 그 세션의 것인지 다시 확인하는 어댑터 — 생성 시각이 다르면(pid 재사용) 끝내지 않는다.
struct RealIo<'a> {
    hub: &'a TermHub,
    pid_ctime: Option<f64>,
}

impl Io for RealIo<'_> {
    fn surface(&self, id: &str, bytes: &[u8]) -> bool {
        self.hub.write(id, bytes).is_ok()
    }
    fn console(&self, pid: u32, inputs: &[Input]) -> bool {
        console::send_input(pid, inputs)
    }
    fn kill(&self, pid: u32) -> bool {
        proc::alive(pid, self.pid_ctime) && proc::terminate(pid)
    }
    fn sleep(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// 세션을 구동한다 — 지금의 프로브(claude 가 도는 터미널·콘솔 접근)로 목적지를 다시 정한다. 성공했으면 true.
pub fn drive(board: &Board, s: &Session, steps: &[Step], purpose: Purpose) -> bool {
    let Some(s) = &still_same_session(s, board.session(&s.id)) else {
        board_log(&format!("세션 {} 이(가) 이미 끝났거나 바뀌어 구동하지 않습니다", s.id));
        return false;
    };
    let (runs, attachable) = {
        let probe = board.probe.lock().unwrap();
        (probe.runs_claude.clone(), probe.drivable.contains(&s.pid))
    };
    let target = drive_target(s, &runs, attachable, purpose);
    run_with_fallback(&target, s.pid, steps, purpose, &RealIo { hub: &board.app.state::<TermHub>(), pid_ctime: s.pid_ctime })
}

/// 프론트가 보내는 입력 단계 — `{key:"escape"|"enter"}` · `{text}` · `{wait:ms}`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StepSpec {
    Key { key: KeyName },
    Text { text: String },
    Wait { wait: u64 },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyName {
    Escape,
    Enter,
}

impl StepSpec {
    /// 기다림이 상한을 넘으면 거부한다.
    fn into_step(self) -> Result<Step, String> {
        match self {
            StepSpec::Key { key: KeyName::Escape } => Ok(Step::Escape),
            StepSpec::Key { key: KeyName::Enter } => Ok(Step::Enter),
            StepSpec::Text { text } => Ok(Step::Text(text)),
            StepSpec::Wait { wait } if wait <= MAX_WAIT_MS => Ok(Step::Wait(wait)),
            StepSpec::Wait { wait } => Err(format!("기다림이 너무 길다: {wait} ms")),
        }
    }
}

type B<'a> = State<'a, Arc<Board>>;

/// 이름 바꾸기 같은 프론트의 입력 단계를 세션에 넣는다. 세션이 없거나 단계가 거부되거나 닿을 곳이 없으면 false.
#[tauri::command(async)]
pub fn drive_session(board: B<'_>, id: String, steps: Vec<StepSpec>) -> bool {
    let steps: Result<Vec<Step>, String> = steps.into_iter().map(StepSpec::into_step).collect();
    match (board.session(&id), steps) {
        (_, Ok(steps)) if !total_wait_ok(&steps) => {
            board_log("drive_session: 기다림 합계가 너무 길다");
            false
        }
        (Some(s), Ok(steps)) => drive(&board, &s, &steps, Purpose::Keys),
        (_, Err(e)) => {
            board_log(&format!("drive_session: {e}"));
            false
        }
        (None, _) => false,
    }
}

/// `/rename` 을 쳐도 되는가 — 신선한 세션 상태와 대화 기록 유무.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RenameCheck {
    pub safe: bool,
    pub transcript: bool,
}

/// mac `renameSafe` — 보드 상태가 작업 중·대기이고 승인 창·질문 창이 없다. Claude Code 의 status 가 있으면 그것을,
/// 없으면 hook 이 쓴 세션 파일의 상태를 본다.
pub fn rename_safe(state: &str, claude_status: Option<&str>, record_state: Option<&str>) -> bool {
    matches!(state, "working" | "idle") && claude_status.or(record_state) != Some("waiting")
}

#[tauri::command(async)]
pub fn rename_check(board: B<'_>, id: String) -> RenameCheck {
    let Some(s) = board.session(&id) else { return RenameCheck { safe: false, transcript: false } };
    let record: Option<Value> = std::fs::read(&s.file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let field = |k: &str| record.as_ref().and_then(|r| r.get(k)).and_then(Value::as_str);
    let status = board.store.lock().unwrap().names.get(s.pid).and_then(|c| c.status);
    let transcript = crate::store::transcript_path(field("transcript_path"), &s.cwd, &s.id, &paths::claude_dir()).exists();
    RenameCheck { safe: rename_safe(&s.state, status.as_deref(), field("state")), transcript }
}

#[cfg(test)]
mod tests;
