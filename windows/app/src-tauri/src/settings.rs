//! 보드 설정 저장 — macOS UserDefaults 대응 (SPEC §5.9). `board-settings.json` 한 파일이고 백엔드가 단일 출처다.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use claude_board_core::{fileio, paths};
use serde::{Deserialize, Serialize};

use crate::model::SettingsView;

/// 창 위치·크기(물리 픽셀).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub collapsed: Vec<String>,
    pub ended_open: Vec<String>,
    pub pinned_groups: Vec<String>,
    /// 목록에서 지운 작업 경로와 지운 시각(유닉스 초)
    pub hidden_groups: BTreeMap<String, f64>,
    /// 자동으로 접은 작업 경로와 그때의 마지막 활동 시각
    pub auto_folded: BTreeMap<String, f64>,
    pub kind_collapsed: Vec<String>,
    pub sub_collapsed: Vec<String>,
    pub usage_compact: bool,
    pub pinned: bool,
    pub mascot: String,
    pub opacity: f64,
    pub auto_switch: bool,
    pub auto_switch_last: f64,
    pub auto_switch_full: BTreeMap<String, f64>,
    pub board_mini: bool,
    pub board_restore_frame: Option<Frame>,
    pub window_frame: Option<Frame>,
    /// 내장 터미널을 펼쳤는지 — 펼친 채 끄면 다음에도 펼친 채 연다
    pub terminal_expanded: bool,
    /// 보드 칸 폭(논리 픽셀) — 접었을 때 창 폭, 펼쳤을 때 보드 칸 폭
    pub board_pane_width: f64,
    /// 보드가 붙는 화면 쪽 — `right` · `left`
    pub board_side: String,
    /// 펼친 채 최대축소로 들어갔는지의 표지 — 원래 크기로 돌아올 때 다시 펼친다
    pub mini_from_expanded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            collapsed: Vec::new(),
            ended_open: Vec::new(),
            pinned_groups: Vec::new(),
            hidden_groups: BTreeMap::new(),
            auto_folded: BTreeMap::new(),
            kind_collapsed: Vec::new(),
            sub_collapsed: Vec::new(),
            usage_compact: false,
            pinned: true,
            mascot: "tralalero".into(),
            opacity: 1.0,
            auto_switch: false,
            auto_switch_last: 0.0,
            auto_switch_full: BTreeMap::new(),
            board_mini: false,
            board_restore_frame: None,
            window_frame: None,
            terminal_expanded: true,
            board_pane_width: DEFAULT_PANE_WIDTH,
            board_side: "right".into(),
            mini_from_expanded: false,
        }
    }
}

pub const MASCOTS: [&str; 8] = ["tralalero", "bombardiro", "tungtung", "lirili", "patapim", "boneca", "cappuccino", "foreman"];
pub const SIDES: [&str; 2] = ["right", "left"];
/// 보드 칸 폭의 기본값(논리 픽셀)
pub const DEFAULT_PANE_WIDTH: f64 = 454.0;
/// 보드 칸이 이보다 좁으면 전체·중간 화면이 무너진다(논리 픽셀)
pub const MIN_PANE_WIDTH: f64 = 360.0;
pub const POSITIONS: [&str; 5] = ["center", "top-left", "top-right", "bottom-left", "bottom-right"];

impl Settings {
    /// 읽는다. 없거나 깨졌으면 기본값.
    pub fn load(path: &Path) -> Self {
        let mut s: Settings = fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.opacity = clamp_opacity(s.opacity);
        s.board_pane_width = clamp_pane_width(s.board_pane_width);
        if !SIDES.contains(&s.board_side.as_str()) {
            s.board_side = "right".into();
        }
        if !MASCOTS.contains(&s.mascot.as_str()) {
            s.mascot = "tralalero".into();
        }
        s
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        fileio::write_atomic(path, "board-settings", &bytes)
    }

    pub fn view(&self, notify_position: String) -> SettingsView {
        SettingsView {
            collapsed: self.collapsed.clone(),
            ended_open: self.ended_open.clone(),
            pinned_groups: self.pinned_groups.clone(),
            kind_collapsed: self.kind_collapsed.clone(),
            sub_collapsed: self.sub_collapsed.clone(),
            usage_compact: self.usage_compact,
            pinned: self.pinned,
            mascot: self.mascot.clone(),
            opacity: self.opacity,
            auto_switch: self.auto_switch,
            board_mini: self.board_mini,
            terminal_expanded: self.terminal_expanded,
            board_side: self.board_side.clone(),
            board_pane_width: self.board_pane_width,
            notify_position,
        }
    }

    /// 요약 칩으로 찾아갈 때: 카드와 그 안의 터미널·백그라운드 구역을 편다.
    pub fn reveal(&mut self, cwd: &str) {
        set_in(&mut self.collapsed, cwd, false);
        let prefix = format!("{cwd}|");
        self.kind_collapsed.retain(|k| !k.starts_with(&prefix));
    }
}

/// 목록을 집합처럼 — 넣거나 뺀다.
pub fn set_in(list: &mut Vec<String>, item: &str, on: bool) {
    let at = list.iter().position(|x| x == item);
    match (on, at) {
        (true, None) => list.push(item.to_string()),
        (false, Some(i)) => {
            list.remove(i);
        }
        _ => {}
    }
}

/// 있으면 빼고 없으면 넣는다.
pub fn toggle(list: &mut Vec<String>, item: &str) {
    let on = !list.iter().any(|x| x == item);
    set_in(list, item, on);
}

pub fn clamp_opacity(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(0.3, 1.0)
    } else {
        1.0
    }
}

/// 보드 칸 폭 — 숫자가 아니면 기본값, 너무 좁으면 최소값.
pub fn clamp_pane_width(v: f64) -> f64 {
    if v.is_finite() {
        v.max(MIN_PANE_WIDTH)
    } else {
        DEFAULT_PANE_WIDTH
    }
}

/// 알림 위치 — `notify-position` 파일 내용(없거나 모르는 값이면 가운데).
pub fn notify_position() -> String {
    let v = fs::read_to_string(paths::notify_position_file()).unwrap_or_default();
    let v = v.trim();
    if POSITIONS.contains(&v) {
        v.to_string()
    } else {
        "center".into()
    }
}

/// 알림 위치를 고른다 — 가운데는 파일을 지운다(없으면 notify 가 가운데로 띄운다).
pub fn set_notify_position(pos: &str) -> std::io::Result<()> {
    let file = paths::notify_position_file();
    if pos == "center" || !POSITIONS.contains(&pos) {
        match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fileio::write_atomic(&file, "notify-position", pos.as_bytes())
    }
}

/// 설정 파일 위치 — Tauri app config dir. 환경변수로 가짜 홈을 쓰는 개발 실행은 notifier 폴더 안에 둔다(실제 설정을 건드리지 않게).
pub fn settings_path(app_config_dir: Option<PathBuf>) -> PathBuf {
    let name = "board-settings.json";
    if paths::overridden() {
        return paths::notifier_dir().join(name);
    }
    app_config_dir.unwrap_or_else(paths::notifier_dir).join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("cb-settings-{}", crate::util::short_id()));
        let path = dir.join("board-settings.json");
        let d = Settings::load(&path);
        assert!(d.pinned && d.mascot == "tralalero" && d.opacity == 1.0 && !d.auto_switch);
        assert!(d.terminal_expanded && !d.mini_from_expanded);
        assert_eq!((d.board_pane_width, d.board_side.as_str()), (454.0, "right"));

        let mut s = Settings::default();
        toggle(&mut s.collapsed, "/a");
        toggle(&mut s.collapsed, "/b");
        toggle(&mut s.collapsed, "/a");
        s.hidden_groups.insert("/c".into(), 1_792_000_000.25);
        s.auto_switch_full.insert("acc|org".into(), 1_792_003_600.0);
        s.kind_collapsed = vec!["/b|term".into(), "/b|bg".into(), "/x|bg".into()];
        s.window_frame = Some(Frame { x: 10.0, y: 0.0, w: 454.0, h: 1000.0 });
        s.mascot = "foreman".into();
        s.opacity = 0.5;
        s.terminal_expanded = false;
        s.board_pane_width = 520.5;
        s.board_side = "left".into();
        s.mini_from_expanded = true;
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back, s);
        assert_eq!(back.collapsed, ["/b"]);

        // 프론트에 보내는 키 이름
        let v = serde_json::to_value(back.view("top-left".into())).unwrap();
        for k in ["endedOpen", "pinnedGroups", "kindCollapsed", "subCollapsed", "usageCompact", "boardMini", "notifyPosition"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert_eq!(v["terminalExpanded"], false);
        assert_eq!(v["boardSide"], "left");
        assert_eq!(v["boardPaneWidth"], 520.5);
        assert!(v.get("hiddenGroups").is_none());
        assert!(v.get("miniFromExpanded").is_none());

        // 파일에 저장되는 키 이름(SPEC §5.9)
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let stored = [
            "hiddenGroups",
            "autoFolded",
            "autoSwitchLast",
            "autoSwitchFull",
            "boardRestoreFrame",
            "windowFrame",
            "terminalExpanded",
            "boardPaneWidth",
            "boardSide",
            "miniFromExpanded",
        ];
        for k in stored {
            assert!(raw.get(k).is_some(), "{k}");
        }

        let mut r = back.clone();
        r.collapsed.push("/x".into());
        r.reveal("/b");
        r.reveal("/x");
        assert!(r.collapsed.is_empty());
        assert!(r.kind_collapsed.is_empty());

        // 깨진 값은 기본값으로
        fs::write(&path, br#"{"opacity": 7, "mascot": "nope", "pinned": false, "boardSide": "top", "boardPaneWidth": 90}"#).unwrap();
        let odd = Settings::load(&path);
        assert_eq!((odd.opacity, odd.mascot.as_str(), odd.pinned), (1.0, "tralalero", false));
        assert_eq!((odd.board_side.as_str(), odd.board_pane_width), ("right", 360.0));
        fs::write(&path, b"{broken").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(&dir).unwrap();
    }
}
