//! 세션 읽기 — `Store.reload` (`ClaudeBoard.swift Store.reload`, SPEC §5.1).
//! 1초마다 `sessions/*.json` 을 읽어 경로별 그룹으로 묶는다. 프로세스 확인(셸 수·콘솔에 붙을 수 있는지)은
//! 느리므로 백그라운드에서 3초마다 센 값(`Probe`)을 받아 쓴다.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use claude_board_core::claude_live::{live_state, session_name, status_change};
use claude_board_core::{fileio, Record};
use serde_json::Value;

use crate::model::{last_name, Group, Session, Sub};
use crate::drive::can_drive;
use crate::readers::{ClaudeNames, ContextReader, InterruptReader, ModelReader};
use crate::settings::{set_in, Settings};
use crate::util::{ms, num, obj};

#[cfg(test)]
mod scan_table;

mod group_table;

/// 조용한 카드를 자동으로 접기까지의 시간
const QUIET_FOLD_AFTER: f64 = 30.0 * 60.0;
/// 종료된 세션 기록을 지우는 나이
const ENDED_KEEP: f64 = 2.0 * 86400.0;

/// 백그라운드에서 센 프로세스 정보.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    /// Claude 프로세스 pid → 백그라운드 셸 수
    pub shells: HashMap<u32, usize>,
    /// 콘솔에 붙을 수 있는 Claude 프로세스 pid
    pub drivable: HashSet<u32>,
    /// pid → 보드 터미널 짝(W2). 지금 claude 가 도는 보드 터미널 id 들.
    pub places: HashMap<u32, crate::model::PidPlace>,
    pub runs_claude: Vec<String>,
    pub at_prompt: Vec<String>,
}

pub struct Store {
    dir: PathBuf,
    claude_dir: PathBuf,
    home: String,
    records: HashMap<PathBuf, (SystemTime, Record)>,
    models: ModelReader,
    contexts: ContextReader,
    /// Esc 중단 표시(대화 기록 끝)와 Claude Code 의 `sessions\<pid>.json` — 둘 다 파일 수정 시각이 같으면 다시 읽지 않는다
    interrupts: InterruptReader,
    pub(crate) names: ClaudeNames,
    /// 직전 reload 때 승인 대기였던 세션 — 새로 승인 대기가 된 세션만 골라 그 그룹을 편다
    waiting_ids: HashSet<String>,
}

pub struct Reloaded {
    pub groups: Vec<Group>,
    pub all_muted: bool,
    /// 설정(접기·숨김·자동 접기)을 바꿨는지 — 저장해야 한다
    pub settings_changed: bool,
}

/// 대화 기록 위치 — hook 이 알려 준 경로가 없으면 `~/.claude/projects/<cwd 의 영숫자 밖 문자를 -로>/<sid>.jsonl`.
pub fn transcript_path(given: Option<&str>, cwd: &str, sid: &str, claude_dir: &Path) -> PathBuf {
    if let Some(g) = given.filter(|g| !g.is_empty()) {
        return PathBuf::from(g);
    }
    let slug: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    claude_dir.join("projects").join(slug).join(format!("{sid}.jsonl"))
}

/// 홈 폴더를 `~` 로 (Windows 는 대소문자를 가리지 않는다).
pub fn display_path(cwd: &str, home: &str) -> String {
    if home.is_empty() || cwd.len() < home.len() || !cwd.is_char_boundary(home.len()) {
        return cwd.to_string();
    }
    let (head, rest) = cwd.split_at(home.len());
    let same = if cfg!(windows) { head.eq_ignore_ascii_case(home) } else { head == home };
    let boundary = rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\');
    if same && boundary {
        format!("~{rest}")
    } else {
        cwd.to_string()
    }
}

/// topic·last_message 에서 마크다운 기호 `#*\`` 를 지운다.
fn clean(s: &str) -> String {
    let s: String = s.chars().filter(|c| !matches!(c, '#' | '*' | '`')).collect();
    s.trim_matches(|c| c == ' ' || c == '\t').to_string()
}

fn text(rec: &Record, key: &str) -> String {
    rec.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// 세션 기록의 서브에이전트 — 끝난 지 10초 넘은 것과 시작 hook 을 보지 못한 것(`started_seen` 이 참이 아님)은 뺀다.
/// done 이 아닌 것 먼저, 시작 순. 세션 상태에 반영되는 것도 이 목록뿐이다.
fn parse_subs(j: Option<&Record>, alive: bool, now: f64) -> Vec<(Sub, f64)> {
    let Some(j) = j.filter(|_| alive) else { return Vec::new() };
    let mut out: Vec<(Sub, f64)> = j
        .iter()
        .filter_map(|(id, v)| {
            let v = v.as_object()?;
            let ended = num(v.get("ended"));
            if ended.is_some_and(|e| now - e > 10.0) {
                return None;
            }
            // 시작 hook 없이 생긴 기록(끝 hook 이나 도구 hook 만 옴)은 실제 서브에이전트가 아니다 — 전사 파일도 종류도 없고,
            // 끝 hook 이 끝내 안 오면 실행 중으로 남아 세션을 계속 작업 중으로 보이게 하므로 목록에서 뺀다
            if v.get("started_seen").and_then(Value::as_bool) != Some(true) {
                return None;
            }
            let started = num(v.get("started")).unwrap_or(0.0);
            Some((
                Sub {
                    id: id.clone(),
                    kind: text(v, "type"),
                    desc: text(v, "description"),
                    state: v.get("state").and_then(Value::as_str).unwrap_or("running").to_string(),
                    started: ms(started),
                    ended: ended.map(ms),
                    tool: text(v, "tool"),
                    background: v.get("background").and_then(Value::as_bool).unwrap_or(false),
                    brain: None,
                },
                started,
            ))
        })
        .collect();
    out.sort_by(|(a, sa), (b, sb)| {
        ((a.state == "done") as u8, *sa).partial_cmp(&((b.state == "done") as u8, *sb)).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

impl Store {
    /// `sessions` = notifier/sessions, `notifier` = notifier 폴더(context·context-windows.json), `claude_dir` = ~/.claude
    pub fn new(sessions: PathBuf, notifier: &Path, claude_dir: PathBuf, home: String) -> Self {
        Store {
            dir: sessions,
            names: ClaudeNames::new(claude_dir.clone()),
            claude_dir,
            home,
            records: HashMap::new(),
            models: ModelReader::default(),
            contexts: ContextReader::new(notifier.join("context"), notifier.join("context-windows.json")),
            interrupts: InterruptReader::default(),
            waiting_ids: HashSet::new(),
        }
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.dir
    }

    /// 세션 파일을 읽어 그룹을 만든다. `alive` = 세션 기록의 pid 가 살아 있는지(`proc::record_alive`).
    pub fn reload(&mut self, settings: &mut Settings, probe: &Probe, alive: &dyn Fn(&Record) -> bool, mute_all: bool, now: f64) -> Reloaded {
        let mut sessions: Vec<Session> = Vec::new();
        let mut brain_paths = HashSet::new();
        let mut pids = HashSet::new();
        // 컨텍스트 캐시 정리(A1) — 목록에 남은 세션 몫(끝난 것 포함)만 남기기 위한 목록
        let mut ctx_sids = HashSet::new();
        let mut ctx_transcripts = HashSet::new();
        let mut seen = HashSet::new();
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".json")).collect())
            .unwrap_or_default();
        names.sort();
        for name in names {
            let path = self.dir.join(&name);
            let Some(m) = fs::metadata(&path).ok().and_then(|md| md.modified().ok()) else { continue };
            seen.insert(path.clone());
            let j = match self.records.get(&path) {
                Some((cm, j)) if *cm == m => j.clone(),
                _ => {
                    let Some(parsed) = fileio::read_record(&path) else { continue };
                    self.records.insert(path.clone(), (m, parsed.clone()));
                    parsed
                }
            };
            if let Some(s) = self.session(&j, &name, &path, probe, alive, now, &mut brain_paths, &mut pids, &mut ctx_sids, &mut ctx_transcripts) {
                pids.insert(s.pid);
                sessions.push(s);
            }
        }
        self.models.prune(&brain_paths);
        self.interrupts.prune(&brain_paths);
        self.names.prune(&pids);
        // 컨텍스트 캐시도 목록에 남은 세션(끝난 것 포함) 몫만 남긴다
        self.contexts.prune(&ctx_sids, &ctx_transcripts);
        self.records.retain(|k, _| seen.contains(k));

        let mut changed = self.expand_newly_waiting(&sessions, settings);

        // 프로젝트 경로별로 묶고, 그룹 안은 급한 순 → 최근 상태 변화 순
        let mut by_dir: HashMap<String, Vec<Session>> = HashMap::new();
        for s in sessions {
            by_dir.entry(s.project_dir.clone()).or_default().push(s);
        }
        let mut visible: Vec<Group> = Vec::new();
        for (dir, mut list) in by_dir {
            list.sort_by(|a, b| a.rank().cmp(&b.rank()).then(b.since_s.partial_cmp(&a.since_s).unwrap_or(std::cmp::Ordering::Equal)));
            let g = Group { name: last_name(&dir), path: display_path(&dir, &self.home), id: dir, sessions: list };
            // 지운 뒤 새 활동이 없는 작업 경로는 빼고, 새 활동이 생긴 작업 경로는 지운 목록에서 꺼낸다
            if let Some(&at) = settings.hidden_groups.get(&g.id) {
                if g.sessions.iter().all(|s| s.updated_s <= at) {
                    continue;
                }
                settings.hidden_groups.remove(&g.id);
                changed = true;
            }
            visible.push(g);
        }
        changed |= fold_quiet(&visible, settings, now);
        let pins = settings.pinned_groups.clone();
        let pin_at = |id: &str| pins.iter().position(|p| p == id).unwrap_or(usize::MAX);
        visible.sort_by(|a, b| {
            pin_at(&a.id)
                .cmp(&pin_at(&b.id))
                .then(a.min_rank().cmp(&b.min_rank()))
                .then(b.last_updated().partial_cmp(&a.last_updated()).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.id.cmp(&b.id))
        });
        Reloaded { groups: visible, all_muted: mute_all, settings_changed: changed }
    }

    #[allow(clippy::too_many_arguments)]
    fn session(
        &mut self,
        j: &Record,
        name: &str,
        path: &Path,
        probe: &Probe,
        alive: &dyn Fn(&Record) -> bool,
        now: f64,
        brain_paths: &mut HashSet<PathBuf>,
        pids: &mut HashSet<u32>,
        ctx_sids: &mut HashSet<String>,
        ctx_transcripts: &mut HashSet<PathBuf>,
    ) -> Option<Session> {
        let mut state = j.get("state").and_then(Value::as_str).unwrap_or("idle").to_string();
        let updated = num(j.get("updated")).unwrap_or(0.0);
        // pid 없는 기록의 생사 판정에 sessionId 가 필요하므로 상태 판정보다 먼저 꺼낸다
        let sid = j.get("session_id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| name.trim_end_matches(".json").to_string());
        // Session.pid 는 기록 값을 그대로 두고, 생사·status·이름 판정에 쓰는 pid 를 따로 둔다(SPEC §5.1)
        let pid = j.get("pid").and_then(Value::as_u64).filter(|&p| p <= u64::from(u32::MAX)).unwrap_or(0) as u32;
        let mut lookup_pid = pid;
        if state != "ended" {
            if pid == 0 {
                // pid 없는 기록은 Claude Code 세션 파일에서 sessionId 로 pid 를 찾아 생사를 본다.
                // 찾지 못하면 pid 0 으로 판정하므로 record_alive 의 pid 없음=산 것 규칙이 그대로 이어진다
                let found = self.names.pid_for_session(&sid).unwrap_or(0);
                let mut judged = Record::new();
                judged.insert("pid".to_owned(), Value::from(found));
                if found > 0 {
                    // 찾은 pid 로 status·이름도 읽으므로 이름 캐시 정리에서 남긴다
                    pids.insert(found);
                }
                if !alive(&judged) {
                    state = "ended".into();
                } else {
                    lookup_pid = found;
                }
            } else if !alive(j) {
                state = "ended".into();
            }
        }
        let cwd = j.get("cwd").and_then(Value::as_str).unwrap_or("?").to_string();
        let project_dir = j.get("project_dir").and_then(Value::as_str).filter(|d| !d.is_empty()).map_or_else(|| cwd.clone(), str::to_string);
        let transcript = transcript_path(j.get("transcript_path").and_then(Value::as_str), &cwd, &sid, &self.claude_dir);
        // 질문 없이 끝난 세션은 대화 기록 파일이 없어 이어 열 수 없으므로 목록에서 뺀다
        if state == "ended" && (now - updated > ENDED_KEEP || !transcript.exists()) {
            let _ = fs::remove_file(path);
            return None;
        }
        // Esc 로 끊은 세션은 입력을 기다리는 중이고, 승인한 도구가 도는 세션은 작업 중이다(SPEC §3.9).
        // 끊은 뒤에도 서브에이전트·셸이 돌면 아래에서 다시 작업 중이 된다
        let mut since = num(j.get("since")).unwrap_or(0.0);
        let (interrupts, names) = (&mut self.interrupts, &mut self.names);
        let live = live_state(
            &state,
            || interrupts.read(&transcript),
            || status_change(&names.get(lookup_pid)?, &sid, since),
        );
        if let Some((s, at)) = live {
            state = s.to_string();
            since = at;
        }
        // 응답은 끝났어도 서브에이전트나 백그라운드 셸이 돌고 있으면 작업 중으로 보인다
        let base = transcript.to_string_lossy();
        let sub_dir = PathBuf::from(base.strip_suffix(".jsonl").unwrap_or(&base)).join("subagents");
        let subs: Vec<Sub> = parse_subs(obj(j.get("subagents")), state != "ended", now)
            .into_iter()
            .map(|(mut sub, _)| {
                let p = sub_dir.join(format!("agent-{}.jsonl", sub.id));
                sub.brain = self.models.read(&p, "", now);
                brain_paths.insert(p);
                sub
            })
            .collect();
        // 응답 중에 도는 셸은 보통 도구 호출이라 세지 않는다
        let shells = if state == "idle" { probe.shells.get(&pid).copied().unwrap_or(0) } else { 0 };
        if state == "idle" && (shells > 0 || subs.iter().any(|s| s.state != "done")) {
            state = "working".into();
        }
        // 일시중지 표지는 보드가 남기고, 새 요청이 들어오면 hook 이 지운다
        let paused = self.dir.join(format!("{sid}.paused")).exists();
        if paused && state != "ended" {
            state = "paused".into();
        }
        let muted = self.dir.join(format!("{sid}.mute")).exists();
        let model = text(j, "model");
        let mut brain = None;
        if state != "ended" {
            brain = self.models.read(&transcript, &model, now);
            brain_paths.insert(transcript.clone());
            if let (Some(b), Some(e)) = (brain.as_mut(), self.contexts.live_effort(&sid, &transcript)) {
                *b = crate::model::Brain::new(&b.model.clone(), &e);
            }
        }
        let ctx = self.contexts.read(&sid, &transcript, &model, now);
        let topic = clean(&text(j, "topic"));
        let message = clean(&text(j, "last_message"));
        let title = if !topic.is_empty() {
            topic.clone()
        } else if !message.is_empty() {
            message.clone()
        } else {
            "요청 전".to_string()
        };
        let place = probe.places.get(&pid).cloned().unwrap_or_default();
        let headless = j.get("headless").and_then(Value::as_bool).unwrap_or(false) && place.tty.is_none();
        let limit_s = obj(j.get("limit_hit")).and_then(|l| num(l.get("at")));
        let resume_args = j
            .get("resume_args")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| vec!["--resume".to_string(), sid.clone()]);
        let name = if state == "ended" { None } else { self.names.get(lookup_pid).as_ref().and_then(session_name).map(str::to_string) };
        let can_drive = can_drive(&state, pid, headless, &place, &probe.runs_claude, probe.drivable.contains(&pid));
        // 컨텍스트 캐시 정리(A1)에 쓸 목록 — 목록에 남은 세션(끝난 것 포함) 몫만 남기기 위해서다
        ctx_sids.insert(sid.clone());
        ctx_transcripts.insert(transcript.clone());
        Some(Session {
            id: sid,
            cwd,
            since: ms(since),
            updated: ms(updated),
            headless,
            title,
            topic,
            message,
            ctx,
            brain,
            name,
            subagents: subs,
            shells,
            paused,
            muted,
            limit_at: limit_s.map(ms),
            can_drive,
            place,
            pid,
            pid_ctime: num(j.get("pid_ctime")),
            file: path.to_path_buf(),
            project_dir,
            resume_args,
            since_s: since,
            updated_s: updated,
            limit_s,
            state,
        })
    }

    /// 새로 승인 대기가 된 세션의 그룹은 접혀 있으면 편다.
    fn expand_newly_waiting(&mut self, sessions: &[Session], settings: &mut Settings) -> bool {
        let waiting: Vec<&Session> = sessions.iter().filter(|s| s.state == "waiting").collect();
        let opened: HashSet<&str> = waiting
            .iter()
            .filter(|s| !self.waiting_ids.contains(&s.id))
            .map(|s| s.project_dir.as_str())
            .filter(|c| settings.collapsed.iter().any(|x| x == c))
            .collect();
        self.waiting_ids = waiting.iter().map(|s| s.id.clone()).collect();
        for c in &opened {
            set_in(&mut settings.collapsed, c, false);
        }
        !opened.is_empty()
    }
}

/// 승인 대기·작업 중 세션이 없고 마지막 활동이 30분 넘게 지난 카드를 한 번 접는다.
/// 그때의 마지막 활동 시각을 적어 두어, 사람이 다시 펴면 새 활동이 생기기 전까지 또 접지 않는다.
fn fold_quiet(groups: &[Group], settings: &mut Settings, now: f64) -> bool {
    let mut changed = false;
    for g in groups {
        let last = g.last_updated();
        let quiet = !g.sessions.iter().any(|s| s.state == "waiting" || s.state == "working");
        if !quiet || now - last <= QUIET_FOLD_AFTER {
            continue;
        }
        if settings.auto_folded.get(&g.id) == Some(&last) {
            continue;
        }
        settings.auto_folded.insert(g.id.clone(), last);
        set_in(&mut settings.collapsed, &g.id, true);
        changed = true;
    }
    let ids: HashSet<&str> = groups.iter().map(|g| g.id.as_str()).collect();
    let before = settings.auto_folded.len();
    settings.auto_folded.retain(|k, _| ids.contains(k.as_str()));
    changed || settings.auto_folded.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    struct Fake {
        root: PathBuf,
        sessions: PathBuf,
        claude: PathBuf,
    }

    impl Fake {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cb-store-{}", crate::util::short_id()));
            let sessions = root.join("notifier").join("sessions");
            let claude = root.join(".claude");
            fs::create_dir_all(&sessions).unwrap();
            fs::create_dir_all(&claude).unwrap();
            Fake { root, sessions, claude }
        }

        fn store(&self) -> Store {
            Store::new(self.sessions.clone(), &self.root.join("notifier"), self.claude.clone(), self.root.to_string_lossy().into_owned())
        }

        /// 세션 기록과 (있으면) 대화 기록을 쓴다.
        fn session(&self, sid: &str, mut rec: serde_json::Value, transcript: bool) -> PathBuf {
            let t = self.root.join("t").join(format!("{sid}.jsonl"));
            rec["session_id"] = json!(sid);
            rec["transcript_path"] = json!(t.to_string_lossy());
            if transcript {
                fs::create_dir_all(t.parent().unwrap()).unwrap();
                fs::write(&t, r#"{"type":"assistant","effort":"high","message":{"model":"claude-opus-5-5","usage":{"input_tokens":100}}}"#).unwrap();
            }
            let p = self.sessions.join(format!("{sid}.json"));
            fs::write(&p, serde_json::to_vec(&rec).unwrap()).unwrap();
            p
        }
    }

    impl Fake {
        /// Claude Code 의 `sessions\<pid>.json` 을 쓴다.
        fn claude_session(&self, pid: u32, body: serde_json::Value) -> PathBuf {
            let d = self.claude.join("sessions");
            fs::create_dir_all(&d).unwrap();
            let p = d.join(format!("{pid}.json"));
            fs::write(&p, serde_json::to_vec(&body).unwrap()).unwrap();
            p
        }

        fn transcript(&self, sid: &str) -> PathBuf {
            self.root.join("t").join(format!("{sid}.jsonl"))
        }

        /// 대화 기록 끝에 Esc 중단 표시 줄을 붙인다.
        fn interrupt(&self, sid: &str) {
            let line = json!({"type": "user", "timestamp": STAMP, "message": {"content": "[Request interrupted by user]"}});
            let mut f = fs::OpenOptions::new().append(true).open(self.transcript(sid)).unwrap();
            writeln!(f).unwrap();
            writeln!(f, "{line}").unwrap();
        }
    }

    /// Windows 는 수정 시각이 타이머 틱 단위라 방금 쓴 파일과 같은 값이 될 수 있다 — 바뀐 것으로 보이게 확실히 옮긴다.
    fn bump(path: &Path, secs: u64) {
        let f = fs::OpenOptions::new().append(true).open(path).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(secs)).unwrap();
    }

    /// 2026-10-16T19:24:08.5Z — 중단 표시 줄의 시각
    const STAMP: &str = "2026-10-16T19:24:08.5Z";
    const STAMP_S: f64 = 1_792_178_648.5;
    const LIVE_NOW: f64 = STAMP_S + 60.0;

    fn session_of<'a>(groups: &'a [Group], id: &str) -> &'a Session {
        groups.iter().flat_map(|g| g.sessions.iter()).find(|s| s.id == id).unwrap()
    }

    #[test]
    fn interrupt_turns_working_into_idle() {
        let f = Fake::new();
        for (sid, state, pid) in [("w", "working", 10), ("q", "waiting", 11), ("i", "idle", 12)] {
            f.session(sid, json!({"state": state, "pid": pid, "cwd": "/w/a", "updated": LIVE_NOW, "since": LIVE_NOW - 100.0}), true);
            f.interrupt(sid);
        }
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, LIVE_NOW);
        // working·waiting 은 대기로, 끊은 시각이 since. idle 은 그대로
        for sid in ["w", "q"] {
            let x = session_of(&r.groups, sid);
            assert_eq!((x.state.as_str(), x.since), ("idle", ms(STAMP_S)), "{sid}");
        }
        assert_eq!(session_of(&r.groups, "i").since, ms(LIVE_NOW - 100.0));
    }

    #[test]
    fn claude_status_overrides_by_session_id() {
        let f = Fake::new();
        let at = |s: f64| json!((s * 1000.0) as i64);
        let rec = |state: &str, pid: u32| json!({"state": state, "pid": pid, "cwd": "/w", "updated": LIVE_NOW, "since": LIVE_NOW - 50.0});
        // 승인 뒤 도구가 도는 중 — waiting 인데 Claude Code 는 busy
        f.session("a", rec("waiting", 10), true);
        f.claude_session(10, json!({"sessionId": "a", "status": "busy", "statusUpdatedAt": at(LIVE_NOW - 1.0)}));
        // 첫 응답 전에 끊음 — 대화 기록에 표시가 없어도 status 가 idle
        f.session("b", rec("working", 11), true);
        f.claude_session(11, json!({"sessionId": "b", "status": "idle", "statusUpdatedAt": at(LIVE_NOW - 2.0)}));
        // 다른 세션의 파일이거나(/clear 뒤 새 id) 이미 반영된 시각이면 기록 그대로
        f.session("c", rec("waiting", 12), true);
        f.claude_session(12, json!({"sessionId": "other", "status": "busy", "statusUpdatedAt": at(LIVE_NOW - 1.0)}));
        f.session("d", rec("waiting", 13), true);
        f.claude_session(13, json!({"sessionId": "d", "status": "busy", "statusUpdatedAt": at(LIVE_NOW - 60.0)}));
        // working 인데 busy 는 덮을 것이 없다
        f.session("e", rec("working", 14), true);
        f.claude_session(14, json!({"sessionId": "e", "status": "busy", "statusUpdatedAt": at(LIVE_NOW - 1.0)}));
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, LIVE_NOW);
        let get = |id: &str| {
            let x = session_of(&r.groups, id);
            (x.state.clone(), x.since)
        };
        assert_eq!(get("a"), ("working".to_string(), ms(LIVE_NOW - 1.0)));
        assert_eq!(get("b"), ("idle".to_string(), ms(LIVE_NOW - 2.0)));
        assert_eq!(get("c"), ("waiting".to_string(), ms(LIVE_NOW - 50.0)));
        assert_eq!(get("d"), ("waiting".to_string(), ms(LIVE_NOW - 50.0)));
        assert_eq!(get("e"), ("working".to_string(), ms(LIVE_NOW - 50.0)));
    }

    #[test]
    fn running_subagent_and_paused_after_interrupt() {
        let f = Fake::new();
        let run = json!({"r": {"type": "general", "state": "running", "started": LIVE_NOW - 30.0, "started_seen": true}});
        f.session("sub", json!({"state": "working", "pid": 10, "cwd": "/w", "updated": LIVE_NOW, "since": LIVE_NOW - 100.0, "subagents": run}), true);
        f.session("pz", json!({"state": "working", "pid": 11, "cwd": "/w", "updated": LIVE_NOW, "since": LIVE_NOW - 100.0}), true);
        f.interrupt("sub");
        f.interrupt("pz");
        fs::write(f.sessions.join("pz.paused"), b"{}").unwrap();
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, LIVE_NOW);
        // 끊은 뒤에도 서브에이전트가 돌면 작업 중, since 는 끊은 시각
        let x = session_of(&r.groups, "sub");
        assert_eq!((x.state.as_str(), x.since), ("working", ms(STAMP_S)));
        // 일시중지 표지가 덮은 결과보다 우선한다
        assert_eq!(session_of(&r.groups, "pz").state, "paused");
    }

    #[test]
    fn name_label_hidden_when_ended_and_cache_follows_mtime() {
        let f = Fake::new();
        f.session("n", json!({"state": "idle", "pid": 10, "cwd": "/w", "updated": NOW, "since": NOW}), true);
        let p = f.claude_session(10, json!({"sessionId": "n", "name": "빌드-1a"}));
        f.session("x", json!({"state": "idle", "pid": 666, "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.claude_session(666, json!({"sessionId": "x", "name": "죽은-세션"}));
        f.session("e", json!({"state": "idle", "pid": 12, "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.claude_session(12, json!({"sessionId": "e", "name": ""}));
        f.session("z", json!({"state": "idle", "cwd": "/w", "updated": NOW, "since": NOW}), true);
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &dead_pid, false, NOW);
        assert_eq!(session_of(&r.groups, "n").name.as_deref(), Some("빌드-1a"));
        // 종료된 세션·빈 이름·pid 없음은 이름이 없다
        assert_eq!(session_of(&r.groups, "x").state, "ended");
        for sid in ["x", "e", "z"] {
            assert_eq!(session_of(&r.groups, sid).name, None, "{sid}");
        }
        // 파일이 바뀌면 다시 읽는다
        fs::write(&p, r#"{"sessionId":"n","name":"새-이름"}"#).unwrap();
        bump(&p, 2);
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 1.0);
        assert_eq!(session_of(&r.groups, "n").name.as_deref(), Some("새-이름"));
        // 파일이 사라지면 이름도 사라진다
        fs::remove_file(&p).unwrap();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 2.0);
        assert_eq!(session_of(&r.groups, "n").name, None);
    }

    #[test]
    fn interrupt_cache_follows_transcript_mtime_and_prunes() {
        let f = Fake::new();
        f.session("w", json!({"state": "working", "pid": 10, "cwd": "/w", "updated": LIVE_NOW, "since": LIVE_NOW - 100.0}), true);
        f.claude_session(10, json!({"sessionId": "w", "status": "busy", "statusUpdatedAt": 1000}));
        let t = f.transcript("w");
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, LIVE_NOW);
        assert_eq!(session_of(&r.groups, "w").state, "working");
        // 대화 기록이 바뀌면(수정 시각이 달라지면) 다시 읽는다
        f.interrupt("w");
        bump(&t, 2);
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, LIVE_NOW + 1.0);
        assert_eq!(session_of(&r.groups, "w").state, "idle");
        // 수정 시각과 크기가 같으면 파일을 다시 읽지 않는다 — 같은 길이의 다른 내용으로 바꿔도 같은 값을 돌려준다
        let md = fs::metadata(&t).unwrap();
        let mtime = md.modified().unwrap();
        fs::write(&t, " ".repeat(md.len() as usize)).unwrap();
        fs::File::options().append(true).open(&t).unwrap().set_modified(mtime).unwrap();
        assert_eq!(st.interrupts.read(&t), Some(STAMP_S));
        // 수정 시각이 그대로여도 크기가 달라지면 다시 읽는다(Windows 는 같은 틱 안의 쓰기가 같은 시각으로 보인다)
        fs::write(&t, "").unwrap();
        fs::File::options().append(true).open(&t).unwrap().set_modified(mtime).unwrap();
        assert_eq!(st.interrupts.read(&t), None);
        // 목록에서 사라진 세션의 캐시는 버린다
        assert_eq!((st.interrupts.len(), st.names.len()), (1, 1));
        fs::remove_file(f.sessions.join("w.json")).unwrap();
        st.reload(&mut s, &Probe::default(), &|_| true, false, LIVE_NOW + 3.0);
        assert_eq!((st.interrupts.len(), st.names.len()), (0, 0));
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    const NOW: f64 = 1_792_000_000.0;

    fn dead_pid(rec: &Record) -> bool {
        rec.get("pid").and_then(Value::as_u64) != Some(666)
    }

    /// 771·772·773·888 은 죽은 것으로 본다(나머지와 pid 없음은 산 것).
    /// pid 없는 기록은 Claude Code 세션 파일에서 찾은 pid 로 판정되므로 이 pid 들이 판정에 걸린다.
    fn dead_lookup_pids(rec: &Record) -> bool {
        !matches!(rec.get("pid").and_then(Value::as_u64), Some(771) | Some(772) | Some(773) | Some(888))
    }

    fn all(groups: &[Group]) -> Vec<(&str, &str)> {
        groups.iter().flat_map(|g| g.sessions.iter().map(|s| (s.id.as_str(), s.state.as_str()))).collect()
    }

    #[test]
    fn ended_detection_and_pruning() {
        let f = Fake::new();
        let cwd = f.root.join("work").join("a").to_string_lossy().into_owned();
        // pid 가 죽었으면 ended — 대화 기록이 있으면 남는다
        f.session("dead", json!({"state": "working", "pid": 666, "cwd": cwd, "updated": NOW - 5.0, "since": NOW - 5.0}), true);
        // 대화 기록이 없는 종료 세션은 지운다
        let gone = f.session("notranscript", json!({"state": "ended", "cwd": cwd, "updated": NOW - 5.0}), false);
        // 2일 넘은 종료 세션도 지운다
        let old = f.session("old", json!({"state": "ended", "cwd": cwd, "updated": NOW - 3.0 * 86400.0}), true);
        // 대화 기록이 없어도 살아 있는 세션은 남는다
        f.session("live", json!({"state": "idle", "pid": 1, "cwd": cwd, "updated": NOW - 1.0, "since": NOW - 1.0, "topic": "## **빌드** `고치기`  "}), false);
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &dead_pid, false, NOW);
        assert_eq!(all(&r.groups), [("live", "idle"), ("dead", "ended")]);
        assert!(!gone.exists() && !old.exists());
        let live = &r.groups[0].sessions[0];
        assert_eq!(live.title, "빌드 고치기");
        assert_eq!(live.resume_args, ["--resume", "live"]);
        assert_eq!(live.brain, None); // 대화 기록도 model 도 없다
        let dead = &r.groups[0].sessions[1];
        assert_eq!(dead.brain, None); // 종료 세션은 모델을 읽지 않는다
        assert_eq!(dead.ctx.map(|c| c.used), Some(100)); // 컨텍스트는 읽는다
        assert_eq!(r.groups[0].name, "a");
        assert_eq!(r.groups[0].path, format!("~{}work{}a", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn subagents_shells_paused_muted() {
        let f = Fake::new();
        let cwd = "/w/proj";
        f.session(
            "s1",
            json!({"state": "idle", "pid": 10, "cwd": cwd, "updated": NOW, "since": NOW, "subagents": {
                "old": {"type": "Explore", "state": "done", "started": NOW - 100.0, "ended": NOW - 11.0, "started_seen": true},
                "fresh": {"type": "Plan", "state": "done", "started": NOW - 50.0, "ended": NOW - 3.0, "started_seen": true},
                "run": {"type": "general", "description": "찾기", "state": "running", "started": NOW - 60.0, "tool": "Grep", "background": true, "started_seen": true}
            }, "resume_args": ["--model", "claude-opus-5-5", "--resume", "s1"], "limit_hit": {"at": NOW - 30.0, "message": "x"}}),
            true,
        );
        f.session("s2", json!({"state": "idle", "pid": 20, "cwd": cwd, "updated": NOW, "since": NOW - 10.0}), true);
        f.session("s3", json!({"state": "working", "pid": 30, "cwd": cwd, "updated": NOW, "since": NOW - 20.0}), true);
        fs::write(f.sessions.join("s3.paused"), b"{}").unwrap();
        fs::write(f.sessions.join("s2.mute"), b"").unwrap();
        let probe = Probe { shells: HashMap::from([(20, 2), (30, 5)]), drivable: HashSet::from([10, 30]), ..Probe::default() };
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &probe, &|_| true, true, NOW);
        assert!(r.all_muted);
        let g = &r.groups[0];
        let by = |id: &str| g.sessions.iter().find(|x| x.id == id).unwrap();
        // 끝나지 않은 서브에이전트가 있으면 작업 중, 끝난 지 10초 넘은 것은 뺀다, done 이 아닌 것 먼저
        let s1 = by("s1");
        assert_eq!(s1.state, "working");
        assert_eq!(s1.subagents.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["run", "fresh"]);
        assert!(s1.subagents[0].background && s1.subagents[0].tool == "Grep" && s1.subagents[0].desc == "찾기");
        assert_eq!(s1.resume_args, ["--model", "claude-opus-5-5", "--resume", "s1"]);
        // 보드(프런트엔드)에도 간다 — 탭을 되살릴 때 원래 플래그를 살리는 데 쓴다
        assert_eq!(serde_json::to_value(s1).unwrap()["resumeArgs"], serde_json::json!(["--model", "claude-opus-5-5", "--resume", "s1"]));
        assert_eq!(s1.limit_at, Some(ms(NOW - 30.0)));
        assert!(s1.can_drive);
        assert_eq!(s1.brain.as_ref().map(|b| b.effort.as_str()), Some("high"));
        // idle 인데 셸이 돌면 작업 중, 셸 수는 idle 일 때만
        let s2 = by("s2");
        assert_eq!((s2.state.as_str(), s2.shells, s2.muted, s2.can_drive), ("working", 2, true, false));
        let s3 = by("s3");
        assert_eq!((s3.state.as_str(), s3.shells, s3.paused), ("paused", 0, true));
    }

    #[test]
    fn subagents_without_start_record_are_hidden() {
        let f = Fake::new();
        // 시작 hook 을 본 기록(started_seen 참)은 끝났든 아니든 보이고, 표지가 없거나 거짓이면 끝났든 아니든 숨긴다
        let subs = json!({
            "seen_run": {"type": "Plan", "state": "running", "started": NOW - 40.0, "started_seen": true},
            "seen_done": {"type": "Plan", "state": "done", "started": NOW - 50.0, "ended": NOW - 3.0, "started_seen": true},
            "orphan_run": {"type": "", "state": "running", "started": NOW - 5.0, "started_seen": false},
            "orphan_done": {"type": "", "state": "done", "started": NOW - 2.0, "ended": NOW - 2.0, "started_seen": false},
            "legacy_run": {"type": "Plan", "state": "running", "started": NOW - 60.0},
            "legacy_done": {"type": "Plan", "state": "done", "started": NOW - 60.0, "ended": NOW - 3.0},
            "wrong_type": {"type": "Plan", "state": "running", "started": NOW - 60.0, "started_seen": "yes"}
        });
        f.session("a", json!({"state": "idle", "pid": 10, "cwd": "/w", "updated": NOW, "since": NOW, "subagents": subs}), true);
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW);
        let a = session_of(&r.groups, "a");
        assert_eq!(a.subagents.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["seen_run", "seen_done"]);
        assert_eq!(a.state, "working");
    }

    #[test]
    fn subagents_without_start_record_do_not_raise_state() {
        let f = Fake::new();
        let orphan = |state: &str| json!({"x": {"type": "", "state": state, "started": NOW - 5.0, "started_seen": false}});
        // 끝 hook 이 끝내 안 오는 시작 없는 기록이 끝나지 않은 채 남아도 응답이 끝난 세션은 쉬는 상태다
        f.session("run", json!({"state": "idle", "pid": 10, "cwd": "/w", "updated": NOW, "since": NOW, "subagents": orphan("running")}), true);
        f.session("wait", json!({"state": "idle", "pid": 11, "cwd": "/w", "updated": NOW, "since": NOW, "subagents": orphan("waiting")}), true);
        // 표지 없는 예전 기록도 같다
        f.session("old", json!({"state": "idle", "pid": 12, "cwd": "/w", "updated": NOW, "since": NOW,
                                "subagents": {"x": {"type": "", "state": "running", "started": NOW - 5.0}}}), true);
        // 시작을 본 서브에이전트는 그대로 작업 중으로 올린다
        f.session("seen", json!({"state": "idle", "pid": 13, "cwd": "/w", "updated": NOW, "since": NOW,
                                 "subagents": {"x": {"type": "", "state": "running", "started": NOW - 5.0, "started_seen": true}}}), true);
        // 시작 없는 기록은 보이는 서브에이전트를 가리지 않는다
        f.session("mixed", json!({"state": "idle", "pid": 14, "cwd": "/w", "updated": NOW, "since": NOW, "subagents": {
            "o": {"type": "", "state": "running", "started": NOW - 5.0, "started_seen": false},
            "s": {"type": "", "state": "done", "started": NOW - 9.0, "ended": NOW - 1.0, "started_seen": true}}}), true);
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW);
        for sid in ["run", "wait", "old", "mixed"] {
            let x = session_of(&r.groups, sid);
            assert_eq!(x.state, "idle", "{sid}");
        }
        assert!(session_of(&r.groups, "run").subagents.is_empty());
        assert_eq!(session_of(&r.groups, "mixed").subagents.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["s"]);
        assert_eq!(session_of(&r.groups, "seen").state, "working");
    }

    #[test]
    fn sorting_pins_hidden_and_folding() {
        let f = Fake::new();
        f.session("a1", json!({"state": "idle", "cwd": "/w/a", "updated": NOW - 10.0, "since": NOW - 10.0}), true);
        f.session("a2", json!({"state": "waiting", "cwd": "/w/a", "updated": NOW - 20.0, "since": NOW - 20.0}), true);
        f.session("a3", json!({"state": "idle", "cwd": "/w/a", "updated": NOW - 5.0, "since": NOW - 5.0}), true);
        f.session("b1", json!({"state": "working", "cwd": "/w/b", "updated": NOW - 1.0, "since": NOW - 1.0}), true);
        f.session("c1", json!({"state": "idle", "cwd": "/w/c", "updated": NOW - 3600.0, "since": NOW - 3600.0}), true);
        f.session("d1", json!({"state": "idle", "cwd": "/w/d", "updated": NOW - 100.0, "since": NOW - 100.0}), true);
        let mut st = f.store();
        let mut s = Settings { collapsed: vec!["/w/a".into()], hidden_groups: [("/w/d".to_string(), NOW - 50.0)].into(), ..Settings::default() };
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW);
        // 그룹: 가장 급한 상태 → 최근 활동. d 는 숨김(지운 뒤 활동 없음)
        assert_eq!(r.groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["/w/a", "/w/b", "/w/c"]);
        // 그룹 안: 급한 순 → since 최신 먼저
        assert_eq!(r.groups[0].sessions.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a2", "a3", "a1"]);
        // 새로 승인 대기가 된 그룹은 편다, 30분 넘게 조용한 c 는 한 번 접는다
        assert_eq!(s.collapsed, ["/w/c"]);
        assert_eq!(s.auto_folded.get("/w/c"), Some(&(NOW - 3600.0)));
        assert!(r.settings_changed);
        assert_eq!(crate::model::Counts::of(&r.groups).waiting, 1);

        // 사람이 다시 펴면 새 활동 전에는 또 접지 않는다. 같은 승인 대기로는 다시 펴지 않는다
        s.collapsed = vec!["/w/a".into()];
        s.pinned_groups = vec!["/w/c".into()];
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 1.0);
        assert_eq!(s.collapsed, ["/w/a"]);
        assert!(!r.settings_changed);
        // 고정한 그룹이 맨 위
        assert_eq!(r.groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["/w/c", "/w/a", "/w/b"]);

        // 숨긴 그룹에 새 활동이 생기면 다시 보이고 숨김 목록에서 빠진다
        f.session("d2", json!({"state": "idle", "cwd": "/w/d", "updated": NOW + 2.0, "since": NOW + 2.0}), true);
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 3.0);
        assert!(r.groups.iter().any(|g| g.id == "/w/d"));
        assert!(s.hidden_groups.is_empty());
        assert!(r.settings_changed);

        // 사라진 그룹의 자동 접기 기록은 지운다
        fs::remove_file(f.sessions.join("c1.json")).unwrap();
        st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 4.0);
        assert!(!s.auto_folded.contains_key("/w/c"));
    }

    #[test]
    fn groups_by_project_dir() {
        let f = Fake::new();
        // 워크트리에 들어가 cwd 가 바뀐 세션도 claude 를 띄운 폴더의 카드에, project_dir 이 없는 예전 기록은 cwd 의 카드에
        f.session("p1", json!({"state": "idle", "cwd": "/w/repo", "project_dir": "/w/repo", "updated": NOW - 10.0, "since": NOW - 10.0}), true);
        f.session("p2", json!({"state": "idle", "cwd": "/w/repo/.claude/worktrees/t1", "project_dir": "/w/repo", "updated": NOW - 5.0, "since": NOW - 5.0}), true);
        f.session("p3", json!({"state": "idle", "cwd": "/w/repo", "updated": NOW - 1.0, "since": NOW - 1.0}), true);
        let mut st = f.store();
        let mut s = Settings { collapsed: vec!["/w/repo".into()], ..Settings::default() };
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW);
        assert_eq!(r.groups.iter().map(|g| (g.id.as_str(), g.name.as_str(), g.sessions.len())).collect::<Vec<_>>(), [("/w/repo", "repo", 3)]);
        // 세션 하나의 cwd 는 그대로
        let p2 = r.groups[0].sessions.iter().find(|x| x.id == "p2").unwrap();
        assert_eq!((p2.cwd.as_str(), p2.project_dir.as_str()), ("/w/repo/.claude/worktrees/t1", "/w/repo"));
        assert_eq!(s.collapsed, ["/w/repo"]);
        // 워크트리 안 세션이 새로 승인 대기가 되면 접힌 프로젝트 카드를 편다
        f.session("p4", json!({"state": "waiting", "cwd": "/w/repo/.claude/worktrees/t2", "project_dir": "/w/repo", "updated": NOW + 1.0, "since": NOW + 1.0}), true);
        let r = st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 2.0);
        assert!(s.collapsed.is_empty());
        assert!(r.settings_changed);
    }

    #[test]
    fn transcript_and_display_paths() {
        let c = Path::new("/h/.claude");
        assert_eq!(transcript_path(None, "/Users/me/my proj", "id", c), c.join("projects").join("-Users-me-my-proj").join("id.jsonl"));
        assert_eq!(transcript_path(None, r"C:\work\a.b", "id", c), c.join("projects").join("C--work-a-b").join("id.jsonl"));
        assert_eq!(transcript_path(Some("/t/x.jsonl"), "/w", "id", c), PathBuf::from("/t/x.jsonl"));
        assert_eq!(transcript_path(Some(""), "/w", "id", c), c.join("projects").join("-w").join("id.jsonl"));
        assert_eq!(display_path("/Users/me/work", "/Users/me"), "~/work");
        assert_eq!(display_path("/Users/me", "/Users/me"), "~");
        assert_eq!(display_path("/Users/meow/x", "/Users/me"), "/Users/meow/x");
        assert_eq!(display_path(r"C:\Users\demo\work", r"C:\Users\demo"), r"~\work");
    }

    #[test]
    fn pidless_judged_by_claude_session_file() {
        let f = Fake::new();
        // pid 없는 기록(상태 세 가지)은 Claude Code 세션 파일의 sessionId 로 pid 를 찾아 생사를 본다
        for (sid, state, pid) in [("w", "working", 771u32), ("q", "waiting", 772), ("i", "idle", 773)] {
            f.session(sid, json!({"state": state, "cwd": "/w", "updated": STAMP_S, "since": STAMP_S}), true);
            f.claude_session(pid, json!({"sessionId": sid}));
        }
        // 찾은 pid 가 죽었고 대화 기록이 없는 기록은 종료 세션이라 기록 파일도 지운다
        let gone = f.session("nt", json!({"state": "working", "cwd": "/w", "updated": STAMP_S, "since": STAMP_S}), false);
        f.claude_session(888, json!({"sessionId": "nt"}));
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &dead_lookup_pids, false, LIVE_NOW);
        for sid in ["w", "q", "i"] {
            assert_eq!(session_of(&r.groups, sid).state, "ended", "{sid}");
        }
        // 지운 종료 세션은 목록에도 기록 파일도 없다
        assert!(r.groups.iter().flat_map(|g| g.sessions.iter()).all(|x| x.id != "nt"));
        assert!(!gone.exists());
    }

    #[test]
    fn pidless_alive_or_missing_file_unchanged() {
        let f = Fake::new();
        // 찾은 pid 가 살아 있으면 기록 그대로, Claude Code 세션 파일이 없으면 이전처럼 산 것으로 본다
        f.session("a", json!({"state": "waiting", "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.claude_session(55, json!({"sessionId": "a"}));
        f.session("b", json!({"state": "idle", "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.session("c", json!({"state": "working", "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.claude_session(56, json!({"sessionId": "c"}));
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &dead_lookup_pids, false, NOW);
        for (sid, state) in [("a", "waiting"), ("b", "idle"), ("c", "working")] {
            assert_eq!(session_of(&r.groups, sid).state, state, "{sid}");
        }
    }

    #[test]
    fn pidless_status_and_name_use_found_pid() {
        let f = Fake::new();
        let at = |v: f64| json!((v * 1000.0) as i64);
        f.session("s", json!({"state": "waiting", "cwd": "/w", "updated": LIVE_NOW, "since": LIVE_NOW - 50.0}), true);
        f.claude_session(55, json!({"sessionId": "s", "name": "발견-세션", "status": "busy", "statusUpdatedAt": at(LIVE_NOW - 1.0)}));
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &dead_lookup_pids, false, LIVE_NOW);
        let x = session_of(&r.groups, "s");
        // waiting 기록에 찾은 pid 의 busy status 가 겹쳐 작업 중이 되고, since 는 status 가 바뀐 시각
        assert_eq!((x.state.as_str(), x.since), ("working", ms(LIVE_NOW - 1.0)));
        // 이름도 찾은 pid 의 Claude Code 세션 파일에서 읽는다
        assert_eq!(x.name.as_deref(), Some("발견-세션"));
        // 찾은 pid 는 이름 캐시 정리(정리할 pid 목록)에서 남는다 — 기록 pid 0 말고 찾은 pid 55
        assert_eq!(st.names.len(), 1);
    }

    #[test]
    fn ended_record_does_not_look_up_pid() {
        let f = Fake::new();
        // 끝난 기록은 pid 를 찾지 않는다 — Claude Code 세션 파일이 있어도 이름·status 를 겹치지 않는다
        f.session("e", json!({"state": "ended", "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.claude_session(99, json!({"sessionId": "e", "name": "끝남-이름", "status": "busy", "statusUpdatedAt": 1000}));
        let mut st = f.store();
        let mut s = Settings::default();
        let r = st.reload(&mut s, &Probe::default(), &|_| false, false, NOW);
        assert_eq!(all(&r.groups), [("e", "ended")]);
        assert_eq!(session_of(&r.groups, "e").name, None);
    }

    #[test]
    fn record_with_pid_does_not_look_up_pid() {
        let f = Fake::new();
        // pid 가 있는 기록은 찾지 않는다 — Claude Code 파일이 다른 pid 에 있어도 기록의 pid 로 생사와 이름을 본다
        f.session("r", json!({"state": "working", "pid": 42, "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.claude_session(43, json!({"sessionId": "r", "name": "잘못-이름", "status": "busy", "statusUpdatedAt": 1000}));
        let mut st = f.store();
        let mut s = Settings::default();
        // 기록 pid 42 는 산 것, 찾으면 나올 43 은 죽은 것으로 본다
        let r = st.reload(&mut s, &Probe::default(), &|rec| rec.get("pid").and_then(Value::as_u64) != Some(43), false, NOW);
        let x = session_of(&r.groups, "r");
        assert_eq!(x.state, "working");
        assert_eq!(x.name, None);
    }

    #[test]
    fn context_cache_pruned_to_listed_sessions() {
        let f = Fake::new();
        // 끝난 세션도 목록에 남아 있는 동안에는 컨텍스트 캐시를 남긴다
        f.session("a", json!({"state": "ended", "cwd": "/w", "updated": NOW}), true);
        f.session("b", json!({"state": "idle", "pid": 20, "cwd": "/w", "updated": NOW, "since": NOW}), true);
        f.session("c", json!({"state": "idle", "pid": 30, "cwd": "/w", "updated": NOW, "since": NOW}), true);
        let ctx_dir = f.root.join("notifier").join("context");
        fs::create_dir_all(&ctx_dir).unwrap();
        for sid in ["a", "b", "c"] {
            fs::write(ctx_dir.join(format!("{sid}.json")), format!(r#"{{"at":{NOW}}}"#)).unwrap();
        }
        let mut st = f.store();
        let mut s = Settings::default();
        st.reload(&mut s, &Probe::default(), &|_| true, false, NOW);
        assert_eq!(st.contexts.cache_counts(), (3, 3));
        // 목록에서 사라진 세션의 컨텍스트 캐시는 버린다 — 끝난 세션 몫이 쌓이지 않게
        fs::remove_file(f.sessions.join("c.json")).unwrap();
        st.reload(&mut s, &Probe::default(), &|_| true, false, NOW + 1.0);
        assert_eq!(st.contexts.cache_counts(), (2, 2));
    }
}
