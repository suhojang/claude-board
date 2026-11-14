//! 판정 비판(`rules::critique`, 원본 Critique.swift 전체) 시험. 원본: `Tests/AutopilotRulesTests/CritiqueTests.swift`.
//! 이 파일이 옮긴 Swift 시험 22개: `JudgePlaceTests`(`경로`·`스크립트`·`보호`·`대상`·`글자`) · `ProbeTests`(`계획`·`돌리기`·`이미통과`·`까닭`·`카드`→`탐침_카드`) ·
//! `CaseAllowTests`(`허용`·`겹침`·`설정반례`) · `CriticTests`(`모델`·`모델고르기`·`글`→`비판_글`·`읽기`·`확인`·`카드`) · `FixLoopTests`(`차례`·`글`→`고치기_글`·`약해짐`).
//! 표 시험의 줄은 원본과 같은 줄이다. Swift 시험 33개 가운데 Misses(7)·Records 마이그레이션(2)·Records 판 4(1)·DraftMisses(1) 쪽 11개는 다른 규칙 파일 몫이다.
//! 이름 `탐침_카드` 는 비판 세션의 `카드` 와, `비판_글`·`고치기_글` 은 서로 겹치지 않게 붙였다.
//! 가짜 어댑터(버리는 작업 트리·비판 결과 폴더)는 이 파일에만 둔다. 값은 장난감(zz-toy·example.com).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use claude_board_autopilot::rules::critique::*;
use claude_board_autopilot::rules::draft::{DRAFT_HEAD_LINE, DraftFiles, DraftInput, DraftOption, DraftScenario, DraftedUnit};
use claude_board_autopilot::rules::scenarios::{Criterion, JudgeOutcome, JudgeStep, ScenarioKind, StepResult};

// MARK: 가짜 어댑터

struct FakeTree {
    base: HashMap<String, String>, // 출발 기준의 파일
    files: RefCell<HashMap<String, String>>,
    resets: Cell<usize>,
    judged: RefCell<Vec<String>>,
    put_fail: Option<String>, // 이 경로에 쓰면 실패
    steps: Vec<JudgeStep>,
    verdict: fn(&HashMap<String, String>) -> HashMap<String, bool>, // 걸음 이름 → 통과
    sec: f64,
}

impl FakeTree {
    fn new(steps: Vec<JudgeStep>, verdict: fn(&HashMap<String, String>) -> HashMap<String, bool>) -> Self {
        FakeTree {
            base: HashMap::new(),
            files: RefCell::new(HashMap::new()),
            resets: Cell::new(0),
            judged: RefCell::new(vec![]),
            put_fail: None,
            steps,
            verdict,
            sec: 1.0,
        }
    }
}

impl ProbeWorkspace for FakeTree {
    fn reset(&self, _unit: i64) -> Option<String> {
        self.resets.set(self.resets.get() + 1);
        *self.files.borrow_mut() = self.base.clone();
        None
    }
    fn put(&self, _unit: i64, relative: &str, data: &[u8]) -> Option<String> {
        if self.put_fail.as_deref() == Some(relative) {
            return Some("링크라 쓰지 않았습니다".to_string());
        }
        self.files.borrow_mut().insert(relative.to_string(), String::from_utf8_lossy(data).into_owned());
        None
    }
    fn judge(&self, _unit: i64, label: &str) -> JudgeOutcome {
        self.judged.borrow_mut().push(label.to_string());
        let v = (self.verdict)(&self.files.borrow());
        JudgeOutcome {
            steps: self.steps.clone(),
            results: self
                .steps
                .iter()
                .map(|s| {
                    let mut r = StepResult::new(if v.get(&s.name) == Some(&true) { 0 } else { 1 });
                    r.sec = self.sec;
                    r
                })
                .collect(),
        }
    }
}

fn step(name: &str, file: Option<&str>, crit: Option<&str>) -> JudgeStep {
    let label = file.map_or("판정 명령".to_string(), |f| format!("시나리오 {f}"));
    let mut s = JudgeStep::new(name, &label, vec![], name, "shell");
    s.file = file.map(String::from);
    s.criterion = crit.map(String::from);
    s
}

// 견본 초안(docs/자율주행/시험 도구/견본 초안)의 형식만 보는 시나리오 둘과 같은 꼴
const LIST_SH: &str = "#!/bin/sh\n# AC-2: GET /todos 가 있다\ntest -f app/todos.py || { echo \"app/todos.py 가 없습니다\"; exit 1; }\ngrep -q \"GET /todos\" app/todos.py";
const ROUTES_PY: &str = "#!/usr/bin/env python3\n# AC-1: POST /todos 가 있다(장난감: app/todos.py 의 ROUTES 를 본다)\nimport pathlib, sys\np = pathlib.Path(\"app/todos.py\")\nif not p.exists():\n    print(\"app/todos.py 가 없습니다\"); sys.exit(1)\nsys.exit(0 if \"POST /todos\" in p.read_text() else 1)";

fn toy_view() -> JudgeView {
    JudgeView::new(
        1,
        "u1",
        "할 일 만들기·목록 API",
        "",
        vec![JudgeText::new(".autopilot/scenarios/u1/routes.py", ROUTES_PY), JudgeText::new(".autopilot/scenarios/u1/list.sh", LIST_SH)],
    )
}

fn toy_steps() -> Vec<JudgeStep> {
    vec![step("scenario-1", Some(".autopilot/scenarios/u1/routes.py"), Some("AC-1")), step("scenario-2", Some(".autopilot/scenarios/u1/list.sh"), Some("AC-2"))]
}

// 형식만 보는 판정: 글자만 있으면 통과
fn weak_verdict(f: &HashMap<String, String>) -> HashMap<String, bool> {
    let t = f.get("app/todos.py");
    HashMap::from([
        ("scenario-1".to_string(), t.is_some_and(|t| t.contains("POST /todos"))),
        ("scenario-2".to_string(), t.is_some_and(|t| t.contains("GET /todos"))),
    ])
}

// 뜻을 보는 판정: 진짜로 ROUTES 목록이 그 경로를 담아야 통과(장난감: 정확한 구현 글만)
const REAL_IMPL: &str = "ROUTES = ['POST /todos', 'GET /todos']\n";
fn strong_verdict(f: &HashMap<String, String>) -> HashMap<String, bool> {
    let ok = f.get("app/todos.py").map(String::as_str) == Some(REAL_IMPL);
    HashMap::from([("scenario-1".to_string(), ok), ("scenario-2".to_string(), ok)])
}

fn failed_baseline(steps: &[JudgeStep], passing: &[&str]) -> JudgeOutcome {
    JudgeOutcome { steps: steps.to_vec(), results: steps.iter().map(|s| StepResult::new(if passing.contains(&s.name.as_str()) { 0 } else { 1 })).collect() }
}

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

// MARK: 판정이 보는 자리

#[test]
fn 경로() {
    let rows: [(&str, &[&str]); 7] = [
        ("test -f app/todos.py || exit 1", &["app/todos.py"]),
        ("grep -q '할 일' NOTES.md", &["NOTES.md"]),
        ("curl -s http://127.0.0.1:18431/index.html | grep -q ok", &[]),
        ("p = pathlib.Path(\"out/data.json\")", &["out/data.json"]),
        ("cat ../other/a.txt /etc/hosts.txt ~/x.md", &[]),
        ("test -s done && test -e build/out", &["done", "build/out"]),
        ("python3 tests/check.py && test -f result.csv", &["tests/check.py", "result.csv"]),
    ];
    for (text, paths) in rows {
        assert_eq!(judge_paths(text), strs(paths), "{text}");
    }
}

#[test]
fn 스크립트() {
    assert_eq!(judge_run_scripts("python3 tests/check.py && test -f out.json"), strs(&["tests/check.py"]));
    assert_eq!(judge_run_scripts("sh ./run.sh"), strs(&["run.sh"]));
    assert_eq!(judge_run_scripts("./check.sh; node -e 1"), strs(&["check.sh"]));
    assert!(judge_run_scripts("grep -q x a.md").is_empty());
}

#[test]
fn 보호() {
    let rows: &[(&str, bool)] = &[
        (".git/config", true),
        ("sub/.git/HEAD", true),
        (".autopilot/scenarios/u1/list.sh", true),
        ("CLAUDE.md", true),
        ("docs/CLAUDE.md", true),
        (".claude/settings.json", true),
        (".mcp.json", true),
        ("AGENTS.md", true),
        ("tests/test_app.py", true),
        ("app/todos.test.ts", true),
        ("app/todos.py", false),
        ("NOTES.md", false),
        ("check.sh", true),
        ("README.md", false),
        // 대소문자·유니코드 정규화(⑪-3 자동 보안 검토 MEDIUM): APFS 는 가리지 않으므로 접어서 견준다
        (".GIT/config", true),
        (".Git", true),
        ("sub/.Git/HEAD", true),
        (".CLAUDE/settings.json", true),
        (".Claude/settings.json", true),
        ("Claude.md", true),
        ("claude.md", true),
        ("docs/Claude.Local.md", true),
        ("Agents.md", true),
        (".MCP.json", true),
        (".Autopilot/scenarios/u1/list.sh", true),
        ("Check.sh", true),
        ("TESTS/x.py", true),
        ("App/\u{D310}\u{C815}.sh", true),
        ("app/\u{1111}\u{1161}\u{11AB}\u{110C}\u{1165}\u{11BC}.sh", true),
        // 접어서 견주는 쪽의 경계(원본 표에 없는 줄: 같은 함수의 반대 방향·이름이 비슷한 것)
        ("app/todos_test.py", true),
        ("app/todos.spec.js", true),
        ("spec/x.rb", true),
        ("__tests__/a.js", true),
        ("a/test_x.py", true),
        ("latest.py", false),
        ("claude.md.txt", false),
        ("app/.claudex/a.py", false),
        ("app/gitignore.txt", false),
        ("app/.github/a.yml", false),
        ("docs/AGENTS.md", true),
    ];
    let judge = strs(&["check.sh", "app/\u{D310}\u{C815}.sh"]);
    for &(p, protected) in rows {
        assert_eq!(protected_judge_path(p, &judge), protected, "{p}");
        assert_eq!(case_path_ok(p, &judge), !protected, "{p}");
    }
}

#[test]
fn 대상() {
    assert_eq!(probe_targets(&toy_view()), strs(&["app/todos.py"]));
    let v = JudgeView::new(
        2,
        "u2",
        "안내",
        "python3 tests/check.py && grep -q '할 일' NOTES.md && test -f .autopilot/x.json",
        vec![JudgeText::new("s/a.sh", "test -f s/a.sh && test -f CLAUDE.md && test -f out.json")],
    );
    assert_eq!(probe_targets(&v), strs(&["out.json", "NOTES.md"]));
}

#[test]
fn 글자() {
    assert_eq!(judge_literals(&[LIST_SH]), strs(&["app/todos.py 가 없습니다", "GET /todos"]));
    assert_eq!(judge_literals(&[ROUTES_PY, LIST_SH]), strs(&["app/todos.py", "app/todos.py 가 없습니다", "POST /todos", "GET /todos"]));
    assert_eq!(judge_literals(&["echo '$X' \"--\" '  '"]), strs(&["$X"]));
}

// MARK: ① 반대 탐침

#[test]
fn 계획() {
    let p = probe_plan(&toy_view());
    assert_eq!(p.targets, strs(&["app/todos.py"]));
    assert_eq!(p.probes.iter().map(|x| x.kind).collect::<Vec<_>>(), [ProbeKind::Empty, ProbeKind::Junk, ProbeKind::Truncated]);
    let junk = &p.probes[1].files[0].text;
    assert!(junk.contains("ZZ_PROBE = [\"app/todos.py\", \"app/todos.py 가 없습니다\", \"POST /todos\", \"GET /todos\"]"));
    assert!(junk.ends_with("raise SystemExit(3)\n"));
    assert!(p.probes[2].files[0].text.contains("def zz_probe(:"));
    assert_eq!(probe_plan(&toy_view()), p);
    // JSON 대상·서버 포트가 있으면 목록·가짜 서버
    let mut v = JudgeView::new(
        1,
        "u1",
        "t",
        "python3 -c 'import json; d=json.load(open(\"out/todo.json\")); assert d[\"title\"]' && test -f app/server.py",
        vec![],
    );
    v.check_ports = vec![18431];
    let q = probe_plan(&v);
    assert_eq!(q.targets, strs(&["out/todo.json", "app/server.py"]));
    assert_eq!(
        q.probes.iter().map(|x| x.kind).collect::<Vec<_>>(),
        [ProbeKind::Empty, ProbeKind::Junk, ProbeKind::JunkList, ProbeKind::Truncated, ProbeKind::StubServer]
    );
    assert_eq!(q.probes[1].files[0].text, "{\"title\": \"zz-probe\"}\n");
    assert_eq!(q.probes[2].files[0].text, "[{\"title\": \"zz-probe\"}]\n");
    assert_eq!(q.probes[3].files[0].text, "{\"title\": \"zz-pro");
    let stub = &q.probes[4].files[1].text;
    assert!(stub.contains("ThreadingHTTPServer((\"127.0.0.1\", p), _ZZ) for p in [18431]"));
    assert!(stub.contains("if __name__ == \"__main__\":"));
    assert_eq!(q.probes[4].files[0].text, "{\"title\": \"zz-probe\"}\n"); // 자료 대상은 엉터리 값
}

#[test]
fn 돌리기() {
    let weak = FakeTree::new(toy_steps(), weak_verdict);
    let r = run_probes(&toy_view(), &failed_baseline(&toy_steps(), &[]), &weak, 600.0);
    assert_eq!(r.note, None);
    assert_eq!(r.runs.iter().map(|x| x.kind).collect::<Vec<_>>(), [ProbeKind::Empty, ProbeKind::Junk, ProbeKind::Truncated]);
    assert_eq!(r.hits.iter().map(|h| h.step.as_str()).collect::<Vec<_>>(), ["scenario-1", "scenario-2"]);
    assert_eq!(r.hits.first().map(|h| h.kinds.clone()), Some(vec![ProbeKind::Junk, ProbeKind::Truncated]));
    assert!(weak.files.borrow().is_empty()); // 끝나면 깨끗한 기준으로 되돌림
    assert_eq!(weak.judged.borrow().as_slice(), ["probe-empty", "probe-junk", "probe-truncated"]);
    assert_eq!(weak.resets.get(), 4);
    let strong = FakeTree::new(toy_steps(), strong_verdict);
    assert!(run_probes(&toy_view(), &failed_baseline(&toy_steps(), &[]), &strong, 600.0).hits.is_empty());
}

#[test]
fn 이미통과() {
    let t = FakeTree::new(toy_steps(), weak_verdict);
    let r = run_probes(&toy_view(), &failed_baseline(&toy_steps(), &["scenario-2"]), &t, 600.0);
    assert_eq!(r.hits.iter().map(|h| h.step.as_str()).collect::<Vec<_>>(), ["scenario-1"]);
    let all = run_probes(&toy_view(), &failed_baseline(&toy_steps(), &["scenario-1", "scenario-2"]), &t, 600.0);
    assert!(all.runs.is_empty() && all.note.as_deref().is_some_and(|n| n.contains("판정 미리 돌리기 카드")));
}

#[test]
fn 까닭() {
    let mut t = FakeTree::new(toy_steps(), weak_verdict);
    t.sec = 200.0; // 걸음 둘 = 탐침마다 400초: 둘째까지 돌고 셋째는 상한
    let r = run_probes(&toy_view(), &failed_baseline(&toy_steps(), &[]), &t, 600.0);
    assert_eq!(r.runs.iter().map(|x| x.note.is_none()).collect::<Vec<_>>(), [true, true, false]);
    assert_eq!(r.runs[2].note.as_deref(), Some("탐침에 쓸 판정 시간(600초)을 넘어 돌리지 않았습니다"));
    let mut u = FakeTree::new(toy_steps(), weak_verdict);
    u.put_fail = Some("app/todos.py".to_string());
    let r = run_probes(&toy_view(), &failed_baseline(&toy_steps(), &[]), &u, 600.0);
    assert!(r.runs.iter().all(|x| x.note.as_deref() == Some("링크라 쓰지 않았습니다")));
    let none = JudgeView::new(1, "u1", "t", "true", vec![]);
    let r = run_probes(&none, &failed_baseline(&[step("check", None, None)], &[]), &t, 60.0);
    assert!(r.note.as_deref().is_some_and(|n| n.contains("찾지 못해")));
}

#[test]
fn 탐침_카드() {
    let hit = ProbeHit {
        step: "scenario-2".to_string(),
        label: "x".to_string(),
        file: Some(".autopilot/scenarios/u1/list.sh".to_string()),
        criterion: Some("AC-2".to_string()),
        kinds: vec![ProbeKind::Junk],
    };
    let c = probe_card(1, "u1", "할 일", &hit, &strs(&["app/todos.py"]));
    assert_eq!(c["id"].as_str(), Some("probe-u1-.autopilot/scenarios/u1/list.sh"));
    assert_eq!(c["recommended"].as_str(), Some("fix_check"));
    let keys: Vec<&str> = c["options"].as_array().unwrap().iter().map(|o| o["key"].as_str().unwrap_or("")).collect();
    assert_eq!(keys, ["fix_check", "keep", "drop"]);
    assert!(c["title"].as_str().is_some_and(|t| t.contains("수용 기준 AC-2")));
    assert!(!c["why"].as_str().unwrap_or("").contains('—'));
}

// MARK: 반례 허용 목록

#[test]
fn 허용() {
    let rows: [(&str, Option<&str>); 16] = [
        ("app/todos.py", None),
        ("App/Todos.PY", None),
        ("NOTES.md", Some("보는 자리")),
        ("conftest.py", Some("설정 파일")),
        ("package.json", Some("설정 파일")),
        ("Makefile", Some("설정 파일")),
        ("pytest.ini", Some("설정 파일")),
        ("pyproject.toml", Some("설정 파일")),
        ("setup.cfg", Some("설정 파일")),
        ("tox.ini", Some("설정 파일")),
        ("lib/sitecustomize.py", Some("설정 파일")),
        (".npmrc", Some("설정 파일")),
        ("x.pth", Some("설정 파일")),
        ("jest.config.js", Some("설정 파일")),
        (".GIT/config", Some("판정·하네스")),
        ("../x.py", Some("상대 경로")),
    ];
    let judge = strs(&[".autopilot/scenarios/u1/list.sh"]);
    let targets = strs(&["app/todos.py", "package.json"]);
    for (p, want) in rows {
        let why = case_path_problem(p, &judge, &targets);
        match want {
            Some(w) => assert!(why.as_deref().is_some_and(|y| y.contains(w)), "{p}: {why:?}"),
            None => assert_eq!(why, None, "{p}"),
        }
    }
}

#[test]
fn 겹침() {
    let v = JudgeView::new(1, "u1", "t", "test -f out.json && test -f OUT.json && test -f .Git", vec![]);
    assert_eq!(probe_targets(&v), strs(&["out.json"]));
}

// MARK: 추가 — Swift 시험에 없는 이 이식의 몫(정규식을 손 훑기로 옮긴 경계·Foundation 과 다른 자리)

#[test]
fn 추가_경로_훑기_경계() {
    // 확장자 목록 밖·확장자가 조각 앞부분뿐인 것은 아니다
    assert!(judge_paths("a.jsonx b.pyc c.mdx d.shell").is_empty());
    // 확장자 뒤의 점은 허용(문장 끝), 뒤의 글자·숫자·_·-·/ 는 불허
    assert_eq!(judge_paths("see out.json."), strs(&["out.json"]));
    assert!(judge_paths("out.json-x out.json_x out.json/x").is_empty());
    // 앞의 글자가 [A-Za-z0-9_./~$-] 이면 시작하지 않는다
    assert!(judge_paths("$out.json ~out.json -out.json .out.json").is_empty());
    assert_eq!(judge_paths("(out.json) 'out.json' \"out.json\" =out.json"), strs(&["out.json"]));
    // 폴더 조각이 여럿·점이 든 조각·가장 긴 확장자 연쇄
    assert_eq!(judge_paths("a/b.c/d.tar.gz.md"), strs(&["a/b.c/d.tar.gz.md"]));
    assert_eq!(judge_paths("x.json.py"), strs(&["x.json.py"]));
    assert_eq!(judge_paths("x.json.bak"), strs(&["x.json"]));
    // 대문자 확장자는 목록 밖이다
    assert!(judge_paths("A.JSON").is_empty());
    // 주소 안의 경로는 지우고, 주소 밖의 것만 본다
    assert_eq!(judge_paths("ftp://h/a.txt b.txt"), strs(&["b.txt"]));
    // 절대 경로·.. 은 scenario_path_ok 가 거른다
    assert!(judge_paths("../a.txt /abs/a.txt").is_empty());
}

#[test]
fn 추가_test_플래그와_스크립트_경계() {
    assert_eq!(judge_paths("[ -f 'x/y' ]"), strs(&["x/y"]));
    assert_eq!(judge_paths("[ -e \"zz\" ]"), strs(&["zz"]));
    assert!(judge_paths("attest -f x").is_empty()); // \btest 라 단어 가운데는 아니다
    assert!(judge_paths("test -d x").is_empty()); // -e -f -s 만
    assert_eq!(judge_run_scripts("python3 -m pytest tests/a.py"), Vec::<String>::new()); // pytest 는 조각이 없어 거른다
    assert_eq!(judge_run_scripts("bash -ex 'run.sh'"), strs(&["run.sh"]));
    assert_eq!(judge_run_scripts("sh a.sh;sh b.sh"), strs(&["a.sh", "b.sh"]));
    assert_eq!(judge_run_scripts("(node x/main.mjs)"), strs(&["x/main.mjs"]));
    assert!(judge_run_scripts("shx a.sh").is_empty());
    assert!(judge_run_scripts("").is_empty());
}

#[test]
fn 추가_글자_경계() {
    // 2~80자(문자 수), 글자·숫자가 하나는 있어야, 달러·역따옴표·역슬래시가 든 큰따옴표 글은 받지 않는다
    assert_eq!(judge_literals(&["x \"ab\" y"]), strs(&["ab"]));
    assert!(judge_literals(&["x \"a\" y"]).is_empty());
    assert!(judge_literals(&["\"a$b\" \"a`b\" \"a\\b\""]).is_empty());
    assert_eq!(judge_literals(&["'a$b'"]), strs(&["a$b"]));
    let long = format!("\"{}\"", "가".repeat(81));
    assert!(judge_literals(&[long.as_str()]).is_empty());
    let ok = format!("\"{}\"", "가".repeat(80));
    assert_eq!(judge_literals(&[ok.as_str()]).len(), 1);
    // 같은 글자는 한 번, 가장자리 빈칸은 뗀다
    assert_eq!(judge_literals(&["\" ab \" 'ab'"]), strs(&["ab"]));
    // 상한 24
    let many: String = (0..30).map(|i| format!("\"lit{i}\" ")).collect();
    let got = judge_literals(&[many.as_str()]);
    assert_eq!(got.len(), PROBE_LITERAL_MAX);
    assert_eq!(got[0], "lit0");
}

#[test]
fn 추가_탐침_꼴() {
    // 대상 꼴마다: 텍스트(글 대상)·sh·mjs·jsonl
    let v = JudgeView::new(1, "u1", "t", "grep -q 'abc' a.md && sh b.sh_x && test -f c.sh && test -f d.mjs && test -f e.jsonl", vec![]);
    let p = probe_plan(&v);
    assert_eq!(p.targets, strs(&["a.md", "c.sh", "d.mjs", "e.jsonl"]));
    let junk = &p.probes[1].files;
    assert_eq!(junk[0].text, "abc zz-probe\n");
    assert!(junk[1].text.starts_with("#!/bin/sh\n# 반대 탐침(Claude Board)") && junk[1].text.ends_with("# abc\nexit 3\n"));
    assert!(junk[2].text.contains("const ZZ_PROBE = [\"abc\"];\nprocess.exit(3);\n"));
    assert_eq!(junk[3].text, "{\"abc\": \"zz-probe\"}\n"); // 키꼴 글자가 키가 된다
    let cut = &p.probes[2].files;
    assert_eq!(cut[0].text, "a"); // "abc" 의 앞 절반(3/2=1 글자)
    assert!(cut[1].text.ends_with("\nif true; then\n"));
    assert!(cut[2].text.contains("function zz_probe( {\n"));
    assert_eq!(cut[3].text, "{\"abc\": \"zz-pro");
    // 키꼴 글자가 하나도 없으면 zz_probe
    let k = probe_plan(&JudgeView::new(1, "u1", "t", "test -f out.json", vec![]));
    assert_eq!(k.probes[1].files[0].text, "{\"zz_probe\": \"zz-probe\"}\n");
    assert_eq!(k.probes[2].files[0].text, "[{\"zz_probe\": \"zz-probe\"}]\n"); // JSON 대상이라 목록 탐침이 끼어든다
    assert_eq!(k.probes[3].files[0].text, "{\"zz_probe\": \"zz-pro");
    // 포트가 있어도 sh 대상뿐이면 가짜 서버는 없다(코드 대상이 없다)
    let mut sh_only = JudgeView::new(1, "u1", "t", "test -f c.sh", vec![]);
    sh_only.check_ports = vec![18431];
    assert!(!probe_plan(&sh_only).probes.iter().any(|x| x.kind == ProbeKind::StubServer));
    // 범위 밖 포트(1023·65536)는 거른다
    let mut bad = JudgeView::new(1, "u1", "t", "test -f s.mjs", vec![]);
    bad.check_ports = vec![1023, 65536];
    assert!(!probe_plan(&bad).probes.iter().any(|x| x.kind == ProbeKind::StubServer));
    bad.check_ports = vec![1024, 65535];
    let q = probe_plan(&bad);
    let stub = &q.probes.last().unwrap().files[0].text;
    assert!(q.probes.last().unwrap().kind == ProbeKind::StubServer && stub.contains("for (const p of [1024, 65535])"));
    // 대상이 없으면 탐침도 없다
    let none = probe_plan(&JudgeView::new(1, "u1", "t", "true", vec![]));
    assert!(none.targets.is_empty() && none.literals.is_empty() && none.probes.is_empty());
}

#[test]
fn 추가_종류_문자열과_실행_json() {
    for k in ProbeKind::ALL {
        assert_eq!(ProbeKind::from_raw(k.as_str()), Some(k));
        assert!(!probe_label(k).is_empty());
    }
    assert_eq!(ProbeKind::JunkList.as_str(), "junk_list");
    assert_eq!(ProbeKind::StubServer.as_str(), "stub_server");
    let r = ProbeRun { kind: ProbeKind::Junk, passed_steps: strs(&["check"]), passed: false, sec: 1.23456, note: Some("n".to_string()) };
    let j = probe_run_json(&r);
    assert_eq!(j["kind"].as_str(), Some("junk"));
    assert_eq!(j["sec"].as_f64(), Some(1.235));
    assert_eq!(j["note"].as_str(), Some("n"));
    assert_eq!(j["passed_steps"][0].as_str(), Some("check"));
    let r2 = ProbeRun { note: None, ..r };
    assert!(probe_run_json(&r2).get("note").is_none());
}

#[test]
fn 추가_끊긴_기준은_돌리지_않는다() {
    let t = FakeTree::new(toy_steps(), weak_verdict);
    let mut b = failed_baseline(&toy_steps(), &[]);
    b.results[0].interrupted = true;
    let r = run_probes(&toy_view(), &b, &t, 600.0);
    assert!(r.runs.is_empty() && r.note.as_deref().is_some_and(|n| n.contains("끊겨")));
    assert_eq!(t.resets.get(), 0);
}

#[test]
fn 추가_설정_파일_꼴() {
    assert!(judge_config_path("a/b/Makefile") && judge_config_path(".github/workflows/x.yml") && judge_config_path("web/node_modules/x/a.js"));
    assert!(judge_config_path("vite.config.ts") && judge_config_path(".babelrc.json") && judge_config_path(".mocharc.yml"));
    assert!(!judge_config_path("app/todos.py") && !judge_config_path("docs/config.md") && !judge_config_path(""));
    // 이름만 있는 경로·끝이 빈 경로도 안전하다
    assert!(!judge_config_path("/"));
}

// MARK: 비판 세션

// 비판 결과 폴더 가짜 — 지도에 든 파일만 읽는다(APDraftFiles 가 결과 폴더에서 쓰는 쪽)
struct FakeOut {
    files: HashMap<String, String>,
}

impl DraftFiles for FakeOut {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.files.get(name).map(|s| s.as_bytes().to_vec())
    }

    fn list(&self, _folder: &str) -> Vec<String> {
        vec![]
    }
}

fn critic_units() -> Vec<CriticUnit> {
    vec![
        CriticUnit::new(
            "u1",
            "할 일 만들기·목록 API",
            "할 일 경로를 만든다",
            vec![Criterion::new("AC-1", "POST"), Criterion::new("AC-2", "GET")],
            "",
            vec![
                CriticScenario::new(
                    ".autopilot/scenarios/u1/routes.py",
                    ScenarioKind::Shell,
                    Some("AC-1"),
                    "/J/u1/1-routes.py",
                ),
                CriticScenario::new(
                    ".autopilot/scenarios/u1/list.sh",
                    ScenarioKind::Shell,
                    Some("AC-2"),
                    "/J/u1/2-list.sh",
                ),
            ],
        ),
        CriticUnit::new(
            "u2",
            "README 안내",
            "안내를 쓴다",
            vec![Criterion::new("AC-3", "읽힌다")],
            "grep -q '할 일' NOTES.md",
            vec![],
        ),
    ]
}

fn critic_targets() -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    m.insert("u1".to_string(), vec!["app/todos.py".to_string()]);
    m.insert("u2".to_string(), vec!["NOTES.md".to_string()]);
    m
}

const FINDINGS_JSON: &str = r#"[{"id":"f1","unit":"u1","criterion":"AC-2","judge":".autopilot/scenarios/u1/list.sh","kind":"counterexample","why":"GET 글자만 있고 경로가 없는 파일도 통과합니다","files":["app/todos.py"]},
 {"id":"f2","unit":"u2","criterion":"AC-3","kind":"unverifiable","why":"읽기 쉬운지는 반례 파일로 가리지 못합니다"},
 {"id":"f3","unit":"u1","criterion":"AC-1","kind":"interpretation","why":"경로만 있으면 되는지 만들기가 되는지 갈립니다","readings":[{"key":"route","label":"경로만 있으면 됨"},{"key":"works","label":"만들기가 실제로 됨"}],"recommended":"works"},
 {"id":"f4","unit":"u1","judge":".autopilot/scenarios/u1/list.sh","kind":"counterexample","why":"시나리오를 바꾸면 통과합니다","files":[".autopilot/scenarios/u1/list.sh"]},
 {"id":"f5","unit":"u1","kind":"counterexample","why":"반례 파일이 없습니다","files":["app/none.py"]},
 {"id":"f6","unit":"u9","kind":"unverifiable","why":"없는 단위"},
 {"id":"x7","unit":"u1","kind":"unverifiable","why":"id 꼴"},
 {"id":"f8","unit":"u1","judge":"check","kind":"unverifiable","why":"판정 명령 없는 단위"},
 {"id":"f9","unit":"u1","kind":"interpretation","why":"읽기 하나","readings":[{"key":"a","label":"하나"}]}]"#;

fn case_files() -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();
    m.insert("findings.json".to_string(), FINDINGS_JSON.to_string());
    m.insert("cases/f1/app/todos.py".to_string(), "# GET /todos 는 아직 만들지 않음\nROUTES = []\n".to_string());
    m.insert("cases/f4/.autopilot/scenarios/u1/list.sh".to_string(), "exit 0\n".to_string());
    m
}

fn drafted_unit(id: &str, title: &str, prompt: &str, check: &str) -> DraftedUnit {
    DraftedUnit {
        id: id.to_string(),
        title: title.to_string(),
        kind: "dev".to_string(),
        folder: String::new(),
        prompt: prompt.to_string(),
        check: check.to_string(),
        est_min: 30.0,
        depends: vec![],
        criteria: vec![],
        scenarios: vec![],
        check_ports: vec![],
    }
}

// MARK: 비판 세션 시험(원본 CriticTests)

#[test]
fn 모델() {
    for (author, critic) in [("opus", "sonnet"), ("sonnet", "opus"), ("haiku", "sonnet"), ("claude-opus-4-1", "sonnet"), ("claude-sonnet-4-5", "opus"), ("zz-model", "sonnet")] {
        assert_eq!(critic_model(author), critic, "{author}");
        assert_ne!(model_family(&critic_model(author)), model_family(author), "{author}");
    }
}

#[test]
fn 모델고르기() {
    assert_eq!(critic_model_pick("opus", None), ("sonnet".to_string(), None));
    assert_eq!(critic_model_pick("opus", Some("haiku")), ("haiku".to_string(), None));
    let same = critic_model_pick("opus", Some("claude-opus-4-1"));
    assert!(same.0 == "sonnet" && same.1.as_deref().is_some_and(|n| n.contains("같은 계열")));
    assert!(critic_model_pick("sonnet", Some("a b")).1.as_deref().is_some_and(|n| n.contains("꼴이 틀려")));
}

#[test]
fn 비판_글() {
    let x = CriticInput::new(
        critic_units(),
        Some("할 일 API 를 만든다".to_string()),
        "/J",
        Some("/J/tree.txt".to_string()),
        Some("/B".to_string()),
        vec!["link-1.json".to_string()],
        "/O",
        None,
    );
    let p = critic_prompt(&x);
    assert!(p.starts_with(CRITIC_HEAD_LINE));
    assert!(p.contains(CRITIC_RUBRIC));
    assert!(p.contains("/J/u1/2-list.sh") && p.contains("grep -q '할 일' NOTES.md") && p.contains("할 일 API 를 만든다"));
    assert!(p.contains("믿지 않는 자료") && p.contains("결과 폴더 /O 하나뿐"));
    assert!(!p.contains(DRAFT_HEAD_LINE) && !p.contains("—"));
}

#[test]
fn 읽기() {
    let out = FakeOut { files: case_files() };
    let (fs, problems) = critic_parse(&out, &critic_units(), &critic_targets());
    assert_eq!(fs.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["f1", "f2", "f3", "f4", "f5"]);
    assert_eq!(
        fs.iter().map(|f| f.kind).collect::<Vec<_>>(),
        vec![FindingKind::Counterexample, FindingKind::Unverifiable, FindingKind::Interpretation, FindingKind::Unverifiable, FindingKind::Unverifiable]
    );
    assert!(fs[3].refusal.as_deref().is_some_and(|r| r.contains("판정·하네스 자리라")) && fs[3].why == "시나리오를 바꾸면 통과합니다");
    assert!(fs[4].refusal.as_deref().is_some_and(|r| r.contains("판정이 보는 자리(app/todos.py)가 아닌 파일")));
    assert_eq!(fs[2].recommended.as_deref(), Some("works"));
    assert_eq!(problems.len(), 4);
    let none = FakeOut { files: HashMap::new() };
    assert_eq!(critic_parse(&none, &critic_units(), &critic_targets()).1, vec!["결과 파일 findings.json 이 없습니다"]);
}

#[test]
fn 확인() {
    let (fs, _) = critic_parse(&FakeOut { files: case_files() }, &critic_units(), &critic_targets());
    let cf = case_files();
    let read = |p: &str| cf.get(p).map(|s| s.as_bytes().to_vec());
    let views: HashMap<String, JudgeView> = HashMap::from([("u1".to_string(), toy_view())]);
    let base = HashMap::from([("u1".to_string(), failed_baseline(&toy_steps(), &[]))]);
    let weak = FakeTree::new(toy_steps(), weak_verdict);
    let r = verify_findings(&fs, &views, &base, &read, &weak);
    assert_eq!(r.iter().map(|x| verdict_key(&x.verdict)).collect::<Vec<_>>(), vec!["confirmed", "unverified", "reading", "unverified", "unverified"]);
    assert_eq!(r[3].verdict, Verdict::Unverified(fs[3].refusal.clone().unwrap_or_default()));
    assert_eq!(r[1].verdict, Verdict::Unverified("비판 세션이 반례를 만들지 못했다고 적었습니다".to_string()));
    assert_eq!(r[0].step.as_deref(), Some("scenario-2"));
    assert!(weak.files.borrow().is_empty());
    let strong = FakeTree::new(toy_steps(), strong_verdict);
    let only = vec![fs[0].clone()];
    assert_eq!(verdict_key(&verify_findings(&only, &views, &base, &read, &strong)[0].verdict), "refuted");
    let base2 = HashMap::from([("u1".to_string(), failed_baseline(&toy_steps(), &["scenario-2"]))]);
    let pre = verify_findings(&only, &views, &base2, &read, &weak);
    assert_eq!(pre[0].verdict, Verdict::Unverified("그 판정이 일하기 전부터 통과해 반례로 가리지 못합니다(판정 미리 돌리기 카드로)".to_string()));
}

#[test]
fn 카드() {
    let mut f = CriticFinding::new("f1", "u1", FindingKind::Counterexample, "글자만");
    f.criterion = Some("AC-2".to_string());
    f.judge = Some(".autopilot/scenarios/u1/list.sh".to_string());
    f.files = vec!["app/todos.py".to_string()];
    let c1 = critic_card(&CriticResult::new(f.clone(), Verdict::Confirmed), 1, "할 일").unwrap();
    assert!(c1["kind"].as_str() == Some("critic") && c1["recommended"].as_str() == Some("fix_check") && c1["id"].as_str() == Some("critic-f1"));
    let c2 = critic_card(&CriticResult::new(f.clone(), Verdict::Unverified("까닭".to_string())), 1, "할 일").unwrap();
    assert!(c2["kind"].as_str() == Some("critic_unverified") && c2["recommended"].as_str() == Some("keep") && c2["title"].as_str().is_some_and(|t| t.starts_with("확인 안 됨")));
    let mut g = f.clone();
    g.kind = FindingKind::Interpretation;
    g.readings = vec![
        DraftOption { key: "a".to_string(), label: "가".to_string(), note: String::new() },
        DraftOption { key: "b".to_string(), label: "나".to_string(), note: String::new() },
    ];
    g.recommended = Some("b".to_string());
    let c3 = critic_card(&CriticResult::new(g, Verdict::Reading), 1, "할 일").unwrap();
    assert!(c3["kind"].as_str() == Some("critic_reading") && c3["recommended"].as_str() == Some("b"));
    assert!(critic_card(&CriticResult::new(f, Verdict::Refuted), 1, "할 일").is_none());
}

// MARK: 고치기 고리 시험(원본 FixLoopTests)

#[test]
fn 차례() {
    for (done, confirmed, want) in [(0_i64, 0_i64, "stop"), (0, 2, "offer1"), (1, 1, "offer2"), (1, 0, "stop"), (2, 3, "stop")] {
        match fix_next(done, confirmed) {
            FixNext::Offer { round } => assert_eq!(want, format!("offer{round}"), "{done}·{confirmed}"),
            FixNext::Stop(why) => {
                assert_eq!(want, "stop", "{done}·{confirmed}");
                if done == 1 {
                    assert!(why.contains("새로 확인된 지적이 없어"), "{why}");
                }
                if done == 2 {
                    assert!(why.contains("2번을 다 썼습니다"), "{why}");
                }
            }
        }
    }
}

#[test]
fn 고치기_글() {
    let base = DraftInput { topic: "할 일 API".to_string(), new_folder: true, repo: "/R".to_string(), out: "/O".to_string(), ..Default::default() };
    let p = draft_fix_prompt(&base, 1, &[FixItem::new("critic", "u1", Some(".autopilot/scenarios/u1/list.sh".to_string()), "글자만", vec!["f1/app/todos.py".to_string()])], "/F");
    assert!(p.starts_with(&format!("{DRAFT_HEAD_LINE} {DRAFT_FIX_MARKER}")));
    assert!(p.contains("1. 단위 u1 · 시나리오 .autopilot/scenarios/u1/list.sh · 비판 세션 반례: 글자만"));
    assert!(p.contains("/F") && p.contains("약하게 바꾸지 않습니다") && p.contains("units.json"));
    assert!(!p.contains("—"));
}

#[test]
fn 약해짐() {
    let mut u1 = drafted_unit("u1", "a", "p", "test -f a");
    u1.criteria = vec![Criterion::new("AC-1", "x"), Criterion::new("AC-2", "y")];
    u1.scenarios = vec![
        DraftScenario { file: "r.py".to_string(), kind: ScenarioKind::Shell, criterion: Some("AC-1".to_string()) },
        DraftScenario { file: "l.sh".to_string(), kind: ScenarioKind::Shell, criterion: Some("AC-2".to_string()) },
    ];
    let old = vec![u1.clone(), drafted_unit("u2", "b", "p", "x")];
    assert!(judge_weakening(&old, &old).is_empty());
    let mut renamed = old.clone();
    renamed[0].scenarios[1].file = "l2.sh".to_string();
    assert!(judge_weakening(&old, &renamed).is_empty()); // 같은 기준을 보는 시나리오로 바꾼 것은 약해진 것이 아님
    let mut weak = old.clone();
    weak[0].check = String::new();
    weak[0].scenarios.pop();
    weak.pop();
    assert_eq!(
        judge_weakening(&old, &weak),
        strs(&["단위 u1 의 수용 기준 AC-2 를 보던 시나리오가 없어졌습니다", "단위 u1 의 판정 명령이 빠졌습니다", "단위 u1 의 시나리오 l.sh 가 빠졌습니다", "단위 u2(b)가 빠졌습니다"])
    );
    let c = weak_card(1, &strs(&["x"]));
    assert!(c["recommended"].as_str() == Some("reject") && c["id"].as_str() == Some("fix-weak-1"));
}

#[test]
fn 설정반례() {
    let u = vec![CriticUnit::new("u1", "t", "p", vec![], "python3 -m pytest -q", vec![])];
    let files = FakeOut {
        files: HashMap::from([
            (
                "findings.json".to_string(),
                r#"[{"id":"f1","unit":"u1","judge":"check","kind":"counterexample","why":"시험을 건너뜀","files":["conftest.py"]}]"#.to_string(),
            ),
            ("cases/f1/conftest.py".to_string(), "collect_ignore_glob = ['*']\n".to_string()),
        ]),
    };
    let (fs, _) = critic_parse(&files, &u, &HashMap::from([("u1".to_string(), Vec::<String>::new())]));
    assert_eq!(fs.iter().map(|f| f.kind).collect::<Vec<_>>(), vec![FindingKind::Unverifiable]);
    assert!(fs[0].refusal.as_deref().is_some_and(|r| r.contains("conftest.py") && r.contains("설정 파일")));
}
