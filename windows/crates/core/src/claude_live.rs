//! Claude Code 가 hook 없이 남기는 신호로 세션 상태를 겹쳐 보기 — Esc 중단과 `~/.claude/sessions/<pid>.json`.
//!
//! Claude Code 는 사용자가 응답을 Esc 로 끊으면 Stop hook 을 부르지 않아 세션 파일이 작업 중·승인 대기로 남는다.
//! 첫 응답 전에 끊으면 대화 기록에 중단 표시도 남지 않고, 승인한 도구가 도는 동안은 PostToolUse 까지 hook 이 없다.
//! 이 모듈이 그 빈틈을 메운다. 규칙은 macOS 의 세 곳과 같다.
//! - `notify.py:interrupted_at`·`claude_status`·`session_summary`, `board-tui.py:interrupted_at`·`claude_status`·`load`
//! - `ClaudeBoard.swift` 의 `InterruptReader`(대화 기록 끝의 중단 표시), `ClaudeNames`(`statusChange`·`name`)
//! - `ClaudeBoard.swift` 의 `Store.reload`(약 1215-1232행) — 중단·status 로 idle/working 을 덮는 순서
//!
//! 판정 규칙은 [`live_state`] 한 곳에 있다. 파일을 읽는 일은 [`interrupted_at`]·[`read_claude_session`] 이 맡고
//! 시험은 순수 함수([`interrupted_at_in`]·[`parse_claude_session`]·[`status_change`])로 한다.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::text::non_empty_str;
use crate::{clock, notify, Record};

/// 대화 기록 끝에서 읽는 바이트 수.
pub const INTERRUPT_TAIL: u64 = 256_000;
/// Esc 로 끊은 응답이 대화 기록에 남기는 문구의 앞부분(`… by user]`·`… by user for tool use]` 가 모두 이것으로 시작한다).
pub const INTERRUPT_MARK: &[u8] = b"[Request interrupted by user";

/// 대화 기록 끝(`tail`, 256_000 바이트 이하)의 마지막 대화 줄이 중단 표시면 끊은 시각(유닉스 초)을 돌려준다.
/// `mtime` 은 `timestamp` 가 없거나 못 풀 때 쓸 파일 수정 시각(유닉스 초).
///
/// 순수 함수 — `notify.py:interrupted_at` 의 파싱 부분. 중단 뒤에는 cost-state·last-prompt 같은 대화 아닌 줄이 붙고
/// `!` 명령·슬래시 명령 출력은 `<` 로 시작하는 사용자 줄로 남으므로 그런 줄은 건너뛴다.
/// 읽는 창이 256_000 바이트에서 잘려 JSON 이 아니거나 UTF-8 이 깨진 첫 줄도 건너뛴다.
pub fn interrupted_at_in(tail: &[u8], mtime: f64) -> Option<f64> {
    // 중단 표시가 없으면 JSON 을 풀지 않는다 — 작업 중 세션은 대부분 여기서 끝난다
    if !contains(tail, INTERRUPT_MARK) {
        return None;
    }
    for line in tail.rsplit(|&b| b == b'\n') {
        let Some(entry) = parse_line(line) else { continue };
        let kind = entry.get("type").and_then(Value::as_str);
        if !matches!(kind, Some("user" | "assistant"))
            || entry.get("isSidechain") == Some(&Value::Bool(true))
            || entry.get("isMeta") == Some(&Value::Bool(true))
        {
            continue;
        }
        let user = kind == Some("user");
        let text = entry_text(&entry);
        if user && text.starts_with('<') {
            continue;
        }
        if user && text.as_bytes().starts_with(INTERRUPT_MARK) {
            let stamp = entry.get("timestamp").and_then(Value::as_str).and_then(clock::iso_date);
            return Some(stamp.unwrap_or(mtime));
        }
        return None;
    }
    None
}

/// 대화 기록 파일 끝 256_000 바이트로 [`interrupted_at_in`] 을 돌린다. 파일이 없거나 못 읽으면 None.
pub fn interrupted_at(transcript: &Path) -> Option<f64> {
    let tail = notify::read_tail(transcript, INTERRUPT_TAIL)?;
    let mtime = fs::metadata(transcript)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(clock::now, |d| d.as_secs_f64());
    interrupted_at_in(&tail, mtime)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// JSONL 한 줄 — 줄 끝 `\r` 과 줄 앞 BOM 을 떼고 객체로 푼다. 못 풀면(UTF-8 이 깨진 줄 포함) None.
fn parse_line(line: &[u8]) -> Option<Record> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = line.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(line);
    match serde_json::from_slice::<Value>(line) {
        Ok(Value::Object(entry)) => Some(entry),
        _ => None,
    }
}

/// `message.content` 의 글 — 문자열이면 그대로, 목록이면 첫 원소의 `text`. 그 밖(tool_result 등)은 빈 문자열.
fn entry_text(entry: &Record) -> &str {
    let Some(message) = entry.get("message").and_then(Value::as_object) else { return "" };
    match message.get("content") {
        Some(Value::String(s)) => s,
        Some(Value::Array(a)) => a.first().and_then(|c| c.get("text")).and_then(Value::as_str).unwrap_or(""),
        _ => "",
    }
}

/// Claude Code 가 `~/.claude/sessions/<pid>.json` 에 쓰는 값 가운데 보드가 읽는 것.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClaudeSession {
    /// `sessionId` — `/clear` 하면 같은 pid 가 새 id 를 쓴다.
    pub session_id: Option<String>,
    /// `name` — 자동 이름 또는 `--name`·`/rename` 으로 바꾼 이름.
    pub name: Option<String>,
    /// `status` — `idle`·`busy`, 승인 창·질문 창이 떠 있으면 `waiting`.
    pub status: Option<String>,
    /// `statusUpdatedAt`(밀리초)을 1000 으로 나눈 유닉스 초. 숫자가 아니면 None.
    pub status_at: Option<f64>,
}

/// 파일 내용을 푼다. JSON 객체가 아니면 None — `ClaudeNames.record`.
pub fn parse_claude_session(bytes: &[u8]) -> Option<ClaudeSession> {
    let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(bytes) else { return None };
    let string = |key: &str| j.get(key).and_then(Value::as_str).map(str::to_string);
    Some(ClaudeSession {
        session_id: string("sessionId"),
        name: string("name"),
        status: string("status"),
        status_at: j.get("statusUpdatedAt").and_then(Value::as_f64).map(|ms| ms / 1000.0),
    })
}

/// `<claude_dir>/sessions/<pid>.json` — 같은 폴더의 `<pid>.<해시>.key` 같은 파일은 읽지 않는다.
pub fn claude_session_path(claude_dir: &Path, pid: u32) -> PathBuf {
    claude_dir.join("sessions").join(format!("{pid}.json"))
}

/// pid 의 세션 파일을 읽는다. pid 0 이거나 파일이 없거나 객체가 아니면 None.
pub fn read_claude_session(claude_dir: &Path, pid: u32) -> Option<ClaudeSession> {
    if pid == 0 {
        return None;
    }
    parse_claude_session(&fs::read(claude_session_path(claude_dir, pid)).ok()?)
}

/// status 가 `since` 뒤에 바뀌었으면 `(status, 바뀐 시각)` — `notify.py:claude_status`·`ClaudeNames.statusChange`.
/// `sid` 가 비어 있지 않고 파일의 `sessionId` 와 같을 때만 본다(같은 pid 를 다른 세션이 쓸 수 있다).
pub fn status_change(cs: &ClaudeSession, sid: &str, since: f64) -> Option<(String, f64)> {
    if sid.is_empty() || cs.session_id.as_deref() != Some(sid) {
        return None;
    }
    let status = cs.status.as_ref()?;
    let at = cs.status_at.filter(|&at| at > since)?;
    Some((status.clone(), at))
}

/// 보여 줄 세션 이름 — 비어 있지 않은 `name` 만.
pub fn session_name(cs: &ClaudeSession) -> Option<&str> {
    cs.name.as_deref().filter(|n| !n.is_empty())
}

/// 세션 기록의 state 위에 Claude Code 의 신호를 겹친 상태 `(상태, 시작 시각)`. 겹칠 것이 없으면 None.
///
/// 규칙은 여기 한 곳이다(`Store.reload` 와 같은 순서). 클로저는 `state` 가 `working`·`waiting` 일 때만 부른다.
/// - `at = interrupted()`, 없으면 `status` 가 `idle` 일 때의 그 시각 → `("idle", at)`. 중단이 busy 보다 먼저다
/// - 아니면 `state == "waiting"` 이고 `status` 가 `busy` → `("working", 바뀐 시각)` (승인한 도구가 도는 중)
pub fn live_state(
    state: &str,
    interrupted: impl FnOnce() -> Option<f64>,
    status: impl FnOnce() -> Option<(String, f64)>,
) -> Option<(&'static str, f64)> {
    if !matches!(state, "working" | "waiting") {
        return None;
    }
    if let Some(at) = interrupted() {
        return Some(("idle", at));
    }
    let (status, at) = status()?;
    match status.as_str() {
        "idle" => Some(("idle", at)),
        "busy" if state == "waiting" => Some(("working", at)),
        _ => None,
    }
}

/// 세션 기록 하나에 [`live_state`] 를 적용한다 — hook 쪽(알림 요약)과 터미널 보드가 쓴다.
/// 기록의 `state`·`pid`·`session_id`·`since`(없으면 0)·`transcript_path`(비면 중단 없음)를 읽는다.
pub fn live_override(rec: &Record, claude_dir: &Path) -> Option<(&'static str, f64)> {
    let state = rec.get("state").and_then(Value::as_str).unwrap_or("");
    let since = rec.get("since").and_then(Value::as_f64).unwrap_or(0.0);
    live_state(
        state,
        || non_empty_str(rec.get("transcript_path")).and_then(|t| interrupted_at(Path::new(t))),
        || {
            let pid = crate::state::record_pid(rec)?;
            let sid = rec.get("session_id").and_then(Value::as_str)?;
            status_change(&read_claude_session(claude_dir, pid)?, sid, since)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(v: Value) -> String {
        v.to_string()
    }

    fn user(text: &str, ts: Option<&str>) -> String {
        let mut v = json!({"type": "user", "message": {"role": "user", "content": text}});
        if let Some(ts) = ts {
            v["timestamp"] = json!(ts);
        }
        line(v)
    }

    fn joined(lines: &[String]) -> Vec<u8> {
        lines.join("\n").into_bytes()
    }

    const STAMP: &str = "2026-10-16T19:24:08.5Z";
    const STAMP_SEC: f64 = 1_792_178_648.5;

    #[test]
    fn no_mark_is_none_without_parsing() {
        let tail = joined(&[user("안녕", Some(STAMP)), line(json!({"type": "assistant", "message": {"content": "응"}}))]);
        assert_eq!(interrupted_at_in(&tail, 7.0), None);
        assert_eq!(interrupted_at_in(b"", 7.0), None);
    }

    #[test]
    fn interrupt_survives_trailing_non_conversation_lines() {
        let tail = joined(&[
            user("[Request interrupted by user]", Some(STAMP)),
            line(json!({"type": "cost-state", "cost": 1})),
            line(json!({"type": "last-prompt", "lastPrompt": "x"})),
            "not json".to_string(),
            String::new(),
        ]);
        assert_eq!(interrupted_at_in(&tail, 7.0), Some(STAMP_SEC));
    }

    #[test]
    fn angle_bracket_user_lines_are_skipped_but_assistant_ends_it() {
        let cmd = user("<local-command-stdout>ok</local-command-stdout>", Some("2026-10-16T20:00:00Z"));
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), cmd.clone()]);
        assert_eq!(interrupted_at_in(&tail, 7.0), Some(STAMP_SEC));
        // 중단 뒤에 assistant 줄이 오면 이미 다음 응답이 시작된 것이다
        let reply = line(json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "계속"}]}}));
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), reply]);
        assert_eq!(interrupted_at_in(&tail, 7.0), None);
        // 중단 뒤에 일반 사용자 줄이 와도 마찬가지
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), user("다시", None)]);
        assert_eq!(interrupted_at_in(&tail, 7.0), None);
    }

    #[test]
    fn sidechain_and_meta_lines_are_skipped() {
        let side = line(json!({"type": "assistant", "isSidechain": true, "message": {"content": "서브"}}));
        let meta = line(json!({"type": "user", "isMeta": true, "message": {"content": "메타"}}));
        let not_true = line(json!({"type": "assistant", "isSidechain": "true", "message": {"content": "문자열"}}));
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), side, meta]);
        assert_eq!(interrupted_at_in(&tail, 7.0), Some(STAMP_SEC));
        // == true 일 때만 건너뛴다 — 문자열 "true" 는 건너뛰지 않으므로 그 assistant 줄에서 끝난다
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), not_true]);
        assert_eq!(interrupted_at_in(&tail, 7.0), None);
    }

    #[test]
    fn tool_result_user_line_is_not_an_interrupt() {
        let result = line(json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t", "content": "x"}]}}));
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), result]);
        assert_eq!(interrupted_at_in(&tail, 7.0), None);
        // message 가 객체가 아니면 글은 빈 문자열
        let odd = line(json!({"type": "user", "message": "그냥 문자열"}));
        let tail = joined(&[user("[Request interrupted by user]", Some(STAMP)), odd]);
        assert_eq!(interrupted_at_in(&tail, 7.0), None);
    }

    #[test]
    fn both_content_shapes_and_tool_use_variant() {
        let list = line(json!({
            "type": "user", "timestamp": STAMP,
            "message": {"content": [{"type": "text", "text": "[Request interrupted by user for tool use]"}]}
        }));
        assert_eq!(interrupted_at_in(list.as_bytes(), 7.0), Some(STAMP_SEC));
        let text = user("[Request interrupted by user for tool use]", Some(STAMP));
        assert_eq!(interrupted_at_in(text.as_bytes(), 7.0), Some(STAMP_SEC));
    }

    #[test]
    fn missing_or_bad_timestamp_uses_mtime() {
        let none = user("[Request interrupted by user]", None);
        assert_eq!(interrupted_at_in(none.as_bytes(), 7.0), Some(7.0));
        let bad = user("[Request interrupted by user]", Some("어제 오후"));
        assert_eq!(interrupted_at_in(bad.as_bytes(), 7.0), Some(7.0));
        let number = line(json!({"type": "user", "timestamp": 12, "message": {"content": "[Request interrupted by user]"}}));
        assert_eq!(interrupted_at_in(number.as_bytes(), 7.0), Some(7.0));
    }

    #[test]
    fn crlf_and_bom_lines_parse() {
        let mut tail = Vec::new();
        tail.extend_from_slice(b"\xEF\xBB\xBF");
        tail.extend_from_slice(user("[Request interrupted by user]", Some(STAMP)).as_bytes());
        tail.extend_from_slice(b"\r\n");
        tail.extend_from_slice(line(json!({"type": "last-prompt"})).as_bytes());
        tail.extend_from_slice(b"\r\n");
        assert_eq!(interrupted_at_in(&tail, 7.0), Some(STAMP_SEC));
    }

    #[test]
    fn cut_first_line_with_broken_utf8_is_skipped() {
        // 256_000 바이트 경계에서 한글(3바이트)의 가운데가 잘린 첫 줄 — 마크가 들어 있어도 못 풀면 건너뛴다
        let mut cut = b"\xB0\x80 \"[Request interrupted by user".to_vec();
        assert_eq!(interrupted_at_in(&cut, 7.0), None);
        cut.push(b'\n');
        cut.extend_from_slice(user("[Request interrupted by user]", Some(STAMP)).as_bytes());
        assert_eq!(interrupted_at_in(&cut, 7.0), Some(STAMP_SEC));
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-live-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn interrupted_at_reads_only_the_tail_window() {
        let dir = temp("tail");
        let filler = user(&"가".repeat(2000), Some("2026-10-16T19:00:00Z")) + "\n";
        let pad = |n: usize| filler.repeat(n);
        // 256_000 바이트를 넘는 파일의 끝에 있는 중단 표시는 찾는다
        let big = dir.join("big.jsonl");
        fs::write(&big, pad(100) + &user("[Request interrupted by user]", Some(STAMP)) + "\n").unwrap();
        assert!(fs::metadata(&big).unwrap().len() > INTERRUPT_TAIL);
        assert_eq!(interrupted_at(&big), Some(STAMP_SEC));
        // 중단 표시가 창 밖(앞쪽)에만 있으면 None
        let early = dir.join("early.jsonl");
        fs::write(&early, user("[Request interrupted by user]", Some(STAMP)) + "\n" + &pad(100)).unwrap();
        assert!(fs::metadata(&early).unwrap().len() > INTERRUPT_TAIL + 1000);
        assert_eq!(interrupted_at(&early), None);
        // timestamp 가 없으면 파일 수정 시각
        let plain = dir.join("plain.jsonl");
        fs::write(&plain, user("[Request interrupted by user]", None)).unwrap();
        let mtime = fs::metadata(&plain).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert_eq!(interrupted_at(&plain), Some(mtime));
        // 없는 파일·폴더
        assert_eq!(interrupted_at(&dir.join("none.jsonl")), None);
        assert_eq!(interrupted_at(&dir), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn session(sid: &str, status: Option<&str>, at: Value) -> ClaudeSession {
        let mut v = json!({"sessionId": sid, "name": "내-세션", "statusUpdatedAt": at});
        if let Some(s) = status {
            v["status"] = json!(s);
        }
        parse_claude_session(v.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn parse_and_name() {
        let cs = parse_claude_session(br#"{"sessionId":"s","name":"work-1","status":"busy","statusUpdatedAt":2500,"pid":9}"#).unwrap();
        assert_eq!(
            cs,
            ClaudeSession {
                session_id: Some("s".into()),
                name: Some("work-1".into()),
                status: Some("busy".into()),
                status_at: Some(2.5)
            }
        );
        assert_eq!(session_name(&cs), Some("work-1"));
        let empty = parse_claude_session(br#"{"name":""}"#).unwrap();
        assert_eq!(session_name(&empty), None);
        assert_eq!(empty, ClaudeSession { name: Some(String::new()), ..Default::default() });
        // 객체가 아니면 None
        for bad in [&b"[]"[..], b"null", b"\"x\"", b"{", b""] {
            assert_eq!(parse_claude_session(bad), None);
        }
    }

    #[test]
    fn status_change_rules() {
        let cs = session("s", Some("idle"), json!(5000));
        assert_eq!(status_change(&cs, "s", 4.0), Some(("idle".to_string(), 5.0)));
        // 같거나 더 오래된 시각은 이미 반영된 것
        assert_eq!(status_change(&cs, "s", 5.0), None);
        assert_eq!(status_change(&cs, "s", 6.0), None);
        // sessionId 가 다르거나 sid 가 비면 다른 세션의 파일이다
        assert_eq!(status_change(&cs, "other", 0.0), None);
        assert_eq!(status_change(&cs, "", 0.0), None);
        assert_eq!(status_change(&ClaudeSession::default(), "", 0.0), None);
        // 시각이 숫자가 아니거나 status 가 없으면 못 쓴다
        assert_eq!(status_change(&session("s", Some("idle"), json!("5000")), "s", 0.0), None);
        assert_eq!(status_change(&session("s", Some("idle"), Value::Null), "s", 0.0), None);
        assert_eq!(status_change(&session("s", None, json!(5000)), "s", 0.0), None);
    }

    #[test]
    fn read_session_from_claude_dir_with_spaces_and_hangul() {
        let dir = temp("read").join("홍 길동").join(".claude");
        let sessions = dir.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(sessions.join("77.json"), r#"{"sessionId":"s","status":"busy","statusUpdatedAt":9000}"#).unwrap();
        // 같은 폴더의 .key 파일·깨진 파일은 읽지 않는다
        fs::write(sessions.join("77.abc123.key"), "비밀").unwrap();
        fs::write(sessions.join("78.json"), "[1]").unwrap();
        assert_eq!(claude_session_path(&dir, 77), sessions.join("77.json"));
        let cs = read_claude_session(&dir, 77).unwrap();
        assert_eq!(status_change(&cs, "s", 0.0), Some(("busy".to_string(), 9.0)));
        assert_eq!(read_claude_session(&dir, 78), None);
        assert_eq!(read_claude_session(&dir, 79), None);
        assert_eq!(read_claude_session(&dir, 0), None);
        assert_eq!(read_claude_session(&dir.join("없음"), 77), None);
        fs::remove_dir_all(dir.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn live_state_truth_table() {
        let st = |s: &'static str, at: f64| move || Some((s.to_string(), at));
        // 중단이 있으면 idle — status 가 busy 여도 중단이 먼저다
        assert_eq!(live_state("working", || Some(3.0), st("busy", 9.0)), Some(("idle", 3.0)));
        assert_eq!(live_state("waiting", || Some(3.0), st("busy", 9.0)), Some(("idle", 3.0)));
        // 중단이 없고 status 가 idle 이면 그 시각
        assert_eq!(live_state("working", || None, st("idle", 4.0)), Some(("idle", 4.0)));
        assert_eq!(live_state("waiting", || None, st("idle", 4.0)), Some(("idle", 4.0)));
        // 승인한 도구가 도는 중 — waiting 만 working 으로
        assert_eq!(live_state("waiting", || None, st("busy", 5.0)), Some(("working", 5.0)));
        assert_eq!(live_state("working", || None, st("busy", 5.0)), None);
        // status 가 waiting 이거나 모르는 값·없음
        assert_eq!(live_state("waiting", || None, st("waiting", 5.0)), None);
        assert_eq!(live_state("working", || None, st("other", 5.0)), None);
        assert_eq!(live_state("working", || None, || None), None);
        // idle·ended·paused·모르는 상태는 클로저를 부르지 않는다
        for state in ["idle", "ended", "paused", ""] {
            let got = live_state(state, || panic!("interrupted 를 부르면 안 된다"), || panic!("status 를 부르면 안 된다"));
            assert_eq!(got, None, "{state}");
        }
        // 중단이 있으면 status 를 읽지 않는다
        assert_eq!(live_state("working", || Some(1.0), || panic!("status 를 부르면 안 된다")), Some(("idle", 1.0)));
    }

    #[test]
    fn live_override_reads_record_fields() {
        let root = temp("override");
        let claude = root.join("내 폴더").join(".claude");
        fs::create_dir_all(claude.join("sessions")).unwrap();
        let transcript = root.join("t.jsonl");
        let rec = |v: Value| v.as_object().unwrap().clone();
        let t = transcript.to_string_lossy().into_owned();
        // 중단된 대화 기록 → idle, since 는 중단 시각
        fs::write(&transcript, user("[Request interrupted by user]", Some(STAMP))).unwrap();
        let r = rec(json!({"state": "working", "session_id": "s", "since": 1.0, "transcript_path": t}));
        assert_eq!(live_override(&r, &claude), Some(("idle", STAMP_SEC)));
        // transcript_path 가 비면 중단은 없다. pid 도 없으니 status 도 없다
        let r = rec(json!({"state": "working", "session_id": "s", "since": 1.0, "transcript_path": ""}));
        assert_eq!(live_override(&r, &claude), None);
        // status: waiting + busy → working. since 와 같거나 이전이면 무시
        fs::write(claude.join("sessions").join("42.json"), r#"{"sessionId":"s","status":"busy","statusUpdatedAt":7000}"#).unwrap();
        let r = rec(json!({"state": "waiting", "pid": 42, "session_id": "s", "since": 1.0}));
        assert_eq!(live_override(&r, &claude), Some(("working", 7.0)));
        let r = rec(json!({"state": "waiting", "pid": 42, "session_id": "s", "since": 7.0}));
        assert_eq!(live_override(&r, &claude), None);
        // since 가 없으면 0 으로 본다
        let r = rec(json!({"state": "waiting", "pid": 42, "session_id": "s"}));
        assert_eq!(live_override(&r, &claude), Some(("working", 7.0)));
        // 다른 세션의 파일·idle 기록은 겹치지 않는다
        let r = rec(json!({"state": "waiting", "pid": 42, "session_id": "z", "since": 1.0}));
        assert_eq!(live_override(&r, &claude), None);
        let r = rec(json!({"state": "idle", "pid": 42, "session_id": "s", "since": 1.0, "transcript_path": t}));
        assert_eq!(live_override(&r, &claude), None);
        fs::remove_dir_all(&root).unwrap();
    }
}
