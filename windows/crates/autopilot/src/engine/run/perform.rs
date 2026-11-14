//! `Action` 수행 — 쓰는 차례가 곧 계약이다(설계 5장). 단위 열기·닫기, 판정 돌리기, 끝 정리(`Finish`)를 맡는다.
//! 판정·고정 단계·시험 환경 확인은 `exec` 가 막는 호출(`ExecEnv::run_check`)을 옆 스레드에서 돌리고,
//! 본 스레드는 `cfg.tick` 마다 Tick(시간 상한)과 control 을 본다. 받은 일시정지·중단은 그 일이 끝난 뒤에 적용된다(포트에 판정 끊기가 없다).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::prompt::{first_message, is_under, PromptUnit};
use super::{dur, jnum, r3v, slot_f, usd, Engine};
use crate::engine::decide::{
    Action, ActionVerdict, CheckVerdict, Event, Inflight, RunState, Target, UnitStatus, Who,
};
use crate::model::{Num, UnitProgress};
use crate::ports::{CheckOutcome, CheckRun, PortError, PortErrorKind};
use crate::rules::checks::output_tail;
use crate::rules::control::RestartPlan;

/// 루프가 다음 걸음에 돌릴 막는 일
pub(super) enum Job {
    Check { n: usize, attempt: i64 },
    Precheck { n: usize },
    Action { n: usize },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Check,
    Precheck,
    Action,
}

/// 시험 환경 확인의 시간 상한(초)
const PRECHECK_TIMEOUT_SEC: u64 = 120;

impl<'p> Engine<'p> {
    pub(super) fn perform(&mut self, a: Action) {
        match a {
            Action::SetState { state, reason, wait_until } => self.write_state(state, &reason, wait_until),
            Action::Log { who, text } => self.event(who, &text),
            Action::BeginUnit { n } => self.begin_unit(n),
            Action::SaveUnit { n } => self.save_unit_from_current(n),
            Action::StartClaude { n, resume, next } => self.start_claude(n, resume, next),
            Action::RunCheck { n, attempt } => self.job = Some(Job::Check { n, attempt }),
            Action::RunPrecheck { n } => self.job = Some(Job::Precheck { n }),
            Action::RunAction { n } => self.job = Some(Job::Action { n }),
            Action::Interrupt { target } => match target {
                Target::Claude => self.interrupt_claude(),
                Target::Check | Target::Action | Target::Precheck => {
                    // 도는 판정은 끊을 수 없다 — 끝난 뒤에 적용된다
                    if !self.interrupt_logged {
                        self.interrupt_logged = true;
                        self.event(Who::Board, "판정이 끝나면 멈춥니다.");
                    }
                }
            },
            Action::WaitUntil { until } => self.wait_until = Some(until),
            Action::EndUnit { n, status, reason } => self.end_unit(n, status, &reason),
            Action::SkipUnit { n, why } => self.skip_unit(n, &why),
            Action::KillLeftovers { n } => self.kill_leftovers(n),
            Action::MarkRestart { plan } => self.mark_restart(plan),
            Action::MarkRestartStopped => self.mark_restart_stopped(),
            Action::Finish { state, reason } => self.finish(state, &reason),
        }
    }

    // ── 경로 도우미 ──

    /// 환경 쪽 경로 글(네이티브는 그대로, WSL 은 바꾼 글). 바꾸지 못하면 Windows 글 그대로
    fn env_path(&self, p: &Path) -> String {
        self.p.env.to_env_path(p).unwrap_or_else(|_| p.to_string_lossy().into_owned())
    }

    /// 단위 작업 폴더가 속한 저장소의 작업 가지(run.json `branches.repos.<저장소>.work`)
    fn work_branch(&self, n: usize) -> Option<String> {
        let cwd = self.def(n).cwd.get()?;
        let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
        let repos = self.run.extra.get("branches")?.get("repos")?.as_object()?;
        repos
            .iter()
            .find(|(repo, _)| norm(cwd) == norm(repo) || is_under(cwd, repo))
            .and_then(|(_, info)| info.get("work"))
            .and_then(Value::as_str)
            .filter(|b| !b.is_empty())
            .map(str::to_string)
    }

    fn first_message_for(&self, n: usize) -> String {
        let def = self.def(n);
        let cwd = self.env_path(Path::new(def.cwd.get().map_or("", String::as_str)));
        let unit_dir = self.env_path(&self.unit_dir(n));
        let roots: Vec<String> = self.write_roots().iter().map(|r| self.env_path(r)).collect();
        let branch = self.work_branch(n);
        let sandbox = self.run.exec_env.get().and_then(|e| e.sandbox).unwrap_or(false);
        let plan = &self.state.units[n - 1];
        first_message(&PromptUnit {
            prompt: def.prompt.get().map_or("", String::as_str),
            title: &plan.title,
            cwd: &cwd,
            check: &plan.check,
            unit_dir: &unit_dir,
            write_roots: &roots,
            work_branch: branch.as_deref(),
            sandbox,
            answers: None,
        })
    }

    fn caps_json(&self, n: usize) -> Value {
        let c = &self.state.units[n - 1].caps;
        let num = |x: f64| Num::from_f64(x).raw().clone();
        json!({"minutes": num(c.minutes), "usd": num(c.usd), "weekly_pp": num(c.weekly_pp)})
    }

    fn set_queue_status(&mut self, n: usize, status: UnitStatus) {
        self.queue[n - 1].status.set(status.as_str().to_string());
        self.save_queue();
    }

    // ── 단위 ──

    fn begin_unit(&mut self, n: usize) {
        if let Err(e) = self.p.files.make_private_dir(&self.unit_dir(n)) {
            self.warn("단위 폴더", &e);
        }
        let now = self.now();
        let first = self.first_message_for(n);
        let phase = self.state.current.as_ref().map_or("model", |c| c.phase.as_str());
        let seven_day = self.run.usage.get().and_then(|u| u.get("seven_day")).and_then(jnum);
        let zero = || Num::from_f64(0.0);
        let mut e = UnitProgress::default();
        e.unit.set(n as u32);
        e.id.set(self.def(n).id.get().cloned().unwrap_or_else(|| format!("u{n}")));
        e.title.set(self.state.units[n - 1].title.clone());
        e.status.set("running".to_string());
        e.phase.set(phase.to_string());
        e.started.set(Num::r3(now));
        e.active_sec.set(zero());
        e.cost_usd.set(zero());
        e.cost_done.set(zero());
        e.turns.set(zero());
        e.caps.set(self.caps_json(n));
        e.r#try.set(zero());
        e.errors.set(zero());
        e.checks.set(json!([]));
        e.sessions.set(json!([]));
        e.denials.set(json!([]));
        e.first_message.set(first.clone());
        e.next_message.set(first);
        e.directions.set(Value::from(""));
        match seven_day {
            Some(x) => e.weekly_base.set(Num::from_f64(x)),
            None => e.weekly_base.set_null(),
        }
        self.unit = Some((n, e.into()));
        self.weekly = crate::engine::decide::WeeklyMeter { base: seven_day, ..Default::default() };
        self.stream = None;
        self.stream_sid.clear();
        self.job = None;
        self.save_unit(n);
        self.set_queue_status(n, UnitStatus::Running);
        let caps = self.caps_json(n);
        self.run.unit.set(n as u32);
        self.run.unit_caps.set(caps);
        self.save_run();
    }

    /// 판단 상태(`state.current`)의 칸을 engine.json 에 적는다. 같은 묶음에서 단위가 이미 끝났으면(current 가 없음) 쓸 것이 없다
    fn save_unit_from_current(&mut self, n: usize) {
        let Some(c) = self.state.current.clone().filter(|c| c.n == n) else { return };
        let e = self.unit_mut(n);
        e.phase.set(c.phase.as_str().to_string());
        e.r#try.set(Num::from_f64(c.tries as f64));
        e.errors.set(Num::from_f64(c.errors as f64));
        e.limits.set(Value::from(c.limits));
        let first = e.first_message.get().cloned().unwrap_or_default();
        e.next_message.set(super::next_text(&c.next, &first));
        match c.wait_until {
            Some(t) => e.wait_until.set(Num::r3(t)),
            None if !e.wait_until.is_absent() => e.wait_until.set_null(),
            None => {}
        }
        e.session_started.set(c.resume);
        self.save_unit(n);
    }

    fn end_unit(&mut self, n: usize, status: UnitStatus, reason: &str) {
        if let Some(mut c) = self.child.take() {
            // 단위가 끝났는데 claude 가 남아 있으면 안 된다
            let _ = c.kill_tree();
            self.turn = None;
        }
        let now = self.now();
        self.ensure_unit(n);
        let git = (status == UnitStatus::Capped).then(|| self.git_snapshot(n));
        let result_path = self.unit_dir(n).join("result.json");
        let big = self.oversize(&result_path);
        if let Some(len) = big {
            self.event(
                Who::Board,
                &format!("결과 기록(result.json)이 {len} 바이트로 너무 커서(한도 {}) 읽지 않고 결과가 없는 것으로 봅니다.", super::MODEL_FILE_MAX),
            );
        }
        let result_ok = big.is_none() && matches!(self.p.files.read_json(&result_path), Ok(Some(Value::Object(_))));
        let branch = self.def(n).branch.get().cloned();
        let (active, cost) = {
            let e = self.unit_mut(n);
            if let Some(g) = git {
                e.git.set(g);
            }
            e.status.set(status.as_str().to_string());
            e.phase.set("end".to_string());
            e.ended.set(Num::r3(now));
            e.reason.set(reason.to_string());
            e.result_ok.set(result_ok);
            if let Some(b) = branch {
                e.branch.set(b);
            }
            (slot_f(&e.active_sec), slot_f(&e.cost_usd))
        };
        self.save_unit(n);
        self.set_queue_status(n, status);
        let total = self.queue.len();
        self.event(
            Who::Board,
            &format!("단위 {n}/{total} {}: {reason} ({}, {} USD)", status.label(), dur(active), usd(cost)),
        );
        if !result_ok && status != UnitStatus::Stopped {
            self.event(Who::Board, "모델이 결과 기록(result.json)을 남기지 않았습니다.");
        }
        self.unit = None;
        self.stream = None;
        self.job = None;
        self.record(&format!("단위 {n} 판정 뒤"));
    }

    /// 상한에 닿은 단위가 남긴 변경: 작업 폴더 저장소의 `git status`·`git diff --stat`. 저장소가 아니면 까닭만
    fn git_snapshot(&self, n: usize) -> Value {
        let cwd = PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default());
        let status = match self.p.git.run(&cwd, &["status", "--porcelain", "--ignore-submodules=all"]) {
            Ok(o) if o.code == 0 => o.stdout,
            Ok(_) => return json!({"note": "작업 폴더가 git 저장소가 아니라 git 기록이 없습니다"}),
            Err(e) => return json!({"note": format!("git 을 돌리지 못해 git 기록이 없습니다: {}", e.message)}),
        };
        let diff_stat = match self.p.git.run(&cwd, &["diff", "--stat", "--ignore-submodules=all"]) {
            Ok(o) if o.code == 0 => o.stdout,
            _ => String::new(),
        };
        json!({"status": status, "diff_stat": diff_stat})
    }

    fn skip_unit(&mut self, n: usize, why: &str) {
        if let Err(e) = self.p.files.make_private_dir(&self.unit_dir(n)) {
            self.warn("단위 폴더", &e);
        }
        let now = self.now();
        let mut e = UnitProgress::default();
        e.unit.set(n as u32);
        e.id.set(self.def(n).id.get().cloned().unwrap_or_else(|| format!("u{n}")));
        e.title.set(self.state.units[n - 1].title.clone());
        e.status.set("skipped".to_string());
        e.phase.set("end".to_string());
        e.reason.set(why.to_string());
        e.started.set(Num::r3(now));
        e.ended.set(Num::r3(now));
        e.active_sec.set(Num::from_f64(0.0));
        e.cost_usd.set(Num::from_f64(0.0));
        e.turns.set(Num::from_f64(0.0));
        e.checks.set(json!([]));
        if let Some(b) = self.def(n).branch.get() {
            e.branch.set(b.clone());
        }
        self.unit = Some((n, e.into()));
        self.save_unit(n);
        self.set_queue_status(n, UnitStatus::Skipped);
        let total = self.queue.len();
        self.event(Who::Board, &format!("단위 {n}/{total} 건너뜀: {why}"));
        self.unit = None;
        self.record(&format!("단위 {n} 건너뜀"));
    }

    /// 직전 엔진이 남긴 claude 를 끝낸다(한 세션을 두 프로세스가 함께 쓰지 않게)
    fn kill_leftovers(&mut self, n: usize) {
        let known = self.unit.as_ref().is_some_and(|(m, _)| *m == n)
            || matches!(self.read_capped(&self.unit_state_path(n)), Ok(Some(_)));
        if !known {
            return;
        }
        let (pid, ctime) = {
            let e = self.unit_mut(n);
            let procs = e.procs.get().and_then(Value::as_object);
            let pid = procs.and_then(|p| p.get("claude_pid")).and_then(jnum).map(|x| x as u32);
            let ctime = procs.and_then(|p| p.get("ctime")).and_then(jnum);
            (pid, ctime)
        };
        if let Some(pid) = pid.filter(|p| *p > 1) {
            (self.p.reap)(pid, ctime);
        }
        self.unit_mut(n).procs.set(json!({}));
        self.save_unit(n);
    }

    fn mark_restart(&mut self, plan: RestartPlan) {
        let now = self.now();
        let (unit, kind) = match plan {
            RestartPlan::AfterUnit(n) => (Value::from(n), "after_unit"),
            RestartPlan::Now => (Value::Null, "now"),
            RestartPlan::Already => return,
        };
        let seen = self.run.control_seen.get().map_or(Value::Null, |s| s.raw().clone());
        let mut m = Map::new();
        m.insert("asked".into(), r3v(now));
        m.insert("ts".into(), seen);
        m.insert("unit".into(), unit);
        m.insert("plan".into(), Value::from(kind));
        self.run.extra.insert("restart".into(), Value::Object(m));
        self.save_run();
    }

    fn mark_restart_stopped(&mut self) {
        let now = self.now();
        let entry = self.run.extra.entry("restart").or_insert_with(|| Value::Object(Map::new()));
        if let Some(m) = entry.as_object_mut() {
            m.insert("stopped".into(), r3v(now));
        }
        self.save_run();
    }

    // ── 끝 ──

    /// ① ended ② 상태 ③ 기록 ④ 원래 작업 트리 되돌리기 ⑤ wrapped ⑥ 작업 스케줄러 해제 ⑦ 루프 끝
    fn finish(&mut self, state: RunState, reason: &str) {
        let now = self.now();
        if self.run.ended.get().is_none() {
            self.run.ended.set(Num::r3(now));
        }
        self.write_state(state, reason, None);
        self.record("끝");
        if self.restore_branches() {
            self.record("되돌린 뒤");
        }
        let wrapped = self.now();
        self.run.wrapped.set(Num::r3(wrapped));
        self.save_run();
        self.unregister();
        self.finished = true;
    }

    fn git_line(&self, repo: &Path, args: &[&str]) -> Result<Option<String>, PortError> {
        let o = self.p.git.run(repo, args)?;
        let text = o.stdout.trim().to_string();
        Ok((o.code == 0 && !text.is_empty()).then_some(text))
    }

    fn dirty_lines(&self, repo: &Path) -> Result<Vec<String>, String> {
        match self.p.git.run(repo, &["status", "--porcelain"]) {
            Ok(o) if o.code == 0 => Ok(o.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()),
            Ok(o) => Err(format!("git status 가 실패했습니다(종료 코드 {})", o.code)),
            Err(e) => Err(format!("git 을 돌리지 못했습니다: {}", e.message)),
        }
    }

    /// 출발 때 가지로 되돌린다: 깨끗하고 지금 가지가 작업 가지일 때만. 저장소마다 `restored`·`restore_note` 를 run.json `branches` 에 남긴다.
    /// 이미 끝난(restored 가 true/false) 저장소는 건너뛴다. 무엇이든 바꿨으면 true
    fn restore_branches(&mut self) -> bool {
        let Some(mut branches) = self.run.extra.get("branches").cloned() else { return false };
        let Some(repos) = branches.get_mut("repos").and_then(Value::as_object_mut) else { return false };
        let mut changed = false;
        let mut logs: Vec<String> = Vec::new();
        for (repo, info) in repos.iter_mut() {
            let Some(info) = info.as_object_mut() else { continue };
            if info.get("restored").is_some_and(Value::is_boolean) {
                continue;
            }
            if info.contains_key("work_tree") {
                // 따로 작업 트리: 원래 작업 트리·가지를 건드리지 않았으니 되돌릴 것이 없다
                info.insert("restored".into(), Value::Null);
                info.insert("restore_note".into(), Value::from("따로 작업 트리라 원래 작업 트리·가지를 건드리지 않았습니다"));
                continue;
            }
            let path = Path::new(repo.as_str());
            let original = info.get("original").and_then(Value::as_str).map(str::to_string);
            let on = info.get("on").or_else(|| info.get("work")).and_then(Value::as_str).unwrap_or("").to_string();
            let dirty_start: Vec<String> = info
                .get("dirty_start")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let (ok, note) = match self.git_line(path, &["symbolic-ref", "-q", "--short", "HEAD"]) {
                Err(e) => (false, format!("git 을 돌리지 못해 출발 때 가지로 되돌리지 않았습니다: {}", e.message)),
                Ok(cur) if original.is_some() && cur == original => {
                    (true, format!("이미 출발 때 가지({})에 있습니다", original.as_deref().unwrap_or("")))
                }
                Ok(cur) if cur.clone().unwrap_or_default() != on => (
                    false,
                    format!("실행 중에 가지가 바뀌어(지금 {}) 출발 때 가지로 되돌리지 않았습니다", cur.unwrap_or_else(|| "가지 없음".to_string())),
                ),
                Ok(_) => match self.dirty_lines(path) {
                    Err(why) => (false, format!("{why} 출발 때 가지로 되돌리지 않았습니다")),
                    Ok(now_dirty) if now_dirty != dirty_start => (
                        false,
                        format!(
                            "커밋하지 않은 변경 {}개가 있어 출발 때 가지로 되돌리지 않았습니다(작업 트리는 {on} 에 있습니다)",
                            now_dirty.len()
                        ),
                    ),
                    Ok(_) => {
                        let head = info.get("original_head").and_then(Value::as_str).unwrap_or("HEAD").to_string();
                        let sw = match &original {
                            Some(o) => self.p.git.run(path, &["switch", o]),
                            None => self.p.git.run(path, &["switch", "--detach", &head]),
                        };
                        match sw {
                            Ok(o) if o.code == 0 => (
                                true,
                                format!("원래 작업 트리를 출발 때 가지({})로 되돌렸습니다", original.as_deref().unwrap_or("가지 없는 자리")),
                            ),
                            Ok(o) => {
                                let first = format!("{}{}", o.stderr, o.stdout);
                                let first = first.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                                (false, format!("출발 때 가지로 되돌리지 못했습니다: {first}"))
                            }
                            Err(e) => (false, format!("출발 때 가지로 되돌리지 못했습니다: {}", e.message)),
                        }
                    }
                },
            };
            info.insert("restored".into(), Value::Bool(ok));
            info.insert("restore_note".into(), Value::from(note.clone()));
            if ok {
                info.insert("on".into(), Value::from(original.unwrap_or_default()));
            }
            logs.push(format!("{}: {note}.", claude_board_core::paths::basename(repo)));
            changed = true;
        }
        self.run.extra.insert("branches".into(), branches);
        if changed {
            self.save_run();
        }
        for l in logs {
            self.event(Who::Engine, &l);
        }
        changed
    }

    // ── 판정 돌리기 ──

    /// 루프가 Check·Precheck·Action 을 기다릴 때 한 걸음: 맡아 둔 일을 끝까지 돌리고 결과를 이벤트로 넣는다
    pub(super) fn run_pending_job(&mut self) {
        match self.job.take() {
            Some(Job::Check { n, attempt }) => self.job_check(n, attempt),
            Some(Job::Precheck { n }) => self.job_precheck(n),
            Some(Job::Action { n }) => self.job_action(n),
            None => self.lost_job(),
        }
    }

    /// 맡아 둔 일이 없는데 루프가 기다린다 — 실패로 넘겨 헛돌지 않게 한다
    fn lost_job(&mut self) {
        let now = self.now();
        let tail = "맡아 둔 일이 없습니다.".to_string();
        match self.state.inflight {
            Inflight::Check => {
                self.dispatch(Event::CheckDone { now, verdict: CheckVerdict::Failed { code: -1, tail, timed_out: None } });
            }
            Inflight::Precheck(_) => {
                self.dispatch(Event::PrecheckDone { now, ok: false, code: -1, last_line: tail });
            }
            _ => {
                self.dispatch(Event::ActionDone { now, verdict: ActionVerdict::Failed(tail) });
            }
        }
    }

    fn base_env(&self, n: usize) -> Vec<(String, String)> {
        vec![
            ("CLAUDE_BOARD_AUTOPILOT".to_string(), self.run_id()),
            ("CLAUDE_BOARD_AUTOPILOT_DIR".to_string(), self.cfg.dir.to_string_lossy().into_owned()),
            ("CLAUDE_BOARD_UNIT_DIR".to_string(), self.unit_dir(n).to_string_lossy().into_owned()),
        ]
    }

    fn clear_dir(&self, dir: &Path) {
        match self.p.files.list(dir) {
            Ok(names) => {
                for name in names {
                    if let Err(e) = self.p.files.remove(&dir.join(&name)) {
                        self.warn("증거 폴더 비우기", &e);
                    }
                }
            }
            Err(e) => self.warn("증거 폴더 비우기", &e),
        }
        if let Err(e) = self.p.files.make_private_dir(dir) {
            self.warn("증거 폴더", &e);
        }
    }

    /// 막는 호출을 옆 스레드에서 돌리고 본 스레드는 `cfg.tick` 마다 Tick(판정·고정 단계)과 control 을 본다
    fn exec(&mut self, kind: JobKind, run: CheckRun) -> CheckOutcome {
        self.interrupt_logged = false;
        self.job_last_tick = self.now();
        let env = self.p.env;
        let tick = self.cfg.tick;
        let (tx, rx) = mpsc::channel();
        let res = std::thread::scope(|s| {
            s.spawn(move || {
                let _ = tx.send(env.run_check(&run));
            });
            loop {
                match rx.recv_timeout(tick) {
                    Ok(r) => break r,
                    Err(RecvTimeoutError::Timeout) => self.job_tick(kind),
                    Err(RecvTimeoutError::Disconnected) => {
                        break Err(PortError::new(PortErrorKind::Io, "명령을 돌리는 스레드가 결과 없이 끝났습니다."));
                    }
                }
            }
        });
        match res {
            Ok(o) => CheckOutcome { tail: output_tail(o.tail.as_bytes()), ..o },
            Err(e) => CheckOutcome {
                exit: None,
                timed_out: false,
                seconds: 0.0,
                tail: format!("명령을 돌리지 못했습니다: {}", e.message),
            },
        }
    }

    fn job_tick(&mut self, kind: JobKind) {
        if kind != JobKind::Precheck {
            if let Some(n) = self.state.current.as_ref().map(|c| c.n) {
                let now = self.now();
                let meter = self.account(n, self.job_last_tick, now, 0.0);
                self.job_last_tick = now;
                self.dispatch(Event::Tick { now, meter, limit_seen: false });
            }
        }
        self.poll_control();
    }

    /// 일이 끝났을 때 단위 시계를 마지막으로 더한다
    fn account_job_end(&mut self, n: usize) {
        let now = self.now();
        self.account(n, self.job_last_tick, now, 0.0);
        self.job_last_tick = now;
    }

    fn check_timeout_sec(&self, n: usize) -> f64 {
        let min = self
            .def(n)
            .extra
            .get("check_timeout_min")
            .and_then(jnum)
            .or_else(|| self.frozen.extra.get("check_timeout_min").and_then(jnum))
            .unwrap_or(10.0);
        min * 60.0
    }

    fn job_check(&mut self, n: usize, k: i64) {
        let evidence = self.unit_dir(n).join("evidence").join(format!("check-{k}"));
        self.clear_dir(&evidence);
        self.event(Who::Board, &if k == 0 { "판정 명령을 돌립니다.".to_string() } else { format!("판정 명령을 돌립니다(다시 {k}번째).") });
        let limit = self.check_timeout_sec(n);
        let mut env = self.base_env(n);
        env.push(("CLAUDE_BOARD_EVIDENCE_DIR".to_string(), evidence.to_string_lossy().into_owned()));
        let command = self.state.units[n - 1].check.clone();
        let run = CheckRun {
            command: command.clone(),
            cwd: PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default()),
            env,
            path_env: ["CLAUDE_BOARD_UNIT_DIR", "CLAUDE_BOARD_EVIDENCE_DIR", "CLAUDE_BOARD_AUTOPILOT_DIR"].map(String::from).to_vec(),
            timeout: Duration::from_secs_f64(limit),
            output_log: evidence.join("output.log"),
        };
        let out = self.exec(JobKind::Check, run);
        self.account_job_end(n);
        let now = self.now();
        let code = out.exit.unwrap_or(-1);
        let sec = Num::r3(out.seconds);
        let env_key = self.p.env.choice().key();
        self.put(
            &evidence.join("summary.json"),
            &json!({"k": k, "exit": code, "timed_out": out.timed_out, "sec": sec.raw(), "at": r3v(now), "command": command, "env": env_key}),
        );
        {
            let e = self.unit_mut(n);
            let mut checks = e.checks.get().and_then(Value::as_array).cloned().unwrap_or_default();
            checks.push(json!({
                "try": k, "exit": code, "sec": sec.raw(), "tail": out.tail, "at": r3v(now),
                "interrupted": false, "timed_out": out.timed_out, "evidence": format!("evidence/check-{k}"),
                "files": [], "sandbox": false,
            }));
            e.checks.set(Value::Array(checks));
        }
        self.save_unit(n);
        let passed = out.exit == Some(0) && !out.timed_out;
        let verdict = if passed {
            self.event(Who::Board, &format!("판정 통과({}).", dur(out.seconds)));
            CheckVerdict::Passed
        } else {
            CheckVerdict::Failed { code, tail: out.tail, timed_out: out.timed_out.then_some(limit) }
        };
        let now = self.now();
        self.dispatch(Event::CheckDone { now, verdict });
    }

    fn job_precheck(&mut self, n: usize) {
        let mut env = self.base_env(n);
        env.push(("CLAUDE_BOARD_PRIVATE_DIR".to_string(), self.cfg.private_dir.to_string_lossy().into_owned()));
        let run = CheckRun {
            command: self.def_text(n, "precheck").unwrap_or_default(),
            cwd: PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default()),
            env,
            path_env: ["CLAUDE_BOARD_UNIT_DIR", "CLAUDE_BOARD_AUTOPILOT_DIR", "CLAUDE_BOARD_PRIVATE_DIR"].map(String::from).to_vec(),
            timeout: Duration::from_secs(PRECHECK_TIMEOUT_SEC),
            output_log: self.cfg.private_dir.join(format!("precheck-{n}.log")),
        };
        let out = self.exec(JobKind::Precheck, run);
        let last_line: String = out
            .tail
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(120)
            .collect();
        let now = self.now();
        self.dispatch(Event::PrecheckDone {
            now,
            ok: out.exit == Some(0) && !out.timed_out,
            code: out.exit.unwrap_or(-1),
            last_line,
        });
    }

    fn job_action(&mut self, n: usize) {
        let limit = self
            .def(n)
            .extra
            .get("action_timeout_min")
            .and_then(jnum)
            .map_or(self.state.units[n - 1].caps.minutes * 60.0, |m| m * 60.0);
        let log = self.unit_dir(n).join("action.log");
        let run = CheckRun {
            command: self.def_text(n, "action").unwrap_or_default(),
            cwd: PathBuf::from(self.def(n).cwd.get().cloned().unwrap_or_default()),
            env: self.base_env(n),
            path_env: ["CLAUDE_BOARD_UNIT_DIR", "CLAUDE_BOARD_AUTOPILOT_DIR"].map(String::from).to_vec(),
            timeout: Duration::from_secs_f64(limit),
            output_log: log.clone(),
        };
        let out = self.exec(JobKind::Action, run);
        self.account_job_end(n);
        let code = out.exit.unwrap_or(-1);
        let ok = out.exit == Some(0) && !out.timed_out;
        {
            let e = self.unit_mut(n);
            let sec = Num::r3(out.seconds);
            e.extra.insert("action_result".into(), json!({"exit": code, "sec": sec.raw(), "tail": out.tail}));
            if ok {
                // 모델이 고정 단계가 이미 돌았음을 알도록 첫 입력 끝에 덧붙인다(next_message 도 같은 글로 맞춘다)
                let note = format!(
                    "\n\n---\n엔진이 이 단위를 시작하기 전에 고정 단계(action)를 돌렸습니다: 종료 코드 0. 출력은 `{}` 에 있습니다.\n이 결과를 읽고 판단·기록만 하세요. 고정 단계를 다시 돌리거나 같은 일을 직접 하지 마세요.",
                    log.display()
                );
                let first = format!("{}{note}", e.first_message.get().cloned().unwrap_or_default());
                e.first_message.set(first.clone());
                e.next_message.set(first);
            }
        }
        self.save_unit(n);
        let verdict = if ok { ActionVerdict::Ok } else { ActionVerdict::Failed(format!("고정 단계가 실패했습니다(종료 코드 {code}).")) };
        let now = self.now();
        self.dispatch(Event::ActionDone { now, verdict });
    }
}
