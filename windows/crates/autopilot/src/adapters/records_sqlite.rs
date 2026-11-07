//! 실행 기록 저장소 어댑터(SQLite, 판 4). 포트 `Records` 의 진짜 구현.
//! macOS 판 `RecordsDB`(main.swift)와 같은 약속이다: WAL, busy_timeout 5초, 잠겨 실패한 걸음은 0.1·0.2·0.4·0.8·1 초 쉬며 여덟 번까지 다시 한다
//! (원본: `apBusyRetryDelay`). 판 올리기는 `BEGIN IMMEDIATE` 로 잠금을 잡은 뒤 판을 다시 읽어 남은 판만 건다 —
//! 여러 엔진이 함께 열어도 한 번만 올라간다. 저장소 판이 이 엔진이 아는 판보다 새로우면 내리지 않고 `Unavailable` 로 거절한다.
//! 저장소 파일 자리는 부르는 쪽이 정한다(`private/records.sqlite`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, ErrorCode};

use crate::ports::{PortError, PortErrorKind, PortResult, Records, Row};
use crate::rules::records::{records_steps, RECORDS_TABLES, RECORDS_VERSION};

/// 다른 엔진이 쓰는 중이면 기다리는 시간(원본 `sqlite3_busy_timeout(db, 5000)`)
const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);
/// 원본: `apBusyRetries`
const BUSY_RETRIES: u32 = 8;

/// 원본: `apBusyRetryDelay`. `attempt` 는 1 부터, 여덟 번을 넘으면 None(포기)
fn busy_retry_delay(attempt: u32) -> Option<Duration> {
    if !(1..=BUSY_RETRIES).contains(&attempt) {
        return None;
    }
    let secs = (0.1 * f64::from(1u32 << (attempt - 1).min(4))).min(1.0);
    Some(Duration::from_secs_f64(secs))
}

fn to_port(e: rusqlite::Error) -> PortError {
    let kind = match &e {
        rusqlite::Error::SqliteFailure(f, _) => match f.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => PortErrorKind::Busy,
            ErrorCode::PermissionDenied | ErrorCode::ReadOnly => PortErrorKind::Denied,
            ErrorCode::ConstraintViolation | ErrorCode::TypeMismatch => PortErrorKind::Invalid,
            _ => PortErrorKind::Io,
        },
        rusqlite::Error::MultipleStatement | rusqlite::Error::InvalidParameterCount(..) => PortErrorKind::Invalid,
        _ => PortErrorKind::Io,
    };
    PortError::new(kind, e.to_string())
}

/// 잠겨 실패한 걸음을 규칙대로 다시 한다. 예산이 다하면 Busy 를 그대로 돌려준다
fn again<T>(mut step: impl FnMut() -> PortResult<T>) -> PortResult<T> {
    let mut attempt = 0;
    loop {
        match step() {
            Err(e) if e.kind == PortErrorKind::Busy => {
                attempt += 1;
                match busy_retry_delay(attempt) {
                    Some(d) => std::thread::sleep(d),
                    None => return Err(e),
                }
            }
            other => return other,
        }
    }
}

/// 쓰기 잠금을 먼저 잡고(BEGIN IMMEDIATE) 한 번에 쓴다. 원본: `RecordsDB.transaction`
fn transaction(conn: &Connection, body: impl Fn() -> PortResult<()>) -> PortResult<()> {
    again(|| {
        conn.execute_batch("BEGIN IMMEDIATE").map_err(to_port)?;
        let done = body().and_then(|()| conn.execute_batch("COMMIT").map_err(to_port));
        if done.is_err() {
            let _ = conn.execute_batch("ROLLBACK");
        }
        done
    })
}

fn read_version(conn: &Connection) -> PortResult<i32> {
    again(|| conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)).map_err(to_port))
}

/// 표 하나의 칸 이름(정의 순서)과 기본키 칸(기본키 순서)
struct TableInfo {
    columns: Vec<String>,
    pk: Vec<String>,
}

pub struct SqliteRecords {
    conn: Mutex<Connection>,
    /// 표 정의 캐시. 판은 열 때 정해지므로 연결이 사는 동안 바뀌지 않는다
    tables: Mutex<HashMap<String, TableInfo>>,
}

impl SqliteRecords {
    /// 저장소를 열고(없으면 만든다) 남은 판을 올린다.
    pub fn open(path: &Path) -> PortResult<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path).map_err(to_port)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(to_port)?;
        // 빈 저장소를 여럿이 함께 열면 WAL 로 바꾸는 걸음이 busy_timeout 을 거치지 않고 잠겨 실패할 수 있어 같은 규칙으로 다시 한다
        again(|| conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)).map(|_| ()).map_err(to_port))?;

        let v = read_version(&conn)?;
        if v > RECORDS_VERSION {
            return Err(PortError::new(
                PortErrorKind::Unavailable,
                format!("저장소 판({v})이 이 엔진이 아는 판({RECORDS_VERSION})보다 새롭습니다. 보드를 업데이트하세요"),
            ));
        }
        if v < RECORDS_VERSION {
            transaction(&conn, || {
                // 다른 엔진이 먼저 올렸을 수 있다 — 잠금을 잡은 뒤 판을 다시 보고 남은 판만 건다
                let cur = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)).map_err(to_port)?;
                for step in records_steps(cur) {
                    conn.execute_batch(step.sql).map_err(to_port)?;
                    conn.execute_batch(&format!("PRAGMA user_version={}", step.version)).map_err(to_port)?;
                }
                Ok(())
            })?;
        }
        Ok(Self { conn: Mutex::new(conn), tables: Mutex::new(HashMap::new()) })
    }

    fn lock_conn(&self) -> PortResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| PortError::new(PortErrorKind::Io, "저장소 연결 잠금이 깨졌습니다"))
    }

    /// 표 정의를 캐시에서 꺼내고 없으면 `PRAGMA table_info` 로 읽는다. 표 이름은 `RECORDS_TABLES` 안이어야 한다.
    fn table_info<R>(&self, conn: &Connection, table: &str, f: impl FnOnce(&TableInfo) -> R) -> PortResult<R> {
        if !RECORDS_TABLES.contains(&table) {
            return Err(PortError::new(PortErrorKind::Invalid, format!("기록 저장소에 없는 표입니다: {table}")));
        }
        let mut cache = self.tables.lock().map_err(|_| PortError::new(PortErrorKind::Io, "표 정의 캐시 잠금이 깨졌습니다"))?;
        if !cache.contains_key(table) {
            // 이름이 허용 목록 안이라 그대로 넣어도 안전하다
            let mut st = conn.prepare(&format!("PRAGMA table_info(\"{table}\")")).map_err(to_port)?;
            let rows: Vec<(String, i64)> = st
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))
                .map_err(to_port)?
                .collect::<Result<_, _>>()
                .map_err(to_port)?;
            let columns = rows.iter().map(|(n, _)| n.clone()).collect();
            let mut pk: Vec<(i64, String)> = rows.into_iter().filter(|(_, p)| *p > 0).map(|(n, p)| (p, n)).collect();
            pk.sort();
            cache.insert(table.to_string(), TableInfo { columns, pk: pk.into_iter().map(|(_, n)| n).collect() });
        }
        Ok(f(&cache[table]))
    }
}

/// JSON 값 → SQLite 값. 불리언은 0/1, 객체·배열은 JSON 글
fn to_sql(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Integer(i64::from(*b)),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => n.as_f64().map_or(Value::Null, Value::Real),
        },
        serde_json::Value::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

/// SQLite 값 → JSON 값
fn from_sql(v: ValueRef<'_>) -> PortResult<serde_json::Value> {
    Ok(match v {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(i) => i.into(),
        ValueRef::Real(f) => serde_json::Number::from_f64(f).map_or(serde_json::Value::Null, serde_json::Value::Number),
        ValueRef::Text(t) => serde_json::Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(_) => return Err(PortError::new(PortErrorKind::Invalid, "BLOB 칸은 읽지 않습니다")),
    })
}

fn quote(name: &str) -> String {
    format!("\"{name}\"")
}

impl Records for SqliteRecords {
    fn version(&self) -> PortResult<i32> {
        read_version(&*self.lock_conn()?)
    }

    fn upsert(&self, table: &str, row: &Row) -> PortResult<()> {
        let conn = self.lock_conn()?;
        // 칸 이름은 표 정의에 있는 것만 SQL 에 넣는다 — 정의 밖 이름은 여기서 막힌다
        let (pk, unknown) = self.table_info(&conn, table, |t| {
            let unknown = row.keys().find(|k| !t.columns.contains(k)).cloned();
            (t.pk.clone(), unknown)
        })?;
        if let Some(k) = unknown {
            return Err(PortError::new(PortErrorKind::Invalid, format!("{table} 표에 없는 칸입니다: {k}")));
        }
        if row.is_empty() {
            return Err(PortError::new(PortErrorKind::Invalid, "빈 행은 쓰지 않습니다"));
        }
        if let Some(k) = pk.iter().find(|k| row.get(*k).is_none_or(|v| v.is_null())) {
            return Err(PortError::new(PortErrorKind::Invalid, format!("{table} 표의 기본키 칸이 비었습니다: {k}")));
        }

        let cols: Vec<&String> = row.keys().collect();
        let names = cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
        let marks = (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
        let sets = cols
            .iter()
            .filter(|c| !pk.contains(**c))
            .map(|c| format!("{0}=excluded.{0}", quote(c)))
            .collect::<Vec<_>>();
        let conflict = pk.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
        let action = if sets.is_empty() { "DO NOTHING".to_string() } else { format!("DO UPDATE SET {}", sets.join(", ")) };
        let sql = format!("INSERT INTO {} ({names}) VALUES ({marks}) ON CONFLICT ({conflict}) {action}", quote(table));
        let params: Vec<Value> = cols.iter().map(|c| to_sql(&row[*c])).collect();

        again(|| conn.execute(&sql, rusqlite::params_from_iter(params.iter())).map(|_| ()).map_err(to_port))
    }

    fn select(&self, sql: &str, args: &[serde_json::Value]) -> PortResult<Vec<Row>> {
        let head = sql.trim_start();
        if !head.get(..6).is_some_and(|h| h.eq_ignore_ascii_case("select")) {
            return Err(PortError::new(PortErrorKind::Invalid, "읽기 전용입니다: SELECT 만 받습니다"));
        }
        let params: Vec<Value> = args.iter().map(to_sql).collect();
        let conn = self.lock_conn()?;
        again(|| {
            // 문장이 둘 이상이면 prepare 가 MultipleStatement 로 거절한다
            let mut st = conn.prepare(sql).map_err(to_port)?;
            if !st.readonly() {
                return Err(PortError::new(PortErrorKind::Invalid, "읽기 전용입니다: 쓰는 문장은 받지 않습니다"));
            }
            let names: Vec<String> = st.column_names().map(String::from).collect();
            let mut rows = st.query(rusqlite::params_from_iter(params.iter())).map_err(to_port)?;
            let mut out = Vec::new();
            while let Some(r) = rows.next().map_err(to_port)? {
                let mut row = Row::new();
                for (i, name) in names.iter().enumerate() {
                    row.insert(name.clone(), from_sql(r.get_ref(i).map_err(to_port)?)?);
                }
                out.push(row);
            }
            Ok(out)
        })
    }
}
