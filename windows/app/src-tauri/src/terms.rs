//! 내장 터미널 백엔드 (SPEC §5.12 터미널 계약) — 터미널 목록과 `term_*` 명령.
//!
//! 터미널마다 스레드 셋이 돈다. 리더는 출력 파이프를 늘 비워 [`Stream`] 고리에 쓰고(웹뷰를 기다리지 않는다),
//! 웨이터는 프로세스가 끝나면 종료 코드를 남기고 가짜 콘솔을 닫는다. 웹뷰로 보내는 배처는 `term_attach` 마다 [`Stream`] 이 띄운다.
//! 명령 등록(`generate_handler!`)과 `TermHub` 상태 등록(`manage`)은 `board.rs` 가 한다.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use claude_board_core::paths;
use claude_board_core::place::BoardTerm;
use claude_board_core::proc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use crate::pty::{self, Pty};
use crate::ring::{Out, Sink, Stream, RING_CAP};

/// `term_kill` 이 가짜 콘솔을 닫은 뒤 프로세스가 끝나길 기다리는 시간. 넘으면 강제 종료한다.
const KILL_GRACE: Duration = Duration::from_secs(6);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSpec {
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermInfo {
    pub id: String,
    pub pid: u32,
    /// `pwsh` · `powershell` · `cmd`
    pub shell: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    /// `running` · `exited`
    pub status: &'static str,
    pub exit_code: Option<u32>,
    pub windows_build: Option<u32>,
    pub oldest_seq: u64,
    pub next_seq: u64,
}

struct Term {
    id: String,
    shell: String,
    cwd: String,
    size: Mutex<(u16, u16)>,
    pty: Arc<Pty>,
    stream: Arc<Stream>,
    /// `term_kill` 이 불렸다 - 끝난 뒤 목록에서 치워도 된다.
    kill_requested: AtomicBool,
}

impl Term {
    /// 끝났고 더 둘 이유가 없다: 죽이라고 했거나, 보던 웹뷰가 없어졌다.
    fn removable(&self) -> bool {
        self.stream.exit_code().is_some() && (self.kill_requested.load(Ordering::SeqCst) || self.stream.viewer_lost())
    }

    fn info(&self) -> TermInfo {
        let (cols, rows) = *lock(&self.size);
        let exit_code = self.stream.exit_code();
        let b = self.stream.bounds();
        TermInfo {
            id: self.id.clone(),
            pid: self.pty.pid(),
            shell: self.shell.clone(),
            cwd: self.cwd.clone(),
            cols,
            rows,
            status: if exit_code.is_some() { "exited" } else { "running" },
            exit_code,
            windows_build: pty::windows_build(),
            oldest_seq: b.oldest,
            next_seq: b.next,
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 터미널 목록. 만든 순서를 지키고 `TermId`("t1", "t2", …)는 프로세스 수명 동안 겹치지 않는다.
#[derive(Default)]
pub struct TermHub {
    terms: Arc<Mutex<Vec<Arc<Term>>>>,
    next_id: AtomicU64,
}

impl TermHub {
    /// 치울 수 있는 터미널을 목록에서 빼고 보내기 스레드를 끝낸다. 몇 번 불러도 같다.
    fn reap(terms: &Mutex<Vec<Arc<Term>>>) {
        lock(terms).retain(|t| {
            let dead = t.removable();
            if dead {
                t.stream.close();
            }
            !dead
        });
    }

    fn get(&self, id: &str) -> Result<Arc<Term>, String> {
        lock(&self.terms).iter().find(|t| t.id == id).cloned().ok_or_else(|| format!("없는 터미널: {id}"))
    }

    /// 기본 셸(pwsh → powershell → ComSpec)을 `spec` 의 크기로 띄운다. 시작 폴더는 `spec.cwd`, 없으면 홈 폴더.
    pub fn spawn(&self, spec: SpawnSpec) -> Result<TermInfo, String> {
        let shell = pty::resolve_shell().ok_or("셸(pwsh·powershell·cmd)을 찾지 못했다")?;
        let cwd = spec.cwd.map(Into::into).filter(|p: &std::path::PathBuf| p.is_dir()).unwrap_or_else(paths::home);
        self.spawn_program(shell.kind, &shell.path, &shell.args, &cwd, spec.cols, spec.rows)
    }

    /// 프로그램 하나를 가짜 콘솔에 띄우고 리더·웨이터를 돌린다.
    fn spawn_program(&self, shell: &str, program: &Path, args: &[String], cwd: &Path, cols: u16, rows: u16) -> Result<TermInfo, String> {
        let (cols, rows) = (cols.max(2), rows.max(1));
        let pty = Arc::new(Pty::spawn(program, args, cwd, cols, rows).map_err(|e| format!("{} 을(를) 띄우지 못했다: {e}", program.display()))?);
        let stream = Arc::new(Stream::new(RING_CAP));
        let id = format!("t{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        spawn_reader(&id, &pty, &stream);
        let term = Arc::new(Term {
            id,
            shell: shell.to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            size: Mutex::new((cols, rows)),
            pty,
            stream,
            kill_requested: AtomicBool::new(false),
        });
        let info = term.info();
        Self::reap(&self.terms);
        lock(&self.terms).push(Arc::clone(&term));
        spawn_waiter(&term, &self.terms);
        Ok(info)
    }

    /// 목록. 끝났고 죽이라고 했거나 보는 쪽이 없어진 터미널은 여기서도 치운다.
    pub fn list(&self) -> Vec<TermInfo> {
        Self::reap(&self.terms);
        lock(&self.terms).iter().map(|t| t.info()).collect()
    }

    /// 도는 터미널들의 짝짓기 자료 — 셸 pid·생성 시각·콘솔 호스트(`pairing` 이 세션 pid 와 맞춘다).
    pub fn hosts(&self) -> Vec<BoardTerm> {
        let all: Vec<Arc<Term>> = lock(&self.terms).clone();
        all.iter()
            .filter(|t| t.stream.exit_code().is_none())
            .map(|t| {
                let pid = t.pty.pid();
                BoardTerm { id: t.id.clone(), shell_pid: pid, shell_ctime: proc::creation_time(pid), host: proc::console_host(pid) }
            })
            .collect()
    }

    /// 보는 쪽을 `sink` 로 바꾸고 `from_seq`(0 이면 고리에 남은 처음)부터 보낸다.
    pub fn attach(&self, id: &str, from_seq: u64, sink: Sink) -> Result<TermInfo, String> {
        let t = self.get(id)?;
        t.stream.attach(from_seq, sink);
        Ok(t.info())
    }

    pub fn ack(&self, id: &str, seq: u64) -> Result<(), String> {
        self.get(id)?.stream.ack(seq);
        Ok(())
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        self.get(id)?.pty.write_all(data).map_err(|e| format!("터미널 입력 실패: {e}"))
    }

    /// 크기를 바꾼다 — 적용했으면 `size` 락을 쥔 채 `pty.resize` 직전에 읽은 고리의 다음 오프셋을 `Some(next)` 로,
    /// 적용하지 않았으면(열이 2 미만·행이 1 미만·지금과 같은 크기·이미 끝남) `None` 을 돌려 준다.
    /// 가짜 콘솔이 닫힌 뒤에는 Err 가 아니라 `Ok(None)` 로 아무 일도 하지 않고 크기 기록도 그대로다(닫힘과의 경합은 `Pty::resize` 가 막고 적용 여부를 돌려준다).
    /// 알려진 한계(L5): 크기 락은 터미널마다 하나라 `stream.bounds()` 와 `pty.resize` 사이로 들어오는 출력은 막지 못한다. 웹뷰는 `next` 아래를 옛 크기, 이후를 새 크기로 본다.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<Option<u64>, String> {
        let t = self.get(id)?;
        if cols < 2 || rows < 1 || t.stream.exit_code().is_some() {
            return Ok(None);
        }
        let mut size = lock(&t.size);
        if *size == (cols, rows) {
            return Ok(None);
        }
        // 웹뷰가 이 위치까지는 온전히 받았고 새 크기 아래의 출력은 이 위치부터 흐른다고 알 수 있게, 크기 락 아래 직전에 읽는다
        let next = t.stream.bounds().next;
        let applied = t.pty.resize(cols, rows).map_err(|e| format!("터미널 크기 바꾸기 실패: {e}"))?;
        if !applied {
            return Ok(None);
        }
        *size = (cols, rows);
        Ok(Some(next))
    }

    /// 가짜 콘솔을 별도 스레드에서 닫는다. 6초 안에 프로세스가 안 끝나면 강제로 끝낸다. 기다리지 않고 돌아온다.
    /// 프로세스가 끝나면 터미널은 목록에서 빠지고 보내기 스레드도 끝난다.
    pub fn kill(&self, id: &str) -> Result<(), String> {
        let t = self.get(id)?;
        t.kill_requested.store(true, Ordering::SeqCst);
        if t.stream.exit_code().is_some() {
            Self::reap(&self.terms);
            return Ok(());
        }
        let terms = Arc::clone(&self.terms);
        std::thread::spawn(move || {
            let closer = Arc::clone(&t.pty);
            std::thread::spawn(move || closer.close_console());
            if !t.stream.wait_exit(KILL_GRACE) {
                t.pty.kill();
                t.stream.wait_exit(KILL_GRACE);
            }
            Self::reap(&terms);
        });
        Ok(())
    }

    /// 앱을 끝낼 때: 모든 터미널의 프로세스를 바로 끝내고 가짜 콘솔을 닫는다. 기다리지 않는다.
    pub fn kill_all(&self) {
        let all: Vec<Arc<Term>> = lock(&self.terms).clone();
        for t in all {
            t.pty.kill();
            std::thread::spawn(move || t.pty.close_console());
        }
    }
}

/// 출력 파이프를 끝까지 비워 고리에 쓴다. 끝(EOF)에 닿으면 `Exit` 이 나갈 수 있게 표시한다.
fn spawn_reader(id: &str, pty: &Arc<Pty>, stream: &Arc<Stream>) {
    let (pty, stream) = (Arc::clone(pty), Arc::clone(stream));
    std::thread::Builder::new()
        .name(format!("term-read-{id}"))
        .spawn(move || {
            let mut buf = vec![0u8; 64 << 10];
            while let Ok(n @ 1..) = pty.read(&mut buf) {
                stream.push(&buf[..n]);
            }
            pty.close_output();
            stream.finish_output();
        })
        .expect("리더 스레드를 만들지 못했다");
}

/// 프로세스가 끝나길 기다려 종료 코드를 남기고, 가짜 콘솔을 별도 스레드에서 닫는다
/// (자손이 붙어 있으면 콘솔을 닫아야 출력 파이프가 끊긴다. 닫기가 오래 걸려도 리더는 계속 비운다).
fn spawn_waiter(term: &Arc<Term>, terms: &Arc<Mutex<Vec<Arc<Term>>>>) {
    let (term, terms) = (Arc::clone(term), Arc::clone(terms));
    std::thread::Builder::new()
        .name(format!("term-wait-{}", term.id))
        .spawn(move || {
            let code = term.pty.wait_exit();
            term.stream.set_exit(code);
            let closer = Arc::clone(&term.pty);
            std::thread::spawn(move || closer.close_console());
            term.pty.release();
            // 죽이라고 했거나 보는 쪽이 없어졌다면 지금 치운다
            TermHub::reap(&terms);
        })
        .expect("웨이터 스레드를 만들지 못했다");
}

/// Tauri 채널을 배처의 받는 쪽으로: 원시 프레임은 그대로, 제어(`gap`·`exit`)는 JSON.
fn channel_sink(ch: Channel<InvokeResponseBody>) -> Sink {
    Arc::new(move |out| {
        let body = match out {
            Out::Data(frame) => InvokeResponseBody::Raw(frame),
            Out::Gap(seq) => InvokeResponseBody::Json(json!({ "t": "gap", "seq": seq }).to_string()),
            Out::Exit(code) => InvokeResponseBody::Json(json!({ "t": "exit", "code": code }).to_string()),
        };
        ch.send(body).is_ok()
    })
}

type Hub<'a> = State<'a, TermHub>;

#[tauri::command(async)]
pub fn term_spawn(hub: Hub<'_>, spec: SpawnSpec) -> Result<TermInfo, String> {
    hub.spawn(spec)
}

#[tauri::command(async)]
pub fn term_list(hub: Hub<'_>) -> Vec<TermInfo> {
    hub.list()
}

#[tauri::command(async)]
pub fn term_attach(hub: Hub<'_>, id: String, from_seq: u64, on_data: Channel<InvokeResponseBody>) -> Result<TermInfo, String> {
    hub.attach(&id, from_seq, channel_sink(on_data))
}

#[tauri::command(async)]
pub fn term_ack(hub: Hub<'_>, id: String, seq: u64) -> Result<(), String> {
    hub.ack(&id, seq)
}

#[tauri::command(async)]
pub fn term_write(hub: Hub<'_>, id: String, data: String) -> Result<(), String> {
    hub.write(&id, data.as_bytes())
}

#[tauri::command(async)]
pub fn term_write_bin(hub: Hub<'_>, id: String, data: Vec<u8>) -> Result<(), String> {
    hub.write(&id, &data)
}

#[tauri::command(async)]
pub fn term_resize(hub: Hub<'_>, id: String, cols: u16, rows: u16) -> Result<Option<u64>, String> {
    hub.resize(&id, cols, rows)
}

#[tauri::command(async)]
pub fn term_kill(hub: Hub<'_>, id: String) -> Result<(), String> {
    hub.kill(&id)
}

pub mod pairing;

#[cfg(test)]
mod tests;

/// 실제 Windows 에서만 — ConPTY 로 진짜 프로세스를 띄워 확인한다.
#[cfg(all(test, windows))]
mod real;
