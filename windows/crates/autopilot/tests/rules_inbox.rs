//! 방향을 바꿀 말 규칙(`rules::inbox`)과 멈추고 다시 출발 규칙(`rules::control` 끝)의 시험. 원본: `Tests/AutopilotRulesTests/InboxTests.swift`.
//! Swift 시험 파일의 시험 19개를 모두 같은 이름으로 옮겼다: 「글 꼴」 `읽기`·`되읽기`·`이름` · 「어느 단위에」 `단위`·`까닭` ·
//! 「넘기기와 닫기」 `고르기`·`판정뒤`·`턴뒤`·`닫기`·`글`·`줄` · 「받기와 보내기」 `받기`·`보내기`·`기록` · `RestartTests`(모듈 `restart_tests`) `계획`·`글`·`멈춤`·`가르기`·`다시열기`.
//! `글` 은 두 스위트에 있어 `RestartTests` 쪽은 모듈로 가른다.
//! 표(`parseRows`·`bindRows`)와 인자 목록은 Swift 에서 기계로 옮겼다(글자 값을 `String(repeating:)` 으로 만든 줄은 `"가".repeat(n)`).
//! 가짜 어댑터(`FakeInboxFiles`·`FixedClock`·`FakeInboxOut`)는 Swift 시험처럼 이 파일에만 둔다.
//! 끝의 `추가_` 시험은 Swift 시험에 없는 것(경계·직렬화·Foundation 과의 차이를 지키는 것)이다.

use claude_board_autopilot::ports::Clock;
use claude_board_autopilot::rules::control::{
    is_restart_stop, reopen_intake, restart_plan, restart_stops_before, restart_text, RestartPlan, RESTART_REASON,
};
use claude_board_autopilot::rules::inbox::{
    after_check, after_turn, bind, close_run, close_unit, compose, label, name_ok, next_for, next_text, now_for, now_text, parse, receive,
    received_text, send, AfterCheck, AfterTurn, InboxEntry, InboxFiles, InboxNote, InboxOut, InboxParse, InboxRead, InboxSend, InboxTo,
    INBOX_MAX_BYTES, INBOX_MAX_CHARS, INBOX_PASSED_TEXT, INBOX_STATUSES,
};

// ── 글 꼴 ────────────────────────────────────────────

struct ParseRow {
    note: String,
    raw: String,
    /// "to unit at | 본문" 또는 "bad: 까닭"
    want: String,
}

// 원본 `show` — Swift 는 `at` 을 `"\($0)"`(Double 설명)으로 쓰고 여기는 `{}` 로 쓴다. 표의 값(1761800000.5)에서는 같다
fn show(p: &InboxParse) -> String {
    match p {
        InboxParse::Note(n) => format!(
            "{} {} {} | {}",
            n.to.as_str(),
            n.unit.map(|u| u.to_string()).unwrap_or_else(|| "-".to_string()),
            n.at.map(|a| format!("{a}")).unwrap_or_else(|| "-".to_string()),
            n.text
        ),
        InboxParse::Bad(w) => format!("bad: {w}"),
    }
}

fn parse_rows() -> Vec<ParseRow> {
    vec![
        ParseRow { note: "보드가 쓰는 꼴".to_string(), raw: "to: now\nunit: 3\nat: 1761800000.500\n\n정렬은 화면 순서와 같게\n".to_string(), want: "now 3 1761800000.5 | 정렬은 화면 순서와 같게".to_string() },
        ParseRow { note: "다음 단위부터, unit 없음, 여러 줄 본문".to_string(), raw: "to: next\n\n첫 줄\n둘째 줄\n".to_string(), want: "next - - | 첫 줄\n둘째 줄".to_string() },
        ParseRow { note: "머리 이름은 대소문자·앞뒤 빈칸을 가리지 않는다, 모르는 칸은 넘김".to_string(), raw: " To : now \nfrom: board\n\n말\n".to_string(), want: "now - - | 말".to_string() },
        ParseRow { note: "CRLF 도 읽는다".to_string(), raw: "to: now\r\n\r\n말\r\n".to_string(), want: "now - - | 말".to_string() },
        ParseRow { note: "to 없음".to_string(), raw: "unit: 3\n\n말\n".to_string(), want: "bad: 머리에 to: now 또는 to: next 가 없습니다".to_string() },
        ParseRow { note: "to 가 다른 값".to_string(), raw: "to: later\n\n말\n".to_string(), want: "bad: 머리 to 는 now 또는 next 입니다(later)".to_string() },
        ParseRow { note: "unit 이 0".to_string(), raw: "to: now\nunit: 0\n\n말\n".to_string(), want: "bad: 머리 unit 은 1 이상의 정수입니다(0)".to_string() },
        ParseRow { note: "at 이 수가 아님".to_string(), raw: "to: now\nat: 어제\n\n말\n".to_string(), want: "bad: 머리 at 은 수입니다(어제)".to_string() },
        ParseRow { note: "빈 줄이 없음".to_string(), raw: "to: now".to_string(), want: "bad: 머리와 본문 사이 빈 줄이 없습니다".to_string() },
        ParseRow { note: "머리 줄 꼴이 아님".to_string(), raw: "그냥 글\n\n말\n".to_string(), want: "bad: 머리 줄 꼴이 아닙니다: 그냥 글".to_string() },
        ParseRow { note: "본문이 빈칸뿐".to_string(), raw: "to: next\n\n   \n\n".to_string(), want: "bad: 본문이 비었습니다".to_string() },
        ParseRow { note: "본문이 상한보다 김".to_string(), raw: ["to: now\n\n", "가".repeat(2001).as_str()].concat(), want: "bad: 본문이 2000자보다 깁니다(2001자)".to_string() },
        ParseRow { note: "본문이 상한과 같으면 받는다".to_string(), raw: ["to: now\n\n", "가".repeat(2000).as_str()].concat(), want: ["now - - | ", "가".repeat(2000).as_str()].concat() },
    ]
}
const PARSE_ROWS_LEN: usize = 13;

fn bind_rows() -> Vec<BindRow> {
    vec![
        BindRow { note: "지금 도는 단위".to_string(), to: InboxTo::Now, seen: None, statuses: "d r p".to_string(), want: "waiting unit:2".to_string() },
        BindRow { note: "보낼 때 본 단위가 아직 돈다".to_string(), to: InboxTo::Now, seen: Some(2), statuses: "d r p".to_string(), want: "waiting unit:2".to_string() },
        BindRow { note: "보낼 때 본 단위가 끝났으면 다른 단위에 넘기지 않는다".to_string(), to: InboxTo::Now, seen: Some(1), statuses: "d r p".to_string(), want: "late".to_string() },
        BindRow { note: "보낼 때 본 단위가 아직 시작 전이면 그 단위".to_string(), to: InboxTo::Now, seen: Some(3), statuses: "d d p".to_string(), want: "waiting unit:3".to_string() },
        BindRow { note: "없는 단위".to_string(), to: InboxTo::Now, seen: Some(9), statuses: "d r".to_string(), want: "late".to_string() },
        BindRow { note: "도는 단위가 없으면(단위 사이·멈출 선 대기) 다음에 시작할 단위".to_string(), to: InboxTo::Now, seen: None, statuses: "d f p p".to_string(), want: "waiting unit:3".to_string() },
        BindRow { note: "남은 단위가 없음".to_string(), to: InboxTo::Now, seen: None, statuses: "d d".to_string(), want: "late".to_string() },
        BindRow { note: "다음 단위부터: 도는 단위 뒤의 첫 대기".to_string(), to: InboxTo::Next, seen: None, statuses: "d r p p".to_string(), want: "waiting from:3".to_string() },
        BindRow { note: "다음 단위부터: 뒤 단위가 건너뜀이면 그 뒤 대기".to_string(), to: InboxTo::Next, seen: Some(2), statuses: "d r s p".to_string(), want: "waiting from:4".to_string() },
        BindRow { note: "다음 단위부터: 도는 단위가 없으면 첫 대기".to_string(), to: InboxTo::Next, seen: None, statuses: "d d p".to_string(), want: "waiting from:3".to_string() },
        BindRow { note: "다음 단위부터: 뒤에 남은 단위가 없음".to_string(), to: InboxTo::Next, seen: None, statuses: "d r".to_string(), want: "late".to_string() },
    ]
}
const BIND_ROWS_LEN: usize = 11;

#[test]
fn 읽기() {
    let rows = parse_rows();
    assert_eq!(rows.len(), PARSE_ROWS_LEN);
    for r in &rows {
        assert_eq!(show(&parse(&r.raw)), r.want, "{}", r.note);
    }
}

#[test]
fn 되읽기() {
    let mut a = InboxNote::new(InboxTo::Now, "  한 줄  \n");
    a.unit = Some(2);
    a.at = Some(1761800000.25);
    let b = InboxNote::new(InboxTo::Next, "여러\r\n줄");
    for n in [a, b] {
        let InboxParse::Note(back) = parse(&compose(&n)) else {
            panic!("읽지 못함");
        };
        assert!(back.to == n.to && back.unit == n.unit && back.at == n.at);
        assert_eq!(back.text, n.text.replace("\r\n", "\n").trim_matches(char::is_whitespace));
    }
}

#[test]
fn 이름() {
    let cases = [
        ("20261030-140512-123.md", true),
        ("20261030-140512-123-2.md", true),
        ("a.md", true),
        (".tmp-20261030-140512-123.md", false),
        ("note.txt", false),
        ("../x.md", false),
        ("한글.md", false),
        (".md", false),
    ];
    for (name, want) in cases {
        assert_eq!(name_ok(name), want, "{name}");
    }
}

// ── 어느 단위에 붙일지 ───────────────────────────────

struct BindRow {
    note: String,
    to: InboxTo,
    seen: Option<i64>,
    /// 단위 상태를 빈칸으로(d done, r running, p pending, f failed, s skipped)
    statuses: String,
    /// "waiting unit:n" · "waiting from:n" · "late"
    want: String,
}

fn sts(s: &str) -> Vec<String> {
    s.split(' ')
        .filter(|x| !x.is_empty())
        .map(|x| match x {
            "d" => "done",
            "r" => "running",
            "p" => "pending",
            "f" => "failed",
            "s" => "skipped",
            "c" => "capped",
            other => other,
        })
        .map(|x| x.to_string())
        .collect()
}

#[test]
fn 단위() {
    let rows = bind_rows();
    assert_eq!(rows.len(), BIND_ROWS_LEN);
    for r in &rows {
        let mut note = InboxNote::new(r.to, "말");
        note.unit = r.seen;
        let b = bind(&note, &sts(&r.statuses));
        let got = if b.status == "late" {
            "late".to_string()
        } else if let Some(u) = b.unit {
            format!("waiting unit:{u}")
        } else if let Some(f) = b.from {
            format!("waiting from:{f}")
        } else {
            "?".to_string()
        };
        assert_eq!(got, r.want, "{}", r.note);
        if b.status == "late" {
            assert!(!b.why.clone().unwrap_or_default().is_empty(), "{}", r.note);
        }
    }
}

#[test]
fn 까닭() {
    let mut note = InboxNote::new(InboxTo::Now, "말");
    note.unit = Some(1);
    let b = bind(&note, &["capped", "running"]);
    assert_eq!(
        b.why.as_deref(),
        Some("보낼 때 본 단위(단위 1)가 이미 끝나(상한에서 멈춤) 넘기지 않았습니다. 다음 단위부터로 다시 보내거나 멈추고 다시 출발합니다")
    );
}

// ── 언제 넘길지 ──────────────────────────────────────

fn e(to: &str, status: &str, unit: Option<i64>, from: Option<i64>, units: &[i64], file: &str) -> InboxEntry {
    let mut x = InboxEntry::new(file, to, &format!("말 {file}"), 0.0, status);
    x.unit = unit;
    x.from = from;
    x.units = units.to_vec();
    x
}

#[test]
fn 고르기() {
    let es = vec![
        e("now", "waiting", Some(2), None, &[], "a"),
        e("now", "delivered", Some(2), None, &[2], "b"),
        e("now", "waiting", Some(3), None, &[], "c"),
        e("next", "waiting", None, Some(3), &[], "d"),
        e("next", "applied", None, Some(2), &[2], "e"),
        e("next", "late", None, None, &[], "f"),
        e("next", "unused", None, Some(2), &[], "g"),
    ];
    assert_eq!(now_for(&es, 2), vec![0]);
    assert_eq!(now_for(&es, 3), vec![2]);
    assert_eq!(next_for(&es, 2), Vec::<usize>::new()); // e 는 이미 단위 2 에 붙였다, d 는 3 부터
    assert_eq!(next_for(&es, 3), vec![3, 4]); // 붙인 방향은 뒤 단위에도
    assert_eq!(next_for(&es, 4), vec![3, 4]);
}

#[test]
fn 판정뒤() {
    let rows: [(bool, i64, i64, bool, AfterCheck); 7] = [
        (true, 0, 2, false, AfterCheck::Pass),
        (true, 0, 2, true, AfterCheck::Note),
        (true, 2, 2, true, AfterCheck::Note),
        (false, 0, 2, false, AfterCheck::Retry),
        (false, 2, 2, false, AfterCheck::Fail),
        (false, 2, 2, true, AfterCheck::Retry),
        (false, 3, 2, false, AfterCheck::Fail),
    ];
    for a in rows {
        assert_eq!(after_check(a.0, a.1, a.2, a.3), a.4, "{a:?}");
    }
}

#[test]
fn 턴뒤() {
    assert_eq!(after_turn(true), AfterTurn::Again);
    assert_eq!(after_turn(false), AfterTurn::Check);
}

#[test]
fn 닫기() {
    let mut es = vec![
        e("now", "waiting", Some(2), None, &[], "a"),
        e("now", "waiting", Some(3), None, &[], "b"),
        e("next", "waiting", None, Some(4), &[], "c"),
        e("next", "applied", None, Some(2), &[2], "d"),
        e("now", "delivered", Some(1), None, &[1], "x"),
    ];
    assert_eq!(close_unit(&mut es, 2, "capped"), 1);
    assert!(es[0].status == "dropped" && es[0].why.as_deref() == Some("넘기기 전에 그 단위(단위 2)가 끝나(상한에서 멈춤) 넘기지 못했습니다"));
    assert_eq!(es[1].status, "waiting");
    assert_eq!(close_run(&mut es), 2);
    let got: Vec<&str> = es.iter().map(|x| x.status.as_str()).collect();
    assert_eq!(got, ["dropped", "dropped", "unused", "applied", "delivered"]);
}

#[test]
fn 글() {
    let now = now_text(&["정렬은 화면 순서", "둘째\n줄"]);
    assert!(now.starts_with("사람이 실행을 지켜보다 이 단위에 한마디를 보냈습니다(보드의 \"지금 단위에\"):\n- 정렬은 화면 순서\n- 둘째\n  줄\n"));
    assert!(now.contains("판정 명령·쓰기 범위·가지 규칙은 출발 때 고정한 그대로"));
    assert!(now.contains("다른 가지 맞추기"));
    let next = next_text(&["한국어로"]);
    assert!(next.starts_with("사람이 실행 중에 보낸 방향(보드의 \"다음 단위부터\", 이 단위와 뒤 단위에 모두 붙습니다):\n- 한국어로\n"));
    assert!(next.contains("next_start"));
    assert!(!format!("{now}{next}{INBOX_PASSED_TEXT}").contains('—'));
}

#[test]
fn 줄() {
    let rows: Vec<(InboxEntry, &str, &str)> = vec![
        (e("now", "waiting", Some(2), None, &[], "f.md"), "기다림", "단위 2 의 다음 차례에 넘깁니다"),
        (e("next", "waiting", None, Some(3), &[], "f.md"), "기다림", "단위 3 부터 첫 입력에 붙입니다"),
        (e("now", "delivered", Some(2), None, &[2], "f.md"), "넘김", "단위 2 의 세션에 넘겼습니다"),
        (e("next", "applied", None, Some(2), &[2, 3], "f.md"), "붙임", "단위 2·3 의 첫 입력에 붙였습니다(뒤 단위에도 붙습니다)"),
    ];
    for a in rows {
        let l = label(&a.0);
        assert!(l.0 == a.1 && l.1 == a.2, "{l:?}");
    }
}

// ── 포트와 유스케이스(가짜 어댑터) ───────────────────

struct FakeInboxFiles {
    files: Vec<(String, InboxRead)>,
}

impl FakeInboxFiles {
    fn new(files: Vec<(&str, InboxRead)>) -> FakeInboxFiles {
        FakeInboxFiles { files: files.into_iter().map(|(n, r)| (n.to_string(), r)).collect() }
    }
}

impl InboxFiles for FakeInboxFiles {
    fn names(&self) -> Vec<String> {
        self.files.iter().map(|(n, _)| n.clone()).collect()
    }
    fn read(&self, name: &str) -> InboxRead {
        self.files.iter().find(|(n, _)| n == name).map(|(_, r)| r.clone()).unwrap_or(InboxRead::NotFile)
    }
}

struct FixedClock {
    now: f64,
}

impl Clock for FixedClock {
    fn now(&self) -> f64 {
        self.now
    }
    fn sleep(&self, _d: std::time::Duration) {}
}

struct FakeInboxOut {
    written: std::sync::Mutex<std::collections::HashMap<String, String>>,
    fail: std::sync::atomic::AtomicBool,
}

impl FakeInboxOut {
    fn new(existing: &[&str]) -> FakeInboxOut {
        FakeInboxOut {
            written: std::sync::Mutex::new(existing.iter().map(|n| (n.to_string(), String::new())).collect()),
            fail: std::sync::atomic::AtomicBool::new(false),
        }
    }
    fn get(&self, name: &str) -> Option<String> {
        self.written.lock().unwrap().get(name).cloned()
    }
}

impl InboxOut for FakeInboxOut {
    fn exists(&self, name: &str) -> bool {
        self.written.lock().unwrap().contains_key(name)
    }
    fn write(&self, name: &str, data: &[u8]) -> bool {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        self.written.lock().unwrap().insert(name.to_string(), String::from_utf8_lossy(data).into_owned());
        true
    }
}

#[test]
fn 받기() {
    let files = FakeInboxFiles::new(vec![
        ("20261030-000002-000.md", InboxRead::Text("to: next\n\n방향\n".to_string())),
        ("20261030-000001-000.md", InboxRead::Text("to: now\n\n지금\n".to_string())),
        ("20261030-000003-000.md", InboxRead::TooBig(99_999)),
        ("20261030-000004-000.md", InboxRead::NotFile),
        ("20261030-000005-000.md", InboxRead::Text("to: now\n".to_string())),
        (".tmp-20261030-000006-000.md", InboxRead::Text("to: now\n\n반쯤\n".to_string())),
        ("old.md", InboxRead::Text("to: now\n\n이미 받음\n".to_string())),
    ]);
    let mut old = InboxEntry::new("old.md", "now", "이미 받음", 1.0, "delivered");
    old.unit = Some(1);
    old.units = vec![1];
    let got = receive(&files, &[old], &["done", "running", "pending"], &FixedClock { now: 50.0 });
    let names: Vec<&str> = got.iter().map(|x| x.file.as_str()).collect();
    assert_eq!(names, ["20261030-000001-000.md", "20261030-000002-000.md", "20261030-000003-000.md", "20261030-000004-000.md", "20261030-000005-000.md"]);
    let sts: Vec<&str> = got.iter().map(|x| x.status.as_str()).collect();
    assert_eq!(sts, ["waiting", "waiting", "bad", "bad", "bad"]);
    assert!(got[0].unit == Some(2) && got[1].from == Some(3) && got.iter().all(|x| x.received == 50.0));
    assert_eq!(got[2].why.as_deref(), Some("파일이 16384바이트보다 커서(99999바이트) 읽지 않았습니다"));
    assert_eq!(got[4].why.as_deref(), Some("본문이 비었습니다")); // "to: now\n" 은 빈 줄 뒤 본문이 없는 것
}

#[test]
fn 보내기() {
    let out = FakeInboxOut::new(&["20261030-140512-123.md"]);
    let note = |to: InboxTo, text: &str| InboxNote::new(to, text);
    let mut with_head = note(InboxTo::Now, "  말  ");
    with_head.unit = Some(2);
    with_head.at = Some(5.0);
    let r = send(&with_head, "running", "20261030-140512-123", &out);
    assert_eq!(r, InboxSend::Sent("20261030-140512-123-2.md".to_string()));
    assert_eq!(out.get("20261030-140512-123-2.md").as_deref(), Some("to: now\nunit: 2\nat: 5.000\n\n말\n"));
    assert_eq!(send(&note(InboxTo::Now, "말"), "done", "x", &out), InboxSend::Refused("끝난 실행에는 보내지 않습니다".to_string()));
    assert_eq!(send(&note(InboxTo::Now, "말"), "", "x", &out), InboxSend::Refused("끝난 실행에는 보내지 않습니다".to_string()));
    assert_eq!(send(&note(InboxTo::Next, " \n "), "paused", "x", &out), InboxSend::Refused("보낼 말을 써 주세요".to_string()));
    assert_eq!(
        send(&note(InboxTo::Next, &"a".repeat(2001)), "paused", "x", &out),
        InboxSend::Refused("2000자까지 보낼 수 있습니다(지금 2001자)".to_string())
    );
    out.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(send(&note(InboxTo::Next, "말"), "running", "y", &out), InboxSend::Refused("inbox 에 쓰지 못했습니다".to_string()));
}

#[test]
fn 기록() {
    let mut w = e("now", "waiting", Some(2), None, &[], "f.md");
    w.text = "정렬\n순서".to_string();
    assert_eq!(received_text(&w, true), "사람이 보낸 말을 받았습니다(지금 단위에 · 단위 2): «정렬 순서». 이 세션의 다음 차례에 넘깁니다.");
    assert!(received_text(&w, false).ends_with("그 단위가 시작할 때 첫 입력에 붙입니다."));
    let mut long = e("next", "waiting", None, Some(3), &[], "f.md");
    long.text = "가".repeat(90);
    assert!(received_text(&long, false).contains(&format!("«{}…»", "가".repeat(80))));
}

// ── 멈추고 다시 출발(`rules::control`) ───────────────

mod restart_tests {
    use super::*;

    #[test]
    fn 계획() {
        let rows: [(&str, bool, RestartPlan); 5] = [
            ("d r p", false, RestartPlan::AfterUnit(2)),
            ("d d p", false, RestartPlan::Now),
            ("d d", false, RestartPlan::Now),
            ("r p", true, RestartPlan::Already),
            ("", false, RestartPlan::Now),
        ];
        for a in rows {
            assert_eq!(restart_plan(&sts(a.0), a.1), a.2, "{a:?}");
        }
    }

    #[test]
    fn 글() {
        assert_eq!(
            restart_text(RestartPlan::AfterUnit(2), false, None),
            "멈추고 다시 출발 요청을 받았습니다. 지금 단위(단위 2)를 마친 뒤 멈추고, 새 단위는 시작하지 않습니다."
        );
        assert!(restart_text(RestartPlan::AfterUnit(2), true, None).contains("재개하면 그 단위(단위 2)를 마친 뒤 멈춥니다"));
        assert_eq!(
            restart_text(RestartPlan::Already, false, Some(3)),
            "멈추고 다시 출발 요청을 이미 받았습니다(그 단위(단위 3)를 마친 뒤 멈춤)."
        );
    }

    #[test]
    fn 멈춤() {
        let rows: [(bool, bool, bool); 3] = [(true, true, true), (true, false, false), (false, true, false)];
        for a in rows {
            assert_eq!(restart_stops_before(a.0, a.1), a.2, "{a:?}");
        }
    }

    #[test]
    fn 가르기() {
        assert!(is_restart_stop("stopped", RESTART_REASON));
        assert!(!is_restart_stop("stopped", "사람이 중단했습니다."));
        assert!(!is_restart_stop("done", RESTART_REASON));
    }

    struct ReopenRow {
        note: &'static str,
        state: &'static str,
        wrapped: bool,
        flag: bool,
        asked: bool,
        opened: bool,
        project: Option<&'static str>,
        want: bool,
    }

    // Swift 의 기본값(state stopped · wrapped·flag·asked true · opened false · project todo)을 채운 줄
    fn row(note: &'static str, want: bool) -> ReopenRow {
        ReopenRow { note, state: "stopped", wrapped: true, flag: true, asked: true, opened: false, project: Some("todo"), want }
    }

    #[test]
    fn 다시열기() {
        let rows = vec![
            row("보낸 보드, 끝 정리까지 마침", true),
            ReopenRow { state: "done", ..row("마지막 단위 중에 보내 끝남(done)이어도 연다", true) },
            ReopenRow { state: "running", ..row("아직 도는 중", false) },
            ReopenRow { wrapped: false, ..row("끝 정리 전(기록 저장소에 아직 없음)", false) },
            ReopenRow { flag: false, ..row("엔진이 받지 않음(표지 없음)", false) },
            ReopenRow { asked: false, ..row("다른 보드가 보냈거나 보드를 껐다 켬", false) },
            ReopenRow { opened: true, ..row("이미 열었음", false) },
            ReopenRow { project: None, ..row("프로젝트 없는 실행은 이어 갈 수 없음", false) },
        ];
        for r in rows {
            assert_eq!(reopen_intake(r.state, r.wrapped, r.flag, r.asked, r.opened, r.project), r.want, "{}", r.note);
        }
    }
}

// ── 추가: Swift 시험에 없는 것 ───────────────────────

#[test]
fn 추가_상수와_상태_이름() {
    assert_eq!(INBOX_MAX_CHARS, 2000);
    assert_eq!(INBOX_MAX_BYTES, 16_384);
    assert_eq!(INBOX_STATUSES, ["waiting", "delivered", "applied", "late", "dropped", "unused", "bad"]);
}

#[test]
fn 추가_compose_글_꼴() {
    let mut n = InboxNote::new(InboxTo::Now, "  말  \r\n둘째\n");
    n.unit = Some(2);
    n.at = Some(5.0);
    assert_eq!(compose(&n), "to: now\nunit: 2\nat: 5.000\n\n말  \n둘째\n");
    assert_eq!(compose(&InboxNote::new(InboxTo::Next, "x")), "to: next\n\nx\n");
    n.at = Some(1.0005);
    assert!(compose(&n).contains("at: 1.000\n")); // 1.0005 는 이진수로 1.000499… 라 printf 와 같이 내림
}

#[test]
fn 추가_이름_길이_경계() {
    let stem81 = format!("a{}", "b".repeat(80));
    assert!(name_ok(&format!("{stem81}.md")));
    assert!(!name_ok(&format!("{stem81}b.md")));
    assert!(name_ok("a.b_c-d.md"));
    assert!(!name_ok("-a.md"));
    assert!(!name_ok("_a.md"));
    assert!(!name_ok("a b.md"));
    assert!(!name_ok("a.md\n"));
    assert!(!name_ok("a.MD"));
    assert!(!name_ok(""));
}

#[test]
fn 추가_읽기_경계() {
    let bad = |raw: &str| show(&parse(raw));
    assert_eq!(bad("to: now\nunit: +3\n\n말"), "now 3 - | 말");
    assert_eq!(bad("to: now\nunit: 1.5\n\n말"), "bad: 머리 unit 은 1 이상의 정수입니다(1.5)");
    assert_eq!(bad("to: now\nunit: -1\n\n말"), "bad: 머리 unit 은 1 이상의 정수입니다(-1)");
    assert_eq!(bad("to: NOW\n\n말"), "bad: 머리 to 는 now 또는 next 입니다(NOW)");
    assert_eq!(bad("to: now\nat: 1e999\n\n말"), "bad: 머리 at 은 수입니다(1e999)");
    assert_eq!(bad("to: now\nat: nan\n\n말"), "bad: 머리 at 은 수입니다(nan)");
    assert_eq!(bad("\n말"), "bad: 머리에 to: now 또는 to: next 가 없습니다");
    assert_eq!(bad("to: now\n \t\n 안 쪽  \n\n줄 "), "now - - | 안 쪽  \n\n줄");
    assert_eq!(bad("to: now\n\n"), "bad: 본문이 비었습니다");
    // 머리 줄 오류 글은 40 글자까지
    let long = "가".repeat(60);
    assert_eq!(bad(&format!("{long}\n\n말")), format!("bad: 머리 줄 꼴이 아닙니다: {}", "가".repeat(40)));
    // 같은 머리 칸이 둘이면 뒤가 이긴다
    assert_eq!(bad("to: now\nto: next\n\n말"), "next - - | 말");
}

#[test]
fn 추가_bind_범위_밖_단위() {
    let mut n = InboxNote::new(InboxTo::Now, "말");
    n.unit = Some(0);
    let b = bind(&n, &["running"]);
    assert_eq!(b.status, "late");
    assert_eq!(b.why.as_deref(), Some("보낼 때 본 단위(단위 0)가 이 실행에 없어 넘기지 않았습니다"));
    n.unit = Some(2);
    assert_eq!(bind(&n, &["running"]).why.as_deref(), Some("보낼 때 본 단위(단위 2)가 이 실행에 없어 넘기지 않았습니다"));
    // 끝난 꼴의 쉬운 말 다섯과 모르는 꼴
    for (st, word) in [("done", "통과"), ("failed", "미통과"), ("capped", "상한에서 멈춤"), ("stopped", "중단"), ("skipped", "건너뜀"), ("weird", "weird")] {
        n.unit = Some(1);
        let why = bind(&n, &[st, "running"]).why.unwrap();
        assert!(why.contains(&format!("가 이미 끝나({word}) 넘기지 않았습니다")), "{st}");
    }
    // next 는 도는 단위 앞의 대기를 건너뛴다(도는 단위 뒤의 첫 대기부터)
    let next = InboxNote::new(InboxTo::Next, "말");
    assert_eq!(bind(&next, &["pending", "running", "pending"]).from, Some(3));
    assert_eq!(bind(&next, &["pending", "running"]).status, "late");
    assert_eq!(bind(&next, &["pending", "pending"]).from, Some(1));
    // now 는 단위가 없으면 도는 단위, 없으면 첫 대기
    assert_eq!(bind(&InboxNote::new(InboxTo::Now, "말"), &["pending", "running", "pending"]).unit, Some(2));
    assert_eq!(bind(&InboxNote::new(InboxTo::Now, "말"), &["done", "pending", "pending"]).unit, Some(2));
    // 상태가 빈 실행
    let none: [&str; 0] = [];
    assert_eq!(bind(&InboxNote::new(InboxTo::Next, "말"), &none).why.as_deref(), Some("뒤에 남은 단위가 없어 붙일 곳이 없습니다"));
    assert_eq!(bind(&InboxNote::new(InboxTo::Now, "말"), &none).why.as_deref(), Some("남은 단위가 없어 넘기지 않았습니다"));
}

#[test]
fn 추가_next_for_에서_from_이_없으면_붙이지_않는다() {
    let es = vec![e("next", "waiting", None, None, &[], "a"), e("now", "waiting", Some(1), None, &[], "b")];
    assert_eq!(next_for(&es, 99), Vec::<usize>::new());
    assert_eq!(now_for(&es, 99), Vec::<usize>::new());
    assert_eq!(now_for(&es, 1), vec![1]);
}

#[test]
fn 추가_entry_직렬화는_빈_칸을_쓰지_않고_정수_시각은_정수로() {
    let mut x = InboxEntry::new("f.md", "now", "말", 50.0, "waiting");
    x.unit = Some(2);
    x.sent = Some(1761800000.5);
    assert_eq!(
        serde_json::to_string(&x).unwrap(),
        r#"{"file":"f.md","to":"now","text":"말","sent":1761800000.5,"received":50,"status":"waiting","unit":2,"units":[]}"#
    );
    // 옛 꼴(source·done 칸 없음)도 읽힌다, units 는 꼭 있어야 한다(Swift 합성 Codable 과 같다)
    let old: InboxEntry = serde_json::from_str(r#"{"file":"f.md","to":"next","text":"t","received":1.5,"status":"bad","units":[],"why":"w"}"#).unwrap();
    assert_eq!(old.source, None);
    assert_eq!(old.done, None);
    assert_eq!(old.why.as_deref(), Some("w"));
    assert_eq!(old.received, 1.5);
    assert!(serde_json::from_str::<InboxEntry>(r#"{"file":"f","to":"","text":"","received":1,"status":"bad"}"#).is_err());
    let back: InboxEntry = serde_json::from_str(&serde_json::to_string(&x).unwrap()).unwrap();
    assert_eq!(back, x);
}

#[test]
fn 추가_label_의_나머지_상태와_받은_기록의_나머지_갈래() {
    let mut x = e("now", "late", None, None, &[], "f.md");
    assert_eq!(label(&x), ("넘기지 않음".to_string(), "지금 단위에 넘길 곳이 없었습니다".to_string()));
    x.to = "next".to_string();
    assert_eq!(label(&x).1, "다음 단위부터 넘길 곳이 없었습니다");
    x.why = Some("까닭".to_string());
    assert_eq!(label(&x).1, "까닭");
    for (st, word) in [("dropped", "넘기지 못함"), ("unused", "쓰이지 않음"), ("bad", "읽지 못함")] {
        x.status = st.to_string();
        assert_eq!(label(&x), (word.to_string(), "까닭".to_string()));
        x.why = None;
        assert_eq!(label(&x), (word.to_string(), String::new()));
        x.why = Some("까닭".to_string());
    }
    x.status = "weird".to_string();
    assert_eq!(label(&x), ("weird".to_string(), "까닭".to_string()));
    assert_eq!(label(&e("now", "waiting", None, None, &[], "f.md")).1, "단위 ? 의 다음 차례에 넘깁니다");
    assert_eq!(label(&e("next", "waiting", None, None, &[], "f.md")).1, "단위 ? 부터 첫 입력에 붙입니다");
    assert_eq!(label(&e("now", "applied", None, None, &[4], "f.md")).1, "단위 4 의 첫 입력에 붙였습니다");

    let mut b = e("now", "bad", None, None, &[], "g.md");
    b.why = Some("왜".to_string());
    assert_eq!(received_text(&b, false), "inbox 의 g.md 을 읽지 못해 넘겼습니다: 왜");
    let mut l = e("next", "late", None, None, &[], "g.md");
    l.text = "말".to_string();
    l.why = Some("뒤에 남은 단위가 없어 붙일 곳이 없습니다".to_string());
    assert_eq!(received_text(&l, false), "사람이 보낸 말을 받았지만 넘기지 않습니다(다음 단위부터): «말». 뒤에 남은 단위가 없어 붙일 곳이 없습니다");
    let mut n = e("next", "waiting", None, Some(3), &[], "g.md");
    n.text = "가".repeat(80);
    assert_eq!(
        received_text(&n, true),
        format!("사람이 보낸 방향을 받았습니다(다음 단위부터 · 단위 3 부터): «{}». 대기열(범위)은 그대로이고 뒤 단위의 첫 입력에 붙입니다.", "가".repeat(80))
    );
}

#[test]
fn 추가_보내기_글자_수와_이름_꼴() {
    let out = FakeInboxOut::new(&[]);
    // 2000 글자의 한글은 6000 바이트지만 글자 수 상한 안이라 보낸다
    let r = send(&InboxNote::new(InboxTo::Now, &"가".repeat(2000)), "running", "20261030-140512-123", &out);
    assert_eq!(r, InboxSend::Sent("20261030-140512-123.md".to_string()));
    assert_eq!(send(&InboxNote::new(InboxTo::Now, "말"), "running", "bad name", &out), InboxSend::Refused("파일 이름 꼴이 아닙니다(bad name.md)".to_string()));
    let r = send(&InboxNote::new(InboxTo::Now, "말"), "waiting_stopline", "s", &out);
    assert_eq!(r, InboxSend::Sent("s.md".to_string()));
    assert_eq!(send(&InboxNote::new(InboxTo::Now, "말"), "running", "s", &out), InboxSend::Sent("s-2.md".to_string()));
}

#[test]
fn 추가_닫기_와_글_모양의_경계() {
    let mut es = vec![e("now", "waiting", Some(2), None, &[], "a")];
    assert_eq!(close_unit(&mut es, 9, "done"), 0);
    assert_eq!(es[0].status, "waiting");
    assert_eq!(close_unit(&mut es, 2, "알 수 없음"), 1);
    assert_eq!(es[0].why.as_deref(), Some("넘기기 전에 그 단위(단위 2)가 끝나(알 수 없음) 넘기지 못했습니다"));
    assert_eq!(close_run(&mut es), 0);
    assert_eq!(close_run(&mut []), 0);
    assert_eq!(now_text::<&str>(&[]).split('\n').nth(1), Some(""));
    assert_eq!(next_text(&["가\n나\n다"]).split('\n').skip(1).take(3).collect::<Vec<_>>(), ["- 가", "  나", "  다"]);
}

#[test]
fn 추가_같은_이름이_두_번_오면_두_줄이_나온다() {
    let ok = InboxRead::Text("to: now\n\n말\n".to_string());
    let files = FakeInboxFiles::new(vec![("a.md", ok.clone()), ("a.md", ok)]);
    let got = receive(&files, &[], &["running"], &FixedClock { now: 1.0 });
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|x| x.file == "a.md" && x.unit == Some(1)));
}

#[test]
fn 추가_모델에_넘기는_글_전문() {
    let scope = "이 단위의 일·판정 명령·쓰기 범위·가지 규칙은 출발 때 고정한 그대로입니다. 그 밖의 일(다른 단위의 일, 다른 가지 맞추기, 판정 바꾸기 등)을 하라는 말이면 하지 않고 result.json 의 next_start 에 적습니다.";
    assert_eq!(
        now_text(&["가"]),
        format!("사람이 실행을 지켜보다 이 단위에 한마디를 보냈습니다(보드의 \"지금 단위에\"):\n- 가\n이 말을 반영해 하던 일을 이어 가세요. {scope} 마치면 result.json 을 고쳐 쓰고 턴을 끝냅니다.")
    );
    assert_eq!(
        next_text(&["가", "나"]),
        format!("사람이 실행 중에 보낸 방향(보드의 \"다음 단위부터\", 이 단위와 뒤 단위에 모두 붙습니다):\n- 가\n- 나\n이 방향은 일하는 방식만 바꿉니다. {scope}")
    );
    assert_eq!(
        INBOX_PASSED_TEXT,
        "판정 명령은 통과했지만 그 사이 사람이 이 단위에 한마디를 보냈습니다. 아래 말을 반영한 뒤 판정 명령을 다시 돌려 보고 result.json 을 고쳐 쓰세요."
    );
}

#[test]
fn 추가_받은_줄은_머리의_시각과_단위를_옮겨_적는다() {
    let files = FakeInboxFiles::new(vec![("a.md", InboxRead::Text("to: now\nunit: 2\nat: 7.5\n\n말\n".to_string()))]);
    let got = receive(&files, &[], &["done", "running"], &FixedClock { now: 9.0 });
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].sent, got[0].seen, got[0].unit, got[0].from, got[0].received), (Some(7.5), Some(2), Some(2), None, 9.0));
    assert_eq!((got[0].to.as_str(), got[0].text.as_str(), got[0].status.as_str()), ("now", "말", "waiting"));
    let late = receive(&FakeInboxFiles::new(vec![("b.md", InboxRead::Unreadable)]), &[], &["done"], &FixedClock { now: 1.0 });
    assert_eq!(late[0].why.as_deref(), Some("글(UTF-8)로 읽지 못했습니다"));
    assert_eq!((late[0].to.as_str(), late[0].text.as_str(), late[0].status.as_str()), ("", "", "bad"));
}

#[test]
fn 추가_닫기의_까닭_글과_80자_끊기_경계() {
    let mut es = vec![e("now", "waiting", Some(2), None, &[], "a"), e("next", "waiting", None, Some(4), &[], "c")];
    assert_eq!(close_run(&mut es), 2);
    assert_eq!(es[0].why.as_deref(), Some("넘기기 전에 실행이 끝났습니다"));
    assert_eq!(es[1].why.as_deref(), Some("붙일 단위가 시작하지 않고 실행이 끝났습니다"));
    let mut x = e("next", "waiting", None, Some(3), &[], "g.md");
    x.text = "가".repeat(81);
    assert!(received_text(&x, false).contains(&format!("«{}…»", "가".repeat(80))));
    x.text = "가".repeat(80);
    assert!(received_text(&x, false).contains(&format!("«{}»", "가".repeat(80))));
}
