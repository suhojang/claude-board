//! claude-board-hook — Claude Board 의 hook·설치·TUI 명령줄 실행 파일 (SPEC §1.1, §3).
//!
//! - `state <working|waiting|idle|ended|subagent>` / `notify <stop|notification>`: Claude Code hook.
//!   hook 입력 JSON 은 stdin. Claude Code 를 막거나 실패시키면 안 되므로 패닉까지 모두 잡고, 아무것도 출력하지 않고, 늘 0 으로 끝난다.
//! - `setup --app <ClaudeBoard.exe>` / `uninstall` / `install-hooks` / `uninstall-hooks`: 설치 도구. 짧은 진행 문구를 쓰고 실패하면 0 이 아닌 코드.
//! - `tui`: 터미널판 보드.

mod setup;
mod spawn;
mod tui;

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use claude_board_core::text::strip_utf8_bom;
use claude_board_core::{notify, paths, state};
use serde_json::{Map, Value};

/// windows/VERSION (build.rs 가 넣는다)
const VERSION: &str = env!("CLAUDE_BOARD_VERSION");

const USAGE: &str = "사용법: claude-board-hook <명령>
  state <working|waiting|idle|ended|subagent>   hook 입력(stdin)을 세션 기록에 남긴다
  notify <stop|notification>                    알림 패널을 띄운다
  setup --app <ClaudeBoard.exe 경로>            hook 사본·hook 등록·명령·자동 실행을 설정한다
  uninstall                                     setup 이 한 일을 되돌린다
  install-hooks | uninstall-hooks               ~/.claude/settings.json 의 hook 만 넣거나 뺀다
  tui                                           터미널판 보드
  --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    match arg(1).unwrap_or("") {
        "state" => {
            let s = arg(2).unwrap_or("idle").to_string();
            silent(move || run_state(&s))
        }
        "notify" => {
            let kind = arg(2).unwrap_or("stop").to_string();
            silent(move || run_notify(&kind))
        }
        "setup" => setup::setup(&args[2..]),
        "uninstall" => setup::uninstall(),
        "install-hooks" => setup::install_hooks(),
        "uninstall-hooks" => setup::uninstall_hooks(),
        "tui" => tui::run(),
        "--version" | "-V" | "version" => {
            println!("claude-board-hook {VERSION}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// hook 본체를 패닉까지 잡아 실행하고 늘 0 으로 끝낸다. 패닉 문구도 stderr 에 쓰지 않는다.
fn silent(f: impl FnOnce()) -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    ExitCode::SUCCESS
}

/// stdin 전부를 읽고 앞의 UTF-8 BOM 을 뗀다. PowerShell 5.1 의 `|` 같은 Windows 도구가 BOM 을 붙이는데,
/// 그 관용은 여기 한 곳에서만 처리해 state·notify 가 같은 입력을 본다(state.py·notify.py 는 BOM 을 받지 않는다).
fn read_input(mut r: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = r.read_to_end(&mut buf);
    strip_utf8_bom(&buf).to_vec()
}

fn read_stdin() -> Vec<u8> {
    read_input(std::io::stdin())
}

fn run_state(state_arg: &str) {
    let input = read_stdin();
    state::run(&paths::sessions_dir(), state_arg, &input, &state::SystemEnv);
}

/// `notify` 의 판단 — 깨진 입력은 빈 입력으로 본다(notify.py 와 같다). 객체가 아닌 JSON 은 띄우지 않는다.
fn notify_panel(kind: &str, input: &[u8], notifier: &Path, cwd: &str) -> Option<notify::Panel> {
    let data = match serde_json::from_slice::<Value>(input) {
        Ok(Value::Object(m)) => m,
        Ok(_) => return None,
        Err(_) => Map::new(),
    };
    notify::prepare(kind, &data, notifier, cwd)
}

fn run_notify(kind: &str) {
    let input = read_stdin();
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(panel) = notify_panel(kind, &input, &paths::notifier_dir(), &cwd) {
        spawn::detached(&panel.app, &panel.args);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

    fn with_bom(json: &str) -> Vec<u8> {
        [BOM.as_slice(), json.as_bytes()].concat()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-hook-main-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_input_drops_one_leading_bom() {
        assert_eq!(read_input(with_bom("{}").as_slice()), b"{}");
        assert_eq!(read_input(b"{}".as_slice()), b"{}");
        assert!(read_input(BOM.as_slice()).is_empty());
    }

    /// BOM 이 붙은 notify 입력도 BOM 없는 입력과 같은 패널 인자를 만든다.
    #[test]
    fn notify_with_bom_yields_the_same_panel_args() {
        let dir = temp("notify");
        let app = dir.join("ClaudeBoard.exe");
        fs::write(&app, b"").unwrap();
        fs::write(dir.join("app-path"), app.to_string_lossy().as_bytes()).unwrap();
        let json = r#"{"session_id":"bom-s","cwd":"/w/proj","message":"승인이 필요합니다"}"#;
        let plain = notify_panel("notification", &read_input(json.as_bytes()), &dir, "/fallback").unwrap();
        let bommed = notify_panel("notification", &read_input(with_bom(json).as_slice()), &dir, "/fallback").unwrap();
        assert_eq!(plain, bommed);
        let i = plain.args.iter().position(|a| a == "--subtitle").unwrap();
        assert_eq!(plain.args[i + 1], "proj");
        let i = plain.args.iter().position(|a| a == "--body").unwrap();
        assert_eq!(plain.args[i + 1], "승인이 필요합니다");
        // BOM 을 떼지 않으면 입력이 깨져 session·message 를 잃는다 — 이 시험이 지키는 차이
        let unstripped = notify_panel("notification", &with_bom(json), &dir, "/fallback").unwrap();
        assert_ne!(plain, unstripped);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// BOM 이 붙은 state 입력도 기록된다(hook 이 입력을 받는 길 `read_input` 을 지난다).
    #[test]
    fn state_with_bom_is_recorded() {
        let dir = temp("state");
        let input = read_input(with_bom(r#"{"session_id":"bom","cwd":"/a"}"#).as_slice());
        state::run(&dir, "working", &input, &state::SystemEnv);
        let rec = std::fs::read_to_string(dir.join("bom.json")).unwrap();
        assert!(rec.contains(r#""session_id": "bom""#) || rec.contains(r#""session_id":"bom""#), "{rec}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
