// 다시 띄우기 어댑터(작업 스케줄러) 시험.
// 위쪽 시험은 순수 함수만 본다(실제 작업을 만들지 않는다).
// `#[ignore]` 실측은 이 PC 에 진짜 작업을 만들고 반드시 지운다 — 시험 전용 폴더 `FOLDER_TEST` 만 쓴다.
use std::path::PathBuf;

use claude_board_autopilot::adapters::supervisor::*;
use claude_board_autopilot::ports::*;

fn job(label: &str, args: &[&str]) -> KeepAlive {
    KeepAlive {
        label: label.to_string(),
        program: PathBuf::from(r"C:\Program Files\Claude Board\claude-autopilot.exe"),
        args: args.iter().map(|s| s.to_string()).collect(),
        working_dir: PathBuf::from(r"C:\Users\u\AppData\Local\Claude Board\autopilot\runs\r1"),
    }
}

#[test]
fn label_rule() {
    for ok in ["com.claude.board.autopilot.20261105-0900-toy", "a", &"a".repeat(200)] {
        assert!(valid_label(ok), "통과해야 한다: {ok}");
    }
    for bad in ["", " ", "a b", "a/b", r"a\b", &"a".repeat(201), "한글", "a:b"] {
        assert!(!valid_label(bad), "거부해야 한다: {bad:?}");
    }
}

#[test]
fn folder_rule() {
    for ok in [FOLDER_TEST, r"\X\"] {
        assert!(TaskScheduler::with_folder(ok).is_ok(), "통과해야 한다: {ok}");
    }
    for bad in [r"X\", r"\X", r"\a b\", r"\", r"\\", r"\a\\b\"] {
        let err = TaskScheduler::with_folder(bad).expect_err(bad);
        assert_eq!(err.kind, PortErrorKind::Invalid, "{bad}");
    }
    assert_eq!(TaskScheduler::with_folder(FOLDER_TEST).unwrap().folder(), FOLDER_TEST);
}

#[test]
fn task_path_join() {
    assert_eq!(TaskScheduler::new(false).task_path("lab").unwrap(), r"\ClaudeBoard\autopilot\lab");
    assert_eq!(TaskScheduler::new(true).task_path("lab").unwrap(), r"\ClaudeBoard\autopilot-dev\lab");
    let err = TaskScheduler::new(false).task_path("a b").unwrap_err();
    assert_eq!(err.kind, PortErrorKind::Invalid);
}

#[test]
fn user_from_table() {
    assert_eq!(user_from(Some("PC"), Some("u")).as_deref(), Some(r"PC\u"));
    assert_eq!(user_from(None, Some("u")).as_deref(), Some("u"));
    assert_eq!(user_from(Some(""), Some("u")).as_deref(), Some("u"));
    assert_eq!(user_from(Some("PC"), None), None);
    assert_eq!(user_from(Some("PC"), Some("")), None);
}

#[test]
fn local_iso_shape() {
    let s = local_iso(1_800_000_000.0);
    // YYYY-MM-DDTHH:MM:SS — 시간대가 머신마다 달라 값은 보지 않는다
    assert_eq!(s.len(), 19, "{s}");
    for (i, c) in s.chars().enumerate() {
        match i {
            4 | 7 => assert_eq!(c, '-', "{s}"),
            10 => assert_eq!(c, 'T', "{s}"),
            13 | 16 => assert_eq!(c, ':', "{s}"),
            _ => assert!(c.is_ascii_digit(), "{s} 의 {i}번째"),
        }
    }
}

#[test]
fn xml_escape_table() {
    assert_eq!(xml_escape("&"), "&amp;");
    assert_eq!(xml_escape("<"), "&lt;");
    assert_eq!(xml_escape(">"), "&gt;");
    assert_eq!(xml_escape("\""), "&quot;");
    assert_eq!(xml_escape("'"), "&apos;");
    assert_eq!(xml_escape("한글 a b"), "한글 a b");
    assert_eq!(xml_escape("&lt;"), "&amp;lt;");
}

#[test]
fn task_xml_exact() {
    let j = KeepAlive {
        label: "com.claude.board.autopilot.r1".into(),
        program: PathBuf::from(r"C:\Program Files\Claude Board\claude-autopilot.exe"),
        args: vec![
            "run".into(),
            r"C:\Users\u\AppData\Local\Claude Board\autopilot\runs\r1".into(),
            "abc123".into(),
        ],
        working_dir: PathBuf::from(r"C:\Users\u\AppData\Local\Claude Board\autopilot\runs\r1"),
    };
    let expected = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Claude Board 자율주행 엔진 com.claude.board.autopilot.r1. 꺼지면 1분 안에 다시 띄운다</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>PC\u</UserId>
    </LogonTrigger>
    <TimeTrigger>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>2026-11-05T10:00:00</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>PC\u</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>C:\Program Files\Claude Board\claude-autopilot.exe</Command>
      <Arguments>run &quot;C:\Users\u\AppData\Local\Claude Board\autopilot\runs\r1&quot; abc123</Arguments>
      <WorkingDirectory>C:\Users\u\AppData\Local\Claude Board\autopilot\runs\r1</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    .replace('\n', "\r\n");
    let xml = task_xml(&j, r"PC\u", "2026-11-05T10:00:00");
    assert_eq!(xml, expected);
    assert!(xml.ends_with("</Task>\r\n"));
    // 줄 끝은 모두 CRLF — 홀로 선 LF 가 없다
    assert_eq!(xml.matches('\n').count(), xml.matches("\r\n").count());
}

#[test]
fn task_xml_escapes() {
    let mut j = job("lab", &["<x>", "\"q\""]);
    j.program = PathBuf::from(r"C:\A&B\e.exe");
    let xml = task_xml(&j, r"PC\u", "2026-11-05T10:00:00");
    assert!(xml.contains(r"<Command>C:\A&amp;B\e.exe</Command>"), "{xml}");
    // join_windows 는 <x> 를 "<x>" 로, "q" 를 "\"q\"" 로 감싼다 — 그 따옴표도 XML 이스케이프된다
    assert!(xml.contains(r"<Arguments>&quot;&lt;x&gt;&quot; &quot;\&quot;q\&quot;&quot;</Arguments>"), "{xml}");
}

#[test]
fn task_xml_values_are_not_expanded_twice() {
    // 값 안의 {user} 같은 글이 틀 채우기에 다시 풀리지 않는다
    let xml = task_xml(&job("lab", &["{user}"]), r"PC\u", "2026-11-05T10:00:00");
    assert!(xml.contains("<Arguments>{user}</Arguments>"), "{xml}");
}

#[test]
fn task_xml_empty_args() {
    let xml = task_xml(&job("lab", &[]), r"PC\u", "2026-11-05T10:00:00");
    assert!(xml.contains("<Arguments></Arguments>"), "{xml}");
}

#[test]
fn utf16_bom() {
    assert_eq!(utf16le_with_bom("<a"), vec![0xFF, 0xFE, 0x3C, 0x00, 0x61, 0x00]);
    // 한(U+D55C)
    assert_eq!(utf16le_with_bom("한"), vec![0xFF, 0xFE, 0x5C, 0xD5]);
    assert_eq!(utf16le_with_bom(""), vec![0xFF, 0xFE]);
}

#[test]
fn parse_list_table() {
    let csv = "\"\\Microsoft\\Windows\\Foo\\bar\",\"N/A\",\"Ready\"\r\n\
               \"\\ClaudeBoard\\autopilot\\한글작업\",\"N/A\",\"Ready\"\r\n\
               \"\\ClaudeBoard\\autopilot-test\\b\",\"2026-11-05 10:00:00\",\"Running\"\r\n\
               \r\n\
               \"\\CLAUDEBOARD\\AUTOPILOT-TEST\\a\",\"N/A\",\"Ready\"\r\n\
               \"\\ClaudeBoard\\autopilot-test\\a\",\"N/A\",\"Ready\"\r\n\
               \"\\ClaudeBoard\\autopilot-test\\sub\\x\",\"N/A\",\"Ready\"\r\n\
               broken,line,without,quotes\r\n\
               \"\\ClaudeBoard\\autopilot-test\\unclosed";
    assert_eq!(parse_task_list(csv, FOLDER_TEST), vec!["a".to_string(), "b".to_string()]);
    assert_eq!(parse_task_list(csv, FOLDER), vec!["한글작업".to_string()]);
    assert!(parse_task_list("", FOLDER_TEST).is_empty());
}

#[test]
fn parse_list_quote_escape() {
    let csv = "\"\\X\\a\"\"b\",\"N/A\"\r\n";
    assert_eq!(parse_task_list(csv, r"\X\"), vec!["a\"b".to_string()]);
}

#[cfg(windows)]
#[test]
fn create_args_never_overwrite_an_existing_task() {
    let args = create_args(r"\ClaudeBoard\autopilot-test\x", r"C:\t\x.xml");
    assert_eq!(args, ["/create", "/tn", r"\ClaudeBoard\autopilot-test\x", "/xml", r"C:\t\x.xml"]);
    assert!(!args.iter().any(|a| a.eq_ignore_ascii_case("/f")), "/f 는 같은 이름의 작업을 조용히 덮는다");
}

#[test]
fn bad_label_never_reaches_schtasks() {
    let s = TaskScheduler::with_folder(FOLDER_TEST).unwrap();
    assert_eq!(s.register(&job("a b", &[])).unwrap_err().kind, PortErrorKind::Invalid);
    assert_eq!(s.unregister("a/b").unwrap_err().kind, PortErrorKind::Invalid);
    assert_eq!(s.is_registered("").unwrap_err().kind, PortErrorKind::Invalid);
}

#[cfg(not(windows))]
#[test]
fn non_windows_unavailable() {
    let s = TaskScheduler::new(false);
    assert_eq!(s.register(&job("lab", &[])).unwrap_err().kind, PortErrorKind::Unavailable);
    assert_eq!(s.unregister("lab").unwrap_err().kind, PortErrorKind::Unavailable);
    assert_eq!(s.is_registered("lab").unwrap_err().kind, PortErrorKind::Unavailable);
    assert_eq!(s.list().unwrap_err().kind, PortErrorKind::Unavailable);
}

// ── 실측 (#[ignore]) ─────────────────────────────────
// 관리자가 아닌 PowerShell 에서 한 번에 하나씩:
//   cargo test -p claude-board-autopilot --test supervisor -- --ignored --test-threads=1 --nocapture

#[cfg(windows)]
mod measured {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// 탐침: log 파일에 뜬 시각(밀리초)을 한 줄 덧붙이고, 두 번째 인자만큼 잔다
    const PROBE_JS: &str = "var fso = new ActiveXObject(\"Scripting.FileSystemObject\");\r\n\
        var f = fso.OpenTextFile(WScript.Arguments(0), 8, true);\r\n\
        f.WriteLine(new Date().getTime());\r\n\
        f.Close();\r\n\
        if (WScript.Arguments.length > 1) WScript.Sleep(parseInt(WScript.Arguments(1), 10));\r\n";

    fn sys32(exe: &str) -> PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32").join(exe)
    }

    /// 창 없이 명령을 돌려 (종료 코드, stdout+stderr)
    fn run(exe: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> (i32, String) {
        let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().expect("실행 실패");
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    fn schtasks(args: &[&str]) -> (i32, String) {
        run(sys32("schtasks.exe"), args)
    }

    fn now_ms() -> u128 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
    }

    /// 만든 작업을 반드시 치운다 — 패닉 중에도 `Drop` 이 돈다. 몇 번 불려도 해롭지 않다.
    struct Cleanup {
        path: String,
        dir: PathBuf,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = schtasks(&["/end", "/tn", &self.path]);
            let _ = schtasks(&["/delete", "/tn", &self.path, "/f"]);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// 한 시험이 쓰는 것 — 작업 스케줄러, label, 정리 담당, 탐침 파일들
    struct Rig {
        sched: TaskScheduler,
        label: String,
        path: String,
        log: PathBuf,
        probe: PathBuf,
        cleanup: Cleanup,
    }

    fn rig(folder: &str) -> Rig {
        let sched = TaskScheduler::with_folder(folder).unwrap();
        let label = format!("probe-{}-{}", std::process::id(), now_ms());
        let path = sched.task_path(&label).unwrap();
        let dir = std::env::temp_dir().join(format!("cb-probe-{label}"));
        // 정리 담당을 먼저 만든다: 이후 어디서 패닉해도 작업과 폴더가 치워진다
        let cleanup = Cleanup { path: path.clone(), dir: dir.clone() };
        std::fs::create_dir_all(&dir).unwrap();
        let probe = dir.join("probe.js");
        std::fs::write(&probe, PROBE_JS).unwrap();
        Rig { sched, label, path, log: dir.join("probe.log"), probe, cleanup }
    }

    impl Rig {
        /// 창이 뜨지 않는 wscript 로 탐침을 돌리는 작업 정의
        fn job(&self, sleep_ms: Option<u32>) -> KeepAlive {
            let mut args = vec![
                "//B".to_string(),
                "//Nologo".to_string(),
                self.probe.to_string_lossy().into_owned(),
                self.log.to_string_lossy().into_owned(),
            ];
            if let Some(ms) = sleep_ms {
                args.push(ms.to_string());
            }
            KeepAlive {
                label: self.label.clone(),
                program: sys32("wscript.exe"),
                args,
                working_dir: std::env::temp_dir(),
            }
        }

        /// log 의 시각들(밀리초)
        fn stamps(&self) -> Vec<u128> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        }

        /// log 가 n 줄이 될 때까지 최대 max 기다린다
        fn wait_lines(&self, n: usize, max: Duration) -> Vec<u128> {
            let end = Instant::now() + max;
            loop {
                let s = self.stamps();
                if s.len() >= n || Instant::now() >= end {
                    return s;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }

        /// 치운 뒤 작업이 없는지 본다
        fn assert_gone(self) {
            let Rig { sched, label, cleanup, .. } = self;
            drop(cleanup);
            assert!(!sched.is_registered(&label).unwrap(), "치웠는데 작업이 남았다");
            assert!(!sched.list().unwrap().contains(&label), "치웠는데 목록에 남았다");
        }
    }

    fn assert_not_elevated() {
        let (_, groups) = run(sys32("whoami.exe"), &["/groups"]);
        assert!(!groups.contains("S-1-16-12288"), "관리자가 아닌 셸에서 돌려야 한다");
    }

    fn wscript_count() -> usize {
        let (_, out) = run(sys32("tasklist.exe"), &["/fi", "imagename eq wscript.exe", "/fo", "csv", "/nh"]);
        out.lines().filter(|l| l.to_lowercase().contains("wscript.exe")).count()
    }

    #[test]
    #[ignore = "실제 작업을 만든다 — 관리자 아닌 셸에서 --ignored 로"]
    fn measured_register_without_admin() {
        assert_not_elevated();
        let r = rig(FOLDER_TEST);
        let t0 = Instant::now();
        let t0_ms = now_ms();
        match r.sched.register(&r.job(None)) {
            Ok(()) => println!("register Ok, {} ms", t0.elapsed().as_millis()),
            Err(e) => panic!("관리자 없이 register 실패(kind {:?}): {}", e.kind, e.message),
        }
        assert!(r.sched.is_registered(&r.label).unwrap());
        let names = r.sched.list().unwrap();
        println!("list: {names:?}");
        assert!(names.contains(&r.label));

        let (code, xml) = schtasks(&["/query", "/tn", &r.path, "/xml"]);
        assert_eq!(code, 0, "{xml}");
        for needle in [
            "<LogonTrigger>",
            "<Interval>PT1M</Interval>",
            "IgnoreNew",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
        ] {
            assert!(xml.contains(needle), "저장된 XML 에 {needle} 가 없다:\n{xml}");
        }
        // 작업 스케줄러는 기본값(RunLevel LeastPrivilege)을 저장 XML 에서 빼므로, 높은 권한이 아님을 본다
        assert!(!xml.contains("HighestAvailable"), "높은 권한으로 저장됐다:
{xml}");
        println!("저장된 XML 에 LogonTrigger·PT1M·IgnoreNew·PT0S 가 있고 HighestAvailable 은 없다(LeastPrivilege 는 기본값이라 저장 XML 에 안 나온다)");

        let stamps = r.wait_lines(1, Duration::from_secs(20));
        assert!(!stamps.is_empty(), "20초 안에 탐침이 한 번도 뜨지 않았다(/run 지연)");
        println!("register 시작부터 탐침 첫 줄까지 {:.1} 초", (stamps[0].saturating_sub(t0_ms)) as f64 / 1000.0);

        r.sched.unregister(&r.label).unwrap();
        assert!(!r.sched.is_registered(&r.label).unwrap());
        r.assert_gone();
    }

    #[test]
    #[ignore = "실제 작업을 만들고 최대 200초 기다린다"]
    fn measured_repetition_interval() {
        assert_not_elevated();
        let r = rig(FOLDER_TEST);
        let t0_ms = now_ms();
        r.sched.register(&r.job(None)).unwrap_or_else(|e| panic!("register 실패: {e}"));
        let s = r.wait_lines(3, Duration::from_secs(200));
        println!("탐침 시각(register 기준 초): {:?}", s.iter().map(|t| t.saturating_sub(t0_ms) as f64 / 1000.0).collect::<Vec<_>>());
        assert!(s.len() >= 3, "200초 안에 3줄이 모이지 않았다: {} 줄", s.len());
        let first = (s[1] - s[0]) as f64 / 1000.0;
        let second = (s[2] - s[1]) as f64 / 1000.0;
        println!("간격: /run → 첫 반복 {first:.1} 초, 둘째 {second:.1} 초");
        assert!((0.0..=75.0).contains(&first), "첫 간격 {first}");
        assert!((50.0..=75.0).contains(&second), "둘째 간격 {second}");
        r.assert_gone();
    }

    #[test]
    #[ignore = "실제 작업을 만들고 140초 기다린다"]
    fn measured_ignore_new() {
        assert_not_elevated();
        let before = wscript_count();
        let r = rig(FOLDER_TEST);
        r.sched.register(&r.job(Some(150_000))).unwrap_or_else(|e| panic!("register 실패: {e}"));
        std::thread::sleep(Duration::from_secs(140));
        let lines = r.stamps().len();
        println!("140초 동안 탐침이 뜬 횟수: {lines} 줄 (1이어야 IgnoreNew 가 겹침을 막은 것)");
        assert_eq!(lines, 1, "돌고 있는 동안 반복이 새로 띄웠다");
        let during = wscript_count();
        r.assert_gone();
        std::thread::sleep(Duration::from_secs(2));
        let after = wscript_count();
        println!("wscript 수: 시험 전 {before}, 도는 중 {during}, 정리 뒤 {after}");
        assert!(after <= before, "정리 뒤에도 탐침이 남았다(전 {before}, 후 {after})");
    }

    #[test]
    #[ignore = "실제 작업을 만들고 지운다"]
    fn measured_second_register_with_the_same_name_fails_and_keeps_the_first() {
        assert_not_elevated();
        let r = rig(FOLDER_TEST);
        r.sched.register(&r.job(None)).unwrap_or_else(|e| panic!("첫 register 실패: {e}"));
        // 같은 이름에 다른 프로그램 인자를 가진 두 번째 등록은 덮어쓰지 않고 실패한다
        let mut other = r.job(None);
        other.args.push("SECOND-REGISTER-MARKER".to_string());
        let err = r.sched.register(&other).expect_err("같은 이름의 두 번째 등록은 실패해야 한다");
        println!("두 번째 register 오류: {err}");
        assert!(r.sched.is_registered(&r.label).unwrap(), "두 번째 등록이 실패하면서 첫 작업을 지웠다");
        let (code, xml) = schtasks(&["/query", "/tn", &r.path, "/xml"]);
        assert_eq!(code, 0, "{xml}");
        assert!(!xml.contains("SECOND-REGISTER-MARKER"), "첫 작업이 두 번째 정의로 덮였다:\n{xml}");
        r.assert_gone();
    }

    #[test]
    #[ignore = "작업 스케줄러를 읽기만 한다"]
    fn measured_unregister_missing_ok() {
        let s = TaskScheduler::with_folder(FOLDER_TEST).unwrap();
        let label = format!("missing-{}-{}", std::process::id(), now_ms());
        assert!(!s.is_registered(&label).unwrap());
        s.unregister(&label).unwrap();
        println!("없는 작업 unregister Ok, is_registered false");
    }

    /// 이름의 `zz` 는 `--test-threads=1` 에서 맨 뒤에 돌게 하려는 것
    #[test]
    #[ignore = "작업 스케줄러를 읽기만 한다"]
    fn measured_zz_list_empty_after() {
        let s = TaskScheduler::with_folder(FOLDER_TEST).unwrap();
        let left = s.list().unwrap();
        println!("FOLDER_TEST 아래 남은 작업: {left:?}");
        assert!(left.is_empty(), "시험 뒤 남은 작업: {left:?}");
    }
}
