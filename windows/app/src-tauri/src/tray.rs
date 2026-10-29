//! 트레이 아이콘 (SPEC §5.10) — macOS 메뉴 막대 아이콘·앱 메뉴 대응.
//! 왼쪽 클릭 = 보드 열고 닫기, 툴팁 = 상태별 개수, 승인 대기가 있으면 주황 점·작업 중만 있으면 파란 점 아이콘.
//! 오른쪽 클릭 메뉴: 세션 보드 보기 · 터미널 펼치기(체크) · 항상 위에 고정(체크) · 설정… · 업데이트 확인… · 종료.
//! 캐릭터·알림 위치·보드 위치는 설정 화면(설정 > 일반·알림·창)에서 고른다(mac 3.2.0 이 메뉴에서 없앤 것과 같다).

use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{include_image, AppHandle, Manager, Wry};

use crate::board::Board;
use crate::model::Counts;

pub struct Tray {
    icon: TrayIcon<Wry>,
    terminal: CheckMenuItem<Wry>,
    pin: CheckMenuItem<Wry>,
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

/// 터미널 항목 글자 — 지금 단축키를 괄호로 붙인다(비운 동작이면 키 없이)
pub fn terminal_label(combo: Option<&str>) -> String {
    match combo {
        Some(c) => format!("터미널 펼치기 ({c})"),
        None => "터미널 펼치기".to_string(),
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<Tray> {
    let pin = CheckMenuItem::with_id(app, "pin", "항상 위에 고정", true, true, None::<&str>)?;
    // 처음 글자는 기본 키 — 저장한 키는 첫 `refresh_tray` 가 맞춘다
    let terminal = CheckMenuItem::with_id(app, "terminal", terminal_label(Some("Ctrl+Alt+T")), true, false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "세션 보드 보기", true, None::<&str>)?,
            &terminal,
            &pin,
            &MenuItem::with_id(app, "settings", "설정…", true, None::<&str>)?,
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
    Ok(Tray { icon, terminal, pin })
}

fn on_menu(app: &AppHandle, e: MenuEvent) {
    let Some(board) = app.try_state::<Arc<Board>>().map(|s| s.inner().clone()) else { return };
    let id = e.id().as_ref().to_string();
    match id.as_str() {
        "show" => crate::window::show(app, true),
        "quit" => crate::shutdown::quit(app),
        "settings" => {
            // 설정 화면은 보드 창 안에 뜨므로 창을 먼저 연다
            crate::window::show(app, true);
            crate::settings::emit_settings(app, true, None);
        }
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
    pub terminal_expanded: bool,
    /// 터미널 펼치기 항목 글자(지금 단축키가 들어 있다)
    pub terminal_label: String,
}

impl Tray {
    /// 아이콘·툴팁·체크를 고친다 — 메인 스레드에서 부른다.
    pub fn update(&self, s: &TrayState) {
        let _ = self.icon.set_icon(Some(icon_for(&s.counts)));
        let _ = self.icon.set_tooltip(Some(tooltip(&s.counts)));
        let _ = self.pin.set_checked(s.pinned);
        let _ = self.terminal.set_checked(s.terminal_expanded);
        let _ = self.terminal.set_text(&s.terminal_label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_label_carries_the_current_key() {
        assert_eq!(terminal_label(Some("Ctrl+Alt+T")), "터미널 펼치기 (Ctrl+Alt+T)");
        assert_eq!(terminal_label(Some("Ctrl+Shift+1")), "터미널 펼치기 (Ctrl+Shift+1)");
        assert_eq!(terminal_label(None), "터미널 펼치기");
    }
}
