//! 같은 경로 자리 어댑터(`engine::claims`) 시험 — 두 책이 기록 저장소의 자리 표를 읽고 쓰고 경쟁하는 것,
//! 읽기 전용 책의 버림, 자리를 잡은 실행의 지금을 보는 탐침, 고정 설정에서 잡을 자리 고르기.
//! 원본: `Sources/ClaudeAutopilot/main.swift` 1501~1583줄.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use claude_board_autopilot::adapters::records_sqlite::SqliteRecords;
use claude_board_autopilot::engine::claims::{claim_paths_of, EngineClaimProbe, ReadOnlyClaimBook, SqliteClaimBook};
use claude_board_autopilot::ports::{KeepAlive, PortError, PortErrorKind, PortResult, Records, Row, Supervisor};
use claude_board_autopilot::rules::multirun::{Claim, ClaimBook, ClaimChange, ClaimProbe, ClaimRunView};
use rusqlite::Connection;
use serde_json::{json, Value};

// ── 도구 ────────────────────────────────────────────────

fn row(v: Value) -> Row {
    v.as_object().unwrap().clone()
}

/// 판 5 저장소를 만들어 열어 둔다(상위 폴더까지 만드는가는 `tests/records_sqlite.rs` 가 검사한다)
fn temp_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.sqlite");
    SqliteRecords::open(&path).unwrap();
    (dir, path)
}

fn seed(path: &Path, body: Value) {
    let store = SqliteRecords::open(path).unwrap();
    store.upsert("path_claims", &row(body)).unwrap();
}

/// 책을 거치지 않고 직접 읽는다(key 순) — key·run_id·run_dir·pid·claimed
type ClaimRow = (String, String, String, Option<i64>, f64);

fn rows_direct(path: &Path) -> Vec<ClaimRow> {
    let db = Connection::open(path).unwrap();
    let mut st = db.prepare("SELECT key, run_id, run_dir, pid, claimed FROM path_claims ORDER BY key").unwrap();
    let rows = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn claim(key: &str, run: &str, run_dir: &str) -> Claim {
    Claim {
        key: key.to_string(),
        path: key.to_string(),
        run: run.to_string(),
        run_dir: run_dir.to_string(),
        label: format!("com.claude.board.dev.autopilot.{run}"),
        pid: None,
        claimed: 1234.5678,
    }
}

// ── 책: 읽고 지우고 넣는다 ──────────────────────────────

#[test]
fn 자리_표를_읽고_지우고_넣는다() {
    let (_dir, path) = temp_store();
    seed(&path, json!({ "key": "c:/a", "path": "C:/a", "run_id": "r0", "run_dir": "C:/r/r0", "label": "lbl", "pid": 4242, "claimed": 1.5 }));

    let book = SqliteClaimBook { path: path.clone() };
    let mut seen = Vec::new();
    book.update(&mut |rows| {
        seen = rows.to_vec();
        ClaimChange {
            put: vec![Claim {
                key: "c:/b".to_string(),
                path: "C:/b".to_string(),
                run: "r1".to_string(),
                run_dir: "C:/r/r1".to_string(),
                label: "com.claude.board.dev.autopilot.r1".to_string(),
                pid: Some(7),
                claimed: 1.23456,
            }],
            drop: vec!["r0".to_string()],
        }
    })
    .expect("같은 key 가 남아 있지 않으므로 이긴다");

    // 읽은 줄의 칸이 원래 심은 대로다(pid 도 시각도)
    assert_eq!(
        seen,
        vec![Claim {
            key: "c:/a".to_string(),
            path: "C:/a".to_string(),
            run: "r0".to_string(),
            run_dir: "C:/r/r0".to_string(),
            label: "lbl".to_string(),
            pid: Some(4242),
            claimed: 1.5,
        }]
    );
    // 지운 줄은 없고 넣은 줄이 심었다(pid 없음, 시각은 셋째 자리에서 맞춤: 1.23456 → 1.235)
    let got = rows_direct(&path);
    assert_eq!(got.len(), 1);
    let r = &got[0];
    assert_eq!((r.0.as_str(), r.1.as_str(), r.2.as_str()), ("c:/b", "r1", "C:/r/r1"));
    assert_eq!(r.3, Some(7));
    assert!((r.4 - 1.235).abs() < f64::EPSILON * 1.235, "셋째 자리 맞추기: {}", r.4);
}

// ── 책: 둘이 한 자리를 둘 때 ────────────────────────────

#[test]
fn 두_책이_같은_key를_두고_다툰다() {
    let (_dir, path) = temp_store();
    // c:/a 자리는 r0 가 잡아 두었고 c:/x 는 rx 가 따로 잡았다
    seed(&path, json!({ "key": "c:/a", "path": "C:/a", "run_id": "r0", "run_dir": "C:/r/r0", "label": "lbl", "pid": null, "claimed": 0.0 }));
    seed(&path, json!({ "key": "c:/x", "path": "C:/x", "run_id": "rx", "run_dir": "C:/r/rx", "label": "lbl", "pid": null, "claimed": 0.0 }));

    // 첫 책은 r0 를 지우고 같은 자리를 다시 잡는다 — 이긴다
    let first = SqliteClaimBook { path: path.clone() };
    first
        .update(&mut |_| ClaimChange { put: vec![claim("c:/a", "r1", "C:/r/r1")], drop: vec!["r0".to_string()] })
        .expect("첫 책은 자리를 다시 잡는다");

    // 같은 key 가 이미 남아 있는 채로 또 넣으면 실패하고 — 그 바꿈(지움까지)은 통째로 물려 돌아온다
    let second = SqliteClaimBook { path: path.clone() };
    let err = second
        .update(&mut |_| ClaimChange { put: vec![claim("c:/a", "r2", "C:/r/r2")], drop: vec!["rx".to_string()] })
        .unwrap_err();
    assert!(err.contains("UNIQUE"), "까닭은 SQLite의 까닭 글이다: {err}");
    // 실패한 책의 지움도 돌아왔다 — rx 가 남아 있다
    let got = rows_direct(&path);
    assert_eq!(got.len(), 2, "실패한 책이 다시 쓸 자리는 남지 않았다: {got:?}");
    assert_eq!(got[0].1, "r1");
    assert_eq!(got[1].1, "rx");
}

#[test]
fn 두_책이_같은_때에_한_자리를_잡으면_한쪽만_이긴다() {
    let (_dir, path) = temp_store();
    let store = std::sync::Arc::new(path);
    let results: Vec<bool> = std::thread::scope(|s| {
        let handles = ["r1", "r2"]
            .map(|run| {
                let p = store.clone();
                s.spawn(move || {
                    let b = SqliteClaimBook { path: p.to_path_buf() };
                    b.update(&mut |_| {
                        ClaimChange { put: vec![claim("c:/a", run, &format!("C:/r/{run}"))], drop: vec![] }
                    })
                    .is_ok()
                })
            })
            .into_iter();
        handles.map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|&&ok| ok).count(), 1, "한쪽만 이긴다: {results:?}");
    assert_eq!(rows_direct(&store).len(), 1, "이긴 책의 줄만 남는다");
}

// ── 책: 읽기 전용 ────────────────────────────────────────

#[test]
fn 읽기_전용_책은_줄을_보고_바꿈은_버린다() {
    let (_dir, path) = temp_store();
    seed(&path, json!({ "key": "c:/a", "path": "C:/a", "run_id": "r0", "run_dir": "C:/r/r0", "label": "lbl", "pid": null, "claimed": 9.5 }));

    let book = ReadOnlyClaimBook { path: path.clone() };
    let mut seen = 0;
    book.update(&mut |rows| {
        seen = rows.len();
        ClaimChange { put: vec![claim("c:/b", "r1", "C:/r/r1")], drop: vec!["r0".to_string()] }
    })
    .expect("읽기 전용 책은 실패하지 않는다");
    assert_eq!(seen, 1);
    // 바꿈은 버렸다 — 지우지도 넣지도 않았다
    let got = rows_direct(&path);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, "r0");
}

#[test]
fn 읽기_전용_책은_파일이_없거나_표가_없으면_빈_줄이다() {
    let dir = tempfile::tempdir().unwrap();
    // 파일이 없다
    let book = ReadOnlyClaimBook { path: dir.path().join("없는.sqlite") };
    let mut seen = usize::MAX;
    book.update(&mut |rows| {
        seen = rows.len();
        ClaimChange { put: vec![], drop: vec![] }
    })
    .expect("파일이 없어도 실패하지 않는다");
    assert_eq!(seen, 0);
    // 파일이 있지만 표가 없다(빈 파일 — 판 0)
    let empty = dir.path().join("빈.sqlite");
    std::fs::write(&empty, b"").unwrap();
    let book = ReadOnlyClaimBook { path: empty };
    let mut seen = usize::MAX;
    book.update(&mut |rows| {
        seen = rows.len();
        ClaimChange { put: vec![], drop: vec![] }
    })
    .expect("표가 없어도 실패하지 않는다");
    assert_eq!(seen, 0);
    dir.close().expect("연결이 닫혔으므로 지운다");
}

// ── 탐침: 실행 폴더와 살아 있음 ──────────────────────────

fn write_run_json(dir: &Path, body: Value) {
    std::fs::write(dir.join("run.json"), serde_json::to_string(&body).unwrap()).unwrap();
}

/// 묶은 label 을 기록하는 감독 가짜 — 실리지 않은 것을 보려고
struct FakeSupervisor {
    registered: Mutex<HashSet<String>>,
    asked: Mutex<Vec<String>>,
    fail: bool,
}

impl Default for FakeSupervisor {
    fn default() -> Self {
        Self { registered: Mutex::new(HashSet::new()), asked: Mutex::new(Vec::new()), fail: false }
    }
}

impl Supervisor for FakeSupervisor {
    fn register(&self, _job: &KeepAlive) -> PortResult<()> {
        Ok(())
    }
    fn unregister(&self, _label: &str) -> PortResult<()> {
        Ok(())
    }
    fn is_registered(&self, label: &str) -> PortResult<bool> {
        self.asked.lock().unwrap().push(label.to_string());
        if self.fail {
            return Err(PortError::new(PortErrorKind::Unavailable, "감독이 망가졌다"));
        }
        Ok(self.registered.lock().unwrap().contains(label))
    }
    fn list(&self) -> PortResult<Vec<String>> {
        Ok(self.registered.lock().unwrap().iter().cloned().collect())
    }
}

fn probe(run_dir: &str, sup: &dyn Supervisor, pid: Option<i32>) -> ClaimRunView {
    let viewer = EngineClaimProbe { supervisor: sup };
    viewer.view(&Claim { pid, ..claim("c:/a", "r1", run_dir) })
}

/// 시험 끝에 자식을 반드시 끝마친다 — 도는 cmd 가 남지 않게(wait() 로 끝남을 확인하고 갑니다)
struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 엔진 자식 — 명령줄에 실행 폴더가 앉는 cmd(`cmd /K cd 실행폴더`)
fn spawn_engine_child(run_dir: &str) -> (ChildGuard, u32) {
    let child = std::process::Command::new("cmd").args(["/K", "cd", run_dir]).spawn().unwrap();
    let pid = child.id();
    (ChildGuard(child), pid)
}

/// 출발 자식 — 명령줄에 `start` 인자와 실행 폴더 이름이 함께 앉는 cmd
fn spawn_starter_child(run_dir: &str) -> (ChildGuard, u32) {
    let child = std::process::Command::new("cmd")
        .args(["/K", "cd", run_dir, "&", "echo", "start"])
        .spawn()
        .unwrap();
    let pid = child.id();
    (ChildGuard(child), pid)
}

fn wait_command_line(pid: u32) {
    for _ in 0..40 {
        if claude_board_core::proc::command_line(pid).is_some_and(|args| !args.is_empty()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("자식 {pid} 의 명령줄을 2 초에도 읽지 못했습니다");
}

#[test]
fn 엔진_pid가_살아_있고_명령줄에_실행_폴더가_있으면_산_엔진이다() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().to_string_lossy().to_string();
    let (child, child_pid) = spawn_engine_child(&run_dir);
    wait_command_line(child_pid);
    let sup = FakeSupervisor::default();

    write_run_json(dir.path(), json!({ "state": "running", "engine_pid": child_pid }));
    let v = probe(&run_dir, &sup, None);
    assert!(v.exists);
    assert_eq!(v.state.as_deref(), Some("running"));
    assert!(!v.wrapped);
    assert!(v.engine_alive, "자식 cmd 의 명령줄에 실행 폴더가 앉아 있다");

    drop(child);
    let v = probe(&run_dir, &sup, None);
    assert!(!v.engine_alive, "끝낸 엔진이다");
}

#[test]
fn 출발_명령이_살아_있으면_산_출발이다() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().to_string_lossy().to_string();
    let (child, child_pid) = spawn_starter_child(&run_dir);
    wait_command_line(child_pid);
    let sup = FakeSupervisor::default();

    let v = probe(&run_dir, &sup, Some(child_pid as i32));
    assert!(v.starter_alive, "start 인자와 실행 폴더 이름이 명령줄에 함께 앉아 있다");

    // start 인자가 없는 프로세스(시험 몸체 자신)는 출발이 아니다
    let v = probe(&run_dir, &sup, Some(std::process::id() as i32));
    assert!(!v.starter_alive);

    drop(child);
    let v = probe(&run_dir, &sup, Some(child_pid as i32));
    assert!(!v.starter_alive, "끝낸 출발이다");

    // pid 도 없으면 볼 수 없다
    let v = probe(&run_dir, &sup, None);
    assert!(!v.starter_alive);
}

#[test]
fn 엔진_pid가_거짓이거나_범위를_벗어나면_엔진은_죽은_것이다() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().to_string_lossy().to_string();
    let sup = FakeSupervisor::default();
    for wrong in [json!(0), json!(1), json!("아니오"), json!(99_999_999_999i64)] {
        write_run_json(dir.path(), json!({ "engine_pid": wrong }));
        let v = probe(&run_dir, &sup, None);
        assert!(!v.engine_alive, "{wrong} 는 엔진이 아니다");
    }
}

#[test]
fn run_json이_없으면_전부_거짓이다() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().to_string_lossy().to_string();
    let sup = FakeSupervisor::default();
    let v = probe(&run_dir, &sup, None);
    assert!(!v.exists);
    assert_eq!(v.state, None);
    assert!(!v.wrapped && !v.engine_alive && !v.job_loaded && !v.starter_alive);

    // run.json 이 있으면 state 와 끝 정리를 본다(null 이여도 감은 것이다)
    write_run_json(dir.path(), json!({ "state": "wrapping", "wrapped": null }));
    let v = probe(&run_dir, &sup, None);
    assert!(v.exists);
    assert_eq!(v.state.as_deref(), Some("wrapping"));
    assert!(v.wrapped);
    assert!(!v.engine_alive && !v.job_loaded && !v.starter_alive);
}

#[test]
fn 작업이_실렸는지는_감독에게_묻는다() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().to_string_lossy().to_string();

    // run.json 의 label 이 실렸는지를 묻는다 — 자리 줄의 label 이 아니다
    write_run_json(dir.path(), json!({ "launchd_label": "com.claude.board.dev.autopilot.r9" }));
    let sup = FakeSupervisor::default();
    sup.registered.lock().unwrap().insert("com.claude.board.dev.autopilot.r9".to_string());
    let v = probe(&run_dir, &sup, None);
    assert!(v.job_loaded);
    assert_eq!(*sup.asked.lock().unwrap(), vec!["com.claude.board.dev.autopilot.r9".to_string()]);

    // run.json 에 label 이 없으면 자리 줄의 label 을 묻는다
    write_run_json(dir.path(), json!({}));
    let sup = FakeSupervisor::default();
    sup.registered.lock().unwrap().insert("com.claude.board.dev.autopilot.r1".to_string());
    let v = probe(&run_dir, &sup, None);
    assert!(v.job_loaded);
    assert_eq!(*sup.asked.lock().unwrap(), vec!["com.claude.board.dev.autopilot.r1".to_string()]);

    // 틀린 접두·틀린 둘째 마디면 묻지도 않는다
    for wrong in ["com.other.autopilot.x", "com.claude.board.backup.y"] {
        write_run_json(dir.path(), json!({ "launchd_label": wrong }));
        let sup = FakeSupervisor::default();
        let v = probe(&run_dir, &sup, None);
        assert!(!v.job_loaded, "{wrong} 는 보드의 작업이 아니다");
        assert!(sup.asked.lock().unwrap().is_empty());
    }

    // 감독이 망가져도 실리지 않은 것으로 본다
    write_run_json(dir.path(), json!({ "launchd_label": "com.claude.board.dev.autopilot.r1" }));
    let broken = FakeSupervisor { fail: true, ..Default::default() };
    let v = probe(&run_dir, &broken, None);
    assert!(!v.job_loaded);
}

// ── 고정 설정에서 잡을 자리 고르기 ───────────────────────

#[test]
fn 고정_설정에서_잡을_자리를_고른다() {
    // 저장소 각각과, 저장소 밖 작업 폴더는 그 폴더
    let frozen = json!({
        "repos": ["/w/repo", "/w/repo/sub"],
        "units": [ { "cwd": "/w/repo" }, { "cwd": "/w/repo/unit" }, { "cwd": "/w/other" }, { "cwd": 5 }, {} ],
    });
    assert_eq!(
        claim_paths_of(&frozen),
        vec!["/w/repo".to_string(), "/w/repo/sub".to_string(), "/w/other".to_string()]
    );

    // Windows 꼴 경로도 접어 겨룬다(구분자·대소문자 차이)
    let frozen = json!({ "repos": ["C:\\work\\repo"], "units": [ { "cwd": "c:/work/repo/unit" } ] });
    assert_eq!(claim_paths_of(&frozen), vec!["C:/work/repo".to_string()]);

    // `..` 은 접는다
    let frozen = json!({ "units": [ { "cwd": "C:\\w\\a\\..\\b" } ] });
    assert_eq!(claim_paths_of(&frozen), vec!["C:/w/b".to_string()]);

    // 잡을 자리가 없거나 잡을 수 없는 낱개면 빈 목록이다
    assert_eq!(claim_paths_of(&json!({})), Vec::<String>::new());
    assert_eq!(claim_paths_of(&json!({ "repos": 4, "units": "없음" })), Vec::<String>::new());
}
