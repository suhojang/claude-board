use super::*;
use crate::ring::{Out, Sink};
use crate::terms::TermHub;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use claude_board_core::paths;
use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleProcessList};

/// 끝나면 터미널과 시험이 띄운 프로세스를 모두 끝낸다.
struct Guard {
    hub: TermHub,
    pids: Vec<u32>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        for &p in &self.pids {
            proc::terminate(p);
        }
        self.hub.kill_all();
    }
}

fn find_node() -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.into_iter().map(|d| d.join("node.exe")).find(|p| p.is_file())
}

/// 터미널 출력을 처음부터 모은다(받은 만큼 ack).
fn collect(hub: &TermHub, id: &str) -> Arc<Mutex<Vec<u8>>> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (me, stream) = (Arc::clone(&bytes), Arc::clone(&hub.get(id).unwrap().stream));
    let sink: Sink = Arc::new(move |o| {
        if let Out::Data(f) = o {
            let seq = u64::from_le_bytes(f[..8].try_into().unwrap());
            me.lock().unwrap().extend_from_slice(&f[8..]);
            stream.ack(seq + f.len() as u64 - 8);
        }
        true
    });
    hub.attach(id, 0, sink).unwrap();
    bytes
}

fn wait_for<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "{secs}초 안에 {what} 이(가) 되지 않았다");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `{"self":…}` 한 줄에서 숫자 하나.
fn num(line: &str, name: &str) -> u32 {
    let rest = &line[line.find(&format!("\"{name}\":")).unwrap_or_else(|| panic!("{name} 없음: {line}")) + name.len() + 3..];
    rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap()
}

/// 시험 프로세스가 아닌 보조 프로세스(이 시험 바이너리를 다시 실행)가 그 콘솔에 붙어 프로세스 목록을 돌려준다 — 판정의 기준.
/// `AttachConsole` 은 자기 콘솔을 바꾸므로 시험 프로세스에서는 하지 않는다.
#[test]
fn oracle_child() {
    let Some(target) = std::env::var("CB_ORACLE_PID").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    let mut list = [0u32; 64];
    // SAFETY: 자기 콘솔을 떼고 대상 콘솔에 붙어 프로세스 목록만 읽는다
    let n = unsafe {
        FreeConsole();
        if AttachConsole(target) == 0 {
            println!("ORACLE:attach-failed");
            return;
        }
        GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32)
    };
    let ids: Vec<String> = list[..(n as usize).min(list.len())].iter().map(u32::to_string).collect();
    println!("ORACLE:{}", ids.join(","));
}

fn oracle_list(pid: u32) -> Vec<u32> {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["terms::pairing::real::oracle_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("CB_ORACLE_PID", pid.to_string())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text.lines().find_map(|l| l.split("ORACLE:").nth(1)).unwrap_or_else(|| panic!("보조 프로세스 출력 없음: {text}"));
    line.split(',').filter_map(|s| s.trim().parse().ok()).collect()
}

const CHILD: &str = r#"
const { spawn } = require('child_process');
const path = require('path');
const keep = path.join(__dirname, 'keep.js');
const tag = process.argv[2];
const inherit = spawn(process.execPath, [keep], { stdio: 'inherit' });
const detached = spawn(process.execPath, [keep], { detached: true, stdio: 'ignore' });
const nowin = spawn(process.execPath, [keep], { windowsHide: true, stdio: 'ignore' });
spawn('cmd', ['/c', 'start', '""', '/min', process.execPath, keep, 'newcon-' + tag], { stdio: 'ignore' });
console.log('PIDS:' + JSON.stringify({ self: process.pid, inherit: inherit.pid, detached: detached.pid, nowin: nowin.pid }));
setInterval(() => {}, 1000);
"#;

/// ConPTY 셸에서 node 를 띄워, 같은 콘솔을 물려받은 자식·분리된 자식·숨은 콘솔 자식·새 콘솔 자식의 짝짓기를 가린다.
#[test]
fn console_host_pairs_conpty_child() {
    let Some(node) = find_node() else {
        assert!(std::env::var_os("CI").is_none(), "CI 인데 PATH 에서 node.exe 를 찾지 못했다");
        eprintln!("node.exe 가 PATH 에 없어 이 시험을 건너뛴다");
        return;
    };
    let tag = crate::util::short_id();
    let dir = std::env::temp_dir().join(format!("cb-pair-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.js"), "setInterval(() => {}, 1000);").unwrap();
    std::fs::write(dir.join("child.js"), CHILD).unwrap();
    let mut g = Guard { hub: TermHub::default(), pids: Vec::new() };
    let cmd = std::env::var_os("ComSpec").map(PathBuf::from).expect("ComSpec");
    let info = g.hub.spawn_program("cmd", &cmd, &["/d".to_string(), "/k".to_string()], &paths::home(), 120, 30).unwrap();
    let out = collect(&g.hub, &info.id);
    let line = format!("\"{}\" \"{}\" {tag}\r", node.display(), dir.join("child.js").display());
    g.hub.write(&info.id, line.as_bytes()).unwrap();
    let pids = wait_for(20, "자식 pid 출력", || {
        let text = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
        let at = text.find("PIDS:{")?;
        let end = text[at..].find('}')?;
        Some(text[at + 5..=at + end].to_string())
    });
    let (me, inherit, detached, nowin) = (num(&pids, "self"), num(&pids, "inherit"), num(&pids, "detached"), num(&pids, "nowin"));
    let table = wait_for(20, "새 콘솔 자식", || {
        let t = ProcessTable::capture();
        t.pids_named("node.exe").iter().any(|&p| proc::command_line(p).is_some_and(|a| a.iter().any(|s| s.contains(&format!("newcon-{tag}"))))).then_some(t)
    });
    let newcon = *table.pids_named("node.exe").iter().find(|&&p| proc::command_line(p).is_some_and(|a| a.iter().any(|s| s.contains(&format!("newcon-{tag}"))))).unwrap();
    g.pids.extend([me, inherit, detached, nowin, newcon]);
    // 자식의 콘솔 연결이 자리 잡을 때까지
    let shell = info.pid;
    let host = wait_for(10, "셸 콘솔 호스트", || match proc::console_host(shell) {
        Host::Console(h) => Some(h),
        _ => None,
    });
    wait_for(10, "inherit 자식 콘솔", || (proc::console_host(inherit) != Host::Unknown).then_some(()));
    // 호스트 비교
    assert_eq!(proc::console_host(me), Host::Console(host), "node 본인");
    assert_eq!(proc::console_host(inherit), Host::Console(host), "콘솔을 물려받은 자식");
    assert_eq!(proc::console_host(detached), Host::Detached, "DETACHED_PROCESS 자식");
    for (name, pid) in [("숨은 콘솔 자식", nowin), ("새 콘솔 자식", newcon)] {
        let h = proc::console_host(pid);
        assert!(matches!(h, Host::Console(x) if x != host), "{name} 는 다른 콘솔이어야 한다: {h:?}");
    }
    // 기준: 같은 콘솔에 붙은 프로세스 목록
    let listed = oracle_list(me);
    assert!(listed.contains(&shell) && listed.contains(&inherit), "기준 목록 {listed:?}");
    assert!(!listed.contains(&newcon) && !listed.contains(&detached), "기준 목록 {listed:?}");
    // 짝짓기
    let terms = g.hub.hosts();
    assert_eq!(terms.len(), 1);
    let sessions = [
        SessionProc { pid: inherit, headless: false, ended: false },
        SessionProc { pid: newcon, headless: false, ended: false },
        SessionProc { pid: nowin, headless: true, ended: false },
        SessionProc { pid: detached, headless: true, ended: false },
    ];
    let out = pair_all(&terms, &sessions, &table);
    assert_eq!(out.places[&inherit].tty.as_deref(), Some("t1"));
    assert_eq!(out.places[&newcon].tty, None, "새 콘솔 자식은 짝짓지 않는다");
    assert_eq!(out.places[&nowin].tty, None);
    assert_eq!(out.places[&nowin].tab_ancestor.as_deref(), Some("t1"), "숨은 콘솔 headless 자식은 셸 조상 아래");
    assert!(out.no_attach.contains(&inherit) && !out.no_attach.contains(&newcon));
}
