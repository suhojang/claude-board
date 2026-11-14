//! 실행 기록 저장소(SQLite)의 표 꼴과 판 올리기 차례(설계 5장 7절). 원본: `Sources/AutopilotRules/Records.swift`.
//! 저장소 파일은 macOS 판과 같은 약속이다(records store version 5, `PRAGMA user_version` = 5).
//! 아래 DDL 글은 원본과 글자 하나까지 같아야 한다 — 고치지 않는다. SQLite 는 부르지 않는다(어댑터 몫).

/// 원본: `apRecordsVersion`. `PRAGMA user_version` 에 적는 지금 판
pub const RECORDS_VERSION: i32 = 5;

/// 원본: `apRecordsTables`
pub const RECORDS_TABLES: [&str; 13] = [
    "projects",
    "runs",
    "units",
    "queue_items",
    "cards",
    "carry",
    "sets",
    "tickets",
    "imports",
    "judge_misses",
    "path_claims",
    "stopline_slots",
    "unit_commits",
];

/// 원본: `apRecordsSchemaV1`
pub const RECORDS_SCHEMA_V1: &str = r"CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT UNIQUE, kind TEXT, repos TEXT, created REAL, updated REAL);
CREATE TABLE runs (id TEXT PRIMARY KEY, project_id TEXT, title TEXT, source TEXT, continues TEXT, state TEXT, reason TEXT, model TEXT,
  effort TEXT, dev INTEGER, started REAL, ended REAL, units_total INTEGER, units_done INTEGER, units_failed INTEGER, units_capped INTEGER,
  units_stopped INTEGER, active_sec REAL, cost_usd REAL, weekly_pp REAL, worktree TEXT, branches TEXT, harness TEXT, set_id TEXT,
  epic TEXT, run_dir TEXT, origin TEXT, updated REAL);
CREATE INDEX runs_project ON runs(project_id);
CREATE TABLE units (run_id TEXT NOT NULL, n INTEGER NOT NULL, unit_id TEXT, title TEXT, kind TEXT, source_key TEXT, story_key TEXT,
  task_key TEXT, branch TEXT, status TEXT, merged TEXT, est_min REAL, est_basis TEXT, active_sec REAL, cost_usd REAL, weekly_pp REAL,
  turns INTEGER, tries INTEGER, check_exit INTEGER, denials TEXT, result TEXT, model TEXT, started REAL, ended REAL, PRIMARY KEY (run_id, n));
CREATE INDEX units_source ON units(source_key);
CREATE TABLE queue_items (project_id TEXT NOT NULL, source_key TEXT NOT NULL, title TEXT, kind TEXT, grp TEXT, est_min REAL, est_basis TEXT,
  depends TEXT, needs TEXT, state TEXT, done_run TEXT, done_unit INTEGER, origin TEXT, story_key TEXT, task_key TEXT, note TEXT,
  added REAL, updated REAL, PRIMARY KEY (project_id, source_key));
CREATE TABLE cards (run_id TEXT NOT NULL, card_id TEXT NOT NULL, def TEXT, answer TEXT, policy INTEGER, answered REAL, PRIMARY KEY (run_id, card_id));
CREATE TABLE carry (id TEXT PRIMARY KEY, run_id TEXT, unit_n INTEGER, kind TEXT, title TEXT, why TEXT, state TEXT, used_run TEXT);
CREATE INDEX carry_run ON carry(run_id);
CREATE TABLE sets (id TEXT PRIMARY KEY, project_id TEXT, digest TEXT UNIQUE, instruction TEXT, spec TEXT, stack TEXT, checks TEXT, harness TEXT,
  worktree_files TEXT, parent_id TEXT, origin TEXT, created REAL);
CREATE TABLE tickets (project_id TEXT NOT NULL, key TEXT NOT NULL, kind TEXT, parent_key TEXT, title TEXT, url TEXT, source_key TEXT,
  created_run TEXT, created REAL, PRIMARY KEY (project_id, key));
CREATE TABLE imports (path TEXT NOT NULL, sha256 TEXT NOT NULL, rows INTEGER, at REAL, PRIMARY KEY (path, sha256));";

/// 판 2: 판정 놓침 표. 원본: `apRecordsSchemaV2`
pub const RECORDS_SCHEMA_V2: &str = r"CREATE TABLE judge_misses (id TEXT PRIMARY KEY, project_id TEXT, run_id TEXT, unit_n INTEGER, unit_id TEXT, source_key TEXT, title TEXT,
  signal TEXT, side TEXT, judge TEXT, judge_digest TEXT, why TEXT, set_id TEXT, state TEXT, created REAL, used_run TEXT);
CREATE INDEX judge_misses_project ON judge_misses(project_id, state);";

/// 판 3: 여러 실행 함께 돌리기의 자리 표 둘. 원본: `apRecordsSchemaV3`
pub const RECORDS_SCHEMA_V3: &str = r"CREATE TABLE path_claims (key TEXT PRIMARY KEY, path TEXT NOT NULL, run_id TEXT NOT NULL, run_dir TEXT, label TEXT, pid INTEGER, claimed REAL);
CREATE INDEX path_claims_run ON path_claims(run_id);
CREATE TABLE stopline_slots (run_id TEXT PRIMARY KEY, project_id TEXT, unit_n INTEGER, kind TEXT NOT NULL, pp REAL, used REAL, pid INTEGER,
  since REAL, beat REAL);";

/// 판 4: 가져오기 줄의 칸 넷. 원본: `apRecordsSchemaV4`
pub const RECORDS_SCHEMA_V4: &str = r"ALTER TABLE queue_items ADD COLUMN digest TEXT;
ALTER TABLE queue_items ADD COLUMN summary TEXT;
ALTER TABLE queue_items ADD COLUMN source TEXT;
ALTER TABLE queue_items ADD COLUMN def TEXT;";

/// 판 5: 단위 커밋 기록(실행·단위·저장소·커밋 이름·종류·든 가지·합칠 쪽·때). 원본: `apRecordsSchemaV5`.
/// macOS 의 승격 뒤 되돌림 짝짓기(`UnitCommits.swift`)가 쓰는 표다. Windows 엔진은 아직 이 표에 쓰지 않는다
/// (저장소 판만 macOS 와 맞춰 두어, 같은 판의 저장소를 서로 열 수 있다).
pub const RECORDS_SCHEMA_V5: &str = r"CREATE TABLE unit_commits (run_id TEXT NOT NULL, unit_n INTEGER, repo TEXT NOT NULL, sha TEXT NOT NULL, kind TEXT NOT NULL, ref TEXT, source TEXT, at REAL,
  PRIMARY KEY (run_id, repo, sha, kind));
CREATE INDEX unit_commits_sha ON unit_commits(repo, sha);";

/// 판 `version` 으로 올리는 SQL 한 묶음
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordsMigration {
    pub version: i32,
    pub sql: &'static str,
}

/// 원본: `apRecordsMigrations`. 판 번호 오름차순
pub const RECORDS_MIGRATIONS: [RecordsMigration; 5] = [
    RecordsMigration { version: 1, sql: RECORDS_SCHEMA_V1 },
    RecordsMigration { version: 2, sql: RECORDS_SCHEMA_V2 },
    RecordsMigration { version: 3, sql: RECORDS_SCHEMA_V3 },
    RecordsMigration { version: 4, sql: RECORDS_SCHEMA_V4 },
    RecordsMigration { version: 5, sql: RECORDS_SCHEMA_V5 },
];

/// 원본: `apRecordsSteps`. 지금 판 `from` 보다 높은 판을 차례로 돌려준다
pub fn records_steps(from: i32) -> Vec<RecordsMigration> {
    RECORDS_MIGRATIONS.iter().copied().filter(|m| m.version > from).collect()
}
