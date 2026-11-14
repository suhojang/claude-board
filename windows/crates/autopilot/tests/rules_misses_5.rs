//! 세트 이어 쓰기 세션 청 5(프롬프트·결과 검사). 원본: `Tests/AutopilotRulesTests/CritiqueTests.swift` 의 `이어쓰기검사`,
//! `Tests/AutopilotRulesTests/HarnessTests.swift` 의 `인용우회`·`시나리오`·`하네스` — 이름까지 같다(1:1).
//! 인용우회의 `apInstructionCheck` 부분은 하네스 청 시험칸에 이미 있으므로 여기는 이어 쓰기 검사 부분만 옮긴다.
//!
//! 값은 장난감(u1·u2·hello.txt)과 시험 잡글자다.

use claude_board_autopilot::rules::draft::{DraftFiles, INSTRUCTION_HEAD_LINE};
use claude_board_autopilot::rules::harness::HarnessPoint;
use claude_board_autopilot::rules::revise::{
    revise_check, revise_prompt, ReviseHarness, ReviseHarnessFile, ReviseUnit, REVISE_HEAD_LINE,
};
use std::collections::BTreeMap;

/// 엔진이 넣어 둔 결과 폴더 흉내 — Swift `ReviseOut(APDraftFiles)`
struct Out(BTreeMap<String, Vec<u8>>);

impl DraftFiles for Out {
    fn read(&self, relative: &str) -> Option<Vec<u8>> {
        self.0.get(relative).cloned()
    }
    fn list(&self, relative: &str) -> Vec<String> {
        self.0
            .keys()
            .filter(|k| k.starts_with(&(relative.to_string() + "/")))
            .map(|k| k[relative.len() + 1..].to_string())
            .filter(|k| !k.contains('/'))
            .collect()
    }
}

/// (이름, 내용) 짝 — 이어 쓰기 검사가 바라는 결과 폴더 내용
fn pair(name: &str, content: impl AsRef<[u8]>) -> (String, Vec<u8>) {
    (name.to_string(), content.as_ref().to_vec())
}

#[test]
fn 이어쓰기검사() {
    let us = vec![
        ReviseUnit {
            id: "u1".to_string(),
            title: "안내".to_string(),
            prompt: "p".to_string(),
            check: "grep -q '할 일' NOTES.md".to_string(),
            scenarios: vec![],
        },
        ReviseUnit {
            id: "u2".to_string(),
            title: "목록".to_string(),
            prompt: "p".to_string(),
            check: "test -f a".to_string(),
            scenarios: vec![],
        },
    ];
    let good = Out(BTreeMap::from([
        pair(
            "units.json",
            r#"[{"id":"u1","check":"test \"$(sed -n 's/^할 일: //p' NOTES.md)\" = 우유","why":"글자만 보던 것"},{"id":"u2","check":"test -f a"}]"#,
        ),
        pair("autopilot.md", format!("{}\n대답할 사람이 없습니다.\n", INSTRUCTION_HEAD_LINE)),
    ]));
    let r = revise_check(&good, &us, &BTreeMap::new(), &[]);
    assert!(
        r.ok()
            && r.checks.contains_key("u1")
            && !r.checks.contains_key("u2")
            && r.instruction.as_deref().unwrap_or("").starts_with(INSTRUCTION_HEAD_LINE)
            && r.whys.get("u1").map(String::as_str) == Some("글자만 보던 것"),
        "{:?}",
        r.problems
    );
    let bad = Out(BTreeMap::from([
        pair(
            "units.json",
            r#"[{"id":"u1","check":""},{"id":"u2","check":"git push origin live"},{"id":"u9","check":"x"},{"id":"u1","check":"x","prompt":"y"}]"#,
        ),
        pair("autopilot.md", "첫 줄이 틀림\n"),
    ]));
    let b = revise_check(&bad, &us, &BTreeMap::new(), &[]);
    assert_eq!(b.problems.len(), 5, "{:?}", b.problems);
    assert!(b.instruction.is_none());
    assert!(b.problems[0].contains("지웠습니다"), "{:?}", b.problems);
    assert!(revise_check(&Out(BTreeMap::new()), &us, &BTreeMap::new(), &[]).ok());
    let p = revise_prompt(
        &us,
        None,
        "판정 쪽:\n- x",
        &["판정 놓침 1건이 있습니다".to_string()],
        "/O",
        None,
    );
    assert!(
        p.starts_with(REVISE_HEAD_LINE)
            && p.contains("판정 놓침 1건")
            && p.contains("grep -q '할 일' NOTES.md")
            && !p.contains("—"),
        "{:?}",
        p
    );
}

// 고친 지시에 바깥 쓰기 꼴이 있으면 막는다

#[test]
fn 인용우회() {
    let w = HarnessPoint {
        file: ".claude/skills/t/SKILL.md".to_string(),
        line: 5,
        kind: "write".to_string(),
        text: "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.".to_string(),
    };
    let p = HarnessPoint {
        file: "CLAUDE.md".to_string(),
        line: 7,
        kind: "write".to_string(),
        text: "작업이 끝나면 git push 로 올린다.".to_string(),
    };
    let head = format!("{}\n대답할 사람이 없어 멈추므로 보류합니다.\n", INSTRUCTION_HEAD_LINE);
    let ins = head + "- CLAUDE.md:7 보류\n- 단위마다 작업이 끝나면 git push 로 올린다.\n";
    let r = revise_check(&Out(BTreeMap::from([pair("autopilot.md", ins)])), &[], &BTreeMap::new(), &[w, p]);
    assert!(
        r.problems.iter().any(|x| x.contains("바깥 쓰기")),
        "{:?}",
        r.problems
    );
}

// 시나리오: 고친 것만 받고, 지움·#! 없음·#! 만·비밀 꼴·새 파일은 받지 않음

#[test]
fn 시나리오() {
    let units = vec![ReviseUnit {
        id: "u1".to_string(),
        title: "인사".to_string(),
        prompt: "p".to_string(),
        check: String::new(),
        scenarios: vec![".autopilot/scenarios/u1/check.sh".to_string()],
    }];
    let orig = "#!/bin/sh\ntest -f hello.txt\n";
    let originals =
        BTreeMap::from([("u1/check.sh".to_string(), orig.as_bytes().to_vec())]);
    let fixed = "#!/bin/sh\ntest \"$(cat hello.txt)\" = 안녕\n";
    let r = revise_check(
        &Out(BTreeMap::from([pair("scenarios/u1/check.sh", fixed)])),
        &units,
        &originals,
        &[],
    );
    assert!(
        r.ok() && r.scenarios.len() == 1 && r.scenarios.contains_key("u1/check.sh"),
        "{:?}",
        r.problems
    );
    let unchanged = revise_check(
        &Out(BTreeMap::from([pair("scenarios/u1/check.sh", orig)])),
        &units,
        &originals,
        &[],
    );
    assert!(unchanged.scenarios.is_empty());
    let removed = revise_check(&Out(BTreeMap::new()), &units, &originals, &[]);
    assert!(removed.problems[0].contains("지웠습니다"), "{:?}", removed.problems);
    let shebang = revise_check(&Out(BTreeMap::from([pair("scenarios/u1/check.sh", "test -f a\n")])), &units, &originals, &[]);
    assert!(shebang.problems[0].contains("#!"), "{:?}", shebang.problems);
    let only = revise_check(&Out(BTreeMap::from([pair("scenarios/u1/check.sh", "#!/bin/sh\n")])), &units, &originals, &[]);
    assert!(only.problems[0].contains("#! 줄밖에"), "{:?}", only.problems);
    let tok = format!("ghp_{}", "A1".repeat(18));
    let secret = revise_check(
        &Out(BTreeMap::from([pair("scenarios/u1/check.sh", format!("#!/bin/sh\necho {}\n", tok))])),
        &units,
        &originals,
        &[],
    );
    assert!(secret.problems[0].contains("비밀"), "{:?}", secret.problems);
    let extra = revise_check(
        &Out(BTreeMap::from([
            pair("scenarios/u1/check.sh", orig),
            pair("scenarios/u1/new.sh", "#!/bin/sh\nexit 0\n"),
        ])),
        &units,
        &originals,
        &[],
    );
    assert!(
        extra.ok() && extra.scenarios.is_empty() && extra.warnings[0].contains("받지 않았습니다"),
        "{:?}",
        (extra.problems, extra.warnings)
    );
}

// 바뀐 하네스: 글에 바뀐 파일·확인 지점 전체·빠진 자리, 고친 지시가 다루지 않은 자리는 카드로

#[test]
fn 하네스() {
    let units = vec![ReviseUnit {
        id: "u1".to_string(),
        title: "인사".to_string(),
        prompt: "p".to_string(),
        check: String::new(),
        scenarios: vec![".autopilot/scenarios/u1/check.sh".to_string()],
    }];
    let pts = vec![
        HarnessPoint {
            file: "CLAUDE.md".to_string(),
            line: 4,
            kind: "confirm".to_string(),
            text: "배포 전 확인".to_string(),
        },
        HarnessPoint {
            file: ".claude/rules/release.md".to_string(),
            line: 5,
            kind: "confirm".to_string(),
            text: "마이그레이션 확인".to_string(),
        },
    ];
    let hz = ReviseHarness {
        files: vec![ReviseHarnessFile {
            path: ".claude/rules/release.md".to_string(),
            change: "changed".to_string(),
            copy: Some("/P/revise-in/harness/.claude/rules/release.md".to_string()),
        }],
        points: pts.clone(),
        missing: vec![pts[1].clone()],
    };
    let p = revise_prompt(
        &units,
        Some("옛 지시"),
        "",
        &["하네스가 출발 때와 달라졌습니다".to_string()],
        "/O",
        Some(&hz),
    );
    assert!(
        p.contains("읽기 전용 사본 /P/revise-in/harness/.claude/rules/release.md")
            && p.contains(".claude/rules/release.md:5 [confirm]")
            && p.contains("지난 지시가 다루지 않은 자리: .claude/rules/release.md:5")
            && p.contains("scenarios/u1/check.sh")
            && !p.contains("—"),
        "{:?}",
        p
    );
    let ins = format!("{}\n까닭.\n- CLAUDE.md:4 보류\n끝내기 전에 다시 본다.\n", INSTRUCTION_HEAD_LINE);
    let r = revise_check(&Out(BTreeMap::from([pair("autopilot.md", ins)])), &units, &BTreeMap::new(), &pts);
    assert!(
        r.ok() && r.harness_missing.iter().map(|x| x.place()).collect::<Vec<_>>() == vec![".claude/rules/release.md:5".to_string()],
        "{:?}",
        (r.problems, r.harness_missing)
    );
    let empty = revise_prompt(&[], None, "", &["x".to_string()], "/O", None);
    assert!(!empty.contains("scenarios/"), "{:?}", empty);
}

