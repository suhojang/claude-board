use super::*;

fn read(r: &Ring, seq: u64, max: usize) -> Result<Vec<u8>, Gap> {
    let mut v = Vec::new();
    r.read_into(seq, max, &mut v).map(|_| v)
}

#[test]
fn ring_keeps_order_across_wrap() {
    let mut r = Ring::new(8);
    r.push(b"abc");
    r.push(b"def");
    assert_eq!((r.oldest(), r.next()), (0, 6));
    assert_eq!(read(&r, 0, 100).unwrap(), b"abcdef");
    // 용량 8 을 넘겨 한 바퀴 돈다
    r.push(b"ghijk");
    assert_eq!((r.oldest(), r.next()), (3, 11));
    assert_eq!(read(&r, 3, 100).unwrap(), b"defghijk");
    assert_eq!(read(&r, 7, 100).unwrap(), b"hijk");
    assert_eq!(read(&r, 5, 2).unwrap(), b"fg");
}

#[test]
fn ring_drops_oldest_and_reports_gap() {
    let mut r = Ring::new(4);
    r.push(b"0123456789");
    assert_eq!((r.oldest(), r.next()), (6, 10));
    assert_eq!(read(&r, 6, 100).unwrap(), b"6789");
    assert_eq!(read(&r, 0, 100), Err(Gap { oldest: 6 }));
    assert_eq!(read(&r, 5, 100), Err(Gap { oldest: 6 }));
    // 끝이거나 넘으면 빈 값
    assert_eq!(read(&r, 10, 100).unwrap(), b"");
    assert_eq!(read(&r, 99, 100).unwrap(), b"");
}

#[test]
fn ring_oversized_chunk_then_small_ones_stay_contiguous() {
    let mut r = Ring::new(5);
    r.push(b"abcdefghijkl"); // 12바이트, 남는 것은 hijkl
    r.push(b"mn");
    assert_eq!(read(&r, r.oldest(), 100).unwrap(), b"jklmn");
    assert_eq!((r.oldest(), r.next()), (9, 14));
}

/// 시험용 받는 쪽 — 덩어리를 모으고, 요청하면 ack 한다.
#[derive(Default)]
struct Cap {
    outs: Mutex<Vec<Out>>,
}

impl Cap {
    fn sink(self: &Arc<Self>) -> Sink {
        let me = Arc::clone(self);
        Arc::new(move |o| {
            me.outs.lock().unwrap().push(o);
            true
        })
    }

    fn data_bytes(&self) -> u64 {
        bytes_of(&self.outs.lock().unwrap())
    }

    fn wait(&self, what: &str, f: impl Fn(&[Out]) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !f(&self.outs.lock().unwrap()) {
            assert!(Instant::now() < end, "10초 안에 \"{what}\" 가 되지 않았다");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn bytes_of(outs: &[Out]) -> u64 {
    outs.iter().map(|o| if let Out::Data(f) = o { f.len() as u64 - 8 } else { 0 }).sum()
}

fn seq_of(f: &[u8]) -> u64 {
    u64::from_le_bytes(f[..8].try_into().unwrap())
}

#[test]
fn stream_sends_contiguous_frames_then_exit_last() {
    let s = Arc::new(Stream::new(1 << 16));
    let cap = Arc::new(Cap::default());
    s.push(b"hello ");
    let b = s.attach(0, cap.sink());
    assert_eq!((b.oldest, b.next), (0, 6));
    s.push(b"world");
    cap.wait("11바이트", |o| bytes_of(o) == 11);
    // 종료 코드만 있고 출력이 안 끝났으면 Exit 은 아직이다
    s.set_exit(3);
    std::thread::sleep(Duration::from_millis(60));
    assert!(!cap.outs.lock().unwrap().iter().any(|o| matches!(o, Out::Exit(_))));
    s.push(b"!");
    s.finish_output();
    cap.wait("Exit", |o| matches!(o.last(), Some(Out::Exit(3))));
    let outs = cap.outs.lock().unwrap();
    let (mut at, mut text) = (0u64, Vec::new());
    for o in outs.iter() {
        if let Out::Data(f) = o {
            assert_eq!(seq_of(f), at, "프레임 오프셋이 이어지지 않는다");
            at += f.len() as u64 - 8;
            text.extend_from_slice(&f[8..]);
        }
    }
    assert_eq!(text, b"hello world!");
    assert_eq!(outs.iter().filter(|o| matches!(o, Out::Exit(_))).count(), 1);
}

#[test]
fn attach_from_zero_after_overwrite_starts_at_oldest_without_gap() {
    let s = Arc::new(Stream::new(100));
    s.push(&[7u8; 250]);
    let cap = Arc::new(Cap::default());
    s.attach(0, cap.sink());
    cap.wait("100바이트", |o| bytes_of(o) == 100);
    let outs = cap.outs.lock().unwrap();
    assert_eq!(outs.len(), 1);
    let Out::Data(f) = &outs[0] else { panic!("데이터가 아니다: {:?}", outs[0]) };
    assert_eq!(seq_of(f), 150);
}

#[test]
fn attach_from_nonzero_inside_overwritten_area_gets_gap_first() {
    let s = Arc::new(Stream::new(100));
    s.push(&[1u8; 250]);
    let cap = Arc::new(Cap::default());
    s.attach(10, cap.sink());
    cap.wait("Gap 과 100바이트", |o| bytes_of(o) == 100);
    let outs = cap.outs.lock().unwrap();
    assert_eq!(outs[0], Out::Gap(150));
    let Out::Data(f) = &outs[1] else { panic!("Gap 다음이 데이터가 아니다") };
    assert_eq!(seq_of(f), 150);
}

#[test]
fn slow_viewer_overwritten_midstream_gets_gap_and_resumes_at_oldest() {
    let s = Arc::new(Stream::new(128 << 10));
    let gate = Arc::new(Mutex::new(()));
    let cap = Arc::new(Cap::default());
    let (c, g) = (Arc::clone(&cap), Arc::clone(&gate));
    let sink: Sink = Arc::new(move |o| {
        let _held = g.lock().unwrap();
        c.outs.lock().unwrap().push(o);
        true
    });
    let hold = gate.lock().unwrap();
    s.push(&[1u8; 70_000]);
    s.attach(0, sink);
    std::thread::sleep(Duration::from_millis(50));
    // 첫 프레임이 받는 쪽에서 막힌 사이 고리가 한 바퀴 넘게 돈다
    for _ in 0..10 {
        s.push(&[2u8; 64 << 10]);
    }
    drop(hold);
    let end = s.bounds().next;
    cap.wait("Gap 뒤 끝까지", |o| {
        o.iter().any(|x| matches!(x, Out::Gap(_)))
            && o.iter().rev().find_map(|x| if let Out::Data(f) = x { Some(seq_of(f) + f.len() as u64 - 8) } else { None }) == Some(end)
    });
    let outs = cap.outs.lock().unwrap();
    let gap_at = outs.iter().position(|x| matches!(x, Out::Gap(_))).unwrap();
    let Out::Gap(oldest) = outs[gap_at] else { unreachable!() };
    let Out::Data(next) = &outs[gap_at + 1] else { panic!("Gap 다음이 데이터가 아니다") };
    assert_eq!(seq_of(next), oldest, "Gap 뒤에는 가장 오래된 바이트부터 다시 온다");
}

#[test]
fn unacked_bytes_stop_at_window_and_ack_releases_more() {
    let s = Arc::new(Stream::new(RING_CAP));
    let cap = Arc::new(Cap::default());
    s.push(&vec![9u8; 3 << 20]);
    s.attach(0, cap.sink());
    cap.wait("창이 찰 때까지", |o| bytes_of(o) >= WINDOW);
    std::thread::sleep(Duration::from_millis(80));
    let held = cap.data_bytes();
    assert!(held >= WINDOW && held < WINDOW + FRAME_MAX as u64, "ack 없이 보낸 바이트 {held}");
    s.ack(held);
    cap.wait("더 보냄", |o| bytes_of(o) > held);
    // 계속 ack 하면 끝까지 간다
    let end = Instant::now() + Duration::from_secs(10);
    while cap.data_bytes() < 3 << 20 {
        assert!(Instant::now() < end, "3MiB 가 다 가지 않았다: {}", cap.data_bytes());
        s.ack(cap.data_bytes());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn new_attach_replaces_old_viewer_and_resets_credit() {
    let s = Arc::new(Stream::new(1 << 16));
    let (a, b) = (Arc::new(Cap::default()), Arc::new(Cap::default()));
    s.push(b"one");
    s.attach(0, a.sink());
    a.wait("첫 보는 쪽이 3바이트", |o| bytes_of(o) == 3);
    s.attach(0, b.sink());
    b.wait("둘째 보는 쪽이 처음부터 3바이트", |o| bytes_of(o) == 3);
    s.push(b"two");
    b.wait("둘째가 6바이트", |o| bytes_of(o) == 6);
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(a.data_bytes(), 3, "대체된 보는 쪽에는 더 보내지 않는다");
}

#[test]
fn exit_is_replayed_to_a_viewer_that_attaches_afterwards() {
    let s = Arc::new(Stream::new(1 << 16));
    s.push(b"bye");
    s.set_exit(0);
    s.finish_output();
    let cap = Arc::new(Cap::default());
    s.attach(0, cap.sink());
    cap.wait("Exit", |o| matches!(o.last(), Some(Out::Exit(0))));
    assert_eq!(cap.data_bytes(), 3);
}

fn wait_until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < end, "10초 안에 \"{what}\" 가 되지 않았다");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn close_ends_an_idle_batcher() {
    let s = Arc::new(Stream::new(64));
    let cap = Arc::new(Cap::default());
    s.attach(0, cap.sink());
    wait_until("배처 시작", || s.active_batchers() == 1);
    s.close();
    wait_until("배처 종료", || s.active_batchers() == 0);
    // 닫힌 뒤 attach 는 배처를 띄우지 않는다
    s.attach(0, cap.sink());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.active_batchers(), 0);
}

#[test]
fn close_ends_a_batcher_waiting_for_credit() {
    let s = Arc::new(Stream::new(4 << 20));
    let cap = Arc::new(Cap::default());
    s.attach(0, cap.sink());
    s.push(&vec![7u8; (WINDOW as usize) + FRAME_MAX]);
    cap.wait("창이 찰 때까지", |o| bytes_of(o) >= WINDOW);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.active_batchers(), 1, "ack 를 기다리며 서 있어야 한다");
    s.close();
    wait_until("배처 종료", || s.active_batchers() == 0);
}

#[test]
fn closed_sink_marks_viewer_lost_and_ends_the_batcher() {
    let s = Arc::new(Stream::new(64));
    s.attach(0, Arc::new(|_| false));
    s.push(b"x");
    wait_until("배처 종료", || s.active_batchers() == 0);
    assert!(s.viewer_lost());
    let cap = Arc::new(Cap::default());
    s.attach(0, cap.sink());
    assert!(!s.viewer_lost());
    s.close();
}
