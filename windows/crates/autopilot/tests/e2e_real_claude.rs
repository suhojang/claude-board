// 진짜 claude 로 단위 하나가 끝까지 통과하는지 보는 시험. `#[ignore]` — 모델을 부르고(haiku, 약 0.04 USD) 로그인한 claude.exe 가 있어야 하므로
// 사람이 `cargo test -p claude-board-autopilot --test e2e_real_claude -- --ignored --nocapture` 로 돌릴 때만 돈다.
// 실제 어댑터(files·native_env·sqlite·git)와 실제 실행 파일(`claude-autopilot.exe`)을 쓰고 `CLAUDE_BOARD_HOME` 임시 폴더 안에서만 돈다.
// 환경은 명령 프롬프트(cmd). 판정 명령은 파일 있음 확인이다: `if exist` 는 거짓일 때 종료 코드를 바꾸지 않으므로 `exit /b` 를 둘 다 적는다.
// 사용률: 첫 단위 전에 엔진이 계정 사용률을 알아야 해서 캐시 파일을 두는데, 값이 진짜 사용률과 다르면 stream 이 알려 주는 진짜 값과의 차이를
// 주간 상한(`weekly_pp`)이 단위가 쓴 양으로 센다. 그래서 이 시험은 단위 `weekly_pp` 상한을 100 으로 열어 둔다(비용·시간 상한은 그대로).
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const CLI: &str = env!("CARGO_BIN_EXE_claude-autopilot");
const RUN_LIMIT: Duration = Duration::from_secs(300);

fn plain_canonical(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap();
    let s = c.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn write_json(path: &Path, v: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{} 을(를) 읽지 못했습니다: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{} 이(가) JSON 이 아닙니다: {e}", path.display()))
}

fn tail(path: &Path, n: usize) -> String {
    let t = std::fs::read_to_string(path).unwrap_or_else(|e| format!("<읽지 못함: {e}>"));
    let lines: Vec<&str> = t.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[test]
#[ignore = "진짜 claude(haiku)를 부른다: 약 0.04 USD, 로그인한 claude.exe 필요"]
fn real_claude_unit_passes_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let t = plain_canonical(tmp.path());
    let home = t.join("home");
    let temp = t.join("tmp");
    let toy = t.join("toy");
    std::fs::create_dir_all(&temp).unwrap();
    let dir = home.join("AppData").join("Local").join("Claude Board").join("autopilot").join("runs").join("real-e2e");
    std::fs::create_dir_all(&dir).unwrap();

    std::fs::create_dir_all(&toy).unwrap();
    git(&toy, &["init", "-q", "-b", "main"]);
    git(&toy, &["config", "user.name", "e2e"]);
    git(&toy, &["config", "user.email", "e2e@example.com"]);
    std::fs::write(toy.join("README.md"), "toy\n").unwrap();
    git(&toy, &["add", "README.md"]);
    git(&toy, &["commit", "-q", "-m", "first"]);

    write_json(
        &temp.join(".claude-usage-cache.json"),
        &json!({
            "five_hour": {"utilization": 3.0, "resets_at": "2099-01-01T05:00:00+00:00"},
            "seven_day": {"utilization": 10.0, "resets_at": "2099-01-07T00:00:00+00:00"}
        }),
    );
    let toy_s = toy.to_string_lossy().into_owned();
    write_json(
        &dir.join("run.json"),
        &json!({
            "title": "real claude", "model": "haiku", "project": "toy", "project_root": toy_s,
            "exec_env": {"key": "cmd", "kind": "cmd"},
            "stopline": {"seven_day": 1.01, "five_hour": 1.01}
        }),
    );
    write_json(
        &dir.join("queue.json"),
        &json!([{
            "id": "u1", "title": "create hello.txt",
            "prompt": "Create a file named hello.txt in the current directory containing exactly the word ok (use the Write tool). Do nothing else.",
            "check": "if exist hello.txt (exit /b 0) else (exit /b 1)",
            "cwd": toy_s,
            "caps": {"minutes": 10, "usd": 0.5, "weekly_pp": 100}
        }]),
    );

    // 진짜 claude 를 쓰므로 시험용 claude 변수는 지운다. 사용률 캐시는 TEMP 아래에서 읽는다
    let command = || {
        let mut c = Command::new(CLI);
        c.env_remove("CLAUDE_BOARD_CLAUDE");
        for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("FAKE_CLAUDE_")) {
            c.env_remove(k);
        }
        c.env("CLAUDE_BOARD_HOME", &home).env("TEMP", &temp).env("TMP", &temp);
        c
    };
    let o = command().arg("start").arg(&dir).args(["--accept-unsandboxed", "--no-schedule", "--json"]).output().unwrap();
    let start_out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert_eq!(o.status.code(), Some(0), "start 종료 코드\n{start_out}\n{}", String::from_utf8_lossy(&o.stderr));
    let started: Value = serde_json::from_str(start_out.trim()).unwrap();
    let hash = started["hash"].as_str().unwrap().to_string();

    let mut child = command().arg("run").arg(&dir).arg(&hash).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let began = Instant::now();
    let exit_code = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st.code();
        }
        if began.elapsed() > RUN_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("엔진이 {}초 안에 끝나지 않았습니다.\nevents.jsonl:\n{}", RUN_LIMIT.as_secs(), tail(&dir.join("events.jsonl"), 30));
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let events = tail(&dir.join("events.jsonl"), 40);
    let run = read_json(&dir.join("run.json"));
    let unit = read_json(&dir.join("units").join("1").join("engine.json"));
    let ctx = format!("엔진 종료 코드 {exit_code:?}\nevents.jsonl:\n{events}\nengine.log:\n{}", tail(&dir.join("engine.log"), 30));
    assert_eq!(exit_code, Some(0), "{ctx}");
    assert_eq!(run["state"], json!("done"), "{ctx}");
    assert_eq!(unit["status"], json!("done"), "{ctx}");
    assert_eq!(unit["checks"][0]["exit"], json!(0), "{ctx}");
    assert!(unit["cost_usd"].as_f64().is_some_and(|c| c > 0.0 && c < 0.5), "비용 {}\n{ctx}", unit["cost_usd"]);
    assert!(std::fs::read_to_string(toy.join("hello.txt")).is_ok_and(|s| s.trim() == "ok"), "hello.txt 에 ok 가 있어야 한다\n{ctx}");
    eprintln!("단위 상태 {} · 비용 {} USD · {}초", unit["status"], unit["cost_usd"], began.elapsed().as_secs());
}
