//! hook 공유 시험 표 — macOS hook(`state.py`·`notify.py`·`hooks.py`)의 `main()` 을 끝까지 돌려 만든 표(저장소 맨 위 `hook-table/`)를
//! Rust 코어의 [`state::run`]·[`notify::prepare`]·[`hooks::add`] 결과와 견준다. 표는 `python3 -I hook-table/gen.py` 로 만든다.
//! 표시(`@DIR`·`@<k>`·`@seed`·`@LIVE_PID` …)·뺀 것·유지 규칙은 `hook-table/README.md`.
//!
//! 바깥은 표를 만든 쪽과 같게 가짜로 둔다: claude 프로세스(pid 4242, 고정 tty, 단계마다의 실행 인자), 4242 의 생사(늘 산 것),
//! 현재 폴더, `CLAUDE_PROJECT_DIR`. 시계는 진짜이고 단계마다 앞뒤 시각을 재어 그 사이 값을 `@<k>` 로 바꾼다.
//! 다른 pid 의 생사·파일 읽기는 진짜로 한다 — 그래서 unix 에서만 돈다(Windows 러너는 `@PARENT_PID` 자리가 맞지 않는다).
#![cfg(unix)]

mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use claude_board_core::state::{self, ClaudeProcess, Env, SystemEnv};
use claude_board_core::{clock, hooks, notify, proc, quote, Record};
use serde_json::{json, Map, Value};

/// 통과해도 남기는 기록 — 시험 틀이 가로채지 않게 stderr 에 바로 쓴다.
fn note(msg: &str) {
    let _ = writeln!(std::io::stderr(), "[hook-table] {msg}");
}

fn table(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../hook-table").join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} 를 읽지 못했다: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 가 JSON 이 아니다: {e}", path.display()))
}

/// 표의 진짜 pid 표시 — 표를 만든 쪽과 같은 뜻의 이 프로세스 값.
fn real_pids() -> Vec<(&'static str, u32)> {
    let mut child = Command::new("true").spawn().expect("true 를 띄우지 못했다");
    let dead = child.id();
    child.wait().expect("true 를 거두지 못했다");
    let pids = vec![("@LIVE_PID", std::process::id()), ("@PARENT_PID", std::os::unix::process::parent_id()), ("@DEAD_PID", dead)];
    assert!(proc::alive(pids[0].1, None) && proc::alive(pids[1].1, None) && !proc::alive(dead, None), "{pids:?}");
    pids
}

/// 표시를 이 실행의 값으로 채우고(fill), 결과 값을 다시 표시로 바꾼다(mark).
struct Marks {
    root: String,
    t0: f64,
    pids: Vec<(&'static str, u32)>,
    seeds: Vec<(f64, String)>,
    /// 단계마다 [hook 을 부르기 직전, 끝난 직후] 시각
    steps: Vec<(f64, f64)>,
}

impl Marks {
    fn new(root: &Path, pids: &[(&'static str, u32)]) -> Self {
        Marks { root: root.to_str().expect("임시 폴더 경로").to_string(), t0: clock::now(), pids: pids.to_vec(), seeds: Vec::new(), steps: Vec::new() }
    }

    fn seed_time(&mut self, mark: &str) -> f64 {
        let n: f64 = if mark == "@seed" { 1000.0 } else { mark["@seed-".len()..].parse().expect("@seed-<n>") };
        let value = self.t0 - n;
        if !self.seeds.iter().any(|(v, _)| *v == value) {
            self.seeds.push((value, mark.to_string()));
        }
        value
    }

    fn fill(&mut self, v: &Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), self.fill(x))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.fill(x)).collect()),
            Value::String(s) => {
                if let Some((_, pid)) = self.pids.iter().find(|(m, _)| m == s) {
                    return json!(pid);
                }
                if s == "@seed" || s.starts_with("@seed-") {
                    return json!(self.seed_time(s));
                }
                if let Some(n) = s.strip_prefix("@ms:seed-") {
                    let n: f64 = n.parse().expect("@ms:seed-<n>");
                    return json!(((self.t0 - n) * 1000.0).round() as u64);
                }
                let mut out = s.replace("@DIR", &self.root);
                for (m, pid) in &self.pids {
                    out = out.replace(m, &pid.to_string());
                }
                Value::String(out)
            }
            _ => v.clone(),
        }
    }

    fn mark(&self, v: &Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), self.mark(x))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.mark(x)).collect()),
            Value::String(s) => Value::String(s.replace(&self.root, "@DIR")),
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    if let Some(k) = self.steps.iter().position(|&(a, b)| a <= f && f <= b) {
                        return json!(format!("@{k}"));
                    }
                    if let Some((_, label)) = self.seeds.iter().find(|(x, _)| *x == f) {
                        return json!(label);
                    }
                }
                match n.as_u64().and_then(|u| self.pids.iter().find(|(_, p)| u64::from(*p) == u)) {
                    Some((label, _)) => json!(label),
                    None => v.clone(),
                }
            }
            _ => v.clone(),
        }
    }

    /// 한 단계를 시각 구간으로 잰다 — 앞 단계 구간과 겹치지 않게 시계가 넘어갈 때까지 기다린다.
    fn step<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let floor = self.steps.last().map_or(self.t0, |s| s.1);
        let mut before = clock::now();
        while before <= floor {
            std::thread::yield_now();
            before = clock::now();
        }
        let out = f();
        self.steps.push((before, clock::now()));
        out
    }
}

/// 시나리오마다 임시 폴더를 비우고 `notifier/sessions` 만 남긴다(표를 만든 쪽과 같다).
fn reset(root: &Path) {
    for e in fs::read_dir(root).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            fs::remove_dir_all(&p).unwrap();
        } else {
            fs::remove_file(&p).unwrap();
        }
    }
    fs::create_dir_all(root.join("notifier/sessions")).unwrap();
}

/// 심을 파일 — json(객체 하나)·lines(JSONL)·text, age 가 있으면 수정 시각을 그만큼 앞당긴다.
fn plant(seed: Option<&Value>, m: &mut Marks) {
    let Some(files) = seed.and_then(Value::as_array) else { return };
    let path_of = |m: &mut Marks, f: &Value| PathBuf::from(&m.root).join(m.fill(&f["path"]).as_str().expect("path"));
    for f in files {
        let path = path_of(m, f);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = if let Some(j) = f.get("json") {
            serde_json::to_string(&m.fill(j)).unwrap()
        } else if let Some(lines) = f.get("lines").and_then(Value::as_array) {
            lines.iter().map(|l| format!("{}\n", m.fill(l))).collect()
        } else {
            m.fill(&f["text"]).as_str().expect("text").to_string()
        };
        fs::write(&path, body).unwrap();
    }
    for f in files {
        if let Some(age) = f.get("age").and_then(Value::as_f64) {
            let at = UNIX_EPOCH + Duration::from_secs_f64(m.t0 - age);
            fs::File::options().write(true).open(path_of(m, f)).unwrap().set_modified(at).unwrap();
        }
    }
}

/// 세션 기록 — Rust 에만 있는 `pid_ctime` 을 빼고, `resume_cmd` 는 같은 내용인 `resume_args` 앞에 claude 를 붙인 토큰 목록으로.
/// `resume_cmd` 문자열이 그 토큰을 이 플랫폼 규칙으로 이은 것과 다르면 문자열을 그대로 두어 표와 어긋나게 한다.
fn normalize_record(v: Value) -> Value {
    let Value::Object(mut r) = v else { return v };
    r.shift_remove("pid_ctime");
    if let Some(args) = r.shift_remove("resume_args") {
        let mut tokens = vec!["claude".to_string()];
        tokens.extend(args.as_array().into_iter().flatten().map(|a| a.as_str().unwrap_or("?").to_string()));
        if r.get("resume_cmd").and_then(Value::as_str) == Some(quote::join(&tokens).as_str()) {
            r.insert("resume_cmd".into(), json!(tokens));
        }
    }
    Value::Object(r)
}

/// sessions 폴더 — .json 은 내용(표시로 바꾼 것), 그 밖과 JSON 이 아닌 것은 있음(true)만.
fn snapshot(m: &Marks) -> Value {
    let dir = PathBuf::from(&m.root).join("notifier/sessions");
    let mut names: Vec<String> =
        fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let mut out = Map::new();
    for name in names {
        let mut value = Value::Bool(true);
        if name.ends_with(".json") {
            if let Ok(v) = serde_json::from_slice::<Value>(&fs::read(dir.join(&name)).unwrap_or_default()) {
                value = m.mark(&normalize_record(v));
            }
        }
        out.insert(name, value);
    }
    Value::Object(out)
}

/// 다른 곳을 `경로: 표 … / Rust …` 줄로 모은다.
fn diff(path: &str, want: Option<&Value>, got: Option<&Value>, out: &mut Vec<String>) {
    if want == got {
        return;
    }
    if let (Some(Value::Object(a)), Some(Value::Object(b))) = (want, got) {
        let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            diff(&format!("{path}.{k}"), a.get(k), b.get(k), out);
        }
        return;
    }
    let show = |v: Option<&Value>| v.map_or("(없음)".to_string(), Value::to_string);
    out.push(format!("{path}: 표 {} / Rust {}", show(want), show(got)));
}

/// 가짜 바깥 — 표를 만든 쪽(gen.py)의 가짜와 같은 규칙.
struct Fake {
    claude_pid: u32,
    tty: String,
    cwd: String,
    /// 이 단계의 claude 실행 인자. None 이면 claude 를 찾지 못한다
    argv: Option<Vec<String>>,
    project_dir: Option<String>,
}

impl Env for Fake {
    fn find_claude(&self) -> Option<ClaudeProcess> {
        let argv = self.argv.clone()?;
        Some(ClaudeProcess { pid: self.claude_pid, ctime: None, tty: Some(self.tty.clone()), headless: false, argv })
    }
    fn alive(&self, pid: u32, ctime: Option<f64>) -> bool {
        pid == self.claude_pid || proc::alive(pid, ctime)
    }
    fn argv(&self, pid: u32) -> Vec<String> {
        match &self.argv {
            Some(a) if pid == self.claude_pid => a.clone(),
            _ => Vec::new(),
        }
    }
    fn subagent_meta(&self, transcript: &str, agent_id: &str) -> Record {
        SystemEnv.subagent_meta(transcript, agent_id)
    }
    fn cwd(&self) -> String {
        self.cwd.clone()
    }
    fn project_dir(&self) -> Option<String> {
        self.project_dir.clone()
    }
}

fn scenarios(t: &Value) -> &Vec<Value> {
    t["scenarios"].as_array().expect("scenarios")
}

fn report(kind: &str, failures: &[String], total: usize) {
    assert!(
        failures.is_empty(),
        "{kind} 표와 Rust 결과가 {}/{total} 시나리오에서 다르다 (표: hook-table/{kind}.json, 만들기: python3 -I hook-table/gen.py)\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn state_matches_python_table() {
    let t = table("state.json");
    let pids = real_pids();
    let claude_pid = u32::try_from(t["claude"]["pid"].as_u64().unwrap()).unwrap();
    assert!(pids.iter().all(|(_, p)| *p != claude_pid), "진짜 pid 가 가짜 claude pid 와 겹친다: {pids:?}");
    let tmp = common::TempDir::new("hook-table-state");
    let root = tmp.path();
    let (mut failures, mut steps_run) = (Vec::new(), 0);
    let list = scenarios(&t);
    assert!(list.len() >= 20, "state 시나리오가 너무 적다: {}", list.len());
    for sc in list {
        let name = sc["name"].as_str().unwrap();
        reset(root);
        let mut m = Marks::new(root, &pids);
        plant(sc.get("seed"), &mut m);
        let sessions = root.join("notifier/sessions");
        for (k, step) in sc["steps"].as_array().unwrap().iter().enumerate() {
            let arg = step["arg"].as_str().unwrap();
            let input = match step.get("input_text") {
                Some(text) => text.as_str().unwrap().as_bytes().to_vec(),
                None => serde_json::to_vec(&m.fill(&step["input"])).unwrap(),
            };
            let argv = match m.fill(&step["claude"]) {
                Value::Null => None,
                v => Some(serde_json::from_value::<Vec<String>>(v).unwrap()),
            };
            let env = Fake {
                claude_pid,
                tty: t["claude"]["tty"].as_str().unwrap().into(),
                cwd: t["fallback_cwd"].as_str().unwrap().into(),
                argv,
                project_dir: step["env"]["CLAUDE_PROJECT_DIR"].as_str().map(str::to_string),
            };
            m.step(|| state::run(&sessions, arg, &input, &env));
            steps_run += 1;
            let mut lines = Vec::new();
            diff("sessions", step["expect"].get("sessions"), Some(&snapshot(&m)), &mut lines);
            if !lines.is_empty() {
                let event = step["input"]["hook_event_name"].as_str().unwrap_or("-");
                failures.push(format!("- 「{name}」 단계 {k} (state {arg}, {event}):\n    {}", lines.join("\n    ")));
                break; // 뒤 단계는 앞의 차이를 물려받으므로 시나리오마다 첫 차이만 적는다
            }
        }
    }
    note(&format!("state: 시나리오 {} 개 · 단계 {steps_run} 개를 견줬다", list.len()));
    report("state", &failures, list.len());
}

/// 패널 인자(--이름 값 …)를 이름별 값으로 — 맨 앞 --notify·순서·--sound 는 뺀다(README).
fn panel_fields(args: &[String]) -> Value {
    let args = args.strip_prefix(&["--notify".to_string()]).expect("패널 인자는 --notify 로 시작한다");
    assert!(args.len().is_multiple_of(2), "{args:?}");
    let mut out = Map::new();
    for pair in args.chunks(2) {
        let name = pair[0].strip_prefix("--").unwrap_or_else(|| panic!("{args:?}"));
        out.insert(name.to_string(), json!(pair[1]));
    }
    out.shift_remove("sound");
    if out.get("session").and_then(Value::as_str).is_none_or(str::is_empty) {
        out.insert("session".into(), Value::Null);
    }
    Value::Object(out)
}

#[test]
fn notify_matches_python_table() {
    let t = table("notify.json");
    let pids = real_pids();
    let tmp = common::TempDir::new("hook-table-notify");
    let root = tmp.path();
    // ~/.claude/sessions/<pid>.json(Claude Code 의 status)을 임시 폴더에서 읽게 한다 — 이 시험 실행 파일 안에서 이 시험만 쓴다
    std::env::set_var("CLAUDE_BOARD_HOME", root.join("home"));
    let cwd = t["fallback_cwd"].as_str().unwrap();
    let mut failures = Vec::new();
    let list = scenarios(&t);
    assert!(list.len() >= 20, "notify 시나리오가 너무 적다: {}", list.len());
    for sc in list {
        let name = sc["name"].as_str().unwrap();
        reset(root);
        let mut m = Marks::new(root, &pids);
        plant(sc.get("seed"), &mut m);
        let notifier = root.join("notifier");
        if sc.get("panel_app") != Some(&Value::Bool(false)) {
            // 패널 실행 파일 — macOS 는 notifier/claude-notifier 를 보고, Windows 는 app-path 가 가리키는 파일을 본다
            let app = notifier.join("claude-notifier");
            fs::write(&app, "").unwrap();
            fs::write(notifier.join("app-path"), app.to_str().unwrap()).unwrap();
        }
        let kind = sc["kind"].as_str().unwrap();
        let data = m.fill(&sc["input"]).as_object().cloned().expect("input 은 객체");
        let got = match notify::prepare(kind, &data, &notifier, cwd) {
            Some(panel) => m.mark(&panel_fields(&panel.args)),
            None => Value::Null,
        };
        let mut lines = Vec::new();
        diff("panel", sc["expect"].get("panel"), Some(&got), &mut lines);
        if !lines.is_empty() {
            failures.push(format!("- 「{name}」 (notify {kind}):\n    {}", lines.join("\n    ")));
        }
    }
    note(&format!("notify: 시나리오 {} 개를 견줬다", list.len()));
    report("notify", &failures, list.len());
}

#[test]
fn hooks_match_python_table() {
    // 실행 경로·명령 모양(python3 "…/state.py" idle ↔ exe + args)은 판마다 다르다 — 이벤트·matcher·하위 명령·type·timeout 만 견준다
    let t = table("hooks.json");
    let mut settings = Map::new();
    hooks::add(&mut settings, "/x/.claude/notifier/claude-board-hook");
    let mut got = Map::new();
    for (event, groups) in settings["hooks"].as_object().unwrap() {
        let rows: Vec<Value> = groups
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                let hooks: Vec<Value> = g["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|h| json!({"type": h["type"], "timeout": h["timeout"], "run": h["args"]}))
                    .collect();
                let mut row = Map::new();
                if let Some(m) = g.get("matcher") {
                    row.insert("matcher".into(), m.clone());
                }
                row.insert("hooks".into(), Value::Array(hooks));
                Value::Object(row)
            })
            .collect();
        got.insert(event.clone(), Value::Array(rows));
    }
    let mut lines = Vec::new();
    diff("hooks", t.get("hooks"), Some(&Value::Object(got)), &mut lines);
    diff("env", t.get("env"), settings.get("env"), &mut lines);
    assert!(lines.is_empty(), "hooks.py 의 HOOKS 와 hooks::HOOKS 가 다르다 (표: hook-table/hooks.json)\n    {}", lines.join("\n    "));
    note(&format!("hooks: 이벤트 {} 개를 견줬다", t["hooks"].as_object().unwrap().len()));
}
