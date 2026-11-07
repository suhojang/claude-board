//! 실행 폴더 파일 꼴(`model`) 시험 — macOS 가 쓴 파일을 읽고 다시 써도 값·키 순서가 그대로여야 한다.
//! 견본은 손으로 만든 것이다(`tests/fixtures/mac-run/README.txt`).

use claude_board_autopilot::model::{
    from_slice, to_line, to_pretty_json, ControlFile, EventFile, ExecEnv, FrozenFile, Num, Ordered, QueueFile, Run, RunFile,
    Slot, UnitProgressFile, UnsandboxedOk,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mac-run/");

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}{name}")).unwrap_or_else(|e| panic!("견본 {name} 을 읽지 못함: {e}"))
}

/// 깊이 우선으로 모든 키 경로를 읽은 차례대로 모은다. 객체 비교는 순서를 안 보므로 순서는 이걸로 따로 견준다
fn key_paths(v: &Value, prefix: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                let p = format!("{prefix}/{k}");
                out.push(p.clone());
                key_paths(v, &p, out);
            }
        }
        Value::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                key_paths(v, &format!("{prefix}[{i}]"), out);
            }
        }
        _ => {}
    }
}

fn paths(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    key_paths(v, "", &mut out);
    out
}

/// 견본 → 형 → 글 → 값 이 처음 값과 같고 키 순서도 같다
fn assert_round_trip<T: DeserializeOwned + Serialize>(name: &str) {
    let bytes = fixture(name);
    let typed: T = from_slice(&bytes).unwrap_or_else(|e| panic!("{name} 읽기 실패: {e}"));
    let written = to_pretty_json(&typed).unwrap();
    let before: Value = from_slice(&bytes).unwrap();
    let after: Value = from_slice(written.as_bytes()).unwrap();
    assert_eq!(before, after, "{name}: 값이 달라짐");
    assert_eq!(paths(&before), paths(&after), "{name}: 키 순서가 달라짐");
    // 다시 읽어 또 쓰면 글이 그대로 — 쓰는 쪽이 흔들리지 않는다
    let again: T = from_slice(written.as_bytes()).unwrap();
    assert_eq!(to_pretty_json(&again).unwrap(), written, "{name}: 두 번째 쓰기가 다름");
}

#[test]
fn 견본_run_json_이_그대로_돌아온다() {
    assert_round_trip::<RunFile>("run.json");
}

#[test]
fn 견본_queue_json_이_그대로_돌아온다() {
    assert_round_trip::<QueueFile>("queue.json");
}

#[test]
fn 견본_engine_json_이_그대로_돌아온다() {
    assert_round_trip::<UnitProgressFile>("units/1/engine.json");
    assert_round_trip::<UnitProgressFile>("units/2/engine.json");
}

#[test]
fn 견본_control_json_이_그대로_돌아온다() {
    assert_round_trip::<ControlFile>("control.json");
    assert_round_trip::<ControlFile>("control-giveback.json");
}

#[test]
fn 견본_frozen_json_이_그대로_돌아온다() {
    assert_round_trip::<FrozenFile>("pinned/frozen.json");
}

#[test]
fn events_jsonl_한_줄이_글자까지_그대로_돌아온다() {
    let text = String::from_utf8(fixture("events.jsonl")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4);
    for line in lines {
        let ev: EventFile = from_slice(line.as_bytes()).unwrap();
        // 한 줄 JSON 은 macOS(키 정렬, 슬래시 안 이스케이프)와 글자까지 같다
        assert_eq!(to_line(&ev).unwrap(), line);
    }
    let first: EventFile = from_slice(text.lines().next().unwrap().as_bytes()).unwrap();
    assert_eq!(first.who.get().map(String::as_str), Some("engine"));
    assert_eq!(first.ts.get().and_then(Num::as_f64), Some(1761010005.0));
}

#[test]
fn 견본의_칸을_타입으로_읽는다() {
    let run: RunFile = from_slice(&fixture("run.json")).unwrap();
    assert_eq!(run.state.get().map(String::as_str), Some("running"));
    assert_eq!(run.unit.get(), Some(&1));
    assert_eq!(run.engine_pid.get(), Some(&48213));
    assert_eq!(run.sessions.get().map(Vec::len), Some(1));
    assert_eq!(run.control_seen.get().and_then(Num::as_f64), Some(1761012345.5));
    // 이름 붙이지 않은 칸은 extra 에 남는다
    assert!(run.extra.contains_key("allowed_tools") && run.extra.contains_key("branches"));

    let queue: QueueFile = from_slice(&fixture("queue.json")).unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[1].depends.get(), Some(&vec!["u1".to_string()]));
    assert_eq!(queue[1].tickets.get(), Some(&vec!["TOY-12".to_string()]));
    assert!(queue[1].extra.contains_key("check_ports"));

    let frozen: FrozenFile = from_slice(&fixture("pinned/frozen.json")).unwrap();
    assert_eq!(frozen.units.get().map(Vec::len), Some(2));
    assert!(frozen.extra.contains_key("network_allow"));
}

#[test]
fn null_과_정수_표기가_보존된다() {
    let run: RunFile = from_slice(&fixture("run.json")).unwrap();
    assert!(run.wait_until.is_null(), "null 칸은 없음이 아니라 null");
    let out = to_pretty_json(&run).unwrap();
    assert!(out.contains("\"wait_until\": null"), "{out}");
    // 정수로 쓰인 시각은 정수로 나간다(1761010005.0 이 되면 안 됨)
    assert!(out.contains("\"started\": 1761010005,"), "{out}");

    let unit: UnitProgressFile = from_slice(&fixture("units/1/engine.json")).unwrap();
    assert!(unit.weekly_base.is_null());
    assert_eq!(unit.r#try.get().and_then(Num::as_f64), Some(1.0));
}

#[test]
fn 모르는_최상위_칸과_단위_안_칸이_살아남고_순서도_지킨다() {
    // 모르는 칸이 아는 칸 사이에 낀 run.json
    let src = r#"{"model":"m","zz_future":{"a":[1,{"b":null},2.50]},"state":"running","aa_first":true,"unit":3}"#;
    let mut run: RunFile = from_slice(src.as_bytes()).unwrap();
    run.state.set("paused".to_string());
    run.wait_until.set(Num::from_f64(1761015000.0)); // 읽은 적 없는 칸 — 끝에 붙는다
    let out: Value = from_slice(to_pretty_json(&run).unwrap().as_bytes()).unwrap();
    assert_eq!(
        out,
        json!({"model":"m","zz_future":{"a":[1,{"b":null},2.50]},"state":"paused","aa_first":true,"unit":3,"wait_until":1761015000})
    );
    assert_eq!(paths(&out)[..5], ["/model", "/zz_future", "/zz_future/a", "/zz_future/a[1]/b", "/state"]);
    assert_eq!(paths(&out)[5..], ["/aa_first", "/unit", "/wait_until"]);

    // 단위 안(queue.json) — 모르는 칸이 id 와 title 사이에 끼어 있다
    let src = r#"[{"id":"u1","x_future":{"deep":[true,null,1.50]},"title":"t","status":"pending"}]"#;
    let mut queue: QueueFile = from_slice(src.as_bytes()).unwrap();
    queue[0].status.set("running".to_string());
    let out: Value = from_slice(to_pretty_json(&queue).unwrap().as_bytes()).unwrap();
    assert_eq!(out, json!([{"id":"u1","x_future":{"deep":[true,null,1.50]},"title":"t","status":"running"}]));
    assert_eq!(paths(&out)[..5], ["[0]/id", "[0]/x_future", "[0]/x_future/deep", "[0]/title", "[0]/status"]);
}

#[test]
fn 견본의_순서가_바뀌지_않은_채_칸을_고칠_수_있다() {
    let bytes = fixture("run.json");
    let mut run: RunFile = from_slice(&bytes).unwrap();
    run.state.set("paused".to_string());
    run.reason.set("사람이 일시정지했습니다.".to_string());
    run.wait_until.set(Num::r3(1761016000.0004)); // null → 숫자
    let before: Value = from_slice(&bytes).unwrap();
    let after: Value = from_slice(to_pretty_json(&run).unwrap().as_bytes()).unwrap();
    assert_eq!(paths(&before), paths(&after));
    assert_eq!(after["state"], "paused");
    assert_eq!(after["wait_until"], 1761016000);
}

#[test]
fn 새로_만든_값은_이름_순으로_나간다() {
    let mut run = Run::default();
    run.title.set("제목".to_string());
    run.state.set("starting".to_string());
    run.unit.set_null();
    run.model.set("m".to_string());
    run.sessions.set(Vec::new());
    let out: Value = from_slice(to_pretty_json(&Ordered::new(run)).unwrap().as_bytes()).unwrap();
    let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["model", "sessions", "state", "title", "unit"]);
    assert_eq!(out["unit"], Value::Null);
}

#[test]
fn exec_env_가_돌아온다() {
    let src = r#"{"state":"running","exec_env":{"distro":"Ubuntu","key":"wsl:Ubuntu","kind":"wsl","sandbox":true,"unsandboxed_ok":{"at":1761012000.5,"by":"board"}},"unit":1}"#;
    let run: RunFile = from_slice(src.as_bytes()).unwrap();
    let env = run.exec_env.get().expect("exec_env 를 읽는다");
    assert_eq!(env.key, "wsl:Ubuntu");
    assert_eq!(env.kind, "wsl");
    assert_eq!(env.distro.as_deref(), Some("Ubuntu"));
    assert_eq!(env.sandbox, Some(true));
    let ok = env.unsandboxed_ok.as_ref().unwrap();
    assert_eq!((ok.at.as_f64(), ok.by.as_str()), (Some(1761012000.5), "board"));
    assert!(run.extra.is_empty(), "exec_env 는 extra 로 새지 않는다");

    let out = to_pretty_json(&run).unwrap();
    let a: Value = from_slice(src.as_bytes()).unwrap();
    let b: Value = from_slice(out.as_bytes()).unwrap();
    assert_eq!(a, b);
    assert_eq!(paths(&a), paths(&b));

    // 필수 key·kind 만 있는 네이티브 환경은 선택 칸을 null 로 쓰지 않고 뺀다
    let mut run = Run::default();
    run.exec_env.set(Ordered::new(ExecEnv { key: "pwsh".into(), kind: "pwsh".into(), ..ExecEnv::default() }));
    let out: Value = from_slice(to_pretty_json(&Ordered::new(run)).unwrap().as_bytes()).unwrap();
    assert_eq!(out, json!({"exec_env": {"key": "pwsh", "kind": "pwsh"}}));

    // 동의 기록을 달아 쓰고 다시 읽는다
    let mut env = ExecEnv { key: "wsl:Ubuntu".into(), kind: "wsl".into(), distro: Some("Ubuntu".into()), sandbox: Some(false), ..ExecEnv::default() };
    env.unsandboxed_ok = Some(UnsandboxedOk { at: Num::from_f64(1761012000.0), by: "cli".into() });
    let mut run = Run::default();
    run.exec_env.set(Ordered::new(env.clone()));
    let text = to_pretty_json(&Ordered::new(run)).unwrap();
    let back: RunFile = from_slice(text.as_bytes()).unwrap();
    assert_eq!(**back.exec_env.get().unwrap(), env);
}

#[test]
fn exec_env_가_없는_macos_파일은_없음으로_읽고_키를_더하지_않는다() {
    let run: RunFile = from_slice(&fixture("run.json")).unwrap();
    assert_eq!(run.exec_env, Slot::Absent);
    assert!(run.exec_env.get().is_none());
    let out = to_pretty_json(&run).unwrap();
    assert!(!out.contains("exec_env"), "{out}");
    let v: Value = from_slice(out.as_bytes()).unwrap();
    assert!(v.get("exec_env").is_none());
}

#[test]
fn 숫자_도우미() {
    assert_eq!(serde_json::to_string(&Num::from_f64(0.0)).unwrap(), "0");
    assert_eq!(serde_json::to_string(&Num::from_f64(1761000000.0)).unwrap(), "1761000000");
    assert_eq!(serde_json::to_string(&Num::from_f64(0.5)).unwrap(), "0.5");
    assert_eq!(serde_json::to_string(&Num::r3(1.23456)).unwrap(), "1.235");
    assert_eq!(serde_json::to_string(&Num::from_f64(f64::NAN)).unwrap(), "null");
    // 원본 num 처럼 숫자로 읽히는 글자도 받는다
    let n: Num = serde_json::from_str("\"12.5\"").unwrap();
    assert_eq!(n.as_f64(), Some(12.5));
    let n: Num = serde_json::from_str("true").unwrap();
    assert_eq!(n.as_f64(), None);
}

#[test]
fn 최상위가_객체가_아니면_읽기가_실패한다() {
    assert!(from_slice::<RunFile>(b"[1,2]").is_err());
    assert!(from_slice::<RunFile>(b"").is_err());
    assert!(from_slice::<QueueFile>(b"{}").is_err());
}
