//! `screens` 의 시험 — 손으로 만든 상태(터미널·세션·짝짓기·앱 값)에서 열기·닫기 화면 값이 나오는지 본다.
//! 주 세션 고르기·busy·앞 프로그램·이름 모으기의 규칙을 하나씩 붙든다. 실제 짝짓기 규칙은
//! `terms/pairing.rs` 의 시험이 이미 보므로 여기선 짝짓기 표를 손으로 채워 넣는다.

use std::path::PathBuf;

use super::screens::{close_tab_screens, open_tab_screens, PaneApp};
use crate::model::{PidPlace, Session};
use crate::open_tab::close::CloseTabForeground;
use crate::open_tab::plan::OpenTabScreen;
use crate::terms::pairing::Pairing;
use crate::terms::TermInfo;

/// 가짜 세션 — `drive/tests.rs` 의 모양. 이 시험이 보는 값(id·pid·tty·이름·기록)만 인자로 받는다.
fn session(id: &str, pid: u32, tty: Option<&str>, name: Option<&str>, updated: i64, since: i64) -> Session {
    Session {
        id: id.into(),
        cwd: String::new(),
        state: "idle".into(),
        since,
        updated,
        headless: false,
        title: String::new(),
        topic: String::new(),
        message: String::new(),
        name: name.map(str::to_string),
        ctx: None,
        brain: None,
        subagents: Vec::new(),
        shells: 0,
        paused: false,
        muted: false,
        limit_at: None,
        can_drive: false,
        place: place(tty),
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

/// 세션이 세션 파일에 펴 둔 자리 — tty 만 이 시험에서 쓴다.
fn place(tty: Option<&str>) -> PidPlace {
    PidPlace { tty: tty.map(str::to_string), tab_ancestor: None, mode: None, app_name: None }
}

/// 가짜 터미널 — 이 시험이 보는 값(id·cwd)만 인자로.
fn term(id: &str, cwd: &str) -> TermInfo {
    TermInfo {
        id: id.into(),
        pid: 100,
        shell: "pwsh".into(),
        cwd: cwd.into(),
        cols: 80,
        rows: 24,
        status: "running",
        exit_code: None,
        windows_build: None,
        oldest_seq: 0,
        next_seq: 0,
    }
}

/// 가짜 짝짓기 — 살아 있는 pid 의 자리와 지금 징후 목록만 채운다.
fn pairing(pairs: &[(u32, &str)], runs: &[&str], prompt: &[&str]) -> Pairing {
    Pairing {
        places: pairs.iter().map(|(pid, tty)| (*pid, place(Some(tty)))).collect(),
        runs_claude: runs.iter().map(|s| s.to_string()).collect(),
        at_prompt: prompt.iter().map(|s| s.to_string()).collect(),
        no_attach: Default::default(),
    }
}

/// 가짜 앱 값 — id 만 꼭 넣고 나머지는 기본값에서 바꿀 것만 바꾼다.
fn app(id: &str) -> PaneApp {
    PaneApp { id: id.into(), ..Default::default() }
}

/// 짝지어 온 세션이 화면 값을 이끈다 — 세션 id·pid·이름(세션 이름 먼저, 앱 이름 뒤)은 짝지어 온 쪽을 따르고,
/// 화면이 기억하던 세션 id 는 짝지어 온 세션이 있으면 물러난다. claude 가 도는 화면은 busy 다.
#[test]
fn paired_pane_reports_the_session_it_runs() {
    let terms = [term("t1", "C:\\work")];
    let sessions = [session("s1", 42, Some("t1"), Some("역할"), 10, 5)];
    let pairing = pairing(&[(42, "t1")], &["t1"], &[]);
    let app = [PaneApp {
        names: vec!["바꿀이름".into(), "탭이름".into()],
        session_id: Some("s-old".into()),
        tab: "탭1".into(),
        ..app("t1")
    }];
    let out = open_tab_screens(&terms, &sessions, &pairing, &app);
    assert_eq!(
        out,
        vec![OpenTabScreen {
            id: "t1".into(),
            session_id: Some("s1".into()),
            cwd: "C:\\work".into(),
            pid: Some(42),
            names: vec!["역할".to_string(), "바꿀이름".to_string(), "탭이름".to_string()],
            busy: true,
            takeover: false,
        }]
    );
}

/// 짝지어 온 세션이 없는 화면은 세션 이름·pid 가 비고, 화면이 기억하던 세션 id 가 답에 나온다.
#[test]
fn a_shell_pane_without_a_session_keeps_the_remembered_session_id() {
    let terms = [term("t2", "C:\\tmp")];
    // tty 가 없는 세션(밖 터미널)은 화면에 짝지어 오지 않는다
    let sessions = [session("s-outside", 9, None, Some("밖"), 10, 5)];
    let pairing = pairing(&[], &[], &[]);
    let app = [PaneApp { session_id: Some("s-old".into()), ..app("t2") }];
    let out = open_tab_screens(&terms, &sessions, &pairing, &app);
    assert_eq!(out[0].session_id.as_deref(), Some("s-old"));
    assert_eq!(out[0].pid, None);
    assert_eq!(out[0].names, Vec::<String>::new());
    assert!(!out[0].busy);
    assert!(!out[0].takeover);
}

/// 같은 화면에 세션이 둘 이상 짝지어 오면 마지막으로 갱신된 세션이 주 세션이다 — id 순서와 무관하게
/// updated 가 앞선다(프론트 place.ts ttyWinner).
#[test]
fn two_sessions_on_the_same_pane_pick_the_latest_update() {
    let terms = [term("t1", "C:\\w")];
    let sessions = [
        session("b", 41, Some("t1"), None, 1, 9), // id 는 크지만 기록이 늦은 세션 — 져야 한다
        session("a", 42, Some("t1"), None, 2, 0), // updated 가 큰 쪽이 이긴다
    ];
    let pairing = pairing(&[(41, "t1"), (42, "t1")], &[], &[]);
    let out = open_tab_screens(&terms, &sessions, &pairing, &[]);
    assert_eq!(out[0].session_id.as_deref(), Some("a"));
}

/// updated 까지 같으면 since 가 이기고, since 까지 같으면 id 가 큰 쪽이 이긴다.
#[test]
fn sessions_updated_at_the_same_time_pick_the_later_since_then_id() {
    let terms = [term("t1", "C:\\w")];
    let since_tie = pairing(&[(41, "t1"), (42, "t1")], &[], &[]);
    let sessions = [
        session("a", 41, Some("t1"), None, 5, 1),
        session("z", 42, Some("t1"), None, 5, 2), // since 로 이긴다
    ];
    let out = open_tab_screens(&terms, &sessions, &since_tie, &[]);
    assert_eq!(out[0].session_id.as_deref(), Some("z"));

    // since 까지 같으면 id 가 큰 쪽
    let id_tie = pairing(&[(43, "t1"), (44, "t1")], &[], &[]);
    let sessions = [
        session("a", 43, Some("t1"), None, 5, 2),
        session("b", 44, Some("t1"), None, 5, 2),
    ];
    let out = open_tab_screens(&terms, &sessions, &id_tie, &[]);
    assert_eq!(out[0].session_id.as_deref(), Some("b"));
}

/// 짝짓기 표에 자리가 없는 세션(끝났거나 pid 가 없는 것 — `terms/pairing.rs` 가 걸러 둔다)은
/// 기록이 얼마나 새로워도 화면의 주 세션이 되지 못한다.
#[test]
fn a_session_without_a_pairing_place_is_not_the_panes_session() {
    let terms = [term("t1", "C:\\w")];
    let sessions = [
        session("ended", 40, Some("t1"), Some("끝남"), 99, 9), // 표에 자리가 없다(끝난 세션)
        session("live", 42, Some("t1"), Some("살아"), 2, 2),
        session("pidless", 0, Some("t1"), Some("무pid"), 100, 100), // 표에 자리가 없다(pid 없음)
    ];
    let pairing = pairing(&[(42, "t1")], &[], &[]);
    let out = open_tab_screens(&terms, &sessions, &pairing, &[]);
    assert_eq!(out[0].session_id.as_deref(), Some("live"));
}

/// busy 는 claude 가 도는 화면만 true 다 — 프롬프트에 서 있으면(claude 가 끝난 뒤) false 고,
/// 아무 징후도 없는 화면도 false 다. 앱 값이 없는 화면은 기본값(이어받기 없음)으로 나온다.
#[test]
fn busy_is_true_only_while_claude_runs_and_not_at_the_prompt() {
    let terms = [term("t1", "C:\\a"), term("t2", "C:\\b"), term("t3", "C:\\c")];
    // t1: claude 가 돈다. t2: 프롬프트 징후와 claude 징후가 같이 남았다. t3: 징후 없음
    let pairing = pairing(&[], &["t1", "t2"], &["t2"]);
    let out = open_tab_screens(&terms, &[], &pairing, &[]);
    assert!(out[0].busy);
    assert!(!out[1].busy);
    assert!(!out[2].busy);
    assert!(!out[0].takeover && !out[1].takeover && !out[2].takeover);
}

/// 닫기 화면 값은 탭 이름·바깥 열기·이어받기·나눈 탭·--stop 표지를 앱 값 그대로 옮기고,
/// 앱 값이 없는 화면은 기본값이 된다. 화면 순서는 들어온 터미널 목록을 따른다.
#[test]
fn close_tab_reports_the_tabs_name_flags_and_stopping() {
    let terms = [term("t2", "C:\\b"), term("t1", "C:\\a")]; // 목록 순서(t2 먼저)를 그대로 옮기는지 본다
    let sessions = [session("s1", 42, Some("t1"), Some("역할"), 1, 1)];
    let pairing = pairing(&[(42, "t1")], &[], &[]);
    let app = [PaneApp { tab: "웹앱".into(), external: true, takeover: true, split: true, ..app("t1") }];
    let stopping = ["t1".to_string()];
    let out = close_tab_screens(&terms, &sessions, &pairing, &app, &stopping);
    assert_eq!(out.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["t2", "t1"]);

    let t1 = &out[1];
    assert_eq!(t1.tab, "웹앱");
    assert!(t1.external && t1.takeover && t1.split);
    assert!(t1.stopping);
    assert_eq!(t1.session_id.as_deref(), Some("s1")); // 짝지어 온 세션
    assert_eq!(t1.names, vec!["역할".to_string()]);

    let t2 = &out[0]; // 앱 값이 없는 화면은 기본값
    assert_eq!(t2.tab, "");
    assert!(!t2.external && !t2.takeover && !t2.split);
    assert!(!t2.stopping);
}

/// 앞 프로그램은 막 뜨는 중 → 프롬프트 → claude → 그 밖의 순서로 정해진다 —
/// 프롬프트 화면에 claude 징후가 같이 남아 있어도 프롬프트가 앞선다(mac closeTabScreen 의 순서).
#[test]
fn close_tab_foreground_follows_starting_prompt_claude_other() {
    let terms = [
        term("t-start", "C:\\a"),
        term("t-prompt", "C:\\b"),
        term("t-claude", "C:\\c"),
        term("t-other", "C:\\d"),
    ];
    let app = [PaneApp { starting: true, ..app("t-start") }];
    // t-start: 막 넣음 징후 + 프롬프트 징후. t-prompt: 프롬프트 + claude 징후. t-claude: claude 만
    let pairing = pairing(&[], &["t-prompt", "t-claude"], &["t-prompt", "t-start"]);
    let out = close_tab_screens(&terms, &[], &pairing, &app, &[]);
    assert_eq!(out[0].foreground, CloseTabForeground::Starting);
    assert_eq!(out[1].foreground, CloseTabForeground::Prompt);
    assert_eq!(out[2].foreground, CloseTabForeground::Claude);
    assert_eq!(out[3].foreground, CloseTabForeground::Other);
}

/// 세션 이름이 없을 때 이름 목록은 앱 쪽 이름들만 든다 — 있을 때 세션 이름이 앞선 것(첫 시험)과 짝이다.
#[test]
fn pane_names_carry_the_session_name_then_the_app_names() {
    let terms = [term("t1", "C:\\w")];
    let sessions = [session("s1", 42, Some("t1"), None, 1, 1)];
    let pairing = pairing(&[(42, "t1")], &[], &[]);
    let app = [PaneApp { names: vec!["앱이름".into()], ..app("t1") }];
    let out = open_tab_screens(&terms, &sessions, &pairing, &app);
    assert_eq!(out[0].names, vec!["앱이름".to_string()]);
}

/// 터미널이 하나도 없으면 화면 값도 없다 — 세션·짝짓기·앱 값이 남아 있어도 화면을 지어내지 않는다.
#[test]
fn empty_input_yields_no_screens() {
    let terms: [TermInfo; 0] = [];
    let sessions = [session("s1", 42, Some("t1"), None, 1, 1)];
    let pairing = pairing(&[(42, "t1")], &["t1"], &[]);
    let app = [app("t1")];
    assert!(open_tab_screens(&terms, &sessions, &pairing, &app).is_empty());
    assert!(close_tab_screens(&terms, &sessions, &pairing, &app, &["t1".into()]).is_empty());
}
