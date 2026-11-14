//! 보드 git 자리 박기·막기 규칙(`rules::board_git`) 시험.
//! 원본: `Tests/AutopilotRulesTests/BoardGitTests.swift`(표 12줄 + `박기`). Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다.
//! 시험 이름은 Swift 와 같다(`바뀜`·`박기`). 표 길이를 세어 줄이 빠지면 실패한다.
//! 끝의 "추가" 절은 Swift 시험에 없는 Windows 경로(`\`·드라이브·UNC)를 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::rules::board_git::{board_git_changed, board_refs_pin, GitRecord};

const 공통: &str = "git 폴더가 출발 때와 달라져";
const 작업: &str = "작업 트리 git 폴더가 출발 때와 달라져";

struct Row {
    note: &'static str,
    common_dir: Option<&'static str>,
    common_ino: Option<&'static str>,
    work_git_dir: Option<&'static str>,
    work_git_ino: Option<&'static str>,
    live_common: Option<&'static str>,
    live_common_ino: Option<&'static str>,
    live_work_ino: Option<&'static str>,
    work_tree_gone: bool,
    changed: Option<&'static str>,
}

// Swift BoardGitRow 의 기본값(workGitDir·workGitIno·liveWorkIno nil, workTreeGone false)
const BASE: Row = Row {
    note: "",
    common_dir: None,
    common_ino: None,
    work_git_dir: None,
    work_git_ino: None,
    live_common: None,
    live_common_ino: None,
    live_work_ino: None,
    work_tree_gone: false,
    changed: None,
};

const BOARD_ROWS: [Row; 12] = [
    Row { note: "기록 없음(옛 실행)은 막지 않음", live_common: Some("/r/.git"), live_common_ino: Some("1:2"), changed: None, ..BASE },
    Row { note: "경로·inode 같음 → 통과", common_dir: Some("/r/.git"), common_ino: Some("1:2"), live_common: Some("/r/.git"), live_common_ino: Some("1:2"), changed: None, ..BASE },
    Row { note: "공통 git 폴더 경로가 다름 → 막음", common_dir: Some("/r/.git"), common_ino: Some("1:2"), live_common: Some("/evil/.git"), live_common_ino: Some("1:2"), changed: Some(공통), ..BASE },
    Row { note: "inode 가 다름(바꿔치기) → 막음", common_dir: Some("/r/.git"), common_ino: Some("1:2"), live_common: Some("/r/.git"), live_common_ino: Some("9:9"), changed: Some(공통), ..BASE },
    Row { note: "지금 공통 폴더를 못 읽음(nil) → 경로가 다른 것으로 막음", common_dir: Some("/r/.git"), common_ino: Some("1:2"), live_common: None, live_common_ino: None, changed: Some(공통), ..BASE },
    Row { note: "출발 때 inode 기록이 없으면 경로만 본다(지금 inode 를 못 읽어도 통과)", common_dir: Some("/r/.git"), common_ino: None, live_common: Some("/r/.git"), live_common_ino: None, changed: None, ..BASE },
    Row { note: "보안 검토 4: inode 를 기록했는데 지금 nil 이면(못 읽음) 다르다고 본다(전에는 nil 이면 통과)", common_dir: Some("/r/.git"), common_ino: Some("1:2"), live_common: Some("/r/.git"), live_common_ino: None, changed: Some(공통), ..BASE },
    Row { note: "보안 검토 4: 따로 작업 트리 git 폴더 inode 가 다름 → 막음", common_dir: Some("/r/.git"), common_ino: Some("1:2"), work_git_dir: Some("/r/.git/worktrees/x"), work_git_ino: Some("3:4"), live_common: Some("/r/.git"), live_common_ino: Some("1:2"), live_work_ino: Some("9:9"), changed: Some(작업), ..BASE },
    Row { note: "따로 작업 트리 git 폴더 inode 가 같음 → 통과", common_dir: Some("/r/.git"), common_ino: Some("1:2"), work_git_dir: Some("/r/.git/worktrees/x"), work_git_ino: Some("3:4"), live_common: Some("/r/.git"), live_common_ino: Some("1:2"), live_work_ino: Some("3:4"), changed: None, ..BASE },
    Row { note: "2026-11-05 진짜 claude 실행: 합친 뒤 따로 작업 트리를 지워 그 git 폴더가 없음(nil) → 공통 git 폴더만 보고 통과(엔진 finish 와 같은 기준)",
          common_dir: Some("/r/.git"), common_ino: Some("1:2"), work_git_dir: Some("/r/.git/worktrees/x"), work_git_ino: Some("3:4"), live_common: Some("/r/.git"), live_common_ino: Some("1:2"), live_work_ino: None,
          work_tree_gone: true, changed: None },
    Row { note: "작업 트리가 지워졌어도 공통 git 폴더 inode 가 다르면 막음", common_dir: Some("/r/.git"), common_ino: Some("1:2"), work_git_dir: Some("/r/.git/worktrees/x"), work_git_ino: Some("3:4"),
          live_common: Some("/r/.git"), live_common_ino: Some("9:9"), live_work_ino: None, work_tree_gone: true, changed: Some(공통) },
    Row { note: "작업 트리가 아직 있는데 그 git 폴더를 못 읽음(nil) → 막음(닫힌 쪽 그대로)", common_dir: Some("/r/.git"), common_ino: Some("1:2"), work_git_dir: Some("/r/.git/worktrees/x"), work_git_ino: Some("3:4"),
          live_common: Some("/r/.git"), live_common_ino: Some("1:2"), live_work_ino: None, changed: Some(작업), ..BASE },
];

fn rec(common_dir: Option<&str>, common_ino: Option<&str>, work_git_dir: Option<&str>, work_git_ino: Option<&str>) -> GitRecord {
    GitRecord {
        common_dir: common_dir.map(str::to_string),
        common_ino: common_ino.map(str::to_string),
        work_git_dir: work_git_dir.map(str::to_string),
        work_git_ino: work_git_ino.map(str::to_string),
    }
}

// Swift `바뀜`: git 폴더가 출발 때와 다르면 막는다
#[test]
fn 바뀜() {
    assert_eq!(BOARD_ROWS.len(), 12, "Swift 표는 12줄");
    for r in &BOARD_ROWS {
        let record = rec(r.common_dir, r.common_ino, r.work_git_dir, r.work_git_ino);
        let got = board_git_changed(&record, r.live_common, r.live_common_ino, r.live_work_ino, r.work_tree_gone);
        assert_eq!(got, r.changed, "{}", r.note);
    }
}

// Swift `박기`: ref·객체를 읽을 때 박는 env: GIT_DIR·GIT_CEILING
#[test]
fn 박기() {
    assert_eq!(board_refs_pin(&rec(None, None, None, None), "/w/repo"), Vec::<String>::new());
    assert_eq!(
        board_refs_pin(&rec(Some("/w/repo/.git"), Some("1:2"), None, None), "/w/repo"),
        ["GIT_DIR=/w/repo/.git", "GIT_CEILING_DIRECTORIES=/w"]
    );
    assert_eq!(
        board_refs_pin(&rec(Some("/.git"), None, None, None), "/repo"),
        ["GIT_DIR=/.git"],
        "뿌리의 부모가 / 이면 GIT_CEILING 을 넣지 않는다"
    );
}

// ── 추가: Windows 경로 ─────────────────────────────────

fn pin(common: &str, root: &str) -> Vec<String> {
    board_refs_pin(&rec(Some(common), None, None, None), root)
}

#[test]
fn 박기_윈도우_경로() {
    // 역슬래시 구분자: 부모를 같은 구분자 꼴로 낸다
    assert_eq!(pin(r"C:\w\repo\.git", r"C:\w\repo"), [r"GIT_DIR=C:\w\repo\.git", r"GIT_CEILING_DIRECTORIES=C:\w"]);
    // 앞 슬래시 꼴 드라이브 경로
    assert_eq!(pin("C:/w/repo/.git", "C:/w/repo"), ["GIT_DIR=C:/w/repo/.git", "GIT_CEILING_DIRECTORIES=C:/w"]);
    // 끝 구분자는 무시
    assert_eq!(pin(r"C:\w\repo\.git", r"C:\w\repo\"), [r"GIT_DIR=C:\w\repo\.git", r"GIT_CEILING_DIRECTORIES=C:\w"]);
    // 저장소가 드라이브 바로 아래: 부모가 드라이브 뿌리라 GIT_CEILING 을 넣지 않는다
    assert_eq!(pin(r"C:\repo\.git", r"C:\repo"), [r"GIT_DIR=C:\repo\.git"], "C:\\ 는 뿌리");
    assert_eq!(pin("C:/repo/.git", "C:/repo"), ["GIT_DIR=C:/repo/.git"], "C:/ 는 뿌리");
    // 공백·한글 경로
    assert_eq!(
        pin(r"D:\내 프로젝트\저장소\.git", r"D:\내 프로젝트\저장소"),
        [r"GIT_DIR=D:\내 프로젝트\저장소\.git", r"GIT_CEILING_DIRECTORIES=D:\내 프로젝트"]
    );
    // 확장 경로 접두(\\?\)
    assert_eq!(
        pin(r"\\?\C:\w\repo\.git", r"\\?\C:\w\repo"),
        [r"GIT_DIR=\\?\C:\w\repo\.git", r"GIT_CEILING_DIRECTORIES=\\?\C:\w"]
    );
    assert_eq!(pin(r"\\?\C:\repo\.git", r"\\?\C:\repo"), [r"GIT_DIR=\\?\C:\repo\.git"]);
    // UNC: 공유 아래 폴더면 공유 뿌리를 넣고, 공유 뿌리 자체의 부모(서버)는 넣지 않는다
    assert_eq!(
        pin(r"\\srv\share\repo\.git", r"\\srv\share\repo"),
        [r"GIT_DIR=\\srv\share\repo\.git", r"GIT_CEILING_DIRECTORIES=\\srv\share"]
    );
    assert_eq!(pin(r"\\srv\share\.git", r"\\srv\share"), [r"GIT_DIR=\\srv\share\.git"]);
    // 부모가 없는 상대 경로·빈 글·뿌리 자체
    assert_eq!(pin("/.git", "repo"), ["GIT_DIR=/.git"]);
    assert_eq!(pin("/.git", ""), ["GIT_DIR=/.git"]);
    assert_eq!(pin("/.git", "/"), ["GIT_DIR=/.git"]);
    // POSIX 끝 슬래시
    assert_eq!(pin("/w/repo/.git", "/w/repo/"), ["GIT_DIR=/w/repo/.git", "GIT_CEILING_DIRECTORIES=/w"]);
}

#[test]
fn 바뀜_윈도우_경로() {
    let record = rec(Some(r"C:\r\.git"), Some("7:1234"), Some(r"C:\r\.git\worktrees\x"), Some("7:99"));
    // 같은 글자 → 통과
    assert_eq!(board_git_changed(&record, Some(r"C:\r\.git"), Some("7:1234"), Some("7:99"), false), None);
    // 글자가 다르면(대소문자·구분자 포함) 닫힌 쪽
    assert_eq!(board_git_changed(&record, Some(r"c:\r\.git"), Some("7:1234"), Some("7:99"), false), Some(공통));
    assert_eq!(board_git_changed(&record, Some("C:/r/.git"), Some("7:1234"), Some("7:99"), false), Some(공통));
    // 다른 드라이브
    assert_eq!(board_git_changed(&record, Some(r"D:\r\.git"), Some("7:1234"), Some("7:99"), false), Some(공통));
    // 파일 ID 가 다름
    assert_eq!(board_git_changed(&record, Some(r"C:\r\.git"), Some("7:1235"), Some("7:99"), false), Some(공통));
    assert_eq!(board_git_changed(&record, Some(r"C:\r\.git"), Some("7:1234"), Some("7:100"), false), Some(작업));
    // 작업 트리 폴더를 지운 뒤에는 공통만 본다
    assert_eq!(board_git_changed(&record, Some(r"C:\r\.git"), Some("7:1234"), None, true), None);
}
