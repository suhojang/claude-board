//! 실행 보고서 틀(`rules::report`, 원본 `Report.swift`) 시험. 3/4 구간의 시험은 끝의 `date_`·`writer_`·`chip_`·`css_` 시험이다.
//! 원본 시험: `ReportTests.swift` 의 `ResultParseTests` 셋을 같은 이름으로 옮겼다 — `아님`(인자 다섯, nil 포함) · `꼴` · `상한`.
//! 이어서 `ReportRunTests` 셋 — `견본`(견본 기록 `Tests/AutopilotRulesTests/Report/sample-run.json` 을 그 자리에서 읽는다) · `칸` · `없음`.
//! 이 구간의 선언(`APResultRecord` 계열·`apCut`·`apReportText`·`apParseResult`·`APReport*` 값 꼴·`apReportRun`)을 겨냥한 Swift 시험은 이 여섯뿐이다.
//! 이름표 사전·`apReportDur`·`lastPart` 는 Swift 에서 골든(HTML)만 덮는다. `ReportHTMLTests` 셋(원본 337~539줄의 `apReportHTML`)도 끝의 `html_` 로 시작하는 일곱 시험 — `html_견본`(골든) · `html_단위`(골든) · `html_악성골든`(골든) · `html_악성`(악성 골든의 거부 내용) · `html_견본안전`(안전 검사) · `html_결정적`(같은 곳 두 번) · `html_바탕`(기록 저장소 바탕, 빈 기록) — 으로 옮겼다. 그림 칸(`APReportDraw`) 시험도 `draw_` 로 시작하는 세 시험(쓰기·실패·없음)으로 옮겼다.
//!
//! 끝의 `extra_` 시험은 Swift 시험에 없는 것(자르기 경계·덩어리·수 글·칸별 상한·목록 거르기 순서·값 꼴 처음 값·기록 읽기 칸별 규칙·이름표 사전·걸린 시간)이다.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use claude_board_autopilot::ports::Row;
use claude_board_autopilot::rules::formats::grapheme_len;
use claude_board_autopilot::rules::reading::ReadClock;
use claude_board_autopilot::rules::report::{
    chip, chip_plain, cut, draw_reports, kind_label, kind_text, last_part, merge_label, outcome_label, parse_result, report_dur, report_html, report_run, report_text, report_text_n, run_label,
    test_label, unit_class, unit_label, ReportCheck, ReportFile, ReportOrigin, ReportRun, ReportSink, ReportSource, ReportUnit, ResultCarry, ResultChange, ResultFlow, ResultRecord, ResultState,
    ResultTest, Writer, FixedZone, CSS, REPORT_FLOW_MAX, REPORT_LIST_MAX, REPORT_TAIL_MAX, REPORT_TEXT_MAX,
};
use serde_json::{json, Value};

// ── Swift: ResultParseTests ──────────────────────────

#[test]
fn 아님() {
    // @Test("JSON 객체가 아니면 nil", arguments: [nil, "", "[1,2]", "글", "{\"headline\":"])
    let args: [Option<&str>; 5] = [None, Some(""), Some("[1,2]"), Some("글"), Some("{\"headline\":")];
    for t in args {
        assert_eq!(parse_result(t), None, "{t:?}");
    }
}

#[test]
fn 꼴() {
    let r = parse_result(Some(
        r#"{"unit": "글", "outcome": 3, "headline": {"x": 1}, "summary": ["가", 2, null, {"y": 1}, true, ""],
 "tests": [{"name": "시험", "result": "pass"}, "글"], "changes": {"what": "x"}, "refs": {"z": "끝", "a": "https://example.com", "m": ["x"], "b": 7},
 "sequence": [{"title": "그림"}]}"#,
    ))
    .unwrap();
    assert!(r.title.is_empty() && r.outcome == "3" && r.headline.is_empty());
    assert_eq!(r.summary, ["가", "2"]);
    assert_eq!(r.tests, [ResultTest { name: "시험".into(), result: "pass".into(), note: "".into() }]);
    assert!(r.changes.is_empty());
    let keys: Vec<&str> = r.refs.iter().map(|x| x.key.as_str()).collect();
    assert!(keys == ["a", "b", "z"] && r.refs[1].value == "7");
    assert_eq!(r.flows, [ResultFlow { title: "그림".into(), source: "".into() }]);
}

#[test]
fn 상한() {
    let long = "가".repeat(REPORT_TEXT_MAX + 10);
    let many = (0..(REPORT_LIST_MAX + 5)).map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(",");
    let r = parse_result(Some(&format!("{{\"headline\": \"{long}\", \"summary\": [{many}]}}"))).unwrap();
    assert!(grapheme_len(&r.headline) == REPORT_TEXT_MAX + 1 && r.headline.ends_with('…'));
    assert_eq!(r.summary.len(), REPORT_LIST_MAX);
}

// ── Swift: ReportRunTests ────────────────────────────

// 가짜 읽을 곳 — 견본 기록의 사전을 그대로. 결과 기록은 글(객체면 JSON 글로, 글이면 그대로). 원본: `FixtureSource`
struct FixtureSource {
    j: Value,
    origin: ReportOrigin,
    reads: RefCell<Vec<String>>,
}

impl FixtureSource {
    fn new(j: Value) -> Self {
        FixtureSource { j, origin: ReportOrigin::Folder, reads: RefCell::new(Vec::new()) }
    }
    // Swift 의 `FixtureSource(j, origin:)` initializer — `html_바탕` 이 기록 저장소 바탕을 시험할 때 쓴다
    fn with_origin(j: Value, origin: ReportOrigin) -> Self {
        FixtureSource { j, origin, reads: RefCell::new(Vec::new()) }
    }
    fn read(&self, s: String) {
        self.reads.borrow_mut().push(s);
    }
}

impl ReportSource for FixtureSource {
    fn origin(&self) -> ReportOrigin {
        self.origin
    }
    fn run(&self) -> Option<Row> {
        self.read("run".into());
        self.j.get("run").and_then(Value::as_object).cloned()
    }
    fn queue(&self) -> Vec<Row> {
        self.read("queue".into());
        // Swift: `j["queue"] as? [[String: Any]] ?? []`
        let Some(Value::Array(a)) = self.j.get("queue") else { return Vec::new() };
        a.iter().map(|v| v.as_object().cloned()).collect::<Option<Vec<_>>>().unwrap_or_default()
    }
    fn engine(&self, n: i64) -> Option<Row> {
        self.read(format!("engine {n}"));
        self.j.get("engines")?.get(n.to_string())?.as_object().cloned()
    }
    fn result(&self, n: i64) -> Option<String> {
        self.read(format!("result {n}"));
        match self.j.get("results")?.get(n.to_string())? {
            Value::String(s) => Some(s.clone()),
            o => Some(o.to_string()),
        }
    }
}

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Tests/AutopilotRulesTests/Report").join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn 견본() {
    // @Test("견본 기록: 단위 상태는 queue 가 먼저, 판정·거부·결과 기록 상태")
    let r = report_run("x", &FixtureSource::new(fixture("sample-run.json"))).unwrap();
    assert_eq!(r.units.iter().map(|u| u.status.as_str()).collect::<Vec<_>>(), ["done", "failed", "capped", "skipped"]);
    assert_eq!(r.units.iter().map(|u| u.result_state).collect::<Vec<_>>(), [ResultState::Ok, ResultState::Ok, ResultState::Missing, ResultState::Missing]);
    assert!(r.units[1].checks.iter().map(|c| c.n).collect::<Vec<_>>() == [1, 2, 3]);
    assert!(r.units[1].checks.iter().map(|c| c.exit).collect::<Vec<_>>() == [Some(1), Some(1), Some(1)]);
    assert_eq!(r.units[1].last_exit, Some(1));
    assert_eq!(r.units[2].denials, ["Bash: git push origin live"]);
    assert!(r.mode == "worktree" && r.repos == ["todo-api"] && r.bases == ["todo-api: live"] && r.work == "autopilot/20261030-0900-todo-api");
    assert!(r.harness_same == Some(true) && r.harness_files == 3 && r.moved.is_empty() && r.stashed == 0);
}

#[test]
fn 칸() {
    // @Test("작업 키는 스토리가 있을 때만, 종류가 없으면 결과 기록의 것, 깨진 결과 기록")
    let src = FixtureSource::new(json!({
        "run": {"title": "t", "state": "running"},
        "queue": [{"title": "a", "status": "", "tickets": ["SHOP-2"]}, {"title": "b", "story": "SHOP-1", "tickets": ["SHOP-3"]}],
        "engines": {"1": {"status": "running", "checks": [{"exit": 0, "tail": "x".repeat(REPORT_TAIL_MAX + 50)}]}},
        "results": {"1": "{\"unit\": {\"kind\": \"fix\"}}", "2": "이건 JSON 이 아님"},
    }));
    let r = report_run("x", &src).unwrap();
    assert!(r.units[0].status == "running" && r.units[0].task.is_empty() && r.units[0].kind == "fix");
    assert!(r.units[0].checks[0].n == 1 && grapheme_len(&r.units[0].checks[0].tail) == REPORT_TAIL_MAX + 1 && r.units[0].checks[0].tail.starts_with('…'));
    assert!(r.units[1].status == "pending" && r.units[1].task == "SHOP-3" && r.units[1].story == "SHOP-1");
    assert!(r.units[1].result_state == ResultState::Broken && r.units[1].result.is_none());
    assert_eq!(*src.reads.borrow(), ["run", "queue", "engine 1", "result 1", "engine 2", "result 2"]);
}

#[test]
fn 없음() {
    // @Test("run 이 없으면 nil")
    assert_eq!(report_run("x", &FixtureSource::new(json!({}))), None);
}

// ── Swift 시험에 없는 것 ──────────────────────────────

#[test]
fn extra_수_글_지수_꼴() {
    // 지수 17 이상·-5 아래는 `e+XX`·`e-XX`(이 구간에서 정한 꼴, Foundation 경계는 확인하지 못함)
    let f = |s: &str| report_text(Some(&serde_json::from_str::<Value>(s).unwrap()));
    assert_eq!(f("1e20"), "1e+20");
    assert_eq!(f("1.5e-7"), "1.5e-07");
    assert_eq!(f("1e16"), "10000000000000000");
    assert_eq!(f("0.00001"), "0.00001");
    assert_eq!(f("-2.5"), "-2.5");
}

#[test]
fn extra_상수() {
    assert_eq!((REPORT_TEXT_MAX, REPORT_LIST_MAX, REPORT_FLOW_MAX, REPORT_TAIL_MAX), (4000, 200, 20_000, 2000));
}

#[test]
fn extra_cut_경계() {
    assert_eq!(cut("", 0), "");
    assert_eq!(cut("abc", 3), "abc");
    assert_eq!(cut("abcd", 3), "abc…");
    assert_eq!(cut("abc", 0), "…");
    assert_eq!(cut("가나다", 3), "가나다"); // 바이트(9)는 3 보다 크지만 글자는 3
    assert_eq!(cut("가나다라", 3), "가나다…");
    assert_eq!(cut("가나다라", 0), "…");
}

#[test]
fn extra_cut_덩어리는_쪼개지_않는다() {
    // 결합 부호: e + U+0301 은 한 글자
    assert_eq!(cut("e\u{301}e\u{301}e\u{301}", 3), "e\u{301}e\u{301}e\u{301}");
    assert_eq!(cut("e\u{301}e\u{301}e\u{301}x", 3), "e\u{301}e\u{301}e\u{301}…");
    assert_eq!(cut("e\u{301}e\u{301}e\u{301}x", 2), "e\u{301}e\u{301}…");
    // 가족 이모지(ZWJ 열) 하나가 한 글자
    let fam = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    assert_eq!(cut(&format!("{fam}{fam}a"), 2), format!("{fam}{fam}…"));
    assert_eq!(cut(&format!("{fam}{fam}a"), 1), format!("{fam}…"));
    // 한글 자모 열은 한 글자
    assert_eq!(cut("\u{1112}\u{1161}\u{11AB}\u{1112}\u{1161}\u{11AB}x", 2), "\u{1112}\u{1161}\u{11AB}\u{1112}\u{1161}\u{11AB}…");
    // Windows 글: CR LF 는 한 글자
    assert_eq!(cut("a\r\nb\r\nc", 3), "a\r\nb…");
    assert_eq!(cut("a\r\nb", 3), "a\r\nb");
}

#[test]
fn extra_cut_긴_글도_빠르게_정확히() {
    let s = "나".repeat(REPORT_FLOW_MAX + 1);
    let c = cut(&s, REPORT_FLOW_MAX);
    assert_eq!(grapheme_len(&c), REPORT_FLOW_MAX + 1);
    assert!(c.ends_with('…') && c.starts_with('나'));
}

#[test]
fn extra_report_text_꼴() {
    assert_eq!(report_text(Some(&json!("글"))), "글");
    assert_eq!(report_text(Some(&json!(3))), "3");
    assert_eq!(report_text(Some(&json!(-3))), "-3");
    assert_eq!(report_text(Some(&json!(18_446_744_073_709_551_615u64))), "18446744073709551615");
    let f: Value = serde_json::from_str("7.0").unwrap();
    assert_eq!(report_text(Some(&f)), "7");
    assert_eq!(report_text(Some(&json!(1.5))), "1.5");
    assert_eq!(report_text(Some(&json!(0.1))), "0.1");
    assert_eq!(report_text(Some(&json!(true))), "");
    assert_eq!(report_text(Some(&json!(false))), "");
    assert_eq!(report_text(Some(&json!(null))), "");
    assert_eq!(report_text(Some(&json!(["x"]))), "");
    assert_eq!(report_text(Some(&json!({"x": 1}))), "");
    assert_eq!(report_text(None), "");
}

#[test]
fn extra_report_text_상한은_글에만_걸린다() {
    assert_eq!(report_text_n(Some(&json!("abcdef")), 3), "abc…");
    assert_eq!(report_text_n(Some(&json!("abc")), 3), "abc");
    assert_eq!(report_text_n(Some(&json!(123_456_789)), 3), "123456789");
    let long = "x".repeat(REPORT_TEXT_MAX + 1);
    assert_eq!(grapheme_len(&report_text(Some(&json!(long)))), REPORT_TEXT_MAX + 1);
    assert_eq!(report_text(Some(&json!("x".repeat(REPORT_TEXT_MAX)))).len(), REPORT_TEXT_MAX);
}

fn parse(v: Value) -> ResultRecord {
    parse_result(Some(&v.to_string())).unwrap()
}

#[test]
fn extra_칸별_상한과_이름() {
    let r = parse(json!({
        "unit": {"title": "t".repeat(301), "kind": "k".repeat(41), "ref": "r".repeat(301)},
        "outcome": "o".repeat(41),
        "tests": [{"name": "n".repeat(501), "result": "p".repeat(41), "note": "ok"}],
        "changes": [{"what": "w", "where": "p".repeat(501)}],
        "decisions_applied": [{"what": "d", "basis": "b"}],
        "sequence": [{"title": "s".repeat(301), "mermaid": "m".repeat(REPORT_FLOW_MAX + 1)}],
        "next_start": [{"title": "a", "why": "b"}],
        "new_work": [{"title": "c", "why": "d"}],
        "new_tickets": [{"temp": "T".repeat(21), "title": "u".repeat(301), "why": "y".repeat(1001)}],
        "next": "다음",
    }));
    // 상한 + 1 글자를 주면 앞 상한 글자 + … 가 나온다(글자 수만 재면 상한이 하나 어긋나도 모른다)
    let cap = |c: &str, n: usize| format!("{}…", c.repeat(n));
    assert_eq!(r.title, cap("t", 300));
    assert_eq!(r.kind, cap("k", 40));
    assert_eq!(r.r#ref, cap("r", 300));
    assert_eq!(r.outcome, cap("o", 40));
    assert_eq!(r.tests[0].name, cap("n", 500));
    assert_eq!(r.tests[0].result, cap("p", 40));
    assert_eq!(r.tests[0].note, "ok");
    assert_eq!(r.changes[0].what, "w");
    assert_eq!(r.changes[0].place, cap("p", 500));
    assert_eq!(r.decisions[0].what, "d");
    assert_eq!(r.decisions[0].basis, "b");
    assert_eq!(r.flows[0].title, cap("s", 300));
    assert_eq!(r.flows[0].source, cap("m", REPORT_FLOW_MAX));
    assert_eq!(r.next_start, [ResultCarry { title: "a".into(), why: "b".into() }]);
    assert_eq!(r.new_work, [ResultCarry { title: "c".into(), why: "d".into() }]);
    assert_eq!(r.new_tickets[0].temp, cap("T", 20));
    assert_eq!(r.new_tickets[0].title, cap("u", 300));
    assert_eq!(r.new_tickets[0].why, cap("y", 1000));
    assert_eq!(r.next, "다음");
    // 상한 안이면 그대로
    let ok = parse(json!({"unit": {"title": "t".repeat(300)}}));
    assert_eq!(ok.title, "t".repeat(300));
}

#[test]
fn extra_목록은_앞_200개를_먼저_자른_뒤_객체만_고른다() {
    // 앞 200 개 중 객체가 아닌 것은 버려져도 201 번째 이후 객체가 채우지 않는다
    let mut tests: Vec<Value> = (0..REPORT_LIST_MAX).map(|i| if i % 2 == 0 { json!("글") } else { json!({"name": format!("{i}")}) }).collect();
    tests.push(json!({"name": "밖"}));
    let r = parse(json!({"tests": tests}));
    assert_eq!(r.tests.len(), REPORT_LIST_MAX / 2);
    assert!(r.tests.iter().all(|t| t.name != "밖"));
    // 글 목록도 앞 200 개를 먼저 자르고 빈 것을 거른다
    let mut sum: Vec<Value> = (0..REPORT_LIST_MAX).map(|i| if i < 100 { json!("") } else { json!("x") }).collect();
    sum.push(json!("밖"));
    let r = parse(json!({"summary": sum, "risks": ["r1", null, "r2"]}));
    assert_eq!(r.summary.len(), 100);
    assert_eq!(r.risks, ["r1", "r2"]);
}

#[test]
fn extra_새_티켓은_스무_개까지() {
    let t: Vec<Value> = (0..30).map(|i| json!({"temp": format!("TMP-{i}")})).collect();
    let r = parse(json!({"new_tickets": t}));
    assert_eq!(r.new_tickets.len(), 20);
    assert_eq!(r.new_tickets[19].temp, "TMP-19");
}

#[test]
fn extra_참고는_키_차례_쉰_개_빈_값_거름_키_예순자() {
    let mut refs = serde_json::Map::new();
    for i in 0..60 {
        refs.insert(format!("k{i:02}"), json!(format!("v{i}")));
    }
    let r = parse(json!({"refs": refs}));
    assert_eq!(r.refs.len(), 50);
    assert_eq!(r.refs[0].key, "k00");
    assert_eq!(r.refs[49].key, "k49");
    // 쉰 개를 먼저 고른 뒤 빈 값을 거른다(빈 값 하나가 있으면 49 개)
    let mut refs = serde_json::Map::new();
    for i in 0..60 {
        refs.insert(format!("k{i:02}"), if i == 3 { json!("") } else { json!("v") });
    }
    assert_eq!(parse(json!({"refs": refs})).refs.len(), 49);
    // 키 글자 수 상한
    let r = parse(json!({"refs": {"a".repeat(61): "x", "b": "y".repeat(1001)}}));
    assert_eq!(r.refs.len(), 2);
    assert!(r.refs[0].key.ends_with('…') && grapheme_len(&r.refs[0].key) == 61);
    assert_eq!(grapheme_len(&r.refs[1].value), 1001);
    // refs 가 객체가 아니면 비어 있다
    assert!(parse(json!({"refs": ["a"]})).refs.is_empty());
    assert!(parse(json!({"refs": "a"})).refs.is_empty());
}

#[test]
fn extra_빈_객체와_공백() {
    assert_eq!(parse_result(Some("{}")), Some(ResultRecord::default()));
    assert_eq!(parse_result(Some(" \n{ } \n")), Some(ResultRecord::default()));
    assert_eq!(parse_result(Some("{} x")), None);
    assert_eq!(parse_result(Some("null")), None);
}

#[test]
fn extra_unit_이_객체면_세_칸을_읽는다() {
    let r = parse(json!({"unit": {"title": "제목", "kind": 5, "ref": null}}));
    assert_eq!((r.title.as_str(), r.kind.as_str(), r.r#ref.as_str()), ("제목", "5", ""));
}

#[test]
fn extra_값_꼴_처음_값() {
    let u = ReportUnit::default();
    assert_eq!((u.status.as_str(), u.result_state, u.n, u.started, u.last_exit), ("pending", ResultState::Missing, 0, None, None));
    assert!(u.checks.is_empty() && u.denials.is_empty() && u.result.is_none());
    assert_eq!((u.active_sec, u.cost, u.turns, u.retries), (0.0, 0.0, 0, 0));
    let c = ReportCheck::default();
    assert_eq!((c.n, c.exit, c.sec, c.tail.as_str(), c.timed_out, c.interrupted), (0, None, 0.0, "", false, false));
    let r = ReportRun::default();
    assert_eq!(r.origin, ReportOrigin::Folder);
    assert_eq!((r.harness_same, r.harness_files, r.stashed), (None, 0, 0));
    assert!(r.units.is_empty() && r.repos.is_empty() && r.bases.is_empty() && r.moved.is_empty() && r.harness_changed.is_empty());
    assert_eq!(ResultState::default(), ResultState::Missing);
    assert_eq!(ResultState::Ok.raw(), "ok");
    assert_eq!(ResultState::Missing.raw(), "missing");
    assert_eq!(ResultState::Broken.raw(), "broken");
    let ch = ResultChange::default();
    assert_eq!((ch.what.as_str(), ch.place.as_str()), ("", ""));
}

// ── Swift 시험에 없는 것: 기록 → 보고서 값(2/4) ────────

fn run_of(j: Value) -> ReportRun {
    report_run("id", &FixtureSource::new(j)).unwrap()
}

// run 은 비워 두고 단위 하나만 보고 싶을 때
fn unit_of(q: Value, e: Value) -> ReportUnit {
    run_of(json!({"run": {}, "queue": [q], "engines": {"1": e}})).units.remove(0)
}

fn glyphs(n: usize) -> String {
    "가".repeat(n)
}

#[test]
fn extra_실행_칸_상한과_꼴() {
    let r = run_of(json!({"run": {
        "title": glyphs(400), "project": glyphs(130), "model": glyphs(90), "state": glyphs(50), "reason": glyphs(1100), "epic": glyphs(90),
        "started": 10.5, "ended": 20,
    }}));
    assert_eq!(r.id, "id");
    for (got, max) in [(&r.title, 300), (&r.project, 120), (&r.model, 80), (&r.state, 40), (&r.reason, 1000), (&r.epic, 80)] {
        assert!(grapheme_len(got) == max + 1 && got.ends_with('…'), "{max}");
    }
    assert_eq!((r.started, r.ended), (Some(10.5), Some(20.0)));
    // 수가 아닌 시각(글·불리언·널)은 비고, 글 자리의 수는 글이 된다
    let r = run_of(json!({"run": {"started": "5", "ended": true, "title": 7, "state": null, "reason": ["x"]}}));
    assert!(r.started.is_none() && r.ended.is_none() && r.title == "7" && r.state.is_empty() && r.reason.is_empty());
    // 바탕은 읽을 곳이 알려 준다
    let mut src = FixtureSource::new(json!({"run": {}}));
    src.origin = ReportOrigin::Records;
    assert_eq!(report_run("z", &src).unwrap().origin, ReportOrigin::Records);
    assert_eq!(run_of(json!({"run": {}})).origin, ReportOrigin::Folder);
}

#[test]
fn extra_저장소와_가지() {
    // repos 는 키 차례로, 이름은 마지막 조각. 출발 가지는 글일 때만. 작업 가지는 worktree.branch 가 먼저고 없을 때 처음 만난 work
    let r = run_of(json!({"run": {
        "worktree": {"mode": "in_place", "repo": "/x/ignored", "repos": ["/y/ignored"]},
        "branches": {"repos": {
            "/b/zeta": {"base": {"branch": "main"}, "work": "w-zeta"},
            "/a/alpha": {"base": {"branch": 3}, "work": "w-alpha"},
            "/c/gamma": "글",
        }},
    }}));
    assert_eq!(r.mode, "in_place");
    assert_eq!(r.repos, ["alpha", "zeta", "gamma"]);
    assert_eq!(r.bases, ["zeta: main"]);
    assert_eq!(r.work, "w-alpha");
    let r = run_of(json!({"run": {"worktree": {"branch": "wb"}, "branches": {"repos": {"/a/alpha": {"work": "w-alpha"}}}}}));
    assert_eq!(r.work, "wb");
    // 빈 글 work 도 그대로 넣는다(Swift: `let w = e["work"] as? String`)
    let r = run_of(json!({"run": {"branches": {"repos": {"/a/x": {"work": ""}, "/b/y": {"work": "later"}}}}}));
    assert_eq!(r.work, "later");
    // repos 가 비면 worktree.repo, 그것도 없으면 worktree.repos(글만, 마지막 조각, 정렬)
    assert_eq!(run_of(json!({"run": {"worktree": {"repo": "/q/one/", "repos": ["/z/b"]}}})).repos, ["one"]);
    assert_eq!(run_of(json!({"run": {"worktree": {"repos": ["/z/b", 5, "/a/c", null]}}})).repos, ["b", "c"]);
    assert_eq!(run_of(json!({"run": {"worktree": {"repos": ["C:\\w\\zz", "D:\\w\\aa"]}}})).repos, ["aa", "zz"]);
    assert_eq!(run_of(json!({"run": {"worktree": {"repo": 5}}})).repos, Vec::<String>::new());
    assert!(run_of(json!({"run": {"worktree": "x", "branches": 3}})).repos.is_empty());
}

#[test]
fn extra_움직인_가지와_숨긴_변경() {
    let r = run_of(json!({"run": {"branches": {
        "moved": [{"repo": "/a/todo-api", "branch": "live"}, {"repo": 3}],
        "stashed": [1, "x", null],
    }}}));
    assert_eq!(r.moved, ["todo-api live", " "]);
    assert_eq!(r.stashed, 3);
    // 객체가 아닌 원소가 하나라도 있으면 통째로 빈 배열
    assert!(run_of(json!({"run": {"branches": {"moved": [{"repo": "/a/b", "branch": "c"}, "글"]}}})).moved.is_empty());
    assert_eq!(run_of(json!({"run": {"branches": {"stashed": "x"}}})).stashed, 0);
}

#[test]
fn extra_하네스_칸() {
    let r = run_of(json!({"run": {"harness": {"files": 7.9, "same": false, "changed": [
        {"path": "a.md", "change": "changed"}, {"path": "b", "change": "added"}, {"path": "c", "change": "removed"}, {"path": "d", "change": "weird"}, {"path": 5},
    ]}}}));
    assert_eq!(r.harness_files, 7);
    assert_eq!(r.harness_same, Some(false));
    assert_eq!(r.harness_changed, ["a.md (바뀜)", "b (더함)", "c (없어짐)", "d (weird)", " ()"]);
    // 앞 30 개만
    let many: Vec<Value> = (0..40).map(|i| json!({"path": format!("p{i}"), "change": "added"})).collect();
    let r = run_of(json!({"run": {"harness": {"changed": many}}}));
    assert_eq!(r.harness_changed.len(), 30);
    assert_eq!(r.harness_changed[29], "p29 (더함)");
    // 객체 아닌 원소가 하나라도 있으면 빈 배열. same 이 불리언이 아니면 None. 없으면 모두 처음 값
    assert!(run_of(json!({"run": {"harness": {"changed": [{"path": "a"}, 1]}}})).harness_changed.is_empty());
    let r = run_of(json!({"run": {"harness": {"same": "yes", "files": "3"}}}));
    assert_eq!((r.harness_same, r.harness_files), (None, 0));
    let r = run_of(json!({"run": {"harness": 5}}));
    assert_eq!((r.harness_same, r.harness_files, r.harness_changed.len()), (None, 0, 0));
}

#[test]
fn extra_단위_상태와_이름과_가지() {
    // 이름: queue 의 것이 먼저(널이어도 queue 의 것 = 빈 글), 키가 없을 때만 엔진의 것
    assert_eq!(unit_of(json!({}), json!({"title": "엔진"})).title, "엔진");
    assert_eq!(unit_of(json!({"title": "큐"}), json!({"title": "엔진"})).title, "큐");
    assert_eq!(unit_of(json!({"title": null}), json!({"title": "엔진"})).title, "");
    // 상태: queue → 엔진 → pending
    assert_eq!(unit_of(json!({"status": "done"}), json!({"status": "failed"})).status, "done");
    assert_eq!(unit_of(json!({"status": ""}), json!({"status": "failed"})).status, "failed");
    assert_eq!(unit_of(json!({}), json!({"status": ""})).status, "pending");
    assert_eq!(unit_of(json!({"status": 3}), json!({})).status, "pending");
    // 가지: 엔진 → queue
    assert_eq!(unit_of(json!({"branch": "qb"}), json!({"branch": "eb"})).branch, "eb");
    assert_eq!(unit_of(json!({"branch": "qb"}), json!({"branch": ""})).branch, "qb");
    // 합치기 상태
    assert_eq!(unit_of(json!({}), json!({"merge": {"state": "merged"}})).merge, "merged");
    assert_eq!(unit_of(json!({}), json!({"merge": "merged"})).merge, "");
    // 엔진 기록이 없으면 처음 값
    let r = run_of(json!({"run": {}, "queue": [{"title": "a"}]}));
    assert_eq!(r.units[0], ReportUnit { n: 1, title: "a".into(), ..ReportUnit::default() });
    // 차례는 1부터
    let r = run_of(json!({"run": {}, "queue": [{}, {}, {}]}));
    assert_eq!(r.units.iter().map(|u| u.n).collect::<Vec<_>>(), [1, 2, 3]);
}

#[test]
fn extra_단위_글_상한() {
    let long = glyphs(1100);
    let u = unit_of(
        json!({"title": long, "kind": glyphs(50), "story": glyphs(90), "tickets": [glyphs(90)]}),
        json!({"reason": long}),
    );
    for (got, max) in [(&u.title, 300), (&u.kind, 40), (&u.reason, 1000), (&u.story, 80), (&u.task, 80)] {
        assert!(grapheme_len(got) == max + 1 && got.ends_with('…'), "{max}");
    }
}

#[test]
fn extra_단위_수_칸() {
    let u = unit_of(
        json!({}),
        json!({"active_sec": 12.5, "cost_usd": 0.25, "turns": 2.9, "try": -2.9, "started": 5, "ended": 6.5}),
    );
    assert_eq!((u.active_sec, u.cost, u.turns, u.retries, u.started, u.ended), (12.5, 0.25, 2, -2, Some(5.0), Some(6.5)));
    // 글·불리언·1e15 이상은 처음 값
    let u = unit_of(json!({}), json!({"active_sec": "9", "cost_usd": true, "turns": 1e15, "try": "1", "started": "5"}));
    assert_eq!((u.active_sec, u.cost, u.turns, u.retries, u.started), (0.0, 0.0, 0, 0, None));
    assert_eq!(unit_of(json!({}), json!({"turns": 999_999_999_999_999i64})).turns, 999_999_999_999_999);
}

#[test]
fn extra_단위_작업_키() {
    // 스토리가 있고 tickets 의 처음 것이 있을 때만. 글이 아닌 처음 원소는 빈 글
    assert_eq!(unit_of(json!({"story": "S", "tickets": ["A", "B"]}), json!({})).task, "A");
    assert_eq!(unit_of(json!({"tickets": ["A"]}), json!({})).task, "");
    assert_eq!(unit_of(json!({"story": "S", "tickets": []}), json!({})).task, "");
    assert_eq!(unit_of(json!({"story": "S", "tickets": [{"x": 1}, "B"]}), json!({})).task, "");
    assert_eq!(unit_of(json!({"story": "S", "tickets": [4]}), json!({})).task, "4");
    assert_eq!(unit_of(json!({"story": "S", "tickets": "A"}), json!({})).task, "");
}

#[test]
fn extra_판정_칸() {
    let u = unit_of(
        json!({}),
        json!({"checks": [
            {"try": 4, "exit": 2, "sec": 1.5, "tail": "끝", "timed_out": true, "interrupted": true},
            {"exit": "x", "timed_out": 1, "interrupted": "yes"},
            {"try": 0.9, "exit": 3.7},
        ]}),
    );
    assert_eq!(
        u.checks,
        [
            ReportCheck { n: 5, exit: Some(2), sec: 1.5, tail: "끝".into(), timed_out: true, interrupted: true },
            ReportCheck { n: 2, exit: None, sec: 0.0, tail: "".into(), timed_out: false, interrupted: false },
            ReportCheck { n: 1, exit: Some(3), sec: 0.0, tail: "".into(), timed_out: false, interrupted: false },
        ]
    );
    assert_eq!(u.last_exit, Some(3));
    // 마지막 판정에 종료 코드가 없으면 엔진의 check_exit, 판정이 없어도 마찬가지
    let u = unit_of(json!({}), json!({"checks": [{"exit": 1}, {}], "check_exit": 9}));
    assert_eq!(u.last_exit, Some(9));
    assert_eq!(unit_of(json!({}), json!({"check_exit": 4})).last_exit, Some(4));
    assert_eq!(unit_of(json!({}), json!({"checks": [{"exit": 1}, {}]})).last_exit, None);
    assert_eq!(unit_of(json!({}), json!({"checks": [{"exit": 0}], "check_exit": 9})).last_exit, Some(0));
    // 앞 50 번만, 객체 아닌 원소가 하나라도 있으면 통째로 비어 있다
    let many: Vec<Value> = (0..60).map(|_| json!({"exit": 0})).collect();
    let u = unit_of(json!({}), json!({"checks": many}));
    assert_eq!((u.checks.len(), u.checks[49].n), (50, 50));
    assert!(unit_of(json!({}), json!({"checks": [{"exit": 0}, 5]})).checks.is_empty());
}

#[test]
fn extra_판정_출력_끝부분() {
    let tail_of = |t: String| unit_of(json!({}), json!({"checks": [{"tail": t}]})).checks.remove(0).tail;
    // 상한과 같으면 그대로, 넘으면 끝 상한 글자 앞에 …
    let exact = "x".repeat(REPORT_TAIL_MAX);
    assert_eq!(tail_of(exact.clone()), exact);
    let over = format!("{}{}", "a".repeat(5), "b".repeat(REPORT_TAIL_MAX));
    assert_eq!(tail_of(over), format!("…{}", "b".repeat(REPORT_TAIL_MAX)));
    // 글자는 덩어리 단위(CR LF 는 한 글자)
    let crlf = format!("앞{}", "\r\n".repeat(REPORT_TAIL_MAX));
    assert_eq!(tail_of(crlf), format!("…{}", "\r\n".repeat(REPORT_TAIL_MAX)));
    // 긴 글은 10만 글자에서 한 번 더 잘린다(첫 자름이 붙인 … 가 끝 상한 글자 안에 든다)
    let huge = "y".repeat(100_050);
    assert_eq!(tail_of(huge), format!("…{}…", "y".repeat(REPORT_TAIL_MAX - 1)));
}

#[test]
fn extra_거부_칸() {
    let u = unit_of(
        json!({}),
        json!({"denials": [
            {"tool": "Bash", "what": "git push"}, {"tool": "Read"}, {"what": "홀로"}, {}, {"tool": 3, "what": 4},
        ]}),
    );
    assert_eq!(u.denials, ["Bash: git push", "Read", "홀로", "", "4"]);
    assert!(unit_of(json!({}), json!({"denials": [{"tool": "x"}, "y"]})).denials.is_empty());
    let many: Vec<Value> = (0..40).map(|i| json!({"tool": format!("t{i}")})).collect();
    let d = unit_of(json!({}), json!({"denials": many})).denials;
    assert_eq!((d.len(), d[29].as_str()), (30, "t29"));
    let d = unit_of(json!({}), json!({"denials": [{"tool": "T", "what": glyphs(400)}]})).denials;
    assert_eq!(grapheme_len(&d[0]), "T: ".len() + 301);
}

#[test]
fn extra_결과_기록_상태와_종류() {
    // 없으면 Missing, 읽히면 Ok 와 값, 깨지면 Broken. 종류는 queue 의 것이 먼저고 비었을 때만 결과 기록의 것
    let go = |q: Value, res: Value| {
        let src = FixtureSource::new(json!({"run": {}, "queue": [q], "results": {"1": res}}));
        report_run("x", &src).unwrap().units.remove(0)
    };
    let u = go(json!({"kind": "dev"}), json!({"unit": {"kind": "fix"}, "headline": "요약"}));
    assert_eq!((u.kind.as_str(), u.result_state), ("dev", ResultState::Ok));
    assert_eq!(u.result.as_ref().unwrap().headline, "요약");
    let u = go(json!({}), json!({"unit": {"kind": "fix"}}));
    assert_eq!((u.kind.as_str(), u.result_state), ("fix", ResultState::Ok));
    let u = go(json!({}), json!("[1]"));
    assert_eq!((u.kind.as_str(), u.result_state, u.result.is_none()), ("", ResultState::Broken, true));
    let u = unit_of(json!({"kind": "x"}), json!({}));
    assert_eq!((u.result_state, u.result.is_none()), (ResultState::Missing, true));
}

#[test]
fn extra_queue_가_객체가_아니면_단위가_없다() {
    let r = run_of(json!({"run": {}, "queue": [{"title": "a"}, 5]}));
    assert!(r.units.is_empty());
}

#[test]
fn extra_마지막_경로_조각() {
    for (p, want) in [
        ("/Users/toy/workspace/todo-api", "todo-api"),
        ("todo-api", "todo-api"),
        ("/a/b/", "b"),
        ("/a/b//", "b"),
        ("a//b", "b"),
        ("C:\\work\\todo-api", "todo-api"),
        ("C:\\work\\todo-api\\", "todo-api"),
        ("C:/work/todo-api", "todo-api"),
        ("D:\\", "D:"),
        ("/", "/"),
        ("//", "/"),
        ("\\", "\\"),
        ("", ""),
    ] {
        assert_eq!(last_part(p), want, "{p:?}");
    }
}

#[test]
fn extra_이름표_사전() {
    let unit = [("pending", "대기"), ("running", "진행 중"), ("done", "통과"), ("failed", "미통과"), ("capped", "상한에서 멈춤"), ("stopped", "중단"), ("skipped", "건너뜀")];
    for (k, v) in unit {
        assert_eq!(unit_label(k), Some(v), "{k}");
    }
    assert_eq!((unit_label("Done"), unit_label(""), unit_label("other")), (None, None, None));
    for (k, v) in [("done", "ok"), ("failed", "fail"), ("capped", "cap"), ("running", "run")] {
        assert_eq!(unit_class(k), Some(v), "{k}");
    }
    for k in ["pending", "stopped", "skipped", ""] {
        assert_eq!(unit_class(k), None, "{k}");
    }
    let run = [
        ("starting", "출발 중"), ("running", "도는 중"), ("waiting_limit", "한도 대기"), ("waiting_stopline", "멈출 선 대기"), ("waiting_precheck", "환경 확인 대기"),
        ("paused", "일시정지"), ("stopped", "중단됨"), ("done", "끝남"), ("error", "오류로 멈춤"),
    ];
    for (k, v) in run {
        assert_eq!(run_label(k), Some(v), "{k}");
    }
    assert_eq!(run_label("pending"), None);
    let kinds = [("fix", "고침"), ("dev", "개발"), ("feature", "개발"), ("story", "개발"), ("test", "시험"), ("doc", "문서"), ("docs", "문서"), ("env", "환경")];
    for (k, v) in kinds {
        assert_eq!(kind_label(k), Some(v), "{k}");
        assert_eq!(kind_text(&k.to_uppercase()), v, "{k}");
    }
    assert_eq!((kind_label("FIX"), kind_label("")), (None, None));
    assert_eq!((kind_text("Other"), kind_text("")), ("Other".to_string(), String::new()));
    for (k, v) in [("done", "끝냄"), ("failed", "끝내지 못함"), ("parked", "보류"), ("capped", "상한")] {
        assert_eq!(outcome_label(k), Some(v), "{k}");
    }
    assert_eq!(outcome_label("skipped"), None);
    assert_eq!(test_label("pass"), Some(("통과", "ok")));
    assert_eq!(test_label("fail"), Some(("실패", "fail")));
    assert_eq!(test_label("skip"), Some(("건너뜀", "")));
    assert_eq!(test_label("other"), None);
    for (k, v) in [("merged", "스토리 가지에 합침"), ("nothing", "합칠 커밋 없음"), ("not_merged", "스토리 가지에 합치지 않음")] {
        assert_eq!(merge_label(k), Some(v), "{k}");
    }
    assert_eq!(merge_label("x"), None);
}

#[test]
fn extra_걸린_시간() {
    for (sec, want) in [
        (0.0, "0분"),
        (0.9, "0분"),
        (1.0, "1초"),
        (59.9, "59초"),
        (60.0, "1분"),
        (119.0, "1분"),
        (3599.0, "59분"),
        (3600.0, "1시간 0분"),
        (3725.0, "1시간 2분"),
        (7200.0, "2시간 0분"),
        (-5.0, "0분"),
        (f64::NAN, "0분"),
        (f64::INFINITY, "0분"),
        (f64::NEG_INFINITY, "0분"),
    ] {
        assert_eq!(report_dur(sec), want, "{sec}");
    }
}

// ── 3/4 구간: Writer · chip · css (원본 253~334줄, Swift 에서 비공개 → 시험은 골든과 `결정적` 의 시각 부분) ──────────────

fn golden(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Tests/AutopilotRulesTests/Report").join(name);
    // 윈도우 체크아웃은 줄 끝이 CRLF 일 수 있다. Swift 가 쓴 글은 LF 뿐이다.
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
}

fn seoul() -> Writer {
    Writer::new(FixedZone::SEOUL, None)
}

fn num_at(v: &Value, path: &[&str]) -> Option<f64> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    cur.as_f64()
}

/// 파이썬 datetime(독립 구현)이 만든 경계 표: 서울 자정 앞뒤 1초(월말·연말·윤일·평년 2월·100년 평년·400년 윤년), 소수 초, 골든 시각, 시대 가장자리, UTC·서울·UTC-5.
const DATE_ROWS: [(f64, i32, &str, &str); 183] = [
    (1793458799.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 31일 23:59"),
    (1793458799.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 10월 31일 14:59"),
    (1793458799.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-31 23:59:59"),
    (1793458799.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-10-31 14:59:59"),
    (1793458800.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 11월 1일 00:00"),
    (1793458800.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 10월 31일 15:00"),
    (1793458800.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-11-01 00:00:00"),
    (1793458800.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-10-31 15:00:00"),
    (1793458801.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 11월 1일 00:00"),
    (1793458801.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 10월 31일 15:00"),
    (1793458801.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-11-01 00:00:01"),
    (1793458801.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-10-31 15:00:01"),
    (1793458799.5, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-31 23:59:59"),
    (1793458800.999, 32400, "yyyy-MM-dd HH:mm:ss", "2026-11-01 00:00:00"),
    (1798729199.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 12월 31일 23:59"),
    (1798729199.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 12월 31일 14:59"),
    (1798729199.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-12-31 23:59:59"),
    (1798729199.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-12-31 14:59:59"),
    (1798729200.0, 32400, "yyyy년 M월 d일 HH:mm", "2027년 1월 1일 00:00"),
    (1798729200.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 12월 31일 15:00"),
    (1798729200.0, 32400, "yyyy-MM-dd HH:mm:ss", "2027-01-01 00:00:00"),
    (1798729200.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-12-31 15:00:00"),
    (1798729201.0, 32400, "yyyy년 M월 d일 HH:mm", "2027년 1월 1일 00:00"),
    (1798729201.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 12월 31일 15:00"),
    (1798729201.0, 32400, "yyyy-MM-dd HH:mm:ss", "2027-01-01 00:00:01"),
    (1798729201.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-12-31 15:00:01"),
    (1798729199.5, 32400, "yyyy-MM-dd HH:mm:ss", "2026-12-31 23:59:59"),
    (1798729200.999, 32400, "yyyy-MM-dd HH:mm:ss", "2027-01-01 00:00:00"),
    (1709132399.0, 32400, "yyyy년 M월 d일 HH:mm", "2024년 2월 28일 23:59"),
    (1709132399.0, 0, "yyyy년 M월 d일 HH:mm", "2024년 2월 28일 14:59"),
    (1709132399.0, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-28 23:59:59"),
    (1709132399.0, 0, "yyyy-MM-dd HH:mm:ss", "2024-02-28 14:59:59"),
    (1709132400.0, 32400, "yyyy년 M월 d일 HH:mm", "2024년 2월 29일 00:00"),
    (1709132400.0, 0, "yyyy년 M월 d일 HH:mm", "2024년 2월 28일 15:00"),
    (1709132400.0, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-29 00:00:00"),
    (1709132400.0, 0, "yyyy-MM-dd HH:mm:ss", "2024-02-28 15:00:00"),
    (1709132401.0, 32400, "yyyy년 M월 d일 HH:mm", "2024년 2월 29일 00:00"),
    (1709132401.0, 0, "yyyy년 M월 d일 HH:mm", "2024년 2월 28일 15:00"),
    (1709132401.0, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-29 00:00:01"),
    (1709132401.0, 0, "yyyy-MM-dd HH:mm:ss", "2024-02-28 15:00:01"),
    (1709132399.5, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-28 23:59:59"),
    (1709132400.999, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-29 00:00:00"),
    (1709218799.0, 32400, "yyyy년 M월 d일 HH:mm", "2024년 2월 29일 23:59"),
    (1709218799.0, 0, "yyyy년 M월 d일 HH:mm", "2024년 2월 29일 14:59"),
    (1709218799.0, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-29 23:59:59"),
    (1709218799.0, 0, "yyyy-MM-dd HH:mm:ss", "2024-02-29 14:59:59"),
    (1709218800.0, 32400, "yyyy년 M월 d일 HH:mm", "2024년 3월 1일 00:00"),
    (1709218800.0, 0, "yyyy년 M월 d일 HH:mm", "2024년 2월 29일 15:00"),
    (1709218800.0, 32400, "yyyy-MM-dd HH:mm:ss", "2024-03-01 00:00:00"),
    (1709218800.0, 0, "yyyy-MM-dd HH:mm:ss", "2024-02-29 15:00:00"),
    (1709218801.0, 32400, "yyyy년 M월 d일 HH:mm", "2024년 3월 1일 00:00"),
    (1709218801.0, 0, "yyyy년 M월 d일 HH:mm", "2024년 2월 29일 15:00"),
    (1709218801.0, 32400, "yyyy-MM-dd HH:mm:ss", "2024-03-01 00:00:01"),
    (1709218801.0, 0, "yyyy-MM-dd HH:mm:ss", "2024-02-29 15:00:01"),
    (1709218799.5, 32400, "yyyy-MM-dd HH:mm:ss", "2024-02-29 23:59:59"),
    (1709218800.999, 32400, "yyyy-MM-dd HH:mm:ss", "2024-03-01 00:00:00"),
    (1677596399.0, 32400, "yyyy년 M월 d일 HH:mm", "2023년 2월 28일 23:59"),
    (1677596399.0, 0, "yyyy년 M월 d일 HH:mm", "2023년 2월 28일 14:59"),
    (1677596399.0, 32400, "yyyy-MM-dd HH:mm:ss", "2023-02-28 23:59:59"),
    (1677596399.0, 0, "yyyy-MM-dd HH:mm:ss", "2023-02-28 14:59:59"),
    (1677596400.0, 32400, "yyyy년 M월 d일 HH:mm", "2023년 3월 1일 00:00"),
    (1677596400.0, 0, "yyyy년 M월 d일 HH:mm", "2023년 2월 28일 15:00"),
    (1677596400.0, 32400, "yyyy-MM-dd HH:mm:ss", "2023-03-01 00:00:00"),
    (1677596400.0, 0, "yyyy-MM-dd HH:mm:ss", "2023-02-28 15:00:00"),
    (1677596401.0, 32400, "yyyy년 M월 d일 HH:mm", "2023년 3월 1일 00:00"),
    (1677596401.0, 0, "yyyy년 M월 d일 HH:mm", "2023년 2월 28일 15:00"),
    (1677596401.0, 32400, "yyyy-MM-dd HH:mm:ss", "2023-03-01 00:00:01"),
    (1677596401.0, 0, "yyyy-MM-dd HH:mm:ss", "2023-02-28 15:00:01"),
    (1677596399.5, 32400, "yyyy-MM-dd HH:mm:ss", "2023-02-28 23:59:59"),
    (1677596400.999, 32400, "yyyy-MM-dd HH:mm:ss", "2023-03-01 00:00:00"),
    (4107509999.0, 32400, "yyyy년 M월 d일 HH:mm", "2100년 2월 28일 23:59"),
    (4107509999.0, 0, "yyyy년 M월 d일 HH:mm", "2100년 2월 28일 14:59"),
    (4107509999.0, 32400, "yyyy-MM-dd HH:mm:ss", "2100-02-28 23:59:59"),
    (4107509999.0, 0, "yyyy-MM-dd HH:mm:ss", "2100-02-28 14:59:59"),
    (4107510000.0, 32400, "yyyy년 M월 d일 HH:mm", "2100년 3월 1일 00:00"),
    (4107510000.0, 0, "yyyy년 M월 d일 HH:mm", "2100년 2월 28일 15:00"),
    (4107510000.0, 32400, "yyyy-MM-dd HH:mm:ss", "2100-03-01 00:00:00"),
    (4107510000.0, 0, "yyyy-MM-dd HH:mm:ss", "2100-02-28 15:00:00"),
    (4107510001.0, 32400, "yyyy년 M월 d일 HH:mm", "2100년 3월 1일 00:00"),
    (4107510001.0, 0, "yyyy년 M월 d일 HH:mm", "2100년 2월 28일 15:00"),
    (4107510001.0, 32400, "yyyy-MM-dd HH:mm:ss", "2100-03-01 00:00:01"),
    (4107510001.0, 0, "yyyy-MM-dd HH:mm:ss", "2100-02-28 15:00:01"),
    (4107509999.5, 32400, "yyyy-MM-dd HH:mm:ss", "2100-02-28 23:59:59"),
    (4107510000.999, 32400, "yyyy-MM-dd HH:mm:ss", "2100-03-01 00:00:00"),
    (951749999.0, 32400, "yyyy년 M월 d일 HH:mm", "2000년 2월 28일 23:59"),
    (951749999.0, 0, "yyyy년 M월 d일 HH:mm", "2000년 2월 28일 14:59"),
    (951749999.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-28 23:59:59"),
    (951749999.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-28 14:59:59"),
    (951750000.0, 32400, "yyyy년 M월 d일 HH:mm", "2000년 2월 29일 00:00"),
    (951750000.0, 0, "yyyy년 M월 d일 HH:mm", "2000년 2월 28일 15:00"),
    (951750000.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-29 00:00:00"),
    (951750000.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-28 15:00:00"),
    (951750001.0, 32400, "yyyy년 M월 d일 HH:mm", "2000년 2월 29일 00:00"),
    (951750001.0, 0, "yyyy년 M월 d일 HH:mm", "2000년 2월 28일 15:00"),
    (951750001.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-29 00:00:01"),
    (951750001.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-28 15:00:01"),
    (951749999.5, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-28 23:59:59"),
    (951750000.999, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-29 00:00:00"),
    (951836399.0, 32400, "yyyy년 M월 d일 HH:mm", "2000년 2월 29일 23:59"),
    (951836399.0, 0, "yyyy년 M월 d일 HH:mm", "2000년 2월 29일 14:59"),
    (951836399.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-29 23:59:59"),
    (951836399.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-29 14:59:59"),
    (951836400.0, 32400, "yyyy년 M월 d일 HH:mm", "2000년 3월 1일 00:00"),
    (951836400.0, 0, "yyyy년 M월 d일 HH:mm", "2000년 2월 29일 15:00"),
    (951836400.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-03-01 00:00:00"),
    (951836400.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-29 15:00:00"),
    (951836401.0, 32400, "yyyy년 M월 d일 HH:mm", "2000년 3월 1일 00:00"),
    (951836401.0, 0, "yyyy년 M월 d일 HH:mm", "2000년 2월 29일 15:00"),
    (951836401.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-03-01 00:00:01"),
    (951836401.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-29 15:00:01"),
    (951836399.5, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-29 23:59:59"),
    (951836400.999, 32400, "yyyy-MM-dd HH:mm:ss", "2000-03-01 00:00:00"),
    (1793285999.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 29일 23:59"),
    (1793285999.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 10월 29일 14:59"),
    (1793285999.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-29 23:59:59"),
    (1793285999.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-10-29 14:59:59"),
    (1793286000.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 00:00"),
    (1793286000.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 10월 29일 15:00"),
    (1793286000.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-30 00:00:00"),
    (1793286000.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-10-29 15:00:00"),
    (1793286001.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 00:00"),
    (1793286001.0, 0, "yyyy년 M월 d일 HH:mm", "2026년 10월 29일 15:00"),
    (1793286001.0, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-30 00:00:01"),
    (1793286001.0, 0, "yyyy-MM-dd HH:mm:ss", "2026-10-29 15:00:01"),
    (1793285999.5, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-29 23:59:59"),
    (1793286000.999, 32400, "yyyy-MM-dd HH:mm:ss", "2026-10-30 00:00:00"),
    (1793318400.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 09:00"),
    (1793318400.0, 32400, "M월 d일 HH:mm", "10월 30일 09:00"),
    (1793318400.0, 32400, "HH:mm", "09:00"),
    (1793318400.0, 0, "HH:mm", "00:00"),
    (1793329500.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 12:05"),
    (1793329500.0, 32400, "M월 d일 HH:mm", "10월 30일 12:05"),
    (1793329500.0, 32400, "HH:mm", "12:05"),
    (1793329500.0, 0, "HH:mm", "03:05"),
    (1793329800.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 12:10"),
    (1793329800.0, 32400, "M월 d일 HH:mm", "10월 30일 12:10"),
    (1793329800.0, 32400, "HH:mm", "12:10"),
    (1793329800.0, 0, "HH:mm", "03:10"),
    (1793318460.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 09:01"),
    (1793318460.0, 32400, "M월 d일 HH:mm", "10월 30일 09:01"),
    (1793318460.0, 32400, "HH:mm", "09:01"),
    (1793318460.0, 0, "HH:mm", "00:01"),
    (1793320100.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 09:28"),
    (1793320100.0, 32400, "M월 d일 HH:mm", "10월 30일 09:28"),
    (1793320100.0, 32400, "HH:mm", "09:28"),
    (1793320100.0, 0, "HH:mm", "00:28"),
    (1793323500.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 10:25"),
    (1793323500.0, 32400, "M월 d일 HH:mm", "10월 30일 10:25"),
    (1793323500.0, 32400, "HH:mm", "10:25"),
    (1793323500.0, 0, "HH:mm", "01:25"),
    (1793325510.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 10:58"),
    (1793325510.0, 32400, "M월 d일 HH:mm", "10월 30일 10:58"),
    (1793325510.0, 32400, "HH:mm", "10:58"),
    (1793325510.0, 0, "HH:mm", "01:58"),
    (1793322000.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 10:00"),
    (1793322000.0, 32400, "M월 d일 HH:mm", "10월 30일 10:00"),
    (1793322000.0, 32400, "HH:mm", "10:00"),
    (1793322000.0, 0, "HH:mm", "01:00"),
    (1793320200.0, 32400, "yyyy년 M월 d일 HH:mm", "2026년 10월 30일 09:30"),
    (1793320200.0, 32400, "M월 d일 HH:mm", "10월 30일 09:30"),
    (1793320200.0, 32400, "HH:mm", "09:30"),
    (1793320200.0, 0, "HH:mm", "00:30"),
    (0.0, 32400, "yyyy-MM-dd HH:mm:ss", "1970-01-01 09:00:00"),
    (0.0, 0, "yyyy-MM-dd HH:mm:ss", "1970-01-01 00:00:00"),
    (0.0, -18000, "yyyy-MM-dd HH:mm:ss", "1969-12-31 19:00:00"),
    (-1.0, 32400, "yyyy-MM-dd HH:mm:ss", "1970-01-01 08:59:59"),
    (-1.0, 0, "yyyy-MM-dd HH:mm:ss", "1969-12-31 23:59:59"),
    (-1.0, -18000, "yyyy-MM-dd HH:mm:ss", "1969-12-31 18:59:59"),
    (-0.5, 32400, "yyyy-MM-dd HH:mm:ss", "1970-01-01 08:59:59"),
    (-0.5, 0, "yyyy-MM-dd HH:mm:ss", "1969-12-31 23:59:59"),
    (-0.5, -18000, "yyyy-MM-dd HH:mm:ss", "1969-12-31 18:59:59"),
    (86399.0, 32400, "yyyy-MM-dd HH:mm:ss", "1970-01-02 08:59:59"),
    (86399.0, 0, "yyyy-MM-dd HH:mm:ss", "1970-01-01 23:59:59"),
    (86399.0, -18000, "yyyy-MM-dd HH:mm:ss", "1970-01-01 18:59:59"),
    (86400.0, 32400, "yyyy-MM-dd HH:mm:ss", "1970-01-02 09:00:00"),
    (86400.0, 0, "yyyy-MM-dd HH:mm:ss", "1970-01-02 00:00:00"),
    (86400.0, -18000, "yyyy-MM-dd HH:mm:ss", "1970-01-01 19:00:00"),
    (951782399.0, 32400, "yyyy-MM-dd HH:mm:ss", "2000-02-29 08:59:59"),
    (951782399.0, 0, "yyyy-MM-dd HH:mm:ss", "2000-02-28 23:59:59"),
    (951782399.0, -18000, "yyyy-MM-dd HH:mm:ss", "2000-02-28 18:59:59"),
    (4102444799.0, 32400, "yyyy-MM-dd HH:mm:ss", "2100-01-01 08:59:59"),
    (4102444799.0, 0, "yyyy-MM-dd HH:mm:ss", "2099-12-31 23:59:59"),
    (4102444799.0, -18000, "yyyy-MM-dd HH:mm:ss", "2099-12-31 18:59:59"),
];

#[test]
fn date_경계표() {
    let mut bad = Vec::new();
    for (ts, off, f, want) in DATE_ROWS {
        let got = Writer::new(FixedZone { offset_secs: off }, None).date(Some(ts), f);
        if got != want {
            bad.push(format!("{ts} off={off} {f}: got {got:?} want {want:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn date_모름() {
    // `guard let ts, ts.isFinite else { return "때 모름" }`
    for ts in [None, Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY)] {
        for f in ["yyyy년 M월 d일 HH:mm", "M월 d일 HH:mm", "HH:mm"] {
            assert_eq!(seoul().date(ts, f), "때 모름", "{ts:?} {f}");
        }
    }
    // 0 은 모름이 아니다
    assert_eq!(seoul().date(Some(0.0), "HH:mm"), "09:00");
}

#[test]
fn date_결정적() {
    // Swift `ReportTests.결정적` 의 시각 부분: 같은 기록을 UTC 로 그리면 견본의 시작이 00:00, 서울이면 09:00 이다
    let j = fixture("sample-run.json");
    let started = num_at(&j, &["run", "started"]);
    assert_eq!(started, Some(1_793_318_400.0));
    let utc = Writer::new(FixedZone::UTC, None).date(started, "yyyy년 M월 d일 HH:mm");
    let kst = Writer::new(FixedZone::SEOUL, None).date(started, "yyyy년 M월 d일 HH:mm");
    assert_eq!(utc, "2026년 10월 30일 00:00");
    assert_eq!(kst, "2026년 10월 30일 09:00");
    assert_eq!(FixedZone::SEOUL.offset_secs, 32_400);
    assert_eq!(FixedZone::UTC.offset_secs, 0);
}

#[test]
fn date_골든() {
    // 골든 HTML 의 때 글자와 바이트로 같은가: 머리 meta 줄(시작 ~ 끝)과 바닥 글(그린 때)
    for (run, html) in [("sample-run.json", "sample-report.html"), ("evil-run.json", "evil-report.html"), ("sample-run.json", "sample-unit-2.html")] {
        let j = fixture(run);
        let g = golden(html);
        let w = seoul();
        let s = w.date(num_at(&j, &["run", "started"]), "yyyy년 M월 d일 HH:mm");
        let e = w.date(num_at(&j, &["run", "ended"]), "M월 d일 HH:mm");
        let at = w.date(num_at(&j, &["at"]), "yyyy년 M월 d일 HH:mm");
        assert!(g.contains(&format!("{s} ~ {e} · ")), "{run}: {s} ~ {e}");
        assert!(g.contains(&format!("그린 때 {at} · 바탕 ")), "{run}: {at}");
    }
    // 단위 시각(`HH:mm ~ HH:mm`) — 견본 골든의 단위 머리 줄
    let g = golden("sample-report.html");
    let w = seoul();
    for (a, b) in [(1_793_318_460.0, 1_793_320_100.0), (1_793_320_160.0, 1_793_323_500.0), (1_793_325_510.0, 1_793_325_510.0)] {
        let line = format!("{} ~ {}</p>", w.date(Some(a), "HH:mm"), w.date(Some(b), "HH:mm"));
        assert!(g.contains(&line), "{line}");
    }
    // 때가 없으면 `때 모름`(골든에는 없는 칸)
    assert!(!g.contains("때 모름"));
}

#[test]
fn date_무늬() {
    let w = Writer::new(FixedZone::UTC, None);
    let t = Some(1_793_318_400.0 + 18_429.0); // 2026-10-30 05:07:09 UTC
    assert_eq!(w.date(t, "yyyy년 M월 d일 HH:mm"), "2026년 10월 30일 05:07");
    assert_eq!(w.date(t, "yy"), "26");
    assert_eq!(w.date(t, "y"), "2026");
    assert_eq!(w.date(t, "yyyyy"), "02026");
    assert_eq!(w.date(t, "H:m:s"), "5:7:9");
    assert_eq!(w.date(t, "HH:mm:ss"), "05:07:09");
    assert_eq!(w.date(t, "MM-dd"), "10-30");
    // 따옴표 글과 `''`
    assert_eq!(w.date(t, "HH'시' mm'분'"), "05시 07분");
    assert_eq!(w.date(t, "HH 'mm' ''"), "05 mm '");
    assert_eq!(w.date(t, "'it''s' HH"), "it's 05");
    // 풀지 않는 글자는 그대로(이어 쓴 것도 그 길이만큼)
    assert_eq!(w.date(t, "hh:QQQ"), "hh:QQQ");
    // 서기 1년 1월 1일 0시(UTC)
    assert_eq!(w.date(Some(-62_135_596_800.0), "yyyy-MM-dd HH:mm:ss"), "0001-01-01 00:00:00");
    // `y` 한 글자는 0 을 채우지 않는다
    assert_eq!(w.date(Some(-62_135_596_800.0), "y"), "1");
    assert_eq!(w.date(Some(-62_135_596_800.0), "yy"), "01");
    assert_eq!(w.date(Some(-62_135_596_800.0), "yyyy"), "0001");
    // 작은 해: 2005 → yy 05
    assert_eq!(w.date(Some(1_105_000_000.0), "yy"), "05");
    // 아주 큰 수도 멈추지 않는다(끝값에 붙는다)
    assert!(!w.date(Some(1e300), "yyyy").is_empty());
    assert!(!w.date(Some(-1e300), "yyyy").is_empty());
    // 달력 값
    let c = FixedZone::SEOUL.civil(1_793_458_800.0);
    assert_eq!((c.year, c.month, c.day, c.hour, c.minute, c.second), (2026, 11, 1, 0, 0, 0));
}

#[test]
fn writer_글자() {
    // `t` 는 긴 줄표를 쉼표로 바꾸고 이스케이프한다, `c` 는 홈을 줄이고 이스케이프한다(줄표는 그대로)
    let w = Writer::new(FixedZone::SEOUL, Some("/Users/toy"));
    assert_eq!(w.t("가 — 나 <b>&\"'"), "가, 나 &lt;b&gt;&amp;&quot;&#39;");
    assert_eq!(w.c("/Users/toy/work/a — b & <c>"), "~/work/a — b &amp; &lt;c&gt;");
    // 홈 뒤에 `/` 가 와야 줄인다
    assert_eq!(w.short("/Users/toyota/x"), "/Users/toyota/x");
    assert_eq!(w.short("/Users/toy"), "/Users/toy");
    assert_eq!(w.short("/Users/toy/a /Users/toy/b"), "~/a ~/b");
    assert_eq!(w.short("x/Users/toy/y"), "x~/y");
    // 홈이 없거나 한 글자 이하면 줄이지 않는다
    assert_eq!(Writer::new(FixedZone::UTC, None).short("/Users/toy/x"), "/Users/toy/x");
    assert_eq!(Writer::new(FixedZone::UTC, Some("/")).short("//x"), "//x");
    assert_eq!(Writer::new(FixedZone::UTC, Some("")).short("/x"), "/x");
    assert_eq!(Writer::new(FixedZone::UTC, Some("a")).short("a/x"), "a/x");
    assert_eq!(Writer::new(FixedZone::UTC, Some("ab")).short("ab/x"), "~/x");
    // 글자 수는 Character 수: 한 글자 홈(CR LF 한 글자)도 줄이지 않는다
    assert_eq!(Writer::new(FixedZone::UTC, Some("\r\n")).short("\r\n/x"), "\r\n/x");
    // Windows 경로는 `/` 로 이어지지 않으면 줄이지 않는다(Swift 와 같다)
    assert_eq!(Writer::new(FixedZone::UTC, Some("C:\\Users\\x")).short("C:\\Users\\x\\proj"), "C:\\Users\\x\\proj");
    assert_eq!(Writer::new(FixedZone::UTC, Some("C:\\Users\\x")).short("C:\\Users\\x/proj"), "~/proj");
}

#[test]
fn writer_이어붙임() {
    let mut w = seoul();
    assert_eq!(w.output(), "");
    w.add("<p>");
    w.add("가");
    assert_eq!(w.output(), "<p>가");
    assert_eq!(w.clone().into_output(), "<p>가");
    assert_eq!(w.tz, FixedZone::SEOUL);
}

#[test]
fn chip_글() {
    assert_eq!(chip("통과", "ok"), "<span class=\"chip ok\">통과</span>");
    assert_eq!(chip("건너뜀", ""), "<span class=\"chip\">건너뜀</span>");
    assert_eq!(chip_plain("3개 중 1개 통과"), "<span class=\"chip\">3개 중 1개 통과</span>");
    // 글은 이스케이프하지 않는다(받은 대로)
    assert_eq!(chip("<b>", "fail"), "<span class=\"chip fail\"><b></span>");
    // 골든의 칩 글자와 같다
    let g = golden("sample-report.html");
    assert!(g.contains(&chip("통과", "ok")));
    assert!(g.contains(&chip("미통과", "fail")));
}

#[test]
fn css_골든() {
    // 골든 HTML 의 <style> 안과 바이트로 같다
    for name in ["sample-report.html", "evil-report.html", "sample-unit-2.html"] {
        let g = golden(name);
        let a = g.find("<style>\n").unwrap() + "<style>\n".len();
        let b = g.find("\n</style>").unwrap();
        let (got, want) = (CSS, &g[a..b]);
        if got != want {
            let at = got.bytes().zip(want.bytes()).position(|(x, y)| x != y).unwrap_or(got.len().min(want.len()));
            panic!("{name}: 달라지는 자리 {at}\n got: {:?}\nwant: {:?}", &got[at.saturating_sub(40)..(at + 40).min(got.len())], &want[at.saturating_sub(40)..(at + 40).min(want.len())]);
        }
    }
    // 앞뒤에 줄바꿈이 없다
    assert!(!CSS.starts_with('\n') && !CSS.ends_with('\n') && !CSS.contains('\r'));
}

#[test]
fn css_원본() {
    // Swift 소스의 여러 줄 글자(줄 끝 `\` 는 줄 이음)를 그 자리에서 풀어 같은지 본다
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Sources/AutopilotRules/Report.swift");
    let src = std::fs::read_to_string(p).unwrap().replace("\r\n", "\n");
    let open = "private let css = \"\"\"\n";
    let a = src.find(open).unwrap() + open.len();
    let b = a + src[a..].find("\n\"\"\"\n").unwrap();
    let want = src[a..b].replace("\\\n", "");
    assert_eq!(CSS, want);
}

// ── 보고서 HTML 시험(원본 ReportHTMLTests) — 보조 도구 ──

// 태그·속성 허용 목록 — 틀이 쓰는 것만. 모델 글이 태그·속성을 하나라도 더 만들면 걸린다(Swift `allowedTags`·`allowedAttrs`)
const ALLOWED_TAGS: [&str; 42] = [
    "html", "head", "meta", "title", "style", "body", "header", "nav", "section", "footer", "h1", "h2", "h3", "p", "span", "div", "dl", "dt", "dd", "ol", "ul", "li", "a", "table", "thead",
    "tbody", "tr", "th", "td", "code", "pre", "details", "summary", "figure", "figcaption", "svg", "rect", "line", "polyline", "polygon", "path", "text",
];
const ALLOWED_ATTRS: [&str; 24] = [
    "lang", "charset", "name", "content", "http-equiv", "class", "id", "href", "rel", "aria-label", "role", "viewBox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "points", "d", "rx",
    "text-anchor",
];

/// 태그·속성은 허용 목록의 것만, href 는 https·#unit- 만, 긴 줄표·방향 바꾸기 글자·스크립트 없음, CSP 있음, CSS 는 바깥 참조 없음.
/// 원본은 NSRegularExpression 두 개(`<([a-zA-Z][a-zA-Z0-9-]*)([^>]*)>` 와 `\s([^\s=/>]+)(="([^"]*)")?`)를 쓰는데 레포에 regex 의존성이 없어(`harness.rs` 같은 관례)
/// 손으로 같은 꼴을 훑는다. 태그는 `<` 뒤 첫 글자가 영문자일 때 이름(영문자로 시작, 뒤 영숫자·하이픈)을 읽고 `>` 앞까지를 속성 칸으로 삼는다.
fn safety(html: &str) -> Vec<String> {
    let mut bad: Vec<String> = vec![];
    let cs: Vec<char> = html.chars().collect();
    let mut at = 0;
    while at < cs.len() {
        let Some((name_end, tag_end)) = tag_at(&cs, at) else {
            at += 1;
            continue;
        };
        let name: String = cs[at + 1..name_end].iter().collect::<String>().to_lowercase();
        if !ALLOWED_TAGS.contains(&name.as_str()) {
            bad.push(format!("태그 {name}"));
        }
        let rest: Vec<char> = cs[name_end..tag_end].to_vec();
        let mut i = 0;
        while i < rest.len() {
            if rest[i].is_whitespace() {
                let mut j = i + 1;
                while j < rest.len() && !rest[j].is_whitespace() && rest[j] != '=' && rest[j] != '/' && rest[j] != '>' {
                    j += 1;
                }
                if j > i + 1 {
                    let an: String = rest[i + 1..j].iter().collect();
                    if !ALLOWED_ATTRS.contains(&an.as_str()) {
                        bad.push(format!("속성 {an}"));
                    }
                    // 값은 ="..." 꼴일 때만(원본의 옵션 그룹). href 값은 #unit- 또는 https:// 로만 열린다
                    if j + 1 < rest.len() && rest[j] == '=' && rest[j + 1] == '"' {
                        let Some(e) = rest[j + 2..].iter().position(|c| *c == '"').map(|k| j + 2 + k) else {
                            i = j;
                            continue;
                        };
                        let v: String = rest[j + 2..e].iter().collect();
                        if an == "href" && !(v.starts_with("#unit-") || v.starts_with("https://")) {
                            bad.push(format!("href {v}"));
                        }
                        i = e + 1;
                    } else {
                        i = j;
                    }
                    continue;
                }
            }
            i += 1;
        }
        at = tag_end + 1; // 원본 정규식은 겹치지 않게 맞힌 다음 자리부터 본다
        continue;
    }
    if html.contains('—') {
        bad.push("긴 줄표".into());
    }
    if cs.iter().any(|c| {
        let v = *c as u32;
        (0x202A..=0x202E).contains(&v) || (0x2066..=0x2069).contains(&v)
    }) {
        bad.push("방향 바꾸기 글자".into());
    }
    if html.split("<style>").count() != 2 {
        bad.push("style 태그 수".into());
    }
    if html.to_lowercase().split("<script").count() > 1 {
        bad.push("script".into());
    }
    const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\">";
    if !html.contains(CSP) {
        bad.push("CSP 없음".into());
    }
    let style = html.split("<style>").nth(1).and_then(|p| p.split("</style>").next()).unwrap_or("");
    if style.contains("url(") || style.contains("@import") {
        bad.push("CSS 바깥 참조".into());
    }
    bad
}

/// `at` 가 `<영문자…>` 태그의 시작이면 곱씹은 (이름 끝, '>' 자리) 를 돌려주고(원본 `<([a-zA-Z][a-zA-Z0-9-]*)([^>]*)>`), 아니면 None.
fn tag_at(cs: &[char], at: usize) -> Option<(usize, usize)> {
    if cs[at] != '<' || at + 1 >= cs.len() || !cs[at + 1].is_ascii_alphabetic() {
        return None;
    }
    let mut k = at + 2;
    while k < cs.len() && (cs[k].is_ascii_alphanumeric() || cs[k] == '-') {
        k += 1;
    }
    let mut e = k;
    while e < cs.len() && cs[e] != '>' {
        e += 1;
    }
    if e < cs.len() {
        Some((k, e))
    } else {
        None
    }
}

/// 견본 기록을 읽어 보고서 HTML 글자를 만든다. 원본 `render(name:only:origin:)`: 기록의 `id`·`at`·`home` 칸을 쓰고, 시간대는 서울.
fn render(name: &str, only: Option<i64>, origin: ReportOrigin) -> String {
    let j = fixture(name);
    let Some(r) = report_run(j.get("id").and_then(Value::as_str).unwrap_or("?"), &FixtureSource::with_origin(j.clone(), origin)) else {
        return String::new();
    };
    report_html(&r, only, j.get("at").and_then(Value::as_f64).unwrap_or(0.0), &FixedZone::SEOUL, j.get("home").and_then(Value::as_str))
}

/// 골든과 견준다. 다르면 첫 달라지는 줄(300글자까지)로 멈춘다. Swift 의 `AP_REPORT_GOLDEN=write` 다시 쓰기는 옮기지 않았다
/// (사람이 보고 고른 출력이라 시험이 저절로 고치지 않는다는 원본 규칙을 그대로 둔 것이다).
fn golden_cmp(name: &str, html: &str) {
    let want = golden(name);
    if html == want {
        return;
    }
    let a: Vec<&str> = html.split('\n').collect();
    let b: Vec<&str> = want.split('\n').collect();
    let k = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let first = |ss: &[&str]| -> String { ss.get(k).map(|s| s.chars().take(300).collect::<String>()).unwrap_or_else(|| "(없음)".to_string()) };
    panic!("골든 {name} 와 다름: {}번째 줄\n  지금: {}\n  골든: {}", k + 1, first(&a), first(&b));
}

// MARK: 보고서 HTML 시험 — Swift ReportHTMLTests 를 같은 이름으로(이 파일의 `견본` 과 겹치지 않게 `html_` 을 앞에 붙였다)

#[test]
fn html_견본() {
    // @Test("견본 기록 실행 보고서 = 골든")
    golden_cmp("sample-report.html", &render("sample-run.json", None, ReportOrigin::Folder));
}

#[test]
fn html_단위() {
    // @Test("견본 기록 단위 보고서 = 골든")
    golden_cmp("sample-unit-2.html", &render("sample-run.json", Some(2), ReportOrigin::Folder));
}

#[test]
fn html_악성골든() {
    // @Test("악성 견본 = 골든")
    golden_cmp("evil-report.html", &render("evil-run.json", None, ReportOrigin::Folder));
}

#[test]
fn html_악성() {
    // @Test("악성 견본: 태그·속성은 틀의 것만, 링크는 https 만, 긴 줄표 없음, CSP")
    let h = render("evil-run.json", None, ReportOrigin::Folder);
    let bad = safety(&h);
    assert!(bad.is_empty(), "{bad:?}");
    assert!(h.contains("&lt;script&gt;alert(1)&lt;/script&gt; 실행, 줄표"));
    assert!(h.contains("<a href=\"https://example.com/ok?q=1&amp;r=2\" rel=\"noopener noreferrer\">"));
    assert!(!h.contains("href=\"https://evil.example") && !h.contains("href=\"http://"));
    assert!(!h.contains("user:pw@example.com/\""));
    assert!(h.contains("그리지 못해 원문을 보입니다(1번째 줄: sequenceDiagram 으로 시작하지 않습니다)"));
    assert!(!h.contains("/Users/toy/")); // 홈 폴더는 ~ 로
}

#[test]
fn html_견본안전() {
    // @Test("견본 기록도 같은 안전 검사를 지난다")
    for (html, note) in [
        (render("sample-run.json", None, ReportOrigin::Folder), "실행 보고서"),
        (render("sample-run.json", Some(1), ReportOrigin::Folder), "단위 보고서"),
        (render("sample-run.json", None, ReportOrigin::Records), "기록 저장소 바탕"),
    ] {
        let bad = safety(&html);
        assert!(bad.is_empty(), "{note}: {bad:?}");
    }
}

#[test]
fn html_결정적() {
    // @Test("같은 기록이면 같은 글자, 시간대만 바꾸면 시각만 다름")
    let a = render("sample-run.json", None, ReportOrigin::Folder);
    let b = render("sample-run.json", None, ReportOrigin::Folder);
    assert_eq!(a, b);
    let j = fixture("sample-run.json");
    let r = report_run("x", &FixtureSource::new(j)).unwrap();
    let utc = report_html(&r, None, 1_793_330_000.0, &FixedZone::UTC, None);
    assert!(utc.contains("2026년 10월 30일 00:00") && !utc.contains("2026년 10월 30일 09:00"));
}

#[test]
fn html_바탕() {
    // @Test("기록 저장소 바탕이면 바닥 글이 알려 준다, 단위가 없으면 그 말")
    assert!(render("sample-run.json", None, ReportOrigin::Records).contains("바탕 기록 저장소(실행 폴더를 지워 판정 출력은 없습니다)"));
    // Swift `var r = APReportRun(); r.id = "빈"`
    let r = ReportRun { id: "빈".into(), ..ReportRun::default() };
    let h = report_html(&r, None, 0.0, &FixedZone::SEOUL, None);
    assert!(h.contains("단위 기록이 없습니다"));
    assert!(!h.contains("<nav"));
    let bad = safety(&h);
    assert!(bad.is_empty(), "{bad:?}");
}

// ── 유스케이스: 보고서 그리기(가짜 어댑터) — Swift `ReportDrawTests` ──

/// 그리기 시계 — 받은 때를 그대로 돌려준다. 원본: `ReportClock`(`now: Date(timeIntervalSince1970: 1_793_330_000)`)
struct ReportClock {
    now: f64,
}

impl ReadClock for ReportClock {
    fn now(&self) -> f64 {
        self.now
    }
}

/// 원본 시계의 고정 때 — 2026-10-30 03:13:20 UTC(서울 12:13)
const DRAW_NOW: f64 = 1_793_330_000.0;

/// 가짜 쓸 곳 — `fail` 에 담긴 파일은 까닭("쓰지 못했습니다(가짜)")을 돌려주고, 나머지는 그린 글을 기록한다. 원본: `FakeSink`
#[derive(Default)]
struct FakeSink {
    fail: HashSet<ReportFile>,
    wrote: RefCell<HashMap<ReportFile, String>>,
}

impl ReportSink for FakeSink {
    fn write(&self, file: ReportFile, html: &str) -> Option<String> {
        if self.fail.contains(&file) {
            return Some("쓰지 못했습니다(가짜)".to_string());
        }
        self.wrote.borrow_mut().insert(file, html.to_string());
        None
    }
}

#[test]
fn draw_쓰기() {
    // @Test("고른 단위와 실행 보고서를 쓴다") — 단위를 먼저, 실행을 마지막에 쓴다
    let sink = FakeSink::default();
    let d = draw_reports(
        "20261030-0900-todo-api",
        &FixtureSource::new(fixture("sample-run.json")),
        &sink,
        &[2],
        &ReportClock { now: DRAW_NOW },
        &FixedZone::SEOUL,
        Some("/Users/toy"),
    );
    assert_eq!(d.written, vec![ReportFile::Unit(2), ReportFile::Run]);
    assert!(d.errors.is_empty(), "{d:?}");
    let unit = sink.wrote.borrow()[&ReportFile::Unit(2)].clone();
    assert!(unit.contains("<section id=\"unit-2\">"), "단위 보고서");
    assert!(!unit.contains("id=\"unit-1\""), "고른 단위만 그린다");
    let run = sink.wrote.borrow()[&ReportFile::Run].clone();
    assert!(run.contains("<section id=\"unit-4\">"), "실행 보고서");
    assert!(run.contains("그린 때 2026년 10월 30일 12:13"), "그린 때");
}

#[test]
fn draw_실패() {
    // @Test("쓸 곳이 실패하면 까닭을 남기고 나머지는 쓴다")
    let sink = FakeSink { fail: HashSet::from([ReportFile::Unit(1)]), ..Default::default() };
    let d = draw_reports("x", &FixtureSource::new(fixture("sample-run.json")), &sink, &[1, 9], &ReportClock { now: DRAW_NOW }, &FixedZone::SEOUL, None);
    assert_eq!(d.written, vec![ReportFile::Run]);
    assert_eq!(d.errors, ["단위 1 보고서: 쓰지 못했습니다(가짜)".to_string(), "대기열에 없는 단위(9)라 그 단위 보고서를 그리지 않았습니다".to_string()]);
}

#[test]
fn draw_없음() {
    // @Test("run.json 이 없으면 아무것도 쓰지 않는다")
    let sink = FakeSink::default();
    let d = draw_reports("x", &FixtureSource::new(json!({})), &sink, &[1], &ReportClock { now: DRAW_NOW }, &FixedZone::SEOUL, None);
    assert!(d.written.is_empty());
    assert!(sink.wrote.borrow().is_empty());
    assert_eq!(d.errors, ["실행 기록(run.json)을 읽지 못해 보고서를 그리지 않았습니다".to_string()]);
}
