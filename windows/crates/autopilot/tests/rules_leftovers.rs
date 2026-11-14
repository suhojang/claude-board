//! 단위가 끝난 자리에 남은 변경 규칙(`rules::leftovers`) 시험.
//! 원본: `Tests/AutopilotRulesTests/LeftoversTests.swift`(표 12+10+7 줄과 메시지 시험 1). Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다.
//! 표 길이를 세어 줄이 빠지면 실패한다. 끝의 "추가" 절은 Swift 시험에 없는, Windows 엔진이 넘기는 상태 줄 꼴과 경계값을 지킨다.

use claude_board_autopilot::rules::leftovers::{leftover, stash_message, stash_paths, stash_unit, Leftover, LeftoverWhy};

fn v(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| (*s).to_string()).collect()
}

fn stash(paths: &[&str]) -> Leftover {
    Leftover::Stash(Some(v(paths)))
}

// ── 가지를 바꾸기 전 (apLeftover) ─────────────────────

struct LeftoverRow {
    note: &'static str,
    dirty: &'static [&'static str],
    start: &'static [&'static str],
    story: bool,
    made: bool,
    want: Leftover,
}

fn leftover_rows() -> Vec<LeftoverRow> {
    vec![
        LeftoverRow { note: "변경 없음 → 그대로", dirty: &[], start: &[], story: false, made: true, want: Leftover::Clean },
        LeftoverRow { note: "출발 때와 같은 변경 → 그대로(스토리 가지여도)", dirty: &[" M a.txt"], start: &[" M a.txt"], story: true, made: false, want: Leftover::Clean },
        LeftoverRow { note: "이 실행이 만든 가지에 새 변경, 출발 때 깨끗 → 커밋", dirty: &["?? b.txt"], start: &[], story: false, made: true, want: Leftover::Commit },
        LeftoverRow { note: "이 실행이 만든 가지가 아님 → 바꾸지 않음", dirty: &["?? b.txt"], start: &[], story: false, made: false, want: Leftover::Refuse(LeftoverWhy::Dirty) },
        LeftoverRow { note: "출발 때부터 변경이 있었고 더 바뀜 → 바꾸지 않음", dirty: &[" M a.txt", "?? b.txt"], start: &[" M a.txt"], story: false, made: true, want: Leftover::Refuse(LeftoverWhy::Dirty) },
        LeftoverRow { note: "줄 차례만 달라도 다른 것으로 본다", dirty: &["?? b.txt", " M a.txt"], start: &[" M a.txt", "?? b.txt"], story: false, made: true, want: Leftover::Refuse(LeftoverWhy::Dirty) },
        LeftoverRow { note: "출발 때 있던 변경이 없어짐 → 바꾸지 않음", dirty: &[], start: &[" M a.txt"], story: false, made: true, want: Leftover::Refuse(LeftoverWhy::Dirty) },
        LeftoverRow { note: "5-3b ①: 읽기 전용 스토리 가지에 새 변경, 출발 때 깨끗 → 모두 보관하고 이어 감(전에는 바꾸지 않아 뒤 단위를 건너뜀)", dirty: &["?? stray.txt"], start: &[], story: true, made: true, want: Leftover::Stash(None) },
        LeftoverRow { note: "읽기 전용 스토리 가지는 만든 가지인지 보지 않는다", dirty: &[" M a.txt"], start: &[], story: true, made: false, want: Leftover::Stash(None) },
        LeftoverRow { note: "5-3b ①: 출발 때부터 사람의 변경이 있으면 새 변경의 경로만 보관(사람의 변경은 남김)", dirty: &[" M a.txt", "?? stray.txt", " M src/b.txt"], start: &[" M a.txt"], story: true, made: true, want: stash(&["stray.txt", "src/b.txt"]) },
        LeftoverRow { note: "5-3b ①: 사람이 바꾼 파일을 모델이 더 바꿈(같은 파일) → 갈라 낼 수 없어 바꾸지 않음", dirty: &["MM a.txt"], start: &[" M a.txt"], story: true, made: true, want: Leftover::Refuse(LeftoverWhy::ReadOnly) },
        LeftoverRow { note: "5-3b ①: 출발 때 있던 변경이 없어짐 → 보관으로 되돌릴 수 없어 바꾸지 않음", dirty: &["?? stray.txt"], start: &[" M a.txt"], story: true, made: true, want: Leftover::Refuse(LeftoverWhy::ReadOnly) },
    ]
}

#[test]
fn leftover_rows_table() {
    let rows = leftover_rows();
    assert_eq!(rows.len(), 12, "Swift leftoverRows 는 12 줄");
    for r in &rows {
        assert_eq!(leftover(&v(r.dirty), &v(r.start), r.story, r.made), r.want, "{}", r.note);
    }
}

// ── 새 변경의 경로 (apStashPaths) ─────────────────────

struct StashPathsRow {
    note: &'static str,
    dirty: &'static [&'static str],
    start: &'static [&'static str],
    want: Option<&'static [&'static str]>,
}

const STASH_PATHS_ROWS: [StashPathsRow; 10] = [
    StashPathsRow { note: "새 줄의 경로만, 상태 줄 차례대로", dirty: &["?? z.txt", " M a.txt", " D old.txt"], start: &[" M a.txt"], want: Some(&["z.txt", "old.txt"]) },
    StashPathsRow { note: "추적 안 하는 폴더(dir/)도 경로 그대로", dirty: &[" M a.txt", "?? notes/"], start: &[" M a.txt"], want: Some(&["notes/"]) },
    StashPathsRow { note: "새 변경이 사람이 만든 추적 안 하는 폴더 안 → 갈라 낼 수 없음", dirty: &["?? mine/", "?? mine/x.txt"], start: &["?? mine/"], want: None },
    StashPathsRow { note: "새 폴더가 사람의 파일을 품음 → 갈라 낼 수 없음", dirty: &[" M d/a.txt", "?? d/"], start: &[" M d/a.txt"], want: None },
    StashPathsRow { note: "이름 바꿈(R) → 갈라 낼 수 없음", dirty: &[" M a.txt", "R  b.txt -> c.txt"], start: &[" M a.txt"], want: None },
    StashPathsRow { note: "복사(C) → 갈라 낼 수 없음", dirty: &[" M a.txt", "C  b.txt -> c.txt"], start: &[" M a.txt"], want: None },
    StashPathsRow { note: "따옴표로 싼 경로(특수 글자) → 갈라 낼 수 없음", dirty: &[" M a.txt", r#"?? "sp ace\t.txt""#], start: &[" M a.txt"], want: None },
    StashPathsRow { note: "출발 때 줄이 없어짐 → 갈라 낼 수 없음", dirty: &["?? z.txt"], start: &[" M a.txt"], want: None },
    StashPathsRow { note: "새 줄이 없음 → 없음(보관할 것 없음)", dirty: &[" M a.txt"], start: &[" M a.txt"], want: None },
    StashPathsRow { note: "짧은 줄(꼴이 아님) → 갈라 낼 수 없음", dirty: &[" M a.txt", "??"], start: &[" M a.txt"], want: None },
];

#[test]
fn stash_paths_rows_table() {
    for r in &STASH_PATHS_ROWS {
        assert_eq!(stash_paths(&v(r.dirty), &v(r.start)), r.want.map(v), "{}", r.note);
    }
}

// ── 보관 메시지와 누구 몫 (apStashMessage·apStashUnit) ─

struct StashUnitRow {
    note: &'static str,
    statuses: &'static [&'static str],
    before: Option<i64>,
    want: Option<i64>,
}

const STASH_UNIT_ROWS: [StashUnitRow; 7] = [
    StashUnitRow { note: "단위 3 을 시작하기 전 → 앞에서 마지막으로 돈 단위 2", statuses: &["done", "failed", "pending"], before: Some(3), want: Some(2) },
    StashUnitRow { note: "건너뛴 단위는 돌지 않았으므로 세지 않는다", statuses: &["done", "skipped", "pending"], before: Some(3), want: Some(1) },
    StashUnitRow { note: "첫 단위 앞 → 없음", statuses: &["pending", "pending"], before: Some(1), want: None },
    StashUnitRow { note: "끝 정리(before 없음) → 모든 단위 가운데 마지막으로 돈 단위", statuses: &["done", "capped", "skipped"], before: None, want: Some(2) },
    StashUnitRow { note: "중단·도는 중도 돈 단위", statuses: &["done", "stopped"], before: None, want: Some(2) },
    StashUnitRow { note: "before 가 대기열보다 크면 끝까지 본다", statuses: &["done"], before: Some(5), want: Some(1) },
    StashUnitRow { note: "빈 대기열 → 없음", statuses: &[], before: None, want: None },
];

#[test]
fn stash_unit_rows_table() {
    for r in &STASH_UNIT_ROWS {
        assert_eq!(stash_unit(&v(r.statuses), r.before), r.want, "{}", r.note);
    }
}

#[test]
fn stash_message_has_run_unit_repo_branch() {
    assert_eq!(
        stash_message("zz-run", Some(3), "shop-core", "features/parent/SHOP-1"),
        "자율주행 zz-run 단위 3 · shop-core: 읽기 전용으로 걸어 둔 features/parent/SHOP-1 에 남은 변경(엔진이 보관)"
    );
    assert_eq!(
        stash_message("zz-run", None, "web", "f/S"),
        "자율주행 zz-run 단위 ? · web: 읽기 전용으로 걸어 둔 f/S 에 남은 변경(엔진이 보관)"
    );
}

// ── 추가: Swift 시험에 없는 경계값과 Windows 엔진이 넘기는 상태 줄 꼴 ─────

#[test]
fn extra_windows_porcelain_lines_pass_through_unchanged() {
    // Windows git 의 `status --porcelain` 은 `/` 구분자 경로를 낸다. 규칙은 경로를 고치지 않고 돌려준다
    assert_eq!(
        stash_paths(&v(&[" M a.txt", "?? sub/dir/new file.txt", " M C:/work/x.txt"]), &v(&[" M a.txt"])),
        Some(v(&["sub/dir/new file.txt", "C:/work/x.txt"]))
    );
    // 역슬래시는 구분자로 보지 않는다(글자 그대로) — 폴더 겹침 판정은 `/` 로만 한다
    assert_eq!(
        stash_paths(&v(&["?? mine\\", "?? mine\\x.txt"]), &v(&["?? mine\\"])),
        Some(v(&["mine\\x.txt"]))
    );
}

#[test]
fn extra_quoted_korean_path_is_not_split() {
    // core.quotepath 기본값에서 git 은 한글 경로를 8진 이스케이프를 따옴표로 싸서 낸다 → 갈라 낼 수 없음
    let quoted = r#"?? "\355\225\234\352\270\200.txt""#;
    assert_eq!(stash_paths(&v(&[" M a.txt", quoted]), &v(&[" M a.txt"])), None);
    assert_eq!(
        leftover(&v(&[" M a.txt", quoted]), &v(&[" M a.txt"]), true, true),
        Leftover::Refuse(LeftoverWhy::ReadOnly)
    );
    // quotepath=false 로 한글 경로가 그대로 오면 글자(char) 단위로 자르므로 경로를 돌려준다
    assert_eq!(stash_paths(&v(&[" M a.txt", "?? 한글/새 파일.txt"]), &v(&[" M a.txt"])), Some(v(&["한글/새 파일.txt"])));
}

#[test]
fn extra_line_shape_edges() {
    // 길이 3 이하는 꼴이 아니다. 4 글자는 경로 한 글자로 읽는다
    assert_eq!(stash_paths(&v(&[" M a.txt", "?? "]), &v(&[" M a.txt"])), None);
    assert_eq!(stash_paths(&v(&[" M a.txt", "?? z"]), &v(&[" M a.txt"])), Some(v(&["z"])));
    // 출발 줄이 짧은 꼴이어도 새 줄 판정에는 영향이 없다(경로로 세지 않을 뿐)
    assert_eq!(stash_paths(&v(&["??", "?? z"]), &v(&["??"])), Some(v(&["z"])));
    // X 가 R·C 인 줄만 막고 Y 쪽 R 도 막는다(XY 두 글자 어디든)
    assert_eq!(stash_paths(&v(&[" M a.txt", " R b.txt"]), &v(&[" M a.txt"])), None);
    // 경로 쪽의 R·C 글자는 막지 않는다
    assert_eq!(stash_paths(&v(&[" M a.txt", "?? CR.txt"]), &v(&[" M a.txt"])), Some(v(&["CR.txt"])));
}

#[test]
fn extra_stash_unit_bounds() {
    let s = v(&["done", "done"]);
    assert_eq!(stash_unit(&s, Some(0)), None);
    assert_eq!(stash_unit(&s, Some(-3)), None);
    assert_eq!(stash_unit(&s, Some(i64::MIN)), None);
    assert_eq!(stash_unit(&s, Some(2)), Some(1));
    assert_eq!(stash_unit(&s, Some(3)), Some(2));
    assert_eq!(stash_unit(&v(&["pending", "skipped"]), None), None);
    // 모르는 상태 글자는 돈 것으로 센다
    assert_eq!(stash_unit(&v(&["done", "weird"]), None), Some(2));
}

#[test]
fn extra_leftover_story_branch_cases() {
    // 스토리 가지 + 출발 변경 있음 + 같은 줄만 늘어남 → 새 경로만
    assert_eq!(leftover(&v(&[" M a.txt", "?? n.txt"]), &v(&[" M a.txt"]), true, false), stash(&["n.txt"]));
    // 스토리 가지가 아니고 출발 때부터 변경이 있었으면 만든 가지여도 바꾸지 않음
    assert_eq!(leftover(&v(&[" M a.txt", "?? n.txt"]), &v(&[" M a.txt"]), false, true), Leftover::Refuse(LeftoverWhy::Dirty));
    // 스토리 가지가 아니고 출발 때 깨끗하며 이 실행이 만든 가지면 커밋
    assert_eq!(leftover(&v(&[" M a.txt"]), &[], false, true), Leftover::Commit);
}
