//! 지난 실행 원문 지우기 규칙(`rules::purge`) 시험.
//! 원본: `Tests/AutopilotRulesTests/StartTests.swift` 의 `PurgeTests`(표 12줄을 도는 시험 `표` 1개 + 유스케이스 시험 `지우기`·`갈래`).
//! 이 규칙 전용 시험 파일은 없다(`PurgeTests.swift` 없음). Swift 시험 이름을 Rust 시험 이름으로 그대로 쓰고, 표의 줄을 같은 입력·같은 기대값으로 옮겼다.
//! 표 길이를 세어 줄이 빠지면 실패한다. 끝의 "추가" 절은 Swift 시험에 없는 Windows 경로 꼴·경계값을 이 이식이 따로 지키는 것이다.

use claude_board_autopilot::ports::Clock;
use claude_board_autopilot::rules::purge::{purge_check, purge_raw, PurgeDecision, PurgeInput, PurgeRecord, RawFiles, PURGE_KEPT};
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Duration;

const RUN: &str = "20261030-0100-zz-todo";
const RUN_DIR: &str = "/R/autopilot/runs/20261030-0100-zz-todo";

// ── 표 (Swift PurgeRow) ───────────────────────────────

struct PurgeRow {
    #[allow(dead_code)]
    note: &'static str,
    run: &'static str,
    run_dir: &'static str,
    state: &'static str,
    wrapped: bool,
    merge: bool,
    worktree: bool,
    draft: bool,
    exists: bool,
    /// "ok" / "gone" / 까닭
    want: &'static str,
}

impl PurgeRow {
    fn new(note: &'static str, want: &'static str) -> Self {
        PurgeRow { note, run: RUN, run_dir: RUN_DIR, state: "done", wrapped: true, merge: false, worktree: false, draft: false, exists: true, want }
    }
}

fn purge_rows() -> Vec<PurgeRow> {
    vec![
        PurgeRow::new("끝나고 정리를 마친 실행", "ok"),
        PurgeRow { state: "stopped", ..PurgeRow::new("중단·오류로 끝난 실행도", "ok") },
        PurgeRow { exists: false, ..PurgeRow::new("이미 지움", "gone") },
        PurgeRow { state: "running", ..PurgeRow::new("도는 실행", "끝나지 않은 실행입니다") },
        PurgeRow {
            wrapped: false,
            ..PurgeRow::new("끝 정리 전", "엔진이 끝 정리를 마치지 않았습니다(하네스 검사·기록 저장소 쓰기가 실행 폴더를 씁니다)")
        },
        PurgeRow {
            merge: true,
            ..PurgeRow::new("합치기 기다림", "합치기·승격이 남았습니다. 합치기·승격이 이 실행 폴더의 기록을 쓰므로 합치거나 버린 뒤에 지웁니다")
        },
        PurgeRow {
            worktree: true,
            ..PurgeRow::new(
                "따로 작업 트리가 남음",
                "따로 작업 트리(~/.claude-board/worktrees/20261030-0100-zz-todo)가 남아 있습니다. 합치기·승격에서 버리거나 합친 뒤에 지웁니다",
            )
        },
        PurgeRow {
            draft: true,
            ..PurgeRow::new("출발 준비가 이어 가는 중", "출발 준비가 이 실행을 이어 가는 중입니다(남은 단위의 정의를 이 실행 폴더에서 읽습니다)")
        },
        PurgeRow {
            run_dir: "/Users/u/.claude/notifier/autopilot/runs/20261030-0100-zz-todo",
            ..PurgeRow::new(
                "기록의 실행 폴더가 뿌리 밖(옛 자리에서 옮겨 담은 실행)",
                "실행 폴더가 이 보드의 자율주행 뿌리 밖(/Users/u/.claude/notifier/autopilot/runs/20261030-0100-zz-todo)이라 보드에서 지우지 않습니다",
            )
        },
        PurgeRow {
            run_dir: "/R/autopilot/runs/other",
            ..PurgeRow::new("id 와 폴더 이름이 다름", "실행 폴더가 이 보드의 자율주행 뿌리 밖(/R/autopilot/runs/other)이라 보드에서 지우지 않습니다")
        },
        PurgeRow {
            run: "../private",
            run_dir: "/R/autopilot/runs/../private",
            ..PurgeRow::new("id 에 / (다른 자리로)", "실행 id 꼴이 아니라 지우지 않습니다")
        },
        PurgeRow { run: "..", run_dir: "/R/autopilot/runs/..", ..PurgeRow::new("id 가 ..", "실행 id 꼴이 아니라 지우지 않습니다") },
    ]
}

fn input(r: &PurgeRow) -> PurgeInput {
    PurgeInput {
        run: r.run.to_string(),
        root: "/R/autopilot".to_string(),
        run_dir: r.run_dir.to_string(),
        state: r.state.to_string(),
        wrapped: r.wrapped,
        merge_waiting: r.merge,
        worktree_left: r.worktree,
        in_draft: r.draft,
        exists: r.exists,
    }
}

// ── 가짜 포트 (Swift FakeRaw·FakeClock) ───────────────

#[derive(Default)]
struct FakeRaw {
    dirs: Mutex<BTreeSet<String>>,
    removed: Mutex<Vec<String>>,
    records: Mutex<Vec<(String, Vec<u8>)>>,
    fail_remove: Option<&'static str>,
    link: bool,
    fail_write: bool,
}

impl FakeRaw {
    fn new(dirs: &[&str]) -> Self {
        FakeRaw { dirs: Mutex::new(dirs.iter().map(|d| (*d).to_string()).collect()), ..Default::default() }
    }
    fn removed(&self) -> Vec<String> {
        self.removed.lock().unwrap().clone()
    }
    fn has_dir(&self, p: &str) -> bool {
        self.dirs.lock().unwrap().contains(p)
    }
    fn record(&self, run: &str) -> Option<Vec<u8>> {
        self.records.lock().unwrap().iter().find(|(r, _)| r == run).map(|(_, d)| d.clone())
    }
    fn record_count(&self) -> usize {
        self.records.lock().unwrap().len()
    }
}

impl RawFiles for FakeRaw {
    fn is_real_dir(&self, path: &str) -> bool {
        !self.link && self.has_dir(path)
    }
    fn usage(&self, _path: &str) -> (i64, i64) {
        (12, 34_567)
    }
    fn remove(&self, path: &str) -> Option<String> {
        if let Some(f) = self.fail_remove {
            return Some(f.to_string());
        }
        self.removed.lock().unwrap().push(path.to_string());
        self.dirs.lock().unwrap().remove(path);
        None
    }
    fn write_record(&self, run: &str, data: &[u8]) -> bool {
        if self.fail_write {
            return false;
        }
        let mut g = self.records.lock().unwrap();
        g.retain(|(r, _)| r != run);
        g.push((run.to_string(), data.to_vec()));
        true
    }
}

struct FakeClock(f64);

impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.0
    }
    fn sleep(&self, _d: Duration) {}
}

fn decode(bytes: &[u8]) -> PurgeRecord {
    serde_json::from_slice(bytes).expect("지운 기록은 PurgeRecord 로 읽힌다")
}

// ── Swift PurgeTests ──────────────────────────────────

#[test]
fn 표() {
    let rows = purge_rows();
    assert_eq!(rows.len(), 12, "Swift purgeRows 는 12 줄");
    for r in &rows {
        let s = match purge_check(&input(r)) {
            PurgeDecision::Ok(t) => {
                assert_eq!(t, format!("/R/autopilot/runs/{}", r.run), "{}", r.note);
                "ok".to_string()
            }
            PurgeDecision::Gone => "gone".to_string(),
            PurgeDecision::Refuse(w) => w,
        };
        assert_eq!(s, r.want, "{}", r.note);
    }
}

#[test]
fn 지우기() {
    let r = PurgeRow::new("", "ok");
    let f = FakeRaw::new(&["/R/autopilot/runs/20261030-0100-zz-todo", "/R/autopilot/private/20261030-0100-zz-todo"]);
    let x = purge_raw(&input(&r), &f, &FakeClock(1_793_300_000.0));
    assert!(x.ok && x.error.is_none());
    assert_eq!(f.removed(), vec!["/R/autopilot/runs/20261030-0100-zz-todo".to_string()]);
    assert!(f.has_dir("/R/autopilot/private/20261030-0100-zz-todo"));
    let rec = decode(&f.record(r.run).expect("기록이 쓰였다"));
    assert!(rec.files == 12 && rec.bytes == 34_567 && rec.at == 1_793_300_000.0 && rec.error.is_none());
    assert!(rec.kept.contains(&"엔진 전용 private/20261030-0100-zz-todo/(하네스 사본)".to_string()));
    assert_eq!(x.record, Some(rec));
}

#[test]
fn 갈래() {
    let r = PurgeRow::new("", "ok");
    let dir = "/R/autopilot/runs/20261030-0100-zz-todo";
    // 링크면 지우지 않는다
    let f = FakeRaw { link: true, ..FakeRaw::new(&[dir]) };
    let a = purge_raw(&input(&r), &f, &FakeClock(0.0));
    assert!(!a.ok && f.removed().is_empty() && f.record_count() == 0 && a.error.as_deref() == Some("실행 폴더가 링크이거나 폴더가 아니라 지우지 않습니다"));
    // 지우다 실패해도 기록은 쓴다
    let g = FakeRaw { fail_remove: Some("권한 없음"), ..FakeRaw::new(&[dir]) };
    let b = purge_raw(&input(&r), &g, &FakeClock(0.0));
    assert!(!b.ok && b.error.as_deref() == Some("권한 없음"));
    assert_eq!(decode(&g.record(r.run).expect("실패해도 기록은 쓴다")).error.as_deref(), Some("권한 없음"));
    // 다시 확인해 거부
    let h = FakeRaw::new(&[dir]);
    let mut m = input(&r);
    m.merge_waiting = true;
    assert!(purge_raw(&m, &h, &FakeClock(0.0)).error.is_some_and(|e| e.starts_with("합치기·승격이 남았습니다")) && h.removed().is_empty());
}

// ── 추가: Swift 시험에 없는 자리 ─────────────────────────

fn base() -> PurgeInput {
    input(&PurgeRow::new("", "ok"))
}

fn refuse_text(i: &PurgeInput) -> String {
    match purge_check(i) {
        PurgeDecision::Refuse(w) => w,
        other => panic!("거부여야 한다: {other:?}"),
    }
}

const BAD_ID: &str = "실행 id 꼴이 아니라 지우지 않습니다";

#[test]
fn extra_ok_path_is_root_runs_id_for_every_end_state() {
    for state in ["done", "stopped", "error"] {
        let i = PurgeInput { state: state.to_string(), ..base() };
        assert_eq!(purge_check(&i), PurgeDecision::Ok("/R/autopilot/runs/20261030-0100-zz-todo".to_string()), "{state}");
    }
    for state in ["", "running", "Done", "done ", "queued", "paused"] {
        let i = PurgeInput { state: state.to_string(), ..base() };
        assert_eq!(refuse_text(&i), "끝나지 않은 실행입니다", "{state:?}");
    }
}

#[test]
fn extra_check_order_is_id_then_dir_then_gone_then_state_wrapped_merge_worktree_draft() {
    // id 꼴이 가장 먼저
    let i = PurgeInput { run: "a b".to_string(), run_dir: "/x".to_string(), exists: false, state: "running".to_string(), ..base() };
    assert_eq!(refuse_text(&i), BAD_ID);
    // 자리가 다르면 없어도 거부(없다는 답은 자리가 맞을 때만)
    let i = PurgeInput { run_dir: "/R/autopilot/runs/other".to_string(), exists: false, ..base() };
    assert!(refuse_text(&i).starts_with("실행 폴더가 이 보드의 자율주행 뿌리 밖("));
    // 없으면 상태·정리 여부를 보지 않는다
    let i = PurgeInput { exists: false, state: "running".to_string(), wrapped: false, merge_waiting: true, worktree_left: true, in_draft: true, ..base() };
    assert_eq!(purge_check(&i), PurgeDecision::Gone);
    // 상태 → 끝 정리 → 합치기 → 작업 트리 → 출발 준비 차례
    let all = PurgeInput { state: "running".to_string(), wrapped: false, merge_waiting: true, worktree_left: true, in_draft: true, ..base() };
    assert_eq!(refuse_text(&all), "끝나지 않은 실행입니다");
    let all = PurgeInput { state: "done".to_string(), ..all };
    assert!(refuse_text(&all).starts_with("엔진이 끝 정리를 마치지 않았습니다"));
    let all = PurgeInput { wrapped: true, ..all };
    assert!(refuse_text(&all).starts_with("합치기·승격이 남았습니다"));
    let all = PurgeInput { merge_waiting: false, ..all };
    assert!(refuse_text(&all).starts_with("따로 작업 트리("));
    let all = PurgeInput { worktree_left: false, ..all };
    assert!(refuse_text(&all).starts_with("출발 준비가 이 실행을 이어 가는 중입니다"));
    let all = PurgeInput { in_draft: false, ..all };
    assert!(matches!(purge_check(&all), PurgeDecision::Ok(_)));
}

#[test]
fn extra_run_id_shapes() {
    for ok in ["a", "A", "0", ".a", "a.", "a_b", "a-b", "a.b.c", "20261030-0100-zz-todo", "...", "-", "_"] {
        let i = PurgeInput { run: ok.to_string(), run_dir: format!("/R/autopilot/runs/{ok}"), ..base() };
        assert!(matches!(purge_check(&i), PurgeDecision::Ok(_)), "{ok:?} 는 id 꼴이다");
    }
    for bad in ["", ".", "..", "a b", "a/b", "a\\b", "a:b", "a*b", "a?b", "한글", "a\u{FF21}", "a\u{3000}", "a\0", " a", "a ", "~1", "a\u{301}"] {
        let i = PurgeInput { run: bad.to_string(), run_dir: format!("/R/autopilot/runs/{bad}"), ..base() };
        assert_eq!(refuse_text(&i), BAD_ID, "{bad:?} 는 id 꼴이 아니다");
    }
}

#[test]
fn extra_trailing_newline_id_is_refused() {
    // 원본(ICU 정규식 `$`)은 끝 줄바꿈 앞에서도 맞아 이 id 를 통과시킨다. 이식은 더 엄격하게 거부한다(다른 점으로 적어 둔다)
    for bad in ["abc\n", "abc\r\n", "abc\r", "abc\u{2028}"] {
        let i = PurgeInput { run: bad.to_string(), run_dir: format!("/R/autopilot/runs/{bad}"), ..base() };
        assert_eq!(refuse_text(&i), BAD_ID, "{bad:?}");
    }
}

#[test]
fn extra_windows_root_with_backslashes_joins_with_slash_and_compares_literally() {
    let root = r"C:\Users\u\.claude\notifier\autopilot";
    let want = format!("{root}/runs/{RUN}");
    let i = PurgeInput { root: root.to_string(), run_dir: want.clone(), ..base() };
    assert_eq!(purge_check(&i), PurgeDecision::Ok(want.clone()));
    // 어댑터가 모두 `\` 로 이어 붙인 runDir 는 같은 자리여도 글자가 달라 거부된다 — 어댑터가 root·run_dir 를 같은 방식으로 넘겨야 한다
    let all_back = format!(r"{root}\runs\{RUN}");
    let i = PurgeInput { root: root.to_string(), run_dir: all_back.clone(), ..base() };
    assert_eq!(refuse_text(&i), format!("실행 폴더가 이 보드의 자율주행 뿌리 밖({all_back})이라 보드에서 지우지 않습니다"));
}

#[test]
fn extra_windows_path_forms_are_different_places_never_equal() {
    let root = r"C:\Users\u\autopilot";
    let want = format!("{root}/runs/{RUN}");
    let mk = |dir: &str| PurgeInput { root: root.to_string(), run_dir: dir.to_string(), ..base() };
    // 대소문자만 다름(Windows 파일 시스템은 같은 폴더로 보지만 규칙은 다른 자리로 본다)
    assert!(matches!(purge_check(&mk(&want.to_uppercase())), PurgeDecision::Refuse(_)));
    assert!(matches!(purge_check(&mk(&want.replacen("C:", "c:", 1))), PurgeDecision::Refuse(_)));
    // `\\?\` 확장 경로 접두
    assert!(matches!(purge_check(&mk(&format!(r"\\?\{want}"))), PurgeDecision::Refuse(_)));
    assert!(matches!(purge_check(&mk(&format!(r"\\?\UNC\srv\share\{RUN}"))), PurgeDecision::Refuse(_)));
    // 끝 점·공백·구분자(Windows 가 떼어 내는 글자)
    for tail in [".", " ", "/", "\\", ". ", ".."] {
        assert!(matches!(purge_check(&mk(&format!("{want}{tail}"))), PurgeDecision::Refuse(_)), "끝 {tail:?}");
    }
    // 8.3 짧은 이름·`..` 로 돌아온 같은 자리
    assert!(matches!(purge_check(&mk(&format!("{root}/runs/../runs/{RUN}"))), PurgeDecision::Refuse(_)));
    assert!(matches!(purge_check(&mk(&format!("{root}//runs/{RUN}"))), PurgeDecision::Refuse(_)));
    // 같은 글자만 통과
    assert_eq!(purge_check(&mk(&want)), PurgeDecision::Ok(want));
}

#[test]
fn extra_long_windows_path_over_260_chars_is_compared_the_same() {
    let root = format!(r"C:\{}\autopilot", "d".repeat(280));
    let want = format!("{root}/runs/{RUN}");
    assert!(want.len() > 260);
    let i = PurgeInput { root: root.clone(), run_dir: want.clone(), ..base() };
    assert_eq!(purge_check(&i), PurgeDecision::Ok(want.clone()));
    let i = PurgeInput { root, run_dir: format!("{want}x"), ..base() };
    assert!(matches!(purge_check(&i), PurgeDecision::Refuse(_)));
}

#[test]
fn extra_korean_root_is_kept_verbatim_in_ok_and_refuse_messages() {
    let root = r"C:\Users\홍길동\문서\autopilot";
    let want = format!("{root}/runs/{RUN}");
    let i = PurgeInput { root: root.to_string(), run_dir: want.clone(), ..base() };
    assert_eq!(purge_check(&i), PurgeDecision::Ok(want));
    // NFC·NFD 는 다른 글자열이다(원본 Swift 문자열 견줌은 정규 동치로 같게 본다 — 다른 점으로 적어 둔다)
    let nfd = "C:\\Users\\\u{1112}\u{1169}\u{11bc}\u{1100}\u{1175}\u{11ab}\u{1103}\u{1169}\u{11bc}\\autopilot/runs/20261030-0100-zz-todo";
    let i = PurgeInput { root: r"C:\Users\홍길동\autopilot".to_string(), run_dir: nfd.to_string(), ..base() };
    assert!(matches!(purge_check(&i), PurgeDecision::Refuse(_)));
}

#[test]
fn extra_refuse_run_worktree_message_uses_run_id() {
    let i = PurgeInput { run: "a.b-c_d".to_string(), run_dir: "/R/autopilot/runs/a.b-c_d".to_string(), worktree_left: true, ..base() };
    assert_eq!(
        refuse_text(&i),
        "따로 작업 트리(~/.claude-board/worktrees/a.b-c_d)가 남아 있습니다. 합치기·승격에서 버리거나 합친 뒤에 지웁니다"
    );
}

#[test]
fn extra_purge_raw_gone_and_refusals_touch_nothing() {
    let dir = "/R/autopilot/runs/20261030-0100-zz-todo";
    let f = FakeRaw::new(&[dir]);
    let gone = PurgeInput { exists: false, ..base() };
    let x = purge_raw(&gone, &f, &FakeClock(1.0));
    assert!(!x.ok && x.record.is_none() && x.error.as_deref() == Some("실행 폴더가 이미 없습니다"));
    for bad in [PurgeInput { state: "running".to_string(), ..base() }, PurgeInput { in_draft: true, ..base() }, PurgeInput { run: "..".to_string(), run_dir: "/R/autopilot/runs/..".to_string(), ..base() }] {
        let x = purge_raw(&bad, &f, &FakeClock(1.0));
        assert!(!x.ok && x.record.is_none() && x.error.is_some());
    }
    assert!(f.removed().is_empty() && f.record_count() == 0 && f.has_dir(dir));
    // 폴더가 아예 없으면(입력은 있다고 했지만) 진짜 폴더가 아니라고 거부
    let none = FakeRaw::new(&[]);
    let x = purge_raw(&base(), &none, &FakeClock(1.0));
    assert!(!x.ok && x.record.is_none() && x.error.as_deref() == Some("실행 폴더가 링크이거나 폴더가 아니라 지우지 않습니다"));
}

#[test]
fn extra_remove_failure_keeps_dir_and_writes_record_with_error_and_path() {
    let dir = "/R/autopilot/runs/20261030-0100-zz-todo";
    let g = FakeRaw { fail_remove: Some("다른 프로그램이 쓰는 중"), ..FakeRaw::new(&[dir]) };
    let x = purge_raw(&base(), &g, &FakeClock(5.0));
    assert!(!x.ok && x.error.as_deref() == Some("다른 프로그램이 쓰는 중"));
    let rec = decode(&g.record(RUN).unwrap());
    assert_eq!((rec.files, rec.bytes, rec.path.as_str()), (12, 34_567, dir));
    assert!(g.has_dir(dir) && g.removed().is_empty());
}

#[test]
fn extra_record_write_failure_message_and_ok_flag() {
    let dir = "/R/autopilot/runs/20261030-0100-zz-todo";
    // 지우기는 됐고 기록만 못 씀 → ok 는 true 로 남고 error 에 한 줄(원본 `r.ok = err == nil`)
    let f = FakeRaw { fail_write: true, ..FakeRaw::new(&[dir]) };
    let x = purge_raw(&base(), &f, &FakeClock(5.0));
    assert!(x.ok);
    assert_eq!(x.error.as_deref(), Some("지운 기록을 쓰지 못했습니다"));
    assert_eq!(x.record.as_ref().and_then(|r| r.error.as_deref()), Some("지운 기록을 쓰지 못했습니다"));
    assert_eq!(f.removed(), vec![dir.to_string()]);
    // 지우기도 기록도 실패 → 두 까닭을 ` · ` 로 잇는다
    let g = FakeRaw { fail_remove: Some("권한 없음"), fail_write: true, ..FakeRaw::new(&[dir]) };
    let y = purge_raw(&base(), &g, &FakeClock(5.0));
    assert!(!y.ok);
    assert_eq!(y.error.as_deref(), Some("권한 없음 · 지운 기록을 쓰지 못했습니다"));
}

#[test]
fn extra_record_json_has_sorted_keys_integer_time_and_no_escaped_slash() {
    let dir = "/R/autopilot/runs/20261030-0100-zz-todo";
    let f = FakeRaw::new(&[dir]);
    purge_raw(&base(), &f, &FakeClock(1_793_300_000.0));
    let text = String::from_utf8(f.record(RUN).unwrap()).unwrap();
    assert!(!text.contains("\\/"), "슬래시는 이스케이프하지 않는다: {text}");
    assert!(text.contains(r#""at": 1793300000,"#), "정수 시각은 정수로: {text}");
    assert!(!text.contains("\"error\""), "오류가 없으면 키가 없다: {text}");
    let keys: Vec<usize> = ["\"at\"", "\"bytes\"", "\"files\"", "\"kept\"", "\"path\"", "\"run\""].iter().map(|k| text.find(k).unwrap()).collect();
    assert!(keys.windows(2).all(|w| w[0] < w[1]), "키 차례(글자순): {text}");
    assert!(text.contains(r#""path": "/R/autopilot/runs/20261030-0100-zz-todo""#));
    // 오류가 있으면 error 키가 files 앞(글자순)에 있다
    let g = FakeRaw { fail_remove: Some("권한 없음"), ..FakeRaw::new(&[dir]) };
    purge_raw(&base(), &g, &FakeClock(1.5));
    let text = String::from_utf8(g.record(RUN).unwrap()).unwrap();
    assert!(text.contains(r#""at": 1.5,"#), "{text}");
    assert!(text.find("\"bytes\"").unwrap() < text.find("\"error\"").unwrap() && text.find("\"error\"").unwrap() < text.find("\"files\"").unwrap(), "{text}");
}

#[test]
fn extra_record_reads_swift_written_json_with_spaced_colons() {
    // 원본(Swift)이 쓴 꼴: 정렬된 키, `"키" : 값`, 소수 없는 시각, error 키 없음
    let swift = "{\n  \"at\" : 1793300000,\n  \"bytes\" : 34567,\n  \"files\" : 12,\n  \"kept\" : [\n    \"a\"\n  ],\n  \"path\" : \"/R/autopilot/runs/x\",\n  \"run\" : \"x\"\n}";
    let rec = decode(swift.as_bytes());
    assert_eq!(
        rec,
        PurgeRecord { at: 1_793_300_000.0, bytes: 34_567, error: None, files: 12, kept: vec!["a".to_string()], path: "/R/autopilot/runs/x".to_string(), run: "x".to_string() }
    );
}

#[test]
fn extra_kept_list_replaces_run_id_and_keeps_three_lines() {
    assert_eq!(PURGE_KEPT.len(), 3);
    assert_eq!(PURGE_KEPT[0], "기록 저장소의 실행·단위 행(결과 기록 전문)");
    assert_eq!(PURGE_KEPT[2], "프로젝트 설정과 자율주행 세트");
    let f = FakeRaw::new(&["/R/autopilot/runs/20261030-0100-zz-todo"]);
    let x = purge_raw(&base(), &f, &FakeClock(1.0));
    let kept = x.record.unwrap().kept;
    assert_eq!(
        kept,
        vec![
            "기록 저장소의 실행·단위 행(결과 기록 전문)".to_string(),
            "엔진 전용 private/20261030-0100-zz-todo/(하네스 사본)".to_string(),
            "프로젝트 설정과 자율주행 세트".to_string(),
        ]
    );
    assert!(kept.iter().all(|k| !k.contains("<실행 id>")));
}
