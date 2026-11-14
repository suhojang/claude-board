//! 지난 실행 화면 규칙(`rules::history`) 시험.
//! 원본: `Tests/AutopilotRulesTests/HistoryTests.swift`(표 8줄을 시험 하나가 돌린다).
//! Swift 표의 줄을 같은 입력·같은 기대값·같은 note 로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 자리를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::history::{history_pick, RunTag};

struct PickRow {
    note: &'static str,
    runs: Vec<(&'static str, &'static str)>, // (id, 프로젝트) 최근 것부터
    selected: Option<&'static str>,
    project: Option<&'static str>,
    want: Option<&'static str>,
}

fn pick_rows() -> Vec<PickRow> {
    vec![
        PickRow { note: "목록이 비었고 고른 것도 없음 → 없음", runs: vec![], selected: None, project: None, want: None },
        PickRow { note: "고른 것이 없으면 첫 실행", runs: vec![("r2", "a"), ("r1", "b")], selected: None, project: None, want: Some("r2") },
        PickRow { note: "고른 실행", runs: vec![("r2", "a"), ("r1", "b")], selected: Some("r1"), project: None, want: Some("r1") },
        PickRow {
            note: "5-3b ⑥: 프로젝트를 거르면 거른 목록의 첫 실행(전에는 전체 목록의 첫 실행)",
            runs: vec![("r2", "a"), ("r1", "b")],
            selected: None,
            project: Some("b"),
            want: Some("r1"),
        },
        PickRow {
            note: "5-3b ⑥: 고른 실행이 거른 목록에 없으면 거른 목록의 첫 실행",
            runs: vec![("r3", "a"), ("r2", "b"), ("r1", "b")],
            selected: Some("r3"),
            project: Some("b"),
            want: Some("r2"),
        },
        PickRow {
            note: "고른 실행이 거른 목록에 있으면 그것",
            runs: vec![("r3", "a"), ("r2", "b"), ("r1", "b")],
            selected: Some("r1"),
            project: Some("b"),
            want: Some("r1"),
        },
        PickRow { note: "거른 목록이 비면 없음", runs: vec![("r2", "a")], selected: None, project: Some("c"), want: None },
        PickRow {
            note: "5-3b ⑥: 목록에 없는 고른 실행은 첫 실행으로(전에는 그대로 두어 상세가 비었다)",
            runs: vec![("r2", "a")],
            selected: Some("zz"),
            project: None,
            want: Some("r2"),
        },
    ]
}

fn tags(runs: &[(&str, &str)]) -> Vec<RunTag> {
    runs.iter().map(|(id, project)| RunTag::new(*id, *project)).collect()
}

#[test]
fn 표_길이() {
    assert_eq!(pick_rows().len(), 8, "Swift 표는 8줄이다");
}

#[test]
fn 표() {
    for r in pick_rows() {
        let got = history_pick(r.selected, &tags(&r.runs), r.project);
        assert_eq!(got.as_deref(), r.want, "{}", r.note);
    }
}

// ── 추가: Swift 시험에 없는 자리 ─────────────────────────

#[test]
fn extra_selected_in_other_project_is_dropped_even_if_it_is_first_overall() {
    // 고른 것이 전체 목록의 첫 실행이어도 거른 목록 밖이면 거른 목록의 첫 실행
    let runs = tags(&[("r3", "a"), ("r2", "b")]);
    assert_eq!(history_pick(Some("r3"), &runs, Some("b")).as_deref(), Some("r2"));
}

#[test]
fn extra_selected_with_empty_filtered_list_is_none() {
    let runs = tags(&[("r2", "a")]);
    assert_eq!(history_pick(Some("r2"), &runs, Some("c")), None);
    assert_eq!(history_pick(Some("r2"), &[], None), None);
}

#[test]
fn extra_empty_string_project_is_a_filter_not_all() {
    // Swift 의 `project == nil` 만 전부를 뜻한다. 빈 문자열은 그 이름의 프로젝트로 거른다
    let runs = tags(&[("r2", "a"), ("r1", "")]);
    assert_eq!(history_pick(None, &runs, Some("")).as_deref(), Some("r1"));
}

#[test]
fn extra_korean_and_case_are_compared_exactly() {
    let runs = tags(&[("r2", "프로젝트-가"), ("r1", "Proj")]);
    assert_eq!(history_pick(None, &runs, Some("프로젝트-가")).as_deref(), Some("r2"));
    assert_eq!(history_pick(None, &runs, Some("proj")), None);
    assert_eq!(history_pick(Some("R1"), &runs, None).as_deref(), Some("r2"));
}

#[test]
fn extra_duplicate_ids_return_the_selected_one() {
    let runs = tags(&[("r1", "a"), ("r1", "b")]);
    assert_eq!(history_pick(Some("r1"), &runs, Some("b")).as_deref(), Some("r1"));
}
