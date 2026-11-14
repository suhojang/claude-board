//! 중첩 git(하위 모듈) 가리기·막기 규칙(`rules::gitlinks`) 시험.
//! 원본: `Tests/AutopilotRulesTests/GitlinksTests.swift`(표 7줄을 두 시험이 돌고, 멈춤·까닭 시험 각 1개).
//! Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다. 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 Windows 경로·줄바꿈 경로를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::gitlinks::{gitlink_excludes, gitlink_paths, has_submodule, submodule_reason};

struct GitlinkRow {
    note: &'static str,
    ls_files_z: String,
    paths: Vec<&'static str>,
}

// \0 로 나뉜 ls-files -z 출력(항목 끝마다 NUL)
fn z(entries: &[&str]) -> String {
    entries.iter().map(|e| format!("{e}\0")).collect()
}

fn rows() -> Vec<GitlinkRow> {
    vec![
        GitlinkRow { note: "비었으면 없음", ls_files_z: String::new(), paths: vec![] },
        GitlinkRow {
            note: "보통 파일(100644)·실행 파일(100755)·링크(120000)는 gitlink 가 아님",
            ls_files_z: z(&["100644 aaaa 0\ta.txt", "100755 bbbb 0\trun.sh", "120000 cccc 0\tlink"]),
            paths: vec![],
        },
        GitlinkRow { note: "mode 160000 하나", ls_files_z: z(&["100644 aaaa 0\ta.txt", "160000 bbbb 0\tsub"]), paths: vec!["sub"] },
        GitlinkRow {
            note: "보안 검토 1: 경로에 한글·빈칸·슬래시(quotePath=false·-z 라 따옴표·escape 없이 그대로)",
            ls_files_z: z(&["160000 a 0\t모듈/하위 디렉터리", "160000 b 0\tsub dir", "100644 c 0\tx"]),
            paths: vec!["모듈/하위 디렉터리", "sub dir"],
        },
        GitlinkRow {
            note: "보안 검토 1: 경로에 탭이 들어가도 첫 탭 뒤 전부가 경로",
            ls_files_z: z(&["160000 a 0\ta\tb"]),
            paths: vec!["a\tb"],
        },
        GitlinkRow {
            note: "같은 경로가 여러 stage 로 나와도 한 번",
            ls_files_z: z(&["160000 a 1\tsub", "160000 b 2\tsub", "160000 c 3\tsub"]),
            paths: vec!["sub"],
        },
        GitlinkRow {
            note: "160000 으로 시작하는 object 이름에 속지 않음(mode 로만)",
            ls_files_z: z(&["100644 160000abcd 0\thash.txt"]),
            paths: vec![],
        },
    ]
}

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|i| i.to_string()).collect()
}

#[test]
fn 표_길이() {
    assert_eq!(rows().len(), 7, "Swift 표는 7줄이다");
}

#[test]
fn 경로() {
    for r in rows() {
        assert_eq!(gitlink_paths(&r.ls_files_z), r.paths, "{}", r.note);
    }
}

#[test]
fn pathspec() {
    for r in rows() {
        let want: Vec<String> = r.paths.iter().map(|p| format!(":(exclude,literal){p}")).collect();
        assert_eq!(gitlink_excludes(&r.ls_files_z), want, "{}", r.note);
    }
}

#[test]
fn 멈춤() {
    assert!(!has_submodule(&[], false));
    assert!(has_submodule(&s(&["sub"]), false));
    assert!(has_submodule(&[], true), "보안 검토 2: .gitmodules 만 있어도 멈춘다(ignore=none 이 설정을 이기므로)");
    assert!(has_submodule(&s(&["sub"]), true));
}

#[test]
fn 까닭() {
    assert_eq!(submodule_reason(&s(&["sub"]), false), "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(sub))이 있어");
    assert_eq!(submodule_reason(&[], true), "작업 폴더에 하위 모듈(.gitmodules 파일)이 있어");
    assert_eq!(
        submodule_reason(&s(&["a", "b"]), true),
        "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b), .gitmodules 파일)이 있어"
    );
    assert_eq!(
        submodule_reason(&s(&["a", "b", "c", "d"]), false),
        "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b, c 외 1개))이 있어"
    );
}

// ── 추가: Swift 시험에 없는 자리 ─────────────────────────

#[test]
fn extra_windows_path_forms_are_kept_verbatim() {
    // git 은 Windows 에서도 `/` 로 낸다. 글은 해석하지 않으므로 `\`·드라이브 글자가 든 경로는 그대로 남는다
    let got = gitlink_paths(&z(&["160000 a 0\tsub\\inner", "160000 b 0\tC:/x/y", "160000 c 0\tsub/inner"]));
    assert_eq!(got, vec!["sub\\inner", "C:/x/y", "sub/inner"]);
    assert_eq!(gitlink_excludes(&z(&["160000 a 0\tsub\\inner"])), vec![":(exclude,literal)sub\\inner"]);
}

#[test]
fn extra_crlf_and_newline_in_path_are_kept() {
    let got = gitlink_paths(&z(&["160000 a 0\tx\r\ny", "160000 b 0\tz\n"]));
    assert_eq!(got, vec!["x\r\ny", "z\n"]);
}

#[test]
fn extra_malformed_entries_are_skipped() {
    // 탭이 없음, 경로가 빔, NUL 이 이어짐, 끝 NUL 없음
    assert!(gitlink_paths(&z(&["160000 a 0"])).is_empty());
    assert!(gitlink_paths(&z(&["160000 a 0\t"])).is_empty());
    let nul_run = format!("160000 a 0\tp{0}{0}{0}160000 b 0\tq", '\0');
    assert_eq!(gitlink_paths(&nul_run), vec!["p", "q"]);
    // mode 뒤 빈칸이 없으면 gitlink 가 아니다
    assert!(gitlink_paths(&z(&["160000\ta 0\tp"])).is_empty());
}

#[test]
fn extra_reason_exactly_three_has_no_more_suffix() {
    assert_eq!(
        submodule_reason(&s(&["a", "b", "c"]), false),
        "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b, c))이 있어"
    );
    assert_eq!(
        submodule_reason(&s(&["a", "b", "c", "d", "e"]), true),
        "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b, c 외 2개), .gitmodules 파일)이 있어"
    );
}
