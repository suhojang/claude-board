//! 트레이 아이콘 (SPEC §5.10) — macOS 메뉴 막대 아이콘·앱 메뉴 대응.
//! 왼쪽 클릭 = 보드 열고 닫기, 툴팁 = 상태별 개수, 승인 대기가 있으면 주황 점·작업 중만 있으면 파란 점 아이콘.
//! 오른쪽 클릭 메뉴에는 터미널 펼치기(체크)와 보드 위치(오른쪽·왼쪽)가 있다.

use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{include_image, AppHandle, Manager, Wry};

use crate::board::Board;
use crate::model::Counts;

/// 캐릭터 메뉴 — SPEC §7.1 순서
pub const MASCOT_NAMES: [(&str, &str); 8] = [
    ("tralalero", "트랄랄레로 트랄랄라"),
    ("bombardiro", "봄바르디로 크로코딜로"),
    ("tungtung", "퉁 퉁 퉁 사후르"),
    ("lirili", "리릴리 라릴라"),
    ("patapim", "브르르 브르르 파타핌"),
    ("boneca", "보네카 암발라부"),
    ("cappuccino", "카푸치노 아사시노"),
    ("foreman", "현장 감독"),
];

const POSITION_NAMES: [(&str, &str); 5] = [
    ("center", "중앙 (기본)"),
    ("top-left", "왼쪽 상단"),
    ("top-right", "오른쪽 상단"),
    ("bottom-left", "왼쪽 하단"),
    ("bottom-right", "오른쪽 하단"),
];

const SIDE_NAMES: [(&str, &str); 2] = [("right", "오른쪽 (기본)"), ("left", "왼쪽")];

pub struct Tray {
    icon: TrayIcon<Wry>,
    terminal: CheckMenuItem<Wry>,
    pin: CheckMenuItem<Wry>,
    mascots: Vec<(&'static str, CheckMenuItem<Wry>)>,
    positions: Vec<(&'static str, CheckMenuItem<Wry>)>,
    sides: Vec<(&'static str, CheckMenuItem<Wry>)>,
}

fn icon_for(counts: &Counts) -> Image<'static> {
    if counts.waiting > 0 {
        include_image!("icons/tray-waiting.png")
    } else if counts.working > 0 {
        include_image!("icons/tray-working.png")
    } else {
        include_image!("icons/tray.png")
    }
}

pub fn tooltip(counts: &Counts) -> String {
    format!("Claude Board — 승인 대기 {} · 작업 중 {}", counts.waiting, counts.working)
}

fn refs<'a>(list: &'a [(&str, CheckMenuItem<Wry>)]) -> Vec<&'a dyn IsMenuItem<Wry>> {
    list.iter().map(|(_, i)| i as &dyn IsMenuItem<Wry>).collect()
}

pub fn build(app: &AppHandle) -> tauri::Result<Tray> {
    let pin = CheckMenuItem::with_id(app, "pin", "항상 위에 고정", true, true, None::<&str>)?;
    let mascots: Vec<(&str, CheckMenuItem<Wry>)> = MASCOT_NAMES
        .iter()
        .map(|(k, name)| Ok((*k, CheckMenuItem::with_id(app, format!("mascot:{k}"), name, true, false, None::<&str>)?)))
        .collect::<tauri::Result<_>>()?;
    let positions: Vec<(&str, CheckMenuItem<Wry>)> = POSITION_NAMES
        .iter()
        .map(|(k, name)| Ok((*k, CheckMenuItem::with_id(app, format!("pos:{k}"), name, true, false, None::<&str>)?)))
        .collect::<tauri::Result<_>>()?;
    let sides: Vec<(&str, CheckMenuItem<Wry>)> = SIDE_NAMES
        .iter()
        .map(|(k, name)| Ok((*k, CheckMenuItem::with_id(app, format!("side:{k}"), name, true, false, None::<&str>)?)))
        .collect::<tauri::Result<_>>()?;
    let terminal = CheckMenuItem::with_id(app, "terminal", "터미널 펼치기 (Ctrl+Alt+T)", true, false, None::<&str>)?;
    let mascot_menu = Submenu::with_id_and_items(app, "mascots", "캐릭터", true, &refs(&mascots))?;
    let position_menu = Submenu::with_id_and_items(app, "positions", "알림 위치", true, &refs(&positions))?;
    let side_menu = Submenu::with_id_and_items(app, "sides", "보드 위치", true, &refs(&sides))?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "세션 보드 보기", true, None::<&str>)?,
            &terminal,
            &pin,
            &mascot_menu,
            &position_menu,
            &side_menu,
            &MenuItem::with_id(app, "update", "업데이트 확인…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?,
        ],
    )?;
    let icon = TrayIconBuilder::with_id("main")
        .icon(include_image!("icons/tray.png"))
        .tooltip("Claude Board")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                crate::window::toggle(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(Tray { icon, terminal, pin, mascots, positions, sides })
}

fn on_menu(app: &AppHandle, e: MenuEvent) {
    let Some(board) = app.try_state::<Arc<Board>>().map(|s| s.inner().clone()) else { return };
    let id = e.id().as_ref().to_string();
    match id.as_str() {
        "show" => crate::window::show(app, true),
        "quit" => crate::shutdown::quit(app),
        "update" => {
            // 결과는 보드의 알림 줄에 뜨므로 보드를 먼저 연다
            crate::window::show(app, true);
            std::thread::spawn(move || crate::updater::check(&board, true));
        }
        // 설정 저장·다시 읽기는 메인 스레드 밖에서
        _ => {
            std::thread::spawn(move || {
                if id == "pin" {
                    let on = !board.settings.lock().unwrap().pinned;
                    board.set_pinned(on);
                } else if id == "terminal" {
                    let on = !board.settings.lock().unwrap().terminal_expanded;
                    board.set_terminal(on);
                } else if let Some(side) = id.strip_prefix("side:") {
                    board.set_side(crate::window::Side::parse(side));
                } else if let Some(kind) = id.strip_prefix("mascot:") {
                    board.update_settings(|s| s.mascot = kind.to_string());
                    board.publish();
                } else if let Some(pos) = id.strip_prefix("pos:") {
                    let _ = crate::settings::set_notify_position(pos);
                    board.publish();
                }
                // 체크 메뉴는 누르면 저절로 뒤집히므로 지금 설정으로 다시 맞춘다
                board.refresh_tray(true);
            });
        }
    }
}

/// 트레이에 보일 상태.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayState {
    pub counts: Counts,
    pub pinned: bool,
    pub mascot: String,
    pub position: String,
    pub terminal_expanded: bool,
    pub side: String,
}

impl Tray {
    /// 아이콘·툴팁·체크를 고친다 — 메인 스레드에서 부른다.
    pub fn update(&self, s: &TrayState) {
        let _ = self.icon.set_icon(Some(icon_for(&s.counts)));
        let _ = self.icon.set_tooltip(Some(tooltip(&s.counts)));
        let _ = self.pin.set_checked(s.pinned);
        let _ = self.terminal.set_checked(s.terminal_expanded);
        for (k, item) in &self.sides {
            let _ = item.set_checked(*k == s.side);
        }
        for (k, item) in &self.mascots {
            let _ = item.set_checked(*k == s.mascot);
        }
        for (k, item) in &self.positions {
            let _ = item.set_checked(*k == s.position);
        }
    }
}
