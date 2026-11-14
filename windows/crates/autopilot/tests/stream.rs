//! stream-json 해석기 시험. 사건 종류마다·한도 거절 세 꼴·빈 result 규칙·깨진 줄·CRLF·1MB 줄.
use claude_board_autopilot::stream::*;
use serde_json::{json, Value};

const SID: &str = "sid-1";

fn feed(st: &mut StreamState, v: Value, now: f64) -> Vec<Emit> {
    let ev = parse_line(&v.to_string());
    st.feed(&ev, now)
}

fn state() -> StreamState {
    StreamState::new(SID, 100.0)
}

fn assistant(content: Value) -> Value {
    json!({"type": "assistant", "message": {"role": "assistant", "content": content}})
}

fn result(turns: i64, text: &str, is_error: bool) -> Value {
    json!({"type": "result", "subtype": "success", "num_turns": turns, "result": text, "is_error": is_error})
}

// ---- 사건 종류 ----

#[test]
fn system_init_confirms_session_only_for_expected_id() {
    let mut st = state();
    let out = feed(&mut st, json!({"type": "system", "subtype": "init", "session_id": "other"}), 101.0);
    assert!(out.is_empty());
    assert!(!st.session_started);
    assert_eq!(st.session_id.as_deref(), Some("other"));
    assert!(st.open, "init 은 턴을 연다");
    assert_eq!(st.last_line, 101.0);

    let out = feed(&mut st, json!({"type": "system", "subtype": "init", "session_id": SID}), 102.0);
    assert_eq!(out, vec![Emit::SessionStarted]);
    assert!(st.session_started);
    // 두 번째 init 은 다시 알리지 않는다
    assert!(feed(&mut st, json!({"type": "system", "subtype": "init", "session_id": SID}), 103.0).is_empty());
}

#[test]
fn background_tasks_changed_counts_tasks() {
    let mut st = state();
    feed(&mut st, json!({"type": "system", "subtype": "background_tasks_changed", "tasks": [{"id": 1}, {"id": 2}]}), 1.0);
    assert_eq!(st.bg, 2);
    feed(&mut st, json!({"type": "system", "subtype": "background_tasks_changed"}), 2.0);
    assert_eq!(st.bg, 0, "tasks 가 없으면 0");
    assert!(!st.open, "background_tasks_changed 는 턴을 열지 않는다");
}

#[test]
fn api_retry_emits_note() {
    let mut st = state();
    let out = feed(&mut st, json!({"type": "system", "subtype": "api_retry", "attempt": 3}), 1.0);
    assert_eq!(out, vec![Emit::Note("API 다시 시도 3번째".into())]);
    let out = feed(&mut st, json!({"type": "system", "subtype": "api_retry"}), 1.0);
    assert_eq!(out, vec![Emit::Note("API 다시 시도 ?번째".into())]);
}

#[test]
fn assistant_text_is_deduped_and_main_only() {
    let mut st = state();
    let out = feed(&mut st, assistant(json!([{"type": "text", "text": "  첫 줄입니다\n둘째 줄"}])), 5.0);
    assert_eq!(out, vec![Emit::ModelText("첫 줄입니다".into())]);
    assert!(st.open);
    // 같은 첫 줄은 되풀이하지 않는다
    assert!(feed(&mut st, assistant(json!([{"type": "text", "text": "첫 줄입니다"}])), 6.0).is_empty());
    // 보조 에이전트 글은 내지도 꼬리에 넣지도 않는다
    let mut sub = assistant(json!([{"type": "text", "text": "보조의 말"}]));
    sub["parent_tool_use_id"] = json!("toolu_x");
    assert!(feed(&mut st, sub, 7.0).is_empty());
    assert_eq!(st.text_tail, "  첫 줄입니다\n둘째 줄\n첫 줄입니다");
    // 빈 글은 내지 않는다
    assert!(feed(&mut st, assistant(json!([{"type": "text", "text": "  \n "}])), 8.0).is_empty());
    // parent_tool_use_id 가 null 이면 본 세션이다
    let mut nul = assistant(json!([{"type": "text", "text": "다른 말"}]));
    nul["parent_tool_use_id"] = Value::Null;
    assert_eq!(feed(&mut st, nul, 9.0), vec![Emit::ModelText("다른 말".into())]);
}

#[test]
fn long_model_text_is_cut_with_ellipsis() {
    let mut st = state();
    let long = "가".repeat(200);
    let out = feed(&mut st, assistant(json!([{"type": "text", "text": long}])), 1.0);
    assert_eq!(out, vec![Emit::ModelText(format!("{}…", "가".repeat(140)))]);
    assert_eq!(st.text_tail.chars().count(), 200);
}

#[test]
fn text_tail_keeps_only_last_chars() {
    let mut st = state();
    let long = format!("{}{}", "a".repeat(TEXT_TAIL_CHARS), "끝");
    feed(&mut st, assistant(json!([{"type": "text", "text": long}])), 1.0);
    assert_eq!(st.text_tail.chars().count(), TEXT_TAIL_CHARS);
    assert!(st.text_tail.ends_with('끝'));
}

#[test]
fn tool_uses_are_recorded_and_looked_is_flushed() {
    let mut st = state();
    let out = feed(
        &mut st,
        assistant(json!([
            {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/a"}},
            {"type": "tool_use", "id": "t2", "name": "Grep", "input": {"pattern": "x"}},
            {"type": "tool_use", "id": "t3", "name": "Edit", "input": {"file_path": "/a", "old_string": "1", "new_string": "2"}},
        ])),
        1.0,
    );
    assert_eq!(st.looked, 0);
    let kinds: Vec<&str> = out
        .iter()
        .map(|e| match e {
            Emit::ToolUse(t) => t.name.as_str(),
            Emit::Looked(2) => "looked2",
            _ => "?",
        })
        .collect();
    assert_eq!(kinds, vec!["Read", "Grep", "looked2", "Edit"]);
    assert_eq!(st.tool_uses.len(), 3);
    assert_eq!(st.tool_uses[2].name, "Edit");
    assert_eq!(st.tool_uses[2].input["file_path"], "/a");
    assert!(st.tool_uses[2].main);

    let taken = st.take_tool_uses();
    assert_eq!(taken.len(), 3);
    assert!(st.tool_uses.is_empty());
}

#[test]
fn tool_use_defaults_and_sub_agent() {
    let mut st = state();
    let mut sub = assistant(json!([{"type": "tool_use", "input": "not-an-object"}, {"type": "image"}]));
    sub["parent_tool_use_id"] = json!("p");
    let out = feed(&mut st, sub, 1.0);
    assert_eq!(out.len(), 1);
    let Emit::ToolUse(t) = &out[0] else { panic!("ToolUse 가 아님") };
    assert_eq!((t.id.as_str(), t.name.as_str(), t.main), ("", "?", false));
    assert_eq!(t.input, json!({}));
    assert_eq!(st.looked, 0, "보조의 읽기는 모으지 않는다");
}

#[test]
fn mixed_content_array_is_ignored_whole() {
    // 원본은 `as? [[String: Any]]` 라 객체가 아닌 원소가 하나라도 있으면 통째로 읽지 않는다
    let mut st = state();
    let out = feed(&mut st, assistant(json!([{"type": "text", "text": "보임"}, "문자열"])), 1.0);
    assert!(out.is_empty());
    assert!(st.open);
}

#[test]
fn tool_input_summary_cuts_at_600_chars() {
    let input = json!({"command": "x".repeat(2000)});
    assert_eq!(tool_input_summary(&input).chars().count(), 600);
    assert_eq!(tool_input_summary(&json!({"a": 1})), r#"{"a":1}"#);
}

#[test]
fn user_tool_results_string_and_array() {
    let mut st = state();
    let out = feed(
        &mut st,
        json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "결과"},
            {"type": "tool_result", "tool_use_id": "t2", "is_error": true, "content": [{"type": "text", "text": "a"}, {"type": "image"}, {"type": "text", "text": "b"}]},
            {"type": "text", "text": "무시"},
        ]}}),
        1.0,
    );
    assert!(st.open);
    assert_eq!(
        out,
        vec![
            Emit::ToolResult { tool_use_id: "t1".into(), is_error: false, output: "결과".into() },
            Emit::ToolResult { tool_use_id: "t2".into(), is_error: true, output: "a\nb".into() },
        ]
    );
    let out = feed(
        &mut st,
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t3", "content": "y".repeat(1000)}]}}),
        2.0,
    );
    let Emit::ToolResult { output, .. } = &out[0] else { panic!() };
    assert_eq!(output.chars().count(), 400);
}

#[test]
fn rate_limit_event_updates_usage_windows() {
    let mut st = state();
    let out = feed(
        &mut st,
        json!({"type": "rate_limit_event", "rate_limit_info": {
            "status": "allowed",
            "unifiedWindows": {
                "five_hour": {"utilization": 0.42, "resetsAt": 1000},
                "seven_day": {"utilization": 0.07, "resetsAt": 5000}
            }}}),
        200.12345,
    );
    assert!(out.is_empty());
    assert!(!st.limit);
    let u = st.usage.clone().unwrap();
    assert_eq!(u.five_hour, Some(0.42));
    assert_eq!(u.five_hour_resets, Some(1000.0));
    assert_eq!(u.seven_day, Some(0.07));
    assert_eq!(u.seven_day_resets, Some(5000.0));
    assert_eq!(u.at, 200.123);
    assert!(!st.open, "rate_limit_event 는 턴을 열지 않는다");

    // 한 창만 오면 다른 창은 그대로, 온 창의 값이 없으면 지워진다(원본: nil 대입)
    feed(&mut st, json!({"type": "rate_limit_event", "rate_limit_info": {"unifiedWindows": {"five_hour": {"resetsAt": 2000}}}}), 300.0);
    let u = st.usage.clone().unwrap();
    assert_eq!(u.five_hour, None);
    assert_eq!(u.five_hour_resets, Some(2000.0));
    assert_eq!(u.seven_day, Some(0.07));
    assert_eq!(u.at, 300.0);
}

#[test]
fn rate_limit_event_flat_form_only_when_windows_empty() {
    let mut st = state();
    feed(&mut st, json!({"type": "rate_limit_event", "rate_limit_info": {"rateLimitType": "seven_day", "utilization": "0.5", "resetsAt": 9000}}), 10.0);
    let u = st.usage.clone().unwrap();
    assert_eq!(u.seven_day, Some(0.5));
    assert_eq!(u.seven_day_resets, Some(9000.0));
    assert_eq!(u.five_hour, None);

    // 모르는 창 종류는 무시, unifiedWindows 가 비어 있지 않으면 평평한 값을 쓰지 않는다
    let mut st = state();
    feed(&mut st, json!({"type": "rate_limit_event", "rate_limit_info": {"rateLimitType": "opus", "utilization": 0.9}}), 10.0);
    assert_eq!(st.usage.clone().unwrap().five_hour, None);
    feed(
        &mut st,
        json!({"type": "rate_limit_event", "rate_limit_info": {"rateLimitType": "five_hour", "utilization": 0.9, "unifiedWindows": {"other": {}}}}),
        11.0,
    );
    assert_eq!(st.usage.clone().unwrap().five_hour, None);
}

#[test]
fn rate_limit_event_without_info_still_stamps_time() {
    let mut st = state();
    feed(&mut st, json!({"type": "rate_limit_event"}), 7.0);
    assert_eq!(st.usage.clone().unwrap().at, 7.0);
}

// ---- 한도 거절 세 꼴 ----

#[test]
fn refusal_shape_1_rate_limit_event_rejected() {
    let mut st = state();
    feed(
        &mut st,
        json!({"type": "rate_limit_event", "rate_limit_info": {
            "status": "rejected", "rateLimitType": "seven_day", "resetsAt": 111,
            "unifiedWindows": {"seven_day": {"utilization": 1.0, "resetsAt": 777}}}}),
        1.0,
    );
    assert!(st.limit);
    assert_eq!(st.limit_resets, Some(777.0), "unifiedWindows[rateLimitType].resetsAt 가 먼저");
    assert_eq!(st.usage.clone().unwrap().seven_day, Some(1.0));

    // 창에 resetsAt 이 없으면 info.resetsAt
    let mut st = state();
    feed(&mut st, json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "rateLimitType": "five_hour", "resetsAt": 111}}), 1.0);
    assert!(st.limit);
    assert_eq!(st.limit_resets, Some(111.0));

    // 시각이 없으면 앞서 안 값을 지키고, 허용 상태는 거절이 아니다
    feed(&mut st, json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected"}}), 2.0);
    assert_eq!(st.limit_resets, Some(111.0));
    let mut ok = state();
    feed(&mut ok, json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed_warning", "resetsAt": 5}}), 1.0);
    assert!(!ok.limit);
}

#[test]
fn refusal_shape_2_assistant_rate_limit_error() {
    let mut st = state();
    let mut a = assistant(json!([{"type": "text", "text": "limit reached"}]));
    a["error"] = json!("rate_limit");
    a["api_error_params"] = json!({"rate_limit_info": {"rateLimitType": "five_hour", "unifiedWindows": {"five_hour": {"resetsAt": 4242}}}});
    feed(&mut st, a, 1.0);
    assert!(st.limit);
    assert_eq!(st.limit_resets, Some(4242.0));

    // api_error_status 429 만 있어도 거절이고, 시각이 없으면 비워 둔다
    let mut st = state();
    let mut a = assistant(json!([]));
    a["api_error_status"] = json!(429);
    feed(&mut st, a, 1.0);
    assert!(st.limit);
    assert_eq!(st.limit_resets, None);

    // 다른 오류는 거절이 아니다
    let mut st = state();
    let mut a = assistant(json!([]));
    a["error"] = json!("server_error");
    a["api_error_status"] = json!(500);
    feed(&mut st, a, 1.0);
    assert!(!st.limit);
}

#[test]
fn refusal_shape_3_result_is_error_with_429_or_usage_limit() {
    let mut st = state();
    let mut r = result(1, "You've hit your limit", true);
    r["api_error_status"] = json!(429);
    feed(&mut st, r, 1.0);
    assert!(st.limit);
    assert!(st.result_seen);
    assert!(st.last_error);
    assert_eq!(st.err_text, "You've hit your limit");

    let mut st = state();
    let mut r = result(1, "x", true);
    r["api_error"] = json!("usage_limit_reached");
    feed(&mut st, r, 1.0);
    assert!(st.limit, "subtype 은 success 로 와도 is_error 와 usage_limit_reached 면 거절");

    // is_error 가 아니면 429 가 있어도 거절이 아니다
    let mut st = state();
    let mut r = result(1, "x", false);
    r["api_error_status"] = json!(429);
    r["api_error"] = json!("usage_limit_reached");
    feed(&mut st, r, 1.0);
    assert!(!st.limit);

    // 다른 오류 코드는 거절이 아니다
    let mut st = state();
    let mut r = result(1, "x", true);
    r["api_error_status"] = json!(500);
    feed(&mut st, r, 1.0);
    assert!(!st.limit);
}

// ---- result ----

#[test]
fn result_counts_cost_turns_and_denials() {
    let mut st = state();
    feed(&mut st, assistant(json!([{"type": "tool_use", "id": "t", "name": "Glob", "input": {}}])), 1.0);
    assert_eq!(st.looked, 1);
    let mut r = result(3, "끝", false);
    r["total_cost_usd"] = json!(1.25);
    r["session_id"] = json!("sid-r");
    r["permission_denials"] = json!([{"tool_name": "Bash", "tool_input": {"command": "rm -rf /"}}, {"tool_input": 5}]);
    let out = feed(&mut st, r, 2.0);
    assert!(st.result_seen);
    assert!(!st.open);
    assert!(!st.last_error);
    assert_eq!(st.cost, 1.25);
    assert_eq!(st.turns_total, 3);
    assert_eq!(st.session_id.as_deref(), Some("sid-r"));
    assert_eq!(st.looked, 0);
    assert_eq!(
        out,
        vec![
            Emit::Looked(1),
            Emit::Denial(Denial { tool_name: "Bash".into(), tool_input: json!({"command": "rm -rf /"}) }),
            Emit::Denial(Denial { tool_name: "?".into(), tool_input: json!({}) }),
        ]
    );

    // 비용은 줄지 않고(max), 턴 수는 쌓인다
    let mut r = result(2, "또", false);
    r["total_cost_usd"] = json!(0.5);
    feed(&mut st, r, 3.0);
    assert_eq!(st.cost, 1.25);
    assert_eq!(st.turns_total, 5);
}

#[test]
fn result_budget_and_error_text() {
    let mut st = state();
    let mut r = json!({"type": "result", "subtype": "error_max_budget_usd", "num_turns": 4, "is_error": false});
    r["total_cost_usd"] = json!(100.0);
    feed(&mut st, r, 1.0);
    assert!(st.budget);
    assert!(st.result_seen);
    assert_eq!(st.cost, 100.0);

    let mut st = state();
    feed(&mut st, json!({"type": "result", "subtype": "error_during_execution", "num_turns": 1, "is_error": true}), 1.0);
    assert_eq!(st.err_text, "error_during_execution", "글이 없으면 subtype");
}

#[test]
fn string_numbers_are_accepted_like_the_original() {
    let mut st = state();
    let mut r = result(1, "x", false);
    r["num_turns"] = json!("2");
    r["total_cost_usd"] = json!("0.75");
    feed(&mut st, r, 1.0);
    assert_eq!(st.turns_total, 2);
    assert_eq!(st.cost, 0.75);
}

#[test]
fn empty_result_is_not_a_real_end() {
    let mut st = state();
    feed(&mut st, json!({"type": "system", "subtype": "init", "session_id": SID}), 1.0);
    assert!(st.open);
    let out = feed(&mut st, result(0, "", false), 2.0);
    assert!(!st.result_seen, "빈 result 는 끝으로 세지 않는다");
    assert!(!st.open, "열린 턴은 닫는다");
    assert_eq!(st.turns_total, 0);
    assert_eq!(st.last_line, 2.0);
    assert_eq!(out, vec![Emit::Note(EMPTY_RESULT_NOTE.into())]);

    // num_turns 필드가 없어도 같다
    let mut st = state();
    feed(&mut st, json!({"type": "result", "subtype": "success"}), 1.0);
    assert!(!st.result_seen);

    // 글이 있거나 오류거나 턴이 0 이 아니면 진짜 result 다
    for (turns, text, err) in [(0, "글", false), (0, "", true), (1, "", false)] {
        let mut st = state();
        feed(&mut st, result(turns, text, err), 1.0);
        assert!(st.result_seen, "turns={turns} text={text:?} err={err}");
    }
}

#[test]
fn empty_result_after_real_one_keeps_result_seen() {
    let mut st = state();
    feed(&mut st, result(2, "끝", false), 1.0);
    feed(&mut st, assistant(json!([])), 2.0);
    assert!(st.open);
    feed(&mut st, result(0, "", false), 3.0);
    assert!(st.result_seen);
    assert!(!st.open);
}

#[test]
fn restart_turn_resets_turn_fields_only() {
    let mut st = state();
    feed(&mut st, json!({"type": "system", "subtype": "init", "session_id": SID}), 1.0);
    feed(&mut st, json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": 9}}), 2.0);
    feed(&mut st, assistant(json!([{"type": "text", "text": "글"}, {"type": "tool_use", "id": "t", "name": "Read", "input": {}}])), 3.0);
    let mut r = result(2, "끝", true);
    r["total_cost_usd"] = json!(3.0);
    feed(&mut st, r, 4.0);
    st.restart_turn(50.0);
    assert!(!st.result_seen && !st.open && !st.last_error && !st.limit && !st.budget);
    assert_eq!((st.limit_resets, st.cost, st.looked, st.bg), (None, 0.0, 0, 0));
    assert_eq!((st.last_line, st.last_text.as_str(), st.err_text.as_str()), (50.0, "", ""));
    // 단위에 걸친 값은 남는다
    assert_eq!(st.turns_total, 2);
    assert!(st.session_started);
    assert!(st.usage.is_some());
    assert_eq!(st.tool_uses.len(), 1);
    assert_eq!(st.text_tail, "글");
}

// ---- 읽기 ----

#[test]
fn parse_line_kinds() {
    assert!(matches!(parse_line(r#"{"type":"system","subtype":"init","session_id":"s"}"#), StreamEvent::System { .. }));
    assert!(matches!(parse_line(r#"{"type":"assistant","message":{"content":[]}}"#), StreamEvent::Assistant { main: true, .. }));
    assert!(matches!(parse_line(r#"{"type":"user","message":{"content":[]}}"#), StreamEvent::User { .. }));
    assert!(matches!(parse_line(r#"{"type":"rate_limit_event"}"#), StreamEvent::RateLimit(_)));
    assert!(matches!(parse_line(r#"{"type":"result"}"#), StreamEvent::Result(_)));
    assert_eq!(parse_line(r#"{"type":"stream_event","event":{}}"#), StreamEvent::Other);
    assert_eq!(parse_line(r#"{"type":7}"#), StreamEvent::Other);
    assert_eq!(parse_line("{}"), StreamEvent::Other);
}

#[test]
fn garbage_lines_never_panic_and_change_nothing() {
    let mut deep = "[".repeat(100_000);
    deep.push_str(&"]".repeat(100_000));
    let lines = [
        "", "   ", "\n", "\r\n", "not json", "{", "}", "[1,2,3]", "42", "\"type\"", "null", "true", "{\"type\":", "{\"type\":\"result\"",
        "\u{feff}{\"type\":\"result\"}", "\u{0}\u{1}\u{2}", "{\"type\":\"result\"}}", "💥", deep.as_str(),
    ];
    let mut st = state();
    for l in lines {
        let ev = parse_line(l);
        assert_eq!(ev, StreamEvent::Garbage, "줄: {:?}", l.chars().take(20).collect::<String>());
        assert!(st.feed(&ev, 999.0).is_empty());
    }
    assert_eq!(st.last_line, 100.0, "읽을 수 없는 줄은 마지막 줄 시각을 바꾸지 않는다");
    assert!(!st.open && !st.result_seen && !st.limit);
    assert_eq!(st, state());
}

#[test]
fn unknown_type_updates_last_line_but_nothing_else() {
    let mut st = state();
    assert!(feed(&mut st, json!({"type": "stream_event"}), 55.0).is_empty());
    assert_eq!(st.last_line, 55.0);
    st.last_line = 100.0;
    assert_eq!(st, state());
}

#[test]
fn crlf_terminated_line_parses_like_lf() {
    let line = "{\"type\":\"result\",\"subtype\":\"success\",\"num_turns\":1,\"result\":\"ok\",\"total_cost_usd\":0.5}\r\n";
    assert_eq!(parse_line(line), parse_line(line.trim_end()));
    let mut st = state();
    st.feed(&parse_line(line), 1.0);
    assert!(st.result_seen);
    assert_eq!(st.cost, 0.5);
    // 줄 끝의 \r 만 있어도, 줄바꿈이 여러 개여도 같다
    assert!(matches!(parse_line("{\"type\":\"user\"}\r"), StreamEvent::User { .. }));
    assert!(matches!(parse_line("{\"type\":\"user\"}\r\n\r\n"), StreamEvent::User { .. }));
}

#[test]
fn one_megabyte_lines_do_not_panic() {
    let big = "가".repeat(350_000); // 약 1MB(UTF-8)
    assert!(big.len() > 1_000_000);

    // 깨진 1MB 줄
    let garbage = format!("{{\"type\":\"result\",\"result\":\"{big}");
    let mut st = state();
    assert_eq!(parse_line(&garbage), StreamEvent::Garbage);
    assert!(st.feed(&parse_line(&garbage), 1.0).is_empty());

    // 1MB 글이 든 assistant: 진행 기록 글은 140자, 꼬리는 4000자
    let line = assistant(json!([{"type": "text", "text": big}])).to_string() + "\r\n";
    assert!(line.len() > 1_000_000);
    let out = st.feed(&parse_line(&line), 2.0);
    let Emit::ModelText(s) = &out[0] else { panic!("ModelText 가 아님") };
    assert_eq!(s.chars().count(), 141);
    assert_eq!(st.text_tail.chars().count(), TEXT_TAIL_CHARS);

    // 1MB 도구 입력: 기록은 그대로, 요약은 600자
    let line = assistant(json!([{"type": "tool_use", "id": "t", "name": "Write", "input": {"content": big}}])).to_string();
    st.feed(&parse_line(&line), 3.0);
    assert_eq!(tool_input_summary(&st.tool_uses[0].input).chars().count(), 600);

    // 1MB 도구 결과
    let line = json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t", "content": big}]}}).to_string();
    let out = st.feed(&parse_line(&line), 4.0);
    let Emit::ToolResult { output, .. } = &out[0] else { panic!() };
    assert_eq!(output.chars().count(), 400);

    // 1MB 오류 글이 든 result: 오류 글은 120자에서 자른다
    let line = result(1, &big, true).to_string();
    st.feed(&parse_line(&line), 5.0);
    assert!(st.result_seen);
    assert_eq!(st.err_text.chars().count(), 121);

    // 1MB 공백뿐인 줄·1MB 숫자
    assert_eq!(parse_line(&" ".repeat(1_000_000)), StreamEvent::Garbage);
    assert_eq!(parse_line(&"9".repeat(1_000_000)), StreamEvent::Garbage);
}

#[test]
fn extreme_numbers_do_not_panic() {
    let mut st = state();
    let mut r = result(0, "x", false);
    r["num_turns"] = json!(1e300);
    r["total_cost_usd"] = json!("NaN");
    feed(&mut st, r, 1.0);
    assert_eq!(st.turns_total, i64::MAX);
    assert_eq!(st.cost, 0.0, "유한하지 않은 비용은 무시한다");
    let mut r = result(0, "x", false);
    r["num_turns"] = json!(1e300);
    feed(&mut st, r, 2.0);
    assert_eq!(st.turns_total, i64::MAX, "합은 넘치지 않고 멈춘다");
    feed(&mut st, json!({"type": "system", "subtype": "api_retry", "attempt": 1e300}), 3.0);
}
