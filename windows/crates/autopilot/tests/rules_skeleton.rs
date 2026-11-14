//! 뼈대 규칙(`rules::skeleton`) 시험. 원본: `Tests/AutopilotRulesTests/DraftTests.swift` 의 Skeleton 몫 —
//! `SkeletonTests` 스위트 전부와 `DraftSecurityTests` 의 `판`·`꼴`(⑪-2 자동 보안 검토): `SkeletonTests.fastapi`(`skeleton_fastapi`) ·
//! `SkeletonTests.spring`(`skeleton_spring`) · `SkeletonTests.둘`(`skeleton_둘`) · `SkeletonTests.직접입력`(`skeleton_직접입력`) ·
//! `SkeletonTests.도구없음`(`skeleton_도구없음`) · `SkeletonTests.만들기`(`skeleton_만들기`) · `SkeletonTests.이어받기`(`skeleton_이어받기`) ·
//! `DraftSecurityTests.판`(`security_판`) · `DraftSecurityTests.꼴`(`security_꼴`).
//! 이 파일에 없는 Swift 시험(같은 파일의 Draft.swift 몫 — Draft.swift 는 아직 이식 전이라 여기에 둘 수 없다): DraftCheckTests(검사·값·남는파일·명세·금지줄) ·
//! HarnessPointTests.찾기 · DraftSessionTests(인자·경로규칙·설정·글·대기열) · DraftSweepTests.정리 · DraftSecurityTests.부정어(apForbiddenLines).
//! 또한 `apScenarioCommitMessage` 는 DraftTests.swift 어디에도 시험이 없다(다른 시험 파일의 몫인지는 이 파일이 보지 않는다).
//! 표(`injectRows`, 6 줄)는 Swift 에서 그대로 옮겼다(손으로 다시 치지 않았다): 문자열 리터럴은 `#"…"#` → `r#"…"#`, `evil` 표는 같은 순서로.
//! 끝의 `extra_` 시험은 Swift 시험에 없는 것(손으로 옮긴 경계·원본 FakeWork 가 실패를 못 내는 자리)이다.
//! 가짜 어댑터(뼈대 쓸 곳·도구 찾기)는 이 파일에만 둔다(원본과 같다). 값은 장난감(zz-toy·example.com).

use claude_board_autopilot::rules::scenarios::ToolProbe;
use claude_board_autopilot::rules::skeleton::{
    fastapi_template, gen_command_text, make_skeleton, package_part, query_value, seed_parse, seed_problem, skeleton_message, skeleton_plan, version_ok, GenKind, Seed, SkeletonPlan, SkeletonWork,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

// MARK: 가짜 어댑터(원본 DraftTests.swift 의 FakeWork·ToolProbe)

struct FakeProbe {
    have: Vec<&'static str>,
}

impl ToolProbe for FakeProbe {
    fn program(&self, _path: &str) -> bool {
        false
    }
    fn command(&self, name: &str) -> Option<String> {
        if self.have.contains(&name) {
            Some(format!("/bin/{name}"))
        } else {
            None
        }
    }
    fn browser(&self) -> Option<String> {
        None
    }
    fn node_module(&self, _name: &str, _dir: &str) -> bool {
        false
    }
}

struct FakeWork {
    wrote: RefCell<HashMap<String, String>>,
    ran: RefCell<Vec<Vec<String>>>,
    got: RefCell<Vec<String>>,
    commits: RefCell<Vec<String>>,
    run_code: Cell<i32>,
}

impl FakeWork {
    fn new() -> Self {
        FakeWork {
            wrote: RefCell::new(HashMap::new()),
            ran: RefCell::new(Vec::new()),
            got: RefCell::new(Vec::new()),
            commits: RefCell::new(Vec::new()),
            run_code: Cell::new(0),
        }
    }
}

impl SkeletonWork for FakeWork {
    fn write(&self, relative: &str, text: &str) -> bool {
        self.wrote.borrow_mut().insert(relative.to_string(), text.to_string());
        true
    }
    fn run(&self, argv: &[String], _dir: &str) -> (i32, String) {
        self.ran.borrow_mut().push(argv.to_vec());
        (self.run_code.get(), "출력".to_string())
    }
    fn download(&self, url: &str, _into: &str) -> Option<String> {
        self.got.borrow_mut().push(url.to_string());
        None
    }
    fn commit(&self, message: &str) -> (Option<String>, Option<String>) {
        self.commits.borrow_mut().push(message.to_string());
        (Some("abc1234".to_string()), None)
    }
}

/// 도구가 모두 있는 가짜(원본의 `ToolProbe(have: [...])` 자리)
fn all_probe() -> FakeProbe {
    FakeProbe { have: vec!["python3", "node", "npm", "npx", "unzip", "java"] }
}

/// 원본 보조 함수 `slot`의 자리 — 칸을 만들고(Swift 기본값 origin "picked"), 이름 인자로 고친 남김을 클로저로 받는다
fn slot(key: &str, name: &str, set: impl FnOnce(&mut claude_board_autopilot::rules::stack::StackSlot)) -> claude_board_autopilot::rules::stack::StackSlot {
    use claude_board_autopilot::rules::stack::StackSlot;
    let mut s = StackSlot { key: key.to_string(), name: name.to_string(), origin: "picked".to_string(), ..Default::default() };
    set(&mut s);
    s
}

// MARK: 뼈대 계획과 만들기(원본 SkeletonTests)

#[test]
fn skeleton_fastapi() {
    let st = claude_board_autopilot::rules::stack::StackSet {
        back: Some(slot("fastapi", "Python · FastAPI", |s| {
            s.lang_version = "3.14".to_string();
            s.framework_version = "0.156".to_string();
        })),
        ..Default::default()
    };
    let p = skeleton_plan(&st, "zz-toy");
    assert_eq!(p.steps.len(), 1);
    assert_eq!(p.steps[0].kind, GenKind::Template);
    assert!(!p.steps[0].external());
    assert_eq!(p.steps[0].folder, "");
    let paths: Vec<&str> = p.steps[0].files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["pyproject.toml", "app/__init__.py", "app/main.py", ".gitignore", "README.md"], "{:?}", p.steps[0].files);
    assert!(p.steps[0].files[0].text.contains("requires-python = \">=3.14\""));
    assert!(p.steps[0].files[0].text.contains("fastapi>=0.156"));
    let tools: Vec<&str> = p.steps[0].tools.iter().map(|t| t.command.as_str()).collect();
    assert_eq!(tools, ["python3"]);
    assert!(!p.first_unit);
}

#[test]
fn skeleton_spring() {
    let st = claude_board_autopilot::rules::stack::StackSet {
        back: Some(slot("spring-java", "Java · Spring Boot", |s| {
            s.lang = "Java".to_string();
            s.lang_version = "25".to_string();
            s.framework_version = "4.1.1".to_string();
            s.build = "Gradle Groovy".to_string();
        })),
        ..Default::default()
    };
    let p = skeleton_plan(&st, "zz-Toy");
    assert_eq!(
        p.steps[0].url,
        "https://start.spring.io/starter.zip?type=gradle-project&language=java&bootVersion=4.1.1&javaVersion=25&groupId=com.example&artifactId=zztoy&name=zztoy&packageName=com.example.zztoy&dependencies=web"
    );
    assert!(p.steps[0].external());
    let tools: Vec<&str> = p.steps[0].tools.iter().map(|t| t.command.as_str()).collect();
    assert_eq!(tools, ["unzip", "java"]);
    let k = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            back: Some(slot("spring-kotlin", "Kotlin · Spring Boot", |s| {
                s.build = "Maven".to_string();
            })),
            ..Default::default()
        },
        "zz-toy",
    );
    assert!(k.steps[0].url.contains("type=maven-project&language=kotlin&bootVersion=4.1.1&groupId"), "{}", k.steps[0].url);
    assert!(!k.steps[0].url.contains("javaVersion"));
}

#[test]
fn skeleton_둘() {
    let st = claude_board_autopilot::rules::stack::StackSet {
        front: Some(slot("react", "React (Vite)", |s| {
            s.framework_version = "19.3".to_string();
            s.build = "Vite 8.3".to_string();
        })),
        back: Some(slot("nest", "TypeScript · NestJS", |s| {
            s.framework_version = "12.2".to_string();
        })),
        db: Some(slot("postgresql", "PostgreSQL", |s| {
            s.framework_version = "18".to_string();
        })),
    };
    let p = skeleton_plan(&st, "zz-toy");
    let folders: Vec<&str> = p.steps.iter().map(|s| s.folder.as_str()).collect();
    assert_eq!(folders, ["backend", "frontend", ""]);
    assert_eq!(gen_command_text(&p.steps[0]), "npx --yes @nestjs/cli@12.2 new backend --package-manager npm --skip-git");
    assert_eq!(gen_command_text(&p.steps[1]), "npm create --yes vite@8.3 frontend -- --template react-ts");
    let compose = p.steps[2].files.first().unwrap();
    assert!(compose.text.contains("image: postgres:18"), "{}", compose.text);
    assert_eq!(compose.path, "compose.yaml");
    let n = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            front: Some(slot("next", "Next.js", |s| {
                s.framework_version = "16.4".to_string();
            })),
            ..Default::default()
        },
        "zz",
    );
    assert_eq!(gen_command_text(&n.steps[0]), "npx --yes create-next-app@16.4 . --ts --app --use-npm --yes --disable-git");
}

#[test]
fn skeleton_직접입력() {
    let p = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            back: Some(slot("", "Rust · Axum", |s| {
                s.origin = "custom".to_string();
            })),
            ..Default::default()
        },
        "zz-toy",
    );
    assert!(p.steps.is_empty() && p.first_unit);
    assert!(p.notes.first().unwrap().contains("프로젝트 구성"), "{:?}", p.notes);
    assert_eq!(skeleton_plan(&claude_board_autopilot::rules::stack::StackSet::default(), "zz").notes, ["고른 스택이 없어 뼈대를 만들지 않습니다"]);
}

#[test]
fn skeleton_도구없음() {
    let p = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            front: Some(slot("react", "React (Vite)", |_| {})),
            ..Default::default()
        },
        "zz",
    );
    let w = FakeWork::new();
    let r = make_skeleton(&p, &FakeProbe { have: vec!["python3"] }, &w, true);
    assert!(!r.ok);
    assert_eq!(
        r.problems,
        ["Vite(npm create vite, react-ts): Node.js(node)·npm 를 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 깐 뒤 준비를 다시 하거나 다른 스택을 고르세요"]
    );
    assert!(w.wrote.borrow().is_empty() && w.ran.borrow().is_empty() && w.commits.borrow().is_empty());
}

#[test]
fn skeleton_만들기() {
    let p = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            front: Some(slot("vue", "Vue", |_| {})),
            back: Some(slot("fastapi", "Python · FastAPI", |_| {})),
            ..Default::default()
        },
        "zz-toy",
    );
    let w = FakeWork::new();
    let r = make_skeleton(&p, &FakeProbe { have: vec!["python3", "node", "npm"] }, &w, false);
    assert!(!r.ok);
    let mut keys: Vec<String> = w.wrote.borrow().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys.first().unwrap(), "backend/.gitignore", "{:?}", keys);
    assert!(w.ran.borrow().is_empty() && w.commits.borrow().is_empty());
    let problem = r.problems.first().unwrap();
    assert!(
        problem.starts_with("create-vue(npm create vue): 바깥 요청을 하는 생성기라 돌리지 않았습니다(npm create --yes vue@latest frontend -- --ts --default)"),
        "{}",
        problem
    );

    let w2 = FakeWork::new();
    let r2 = make_skeleton(&p, &FakeProbe { have: vec!["python3", "node", "npm"] }, &w2, true);
    assert!(r2.ok);
    assert_eq!(r2.commit.as_deref(), Some("abc1234"));
    assert_eq!(r2.external, 1);
    assert_eq!(w2.ran.borrow().len(), 1, "{:?}", w2.ran.borrow());
    assert_eq!(*w2.commits.borrow(), vec![skeleton_message(&p)], "{:?}", w2.commits.borrow());

    let w3 = FakeWork::new();
    w3.run_code.set(1);
    let r3 = make_skeleton(&p, &FakeProbe { have: vec!["python3", "node", "npm"] }, &w3, true);
    assert!(r3.problems.first().unwrap().starts_with("create-vue(npm create vue) 이 실패했습니다(종료 코드 1)"), "{:?}", r3.problems);
}

#[test]
fn skeleton_이어받기() {
    let s = Seed {
        repo: "/W/zz".to_string(),
        branch: "autopilot/r1".to_string(),
        base: "live".to_string(),
        base_head: "b1".to_string(),
        head: "h2".to_string(),
    };
    assert_eq!(seed_problem(&s, "r1", Some("h2"), Some("b1"), true), None);
    assert!(seed_problem(&s, "r2", Some("h2"), Some("b1"), true).unwrap().contains("이 실행(autopilot/r2)"));
    assert!(seed_problem(&s, "r1", None, Some("b1"), true).unwrap().contains("가 없습니다"));
    assert!(seed_problem(&s, "r1", Some("h3"), Some("b1"), true).unwrap().contains("준비 뒤에 바뀌었습니다"));
    assert!(seed_problem(&s, "r1", Some("h2"), Some("b9"), true).unwrap().contains("움직였습니다"));
    assert!(seed_problem(&s, "r1", Some("h2"), Some("b1"), false).unwrap().contains("시작하지 않았습니다"));
    let s2 = s.clone();
    assert_eq!(
        seed_parse(&json!({"repo": "/W/zz", "branch": "autopilot/r1", "base": "live", "base_head": "b1", "head": "h2"})),
        Some(s2)
    );
    assert_eq!(seed_parse(&json!({"repo": "/W/zz", "branch": "autopilot/r1", "base": "live", "base_head": "b1", "head": ""})), None);
}

// MARK: 보안 검토(원본 DraftSecurityTests — 표 injectRows 를 그대로 옮겼다)

#[test]
fn security_판() {
    let evil = ["latest; rm -rf ~", "1.0 --registry=http://evil.example.com", "18\n    command: evil", "4.1.1&dependencies=evil", "25&x=1"];
    type Set = claude_board_autopilot::rules::stack::StackSet;
    let rows: Vec<(&str, Set, Vec<&str>)> = vec![
        (
            "Nest 판에 셸·인자",
            Set {
                back: Some(slot("nest", "TypeScript · NestJS", |s| {
                    s.framework_version = evil[1].to_string();
                })),
                ..Default::default()
            },
            vec!["--registry", "evil"],
        ),
        (
            "Next 판에 ;",
            Set {
                front: Some(slot("next", "Next.js", |s| {
                    s.framework_version = evil[0].to_string();
                })),
                ..Default::default()
            },
            vec![";", "rm -rf"],
        ),
        (
            "Spring 판·Java 판에 & 질의 키",
            Set {
                back: Some(slot("spring-java", "Java · Spring Boot", |s| {
                    s.lang_version = evil[4].to_string();
                    s.framework_version = evil[3].to_string();
                })),
                ..Default::default()
            },
            vec!["dependencies=evil", "x=1"],
        ),
        (
            "MySQL 판에 줄바꿈과 YAML 키",
            Set {
                db: Some(slot("mysql", "MySQL", |s| {
                    s.framework_version = evil[2].to_string();
                })),
                ..Default::default()
            },
            vec!["command: evil"],
        ),
        (
            "FastAPI 판에 TOML 줄",
            Set {
                back: Some(slot("fastapi", "Python · FastAPI", |s| {
                    s.lang_version = "3.13\"\nevil = \"1".to_string();
                    s.framework_version = evil[0].to_string();
                })),
                ..Default::default()
            },
            vec!["evil", "rm -rf"],
        ),
        (
            "Vite 빌드 글",
            Set {
                front: Some(slot("react", "React (Vite)", |s| {
                    s.build = "Vite 8.3 --registry=x".to_string();
                })),
                ..Default::default()
            },
            vec!["--registry"],
        ),
    ];
    for (note, st, must_not) in &rows {
        let p = skeleton_plan(st, "zz-toy");
        let text = p
            .steps
            .iter()
            .map(|s| {
                let files: String = s.files.iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join("");
                format!("{} {} {}", gen_command_text(s), s.url, files)
            })
            .collect::<Vec<_>>()
            .join("\n");
        for bad in must_not {
            assert!(!text.contains(bad), "{bad} 가 들어감: {text}");
        }
        if *note != "Vite 빌드 글" {
            assert!(!p.problems.is_empty(), "{note}: {:?} / {text}", p.problems);
        }
        let w = FakeWork::new();
        let res = make_skeleton(&p, &all_probe(), &w, true);
        if !p.problems.is_empty() {
            assert!(!res.ok && w.wrote.borrow().is_empty() && w.ran.borrow().is_empty() && w.got.borrow().is_empty());
        }
    }
}

#[test]
fn security_꼴() {
    for ok in ["25", "4.1.1", "3.14", "0.156", "18", "8.4", "12.2.0"] {
        assert!(version_ok(ok), "{ok}");
    }
    for bad in ["latest", "", "1.0 --x", "1;2", "1.2.3.4.5", "v1", "1..2", "1.2\n"] {
        assert!(!version_ok(bad), "{bad:?}");
    }
    assert_eq!(query_value("a&b=c d"), "a%26b%3Dc%20d");
}

// MARK: 끝의 extra_ 시험은 Swift 시험에 없는 것(손으로 옮긴 경계·원본 FakeWork 가 실패를 못 내는 자리)이다

/// 원본 FakeWork 는 실패를 안 내는 자리(쓰기·받기·커밋이 막힌 걸음을 Swift 시험이 못 본다) — 실패를 내는 가짜
struct BrokenWork {
    fail_write: bool,
    fail_download: bool,
    fail_commit: bool,
}

impl SkeletonWork for BrokenWork {
    fn write(&self, _relative: &str, _text: &str) -> bool {
        !self.fail_write
    }
    fn run(&self, _argv: &[String], _dir: &str) -> (i32, String) {
        (0, String::new())
    }
    fn download(&self, _url: &str, _into: &str) -> Option<String> {
        self.fail_download.then(|| "네트워크가 안 됩니다".to_string())
    }
    fn commit(&self, _message: &str) -> (Option<String>, Option<String>) {
        if self.fail_commit {
            (None, Some("가지를 잠궜다".to_string()))
        } else {
            (Some("abc1234".to_string()), None)
        }
    }
}

#[test]
fn extra_fastapi_틀_본문() {
    let files = fastapi_template("zz-Toy", "3.13", "0.115");
    assert_eq!(
        files[0].text,
        "[project]\nname = \"zztoy\"\nversion = \"0.1.0\"\ndescription = \"Claude Board 자율주행이 만든 FastAPI 뼈대\"\nrequires-python = \">=3.13\"\ndependencies = [\"fastapi>=0.115\", \"uvicorn>=0.30\"]\n\n"
    );
    assert_eq!(files[1].text, "");
    assert_eq!(
        files[2].text,
        "from fastapi import FastAPI\n\napp = FastAPI(title=\"zztoy\")\n\n\n@app.get(\"/health\")\ndef health() -> dict:\n    return {\"status\": \"ok\"}\n\n"
    );
    assert_eq!(files[3].text, "__pycache__/\n*.pyc\n.venv/\n.pytest_cache/\n");
    // 제목은 이름을 꼴에 맞는 글자만 남긴 것(대문자로 고치지 않는다 — 원본 Skeleton.swift:177)
    assert!(files[4].text.starts_with("# zz-Toy\n\nClaude Board 자율주행이 주제로 시작에서 만든 FastAPI 뼈대입니다(보드에 든 틀, 바깥 요청 없음).\n"));
}

/// version_ok 는 손으로 옮긴 바이트 검사자라 꼴 경계를 값으로 못박는다(조각 1~4·자리 1~6)
#[test]
fn extra_꼴_경계() {
    for ok in ["123456", "1.123456", "1.2.3", "1.2.3.4"] {
        assert!(version_ok(ok), "{ok}");
    }
    for bad in ["1234567", "1.1234567", "1.2.3.4.5", "١٢٣", "1.2 "] {
        assert!(!version_ok(bad), "{bad:?}");
    }
    // package_part: a-z·0-9 만 남기고 앞이 소문 글자가 아니면 app 을 앞에 붙인다
    assert_eq!(package_part("zz-Toy"), "zztoy");
    assert_eq!(package_part("3D-Print App"), "app3dprintapp");
    assert_eq!(package_part(""), "app");
    assert_eq!(package_part("예제"), "app");
    // query_value: 한글 등 밖 글자도 바이트마다 퍼센트 인코딩
    assert_eq!(query_value("한&글"), "%ED%95%9C%26%EA%B8%80");
}

#[test]
fn extra_빈계획() {
    let p = SkeletonPlan::default();
    let w = FakeWork::new();
    let r = make_skeleton(&p, &all_probe(), &w, true);
    assert!(r.ok && r.commit.is_none() && r.external == 0 && r.problems.is_empty());
    assert!(w.wrote.borrow().is_empty() && w.ran.borrow().is_empty() && w.got.borrow().is_empty() && w.commits.borrow().is_empty());
}

#[test]
fn extra_seed_parse_부재() {
    for v in [json!(null), json!([]), json!("뼈대"), json!({}), json!({"repo": 1, "branch": "", "base": "", "base_head": "", "head": ""}), json!({"repo": "", "branch": "", "base": "", "base_head": ""})] {
        assert_eq!(seed_parse(&v), None, "{v}");
    }
}

#[test]
fn extra_쓰기실패() {
    let p = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            back: Some(slot("fastapi", "Python · FastAPI", |_| {})),
            ..Default::default()
        },
        "zz-toy",
    );
    let r = make_skeleton(&p, &all_probe(), &BrokenWork { fail_write: true, fail_download: false, fail_commit: false }, true);
    assert!(!r.ok);
    assert_eq!(r.problems, ["FastAPI 틀(보드에 든 것, 공식 생성기가 없음): pyproject.toml 를 쓰지 못했습니다"], "{:?}", r.problems);
}

#[test]
fn extra_받기실패() {
    let p = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            back: Some(slot("spring-java", "Java · Spring Boot", |s| {
                s.lang = "Java".to_string();
                s.lang_version = "25".to_string();
                s.framework_version = "4.1.1".to_string();
            })),
            ..Default::default()
        },
        "zz-toy",
    );
    let r = make_skeleton(&p, &all_probe(), &BrokenWork { fail_write: false, fail_download: true, fail_commit: false }, true);
    assert!(!r.ok && r.external == 1);
    assert_eq!(r.problems, ["Spring Initializr(start.spring.io): 네트워크가 안 됩니다"], "{:?}", r.problems);
}

#[test]
fn extra_커밋실패() {
    let p = skeleton_plan(
        &claude_board_autopilot::rules::stack::StackSet {
            db: Some(slot("postgresql", "PostgreSQL", |s| {
                s.framework_version = "18".to_string();
            })),
            ..Default::default()
        },
        "zz-toy",
    );
    let r = make_skeleton(&p, &all_probe(), &BrokenWork { fail_write: false, fail_download: false, fail_commit: true }, true);
    assert!(!r.ok && r.commit.is_none());
    // 앞의 골음(틀 쓰기)이 다 돌고 커밋에서 멈춘다 — 문제가 커밋 실패 하나여야 걸음이 다 돌았다는 뜻
    assert_eq!(r.problems, ["뼈대를 커밋하지 못했습니다: 가지를 잠궜다"], "{:?}", r.problems);
    assert_eq!(r.done.len(), 1, "{:?}", r.done);
}
