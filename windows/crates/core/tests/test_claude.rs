// 시험용 claude 판정(`test_claude::decide`) 시험. 보드(src-tauri)와 자율주행 start 가 같은 규칙을 쓰므로 사례는 여기가 소유한다.
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use claude_board_core::paths::HOME_ENV;
use claude_board_core::test_claude::{decide, Decision, CLAUDE_ENV};

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
    move |key| map.get(key).cloned()
}

fn rejected(d: &Decision) -> &str {
    match d {
        Decision::Rejected(why) => why,
        other => panic!("거절이어야 한다: {other:?}"),
    }
}

// 실재하는 절대 경로 둘 — 임시 폴더 안 파일. 시험마다 폴더를 달리해 병렬 실행끼리 겹치지 않는다
fn fixture(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("cb-core-test-claude-{}-{tag}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("claude.exe");
    let script = dir.join("fake-claude-tty.cjs");
    fs::write(&exe, b"").unwrap();
    fs::write(&script, b"").unwrap();
    (dir, exe, script)
}

fn always(_: &Path) -> bool {
    true
}

const HOME: (&str, &str) = (HOME_ENV, "C:\\h");

#[test]
fn unset_is_off() {
    assert_eq!(decide(&env_of(&[]), &always), Decision::Off);
    assert_eq!(decide(&env_of(&[HOME]), &always), Decision::Off);
    assert_eq!(decide(&env_of(&[(CLAUDE_ENV, ""), HOME]), &always), Decision::Off);
}

#[test]
fn inactive_unless_both_vars_are_set() {
    let (_dir, exe, _) = fixture("both");
    let claude = exe.to_str().unwrap();
    let only_claude = decide(&env_of(&[(CLAUDE_ENV, claude)]), &always);
    assert!(rejected(&only_claude).contains(HOME_ENV));
    let empty_home = decide(&env_of(&[(CLAUDE_ENV, claude), (HOME_ENV, "")]), &always);
    assert!(rejected(&empty_home).contains(HOME_ENV));
}

#[test]
fn relative_part_is_rejected() {
    let (_dir, exe, _) = fixture("relative");
    let abs = exe.to_str().unwrap();
    for bad in ["claude.exe", "fake\\claude.exe", ".\\claude.exe", "..\\claude.exe", "\\claude.exe", "C:claude.exe"] {
        let d = decide(&env_of(&[(CLAUDE_ENV, bad), HOME]), &always);
        assert!(rejected(&d).contains("절대 경로"), "{bad}: {d:?}");
        // 앞쪽 인자 자리의 상대 경로도 거절 — 하나라도 어긋나면 통째로 꺼진다
        let joined = format!("{abs}|{bad}");
        let d = decide(&env_of(&[(CLAUDE_ENV, joined.as_str()), HOME]), &always);
        assert!(rejected(&d).contains("절대 경로"), "{joined}: {d:?}");
    }
}

#[test]
fn missing_file_and_empty_piece_and_quote_are_rejected() {
    let (_dir, exe, _) = fixture("missing");
    let abs = exe.to_str().unwrap();
    let ghost = exe.with_file_name("no-such-file.exe");
    let ghost = ghost.to_str().unwrap();
    let missing = decide(&env_of(&[(CLAUDE_ENV, ghost), HOME]), &|p| p.exists());
    assert!(rejected(&missing).contains("없는 파일"));
    let second_missing = format!("{abs}|{ghost}");
    let missing = decide(&env_of(&[(CLAUDE_ENV, second_missing.as_str()), HOME]), &|p| p.exists());
    assert!(rejected(&missing).contains("없는 파일"));
    for bad in [format!("{abs}|"), format!("|{abs}"), format!("{abs}||{abs}")] {
        let d = decide(&env_of(&[(CLAUDE_ENV, bad.as_str()), HOME]), &always);
        assert!(rejected(&d).contains("빈 조각"), "{bad}: {d:?}");
    }
    let quoted = format!("\"{abs}\"");
    let d = decide(&env_of(&[(CLAUDE_ENV, quoted.as_str()), HOME]), &always);
    assert!(rejected(&d).contains("큰따옴표"));
}

#[test]
fn valid_values_keep_order_and_spaces() {
    let (dir, exe, script) = fixture("valid");
    let spaced = dir.join("홍 길동");
    fs::create_dir_all(&spaced).unwrap();
    let tool = spaced.join("claude.exe");
    fs::write(&tool, b"").unwrap();
    let joined = format!("{}|{}", exe.display(), script.display());
    let env = env_of(&[(CLAUDE_ENV, joined.as_str()), HOME]);
    assert_eq!(decide(&env, &|p| p.exists()), Decision::On(vec![exe, script]));
    let one = tool.to_str().unwrap();
    assert_eq!(decide(&env_of(&[(CLAUDE_ENV, one), HOME]), &|p| p.exists()), Decision::On(vec![tool]));
}

#[test]
fn non_unicode_value_is_rejected() {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0043, 0xD800]);
        let env = move |key: &str| match key {
            CLAUDE_ENV => Some(bad.clone()),
            HOME_ENV => Some(OsString::from("C:\\h")),
            _ => None,
        };
        assert!(rejected(&decide(&env, &always)).contains("유니코드"));
    }
}
