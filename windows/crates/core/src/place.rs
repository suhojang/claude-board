//! 세션 pid 와 보드 터미널(탭)을 짝짓는 순수 규칙 — W2 설계 §1, mac `TerminalHub.pair`·`tabAncestor` 의 자리.
//!
//! Windows 에는 tty 가 없다. 대신 ConPTY 를 연 프로세스마다 콘솔 호스트(conhost/OpenConsole `--headless`)가
//! 있고, 그 콘솔을 물려받은 프로세스들은 같은 호스트를 본다 — 그래서 "같은 tty" 의 자리는 "콘솔 호스트가
//! 같다"다(R1). 콘솔 정보를 못 읽는 프로세스(`Host::Unknown`)는 pid 조상 걸어 올리기로 되돌아가고(R2·R3,
//! mac `tabAncestor` 와 같은 걸음), `--bg` 감시자는 콘솔 없이 뜨므로 걸음의 경계로 막는다.
//! 걸음마다 생성 시각 게이트를 두어 부모가 자식보다 늦게 태어났으면(pid 재사용) 멈춘다.
//!
//! 이 모듈은 Win32 를 모른다 — 실제 조사는 `ProcessView` 포트를 구현한 어댑터(보드 백엔드)가
//! `proc::console_host`·`proc::creation_time`·`proc::command_line` 을 연결해 준다.

/// 세션 pid 프로세스가 쓰는 콘솔의 호스트. 같은 값이면 같은 콘솔(같은 tty 의 대응물)이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// 콘솔 호스트 프로세스 pid.
    Console(u32),
    /// 콘솔이 없다 — `DETACHED_PROCESS` 로 뜬 `--bg` 감시자와 그 밑의 일꾼.
    Detached,
    /// 콘솔 정보를 못 읽었다 — 조상 걸음(R2·R3)으로만 판단한다.
    Unknown,
}

/// 세션이 시작된 방식(mac 의 `-p`·`--bg` 문자열 자리).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 대화를 찍고 끝나는 방식 — `-p`·`--print`.
    Print,
    /// 콘솔 없이 도는 방식 — `--bg`·`--background`·`--bg-pty-host`.
    Background,
}

/// 걸어 올릴 때 보는 프로세스 사실들. Win32 는 이 포트로만 들어온다.
pub trait ProcessView {
    /// 부모 pid. 모르면 None.
    fn parent(&self, pid: u32) -> Option<u32>;
    /// 프로세스 생성 시각(유닉스 초). 모르면 None — 걸음의 게이트가 쓴다.
    fn ctime(&self, pid: u32) -> Option<f64>;
    /// 프로세스의 콘솔 호스트.
    fn host(&self, pid: u32) -> Host;
    /// 실행 인자에서 본 세션 시작 방식.
    fn mode(&self, pid: u32) -> Option<Mode>;
    /// 실행 파일 이름. 모르면 None.
    fn name(&self, pid: u32) -> Option<String>;
}

/// 보드가 쥔 터미널 탭 하나 — ConPTY 의 pid 와 그 콘솔 정보(`TermHub::hosts` 가 모아 준다).
#[derive(Debug, Clone)]
pub struct BoardTerm {
    /// 보드 터미널(탭) 식별자.
    pub id: String,
    /// 이 탭이 띄운 셸 pid — 조상 걸음이 여기에 닿으면 이 탭이다.
    pub shell_pid: u32,
    /// 셸 생성 시각(유닉스 초). pid 재사용을 가르는 데 쓴다. 모르면 짝짓지 않는다.
    pub shell_ctime: Option<f64>,
    /// 셸이 쓰는 콘솔 호스트.
    pub host: Host,
}

/// 짝짓기에 쓰는 세션 기록의 세 가지 사실.
#[derive(Debug, Clone)]
pub struct SessionProc {
    /// 세션 파일의 pid(0 이면 짝짓지 않는다).
    pub pid: u32,
    /// 세션 tty(콘솔) 없이 떴는지.
    pub headless: bool,
    /// 세션이 끝났는지 — 끝난 세션은 아무것도 짓지 않는다(R0).
    pub ended: bool,
}

/// 짝짓기 결과 — mac `pair` 돌려값 자리.
/// `tty` 는 같은 콘솔(R1)·조상 걸음(R2)으로 짝지은 보드 터미널,
/// `tab_ancestor` 는 headless 세션이 그 셸 조상 아래에서 도는 보드 터미널(R3),
/// `mode` 는 세션 자신의 시작 방식이다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Placement {
    /// 이 보드 터미널의 콘솔(또는 조상)에서 도는 세션이다.
    pub tty: Option<String>,
    /// 이 보드 터미널의 셸 조상 아래에서 도는 headless 세션이다(탭이 닫히면 함께 끝난다).
    pub tab_ancestor: Option<String>,
    /// 세션 자신의 시작 방식.
    pub mode: Option<Mode>,
}

/// 조상 걸음의 상한 — mac `tabAncestor` 와 같은 12 걸음.
const MAX_HOPS: usize = 12;

/// 생성 시각을 같다고 보는 오차(초) — `proc::alive` 와 같은 값.
const CTIME_TOLERANCE: f64 = 0.001;

/// 세션 pid 와 보드 터미널들을 짝짓는다.
///
/// - R0 — 끝난 세션이나 pid 없는 세션은 시작 방식까지 모두 내지 않는다(`/clear` 뒤 끝난 id 가
///   살아 있는 pid 와 같아도 짝짓지 않는다, mac 도 끝난 세션을 건너뛴다).
/// - R1 — 세션의 콘솔 호스트와 같은 콘솔을 쓰는 보드 터미널이 있으면 그것이다.
/// - R2 — R1 로 못 짝지었고 콘솔 정보를 못 읽은 쪽(세션이나 그 터미널)이 있으면, 대화형 세션은
///   조상 걸음이 그 탭의 셸에 닿을 때(셸 생성 시각이 같을 때) 그 탭이다.
/// - R3 — R2 로도 못 짝지었고 headless 세션이고 시작 방식이 `--bg` 가 아니면, 조상 걸음이 닿은
///   탭 셸이다. 탭이 닫히면 함께 끝난 세션이다.
/// - `mode` — 세션 자신의 실행 인자에서 본 시작 방식.
pub fn place(session: &SessionProc, terms: &[BoardTerm], view: &impl ProcessView) -> Placement {
    // R0 — 끝난 세션·pid 없는 세션은 아무것도 짓지 않는다
    if session.ended || session.pid == 0 {
        return Placement::default();
    }
    let session_host = view.host(session.pid);
    let mode = view.mode(session.pid);
    // R1 — 같은 콘솔 호스트 = 같은 tty 의 대응물
    let tty = pair_by_console(&session_host, terms);
    // R1 로 못 짝지었을 때만 걸어 올린다(걸음은 두 규칙이 같은 조상 나무를 본다)
    let chain = if tty.is_none() { walk_ancestors(session.pid, view) } else { Vec::new() };
    // R2 — 콘솔로 짝짓지 못한 대화형 세션
    let tty = if session.headless {
        tty
    } else {
        tty.or_else(|| nearest_reached_term(&chain, terms, view, |t| fallback_allowed(&session_host, t)))
    };
    // R3 — headless 세션은 탭 셸 조상을 찾는다(세션 자신이 `--bg` 이면 감시자이므로 막는다)
    let tab_ancestor = if tty.is_none() && session.headless && mode != Some(Mode::Background) {
        nearest_reached_term(&chain, terms, view, |_| true)
    } else {
        None
    };
    Placement { tty, tab_ancestor, mode }
}

/// 같은 콘솔을 쓰는 보드 터미널(R1) — 세션 콘솔을 보기로 읽었을 때만 온다.
fn pair_by_console(session_host: &Host, terms: &[BoardTerm]) -> Option<String> {
    match session_host {
        Host::Console(session_console) => terms
            .iter()
            .find(|t| matches!(t.host, Host::Console(host) if host == *session_console))
            .map(|t| t.id.clone()),
        _ => None,
    }
}

/// R2 의 보조 걸음이 허용되는 터미널 — 세션이나 그 터미널이 콘솔 보기를 실패했을 때만이다.
/// 대답이 확실한 Detached 는 허용하지 않는다: 확실한 "아니오"로 걸어 올려 짝짓지 않는다.
fn fallback_allowed(session_host: &Host, term: &BoardTerm) -> bool {
    session_host == &Host::Unknown || term.host == Host::Unknown
}

/// pid 위로 조상을 최대 12 걸음 걷는다. 닿은 조상들을 닿은 순서로 돌려준다.
/// 한 걸음마다 두 가지를 보고 멈춘다(fail safe):
/// - 생성 시각 게이트 — 부모의 생성 시각을 모르거나 부모가 자식보다 늦게 태어났으면(pid 재사용) 멈춘다.
/// - 감시자 경계 — 중간 걸음이 콘솔 없는 프로세스(Detached)이거나 `--bg` 이면 멈춘다.
fn walk_ancestors(pid: u32, view: &impl ProcessView) -> Vec<u32> {
    let mut chain = Vec::new();
    // 세션 자신의 생성 시각부터 게이트가 보므로, 모르면 걷지 못한다
    let Some(mut child_ctime) = view.ctime(pid) else {
        return chain;
    };
    let mut cur = pid;
    for _ in 0..MAX_HOPS {
        let Some(parent) = view.parent(cur) else { break };
        if parent == 0 || parent == cur || chain.contains(&parent) {
            break;
        }
        let Some(parent_ctime) = view.ctime(parent) else { break };
        if parent_ctime > child_ctime {
            break;
        }
        chain.push(parent);
        // 감시자 경계 — 그 위로는 세션과 상관이 없어진다
        if view.host(parent) == Host::Detached || view.mode(parent) == Some(Mode::Background) {
            break;
        }
        cur = parent;
        child_ctime = parent_ctime;
    }
    chain
}

/// 조상 걸음이 닿은 조상 가운데 `eligible` 이고(탭 셸이고, 기록된 셸 생성 시각이 맞는)
/// 가장 가까운 보드 터미널 하나.
fn nearest_reached_term(
    chain: &[u32],
    terms: &[BoardTerm],
    view: &impl ProcessView,
    eligible: impl Fn(&BoardTerm) -> bool,
) -> Option<String> {
    chain.iter().find_map(|&pid| {
        terms
            .iter()
            .find(|t| eligible(t) && t.shell_pid == pid && shell_ctime_matches(t, view, pid))
            .map(|t| t.id.clone())
    })
}

/// 조상 걸음이 닿은 pid 가 이 터미널이 기록해 둔 셸이 맞는지 — 생성 시각까지 같아야 한다.
/// 둘 중 하나라도 모르면 다른 프로세스일 수 있어 짝짓지 않는다(pid 재사용).
fn shell_ctime_matches(term: &BoardTerm, view: &impl ProcessView, pid: u32) -> bool {
    let (Some(placed), Some(now)) = (term.shell_ctime, view.ctime(pid)) else {
        return false;
    };
    (placed - now).abs() < CTIME_TOLERANCE
}

/// 실행 인자에서 세션 시작 방식을 본다 — argv[0] 은 넘기고, `--bg` 계열이 `-p` 계열보다 우선한다
/// (mac `mode`: `--bg`, `--background`, `--bg-pty-host`, `-p`, `--print`).
pub fn mode(argv: &[String]) -> Option<Mode> {
    let mut flags = argv.iter().skip(1);
    if flags.clone().any(|a| matches!(a.as_str(), "--bg" | "--background" | "--bg-pty-host")) {
        Some(Mode::Background)
    } else if flags.any(|a| matches!(a.as_str(), "-p" | "--print")) {
        Some(Mode::Print)
    } else {
        None
    }
}

/// 터미널 창의 실행 파일로 앱 이름을 본다(mac 의 이름표 대신 실행 파일로; 경로가 오면 파일 이름만).
/// Windows 의 실행 파일 이름은 대소문자가 갈려 대소문자를 무시하고 본다.
pub fn app_name(exe: &str) -> &'static str {
    let file_name = match exe.rfind(['\\', '/']) {
        Some(sep) => &exe[sep + 1..],
        None => exe,
    };
    match file_name.to_ascii_lowercase().as_str() {
        "windowsterminal.exe" => "Windows Terminal",
        "code.exe" => "VS Code",
        "mintty.exe" => "Git Bash",
        _ => "터미널",
    }
}

/// 이 터미널이 지금 claude 를 도는지 — claude pid 가운데 콘솔 호스트가 이 탭의 콘솔과 같은 것이
/// 있는지(mac `runsClaude`: 앞에서 claude 가 도는지). claude pid 목록은 어댑터가 셸 자손에서 고른다.
pub fn runs_claude(term: &BoardTerm, claude_pids: &[u32], view: &impl ProcessView) -> bool {
    // 콘솔을 읽은 호스트끼리만 같은 터미널이다 — Unknown·Detached 끼리 같다고 보면 엉뚱한 터미널에 claude 가 도는 것으로 오인한다
    let Host::Console(host) = term.host else { return false };
    claude_pids.iter().any(|&pid| view.host(pid) == Host::Console(host))
}

/// 셸이 프롬프트에 있는지 — 셸에 살아 있는 자식이 없다(콘솔 호스트 `conhost.exe`·`OpenConsole.exe` 는 빼고)
/// (mac `atPrompt`: 셸의 자식 프로세스가 없다). `children` 는 어댑터가 프로세스 표에서 고른 셸의 직접 자식 pid.
/// 생성 시각이 셸보다 앞서는 자식은 pid 재사용으로 남은 옛 부모 기록이라 살아 있는 자식으로 세지 않는다.
/// 셸의 생성 시각을 모르면 그 판정을 못 하므로 자식을 모두 센다 — 실행 중인 프로그램에 글자를 치지 않는 쪽으로 틀린다.
pub fn at_prompt(shell_pid: u32, view: &impl ProcessView, children: &[u32]) -> bool {
    let shell_ctime = view.ctime(shell_pid);
    children.iter().all(|&pid| {
        let host = view.name(pid).is_some_and(|n| {
            n.eq_ignore_ascii_case("conhost.exe") || n.eq_ignore_ascii_case("OpenConsole.exe")
        });
        let stale = matches!((shell_ctime, view.ctime(pid)), (Some(shell), Some(child)) if child + CTIME_TOLERANCE < shell);
        host || stale
    })
}

#[cfg(test)]
mod tests;