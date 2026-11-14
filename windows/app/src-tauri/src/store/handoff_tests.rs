//! 넘긴 세션 표지(대화 기록 끝의 continued-in 줄)를 보드에 잇는 일의 시험 — `crate::readers::HandoffReader` 가
//! core `handoff::handed_off_to_in` 으로 꼬리를 읽고, `Store::reload` 가 그 결과로 `handoff::visible_sessions`
//! 를 돌려 넘긴 세션을 목록에서 빼는 것(SPEC §5.12.15)을 가짜 세션 기록·대화 기록으로 견준다.

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use serde_json::{json, Value};

    use crate::readers::HandoffReader;
    use crate::settings::Settings;
    use crate::store::{Probe, Store};

    /// 2026-10-14T17:46:40Z — 세션 기록 updated 의 기준 시각
    const NOW: f64 = 1_792_000_000.0;
    /// 세션을 연 시각 — 표지 시각 전(SessionStart 이 먼저였다는 뜻)
    const STARTED: f64 = NOW - 100.0;
    /// 표지 줄에 적힌 넘긴 시각(NOW - 50)
    const HAND_AT: f64 = NOW - 50.0;
    const HAND_ISO: &str = "2026-10-14T17:45:50Z";
    /// 넘김 표지가 없는 평범한 대화 줄
    const CHAT: &str = r#"{"type":"assistant","message":{"content":"작업한다"}}"#;

    struct Fake {
        root: PathBuf,
        sessions: PathBuf,
        claude: PathBuf,
    }

    impl Fake {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cb-handoff-{}", crate::util::short_id()));
            let sessions = root.join("notifier").join("sessions");
            let claude = root.join(".claude");
            fs::create_dir_all(&sessions).unwrap();
            fs::create_dir_all(&claude).unwrap();
            Fake { root, sessions, claude }
        }

        fn store(&self) -> Store {
            Store::new(self.sessions.clone(), &self.root.join("notifier"), self.claude.clone(), self.root.to_string_lossy().into_owned())
        }

        /// 세션 기록 — 대화 기록은 root/t/<sid>.jsonl 로 가리킨다(내용은 시험마다 쓴다)
        fn session(&self, sid: &str, rec: Value) {
            let t = self.transcript(sid);
            fs::create_dir_all(t.parent().unwrap()).unwrap();
            let mut rec = rec;
            rec["session_id"] = json!(sid);
            rec["transcript_path"] = json!(t.to_string_lossy());
            fs::write(self.sessions.join(format!("{sid}.json")), serde_json::to_vec(&rec).unwrap()).unwrap();
        }

        fn transcript(&self, sid: &str) -> PathBuf {
            self.root.join("t").join(format!("{sid}.jsonl"))
        }

        /// 대화 기록 파일 전체를 쓴다 — 세션 기록 없이 쓰는 시험도 있으므로 폴더를 함께 만든다
        fn write_transcript(&self, sid: &str, body: &str) {
            let t = self.transcript(sid);
            fs::create_dir_all(t.parent().unwrap()).unwrap();
            fs::write(t, body).unwrap();
        }

        /// 대화 기록 끝에 줄을 붙인다 — 크기가 늘어 캐시가 다시 읽는다
        fn append_transcript(&self, sid: &str, line: &str) {
            let mut f = fs::OpenOptions::new().append(true).open(self.transcript(sid)).unwrap();
            writeln!(f).unwrap();
            writeln!(f, "{line}").unwrap();
        }

        /// 백그라운드 넘김 표지 줄(continued-in)을 붙인다
        fn continued_in(&self, sid: &str, to: &str) {
            let line = json!({"type": "continued-in", "continuedInSessionId": to, "timestamp": HAND_ISO});
            self.append_transcript(sid, &line.to_string());
        }

        /// 사람이 이이서 쓴 줄 — 표지 뒤에 붙는다
        fn resumed(&self, sid: &str) {
            let line = json!({"type": "user", "timestamp": HAND_ISO, "message": {"content": "이어서 진행해"}});
            self.append_transcript(sid, &line.to_string());
        }

        fn rec(&self, state: &str, pid: u32, started: f64) -> Value {
            json!({"state": state, "pid": pid, "cwd": "/w/proj", "updated": NOW, "since": NOW - 20.0, "started": started})
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// Windows 는 수정 시각이 타이머 틱 단위라 방금 쓴 파일과 같은 값이 될 수 있다 — 확실히 옮긴다
    fn bump(path: &Path, secs: u64) {
        let f = fs::File::options().append(true).open(path).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(secs)).unwrap();
    }

    /// 보드에 남는 세션 id 들 — reload 가 groups 로 만들고 상태별 개수·트레이도 여기서 나온다
    fn show(st: &mut Store, now: f64) -> Vec<String> {
        let mut settings = Settings::default();
        let r = st.reload(&mut settings, &Probe::default(), &|_| true, false, now);
        let mut ids: Vec<String> = r.groups.iter().flat_map(|g| g.sessions.iter()).map(|s| s.id.clone()).collect();
        ids.sort();
        ids
    }

    #[test]
    fn handoff_reader_caches_by_stamp_and_skips_missing_files() {
        let f = Fake::new();
        let mut r = HandoffReader::default();
        // 파일이 없으면 None — 캐시도 남지 않는다
        assert_eq!(r.read(&f.transcript("none")), None);
        assert_eq!(r.len(), 0);
        // 표지가 있으면 넘긴 세션을 돌려준다
        f.write_transcript("t", CHAT);
        f.continued_in("t", "cont");
        let t = f.transcript("t");
        assert_eq!(r.read(&t).as_ref().map(|x| x.id.as_str()), Some("cont"));
        // 크기·수정 시각이 같으면 다시 읽지 않는다 — 표지를 지우고 같은 길이로 채워도 전 값이 나온다
        let marker_line = json!({"type": "continued-in", "continuedInSessionId": "cont", "timestamp": HAND_ISO}).to_string();
        let body = format!("{CHAT}\n{marker_line}\n");
        let base_user = json!({"type": "user", "message": {"content": ""}}).to_string();
        let user_line = json!({"type": "user", "message": {"content": " ".repeat(marker_line.len() - base_user.len())}}).to_string();
        assert_eq!(body.len(), format!("{CHAT}\n{user_line}\n").len());
        let md = fs::metadata(&t).unwrap();
        let mtime = md.modified().unwrap();
        fs::write(&t, format!("{CHAT}\n{user_line}\n")).unwrap();
        fs::File::options().append(true).open(&t).unwrap().set_modified(mtime).unwrap();
        assert_eq!(r.read(&t).as_ref().map(|x| x.id.as_str()), Some("cont"));
        // 수정 시각이 그대로여도 크기가 달라지면 다시 읽는다
        fs::write(&t, "").unwrap();
        fs::File::options().append(true).open(&t).unwrap().set_modified(mtime).unwrap();
        assert_eq!(r.read(&t), None);
        // 목록에 남지 않은 캐시는 버린다
        r.prune(&HashSet::new());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn handed_off_session_hides_only_while_continuation_is_listed() {
        let f = Fake::new();
        f.session("old", f.rec("waiting", 11, STARTED));
        f.session("near", f.rec("idle", 13, STARTED));
        f.write_transcript("old", CHAT);
        f.write_transcript("near", CHAT);
        f.continued_in("old", "cont");
        let mut st = f.store();
        // 이어 받은 세션 기록이 아직 없으면(hook 이 아직 안 온 순간) 빠지지 않는다
        assert_eq!(show(&mut st, NOW), ["near", "old"]);
        // 이어 받은 세션이 목록에 오면 그때부터 숨는다 — 넘기지 않은 세션은 그대로
        f.session("cont", f.rec("idle", 12, STARTED));
        f.write_transcript("cont", CHAT);
        assert_eq!(show(&mut st, NOW + 1.0), ["cont", "near"]);
    }

    #[test]
    fn session_resumed_by_human_is_not_hidden() {
        let f = Fake::new();
        f.session("old", f.rec("waiting", 11, STARTED));
        f.session("cont", f.rec("idle", 12, STARTED));
        f.write_transcript("old", CHAT);
        f.write_transcript("cont", CHAT);
        f.continued_in("old", "cont");
        let mut st = f.store();
        assert_eq!(show(&mut st, NOW), ["cont"]);
        // 표지 뒤에 사람이 이이서 쓴 줄이 오면 넘긴 세션으로 보지 않는다
        f.resumed("old");
        assert_eq!(show(&mut st, NOW + 1.0), ["cont", "old"]);
    }

    #[test]
    fn session_started_after_marker_time_is_not_hidden() {
        let f = Fake::new();
        // 표지 시각에 열린 세션은 숨고(경계 <=), 그 뒤에 다시 열린 세션(--resume 의 SessionStart 로 started 갱신)은 보인다
        f.session("edge", f.rec("waiting", 11, HAND_AT));
        f.session("reopen", f.rec("waiting", 14, HAND_AT + 1.0));
        f.session("cont", f.rec("idle", 12, STARTED));
        for sid in ["edge", "reopen"] {
            f.write_transcript(sid, CHAT);
            f.continued_in(sid, "cont");
        }
        let mut st = f.store();
        assert_eq!(show(&mut st, NOW), ["cont", "reopen"]);
    }

    #[test]
    fn marker_pointing_to_itself_is_ignored() {
        let f = Fake::new();
        f.session("self", f.rec("waiting", 11, STARTED));
        f.write_transcript("self", CHAT);
        f.continued_in("self", "self");
        let mut st = f.store();
        assert_eq!(show(&mut st, NOW), ["self"]);
    }

    #[test]
    fn ended_handed_off_session_is_hidden_too() {
        let f = Fake::new();
        // 끝난 세션도 대화 기록이 지워지지 않은 몫은 넘긴 세션에서 뺀다(끝난 것도 읽는다)
        f.session("old", f.rec("ended", 11, STARTED));
        f.session("cont", f.rec("idle", 12, STARTED));
        f.write_transcript("old", CHAT);
        f.write_transcript("cont", CHAT);
        f.continued_in("old", "cont");
        let mut st = f.store();
        assert_eq!(show(&mut st, NOW), ["cont"]);
    }

    #[test]
    fn chained_handoff_shows_only_the_last_card() {
        let f = Fake::new();
        for (sid, pid, to) in [("a", 11, "b"), ("b", 12, "c")] {
            f.session(sid, f.rec("waiting", pid, STARTED));
            f.write_transcript(sid, CHAT);
            f.continued_in(sid, to);
        }
        f.session("c", f.rec("idle", 13, STARTED));
        f.write_transcript("c", CHAT);
        let mut st = f.store();
        // a→b→c — 마지막 세션만 보인다
        assert_eq!(show(&mut st, NOW), ["c"]);
        // 이어 받은 마지막 세션 기록이 아직 없으면 b 는 지금처럼 보이고, b 를 가리키는 a 는 그때부터 숨는다
        fs::remove_file(f.sessions.join("c.json")).unwrap();
        assert_eq!(show(&mut st, NOW + 1.0), ["b"]);
    }

    #[test]
    fn handoff_cache_rereads_when_transcript_grows_and_prunes_with_the_list() {
        let f = Fake::new();
        f.session("old", f.rec("waiting", 11, STARTED));
        f.session("cont", f.rec("idle", 12, STARTED));
        f.write_transcript("old", CHAT);
        f.write_transcript("cont", CHAT);
        let mut st = f.store();
        // 표지 없는 대화 기록 — 보인다(가리는 없음도 캐시에는 남는다)
        assert_eq!(show(&mut st, NOW), ["cont", "old"]);
        assert_eq!(st.handoffs.len(), 2);
        // 넘김 표지가 붙으면 다시 읽어 숨는다
        f.continued_in("old", "cont");
        bump(&f.transcript("old"), 2);
        assert_eq!(show(&mut st, NOW + 1.0), ["cont"]);
        assert_eq!(st.handoffs.len(), 2);
        // 세션 기록이 지워져 목록에서 빠지면 그 몫의 캐시도 버린다
        fs::remove_file(f.sessions.join("old.json")).unwrap();
        show(&mut st, NOW + 2.0);
        assert_eq!(st.handoffs.len(), 1);
    }
}


