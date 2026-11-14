//! `adapters::secrets` — DPAPI 파일 비밀 저장소. Windows 에서는 왕복·엔트로피·이름 거절·원자적 쓰기·망가진 파일, 그 밖 OS 에서는 Err.

use claude_board_autopilot::adapters::secrets::DpapiSecrets;
use claude_board_autopilot::ports::{PortErrorKind, Secrets};

const TOKEN: &str = "SECRET-TOKEN-0123456789-zzz";

fn names_in(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

#[cfg(windows)]
mod win {
    use super::*;
    use claude_board_autopilot::adapters::secrets::ENTROPY;
    use claude_board_autopilot::rules::connections::ConnSecrets;
    use claude_board_core::dpapi::{protect, protect_with_entropy, unprotect, unprotect_with_entropy};

    fn store() -> (tempfile::TempDir, DpapiSecrets) {
        let d = tempfile::tempdir().unwrap();
        let s = DpapiSecrets::new(d.path().join("secrets"));
        (d, s)
    }

    fn no_leak(text: &str) {
        assert!(!text.contains(TOKEN), "비밀이 글에 있다: {text}");
        assert!(!text.contains("SECRET"), "비밀 조각이 글에 있다: {text}");
    }

    #[test]
    fn round_trip_and_file_is_sealed() {
        let (d, s) = store();
        s.put("atlassian", TOKEN.as_bytes()).unwrap();
        assert_eq!(s.get("atlassian").unwrap().as_deref(), Some(TOKEN.as_bytes()));
        let raw = std::fs::read(d.path().join("secrets").join("atlassian.dpapi")).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"SECRET"), "비밀이 파일에 평문으로 있다");
        s.put("atlassian", b"second").unwrap();
        assert_eq!(s.get("atlassian").unwrap().as_deref(), Some(&b"second"[..]), "덮어쓰기");
        s.put("figma", b"").unwrap();
        assert_eq!(s.get("figma").unwrap().as_deref(), Some(&b""[..]), "빈 비밀");
        assert_eq!(names_in(&d.path().join("secrets")), ["atlassian.dpapi", "figma.dpapi"], "임시 파일이 남았다");
    }

    #[test]
    fn two_names_do_not_collide() {
        let (_d, s) = store();
        s.put("atlassian", b"aaa").unwrap();
        s.put("notion", b"nnn").unwrap();
        s.put("webhook-1", b"w1").unwrap();
        s.put("webhook-10", b"w10").unwrap();
        assert_eq!(s.get("atlassian").unwrap().unwrap(), b"aaa");
        assert_eq!(s.get("notion").unwrap().unwrap(), b"nnn");
        assert_eq!(s.get("webhook-1").unwrap().unwrap(), b"w1");
        assert_eq!(s.get("webhook-10").unwrap().unwrap(), b"w10");
        s.delete("notion").unwrap();
        assert!(s.get("notion").unwrap().is_none());
        assert_eq!(s.get("atlassian").unwrap().unwrap(), b"aaa", "다른 이름이 지워졌다");
    }

    #[test]
    fn entropy_differs_from_board_vault() {
        assert!(!ENTROPY.is_empty());
        let (d, s) = store();
        // 이 어댑터가 쓴 파일은 보드 금고(엔트로피 없음)의 풀기로 풀리지 않는다
        s.put("atlassian", TOKEN.as_bytes()).unwrap();
        let dir = d.path().join("secrets");
        let raw = std::fs::read(dir.join("atlassian.dpapi")).unwrap();
        assert!(unprotect(&raw).is_err(), "금고 풀기로 풀렸다");
        assert_eq!(unprotect_with_entropy(&raw, ENTROPY).unwrap(), TOKEN.as_bytes());
        // 금고가 봉인한 것을 이 어댑터가 읽으면 Err
        std::fs::write(dir.join("notion.dpapi"), protect(TOKEN.as_bytes()).unwrap()).unwrap();
        let e = s.get("notion").unwrap_err();
        assert_eq!(e.kind, PortErrorKind::Invalid);
        no_leak(&e.to_string());
        // 다른 엔트로피로 봉인한 것도 Err
        std::fs::write(dir.join("figma.dpapi"), protect_with_entropy(TOKEN.as_bytes(), b"other").unwrap()).unwrap();
        assert!(s.get("figma").is_err());
        assert!(!s.exists("figma") && !s.exists("notion"), "읽을 수 없는 파일이 있다고 답했다");
        assert!(s.read("figma").is_none());
    }

    #[test]
    fn invalid_names_refused_without_files() {
        let (d, s) = store();
        let long = "a".repeat(300);
        let bad = [
            "..\\x",
            "../x",
            "a:b",
            "C:\\x",
            "con",
            "CON",
            "nul",
            "",
            &long,
            "atlassian ",
            "atlassian\n",
            "figma.dpapi",
            "webhook-0",
            "webhook-01",
            "webhook-1000",
            "..",
            "atlassian\\..\\notion",
            "atlassian:stream",
            "ATLASSIAN",
        ];
        for name in bad {
            for r in [s.put(name, TOKEN.as_bytes()), s.get(name).map(|_| ()), s.delete(name)] {
                let e = r.expect_err(&format!("{name:?} 이 받아졌다"));
                assert_eq!(e.kind, PortErrorKind::Invalid, "{name:?}");
                no_leak(&e.to_string());
                assert!(name.len() < 4 || !e.to_string().contains(name), "거절 글에 이름이 있다");
            }
        }
        assert!(!d.path().join("secrets").exists(), "거절했는데 폴더가 만들어졌다");
        assert!(names_in(d.path()).is_empty(), "거절했는데 파일이 생겼다: {:?}", names_in(d.path()));
        // 폴더가 이미 있어도 아무 파일도 안 생긴다
        std::fs::create_dir(d.path().join("secrets")).unwrap();
        for name in bad {
            let _ = s.put(name, TOKEN.as_bytes());
        }
        assert!(names_in(&d.path().join("secrets")).is_empty());
        assert_eq!(names_in(d.path()), ["secrets"], "폴더 밖에 파일이 생겼다");
        // ConnSecrets 도 같은 문
        assert!(!s.exists("..\\x") && s.read("../x").is_none());
    }

    #[test]
    fn failed_write_keeps_previous_value() {
        let (d, s) = store();
        s.put("atlassian", b"old-value").unwrap();
        let dir = d.path().join("secrets");
        // 대상 이름 자리가 폴더라 이름 바꾸기가 실패한다
        std::fs::create_dir(dir.join("notion.dpapi")).unwrap();
        std::fs::write(dir.join("notion.dpapi").join("keep"), b"x").unwrap();
        let e = s.put("notion", TOKEN.as_bytes()).unwrap_err();
        no_leak(&e.to_string());
        assert!(dir.join("notion.dpapi").join("keep").exists(), "대상이 바뀌었다");
        assert_eq!(names_in(&dir), ["atlassian.dpapi", "notion.dpapi"], "임시 파일이 남았다");
        assert_eq!(s.get("atlassian").unwrap().unwrap(), b"old-value");
    }

    #[test]
    fn locked_target_keeps_previous_value() {
        use std::os::windows::fs::OpenOptionsExt;
        let (d, s) = store();
        s.put("figma", b"old-value").unwrap();
        let dir = d.path().join("secrets");
        // 공유 없이 열어 두면 덮어쓰기가 실패한다(백신·다른 프로세스가 쥔 경우)
        let hold = std::fs::OpenOptions::new().read(true).share_mode(0).open(dir.join("figma.dpapi")).unwrap();
        let e = s.put("figma", TOKEN.as_bytes()).expect_err("잠긴 파일을 덮었다");
        no_leak(&e.to_string());
        drop(hold);
        assert_eq!(s.get("figma").unwrap().unwrap(), b"old-value");
        assert_eq!(names_in(&dir), ["figma.dpapi"], "임시 파일이 남았다");
    }

    #[test]
    fn overwrite_replaces_whole_file_not_in_place() {
        use std::io::Read;
        use std::os::windows::fs::OpenOptionsExt;
        let (d, s) = store();
        s.put("figma", b"old-value").unwrap();
        let path = d.path().join("secrets").join("figma.dpapi");
        let before = std::fs::read(&path).unwrap();
        // 읽는 쪽이 열어 둔 채(지우기 공유 허용) 덮어쓴다: 임시 파일 + 이름 바꾸기면 연 핸들은 옛 내용을 끝까지 본다
        let mut reader = std::fs::OpenOptions::new().read(true).share_mode(7).open(&path).unwrap();
        s.put("figma", b"new-value-that-is-longer").unwrap();
        let mut seen = Vec::new();
        reader.read_to_end(&mut seen).unwrap();
        assert_eq!(seen, before, "옛 파일을 제자리에서 고쳤다(원자적이 아니다)");
        drop(reader);
        assert_eq!(s.get("figma").unwrap().unwrap(), b"new-value-that-is-longer");
    }

    #[test]
    fn delete_missing_ok_and_get_missing_none() {
        let (d, s) = store();
        s.delete("atlassian").unwrap();
        assert!(s.get("atlassian").unwrap().is_none());
        assert!(!d.path().join("secrets").exists(), "읽기·지우기가 폴더를 만들었다");
        s.put("atlassian", b"x").unwrap();
        s.delete("atlassian").unwrap();
        s.delete("atlassian").unwrap();
        assert!(s.get("atlassian").unwrap().is_none());
        assert!(!s.exists("atlassian"));
    }

    #[test]
    fn damaged_file_is_err_never_garbage() {
        let (d, s) = store();
        s.put("atlassian", TOKEN.repeat(5).as_bytes()).unwrap();
        let path = d.path().join("secrets").join("atlassian.dpapi");
        let good = std::fs::read(&path).unwrap();
        let mut cases: Vec<Vec<u8>> = vec![Vec::new(), b"plain text token".to_vec(), good[..good.len() / 2].to_vec(), good[..good.len() - 1].to_vec()];
        let mut flipped = good.clone();
        let mid = flipped.len() / 2;
        flipped[mid] ^= 0x5a;
        cases.push(flipped);
        for (i, bytes) in cases.iter().enumerate() {
            std::fs::write(&path, bytes).unwrap();
            let e = s.get("atlassian").expect_err(&format!("망가진 파일 {i} 이 풀렸다"));
            assert_eq!(e.kind, PortErrorKind::Invalid);
            no_leak(&e.to_string());
            no_leak(&format!("{e:?}"));
            assert!(s.read("atlassian").is_none() && !s.exists("atlassian"));
        }
    }

    #[test]
    fn conn_secrets_follow_original_rules() {
        let (_d, s) = store();
        assert!(s.read("atlassian").is_none() && !s.exists("atlassian"));
        s.put("atlassian", b"tok\r\n").unwrap();
        assert_eq!(s.read("atlassian").as_deref(), Some("tok"), "끝 줄바꿈을 깎는다");
        assert!(s.exists("atlassian"));
        s.put("notion", b"\n").unwrap();
        assert!(s.read("notion").is_none(), "빈 글은 None");
        s.put("figma", &[0xff, 0xfe]).unwrap();
        assert!(s.read("figma").is_none(), "UTF-8 이 아니면 None");
        assert!(s.exists("figma"));
    }

    #[test]
    fn debug_never_shows_secrets() {
        let (_d, s) = store();
        s.put("atlassian", TOKEN.as_bytes()).unwrap();
        no_leak(&format!("{s:?}"));
        let got = s.get("atlassian").unwrap();
        assert!(got.is_some());
    }
}

#[cfg(not(windows))]
#[test]
fn non_windows_put_and_get_err() {
    let d = tempfile::tempdir().unwrap();
    let s = DpapiSecrets::new(d.path().join("secrets"));
    let e = s.put("atlassian", TOKEN.as_bytes()).unwrap_err();
    assert!(!e.to_string().contains(TOKEN));
    assert!(names_in(d.path()).is_empty(), "실패했는데 파일이 생겼다");
    // 파일이 있어도 풀 수 없으니 Err
    std::fs::create_dir(d.path().join("secrets")).unwrap();
    std::fs::write(d.path().join("secrets").join("atlassian.dpapi"), b"x").unwrap();
    assert!(s.get("atlassian").is_err());
    assert!(s.delete("notion").is_ok());
    assert_eq!(s.put("..\\x", b"x").unwrap_err().kind, PortErrorKind::Invalid);
}
