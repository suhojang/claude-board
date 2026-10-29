//! 알림 — macOS `notify.py` 이식 (SPEC §3.6).
//!
//! 본문 다듬기(`plain`·`first_sentence`), transcript 의 마지막 assistant 텍스트, 상태별 개수 요약,
//! 알림 패널(`ClaudeBoard.exe --notify …`) 인자 만들기. 패널을 실제로 띄우는 일은 hook 실행 파일이 한다.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::text::{is_py_space, non_empty_str, truthy};
use crate::{claude_live, fileio, paths, proc, state, Record};

/// 본문 최대 길이(코드포인트).
pub const MAX_BODY: usize = 140;
/// transcript 끝에서 읽는 바이트 수.
pub const TAIL_BYTES: u64 = 400_000;

// Python re 의 \s 는 str.isspace 와 같아 \x1c-\x1f 도 공백으로 본다
static CODE_BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```.*?```").expect("re"));
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]*)`").expect("re"));
static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]*)\*\*").expect("re"));
static LINE_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[\s\x1c-\x1f]*(#+|[-*]|\d+\.|\|)[\s\x1c-\x1f]*").expect("re"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s\x1c-\x1f]+").expect("re"));
static SENTENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(.+?[.!?。])(?:[\s\x1c-\x1f]|$)").expect("re"));

/// 마크다운 기호를 덜어내고 한 문단으로 줄인다. 140자를 넘으면 139자 + `…`.
pub fn plain(text: &str) -> String {
    let t = CODE_BLOCK.replace_all(text, " ");
    let t = INLINE_CODE.replace_all(&t, "$1");
    let t = BOLD.replace_all(&t, "$1");
    let t = LINE_HEAD.replace_all(&t, "");
    let t = SPACES.replace_all(&t, " ");
    let t = t.trim_matches(is_py_space);
    if t.chars().count() > MAX_BODY {
        let mut s: String = t.chars().take(MAX_BODY - 1).collect();
        s.push('…');
        s
    } else {
        t.to_string()
    }
}

/// 요약 배너용으로 첫 문장만 남긴다(`.!?。` 로 끝나는 가장 짧은 앞부분, 없으면 전체).
pub fn first_sentence(text: &str) -> String {
    let t = plain(text);
    match SENTENCE.captures(&t) {
        Some(c) => c[1].to_string(),
        None => t,
    }
}

/// transcript JSONL 끝 400KB 에서 마지막 assistant 텍스트를 찾는다. 없거나 읽지 못하면 빈 문자열.
///
/// JSONL 은 `\n` 으로만 줄을 나눈다 — Python `splitlines()` 는 문자열 안의 U+2028 등에서도 잘라 그 줄을 놓치지만 여기서는 놓치지 않는다.
pub fn last_assistant_text(path: &Path) -> String {
    let Some(bytes) = read_tail(path, TAIL_BYTES) else { return String::new() };
    // decode("utf-8", "ignore") — 잘린 첫 줄의 깨진 바이트는 버린다
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
    }
    for line in text.split('\n').rev() {
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line.trim_end_matches('\r')) else { continue };
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(content) = entry.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) else {
            continue;
        };
        let texts: Vec<&str> = content
            .iter()
            .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .filter(|t| !t.trim_matches(is_py_space).is_empty())
            .collect();
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    String::new()
}

pub(crate) fn read_tail(path: &Path, max: u64) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    let size = f.seek(SeekFrom::End(0)).ok()?;
    f.seek(SeekFrom::Start(size.saturating_sub(max))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// 전체 세션의 상태별 개수 `waiting=<n>,working=<n>,idle=<n>`.
/// 이 hook 이 막 바꾼 세션은 state hook 보다 먼저 실행될 수 있어 `current_state` 로 센다.
/// 다른 세션은 `alive` 가 거짓이면(프로세스가 죽었으면) 뺀다.
///
/// 다른 세션이 working·waiting 이면 `live` 로 Claude Code 의 신호를 겹쳐 본다(`notify.py:session_summary`).
/// Esc 로 끊긴 세션은 입력을 기다리는 중이라 대기이고, 승인한 도구가 도는 세션은 작업 중이다.
/// 이 hook 이 막 바꾼 현재 세션은 덮지 않는다. 겹치는 규칙은 [`claude_live::live_state`] 한 곳이다.
pub fn session_summary(
    sessions: &Path,
    current_id: Option<&Value>,
    current_state: &str,
    alive: impl Fn(&Record) -> bool,
    live: impl Fn(&Record) -> Option<(&'static str, f64)>,
) -> String {
    let keys = ["waiting", "working", "idle"];
    let mut counts = [0usize; 3];
    // JSON null 과 키 없음은 Python 에서 둘 다 None 이다
    let current_id = current_id.filter(|v| !v.is_null());
    let mut seen = false;
    for entry in fs::read_dir(sessions).into_iter().flatten().flatten() {
        if !entry.file_name().to_string_lossy().ends_with(".json") {
            continue;
        }
        let Some(rec) = fileio::read_record(&entry.path()) else { continue };
        let mut state = rec.get("state").and_then(Value::as_str);
        if rec.get("session_id").filter(|v| !v.is_null()) == current_id {
            state = Some(current_state);
            seen = true;
        } else if truthy(rec.get("pid")) && !alive(&rec) {
            continue;
        } else if let Some((overlaid, _)) = live(&rec) {
            state = Some(overlaid);
        }
        if let Some(i) = state.and_then(|s| keys.iter().position(|k| *k == s)) {
            counts[i] += 1;
        }
    }
    if !seen && truthy(current_id) {
        if let Some(i) = keys.iter().position(|k| *k == current_state) {
            counts[i] += 1;
        }
    }
    keys.iter().zip(counts).map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(",")
}

/// 띄울 알림 패널 — `app` 을 `args` 로 실행한다.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub app: PathBuf,
    pub args: Vec<String>,
}

/// `notify <stop|notification>` 의 판단 — 띄울 패널이 없으면 None.
///
/// 거르기: `mute-all` 이 있음 / `sessions/<sid>.mute` 가 있음 / `notification` 인데 종류가 승인 계열([`state::WAIT_NOTIFICATIONS`]) 밖 /
/// `app-path` 가 없거나 그 파일이 없음. `cwd_fallback` 은 입력에 cwd 가 없을 때의 현재 폴더.
pub fn prepare(kind: &str, data: &Record, notifier: &Path, cwd_fallback: &str) -> Option<Panel> {
    // 보드 윗줄 종 단추로 전체 알림을 끄면 mute-all 이 생긴다 — 세션별 설정보다 먼저 본다
    if notifier.join("mute-all").exists() {
        return None;
    }
    let sessions = notifier.join("sessions");
    // 보드에서 알림을 끈 세션은 작업 완료·승인 대기 모두 띄우지 않는다
    if let Some(sid) = non_empty_str(data.get("session_id")) {
        if sessions.join(format!("{sid}.mute")).exists() {
            return None;
        }
    }
    // 응답을 마친 뒤 입력이 없을 때 오는 알림(idle_prompt)은 이미 '작업 완료' 로 알렸고, 다른 백그라운드 작업이 끝남
    // 같은 승인과 상관없는 알림은 승인 대기가 아니므로 띄우지 않는다(종류가 없는 예전 Claude Code 의 알림은 띄운다)
    let wait = kind == "notification";
    if wait && non_empty_str(data.get("notification_type")).is_some_and(|k| !state::WAIT_NOTIFICATIONS.contains(&k)) {
        return None;
    }

    let app = fs::read_to_string(notifier.join("app-path")).ok()?;
    let app = PathBuf::from(app.trim());
    if app.as_os_str().is_empty() || !app.is_file() {
        return None;
    }

    let (title, accent) = if wait { ("승인 대기", "wait") } else { ("작업 완료", "done") };
    let body = if wait {
        non_empty_str(data.get("message")).unwrap_or("Claude 가 입력을 기다리고 있습니다").to_string()
    } else {
        match non_empty_str(data.get("last_assistant_message")) {
            Some(m) => m.to_string(),
            None => {
                let transcript = data.get("transcript_path").and_then(Value::as_str).unwrap_or("");
                if transcript.is_empty() { String::new() } else { last_assistant_text(Path::new(transcript)) }
            }
        }
    };
    let cwd = non_empty_str(data.get("cwd")).unwrap_or(cwd_fallback);
    let project = paths::basename(cwd);
    // 보드에서 모서리를 고르면 notify-position 이 생긴다 — 없으면 가운데
    let position = fs::read_to_string(notifier.join("notify-position"))
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "center".to_string());
    let claude_dir = paths::claude_dir();
    let summary = session_summary(
        &sessions,
        data.get("session_id"),
        if wait { "waiting" } else { "idle" },
        proc::record_alive,
        |rec| claude_live::live_override(rec, &claude_dir),
    );
    let session = non_empty_str(data.get("session_id")).unwrap_or("");
    Some(Panel { app, args: panel_args(title, project, &first_sentence(&body), &summary, accent, &position, session) })
}

/// 알림 패널 인자 — `--notify --title … --subtitle … --body … --summary … --sound … --kind … --duration 10 --position … [--session <sid>]`.
/// `session` 이 비어 있지 않을 때만 `--session` 을 붙인다 — 패널을 누르면 보드가 그 세션으로 이동한다(`--action focus <sid>`).
pub fn panel_args(title: &str, subtitle: &str, body: &str, summary: &str, kind: &str, position: &str, session: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "--notify", "--title", title, "--subtitle", subtitle, "--body", body, "--summary", summary, "--sound", kind,
        "--kind", kind, "--duration", "10", "--position", position,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if !session.is_empty() {
        args.extend(["--session".to_string(), session.to_string()]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-notify-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sessions")).unwrap();
        dir
    }

    fn no_live(_: &Record) -> Option<(&'static str, f64)> {
        None
    }

    #[test]
    fn summary_overlays_other_sessions_only() {
        let dir = temp("summary-live");
        let s = dir.join("sessions");
        fs::write(s.join("a.json"), r#"{"session_id":"a","state":"working"}"#).unwrap();
        fs::write(s.join("b.json"), r#"{"session_id":"b","state":"working","pid":5}"#).unwrap();
        fs::write(s.join("c.json"), r#"{"session_id":"c","state":"waiting"}"#).unwrap();
        fs::write(s.join("d.json"), r#"{"session_id":"d","state":"idle"}"#).unwrap();
        // b 는 Esc 로 끊겼고 c 는 승인한 도구가 도는 중이다. a 는 이 hook 이 바꾸는 현재 세션이라 덮지 않는다
        let live = |r: &Record| match r["session_id"].as_str() {
            Some("a" | "b") => Some(("idle", 1.0)),
            Some("c") => Some(("working", 1.0)),
            _ => None,
        };
        let alive = |_: &Record| true;
        assert_eq!(session_summary(&s, Some(&json!("a")), "waiting", alive, live), "waiting=1,working=1,idle=2");
        assert_eq!(session_summary(&s, Some(&json!("a")), "waiting", alive, no_live), "waiting=2,working=1,idle=1");
        // 죽은 세션은 덮기 전에 뺀다 — live 를 부르지 않는다
        let dead = |r: &Record| r.get("pid") != Some(&json!(5));
        let counted = |r: &Record| {
            assert_ne!(r.get("pid"), Some(&json!(5)));
            live(r)
        };
        assert_eq!(session_summary(&s, Some(&json!("x")), "idle", dead, counted), "waiting=0,working=1,idle=3");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn summary_counts() {
        let dir = temp("summary");
        let s = dir.join("sessions");
        fs::write(s.join("a.json"), r#"{"session_id":"a","state":"working"}"#).unwrap();
        fs::write(s.join("b.json"), r#"{"session_id":"b","state":"idle","pid":5}"#).unwrap();
        fs::write(s.join("c.json"), r#"{"session_id":"c","state":"waiting","pid":6}"#).unwrap();
        fs::write(s.join("d.json"), r#"{"session_id":"d","state":"ended"}"#).unwrap();
        fs::write(s.join("e.json"), "{broken").unwrap();
        fs::write(s.join("a.mute"), "").unwrap();
        let alive = |r: &Record| r["pid"] != 6;
        // 이 세션(a)은 막 바뀐 상태로 센다, 죽은 c 는 뺀다
        assert_eq!(session_summary(&s, Some(&json!("a")), "waiting", alive, no_live), "waiting=1,working=0,idle=1");
        // 이 세션 파일이 아직 없으면 그 상태에 1 을 더한다
        assert_eq!(session_summary(&s, Some(&json!("new")), "idle", alive, no_live), "waiting=0,working=1,idle=2");
        // session_id 가 없으면 더하지 않는다
        assert_eq!(session_summary(&s, None, "idle", alive, no_live), "waiting=0,working=1,idle=1");
        assert_eq!(session_summary(&dir.join("none"), Some(&json!("x")), "waiting", alive, no_live), "waiting=1,working=0,idle=0");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn panel_args_appends_session_last_only_when_given() {
        let with = panel_args("t", "s", "b", "waiting=1", "wait", "center", "sid-1");
        assert_eq!(&with[with.len() - 2..], ["--session", "sid-1"]);
        let without = panel_args("t", "s", "b", "waiting=1", "wait", "center", "");
        assert_eq!(without.len(), with.len() - 2);
        assert_eq!(without[..], with[..without.len()]);
        assert!(!without.iter().any(|a| a == "--session"));
    }

    #[test]
    fn prepare_filters_and_builds_args() {
        let dir = temp("prepare");
        let data = json!({"session_id": "s1", "cwd": "/w/proj", "message": "Claude needs your permission to use Bash"});
        let data = data.as_object().unwrap();
        // app-path 가 없으면 띄우지 않는다
        assert_eq!(prepare("notification", data, &dir, "/x"), None);
        let app = dir.join("ClaudeBoard.exe");
        fs::write(&app, "").unwrap();
        fs::write(dir.join("app-path"), format!("{}\r\n", app.display())).unwrap();
        let p = prepare("notification", data, &dir, "/x").unwrap();
        assert_eq!(p.app, app);
        assert_eq!(
            p.args,
            panel_args("승인 대기", "proj", "Claude needs your permission to use Bash", "waiting=1,working=0,idle=0", "wait", "center", "s1")
        );
        fs::write(dir.join("notify-position"), "top-right\n").unwrap();
        let stop = json!({"session_id": "s1", "last_assistant_message": "끝났습니다. 다음은 시험입니다."});
        let p = prepare("stop", stop.as_object().unwrap(), &dir, "/fallback/here").unwrap();
        assert_eq!(p.args, panel_args("작업 완료", "here", "끝났습니다.", "waiting=0,working=0,idle=1", "done", "top-right", "s1"));
        // 세션 id 가 없는 알림은 --session 을 붙이지 않는다
        let anonymous = json!({"cwd": "/w/proj", "message": "m"});
        let p = prepare("notification", anonymous.as_object().unwrap(), &dir, "/x").unwrap();
        assert!(!p.args.iter().any(|a| a == "--session"), "{:?}", p.args);
        let empty_id = json!({"session_id": "", "cwd": "/w/proj", "message": "m"});
        let p = prepare("notification", empty_id.as_object().unwrap(), &dir, "/x").unwrap();
        assert!(!p.args.iter().any(|a| a == "--session"), "{:?}", p.args);
        // idle_prompt·세션 음소거·전체 음소거
        let idle = json!({"session_id": "s1", "notification_type": "idle_prompt"});
        assert_eq!(prepare("notification", idle.as_object().unwrap(), &dir, "/x"), None);
        // 다른 백그라운드 작업이 끝났다는 알림은 승인 대기가 아니다
        let done = json!({"session_id": "s1", "message": "작업 failed", "notification_type": "agent_completed"});
        assert_eq!(prepare("notification", done.as_object().unwrap(), &dir, "/x"), None);
        fs::write(dir.join("sessions/s1.mute"), "").unwrap();
        assert_eq!(prepare("stop", stop.as_object().unwrap(), &dir, "/x"), None);
        fs::remove_file(dir.join("sessions/s1.mute")).unwrap();
        fs::write(dir.join("mute-all"), "").unwrap();
        assert_eq!(prepare("stop", stop.as_object().unwrap(), &dir, "/x"), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
