//! (청크 Critique 0/5 — `apPathKey` 한 함수만의 시험) 경로 키(`rules::path_key`) 시험.
//! 원본: `Sources/AutopilotRules/Critique.swift` 의 `apPathKey`. 그 함수를 직접 부르는 Swift 시험은 없다.
//! `grep -rn apPathKey Tests/` 가 가리키는 곳은 `Tests/AutopilotRulesTests/MultirunTests.swift` 뿐이다:
//! - `MultirunTests.겹침`(`@Test("경로가 겹치는가", arguments: overlapRows)`, 7줄) — `apPathOverlap(apPathKey(a), apPathKey(b), home: "/Users/zz")` 로 부른다. 같은 이름 `겹침` 으로 옮겼다.
//!   아래 `overlap` 은 `rules::multirun::path_overlap`(원본 `apPathOverlap`)이다. 이 표는 `tests/rules_multirun.rs` 의 `겹침` 에도 있다.
//! - `MultirunTests.접기`·`MultirunTests.출발` 은 `apClaimKeys`·`apClaimPaths` 의 시험이라 `tests/rules_multirun.rs` 가 옮겼다. `접기` 의 입력(대소문자·NFD)이 만드는 키만 `extra_` 시험으로 따로 확인한다.
//!
//! 표(`OVERLAP_ROWS`)는 Swift 시험 파일에서 변환 스크립트로 기계가 옮긴 것이다(손으로 다시 치지 않았다): `note`·`a`·`b`·`want` 글자 그대로.
//! `extra_` 시험은 Swift 시험에 없는 것(경계·Windows 판단 — `path_key_windows`)이다.

use claude_board_autopilot::rules::multirun::path_overlap as overlap;
use claude_board_autopilot::rules::path_key::{path_key, path_key_windows};
use unicode_normalization::UnicodeNormalization;

// BEGIN converted overlapRows
const OVERLAP_ROWS: &[(&str, &str, &str, bool)] = &[
    ("같은 저장소", "/w/zz-toy-a", "/w/zz-toy-a", true),
    ("다른 저장소", "/w/zz-toy-a", "/w/zz-toy-b", false),
    ("이름 앞부분만 같으면 겹치지 않는다", "/w/zz-toy", "/w/zz-toy-b", false),
    ("저장소를 품은 폴더", "/w/shop", "/w/shop/shop-core", true),
    ("품은 쪽이 뒤", "/w/shop/shop-web", "/w/shop", true),
    ("홈 폴더는 품은 자리로 보지 않는다", "/users/zz", "/users/zz/w/zz-toy-a", false),
    ("홈 폴더끼리는 같다", "/users/zz", "/users/zz", true),
];
// END converted overlapRows

#[test]
fn 겹침() {
    assert_eq!(OVERLAP_ROWS.len(), 7);
    for (note, a, b, want) in OVERLAP_ROWS {
        assert_eq!(overlap(&path_key(a), &path_key(b), "/Users/zz"), *want, "{note}: {a} vs {b}");
    }
}

// 같은 표를 Windows 키로도 돌린다 — 이 표의 경로는 모두 `/` 뿌리의 소문자라 같은 답이어야 한다(Windows 엔진이 쓰는 함수가 원본 판정을 바꾸지 않는다)
#[test]
fn extra_겹침_표가_windows_키에서도_같다() {
    for (note, a, b, want) in OVERLAP_ROWS {
        let home = path_key_windows("/Users/zz");
        assert_eq!(overlap(&path_key_windows(a), &path_key_windows(b), &home), *want, "{note}: {a} vs {b}");
        assert_eq!(path_key_windows(a), path_key(a), "{note}");
    }
}

// ── path_key (Swift 와 같은 동작) ─────────────────────────────

#[test]
fn extra_표의_소문자_경로는_키가_자기_자신() {
    for (note, a, b, _) in OVERLAP_ROWS {
        assert_eq!(path_key(a), *a, "{note}");
        assert_eq!(path_key(b), *b, "{note}");
    }
}

// `접기` 시험(apClaimKeys)의 입력이 apPathKey 를 지나 만드는 값
#[test]
fn extra_접기_시험_입력의_키() {
    assert_eq!(path_key("/w/ZZ-Toy-A"), "/w/zz-toy-a");
    assert_eq!(path_key("/w/zz-toy-a/"), "/w/zz-toy-a/"); // 끝 「/」는 키가 남긴다(apClaimKeys 가 앞서 뗀다)
    let nfd: String = "/w/zz-토이".nfd().collect(); // Swift decomposedStringWithCanonicalMapping
    assert_ne!(nfd, "/w/zz-토이");
    assert_eq!(path_key(&nfd), "/w/zz-토이"); // NFD 는 NFC 로 합쳐진다
}

#[test]
fn extra_빈_조각_점_끝구분자는_그대로_남는다() {
    assert_eq!(path_key(""), "");
    assert_eq!(path_key("/"), "/");
    assert_eq!(path_key("//A//B/./C/../D/"), "//a//b/./c/../d/");
    assert_eq!(path_key("rel/X"), "rel/x");
}

#[test]
fn extra_보호_자리_이름이_대소문자를_접는다() {
    // 원본 주석의 사례: .GIT · Claude.md · .Autopilot
    assert_eq!(path_key(".GIT/Config"), ".git/config");
    assert_eq!(path_key("Claude.md"), "claude.md");
    assert_eq!(path_key(".Autopilot/Scenario.md"), ".autopilot/scenario.md");
}

#[test]
fn extra_소문자는_글자마다이고_문맥_규칙이_없다() {
    // Swift lowercased() 는 마지막 시그마 문맥 규칙이 없다: ΑΣ → ας 가 아니라 ασ
    assert_eq!(path_key("ΑΣ"), "ασ");
    // 한 글자가 여러 글자로: İ → i + 결합 윗점
    assert_eq!(path_key("\u{130}"), "i\u{307}");
}

#[test]
fn extra_정규화는_조각_안에서_합쳐진다() {
    assert_eq!(path_key("e\u{301}/E\u{301}"), "\u{e9}/\u{e9}");
}

// ── path_key_windows ──────────────────────────────────────────

fn w(p: &str) -> String {
    path_key_windows(p)
}

#[test]
fn extra_windows_역슬래시와_대소문자는_같은_키() {
    assert_eq!(w(r"C:\Users\ZZ\W\Toy"), "c:/users/zz/w/toy");
    assert_eq!(w("c:/users/zz/w/toy"), w(r"C:\Users\ZZ\W\Toy"));
    assert_eq!(w(r"C:/Users\ZZ/W\Toy"), "c:/users/zz/w/toy");
    assert_eq!(w(r"c:\"), "c:");
    assert_eq!(w("C:/"), "c:");
    assert_eq!(w("C:"), "c:");
}

#[test]
fn extra_windows_확장_앞붙이는_떨어진다() {
    assert_eq!(w(r"\\?\C:\Work\Toy"), "c:/work/toy");
    assert_eq!(w(r"\\.\C:\Work\Toy"), "c:/work/toy");
    assert_eq!(w(r"\??\C:\Work\Toy"), "c:/work/toy");
    assert_eq!(w(r"//?/c:/work/toy"), "c:/work/toy");
    assert_eq!(w(r"\\?\UNC\Server\Share\Dir"), "//server/share/dir");
    assert_eq!(w(r"\\Server\Share\Dir"), "//server/share/dir");
    assert_eq!(w(r"\\?\unc\server\share"), w(r"\\SERVER\share"));
}

#[test]
fn extra_windows_끝의_점과_공백은_같은_이름() {
    assert_eq!(w(r"C:\Work\Toy."), "c:/work/toy");
    assert_eq!(w(r"C:\Work\Toy "), "c:/work/toy");
    assert_eq!(w(r"C:\Work\Toy. . "), "c:/work/toy");
    assert_eq!(w(r"C:\Work.\Toy\"), "c:/work/toy");
    assert_eq!(w(r"C:\Work \ Toy"), "c:/work/ toy"); // 앞쪽 공백은 이름의 일부다
    assert_eq!(w(r"C:\Work\...\Toy"), "c:/work/toy"); // 점뿐인 조각은 버린다(더 합치는 쪽)
}

#[test]
fn extra_windows_반복_구분자와_점_조각은_접힌다() {
    assert_eq!(w(r"C:\\Work\\\Toy"), "c:/work/toy");
    assert_eq!(w(r"C:\Work\.\Toy\."), "c:/work/toy");
    assert_eq!(w(r"C:\Work\Other\..\Toy"), "c:/work/toy");
    assert_eq!(w(r"C:\Work\Toy\..\..\Toy"), "c:/toy");
    assert_eq!(w(r"C:\..\..\Work"), "c:/work"); // 뿌리 위로는 못 올라간다
    assert_eq!(w(r"C:\Work\.. \Toy"), "c:/toy"); // `..` 뒤 공백은 떼고 같은 `..`
    assert_eq!(w(r"C:\Work\ ..\Toy"), "c:/work/toy"); // 앞 공백이 붙은 ` ..` 는 점 조각이 아니고, 끝 점을 떼면 빈 조각이라 버린다(더 합치는 쪽)
    assert_eq!(w("/w//ZZ/./x/../y"), "/w/zz/y");
}

#[test]
fn extra_windows_unc_는_공유_아래로_올라가지_않는다() {
    assert_eq!(w(r"\\srv\share\..\..\x"), "//srv/share/x");
    assert_eq!(w(r"\\srv\share"), "//srv/share");
    assert_ne!(w(r"\\srv\share"), w(r"\srv\share")); // UNC 와 드라이브 없는 절대는 다른 자리
}

#[test]
fn extra_windows_대체_데이터_스트림은_본_이름과_같다() {
    assert_eq!(w(r"C:\Work\a.txt::$DATA"), "c:/work/a.txt");
    assert_eq!(w(r"C:\Work\a.txt:evil"), "c:/work/a.txt");
    assert_eq!(w(r"C:\Work\Dir::$INDEX_ALLOCATION\x"), "c:/work/dir/x");
}

#[test]
fn extra_windows_상대_경로와_드라이브_상대() {
    assert_eq!(w(r"Work\Toy\..\.\X"), "work/x");
    assert_eq!(w(r"..\..\Work"), "../../work"); // 뿌리가 없으면 앞쪽 `..` 는 남긴다
    assert_eq!(w(r"c:Work\Toy"), "c:work/toy"); // 드라이브 상대는 절대 경로와 다른 꼴
    assert_ne!(w(r"c:Work"), w(r"c:\Work"));
    assert_eq!(w(""), "");
}

#[test]
fn extra_windows_유니코드_정규화도_접는다() {
    assert_eq!(w("C:\\W\\e\u{301}"), "c:/w/\u{e9}");
}

#[test]
fn extra_windows_키는_두_번_불러도_같다() {
    for p in [
        r"\\?\C:\Work\Toy. ", r"\\?\UNC\Srv\Share\..\X", r"C:\A\..\..\B", r"c:rel\X", "/w//x/./y", r"..\a", "", "/", r"C:\", r"\\srv\share", r"C:\a.txt::$DATA",
    ] {
        assert_eq!(w(&w(p)), w(p), "{p}");
    }
}

// 보안 불변식: 같은 폴더를 가리키는 서로 다른 쓰기 열 가지가 모두 한 키가 되어, 겹침 판정에서 같은 자리로 센다
#[test]
fn extra_windows_같은_폴더의_여러_쓰기가_한_자리로_겹친다() {
    let spellings = [
        r"C:\Work\Toy-A",
        r"c:\work\toy-a",
        "C:/Work/Toy-A",
        r"C:\Work\Toy-A\",
        r"C:\Work\\Toy-A",
        r"C:\Work\.\Toy-A",
        r"C:\Work\Other\..\Toy-A",
        r"C:\Work\Toy-A. ",
        r"\\?\C:\Work\Toy-A",
        r"\\.\C:\WORK\TOY-A",
        r"\??\c:\work\toy-a",
    ];
    let home = w(r"C:\Users\zz");
    for a in spellings {
        for b in spellings {
            assert!(overlap(&w(a), &w(b), &home), "{a} vs {b}");
        }
    }
    // 품은 폴더와 안쪽도 겹침
    assert!(overlap(&w(r"C:\Work"), &w(r"\\?\c:\WORK\Toy-A "), &home));
    // 다른 폴더·이름 앞부분만 같은 폴더는 겹치지 않음
    assert!(!overlap(&w(r"C:\Work\Toy"), &w(r"C:\Work\Toy-B"), &home));
    assert!(!overlap(&w(r"C:\Work\Toy-A"), &w(r"D:\Work\Toy-A"), &home));
    // 홈 폴더는 품은 자리로 보지 않는다(같을 때만 겹침) — 홈도 같은 Windows 키로 접는다
    assert!(!overlap(&w(r"C:\USERS\zz"), &w(r"C:\Users\ZZ\w\toy-a"), &home));
    assert!(overlap(&w(r"C:\USERS\zz\"), &w(r"\\?\C:\Users\ZZ"), &home));
}
