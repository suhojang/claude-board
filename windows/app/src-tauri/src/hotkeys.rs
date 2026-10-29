//! 전역 단축키(Ctrl+Alt+B 붙이기 · Ctrl+Alt+T 터미널 펼치기·접기)와 단축키 표 저장 명령.
//!
//! 앱 안 단축키(탭 이동·새 세션 등)는 프론트(`shortcuts/dispatch.ts`)가 맞추고, 여기는 어느 앱에서든 듣는 둘(`snap`·`terminal`)과
//! 사용자가 바꾼 키(`shortcuts`)의 저장만 맡는다. 저장 모양은 `settings.rs Settings::set_shortcut` 이 쥔다 —
//! 동작 id → `[키 코드(KeyboardEvent.code), 수정키 비트(Ctrl=1 Alt=2 Shift=4 Win=8)]`, 비움 `[]`, 기본값이면 항목 없음.
//!
//! 키 표가 바뀔 때마다(`set_shortcut`·`reset_shortcuts`) 두 키를 다시 등록하고, 설정 화면이 새 키를 받는 동안(`sync_hotkeys(paused=true)`)은
//! 풀어 둔다 — 누른 키가 동작하지 않고 화면으로 들어오게. 등록하지 못한 동작 id 는 `board://hotkeys` 이벤트와 `sync_hotkeys` 반환값으로 알린다.
//! 등록·해제는 `Registrar` 로 받아 시험은 가짜로 한다(진짜 전역 단축키를 건드리지 않는다).

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::board::Board;
use crate::util::board_log;

/// 등록에 실패한 동작 id 목록을 프론트에 알리는 이벤트 — 내용은 `["snap", …]`
pub const HOTKEYS_EVENT: &str = "board://hotkeys";

const CTRL: u32 = 1;
const ALT: u32 = 2;
const SHIFT: u32 = 4;
const WIN: u32 = 8;

/// 어느 앱에서든 듣는 동작 하나와 그 기본 키
struct HotkeyAction {
    id: &'static str,
    code: &'static str,
    mods: u32,
}

const ACTIONS: [HotkeyAction; 2] = [
    HotkeyAction { id: "snap", code: "KeyB", mods: CTRL | ALT },
    HotkeyAction { id: "terminal", code: "KeyT", mods: CTRL | ALT },
];

/// 키 하나 — 저장 모양과 같다(`KeyboardEvent.code` 와 수정키 비트)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub code: String,
    pub mods: u32,
}

impl Combo {
    /// 로그에 남길 글자 — "Ctrl+Alt+KeyB"
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        for (bit, name) in [(CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift"), (WIN, "Win")] {
            if self.mods & bit != 0 {
                parts.push(name);
            }
        }
        parts.push(&self.code);
        parts.join("+")
    }
}

impl Combo {
    /// 메뉴에 보일 글자 — "Ctrl+Alt+T" (KeyT → T, Digit1 → 1)
    pub fn display(&self) -> String {
        let code = self.code.strip_prefix("Key").or_else(|| self.code.strip_prefix("Digit")).unwrap_or(&self.code);
        let mut parts: Vec<&str> = Vec::new();
        for (bit, name) in [(CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift"), (WIN, "Win")] {
            if self.mods & bit != 0 {
                parts.push(name);
            }
        }
        parts.push(code);
        parts.join("+")
    }
}

/// 트레이 메뉴의 터미널 항목에 붙일 키 — 터미널 펼치기 동작의 지금 키(비웠으면 None)
pub fn terminal_combo_display(overrides: &BTreeMap<String, Vec<Value>>) -> Option<String> {
    let action = ACTIONS.iter().find(|a| a.id == "terminal")?;
    combo_for(action, overrides).map(|c| c.display())
}

/// 그 동작의 지금 키 — 바꾼 것이 있으면 그것(비움이거나 모양이 틀리면 None), 없으면 기본 키
fn combo_for(action: &HotkeyAction, overrides: &BTreeMap<String, Vec<Value>>) -> Option<Combo> {
    match overrides.get(action.id) {
        None => Some(Combo { code: action.code.to_string(), mods: action.mods }),
        Some(saved) => match saved.as_slice() {
            [Value::String(code), mods] if !code.is_empty() => mods.as_u64().map(|m| Combo { code: code.clone(), mods: (m & 15) as u32 }),
            _ => None,
        },
    }
}

/// 전역 단축키 등록·해제 — 진짜는 플러그인, 시험은 가짜
pub trait Registrar {
    /// 등록하고 단축키 번호를 돌려준다. 다른 앱이 먼저 잡았거나 이미 등록했거나 키 모양이 틀리면 Err
    fn register(&self, combo: &Combo) -> Result<u32, String>;
    fn unregister(&self, combo: &Combo);
}

/// 등록하지 못한 동작 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub action: &'static str,
    pub combo: Combo,
    pub reason: String,
}

struct Registered {
    action: &'static str,
    combo: Combo,
    key_id: u32,
}

/// 지금 등록된 전역 단축키와 풀어 둔 상태
#[derive(Default)]
pub struct Hotkeys {
    registered: Vec<Registered>,
    paused: bool,
}

impl Hotkeys {
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// 건 것을 모두 풀고 지금 표대로 다시 건다(풀어 두는 중이면 아무것도 걸지 않는다). 걸지 못한 동작을 돌려준다
    pub fn sync(&mut self, registrar: &dyn Registrar, overrides: &BTreeMap<String, Vec<Value>>) -> Vec<Failure> {
        for old in self.registered.drain(..) {
            registrar.unregister(&old.combo);
        }
        let mut failures = Vec::new();
        if self.paused {
            return failures;
        }
        for action in &ACTIONS {
            let Some(combo) = combo_for(action, overrides) else { continue };
            match registrar.register(&combo) {
                Ok(key_id) => self.registered.push(Registered { action: action.id, combo, key_id }),
                Err(reason) => failures.push(Failure { action: action.id, combo, reason }),
            }
        }
        failures
    }

    /// 눌린 전역 단축키(번호)가 가리키는 동작
    pub fn action_for(&self, key_id: u32) -> Option<&'static str> {
        self.registered.iter().find(|r| r.key_id == key_id).map(|r| r.action)
    }
}

/// 저장 모양의 키를 플러그인의 단축키로 — 수정키 비트가 0 이면 수정키 없음, 모르는 코드는 Err
pub fn to_shortcut(combo: &Combo) -> Result<Shortcut, String> {
    let code = Code::from_str(&combo.code).map_err(|_| format!("모르는 키 코드 {}", combo.code))?;
    let mut mods = Modifiers::empty();
    for (bit, modifier) in [(CTRL, Modifiers::CONTROL), (ALT, Modifiers::ALT), (SHIFT, Modifiers::SHIFT), (WIN, Modifiers::SUPER)] {
        if combo.mods & bit != 0 {
            mods |= modifier;
        }
    }
    Ok(Shortcut::new((!mods.is_empty()).then_some(mods), code))
}

/// 진짜 전역 단축키 플러그인에 거는 `Registrar`
struct AppRegistrar<'a>(&'a AppHandle);

impl Registrar for AppRegistrar<'_> {
    fn register(&self, combo: &Combo) -> Result<u32, String> {
        let shortcut = to_shortcut(combo)?;
        // 플러그인을 못 켠 채로(register 가 일찍 돌아간 경우) 부르면 플러그인 상태가 없어 패닉하므로 먼저 확인한다
        if self.0.try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>().is_none() {
            return Err("전역 단축키 플러그인이 꺼져 있습니다".to_string());
        }
        self.0.global_shortcut().register(shortcut).map_err(|e| e.to_string())?;
        Ok(shortcut.id())
    }

    fn unregister(&self, combo: &Combo) {
        let Ok(shortcut) = to_shortcut(combo) else { return };
        if self.0.try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>().is_some() {
            let _ = self.0.global_shortcut().unregister(shortcut);
        }
    }
}

/// 앱이 쥐는 전역 단축키 상태
#[derive(Default)]
pub struct HotkeyState(Mutex<Hotkeys>);

/// 단축키 잠금을 잡은 뒤에 표를 읽어 다시 건다. 키를 받는 화면이 `sync_hotkeys(false)` 와 `set_shortcut` 을 따로 보내므로 두 명령이 겹쳐 돈다 —
/// 표를 잠금 밖에서 읽으면 먼저 읽은 낡은 표가 나중에 저장된 새 표 위에 걸린다. 잠금 안에서 읽으면 마지막으로 걸리는 것은 늘 그 시점의 최신 표다.
fn resync_locked(state: &Mutex<Hotkeys>, registrar: &dyn Registrar, read_overrides: impl FnOnce() -> BTreeMap<String, Vec<Value>>, paused: Option<bool>) -> Vec<Failure> {
    let mut hotkeys = state.lock().unwrap();
    let overrides = read_overrides();
    if let Some(paused) = paused {
        hotkeys.set_paused(paused);
    }
    hotkeys.sync(registrar, &overrides)
}

/// 지금 키 표대로 다시 걸고, 걸지 못한 동작 id 를 로그에 남기고 프론트에 알린다
fn resync(app: &AppHandle, board: &Board, paused: Option<bool>) -> Vec<String> {
    let Some(state) = app.try_state::<HotkeyState>() else { return Vec::new() };
    let failures = resync_locked(&state.0, &AppRegistrar(app), || board.settings.lock().unwrap().shortcuts.clone(), paused);
    // 다른 앱이 먼저 잡은 단축키면 등록에 실패한다 — 보드는 그대로 동작하고 로그를 남기고 설정 화면에 알린다
    for f in &failures {
        board_log(&format!("전역 단축키 {} ({}) 등록 실패 ({})", f.action, f.combo.label(), f.reason));
    }
    let ids: Vec<String> = failures.iter().map(|f| f.action.to_string()).collect();
    if let Err(e) = app.emit(HOTKEYS_EVENT, &ids) {
        board_log(&format!("{HOTKEYS_EVENT} 이벤트를 보내지 못했습니다: {e}"));
    }
    ids
}

/// 전역 단축키 플러그인을 켜고 저장한 키(없으면 Ctrl+Alt+B/T)를 등록한다. 보드를 켤 때 한 번 부른다.
pub fn register(app: &AppHandle) {
    app.manage(HotkeyState::default());
    let plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let Some(board) = app.try_state::<Arc<Board>>() else { return };
            let action = app.try_state::<HotkeyState>().and_then(|state| state.0.lock().unwrap().action_for(shortcut.id()));
            match action {
                // 터미널을 접고 보드 쪽에 붙여 앞으로 — 포커스는 지금 앱에 남긴다
                Some("snap") => board.snap_right(),
                Some("terminal") => board.toggle_terminal(),
                _ => {}
            }
        })
        .build();
    if let Err(e) = app.plugin(plugin) {
        board_log(&format!("전역 단축키 플러그인을 켜지 못했습니다: {e}"));
    }
    if let Some(board) = app.try_state::<Arc<Board>>() {
        resync(app, &board, None);
    }
}

/// 프론트의 `setShortcut(id, combo)` — 저장 모양 그대로 받는다. `combo` 가 없으면 기본값으로 돌아가고(항목 삭제), `[]` 는 비움이다.
/// 전역 단축키도 새 표대로 다시 건다.
#[tauri::command(async)]
pub fn set_shortcut(app: AppHandle, board: State<'_, Arc<Board>>, id: String, combo: Option<Vec<Value>>) {
    if !board.update_settings(|s| s.set_shortcut(&id, combo)) {
        board_log(&format!("단축키 {id} 의 키 모양이 틀려 저장하지 않았습니다"));
        return;
    }
    resync(&app, &board, None);
    board.publish();
}

/// 프론트의 `resetShortcuts()` — 바꾼 키를 모두 지워 기본값으로(mac `Shortcuts.resetAll`).
#[tauri::command(async)]
pub fn reset_shortcuts(app: AppHandle, board: State<'_, Arc<Board>>) {
    board.update_settings(|s| s.shortcuts.clear());
    resync(&app, &board, None);
    board.publish();
}

/// 프론트의 `syncHotkeys(paused)` — 설정 화면이 새 키를 받는 동안은 `paused` 로 모두 풀어 두고, 끝나면 다시 건다.
/// 등록하지 못한 동작 id 를 돌려준다(설정 > 단축키가 열릴 때 지금 상태를 읽는 데도 쓴다)
#[tauri::command(async)]
pub fn sync_hotkeys(app: AppHandle, board: State<'_, Arc<Board>>, paused: bool) -> Vec<String> {
    resync(&app, &board, Some(paused))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// 가짜 등록기 — 진짜 전역 단축키를 건드리지 않는다. `taken` 은 다른 앱이 이미 잡은 키
    #[derive(Default)]
    struct Fake {
        active: RefCell<Vec<Combo>>,
        taken: Vec<Combo>,
        log: RefCell<Vec<String>>,
    }

    impl Fake {
        fn taking(combos: &[Combo]) -> Self {
            Fake { taken: combos.to_vec(), ..Fake::default() }
        }
        fn active_labels(&self) -> Vec<String> {
            self.active.borrow().iter().map(Combo::label).collect()
        }
    }

    impl Registrar for Fake {
        fn register(&self, combo: &Combo) -> Result<u32, String> {
            self.log.borrow_mut().push(format!("+{}", combo.label()));
            if self.taken.contains(combo) || self.active.borrow().contains(combo) {
                return Err("이미 등록된 키".to_string());
            }
            self.active.borrow_mut().push(combo.clone());
            Ok(fake_id(&combo.code, combo.mods))
        }
        fn unregister(&self, combo: &Combo) {
            self.log.borrow_mut().push(format!("-{}", combo.label()));
            let mut active = self.active.borrow_mut();
            let at = active.iter().position(|c| c == combo).expect("등록하지 않은 키를 풀려 했다");
            active.remove(at);
        }
    }

    /// 가짜가 돌려주는 단축키 번호 — 키 코드 글자와 수정키로 만든다(같은 키는 같은 번호)
    fn fake_id(code: &str, mods: u32) -> u32 {
        code.bytes().map(u32::from).sum::<u32>() * 16 + mods
    }

    fn combo(code: &str, mods: u32) -> Combo {
        Combo { code: code.to_string(), mods }
    }

    fn overrides(entries: &[(&str, Vec<Value>)]) -> BTreeMap<String, Vec<Value>> {
        entries.iter().map(|(id, v)| (id.to_string(), v.clone())).collect()
    }

    fn key(code: &str, mods: u64) -> Vec<Value> {
        vec![Value::from(code), Value::from(mods)]
    }

    #[test]
    fn defaults_are_ctrl_alt_b_and_t() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        assert!(hotkeys.sync(&fake, &BTreeMap::new()).is_empty());
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyB", "Ctrl+Alt+KeyT"]);
    }

    #[test]
    fn changed_key_is_registered_and_the_old_one_released() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        hotkeys.sync(&fake, &BTreeMap::new());
        let failed = hotkeys.sync(&fake, &overrides(&[("snap", key("KeyN", 3))]));
        assert!(failed.is_empty());
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyN", "Ctrl+Alt+KeyT"]);
    }

    #[test]
    fn back_to_default_registers_ctrl_alt_b_again() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        hotkeys.sync(&fake, &overrides(&[("snap", key("KeyN", 3))]));
        hotkeys.sync(&fake, &BTreeMap::new());
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyB", "Ctrl+Alt+KeyT"]);
    }

    #[test]
    fn cleared_action_is_neither_registered_nor_failed() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        let failed = hotkeys.sync(&fake, &overrides(&[("terminal", vec![])]));
        assert!(failed.is_empty());
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyB"]);
    }

    #[test]
    fn malformed_saved_value_counts_as_cleared() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        hotkeys.sync(&fake, &overrides(&[("snap", vec![Value::from(7), Value::from(3)]), ("terminal", vec![Value::from("KeyT")])]));
        assert!(fake.active_labels().is_empty());
    }

    #[test]
    fn paused_releases_everything_and_resume_registers_again() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        hotkeys.sync(&fake, &BTreeMap::new());
        hotkeys.set_paused(true);
        assert!(hotkeys.sync(&fake, &BTreeMap::new()).is_empty());
        assert!(fake.active_labels().is_empty());
        // 풀어 둔 채 표가 바뀌어도(받는 중에 되돌리기를 눌렀다) 걸지 않는다
        hotkeys.sync(&fake, &overrides(&[("snap", key("KeyN", 3))]));
        assert!(fake.active_labels().is_empty());
        hotkeys.set_paused(false);
        hotkeys.sync(&fake, &overrides(&[("snap", key("KeyN", 3))]));
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyN", "Ctrl+Alt+KeyT"]);
    }

    #[test]
    fn key_taken_by_another_app_fails_and_the_rest_registers() {
        let fake = Fake::taking(&[combo("KeyB", CTRL | ALT)]);
        let mut hotkeys = Hotkeys::default();
        let failed = hotkeys.sync(&fake, &BTreeMap::new());
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].action, "snap");
        assert_eq!(failed[0].combo, combo("KeyB", CTRL | ALT));
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyT"]);
        // 실패한 키는 다음 번에 풀려 하지 않는다(가짜가 등록하지 않은 키를 풀면 패닉한다)
        assert_eq!(hotkeys.sync(&fake, &BTreeMap::new()).len(), 1);
    }

    #[test]
    fn changing_a_failed_key_clears_the_failure() {
        let fake = Fake::taking(&[combo("KeyB", CTRL | ALT)]);
        let mut hotkeys = Hotkeys::default();
        assert_eq!(hotkeys.sync(&fake, &BTreeMap::new()).len(), 1);
        assert!(hotkeys.sync(&fake, &overrides(&[("snap", key("KeyN", 3))])).is_empty());
    }

    #[test]
    fn same_key_for_two_actions_fails_the_later_one() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        let failed = hotkeys.sync(&fake, &overrides(&[("terminal", key("KeyB", 3))]));
        assert_eq!(failed.iter().map(|f| f.action).collect::<Vec<_>>(), ["terminal"]);
        assert_eq!(fake.active_labels(), ["Ctrl+Alt+KeyB"]);
    }

    #[test]
    fn pressed_key_id_finds_its_action() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        hotkeys.sync(&fake, &overrides(&[("snap", key("KeyN", 3))]));
        assert_eq!(hotkeys.action_for(fake_id("KeyN", 3)), Some("snap"));
        assert_eq!(hotkeys.action_for(fake_id("KeyT", 3)), Some("terminal"));
        assert_eq!(hotkeys.action_for(fake_id("KeyB", 3)), None, "released old key");
    }

    #[test]
    fn releases_only_what_it_registered_before_registering() {
        let fake = Fake::default();
        let mut hotkeys = Hotkeys::default();
        hotkeys.sync(&fake, &BTreeMap::new());
        fake.log.borrow_mut().clear();
        hotkeys.sync(&fake, &BTreeMap::new());
        assert_eq!(*fake.log.borrow(), ["-Ctrl+Alt+KeyB", "-Ctrl+Alt+KeyT", "+Ctrl+Alt+KeyB", "+Ctrl+Alt+KeyT"]);
    }

    #[test]
    fn stored_shape_converts_to_plugin_shortcut() {
        let ctrl_alt_b = to_shortcut(&combo("KeyB", CTRL | ALT)).unwrap();
        assert_eq!(ctrl_alt_b, Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyB));
        let win_shift = to_shortcut(&combo("F5", WIN | SHIFT)).unwrap();
        assert_eq!(win_shift, Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::F5));
        let bare = to_shortcut(&combo("F6", 0)).unwrap();
        assert_eq!(bare, Shortcut::new(None, Code::F6));
        let digit = to_shortcut(&combo("Digit1", CTRL)).unwrap();
        assert_eq!(digit, Shortcut::new(Some(Modifiers::CONTROL), Code::Digit1));
    }

    #[test]
    fn unknown_key_code_is_refused() {
        assert!(to_shortcut(&combo("Nope", CTRL)).is_err());
        assert!(to_shortcut(&combo("", CTRL)).is_err());
    }

    /// 스레드에 넘길 수 있는 가짜 등록기(`Fake` 는 RefCell 이라 못 넘긴다)
    #[derive(Default)]
    struct SyncFake(Mutex<Vec<Combo>>);

    impl Registrar for SyncFake {
        fn register(&self, combo: &Combo) -> Result<u32, String> {
            self.0.lock().unwrap().push(combo.clone());
            Ok(fake_id(&combo.code, combo.mods))
        }
        fn unregister(&self, combo: &Combo) {
            let mut active = self.0.lock().unwrap();
            let at = active.iter().position(|c| c == combo).expect("등록하지 않은 키를 풀려 했다");
            active.remove(at);
        }
    }

    /// 먼저 잠금을 기다리던 호출이, 그 사이 저장된 새 표를 읽는다 — 표를 잠금 밖에서 읽던 때는 낡은 표가 걸렸다
    #[test]
    fn resync_reads_the_table_after_taking_the_lock() {
        let state = Mutex::new(Hotkeys::default());
        let settings: Mutex<BTreeMap<String, Vec<Value>>> = Mutex::new(BTreeMap::new());
        let fake = SyncFake::default();
        let held = state.lock().unwrap();
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| resync_locked(&state, &fake, || settings.lock().unwrap().clone(), Some(false)));
            // 기다리는 호출이 잠금 앞에 서 있는 동안 새 표가 저장된다
            std::thread::sleep(std::time::Duration::from_millis(100));
            settings.lock().unwrap().insert("snap".to_string(), key("KeyN", 3));
            drop(held);
            assert!(waiting.join().unwrap().is_empty());
        });
        let labels: Vec<String> = fake.0.lock().unwrap().iter().map(Combo::label).collect();
        assert_eq!(labels, ["Ctrl+Alt+KeyN", "Ctrl+Alt+KeyT"]);
    }

    /// 어느 순서로 끝나든 마지막으로 걸린 것은 최신 표다(받는 중 풀기 → 새 키 저장 → 다시 걸기)
    #[test]
    fn final_registered_set_matches_latest_settings_in_any_order() {
        for order in [[0, 1], [1, 0]] {
            let state = Mutex::new(Hotkeys::default());
            let settings: Mutex<BTreeMap<String, Vec<Value>>> = Mutex::new(BTreeMap::new());
            let fake = SyncFake::default();
            resync_locked(&state, &fake, || settings.lock().unwrap().clone(), None);
            settings.lock().unwrap().insert("snap".to_string(), key("KeyN", 3));
            for step in order {
                let paused = if step == 0 { Some(false) } else { None };
                resync_locked(&state, &fake, || settings.lock().unwrap().clone(), paused);
            }
            let mut labels: Vec<String> = fake.0.lock().unwrap().iter().map(Combo::label).collect();
            labels.sort();
            assert_eq!(labels, ["Ctrl+Alt+KeyN", "Ctrl+Alt+KeyT"], "order {order:?}");
        }
    }

    #[test]
    fn terminal_menu_key_follows_the_table() {
        assert_eq!(terminal_combo_display(&BTreeMap::new()).as_deref(), Some("Ctrl+Alt+T"));
        assert_eq!(terminal_combo_display(&overrides(&[("terminal", key("Digit1", 5))])).as_deref(), Some("Ctrl+Shift+1"));
        assert_eq!(terminal_combo_display(&overrides(&[("terminal", key("F5", 0))])).as_deref(), Some("F5"));
        assert_eq!(terminal_combo_display(&overrides(&[("terminal", vec![])])), None);
        // 다른 동작의 키는 상관없다
        assert_eq!(terminal_combo_display(&overrides(&[("snap", key("KeyN", 3))])).as_deref(), Some("Ctrl+Alt+T"));
    }

    #[test]
    fn only_four_modifier_bits_count() {
        let c = combo_for(&ACTIONS[0], &overrides(&[("snap", key("KeyN", 0xFFFF))])).unwrap();
        assert_eq!(c.mods, 15);
    }
}
