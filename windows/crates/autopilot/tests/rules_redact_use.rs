//! 비밀 꼴 가리기 사용 자리 시험(원본 Swift 시험 없음, extra_) — 초안 단위 검사(`draft_unit_parse` 의 text 칸)·지시 검사(`instruction_check`)·
//! 판정 비판 `why`(`critic_parse`)가 전체 규칙 가리기 `rules::meta::redact`(`apRedact`)를 쓰는지 견준다(R-Redact-a).
//! 그 전엔 세 자리 모두 Bearer·basic 만 보는 최소판 사본을 썼으므로, GitHub 토큰(`ghp_`) 꼴 케이스가 옛 코드에서 빨개진다 —
//! 이 케이스가 곧 전체 규칙으로 바뀌었음의 월줄이고, Bearer 케이스는 옛 최소판도 잡던 꼴이 그대로 잡히는 복원 검사다.
//! 토큰은 글자 이어 붙여 만든다(진짜 토큰 꼴이 저장소에 남지 않게). 가짜 어댑터는 rules_draft.rs·rules_critique.rs 의 것을 최소로 베꼈다. 값은 장난감(zz-toy·example.com).

use std::collections::HashMap;

use serde_json::json;

use claude_board_autopilot::rules::critique::{critic_parse, CriticUnit};
use claude_board_autopilot::rules::draft::{draft_unit_parse, instruction_check, DraftFiles, INSTRUCTION_HEAD_LINE};
use claude_board_autopilot::rules::scenarios::Criterion;

// MARK: 가짜 어댑터(secrets 문구로 남지 않게 이어 붙인 토큰)

/// 결과 폴더 가짜 — 지도에 든 파일만 읽는다(rules_draft.rs 의 FakeDraftFiles 같은 패)
struct FakeFiles(HashMap<String, String>);

impl DraftFiles for FakeFiles {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.0.get(name).map(|s| s.as_bytes().to_vec())
    }

    fn list(&self, _folder: &str) -> Vec<String> {
        vec![]
    }
}

/// GitHub 토큰 꼴(ghp_ 뒤 알파벳 숫자 36 글자 — 전체 규칙의 규칙 9가 잡는 꼴, 최소판은 못 잡는다)
fn github_token() -> String {
    format!("ghp_{}", "Qq7".repeat(12))
}

/// Bearer 인증 값 꼴(뒤 알파벳 숫자 24 글자 — 규칙 16이 잡는 꼴, 옛 최소판도 잡는다)
fn bearer_value() -> String {
    format!("Bearer {}", "Qq7".repeat(8))
}

/// 통과 꼴 초안 단위(check 만 넣어 바꾼다)
fn unit_json(check: &str) -> serde_json::Value {
    json!({
        "id": "u1",
        "title": "할 일 목록 zz-toy API",
        "kind": "dev",
        "prompt": "목록 API 를 만든다",
        "check": check,
        "est_min": 30
    })
}

/// 판정 비판 세션이 보는 단위 하나(검사에 쓰는 최소 꼴)
fn critic_units() -> Vec<CriticUnit> {
    vec![CriticUnit::new("u1", "할 일 목록 API", "목록 API 를 만든다", vec![Criterion::new("AC-1", "응답 본문이 있다")], "", vec![])]
}

const WHY_PROBLEM: &str = "지적 f1 의 why 는 300자까지의 글(태그·비밀 꼴 없음)입니다";

/// findings.json 지도 한 장(why 만 넣는다) — 종류는 files 칸 1~10개가 필수가 아닌 unverifiable 로 둔다(R-Redact-a: why 가 문제면
/// 이 뒤의 반례 파일 검사에 가지 않는다(`continue`) — why 만 바꾼 검사의 짝이 되게 하려고 꼴은 늘 온전하게 둔다)
fn findings_files(why: &str) -> FakeFiles {
    FakeFiles(HashMap::from([(
        "findings.json".to_string(),
        json!([{"id": "f1", "unit": "u1", "kind": "unverifiable", "why": why}]).to_string(),
    )]))
}

// MARK: 초안 단위 검사(draft_unit_parse 의 text 칸)

/// GitHub 토큰 — 최소판은 Bearer·basic 만 봤으므로 이 케이스는 옛 코드에서 빨개진다(전체 규칙으로 바뀌었음의 월줄)
#[test]
fn extra_단위_판정명령의_github_토큰을_비밀_꼴로_잡는다() {
    let check = format!("curl -s {} https://api.example.com", github_token());
    let (u, problems, warnings) = draft_unit_parse(&unit_json(&check), 1, &FakeFiles(HashMap::new()), &[]);
    assert!(u.is_some(), "단위는 만들어진다: {problems:?}");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        problems.iter().any(|p| p == "단위 1 판정 명령 에 비밀 꼴(토큰·키 등)이 있습니다"),
        "problems: {problems:?}"
    );
}

/// Bearer 인증 값 — 옛 최소판도 잡던 꼴이 전체 규칙을 쓴 뒤에도 그대로 잡힌다(복원 검사)
#[test]
fn extra_단위_판정명령의_bearer_토큰도_잡는다() {
    let check = format!("curl -H 'Authorization: {}' https://api.example.com", bearer_value());
    let (_, problems, _) = draft_unit_parse(&unit_json(&check), 1, &FakeFiles(HashMap::new()), &[]);
    assert!(
        problems.iter().any(|p| p == "단위 1 판정 명령 에 비밀 꼴(토큰·키 등)이 있습니다"),
        "problems: {problems:?}"
    );
}

/// 비밀이 없는 평범한 판정 명령은 비밀 꼴 문제 없이 통과한다(거짓잡기 없는 짝)
#[test]
fn extra_단위_판정명령에_비밀이_없으면_잡지_않는다() {
    let (u, problems, warnings) = draft_unit_parse(&unit_json("python -m pytest zz_toy_test -q"), 1, &FakeFiles(HashMap::new()), &[]);
    assert!(u.is_some());
    assert!(problems.is_empty(), "problems: {problems:?}");
    assert!(warnings.is_empty(), "{warnings:?}");
}

// MARK: 지시 검사(instruction_check)

#[test]
fn extra_지시_검사의_github_토큰을_비밀_꼴로_잡는다() {
    let t = format!("{}\n누설 토큰은 {} 와 같은 꼴이다.", INSTRUCTION_HEAD_LINE, github_token());
    let (_, problems, _, warnings) = instruction_check(&t, &[]);
    assert!(
        problems.iter().any(|p| p == "자율주행 지시(autopilot.md)에 비밀 꼴(토큰·키 등)이 있습니다"),
        "problems: {problems:?} warnings: {warnings:?}"
    );
}

#[test]
fn extra_지시_검사의_bearer_토큰도_잡는다() {
    let t = format!("{}\n인증 머리는 {} 처럼 붙인다.", INSTRUCTION_HEAD_LINE, bearer_value());
    let (_, problems, _, warnings) = instruction_check(&t, &[]);
    assert!(
        problems.iter().any(|p| p == "자율주행 지시(autopilot.md)에 비밀 꼴(토큰·키 등)이 있습니다"),
        "problems: {problems:?} warnings: {warnings:?}"
    );
}

// MARK: 판정 비판 why(critic_parse)

#[test]
fn extra_판정_비판_why의_github_토큰을_비밀_꼴로_잡는다() {
    let why = format!("반례 파일은 {} 한 장이면 된다", github_token());
    let (findings, problems) = critic_parse(&findings_files(&why), &critic_units(), &HashMap::new());
    assert_eq!(problems, vec![WHY_PROBLEM], "지적은 버려진다");
    assert!(findings.is_empty());
}

#[test]
fn extra_판정_비판_why의_bearer_토큰도_잡는다() {
    let why = format!("Bearer {} 로 부르면 통과하지만 실제로는 만들어지지 않는다", bearer_value());
    let (findings, problems) = critic_parse(&findings_files(&why), &critic_units(), &HashMap::new());
    assert_eq!(problems, vec![WHY_PROBLEM], "지적은 버려진다");
    assert!(findings.is_empty());
}

#[test]
fn extra_판정_비판_why에_비밀이_없으면_잡지_않는다() {
    let (findings, problems) = critic_parse(
        &findings_files("응답 본문이 비어 있어도 통과한다"),
        &critic_units(),
        &HashMap::new(),
    );
    assert!(problems.is_empty(), "problems: {problems:?}");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id, "f1");
}
