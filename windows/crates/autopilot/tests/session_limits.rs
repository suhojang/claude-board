//! 결과 폴더에만 쓰는 모델 세션(`engine::session_limits`) 시험 — 가짜 실행 환경(tests/fakes)으로 정책·설정 파일·인자·
//! 시간 상한·취소·실패 지점을 본다. 진짜 프로세스·kill_tree 실측은 tests/session_runner.rs 의 가짜 claude 가 한다.

mod fakes;

use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_autopilot::adapters::native_env::claude_args;
use claude_board_autopilot::engine::session_limits::{out_args, run_out_session, session_deny_read, OutSession};
use claude_board_autopilot::engine::session_runner::{
    prompt_line, CancelToken, SessionOutcome, SessionRequest, DENIED_TOOLS, MAX_LINE_BYTES,
};
use claude_board_autopilot::rules::draft::DRAFT_TOOLS;
use fakes::*;
use serde_json::Value;

/// 가짜 환경이 경로를 적어둔 꼴(`/`)로 맞춰 본다 — `fakes::key` 와 같은 규칙
fn plain(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 결과 폴더 세션에서 거부해야 할 도구 — 이 세션이 쓰는 Write·Edit 를 뺀다
fn denied_out_expect() -> Vec<String> {
    DENIED_TOOLS
        .iter()
        .copied()
        .filter(|tool| *tool != "Write" && *tool != "Edit")
        .map(String::from)
        .collect()
}

/// 세션 준비물 — 엔진 폴더는 진짜 임시 폴더(설정 파일·stderr 를 진짜로 쓴다), 나머지 경로 꼴만 필요하다.
/// 결과 폴더는 부르는 쪽(초안 흐름)이 미리 만든다 — 여기서도 같이 만든다
fn out_session(root: &Path) -> OutSession {
    std::fs::create_dir_all(root.join("out")).unwrap();
    OutSession {
        req: SessionRequest {
            prompt: "초안 지시 \"인용\"".to_string(),
            cwd: root.join("cwd-unused"),
            model: "opus".to_string(),
            effort: Some("high".to_string()),
            max_budget_usd: 0.5,
            max_turns: 6,
            timeout: Duration::from_secs(20),
            scratch_dir: root.join("engine"),
            env: vec![("FAKE_OUT_MARK".to_string(), "out-1".to_string())],
            max_line_bytes: MAX_LINE_BYTES,
        },
        out: root.join("out"),
        reads: vec![root.join("repo"), root.join("bundle")],
        deny_read: vec![root.join("home").join(".ssh"), root.join("home").join(".claude")],
    }
}

/// 뿌리 두 개를 막는 읽기 금지 목록 — `session_deny_read` 꼴 그대로
fn deny_read_of(root: &Path) -> Vec<PathBuf> {
    session_deny_read(
        &root.join("home"),
        &[root.join("ap-inst"), root.join("ap-dev")],
    )
}

/// 성공 턴 세션을 돌려 둔다 — 출발 검증이 끝난 뒤의 상태를 보는 데 쓴다
fn run_one(env: &ScriptedEnv, session: &OutSession) -> SessionOutcome {
    env.push_spawn(ChildScript::ok_turn(0.2));
    run_out_session(env, session, &CancelToken::new())
}

#[test]
fn deny_read_marks_secret_folders_and_every_root() {
    let home = PathBuf::from("C:/Users/u");
    let root_a = PathBuf::from("C:/ap-inst");
    let root_b = PathBuf::from("C:/ap-dev");
    let denied = session_deny_read(&home, &[root_a.clone(), root_b.clone()]);
    // 홈 비밀 폴더 다섯 + 뿌리마다 엔진 전용(private)·실행(runs) 폴더. 키체인 자리는 Windows 에 없다
    let expect = vec![
        home.join(".ssh"),
        home.join(".aws"),
        home.join(".gnupg"),
        home.join(".claude"),
        home.join(".config"),
        root_a.join("private"),
        root_a.join("runs"),
        root_b.join("private"),
        root_b.join("runs"),
    ];
    assert_eq!(denied, expect);
    let joined = denied.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join(";");
    assert!(!joined.contains("Keychain"), "키체인 경로가 섞였다: {joined}");
    // 뿌리가 없어도 홈의 비밀 다섯은 늘 막힌다
    assert_eq!(session_deny_read(&home, &[]), vec![home.join(".ssh"), home.join(".aws"), home.join(".gnupg"), home.join(".claude"), home.join(".config")]);
}

#[test]
fn settings_allow_writes_only_the_out_folder() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    let mut session = out_session(root.path());
    session.deny_read = deny_read_of(root.path());
    let outcome = run_one(&env, &session);
    match &outcome {
        SessionOutcome::Done { text, cost, .. } => {
            assert_eq!(text, "ok");
            assert!((cost - 0.2).abs() < 1e-9);
        }
        other => panic!("정상 턴이어야 한다: {other:?}"),
    }
    // 정책 — 결과 폴더에만 쓰고, 초안 도구만 내주고, 막을 자리는 통째로 내려간다
    let policy = env.policies.lock().unwrap()[0].clone();
    assert_eq!(policy.allow_write, vec![session.out.clone()]);
    assert_eq!(policy.deny_read, session.deny_read.clone());
    assert_eq!(policy.allowed_tools, DRAFT_TOOLS.iter().map(|t| t.to_string()).collect::<Vec<_>>());
    assert_eq!(policy.disallowed_tools, denied_out_expect());
    assert!(policy.deny_write.is_empty());
    assert!(policy.network_allow.is_empty());
    // 설정 파일에도 결과 폴더 하나만 허용으로 적혔다
    let settings_text = std::fs::read_to_string(root.path().join("engine").join("settings.json")).unwrap();
    let settings: Value = serde_json::from_str(&settings_text).unwrap();
    assert_eq!(settings["allow_write"], serde_json::json!([plain(&session.out)]));
}

#[test]
fn launch_runs_in_out_and_opens_only_reads() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    let session = out_session(root.path());
    run_one(&env, &session);
    let launch = &env.launches()[0];
    assert_eq!(launch.cwd, session.out);
    assert_eq!(launch.add_dirs, session.reads.clone());
    assert_eq!(launch.model, "opus");
    assert_eq!(launch.effort, Some("high".to_string()));
    assert_eq!(launch.env, vec![("FAKE_OUT_MARK".to_string(), "out-1".to_string())]);
    assert!(launch.settings_file.starts_with(root.path().join("engine")));
    assert!(launch.stderr_log.starts_with(root.path().join("engine")));
    // 인자 — `-p`·스트림 꼴·설정 파일·읽을 자리는 띄우기(claude_args)가 붙이고, 도구 상한은 여기서 내린다
    let argv = claude_args(launch);
    assert!(argv.contains(&"-p".to_string()));
    assert!(argv.windows(2).any(|w| w[0] == "--input-format" && w[1] == "stream-json"));
    assert!(argv.windows(2).any(|w| w[0] == "--settings" && w[1].starts_with(root.path().to_string_lossy().as_ref())));
    for read in &session.reads {
        assert!(argv.windows(2).any(|w| w[0] == "--add-dir" && Path::new(&w[1]) == read), "읽을 자리가 안 열렸다: {read:?} / {argv:?}");
    }
    assert!(argv.windows(2).any(|w| w[0] == "--tools" && w[1] == DRAFT_TOOLS.join(",")));
    let denied = argv.windows(2).find(|w| w[0] == "--disallowedTools").expect("거부 도구 인자가 없다")[1].clone();
    assert_eq!(denied.split(' ').map(String::from).collect::<Vec<_>>(), denied_out_expect());
    assert!(!argv.iter().any(|a| a == "--allowedTools"), "쓰기 허용은 설정 파일 규칙만 — 도구 낱 이름을 내리면 넓어진다");
    assert!(!argv.iter().any(|a| a.contains("초안 지시")), "지시 글은 인자로 안 나간다: {argv:?}");
}

#[test]
fn done_turn_comes_back_with_prompt_sent_by_stdin() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    let session = out_session(root.path());
    let outcome = run_one(&env, &session);
    assert!(matches!(outcome, SessionOutcome::Done { .. }));
    assert_eq!(env.sent(0), vec![prompt_line(&session.req.prompt)]);
    assert_eq!(env.spawn_count(), 1);
    // 나머지 읽을 자리·막을 자리는 띄운 claude 에 몫 없다 — 위 launch 시험에서 본다
}

#[test]
fn timeout_kills_the_tree() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    // 한마디만 내놓고 끝나지도 않는다 — 시간 상한에 닿아야 한다
    let mut never_end = ChildScript::new(vec![Step::Line(init_line())]);
    never_end.on_close = None;
    env.push_spawn(never_end);
    let mut session = out_session(root.path());
    session.req.timeout = Duration::from_millis(700);
    let outcome = run_out_session(env.as_ref(), &session, &CancelToken::new());
    assert_eq!(outcome, SessionOutcome::TimedOut);
    assert!(env.child_events(0).iter().any(|e| e == "kill_tree"), "시간이 다 하면 나무째 끝내야 한다: {:?}", env.child_events(0));
}

#[test]
fn cancelled_before_start_never_spawns() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    env.push_spawn(ChildScript::ok_turn(0.1));
    let token = CancelToken::new();
    token.cancel();
    let session = out_session(root.path());
    assert_eq!(run_out_session(env.as_ref(), &session, &token), SessionOutcome::Cancelled);
    assert_eq!(env.spawn_count(), 0);
    assert!(!root.path().join("engine").exists(), "출발 전 엔진 폴더도 만들지 않아야 한다");
}

#[test]
fn cancel_while_running_kills_the_tree() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    // 끝나지 않는 claude — 취소할 때까지 기다린다
    let mut never_end = ChildScript::new(vec![Step::Line(init_line())]);
    never_end.on_close = None;
    env.push_spawn(never_end);
    let token = CancelToken::new();
    let canceller = token.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        canceller.cancel();
    });
    let session = out_session(root.path());
    let outcome = run_out_session(env.as_ref(), &session, &token);
    handle.join().unwrap();
    assert_eq!(outcome, SessionOutcome::Cancelled);
    assert!(env.child_events(0).iter().any(|e| e == "kill_tree"), "취소하면 나무째 끝내야 한다: {:?}", env.child_events(0));
}

#[test]
fn spawn_failure_reports_reason() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    env.push_spawn_error("claude 를 찾지 못했다");
    let session = out_session(root.path());
    let outcome = run_out_session(env.as_ref(), &session, &CancelToken::new());
    match &outcome {
        SessionOutcome::Failed { reason } => {
            assert!(reason.contains("claude 를 띄우지 못했다"), "실패 이유가 안 적혔다: {reason}");
            assert!(reason.contains("claude 를 찾지 못했다"), "가짜 환경의 이유가 안 적혔다: {reason}");
        }
        other => panic!("실패로 끝나야 한다: {other:?}"),
    }
}

#[test]
fn missing_out_folder_fails_before_any_spawn() {
    let root = tempfile::tempdir().unwrap();
    let env = Scenario::new(0).build().env;
    let mut session = out_session(root.path());
    // 결과 폴더가 없는 꼴 — 부르는 쪽이 만들지 못했다고 가정한다
    session.out = root.path().join("out-gone");
    let outcome = run_out_session(env.as_ref(), &session, &CancelToken::new());
    match &outcome {
        SessionOutcome::Failed { reason } => assert!(reason.contains("결과 폴더가 없다"), "이유가 안 적혔다: {reason}"),
        other => panic!("실패로 끝나야 한다: {other:?}"),
    }
    assert_eq!(env.spawn_count(), 0, "결과 폴더가 없으면 띄우지 않는다");
    assert!(env.policies.lock().unwrap().is_empty(), "정책도 만들지 않아야 한다");
    assert!(!root.path().join("engine").exists(), "출발 전 엔진 폴더도 만들지 않아야 한다");
}

#[test]
fn out_args_limit_budget_and_turns() {
    let mut req = out_session(Path::new("C:/tmp-root")).req;
    req.max_budget_usd = 0.0;
    req.max_turns = 0;
    let argv = out_args(&req);
    assert!(argv.windows(2).any(|w| w[0] == "--max-budget-usd" && w[1] == "0.01"), "예산 하한 0.01 — {argv:?}");
    assert!(argv.windows(2).any(|w| w[0] == "--max-turns" && w[1] == "1"), "회전 하한 1 — {argv:?}");
    assert!(argv.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "dontAsk"));
    assert!(argv.windows(2).any(|w| w[0] == "--setting-sources" && w[1] == "project,local"));
    let req2 = out_session(Path::new("C:/tmp-root2")).req;
    let argv2 = out_args(&req2);
    assert!(argv2.windows(2).any(|w| w[0] == "--max-budget-usd" && w[1] == "0.50"));
    assert!(argv2.windows(2).any(|w| w[0] == "--max-turns" && w[1] == "6"));
    // `-p`·`--settings`·`--add-dir` 는 띄우기(claude_args)의 몫 — 여기 두 번 붙지 않는다
    for flag in ["-p", "--settings", "--add-dir", "--session-id"] {
        assert!(!argv2.windows(2).any(|w| w[0] == flag), "{flag} 는 out_args 에 없어야 한다: {argv2:?}");
    }
}
