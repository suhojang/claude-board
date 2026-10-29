//! 세션 스캔 공유 규칙 표(`rules-table/session-scan.json`)의 Windows 돌리는 쪽.
//!
//! 꼴은 `rules-table/README.md`(plan §2.2·2.3) 가 소유하고 줄 목록은 plan §2.4 가 소유한다.
//! 이 모듈은 매 줄마다 임시 폴더로 세상을 만들어(`World`) `Store::reload` 를 한 번 돌리고
//! 기대(`kept`·`state`)를 견준다. 시각은 표 안에서 지금 = 0 기준 초라 지금 값(`NOW`) 을 더해
//! 절대시각으로 바꾸고, `claude_sessions` 의 `age` 는 그 파일 mtime 이 진짜 지금보다 몇 초 전인
//! 거라 그대로 옮긴다(같은 sessionId 파일이 둘일 때 가장 최근 파일이 이긴다). 세션의 생사는
//! 표의 `procs` 가 정한다: "alive"·"denied" → 산 것("denied" 는 권한이 없어 볼 수 없을 뿐이라
//! 산 것으로 본다), "gone" → 죽은 것, 표에 없는 pid 는 없다. 기록의 pid 가 없으면(0) 지금의
//! 옛 동작 대로 산 것으로 본다 · sessionId 로 pid 를 찾는 일은 store.rs 쪽이다.

use std::collections::HashMap;
use std::fs;
use std::fs::FileTimes;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use super::{transcript_path, Probe};
use crate::rules_table::Case;
use crate::settings::Settings;
use crate::store::Store;
use claude_board_core::claude_live::claude_session_path;
use claude_board_core::Record;

/// 기록의 상대 시각을 절대시각으로 만들 때 더하는 지금 값.
const NOW: f64 = 1_800_000_000.0;

/// 표를 끝까지 돌린다. 실패하는 줄은 `run` 이 id 를 모아 한 번에 보인다(첫 실패에서 끊지 않는다).
#[test]
fn session_scan_table_runs_every_row() {
    crate::rules_table::run("session-scan", &[("scan", scan)]);
}

/// 표 시험 세계 · 매 줄마다 새 임시 폴더. store.rs 의 Fake 는 tests 모듈의 사적 자리라 여기서 작게 만든다.
struct World {
    root: PathBuf,
    sessions: PathBuf,
    claude: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cb-scan-table-{}", crate::util::short_id()));
        let sessions = root.join("notifier").join("sessions");
        let claude = root.join(".claude");
        fs::create_dir_all(&sessions).expect("세션 폴더를 만들어야 한다");
        fs::create_dir_all(&claude).expect("claude 폴더를 만들어야 한다");
        World { root, sessions, claude }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// `claude_sessions` 줄의 원소 · `{pid, sessionId, age}`.
struct ClaudeFile {
    pid: u32,
    session_id: String,
    age: f64,
}

/// `procs` 를 꺼낸다 · pid → 산 것인지. "gone" 만 죽은 것이고 표에 없는 pid 는 모른다(plan §2.3).
/// "denied" 는 권한이 없어 볼 수 없을 뿐이라 산 것으로 본다(PR #12).
fn parse_procs(input: &serde_json::Map<String, Value>) -> Result<HashMap<u32, bool>, String> {
    let Some(procs) = input.get("procs") else {
        return Ok(HashMap::new());
    };
    let map = procs.as_object().ok_or_else(|| "scan input 의 procs 는 객체여야 한다".to_string())?;
    let mut out = HashMap::new();
    for (pid_text, verdict) in map {
        let pid = pid_text
            .parse::<u32>()
            .map_err(|_| format!("procs 의 pid '{pid_text}' 은 u32 수가 아니다"))?;
        if pid == 0 {
            return Err("procs 에 pid 0 은 없다 · 0 은 '기록에 pid 가 없다' 는 뜻이다".to_string());
        }
        let verdict_text = verdict
            .as_str()
            .ok_or_else(|| format!("procs 의 '{pid_text}' 값은 문자열이어야 한다"))?;
        let alive = match verdict_text {
            "alive" | "denied" => true,
            "gone" => false,
            other => return Err(format!("procs 의 '{other}' 은 alive·denied·gone 중 하나여야 한다")),
        };
        out.insert(pid, alive);
    }
    Ok(out)
}

/// `claude_sessions` 배열을 꺼낸다 · 원소는 `{pid, sessionId, age}` 이고 모르는 키·이상한 값은 실패한다.
fn parse_claude_files(input: &serde_json::Map<String, Value>) -> Result<Vec<ClaudeFile>, String> {
    let Some(claude_sessions) = input.get("claude_sessions") else {
        return Ok(Vec::new());
    };
    let list = claude_sessions.as_array().ok_or_else(|| "scan input 의 claude_sessions 는 배열이어야 한다".to_string())?;
    const FILE_KEYS: [&str; 3] = ["pid", "sessionId", "age"];
    list.iter()
        .enumerate()
        .map(|(index, element)| {
            let element =
                element.as_object().ok_or_else(|| format!("claude_sessions[{index}] 는 객체여야 한다"))?;
            for key in element.keys() {
                if !FILE_KEYS.contains(&key.as_str()) {
                    return Err(format!("claude_sessions[{index}] 에 모르는 키 '{key}' 가 있다"));
                }
            }
            let pid = element
                .get("pid")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("claude_sessions[{index}] 의 pid 는 수여야 한다"))?;
            let pid = u32::try_from(pid).map_err(|_| format!("claude_sessions[{index}] 의 pid {pid} 은 u32 에 안 담긴다"))?;
            if pid == 0 {
                return Err(format!("claude_sessions[{index}] 의 pid 는 0 이 아니다 · 0 짜리 파일은 읽히지 않는다"));
            }
            let session_id = element
                .get("sessionId")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("claude_sessions[{index}] 의 sessionId 는 문자열이어야 한다"))?
                .to_string();
            let age = element
                .get("age")
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("claude_sessions[{index}] 의 age 는 수여야 한다"))?;
            if age < 0.0 {
                return Err(format!("claude_sessions[{index}] 의 age 는 지금보다 과거여야 한다(거꾸로다)"));
            }
            Ok(ClaudeFile { pid, session_id, age })
        })
        .collect::<Result<Vec<ClaudeFile>, String>>()
}

/// 기록의 `updated`·`since` 를 절대시각으로 · 지금 = 0 기준 초(plan §2.2) 라 NOW 를 더한다.
fn clocked(mut record: Record) -> Record {
    for key in ["updated", "since"] {
        let Some(value) = record.get_mut(key) else { continue };
        if let Some(seconds) = value.as_f64() {
            *value = json!(NOW + seconds);
        }
    }
    record
}

/// `scan` 줄 하나를 옮기는 것 · `Store::reload` 를 한 번 돌리고 기대를 견준다.
///
/// input 의 키는 plan §2.3 이 고정한다: `file`(세션 파일 이름, 빠져 있으면 "a"), `record`(세션
/// 파일 JSON 그대로), `transcript`(대화 기록을 만들지, 빠져 있으면 만든다), `claude_sessions`
/// (Claude Code 세션 파일들), `procs`(pid → 생사), `paused`(일시중지 표지를 남길지, 빠져 있으면
/// 만들지 않는다). expect 는 `kept`(목록에 남는지) 이고 남는 줄에는 `state` 도 견준다.
fn scan(case: &Case) -> Result<(), String> {
    let input = case.input.as_object().ok_or("scan 의 input 은 객체여야 한다")?;
    const INPUT_KEYS: [&str; 6] = ["file", "record", "transcript", "claude_sessions", "procs", "paused"];
    for key in input.keys() {
        if !INPUT_KEYS.contains(&key.as_str()) {
            return Err(format!("scan input 에 모르는 키 '{key}' 가 있다"));
        }
    }
    let expect = case.expect.as_object().ok_or("scan 의 expect 는 객체여야 한다")?;
    const EXPECT_KEYS: [&str; 2] = ["kept", "state"];
    for key in expect.keys() {
        if !EXPECT_KEYS.contains(&key.as_str()) {
            return Err(format!("scan expect 에 모르는 키 '{key}' 가 있다"));
        }
    }
    let want_kept = expect
        .get("kept")
        .and_then(Value::as_bool)
        .ok_or("scan expect 의 kept 는 불 값이어야 한다")?;
    let want_state = expect.get("state").and_then(Value::as_str).map(str::to_string);
    // state 는 kept 가 참인 줄에만 온다(plan §2.3)
    if want_kept != want_state.is_some() {
        return Err("kept 가 참이면 state 도 있어야 하고, 거짓이면 state 가 없어야 한다".to_string());
    }

    let record = match input.get("record") {
        Some(Value::Object(record)) => clocked(record.clone()),
        _ => return Err("scan input 의 record 는 객체여야 한다".to_string()),
    };
    let file_name = input
        .get("file")
        .and_then(Value::as_str)
        .filter(|file| !file.is_empty())
        .unwrap_or("a")
        .to_string();
    let write_transcript = input.get("transcript").and_then(Value::as_bool).unwrap_or(true);
    let write_paused = input.get("paused").and_then(Value::as_bool).unwrap_or(false);
    let claude_files = parse_claude_files(input)?;
    let procs = parse_procs(input)?;

    // 세션 id · 기록에 session_id 가 없으면 파일 이름의 .json 을 뗀 것(store::session 과 같다)
    let sid = record
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| file_name.trim_end_matches(".json"))
        .to_string();

    let world = World::new();
    let cwd = record.get("cwd").and_then(Value::as_str).unwrap_or("?").to_string();
    let record_path = world.sessions.join(format!("{file_name}.json"));
    let record_body =
        serde_json::to_vec(&Value::Object(record)).map_err(|error| format!("기록을 JSON 으로 못 만든다: {error}"))?;
    fs::write(&record_path, record_body).map_err(|error| format!("세션 기록 파일을 쓰지 못했다: {error}"))?;
    if write_paused {
        // 일시중지 표지 · 보드가 세션 폴더에 같이 남긴다(store::session 이 읽는 자리)
        fs::write(world.sessions.join(format!("{sid}.paused")), b"")
            .map_err(|error| format!("일시중지 표지를 쓰지 못했다: {error}"))?;
    }
    if write_transcript {
        // 대화 기록 · 자리 계산은 저장소와 같은 transcript_path 로 한다
        let transcript = transcript_path(None, &cwd, &sid, &world.claude);
        let transcript_dir = transcript
            .parent()
            .ok_or("대화 기록 자리가 부모 폴더가 없는 경로다")?
            .to_path_buf();
        fs::create_dir_all(&transcript_dir).map_err(|error| format!("대화 기록 폴더를 만들지 못했다: {error}"))?;
        fs::write(&transcript, b"").map_err(|error| format!("대화 기록을 쓰지 못했다: {error}"))?;
    }
    if !claude_files.is_empty() {
        // Claude Code 세션 파일 · mtime 을 age 만큼 옮겨 최신 순서를 만든다(가장 최근 파일이 이긴다)
        let claude_sessions_dir = world.claude.join("sessions");
        fs::create_dir_all(&claude_sessions_dir).map_err(|error| format!("claude 세션 폴더를 만들지 못했다: {error}"))?;
        for file in &claude_files {
            let claude_path = claude_session_path(&world.claude, file.pid);
            let claude_body = serde_json::to_vec(&json!({"sessionId": file.session_id}))
                .map_err(|error| format!("claude 세션 파일을 만들지 못한다: {error}"))?;
            fs::write(&claude_path, claude_body).map_err(|error| format!("claude 세션 파일을 쓰지 못했다: {error}"))?;
            let mtime = SystemTime::now() - Duration::from_secs_f64(file.age);
            fs::OpenOptions::new()
                .append(true)
                .open(&claude_path)
                .map_err(|error| format!("claude 세션 파일을 다시 열지 못했다: {error}"))?
                .set_times(FileTimes::new().set_modified(mtime))
                .map_err(|error| format!("claude 세션 파일의 mtime 을 못 옮긴다: {error}"))?;
        }
    }

    // 판정 · store::reload 를 한 번 돌린다. 생사는 표의 procs 가 정한다.
    let mut store = Store::new(
        world.sessions.clone(),
        &world.root.join("notifier"),
        world.claude.clone(),
        world.root.to_string_lossy().into_owned(),
    );
    let alive = |record: &Record| -> bool {
        let pid = record
            .get("pid")
            .and_then(Value::as_u64)
            .filter(|&pid| pid <= u64::from(u32::MAX))
            .unwrap_or(0) as u32;
        // 기록의 pid 가 없으면 산 것으로 본다 · sessionId 로 pid 를 찾는 일은 store.rs 쪽이다.
        if pid == 0 {
            return true;
        }
        // 표에 없는 pid 는 없다는 뜻이라(plan §2.3) 죽은 것.
        procs.get(&pid).copied().unwrap_or(false)
    };
    let mut settings = Settings::default();
    let reloaded = store.reload(&mut settings, &Probe::default(), &alive, false, NOW);

    // 기대 몫 · kept: 목록에 남았는지. 남는 줄은 state 도 견준다.
    let Some(session) = reloaded.groups.iter().flat_map(|g| g.sessions.iter()).find(|s| s.id == sid) else {
        if want_kept {
            return Err(format!("세션 {sid} 을 남긴다고 기대했는데 목록에 없다"));
        }
        // 지우는 줄은 목록에서 빠질 뿐 아니라 세션 기록 파일도 지워진다(끝난 세션 정리)
        if record_path.exists() {
            return Err("목록에서는 지워졌지만 세션 기록 파일이 남아 있다".to_string());
        }
        return Ok(());
    };
    if !want_kept {
        return Err(format!("세션 {sid} 을 지운다고 기대했는데 목록에 남아 있다"));
    }
    let want_state = want_state.expect("kept 줄에는 state 가 있다(앞에서 확인했다)");
    if session.state != want_state {
        return Err(format!("상태가 '{}' 인데 '{}' 라고 기대했다", session.state, want_state));
    }
    Ok(())
}