//! 시험 실행기의 순수 규칙: 단위의 판정 시나리오(실행 파일)와 수용 기준 읽기, 돌리는 법과 필요한 도구, 환경 확인, 판정 차례와 결과, 증거 요약, 미리 돌리기 카드.
//! 원본: `Sources/AutopilotRules/Scenarios.swift`(전체 이식). 프로세스·파일·샌드박스는 엔진(진짜 어댑터)이, 가짜는 시험이 맡는다. 순수 함수만 있다(파일·시각·환경 없음).
//!
//! 선언 대응(원본 순서): `ScenarioKind`(APScenarioKind) · `BrowserDriver`(APBrowserDriver) · `Scenario`(APScenario) · `Criterion`(APCriterion) ·
//! `SCENARIO_MAX`(apScenarioMax) · `scenario_path_ok`(apScenarioPathOK) · `criterion_id_ok`(apCriterionIdOK) · `parse_scenarios`(apParseScenarios) ·
//! `parse_criteria`(apParseCriteria) · `scenario_criterion_problems`(apScenarioCriterionProblems) · `criteria_gaps`(apCriteriaGaps) ·
//! `scenario_json`(apScenarioJSON) · `criterion_json`(apCriterionJSON) · `CRITERION_MODEL_JUDGE`·`CRITERION_DROP` · `criterion_card_id`(apCriterionCardId) ·
//! `CardText`·`CardOption`(APCardText) · `criterion_card`(apCriterionCard) · `criterion_answer`(apCriterionAnswer) · `ToolNeed`(APToolNeed) ·
//! `tool_label`(apToolLabel) · `ScenarioRun`(APScenarioRun) · 비공개 `ext_interpreter`(extInterpreters)·`interpreter_needs` · `scenario_run`(apScenarioRun) ·
//! `ToolProbe`(포트 APToolProbe) · `ToolStatus`(APToolStatus) · `EnvRow`(APEnvRow) · `env_check`(apEnvCheck) · `kind_label`(apKindLabel) ·
//! `env_problem`(apEnvProblem) · `env_row_json`(apEnvRowJSON) · `JudgeStep`(APJudgeStep) · `PinnedScenario`(APPinnedScenario) · `scenario_label`(apScenarioLabel) ·
//! `judge_steps`(apJudgeSteps) · `StepResult`(APStepResult) · `JudgeOutcome`(APJudgeOutcome) · `JudgeRunner`(포트 APJudgeRunner) · `run_judge`(apRunJudge) ·
//! 비공개 `IMAGE_EXT`(imageExt) · `judge_summary`(apJudgeSummary) · `prerun_scenario_card`(apPrerunScenarioCard).
//!
//! 포트 둘의 자리: 원본은 `Ports.swift` 지만 `ports.rs` 는 통합자 몫이라 이 파일에 둔다(`ToolProbe`·`JudgeRunner`). 통합자가 `ports.rs` 로 옮기면
//! 이 파일은 `pub use crate::ports::{JudgeRunner, ToolProbe}` 한 줄로 바뀐다(규칙 쪽 호출은 그대로). 트레이트는 원본처럼 `&self` 다(가짜는 `RefCell` 로 기록한다).
//!
//! 글자 수는 `rules::formats::grapheme_len` 을 불러 센다(복사하지 않았다).
//!
//! Windows 에서 어댑터가 정할 몫(규칙은 원본 1:1 이고 OS 별 따옴표·경로 결정을 안 한다):
//! - `judge_steps` 의 판정 명령 argv `[셸, "-l", "-c", 명령]` 은 원본 꼴 그대로다. 실제로 띄우는 꼴(PowerShell 7·Windows PowerShell·cmd·Git Bash·WSL 별 `Invocation`,
//!   명령 글자를 `CLAUDE_BOARD_CHECK` 로 넘기기)은 `env_policy::check_invocation` 이 소유한다. 여기서 중복하지 않는다.
//! - 시나리오 걸음의 argv(`interpreter + [고정본 경로]`)는 `/bin/sh`·`/usr/bin/env` 같은 POSIX 해석기 글자다. PowerShell·cmd 환경에서 이 해석기를 어떻게 바꿔 부를지
//!   (또는 거절할지), 고정본 경로를 환경 쪽 경로로 바꾸는 일(`ExecEnv`)과 따옴표는 어댑터가 정한다. `scenario_run` 은 POSIX 해석기 표만 안다.
//! - 걸음 출력(`StepResult.tail`)의 줄 끝: `JudgeOutcome::tail` 의 「끝 20줄」은 원본처럼 `\n` 으로만 줄을 나눈다(아래 Foundation 절의 `JudgeOutcome::tail` 줄).
//!   Windows 셸의 CRLF 출력을 줄로 세고 싶으면 어댑터가 tail 을 LF 로 바꿔 넘긴다.
//! - `judge_summary` 의 숫자: `sec`·`at` 은 `f64` 로 밀리초까지 반올림한다. 값이 0 이면 JSON 에서 `0.0` 이다(Swift 의 `JSONSerialization` 은 `0`). 읽는 쪽이 숫자로 읽으면 같다.
//!
//! Foundation 과 다른 점(원본 Swift 는 `Character`=자소 묶음 단위, 여기는 바이트·유니코드 글자 단위. 모두 시험 「추가」 절이 고정한다):
//! - `criterion_id_ok`: 원본은 ICU 정규식이라 `$` 가 끝의 줄바꿈 앞에서도 맞아 `"AC-1\n"` 이 통과한다. 여기는 줄바꿈이 붙은 id 를 거절한다
//!   (id 는 카드 id·파일 글에 들어간다 — 더 엄격한 쪽).
//! - `scenario_path_ok`: `starts_with("/")`·`"~"` 와 `/` 로 나누기가 바이트 기준이라, 원본이 `/`+결합 글자(`/\u{301}`)를 별개 글자로 보아 받는 경로를
//!   여기서는 거절한다(더 엄격한 쪽). 받는 것이 늘어나는 방향의 차이는 없다.
//! - `scenario_run` 의 첫 줄: 원본은 `"\n"` 으로 나누는데 `"\r\n"` 이 글자 하나라 CRLF 머리에서는 나눠지지 않고 `\r\n` 이 해석기 글자에 섞인다
//!   (그 파일은 도구 확인에서 막힌다). 여기도 같게 `\n` 이 `\r` 바로 뒤가 아닐 때만 줄 끝으로 보고 `\r`(바로 뒤가 `\n` 아님)·빈칸·탭으로 가른다.
//! - `#!` 접두와 `-` 접두 판정은 바이트 기준(바로 뒤에 결합 글자가 붙은 꼴만 갈린다). 오류 글의 `prefix(60)` 은 `grapheme_len` 으로 자소 묶음 60개에서 자른다.
//! - `trimmingCharacters(in: .whitespacesAndNewlines)` 는 `str::trim`(유니코드 White_Space, 같은 글자 집합)으로 옮겼다.
//! - `JudgeOutcome::tail` 의 끝 20줄 나누기: 원본 `split(separator: "\n")` 은 `"\r\n"` 이 글자 하나라 CRLF 를 나누지 못한다. 여기도 `\r` 바로 뒤가 아닌 `\n` 에서만 자른다(시험 `추가_꼬리_crlf`).
//! - 원본의 `Any?`(JSONSerialization 결과)는 `Option<&serde_json::Value>` 다. `Some(Null)`(NSNull)은 배열도 글자도 아니라 원본처럼 고칠 것을 낸다.

use super::formats::grapheme_len;
use serde_json::{json, Map, Value};

/// 시나리오 종류. 원본: `APScenarioKind`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScenarioKind {
    /// 명령·HTTP 요청을 단언하는 실행 파일(셸·파이썬·노드 등, 해석기는 첫 줄 #! 이나 확장자)
    Shell,
    /// 브라우저 시나리오: 해석기 + 브라우저 드라이버
    Browser,
}

impl ScenarioKind {
    /// 원본 `rawValue`
    pub fn as_str(self) -> &'static str {
        match self {
            ScenarioKind::Shell => "shell",
            ScenarioKind::Browser => "browser",
        }
    }

    /// 원본 `APScenarioKind(rawValue:)`
    pub fn from_raw(s: &str) -> Option<Self> {
        match s {
            "shell" => Some(ScenarioKind::Shell),
            "browser" => Some(ScenarioKind::Browser),
            _ => None,
        }
    }
}

/// 브라우저 드라이버. 원본: `APBrowserDriver`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserDriver {
    /// 깔려 있는 Chrome 계열 브라우저를 headless 로(엔진이 `$CLAUDE_BOARD_BROWSER`·`$CLAUDE_BOARD_BROWSER_FLAGS` 로 넘김)
    Chrome,
    /// node 모듈 playwright(보드는 깔지 않는다)
    Playwright,
}

impl BrowserDriver {
    /// 원본 `rawValue`
    pub fn as_str(self) -> &'static str {
        match self {
            BrowserDriver::Chrome => "chrome",
            BrowserDriver::Playwright => "playwright",
        }
    }

    /// 원본 `APBrowserDriver(rawValue:)`
    pub fn from_raw(s: &str) -> Option<Self> {
        match s {
            "chrome" => Some(BrowserDriver::Chrome),
            "playwright" => Some(BrowserDriver::Playwright),
            _ => None,
        }
    }
}

/// 판정 시나리오 하나. 원본: `APScenario`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    /// 작업 폴더(cwd) 기준 상대 경로
    pub file: String,
    pub kind: ScenarioKind,
    /// 짝지은 수용 기준 id
    pub criterion: Option<String>,
    /// 브라우저만(기본 chrome)
    pub driver: Option<BrowserDriver>,
}

impl Scenario {
    /// 원본 `init`: 브라우저면 드라이버가 없을 때 chrome, 셸이면 드라이버는 늘 없음
    pub fn new(file: &str, kind: ScenarioKind, criterion: Option<&str>, driver: Option<BrowserDriver>) -> Self {
        let driver = if kind == ScenarioKind::Browser { Some(driver.unwrap_or(BrowserDriver::Chrome)) } else { None };
        Scenario { file: file.to_string(), kind, criterion: criterion.map(str::to_string), driver }
    }
}

/// 수용 기준 하나. 원본: `APCriterion`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    pub id: String,
    pub text: String,
}

impl Criterion {
    pub fn new(id: &str, text: &str) -> Self {
        Criterion { id: id.to_string(), text: text.to_string() }
    }
}

/// 단위 하나의 시나리오 수 상한. 원본: `apScenarioMax`
pub const SCENARIO_MAX: usize = 30;

/// 작업 폴더 기준 상대 경로로 쓸 수 있는가(절대 경로·~·.. ·빈 조각·조절 글자·따옴표·역슬래시는 받지 않는다 — 고정본 자리와 샌드박스 글에 들어가므로). 원본: `apScenarioPathOK`
pub fn scenario_path_ok(p: &str) -> bool {
    if p.is_empty() || grapheme_len(p) > 300 || p.starts_with('/') || p.starts_with('~') {
        return false;
    }
    if p.chars().any(|c| (c as u32) < 0x20 || c == '"' || c == '\\' || c as u32 == 0x7f) {
        return false;
    }
    !p.split('/').any(|part| part.is_empty() || part == ".." || part == ".")
}

/// 수용 기준 id 꼴: 영문·숫자·`.-_` 1~40자(카드 id 와 파일 글에 들어간다). 원본: `apCriterionIdOK`
pub fn criterion_id_ok(s: &str) -> bool {
    !s.is_empty() && grapheme_len(s) <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn problem_scenario(n: i64, k: usize, rest: &str) -> String {
    format!("단위 {n} 의 시나리오 {k} 의 {rest}")
}

/// 단위의 `scenarios` 칸 읽기. 고칠 것은 "단위 n 의 …" 문장으로(엔진 출발 전 확인의 고칠 것). 원본: `apParseScenarios`
pub fn parse_scenarios(v: Option<&Value>, unit: i64) -> (Vec<Scenario>, Vec<String>) {
    let n = unit;
    let Some(v) = v else { return (vec![], vec![]) };
    let Some(arr) = v.as_array() else { return (vec![], vec![format!("단위 {n} 의 scenarios 는 배열이어야 합니다")]) };
    let mut items: Vec<Scenario> = vec![];
    let mut problems: Vec<String> = vec![];
    if arr.len() > SCENARIO_MAX {
        problems.push(format!("단위 {n} 의 시나리오가 {}개입니다({SCENARIO_MAX}개까지)", arr.len()));
    }
    for (i, x) in arr.iter().take(SCENARIO_MAX).enumerate() {
        let k = i + 1;
        // 글자 하나면 셸 시나리오 파일 경로로 본다
        let o: Map<String, Value> = match x {
            Value::String(s) => {
                let mut m = Map::new();
                m.insert("file".to_string(), Value::String(s.clone()));
                m
            }
            Value::Object(m) => m.clone(),
            _ => Map::new(),
        };
        let file = match o.get("file").and_then(Value::as_str) {
            Some(f) if scenario_path_ok(f) => f,
            _ => {
                problems.push(problem_scenario(n, k, "file 은 작업 폴더 기준 상대 경로(.. 없이)여야 합니다"));
                continue;
            }
        };
        let mut kind = ScenarioKind::Shell;
        if let Some(ks) = o.get("kind") {
            match ks.as_str().and_then(ScenarioKind::from_raw) {
                Some(kk) => kind = kk,
                None => {
                    problems.push(problem_scenario(n, k, "kind 는 shell 또는 browser 입니다"));
                    continue;
                }
            }
        }
        let mut driver: Option<BrowserDriver> = None;
        if let Some(ds) = o.get("driver") {
            match ds.as_str().and_then(BrowserDriver::from_raw) {
                Some(dd) if kind == ScenarioKind::Browser => driver = Some(dd),
                _ => {
                    problems.push(problem_scenario(n, k, "driver 는 브라우저 시나리오에만, chrome 또는 playwright 입니다"));
                    continue;
                }
            }
        }
        let mut crit: Option<&str> = None;
        if let Some(c) = o.get("criterion") {
            match c.as_str() {
                Some(s) if criterion_id_ok(s) => crit = Some(s),
                _ => {
                    problems.push(problem_scenario(n, k, "criterion 은 수용 기준 id(영문·숫자·.-_)입니다"));
                    continue;
                }
            }
        }
        if items.iter().any(|s| s.file == file) {
            problems.push(format!("단위 {n} 의 시나리오 {file} 가 두 번 있습니다"));
            continue;
        }
        items.push(Scenario::new(file, kind, crit, driver));
    }
    (items, problems)
}

/// 단위의 `criteria` 칸 읽기(수용 기준: `[{id, text}]`). 원본: `apParseCriteria`
pub fn parse_criteria(v: Option<&Value>, unit: i64) -> (Vec<Criterion>, Vec<String>) {
    let n = unit;
    let Some(v) = v else { return (vec![], vec![]) };
    let Some(arr) = v.as_array() else { return (vec![], vec![format!("단위 {n} 의 criteria 는 배열이어야 합니다")]) };
    let mut items: Vec<Criterion> = vec![];
    let mut problems: Vec<String> = vec![];
    for (i, x) in arr.iter().enumerate() {
        let id = match x.as_object().and_then(|o| o.get("id")).and_then(Value::as_str) {
            Some(id) if criterion_id_ok(id) => id,
            _ => {
                problems.push(format!("단위 {n} 의 수용 기준 {} 에 id(영문·숫자·.-_)가 없습니다", i + 1));
                continue;
            }
        };
        let text = x.get("text").and_then(Value::as_str).unwrap_or("").trim();
        if text.is_empty() {
            problems.push(format!("단위 {n} 의 수용 기준 {id} 에 text 가 없습니다"));
            continue;
        }
        if items.iter().any(|c| c.id == id) {
            problems.push(format!("단위 {n} 의 수용 기준 {id} 가 두 번 있습니다"));
            continue;
        }
        items.push(Criterion::new(id, text));
    }
    (items, problems)
}

/// 시나리오가 가리킨 수용 기준이 목록에 있는가(수용 기준을 적은 단위만 본다). 원본: `apScenarioCriterionProblems`
pub fn scenario_criterion_problems(scenarios: &[Scenario], criteria: &[Criterion], unit: i64) -> Vec<String> {
    if criteria.is_empty() {
        return vec![];
    }
    scenarios
        .iter()
        .filter_map(|s| {
            let c = s.criterion.as_deref()?;
            if criteria.iter().any(|x| x.id == c) {
                None
            } else {
                Some(format!("단위 {unit} 의 시나리오 {} 가 가리킨 수용 기준 {c} 가 criteria 에 없습니다", s.file))
            }
        })
        .collect()
}

/// 실행 파일(시나리오)이 짝지어지지 않은 수용 기준 — 출발 카드로 묻는다(동결 뒤로는 묻지 않는다). 원본: `apCriteriaGaps`
pub fn criteria_gaps(criteria: &[Criterion], scenarios: &[Scenario]) -> Vec<Criterion> {
    criteria.iter().filter(|c| !scenarios.iter().any(|s| s.criterion.as_deref() == Some(c.id.as_str()))).cloned().collect()
}

/// 원본: `apScenarioJSON`(`file`·`kind`, 있으면 `criterion`·`driver`)
pub fn scenario_json(s: &Scenario) -> Value {
    let mut o = Map::new();
    o.insert("file".into(), json!(s.file));
    o.insert("kind".into(), json!(s.kind.as_str()));
    if let Some(c) = &s.criterion {
        o.insert("criterion".into(), json!(c));
    }
    if let Some(d) = s.driver {
        o.insert("driver".into(), json!(d.as_str()));
    }
    Value::Object(o)
}

/// 원본: `apCriterionJSON`
pub fn criterion_json(c: &Criterion) -> Value {
    json!({"id": c.id, "text": c.text})
}

/// 모델 판정으로 두기: 단위에 넣고, 그 기준은 결과 기록의 근거로 본다. 원본: `apCriterionModelJudge`
pub const CRITERION_MODEL_JUDGE: &str = "model_judge";
/// 범위에서 빼기(추천): 이 기준만 이번 범위에서 뺀다(단위는 남는다). 원본: `apCriterionDrop`
pub const CRITERION_DROP: &str = "drop_criterion";

/// 원본: `apCriterionCardId`
pub fn criterion_card_id(unit: &str, criterion: &str) -> String {
    format!("criterion-{unit}-{criterion}")
}

/// 카드의 고르기 하나. 원본: `APCardText.options` 의 `(key, label, note)`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardOption {
    pub key: String,
    pub label: String,
    pub note: String,
}

impl CardOption {
    fn new(key: &str, label: &str, note: &str) -> Self {
        CardOption { key: key.to_string(), label: label.to_string(), note: note.to_string() }
    }
}

/// 카드의 글. 원본: `APCardText`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardText {
    pub label: String,
    pub title: String,
    pub why: String,
    pub options: Vec<CardOption>,
    pub recommended: String,
}

/// 실행 파일이 없는 수용 기준 카드의 글. 추천은 범위에서 빼기(실행으로 확인하지 못한 통과를 남기지 않게. 설계 1장 "기준을 낮춰야만 통과하면 미통과"와 같은 뿌리).
/// 원본: `apCriterionCard`
pub fn criterion_card(unit_title: &str, c: &Criterion) -> CardText {
    CardText {
        label: "실행 파일이 없는 수용 기준".to_string(),
        title: format!("「{unit_title}」 의 수용 기준 {} 를 볼 판정 시나리오가 없습니다", c.id),
        why: format!("{} (이 기준은 실행 파일로 판정하지 않아 통과·실패를 기계로 가리지 못합니다.)", c.text),
        options: vec![
            CardOption::new(CRITERION_DROP, "범위에서 빼기", "이 기준만 이번 실행에서 하지 않습니다. 단위는 남고 다음 출발에서 다시 보입니다"),
            CardOption::new(CRITERION_MODEL_JUDGE, "모델 판정으로 두기", "단위에 넣고 그 기준은 결과 기록의 근거로 봅니다. 사람이 합치기·승격 때 확인합니다"),
        ],
        recommended: CRITERION_DROP.to_string(),
    }
}

/// 출발 때 고정할 수용 기준의 답: 시나리오가 있으면 "scenario", 없으면 카드 답(없거나 꼴이 틀리면 추천 = 범위에서 빼기, defaulted 참).
/// 원본: `apCriterionAnswer`
pub fn criterion_answer(c: &Criterion, scenarios: &[Scenario], answer: Option<&str>) -> (String, bool) {
    if scenarios.iter().any(|s| s.criterion.as_deref() == Some(c.id.as_str())) {
        return ("scenario".to_string(), false);
    }
    if let Some(a) = answer {
        if a == CRITERION_MODEL_JUDGE || a == CRITERION_DROP {
            return (a.to_string(), false);
        }
    }
    (CRITERION_DROP.to_string(), true)
}

/// 시나리오를 돌리는 데 필요한 도구. 원본: `APToolNeed`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolNeed {
    /// 절대 경로의 실행 파일(첫 줄 #! 의 해석기 등)
    Program(String),
    /// 로그인 셸 PATH 에서 찾는 명령
    Command(String),
    /// Chrome 계열 브라우저(드라이버 chrome)
    Browser,
    /// 작업 폴더에서 node 가 찾는 모듈(드라이버 playwright)
    NodeModule(String),
}

/// 원본: `apToolLabel`
pub fn tool_label(n: &ToolNeed) -> String {
    match n {
        ToolNeed::Program(p) => p.clone(),
        ToolNeed::Command(c) => format!("{c}(PATH)"),
        ToolNeed::Browser => "브라우저(Google Chrome·Chromium·Microsoft Edge)".to_string(),
        ToolNeed::NodeModule(m) => format!("node 모듈 {m}"),
    }
}

/// 시나리오를 돌리는 법. 원본: `APScenarioRun`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioRun {
    /// 파일 경로 앞에 붙는 해석기와 인자
    pub interpreter: Vec<String>,
    pub needs: Vec<ToolNeed>,
    /// 돌리는 법을 모름
    pub problem: Option<String>,
}

// 원본 `extInterpreters`
fn ext_interpreter(ext: &str) -> Option<&'static [&'static str]> {
    Some(match ext {
        "sh" => &["/bin/sh"],
        "bash" => &["/bin/bash"],
        "zsh" => &["/bin/zsh"],
        "py" => &["/usr/bin/env", "python3"],
        "js" | "mjs" | "cjs" | "ts" | "mts" => &["/usr/bin/env", "node"],
        _ => return None,
    })
}

// `NSString.lastPathComponent`: 끝의 `/` 를 떼고 마지막 `/` 뒤
fn last_path_component(p: &str) -> &str {
    let t = p.trim_end_matches('/');
    t.rsplit('/').next().unwrap_or("")
}

// `NSString.pathExtension`: 마지막 조각에서 마지막 `.` 뒤(`.`이 맨 앞이면 확장자 없음)
fn path_extension(p: &str) -> &str {
    let comp = last_path_component(p);
    match comp.rfind('.') {
        Some(i) if i > 0 => &comp[i + 1..],
        _ => "",
    }
}

// 해석기 argv 에서 필요한 도구: 첫 조각은 절대 경로 실행 파일, env 면 다음 이름(-S·-i 같은 선택 칸은 건너뜀)을 PATH 에서
fn interpreter_needs(argv: &[String]) -> Vec<ToolNeed> {
    let Some(first) = argv.first() else { return vec![] };
    let mut needs = vec![ToolNeed::Program(first.clone())];
    if last_path_component(first) == "env" {
        if let Some(name) = argv.iter().skip(1).find(|a| !a.starts_with('-')) {
            needs.push(ToolNeed::Command(name.clone()));
        }
    }
    needs
}

// 원본 `head.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first`.
// Swift 의 `"\n"` 은 글자 하나라 `"\r\n"` 은 글자 하나로 묶여 나눠지지 않는다 → `\r` 바로 뒤가 아닌 첫 `\n` 앞까지
fn first_line(head: &str) -> &str {
    let b = head.as_bytes();
    let cut = (0..b.len()).find(|&i| b[i] == b'\n' && (i == 0 || b[i - 1] != b'\r')).unwrap_or(b.len());
    &head[..cut]
}

// 원본 `split(whereSeparator: { " " · "\t" · "\r" })`(빈 조각은 버림). `"\r\n"` 은 글자 하나라 구분자가 아니다
fn split_argv(s: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        let sep = c == ' ' || c == '\t' || (c == '\r' && it.peek() != Some(&'\n'));
        if sep {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// 원본 `String.prefix(60)`: 자소 묶음 60개까지(`grapheme_len` 으로 센다)
fn prefix_clusters(s: &str, n: usize) -> &str {
    if grapheme_len(s) <= n {
        return s;
    }
    let mut keep = 0;
    for (i, _) in s.char_indices().skip(1) {
        if grapheme_len(&s[..i]) > n {
            break;
        }
        keep = i;
    }
    &s[..keep]
}

/// 시나리오를 어떻게 돌릴까: 첫 줄이 #! 면 그 해석기(절대 경로여야 함), 아니면 확장자. 브라우저는 드라이버 도구를 더한다. 원본: `apScenarioRun`
pub fn scenario_run(s: &Scenario, head: &str) -> ScenarioRun {
    let mut argv: Vec<String> = vec![];
    let mut problem: Option<String> = None;
    let first = first_line(head);
    if let Some(rest) = first.strip_prefix("#!") {
        argv = split_argv(rest);
        if argv.first().is_none_or(|a| !a.starts_with('/')) {
            problem = Some(format!("첫 줄 #! 의 해석기가 절대 경로가 아닙니다({})", prefix_clusters(first, 60)));
            argv = vec![];
        }
    } else {
        let ext = path_extension(&s.file).to_lowercase();
        match ext_interpreter(&ext) {
            Some(a) => argv = a.iter().map(|x| x.to_string()).collect(),
            None => {
                problem = Some("돌리는 법을 모릅니다. 첫 줄에 #! 해석기를 쓰거나 확장자를 .sh·.bash·.zsh·.py·.js·.mjs·.cjs·.ts 로 씁니다".to_string());
            }
        }
    }
    let mut needs = interpreter_needs(&argv);
    if s.kind == ScenarioKind::Browser {
        match s.driver.unwrap_or(BrowserDriver::Chrome) {
            BrowserDriver::Chrome => needs.push(ToolNeed::Browser),
            BrowserDriver::Playwright => {
                let node = ToolNeed::Command("node".to_string());
                if !needs.contains(&node) {
                    needs.push(node);
                }
                needs.push(ToolNeed::NodeModule("playwright".to_string()));
            }
        }
    }
    ScenarioRun { interpreter: argv, needs, problem }
}

// ---------------------------------------------------------------------------------------------------------------------
// 환경 확인
// ---------------------------------------------------------------------------------------------------------------------

/// 도구 찾기 포트. 원본: `APToolProbe`(Ports.swift). 진짜 어댑터는 엔진, 가짜는 시험에만 둔다
pub trait ToolProbe {
    /// 절대 경로의 실행 파일이 있는가
    fn program(&self, path: &str) -> bool;
    /// 로그인 셸 PATH 에서 찾은 자리(없으면 `None`)
    fn command(&self, name: &str) -> Option<String>;
    /// Chrome 계열 브라우저를 찾은 자리(없으면 `None`)
    fn browser(&self) -> Option<String>;
    /// 작업 폴더 `dir` 에서 node 가 모듈을 찾는가
    fn node_module(&self, name: &str, dir: &str) -> bool;
}

/// 도구 하나의 확인 결과. 원본: `APToolStatus`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    pub need: ToolNeed,
    /// 찾은 자리(모듈은 "있음"), 없으면 `None`
    pub found: Option<String>,
}

impl ToolStatus {
    /// 원본 `ok`
    pub fn ok(&self) -> bool {
        self.found.is_some()
    }
}

/// 시나리오 하나의 환경 확인 줄. 원본: `APEnvRow`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRow {
    pub unit: i64,
    pub file: String,
    pub kind: ScenarioKind,
    pub driver: Option<BrowserDriver>,
    pub tools: Vec<ToolStatus>,
    /// 돌리는 법을 모름
    pub problem: Option<String>,
}

impl EnvRow {
    /// 원본 `ok`
    pub fn ok(&self) -> bool {
        self.problem.is_none() && self.tools.iter().all(ToolStatus::ok)
    }
}

/// 원본: `apEnvCheck`
pub fn env_check(unit: i64, s: &Scenario, run: &ScenarioRun, cwd: &str, probe: &dyn ToolProbe) -> EnvRow {
    let tools = run
        .needs
        .iter()
        .map(|need| {
            let found = match need {
                ToolNeed::Program(p) => probe.program(p).then(|| p.clone()),
                ToolNeed::Command(c) => probe.command(c),
                ToolNeed::Browser => probe.browser(),
                ToolNeed::NodeModule(m) => probe.node_module(m, cwd).then(|| "있음".to_string()),
            };
            ToolStatus { need: need.clone(), found }
        })
        .collect();
    EnvRow { unit, file: s.file.clone(), kind: s.kind, driver: s.driver, tools, problem: run.problem.clone() }
}

/// 원본: `apKindLabel`
pub fn kind_label(k: ScenarioKind) -> &'static str {
    if k == ScenarioKind::Shell {
        "셸"
    } else {
        "브라우저"
    }
}

/// 환경 확인이 실패한 줄의 고칠 것 글(엔진 출발 전 확인의 고칠 것 = 출발 잠금, 준비 화면 실패 줄의 까닭). 통과면 `None`. 원본: `apEnvProblem`
pub fn env_problem(r: &EnvRow) -> Option<String> {
    if r.ok() {
        return None;
    }
    let head = format!("단위 {} 의 시나리오 {}({})", r.unit, r.file, kind_label(r.kind));
    if let Some(p) = &r.problem {
        return Some(format!("{head}를 돌리지 못합니다: {p}"));
    }
    let missing: Vec<String> = r.tools.iter().filter(|t| !t.ok()).map(|t| tool_label(&t.need)).collect();
    Some(format!(
        "{head}를 돌릴 도구가 없습니다: {} 을(를) 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 도구를 깐 뒤 다시 확인하거나 시나리오를 바꿉니다",
        missing.join(", ")
    ))
}

/// 원본: `apEnvRowJSON`(`unit`·`file`·`kind`·`ok`·`tools`, 있으면 `driver`·`why`)
pub fn env_row_json(r: &EnvRow) -> Value {
    let tools: Vec<Value> = r.tools.iter().map(|t| json!({"need": tool_label(&t.need), "found": t.found})).collect();
    let mut o = Map::new();
    o.insert("unit".into(), json!(r.unit));
    o.insert("file".into(), json!(r.file));
    o.insert("kind".into(), json!(r.kind.as_str()));
    o.insert("ok".into(), json!(r.ok()));
    o.insert("tools".into(), Value::Array(tools));
    if let Some(d) = r.driver {
        o.insert("driver".into(), json!(d.as_str()));
    }
    if let Some(p) = env_problem(r) {
        o.insert("why".into(), json!(p));
    }
    Value::Object(o)
}

// ---------------------------------------------------------------------------------------------------------------------
// 판정 차례와 결과
// ---------------------------------------------------------------------------------------------------------------------

/// 판정 차례의 걸음 하나. 원본: `APJudgeStep`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeStep {
    /// "scenario-1" … · "check"
    pub name: String,
    /// 사람이 읽는 이름
    pub label: String,
    /// 돌릴 것(판정 명령이면 `[셸, "-l", "-c", 명령]`)
    pub argv: Vec<String>,
    /// 증거 폴더 안의 자리(`""` 는 맨 위 — 판정 명령, 옛 자리 그대로)
    pub evidence: String,
    /// "check" · "shell" · "browser"
    pub kind: String,
    /// 시나리오 상대 경로
    pub file: Option<String>,
    pub criterion: Option<String>,
    /// 고정본 경로
    pub pinned: Option<String>,
    /// 고정본 지문(출발 때). 있으면 돌리기 전에 견준다
    pub sha256: Option<String>,
}

impl JudgeStep {
    /// 원본 `init`: 시나리오 칸(`file`·`criterion`·`pinned`·`sha256`)은 `None` 으로 시작한다
    pub fn new(name: &str, label: &str, argv: Vec<String>, evidence: &str, kind: &str) -> Self {
        JudgeStep {
            name: name.to_string(),
            label: label.to_string(),
            argv,
            evidence: evidence.to_string(),
            kind: kind.to_string(),
            file: None,
            criterion: None,
            pinned: None,
            sha256: None,
        }
    }
}

/// 고정본 하나: 시나리오·돌리는 법·고정본 경로·지문. 원본: `APPinnedScenario`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedScenario {
    pub scenario: Scenario,
    pub run: ScenarioRun,
    pub pinned: String,
    pub sha256: Option<String>,
}

/// 원본: `apScenarioLabel`
pub fn scenario_label(s: &Scenario) -> String {
    let crit = s.criterion.as_ref().map(|c| format!(" · 수용 기준 {c}")).unwrap_or_default();
    format!("시나리오 {}({}{crit})", s.file, kind_label(s.kind))
}

/// 판정 차례: 시나리오들(고정본, 적은 차례) → 판정 명령(있으면). 판정 명령이 없고 시나리오도 없으면 빈 차례(엔진 출발 전 확인이 막는다). 원본: `apJudgeSteps`
pub fn judge_steps(check: Option<&str>, shell: &str, scenarios: &[PinnedScenario]) -> Vec<JudgeStep> {
    let mut out: Vec<JudgeStep> = vec![];
    for (i, p) in scenarios.iter().enumerate() {
        let name = format!("scenario-{}", i + 1);
        let mut argv = p.run.interpreter.clone();
        argv.push(p.pinned.clone());
        let mut step = JudgeStep::new(&name, &scenario_label(&p.scenario), argv, &name, p.scenario.kind.as_str());
        step.file = Some(p.scenario.file.clone());
        step.criterion = p.scenario.criterion.clone();
        step.pinned = Some(p.pinned.clone());
        step.sha256 = p.sha256.clone();
        out.push(step);
    }
    if let Some(c) = check.filter(|c| !c.is_empty()) {
        out.push(JudgeStep::new("check", "판정 명령", vec![shell.to_string(), "-l".to_string(), "-c".to_string(), c.to_string()], "", "check"));
    }
    out
}

/// 걸음 하나의 결과. 원본: `APStepResult`
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub code: i32,
    pub sec: f64,
    pub timed_out: bool,
    pub interrupted: bool,
    pub tail: String,
    pub files: Vec<String>,
    /// 돌리지 않은 까닭(보안 등)
    pub note: Option<String>,
}

impl StepResult {
    /// 원본 `init(code:)`: 나머지는 기본값(0초·안 끝남·빈 출력·파일 없음·까닭 없음)
    pub fn new(code: i32) -> Self {
        StepResult { code, sec: 0.0, timed_out: false, interrupted: false, tail: String::new(), files: vec![], note: None }
    }
}

/// 판정 한 걸음을 돌리는 포트(진짜 어댑터는 엔진: 샌드박스·로그인 환경·시간 상한·일시정지/중단, 출력은 증거 폴더). 원본: `APJudgeRunner`(Ports.swift)
pub trait JudgeRunner {
    /// 고정본의 지금 지문(sha256 16진수). 없으면 `None`
    fn digest(&self, path: &str) -> Option<String>;
    fn run(&self, step: &JudgeStep) -> StepResult;
}

/// 판정 전체의 결과. 원본: `APJudgeOutcome`
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeOutcome {
    pub steps: Vec<JudgeStep>,
    pub results: Vec<StepResult>,
}

// 원본 `split(separator: "\n", omittingEmptySubsequences: false)`: Swift 의 `"\r\n"` 은 글자 하나라 나눠지지 않는다 → `\r` 바로 뒤가 아닌 `\n` 에서만 자른다
fn split_lines_like_swift(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out: Vec<&str> = vec![];
    let mut start = 0;
    for i in 0..b.len() {
        if b[i] == b'\n' && (i == 0 || b[i - 1] != b'\r') {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}

impl JudgeOutcome {
    /// 원본 `passed`: 끊기지 않았고, 걸음마다 결과가 있고, 모두 종료 코드 0 에 시간 안
    pub fn passed(&self) -> bool {
        !self.interrupted()
            && !self.results.is_empty()
            && self.results.len() == self.steps.len()
            && self.results.iter().all(|r| r.code == 0 && !r.timed_out)
    }

    /// 원본 `interrupted`
    pub fn interrupted(&self) -> bool {
        self.results.iter().any(|r| r.interrupted)
    }

    /// 원본 `timedOut`
    pub fn timed_out(&self) -> bool {
        self.results.iter().any(|r| r.timed_out)
    }

    /// 실패로 볼 종료 코드: 처음 실패한 걸음의 것(통과면 0). 원본 `code`
    pub fn code(&self) -> i32 {
        if self.passed() {
            0
        } else {
            self.results.iter().find(|r| r.code != 0 || r.timed_out).map_or(-1, |r| r.code)
        }
    }

    /// 모델에게 넘기는 출력 끝: 걸음 하나(옛 판정 명령만)면 그 출력 그대로, 여럿이면 실패한 걸음마다 머리 줄과 끝 20줄. 원본 `tail`
    pub fn tail(&self) -> String {
        if self.steps.len() == 1 {
            if let Some(r) = self.results.first() {
                return r.tail.clone();
            }
        }
        self.steps
            .iter()
            .zip(self.results.iter())
            .filter(|(_, r)| r.code != 0 || r.timed_out)
            .map(|(s, r)| {
                let why = match &r.note {
                    Some(n) => n.clone(),
                    None if r.timed_out => "시간 상한에 닿음".to_string(),
                    None => format!("종료 코드 {}", r.code),
                };
                let head = format!("[{}] {why}", s.label);
                let lines = split_lines_like_swift(&r.tail);
                let t = lines[lines.len().saturating_sub(20)..].join("\n");
                if t.is_empty() {
                    head
                } else {
                    format!("{head}\n{t}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// 모델·진행 기록에 쓰는 판정 이름: 걸음 하나면 그 명령, 여럿이면 "시나리오 n개 + 판정 명령". 원본 `describe(check:)`
    pub fn describe(&self, check: Option<&str>) -> String {
        let n = self.steps.iter().filter(|s| s.kind != "check").count();
        if n == 0 {
            return check.unwrap_or("").to_string();
        }
        match check.filter(|c| !c.is_empty()) {
            Some(c) => format!("시나리오 {n}개 + 판정 명령 `{c}`"),
            None => format!("시나리오 {n}개"),
        }
    }
}

/// 판정을 돌린다: 차례대로, 일시정지·중단·상한(interrupted)이면 거기서 멈춘다. 실패해도 나머지를 돌려 무엇이 실패했는지 모두 남긴다.
/// 고정본 지문이 출발 때와 다르면 그 걸음은 돌리지 않고 실패(보안). 원본: `apRunJudge`
pub fn run_judge(steps: &[JudgeStep], runner: &dyn JudgeRunner) -> JudgeOutcome {
    let mut results: Vec<StepResult> = vec![];
    for s in steps {
        if let (Some(want), Some(p)) = (&s.sha256, &s.pinned) {
            if runner.digest(p).as_deref() != Some(want.as_str()) {
                let mut r = StepResult::new(-1);
                r.tail = format!("보안: 판정 시나리오 고정본이 출발 때와 달라 돌리지 않았습니다({})", s.file.as_deref().unwrap_or(p));
                r.note = Some("보안: 고정본이 출발 때와 달라 돌리지 않음".to_string());
                results.push(r);
                continue;
            }
        }
        let r = runner.run(s);
        let stop = r.interrupted;
        results.push(r);
        if stop {
            break;
        }
    }
    JudgeOutcome { steps: steps.to_vec(), results }
}

// 원본 `imageExt`
const IMAGE_EXT: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

// 원본 `(x * 1000).rounded() / 1000`(0 에서 먼 쪽으로 반올림 = `f64::round`)
fn round_ms(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// 증거 요약(증거 폴더 summary.json, engine.json checks[] 의 parts). 브라우저 걸음은 남긴 그림 파일을 screenshots 로, 없으면 note. 원본: `apJudgeSummary`
pub fn judge_summary(o: &JudgeOutcome, sandbox: &str, at: f64) -> Value {
    let mut parts: Vec<Value> = vec![];
    for (s, r) in o.steps.iter().zip(o.results.iter()) {
        let mut p = Map::new();
        p.insert("name".into(), json!(s.name));
        p.insert("label".into(), json!(s.label));
        p.insert("kind".into(), json!(s.kind));
        p.insert("exit".into(), json!(r.code));
        p.insert("sec".into(), json!(round_ms(r.sec)));
        p.insert("timed_out".into(), json!(r.timed_out));
        p.insert("interrupted".into(), json!(r.interrupted));
        p.insert("passed".into(), json!(r.code == 0 && !r.timed_out));
        p.insert("evidence".into(), json!(s.evidence));
        p.insert("files".into(), json!(r.files));
        if let Some(f) = &s.file {
            p.insert("file".into(), json!(f));
        }
        if let Some(c) = &s.criterion {
            p.insert("criterion".into(), json!(c));
        }
        if let Some(pin) = &s.pinned {
            p.insert("pinned".into(), json!(pin));
        }
        if let Some(h) = &s.sha256 {
            p.insert("sha256".into(), json!(h));
        }
        if let Some(n) = &r.note {
            p.insert("note".into(), json!(n));
        }
        if s.kind == "browser" {
            let shots: Vec<&String> = r.files.iter().filter(|f| IMAGE_EXT.contains(&path_extension(f).to_lowercase().as_str())).collect();
            let none = shots.is_empty();
            p.insert("screenshots".into(), json!(shots));
            if none && r.note.is_none() {
                p.insert("note".into(), json!("브라우저 시나리오가 스크린샷을 남기지 않았습니다"));
            }
        }
        parts.push(Value::Object(p));
    }
    json!({"at": round_ms(at), "passed": o.passed(), "sandbox": sandbox, "parts": parts})
}

// ---------------------------------------------------------------------------------------------------------------------
// 판정 미리 돌리기의 시나리오 카드
// ---------------------------------------------------------------------------------------------------------------------

/// 단위 전체는 실패했지만 시나리오 하나가 일하기 전부터 통과(이미 통과)했을 때의 출발 카드 감. 단위 전체가 통과하면 단위 카드를 쓴다.
/// 추천은 판정 고치기(그 시나리오가 뜻을 보지 않을 수 있다. 설계 2장 ① 반대 탐침 카드와 같은 꼴). 원본: `apPrerunScenarioCard`
pub fn prerun_scenario_card(n: i64, id: &str, title: &str, file: &str, criterion: Option<&str>) -> Value {
    let crit = criterion.map(|c| format!("(수용 기준 {c})")).unwrap_or_default();
    json!({
        "id": format!("prerun-{id}-{file}"),
        "unit": n,
        "unit_id": id,
        "kind": "prerun",
        "scenario": file,
        "title": format!("「{title}」 의 시나리오 {file}{crit} 가 일하기 전부터 통과합니다"),
        "why": "그 시나리오가 일한 결과의 뜻을 보지 않거나 이미 된 기준이라는 뜻입니다. 그대로 두면 그 기준은 일하지 않아도 통과로 남습니다.",
        "options": [{"key": "fix_check", "label": "판정 고치기"}, {"key": "keep", "label": "그대로"}, {"key": "drop", "label": "빼기"}],
        "recommended": "fix_check",
    })
}
