// winproc 어댑터 시험. 위쪽은 어디서나 도는 순수 함수, 아래 `windows` 모듈은 진짜 프로세스(node)를 띄운다.
// node 는 PATH 에서 찾는다(없으면 시험이 실패한다 — 이 저장소의 CI 와 개발 PC 에는 늘 있다).

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc;

use claude_board_autopilot::adapters::winproc::*;

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn framed(input: &[u8]) -> Vec<String> {
    let (tx, rx) = mpsc::channel();
    pump_lines(Cursor::new(input.to_vec()), tx);
    rx.iter().map(|b| String::from_utf8(b).unwrap()).collect()
}

#[test]
fn pump_lines_splits_lf_and_crlf() {
    assert_eq!(framed(b"a\r\nb\nc"), ["a", "b", "c"]);
    assert_eq!(framed(b"a\r\n\r\n\nb\r\n"), ["a", "", "", "b"]);
    assert_eq!(framed(b""), Vec::<String>::new());
    // 줄 안의 \r 은 그대로 둔다(끝의 \r 만 뗀다)
    assert_eq!(framed(b"a\rb\r\n"), ["a\rb"]);
    // 끝에 \r 만 남은 조각
    assert_eq!(framed(b"tail\r"), ["tail"]);
}

#[test]
fn pump_lines_handles_long_lines_across_chunks() {
    let long = "가".repeat(20_000);
    let mut input = long.clone().into_bytes();
    input.extend_from_slice(b"\r\nnext\n");
    assert_eq!(framed(&input), [long.as_str(), "next"]);
}

#[test]
fn build_env_keeps_only_the_allow_list() {
    let parent = pairs(&[
        ("Path", r"C:\bin"),
        ("SystemRoot", r"C:\Windows"),
        ("USERPROFILE", r"C:\Users\a"),
        ("ANTHROPIC_API_KEY", "secret"),
        ("PSModulePath", "x"),
        ("GIT_DIR", "x"),
        ("GIT_EXEC_PATH", r"C:\git"),
    ]);
    let env = build_env(&parent, &[]);
    let names: Vec<&str> = env.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Path", "SystemRoot", "USERPROFILE", "GIT_EXEC_PATH", "HOME"]);
    assert_eq!(env.last().unwrap(), &("HOME".to_string(), r"C:\Users\a".to_string()));
}

#[test]
fn build_env_passes_the_macos_names_and_proxy_settings() {
    let parent = pairs(&[
        ("https_proxy", "http://p:8080"),
        ("NO_PROXY", "localhost"),
        ("ALL_PROXY", "http://p:8080"),
        ("NODE_EXTRA_CA_CERTS", r"C:\ca.pem"),
        ("SSL_CERT_FILE", r"C:\ca.pem"),
        ("CLAUDE_CONFIG_DIR", r"C:\cfg"),
        ("JAVA_HOME", r"C:\jdk"),
        ("GRADLE_USER_HOME", r"C:\g"),
        ("LC_ALL", "C"),
        ("SDKMAN_DIR", r"C:\s"),
        ("DOCKER_HOST", "tcp://x"),
        ("NPM_CONFIG_REGISTRY", "x"),
    ]);
    let names: Vec<String> = build_env(&parent, &[]).into_iter().map(|(n, _)| n).collect();
    for want in [
        "https_proxy",
        "NO_PROXY",
        "ALL_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
        "CLAUDE_CONFIG_DIR",
        "JAVA_HOME",
        "GRADLE_USER_HOME",
        "LC_ALL",
        "SDKMAN_DIR",
        "DOCKER_HOST",
    ] {
        assert!(names.iter().any(|n| n == want), "{want} 가 빠졌다: {names:?}");
    }
    assert!(!names.iter().any(|n| n == "NPM_CONFIG_REGISTRY"), "허용 밖은 새지 않는다: {names:?}");
}

#[test]
fn build_env_never_passes_secret_named_variables_even_on_the_allow_list() {
    let parent = pairs(&[
        ("GRADLE_PUBLISH_SECRET", "s"),
        ("GRADLE_ENTERPRISE_ACCESS_KEY", "k"),
        ("SDKMAN_CANDIDATES_API", "ok"),
        ("LC_AUTH_TOKEN", "t"),
        ("HTTPS_PROXY_PASSWORD", "p"),
        ("DOCKER_CONFIG_CREDENTIALS", "c"),
        ("JAVA_HOME", r"C:\jdk"),
        ("GITHUB_TOKEN", "t"),
        ("AWS_SECRET_ACCESS_KEY", "k"),
    ]);
    let names: Vec<String> = build_env(&parent, &[]).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["SDKMAN_CANDIDATES_API", "JAVA_HOME"]);
    // 호출자가 직접 준 쌍은 그대로 간다(env_pass 처럼 사람이 이름을 적은 것)
    let explicit = build_env(&parent, &pairs(&[("GITHUB_TOKEN", "t")]));
    assert!(explicit.contains(&("GITHUB_TOKEN".to_string(), "t".to_string())));
}

#[test]
fn secret_name_rule_matches_the_macos_pattern() {
    for secret in ["TOKEN", "my_token", "A_SECRET_B", "DB_PASSWORD", "PASSWD", "AWS_CREDENTIALS", "PRIVATE_X", "APIKEY", "ANTHROPIC_API_KEY", "SSH_KEY", "KEY_ID", "A_KEY_B", "OAUTH_X"] {
        assert!(is_secret_name(secret), "{secret}");
    }
    for plain in ["PATH", "KEYBOARD", "MONKEY", "HOMEPATH", "JAVA_HOME", "GRADLE_USER_HOME", "TURKEY_X"] {
        assert!(!is_secret_name(plain), "{plain}");
    }
}

#[test]
fn build_env_extra_wins_case_insensitively() {
    let parent = pairs(&[("Path", r"C:\bin"), ("HOME", r"C:\h"), ("USERPROFILE", r"C:\Users\a")]);
    let env = build_env(&parent, &pairs(&[("PATH", r"C:\mine"), ("MY_VAR", "값")]));
    assert_eq!(env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("path")).count(), 1);
    assert!(env.contains(&("PATH".to_string(), r"C:\mine".to_string())));
    assert!(env.contains(&("MY_VAR".to_string(), "값".to_string())));
    // 부모에 HOME 이 있으면 USERPROFILE 로 덮지 않는다
    assert!(env.contains(&("HOME".to_string(), r"C:\h".to_string())));
}

#[test]
fn env_block_is_sorted_and_double_nul_terminated() {
    let block = env_block(&pairs(&[("b", "2"), ("A", "한")])).unwrap();
    let expected: Vec<u16> = "A=한\0b=2\0\0".encode_utf16().collect();
    assert_eq!(block, expected);
    assert_eq!(env_block(&[]).unwrap(), [0, 0]);
}

#[test]
fn env_block_rejects_bad_names() {
    for bad in [("", "x"), ("A=B", "x"), ("A\0", "x"), ("A", "x\0y")] {
        let err = env_block(&pairs(&[bad])).expect_err(&format!("{bad:?}"));
        assert_eq!(err.kind, claude_board_autopilot::ports::PortErrorKind::Invalid);
    }
}

fn spec(program: &str, args: &[&str], raw_tail: Option<&str>) -> SpawnSpec {
    SpawnSpec {
        program: PathBuf::from(program),
        args: args.iter().map(|s| s.to_string()).collect(),
        raw_tail: raw_tail.map(str::to_string),
        cwd: PathBuf::from(r"C:\"),
        env: Vec::new(),
        io: Io::MergedToFile { log: PathBuf::from("x.log") },
    }
}

#[test]
fn command_line_quotes_args_and_keeps_raw_tail() {
    let line = command_line(&spec(r"C:\Program Files\x\cmd.exe", &["/d", "/s", "/c"], Some("\"echo a & exit /b 3\""))).unwrap();
    assert_eq!(line, "\"C:\\Program Files\\x\\cmd.exe\" /d /s /c \"echo a & exit /b 3\"");
    let line = command_line(&spec(r"C:\n\node.exe", &["a b", "q\"x", ""], None)).unwrap();
    assert_eq!(line, "\"C:\\n\\node.exe\" \"a b\" \"q\\\"x\" \"\"");
}

#[test]
fn command_line_rejects_too_long_and_nul() {
    let long = "x".repeat(33_000);
    assert!(command_line(&spec(r"C:\n.exe", &[&long], None)).is_err());
    assert!(command_line(&spec(r"C:\n.exe", &["a\0b"], None)).is_err());
}

#[test]
fn descendants_follow_parent_links_only() {
    use claude_board_core::proc::ProcessTable;
    let table = ProcessTable::from_entries([
        (10, 1, "root.exe".to_string()),
        (11, 10, "child.exe".to_string()),
        (12, 11, "grand.exe".to_string()),
        (13, 10, "child2.exe".to_string()),
        (20, 1, "other.exe".to_string()),
        (21, 20, "otherchild.exe".to_string()),
        // 순환(깨진 표)도 끝난다
        (30, 31, "a.exe".to_string()),
        (31, 30, "b.exe".to_string()),
    ]);
    let mut got = descendants(&table, 10);
    got.sort_unstable();
    assert_eq!(got, [11, 12, 13]);
    assert_eq!(descendants(&table, 12), Vec::<u32>::new());
    let mut cyc = descendants(&table, 30);
    cyc.sort_unstable();
    assert_eq!(cyc, [31]);
}

#[test]
fn descendants_since_skips_children_older_than_their_parent() {
    use claude_board_core::proc::ProcessTable;
    // 14 는 루트(10)보다 먼저 만들어졌는데 표에는 10 의 자식으로 남아 있다(10 의 pid 를 물려받은 경우)
    let table = ProcessTable::from_entries([
        (10, 1, "root.exe".to_string()),
        (11, 10, "child.exe".to_string()),
        (12, 11, "grand.exe".to_string()),
        (13, 10, "stale-child.exe".to_string()),
        (14, 13, "stale-grand.exe".to_string()),
        (15, 11, "older-than-11.exe".to_string()),
        (16, 10, "unreadable.exe".to_string()),
    ]);
    let times = |pid: u32| -> Option<f64> {
        match pid {
            10 => Some(100.0),
            11 => Some(101.0),
            12 => Some(102.0),
            13 => Some(50.0),
            14 => Some(120.0),
            15 => Some(100.5),
            _ => None,
        }
    };
    let mut got = descendants_since(&table, 10, &times);
    got.sort_unstable();
    assert_eq!(got, [11, 12], "옛 자식(13)과 그 아래(14), 부모보다 먼저인 15, 시각을 모르는 16 은 뺀다");
    // 루트의 시각을 모르면 견줄 수 없어 부모 관계만 따른다
    let none = |_: u32| -> Option<f64> { None };
    let mut all = descendants_since(&table, 10, &none);
    all.sort_unstable();
    let mut plain = descendants(&table, 10);
    plain.sort_unstable();
    assert_eq!(all, plain);
}

#[test]
fn strip_verbatim_only_for_drive_paths() {
    use std::path::Path;
    assert_eq!(strip_verbatim(Path::new(r"\\?\C:\a\b")), PathBuf::from(r"C:\a\b"));
    assert_eq!(strip_verbatim(Path::new(r"\\?\UNC\srv\share")), PathBuf::from(r"\\?\UNC\srv\share"));
    assert_eq!(strip_verbatim(Path::new(r"C:\a")), PathBuf::from(r"C:\a"));
    assert_eq!(strip_verbatim(Path::new(r"\\srv\share")), PathBuf::from(r"\\srv\share"));
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use claude_board_autopilot::ports::PortErrorKind;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    const STILL_ACTIVE: u32 = 259;

    fn node() -> PathBuf {
        let path = std::env::var_os("PATH").expect("PATH");
        std::env::split_paths(&path).map(|d| d.join("node.exe")).find(|p| p.is_file()).expect("node.exe 가 PATH 에 있어야 한다")
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

    /// 스크립트 파일을 만들고 `node 스크립트 인자…` 로 띄운다(스트리밍)
    fn run_node(dir: &Path, script: &str, args: &[&str]) -> (WinProcess, PathBuf) {
        let file = dir.join("t.cjs");
        std::fs::write(&file, script).unwrap();
        let log = dir.join("stderr.log");
        let mut all = vec![file.to_string_lossy().into_owned()];
        all.extend(args.iter().map(|s| s.to_string()));
        let spec = SpawnSpec {
            program: node(),
            args: all,
            raw_tail: None,
            cwd: dir.to_path_buf(),
            env: Vec::new(),
            io: Io::Streaming { stderr_log: log.clone() },
        };
        (WinProcess::spawn(&spec).expect("spawn"), log)
    }

    const SPAWN_GRANDCHILD: &str = r#"
const { spawn } = require('child_process');
const c = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'], { stdio: 'ignore' });
console.log(c.pid);
setTimeout(() => {}, 60000);
"#;

    #[test]
    fn kill_tree_ends_the_grandchild_too() {
        let dir = tempfile::tempdir().unwrap();
        let (mut proc, _) = run_node(dir.path(), SPAWN_GRANDCHILD, &[]);
        let grandchild: u32 = proc.read_line(Duration::from_secs(10)).unwrap().expect("손자 pid 줄").trim().parse().unwrap();
        let child = proc.pid();
        assert!(alive(child) && alive(grandchild), "kill 전에는 둘 다 살아 있다");
        proc.kill_tree().unwrap();
        assert!(wait_gone(child), "자식이 끝나야 한다");
        assert!(wait_gone(grandchild), "손자도 끝나야 한다");
        assert_eq!(proc.wait_timeout(Duration::from_secs(5)).unwrap(), Some(1));
        // 이미 끝났어도 다시 불러도 된다
        proc.kill_tree().unwrap();
    }

    #[test]
    fn dropping_the_process_ends_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let (mut proc, _) = run_node(dir.path(), SPAWN_GRANDCHILD, &[]);
        let grandchild: u32 = proc.read_line(Duration::from_secs(10)).unwrap().unwrap().trim().parse().unwrap();
        let child = proc.pid();
        drop(proc);
        assert!(wait_gone(child) && wait_gone(grandchild), "KILL_ON_JOB_CLOSE 로 자손까지 끝나야 한다");
    }

    #[test]
    fn read_line_times_out_then_delivers_then_eof() {
        let dir = tempfile::tempdir().unwrap();
        let script = "setTimeout(() => { process.stdout.write('late\\r\\n'); }, 1500);";
        let (mut proc, _) = run_node(dir.path(), script, &[]);
        let err = proc.read_line(Duration::from_millis(200)).expect_err("아직 줄이 없다");
        assert_eq!(err.kind, PortErrorKind::Timeout);
        assert_eq!(proc.read_line(Duration::from_secs(10)).unwrap().as_deref(), Some("late"));
        assert_eq!(proc.read_line(Duration::from_secs(10)).unwrap(), None, "프로세스가 끝나면 EOF");
        assert_eq!(proc.wait_timeout(Duration::from_secs(5)).unwrap(), Some(0));
    }

    #[test]
    fn stdout_lines_are_framed_for_crlf_and_split_writes() {
        let dir = tempfile::tempdir().unwrap();
        let script = "process.stdout.write('a\\r\\nb'); setTimeout(() => process.stdout.write('c\\r\\n\\r\\nlast'), 300);";
        let (mut proc, _) = run_node(dir.path(), script, &[]);
        let mut got = Vec::new();
        while let Some(line) = proc.read_line(Duration::from_secs(10)).unwrap() {
            got.push(line);
        }
        assert_eq!(got, ["a", "bc", "", "last"]);
    }

    #[test]
    fn write_line_and_close_stdin_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let script = r#"
let buf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\n')) >= 0) { process.stdout.write(buf.slice(0, i).toUpperCase() + '\n'); buf = buf.slice(i + 1); } });
process.stdin.on('end', () => process.stdout.write('bye\n'));
"#;
        let (mut proc, _) = run_node(dir.path(), script, &[]);
        proc.write_line("hello 한글").unwrap();
        assert_eq!(proc.read_line(Duration::from_secs(10)).unwrap().as_deref(), Some("HELLO 한글"));
        proc.close_stdin();
        assert_eq!(proc.read_line(Duration::from_secs(10)).unwrap().as_deref(), Some("bye"));
        assert_eq!(proc.wait_timeout(Duration::from_secs(10)).unwrap(), Some(0));
        assert!(proc.write_line("late").is_err(), "닫은 뒤 쓰기는 오류");
    }

    #[test]
    fn a_huge_first_line_does_not_block_while_the_child_reads_late() {
        let dir = tempfile::tempdir().unwrap();
        // 자식이 2초 쉰 뒤에야 읽기 시작한다. 파이프(64 KB)보다 큰 줄을 보내는 쪽은 그동안 막히면 안 된다
        let script = r#"
setTimeout(() => {
  let n = 0;
  process.stdin.on('data', (d) => { n += d.length; });
  process.stdin.on('end', () => process.stdout.write('got ' + n + '\n'));
}, 2000);
"#;
        let (mut proc, _) = run_node(dir.path(), script, &[]);
        let big = "x".repeat(256 * 1024);
        let started = Instant::now();
        proc.write_line(&big).unwrap();
        proc.write_line("{}").unwrap();
        assert!(started.elapsed() < Duration::from_millis(1500), "쓰기가 자식이 읽기를 기다렸다: {:?}", started.elapsed());
        // 쓰는 동안에도 읽는 쪽은 돈다(루프가 시각 상한과 control 을 볼 수 있다)
        let err = proc.read_line(Duration::from_millis(200)).expect_err("아직 줄이 없다");
        assert_eq!(err.kind, PortErrorKind::Timeout);
        proc.close_stdin();
        let want = 256 * 1024 + 1 + 3;
        assert_eq!(proc.read_line(Duration::from_secs(15)).unwrap().as_deref(), Some(format!("got {want}").as_str()));
        assert_eq!(proc.wait_timeout(Duration::from_secs(10)).unwrap(), Some(0));
    }

    #[test]
    fn stderr_goes_to_the_log_file_not_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let script = "console.error('오류 줄'); console.log('정상 줄');";
        let (mut proc, log) = run_node(dir.path(), script, &[]);
        assert_eq!(proc.read_line(Duration::from_secs(10)).unwrap().as_deref(), Some("정상 줄"));
        assert_eq!(proc.wait_timeout(Duration::from_secs(10)).unwrap(), Some(0));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        while Instant::now() < deadline && !text.contains("오류 줄") {
            std::thread::sleep(Duration::from_millis(50));
            text = std::fs::read_to_string(&log).unwrap_or_default();
        }
        assert!(text.contains("오류 줄"), "stderr 로그: {text:?}");
    }

    #[test]
    fn child_gets_only_allowed_env_plus_caller_pairs() {
        let dir = tempfile::tempdir().unwrap();
        let script = "console.log(JSON.stringify(process.env));";
        let file = dir.path().join("t.cjs");
        std::fs::write(&file, script).unwrap();
        let spec = SpawnSpec {
            program: node(),
            args: vec![file.to_string_lossy().into_owned()],
            raw_tail: None,
            cwd: dir.path().to_path_buf(),
            env: pairs(&[("CB_TEST_VALUE", "값 with spaces"), ("PATHEXT", ".EXE")]),
            io: Io::Streaming { stderr_log: dir.path().join("e.log") },
        };
        let mut proc = WinProcess::spawn(&spec).unwrap();
        let line = proc.read_line(Duration::from_secs(10)).unwrap().unwrap();
        let env: HashMap<String, String> = serde_json::from_str(&line).unwrap();
        let get = |name: &str| env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        assert_eq!(get("CB_TEST_VALUE").as_deref(), Some("값 with spaces"));
        assert_eq!(get("PATHEXT").as_deref(), Some(".EXE"), "호출자 값이 이긴다");
        assert!(get("SystemRoot").is_some() && get("PATH").is_some() && get("HOME").is_some());
        // cargo 가 시험 프로세스에 넣는 변수는 허용 목록 밖이라 물려주지 않는다
        assert!(get("CARGO_PKG_NAME").is_none(), "허용 목록 밖은 새지 않는다: {env:?}");
    }

    #[test]
    fn args_cwd_and_exit_code_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let script = "console.log(JSON.stringify({ argv: process.argv.slice(2), cwd: process.cwd() })); process.exit(7);";
        let (mut proc, _) = run_node(dir.path(), script, &["a b", "q\"x", "한글", "back\\", ""]);
        let line = proc.read_line(Duration::from_secs(10)).unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["argv"], serde_json::json!(["a b", "q\"x", "한글", "back\\", ""]));
        let reported = PathBuf::from(v["cwd"].as_str().unwrap());
        assert_eq!(reported.canonicalize().unwrap(), dir.path().canonicalize().unwrap());
        assert_eq!(proc.wait_timeout(Duration::from_secs(10)).unwrap(), Some(7));
        assert_eq!(proc.try_wait().unwrap(), Some(7), "끝난 뒤 try_wait 도 같은 코드");
    }

    #[test]
    fn try_wait_is_none_while_running_and_wait_timeout_expires() {
        let dir = tempfile::tempdir().unwrap();
        let (mut proc, _) = run_node(dir.path(), "setTimeout(() => {}, 60000);", &[]);
        assert_eq!(proc.try_wait().unwrap(), None);
        let started = Instant::now();
        assert_eq!(proc.wait_timeout(Duration::from_millis(300)).unwrap(), None);
        assert!(started.elapsed() >= Duration::from_millis(250));
        proc.kill_tree().unwrap();
        assert!(proc.wait_timeout(Duration::from_secs(5)).unwrap().is_some());
    }

    #[test]
    fn merged_to_file_collects_stdout_and_stderr_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.cjs");
        std::fs::write(&file, "process.stdout.write('one\\n'); setTimeout(() => { process.stderr.write('two\\n'); setTimeout(() => process.stdout.write('three\\n'), 100); }, 100);").unwrap();
        let log = dir.path().join("out.log");
        let spec = SpawnSpec {
            program: node(),
            args: vec![file.to_string_lossy().into_owned()],
            raw_tail: None,
            cwd: dir.path().to_path_buf(),
            env: Vec::new(),
            io: Io::MergedToFile { log: log.clone() },
        };
        let mut proc = WinProcess::spawn(&spec).unwrap();
        assert_eq!(proc.read_line(Duration::from_millis(50)).unwrap(), None, "읽을 파이프가 없으니 바로 EOF");
        assert_eq!(proc.wait_timeout(Duration::from_secs(10)).unwrap(), Some(0));
        let text = std::fs::read_to_string(&log).unwrap().replace("\r\n", "\n");
        assert_eq!(text, "one\ntwo\nthree\n");
    }

    #[test]
    fn missing_program_and_missing_cwd_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec(r"C:\no\such\program.exe", &[], None);
        s.io = Io::MergedToFile { log: dir.path().join("l.log") };
        s.cwd = dir.path().to_path_buf();
        assert_eq!(WinProcess::spawn(&s).err().expect("없는 프로그램").kind, PortErrorKind::NotFound);

        let mut s = spec(&node().to_string_lossy(), &["-e", "0"], None);
        s.io = Io::MergedToFile { log: dir.path().join("l.log") };
        s.cwd = dir.path().join("no-such-dir");
        assert!(WinProcess::spawn(&s).is_err(), "없는 작업 폴더");
    }

    #[test]
    fn concurrent_spawns_do_not_leak_pipe_ends() {
        // 여러 스레드가 동시에 띄워도 다른 자식의 파이프 끝이 새지 않아, 먼저 끝난 자식의 EOF 가 늦지 않는다
        let dir = tempfile::tempdir().unwrap();
        let slow_dir = dir.path().to_path_buf();
        let slow = std::thread::spawn(move || {
            let (mut p, _) = run_node(&slow_dir, "setTimeout(() => {}, 4000);", &[]);
            let _ = p.wait_timeout(Duration::from_secs(10));
        });
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        let mut handles = Vec::new();
        for i in 0..4 {
            let d = dir.path().join(format!("w{i}"));
            std::fs::create_dir_all(&d).unwrap();
            handles.push(std::thread::spawn(move || {
                let (mut p, _) = run_node(&d, "console.log('x');", &[]);
                let mut n = 0;
                while p.read_line(Duration::from_secs(3)).expect("EOF 가 3초 안에 와야 한다").is_some() {
                    n += 1;
                }
                n
            }));
        }
        for h in handles {
            assert_eq!(h.join().unwrap(), 1);
        }
        assert!(started.elapsed() < Duration::from_secs(4), "느린 자식 때문에 늦어지면 핸들이 샌 것");
        slow.join().unwrap();
    }

    #[test]
    fn decode_oem_decodes_ascii_and_empty() {
        assert_eq!(decode_oem(b""), "");
        assert_eq!(decode_oem(b"abc 123"), "abc 123");
    }
}
