//! 앱에 든 기술 스택 버전 목록 글(`rules::builtin_versions`) 시험. 원본: `Sources/AutopilotRules/BuiltinVersions.swift`(전체).
//! Swift 에는 이 파일을 직접 보는 시험이 없다. `VersionsTests.swift` 의 `CatalogTests.앱목록`·`CatalogLoadTests.표` 가 `apBuiltinCatalogData` 를 쓰고,
//! 그 둘은 `tests/rules_versions.rs` 가 같은 이름으로 옮겨 이 상수를 쓴다.
//! 아래는 모두 Swift 시험에 없는 `extra_` 시험이다 — 글자 수·UTF-8 바이트·풀림·다시 써도 같은 글·항목별 추천 하나를 본다.

use claude_board_autopilot::rules::builtin_versions::{builtin_catalog_data, BUILTIN_CATALOG_JSON};
use claude_board_autopilot::rules::versions::{catalog, catalog_data};

/// Swift `apBuiltinCatalogJSON` 의 UTF-8 바이트 수(원본 파일에서 읽어 센 값)
const UTF8_LEN: usize = 1410;

#[test]
fn extra_데이터는_글의_utf8_바이트() {
    assert_eq!(builtin_catalog_data(), BUILTIN_CATALOG_JSON.as_bytes());
    assert_eq!(builtin_catalog_data().len(), UTF8_LEN);
}

#[test]
fn extra_글_앞뒤와_한글_메모() {
    assert!(BUILTIN_CATALOG_JSON.starts_with(r#"{"at":1793286911,"items":{"fastapi":"#));
    assert!(BUILTIN_CATALOG_JSON.ends_with(r#"밖)"}}"#));
    // 한글이라 글자 수와 바이트 수가 다르다
    assert!(BUILTIN_CATALOG_JSON.chars().count() < UTF8_LEN);
}

#[test]
fn extra_목록으로_풀린다() {
    let c = catalog(builtin_catalog_data()).expect("앱에 든 목록은 풀려야 한다");
    assert_eq!(c.at, 1793286911.0);
    assert_eq!(c.items.len(), 13);
    for k in ["java", "spring-boot", "kotlin", "node", "nest", "python", "fastapi", "react", "vite", "next", "vue", "postgresql", "mysql"] {
        assert!(!c.list(k).is_empty(), "항목 {k}");
        assert_eq!(c.list(k).iter().filter(|v| v.rec).count(), 1, "추천 하나 {k}");
    }
    assert_eq!(c.recommended("java"), Some("25"));
    assert_eq!(c.recommended("spring-boot"), Some("4.1.2"));
    assert_eq!(c.notes.get("java").map(String::as_str), Some("목록에 없는 판: Java 27(LTS 가 아니거나 Spring Boot 4.1 의 범위 17 - 26 밖)"));
}

#[test]
fn extra_다시_써도_글이_같다() {
    let c = catalog(builtin_catalog_data()).unwrap();
    assert_eq!(catalog_data(&c), builtin_catalog_data());
}

/// Swift `apBuiltinCatalogJSON` UTF-8 바이트의 FNV-1a 64 비트 값(원본 파일에서 읽어 센 값). 한 바이트만 달라도 달라진다
const FNV1A64: u64 = 0x33b41da620b55d26;

#[test]
fn extra_글자_하나도_다르지_않다() {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in builtin_catalog_data() {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(h, FNV1A64);
}
