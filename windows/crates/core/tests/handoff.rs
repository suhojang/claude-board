//! 대화 넘김(`continued-in`) 읽기와 카드에 보일 세션 — macOS `SessionsTests.swift` 의 「카드에 보일 세션」과 `HandoffReader` 규칙.
//! Swift 시험과 이름이 같은 것은 `표`·`끝난_세션`·`세션_id_맞추기` 다. 나머지는 `HandoffReader` 의 규칙을 시험한다.

use std::collections::{HashMap, HashSet};

use claude_board_core::handoff::{handed_off_to, handed_off_to_in, session_id, visible_sessions, HandedOff};
use claude_board_core::Record;

const MTIME: f64 = 1_000.0;
const SENT: &str = r#"{"type":"continued-in","continuedInSessionId":"new-2","timestamp":"2026-10-16T12:00:00.000Z"}"#;
const SENT_AT: f64 = 1_792_152_000.0; // 2026-10-16T12:00:00Z

fn user(text: &str) -> String {
    format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#)
}

fn assistant(text: &str) -> String {
    format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#)
}

fn jsonl(lines: &[&str]) -> Vec<u8> {
    let mut out = lines.join("\n");
    out.push('\n');
    out.into_bytes()
}

fn read(lines: &[&str]) -> Option<HandedOff> {
    handed_off_to_in(&jsonl(lines), MTIME)
}

fn handed(id: &str, at: f64) -> Option<HandedOff> {
    Some(HandedOff { id: id.to_string(), at })
}

// MARK: - 카드에 보일 세션 (SessionsTests.swift)

fn ids_of(sessions: &[String], handed_to: &[(&str, &str)], parked_from: &[&str]) -> Vec<String> {
    let handed_to: HashMap<String, String> = handed_to.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let parked_from: HashSet<String> = parked_from.iter().map(|s| s.to_string()).collect();
    visible_sessions(sessions, |s| s.as_str(), &handed_to, &parked_from).into_iter().cloned().collect()
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// (handedTo, parkedFrom, ids, expected)
type Row = (&'static [(&'static str, &'static str)], &'static [&'static str], &'static [&'static str], &'static [&'static str]);

#[test]
fn 표() {
    // Swift 표와 같은 여섯 줄
    let table: [Row; 6] = [
        (&[], &[], &["a", "b"], &["a", "b"]),
        (&[("a", "b")], &[], &["a", "b"], &["b"]),                     // 이어 받은 b 가 있으면 넘긴 a 를 뺀다
        (&[("a", "z")], &[], &["a", "b"], &["a", "b"]),                // 이어 받은 세션 파일이 없으면 그대로
        (&[], &["a"], &["a", "b"], &["b"]),                            // 보드 탭이 이어 받은 작업을 따라가는 동안의 탭 claude
        (&[("a", "b")], &["c"], &["a", "b", "c", "d"], &["b", "d"]),
        (&[("a", "b"), ("b", "c")], &[], &["a", "b", "c"], &["c"]),    // 두 번 넘김
    ];
    for (handed_to, parked_from, ids, expected) in table {
        assert_eq!(ids_of(&strings(ids), handed_to, parked_from), strings(expected), "{handed_to:?} {parked_from:?} {ids:?}");
    }
}

#[test]
fn 끝난_세션() {
    // 상태가 ended 여도 넘겼으면 뺀다 — 규칙은 id 만 본다
    let sessions = [("a", "ended"), ("b", "working")];
    let handed_to = HashMap::from([("a".to_string(), "b".to_string())]);
    let shown = visible_sessions(&sessions, |s| s.0, &handed_to, &HashSet::new());
    assert_eq!(shown.iter().map(|s| s.0).collect::<Vec<_>>(), ["b"]);
}

#[test]
fn 세션_id_맞추기() {
    // session_id 없는 파일이어도 넘긴 쪽 열쇠와 카드의 id 가 같은 함수로 잡혀 서로 맞는다
    let from_rec: Record = serde_json::from_str(r#"{"state":"waiting"}"#).unwrap();
    let to_rec: Record = serde_json::from_str(r#"{"session_id":"new-2"}"#).unwrap();
    let from = session_id(&from_rec, "old-1.json");
    let to = session_id(&to_rec, "new-2.json");
    assert_eq!((from.as_str(), to.as_str()), ("old-1", "new-2"));
    let sessions = [from.clone(), to.clone()];
    let handed_to = HashMap::from([(from, to)]);
    let shown = visible_sessions(&sessions, |s| s.as_str(), &handed_to, &HashSet::new());
    assert_eq!(shown, [&"new-2".to_string()]);
}

#[test]
fn 세션_id_는_글자일_때만_쓰고_확장자가_없으면_이름_그대로() {
    let number: Record = serde_json::from_str(r#"{"session_id":7}"#).unwrap();
    assert_eq!(session_id(&number, "x.json"), "x");
    assert_eq!(session_id(&Record::new(), "x"), "x");
    assert_eq!(session_id(&Record::new(), ".json"), "");
}

// MARK: - 넘김 줄 읽기 (HandoffReader)

#[test]
fn 넘김만() {
    let got = read(&[&user("안녕"), &assistant("네"), SENT]);
    assert_eq!(got, handed("new-2", SENT_AT));
}

#[test]
fn 넘긴_뒤_이어_씀() {
    // 줄 뒤에 사이드체인 아닌 대화 줄이 있으면 사람이 원래 세션을 다시 쓴 것
    assert_eq!(read(&[SENT, &user("다시 왔다")]), None);
    assert_eq!(read(&[SENT, &assistant("응답")]), None);
}

#[test]
fn 사이드체인과_대화_아닌_줄은_이어_쓴_것이_아니다() {
    let side = r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"서브"}}"#;
    let other = r#"{"type":"cost-state","costUSD":1.5}"#;
    let last = r#"{"type":"last-prompt","lastPrompt":"x"}"#;
    assert_eq!(read(&[SENT, side, other, last]), handed("new-2", SENT_AT));
    // isSidechain 이 false 면 대화 줄이다
    let plain = r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":"x"}}"#;
    assert_eq!(read(&[SENT, plain]), None);
}

#[test]
fn 글_속_문자열() {
    // 대화 글 속의 continued-in 은 \" 로 감싸여 표시로 안 잡힌다
    let quoted = r#"{"type":"user","message":{"role":"user","content":"\"continued-in\" 줄을 찾아라 {\"type\":\"continued-in\",\"continuedInSessionId\":\"evil\"}"}}"#;
    assert_eq!(read(&[&assistant("앞"), quoted]), None);
    // 글 속 줄이 넘김 줄 뒤에 있어도 그것은 대화 줄이라 이어 쓴 것으로 본다
    assert_eq!(read(&[SENT, quoted]), None);
    // 글 속 줄이 앞에 있고 진짜 넘김 줄이 뒤에 있으면 진짜가 이긴다
    assert_eq!(read(&[quoted, SENT]), handed("new-2", SENT_AT));
}

#[test]
fn 표시_글자가_다른_줄_값으로_있어도_대화_줄이면_이어_쓴_것() {
    let tricky = r#"{"type":"user","note":"continued-in","message":{"role":"user","content":"x"}}"#;
    assert_eq!(read(&[SENT, tricky]), None);
}

#[test]
fn 잘린_첫_줄() {
    // 읽는 창이 줄 한가운데서 시작해 JSON 이 아닌 첫 줄은 건너뛴다
    let cut = r#"tent":"잘린 줄 끝","x":{"type":"user"}}"#;
    assert_eq!(read(&[cut, SENT]), handed("new-2", SENT_AT));
    let whole = jsonl(&[&user("가나다 라마바"), SENT]);
    let start = whole.iter().position(|&b| b == 0xEA).unwrap() + 1; // '가'(EA B0 80) 한가운데에서 시작 — UTF-8 이 깨진 첫 줄
    assert_eq!(handed_off_to_in(&whole[start..], MTIME), handed("new-2", SENT_AT));
}

#[test]
fn 잘린_마지막_줄과_깨진_줄은_건너뛴다() {
    let partial = r#"{"type":"user","message":{"role":"us"#;
    let bytes = format!("{SENT}\n{partial}").into_bytes();
    assert_eq!(handed_off_to_in(&bytes, MTIME), handed("new-2", SENT_AT));
    let mut broken = format!("{SENT}\n").into_bytes();
    broken.extend([0xFF, 0xFE, b'x', b'\n']);
    assert_eq!(handed_off_to_in(&broken, MTIME), handed("new-2", SENT_AT));
    let not_object = format!("{SENT}\n[1,2]\n\"x\"\n42\nnull\n\n");
    assert_eq!(handed_off_to_in(not_object.as_bytes(), MTIME), handed("new-2", SENT_AT));
}

#[test]
fn 마지막_넘김_줄이_이긴다() {
    let second = r#"{"type":"continued-in","continuedInSessionId":"new-3","timestamp":"2026-10-17T00:00:00Z"}"#;
    assert_eq!(read(&[SENT, second]), handed("new-3", 1_792_195_200.0));
    // 그 사이에 이어 쓴 적이 있어도 마지막 줄 뒤가 비었으면 마지막이 이긴다
    assert_eq!(read(&[SENT, &user("다시"), second]), handed("new-3", 1_792_195_200.0));
    // 마지막 줄 뒤에 이어 썼으면 없다
    assert_eq!(read(&[SENT, second, &assistant("다시")]), None);
}

#[test]
fn 이어_받은_id_가_없으면_없다() {
    for line in [
        r#"{"type":"continued-in"}"#,
        r#"{"type":"continued-in","continuedInSessionId":""}"#,
        r#"{"type":"continued-in","continuedInSessionId":7}"#,
        r#"{"type":"continued-in","continuedInSessionId":null}"#,
    ] {
        // 앞의 멀쩡한 넘김 줄까지 거슬러 가지 않는다 — 가장 뒤의 넘김 줄에서 멈춘다
        assert_eq!(read(&[SENT, line]), None, "{line}");
    }
}

#[test]
fn 시각이_없거나_못_풀면_파일_수정_시각() {
    let none = r#"{"type":"continued-in","continuedInSessionId":"n"}"#;
    let bad = r#"{"type":"continued-in","continuedInSessionId":"n","timestamp":"어제"}"#;
    let no_zone = r#"{"type":"continued-in","continuedInSessionId":"n","timestamp":"2026-10-16T12:00:00"}"#;
    let number = r#"{"type":"continued-in","continuedInSessionId":"n","timestamp":123}"#;
    for line in [none, bad, no_zone, number] {
        assert_eq!(read(&[line]), handed("n", MTIME), "{line}");
    }
}

#[test]
fn type_이_글자가_아닌_줄은_건너뛴다() {
    let weird = r#"{"type":7,"note":"continued-in"}"#;
    let missing = r#"{"note":"x"}"#;
    assert_eq!(read(&[SENT, weird, missing]), handed("new-2", SENT_AT));
}

#[test]
fn crlf_와_bom_과_빈_줄() {
    let crlf = format!("{}\r\n{}\r\n\r\n", user("x"), SENT).into_bytes();
    assert_eq!(handed_off_to_in(&crlf, MTIME), handed("new-2", SENT_AT));
    // 줄 앞 BOM(파일 첫 줄이 창 맨 앞에 올 때)
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend(SENT.as_bytes());
    assert_eq!(handed_off_to_in(&bom, MTIME), handed("new-2", SENT_AT));
    // 넘김 줄이 \r 만 붙은 마지막 줄
    let tail_cr = format!("{SENT}\r").into_bytes();
    assert_eq!(handed_off_to_in(&tail_cr, MTIME), handed("new-2", SENT_AT));
}

#[test]
fn 아주_긴_줄과_한글() {
    let huge = "가".repeat(300_000);
    // 넘김 줄 뒤의 아주 긴 사이드체인 줄은 풀고 건너뛴다
    let side = format!(r#"{{"type":"assistant","isSidechain":true,"message":{{"content":"{huge}"}}}}"#);
    assert_eq!(read(&[SENT, &side]), handed("new-2", SENT_AT));
    // 아주 긴 대화 줄이 뒤에 있으면 이어 쓴 것
    assert_eq!(read(&[SENT, &user(&huge)]), None);
    // 아주 긴 줄이 앞에 있어도 넘김 줄은 찾는다
    assert_eq!(read(&[&user(&huge), SENT]), handed("new-2", SENT_AT));
    // 한글 id
    let kr = r#"{"type":"continued-in","continuedInSessionId":"새-세션-💡","timestamp":"2026-10-16T12:00:00Z"}"#;
    assert_eq!(read(&[kr]), handed("새-세션-💡", SENT_AT));
}

#[test]
fn 빈_입력과_표시_없음() {
    assert_eq!(handed_off_to_in(b"", MTIME), None);
    assert_eq!(read(&[&user("일반 대화"), &assistant("응답")]), None);
}

#[test]
fn 파일로_읽는다() {
    let dir = std::env::temp_dir().join(format!("handoff-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.jsonl");
    std::fs::write(&path, jsonl(&[&user("x"), SENT])).unwrap();
    assert_eq!(handed_off_to(&path), handed("new-2", SENT_AT));
    // 시각 없는 줄은 파일 수정 시각을 쓴다
    std::fs::write(&path, jsonl(&[r#"{"type":"continued-in","continuedInSessionId":"n"}"#])).unwrap();
    let got = handed_off_to(&path).unwrap();
    assert_eq!(got.id, "n");
    assert!(got.at > 1_700_000_000.0, "{}", got.at);
    // 없는 파일
    assert_eq!(handed_off_to(&dir.join("nope.jsonl")), None);
    // 256_000 바이트 창 — 넘김 줄이 창 밖이면 못 본다
    let mut big = jsonl(&[SENT]);
    big.extend(std::iter::repeat_n(b'x', 300_000));
    big.push(b'\n');
    std::fs::write(&path, &big).unwrap();
    assert_eq!(handed_off_to(&path), None);
    std::fs::remove_dir_all(&dir).unwrap();
}
