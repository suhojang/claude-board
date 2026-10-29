//! 보드 — 상태를 모아 1초마다 프론트에 보내고(`board://snapshot`), 명령(SPEC §6.2)을 받는다.
//!
//! 1초 틱: 세션 다시 읽기 → 상태 묶음 보내기 → 트레이 갱신. 3초마다: 계정 파일·로그인 확인, 자동 전환,
//! 백그라운드 프로세스 확인(셸 수·콘솔에 붙을 수 있는지·터미널 실행 여부·Windows Terminal 새 실행).
//! 30초마다 사용 한도, 1시간마다 업데이트 확인. 오래 걸리는 일은 모두 따로 스레드에서 한다.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use claude_board_core::proc::{self, ProcessTable};
use claude_board_core::{clock, console, paths};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindowBuilder};

use crate::accounts::{self, AccountPaths, Accounts};
use crate::autoswitch::{self, AutoSwitch};
use crate::model::{Counts, Group, Session, Snapshot};
use crate::settings::{self, set_in, toggle, Frame, Settings};
use crate::store::{Probe, Store};
use crate::terminal::{self, Terminals};
use crate::terms::{pairing, TermHub};
use claude_board_core::place::SessionProc;
use crate::updater::{self, Updater};
use crate::usage::{self, Next, Usage};
use crate::util::{board_log, ms};
use crate::{tray, window};

pub const SNAPSHOT_EVENT: &str = "board://snapshot";
/// 터미널을 앞으로 불렀을 때 — 프론트가 활성 터미널에 포커스를 준다
pub const TERMINAL_FOCUS_EVENT: &str = "board://terminal-focus";

#[derive(Default)]
pub struct View {
    pub groups: Vec<Group>,
    pub all_muted: bool,
}

pub struct Board {
    pub app: AppHandle,
    settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    pub(crate) store: Mutex<Store>,
    view: Mutex<View>,
    pub(crate) probe: Mutex<Probe>,
    pub open_queue: Mutex<crate::open_session::OpenQueue>,
    probe_busy: AtomicBool,
    pub usage: Mutex<Usage>,
    pub accounts: Mutex<Accounts>,
    /// 계정 파일을 고치는 일(전환·추가 확인·지우기)을 한 번에 하나씩 — macOS 의 직렬 큐
    pub account_io: Mutex<()>,
    pub account_paths: AccountPaths,
    pub updater: Mutex<Updater>,
    pub auto: Mutex<AutoSwitch>,
    pub terminals: Mutex<Terminals>,
    claude_version: Mutex<(f64, Option<String>)>,
    /// 지난 확인 때의 Windows Terminal pid — 새로 생기면 보드를 보인다
    wt_pids: Mutex<Option<HashSet<u32>>>,
    /// 보드가 처음 세션을 읽기 전에 받은 `--action`
    pending_action: Mutex<Option<(String, String)>>,
    tray: Mutex<Option<tray::Tray>>,
    /// 트레이에 마지막으로 넣은 상태 — 바뀔 때만 다시 넣는다
    tray_shown: Mutex<Option<tray::TrayState>>,
    /// 창 위치·크기가 바뀐 시각 — 움직임이 멎으면 저장한다
    frame_dirty: Mutex<Option<f64>>,
}

impl Board {
    fn new(app: AppHandle) -> Arc<Self> {
        let settings_path = settings::settings_path(app.path().app_config_dir().ok());
        let settings = Settings::load(&settings_path);
        let home = paths::home().to_string_lossy().into_owned();
        let store = Store::new(paths::sessions_dir(), &paths::notifier_dir(), paths::claude_dir(), home);
        let account_paths = AccountPaths::system();
        let mut acc = Accounts::default();
        acc.load_saved(&account_paths);
        acc.load_current(&account_paths, true);
        Arc::new(Board {
            app,
            settings_path,
            settings: Mutex::new(settings),
            store: Mutex::new(store),
            view: Mutex::new(View::default()),
            probe: Mutex::new(Probe::default()),
            open_queue: Mutex::default(),
            probe_busy: AtomicBool::new(false),
            usage: Mutex::new(Usage::new(std::env::temp_dir().join(".claude-usage-cache.json"))),
            accounts: Mutex::new(acc),
            account_io: Mutex::new(()),
            account_paths,
            updater: Mutex::new(Updater::new()),
            auto: Mutex::new(AutoSwitch::default()),
            terminals: Mutex::new(Terminals::default()),
            claude_version: Mutex::new((f64::NEG_INFINITY, None)),
            wt_pids: Mutex::new(None),
            pending_action: Mutex::new(None),
            tray: Mutex::new(None),
            tray_shown: Mutex::new(None),
            frame_dirty: Mutex::new(None),
        })
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.store.lock().unwrap().sessions_dir().to_path_buf()
    }

    /// 설정을 고치고 저장한다.
    pub fn update_settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let mut s = self.settings.lock().unwrap();
        let r = f(&mut s);
        if let Err(e) = s.save(&self.settings_path) {
            board_log(&format!("보드 설정을 저장하지 못했습니다: {e}"));
        }
        r
    }

    /// 지금 보이는 모든 세션.
    pub fn sessions(&self) -> Vec<Session> {
        self.view.lock().unwrap().groups.iter().flat_map(|g| g.sessions.iter().cloned()).collect()
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        self.view.lock().unwrap().groups.iter().flat_map(|g| &g.sessions).find(|s| s.id == id).cloned()
    }

    /// 사용 한도 카드 아래 안내 문구(7초).
    pub fn flash(&self, text: impl Into<String>, error: bool) {
        self.accounts.lock().unwrap().flash(text, error);
    }

    /// 세션 파일을 다시 읽는다.
    fn reload(&self) {
        let mut store = self.store.lock().unwrap();
        let probe = self.probe.lock().unwrap().clone();
        let mute_all = paths::mute_all_file().exists();
        let mut st = self.settings.lock().unwrap();
        let r = store.reload(&mut st, &probe, &proc::record_alive, mute_all, clock::now());
        if r.settings_changed {
            if let Err(e) = st.save(&self.settings_path) {
                board_log(&format!("보드 설정을 저장하지 못했습니다: {e}"));
            }
        }
        drop(st);
        drop(store);
        *self.view.lock().unwrap() = View { groups: r.groups, all_muted: r.all_muted };
    }

    fn claude_version(&self, now: f64) -> Option<String> {
        let mut c = self.claude_version.lock().unwrap();
        if now - c.0 > 60.0 {
            *c = (now, terminal::read_claude_version(&paths::home()));
        }
        c.1.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = clock::now();
        let (groups, all_muted) = {
            let v = self.view.lock().unwrap();
            (v.groups.clone(), v.all_muted)
        };
        let (settings, auto_switch) = {
            let s = self.settings.lock().unwrap();
            (s.view(settings::notify_position()), s.auto_switch)
        };
        // probe 잠금은 한 번만 잡는다 — 구조체 리터럴 안에서 두 번 잡으면 첫 임시 값이 함수 끝까지 살아 같은 스레드가 멈춘다
        let (runs_claude, at_prompt) = {
            let p = self.probe.lock().unwrap();
            (p.runs_claude.clone(), p.at_prompt.clone())
        };
        Snapshot {
            now: ms(now),
            counts: Counts::of(&groups),
            groups,
            all_muted,
            usage: self.usage.lock().unwrap().view(),
            accounts: self.accounts.lock().unwrap().view(auto_switch, now),
            updater: self.updater.lock().unwrap().view(now),
            settings,
            claude_version: self.claude_version(now),
            terminals: self.terminals.lock().unwrap().view(),
            runs_claude,
            at_prompt,
        }
    }

    /// 창이 다른 창 위에 있어야 하는지 다시 정해 적용한다 — 고정을 바꿨을 때와 창 크기·위치가 멈췄을 때 부른다.
    pub fn refresh_on_top(&self) {
        let (pinned, expanded) = {
            let s = self.settings.lock().unwrap();
            (s.pinned, s.terminal_expanded)
        };
        // 덮는지 모르는 창(최소화·화면 밖)은 지금 상태를 그대로 둔다 — 되살린 뒤 크기가 멈추면 다시 정한다
        if let Some(w) = window::main_window(&self.app) {
            if let Some(covering) = window::covering_now(&w) {
                let _ = w.set_always_on_top(window::on_top(pinned, expanded, covering));
            }
        }
    }

    /// 다시 읽고 프론트에 보낸다 — 1초마다, 그리고 상태를 바꾼 명령 뒤에 바로.
    pub fn publish(&self) {
        self.reload();
        let snap = self.snapshot();
        let _ = self.app.emit(SNAPSHOT_EVENT, &snap);
        crate::util::trace(|| {
            let c = &snap.counts;
            format!("{SNAPSHOT_EVENT} 그룹 {} · 승인 대기 {} 작업 중 {} 대기 {} 일시중지 {} 종료 {}", snap.groups.len(), c.waiting, c.working, c.idle, c.paused, c.ended)
        });
        self.refresh_tray_with(&snap.counts, false);
    }

    pub fn refresh_tray(&self, force: bool) {
        let counts = Counts::of(&self.view.lock().unwrap().groups);
        self.refresh_tray_with(&counts, force);
    }

    /// 트레이는 메인 스레드에서만 고친다 — 다른 스레드에서 고치면 메인 스레드 응답을 기다리는 동안
    /// 메인 스레드(메뉴 처리)가 같은 잠금을 기다려 서로 멈출 수 있다.
    fn refresh_tray_with(&self, counts: &Counts, force: bool) {
        let (pinned, terminal_expanded, terminal_label) = {
            let s = self.settings.lock().unwrap();
            (s.pinned, s.terminal_expanded, tray::terminal_label(crate::hotkeys::terminal_combo_display(&s.shortcuts).as_deref()))
        };
        let state = tray::TrayState { counts: counts.clone(), pinned, terminal_expanded, terminal_label };
        {
            let mut shown = self.tray_shown.lock().unwrap();
            if !force && shown.as_ref() == Some(&state) {
                return;
            }
            *shown = Some(state.clone());
        }
        let app = self.app.clone();
        let _ = self.app.run_on_main_thread(move || {
            if let Some(board) = app.try_state::<Arc<Board>>() {
                if let Some(t) = board.tray.lock().unwrap().as_ref() {
                    t.update(&state);
                }
            }
        });
    }

    pub fn set_pinned(&self, on: bool) {
        self.update_settings(|s| {
            s.pinned = on;
        });
        // 접은 창과 60% 로 줄인 펼친 창은 고정을 따르고, 작업 영역을 덮은 펼친 창만 쉰다(mac 3.1.9)
        self.refresh_on_top();
        self.publish();
    }

    /// 계정을 바꾼 뒤(손으로·자동) — 앞 계정 값이 든 사용량 캐시를 버리고 새로 받는다. 자동 전환은 이때부터 settle 을 센다.
    pub fn on_account_switched(self: &Arc<Self>) {
        self.usage.lock().unwrap().reset(clock::now());
        self.update_settings(|s| s.auto_switch_last = clock::now());
        self.auto.lock().unwrap().switched();
        self.refresh_usage();
    }

    pub fn refresh_usage(self: &Arc<Self>) {
        let Next::Fetch(generation) = self.usage.lock().unwrap().refresh(clock::now()) else { return };
        let board = self.clone();
        std::thread::spawn(move || {
            let (data, expired) = usage::fetch(&paths::credentials_file());
            let current = board.usage.lock().unwrap().finish(generation, data, expired);
            // 받는 사이 계정을 바꿨으면 앞 계정 값이므로 버리고 다시 받는다
            if !current {
                board.refresh_usage();
            }
            board.publish();
        });
    }

    /// 네이티브 확인 대화상자(보드 창 위). UI 스레드에서 부르지 않는다.
    pub fn confirm(&self, title: &str, text: &str, ok: &str, cancel: &str) -> bool {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let mut d = self
            .app
            .dialog()
            .message(text)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(ok.to_string(), cancel.to_string()));
        if let Some(w) = window::main_window(&self.app).filter(|w| w.is_visible().unwrap_or(false)) {
            d = d.parent(&w);
        }
        d.blocking_show()
    }

    /// 세션 동작(`session_action`·`--action`). `carry_on` 은 `reopen` 에만 쓰인다.
    pub fn perform_action(self: &Arc<Self>, action: &str, id: &str, confirm: bool, carry_on: Option<bool>) {
        match self.session(id) {
            Some(s) => terminal::perform(self, action, carry_on, s, confirm),
            None => board_log(&format!("세션 동작 {action}: 세션을 찾지 못했습니다 ({id})")),
        }
    }

    /// 프로세스 확인 — 3초마다 백그라운드에서. 셸 수·콘솔에 붙을 수 있는 pid·터미널 실행 여부·Windows Terminal 새 실행.
    fn probe_in_background(self: &Arc<Self>) {
        if self.probe_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let procs: Vec<SessionProc> = self
            .sessions()
            .iter()
            .filter(|s| s.pid > 0 && s.state != "ended")
            .map(|s| SessionProc { pid: s.pid, headless: s.headless, ended: false })
            .collect();
        let board = self.clone();
        std::thread::spawn(move || {
            let table = ProcessTable::capture();
            let mut probe = Probe::default();
            let pairing = pairing::pair_all(&board.app.state::<TermHub>().hosts(), &procs, &table);
            for &SessionProc { pid, headless, .. } in &procs {
                let n = proc::shell_count(&table, pid);
                if n > 0 {
                    probe.shells.insert(pid, n);
                }
                if !headless && !pairing.no_attach.contains(&pid) && console::can_attach(pid) {
                    probe.drivable.insert(pid);
                }
            }
            (probe.places, probe.runs_claude, probe.at_prompt) = (pairing.places, pairing.runs_claude, pairing.at_prompt);
            *board.probe.lock().unwrap() = probe;
            board.terminals.lock().unwrap().refresh(&table, clock::now());
            board.watch_windows_terminal(&table);
            board.probe_busy.store(false, Ordering::SeqCst);
        });
    }

    /// Windows Terminal 이 새로 실행되면 보드를 보이게 한다(포커스는 그대로) — macOS 의 Ghostty 연동 대응.
    fn watch_windows_terminal(&self, table: &ProcessTable) {
        let now: HashSet<u32> = table.pids_named("WindowsTerminal.exe").into_iter().collect();
        let mut last = self.wt_pids.lock().unwrap();
        let started = last.as_ref().is_some_and(|prev| now.iter().any(|p| !prev.contains(p)));
        *last = Some(now);
        drop(last);
        if started {
            window::show(&self.app, false);
        }
    }

    /// 창이 움직임을 멈추면 위치를 저장한다. 미니가 아닌 크기면 ⤢ 로 돌아갈 자리로도 기억한다.
    /// 터미널을 펼친 동안은 창이 작업 영역 전체이거나 사용자가 끈 크기라 저장하지 않는다(접으면 늘 보드 쪽에 붙는다).
    fn save_frame_if_settled(&self, now: f64) {
        let mut dirty = self.frame_dirty.lock().unwrap();
        if !dirty.is_some_and(|t| now - t > 0.7) {
            return;
        }
        *dirty = None;
        drop(dirty);
        // 크기·위치가 멈췄으니 펼친 창이 작업 영역을 덮는지 다시 보고 맨 위 고정을 맞춘다
        self.refresh_on_top();
        if self.settings.lock().unwrap().terminal_expanded {
            return;
        }
        let Some(win) = window::main_window(&self.app) else { return };
        if win.is_minimized().unwrap_or(false) || !win.is_visible().unwrap_or(false) {
            return;
        }
        let (Some(f), Ok(scale)) = (window::frame(&win), win.scale_factor()) else { return };
        if f.w < 1.0 {
            return;
        }
        self.update_settings(|s| {
            s.window_frame = Some(f);
            if !s.board_mini && f.w >= window::BOARD_MIN_WIDTH * scale {
                s.board_restore_frame = Some(f);
            }
        });
    }

    fn mark_frame_dirty(&self) {
        *self.frame_dirty.lock().unwrap() = Some(clock::now());
    }

    /// 보드 쪽·칸 폭·고정 여부 — 창 모양을 계산하는 데 쓴다.
    fn layout(&self) -> (window::Side, f64, bool) {
        let s = self.settings.lock().unwrap();
        (window::Side::parse(&s.board_side), s.board_pane_width, s.pinned)
    }

    /// 창을 펼친 모양(작업 영역 전체)이나 접은 모양(보드 쪽에 붙인 폭)으로 바꾼다.
    /// 설정을 먼저 고친다 — 창이 움직이며 생기는 이벤트가 프레임 저장 가드를 지날 때 새 값을 봐야 한다.
    /// 접은 모양은 ⤢ 로 돌아갈 자리로도 기억한다. 돌려주는 값은 새 창 프레임이다.
    fn apply_terminal(&self, on: bool) -> Option<Frame> {
        let win = window::main_window(&self.app)?;
        let m = window::monitor_of(&win)?;
        self.update_settings(|s| {
            s.terminal_expanded = on;
            if on {
                s.mini_from_expanded = false;
            }
        });
        if win.is_minimized().unwrap_or(false) {
            let _ = win.unminimize();
        }
        let (side, width, pinned) = self.layout();
        let shape = window::shape(&m, on, side, width, pinned);
        window::apply_shape(&win, &shape);
        if !on {
            self.update_settings(|s| s.board_restore_frame = Some(shape.frame));
        }
        self.publish();
        Some(shape.frame)
    }

    /// 터미널 펼침·접힘(제목 막대 단추·트레이). 이미 그 상태면 아무것도 하지 않는다.
    pub fn set_terminal(&self, on: bool) {
        if self.settings.lock().unwrap().terminal_expanded == on {
            return;
        }
        if on {
            self.expand_terminal();
        } else {
            self.apply_terminal(false);
        }
    }

    /// 터미널을 펼치고 보드를 앞으로 가져와 활성 터미널에 포커스를 준다.
    fn expand_terminal(&self) {
        self.apply_terminal(true);
        window::show(&self.app, true);
        let _ = self.app.emit(TERMINAL_FOCUS_EVENT, ());
    }

    /// 배율이 바뀌면(설정에서 바꿈·다른 배율 모니터로 끌어감) 펼친 창이 윈도우가 제안한 크기로 줄어 작업 영역과 가장자리마다 1px 어긋난다.
    /// 접힌 창은 논리 크기 그대로 붙어 있으니 건드리지 않고, 펼친 창만 작업 영역 전체로 다시 맞춘다. tao 가 제안 크기를 적용한 뒤에 돌도록 잠깐 기다린다.
    fn refit_expanded(self: &Arc<Self>) {
        let board = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            let app = board.app.clone();
            let _ = app.run_on_main_thread(move || {
                if board.settings.lock().unwrap().terminal_expanded {
                    board.apply_terminal(true);
                }
            });
        });
    }

    /// 터미널을 접고 보드 쪽에 붙인다 — 이미 접혀 있어도 다시 붙인다. 앞으로 가져오되 포커스는 가져가지 않는다.
    pub(crate) fn snap_right(&self) {
        self.apply_terminal(false);
        window::show(&self.app, false);
    }

    /// Ctrl+Alt+T — 접힘이면 펼치고, 펼침이고 앞이면 접고, 펼침이지만 뒤에 있으면 앞으로 가져온다.
    pub(crate) fn toggle_terminal(&self) {
        let expanded = self.settings.lock().unwrap().terminal_expanded;
        let front = window::main_window(&self.app).is_some_and(|w| window::is_front(&w));
        match window::term_key(expanded, front) {
            window::TermKey::Expand => self.expand_terminal(),
            window::TermKey::Collapse => {
                self.apply_terminal(false);
            }
            window::TermKey::Focus => {
                window::show(&self.app, true);
                let _ = self.app.emit(TERMINAL_FOCUS_EVENT, ());
            }
        }
    }

    /// 보드 쪽을 바꾼다. 접혀 있으면 새 쪽으로 옮기고, 펼쳐 있으면 값만 저장한다(프론트가 칸 순서를 바꾼다).
    pub fn set_side(&self, side: window::Side) {
        self.update_settings(|s| s.board_side = side.as_str().to_string());
        let expanded = self.settings.lock().unwrap().terminal_expanded;
        if let (false, Some(win)) = (expanded, window::main_window(&self.app)) {
            if let (Some(m), Some(cur)) = (window::monitor_of(&win), window::frame(&win)) {
                let f = window::side_frame(cur, &m, side, self.layout().1);
                window::set_frame(&win, f);
                if f.w >= window::BOARD_MIN_WIDTH * m.scale {
                    self.update_settings(|s| s.board_restore_frame = Some(f));
                }
            }
        }
        self.publish();
    }

    /// 터미널을 펼친 채 들어왔으면 먼저 접고 그 표지를 남긴다. 미니 프레임은 보드 쪽 위 모서리 기준이다.
    fn enter_mini(&self, rows: usize) {
        let Some(win) = window::main_window(&self.app) else { return };
        let Some(m) = window::monitor_of(&win) else { return };
        let was_expanded = self.settings.lock().unwrap().terminal_expanded;
        let cur = if was_expanded { self.apply_terminal(false) } else { window::frame(&win) };
        let Some(cur) = cur else { return };
        self.update_settings(|s| {
            if was_expanded {
                s.mini_from_expanded = true;
            }
            if cur.w >= window::BOARD_MIN_WIDTH * m.scale {
                s.board_restore_frame = Some(cur);
            }
        });
        let (side, _, _) = self.layout();
        window::set_frame(&win, window::mini_frame(cur, rows, m.scale, side));
    }

    /// 미니에서 원래 크기로 — 펼친 채 들어왔으면 다시 펼친다. 아니면 기억한 위치로, 없거나 지금 연결된 화면 밖이면 보드 쪽에 붙인다.
    fn leave_mini(&self) {
        let Some(win) = window::main_window(&self.app) else { return };
        let (saved, from_expanded) = {
            let s = self.settings.lock().unwrap();
            (s.board_restore_frame, s.mini_from_expanded)
        };
        if from_expanded {
            self.expand_terminal();
            return;
        }
        match saved.filter(|f| window::restorable(*f, &window::monitors(&self.app))) {
            Some(f) => window::set_frame(&win, f),
            None => {
                self.apply_terminal(false);
            }
        }
    }
}

// ── 명령 (SPEC §6.2 — 인자 이름은 프론트 backend.ts 와 같게) ──

type B<'a> = State<'a, Arc<Board>>;

#[tauri::command(async)]
fn get_snapshot(board: B<'_>) -> Snapshot {
    crate::util::trace(|| "get_snapshot".into());
    board.reload();
    board.snapshot()
}

#[tauri::command(async)]
fn toggle_collapsed(board: B<'_>, cwd: String) {
    board.update_settings(|s| toggle(&mut s.collapsed, &cwd));
    board.publish();
}

#[tauri::command(async)]
fn set_all_collapsed(board: B<'_>, collapse: bool) {
    let ids: Vec<String> = board.view.lock().unwrap().groups.iter().map(|g| g.id.clone()).collect();
    board.update_settings(|s| s.collapsed = if collapse { ids } else { Vec::new() });
    board.publish();
}

#[tauri::command(async)]
fn toggle_ended_open(board: B<'_>, cwd: String) {
    board.update_settings(|s| toggle(&mut s.ended_open, &cwd));
    board.publish();
}

#[tauri::command(async)]
fn toggle_pin_group(board: B<'_>, cwd: String) {
    board.update_settings(|s| toggle(&mut s.pinned_groups, &cwd));
    board.publish();
}

fn remove_ended_files(board: &Board, cwd: &str) {
    let view = board.view.lock().unwrap();
    for s in view.groups.iter().filter(|g| g.id == cwd).flat_map(|g| &g.sessions).filter(|s| s.state == "ended") {
        let _ = fs::remove_file(&s.file);
    }
}

/// 목록에서 지우기 — 종료된 세션 기록은 함께 지우고, 그 뒤에 새 활동이 생기면 다시 나타난다.
#[tauri::command(async)]
fn hide_group(board: B<'_>, cwd: String) {
    remove_ended_files(&board, &cwd);
    board.update_settings(|s| {
        s.hidden_groups.insert(cwd.clone(), clock::now());
        set_in(&mut s.pinned_groups, &cwd, false);
    });
    board.publish();
}

/// 그 작업 경로의 종료된 세션 기록을 한 번에 지운다 — 대화 기록(jsonl)은 남는다.
#[tauri::command(async)]
fn remove_ended(board: B<'_>, cwd: String) {
    remove_ended_files(&board, &cwd);
    board.publish();
}

#[tauri::command(async)]
fn remove_session(board: B<'_>, id: String) {
    if let Some(s) = board.session(&id) {
        let _ = fs::remove_file(&s.file);
    }
    board.publish();
}

#[tauri::command(async)]
fn toggle_kind_collapsed(board: B<'_>, key: String) {
    board.update_settings(|s| toggle(&mut s.kind_collapsed, &key));
    board.publish();
}

#[tauri::command(async)]
fn toggle_sub_collapsed(board: B<'_>, id: String) {
    board.update_settings(|s| toggle(&mut s.sub_collapsed, &id));
    board.publish();
}

#[tauri::command(async)]
fn set_usage_compact(board: B<'_>, on: bool) {
    board.update_settings(|s| s.usage_compact = on);
    board.publish();
}

/// 세션 알림 끄기 표지 — 세션 기록이 지워질 때 hook 의 prune 이 함께 치운다.
#[tauri::command(async)]
fn set_mute(board: B<'_>, id: String, muted: bool) {
    let mark = board.sessions_dir().join(format!("{id}.mute"));
    if muted {
        let _ = fs::write(&mark, b"");
    } else {
        let _ = fs::remove_file(&mark);
    }
    board.publish();
}

/// 세션별 표지는 건드리지 않으므로 다시 켜면 세션마다 정해 둔 상태가 그대로 돌아온다.
#[tauri::command(async)]
fn set_all_muted(board: B<'_>, on: bool) {
    let f = paths::mute_all_file();
    if on {
        if let Some(d) = f.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(&f, b"");
    } else {
        let _ = fs::remove_file(&f);
    }
    board.publish();
}

#[tauri::command(async)]
fn reveal(board: B<'_>, cwd: String) {
    board.update_settings(|s| s.reveal(&cwd));
    board.publish();
}

#[tauri::command(async)]
fn session_action(board: B<'_>, action: String, id: String, carry_on: Option<bool>) {
    board.perform_action(&action, &id, true, carry_on);
}

/// 작업 중·승인 대기 세션이 있으면 이전 계정으로 일하는 중이라고 알리고 한 번 더 묻는다.
#[tauri::command(async)]
fn switch_account(board: B<'_>, id: String) {
    let board = board.inner().clone();
    let (target, current) = {
        let acc = board.accounts.lock().unwrap();
        (acc.saved.iter().find(|a| a.id == id).cloned(), acc.current.as_ref().map(|a| a.email()))
    };
    let Some(target) = target else { return };
    let busy: Vec<Session> = board.sessions().into_iter().filter(|s| s.state == "working" || s.state == "waiting").collect();
    if !busy.is_empty() {
        let names: Vec<String> = busy
            .iter()
            .take(5)
            .map(|s| format!("• {} — {}", s.project(), if s.topic.is_empty() { state_name(&s.state) } else { &s.topic }))
            .collect();
        let more = if busy.len() > 5 { format!("\n… 외 {}개", busy.len() - 5) } else { String::new() };
        let text = format!(
            "{} 계정으로 작업 중이거나 승인을 기다리는 세션이 {}개 있습니다.\n{}{}\n\n전환하면 모든 세션이 다음 요청부터 {} 계정으로 이어서 동작합니다(30초 안에 반영). 지금 받고 있는 응답은 이전 계정으로 마무리됩니다.\n\n계정을 전환할까요?",
            current.as_deref().unwrap_or("지금"),
            busy.len(),
            names.join("\n"),
            more,
            target.email()
        );
        if !board.confirm("이전 계정으로 작업 중인 세션이 있습니다", &text, "전환", "취소") {
            return;
        }
    }
    accounts::switch_to(&board, target, None, None);
}

fn state_name(state: &str) -> &str {
    match state {
        "working" => "작업 중",
        "waiting" => "승인 대기",
        "idle" => "대기",
        "paused" => "일시중지",
        "ended" => "종료",
        other => other,
    }
}

#[tauri::command(async)]
fn cancel_add_account(board: B<'_>) {
    accounts::cancel_add(&board);
}

#[tauri::command(async)]
fn set_auto_switch(board: B<'_>, on: bool) {
    board.update_settings(|s| s.auto_switch = on);
    board.publish();
}

#[tauri::command(async)]
fn check_update(board: B<'_>, manual: bool) {
    updater::check(&board, manual);
}

#[tauri::command(async)]
fn run_update(board: B<'_>) {
    updater::run(&board);
}

#[tauri::command(async)]
fn open_changelog(app: AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().open_url(updater::CHANGELOG_URL, None::<&str>) {
        board_log(&format!("변경 내용을 열지 못했습니다: {e}"));
    }
}

#[tauri::command(async)]
fn set_pinned(board: B<'_>, on: bool) {
    board.set_pinned(on);
}

#[tauri::command(async)]
fn set_mascot(board: B<'_>, kind: String) {
    if settings::MASCOTS.contains(&kind.as_str()) {
        board.update_settings(|s| s.mascot = kind);
    }
    board.publish();
}

#[tauri::command(async)]
fn set_opacity(board: B<'_>, v: f64) {
    board.update_settings(|s| s.opacity = settings::clamp_opacity(v));
    board.publish();
}

/// 꾸미기를 끈 세션 id 가운데 세션 파일이 아직 있는 것만(입력 순서 그대로).
fn kept_md_render_off(list: &[String], exists: impl Fn(&str) -> bool) -> Vec<String> {
    list.iter().filter(|id| exists(id)).cloned().collect()
}

/// 프론트의 `pruneMdRenderOff()` — 보드를 켤 때 세션 파일이 없어진 세션의 id 를 끈 세션 목록에서 치운다(mac `pruneMdRenderOff`).
/// 상태 묶음의 세션이 아니라 세션 파일로 본다 — 지운 경로로 숨겨진 세션도 파일이 있는 한 이어 열 수 있다.
#[tauri::command(async)]
fn prune_md_render_off(board: B<'_>) {
    let dir = board.sessions_dir();
    board.update_settings(|s| s.md_render_off = kept_md_render_off(&s.md_render_off, |id| dir.join(format!("{id}.json")).exists()));
}

/// 설정 화면이 열리고 닫히는 움직임 — `slide` · `fade` · `none`, 모르는 값은 받지 않는다.
#[tauri::command(async)]
fn set_settings_transition(board: B<'_>, value: String) {
    if settings::TRANSITIONS.contains(&value.as_str()) {
        board.update_settings(|s| s.settings_transition = value);
    }
    board.publish();
}

/// 새 세션의 기본 명령(비면 프런트가 `claude --dangerously-skip-permissions` 로 쓴다)과 답변 꾸미기 기본값.
#[tauri::command(async)]
fn set_new_session_defaults(board: B<'_>, command: String, md_render: bool) {
    board.update_settings(|s| {
        s.new_session_command = command.trim().to_string();
        s.new_session_md_render = md_render;
    });
    board.publish();
}

#[tauri::command(async)]
fn set_auto_update_check(board: B<'_>, on: bool) {
    board.update_settings(|s| s.auto_update_check = on);
    board.publish();
    // 켜면 바로 한 번 확인한다(mac) — 끄는 동안 못 본 새 버전이 있을 수 있다
    if on {
        updater::check(&board, false);
    }
}

#[tauri::command(async)]
fn set_notify_position(board: B<'_>, pos: String) {
    if let Err(e) = settings::set_notify_position(&pos) {
        board_log(&format!("알림 위치를 저장하지 못했습니다: {e}"));
    }
    board.publish();
}

/// 미니로 둔 채 끄면 다음에도 그 크기로 연다(§5.9 boardMini) — 프론트가 단계가 바뀔 때 알린다.
#[tauri::command(async)]
fn set_board_mini(board: B<'_>, on: bool) {
    board.update_settings(|s| s.board_mini = on);
    board.publish();
}

#[tauri::command(async)]
fn open_terminal(board: B<'_>, id: String) {
    terminal::open_terminal(&board, id);
}

/// 터미널 펼침·접힘 — 창 크기·위치·항상 위는 백엔드가 바꾼다. 이미 그 상태면 아무것도 하지 않는다.
#[tauri::command(async)]
fn set_terminal_expanded(board: B<'_>, on: bool) {
    board.set_terminal(on);
}

#[tauri::command(async)]
fn set_board_side(board: B<'_>, side: String) {
    board.set_side(window::Side::parse(&side));
}

/// 경계를 놓을 때 보드 칸 폭(논리 px)을 저장한다.
#[tauri::command(async)]
fn set_board_width(board: B<'_>, width: f64) {
    board.update_settings(|s| s.board_pane_width = settings::clamp_pane_width(width));
    board.publish();
}

#[tauri::command(async)]
fn enter_mini(board: B<'_>, rows: usize) {
    board.enter_mini(rows);
}

#[tauri::command(async)]
fn leave_mini(board: B<'_>) {
    board.leave_mini();
}

#[tauri::command(async)]
fn snap_right(board: B<'_>) {
    board.snap_right();
}

#[tauri::command(async)]
fn hide_board(app: AppHandle) {
    if let Some(w) = window::main_window(&app) {
        let _ = w.hide();
    }
}

#[tauri::command(async)]
fn minimize_board(app: AppHandle) {
    if let Some(w) = window::main_window(&app) {
        let _ = w.minimize();
    }
}

#[tauri::command(async)]
fn log(text: String) {
    board_log(&format!("[화면] {text}"));
}

// ── 실행 ────────────────────────────────────────────────────

/// 명령줄에서 `--action <동작> <세션 id>` — macOS 판과 같은 이름(pause·resume·stop·reopen·focus) 그대로
/// (resume 은 일시중지한 세션 재개, reopen 은 이어서 다시 열기. 이름 뜻은 `session_action.rs`).
pub fn action_arg(argv: &[String]) -> Option<(String, String)> {
    let i = argv.iter().position(|a| a == "--action")?;
    let action = match argv.get(i + 1)?.as_str() {
        a @ ("pause" | "resume" | "stop" | "reopen" | "focus") => a,
        _ => return None,
    };
    Some((action.to_string(), argv.get(i + 2)?.clone()))
}

/// 두 번째 실행 — 첫 보드를 앞으로(`--show` 와 같음), `--action` 이면 동작을 넘긴다(중단은 확인 없이).
fn second_instance(app: &AppHandle, argv: Vec<String>) {
    let Some(board) = app.try_state::<Arc<Board>>().map(|s| s.inner().clone()) else { return };
    match action_arg(&argv) {
        Some((action, id)) => {
            // 알림을 눌러 온 focus 는 그 세션이 보드 밖(다른 터미널·백그라운드)이거나 못 찾아도 보드가 보이게 한다 — 포커스는 가져가지 않는다
            if shows_board_for(&action) {
                window::show(app, false);
            }
            queue_action(&board, action, id)
        }
        None => window::show(app, !argv.iter().any(|a| a == "--background")),
    }
}

/// `--action` 이 보드 창을 보이게 하는가 — 알림 클릭이 거는 `focus` 만. 세션을 찾지 못하거나 백그라운드 세션이면 그 동작이 아무 화면도 열지 않아,
/// 예전 `--show` 처럼 보드가 안 보이는 일이 없게 한다. 일시중지·재개·중단은 이미 보이는 보드에서 누른 것이라 창을 건드리지 않는다.
fn shows_board_for(action: &str) -> bool {
    action == "focus"
}

fn queue_action(board: &Arc<Board>, action: String, id: String) {
    if board.session(&id).is_some() {
        board.perform_action(&action, &id, false, None);
    } else {
        *board.pending_action.lock().unwrap() = Some((action, id));
    }
}

fn create_window(app: &AppHandle, board: &Arc<Board>, background: bool) -> tauri::Result<()> {
    let cfg = app.config().app.windows.iter().find(|w| w.label == "main").cloned().ok_or(tauri::Error::WindowNotFound)?;
    let win = WebviewWindowBuilder::from_config(app, &cfg)?.build()?;
    let launch = {
        let s = board.settings.lock().unwrap();
        window::launch_frame(&window::monitors(app), &s)
    };
    if let Some(l) = launch {
        window::apply_shape(&win, &l.shape);
        let scale = win.scale_factor().unwrap_or(1.0);
        board.update_settings(|s| {
            s.terminal_expanded = l.expanded;
            // ⤢ 로 돌아갈 자리는 접은 채 연 창만(펼친 창과 미니는 아니다)
            if !l.expanded && !s.board_mini && l.shape.frame.w >= window::BOARD_MIN_WIDTH * scale {
                s.board_restore_frame = Some(l.shape.frame);
            }
        });
    } else {
        let (expanded, pinned) = {
            let s = board.settings.lock().unwrap();
            (s.terminal_expanded, s.pinned)
        };
        let _ = win.set_always_on_top(window::on_top(pinned, expanded, window::covering_now(&win).unwrap_or(true)));
    }
    window::apply_vibrancy(&win);
    // 셸 설정·로그인 자동 실행에서 --background 로 띄우면 창만 보이고 포커스는 지금 앱에 남긴다
    if background {
        window::show_inactive(&win);
    } else {
        let _ = win.show();
        let _ = win.set_focus();
    }
    let b = board.clone();
    win.on_window_event(move |e| match e {
        // 닫기 = 숨기기(앱은 트레이에 남는다)
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if let Some(w) = window::main_window(&b.app) {
                let _ = w.hide();
            }
        }
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => b.mark_frame_dirty(),
        tauri::WindowEvent::ScaleFactorChanged { .. } => b.refit_expanded(),
        _ => {}
    });
    Ok(())
}

/// 1초 틱.
fn start_ticker(board: Arc<Board>) {
    std::thread::spawn(move || {
        let mut n: u64 = 0;
        loop {
            let now = clock::now();
            if n.is_multiple_of(3) {
                board.probe_in_background();
                accounts::tick(&board);
                autoswitch::tick(&board);
            }
            if n.is_multiple_of(30) {
                board.refresh_usage();
            }
            if n > 0 && n.is_multiple_of(3600) {
                updater::check(&board, false);
            }
            board.publish();
            if let Some((action, id)) = board.pending_action.lock().unwrap().take() {
                if board.session(&id).is_some() {
                    board.perform_action(&action, &id, false, None);
                } else {
                    board_log(&format!("--action {action}: 세션을 찾지 못했습니다 ({id})"));
                }
            }
            board.save_frame_if_settled(now);
            n += 1;
            // 다음 초 경계까지 잔다 — 경과 시간 표시가 고르게 넘어가게
            let frac = clock::now().fract();
            std::thread::sleep(Duration::from_secs_f64((1.0 - frac).clamp(0.05, 1.0)));
        }
    });
}

pub fn run(argv: Vec<String>, context: tauri::Context<tauri::Wry>) {
    let background = argv.iter().any(|a| a == "--background");
    let first_action = action_arg(&argv);
    let result = tauri::Builder::default()
        // 단일 실행은 맨 먼저 — 두 번째 실행은 여기서 첫 보드에 인자를 넘기고 끝난다
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| second_instance(app, argv)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            toggle_collapsed,
            set_all_collapsed,
            toggle_ended_open,
            toggle_pin_group,
            hide_group,
            remove_ended,
            remove_session,
            toggle_kind_collapsed,
            toggle_sub_collapsed,
            set_usage_compact,
            set_mute,
            set_all_muted,
            reveal,
            session_action,
            crate::drive::drive_session,
            crate::drive::rename_check,
            crate::open_session::show_board,
            crate::open_session::take_open_session,
            crate::open_session::reopen_done,
            crate::win::double_click_time,
            switch_account,
            cancel_add_account,
            set_auto_switch,
            check_update,
            run_update,
            open_changelog,
            set_pinned,
            set_mascot,
            set_opacity,
            set_notify_position,
            set_board_mini,
            open_terminal,
            enter_mini,
            leave_mini,
            snap_right,
            hide_board,
            minimize_board,
            log,
            set_terminal_expanded,
            set_board_side,
            set_board_width,
            crate::shutdown::save_tabs,
            crate::shutdown::load_tabs,
            crate::shutdown::answer_shutdown,
            crate::shutdown::shutdown_dialog_shown,
            crate::shutdown::lifecycle_started,
            crate::shutdown::existing_dirs,
            prune_md_render_off,
            set_settings_transition,
            set_new_session_defaults,
            set_auto_update_check,
            crate::hotkeys::set_shortcut,
            crate::hotkeys::reset_shortcuts,
            crate::hotkeys::sync_hotkeys,
            crate::login_item::login_launch,
            crate::login_item::set_login_launch,
            crate::account_login::begin_add_account,
            crate::account_login::submit_login_code,
            crate::account_login::open_login_url,
            crate::account_login::delete_account,
            crate::proc_wait::pids_alive,
            crate::proc_wait::wait_exit,
            crate::mdrender::md_render_dir,
            crate::mdrender::remember_md_render_off,
            crate::test_claude::test_claude_prefix,
            crate::terms::term_spawn,
            crate::terms::term_list,
            crate::terms::term_attach,
            crate::terms::term_ack,
            crate::terms::term_write,
            crate::terms::term_write_bin,
            crate::terms::term_resize,
            crate::terms::term_kill,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let board = Board::new(handle.clone());
            app.manage(board.clone());
            app.manage(TermHub::default());
            let paths = board.account_paths.clone();
            std::thread::spawn(move || accounts::cleanup_slots(&paths));
            board.reload();
            create_window(&handle, &board, background)?;
            match tray::build(&handle) {
                Ok(t) => *board.tray.lock().unwrap() = Some(t),
                Err(e) => board_log(&format!("트레이 아이콘을 만들지 못했습니다: {e}")),
            }
            crate::hotkeys::register(&handle);
            if let Some((action, id)) = first_action.clone() {
                *board.pending_action.lock().unwrap() = Some((action, id));
            }
            updater::check(&board, false);
            start_ticker(board);
            Ok(())
        })
        .build(context);
    match result {
        Ok(app) => app.run(|app, e| match e {
            // 창을 닫아도(숨겨도) 앱은 트레이에 남는다 — 종료는 트레이 메뉴의 "종료"(코드가 있는 exit)로만
            tauri::RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
            tauri::RunEvent::Exit => {
                // 보드와 함께 셸과 그 자손(claude 등)을 끝낸다
                if let Some(hub) = app.try_state::<TermHub>() {
                    hub.kill_all();
                }
                if let Some(board) = app.try_state::<Arc<Board>>() {
                    board.save_frame_if_settled(f64::INFINITY);
                }
            }
            _ => {}
        }),
        Err(e) => board_log(&format!("보드를 띄우지 못했습니다: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_args() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(action_arg(&v(&["ClaudeBoard.exe", "--action", "stop", "abc"])), Some(("stop".into(), "abc".into())));
        assert_eq!(action_arg(&v(&["ClaudeBoard.exe", "--action", "resume", "abc"])), Some(("resume".into(), "abc".into())));
        assert_eq!(action_arg(&v(&["ClaudeBoard.exe", "--action", "reopen", "abc"])), Some(("reopen".into(), "abc".into())));
        assert_eq!(action_arg(&v(&["ClaudeBoard.exe", "--action", "activate", "abc"])), None);
        assert_eq!(action_arg(&v(&["ClaudeBoard.exe", "--action", "stop"])), None);
        assert_eq!(action_arg(&v(&["ClaudeBoard.exe", "--show"])), None);
    }

    #[test]
    fn md_render_off_keeps_only_sessions_with_a_file() {
        let list: Vec<String> = ["a", "gone", "b"].iter().map(|s| s.to_string()).collect();
        assert_eq!(kept_md_render_off(&list, |id| id != "gone"), ["a", "b"]);
        assert!(kept_md_render_off(&[], |_| true).is_empty());
        assert!(kept_md_render_off(&list, |_| false).is_empty());
    }

    #[test]
    fn only_focus_shows_the_board_for_an_action() {
        assert!(shows_board_for("focus"));
        for action in ["pause", "resume", "stop", "reopen"] {
            assert!(!shows_board_for(action), "{action}");
        }
    }

    /// `snapshot` 안에서 같은 잠금을 두 번 잡으면(구조체 리터럴 속 임시 값이 함수 끝까지 산다) 같은 스레드가 멈춘다 — 필드마다 한 번만 잡는지 본다.
    #[test]
    fn snapshot_locks_each_field_once() {
        let mut body = String::new();
        for line in include_str!("board.rs").lines().skip_while(|l| !l.contains("pub fn snapshot(&self)")) {
            body.push_str(line);
            body.push('\n');
            if line.trim_end() == "    }" {
                break;
            }
        }
        assert!(body.lines().count() > 10, "snapshot 본문을 찾지 못했다");
        for field in ["view", "settings", "probe", "terminals", "store", "claude_version"] {
            let needle = format!("self.{field}.lock()");
            assert!(body.matches(&needle).count() <= 1, "{field} 잠금을 두 번 이상 잡는다");
        }
    }
}
