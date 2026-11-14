// 네이티브 실행 환경 어댑터 시험. 위쪽은 어디서나 도는 순수 함수(권한 규칙·인자·claude 찾기),
// 아래 `windows` 모듈은 진짜 프로세스를 띄운다: node + 가짜 claude(`windows/ci/fake-claude-stream.cjs`)가 claude.exe 자리에 선다.
// 판정 명령은 이 PC 에 있는 네이티브 셸(cmd·powershell·pwsh·git-bash) 각각으로 돌린다.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use claude_board_autopilot::adapters::native_env::*;
use claude_board_autopilot::ports::*;
use serde_json::json;

fn choice(kind: EnvKind) -> EnvChoice {
    EnvChoice { kind, distro: None }
}

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn permission_path_table() {
    let ok = [
        (r"C:\work\toy", "//c/work/toy"),
        (r"c:/work/toy/", "//c/work/toy"),
        (r"D:\a b\한글", "//d/a b/한글"),
        (r"\\?\C:\work", "//c/work"),
        (r"C:\", "//c"),
        (r"C:\work\*.log", "//c/work/*.log"),
    ];
    for (input, want) in ok {
        assert_eq!(permission_path(Path::new(input)).as_deref(), Ok(want), "{input}");
    }
    for bad in [r"work\toy", r"\\server\share\x", r"\rooted", "", "C:rel"] {
        let err = permission_path(Path::new(bad)).expect_err(bad);
        assert_eq!(err.kind, PortErrorKind::Invalid, "{bad}");
    }
}

fn policy() -> SessionPolicy {
    SessionPolicy {
        allow_write: vec![p(r"C:\work\toy"), p(r"C:\work\toy")],
        deny_write: vec![p(r"C:\work\toy\CLAUDE.md"), p(r"C:\work\toy\.git\hooks"), p(r"C:\work\toy\pinned\*")],
        deny_read: vec![p(r"C:\Users\a\AppData\Local\cb\private")],
        network_allow: vec!["example.com".into()],
        local_binding: true,
        allowed_tools: vec!["Read".into(), "Bash(npm test:*)".into()],
        disallowed_tools: vec!["Bash(git push *)".into(), "Read".into()],
    }
}

#[test]
fn render_settings_has_no_sandbox_and_path_rules() {
    for kind in [EnvKind::Cmd, EnvKind::PowerShell, EnvKind::Pwsh, EnvKind::GitBash] {
        let env = NativeEnv::new(choice(kind), p(r"C:\x\shell.exe")).unwrap();
        let settings = env.render_settings(&policy()).unwrap();
        assert_eq!(settings["sandbox"], json!({ "enabled": false }), "{kind:?}");
        assert!(!settings.to_string().contains("failIfUnavailable"));
        assert_eq!(
            settings["permissions"]["allow"],
            json!(["Read", "Bash(npm test:*)", "Edit(//c/work/toy/**)"]),
            "허용: 도구 규칙이 먼저, 쓰기 범위가 뒤, 같은 규칙은 한 번"
        );
        assert_eq!(
            settings["permissions"]["deny"],
            json!([
                "Bash(git push *)",
                "Read",
                "Edit(//c/work/toy/CLAUDE.md)",
                "Edit(//c/work/toy/CLAUDE.md/**)",
                "Edit(//c/work/toy/.git/hooks)",
                "Edit(//c/work/toy/.git/hooks/**)",
                "Edit(//c/work/toy/pinned/*)",
                "Read(//c/Users/a/AppData/Local/cb/private)",
                "Read(//c/Users/a/AppData/Local/cb/private/**)",
            ])
        );
        let keys: Vec<&String> = settings.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["sandbox", "permissions"]);
    }
}

#[test]
fn render_settings_rejects_relative_paths() {
    let env = NativeEnv::new(choice(EnvKind::Cmd), p(r"C:\x\cmd.exe")).unwrap();
    let bad = SessionPolicy { allow_write: vec![p(r"work\toy")], ..SessionPolicy::default() };
    assert_eq!(env.render_settings(&bad).unwrap_err().kind, PortErrorKind::Invalid);
    let empty = env.render_settings(&SessionPolicy::default()).unwrap();
    assert_eq!(empty["permissions"], json!({ "allow": [], "deny": [] }));
}

#[test]
fn wsl_is_not_a_native_env() {
    let err = NativeEnv::new(EnvChoice { kind: EnvKind::Wsl, distro: Some("Ubuntu".into()) }, p("wsl.exe")).err().unwrap();
    assert_eq!(err.kind, PortErrorKind::Invalid);
}

#[test]
fn env_paths_are_identity_without_verbatim_prefix() {
    let env = NativeEnv::new(choice(EnvKind::Cmd), p(r"C:\x\cmd.exe")).unwrap();
    assert_eq!(env.to_env_path(Path::new(r"\\?\C:\work\a")).unwrap(), r"C:\work\a");
    assert_eq!(env.to_env_path(Path::new(r"C:\work\한글")).unwrap(), r"C:\work\한글");
    assert_eq!(env.env_path_to_windows(r"C:\work\a").unwrap(), p(r"C:\work\a"));
    assert_eq!(env.choice(), &choice(EnvKind::Cmd));
}

fn launch(dir: &Path) -> ClaudeLaunch {
    ClaudeLaunch {
        session: SessionStart::New("sid-1".into()),
        cwd: dir.to_path_buf(),
        model: "opus".into(),
        effort: Some("high".into()),
        settings_file: dir.join("settings.json"),
        system_prompt_file: Some(dir.join("prompt.md")),
        add_dirs: vec![dir.join("a"), dir.join("b c")],
        plugin_dir: Some(dir.join("plug")),
        env: Vec::new(),
        path_env: Vec::new(),
        extra_args: vec!["--permission-mode".into(), "dontAsk".into()],
        stderr_log: dir.join("logs").join("stderr.log"),
    }
}

#[test]
fn claude_args_in_order() {
    let dir = p(r"C:\run");
    let mut l = launch(&dir);
    assert_eq!(
        claude_args(&l),
        [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--session-id",
            "sid-1",
            "--model",
            "opus",
            "--effort",
            "high",
            "--settings",
            r"C:\run\settings.json",
            "--append-system-prompt-file",
            r"C:\run\prompt.md",
            "--add-dir",
            r"C:\run\a",
            "--add-dir",
            r"C:\run\b c",
            "--plugin-dir",
            r"C:\run\plug",
            "--permission-mode",
            "dontAsk",
        ]
    );
    l.session = SessionStart::Resume("sid-2".into());
    l.model.clear();
    l.effort = None;
    l.system_prompt_file = None;
    l.add_dirs.clear();
    l.plugin_dir = None;
    l.extra_args.clear();
    assert_eq!(
        claude_args(&l)[6..],
        ["--resume", "sid-2", "--settings", r"C:\run\settings.json"]
    );
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

#[test]
fn locate_claude_prefers_path_then_home_and_rejects_shims() {
    let root = tempfile::tempdir().unwrap();
    let (a, b, home) = (root.path().join("a"), root.path().join("b"), root.path().join("home"));
    let home_exe = home.join(".local").join("bin").join("claude.exe");

    // 아무 것도 없다
    std::fs::create_dir_all(&a).unwrap();
    let why = locate_claude(std::slice::from_ref(&a), Some(&home)).unwrap_err();
    assert!(why.contains("claude.exe 를 찾지 못했습니다"), "{why}");

    // 홈의 .local\bin
    touch(&home_exe);
    assert_eq!(locate_claude(std::slice::from_ref(&a), Some(&home)).unwrap(), home_exe);

    // PATH 가 홈보다 먼저
    touch(&b.join("claude.exe"));
    assert_eq!(locate_claude(&[a.clone(), b.clone()], Some(&home)).unwrap(), b.join("claude.exe"));

    // .cmd 만 있으면(홈에도 exe 가 없을 때) 쉬운 말로 거절
    std::fs::remove_file(&home_exe).unwrap();
    std::fs::remove_file(b.join("claude.exe")).unwrap();
    touch(&a.join("claude.cmd"));
    let why = locate_claude(std::slice::from_ref(&a), Some(&home)).unwrap_err();
    assert!(why.contains("claude.cmd") && why.contains("claude.exe"), "{why}");

    // 같은 PATH 의 .cmd 가 앞서도 뒤 폴더의 exe 는 쓴다
    touch(&b.join("claude.exe"));
    assert_eq!(locate_claude(&[a, b.clone()], None).unwrap(), b.join("claude.exe"));
}

#[test]
fn check_ready_reports_the_cmd_shim_problem() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    touch(&bin.join("claude.cmd"));
    let shell = root.path().join("cmd.exe");
    touch(&shell);
    let env = NativeEnv::new(choice(EnvKind::Cmd), shell)
        .unwrap()
        .with_claude_search(OsString::from(bin.as_os_str()), root.path().join("no-home"));
    let report = env.check_ready(root.path());
    assert!(!report.ready && !report.sandbox && report.claude_version.is_none());
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("claude.cmd") && report.problems[0].contains("claude.exe"), "{:?}", report.problems);
}

#[test]
fn check_ready_lists_every_plain_problem() {
    let root = tempfile::tempdir().unwrap();
    let env = NativeEnv::new(choice(EnvKind::GitBash), root.path().join("no-bash.exe"))
        .unwrap()
        .with_claude_search(OsString::new(), root.path().join("no-home"));
    let report = env.check_ready(&root.path().join("no-cwd"));
    assert!(!report.ready);
    assert_eq!(report.problems.len(), 3, "{:?}", report.problems);
    assert!(report.problems[0].contains("작업 폴더가 없습니다"));
    assert!(report.problems[1].contains("Git Bash"));
    assert!(report.problems[2].contains("claude.exe 를 찾지 못했습니다"));
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::time::{Duration, Instant};

    use claude_board_core::envs::{detect, EnvEntry};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Globalization::GetOEMCP;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    const STILL_ACTIVE: u32 = 259;

    fn node() -> PathBuf {
        let path = std::env::var_os("PATH").expect("PATH");
        std::env::split_paths(&path).map(|d| d.join("node.exe")).find(|p| p.is_file()).expect("node.exe 가 PATH 에 있어야 한다")
    }

    fn fake_claude() -> PathBuf {
        let full = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("ci").join("fake-claude-stream.cjs").canonicalize().expect("가짜 claude");
        // canonicalize 가 붙인 \?\ 는 node 의 모듈 경로로 쓰지 않는다
        claude_board_autopilot::adapters::winproc::strip_verbatim(&full)
    }

    fn comspec() -> PathBuf {
        PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec"))
    }

    fn fake_env() -> NativeEnv {
        NativeEnv::new(choice(EnvKind::Cmd), comspec()).unwrap().with_claude_program(node(), vec![fake_claude().to_string_lossy().into_owned()])
    }

    fn alive(pid: u32) -> bool {
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

    fn wait_exit(child: &mut dyn ChildProcess) -> Option<i32> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(code) = child.try_wait().unwrap() {
                return Some(code);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// stdout 을 읽어 `type` 이 `until` 인 줄까지 JSON 으로 모은다
    fn read_until(child: &mut dyn ChildProcess, until: &str) -> Vec<serde_json::Value> {
        let mut seen = Vec::new();
        for _ in 0..40 {
            let line = child.read_line(Duration::from_secs(10)).expect("줄이 제때 와야 한다").expect("EOF 가 아니어야 한다");
            let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("JSON 이 아니다 {line:?}: {e}"));
            let done = v["type"] == until;
            seen.push(v);
            if done {
                return seen;
            }
        }
        panic!("{until} 줄이 오지 않았다: {seen:?}");
    }

    const USER_HELLO: &str = r#"{"type":"user","message":{"role":"user","content":"hello 한글"}}"#;

    fn fake_launch(dir: &Path, session: SessionStart, script: Option<&Path>) -> ClaudeLaunch {
        let mut l = launch(dir);
        l.session = session;
        l.settings_file = dir.join("settings.json");
        std::fs::write(&l.settings_file, "{}").unwrap();
        std::fs::write(dir.join("prompt.md"), "시스템 글").unwrap();
        l.add_dirs = vec![dir.join("extra")];
        l.plugin_dir = None;
        l.extra_args = vec!["--ignored-extra".into()];
        l.env = vec![("FAKE_CLAUDE_LOG".into(), dir.join("fake.log").to_string_lossy().into_owned())];
        if let Some(script) = script {
            l.env.push(("FAKE_CLAUDE_SCRIPT".into(), script.to_string_lossy().into_owned()));
        }
        l
    }

    fn fake_log(dir: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(dir.join("fake.log")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn fake_claude_new_then_resume_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let env = fake_env();
        let sid = "11111111-2222-3333-4444-555555555555";

        let mut child = env.spawn_claude(&fake_launch(dir.path(), SessionStart::New(sid.into()), None)).unwrap();
        assert!(child.pid() > 0);
        assert!(dir.path().join("logs").is_dir(), "stderr 로그 폴더는 어댑터가 만든다");
        child.write_line(USER_HELLO).unwrap();
        let events = read_until(&mut *child, "result");
        assert_eq!(events[0]["type"], "system");
        assert_eq!(events[0]["session_id"], sid);
        let said = events.iter().find(|e| e["type"] == "assistant").expect("assistant 줄");
        assert!(said["message"]["content"][0]["text"].as_str().unwrap().contains("hello 한글"), "한글이 그대로 전달된다");
        assert_eq!(events.last().unwrap()["subtype"], "success");
        child.close_stdin();
        assert_eq!(wait_exit(&mut *child), Some(0));
        assert_eq!(child.read_line(Duration::from_secs(5)).unwrap(), None, "끝나면 EOF");

        let mut child = env.spawn_claude(&fake_launch(dir.path(), SessionStart::Resume(sid.into()), None)).unwrap();
        child.write_line(USER_HELLO).unwrap();
        let events = read_until(&mut *child, "result");
        assert_eq!(events[0]["session_id"], sid, "이어 열면 같은 세션");
        child.close_stdin();
        assert_eq!(wait_exit(&mut *child), Some(0));

        let starts: Vec<_> = fake_log(dir.path()).into_iter().filter(|e| e["event"] == "start").collect();
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0]["resume"], false);
        assert_eq!(starts[1]["resume"], true);
        assert_eq!(starts[0]["model"], "opus");
        assert_eq!(starts[0]["systemPrompt"], "시스템 글", "시스템 글은 파일 경로로 넘어간다");
        assert_eq!(starts[0]["settings"], "{}");
        let argv: Vec<&str> = starts[0]["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert_eq!(&argv[..8], ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--session-id", sid]);
        assert_eq!(argv.last(), Some(&"--ignored-extra"));
        // 작업 폴더는 launch.cwd
        let cwd = PathBuf::from(starts[0]["cwd"].as_str().unwrap());
        assert_eq!(cwd.canonicalize().unwrap(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn interrupt_makes_the_fake_claude_exit_130() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("script.json");
        std::fs::write(&script, r#"[{"delay_ms": 20000}]"#).unwrap();
        let mut child = fake_env().spawn_claude(&fake_launch(dir.path(), SessionStart::New("s".into()), Some(&script))).unwrap();
        child.write_line(USER_HELLO).unwrap();
        let init = read_until(&mut *child, "assistant");
        assert_eq!(init[0]["subtype"], "init");
        let started = Instant::now();
        child.interrupt().unwrap();
        let events = read_until(&mut *child, "result");
        assert!(events.iter().any(|e| e["type"] == "control_response"), "interrupt 요청에 답이 온다: {events:?}");
        assert_eq!(events.last().unwrap()["subtype"], "error_during_execution");
        assert_eq!(wait_exit(&mut *child), Some(130));
        assert!(started.elapsed() < Duration::from_secs(10), "20초 지연을 기다리지 않고 끊긴다");
        let sent: Vec<_> = fake_log(dir.path()).into_iter().filter(|e| e["event"] == "in" && e["line"].as_str().unwrap().contains("interrupt")).collect();
        assert_eq!(sent.len(), 1);
        let line: serde_json::Value = serde_json::from_str(sent[0]["line"].as_str().unwrap()).unwrap();
        assert_eq!(line["type"], "control_request");
        assert_eq!(line["request"]["subtype"], "interrupt");
        assert!(!line["request_id"].as_str().unwrap().is_empty());
    }

    #[test]
    fn kill_tree_ends_the_fake_claude() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = fake_env().spawn_claude(&fake_launch(dir.path(), SessionStart::New("s".into()), None)).unwrap();
        let pid = child.pid();
        assert_eq!(child.try_wait().unwrap(), None);
        child.kill_tree().unwrap();
        assert!(wait_gone(pid));
        assert!(wait_exit(&mut *child).is_some());
    }

    #[test]
    fn spawn_claude_without_claude_is_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let env = NativeEnv::new(choice(EnvKind::Cmd), comspec()).unwrap().with_claude_search(OsString::new(), root.path().join("no-home"));
        let err = env.spawn_claude(&fake_launch(root.path(), SessionStart::New("s".into()), None)).err().expect("없는 claude");
        assert_eq!(err.kind, PortErrorKind::Unavailable);
    }

    #[test]
    fn check_ready_reads_the_version_line() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("v.cjs");
        std::fs::write(&script, "console.log('9.9.9 (Claude Code)');").unwrap();
        let env = NativeEnv::new(choice(EnvKind::Cmd), comspec()).unwrap().with_claude_program(node(), vec![script.to_string_lossy().into_owned()]);
        let report = env.check_ready(dir.path());
        assert_eq!(report, EnvReport { ready: true, sandbox: false, claude_version: Some("9.9.9 (Claude Code)".into()), problems: vec![] });
    }

    #[test]
    fn check_ready_reports_a_failing_or_silent_version() {
        let dir = tempfile::tempdir().unwrap();
        let run = |code: &str| {
            let script = dir.path().join("v.cjs");
            std::fs::write(&script, code).unwrap();
            NativeEnv::new(choice(EnvKind::Cmd), comspec()).unwrap().with_claude_program(node(), vec![script.to_string_lossy().into_owned()]).check_ready(dir.path())
        };
        let failing = run("process.exit(3);");
        assert!(!failing.ready && failing.problems[0].contains("종료 코드 3"), "{:?}", failing.problems);
        let silent = run("");
        assert!(!silent.ready && silent.problems[0].contains("아무것도"), "{:?}", silent.problems);
    }

    // ── 판정 명령 ────────────────────────────────────

    /// 판정 명령 글자 셋(정상·실패·손자를 남기는 오래 걸리는 것)
    struct Commands {
        ok: String,
        fail: String,
        sleep: String,
    }

    const PID_SCRIPT: &str = "require('fs').writeFileSync(process.argv[2], String(process.pid)); setTimeout(() => {}, 30000);";

    fn posix(path: &Path) -> String {
        let text = path.to_string_lossy().replace('\\', "/");
        format!("/{}{}", text[..1].to_lowercase(), &text[2..])
    }

    fn commands(kind: EnvKind, script: &Path, pid_file: &Path) -> Commands {
        let node = node();
        match kind {
            EnvKind::Cmd => Commands {
                ok: "echo 한글".into(),
                fail: "echo before & exit /b 3".into(),
                sleep: format!("\"{}\" \"{}\" \"{}\"", node.display(), script.display(), pid_file.display()),
            },
            EnvKind::PowerShell | EnvKind::Pwsh => Commands {
                ok: "Write-Output '한글'".into(),
                fail: "Write-Output 'before'; exit 3".into(),
                sleep: format!("& '{}' '{}' '{}'", node.display(), script.display(), pid_file.display()),
            },
            EnvKind::GitBash => Commands {
                ok: "echo 한글".into(),
                fail: "echo before; exit 3".into(),
                sleep: format!("'{}' '{}' '{}'", posix(&node), posix(script), posix(pid_file)),
            },
            EnvKind::Wsl => unreachable!("WSL 은 이 어댑터가 아니다"),
        }
    }

    fn native_entries() -> Vec<EnvEntry> {
        detect().into_iter().filter(|e| e.choice.kind != EnvKind::Wsl).collect()
    }

    fn check_run(dir: &Path, name: &str, command: &str, timeout: Duration) -> CheckRun {
        CheckRun {
            command: command.to_string(),
            cwd: dir.to_path_buf(),
            env: vec![("CLAUDE_BOARD_AUTOPILOT".into(), "r1".into())],
            path_env: Vec::new(),
            timeout,
            output_log: dir.join("evidence").join(name).join("output.log"),
        }
    }

    #[test]
    fn run_check_for_every_local_native_shell() {
        let entries = native_entries();
        assert!(entries.iter().any(|e| e.choice.kind == EnvKind::Cmd), "cmd 는 어느 Windows 에나 있다");
        let oem = unsafe { GetOEMCP() };
        println!("OEM 코드 페이지 {oem}, 이 PC 의 환경: {:?}", entries.iter().map(|e| e.choice.key()).collect::<Vec<_>>());

        for entry in entries {
            let kind = entry.choice.kind;
            let tag = format!("{kind:?}");
            let dir = tempfile::tempdir().unwrap();
            let script = dir.path().join("pid.cjs");
            std::fs::write(&script, PID_SCRIPT).unwrap();
            let pid_file = dir.path().join("pid.txt");
            let cmds = commands(kind, &script, &pid_file);
            let env = NativeEnv::new(entry.choice.clone(), entry.program.clone()).unwrap();
            let long = Duration::from_secs(60);

            // 정상: 종료 코드 0, 한글 출력이 로그와 tail 에 같이 남는다
            let ok = env.run_check(&check_run(dir.path(), "ok", &cmds.ok, long)).unwrap();
            assert_eq!((ok.exit, ok.timed_out), (Some(0), false), "{tag} {ok:?}");
            // cmd 는 OEM 코드 페이지로 내므로 한글을 담을 수 있는 페이지(949)에서만 글자까지 견준다
            let korean_expected = kind != EnvKind::Cmd || oem == 949;
            if korean_expected {
                assert!(ok.tail.contains("한글"), "{tag} tail={:?}", ok.tail);
            }
            assert!(dir.path().join("evidence").join("ok").join("output.log").is_file(), "{tag} 출력 로그 파일");

            // 실패: 종료 코드가 그대로 온다, 실패 전의 출력도 남는다
            let fail = env.run_check(&check_run(dir.path(), "fail", &cmds.fail, long)).unwrap();
            assert_eq!((fail.exit, fail.timed_out), (Some(3), false), "{tag} {fail:?}");
            assert!(fail.tail.contains("before"), "{tag} tail={:?}", fail.tail);

            // 시간 초과: 자손(node)까지 끝내고 timed_out
            let limit = if kind == EnvKind::Cmd { Duration::from_secs(1) } else { Duration::from_secs(5) };
            let started = Instant::now();
            let slow = env.run_check(&check_run(dir.path(), "slow", &cmds.sleep, limit)).unwrap();
            assert_eq!((slow.exit, slow.timed_out), (None, true), "{tag} {slow:?}");
            assert!(started.elapsed() < limit + Duration::from_secs(10), "{tag} 끊는 데 너무 오래 걸렸다");
            let grandchild: u32 = std::fs::read_to_string(&pid_file).unwrap_or_else(|e| panic!("{tag}: node 가 떴어야 한다: {e}")).trim().parse().unwrap();
            assert!(wait_gone(grandchild), "{tag}: 시간 초과 뒤 손자 node 가 남았다");
        }
    }

    #[test]
    fn run_check_passes_caller_env_to_every_shell() {
        for entry in native_entries() {
            let kind = entry.choice.kind;
            let command = match kind {
                EnvKind::Cmd => "echo %CLAUDE_BOARD_AUTOPILOT%",
                EnvKind::PowerShell | EnvKind::Pwsh => "Write-Output $env:CLAUDE_BOARD_AUTOPILOT",
                _ => "echo $CLAUDE_BOARD_AUTOPILOT",
            };
            let dir = tempfile::tempdir().unwrap();
            let env = NativeEnv::new(entry.choice.clone(), entry.program.clone()).unwrap();
            let out = env.run_check(&check_run(dir.path(), "env", command, Duration::from_secs(60))).unwrap();
            assert_eq!(out.exit, Some(0), "{kind:?} {out:?}");
            assert!(out.tail.contains("r1"), "{kind:?} tail={:?}", out.tail);
        }
    }

    /// Windows PowerShell 5.1 은 안 불러온 모듈의 cmdlet 을 처음 쓸 때 모듈 경로를 앞에서부터 훑어 그 cmdlet 을 내는 모듈을 자동으로 불러온다.
    /// 모듈이 많고 분석 캐시가 없는 PC(GitHub 러너)에서는 이 훑기가 `Write-Output hello` 한 줄에 18~60초라 판정이 시간 초과로 끝났다.
    /// 감싸개가 Utility·Management 를 이름으로 먼저 불러 두면 훑지 않는다 — 여기서는 모듈 경로 맨 앞에 `Test-Path` 를 내는 가짜 모듈을 두고,
    /// 자동 불러오기가 일어났다면 `Test-Path` 가 가짜로 풀려 `DECOY` 가 찍히는 것으로 훑기가 일어났는지 시간과 무관하게 가른다
    #[test]
    fn powershell_judge_does_not_autoload_core_cmdlets_from_the_module_path() {
        let Some(entry) = native_entries().into_iter().find(|e| e.choice.kind == EnvKind::PowerShell) else {
            panic!("Windows PowerShell 5.1 은 어느 Windows 에나 있다");
        };
        let dir = tempfile::tempdir().unwrap();
        let decoy = dir.path().join("modules").join("Decoy");
        std::fs::create_dir_all(&decoy).unwrap();
        std::fs::write(decoy.join("Decoy.psm1"), "function Test-Path { 'DECOY' }\n").unwrap();
        std::fs::write(
            decoy.join("Decoy.psd1"),
            "@{ RootModule='Decoy.psm1'; ModuleVersion='1.0'; GUID='6f6e3a0c-0b1e-4c3b-9a3e-1d2f3a4b5c6d'; FunctionsToExport=@('Test-Path'); CmdletsToExport=@(); AliasesToExport=@() }\n",
        )
        .unwrap();
        let system_modules = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join(r"System32\WindowsPowerShell\v1.0\Modules");
        let module_path = format!("{};{}", dir.path().join("modules").display(), system_modules.display());

        let env = NativeEnv::new(entry.choice.clone(), entry.program.clone()).unwrap();
        let mut run = check_run(dir.path(), "autoload", r"Test-Path C:\", Duration::from_secs(60));
        run.env.push(("PSModulePath".into(), module_path));
        let out = env.run_check(&run).unwrap();
        assert_eq!(out.exit, Some(0), "{out:?}");
        assert!(out.tail.contains("True") && !out.tail.contains("DECOY"), "자동 불러오기가 모듈 경로를 훑었다: tail={:?}", out.tail);
    }

    #[test]
    fn run_check_runs_in_the_given_cwd_and_rejects_empty_commands() {
        let dir = tempfile::tempdir().unwrap();
        let env = NativeEnv::new(choice(EnvKind::Cmd), comspec()).unwrap();
        let out = env.run_check(&check_run(dir.path(), "cwd", "cd", Duration::from_secs(60))).unwrap();
        let printed = PathBuf::from(out.tail.trim());
        assert_eq!(printed.canonicalize().unwrap(), dir.path().canonicalize().unwrap());
        let err = env.run_check(&check_run(dir.path(), "empty", "  ", Duration::from_secs(5))).unwrap_err();
        assert_eq!(err.kind, PortErrorKind::Invalid);
    }

    #[test]
    fn run_check_tail_is_the_last_4000_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("big.cjs");
        std::fs::write(&script, "process.stdout.write('가'.repeat(5000) + 'END');").unwrap();
        let env = NativeEnv::new(choice(EnvKind::Cmd), comspec()).unwrap();
        // node 출력은 UTF-8 이라 cmd 의 OEM 풀이와 맞지 않는다 — 크기만 본다. 제품이 지키는 것은 "원본 바이트 끝 TAIL_BYTES(4000) 개"이고,
        // OEM 풀이는 바이트마다 글자 하나(코드 페이지 437 이면 UTF-8 3바이트가 글자 3개, 글자마다 UTF-8 로 2~3바이트)라 글 길이(바이트)는 4000 을 넘을 수 있다.
        // 그래서 글자 수로 센다: 4000 바이트에서는 4000 글자를 넘을 수 없다
        let cmd = format!("\"{}\" \"{}\"", node().display(), script.display());
        let out = env.run_check(&check_run(dir.path(), "big", &cmd, Duration::from_secs(60))).unwrap();
        assert_eq!(out.exit, Some(0));
        let raw = std::fs::metadata(dir.path().join("evidence").join("big").join("output.log")).unwrap().len();
        assert!(raw > TAIL_BYTES, "출력 로그가 tail 한도보다 커야 자르는 경로를 시험한다: {raw}");
        let chars = out.tail.chars().count();
        assert!(chars > 0 && chars as u64 <= TAIL_BYTES, "tail 은 원본 끝 {TAIL_BYTES} 바이트(글자 {TAIL_BYTES} 개 이하)여야 한다: 글자 {chars}, 바이트 {}", out.tail.len());

        // git-bash 가 있으면 UTF-8 풀이로 글자까지 본다
        if let Some(bash) = native_entries().into_iter().find(|e| e.choice.kind == EnvKind::GitBash) {
            let env = NativeEnv::new(bash.choice.clone(), bash.program.clone()).unwrap();
            let cmd = format!("'{}' '{}'", posix(&node()), posix(&script));
            let out = env.run_check(&check_run(dir.path(), "big2", &cmd, Duration::from_secs(60))).unwrap();
            assert!(out.tail.ends_with("END") && out.tail.starts_with('가'), "깨진 글자로 시작하면 안 된다: {:?}", &out.tail[..12.min(out.tail.len())]);
            assert!(out.tail.len() <= 4000);
        }
    }

    #[test]
    fn env_check_matches_detect_programs() {
        // for_choice 가 detect() 의 프로그램을 그대로 쓴다
        let entries = native_entries();
        let first = entries.first().expect("네이티브 셸 하나는 있다");
        let env = NativeEnv::for_choice(&first.choice).unwrap();
        assert_eq!(env.choice(), &first.choice);
        let err = NativeEnv::for_choice(&EnvChoice { kind: EnvKind::Wsl, distro: Some("nope".into()) }).err().unwrap();
        assert_eq!(err.kind, PortErrorKind::NotFound);
    }
}
