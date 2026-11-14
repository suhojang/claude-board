//! 같은 경로 자리 표의 어댑터 — 기록 저장소의 SQLite 책 두 개와 출발 자리 검사용 탐침.
//! 원본: `Sources/ClaudeAutopilot/main.swift` 1501~1583줄의 `EngineClaimBook`·`EngineClaimProbe`·`claimPathsOf`·`ReadOnlyClaimBook`·`readOnlyRows`.
//!
//! 약속:
//! - `SqliteClaimBook::update` 는 규칙 `rules::multirun::ClaimBook` 대로 한 transaction(`BEGIN IMMEDIATE`)으로 읽고 쓴다.
//!   저장소 열기(없으면 만들고 판을 맞춘다)와 잠김 다시 하기는 `adapters::records_sqlite` 의 `open_store`·`transaction`·`again` 을 같이 쓴다.
//!   넣을 줄은 접은 경로 `key` 가 기본키라(PRIMARY KEY) 같은 key 가 남아 있으면 INSERT 가 실패하고(UNIQUE), 지울 실행은 먼저 지운다.
//! - `ReadOnlyClaimBook` 은 출발 전에 확인할 때 쓴다 — 표를 읽기만 하고 바꿈은 버린다.
//!   파일이 없거나 판이 낮아 표가 없으면 빈 줄이고 실패하지 않는다(원본 `readOnlyRows`).
//! - `EngineClaimProbe` 는 실행 폴더의 `run.json`(state·wrapped·engine_pid·launchd_label), 엔진 pid 의 명령줄,
//!   작업 스케줄러(`ports::Supervisor` — 원본은 LaunchAgent 의 `launchctl print`)로 자리를 잡은 실행의 지금을 본다.
//! - Windows 차이: 명령줄·실행 폴더 견주기는 대소문자와 `\`·`/` 를 가리지 않는다(원본은 POSIX 로 대소문자를 가린다).

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::adapters::records_sqlite::{open_store, to_port, transaction};
use crate::adapters::worktree::canonical_loose;
use crate::ports::{PortResult, Supervisor};
use crate::rules::multirun::{Claim, ClaimBook, ClaimChange, ClaimProbe, ClaimRunView};
use crate::rules::path_key::path_key_windows;
use crate::rules::stopline::stop_num;

/// 읽기 전용 연결이 잠김을 기다리는 시간(원본 `readOnlyClaimsBook` 의 `sqlite3_busy_timeout(db, 2000)`)
const READ_BUSY_TIMEOUT: Duration = Duration::from_millis(2000);

/// 자리 표의 칸 — 원본과 같은 순서다(`SELECT key,path,run_id,run_dir,label,pid,claimed FROM path_claims`)
const CLAIM_COLUMNS_SQL: &str = "SELECT key, path, run_id, run_dir, label, pid, claimed FROM path_claims";
/// 칸 개수 — `CLAIM_COLUMNS_SQL` 의 칸과 같아야 한다
const CLAIM_CELL_COUNT: usize = 7;

/// SQLite 칸을 느슨한 값으로 읽는다 — 수와 글은 그대로, 나머지는 없음
fn cell_value(v: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef as V;
    match v {
        V::Null | V::Blob(_) => Value::Null,
        V::Integer(i) => Value::from(i),
        V::Real(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        V::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
    }
}

/// 원본의 `cells as? String ?? ""` — 글이 아니면 빈 글
fn cell_text(cells: &[Value], i: usize) -> String {
    cells.get(i).and_then(Value::as_str).unwrap_or("").to_string()
}

/// 원본의 `num(cells[i])` — 수 칸과 수 글 칸을 수로 읽는다
fn cell_num(cells: &[Value], i: usize) -> Option<f64> {
    cells.get(i).and_then(|v| stop_num(Some(v)))
}

/// 원본의 `inum(cells[i])` — 소수를 버리고(`Int($0)`) 범위 밖이면 없다(`pid_t(exactly:)`)
fn cell_pid(cells: &[Value], i: usize) -> Option<i32> {
    cell_num(cells, i).map(|n| n.trunc() as i64).and_then(|i| i32::try_from(i).ok())
}

/// 자리 표의 한 줄. 칸 순서는 `CLAIM_COLUMNS_SQL` 과 같다
fn claim_of_row(cells: &[Value]) -> Claim {
    Claim {
        key: cell_text(cells, 0),
        path: cell_text(cells, 1),
        run: cell_text(cells, 2),
        run_dir: cell_text(cells, 3),
        label: cell_text(cells, 4),
        pid: cell_pid(cells, 5),
        claimed: cell_num(cells, 6).unwrap_or(0.0),
    }
}

/// 자리 표에서 줄을 읽는다. 칸 순서는 `CLAIM_COLUMNS_SQL` 과 같다
fn select_claims(conn: &Connection) -> PortResult<Vec<Claim>> {
    let mut st = conn.prepare(CLAIM_COLUMNS_SQL).map_err(to_port)?;
    let rows = st
        .query_map([], |r| {
            let cells: Vec<Value> = (0..CLAIM_CELL_COUNT)
                .map(|i| Ok(cell_value(r.get_ref(i)?)))
                .collect::<rusqlite::Result<_>>()?;
            Ok(claim_of_row(&cells))
        })
        .map_err(to_port)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(to_port)?;
    Ok(rows)
}

/// 바꿈을 쓴다 — 지울 실행을 먼저 지우고 넣을 줄을 넣는다. 원본과 같은 평범한 INSERT
fn write_change(conn: &Connection, c: &ClaimChange) -> PortResult<()> {
    for run in &c.drop {
        conn.execute("DELETE FROM path_claims WHERE run_id = ?", rusqlite::params![run])
            .map(|_| ())
            .map_err(to_port)?;
    }
    for p in &c.put {
        conn.execute(
            "INSERT INTO path_claims(key, path, run_id, run_dir, label, pid, claimed) VALUES(?,?,?,?,?,?,?)",
            rusqlite::params![p.key, p.path, p.run, p.run_dir, p.label, p.pid, r3(p.claimed)],
        )
        .map(|_| ())
        .map_err(to_port)?;
    }
    Ok(())
}

/// 소수 셋째 자리에서 맞춘다. 원본: `r3`(기록 저장소의 시각 칸 길이를 일정하게)
fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// 같은 경로 자리 표의 진짜 어댑터 — 같은 약속: 한 transaction(`BEGIN IMMEDIATE`)으로 읽고 쓰고,
/// 넣을 줄은 접은 경로 `key` 가 기본키라 같은 key 가 남아 있으면 실패하고, 지울 실행은 먼저 지운다.
/// 원본: `EngineClaimBook`(macOS 는 `RecordsDB` 를 열어 `BEGIN IMMEDIATE` transaction 으로 읽고 쓴다)
pub struct SqliteClaimBook {
    pub path: PathBuf,
}

impl ClaimBook for SqliteClaimBook {
    /// 자리 줄을 읽어 body 에 주고 바꿈을 저장한다. 실패 까닭은 SQLite의 까닭 글이다(원본 `DBError` 와 같은 맛)
    fn update(&self, body: &mut dyn FnMut(&[Claim]) -> ClaimChange) -> Result<(), String> {
        let conn = open_store(&self.path).map_err(|e| e.message)?;
        transaction(&conn, || {
            let rows = select_claims(&conn)?;
            let change = body(&rows);
            write_change(&conn, &change)?;
            Ok(())
        })
        .map_err(|e| e.message)
    }
}

/// 자리를 잡은 실행의 지금을 보는 탐침. 원본: `EngineClaimProbe`
pub struct EngineClaimProbe<'a> {
    pub supervisor: &'a dyn Supervisor,
}

impl ClaimProbe for EngineClaimProbe<'_> {
    fn view(&self, c: &Claim) -> ClaimRunView {
        let run = read_run_json(&c.run_dir);
        let mut v = ClaimRunView {
            exists: run.is_some(),
            state: run.as_ref().and_then(|r| r.get("state")).and_then(Value::as_str).map(str::to_string),
            // 원본 `map["wrapped"] != nil` — 칸이 있기만 하면 감은 것(null 이여도)
            wrapped: run.as_ref().is_some_and(|r| r.get("wrapped").is_some()),
            engine_alive: false,
            job_loaded: false,
            starter_alive: false,
        };

        // 엔진 pid — 원본과 같이 소수를 버리고(inum) 범위로 거른다(`pid_t(exactly:)`),
        // 1 이하는 자기 자신을 엔진으로 보지 않게 거른다
        if let Some(n) = run.as_ref().and_then(|r| r.get("engine_pid")).and_then(|v| stop_num(Some(v))) {
            let pid = n.trunc() as i64;
            if pid > 1 && pid <= i64::from(u32::MAX) && claude_board_core::proc::alive(pid as u32, None) {
                // 명령줄로도 보아 이 실행 폴더가 명령줄에 있는지 — pid 를 훔친 다른 실행을 안 섞는다
                v.engine_alive = command_contains(pid as u32, &c.run_dir);
            }
        }

        // 출발 명령 — 원본은 `cmd.contains(" start ")` 였으나 인자가 진짜 `start` 인지로 본다.
        // 살아 있고 명령줄 인자에 `start` 와 실행 폴더 이름이 함께 있으면 아직 도는 출발이다
        if let Some(p) = c.pid {
            if p > 1 && claude_board_core::proc::alive(p as u32, None) {
                let args = claude_board_core::proc::command_line(p as u32).unwrap_or_default();
                let run_name = Path::new(&c.run_dir)
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                v.starter_alive = args.iter().any(|arg| arg == "start") && args.iter().any(|arg| contains_fold(arg, &run_name));
            }
        }

        // 원본 `launchd_label ?? c.label`
        let label = run
            .as_ref()
            .and_then(|r| r.get("launchd_label"))
            .and_then(Value::as_str)
            .map_or_else(|| c.label.clone(), str::to_string);
        if label.starts_with("com.claude.board") && label.contains(".autopilot.") {
            // 원본 `launchdLoaded` — 실렸는지를 못 알면 실리지 않은 것으로 본다
            v.job_loaded = self.supervisor.is_registered(&label).unwrap_or(false);
        }
        v
    }
}

/// 실행 폴더의 run.json — 없거나 유효한 객체가 아니면 없다. 원본: `readJSON(run_dir.appendingPathComponent("run.json"))`
fn read_run_json(run_dir: &str) -> Option<serde_json::Map<String, Value>> {
    let text = std::fs::read_to_string(Path::new(run_dir).join("run.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.as_object().cloned()
}

/// pid 의 명령줄 인자 가운데 `needle`(실행 폴더)이 있나 — 대소문자와 `\`·`/` 를 가리지 않는다(Windows 경로 꼴 차이)
fn command_contains(pid: u32, run_dir: &str) -> bool {
    let needle = fold_path(run_dir);
    claude_board_core::proc::command_line(pid).is_some_and(|args| args.iter().any(|arg| fold_path(arg).contains(&needle)))
}

/// 같은 글인가를 대소문자 가리지 않고 본다
fn contains_fold(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

/// 경로 견주기를 위한 접기 — `\` 를 `/` 로, 대소문자를 다 낮춘다
fn fold_path(s: &str) -> String {
    s.replace('\\', "/").to_lowercase()
}

/// 실행이 잡을 같은 경로 자리 — 고정 설정의 저장소 각각과, 저장소 밖 작업 폴더는 그 폴더. 실제 경로로 접어 돌려준다. 원본: `claimPathsOf`
pub fn claim_paths_of(frozen: &Value) -> Vec<String> {
    let repos = frozen
        .get("repos")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |list| list.iter().filter_map(Value::as_str).map(canonical_loose).collect());
    let cwds = frozen
        .get("units")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |list| {
            list.iter().filter_map(|u| u.get("cwd").and_then(Value::as_str)).map(canonical_loose).collect()
        });
    // 저장소 안에 들어가는 작업 폴더는 그 저장소가 이미 잡은 자리다.
    // 자리 표의 견주기는 접은 경로 키(대소문자·`\`/`/` 가리지 않음)로 하므로 여기서도 같은 접기로 견준다(원본은 POSIX 로 그냥 견준다)
    let repo_keys: Vec<String> = repos.iter().map(|repo| path_key_windows(repo)).collect();
    let mut places = repos.clone();
    places.extend(cwds.into_iter().filter(|cwd| {
        let cwd_key = path_key_windows(cwd);
        !repo_keys.iter().any(|repo_key| cwd_key == *repo_key || cwd_key.starts_with(&format!("{repo_key}/")))
    }));
    places
}

/// 읽기 전용 책 — 출발 전에 확인할 때 쓴다. 줄을 보고 바꿈은 버리고, 실패하지 않는다. 원본: `ReadOnlyClaimBook`
pub struct ReadOnlyClaimBook {
    pub path: PathBuf,
}

impl ClaimBook for ReadOnlyClaimBook {
    fn update(&self, body: &mut dyn FnMut(&[Claim]) -> ClaimChange) -> Result<(), String> {
        // 바꿈은 버린다 — 읽기 연결이므로 어차피 쓸 수도 없다
        body(&read_only_claims(&self.path));
        Ok(())
    }
}

fn read_only_claims(path: &Path) -> Vec<Claim> {
    read_only_rows(path, CLAIM_COLUMNS_SQL, &[]).iter().map(|cells| claim_of_row(cells)).collect()
}

/// 표를 읽기만 하는 질의 — 파일이 없거나 표가 없으면(판이 낮으면) 빈 목록. 원본: `readOnlyRows`
fn read_only_rows(path: &Path, sql: &str, args: &[&str]) -> Vec<Vec<Value>> {
    let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return Vec::new();
    };
    let _ = conn.busy_timeout(READ_BUSY_TIMEOUT); // 시간을 정하는 것 말고 실패할 자리가 없어 원본도 무시한다
    let mut st = match conn.prepare(sql) {
        Ok(st) => st,
        Err(_) => return Vec::new(), // 표가 없으면 빈 목록
    };
    let cell_count = st.column_count();
    let mut rows = match st.query(rusqlite::params_from_iter(args)) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<Vec<Value>> = Vec::new();
    // 원본의 `while sqlite3_step(st) == SQLITE_ROW` — 걸음이 실패하면 거기까지 읽은 만큼만
    while let Ok(Some(row)) = rows.next() {
        let cells: Vec<Value> = (0..cell_count).map(|i| row.get_ref(i).map_or(Value::Null, cell_value)).collect();
        out.push(cells);
    }
    out
}

