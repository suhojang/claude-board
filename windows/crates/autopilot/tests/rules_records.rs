//! 실행 기록 저장소 표 꼴·판 올리기 시험.
//! 원본: Tests/AutopilotRulesTests/CritiqueTests.swift(RecordsMigrationTests·RecordsV4Tests), MultirunTests.swift(RecordsV3Tests)
use claude_board_autopilot::rules::records::*;
use rusqlite::Connection;

fn user_version(db: &Connection) -> i32 {
    db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

/// 원본 시험의 migrate: 지금 판 위의 차례를 걸고 판 번호를 적는다
fn migrate(db: &Connection) -> i32 {
    for s in records_steps(user_version(db)) {
        db.execute_batch(s.sql).unwrap();
        db.execute_batch(&format!("PRAGMA user_version={}", s.version)).unwrap();
    }
    user_version(db)
}

fn versions(steps: &[RecordsMigration]) -> Vec<i32> {
    steps.iter().map(|s| s.version).collect()
}

fn table_names(db: &Connection) -> Vec<String> {
    let mut st = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    st.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect()
}

#[test]
fn 옛판_저장소를_올리면_행은_그대로이고_판정_놓침_표가_생긴다() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zz-records-v1.sqlite");
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(RECORDS_SCHEMA_V1).unwrap();
        db.execute_batch("PRAGMA user_version=1").unwrap();
        db.execute_batch(
            "INSERT INTO projects(id,name,root) VALUES('zz-toy','zz-toy','/w/zz-toy'); INSERT INTO runs(id,project_id,state) VALUES('r1','zz-toy','done')",
        )
        .unwrap();
    }
    let db = Connection::open(&path).unwrap();
    assert_eq!(migrate(&db), RECORDS_VERSION);
    assert_eq!(RECORDS_VERSION, 5); // 판 3(자리 표 둘), 판 4(가져오기 줄 칸 넷), 판 5(단위 커밋 기록)
    let state: String = db.query_row("SELECT state FROM runs WHERE id='r1'", [], |r| r.get(0)).unwrap();
    assert_eq!(state, "done");
    db.execute_batch(
        "INSERT INTO judge_misses(id,project_id,run_id,signal,side,why,state,created) VALUES('r1/1/discard/1','zz-toy','r1','discard','judge','x','open',1)",
    )
    .unwrap();
    let n: i64 = db
        .query_row("SELECT COUNT(*) FROM judge_misses WHERE project_id='zz-toy' AND state='open'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    assert!(records_steps(5).is_empty());
}

#[test]
fn 빈_저장소는_판_1_2_3_4_5_를_차례로() {
    let db = Connection::open_in_memory().unwrap();
    assert_eq!(versions(&records_steps(0)), vec![1, 2, 3, 4, 5]);
    assert_eq!(migrate(&db), 5);
    for t in RECORDS_TABLES {
        let found: String = db
            .query_row("SELECT name FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0))
            .unwrap();
        assert_eq!(found, t);
    }
}

#[test]
fn 판2_저장소를_판3으로_올리면_행은_그대로이고_자리_표_둘이_생긴다_같은_경로_자리는_한_줄만() {
    let db = Connection::open_in_memory().unwrap();
    for s in records_steps(0).iter().filter(|s| s.version <= 2) {
        db.execute_batch(s.sql).unwrap();
    }
    db.execute_batch("PRAGMA user_version=2; INSERT INTO runs(id,state) VALUES('r1','done'); INSERT INTO units(run_id,n,status) VALUES('r1',1,'done')")
        .unwrap();
    assert_eq!(records_steps(2).first().map(|s| s.version), Some(3)); // 판 4 이후는 판4·판5 시험
    for s in records_steps(2).iter().filter(|s| s.version <= 3) {
        db.execute_batch(s.sql).unwrap();
        db.execute_batch(&format!("PRAGMA user_version={}", s.version)).unwrap();
    }
    assert_eq!(user_version(&db), 3);
    let units: i64 = db.query_row("SELECT COUNT(*) FROM units", [], |r| r.get(0)).unwrap();
    assert_eq!(units, 1);
    db.execute_batch("INSERT INTO path_claims(key,path,run_id,claimed) VALUES('/w/zz-toy-a','/w/zz-toy-a','r2',1)").unwrap();
    // 같은 접은 경로는 하나만
    assert!(db
        .execute_batch("INSERT INTO path_claims(key,path,run_id,claimed) VALUES('/w/zz-toy-a','/w/ZZ-TOY-A','r3',2)")
        .is_err());
    db.execute_batch("INSERT INTO stopline_slots(run_id,kind,pp,used,since,beat) VALUES('r2','hold',5,0,1,1)").unwrap();
    // 실행마다 한 줄
    assert!(db
        .execute_batch("INSERT INTO stopline_slots(run_id,kind,pp,used,since,beat) VALUES('r2','wait',5,0,1,1)")
        .is_err());
}

#[test]
fn 판3_저장소를_판4로_올리면_대기열_줄은_그대로이고_가져오기_칸_넷이_생긴다() {
    let db = Connection::open_in_memory().unwrap();
    for s in records_steps(0).iter().filter(|s| s.version <= 3) {
        db.execute_batch(s.sql).unwrap();
    }
    db.execute_batch("PRAGMA user_version=3; INSERT INTO queue_items(project_id,source_key,title,state) VALUES('zz-toy','todo:a#b','가','open')")
        .unwrap();
    assert_eq!(versions(&records_steps(3)), vec![4, 5]); // 판 5 는 판5 시험
    for s in records_steps(3).iter().filter(|s| s.version <= 4) {
        db.execute_batch(s.sql).unwrap();
        db.execute_batch(&format!("PRAGMA user_version={}", s.version)).unwrap();
    }
    let title: String = db
        .query_row("SELECT title FROM queue_items WHERE source_key='todo:a#b'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(title, "가");
    db.execute_batch("UPDATE queue_items SET digest='d1', summary='줄', source='todo', def='{}' WHERE source_key='todo:a#b'")
        .unwrap();
    let joined: String = db.query_row("SELECT digest || source || def FROM queue_items", [], |r| r.get(0)).unwrap();
    assert_eq!(joined, "d1todo{}");
}

#[test]
fn 판4_저장소를_판5로_올리면_행은_그대로이고_단위_커밋_표가_생긴다_같은_커밋_종류는_한_줄만() {
    let db = Connection::open_in_memory().unwrap();
    for s in records_steps(0).iter().filter(|s| s.version <= 4) {
        db.execute_batch(s.sql).unwrap();
    }
    db.execute_batch("PRAGMA user_version=4; INSERT INTO runs(id,state) VALUES('r1','done')").unwrap();
    assert_eq!(versions(&records_steps(4)), vec![5]);
    for s in records_steps(4) {
        db.execute_batch(s.sql).unwrap();
        db.execute_batch(&format!("PRAGMA user_version={}", s.version)).unwrap();
    }
    assert_eq!(user_version(&db), 5);
    let state: String = db.query_row("SELECT state FROM runs WHERE id='r1'", [], |r| r.get(0)).unwrap();
    assert_eq!(state, "done");
    db.execute_batch("INSERT INTO unit_commits(run_id,unit_n,repo,sha,kind) VALUES('r1',1,'/w/a','abc','unit')").unwrap();
    // (실행, 저장소, 커밋, 종류) 가 키다
    assert!(db.execute_batch("INSERT INTO unit_commits(run_id,unit_n,repo,sha,kind) VALUES('r1',2,'/w/a','abc','unit')").is_err());
    db.execute_batch("INSERT INTO unit_commits(run_id,unit_n,repo,sha,kind) VALUES('r1',1,'/w/a','abc','promote')").unwrap();
    let idx: String = db
        .query_row("SELECT name FROM sqlite_master WHERE type='index' AND name='unit_commits_sha'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(idx, "unit_commits_sha");
}

#[test]
fn 판_차례는_오름차순이고_마지막_판이_지금_판과_맞는다() {
    assert_eq!(versions(&RECORDS_MIGRATIONS), vec![1, 2, 3, 4, 5]);
    assert_eq!(RECORDS_MIGRATIONS.last().map(|m| m.version), Some(RECORDS_VERSION));
    assert_eq!(versions(&records_steps(2)), vec![3, 4, 5]);
    assert!(records_steps(RECORDS_VERSION).is_empty());
}

#[test]
fn 메모리_연결에_ddl을_걸면_표_이름이_상수와_같고_판_번호를_적을_수_있다() {
    let db = Connection::open_in_memory().unwrap();
    for s in RECORDS_MIGRATIONS {
        db.execute_batch(s.sql).unwrap();
    }
    let mut want: Vec<String> = RECORDS_TABLES.iter().map(|t| t.to_string()).collect();
    want.sort();
    assert_eq!(table_names(&db), want);
    db.execute_batch(&format!("PRAGMA user_version={RECORDS_VERSION}")).unwrap();
    assert_eq!(user_version(&db), RECORDS_VERSION);
}
