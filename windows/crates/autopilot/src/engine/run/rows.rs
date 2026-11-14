//! 실행 기록 저장소(SQLite) 행 — 실행 폴더의 파일 값에서 `runs`·`units`·`projects` 행을 만든다.
//! 원본: `main.swift` 의 `syncRun`(1574~1680)과 `upsertProject`(1560~1572) 중 runs·units·projects 부분. 순수 함수다.
//! 칸 이름은 `rules::records` 의 표 정의에 있는 것만 쓴다(어댑터가 모르는 칸을 거절한다). 값이 없으면 `Value::Null`.
//! JSON 칸은 원본 `jsonText` 와 같이 키를 정렬한 한 줄 글로 낸다.

use serde_json::{Map, Value};

use crate::engine::decide::UnitStatus;
use crate::model::{FrozenFile, Num, Ordered, QueueFile, QueueUnit, RunFile, Slot, UnitProgressFile};
use crate::ports::Row;

/// 원본: `jsonText` — 키를 정렬한 한 줄 JSON(슬래시를 이스케이프하지 않음)
fn json_text(v: &Value) -> String {
    fn sorted(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                Value::Object(keys.into_iter().map(|k| (k.clone(), sorted(&m[k]))).collect::<Map<_, _>>())
            }
            Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    sorted(v).to_string()
}

/// 원본: `jtext` — 없음·null 은 null, 글은 그대로, 그 밖은 JSON 글
fn jtext(v: Option<&Value>) -> Value {
    match v {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(other) => Value::String(json_text(other)),
    }
}

fn text<S: AsRef<str>>(v: Option<S>) -> Value {
    v.map_or(Value::Null, |s| Value::String(s.as_ref().to_string()))
}

/// 숫자 칸 → 값(정수로 떨어지면 정수). 숫자가 아니면 null
fn number(n: &Slot<Num>) -> Value {
    n.get().and_then(Num::as_f64).map_or(Value::Null, |x| Num::from_f64(x).raw().clone())
}

fn r3(x: f64) -> Value {
    Num::r3(x).raw().clone()
}

fn int(x: Option<f64>) -> Value {
    x.map_or(Value::Null, |v| Value::from(v as i64))
}

fn row(cols: Vec<(&str, Value)>) -> Row {
    cols.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// `runs` 행. 합계(active_sec·cost_usd·weekly_pp)는 단위 engine.json 들의 합이고, 단위 수는 queue 의 status 로 센다.
/// 제목·모델·effort 는 고정 설정이 run.json 보다 앞선다.
pub fn run_row(
    run_id: &str,
    run_dir: &str,
    run: &RunFile,
    queue: &QueueFile,
    frozen: &FrozenFile,
    units: &[Option<UnitProgressFile>],
    now: f64,
) -> Row {
    let status = |q: &Ordered<QueueUnit>| q.status.get().and_then(|s| UnitStatus::parse(s)).unwrap_or(UnitStatus::Pending);
    let count = |s: UnitStatus| queue.iter().filter(|q| status(q) == s).count();
    let sum = |pick: fn(&UnitProgressFile) -> Option<f64>| units.iter().flatten().map(|e| pick(e).unwrap_or(0.0)).sum::<f64>();
    let ex = |k: &str| run.extra.get(k);
    row(vec![
        ("id", Value::from(run_id)),
        ("project_id", text(frozen.project.get().or_else(|| run.project.get()))),
        ("title", text(frozen.title.get().or_else(|| run.title.get()))),
        ("source", jtext(ex("source"))),
        ("continues", jtext(ex("continues"))),
        ("state", text(run.state.get())),
        ("reason", text(run.reason.get())),
        ("model", text(frozen.model.get().or_else(|| run.model.get()))),
        ("effort", text(frozen.effort.get().map(String::as_str).or_else(|| ex("effort").and_then(Value::as_str)))),
        ("dev", run.dev.get().map_or(Value::Null, |d| Value::from(i64::from(*d)))),
        ("started", number(&run.started)),
        ("ended", number(&run.ended)),
        ("units_total", Value::from(queue.len())),
        ("units_done", Value::from(count(UnitStatus::Done))),
        ("units_failed", Value::from(count(UnitStatus::Failed))),
        ("units_capped", Value::from(count(UnitStatus::Capped))),
        ("units_stopped", Value::from(count(UnitStatus::Stopped))),
        ("active_sec", r3(sum(|e| e.active_sec.get().and_then(Num::as_f64)))),
        ("cost_usd", r3(sum(|e| e.cost_usd.get().and_then(Num::as_f64)))),
        ("weekly_pp", r3(sum(|e| e.weekly_pp.get().and_then(Num::as_f64)))),
        ("worktree", jtext(run.worktree.get())),
        ("branches", jtext(ex("branches"))),
        ("harness", jtext(ex("harness"))),
        ("set_id", Value::Null),
        ("epic", jtext(ex("epic"))),
        ("run_dir", Value::from(run_dir)),
        ("origin", Value::from("engine")),
        ("updated", r3(now)),
    ])
}

/// `units` 행(단위 하나). `def` 는 고정 설정의 단위 정의, `e` 는 그 단위의 engine.json, `result_text` 는 result.json 글 그대로.
/// 종류는 정의에 없으면 결과 기록의 `unit.kind` 로 채운다.
pub fn unit_row(
    run_id: &str,
    n: usize,
    def: &Ordered<QueueUnit>,
    status: &str,
    e: Option<&UnitProgressFile>,
    result_text: Option<&str>,
    model: Option<&str>,
) -> Row {
    let kind = def.kind.get().cloned().or_else(|| {
        let result: Value = serde_json::from_str(result_text?).ok()?;
        result.get("unit")?.get("kind")?.as_str().map(str::to_string)
    });
    let from_e = |pick: fn(&UnitProgressFile) -> Value| e.map_or(Value::Null, pick);
    let check_exit = e
        .and_then(|e| e.checks.get())
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|c| c.get("exit"))
        .map(|x| int(match x {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }))
        .unwrap_or(Value::Null);
    row(vec![
        ("run_id", Value::from(run_id)),
        ("n", Value::from(n)),
        ("unit_id", text(def.id.get())),
        ("title", text(def.title.get())),
        ("kind", kind.map_or(Value::Null, Value::String)),
        ("source_key", text(def.source_key.get())),
        ("story_key", Value::Null),
        ("task_key", Value::Null),
        ("branch", text(e.and_then(|e| e.branch.get()).or_else(|| def.branch.get()))),
        ("status", Value::from(status)),
        ("merged", Value::Null),
        ("est_min", number(&def.est_min)),
        ("est_basis", text(def.extra.get("est_basis").and_then(Value::as_str))),
        ("active_sec", from_e(|e| number(&e.active_sec))),
        ("cost_usd", from_e(|e| number(&e.cost_usd))),
        ("weekly_pp", from_e(|e| number(&e.weekly_pp))),
        ("turns", from_e(|e| int(e.turns.get().and_then(Num::as_f64)))),
        ("tries", from_e(|e| int(e.r#try.get().and_then(Num::as_f64)))),
        ("check_exit", check_exit),
        (
            "denials",
            e.and_then(|e| e.denials.get()).filter(|d| d.is_array()).map_or(Value::Null, |d| Value::String(json_text(d))),
        ),
        ("result", result_text.map_or(Value::Null, Value::from)),
        ("model", model.map_or(Value::Null, Value::from)),
        ("started", from_e(|e| number(&e.started))),
        ("ended", from_e(|e| number(&e.ended))),
    ])
}

/// 프로젝트 작업 경로의 꼴을 한 가지로 맞춘 글(저장하는 글). `\\?\` 머리를 떼고, 드라이브 글자·UNC 꼴이면 `/` 를 `\` 로,
/// 겹친 `\` 를 하나로, 끝의 `\` 를 뗀다(`C:\` 뿌리는 그대로). 드라이브 글자는 대문자로 쓴다. 그 밖의 글(POSIX 꼴)은 그대로 둔다.
/// 폴더를 열어 보지 않는 글자 규칙이다 — 짧은 이름(8.3)·심볼릭 링크·접합(junction)은 풀지 못한다.
pub fn normalize_root(raw: &str) -> String {
    let mut t = raw.trim().to_string();
    if let Some(rest) = t.strip_prefix(r"\\?\UNC\") {
        t = format!(r"\\{rest}");
    } else if let Some(rest) = t.strip_prefix(r"\\?\") {
        t = rest.to_string();
    }
    let drive = t.len() >= 2 && t.as_bytes()[0].is_ascii_alphabetic() && t.as_bytes()[1] == b':';
    if !(drive || t.starts_with(r"\\") || t.starts_with("//")) {
        return t;
    }
    let unc = t.starts_with(r"\\") || t.starts_with("//");
    let body: String = t.replace('/', "\\");
    let mut out = String::new();
    let mut prev_sep = false;
    for (i, c) in body.chars().enumerate() {
        let sep = c == '\\';
        if sep && prev_sep && !(unc && i == 1) {
            continue;
        }
        prev_sep = sep;
        out.push(c);
    }
    let floor = if drive { 3 } else { 2 };
    while out.ends_with('\\') && out.len() > floor {
        out.pop();
    }
    if drive && out.len() == 2 {
        out.push('\\');
    }
    if drive {
        let first = out.remove(0).to_ascii_uppercase();
        out.insert(0, first);
    }
    out
}

/// 두 작업 경로가 같은 폴더를 가리키는지 견주는 열쇠. Windows 는 대소문자를 가리지 않으므로 소문자로 낮춘다.
/// 저장된 옛 글(`/` 꼴·끝 `\`)도 같은 열쇠가 되게 `normalize_root` 를 거친다
pub fn root_key(raw: &str) -> String {
    normalize_root(raw).to_lowercase()
}

/// 이 프로젝트가 쓸 `root` 칸. 원본 `upsertProject` 의 「다른 프로젝트의 자리」 규칙: `projects.root` 는 UNIQUE 라
/// 같은 폴더를 다른 프로젝트가 이미 갖고 있으면(`others` = 다른 프로젝트의 (id, root)) 그 자리를 뺏지 않고 이 프로젝트의 이전 root(없으면 null)를 둔다.
/// `want` 가 없으면 이전 root 를 그대로 잇는다
pub fn claim_root(want: Option<&str>, old_root: Option<&str>, others: &[(String, String)]) -> Option<String> {
    let Some(w) = want else { return old_root.map(str::to_string) };
    let key = root_key(w);
    if others.iter().any(|(_, r)| root_key(r) == key) {
        return old_root.map(str::to_string);
    }
    Some(normalize_root(w))
}

/// `projects` 행. 이미 있는 행이면 `created` 를 넣지 않아(일반 upsert 가 `created` 를 덮지 않게) 처음 만든 때가 남는다.
/// `root` 가 None 이면 null 이 되어 이미 있던 root 를 지우므로, 부르는 쪽이 기존 root 를 읽어 넘긴다.
/// 원본: `upsertProject`. 이름은 id 와 같고, 저장소가 둘 이상이면 `bundle`
pub fn project_row(id: &str, root: Option<&str>, repos: &[String], exists: bool, now: f64) -> Row {
    let mut cols = vec![
        ("id", Value::from(id)),
        ("name", Value::from(id)),
        ("root", root.map_or(Value::Null, Value::from)),
        ("kind", Value::from(if repos.len() > 1 { "bundle" } else { "single" })),
        ("repos", Value::String(json_text(&Value::from(repos.to_vec())))),
    ];
    if !exists {
        cols.push(("created", r3(now)));
    }
    cols.push(("updated", r3(now)));
    row(cols)
}
