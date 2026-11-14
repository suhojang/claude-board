//! 자율주행 규칙 — 주제로 시작의 뼈대 만들기와 판정 시나리오 커밋. 원본: `Sources/AutopilotRules/Skeleton.swift`(1:1, 이식한 선언):
//! `GenKind`(APGenKind) · `GenFile`(APGenFile) · `GenTool`(APGenTool) · `GenStep`(APGenStep) · `SkeletonPlan`(APSkeletonPlan) ·
//! `package_part`(apPackagePart) · `version_ok`(apVersionOK) · `query_value`(apQueryValue) · `skeleton_plan`(apSkeletonPlan) ·
//! `fastapi_template`(apFastAPITemplate) · `compose_template`(apComposeTemplate) · `gen_command_text`(apGenCommandText) ·
//! `skeleton_tool_problems`(apSkeletonToolProblems) · `SkeletonResult`(APSkeletonResult) · `skeleton_message`(apSkeletonMessage) ·
//! `make_skeleton`(apMakeSkeleton) · `scenario_commit_message`(apScenarioCommitMessage) · `Seed`(APSeed) · `seed_parse`(apSeedParse) ·
//! `seed_problem`(apSeedProblem). 비공개: `ver` · `vite_version` · `readme_title` · `prefix_chars` · `suffix_chars`.
//! 포트 둘: `ToolProbe`(APToolProbe) 는 `rules::scenarios` 가 이미 이식했다(같은 원본 포트) — 여기서 다시 선언하지 않고 그것을 쓴다.
//! `SkeletonWork`(APSkeletonWork) 는 원본은 `Ports.swift` 지만 `ports.rs` 는 통합자 몫이라 이 파일에 둔다. 통합자가 `ports.rs` 로 옮기면
//! 이 파일은 그 트레이트 선언을 `pub use crate::ports::SkeletonWork` 로 바꾼다(규칙 쪽 호출은 그대로). 트레이트는 원본처럼 `&self` 다(가짜는 `RefCell` 로 기록한다).
//! 원본이 부른 이식 전 규칙 파일의 도우미는 사본으로 둔다: `PROJECT_SETUP_TITLE`(apProjectSetupTitle, Draft.swift).
//! Foundation↔Rust 차이(원본 ICU/Foundation 동작을 손으로 옮긴 자리):
//! - `apVersionOK` 의 정규식 `^[0-9]{1,6}(\.[0-9]{1,6}){0,3}$`(ICU, `$` 는 입력 끝만 — 줄바꿈 뒤는 거짓)는 이 크레이트에 정규식 크레이트가 없어
//!   바이트 스캐너로 옮겼다(`stack.rs` 의 전례와 같다).
//! - `addsPercentEncoding`: 허용 집합(영문·숫자·`-._~`) 밖 글자는 UTF-8 바이트마다 대문자 %XX 로 바꾼다.
//! - `prefix(n)`·`suffix(n)`·`filter { isLetter }` 는 원본이 자소 묶음(`Character`) 단위, 여기는 `char` 단위다. 시험 값은 ASCII 와 한글 완성형뿐이라 같은 결과.
//!   `debugDescription` 은 Rust `{:?}` 로 뒀다(둘 다 큰따옴표로 감싼 꼴이지만 이스케이프 방식은 언어 따라 다르다).
//! - `lowercased` 는 유니코드 전체 접기(원본과 같다), 그 뒤 ASCII a-z·0-9 만 남긴다.
//! - `split(separator: " ")` 은 빈 조각을 버리므로 `split(' ')` + 빈 조각 걸러내기와 같다.
//! - 원본 `Int` 는 `i64`, 종료 코드 `Int32` 는 `i32` 다.

use crate::rules::scenarios::ToolProbe;
use crate::rules::stack::{StackSet, StackSlot};
use serde_json::Value;

/// 초안 세션의 첫 단위 제목. 원본: `apProjectSetupTitle`(Draft.swift — 아직 이식 전 규칙 파일이라 여기에 사본을 둔다. 통합 검토에서
/// Draft 이식 뒤 `crate::rules::draft` 쪽 상수로 바꾼다)
const PROJECT_SETUP_TITLE: &str = "프로젝트 구성";

/// 뼈대 걸음의 종류. 원본: `APGenKind`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenKind {
    /// 보드에 든 틀(파일 쓰기만, 바깥 요청 없음)
    Template,
    /// 공식 생성기 명령(npm·npx 등, 패키지 레지스트리에 요청함)
    Command,
    /// 공식 생성기 주소에서 zip 을 받아 풂(start.spring.io)
    Download,
}

/// 틀 파일 하나. 원본: `APGenFile`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenFile {
    pub path: String,
    pub text: String,
}

impl GenFile {
    pub fn new(path: &str, text: &str) -> Self {
        GenFile { path: path.to_string(), text: text.to_string() }
    }
}

/// 만들려면 필요한 도구. 원본: `APGenTool`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenTool {
    /// 로그인 셸 PATH 에서 찾는 이름
    pub command: String,
    /// 사람에게 보이는 이름
    pub label: String,
}

impl GenTool {
    pub fn new(command: &str, label: &str) -> Self {
        GenTool { command: command.to_string(), label: label.to_string() }
    }
}

/// 뼈대 걸음 하나. 원본: `APGenStep`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenStep {
    /// back·front·db
    pub slot: String,
    /// 생성기 이름(쉬운 말)
    pub label: String,
    /// 저장소 뿌리 기준(빈 글자 = 뿌리)
    pub folder: String,
    pub kind: GenKind,
    /// command: 폴더의 부모(뿌리)에서 돌릴 명령
    pub argv: Vec<String>,
    /// download: 받을 주소
    pub url: String,
    /// template 의 파일(상대 경로·글)
    pub files: Vec<GenFile>,
    /// 생성(과 그 뒤 빌드)에 필요한 도구
    pub tools: Vec<GenTool>,
}

impl GenStep {
    pub fn new(slot: &str, label: &str, folder: &str, kind: GenKind) -> Self {
        GenStep {
            slot: slot.to_string(),
            label: label.to_string(),
            folder: folder.to_string(),
            kind,
            argv: Vec::new(),
            url: String::new(),
            files: Vec::new(),
            tools: Vec::new(),
        }
    }

    /// 바깥 요청(레지스트리·생성기 서버)을 하는가. 원본: 계산 속성 `external`
    pub fn external(&self) -> bool {
        self.kind != GenKind::Template
    }
}

/// 스택 → 뼈대 계획. 원본: `APSkeletonPlan`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkeletonPlan {
    pub steps: Vec<GenStep>,
    /// 직접 입력 칸이 있어 첫 단위 "프로젝트 구성"을 모델이 함
    pub first_unit: bool,
    /// 쉬운 말(준비 줄)
    pub notes: Vec<String>,
    /// 판 글자 꼴이 틀림(⑪-2 보안 검토) — 있으면 생성기를 돌리지 않는다
    pub problems: Vec<String>,
}

/// 프로젝트 이름 → 패키지 이름 조각(영문 소문자·숫자, 앞은 글자). 원본: `apPackagePart`
pub fn package_part(name: &str) -> String {
    let s: String = name.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect();
    match s.chars().next() {
        Some(f) if f.is_ascii_lowercase() => s,
        // 앞이 글자가 아니거나 비었으면 앞에 app 을 붙인다
        _ => format!("app{s}"),
    }
}

/// 판 글자 꼴(⑪-2 자동 보안 검토 MEDIUM 1): 숫자 판(1~4 조각, 조각마다 1~6자리)만. 판은 생성기 명령 인자(npx 의 패키지 지정 — "npm:<다른 패키지>" 같은
/// 별칭이 들어가면 다른 패키지를 받는다)·Spring Initializr 주소의 질의 값·보드 틀(compose YAML·pyproject TOML)에 들어가므로, 꼴이 틀리면 기본값으로 바꾸지
/// 않고 계획의 고칠 것으로 둔다(생성기를 돌리지 않음). 판은 공식 버전 목록·앱에 든 목록·주제 글 낱말 대조·사람이 고른 값에서 온다(초안 세션 결과는 닿지 않는다).
/// 원본 정규식 `^[0-9]{1,6}(\.[0-9]{1,6}){0,3}$`(ICU) — `^`·`$` 는 입력 처음·끝만이라(`1.2\n` 는 거짓) 전체를 다 먹어야 참이다(위의 모듈 글 참고).
/// 원본: `apVersionOK`
pub fn version_ok(v: &str) -> bool {
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    // 첫 조각은 1~6자리
    if i == 0 || i > 6 {
        return false;
    }
    if i == b.len() {
        return true;
    }
    // 뒤 조각은 최대 3개, 조각마다 1~6자리, 마지막 조각 뒤는 입력 끝이다(원본 `$`)
    for _ in 0..3 {
        if b[i] != b'.' {
            return false;
        }
        i += 1;
        let piece = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == piece || i - piece > 6 {
            return false;
        }
        if i == b.len() {
            return true;
        }
    }
    // 조각 3개를 채우고도 글이 남으면 원본이 틀린 꼴로 본다
    false
}

/// 주소 질의 값 — 영문·숫자·-._~ 밖은 모두 퍼센트 인코딩(& = 빈칸 등이 다른 질의 키가 되지 않게). 원본: `apQueryValue`
pub fn query_value(v: &str) -> String {
    let mut out = String::new();
    for c in v.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~') {
            out.push(c);
            continue;
        }
        let mut buf = [0u8; 4];
        for byte in c.encode_utf8(&mut buf).as_bytes() {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn prefix_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn suffix_chars(s: &str, n: usize) -> String {
    let all = s.chars().count();
    if all <= n {
        return s.to_string();
    }
    s.chars().skip(all - n).collect()
}

/// 빈 판은 기본값, 꼴이 틀린 판은 기본값으로 바꾸지 않고 계획의 고칠 것으로 둔다. 원본: 비공개 `ver`
fn ver(v: &str, fallback: &str, what: &str, p: &mut SkeletonPlan) -> String {
    if v.is_empty() {
        return fallback.to_string();
    }
    if version_ok(v) {
        return v.to_string();
    }
    p.problems.push(format!(
        "{} 판 글자 꼴이 틀려 생성기를 돌리지 않습니다(숫자 판만 받습니다): {:?}",
        what,
        prefix_chars(v, 40)
    ));
    fallback.to_string()
}

/// React 칸의 빌드 도구 글 "Vite 8.3" 에서 판(없거나 꼴이 틀리면 latest). 원본: 비공개 `viteVersion`
fn vite_version(s: &StackSlot) -> String {
    let p: Vec<&str> = s.build.split(' ').filter(|piece| !piece.is_empty()).collect();
    if p.len() == 2 && p[0] == "Vite" && version_ok(p[1]) {
        p[1].to_string()
    } else {
        "latest".to_string()
    }
}

/// 스택 → 생성기 계획(설계 2장 "기술 스택"의 뼈대). 프론트와 백엔드를 함께 고르면 frontend/·backend/ 두 폴더, 하나면 뿌리.
/// DB 는 로컬에서 띄우는 docker compose 틀(운영 DB 는 범위 밖). 직접 입력 칸은 생성기 없이 first_unit. 원본: `apSkeletonPlan`
pub fn skeleton_plan(st: &StackSet, name: &str) -> SkeletonPlan {
    let mut p = SkeletonPlan::default();
    let both = st.front.is_some() && st.back.is_some();
    let pkg = package_part(name);
    if let Some(b) = &st.back {
        let folder: &str = if both { "backend" } else { "" };
        match b.key.as_str() {
            "fastapi" => {
                let mut s = GenStep::new("back", "FastAPI 틀(보드에 든 것, 공식 생성기가 없음)", folder, GenKind::Template);
                let py = ver(&b.lang_version, "3.13", "Python", &mut p);
                let fat = ver(&b.framework_version, "0.115", "FastAPI", &mut p);
                s.files = fastapi_template(name, &py, &fat);
                s.tools = vec![GenTool::new("python3", "Python(python3)")];
                p.steps.push(s);
            }
            "spring-java" | "spring-kotlin" => {
                let mut s = GenStep::new("back", "Spring Initializr(start.spring.io)", folder, GenKind::Download);
                let type_ = match b.build.as_str() {
                    "Gradle Kotlin" => "gradle-project-kotlin",
                    "Maven" => "maven-project",
                    _ => "gradle-project",
                };
                let lang = if b.key == "spring-kotlin" { "kotlin" } else { "java" };
                let java = if b.key == "spring-kotlin" { String::new() } else { ver(&b.lang_version, "", "Java", &mut p) };
                let boot = ver(&b.framework_version, "4.1.1", "Spring Boot", &mut p);
                let mut q: Vec<(&str, String)> = vec![
                    ("type", type_.to_string()),
                    ("language", lang.to_string()),
                    ("bootVersion", boot),
                    ("groupId", "com.example".to_string()),
                    ("artifactId", pkg.clone()),
                    ("name", pkg.clone()),
                    ("packageName", format!("com.example.{pkg}")),
                    ("dependencies", "web".to_string()),
                ];
                if !java.is_empty() {
                    q.insert(3, ("javaVersion", java));
                }
                let qs: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", query_value(v))).collect();
                s.url = format!("https://start.spring.io/starter.zip?{}", qs.join("&"));
                s.tools = vec![GenTool::new("unzip", "unzip"), GenTool::new("java", "JDK(java)")];
                p.steps.push(s);
            }
            "nest" => {
                let nest_cli = format!("@nestjs/cli@{}", ver(&b.framework_version, "latest", "NestJS", &mut p));
                let mut s = GenStep::new("back", "Nest CLI(nest new)", folder, GenKind::Command);
                s.argv = vec![
                    "npx".to_string(),
                    "--yes".to_string(),
                    nest_cli,
                    "new".to_string(),
                    if folder.is_empty() { pkg.clone() } else { folder.to_string() },
                    "--package-manager".to_string(),
                    "npm".to_string(),
                    "--skip-git".to_string(),
                ];
                if folder.is_empty() {
                    s.argv[4] = ".".to_string();
                }
                s.tools = vec![GenTool::new("node", "Node.js(node)"), GenTool::new("npx", "npm(npx)")];
                p.steps.push(s);
            }
            _ => {
                p.first_unit = true;
                p.notes.push(format!(
                    "백엔드 {} 는 보드가 생성기를 모르는 스택이라 첫 단위 \"{}\"를 모델이 합니다",
                    b.name, PROJECT_SETUP_TITLE
                ));
            }
        }
    }
    if let Some(f) = &st.front {
        let folder: &str = if both { "frontend" } else { "" };
        let dir = if folder.is_empty() { "." } else { folder };
        let mut s = GenStep::new("front", "", folder, GenKind::Command);
        s.tools = vec![GenTool::new("node", "Node.js(node)"), GenTool::new("npm", "npm")];
        match f.key.as_str() {
            "react" => {
                s.label = "Vite(npm create vite, react-ts)".to_string();
                s.argv = vec![
                    "npm".to_string(),
                    "create".to_string(),
                    "--yes".to_string(),
                    format!("vite@{}", vite_version(f)),
                    dir.to_string(),
                    "--".to_string(),
                    "--template".to_string(),
                    "react-ts".to_string(),
                ];
                p.steps.push(s);
            }
            "next" => {
                s.label = "create-next-app".to_string();
                s.argv = vec![
                    "npx".to_string(),
                    "--yes".to_string(),
                    format!("create-next-app@{}", ver(&f.framework_version, "latest", "Next.js", &mut p)),
                    dir.to_string(),
                    "--ts".to_string(),
                    "--app".to_string(),
                    "--use-npm".to_string(),
                    "--yes".to_string(),
                    "--disable-git".to_string(),
                ];
                s.tools = vec![GenTool::new("node", "Node.js(node)"), GenTool::new("npx", "npm(npx)")];
                p.steps.push(s);
            }
            "vue" => {
                s.label = "create-vue(npm create vue)".to_string();
                // create-vue 는 Vue 판과 따로 나가 latest
                s.argv = vec![
                    "npm".to_string(),
                    "create".to_string(),
                    "--yes".to_string(),
                    "vue@latest".to_string(),
                    dir.to_string(),
                    "--".to_string(),
                    "--ts".to_string(),
                    "--default".to_string(),
                ];
                p.steps.push(s);
            }
            _ => {
                p.first_unit = true;
                p.notes.push(format!(
                    "프론트 {} 는 보드가 생성기를 모르는 스택이라 첫 단위 \"{}\"를 모델이 합니다",
                    f.name, PROJECT_SETUP_TITLE
                ));
            }
        }
    }
    if let Some(d) = &st.db {
        match d.key.as_str() {
            "postgresql" | "mysql" => {
                let mut s = GenStep::new("db", "docker compose 틀(로컬 시험 DB)", "", GenKind::Template);
                let version_source = if d.framework_version.is_empty() { &d.lang_version } else { &d.framework_version };
                let what = d.name.as_str();
                let v = ver(version_source, "", what, &mut p);
                s.files = vec![compose_template(d, &v)];
                p.steps.push(s);
            }
            _ => {
                p.first_unit = true;
                p.notes.push(format!(
                    "DB {} 는 보드가 틀을 모르는 것이라 첫 단위 \"{}\"에서 모델이 구성합니다",
                    d.name, PROJECT_SETUP_TITLE
                ));
            }
        }
    }
    if p.steps.is_empty() && !p.first_unit {
        p.notes.push("고른 스택이 없어 뼈대를 만들지 않습니다".to_string());
    }
    p
}

/// README 머리글의 프로젝트 이름(글자·숫자·`._-` 만, 앞의 60 글자). 원본: 비공개 인라인 `name.filter { $0.isLetter || $0.isNumber || "._-".contains($0) }.prefix(60)`
/// — `isLetter`·`isNumber` 는 유니코드 갈래 L·N 전부라 여기는 `is_alphabetic`·`is_numeric` 으로 뒀다(갈리는 글자는 결합·확장 글자뿐, 시험 값은 ASCII 와 한글 완성형).
fn readme_title(name: &str) -> String {
    name.chars().filter(|c| c.is_alphabetic() || c.is_numeric() || matches!(c, '.' | '_' | '-')).take(60).collect()
}

/// FastAPI 틀 — 공식 생성기가 없어 보드가 든다(설계 2장). 의존성 설치는 하지 않는다(그 위 구성은 모델이 단위로). 원본: `apFastAPITemplate`
pub fn fastapi_template(name: &str, python: &str, fastapi: &str) -> Vec<GenFile> {
    let pkg = package_part(name);
    vec![
        GenFile::new(
            "pyproject.toml",
            &format!(
                "[project]\nname = \"{pkg}\"\nversion = \"0.1.0\"\ndescription = \"Claude Board 자율주행이 만든 FastAPI 뼈대\"\nrequires-python = \">={python}\"\ndependencies = [\"fastapi>={fastapi}\", \"uvicorn>=0.30\"]\n\n"
            ),
        ),
        GenFile::new("app/__init__.py", ""),
        GenFile::new(
            "app/main.py",
            &format!(
                "from fastapi import FastAPI\n\napp = FastAPI(title=\"{pkg}\")\n\n\n@app.get(\"/health\")\ndef health() -> dict:\n    return {{\"status\": \"ok\"}}\n\n"
            ),
        ),
        GenFile::new(".gitignore", "__pycache__/\n*.pyc\n.venv/\n.pytest_cache/\n"),
        GenFile::new(
            "README.md",
            &format!(
                "# {}\n\nClaude Board 자율주행이 주제로 시작에서 만든 FastAPI 뼈대입니다(보드에 든 틀, 바깥 요청 없음).\n띄우기: `python3 -m venv .venv && .venv/bin/pip install -e . && .venv/bin/uvicorn app.main:app`\n\n",
                readme_title(name)
            ),
        ),
    ]
}

/// 로컬 시험 DB 의 docker compose 틀. 운영 DB 연결은 범위 밖. 원본: `apComposeTemplate`
pub fn compose_template(d: &StackSlot, version: &str) -> GenFile {
    let v: &str = if d.key == "mysql" {
        if version.is_empty() { "8.4" } else { version }
    } else if version.is_empty() {
        "18"
    } else {
        version
    };
    let text = if d.key == "mysql" {
        format!(
            "# 로컬 시험 DB(Claude Board 자율주행 뼈대). 운영 DB 연결은 범위 밖입니다\nservices:\n  db:\n    image: mysql:{v}\n    environment:\n      MYSQL_DATABASE: app\n      MYSQL_USER: app\n      MYSQL_PASSWORD: app-local-only\n      MYSQL_ROOT_PASSWORD: app-local-only\n    ports:\n      - \"127.0.0.1:3306:3306\"\n\n"
        )
    } else {
        format!(
            "# 로컬 시험 DB(Claude Board 자율주행 뼈대). 운영 DB 연결은 범위 밖입니다\nservices:\n  db:\n    image: postgres:{v}\n    environment:\n      POSTGRES_DB: app\n      POSTGRES_USER: app\n      POSTGRES_PASSWORD: app-local-only\n    ports:\n      - \"127.0.0.1:5432:5432\"\n\n"
        )
    };
    GenFile::new("compose.yaml", &text)
}

/// 생성기 명령 글(준비 줄·시험) — 사람이 볼 한 줄. 원본: `apGenCommandText`
pub fn gen_command_text(s: &GenStep) -> String {
    match s.kind {
        GenKind::Template => {
            let folder = if s.folder.is_empty() { String::new() } else { format!("{}/", s.folder) };
            let paths: Vec<String> = s.files.iter().map(|f| format!("{folder}{}", f.path)).collect();
            format!("보드에 든 틀: {}", paths.join(", "))
        }
        GenKind::Command => {
            let args: Vec<String> = s.argv.iter().map(|a| if a.contains(' ') { format!("'{a}'") } else { a.clone() }).collect();
            args.join(" ")
        }
        GenKind::Download => format!("GET {} → {}/ 에 풂", s.url, if s.folder.is_empty() { "." } else { &s.folder }),
    }
}

/// 도구 확인(포트 APToolProbe 의 command) — 없으면 준비 줄 실패와 까닭(보드는 도구를 깔지 않는다). 원본: `apSkeletonToolProblems`
pub fn skeleton_tool_problems(p: &SkeletonPlan, probe: &dyn ToolProbe) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in &p.steps {
        let missing: Vec<&GenTool> = s.tools.iter().filter(|t| probe.command(&t.command).is_none()).collect();
        if missing.is_empty() {
            continue;
        }
        let labels: Vec<&str> = missing.iter().map(|t| t.label.as_str()).collect();
        out.push(format!(
            "{}: {} 를 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 깐 뒤 준비를 다시 하거나 다른 스택을 고르세요",
            s.label,
            labels.join("·")
        ));
    }
    out
}

/// 뼈대 만들기의 결과. 원본: `APSkeletonResult`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkeletonResult {
    pub ok: bool,
    /// 한 일(쉬운 말)
    pub done: Vec<String>,
    pub problems: Vec<String>,
    pub commit: Option<String>,
    /// 바깥 요청을 하는 걸음 수(돌린 것)
    pub external: i64,
}

/// 뼈대 커밋 글. 원본: `apSkeletonMessage`
pub fn skeleton_message(p: &SkeletonPlan) -> String {
    let labels: Vec<&str> = p.steps.iter().map(|s| s.label.as_str()).collect();
    format!("뼈대: {} (Claude Board 자율주행이 출발 전에 만듦, 모델 0)", labels.join(" · "))
}

/// 계획을 돌리는 자리(진짜 어댑터는 엔진: 임시 작업 트리 안, 로그인 셸 환경). 경로는 그 작업 트리 기준 상대 경로.
/// 원본: `APSkeletonWork`(Ports.swift) — 이식 자리는 이 파일 모두 글(위)을 보라
pub trait SkeletonWork {
    /// 파일을 쓴다(부모 폴더를 만든다). 되면 true
    fn write(&self, relative: &str, text: &str) -> bool;
    /// 명령을 그 폴더(빈 글자 = 뿌리)에서 돌린다
    fn run(&self, argv: &[String], dir: &str) -> (i32, String);
    /// 주소에서 zip 을 받아 그 폴더에 푼다. 되면 None, 아니면 까닭
    fn download(&self, url: &str, into: &str) -> Option<String>;
    /// 바뀐 것을 모두 더해 커밋한다. 커밋 sha, 바뀐 것이 없으면 None
    fn commit(&self, message: &str) -> (Option<String>, Option<String>);
}

/// 유스케이스: 뼈대 만들기 — 도구 확인 → 걸음마다(틀 쓰기·명령·받아 풀기) → 커밋 하나. 하다 실패하면 거기서 멈춘다(임시 작업 트리는 엔진이 치움).
/// allow_external 이 거짓이면 바깥 요청 걸음을 돌리지 않고 실패로 둔다(시험: 레지스트리를 두드리지 않음). 원본: `apMakeSkeleton`
pub fn make_skeleton(p: &SkeletonPlan, probe: &dyn ToolProbe, work: &dyn SkeletonWork, allow_external: bool) -> SkeletonResult {
    let mut r = SkeletonResult::default();
    if !p.problems.is_empty() {
        r.problems = p.problems.clone();
        return r;
    }
    r.problems = skeleton_tool_problems(p, probe);
    if !r.problems.is_empty() {
        return r;
    }
    for s in &p.steps {
        if s.external() && !allow_external {
            r.problems.push(format!("{}: 바깥 요청을 하는 생성기라 돌리지 않았습니다({})", s.label, gen_command_text(s)));
            return r;
        }
        match s.kind {
            GenKind::Template => {
                let folder = if s.folder.is_empty() { String::new() } else { format!("{}/", s.folder) };
                for f in &s.files {
                    let path = format!("{folder}{}", f.path);
                    if !work.write(&path, &f.text) {
                        r.problems.push(format!("{}: {} 를 쓰지 못했습니다", s.label, path));
                        return r;
                    }
                }
                r.done.push(format!("{}: 파일 {}개", s.label, s.files.len()));
            }
            GenKind::Command => {
                let (code, out) = work.run(&s.argv, "");
                r.external += 1;
                if code != 0 {
                    r.problems.push(format!("{} 이 실패했습니다(종료 코드 {}): {}", s.label, code, suffix_chars(&out, 300)));
                    return r;
                }
                r.done.push(format!("{}: {}", s.label, gen_command_text(s)));
            }
            GenKind::Download => {
                r.external += 1;
                if let Some(why) = work.download(&s.url, &s.folder) {
                    r.problems.push(format!("{}: {}", s.label, why));
                    return r;
                }
                r.done.push(format!("{}: 받아 풂", s.label));
            }
        }
    }
    if p.steps.is_empty() {
        r.ok = true;
        return r;
    }
    let (sha, error) = work.commit(&skeleton_message(p));
    if let Some(e) = error {
        r.problems.push(format!("뼈대를 커밋하지 못했습니다: {}", e));
        return r;
    }
    r.commit = sha;
    r.ok = true;
    r
}

/// 초안 세션이 판정 시나리오를 나눠 커밋할 때의 커밋 글. 원본: `apScenarioCommitMessage`
pub fn scenario_commit_message(units: i64, scenarios: i64) -> String {
    format!("판정 시나리오: 단위 {units}개 · 시나리오 {scenarios}개 (초안 세션이 씀, 출발 뒤에는 고정본으로 판정)")
}

/// 출발 전 자율주행 가지(run.json seed) — 엔진 출발 전 확인·출발·판정 미리 돌리기가 이 기록으로 그 가지를 이어받는다. 원본: `APSeed`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seed {
    pub repo: String,
    /// autopilot/<실행 id>
    pub branch: String,
    /// 출발 가지 이름
    pub base: String,
    /// 가지를 만든 때 출발 가지 머리
    pub base_head: String,
    /// 마지막 커밋(뼈대·시나리오)
    pub head: String,
}

/// JSON 값에서 seed 를 읽는다(head 가 없거나 빈 글자면 None). 원본: `apSeedParse`(입력은 `[String: Any]` — 여기선 JSON 값)
pub fn seed_parse(v: &Value) -> Option<Seed> {
    let o = v.as_object()?;
    let repo = o.get("repo")?.as_str()?;
    let branch = o.get("branch")?.as_str()?;
    let base = o.get("base")?.as_str()?;
    let base_head = o.get("base_head")?.as_str()?;
    let head = o.get("head")?.as_str()?;
    if head.is_empty() {
        return None;
    }
    Some(Seed {
        repo: repo.to_string(),
        branch: branch.to_string(),
        base: base.to_string(),
        base_head: base_head.to_string(),
        head: head.to_string(),
    })
}

/// 출발·출발 전 확인에서 이어받아도 되나 — 가지 이름이 이 실행 것, 그 가지 머리가 기록과 같고, 출발 가지 머리가 그대로이고, 출발 가지 머리가 가지의 조상.
/// 아니면 까닭(출발하지 않음. 보드에서 준비를 다시 하면 새로 만든다). 원본: `apSeedProblem`
pub fn seed_problem(s: &Seed, run_id: &str, branch_head: Option<&str>, base_head_now: Option<&str>, is_ancestor: bool) -> Option<String> {
    if s.branch != format!("autopilot/{run_id}") {
        return Some(format!("출발 전 자율주행 가지 이름({})이 이 실행(autopilot/{})과 다릅니다", s.branch, run_id));
    }
    let Some(bh) = branch_head else {
        return Some(format!("출발 전 자율주행 가지 {} 가 없습니다(준비를 다시 하세요)", s.branch));
    };
    if bh != s.head {
        return Some(format!(
            "출발 전 자율주행 가지 {} 가 준비 뒤에 바뀌었습니다(준비가 커밋한 {}, 지금 {})",
            s.branch,
            prefix_chars(&s.head, 7),
            prefix_chars(bh, 7)
        ));
    }
    let Some(now) = base_head_now else {
        return Some(format!("출발 가지 {} 가 없습니다", s.base));
    };
    if now != s.base_head {
        return Some(format!(
            "출발 가지 {} 가 준비 뒤에 움직였습니다(준비 때 {}, 지금 {}). 준비를 다시 하세요",
            s.base,
            prefix_chars(&s.base_head, 7),
            prefix_chars(now, 7)
        ));
    }
    if !is_ancestor {
        return Some(format!("출발 전 자율주행 가지가 출발 가지 {} 에서 시작하지 않았습니다", s.base));
    }
    None
}
