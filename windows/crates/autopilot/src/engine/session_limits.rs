//! 결과 폴더에만 쓰는 모델 세션(초안·판정 비판·세트 이어 쓰기류)의 Windows 판 — 원본: main.swift `readSession`(7471~7498줄)
//! 와 `sessionDenyRead`(7501~7504줄). 띄우기·출력 읽기·시간 상한·취소·kill_tree 는 `session_runner::run_launch` 를 같이 쓴다.
//!
//! - 모델은 `out` 폴더만 고칠 수 있다(result 폴더): 쓰기 허용 규칙 `Edit(<out>/**)` 는 설정 파일로만 건다. `--allowedTools` 에
//!   도구 이름만 나열하면 결과 폴더 밖 쓰기까지 허용되므로 명령줄에는 붙이지 않는다. 읽을 자리는 `--add-dir` 로 연다.
//! - 읽을 자리와 상관없이 읽지 못하게 할 자리는 `session_deny_read` — 홈의 비밀 폴더 다섯과 각 자율주행 뿌리의 엔진 전용
//!   폴더(private)·실행 폴더(runs)를 설정 파일의 읽기 금지 규칙으로 내려보낸다. 원본의 키체인 자리는 Windows 에 없다.
//! - 원본의 고정 설정 꼴(apDraftSettingsJSON) 대신 `ExecEnv::render_settings` 로 권한 규칙을 만든다 — 경로 → 규칙 꼴 변환은
//!   환경(Windows/WSL)이 정한다. `claude_args` 가 `-p`·`--input-format stream-json`·`--settings`·`--add-dir` 를 붙인다.

use std::path::{Path, PathBuf};

use crate::engine::session_runner::{
    fail, run_launch, stderr_path, unique_session_id, CancelToken, SessionOutcome, SessionRequest, DENIED_TOOLS,
};
use crate::ports::{ClaudeLaunch, ExecEnv, SessionPolicy, SessionStart};
use crate::rules::draft::DRAFT_TOOLS;

/// 결과 폴더에만 쓰는 세션의 준비물 — `req` 는 지시·시간·예산·환경을 갖고, `out` 에만 쓰며 `reads` 만 읽는다.
/// 원본: `readSession` 의 `paths`·`out`·`reads` 자리
#[derive(Debug, Clone)]
pub struct OutSession {
    /// 세션 공통 준비물 — 지시 글·모델·예산·회전 상한·시간 상한·엔진 폴더
    pub req: SessionRequest,
    /// 모델이 결과를 쓰는 유일한 폴더 — 띄우기 작업 폴더이자 쓰기 허용 자리
    pub out: PathBuf,
    /// 모델이 읽을 자리들 — `--add-dir` 로 연다
    pub reads: Vec<PathBuf>,
    /// 읽지 못하게 막을 자리들 — 설정 파일의 읽기 금지 규칙으로 내려간다
    pub deny_read: Vec<PathBuf>,
}

/// 결과 폴더 세션에서 막는 도구 — 거부 목록(`DENIED_TOOLS`)에서 이 세션이 쓰는 `Write`·`Edit` 를 뺀다.
/// 도구 목록은 소스에서 튼다 — 거부 목록이 바뀌면 여기도 따라 간다
pub(crate) fn denied_out_tools() -> Vec<String> {
    DENIED_TOOLS
        .iter()
        .copied()
        .filter(|tool| *tool != "Write" && *tool != "Edit")
        .map(String::from)
        .collect()
}

/// 세션이 읽지 못하게 막는 자리(결과 폴더 세션 공통) — 원본: `sessionDenyRead`. 홈의 비밀이 흔한 폴더 다섯과 각
/// 자율주행 뿌리의 엔진 전용 폴더(private)·실행 폴더(runs)를 막는다. 키체인 자리는 Windows 에 없다
pub fn session_deny_read(home: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut denied: Vec<PathBuf> = [".ssh", ".aws", ".gnupg", ".claude", ".config"]
        .iter()
        .map(|folder| home.join(folder))
        .collect();
    for root in roots {
        denied.push(root.join("private"));
        denied.push(root.join("runs"));
    }
    denied
}

/// 결과 폴더 세션이 claude 에 줄 추가 인자 — `-p`·`--settings`·`--add-dir` 는 `spawn_claude`(`claude_args`)가 붙이므로 여기엔 없다.
/// 쓰기 범위는 설정 파일의 `Edit(<out>/**)` 규칙만 연다. 예산·회전 상한은 세미콜론 없는 낱 인자로 내려간다.
/// 원본: `apDraftArgs` — Windows 판에서 도구·권한 쪽만 `--tools`·설정 파일로 다시 나눈다
pub fn out_args(req: &SessionRequest) -> Vec<String> {
    let mut args: Vec<String> = ["--setting-sources", "project,local", "--permission-mode", "dontAsk", "--permission-prompts", "none", "--strict-mcp-config"]
        .map(String::from)
        .to_vec();
    args.extend(["--tools".to_string(), DRAFT_TOOLS.join(",")]);
    args.extend(["--disallowedTools".to_string(), denied_out_tools().join(" ")]);
    args.extend(["--max-budget-usd".to_string(), format!("{:.2}", req.max_budget_usd.max(0.01))]);
    args.extend(["--max-turns".to_string(), req.max_turns.max(1).to_string()]);
    args
}

/// 결과 폴더에만 쓰는 세션을 돌린다 — 정책(초안 도구·결과 폴더 쓰기·읽기 금지)을 만들어 설정 파일(`render_settings`)로 내리고,
/// `out` 에서 띄워 `reads` 만 `--add-dir` 로 열어 같은 세션 루프(`run_launch`)를 돌린다.
/// 원본: `readSession` — Windows 판에서 설정·인자 만드는 일만 환경 포트의 몫으로 다시 나눴다
pub fn run_out_session(env: &dyn ExecEnv, s: &OutSession, cancel: &CancelToken) -> SessionOutcome {
    if cancel.is_cancelled() {
        return SessionOutcome::Cancelled;
    }
    if !s.out.is_dir() {
        return fail(format!("결과 폴더가 없다: {}", s.out.display()));
    }
    if let Err(e) = std::fs::create_dir_all(&s.req.scratch_dir) {
        return fail(format!("엔진 폴더를 만들지 못했다: {e}"));
    }
    // 정책 — 초안 도구만 내주고, 결과 폴더에만 쓰며, 읽을 자리와 상관없이 비밀 자리는 못 읽는다
    let policy = SessionPolicy {
        allow_write: vec![s.out.clone()],
        deny_read: s.deny_read.clone(),
        allowed_tools: DRAFT_TOOLS.iter().map(|tool| tool.to_string()).collect(),
        disallowed_tools: denied_out_tools(),
        ..SessionPolicy::default()
    };
    let settings = match env.render_settings(&policy) {
        Ok(v) => v,
        Err(e) => return fail(format!("설정을 만들지 못했다: {}", e.message)),
    };
    let settings_file = s.req.scratch_dir.join("settings.json");
    if let Err(e) = std::fs::write(&settings_file, settings.to_string()) {
        return fail(format!("설정 파일을 쓰지 못했다: {e}"));
    }
    let launch = ClaudeLaunch {
        session: SessionStart::New(unique_session_id()),
        cwd: s.out.clone(),
        model: s.req.model.clone(),
        effort: s.req.effort.clone(),
        settings_file,
        system_prompt_file: None,
        add_dirs: s.reads.clone(),
        plugin_dir: None,
        env: s.req.env.clone(),
        path_env: Vec::new(),
        extra_args: out_args(&s.req),
        stderr_log: stderr_path(&s.req.scratch_dir),
    };
    run_launch(env, &launch, &s.req, cancel)
}
