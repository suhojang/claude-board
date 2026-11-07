//! 실행 환경 규칙(`rules::env_policy`) 시험. Windows 전용 규칙이라 Swift 원본이 없다.
//! 끝의 `#[ignore]` 두 시험은 이 PC 의 셸을 실제로 띄워 종료 코드를 잰다(`-- --ignored --nocapture`). WSL 은 띄우지 않는다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_autopilot::model::{ExecEnv, Num, UnsandboxedOk};
use claude_board_autopilot::ports::{CheckRun, EnvReport};
use claude_board_autopilot::rules::env_policy::*;
use claude_board_core::envs::{EnvChoice, EnvKind};
use serde_json::json;

fn choice(key: &str) -> EnvChoice {
    EnvChoice::parse(key).unwrap_or_else(|| panic!("키를 읽지 못함: {key}"))
}

fn rec(key: &str, kind: &str, distro: Option<&str>, ok_at: Option<f64>) -> ExecEnv {
    ExecEnv {
        distro: distro.map(String::from),
        key: key.to_string(),
        kind: kind.to_string(),
        sandbox: None,
        unsandboxed_ok: ok_at.map(|at| UnsandboxedOk { at: Num::from_f64(at), by: "board".to_string() }),
        extra: Default::default(),
    }
}

fn pc() -> Vec<EnvChoice> {
    ["wsl:Ubuntu-26.04", "pwsh", "powershell", "cmd", "git-bash"].map(choice).to_vec()
}

fn run(command: &str, cwd: &str) -> CheckRun {
    CheckRun {
        command: command.to_string(),
        cwd: PathBuf::from(cwd),
        env: Vec::new(),
        path_env: Vec::new(),
        timeout: Duration::from_secs(600),
        output_log: PathBuf::from("output.log"),
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn env_value<'a>(inv: &'a Invocation, name: &str) -> Option<&'a str> {
    inv.env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

#[test]
fn consent_by_kind() {
    for (kind, want) in [
        (EnvKind::Wsl, false),
        (EnvKind::Pwsh, true),
        (EnvKind::PowerShell, true),
        (EnvKind::Cmd, true),
        (EnvKind::GitBash, true),
    ] {
        assert_eq!(needs_unsandboxed_consent(kind), want, "{}", kind.id());
    }
}

#[test]
fn choose_table() {
    let no_ubuntu = vec![choice("pwsh")];
    let rows: Vec<(&str, Option<ExecEnv>, &[EnvChoice], &str)> = vec![
        ("1 기록 없음", None, &[], "실행 환경을 고르지 않았습니다. 출발 확인에서 실행 환경을 고르세요."),
        ("1 key 빈 글", Some(rec("", "", None, None)), &[], "실행 환경을 고르지 않았습니다. 출발 확인에서 실행 환경을 고르세요."),
        ("2 알 수 없는 key", Some(rec("zsh", "zsh", None, None)), &[], "알 수 없는 실행 환경(zsh)입니다."),
        ("2 나쁜 배포판 이름", Some(rec("wsl:a b", "wsl", None, None)), &[], "알 수 없는 실행 환경(wsl:a b)입니다."),
        ("3 kind 어긋남", Some(rec("pwsh", "cmd", None, Some(1.0))), &[], "실행 환경 기록이 어긋납니다(key pwsh, kind cmd)."),
        ("4 배포판 없는 wsl", Some(rec("wsl", "wsl", None, None)), &[], "WSL 배포판을 고르세요."),
        (
            "5 distro 어긋남",
            Some(rec("wsl:Ubuntu-26.04", "wsl", Some("Debian"), None)),
            &[],
            "실행 환경 기록이 어긋납니다(key wsl:Ubuntu-26.04, distro Debian).",
        ),
        (
            "5 키에 배포판이 없는데 distro 가 있음",
            Some(rec("pwsh", "pwsh", Some("Debian"), Some(1.0))),
            &[],
            "실행 환경 기록이 어긋납니다(key pwsh, distro Debian).",
        ),
        ("6 PC 에 없음(wsl)", Some(rec("wsl:Ubuntu-26.04", "wsl", None, None)), &no_ubuntu, "이 PC 에 없는 실행 환경(WSL · Ubuntu-26.04)입니다."),
        ("6 PC 에 없음(cmd)", Some(rec("cmd", "cmd", None, Some(1.0))), &no_ubuntu, "이 PC 에 없는 실행 환경(명령 프롬프트)입니다."),
        (
            "7 동의 없음",
            Some(rec("pwsh", "pwsh", None, None)),
            &no_ubuntu,
            "PowerShell 7 은(는) 샌드박스 없이 돕니다. 출발 확인에서 동의해야 출발합니다.",
        ),
    ];
    for (note, record, available, want) in rows {
        assert_eq!(choose(record.as_ref(), available), Err(want.to_string()), "{note}");
    }

    let pc = pc();
    assert_eq!(choose(Some(&rec("wsl:Ubuntu-26.04", "wsl", None, None)), &pc), Ok(choice("wsl:Ubuntu-26.04")));
    assert_eq!(choose(Some(&rec("wsl:Ubuntu-26.04", "wsl", Some("Ubuntu-26.04"), None)), &pc), Ok(choice("wsl:Ubuntu-26.04")));
    assert_eq!(choose(Some(&rec("cmd", "cmd", None, Some(1_761_000_000.0))), &pc), Ok(choice("cmd")));
    assert_eq!(choose(Some(&rec("git-bash", "git-bash", None, Some(1.0))), &pc), Ok(choice("git-bash")));
}

#[test]
fn choose_consent_at_zero_is_no_consent() {
    let pc = pc();
    for at in [0.0, -5.0] {
        assert_eq!(
            choose(Some(&rec("cmd", "cmd", None, Some(at))), &pc),
            Err("명령 프롬프트 은(는) 샌드박스 없이 돕니다. 출발 확인에서 동의해야 출발합니다.".to_string()),
            "at {at}"
        );
    }
    // 숫자로 읽히지 않는 at 도 동의가 아니다
    let mut bad = rec("cmd", "cmd", None, Some(1.0));
    bad.unsandboxed_ok = Some(UnsandboxedOk { at: serde_json::from_value(json!("언젠가")).unwrap(), by: "cli".to_string() });
    assert!(choose(Some(&bad), &pc).is_err());
}

#[test]
fn confirm_ready_table() {
    let report = |ready: bool, sandbox: bool, problems: &[&str]| EnvReport {
        ready,
        sandbox,
        claude_version: None,
        problems: strs(problems),
    };
    let wsl = choice("wsl:Ubuntu-26.04");
    assert_eq!(
        confirm_ready(&wsl, &report(false, false, &["claude 가 없습니다", "로그인이 필요합니다"])),
        Err("실행 환경을 쓸 수 없습니다: claude 가 없습니다; 로그인이 필요합니다".to_string())
    );
    assert_eq!(confirm_ready(&wsl, &report(false, false, &[])), Err("실행 환경을 쓸 수 없습니다: 까닭 모름".to_string()));
    assert_eq!(
        confirm_ready(&wsl, &report(true, false, &[])),
        Err("WSL 에서 Claude Code 샌드박스를 걸 수 없어 출발하지 않습니다(WSL2 인지, bubblewrap·socat 이 깔렸는지 확인하세요).".to_string())
    );
    assert_eq!(confirm_ready(&wsl, &report(true, true, &[])), Ok(true));
    assert_eq!(confirm_ready(&choice("pwsh"), &report(true, true, &[])), Ok(false));
    assert_eq!(confirm_ready(&choice("cmd"), &report(true, false, &[])), Ok(false));
}

#[test]
fn record_for_fields() {
    let consent = || UnsandboxedOk { at: Num::from_f64(1_761_000_000.0), by: "board".to_string() };

    let wsl = record_for(&choice("wsl:Ubuntu-26.04"), true, Some(consent()));
    assert_eq!(wsl.key, "wsl:Ubuntu-26.04");
    assert_eq!(wsl.kind, "wsl");
    assert_eq!(wsl.distro.as_deref(), Some("Ubuntu-26.04"));
    assert_eq!(wsl.sandbox, Some(true));
    assert_eq!(wsl.unsandboxed_ok, None, "WSL 은 동의를 주어도 담지 않는다");
    assert!(wsl.extra.is_empty());
    assert_eq!(serde_json::to_string(&wsl).unwrap(), r#"{"distro":"Ubuntu-26.04","key":"wsl:Ubuntu-26.04","kind":"wsl","sandbox":true}"#);

    let cmd = record_for(&choice("cmd"), false, Some(consent()));
    assert_eq!(cmd.key, "cmd");
    assert_eq!(cmd.distro, None);
    assert_eq!(cmd.sandbox, Some(false));
    assert_eq!(cmd.unsandboxed_ok, Some(consent()));
    assert!(cmd.extra.is_empty());
    assert_eq!(
        serde_json::to_string(&cmd).unwrap(),
        r#"{"key":"cmd","kind":"cmd","sandbox":false,"unsandboxed_ok":{"at":1761000000,"by":"board"}}"#
    );

    assert_eq!(record_for(&choice("git-bash"), false, None).unsandboxed_ok, None);
}

#[test]
fn sandbox_block_by_kind() {
    let wsl = sandbox_block(EnvKind::Wsl);
    assert_eq!(
        serde_json::to_string(&wsl).unwrap(),
        r#"{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false,"autoAllowBashIfSandboxed":true}"#
    );
    for kind in [EnvKind::Pwsh, EnvKind::PowerShell, EnvKind::Cmd, EnvKind::GitBash] {
        assert_eq!(serde_json::to_string(&sandbox_block(kind)).unwrap(), r#"{"enabled":false}"#, "{}", kind.id());
    }
}

#[test]
fn validate_check_table() {
    let rows: [(EnvKind, &str, Result<(), &str>); 10] = [
        (EnvKind::Pwsh, "", Err("판정 명령이 비었습니다.")),
        (EnvKind::Pwsh, "  \t ", Err("판정 명령이 비었습니다.")),
        (EnvKind::Wsl, " \n ", Err("판정 명령이 비었습니다.")),
        (EnvKind::GitBash, "make\0test", Err("판정 명령에 NUL 글자가 있습니다.")),
        (EnvKind::Cmd, "echo a\nexit /b 1", Err("명령 프롬프트 판정 명령은 한 줄이어야 합니다.")),
        (EnvKind::Cmd, "echo a\rexit /b 1", Err("명령 프롬프트 판정 명령은 한 줄이어야 합니다.")),
        (EnvKind::Cmd, "echo a & exit /b 1", Ok(())),
        (EnvKind::Pwsh, "$a = 1\nexit $a", Ok(())),
        (EnvKind::PowerShell, "Get-Item .\r\nexit 0", Ok(())),
        (EnvKind::GitBash, "make\nmake test", Ok(())),
    ];
    for (kind, command, want) in rows {
        assert_eq!(validate_check(kind, command), want.map_err(String::from), "{} {command:?}", kind.id());
    }
}

#[test]
fn wsl_cd_table() {
    let d = "Ubuntu-26.04";
    let ok = |p: &str, want: &str| assert_eq!(wsl_cd_arg(Path::new(p), d), Ok(want.to_string()), "{p}");
    ok(r"C:\work\toy", r"C:\work\toy");
    ok("c:/work", "c:/work");
    ok(r"\\wsl.localhost\Ubuntu-26.04\home\u\p", "/home/u/p");
    ok(r"\\WSL$\ubuntu-26.04\home\", "/home");
    ok(r"\\wsl.localhost\Ubuntu-26.04", "/");
    // 앞 구분자가 하나여도 같은 뜻으로 읽는다
    ok(r"\wsl.localhost\Ubuntu-26.04\home\u\p", "/home/u/p");
    ok("//wsl$/Ubuntu-26.04/a//b/", "/a/b");
    // fs::canonicalize 가 돌려주는 \\?\UNC\ 꼴도 같은 경로다
    ok(r"\\?\UNC\wsl.localhost\Ubuntu-26.04\home\u\p", "/home/u/p");

    let err = |p: &str, want: &str| assert_eq!(wsl_cd_arg(Path::new(p), d), Err(want.to_string()), "{p}");
    err(r"\\wsl.localhost\Debian\home", "작업 폴더가 다른 WSL 배포판(Debian)에 있습니다.");
    err(r"work\toy", r"WSL 작업 폴더는 절대 경로여야 합니다: work\toy");
    err(r"\\server\share\x", r"WSL 작업 폴더는 절대 경로여야 합니다: \\server\share\x");
    err(r"\\?\UNC\server\share\x", r"WSL 작업 폴더는 절대 경로여야 합니다: \\server\share\x");
    err("C:", "WSL 작업 폴더는 절대 경로여야 합니다: C:");
}

#[test]
fn encoded_vectors() {
    assert_eq!(powershell_encoded(""), "");
    assert_eq!(powershell_encoded("A"), "QQA=");
    assert_eq!(powershell_encoded("ab"), "YQBiAA==");
    assert_eq!(powershell_encoded("가"), "AKw=");
    // 3바이트 나머지 세 가지(0·1·2)를 모두 지난다: "abc" 는 6바이트
    assert_eq!(powershell_encoded("abc"), "YQBiAGMA");
    // 보충 평면 글자는 서로게이트 쌍(4바이트)으로 간다: U+1F600 = D83D DE00
    assert_eq!(powershell_encoded("😀"), "PdgA3g==");
}

#[test]
fn wrapper_text_is_lf_and_ends_with_newline() {
    assert!(!POWERSHELL_WRAPPER.contains('\r'));
    assert!(POWERSHELL_WRAPPER.ends_with("exit 0\n"));
    assert!(POWERSHELL_WRAPPER.starts_with("$ErrorActionPreference = 'Continue'\n"));
    assert_eq!(POWERSHELL_WRAPPER.lines().count(), 15);
}

#[test]
fn wrapper_preloads_core_modules_before_any_other_cmdlet() {
    // 5.1 은 안 불러온 모듈의 cmdlet 을 처음 쓸 때 모듈 경로를 다 훑는다(러너에서 18~60초). 감싸개가 쓰는 cmdlet(New-Object·Out-String)과
    // 판정 명령이 흔히 쓰는 Utility·Management 를 훑기 없이 이름으로 먼저 불러 둬야 한다
    let import = POWERSHELL_WRAPPER.find("Import-Module Microsoft.PowerShell.Utility, Microsoft.PowerShell.Management").expect("Import-Module 줄");
    let first_autoload_cmdlet = ["New-Object", "Out-String", "Write-Output", "Test-Path"].iter().filter_map(|c| POWERSHELL_WRAPPER.find(c)).min().expect("감싸개가 쓰는 cmdlet");
    assert!(import < first_autoload_cmdlet, "모듈을 불러오기 전에 cmdlet 을 쓰면 자동 불러오기가 모듈 경로를 훑는다");
    // 불러오기 실패는 판정 결과를 바꾸지 않는다: 잡아 넘기고, 뒤의 $Error.Clear() 가 기록을 비운다
    assert!(POWERSHELL_WRAPPER.contains("-ErrorAction Stop } catch { }"));
    assert!(import < POWERSHELL_WRAPPER.find("$Error.Clear()").unwrap());
}

#[test]
fn invocation_cmd() {
    let program = Path::new(r"C:\Windows\System32\cmd.exe");
    let mut r = run("echo a & exit /b 3", r"C:\work\toy");
    r.env = vec![("CLAUDE_BOARD_AUTOPILOT".to_string(), "r1".to_string())];
    let inv = check_invocation(&choice("cmd"), program, &r, Some("PATH/l")).unwrap();
    assert_eq!(inv.program, program);
    assert_eq!(inv.args, strs(&["/d", "/s", "/c"]));
    assert_eq!(inv.raw_tail.as_deref(), Some("\"echo a & exit /b 3\""));
    assert_eq!(inv.codec, OutputCodec::Oem);
    assert_eq!(inv.outer_timeout, Duration::from_secs(600));
    assert_eq!(inv.cwd, PathBuf::from(r"C:\work\toy"));
    assert_eq!(inv.env, vec![("CLAUDE_BOARD_AUTOPILOT".to_string(), "r1".to_string())]);
    assert_eq!(env_value(&inv, CHECK_ENV), None);
}

#[test]
fn invocation_powershell_and_pwsh() {
    for key in ["powershell", "pwsh"] {
        let mut r = run("Get-Item .; exit 2", r"C:\work\toy");
        r.env = vec![("A".to_string(), "1".to_string())];
        let inv = check_invocation(&choice(key), Path::new("shell.exe"), &r, None).unwrap();
        let mut want = strs(&["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"]);
        want.push(powershell_encoded(POWERSHELL_WRAPPER));
        assert_eq!(inv.args, want, "{key}");
        assert_eq!(inv.args.len(), 7);
        assert_eq!(inv.raw_tail, None);
        assert_eq!(inv.codec, OutputCodec::Utf8);
        assert_eq!(inv.outer_timeout, Duration::from_secs(600));
        assert_eq!(
            inv.env,
            vec![("A".to_string(), "1".to_string()), (CHECK_ENV.to_string(), "Get-Item .; exit 2".to_string())],
            "{key}"
        );
    }
}

#[test]
fn invocation_git_bash() {
    let inv = check_invocation(&choice("git-bash"), Path::new("bash.exe"), &run("make test\nmake lint", r"C:\work"), None).unwrap();
    assert_eq!(inv.args, strs(&["-lc", "eval \"$CLAUDE_BOARD_CHECK\""]));
    assert_eq!(inv.raw_tail, None);
    assert_eq!(inv.codec, OutputCodec::Utf8);
    assert_eq!(inv.outer_timeout, Duration::from_secs(600));
    assert_eq!(env_value(&inv, CHECK_ENV), Some("make test\nmake lint"));
    assert_eq!(env_value(&inv, "CHERE_INVOKING"), Some("1"));
}

#[test]
fn invocation_wsl() {
    let mut r = run("make test", r"C:\work\toy");
    r.env = vec![
        ("CLAUDE_BOARD_AUTOPILOT".to_string(), "r1".to_string()),
        ("CLAUDE_BOARD_EVIDENCE_DIR".to_string(), r"C:\e".to_string()),
    ];
    r.path_env = strs(&["CLAUDE_BOARD_EVIDENCE_DIR"]);
    let inv = check_invocation(&choice("wsl:Ubuntu-26.04"), Path::new(r"C:\Windows\System32\wsl.exe"), &r, Some("PATH/l:CLAUDE_BOARD_CHECK/p:")).unwrap();
    assert_eq!(
        inv.args,
        strs(&[
            "-d", "Ubuntu-26.04", "--cd", r"C:\work\toy", "-e", "timeout", "-s", "INT", "-k", "20", "600s", "bash", "-lc",
            "eval \"$CLAUDE_BOARD_CHECK\"",
        ])
    );
    assert_eq!(inv.raw_tail, None);
    assert_eq!(inv.codec, OutputCodec::Utf8);
    assert_eq!(inv.outer_timeout, Duration::from_secs(630));
    assert_eq!(
        inv.env,
        vec![
            ("CLAUDE_BOARD_AUTOPILOT".to_string(), "r1".to_string()),
            ("CLAUDE_BOARD_EVIDENCE_DIR".to_string(), r"C:\e".to_string()),
            (CHECK_ENV.to_string(), "make test".to_string()),
            (
                "WSLENV".to_string(),
                "PATH/l:CLAUDE_BOARD_CHECK/u:CLAUDE_BOARD_AUTOPILOT/u:CLAUDE_BOARD_EVIDENCE_DIR/up".to_string()
            ),
        ]
    );

    // 엔진 자신의 WSLENV 가 없으면 우리 목록만, run.env 의 WSLENV 는 목록에서 빠지고 값도 우리 것으로 덮인다
    let mut r2 = run("true", "/ignored");
    r2.cwd = PathBuf::from(r"\\wsl.localhost\Ubuntu-26.04\home\u\p");
    r2.env = vec![("WSLENV".to_string(), "X/u".to_string())];
    let inv2 = check_invocation(&choice("wsl:Ubuntu-26.04"), Path::new("wsl.exe"), &r2, None).unwrap();
    assert_eq!(inv2.args[3], "/home/u/p");
    assert_eq!(env_value(&inv2, "WSLENV"), Some("CLAUDE_BOARD_CHECK/u"));
    assert_eq!(inv2.env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("WSLENV")).count(), 1);
}

#[test]
fn invocation_wsl_errors() {
    let program = Path::new("wsl.exe");
    let wsl = choice("wsl:Ubuntu-26.04");

    let no_distro = EnvChoice { kind: EnvKind::Wsl, distro: None };
    assert_eq!(
        check_invocation(&no_distro, program, &run("true", r"C:\w"), None),
        Err("WSL 배포판을 고르세요.".to_string())
    );

    let mut bad_name = run("true", r"C:\w");
    bad_name.env = vec![("A-B".to_string(), "1".to_string())];
    assert_eq!(
        check_invocation(&wsl, program, &bad_name, None),
        Err("WSL 로 넘길 수 없는 환경 변수 이름입니다: A-B".to_string())
    );
    // 네이티브는 이름을 검사하지 않는다
    assert!(check_invocation(&choice("pwsh"), program, &bad_name, None).is_ok());

    assert_eq!(
        check_invocation(&wsl, program, &run("true", r"\\wsl.localhost\Debian\home"), None),
        Err("작업 폴더가 다른 WSL 배포판(Debian)에 있습니다.".to_string())
    );
    assert_eq!(check_invocation(&wsl, program, &run("  ", r"C:\w"), None), Err("판정 명령이 비었습니다.".to_string()));
}

#[test]
fn invocation_env_dedup() {
    let mut r = run("make test", r"C:\w");
    r.env = vec![
        ("claude_board_check".to_string(), "x".to_string()),
        ("Dup".to_string(), "1".to_string()),
        ("DUP".to_string(), "2".to_string()),
    ];
    for key in ["pwsh", "git-bash", "wsl:Ubuntu-26.04"] {
        let inv = check_invocation(&choice(key), Path::new("sh.exe"), &r, None).unwrap();
        let checks: Vec<_> = inv.env.iter().filter(|(n, _)| n.eq_ignore_ascii_case(CHECK_ENV)).collect();
        assert_eq!(checks, vec![&(CHECK_ENV.to_string(), "make test".to_string())], "{key}");
        let dups: Vec<_> = inv.env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("dup")).collect();
        assert_eq!(dups, vec![&("DUP".to_string(), "2".to_string())], "{key}");
    }
    // WSLENV 목록에는 대소문자만 다른 이름도 한 번만 간다(먼저 나온 글자로)
    let inv = check_invocation(&choice("wsl:Ubuntu-26.04"), Path::new("wsl.exe"), &r, None).unwrap();
    assert_eq!(env_value(&inv, "WSLENV"), Some("CLAUDE_BOARD_CHECK/u:Dup/u"));
}

#[test]
fn timeout_seconds_round_up() {
    let arg = |t: Duration| {
        let mut r = run("true", r"C:\w");
        r.timeout = t;
        check_invocation(&choice("wsl:Ubuntu-26.04"), Path::new("wsl.exe"), &r, None).unwrap().args[10].clone()
    };
    assert_eq!(arg(Duration::from_millis(200)), "1s");
    assert_eq!(arg(Duration::from_secs_f64(600.5)), "601s");
    assert_eq!(arg(Duration::from_secs(600)), "600s");
    assert_eq!(arg(Duration::ZERO), "1s");
}

#[test]
fn wsl_timed_out_codes() {
    assert!(wsl_timed_out(124));
    assert!(wsl_timed_out(137));
    for code in [0, 1, 125, 126, 127, 130, 143, -1] {
        assert!(!wsl_timed_out(code), "{code}");
    }
}

#[test]
fn prerun_matches_table() {
    let cmd = choice("cmd");
    assert_eq!(prerun_matches(None, &cmd), Ok(()));
    assert_eq!(prerun_matches(Some("cmd"), &cmd), Ok(()));
    assert_eq!(
        prerun_matches(Some("pwsh"), &cmd),
        Err("판정 미리 돌리기는 PowerShell 7 에서 했습니다. 지금 고른 명령 프롬프트 으로 다시 돌리세요.".to_string())
    );
    assert_eq!(
        prerun_matches(Some("wsl:Debian"), &choice("wsl:Ubuntu-26.04")),
        Err("판정 미리 돌리기는 WSL · Debian 에서 했습니다. 지금 고른 WSL · Ubuntu-26.04 으로 다시 돌리세요.".to_string())
    );
    assert_eq!(
        prerun_matches(Some("unknown!"), &cmd),
        Err("판정 미리 돌리기는 unknown! 에서 했습니다. 지금 고른 명령 프롬프트 으로 다시 돌리세요.".to_string())
    );
}

// ── 실측(이 PC 에서 셸을 실제로 띄운다. 기본 실행에서는 빠진다) ──────────────

#[cfg(windows)]
mod measured {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    /// `check_invocation` 이 만든 꼴 그대로 띄워 (종료 코드, 표준 출력 글) 을 얻는다
    pub fn launch(inv: &Invocation) -> (Option<i32>, String) {
        let mut cmd = Command::new(&inv.program);
        cmd.args(&inv.args);
        if let Some(tail) = &inv.raw_tail {
            cmd.raw_arg(tail);
        }
        cmd.envs(inv.env.iter().map(|(k, v)| (k, v))).current_dir(&inv.cwd);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let out = cmd.output().expect("셸을 띄우지 못함");
        (out.status.code(), String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn env_path(key: &str) -> Option<PathBuf> {
        std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
    }
}

#[test]
#[ignore]
#[cfg(windows)]
fn measured_powershell_wrapper() {
    let temp = std::env::temp_dir();
    let mut shells: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(root) = measured::env_path("SystemRoot") {
        let p = root.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe");
        if p.is_file() {
            shells.push(("powershell", p));
        }
    }
    let pwsh = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .chain(measured::env_path("ProgramFiles").map(|d| d.join("PowerShell").join("7")))
        .map(|d| d.join("pwsh.exe"))
        .find(|p| p.is_file());
    shells.extend(pwsh.map(|p| ("pwsh", p)));
    assert!(!shells.is_empty(), "PowerShell 이 하나도 없다");

    let cases: [(&str, i32); 8] = [
        ("exit 3", 3),
        ("Write-Error x", 1),
        ("throw 'x'", 1),
        (r"Get-Item C:\no\such\path", 1),
        ("cmd /c exit 0", 0),
        ("Write-Output ok", 0),
        (r#"cmd /c "echo e 1>&2 & exit 0""#, 0),
        ("cmd /c exit 5", 5),
    ];
    let mut misses = Vec::new();
    for (key, program) in &shells {
        println!("[{key}] {}", program.display());
        for (command, want) in cases {
            let mut r = run(command, "");
            r.cwd = temp.clone();
            r.timeout = Duration::from_secs(60);
            let inv = check_invocation(&choice(key), program, &r, None).unwrap();
            let (code, _) = measured::launch(&inv);
            println!("  {:<40} 기대 {want} 실제 {code:?}", command);
            if code != Some(want) {
                misses.push(format!("{key}: {command} 기대 {want} 실제 {code:?}"));
            }
        }
    }
    assert!(misses.is_empty(), "감싸개 규칙이 어긋난 사례: {misses:#?}");
}

#[test]
#[ignore]
#[cfg(windows)]
fn measured_cmd_and_git_bash() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().to_path_buf();
    let mut misses = Vec::new();

    let comspec = measured::env_path("ComSpec").expect("ComSpec 이 없다");
    for (command, want) in [("exit /b 3", 3), ("echo 가", 0)] {
        let mut r = run(command, "");
        r.cwd = cwd.clone();
        let (code, out) = measured::launch(&check_invocation(&choice("cmd"), &comspec, &r, None).unwrap());
        println!("[cmd] {command:<12} 기대 {want} 실제 {code:?} 출력 {out:?}");
        if code != Some(want) {
            misses.push(format!("cmd: {command} 기대 {want} 실제 {code:?}"));
        }
    }

    let bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    if bash.is_file() {
        let mut r = run("exit 4", "");
        r.cwd = cwd.clone();
        let (code, _) = measured::launch(&check_invocation(&choice("git-bash"), &bash, &r, None).unwrap());
        println!("[git-bash] exit 4 기대 4 실제 {code:?}");
        if code != Some(4) {
            misses.push(format!("git-bash: exit 4 실제 {code:?}"));
        }

        // 임시 폴더는 Git Bash 가 /tmp 로 보여 주므로, 드라이브 아래 일반 폴더(이 crate 의 폴더)로 잰다
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut r = run("pwd", "");
        r.cwd = here.clone();
        let (code, out) = measured::launch(&check_invocation(&choice("git-bash"), &bash, &r, None).unwrap());
        // C:\Users\x\p → /c/Users/x/p
        let raw = here.to_string_lossy().replace('\\', "/");
        let want = format!("/{}{}", raw[..1].to_lowercase(), &raw[2..]);
        println!("[git-bash] pwd 종료 {code:?} 출력 {out:?} 기대(POSIX 꼴) {want:?}");
        if code != Some(0) || !out.eq_ignore_ascii_case(&want) {
            misses.push(format!("git-bash: pwd 출력 {out:?} 기대 {want:?}"));
        }
    } else {
        println!("[git-bash] {} 가 없어 건너뜀", bash.display());
    }
    assert!(misses.is_empty(), "어긋난 사례: {misses:#?}");
}
