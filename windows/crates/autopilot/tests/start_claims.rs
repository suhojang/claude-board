// 출발(`start`)의 같은 경로 자리 시험 — 원본 `cmdStart` 의 apClaimPaths 연결(6단계 ⑫-6).
// 같은 경로를 이미 잡은 살아 있는 실행이 있으면 출발하지 않고, 실패한 출발은 잡은 자리를 다시 놓고, 다른 저장소는 그대로 지나간다.
// 자리 잡기 규칙 그 자체는 `tests/multirun.rs`, 어댑터(SQLite 책·탐침)는 `tests/claims.rs` 가 각각 맡는다.
// 명령 전체를 돌려야 하는 시험이라 `cli::execute_with` 에 가짜 `Deps`(가짜 작업 스케줄러)를 넣는다(같은 법: `tests/cli.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use claude_board_autopilot::adapters::files::{FsRunFiles, SystemClock};
use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::cli::{self, Command as Cmd, Deps};
use claude_board_autopilot::engine::claims::claim_paths_of;
use claude_board_autopilot::ports::{KeepAlive, PortError, PortErrorKind, PortResult, Supervisor};
use claude_board_core::envs::{EnvChoice, EnvEntry, EnvKind};
use claude_board_core::test_claude::Decision;
use rusqlite::Connection;
use serde_json::{json, Value};

/// 고정 설정 폴더로 복사할 엔진 실행 파일(진짜 파일이어야 출발의 복사를 지나간다)
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

    /// 실행 폴더를 만들고 run.json·queue.json 을 쓴다
    fn make_run(&self, id: &str, run: Value, queue: Value) -> PathBuf {
        // 기본 설정의 쓰기 범위 자리표(`C:\shared`)는 실제로 있는 임시 폴더로 바꾼다 — 출발은 없는 폴더를 거부한다
        let mut run = run;
        if let Some(roots) = run.get_mut("write_roots").and_then(Value::as_array_mut) {
            for r in roots.iter_mut().filter(|r| r.as_str() == Some("C:\\shared")) {
                std::fs::create_dir_all(self.path("shared")).unwrap();
                *r = json!(self.path("shared").to_string_lossy());
            }
        }
        let dir = self.path("runs").join(id);
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

/// 거부(실행 폴더 문턱 걸림)와 까닭 글을 견준다
fn refused_with(out: &Out, needle: &str) {
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    let j = out.json();
    assert_eq!(j["ok"], json!(false));
    let problems: Vec<String> = j["problems"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    assert!(problems.iter().any(|p| p.contains(needle)), "{needle:?} 가 없다: {problems:?}");
}

/// 기록 저장소 자리 표의 실행 id 줄 — 시험에서 직접 읽어 견주려고(어댑터 시험이 아니라 명령 시험의 증거로)
fn claim_run_ids(db: &Path) -> Vec<String> {
    let conn = Connection::open(db).expect("저장소 열기");
    let mut st = conn.prepare("SELECT run_id FROM path_claims ORDER BY run_id").expect("질의 준비");
    let rows = st.query_map([], |row| row.get::<_, String>(0)).unwrap();
    rows.map(|r| r.expect("줄 읽기")).collect()
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
fn run_with(fx: &Fx, sup: &Arc<FakeSup>, claude: Option<&str>, command: Cmd) -> Out {
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
    let o = cli::execute_with(command, &deps);
    let code = o.code;
    Out { code, stdout: o.stdout, stderr: o.stderr }
}

fn start_cmd(dir: &Path, json: bool) -> Cmd {
    Cmd::Start { dir: dir.to_path_buf(), dev: false, accept_unsandboxed: false, no_schedule: false, json }
}

fn records_db(fx: &Fx) -> PathBuf {
    fx.path("root").join("live").join("private").join("records.sqlite")
}

// ── 같은 경로 자리 잡기 ──────────────────────────────

#[test]
fn second_start_on_the_same_repo_is_refused_and_the_first_claim_stays() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let a = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let b = fx.make_run("r2", base_run(), json!([unit("u2", &repo)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup::default());
    let db = records_db(&fx);

    // 첫 출발은 잡은 자리를 기록으로 남긴다(등록도 하므로 자리의 실행은 살아 있는 것으로 보인다)
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&a, true));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(claim_run_ids(&db), vec!["r1".to_string()], "첫 출발의 경로 자리");
    assert_eq!(branch_of(&repo), "autopilot/r1");

    // 둘째 출발은 같은 저장소라 거부하고, 첫 출발의 자리와 가지를 건드리지 않는다
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&b, true));
    refused_with(&out, "자율주행이 돌고 있습니다(r1,");
    assert_eq!(claim_run_ids(&db), vec!["r1".to_string()], "둘째는 첫 자리를 지우지 않는다");
    assert!(!b.join("pinned").exists() && read(&b, "run.json").get("state").is_none(), "둘째는 아무것도 바꾸지 않는다");
    assert_eq!(branch_of(&repo), "autopilot/r1", "둘째가 가지를 만들지 않는다");
    assert_eq!(sup.registered.lock().unwrap().len(), 1, "둘째를 작업 스케줄러에 등록하지 않는다");
}

#[test]
fn a_failed_start_leaves_no_claim_behind() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let dir = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup { fail_register: true, ..FakeSup::default() });
    let db = records_db(&fx);

    // 등록이 실패해도 잡은 자리는 다시 놓인다(원본 fail 이 failCleanup 을 부르는 자리)
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&dir, true));
    assert_eq!(out.code, 3, "{}{}", out.stdout, out.stderr);
    assert!(out.json()["problems"][0].as_str().unwrap().contains("작업 스케줄러에 등록하지 못했습니다"), "{}", out.stdout);
    assert!(claim_run_ids(&db).is_empty(), "실패한 출발의 남은 자리: {:?}", claim_run_ids(&db));
    assert_eq!(branch_of(&repo), "main");
    assert!(!fx.path("root").join("live").join("private").join("r1").exists());
}

#[test]
fn the_other_repo_s_second_start_goes_through() {
    let fx = Fx::new();
    let repo_a = toy_repo(&fx.path("repo_a"));
    let repo_b = toy_repo(&fx.path("repo_b"));
    let a = fx.make_run("r1", base_run(), json!([unit("u1", &repo_a)]));
    let b = fx.make_run("r2", base_run(), json!([unit("u1", &repo_b)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup::default());
    let db = records_db(&fx);

    // 다른 저장소를 쓰는 둘째 출발은 막지 않는다
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&a, true));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&b, true));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(branch_of(&repo_a), "autopilot/r1");
    assert_eq!(branch_of(&repo_b), "autopilot/r2");
    assert_eq!(claim_run_ids(&db), vec!["r1".to_string(), "r2".to_string()]);
    assert_eq!(sup.registered.lock().unwrap().len(), 2);
}

#[test]
fn a_start_clears_leftover_claims_of_dead_runs_and_says_so() {
    let fx = Fx::new();
    let repo = toy_repo(&fx.path("repo"));
    let a = fx.make_run("r1", base_run(), json!([unit("u1", &repo)]));
    let b = fx.make_run("r2", base_run(), json!([unit("u2", &repo)]));
    let claude = fx.fake_claude();
    let sup = Arc::new(FakeSup::default());
    let db = records_db(&fx);

    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&a, false));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    // r1 의 실행이 끝났는데 자리가 남았다 친다(엔진도 작업도 없음) — 치운 뒤에 지나간다
    let run = read(&a, "run.json");
    let mut ended = run.clone();
    ended["state"] = json!("done");
    ended["wrapped"] = json!(null);
    std::fs::write(a.join("run.json"), serde_json::to_string_pretty(&ended).unwrap()).unwrap();
    sup.existing.lock().unwrap().clear();

    let out = run_with(&fx, &sup, Some(&claude), start_cmd(&b, false));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("출발했습니다: r2"), "{}", out.stdout);
    assert!(
        out.stdout.contains("끝났거나 엔진이 없는 실행의 남은 경로 자리를 치웠습니다: r1"),
        "치운 자리 알림이 없다: {}",
        out.stdout
    );
    assert_eq!(claim_run_ids(&db), vec!["r2".to_string()], "치운 r1 자리는 표에서 없어진다");
}

// ── 자리를 골라 담기 ─────────────────────────────────

#[test]
fn claim_paths_of_ignores_letter_case_and_drops_cwds_inside_a_repo() {
    // 같은 자리를 대소문자만 다른 두 줄로 두 번 잡지 않는다(Windows 접기로 견주는 회귀 시험)
    let places = claim_paths_of(&json!({
        "repos": ["C:\\work\\repo"],
        "units": [{"cwd": "c:/work/repo/unit"}],
    }));
    assert_eq!(places, vec!["C:/work/repo".to_string()]);

    // 저장소 안의 작업 폴더는 저장소가 이미 잡은 자리고, 저장소 밖 작업 폴더는 그 자리를 잡는다(실제 경로로 풀 때 드라이브 글자는 큰 글자로 돌아온다)
    let places = claim_paths_of(&json!({
        "repos": ["C:/work/repo", "d:/elsewhere"],
        "units": [{"cwd": "d:/elsewhere/out"}, {"cwd": "e:/solo"}],
    }));
    assert_eq!(
        places,
        vec!["C:/work/repo".to_string(), "D:/elsewhere".to_string(), "E:/solo".to_string()]
    );
}

#[test]
fn claim_paths_of_folds_letter_case_inside_the_path() {
    // 폴더 이름의 대소문자만 다른 작업 폴더도 저장소 안의 자리다(드라이브 글자만 다른 경우는 `canonical_loose` 가 이미 맞춘다)
    let places = claim_paths_of(&json!({
        "repos": ["C:/Repo"],
        "units": [{"cwd": "C:/repo/sub"}, {"cwd": "C:/REPO"}, {"cwd": "C:/repository"}],
    }));
    assert_eq!(places, vec!["C:/Repo".to_string(), "C:/repository".to_string()]);
}
