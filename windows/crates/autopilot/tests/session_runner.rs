//! 읽기 전용 모델 세션 실행기(`engine::session_runner`) 시험. 진짜 claude 도 네트워크도 쓰지 않는다 —
//! 시험마다 임시 폴더에 쓰는 가짜 claude(node 스크립트)가 claude.exe 자리에 서고, 받은 인자·환경·입력을 기록 파일에 남긴다.
//! 프로세스 시험은 이 PC 의 진짜 프로세스로 한다(Windows 전용).

use claude_board_autopilot::engine::session_runner::{
    prompt_line, readonly_args, redact, CancelToken, SessionOutcome, SessionRequest, SessionRunner, DENIED_TOOLS, MAX_LINE_BYTES, READ_TOOLS,
};
use std::path::Path;
use std::time::Duration;

fn request(prompt: &str, cwd: &Path, scratch: &Path) -> SessionRequest {
    SessionRequest {
        prompt: prompt.to_string(),
        cwd: cwd.to_path_buf(),
        model: "opus".into(),
        effort: None,
        max_budget_usd: 0.5,
        max_turns: 6,
        timeout: Duration::from_secs(20),
        scratch_dir: scratch.to_path_buf(),
        env: Vec::new(),
        max_line_bytes: MAX_LINE_BYTES,
    }
}

#[test]
fn readonly_args_list_only_read_tools_and_deny_the_rest() {
    let dir = Path::new("C:/x");
    let args = readonly_args(&request("p", dir, dir));
    let value_of = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone()).unwrap_or_default();
    assert_eq!(READ_TOOLS, &["Read", "Grep", "Glob"]);
    assert_eq!(value_of("--tools"), "Read,Grep,Glob");
    assert_eq!(value_of("--allowedTools"), "Read Grep Glob");
    let denied = value_of("--disallowedTools");
    for tool in ["Bash", "Edit", "Write", "WebFetch", "WebSearch", "NotebookEdit", "MultiEdit", "PowerShell", "Task", "Agent"] {
        assert!(denied.split(' ').any(|d| d == tool), "{tool} 가 거부 목록에 있어야 한다: {denied}");
    }
    assert_eq!(denied.split(' ').count(), DENIED_TOOLS.len());
    assert_eq!(value_of("--permission-mode"), "dontAsk");
    assert_eq!(value_of("--max-budget-usd"), "0.50");
    assert_eq!(value_of("--max-turns"), "6");
    assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    assert!(!args.iter().any(|a| a == "p"), "지시 글이 인자에 없어야 한다");
}

#[test]
fn prompt_line_is_one_json_line_with_newlines_escaped() {
    let line = prompt_line("첫 줄\n둘째 \"줄\"");
    assert!(!line.contains('\n'));
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "user");
    assert_eq!(v["message"]["content"], "첫 줄\n둘째 \"줄\"");
}

#[test]
fn redact_hides_secret_values_and_names() {
    let env = vec![("MY_TOKEN".to_string(), "abcd1234".to_string())];
    let out = redact("fail abcd1234 ANTHROPIC_API_KEY=sk-live-999 sk-zzzz Bearer xyz ok", &env);
    assert!(!out.contains("abcd1234") && !out.contains("sk-live-999") && !out.contains("sk-zzzz"), "{out}");
    assert!(out.contains("ANTHROPIC_API_KEY=***") && out.ends_with("ok"), "{out}");
}

#[cfg(windows)]
mod windows {
    use super::*;
    use claude_board_autopilot::adapters::native_env::NativeEnv;
    use claude_board_autopilot::adapters::winproc::{is_secret_name, strip_verbatim, ENV_ALLOW, ENV_ALLOW_PREFIXES};
    use claude_board_autopilot::ports::{EnvChoice, EnvKind};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    /// 가짜 claude. 모드는 환경 변수 D1A_MODE, 기록은 D1A_LOG(줄마다 JSON), 자식 pid 는 D1A_PIDS
    const FAKE: &str = r#"'use strict';
const fs = require('fs');
const cp = require('child_process');
const mode = process.env.D1A_MODE || 'ok';
const log = process.env.D1A_LOG || '';
function record(o) { if (log) fs.appendFileSync(log, JSON.stringify(o) + '\n'); }
const argv = process.argv.slice(2);
if (argv.includes('--version')) { process.stdout.write('9.9.9\n'); process.exit(0); }
record({ event: 'start', argv, env: process.env, cwd: process.cwd() });
const out = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const result = (extra) => out(Object.assign({ type: 'result', subtype: 'success', is_error: false, num_turns: 3, result: '한글 결과 ✓', total_cost_usd: 0.0421, session_id: 'sid-fake' }, extra));
let acted = false;
function act(line) {
  if (acted) return;
  acted = true;
  record({ event: 'in', line });
  out({ type: 'system', subtype: 'init', session_id: 'sid-fake' });
  if (mode === 'ok') { out({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: 'x' }] } }); result(); process.exit(0); }
  if (mode === 'hang') {
    const g = cp.spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'ignore' });
    fs.writeFileSync(process.env.D1A_PIDS, JSON.stringify({ child: process.pid, grand: g.pid }));
    setInterval(() => {}, 1000);
    return;
  }
  if (mode === 'malformed') {
    process.stdout.write('this is not json\n{"type":"result"\n{"type":"assistant","message":\n[1,2,3]\n\n');
    process.stdout.write(JSON.stringify({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: 'y'.repeat(5 * 1024 * 1024) }] } }) + '\n');
    process.stdout.write('z'.repeat(5 * 1024 * 1024) + '\n');
    result({ num_turns: 2 });
    process.exit(0);
  }
  if (mode === 'exit2') { process.stderr.write('boom ANTHROPIC_API_KEY=sk-abc123456789 D1A_SECRET_TOKEN=hunter22 refused hunter22\n', () => process.exit(2)); return; }
  if (mode === 'silent0') { process.exit(0); }
  if (mode === 'error_result') { result({ subtype: 'error_max_turns', is_error: true, result: '' }); process.exit(1); }
  if (mode === 'subtype_other') { result({ subtype: 'error_during_execution', is_error: false, result: 'half' }); process.exit(0); }
  if (mode === 'hugeline') {
    fs.writeFileSync(process.env.D1A_PIDS, JSON.stringify({ child: process.pid, grand: process.pid }));
    process.stdout.write('q'.repeat(Number(process.env.D1A_LINE_BYTES)) + '\n');
    setInterval(() => process.stdout.write('{"type":"other"}\n'), 200);
    return;
  }
  if (mode === 'api_error') { result({ is_error: true, result: 'x', api_error: 'overloaded' }); process.exit(1); }
}
let pending = '';
process.stdin.setEncoding('utf8');
let first;
process.stdin.on('data', (c) => {
  pending += c;
  const at = pending.indexOf('\n');
  if (first === undefined && at >= 0) { first = pending.slice(0, at); if (mode === 'hang') act(first); }
});
// 진짜 claude 처럼 입력이 닫혀야 일을 시작한다(hang 만 바로)
process.stdin.on('end', () => { if (first === undefined) first = pending; if (first.trim()) act(first); else process.exit(0); });
process.stdin.resume();
setTimeout(() => process.exit(9), 15000).unref();
"#;

    fn node() -> PathBuf {
        let path = std::env::var_os("PATH").expect("PATH");
        std::env::split_paths(&path).map(|d| d.join("node.exe")).find(|p| p.is_file()).expect("node.exe 가 PATH 에 있어야 한다")
    }

    fn choice() -> EnvChoice {
        EnvChoice { kind: EnvKind::Cmd, distro: None }
    }

    fn comspec() -> PathBuf {
        PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec"))
    }

    struct Fixture {
        _root: tempfile::TempDir,
        cwd: PathBuf,
        scratch: PathBuf,
        log: PathBuf,
        pids: PathBuf,
        env: NativeEnv,
    }

    fn fixture_in(cwd_name: &str) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join(cwd_name);
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("keep.txt"), "원본").unwrap();
        std::fs::create_dir_all(cwd.join("sub")).unwrap();
        std::fs::write(cwd.join("sub").join("a.md"), "a").unwrap();
        let script = root.path().join("fake.cjs");
        std::fs::write(&script, FAKE).unwrap();
        let env = NativeEnv::new(choice(), comspec()).unwrap().with_claude_program(node(), vec![script.to_string_lossy().into_owned()]);
        Fixture {
            cwd: strip_verbatim(&cwd),
            scratch: root.path().join("engine-scratch"),
            log: root.path().join("fake.log"),
            pids: root.path().join("pids.json"),
            _root: root,
            env,
        }
    }

    fn fixture() -> Fixture {
        fixture_in("work")
    }

    impl Fixture {
        fn req(&self, mode: &str) -> SessionRequest {
            let mut r = request("지시 글 \"인용\"\n둘째 줄 SECRET-PROMPT-MARK", &self.cwd, &self.scratch);
            r.env = vec![
                ("D1A_MODE".into(), mode.into()),
                ("D1A_LOG".into(), self.log.to_string_lossy().into_owned()),
                ("D1A_PIDS".into(), self.pids.to_string_lossy().into_owned()),
            ];
            r
        }

        fn run(&self, mode: &str) -> SessionOutcome {
            SessionRunner::new(&self.env).run(&self.req(mode), &CancelToken::new())
        }

        fn log(&self) -> Vec<serde_json::Value> {
            std::fs::read_to_string(&self.log).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        }

        fn start(&self) -> serde_json::Value {
            self.log().into_iter().find(|e| e["event"] == "start").expect("가짜 claude 가 떠야 한다")
        }

        fn pids(&self) -> (u32, u32) {
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&self.pids).unwrap()).unwrap();
            (v["child"].as_u64().unwrap() as u32, v["grand"].as_u64().unwrap() as u32)
        }
    }

    fn alive(pid: u32) -> bool {
        const STILL_ACTIVE: u32 = 259;
        // SAFETY: 조회 권한만 열고 바로 닫는다
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE
        }
    }

    fn wait_gone(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if !alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn wait_for_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline && std::fs::read_to_string(path).map(|s| s.is_empty()).unwrap_or(true) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 폴더 아래 모든 파일과 폴더(상대 경로, 크기, 바뀐 시각)를 정렬해 한 목록으로
    fn listing(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let meta = e.metadata().unwrap();
                let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().into_owned();
                // 폴더의 바뀐 시각은 NTFS 가 늦게 갱신하므로 파일만 본다
                let modified = if meta.is_dir() {
                    0
                } else {
                    meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
                };
                out.push(format!("{rel}|{}|{modified}", meta.len()));
                if meta.is_dir() {
                    walk(base, &e.path(), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn normal_run_returns_text_cost_turns() {
        let f = fixture();
        match f.run("ok") {
            SessionOutcome::Done { text, cost, turns, session_id } => {
                assert_eq!(text, "한글 결과 ✓");
                assert!((cost - 0.0421).abs() < 1e-9, "{cost}");
                assert_eq!(turns, 3);
                assert_eq!(session_id, "sid-fake");
            }
            other => panic!("Done 이어야 한다: {other:?}"),
        }
    }

    #[test]
    fn argv_has_readonly_allow_list_deny_list_and_no_write_tool() {
        let f = fixture();
        assert!(matches!(f.run("ok"), SessionOutcome::Done { .. }));
        let argv: Vec<String> = f.start()["argv"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        let value_of = |flag: &str| argv.iter().position(|a| a == flag).map(|i| argv[i + 1].clone()).unwrap_or_default();
        assert_eq!(value_of("--allowedTools"), "Read Grep Glob");
        assert_eq!(value_of("--tools"), "Read,Grep,Glob");
        let denied = value_of("--disallowedTools");
        for tool in DENIED_TOOLS {
            assert!(denied.split(' ').any(|d| d == *tool), "{tool}: {denied}");
        }
        for flag in ["--allowedTools", "--tools"] {
            let allowed = value_of(flag);
            for bad in ["Bash", "Edit", "Write", "WebFetch", "mcp"] {
                assert!(!allowed.contains(bad), "{flag} 에 {bad} 가 있으면 안 된다: {allowed}");
            }
        }
        assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
        assert_eq!(value_of("--permission-mode"), "dontAsk");
        assert_eq!(value_of("--max-budget-usd"), "0.50");
        assert_eq!(value_of("--max-turns"), "6");
        assert!(argv.iter().any(|a| a == "-p") && value_of("--input-format") == "stream-json");
        // 설정 파일에도 같은 규칙이 있다
        let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(value_of("--settings")).unwrap()).unwrap();
        assert_eq!(settings["permissions"]["allow"], serde_json::json!(["Read", "Grep", "Glob"]));
        assert!(settings["permissions"]["deny"].as_array().unwrap().iter().any(|d| d == "Bash"));
    }

    #[test]
    fn prompt_goes_through_stdin_never_argv() {
        let f = fixture();
        assert!(matches!(f.run("ok"), SessionOutcome::Done { .. }));
        let argv = f.start()["argv"].to_string();
        assert!(!argv.contains("SECRET-PROMPT-MARK") && !argv.contains("지시 글"), "{argv}");
        let input = f.log().into_iter().find(|e| e["event"] == "in").expect("표준 입력을 받아야 한다");
        let v: serde_json::Value = serde_json::from_str(input["line"].as_str().unwrap()).unwrap();
        assert_eq!(v["message"]["content"], "지시 글 \"인용\"\n둘째 줄 SECRET-PROMPT-MARK");
    }

    #[test]
    fn very_long_prompt_does_not_hit_the_command_line_limit() {
        let f = fixture();
        let mut req = f.req("ok");
        req.prompt = "가".repeat(200_000);
        assert!(matches!(SessionRunner::new(&f.env).run(&req, &CancelToken::new()), SessionOutcome::Done { .. }));
    }

    #[test]
    fn timeout_kills_the_whole_process_tree() {
        let f = fixture();
        let mut req = f.req("hang");
        req.timeout = Duration::from_secs(3);
        let started = Instant::now();
        let outcome = SessionRunner::new(&f.env).run(&req, &CancelToken::new());
        assert_eq!(outcome, SessionOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(12), "시간 상한 뒤 곧 돌아와야 한다: {:?}", started.elapsed());
        let (child, grand) = f.pids();
        assert!(wait_gone(child), "가짜 claude 가 남았다");
        assert!(wait_gone(grand), "손자 프로세스가 남았다");
    }

    #[test]
    fn cancel_while_running_returns_cancelled_and_kills_the_tree() {
        let f = fixture();
        let token = CancelToken::new();
        let pids = f.pids.clone();
        let canceller = token.clone();
        let handle = std::thread::spawn(move || {
            wait_for_file(&pids);
            canceller.cancel();
        });
        let outcome = SessionRunner::new(&f.env).run(&f.req("hang"), &token);
        handle.join().unwrap();
        assert_eq!(outcome, SessionOutcome::Cancelled);
        let (child, grand) = f.pids();
        assert!(wait_gone(child) && wait_gone(grand), "취소하면 자손까지 끝나야 한다");
    }

    #[test]
    fn cancelled_before_start_never_spawns() {
        let f = fixture();
        let token = CancelToken::new();
        token.cancel();
        assert_eq!(SessionRunner::new(&f.env).run(&f.req("ok"), &token), SessionOutcome::Cancelled);
        assert!(f.log().is_empty(), "띄우지 않아야 한다");
        assert!(!f.scratch.exists(), "엔진 폴더도 만들지 않아야 한다");
    }

    #[test]
    fn malformed_partial_and_five_megabyte_lines_do_not_panic() {
        let f = fixture();
        match f.run("malformed") {
            SessionOutcome::Done { text, turns, .. } => {
                assert_eq!(text, "한글 결과 ✓");
                assert_eq!(turns, 2);
            }
            other => panic!("깨진 줄 뒤의 결과를 읽어야 한다: {other:?}"),
        }
    }

    #[test]
    fn nonzero_exit_is_failed_with_the_reason_and_no_secrets() {
        let f = fixture();
        let mut req = f.req("exit2");
        req.env.push(("D1A_SECRET_TOKEN".into(), "hunter22".into()));
        match SessionRunner::new(&f.env).run(&req, &CancelToken::new()) {
            SessionOutcome::Failed { reason } => {
                assert!(reason.contains("종료 코드 2") && reason.contains("boom") && reason.contains("refused"), "{reason}");
                for secret in ["sk-abc123456789", "hunter22"] {
                    assert!(!reason.contains(secret), "{reason}");
                }
            }
            other => panic!("Failed 여야 한다: {other:?}"),
        }
    }

    #[test]
    fn non_success_subtype_without_error_flag_is_not_done() {
        let f = fixture();
        match f.run("subtype_other") {
            SessionOutcome::Failed { reason } => assert!(reason.contains("error_during_execution"), "{reason}"),
            other => panic!("Done 이 아니어야 한다: {other:?}"),
        }
    }

    #[test]
    fn line_over_the_cap_ends_the_session_and_stops_the_reader() {
        let f = fixture();
        let mut req = f.req("hugeline");
        req.max_line_bytes = 1024 * 1024;
        req.env.push(("D1A_LINE_BYTES".into(), (req.max_line_bytes + 1024).to_string()));
        let started = Instant::now();
        match SessionRunner::new(&f.env).run(&req, &CancelToken::new()) {
            SessionOutcome::Failed { reason } => assert!(reason.contains("상한"), "{reason}"),
            other => panic!("Failed 여야 한다: {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(15), "상한을 넘으면 곧 끝나야 한다(시간 상한 20초 전에): {:?}", started.elapsed());
        let (child, _) = f.pids();
        assert!(wait_gone(child), "읽기를 멈추고 가짜 claude 를 끝내야 한다");
    }

    #[test]
    fn line_just_under_the_cap_is_still_read() {
        let f = fixture();
        let mut req = f.req("hugeline");
        req.max_line_bytes = 1024 * 1024;
        req.env.push(("D1A_LINE_BYTES".into(), (req.max_line_bytes - 1024).to_string()));
        req.timeout = Duration::from_secs(3);
        // 상한 안의 줄은 읽고 넘긴다 — 결과가 없으니 시간 상한으로 끝난다
        assert_eq!(SessionRunner::new(&f.env).run(&req, &CancelToken::new()), SessionOutcome::TimedOut);
    }

    #[test]
    fn every_emitted_flag_is_known_to_the_real_cli() {
        // 진짜 claude 가 아는 옵션(claude --help, --max-turns 는 번들 문서). 오타·없는 옵션이 나가면 여기서 깨진다
        const KNOWN: &[&str] = &[
            "-p",
            "--input-format",
            "--output-format",
            "--verbose",
            "--session-id",
            "--model",
            "--effort",
            "--settings",
            "--tools",
            "--allowedTools",
            "--disallowedTools",
            "--permission-mode",
            "--permission-prompts",
            "--strict-mcp-config",
            "--max-budget-usd",
            "--max-turns",
        ];
        let f = fixture();
        let mut req = f.req("ok");
        req.effort = Some("high".into());
        assert!(matches!(SessionRunner::new(&f.env).run(&req, &CancelToken::new()), SessionOutcome::Done { .. }));
        let argv: Vec<String> = f.start()["argv"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        let argv = &argv[1..]; // 앞 인자는 가짜 스크립트 경로
        let flags: Vec<&String> = argv.iter().filter(|a| a.starts_with('-') && a.len() > 1 && !a.contains(' ')).collect();
        assert!(flags.len() >= 14, "{flags:?}");
        for flag in &flags {
            assert!(KNOWN.contains(&flag.as_str()), "진짜 claude 가 모를 수 있는 옵션: {flag}");
        }
        for flag in readonly_args(&req).iter().filter(|a| a.starts_with("--")) {
            assert!(KNOWN.contains(&flag.as_str()), "{flag}");
        }
    }

    #[test]
    fn error_result_and_silent_exit_are_failed() {
        let f = fixture();
        match f.run("error_result") {
            SessionOutcome::Failed { reason } => assert!(reason.contains("error_max_turns"), "{reason}"),
            other => panic!("{other:?}"),
        }
        match f.run("api_error") {
            SessionOutcome::Failed { reason } => assert!(reason.contains("overloaded"), "{reason}"),
            other => panic!("{other:?}"),
        }
        match f.run("silent0") {
            SessionOutcome::Failed { reason } => assert!(reason.contains("결과 없이"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_claude_is_failed_with_a_clear_reason() {
        let root = tempfile::tempdir().unwrap();
        let env = NativeEnv::new(choice(), comspec()).unwrap().with_claude_search(OsString::new(), root.path().to_path_buf());
        let req = request("p", root.path(), &root.path().join("scratch"));
        match SessionRunner::new(&env).run(&req, &CancelToken::new()) {
            SessionOutcome::Failed { reason } => assert!(reason.contains("claude.exe"), "{reason}"),
            other => panic!("Failed 여야 한다: {other:?}"),
        }
    }

    #[test]
    fn target_folder_listing_is_identical_before_and_after() {
        let f = fixture();
        let before = listing(&f.cwd);
        for mode in ["ok", "malformed", "exit2", "silent0", "error_result"] {
            let _ = f.run(mode);
        }
        assert_eq!(listing(&f.cwd), before, "작업 폴더에 아무것도 쓰지 않아야 한다");
        assert!(f.scratch.join("settings.json").is_file(), "설정은 엔진 폴더에 쓴다");
    }

    #[test]
    fn korean_and_space_working_folder() {
        let f = fixture_in("한글 폴더 with space");
        assert!(matches!(f.run("ok"), SessionOutcome::Done { .. }));
        let cwd = f.start()["cwd"].as_str().unwrap().to_string();
        assert!(cwd.ends_with("한글 폴더 with space"), "{cwd}");
    }

    #[test]
    fn env_passed_to_claude_has_no_stripped_variable() {
        std::env::set_var("D1A_PROBE_TOKEN", "leak-me");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-should-not-pass");
        let f = fixture();
        assert!(matches!(f.run("ok"), SessionOutcome::Done { .. }));
        let env = f.start()["env"].as_object().unwrap().clone();
        let extras = ["D1A_MODE", "D1A_LOG", "D1A_PIDS"];
        assert!(env.keys().all(|k| !k.eq_ignore_ascii_case("D1A_PROBE_TOKEN") && !k.eq_ignore_ascii_case("ANTHROPIC_API_KEY")), "{:?}", env.keys());
        for key in env.keys().filter(|k| !k.starts_with('=')) {
            let allowed = ENV_ALLOW.iter().any(|a| a.eq_ignore_ascii_case(key))
                || ENV_ALLOW_PREFIXES.iter().any(|p| key.len() >= p.len() && key[..p.len()].eq_ignore_ascii_case(p))
                || extras.contains(&key.as_str());
            assert!(allowed, "허용 목록 밖 변수가 넘어갔다: {key}");
            assert!(!is_secret_name(key), "비밀 이름이 넘어갔다: {key}");
        }
        assert!(env.keys().any(|k| k.eq_ignore_ascii_case("PATH")), "PATH 는 물려받는다");
    }
}
