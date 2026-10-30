// 자율주행 규칙(AutopilotRules) — 실행 기록 저장소(SQLite)의 표 꼴(설계 5장 7절). 엔진(쓰는 쪽)이 열 때 이 글로 만들고 판을 올린다.
// 2026-10-30 5단계 ⑪-3 에서 엔진 RecordsDB 의 상수를 그대로 옮겼다(옮기기 커밋, 같은 글자). 판을 올리는 SQL 과 옛 판 저장소를 여는 시험은
// 규칙 라이브러리에 두어 Tests/AutopilotRulesTests 가 SQLite 로 직접 건다(라이브러리는 Foundation 만, SQLite 는 엔진·보드·시험이)
import Foundation

public let apRecordsVersion: Int32 = 1
public let apRecordsTables = ["projects", "runs", "units", "queue_items", "cards", "carry", "sets", "tickets", "imports"]
public let apRecordsSchemaV1 = """
CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT UNIQUE, kind TEXT, repos TEXT, created REAL, updated REAL);
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
CREATE TABLE imports (path TEXT NOT NULL, sha256 TEXT NOT NULL, rows INTEGER, at REAL, PRIMARY KEY (path, sha256));
"""
