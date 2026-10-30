// 자율주행 규칙(AutopilotRules) — 실행 기록 저장소(SQLite)의 표 꼴(설계 5장 7절). 엔진(쓰는 쪽)이 열 때 이 글로 만들고 판을 올린다.
// 2026-10-30 5단계 ⑪-3 에서 엔진 RecordsDB 의 상수를 그대로 옮겼다(옮기기 커밋, 같은 글자). 판을 올리는 SQL 과 옛 판 저장소를 여는 시험은
// 규칙 라이브러리에 두어 Tests/AutopilotRulesTests 가 SQLite 로 직접 건다(라이브러리는 Foundation 만, SQLite 는 엔진·보드·시험이)
import Foundation

public let apRecordsVersion: Int32 = 3
public let apRecordsTables = ["projects", "runs", "units", "queue_items", "cards", "carry", "sets", "tickets", "imports", "judge_misses",
                                 "path_claims", "stopline_slots"]
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

// 판 2(5단계 ⑪-3, 2026-10-30): 판정 놓침 표(설계 5장 7절). 실행·단위·신호 종류·판정 경로와 지문·짧은 까닭·쓰인 세트·상태(open·used·fixed)
public let apRecordsSchemaV2 = """
CREATE TABLE judge_misses (id TEXT PRIMARY KEY, project_id TEXT, run_id TEXT, unit_n INTEGER, unit_id TEXT, source_key TEXT, title TEXT,
  signal TEXT, side TEXT, judge TEXT, judge_digest TEXT, why TEXT, set_id TEXT, state TEXT, created REAL, used_run TEXT);
CREATE INDEX judge_misses_project ON judge_misses(project_id, state);
"""

// 판 3(6단계 ⑫-6, 2026-10-30): 여러 실행 함께 돌리기의 자리 표 둘(설계 5장 7절, 규칙 Multirun.swift).
// path_claims — 출발할 때 잡는 저장소 경로 자리(접은 경로 key 하나에 실행 하나, 끝 정리를 마치면 지움). stopline_slots — 새 단위 앞에서 잡는 멈출 선 자리
// (실행마다 한 줄, kind hold 단위가 도는 중·wait 기다리는 차례, pp·used 주간 %p, beat 가 15분 지나면 죽은 엔진의 자리)
public let apRecordsSchemaV3 = """
CREATE TABLE path_claims (key TEXT PRIMARY KEY, path TEXT NOT NULL, run_id TEXT NOT NULL, run_dir TEXT, label TEXT, pid INTEGER, claimed REAL);
CREATE INDEX path_claims_run ON path_claims(run_id);
CREATE TABLE stopline_slots (run_id TEXT PRIMARY KEY, project_id TEXT, unit_n INTEGER, kind TEXT NOT NULL, pp REAL, used REAL, pid INTEGER,
  since REAL, beat REAL);
"""

// 판 올리기 차례: 판 n 으로 올리는 SQL. 엔진이 열 때 지금 판보다 높은 것을 차례로(한 transaction 안에서, 잠금을 잡은 뒤 판을 다시 보고) 건다
public let apRecordsMigrations: [(version: Int32, sql: String)] = [(1, apRecordsSchemaV1), (2, apRecordsSchemaV2), (3, apRecordsSchemaV3)]
public func apRecordsSteps(from v: Int32) -> [(version: Int32, sql: String)] { apRecordsMigrations.filter { $0.version > v } }
