//! 터미널 출력 고리 버퍼와 보내기 흐름 제어(크레딧) — OS 에 기대지 않는 순수 부분.
//!
//! 읽기 스레드는 웹뷰를 기다리지 않고 [`Stream::push`] 로 바이트를 쏟아 넣는다. 오래된 바이트는 고리가 덮어쓴다.
//! 보내는 쪽(배처)은 [`Stream::attach`] 마다 스레드 하나가 돌며, 웹뷰가 처리했다고 알려 준(ack) 끝 오프셋보다
//! 1MiB 넘게 앞서가지 않는다. 고리가 아직 안 보낸 바이트를 덮으면 `Gap` 을 보내고 가장 오래된 바이트부터 다시 보낸다.
//! 오프셋(`seq`)은 터미널이 태어난 뒤 쓴 총 바이트 수이고 줄지 않는다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// 터미널 하나의 출력 고리 크기.
pub const RING_CAP: usize = 4 << 20;
/// 한 번에 보내는 최대 바이트(프레임 머리 8바이트는 따로).
pub const FRAME_MAX: usize = 64 << 10;
/// 처리 확인(ack)이 안 된 채 보낼 수 있는 바이트 한도 — 이 미만일 때만 더 보낸다.
pub const WINDOW: u64 = 1 << 20;
/// 데이터가 적을 때 모아서 보내기까지 기다리는 시간.
const FLUSH: Duration = Duration::from_millis(8);

/// 고리가 덮어써 버려 읽을 수 없는 자리를 읽으려 했다. `oldest` 부터는 읽을 수 있다.
#[derive(Debug, PartialEq, Eq)]
pub struct Gap {
    pub oldest: u64,
}

/// 오프셋이 붙은 고리 버퍼. 용량 이상을 쓰면 가장 오래된 바이트부터 사라진다.
pub struct Ring {
    buf: Vec<u8>,
    total: u64,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "고리 용량은 0 보다 커야 한다");
        Ring { buf: vec![0; cap], total: 0 }
    }

    /// 아직 읽을 수 있는 가장 오래된 오프셋.
    pub fn oldest(&self) -> u64 {
        self.total.saturating_sub(self.buf.len() as u64)
    }

    /// 다음에 쓰일 바이트의 오프셋(= 지금까지 쓴 총 바이트).
    pub fn next(&self) -> u64 {
        self.total
    }

    pub fn push(&mut self, data: &[u8]) {
        let cap = self.buf.len();
        // 용량보다 큰 덩어리는 끝 `cap` 바이트만 남는다
        let keep = &data[data.len().saturating_sub(cap)..];
        let start = ((self.total + (data.len() - keep.len()) as u64) % cap as u64) as usize;
        let first = keep.len().min(cap - start);
        self.buf[start..start + first].copy_from_slice(&keep[..first]);
        self.buf[..keep.len() - first].copy_from_slice(&keep[first..]);
        self.total += data.len() as u64;
    }

    /// `seq` 부터 최대 `max` 바이트를 `out` 뒤에 붙이고 붙인 길이를 돌려준다. 이미 덮인 자리면 `Gap`.
    /// `seq` 가 끝에 닿았거나 넘으면 0바이트.
    pub fn read_into(&self, seq: u64, max: usize, out: &mut Vec<u8>) -> Result<usize, Gap> {
        if seq < self.oldest() {
            return Err(Gap { oldest: self.oldest() });
        }
        let n = (self.total.saturating_sub(seq) as usize).min(max);
        let cap = self.buf.len();
        let start = (seq % cap as u64) as usize;
        let first = n.min(cap - start);
        out.extend_from_slice(&self.buf[start..start + first]);
        out.extend_from_slice(&self.buf[..n - first]);
        Ok(n)
    }
}

/// 웹뷰로 보낼 한 덩어리.
#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    /// 8바이트 little-endian `seq`(첫 페이로드 바이트의 오프셋) + 페이로드(최대 [`FRAME_MAX`]).
    Data(Vec<u8>),
    /// 웹뷰가 놓친 바이트가 있다. 터미널을 비우고 `seq` 부터 다시 받는다.
    Gap(u64),
    /// 셸이 끝났고 마지막 출력까지 다 보냈다.
    Exit(u32),
}

/// 덩어리를 받는 쪽. 받을 수 없으면(채널이 닫힘) false — 그 배처는 멈춘다.
pub type Sink = Arc<dyn Fn(Out) -> bool + Send + Sync>;

/// 지금 읽을 수 있는 오프셋 범위.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub oldest: u64,
    pub next: u64,
}

struct State {
    ring: Ring,
    /// 읽기 스레드가 EOF 에 닿았다(= 더 쏟아질 출력이 없다).
    output_done: bool,
    exit_code: Option<u32>,
    /// attach 마다 올라간다 — 지난 배처는 이 값이 달라지면 멈춘다.
    gen: u64,
    /// 현재 배처가 다음에 보낼 오프셋.
    sent: u64,
    /// 웹뷰가 처리했다고 알려 준 끝 오프셋.
    acked: u64,
    /// 터미널이 목록에서 빠졌다 - 배처는 크레딧도 새 출력도 기다리지 않고 끝난다.
    closed: bool,
    /// 보내다가 받는 쪽이 닫혀 있었다(웹뷰가 없어졌다). 다시 attach 하면 지워진다.
    viewer_lost: bool,
}

/// 터미널 하나의 출력 흐름 — 읽기 스레드가 쓰고, attach 한 배처가 보낸다.
pub struct Stream {
    st: Mutex<State>,
    cv: Condvar,
    /// 지금 도는 배처 스레드 수.
    batchers: AtomicUsize,
}

/// 배처 스레드가 어떻게 끝나든 수를 줄인다.
struct BatcherCount<'a>(&'a AtomicUsize);

impl Drop for BatcherCount<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Stream {
    pub fn new(cap: usize) -> Self {
        Stream {
            st: Mutex::new(State { ring: Ring::new(cap), output_done: false, exit_code: None, gen: 0, sent: 0, acked: 0, closed: false, viewer_lost: false }),
            cv: Condvar::new(),
            batchers: AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 읽기 스레드: 새 출력 바이트.
    pub fn push(&self, data: &[u8]) {
        self.lock().ring.push(data);
        self.cv.notify_all();
    }

    /// 읽기 스레드: 파이프 끝(EOF). 이 뒤에야 `Exit` 이 나갈 수 있다.
    pub fn finish_output(&self) {
        self.lock().output_done = true;
        self.cv.notify_all();
    }

    /// 웨이터: 셸 프로세스의 종료 코드.
    pub fn set_exit(&self, code: u32) {
        self.lock().exit_code = Some(code);
        self.cv.notify_all();
    }

    pub fn exit_code(&self) -> Option<u32> {
        self.lock().exit_code
    }

    /// 종료 코드가 정해질 때까지 `timeout` 만큼 기다린다. 정해졌으면 true.
    pub fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.lock();
        while g.exit_code.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            g = self.cv.wait_timeout(g, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        true
    }

    /// 터미널을 치운다: 도는 배처를 모두 깨워 끝내고 이후 attach 는 배처를 띄우지 않는다.
    pub fn close(&self) {
        self.lock().closed = true;
        self.cv.notify_all();
    }

    /// 지금 도는 배처 스레드 수(시험용).
    #[cfg(test)]
    pub fn active_batchers(&self) -> usize {
        self.batchers.load(Ordering::SeqCst)
    }

    /// 마지막으로 보내려던 받는 쪽이 닫혀 있었다.
    pub fn viewer_lost(&self) -> bool {
        self.lock().viewer_lost
    }

    pub fn bounds(&self) -> Bounds {
        let g = self.lock();
        Bounds { oldest: g.ring.oldest(), next: g.ring.next() }
    }

    /// 웹뷰가 `seq` 까지 처리했다. 보낸 것보다 앞설 수 없고 뒤로 물러서지 않는다.
    pub fn ack(&self, seq: u64) {
        let mut g = self.lock();
        g.acked = g.acked.max(seq.min(g.sent));
        drop(g);
        self.cv.notify_all();
    }

    /// 보는 쪽을 `sink` 로 정하고 `from_seq` 부터 보내기 시작한다(이전 보는 쪽은 대체되고 크레딧은 처음부터).
    /// `from_seq` 가 0 이면 처음 받는 쪽이라 고리에 남은 가장 오래된 바이트부터 보낸다.
    /// 0 이 아닌데 이미 덮여 있으면 `Gap` 을 먼저 보낸다.
    pub fn attach(self: &Arc<Self>, from_seq: u64, sink: Sink) -> Bounds {
        let mut g = self.lock();
        g.gen += 1;
        let gen = g.gen;
        let b = Bounds { oldest: g.ring.oldest(), next: g.ring.next() };
        g.sent = if from_seq == 0 { b.oldest } else { from_seq.min(b.next) };
        g.acked = g.sent;
        g.viewer_lost = false;
        let closed = g.closed;
        // 배처 수는 락 아래서 올려 close 뒤에 새 배처가 생기지 않게 한다
        if !closed {
            self.batchers.fetch_add(1, Ordering::SeqCst);
        }
        drop(g);
        // 지난 배처를 깨워 멈추게 한다
        self.cv.notify_all();
        if closed {
            return b;
        }
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("term-batch".into())
            .spawn(move || me.batcher(gen, sink))
            .expect("배처 스레드를 만들지 못했다");
        b
    }

    /// 보낼 것이 있을 때 8ms 또는 64KiB 마다 보내고, ack 안 된 바이트가 [`WINDOW`] 이상이면 기다린다.
    fn batcher(&self, gen: u64, sink: Sink) {
        let _count = BatcherCount(&self.batchers);
        let mut g = self.lock();
        let mut since: Option<Instant> = None;
        loop {
            if g.gen != gen || g.closed {
                return;
            }
            let (oldest, next) = (g.ring.oldest(), g.ring.next());
            let avail = next.saturating_sub(g.sent);
            let (out, last) = if g.sent < oldest {
                g.sent = oldest;
                g.acked = oldest;
                since = None;
                (Out::Gap(oldest), false)
            } else if avail == 0 {
                since = None;
                match g.exit_code.filter(|_| g.output_done) {
                    Some(code) => (Out::Exit(code), true),
                    None => {
                        g = self.cv.wait(g).unwrap_or_else(PoisonError::into_inner);
                        continue;
                    }
                }
            } else if g.sent - g.acked >= WINDOW {
                g = self.cv.wait(g).unwrap_or_else(PoisonError::into_inner);
                continue;
            } else {
                let t0 = *since.get_or_insert_with(Instant::now);
                let full = avail >= FRAME_MAX as u64;
                let left = FLUSH.saturating_sub(t0.elapsed());
                if !full && !g.output_done && !left.is_zero() {
                    g = self.cv.wait_timeout(g, left).unwrap_or_else(PoisonError::into_inner).0;
                    continue;
                }
                let mut frame = Vec::with_capacity(8 + FRAME_MAX.min(avail as usize));
                frame.extend_from_slice(&g.sent.to_le_bytes());
                let n = g.ring.read_into(g.sent, FRAME_MAX, &mut frame).expect("오프셋 검사를 이미 했다");
                g.sent += n as u64;
                since = None;
                (Out::Data(frame), false)
            };
            // 보내는 동안 락을 쥐지 않는다 — 읽기 스레드와 ack 가 막히면 안 된다
            drop(g);
            if !sink(out) {
                let mut g = self.lock();
                if g.gen == gen {
                    g.viewer_lost = true;
                }
                return;
            }
            if last {
                return;
            }
            g = self.lock();
        }
    }
}

#[cfg(test)]
mod tests;
