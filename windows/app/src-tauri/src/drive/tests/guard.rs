// 리뷰 수정(재사용 pid 보호·Stop 대체 경로·단계 합계 상한)에서 더한 시험
use super::*;

#[test]
fn stale_snapshot_never_drives_or_kills() {
    let old = session(5, "working", Some("t1"), None, false, None);
    let mut same = old.clone();
    same.state = "idle".into();
    assert!(still_same_session(&old, Some(same)).is_some());
    let mut ended = old.clone();
    ended.state = "ended".into();
    assert!(still_same_session(&old, Some(ended)).is_none());
    assert!(still_same_session(&old, None).is_none());
    let mut reused = old.clone();
    reused.pid = 6;
    assert!(still_same_session(&old, Some(reused)).is_none());
    let mut other_start = old.clone();
    other_start.pid_ctime = Some(1.0);
    assert!(still_same_session(&old, Some(other_start)).is_none());
}

#[test]
fn stop_falls_back_to_kill_when_the_surface_write_fails() {
    let io = Fake::new(false);
    let target = Target::Surface("t1".into());
    assert!(!run_with_fallback(&target, 5, &steps_for(SessionAction::Stop), Purpose::Stop, &io));
    assert_eq!(io.calls().last().map(String::as_str), Some("kill 5"));
    // 중단이 아니면 끝내지 않는다
    let keys = Fake::new(false);
    assert!(!run_with_fallback(&target, 5, &steps_for(SessionAction::Pause), Purpose::Keys, &keys));
    assert!(!keys.calls().iter().any(|c| c.starts_with("kill")));
    // 쓰기가 되면 끝내지 않는다
    let ok = Fake::new(true);
    assert!(run_with_fallback(&target, 5, &steps_for(SessionAction::Stop), Purpose::Stop, &ok));
    assert!(!ok.calls().iter().any(|c| c.starts_with("kill")));
}

#[test]
fn total_wait_is_capped() {
    assert!(total_wait_ok(&[Step::Wait(5000), Step::Wait(5000)]));
    assert!(!total_wait_ok(&[Step::Wait(5000), Step::Wait(5000), Step::Wait(1)]));
    assert!(total_wait_ok(&steps_for(SessionAction::Stop)));
}
