//! `control`·`status`·`unregister` — 실행 폴더의 파일과 작업 스케줄러만 읽거나(`status`) 요청 파일 하나·작업 하나만 바꾼다.

use std::path::Path;

use serde_json::{json, Value};

use super::{absolute, label_for, read_fail, read_required, refuse, run_id_of, Deps, Fail, Reply, Target};
use crate::model::Num;
use crate::rules::control::{board_can_control, control_name, run_ended, TAKEN_OVER_STATE};

fn why_not(action: &str, state: &str) -> String {
    let name = control_name(action).unwrap_or(action);
    if state.is_empty() {
        "아직 출발하지 않은 실행입니다.".to_string()
    } else if run_ended(state) {
        format!("이미 끝난 실행입니다({state}).")
    } else if state == TAKEN_OVER_STATE {
        format!("이어받은 실행에는 {name} 요청을 보낼 수 없습니다.")
    } else {
        format!("지금 상태({state})에서는 {name} 요청을 받을 수 없습니다.")
    }
}

pub(super) fn control(d: &Deps, dir: &Path, action: &str) -> Result<Reply, Fail> {
    if !dir.is_dir() {
        return Err(refuse(format!("실행 폴더가 없습니다: {}", dir.display())));
    }
    let run = read_required(d, &dir.join("run.json"), "run.json")?;
    let state = run.get("state").and_then(Value::as_str).unwrap_or("");
    if !board_can_control(action, state) {
        return Err(refuse(why_not(action, state)));
    }
    let request = json!({"action": action, "ts": Num::r3(d.clock.now()).raw()});
    d.files.write_json(&dir.join("control.json"), &request).map_err(|e| Fail::Internal(format!("control.json 을 쓰지 못했습니다: {}", e.message)))?;
    let why = format!("{} 요청을 보냈습니다.", control_name(action).unwrap_or(action));
    Ok(Reply { json: json!({"ok": true, "why": why}), text: why })
}

// ── status ───────────────────────────────────────────

fn label_of(run: &Value, id: &str, dev: bool) -> String {
    run.get("launchd_label").and_then(Value::as_str).filter(|l| !l.is_empty()).map_or_else(|| label_for(dev, id), str::to_string)
}

fn number_or_null(v: Option<&Value>) -> Value {
    v.filter(|x| x.is_number()).cloned().unwrap_or(Value::Null)
}

pub(super) fn status(d: &Deps, dir: &Path) -> Result<Reply, Fail> {
    if !dir.is_dir() {
        return Err(refuse(format!("실행 폴더가 없습니다: {}", dir.display())));
    }
    let id = run_id_of(dir);
    let run = read_required(d, &dir.join("run.json"), "run.json")?;
    let queue = match d.files.read_json(&dir.join("queue.json")) {
        Ok(Some(Value::Array(a))) => a,
        Ok(_) => Vec::new(),
        Err(e) => return Err(read_fail("queue.json", e)),
    };
    let mut units = Vec::new();
    for (i, u) in queue.iter().enumerate() {
        let n = i + 1;
        let engine = d.files.read_json(&dir.join("units").join(n.to_string()).join("engine.json")).ok().flatten();
        units.push(json!({
            "n": n,
            "title": u.get("title").and_then(Value::as_str).map_or_else(|| format!("단위 {n}"), str::to_string),
            "status": u.get("status").and_then(Value::as_str).unwrap_or("pending"),
            "active_sec": number_or_null(engine.as_ref().and_then(|e| e.get("active_sec"))),
            "cost_usd": number_or_null(engine.as_ref().and_then(|e| e.get("cost_usd"))),
        }));
    }
    let dev = run.get("dev").and_then(Value::as_bool).unwrap_or(false);
    let label = label_of(&run, &id, dev);
    let scheduled = (d.supervisor)(dev).is_registered(&label).unwrap_or(false);
    let text_of = |key: &str| run.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let (state, reason) = (text_of("state"), text_of("reason"));
    let json = json!({
        "run": id,
        "state": state,
        "reason": reason,
        "wait_until": number_or_null(run.get("wait_until")),
        "unit": number_or_null(run.get("unit")),
        "exec_env": run.get("exec_env").cloned().unwrap_or(Value::Null),
        "units": units,
        "scheduled": scheduled,
        "usage": run.get("usage").cloned().unwrap_or(Value::Null),
    });
    let mut text = format!("실행 {id}: {}\n{reason}", if state.is_empty() { "출발 전" } else { &state });
    for u in &units {
        let sec = u["active_sec"].as_f64().map_or(String::new(), |s| format!(" · {}분", (s / 60.0).round() as i64));
        let cost = u["cost_usd"].as_f64().map_or(String::new(), |c| format!(" · ${c:.2}"));
        text.push_str(&format!("\n단위 {}: {} · {}{sec}{cost}", u["n"], u["title"].as_str().unwrap_or(""), u["status"].as_str().unwrap_or("")));
    }
    text.push_str(&format!("\n작업 스케줄러: {}", if scheduled { "등록됨" } else { "없음" }));
    Ok(Reply { json, text })
}

// ── unregister ───────────────────────────────────────

pub(super) fn unregister(d: &Deps, target: &Target, dev: bool) -> Result<Reply, Fail> {
    let (sup, labels) = match target {
        Target::All => {
            let sup = (d.supervisor)(dev);
            let all = sup.list().map_err(|e| Fail::Internal(format!("작업 목록을 읽지 못했습니다: {}", e.message)))?;
            (sup, all)
        }
        Target::Folder(dir) => {
            let dir = absolute(dir);
            let run = d.files.read_json(&dir.join("run.json")).ok().flatten().unwrap_or(Value::Null);
            let dev = dev || run.get("dev").and_then(Value::as_bool).unwrap_or(false);
            ((d.supervisor)(dev), vec![label_of(&run, &run_id_of(&dir), dev)])
        }
    };
    let mut removed = Vec::new();
    for label in labels {
        if sup.is_registered(&label).map_err(|e| Fail::Internal(e.message))? {
            sup.unregister(&label).map_err(|e| Fail::Internal(format!("작업을 지우지 못했습니다({label}): {}", e.message)))?;
            removed.push(label);
        }
    }
    let text = if removed.is_empty() { "지울 작업이 없습니다.".to_string() } else { format!("작업을 지웠습니다: {}", removed.join(", ")) };
    Ok(Reply { json: json!({"ok": true, "removed": removed}), text })
}
