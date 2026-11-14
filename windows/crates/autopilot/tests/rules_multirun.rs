//! 여러 실행 함께 돌리기 규칙(`rules::multirun`) 시험.
//! 원본: `Tests/AutopilotRulesTests/MultirunTests.swift` 의 `MultirunTests`(`@Suite("자율주행 여러 실행 함께 돌리기")`).
//! 옮긴 시험(같은 이름): `사용률`(표 6줄) · `자리크기` · `판정`(표 17줄) · `같은때` · `번갈아` · `다시적기` · `다시하기` · `겹침`(표 7줄) · `접기` ·
//! `살아있나`(표 12줄) · `출발` · `같은때출발`(Swift `withTaskGroup` → `std::thread::scope`). 같은 파일의 `RecordsV3Tests`(`판3`)는
//! `tests/rules_records.rs` 가 옮겼다(`판2_저장소를_판3으로_올리면_…`). `겹침` 은 `tests/rules_path_key.rs` 에도 같은 표로 있다(그쪽은 경로 키 시험의 일부).
//! 표(`usage_rows`·`slot_rows`·`OVERLAP_ROWS`·`alive_rows`)는 Swift 시험 파일에서 변환 스크립트로 기계가 옮긴 것이다(손으로 다시 치지 않았다): 글자는 그대로,
//! `hold("a", pp: 5, used: 3)` 는 `hold("a", 5.0, 3.0, T - 100.0, T - 10.0)`(기본값을 풀어 쓴 같은 뜻), `.toy("…")` 는 `UsageStep::Toy("…".to_string())`,
//! `v("running", exists: false, engine: true)` 는 `v(Some("running"), false, false, true, false, false)`(인자 순서 state·exists·wrapped·engine·job·starter).
//! Swift 시험의 `claim()` 은 키를 `apPathKey` 로 만든다. 여기는 엔진이 쓰는 `path_key_windows` 로 만든다(이 시험의 경로는 소문자 `/` 경로라 같다).
//! 끝의 `extra_` 시험은 Swift 시험에 없는 것(경계·Windows 판단)이다.

use claude_board_autopilot::ports::Clock;
use claude_board_autopilot::rules::multirun::{
    beat_slot, busy_retry_delay, claim_alive, claim_busy_text, claim_keys, claim_paths, path_overlap, pp_text, release_claims, release_slot,
    slot_decide, slot_left, slot_live, slot_size, take_slot, usage_steps, Claim, ClaimBook, ClaimChange, ClaimProbe, ClaimResult,
    ClaimRunView, Slot, SlotBook, SlotChange, SlotKind, UsageStep, BUSY_RETRIES, SLOT_RECHECK, SLOT_TTL,
};
use claude_board_autopilot::rules::path_key::{path_key, path_key_windows};
use claude_board_autopilot::rules::stopline::Tz;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

const T: f64 = 1_793_000_000.0;

struct MrClock(f64);
impl Clock for MrClock {
    fn now(&self) -> f64 {
        self.0
    }
    fn sleep(&self, _d: Duration) {}
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        other => panic!("객체가 아니다: {other}"),
    }
}

// ── 사용률 읽을 곳 ───────────────────────────────────

struct UsageRow {
    note: &'static str,
    dev: bool,
    toy: Option<&'static str>,
    want: Vec<UsageStep>,
}

// ── 멈출 선 자리 ─────────────────────────────────────

fn hold(run: &str, pp: f64, used: f64, since: f64, beat: f64) -> Slot {
    Slot {
        run: run.to_string(),
        project: Some(format!("zz-{run}")),
        unit: 1,
        kind: SlotKind::Hold,
        pp,
        used,
        pid: Some(100),
        since,
        beat,
    }
}

fn wait(run: &str, pp: f64, since: f64, beat: f64) -> Slot {
    Slot { run: run.to_string(), project: Some(format!("zz-{run}")), unit: 2, kind: SlotKind::Wait, pp, used: 0.0, pid: Some(101), since, beat }
}

struct SlotRow {
    note: &'static str,
    v7: Option<f64>, // None 이면 사용률을 모름
    line: f64,
    slots: Vec<Slot>,
    want: &'static str, // "go" · "wait:<until-T>:<글 앞부분>"
    put: &'static str,  // 넣는 줄 "<run>:<kind>" 쉼표
    drop: &'static [&'static str],
}

fn usage_rows() -> Vec<UsageRow> {
    vec![
        UsageRow { note: "설치판은 지금 그대로(stream → 캐시 → API)", dev: false, toy: None, want: vec![UsageStep::Stream, UsageStep::Cache, UsageStep::Api] },
        UsageRow { note: "설치판은 장난감 사용량 파일 덮어쓰기를 듣지 않는다", dev: false, toy: Some("/w/toy-usage.json"), want: vec![UsageStep::Stream, UsageStep::Cache, UsageStep::Api] },
        UsageRow { note: "설치판은 빈 덮어쓰기도 그대로", dev: false, toy: Some(""), want: vec![UsageStep::Stream, UsageStep::Cache, UsageStep::Api] },
        UsageRow { note: "시험 빌드에 장난감 파일이 있으면 그것을 먼저, stream 은 보조, 캐시·API 는 보지 않는다", dev: true, toy: Some("/w/toy-usage.json"), want: vec![UsageStep::Toy("/w/toy-usage.json".to_string()), UsageStep::Stream] },
        UsageRow { note: "시험 빌드에 장난감 파일이 없으면 stream → 캐시(읽기만), 사용량 API 로 떨어지지 않는다", dev: true, toy: None, want: vec![UsageStep::Stream, UsageStep::Cache] },
        UsageRow { note: "시험 빌드의 빈 덮어쓰기는 없는 것", dev: true, toy: Some(""), want: vec![UsageStep::Stream, UsageStep::Cache] },
    ]
}

fn slot_rows() -> Vec<SlotRow> {
    vec![
        SlotRow { note: "다른 실행이 없으면 6단계 전과 같이 지나가고 내 자리를 잡는다", v7: Some(0.25), line: 0.3, slots: vec![], want: "go", put: "me:hold", drop: &[] },
        SlotRow { note: "다른 실행이 없고 사용률만으로 닿으면 지금처럼 초기화까지(내 줄 없음)", v7: Some(0.3), line: 0.3, slots: vec![], want: "wait:150:계정 주간 사용률 30% 가", put: "", drop: &[] },
        SlotRow { note: "다른 실행의 자리를 더하면 닿아 기다린다(1분 뒤, 차례 줄)", v7: Some(0.27), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 10.0)], want: "wait:60:계정 주간 사용률 27% 에 다른 실행 1개가 잡아 둔 자리 5%p 를 더하면 멈출 선 30% 에 닿아", put: "me:wait", drop: &[] },
        SlotRow { note: "다른 실행의 자리를 더해도 닿지 않으면 시작", v7: Some(0.2), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 10.0)], want: "go", put: "me:hold", drop: &[] },
        SlotRow { note: "그 단위가 이미 쓴 만큼은 사용률에 들어 있어 빼고 더한다", v7: Some(0.27), line: 0.3, slots: vec![hold("a", 5.0, 3.0, T - 100.0, T - 10.0)], want: "go", put: "me:hold", drop: &[] },
        SlotRow { note: "다 쓴 자리는 0", v7: Some(0.29), line: 0.3, slots: vec![hold("a", 5.0, 9.0, T - 100.0, T - 10.0)], want: "go", put: "me:hold", drop: &[] },
        SlotRow { note: "두 실행의 자리를 함께 더한다", v7: Some(0.2), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 10.0), hold("b", 5.0, 0.0, T - 100.0, T - 10.0)], want: "wait:60:계정 주간 사용률 20% 에 다른 실행 2개가 잡아 둔 자리 10%p", put: "me:wait", drop: &[] },
        SlotRow { note: "죽은 엔진의 자리(15분 지남)는 보지 않고 지운다", v7: Some(0.27), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 900.0)], want: "go", put: "me:hold", drop: &["a"] },
        SlotRow { note: "14분 지난 자리는 아직 산 것", v7: Some(0.27), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 840.0)], want: "wait:60:계정 주간 사용률 27% 에", put: "me:wait", drop: &[] },
        SlotRow { note: "먼저 기다리던 실행에 차례를 넘긴다", v7: Some(0.27), line: 0.3, slots: vec![wait("a", 5.0, T - 50.0, T - 10.0)], want: "wait:60:먼저 기다리던 실행 1개(자리 5%p)에 차례를 넘겨", put: "me:wait", drop: &[] },
        SlotRow { note: "내가 먼저 기다렸으면 내가 간다", v7: Some(0.27), line: 0.3, slots: vec![wait("me", 5.0, T - 90.0, T - 10.0), wait("a", 5.0, T - 50.0, T - 10.0)], want: "go", put: "me:hold", drop: &[] },
        SlotRow { note: "같은 때 기다렸으면 실행 id 차례", v7: Some(0.27), line: 0.3, slots: vec![wait("me", 5.0, T - 50.0, T - 10.0), wait("a", 5.0, T - 50.0, T - 10.0)], want: "wait:60:먼저 기다리던 실행 1개", put: "me:wait@50", drop: &[] },
        SlotRow { note: "먼저 기다리던 실행이 있어도 둘 다 들어가면 함께 간다", v7: Some(0.1), line: 0.3, slots: vec![wait("a", 5.0, T - 50.0, T - 10.0)], want: "go", put: "me:hold", drop: &[] },
        SlotRow { note: "기다리던 차례는 그대로(since 를 지킨다)", v7: Some(0.27), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 10.0), wait("me", 5.0, T - 300.0, T - 10.0)], want: "wait:60:계정 주간 사용률 27% 에 다른 실행 1개", put: "me:wait@300", drop: &[] },
        SlotRow { note: "계정 사용률만으로 닿으면 내 기다림 줄을 지운다(모두 초기화까지)", v7: Some(0.31), line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 10.0), wait("me", 5.0, T - 300.0, T - 10.0)], want: "wait:150:계정 주간 사용률 31% 가", put: "", drop: &["me"] },
        SlotRow { note: "사용률을 모르면 10분 뒤(지금처럼)", v7: None, line: 0.3, slots: vec![hold("a", 5.0, 0.0, T - 100.0, T - 10.0)], want: "wait:600:계정 사용률을 알 수 없어", put: "", drop: &[] },
        SlotRow { note: "소수 자리 글자", v7: Some(0.2), line: 0.3, slots: vec![hold("a", 12.5, 2.4, T - 100.0, T - 10.0)], want: "wait:60:계정 주간 사용률 20% 에 다른 실행 1개가 잡아 둔 자리 10.1%p", put: "me:wait", drop: &[] },
    ]
}

// 잠금 하나로 줄을 지키는 가짜 자리 표 — 엔진의 BEGIN IMMEDIATE 처럼 update 는 한 번에 하나
struct FakeSlotBook {
    rows: Mutex<Vec<Slot>>,
    fail: AtomicBool,
}

impl FakeSlotBook {
    fn new(rows: Vec<Slot>) -> Self {
        FakeSlotBook { rows: Mutex::new(rows), fail: AtomicBool::new(false) }
    }
    fn snapshot(&self) -> Vec<Slot> {
        self.rows.lock().unwrap().clone()
    }
}

impl SlotBook for FakeSlotBook {
    fn update(&self, body: &mut dyn FnMut(&[Slot]) -> SlotChange) -> Result<(), String> {
        if self.fail.load(Ordering::SeqCst) {
            return Err("database is locked".to_string());
        }
        let mut rows = self.rows.lock().unwrap();
        let c = body(&rows);
        rows.retain(|r| !c.drop.contains(&r.run));
        for p in c.put {
            rows.retain(|r| r.run != p.run);
            rows.push(p);
        }
        Ok(())
    }
}

// ── 같은 경로 ────────────────────────────────────────

// 잠금 하나로 줄을 지키는 가짜 경로 자리 표. 같은 키를 넣으면 Err(UNIQUE) — 저장소 `path_claims(key)` 의 유일 제약
struct FakeClaimBook {
    rows: Mutex<Vec<Claim>>,
}

impl FakeClaimBook {
    fn new(rows: Vec<Claim>) -> Self {
        FakeClaimBook { rows: Mutex::new(rows) }
    }
    fn snapshot(&self) -> Vec<Claim> {
        self.rows.lock().unwrap().clone()
    }
}

impl ClaimBook for FakeClaimBook {
    fn update(&self, body: &mut dyn FnMut(&[Claim]) -> ClaimChange) -> Result<(), String> {
        let mut rows = self.rows.lock().unwrap();
        let c = body(&rows);
        rows.retain(|r| !c.drop.contains(&r.run));
        for p in c.put {
            if rows.iter().any(|r| r.key == p.key) {
                return Err("UNIQUE constraint failed".to_string());
            }
            rows.push(p);
        }
        Ok(())
    }
}

struct FakeProbe {
    views: Vec<(&'static str, ClaimRunView)>,
}

impl ClaimProbe for FakeProbe {
    fn view(&self, c: &Claim) -> ClaimRunView {
        self.views.iter().find(|(r, _)| *r == c.run).map_or_else(|| v(None, false, false, false, false, false), |(_, x)| x.clone())
    }
}

// Swift 시험은 `apPathKey(path)` 로 키를 만든다. 엔진이 쓰는 키(`path_key_windows`)로 만든다 — 이 시험의 경로는 모두 소문자 `/` 경로라 둘이 같다
fn claim(run: &str, path: &str) -> Claim {
    Claim {
        key: path_key_windows(path),
        path: path.to_string(),
        run: run.to_string(),
        run_dir: format!("/r/runs/{run}"),
        label: format!("com.claude.board.dev.autopilot.{run}"),
        pid: Some(4242),
        claimed: T - 60.0,
    }
}

fn v(state: Option<&str>, exists: bool, wrapped: bool, engine: bool, job: bool, starter: bool) -> ClaimRunView {
    ClaimRunView {
        exists,
        state: state.map(str::to_string),
        wrapped,
        engine_alive: engine,
        job_loaded: job,
        starter_alive: starter,
    }
}

fn sv(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

fn claim_at(
    run: &str,
    pid: i32,
    paths: &[&str],
    home: &str,
    book: &FakeClaimBook,
    probe: &FakeProbe,
) -> ClaimResult {
    claim_paths(run, &format!("/r/runs/{run}"), "l", Some(pid), &sv(paths), home, book, probe, &MrClock(T))
}

// BEGIN converted overlapRows
const OVERLAP_ROWS: &[(&str, &str, &str, bool)] = &[
    ("같은 저장소", "/w/zz-toy-a", "/w/zz-toy-a", true),
    ("다른 저장소", "/w/zz-toy-a", "/w/zz-toy-b", false),
    ("이름 앞부분만 같으면 겹치지 않는다", "/w/zz-toy", "/w/zz-toy-b", false),
    ("저장소를 품은 폴더", "/w/shop", "/w/shop/shop-core", true),
    ("품은 쪽이 뒤", "/w/shop/shop-web", "/w/shop", true),
    ("홈 폴더는 품은 자리로 보지 않는다", "/users/zz", "/users/zz/w/zz-toy-a", false),
    ("홈 폴더끼리는 같다", "/users/zz", "/users/zz", true),
];
// END converted overlapRows

// BEGIN converted aliveRows
// (note, v(state, exists, wrapped, engine, job, starter), want) — Swift `v(state, exists:, wrapped:, engine:, job:, starter:)` 의 기본값(exists true, 나머지 false)을 풀어 쓴 같은 뜻
fn alive_rows() -> Vec<(&'static str, ClaimRunView, bool)> {
    vec![
        ("실행 폴더가 없으면 남은 자리", v(Some("running"), false, false, true, false, false), false),
        ("출발 명령이 상태를 쓰기 전: 그 명령이 도는 동안 산 것", v(None, true, false, false, false, true), true),
        ("출발 명령이 상태를 쓰지 못하고 끝났으면 남은 자리", v(None, true, false, false, false, false), false),
        ("starting: LaunchAgent 가 실렸으면 산 것", v(Some("starting"), true, false, false, true, false), true),
        ("starting: 출발 명령이 아직 돌면 산 것", v(Some("starting"), true, false, false, false, true), true),
        ("도는 실행: 엔진이 살아 있음", v(Some("running"), true, false, true, false, false), true),
        ("도는 실행: 엔진이 죽어도 LaunchAgent 가 다시 띄움", v(Some("waiting_stopline"), true, false, false, true, false), true),
        ("도는 상태인데 엔진도 LaunchAgent 도 없으면 남은 자리", v(Some("running"), true, false, false, false, false), false),
        ("일시정지도 엔진이 살아 있으면 산 것", v(Some("paused"), true, false, true, true, false), true),
        ("끝났지만 정리 중(가지 되돌리기)은 엔진이 살아 있으면 산 것", v(Some("done"), true, false, true, false, false), true),
        ("끝나고 정리를 마쳤으면 남은 자리", v(Some("stopped"), true, true, true, true, false), false),
        ("끝났고 엔진이 죽었으면 남은 자리", v(Some("error"), true, false, false, false, false), false),
    ]
}
// END converted aliveRows

// ── 시험 ─────────────────────────────────────────────

#[test]
fn 사용률() {
    let rows = usage_rows();
    assert_eq!(rows.len(), 6, "표 줄이 빠졌다");
    for r in rows {
        assert_eq!(usage_steps(r.dev, r.toy), r.want, "{}", r.note);
    }
}

#[test]
fn 자리크기() {
    assert_eq!(slot_size(None, 5.0), 5.0);
    assert_eq!(slot_size(Some(2.5), 5.0), 2.5);
    assert_eq!(slot_size(Some(9.0), 5.0), 5.0); // 단위 상한을 넘지 않는다
    assert_eq!(slot_size(Some(0.0), 5.0), 5.0); // 추정 0 은 없는 것
    assert_eq!(slot_left(&hold("a", 5.0, 7.0, T - 100.0, T - 10.0)), 0.0);
    assert!(pp_text(5.0) == "5" && pp_text(2.46) == "2.5" && pp_text(0.04) == "0");
}

#[test]
fn 판정() {
    let rows = slot_rows();
    assert_eq!(rows.len(), 17, "표 줄이 빠졌다");
    for r in rows {
        let usage = r.v7.map(|v| obj(json!({"seven_day": v, "seven_day_resets": T + 30.0, "five_hour": 0.1})));
        let stopline = obj(json!({"seven_day": r.line}));
        let g = slot_decide("me", Some("zz-me"), 3, 5.0, Some(7), usage.as_ref(), &stopline, &r.slots, T, Tz::Utc);
        let got = g.wait.as_ref().map_or("go".to_string(), |w| format!("wait:{}:{}", (w.until - T) as i64, w.reason));
        if r.want == "go" {
            assert_eq!(got, "go", "{}", r.note);
        } else {
            assert!(got.starts_with(r.want), "{}: {got}", r.note);
        }
        let put = g
            .change
            .put
            .iter()
            .map(|p| {
                format!("{}:{}", p.run, p.kind.as_str()) + &if p.kind == SlotKind::Wait && p.since != T { format!("@{}", (T - p.since) as i64) } else { String::new() }
            })
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(put, r.put, "{}", r.note);
        assert_eq!(g.change.drop, r.drop.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{}", r.note);
        if let Some(p) = g.change.put.first() {
            assert!(p.unit == 3 && p.pp == 5.0 && p.beat == T && p.pid == Some(7), "{}", r.note);
        }
        assert!(!g.wait.as_ref().is_some_and(|w| w.reason.contains('—')), "{}", r.note);
        assert_eq!(g.by_others, r.want.starts_with("wait:60:"), "{}", r.note); // 다른 실행 때문에 기다릴 때만(1분마다 다시 봄)
    }
}

#[test]
fn 같은때() {
    for _ in 0..20 {
        let book = FakeSlotBook::new(Vec::new());
        let results: Vec<(String, bool)> = std::thread::scope(|s| {
            let handles: Vec<_> = ["a", "b"]
                .into_iter()
                .map(|me| {
                    let book = &book;
                    s.spawn(move || {
                        let usage = obj(json!({"seven_day": 0.27, "seven_day_resets": T + 3600.0}));
                        let r = take_slot(me, None, 1, 5.0, None, Some(&usage), &obj(json!({"seven_day": 0.3})), book, &MrClock(T), Tz::Utc);
                        (me.to_string(), r.wait.is_none())
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(results.iter().filter(|x| x.1).count(), 1);
        let rows = book.snapshot();
        assert!(rows.iter().filter(|r| r.kind == SlotKind::Hold).count() == 1 && rows.iter().filter(|r| r.kind == SlotKind::Wait).count() == 1);
    }
}

#[test]
fn 번갈아() {
    let book = FakeSlotBook::new(Vec::new());
    let usage = obj(json!({"seven_day": 0.27, "seven_day_resets": T + 9000.0}));
    let line = obj(json!({"seven_day": 0.3}));
    let take = |me: &str, t: f64| take_slot(me, None, 1, 5.0, None, Some(&usage), &line, &book, &MrClock(t), Tz::Utc).wait.is_none();
    assert!(take("a", T));
    assert!(!take("b", T + 1.0)); // a 의 자리 때문에 기다림
    assert!(release_slot("a", &book).is_ok()); // a 의 단위 끝
    assert!(!take("a", T + 2.0)); // b 가 먼저 기다렸다
    assert!(take("b", T + 61.0)); // b 가 다시 보면 시작
    assert!(release_slot("b", &book).is_ok());
    assert!(!take("b", T + 70.0)); // 이번엔 a 가 먼저 기다렸다
    assert!(take("a", T + 62.0 + 60.0));
}

#[test]
fn 다시적기() {
    let book = FakeSlotBook::new(vec![hold("old", 5.0, 0.0, T - 100.0, T - 2000.0)]);
    beat_slot("me", Some("zz"), 2, 4.0, 1.5, Some(9), &book, &MrClock(T)).unwrap();
    let rows = book.snapshot();
    assert_eq!(rows.iter().map(|r| r.run.as_str()).collect::<Vec<_>>(), ["me"]); // 죽은 엔진의 자리를 함께 지움
    assert!(rows[0].used == 1.5 && rows[0].kind == SlotKind::Hold);
    beat_slot("me", Some("zz"), 2, 4.0, 2.0, Some(9), &book, &MrClock(T + 60.0)).unwrap();
    let rows = book.snapshot();
    assert!(rows[0].since == T && rows[0].beat == T + 60.0); // 잡은 때는 그대로
    release_slot("me", &book).unwrap();
    assert!(book.snapshot().is_empty());
    book.fail.store(true, Ordering::SeqCst);
    let r = take_slot("me", None, 1, 5.0, None, Some(&obj(json!({"seven_day": 0.1}))), &Map::new(), &book, &MrClock(T), Tz::Utc);
    assert!(r.wait.is_none() && r.note.as_deref().is_some_and(|n| n.contains("사용률만으로")));
}

#[test]
fn 다시하기() {
    let all: Vec<f64> = (1..=BUSY_RETRIES).filter_map(busy_retry_delay).collect();
    assert_eq!(all, [0.1, 0.2, 0.4, 0.8, 1.0, 1.0, 1.0, 1.0]);
    assert!(busy_retry_delay(0).is_none() && busy_retry_delay(BUSY_RETRIES + 1).is_none());
}

#[test]
fn 겹침() {
    assert_eq!(OVERLAP_ROWS.len(), 7);
    for (note, a, b, want) in OVERLAP_ROWS {
        assert_eq!(path_overlap(&path_key(a), &path_key(b), "/Users/zz"), *want, "{note}: {a} vs {b}");
    }
}

#[test]
fn 접기() {
    let nfd: String = "/w/zz-토이".nfd().collect();
    let k = claim_keys(&sv(&["/w/ZZ-Toy-A", "/w/zz-toy-a/", &nfd, "/w/zz-토이", "rel/x", ""]));
    let nfc: String = "/w/zz-토이".nfc().collect();
    assert_eq!(k.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["/w/zz-toy-a", nfc.as_str()]);
    assert_eq!(k.first().map(|x| x.1.as_str()), Some("/w/ZZ-Toy-A"));
}

#[test]
fn 살아있나() {
    let rows = alive_rows();
    assert_eq!(rows.len(), 12, "표 줄이 빠졌다");
    for (note, view, want) in rows {
        assert_eq!(claim_alive(&view), want, "{note}");
    }
}

#[test]
fn 출발() {
    let home = "/Users/zz";
    let book = FakeClaimBook::new(vec![
        claim("a", "/w/zz-toy-a"),
        claim("dead", "/w/zz-toy-b"),
        claim("shop", "/w/shop/shop-core"),
        claim("shop", "/w/shop/shop-web"),
    ]);
    let probe = FakeProbe {
        views: vec![
            ("a", v(Some("running"), true, false, true, false, false)),
            ("dead", v(Some("running"), true, false, false, false, false)),
            ("shop", v(Some("waiting_limit"), true, false, false, true, false)),
        ],
    };
    // 대소문자를 바꿔 적은 같은 경로
    let r1 = claim_at("x", 1, &["/w/ZZ-TOY-A"], home, &book, &probe);
    assert_eq!(r1, ClaimResult::Busy { run: "a".into(), path: "/w/zz-toy-a".into(), mine: "/w/ZZ-TOY-A".into() });
    assert_eq!(
        claim_busy_text(&r1, home).as_deref(),
        Some("이 경로에서 자율주행이 돌고 있습니다(a, /w/zz-toy-a). 그 실행이 끝난 뒤에 출발합니다")
    );
    assert!(!book.snapshot().iter().any(|c| c.run == "x"));
    // 엔진도 작업도 없는 실행의 남은 자리는 치우고 잡는다
    let r2 = claim_at("y", 2, &["/w/zz-toy-b"], home, &book, &probe);
    assert_eq!(r2, ClaimResult::Ok { cleared: vec!["dead".into()] });
    let rows = book.snapshot();
    assert!(rows.iter().filter(|c| c.run == "y").map(|c| c.path.as_str()).collect::<Vec<_>>() == ["/w/zz-toy-b"] && !rows.iter().any(|c| c.run == "dead"));
    // 묶음의 저장소 하나만 겹쳐도 거부, 묶음을 품은 폴더도 거부
    let r3 = claim_at("z", 3, &["/w/zz-toy-c", "/w/shop/shop-web"], home, &book, &probe);
    assert_eq!(r3, ClaimResult::Busy { run: "shop".into(), path: "/w/shop/shop-web".into(), mine: "/w/shop/shop-web".into() });
    let r4 = claim_at("z", 3, &["/w/shop"], home, &book, &probe);
    match &r4 {
        ClaimResult::Busy { run: other, .. } => assert_eq!(other, "shop"),
        other => panic!("품은 폴더를 받았다: {other:?}"),
    }
    // 겹치지 않는 경로는 함께
    let r5 = claim_at("z", 3, &["/w/zz-toy-c"], home, &book, &probe);
    assert_eq!(r5, ClaimResult::Ok { cleared: vec![] });
    // 끝 정리를 마치면 푼다
    assert!(release_claims("a", &book).is_ok());
    let r6 = claim_at("x", 1, &["/w/zz-toy-a"], home, &book, &probe);
    assert_eq!(r6, ClaimResult::Ok { cleared: vec![] });
}

#[test]
fn 같은때출발() {
    for _ in 0..20 {
        let book = FakeClaimBook::new(vec![]);
        // 상태를 아직 쓰지 않은 실행 — 출발 명령이 도는 중이라 산 것
        let probe = FakeProbe { views: vec![("p", v(None, true, false, false, false, true)), ("q", v(None, true, false, false, false, true))] };
        let got: Vec<ClaimResult> = std::thread::scope(|s| {
            let hs: Vec<_> = ["p", "q"]
                .into_iter()
                .map(|id| {
                    let (book, probe) = (&book, &probe);
                    s.spawn(move || {
                        claim_paths(id, &format!("/r/runs/{id}"), "l", Some(5), &sv(&["/w/zz-toy-a"]), "/Users/zz", book, probe, &MrClock(T))
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(got.iter().filter(|r| matches!(r, ClaimResult::Ok { .. })).count(), 1);
        assert_eq!(got.iter().filter(|r| matches!(r, ClaimResult::Busy { .. })).count(), 1);
        assert_eq!(book.snapshot().len(), 1);
    }
}

// ── 아래는 Swift 시험에 없는 것 ──────────────────────

#[test]
fn extra_자리_산_경계() {
    let at = |beat: f64| slot_live(&hold("a", 5.0, 0.0, T - 100.0, beat), T);
    assert!(at(T - SLOT_TTL + 1.0));
    assert!(!at(T - SLOT_TTL)); // 정확히 15분 지나면 죽은 것
    assert!(at(T + 60.0)); // 시계가 조금 앞선 쪽은 산 것
    assert!(!at(T + 61.0)); // 너무 앞선 쪽(시계가 틀린 자리)은 죽은 것
    assert_eq!(SLOT_RECHECK, 60.0);
}

#[test]
fn extra_퍼센트_글자_경계() {
    assert_eq!(pp_text(-0.04), "0"); // 음의 0 도 "0"
    assert_eq!(pp_text(12.5), "12.5");
    assert_eq!(pp_text(10.04), "10");
    assert_eq!(pp_text(10.06), "10.1");
    assert_eq!(slot_size(Some(-1.0), 5.0), 5.0); // 음수 추정은 없는 것
    assert_eq!(slot_size(Some(5.0), 5.0), 5.0);
    assert_eq!(slot_left(&hold("a", 5.0, 0.0, T, T)), 5.0);
}

#[test]
fn extra_사용률이_글자여도_같게_판정한다() {
    // run.json usage 값이 글자("0.27")여도 수로 읽는다(`stop_num`) — 표 3번째 줄과 같은 결과
    let usage = obj(json!({"seven_day": "0.27", "seven_day_resets": T + 30.0}));
    let g = slot_decide("me", None, 1, 5.0, None, Some(&usage), &obj(json!({"seven_day": 0.3})), &[hold("a", 5.0, 0.0, T - 100.0, T - 10.0)], T, Tz::Utc);
    assert!(g.by_others && g.wait.is_some_and(|w| w.reason.contains("27%")));
    assert_eq!((g.others, g.held), (1, 5.0));
}

#[test]
fn extra_초기화가_지난_창은_0으로_보고_자리를_더하지_않는다() {
    // 주간 초기화 시각이 지났으면 사용률은 0 — 다른 실행의 자리 5%p 를 더해도 멈출 선 30% 에 닿지 않는다
    let usage = obj(json!({"seven_day": 0.9, "seven_day_resets": T - 1.0}));
    let g = slot_decide("me", None, 1, 5.0, None, Some(&usage), &obj(json!({"seven_day": 0.3})), &[hold("a", 5.0, 0.0, T - 100.0, T - 10.0)], T, Tz::Utc);
    assert!(g.wait.is_none());
    assert_eq!(g.change.put.len(), 1);
}

#[test]
fn extra_다른_실행_자리_때문이_아니면_by_others_는_거짓이다() {
    let usage = obj(json!({"seven_day": 0.31, "seven_day_resets": T + 30.0}));
    let g = slot_decide("me", None, 1, 5.0, None, Some(&usage), &obj(json!({"seven_day": 0.3})), &[], T, Tz::Utc);
    assert!(g.wait.is_some() && !g.by_others);
}

#[test]
fn extra_실행_id_는_글자_그대로_견준다_윈도_대소문자_무시_파일_시스템에서도() {
    // 윈도의 실행 폴더는 대소문자를 가리지 않아 `A` 와 `a` 가 한 폴더로 풀리지만, 실행 id 는 엔진이 소문자 ASCII 로 만든다.
    // 자리 표의 `me` 견줌은 글자 그대로라 `A` 의 줄은 `a` 에게 다른 실행의 줄이다(같은 실행을 둘로 세지 않는 쪽으로는 기울지 않는다).
    let slots = [hold("A", 5.0, 0.0, T - 100.0, T - 10.0)];
    let g = slot_decide("a", None, 1, 5.0, None, Some(&obj(json!({"seven_day": 0.27, "seven_day_resets": T + 30.0}))), &obj(json!({"seven_day": 0.3})), &slots, T, Tz::Utc);
    assert_eq!(g.others, 1);
    assert!(g.by_others);
}

#[test]
fn extra_자리_종류_글자는_표의_kind_칸과_같다() {
    assert_eq!((SlotKind::Hold.as_str(), SlotKind::Wait.as_str()), ("hold", "wait"));
}

#[test]
fn extra_죽은_자리는_내_것이면_지우지_않고_덮어쓴다() {
    // 내 줄이 죽은 것으로 보여도(beat 가 오래됨) drop 에 넣지 않는다 — put 이 같은 실행의 줄을 바꾼다
    let slots = [hold("me", 5.0, 0.0, T - 5000.0, T - 4000.0), hold("old", 5.0, 0.0, T - 5000.0, T - 4000.0)];
    let g = slot_decide("me", None, 1, 5.0, None, Some(&obj(json!({"seven_day": 0.1, "seven_day_resets": T + 30.0}))), &obj(json!({"seven_day": 0.3})), &slots, T, Tz::Utc);
    assert_eq!(g.change.drop, ["old"]);
    assert_eq!(g.change.put.len(), 1);
}

#[test]
fn extra_먼저_기다리던_실행의_자리는_잡아_둔_자리와_함께_더한다() {
    // 사용률 20% + 잡아 둔 5%p = 25%(선 30% 아래)지만 먼저 기다리던 5%p 까지 더하면 30% 에 닿아 차례를 넘긴다
    let slots = [hold("a", 5.0, 0.0, T - 100.0, T - 10.0), wait("b", 5.0, T - 50.0, T - 10.0)];
    let usage = obj(json!({"seven_day": 0.2, "seven_day_resets": T + 30.0}));
    let g = slot_decide("me", None, 1, 5.0, None, Some(&usage), &obj(json!({"seven_day": 0.3})), &slots, T, Tz::Utc);
    assert!(g.by_others);
    assert!(g.wait.is_some_and(|w| w.reason.starts_with("먼저 기다리던 실행 1개(자리 5%p)")));
}

#[test]
fn extra_기다리다_들어가면_잡은_때는_지금이다() {
    // 기다림 줄의 since(차례)는 hold 로 바뀔 때 지금으로 다시 적는다 — 단위가 도는 시간의 시작이다
    let slots = [wait("me", 5.0, T - 300.0, T - 10.0)];
    let usage = obj(json!({"seven_day": 0.1, "seven_day_resets": T + 30.0}));
    let g = slot_decide("me", None, 1, 5.0, None, Some(&usage), &obj(json!({"seven_day": 0.3})), &slots, T, Tz::Utc);
    assert!(g.wait.is_none());
    assert_eq!((g.change.put[0].kind, g.change.put[0].since, g.change.put[0].beat), (SlotKind::Hold, T, T));
}

// ── 아래는 Swift 시험에 없는 것(③ 같은 경로 자리, Windows 판단) ──

#[test]
fn extra_자리_줄은_windows_쓰기를_한_키로_모은다() {
    let k = claim_keys(&sv(&[r"C:\Work\Toy-A", "c:/work/toy-a/", r"\\?\C:\WORK\TOY-A", r"D:\x", "C:foo", r"rel\x", "", "x:"]));
    assert_eq!(k, vec![("c:/work/toy-a".to_string(), r"C:\Work\Toy-A".to_string()), ("d:/x".to_string(), r"D:\x".to_string())]);
    // 끝 구분자가 있는 글자는 path 에 그대로 남는다
    let k = claim_keys(&sv(&[r"C:\Work\Toy-A\"]));
    assert_eq!(k, vec![("c:/work/toy-a".to_string(), r"C:\Work\Toy-A\".to_string())]);
    // UNC 와 `\` 로 시작하는 글자는 절대로 받는다
    let k = claim_keys(&sv(&[r"\\srv\Share\a", r"\w\x"]));
    assert_eq!(k.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["//srv/share/a", "/w/x"]);
}

#[test]
fn extra_다른_쓰기의_같은_경로는_거부하고_다른_드라이브는_함께() {
    let home = r"C:\Users\zz";
    let keys = claim_keys(&sv(&[r"C:\Work\Toy-A"]));
    let mut a = claim("a", "/x");
    (a.key, a.path) = keys[0].clone();
    let book = FakeClaimBook::new(vec![a]);
    let probe = FakeProbe { views: vec![("a", v(Some("running"), true, false, true, false, false))] };
    for p in [r"\\?\c:\work\TOY-A\", r"c:/Work/./Toy-A", r"C:\Work\Other\..\Toy-A", r"C:\Work", r"C:\Work\Toy-A\sub"] {
        match claim_at("x", 1, &[p], home, &book, &probe) {
            ClaimResult::Busy { run, path, mine } => assert_eq!((run.as_str(), path.as_str(), mine.as_str()), ("a", r"C:\Work\Toy-A", p)),
            other => panic!("{p}: {other:?}"),
        }
    }
    for p in [r"C:\Work\Toy", r"C:\Work\Toy-B", r"D:\Work\Toy-A"] {
        assert!(matches!(claim_at("y", 2, &[p], home, &book, &probe), ClaimResult::Ok { .. }), "{p}");
    }
}

#[test]
fn extra_홈_폴더는_품은_자리로_보지_않는다_홈도_같은_키로_접는다() {
    let home = r"C:\Users\zz\"; // 끝 `\` 가 있어도 같은 홈
    let book = FakeClaimBook::new(vec![]);
    let probe = FakeProbe { views: vec![("h", v(Some("running"), true, false, true, false, false))] };
    assert!(matches!(claim_at("h", 1, &[r"c:\USERS\ZZ"], home, &book, &probe), ClaimResult::Ok { .. }));
    // 홈을 잡은 실행이 있어도 홈 안쪽 저장소는 함께 돈다
    assert!(matches!(claim_at("w", 2, &[r"C:\Users\zz\w\toy-a"], home, &book, &probe), ClaimResult::Ok { .. }));
    // 같은 홈을 또 잡으면 같으므로 거부
    assert!(matches!(claim_at("x", 3, &[r"\\?\C:\Users\ZZ"], home, &book, &probe), ClaimResult::Busy { .. }));
    assert!(path_overlap(&path_key_windows(r"C:\USERS\zz\"), &path_key_windows(r"\\?\C:\Users\ZZ"), home));
    assert!(!path_overlap("c:/users/zz", "c:/users/zz/w/toy-a", home));
}

#[test]
fn extra_뿌리는_모든_아래를_품는다() {
    assert!(path_overlap("/", "/w", "/Users/zz"));
    assert!(path_overlap("/w", "/", "/Users/zz"));
    assert!(path_overlap("c:", "c:/w/x", r"C:\Users\zz"));
    assert!(!path_overlap("c:", "d:/w/x", r"C:\Users\zz"));
    assert!(path_overlap("//", "//srv/share/a", r"C:\Users\zz"));
    assert!(!path_overlap("c:/w", "c:/wx", r"C:\Users\zz"));
    assert!(!path_overlap("c:/w/x", "c:/w/y", r"C:\Users\zz"));
}

#[test]
fn extra_자기_줄은_다시_잡을_때_지우고_다시_넣는다() {
    let book = FakeClaimBook::new(vec![claim("x", "/w/old")]);
    let probe = FakeProbe { views: vec![] };
    assert_eq!(claim_at("x", 9, &["/w/new"], "/Users/zz", &book, &probe), ClaimResult::Ok { cleared: vec![] });
    let rows = book.snapshot();
    assert_eq!(rows.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["/w/new"]);
    assert_eq!((rows[0].pid, rows[0].claimed, rows[0].run_dir.as_str(), rows[0].label.as_str()), (Some(9), T, "/r/runs/x", "l"));
}

#[test]
fn extra_표를_못_쓰면_출발하지_않는다_닫힌_쪽() {
    // 표 자체가 거부하는 경우(저장소 잠김)
    struct Locked;
    impl ClaimBook for Locked {
        fn update(&self, _b: &mut dyn FnMut(&[Claim]) -> ClaimChange) -> Result<(), String> {
            Err("database is locked".to_string())
        }
    }
    let r = claim_paths("x", "/r/runs/x", "l", None, &sv(&["/w/a"]), "/Users/zz", &Locked, &FakeProbe { views: vec![] }, &MrClock(T));
    assert_eq!(r, ClaimResult::Failed("database is locked".to_string()));
    assert!(release_claims("x", &Locked).is_err());
}

#[test]
fn extra_거부_글은_홈_아래를_줄이고_거부가_아니면_없다() {
    let busy = |p: &str| ClaimResult::Busy { run: "a".into(), path: p.to_string(), mine: "m".into() };
    let t = |p: &str, home: &str| claim_busy_text(&busy(p), home).unwrap();
    let text = |shown: &str| format!("이 경로에서 자율주행이 돌고 있습니다(a, {shown}). 그 실행이 끝난 뒤에 출발합니다");
    assert_eq!(t("/Users/zz/w/toy", "/Users/zz"), text("~/w/toy"));
    assert_eq!(t(r"C:\Users\ZZ\w\toy", r"c:\users\zz\"), text("~/w/toy"));
    assert_eq!(t(r"C:\Users\zz", r"C:\Users\zz"), text(r"C:\Users\zz")); // 홈 그 자체는 줄이지 않는다
    assert_eq!(t(r"C:\Users\zzz\w", r"C:\Users\zz"), text(r"C:\Users\zzz\w")); // 이름 앞부분만 같은 폴더
    assert_eq!(t(r"D:\w", r"C:\Users\zz"), text(r"D:\w"));
    assert_eq!(t("/w/x", ""), text("/w/x"));
    assert!(claim_busy_text(&ClaimResult::Ok { cleared: vec![] }, "/h").is_none());
    assert!(claim_busy_text(&ClaimResult::Failed("x".into()), "/h").is_none());
}

#[test]
fn extra_빈_상태_글은_상태를_아직_안_쓴_것과_같다() {
    assert!(claim_alive(&v(Some(""), true, false, true, true, true)));
    assert!(!claim_alive(&v(Some(""), true, false, true, true, false)));
    assert!(!claim_alive(&v(None, false, false, true, true, true)));
}

#[test]
fn extra_다시하기_간격은_범위_밖에서_없다() {
    assert!(busy_retry_delay(-1).is_none() && busy_retry_delay(i64::MIN).is_none() && busy_retry_delay(i64::MAX).is_none());
    assert_eq!(busy_retry_delay(5), Some(1.0));
}

#[test]
fn extra_출발_명령만_산_도는_상태는_남은_자리다() {
    // 엔진도 작업도 없이 출발 명령만 도는 경우: 상태를 아직 안 썼을 때(None)만 산 것이고, 상태가 있으면 starting 일 때만 산 것이다
    assert!(!claim_alive(&v(Some("running"), true, false, false, false, true)));
    assert!(!claim_alive(&v(Some("paused"), true, false, false, false, true)));
    assert!(!claim_alive(&v(Some("starting"), true, false, false, false, false)));
}

#[test]
fn extra_내_실행의_옛_줄은_거부하지_않고_치운다() {
    // 자기 자신의 줄은 살아 있어도 겹침으로 세지 않는다(다시 출발할 때 옛 줄을 바꿔 넣는다)
    let book = FakeClaimBook::new(vec![claim("x", "/w/a")]);
    let probe = FakeProbe { views: vec![("x", v(Some("running"), true, false, true, true, true))] };
    assert_eq!(claim_at("x", 2, &["/w/a"], "/Users/zz", &book, &probe), ClaimResult::Ok { cleared: vec![] });
    assert_eq!(book.snapshot().iter().map(|c| (c.run.as_str(), c.pid)).collect::<Vec<_>>(), [("x", Some(2))]);
}

#[test]
fn extra_남은_자리가_여러_줄이어도_한_번만_보고_한_번만_치운다() {
    // 죽은 실행 d 의 두 줄이 모두 겹친다 — 첫 줄의 판정을 둘째 줄에 다시 쓰고, 치운 실행 id 는 하나다
    let book = FakeClaimBook::new(vec![claim("d", "/w/a"), claim("d", "/w/a/b")]);
    let probe = FakeProbe { views: vec![] };
    assert_eq!(claim_at("x", 1, &["/w/a"], "/Users/zz", &book, &probe), ClaimResult::Ok { cleared: vec!["d".into()] });
    assert_eq!(book.snapshot().iter().map(|c| c.run.as_str()).collect::<Vec<_>>(), ["x"]);
    // 살아 있는 실행의 두 줄은 첫 겹침에서 거부(둘째 줄을 보지 않는다)
    let book = FakeClaimBook::new(vec![claim("d", "/w/a"), claim("d", "/w/a/b")]);
    let probe = FakeProbe { views: vec![("d", v(Some("running"), true, false, true, false, false))] };
    assert_eq!(
        claim_at("x", 1, &["/w/a"], "/Users/zz", &book, &probe),
        ClaimResult::Busy { run: "d".into(), path: "/w/a".into(), mine: "/w/a".into() }
    );
}
