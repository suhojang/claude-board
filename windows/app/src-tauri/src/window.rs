//! 보드 창 — 터미널 펼침·접힘 모양, 보드 쪽에 붙이기, 최대축소, 항상 위, 보이기 (SPEC §5.10, §7, `ClaudeBoard.swift:4246-4475, 4934-5014`).
//! 위치·크기는 모두 물리 픽셀로 다룬다(모니터마다 배율이 다를 수 있어 그 모니터의 배율을 곱한다).

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::settings::{Frame, Settings, MIN_PANE_WIDTH};

/// 보드 칸이 무너지지 않는 최소 폭 — 더 좁으면 미니로 그린다
pub const BOARD_MIN_WIDTH: f64 = MIN_PANE_WIDTH;
/// 펼쳤을 때 터미널 칸과 보드 칸 사이 경계 폭
pub const DIVIDER_WIDTH: f64 = 8.0;
/// 펼쳤을 때 터미널 칸의 최소 폭
pub const TERMINAL_MIN_WIDTH: f64 = 480.0;
/// 펼친 창의 최소 크기 — 보드 칸 360 + 경계 8 + 터미널 칸 480
pub const EXPANDED_MIN_WIDTH: f64 = BOARD_MIN_WIDTH + DIVIDER_WIDTH + TERMINAL_MIN_WIDTH;
pub const EXPANDED_MIN_HEIGHT: f64 = 300.0;
pub const MINI_WIDTH: f64 = 300.0;
pub const MINI_MIN_HEIGHT: f64 = 144.0;
pub const MINI_ROW_HEIGHT: f64 = 26.0;

/// 보드가 붙는 화면 쪽.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// 설정 값 → 쪽. 모르는 값은 오른쪽(기본).
    pub fn parse(s: &str) -> Side {
        if s == "left" {
            Side::Left
        } else {
            Side::Right
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// 모니터의 작업 영역(작업 표시줄 제외, 물리 픽셀)과 배율.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mon {
    pub work: Frame,
    pub scale: f64,
}

/// 창이 가져야 할 모양 — 프레임·최소 크기(논리 픽셀)·항상 위.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub frame: Frame,
    pub min: (f64, f64),
    pub always_on_top: bool,
}

/// 실행할 때의 창과 터미널을 펼친 채인지.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Launch {
    pub shape: Shape,
    pub expanded: bool,
}

pub fn intersects(a: Frame, b: Frame) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn contains(m: &Mon, x: f64, y: f64) -> bool {
    x >= m.work.x && x < m.work.x + m.work.w && y >= m.work.y && y < m.work.y + m.work.h
}

/// 프레임이 있는 모니터 — 중심이 든 곳, 없으면 겹치는 곳, 없으면 첫(주) 모니터.
fn monitor_at(mons: &[Mon], f: Frame) -> Option<Mon> {
    mons.iter()
        .find(|m| contains(m, f.x + f.w / 2.0, f.y + f.h / 2.0))
        .or_else(|| mons.iter().find(|m| intersects(m.work, f)))
        .or(mons.first())
        .copied()
}

/// 접힌 창(`snap_right` 명령·Ctrl+Alt+B 가 붙이는 자리) — 보드 쪽 가장자리에 `pane_width`(논리 픽셀) 폭, 작업 영역 높이 전체.
pub fn dock_frame(m: &Mon, side: Side, pane_width: f64) -> Frame {
    // 폭을 먼저 물리 픽셀로 반올림한다 — 454 * 1.25·1.75 처럼 .5 로 끝나면 x 와 폭이 따로 반올림되어 오른쪽 가장자리가 1px 넘친다
    let w = (pane_width * m.scale).round().min(m.work.w);
    let x = match side {
        Side::Right => m.work.x + m.work.w - w,
        Side::Left => m.work.x,
    };
    Frame { x, y: m.work.y, w, h: m.work.h }
}

/// 터미널을 펼친 창은 그 모니터 작업 영역 전체, 접은 창은 보드 쪽에 붙인다.
pub fn shape(m: &Mon, expanded: bool, side: Side, pane_width: f64, pinned: bool) -> Shape {
    if expanded {
        Shape { frame: m.work, min: (EXPANDED_MIN_WIDTH, EXPANDED_MIN_HEIGHT), always_on_top: false }
    } else {
        Shape { frame: dock_frame(m, side, pane_width), min: (MINI_WIDTH, MINI_MIN_HEIGHT), always_on_top: pinned }
    }
}

/// 실행할 때의 창. 미니로 두고 껐으면 그 크기·위치 그대로(화면 안에 있을 때).
/// 아니면 터미널을 펼쳤을 때는 작업 영역 전체, 접었을 때는 보드 쪽(`board_side`)에 `board_pane_width` 폭으로 붙인다.
/// 어느 모니터인지는 저장한 위치로 고르고, 없으면 주 모니터다.
pub fn launch_frame(mons: &[Mon], s: &Settings) -> Option<Launch> {
    let first = mons.first()?;
    let pinned = s.pinned;
    if let Some(f) = s.window_frame.filter(|f| s.board_mini && mons.iter().any(|m| intersects(m.work, *f))) {
        let shape = Shape { frame: f, min: (MINI_WIDTH, MINI_MIN_HEIGHT), always_on_top: pinned };
        return Some(Launch { shape, expanded: false });
    }
    let m = s.window_frame.and_then(|f| monitor_at(mons, f)).unwrap_or(*first);
    let expanded = s.terminal_expanded;
    Some(Launch { shape: shape(&m, expanded, Side::parse(&s.board_side), s.board_pane_width, pinned), expanded })
}

/// 최대축소 — 보드 쪽 위 모서리는 그대로 두고 폭 300, 높이는 경로 수(최대 6줄)에 맞춘다(최소 높이에 두 줄).
pub fn mini_frame(cur: Frame, rows: usize, scale: f64, side: Side) -> Frame {
    let h = (MINI_MIN_HEIGHT + (rows.clamp(2, 6) - 2) as f64 * MINI_ROW_HEIGHT) * scale;
    let w = MINI_WIDTH * scale;
    let x = match side {
        Side::Right => cur.x + cur.w - w,
        Side::Left => cur.x,
    };
    Frame { x, y: cur.y, w, h }
}

/// 보드 쪽을 바꿨을 때의 접힌 창 — 미니이면 크기·높이는 그대로 새 쪽 가장자리로, 아니면 새 쪽에 붙인다.
pub fn side_frame(cur: Frame, m: &Mon, side: Side, pane_width: f64) -> Frame {
    if cur.w >= BOARD_MIN_WIDTH * m.scale {
        return dock_frame(m, side, pane_width);
    }
    let x = match side {
        Side::Right => m.work.x + m.work.w - cur.w,
        Side::Left => m.work.x,
    };
    Frame { x, ..cur }
}

/// ⤢ 로 돌아갈 위치로 쓸 수 있는지 — 폭이 미니보다 넓고 지금 연결된 화면과 겹친다.
pub fn restorable(f: Frame, mons: &[Mon]) -> bool {
    mons.iter().any(|m| intersects(m.work, f) && f.w >= BOARD_MIN_WIDTH * m.scale)
}

/// Ctrl+Alt+T 가 하는 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKey {
    /// 접혀 있다 → 펼치고, 보이고, 포커스를 주고, 터미널에 포커스 이벤트
    Expand,
    /// 펼쳐져 있고 앞에 있다 → 접는다
    Collapse,
    /// 펼쳐져 있지만 다른 앱 뒤에 있다 → 보이고, 포커스를 주고, 터미널에 포커스 이벤트
    Focus,
}

pub fn term_key(expanded: bool, front: bool) -> TermKey {
    match (expanded, front) {
        (false, _) => TermKey::Expand,
        (true, true) => TermKey::Collapse,
        (true, false) => TermKey::Focus,
    }
}

/// 연결된 모니터들(주 모니터가 먼저).
pub fn monitors(app: &AppHandle) -> Vec<Mon> {
    let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let mut list: Vec<(bool, Mon)> = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let a = m.work_area();
            let work = Frame { x: a.position.x as f64, y: a.position.y as f64, w: a.size.width as f64, h: a.size.height as f64 };
            (Some(*m.position()) == primary, Mon { work, scale: m.scale_factor() })
        })
        .collect();
    list.sort_by_key(|(p, _)| !*p);
    list.into_iter().map(|(_, m)| m).collect()
}

/// 창이 있는 모니터(없으면 주 모니터).
pub fn monitor_of(win: &WebviewWindow) -> Option<Mon> {
    let mons = monitors(win.app_handle());
    match frame(win) {
        Some(f) => monitor_at(&mons, f),
        None => mons.first().copied(),
    }
}

pub fn frame(win: &WebviewWindow) -> Option<Frame> {
    let p = win.outer_position().ok()?;
    let s = win.outer_size().ok()?;
    Some(Frame { x: p.x as f64, y: p.y as f64, w: s.width as f64, h: s.height as f64 })
}

/// 두 배율이 다르면 창을 옮길 때 tao 가 방금 준 크기를 새 배율로 다시 곱한다.
pub fn rescales(from: f64, to: f64) -> bool {
    (from - to).abs() > 1e-6
}

/// 위치를 먼저, 크기를 그다음에 준다. 배율이 다른 모니터로 옮기면 tao 가 옮기면서 크기를 다시 곱하므로
/// 옮긴 뒤에 크기를 한 번 더 준다.
pub fn set_frame(win: &WebviewWindow, f: Frame) {
    let mons = monitors(win.app_handle());
    let before = monitor_of(win).map(|m| m.scale);
    let after = monitor_at(&mons, f).map(|m| m.scale);
    let size = PhysicalSize::new(f.w.round().max(1.0) as u32, f.h.round().max(1.0) as u32);
    let _ = win.set_position(PhysicalPosition::new(f.x.round() as i32, f.y.round() as i32));
    let _ = win.set_size(size);
    if matches!((before, after), (Some(b), Some(a)) if rescales(b, a)) {
        let _ = win.set_size(size);
    }
}

/// 최소 크기를 먼저 맞추고(새 프레임이 옛 최소 크기에 막히지 않게) 항상 위와 프레임을 바꾼다.
pub fn apply_shape(win: &WebviewWindow, s: &Shape) {
    let _ = win.set_min_size(Some(LogicalSize::new(s.min.0, s.min.1)));
    let _ = win.set_always_on_top(s.always_on_top);
    set_frame(win, s.frame);
}

pub fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// 보이고, 최소화돼 있지 않고, 포커스가 있다 — 다른 앱 뒤에 있지 않다.
pub fn is_front(win: &WebviewWindow) -> bool {
    win.is_visible().unwrap_or(false) && !win.is_minimized().unwrap_or(false) && win.is_focused().unwrap_or(false)
}

/// 포커스를 가져가지 않고 보인다 — `--background`·전역 단축키·Windows Terminal 실행 때.
///
/// Windows: 창 자체(tao)의 보이기 상태를 거쳐야 한다. Win32 `ShowWindow` 로 직접 보이면 tao 는 숨은 창으로 알고 있어
/// 그 뒤 `hide()` 가 아무 일도 하지 않고(트레이·닫기 단추로 못 숨김) 항상 위·최소화를 바꿀 때 창을 숨겨 버린다.
/// 그래서 잠깐 활성화되지 않는 창(WS_EX_NOACTIVATE)으로 바꿔 `show()` 한 뒤 되돌린다. 세 호출은 주 스레드에서 차례로 돈다.
pub fn show_inactive(win: &WebviewWindow) {
    #[cfg(windows)]
    {
        let _ = win.set_focusable(false);
        let _ = win.show();
        let _ = win.set_focusable(true);
    }
    #[cfg(not(windows))]
    let _ = win.show();
}

/// 보드를 보인다(최소화돼 있으면 되살린다).
pub fn show(app: &AppHandle, focus: bool) {
    let Some(win) = main_window(app) else { return };
    if win.is_minimized().unwrap_or(false) {
        let _ = win.unminimize();
    }
    if focus {
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        show_inactive(&win);
    }
}

/// 트레이 왼쪽 클릭 — 보이면 숨기고, 숨었거나 최소화돼 있으면 보인다.
pub fn toggle(app: &AppHandle) {
    let Some(win) = main_window(app) else { return };
    if win.is_visible().unwrap_or(false) && !win.is_minimized().unwrap_or(false) {
        let _ = win.hide();
    } else {
        show(app, true);
    }
}

/// 창 모양은 프론트 CSS(반지름 16, 1px 가장자리 선)가 혼자 그린다. Windows 11 DWM 이 같은 일을 또 하면
/// 바깥 호(DWM 약 8px)와 안쪽 호(CSS 16)가 겹쳐 보이므로 DWM 쪽을 모두 끈다:
/// 모서리 둥글림(DONOTROUND)·DWM 테두리(COLOR_NONE)·Mica 배경(직사각형 전체에 칠해져 둥근 모서리 밖 쐐기로 비친다).
/// 창 그림자(tauri.conf.json `shadow`)도 끈다 — DWM 그림자는 둥근 CSS 모양이 아니라 직사각형으로 번지고, 보이지 않는 7px 크기 조절 틀 때문에 창이 작업 영역 밖으로 나간다.
/// Windows 10 등 속성이 없는 곳은 DwmSetWindowAttribute 가 오류 코드만 돌려주므로 무시한다.
pub fn apply_vibrancy(win: &WebviewWindow) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
        };
        let _ = window_vibrancy::clear_mica(win);
        let _ = window_vibrancy::clear_acrylic(win);
        let Ok(hwnd) = win.hwnd() else { return };
        let corner: i32 = DWMWCP_DONOTROUND;
        let border: u32 = DWMWA_COLOR_NONE;
        // SAFETY: hwnd 는 살아 있는 이 창의 핸들이고, 값은 속성이 요구하는 4바이트 정수다.
        unsafe {
            DwmSetWindowAttribute(hwnd.0 as _, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &corner as *const i32 as *const _, 4);
            DwmSetWindowAttribute(hwnd.0 as _, DWMWA_BORDER_COLOR as u32, &border as *const u32 as *const _, 4);
        }
    }
    let _ = win;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(x: f64, y: f64, w: f64, h: f64, scale: f64) -> Mon {
        Mon { work: Frame { x, y, w, h }, scale }
    }

    fn fr(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame { x, y, w, h }
    }

    /// 주 모니터(배율 1.0) 오른쪽에 배율 1.5 모니터가 붙은 배치.
    fn two() -> (Mon, Mon) {
        (mon(0.0, 0.0, 1920.0, 1040.0, 1.0), mon(1920.0, 0.0, 2560.0, 1392.0, 1.5))
    }

    #[test]
    fn expanded_fills_the_work_area_on_every_monitor() {
        let (main, hi) = two();
        for m in [main, hi] {
            for side in [Side::Left, Side::Right] {
                let s = shape(&m, true, side, 454.0, true);
                assert_eq!(s.frame, m.work);
                assert_eq!(s.min, (848.0, 300.0));
                assert!(!s.always_on_top, "펼친 동안은 항상 위가 아니다");
            }
        }
    }

    #[test]
    fn collapsed_dock_stays_inside_the_work_area_at_fractional_scales() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 3.5] {
            let m = mon(0.0, 0.0, 1920.0, 1044.0, scale);
            for side in [Side::Left, Side::Right] {
                let f = dock_frame(&m, side, 454.0);
                // set_frame 이 x 와 폭을 따로 반올림해 넘기므로 그 값으로 견준다
                let (x, w) = (f.x.round(), f.w.round());
                assert_eq!(f.w, w, "폭은 이미 정수 픽셀 (scale {scale})");
                assert!(x >= m.work.x && x + w <= m.work.x + m.work.w, "scale {scale} {side:?}: x {x} + w {w} 이 작업 영역을 넘는다");
            }
        }
    }

    #[test]
    fn collapsed_docks_to_the_board_side_at_pane_width_times_scale() {
        let (main, hi) = two();
        let cases = [
            (main, Side::Right, 454.0, fr(1466.0, 0.0, 454.0, 1040.0)),
            (main, Side::Left, 454.0, fr(0.0, 0.0, 454.0, 1040.0)),
            (hi, Side::Right, 454.0, fr(1920.0 + 2560.0 - 681.0, 0.0, 681.0, 1392.0)),
            (hi, Side::Left, 454.0, fr(1920.0, 0.0, 681.0, 1392.0)),
            // 끌어 둔 폭
            (main, Side::Right, 520.0, fr(1400.0, 0.0, 520.0, 1040.0)),
            (hi, Side::Left, 520.0, fr(1920.0, 0.0, 780.0, 1392.0)),
        ];
        for (m, side, width, want) in cases {
            for pinned in [true, false] {
                let s = shape(&m, false, side, width, pinned);
                assert_eq!(s.frame, want, "{side:?} {width} 배율 {}", m.scale);
                assert_eq!(s.min, (300.0, 144.0));
                assert_eq!(s.always_on_top, pinned, "접으면 항상 위는 고정 여부를 따른다");
                assert_eq!(dock_frame(&m, side, width), want);
            }
        }
        // 작업 영역보다 넓게 기억한 폭은 작업 영역에 맞춘다
        assert_eq!(dock_frame(&mon(0.0, 0.0, 800.0, 600.0, 1.0), Side::Right, 900.0), fr(0.0, 0.0, 800.0, 600.0));
    }

    #[test]
    fn launch_expanded_or_collapsed_without_a_last_horizontal_position() {
        let (main, hi) = two();
        let base = Settings::default();
        // 저장한 위치가 없으면 주 모니터
        let l = launch_frame(&[main, hi], &base).unwrap();
        assert!(l.expanded);
        assert_eq!(l.shape.frame, main.work);
        let collapsed = Settings { terminal_expanded: false, ..base.clone() };
        let l = launch_frame(&[main, hi], &collapsed).unwrap();
        assert!(!l.expanded);
        assert_eq!(l.shape.frame, fr(1466.0, 0.0, 454.0, 1040.0));
        // 저장한 위치가 있는 모니터에서, 마지막 가로 위치와 상관없이 보드 쪽에
        let saved = Some(fr(2000.0, 300.0, 800.0, 400.0));
        let on_hi = Settings { window_frame: saved, ..collapsed.clone() };
        assert_eq!(launch_frame(&[main, hi], &on_hi).unwrap().shape.frame, fr(3799.0, 0.0, 681.0, 1392.0));
        let left = Settings { board_side: "left".into(), ..on_hi.clone() };
        assert_eq!(launch_frame(&[main, hi], &left).unwrap().shape.frame, fr(1920.0, 0.0, 681.0, 1392.0));
        let expanded_on_hi = Settings { terminal_expanded: true, ..on_hi };
        assert_eq!(launch_frame(&[main, hi], &expanded_on_hi).unwrap().shape.frame, hi.work);
        // 그 모니터가 없어졌으면 주 모니터
        let gone = Settings { window_frame: Some(fr(9000.0, 0.0, 454.0, 900.0)), ..collapsed.clone() };
        assert_eq!(launch_frame(&[main], &gone).unwrap().shape.frame, fr(1466.0, 0.0, 454.0, 1040.0));
        // 미니로 껐으면 그 자리 그대로(접힌 채), 화면 밖이면 보드 쪽에 붙인다
        let m = fr(1600.0, 20.0, 300.0, 144.0);
        let mini = Settings { board_mini: true, window_frame: Some(m), ..base.clone() };
        let l = launch_frame(&[main], &mini).unwrap();
        assert_eq!((l.shape.frame, l.expanded), (m, false));
        let lost = Settings { board_mini: true, window_frame: Some(fr(9000.0, 20.0, 300.0, 144.0)), terminal_expanded: false, ..base };
        assert_eq!(launch_frame(&[main], &lost).unwrap().shape.frame, fr(1466.0, 0.0, 454.0, 1040.0));
        assert_eq!(launch_frame(&[], &Settings::default()), None);
    }

    #[test]
    fn mini_keeps_the_board_side_corner() {
        let (main, hi) = two();
        for (m, w, h) in [(main, 300.0, 144.0), (hi, 450.0, 216.0)] {
            let right = dock_frame(&m, Side::Right, 454.0);
            let got = mini_frame(right, 1, m.scale, Side::Right);
            assert_eq!(got, fr(right.x + right.w - w, 0.0, w, h), "오른쪽 위 모서리");
            let left = dock_frame(&m, Side::Left, 454.0);
            let got = mini_frame(left, 1, m.scale, Side::Left);
            assert_eq!(got, fr(left.x, 0.0, w, h), "왼쪽 위 모서리");
        }
        let right = dock_frame(&main, Side::Right, 454.0);
        assert_eq!(mini_frame(right, 1, 1.0, Side::Right), fr(1620.0, 0.0, 300.0, 144.0));
        assert_eq!(mini_frame(right, 4, 1.0, Side::Right).h, 144.0 + 52.0);
        assert_eq!(mini_frame(right, 20, 2.0, Side::Right), fr(1320.0, 0.0, 600.0, (144.0 + 104.0) * 2.0));
    }

    #[test]
    fn side_change_moves_the_collapsed_board_or_mini() {
        let (main, hi) = two();
        // 접힌 보드는 새 쪽에 붙는다
        let right = dock_frame(&hi, Side::Right, 454.0);
        assert_eq!(side_frame(right, &hi, Side::Left, 454.0), fr(1920.0, 0.0, 681.0, 1392.0));
        // 미니는 크기와 높이를 두고 새 쪽 가장자리로
        let mini = fr(1620.0, 20.0, 300.0, 144.0);
        assert_eq!(side_frame(mini, &main, Side::Left, 454.0), fr(0.0, 20.0, 300.0, 144.0));
        let mini_left = fr(0.0, 20.0, 300.0, 144.0);
        assert_eq!(side_frame(mini_left, &main, Side::Right, 454.0), fr(1620.0, 20.0, 300.0, 144.0));
    }

    #[test]
    fn size_is_set_again_only_when_the_scale_changes() {
        assert!(rescales(1.0, 1.5));
        assert!(!rescales(1.5, 1.5));
        let (a, b) = two();
        assert!(rescales(a.scale, b.scale));
        // 도크 프레임이 있는 모니터의 배율은 monitor_at 이 고른다
        assert_eq!(monitor_at(&[a, b], dock_frame(&b, Side::Right, 454.0)).map(|m| m.scale), Some(1.5));
    }

    #[test]
    fn restore_and_shortcut() {
        let main = mon(0.0, 0.0, 1920.0, 1040.0, 1.0);
        let cur = fr(1466.0, 0.0, 454.0, 1040.0);
        assert!(restorable(cur, &[main]));
        assert!(!restorable(Frame { w: 300.0, ..cur }, &[main]));
        assert!(!restorable(Frame { x: 5000.0, ..cur }, &[main]));

        assert_eq!(term_key(false, true), TermKey::Expand);
        assert_eq!(term_key(false, false), TermKey::Expand);
        assert_eq!(term_key(true, true), TermKey::Collapse);
        assert_eq!(term_key(true, false), TermKey::Focus);
        assert_eq!(Side::parse("left"), Side::Left);
        assert_eq!(Side::parse("nope"), Side::Right);
        assert_eq!(Side::Left.as_str(), "left");
    }
}
