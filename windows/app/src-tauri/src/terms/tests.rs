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
