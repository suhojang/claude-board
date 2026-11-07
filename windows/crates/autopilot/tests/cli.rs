// 명령줄(`cli`) 시험. 명령 전체는 `claude-autopilot.exe` 를 `CLAUDE_BOARD_HOME`=임시 폴더로 띄워 보고,
// 작업 스케줄러를 가짜로 바꿔야 하는 것(등록 인자·등록 실패 되돌리기)은 `cli::execute_with` 에 가짜 `Deps` 를 넣어 본다.
// 실제 작업 스케줄러를 만드는 시험은 `#[ignore] measured_start_registers_and_unregister_all` 하나뿐이다.
// start 가 준비 확인까지 가는 시험은 node 와 한 줄짜리 가짜 claude(`--version` 만 답한다)를 시험용 claude 로 쓴다.
mod fakes;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use claude_board_autopilot::adapters::files::{FsRunFiles, SystemClock};
use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::cli::{self, Command as Cmd, Deps, Target};
use claude_board_autopilot::engine::run::frozen_digest;
use claude_board_autopilot::ports::{KeepAlive, PortError, PortErrorKind, PortResult, RunFiles, Supervisor};
use claude_board_core::envs::{EnvChoice, EnvEntry, EnvKind};
use claude_board_core::test_claude::Decision;
use serde_json::{json, Value};

const CLI: &str = env!("CARGO_BIN_EXE_claude-autopilot");
const ENGINE: &str = env!("CARGO_BIN_EXE_claude-autopilot-engine");

// ── 도구 ─────────────────────────────────────────────

fn node() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|d| d.join("node.exe")).find(|p| p.is_file()).expect("node.exe 가 PATH 에 있어야 한다")
}

fn comspec() -> PathBuf {
    PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec"))
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim()).unwrap_or_else(|e| panic!("JSON 한 줄이어야 한다: {e}\n{}\n{}", self.stdout, self.stderr))
    }
}

struct Fx {
    tmp: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        Self { tmp }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    /// 실행 폴더를 만들고 run.json·queue.json 을 쓴다
    fn make_run(&self, id: &str, run: Value, queue: Value) -> PathBuf {
        self.make_run_in("runs", id, run, queue)
    }

    /// `make_run` 인데 부모 폴더를 고른다(같은 이름의 실행 폴더를 다른 부모에 둘 때)
    fn make_run_in(&self, parent: &str, id: &str, mut run: Value, queue: Value) -> PathBuf {
        // 기본 설정의 쓰기 범위 자리표(`C:\shared`)는 실제로 있는 임시 폴더로 바꾼다 — 출발은 없는 폴더를 거부한다
        if let Some(roots) = run.get_mut("write_roots").and_then(Value::as_array_mut) {
            for r in roots.iter_mut().filter(|r| r.as_str() == Some("C:\\shared")) {
                std::fs::create_dir_all(self.path("shared")).unwrap();
                *r = json!(self.path("shared").to_string_lossy());
            }
        }
        let dir = self.path(parent).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("run.json"), serde_json::to_string_pretty(&run).unwrap()).unwrap();
        std::fs::write(dir.join("queue.json"), serde_json::to_string_pretty(&queue).unwrap()).unwrap();
        dir
    }

    /// 시험용 claude 값 `node|v.cjs`(`--version` 에 한 줄 답한다)
    fn fake_claude(&self) -> String {
        let script = self.path("v.cjs");
        std::fs::write(&script, "console.log('9.9.9 (Claude Code)');").unwrap();
        format!("{}|{}", node().display(), script.display())
    }

    fn cli(&self, args: &[&str]) -> Out {
        self.cli_env(args, &[])
    }

    fn cli_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Out {
        let mut c = Command::new(CLI);
        c.args(args).env("CLAUDE_BOARD_HOME", self.home()).env_remove("CLAUDE_BOARD_CLAUDE");
        for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("FAKE_CLAUDE_")) {
            c.env_remove(k);
        }
        c.envs(envs.iter().copied());
        finish(c)
    }
}

fn finish(mut c: Command) -> Out {
    let o = c.output().unwrap();
    Out { code: o.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr: String::from_utf8_lossy(&o.stderr).into_owned() }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t.t"]).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// 파일 하나를 커밋한 장난감 저장소(가지 main)
fn toy_repo(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-q", "-m", "first"]);
    dir.to_path_buf()
}

/// `\\?\` 를 뗀 정규 경로(출발이 고정 설정에 싣는 꼴)
fn canon(p: &Path) -> String {
    let c = std::fs::canonicalize(p).unwrap();
    let s = c.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

fn branch_of(dir: &Path) -> String {
    git(dir, &["branch", "--show-current"])
}

fn read(dir: &Path, rel: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(rel)).unwrap()).unwrap()
}

/// 동의까지 있는 네이티브(cmd) 실행 환경 기록
fn cmd_env() -> Value {
    json!({"key": "cmd", "kind": "cmd", "unsandboxed_ok": {"at": 1761000000, "by": "board"}})
}

fn unit(id: &str, cwd: &Path) -> Value {
    json!({"id": id, "title": format!("단위 {id}"), "check": "exit 0", "cwd": cwd.to_string_lossy(), "prompt": "p"})
}

fn base_run() -> Value {
    json!({"title": "장난감", "model": "claude-sonnet-5-5", "effort": "high", "project": "toy", "caps": {"minutes": 60, "usd": 5, "weekly_pp": 2},
           "stopline": {"five_hour": 0.95, "seven_day": 0.8}, "check_retries": 2, "write_roots": ["C:\\shared"], "exec_env": cmd_env(), "memo": "frozen 에 안 간다"})
}

fn problems(out: &Out) -> Vec<String> {
    out.json()["problems"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect()
}

fn refused_with(out: &Out, needle: &str) {
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    let j = out.json();
    assert_eq!(j["ok"], json!(false));
    assert!(problems(out).iter().any(|p| p.contains(needle)), "{needle:?} 가 없다: {:?}", problems(out));
}

// ── 인자 ─────────────────────────────────────────────

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_reads_every_command() {
    assert_eq!(
        cli::parse(&args(&["start", "d", "--dev", "--accept-unsandboxed", "--no-schedule", "--json"])),
        Ok(Cmd::Start { dir: "d".into(), dev: true, accept_unsandboxed: true, no_schedule: true, json: true })
    );
    assert_eq!(cli::parse(&args(&["run", "d", "abc"])), Ok(Cmd::Run { dir: "d".into(), hash: "abc".into() }));
    assert_eq!(cli::parse(&args(&["control", "d", "pause"])), Ok(Cmd::Control { dir: "d".into(), action: "pause".into(), json: false }));
    assert_eq!(cli::parse(&args(&["status", "d", "--json"])), Ok(Cmd::Status { dir: "d".into(), json: true }));
    assert_eq!(cli::parse(&args(&["unregister", "--all", "--dev"])), Ok(Cmd::Unregister { target: Target::All, dev: true, json: false }));
    assert_eq!(cli::parse(&args(&["unregister", "d"])), Ok(Cmd::Unregister { target: Target::Folder("d".into()), dev: false, json: false }));
    for bad in [
        vec![],
        vec!["bogus"],
        vec!["start"],
        vec!["start", "a", "b"],
        vec!["start", "a", "--bogus"],
        vec!["run", "a"],
        vec!["run", "a", "b", "c"],
        vec!["control", "a", "dance"],
        vec!["control", "a", "takeover"],
        vec!["control", "a", "giveback"],
        vec!["status"],
        vec!["status", "a", "--dev"],
        vec!["unregister"],
        vec!["unregister", "--all", "a"],
    ] {
        assert!(cli::parse(&args(&bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn names_follow_the_run_id_rule() {
    assert!(cli::valid_run_id("20261021-0900-toy"));
    assert!(cli::valid_run_id("a.b_c-D9"));
    for bad in ["", ".", "..", "a b", "a/b", "한글", &"x".repeat(121)] {
        assert!(!cli::valid_run_id(bad), "{bad:?}");
    }
    assert!(cli::valid_run_id(&"x".repeat(120)));
    assert_eq!(cli::label_for(false, "r1"), "com.claude.board.autopilot.r1");
    assert_eq!(cli::label_for(true, "r1"), "com.claude.board.dev.autopilot.r1");
}

#[test]
fn frozen_has_sorted_keys_and_only_the_listed_settings() {
    let run = base_run();
    let queue = json!([{"id": "u1", "check": "exit 0", "cwd": "C:\\x"}]);
    let off = cli::build_frozen(&run, &queue, cmd_env(), "r1", true, 1761000000.5, None);
    let keys: Vec<&String> = off.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(off["id"], "r1");
    assert_eq!(off["dev"], true);
    assert_eq!(off["version"], 1);
    assert_eq!(off["frozen_at"], json!(1761000000.5));
    assert_eq!(off["units"], queue);
    assert_eq!(off["model"], "claude-sonnet-5-5");
    assert_eq!(off["write_roots"], json!(["C:\\shared"]));
    assert!(off.get("memo").is_none());
    assert!(off.get("claude_cmd").is_none() && off.get("claude_env").is_none());

    let on = cli::build_frozen(&run, &queue, cmd_env(), "r1", false, 1.0, Some((vec!["a".into(), "b".into()], vec![("FAKE_CLAUDE_LOG".into(), "x".into())])));
    assert_eq!(on["claude_cmd"], json!(["a", "b"]));
    assert_eq!(on["claude_env"], json!({"FAKE_CLAUDE_LOG": "x"}));
}

#[test]
fn engine_exit_codes() {
    use claude_board_autopilot::engine::decide::RunState;
    use claude_board_autopilot::engine::run::RunExit;
    assert_eq!(cli::exit_code(&RunExit::Finished(RunState::Done)), 0);
    assert_eq!(cli::exit_code(&RunExit::LockHeld), 0);
    assert_eq!(cli::exit_code(&RunExit::FrozenChanged), 3);
    assert_eq!(cli::exit_code(&RunExit::LoadFailed("x".into())), 3);
}

#[test]
fn usage_errors_exit_2() {
    let fx = Fx::new();
    let dir = fx.make_run("r1", base_run(), json!([]));
    let d = dir.to_str().unwrap();
    for bad in [
        vec![],
        vec!["bogus"],
        vec!["start"],
        vec!["start", d, "--bogus"],
        vec!["run", d],
        vec!["control", d, "dance"],
        vec!["control", d],
        vec!["status"],
        vec!["unregister"],
        vec!["unregister", "--all", d],
    ] {
        let out = fx.cli(&bad);
        assert_eq!(out.code, 2, "{bad:?}: {}{}", out.stdout, out.stderr);
        assert!(out.stdout.is_empty() && out.stderr.contains("사용법"), "{bad:?}: {}", out.stderr);
    }
}

// ── start 거부 ───────────────────────────────────────

#[test]
fn start_refuses_no_env() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    run.as_object_mut().unwrap().remove("exec_env");
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    let out = fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]);
    refused_with(&out, "실행 환경을 고르지 않았습니다");
    assert!(!dir.join("pinned").exists());
    assert_eq!(branch_of(&repo), "main");
}

#[test]
fn start_refuses_native_without_consent() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    run["exec_env"] = json!({"key": "cmd", "kind": "cmd"});
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    let out = fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]);
    refused_with(&out, "동의");
    assert!(read(&dir, "run.json").get("state").is_none());
    assert_eq!(branch_of(&repo), "main");
}

#[test]
fn start_accept_unsandboxed_records_consent() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    run["exec_env"] = json!({"key": "cmd", "kind": "cmd"});
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--accept-unsandboxed", "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let ok = &read(&dir, "run.json")["exec_env"]["unsandboxed_ok"];
    assert_eq!(ok["by"], "cli");
    assert!(ok["at"].as_f64().unwrap() > 1.0e9, "{ok}");
    assert_eq!(read(&dir, "pinned/frozen.json")["exec_env"]["unsandboxed_ok"]["by"], "cli");
}

#[test]
fn start_keeps_an_earlier_consent_when_accepting_again() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--accept-unsandboxed", "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(read(&dir, "run.json")["exec_env"]["unsandboxed_ok"], json!({"at": 1761000000, "by": "board"}));
}

#[test]
fn start_refuses_unknown_env() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    run["exec_env"] = json!({"key": "nope", "kind": "nope"});
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "알 수 없는 실행 환경");
}

#[test]
fn start_refuses_a_wsl_distro_that_is_not_on_this_pc() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    // 어느 PC 에도 있을 수 없는 이름 — 실제 배포판 이름을 쓰면 시험이 VM 을 깨울 수 있다
    run["exec_env"] = json!({"key": "wsl:zz-no-such-distro-for-tests", "kind": "wsl", "distro": "zz-no-such-distro-for-tests"});
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    // 배포판이 없으면 "이 PC 에 없는 실행 환경"으로 출발하지 않는다
    let out = fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(!dir.join("pinned").exists());
    assert_eq!(branch_of(&repo), "main");
}

#[test]
fn start_refuses_already_started() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    run["state"] = json!("running");
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "이미 출발한 실행");

    let dir = fx.make_run("r2", base_run(), json!([unit("u1", &repo)]));
    std::fs::create_dir_all(dir.join("pinned")).unwrap();
    std::fs::write(dir.join("pinned").join("frozen.json"), "{}").unwrap();
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "이미 출발한 실행");

    // 출발 준비 중(starting)은 다시 출발할 수 있다
    let mut run = base_run();
    run["state"] = json!("starting");
    let dir = fx.make_run("r3", run, json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
}

#[test]
fn start_refuses_bad_folder_and_run_id() {
    let fx = Fx::new();
    refused_with(&fx.cli(&["start", fx.path("nope").to_str().unwrap(), "--no-schedule", "--json"]), "실행 폴더가 없습니다");
    let dir = fx.make_run("bad name", base_run(), json!([]));
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "실행 id 규칙");
    let dir = fx.path("runs").join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "run.json");
}

#[test]
fn start_refuses_bad_cwd() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let queue = json!([
        unit("u1", &repo),
        {"id": "u2", "title": "상대", "check": "exit 0", "cwd": "rel\\dir"},
        {"id": "u3", "title": "없음", "check": "exit 0", "cwd": fx.path("ghost").to_string_lossy()},
        {"id": "u4", "title": "빈칸", "check": "exit 0"},
        {"id": "u5", "title": "판정없음", "cwd": repo.to_string_lossy()},
    ]);
    let dir = fx.make_run("r1", base_run(), queue);
    let out = fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]);
    assert_eq!(out.code, 1, "{}", out.stdout);
    let p = problems(&out);
    assert_eq!(p.len(), 4, "단위마다 한 줄: {p:?}");
    assert!(p[0].contains("단위 2") && p[0].contains("절대 경로"), "{p:?}");
    assert!(p[1].contains("단위 3") && p[1].contains("폴더가 아니거나 없습니다"), "{p:?}");
    assert!(p[2].contains("단위 4") && p[2].contains("cwd"), "{p:?}");
    assert!(p[3].contains("단위 5") && p[3].contains("check"), "{p:?}");
    assert_eq!(branch_of(&repo), "main");

    let dir = fx.make_run("r2", base_run(), json!([]));
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "단위가 하나도 없습니다");
}

#[test]
fn start_refuses_cmd_multiline_check() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut u = unit("u1", &repo);
    u["check"] = json!("echo a\necho b");
    let dir = fx.make_run("r1", base_run(), json!([u]));
    refused_with(&fx.cli(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"]), "한 줄");
    assert_eq!(branch_of(&repo), "main");
}

#[test]
fn start_rejected_test_claude() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let ghost = fx.path("no-such-claude.exe");
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", ghost.to_str().unwrap())]);
    refused_with(&out, "시험용 claude 설정이 어긋납니다");
    // 가지도 고정 설정도 건드리지 않는다
    assert_eq!(branch_of(&repo), "main");
    assert!(!dir.join("pinned").exists());
    assert!(read(&dir, "run.json").get("state").is_none());
}

#[test]
fn start_no_schedule_requires_home() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let mut c = Command::new(CLI);
    c.args(["start", dir.to_str().unwrap(), "--no-schedule", "--json"]).env_remove("CLAUDE_BOARD_HOME").env_remove("CLAUDE_BOARD_CLAUDE");
    let out = finish(c);
    assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
    assert!(out.stderr.contains("CLAUDE_BOARD_HOME"), "{}", out.stderr);
    assert!(!dir.join("pinned").exists());
    assert_eq!(branch_of(&repo), "main");
}

// ── start 성공·되돌리기 ──────────────────────────────

#[test]
fn start_no_schedule_writes_frozen_and_run_json() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("20261105-toy", base_run(), json!([unit("u1", &repo), unit("u2", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let j = out.json();
    assert_eq!(j["ok"], true);
    assert_eq!(j["run"], "20261105-toy");
    assert_eq!(j["label"], cli::label_for_dir(false, &dir).as_str());
    assert!(j["label"].as_str().unwrap().starts_with("com.claude.board.autopilot.20261105-toy."), "{}", j["label"]);
    assert_eq!(j["scheduled"], false);
    assert_eq!(j["exec_env"]["key"], "cmd");
    assert_eq!(j["exec_env"]["sandbox"], false);

    // 고정 설정: 칸·해시·엔진 사본
    let frozen = read(&dir, "pinned/frozen.json");
    assert_eq!(frozen["id"], "20261105-toy");
    assert_eq!(frozen["dev"], false);
    assert_eq!(frozen["version"], 1);
    assert_eq!(frozen["model"], "claude-sonnet-5-5");
    assert_eq!(frozen["effort"], "high");
    assert_eq!(frozen["caps"], json!({"minutes": 60, "usd": 5, "weekly_pp": 2}));
    assert_eq!(frozen["write_roots"], json!([canon(&fx.path("shared"))]), "쓰기 범위는 정규 경로로 고정한다");
    assert_eq!(frozen["units"].as_array().unwrap().len(), 2);
    assert_eq!(frozen["units"][0]["check"], "exit 0");
    assert!(frozen.get("memo").is_none());
    assert!(frozen["frozen_at"].as_f64().unwrap() > 1.0e9);
    assert_eq!(j["hash"].as_str().unwrap(), frozen_digest(&frozen));
    let engine = dir.join("pinned").join("claude-autopilot-engine.exe");
    assert_eq!(std::fs::metadata(&engine).unwrap().len(), std::fs::metadata(ENGINE).unwrap().len());

    // run.json: 출발 직후 모습과 가지 기록
    let run = read(&dir, "run.json");
    assert_eq!(run["state"], "starting");
    assert_eq!(run["reason"], "출발을 기다립니다.");
    assert_eq!(run["dev"], false);
    assert_eq!(run["launchd_label"], cli::label_for_dir(false, &dir).as_str());
    assert_eq!(run["sessions"], json!([]));
    assert!(run["unit"].is_null() && run["wait_until"].is_null());
    assert!(run["created"].as_f64().unwrap() > 1.0e9);
    assert_eq!(run["exec_env"]["sandbox"], false);
    assert_eq!(run["memo"], "frozen 에 안 간다");
    let repos = run["branches"]["repos"].as_object().unwrap();
    assert_eq!(repos.len(), 1, "같은 저장소는 한 번: {repos:?}");
    let (_, info) = repos.iter().next().unwrap();
    assert_eq!(info["work"], "autopilot/20261105-toy");
    assert_eq!(info["original"], "main");
    assert_eq!(info["base"]["branch"], "main");
    assert_eq!(info["base"]["head"].as_str().unwrap(), git(&repo, &["rev-parse", "main"]));
    assert!(info["restored"].is_null());
    assert_eq!(branch_of(&repo), "autopilot/20261105-toy");

    // queue.json: 단위는 모두 pending
    let queue = read(&dir, "queue.json");
    assert!(queue.as_array().unwrap().iter().all(|u| u["status"] == "pending"));

    // 엔진 전용 폴더
    assert!(fx.home().join("AppData").join("Local").join("Claude Board").join("autopilot").join("private").join("20261105-toy").is_dir());
}

#[test]
fn start_text_mode_prints_plain_korean() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("출발했습니다: r1") && out.stdout.contains("명령 프롬프트"), "{}", out.stdout);
    assert!(out.stderr.is_empty());
}

#[test]
fn start_branch_failure_rolls_back() {
    let fx = Fx::new();
    let a = toy_repo(&fx.path("a"));
    let b = toy_repo(&fx.path("b"));
    // a 에는 커밋 안 한 변경이 있고, b 에는 같은 이름의 가지가 이미 있어 두 번째 저장소에서 실패한다
    std::fs::write(a.join("a.txt"), "changed\n").unwrap();
    git(&b, &["branch", "autopilot/r1"]);
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &a), unit("u2", &b)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    refused_with(&out, "가지를 만들지 못했습니다");

    assert_eq!(branch_of(&a), "main");
    assert!(git(&a, &["branch", "--list", "autopilot/r1"]).is_empty(), "만든 가지를 지워야 한다");
    assert_eq!(std::fs::read_to_string(a.join("a.txt")).unwrap(), "changed\n", "커밋 안 한 변경은 그대로");
    assert_eq!(branch_of(&b), "main");
    assert!(!dir.join("pinned").exists());
    assert!(read(&dir, "run.json").get("state").is_none());
    assert!(read(&dir, "queue.json")[0].get("status").is_none());
}

#[test]
fn start_test_mode_freezes_claude_cmd_and_env() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let log = fx.path("fake.log");
    let out = fx.cli_env(
        &["start", dir.to_str().unwrap(), "--no-schedule", "--json"],
        &[("CLAUDE_BOARD_CLAUDE", &claude), ("FAKE_CLAUDE_LOG", log.to_str().unwrap()), ("FAKE_CLAUDE_SCRIPT", "s.json"), ("OTHER_VAR", "x")],
    );
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let frozen = read(&dir, "pinned/frozen.json");
    let parts: Vec<&str> = claude.split('|').collect();
    assert_eq!(frozen["claude_cmd"], json!(parts));
    assert_eq!(frozen["claude_env"], json!({"FAKE_CLAUDE_LOG": log.to_str().unwrap(), "FAKE_CLAUDE_SCRIPT": "s.json"}));
}

// ── 가짜 작업 스케줄러 ───────────────────────────────

#[derive(Default)]
struct FakeSup {
    registered: Mutex<Vec<KeepAlive>>,
    existing: Mutex<Vec<String>>,
    removed: Mutex<Vec<String>>,
    fail_register: bool,
}

struct Shared(Arc<FakeSup>);

impl Supervisor for Shared {
    fn register(&self, job: &KeepAlive) -> PortResult<()> {
        if self.0.fail_register {
            return Err(PortError::new(PortErrorKind::Denied, "거부됨"));
        }
        self.0.registered.lock().unwrap().push(job.clone());
        self.0.existing.lock().unwrap().push(job.label.clone());
        Ok(())
    }
    fn unregister(&self, label: &str) -> PortResult<()> {
        self.0.existing.lock().unwrap().retain(|l| l != label);
        self.0.removed.lock().unwrap().push(label.to_string());
        Ok(())
    }
    fn is_registered(&self, label: &str) -> PortResult<bool> {
        Ok(self.0.existing.lock().unwrap().iter().any(|l| l == label))
    }
    fn list(&self) -> PortResult<Vec<String>> {
        Ok(self.0.existing.lock().unwrap().clone())
    }
}

/// 가짜 작업 스케줄러·가짜 시험용 claude(`claude`, 없으면 꺼짐)·임시 뿌리로 명령을 돌린다
fn run_with(fx: &Fx, sup: &Arc<FakeSup>, claude: Option<&str>, command: Cmd) -> cli::Outcome {
    let entries = vec![EnvEntry { choice: EnvChoice { kind: EnvKind::Cmd, distro: None }, label: "cmd".into(), program: comspec() }];
    let parts: Option<Vec<PathBuf>> = claude.map(|c| c.split('|').map(PathBuf::from).collect());
    let (files, git, clock) = (FsRunFiles::new(), ProcessGit::new(), SystemClock);
    let make = {
        let sup = sup.clone();
        move |_dev: bool| -> Box<dyn Supervisor> { Box::new(Shared(sup.clone())) }
    };
    let root = fx.path("root");
    let deps = Deps {
        files: &files,
        git: &git,
        clock: &clock,
        supervisor: &make,
        detect: &|| entries.clone(),
        test_claude: &|| parts.clone().map_or(Decision::Off, Decision::On),
        fake_env: &|| vec![("FAKE_CLAUDE_X".to_string(), "1".to_string())],
        engine_exe: PathBuf::from(ENGINE),
        autopilot_root: &|dev| root.join(if dev { "dev" } else { "live" }),
        home_set: true,
    };
    cli::execute_with(command, &deps)
}

fn start_cmd(dir: &Path, json: bool) -> Cmd {
    Cmd::Start { dir: dir.to_path_buf(), dev: false, accept_unsandboxed: false, no_schedule: false, json }
}

#[test]
fn start_registers_a_keepalive_job_for_the_pinned_engine() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup::default());
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&dir, true));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let j: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(j["scheduled"], true);
    let jobs = sup.registered.lock().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].label, cli::label_for_dir(false, &dir));
    assert_eq!(jobs[0].program, dir.join("pinned").join("claude-autopilot-engine.exe"));
    assert_eq!(jobs[0].args, vec!["run".to_string(), dir.to_string_lossy().into_owned(), j["hash"].as_str().unwrap().to_string()]);
    assert_eq!(jobs[0].working_dir, dir);
    assert_eq!(read(&dir, "run.json")["state"], "starting");
    // 엔진 전용 폴더는 주입한 뿌리 아래
    assert!(fx.path("root").join("live").join("private").join("r1").is_dir());
    // 시험 환경 변수는 주입한 목록 그대로 고정
    assert_eq!(read(&dir, "pinned/frozen.json")["claude_env"], json!({"FAKE_CLAUDE_X": "1"}));
}

#[test]
fn start_register_failure_leaves_an_error_state_and_undoes_the_rest() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup { fail_register: true, ..FakeSup::default() });
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&dir, true));
    assert_eq!(out.code, 3, "{}{}", out.stdout, out.stderr);
    let j: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert!(j["problems"][0].as_str().unwrap().contains("작업 스케줄러에 등록하지 못했습니다"), "{j}");
    let run = read(&dir, "run.json");
    assert_eq!(run["state"], "error");
    assert!(run["reason"].as_str().unwrap().contains("작업 스케줄러에 등록하지 못했습니다"));
    assert!(!dir.join("pinned").exists());
    assert!(!fx.path("root").join("live").join("private").join("r1").exists());
    assert_eq!(branch_of(&repo), "main");
    assert!(git(&repo, &["branch", "--list", "autopilot/r1"]).is_empty());
    // 이미 끝난 상태라 다시 출발할 수 없다
    let again = run_with(&fx, &sup, Some(&claude), start_cmd(&dir, true));
    assert_eq!(again.code, 1);
}

// ── 같은 이름의 실행 폴더 ────────────────────────────

#[test]
fn same_named_run_folders_get_different_task_names() {
    let fx = Fx::new();
    let a = fx.make_run_in("a", "r1", base_run(), json!([]));
    let b = fx.make_run_in("b", "r1", base_run(), json!([]));
    let (la, lb) = (cli::label_for_dir(false, &a), cli::label_for_dir(false, &b));
    assert_ne!(la, lb, "부모만 다른 같은 이름은 작업 이름이 달라야 한다");
    assert!(la.starts_with("com.claude.board.autopilot.r1.") && lb.starts_with("com.claude.board.autopilot.r1."), "{la} {lb}");
    // 같은 폴더는 늘 같은 이름이고, 개발 판은 이름이 다르다
    assert_eq!(la, cli::label_for_dir(false, &a));
    assert!(cli::label_for_dir(true, &a).starts_with("com.claude.board.dev.autopilot.r1."));
    // 작업 스케줄러가 받는 이름 규칙 안(가장 긴 실행 id 도)
    let long = fx.make_run_in("c", &"x".repeat(120), base_run(), json!([]));
    for label in [&la, &lb, &cli::label_for_dir(true, &long)] {
        assert!(claude_board_autopilot::adapters::supervisor::valid_label(label), "{label}");
    }
}

#[test]
fn second_start_of_a_same_named_folder_is_refused_and_leaves_the_first_run_alone() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let a = fx.make_run_in("a", "r1", base_run(), json!([unit("u1", &repo)]));
    let b = fx.make_run_in("b", "r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let env = [("CLAUDE_BOARD_CLAUDE", claude.as_str())];
    let out = fx.cli_env(&["start", a.to_str().unwrap(), "--no-schedule", "--json"], &env);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let private = fx.home().join("AppData").join("Local").join("Claude Board").join("autopilot").join("private").join("r1");
    assert!(private.is_dir());
    std::fs::write(private.join("marker.txt"), "first").unwrap();

    let out = fx.cli_env(&["start", b.to_str().unwrap(), "--no-schedule", "--json"], &env);
    refused_with(&out, "같은 이름의 실행이 이미 엔진 전용 폴더를 쓰고 있습니다");
    assert_eq!(std::fs::read_to_string(private.join("marker.txt")).unwrap(), "first", "첫 실행의 엔진 전용 폴더가 그대로여야 한다");
    assert!(a.join("pinned").join("frozen.json").is_file());
    assert!(!b.join("pinned").exists() && read(&b, "run.json").get("state").is_none(), "둘째는 아무것도 바꾸지 않는다");
    assert_eq!(branch_of(&repo), "autopilot/r1", "둘째가 가지를 건드리지 않았다");
}

#[test]
fn start_is_refused_when_its_task_name_is_already_registered() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup::default());
    sup.existing.lock().unwrap().push(cli::label_for_dir(false, &dir));
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&dir, true));
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("작업 스케줄러에 있습니다"), "{}", out.stdout);
    assert!(sup.registered.lock().unwrap().is_empty());
    assert!(sup.removed.lock().unwrap().is_empty(), "남의 작업을 지우지 않는다");
    assert!(!dir.join("pinned").exists() && !fx.path("root").join("live").join("private").join("r1").exists());
    assert_eq!(branch_of(&repo), "main");

    // 다른 부모 폴더의 같은 이름은 작업 이름이 달라 그대로 출발한다
    let other = fx.make_run_in("b", "r1", base_run(), json!([unit("u1", &repo)]));
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&other, true));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(sup.registered.lock().unwrap()[0].label, cli::label_for_dir(false, &other));
}

// ── 경로 확인·고정할 설정 ────────────────────────────

#[test]
fn start_refuses_write_roots_that_are_too_wide_or_escape() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let escaping = format!("{}{sep}..{sep}shared", fx.path("shared").display(), sep = std::path::MAIN_SEPARATOR);
    let cases = [
        (json!([fx.home().to_string_lossy()]), "홈 폴더 전체나 그 위"),
        (json!([fx.path("").to_string_lossy()]), "홈 폴더 전체나 그 위"),
        (json!([fx.home().join(".claude").to_string_lossy()]), ".claude 아래"),
        (json!([escaping]), ".."),
        (json!([fx.path("ghost").to_string_lossy()]), "write_roots"),
    ];
    std::fs::create_dir_all(fx.path("shared")).unwrap();
    std::fs::create_dir_all(fx.home().join(".claude")).unwrap();
    let claude = fx.fake_claude();
    for (i, (roots, needle)) in cases.iter().enumerate() {
        let mut run = base_run();
        run["write_roots"] = roots.clone();
        let dir = fx.make_run(&format!("r{i}"), run, json!([unit("u1", &repo)]));
        let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
        refused_with(&out, needle);
        assert!(!dir.join("pinned").exists(), "{roots}");
    }
    assert_eq!(branch_of(&repo), "main", "거부한 출발은 가지를 만들지 않는다");
}

#[test]
fn start_copies_set_network_local_binding_and_claude_args_and_the_engine_uses_them() {
    use claude_board_autopilot::engine::decide::RunState;
    use claude_board_autopilot::engine::run::RunExit;
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let mut run = base_run();
    run["set"] = json!({"dir": "ignored", "instruction": "이 프로젝트는 쉬운 말로 보고한다"});
    run["network_allow"] = json!(["example.com", "repo.maven.apache.org"]);
    run["local_binding"] = json!(true);
    run["claude_args"] = json!(["--add-flag-for-test"]);
    let dir = fx.make_run("r1", run, json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--dev", "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let frozen = read(&dir, "pinned/frozen.json");
    assert_eq!(frozen["set"]["instruction"], "이 프로젝트는 쉬운 말로 보고한다");
    assert_eq!(frozen["network_allow"], json!(["example.com", "repo.maven.apache.org"]));
    assert_eq!(frozen["local_binding"], json!(true));
    assert_eq!(frozen["claude_args"], json!(["--add-flag-for-test"]));

    // 고정된 값을 그대로 엔진(가짜 포트)에 먹여, 정책·시스템 프롬프트·띄우기 인자까지 닿는지 본다
    let mut s = fakes::Scenario::new(1);
    for key in ["set", "network_allow", "local_binding", "claude_args"] {
        s.frozen[key] = frozen[key].clone();
    }
    let w = s.build();
    w.env.push_spawn(fakes::ChildScript::ok_turn(0.5));
    assert_eq!(w.run(), RunExit::Finished(RunState::Done));
    let policy = w.env.policies.lock().unwrap()[0].clone();
    assert_eq!(policy.network_allow, ["example.com", "repo.maven.apache.org"]);
    assert!(policy.local_binding);
    let prompt = w.files.text("/private/r1/system-prompt.md").expect("시스템 프롬프트 파일");
    assert!(prompt.contains("이 프로젝트는 쉬운 말로 보고한다"), "{prompt}");
    assert!(w.env.launches()[0].extra_args.contains(&"--add-flag-for-test".to_string()));
}

#[test]
fn start_refuses_bad_setting_shapes_and_claude_args_outside_dev() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let claude = fx.fake_claude();
    let cases = [
        ("network_allow", json!("example.com"), "network_allow 는 글자 배열"),
        ("local_binding", json!("yes"), "local_binding 은 true/false"),
        ("set", json!(["x"]), "set 은 객체"),
        ("claude_args", json!(["--x"]), "시험용이라 --dev"),
    ];
    for (i, (key, value, needle)) in cases.iter().enumerate() {
        let mut run = base_run();
        run[*key] = value.clone();
        let dir = fx.make_run(&format!("r{i}"), run, json!([unit("u1", &repo)]));
        let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--no-schedule", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
        refused_with(&out, needle);
    }
    assert_eq!(branch_of(&repo), "main");
}

#[test]
fn unregister_removes_only_registered_tasks() {
    let fx = Fx::new();
    let dir = fx.make_run("r1", base_run(), json!([]));
    let sup = Arc::new(FakeSup::default());
    sup.existing.lock().unwrap().extend(["com.claude.board.autopilot.r1".to_string(), "com.claude.board.autopilot.r2".to_string()]);
    let folder = Cmd::Unregister { target: Target::Folder(dir.clone()), dev: false, json: true };
    let out = run_with(&fx, &sup, None, folder.clone());
    assert_eq!(out.code, 0);
    assert_eq!(serde_json::from_str::<Value>(out.stdout.trim()).unwrap(), json!({"ok": true, "removed": ["com.claude.board.autopilot.r1"]}));
    // 이미 없으면 지울 것 없이 ok
    let out = run_with(&fx, &sup, None, folder);
    assert_eq!(serde_json::from_str::<Value>(out.stdout.trim()).unwrap(), json!({"ok": true, "removed": []}));
    let all = Cmd::Unregister { target: Target::All, dev: false, json: true };
    let out = run_with(&fx, &sup, None, all);
    assert_eq!(serde_json::from_str::<Value>(out.stdout.trim()).unwrap(), json!({"ok": true, "removed": ["com.claude.board.autopilot.r2"]}));
    assert!(sup.existing.lock().unwrap().is_empty());
}

#[test]
fn unregister_uses_the_label_recorded_in_run_json() {
    let fx = Fx::new();
    let mut run = base_run();
    run["launchd_label"] = json!("custom.label");
    let dir = fx.make_run("r1", run, json!([]));
    let sup = Arc::new(FakeSup::default());
    sup.existing.lock().unwrap().push("custom.label".to_string());
    let out = run_with(&fx, &sup, None, Cmd::Unregister { target: Target::Folder(dir), dev: false, json: true });
    assert_eq!(serde_json::from_str::<Value>(out.stdout.trim()).unwrap()["removed"], json!(["custom.label"]));
}

#[test]
fn status_reports_whether_the_task_is_scheduled() {
    let fx = Fx::new();
    let mut run = base_run();
    run["state"] = json!("running");
    let dir = fx.make_run("r1", run, json!([]));
    let sup = Arc::new(FakeSup::default());
    let st = |sup: &Arc<FakeSup>| {
        let out = run_with(&fx, sup, None, Cmd::Status { dir: dir.clone(), json: true });
        assert_eq!(out.code, 0, "{}", out.stderr);
        serde_json::from_str::<Value>(out.stdout.trim()).unwrap()["scheduled"].clone()
    };
    assert_eq!(st(&sup), json!(false));
    sup.existing.lock().unwrap().push("com.claude.board.autopilot.r1".to_string());
    assert_eq!(st(&sup), json!(true));
}

// ── control · status · unregister (실행 파일) ────────

#[test]
fn control_writes_file() {
    let fx = Fx::new();
    let mut run = base_run();
    run["state"] = json!("running");
    let dir = fx.make_run("r1", run, json!([]));
    for action in ["pause", "stop", "restart_from_intake", "resume"] {
        let out = fx.cli(&["control", dir.to_str().unwrap(), action, "--json"]);
        assert_eq!(out.code, 0, "{action}: {}{}", out.stdout, out.stderr);
        assert_eq!(out.json()["ok"], true);
        let req = read(&dir, "control.json");
        assert_eq!(req["action"], action);
        assert!(req["ts"].as_f64().unwrap() > 1.0e9);
    }
    // 임시 파일이 남지 않는다
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    let text = fx.cli(&["control", dir.to_str().unwrap(), "pause"]);
    assert_eq!(text.code, 0);
    assert!(text.stdout.contains("일시정지"), "{}", text.stdout);
}

#[test]
fn control_refuses_ended() {
    let fx = Fx::new();
    for (i, state) in ["done", "stopped", "error", ""].iter().enumerate() {
        let mut run = base_run();
        if !state.is_empty() {
            run["state"] = json!(state);
        }
        let dir = fx.make_run(&format!("r{i}"), run, json!([]));
        let out = fx.cli(&["control", dir.to_str().unwrap(), "pause", "--json"]);
        assert_eq!(out.code, 1, "{state}: {}{}", out.stdout, out.stderr);
        let j = out.json();
        assert_eq!(j["ok"], false);
        assert!(!j["why"].as_str().unwrap().is_empty());
        assert!(!dir.join("control.json").exists(), "{state}");
    }
    let out = fx.cli(&["control", fx.path("nope").to_str().unwrap(), "pause", "--json"]);
    assert_eq!(out.code, 1);
}

#[test]
fn control_refuses_takeover() {
    let fx = Fx::new();
    let mut run = base_run();
    run["state"] = json!("running");
    let dir = fx.make_run("r1", run, json!([]));
    for action in ["takeover", "giveback"] {
        let out = fx.cli(&["control", dir.to_str().unwrap(), action]);
        assert_eq!(out.code, 2, "{action}: {}", out.stderr);
        assert!(!dir.join("control.json").exists());
    }
}

#[test]
fn status_json_shape() {
    let fx = Fx::new();
    let mut run = base_run();
    run["state"] = json!("running");
    run["reason"] = json!("단위 1/2 하는 중");
    run["unit"] = json!(1);
    run["wait_until"] = Value::Null;
    run["usage"] = json!({"five_hour": 0.1, "seven_day": 0.2});
    let queue = json!([{"id": "u1", "title": "첫째", "status": "running", "check": "exit 0"}, {"id": "u2", "check": "exit 0"}]);
    let dir = fx.make_run("r1", run, queue);
    std::fs::create_dir_all(dir.join("units").join("1")).unwrap();
    std::fs::write(dir.join("units").join("1").join("engine.json"), r#"{"active_sec": 90.5, "cost_usd": 0.25}"#).unwrap();
    let before: Vec<_> = ["run.json", "queue.json"].iter().map(|f| std::fs::read(dir.join(f)).unwrap()).collect();

    let out = fx.cli(&["status", dir.to_str().unwrap(), "--json"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let j = out.json();
    assert_eq!(j["run"], "r1");
    assert_eq!(j["state"], "running");
    assert_eq!(j["reason"], "단위 1/2 하는 중");
    assert_eq!(j["unit"], 1);
    assert!(j["wait_until"].is_null());
    assert_eq!(j["exec_env"]["key"], "cmd");
    assert_eq!(j["usage"], json!({"five_hour": 0.1, "seven_day": 0.2}));
    assert_eq!(j["scheduled"], false);
    assert_eq!(
        j["units"],
        json!([
            {"n": 1, "title": "첫째", "status": "running", "active_sec": 90.5, "cost_usd": 0.25},
            {"n": 2, "title": "단위 2", "status": "pending", "active_sec": null, "cost_usd": null},
        ])
    );
    let keys: Vec<&str> = j.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["run", "state", "reason", "wait_until", "unit", "exec_env", "units", "scheduled", "usage"]);

    // 파일만 읽는다
    let after: Vec<_> = ["run.json", "queue.json"].iter().map(|f| std::fs::read(dir.join(f)).unwrap()).collect();
    assert_eq!(before, after);
    assert!(!dir.join("control.json").exists());

    let text = fx.cli(&["status", dir.to_str().unwrap()]);
    assert_eq!(text.code, 0);
    assert!(text.stdout.contains("running") && text.stdout.contains("첫째") && text.stdout.contains("$0.25"), "{}", text.stdout);
}

#[test]
fn status_of_a_run_not_started_has_empty_state() {
    let fx = Fx::new();
    let dir = fx.make_run("r1", base_run(), json!([]));
    let j = fx.cli(&["status", dir.to_str().unwrap(), "--json"]).json();
    assert_eq!(j["state"], "");
    assert_eq!(j["units"], json!([]));
    assert!(j["unit"].is_null() && j["usage"].is_null());
}

#[test]
fn unregister_folder_without_task_ok() {
    let fx = Fx::new();
    let dir = fx.make_run("no-such-task-r1", base_run(), json!([]));
    let out = fx.cli(&["unregister", dir.to_str().unwrap(), "--dev", "--json"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.json(), json!({"ok": true, "removed": []}));
    let text = fx.cli(&["unregister", dir.to_str().unwrap(), "--dev"]);
    assert_eq!(text.code, 0);
    assert!(text.stdout.contains("지울 작업이 없습니다"), "{}", text.stdout);
}

// ── run ──────────────────────────────────────────────

/// 출발한 실행 폴더 한 벌(고정 설정까지)을 직접 만든다. 해시를 돌려준다
fn started_run(fx: &Fx, id: &str, repo: &Path) -> (PathBuf, String) {
    let mut run = base_run();
    run["state"] = json!("starting");
    run["launchd_label"] = json!(cli::label_for(false, id));
    let queue = json!([unit("u1", repo)]);
    let dir = fx.make_run(id, run.clone(), queue.clone());
    let frozen = cli::build_frozen(&run, &queue, cmd_env(), id, false, 1761000000.0, None);
    std::fs::create_dir_all(dir.join("pinned")).unwrap();
    FsRunFiles::new().write_json(&dir.join("pinned").join("frozen.json"), &frozen).unwrap();
    (dir, frozen_digest(&frozen))
}

#[test]
fn run_lock_held_exit_0() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let (dir, hash) = started_run(&fx, "r1", &repo);
    // 다른 엔진이 잠금을 쥐고 있다
    let _held = FsRunFiles::new().try_lock(&dir.join(".engine.lock")).unwrap().expect("잠금");
    let before = std::fs::read(dir.join("run.json")).unwrap();
    let out = fx.cli(&["run", dir.to_str().unwrap(), &hash]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    // 아무것도 쓰지 않는다
    assert_eq!(std::fs::read(dir.join("run.json")).unwrap(), before);
    assert!(!dir.join("starts.log").exists());
}

#[test]
fn run_frozen_changed_exit_3() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let (dir, hash) = started_run(&fx, "r1", &repo);
    // 출발 뒤에 고정 설정이 바뀌었다
    let path = dir.join("pinned").join("frozen.json");
    let mut frozen = read(&dir, "pinned/frozen.json");
    frozen["model"] = json!("다른 모델");
    FsRunFiles::new().write_json(&path, &frozen).unwrap();
    let out = fx.cli(&["run", dir.to_str().unwrap(), &hash]);
    assert_eq!(out.code, 3, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.is_empty());
    let run = read(&dir, "run.json");
    assert_eq!(run["state"], "error");
    assert!(run["reason"].as_str().unwrap().contains("고정 설정"), "{run}");
    let log = std::fs::read_to_string(dir.join("engine.log")).unwrap();
    assert!(log.contains("고정 설정"), "{log}");
}

#[test]
fn run_without_a_frozen_file_exits_3_with_a_log_line() {
    let fx = Fx::new();
    let dir = fx.make_run("r1", base_run(), json!([]));
    let out = fx.cli(&["run", dir.to_str().unwrap(), "0000"]);
    assert_eq!(out.code, 3, "{}{}", out.stdout, out.stderr);
    assert!(std::fs::read_to_string(dir.join("engine.log")).unwrap().contains("pinned/frozen.json"));
}

#[test]
fn engine_exe_takes_run_only() {
    let fx = Fx::new();
    let dir = fx.make_run("r1", base_run(), json!([]));
    let mut c = Command::new(ENGINE);
    c.args(["status", dir.to_str().unwrap()]).env("CLAUDE_BOARD_HOME", fx.home());
    assert_eq!(finish(c).code, 2);
    let mut c = Command::new(ENGINE);
    c.args(["run", dir.to_str().unwrap(), "0000"]).env("CLAUDE_BOARD_HOME", fx.home());
    assert_eq!(finish(c).code, 3);
}

// ── 실제 작업 스케줄러 (측정용) ──────────────────────

#[test]
#[ignore = "실제 작업 스케줄러에 등록한다 — 측정 때만 `cargo test -p claude-board-autopilot --test cli -- --ignored` 로"]
fn measured_start_registers_and_unregister_all() {
    use claude_board_autopilot::adapters::supervisor::TaskScheduler;
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("cli-measured", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let out = fx.cli_env(&["start", dir.to_str().unwrap(), "--dev", "--json"], &[("CLAUDE_BOARD_CLAUDE", &claude)]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let sup = TaskScheduler::new(true);
    assert_eq!(sup.list().unwrap(), vec!["com.claude.board.dev.autopilot.cli-measured".to_string()]);
    let status = fx.cli(&["status", dir.to_str().unwrap(), "--json"]).json();
    assert_eq!(status["scheduled"], true);
    let out = fx.cli(&["unregister", "--all", "--dev", "--json"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.json(), json!({"ok": true, "removed": ["com.claude.board.dev.autopilot.cli-measured"]}));
    assert!(sup.list().unwrap().is_empty(), "남은 작업이 0 이어야 한다");
}
