//! `start` — 출발. 차례(위에서 걸리면 멈추고 이미 바꾼 것만 되돌린다): 폴더·파일 → 단위·설정 검사 → 실행 환경 문턱 → 판정 명령 꼴 →
//! 경로 확인(정규화·쓸 수 없는 자리) → 같은 이름의 실행이 이미 있는지 → 시험용 claude 판정과 준비 확인 → 같은 경로 자리 잡기 →
//! 가지 → 고정 설정 → run.json·queue.json → 작업 스케줄러 등록.
//! 같은 이름의 실행 폴더가 다른 곳에 있어도 엔진 전용 폴더(`private\<id>`)를 나눠 쓰지 못하게 막고, 작업 이름에는 폴더 경로 해시를 붙인다.
//! 같은 경로를 이미 쓰는 살아 있는 실행이 있으면 출발하지 않는다(기록 저장소 `path_claims`, 6단계 ⑫-6).

use std::path::{Path, PathBuf};

use claude_board_core::envs::{EnvChoice, EnvEntry, EnvKind};
use claude_board_core::test_claude::Decision;
use serde_json::{json, Map, Value};

use super::roots::{self, Forbidden};
use super::{first_line, label_for_dir, native_env, read_required, refuse, run_id_of, valid_run_id, Deps, Fail, Reply, ENGINE_EXE};
use crate::adapters::git::autopilot_branch_name;
use crate::adapters::wsl_env::WslEnv;
use crate::engine::claims::{claim_paths_of, EngineClaimProbe, SqliteClaimBook};
use crate::engine::run::frozen_digest;
use crate::model::{ExecEnv as ExecEnvRecord, Num, Ordered, QueueFile, RunFile, UnsandboxedOk};
use crate::ports::{ExecEnv, Git, KeepAlive};
use crate::rules::env_policy::{choose, confirm_ready, record_for, validate_check};
use crate::rules::multirun::{claim_busy_text, claim_paths, release_claims, ClaimResult};

struct Loaded {
    id: String,
    /// 설정 칸을 그대로 옮기려고 쥔 원본 값
    run_v: Value,
    queue_v: Value,
    run: RunFile,
    queue: QueueFile,
}

/// 걸음 1 — 폴더·이름·파일. 이미 출발했으면 거부
fn load_run(d: &Deps, dir: &Path) -> Result<Loaded, Fail> {
    if !dir.is_dir() {
        return Err(refuse(format!("실행 폴더가 없습니다: {}", dir.display())));
    }
    let id = run_id_of(dir);
    if !valid_run_id(&id) {
        return Err(refuse(format!("실행 폴더 이름이 실행 id 규칙(영문·숫자·점·밑줄·붙임표, 120자 이하)에 맞지 않습니다: {id}")));
    }
    let run_v = read_required(d, &dir.join("run.json"), "run.json")?;
    let queue_v = read_required(d, &dir.join("queue.json"), "queue.json")?;
    let run: RunFile = serde_json::from_value(run_v.clone()).map_err(|e| refuse(format!("run.json 꼴이 틀립니다: {e}")))?;
    let queue: QueueFile = serde_json::from_value(queue_v.clone()).map_err(|e| refuse(format!("queue.json 꼴이 틀립니다: {e}")))?;
    let pinned = d.files.stat(&dir.join("pinned").join("frozen.json")).map_err(|e| Fail::Internal(e.message))?.is_some();
    let started = run.state.get().is_some_and(|s| !s.is_empty() && s != "starting");
    if pinned || started {
        return Err(refuse("이미 출발한 실행입니다."));
    }
    Ok(Loaded { id, run_v, queue_v, run, queue })
}

fn unit_title(queue: &QueueFile, n: usize) -> String {
    queue[n - 1].title.get().cloned().unwrap_or_else(|| format!("단위 {n}"))
}

/// 걸음 2 — 단위마다 판정 명령과 작업 폴더. 단위마다 한 줄
fn unit_problems(queue: &QueueFile) -> Vec<String> {
    if queue.is_empty() {
        return vec!["대기열(queue.json)에 단위가 하나도 없습니다.".to_string()];
    }
    let mut out = Vec::new();
    for n in 1..=queue.len() {
        let u = &queue[n - 1];
        let mut why: Vec<String> = Vec::new();
        if u.check.get().is_none_or(|c| c.trim().is_empty()) {
            why.push("판정 명령(check)이 없습니다".to_string());
        }
        match u.cwd.get().map(String::as_str) {
            None | Some("") => why.push("작업 폴더(cwd)가 없습니다".to_string()),
            Some(cwd) if !Path::new(cwd).is_absolute() => why.push(format!("작업 폴더(cwd)가 절대 경로가 아닙니다: {cwd}")),
            Some(cwd) if !Path::new(cwd).is_dir() => why.push(format!("작업 폴더(cwd)가 폴더가 아니거나 없습니다: {cwd}")),
            Some(_) => {}
        }
        if !why.is_empty() {
            out.push(format!("단위 {n}({}): {}.", unit_title(queue, n), why.join("; ")));
        }
    }
    out
}

/// 걸음 3·4 — 실행 환경 문턱과 판정 명령 꼴. (고른 환경, 기록할 `exec_env` 초안)
fn pick_env(
    l: &Loaded,
    entries: &[EnvEntry],
    accept_unsandboxed: bool,
    now: f64,
) -> Result<(EnvEntry, Option<UnsandboxedOk>), Fail> {
    let mut rec: Option<ExecEnvRecord> = l.run.exec_env.get().map(|o| (**o).clone());
    if let Some(r) = rec.as_mut().filter(|r| accept_unsandboxed && r.unsandboxed_ok.is_none()) {
        r.unsandboxed_ok = Some(UnsandboxedOk { at: Num::r3(now), by: "cli".to_string() });
    }
    let choices: Vec<EnvChoice> = entries.iter().map(|e| e.choice.clone()).collect();
    let choice = choose(rec.as_ref(), &choices).map_err(refuse)?;
    let problems: Vec<String> = (1..=l.queue.len())
        .filter_map(|n| {
            let check = l.queue[n - 1].check.get()?;
            validate_check(choice.kind, check).err().map(|m| format!("단위 {n}({}): {m}", unit_title(&l.queue, n)))
        })
        .collect();
    if !problems.is_empty() {
        return Err(Fail::Refused(problems));
    }
    let entry = entries.iter().find(|e| e.choice == choice).cloned().ok_or_else(|| refuse("고른 실행 환경을 찾지 못했습니다."))?;
    Ok((entry, rec.and_then(|r| r.unsandboxed_ok)))
}

/// 시험 모드에서 고정하는 `claude_cmd`(조각 배열)와 `claude_env`(이름·값). 꺼짐이면 None
pub type TestClaude = Option<(Vec<String>, Vec<(String, String)>)>;

/// 걸음 5 — 시험용 claude 판정과 준비 확인. (시험 모드의 고정 값, 이 실행의 sandbox 값)
fn confirm_env(d: &Deps, entry: &EnvEntry, first_cwd: &Path) -> Result<(TestClaude, bool), Fail> {
    let test = match (d.test_claude)() {
        Decision::Off => None,
        Decision::On(parts) => {
            Some((parts.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(), (d.fake_env)()))
        }
        Decision::Rejected(why) => return Err(refuse(format!("시험용 claude 설정이 어긋납니다: {why}"))),
    };
    let env: Box<dyn ExecEnv> = if entry.choice.kind == EnvKind::Wsl {
        // 시험용 claude 는 Windows 프로그램(node.exe 등)이라 배포판 안에서 돌릴 수 없다. 진짜 claude 가 대신 뜨지 않게 여기서 멈춘다
        if test.is_some() {
            return Err(refuse("시험용 claude 는 WSL 실행 환경에서 쓸 수 없습니다."));
        }
        Box::new(WslEnv::from_entry(entry).map_err(|e| refuse(e.message))?)
    } else {
        Box::new(native_env(entry, test.as_ref().map(|(cmd, _)| cmd.as_slice())).map_err(|e| refuse(e.message))?)
    };
    let report = env.check_ready(first_cwd);
    let sandbox = confirm_ready(&entry.choice, &report).map_err(refuse)?;
    Ok((test, sandbox))
}

// ── 가지 ─────────────────────────────────────────────

/// 가지를 만든 저장소 하나 — 되돌릴 때 필요한 것
struct Repo {
    path: PathBuf,
    original: Option<String>,
    original_head: String,
    dirty: Vec<String>,
}

fn git_text(git: &dyn Git, repo: &Path, args: &[&str]) -> Option<String> {
    let o = git.run(repo, args).ok()?;
    (o.code == 0).then(|| o.stdout.trim().to_string()).filter(|t| !t.is_empty())
}

/// 단위 작업 폴더가 속한 저장소 뿌리들(중복 없이, 대소문자 무시). 저장소가 아니면 건너뛴다
fn repos_of(git: &dyn Git, cwds: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for cwd in cwds {
        let Some(top) = git_text(git, cwd, &["rev-parse", "--show-toplevel"]) else { continue };
        let top = PathBuf::from(top.replace('/', std::path::MAIN_SEPARATOR_STR));
        let key = top.to_string_lossy().to_lowercase();
        if !out.iter().any(|p| p.to_string_lossy().to_lowercase() == key) {
            out.push(top);
        }
    }
    out
}

/// 바꾼 저장소를 원래 가지로 되돌리고 만든 가지를 지운다
fn undo_branches(git: &dyn Git, repos: &[Repo], work: &str) {
    for r in repos.iter().rev() {
        let _ = match (&r.original, r.original_head.is_empty()) {
            (Some(o), _) => git.run(&r.path, &["switch", o]),
            (None, false) => git.run(&r.path, &["switch", "--detach", &r.original_head]),
            (None, true) => continue,
        };
        let _ = git.run(&r.path, &["branch", "-D", work]);
    }
}

/// 걸음 6 — 저장소마다 지금 가지·머리를 적고 `git switch -c <work>`. 하나라도 실패하면 바꾼 것을 되돌린다
fn create_branches(git: &dyn Git, cwds: &[PathBuf], work: &str) -> Result<Vec<Repo>, String> {
    let mut made: Vec<Repo> = Vec::new();
    for path in repos_of(git, cwds) {
        let dirty = git
            .run(&path, &["status", "--porcelain"])
            .ok()
            .filter(|o| o.code == 0)
            .map(|o| o.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        let repo = Repo {
            original: git_text(git, &path, &["symbolic-ref", "-q", "--short", "HEAD"]),
            original_head: git_text(git, &path, &["rev-parse", "HEAD"]).unwrap_or_default(),
            dirty,
            path,
        };
        let why = match git.run(&repo.path, &["switch", "-c", work]) {
            Ok(o) if o.code == 0 => None,
            Ok(o) => Some(first_line(&format!("{}\n{}", o.stderr, o.stdout))),
            Err(e) => Some(e.message),
        };
        if let Some(why) = why {
            let at = repo.path.display().to_string();
            undo_branches(git, &made, work);
            return Err(format!("가지를 만들지 못했습니다({at}): {why}"));
        }
        made.push(repo);
    }
    Ok(made)
}

fn branches_value(repos: &[Repo], work: &str) -> Value {
    let mut by_repo = Map::new();
    for r in repos {
        by_repo.insert(
            r.path.to_string_lossy().into_owned(),
            json!({
                "base": {"branch": r.original.clone().unwrap_or_default(), "head": r.original_head},
                "work": work,
                "on": work,
                "original": r.original,
                "original_head": r.original_head,
                "dirty_start": r.dirty,
                "restored": null,
            }),
        );
    }
    json!({"repos": by_repo})
}

// ── 고정 설정 ────────────────────────────────────────

/// run.json 에서 frozen 으로 옮기는 설정 칸. `claude_args`(시험용)는 `--dev` 일 때만 `build_frozen` 이 따로 옮긴다
const FROZEN_FROM_RUN: [&str; 18] = [
    "model",
    "effort",
    "stopline",
    "caps",
    "check_timeout_min",
    "check_retries",
    "precheck_retry_min",
    "background_quiet_sec",
    "allowed_tools",
    "disallowed_tools",
    "write_roots",
    "read_deny",
    "project",
    "project_root",
    "title",
    // 엔진이 세션 정책을 만들 때 읽는다: 프로젝트 자율주행 지시(`set.instruction`), 네트워크 허용 목록, localhost 모든 포트
    "set",
    "network_allow",
    "local_binding",
];

/// 설정 칸의 꼴 확인. 틀리면 출발하지 않는다(엔진은 틀린 꼴을 조용히 빈 값으로 읽는다)
fn settings_problems(run: &Value, dev: bool) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["network_allow", "allowed_tools", "disallowed_tools", "claude_args"] {
        if let Some(v) = run.get(key).filter(|v| !v.is_null()) {
            if !v.as_array().is_some_and(|a| a.iter().all(Value::is_string)) {
                out.push(format!("run.json 의 {key} 는 글자 배열이어야 합니다."));
            }
        }
    }
    if run.get("local_binding").is_some_and(|v| !v.is_null() && !v.is_boolean()) {
        out.push("run.json 의 local_binding 은 true/false 여야 합니다.".to_string());
    }
    if run.get("set").is_some_and(|v| !v.is_null() && !v.is_object()) {
        out.push("run.json 의 set 은 객체여야 합니다.".to_string());
    }
    if !dev && run.get("claude_args").is_some_and(|v| !v.is_null()) {
        out.push("run.json 의 claude_args 는 시험용이라 --dev 로 출발할 때만 받습니다.".to_string());
    }
    out
}

/// 경로 확인을 마친 값(정규 경로)으로 고정 설정의 작업 폴더·쓰기 범위·읽기 금지를 바꾼다. 원래 없던 칸은 만들지 않는다
fn apply_checked(frozen: &mut Value, checked: &roots::Checked) {
    if let Some(units) = frozen.get_mut("units").and_then(Value::as_array_mut) {
        for (unit, cwd) in units.iter_mut().zip(&checked.cwds) {
            if let Some(slot) = unit.get_mut("cwd") {
                *slot = Value::from(cwd.to_string_lossy().into_owned());
            }
        }
    }
    for (key, list) in [("write_roots", &checked.write_roots), ("read_deny", &checked.read_deny)] {
        if let Some(slot) = frozen.get_mut(key) {
            *slot = json!(list);
        }
    }
}

/// `pinned/frozen.json` 에 쓸 값. 키는 이름 순이다(macOS 가 쓴 것과 같은 순서). 시험 모드면 `claude_cmd`·`claude_env` 도 고정한다
pub fn build_frozen(
    run: &Value,
    queue: &Value,
    exec_env: Value,
    id: &str,
    dev: bool,
    now: f64,
    test_claude: TestClaude,
) -> Value {
    let mut fields: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for key in FROZEN_FROM_RUN {
        if let Some(v) = run.get(key) {
            fields.insert(key.to_string(), v.clone());
        }
    }
    if let (true, Some(v)) = (dev, run.get("claude_args")) {
        fields.insert("claude_args".into(), v.clone());
    }
    fields.insert("units".into(), queue.clone());
    fields.insert("exec_env".into(), exec_env);
    if let Some((cmd, env)) = test_claude {
        fields.insert("claude_cmd".into(), json!(cmd));
        fields.insert("claude_env".into(), Value::Object(env.into_iter().map(|(k, v)| (k, Value::from(v))).collect()));
    }
    fields.insert("id".into(), Value::from(id));
    fields.insert("dev".into(), Value::from(dev));
    fields.insert("frozen_at".into(), Num::r3(now).raw().clone());
    fields.insert("version".into(), Value::from(1));
    Value::Object(fields.into_iter().collect())
}

/// 걸음 7 에서 만든 것을 지운다
fn remove_frozen(dir: &Path, private_dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.join("pinned"));
    let _ = std::fs::remove_dir_all(private_dir);
}

/// 걸음 7 — 엔진 전용 폴더·`pinned\`·고정 설정·엔진 사본. 고정 설정의 해시를 돌려준다
fn write_pinned(d: &Deps, dir: &Path, private_dir: &Path, frozen: &Value) -> Result<(String, PathBuf), String> {
    let pinned = dir.join("pinned");
    d.files.make_private_dir(private_dir).map_err(|e| e.message)?;
    d.files.make_private_dir(&pinned).map_err(|e| e.message)?;
    d.files.write_json(&pinned.join("frozen.json"), frozen).map_err(|e| e.message)?;
    let engine = pinned.join(ENGINE_EXE);
    std::fs::copy(&d.engine_exe, &engine).map_err(|e| format!("엔진 실행 파일({})을 복사하지 못했습니다: {e}", d.engine_exe.display()))?;
    Ok((frozen_digest(frozen), engine))
}

fn write_value<T: serde::Serialize>(d: &Deps, path: &Path, value: &T) -> Result<(), String> {
    let v = serde_json::to_value(value).map_err(|e| e.to_string())?;
    d.files.write_json(path, &v).map_err(|e| e.message)
}

/// 같은 이름의 실행이 이미 쓰는 자리가 있으면 거부한다: 엔진 전용 폴더(`private\<id>`)가 이미 있거나(다른 부모 폴더의 같은 이름 실행),
/// 이 폴더의 작업이 작업 스케줄러에 이미 있으면. 되돌리기(`remove_frozen`)가 남의 엔진 전용 폴더를 지우는 일을 이것이 막는다
fn refuse_if_taken(d: &Deps, dev: bool, no_schedule: bool, private_dir: &Path, label: &str) -> Result<(), Fail> {
    if private_dir.exists() {
        return Err(refuse(format!(
            "같은 이름의 실행이 이미 엔진 전용 폴더를 쓰고 있습니다: {}. 실행 폴더 이름을 바꾸거나 그 실행을 끝낸 뒤 다시 출발하세요.",
            private_dir.display()
        )));
    }
    // 스케줄러를 읽지 못하면(Err) 판단하지 않는다 — 등록할 때 같은 까닭으로 실패하고 되돌린다
    if !no_schedule && (d.supervisor)(dev).is_registered(label).unwrap_or(false) {
        return Err(refuse(format!("이 실행 폴더의 작업이 이미 작업 스케줄러에 있습니다({label}). 끝난 실행이면 unregister 로 지운 뒤 출발하세요.")));
    }
    Ok(())
}

// ── start 본체 ───────────────────────────────────────

pub(super) fn start(d: &Deps, dir: &Path, dev: bool, accept_unsandboxed: bool, no_schedule: bool) -> Result<Reply, Fail> {
    if no_schedule && !d.home_set {
        return Err(Fail::Usage("--no-schedule 은 시험 전용이라 CLAUDE_BOARD_HOME 이 있을 때만 받습니다.".to_string()));
    }
    let now = d.clock.now();
    let mut l = load_run(d, dir)?;
    let mut problems = unit_problems(&l.queue);
    problems.extend(settings_problems(&l.run_v, dev));
    if !problems.is_empty() {
        return Err(Fail::Refused(problems));
    }
    let entries = (d.detect)();
    let (entry, consent) = pick_env(&l, &entries, accept_unsandboxed, now)?;

    let id = l.id.clone();
    let private_dir = (d.autopilot_root)(dev).join("private").join(&id);
    let forbidden = Forbidden::from_env(dir, &private_dir);
    let checked = roots::check(&forbidden, entry.choice.kind, &l.queue, &l.run_v).map_err(Fail::Refused)?;
    let label = label_for_dir(dev, dir);
    refuse_if_taken(d, dev, no_schedule, &private_dir, &label)?;
    let cwds = checked.cwds.clone();
    let (test, sandbox) = confirm_env(d, &entry, &cwds[0])?;

    // 6단계 ⑫-6(apClaimPaths) — 같은 경로 자리 잡기. 가지·작업 트리를 만들기 전에 기록 저장소의 경로 자리 표를 한 transaction 으로 잡아,
    // 거의 같은 때 온 두 출발 가운데 하나만 지나간다. 단위 작업 폴더가 속한 저장소 각각과 저장소 밖 작업 폴더 그 자리(원본 cmdStart 의 claimPathsOf).
    // 겹치는 자리의 실행이 살아 있으면 거부하고, 끝났거나 엔진·작업이 없는 남은 자리는 치우고 잡는다. 표를 못 쓰면 출발하지 않는다(닫힌 쪽). 잡은 뒤 실패하면 푼다.
    let private_root = (d.autopilot_root)(dev).join("private");
    d.files.make_private_dir(&private_root).map_err(|e| Fail::Internal(format!(
        "엔진 전용 폴더({})를 만들지 못했습니다: {}", private_root.display(), e.message
    )))?;
    let book = SqliteClaimBook { path: private_root.join("records.sqlite") };
    let home = claude_board_core::paths::home().to_string_lossy().into_owned();
    let supervisor = (d.supervisor)(dev);
    let probe = EngineClaimProbe { supervisor: &*supervisor };
    let places = claim_paths_of(&json!({
        "repos": repos_of(d.git, &cwds).iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "units": cwds.iter().map(|c| json!({ "cwd": c.to_string_lossy().into_owned() })).collect::<Vec<_>>(),
    }));
    let claimed = claim_paths(
        &id, &dir.to_string_lossy(), &label, Some(std::process::id() as i32), &places, &home, &book, &probe, d.clock,
    );
    let cleared_note = match &claimed {
        ClaimResult::Ok { cleared } => (!cleared.is_empty()).then(|| cleared.join(", ")),
        ClaimResult::Busy { .. } => {
            return Err(refuse(
                claim_busy_text(&claimed, &home).unwrap_or_else(|| "같은 경로에서 자율주행이 돌고 있습니다".to_string()),
            ));
        }
        ClaimResult::Failed(e) => {
            return Err(refuse(format!(
                "실행 기록 저장소에 같은 경로 자리를 잡지 못했습니다({e}). 같은 경로의 실행과 겹치는지 볼 수 없어 출발하지 않습니다"
            )));
        }
    };

    let work = match autopilot_branch_name(&id) {
        Ok(work) => work,
        Err(e) => {
            let _ = release_claims(&id, &book);
            return Err(refuse(e.message));
        }
    };
    let repos = match create_branches(d.git, &cwds, &work) {
        Ok(repos) => repos,
        Err(why) => {
            let _ = release_claims(&id, &book);
            return Err(refuse(why));
        }
    };
    let undo_all = |why: &str| -> Fail {
        let _ = release_claims(&id, &book);
        remove_frozen(dir, &private_dir);
        undo_branches(d.git, &repos, &work);
        Fail::Internal(why.to_string())
    };

    let record = record_for(&entry.choice, sandbox, consent);
    let record_v = serde_json::to_value(&record).unwrap_or(Value::Null);
    let mut frozen = build_frozen(&l.run_v, &l.queue_v, record_v, &id, dev, now, test);
    apply_checked(&mut frozen, &checked);
    let (hash, engine) = match write_pinned(d, dir, &private_dir, &frozen) {
        Ok(x) => x,
        Err(why) => return Err(undo_all(&format!("고정 설정을 쓰지 못했습니다: {why}"))),
    };

    for u in l.queue.iter_mut().filter(|u| u.status.is_absent()) {
        u.status.set("pending".to_string());
    }
    l.run.exec_env.set(Ordered::new(record.clone()));
    l.run.state.set("starting".to_string());
    l.run.reason.set("출발을 기다립니다.".to_string());
    l.run.created.set(Num::r3(now));
    l.run.dev.set(dev);
    l.run.launchd_label.set(label.clone());
    l.run.sessions.set(Vec::new());
    l.run.unit.set_null();
    l.run.wait_until.set_null();
    l.run.extra.insert("branches".into(), branches_value(&repos, &work));
    let written = write_value(d, &dir.join("queue.json"), &l.queue).and_then(|()| write_value(d, &dir.join("run.json"), &l.run));
    if let Err(why) = written {
        return Err(undo_all(&format!("실행 폴더 파일을 쓰지 못했습니다: {why}")));
    }

    if !no_schedule {
        let job = KeepAlive {
            label: label.clone(),
            program: engine,
            args: vec!["run".to_string(), dir.to_string_lossy().into_owned(), hash.clone()],
            working_dir: dir.to_path_buf(),
        };
        if let Err(e) = (d.supervisor)(dev).register(&job) {
            let why = format!("작업 스케줄러에 등록하지 못했습니다: {}", e.message);
            // 사람이 까닭을 볼 수 있게 run.json 은 error 로 남기고, 고정 설정과 가지만 되돌린다
            l.run.state.set("error".to_string());
            l.run.reason.set(why.clone());
            let _ = write_value(d, &dir.join("run.json"), &l.run);
            return Err(undo_all(&why));
        }
    }

    let scheduled = !no_schedule;
    let mut text = format!(
        "출발했습니다: {id}\n실행 환경: {}\n작업 스케줄러: {}",
        env_label(&entry.choice),
        if scheduled { "등록했습니다" } else { "등록하지 않았습니다(--no-schedule)" }
    );
    if let Some(cleared) = &cleared_note {
        // 원본 cmdStart 이 .ok(cleared) 에 표준 출력으로 내놓는 알림 줄
        text.push_str(&format!("\n끝났거나 엔진이 없는 실행의 남은 경로 자리를 치웠습니다: {cleared}"));
    }
    let json = json!({"ok": true, "run": id, "label": label, "hash": hash, "exec_env": record, "scheduled": scheduled});
    Ok(Reply { json, text })
}

fn env_label(choice: &EnvChoice) -> String {
    match &choice.distro {
        Some(d) => format!("{} · {d}", choice.kind.label()),
        None => choice.kind.label().to_string(),
    }
}
