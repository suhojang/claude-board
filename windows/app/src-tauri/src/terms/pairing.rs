//! 세션 pid ↔ 보드 터미널 짝짓기 어댑터 (W2 설계 §1·§4) — 순수 규칙은 `claude_board_core::place` 가 갖고,
//! 여기는 Win32 사실(부모·생성 시각·콘솔 호스트·실행 인자·터미널 창)을 `ProcessView` 로 이어 주고
//! 세션마다 `Placement`·앱 이름과 "지금 claude 가 도는 터미널" 목록을 만든다. 3초 백그라운드 확인이 부른다.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use claude_board_core::place::{self, BoardTerm, Host, Mode, ProcessView, SessionProc};
use claude_board_core::proc::{self, ProcessTable};
use claude_board_core::console;

use crate::model::PidPlace;

/// 캐시가 이만큼 넘게 쌓이면 비운다(끝난 pid 의 항목이 쌓이는 것을 막는다).
const CACHE_CAP: usize = 1024;
/// 터미널 셸의 자손을 찾을 때 훑는 프로세스 수 상한.
const DESCENDANT_CAP: usize = 256;

/// 한 번의 확인 결과.
#[derive(Debug, Default)]
pub struct Pairing {
    /// 살아 있는 세션 pid → 자리.
    pub places: HashMap<u32, PidPlace>,
    /// 지금 claude 가 도는 보드 터미널 `TermId`.
    pub runs_claude: Vec<String>,
    /// 셸이 프롬프트에 있는(자식 프로세스가 없는) 보드 터미널 `TermId` — 끝난 세션을 그 탭에서 이어 열 수 있다.
    pub at_prompt: Vec<String>,
    /// 콘솔에 붙어 보지 않을 pid — 보드 터미널 세션(자기 ConPTY 에 붙지 않는다)과 콘솔을 못 읽은 pid.
    pub no_attach: HashSet<u32>,
}

/// 터미널 창 소유자 조회 결과 — 보드 자신의 창이면 이름이 없다.
#[derive(Debug, Clone, PartialEq)]
enum Owner {
    Board,
    App(String),
}

/// 확인 사이에 남기는 캐시. 키는 (pid, 생성 시각 비트) — pid 재사용을 다른 프로세스로 본다.
#[derive(Default)]
struct Cache {
    modes: HashMap<(u32, u64), Option<Mode>>,
    owners: HashMap<(u32, u64), Owner>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn key(pid: u32) -> Option<(u32, u64)> {
    proc::creation_time(pid).map(|c| (pid, c.to_bits()))
}

/// Win32 로 읽는 `ProcessView`. 실행 인자는 캐시한다(조상마다 3초마다 읽지 않는다).
pub struct Win32View<'a> {
    table: &'a ProcessTable,
}

impl ProcessView for Win32View<'_> {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.table.parent(pid)
    }

    fn ctime(&self, pid: u32) -> Option<f64> {
        proc::creation_time(pid)
    }

    fn host(&self, pid: u32) -> Host {
        proc::console_host(pid)
    }

    fn name(&self, pid: u32) -> Option<String> {
        self.table.name(pid).map(str::to_string)
    }

    fn mode(&self, pid: u32) -> Option<Mode> {
        let k = key(pid);
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = k.and_then(|k| c.modes.get(&k)) {
            return *m;
        }
        let m = proc::command_line(pid).and_then(|argv| place::mode(&argv));
        if let Some(k) = k {
            if c.modes.len() > CACHE_CAP {
                c.modes.clear();
            }
            c.modes.insert(k, m);
        }
        m
    }
}

/// 그 pid 의 터미널 창 소유자. 창을 못 찾으면 None(캐시하지 않고 다음 확인에 다시 본다).
fn window_owner(table: &ProcessTable, pid: u32) -> Option<Owner> {
    let k = key(pid);
    if let Some(o) = k.and_then(|k| cache().lock().unwrap_or_else(|e| e.into_inner()).owners.get(&k).cloned()) {
        return Some(o);
    }
    let window_pid = crate::win::window_pid(console::terminal_window(pid)?);
    let owner = if window_pid == std::process::id() {
        Owner::Board
    } else {
        Owner::App(place::app_name(table.name(window_pid).unwrap_or("")).to_string())
    };
    if let Some(k) = k {
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if c.owners.len() > CACHE_CAP {
            c.owners.clear();
        }
        c.owners.insert(k, owner.clone());
    }
    Some(owner)
}

/// 세션들을 보드 터미널과 짝짓는다(Win32). 규칙은 `place::place`, 여기는 연결뿐이다.
pub fn pair_all(terms: &[BoardTerm], sessions: &[SessionProc], table: &ProcessTable) -> Pairing {
    let view = Win32View { table };
    let is_claude = |pid: u32| table.name(pid).is_some_and(|n| proc::is_claude_exe(n, || proc::command_line(pid)));
    let app_of = |pid: u32| match window_owner(table, pid) {
        Some(Owner::App(name)) => Some(name),
        _ => None,
    };
    compute(terms, sessions, table, &view, &is_claude, &app_of)
}

/// 짝짓기 본체 — Win32 를 모르고 포트로만 읽는다(가짜 `ProcessView` 로 시험한다).
fn compute(
    terms: &[BoardTerm],
    sessions: &[SessionProc],
    table: &ProcessTable,
    view: &impl ProcessView,
    is_claude: &dyn Fn(u32) -> bool,
    app_of: &dyn Fn(u32) -> Option<String>,
) -> Pairing {
    let mut out = Pairing::default();
    for sp in sessions.iter().filter(|s| !s.ended && s.pid > 0) {
        if out.places.contains_key(&sp.pid) {
            continue;
        }
        let p = place::place(sp, terms, view);
        let host = view.host(sp.pid);
        // 보드 밖 터미널 창의 앱 이름 — 대화형이고 보드 터미널이 아니며 콘솔이 읽힐 때만(보드 자신의 창이면 이름 없음)
        let app_name = if !sp.headless && p.tty.is_none() && matches!(host, Host::Console(_)) { app_of(sp.pid) } else { None };
        if p.tty.is_some() || host == Host::Unknown {
            out.no_attach.insert(sp.pid);
        }
        let mode = p.mode.map(|m| if m == Mode::Print { "-p" } else { "--bg" });
        out.places.insert(sp.pid, PidPlace { tty: p.tty, tab_ancestor: p.tab_ancestor, mode, app_name });
    }
    for term in terms {
        let claudes: Vec<u32> = descendants(table, term.shell_pid).into_iter().filter(|&p| is_claude(p)).collect();
        if place::runs_claude(term, &claudes, view) {
            out.runs_claude.push(term.id.clone());
        }
        if place::at_prompt(term.shell_pid, view, &table.children(term.shell_pid)) {
            out.at_prompt.push(term.id.clone());
        }
    }
    out
}

/// `root` 의 자손 pid(너비 우선, 상한 `DESCENDANT_CAP`).
fn descendants(table: &ProcessTable, root: u32) -> Vec<u32> {
    let mut seen = vec![root];
    let mut i = 0;
    while i < seen.len() && seen.len() < DESCENDANT_CAP {
        for c in table.children(seen[i]) {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        i += 1;
    }
    seen.split_off(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험용 가짜 — pid → (부모, 생성 시각, 호스트).
    struct Fake(HashMap<u32, (u32, f64, Host)>);

    impl ProcessView for Fake {
        fn parent(&self, pid: u32) -> Option<u32> {
            self.0.get(&pid).map(|e| e.0)
        }
        fn ctime(&self, pid: u32) -> Option<f64> {
            self.0.get(&pid).map(|e| e.1)
        }
        fn host(&self, pid: u32) -> Host {
            self.0.get(&pid).map_or(Host::Unknown, |e| e.2)
        }
        fn mode(&self, _pid: u32) -> Option<Mode> {
            None
        }
        fn name(&self, _pid: u32) -> Option<String> {
            None
        }
    }

    fn term(id: &str, shell: u32, host: Host) -> BoardTerm {
        BoardTerm { id: id.into(), shell_pid: shell, shell_ctime: Some(1.0), host }
    }

    fn sess(pid: u32, headless: bool) -> SessionProc {
        SessionProc { pid, headless, ended: false }
    }

    /// 셸 10(호스트 100) → claude 20(호스트 100), 밖 터미널의 claude 30(호스트 200), 콘솔을 못 읽은 40.
    fn world() -> (ProcessTable, Fake) {
        let table = ProcessTable::from_entries([(10, 1, "cmd.exe".into()), (20, 10, "claude.exe".into()), (30, 1, "claude.exe".into()), (40, 1, "x.exe".into())]);
        let view = Fake(HashMap::from([
            (10, (1, 1.0, Host::Console(100))),
            (20, (10, 2.0, Host::Console(100))),
            (30, (1, 2.0, Host::Console(200))),
            (40, (1, 2.0, Host::Unknown)),
        ]));
        (table, view)
    }

    #[test]
    fn pairs_by_console_and_names_foreign_terminal() {
        let (table, view) = world();
        let terms = [term("t1", 10, Host::Console(100))];
        let sessions = [sess(20, false), sess(30, false), sess(40, false)];
        let app = |pid: u32| (pid == 30).then(|| "Windows Terminal".to_string());
        let out = compute(&terms, &sessions, &table, &view, &|p| table.name(p) == Some("claude.exe"), &app);
        assert_eq!(out.places[&20].tty.as_deref(), Some("t1"));
        assert_eq!(out.places[&20].app_name, None);
        assert_eq!(out.places[&30].tty, None);
        assert_eq!(out.places[&30].app_name.as_deref(), Some("Windows Terminal"));
        // 콘솔을 못 읽은 pid 는 이름도 붙이지 않는다
        assert_eq!(out.places[&40].app_name, None);
        assert_eq!(out.runs_claude, vec!["t1".to_string()]);
    }

    #[test]
    fn at_prompt_follows_shell_children() {
        let (table, view) = world();
        // t1 의 셸(10)에는 claude(20)가 자식으로 돈다 — 프롬프트가 아니다. t2 의 셸(30)은 자식이 없다
        let terms = [term("t1", 10, Host::Console(100)), term("t2", 30, Host::Console(200))];
        let out = compute(&terms, &[], &table, &view, &|_| false, &|_| None);
        assert_eq!(out.at_prompt, vec!["t2".to_string()]);
    }

    #[test]
    fn skips_attach_for_tab_sessions_and_unknown_hosts() {
        let (table, view) = world();
        let terms = [term("t1", 10, Host::Console(100))];
        let sessions = [sess(20, false), sess(30, false), sess(40, false)];
        let out = compute(&terms, &sessions, &table, &view, &|_| false, &|_| None);
        assert_eq!(out.no_attach, HashSet::from([20, 40]));
        assert!(out.runs_claude.is_empty());
    }

    #[test]
    fn ended_and_pidless_sessions_get_nothing() {
        let (table, view) = world();
        let terms = [term("t1", 10, Host::Console(100))];
        let ended = SessionProc { pid: 20, headless: false, ended: true };
        let out = compute(&terms, &[ended, sess(0, false)], &table, &view, &|_| false, &|_| None);
        assert!(out.places.is_empty() && out.no_attach.is_empty());
    }

    #[test]
    fn headless_session_gets_no_app_name() {
        let (table, view) = world();
        let out = compute(&[], &[sess(30, true)], &table, &view, &|_| false, &|_| Some("x".into()));
        assert_eq!(out.places[&30].app_name, None);
    }

    #[test]
    fn descendants_walks_down_and_stops_on_cycles() {
        let table = ProcessTable::from_entries([(2, 1, "a".into()), (3, 2, "b".into()), (4, 3, "c".into()), (1, 4, "root".into())]);
        assert_eq!(descendants(&table, 1), vec![2, 3, 4]);
    }
}

/// 실제 Windows 에서만 — ConPTY 셸과 자식 프로세스로 콘솔 호스트 짝짓기를 확인한다(`--test-threads=1`).
#[cfg(all(test, windows))]
mod real;
