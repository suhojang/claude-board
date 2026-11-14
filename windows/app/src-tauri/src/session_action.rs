//! 세션 동작 이름 — mac `ClaudeBoard.swift performAction` 의 `pause`·`resume`·`stop`·`reopen`·`focus` 와
//! Windows 전용 `activate`. 프론트의 `session_action`(IPC)과 명령줄 `--action` 이 같은 이름을 쓴다.
//!
//! - `resume` = 일시중지한 세션 재개(mac `resumePaused`)
//! - `reopen` = 꺼진 세션을 보드 터미널 탭에서 이어 열기(mac `resumeSession(carryOn:)`) — 허브에 `board://open-session` 으로 요청한다. carryOn 을 안 주면 명령줄처럼 true

/// 이름이 가리키는 세션 동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    /// 한 번 클릭 — 그 세션의 터미널 창만 앞으로
    Activate,
    /// 두 번 클릭 — 그 세션의 창(탭)으로
    Focus,
    /// 이어 열기. `carry_on` 이면 "이어서 진행" 문구를 첫 요청으로 넘긴다
    Reopen { carry_on: bool },
    /// 일시중지
    Pause,
    /// 일시중지한 세션 재개
    Resume,
    /// 중단
    Stop,
}

impl SessionAction {
    /// 이름 → 동작. 모르는 이름(옛 `resume_paused` 포함)은 None. `carry_on` 은 `reopen` 에만 쓰인다.
    pub fn parse(name: &str, carry_on: Option<bool>) -> Option<SessionAction> {
        Some(match name {
            "activate" => SessionAction::Activate,
            "focus" => SessionAction::Focus,
            "reopen" => SessionAction::Reopen { carry_on: carry_on.unwrap_or(true) },
            "pause" => SessionAction::Pause,
            "resume" => SessionAction::Resume,
            "stop" => SessionAction::Stop,
            _ => return None,
        })
    }
}

/// 세션을 가리키지 않는 `--action` — 세션 id 자리는 `-` 같은 아무 글자이고 보지 않는다.
/// 세션 동작(`SessionAction`)과 섞지 않는다: 세션을 찾는 길(`queue_action`·`pending_action`)로 보내면 세션이 없다며 버려진다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardAction {
    /// 설치·제거·업데이트 스크립트가 청하는 정상 종료(mac `requestQuit`) — 끊길 세션 확인 창을 거친다
    Quit,
}

impl BoardAction {
    /// 이름 → 보드 동작. 세션 동작 이름과 모르는 이름은 None.
    pub fn parse(name: &str) -> Option<BoardAction> {
        match name {
            "quit" => Some(BoardAction::Quit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_is_a_board_action_and_not_a_session_action() {
        assert_eq!(BoardAction::parse("quit"), Some(BoardAction::Quit));
        assert_eq!(SessionAction::parse("quit", None), None, "세션을 찾는 길로 가면 버려진다");
        for name in ["Quit", "exit", "", "stop", "pause", "focus"] {
            assert_eq!(BoardAction::parse(name), None, "{name}");
        }
    }

    #[test]
    fn dispatch_table_uses_mac_names() {
        let table = [
            ("activate", SessionAction::Activate),
            ("focus", SessionAction::Focus),
            ("reopen", SessionAction::Reopen { carry_on: true }),
            ("pause", SessionAction::Pause),
            ("resume", SessionAction::Resume),
            ("stop", SessionAction::Stop),
        ];
        for (name, want) in table {
            assert_eq!(SessionAction::parse(name, None), Some(want), "{name}");
        }
    }

    #[test]
    fn carry_on_only_changes_reopen() {
        assert_eq!(SessionAction::parse("reopen", Some(false)), Some(SessionAction::Reopen { carry_on: false }));
        assert_eq!(SessionAction::parse("reopen", Some(true)), Some(SessionAction::Reopen { carry_on: true }));
        assert_eq!(SessionAction::parse("resume", Some(false)), Some(SessionAction::Resume));
        assert_eq!(SessionAction::parse("stop", Some(true)), Some(SessionAction::Stop));
    }

    #[test]
    fn old_and_unknown_names_are_rejected() {
        let old_resume = ["resume", "paused"].join("_");
        for name in [old_resume.as_str(), "Resume", "", "exit"] {
            assert_eq!(SessionAction::parse(name, None), None, "{name}");
        }
    }
}
