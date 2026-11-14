//! 보드 멈출 선 자리 표(`rules::slot_table`) 시험.
//! 원본: `Tests/AutopilotRulesTests/SlotTableTests.swift` 의 `SlotTableTests`(`@Suite("자율주행 보드 멈출 선 자리 표")`).
//! 옮긴 시험(같은 이름): `표`(표 9줄, Swift `arguments: tableRows`) · `경로` · `글자`.
//! 자리 줄은 엔진이 쓰는 꼴(`Slot`·`Claim`) 그대로 넣고, 줄 차례·상태·기다림 차례·보일지·요약을 본다. 시각은 장난감 값(`T` = 10_000).
//! Swift 시험의 `slot(run, kind, unit: 1, pp: 5, used: 0, since:, beat: T - 30)` 은 기본값을 풀어 쓴 `slot(run, kind, pp, used, since, beat)`
//! (단위는 늘 1)이고, `claim` 의 키는 `path.lowercased()` 그대로다. 표의 글자는 Swift 시험에서 그대로 옮겼다.

use claude_board_autopilot::rules::multirun::{Claim, Slot, SlotKind};
use claude_board_autopilot::rules::slot_table::{slot_pp_text, slot_state_text, slot_table, ClaimRow, SlotRowState};

const T: f64 = 10_000.0;

fn slot(run: &str, kind: SlotKind, pp: f64, used: f64, since: f64, beat: f64) -> Slot {
    Slot { run: run.to_string(), project: Some("zz-toy".to_string()), unit: 1, kind, pp, used, pid: Some(1), since, beat }
}

/// 기본값(`pp: 5, used: 0, beat: T - 30`)으로 만든 줄
fn sl(run: &str, kind: SlotKind, since: f64) -> Slot {
    slot(run, kind, 5.0, 0.0, since, T - 30.0)
}

fn claim(run: &str, path: &str) -> Claim {
    Claim {
        key: path.to_lowercase(),
        path: path.to_string(),
        run: run.to_string(),
        run_dir: format!("/w/runs/{run}"),
        label: format!("com.claude.board.dev.autopilot.{run}"),
        pid: Some(1),
        claimed: T - 100.0,
    }
}

struct SlotTableRow {
    note: &'static str,
    slots: Vec<Slot>,
    claims: Vec<Claim>,
    active: i64,
    viewing: Option<&'static str>,
    /// "<실행>:<상태>[#차례]" 를 차례로
    rows: &'static str,
    show: bool,
    summary: &'static str,
}

fn row(note: &'static str, slots: Vec<Slot>, active: i64, viewing: &'static str, rows: &'static str, show: bool, summary: &'static str) -> SlotTableRow {
    SlotTableRow { note, slots, claims: Vec::new(), active, viewing: Some(viewing), rows, show, summary }
}

fn table_rows() -> Vec<SlotTableRow> {
    use SlotKind::{Hold, Wait};
    vec![
        row("실행 하나·자리 하나는 보이지 않는다(늘 같은 한 줄)", vec![sl("a", Hold, T - 300.0)], 1, "a", "a:running", false,
            "도는 중 1 · 잡아 두고 아직 쓰지 않은 몫 5%p"),
        row("실행이 둘이면 보인다(자리가 하나여도)", vec![sl("a", Hold, T - 300.0)], 2, "a", "a:running", true,
            "도는 중 1 · 잡아 두고 아직 쓰지 않은 몫 5%p"),
        row("기다리는 실행이 있으면 보인다, 도는 중이 먼저·기다림은 기다린 차례대로",
            vec![sl("c", Wait, T - 50.0), sl("b", Wait, T - 200.0), slot("a", Hold, 5.0, 1.5, T - 300.0, T - 30.0)], 3, "a",
            "a:running b:waiting#1 c:waiting#2", true, "도는 중 1 · 기다림 2 · 잡아 두고 아직 쓰지 않은 몫 3.5%p"),
        row("같은 때 기다리기 시작했으면 실행 id 차례(엔진 apSlotDecide 와 같음)",
            vec![sl("z", Wait, T - 60.0), sl("m", Wait, T - 60.0)], 1, "z", "m:waiting#1 z:waiting#2", true, "기다림 2"),
        row("15분 넘게 다시 적지 않은 자리는 죽은 엔진의 자리로 따로(실행이 하나여도 보임)",
            vec![slot("a", Hold, 5.0, 0.0, T - 2000.0, T - 901.0), sl("b", Hold, T - 300.0)], 1, "b",
            "b:running a:dead", true, "도는 중 1 · 죽은 엔진의 자리 1 · 잡아 두고 아직 쓰지 않은 몫 5%p"),
        row("15분 바로 아래는 아직 산 자리", vec![slot("a", Hold, 5.0, 0.0, T - 2000.0, T - 899.0)], 1, "a", "a:running", false,
            "도는 중 1 · 잡아 두고 아직 쓰지 않은 몫 5%p"),
        row("시각이 1분 넘게 앞선 자리도 산 것으로 보지 않는다(엔진과 같음)", vec![slot("a", Wait, 5.0, 0.0, T, T + 61.0)], 1, "a",
            "a:dead", true, "죽은 엔진의 자리 1"),
        row("죽은 엔진의 기다림 줄은 차례에 들지 않는다",
            vec![slot("x", Wait, 5.0, 0.0, T - 3000.0, T - 1000.0), sl("y", Wait, T - 100.0)], 1, "y", "y:waiting#1 x:dead", true,
            "기다림 1 · 죽은 엔진의 자리 1"),
        row("자리 표가 비면 잡은 자리 없음", vec![], 2, "a", "", true, "잡은 자리 없음"),
    ]
}

#[test]
fn 표() {
    for r in table_rows() {
        let t = slot_table(&r.slots, &r.claims, r.active, r.viewing, T);
        let got = t
            .rows
            .iter()
            .map(|x| format!("{}:{}", x.run, x.state.as_str()) + &x.turn.map(|n| format!("#{n}")).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(got, r.rows, "{}", r.note);
        assert_eq!(t.show, r.show, "{}", r.note);
        assert_eq!(t.summary, r.summary, "{}", r.note);
    }
}

#[test]
fn 경로() {
    let t = slot_table(
        &[sl("a", SlotKind::Hold, T - 300.0)],
        &[claim("a", "/w/toy/beta"), claim("a", "/w/toy/alpha"), claim("p", "/w/toy/gamma"), claim("a", "/w/toy/alpha")],
        2,
        Some("a"),
        T,
    );
    assert_eq!(t.rows.first().map(|r| r.paths.clone()), Some(vec!["/w/toy/alpha".to_string(), "/w/toy/beta".to_string()]));
    assert_eq!(t.rows.first().map(|r| r.mine), Some(true));
    assert_eq!(t.claims_only, vec![ClaimRow { run: "p".to_string(), paths: vec!["/w/toy/gamma".to_string()] }]);
}

#[test]
fn 글자() {
    let t = slot_table(
        &[
            slot("a", SlotKind::Hold, 5.0, 1.25, T - 300.0, T - 30.0),
            slot("b", SlotKind::Wait, 2.5, 0.0, T - 10.0, T - 30.0),
            slot("c", SlotKind::Hold, 4.0, 4.0, T - 3000.0, T - 2000.0),
        ],
        &[],
        2,
        None,
        T,
    );
    assert_eq!(t.rows.iter().map(slot_state_text).collect::<Vec<_>>(), vec!["도는 중", "기다림 1번째", "죽은 엔진의 자리"]);
    assert_eq!(
        t.rows.iter().map(slot_pp_text).collect::<Vec<_>>(),
        vec!["5%p 잡음(쓴 1.3%p)", "2.5%p 기다림", "4%p(쓴 4%p) · 판정에 들지 않음, 다음 판정이 지움"]
    );
    assert_eq!(t.rows[1].used, 0.0, "기다림 줄은 쓴 몫이 없다");
}

// ── Swift 시험에 없는 것(경계) ──

#[test]
fn extra_상태_글자는_원본_rawvalue() {
    assert_eq!(SlotRowState::Running.as_str(), "running");
    assert_eq!(SlotRowState::Waiting.as_str(), "waiting");
    assert_eq!(SlotRowState::Dead.as_str(), "dead");
}

#[test]
fn extra_보는_실행이_없으면_mine_은_모두_거짓() {
    let t = slot_table(&[sl("a", SlotKind::Hold, T - 300.0)], &[], 1, None, T);
    assert!(!t.rows[0].mine);
    let t = slot_table(&[sl("a", SlotKind::Hold, T - 300.0)], &[], 1, Some("b"), T);
    assert!(!t.rows[0].mine);
}

#[test]
fn extra_같은_때_도는_중은_실행_id_차례_죽은_줄은_마지막_적은_때_차례() {
    let t = slot_table(
        &[
            sl("z", SlotKind::Hold, T - 60.0),
            sl("m", SlotKind::Hold, T - 60.0),
            slot("q", SlotKind::Hold, 5.0, 0.0, T - 5000.0, T - 1000.0),
            slot("p", SlotKind::Hold, 5.0, 0.0, T - 9000.0, T - 2000.0),
        ],
        &[],
        1,
        None,
        T,
    );
    let got: Vec<&str> = t.rows.iter().map(|r| r.run.as_str()).collect();
    assert_eq!(got, vec!["m", "z", "p", "q"]);
}

#[test]
fn extra_경로만_쥔_실행은_실행_id_순이고_자리_줄이_있는_실행은_뺀다() {
    let t = slot_table(&[], &[claim("q", "/w/b"), claim("p", "/w/a"), claim("q", "/w/a")], 1, None, T);
    assert_eq!(
        t.claims_only,
        vec![
            ClaimRow { run: "p".to_string(), paths: vec!["/w/a".to_string()] },
            ClaimRow { run: "q".to_string(), paths: vec!["/w/a".to_string(), "/w/b".to_string()] },
        ]
    );
    assert_eq!(t.summary, "잡은 자리 없음");
    assert!(!t.show);
}

#[test]
fn extra_죽은_엔진의_줄은_쓴_몫을_그대로_두고_기다림만_0() {
    let t = slot_table(&[slot("a", SlotKind::Wait, 3.0, 2.0, T - 3000.0, T - 2000.0)], &[], 1, None, T);
    assert_eq!(t.rows[0].state, SlotRowState::Dead);
    assert_eq!(t.rows[0].used, 2.0);
}

#[test]
fn extra_기다림_줄은_자리_표에_쓴_몫이_있어도_0() {
    let t = slot_table(&[slot("a", SlotKind::Wait, 3.0, 2.0, T - 10.0, T - 30.0)], &[], 1, None, T);
    assert_eq!(t.rows[0].state, SlotRowState::Waiting);
    assert_eq!(t.rows[0].used, 0.0);
    assert_eq!(slot_pp_text(&t.rows[0]), "3%p 기다림");
}

#[test]
fn extra_줄은_자리_줄의_단위_시각을_그대로_담는다() {
    let mut s = slot("a", SlotKind::Hold, 4.0, 1.0, T - 300.0, T - 20.0);
    s.unit = 7;
    let t = slot_table(&[s], &[claim("a", "/w/x")], 1, Some("a"), T);
    assert_eq!(t.rows.len(), 1);
    let r = &t.rows[0];
    assert_eq!((r.run.as_str(), r.unit, r.state, r.turn), ("a", 7, SlotRowState::Running, None));
    assert_eq!((r.pp, r.used, r.since, r.beat), (4.0, 1.0, T - 300.0, T - 20.0));
    assert_eq!(r.paths, vec!["/w/x".to_string()]);
    assert!(r.mine);
    assert!(t.claims_only.is_empty());
    assert_eq!(slot_pp_text(r), "4%p 잡음(쓴 1%p)");
}
