//! 판정 차이(출발 확인) 규칙 시험. 원본: `Tests/AutopilotRulesTests/CritiqueTests.swift` 의 `차이`
//! (판정 차이: 단위 id 로 짝지어 판정 명령·시나리오 목록).

use claude_board_autopilot::rules::judge_changes::*;
use std::collections::BTreeMap;

#[test]
fn 차이() {
    // 단위 id 로 짝지어 판정 명령·시나리오 목록 — old 에 없는 단위(u3)는 낸 것이 없다
    let old = [
        serde_json::json!({"id": "u1", "title": "안내", "check": "grep -q a N.md"}),
        serde_json::json!({"id": "u2", "title": "목록", "scenarios": [{"file": "s.sh", "criterion": "AC-1"}]}),
    ];
    let new = [
        serde_json::json!({"id": "u1", "title": "안내", "check": "test x"}),
        serde_json::json!({"id": "u2", "title": "목록", "scenarios": [{"file": "s2.sh", "criterion": "AC-1"}]}),
        serde_json::json!({"id": "u3", "check": "y"}),
    ];
    let got = judge_changes(&old, &new);
    assert_eq!(
        got,
        vec![
            JudgeChange {
                unit: "u1".to_string(),
                title: "안내".to_string(),
                field: "check".to_string(),
                old: "grep -q a N.md".to_string(),
                new: "test x".to_string(),
            },
            JudgeChange {
                unit: "u2".to_string(),
                title: "목록".to_string(),
                field: "scenarios".to_string(),
                old: "s.sh(AC-1)".to_string(),
                new: "s2.sh(AC-1)".to_string(),
            },
        ]
    );
    // 같은 판 견주기에는 차이가 없다
    assert!(judge_changes(&old, &old).is_empty());
    // 줄 차이는 짝지어 뺀다
    assert_eq!(line_change("a\nb\nc", "a\nc\nd\ne"), (2, 1));
    // 시나리오 글자 차이 — 없어진 시나리오(u1/gone.sh)와 새 시나리오(u1/new.sh)는 낸 것이 없다
    let old_text = BTreeMap::from([
        ("u1/list.sh".to_string(), "a\nb".to_string()),
        ("u1/gone.sh".to_string(), "x".to_string()),
    ]);
    let new_text = BTreeMap::from([
        ("u1/list.sh".to_string(), "a\nc".to_string()),
        ("u1/new.sh".to_string(), "y".to_string()),
    ]);
    let titles = BTreeMap::from([("u1".to_string(), "할 일".to_string())]);
    let got = scenario_text_changes(&old_text, &new_text, &titles);
    assert_eq!(
        got,
        vec![JudgeChange {
            unit: "u1".to_string(),
            title: "할 일".to_string(),
            field: "scenario:list.sh".to_string(),
            old: "1줄 뺌".to_string(),
            new: "1줄 더함".to_string(),
        }]
    );
}
