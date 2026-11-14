//! 실행 기록 저장소의 `projects.root`(UNIQUE) — 같은 폴더에서 실행을 다시 돌려도 기록이 저장된다.
//! 가짜 포트(`tests/fakes`)로 엔진을 돌리고 기록 저장소는 진짜 SQLite 어댑터를 쓴다. 진짜 claude·셸·git 은 없다.
//! 계약(원본 `upsertProject`): 프로젝트 id 는 run.json 의 `project` 글이다. root 는 먼저 쓴 프로젝트가 갖고,
//! 같은 폴더를 다른 프로젝트가 또 적으면 그 프로젝트의 root 는 이전 값(없으면 null)으로 둔다 — 쓰기는 실패하지 않는다.

mod fakes;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use claude_board_autopilot::adapters::records_sqlite::SqliteRecords;
use claude_board_autopilot::engine::decide::RunState;
use claude_board_autopilot::engine::run::RunExit;
use claude_board_autopilot::ports::{PortResult, Records, Row};
use fakes::*;
use serde_json::{json, Value};

/// 단위 하나짜리 실행. 실행 id 는 실행 폴더 이름이다
fn world(run_id: &str, project: &str, root: &str) -> World {
    let mut s = Scenario::new(1);
    s.run_id = run_id.to_string();
    s.run["id"] = json!(run_id);
    s.frozen["id"] = json!(run_id);
    s.run["project"] = json!(project);
    s.run["project_root"] = json!(root);
    let w = s.build();
    w.env.push_spawn(ChildScript::ok_turn(0.5));
    w
}

fn finish(w: &World, db: &dyn Records) {
    assert_eq!(w.run_with(db), RunExit::Finished(RunState::Done));
    let bad: Vec<String> = w.event_texts().into_iter().filter(|t| t.contains("쓰지 못했습니다")).collect();
    assert!(bad.is_empty(), "기록 저장소 쓰기 실패 알림이 없어야 한다: {bad:?}");
}

fn open(dir: &Path) -> SqliteRecords {
    SqliteRecords::open(&dir.join("records.sqlite")).expect("저장소 열기")
}

fn projects(db: &dyn Records) -> Vec<Row> {
    db.select("SELECT id, root, created FROM projects ORDER BY id", &[]).unwrap()
}

fn runs(db: &dyn Records) -> Vec<String> {
    db.select("SELECT id FROM runs ORDER BY id", &[]).unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect()
}

fn root_of(db: &dyn Records, id: &str) -> Value {
    db.select("SELECT root FROM projects WHERE id = ?", &[json!(id)]).unwrap()[0]["root"].clone()
}

/// 폴더 하나를 두 프로젝트 이름이 쓴다 — 실제 실행에서 15번 난 실패(`UNIQUE constraint failed: projects.root`)
#[test]
fn same_root_two_project_ids_both_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    finish(&world("long12", "long12", r"C:\repo"), &db);
    finish(&world("long12b", "long12b", r"C:\repo"), &db);
    assert_eq!(runs(&db), ["long12", "long12b"], "두 실행의 runs 행이 모두 있다");
    let p = projects(&db);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0]["root"], r"C:\repo", "먼저 쓴 프로젝트가 root 를 지킨다");
    assert_eq!(p[1]["root"], Value::Null, "나중 프로젝트는 root 를 뺏지 않는다");
}

/// 같은 프로젝트로 같은 폴더에서 다시 돌리면 행 하나가 갱신된다. root·created 는 그대로
#[test]
fn same_project_same_root_second_run_updates_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    finish(&world("r1", "toy", r"C:\repo"), &db);
    let created = projects(&db)[0]["created"].clone();
    finish(&world("r2", "toy", r"C:\repo"), &db);
    let p = projects(&db);
    assert_eq!(p.len(), 1);
    assert_eq!((&p[0]["id"], &p[0]["root"], &p[0]["created"]), (&json!("toy"), &json!(r"C:\repo"), &created));
    assert_eq!(runs(&db), ["r1", "r2"]);
}

/// 폴더가 다르면 이름이 같아도 행 하나이고 root 는 최근 값으로 옮겨 간다. 이름이 다르면 각자 root 를 갖는다
#[test]
fn different_roots_keep_their_own_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    finish(&world("r1", "a", r"C:\one"), &db);
    finish(&world("r2", "b", r"C:\two"), &db);
    assert_eq!((root_of(&db, "a"), root_of(&db, "b")), (json!(r"C:\one"), json!(r"C:\two")));
    finish(&world("r3", "a", r"C:\three"), &db);
    assert_eq!(projects(&db).len(), 2);
    assert_eq!(root_of(&db, "a"), json!(r"C:\three"));
}

/// 같은 폴더를 가리키는 다른 글(대소문자·`\\?\`·`/`·끝 `\`)도 같은 자리로 본다
#[test]
fn root_spellings_are_one_folder() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    finish(&world("r0", "first", r"C:\Work\Repo"), &db);
    for (i, spelling) in [r"c:\work\repo", r"C:\Work\Repo\", r"\\?\C:\Work\Repo", "C:/Work/Repo", r"C:\WORK\REPO"].iter().enumerate() {
        let id = format!("p{i}");
        finish(&world(&format!("r{}", i + 1), &id, spelling), &db);
        assert_eq!(root_of(&db, &id), Value::Null, "{spelling}");
    }
    assert_eq!(root_of(&db, "first"), json!(r"C:\Work\Repo"));
    assert_eq!(runs(&db).len(), 6);
}

/// 옛 코드가 쓴 저장소(root 글이 정리 안 된 행)가 있어도 새 실행이 쓰이고, 옛 행은 그대로다
#[test]
fn old_store_row_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    db.upsert(
        "projects",
        &[("id", json!("long12")), ("name", json!("long12")), ("root", json!(r"c:\users\me\repo\")), ("kind", json!("single")), ("repos", json!("[]")), ("created", json!(5.0)), ("updated", json!(5.0))]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
    .unwrap();
    finish(&world("long12b", "long12b", r"C:\Users\me\repo"), &db);
    let p = projects(&db);
    assert_eq!(p.len(), 2);
    assert_eq!((&p[0]["id"], &p[0]["root"], p[0]["created"].as_f64()), (&json!("long12"), &json!(r"c:\users\me\repo\"), Some(5.0)));
    assert_eq!(p[1]["root"], Value::Null);
    assert_eq!(runs(&db), ["long12b"]);
}

/// 읽은 뒤 쓰기 전에 다른 엔진이 같은 폴더를 잡은 경우(진짜 UNIQUE 오류) 한 번 다시 써서 기록이 남는다
struct Rival<'a> {
    inner: &'a SqliteRecords,
    armed: AtomicBool,
}

impl Records for Rival<'_> {
    fn version(&self) -> PortResult<i32> {
        self.inner.version()
    }
    fn upsert(&self, table: &str, row: &Row) -> PortResult<()> {
        if table == "projects" && self.armed.swap(false, Ordering::SeqCst) {
            let rival: Row = [("id", json!("rival")), ("name", json!("rival")), ("root", row["root"].clone())].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            self.inner.upsert("projects", &rival).unwrap();
        }
        self.inner.upsert(table, row)
    }
    fn select(&self, sql: &str, args: &[Value]) -> PortResult<Vec<Row>> {
        self.inner.select(sql, args)
    }
}

#[test]
fn lost_race_retries_without_root() {
    let dir = tempfile::tempdir().unwrap();
    let inner = open(dir.path());
    let db = Rival { inner: &inner, armed: AtomicBool::new(true) };
    finish(&world("r1", "mine", r"C:\repo"), &db);
    assert_eq!(root_of(&inner, "rival"), json!(r"C:\repo"));
    assert_eq!(root_of(&inner, "mine"), Value::Null);
    assert_eq!(runs(&inner), ["r1"]);
}

/// 엔진 둘이 같은 저장소 파일에 같은 폴더로 동시에 쓴다
#[test]
fn concurrent_engines_same_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    std::thread::scope(|s| {
        for i in 0..4 {
            let path = path.clone();
            s.spawn(move || {
                let db = open(&path);
                finish(&world(&format!("c{i}"), &format!("proj{i}"), r"C:\shared"), &db);
            });
        }
    });
    let db = open(&path);
    assert_eq!(runs(&db), ["c0", "c1", "c2", "c3"]);
    let p = projects(&db);
    assert_eq!(p.len(), 4);
    assert_eq!(p.iter().filter(|r| r["root"] == json!(r"C:\shared")).count(), 1, "root 를 가진 프로젝트는 하나뿐");
}

