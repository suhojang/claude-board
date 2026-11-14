//! 판정 명령이 실패했을 때 엔진이 남기는 글(원본 `Sources/AutopilotRules/Checks.swift`).
//! 판정 실패 뒤 다시 맡길지·미통과로 끝낼지는 `apAfterCheck`(Inbox)가 정하고, 여기는 그때의 진행 기록 글·모델에게 넘기는 글·미통과 까닭·상한 글이다.
//! 시간 상한(`check_timeout_min`)에 닿아 멈춘 판정도 실패로 보되 글에는 상한에 닿았다고 적는다. `timed_out` 은 그때의 상한(초).
//! 모두 순수 함수다. 파일·시각은 건드리지 않는다.

use serde_json::{json, Value};

/// 원본: `apLimitText`. 1분 아래는 초, 딱 나뉘면 분, 아니면 분과 초(0.1분이 "0분"으로 보이지 않게)
pub fn limit_text(sec: f64) -> String {
    // Swift `rounded()` 와 같이 .5 는 0 에서 먼 쪽으로 올린다(f64::round 도 같다)
    let s = (sec.round() as i64).max(0);
    if s < 60 {
        format!("{s}초")
    } else if s % 60 == 0 {
        format!("{}분", s / 60)
    } else {
        format!("{}분 {}초", s / 60, s % 60)
    }
}

/// 원본: `apCheckFailEvent`. 판정 실패 뒤 다시 맡길 때의 진행 기록 글.
/// `tries` 는 이미 다시 맡긴 횟수(0 이면 첫 판정), `retries` 는 다시 맡길 수 있는 횟수.
/// 횟수가 다 찼는데 다시 맡기는 것은 그 사이 사람이 이 단위에 보낸 말이 있을 때뿐이다.
pub fn check_fail_event(code: i32, tries: i64, retries: i64, timed_out: Option<f64>) -> String {
    let head = match timed_out {
        Some(t) => format!("판정 명령이 시간 상한({})에 닿아 엔진이 멈췄습니다(실패로 봅니다).", limit_text(t)),
        None => format!("판정 실패(종료 코드 {code})."),
    };
    if tries < retries {
        format!("{head} 실패 출력을 모델에게 넘겨 다시 고치게 합니다({}/{retries}).", tries + 1)
    } else {
        format!("{head} 다시 맡길 횟수는 다 찼지만 그 사이 사람이 이 단위에 보낸 말이 있어, 그 말과 실패 출력을 넘겨 한 번 더 맡깁니다.")
    }
}

/// 원본: `apCheckFailMessage`. 판정 실패 뒤 같은 세션에 넘기는 글(실패 코드·판정 명령·출력 끝부분)
pub fn check_fail_message(code: i32, check: &str, tail: &str, timed_out: Option<f64>) -> String {
    match timed_out {
        Some(t) => format!(
            "판정 명령이 시간 상한({})에 닿아 끝나지 않아 엔진이 멈췄습니다(실패로 봅니다). 판정 명령: `{check}`\n\
             아래는 멈출 때까지의 출력 끝부분입니다. 판정이 끝나지 않는 까닭(끝나지 않는 시험, 응답을 기다리는 서버·입력 등)을 고쳐 상한 안에 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.\n\
             \n```\n{tail}\n```",
            limit_text(t)
        ),
        None => format!(
            "판정 명령이 실패했습니다(종료 코드 {code}). 판정 명령: `{check}`\n\
             아래는 출력 끝부분입니다. 원인을 고쳐 판정을 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.\n\
             \n```\n{tail}\n```"
        ),
    }
}

/// 원본: `apCheckFailReason`. 다시 맡겨도 판정을 통과하지 못해 미통과로 끝낼 때의 까닭.
/// `timed_out`: 마지막 판정이 시간 상한에 닿아 멈췄으면 그 상한(초)
pub fn check_fail_reason(tries: i64, timed_out: Option<f64>) -> String {
    let again = if tries == 0 { String::new() } else { format!("{tries}번 다시 해도 ") };
    let limit = timed_out.map(|t| format!("(마지막 판정은 시간 상한 {}에 닿아 멈춤)", limit_text(t))).unwrap_or_default();
    format!("{again}판정 명령을 통과하지 못해{limit} 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다.")
}

/// 원본: `apCheckLimitNote`. 출발 전 status 의 판정 줄 끝에 붙는 상한 글(단위에 `check_timeout_min` 이 있을 때).
/// `minutes` 가 수가 아니면 `None`
pub fn check_limit_note(minutes: Option<f64>) -> String {
    format!(" ({} 상한)", minutes.map_or_else(|| "?분".to_string(), |m| limit_text(m * 60.0)))
}

/// 원본: `apOutputTail`. 판정·고정 단계 출력의 끝부분: 마지막 4000 바이트를 UTF-8 로 풀어 마지막 40줄.
/// 모델에게 넘기는 실패 글·engine.json 의 tail·prerun.json 의 tail 이 이 글이다.
/// 줄은 `\n` 으로만 가른다. Swift 는 `\r\n` 을 글자 하나로 세어 CRLF 출력을 가르지 못했으나,
/// Windows 출력은 CRLF 라 줄 수 의도(40줄)를 지키려고 `\n` 에서 가르고 `\r` 은 줄 끝에 그대로 둔다.
pub fn output_tail(data: &[u8]) -> String {
    let start = data.len().saturating_sub(4000);
    let text = String::from_utf8_lossy(&data[start..]);
    let lines: Vec<&str> = text.split('\n').collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

/// 원본: `apPrerunCard`. 판정 미리 돌리기에서 일하기 전부터 통과한 단위의 출발 카드 감(prerun.json 의 cards)
pub fn prerun_card(n: i64, id: &str, title: &str) -> Value {
    json!({
        "id": format!("prerun-{id}"),
        "unit": n,
        "kind": "prerun",
        "title": format!("「{title}」 의 판정이 일하기 전부터 통과합니다"),
        "why": "이미 끝난 일이거나 판정이 약하다는 뜻입니다. 그대로 두면 모델이 일하지 않아도 통과로 기록됩니다.",
        "options": [
            {"key": "drop", "label": "빼기"},
            {"key": "fix_check", "label": "판정 고치기"},
            {"key": "keep", "label": "그대로"},
        ],
        "recommended": "drop",
    })
}
