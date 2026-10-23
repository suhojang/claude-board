use super::*;

#[test]
fn unknown_id_is_an_error_not_a_panic() {
    let hub = TermHub::default();
    assert!(hub.ack("t9", 0).is_err());
    assert!(hub.write("t9", b"x").is_err());
    assert!(hub.resize("t9", 80, 24).is_err());
    assert!(hub.kill("t9").is_err());
    assert!(hub.list().is_empty());
}

#[test]
fn term_info_serializes_with_camel_case_keys() {
    let info = TermInfo {
        id: "t1".into(),
        pid: 4,
        shell: "pwsh".into(),
        cwd: "C:\\".into(),
        cols: 80,
        rows: 24,
        status: "exited",
        exit_code: Some(3),
        windows_build: None,
        oldest_seq: 0,
        next_seq: 9,
    };
    let v = serde_json::to_value(&info).unwrap();
    assert_eq!(
        v,
        json!({"id":"t1","pid":4,"shell":"pwsh","cwd":"C:\\","cols":80,"rows":24,"status":"exited","exitCode":3,"windowsBuild":null,"oldestSeq":0,"nextSeq":9})
    );
    let spec: SpawnSpec = serde_json::from_value(json!({"cols": 100, "rows": 40})).unwrap();
    assert_eq!((spec.cwd, spec.cols, spec.rows), (None, 100, 40));
}

#[cfg(not(windows))]
#[test]
fn spawn_is_unsupported_off_windows() {
    let err = TermHub::default().spawn(SpawnSpec { cwd: None, cols: 80, rows: 24 }).unwrap_err();
    assert!(!err.is_empty());
}

/// resize 반환값은 실제 ConPTY 로 확인한다 — Windows 에서만 돈다.
#[cfg(windows)]
mod resize {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// 시험이 띄운 터미널을 남기지 않게 끝날 때 모두 끝낸다.
    struct Sweep(TermHub);

    impl std::ops::Deref for Sweep {
        type Target = TermHub;
        fn deref(&self) -> &TermHub {
            &self.0
        }
    }

    impl Drop for Sweep {
        fn drop(&mut self) {
            self.0.kill_all();
        }
    }

    /// 계속 사는 대화형 cmd 하나를 띄운다.
    fn idle_cmd(hub: &TermHub) -> TermInfo {
        let cmd = std::env::var_os("ComSpec").map(PathBuf::from).expect("ComSpec 이 없다");
        hub.spawn_program("cmd", &cmd, &[], &paths::home(), 80, 24).unwrap()
    }

    /// 고리 위치가 잦아들 때까지 기다린다 — resize 가 읽는 순간 뒤로 새 출력이 섞이지 않게.
    fn wait_quiet(t: &Term, secs: u64) {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let a = t.stream.bounds();
            std::thread::sleep(Duration::from_millis(400));
            let b = t.stream.bounds();
            if a == b {
                return;
            }
            assert!(Instant::now() < end, "{secs}초 안에 출력이 잦아들지 않았다: {a:?} → {b:?}");
        }
    }

    /// 새 크기 → 적용하고 `Some(next)` 는 그 순간의 고리 위치, 같은 크기 → None.
    #[test]
    fn applied_returns_stream_next_and_same_size_returns_none() {
        let hub = Sweep(TermHub::default());
        let info = idle_cmd(&hub);
        wait_quiet(&hub.get(&info.id).unwrap(), 20);
        let before = hub.get(&info.id).unwrap().stream.bounds();
        let got = hub.resize(&info.id, 100, 40).unwrap();
        assert_eq!(got, Some(before.next), "적용했으면 그 순간의 다음 오프셋");
        let after = &hub.list()[0];
        assert_eq!((after.cols, after.rows), (100, 40), "크기도 실제로 바뀐다");
        assert_eq!(hub.resize(&info.id, 100, 40).unwrap(), None, "같은 크기는 적용하지 않는다");
    }

    /// 열이 2 미만이거나 행이 1 미만이면 None 이고 크기는 그대로.
    #[test]
    fn tiny_size_is_not_applied() {
        let hub = Sweep(TermHub::default());
        let info = idle_cmd(&hub);
        assert_eq!(hub.resize(&info.id, 1, 40).unwrap(), None, "열이 2 미만");
        assert_eq!(hub.resize(&info.id, 100, 0).unwrap(), None, "행이 1 미만");
        let after = &hub.list()[0];
        assert_eq!((after.cols, after.rows), (80, 24), "무시한 크기는 반영하지 않는다");
    }

    /// 끝난 터미널의 resize 는 None.
    #[test]
    fn exited_terminal_returns_none() {
        let hub = Sweep(TermHub::default());
        let cmd = std::env::var_os("ComSpec").map(PathBuf::from).expect("ComSpec 이 없다");
        let info = hub
            .spawn_program("cmd", &cmd, &["/c".into(), "exit 0".into()], &paths::home(), 80, 24)
            .unwrap();
        let end = Instant::now() + Duration::from_secs(20);
        while hub.list().first().map(|t| t.status) != Some("exited") {
            assert!(Instant::now() < end, "20초 안에 종료하지 않았다: {:?}", hub.list().first().map(|t| t.status));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(hub.resize(&info.id, 100, 40).unwrap(), None, "끝난 터미널은 적용하지 않는다");
    }

    /// 가짜 콘솔이 닫힌 터미널의 resize 는 None 이고 기록된 크기도 그대로.
    #[test]
    fn closed_console_returns_none_and_keeps_size() {
        let hub = Sweep(TermHub::default());
        let info = idle_cmd(&hub);
        hub.get(&info.id).unwrap().pty.close_console();
        assert_eq!(hub.resize(&info.id, 100, 40).unwrap(), None, "닫힌 콘솔은 적용하지 않는다");
        let after = &hub.list()[0];
        assert_eq!((after.cols, after.rows), (80, 24), "적용하지 않은 크기는 반영하지 않는다");
    }
}
