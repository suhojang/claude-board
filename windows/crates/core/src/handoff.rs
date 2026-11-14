//! 대화를 백그라운드로 넘긴 세션 알아보기와 카드에 보일 세션 거르기.
//!
//! Claude Code 는 대화를 새 세션 id 로 넘기면(`/bg`) 원래 대화 기록 끝에
//! `{"type":"continued-in","continuedInSessionId":"<새 id>",…}` 줄을 남기고, 그 뒤 hook 은 모두 새 id 로 온다.
//! 원래 세션 파일은 대기로 멈추므로 보드는 그 카드를 숨겨야 한다. 규칙은 macOS 의 두 곳과 1:1 이다.
//! - `Sources/ClaudeBoard/BoardData.swift` 의 `HandoffReader`(대화 기록 꼬리 읽기)
//! - `Sources/BoardRules/Sessions.swift` 의 `visibleSessions`·`sessionID`(카드에 보일 세션)
//!
//! 순수 함수는 [`handed_off_to_in`]·[`visible_sessions`]·[`session_id`] 이고, 파일을 읽는 일은 [`handed_off_to`] 가 맡는다.
//! 보드 스냅샷에 잇는 일(mtime 캐시·목록·개수에서 빼기)은 이 모듈 밖이다.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use crate::text::strip_utf8_bom;
use crate::{clock, notify, Record};

/// 대화 기록 끝에서 읽는 바이트 수(`HandoffReader.read`).
pub const HANDOFF_TAIL: u64 = 256_000;
/// 넘김 줄의 `type` 값. 따옴표까지 넣어 찾는다 — 대화 글 속의 것은 `\"` 로 감싸여 걸리지 않는다.
const MARK: &[u8] = b"\"continued-in\"";

/// 이어 받은 세션 — 새 세션 id 와 넘긴 시각(유닉스 초).
#[derive(Debug, Clone, PartialEq)]
pub struct HandedOff {
    pub id: String,
    pub at: f64,
}

/// 대화 기록 끝(`tail`)에서 넘김 줄을 읽는다. 줄 뒤에 대화 줄(사이드체인 아닌 user·assistant)이 있으면
/// 사람이 원래 세션을 다시 이어 쓴 것이라 None 이다. 마지막 넘김 줄이 이긴다(뒤에서부터 읽는다).
/// `mtime` 은 `timestamp` 가 없거나 못 풀 때 쓸 파일 수정 시각(유닉스 초).
///
/// JSON 이 아니거나 UTF-8 이 깨진 줄(읽는 창에서 잘린 첫 줄 포함)·`type` 이 글자가 아닌 줄은 건너뛴다.
/// 넘김 줄의 `continuedInSessionId` 가 글자가 아니거나 비면 이어 받은 곳이 없는 것으로 읽고 거기서 멈춘다.
pub fn handed_off_to_in(tail: &[u8], mtime: f64) -> Option<HandedOff> {
    // 표시가 없으면 JSON 을 풀지 않는다
    if !tail.windows(MARK.len()).any(|w| w == MARK) {
        return None;
    }
    for line in tail.rsplit(|&b| b == b'\n') {
        let Some(entry) = parse_line(line) else { continue };
        let Some(kind) = entry.get("type").and_then(Value::as_str) else { continue };
        if kind == "continued-in" {
            return match entry.get("continuedInSessionId").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => {
                    let at = entry.get("timestamp").and_then(Value::as_str).and_then(clock::iso_date);
                    Some(HandedOff { id: id.to_string(), at: at.unwrap_or(mtime) })
                }
                _ => None,
            };
        }
        if matches!(kind, "user" | "assistant") && entry.get("isSidechain") != Some(&Value::Bool(true)) {
            return None; // 넘긴 뒤 다시 이어 썼다
        }
    }
    None
}

/// 대화 기록 파일 끝 [`HANDOFF_TAIL`] 바이트로 [`handed_off_to_in`] 을 돌린다. 파일이 없거나 못 읽으면 None.
pub fn handed_off_to(transcript: &Path) -> Option<HandedOff> {
    let tail = notify::read_tail(transcript, HANDOFF_TAIL)?;
    let mtime = std::fs::metadata(transcript)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(clock::now, |d| d.as_secs_f64());
    handed_off_to_in(&tail, mtime)
}

/// JSONL 한 줄 — 줄 끝 `\r` 과 줄 앞 BOM 을 떼고 객체로 푼다. 못 풀면 None.
fn parse_line(line: &[u8]) -> Option<Record> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    match serde_json::from_slice::<Value>(strip_utf8_bom(line)) {
        Ok(Value::Object(entry)) => Some(entry),
        _ => None,
    }
}

/// 세션 id — 기록의 `session_id`(글자일 때), 없으면 파일 이름에서 `.json` 을 뗀 것(`sessionID`).
pub fn session_id(record: &Record, file_name: &str) -> String {
    if let Some(id) = record.get("session_id").and_then(Value::as_str) {
        return id.to_string();
    }
    file_name.strip_suffix(".json").unwrap_or(file_name).to_string()
}

/// 카드에 보일 세션 — 넘긴 세션(`handed_to`: 넘긴 id → 이어 받은 id)은 이어 받은 세션이 목록에 있을 때만 빼고
/// (그 세션 파일이 없으면 지금처럼 보인다), 보드 탭이 이어 받은 작업을 따라가는 동안의 탭 claude 세션(`parked_from`)은 늘 뺀다.
/// 원래 순서를 지킨다. 탭 짝짓기는 이 목록이 아니라 모든 세션을 본다.
pub fn visible_sessions<'a, T>(
    sessions: &'a [T],
    id: impl Fn(&T) -> &str,
    handed_to: &HashMap<String, String>,
    parked_from: &HashSet<String>,
) -> Vec<&'a T> {
    if handed_to.is_empty() && parked_from.is_empty() {
        return sessions.iter().collect();
    }
    let ids: HashSet<&str> = sessions.iter().map(&id).collect();
    sessions
        .iter()
        .filter(|s| {
            let sid = id(s);
            !parked_from.contains(sid) && handed_to.get(sid).is_none_or(|to| !ids.contains(to.as_str()))
        })
        .collect()
}
