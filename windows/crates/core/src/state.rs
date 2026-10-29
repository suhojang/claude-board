//! hook 상태 기록 — macOS `state.py` 이식 (SPEC §3.2–3.5).
//!
//! [`apply`] 는 순수 함수다: 세션 기록 + 상태 인자 + hook 입력 + 시각 + 주입한 프로세스 정보([`Env`])
//! → 새 기록 + 바꿀 터미널 제목. 파일 잠금·읽기·쓰기·prune·`.paused` 삭제는 [`run`] 이 맡는다.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::text::{collapse_ws, non_empty_str, take_chars, truthy};
use crate::{fileio, paths, proc, quote, Record};

/// 상태별 터미널 제목 아이콘.
pub fn icon(state: &str) -> &'static str {
    match state {
        "working" => "🔵",
        "waiting" => "🟠",
        "idle" => "🟢",
        "ended" => "⚫",
        _ => "",
    }
}

/// hook 의 조상에서 찾은 Claude Code 프로세스.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeProcess {
    pub pid: u32,
    /// 프로세스 생성 시각(유닉스 초) — Windows 에서 pid 재사용을 가려낸다. macOS 는 None.
    pub ctime: Option<f64>,
    /// 터미널 장치 이름 — macOS 만. Windows 는 늘 None.
    pub tty: Option<String>,
    pub headless: bool,
    /// 실행 인자 전체(argv[0] 포함). 읽지 못하면 빈 목록.
    pub argv: Vec<String>,
}

/// [`apply`] 가 바깥 세계에서 얻는 것들 — 시험에서는 가짜로 바꾼다.
pub trait Env {
    /// hook 프로세스의 조상 가운데 Claude Code 프로세스.
    fn find_claude(&self) -> Option<ClaudeProcess>;
    /// 그 pid 가 아직 살아 있는지(`ctime` 이 있으면 생성 시각도 같아야 한다).
    fn alive(&self, pid: u32, ctime: Option<f64>) -> bool;
    /// 그 프로세스의 실행 인자 전체(argv[0] 포함).
    fn argv(&self, pid: u32) -> Vec<String>;
    /// `<transcript 에서 .jsonl 을 뺀 경로>/subagents/agent-<id>.meta.json` 내용(없으면 빈 객체).
    fn subagent_meta(&self, transcript: &str, agent_id: &str) -> Record;
    /// hook 입력과 기록에 cwd 가 없을 때 쓸 현재 폴더.
    fn cwd(&self) -> String;
    /// Claude Code 가 hook 에 넘기는 환경 변수 `CLAUDE_PROJECT_DIR`(claude 를 띄운 폴더). 없으면 None.
    fn project_dir(&self) -> Option<String>;
}

/// [`apply`] 의 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub record: Record,
    /// 바꿀 터미널 제목(`"<아이콘> <project> · <topic 앞 20자>…"`). 상태나 topic 이 바뀌었고 터미널이 있을 때만.
    pub title: Option<String>,
    /// `UserPromptSubmit` — 보드의 일시중지 표지(`<sid>.paused`)를 지운다.
    pub remove_paused: bool,
}

// 세션을 다시 열 때 그대로 넘길 실행 인자 — 값을 받는 것, 값을 여러 개 받는 것, 값이 없는 것
const CARRY_ONE: &[&str] = &["--model", "--permission-mode", "--agent", "--settings", "--plugin-dir"];
const CARRY_MANY: &[&str] = &["--add-dir", "--mcp-config"];
const CARRY_FLAG: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "--chrome",
    "--ide",
    "--verbose",
];

/// 다시 열 때 넘길 실행 인자만 고른다(argv[0] 은 건너뛴다) — `state.py:carry_args`.
/// 같은 플래그·같은 `(이름, 값)` 쌍·같은 `--x=v` 형은 한 번만 넘긴다. Claude Code 는 bypass 권한 모드로 이어 열 때
/// `--allow-dangerously-skip-permissions` 를 스스로 하나 더 붙여 띄우므로, 그대로 넘기면 이어 열 때마다 하나씩 늘어난다.
/// 값을 여러 개 받는 `--add-dir x` 꼴은 중복을 걸러내지 않는다.
pub fn carry_args<S: AsRef<str>>(argv: &[S]) -> Vec<String> {
    let argv: Vec<&str> = argv.iter().map(AsRef::as_ref).collect();
    let mut out = Vec::new();
    let mut seen: HashSet<(&str, Option<&str>)> = HashSet::new();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i];
        let name = a.split('=').next().unwrap_or(a);
        if (a.contains('=') && (CARRY_ONE.contains(&name) || CARRY_MANY.contains(&name))) || CARRY_FLAG.contains(&a) {
            if seen.insert((a, None)) {
                out.push(a.to_string());
            }
        } else if CARRY_ONE.contains(&a) && i + 1 < argv.len() {
            if seen.insert((a, Some(argv[i + 1]))) {
                out.push(a.to_string());
                out.push(argv[i + 1].to_string());
            }
            i += 1;
        } else if CARRY_MANY.contains(&a) {
            out.push(a.to_string());
            while i + 1 < argv.len() && !argv[i + 1].starts_with('-') {
                out.push(argv[i + 1].to_string());
                i += 1;
            }
        }
        i += 1;
    }
    out
}

/// 종료된 세션을 다시 열 때 `claude` 뒤에 붙일 인자 배열 — 같은 인자에 마지막 권한 모드·모델을 다시 붙이고
/// `--resume <sid>` 로 끝난다(`state.py:resume_cmd` 와 같은 규칙). 보드는 이 배열로 claude 를 셸 없이 직접 실행한다 —
/// 모델 이름 같은 값은 저장소의 설정 파일에서 올 수 있어 셸 문자열로 실행하면 명령 주입이 된다.
pub fn resume_args(rec: &Record) -> Vec<String> {
    let mut args: Vec<String> = rec
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    for (flag, key) in [("--permission-mode", "permission_mode"), ("--model", "model")] {
        let Some(value) = non_empty_str(rec.get(key)) else { continue };
        let with_eq = format!("{flag}=");
        if let Some(j) = args.iter().position(|a| a == flag || a.starts_with(&with_eq)) {
            let n = if args[j].contains('=') { 1 } else { 2 };
            args.drain(j..(j + n).min(args.len()));
        }
        args.push(flag.to_string());
        args.push(value.to_string());
    }
    let sid = rec.get("session_id").and_then(Value::as_str).unwrap_or_default();
    args.push("--resume".to_string());
    args.push(sid.to_string());
    args
}

/// 표시용 다시 열기 명령 `claude <resume_args…>` — `join` 으로 인용 규칙을 고른다([`quote::join`] 은 플랫폼 규칙).
/// 이 문자열을 셸로 실행하지 않는다(실행은 [`resume_args`]).
pub fn resume_cmd_with(rec: &Record, join: fn(&[String]) -> String) -> String {
    let mut all = vec!["claude".to_string()];
    all.extend(resume_args(rec));
    join(&all)
}

/// 이 플랫폼의 인용 규칙으로 만든 표시용 `resume_cmd`.
pub fn resume_cmd(rec: &Record) -> String {
    resume_cmd_with(rec, quote::join)
}

/// 사람의 답을 기다리게 하는 Notification(`notification_type`) — 승인 창(서브에이전트·팀원 것 포함)과 MCP 의 입력 요청.
/// 다른 백그라운드 작업이 끝남(`agent_completed`)·입력 필요(`agent_needs_input`), 로그인 성공 같은 알림도 같은 hook 으로 오므로
/// 이 밖의 알림은 이 세션의 상태가 아니다. 알림 패널([`crate::notify::prepare`])도 이 목록만 띄운다.
pub const WAIT_NOTIFICATIONS: [&str; 4] =
    ["permission_prompt", "worker_permission_prompt", "elicitation_dialog", "elicitation_url_dialog"];

/// 승인과 상관없는 Notification 이면 참 — 세션 기록을 건드리지 않는다. 종류가 없는 예전 Claude Code 의 알림은 승인으로 본다.
pub fn unrelated_notification(state: &str, data: &Record) -> bool {
    state == "waiting"
        && non_empty_str(data.get("notification_type")).is_some_and(|k| k != "idle_prompt" && !WAIT_NOTIFICATIONS.contains(&k))
}

/// 응답을 마친 뒤 입력을 오래 기다릴 때 오는 Notification 은 승인 대기가 아니라 (입력) 대기다.
pub fn effective_state(state: &str, data: &Record) -> String {
    if state == "waiting" && data.get("notification_type").and_then(Value::as_str) == Some("idle_prompt") {
        "idle".to_string()
    } else {
        state.to_string()
    }
}

/// 기록의 pid 가 양의 정수면 그 값. Python 은 pid 가 None·문자열이면 `alive()` 가 참이다.
pub(crate) fn record_pid(rec: &Record) -> Option<u32> {
    rec.get("pid").and_then(Value::as_u64).filter(|&p| p > 0).and_then(|p| u32::try_from(p).ok())
}

fn record_ctime(rec: &Record) -> Option<f64> {
    rec.get("pid_ctime").and_then(Value::as_f64)
}

fn set(map: &mut Record, key: &str, value: impl Into<Value>) {
    map.insert(key.to_string(), value.into());
}

/// 서브에이전트 시작·도구 호출·승인 대기·종료를 세션 기록의 subagents 에 남긴다 — `state.py:track_subagent`.
fn track_subagent(rec: &mut Record, data: &Record, state: &str, now: f64, env: &dyn Env) {
    let aid = match data.get("agent_id") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => return,
    };
    let event = data.get("hook_event_name").and_then(Value::as_str);
    if !rec.get("subagents").is_some_and(Value::is_object) {
        set(rec, "subagents", Map::new());
    }
    let Some(Value::Object(subs)) = rec.get_mut("subagents") else { return };
    if !subs.get(&aid).is_some_and(Value::is_object) {
        let mut fresh = Map::new();
        let kind = non_empty_str(data.get("agent_type")).unwrap_or("");
        set(&mut fresh, "type", kind);
        set(&mut fresh, "started", now);
        set(&mut fresh, "state", "running");
        set(&mut fresh, "started_seen", false);
        subs.insert(aid.clone(), Value::Object(fresh));
    }
    let Some(Value::Object(sub)) = subs.get_mut(&aid) else { return };
    // 시작 hook 을 본 서브에이전트만 시작 시각을 믿는다 — 시작 없이 끝 hook 만 왔으면 시작 시각은 끝 시각을 흉내 낸 값이다
    if event == Some("SubagentStart") && !truthy(sub.get("started_seen")) {
        set(sub, "started", now);
        set(sub, "started_seen", true);
    }
    if !truthy(sub.get("description")) {
        let transcript = data.get("transcript_path").and_then(Value::as_str).unwrap_or("");
        let meta = if transcript.is_empty() { Map::new() } else { env.subagent_meta(transcript, &aid) };
        if !meta.is_empty() {
            set(sub, "description", meta.get("description").cloned().unwrap_or_else(|| "".into()));
            set(sub, "background", meta.get("requestShape").and_then(Value::as_str) == Some("background"));
        }
    }
    if event == Some("SubagentStop") {
        set(sub, "state", "done");
        set(sub, "ended", now);
        if let Some(msg) = non_empty_str(data.get("last_assistant_message")) {
            set(sub, "last_message", take_chars(&collapse_ws(msg), 200));
        }
    } else {
        // 끝난 서브에이전트도 남은 백그라운드 작업 때문에 다시 깨어나 도구를 쓸 수 있다
        sub.shift_remove("ended");
        set(sub, "state", if state == "waiting" { "waiting" } else { "running" });
        if let Some(tool) = data.get("tool_name").filter(|v| truthy(Some(v))) {
            set(sub, "tool", tool.clone());
        }
    }
    set(sub, "updated", now);
    // 끝난 지 1시간이 넘은 서브에이전트는 기록에서 지운다
    subs.retain(|_, v| {
        let done = v.get("state").and_then(Value::as_str) == Some("done");
        let ended = v.get("ended").and_then(Value::as_f64).unwrap_or(now);
        !(done && now - ended > 3600.0)
    });
}

/// hook 이벤트 하나를 세션 기록에 반영한다 — `state.py:update` 의 순수한 부분.
pub fn apply(mut rec: Record, state_arg: &str, data: &Record, now: f64, env: &dyn Env) -> Outcome {
    let sub_event = truthy(data.get("agent_id"));
    let mut state = effective_state(state_arg, data);
    let event = data.get("hook_event_name").and_then(Value::as_str);

    // 처음 보는 세션이거나 --resume 으로 다시 열린 세션이면 프로세스 정보를 새로 잡는다
    let alive = record_pid(&rec).is_none_or(|pid| env.alive(pid, record_ctime(&rec)));
    if !rec.contains_key("pid") || event == Some("SessionStart") || !alive {
        match env.find_claude() {
            Some(p) => {
                set(&mut rec, "pid", p.pid);
                match p.ctime {
                    Some(ct) => set(&mut rec, "pid_ctime", ct),
                    None => {
                        rec.shift_remove("pid_ctime");
                    }
                }
                set(&mut rec, "tty", p.tty.clone().map_or(Value::Null, Value::from));
                set(&mut rec, "headless", p.headless);
                set(&mut rec, "started", now);
                set(&mut rec, "args", carry_args(&p.argv));
            }
            None => {
                set(&mut rec, "pid", Value::Null);
                rec.shift_remove("pid_ctime");
                set(&mut rec, "tty", Value::Null);
                set(&mut rec, "headless", true);
                set(&mut rec, "started", now);
            }
        }
    } else if !rec.contains_key("args") && truthy(rec.get("pid")) {
        let argv = record_pid(&rec).map(|pid| env.argv(pid)).unwrap_or_default();
        set(&mut rec, "args", carry_args(&argv));
    }
    if let Some(t) = data.get("transcript_path").filter(|v| truthy(Some(v))) {
        set(&mut rec, "transcript_path", t.clone());
    }
    for key in ["permission_mode", "model"] {
        if let (false, Some(v)) = (sub_event, non_empty_str(data.get(key))) {
            set(&mut rec, key, v);
        }
    }
    let cwd = non_empty_str(data.get("cwd"))
        .or_else(|| non_empty_str(rec.get("cwd")))
        .map(str::to_string)
        .unwrap_or_else(|| env.cwd());
    let sid = data.get("session_id").cloned().unwrap_or(Value::Null);
    set(&mut rec, "session_id", sid);
    set(&mut rec, "cwd", cwd.as_str());
    set(&mut rec, "project", paths::basename(&cwd));
    set(&mut rec, "updated", now);
    // 프로젝트 경로 — claude 를 띄운 폴더. cwd 는 Claude 가 하위 폴더로 cd 하거나 워크트리에 들어가면 따라 바뀌지만
    // Claude Code 가 hook 에 넘기는 CLAUDE_PROJECT_DIR 은 그대로다(2.1.304 실측). 보드는 이 경로로 카드를 묶는다
    let project_dir = env
        .project_dir()
        .filter(|d| !d.is_empty())
        .or_else(|| non_empty_str(rec.get("project_dir")).map(str::to_string))
        .unwrap_or_else(|| cwd.clone());
    set(&mut rec, "project_dir", project_dir);

    // 같은 프로젝트에서 여러 세션을 띄워도 구분되도록 마지막 요청 문구를 주제로 남긴다
    let prompt = collapse_ws(data.get("prompt").and_then(Value::as_str).unwrap_or(""));
    let head = take_chars(&prompt, 40);
    let topic_changed =
        !prompt.is_empty() && !prompt.starts_with('<') && rec.get("topic").and_then(Value::as_str) != Some(head.as_str());
    if topic_changed {
        set(&mut rec, "topic", head);
    }

    let notification = event == Some("Notification");
    if sub_event {
        track_subagent(&mut rec, data, &state, now, env);
        // 서브에이전트의 도구 호출은 세션 상태를 바꾸지 않는다 — 승인 요청만 세션도 승인 대기로 올리고,
        // 승인이 끝나 서브에이전트가 다시 움직이면 세션을 승인 요청 전 상태로 돌려놓는다
        let current = rec.get("state").and_then(Value::as_str).map(str::to_string);
        if state == "waiting" {
            if current.as_deref() != Some("waiting") {
                set(&mut rec, "state_before_sub_wait", current.as_deref().unwrap_or("working"));
            }
        } else if current.as_deref() == Some("waiting") && rec.contains_key("state_before_sub_wait") {
            let before = rec.shift_remove("state_before_sub_wait");
            state = before.as_ref().and_then(Value::as_str).unwrap_or("working").to_string();
        } else {
            state = current.unwrap_or_else(|| "working".to_string());
        }
    } else if state == "ended" {
        if let Some(Value::Object(subs)) = rec.get_mut("subagents") {
            for sub in subs.values_mut().filter_map(Value::as_object_mut) {
                if sub.get("state").and_then(Value::as_str) != Some("done") {
                    set(sub, "state", "done");
                    set(sub, "ended", now);
                }
            }
        }
    } else if notification
        && data.get("notification_type").and_then(Value::as_str) == Some("permission_prompt")
        && rec.get("state").and_then(Value::as_str) == Some("idle")
    {
        // 쉬고 있는 세션은 스스로 승인을 묻지 않으므로 서브에이전트의 승인 요청이다 — 서브에이전트가 다시 움직이면 대기로 돌린다
        set(&mut rec, "state_before_sub_wait", "idle");
    }

    // 승인 알림(Notification)은 서브에이전트의 승인 요청이어도 agent_id 없이 오므로 되돌릴 상태를 지우지 않는다
    if !sub_event && !notification {
        rec.shift_remove("state_before_sub_wait");
    }
    // 사용 한도에 걸려 응답이 끊긴 세션 — 보드가 계정을 자동 전환한 뒤 이어서 진행시킨다.
    // rate_limit 은 서버의 일시적인 요청 제한일 수도 있어 한도 문구일 때만 남기고,
    // 새 요청·도구 사용·응답 완료·다시 열기가 오면 세션이 다시 움직인 것이므로 지운다
    if event == Some("StopFailure") && data.get("error").and_then(Value::as_str) == Some("rate_limit") {
        let msg = collapse_ws(data.get("last_assistant_message").and_then(Value::as_str).unwrap_or(""));
        if limit_re().is_match(&msg) {
            let mut hit = Map::new();
            set(&mut hit, "at", now);
            set(&mut hit, "message", take_chars(&msg, 200));
            set(&mut rec, "limit_hit", hit);
        }
    } else if !sub_event
        && matches!(event, Some("UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop" | "SessionStart"))
    {
        rec.shift_remove("limit_hit");
    }
    let changed = rec.get("state").and_then(Value::as_str) != Some(state.as_str());
    // Esc 로 끊으면 hook 이 오지 않아 다음 요청은 작업 중 → 작업 중이다 — 새 요청이면 시작 시각을 새로 잡는다
    if changed || event == Some("UserPromptSubmit") {
        set(&mut rec, "state", state.as_str());
        set(&mut rec, "since", now);
    }
    if state == "idle" {
        if let Some(msg) = non_empty_str(data.get("last_assistant_message")) {
            set(&mut rec, "last_message", take_chars(&collapse_ws(msg), 200));
        }
    }
    if state == "waiting" {
        if let Some(msg) = non_empty_str(data.get("message")) {
            set(&mut rec, "last_message", take_chars(msg, 200));
        }
    }

    let cmd = resume_cmd(&rec);
    set(&mut rec, "resume_cmd", cmd);
    let args = resume_args(&rec);
    set(&mut rec, "resume_args", args);

    // 터미널이 있는 세션만 제목을 바꾼다 — macOS 는 tty, Windows 는 headless 가 아닌 콘솔
    let has_terminal = truthy(rec.get("tty")) || rec.get("headless") == Some(&Value::Bool(false));
    let title = ((changed || topic_changed) && has_terminal).then(|| title_text(&state, &rec));
    Outcome { record: rec, title, remove_paused: event == Some("UserPromptSubmit") }
}

/// `"<아이콘> <project> · <topic 앞 20자>…"` — topic 이 20자 이하면 `…` 없이, 없으면 ` · ` 부분 없음.
pub fn title_text(state: &str, rec: &Record) -> String {
    let topic = rec.get("topic").and_then(Value::as_str).unwrap_or("");
    let tail = if topic.chars().count() > 20 {
        format!(" · {}…", take_chars(topic, 20))
    } else if !topic.is_empty() {
        format!(" · {topic}")
    } else {
        String::new()
    };
    let project = rec.get("project").and_then(Value::as_str).unwrap_or("");
    format!("{} {project}{tail}", icon(state))
}

fn limit_re() -> &'static regex::Regex {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)hit your|usage limit").expect("limit regex"));
    &RE
}

/// 진짜 프로세스·파일을 쓰는 [`Env`].
pub struct SystemEnv;

impl Env for SystemEnv {
    fn find_claude(&self) -> Option<ClaudeProcess> {
        proc::find_claude()
    }
    fn alive(&self, pid: u32, ctime: Option<f64>) -> bool {
        proc::alive(pid, ctime)
    }
    fn argv(&self, pid: u32) -> Vec<String> {
        proc::command_line(pid).unwrap_or_default()
    }
    fn subagent_meta(&self, transcript: &str, agent_id: &str) -> Record {
        let base = transcript.strip_suffix(".jsonl").unwrap_or(transcript);
        let path = Path::new(base).join("subagents").join(format!("agent-{agent_id}.meta.json"));
        fileio::read_record(&path).unwrap_or_default()
    }
    fn cwd(&self) -> String {
        std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
    }
    fn project_dir(&self) -> Option<String> {
        std::env::var_os("CLAUDE_PROJECT_DIR").map(|v| v.to_string_lossy().into_owned())
    }
}

/// 종료된 지 2일이 넘은 세션 파일을 지운다 — 보드를 띄우지 않는 동안 쌓이는 것을 막는다 (`state.py:prune`).
/// `.lock`·`.paused`·`.mute` 는 짝인 `.json` 이 없으면 지운다. `alive` 는 기록을 받아 그 프로세스가 살아 있는지 답한다.
pub fn prune(sessions: &Path, now: f64, alive: impl Fn(&Record) -> bool) {
    let limit = now - 2.0 * 86400.0;
    let Ok(entries) = fs::read_dir(sessions) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.ends_with(".lock") || name.ends_with(".paused") || name.ends_with(".mute") {
            let base = if let Some(stem) = name.strip_suffix(".lock") {
                stem.strip_prefix('.').unwrap_or(stem).to_string()
            } else {
                name.rsplit_once('.').map(|(b, _)| b.to_string()).unwrap_or_default()
            };
            if !sessions.join(format!("{base}.json")).exists() {
                let _ = fs::remove_file(&path);
            }
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|d| d.as_secs_f64() < limit);
        if !old {
            continue;
        }
        if let Some(rec) = fileio::read_record(&path) {
            if !truthy(rec.get("pid")) || !alive(&rec) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// `state <상태>` hook 의 파일 쪽 — stdin 으로 받은 입력을 `sessions` 폴더의 세션 기록에 반영한다.
/// 어떤 오류도 Claude Code 를 막으면 안 되므로 실패는 조용히 버린다(부르는 쪽이 패닉도 잡는다).
pub fn run(sessions: &Path, state_arg: &str, input: &[u8], env: &dyn Env) {
    let Ok(Value::Object(data)) = serde_json::from_slice::<Value>(input) else { return };
    if unrelated_notification(state_arg, &data) {
        return;
    }
    let state = effective_state(state_arg, &data);
    // 경로 구분자가 든 id 는 sessions 밖에 쓰게 되므로 받지 않는다
    let Some(sid) = non_empty_str(data.get("session_id")).filter(|s| !s.contains(['/', '\\']) && *s != "..") else {
        return;
    };
    if fs::create_dir_all(sessions).is_err() {
        return;
    }
    let event = data.get("hook_event_name").and_then(Value::as_str);
    if state == "idle" && event == Some("SessionStart") {
        prune(sessions, crate::clock::now(), proc::record_alive);
    }
    let path = sessions.join(format!("{sid}.json"));
    // 서브에이전트가 여럿 동시에 돌면 hook 도 동시에 돌므로, 읽고 고쳐 쓰는 동안 세션 파일을 잠근다
    let Ok(lock) = OpenOptions::new().create(true).truncate(false).write(true).open(lock_path(sessions, sid)) else {
        return;
    };
    if lock.lock().is_err() {
        return;
    }
    let rec = fileio::read_record(&path).unwrap_or_default();
    let out = apply(rec, &state, &data, crate::clock::now(), env);
    if out.remove_paused {
        let _ = fs::remove_file(sessions.join(format!("{sid}.paused")));
    }
    let Ok(bytes) = serde_json::to_vec(&out.record) else { return };
    // 동시에 도는 hook 끼리 같은 임시 파일을 덮어쓰지 않도록 프로세스마다 다른 이름을 쓴다
    if fileio::write_atomic(&path, sid, &bytes).is_err() {
        return;
    }
    if let Some(title) = out.title {
        set_terminal_title(&out.record, &title);
    }
    drop(lock);
}

/// `.<sid>.lock`
pub fn lock_path(sessions: &Path, sid: &str) -> PathBuf {
    sessions.join(format!(".{sid}.lock"))
}

/// 세션의 터미널 제목을 바꾼다. Windows 는 그 Claude 프로세스의 콘솔에 붙어서, 그 밖은 tty 에 OSC 2 를 쓴다.
fn set_terminal_title(rec: &Record, title: &str) {
    #[cfg(windows)]
    if let Some(pid) = record_pid(rec) {
        crate::console::set_title(pid, title);
    }
    #[cfg(not(windows))]
    if let Some(tty) = non_empty_str(rec.get("tty")) {
        use std::io::Write;
        if let Ok(mut t) = OpenOptions::new().write(true).open(format!("/dev/{tty}")) {
            let _ = write!(t, "\x1b]2;{title}\x07");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fake {
        found: Option<ClaudeProcess>,
        alive: bool,
        meta: Record,
        /// CLAUDE_PROJECT_DIR
        project_dir: Option<String>,
    }

    impl Env for Fake {
        fn find_claude(&self) -> Option<ClaudeProcess> {
            self.found.clone()
        }
        fn alive(&self, _pid: u32, _ctime: Option<f64>) -> bool {
            self.alive
        }
        fn argv(&self, _pid: u32) -> Vec<String> {
            vec!["claude".into(), "--verbose".into()]
        }
        fn subagent_meta(&self, _t: &str, _a: &str) -> Record {
            self.meta.clone()
        }
        fn cwd(&self) -> String {
            "/fallback/dir".into()
        }
        fn project_dir(&self) -> Option<String> {
            self.project_dir.clone()
        }
    }

    fn fake() -> Fake {
        Fake {
            found: Some(ClaudeProcess { pid: 42, ctime: Some(1.5), tty: None, headless: false, argv: vec!["claude".into(), "--chrome".into()] }),
            alive: true,
            meta: Map::new(),
            project_dir: None,
        }
    }

    fn obj(v: Value) -> Record {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn carry_args_rules() {
        let argv = [
            "/usr/bin/claude", "--model", "opus", "--permission-mode=plan", "--add-dir", "a", "b", "-c",
            "--verbose", "--print", "--mcp-config=x.json", "--settings", "s.json", "--verbose=1", "--agent",
        ];
        assert_eq!(
            carry_args(&argv),
            ["--model", "opus", "--permission-mode=plan", "--add-dir", "a", "b", "--verbose", "--mcp-config=x.json", "--settings", "s.json"]
        );
        assert!(carry_args(&["claude"]).is_empty());
        // 값이 없는 마지막 --agent 는 버린다(state.py 와 같다)
        assert!(carry_args(&["claude", "--agent"]).is_empty());
    }

    #[test]
    fn carry_args_dedups_same_args() {
        let run = |a: &[&str]| {
            let mut argv = vec!["claude"];
            argv.extend_from_slice(a);
            carry_args(&argv)
        };
        assert_eq!(run(&["--chrome", "--chrome"]), ["--chrome"]);
        assert_eq!(
            run(&["--allow-dangerously-skip-permissions", "--allow-dangerously-skip-permissions"]),
            ["--allow-dangerously-skip-permissions"]
        );
        assert_eq!(run(&["--model", "a", "--model", "a"]), ["--model", "a"]);
        assert_eq!(run(&["--model", "a", "--model", "b"]), ["--model", "a", "--model", "b"]);
        assert_eq!(run(&["--model=a", "--model=a"]), ["--model=a"]);
        assert_eq!(run(&["--model", "a", "--model=a"]), ["--model", "a", "--model=a"]);
        assert_eq!(run(&["--add-dir=x", "--add-dir=x"]), ["--add-dir=x"]);
        // 띄운 형은 값이 여러 개일 수 있어 중복을 거르지 않는다
        assert_eq!(run(&["--add-dir", "x", "--add-dir", "x"]), ["--add-dir", "x", "--add-dir", "x"]);
    }

    #[test]
    fn since_restarts_on_new_prompt_only() {
        let env = fake();
        let rec = obj(json!({"pid": 42, "args": [], "state": "working", "since": 100.0}));
        let hook = |name: &str| obj(json!({"session_id": "s", "hook_event_name": name, "cwd": "/w/p"}));
        // 새 요청은 같은 working 이어도 시작 시각을 새로 잡는다
        let out = apply(rec.clone(), "working", &hook("UserPromptSubmit"), 500.0, &env);
        assert_eq!(out.record["state"], "working");
        assert_eq!(out.record["since"], 500.0);
        // 도구 호출은 그대로
        let out = apply(rec.clone(), "working", &hook("PreToolUse"), 500.0, &env);
        assert_eq!(out.record["since"], 100.0);
        // 제목은 상태나 주제가 바뀔 때만 — 같은 working 의 새 요청은 제목을 다시 쓰지 않는다
        let tty_rec = obj(json!({"pid": 42, "args": [], "state": "working", "since": 100.0, "headless": false}));
        let out = apply(tty_rec.clone(), "working", &hook("UserPromptSubmit"), 500.0, &env);
        assert_eq!(out.title, None);
        let out = apply(tty_rec, "idle", &hook("Stop"), 500.0, &env);
        assert_eq!(out.title.as_deref(), Some("🟢 p"));
        assert_eq!(out.record["since"], 500.0);
    }

    #[test]
    fn resume_cmd_replaces_mode_and_model() {
        let rec = obj(json!({
            "session_id": "sid-1",
            "args": ["--model=old", "--add-dir", "my dir", "--permission-mode", "default", "--chrome"],
            "permission_mode": "plan",
            "model": "claude-opus-5-5",
        }));
        assert_eq!(
            resume_cmd_with(&rec, quote::join_posix),
            "claude --add-dir 'my dir' --chrome --permission-mode plan --model claude-opus-5-5 --resume sid-1"
        );
        assert_eq!(
            resume_cmd_with(&rec, quote::join_windows),
            "claude --add-dir \"my dir\" --chrome --permission-mode plan --model claude-opus-5-5 --resume sid-1"
        );
        // 기록에 값이 없으면 인자를 그대로 둔다. 마지막에 값 없이 남은 --model 도 지운다
        let rec = obj(json!({"session_id": "s", "args": ["--model"], "model": "m"}));
        assert_eq!(resume_cmd_with(&rec, quote::join_posix), "claude --model m --resume s");
        let rec = obj(json!({"session_id": "s"}));
        assert_eq!(resume_cmd_with(&rec, quote::join_posix), "claude --resume s");
    }

    #[test]
    fn resume_args_keep_hostile_values_whole() {
        let rec = obj(json!({
            "session_id": "sid-1",
            "args": ["--model", "old", "--add-dir", "a b", "c&d", "--settings", "{\"x\": 1}", "--chrome"],
            "permission_mode": "plan",
            "model": "x\" & calc & \"",
        }));
        assert_eq!(
            resume_args(&rec),
            ["--add-dir", "a b", "c&d", "--settings", "{\"x\": 1}", "--chrome", "--permission-mode", "plan", "--model", "x\" & calc & \"", "--resume", "sid-1"]
        );
        // apply 가 기록에 resume_cmd 와 같은 내용의 배열을 남긴다 — 위험한 모델 이름도 원소 하나
        let env = Fake { found: Some(ClaudeProcess { argv: vec!["claude".into(), "--chrome".into()], ..fake().found.unwrap() }), alive: true, meta: Map::new(), project_dir: None };
        let data = obj(json!({"session_id": "s9", "hook_event_name": "SessionStart", "cwd": "/w/p", "model": "x\" & calc & \""}));
        let out = apply(Map::new(), "idle", &data, 1.0, &env);
        assert_eq!(out.record["resume_args"], json!(["--chrome", "--model", "x\" & calc & \"", "--resume", "s9"]));
        let keys: Vec<&String> = out.record.keys().collect();
        assert_eq!(&keys[keys.len() - 2..], ["resume_cmd", "resume_args"]);
        // 기록이 비어 있어도 --resume <sid> 로 끝난다
        assert_eq!(resume_args(&obj(json!({"session_id": "s"}))), ["--resume", "s"]);
    }

    #[test]
    fn null_pid_is_not_probed_again() {
        // state.py 의 alive(None) 은 참 — pid 가 null 로 남은 기록은 SessionStart 가 아니면 다시 찾지 않는다
        let env = Fake { found: None, alive: false, meta: Map::new(), project_dir: None };
        let data = obj(json!({"session_id": "s", "hook_event_name": "UserPromptSubmit", "cwd": "/w/p"}));
        let rec = obj(json!({"pid": null, "started": 1.0}));
        let out = apply(rec, "working", &data, 10.0, &env);
        assert_eq!(out.record["started"], 1.0);
        assert!(out.remove_paused);
        // 처음 보는 세션은 찾고, 못 찾으면 headless
        let out = apply(Map::new(), "working", &data, 10.0, &env);
        assert_eq!(out.record["pid"], Value::Null);
        assert_eq!(out.record["headless"], true);
        assert!(!out.record.contains_key("args"));
        assert_eq!(out.title, None);
    }

    #[test]
    fn dead_pid_is_replaced_and_args_filled() {
        let mut env = fake();
        let data = obj(json!({"session_id": "s", "hook_event_name": "PreToolUse"}));
        env.alive = false;
        let rec = obj(json!({"pid": 7, "pid_ctime": 9.0, "cwd": "/old/proj", "state": "working"}));
        let out = apply(rec, "working", &data, 5.0, &env);
        assert_eq!(out.record["pid"], 42);
        assert_eq!(out.record["pid_ctime"], 1.5);
        assert_eq!(out.record["args"], json!(["--chrome"]));
        assert_eq!(out.record["project"], "proj");
        // 살아 있는데 args 만 없으면 args 만 채운다
        env.alive = true;
        let rec = obj(json!({"pid": 7, "state": "working"}));
        let out = apply(rec, "working", &data, 5.0, &env);
        assert_eq!(out.record["pid"], 7);
        assert_eq!(out.record["args"], json!(["--verbose"]));
        assert_eq!(out.record["cwd"], "/fallback/dir");
    }

    #[test]
    fn subagent_meta_and_cleanup() {
        let mut env = fake();
        env.meta = obj(json!({"description": "코드 찾기", "requestShape": "background"}));
        let rec = obj(json!({
            "pid": 42, "args": [], "state": "working",
            "subagents": {"old": {"state": "done", "ended": 100.0}, "recent": {"state": "done", "ended": 4000.0}}
        }));
        let data = obj(json!({"session_id": "s", "hook_event_name": "SubagentStart", "agent_id": "a1",
                              "agent_type": "Explore", "transcript_path": "/t/x.jsonl", "model": "ignored"}));
        let out = apply(rec, "subagent", &data, 5000.0, &env);
        let subs = out.record["subagents"].as_object().unwrap();
        assert!(!subs.contains_key("old"));
        assert!(subs.contains_key("recent"));
        assert_eq!(subs["a1"]["description"], "코드 찾기");
        assert_eq!(subs["a1"]["background"], true);
        assert_eq!(out.record["state"], "working");
        assert!(!out.record.contains_key("model"));
        assert_eq!(out.title, None);
    }

    #[test]
    fn subagent_started_seen_only_on_start_hook() {
        let env = fake();
        let ev = |name: &str, aid: &str| obj(json!({"session_id": "s", "hook_event_name": name, "agent_id": aid, "agent_type": "Explore", "tool_name": "Bash"}));
        let mut rec = obj(json!({"pid": 42, "args": [], "state": "working"}));
        // 시작 hook 을 본 서브에이전트: 시작 시각을 믿는다
        rec = apply(rec, "subagent", &ev("SubagentStart", "seen"), 100.0, &env).record;
        assert_eq!(rec["subagents"]["seen"]["started_seen"], true);
        assert_eq!(rec["subagents"]["seen"]["started"], 100.0);
        // 시작 뒤 도구·종료 hook 은 시작 시각과 표지를 그대로 둔다
        rec = apply(rec, "subagent", &ev("PreToolUse", "seen"), 110.0, &env).record;
        rec = apply(rec, "subagent", &ev("SubagentStop", "seen"), 120.0, &env).record;
        assert_eq!(rec["subagents"]["seen"]["started_seen"], true);
        assert_eq!(rec["subagents"]["seen"]["started"], 100.0);
        assert_eq!(rec["subagents"]["seen"]["ended"], 120.0);
        // 시작 없이 끝·도구 hook 만 온 기록: 표지는 거짓이다
        rec = apply(rec, "subagent", &ev("SubagentStop", "orphan_stop"), 130.0, &env).record;
        rec = apply(rec, "subagent", &ev("PreToolUse", "orphan_tool"), 131.0, &env).record;
        assert_eq!(rec["subagents"]["orphan_stop"]["started_seen"], false);
        assert_eq!(rec["subagents"]["orphan_tool"]["started_seen"], false);
        assert_eq!(rec["subagents"]["orphan_tool"]["started"], 131.0);
        // 시작 없이 만들어진 기록에 시작 hook 이 뒤늦게 오면 그때부터 믿는다
        rec = apply(rec, "subagent", &ev("SubagentStart", "orphan_tool"), 140.0, &env).record;
        assert_eq!(rec["subagents"]["orphan_tool"]["started_seen"], true);
        assert_eq!(rec["subagents"]["orphan_tool"]["started"], 140.0);
        // 표지 이전에 쓰인 기록(started_seen 키 없음)에 도구 hook 이 와도 키를 만들지 않는다
        let old = obj(json!({"pid": 42, "args": [], "state": "working", "subagents": {"x": {"type": "", "started": 1.0, "state": "running"}}}));
        let out = apply(old, "subagent", &ev("PreToolUse", "x"), 5.0, &env).record;
        assert!(!out["subagents"]["x"].as_object().unwrap().contains_key("started_seen"));
        // 키 순서는 state.py 와 같다: type, started, state, started_seen
        let keys: Vec<&String> = rec["subagents"]["seen"].as_object().unwrap().keys().collect();
        assert_eq!(&keys[..4], ["type", "started", "state", "started_seen"]);
    }

    #[test]
    fn ended_closes_running_subagents() {
        let env = fake();
        let rec = obj(json!({"pid": 42, "args": [], "state": "idle", "headless": false,
                             "subagents": {"a": {"state": "running"}, "b": {"state": "done", "ended": 1.0}}}));
        let data = obj(json!({"session_id": "s", "hook_event_name": "SessionEnd", "cwd": "/w/p"}));
        let out = apply(rec, "ended", &data, 50.0, &env);
        assert_eq!(out.record["subagents"]["a"]["state"], "done");
        assert_eq!(out.record["subagents"]["a"]["ended"], 50.0);
        assert_eq!(out.record["subagents"]["b"]["ended"], 1.0);
        assert_eq!(out.title.as_deref(), Some("⚫ p"));
    }

    #[test]
    fn subagent_permission_wait_survives_notification() {
        let env = fake();
        let sub = |name: &str| obj(json!({"session_id": "s", "hook_event_name": name, "agent_id": "a1", "tool_name": "Bash"}));
        let main = |name: &str| obj(json!({"session_id": "s", "hook_event_name": name, "tool_name": "Bash"}));
        let noti = obj(json!({"session_id": "s", "hook_event_name": "Notification",
                              "message": "Claude needs your permission", "notification_type": "permission_prompt"}));
        let run = |start: &str, steps: &[(&Record, &str, &str)]| {
            let mut rec = obj(json!({"pid": 42, "args": [], "state": start}));
            for (i, (data, arg, want)) in steps.iter().enumerate() {
                rec = apply(rec, arg, data, i as f64, &env).record;
                assert_eq!(rec["state"], *want, "{start} 에서 {i} 번째");
            }
        };
        // 서브에이전트 승인 뒤 알림(agent_id 없음)이 와도 승인이 끝나면 요청 전 상태로 돌아간다
        run("idle", &[(&sub("PermissionRequest"), "waiting", "waiting"), (&noti, "waiting", "waiting"),
                      (&sub("PostToolUse"), "working", "idle")]);
        run("working", &[(&sub("PermissionRequest"), "waiting", "waiting"), (&noti, "waiting", "waiting"),
                         (&sub("PostToolUse"), "working", "working")]);
        // 쉬는 세션에 승인 알림만 오면 서브에이전트의 승인이다
        run("idle", &[(&noti, "waiting", "waiting"), (&sub("PostToolUse"), "working", "idle")]);
        // 메인 자신의 승인은 다른 서브에이전트가 움직여도 풀리지 않는다
        run("working", &[(&main("PermissionRequest"), "waiting", "waiting"), (&noti, "waiting", "waiting"),
                         (&sub("PostToolUse"), "working", "waiting"), (&main("PostToolUse"), "working", "working")]);
        run("working", &[(&noti, "waiting", "waiting"), (&sub("PostToolUse"), "working", "waiting")]);
    }

    #[test]
    fn title_text_shapes() {
        let rec = obj(json!({"project": "proj", "topic": "가나다라마바사아자차카타파하가나다라마바사아"}));
        assert_eq!(title_text("working", &rec), "🔵 proj · 가나다라마바사아자차카타파하가나다라마바…");
        let rec = obj(json!({"project": "proj", "topic": "짧은 주제"}));
        assert_eq!(title_text("waiting", &rec), "🟠 proj · 짧은 주제");
        let rec = obj(json!({"project": "proj"}));
        assert_eq!(title_text("idle", &rec), "🟢 proj");
    }

    #[test]
    fn limit_hit_needs_limit_words() {
        let env = fake();
        let base = obj(json!({"pid": 42, "args": [], "state": "working"}));
        let data = obj(json!({"session_id": "s", "hook_event_name": "StopFailure", "error": "rate_limit",
                              "last_assistant_message": "API Error: overloaded"}));
        let out = apply(base.clone(), "idle", &data, 1.0, &env);
        assert!(!out.record.contains_key("limit_hit"));
        let data = obj(json!({"session_id": "s", "hook_event_name": "StopFailure", "error": "rate_limit",
                              "last_assistant_message": "Claude USAGE LIMIT reached"}));
        let out = apply(base, "idle", &data, 1.0, &env);
        assert_eq!(out.record["limit_hit"]["message"], "Claude USAGE LIMIT reached");
    }

    #[test]
    fn prune_removes_old_dead_and_orphans() {
        let dir = std::env::temp_dir().join(format!("cb-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86400);
        let write = |name: &str, body: &str, aged: bool| {
            let p = dir.join(name);
            fs::write(&p, body).unwrap();
            if aged {
                fs::File::options().write(true).open(&p).unwrap().set_modified(old).unwrap();
            }
        };
        write("dead.json", r#"{"pid": 111}"#, true);
        write("nopid.json", r#"{"state": "ended"}"#, true);
        write("live.json", r#"{"pid": 222}"#, true);
        write("fresh.json", r#"{"pid": 111}"#, false);
        write("broken.json", "{", true);
        write(".live.lock", "", false);
        write(".gone.lock", "", false);
        write("gone.paused", "{}", false);
        write("gone.mute", "", false);
        write("fresh.mute", "", false);
        prune(&dir, crate::clock::now(), |rec| rec["pid"] == 222);
        let mut left: Vec<String> =
            fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, [".live.lock", "broken.json", "fresh.json", "fresh.mute", "live.json"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_writes_record_and_removes_paused() {
        let dir = std::env::temp_dir().join(format!("cb-run-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("s1.paused"), "{}").unwrap();
        let input = br#"{"session_id":"s1","hook_event_name":"UserPromptSubmit","cwd":"/w/proj","prompt":"hello"}"#;
        run(&dir, "working", input, &Fake { found: None, alive: true, meta: Map::new(), project_dir: None });
        let rec = fileio::read_record(&dir.join("s1.json")).unwrap();
        assert_eq!(rec["state"], "working");
        assert_eq!(rec["topic"], "hello");
        assert!(!dir.join("s1.paused").exists());
        assert!(dir.join(".s1.lock").exists());
        // 깨진 입력·session_id 없음·경로가 든 id 는 아무것도 쓰지 않는다
        run(&dir, "working", b"not json", &fake());
        run(&dir, "working", br#"{"session_id":"../x"}"#, &fake());
        run(&dir, "working", br#"{"cwd":"/a"}"#, &fake());
        let n = fs::read_dir(&dir).unwrap().count();
        assert_eq!(n, 2);
        // BOM 은 hook 입력을 받는 쪽(crates/hook main.rs read_input)이 뗀다 — 여기서는 받지 않는다
        run(&dir, "working", b"\xEF\xBB\xBF{\"session_id\":\"bom\"}", &fake());
        assert!(!dir.join("bom.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_ignores_unrelated_notifications() {
        let dir = std::env::temp_dir().join(format!("cb-noti-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let env = Fake { found: None, alive: true, meta: Map::new(), project_dir: None };
        let noti = |kind: &str| {
            format!(r#"{{"session_id":"s1","hook_event_name":"Notification","message":"m","notification_type":"{kind}"}}"#)
        };
        run(&dir, "working", br#"{"session_id":"s1","hook_event_name":"UserPromptSubmit","prompt":"hi"}"#, &env);
        let before = fs::read(dir.join("s1.json")).unwrap();
        // 다른 백그라운드 작업이 끝남·입력 필요, 로그인 성공 같은 알림은 기록을 그대로 둔다
        for kind in ["agent_completed", "agent_needs_input", "auth_success", "push_notification"] {
            run(&dir, "waiting", noti(kind).as_bytes(), &env);
            assert_eq!(fs::read(dir.join("s1.json")).unwrap(), before, "{kind}");
        }
        for kind in WAIT_NOTIFICATIONS {
            assert!(!unrelated_notification("waiting", &obj(serde_json::from_str(&noti(kind)).unwrap())), "{kind}");
        }
        run(&dir, "waiting", noti("permission_prompt").as_bytes(), &env);
        assert_eq!(fileio::read_record(&dir.join("s1.json")).unwrap()["state"], "waiting");
        // 종류가 없는 예전 알림·idle_prompt 는 지금처럼
        let old = obj(json!({"session_id": "s1", "hook_event_name": "Notification", "message": "m"}));
        assert!(!unrelated_notification("waiting", &old));
        run(&dir, "waiting", noti("idle_prompt").as_bytes(), &env);
        assert_eq!(fileio::read_record(&dir.join("s1.json")).unwrap()["state"], "idle");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_dir_stays_through_cd_and_worktree() {
        let hook = |cwd: &str| obj(json!({"session_id": "s", "hook_event_name": "PreToolUse", "cwd": cwd}));
        let walk = |env: &Fake, rec: Record| {
            let mut rec = rec;
            for cwd in ["/w/repo", "/w/repo/sub", "/w/repo/.claude/worktrees/t1"] {
                rec = apply(rec, "working", &hook(cwd), 1.0, env).record;
                assert_eq!(rec["cwd"], cwd);
                assert_eq!(rec["project"], paths::basename(cwd));
                assert_eq!(rec["project_dir"], "/w/repo", "cwd {cwd}");
            }
        };
        // 환경 변수가 있으면 그 값 — 기록에 다른 값이 있어도 환경 변수가 이긴다
        let launched = Fake { project_dir: Some("/w/repo".into()), ..fake() };
        walk(&launched, Map::new());
        walk(&launched, obj(json!({"pid": 42, "args": [], "state": "working", "project_dir": "/elsewhere"})));
        // 환경 변수가 없거나 비어 있으면 처음 기록한 값을 지킨다(처음엔 그 hook 의 cwd)
        walk(&fake(), Map::new());
        walk(&Fake { project_dir: Some(String::new()), ..fake() }, Map::new());
        // project_dir 이 없는 예전 기록은 그 hook 의 cwd 부터
        let rec = obj(json!({"pid": 42, "args": [], "state": "working", "cwd": "/w/old"}));
        let out = apply(rec, "working", &hook("/w/repo/sub"), 1.0, &fake());
        assert_eq!(out.record["project_dir"], "/w/repo/sub");
        // state.py 와 같은 키 순서 — updated 바로 뒤
        let keys: Vec<&String> = out.record.keys().collect();
        let at = keys.iter().position(|k| *k == "updated").unwrap();
        assert_eq!(keys[at + 1], "project_dir");
    }
}
