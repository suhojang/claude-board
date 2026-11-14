//! 하네스 읽기(`rules::harness`) 시험. 원본: `Tests/AutopilotRulesTests/HarnessTests.swift`.
//! 이 파일이 옮긴 Swift 시험 11개(같은 이름): `줄`(표 44줄) · `울타리` · `한계` · `파일`(표 19줄) ·
//! `모으기` · `상한` · `시나리오자리` · `경로넘기` · `견주기` · `경계`(표 7줄) · `정책`, 가짜 `FakeHarness` 도 같이 옮겼다.
//! Swift 시험의 표 글자 값은 그대로 옮겼고, 표 길이를 세어 줄이 빠지면 실패한다.
//! 옮기지 않은 Swift 시험 5개: `인용`·`인용우회`의 `apInstructionCheck`, 이어 쓰기 `시나리오`·`하네스`의
//! `apReviseCheck`·`apRevisePrompt`·`APReviseUnit`(Draft.swift 가 미이식), 가져오기 `시나리오`의
//! `apImportCheck`(Import.swift 가 미이식). 그 검사기들이 이식되면 이 표들도 따라 옮긴다.
//! `견주기` 끝의 지문 검사는 원본이 `apFNV64Data == apFNV64`(문자열 변형과 바이트 변형의 일치)를 잡는 자리인데,
//! 이쪽은 하나의 `fnv64_data` 라 알려진 FNV-1a 64 벡터로 고정한다(빈 글·"a") — 안 맞으면 조용히 틀린 지문이 남는다.
//! 끝의 「추가」 절은 Swift 시험에 없는 이 이식의 몫(Foundation 과 다른 점·경계)이다.

use claude_board_autopilot::rules::harness::{
    harness_baseline_json, harness_baseline_parse, harness_card_id, harness_changes, harness_collect, harness_file_wanted, harness_missing,
    harness_points, harness_policy_card, harness_policy_class, harness_policy_id, harness_state, harness_state_note, harness_texts,
    instruction_clean, instruction_forbidden_lines, pinned_scenario_path, seed_scenario_path_ok, unit_dir_ok, fnv64_data, HarnessBaseline,
    HarnessFileMark, HarnessFileRead, HarnessPoint, HarnessRead, HarnessSource, WalkStep, HARNESS_FILE_MAX, HARNESS_POLICY_PREFIX,
};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

// MARK: 확인 지점 찾기

#[test]
fn 줄() {
    // 줄 하나 → 찾을 종류(None 이면 지나쳐야 함). 2번째는 그 줄이 든 하네스 파일(기본 CLAUDE.md)
    let rows: Vec<(&str, &str, &str, Option<&str>)> = vec![
        ("확인받기", "CLAUDE.md", "배포 전에는 사람에게 확인받고 진행한다.", Some("confirm")),
        ("묻기(문장 끝)", "CLAUDE.md", "모호하면 사용자에게 묻는다.", Some("ask")),
        ("묻고 진행", "CLAUDE.md", "작업 전에 사람에게 묻고 진행한다", Some("ask")),
        ("물어보기", "CLAUDE.md", "범위가 넓으면 먼저 물어본다", Some("ask")),
        ("AskUserQuestion 도구", ".claude/skills/t/SKILL.md", "2. AskUserQuestion 으로 담당자를 고른다.", Some("ask")),
        ("영어 묻기", "CLAUDE.md", "Ask the user before deleting files.", Some("ask")),
        ("사람의 확인(놓치던 꼴)", "CLAUDE.md", "삭제하기 전에 담당자의 확인을 거친다.", Some("confirm")),
        ("검토 받기(놓치던 꼴)", "CLAUDE.md", "스키마 변경은 리뷰를 받은 뒤 합친다.", Some("confirm")),
        ("허락(놓치던 꼴)", "CLAUDE.md", "새 의존성은 허락을 받고 넣는다.", Some("confirm")),
        ("영어 확인", "CLAUDE.md", "Confirm with the user before running migrations.", Some("confirm")),
        ("승인 받은 뒤", "CLAUDE.md", "- PR 은 리뷰어의 승인을 받은 뒤 머지한다.", Some("approve")),
        ("승인 없이 + 부정(이중 부정은 확인 지점)", "CLAUDE.md", "- 승인 없이 main 에 합치지 않는다.", Some("approve")),
        ("영어 승인", "CLAUDE.md", "Merging requires approval from a maintainer.", Some("approve")),
        ("미리보기로 보여 줌", "CLAUDE.md", "- 릴리스 노트는 미리보기로 사람에게 보여 준 뒤 올린다.", Some("preview")),
        ("사람 답 기다리기", "CLAUDE.md", "3. 사용자의 답을 기다린 뒤 다음으로 간다.", Some("wait")),
        ("멈추고 기다리기", "CLAUDE.md", "막히면 멈추고 기다린다.", Some("wait")),
        ("영어 기다리기", "CLAUDE.md", "Wait for the user to confirm the plan.", Some("wait")),
        ("Jira 쓰기 도구", "CLAUDE.md", "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.", Some("write")),
        ("Jira 쓰기 말(놓치던 꼴)", "CLAUDE.md", "끝나면 지라에 티켓을 만든다.", Some("write")),
        ("메신저 알림(놓치던 꼴)", "CLAUDE.md", "배포가 끝나면 슬랙으로 알린다.", Some("write")),
        ("git push", "CLAUDE.md", "작업이 끝나면 git push 로 올린다.", Some("write")),
        ("gh pr create", "CLAUDE.md", "gh pr create 로 PR 을 만든다.", Some("write")),
        ("울타리 코드 안의 도구 이름은 잡음", "CLAUDE.md", "git push origin main", Some("write")),
        ("hook exit 2", ".claude/hooks/guard.sh", "  exit 2", Some("block")),
        (
            "hook decision block",
            ".claude/hooks/guard.py",
            "print(json.dumps({\"decision\": \"block\", \"reason\": \"x\"}))",
            Some("block"),
        ),
        ("hook permissionDecision deny", ".claude/hooks/g.js", "out({permissionDecision: \"deny\"})", Some("block")),
        ("설정의 PreToolUse hook", ".claude/settings.json", "\"PreToolUse\": [", Some("block")),
        ("설정의 권한 묻기", ".claude/settings.json", "\"ask\": [\"Bash(git push:*)\"]", Some("ask")),
        ("대기열 낱말", "CLAUDE.md", "대기열 파일(queue.txt)을 먼저 읽는다.", None),
        ("자주 묻는 질문(옛 꼴이 잡던 것)", "CLAUDE.md", "자주 묻는 질문은 docs/faq.md 에 있다.", None),
        ("묻지 않고 진행", "CLAUDE.md", "사람에게 묻지 않고 끝까지 진행한다.", None),
        ("물어보지 않음", "CLAUDE.md", "작은 결정은 물어보지 않고 고른다.", None),
        ("결제 승인 API(옛 꼴이 잡던 이름)", "CLAUDE.md", "결제 승인 API 는 src/pay.py 에 있다.", None),
        ("승인 없이 진행(부정 하나)", "CLAUDE.md", "문서 고침은 승인 없이 진행한다.", None),
        ("빌드 기다리기(옛 꼴이 잡던 것)", "CLAUDE.md", "빌드가 끝날 때까지 기다린다.", None),
        ("미리보기 기능 이름", "CLAUDE.md", "미리보기 화면 컴포넌트는 src/preview.tsx 에 있다.", None),
        ("스스로 확인", "CLAUDE.md", "테스트가 통과하는지 확인한다.", None),
        ("영어 부정", "CLAUDE.md", "Don't ask the user for small choices.", None),
        ("읽기 도구 이름", "CLAUDE.md", "mcp__atlassian__getJiraIssue 로 티켓을 읽는다.", None),
        ("push 하지 않음", "CLAUDE.md", "git push 는 하지 않는다.", None),
        ("커밋 메시지의 지라 키", "CLAUDE.md", "커밋 메시지에 지라 티켓 번호를 남긴다.", None),
        ("hook 스크립트의 설명 줄은 사람 말 꼴을 보지 않음", ".claude/hooks/guard.sh", "# 배포 전에는 사람에게 확인받는다", None),
        ("설정의 허용 목록 도구 이름", ".claude/settings.json", "\"allow\": [\"Bash(git push:*)\"]", None),
        ("exit 0", ".claude/hooks/guard.sh", "exit 0", None),
    ];
    assert_eq!(rows.len(), 44);
    for (note, file, line, kind) in rows {
        let fenced = note.starts_with("울타리");
        let text = if fenced { format!("# 규칙\n```sh\n{}\n```\n", line) } else { line.to_string() };
        let p = harness_points(&[(file.to_string(), text)]);
        assert_eq!(p.first().map(|x| x.kind.as_str()), kind, "{}", note);
        if let Some(k) = kind {
            let f = p.first().unwrap();
            assert_eq!((f.file.as_str(), f.line, f.kind.as_str()), (file, if fenced { 3 } else { 1 }, k), "{}", note);
        }
    }
}

#[test]
fn 울타리() {
    let p = harness_points(&[("CLAUDE.md".to_string(), "```\n사람에게 확인받고 진행한다\n```\n밖의 줄: 사람에게 묻는다\n".to_string())]);
    let places: Vec<String> = p.iter().map(|x| x.place()).collect();
    assert_eq!(places, ["CLAUDE.md:4"]);
}

#[test]
fn 한계() {
    // 사람·답 낱말 없이 쓴 기다리기, 줄을 넘는 문장, 스크립트가 바깥 파일을 불러 막는 hook 은 놓친다(진행 상황 ⑫-7b 의 한계 표)
    for t in ["다음 단계로 가기 전에 잠깐 멈춘다.", "결정은 사람이\n한다.", "python3 .claude/hooks/check.py"] {
        assert!(harness_points(&[("CLAUDE.md".to_string(), t.to_string())]).is_empty(), "{}", t);
    }
}

// MARK: 읽는 파일과 지문

#[test]
fn 파일() {
    let rows: &[(&str, &str, bool)] = &[
        ("뿌리 CLAUDE.md", "CLAUDE.md", true),
        ("대소문자가 다른 이름(APFS 는 같은 파일)", "Claude.local.md", true),
        ("깊이 4 의 CLAUDE.md(지문과 같은 자리)", "a/b/c/d/CLAUDE.md", true),
        ("깊이 5 는 보지 않음", "a/b/c/d/e/CLAUDE.md", false),
        (".mcp.json", ".mcp.json", true),
        (".claude 규칙", ".claude/rules/a.md", true),
        (".claude 설정", ".claude/settings.json", true),
        (".claude 개인 설정", ".claude/settings.local.json", true),
        (".claude hook 스크립트(확장자 없음)", ".claude/hooks/guard", true),
        (".claude 안의 build 이름 폴더도 읽음", ".claude/skills/build/SKILL.md", true),
        ("대문자 .CLAUDE", ".CLAUDE/Rules/A.md", true),
        ("하위 폴더의 .claude", "sub/.claude/rules/a.md", true),
        (".claude/worktrees 는 뺌", ".claude/worktrees/w/CLAUDE.md", false),
        (".claude/.cc-writes 는 뺌", ".claude/.cc-writes/x.md", false),
        (".claude 안의 그림은 글이 아님", ".claude/skills/a/logo.png", false),
        ("node_modules 안", "node_modules/x/CLAUDE.md", false),
        ("하네스가 아닌 문서", "docs/faq.md", false),
        ("상위 폴더 꼴", "../CLAUDE.md", false),
    ];
    assert_eq!(rows.len(), 18, "표 줄 수가 바뀌었다");
    for &(note, path, wanted) in rows {
        assert_eq!(harness_file_wanted(path), wanted, "{}", note);
    }
    // NFD 로 쓴 폴더 이름 아래(바깥 행으로 두어 임시 글을 빌리지 않는다)
    let nfd_path: String = "문서/CLAUDE.md".nfd().collect();
    assert!(harness_file_wanted(&nfd_path), "{}", nfd_path);
}

// 가짜 하네스 자리: 경로 → 글 또는 종류(link·large)
struct FakeHarness {
    files: BTreeMap<String, String>,
    kinds: BTreeMap<String, String>,
}

impl HarnessSource for FakeHarness {
    fn walk(&self, visit: &mut dyn FnMut(&str) -> WalkStep) {
        // 폴더 이름들도 넘긴다(어댑터의 enumerator 처럼)
        let mut dirs: Vec<String> = Vec::new();
        let mut cands: Vec<String> = self.files.keys().cloned().chain(self.kinds.keys().cloned()).collect();
        for p in &cands {
            let parts: Vec<&str> = p.split('/').collect();
            let mut cur = String::new();
            for part in &parts[..parts.len() - 1] {
                cur = if cur.is_empty() { (*part).to_string() } else { format!("{}/{}", cur, part) };
                dirs.push(cur.clone());
            }
        }
        cands.append(&mut dirs);
        cands.sort();
        cands.dedup();
        let mut skipped: Vec<String> = Vec::new();
        for p in cands {
            if skipped.iter().any(|s| p.starts_with(&format!("{}/", s))) {
                continue;
            }
            match visit(&p) {
                WalkStep::Next => continue,
                WalkStep::Skip => skipped.push(p),
                WalkStep::Stop => return,
            }
        }
    }

    fn read(&self, rel: &str, max: usize) -> Option<HarnessFileRead> {
        if let Some(k) = self.kinds.get(rel) {
            return Some(HarnessFileRead::Other(k.clone()));
        }
        let t = self.files.get(rel)?;
        let b = t.as_bytes();
        if b.len() > max {
            Some(HarnessFileRead::Other(format!("large:{}", b.len())))
        } else {
            Some(HarnessFileRead::Data(b.to_vec()))
        }
    }
}

#[test]
fn 모으기() {
    let mut files = BTreeMap::new();
    files.insert("CLAUDE.md".to_string(), "사람에게 묻는다".to_string());
    files.insert(".claude/rules/big.md".to_string(), "가".repeat(100_000));
    files.insert(".claude/settings.json".to_string(), "{}".to_string());
    files.insert("node_modules/x/CLAUDE.md".to_string(), "묻는다".to_string());
    files.insert(".claude/worktrees/w/CLAUDE.md".to_string(), "묻는다".to_string());
    files.insert("a/b/c/d/e/f/CLAUDE.md".to_string(), "묻는다".to_string());
    files.insert("src/a.txt".to_string(), "x".to_string());
    let mut kinds = BTreeMap::new();
    kinds.insert(".claude/rules/link.md".to_string(), "link:/etc/hosts".to_string());
    let src = FakeHarness { files, kinds };
    let r = harness_collect(&src);
    let keys: Vec<String> = r.texts.keys().cloned().collect();
    assert_eq!(keys, [".claude/settings.json", "CLAUDE.md"]);
    let paths: Vec<&str> = r.marks.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        [".claude/rules/big.md", ".claude/rules/link.md", ".claude/settings.json", "CLAUDE.md"]
    );
    assert_eq!(
        r.marks.iter().find(|m| m.path == ".claude/rules/link.md").map(|m| m.digest.as_str()),
        Some("link:/etc/hosts")
    );
    assert_eq!(r.skipped.len(), 2);
    assert!(r.skipped.iter().any(|s| s.contains("바로 가기")), "{}", r.skipped.join(" / "));
    assert!(r.skipped.iter().any(|s| s.contains("크기 상한")), "{}", r.skipped.join(" / "));
    assert_eq!(harness_texts(&src), vec![(".claude/settings.json".to_string(), "{}".to_string()), ("CLAUDE.md".to_string(), "사람에게 묻는다".to_string())]);
}

#[test]
fn 상한() {
    let mut files = BTreeMap::new();
    for i in 0..320 {
        files.insert(format!(".claude/rules/r{}.md", i), "x".to_string());
    }
    let r = harness_collect(&FakeHarness { files, kinds: BTreeMap::new() });
    assert_eq!(r.marks.len(), HARNESS_FILE_MAX);
    assert!(r.capped);
}

#[test]
fn 시나리오자리() {
    // 출발 전 자율주행 가지에 시나리오로 커밋해도 되는 자리: 하네스·.git 은 아님(접은 꼴로)
    assert!(seed_scenario_path_ok(".autopilot/scenarios/u1/check.sh"));
    assert!(seed_scenario_path_ok("sub/.autopilot/scenarios/p/u1/a.py"));
    for bad in [
        ".claude/hooks/guard.sh",
        ".Claude/settings.json",
        "CLAUDE.md",
        "docs/Claude.md",
        ".mcp.json",
        ".git/hooks/pre-commit",
        ".GIT/config",
        "../x.sh",
        "/etc/x",
        "a/.claude/rules/r.md",
        ".gitattributes",
    ] {
        assert!(!seed_scenario_path_ok(bad), "{}", bad);
    }
}

#[test]
fn 경로넘기() {
    // 보안 검토 지적 둘(⑫-7b 자동 보안 검토 MEDIUM 7·8, 이어 가기 시나리오 길): 고정본 기록 글자의 ".." 로 다른 파일을 읽음, 단위 id 의 "../" 로 결과 폴더 밖에 씀
    assert_eq!(
        pinned_scenario_path("/R/runs/a", 2, 1, ".autopilot/scenarios/u2/check.sh"),
        Some("/R/runs/a/pinned/checks/scenarios/2/1/check.sh".to_string())
    );
    for bad in ["..", ".", "", "a\u{1}b"] {
        assert_eq!(pinned_scenario_path("/R", 1, 1, bad), None, "{}", bad);
    }
    assert_eq!(pinned_scenario_path("/R", 0, 1, "a.sh"), None);
    assert_eq!(
        pinned_scenario_path("/R", 1, 1, "x/../../../../private/set/autopilot.md"),
        Some("/R/pinned/checks/scenarios/1/1/autopilot.md".to_string())
    );
    for ok in ["u1", "c2", "u-3.b"] {
        assert!(unit_dir_ok(ok), "{}", ok);
    }
    for bad in ["../../zz-escape", "..", ".", ".hidden", "a/b", "", "a".repeat(61).as_str()] {
        assert!(!unit_dir_ok(bad), "{}", bad);
    }
}

#[test]
fn 견주기() {
    // 지문 견주기: 접은 경로, 바뀜·더함·뺌, 세트 기준 없으면 그대로
    let a = vec![
        HarnessFileMark { path: "CLAUDE.md".to_string(), digest: "1".to_string(), size: 3 },
        HarnessFileMark { path: ".claude/rules/a.md".to_string(), digest: "2".to_string(), size: 3 },
        HarnessFileMark { path: "x.md".to_string(), digest: "9".to_string(), size: 1 },
    ];
    let b = vec![
        HarnessFileMark { path: "claude.md".to_string(), digest: "1".to_string(), size: 3 },
        HarnessFileMark { path: ".claude/rules/a.md".to_string(), digest: "3".to_string(), size: 3 },
        HarnessFileMark { path: ".mcp.json".to_string(), digest: "4".to_string(), size: 2 },
    ];
    let diff = harness_changes(&a, &b);
    let got: Vec<(&str, &str)> = diff.iter().map(|c| (c.path.as_str(), c.change.as_str())).collect();
    assert_eq!(got, [(".claude/rules/a.md", "changed"), (".mcp.json", "added"), ("x.md", "removed")]);
    assert!(harness_changes(&a, &a).is_empty());
    let base = HarnessBaseline { marks: a.clone(), points: 4, at: 1_793_340_000.0, run: "zz-toy-1".to_string() };
    let bytes = harness_baseline_json(&base).to_string();
    assert_eq!(harness_baseline_parse(Some(bytes.as_bytes())), Some(base.clone()));
    assert_eq!(harness_baseline_parse(Some("{\"format\":\"다른 것\",\"files\":[]}".as_bytes())), None);
    let mut read = HarnessRead { marks: b.clone(), ..Default::default() };
    read.texts.insert("claude.md".to_string(), "묻는다".to_string());
    let same_baseline = HarnessBaseline { marks: b.clone(), points: 1, at: 0.0, run: String::new() };
    let same = harness_state(&read, Some(&same_baseline));
    assert!(!same.changed() && same.has_baseline && same.points.len() == 1);
    let ch = harness_state(&read, Some(&base));
    assert!(ch.changed() && ch.changes.len() == 3);
    let none = harness_state(&read, None);
    assert!(!none.changed() && !none.has_baseline);
    assert!(harness_state_note(&none, true).contains("기준이 없어 그대로로 봅니다"));
    assert!(harness_state_note(&ch, true).contains("달라진 파일 3개"));
    // 원본은 apFNV64Data 가 apFNV64(문자열 변형)와 같음을 잡는다 — 이쪽은 알려진 벡터로 고정한다
    assert_eq!(fnv64_data(b""), "cbf29ce484222325");
    assert_eq!(fnv64_data(b"a"), "af63dc4c8601ec8c");
}

// 지시가 다뤘나 경계 표: 지시 글자 → 다루지 않은 확인 지점 자리
fn cover_points() -> Vec<HarnessPoint> {
    vec![
        HarnessPoint { file: "CLAUDE.md".to_string(), line: 3, kind: "confirm".to_string(), text: "x".to_string() },
        HarnessPoint {
            file: ".claude/rules/release.md".to_string(),
            line: 4,
            kind: "approve".to_string(),
            text: "y".to_string(),
        },
    ]
}

#[test]
fn 경계() {
    let rows: &[(&str, &str, &[&str])] = &[
        ("둘 다 이름으로", "- CLAUDE.md:3 보류\n- .claude/rules/release.md:4 보류", &[]),
        ("대소문자를 접음", "claude.md:3 · .Claude/Rules/Release.md:4", &[]),
        ("CLAUDE.md:30 은 :3 이 아님", "CLAUDE.md:30 · .claude/rules/release.md:4", &["CLAUDE.md:3"]),
        ("docs/CLAUDE.md:3 은 뿌리 CLAUDE.md 가 아님", "docs/CLAUDE.md:3 · .claude/rules/release.md:4", &["CLAUDE.md:3"]),
        ("줄 범위", "CLAUDE.md:2-4 · .claude/rules/release.md:3~5", &[]),
        ("쉼표 목록", "CLAUDE.md:1,3 · .claude/rules/release.md:9", &[".claude/rules/release.md:4"]),
        ("파일 이름만", "CLAUDE.md 와 release.md 를 따른다", &["CLAUDE.md:3", ".claude/rules/release.md:4"]),
    ];
    assert_eq!(rows.len(), 7);
    for (note, instruction, missing) in rows {
        let miss: Vec<String> = harness_missing(&cover_points(), instruction).iter().map(|p| p.place()).collect();
        assert_eq!(miss, *missing, "{}", note);
    }
}

#[test]
fn 정책() {
    // 정책 카드: 바깥 쓰기·배포 확인만, id 는 줄이 밀려도 같음
    let w = HarnessPoint {
        file: ".claude/skills/t/SKILL.md".to_string(),
        line: 5,
        kind: "write".to_string(),
        text: "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.".to_string(),
    };
    let d = HarnessPoint {
        file: "CLAUDE.md".to_string(),
        line: 4,
        kind: "confirm".to_string(),
        text: "배포 전에는 사람에게 확인받고 진행한다.".to_string(),
    };
    let a = HarnessPoint {
        file: "CLAUDE.md".to_string(),
        line: 5,
        kind: "ask".to_string(),
        text: "모호하면 사용자에게 묻는다.".to_string(),
    };
    assert_eq!(harness_policy_class(&w), Some("write"));
    assert_eq!(harness_policy_class(&d), Some("deploy"));
    assert_eq!(harness_policy_class(&a), None);
    let mut moved = w.clone();
    moved.line = 9;
    assert_eq!(harness_policy_id(&w), harness_policy_id(&moved));
    assert!(harness_policy_id(&w).starts_with(HARNESS_POLICY_PREFIX));
    assert_ne!(harness_policy_id(&w), harness_policy_id(&d));
    let c = harness_policy_card(&w, true).expect("쓰기 지점의 정책 카드");
    assert_eq!(c.recommended, "defer");
    let keys: Vec<&str> = c.options.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, ["defer", "record", "drop"]);
    assert!(c.why.contains("다루지 않았습니다"), "{}", c.why);
    assert!(!c.title.contains("—"), "{}", c.title);
    let dc = harness_policy_card(&d, false).expect("배포 확인 지점의 정책 카드");
    assert_eq!(dc.options.iter().map(|o| o.key.as_str()).collect::<Vec<&str>>(), ["defer", "drop"]);
    assert!(harness_policy_card(&a, true).is_none(), "묻기 지점은 정책 카드가 없다");
}

// MARK: 추가 — Swift 시험에 없는 이 이식의 몫(Foundation 과 다른 점·경계·원본 동작 고정)

#[test]
fn 추가_카드_id_꼴() {
    // 카드 id 는 harness- 접두와 접은 파일·줄 글자(원본 apHarnessCardId)
    let p = HarnessPoint { file: "CLAUDE.md".to_string(), line: 4, kind: "confirm".to_string(), text: "x".to_string() };
    let id = harness_card_id(&p);
    assert!(id.starts_with("harness-CLAUDE-md-4"), "{}", id);
    assert!(id.chars().all(|c| c.is_alphanumeric() || c == '-'), "{}", id);
    assert_ne!(harness_card_id(&p), harness_card_id(&HarnessPoint { line: 5, ..p.clone() }));
}

#[test]
fn 추가_금지줄_인용() {
    // 지시에서 다뤄진 자리의 줄은 빠지고(금지가 아니라), 지시에 남는 금지 줄은 도구 꼴이 그대로 남아 잡힌다 — 깊은 검사의 몫은 이어 쓰기 검사기(apInstructionCheck, 미이식)
    let harness = cover_points();
    let covering = "- CLAUDE.md:3: 보류하고 git push origin live 도 한다.\n";
    let clean = instruction_clean(covering, &harness);
    assert!(clean.forbidden.is_empty() && !clean.dropped.is_empty(), "dropped={:?} forbidden={:?}", clean.dropped, clean.forbidden);
    let loose = "단위마다 작업이 끝나면 git push 로 올린다.\n";
    let bad = instruction_forbidden_lines(loose, &harness);
    assert_eq!(bad.len(), 1, "{}", bad.join(" | "));
    assert!(bad[0].contains("git push"), "{}", bad[0]);
}
