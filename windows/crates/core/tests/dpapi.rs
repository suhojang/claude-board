//! DPAPI 봉인·풀기 — Windows 에서는 왕복과 망가진 입력 거절, 그 밖 OS 에서는 항상 Err.

#[cfg(windows)]
mod windows_only {
    use claude_board_core::dpapi::{protect, unprotect};

    fn round_trip(plain: &[u8]) -> Vec<u8> {
        let sealed = protect(plain).unwrap_or_else(|e| panic!("protect 실패 — 원문 {}바이트: {e}", plain.len()));
        let back = unprotect(&sealed).unwrap_or_else(|e| panic!("unprotect 실패 — 봉인 {}바이트: {e}", sealed.len()));
        assert_eq!(back, plain, "왕복 결과가 원문과 다르다");
        sealed
    }

    #[test]
    fn round_trip_small_and_changes_bytes() {
        let plain = "계정 토큰 sk-ant-oat01-0123456789abcdef".as_bytes();
        let sealed = round_trip(plain);
        assert_ne!(sealed, plain, "봉인한 바이트가 원문 그대로다");
        assert!(sealed.len() > plain.len());
    }

    #[test]
    fn round_trip_empty() {
        let sealed = round_trip(b"");
        assert!(!sealed.is_empty(), "빈 원문의 봉인이 비어 있다");
    }

    #[test]
    fn round_trip_one_mib() {
        let plain: Vec<u8> = (0..=255u8).cycle().take(1 << 20).collect();
        round_trip(&plain);
    }

    #[test]
    fn unprotect_rejects_foreign_bytes() {
        assert!(unprotect(b"").is_err(), "빈 입력이 풀렸다");
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for n in [1usize, 16, 300, 1000] {
            let junk: Vec<u8> = (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect();
            assert!(unprotect(&junk).is_err(), "무작위 {n}바이트가 풀렸다");
        }
    }

    #[test]
    fn unprotect_rejects_damaged_real_blob() {
        let plain = "계정 토큰 sk-ant-oat01-0123456789abcdef ".repeat(30);
        let sealed = round_trip(plain.as_bytes());
        let n = sealed.len();
        let flip = |i: usize| {
            let mut s = sealed.clone();
            s[i] ^= 0x5a;
            s
        };
        let damaged = [
            ("끝 1바이트 잘림", sealed[..n - 1].to_vec()),
            ("절반으로 잘림", sealed[..n / 2].to_vec()),
            ("앞 16바이트만", sealed[..16].to_vec()),
            ("가운데 1바이트 바뀜", flip(n / 2)),
            ("마지막 바이트 바뀜", flip(n - 1)),
        ];
        for (what, bytes) in damaged {
            assert!(unprotect(&bytes).is_err(), "망가진 봉인({what}, 원래 {n}바이트)이 풀렸다");
        }
    }
}

#[cfg(windows)]
mod entropy {
    use claude_board_core::dpapi::{protect, protect_with_entropy, unprotect, unprotect_with_entropy};

    #[test]
    fn entropy_round_trip_and_changes_bytes() {
        let plain = b"entropy secret 0123456789";
        let sealed = protect_with_entropy(plain, b"ent-a").unwrap();
        assert_ne!(sealed, plain);
        assert_eq!(unprotect_with_entropy(&sealed, b"ent-a").unwrap(), plain);
        let empty = protect_with_entropy(b"", b"ent-a").unwrap();
        assert_eq!(unprotect_with_entropy(&empty, b"ent-a").unwrap(), b"");
    }

    #[test]
    fn other_entropy_or_none_cannot_unprotect() {
        let sealed = protect_with_entropy(b"payload", b"ent-a").unwrap();
        assert!(unprotect_with_entropy(&sealed, b"ent-b").is_err(), "다른 엔트로피로 풀렸다");
        assert!(unprotect_with_entropy(&sealed, b"ent-a2").is_err(), "엔트로피 접두가 같아도 풀렸다");
        assert!(unprotect(&sealed).is_err(), "엔트로피 없이 풀렸다");
        let plain_sealed = protect(b"payload").unwrap();
        assert!(unprotect_with_entropy(&plain_sealed, b"ent-a").is_err(), "엔트로피 없는 봉인이 엔트로피로 풀렸다");
        assert_eq!(unprotect(&plain_sealed).unwrap(), b"payload", "기존 함수 동작이 바뀌었다");
    }

    #[test]
    fn empty_entropy_refused_and_damaged_rejected() {
        assert_eq!(protect_with_entropy(b"x", b"").unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(unprotect_with_entropy(b"x", b"").unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        assert!(unprotect_with_entropy(b"", b"ent-a").is_err());
        let sealed = protect_with_entropy(&b"abc".repeat(50), b"ent-a").unwrap();
        let n = sealed.len();
        let mut flipped = sealed.clone();
        flipped[n / 2] ^= 0x5a;
        assert!(unprotect_with_entropy(&flipped, b"ent-a").is_err());
        assert!(unprotect_with_entropy(&sealed[..n - 1], b"ent-a").is_err());
    }
}

#[cfg(not(windows))]
#[test]
fn non_windows_always_errs() {
    use claude_board_core::dpapi::{protect, protect_with_entropy, unprotect, unprotect_with_entropy};
    assert!(protect(b"x").is_err());
    assert!(unprotect(b"x").is_err());
    assert!(protect_with_entropy(b"x", b"e").is_err());
    assert!(unprotect_with_entropy(b"x", b"e").is_err());
}
