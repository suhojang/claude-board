// WSL 실행 환경 어댑터 시험. 이 파일의 `#[ignore]` 아닌 시험은 **WSL 을 띄우지 않는다**(VM 이 메모리를 쓴다):
// - 경로 바꾸기·설정 만들기·명령(argv)·스크립트 글자는 순수 함수라 어디서나 돈다.
// - 보조 명령(목록·탐침·쓸어내기)의 흐름은 가짜 `WslRunner` 로 본다.
// - 리눅스 쪽 스크립트는 Windows 의 Git Bash `sh` 로 문법만 보고(`sh -n`), 띄우기 스크립트는 가짜 claude 로 돌려 본다. Git Bash 가 없으면 건너뛴다.
// `#[ignore]` 시험은 진짜 배포판을 깨운다 — 사람이 `--ignored` 로 돌릴 때만 돈다.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use claude_board_autopilot::adapters::wsl_env::*;
use claude_board_autopilot::ports::*;
use serde_json::json;

const DISTRO: &str = "Ubuntu-26.04";
const WSL_EXE: &str = r"C:\Windows\System32\wsl.exe";

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn env() -> WslEnv {
    WslEnv::new(DISTRO, p(WSL_EXE)).unwrap()
}

fn env_with(runner: Arc<Fake>) -> WslEnv {
    env().with_runner(runner)
}

// ── 가짜 이음매 ──────────────────────────────────────

struct Fake {
    calls: Mutex<Vec<WslCommand>>,
    replies: Mutex<VecDeque<PortResult<RunOutput>>>,
}

impl Fake {
    fn new(replies: Vec<PortResult<RunOutput>>) -> Arc<Self> {
        Arc::new(Fake { calls: Mutex::new(Vec::new()), replies: Mutex::new(replies.into()) })
    }

    fn calls(&self) -> Vec<WslCommand> {
        self.calls.lock().unwrap().clone()
    }

    fn args(&self) -> Vec<Vec<String>> {
        self.calls().into_iter().map(|c| c.args).collect()
    }
}

impl WslRunner for Fake {
    fn run(&self, cmd: &WslCommand, _limit: Duration) -> PortResult<RunOutput> {
        self.calls.lock().unwrap().push(cmd.clone());
        self.replies.lock().unwrap().pop_front().unwrap_or_else(|| Err(PortError::new(PortErrorKind::Io, "가짜 답이 모자란다")))
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn reply(exit: i32, output: Vec<u8>) -> PortResult<RunOutput> {
    Ok(RunOutput { exit: Some(exit), output })
}

fn reply_text(text: &str) -> PortResult<RunOutput> {
    reply(0, text.as_bytes().to_vec())
}

fn list_reply(version: u8) -> PortResult<RunOutput> {
    reply(
        0,
        utf16(&format!("  NAME              STATE           VERSION\r\n* {DISTRO}      Stopped         {version}\r\n  docker-desktop    Stopped         2\r\n")),
    )
}

const GOOD_PROBE: &str = "CB_CLAUDE=/home/u/.local/bin/claude\nCB_VERSION=2.1.5 (Claude Code)\nCB_BWRAP=/usr/bin/bwrap\nCB_SOCAT=/usr/bin/socat\nCB_BWRAP_RUN=1\nCB_AUTOMOUNT=\n";

// ── 경로 바꾸기 ──────────────────────────────────────

#[test]
fn to_env_path_table() {
    let ok = [
        (r"C:\a\b", "/mnt/c/a/b"),
        (r"c:/a/b/", "/mnt/c/a/b"),
        (r"D:\", "/mnt/d"),
        (r"\\?\C:\work\한글 폴더", "/mnt/c/work/한글 폴더"),
        (r"C:\a\.\b", "/mnt/c/a/b"),
        (r"C:\a\\b", "/mnt/c/a/b"),
        (r"C:\work\toy\pinned\*", "/mnt/c/work/toy/pinned/*"),
        (r"\\wsl.localhost\Ubuntu-26.04\home\u\p", "/home/u/p"),
        (r"\\wsl$\ubuntu-26.04\home", "/home"),
        (r"\\WSL.LOCALHOST\UBUNTU-26.04\home\", "/home"),
        (r"\\wsl.localhost\Ubuntu-26.04", "/"),
        (r"\\wsl.localhost\Ubuntu-26.04\", "/"),
        (r"\\?\UNC\wsl.localhost\Ubuntu-26.04\x", "/x"),
        (r"//wsl.localhost/Ubuntu-26.04/x/y", "/x/y"),
    ];
    let env = env();
    for (input, want) in ok {
        assert_eq!(env.to_env_path(Path::new(input)).as_deref(), Ok(want), "{input}");
    }
}

#[test]
fn to_env_path_rejects_what_it_cannot_map() {
    let bad = [
        "",
        r"work\toy",
        "C:rel",
        "C:",
        r"\rooted",
        r"C:\a\..\b",
        "C:\\a\0b",
        r"\\server\share\x",
        r"\\wsl.localhost\Debian\x",
        r"\\wsl.localhost\",
        r"\\wsl.localhost",
        r"\\?\UNC\server\share\x",
        r"\\wsl.localhost\Ubuntu-26.04\a\..\b",
    ];
    let env = env();
    for input in bad {
        let err = env.to_env_path(Path::new(input)).expect_err(input);
        assert_eq!(err.kind, PortErrorKind::Invalid, "{input}");
    }
}

#[test]
fn env_path_to_windows_table() {
    let ok = [
        ("/mnt/c/a/b", r"C:\a\b"),
        ("/mnt/c", r"C:\"),
        ("/mnt/C/x", r"C:\x"),
        ("/mnt/c/a/./b/", r"C:\a\b"),
        ("/mnt/c/한글 폴더", r"C:\한글 폴더"),
        ("/home/u/p", r"\\wsl.localhost\Ubuntu-26.04\home\u\p"),
        ("/", r"\\wsl.localhost\Ubuntu-26.04\"),
        ("/mnt/wsl/x", r"\\wsl.localhost\Ubuntu-26.04\mnt\wsl\x"),
        ("/mnt", r"\\wsl.localhost\Ubuntu-26.04\mnt"),
        ("/mnt/cc/x", r"\\wsl.localhost\Ubuntu-26.04\mnt\cc\x"),
    ];
    let env = env();
    for (input, want) in ok {
        assert_eq!(env.env_path_to_windows(input).unwrap(), p(want), "{input}");
    }
    for bad in ["", "home/u", "mnt/c/a", "/a/../b", "/mnt/c/..", "/a\\b", "/a\0b"] {
        let err = env.env_path_to_windows(bad).expect_err(bad);
        assert_eq!(err.kind, PortErrorKind::Invalid, "{bad}");
    }
}

#[test]
fn paths_round_trip() {
    let env = env();
    for windows in [r"C:\a\b", r"D:\", r"\\wsl.localhost\Ubuntu-26.04\home\u\p"] {
        let linux = env.to_env_path(Path::new(windows)).unwrap();
        assert_eq!(env.env_path_to_windows(&linux).unwrap(), p(windows), "{windows}");
    }
}

#[test]
fn automount_root_can_differ() {
    let env = env().with_automount_root("/media/win/").unwrap();
    assert_eq!(env.to_env_path(Path::new(r"C:\a")).unwrap(), "/media/win/c/a");
    assert_eq!(env.env_path_to_windows("/media/win/c/a").unwrap(), p(r"C:\a"));
    // 뿌리가 아닌 /mnt 아래는 그냥 배포판 경로다
    assert_eq!(env.env_path_to_windows("/mnt/c/a").unwrap(), p(r"\\wsl.localhost\Ubuntu-26.04\mnt\c\a"));

    let at_top = env.with_automount_root("/").unwrap();
    assert_eq!(at_top.to_env_path(Path::new(r"C:\a")).unwrap(), "/c/a");
    assert_eq!(at_top.env_path_to_windows("/c/a").unwrap(), p(r"C:\a"));
    assert_eq!(at_top.env_path_to_windows("/etc").unwrap(), p(r"\\wsl.localhost\Ubuntu-26.04\etc"));

    for bad in ["mnt", "", "/mnt/../x"] {
        assert_eq!(env_err(bad).kind, PortErrorKind::Invalid, "{bad}");
    }
}

fn env_err(root: &str) -> PortError {
    env().with_automount_root(root).err().expect(root)
}

#[test]
fn constructor_checks_the_distro_name() {
    for bad in ["", "a b", "a;b", "a/b", &"x".repeat(65)] {
        assert_eq!(WslEnv::new(bad, p(WSL_EXE)).err().unwrap().kind, PortErrorKind::Invalid, "{bad}");
    }
    let e = env();
    assert_eq!(e.choice(), &EnvChoice { kind: EnvKind::Wsl, distro: Some(DISTRO.into()) });
}

#[test]
fn from_entry_takes_only_wsl_entries() {
    use claude_board_core::envs::EnvEntry;
    let wsl = EnvEntry {
        choice: EnvChoice { kind: EnvKind::Wsl, distro: Some("Debian".into()) },
        label: "WSL · Debian".into(),
        program: p(WSL_EXE),
    };
    assert_eq!(WslEnv::from_entry(&wsl).unwrap().choice(), &wsl.choice);
    let cmd = EnvEntry { choice: EnvChoice { kind: EnvKind::Cmd, distro: None }, label: "cmd".into(), program: p(r"C:\Windows\System32\cmd.exe") };
    assert_eq!(WslEnv::from_entry(&cmd).err().unwrap().kind, PortErrorKind::Invalid);
    let nameless = EnvEntry { choice: EnvChoice { kind: EnvKind::Wsl, distro: None }, ..wsl };
    assert_eq!(WslEnv::from_entry(&nameless).err().unwrap().kind, PortErrorKind::Invalid);
}

// ── 설정 ─────────────────────────────────────────────

fn policy() -> SessionPolicy {
    SessionPolicy {
        allow_write: vec![p(r"C:\work\toy"), p(r"C:\work\toy")],
        deny_write: vec![p(r"C:\work\toy\CLAUDE.md"), p(r"C:\work\toy\.git\hooks"), p(r"C:\work\toy\pinned\*")],
        deny_read: vec![p(r"C:\Users\a\AppData\Local\cb\private")],
        network_allow: vec!["example.com".into(), "localhost:8080".into()],
        local_binding: true,
        allowed_tools: vec!["Read".into(), "Bash(npm test:*)".into()],
        disallowed_tools: vec!["Bash(git push *)".into(), "Read".into()],
    }
}

#[test]
fn render_settings_sandbox_and_linux_paths() {
    let settings = env().render_settings(&policy()).unwrap();
    assert_eq!(
        settings["sandbox"],
        json!({
            "enabled": true,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": false,
            "autoAllowBashIfSandboxed": true,
            "filesystem": {
                "allowWrite": ["/mnt/c/work/toy"],
                "denyWrite": [
                    "/mnt/c/work/toy/CLAUDE.md",
                    "/mnt/c/work/toy/.git/hooks",
                    "/mnt/c/work/toy/pinned/*",
                    "/mnt/c/Users/a/AppData/Local/cb/private",
                ],
                "denyRead": ["/mnt/c/Users/a/AppData/Local/cb/private"],
            },
            "network": {
                "allowedDomains": ["example.com", "localhost:8080"],
                "strictAllowlist": true,
                "allowLocalBinding": true,
                "allowUnixSockets": [],
            },
        })
    );
    assert_eq!(settings["permissions"]["allow"], json!(["Read", "Bash(npm test:*)", "Edit(//mnt/c/work/toy/**)"]));
    assert_eq!(
        settings["permissions"]["deny"],
        json!([
            "Bash(git push *)",
            "Read",
            "Edit(//mnt/c/work/toy/CLAUDE.md)",
            "Edit(//mnt/c/work/toy/CLAUDE.md/**)",
            "Edit(//mnt/c/work/toy/.git/hooks)",
            "Edit(//mnt/c/work/toy/.git/hooks/**)",
            "Edit(//mnt/c/work/toy/pinned/*)",
            "Read(//mnt/c/Users/a/AppData/Local/cb/private)",
            "Read(//mnt/c/Users/a/AppData/Local/cb/private/**)",
        ])
    );
}

#[test]
fn render_settings_key_order_and_empty_policy() {
    let settings = env().render_settings(&SessionPolicy::default()).unwrap();
    let top: Vec<&String> = settings.as_object().unwrap().keys().collect();
    assert_eq!(top, ["sandbox", "permissions"]);
    let sandbox: Vec<&String> = settings["sandbox"].as_object().unwrap().keys().collect();
    assert_eq!(sandbox, ["enabled", "failIfUnavailable", "allowUnsandboxedCommands", "autoAllowBashIfSandboxed", "filesystem", "network"]);
    assert_eq!(settings["sandbox"]["filesystem"], json!({ "allowWrite": [], "denyWrite": [], "denyRead": [] }));
    assert_eq!(settings["permissions"], json!({ "allow": [], "deny": [] }));
}

#[test]
fn render_settings_rejects_paths_it_cannot_map() {
    for bad in [r"work\toy", r"\\wsl.localhost\Debian\x", r"\\server\share"] {
        for place in 0..3 {
            let mut policy = SessionPolicy::default();
            match place {
                0 => policy.allow_write = vec![p(bad)],
                1 => policy.deny_write = vec![p(bad)],
                _ => policy.deny_read = vec![p(bad)],
            }
            assert_eq!(env().render_settings(&policy).unwrap_err().kind, PortErrorKind::Invalid, "{bad} {place}");
        }
    }
}

#[test]
fn render_settings_for_a_wsl_unc_workdir() {
    let policy = SessionPolicy { allow_write: vec![p(r"\\wsl.localhost\Ubuntu-26.04\home\u\proj")], ..SessionPolicy::default() };
    let settings = env().render_settings(&policy).unwrap();
    assert_eq!(settings["sandbox"]["filesystem"]["allowWrite"], json!(["/home/u/proj"]));
    assert_eq!(settings["permissions"]["allow"], json!(["Edit(//home/u/proj/**)"]));
}

// ── claude 띄우는 명령 ───────────────────────────────

fn launch() -> ClaudeLaunch {
    ClaudeLaunch {
        session: SessionStart::New("sid-1".into()),
        cwd: p(r"C:\work\toy"),
        model: "opus".into(),
        effort: Some("high".into()),
        settings_file: p(r"C:\run\settings.json"),
        system_prompt_file: Some(p(r"C:\run\prompt.md")),
        add_dirs: vec![p(r"C:\run\a"), p(r"C:\run\b c")],
        plugin_dir: Some(p(r"C:\run\plug")),
        env: vec![("CLAUDE_BOARD_AUTOPILOT".into(), "r1".into()), ("CLAUDE_BOARD_EVIDENCE_DIR".into(), r"C:\e".into())],
        path_env: vec!["CLAUDE_BOARD_EVIDENCE_DIR".into()],
        extra_args: vec!["--permission-mode".into(), "dontAsk".into()],
        stderr_log: p(r"C:\run\stderr.log"),
    }
}

#[test]
fn launch_command_is_argv_with_a_fixed_script() {
    let cmd = env().launch_command(&launch(), "cbap-1-ab-0").unwrap();
    assert_eq!(cmd.program, p(WSL_EXE));
    let want: Vec<String> = [
        "-d",
        DISTRO,
        "--cd",
        r"C:\work\toy",
        "-e",
        "sh",
        "-lc",
        LAUNCH_SCRIPT,
        "sh",
        "cbap-1-ab-0",
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
        "/mnt/c/run/settings.json",
        "--append-system-prompt-file",
        "/mnt/c/run/prompt.md",
        "--add-dir",
        "/mnt/c/run/a",
        "--add-dir",
        "/mnt/c/run/b c",
        "--plugin-dir",
        "/mnt/c/run/plug",
        "--permission-mode",
        "dontAsk",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(cmd.args, want);
    assert_eq!(
        cmd.env,
        [
            ("CLAUDE_BOARD_AUTOPILOT".to_string(), "r1".to_string()),
            ("CLAUDE_BOARD_EVIDENCE_DIR".to_string(), r"C:\e".to_string()),
            ("WSLENV".to_string(), "CLAUDE_BOARD_AUTOPILOT/u:CLAUDE_BOARD_EVIDENCE_DIR/up".to_string()),
        ]
    );
}

#[test]
fn launch_command_resume_verbatim_cwd_and_no_env() {
    let mut l = launch();
    l.session = SessionStart::Resume("sid-2".into());
    l.cwd = p(r"\\?\C:\work\toy");
    l.env.clear();
    l.path_env.clear();
    let cmd = env().launch_command(&l, "t").unwrap();
    assert_eq!(&cmd.args[2..4], ["--cd", r"C:\work\toy"]);
    assert!(cmd.args.windows(2).any(|w| w == ["--resume", "sid-2"]));
    assert_eq!(cmd.env, Vec::<(String, String)>::new(), "넘길 변수가 없으면 WSLENV 도 없다");

    l.cwd = p(r"\\wsl.localhost\Ubuntu-26.04\home\u\proj");
    assert_eq!(&env().launch_command(&l, "t").unwrap().args[2..4], ["--cd", "/home/u/proj"]);
}

#[test]
fn launch_command_never_puts_values_into_the_script() {
    let hostile = ["$(touch /tmp/x)", "`id`", "\"; rm -rf /; \"", "a'b", "line1\nline2", "--flag=$HOME"];
    let mut l = launch();
    l.extra_args = hostile.iter().map(|s| s.to_string()).collect();
    let tag = "cbap-9-ff-3";
    let cmd = env().launch_command(&l, tag).unwrap();
    let script_at = cmd.args.iter().position(|a| a == LAUNCH_SCRIPT).expect("스크립트는 고정 글자 그대로");
    for h in hostile {
        let at = cmd.args.iter().position(|a| a == h).unwrap_or_else(|| panic!("{h:?} 가 한 조각으로 있어야 한다"));
        assert!(at > script_at, "{h:?} 는 스크립트 뒤 위치 매개변수");
    }
    assert!(!LAUNCH_SCRIPT.contains(tag));
    assert_eq!(cmd.args[script_at + 1], "sh");
    assert_eq!(cmd.args[script_at + 2], tag);
}

#[test]
fn launch_command_errors() {
    let e = env();
    let mut l = launch();
    l.env.push(("A-B".into(), "x".into()));
    assert_eq!(e.launch_command(&l, "t").unwrap_err().kind, PortErrorKind::Invalid);

    let mut l = launch();
    l.cwd = p(r"\\wsl.localhost\Debian\home");
    assert_eq!(e.launch_command(&l, "t").unwrap_err().kind, PortErrorKind::Invalid);

    let mut l = launch();
    l.cwd = p(r"work\toy");
    assert_eq!(e.launch_command(&l, "t").unwrap_err().kind, PortErrorKind::Invalid);

    let mut l = launch();
    l.settings_file = p(r"rel\settings.json");
    assert_eq!(e.launch_command(&l, "t").unwrap_err().kind, PortErrorKind::Invalid);

    let mut l = launch();
    l.add_dirs = vec![p(r"\\server\share")];
    assert_eq!(e.launch_command(&l, "t").unwrap_err().kind, PortErrorKind::Invalid);
}

#[test]
fn launch_wslenv_skips_wslenv_and_duplicates() {
    let mut l = launch();
    l.env = vec![("A".into(), "1".into()), ("a".into(), "2".into()), ("WSLENV".into(), "X".into()), ("B".into(), "3".into())];
    l.path_env = vec!["b".into()];
    let cmd = env().launch_command(&l, "t").unwrap();
    let wslenv: Vec<&(String, String)> = cmd.env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("WSLENV")).collect();
    assert_eq!(wslenv.len(), 1);
    assert_eq!(wslenv[0].1, "A/u:B/up");
}

// ── 판정 명령 ────────────────────────────────────────

fn check_run() -> CheckRun {
    CheckRun {
        command: "pytest -q".into(),
        cwd: p(r"\\?\C:\work\toy"),
        env: vec![("CLAUDE_BOARD_AUTOPILOT".into(), "r1".into()), ("CLAUDE_BOARD_EVIDENCE_DIR".into(), r"C:\e".into())],
        path_env: vec!["CLAUDE_BOARD_EVIDENCE_DIR".into()],
        timeout: Duration::from_secs(600),
        output_log: p(r"C:\e\output.log"),
    }
}

#[test]
fn check_command_adds_the_tag_to_env_and_wslenv() {
    let inv = env().check_command(&check_run(), "cbap-1-ab-0", Some("PATH/l")).unwrap();
    assert_eq!(inv.program, p(WSL_EXE));
    assert_eq!(
        inv.args,
        [
            "-d", DISTRO, "--cd", r"C:\work\toy", "-e", "sh", "-c", MERGE_STDERR_SCRIPT, "sh", "timeout", "-s", "INT", "-k", "20", "600s", "bash", "-lc",
            "eval \"$CLAUDE_BOARD_CHECK\""
        ]
    );
    assert_eq!(inv.outer_timeout, Duration::from_secs(630));
    let get = |name: &str| inv.env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    assert_eq!(get("CLAUDE_BOARD_CHECK"), Some("pytest -q"));
    assert_eq!(get("CLAUDE_BOARD_AP_TAG"), Some("cbap-1-ab-0"));
    assert_eq!(
        get("WSLENV"),
        Some("PATH/l:CLAUDE_BOARD_CHECK/u:CLAUDE_BOARD_AUTOPILOT/u:CLAUDE_BOARD_EVIDENCE_DIR/up:CLAUDE_BOARD_AP_TAG/u")
    );
    let names: Vec<String> = inv.env.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(names.len(), unique.len(), "같은 이름이 둘 남지 않는다: {names:?}");
}

#[test]
fn check_command_replaces_a_caller_tag_and_keeps_rule_errors() {
    let mut run = check_run();
    run.env.push(("claude_board_ap_tag".into(), "forged".into()));
    let inv = env().check_command(&run, "real", None).unwrap();
    let tags: Vec<&(String, String)> = inv.env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("CLAUDE_BOARD_AP_TAG")).collect();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].1, "real");
    assert!(inv.env.iter().find(|(n, _)| n == "WSLENV").unwrap().1.ends_with(":CLAUDE_BOARD_AP_TAG/u"));

    let mut run = check_run();
    run.command = "  ".into();
    assert_eq!(env().check_command(&run, "t", None).unwrap_err().kind, PortErrorKind::Invalid);
    let mut run = check_run();
    run.env.push(("A-B".into(), "x".into()));
    assert_eq!(env().check_command(&run, "t", None).unwrap_err().kind, PortErrorKind::Invalid);
    let mut run = check_run();
    run.cwd = p(r"\\wsl.localhost\Debian\home");
    assert_eq!(env().check_command(&run, "t", None).unwrap_err().kind, PortErrorKind::Invalid);
}

/// 합침 감싸개는 판정 명령의 글과 무관하다 — 글은 환경 변수로만 가고 argv 는 늘 같은 꼴이다(따옴표·`$`·줄바꿈·한글·아주 긴 글도 argv 를 오염시키지 않는다)
#[test]
fn check_command_argv_does_not_depend_on_the_command_text() {
    let base = env().check_command(&check_run(), "t", None).unwrap().args;
    assert_eq!(base.iter().filter(|a| *a == MERGE_STDERR_SCRIPT).count(), 1, "감싸개 스크립트는 한 번만 든다");
    let at = base.iter().position(|a| a == "-e").unwrap();
    assert_eq!(base[at + 1..at + 5], ["sh", "-c", MERGE_STDERR_SCRIPT, "sh"], "-e 바로 뒤에 감싸개");
    let texts = [
        "echo \"a b\" && echo 'c'".to_string(),
        "echo $HOME `date` $(id)".to_string(),
        "첫 줄\n둘째 줄\r\n셋째".to_string(),
        "한글 명령 \u{1F600} 이모지".to_string(),
        "x".repeat(100_000),
        "-e sh -c evil".to_string(),
    ];
    for text in texts {
        let mut run = check_run();
        run.command = text.clone();
        let inv = env().check_command(&run, "t", None).unwrap();
        assert_eq!(inv.args, base, "명령 글이 argv 에 새어 들었다: {:?}", text.chars().take(30).collect::<String>());
        let carried = inv.env.iter().find(|(n, _)| n == "CLAUDE_BOARD_CHECK").map(|(_, v)| v.as_str());
        assert_eq!(carried, Some(text.as_str()), "명령 글은 환경 변수로 그대로 간다");
    }
}

// ── 보조 명령(목록·탐침·쓸어내기) ────────────────────

#[test]
fn helper_commands_are_argv_and_one_line_scripts() {
    let e = env();
    let r = e.remote();
    assert_eq!(r.list_command().args, ["-l", "-v"]);
    assert_eq!(r.running_command().args, ["-l", "--running", "-q"]);
    assert_eq!(r.probe_command().args, ["-d", DISTRO, "-e", "sh", "-c", MERGE_STDERR_SCRIPT, "sh", "sh", "-lc", PROBE_SCRIPT]);
    assert_eq!(r.sweep_command("cbap-1-a-0").args, ["-d", DISTRO, "-e", "sh", "-c", SWEEP_SCRIPT, "sh", "cbap-1-a-0"]);
    assert_eq!(r.interrupt_command("cbap-1-a-0").args, ["-d", DISTRO, "-e", "sh", "-c", INTERRUPT_SCRIPT, "sh", "cbap-1-a-0"]);
    for c in [r.list_command(), r.running_command(), r.probe_command(), r.sweep_command("t"), r.interrupt_command("t")] {
        assert_eq!(c.program, p(WSL_EXE));
        assert!(c.env.is_empty());
    }
    for (name, script) in [("launch", LAUNCH_SCRIPT), ("probe", PROBE_SCRIPT), ("interrupt", INTERRUPT_SCRIPT), ("sweep", SWEEP_SCRIPT), ("merge", MERGE_STDERR_SCRIPT)] {
        assert!(!script.contains('\n') && !script.contains('\r'), "{name} 은 한 줄");
        assert!(!script.contains("cbap"), "{name} 에 꼬리표가 박혀 있지 않다");
    }
}

#[test]
fn parse_wsl_verbose_reads_columns_without_trusting_the_language() {
    let rows = parse_wsl_verbose(&utf16(
        "\u{feff}  NAME              STATE           VERSION\r\n* Ubuntu-26.04      Running         2\r\n  Debian            Stopped         1\r\n  docker-desktop    Stopped         2\r\n",
    ));
    assert_eq!(
        rows,
        [
            DistroRow { name: "Ubuntu-26.04".into(), state: "Running".into(), version: 2, is_default: true },
            DistroRow { name: "Debian".into(), state: "Stopped".into(), version: 1, is_default: false },
            DistroRow { name: "docker-desktop".into(), state: "Stopped".into(), version: 2, is_default: false },
        ]
    );
    let korean = parse_wsl_verbose(&utf16("  이름              상태            버전\r\n* Ubuntu-26.04      실행 중         2\r\n"));
    assert_eq!(korean, [DistroRow { name: "Ubuntu-26.04".into(), state: "실행 중".into(), version: 2, is_default: true }]);
    assert!(parse_wsl_verbose(b"").is_empty());
    assert!(parse_wsl_verbose(&utf16("Windows Subsystem for Linux 가 설치되어 있지 않습니다.\r\n")).is_empty());
}

#[test]
fn parse_probe_reads_only_cb_lines() {
    let p = parse_probe(&format!("welcome to ubuntu\r\nA=B\r\n{GOOD_PROBE}"));
    assert_eq!(
        p,
        Probe {
            claude: Some("/home/u/.local/bin/claude".into()),
            version: Some("2.1.5 (Claude Code)".into()),
            bwrap: true,
            socat: true,
            bwrap_runs: Some(true),
            automount: None,
        }
    );
    assert!(sandbox_ready(&p));
    assert_eq!(parse_probe(""), Probe::default());
    let crlf = parse_probe("CB_CLAUDE=/x/claude\r\nCB_AUTOMOUNT=/media/win\r\nCB_BWRAP_RUN=0\r\nCB_BWRAP=/usr/bin/bwrap\r\nCB_SOCAT=/usr/bin/socat\r\n");
    assert_eq!(crlf.automount.as_deref(), Some("/media/win"));
    assert_eq!(crlf.bwrap_runs, Some(false));
    assert!(!sandbox_ready(&crlf), "bubblewrap 이 돌지 않으면 준비 안 됨");
}

#[test]
fn probe_problems_say_what_to_install() {
    let good = parse_probe(GOOD_PROBE);
    assert_eq!(probe_problems(DISTRO, "/mnt", &good), Vec::<String>::new());

    let none = parse_probe("CB_CLAUDE=\nCB_BWRAP=\nCB_SOCAT=\n");
    let problems = probe_problems(DISTRO, "/mnt", &none);
    assert_eq!(problems.len(), 2);
    assert!(problems[0].contains("안에 claude 가 없습니다"));
    assert!(problems[1].contains("bubblewrap·socat") && problems[1].contains("sudo apt install bubblewrap socat"));

    let only_socat_missing = parse_probe(&GOOD_PROBE.replace("CB_SOCAT=/usr/bin/socat", "CB_SOCAT="));
    let problems = probe_problems(DISTRO, "/mnt", &only_socat_missing);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("socat 이(가)") && !problems[0].contains("bubblewrap 이(가)") && !problems[0].contains("bubblewrap·"));
    assert!(!sandbox_ready(&only_socat_missing));

    let windows_claude = parse_probe(&GOOD_PROBE.replace("/home/u/.local/bin/claude", "/mnt/c/Users/a/AppData/Roaming/npm/claude"));
    let problems = probe_problems(DISTRO, "/mnt", &windows_claude);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("Windows 쪽 파일"));
    // /mnt 아래라도 드라이브 글자 칸이 아니면 Windows 쪽이 아니다
    let wsl_internal = parse_probe(&GOOD_PROBE.replace("/home/u/.local/bin/claude", "/mnt/wsl/tools/claude"));
    assert!(probe_problems(DISTRO, "/mnt", &wsl_internal).is_empty());

    let silent_version = parse_probe(&GOOD_PROBE.replace("CB_VERSION=2.1.5 (Claude Code)\n", "CB_VERSION=\n"));
    assert!(probe_problems(DISTRO, "/mnt", &silent_version)[0].contains("claude --version"));

    let apparmor = parse_probe(&GOOD_PROBE.replace("CB_BWRAP_RUN=1", "CB_BWRAP_RUN=0"));
    let problems = probe_problems(DISTRO, "/mnt", &apparmor);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("AppArmor"));

    let moved = parse_probe(&GOOD_PROBE.replace("CB_AUTOMOUNT=", "CB_AUTOMOUNT=/media/win/"));
    let problems = probe_problems(DISTRO, "/mnt", &moved);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("/media/win") && problems[0].contains("/mnt"));
    assert!(probe_problems(DISTRO, "/media/win", &moved).is_empty());
    assert_eq!(probe_problems(DISTRO, "", &good).len(), 1, "뿌리를 /(빈 글)로 알고 있는데 배포판은 /mnt");
}

#[test]
fn check_ready_ok_walks_list_then_probe() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(vec![list_reply(2), reply_text(GOOD_PROBE)]);
    let report = env_with(fake.clone()).check_ready(dir.path());
    assert_eq!(
        report,
        EnvReport { ready: true, sandbox: true, claude_version: Some("2.1.5 (Claude Code)".into()), problems: vec![] }
    );
    let args = fake.args();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], ["-l", "-v"], "목록이 먼저(깨우지 않음)");
    assert_eq!(args[1][..9], ["-d", DISTRO, "-e", "sh", "-c", MERGE_STDERR_SCRIPT, "sh", "sh", "-lc"], "탐침은 표준 오류를 합친 뒤 로그인 셸");
}

#[test]
fn check_ready_does_not_wake_the_vm_when_the_list_already_says_no() {
    let dir = tempfile::tempdir().unwrap();

    let fake = Fake::new(vec![list_reply(1)]);
    let report = env_with(fake.clone()).check_ready(dir.path());
    assert!(!report.ready && !report.sandbox);
    assert_eq!(report.problems.len(), 1);
    assert!(report.problems[0].contains("WSL1") && report.problems[0].contains("wsl --set-version Ubuntu-26.04 2"));
    assert_eq!(fake.calls().len(), 1, "WSL1 이면 탐침(배포판 깨우기)을 하지 않는다");

    let other = Fake::new(vec![reply(0, utf16("  NAME STATE VERSION\r\n* Debian Stopped 2\r\n"))]);
    let report = env_with(other.clone()).check_ready(dir.path());
    assert!(!report.ready);
    assert!(report.problems[0].contains("찾지 못했습니다"));
    assert_eq!(other.calls().len(), 1);

    let missing_dir = dir.path().join("없는 폴더");
    let fake = Fake::new(vec![list_reply(2)]);
    let report = env_with(fake.clone()).check_ready(&missing_dir);
    assert!(!report.ready);
    assert!(report.problems[0].contains("작업 폴더가 없습니다"));
    assert_eq!(fake.calls().len(), 1, "다른 문제가 있으면 탐침하지 않는다");
}

#[test]
fn check_ready_reports_wsl_exe_failures() {
    let dir = tempfile::tempdir().unwrap();
    let down = Fake::new(vec![Err(PortError::new(PortErrorKind::NotFound, "wsl.exe 를 찾지 못했습니다"))]);
    let report = env_with(down).check_ready(dir.path());
    assert!(!report.ready && report.problems[0].contains("wsl.exe 를 실행하지 못했습니다"));

    let failing = Fake::new(vec![reply(1, utf16("WSL 이 설치되지 않았습니다."))]);
    let report = env_with(failing).check_ready(dir.path());
    assert!(report.problems[0].contains("종료 코드 1"));

    let hung = Fake::new(vec![Ok(RunOutput { exit: None, output: Vec::new() })]);
    let report = env_with(hung).check_ready(dir.path());
    assert!(report.problems[0].contains("시간 안에"));
}

#[test]
fn check_ready_collects_probe_problems() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(vec![list_reply(2), reply_text("CB_CLAUDE=\nCB_BWRAP=\nCB_SOCAT=\nCB_AUTOMOUNT=\n")]);
    let report = env_with(fake).check_ready(dir.path());
    assert!(!report.ready && !report.sandbox);
    assert_eq!(report.claude_version, None);
    assert_eq!(report.problems.len(), 2);
    assert!(report.problems[1].contains("sudo apt install bubblewrap socat"));

    let timed_out = Fake::new(vec![list_reply(2), Ok(RunOutput { exit: None, output: Vec::new() })]);
    let report = env_with(timed_out).check_ready(dir.path());
    assert!(!report.ready && report.problems[0].contains("20초 안에"));

    let broken = Fake::new(vec![list_reply(2), reply(1, b"wsl: something failed".to_vec())]);
    let report = env_with(broken).check_ready(dir.path());
    assert!(!report.ready && report.problems[0].contains("종료 코드 1"));

    let no_runner = Fake::new(vec![list_reply(2), Err(PortError::new(PortErrorKind::Timeout, "시간"))]);
    assert!(!env_with(no_runner).check_ready(dir.path()).ready);

    // 소스가 claude 는 있는데 샌드박스 준비물만 모자란 경우: ready 는 false, sandbox 도 false
    let fake = Fake::new(vec![list_reply(2), reply_text(&GOOD_PROBE.replace("CB_BWRAP=/usr/bin/bwrap", "CB_BWRAP="))]);
    let report = env_with(fake).check_ready(dir.path());
    assert!(!report.ready && !report.sandbox);
    assert_eq!(report.claude_version.as_deref(), Some("2.1.5 (Claude Code)"));
}

#[test]
fn check_ready_respects_a_custom_automount_root() {
    let dir = tempfile::tempdir().unwrap();
    let probe = GOOD_PROBE.replace("CB_AUTOMOUNT=", "CB_AUTOMOUNT=/media/win");
    let fake = Fake::new(vec![list_reply(2), reply_text(&probe)]);
    let ok = env().with_automount_root("/media/win").unwrap().with_runner(fake).check_ready(dir.path());
    assert!(ok.ready, "{:?}", ok.problems);
    let fake = Fake::new(vec![list_reply(2), reply_text(&probe)]);
    assert!(!env_with(fake).check_ready(dir.path()).ready);
}

fn running_reply(names: &str) -> PortResult<RunOutput> {
    reply(0, utf16(names))
}

#[test]
fn sweep_does_not_wake_a_stopped_distro() {
    let fake = Fake::new(vec![running_reply("Debian\r\n")]);
    let e = env_with(fake.clone());
    e.remote().sweep("cbap-1-a-0", true).unwrap();
    assert_eq!(fake.args(), [vec!["-l", "--running", "-q"]], "꺼져 있으면 쓸어내기 명령을 보내지 않는다");

    // 확인 자체가 실패해도 깨우지 않는 쪽으로 기운다
    let fake = Fake::new(vec![Err(PortError::new(PortErrorKind::Io, "x"))]);
    env_with(fake.clone()).remote().sweep("t", true).unwrap();
    assert_eq!(fake.calls().len(), 1);
    let fake = Fake::new(vec![reply(1, utf16("There are no running distributions.\r\n"))]);
    env_with(fake.clone()).remote().sweep("t", true).unwrap();
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn sweep_runs_the_script_with_the_tag_when_running_or_forced() {
    let fake = Fake::new(vec![running_reply("Ubuntu-26.04\r\ndocker-desktop\r\n"), reply(0, Vec::new())]);
    let e = env_with(fake.clone());
    e.remote().sweep("cbap-1-a-0", true).unwrap();
    let args = fake.args();
    assert_eq!(args.len(), 2);
    assert_eq!(args[1], e.remote().sweep_command("cbap-1-a-0").args);

    let forced = Fake::new(vec![reply(0, Vec::new())]);
    env_with(forced.clone()).remote().sweep("cbap-2-a-0", false).unwrap();
    assert_eq!(forced.calls().len(), 1, "방금 판정을 돌린 뒤에는 확인 없이 바로 쓸어낸다");

    let failed = Fake::new(vec![reply(5, Vec::new())]);
    assert_eq!(env_with(failed).remote().sweep("t", false).unwrap_err().kind, PortErrorKind::Io);
    let hung = Fake::new(vec![Ok(RunOutput { exit: None, output: Vec::new() })]);
    assert_eq!(env_with(hung).remote().sweep("t", false).unwrap_err().kind, PortErrorKind::Timeout);
    let down = Fake::new(vec![Err(PortError::new(PortErrorKind::NotFound, "없음"))]);
    assert_eq!(env_with(down).remote().sweep("t", false).unwrap_err().kind, PortErrorKind::NotFound);
}

#[test]
fn interrupt_maps_script_exit_codes() {
    let table = [(0, None), (2, Some(PortErrorKind::NotFound)), (3, Some(PortErrorKind::NotFound)), (64, Some(PortErrorKind::Io))];
    for (code, want) in table {
        let fake = Fake::new(vec![running_reply("Ubuntu-26.04\r\n"), reply(code, Vec::new())]);
        let got = env_with(fake.clone()).remote().interrupt("cbap-1-a-0");
        assert_eq!(got.err().map(|e| e.kind), want, "종료 코드 {code}");
        assert_eq!(fake.args()[1], env().remote().interrupt_command("cbap-1-a-0").args);
    }
    let off = Fake::new(vec![running_reply("")]);
    assert_eq!(env_with(off.clone()).remote().interrupt("t").unwrap_err().kind, PortErrorKind::Unavailable);
    assert_eq!(off.calls().len(), 1);
}

// ── 리눅스 쪽 스크립트를 Git Bash 의 sh 로 ────────────

#[cfg(windows)]
mod git_sh {
    use super::*;
    use std::process::Command;

    fn find_sh() -> Option<PathBuf> {
        ["ProgramFiles", "ProgramW6432"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|d| PathBuf::from(d).join("Git").join("usr").join("bin").join("sh.exe"))
            .find(|p| p.is_file())
    }

    fn sh_or_skip() -> Option<PathBuf> {
        let sh = find_sh();
        if sh.is_none() {
            eprintln!("Git Bash 의 sh.exe 가 없어 건너뜀");
        }
        sh
    }

    fn run_script(sh: &Path, script: &str, args: &[&str], path_prefix: Option<&Path>) -> std::process::Output {
        let mut cmd = Command::new(sh);
        cmd.arg("-c").arg(script).arg("sh").args(args);
        let usr_bin = sh.parent().unwrap().display().to_string();
        let path = match path_prefix {
            Some(dir) => format!("{};{usr_bin}", dir.display()),
            None => usr_bin,
        };
        cmd.env("PATH", path).output().unwrap()
    }

    /// 스크립트 파일로 써서 `sh -n`(문법 검사만)
    #[test]
    fn scripts_parse() {
        let Some(sh) = sh_or_skip() else { return };
        let dir = tempfile::tempdir().unwrap();
        for (name, script) in [("launch", LAUNCH_SCRIPT), ("probe", PROBE_SCRIPT), ("interrupt", INTERRUPT_SCRIPT), ("sweep", SWEEP_SCRIPT), ("merge", MERGE_STDERR_SCRIPT)] {
            let file = dir.path().join(format!("{name}.sh"));
            std::fs::write(&file, script).unwrap();
            let out = Command::new(&sh).arg("-n").arg(&file).output().unwrap();
            assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    /// 표준 오류가 표준 출력으로 합쳐져 나오고(순서 그대로) 표준 오류 쪽에는 아무것도 남지 않는다 — wsl.exe 가 둘을 같은 파일에 덮어쓰지 못하게 하는 장치
    #[test]
    fn merge_script_folds_stderr_into_stdout_in_order() {
        let Some(sh) = sh_or_skip() else { return };
        let out = run_script(&sh, MERGE_STDERR_SCRIPT, &["sh", "-c", "echo hello; echo err >&2; echo more; exit 3"], None);
        assert_eq!(out.status.code(), Some(3), "안쪽 명령의 종료 코드가 그대로 나온다");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nerr\nmore\n");
        assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 안쪽 `sh -c <script>` 를 합침 스크립트로 감싸 돌린다
    fn merge_sh(sh: &Path, script: &str) -> std::process::Output {
        run_script(sh, MERGE_STDERR_SCRIPT, &["sh", "-c", script], None)
    }

    /// 종료 코드는 안쪽 명령 것이 그대로 나온다(0·1·2·42·127·255). 표준 오류 쪽에는 아무것도 남지 않는다
    #[test]
    fn merge_script_preserves_exit_codes() {
        let Some(sh) = sh_or_skip() else { return };
        let table: [(&str, i32); 7] = [
            ("exit 0", 0),
            ("exit 1", 1),
            ("exit 2", 2),
            ("echo e >&2; exit 42", 42),
            ("exit 255", 255),
            ("cb_no_such_command_xyz", 127),
            ("false", 1),
        ];
        for (script, want) in table {
            let out = merge_sh(&sh, script);
            assert_eq!(out.status.code(), Some(want), "{script:?}: {}", String::from_utf8_lossy(&out.stdout));
            assert!(out.stderr.is_empty(), "{script:?}: 표준 오류가 비어 있지 않다: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    /// 실행할 수 없는 프로그램(`sh -c` 를 거치지 않고 바로 `exec`)도 127 이고, 셸이 낸 "없다" 글은 표준 출력으로 나온다
    #[test]
    fn merge_script_exec_failure_is_127_and_message_goes_to_stdout() {
        let Some(sh) = sh_or_skip() else { return };
        for cmd in [vec!["cb-no-such-program-xyz"], vec!["cb-no-such-program-xyz", "arg one"]] {
            let out = run_script(&sh, MERGE_STDERR_SCRIPT, &cmd, None);
            assert_eq!(out.status.code(), Some(127), "{cmd:?}");
            assert!(out.stderr.is_empty(), "{cmd:?}: {}", String::from_utf8_lossy(&out.stderr));
            assert!(String::from_utf8_lossy(&out.stdout).contains("cb-no-such-program-xyz"), "{cmd:?}: 오류 글이 표준 출력에 없다");
        }
        let out = run_script(&sh, MERGE_STDERR_SCRIPT, &[""], None);
        assert_eq!(out.status.code(), Some(127), "빈 문자열 명령");
    }

    /// 명령이 하나도 없으면 `exec` 는 할 일 없이 0 으로 끝난다 — 호출 쪽(check_command·probe_command)은 늘 명령을 붙인다는 전제를 못 박는다
    #[test]
    fn merge_script_without_a_command_exits_0_silently() {
        let Some(sh) = sh_or_skip() else { return };
        let out = run_script(&sh, MERGE_STDERR_SCRIPT, &[], None);
        assert_eq!(out.status.code(), Some(0));
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }

    /// 표준 출력·표준 오류 글이 한 흐름으로, 쓴 순서 그대로 나온다(바이트 그대로 — 줄바꿈 없는 끝, 비 UTF-8 바이트, 큰 양 포함)
    #[test]
    fn merge_script_stream_table() {
        let Some(sh) = sh_or_skip() else { return };
        let alternating: String = (0..200).map(|i| format!("o{i}\ne{i}\n")).collect();
        let big = "x".repeat(300_000);
        let table: Vec<(&str, &str, Vec<u8>)> = vec![
            ("표준 오류만", "echo e1 >&2; echo e2 >&2", b"e1\ne2\n".to_vec()),
            ("표준 출력만", "echo o1; echo o2", b"o1\no2\n".to_vec()),
            ("번갈아", "echo 1; echo 2 >&2; echo 3; echo 4 >&2", b"1\n2\n3\n4\n".to_vec()),
            ("줄바꿈 없는 끝", "printf a; printf b >&2; printf c", b"abc".to_vec()),
            ("아무 글도 없음", "true", Vec::new()),
            ("비 UTF-8 바이트", "printf '\\377\\376'; printf '\\200' >&2", vec![0xff, 0xfe, 0x80]),
            ("NUL 바이트", "printf 'a\\0b'; printf '\\0c' >&2", b"a\0b\0c".to_vec()),
            ("자식 프로세스의 표준 오류", "echo a; sh -c 'echo b >&2'; echo c", b"a\nb\nc\n".to_vec()),
            ("한글", "echo 가; echo 나 >&2", "가\n나\n".as_bytes().to_vec()),
            ("200번 번갈아", "i=0; while [ $i -lt 200 ]; do echo o$i; echo e$i >&2; i=$((i+1)); done", alternating.into_bytes()),
            ("표준 오류로 큰 양", "head -c 300000 /dev/zero | tr '\\0' x >&2", big.into_bytes()),
        ];
        for (label, script, want) in table {
            let out = merge_sh(&sh, script);
            assert!(out.status.success(), "{label}: {:?}", out.status);
            assert!(out.stderr.is_empty(), "{label}: 표준 오류가 남았다: {}", String::from_utf8_lossy(&out.stderr));
            let (got, exp) = (&out.stdout, &want);
            assert!(got == exp, "{label}: 출력이 다르다\n기대 {:?}\n실제 {:?}", String::from_utf8_lossy(&exp[..exp.len().min(80)]), String::from_utf8_lossy(&got[..got.len().min(80)]));
        }
    }

    /// 감싸개를 거쳐도 명령 인자는 한 칸도 쪼개지거나 풀리지 않는다(공백·`$`·`;`·`&|`·`-`로 시작하는 글). `*`·`~` 는 msys 가 Windows 에서 온 인자를 먼저 풀어 버려 가르지 못하고, 작은따옴표·큰따옴표·빈 인자·줄바꿈은 Windows 명령줄을 거치며
    /// Git Bash(msys) 쪽 규칙에 걸려 뒤 인자까지 망가뜨리므로 이 시험으로는 가르지 못한다(`launch_script_...` 와 같은 사정)
    #[test]
    fn merge_script_passes_argv_verbatim() {
        let Some(sh) = sh_or_skip() else { return };
        let payloads = ["a b", "$HOME", "a;b", "a&b|c", "--", "-n", "\\n", "  lead and trail  ", "한 글"];
        let mut args = vec!["sh", "-c", "for a in \"$@\"; do printf '[%s]' \"$a\"; done", "x"];
        args.extend(payloads);
        let out = run_script(&sh, MERGE_STDERR_SCRIPT, &args, None);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        let want: String = payloads.iter().map(|a| format!("[{a}]")).collect();
        assert_eq!(String::from_utf8_lossy(&out.stdout), want);
        assert!(out.stderr.is_empty());
    }

    /// 감싸개는 입력도 가로채지 않는다 — 표준 입력이 안쪽 명령에 닿는다
    #[test]
    fn merge_script_keeps_stdin_for_the_inner_command() {
        let Some(sh) = sh_or_skip() else { return };
        use std::io::Write;
        use std::process::Stdio;
        let mut cmd = Command::new(&sh);
        cmd.arg("-c").arg(MERGE_STDERR_SCRIPT).arg("sh").arg("cat").env("PATH", sh.parent().unwrap().display().to_string());
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all("들어간 글\n".as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "들어간 글\n");
    }

    #[test]
    fn script_tag_guard_rejects_odd_tags() {
        let Some(sh) = sh_or_skip() else { return };
        for script in [LAUNCH_SCRIPT, INTERRUPT_SCRIPT, SWEEP_SCRIPT] {
            for tag in ["", "a b", "a;b", "../x", "a$b"] {
                let out = run_script(&sh, script, &[tag], None);
                assert_eq!(out.status.code(), Some(64), "tag {tag:?}: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
    }

    #[test]
    fn interrupt_without_a_pid_file_exits_2() {
        let Some(sh) = sh_or_skip() else { return };
        let out = run_script(&sh, INTERRUPT_SCRIPT, &["cbap-no-such-tag-0"], None);
        assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 찾을 프로세스가 없어도 성공하고 pid 파일은 지운다. 남의 pid(없는 번호)가 적혀 있어도 아무것도 끝내지 않는다
    #[test]
    fn sweep_with_nothing_to_kill_removes_the_pid_file() {
        let Some(sh) = sh_or_skip() else { return };
        let tag = format!("cbap-test-{}-sweep", std::process::id());
        let pidfile = std::env::temp_dir().join(format!("{tag}.pid"));
        std::fs::write(&pidfile, "999999999\n").unwrap();
        let out = run_script(&sh, SWEEP_SCRIPT, &[&tag], None);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(!pidfile.exists(), "pid 파일(/tmp 가 이 임시 폴더로 보이는 Git Bash 기준)이 지워져야 한다");
    }

    /// 띄우기 스크립트: pid 파일에 `$$` 를 적고, 꼬리표를 환경에 넣고, 인자를 있는 그대로(공백·따옴표·`$`) claude 에 넘긴다
    #[test]
    fn launch_script_writes_the_pid_exports_the_tag_and_passes_args_verbatim() {
        let Some(sh) = sh_or_skip() else { return };
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("claude");
        std::fs::write(&fake, "#!/bin/sh\necho \"pid=$$\"\nfor a in \"$@\"; do echo \"arg=$a\"; done\necho \"tag=$CLAUDE_BOARD_AP_TAG\"\n").unwrap();
        let tag = format!("cbap-test-{}-launch", std::process::id());
        let pidfile = std::env::temp_dir().join(format!("{tag}.pid"));
        let _ = std::fs::remove_file(&pidfile);
        // 따옴표가 든 인자는 Windows 명령줄 → msys 를 거치며 Git Bash 쪽 규칙에 걸려 이 시험으로는 가를 수 없다(argv 조각 보존은 위 순수 시험이 본다)
        let args = ["-p", "a b", "$(touch /tmp/cb-pwned)", "--flag=$HOME", "a;b"];
        let mut all = vec![tag.as_str()];
        all.extend(args);
        let out = run_script(&sh, LAUNCH_SCRIPT, &all, Some(dir.path()));
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
        let got_args: Vec<&str> = stdout.lines().filter_map(|l| l.strip_prefix("arg=")).collect();
        assert_eq!(got_args, args);
        assert!(stdout.lines().any(|l| l == format!("tag={tag}")), "{stdout}");
        let pid_line = stdout.lines().find_map(|l| l.strip_prefix("pid=")).expect("가짜 claude 가 pid 를 냄");
        let written = std::fs::read_to_string(&pidfile).expect("pid 파일");
        assert_eq!(written.trim(), pid_line.trim(), "exec 로 같은 pid 가 claude 가 된다");
        let _ = std::fs::remove_file(&pidfile);
    }
}

// ── 진짜 배포판(사람이 `--ignored` 로 돌릴 때만) ─────────
// 아래는 배포판을 깨운다. 이 저장소의 어떤 자동 실행도 돌리지 않는다.

fn first_real_wsl() -> WslEnv {
    let entry = claude_board_core::envs::detect().into_iter().find(|e| e.choice.kind == EnvKind::Wsl).expect("이 PC 에 WSL 배포판이 있어야 한다");
    WslEnv::from_entry(&entry).unwrap()
}

#[test]
#[ignore = "WSL VM 을 깨운다"]
fn real_check_ready_first_distro() {
    let dir = tempfile::tempdir().unwrap();
    let report = first_real_wsl().check_ready(dir.path());
    println!("{report:#?}");
    assert!(report.problems.is_empty() == report.ready);
}

#[test]
#[ignore = "WSL VM 을 깨운다"]
fn real_run_check_exit_code_and_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("out").join("output.log");
    let run = CheckRun {
        command: "echo hello-$CLAUDE_BOARD_AUTOPILOT; pwd; echo err >&2; exit 3".into(),
        cwd: dir.path().to_path_buf(),
        env: vec![("CLAUDE_BOARD_AUTOPILOT".into(), "r1".into())],
        path_env: vec![],
        timeout: Duration::from_secs(30),
        output_log: log.clone(),
    };
    let env = first_real_wsl();
    let outcome = env.run_check(&run).unwrap();
    println!("{outcome:#?}");
    assert_eq!(outcome.exit, Some(3));
    assert!(!outcome.timed_out);
    assert!(outcome.tail.contains("hello-r1") && outcome.tail.contains("err"));
    let expected = env.to_env_path(dir.path()).unwrap();
    assert!(outcome.tail.contains(&expected), "{} 가 {} 를 품어야 한다", outcome.tail, expected);
    // 표준 출력과 오류가 순서 그대로 한 파일에 남는다(wsl.exe 는 둘을 같은 파일에 두면 나중 글이 앞 글을 덮는다)
    assert_eq!(outcome.tail, format!("hello-r1\n{expected}\nerr\n"));
}

#[test]
#[ignore = "WSL VM 을 깨운다"]
fn real_run_check_timeout_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let run = CheckRun {
        command: "sleep 300 & sleep 301".into(),
        cwd: dir.path().to_path_buf(),
        env: vec![],
        path_env: vec![],
        timeout: Duration::from_secs(2),
        output_log: dir.path().join("output.log"),
    };
    let env = first_real_wsl();
    let outcome = env.run_check(&run).unwrap();
    println!("{outcome:#?}");
    assert!(outcome.timed_out);
    assert_eq!(outcome.exit, None);
    // 남은 sleep 이 없어야 한다(pgrep 이 아무것도 못 찾으면 종료 코드 1)
    let still = std::process::Command::new(&env.remote().list_command().program)
        .args(["-d", DISTRO, "-e", "pgrep", "-f", "sleep 30[01]"])
        .output()
        .unwrap();
    assert_eq!(still.status.code(), Some(1), "{}", String::from_utf8_lossy(&still.stdout));
}

#[test]
#[ignore = "WSL VM 을 깨운다"]
fn real_path_conversion_matches_wslpath() {
    let env = first_real_wsl();
    let wsl = env.remote().list_command().program;
    let distro = env.choice().distro.clone().unwrap();
    let out = std::process::Command::new(wsl).args(["-d", &distro, "-e", "wslpath", "-u", r"C:\Windows"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), env.to_env_path(Path::new(r"C:\Windows")).unwrap());
}

#[test]
#[ignore = "WSL VM 을 깨우고 배포판 안에 claude 가 있어야 한다"]
fn real_spawn_claude_version() {
    let dir = tempfile::tempdir().unwrap();
    // claude 는 `--settings` 로 받은 파일이 없으면 곧바로 종료 코드 1 로 끝난다. 엔진은 늘 파일을 써 두고 넘긴다
    std::fs::write(dir.path().join("settings.json"), "{}").unwrap();
    let env = first_real_wsl();
    let l = ClaudeLaunch {
        session: SessionStart::New("00000000-0000-4000-8000-000000000001".into()),
        cwd: dir.path().to_path_buf(),
        model: String::new(),
        effort: None,
        settings_file: dir.path().join("settings.json"),
        system_prompt_file: None,
        add_dirs: vec![],
        plugin_dir: None,
        env: vec![],
        path_env: vec![],
        extra_args: vec!["--version".into()],
        stderr_log: dir.path().join("stderr.log"),
    };
    let mut child = env.spawn_claude(&l).unwrap();
    let mut lines = Vec::new();
    while let Ok(Some(line)) = child.read_line(Duration::from_secs(60)) {
        lines.push(line);
    }
    println!("{lines:?}");
    let mut exit = None;
    for _ in 0..50 {
        exit = child.try_wait().unwrap();
        if exit.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    child.kill_tree().unwrap();
    let stderr = std::fs::read_to_string(dir.path().join("stderr.log")).unwrap_or_default();
    assert_eq!(exit, Some(0), "stderr: {stderr}");
    assert!(lines.iter().any(|l| l.contains("Claude Code")), "{lines:?} stderr: {stderr}");
}

#[test]
#[ignore = "WSL VM 을 깨우고 배포판 안에 로그인한 claude 가 있어야 한다(haiku 한 턴, 약 0.01 USD)"]
fn real_spawn_claude_one_stream_json_turn() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("settings.json"), "{}").unwrap();
    let env = first_real_wsl();
    let l = ClaudeLaunch {
        session: SessionStart::New("00000000-0000-4000-8000-000000000002".into()),
        cwd: dir.path().to_path_buf(),
        model: "haiku".into(),
        effort: None,
        settings_file: dir.path().join("settings.json"),
        system_prompt_file: None,
        add_dirs: vec![],
        plugin_dir: None,
        env: vec![],
        path_env: vec![],
        extra_args: vec!["--max-turns".into(), "1".into()],
        stderr_log: dir.path().join("stderr.log"),
    };
    let mut child = env.spawn_claude(&l).unwrap();
    child.write_line(r#"{"type":"user","message":{"role":"user","content":"Reply with exactly the word pong and nothing else."}}"#).unwrap();
    let mut lines = Vec::new();
    let mut result = None;
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(120) {
        match child.read_line(Duration::from_secs(5)) {
            Ok(Some(line)) => {
                let done = line.contains(r#""type":"result""#);
                lines.push(line.clone());
                if done {
                    result = Some(line);
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }
    child.kill_tree().unwrap();
    let stderr = std::fs::read_to_string(dir.path().join("stderr.log")).unwrap_or_default();
    let result = result.unwrap_or_else(|| panic!("result 줄이 없다. 받은 줄 {} 개, stderr: {stderr}", lines.len()));
    println!("{result}");
    assert!(result.contains(r#""is_error":false"#) && result.contains("pong"), "{result}");
}
