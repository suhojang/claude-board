use claude_board_core::envs::{detect, parse_wsl_list, EnvChoice, EnvKind};

fn utf16le(text: &str, bom: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if bom {
        out.extend([0xFF, 0xFE]);
    }
    for u in text.encode_utf16() {
        out.extend(u.to_le_bytes());
    }
    out
}

#[test]
fn utf16_with_bom() {
    let raw = utf16le("Ubuntu-26.04\r\nDebian\r\n", true);
    assert_eq!(parse_wsl_list(&raw), ["Ubuntu-26.04", "Debian"]);
}

#[test]
fn utf16_without_bom() {
    let raw = utf16le("Ubuntu\r\nopenSUSE_Leap.15\r\n", false);
    assert_eq!(parse_wsl_list(&raw), ["Ubuntu", "openSUSE_Leap.15"]);
}

#[test]
fn lf_only_blank_lines_and_trailing_nul_are_tolerated() {
    let mut raw = utf16le("\r\nUbuntu\n\r\n\r\nDebian", false);
    raw.extend([0, 0, 0, 0]);
    raw.push(0); // 홀수 바이트 꼬리
    assert_eq!(parse_wsl_list(&raw), ["Ubuntu", "Debian"]);
}

#[test]
fn docker_desktop_is_dropped() {
    let raw = utf16le("docker-desktop\r\nUbuntu\r\ndocker-desktop-data\r\n", true);
    assert_eq!(parse_wsl_list(&raw), ["Ubuntu"]);
}

#[test]
fn bad_names_are_rejected() {
    let long = "a".repeat(65);
    let raw = utf16le(&format!("Ubuntu\r\nmy distro\r\n{long}\r\n우분투\r\nok;rm\r\nDebian\r\n"), true);
    assert_eq!(parse_wsl_list(&raw), ["Ubuntu", "Debian"]);
    assert_eq!(parse_wsl_list(&utf16le(&"a".repeat(64), false)), ["a".repeat(64)]);
}

#[test]
fn duplicates_collapse_and_order_is_kept() {
    let raw = utf16le("B\r\nA\r\nB\r\n", false);
    assert_eq!(parse_wsl_list(&raw), ["B", "A"]);
}

#[test]
fn empty_and_garbage_input_gives_nothing() {
    assert!(parse_wsl_list(&[]).is_empty());
    assert!(parse_wsl_list(&[0xFF, 0xFE]).is_empty());
    assert!(parse_wsl_list(&[0x41]).is_empty());
}

#[test]
fn choice_keys_round_trip() {
    for key in ["wsl", "wsl:Ubuntu-26.04", "pwsh", "powershell", "cmd", "git-bash"] {
        let choice = EnvChoice::parse(key).unwrap_or_else(|| panic!("{key} 를 읽지 못했다"));
        assert_eq!(choice.key(), key);
    }
    let wsl = EnvChoice::parse("wsl:Ubuntu-26.04").unwrap();
    assert_eq!((wsl.kind, wsl.distro.as_deref()), (EnvKind::Wsl, Some("Ubuntu-26.04")));
    assert_eq!(EnvChoice::parse("wsl").unwrap().distro, None);
}

#[test]
fn bad_keys_are_rejected() {
    let long = format!("wsl:{}", "a".repeat(65));
    for key in ["wsl:", "wsl:a b", long.as_str(), "wsl:a;b", "bash", "", "pwsh:x"] {
        assert!(EnvChoice::parse(key).is_none(), "{key:?} 가 받아들여졌다");
    }
    assert!(EnvChoice::parse(&format!("wsl:{}", "a".repeat(64))).is_some());
}

#[test]
fn only_wsl_is_sandbox_capable() {
    for kind in EnvKind::ALL {
        assert_eq!(kind.sandbox_capable(), kind == EnvKind::Wsl, "{}", kind.id());
    }
}

#[cfg(not(windows))]
#[test]
fn detect_is_empty_off_windows() {
    assert!(detect().is_empty());
}

#[cfg(windows)]
#[test]
fn detect_returns_existing_programs_in_kind_order() {
    let found = detect();
    for e in &found {
        assert!(e.program.exists(), "{} 의 실행 파일이 없다: {}", e.choice.key(), e.program.display());
        assert_eq!(EnvChoice::parse(&e.choice.key()).as_ref(), Some(&e.choice));
        assert!(e.label.starts_with(e.choice.kind.label()));
    }
    let order: Vec<usize> = found.iter().map(|e| EnvKind::ALL.iter().position(|k| *k == e.choice.kind).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] <= w[1]), "EnvKind::ALL 차례가 아니다");
    // WSL 이 아닌 환경은 한 종류에 한 줄
    for kind in EnvKind::ALL.into_iter().filter(|k| *k != EnvKind::Wsl) {
        assert!(found.iter().filter(|e| e.choice.kind == kind).count() <= 1);
    }
}
