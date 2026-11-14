//! 보드 밖에서 탭 열기·닫기 — `ClaudeBoard --open-tab`·`--close-tab` 규칙.
//! 원본(macOS): `Sources/BoardRules/OpenTab.swift`(열기 — `args.rs`)·`CloseTab.swift`(닫기 — `close.rs`).
//! 다른 프로그램(다른 Claude Code 세션의 스킬 등)이 보드 탭에 claude 를 띄우는 입구다. 명령줄 도구(같은 실행 파일)가
//! 요청을 사용자만 쓰는 폴더에 파일로 남기면, 보드가 그 파일을 읽어 보드가 직접 연 탭과 같은 길(`TerminalHub.run`)로 연다.
//! 분산 알림에는 내용을 싣지 않는다 — 알림은 같은 로그인의 어떤 프로세스든 보낼 수 있지만
//! 요청 파일은 그 사용자만 쓸 수 있는 폴더에 있어야 읽는다.

pub mod args;
pub mod cli;
pub mod close;
pub mod close_cli;
pub mod close_handle;
pub mod handle;
pub mod inbox;
pub mod plan;
