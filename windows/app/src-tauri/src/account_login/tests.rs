//! `account_login` 시험 — 상태 기계 밖의 부분(이벤트 모양·환경·지우기 확인)과 가짜 로그인 스크립트로 ConPTY 끝까지 도는 시험.

use super::*;
use claude_board_core::Record;
use serde_json::json;
use std::sync::atomic::AtomicUsize;

#[test]
fn event_sends_only_the_fields_that_are_set() {
    let url = AccountLoginEvent { url: Some("https://claude.ai/oauth/authorize?x=1".into()), ..Default::default() };
    assert_eq!(serde_json::to_value(&url).unwrap(), serde_json::json!({"url": "https://claude.ai/oauth/authorize?x=1"}));
    let done = AccountLoginEvent { done: Some(true), error: Some("취소".into()), ..Default::default() };
    assert_eq!(serde_json::to_value(&done).unwrap(), serde_json::json!({"done": true, "error": "취소"}));
    assert_eq!(serde_json::to_value(AccountLoginEvent::default()).unwrap(), serde_json::json!({}));
}

fn pairs(list: &[(&str, &str)]) -> impl Iterator<Item = (OsString, OsString)> {
    list.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect::<Vec<_>>().into_iter()
}

#[test]
fn launch_vars_always_point_at_the_slot_and_forward_only_fake_login_knobs_in_test_mode() {
    let slot = Path::new("C:\\n\\accounts\\login-ab12cd34");
    let env = [("FAKE_LOGIN_LOG", "C:\\t\\log.jsonl"), ("FAKE_LOGIN_CODE", "c1"), ("PATH", "C:\\bin"), ("CLAUDE_CONFIG_DIR", "C:\\Users\\me\\.claude"), ("TOKEN", "secret")];
    let browser = slot.join("open-browser.cmd").to_string_lossy().into_owned();
    let real = launch_vars(slot, false, pairs(&env));
    assert_eq!(
        real,
        [("CLAUDE_CONFIG_DIR".to_string(), slot.to_string_lossy().into_owned()), ("BROWSER".to_string(), browser.clone())],
        "진짜 모드는 보드 환경을 하나도 넘기지 않는다(BROWSER 는 슬롯의 도우미)"
    );
    let fake = launch_vars(slot, true, pairs(&env));
    assert_eq!(
        fake,
        [
            ("CLAUDE_CONFIG_DIR".to_string(), slot.to_string_lossy().into_owned()),
            ("BROWSER".to_string(), browser),
            ("FAKE_LOGIN_LOG".to_string(), "C:\\t\\log.jsonl".to_string()),
            ("FAKE_LOGIN_CODE".to_string(), "c1".to_string()),
        ],
        "보드 환경의 CLAUDE_CONFIG_DIR 는 슬롯을 덮지 못한다"
    );
}

/// 도우미는 주소를 파일에 적기만 한다 — 브라우저·네트워크를 부르는 명령이 없다
#[test]
fn browser_helper_only_appends_its_argument_to_the_url_file() {
    let script = browser_helper_script();
    for forbidden in ["start ", "curl", "powershell", "http", "explorer", "msedge", "chrome"] {
        assert!(!script.to_lowercase().contains(forbidden), "{forbidden}");
    }
    assert!(script.contains(">>\"%~dp0url.txt\" echo(!address!"));
    assert!(script.contains("set \"address=%~1\""), "주소는 큰따옴표 안에 받는다");
    assert!(script.is_ascii() && script.contains("\r\n"));
    // 파일을 쓰는 길: 슬롯 안에 도우미를 쓰고 주소 파일 경로를 알려 준다
    let dir = std::env::temp_dir().join(format!("cb-browser-helper-{}", crate::util::short_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (helper, url_file) = write_browser_helper(&dir).unwrap();
    assert_eq!((helper.clone(), url_file), (dir.join("open-browser.cmd"), dir.join("url.txt")));
    assert_eq!(std::fs::read_to_string(&helper).unwrap(), script);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stop_then_remove_slot_removes_the_slot_even_without_a_running_login() {
    let dir = std::env::temp_dir().join(format!("cb-stop-remove-{}", crate::util::short_id()));
    let slot = dir.join("login-aaaa0001");
    std::fs::create_dir_all(&slot).unwrap();
    std::fs::write(slot.join(".credentials.json"), b"{}").unwrap();
    stop_then_remove_slot(None, &slot, Duration::from_millis(10));
    assert!(!slot.exists());
    // 없는 슬롯을 지워도 오류가 아니다
    stop_then_remove_slot(None, &slot, Duration::from_millis(10));
    std::fs::remove_dir_all(&dir).unwrap();
}

fn saved_account(uuid: &str, email: &str) -> Account {
    let oauth: Record = json!({"accountUuid": uuid, "organizationUuid": "org", "emailAddress": email}).as_object().unwrap().clone();
    Account::new(&oauth, None).unwrap()
}

#[test]
fn delete_asks_with_the_email_and_refuses_unknown_or_current_accounts() {
    let saved = [saved_account("a", "a@x.com"), saved_account("b", "b@x.com")];
    let ask = delete_ask(Some("a|org"), &saved, "b|org").unwrap();
    assert_eq!(ask.title, "b@x.com 계정을 지울까요?");
    assert!(ask.text.contains("다시 쓰려면 계정을 다시 추가"));
    assert_eq!(delete_ask(Some("a|org"), &saved, "a|org"), None, "지금 계정은 지우지 않는다");
    assert_eq!(delete_ask(Some("a|org"), &saved, "ghost|org"), None);
    assert!(delete_ask(None, &saved, "a|org").is_some());
}

#[test]
fn delete_removes_only_after_a_yes() {
    let ask = DeleteAsk { title: "t".into(), text: "x".into() };
    let removed = AtomicUsize::new(0);
    assert!(!delete_after_confirm(&ask, |_| false, || {
        removed.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(removed.load(Ordering::SeqCst), 0, "취소하면 지우지 않는다");
    assert!(delete_after_confirm(&ask, |a| a.title == "t", || {
        removed.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(removed.load(Ordering::SeqCst), 1);
}

// ── 가짜 로그인 스크립트로 ConPTY 끝까지(진짜 claude 는 어디에도 없다). ConPTY 는 Windows 에만 있다 ──

#[cfg(windows)]
mod through_conpty {
    use super::*;
    use crate::accounts::{AccountPaths, LoginPoll};
    use crate::util::short_id;
    use std::time::Instant;

    #[derive(Default)]
    struct TestHost {
        events: Mutex<Vec<AccountLoginEvent>>,
        failures: Mutex<Vec<String>>,
        stopped: AtomicBool,
    }

    impl TestHost {
        fn waiting() -> Arc<Self> {
            Arc::new(TestHost::default())
        }

        /// 보드가 로그인을 접었다(취소·저장·시간 초과)
        fn stop_waiting(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }

        fn events(&self) -> Vec<AccountLoginEvent> {
            self.events.lock().unwrap().clone()
        }

        fn has(&self, wanted: impl Fn(&AccountLoginEvent) -> bool) -> bool {
            self.events().iter().any(wanted)
        }
    }

    impl Host for TestHost {
        fn emit(&self, event: AccountLoginEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn pending(&self) -> bool {
            !self.stopped.load(Ordering::SeqCst)
        }

        fn fail(&self, message: &str) {
            self.failures.lock().unwrap().push(message.to_string());
            self.stop_waiting();
        }
    }

    fn wait_until(what: &str, limit: Duration, condition: impl Fn() -> bool) {
        let started = Instant::now();
        while !condition() {
            assert!(started.elapsed() < limit, "기다리다 시간이 지났다: {what}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn find_on_path(name: &str) -> PathBuf {
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("{name} 이 PATH 에 없다 — 가짜 로그인 시험에는 node 가 필요하다"))
    }

    /// 가짜 로그인 스크립트 — 저장소의 windows/ci
    fn fake_script() -> PathBuf {
        // canonicalize 는 `\\?\` 접두 경로를 만들어 node 가 못 읽을 수 있어 `..` 를 그대로 둔다
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("ci").join("fake-claude-login.cjs");
        assert!(script.is_file(), "가짜 로그인 스크립트가 없다: {}", script.display());
        script
    }

    struct Scratch {
        root: PathBuf,
        slot: PathBuf,
        log: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cb-account-login-{}", short_id()));
            let slot = root.join("accounts").join("login-test0001");
            std::fs::create_dir_all(&slot).unwrap();
            Scratch { log: root.join("fake-login.jsonl"), root, slot }
        }

        /// 시험 모드 결정(`Decision::On`)으로 고른 가짜를 slot 에 띄운다 — 진짜 claude 를 찾는 길은 아예 넘기지 않는다
        fn launch(&self, extra_vars: &[(&str, &str)]) -> Launch {
            let decided = Decision::On(vec![find_on_path("node.exe"), fake_script()]);
            let never_real = || -> Option<PathBuf> { panic!("시험이 진짜 claude 를 찾았다") };
            let (program, mut args) = test_claude::login_program(decided, false, never_real).unwrap();
            args.extend(["auth".to_string(), "login".into(), "--claudeai".into()]);
            let mut vars = launch_vars(&self.slot, false, std::iter::empty());
            vars.push(("FAKE_LOGIN_LOG".into(), self.log.to_string_lossy().into_owned()));
            vars.extend(extra_vars.iter().map(|(k, v)| (k.to_string(), v.to_string())));
            Launch { program, args, cwd: std::env::temp_dir(), vars, url_file: Some(self.slot.join("url.txt")) }
        }

        fn log_events(&self) -> Vec<serde_json::Value> {
            std::fs::read_to_string(&self.log).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
        }

        fn account_paths(&self) -> AccountPaths {
            let claude_dir = self.root.join("claude-home");
            std::fs::create_dir_all(&claude_dir).unwrap();
            AccountPaths {
                credentials: claude_dir.join(".credentials.json"),
                claude_json: self.root.join(".claude.json"),
                meta: self.root.join("notifier").join("accounts.json"),
                root: self.root.join("accounts"),
                claude_dir,
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const LIMIT: Duration = Duration::from_secs(20);

    #[test]
    fn right_code_runs_the_whole_flow_through_a_real_conpty() {
        let scratch = Scratch::new();
        let host = TestHost::waiting();
        let session = start(host.clone(), &scratch.launch(&[]), Duration::from_millis(50)).unwrap();

        // 주소 — 한 번만, 가짜의 예약 도메인
        wait_until("로그인 주소", LIMIT, || host.has(|e| e.url.is_some()));
        let url = session.url().expect("알려 준 주소");
        assert!(url.starts_with("https://login.fake.test/oauth/authorize?"), "{url}");
        assert!(login_window::login_url(&url).is_some());

        // 코드를 쓰면 가짜가 슬롯에 계정을 쓰고 0 으로 끝난다
        session.submit_code("  fake-code-1234 ");
        wait_until("명령 종료", LIMIT, || session.finished());
        let events = host.events();
        assert_eq!(events.iter().filter(|e| e.url.is_some()).count(), 1);
        assert!(events.iter().any(|e| e.note.as_deref() == Some("코드를 보냈습니다. 확인하는 중…")));
        assert!(events.iter().any(|e| e.note.as_deref() == Some("로그인을 마쳤습니다. 계정 정보를 확인하는 중…")), "{events:?}");
        assert!(host.failures.lock().unwrap().is_empty());

        // 보드의 슬롯 지켜보기(poll_login)가 그 슬롯에서 계정을 찾는다 — 사용자 폴더가 아니라 임시 폴더만 쓴다
        let paths = scratch.account_paths();
        let LoginPoll::Done(Ok(account)) = accounts::poll_login(&paths, &scratch.slot) else { panic!("슬롯에서 계정을 찾지 못했다") };
        assert_eq!(account.email(), "fake-login@example.test");

        // 보드가 로그인을 끝내면 끝났다고 한 번 알린다
        host.stop_waiting();
        wait_until("끝남 알림", LIMIT, || host.has(|e| e.done == Some(true)));
        assert_eq!(host.events().iter().filter(|e| e.done == Some(true)).count(), 1);
        assert!(host.events().iter().all(|e| e.error.is_none()));

        // 가짜가 받은 것 — 인자와 슬롯, 코드는 앞뒤 공백 없이 한 번
        let log = scratch.log_events();
        let start = log.iter().find(|e| e["event"] == "start").expect("start 기록");
        assert_eq!(start["argv"], json!(["auth", "login", "--claudeai"]));
        assert_eq!(start["slot"], json!(scratch.slot.to_string_lossy()));
        assert_eq!(log.iter().filter(|e| e["event"] == "code").count(), 1);
        assert_eq!(log.iter().find(|e| e["event"] == "code").unwrap()["code"], "fake-code-1234");
    }

    #[test]
    fn wrong_code_fails_with_the_reason_and_leaves_the_slot_empty() {
        let scratch = Scratch::new();
        let host = TestHost::waiting();
        let session = start(host.clone(), &scratch.launch(&[]), Duration::from_millis(50)).unwrap();
        wait_until("로그인 주소", LIMIT, || host.has(|e| e.url.is_some()));
        session.submit_code("wrong-code");
        wait_until("끝남 알림", LIMIT, || host.has(|e| e.done == Some(true)));
        let message = "계정을 추가하지 못했습니다: Login failed: Invalid authorization code";
        assert_eq!(*host.failures.lock().unwrap(), [message]);
        let done = host.events().into_iter().find(|e| e.done == Some(true)).unwrap();
        assert_eq!(done.error.as_deref(), Some(message));
        assert!(!scratch.slot.join(".credentials.json").exists());
        assert!(matches!(accounts::poll_login(&scratch.account_paths(), &scratch.slot), LoginPoll::Waiting));
    }

    #[test]
    fn the_board_giving_up_kills_the_command_and_reports_done_without_an_error() {
        let scratch = Scratch::new();
        let host = TestHost::waiting();
        let session = start(host.clone(), &scratch.launch(&[]), Duration::from_millis(50)).unwrap();
        wait_until("로그인 주소", LIMIT, || host.has(|e| e.url.is_some()));
        // 취소·15분 한도·로그인 창 닫기 — 보드가 접는다
        host.stop_waiting();
        wait_until("끝남 알림", LIMIT, || host.has(|e| e.done == Some(true)));
        wait_until("명령 종료", LIMIT, || session.finished());
        assert!(host.failures.lock().unwrap().is_empty(), "접은 로그인의 종료는 실패로 치지 않는다");
        assert!(host.events().iter().all(|e| e.error.is_none()));
        assert!(scratch.log_events().iter().all(|e| e["event"] != "code"));
        // 접은 뒤 온 코드는 아무 데도 쓰이지 않는다
        session.submit_code("fake-code-1234");
        std::thread::sleep(Duration::from_millis(200));
        assert!(scratch.log_events().iter().all(|e| e["event"] != "code"));
    }

    #[test]
    fn a_command_that_finishes_by_itself_waits_for_the_board_to_pick_up_the_slot() {
        // 브라우저 콜백으로 저절로 끝나는 길 — 코드를 쓰지 않는다
        let scratch = Scratch::new();
        let host = TestHost::waiting();
        let session = start(host.clone(), &scratch.launch(&[("FAKE_LOGIN_AUTO_MS", "300")]), Duration::from_millis(50)).unwrap();
        wait_until("명령 종료", LIMIT, || session.finished());
        assert!(host.has(|e| e.note.as_deref() == Some("로그인을 마쳤습니다. 계정 정보를 확인하는 중…")));
        assert!(!host.has(|e| e.done == Some(true)), "보드가 접기 전에는 끝났다고 알리지 않는다");
        assert!(matches!(accounts::poll_login(&scratch.account_paths(), &scratch.slot), LoginPoll::Done(Ok(_))));
        host.stop_waiting();
        wait_until("끝남 알림", LIMIT, || host.has(|e| e.done == Some(true)));
    }

    #[test]
    fn spawn_failure_is_an_error_not_a_hang() {
        let host = TestHost::waiting();
        let launch = Launch { program: PathBuf::from("C:\\no-such-dir\\no-such.exe"), args: vec![], cwd: std::env::temp_dir(), vars: vec![], url_file: None };
        assert!(start(host, &launch, Duration::from_millis(50)).is_err());
    }

    /// 실제 cmd 가 도우미를 돌려도 주소만 파일에 적힌다 — `&`·`>`·`%` 가 든 주소가 명령으로 읽히지 않는다(큰따옴표로 감싼 인자, 셸이 하는 그대로)
    #[test]
    fn the_helper_run_by_cmd_writes_only_the_address_and_runs_nothing_else() {
        use std::os::windows::process::CommandExt;
        let dir = std::env::temp_dir().join(format!("cb-helper-run-{}", crate::util::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (helper, url_file) = write_browser_helper(&dir).unwrap();
        // `&echo injected>pwned.txt` 가 명령으로 읽히면 cwd 에 pwned.txt 가 생긴다
        let hostile = "https://claude.ai/oauth/authorize?client_id=a&response_type=code&scope=user%3Aprofile&x=1&echo injected>pwned.txt";
        let calm = "https://claude.ai/oauth/authorize?code=true&state=s%2F1";
        for url in [hostile, calm] {
            let status = std::process::Command::new("cmd.exe")
                .raw_arg(format!("/d /s /c \"\"{}\" \"{url}\"\"", helper.display()))
                .current_dir(&dir)
                .status()
                .unwrap();
            assert!(status.success(), "{url}");
        }
        let written = std::fs::read_to_string(&url_file).unwrap();
        assert_eq!(written.lines().collect::<Vec<_>>(), [hostile, calm], "받은 주소가 그대로 한 줄씩");
        assert!(!dir.join("pwned.txt").exists(), "주소의 &·> 가 명령으로 실행됐다");
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["open-browser.cmd", "url.txt"], "도우미는 다른 파일을 만들지 않는다");
        // 파서가 그 파일에서 주소를 읽는다
        let mut machine = Machine::default();
        let effects = machine.on_url_file(&written);
        assert_eq!(effects.len(), 1, "{effects:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 주소가 출력에는 안 나오고 `BROWSER` 도우미 파일로만 오는 길 — 같은 감시 반복이 파일에서 주소를 찾는다
    #[test]
    fn the_url_file_is_read_by_the_watch_loop_when_the_output_has_no_address() {
        let scratch = Scratch::new();
        let host = TestHost::waiting();
        // 아무것도 출력하지 않고 기다리는 명령(진짜 claude 가 아니다)
        let launch = Launch {
            program: find_on_path("node.exe"),
            args: vec!["-e".to_string(), "setTimeout(() => {}, 20000)".to_string()],
            cwd: std::env::temp_dir(),
            vars: vec![],
            url_file: Some(scratch.slot.join("url.txt")),
        };
        let session = start(host.clone(), &launch, Duration::from_millis(50)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(!host.has(|e| e.url.is_some()));
        // 도우미가 쓰다 만 줄은 아직 주소가 아니다
        std::fs::write(scratch.slot.join("url.txt"), "https://login.fake.test/oauth/auth").unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(!host.has(|e| e.url.is_some()), "{:?}", host.events());
        let url = "https://login.fake.test/oauth/authorize?client_id=fake&state=zz";
        std::fs::write(scratch.slot.join("url.txt"), format!("{url}\r\n")).unwrap();
        wait_until("파일에서 찾은 주소", LIMIT, || host.has(|e| e.url.is_some()));
        assert_eq!(session.url().as_deref(), Some(url));
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(host.events().iter().filter(|e| e.url.is_some()).count(), 1, "한 번만 알린다");
        host.stop_waiting();
        wait_until("명령 종료", LIMIT, || session.finished());
    }

    /// 접을 때는 프로세스를 먼저 죽이고 끝난 것을 확인한 뒤 슬롯을 지운다 — 가짜가 나중에 슬롯에 쓰려던 계정 파일이 다시 생기지 않는다
    #[test]
    fn stopping_before_removing_the_slot_leaves_nothing_for_the_login_to_recreate() {
        let scratch = Scratch::new();
        let host = TestHost::waiting();
        // 가짜는 1.5초 뒤 슬롯에 계정·토큰을 쓰려 한다
        let session = start(host.clone(), &scratch.launch(&[("FAKE_LOGIN_AUTO_MS", "1500")]), Duration::from_secs(60)).unwrap();
        wait_until("로그인 주소", LIMIT, || host.has(|e| e.url.is_some()));
        stop_then_remove_slot(Some(&session), &scratch.slot, Duration::from_secs(5));
        assert!(session.exited(), "슬롯을 지우기 전에 프로세스가 끝나 있어야 한다");
        assert!(!scratch.slot.exists());
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!scratch.slot.exists(), "죽은 로그인이 슬롯을 다시 만들었다");
        assert!(scratch.log_events().iter().all(|e| e["event"] != "write"), "계정 파일을 쓰는 데까지 갔다");
        // 이미 끝난 것을 다시 거둬도 같다
        assert!(session.stop_and_wait(Duration::from_millis(100)));
    }
}
