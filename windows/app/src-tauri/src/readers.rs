//! 대화 기록에서 모델·effort(SPEC §5.2)와 컨텍스트 사용량(§5.3), Esc 중단 표시(§3.9)와 Claude Code 세션 파일(§2.3)을 읽는다 — `ClaudeBoard.swift ModelReader`·`InterruptReader`·`ClaudeNames`·`ContextReader`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use claude_board_core::claude_live::{self, ClaudeSession, INTERRUPT_TAIL};
use claude_board_core::{fileio, Record};
use serde_json::Value;

use crate::model::{Brain, Ctx};
use crate::util::{contains, mtime, mtime_key, num, obj, read_tail};

/// 대화 기록 끝에서 마지막 응답의 모델·effort 를 읽는다 — 파일이 바뀌어도 3초에 한 번만 다시 읽는다.
/// (파일 수정 시각, 읽은 시각, 찾은 (모델, effort))
type ModelCache = (Option<SystemTime>, f64, Option<(String, String)>);

#[derive(Default)]
pub struct ModelReader {
    parsed: HashMap<PathBuf, ModelCache>,
}

impl ModelReader {
    pub fn read(&mut self, path: &Path, fallback_model: &str, now: f64) -> Option<Brain> {
        let changed = mtime_key(path);
        let fallback = || (!fallback_model.is_empty()).then(|| Brain::new(fallback_model, ""));
        if let Some((m, checked, found)) = self.parsed.get(path) {
            if *m == changed || now - checked < 3.0 {
                return found.as_ref().map(|(m, e)| Brain::new(m, e)).or_else(fallback);
            }
        }
        let found = last_model(&read_tail(path, 400_000));
        let brain = found.as_ref().map(|(m, e)| Brain::new(m, e));
        self.parsed.insert(path.to_path_buf(), (changed, now, found));
        brain.or_else(fallback)
    }

    /// 목록에서 사라진 기록은 잊는다.
    pub fn prune(&mut self, keep: &std::collections::HashSet<PathBuf>) {
        self.parsed.retain(|k, _| keep.contains(k));
    }
}

/// 뒤에서부터 `type == "assistant"` 이고 `message.model` 이 `<synthetic>` 이 아닌 첫 줄의 (모델, effort).
fn last_model(data: &[u8]) -> Option<(String, String)> {
    for line in data.split(|&b| b == b'\n').rev() {
        if !contains(line, b"\"assistant\"") {
            continue;
        }
        let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(line) else { continue };
        if j.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(model) = obj(j.get("message")).and_then(|m| m.get("model")).and_then(Value::as_str) else { continue };
        if model == "<synthetic>" {
            continue;
        }
        let effort = j.get("effort").and_then(Value::as_str).unwrap_or("");
        return Some((model.to_string(), effort.to_string()));
    }
    None
}

/// 파일 수정 시각과 크기 — Windows 는 수정 시각이 타이머 틱 단위라 같은 틱 안에 다시 쓴 파일이 같은 시각으로 보일 수 있어
/// 크기를 함께 본다(덧붙인 대화 기록 줄이나 고쳐 쓴 세션 파일이 읽다 만 결과로 굳지 않게).
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

/// Esc 로 끊은 응답 — 대화 기록 끝의 마지막 대화 줄이 중단 표시면 그 시각(유닉스 초)을 돌려준다
/// (`InterruptReader`, `ClaudeBoard.swift InterruptReader`). 파일 수정 시각과 크기가 같으면 이전 결과를 쓴다.
#[derive(Default)]
pub struct InterruptReader {
    parsed: HashMap<PathBuf, ((SystemTime, u64), Option<f64>)>,
}

impl InterruptReader {
    /// 파일이 없으면 None(캐시하지 않는다).
    pub fn read(&mut self, path: &Path) -> Option<f64> {
        let changed = stamp(path)?;
        if let Some((m, at)) = self.parsed.get(path) {
            if *m == changed {
                return *at;
            }
        }
        let secs = changed.0.duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let at = claude_live::interrupted_at_in(&read_tail(path, INTERRUPT_TAIL), secs);
        self.parsed.insert(path.to_path_buf(), (changed, at));
        at
    }

    /// 목록에서 사라진 기록은 잊는다.
    pub fn prune(&mut self, keep: &std::collections::HashSet<PathBuf>) {
        self.parsed.retain(|k, _| keep.contains(k));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.parsed.len()
    }
}

/// Claude Code 가 `~/.claude/sessions/<pid>.json` 에 쓰는 세션 이름·status — pid 별로 파일 수정 시각과 크기가 같으면 다시 읽지 않는다
/// (`ClaudeNames`, `ClaudeBoard.swift ClaudeNames`). 파일이 없거나 객체가 아니면 None.
pub struct ClaudeNames {
    dir: PathBuf,
    cache: HashMap<u32, ((SystemTime, u64), Option<ClaudeSession>)>,
}

impl ClaudeNames {
    /// `dir` = `~/.claude`
    pub fn new(dir: PathBuf) -> Self {
        ClaudeNames { dir, cache: HashMap::new() }
    }

    pub fn get(&mut self, pid: u32) -> Option<ClaudeSession> {
        let path = claude_live::claude_session_path(&self.dir, pid);
        let Some(m) = (pid > 0).then(|| stamp(&path)).flatten() else {
            self.cache.remove(&pid);
            return None;
        };
        if let Some((cm, cs)) = self.cache.get(&pid) {
            if *cm == m {
                return cs.clone();
            }
        }
        let cs = claude_live::read_claude_session(&self.dir, pid);
        self.cache.insert(pid, (m, cs.clone()));
        cs
    }

    /// 목록에서 사라진 pid 는 잊는다.
    pub fn prune(&mut self, keep: &std::collections::HashSet<u32>) {
        self.cache.retain(|k, _| keep.contains(k));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.cache.len()
    }
}

/// 세션별 컨텍스트 사용량.
/// 1순위: 상태줄 스크립트가 남긴 `notifier/context/<세션 id>.json`, 2순위: 대화 기록 끝의 마지막 응답 usage.
pub struct ContextReader {
    dir: PathBuf,
    windows_path: PathBuf,
    /// 상태줄에서 배운 모델별 컨텍스트 크기 — 보드를 다시 켜도 쓰도록 파일에 남긴다
    windows: HashMap<String, i64>,
    lines: HashMap<String, (SystemTime, Option<Record>)>,
    parsed: HashMap<PathBuf, (Option<SystemTime>, f64, Option<Ctx>)>,
}

const INPUT_KEYS: [&str; 3] = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"];

fn input_sum(u: Option<&Record>) -> i64 {
    let Some(u) = u else { return 0 };
    INPUT_KEYS.iter().map(|k| num(u.get(*k)).unwrap_or(0.0) as i64).sum()
}

/// 배운 값이 없을 때의 기본 크기 — 5 세대 모델은 1M, 그 밖은 200K.
pub fn default_window(model: &str) -> i64 {
    if model.contains("[1m]") {
        return 1_000_000;
    }
    let one_m = ["claude-opus-5", "claude-sonnet-5", "claude-haiku-5", "claude-fable-5"];
    if one_m.iter().any(|p| model.starts_with(p)) {
        1_000_000
    } else {
        200_000
    }
}

impl ContextReader {
    pub fn new(dir: PathBuf, windows_path: PathBuf) -> Self {
        let windows = fs::read(&windows_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, i64>>(&b).ok())
            .unwrap_or_default();
        ContextReader { dir, windows_path, windows, lines: HashMap::new(), parsed: HashMap::new() }
    }

    fn learn(&mut self, model: &str, size: i64) {
        if self.windows.get(model) == Some(&size) {
            return;
        }
        self.windows.insert(model.to_string(), size);
        if let Ok(bytes) = serde_json::to_vec(&self.windows) {
            let _ = fileio::write_atomic(&self.windows_path, "context-windows", &bytes);
        }
    }

    /// 상태줄 기록 — 파일이 바뀔 때만 다시 읽는다.
    fn status_line(&mut self, sid: &str) -> Option<Record> {
        let path = self.dir.join(format!("{sid}.json"));
        let Some(m) = mtime_key(&path) else {
            self.lines.remove(sid);
            return None;
        };
        if let Some((cm, j)) = self.lines.get(sid) {
            if *cm == m {
                return j.clone();
            }
        }
        let j = fileio::read_record(&path);
        self.lines.insert(sid.to_string(), (m, j.clone()));
        j
    }

    pub fn read(&mut self, sid: &str, transcript: &Path, model: &str, now: f64) -> Option<Ctx> {
        let t_mtime = mtime(transcript).unwrap_or(f64::NEG_INFINITY);
        if let Some(j) = self.status_line(sid) {
            let cw = obj(j.get("context_window"));
            let size = cw.and_then(|c| num(c.get("context_window_size"))).unwrap_or(0.0) as i64;
            if let (Some(cw), true) = (cw, size > 0) {
                if let Some(m) = j.get("model").and_then(Value::as_str).filter(|m| !m.is_empty()) {
                    self.learn(m, size);
                }
                let at = num(j.get("at")).unwrap_or(0.0);
                // 상태줄 기록 뒤에 대화가 더 진행됐으면(백그라운드로 다시 열린 경우 등) 대화 기록 쪽을 쓴다
                if t_mtime - at < 60.0 {
                    let used = input_sum(obj(cw.get("current_usage")));
                    if used > 0 {
                        return Some(Ctx::new(used, size));
                    }
                    if let Some(p) = num(cw.get("used_percentage")) {
                        return Some(Ctx::new(size * (p as i64) / 100, size));
                    }
                }
            }
        }
        self.read_transcript(transcript, model, now)
    }

    /// 상태줄이 남긴 세션의 현재 effort — 컨텍스트와 같은 기준으로 신선할 때만.
    pub fn live_effort(&mut self, sid: &str, transcript: &Path) -> Option<String> {
        let j = self.status_line(sid)?;
        let e = j.get("effort").and_then(Value::as_str).filter(|e| !e.is_empty())?.to_string();
        let t_mtime = mtime(transcript).unwrap_or(f64::NEG_INFINITY);
        let at = num(j.get("at")).unwrap_or(0.0);
        (t_mtime - at < 60.0).then_some(e)
    }

    fn read_transcript(&mut self, path: &Path, model: &str, now: f64) -> Option<Ctx> {
        let key = mtime_key(path);
        if let Some((m, checked, ctx)) = self.parsed.get(path) {
            if *m == key || now - checked < 5.0 {
                return *ctx;
            }
        }
        let data = read_tail(path, 1_500_000);
        let mut used = 0;
        let mut last_model = model.to_string();
        for line in data.split(|&b| b == b'\n').rev() {
            if !contains(line, b"\"assistant\"") {
                continue;
            }
            let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(line) else { continue };
            if j.get("type").and_then(Value::as_str) != Some("assistant") || j.get("isSidechain") == Some(&Value::Bool(true)) {
                continue;
            }
            let Some(msg) = obj(j.get("message")) else { continue };
            let m = msg.get("model").and_then(Value::as_str);
            if m == Some("<synthetic>") {
                continue;
            }
            let Some(u) = obj(msg.get("usage")) else { continue };
            used = input_sum(Some(u));
            if last_model.is_empty() {
                if let Some(m) = m {
                    last_model = m.to_string();
                }
            }
            break;
        }
        let ctx = (used > 0).then(|| {
            let mut size = self.windows.get(&last_model).copied().unwrap_or_else(|| default_window(&last_model));
            if used > size {
                size = 1_000_000;
            }
            Ctx::new(used, size)
        });
        self.parsed.insert(path.to_path_buf(), (key, now, ctx));
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("cb-readers-{}", crate::util::short_id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    const JSONL: &str = concat!(
        r#"{"type":"user","message":{"role":"user","content":"hi"}}"#, "\n",
        r#"{"type":"assistant","effort":"high","message":{"model":"claude-sonnet-5-5","usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}"#, "\n",
        r#"{"type":"assistant","isSidechain":true,"message":{"model":"claude-haiku-4-5","usage":{"input_tokens":99999}}}"#, "\n",
        r#"{"type":"assistant","effort":"xhigh","message":{"model":"claude-opus-5-5","usage":{"input_tokens":1000,"cache_creation_input_tokens":2000,"cache_read_input_tokens":309000}}}"#, "\n",
        r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0}}}"#, "\n",
        r#"{"type":"user","message":{"content":"mentions \"assistant\" text"}}"#, "\n",
    );

    #[test]
    fn model_and_effort_from_transcript() {
        let d = dir();
        let t = d.join("s.jsonl");
        fs::write(&t, JSONL).unwrap();
        let mut r = ModelReader::default();
        let b = r.read(&t, "", 100.0).unwrap();
        assert_eq!((b.model.as_str(), b.effort.as_str(), b.family.as_str(), b.effort_level), ("claude-opus-5-5", "xhigh", "Opus", 4));
        // 없는 파일은 세션 기록의 model 로 대신한다(effort 없음)
        let b = r.read(&d.join("none.jsonl"), "claude-fable-5-1", 100.0).unwrap();
        assert_eq!((b.model.as_str(), b.effort.as_str(), b.version.as_str()), ("claude-fable-5-1", "", "5.1"));
        assert!(r.read(&d.join("none2.jsonl"), "", 100.0).is_none());
        // 3초 안에는 바뀐 파일도 다시 읽지 않는다
        let mut f = fs::OpenOptions::new().append(true).open(&t).unwrap();
        writeln!(f, r#"{{"type":"assistant","effort":"low","message":{{"model":"claude-haiku-5-5"}}}}"#).unwrap();
        // Windows 는 수정 시각이 타이머 틱(약 15ms) 단위라 방금 쓴 파일과 같은 값이 될 수 있다 — 바뀐 것으로 보이게 확실히 옮긴다
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2)).unwrap();
        drop(f);
        assert_eq!(r.read(&t, "", 101.0).unwrap().model, "claude-opus-5-5");
        assert_eq!(r.read(&t, "", 104.0).unwrap().model, "claude-haiku-5-5");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn context_from_transcript_and_status_line() {
        let d = dir();
        let ctx_dir = d.join("context");
        fs::create_dir_all(&ctx_dir).unwrap();
        let windows = d.join("context-windows.json");
        let t = d.join("s.jsonl");
        fs::write(&t, JSONL).unwrap();
        let mut r = ContextReader::new(ctx_dir.clone(), windows.clone());

        // 2순위: 사이드체인·synthetic 을 건너뛴 마지막 응답의 입력 토큰 합, 5 세대 Opus 는 1M
        let c = r.read("s", &t, "", 100.0).unwrap();
        assert_eq!((c.used, c.size, c.pct), (312_000, 1_000_000, 31));
        // 기록의 model 이 있으면 그 이름으로 크기를 고른다(200K 보다 크면 1M)
        let mut r2 = ContextReader::new(ctx_dir.clone(), windows.clone());
        assert_eq!(r2.read("s", &t, "claude-sonnet-4-5", 100.0).unwrap().size, 1_000_000);
        let small = d.join("small.jsonl");
        fs::write(&small, r#"{"type":"assistant","message":{"model":"claude-sonnet-4-5","usage":{"input_tokens":50000}}}"#).unwrap();
        assert_eq!(r2.read("x", &small, "", 100.0).unwrap(), Ctx::new(50_000, 200_000));

        // 1순위: 상태줄 기록 — 크기를 배워 context-windows.json 에 남긴다
        let at = mtime(&t).unwrap();
        fs::write(
            ctx_dir.join("s.json"),
            format!(r#"{{"context_window":{{"context_window_size":400000,"current_usage":{{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":3}},"used_percentage":0}},"model":"claude-opus-5-5","effort":"max","at":{at}}}"#),
        )
        .unwrap();
        assert_eq!(r.read("s", &t, "", 200.0).unwrap(), Ctx::new(6, 400_000));
        assert_eq!(r.live_effort("s", &t).as_deref(), Some("max"));
        let learned: HashMap<String, i64> = serde_json::from_slice(&fs::read(&windows).unwrap()).unwrap();
        assert_eq!(learned["claude-opus-5-5"], 400_000);
        // 배운 크기는 대화 기록 쪽 계산에도 쓴다
        let mut r3 = ContextReader::new(d.join("empty"), windows.clone());
        assert_eq!(r3.read("s", &t, "", 300.0).unwrap().size, 400_000);

        // 상태줄 기록이 60초 넘게 낡았으면 대화 기록 쪽을 쓴다
        fs::write(
            ctx_dir.join("s.json"),
            format!(r#"{{"context_window":{{"context_window_size":400000,"current_usage":{{}},"used_percentage":50}},"effort":"low","at":{}}}"#, at - 120.0),
        )
        .unwrap();
        let mut r4 = ContextReader::new(ctx_dir.clone(), windows.clone());
        assert_eq!(r4.read("s", &t, "", 300.0).unwrap().used, 312_000);
        assert_eq!(r4.live_effort("s", &t), None);
        // 신선하고 토큰이 0 이면 used_percentage 로
        fs::write(
            ctx_dir.join("s.json"),
            format!(r#"{{"context_window":{{"context_window_size":400000,"current_usage":{{}},"used_percentage":50}},"at":{at}}}"#),
        )
        .unwrap();
        // Windows 는 수정 시각이 타이머 틱(약 15ms) 단위라 바로 앞에 쓴 기록과 같은 값이 될 수 있다 — 바뀐 것으로 보이게 확실히 옮긴다
        fs::File::options()
            .append(true)
            .open(ctx_dir.join("s.json"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(r4.read("s", &t, "", 301.0).unwrap(), Ctx::new(200_000, 400_000));
        fs::remove_dir_all(&d).unwrap();
    }
}
