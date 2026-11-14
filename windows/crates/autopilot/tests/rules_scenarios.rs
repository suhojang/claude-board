//! 시험 실행기 규칙(`rules::scenarios`) 시험. 원본: `Tests/AutopilotRulesTests/ScenariosTests.swift` 의 `ScenarioRuleTests`·`ScenarioEnvTests`·`JudgeRunTests`.
//! 이 파일이 옮긴 Swift 시험 11개(같은 이름): `읽기`(표 9줄) · `배열` · `수용기준` · `해석기`(표 9줄) · `환경` · `옛판정` · `여럿` · `멈춤` · `지문` · `요약` · `카드`,
//! 가짜 `FakeProbe`·`FakeRunner`·`pinned` 도 같이 옮겼다. 표의 글자 값은 `convert.py` 로 Swift 파일에서 기계로 옮겼다(`#"…"#` → `r#"…"#`).
//! 마지막 줄 「상한 30개」만 원본이 코드로 만드는 입력이라 시험 함수 안에서 같은 식으로 만든다. 표 길이를 세어 줄이 빠지면 실패한다.
//! `JudgeSandboxTests` 5개(`거르개`·`골든`·`차례`·`네트워크`·`ipc`)는 macOS Seatbelt 프로필 시험이라 이식하지 않는다(plan.md 의 이식불가 행, 원본 `JudgeSandbox.swift` 몫).
//! 끝의 「추가」 절은 Swift 시험에 없는 이 이식의 몫(Foundation 과 다른 점·경계·원본 동작 고정)이다.

use claude_board_autopilot::rules::scenarios::{
    criteria_gaps, criterion_answer, criterion_card, criterion_card_id, criterion_id_ok, criterion_json, env_check, env_problem, env_row_json, judge_steps,
    judge_summary, kind_label, parse_criteria, parse_scenarios, prerun_scenario_card, run_judge, scenario_criterion_problems, scenario_json, scenario_label,
    scenario_path_ok, scenario_run, tool_label, BrowserDriver, Criterion, JudgeRunner, JudgeStep, PinnedScenario, Scenario, ScenarioKind, StepResult,
    ToolNeed, ToolProbe, CRITERION_DROP, CRITERION_MODEL_JUDGE, SCENARIO_MAX,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

struct ParseRow {
    note: &'static str,
    json: &'static str,
    files: &'static [&'static str],
    problems: usize,
}

struct InterpreterRow {
    note: &'static str,
    file: &'static str,
    kind: ScenarioKind,
    driver: Option<BrowserDriver>,
    head: &'static str,
    argv: &'static [&'static str],
    needs: Vec<ToolNeed>,
    problem: bool,
}

fn sh(file: &str) -> Scenario {
    Scenario::new(file, ScenarioKind::Shell, None, None)
}

fn sh_crit(file: &str, crit: &str) -> Scenario {
    Scenario::new(file, ScenarioKind::Shell, Some(crit), None)
}

const PARSE_ROWS: [ParseRow; 8] = [
    ParseRow { note: "글자 하나는 셸 시나리오", json: r#"["t/a.sh"]"#, files: &["t/a.sh"], problems: 0 },
    ParseRow { note: "브라우저·기준·드라이버", json: r#"[{"file": "t/p.mjs", "kind": "browser", "criterion": "AC-2", "driver": "playwright"}]"#, files: &["t/p.mjs"], problems: 0 },
    ParseRow { note: "절대 경로는 받지 않음", json: r#"["/etc/passwd"]"#, files: &[], problems: 1 },
    ParseRow { note: "..·~·빈 조각·따옴표·. 조각은 받지 않음", json: r#"["../x.sh", "~/x.sh", "a//b.sh", "a\"b.sh", "./a.sh"]"#, files: &[], problems: 5 },
    ParseRow { note: "kind 꼴", json: r#"[{"file": "a.sh", "kind": "gui"}]"#, files: &[], problems: 1 },
    ParseRow { note: "driver 는 브라우저만", json: r#"[{"file": "a.sh", "driver": "chrome"}]"#, files: &[], problems: 1 },
    ParseRow { note: "criterion 꼴", json: r#"[{"file": "a.sh", "criterion": "기준 1"}]"#, files: &[], problems: 1 },
    ParseRow { note: "같은 파일 두 번", json: r#"["a.sh", {"file": "a.sh"}]"#, files: &["a.sh"], problems: 1 },
];

// INTERPRETER ROWS (needs_N is replaced below)
fn interpreter_rows() -> Vec<InterpreterRow> {
    vec![
        InterpreterRow { note: "#! 절대 경로", file: "a", kind: ScenarioKind::Shell, driver: None, head: "#!/bin/bash -e\necho", argv: &["/bin/bash", "-e"], needs: vec![ToolNeed::Program("/bin/bash".to_string())], problem: false },
        InterpreterRow { note: "#! env 는 다음 이름을 PATH 에서", file: "a", kind: ScenarioKind::Shell, driver: None, head: "#!/usr/bin/env python3\n", argv: &["/usr/bin/env", "python3"], needs: vec![ToolNeed::Program("/usr/bin/env".to_string()), ToolNeed::Command("python3".to_string())], problem: false },
        InterpreterRow { note: "#! env -S", file: "a", kind: ScenarioKind::Shell, driver: None, head: "#!/usr/bin/env -S node --no-warnings\n", argv: &["/usr/bin/env", "-S", "node", "--no-warnings"], needs: vec![ToolNeed::Program("/usr/bin/env".to_string()), ToolNeed::Command("node".to_string())], problem: false },
        InterpreterRow { note: "#! 상대 해석기는 받지 않음", file: "a.sh", kind: ScenarioKind::Shell, driver: None, head: "#!bash\n", argv: &[], needs: vec![], problem: true },
        InterpreterRow { note: "확장자 .sh", file: "t/a.sh", kind: ScenarioKind::Shell, driver: None, head: "curl …", argv: &["/bin/sh"], needs: vec![ToolNeed::Program("/bin/sh".to_string())], problem: false },
        InterpreterRow { note: "확장자 .mjs", file: "t/a.mjs", kind: ScenarioKind::Shell, driver: None, head: "import x", argv: &["/usr/bin/env", "node"], needs: vec![ToolNeed::Program("/usr/bin/env".to_string()), ToolNeed::Command("node".to_string())], problem: false },
        InterpreterRow { note: "모르는 확장자", file: "t/a.rb", kind: ScenarioKind::Shell, driver: None, head: "puts 1", argv: &[], needs: vec![], problem: true },
        InterpreterRow { note: "브라우저 chrome", file: "p.sh", kind: ScenarioKind::Browser, driver: None, head: "#!/bin/sh\n", argv: &["/bin/sh"], needs: vec![ToolNeed::Program("/bin/sh".to_string()), ToolNeed::Browser], problem: false },
        InterpreterRow { note: "브라우저 playwright", file: "p.mjs", kind: ScenarioKind::Browser, driver: Some(BrowserDriver::Playwright), head: "import { chromium } from 'playwright'", argv: &["/usr/bin/env", "node"], needs: vec![ToolNeed::Program("/usr/bin/env".to_string()), ToolNeed::Command("node".to_string()), ToolNeed::NodeModule("playwright".to_string())], problem: false },
    ]
}

fn check_parse_row(note: &str, raw: &Value, files: &[String], problems: usize) {
    let (items, ps) = parse_scenarios(Some(raw), 2);
    let got: Vec<&str> = items.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(got, files, "{note}");
    assert_eq!(ps.len(), problems, "{note}: {ps:?}");
    assert!(ps.iter().all(|p| p.starts_with("단위 2 의 ")), "{note}: {ps:?}");
}

#[test]
fn 읽기() {
    assert_eq!(PARSE_ROWS.len(), 8, "표 줄이 빠졌다");
    for r in PARSE_ROWS.iter() {
        let raw: Value = serde_json::from_str(r.json).unwrap_or_else(|e| panic!("{}: {e}", r.note));
        let files: Vec<String> = r.files.iter().map(|s| s.to_string()).collect();
        check_parse_row(r.note, &raw, &files, r.problems);
    }
    // 「상한 30개」 — Swift: "[" + (1...31).map { "\"s\($0).sh\"" }.joined(separator: ",") + "]", files: (1...30).map { "s\($0).sh" }, problems: 1
    let json31 = format!("[{}]", (1..=31).map(|i| format!("\"s{i}.sh\"")).collect::<Vec<_>>().join(","));
    let raw: Value = serde_json::from_str(&json31).unwrap();
    let files: Vec<String> = (1..=30).map(|i| format!("s{i}.sh")).collect();
    check_parse_row("상한 30개", &raw, &files, 1);
}

#[test]
fn 배열() {
    assert_eq!(parse_scenarios(Some(&json!("a.sh")), 1).1, vec!["단위 1 의 scenarios 는 배열이어야 합니다".to_string()]);
    assert!(parse_scenarios(None, 1).0.is_empty());
    assert_eq!(parse_scenarios(Some(&json!([{"file": "p.js", "kind": "browser"}])), 1).0.first().and_then(|s| s.driver), Some(BrowserDriver::Chrome));
}

#[test]
fn 수용기준() {
    let raw = json!([{"id": "AC-1", "text": "예약을 만들면 201"}, {"id": "AC-2", "text": " 겹치면 409 "}, {"id": "AC-2", "text": "x"}, {"text": "id 없음"}, {"id": "AC-3", "text": ""}]);
    let (cs, ps) = parse_criteria(Some(&raw), 1);
    assert_eq!(cs, vec![Criterion::new("AC-1", "예약을 만들면 201"), Criterion::new("AC-2", "겹치면 409")]);
    assert_eq!(
        ps,
        vec![
            "단위 1 의 수용 기준 AC-2 가 두 번 있습니다".to_string(),
            "단위 1 의 수용 기준 4 에 id(영문·숫자·.-_)가 없습니다".to_string(),
            "단위 1 의 수용 기준 AC-3 에 text 가 없습니다".to_string(),
        ]
    );
    let ss = vec![sh_crit("a.sh", "AC-1"), sh_crit("b.sh", "AC-9")];
    assert_eq!(criteria_gaps(&cs, &ss), vec![Criterion::new("AC-2", "겹치면 409")]);
    assert_eq!(scenario_criterion_problems(&ss, &cs, 1), vec!["단위 1 의 시나리오 b.sh 가 가리킨 수용 기준 AC-9 가 criteria 에 없습니다".to_string()]);
    assert!(scenario_criterion_problems(&ss, &[], 1).is_empty()); // 기준을 적지 않은 단위는 이름표로만
    assert_eq!(criterion_answer(&cs[0], &ss, Some("drop_criterion")), ("scenario".to_string(), false));
    assert_eq!(criterion_answer(&cs[1], &ss, Some("model_judge")), ("model_judge".to_string(), false));
    assert_eq!(criterion_answer(&cs[1], &ss, None), ("drop_criterion".to_string(), true));
    assert_eq!(criterion_answer(&cs[1], &ss, Some("keep")), ("drop_criterion".to_string(), true));
    let card = criterion_card("예약 API", &cs[1]);
    assert_eq!(card.recommended, "drop_criterion");
    assert_eq!(card.options.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(), vec!["drop_criterion", "model_judge"]);
    let all: String = card.title.clone() + &card.why + &card.options.iter().map(|o| format!("{}{}", o.label, o.note)).collect::<String>();
    assert!(!all.contains('—'));
    assert_eq!(criterion_card_id("u2", "AC-2"), "criterion-u2-AC-2");
}

#[test]
fn 해석기() {
    let rows = interpreter_rows();
    assert_eq!(rows.len(), 9, "표 줄이 빠졌다");
    for r in rows {
        let x = scenario_run(&Scenario::new(r.file, r.kind, None, r.driver), r.head);
        let want: Vec<String> = r.argv.iter().map(|s| s.to_string()).collect();
        assert_eq!(x.interpreter, want, "{}", r.note);
        assert_eq!(x.needs, r.needs, "{}", r.note);
        assert_eq!(x.problem.is_some(), r.problem, "{}", r.note);
    }
}

// MARK: 가짜 어댑터(원본 `FakeProbe`·`FakeRunner`·`pinned`)

struct FakeProbe {
    programs: HashSet<&'static str>,
    commands: HashMap<&'static str, &'static str>,
    browser_path: Option<&'static str>,
    modules: HashSet<&'static str>,
}

impl FakeProbe {
    fn new() -> Self {
        FakeProbe {
            programs: HashSet::from(["/bin/sh", "/bin/bash", "/usr/bin/env"]),
            commands: HashMap::from([("python3", "/opt/homebrew/bin/python3"), ("node", "/opt/homebrew/bin/node")]),
            browser_path: Some("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            modules: HashSet::new(),
        }
    }
}

impl ToolProbe for FakeProbe {
    fn program(&self, path: &str) -> bool {
        self.programs.contains(path)
    }
    fn command(&self, name: &str) -> Option<String> {
        self.commands.get(name).map(|s| s.to_string())
    }
    fn browser(&self) -> Option<String> {
        self.browser_path.map(str::to_string)
    }
    fn node_module(&self, name: &str, _dir: &str) -> bool {
        self.modules.contains(name)
    }
}

struct FakeRunner {
    codes: HashMap<String, i32>,
    digests: HashMap<String, String>,
    interrupt_at: Option<String>,
    ran: RefCell<Vec<String>>,
}

impl FakeRunner {
    fn new(codes: &[(&str, i32)]) -> Self {
        FakeRunner {
            codes: codes.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            digests: HashMap::new(),
            interrupt_at: None,
            ran: RefCell::new(vec![]),
        }
    }
    fn ran(&self) -> Vec<String> {
        self.ran.borrow().clone()
    }
}

impl JudgeRunner for FakeRunner {
    fn digest(&self, path: &str) -> Option<String> {
        self.digests.get(path).cloned()
    }
    fn run(&self, step: &JudgeStep) -> StepResult {
        self.ran.borrow_mut().push(step.name.clone());
        if self.interrupt_at.as_deref() == Some(step.name.as_str()) {
            return StepResult { interrupted: true, ..StepResult::new(-1) };
        }
        let c = self.codes.get(&step.name).copied().unwrap_or(0);
        StepResult {
            sec: 1.25,
            tail: format!("{} 출력\n끝 줄", step.name),
            files: if step.kind == "browser" && c == 0 { vec!["shot.png".to_string(), "dom.html".to_string()] } else { vec![] },
            ..StepResult::new(c)
        }
    }
}

fn pinned_full(file: &str, kind: ScenarioKind, crit: Option<&str>, sha: Option<&str>, head: &str) -> PinnedScenario {
    let s = Scenario::new(file, kind, crit, None);
    let base = file.rsplit('/').next().unwrap();
    PinnedScenario { run: scenario_run(&s, head), scenario: s, pinned: format!("/R/pinned/checks/scenarios/1/{base}"), sha256: sha.map(str::to_string) }
}

fn pinned(file: &str) -> PinnedScenario {
    pinned_full(file, ScenarioKind::Shell, None, None, "#!/bin/sh\n")
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn 환경() {
    let shell = sh("t/a.py");
    let ok = env_check(1, &shell, &scenario_run(&shell, ""), "/w", &FakeProbe::new());
    assert!(ok.ok() && env_problem(&ok).is_none());
    assert_eq!(ok.tools.iter().map(|t| t.found.clone()).collect::<Vec<_>>(), vec![Some("/usr/bin/env".to_string()), Some("/opt/homebrew/bin/python3".to_string())]);

    let pw = Scenario::new("t/p.mjs", ScenarioKind::Browser, None, Some(BrowserDriver::Playwright));
    let miss = env_check(2, &pw, &scenario_run(&pw, ""), "/w", &FakeProbe::new());
    assert!(!miss.ok());
    assert_eq!(
        env_problem(&miss).as_deref(),
        Some("단위 2 의 시나리오 t/p.mjs(브라우저)를 돌릴 도구가 없습니다: node 모듈 playwright 을(를) 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 도구를 깐 뒤 다시 확인하거나 시나리오를 바꿉니다")
    );

    let ch = Scenario::new("t/p.sh", ScenarioKind::Browser, None, None);
    let mut none = FakeProbe::new();
    none.browser_path = None;
    let nb = env_check(3, &ch, &scenario_run(&ch, "#!/bin/sh"), "/w", &none);
    assert!(env_problem(&nb).unwrap().contains("브라우저(Google Chrome·Chromium·Microsoft Edge) 을(를) 찾지 못했습니다"));
    let j = env_row_json(&nb);
    assert!(j["ok"] == json!(false) && j["driver"] == json!("chrome") && !j["why"].as_str().unwrap().is_empty());

    let rb = sh("a.rb");
    let unk = env_check(4, &rb, &scenario_run(&rb, ""), "/w", &FakeProbe::new());
    assert!(env_problem(&unk).unwrap().starts_with("단위 4 의 시나리오 a.rb(셸)를 돌리지 못합니다: 돌리는 법을 모릅니다"));
    for r in [&miss, &nb, &unk] {
        assert!(!env_problem(r).unwrap().contains('—'));
    }
}

#[test]
fn 옛판정() {
    let steps = judge_steps(Some("test -f a.txt"), "/bin/zsh", &[]);
    assert_eq!(steps.iter().map(|s| s.argv.clone()).collect::<Vec<_>>(), vec![strs(&["/bin/zsh", "-l", "-c", "test -f a.txt"])]);
    assert_eq!(steps.first().map(|s| s.evidence.as_str()), Some(""));
    let r = FakeRunner::new(&[("check", 3)]);
    let o = run_judge(&steps, &r);
    assert!(!o.passed() && o.code() == 3 && o.tail() == "check 출력\n끝 줄");
    assert_eq!(o.describe(Some("test -f a.txt")), "test -f a.txt");
    assert!(run_judge(&steps, &FakeRunner::new(&[])).passed());
}

#[test]
fn 여럿() {
    let steps = judge_steps(Some("true"), "/bin/sh", &[pinned_full("s/a.sh", ScenarioKind::Shell, Some("AC-1"), None, "#!/bin/sh\n"), pinned_full("s/b.sh", ScenarioKind::Browser, None, None, "#!/bin/sh\n")]);
    assert_eq!(steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["scenario-1", "scenario-2", "check"]);
    assert_eq!(steps[0].argv, strs(&["/bin/sh", "/R/pinned/checks/scenarios/1/a.sh"]));
    assert_eq!(steps[0].label, "시나리오 s/a.sh(셸 · 수용 기준 AC-1)");
    let r = FakeRunner::new(&[("scenario-1", 1)]);
    let o = run_judge(&steps, &r);
    assert_eq!(r.ran(), strs(&["scenario-1", "scenario-2", "check"])); // 실패해도 나머지를 돌린다
    assert!(!o.passed() && o.code() == 1);
    assert_eq!(o.tail(), "[시나리오 s/a.sh(셸 · 수용 기준 AC-1)] 종료 코드 1\nscenario-1 출력\n끝 줄");
    assert_eq!(o.describe(Some("true")), "시나리오 2개 + 판정 명령 `true`");
    assert!(run_judge(&steps, &FakeRunner::new(&[])).passed());
}

#[test]
fn 멈춤() {
    let steps = judge_steps(Some("true"), "/bin/sh", &[pinned("a.sh"), pinned("b.sh")]);
    let mut r = FakeRunner::new(&[]);
    r.interrupt_at = Some("scenario-1".to_string());
    let o = run_judge(&steps, &r);
    assert!(r.ran() == strs(&["scenario-1"]) && o.interrupted() && !o.passed());
}

#[test]
fn 지문() {
    let steps = judge_steps(None, "/bin/sh", &[pinned_full("a.sh", ScenarioKind::Shell, None, Some("aaa"), "#!/bin/sh\n"), pinned_full("b.sh", ScenarioKind::Shell, None, Some("bbb"), "#!/bin/sh\n")]);
    let mut r = FakeRunner::new(&[]);
    r.digests = HashMap::from([
        ("/R/pinned/checks/scenarios/1/a.sh".to_string(), "aaa".to_string()),
        ("/R/pinned/checks/scenarios/1/b.sh".to_string(), "바뀜".to_string()),
    ]);
    let o = run_judge(&steps, &r);
    assert_eq!(r.ran(), strs(&["scenario-1"]));
    assert!(!o.passed() && o.code() == -1);
    assert!(o.tail().starts_with("[시나리오 b.sh(셸)] 보안: 고정본이 출발 때와 달라 돌리지 않음\n보안: 판정 시나리오 고정본이 출발 때와 달라 돌리지 않았습니다(b.sh)"));
}

#[test]
fn 요약() {
    let b = |f: &str| pinned_full(f, ScenarioKind::Browser, None, None, "#!/bin/sh\n");
    let steps = judge_steps(None, "/bin/sh", &[b("p.sh"), b("q.sh")]);
    let o = run_judge(&steps, &FakeRunner::new(&[("scenario-2", 2)]));
    let s = judge_summary(&o, "샌드박스 안", 10.12345);
    assert!(s["passed"] == json!(false) && s["at"].as_f64() == Some(10.123) && s["sandbox"] == json!("샌드박스 안"));
    let parts = s["parts"].as_array().cloned().unwrap_or_default();
    assert_eq!(parts.len(), 2);
    assert!(parts[0]["screenshots"] == json!(["shot.png"]) && parts[0]["passed"] == json!(true) && parts[0]["sec"].as_f64() == Some(1.25));
    assert!(parts[1]["screenshots"] == json!([]) && parts[1]["note"] == json!("브라우저 시나리오가 스크린샷을 남기지 않았습니다"));
    assert!(parts[1]["exit"].as_i64() == Some(2) && parts[1]["evidence"] == json!("scenario-2"));
}

#[test]
fn 카드() {
    let c = prerun_scenario_card(2, "u2", "예약 API", "s/a.sh", Some("AC-1"));
    assert!(c["id"] == json!("prerun-u2-s/a.sh") && c["recommended"] == json!("fix_check") && c["scenario"] == json!("s/a.sh"));
    assert!(c["unit_id"] == json!("u2") && c["unit"].as_i64() == Some(2));
    assert_eq!(c["title"], json!("「예약 API」 의 시나리오 s/a.sh(수용 기준 AC-1) 가 일하기 전부터 통과합니다"));
}

// ---------------------------------------------------------------------------------------------------------------------
// 추가 — Swift 시험에 없는 것(원본 동작 고정·Foundation 과 다른 점·경계)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn 추가_경로_꼴() {
    assert!(scenario_path_ok("t/a.sh"));
    assert!(scenario_path_ok("a.sh"));
    assert!(!scenario_path_ok(""));
    assert!(!scenario_path_ok("/a"));
    assert!(!scenario_path_ok("~a"));
    assert!(!scenario_path_ok("a/../b"));
    assert!(!scenario_path_ok("a/./b"));
    assert!(!scenario_path_ok("a/"));
    assert!(!scenario_path_ok("a\\b"));
    assert!(!scenario_path_ok("a\"b"));
    assert!(!scenario_path_ok("a\u{7f}b"));
    assert!(!scenario_path_ok("a\u{1f}b"));
    assert!(!scenario_path_ok("a\nb"));
    // 300 자(자소 묶음)까지: 결합 글자는 앞 글자와 한 글자로 센다
    assert!(scenario_path_ok(&"a".repeat(300)));
    assert!(!scenario_path_ok(&"a".repeat(301)));
    assert!(scenario_path_ok(&format!("{}\u{301}", "a".repeat(300))));
    // Foundation 과 다른 점: `/`+결합 글자는 바이트 기준이라 거절(원본은 받음, 더 엄격한 쪽)
    assert!(!scenario_path_ok("/\u{301}x"));
    assert!(!scenario_path_ok("~\u{301}x"));
}

#[test]
fn 추가_기준_id_꼴() {
    assert!(criterion_id_ok("AC-1"));
    assert!(criterion_id_ok("a.b_c-9"));
    assert!(!criterion_id_ok(""));
    assert!(!criterion_id_ok("기준 1"));
    assert!(!criterion_id_ok("a b"));
    assert!(criterion_id_ok(&"a".repeat(40)));
    assert!(!criterion_id_ok(&"a".repeat(41)));
    // Foundation 과 다른 점: ICU `$` 는 끝 줄바꿈 앞에서도 맞아 원본은 "AC-1\n" 을 받는다. 여기는 거절한다(더 엄격한 쪽)
    assert!(!criterion_id_ok("AC-1\n"));
}

#[test]
fn 추가_읽기_경계() {
    assert!(parse_scenarios(Some(&json!([])), 1).1.is_empty());
    // NSNull 은 배열도 글자도 아니다
    assert_eq!(parse_scenarios(Some(&Value::Null), 3).1, vec!["단위 3 의 scenarios 는 배열이어야 합니다".to_string()]);
    assert_eq!(parse_criteria(Some(&Value::Null), 3).1, vec!["단위 3 의 criteria 는 배열이어야 합니다".to_string()]);
    assert_eq!(parse_criteria(Some(&json!("x")), 1).1, vec!["단위 1 의 criteria 는 배열이어야 합니다".to_string()]);
    assert!(parse_criteria(None, 1).0.is_empty() && parse_criteria(None, 1).1.is_empty());
    // 글자도 객체도 아닌 칸은 file 이 없는 것
    let (items, ps) = parse_scenarios(Some(&json!([3, null, {"file": 3}])), 4);
    assert!(items.is_empty());
    assert_eq!(ps.len(), 3);
    assert_eq!(ps[0], "단위 4 의 시나리오 1 의 file 은 작업 폴더 기준 상대 경로(.. 없이)여야 합니다");
    // 값이 글자가 아닌 kind·driver·criterion 은 고칠 것
    assert_eq!(parse_scenarios(Some(&json!([{"file": "a.sh", "kind": 3}])), 1).1, vec!["단위 1 의 시나리오 1 의 kind 는 shell 또는 browser 입니다".to_string()]);
    assert_eq!(
        parse_scenarios(Some(&json!([{"file": "a.mjs", "kind": "browser", "driver": "firefox"}])), 1).1,
        vec!["단위 1 의 시나리오 1 의 driver 는 브라우저 시나리오에만, chrome 또는 playwright 입니다".to_string()]
    );
    assert_eq!(parse_scenarios(Some(&json!([{"file": "a.sh", "criterion": null}])), 1).1, vec!["단위 1 의 시나리오 1 의 criterion 은 수용 기준 id(영문·숫자·.-_)입니다".to_string()]);
    // 같은 파일 두 번의 고칠 것 글과 상한 초과 글
    assert_eq!(parse_scenarios(Some(&json!(["a.sh", "a.sh"])), 2).1, vec!["단위 2 의 시나리오 a.sh 가 두 번 있습니다".to_string()]);
    let many: Vec<Value> = (1..=SCENARIO_MAX + 1).map(|i| json!(format!("s{i}.sh"))).collect();
    let (items, ps) = parse_scenarios(Some(&Value::Array(many)), 2);
    assert_eq!((items.len(), ps), (SCENARIO_MAX, vec!["단위 2 의 시나리오가 31개입니다(30개까지)".to_string()]));
    // 상한 밖의 항목은 읽지 않는다: 31 번째가 틀려도 고칠 것은 상한 글 하나뿐
    let mut over: Vec<Value> = (1..=SCENARIO_MAX).map(|i| json!(format!("s{i}.sh"))).collect();
    over.push(json!("/bad"));
    assert_eq!(parse_scenarios(Some(&Value::Array(over)), 2).1.len(), 1);
    // 브라우저는 driver 가 없으면 chrome, 셸은 늘 없음
    let (items, _) = parse_scenarios(Some(&json!([{"file": "a.sh", "kind": "shell"}, {"file": "b.sh", "kind": "browser", "driver": "playwright", "criterion": "AC-1"}])), 1);
    assert_eq!(items[0].driver, None);
    assert_eq!(items[1], Scenario::new("b.sh", ScenarioKind::Browser, Some("AC-1"), Some(BrowserDriver::Playwright)));
    assert_eq!(Scenario::new("a.sh", ScenarioKind::Shell, None, Some(BrowserDriver::Playwright)).driver, None);
}

#[test]
fn 추가_수용_기준_경계() {
    // text 가 빈칸·줄바꿈뿐이면 없는 것, 앞뒤 줄바꿈은 다듬는다
    let (cs, ps) = parse_criteria(Some(&json!([{"id": "A", "text": " \n\t"}, {"id": "B", "text": "\n본문\n"}, 7])), 5);
    assert_eq!(cs, vec![Criterion::new("B", "본문")]);
    assert_eq!(ps, vec!["단위 5 의 수용 기준 A 에 text 가 없습니다".to_string(), "단위 5 의 수용 기준 3 에 id(영문·숫자·.-_)가 없습니다".to_string()]);
    // 짝이 모두 있으면 빈 목록, 시나리오가 없으면 모두 빈 칸
    let cs = vec![Criterion::new("A", "a"), Criterion::new("B", "b")];
    assert!(criteria_gaps(&cs, &[sh_crit("x.sh", "A"), sh_crit("y.sh", "B")]).is_empty());
    assert_eq!(criteria_gaps(&cs, &[sh("x.sh")]), cs);
    // 카드 글 전체와 답의 우선순위(시나리오가 이긴다)
    let card = criterion_card("단위", &cs[0]);
    assert_eq!(card.label, "실행 파일이 없는 수용 기준");
    assert_eq!(card.title, "「단위」 의 수용 기준 A 를 볼 판정 시나리오가 없습니다");
    assert_eq!(card.why, "a (이 기준은 실행 파일로 판정하지 않아 통과·실패를 기계로 가리지 못합니다.)");
    assert_eq!(card.options[0].label, "범위에서 빼기");
    assert_eq!(card.options[1].label, "모델 판정으로 두기");
    assert_eq!(CRITERION_DROP, "drop_criterion");
    assert_eq!(CRITERION_MODEL_JUDGE, "model_judge");
    assert_eq!(criterion_answer(&cs[0], &[sh_crit("x.sh", "A")], Some("model_judge")), ("scenario".to_string(), false));
    // 시나리오가 없는 기준이 카드에서 범위에서 빼기를 고르면 그 답 그대로(defaulted 아님)
    assert_eq!(criterion_answer(&cs[1], &[sh("x.sh")], Some("drop_criterion")), ("drop_criterion".to_string(), false));
    assert_eq!(criterion_answer(&cs[1], &[], Some("model_judge")), ("model_judge".to_string(), false));
}

#[test]
fn 추가_json() {
    assert_eq!(scenario_json(&sh("a.sh")), json!({"file": "a.sh", "kind": "shell"}));
    assert_eq!(
        scenario_json(&Scenario::new("b.mjs", ScenarioKind::Browser, Some("AC-1"), Some(BrowserDriver::Playwright))),
        json!({"file": "b.mjs", "kind": "browser", "criterion": "AC-1", "driver": "playwright"})
    );
    assert_eq!(scenario_json(&Scenario::new("b.mjs", ScenarioKind::Browser, None, None))["driver"], "chrome");
    assert_eq!(criterion_json(&Criterion::new("A", "글")), json!({"id": "A", "text": "글"}));
}

#[test]
fn 추가_도구_글() {
    assert_eq!(tool_label(&ToolNeed::Program("/bin/sh".into())), "/bin/sh");
    assert_eq!(tool_label(&ToolNeed::Command("node".into())), "node(PATH)");
    assert_eq!(tool_label(&ToolNeed::Browser), "브라우저(Google Chrome·Chromium·Microsoft Edge)");
    assert_eq!(tool_label(&ToolNeed::NodeModule("playwright".into())), "node 모듈 playwright");
}

fn run_of(file: &str, head: &str) -> (Vec<String>, Vec<ToolNeed>, Option<String>) {
    let r = scenario_run(&sh(file), head);
    (r.interpreter, r.needs, r.problem)
}

#[test]
fn 추가_돌리는_법_경계() {
    // 확장자는 대소문자를 가리지 않고, 점 파일·확장자 없음·끝 점은 확장자 없음
    assert_eq!(run_of("a.SH", "").0, vec!["/bin/sh"]);
    assert_eq!(run_of("d/a.Py", "").0, vec!["/usr/bin/env", "python3"]);
    for f in ["a.bash", "a.zsh", "a.js", "a.cjs", "a.ts", "a.mts", "a.mjs"] {
        assert!(run_of(f, "").2.is_none(), "{f}");
    }
    for f in [".sh", "sh", "a.", "a.sh.bak", "d.sh/x"] {
        assert!(run_of(f, "").2.is_some(), "{f}");
    }
    assert_eq!(run_of("d/a.sh/", "").0, vec!["/bin/sh"]); // NSString.pathExtension 은 끝 `/` 를 뗀다
    // #! 이 확장자보다 먼저, 빈칸·탭으로 가름, 빈 #! 은 절대 경로가 아님
    assert_eq!(run_of("a.py", "#!/bin/sh\n").0, vec!["/bin/sh"]);
    assert_eq!(run_of("a", "#!/usr/bin/env\tnode  --x\n").0, vec!["/usr/bin/env", "node", "--x"]);
    assert_eq!(run_of("a", "#!\n").2.as_deref(), Some("첫 줄 #! 의 해석기가 절대 경로가 아닙니다(#!)"));
    assert_eq!(run_of("a", "#! /bin/sh").0, vec!["/bin/sh"]);
    // env 만 있고 이름이 없으면 도구는 env 하나, -로 시작하는 것만 있어도 같다
    assert_eq!(run_of("a", "#!/usr/bin/env\n").1, vec![ToolNeed::Program("/usr/bin/env".into())]);
    assert_eq!(run_of("a", "#!/usr/bin/env -i\n").1, vec![ToolNeed::Program("/usr/bin/env".into())]);
    // env 가 아닌 해석기의 다음 조각은 이름으로 보지 않는다. 끝 `/` 가 붙은 env 도 env
    assert_eq!(run_of("a", "#!/bin/bash node\n").1, vec![ToolNeed::Program("/bin/bash".into())]);
    assert_eq!(run_of("a", "#!/usr/bin/env/ node\n").1, vec![ToolNeed::Program("/usr/bin/env/".into()), ToolNeed::Command("node".into())]);
    // 오류 글은 첫 줄을 자소 묶음 60개까지
    let long = format!("#!{}\nx", "b".repeat(100));
    let want = format!("첫 줄 #! 의 해석기가 절대 경로가 아닙니다(#!{})", "b".repeat(58));
    assert_eq!(run_of("a", &long).2, Some(want));
    // 브라우저 playwright 는 node 가 이미 있으면 다시 더하지 않는다. 드라이버 없는 브라우저는 chrome
    let pw = Scenario { file: "p.mjs".into(), kind: ScenarioKind::Browser, criterion: None, driver: Some(BrowserDriver::Playwright) };
    assert_eq!(scenario_run(&pw, "#!/usr/bin/env node\n").needs.iter().filter(|n| **n == ToolNeed::Command("node".into())).count(), 1);
    let bare = Scenario { file: "p.sh".into(), kind: ScenarioKind::Browser, criterion: None, driver: None };
    assert_eq!(scenario_run(&bare, "").needs.last(), Some(&ToolNeed::Browser));
    // 돌리는 법을 모르면 도구도 없다(브라우저도 드라이버 도구는 더한다)
    assert_eq!(scenario_run(&bare, "#!sh\n").needs, vec![ToolNeed::Browser]);
}

#[test]
fn 추가_돌리는_법_글() {
    assert_eq!(
        run_of("a.rb", "").2.as_deref(),
        Some("돌리는 법을 모릅니다. 첫 줄에 #! 해석기를 쓰거나 확장자를 .sh·.bash·.zsh·.py·.js·.mjs·.cjs·.ts 로 씁니다")
    );
    assert_eq!(run_of("a.sh", "#!bash\n").2.as_deref(), Some("첫 줄 #! 의 해석기가 절대 경로가 아닙니다(#!bash)"));
}

#[test]
fn 추가_crlf_머리() {
    // Foundation 과 같음: Swift 의 "\n" 은 글자 하나라 "\r\n" 으로 끝나는 첫 줄은 나눠지지 않아 `\r\n` 이 해석기 글자에 섞인다(도구 확인에서 막힘)
    let (argv, needs, problem) = run_of("a", "#!/bin/sh\r\necho hi");
    assert_eq!(argv, vec!["/bin/sh\r\necho", "hi"]);
    assert_eq!(needs[0], ToolNeed::Program("/bin/sh\r\necho".into()));
    assert!(problem.is_none());
    // 홀로 선 `\r` 은 구분자, 홀로 선 `\n` 은 줄 끝
    assert_eq!(run_of("a", "#!/bin/sh\r-e\nfoo").0, vec!["/bin/sh", "-e"]);
    assert!(run_of("a", "\n#!/bin/sh").2.is_some()); // 첫 줄이 빈 줄이면 #! 이 아님 → 확장자(없음)
}

#[test]
fn 추가_환경_글과_json() {
    assert_eq!(kind_label(ScenarioKind::Shell), "셸");
    assert_eq!(kind_label(ScenarioKind::Browser), "브라우저");
    // 통과한 줄: why 없음, found 는 찾은 자리, 도구 줄은 원본 순서
    let s = sh("t/a.py");
    let ok = env_check(1, &s, &scenario_run(&s, ""), "/w", &FakeProbe::new());
    assert_eq!(
        env_row_json(&ok),
        json!({"unit": 1, "file": "t/a.py", "kind": "shell", "ok": true, "tools": [
            {"need": "/usr/bin/env", "found": "/usr/bin/env"}, {"need": "python3(PATH)", "found": "/opt/homebrew/bin/python3"}]})
    );
    // 없는 도구는 found 가 null, 여럿이면 쉼표로 이어 모두 적는다. 프로그램이 없으면 그 경로 자체가 라벨
    let mut probe = FakeProbe::new();
    probe.programs.clear();
    probe.commands.clear();
    let pw = Scenario::new("t/p.mjs", ScenarioKind::Browser, None, Some(BrowserDriver::Playwright));
    let miss = env_check(7, &pw, &scenario_run(&pw, ""), "/w", &probe);
    assert_eq!(miss.tools.iter().map(|t| t.ok()).collect::<Vec<_>>(), vec![false, false, false]);
    assert_eq!(
        env_problem(&miss).as_deref(),
        Some("단위 7 의 시나리오 t/p.mjs(브라우저)를 돌릴 도구가 없습니다: /usr/bin/env, node(PATH), node 모듈 playwright 을(를) 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 도구를 깐 뒤 다시 확인하거나 시나리오를 바꿉니다")
    );
    let j = env_row_json(&miss);
    assert_eq!(j["tools"][0], json!({"need": "/usr/bin/env", "found": null}));
    assert_eq!(j["driver"], json!("playwright"));
    // 모듈은 찾으면 "있음", 도구는 모두 찾아도 돌리는 법을 모르면 실패(problem 이 이긴다)
    let mut with_mod = FakeProbe::new();
    with_mod.modules.insert("playwright");
    let found = env_check(1, &pw, &scenario_run(&pw, ""), "/w", &with_mod);
    assert_eq!(found.tools[2].found.as_deref(), Some("있음"));
    assert!(found.ok());
    assert!(env_row_json(&found).get("why").is_none());
    let rb = sh("a.rb");
    let unk = env_check(4, &rb, &scenario_run(&rb, ""), "/w", &FakeProbe::new());
    assert!(!unk.ok() && unk.tools.is_empty());
    assert_eq!(env_row_json(&unk)["why"], json!(env_problem(&unk).unwrap()));
    assert!(env_row_json(&unk).get("driver").is_none());
    // 셸 시나리오는 driver 칸이 없다
    assert_eq!(env_check(1, &s, &scenario_run(&s, ""), "/w", &FakeProbe::new()).driver, None);
}

#[test]
fn 추가_걸음_필드와_라벨() {
    assert_eq!(scenario_label(&sh("a.sh")), "시나리오 a.sh(셸)");
    assert_eq!(scenario_label(&Scenario::new("b.mjs", ScenarioKind::Browser, Some("AC-2"), None)), "시나리오 b.mjs(브라우저 · 수용 기준 AC-2)");
    let steps = judge_steps(
        Some("make test"),
        "/bin/zsh",
        &[pinned_full("s/p.mjs", ScenarioKind::Browser, Some("AC-1"), Some("abc"), "#!/usr/bin/env node\n"), pinned("t/q.sh")],
    );
    let s0 = &steps[0];
    assert_eq!(s0.argv, strs(&["/usr/bin/env", "node", "/R/pinned/checks/scenarios/1/p.mjs"]));
    assert_eq!((s0.evidence.as_str(), s0.kind.as_str()), ("scenario-1", "browser"));
    assert_eq!(s0.file.as_deref(), Some("s/p.mjs"));
    assert_eq!(s0.criterion.as_deref(), Some("AC-1"));
    assert_eq!(s0.pinned.as_deref(), Some("/R/pinned/checks/scenarios/1/p.mjs"));
    assert_eq!(s0.sha256.as_deref(), Some("abc"));
    assert_eq!((steps[1].name.as_str(), steps[1].kind.as_str(), steps[1].evidence.as_str()), ("scenario-2", "shell", "scenario-2"));
    assert_eq!((steps[1].criterion.clone(), steps[1].sha256.clone()), (None, None));
    // 판정 명령 걸음: 증거는 맨 위, 시나리오 칸 없음
    let c = &steps[2];
    assert_eq!((c.name.as_str(), c.label.as_str(), c.evidence.as_str(), c.kind.as_str()), ("check", "판정 명령", "", "check"));
    assert_eq!((c.file.clone(), c.criterion.clone(), c.pinned.clone(), c.sha256.clone()), (None, None, None, None));
    // 판정 명령이 없거나 비면 걸음이 없다. 시나리오도 없으면 빈 차례
    assert_eq!(judge_steps(Some(""), "/bin/sh", &[pinned("a.sh")]).len(), 1);
    assert!(judge_steps(None, "/bin/sh", &[]).is_empty());
    assert!(judge_steps(Some(""), "/bin/sh", &[]).is_empty());
}

#[test]
fn 추가_결과_판정() {
    let mk = |kinds: &[&str]| -> Vec<JudgeStep> { kinds.iter().enumerate().map(|(i, k)| JudgeStep::new(&format!("s{i}"), "L", vec![], "", k)).collect() };
    let res = |code: i32| StepResult::new(code);
    let outcome = |steps: Vec<JudgeStep>, results: Vec<StepResult>| claude_board_autopilot::rules::scenarios::JudgeOutcome { steps, results };
    // 결과가 없거나 걸음 수와 다르면 통과가 아니다
    assert!(!outcome(mk(&["check"]), vec![]).passed());
    // 시간 상한만 닿고 종료 코드가 0 이어도 통과가 아니다. 새 결과의 기본값은 0초·안 끝남·빈 출력·파일 없음·까닭 없음
    assert!(!outcome(mk(&["shell"]), vec![StepResult { timed_out: true, ..res(0) }]).passed());
    let d = res(1);
    assert!(d.sec == 0.0 && !d.timed_out && !d.interrupted && d.tail.is_empty() && d.files.is_empty() && d.note.is_none());
    assert!(!outcome(mk(&["check", "shell"]), vec![res(0)]).passed());
    assert!(outcome(mk(&["check", "shell"]), vec![res(0), res(0)]).passed());
    assert_eq!(outcome(mk(&["check"]), vec![]).code(), -1);
    // 시간 상한은 종료 코드가 0 이어도 실패, code 는 처음 실패한 걸음의 것
    let t = StepResult { timed_out: true, ..res(0) };
    let o = outcome(mk(&["shell", "shell", "shell"]), vec![res(0), t, res(5)]);
    assert!(!o.passed() && o.timed_out() && !o.interrupted());
    assert_eq!(o.code(), 0);
    let o = outcome(mk(&["shell", "shell"]), vec![res(4), res(5)]);
    assert_eq!(o.code(), 4);
    // 끊긴 것: 종료 코드가 0 이어도 통과가 아니고, 실패한 걸음이 없으면 -1
    let i = StepResult { interrupted: true, ..res(0) };
    let o = outcome(mk(&["shell"]), vec![i]);
    assert!(o.interrupted() && !o.passed() && o.code() == -1);
    // describe: 시나리오 없으면 판정 명령 그대로(없으면 빈 글), 시나리오만이면 개수만
    let only_check = outcome(mk(&["check"]), vec![res(0)]);
    assert_eq!(only_check.describe(Some("make")), "make");
    assert_eq!(only_check.describe(None), "");
    let two = outcome(mk(&["shell", "browser"]), vec![res(0), res(0)]);
    assert_eq!(two.describe(None), "시나리오 2개");
    assert_eq!(two.describe(Some("")), "시나리오 2개");
    assert_eq!(two.describe(Some("x")), "시나리오 2개 + 판정 명령 `x`");
    let with_check = outcome(mk(&["shell", "check"]), vec![res(0), res(0)]);
    assert_eq!(with_check.describe(Some("x")), "시나리오 1개 + 판정 명령 `x`");
}

#[test]
fn 추가_꼬리() {
    let mk = |tails: &[(&str, StepResult)]| {
        let steps: Vec<JudgeStep> = tails.iter().map(|(l, _)| JudgeStep::new("n", l, vec![], "", "shell")).collect();
        claude_board_autopilot::rules::scenarios::JudgeOutcome { steps, results: tails.iter().map(|(_, r)| r.clone()).collect() }
    };
    let lines = |a: usize, b: usize| (a..=b).map(|i| format!("줄{i}")).collect::<Vec<_>>().join("\n");
    // 끝 20줄만 남긴다(25줄 중 6~25)
    let fail = StepResult { tail: lines(1, 25), ..StepResult::new(2) };
    let ok = StepResult { tail: "무시".to_string(), ..StepResult::new(0) };
    let o = mk(&[("A", fail.clone()), ("B", ok.clone())]);
    assert_eq!(o.tail(), format!("[A] 종료 코드 2\n{}", lines(6, 25)));
    // 20줄 이하면 그대로, 실패한 걸음마다 빈 줄 하나로 잇는다. 성공한 걸음은 빠진다
    let f2 = StepResult { tail: lines(1, 3), ..StepResult::new(3) };
    let o = mk(&[("A", fail), ("B", ok.clone()), ("C", f2)]);
    assert_eq!(o.tail(), format!("[A] 종료 코드 2\n{}\n\n[C] 종료 코드 3\n{}", lines(6, 25), lines(1, 3)));
    // 출력이 비면 머리 줄만, 시간 상한·까닭 글이 종료 코드보다 앞선다
    let to = StepResult { timed_out: true, ..StepResult::new(0) };
    let nt = StepResult { note: Some("까닭".to_string()), ..StepResult::new(-1) };
    let o = mk(&[("T", to.clone()), ("N", nt)]);
    assert_eq!(o.tail(), "[T] 시간 상한에 닿음\n\n[N] 까닭");
    // 걸음이 하나면 실패가 아니어도 그 출력 그대로(스무 줄 넘어도 자르지 않는다)
    let one = mk(&[("A", StepResult { tail: lines(1, 30), ..StepResult::new(1) })]);
    assert_eq!(one.tail(), lines(1, 30));
    // 모두 통과면 빈 글
    assert_eq!(mk(&[("A", ok.clone()), ("B", ok)]).tail(), "");
}

#[test]
fn 추가_꼬리_crlf() {
    // Foundation 과 같음: Swift 의 "\n" 은 글자 하나라 "\r\n" 은 나눠지지 않는다 → CRLF 출력은 한 줄로 센다(원본 동작 고정)
    let crlf = (1..=30).map(|i| format!("줄{i}")).collect::<Vec<_>>().join("\r\n");
    let steps: Vec<JudgeStep> = vec![JudgeStep::new("a", "A", vec![], "", "shell"), JudgeStep::new("b", "B", vec![], "", "shell")];
    let o = claude_board_autopilot::rules::scenarios::JudgeOutcome {
        steps,
        results: vec![StepResult { tail: crlf.clone(), ..StepResult::new(1) }, StepResult::new(0)],
    };
    assert_eq!(o.tail(), format!("[A] 종료 코드 1\n{crlf}"));
    // 홀로 선 LF 는 줄 끝이다(CRLF 와 섞여도 LF 만 센다): 25 개의 LF 줄 앞에 CRLF 한 덩어리
    let mixed = format!("a\r\nb\n{}", (1..=25).map(|i| format!("m{i}")).collect::<Vec<_>>().join("\n"));
    let o2 = claude_board_autopilot::rules::scenarios::JudgeOutcome {
        steps: vec![JudgeStep::new("a", "A", vec![], "", "shell"), JudgeStep::new("b", "B", vec![], "", "shell")],
        results: vec![StepResult { tail: mixed, ..StepResult::new(1) }, StepResult::new(0)],
    };
    let want = (6..=25).map(|i| format!("m{i}")).collect::<Vec<_>>().join("\n");
    assert_eq!(o2.tail(), format!("[A] 종료 코드 1\n{want}"));
    // 맨 앞 LF 는 줄 끝이다
    let o3 = claude_board_autopilot::rules::scenarios::JudgeOutcome {
        steps: vec![JudgeStep::new("a", "A", vec![], "", "shell"), JudgeStep::new("b", "B", vec![], "", "shell")],
        results: vec![StepResult { tail: "\nx".to_string(), ..StepResult::new(1) }, StepResult::new(0)],
    };
    assert_eq!(o3.tail(), "[A] 종료 코드 1\n\nx");
}

#[test]
fn 추가_지문_경계() {
    let mut step = JudgeStep::new("scenario-1", "L", vec![], "scenario-1", "shell");
    step.file = Some("a.sh".to_string());
    // 지문이 있는데 지금 지문을 못 읽으면(없음) 달라진 것으로 본다
    step.sha256 = Some("aaa".to_string());
    step.pinned = Some("/P/a.sh".to_string());
    let r = FakeRunner::new(&[]);
    let o = run_judge(std::slice::from_ref(&step), &r);
    assert!(r.ran().is_empty() && o.code() == -1 && !o.passed());
    assert_eq!(o.results[0].note.as_deref(), Some("보안: 고정본이 출발 때와 달라 돌리지 않음"));
    // 파일 칸이 없으면 고정본 경로를 글에 쓴다
    let mut nofile = step.clone();
    nofile.file = None;
    let o = run_judge(&[nofile], &FakeRunner::new(&[]));
    assert_eq!(o.results[0].tail, "보안: 판정 시나리오 고정본이 출발 때와 달라 돌리지 않았습니다(/P/a.sh)");
    // 같으면 돌린다
    let mut same = FakeRunner::new(&[]);
    same.digests.insert("/P/a.sh".to_string(), "aaa".to_string());
    assert!(run_judge(std::slice::from_ref(&step), &same).passed());
    // 지문만 있고 고정본 경로가 없거나, 고정본 경로만 있고 지문이 없으면 견주지 않고 돌린다
    let mut only_sha = step.clone();
    only_sha.pinned = None;
    let mut only_pin = step.clone();
    only_pin.sha256 = None;
    let r = FakeRunner::new(&[]);
    run_judge(&[only_sha, only_pin], &r);
    assert_eq!(r.ran(), strs(&["scenario-1", "scenario-1"]));
    // 판정 걸음이 없으면 빈 결과(통과 아님)
    assert!(!run_judge(&[], &FakeRunner::new(&[])).passed());
}

#[test]
fn 추가_요약_경계() {
    let mut shell_step = JudgeStep::new("scenario-1", "S", vec![], "scenario-1", "shell");
    shell_step.file = Some("a.sh".to_string());
    shell_step.criterion = Some("AC-1".to_string());
    shell_step.pinned = Some("/P/a.sh".to_string());
    shell_step.sha256 = Some("abc".to_string());
    let check_step = JudgeStep::new("check", "판정 명령", vec![], "", "check");
    let browser = JudgeStep::new("scenario-2", "B", vec![], "scenario-2", "browser");
    let timed = StepResult { timed_out: true, sec: 2.0006, files: strs(&["x.png"]), ..StepResult::new(0) };
    let noted = StepResult { note: Some("까닭".to_string()), files: strs(&["A.PNG", "b.JPEG", "c.webp", "d.gif", "e.html", "f.jpg", "png"]), ..StepResult::new(0) };
    let o = claude_board_autopilot::rules::scenarios::JudgeOutcome {
        steps: vec![shell_step, check_step, browser],
        results: vec![timed, StepResult { sec: 2.0004, ..StepResult::new(7) }, noted],
    };
    let s = judge_summary(&o, "샌드박스 밖", 3.0);
    assert_eq!(s["at"].as_f64(), Some(3.0));
    assert_eq!(s["passed"], json!(false));
    let p = s["parts"].as_array().unwrap();
    // 셸 걸음: 시간 상한이면 passed 거짓, 시나리오 칸이 모두 붙고 스크린샷 칸은 없다
    assert_eq!(
        p[0],
        json!({"name": "scenario-1", "label": "S", "kind": "shell", "exit": 0, "sec": 2.001, "timed_out": true, "interrupted": false, "passed": false,
               "evidence": "scenario-1", "files": ["x.png"], "file": "a.sh", "criterion": "AC-1", "pinned": "/P/a.sh", "sha256": "abc"})
    );
    // 판정 명령 걸음: 시나리오 칸이 없다. 밀리초 밑은 반올림
    assert_eq!(
        p[1],
        json!({"name": "check", "label": "판정 명령", "kind": "check", "exit": 7, "sec": 2.0, "timed_out": false, "interrupted": false, "passed": false,
               "evidence": "", "files": []})
    );
    // 브라우저 걸음: 그림 확장자(대소문자 무관)만 screenshots, 까닭이 이미 있으면 그대로 두고 덧붙이지 않는다
    assert_eq!(p[2]["screenshots"], json!(["A.PNG", "b.JPEG", "c.webp", "f.jpg"]));
    assert_eq!(p[2]["note"], json!("까닭"));
}

#[test]
fn 추가_카드_글() {
    let c = prerun_scenario_card(3, "u3", "T", "a.sh", None);
    assert_eq!(c["title"], json!("「T」 의 시나리오 a.sh 가 일하기 전부터 통과합니다"));
    assert_eq!(c["kind"], json!("prerun"));
    assert_eq!(c["why"], json!("그 시나리오가 일한 결과의 뜻을 보지 않거나 이미 된 기준이라는 뜻입니다. 그대로 두면 그 기준은 일하지 않아도 통과로 남습니다."));
    assert_eq!(c["options"], json!([{"key": "fix_check", "label": "판정 고치기"}, {"key": "keep", "label": "그대로"}, {"key": "drop", "label": "빼기"}]));
    assert_eq!(c["unit"].as_i64(), Some(3));
    assert!(!c.to_string().contains('—'));
}

#[test]
fn 추가_브라우저_찾음() {
    // 브라우저를 찾으면 그 자리가 found, 줄은 통과
    let ch = Scenario::new("t/p.sh", ScenarioKind::Browser, None, None);
    let row = env_check(1, &ch, &scenario_run(&ch, "#!/bin/sh"), "/w", &FakeProbe::new());
    assert!(row.ok());
    assert_eq!(row.tools.last().and_then(|t| t.found.clone()).as_deref(), Some("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
}

#[test]
fn 추가_요약_브라우저_까닭_유지() {
    // 돌리지 않은 까닭이 이미 있으면 브라우저 걸음에 스크린샷이 없어도 그 까닭을 그대로 둔다
    let browser = JudgeStep::new("scenario-1", "B", vec![], "scenario-1", "browser");
    let r = StepResult { note: Some("보안: 안 돌림".to_string()), ..StepResult::new(-1) };
    let o = claude_board_autopilot::rules::scenarios::JudgeOutcome { steps: vec![browser], results: vec![r] };
    let s = judge_summary(&o, "x", 0.0);
    assert_eq!(s["parts"][0]["note"], json!("보안: 안 돌림"));
    assert_eq!(s["parts"][0]["screenshots"], json!([]));
}
