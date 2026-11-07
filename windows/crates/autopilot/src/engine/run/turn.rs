//! 턴 — claude 를 띄워 첫 줄을 넣고, 줄을 읽어 진행 기록으로 옮기고, 끝맺는다(원본 `turn` 3779~3910).
//! 루프는 `turn_step` 을 부를 때마다 한 걸음(줄 하나 읽기 + 1초마다 Tick·control)만 나아간다.
//! 끝맺음(`finish_turn`·`interrupt_claude`)은 마지막에 `ClaudeExited` 를 넣는다 — 그 뒤는 `decide` 가 정한다.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::guard;
use super::load::session_uuid;
use super::prompt::SYSTEM_PROMPT;
use super::{dur, jnum, slot_f, Engine};
use crate::engine::decide::{turn_progress, ClaudeOutcome, Event, Next, TurnProgress, Who};
use crate::model::Num;
use crate::ports::{ClaudeLaunch, EnvKind, PortErrorKind, SessionPolicy, SessionStart};
use crate::stream::{parse_line, tool_input_summary, Emit, StreamState, StreamUsage};

/// 한 번 띄운 claude 의 턴 동안 이어지는 값
pub(super) struct Turn {
    n: usize,
    /// 마지막으로 1초 일(Tick·control)을 한 때
    last_sec: f64,
    last_save: f64,
    /// 단위 시계를 마지막으로 더한 때
    last_tick: f64,
    bg_cleared: Option<f64>,
    at_eof: bool,
    exit: Option<i32>,
    /// 이번 띄우기가 stderr.log 에 쓰기 시작한 자리
    stderr_start: u64,
    /// engine.json `turns` 에 이미 더한 stream 턴 수
    turns_seen: i64,
    usage_seen: Option<StreamUsage>,
}

enum Pump {
    Line,
    Idle,
    Eof,
}

const FILE_WRITE_TOOLS: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// 고정 설정에 `allowed_tools` 가 없을 때의 허용 목록 기본값(원본 `defaultAllowed`). Bash 는 맨 이름으로 들어 있다
const DEFAULT_ALLOWED: [&str; 11] =
    ["Read", "Glob", "Grep", "Bash", "Task", "TaskStop", "SendMessage", "Skill", "ToolSearch", "Monitor", "TodoWrite"];

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// 앞 `n` 글자
fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl<'p> Engine<'p> {
    // ── 띄우기 ──

    /// `StartClaude` 수행. 못 띄우면 곧바로 `ClaudeExited`(결과 없음)를 넣는다
    pub(super) fn start_claude(&mut self, n: usize, resume: bool, next: Next) {
        if let Err(why) = self.try_launch(n, resume, &next) {
            let now = self.now();
            let outcome = ClaudeOutcome {
                session_started: false,
                result_seen: false,
                last_error: false,
                limit: false,
                limit_resets: None,
                budget: false,
                err_text: String::new(),
                exit_code: None,
                stderr_tail: format!("claude 를 띄우지 못했습니다: {why}"),
            };
            self.dispatch(Event::ClaudeExited { now, outcome });
        }
    }

    fn try_launch(&mut self, n: usize, resume: bool, next: &Next) -> Result<(), String> {
        let now = self.now();
        let run_id = self.run_id();
        let nanos = (now * 1e9) as u128;
        let pid = self.cfg.pid;

        // 세션 id: 이어 열 때는 engine.json 의 것, 새로 열 때는 앞서 띄운 적이 없는 id (띄웠다 끊긴 id 는 다시 쓰지 않는다)
        let (sid, resume, first) = {
            let e = self.unit_mut(n);
            let first = e.first_message.get().cloned().unwrap_or_default();
            let existing = e.session_id.get().filter(|s| !s.is_empty()).cloned();
            let tried = e.extra.get("session_tried").and_then(Value::as_bool) == Some(true);
            match existing {
                Some(s) if resume => (s, true, first),
                Some(s) if !tried => (s, false, first),
                _ => {
                    let runs = slot_f(&e.claude_runs);
                    let seed = format!("{run_id}\0{n}\0{nanos}\0{pid}\0{runs}");
                    (session_uuid(seed.as_bytes()), false, first)
                }
            }
        };
        if !resume {
            let e = self.unit_mut(n);
            e.session_id.set(sid.clone());
            e.extra.insert("session_tried".into(), Value::Bool(true));
            let mut list = e.sessions.get().and_then(Value::as_array).cloned().unwrap_or_default();
            if !list.iter().any(|x| x.as_str() == Some(sid.as_str())) {
                list.push(Value::from(sid.clone()));
            }
            e.sessions.set(Value::Array(list));
        }
        // 새 세션이면 이어 가는 말이 아니라 단위의 첫 입력을 넘긴다
        let text = if resume { next.text().map_or(first, str::to_string) } else { first };

        let policy = self.session_policy(n);
        let settings = self.p.env.render_settings(&policy).map_err(|e| format!("권한 설정을 만들지 못했습니다: {}", e.message))?;
        let settings_file = self.cfg.private_dir.join(format!("settings-{n}.json"));
        self.p.files.write_json(&settings_file, &settings).map_err(|e| format!("권한 설정 파일을 쓰지 못했습니다: {}", e.message))?;
        let prompt_file = self.cfg.private_dir.join("system-prompt.md");
        self.p
            .files
            .remove(&prompt_file)
            .and_then(|()| self.p.files.append_line(&prompt_file, &self.system_prompt_text()))
            .map_err(|e| format!("시스템 프롬프트 파일을 쓰지 못했습니다: {}", e.message))?;

        let unit_dir = self.unit_dir(n);
        let stderr_log = unit_dir.join("stderr.log");
        let stderr_start = self.p.files.stat(&stderr_log).ok().flatten().map_or(0, |s| s.len);
        let mut env = vec![
            ("CLAUDE_BOARD_AUTOPILOT".to_string(), run_id.clone()),
            ("CLAUDE_BOARD_AUTOPILOT_DIR".to_string(), self.cfg.dir.to_string_lossy().into_owned()),
            ("CLAUDE_BOARD_UNIT_DIR".to_string(), unit_dir.to_string_lossy().into_owned()),
        ];
        env.extend(self.cfg.claude_env_extra.iter().cloned());

        let cap_usd = self.state.units[n - 1].caps.usd;
        let cost_done = slot_f(&self.unit_mut(n).cost_done);
        let mut extra_args: Vec<String> = [
            "--setting-sources",
            "project,local",
            "--permission-mode",
            "dontAsk",
            "--permission-prompts",
            "none",
            "--strict-mcp-config",
            "--max-budget-usd",
        ]
        .map(String::from)
        .to_vec();
        extra_args.push(format!("{:.2}", (cap_usd - cost_done).max(0.01)));
        extra_args.extend(strs(self.frozen.extra.get("claude_args")));

        let mut add_dirs = vec![unit_dir.clone()];
        add_dirs.extend(self.write_roots());
        let launch = ClaudeLaunch {
            session: if resume { SessionStart::Resume(sid.clone()) } else { SessionStart::New(sid.clone()) },
            cwd: PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default()),
            model: self.state.model.clone(),
            effort: self.frozen.effort.get().filter(|e| !e.is_empty()).cloned(),
            settings_file,
            system_prompt_file: Some(prompt_file),
            add_dirs,
            plugin_dir: None,
            env,
            path_env: vec!["CLAUDE_BOARD_AUTOPILOT_DIR".to_string(), "CLAUDE_BOARD_UNIT_DIR".to_string()],
            extra_args,
            stderr_log,
        };
        let mut child = self.p.env.spawn_claude(&launch).map_err(|e| e.message)?;
        let line = json!({"type": "user", "message": {"role": "user", "content": text}}).to_string();
        if let Err(e) = child.write_line(&line) {
            let _ = child.kill_tree();
            return Err(format!("첫 입력을 보내지 못했습니다: {}", e.message));
        }
        let child_pid = child.pid();
        self.child = Some(child);

        let t = self.now();
        self.stream = Some(match self.stream.take() {
            Some(mut s) if self.stream_sid == sid => {
                s.restart_turn(t);
                s
            }
            old => {
                // 세션 id 가 바뀌어도 단위에 걸친 셈(턴 수·사용률·글 꼬리)은 이어 간다
                let mut s = StreamState::new(&sid, t);
                if let Some(o) = old {
                    s.turns_total = o.turns_total;
                    s.usage = o.usage;
                    s.text_tail = o.text_tail;
                }
                s
            }
        });
        self.stream_sid = sid;
        let (turns_seen, usage_seen) = self.stream.as_ref().map_or((0, None), |s| (s.turns_total, s.usage.clone()));

        // 생성 시각은 직전 엔진이 남긴 프로세스를 가려 끝낼 때 쓴다(같은 pid 를 다른 프로세스가 물려받은 경우를 거른다)
        let ctime = claude_board_core::proc::creation_time(child_pid);
        let e = self.unit_mut(n);
        let runs = slot_f(&e.claude_runs) + 1.0;
        e.claude_runs.set(Num::from_f64(runs));
        e.procs.set(json!({"claude_pid": child_pid, "ctime": ctime}));
        self.save_unit(n);
        self.turn = Some(Turn {
            n,
            last_sec: t,
            last_save: t,
            last_tick: t,
            bg_cleared: None,
            at_eof: false,
            exit: None,
            stderr_start,
            turns_seen,
            usage_seen,
        });
        Ok(())
    }

    fn system_prompt_text(&self) -> String {
        let mut s = SYSTEM_PROMPT.to_string();
        let instruction = self
            .frozen
            .extra
            .get("set")
            .and_then(|set| set.get("instruction"))
            .and_then(Value::as_str)
            .filter(|i| !i.is_empty());
        if let Some(i) = instruction {
            s.push_str("\n\n# 이 프로젝트의 자율주행 지시(보드가 출발 때 고정함)\n\n");
            s.push_str(i);
        }
        s
    }

    pub(super) fn write_roots(&self) -> Vec<PathBuf> {
        self.frozen.write_roots.get().map(|v| v.iter().map(PathBuf::from).collect()).unwrap_or_default()
    }

    /// 허용 도구 목록. 고정 설정의 `allowed_tools` 가 있으면 환경과 상관없이 그대로 쓴다(항상 이긴다).
    /// 없을 때의 기본값은 환경이 정한다 — 고정 설정 `exec_env.sandbox` 가 true(작동하는 샌드박스가 있는 WSL)면 원본 기본 목록 그대로,
    /// 아니면(네이티브 Windows, 값 없음 포함) 같은 목록에서 맨 `Bash` 만 뺀다. 샌드박스가 없으면 맨 `Bash` 는 홈 어디든 쓰고 읽을 수 있어
    /// 기본으로 주지 않는다. 네이티브에서 셸이 필요하면 고정 설정이 `Bash(npm test *)` 꼴 규칙을 명시해야 하고, 그 규칙은 그대로 넘어간다
    fn allowed_tools_base(&self) -> Vec<String> {
        if let Some(v) = self.frozen.extra.get("allowed_tools") {
            return strs(Some(v));
        }
        let sandboxed = self.frozen.extra.get("exec_env").and_then(|e| e.get("sandbox")).and_then(Value::as_bool).unwrap_or(false);
        DEFAULT_ALLOWED.iter().filter(|t| sandboxed || **t != "Bash").map(|t| t.to_string()).collect()
    }

    /// 모델 세션이 할 수 있는 일의 범위. 실행 폴더의 상태 파일·고정 사본·다른 단위·엔진 전용 폴더는 쓰기 거부, 엔진 전용 폴더는 읽기도 거부
    fn session_policy(&self, n: usize) -> SessionPolicy {
        let unit_dir = self.unit_dir(n);
        let mut allow_write = vec![PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default())];
        allow_write.extend(self.write_roots());
        allow_write.push(unit_dir.clone());

        let mut deny_write: Vec<PathBuf> =
            ["run.json", "queue.json", "control.json", "events.jsonl", "context.jsonl", "starts.log", ".engine.lock", "pinned"]
                .iter()
                .map(|f| self.path(f))
                .collect();
        deny_write.extend(["engine.json", "stream.jsonl", "stderr.log", "evidence"].iter().map(|f| unit_dir.join(f)));
        deny_write.extend((1..=self.queue.len()).filter(|m| *m != n).map(|m| self.unit_dir(m)));
        deny_write.push(self.cfg.private_dir.clone());

        let mut deny_read = vec![self.cfg.private_dir.clone()];
        deny_read.extend(self.frozen.read_deny.get().map(|v| v.iter().map(PathBuf::from).collect::<Vec<_>>()).unwrap_or_default());

        // 늘 거는 거부 목록: 쓰기 범위(작업 폴더 + write_roots)마다 하네스 자리, 사용자 설정·로그인 정보·시작 프로그램, git push.
        // 사용자 영역의 절대 경로는 네이티브 Windows 만 — WSL 은 홈이 리눅스 쪽이라 `~/` 꼴 규칙이 맡는다
        let mut roots = vec![PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default())];
        roots.extend(self.write_roots());
        let native = self.p.env.choice().kind != EnvKind::Wsl;
        let user = native.then(guard::UserArea::from_env);
        let always = guard::build(&roots, user.as_ref());
        for p in always.deny_write {
            if !deny_write.contains(&p) {
                deny_write.push(p);
            }
        }
        for p in always.deny_read {
            if !deny_read.contains(&p) {
                deny_read.push(p);
            }
        }
        let mut disallowed_tools = strs(self.frozen.extra.get("disallowed_tools"));
        for t in always.disallowed_tools {
            if !disallowed_tools.contains(&t) {
                disallowed_tools.push(t);
            }
        }

        SessionPolicy {
            allow_write,
            deny_write,
            deny_read,
            network_allow: strs(self.frozen.extra.get("network_allow")),
            local_binding: self.frozen.extra.get("local_binding").and_then(Value::as_bool).unwrap_or(false),
            // 파일 쓰기 도구는 쓰기 범위마다 경로를 붙인 규칙으로만 허용한다
            allowed_tools: self
                .allowed_tools_base()
                .into_iter()
                .filter(|t| !FILE_WRITE_TOOLS.contains(&t.as_str()))
                .collect(),
            disallowed_tools,
        }
    }

    // ── 걸음 ──

    /// 도는 claude 가 없는데 루프가 Claude 를 기다리는 길 — 오류로 끝난 것으로 넘긴다
    fn lost_claude(&mut self) {
        let now = self.now();
        let outcome = ClaudeOutcome {
            session_started: false,
            result_seen: false,
            last_error: false,
            limit: false,
            limit_resets: None,
            budget: false,
            err_text: String::new(),
            exit_code: None,
            stderr_tail: "도는 claude 가 없습니다.".to_string(),
        };
        self.dispatch(Event::ClaudeExited { now, outcome });
    }

    /// 줄 하나를 읽는다(`cfg.poll` 까지 기다림)
    fn pump_line(&mut self) -> Pump {
        let poll = self.cfg.poll;
        let Some(child) = self.child.as_mut() else { return Pump::Eof };
        match child.read_line(poll) {
            Ok(Some(line)) => {
                self.handle_line(&line);
                Pump::Line
            }
            Ok(None) => {
                if let Some(t) = self.turn.as_mut() {
                    t.at_eof = true;
                }
                Pump::Eof
            }
            Err(e) if e.kind == PortErrorKind::Timeout => Pump::Idle,
            Err(e) => {
                self.warn("claude 출력 읽기", &e);
                if let Some(t) = self.turn.as_mut() {
                    t.at_eof = true;
                }
                Pump::Eof
            }
        }
    }

    fn poll_exit(&mut self) -> Option<i32> {
        if let Some(c) = self.turn.as_ref().and_then(|t| t.exit) {
            return Some(c);
        }
        let code = self.child.as_mut().and_then(|c| c.try_wait().ok().flatten())?;
        if let Some(t) = self.turn.as_mut() {
            t.exit = Some(code);
        }
        Some(code)
    }

    /// 루프가 부르는 한 걸음
    pub(super) fn turn_step(&mut self) {
        if self.turn.is_none() || self.child.is_none() {
            return self.lost_claude();
        }
        let got = matches!(self.pump_line(), Pump::Line);
        let exited = self.poll_exit().is_some();
        let now = self.now();
        let (n, at_eof, bg_cleared) = match self.turn.as_ref() {
            Some(t) => (t.n, t.at_eof, t.bg_cleared),
            None => return,
        };
        let bg_quiet = self.frozen.extra.get("background_quiet_sec").and_then(jnum).unwrap_or(300.0);
        let progress = match self.stream.as_ref() {
            Some(s) => turn_progress(s, now, bg_quiet, bg_cleared, exited, at_eof),
            None => TurnProgress::Done,
        };
        match progress {
            TurnProgress::Done => return self.finish_turn(),
            TurnProgress::ClearBackground => {
                // Windows 는 도구 프로세스 묶음을 따로 끝내지 못한다(Job 째로는 claude 도 끝난다) — 입력을 닫아 claude 가 스스로 끝나게 한다
                self.event(
                    Who::Board,
                    &format!("결과를 보낸 뒤 백그라운드 작업만 남아 {} 동안 새 출력이 없어, 입력을 닫아 판정으로 넘어갑니다.", dur(bg_quiet)),
                );
                if let Some(c) = self.child.as_mut() {
                    c.close_stdin();
                }
                if let Some(t) = self.turn.as_mut() {
                    t.bg_cleared = Some(now);
                }
            }
            TurnProgress::Continue => {}
        }
        let (last_sec, last_save) = match self.turn.as_ref() {
            Some(t) => (t.last_sec, t.last_save),
            None => return,
        };
        if now - last_sec >= 1.0 {
            if let Some(t) = self.turn.as_mut() {
                t.last_sec = now;
            }
            self.turn_second(now);
            if self.turn.is_none() {
                return;
            }
        }
        if now - last_save >= 5.0 {
            if let Some(t) = self.turn.as_mut() {
                t.last_save = now;
            }
            self.save_unit(n);
            self.save_run();
        }
        if at_eof && !exited && !got {
            // stdout 은 닫혔는데 프로세스가 아직 안 끝났다 — 헛돌지 않게 한 템포 쉰다
            self.p.clock.sleep(self.cfg.poll);
        }
    }

    /// 1초마다: 단위 시계·비용·주간 %p 를 고치고 Tick 을 넣은 뒤 control 을 본다
    fn turn_second(&mut self, now: f64) {
        let (n, last_tick) = match self.turn.as_ref() {
            Some(t) => (t.n, t.last_tick),
            None => return,
        };
        let (cost, limit) = self.stream.as_ref().map_or((0.0, false), |s| (s.cost, s.limit));
        let meter = self.account(n, last_tick, now, cost);
        if let Some(t) = self.turn.as_mut() {
            t.last_tick = now;
        }
        self.dispatch(Event::Tick { now, meter, limit_seen: limit });
        if self.turn.is_none() {
            return;
        }
        self.poll_control();
    }

    // ── 줄 ──

    fn handle_line(&mut self, line: &str) {
        let Some(n) = self.turn.as_ref().map(|t| t.n) else { return };
        let now = self.now();
        if let Err(e) = self.p.files.append_line(&self.unit_dir(n).join("stream.jsonl"), line) {
            self.warn("stream.jsonl", &e);
        }
        let ev = parse_line(line);
        let Some(stream) = self.stream.as_mut() else { return };
        let emits = stream.feed(&ev, now);
        // 도구 호출은 context.jsonl 에 이미 남겼다 — 쌓아 두지 않는다
        let _ = stream.take_tool_uses();
        let turns = stream.turns_total;
        let usage = stream.usage.clone();
        for em in emits {
            self.emit(n, em);
        }
        let Some(t) = self.turn.as_mut() else { return };
        let delta = turns - t.turns_seen;
        t.turns_seen = turns;
        let usage_changed = usage != t.usage_seen;
        t.usage_seen.clone_from(&usage);
        if delta != 0 {
            let e = self.unit_mut(n);
            let total = slot_f(&e.turns) + delta as f64;
            e.turns.set(Num::from_f64(total));
        }
        if let (true, Some(u)) = (usage_changed, usage) {
            // 이미 run.json 에 있는 칸은 두고 stream 이 본 창만 바꾼다
            let mut merged = self.run.usage.get().and_then(Value::as_object).cloned().unwrap_or_default();
            let mut put = |k: &str, v: Option<f64>| {
                if let Some(x) = v {
                    merged.insert(k.to_string(), Num::from_f64(x).raw().clone());
                }
            };
            put("five_hour", u.five_hour);
            put("five_hour_resets", u.five_hour_resets);
            put("seven_day", u.seven_day);
            put("seven_day_resets", u.seven_day_resets);
            merged.insert("at".into(), Num::r3(u.at).raw().clone());
            merged.insert("from".into(), Value::from("stream"));
            self.run.usage.set(Value::Object(merged));
            self.save_run();
        }
    }

    fn emit(&mut self, n: usize, em: Emit) {
        match em {
            Emit::ModelText(s) => self.event(Who::Model, &s),
            Emit::Looked(k) => self.event(Who::Model, &format!("파일 {k}곳을 살펴봤습니다.")),
            Emit::Note(s) => self.context(n, vec![("kind", "note".into()), ("text", s.into())]),
            Emit::ToolUse(t) => self.context(
                n,
                vec![
                    ("agent", Value::Bool(!t.main)),
                    ("id", t.id.into()),
                    ("input", tool_input_summary(&t.input).into()),
                    ("kind", "tool_use".into()),
                    ("tool", t.name.into()),
                ],
            ),
            Emit::ToolResult { tool_use_id, is_error, output } => self.context(
                n,
                vec![
                    ("id", tool_use_id.into()),
                    ("is_error", Value::Bool(is_error)),
                    ("kind", "tool_result".into()),
                    ("output", output.into()),
                ],
            ),
            Emit::Denial(d) => {
                let what = format!("{} {}", d.tool_name, cut(&tool_input_summary(&d.tool_input), 80));
                let ts = self.now();
                self.event(Who::Board, &format!("허용 범위 밖이라 거부했습니다: {what}"));
                let e = self.unit_mut(n);
                let mut list = e.denials.get().and_then(Value::as_array).cloned().unwrap_or_default();
                list.push(json!({"tool": d.tool_name, "what": what, "ts": Num::r3(ts).raw()}));
                e.denials.set(Value::Array(list));
            }
            Emit::SessionStarted => {
                self.unit_mut(n).session_started.set(true);
                self.save_unit(n);
                let sid = self.stream_sid.clone();
                let mut list = self.run.sessions.get().cloned().unwrap_or_default();
                if !list.contains(&sid) {
                    list.push(sid);
                    self.run.sessions.set(list);
                    self.save_run();
                }
            }
        }
    }

    // ── 끝맺음 ──

    /// 프로세스가 끝나거나 `limit` 초가 지날 때까지 줄을 읽으며 기다린다. 끝나면 종료 코드
    ///
    /// `close_on_result` 면 이 턴의 `result` 줄을 읽는 순간 입력을 한 번 닫는다. 진짜 claude 는 interrupt 에 `result`(error_during_execution)로
    /// 답하고도 다음 입력을 기다리며 살아 있어서, 입력을 닫지 않으면 20초를 꼬박 기다렸다가 자손째 끝내게 된다
    fn wait_exit(&mut self, limit: f64, close_on_result: bool) -> Option<i32> {
        let start = self.now();
        let mut closed = false;
        loop {
            if let Some(c) = self.poll_exit() {
                return Some(c);
            }
            if close_on_result && !closed && self.stream.as_ref().is_some_and(|s| s.result_seen) {
                closed = true;
                if let Some(c) = self.child.as_mut() {
                    c.close_stdin();
                }
            }
            if self.now() - start >= limit {
                return None;
            }
            if matches!(self.pump_line(), Pump::Eof) {
                self.p.clock.sleep(self.cfg.poll);
            }
        }
    }

    /// 턴이 끝났다: 입력을 닫고 10초 기다린 뒤 안 끝났으면 자손까지 끝낸다
    fn finish_turn(&mut self) {
        if let Some(c) = self.child.as_mut() {
            c.close_stdin();
        }
        let code = self.wait_exit(10.0, false).or_else(|| self.kill_child());
        self.end_turn(code);
    }

    fn kill_child(&mut self) -> Option<i32> {
        let child = self.child.as_mut()?;
        if let Err(e) = child.kill_tree() {
            self.warn("claude 끝내기", &e);
        }
        self.poll_exit()
    }

    /// `Interrupt{Claude}` 수행: 점잖게 멈추고(`result` 가 오면 입력을 닫는다) 20초 안에 안 끝나면 자손까지 끝낸 뒤 `ClaudeExited` 를 넣는다
    pub(super) fn interrupt_claude(&mut self) {
        if self.turn.is_none() || self.child.is_none() {
            return self.lost_claude();
        }
        self.event(Who::Board, "claude 를 멈춥니다.");
        if let Some(c) = self.child.as_mut() {
            if let Err(e) = c.interrupt() {
                self.warn("claude 멈추기", &e);
            }
        }
        let code = self.wait_exit(20.0, true).or_else(|| self.kill_child());
        self.end_turn(code);
    }

    /// 남은 줄을 읽고, 비용·procs 를 정리해 저장하고, `ClaudeExited` 를 넣는다
    fn end_turn(&mut self, code: Option<i32>) {
        while matches!(self.pump_line(), Pump::Line) {}
        let now = self.now();
        let (n, last_tick, stderr_start) = match self.turn.as_ref() {
            Some(t) => (t.n, t.last_tick, t.stderr_start),
            None => return,
        };
        let cost = self.stream.as_ref().map_or(0.0, |s| s.cost);
        self.account(n, last_tick, now, cost);
        {
            let e = self.unit_mut(n);
            let done = slot_f(&e.cost_done) + cost;
            e.cost_done.set(Num::r3(done));
            e.cost_usd.set(Num::r3(done));
            e.procs.set(json!({}));
        }
        self.save_unit(n);
        self.save_run();
        let looked = self.stream.as_mut().map_or(0, StreamState::take_looked);
        if looked > 0 {
            self.event(Who::Model, &format!("파일 {looked}곳을 살펴봤습니다."));
        }
        let tail = match self.p.files.read_from(&self.unit_dir(n).join("stderr.log"), stderr_start, 4000) {
            Ok(Some(b)) => String::from_utf8_lossy(&b).trim().to_string(),
            _ => String::new(),
        };
        let outcome = match self.stream.as_ref() {
            Some(s) => ClaudeOutcome::from_stream(s, code, &tail),
            None => return,
        };
        self.child = None;
        self.turn = None;
        let now = self.now();
        self.dispatch(Event::ClaudeExited { now, outcome });
    }
}
