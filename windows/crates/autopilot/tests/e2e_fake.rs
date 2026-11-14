// 끝까지 도는 시험 — 진짜 claude 없이(가짜 `windows/ci/fake-claude-stream.cjs`, node) 실제 어댑터(files·native_env·sqlite·git)와
// 실제 실행 파일(`claude-autopilot.exe`)로 실행 하나를 출발부터 끝까지 돌리고, 실행 폴더 파일과 기록 저장소 행이 약속대로 남는지 본다.
// 환경은 명령 프롬프트(cmd)와 Git Bash 둘(WSL 은 넣지 않는다). 작업 스케줄러는 쓰지 않는다: `start --no-schedule` 뒤 `run` 을 직접 띄운다.
// 전부 `CLAUDE_BOARD_HOME` 임시 폴더 안에서 돌아 실제 홈·실제 기록 저장소·작업 스케줄러를 건드리지 않는다.
// node 와 Git Bash 가 없으면 건너뛰지 않고 실패한다(빠진 채 초록이 되지 않게). 디버깅: 환경변수 E2E_KEEP=1 이면 임시 폴더를 남긴다.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Map, Value};

const CLI: &str = env!("CARGO_BIN_EXE_claude-autopilot");
const GIT_BASH: &str = r"C:\Program Files\Git\bin\bash.exe";
const RUN_LIMIT: Duration = Duration::from_secs(120);

// ── 도구 ─────────────────────────────────────────────

fn find_on_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|d| d.join(exe)).find(|p| p.is_file())
}

/// 시험이 claude 자리에 두는 파일: 저장소의 가짜 claude(`windows/ci/fake-claude-stream.cjs`)
fn fake_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("ci").join("fake-claude-stream.cjs")
}

/// `\\?\` 머리를 뗀 정규 경로. 짧은 이름(8.3)이 섞여 git 이 돌려주는 긴 이름과 어긋나지 않게 한다
fn plain_canonical(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap();
    let s = c.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn read_text(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(b) => {
            let t = String::from_utf8_lossy(&b).into_owned();
            if t.len() > 6000 {
                format!("{}…(앞 6000자)", t.chars().take(6000).collect::<String>())
            } else {
                t
            }
        }
        Err(e) => format!("<읽지 못함: {e}>"),
    }
}

fn read_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{} 을(를) 읽지 못했습니다: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{} 이(가) JSON 이 아닙니다: {e}\n{}", path.display(), read_text(path)))
}

/// 줄마다 JSON 하나인 파일(없으면 빈 목록)
fn read_lines_json(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or(Value::String(format!("<JSON 아님> {l}"))))
        .collect()
}

fn tail(path: &Path, n: usize) -> String {
    let t = std::fs::read_to_string(path).unwrap_or_else(|e| format!("<읽지 못함: {e}>"));
    let lines: Vec<&str> = t.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn num_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// 칸마다 따로 견주고 실패를 모았다가 한 번에 낸다. 실패 글에는 그 칸이 든 파일 내용을 붙인다
struct Checks {
    failures: Vec<String>,
    total: usize,
}

impl Checks {
    fn new() -> Self {
        Self { failures: Vec::new(), total: 0 }
    }

    fn that(&mut self, ok: bool, what: &str, ctx: &str) {
        self.total += 1;
        if !ok {
            self.failures.push(format!("실패: {what}\n--- 내용 ---\n{ctx}\n---"));
        }
    }

    fn eq(&mut self, got: Option<&Value>, want: Value, what: &str, ctx: &str) {
        self.that(got.is_some_and(|g| num_eq(g, &want)), &format!("{what} (기대 {want}, 실제 {})", got.map_or("<없음>".into(), Value::to_string)), ctx);
    }

    fn finish(self) {
        eprintln!("칸 {}개 중 실패 {}개", self.total, self.failures.len());
        assert!(self.failures.is_empty(), "끝까지 시험 실패 {}건:\n{}", self.failures.len(), self.failures.join("\n\n"));
    }
}

/// 시험 하나마다 새 임시 폴더 `T`
struct Fx {
    tmp: Option<tempfile::TempDir>,
    t: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let t = plain_canonical(tmp.path());
        std::fs::create_dir_all(t.join("home")).unwrap();
        Self { tmp: Some(tmp), t }
    }

    fn home(&self) -> PathBuf {
        self.t.join("home")
    }

    fn toy(&self) -> PathBuf {
        self.t.join("toy")
    }

    fn fake_log(&self) -> PathBuf {
        self.t.join("fake.log")
    }

    /// 실행 폴더 = `autopilot_root(false)\runs\<id>`
    fn run_dir(&self, id: &str) -> PathBuf {
        self.home().join("AppData").join("Local").join("Claude Board").join("autopilot").join("runs").join(id)
    }

    fn records_db(&self) -> PathBuf {
        self.home().join("AppData").join("Local").join("Claude Board").join("autopilot").join("private").join("records.sqlite")
    }

    /// `start`·`run` 둘 다에 주는 환경: 가짜 홈과 가짜 claude. 바깥의 같은 이름 변수는 먼저 지운다
    fn command(&self, claude: &str) -> Command {
        let mut c = Command::new(CLI);
        c.env_remove("CLAUDE_BOARD_CLAUDE");
        for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("FAKE_CLAUDE_")) {
            c.env_remove(k);
        }
        c.env("CLAUDE_BOARD_HOME", self.home())
            .env("CLAUDE_BOARD_CLAUDE", claude)
            .env("FAKE_CLAUDE_SCRIPT", self.t.join("script.json"))
            .env("FAKE_CLAUDE_STATE", self.t.join("script.state.json"))
            .env("FAKE_CLAUDE_LOG", self.fake_log())
            .env("FAKE_CLAUDE_MAX_MS", "120000")
            // 사용률 캐시 파일(TEMP 아래 .claude-usage-cache.json)을 시험 폴더로 돌려, 실제 보드의 캐시를 읽지도 쓰지도 않는다
            .env("TEMP", self.t.join("tmp"))
            .env("TMP", self.t.join("tmp"));
        c
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        if std::env::var_os("E2E_KEEP").is_some() {
            if let Some(tmp) = self.tmp.take() {
                eprintln!("E2E_KEEP: 임시 폴더를 남깁니다: {}", tmp.keep().display());
            }
        }
    }
}

/// 장난감 저장소: main 가지, 무시 파일 `result.txt`, 첫 커밋
fn make_toy(toy: &Path) {
    std::fs::create_dir_all(toy).unwrap();
    git(toy, &["init", "-q", "-b", "main"]);
    git(toy, &["config", "user.name", "e2e"]);
    git(toy, &["config", "user.email", "e2e@example.com"]);
    std::fs::write(toy.join(".gitignore"), "result.txt\n").unwrap();
    std::fs::write(toy.join("README.md"), "toy\n").unwrap();
    git(toy, &["add", ".gitignore", "README.md"]);
    git(toy, &["commit", "-q", "-m", "first"]);
}

fn write_json(path: &Path, v: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

/// 실행 폴더에 run.json·queue.json 을 쓴다. 판정 명령은 환경마다 다르다
fn make_run(dir: &Path, toy: &Path, env_key: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let toy_s = toy.to_string_lossy();
    write_json(
        &dir.join("run.json"),
        &json!({
            "title": "끝까지 시험", "model": "haiku", "project": "toy", "project_root": toy_s,
            "exec_env": {"key": env_key, "kind": env_key},
            "stopline": {"seven_day": 1.01, "five_hour": 1.01}
        }),
    );
    let check = if env_key == "cmd" { r#"findstr /c:"ok" result.txt"# } else { "grep -qx ok result.txt" };
    write_json(
        &dir.join("queue.json"),
        &json!([
            {"id": "u1", "title": "결과 파일 맞추기", "prompt": "result.txt 에 ok 를 쓰세요", "check": check, "cwd": toy_s},
            {"id": "u2", "title": "상한 시험", "prompt": "오래 걸리는 일", "check": check, "cwd": toy_s, "caps": {"minutes": 0.05}}
        ]),
    );
}

/// 사용률 캐시. 엔진은 첫 단위 전에 계정 사용률을 알아야 하는데(모르면 10분 기다린다) 이 판의 사용량 API 길은 늘 닫혀 있어
/// 캐시 파일만이 값을 줄 수 있다. 멈출 선(1.01)을 넘지 않는 낮은 값을 방금 쓴 것으로 둔다(15분 안만 믿는다)
fn make_usage_cache(tmp: &Path) {
    std::fs::create_dir_all(tmp).unwrap();
    write_json(
        &tmp.join(".claude-usage-cache.json"),
        &json!({
            "five_hour": {"utilization": 3.0, "resets_at": "2099-01-01T05:00:00+00:00"},
            "seven_day": {"utilization": 10.0, "resets_at": "2099-01-07T00:00:00+00:00"}
        }),
    );
}

fn make_script(path: &Path) {
    write_json(
        path,
        &json!({"turns": [
            {"text": "첫 시도", "writes": [{"path": "result.txt", "content": "no\n"}], "cost": 0.01},
            {"text": "고침", "writes": [{"path": "result.txt", "content": "ok\n"}], "cost": 0.02},
            {"text": "오래 걸리는 일", "delay_ms": 30000, "cost": 0.01}
        ]}),
    );
}

/// 폴더 안 이름들(상대 경로, `/` 구분, 정렬)
fn tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &p, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// 한 행을 {칸 이름: 값} 으로
fn sql_rows(db: &Connection, sql: &str) -> Vec<Map<String, Value>> {
    let mut st = db.prepare(sql).unwrap();
    let names: Vec<String> = st.column_names().map(|s| s.to_string()).collect();
    st.query_map([], |r| {
        let mut m = Map::new();
        for (i, name) in names.iter().enumerate() {
            let v = match r.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => json!(n),
                ValueRef::Real(f) => json!(f),
                ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Blob(_) => Value::String("<blob>".into()),
            };
            m.insert(name.clone(), v);
        }
        Ok(m)
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

/// 시험이 띄운 프로세스가 남았는지: 가짜 claude 가 기록에 남긴 pid(node)와, 명령줄에 임시 폴더가 든 node·cmd·bash·claude-autopilot
fn stray_processes(t: &Path, fake_pids: &[u64]) -> String {
    let pids = fake_pids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
    let script = format!(
        "$t = '{}'; $me = $PID; \
         $all = Get-CimInstance Win32_Process; \
         $byPid = $all | Where-Object {{ @({pids}) -contains $_.ProcessId -and $_.Name -match '^(node|cmd|bash|claude-autopilot.*)\\.exe$' }}; \
         $byCmd = $all | Where-Object {{ $_.ProcessId -ne $me -and $_.Name -match '^(node|cmd|bash|findstr|grep|claude-autopilot.*)\\.exe$' -and $_.CommandLine -and $_.CommandLine.IndexOf($t, [StringComparison]::OrdinalIgnoreCase) -ge 0 }}; \
         @($byPid) + @($byCmd) | Sort-Object ProcessId -Unique | ForEach-Object {{ $_.ProcessId.ToString() + ' ' + $_.Name + ' ' + $_.CommandLine }}",
        t.to_string_lossy().replace('\'', "''"),
    );
    let o = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output().unwrap();
    assert!(o.status.success(), "프로세스 검사가 실패했습니다: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

// ── 시험 ─────────────────────────────────────────────

#[test]
fn e2e_prereqs_present() {
    let mut missing: Vec<String> = Vec::new();
    if find_on_path("node.exe").is_none() {
        missing.push("node.exe 가 PATH 에 없습니다".to_string());
    }
    if !Path::new(GIT_BASH).is_file() {
        missing.push(format!("Git Bash 가 없습니다: {GIT_BASH}"));
    }
    if find_on_path("git.exe").is_none() {
        missing.push("git.exe 가 PATH 에 없습니다".to_string());
    }
    if !fake_script().is_file() {
        missing.push(format!("가짜 claude 가 없습니다: {}", fake_script().display()));
    }
    assert!(missing.is_empty(), "끝까지 시험의 준비물이 빠졌습니다:\n{}", missing.join("\n"));
}

#[test]
fn e2e_cmd() {
    run_e2e("cmd");
}

#[test]
fn e2e_git_bash() {
    run_e2e("git-bash");
}

fn run_e2e(env_key: &str) {
    let began = Instant::now();
    let node = find_on_path("node.exe").expect("node.exe 가 PATH 에 있어야 한다(e2e_prereqs_present 참고)");
    assert!(Path::new(GIT_BASH).is_file(), "Git Bash 가 있어야 한다: {GIT_BASH}");
    let fake = plain_canonical(&fake_script());
    let claude = format!("{}|{}", node.display(), fake.display());

    let fx = Fx::new();
    let id = format!("e2e-{env_key}");
    let toy = fx.toy();
    make_toy(&toy);
    make_script(&fx.t.join("script.json"));
    make_usage_cache(&fx.t.join("tmp"));
    let dir = fx.run_dir(&id);
    make_run(&dir, &toy, env_key);

    // 출발
    let o = fx
        .command(&claude)
        .args(["start"])
        .arg(&dir)
        .args(["--accept-unsandboxed", "--no-schedule", "--json"])
        .output()
        .unwrap();
    let start_out = String::from_utf8_lossy(&o.stdout).into_owned();
    let start_err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(0), "start 종료 코드\n{start_out}\n{start_err}");
    let started: Value = serde_json::from_str(start_out.trim()).unwrap_or_else(|e| panic!("start 출력이 JSON 한 줄이 아닙니다: {e}\n{start_out}\n{start_err}"));
    assert_eq!(started["ok"], json!(true), "start 결과\n{start_out}");
    let hash = started["hash"].as_str().unwrap_or_else(|| panic!("start 결과에 hash 가 없습니다\n{start_out}")).to_string();

    // 엔진
    let mut child = fx.command(&claude).args(["run"]).arg(&dir).arg(&hash).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let run_began = Instant::now();
    let exit_code = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st.code();
        }
        if run_began.elapsed() > RUN_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "엔진이 {}초 안에 끝나지 않아 끝냈습니다.\nevents.jsonl 끝 30줄:\n{}\nengine.log:\n{}",
                RUN_LIMIT.as_secs(),
                tail(&dir.join("events.jsonl"), 30),
                read_text(&dir.join("engine.log"))
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let run_secs = run_began.elapsed().as_secs_f64();
    let events_path = dir.join("events.jsonl");
    let event_lines = std::fs::read_to_string(&events_path).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).count();
    eprintln!("[{env_key}] 엔진 종료 코드 {exit_code:?}, run {run_secs:.1}초, events.jsonl {event_lines}줄");
    eprintln!("[{env_key}] 실행 폴더 이름들:\n{}", tree(&dir).join("\n"));
    eprintln!("[{env_key}] events.jsonl 전체:\n{}", read_text(&events_path));

    let mut c = Checks::new();
    c.that(exit_code == Some(0), &format!("엔진 종료 코드 0 (실제 {exit_code:?})"), &format!("engine.log:\n{}\n\nevents.jsonl 끝 30줄:\n{}", read_text(&dir.join("engine.log")), tail(&events_path, 30)));
    check_run_json(&mut c, &dir, &id, env_key, &toy);
    check_units(&mut c, &dir, env_key);
    check_events(&mut c, &events_path);
    check_starts_log(&mut c, &dir);
    let fake_pids = check_fake_log(&mut c, &fx.fake_log());
    check_records(&mut c, &fx, &id, &dir, &toy);
    check_git(&mut c, &toy, &id);

    let strays = stray_processes(&fx.t, &fake_pids);
    c.that(strays.is_empty(), "시험이 띄운 프로세스(node·cmd·bash·claude-autopilot)가 남지 않았다", &strays);
    eprintln!("[{env_key}] 남은 프로세스 검사: {}", if strays.is_empty() { "없음".to_string() } else { strays.clone() });
    eprintln!("[{env_key}] 전체 {:.1}초", began.elapsed().as_secs_f64());
    c.finish();
}

fn check_run_json(c: &mut Checks, dir: &Path, id: &str, env_key: &str, toy: &Path) {
    let path = dir.join("run.json");
    let ctx = read_text(&path);
    let r = read_json(&path);
    let toy_s = toy.to_string_lossy().into_owned();
    c.eq(r.get("state"), json!("done"), "run.json state", &ctx);
    c.eq(r.get("reason"), json!("대기열을 모두 마쳤습니다."), "run.json reason", &ctx);
    for key in ["started", "ended", "wrapped"] {
        c.that(r.get(key).is_some_and(Value::is_number), &format!("run.json {key} 가 숫자"), &ctx);
    }
    c.that(r.get("wait_until") == Some(&Value::Null), "run.json wait_until 이 null (칸이 있고 null)", &ctx);
    c.eq(r.get("unit"), json!(2), "run.json unit", &ctx);
    c.eq(r.pointer("/exec_env/key"), json!(env_key), "run.json exec_env.key", &ctx);
    c.eq(r.pointer("/exec_env/kind"), json!(env_key), "run.json exec_env.kind", &ctx);
    c.eq(r.pointer("/exec_env/sandbox"), json!(false), "run.json exec_env.sandbox", &ctx);
    c.eq(r.pointer("/exec_env/unsandboxed_ok/by"), json!("cli"), "run.json exec_env.unsandboxed_ok.by", &ctx);
    c.eq(r.get("launchd_label"), json!(claude_board_autopilot::cli::label_for_dir(false, dir)), "run.json launchd_label", &ctx);
    let sessions = r.get("sessions").and_then(Value::as_array).cloned().unwrap_or_default();
    c.that(sessions.len() == 2 && sessions[0] != sessions[1] && sessions.iter().all(Value::is_string), "run.json sessions 가 서로 다른 글 2개", &ctx);
    let repos = r.pointer("/branches/repos").and_then(Value::as_object);
    let repo = repos.and_then(|m| m.get(&toy_s));
    c.that(repo.is_some(), &format!("run.json branches.repos 에 {toy_s} 칸이 있음 (실제 키 {:?})", repos.map(|m| m.keys().collect::<Vec<_>>())), &ctx);
    if let Some(repo) = repo {
        c.eq(repo.pointer("/base/branch"), json!("main"), "branches.repos[toy].base.branch", &ctx);
        c.eq(repo.get("work"), json!(format!("autopilot/{id}")), "branches.repos[toy].work", &ctx);
        c.eq(repo.get("restored"), json!(true), "branches.repos[toy].restored", &ctx);
    }
    c.that(r.get("records_error").is_none(), "run.json 에 records_error 가 없다", &ctx);
}

fn check_units(c: &mut Checks, dir: &Path, env_key: &str) {
    let qpath = dir.join("queue.json");
    let q = read_json(&qpath);
    let statuses: Vec<Value> = q.as_array().map(|a| a.iter().map(|u| u.get("status").cloned().unwrap_or(Value::Null)).collect()).unwrap_or_default();
    c.that(statuses == [json!("done"), json!("capped")], &format!("queue.json status [done, capped] (실제 {statuses:?})"), &read_text(&qpath));

    let u1p = dir.join("units").join("1").join("engine.json");
    let ctx1 = read_text(&u1p);
    let u1 = read_json(&u1p);
    c.eq(u1.get("status"), json!("done"), "units/1/engine.json status", &ctx1);
    c.eq(u1.get("try"), json!(1), "units/1/engine.json try", &ctx1);
    c.eq(u1.get("errors"), json!(0), "units/1/engine.json errors", &ctx1);
    let checks = u1.get("checks").and_then(Value::as_array).cloned().unwrap_or_default();
    c.that(checks.len() == 2, &format!("units/1/engine.json checks 가 2개 (실제 {})", checks.len()), &ctx1);
    c.that(checks.first().and_then(|x| x.get("exit")).and_then(Value::as_i64).is_some_and(|e| e != 0), "units/1 checks[0].exit != 0", &ctx1);
    c.that(checks.get(1).and_then(|x| x.get("exit")).and_then(Value::as_i64) == Some(0), "units/1 checks[1].exit == 0", &ctx1);
    c.eq(checks.first().and_then(|x| x.get("evidence")), json!("evidence/check-0"), "units/1 checks[0].evidence", &ctx1);
    c.eq(u1.get("session_started"), json!(true), "units/1/engine.json session_started", &ctx1);
    c.eq(u1.get("claude_runs"), json!(2), "units/1/engine.json claude_runs", &ctx1);
    c.eq(u1.get("result_ok"), json!(false), "units/1/engine.json result_ok", &ctx1);

    let u2p = dir.join("units").join("2").join("engine.json");
    let ctx2 = read_text(&u2p);
    let u2 = read_json(&u2p);
    c.eq(u2.get("status"), json!("capped"), "units/2/engine.json status", &ctx2);
    c.that(u2.get("reason").and_then(Value::as_str).is_some_and(|s| s.starts_with("시간 상한에 닿아 멈췄습니다")), "units/2/engine.json reason 이 '시간 상한에 닿아 멈췄습니다' 로 시작", &ctx2);
    c.that(u2.pointer("/git/status").is_some(), "units/2/engine.json git.status 칸이 있음", &ctx2);
    c.that(u2.get("active_sec").and_then(Value::as_f64).is_some_and(|s| s >= 3.0), "units/2/engine.json active_sec >= 3", &ctx2);

    let ev = dir.join("units").join("1").join("evidence");
    for n in 0..2 {
        for name in ["output.log", "summary.json"] {
            let p = ev.join(format!("check-{n}")).join(name);
            c.that(p.is_file(), &format!("units/1/evidence/check-{n}/{name} 가 있음"), &tree(&dir.join("units").join("1")).join("\n"));
        }
    }
    let sp = ev.join("check-0").join("summary.json");
    let summary = std::fs::read(&sp).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    c.eq(summary.as_ref().and_then(|s| s.get("env")), json!(env_key), "units/1/evidence/check-0/summary.json env", &read_text(&sp));

    let s1 = dir.join("units").join("1").join("stream.jsonl");
    let results = read_lines_json(&s1).iter().filter(|v| v.get("type") == Some(&json!("result"))).count();
    c.that(results >= 2, &format!("units/1/stream.jsonl 에 result 줄 2개 이상 (실제 {results})"), &read_text(&s1));
    let s2 = dir.join("units").join("2").join("stream.jsonl");
    let aborted = read_lines_json(&s2).iter().any(|v| v.get("type") == Some(&json!("result")) && v.get("subtype") == Some(&json!("error_during_execution")));
    c.that(aborted, "units/2/stream.jsonl 에 error_during_execution result 줄(멈춤의 결과)", &read_text(&s2));
}

const EXPECTED_EVENTS: [&str; 12] = [
    "자율주행을 시작합니다. 단위 2개를",
    "단위 1/2 시작: 결과 파일 맞추기",
    "판정 명령을 돌립니다.",
    "판정 실패(종료 코드",
    "판정 명령을 돌립니다(다시 1번째).",
    "판정 통과(",
    "단위 1/2 통과: 판정 명령을 통과했습니다.",
    "모델이 결과 기록(result.json)을 남기지 않았습니다.",
    "단위 2/2 시작: 상한 시험",
    "claude 를 멈춥니다.",
    "단위 2/2 상한에서 멈춤: 시간 상한에 닿아 멈췄습니다.",
    "자율주행을 마쳤습니다. 통과 1, 미통과 0, 상한에서 멈춤 1.",
];

fn check_events(c: &mut Checks, path: &Path) {
    let ctx = read_text(path);
    let lines = read_lines_json(path);
    let well_formed = |v: &Value| {
        v.as_object().is_some_and(|o| {
            o.len() == 3
                && o.get("ts").is_some_and(Value::is_number)
                && o.get("who").and_then(Value::as_str).is_some_and(|w| matches!(w, "board" | "model" | "engine"))
                && o.get("text").is_some_and(Value::is_string)
        })
    };
    let bad: Vec<&Value> = lines.iter().filter(|v| !well_formed(v)).collect();
    c.that(bad.is_empty(), &format!("events.jsonl 모든 줄이 {{ts 숫자, who board|model|engine, text 글}} 꼴 (어긋난 {}줄: {:?})", bad.len(), bad.iter().take(3).collect::<Vec<_>>()), &ctx);

    let texts: Vec<&str> = lines.iter().filter_map(|v| v.get("text").and_then(Value::as_str)).collect();
    let mut from = 0usize;
    for (i, want) in EXPECTED_EVENTS.iter().enumerate() {
        let found = texts[from.min(texts.len())..].iter().position(|t| t.starts_with(want));
        c.that(found.is_some(), &format!("events.jsonl {}번째 글 '{want}' 가 앞 글 다음에 나타남 ({from}번째 줄 이후)", i + 1), &ctx);
        match found {
            Some(p) => from += p + 1,
            None => break,
        }
    }
}

fn check_starts_log(c: &mut Checks, dir: &Path) {
    let path = dir.join("starts.log");
    let ctx = read_text(&path);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    c.that(lines.len() == 1, &format!("starts.log 한 줄 (실제 {}줄)", lines.len()), &ctx);
    let cols: Vec<&str> = lines.first().map(|l| l.split('\t').collect()).unwrap_or_default();
    c.that(cols.get(2) == Some(&"normal"), "starts.log 세 번째 칸 normal", &ctx);
    c.that(cols.contains(&"prev=starting"), "starts.log 에 prev=starting", &ctx);
}

/// 가짜 claude 기록을 보고, 그 프로세스 pid 들을 돌려준다
fn check_fake_log(c: &mut Checks, path: &Path) -> Vec<u64> {
    let ctx = read_text(path);
    let entries = read_lines_json(path);
    let pids: Vec<u64> = entries.iter().filter_map(|e| e.get("pid").and_then(Value::as_u64)).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let starts: Vec<&Value> = entries.iter().filter(|e| e.get("event") == Some(&json!("start"))).collect();
    c.that(starts.len() == 3, &format!("fake.log start 가 3개 (실제 {})", starts.len()), &ctx);
    let argv_of = |s: &Value| -> Vec<String> { s.get("argv").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default() };
    let value_of = |argv: &[String], flag: &str| argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1)).cloned();
    if starts.len() == 3 {
        let a: Vec<Vec<String>> = starts.iter().map(|s| argv_of(s)).collect();
        let first = value_of(&a[0], "--session-id");
        let resumed = value_of(&a[1], "--resume");
        let third = value_of(&a[2], "--session-id");
        c.that(first.is_some(), "fake.log 첫째 start argv 에 --session-id", &ctx);
        c.that(resumed.is_some() && resumed == first, &format!("fake.log 둘째 start 의 --resume 이 첫째의 --session-id 와 같음 ({resumed:?} / {first:?})"), &ctx);
        c.that(third.is_some() && third != first, &format!("fake.log 셋째 start 의 --session-id 가 첫째와 다름 ({third:?} / {first:?})"), &ctx);
        for (i, argv) in a.iter().enumerate() {
            c.that(argv.iter().any(|x| x == "--settings"), &format!("fake.log {}번째 start 에 --settings", i + 1), &ctx);
            c.that(argv.iter().any(|x| x == "--append-system-prompt-file"), &format!("fake.log {}번째 start 에 --append-system-prompt-file", i + 1), &ctx);
            let settings: Option<Value> = starts[i].get("settings").and_then(Value::as_str).and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok());
            c.that(
                settings.as_ref().and_then(|s| s.pointer("/sandbox/enabled")) == Some(&json!(false)),
                &format!("fake.log {}번째 start 의 settings 파일 내용 sandbox.enabled == false (읽힌 settings {:?})", i + 1, starts[i].get("settings")),
                &ctx,
            );
        }
        let third_pid = starts[2].get("pid").and_then(Value::as_u64);
        let signalled = entries.iter().any(|e| e.get("pid").and_then(Value::as_u64) == third_pid && matches!(e.get("event").and_then(Value::as_str), Some("signal" | "interrupt")));
        c.that(signalled, "fake.log 셋째 프로세스에 signal(또는 interrupt) 기록", &ctx);
    }
    pids
}

fn check_records(c: &mut Checks, fx: &Fx, id: &str, dir: &Path, toy: &Path) {
    let dbp = fx.records_db();
    let db = match Connection::open_with_flags(&dbp, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(db) => db,
        Err(e) => {
            c.that(false, &format!("기록 저장소를 읽기 전용으로 엽니다: {e}"), &dbp.display().to_string());
            return;
        }
    };
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    c.that(version == 5, &format!("기록 저장소 user_version 5 (실제 {version})"), &dbp.display().to_string());

    let dump = |sql: &str| format!("{sql}\n{:#?}", sql_rows(&db, sql));
    let toy_s = toy.to_string_lossy().into_owned();

    let p = sql_rows(&db, "SELECT * FROM projects");
    let pctx = dump("SELECT * FROM projects");
    c.that(p.len() == 1, &format!("projects 1행 (실제 {})", p.len()), &pctx);
    if let Some(p) = p.first() {
        c.eq(p.get("id"), json!("toy"), "projects.id", &pctx);
        c.eq(p.get("root"), json!(toy_s), "projects.root (어댑터가 쓴 글 그대로)", &pctx);
        c.eq(p.get("kind"), json!("single"), "projects.kind", &pctx);
    }

    let r = sql_rows(&db, "SELECT * FROM runs");
    let rctx = dump("SELECT * FROM runs");
    c.that(r.len() == 1, &format!("runs 1행 (실제 {})", r.len()), &rctx);
    if let Some(r) = r.first() {
        c.eq(r.get("id"), json!(id), "runs.id", &rctx);
        c.eq(r.get("state"), json!("done"), "runs.state", &rctx);
        for (k, want) in [("units_total", 2), ("units_done", 1), ("units_failed", 0), ("units_capped", 1), ("units_stopped", 0)] {
            c.eq(r.get(k), json!(want), &format!("runs.{k}"), &rctx);
        }
        c.eq(r.get("model"), json!("haiku"), "runs.model", &rctx);
        c.eq(r.get("origin"), json!("engine"), "runs.origin", &rctx);
        c.that(r.get("cost_usd").and_then(Value::as_f64).is_some_and(|x| x > 0.0), "runs.cost_usd > 0", &rctx);
        c.eq(r.get("run_dir"), json!(dir.to_string_lossy()), "runs.run_dir (실행 폴더)", &rctx);
    }

    let u = sql_rows(&db, "SELECT * FROM units ORDER BY n");
    let uctx = dump("SELECT * FROM units ORDER BY n");
    c.that(u.len() == 2, &format!("units 2행 (실제 {})", u.len()), &uctx);
    if let [u1, u2] = u.as_slice() {
        c.eq(u1.get("n"), json!(1), "units[0].n", &uctx);
        c.eq(u1.get("status"), json!("done"), "units n=1 status", &uctx);
        c.eq(u1.get("tries"), json!(1), "units n=1 tries", &uctx);
        c.eq(u1.get("check_exit"), json!(0), "units n=1 check_exit", &uctx);
        c.eq(u1.get("unit_id"), json!("u1"), "units n=1 unit_id", &uctx);
        c.eq(u2.get("n"), json!(2), "units[1].n", &uctx);
        c.eq(u2.get("status"), json!("capped"), "units n=2 status", &uctx);
        c.eq(u2.get("tries"), json!(0), "units n=2 tries", &uctx);
        c.eq(u2.get("unit_id"), json!("u2"), "units n=2 unit_id", &uctx);
    }
}

fn check_git(c: &mut Checks, toy: &Path, id: &str) {
    let branch = git(toy, &["branch", "--show-current"]);
    let work = format!("autopilot/{id}");
    let branches = git(toy, &["branch", "--list"]);
    let status = git(toy, &["status", "--porcelain"]);
    c.that(branch == "main", &format!("T\\toy 의 지금 가지 main (실제 {branch})"), &branches);
    c.that(branches.lines().any(|l| l.trim_start_matches(['*', ' ']).trim() == work), &format!("가지 {work} 가 있음"), &branches);
    c.that(status.is_empty(), "git status --porcelain 이 빔", &status);
}
