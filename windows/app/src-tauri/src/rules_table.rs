//! 공유 규칙 표(`rules-table/`)의 돌리는 쪽. 시험 전용이라 모듈째 `#[cfg(test)]` 이다.
//!
//! 꼴과 유지 규칙은 저장소 맨 위 `rules-table/README.md` 가 소유한다(plan §2.2). 이 모듈은:
//! `load(family)` 가 `CARGO_MANIFEST_DIR/../../../rules-table/<family>.json` 을 읽어 필수 키와
//! id 유일성을 확인하고 `only == "mac"` 줄을 건너뛰며, `run(family, rules)` 가 파일의 모든 줄을
//! 돌려 실패한 id 를 모아 한 번에 보인다. 모르는 rule 과 돈 줄이 0 인 경우도 실패다.

use std::path::Path;

use serde_json::Value;

/// 규칙 표의 줄 하나.
#[derive(Debug)]
pub struct Case {
    /// `<family>.<rule>.<영문 slug>` — 파일 안에서 유일, 한 번 쓴 id 는 다시 쓰지 않는다.
    pub id: String,
    /// 돌리는 쪽이 이 이름으로 판정 함수를 고른다.
    pub rule: String,
    /// 의도한 동작(한국어) — 사람이 읽는 근거이고 Swift·Rust 출력으로 만들지 않는다.
    pub why: String,
    /// 근거(PR·주인 문장). 선택.
    pub source: Option<String>,
    pub input: Value,
    pub expect: Value,
}

/// rule 하나의 판정 함수 — 통과면 `Ok(())`, 실패면 까닭.
pub type RuleFn = fn(&Case) -> Result<(), String>;

/// 줄의 필수 문자열 키를 꺼낸다. 비지 않은 문자열만 받는다.
fn need_string(row: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    match row.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(format!("{key} 는 비지 않은 문자열이어야 한다")),
    }
}

/// 표 본문을 꼴대로 확인한다. `only == "mac"` 줄은 Windows 돌리는 쪽이 건너뛴다(mac 쪽이 돌릴 줄).
/// 돌려주는 값은 파일의 `family` 값과 건너뛴 뒤의 줄들이다.
pub fn parse_table(text: &str) -> Result<(String, Vec<Case>), String> {
    use std::collections::HashSet;

    let root: Value = serde_json::from_str(text).map_err(|e| format!("JSON 을 읽지 못했다: {e}"))?;
    let table = root.as_object().ok_or_else(|| "규칙 표 최상위는 객체여야 한다".to_string())?;

    let family = match table.get("family") {
        Some(Value::String(value)) if !value.is_empty() => value.clone(),
        _ => return Err("family 는 비지 않은 문자열이어야 한다".to_string()),
    };
    match table.get("schema") {
        Some(Value::Number(value)) if value.as_i64() == Some(1) => {}
        Some(other) => return Err(format!("schema 는 숫자 1 이어야 한다(지금 {other})")),
        None => return Err("schema 가 없다".to_string()),
    }
    let rows = match table.get("cases") {
        Some(Value::Array(rows)) => rows,
        Some(_) => return Err("cases 는 배열이어야 한다".to_string()),
        None => return Err("cases 가 없다".to_string()),
    };

    // 줄에서 그 밖에 쓸 수 있는 키는 이 일곱뿐이다. 모르는 키는 꼴 오류다.
    const CASE_KEYS: [&str; 7] = ["id", "rule", "why", "source", "only", "input", "expect"];

    let mut seen = HashSet::new();
    let mut cases = Vec::new();
    for row in rows {
        let Some(row) = row.as_object() else {
            return Err("case 는 객체여야 한다".to_string());
        };
        for key in row.keys() {
            if !CASE_KEYS.contains(&key.as_str()) {
                return Err(format!("모르는 키 '{key}' 가 있는 줄이 있다"));
            }
        }
        let id = need_string(row, "id")?;
        let rule = need_string(row, "rule")?;
        let why = need_string(row, "why")?;
        let source = match row.get("source") {
            Some(Value::String(source)) => Some(source.clone()),
            Some(_) => return Err("source 는 문자열이어야 한다".to_string()),
            None => None,
        };
        let only = match row.get("only") {
            Some(Value::String(only)) if only == "mac" || only == "windows" => Some(only.as_str()),
            Some(_) => return Err("only 는 \"mac\" 또는 \"windows\" 이어야 한다".to_string()),
            None => None,
        };
        let input = row
            .get("input")
            .ok_or_else(|| "input 이 없는 줄이 있다".to_string())?
            .clone();
        let expect = row
            .get("expect")
            .ok_or_else(|| "expect 이 없는 줄이 있다".to_string())?
            .clone();
        if !seen.insert(id.clone()) {
            return Err(format!("id 가 겹친다: {id}"));
        }
        if only == Some("mac") {
            continue; // mac 쪽만 돌릴 줄 — Windows 돌리는 쪽은 건너뛴다
        }
        cases.push(Case { id, rule, why, source, input, expect });
    }
    Ok((family, cases))
}

/// `rules-table/<family>.json` 을 읽어 줄로 돌려준다. 파일이 없거나 꼴이 틀리거나
/// JSON 의 `family` 값이 파일 이름과 다르면 멈춘다(setup 실수를 조용히 못 넘어가게).
pub fn load(family: &str) -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../rules-table")
        .join(format!("{family}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("규칙 표 파일을 읽지 못했다: {} ({e})", path.display()));
    let (file_family, cases) =
        parse_table(&text).unwrap_or_else(|e| panic!("규칙 표 {} 해석 실패: {e}", path.display()));
    if file_family != family {
        panic!("규칙 표 {} 의 family 값은 '{}' 인데 파일 이름은 '{}' 다", path.display(), file_family, family);
    }
    cases
}

/// 줄 전부를 돌린다. 모르는 rule 과 판정 실패를 실패로 모아 한 번에 돌려주고, 돈 줄이 0 이면 실패다.
pub fn run_cases(cases: &[Case], rules: &[(&str, RuleFn)]) -> Result<(), String> {
    if cases.is_empty() {
        return Err("돌린 줄이 0 개다".to_string());
    }
    let mut failures = Vec::new();
    for case in cases {
        let Some((_, rule_fn)) = rules.iter().find(|(name, _)| *name == case.rule) else {
            failures.push(format!("{}: 모르는 rule '{}'", case.id, case.rule));
            continue;
        };
        if let Err(reason) = rule_fn(case) {
            failures.push(format!("{}: {reason}", case.id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// `load(family)` 로 줄을 읽고 전부 돌린다. 실패(모은 실패 id·모르는 rule·0 줄)면 패닉한다.
pub fn run(family: &str, rules: &[(&str, RuleFn)]) {
    let cases = load(family);
    if let Err(failures) = run_cases(&cases, rules) {
        panic!("공유 규칙 표 시험이 실패한다({family}): {failures}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 지나가는 줄을 기록하는 판정 함수 — 정상 표가 전부 돌았는지 세는 데 쓴다.
    fn record(case: &Case) -> Result<(), String> {
        RAN.lock().unwrap().push(case.id.clone());
        Ok(())
    }

    /// 항상 실패하는 판정 함수 — 실패 id 가 한 번에 모이는지 보는 데 쓴다.
    fn always_fail(_case: &Case) -> Result<(), String> {
        Err("항상 실패한다".to_string())
    }

    /// 돌아간 줄을 순서대로 남긴다.
    static RAN: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn cases(text: &str) -> Vec<Case> {
        parse_table(text).unwrap().1
    }

    /// 줄 세 개의 정상 표.
    const GOOD: &str = r#"{
  "family": "probe",
  "schema": 1,
  "cases": [
    {"id": "probe.probe.alpha", "rule": "probe", "why": "첫 줄", "input": {"a": 1}, "expect": {"a": 1}},
    {"id": "probe.probe.beta", "rule": "probe", "why": "둘째 줄", "source": "PR #0", "input": {}, "expect": {}},
    {"id": "probe.probe.gamma", "rule": "probe", "why": "셋째 줄", "input": {}, "expect": {}}
  ]
}"#;

    #[test]
    fn duplicate_id_fails() {
        const DUP: &str = r#"{
  "family": "probe",
  "schema": 1,
  "cases": [
    {"id": "probe.probe.same", "rule": "probe", "why": "먼저 쓴 줄", "input": {}, "expect": {}},
    {"id": "probe.probe.same", "rule": "probe", "why": "id 를 다시 쓴 줄", "input": {}, "expect": {}}
  ]
}"#;
        let err = parse_table(DUP).unwrap_err();
        assert!(err.contains("id 가 겹친다: probe.probe.same"), "{err}");
    }

    #[test]
    fn missing_why_fails() {
        const NO_WHY: &str = r#"{
  "family": "probe",
  "schema": 1,
  "cases": [
    {"id": "probe.probe.nowhy", "rule": "probe", "input": {}, "expect": {}}
  ]
}"#;
        let err = parse_table(NO_WHY).unwrap_err();
        assert!(err.contains("why"), "{err}");
    }

    #[test]
    fn schema_other_than_1_fails() {
        // 꼴이 바뀌는 뒷판을 막는 문이다 — schema 값이 1 이 아니면 꼴 오류다
        const BAD: &str = r#"{"family": "probe", "schema": 2, "cases": []}"#;
        let err = parse_table(BAD).unwrap_err();
        assert!(err.contains("schema 는 숫자 1 이어야 한다"), "{err}");
    }

    #[test]
    fn unknown_rule_fails() {
        // 돌리는 쪽이 "cache" 함수를 주지 않으면 그 줄은 실패로 모인다(plan §2.2 "모르는 rule 은 실패")
        let table = cases(GOOD);
        run_cases(&table, &[("probe", record)]).expect("probe 함수를 주었으면 실패가 없다");
        let err = run_cases(&table, &[("other", record)]).unwrap_err();
        assert!(err.contains("probe.probe.alpha: 모르는 rule 'probe'"), "{err}");
    }

    #[test]
    fn empty_table_fails() {
        const EMPTY: &str = r#"{
  "family": "probe",
  "schema": 1,
  "cases": []
}"#;
        let table = parse_table(EMPTY).unwrap().1;
        let err = run_cases(&table, &[("probe", record)]).unwrap_err();
        assert!(err.contains("0"), "{err}");
    }

    #[test]
    fn good_table_runs_all_cases_in_order() {
        RAN.lock().unwrap().clear();
        let table = cases(GOOD);
        assert_eq!(table.len(), 3);
        // 줄의 값들이 온전히 실려 가는지 — 판정 함수가 input 과 expect 만으로 판단할 수 있어야 한다
        let first = &table[0];
        assert_eq!(first.input["a"], 1, "input 은 JSON 그대로 실려 가야 한다");
        assert_eq!(first.expect["a"], 1, "expect 도 그대로다");
        assert_eq!(first.why, "첫 줄");
        assert_eq!(first.source, None);
        let middle = &table[1];
        assert_eq!(middle.source.as_deref(), Some("PR #0"));
        assert_eq!(table[2].source, None, "source 는 없으면 없는 것이다");
        run_cases(&table, &[("probe", record)]).expect("정상 표는 실패가 없다");
        let ran = RAN.lock().unwrap().clone();
        assert_eq!(ran, vec!["probe.probe.alpha", "probe.probe.beta", "probe.probe.gamma"]);
    }

    #[test]
    fn failing_ids_are_collected_together() {
        let table = cases(GOOD);
        let err = run_cases(&table, &[("probe", always_fail)]).unwrap_err();
        assert!(err.contains("probe.probe.alpha: 항상 실패한다"), "{err}");
        assert!(err.contains("probe.probe.beta: 항상 실패한다"), "{err}");
        assert!(err.contains("probe.probe.gamma: 항상 실패한다"), "{err}");
    }

    #[test]
    fn only_mac_rows_are_skipped() {
        const WITH_MAC: &str = r#"{
  "family": "probe",
  "schema": 1,
  "cases": [
    {"id": "probe.probe.here", "rule": "probe", "why": "둘 다 돌리는 줄", "input": {}, "expect": {}},
    {"id": "probe.probe.macthat", "only": "mac", "rule": "probe", "why": "mac 쪽만 돌리는 줄", "input": {}, "expect": {}}
  ]
}"#;
        let table = cases(WITH_MAC);
        let ids: Vec<&str> = table.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["probe.probe.here"]);
        // only 값이 "mac"/"windows" 밖이면 꼴 오류다
        const BAD_ONLY: &str = r#"{
  "family": "probe",
  "schema": 1,
  "cases": [
    {"id": "probe.probe.stray", "only": "both", "rule": "probe", "why": "값이 틀린 줄", "input": {}, "expect": {}}
  ]
}"#;
        let err = parse_table(BAD_ONLY).unwrap_err();
        assert!(err.contains("only"), "{err}");
    }

    #[test]
    #[should_panic(expected = "규칙 표 파일을 읽지 못했다")]
    fn load_without_file_stops_loudly() {
        // probe.json 은 실제 표로 만들지 않을 이름이라 파일이 없는 것이 보장된다
        load("probe");
    }

    #[test]
    #[should_panic(expected = "규칙 표 파일을 읽지 못했다")]
    fn run_without_file_stops_loudly() {
        // run 은 load 로 파일을 읽는다 — 파일이 없으면 조용히 빈 표로 넘어가지 않는다
        run("probe", &[("probe", record)]);
    }
}