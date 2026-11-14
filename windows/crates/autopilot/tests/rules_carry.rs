//! 이어 가기 미룬 몫 규칙(`rules::carry`) 시험.
//! 원본: `Tests/AutopilotRulesTests/StartTests.swift` 의 `OpenParkedTests`(표 5줄, 시험 1개 `표`). 이 규칙 전용 시험 파일은 없다.
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 경계(정렬 꼬리 견줌·started 없음·글 그대로 견줌)를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::carry::{open_parked, CarryRow};
use std::collections::{HashMap, HashSet};

struct OpenParkedRow {
    note: &'static str,
    /// (id, 실행, kind, 제목, 실행 출발)
    rows: Vec<(&'static str, &'static str, &'static str, &'static str, f64)>,
    answers: Vec<(&'static str, &'static str)>,
    chain: Vec<&'static str>,
    shown: Vec<(&'static str, &'static str)>,
    /// id
    want: Vec<&'static str>,
}

fn open_rows() -> Vec<OpenParkedRow> {
    vec![
        OpenParkedRow {
            note: "줄 밖 실행의 미룬 몫은 보인다, 줄 안은 뺀다",
            rows: vec![("A/1/next_start/0", "A", "next_start", "재시도 횟수", 10.0), ("C/2/next_start/0", "C", "next_start", "알림 주소", 30.0)],
            answers: vec![],
            chain: vec!["C"],
            shown: vec![],
            want: vec!["A/1/next_start/0"],
        },
        OpenParkedRow {
            note: "새로 찾은 일은 미룬 몫이 아니다",
            rows: vec![("A/1/new_work/0", "A", "new_work", "페이지 나누기", 10.0)],
            answers: vec![],
            chain: vec![],
            shown: vec![],
            want: vec![],
        },
        OpenParkedRow {
            note: "어느 실행에서든 정함·닫음으로 답했으면 뺀다, 다시 미룸은 열린 채",
            rows: vec![
                ("A/1/next_start/0", "A", "next_start", "가", 10.0),
                ("A/1/next_start/1", "A", "next_start", "나", 10.0),
                ("A/2/next_start/0", "A", "next_start", "다", 10.0),
            ],
            answers: vec![("A/1/next_start/0", "decide"), ("A/1/next_start/1", "close"), ("A/2/next_start/0", "park")],
            chain: vec![],
            shown: vec![],
            want: vec!["A/2/next_start/0"],
        },
        OpenParkedRow {
            note: "이미 카드로 가져온 것(같은 id·같은 제목)은 뺀다",
            rows: vec![("A/1/next_start/0", "A", "next_start", "재시도 횟수", 10.0), ("B/1/next_start/0", "B", "next_start", "알림 주소", 20.0)],
            answers: vec![],
            chain: vec![],
            shown: vec![("A/1/next_start/0", "x"), ("C/9/next_start/0", "알림 주소")],
            want: vec![],
        },
        OpenParkedRow {
            note: "같은 제목이 여럿이면 최근 실행의 것 하나, 최근부터",
            rows: vec![
                ("A/1/next_start/0", "A", "next_start", "재시도 횟수", 10.0),
                ("B/3/next_start/0", "B", "next_start", "재시도 횟수", 20.0),
                ("A/2/next_start/0", "A", "next_start", "운영 값", 10.0),
            ],
            answers: vec![],
            chain: vec![],
            shown: vec![],
            want: vec!["B/3/next_start/0", "A/2/next_start/0"],
        },
    ]
}

// Swift 시험의 변환: `APCarryRow(id:run:unit: 1, kind:title:why: "", started:)`
fn carry(id: &str, run: &str, unit: i64, kind: &str, title: &str, started: Option<f64>) -> CarryRow {
    CarryRow { id: id.into(), run: run.into(), unit, kind: kind.into(), title: title.into(), why: String::new(), started }
}

fn answers_of(items: &[(&str, &str)]) -> HashMap<String, String> {
    items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn chain_of(items: &[&str]) -> HashSet<String> {
    items.iter().map(|i| i.to_string()).collect()
}

fn shown_of(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn ids(rows: &[CarryRow]) -> Vec<&str> {
    rows.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn 표_길이() {
    assert_eq!(open_rows().len(), 5, "Swift 표는 5줄이다");
}

#[test]
fn 열린_것만() {
    for r in open_rows() {
        let rows: Vec<CarryRow> = r.rows.iter().map(|x| carry(x.0, x.1, 1, x.2, x.3, Some(x.4))).collect();
        let got = open_parked(&rows, &answers_of(&r.answers), &chain_of(&r.chain), &shown_of(&r.shown));
        assert_eq!(ids(&got), r.want, "{}", r.note);
    }
}

// ---- 추가: Swift 시험에 없는 경계 ----

#[test]
fn extra_returned_rows_are_the_input_rows_unchanged() {
    let row = CarryRow {
        id: "A/1/next_start/0".into(),
        run: "A".into(),
        unit: 4,
        kind: "next_start".into(),
        title: "t".into(),
        why: "이유".into(),
        started: Some(5.0),
    };
    let got = open_parked(std::slice::from_ref(&row), &HashMap::new(), &HashSet::new(), &[]);
    assert_eq!(got, vec![row]);
}

#[test]
fn extra_empty_input_gives_empty() {
    assert!(open_parked(&[], &HashMap::new(), &HashSet::new(), &[]).is_empty());
}

#[test]
fn extra_missing_started_counts_as_zero_and_sorts_last() {
    let rows = vec![
        carry("none", "A", 1, "next_start", "가", None),
        carry("old", "A", 1, "next_start", "나", Some(1.0)),
        carry("zero", "A", 1, "next_start", "다", Some(0.0)),
    ];
    // 없음(0)과 0 은 같은 값이라 run·unit 도 같으면 입력 순서(안정 정렬)를 지킨다
    assert_eq!(ids(&open_parked(&rows, &HashMap::new(), &HashSet::new(), &[])), vec!["old", "none", "zero"]);
}

#[test]
fn extra_same_start_breaks_ties_by_run_descending_then_unit_ascending() {
    let rows = vec![
        carry("a2", "A", 2, "next_start", "t1", Some(10.0)),
        carry("a1", "A", 1, "next_start", "t2", Some(10.0)),
        carry("b1", "B", 1, "next_start", "t3", Some(10.0)),
    ];
    // (started, run, -unit) 내림차순: run 은 B 가 앞, 같은 run 에서는 unit 이 작은 쪽이 앞
    assert_eq!(ids(&open_parked(&rows, &HashMap::new(), &HashSet::new(), &[])), vec!["b1", "a1", "a2"]);
}

#[test]
fn extra_duplicate_title_keeps_the_later_sorted_not_the_first_listed() {
    let rows = vec![
        carry("old", "A", 1, "next_start", "같은 제목", Some(1.0)),
        carry("new", "B", 1, "next_start", "같은 제목", Some(9.0)),
    ];
    assert_eq!(ids(&open_parked(&rows, &HashMap::new(), &HashSet::new(), &[])), vec!["new"]);
}

#[test]
fn extra_duplicate_title_loser_is_dropped_even_if_the_winner_is_filtered_first() {
    // 걸러진 몫은 제목을 차지하지 않는다 — 최근 것이 답했으면 옛 것이 같은 제목으로 남는다
    let rows = vec![
        carry("old", "A", 1, "next_start", "같은 제목", Some(1.0)),
        carry("new", "B", 1, "next_start", "같은 제목", Some(9.0)),
    ];
    let got = open_parked(&rows, &answers_of(&[("new", "close")]), &HashSet::new(), &[]);
    assert_eq!(ids(&got), vec!["old"]);
}

#[test]
fn extra_answers_other_than_decide_close_keep_the_row_open() {
    let rows = vec![carry("x", "A", 1, "next_start", "t", Some(1.0))];
    for answer in ["park", "", "DECIDE", "closed"] {
        assert_eq!(ids(&open_parked(&rows, &answers_of(&[("x", answer)]), &HashSet::new(), &[])), vec!["x"], "{answer:?}");
    }
}

#[test]
fn extra_comparisons_are_verbatim_text_so_case_and_separators_matter() {
    // Windows 경로 꼴 글도 그대로 견준다 — 대소문자·`\` 와 `/` 는 다른 글이다
    let rows = vec![
        carry("r1", "Run\\A", 1, "next_start", "t1", Some(2.0)),
        carry("r2", "run/a", 1, "next_start", "t2", Some(1.0)),
    ];
    assert_eq!(ids(&open_parked(&rows, &HashMap::new(), &chain_of(&["run\\a"]), &[])), vec!["r1", "r2"]);
    assert_eq!(ids(&open_parked(&rows, &HashMap::new(), &chain_of(&["Run\\A"]), &[])), vec!["r2"]);
    assert_eq!(ids(&open_parked(&rows, &HashMap::new(), &HashSet::new(), &shown_of(&[("R1", "T2")]))), vec!["r1", "r2"]);
}

#[test]
fn extra_shown_id_and_shown_title_each_filter_independently() {
    let rows = vec![
        carry("a", "A", 1, "next_start", "t-a", Some(3.0)),
        carry("b", "A", 2, "next_start", "t-b", Some(3.0)),
        carry("c", "A", 3, "next_start", "t-c", Some(3.0)),
    ];
    // id "a" 는 제목이 달라도 빠지고, 제목 "t-b" 는 id 가 달라도 빠진다
    let got = open_parked(&rows, &HashMap::new(), &HashSet::new(), &shown_of(&[("a", "zzz"), ("zzz", "t-b")]));
    assert_eq!(ids(&got), vec!["c"]);
}
