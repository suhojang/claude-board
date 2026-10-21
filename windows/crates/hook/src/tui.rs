//! 터미널판 보드 — macOS `board-tui.py` 이식 (SPEC §3.8).
//!
//! 작업 경로별 카드로 세션을 묶어 상태를 실시간으로 보여 준다. 0.12초마다 다시 그리고 1초마다 세션 파일을 다시 읽는다.
//! `q` 종료, `t` 테마 바꾸기(`notifier/board-theme` 에 남긴다). 화면 문자열은 Python 과 같은 ANSI 문자열을 직접 만들고,
//! crossterm 은 raw 모드·대체 화면·키 읽기·창 크기에만 쓴다.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use claude_board_core::text::is_py_space;
use claude_board_core::{claude_live, clock, fileio, paths, proc, Record};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use serde_json::Value;
use unicode_width::UnicodeWidthChar;

/// 화면을 다시 그리는 간격(애니메이션)
const FRAME: Duration = Duration::from_millis(120);
/// 상태 파일을 다시 읽는 간격
const RELOAD: Duration = Duration::from_secs(1);
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const STATES: [&str; 4] = ["waiting", "working", "idle", "ended"];

type Rgb = (u8, u8, u8);

/// 보드 색 테마 — working, waiting, idle, ended, accent, text, muted, faint, base
struct Theme {
    name: &'static str,
    colors: [Rgb; 9],
}

const KEYS: [&str; 9] = ["working", "waiting", "idle", "ended", "accent", "text", "muted", "faint", "base"];

const THEMES: [Theme; 4] = [
    // Ghostty 테마와 같은 색
    Theme {
        name: "Catppuccin Mocha",
        colors: [(137, 180, 250), (250, 179, 135), (166, 227, 161), (108, 112, 134), (203, 166, 247), (205, 214, 244), (166, 173, 200), (88, 91, 112), (17, 17, 27)],
    },
    Theme {
        name: "Tokyo Night",
        colors: [(122, 162, 247), (255, 158, 100), (158, 206, 106), (86, 95, 137), (187, 154, 247), (192, 202, 245), (154, 165, 206), (65, 72, 104), (26, 27, 38)],
    },
    Theme {
        name: "Rosé Pine",
        colors: [(196, 167, 231), (246, 193, 119), (156, 207, 216), (110, 106, 134), (235, 188, 186), (224, 222, 244), (144, 140, 170), (82, 79, 103), (25, 23, 36)],
    },
    Theme {
        name: "Nord",
        colors: [(136, 192, 208), (235, 203, 139), (163, 190, 140), (97, 110, 136), (180, 142, 173), (236, 239, 244), (155, 165, 185), (67, 76, 94), (46, 52, 64)],
    },
];

fn order(state: Option<&str>) -> u8 {
    match state {
        Some("waiting") => 0,
        Some("working") => 1,
        Some("idle") => 2,
        Some("ended") => 3,
        _ => 9,
    }
}

fn state_name(state: &str) -> &str {
    match state {
        "working" => "작업 중",
        "waiting" => "승인 대기",
        "idle" => "대기",
        "ended" => "종료",
        other => other,
    }
}

/// 표시 폭 — 한글·이모지 등 넓은 글자는 2칸, 결합 문자는 0칸, 그 밖 1칸.
fn width(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(1)).sum()
}

/// 표시 폭 w 에 맞춰 자르거나 공백으로 채운다(한글·이모지는 2칸). 제어 문자는 화면을 깨므로 공백으로 바꾼다.
fn fit(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let sw = width(&s);
    if sw <= w {
        return s + &" ".repeat(w - sw);
    }
    let (mut out, mut used) = (String::new(), 0);
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(1);
        if used + cw > w - 1 {
            break;
        }
        out.push(c);
        used += cw;
    }
    out + "…" + &" ".repeat(w - used - 1)
}

fn ago(sec: f64) -> String {
    let sec = sec.max(0.0) as u64;
    if sec < 60 {
        format!("{sec}초")
    } else if sec < 3600 {
        format!("{}분 {:02}초", sec / 60, sec % 60)
    } else {
        format!("{}시간 {:02}분", sec / 3600, sec % 3600 / 60)
    }
}

fn get_str<'a>(s: &'a Record, key: &str) -> Option<&'a str> {
    s.get(key).and_then(Value::as_str)
}

fn get_f64(s: &Record, key: &str) -> Option<f64> {
    s.get(key).and_then(Value::as_f64)
}

fn non_empty<'a>(s: &'a Record, key: &str) -> Option<&'a str> {
    get_str(s, key).filter(|v| !v.is_empty())
}

/// 세션 파일을 읽는다. 프로세스가 죽은 세션은 종료로 보고, 종료된 지 30분 넘은 파일은 지운다.
///
/// working·waiting 인 세션은 Claude Code 의 신호로 덮는다(`board-tui.py:load`) — Esc 로 끊긴 세션은 입력을 기다리는 중이라
/// idle 이고 대기 시간은 끊은 때부터 센다. 승인한 도구가 도는 동안은 승인 대기로 남으므로 Claude Code 가 다시 busy 면 작업 중이다.
/// 겹치는 규칙은 [`claude_live::live_state`] 한 곳이다.
fn load(sessions: &Path, claude_dir: &Path, now: f64) -> Vec<Record> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(sessions) else { return out };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().ends_with(".json") {
            continue;
        }
        let Some(mut s) = fileio::read_record(&entry.path()) else { continue };
        if get_str(&s, "state") != Some("ended") && !proc::record_alive(&s) {
            s.insert("state".into(), "ended".into());
        }
        if let Some((state, since)) = claude_live::live_override(&s, claude_dir) {
            s.insert("state".into(), state.into());
            s.insert("since".into(), since.into());
        }
        if get_str(&s, "state") == Some("ended") && now - get_f64(&s, "updated").unwrap_or(0.0) > 1800.0 {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        out.push(s);
    }
    out
}

struct Painter<'a> {
    theme: &'a Theme,
}

impl Painter<'_> {
    fn rgb(&self, key: &str) -> Rgb {
        // 모르는 상태 이름은 흐린 글자색으로 그린다
        let key = if KEYS.contains(&key) { key } else { "muted" };
        self.theme.colors[KEYS.iter().position(|k| *k == key).unwrap_or(6)]
    }
    fn fg(&self, key: &str) -> String {
        let (r, g, b) = self.rgb(key);
        format!("\x1b[38;2;{r};{g};{b}m")
    }
    fn bg(&self, key: &str) -> String {
        let (r, g, b) = self.rgb(key);
        format!("\x1b[48;2;{r};{g};{b}m")
    }
    fn dark(&self) -> String {
        self.fg("base")
    }

    /// 상태별 움직이는 표시 — 작업 중은 회전, 승인 대기는 깜빡임.
    fn indicator(&self, state: &str, tick: usize) -> String {
        match state {
            "working" => format!("{}{}", self.fg("working"), SPINNER[tick % SPINNER.len()]),
            "waiting" => format!("{}{}", self.fg("waiting"), if (tick / 4).is_multiple_of(2) { "◉" } else { "○" }),
            "idle" => format!("{}✓", self.fg("idle")),
            _ => format!("{}✕", self.fg("ended")),
        }
    }

    fn pill(&self, state: &str) -> String {
        let label = format!(" {} ", state_name(state));
        format!("{}{}{BOLD}{}{RESET}", self.bg(state), self.dark(), fit(&label, 11))
    }

    fn header(&self, sessions: &[Record], cols: usize, now: f64) -> Vec<String> {
        let title = format!("  {}{BOLD}✳ Claude Code{RESET}  {}세션 보드{RESET}", self.fg("accent"), self.fg("muted"));
        let clock = format!("{}{}{RESET}  ", self.fg("muted"), clock::local(now).hms());
        let pad = (cols as i64 - width("  ✳ Claude Code  세션 보드") as i64 - 11).max(1) as usize;
        let mut lines = vec![format!("{title}{}{clock}", " ".repeat(pad))];
        let parts: Vec<String> = STATES
            .iter()
            .filter_map(|k| {
                let n = sessions.iter().filter(|s| get_str(s, "state") == Some(k)).count();
                (n > 0).then(|| format!("{}{}{BOLD} {} {n} {RESET}", self.bg(k), self.dark(), state_name(k)))
            })
            .collect();
        let total = sessions.iter().filter(|s| get_str(s, "state") != Some("ended")).count();
        lines.push(format!("  {}활성 {total}개{RESET}   {}", self.fg("muted"), parts.join("  ")));
        lines.push(String::new());
        lines
    }

    fn card(&self, cwd: &str, members: &[&Record], cols: usize, now: f64, tick: usize, home: &str) -> Vec<String> {
        // " │ " + 내용 + " │" 에 마지막 한 칸을 비운다
        let inner = cols as i64 - 6;
        let urgent = members
            .iter()
            .min_by_key(|s| order(get_str(s, "state")))
            .and_then(|s| get_str(s, "state"))
            .unwrap_or("idle");
        let edge = self.fg(urgent);
        let name = match paths::basename(cwd) {
            "" => cwd,
            n => n,
        };
        let path = if home.is_empty() { cwd.to_string() } else { cwd.replacen(home, "~", 1) };

        // 윗변: 폴더 이름(왼쪽)과 경로(오른쪽)를 테두리 안에 새긴다
        let left = format!(" 📁 {name} ");
        let mut right = format!(" {path} ");
        let room = cols as i64 - 2 - width(&left) as i64 - 2;
        if width(&right) as i64 > room - 2 {
            right = format!(" {} ", fit(&path, (room - 4).max(0) as usize).trim_end_matches(is_py_space));
        }
        let dash = (cols as i64 - 6 - width(&left) as i64 - width(&right) as i64).max(1) as usize;
        let mut lines = vec![format!(
            " {edge}╭─{RESET}{BOLD}{}{left}{RESET}{edge}{}{RESET}{}{right}{RESET}{edge}─╮{RESET}",
            self.fg("text"),
            "─".repeat(dash),
            self.fg("muted"),
        )];

        for s in members {
            let st = get_str(s, "state").unwrap_or("idle");
            let elapsed = ago(now - get_f64(s, "since").unwrap_or(now));
            let when = match st {
                "ended" => format!("{} 종료", clock::local(get_f64(s, "updated").unwrap_or(now)).hm()),
                "idle" => format!("{elapsed} 전"),
                _ => elapsed,
            };
            let raw = non_empty(s, "topic").or_else(|| non_empty(s, "last_message")).unwrap_or("요청 전");
            // 마크다운 기호 #*` 를 지운다(re.sub(r"[#*`]+", "", …))
            let topic = raw.replace(['#', '*', '`'], "");
            let topic = topic.trim_matches(is_py_space);
            let tag = if s.get("headless").is_some_and(|v| v.as_bool() == Some(true)) {
                "백그라운드"
            } else {
                non_empty(s, "tty").unwrap_or("")
            };
            let topic_w = (inner - (1 + 2 + 11 + 2 + 12 + 2) - (width(tag) as i64 + 2)).max(8) as usize;
            let text_color = if st == "ended" { self.fg("muted") } else { self.fg("text") };
            let row = format!(
                "{}{RESET}  {}  {}{}{RESET}  {text_color}{}{RESET}  {}{tag}{RESET}",
                self.indicator(st, tick),
                self.pill(st),
                self.fg("muted"),
                fit(&when, 12),
                fit(topic, topic_w),
                self.fg("faint"),
            );
            lines.push(format!(" {edge}│{RESET} {row} {edge}│{RESET}"));
            if st == "waiting" {
                if let Some(m) = non_empty(s, "last_message") {
                    let msg = format!("↳ {m}");
                    let w = (inner - 3).max(0) as usize;
                    lines.push(format!(" {edge}│{RESET}    {}{}{RESET} {edge}│{RESET}", self.fg("waiting"), fit(&msg, w)));
                }
            }
        }

        // 아랫변: 이 그룹의 상태별 세션 수
        let counts: Vec<(&str, usize)> = STATES
            .iter()
            .map(|k| (*k, members.iter().filter(|s| get_str(s, "state") == Some(k)).count()))
            .filter(|(_, n)| *n > 0)
            .collect();
        let badge_plain = counts.iter().map(|(_, n)| format!("● {n}")).collect::<Vec<_>>().join("  ");
        let badge = counts.iter().map(|(k, n)| format!("{}● {n}{RESET}", self.fg(k))).collect::<Vec<_>>().join("  ");
        let dash = (cols as i64 - 2 - 2 - width(&badge_plain) as i64 - 2 - 1).max(1) as usize;
        lines.push(format!(" {edge}╰{}{RESET} {badge} {edge}─╯{RESET}", "─".repeat(dash)));
        lines
    }

    /// 한 화면 전체(커서를 맨 위로 옮겨 덮어 그리는 문자열).
    fn render(&self, sessions: &[Record], tick: usize, size: (usize, usize), now: f64, home: &str) -> String {
        let (cols, rows) = (size.0.max(60), size.1);
        // 프로젝트 경로(claude 를 띄운 폴더, 없으면 cwd)별로 묶고(처음 나온 순서), 묶음 안은 급한 상태 → 최근에 바뀐 것 순
        let mut groups: Vec<(String, Vec<&Record>)> = Vec::new();
        for s in sessions {
            let dir = get_str(s, "project_dir").filter(|d| !d.is_empty()).or_else(|| get_str(s, "cwd")).unwrap_or("?").to_string();
            match groups.iter_mut().find(|(c, _)| *c == dir) {
                Some((_, g)) => g.push(s),
                None => groups.push((dir, vec![s])),
            }
        }
        let key = |s: &Record| (order(get_str(s, "state")), -get_f64(s, "since").unwrap_or(0.0));
        for (_, g) in &mut groups {
            g.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
        }
        let gkey = |g: &[&Record]| {
            let urgent = g.iter().map(|s| order(get_str(s, "state"))).min().unwrap_or(9);
            let latest = g.iter().map(|s| get_f64(s, "updated").unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
            (urgent, -latest)
        };
        groups.sort_by(|a, b| gkey(&a.1).partial_cmp(&gkey(&b.1)).unwrap_or(std::cmp::Ordering::Equal));

        let mut lines = self.header(sessions, cols, now);
        if groups.is_empty() {
            lines.push(String::new());
            lines.push(String::new());
            lines.push(format!("{}{}아직 기록된 세션이 없습니다{RESET}", " ".repeat((cols / 2).saturating_sub(18)), self.fg("muted")));
            lines.push(format!(
                "{}{}Claude Code 세션에서 요청을 보내면 나타납니다{RESET}",
                " ".repeat((cols / 2).saturating_sub(22)),
                self.fg("faint")
            ));
        }
        for (cwd, members) in &groups {
            lines.extend(self.card(cwd, members, cols, now, tick, home));
            lines.push(String::new());
        }

        let faint = self.fg("faint");
        let footer = format!(
            "  {}{}{BOLD} q {RESET}{faint} 종료   {}⠹{faint} 작업 중   {}◉{faint} 승인 대기   {}✓{faint} 대기   {}✕{faint} 종료   {}{}{BOLD} t {RESET}{faint} 테마: {}{}{RESET}",
            self.bg("faint"),
            self.fg("text"),
            self.fg("working"),
            self.fg("waiting"),
            self.fg("idle"),
            self.fg("ended"),
            self.bg("faint"),
            self.fg("text"),
            self.fg("accent"),
            self.theme.name,
        );
        let body = rows.saturating_sub(1);
        lines.truncate(body);
        lines.resize(body, String::new());
        lines.push(footer);
        // raw 모드에서는 \n 이 줄 처음으로 돌아가지 않으므로 \r\n 으로 잇는다
        let joined: Vec<String> = lines.into_iter().map(|l| l + "\x1b[K").collect();
        format!("\x1b[?2026h\x1b[H{}\x1b[J\x1b[?2026l", joined.join("\r\n"))
    }
}

fn load_theme(file: &Path) -> usize {
    let name = fs::read_to_string(file).map(|s| s.trim().to_string()).unwrap_or_default();
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

/// 창 크기 — Python `shutil.get_terminal_size((100, 30))` 처럼 COLUMNS·LINES 를 먼저 보고, 0 은 모르는 값으로 친다.
fn terminal_size() -> (usize, usize) {
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
    let (mut cols, mut rows) = (env("COLUMNS"), env("LINES"));
    if cols == 0 || rows == 0 {
        let (c, r) = terminal::size().map(|(c, r)| (usize::from(c), usize::from(r))).unwrap_or((0, 0));
        if cols == 0 {
            cols = if c > 0 { c } else { 100 };
        }
        if rows == 0 {
            rows = if r > 0 { r } else { 30 };
        }
    }
    (cols, rows)
}

/// 끝날 때(패닉 포함) 터미널을 되돌린다.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = out.flush();
    }
}

/// `tui` — q 로 끝날 때까지 보드를 그린다.
pub fn run() -> ExitCode {
    // Windows 콘솔은 VT 처리를 켜야 ANSI 문자열을 그린다
    #[cfg(windows)]
    crossterm::ansi_support::supports_ansi();
    let sessions_dir = paths::sessions_dir();
    let theme_file = paths::board_theme_file();
    let claude_dir = paths::claude_dir();
    let home = paths::home().to_string_lossy().into_owned();
    let mut theme = load_theme(&theme_file);

    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("터미널에서 실행해야 합니다: {e}");
        return ExitCode::FAILURE;
    }
    let _restore = Restore;
    let mut out = io::stdout();
    let _ = out.write_all("\x1b]2;📋 Claude 세션 보드\x07\x1b[?1049h\x1b[?25l".as_bytes());

    let mut sessions = Vec::new();
    let mut loaded: Option<Instant> = None;
    let mut tick = 0usize;
    loop {
        if loaded.is_none_or(|t| t.elapsed() >= RELOAD) {
            sessions = load(&sessions_dir, &claude_dir, clock::now());
            loaded = Some(Instant::now());
        }
        let size = terminal_size();
        let screen = Painter { theme: &THEMES[theme] }.render(&sessions, tick, size, clock::now(), &home);
        let _ = out.write_all(screen.as_bytes());
        let _ = out.flush();
        tick += 1;
        if !event::poll(FRAME).unwrap_or(false) {
            continue;
        }
        let Ok(Event::Key(key)) = event::read() else { continue };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char(c) if c.eq_ignore_ascii_case(&'q') => break,
            KeyCode::Char(c) if c.eq_ignore_ascii_case(&'t') => {
                theme = (theme + 1) % THEMES.len();
                let _ = fs::write(&theme_file, THEMES[theme].name);
            }
            _ => {}
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn widths_and_fit() {
        assert_eq!(width("세션 보드"), 9);
        assert_eq!(width("  ✳ Claude Code  세션 보드"), 26);
        assert_eq!(width("📁 ⠋◉✓✕●…─"), 2 + 1 + 7);
        assert_eq!(fit("가나다", 7), "가나다 ");
        assert_eq!(fit("가나다라", 6), "가나… ");
        assert_eq!(fit("abc", 0), "");
        assert_eq!(fit("a\nb", 4), "a b ");
    }

    #[test]
    fn ago_formats() {
        assert_eq!(ago(-5.0), "0초");
        assert_eq!(ago(59.9), "59초");
        assert_eq!(ago(61.0), "1분 01초");
        assert_eq!(ago(3725.0), "1시간 02분");
    }

    /// CSI(`ESC [ … 글자`) 를 지운다.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\x1b' && it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn render_groups_and_orders() {
        let now = 10_000.0;
        let rec = |v: Value| v.as_object().unwrap().clone();
        let sessions = vec![
            rec(json!({"cwd": "/home/u/a", "state": "idle", "since": 9_000.0, "updated": 9_500.0, "topic": "**굵은** 주제"})),
            rec(json!({"cwd": "/home/u/b", "state": "waiting", "since": 9_990.0, "updated": 9_990.0, "headless": true,
                       "last_message": "Claude needs your permission"})),
            rec(json!({"cwd": "/home/u/a", "state": "working", "since": 9_900.0, "updated": 9_900.0})),
        ];
        let screen = Painter { theme: &THEMES[0] }.render(&sessions, 0, (80, 30), now, "/home/u");
        let plain = strip_ansi(&screen);
        let lines: Vec<&str> = plain.split("\r\n").collect();
        assert_eq!(lines.len(), 30);
        assert!(lines[1].starts_with("  활성 3개    승인 대기 1    작업 중 1    대기 1 "), "{:?}", lines[1]);
        // 승인 대기가 있는 b 카드가 먼저, a 카드 안은 작업 중 → 대기
        assert!(lines[3].contains("📁 b") && lines[3].contains("~/b"), "{:?}", lines[3]);
        assert!(lines[4].contains("승인 대기") && lines[4].contains("백그라운드"));
        assert!(lines[5].contains("↳ Claude needs your permission"));
        let a = lines.iter().position(|l| l.contains("📁 a")).unwrap();
        assert!(lines[a + 1].contains("⠋") && lines[a + 1].contains("1분 40초") && lines[a + 1].contains("요청 전"));
        assert!(lines[a + 2].contains("✓") && lines[a + 2].contains("16분 40초 전") && lines[a + 2].contains("굵은 주제"));
        assert!(lines[a + 3].contains("● 1  ● 1"));
        assert!(lines[29].contains("테마: Catppuccin Mocha"));
        // 세션이 없으면 안내 문구
        let empty = strip_ansi(&Painter { theme: &THEMES[1] }.render(&[], 3, (60, 10), now, ""));
        assert!(empty.contains("아직 기록된 세션이 없습니다"));
        assert!(empty.contains("테마: Tokyo Night"));
    }

    #[test]
    fn load_overlays_interrupted_and_busy_sessions() {
        let root = std::env::temp_dir().join(format!("cb-tui-load-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (sessions, claude) = (root.join("sessions"), root.join("claude"));
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(claude.join("sessions")).unwrap();
        let stamp = "2026-10-16T19:24:08.5Z";
        let transcript = root.join("t.jsonl");
        let line = json!({"type": "user", "timestamp": stamp, "message": {"content": "[Request interrupted by user]"}});
        fs::write(&transcript, line.to_string()).unwrap();
        let write = |name: &str, v: Value| fs::write(sessions.join(name), v.to_string()).unwrap();
        let t = transcript.to_string_lossy().into_owned();
        // working + 중단된 기록 → idle, since 는 중단 시각
        write("a.json", json!({"session_id": "a", "state": "working", "since": 1.0, "updated": 2.0, "transcript_path": t}));
        // waiting + Claude Code status busy → working, since 는 status 가 바뀐 시각 (살아 있는 pid 로 이 프로세스를 쓴다)
        let pid = std::process::id();
        fs::write(claude.join(format!("sessions/{pid}.json")), r#"{"sessionId":"b","status":"busy","statusUpdatedAt":7000}"#).unwrap();
        write("b.json", json!({"session_id": "b", "state": "waiting", "since": 1.0, "updated": 2.0, "pid": pid}));
        // 겹칠 것이 없는 세션은 그대로
        write("c.json", json!({"session_id": "c", "state": "working", "since": 1.0, "updated": 2.0}));
        let mut got = load(&sessions, &claude, 100.0);
        got.sort_by_key(|s| get_str(s, "session_id").unwrap_or("").to_string());
        let view: Vec<(&str, f64)> = got.iter().map(|s| (get_str(s, "state").unwrap(), get_f64(s, "since").unwrap())).collect();
        assert_eq!(view, [("idle", 1_792_178_648.5), ("working", 7.0), ("working", 1.0)]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn render_groups_by_project_dir() {
        let rec = |v: Value| v.as_object().unwrap().clone();
        // 워크트리에 들어가 cwd 가 바뀐 세션도 claude 를 띄운 폴더의 카드에, project_dir 이 없는 예전 기록은 cwd 의 카드에
        let sessions = vec![
            rec(json!({"cwd": "/home/u/a", "project_dir": "/home/u/a", "state": "idle", "since": 1.0, "updated": 1.0})),
            rec(json!({"cwd": "/home/u/a/.claude/worktrees/t1", "project_dir": "/home/u/a", "state": "working", "since": 2.0, "updated": 2.0})),
            rec(json!({"cwd": "/home/u/a", "state": "idle", "since": 3.0, "updated": 3.0})),
        ];
        let plain = strip_ansi(&Painter { theme: &THEMES[0] }.render(&sessions, 0, (80, 30), 10.0, "/home/u"));
        assert_eq!(plain.matches("📁 a").count(), 1, "{plain}");
        assert!(!plain.contains("📁 t1"), "{plain}");
    }
}
