//! 알림 패널 — `ClaudeBoard.exe --notify …` (`ClaudeNotifier.swift`, SPEC §4·§6.3).
//!
//! 보드와 따로 도는 프로세스다(단일 실행·트레이·단축키 없음). 패널 하나를 보이고 닫히면 끝난다.
//! 화면(카드·카운트다운·애니메이션)은 프론트 `/notify` 가 그리고, 백엔드는 자리 잡기·슬롯·소리·클릭 처리만 한다.
//! 여러 패널이 동시에 뜨면 `%TEMP%\claude-notifier-slots` 의 슬롯 파일로 자리를 나눈다.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use claude_board_core::clock;
use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};

use crate::settings::{Frame, POSITIONS};
use crate::util::{board_log, mtime};

/// 패널 폭(논리 픽셀)
const WIDTH: f64 = 440.0;
/// 카드 위아래 여백(논리 px) — 프론트(routes/notify)의 카드 margin 과 같게
const PAD: f64 = 14.0;
/// 모서리에 둘 때 화면 가장자리와의 간격
const MARGIN: f64 = 12.0;
/// 모서리에 쌓인 패널 사이 간격
const GAP: f64 = 8.0;
/// 이만큼 갱신 없는 슬롯은 버려진 것으로 본다
const STALE: f64 = 120.0;
/// 슬롯 순번별 가로 위치(패널 폭 배수) — 가운데를 기준으로 오른쪽·왼쪽 번갈아 반씩 비켜 놓는다
const SHIFTS: [f64; 5] = [0.0, 0.5, -0.5, 1.0, -1.0];

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub kind: String,
    pub summary: BTreeMap<String, i64>,
    pub sound: String,
    pub duration: f64,
    pub position: String,
    /// 이 알림이 속한 세션 id(`--session`) — 비어 있으면 세션 없는 알림. 누르면 보드가 그 세션으로 이동한다
    pub session: String,
}

/// 프론트에 주는 모양 — types.ts 의 NotifyOptions.
#[derive(Debug, Clone, Serialize)]
pub struct OptionsView {
    title: String,
    subtitle: String,
    body: String,
    kind: String,
    summary: BTreeMap<String, i64>,
    duration: f64,
    position: String,
}

/// 인자를 읽는다 — 같은 키를 다시 주면 뒤의 값. `--notify` 는 값 없는 표시라 건너뛴다.
pub fn parse(argv: &[String]) -> Options {
    let mut o = Options {
        title: "Claude Code".into(),
        subtitle: String::new(),
        body: String::new(),
        kind: "done".into(),
        summary: BTreeMap::new(),
        sound: "done".into(),
        duration: 10.0,
        position: "center".into(),
        session: String::new(),
    };
    let mut it = argv.iter().skip(1);
    while let Some(key) = it.next() {
        if key == "--notify" {
            continue;
        }
        let Some(value) = it.next() else { break };
        match key.as_str() {
            "--title" => o.title = value.clone(),
            "--subtitle" => o.subtitle = value.clone(),
            "--body" => o.body = value.clone(),
            "--kind" => o.kind = if value == "wait" { "wait".into() } else { "done".into() },
            "--sound" => o.sound = value.clone(),
            "--duration" => o.duration = value.parse().ok().filter(|d: &f64| d.is_finite() && *d > 0.0).unwrap_or(o.duration),
            "--session" => o.session = value.clone(),
            "--position" => o.position = if POSITIONS.contains(&value.as_str()) { value.clone() } else { "center".into() },
            "--summary" => {
                for pair in value.split(',') {
                    if let Some((k, n)) = pair.split_once('=') {
                        if let Ok(n) = n.parse() {
                            o.summary.insert(k.to_string(), n);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    o
}

// ── 슬롯 ────────────────────────────────────────────────────

pub struct Slot {
    pub index: usize,
    path: Option<PathBuf>,
    dir: PathBuf,
}

impl Slot {
    pub fn dir() -> PathBuf {
        std::env::temp_dir().join("claude-notifier-slots")
    }

    /// 0..5 가운데 빈 슬롯을 잡는다(새로 만들기로). 120초 넘게 갱신 없는 슬롯은 먼저 버린다. 다 차 있으면 0번 자리, 파일 없음.
    pub fn claim(dir: &Path, now: f64) -> Slot {
        let _ = fs::create_dir_all(dir);
        for i in 0..SHIFTS.len() {
            let p = dir.join(i.to_string());
            if mtime(&p).is_some_and(|m| now - m > STALE) {
                let _ = fs::remove_file(&p);
            }
            if OpenOptions::new().write(true).create_new(true).open(&p).is_ok() {
                return Slot { index: i, path: Some(p), dir: dir.to_path_buf() };
            }
        }
        Slot { index: 0, path: None, dir: dir.to_path_buf() }
    }

    pub fn shift(&self) -> f64 {
        SHIFTS[self.index]
    }

    /// 모서리에 둔 이 패널의 세로 범위 — 예: "top-right 12 125"
    pub fn record(&self, position: &str, top: f64, bottom: f64) {
        if let Some(p) = &self.path {
            let _ = fs::write(p, format!("{position} {top} {bottom}"));
        }
    }

    /// 같은 모서리에 떠 있는 다른 패널들의 세로 범위 — 낡은 슬롯은 뺀다.
    pub fn taken(&self, position: &str, now: f64) -> Vec<(f64, f64)> {
        let Ok(rd) = fs::read_dir(&self.dir) else { return Vec::new() };
        rd.flatten()
            .filter_map(|e| {
                let p = e.path();
                if self.path.as_ref() == Some(&p) || mtime(&p).is_none_or(|m| now - m > STALE) {
                    return None;
                }
                let text = fs::read_to_string(&p).ok()?;
                let f: Vec<&str> = text.split(' ').collect();
                if f.len() != 3 || f[0] != position {
                    return None;
                }
                Some((f[1].parse().ok()?, f[2].parse().ok()?))
            })
            .collect()
    }

    /// 마우스를 올려 오래 머무는 동안 다른 알림이 이 슬롯을 낡은 것으로 보고 가져가지 않도록 수정 시각을 갱신한다.
    pub fn touch(&self) {
        if let Some(p) = &self.path {
            if let Ok(f) = OpenOptions::new().write(true).open(p) {
                let _ = f.set_modified(SystemTime::now());
            }
        }
    }

    pub fn release(&self) {
        if let Some(p) = &self.path {
            let _ = fs::remove_file(p);
        }
    }
}

/// 패널 자리(물리 픽셀). 가운데: 슬롯 순번만큼 가로로 비켜 놓는다. 모서리: 가장자리에서 가까운 패널부터 보며
/// 겹치면 그 너머로 옮겨 쌓는다(위 모서리는 아래로, 아래 모서리는 위로). 가로는 작업 영역 안 8px 안쪽으로 맞춘다.
pub fn place(work: Frame, w: f64, h: f64, scale: f64, position: &str, shift: f64, taken: &[(f64, f64)]) -> (f64, f64) {
    let (margin, gap) = (MARGIN * scale, GAP * scale);
    let mut x = work.x + work.w / 2.0 - w / 2.0 + shift * w;
    let mut y = work.y + work.h / 2.0 - h / 2.0;
    if position != "center" {
        let top = position.starts_with("top");
        x = if position.ends_with("left") { work.x + margin } else { work.x + work.w - w - margin };
        y = if top { work.y + margin } else { work.y + work.h - margin - h };
        let mut sorted = taken.to_vec();
        if top {
            sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        } else {
            sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
        }
        for (t, b) in sorted {
            if y < b + gap && y + h > t - gap {
                y = if top { b + gap } else { t - gap - h };
            }
        }
    }
    let edge = 8.0 * scale;
    (x.max(work.x + edge).min(work.x + work.w - w - edge), y)
}

// ── 실행 ────────────────────────────────────────────────────

struct Panel {
    opts: Options,
    slot: Slot,
    /// notify_ready 를 받았는지
    ready: bool,
    hovered: bool,
    /// 마지막으로 카운트다운이 (다시) 시작된 시각 — 프론트가 notify_finish 를 못 부르면 이것으로 끝낸다
    since: f64,
}

type PanelState = Mutex<Panel>;

#[tauri::command]
fn get_notify_options(state: State<'_, PanelState>) -> OptionsView {
    crate::util::trace(|| "get_notify_options".into());
    let o = state.lock().unwrap().opts.clone();
    OptionsView { title: o.title, subtitle: o.subtitle, body: o.body, kind: o.kind, summary: o.summary, duration: o.duration, position: o.position }
}

/// 내용 높이를 받아 자리를 잡고, 포커스를 가져가지 않고 보인 뒤 소리를 낸다. 보인 뒤에 돌아온다.
#[tauri::command(async)]
fn notify_ready(app: AppHandle, state: State<'_, PanelState>, height: f64) {
    crate::util::trace(|| format!("notify_ready height={height}"));
    let Some(win) = app.get_webview_window("notify") else { return };
    // 마우스가 있는 모니터의 작업 영역
    let cursor = app.cursor_position().ok();
    let monitor = cursor
        .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (work, scale) = match &monitor {
        Some(m) => {
            let a = m.work_area();
            (Frame { x: a.position.x as f64, y: a.position.y as f64, w: a.size.width as f64, h: a.size.height as f64 }, m.scale_factor())
        }
        None => (Frame { x: 0.0, y: 0.0, w: 1280.0, h: 800.0 }, 1.0),
    };
    let (w, h) = (WIDTH * scale, height.clamp(40.0, 600.0) * scale);
    let (sound, position) = {
        let p = state.lock().unwrap();
        (p.opts.sound.clone(), p.opts.position.clone())
    };
    let (x, y) = {
        let p = state.lock().unwrap();
        if position == "center" {
            place(work, w, h, scale, &position, p.slot.shift(), &[])
        } else {
            // 동시에 뜬 패널이 서로의 기록을 못 보고 같은 자리를 고르지 않도록, 범위를 읽고 적는 동안 잠근다
            let lock = File::create(Slot::dir().join("lock")).ok();
            if let Some(l) = &lock {
                let _ = l.lock();
            }
            let now = clock::now();
            let at = place(work, w, h, scale, &position, 0.0, &p.slot.taken(&position, now));
            p.slot.record(&position, at.1, at.1 + h);
            if let Some(l) = &lock {
                let _ = l.unlock();
            }
            at
        }
    };
    crate::util::trace(|| format!("notify 자리 x={x} y={y} w={w} h={h} (작업 영역 {work:?}, 배율 {scale})"));
    // 자리는 카드 높이로 잡고, 창은 위아래로 PAD 씩 넓힌다 — 카드가 14px 아래에서 올라오고 10px 위로 사라지는 동안 창 경계에 잘리지 않게
    let pad = PAD * scale;
    let _ = win.set_size(PhysicalSize::new(w.round() as u32, (h + 2.0 * pad).round() as u32));
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, (y - pad).round() as i32));
    // Win32 로 직접 스타일을 바꾸고 보이므로 tao 는 이 창을 숨은 창·기본 스타일로 알고 있다. 이 뒤에 tao 의 창 상태를 바꾸는
    // 호출(항상 위·최소화·크기 조절 가능 등)을 하면 tao 가 확장 스타일을 덮어쓰고 창을 숨기므로, 패널에서는 그런 호출을 하지 않는다.
    // (win.hwnd() 는 주 스레드를 거쳐 돌아오므로 위의 크기·위치 변경이 먼저 끝난 뒤에 보인다)
    #[cfg(windows)]
    if let Ok(hwnd) = win.hwnd() {
        crate::win::style_notify_panel(hwnd.0 as isize);
        crate::win::show_no_activate(hwnd.0 as isize);
    }
    #[cfg(not(windows))]
    let _ = win.show();
    crate::win::play_sound(&sound);
    let mut p = state.lock().unwrap();
    p.ready = true;
    p.since = clock::now();
}

#[tauri::command]
fn notify_hover(state: State<'_, PanelState>, inside: bool) {
    let mut p = state.lock().unwrap();
    p.hovered = inside;
    p.since = clock::now();
}

/// 누를 때 보드에 거는 인자 — 세션 알림이면 `--action focus <sid>`(보드 탭 세션은 그 탭으로, 다른 터미널은 그 창으로),
/// 세션이 없으면 `--show`. 둘 다 떠 있는 보드에는 단일 실행 플러그인이 넘기고, 안 떠 있으면 새 보드가 처리한다.
fn click_args(session: &str) -> Vec<&str> {
    if session.is_empty() { vec!["--show"] } else { vec!["--action", "focus", session] }
}

/// 누르면 보드를 띄우고 세션 알림이면 그 세션으로 이동시킨다. 닫기는 프론트가 notify_finish 로.
#[tauri::command(async)]
fn notify_clicked(state: State<'_, PanelState>) {
    let session = state.lock().unwrap().opts.session.clone();
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.args(click_args(&session));
    if let Err(e) = crate::util::spawn_detached(cmd) {
        board_log(&format!("알림 패널에서 보드를 띄우지 못했습니다: {e}"));
    }
}

#[tauri::command]
fn notify_finish(app: AppHandle, state: State<'_, PanelState>) {
    crate::util::trace(|| "notify_finish".into());
    state.lock().unwrap().slot.release();
    app.exit(0);
}

pub fn run(argv: &[String], context: tauri::Context<tauri::Wry>) {
    let opts = parse(argv);
    let slot = Slot::claim(&Slot::dir(), clock::now());
    let duration = opts.duration;
    let panel = Panel { opts, slot, ready: false, hovered: false, since: clock::now() };
    let result = tauri::Builder::default()
        .manage(Mutex::new(panel))
        .invoke_handler(tauri::generate_handler![get_notify_options, notify_ready, notify_hover, notify_clicked, notify_finish])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            WebviewWindowBuilder::new(app, "notify", WebviewUrl::App("notify/".into()))
                .title("Claude Board")
                .inner_size(WIDTH, 120.0)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .build()?;
            watchdog(app.handle().clone(), duration);
            Ok(())
        })
        .build(context);
    match result {
        Ok(app) => app.run(|app, e| {
            if let tauri::RunEvent::Exit = e {
                if let Some(s) = app.try_state::<PanelState>() {
                    s.lock().unwrap().slot.release();
                }
            }
        }),
        Err(e) => board_log(&format!("알림 패널을 띄우지 못했습니다: {e}")),
    }
}

/// 1초마다 슬롯 수정 시각을 갱신하고, 프론트가 멈춰 끝을 알리지 못하면 대신 끝낸다
/// (준비를 15초 안에 못 알렸거나, 카운트다운이 끝날 시각에서 10초가 지났을 때).
fn watchdog(app: AppHandle, duration: f64) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(state) = app.try_state::<PanelState>() else { return };
        let stuck = {
            let p = state.lock().unwrap();
            p.slot.touch();
            let idle = clock::now() - p.since;
            if p.ready {
                !p.hovered && idle > duration + 10.0
            } else {
                idle > 15.0
            }
        };
        if stuck {
            state.lock().unwrap().slot.release();
            app.exit(0);
            return;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("ClaudeBoard.exe").chain(list.iter().copied()).map(str::to_string).collect()
    }

    #[test]
    fn parses_arguments() {
        let o = parse(&args(&[
            "--notify", "--title", "작업 완료", "--subtitle", "demo", "--body", "--notify 는 본문", "--kind", "wait",
            "--summary", "waiting=1,working=2,idle=0,bad", "--duration", "3", "--position", "top-right", "--title", "승인 대기",
        ]));
        assert_eq!(o.title, "승인 대기");
        assert_eq!(o.body, "--notify 는 본문");
        assert_eq!(o.kind, "wait");
        assert_eq!(o.summary, BTreeMap::from([("waiting".into(), 1), ("working".into(), 2), ("idle".into(), 0)]));
        assert_eq!((o.duration, o.position.as_str(), o.sound.as_str()), (3.0, "top-right", "done"));
        assert_eq!(o.session, "");
        let d = parse(&args(&["--notify", "--kind", "odd", "--position", "middle", "--duration", "-1", "--sound"]));
        assert_eq!((d.kind.as_str(), d.position.as_str(), d.duration), ("done", "center", 10.0));
    }

    #[test]
    fn session_argument_decides_what_a_click_runs() {
        let o = parse(&args(&["--notify", "--title", "t", "--session", "abc-123"]));
        assert_eq!(o.session, "abc-123");
        assert_eq!(click_args(&o.session), ["--action", "focus", "abc-123"]);
        // 맥 notify.py 는 세션 id 가 없으면 빈 값을 넘긴다 — 세션 없는 알림으로 본다
        let empty = parse(&args(&["--notify", "--session", ""]));
        assert_eq!(click_args(&empty.session), ["--show"]);
        assert_eq!(click_args(&parse(&args(&["--notify"])).session), ["--show"]);
        // 뒤의 값이 이긴다
        assert_eq!(parse(&args(&["--session", "a", "--session", "b"])).session, "b");
    }

    const WORK: Frame = Frame { x: 0.0, y: 0.0, w: 1920.0, h: 1040.0 };

    #[test]
    fn center_shifts() {
        let xs: Vec<f64> = SHIFTS.iter().map(|s| place(WORK, 440.0, 100.0, 1.0, "center", *s, &[]).0).collect();
        assert_eq!(xs, [740.0, 960.0, 520.0, 1180.0, 300.0]);
        assert_eq!(place(WORK, 440.0, 100.0, 1.0, "center", 0.0, &[]).1, 470.0);
        // 좁은 화면에서는 작업 영역 안쪽 8px 로 맞춘다
        let narrow = Frame { x: 0.0, y: 0.0, w: 800.0, h: 600.0 };
        assert_eq!(place(narrow, 440.0, 100.0, 1.0, "center", 1.0, &[]).0, 800.0 - 440.0 - 8.0);
        assert_eq!(place(narrow, 440.0, 100.0, 1.0, "center", -1.0, &[]).0, 8.0);
    }

    #[test]
    fn corners_stack() {
        // 위 모서리: 가장자리부터, 겹치면 아래로
        assert_eq!(place(WORK, 440.0, 100.0, 1.0, "top-right", 0.0, &[]), (1920.0 - 440.0 - 12.0, 12.0));
        assert_eq!(place(WORK, 440.0, 100.0, 1.0, "top-left", 0.0, &[(12.0, 112.0)]), (12.0, 120.0));
        assert_eq!(place(WORK, 440.0, 50.0, 1.0, "top-left", 0.0, &[(120.0, 220.0), (12.0, 112.0)]), (12.0, 228.0));
        // 빈틈에 들어가면 그 자리에 둔다
        assert_eq!(place(WORK, 440.0, 50.0, 1.0, "top-left", 0.0, &[(200.0, 300.0)]), (12.0, 12.0));
        // 아래 모서리: 겹치면 위로
        let bottom = 1040.0 - 12.0;
        assert_eq!(place(WORK, 440.0, 100.0, 1.0, "bottom-right", 0.0, &[]).1, bottom - 100.0);
        assert_eq!(place(WORK, 440.0, 100.0, 1.0, "bottom-left", 0.0, &[(bottom - 100.0, bottom)]).1, bottom - 100.0 - 8.0 - 100.0);
        // 배율 2: 여백·간격도 두 배
        assert_eq!(place(WORK, 880.0, 200.0, 2.0, "top-left", 0.0, &[(24.0, 224.0)]), (24.0, 240.0));
    }

    #[test]
    fn slots() {
        let dir = std::env::temp_dir().join(format!("cb-slots-{}", crate::util::short_id()));
        let now = clock::now();
        let a = Slot::claim(&dir, now);
        let b = Slot::claim(&dir, now);
        assert_eq!((a.index, b.index), (0, 1));
        assert_eq!(b.shift(), 0.5);
        a.record("top-right", 12.0, 112.5);
        b.record("bottom-left", 900.0, 1000.0);
        assert_eq!(b.taken("top-right", now), [(12.0, 112.5)]);
        assert!(a.taken("top-right", now).is_empty());
        // 낡은 슬롯은 버리고 다시 쓴다
        a.release();
        let c = Slot::claim(&dir, now);
        assert_eq!(c.index, 0);
        assert!(b.taken("top-right", now + 200.0).is_empty());
        let _rest: Vec<Slot> = (0..3).map(|_| Slot::claim(&dir, now)).collect();
        let full = Slot::claim(&dir, now);
        assert_eq!((full.index, full.path.is_none()), (0, true));
        let reused = Slot::claim(&dir, now + 200.0);
        assert!(reused.path.is_some());
        c.touch();
        fs::remove_dir_all(&dir).unwrap();
    }
}
