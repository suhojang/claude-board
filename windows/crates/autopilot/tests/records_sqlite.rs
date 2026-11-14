//! 실행 기록 저장소 어댑터(SQLite) 시험: 열기·판 올리기·upsert·select·동시 쓰기.
use std::path::Path;

use claude_board_autopilot::adapters::records_sqlite::SqliteRecords;
use claude_board_autopilot::ports::{PortErrorKind, Records, Row};
use claude_board_autopilot::rules::records::*;
use rusqlite::Connection;
use serde_json::{json, Value};

fn row(v: Value) -> Row {
    v.as_object().unwrap().clone()
}

fn table_names(path: &Path) -> Vec<String> {
    let db = Connection::open(path).unwrap();
    let mut st = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    st.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect()
}

fn user_version(path: &Path) -> i32 {
    Connection::open(path).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

/// 옛 판 저장소를 원본 DDL 상수로 만든다
fn old_store(path: &Path, upto: i32, extra: &str) {
    let db = Connection::open(path).unwrap();
    for m in RECORDS_MIGRATIONS.iter().filter(|m| m.version <= upto) {
        db.execute_batch(m.sql).unwrap();
    }
    db.execute_batch(&format!("PRAGMA user_version={upto}")).unwrap();
    if !extra.is_empty() {
        db.execute_batch(extra).unwrap();
    }
}

#[test]
fn 빈_저장소는_판_5와_표_열세_개를_갖고_상위_폴더도_만든다() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("deep").join("records.sqlite");
    let db = SqliteRecords::open(&path).unwrap();
    assert_eq!(db.version().unwrap(), 5);
    drop(db);
    assert_eq!(user_version(&path), RECORDS_VERSION);
    let mut want: Vec<String> = RECORDS_TABLES.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(table_names(&path).len(), 13);
    assert_eq!(table_names(&path), want);
    let mode: String = Connection::open(&path).unwrap().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn 다시_열어도_그대로다() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.sqlite");
    let db = SqliteRecords::open(&path).unwrap();
    db.upsert("projects", &row(json!({"id": "p1", "name": "n"}))).unwrap();
    drop(db);
    let db = SqliteRecords::open(&path).unwrap();
    assert_eq!(db.version().unwrap(), 5);
    let rows = db.select("SELECT id, name FROM projects", &[]).unwrap();
    assert_eq!(rows, vec![row(json!({"id": "p1", "name": "n"}))]);
    assert_eq!(table_names(&path).len(), 13);
}

#[test]
fn 판1_2_3_4_저장소를_열면_판5로_올라가고_행은_그대로다() {
    for from in 1..=4 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("records.sqlite");
        old_store(&path, from, "INSERT INTO projects(id,name,root) VALUES('p','p','/w/p'); INSERT INTO runs(id,project_id,state) VALUES('r1','p','done')");
        let db = SqliteRecords::open(&path).unwrap();
        assert_eq!(db.version().unwrap(), 5, "판 {from} 에서");
        let rows = db.select("SELECT state FROM runs WHERE id=?", &[json!("r1")]).unwrap();
        assert_eq!(rows, vec![row(json!({"state": "done"}))]);
        // 판 4 칸과 판 5 표가 생겼다
        db.upsert("queue_items", &row(json!({"project_id": "p", "source_key": "k", "digest": "d", "def": "{}"}))).unwrap();
        assert_eq!(table_names(&path).len(), 13);
    }
}

#[test]
fn 더_새로운_판_저장소는_거절하고_건드리지_않는다() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.sqlite");
    old_store(&path, 5, "PRAGMA user_version=6");
    let err = SqliteRecords::open(&path).err().expect("거절해야 한다");
    assert_eq!(err.kind, PortErrorKind::Unavailable);
    assert_eq!(user_version(&path), 6);
}

#[test]
fn upsert_는_넣고_같은_키면_덮어쓴다() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    db.upsert("units", &row(json!({"run_id": "r1", "n": 1, "title": "a", "tries": 1, "cost_usd": 0.5, "merged": null}))).unwrap();
    db.upsert("units", &row(json!({"run_id": "r1", "n": 1, "title": "b", "tries": 2}))).unwrap();
    db.upsert("units", &row(json!({"run_id": "r1", "n": 2, "title": "c"}))).unwrap();
    let rows = db.select("SELECT n, title, tries, cost_usd FROM units ORDER BY n", &[]).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], row(json!({"n": 1, "title": "b", "tries": 2, "cost_usd": 0.5})));
    assert_eq!(rows[1], row(json!({"n": 2, "title": "c", "tries": null, "cost_usd": null})));
}

#[test]
fn upsert_값_변환_불리언_객체_배열_실수() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    db.upsert(
        "runs",
        &row(json!({"id": "r", "dev": true, "branches": ["a", "b"], "harness": {"k": 1}, "cost_usd": 1.25, "units_total": 3, "title": "한글"})),
    )
    .unwrap();
    let r = &db.select("SELECT dev, branches, harness, cost_usd, units_total, title FROM runs", &[]).unwrap()[0];
    assert_eq!(r["dev"], json!(1));
    assert_eq!(r["branches"], json!("[\"a\",\"b\"]"));
    assert_eq!(r["harness"], json!("{\"k\":1}"));
    assert_eq!(r["cost_usd"], json!(1.25));
    assert_eq!(r["units_total"], json!(3));
    assert_eq!(r["title"], json!("한글"));
}

#[test]
fn 표_안_모든_칸이_기본키뿐이어도_두_번_넣을_수_있다() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    db.upsert("cards", &row(json!({"run_id": "r", "card_id": "c"}))).unwrap();
    db.upsert("cards", &row(json!({"run_id": "r", "card_id": "c"}))).unwrap();
    assert_eq!(db.select("SELECT COUNT(*) AS n FROM cards", &[]).unwrap()[0]["n"], json!(1));
}

#[test]
fn 모르는_표_모르는_칸_기본키_없는_행은_거절한다() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    let kind = |r: Result<(), claude_board_autopilot::ports::PortError>| r.unwrap_err().kind;
    assert_eq!(kind(db.upsert("nope", &row(json!({"id": "x"})))), PortErrorKind::Invalid);
    assert_eq!(kind(db.upsert("projects", &row(json!({"id": "x", "name": "n", "nope": 1})))), PortErrorKind::Invalid);
    assert_eq!(kind(db.upsert("projects", &row(json!({"name": "n"})))), PortErrorKind::Invalid);
    assert_eq!(kind(db.upsert("projects", &row(json!({})))), PortErrorKind::Invalid);
    // 칸 이름에 SQL 을 섞은 경우
    assert_eq!(kind(db.upsert("projects", &row(json!({"id": "x", "name\") VALUES(1); DROP TABLE projects; --": 1})))), PortErrorKind::Invalid);
    assert_eq!(db.select("SELECT COUNT(*) AS n FROM projects", &[]).unwrap()[0]["n"], json!(0));
}

#[test]
fn sql_주입_꼴_표_이름은_거절하고_표는_그대로다() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    for t in [
        "projects; DROP TABLE projects",
        "projects\" (id) VALUES ('x'); DROP TABLE runs; --",
        "projects --",
        "PROJECTS",
        "sqlite_master",
        "",
    ] {
        let e = db.upsert(t, &row(json!({"id": "x", "name": "n"}))).unwrap_err();
        assert_eq!(e.kind, PortErrorKind::Invalid, "{t}");
    }
    assert_eq!(db.select("SELECT COUNT(*) AS n FROM sqlite_master WHERE type='table'", &[]).unwrap()[0]["n"], json!(13));
}

#[test]
fn select_는_읽기_전용이다() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    db.upsert("projects", &row(json!({"id": "p", "name": "n"}))).unwrap();
    for sql in [
        "DELETE FROM projects",
        "  delete from projects",
        "UPDATE projects SET name='x'",
        "DROP TABLE projects",
        "PRAGMA user_version=9",
        "SELECT 1; DELETE FROM projects",
        "SELECT 1; SELECT 2",
        "WITH x AS (SELECT 1) DELETE FROM projects",
        "",
    ] {
        let e = db.select(sql, &[]).unwrap_err();
        assert_eq!(e.kind, PortErrorKind::Invalid, "{sql}");
    }
    assert_eq!(db.version().unwrap(), 5);
    assert_eq!(db.select("SELECT id FROM projects", &[]).unwrap().len(), 1);
    // 앞 공백·소문자·끝 세미콜론은 허용한다
    assert_eq!(db.select("  \n select id from projects;", &[]).unwrap().len(), 1);
}

#[test]
fn select_는_인자를_묶고_정수_실수_글_null_을_돌려준다() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteRecords::open(&dir.path().join("r.sqlite")).unwrap();
    let rows = db
        .select("SELECT ? AS a, ? AS b, ? AS c, NULL AS d, ? AS e", &[json!(7), json!(2.5), json!("글"), json!(true)])
        .unwrap();
    assert_eq!(rows, vec![row(json!({"a": 7, "b": 2.5, "c": "글", "d": null, "e": 1}))]);
    // 인자 수가 어긋나면 Invalid
    assert_eq!(db.select("SELECT ?", &[]).unwrap_err().kind, PortErrorKind::Invalid);
}

#[test]
fn 두_연결이_스레드로_함께_빈_저장소를_열고_써도_둘_다_성공한다() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.sqlite");
    let handles: Vec<_> = (0..2)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                let db = SqliteRecords::open(&path).unwrap();
                for i in 0..60 {
                    db.upsert("carry", &row(json!({"id": format!("t{t}-{i}"), "run_id": "r", "unit_n": i}))).unwrap();
                    // 같은 키를 두 스레드가 겹쳐 덮어쓰기도 한다
                    db.upsert("projects", &row(json!({"id": format!("p{}", i % 5), "name": format!("t{t}")}))).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let db = SqliteRecords::open(&path).unwrap();
    assert_eq!(db.select("SELECT COUNT(*) AS n FROM carry", &[]).unwrap()[0]["n"], json!(120));
    assert_eq!(db.select("SELECT COUNT(*) AS n FROM projects", &[]).unwrap()[0]["n"], json!(5));
    assert_eq!(db.version().unwrap(), 5);
}
