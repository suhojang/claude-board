// 보드 탭(내장 터미널) 짝짓기·자리 규칙 — 세션을 보드 탭의 화면과 짝짓고(pairTabs), 세션이 도는 곳을 가른다(sessionPlace·parkedScreen·shownSessionPlace).
// ClaudeBoard.swift 의 TerminalHub.pair·place·parkedTab·shownPlace 와 tabAncestor 에 있던 판단을 옮김(이슈 #9 마지막 칸) — 결과·포트 호출 차례는 그대로다.
// 옮긴 뒤 고친 것: 짝 없는 세션을 세션 id 로 화면에 붙이는 길은 그 화면에서 claude 가 뜨는 중이거나 돌 때만(sessionPlace),
// 같은 tty 인 끝나지 않은 세션이 여럿이면 정해진 차례(ttyWinner)로 화면의 세션을 고르고, 경로 상태 점은 일시중지를 대기보다 먼저 본다(pairTabs).
// 화면의 지금 값(tty·맨 위 프로세스·세션 id·명령)은 앱이 떠서 값(TabScreen·ScreenSession)으로 넘긴다. 비싸서 그 갈래에서만 묻던 것은 포트로 부른다:
// 화면의 claude·세션 실행 방식은 TabProbe, 부모 pid 는 ProcessControl, 넘긴 작업 id 는 ClaudeLive.
// 결과는 값(TabPairing)으로 돌려주고 앱(TerminalHub)이 화면(BoardSurface)·@Published 에 쓴다. 다른 터미널 세션의 앱 이름(appNames)·쥔 탭 목록 남기기·
// 이름 바꾸기는 앱에 남는다(AppKit·UserDefaults)
import Foundation

/// 보드 탭의 화면 하나의 지금 값 — 짝짓기(pairTabs)가 읽는다. 앱이 BoardSurface 에서 떠서 탭 차례(탭 안은 트리 차례)로 넘긴다
public struct TabScreen: Equatable, Sendable {
    public let id: UUID
    public let workspace: String    // 화면이 든 탭의 작업 경로 — 경로 줄 상태 점
    public let tty: String          // 엔진이 준 /dev/ttysNNN 에서 /dev/ 를 뗀 이름(세션 파일의 tty 와 같은 꼴). 아직 없으면 빈 글자
    public let leader: Int32        // 화면의 맨 위 프로세스(엔진이 띄운 login — 앞 프로세스의 세션 리더) pid. 앞 프로세스가 없거나 못 읽으면 0
    public let sessionId: String?   // 이 화면의 세션 id(새 세션의 --session-id, 이어 연 세션, tty 로 짝지은 지금 세션) — 끝난 뒤에도 남는다
    public let command: String?     // 보드가 셸에 넣은 claude 명령 — nil 이면 셸 화면
    public let mdRenderOff: Bool    // 답변 꾸미기를 끄고 띄운 claude 화면

    public init(id: UUID, workspace: String, tty: String, leader: Int32, sessionId: String?, command: String?, mdRenderOff: Bool) {
        self.id = id; self.workspace = workspace; self.tty = tty; self.leader = leader
        self.sessionId = sessionId; self.command = command; self.mdRenderOff = mdRenderOff
    }
}

/// 화면 하나의 짝짓기 결과 — 앱이 BoardSurface 에 쓴다(@Published 인 session·parked 는 바뀐 것만)
public struct ScreenPairing: Equatable, Sendable {
    public let id: UUID
    public let sessionId: String?   // tty 로 짝지은 세션이 있으면 그 id(/clear 로 바뀐 id, 셸에서 사람이 띄운 claude), 없으면 그대로
    public let command: String?     // 셸에서 사람이 친 대화형 claude 면 "claude", claude 가 끝나 셸만 남았으면 nil, 아니면 그대로
    public let mdRenderOff: Bool    // 셸만 남으면 false, 아니면 그대로
    public let session: Session?    // 이 화면에서 도는 세션 — 탭의 상태 점·제목
    public let parked: Session?     // 화면의 claude 가 대화를 넘겨 붙여 보여 주는 이어 받은 작업 — 탭의 상태 점·지금 하는 일

    public init(id: UUID, sessionId: String?, command: String?, mdRenderOff: Bool, session: Session?, parked: Session?) {
        self.id = id; self.sessionId = sessionId; self.command = command; self.mdRenderOff = mdRenderOff
        self.session = session; self.parked = parked
    }

    /// 끈 세션으로 기억할 세션 id — 꾸미기를 끈 화면이면 지금 세션(그 화면에서 /clear 등으로 id 가 바뀌어도 끈 채로 이어 열게).
    /// 셸만 남아 끈 표시를 비운 화면은 nil(끈 꾸미기는 끈 세션 기억에 이미 남아 있다)
    public var mdRenderOffSession: String? { mdRenderOff ? session?.id : nil }
}

/// 짝짓기 결과 — 화면마다의 값(넘긴 화면 차례)과 세션 쪽 표
public struct TabPairing: Equatable, Sendable {
    public let screens: [ScreenPairing]
    public let paired: [String: UUID]          // 세션 id → 그 세션이 도는 화면(tty 로 짝지은 세션, 화면 안에서 시작된 headless)
    public let ownedHeadless: Set<String>      // 화면 안에서 시작된 headless 세션(claude -p) — 화면이 닫히면 함께 끝난다
    public let parkedTabs: [String: UUID]      // 화면 claude 가 넘긴 대화를 이어 받은 작업의 세션 id → 그 화면(paired 에는 넣지 않는다)
    public let parkedFrom: Set<String>         // 넘긴 쪽(화면 claude)의 세션 id — 그 작업을 따라가는 동안 카드 목록에서 뺀다
    public let workspaceStates: [String: String]   // 경로 줄 상태 점 — 경로마다 그 화면들의 세션 가운데 가장 급한 상태

    public init(screens: [ScreenPairing], paired: [String: UUID], ownedHeadless: Set<String>, parkedTabs: [String: UUID],
                parkedFrom: Set<String>, workspaceStates: [String: String]) {
        self.screens = screens; self.paired = paired; self.ownedHeadless = ownedHeadless; self.parkedTabs = parkedTabs
        self.parkedFrom = parkedFrom; self.workspaceStates = workspaceStates
    }
}

/// 화면과 그 화면의 세션 id — 자리 판정(sessionPlace)이 읽는 가벼운 값. place 는 카드를 그릴 때마다 불리므로 tty·앞 프로세스는 읽지 않는다
/// (화면의 tty 는 BoardSurface 에 저장된 값이 아니라 읽을 때마다 엔진을 부르는 값이라, 화면마다 떠서 넘기면 카드를 그릴 때마다 화면 수만큼 엔진을 부른다)
public struct ScreenSession: Equatable, Sendable {
    public let id: UUID
    public let sessionId: String?
    public init(id: UUID, sessionId: String?) { self.id = id; self.sessionId = sessionId }
}

// MARK: - 짝짓기

/// 세션 목록을 다시 읽을 때마다 탭의 화면들과 짝짓는다. tty 가 같으면 그 화면의 세션, headless 세션은 조상에 화면의 맨 위 프로세스가 있으면
/// 그 화면 소속(화면이 닫히면 함께 끝난다 — Claude 가 Bash 도구로 띄운 claude -p 등). 끝난 세션은 같은 세션 id 로 연 화면에서 claude 가 뜨는 중일 때만.
/// 같은 tty·같은 맨 위 프로세스인 화면이 여럿이면 뒤의 것(지금 동작). 같은 tty 인 끝나지 않은 세션이 여럿이면 ttyWinner 로 고른 하나가
/// 화면의 세션·세션 id 이고(전에는 세션 목록 — 파일 목록 — 차례의 뒤 것이라 정해져 있지 않았다), paired 에는 지금처럼 모두 그 화면으로 넣는다
public func pairTabs(_ sessions: [Session], screens: [TabScreen], probe: TabProbe, processes: ProcessControl, live: ClaudeLive) -> TabPairing {
    var byTty: [String: Int] = [:]
    for (i, t) in screens.enumerated() where !t.tty.isEmpty { byTty[t.tty] = i }
    var map: [String: UUID] = [:]
    // 화면의 맨 위 프로세스(엔진이 띄운 login — 그 tty 의 세션 리더). 셸 화면에서 사람이 띄운 claude -p 도 그 아래에 있다
    var leaders: [Int32: UUID] = [:]
    for t in screens where t.leader > 0 { leaders[t.leader] = t.id }
    var sessionIds = screens.map(\.sessionId)
    var main: [UUID: Session] = [:]
    for s in sessions where s.state != "ended" && !s.tty.isEmpty {
        guard let i = byTty[s.tty] else { continue }
        map[s.id] = screens[i].id
        if let held = main[screens[i].id], !ttyWinner(s, over: held) { continue }
        main[screens[i].id] = s
        // /clear 등으로 id 가 바뀌었거나 셸에서 사람이 띄운 claude — 끝난 뒤에도 그 세션이 돌던 화면으로 찾는다(resumeSession)
        sessionIds[i] = s.id
    }
    var owned: Set<String> = []
    for s in sessions where s.state != "ended" && s.headless && s.pid > 0 {
        if let id = tabAncestor(s.pid, leaders, processes) { map[s.id] = id; owned.insert(s.id) }
    }
    var parked: [String: UUID] = [:], from: Set<String> = [], out: [ScreenPairing] = []
    for (i, t) in screens.enumerated() {
        let running = main[t.id] != nil || probe.runsClaude(t.id)
        var command = t.command, off = t.mdRenderOff
        // 셸 프롬프트에서 사람이 친 대화형 claude 도 claude 화면으로 — 쥔 목록에 세션이 남아 다시 뜰 때 이어 연다(-p·--bg 는 빼고)
        if command == nil, let s = main[t.id], probe.mode(s) == nil { command = "claude" }
        // 보드가 넣은 claude 가 끝나 셸만 남았다 — 셸 화면으로(쥔 목록·상태 점)
        if !running, command != nil { command = nil; off = false }
        let s = main[t.id] ?? (running ? sessionIds[i].flatMap { id in sessions.first { $0.id == id } } : nil)
        // 화면의 claude 가 대화를 백그라운드 작업으로 넘겨 그 작업을 붙여 보여 주는 중이면(parkedJobId — 이어 받은 세션 id 앞 8글자)
        // 그 작업 세션을 따라 보인다. 끝난 작업이나 작업을 풀면(parkedJobId 지움) 화면의 claude 로 돌아간다
        var p: Session? = nil
        if let s, s.state != "ended", let job = live.parkedJob(s.pid, session: s.id) {
            p = sessions.first { $0.id != s.id && $0.id.hasPrefix(job) && $0.state != "ended" }
            if let p { parked[p.id] = t.id; from.insert(s.id) }
        }
        out.append(ScreenPairing(id: t.id, sessionId: sessionIds[i], command: command, mdRenderOff: off, session: s, parked: p))
    }
    // 경로 줄의 상태 점 — 그 경로 탭들의 모든 화면에서 승인 대기 > 작업 중 > 일시중지 > 대기 > 끝남(세션이 없으면 대기, workspaceDotRank).
    // 넘긴 작업을 붙여 보는 화면은 그 작업. 같은 상태끼리는 화면 차례의 앞 것
    var states: [String: String] = [:]
    for ws in screens.map(\.workspace) where states[ws] == nil {
        states[ws] = zip(screens, out).filter { $0.0.workspace == ws }.compactMap { $0.1.parked ?? $0.1.session }
            .min { workspaceDotRank($0.state) < workspaceDotRank($1.state) }?.state ?? "idle"
    }
    return TabPairing(screens: out, paired: map, ownedHeadless: owned, parkedTabs: parked, parkedFrom: from, workspaceStates: states)
}

/// 경로 줄 상태 점의 순위 — 승인 대기 > 작업 중 > 일시중지 > 대기 > 끝남. Session.rank 는 카드 정렬에도 쓰여 일시중지·대기가 같은 순위인데,
/// 이 점에서는 일시중지를 대기보다 먼저 보인다(사람이 재개해야 한다). 전에는 Session.rank 로 골라 같은 순위면 화면 차례의 앞 것이었다
func workspaceDotRank(_ state: String) -> Int { ["waiting": 0, "working": 1, "paused": 2, "idle": 3, "ended": 4][state] ?? 9 }

/// 같은 tty 인 끝나지 않은 세션 둘 가운데 a 가 화면의 세션인가 — 마지막 hook 기록(updated)이 늦은 것, 같으면 이 상태가 된 시각(since)이
/// 늦은 것, 그것도 같으면 세션 id 글자 차례의 뒤 것. 세션 목록 차례와 관계없이 정해진다
func ttyWinner(_ a: Session, over b: Session) -> Bool {
    (a.updated, a.since, a.id) > (b.updated, b.since, b.id)
}

/// pid 의 조상 가운데 화면의 맨 위 프로세스가 있으면 그 화면 — 부모로 한 칸 올라간 뒤 견주기를 12번까지(1 이하 pid 에서는 더 올라가지 않는다).
/// 부모를 못 읽으면 거기서 nil
func tabAncestor(_ pid: Int32, _ leaders: [Int32: UUID], _ processes: ProcessControl) -> UUID? {
    var p = pid
    for _ in 0..<12 where p > 1 {
        guard let parent = processes.parentPID(p) else { return nil }
        p = parent
        if let t = leaders[p] { return t }
    }
    return nil
}

// MARK: - 자리

/// 세션이 어디서 도는가 — 보드 탭의 화면(그 안에서 시작된 claude -p 포함)·다른 터미널·백그라운드·끝남.
/// 짝(paired)이 없으면 같은 세션 id 로 연 화면 가운데 claude 가 막 뜨는 중이거나 도는 첫 화면(이어 열기를 넣은 직후·tty 를 아직 안 쓴 새 세션).
/// claude 가 끝나 셸만 남은 화면에는 붙이지 않는다 — 끝난 세션은 끝남, 끝나지 않은 세션은 다른 곳에서 돈다(보드 탭에서 끝난 세션을
/// 다른 터미널에서 이어 열면 화면에 세션 id 가 남아 있어도 다른 터미널. 전에는 끝나지 않은 세션이면 claude 가 돌지 않는 화면에도 붙였다).
/// runsClaude 는 세션 id 가 같은 화면에서만 묻는다. headless 표시는 화면 안에서 시작된 headless(ownedHeadless)일 때만.
/// screens 는 짝이 없을 때만 만든다(카드를 그릴 때마다 불린다)
public func sessionPlace(_ s: Session, paired: [String: UUID], ownedHeadless: Set<String>,
                         screens: @autoclosure () -> [ScreenSession], probe: TabProbe) -> SessionPlace {
    if let id = paired[s.id] ?? screens().first(where: { $0.sessionId == s.id && probe.runsClaude($0.id) })?.id {
        return .tab(id, headless: ownedHeadless.contains(s.id))
    }
    if s.state == "ended" { return .ended }
    return s.headless ? .background : .terminal
}

/// 보드 탭 claude 가 넘긴 대화를 이어 받은 백그라운드 작업이면 그 화면 — 끝난 작업은 nil
public func parkedScreen(_ s: Session, parkedTabs: [String: UUID]) -> UUID? {
    s.state == "ended" ? nil : parkedTabs[s.id]
}

/// 카드가 보이는 자리(터미널 구역·CB 칩)와 누르면 갈 곳 — 이어 받은 작업이면 그 화면(headless 아님), 아니면 place.
/// 탭에 쳐 넣는 동작은 place 그대로라 그 작업 때문에 탭에 아무것도 넣지 않는다. place 는 이어 받은 작업이 아닐 때만 부른다
public func shownSessionPlace(parked: UUID?, place: @autoclosure () -> SessionPlace) -> SessionPlace {
    parked.map { .tab($0, headless: false) } ?? place()
}
