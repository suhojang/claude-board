//! 프로세스 생사 확인과 끝나기 기다리기 — 다시 뜰 때 앞 보드가 남긴 pid 가 아직 도는지(`pids_alive`),
//! 보드 밖 터미널의 claude 가 `/exit` 뒤 끝났는지(`wait_exit`)를 프론트에 답한다. Win32 는 `claude_board_core::proc::alive` 가 쥔다.

use std::time::Duration;

use claude_board_core::proc;
use serde::Deserialize;

/// 기다리는 간격 — 프로세스가 끝나자마자 알아채되 CPU 를 먹지 않을 만큼
pub const POLL: Duration = Duration::from_millis(100);

/// 생사를 물을 프로세스 — pid 와 그 프로세스의 생성 시각(세션 기록의 `pid_ctime`). 생성 시각이 있으면 재사용된 pid 를 산 것으로 보지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct PidTarget {
    pub pid: u32,
    #[serde(default)]
    pub ctime: Option<f64>,
}

fn target_alive(target: PidTarget) -> bool {
    target.pid != 0 && proc::alive(target.pid, target.ctime)
}

/// 주어진 프로세스 가운데 아직 산 것의 pid(입력 순서 그대로). pid 0 은 프로세스가 아니므로 산 것으로 치지 않는다.
pub fn alive_among(targets: &[PidTarget]) -> Vec<u32> {
    targets.iter().copied().filter(|t| target_alive(*t)).map(|t| t.pid).collect()
}

/// `is_alive` 가 거짓이 될 때까지 `poll` 간격으로 본다. `timeout` 안에 끝났으면 true, 아니면 false.
/// 경과 시간은 `sleep` 에 넘긴 시간의 합으로 센다 — 시계를 주입하지 않고도 시험할 수 있게 한다(진짜 잠은 `sleep` 이 잔다).
pub fn wait_gone(mut is_alive: impl FnMut() -> bool, timeout: Duration, poll: Duration, mut sleep: impl FnMut(Duration)) -> bool {
    let mut waited = Duration::ZERO;
    loop {
        if !is_alive() {
            return true;
        }
        if waited >= timeout {
            return false;
        }
        let step = poll.min(timeout - waited);
        sleep(step);
        waited += step;
    }
}

/// 그 프로세스가 `timeout_ms` 안에 끝나기를 기다린다. 이미 없거나 끝났으면 true, 시간이 지나도 살아 있으면 false.
/// pid 0 은 어떤 프로세스인지 모른다는 뜻이라 끝났다고 말할 수 없다 — 기다리지 않고 false.
pub fn wait_exit_blocking(target: PidTarget, timeout_ms: u64) -> bool {
    if target.pid == 0 {
        return false;
    }
    wait_gone(|| target_alive(target), Duration::from_millis(timeout_ms), POLL, std::thread::sleep)
}

/// 산 프로세스의 pid 만 추려 돌려준다. 프론트의 `pidsAlive(targets)`.
#[tauri::command(async)]
pub fn pids_alive(targets: Vec<PidTarget>) -> Vec<u32> {
    alive_among(&targets)
}

/// 프론트의 `waitExit(pid, pidCtime, timeoutMs)` — 끝났으면 true. 기다리는 동안 이 명령 스레드가 막힌다(`async` 명령은 별도 스레드에서 돈다).
#[tauri::command(async)]
pub fn wait_exit(pid: u32, pid_ctime: Option<f64>, timeout_ms: u64) -> bool {
    wait_exit_blocking(PidTarget { pid, ctime: pid_ctime }, timeout_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::process::{Command, Stdio};

    /// 곧 끝나는 자식 프로세스 — 시험 실행 파일 자신을 목록만 찍고 끝나게 부른다
    fn short_lived_child() -> std::process::Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--list", "--format", "terse"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn plain(pid: u32) -> PidTarget {
        PidTarget { pid, ctime: None }
    }

    #[test]
    fn this_process_is_alive_and_zero_never() {
        let me = std::process::id();
        assert_eq!(alive_among(&[plain(0), plain(me), plain(0)]), [me]);
        assert!(alive_among(&[]).is_empty());
    }

    #[test]
    fn a_reused_pid_with_another_creation_time_is_not_alive() {
        let me = std::process::id();
        let real = proc::creation_time(me).expect("이 프로세스의 생성 시각");
        assert_eq!(alive_among(&[PidTarget { pid: me, ctime: Some(real) }]), [me]);
        assert!(alive_among(&[PidTarget { pid: me, ctime: Some(real - 100.0) }]).is_empty(), "생성 시각이 다르면 재사용된 pid 다");
        assert!(wait_exit_blocking(PidTarget { pid: me, ctime: Some(real - 100.0) }, 0), "다른 프로세스라면 그 프로세스는 이미 끝났다");
    }

    #[test]
    fn pid_zero_never_counts_as_exited() {
        let started = std::time::Instant::now();
        assert!(!wait_exit_blocking(plain(0), 10_000), "pid 를 모르는 세션은 끝났다고 말하지 않는다");
        assert!(started.elapsed() < Duration::from_secs(1), "pid 0 은 기다리지 않고 바로 돌아온다");
    }

    #[test]
    fn finished_child_is_not_alive_and_wait_returns_at_once() {
        let mut child = short_lived_child();
        let pid = child.id();
        child.wait().unwrap();
        assert!(alive_among(&[plain(pid)]).is_empty());
        assert!(wait_exit_blocking(plain(pid), 50));
    }

    #[test]
    fn real_wait_sees_a_process_end() {
        let mut child = short_lived_child();
        let pid = child.id();
        // 도는 동안 기다리기 시작해도 끝나면 true (수거(`wait`)는 생사 판정과 무관하다)
        let waiter = std::thread::spawn(move || wait_exit_blocking(plain(pid), 10_000));
        child.wait().unwrap();
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn wait_gone_ends_when_process_ends_within_timeout() {
        let polls = Cell::new(0);
        let slept = Cell::new(Duration::ZERO);
        let done = wait_gone(
            || {
                polls.set(polls.get() + 1);
                polls.get() < 4 // 네 번째 확인에서 끝남
            },
            Duration::from_millis(5000),
            Duration::from_millis(200),
            |d| slept.set(slept.get() + d),
        );
        assert!(done);
        assert_eq!(slept.get(), Duration::from_millis(600));
    }

    #[test]
    fn wait_gone_gives_up_at_the_timeout_without_oversleeping() {
        let slept = Cell::new(Duration::ZERO);
        let done = wait_gone(|| true, Duration::from_millis(5000), Duration::from_millis(200), |d| slept.set(slept.get() + d));
        assert!(!done);
        assert_eq!(slept.get(), Duration::from_millis(5000));
        // 간격이 나누어떨어지지 않아도 상한을 넘겨 자지 않는다
        let slept = Cell::new(Duration::ZERO);
        assert!(!wait_gone(|| true, Duration::from_millis(250), Duration::from_millis(200), |d| slept.set(slept.get() + d)));
        assert_eq!(slept.get(), Duration::from_millis(250));
    }

    #[test]
    fn wait_gone_zero_timeout_checks_once() {
        assert!(wait_gone(|| false, Duration::ZERO, POLL, |_| panic!("자면 안 된다")));
        assert!(!wait_gone(|| true, Duration::ZERO, POLL, |_| panic!("자면 안 된다")));
    }
}
