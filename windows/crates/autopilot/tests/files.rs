// 실행 폴더 파일·시각 어댑터 시험. 실제 디스크와 `icacls`·`powershell` 자식 프로세스를 쓴다.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use claude_board_autopilot::adapters::files::*;
use claude_board_autopilot::ports::*;
use serde_json::json;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn write_json_is_pretty_utf8_without_bom_and_replaces_existing() {
    let d = dir();
    let p = d.path().join("run.json");
    let files = FsRunFiles::new();
    files.write_json(&p, &json!({"a": 1, "이름": "한글"})).unwrap();
    files.write_json(&p, &json!({"a": 2, "이름": "바뀜"})).unwrap();

    let bytes = std::fs::read(&p).unwrap();
    assert!(!bytes.starts_with(b"\xEF\xBB\xBF"));
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains('\r'));
    assert_eq!(text, "{\n  \"a\": 2,\n  \"이름\": \"바뀜\"\n}\n");
    // 임시 파일이 남지 않는다
    assert_eq!(files.list(d.path()).unwrap(), vec!["run.json".to_string()]);
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
}

#[test]
fn write_json_missing_parent_is_not_found() {
    let d = dir();
    let err = FsRunFiles::new().write_json(&d.path().join("nope").join("x.json"), &json!({})).unwrap_err();
    assert_eq!(err.kind, PortErrorKind::NotFound);
}

#[test]
fn concurrent_reader_never_sees_partial_file() {
    let d = dir();
    let p = d.path().join("state.json");
    let files = Arc::new(FsRunFiles::new());
    files.write_json(&p, &json!({"n": 0, "pad": ""})).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let reader = {
        let (files, p, stop) = (files.clone(), p.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut reads = 0u32;
            while !stop.load(Ordering::Relaxed) {
                // 파일이 사라지거나(None) 반쯤 쓴(Invalid) 일이 없어야 한다
                let v = files.read_json(&p).expect("읽기 오류").expect("파일이 사라짐");
                let n = v["n"].as_u64().unwrap() as usize;
                assert_eq!(v["pad"].as_str().unwrap().len(), n * 50, "내용이 반쪽");
                reads += 1;
            }
            reads
        })
    };
    for n in 1..=300usize {
        files.write_json(&p, &json!({"n": n, "pad": "x".repeat(n * 50)})).unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    let reads = reader.join().unwrap();
    assert!(reads > 0);
}

#[test]
fn read_json_missing_is_none() {
    let d = dir();
    assert_eq!(FsRunFiles::new().read_json(&d.path().join("no.json")).unwrap(), None);
}

#[test]
fn read_json_strips_bom() {
    let d = dir();
    let p = d.path().join("bom.json");
    std::fs::write(&p, b"\xEF\xBB\xBF{\"k\": \"v\"}").unwrap();
    assert_eq!(FsRunFiles::new().read_json(&p).unwrap(), Some(json!({"k": "v"})));
}

#[test]
fn read_json_invalid_is_invalid() {
    let d = dir();
    let p = d.path().join("bad.json");
    let files = FsRunFiles::new();
    for bad in ["", "{\"a\": ", "not json"] {
        std::fs::write(&p, bad).unwrap();
        assert_eq!(files.read_json(&p).unwrap_err().kind, PortErrorKind::Invalid, "{bad:?}");
    }
}

#[test]
fn append_line_creates_parents_and_adds_newline_without_bom() {
    let d = dir();
    let p = d.path().join("a").join("b").join("events.jsonl");
    let files = FsRunFiles::new();
    files.append_line(&p, "{\"x\":1}").unwrap();
    files.append_line(&p, "둘째").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), "{\"x\":1}\n둘째\n".as_bytes());
}

#[test]
fn append_from_eight_threads_keeps_every_line_intact() {
    let d = dir();
    let p = d.path().join("events.jsonl");
    let files = Arc::new(FsRunFiles::new());
    let pad = "y".repeat(300);
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let (files, p, pad) = (files.clone(), p.clone(), pad.clone());
            std::thread::spawn(move || {
                for i in 0..200 {
                    // 길게 만들어 두 번에 나눠 쓰면 섞이게 한다
                    files.append_line(&p, &format!("t{t}-i{i}-{pad}-end")).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let text = std::fs::read_to_string(&p).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1600);
    let tail = format!("-{pad}-end");
    let mut seen = std::collections::HashSet::new();
    for l in lines {
        let id = l.strip_prefix('t').and_then(|r| r.strip_suffix(tail.as_str())).unwrap_or_else(|| panic!("깨진 줄: {l}"));
        assert!(seen.insert(id.to_string()), "중복 줄: {l}");
    }
    assert_eq!(seen.len(), 1600);
}

#[test]
fn remove_missing_is_ok_and_existing_goes_away() {
    let d = dir();
    let p = d.path().join("x");
    let files = FsRunFiles::new();
    files.remove(&p).unwrap();
    std::fs::write(&p, "1").unwrap();
    files.remove(&p).unwrap();
    assert!(!p.exists());
}

#[test]
fn list_is_sorted_and_missing_dir_is_empty() {
    let d = dir();
    let files = FsRunFiles::new();
    for n in ["b.json", "a.json", "10", "2", "units"] {
        std::fs::write(d.path().join(n), "").unwrap();
    }
    assert_eq!(files.list(d.path()).unwrap(), ["10", "2", "a.json", "b.json", "units"]);
    assert!(files.list(&d.path().join("none")).unwrap().is_empty());
}

#[test]
fn list_hides_leftover_temp_files() {
    let d = dir();
    std::fs::write(d.path().join(".run.json.1.0.tmp"), "").unwrap();
    std::fs::write(d.path().join(".engine.lock"), "").unwrap();
    assert_eq!(FsRunFiles::new().list(d.path()).unwrap(), [".engine.lock"]);
}

#[test]
fn system_clock_is_unix_seconds() {
    let c = SystemClock;
    let t0 = c.now();
    assert!(t0 > 1_700_000_000.0);
    c.sleep(Duration::from_millis(30));
    let dt = c.now() - t0;
    assert!((0.02..5.0).contains(&dt), "{dt}");
}

// ── Windows 전용 ─────────────────────────────────────

#[cfg(windows)]
mod windows_only {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn icacls(args: &[&OsStr]) -> (bool, String) {
        let out = Command::new("icacls").args(args).creation_flags(CREATE_NO_WINDOW).output().unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// 다른 프로세스가 같은 파일 내용 바깥 1바이트(2^40)를 배타로 걸 수 있는가(.NET FileStream.Lock = LockFile)
    fn other_process_can_lock(path: &Path) -> bool {
        let script = format!(
            "$fs=[IO.File]::Open('{}','OpenOrCreate','ReadWrite','ReadWrite'); try {{ $fs.Lock(1099511627776,1); exit 0 }} catch {{ exit 3 }}",
            path.display().to_string().replace('\'', "''")
        );
        let st = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .unwrap();
        match st.code() {
            Some(0) => true,
            Some(3) => false,
            other => panic!("powershell 도우미 실패: {other:?}"),
        }
    }

    #[test]
    fn lock_is_exclusive_in_process_and_across_processes_and_drop_releases() {
        let d = dir();
        let p = d.path().join(".engine.lock");
        let files = FsRunFiles::new();

        let held = files.try_lock(&p).unwrap().expect("처음에는 잡혀야 한다");
        assert!(files.try_lock(&p).unwrap().is_none(), "같은 프로세스의 두 번째");
        assert!(!other_process_can_lock(&p), "다른 프로세스는 걸 수 없어야 한다");

        drop(held);
        assert!(other_process_can_lock(&p), "놓은 뒤에는 다른 프로세스가 걸 수 있어야 한다");
        assert!(files.try_lock(&p).unwrap().is_some(), "놓은 뒤 다시 잡힌다");
    }

    #[test]
    fn lock_does_not_clobber_file_content() {
        let d = dir();
        let p = d.path().join(".engine.lock");
        std::fs::write(&p, "pid 1\n").unwrap();
        let _held = FsRunFiles::new().try_lock(&p).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "pid 1\n");
    }

    fn sddl_of(path: &Path) -> String {
        let d = dir();
        let save = d.path().join("acl.txt");
        let (ok, _) = icacls(&[path.as_os_str(), "/save".as_ref(), save.as_os_str()]);
        assert!(ok, "icacls /save 실패");
        // /save 는 UTF-16LE(BOM 포함)로 쓴다
        let b = std::fs::read(&save).unwrap();
        let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&u)
    }

    fn assert_private(path: &Path) {
        let (ok, text) = icacls(&[path.as_os_str()]);
        assert!(ok);
        // 출력 첫 줄이 경로라 경로 속 Users 와 헷갈리지 않게 뺀다
        let text = text.replace(path.to_str().unwrap(), "");
        for who in ["Everyone", "Users", "Authenticated Users"] {
            assert!(!text.contains(who), "{who} 항목이 있다:\n{text}");
        }
        let user = std::env::var("USERNAME").unwrap();
        assert!(text.contains(&user), "{user} 항목이 없다:\n{text}");
        // 이름이 현지화돼도 SID 약어(WD=모두, BU=Users, AU=인증된 사용자)로 한 번 더 본다
        let sddl = sddl_of(path);
        for sid in [";;;WD)", ";;;BU)", ";;;AU)"] {
            assert!(!sddl.contains(sid), "{sid} 가 있다:\n{sddl}");
        }
        assert!(sddl.contains("D:P"), "상속이 켜져 있다(보호 DACL 아님):\n{sddl}");
    }

    #[test]
    fn make_private_dir_leaves_only_current_user_and_is_idempotent() {
        let d = dir();
        let target = d.path().join("run").join("units");
        let files = FsRunFiles::new();
        files.make_private_dir(&target).unwrap();
        assert_private(&target);
        files.make_private_dir(&target).unwrap();
        assert_private(&target);
        // 비공개가 된 뒤에도 내가 쓰고 읽을 수 있다
        files.write_json(&target.join("engine.json"), &json!({"ok": true})).unwrap();
        assert_eq!(files.read_json(&target.join("engine.json")).unwrap(), Some(json!({"ok": true})));
    }

    #[test]
    fn make_private_dir_strips_existing_broad_grants() {
        let d = dir();
        let target = d.path().join("open");
        std::fs::create_dir(&target).unwrap();
        let (ok, _) = icacls(&[
            target.as_os_str(),
            "/grant".as_ref(),
            "*S-1-1-0:(OI)(CI)R".as_ref(),
            "*S-1-5-32-545:(OI)(CI)R".as_ref(),
        ]);
        assert!(ok);
        assert!(sddl_of(&target).contains(";;;WD)"), "준비가 안 됐다");

        FsRunFiles::new().make_private_dir(&target).unwrap();
        assert_private(&target);
    }
}

#[test]
fn stat_reports_length_and_recent_modified_time_and_none_when_missing() {
    let d = dir();
    let p = d.path().join("starts.log");
    let files = FsRunFiles::new();
    assert_eq!(files.stat(&p).unwrap(), None);
    files.append_line(&p, "12345").unwrap();
    let st = files.stat(&p).unwrap().unwrap();
    assert_eq!(st.len, 6); // 글자 다섯 + 줄 끝
    let now = SystemClock.now();
    assert!((now - st.modified).abs() < 60.0, "수정 시각이 지금 근처여야 한다: {} vs {now}", st.modified);
}

#[test]
fn read_from_returns_only_the_last_max_bytes_after_offset() {
    let d = dir();
    let p = d.path().join("stderr.log");
    let files = FsRunFiles::new();
    assert_eq!(files.read_from(&p, 0, 10).unwrap(), None);
    std::fs::write(&p, b"0123456789").unwrap();
    // 전체
    assert_eq!(files.read_from(&p, 0, 100).unwrap().unwrap(), b"0123456789");
    // 끝에서 max 바이트만
    assert_eq!(files.read_from(&p, 0, 4).unwrap().unwrap(), b"6789");
    // offset 이 max 보다 뒤면 offset 부터
    assert_eq!(files.read_from(&p, 8, 4).unwrap().unwrap(), b"89");
    // offset 이 파일 끝보다 뒤면 빈 값
    assert_eq!(files.read_from(&p, 99, 4).unwrap().unwrap(), b"");
    // max 0 이면 빈 값
    assert_eq!(files.read_from(&p, 0, 0).unwrap().unwrap(), b"");
}

// ── 읽기 한도·종류 ──

#[test]
fn read_json_refuses_a_directory_and_a_file_over_the_limit() {
    let d = dir();
    let files = FsRunFiles::new();
    // 폴더가 result.json 자리에 있다
    let as_dir = d.path().join("result.json");
    std::fs::create_dir(&as_dir).unwrap();
    assert!(files.read_json(&as_dir).is_err());
    // 한도를 넘는 파일은 올바른 JSON 이어도 읽지 않는다
    let big = d.path().join("big.json");
    let body = format!("\"{}\"", "x".repeat(JSON_MAX_BYTES as usize));
    std::fs::write(&big, body).unwrap();
    let err = files.read_json(&big).unwrap_err();
    assert_eq!(err.kind, PortErrorKind::Invalid);
    // 한도 안의 파일은 읽힌다
    let ok = d.path().join("ok.json");
    std::fs::write(&ok, format!("\"{}\"", "x".repeat(1000))).unwrap();
    assert!(files.read_json(&ok).unwrap().is_some());
}
