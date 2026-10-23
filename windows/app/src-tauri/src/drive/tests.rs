//! `drive` 의 시험 — 가짜 세션과 가짜 `Io` 로 설계 §7 의 표를 하나씩 본다.

use std::cell::RefCell;
use std::path::PathBuf;

use super::*;
use crate::model::PidPlace;

fn session(pid: u32, state: &str, tty: Option<&str>, mode: Option<&'static str>, headless: bool, tab: Option<&str>) -> Session {
    Session {
        id: "sid".into(),
        cwd: String::new(),
        state: state.into(),
        since: 0,
        updated: 0,
        headless,
        title: String::new(),
        topic: String::new(),
        message: String::new(),
        name: None,
        ctx: None,
        brain: None,
        subagents: Vec::new(),
        shells: 0,
        paused: false,
        muted: false,
        limit_at: None,
        can_drive: false,
        place: PidPlace { tty: tty.map(str::to_string), tab_ancestor: tab.map(str::to_string), mode, app_name: None },
        pid,
        pid_ctime: None,
        file: PathBuf::new(),
        project_dir: String::new(),
        resume_args: Vec::new(),
        since_s: 0.0,
        updated_s: 0.0,
        limit_s: None,
    }
}

fn runs(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

const BOTH: [Purpose; 2] = [Purpose::Keys, Purpose::Stop];

#[test]
fn ended_or_pidless_session_has_no_target() {
    for p in BOTH {
        assert_eq!(drive_target(&session(5, "ended", Some("t1"), None, false, None), &runs(&["t1"]), true, p), Target::None);
        assert_eq!(drive_target(&session(0, "working", Some("t1"), None, false, None), &runs(&["t1"]), true, p), Target::None);
    }
}

#[test]
fn interactive_tab_session_writes_to_its_surface() {
    let s = session(5, "working", Some("t1"), None, false, None);
    for p in BOTH {
        assert_eq!(drive_target(&s, &runs(&["t1"]), true, p), Target::Surface("t1".into()));
    }
}

#[test]
fn tab_session_not_in_foreground_is_only_killable() {
    let s = session(5, "working", Some("t1"), None, false, None);
    assert_eq!(drive_target(&s, &runs(&["t2"]), true, Purpose::Keys), Target::None);
    assert_eq!(drive_target(&s, &runs(&["t2"]), true, Purpose::Stop), Target::Kill(5));
}

#[test]
fn print_mode_in_a_tab_is_only_killable() {
    let s = session(5, "working", Some("t1"), Some("-p"), false, None);
    assert_eq!(drive_target(&s, &runs(&["t1"]), true, Purpose::Keys), Target::None);
    assert_eq!(drive_target(&s, &runs(&["t1"]), true, Purpose::Stop), Target::Kill(5));
}

#[test]
fn tab_owned_headless_is_only_killable_and_background_is_untouched() {
    let owned = session(5, "working", None, Some("-p"), true, Some("t1"));
    assert_eq!(drive_target(&owned, &[], true, Purpose::Keys), Target::None);
    assert_eq!(drive_target(&owned, &[], true, Purpose::Stop), Target::Kill(5));
    let background = session(5, "working", None, Some("--bg"), true, None);
    for p in BOTH {
        assert_eq!(drive_target(&background, &[], true, p), Target::None);
    }
}

#[test]
fn foreign_console_session_follows_attachability() {
    let s = session(5, "working", None, None, false, None);
    for p in BOTH {
        assert_eq!(drive_target(&s, &[], true, p), Target::Console(5));
    }
    assert_eq!(drive_target(&s, &[], false, Purpose::Keys), Target::None);
    assert_eq!(drive_target(&s, &[], false, Purpose::Stop), Target::Kill(5));
}

#[test]
fn encode_maps_keys_and_text() {
    assert_eq!(encode(&Step::Escape), Ok(vec![0x1b]));
    assert_eq!(encode(&Step::Enter), Ok(vec![0x0d]));
    assert_eq!(encode(&Step::Text("/exit".into())), Ok(b"/exit".to_vec()));
    assert_eq!(encode(&Step::Text("한글 ok".into())), Ok("한글 ok".as_bytes().to_vec()));
    assert_eq!(encode(&Step::Wait(10)), Ok(Vec::new()));
}

#[test]
fn encode_rejects_control_characters_in_text() {
    for bad in ["a\nb", "a\rb", "a\x1bb", "a\x03b", "\t", "\x7f"] {
        assert!(encode(&Step::Text(bad.into())).is_err(), "{bad:?}");
    }
}

#[test]
fn steps_for_matches_the_mac_lists() {
    assert_eq!(steps_for(SessionAction::Pause), vec![Step::Escape]);
    assert_eq!(steps_for(SessionAction::Resume), vec![Step::Text(CARRY_ON_PROMPT.into()), Step::Wait(200), Step::Enter]);
    assert_eq!(
        steps_for(SessionAction::Stop),
        vec![Step::Escape, Step::Wait(400), Step::Text("/exit".into()), Step::Wait(200), Step::Enter]
    );
    assert!(steps_for(SessionAction::Focus).is_empty());
}

#[test]
fn carry_on_prompt_is_safe_to_type() {
    assert!(encode(&Step::Text(CARRY_ON_PROMPT.into())).is_ok());
}

/// 부른 것을 적어 두는 가짜 — `ok` 가 false 면 쓰기·입력·끝내기가 모두 실패한다.
struct Fake {
    ok: bool,
    log: RefCell<Vec<String>>,
}

impl Fake {
    fn new(ok: bool) -> Self {
        Fake { ok, log: RefCell::new(Vec::new()) }
    }
    fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl Io for Fake {
    fn surface(&self, id: &str, bytes: &[u8]) -> bool {
        self.log.borrow_mut().push(format!("surface {id} {bytes:02x?}"));
        self.ok
    }
    fn console(&self, pid: u32, inputs: &[Input]) -> bool {
        self.log.borrow_mut().push(format!("console {pid} {}", inputs.len()));
        self.ok
    }
    fn kill(&self, pid: u32) -> bool {
        self.log.borrow_mut().push(format!("kill {pid}"));
        self.ok
    }
    fn sleep(&self, ms: u64) {
        self.log.borrow_mut().push(format!("sleep {ms}"));
    }
}

#[test]
fn pause_reports_success_only_when_the_writer_succeeds() {
    let target = Target::Surface("t1".into());
    let ok = Fake::new(true);
    assert!(run(&target, &steps_for(SessionAction::Pause), Purpose::Keys, &ok));
    assert_eq!(ok.calls(), vec!["surface t1 [1b]"]);
    assert!(!run(&target, &steps_for(SessionAction::Pause), Purpose::Keys, &Fake::new(false)));
}

#[test]
fn stop_on_a_surface_sends_the_exit_sequence_with_waits() {
    let io = Fake::new(true);
    assert!(run(&Target::Surface("t1".into()), &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert_eq!(
        io.calls(),
        vec!["surface t1 [1b]", "sleep 400", "surface t1 [2f, 65, 78, 69, 74]", "sleep 200", "surface t1 [0d]"]
    );
}

#[test]
fn stop_on_a_console_kills_when_input_fails() {
    let io = Fake::new(false);
    assert!(!run(&Target::Console(7), &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert_eq!(io.calls(), vec!["console 7 5", "kill 7"]);
    let keys = Fake::new(false);
    assert!(!run(&Target::Console(7), &steps_for(SessionAction::Pause), Purpose::Keys, &keys));
    assert_eq!(keys.calls(), vec!["console 7 1"]);
}

#[test]
fn no_target_never_touches_the_outside() {
    let io = Fake::new(true);
    assert!(!run(&Target::None, &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert!(io.calls().is_empty());
}

#[test]
fn control_character_text_sends_nothing() {
    let io = Fake::new(true);
    let steps = [Step::Escape, Step::Text("a\nb".into())];
    assert!(!run(&Target::Surface("t1".into()), &steps, Purpose::Keys, &io));
    assert!(io.calls().is_empty());
}

#[test]
fn kill_target_terminates() {
    let io = Fake::new(true);
    assert!(run(&Target::Kill(9), &[], Purpose::Stop, &io));
    assert_eq!(io.calls(), vec!["kill 9"]);
}

#[test]
fn rename_is_safe_only_when_working_or_idle_and_not_waiting() {
    assert!(rename_safe("working", Some("busy"), None));
    assert!(rename_safe("idle", None, Some("idle")));
    assert!(!rename_safe("waiting", Some("idle"), None));
    assert!(!rename_safe("working", Some("waiting"), Some("working")));
    assert!(!rename_safe("working", None, Some("waiting")));
    assert!(!rename_safe("ended", None, None));
}

#[test]
fn step_spec_parses_the_frontend_shapes() {
    let specs: Vec<StepSpec> = serde_json::from_str(r#"[{"key":"escape"},{"key":"enter"},{"text":"/rename a"},{"wait":150}]"#).unwrap();
    let steps: Vec<Step> = specs.into_iter().map(|s| s.into_step().unwrap()).collect();
    assert_eq!(steps, vec![Step::Escape, Step::Enter, Step::Text("/rename a".into()), Step::Wait(150)]);
    assert!(StepSpec::Wait { wait: 60_000 }.into_step().is_err());
}

// ── W3e — 설계 §7 표의 빈칸과 가장자리 ────────────────────────────────────────

#[test]
fn tab_session_with_no_claude_running_anywhere_is_only_killable() {
    let s = session(5, "working", Some("t1"), None, false, None);
    assert_eq!(drive_target(&s, &[], true, Purpose::Keys), Target::None);
    assert_eq!(drive_target(&s, &[], true, Purpose::Stop), Target::Kill(5));
    // 콘솔에 붙을 수 없어도 같다
    assert_eq!(drive_target(&s, &[], false, Purpose::Keys), Target::None);
}

#[test]
fn tty_pairing_beats_attachability_for_the_surface() {
    // 보드 탭 짝이 있으면 붙을 수 있어도 Console 로 가지 않는다(ConPTY 에 붙으면 보드가 죽을 수 있다)
    let s = session(5, "working", Some("t1"), None, false, None);
    for attachable in [true, false] {
        for p in BOTH {
            assert_eq!(drive_target(&s, &runs(&["t1"]), attachable, p), Target::Surface("t1".into()));
        }
    }
    let not_running = session(5, "working", Some("t1"), None, false, None);
    assert_ne!(drive_target(&not_running, &[], true, Purpose::Keys), Target::Console(5));
    assert_ne!(drive_target(&not_running, &[], true, Purpose::Stop), Target::Console(5));
}

#[test]
fn headless_with_a_tty_is_only_killable_even_when_its_term_runs_claude() {
    let s = session(5, "working", Some("t1"), None, true, None);
    assert_eq!(drive_target(&s, &runs(&["t1"]), true, Purpose::Keys), Target::None);
    assert_eq!(drive_target(&s, &runs(&["t1"]), true, Purpose::Stop), Target::Kill(5));
}

#[test]
fn ended_or_pidless_never_targets_whatever_the_place() {
    // (tty, 탭 조상, mode, headless)
    type PlaceRow = (Option<&'static str>, Option<&'static str>, Option<&'static str>, bool);
    let places: [PlaceRow; 5] = [
        (Some("t1"), None, None, false),
        (Some("t1"), None, Some("-p"), false),
        (None, Some("t1"), Some("-p"), true),
        (None, None, Some("--bg"), true),
        (None, None, None, false),
    ];
    for (tty, tab, mode, headless) in places {
        for p in BOTH {
            assert_eq!(drive_target(&session(5, "ended", tty, mode, headless, tab), &runs(&["t1"]), true, p), Target::None);
            assert_eq!(drive_target(&session(0, "working", tty, mode, headless, tab), &runs(&["t1"]), true, p), Target::None);
            assert_eq!(drive_target(&session(0, "paused", tty, mode, headless, tab), &runs(&["t1"]), true, p), Target::None);
        }
    }
}

#[test]
fn can_drive_follows_the_keys_target_for_every_live_state() {
    let tab = PidPlace { tty: Some("t1".into()), tab_ancestor: None, mode: None, app_name: None };
    let owned = PidPlace { tty: None, tab_ancestor: Some("t1".into()), mode: Some("-p"), app_name: None };
    let foreign = PidPlace { tty: None, tab_ancestor: None, mode: None, app_name: None };
    for state in ["working", "waiting", "idle", "paused"] {
        assert!(can_drive(state, 5, false, &tab, &runs(&["t1"]), false));
        assert!(!can_drive(state, 5, false, &tab, &[], true));
        assert!(!can_drive(state, 5, true, &owned, &[], true));
        assert!(can_drive(state, 5, false, &foreign, &[], true));
        assert!(!can_drive(state, 5, false, &foreign, &[], false));
    }
    assert!(!can_drive("ended", 5, false, &tab, &runs(&["t1"]), true));
    assert!(!can_drive("working", 0, false, &foreign, &[], true));
}

#[test]
fn encode_rejects_every_c0_c1_and_del_control_in_text() {
    for code in (0u32..0x20).chain([0x7f]).chain(0x80..0xa0) {
        let ch = char::from_u32(code).unwrap();
        for text in [ch.to_string(), format!("a{ch}"), format!("{ch}b")] {
            assert!(encode(&Step::Text(text.clone())).is_err(), "U+{code:04X} in {text:?}");
        }
    }
}

#[test]
fn encode_text_edge_values() {
    assert_eq!(encode(&Step::Text(String::new())), Ok(Vec::new()));
    // 보이는 특수문자·이모지·유니코드 공백은 글이다
    assert_eq!(encode(&Step::Text("a b&c%d\"e".into())), Ok(b"a b&c%d\"e".to_vec()));
    assert_eq!(encode(&Step::Text("😀\u{3000}".into())), Ok("😀\u{3000}".as_bytes().to_vec()));
    // 오류 문구는 문제의 글자를 가리킨다
    assert!(encode(&Step::Text("a\x1bb".into())).unwrap_err().contains("U+001B"));
    assert!(encode(&Step::Text("a\rb".into())).unwrap_err().contains("U+000D"));
    assert!(encode(&Step::Text("a\u{9b}b".into())).unwrap_err().contains("U+009B"));
}

#[test]
fn steps_for_other_actions_send_no_keys() {
    for action in [SessionAction::Focus, SessionAction::Activate, SessionAction::Reopen { carry_on: true }] {
        assert!(steps_for(action).is_empty());
    }
}

#[test]
fn every_text_step_the_board_builds_is_encodable() {
    for action in [SessionAction::Pause, SessionAction::Resume, SessionAction::Stop] {
        for step in steps_for(action) {
            assert!(encode(&step).is_ok(), "{step:?}");
        }
    }
}

#[test]
fn resume_on_a_surface_types_the_prompt_then_waits_then_enter() {
    let io = Fake::new(true);
    assert!(run(&Target::Surface("t1".into()), &steps_for(SessionAction::Resume), Purpose::Keys, &io));
    let calls = io.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls[0].starts_with("surface t1 ["));
    assert_eq!(calls[1], "sleep 200");
    assert_eq!(calls[2], "surface t1 [0d]");
}

/// 두 번째 쓰기부터 실패하는 가짜 — 순서가 끊기는지 본다.
struct FailOnSecondWrite(RefCell<Vec<String>>);

impl Io for FailOnSecondWrite {
    fn surface(&self, id: &str, bytes: &[u8]) -> bool {
        let mut log = self.0.borrow_mut();
        log.push(format!("surface {id} {bytes:02x?}"));
        log.iter().filter(|l| l.starts_with("surface")).count() < 2
    }
    fn console(&self, _: u32, _: &[Input]) -> bool {
        false
    }
    fn kill(&self, pid: u32) -> bool {
        self.0.borrow_mut().push(format!("kill {pid}"));
        true
    }
    fn sleep(&self, ms: u64) {
        self.0.borrow_mut().push(format!("sleep {ms}"));
    }
}

#[test]
fn a_failed_write_midway_stops_the_sequence_and_does_not_kill_a_tab_session() {
    let io = FailOnSecondWrite(RefCell::new(Vec::new()));
    assert!(!run(&Target::Surface("t1".into()), &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    // Esc 는 갔고, 400 ms 쉬고, /exit 쓰기가 실패해 Enter 는 가지 않는다
    assert_eq!(io.0.borrow().clone(), vec!["surface t1 [1b]", "sleep 400", "surface t1 [2f, 65, 78, 69, 74]"]);
}

#[test]
fn console_stop_succeeds_without_a_kill_when_input_works() {
    let io = Fake::new(true);
    assert!(run(&Target::Console(7), &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert_eq!(io.calls(), vec!["console 7 5"]);
}

/// 콘솔 입력은 되고 끝내기만 막힌 가짜가 아니라, 입력도 끝내기도 모두 막힌 가짜.
struct AllRefuse;

impl Io for AllRefuse {
    fn surface(&self, _: &str, _: &[u8]) -> bool {
        false
    }
    fn console(&self, _: u32, _: &[Input]) -> bool {
        false
    }
    fn kill(&self, _: u32) -> bool {
        false
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn console_stop_reports_failure_when_input_and_kill_both_fail() {
    assert!(!run(&Target::Console(7), &steps_for(SessionAction::Stop), Purpose::Stop, &AllRefuse));
    assert!(!run(&Target::Kill(7), &[], Purpose::Stop, &AllRefuse));
}

/// 콘솔로 간 입력을 글자로 옮겨 적는 가짜.
struct CaptureConsole(RefCell<Vec<String>>);

impl Io for CaptureConsole {
    fn surface(&self, _: &str, _: &[u8]) -> bool {
        false
    }
    fn console(&self, _: u32, inputs: &[Input]) -> bool {
        *self.0.borrow_mut() = inputs
            .iter()
            .map(|i| match i {
                Input::Escape => "esc".to_string(),
                Input::Enter => "enter".to_string(),
                Input::Text(t) => format!("text:{t}"),
                Input::Pause(d) => format!("pause:{}", d.as_millis()),
            })
            .collect();
        true
    }
    fn kill(&self, _: u32) -> bool {
        false
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn console_input_maps_each_step_kind() {
    let io = CaptureConsole(RefCell::new(Vec::new()));
    assert!(run(&Target::Console(7), &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert_eq!(io.0.borrow().clone(), vec!["esc", "pause:400", "text:/exit", "pause:200", "enter"]);
}

#[test]
fn kill_target_ignores_steps_but_still_rejects_bad_text() {
    let io = Fake::new(true);
    assert!(run(&Target::Kill(9), &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert_eq!(io.calls(), vec!["kill 9"]);
    let bad = Fake::new(true);
    assert!(!run(&Target::Kill(9), &[Step::Text("x\ny".into())], Purpose::Stop, &bad));
    assert!(bad.calls().is_empty());
}

#[test]
fn stop_on_a_background_or_ended_session_never_reaches_the_writer_or_killer() {
    let io = Fake::new(true);
    let background = session(5, "working", None, Some("--bg"), true, None);
    let target = drive_target(&background, &[], true, Purpose::Stop);
    assert_eq!(target, Target::None);
    assert!(!run(&target, &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    let ended = drive_target(&session(5, "ended", Some("t1"), None, false, None), &runs(&["t1"]), true, Purpose::Stop);
    assert!(!run(&ended, &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert!(io.calls().is_empty());
}

#[test]
fn pause_reports_failure_for_every_target_kind_when_nothing_is_accepted() {
    // 일시중지 표시는 run 이 true 일 때만 쓰이므로, 거부하는 모든 목적지에서 false 여야 한다
    let pause = steps_for(SessionAction::Pause);
    for target in [Target::Surface("t1".into()), Target::Console(7), Target::None] {
        assert!(!run(&target, &pause, Purpose::Keys, &AllRefuse), "{target:?}");
    }
}

#[test]
fn rename_safe_prefers_the_claude_status_over_the_record() {
    assert!(rename_safe("working", Some("busy"), Some("waiting")));
    assert!(!rename_safe("idle", None, Some("waiting")));
    assert!(rename_safe("idle", Some("idle"), Some("waiting")));
    assert!(!rename_safe("paused", None, None));
    assert!(!rename_safe("", None, None));
}

#[test]
fn step_spec_edges() {
    let parse = |json: &str| serde_json::from_str::<StepSpec>(json);
    assert_eq!(parse(r#"{"wait":5000}"#).unwrap().into_step(), Ok(Step::Wait(5000)));
    assert!(parse(r#"{"wait":5001}"#).unwrap().into_step().is_err());
    assert_eq!(parse(r#"{"wait":0}"#).unwrap().into_step(), Ok(Step::Wait(0)));
    assert!(parse(r#"{"key":"tab"}"#).is_err());
    assert!(parse(r#"{"wait":-1}"#).is_err());
    assert!(parse(r#"{}"#).is_err());
    // 제어 문자가 든 글은 구문 분석은 통과하고 encode 에서 거부된다
    let step = parse(r#"{"text":"a\u001bb"}"#).unwrap().into_step().unwrap();
    assert!(encode(&step).is_err());
}

mod guard;
