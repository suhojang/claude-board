//! open-tab 요청 폴더의 잡초 뽑기(drain) — `<요청 id>.req.json` 을 읽어 검사(`open_tab_prepare`)·판정(`plan_open_tab`)하고,
//! 가리기·띄우기 동작을 부른 쪽(`act`)에 물린 뒤 답을 `<요청 id>.reply.json` 에 쓰고 요청을 지운다.
//! mac `Sources/ClaudeBoard/OpenTab.swift` 의 takeRequests·handleOpenTabRequest(188~247 줄) 대응이다.
//!
//! 동작 값(act)의 약속 — 앱이 동작을 이행했으면 `None`, 이행하지 못했으면 그 문구(`Some(이유)`)를 돌려준다.
//! 문구는 refused 답의 `why` 로 그대로 쓰인다(mac 은 대화 기록 검사를 안에서 하지만, Windows 판은 세션 상태를
//! 앱이 알아야 하므로 검사를 act 쪽에 둔다). 동작의 `command` 는 `--session-id` 가 이미 끼워진 낱말 argv 다.
//!
//! mac 과 다른 점(Windows 판 결정):
//! - mac 은 요청을 읽는 곳에서 바로 지우지만, Windows 판은 답을 쓴 뒤 지운다. 겹쳐 부르면 두 번 읽히므로
//!   drain 을 부르는 쪽(wiring)이 같은 순간에 한 번만 부르게 해야 한다.
//! - 답을 못 썼을 때도 요청은 지운다 — 동작은 이미 시전됐고 남겨 두면 다음 틱에 두 번 띄운다.
//!   답이 없으면 명령줄 도구가 기다림을 끝내고 "탭이 열렸을 수 있습니다" 로 끝난다(mac 도 같다).
//! - 파일 크기 상한은 준비 상한의 두 배(mac 으로부터 — 64 KiB 로 두면 64~256 KiB 의 첫 요청이 답도 없이 사라진다).
//! - 권한(소유자 uid) 검사는 Windows 에 없다 — 심볼릭 링크·정션(재해석 지점)은 읽지 않고, 읽지 못할 요청 파일은
//!   다음 틱에 다시 읽히지 않게 지운다. 폴더·링크는 남의 것일 수 있어 지우지 않는다.
//! - 이 모듈은 board_log 를 부르지 않는다(시험이 진짜 board.log 를 더럽힌다). 처리한 요청의 답은 돌려주는 값으로
//!   부른 쪽이 남기고, 버린 요청(오래됨·못 읽음·이름 어긋남)은 요청 파일이 사라진 것으로 드러난다.
//! - exists 답의 `tab` 칸을 None 으로 두었다(mac 은 탭의 보이는 이름을 실지만, `OpenTabScreen` 에는 지금 이름·
//!   바꿀 이름 등이 섞여 있어 어느 것이 탭에 보이는지 모른다. 실제 탭·세션은 `open_tab_done` 이 알아 낸다).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use claude_board_core::clock;

use super::args::{open_tab_stale, OpenTabFailure, OpenTabReply, OpenTabRequest};
use super::plan::{
    is_session_id, open_tab_prepare, plan_open_tab, LiveClaude, OpenTabPlan, OpenTabPlanInput,
    OpenTabScreen, OPEN_TAB_MAX_BYTES,
};
use crate::mdrender::is_link;
use crate::util::mtime;

/// 요청 파일 이름 뒷부분 — args.rs 의 요청 폴더 약속(`<요청 id>.req.json`)
const REQ_SUFFIX: &str = ".req.json";

/// 요청 파일 읽기 상한 — 준비 상한의 두 배(mac takeRequests 의 `maxBytes: openTabMaxBytes * 2`).
/// 이보다 큰 파일은 읽지 않고 버린다(준비 단계의 `너무 깁니다` bad-args 답이 256 KiB 까지는 돌아가게).
pub const OPEN_TAB_READ_MAX_BYTES: usize = OPEN_TAB_MAX_BYTES * 2;

/// 요청 폴더에 쌓인 옛 잡파일(엎어진 답·쓰다 남은 임시 파일)을 지우는 나이 — mac takeRequests 의 600초
const LEFTOVER_MAX_AGE: f64 = 600.0;

/// drain 이 부르는 동작 — 가리기·새 탭·남은 셸에 다시 띄우기
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTabAction {
    /// 새 탭을 열어 명령을 끼워 넣는다
    Open { cwd: String, title: Option<String>, command: Vec<String>, no_focus: bool },
    /// 이미 있는 화면의 셸에 명령을 끼워 넣는다
    Reuse { id: String, command: Vec<String>, no_focus: bool },
    /// 이미 도는 화면을 앞으로 가리키기만 한다
    Focus { id: String },
}

/// 요청 폴더를 훑어 요청을 처리하고 `(<요청 id>, 답)` 쌍을 온 순서(`at` 빠른 것부터)로 돌려준다.
/// 답은 파일로도 쓰고 요청 파일은 지운다. 폴더가 없으면(아직 요청이 없으므로) 아무것도 하지 않는다.
pub fn drain(
    dir: &Path,
    screens: &dyn Fn() -> Vec<OpenTabScreen>,
    live: &dyn Fn(Option<&str>) -> Vec<LiveClaude>,
    act: &mut dyn FnMut(OpenTabAction) -> Option<String>,
) -> Vec<(String, OpenTabReply)> {
    let mut pairs: Vec<(String, OpenTabReply)> = vec![];
    // at 이 빠른 것부터 — 같은 세션 요청이 겹쳐도 먼저 온 요청이 먼저 뜬다(mac 도 at 오름차순)
    for request in read_requests(dir) {
        let reply = handle_request(&request, screens, live, act);
        write_reply(dir, &request.id, &reply);
        let _ = fs::remove_file(request_file(dir, &request.id));
        pairs.push((request.id, reply));
    }
    pairs
}

/// 요청 파일 하나의 경로
fn request_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}{REQ_SUFFIX}"))
}

/// 요청 폴더 훑기 — 요청 파일만 남겨 `at` 오름차순으로 돌려준다.
/// 못 받을 요청(이름이 어긋남·링크·너무 큼·못 읽음·못 풀음·id 어긋남·오래됨)은 지우고 버려
/// 다음 틱에 다시 읽히지 않게 한다(mac 은 읽는 곳에서 미리 지우므로 같은 결과다).
fn read_requests(dir: &Path) -> Vec<OpenTabRequest> {
    let entries = match fs::read_dir(dir) {
        Ok(iter) => iter,
        Err(_) => return vec![],
    };
    let now = clock::now();
    let mut found: Vec<OpenTabRequest> = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().into_string().ok() else { continue };
        if !name.ends_with(REQ_SUFFIX) {
            remove_old_stray(&path, now);
            continue;
        }
        let Some(id) = name.strip_suffix(REQ_SUFFIX) else { unreachable!("뒷부분을 이미 걸렀다") };
        if !is_session_id(id) {
            discard_request_file(&path);
            continue;
        }
        // 폴더·링크는 지우지 않고 지나간다
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(true) || is_link(&path) {
            continue;
        }
        let too_big = fs::metadata(&path).map(|meta| meta.len() as usize > OPEN_TAB_READ_MAX_BYTES).unwrap_or(true);
        if too_big {
            discard_request_file(&path);
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                discard_request_file(&path);
                continue;
            }
        };
        let request: OpenTabRequest = match serde_json::from_slice(&bytes) {
            Ok(request) => request,
            Err(_) => {
                discard_request_file(&path);
                continue;
            }
        };
        // 파일 안의 id 가 이름과 어긋나면 부른 쪽이 기다리지 않는 요청이다
        if request.id != id {
            discard_request_file(&path);
            continue;
        }
        // 명령줄 도구가 기다림을 끝내고 떠난 요청은 답을 쓸 곳이 없다 — 지우고 버린다(mac 도 같다)
        if open_tab_stale(request.at, now) {
            discard_request_file(&path);
            continue;
        }
        found.push(request);
    }
    found.sort_by(|a, b| a.at.total_cmp(&b.at));
    found
}

/// 요청 폴더의 옛 잡파일(엎어진 답·쓰다 남은 임시 파일)을 지운다 — 최근 것은 건드리지 않는다(mac takeRequests 와 같다)
fn remove_old_stray(path: &Path, now: f64) {
    match mtime(path) {
        Some(t) if now - t <= LEFTOVER_MAX_AGE => {}
        _ => {
            let _ = fs::remove_file(path);
        }
    }
}

/// 몇 가지 죄 가운데 하나라 걸린 요청 파일을 지워 다음 틱의 반복 읽기를 끊는다
fn discard_request_file(path: &Path) {
    let _ = fs::remove_file(path);
}

/// 요청 하나를 검사·판정해 동작으로 물리고 답을 꾸민다 — 검사는 요청 cwd 로, 화면 폴더 견주기는
/// 바로 가기를 푼 꼴(`real_dir`)로 각각 본다(mac 도 standardized cwd 로 열고 realDir 로 견주는 것과 같다)
fn handle_request(
    request: &OpenTabRequest,
    screens: &dyn Fn() -> Vec<OpenTabScreen>,
    live: &dyn Fn(Option<&str>) -> Vec<LiveClaude>,
    act: &mut dyn FnMut(OpenTabAction) -> Option<String>,
) -> OpenTabReply {
    let prep = match open_tab_prepare(&request.cwd, request.title.as_deref(), &request.argv, &new_session_id()) {
        Ok(prep) => prep,
        Err(failure) => return failure.reply(),
    };
    // 폴더가 없거나 파일이면 어차피 못 열어 거절한다(mac 이 openTabPrepare 뒤에 폴더를 확인하는 것과 같다)
    if !fs::metadata(&request.cwd).is_ok_and(|meta| meta.is_dir()) {
        return OpenTabFailure::BadArgs(format!("폴더가 없습니다: {}", request.cwd)).reply();
    }
    let real = real_dir(&request.cwd);
    let shot = screens();
    let input = OpenTabPlanInput { cwd: real, ..prep.clone() };
    let plan = plan_open_tab(&input, &shot, &live(prep.session_id.as_deref()));
    match plan {
        OpenTabPlan::Refuse(why) => OpenTabFailure::Refused(why).reply(),
        OpenTabPlan::Exists(id) => {
            // 이미 도는 claude 는 대화 기록이 있으므로 거절할 검사가 없다 — 뒤에서 열기가 아니면 가리킨다
            if request.reveals() {
                if let Some(why) = act(OpenTabAction::Focus { id: id.clone() }) {
                    return OpenTabFailure::Refused(why).reply();
                }
            }
            let screen = shot.iter().find(|screen| screen.id == id);
            OpenTabReply {
                ok: true,
                pid: screen.and_then(|screen| screen.pid),
                session: screen.and_then(|screen| screen.session_id.clone()),
                status: "exists".to_string(),
                tab: None,
                why: None,
            }
        }
        OpenTabPlan::Reuse(id) => {
            let no_focus = !request.reveals();
            // 이어 열 세션의 대화 기록이 있는지 등 앱이 아는 검사는 act 쪽이 본다 — 못 이행하면 그 문구로 거절답
            if let Some(why) = act(OpenTabAction::Reuse { id: id.clone(), command: prep.argv.clone(), no_focus }) {
                return OpenTabFailure::Refused(why).reply();
            }
            OpenTabReply {
                ok: true,
                pid: None,
                session: prep.session_id.clone(),
                status: "reopened".to_string(),
                tab: prep.name.clone(),
                why: None,
            }
        }
        OpenTabPlan::Open => {
            let no_focus = !request.reveals();
            if let Some(why) = act(OpenTabAction::Open {
                cwd: request.cwd.clone(),
                title: prep.name.clone(),
                command: prep.argv.clone(),
                no_focus,
            }) {
                return OpenTabFailure::Refused(why).reply();
            }
            OpenTabReply {
                ok: true,
                pid: None,
                session: prep.session_id.clone(),
                status: "opened".to_string(),
                tab: prep.name.clone(),
                why: None,
            }
        }
    }
}

/// 답을 임시 파일에 쓰고 바꿔 끼워 답 파일을 만든다 — 쓰다 끊겨도 반쪽 답이 남지 않게(mdrender.rs 와 같은 임시 꼴).
/// 실패는 조용히 두는데, 답이 없으면 명령줄 도구의 기다림이 그렇게 끝나기 때문이다. 요청은 drain 이 어쨌든 지운다.
fn write_reply(dir: &Path, id: &str, reply: &OpenTabReply) {
    let at = dir.join(format!("{id}.reply.json"));
    let partial = at.with_extension("cb-partial");
    if fs::write(&partial, reply.line().as_bytes()).is_err() {
        return;
    }
    if fs::rename(&partial, &at).is_err() {
        let _ = fs::remove_file(&partial);
    }
}

/// 새 세션 id(UUID 꼴) — uuid 의존성을 더하지 않고 시각·프로세스·이번 drain 안의 차례를 섞어 해시로 만든다.
/// mac 은 `UUID().uuidString.lowercased()` 로 만든다(OpenTab.swift). `is_session_id` 는 자리수·16진수만 보므로 꼴은 성립한다
fn new_session_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(clock::now().to_bits().to_be_bytes());
    hasher.update(std::process::id().to_be_bytes());
    hasher.update(seq.to_be_bytes());
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// 바로 가기·`.·..`·끝 빗금·드라이브 글자 쓰기를 풀어 한 꼴로 만든다 — 캐논컬라이즈(autopilot_git.rs 의
/// strip_verbatim 과 같다)해 `\\?\` 를 벗긴다. 폴더가 없는 등 풀지 못하면 풀지 않은 것을 돌려준다.
/// 화면 쪽 cwd 도 같은 도구로 풀어 넘겨야 견주기가 성립한다(wiring 조각이 이 함수를 다시 쓴다).
pub fn real_dir(path: &str) -> String {
    match fs::canonicalize(path) {
        Ok(real) => strip_verbatim(&real.to_string_lossy()),
        Err(_) => path.to_string(),
    }
}

/// `\\?\C:\x` → `C:\x`. 그 밖(`\\?\UNC\…` 포함)은 그대로 — git 이 받는 꼴로 되돌리는 autopilot_git.rs 와 같다
fn strip_verbatim(p: &str) -> String {
    match p.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => p.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("cb-opentab-handle-{}", crate::util::short_id()));
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 요청 id·화면 id 로 쓰는 세션 id 꼴(mac 시험 값)
    const SID: &str = "0f0e1d2c-3b4a-5968-7766-554433221100";
    const A: &str = "00000000-0000-0000-0000-00000000000a";
    const B: &str = "00000000-0000-0000-0000-00000000000b";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    fn request(id: &str, cwd: &str, argv: Vec<String>) -> OpenTabRequest {
        OpenTabRequest {
            id: id.to_string(),
            cwd: cwd.to_string(),
            title: Some("역할".to_string()),
            argv,
            at: clock::now(),
            no_focus: None,
        }
    }

    /// 실제 폴더를 만들어 그 real_dir 꼴로 돌려준다 — 요청 cwd 와 화면 cwd 가 같은 꼴이어야
    /// 폴더 검사와 plan 폴더 견주기가 다 같이 통과한다
    fn make_cwd(scratch: &Scratch, at: &str) -> String {
        let dir = scratch.path(at);
        fs::create_dir_all(&dir).unwrap();
        real_dir(&dir.to_string_lossy())
    }

    /// 요청 파일을 명령줄 도구처럼 쓴다
    fn put(dir: &Path, id: &str, request: &OpenTabRequest) {
        fs::write(request_file(dir, id), serde_json::to_string(request).unwrap()).unwrap();
    }

    fn reply_of(dir: &Path, id: &str) -> OpenTabReply {
        serde_json::from_str(&fs::read_to_string(dir.join(format!("{id}.reply.json"))).unwrap()).unwrap()
    }

    /// drain 한 번 부르기 — 화면·살아 있는 claude 를 정해 준 값으로
    fn run(
        dir: &Path,
        screens: Vec<OpenTabScreen>,
        live: Vec<LiveClaude>,
        act: &mut dyn FnMut(OpenTabAction) -> Option<String>,
    ) -> Vec<(String, OpenTabReply)> {
        drain(dir, &move || screens.clone(), &move |_| live.clone(), act)
    }

    fn screen(id: &str, session: Option<&str>, cwd: &str) -> OpenTabScreen {
        OpenTabScreen {
            id: id.to_string(),
            session_id: session.map(str::to_string),
            cwd: cwd.to_string(),
            pid: None,
            names: vec![],
            busy: false,
            takeover: false,
        }
    }

    // ── 네 갈래 답 ──

    /// 새 세션 요청 — Open 동작 하나(--session-id 가 끼워진 명령)와 opened 답, 요청을 지우고 답만 남는다
    #[test]
    fn opened_request_dispatches_a_new_tab_and_writes_the_reply() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        put(&dir, SID, &request(SID, &cwd, args(&["claude", "--name", "역할"])));

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert_eq!(pairs.len(), 1, "요청 하나에 답 하나");
        let (id, reply) = &pairs[0];
        assert_eq!(id, SID);
        assert!(reply.ok);
        assert_eq!(reply.status, "opened");
        assert_eq!(reply.tab.as_deref(), Some("역할"));
        assert_eq!(reply.pid, None);
        let session = reply.session.clone().expect("새 세션의 id 가 실린다");
        assert!(is_session_id(&session));

        let [action] = actions.as_slice() else { panic!("동작 하나여야 한다: {actions:?}") };
        match action {
            OpenTabAction::Open { cwd: action_cwd, title, command, no_focus } => {
                assert_eq!(action_cwd, &cwd);
                assert_eq!(title.as_deref(), Some("역할"));
                assert_eq!(*command, args(&["claude", "--session-id", session.as_str(), "--name", "역할"]));
                assert!(!*no_focus);
            }
            other => panic!("Open 동작이어야 한다: {other:?}"),
        }

        // 요청은 지워지고 답 파일만 남는다 — 내용은 돌려준 답과 같은 한 줄 JSON, 임시 파일은 없다
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(leftovers, vec![format!("{SID}.reply.json")]);
        assert_eq!(fs::read_to_string(dir.join(format!("{SID}.reply.json"))).unwrap(), reply.line());
        assert_eq!(reply_of(&dir, SID), *reply);
    }

    /// 이미 도는 탭 — 가리기 동작 하나와 pid 가 실린 exists 답
    #[test]
    fn exists_request_points_at_the_running_tab_and_replies_pid() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        let mut busy = screen(A, Some(SID), &cwd);
        busy.pid = Some(4242);
        busy.busy = true;
        put(&dir, SID, &request(SID, &cwd, args(&["claude", "--resume", SID])));

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![busy], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert_eq!(pairs.len(), 1);
        let (id, reply) = &pairs[0];
        assert_eq!(id, SID);
        assert!(reply.ok);
        assert_eq!(reply.status, "exists");
        assert_eq!(reply.pid, Some(4242), "화면이 돌리는 claude 의 pid");
        assert_eq!(reply.session.as_deref(), Some(SID));
        assert_eq!(reply.tab, None, "보이는 탭 이름은 open_tab_done 이 알아 내는 것으로 두었다");
        assert_eq!(actions, vec![OpenTabAction::Focus { id: A.to_string() }], "이미 도는 탭은 가리기만 한다");
        assert!(!request_file(&dir, SID).exists(), "요청은 답을 쓴 뒤 지운다");
    }

    /// 뒤에서 열기(--no-focus) 요청이 이미 도는 탭이면 — 아무 동작도 나가지 않고 exists 답만 쓴다
    #[test]
    fn no_focus_existing_tab_replies_exists_without_any_action() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        let mut busy = screen(A, Some(SID), &cwd);
        busy.pid = Some(7);
        busy.busy = true;
        let mut req = request(SID, &cwd, args(&["claude", "--resume", SID]));
        req.no_focus = Some(true);
        put(&dir, SID, &req);

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![busy], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert!(actions.is_empty(), "골라 키보드 포커스를 옮기지 않는다");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].1.status, "exists");
        assert_eq!(pairs[0].1.pid, Some(7));
    }

    /// claude 가 끝나 셸만 남은 같은 폴더 화면 — 그 화면에 다시 띄우는 Reuse 동작과 reopened 답
    #[test]
    fn reused_shell_only_tab_reruns_claude_in_that_tab() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        let idle = screen(A, Some(SID), &cwd);
        put(&dir, SID, &request(SID, &cwd, args(&["claude", "--resume", SID])));

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![idle], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert_eq!(pairs.len(), 1);
        let (_, reply) = &pairs[0];
        assert_eq!(reply.status, "reopened");
        assert_eq!(reply.session.as_deref(), Some(SID));
        assert_eq!(reply.tab.as_deref(), Some("역할"));
        let [action] = actions.as_slice() else { panic!("동작 하나여야 한다: {actions:?}") };
        match action {
            OpenTabAction::Reuse { id, command, no_focus } => {
                assert_eq!(id, A);
                assert_eq!(*command, args(&["claude", "--resume", SID]), "이어 열기는 요청 명령을 그대로");
                assert!(!*no_focus);
            }
            other => panic!("Reuse 동작이어야 한다: {other:?}"),
        }
    }

    /// 보드 탭 밖에서 같은 세션이 돌고 있으면 — 거절답이 쓰이고 동작은 나가지 않는다
    #[test]
    fn outside_session_request_is_refused_without_any_action() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        put(&dir, SID, &request(SID, &cwd, args(&["claude", "--resume", SID])));

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(
            &dir,
            vec![],
            vec![LiveClaude { pid: 33, session: Some(SID.to_string()), name: None }],
            &mut |action| {
                actions.push(action);
                None
            },
        );

        assert_eq!(pairs.len(), 1);
        let (_, reply) = &pairs[0];
        assert_eq!(reply.status, "refused");
        assert_eq!(reply.why.as_deref(), Some("이 세션이 보드 탭 밖(pid 33)에서 돌고 있습니다"));
        assert!(actions.is_empty(), "거절되면 동작을 부르지 않는다");
    }

    /// act 가 이행하지 못한 문구 — refused 답의 why 로 그대로 옮겨 부른 쪽에 알린다
    #[test]
    fn act_refusal_becomes_the_replys_why() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        put(&dir, SID, &request(SID, &cwd, args(&["claude", "--name", "역할"])));

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            Some("이어 열 대화 기록이 없습니다".to_string())
        });

        assert_eq!(pairs.len(), 1);
        let (_, reply) = &pairs[0];
        assert_eq!(reply.status, "refused");
        assert_eq!(reply.why.as_deref(), Some("이어 열 대화 기록이 없습니다"));
        assert!(
            matches!(actions.first(), Some(OpenTabAction::Open { .. })),
            "claude 단독 인자는 새 세션이라 Open 동작이다"
        );
        assert!(!request_file(&dir, SID).exists(), "거절답을 쓴 뒤에도 요청은 지운다");
    }

    /// 명령줄 도구가 기다림을 끝내고 간 오래된 요청 — 지우고 답도 동작도 없다
    #[test]
    fn stale_request_is_discarded_without_a_reply() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        let mut req = request(SID, &cwd, args(&["claude"]));
        req.at = clock::now() - 700.0;
        put(&dir, SID, &req);

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert!(pairs.is_empty());
        assert!(actions.is_empty(), "오래된 요청은 띄우지 않는다");
        assert!(!request_file(&dir, SID).exists(), "오래된 요청 파일은 지운다");
        assert!(!dir.join(format!("{SID}.reply.json")).exists());
    }

    /// 못 받는 요청 파일들 — 이름이 세션 id 꼴이 아님·못 풀음·id 어긋남·너무 큼: 모두 지워지고 아무것도 띄우지 않는다
    #[test]
    fn unreadable_requests_are_discarded() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let rows: Vec<(String, Vec<u8>)> = vec![
            // 이름(id)이 세션 id 꼴이 아니다
            ("oops.req.json".to_string(), b"{\"id\":\"oops\"}".to_vec()),
            // 풀 수 없는 JSON
            (format!("{SID}.req.json"), b"{not json".to_vec()),
            // 파일 안의 id 가 이름과 어긋난다
            (
                format!("{A}.req.json"),
                serde_json::to_vec(&request(SID, "C:\\cb-opentab-시험", args(&["claude"]))).unwrap(),
            ),
            // 읽기 상한이 넘는다
            (format!("{B}.req.json"), vec![b'0'; OPEN_TAB_READ_MAX_BYTES + 1]),
        ];
        for (name, bytes) in &rows {
            fs::write(dir.join(name), bytes).unwrap();
        }

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert!(pairs.is_empty());
        assert!(actions.is_empty());
        for (name, _) in &rows {
            assert!(!dir.join(name).exists(), "{name} 은 지워져야 한다");
        }
    }

    /// 요청은 온 순서(at 오름차순)대로 — 먼저 온 요청이 먼저 뜬다
    #[test]
    fn requests_are_handled_oldest_first() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let cwd = make_cwd(&scratch, "w");
        // 이름은 title 로만 준다(--name 과 --title 이 다르면 준비가 거절하므로)
        let mut first = request(SID, &cwd, args(&["claude"]));
        first.title = Some("먼저".to_string());
        first.at = clock::now() - 10.0;
        let mut second = request(A, &cwd, args(&["claude"]));
        second.title = Some("다음".to_string());
        second.at = clock::now() - 5.0;
        put(&dir, SID, &first);
        put(&dir, A, &second);

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert_eq!(
            pairs.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec![SID, A],
            "옛 요청부터 처리한다"
        );
        assert_eq!(actions.len(), 2);
        let OpenTabAction::Open { title, .. } = &actions[0] else {
            panic!("Open 동작이어야 한다: {:?}", actions[0])
        };
        assert_eq!(title.as_deref(), Some("먼저"));
        let sessions: Vec<Option<String>> = pairs.iter().map(|(_, reply)| reply.session.clone()).collect();
        assert_ne!(sessions[0], sessions[1], "두 요청은 각각의 새 세션 id 를 받는다");
    }

    /// 요청 폴더의 옛 잡파일 — 오래된 것만 지우고 방금 쓴 임시 파일은 남는다(mac takeRequests 와 같다)
    #[test]
    fn only_old_stray_files_are_removed() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.reply.json");
        fs::write(&old, "옛 답").unwrap();
        let fresh = dir.join("stuck.cb-partial");
        fs::write(&fresh, "방금 쓴 임시").unwrap();
        let old_at = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs_f64((clock::now() - 700.0).max(0.0));
        fs::OpenOptions::new().append(true).open(&old).unwrap().set_modified(old_at).unwrap();

        let mut actions: Vec<OpenTabAction> = vec![];
        run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert!(!old.exists(), "오래된 잡파일은 지운다");
        assert!(fresh.exists(), "최근 잡파일은 남는다");
    }

    /// 새 세션 id 는 매번 UUID 꼴이고 서로 다르다(uuid 의존성 없이 해시로 만든 꼴)
    #[test]
    fn new_session_id_is_uuid_shaped_and_unique() {
        let a = new_session_id();
        let b = new_session_id();
        assert!(is_session_id(&a));
        assert!(is_session_id(&b));
        assert_ne!(a, b, "차례가 섞여 서로 다르다");
    }

    /// 요청 cwd 가 폴더가 아니면(없는 폴더·그냥 파일) bad-args 답 — 어차피 못 여는 탭이라 보드가 거절한다
    #[test]
    fn request_with_a_non_folder_cwd_is_refused_as_bad_args() {
        let scratch = Scratch::new();
        let dir = scratch.path("dir");
        fs::create_dir_all(&dir).unwrap();
        let file_cwd = scratch.path("글.txt");
        fs::write(&file_cwd, "폴더가 아니다").unwrap();
        let missing = scratch.path("없음");
        put(&dir, SID, &request(SID, &missing.to_string_lossy(), args(&["claude"])));
        put(&dir, A, &request(A, &file_cwd.to_string_lossy(), args(&["claude"])));

        let mut actions: Vec<OpenTabAction> = vec![];
        let pairs = run(&dir, vec![], vec![], &mut |action| {
            actions.push(action);
            None
        });

        assert_eq!(pairs.len(), 2, "답은 쓴다(부른 쪽이 기다린다)");
        for (id, reply) in &pairs {
            assert_eq!(reply.status, "bad-args", "{id}");
            assert!(reply.why.as_deref().unwrap_or("").contains("폴더가 없습니다"), "{reply:?}");
        }
        assert!(actions.is_empty());
    }
}
