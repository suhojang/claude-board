//! 사람이 끼어드는 요청 control.json 의 판정 규칙. 원본: `Sources/AutopilotRules/Control.swift`.
//! 꼴은 `{"action":"pause|resume|stop|takeover|giveback|restart_from_intake","ts"}`. 파일 읽기·지우기·run.json 의 control_seen 쓰기·
//! 상태 바꾸기는 엔진에 남고, 여기는 순수 판정만 한다.

use serde_json::Value;

/// 끝난 실행의 상태 — 이 실행에는 control.json 을 쓰지 않는다. 원본: `apEndedStates`
pub const ENDED_STATES: [&str; 3] = ["done", "stopped", "error"];

/// 사람이 이어받은 실행의 상태. 원본: `apTakenOverState`(Takeover.swift)
pub const TAKEN_OVER_STATE: &str = "taken_over";

/// 엔진이 지금 어떤 꼴로 요청을 받는가 — 도는 중·일시정지 중·사람이 이어받은 중. 원본: `APControlMode`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMode {
    Running,
    Paused,
    TakenOver,
}

/// 원본: `apRunEnded`
pub fn run_ended(state: &str) -> bool {
    ENDED_STATES.contains(&state)
}

/// 보드·엔진 control 명령이 쓰는 요청. 원본: `apControlActions`
pub const CONTROL_ACTIONS: [&str; 6] = ["pause", "resume", "stop", "restart_from_intake", "takeover", "giveback"];

/// 요청의 사람이 읽는 이름. 모르는 요청이면 None. 원본: `apControlNames`
pub fn control_name(action: &str) -> Option<&'static str> {
    Some(match action {
        "pause" => "일시정지",
        "resume" => "재개",
        "stop" => "중단",
        "restart_from_intake" => "멈추고 다시 출발",
        "takeover" => "터미널로 이어받기",
        "giveback" => "자율주행에 돌려주기",
        _ => return None,
    })
}

/// 보드가 이 요청을 그 실행에 써도 되는가 — 아는 요청이고, 출발한(상태가 있는) 끝나지 않은 실행만.
/// 이어받은 실행에는 돌려주기·중단·멈추고 다시 출발만, 돌려주기는 이어받은 실행에만. 원본: `apBoardCanControl`
pub fn board_can_control(action: &str, state: &str) -> bool {
    if !CONTROL_ACTIONS.contains(&action) || state.is_empty() || run_ended(state) {
        return false;
    }
    if state == TAKEN_OVER_STATE {
        return matches!(action, "giveback" | "stop" | "restart_from_intake");
    }
    action != "giveback"
}

// NSNumber 처럼 불리언도 수로 읽는다(JSONSerialization 이 true 를 NSNumber 로 주는 동작)
fn json_number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

/// control.json 하나가 새 요청인가 — action 이 글자일 때만, ts 가 없으면 파일 수정 시각, 이미 받은 ts(control_seen) 보다 클 때만.
/// 새 요청이면 (action, ts), 아니면 None(엔진은 그래도 파일을 지운다 — 다시 뜬 엔진이 같은 요청을 또 하지 않게). 원본: `apControlNew`
pub fn control_new(json: Option<&Value>, mtime: f64, seen: Option<f64>) -> Option<(String, f64)> {
    let c = json?.as_object()?;
    let action = c.get("action")?.as_str()?;
    let ts_value = c.get("ts");
    let ts = json_number(ts_value)
        .or_else(|| ts_value?.as_str()?.parse::<f64>().ok())
        .unwrap_or(mtime);
    (ts > seen.unwrap_or(0.0)).then(|| (action.to_string(), ts))
}

/// 받은 요청으로 엔진이 할 일. 원본: `APControlStep`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStep {
    /// 지금 하던 일(claude·판정·대기)을 멈추고 일시정지로
    Pause,
    /// 실행을 마친다(사람이 중단)
    Stop,
    /// 일시정지를 풀고 이어 간다
    Resume,
    /// 멈추고 다시 출발 — 언제 멈출지는 `restart_plan`
    Restart,
    /// 일시정지처럼 멈추고 사람이 이어받은 상태로. 이어받을 세션이 있는지는 엔진이 먼저 본다
    Takeover,
    /// 돌려주기 확인 뒤 같은 세션으로 잇는다
    Giveback,
    /// 그대로 간다(진행 기록 한 줄만)
    None,
}

/// 원본: `APControlOutcome`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlOutcome {
    pub step: ControlStep,
    /// 진행 기록(events.jsonl, who board) 한 줄. None 이면 엔진이 그 일을 하며 따로 적는다
    pub text: Option<String>,
}

fn outcome(step: ControlStep, text: Option<&str>) -> ControlOutcome {
    ControlOutcome { step, text: text.map(str::to_string) }
}

fn unsupported(action: &str) -> ControlOutcome {
    outcome(ControlStep::None, Some(&format!("아직 엔진이 하지 못하는 요청({action})이라 넘깁니다.")))
}

/// 원본: `apControlOutcome(_:paused:)`
pub fn control_outcome(action: &str, paused: bool) -> ControlOutcome {
    control_outcome_mode(action, if paused { ControlMode::Paused } else { ControlMode::Running })
}

/// 원본: `apControlOutcome(_:mode:)`
pub fn control_outcome_mode(action: &str, mode: ControlMode) -> ControlOutcome {
    use ControlStep::{Giveback, None as Nothing, Pause, Restart, Resume, Stop, Takeover};
    if action == "restart_from_intake" {
        return outcome(Restart, None);
    }
    if mode == ControlMode::TakenOver {
        // 사람이 이어받은 중: 돌려주기·중단·멈추고 다시 출발만 한다
        return match action {
            "giveback" => outcome(Giveback, None),
            "stop" => outcome(Stop, None),
            "takeover" => outcome(Nothing, Some("이미 사람이 이어받았습니다.")),
            "pause" => outcome(Nothing, Some("사람이 이어받은 상태라 일시정지를 넘깁니다. 돌려주거나 중단합니다.")),
            "resume" => outcome(Nothing, Some("사람이 이어받은 상태라 재개 대신 돌려주기로 잇습니다.")),
            _ => unsupported(action),
        };
    }
    if action == "takeover" {
        return outcome(Takeover, Some("터미널로 이어받기 요청을 받았습니다."));
    }
    if action == "giveback" {
        return outcome(Nothing, Some("이어받은 상태가 아니라 돌려주기를 넘깁니다."));
    }
    if mode == ControlMode::Paused {
        return match action {
            "resume" => outcome(Resume, Some("재개합니다. 멈춘 자리에서 같은 세션으로 이어 갑니다.")),
            "stop" => outcome(Stop, None),
            "pause" => outcome(Nothing, Some("이미 일시정지 상태입니다.")),
            _ => unsupported(action),
        };
    }
    match action {
        "pause" => outcome(Pause, Some("일시정지 요청을 받았습니다.")),
        "stop" => outcome(Stop, Some("중단 요청을 받았습니다.")),
        "resume" => outcome(Nothing, Some("재개 요청을 받았지만 일시정지 상태가 아니라 그대로 갑니다.")),
        _ => unsupported(action),
    }
}

// ── 멈추고 다시 출발(설계 머리말 "목표를 바꾸려면 멈추고 다시 출발") ──

/// 받았을 때 언제 멈추는가 — 지금 단위(일시정지·한도 대기 중인 단위도)가 있으면 그 단위를 마친 뒤(새 단위는 시작하지 않음),
/// 없으면(단위 사이·멈출 선 대기·시험 환경 대기) 바로. 일시정지 중이면 기다린다(재개하면 그 단위를 마치고 멈춤,
/// 바로 멈추려면 중단 — 사람이 멈춘 일을 엔진이 스스로 다시 돌리지 않게). 이미 받은 요청이면 다시 하지 않는다. 원본: `APRestartPlan`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPlan {
    AfterUnit(usize),
    Now,
    Already,
}

/// 원본: `apRestartPlan`
pub fn restart_plan<S: AsRef<str>>(statuses: &[S], asked: bool) -> RestartPlan {
    if asked {
        return RestartPlan::Already;
    }
    match statuses.iter().position(|s| s.as_ref() == "running") {
        Some(i) => RestartPlan::AfterUnit(i + 1),
        None => RestartPlan::Now,
    }
}

/// 진행 기록 한 줄(who board). 원본: `apRestartText`
pub fn restart_text(p: RestartPlan, paused: bool, unit: Option<usize>) -> String {
    match p {
        RestartPlan::AfterUnit(n) if paused => format!(
            "멈추고 다시 출발 요청을 받았습니다. 일시정지 중이라 재개하면 그 단위(단위 {n})를 마친 뒤 멈춥니다(바로 멈추려면 중단합니다). 새 단위는 시작하지 않습니다."
        ),
        RestartPlan::AfterUnit(n) => {
            format!("멈추고 다시 출발 요청을 받았습니다. 지금 단위(단위 {n})를 마친 뒤 멈추고, 새 단위는 시작하지 않습니다.")
        }
        RestartPlan::Now => "멈추고 다시 출발 요청을 받았습니다. 도는 단위가 없어 바로 멈춥니다.".to_string(),
        RestartPlan::Already => {
            let tail = unit.map(|u| format!("(그 단위(단위 {u})를 마친 뒤 멈춤)")).unwrap_or_default();
            format!("멈추고 다시 출발 요청을 이미 받았습니다{tail}.")
        }
    }
}

/// 엔진이 새 단위를 고를 때: 요청을 받았고 남은(대기) 단위가 있으면 시작하지 않고 멈춘다.
/// 남은 단위가 없으면 보통처럼 끝낸다(끝남, 표지는 남음). 원본: `apRestartStopsBefore`
pub fn restart_stops_before(asked: bool, has_pending: bool) -> bool {
    asked && has_pending
}

/// 멈추고 다시 출발로 멈춘 실행의 끝 상태 이유 — 상태는 stopped(보드·finish·기록 저장소가 끝난 상태로 다룬다), 이유 머리로 가른다.
/// 원본: `apRestartReason`
pub const RESTART_REASON: &str = "멈추고 다시 출발: 사람이 요청해 멈췄습니다. 보드에서 이 실행을 이어 가는 출발 준비를 다시 엽니다.";

/// 원본: `apIsRestartStop`
pub fn is_restart_stop(state: &str, reason: &str) -> bool {
    state == "stopped" && reason.starts_with("멈추고 다시 출발")
}

/// 보드가 출발 준비를 저절로 다시 열 때 — 이 보드가 요청을 보냈고(asked), 실행이 끝나 엔진이 끝 정리(기록 저장소 쓰기)를 마쳤고(wrapped),
/// 엔진이 요청을 받았고(flag), 아직 열지 않았을 때(opened) 한 번. 이어 가기는 프로젝트가 있는 실행만. 원본: `apReopenIntake`
pub fn reopen_intake(
    state: &str,
    wrapped: bool,
    flag: bool,
    asked: bool,
    opened: bool,
    project: Option<&str>,
) -> bool {
    asked && !opened && flag && wrapped && run_ended(state) && !project.unwrap_or("").is_empty()
}
