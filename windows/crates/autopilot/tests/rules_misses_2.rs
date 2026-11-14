//! 판정 놓침 되먹임 청 2(출발 준비의 확인된 지적·세션 요약·이어쓰기 조건) 시험. 원본: `Tests/AutopilotRulesTests/CritiqueTests.swift` 의
//! `준비`·`요약`·`이어쓰기`(매 행 모두), `Tests/AutopilotRulesTests/TakeoverTests.swift` 의 `놓침` — 이름까지 같다(1:1).
//!
//! 값은 장난감(zz-toy·r1·u1·k1)과 시험 잡글자다.

use claude_board_autopilot::rules::formats::grapheme_len;
use claude_board_autopilot::rules::miss_digest::*;
use claude_board_autopilot::rules::misses::*;
use std::collections::BTreeSet;

/// 사전 나열을 집합으로 — Swift `Set([...])`
fn set(src: &[&str]) -> BTreeSet<String> {
    src.iter().map(|s| (*s).to_string()).collect()
}

/// json! 목록 값만 슬라이스로 — Swift `[[String: Any]]` 매개바뀜 대응
fn rows(v: &serde_json::Value) -> &[serde_json::Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

// 출발 준비의 확인된 지적: 같은 준비에서 고친 것은 fixed, 마지막 차례에도 남은 것은 open 한 줄

#[test]
fn 준비() {
    let critic = serde_json::json!({
        "rounds": [
            { "results": [
                { "id": "f1", "unit": "u1", "judge": "l.sh", "verdict": "confirmed", "why": "글자만" },
                { "id": "f2", "unit": "u2", "judge": "check", "verdict": "confirmed", "why": "x" },
            ] },
            { "results": [
                { "id": "f1", "unit": "u1", "judge": "l.sh", "verdict": "refuted" },
                { "id": "f2", "unit": "u2", "judge": "check", "verdict": "confirmed", "why": "x" },
            ] },
        ],
    });
    let prerun = serde_json::json!({
        "probe_history": [ { "cards": [ { "id": "probe-u1-l.sh", "unit_id": "u1", "file": "l.sh", "probes": ["junk"] } ] } ],
        "probe_cards": [],
    });
    let answers = serde_json::json!({});
    let units = [
        PrepUnit { n: 1, id: "u1".to_string(), source_key: Some("k1".to_string()), title: "가".to_string() },
        PrepUnit { n: 2, id: "u2".to_string(), source_key: None, title: "나".to_string() },
    ];
    let m = prep_misses(&PrepMissIn {
        project: "p",
        run: "r",
        prerun: Some(&prerun),
        critic: Some(&critic),
        answers: &answers,
        units: &units,
        set_id: None,
        at: 1.0,
    });
    assert_eq!(
        m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
        vec!["r/prep/critic/0/f1", "r/prep/critic/1/f2", "r/prep/probe/0/probe-u1-l.sh"]
    );
    assert_eq!(m.iter().map(|x| x.state.as_str()).collect::<Vec<_>>(), vec!["fixed", "open", "fixed"]);
    assert_eq!(
        m.iter().map(|x| x.signal).collect::<Vec<_>>(),
        vec![MissSignal::Critic, MissSignal::Critic, MissSignal::Probe]
    );
}

// 요약: 열린 것·최근 것 먼저, 판정 쪽·지시 쪽, 줄·글자 상한

#[test]
fn 요약() {
    let mut data: Vec<Miss> = Vec::new();
    for i in 1..=40 {
        let sig = if i % 2 == 0 { MissSignal::Discard } else { MissSignal::Denial };
        let st = if i == 40 { "used" } else { "open" };
        data.push(Miss {
            id: format!("r/{i}"),
            project: "p".to_string(),
            run: "r".to_string(),
            unit_n: None,
            unit_id: Some(format!("u{i}")),
            source_key: None,
            signal: sig,
            judge: None,
            judge_digest: None,
            why: format!("까닭 {i}"),
            set_id: None,
            state: st.to_string(),
            created: f64::from(i),
            title: format!("단위 {i}"),
        });
    }
    let d = miss_digest(&data, MISS_DIGEST_ROWS, MISS_DIGEST_CHARS);
    assert!(d.starts_with("판정 쪽:\n- [통과했지만 버림] 단위 38"), "{d}");
    assert!(d.contains("지시 쪽:"), "{d}");
    assert!(d.ends_with("(그 밖에 10건은 줄였습니다)"), "{d}");
    assert_eq!(d.split('\n').filter(|l| l.starts_with("- ")).count(), 30);
    let small = miss_digest(&data, 100, 200);
    assert!(grapheme_len(&small) <= 260, "{small}");
    assert!(miss_digest(&[], MISS_DIGEST_ROWS, MISS_DIGEST_CHARS).is_empty());
}

// 세트 이어 쓰기를 다시 돌리는 조건

#[test]
fn 이어쓰기() {
    // 원본 @Test arguments 표 — (하네스 바뀜, 새 요구, 열린 놓침, 다시 돌림)
    let cases: [(bool, bool, i64, bool); 4] = [
        (false, false, 0, false),
        (false, false, 1, true),
        (true, false, 0, true),
        (false, true, 0, true),
    ];
    for (h, n, m, want) in cases {
        let (rewrite, reasons) = set_rewrite(h, n, m);
        assert_eq!(rewrite, want, "harness_changed={h} new_request={n} open_misses={m}");
        if m > 0 {
            assert_eq!(reasons, vec![format!("판정 놓침 {m}건이 있습니다")]);
        }
    }
}

// 판정 놓침 기록의 신호 takeover(지시 쪽, 돌려준 줄만, id 는 같은 차례면 같다) — misses::unit_misses·miss_label 과 이 청의 요약을 함께 본다

#[test]
fn 놓침() {
    let tk = serde_json::json!([
        { "back": 20.0, "turns": ["목록을\n정렬", "⟦x⟧"], "files": ["a.swift"], "commits": { "zz-repo": 2 }, "harness": ["0:CLAUDE.md"] },
        { "at": 30.0 },
    ]); // 아직 돌려주지 않은 줄은 넘김
    let m = unit_misses(&UnitMissIn {
        project: "zz-toy",
        run: "r1",
        n: 2,
        unit_id: Some("u2"),
        source_key: None,
        title: "목록",
        status: "done",
        story: None,
        discarded_sources: &set(&[]),
        result: None,
        denials: &[],
        judge: None,
        judge_digest: None,
        set_id: None,
        at: 1.0,
        takeovers: rows(&tk),
    });
    assert_eq!(m.iter().map(|x| x.signal).collect::<Vec<_>>(), vec![MissSignal::Takeover]);
    assert_eq!(m[0].id, "r1/2/takeover/1");
    assert_eq!(m[0].side(), "instruction");
    assert_eq!(
        m[0].why,
        "사람이 터미널로 이어받아 요청 2개·커밋 2개·바뀐 파일 1개를 남겼습니다: «목록을 정렬» «〔x〕» · 하네스 바뀜 0:CLAUDE.md"
    );
    assert_eq!(miss_label(MissSignal::Takeover), "사람이 이어받음");
    assert!(miss_digest(&m, MISS_DIGEST_ROWS, MISS_DIGEST_CHARS).contains("지시 쪽:\n- [사람이 이어받음] 목록"));
}
