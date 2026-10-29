//! 카드 묶음과 store 의 연결 자리 — `rules-table/sessions-group.json` 공유 규칙 표 시험이
//! 이곳에서 `crate::rules_table::run("sessions-group", …)` 로 돌아간다. 꼴은 rules-table/README.md
//! 이다. 표 줄마다 임시 sessions 폴더에 기록을 쓰고 `Store::reload` 가 만든 묶음의 카드 차례와 카드 안 세션 차례를 견준다.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use crate::model::Group;
    use crate::rules_table::{self, Case, RuleFn};
    use crate::settings::Settings;
    use crate::store::{Probe, Store, transcript_path};

    /// 표의 시각은 지금 기준 초(음수 = 과거)다. store 는 유닉스 초를 보므로 여기서 옮긴다.
    const NOW: f64 = 1_792_000_000.0;

    /// input 에서 쓸 수 있는 키. 모르는 키는 꼴 오류다(README).
    const INPUT_KEYS: [&str; 2] = ["records", "pinned"];
    /// expect 에서 쓸 수 있는 키.
    const EXPECT_KEYS: [&str; 1] = ["groups"];
    /// records 항목의 키.
    const ROW_KEYS: [&str; 2] = ["file", "record"];
    /// groups 항목의 키.
    const GROUP_KEYS: [&str; 2] = ["id", "sessions"];

    /// 가짜 보드 폴더 — 세션 기록 폴더·notifier·~/.claude 를 담고 버릴 때 함께 지운다.
    struct Lines {
        root: PathBuf,
        sessions: PathBuf,
        notifier: PathBuf,
        claude: PathBuf,
    }

    impl Lines {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cb-group-table-{}", crate::util::short_id()));
            let sessions = root.join("notifier").join("sessions");
            let claude = root.join(".claude");
            fs::create_dir_all(&sessions).expect("임시 sessions 폴더를 만들지 못했다");
            fs::create_dir_all(&claude).expect("임시 .claude 폴더를 만들지 못했다");
            Lines { notifier: root.join("notifier"), sessions, claude, root }
        }

        /// 세션 기록과 store 가 파악할 자리의 대화 기록을 쓴다. since·updated 를 절대 시각으로 옮긴다.
        /// session_id 를 기록에 더하지 않는다 — 파일 이름으로 세션 id 를 얻는 줄 때문이다.
        fn record(&self, file: &str, mut rec: Value) {
            rec["since"] = json!(NOW + rec["since"].as_f64().unwrap_or_default());
            rec["updated"] = json!(NOW + rec["updated"].as_f64().unwrap_or_default());
            let sid = rec.get("session_id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| file.trim_end_matches(".json").to_string());
            let cwd = rec.get("cwd").and_then(Value::as_str).unwrap_or("?");
            let path = transcript_path(None, cwd, &sid, &self.claude);
            fs::create_dir_all(path.parent().expect("대화 기록의 부모 폴더")).expect("대화 기록 폴더를 만들지 못했다");
            fs::write(&path, br#"{"type":"assistant","effort":"high","message":{"model":"claude-opus-5-5","usage":{"input_tokens":100}}}"#).expect("대화 기록을 쓰지 못했다");
            fs::write(self.sessions.join(file), serde_json::to_vec(&rec).expect("기록을 JSON 으로 만들지 못했다")).expect("세션 기록을 쓰지 못했다");
        }
    }

    impl Drop for Lines {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 묶음 하나의 모르는 키를 가린다.
    fn known_keys(value: &Value, name: &str, allowed: &[&str]) -> Result<(), String> {
        let obj = value.as_object().ok_or_else(|| format!("{name} 는 객체여야 한다"))?;
        for key in obj.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(format!("{name} 의 모르는 키 '{key}'"));
            }
        }
        Ok(())
    }

    /// input·expect 에 모르는 키가 없는지 본다.
    fn check_keys(case: &Case) -> Result<(), String> {
        known_keys(&case.input, "input", &INPUT_KEYS)?;
        known_keys(&case.expect, "expect", &EXPECT_KEYS)?;
        Ok(())
    }

    /// input.records 를 (파일 이름, 기록) 목록으로. 꼴이 틀리면 멈춘다.
    fn records_of(case: &Case) -> Result<Vec<(String, Value)>, String> {
        let rows = case.input.get("records").and_then(Value::as_array).ok_or_else(|| "records 는 배열이어야 한다".to_string())?;
        if rows.is_empty() {
            return Err("records 는 비지 않은 배열이어야 한다".to_string());
        }
        let mut out = Vec::new();
        for row in rows {
            let row = row.as_object().ok_or_else(|| "records 의 항목은 객체여야 한다".to_string())?;
            let file = row.get("file").and_then(Value::as_str).filter(|f| f.len() > 5 && f.ends_with(".json")).ok_or_else(|| "records 항목의 file 은 .json 으로 끝나는 이름이어야 한다".to_string())?;
            let rec = row.get("record").cloned().ok_or_else(|| "records 항목에 record 가 없다".to_string())?;
            for key in ["since", "updated"] {
                if rec.get(key).and_then(Value::as_f64).is_none() {
                    return Err(format!("{file} 의 {key} 는 숫자(지금 기준 초)여야 한다"));
                }
            }
            for key in row.keys() {
                if !ROW_KEYS.contains(&key.as_str()) {
                    return Err(format!("records 항목의 모르는 키 '{key}'"));
                }
            }
            out.push((file.to_string(), rec));
        }
        Ok(out)
    }

    /// input.pinned — 없으면 빈 목록.
    fn pinned_of(case: &Case) -> Result<Vec<String>, String> {
        match case.input.get("pinned") {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => {
                items.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| "pinned 는 경로 문자열 목록이어야 한다".to_string())).collect()
            }
            Some(_) => Err("pinned 는 경로 문자열 목록이어야 한다".to_string()),
        }
    }

    /// expect.groups 를 (카드 id, 세션 id 목록) 목록으로. 꼴이 틀리면 멈춘다.
    fn expected_shape(case: &Case) -> Result<Vec<(String, Vec<String>)>, String> {
        let groups = case.expect.get("groups").and_then(Value::as_array).ok_or_else(|| "expect.groups 는 배열이어야 한다".to_string())?;
        let mut out = Vec::new();
        for group in groups {
            let group = group.as_object().ok_or_else(|| "groups 의 항목은 객체여야 한다".to_string())?;
            let id = group.get("id").and_then(Value::as_str).ok_or_else(|| "groups 항목의 id 는 문자열이어야 한다".to_string())?;
            let sessions = group.get("sessions").and_then(Value::as_array).ok_or_else(|| "groups 항목의 sessions 는 배열이어야 한다".to_string())?;
            let sessions = sessions.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| "sessions 는 세션 id 문자열 목록이어야 한다".to_string())).collect::<Result<Vec<_>, _>>()?;
            for key in group.keys() {
                if !GROUP_KEYS.contains(&key.as_str()) {
                    return Err(format!("groups 항목의 모르는 키 '{key}'"));
                }
            }
            out.push((id.to_string(), sessions));
        }
        Ok(out)
    }

    /// store 가 돌려준 묶음을 (카드 id, 세션 id 목록) 목록으로.
    fn shaped(groups: &[Group]) -> Vec<(String, Vec<String>)> {
        groups.iter().map(|g| (g.id.clone(), g.sessions.iter().map(|s| s.id.clone()).collect())).collect()
    }

    /// 표 줄 하나를 돌린다 — 기록을 임시 폴더에 쓰고 reload 로 묶음을 본 뒤 기대와 견준다.
    fn group(case: &Case) -> Result<(), String> {
        check_keys(case)?;
        let records = records_of(case)?;
        let pinned = pinned_of(case)?;
        let expected = expected_shape(case)?;
        let lines = Lines::new();
        for (file, rec) in &records {
            lines.record(file, rec.clone());
        }
        let mut settings = Settings { pinned_groups: pinned, ..Settings::default() };
        let mut store = Store::new(lines.sessions.clone(), &lines.notifier, lines.claude.clone(), lines.root.to_string_lossy().into_owned());
        let reloaded = store.reload(&mut settings, &Probe::default(), &|_| true, false, NOW);
        let actual = shaped(&reloaded.groups);
        if actual != expected {
            return Err(format!("묶음이 다르다\n  기대 {expected:?}\n  실제 {actual:?}"));
        }
        Ok(())
    }

    #[test]
    fn sessions_group_table() {
        rules_table::run("sessions-group", &[("group", group as RuleFn)]);
    }
}