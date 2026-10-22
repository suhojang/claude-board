//! 프론트엔드에 보내는 상태 묶음의 모양 — `app/src/lib/types.ts`·SPEC §6.1 과 같게 유지한다.
//! 시각은 모두 유닉스 밀리초. 백엔드만 쓰는 값은 `serde(skip)` 으로 뺀다.

use std::path::PathBuf;

use serde::Serialize;

/// 응답을 만든 모델과 effort (`ClaudeBoard.swift` Brain).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brain {
    pub model: String,
    pub effort: String,
    pub family: String,
    pub version: String,
    pub effort_level: u8,
}

const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

impl Brain {
    pub fn new(model: &str, effort: &str) -> Self {
        Brain {
            model: model.to_string(),
            effort: effort.to_string(),
            family: family(model),
            version: version(model),
            effort_level: EFFORTS.iter().position(|e| *e == effort).map_or(0, |i| i as u8 + 1),
        }
    }
}

/// 모델 계열 이름 — opus/sonnet/haiku/fable 이 들어 있으면 첫 글자 대문자, 아니면 `claude-` 를 뺀 이름.
fn family(model: &str) -> String {
    for f in ["opus", "sonnet", "haiku", "fable"] {
        if model.contains(f) {
            return f[..1].to_uppercase() + &f[1..];
        }
    }
    model.replace("claude-", "")
}

/// 계열 뒤 버전 숫자 — 정규식 `-(\d+)-(\d+)` 의 첫 일치. claude-opus-5-5 → 5.5
fn version(model: &str) -> String {
    let b = model.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    for (i, &c) in b.iter().enumerate() {
        if c != b'-' {
            continue;
        }
        let a = digits(i + 1);
        if a == 0 || b.get(i + 1 + a) != Some(&b'-') {
            continue;
        }
        let d = digits(i + 2 + a);
        if d > 0 {
            return format!("{}.{}", &model[i + 1..i + 1 + a], &model[i + 2 + a..i + 2 + a + d]);
        }
    }
    String::new()
}

/// 컨텍스트 사용량.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Ctx {
    pub used: i64,
    pub size: i64,
    pub pct: i64,
}

impl Ctx {
    pub fn new(used: i64, size: i64) -> Self {
        let pct = if size > 0 { (used * 100 / size).min(100) } else { 0 };
        Ctx { used, size, pct }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sub {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub desc: String,
    pub state: String,
    pub started: i64,
    pub ended: Option<i64>,
    pub tool: String,
    pub background: bool,
    pub brain: Option<Brain>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub cwd: String,
    pub state: String,
    pub since: i64,
    pub updated: i64,
    pub headless: bool,
    pub title: String,
    pub topic: String,
    pub message: String,
    /// Claude Code 세션 이름(`~/.claude/sessions/<pid>.json` 의 `name`, SPEC §2.3) — 없거나 종료된 세션이면 None
    pub name: Option<String>,
    pub ctx: Option<Ctx>,
    pub brain: Option<Brain>,
    pub subagents: Vec<Sub>,
    pub shells: usize,
    pub paused: bool,
    pub muted: bool,
    pub limit_at: Option<i64>,
    pub can_drive: bool,
    // ── 백엔드만 쓰는 값 ──
    #[serde(skip)]
    pub pid: u32,
    /// pid 재사용을 가려내는 생성 시각(Windows hook 이 기록)
    #[serde(skip)]
    pub pid_ctime: Option<f64>,
    #[serde(skip)]
    pub file: PathBuf,
    /// claude 를 띄운 폴더(hook 의 `project_dir`, 없으면 cwd) — cd·워크트리에도 그대로라 카드는 이것으로 묶는다
    #[serde(skip)]
    pub project_dir: String,
    /// `claude` 뒤에 붙일 이어 열기 인자(hook 이 기록한 `resume_args`, 없으면 `--resume <id>`)
    #[serde(skip)]
    pub resume_args: Vec<String>,
    /// since·updated 의 유닉스 초 — 숨김·자동 접기 비교에 쓴다
    #[serde(skip)]
    pub since_s: f64,
    #[serde(skip)]
    pub updated_s: f64,
    #[serde(skip)]
    pub limit_s: Option<f64>,
}

impl Session {
    /// 그룹 안 정렬 순위 — 승인 대기 0, 작업 중 1, 일시중지·대기 2, 종료 3.
    pub fn rank(&self) -> u8 {
        match self.state.as_str() {
            "waiting" => 0,
            "working" => 1,
            "paused" | "idle" => 2,
            "ended" => 3,
            _ => 9,
        }
    }

    /// 작업 폴더의 마지막 이름(로그·대화상자 문구용).
    pub fn project(&self) -> String {
        last_name(&self.cwd)
    }
}

/// 경로의 마지막 이름 — 끝의 구분자는 무시한다(NSString.lastPathComponent).
pub fn last_name(path: &str) -> String {
    let seps: &[char] = &['/', '\\'];
    let trimmed = path.trim_end_matches(seps);
    if trimmed.is_empty() {
        return path.to_string();
    }
    trimmed.rsplit(seps).next().unwrap_or(trimmed).to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub path: String,
    pub sessions: Vec<Session>,
}

impl Group {
    pub fn min_rank(&self) -> u8 {
        self.sessions.iter().map(Session::rank).min().unwrap_or(9)
    }

    pub fn last_updated(&self) -> f64 {
        self.sessions.iter().map(|s| s.updated_s).fold(f64::NEG_INFINITY, f64::max)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Counts {
    pub waiting: usize,
    pub working: usize,
    pub idle: usize,
    pub paused: usize,
    pub ended: usize,
}

impl Counts {
    pub fn of(groups: &[Group]) -> Self {
        let mut c = Counts::default();
        for s in groups.iter().flat_map(|g| &g.sessions) {
            match s.state.as_str() {
                "waiting" => c.waiting += 1,
                "working" => c.working += 1,
                "idle" => c.idle += 1,
                "paused" => c.paused += 1,
                "ended" => c.ended += 1,
                _ => {}
            }
        }
        c
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limit {
    pub id: String,
    pub label: String,
    pub short: String,
    pub tiny: String,
    pub pct: f64,
    pub resets: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageView {
    pub limits: Vec<Limit>,
    pub failed: bool,
    pub token_expired: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccountView {
    pub id: String,
    pub email: String,
    pub org: String,
    pub plan: String,
    pub expired: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsView {
    pub current: Option<AccountView>,
    pub saved: Vec<AccountView>,
    pub adding: bool,
    pub switching: bool,
    pub auto_switch: bool,
    pub notice: Option<Notice>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdaterNotice {
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdaterView {
    pub current: String,
    pub dev: bool,
    pub latest: Option<String>,
    pub running: bool,
    pub failed: bool,
    pub checking: bool,
    pub notice: Option<UpdaterNotice>,
}

/// 프론트가 쓰는 보드 설정 — types.ts 의 BoardSettings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub collapsed: Vec<String>,
    pub ended_open: Vec<String>,
    pub pinned_groups: Vec<String>,
    pub kind_collapsed: Vec<String>,
    pub sub_collapsed: Vec<String>,
    pub usage_compact: bool,
    pub pinned: bool,
    pub mascot: String,
    pub opacity: f64,
    pub auto_switch: bool,
    pub board_mini: bool,
    pub terminal_expanded: bool,
    pub board_side: String,
    pub board_pane_width: f64,
    pub notify_position: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TerminalApp {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub now: i64,
    pub groups: Vec<Group>,
    pub all_muted: bool,
    pub counts: Counts,
    pub usage: UsageView,
    pub accounts: AccountsView,
    pub updater: UpdaterView,
    pub settings: SettingsView,
    pub claude_version: Option<String>,
    pub terminals: Vec<TerminalApp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brain_labels() {
        let b = Brain::new("claude-opus-5-5", "xhigh");
        assert_eq!((b.family.as_str(), b.version.as_str(), b.effort_level), ("Opus", "5.5", 4));
        let b = Brain::new("claude-fable-5-1-20261001", "");
        assert_eq!((b.family.as_str(), b.version.as_str(), b.effort_level), ("Fable", "5.1", 0));
        let b = Brain::new("claude-3-5-sonnet", "max");
        assert_eq!((b.family.as_str(), b.version.as_str(), b.effort_level), ("Sonnet", "3.5", 5));
        let b = Brain::new("claude-mystery", "low");
        assert_eq!((b.family.as_str(), b.version.as_str(), b.effort_level), ("mystery", "", 1));
    }

    #[test]
    fn ctx_pct_and_names() {
        assert_eq!(Ctx::new(312_000, 1_000_000).pct, 31);
        assert_eq!(Ctx::new(300_000, 200_000).pct, 100);
        assert_eq!(Ctx::new(5, 0).pct, 0);
        assert_eq!(last_name("/Users/me/work/proj/"), "proj");
        assert_eq!(last_name(r"C:\work\shop-api"), "shop-api");
        assert_eq!(last_name("?"), "?");
    }

    #[test]
    fn snapshot_wire_names() {
        let s = Sub {
            id: "a".into(),
            kind: "Explore".into(),
            desc: String::new(),
            state: "running".into(),
            started: 1,
            ended: None,
            tool: String::new(),
            background: false,
            brain: Some(Brain::new("claude-opus-5-5", "high")),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["type"], "Explore");
        assert_eq!(v["brain"]["effortLevel"], 3);
        assert!(v.get("kind").is_none());
    }
}
