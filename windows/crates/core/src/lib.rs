//! Claude Board for Windows 공용 라이브러리.
//!
//! macOS 판의 `state.py`·`notify.py`·`hooks.py` 규칙과 `~/.claude/notifier/` 파일 약속,
//! Windows 프로세스·콘솔 도구를 담는다. hook 실행 파일(`claude-board-hook`)과 보드 백엔드(Tauri)가 함께 쓴다.
//! 공개 API 요약은 `API.md` 참고.

pub mod claude_live;
pub mod clock;
pub mod console;
pub mod fileio;
pub mod hooks;
pub mod notify;
pub mod paths;
pub mod place;
pub mod proc;
pub mod quote;
pub mod state;
pub mod text;

/// 세션 기록·hook 입력처럼 모양이 느슨한 JSON 객체. 키 순서를 지킨다(`preserve_order`).
pub type Record = serde_json::Map<String, serde_json::Value>;
