// 다른 터미널 조작(Terminals.swift)·세션 조작(SessionControl.swift) 시험 — 다른 터미널 앱·프로세스·tty 제목·보드 탭·세션 표지·세션 터미널·시계를
// 가짜 어댑터로 넣어 진짜 터미널·프로세스·파일 없이 돌린다. 가짜는 이 파일에만 둔다(Sources/BoardRules 에 두지 않는다).
// 가짜는 부른 것을 한 줄씩 적고 done 을 그 자리에서 부른다 — 사슬(넣기 → 표지·신호·기다리기 → 이어 열기)이 바로 풀려 부른 차례를 그대로 견준다.
// 기대값은 ClaudeBoard.swift 에 있던 지금 동작 그대로다(버그 의심도 "지금 동작" 으로 적고 고치지 않는다).
// 옮긴 뒤 고친 것(끝난 세션 중단의 SIGTERM, 키를 못 넣는 살아 있는 세션의 재개, 이슈 #9 3차의 세션을 띄운 앱 기억 정리·
// Ghostty 포커스의 빈 id·AppleScript 글자에 넣는 tty 와 터미널 id 막기·tty 이름 확인)은 고친 동작을 기대값으로 적는다.
// 보통 값(ttys003·영숫자 id)의 AppleScript 글자는 막기 전과 바이트까지 같다(아래 고정 글자 그대로).
// AppleScript 글자의 기대값은 옮기기 전 코드(8904f1f 의 focusByTty·ttyExists·ghosttyTerminalId·ghosttySend·focusGhostty)를 그대로 떼어 돌린 출력이다
import Foundation
import Testing
import BoardRules

// MARK: - 가짜 어댑터 (시험마다 새로 만들고 한 스레드에서만 쓴다 — 포트가 Sendable 이라 @unchecked)

private final class CallLog: @unchecked Sendable {
    var calls: [String] = []
}

private func short(_ bundleId: String) -> String {
    switch bundleId {
    case TerminalBundle.ghostty: return "ghostty"
    case TerminalBundle.iterm: return "iterm"
    case TerminalBundle.terminal: return "terminal"
    default: return bundleId
    }
}

// AppleScript 의 종류 — 기록과 답을 고르는 이름표(글자 자체는 scripts 로 따로 견준다)
private func label(_ script: String) -> String {
    let app = script.contains(TerminalBundle.iterm) ? "iterm" : "terminal"
    if script == ghosttyTerminalsScript { return "list" }
    if script.contains("then return id of t") { return "find" }
    if script.contains("focus t") { return "focus" }
    if script.contains("count of") { return "count \(app)" }
    if script.contains("tell s to select") || script.contains("set selected tab") { return "tty \(app)" }
    if script.contains("(id of t as text) is") { return "send" }
    return "other"
}

private final class FakeApps: TerminalApps, @unchecked Sendable {
    let log: CallLog
    var running = Set<String>()
    var replies: [String: [String?]] = [:]     // 이름표 → 차례로 줄 답
    var fallback: [String: String?] = [:]      // 차례 답이 떨어지면 줄 답(없으면 nil)
    private(set) var scripts: [String] = []
    init(log: CallLog) { self.log = log }
    func isRunning(_ bundleId: String) -> Bool { log.calls.append("running \(short(bundleId))"); return running.contains(bundleId) }
    func activate(_ bundleId: String) { log.calls.append("activate \(short(bundleId))") }
    func openGhostty() { log.calls.append("openGhostty") }
    func run(_ script: String) -> String? {
        let l = label(script)
        scripts.append(script)
        log.calls.append("run \(l)")
        if var q = replies[l], !q.isEmpty {
            let r = q.removeFirst()
            replies[l] = q
            return r
        }
        return fallback[l] ?? nil
    }
    func wait(_ seconds: TimeInterval) { log.calls.append("wait \(seconds)") }
}

private final class FakeProcesses: ProcessControl, @unchecked Sendable {
    let log: CallLog
    var ancestors: [Int32: String] = [:]
    var alive = Set<Int32>()
    init(log: CallLog) { self.log = log }
    func ancestorBundleID(_ pid: Int32) -> String? { log.calls.append("ancestor \(pid)"); return ancestors[pid] }
    func parentPID(_ pid: Int32) -> Int32? { log.calls.append("parent \(pid)"); return nil }   // 다른 터미널·세션 조작은 부르지 않는다(보드 탭 짝짓기 몫)
    func isAlive(_ pid: Int32) -> Bool { log.calls.append("alive \(pid)"); return alive.contains(pid) }
    func terminate(_ pid: Int32) { log.calls.append("terminate \(pid)") }
}

private final class FakeTitles: TerminalTitles, @unchecked Sendable {
    let log: CallLog
    init(log: CallLog) { self.log = log }
    func write(tty: String, title: String) { log.calls.append("title \(tty)=\(title)") }
}

private let tabA = UUID(uuidString: "00000000-0000-0000-0000-00000000000A")!
private let tabB = UUID(uuidString: "00000000-0000-0000-0000-00000000000B")!
private let parkedP = UUID(uuidString: "00000000-0000-0000-0000-0000000000FF")!
private func name(_ id: UUID) -> String { [tabA: "A", tabB: "B", parkedP: "P"][id] ?? id.uuidString }

private func steps(_ s: TerminalSteps) -> String { s.map { "\($0.0):\($0.1)" }.joined(separator: " ") }

private final class FakeTabs: BoardTabs, @unchecked Sendable {
    let log: CallLog
    var places: [String: SessionPlace] = [:]          // 없으면 .terminal
    var parked: [String: UUID] = [:]
    var foreground = Set<UUID>()                       // 그 화면 앞에 세션 pid 가 있다
    var driveOK = true
    var prompts: [String: UUID] = [:]                  // "세션|경로" → 셸 프롬프트로 남은 화면
    var mdRenderOff = false
    var modes: [String: String] = [:]
    init(log: CallLog) { self.log = log }
    func place(_ s: Session) -> SessionPlace { log.calls.append("place \(s.id)"); return places[s.id] ?? .terminal }
    func parkedTab(_ s: Session) -> UUID? { log.calls.append("parked \(s.id)"); return parked[s.id] }
    func isForeground(_ screen: UUID, pid: Int32) -> Bool { log.calls.append("fg \(name(screen)) \(pid)"); return foreground.contains(screen) }
    func drive(_ screen: UUID, _ s: TerminalSteps, done: @escaping (Bool) -> Void) {
        log.calls.append("tab-drive \(name(screen)) \(steps(s))")
        done(driveOK)
    }
    func reveal(_ id: UUID) { log.calls.append("reveal \(name(id))") }
    func promptScreen(session: String, dir: String) -> UUID? { log.calls.append("prompt \(session) \(dir)"); return prompts["\(session)|\(dir)"] }
    func run(_ command: String, in screen: UUID, session: String, mdRenderOff: Bool) {
        log.calls.append("run \(name(screen)) \(session) off=\(mdRenderOff) \(command)")
    }
    func launch(_ command: String, in dir: String, session: String, mdRenderOff: Bool) {
        log.calls.append("launch \(dir) \(session) off=\(mdRenderOff) \(command)")
    }
    // 답변 꾸미기 — 가짜는 " +md" 를 붙여 거쳤는지만 보인다
    func claudeCommand(_ base: String, session: String) -> (command: String, mdRenderOff: Bool) {
        log.calls.append("command \(session) \(base)")
        return (base + " +md", mdRenderOff)
    }
    func mode(_ s: Session) -> String? { log.calls.append("mode \(s.id)"); return modes[s.id] }
    func toast(_ text: String) { log.calls.append("toast \(text)") }
}

private final class FakeMarks: SessionMarks, @unchecked Sendable {
    let log: CallLog
    private(set) var paused: [String: Data] = [:]
    init(log: CallLog) { self.log = log }
    func writePaused(_ sid: String, _ data: Data) { log.calls.append("paused-write \(sid)"); paused[sid] = data }
    func clearPaused(_ sid: String) { log.calls.append("paused-clear \(sid)") }
    func setMuted(_ sid: String, _ on: Bool) { log.calls.append("mute \(sid) \(on)") }
}

private final class FakeTerminals: SessionTerminals, @unchecked Sendable {
    let log: CallLog
    var driveOK = true
    var exited = true
    init(log: CallLog) { self.log = log }
    func drive(_ s: Session, _ st: TerminalSteps, done: @escaping (Bool) -> Void) {
        log.calls.append("drive \(s.id) \(steps(st))")
        done(driveOK)
    }
    func waitForExit(_ pid: Int32, timeout: Double, done: @escaping (Bool) -> Void) {
        log.calls.append("wait-exit \(pid) \(timeout)")
        done(exited)
    }
    func focus(_ s: Session) { log.calls.append("focus \(s.id)") }
    func bringForward(_ s: Session) { log.calls.append("bring \(s.id)") }
}

private struct FixedClock: Clock { let now: Date }
private let c0 = Date(timeIntervalSince1970: 3_000_000)

// 가짜 바깥 한 벌
private final class World {
    let log = CallLog()
    let apps: FakeApps, processes: FakeProcesses, titles: FakeTitles, tabs: FakeTabs, marks: FakeMarks, terminals: FakeTerminals
    init() {
        apps = FakeApps(log: log); processes = FakeProcesses(log: log); titles = FakeTitles(log: log)
        tabs = FakeTabs(log: log); marks = FakeMarks(log: log); terminals = FakeTerminals(log: log)
    }
    lazy var control = TerminalControl(apps: apps, processes: processes, titles: titles, tabs: tabs)
    var sessions: SessionControl {
        SessionControl(terminals: terminals, tabs: tabs, processes: processes, marks: marks, clock: FixedClock(now: c0))
    }
}

private func session(_ id: String = "s1", state: String = "working", pid: Int32 = 42, tty: String = "ttys001",
                     headless: Bool = false, cwd: String = "/w/proj", projectDir: String = "/w/proj", topic: String = "주제",
                     message: String = "", resumeCmd: String = "", paused: Bool = false) -> Session {
    Session(id: id, cwd: cwd, projectDir: projectDir, state: state, since: c0, updated: c0, headless: headless, tty: tty,
            topic: topic, message: message, resumeCmd: resumeCmd, file: "/s/\(id).json", ctx: nil, brain: nil, pid: pid,
            subagents: [], shells: 0, paused: paused, muted: false, limitAt: nil, name: nil)
}

private let vscode = "com.microsoft.VSCode"

// MARK: - 터미널 가르기

@Suite("다른 터미널: 조상 앱 번들 id → 터미널 종류") struct TerminalKindTests {
    @Test("Ghostty·iTerm2·Terminal·그 밖·못 찾음", arguments: [
        (String?.none, TerminalKind.unknown),
        ("com.mitchellh.ghostty", .ghostty),
        ("com.googlecode.iterm2", .iterm),
        ("com.apple.Terminal", .terminal),
        ("com.microsoft.VSCode", .other("com.microsoft.VSCode")),
        ("com.stablyai.orca", .other("com.stablyai.orca")),
        ("com.apple.terminal", .other("com.apple.terminal")),   // 대소문자를 가린다
        ("", .other("")),
    ])
    func 종류(_ bundleId: String?, _ kind: TerminalKind) {
        #expect(terminalKind(bundleId) == kind)
    }
}

struct OwnerRow: Row {
    let note: String
    var pid: Int32 = 42
    var tty = "ttys001"
    var ancestor: String? = nil
    var running: [String] = []
    var counts: [String: String?] = [:]   // "count iterm"·"count terminal" 의 답
    let owner: String?
    let calls: [String]
}

private let ownerRows: [OwnerRow] = [
    .init(note: "조상 앱이 있으면 그것 — tty 는 보지 않는다", ancestor: TerminalBundle.ghostty, owner: TerminalBundle.ghostty, calls: ["ancestor 42"]),
    .init(note: "조상이 그 밖의 앱이어도 그대로", ancestor: vscode, owner: vscode, calls: ["ancestor 42"]),
    .init(note: "조상 없음, tty 없음 → 못 찾음(앱에 묻지 않는다)", tty: "", owner: nil, calls: ["ancestor 42"]),
    .init(note: "pid 0 이면 조상을 보지 않고 tty 로 — iTerm2 먼저", pid: 0, running: [TerminalBundle.iterm, TerminalBundle.terminal],
          counts: ["count iterm": "1"], owner: TerminalBundle.iterm, calls: ["running iterm", "run count iterm"]),
    .init(note: "iTerm2 에 그 tty 가 없으면 Terminal", running: [TerminalBundle.iterm, TerminalBundle.terminal],
          counts: ["count iterm": "0", "count terminal": "2"], owner: TerminalBundle.terminal,
          calls: ["ancestor 42", "running iterm", "run count iterm", "running terminal", "run count terminal"]),
    .init(note: "꺼진 iTerm2 에는 AppleScript 를 보내지 않는다", running: [TerminalBundle.terminal], counts: ["count terminal": "1"],
          owner: TerminalBundle.terminal, calls: ["ancestor 42", "running iterm", "running terminal", "run count terminal"]),
    .init(note: "Terminal 도 꺼져 있으면 못 찾음", owner: nil, calls: ["ancestor 42", "running iterm", "running terminal"]),
    .init(note: "수가 아닌 답은 없음", running: [TerminalBundle.iterm], counts: ["count iterm": "abc"], owner: nil,
          calls: ["ancestor 42", "running iterm", "run count iterm", "running terminal"]),
    .init(note: "AppleScript 오류(nil)는 없음", running: [TerminalBundle.iterm, TerminalBundle.terminal], owner: nil,
          calls: ["ancestor 42", "running iterm", "run count iterm", "running terminal", "run count terminal"]),
    .init(note: "tty 가 tty 이름 꼴이 아니면(경로) 있는지 묻지 않고 없음 — AppleScript 를 보내지 않는다", pid: 0, tty: "../etc/x",
          running: [TerminalBundle.iterm, TerminalBundle.terminal], counts: ["count iterm": "1", "count terminal": "1"], owner: nil,
          calls: ["running iterm", "running terminal"]),
]

@Suite("다른 터미널: 세션을 띄운 앱 찾기") struct OwnerTests {
    @Test("조상 → (pid 0·못 찾으면) 떠 있는 iTerm2 → Terminal 의 tty", arguments: ownerRows)
    func 표(_ row: OwnerRow) {
        let w = World()
        w.processes.ancestors = row.ancestor.map { [42: $0] } ?? [:]
        w.apps.running = Set(row.running)
        for (k, v) in row.counts { w.apps.replies[k] = [v] }
        #expect(w.control.owner(session(pid: row.pid, tty: row.tty)) == row.owner)
        #expect(w.log.calls == row.calls)
    }

    @Test("찾은 앱은 세션마다 기억한다 — 다시 묻지 않는다") func 기억() {
        let w = World()
        w.processes.ancestors = [42: TerminalBundle.ghostty]
        #expect(w.control.owner(session()) == TerminalBundle.ghostty)
        w.processes.ancestors = [42: vscode]
        #expect(w.control.owner(session()) == TerminalBundle.ghostty)
        #expect(w.log.calls == ["ancestor 42"])
        // 다른 세션 id 는 따로
        #expect(w.control.owner(session("s2")) == vscode)
    }

    @Test("기억 정리 — 남길 세션 id 에 없는 세션의 기억은 지워 다음에 다시 찾고, 남긴 세션은 묻지 않는다") func 기억_정리() {
        let w = World()
        w.processes.ancestors = [42: TerminalBundle.ghostty]
        #expect(w.control.owner(session("s1")) == TerminalBundle.ghostty)
        #expect(w.control.owner(session("s2")) == TerminalBundle.ghostty)
        w.control.prune(keep: ["s1"])
        w.processes.ancestors = [42: vscode]
        #expect(w.control.owner(session("s1")) == TerminalBundle.ghostty)
        #expect(w.control.owner(session("s2")) == vscode)
        #expect(w.log.calls == ["ancestor 42", "ancestor 42", "ancestor 42"])
        w.control.prune(keep: [])
        #expect(w.control.owner(session("s1")) == vscode)
        #expect(w.log.calls.count == 4)
    }

    @Test("못 찾으면 기억하지 않는다 — 다음에 다시 찾는다") func 못_찾음() {
        let w = World()
        #expect(w.control.owner(session(tty: "")) == nil)
        w.processes.ancestors = [42: TerminalBundle.iterm]
        #expect(w.control.owner(session(tty: "")) == TerminalBundle.iterm)
        #expect(w.log.calls == ["ancestor 42", "ancestor 42"])
    }

    @Test("tty 확인 AppleScript 는 그 앱·그 tty 의 것") func 스크립트() {
        let w = World()
        w.apps.running = [TerminalBundle.iterm, TerminalBundle.terminal]
        _ = w.control.owner(session(pid: 0, tty: "ttys009"))
        #expect(w.apps.scripts == [ttyCountScript(app: TerminalBundle.iterm, tty: "ttys009"),
                                   ttyCountScript(app: TerminalBundle.terminal, tty: "ttys009")])
    }
}

// MARK: - 조작 길 고르기

struct CanDriveRow: Row {
    let note: String
    var headless = false
    var tty = "ttys001"
    var state = "working"
    var ancestor: String? = TerminalBundle.ghostty
    let can: Bool
    let asked: Bool   // 앱을 찾았나
}

private let canDriveRows: [CanDriveRow] = [
    .init(note: "Ghostty 세션", can: true, asked: true),
    .init(note: "승인 대기·대기·일시중지도", state: "waiting", can: true, asked: true),
    .init(note: "대기", state: "idle", can: true, asked: true),
    .init(note: "headless 는 앱을 찾지 않고 아니다", headless: true, can: false, asked: false),
    .init(note: "tty 없음", tty: "", can: false, asked: false),
    .init(note: "끝남", state: "ended", can: false, asked: false),
    .init(note: "iTerm2", ancestor: TerminalBundle.iterm, can: false, asked: true),
    .init(note: "Terminal", ancestor: TerminalBundle.terminal, can: false, asked: true),
    .init(note: "그 밖의 앱", ancestor: "com.microsoft.VSCode", can: false, asked: true),
]

@Suite("다른 터미널: 조작 길 고르기") struct RouteTests {
    @Test("canDrive — 끝나지 않은 tty 세션이고 Ghostty 에서 돌 때만", arguments: canDriveRows)
    func 키_보내기(_ row: CanDriveRow) {
        let w = World()
        w.processes.ancestors = row.ancestor.map { [42: $0] } ?? [:]
        #expect(w.control.canDrive(session(state: row.state, tty: row.tty, headless: row.headless)) == row.can)
        #expect(w.log.calls.contains("ancestor 42") == row.asked)
    }

    @Test("driveRoute — 보드 탭이 먼저, 다음이 다른 터미널의 Ghostty", arguments: driveRows)
    func 넣는_길(_ row: DriveRow) {
        let w = World()
        w.tabs.places["s1"] = row.place
        if row.foreground { w.tabs.foreground = [tabA] }
        w.processes.ancestors = row.ancestor.map { [42: $0] } ?? [:]
        w.apps.running = Set(row.running)
        w.apps.fallback["count terminal"] = "1"
        #expect(w.control.driveRoute(session(state: row.state, tty: row.tty, headless: row.headless)) == row.route)
        #expect(w.log.calls == row.calls)
    }

    @Test("focusRoute — iTerm2·Terminal 은 tty, 그 밖은 앱만, Ghostty·못 찾음은 Ghostty", arguments: [
        (String?.some(TerminalBundle.iterm), FocusRoute.tty(TerminalBundle.iterm)),
        (TerminalBundle.terminal, .tty(TerminalBundle.terminal)),
        ("com.microsoft.VSCode", .app("com.microsoft.VSCode")),
        ("com.stablyai.orca", .app("com.stablyai.orca")),
        (TerminalBundle.ghostty, .ghostty),
        (nil, .ghostty),
    ])
    func 포커스_길(_ ancestor: String?, _ route: FocusRoute) {
        let w = World()
        w.processes.ancestors = ancestor.map { [42: $0] } ?? [:]
        #expect(w.control.focusRoute(session(tty: "")) == route)
    }

    @Test("canFocus — 터미널이 있는 세션(headless·tty 없음·끝남이 아님)", arguments: [
        (false, "ttys001", "working", true), (false, "ttys001", "idle", true), (false, "ttys001", "paused", true),
        (true, "ttys001", "working", false), (false, "", "working", false), (false, "ttys001", "ended", false),
    ])
    func 포커스_가능(_ headless: Bool, _ tty: String, _ state: String, _ can: Bool) {
        let w = World()
        #expect(w.control.canFocus(session(state: state, tty: tty, headless: headless)) == can)
        #expect(w.log.calls.isEmpty)
    }

    @Test("ownerToActivate — headless·끝남은 앱을 찾지 않고 nil, 그 밖은 찾은 앱", arguments: [
        (false, "working", String?.some(TerminalBundle.iterm), String?.some(TerminalBundle.iterm), true),
        (false, "idle", nil, nil, true),
        (true, "working", TerminalBundle.iterm, nil, false),
        (false, "ended", TerminalBundle.iterm, nil, false),
    ])
    func 앞으로_가져올_앱(_ headless: Bool, _ state: String, _ ancestor: String?, _ app: String?, _ asked: Bool) {
        let w = World()
        w.processes.ancestors = ancestor.map { [42: $0] } ?? [:]
        #expect(w.control.ownerToActivate(session(state: state, tty: "", headless: headless)) == app)
        #expect(w.log.calls.contains("ancestor 42") == asked)
    }
}

struct DriveRow: Row {
    let note: String
    let place: SessionPlace
    var foreground = false
    var headless = false
    var tty = "ttys001"
    var state = "working"
    var ancestor: String? = TerminalBundle.ghostty
    var running: [String] = []
    let route: DriveRoute
    let calls: [String]
}

private let driveRows: [DriveRow] = [
    .init(note: "보드 탭 화면 앞의 대화형 claude → 그 화면(조상이 Ghostty 여도 앱을 찾지 않는다)", place: .tab(tabA, headless: false), foreground: true,
          route: .tab(tabA), calls: ["place s1", "fg A 42"]),
    .init(note: "보드 탭이지만 화면 앞이 아님(셸에서 & 로 띄움 등) → 넣지 않는다, Ghostty 로 가지 않는다", place: .tab(tabA, headless: false),
          route: .unavailable, calls: ["place s1", "fg A 42"]),
    .init(note: "보드 탭 안의 claude -p → 앞인지도 보지 않고 넣지 않는다", place: .tab(tabA, headless: true), foreground: true,
          route: .unavailable, calls: ["place s1"]),
    .init(note: "다른 터미널의 Ghostty 세션 → Ghostty", place: .terminal, route: .ghostty, calls: ["place s1", "ancestor 42"]),
    .init(note: "Ghostty 지만 headless → 앱을 찾지 않는다", place: .terminal, headless: true, route: .unavailable, calls: ["place s1"]),
    .init(note: "Ghostty 지만 tty 없음", place: .terminal, tty: "", route: .unavailable, calls: ["place s1"]),
    .init(note: "끝난 세션", place: .terminal, state: "ended", route: .unavailable, calls: ["place s1"]),
    .init(note: "iTerm2 세션 → 넣지 않는다", place: .terminal, ancestor: TerminalBundle.iterm, route: .unavailable, calls: ["place s1", "ancestor 42"]),
    .init(note: "Terminal 세션(조상 없음, tty 로 찾음) → 넣지 않는다", place: .terminal, ancestor: nil, running: [TerminalBundle.terminal],
          route: .unavailable, calls: ["place s1", "ancestor 42", "running iterm", "running terminal", "run count terminal"]),
    .init(note: "조상 없음, iTerm2·Terminal 꺼짐 → 넣지 않는다", place: .terminal, ancestor: nil, route: .unavailable,
          calls: ["place s1", "ancestor 42", "running iterm", "running terminal"]),
    .init(note: "그 밖의 앱(VS Code)", place: .terminal, ancestor: "com.microsoft.VSCode", route: .unavailable, calls: ["place s1", "ancestor 42"]),
    .init(note: "백그라운드", place: .background, route: .unavailable, calls: ["place s1"]),
    .init(note: "끝남", place: .ended, route: .unavailable, calls: ["place s1"]),
]

// MARK: - AppleScript 글자 (옮기기 전 코드의 출력 그대로)

private let goldenITermFocus = #"""
tell application id "com.googlecode.iterm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "/dev/ttys003" then
          select w
          tell t to select
          tell s to select
          activate
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
  return ""
end tell
"""#

private let goldenTerminalFocus = #"""
tell application id "com.apple.Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "/dev/ttys004" then
        set selected tab of w to t
        set index of w to 1
        activate
        return "ok"
      end if
    end repeat
  end repeat
  return ""
end tell
"""#

// tty 도 AppleScript 글자 막기를 거친다 — 따옴표가 든 이상한 tty 가 와도 스크립트 글자가 깨지지 않는다
private let goldenITermFocusQuote = #"""
tell application id "com.googlecode.iterm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "/dev/tt\"y" then
          select w
          tell t to select
          tell s to select
          activate
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
  return ""
end tell
"""#

private let goldenITermCount = #"tell application id "com.googlecode.iterm2" to return (count of (every session of every tab of every window whose tty is "/dev/ttys003")) as text"#
private let goldenTerminalCount = #"tell application id "com.apple.Terminal" to return (count of (every tab of every window whose tty is "/dev/ttys004")) as text"#
private let goldenOtherCount = #"tell application id "com.apple.Terminal" to return (count of (every tab of every window whose tty is "/dev/ttys005")) as text"#

private let goldenGhosttyList = #"""
tell application "Ghostty"
  set out to ""
  repeat with t in terminals
    set out to out & (id of t) & (character id 9) & (name of t) & linefeed
  end repeat
  return out
end tell
"""#

private let goldenGhosttyFind = #"""
tell application "Ghostty"
  repeat with t in terminals
    if name of t is "CBFIND-XXXXXXXX" then return id of t
  end repeat
  return ""
end tell
"""#

private let goldenGhosttyFocus = #"""
tell application "Ghostty"
  repeat with t in terminals
    if name of t is "CBFOCUS-XXXXXXXX" then
      focus t
      activate
      return id of t
    end if
  end repeat
  return ""
end tell
"""#

private let goldenSendExit = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      send key "escape" to t
      delay 0.4
      input text "/exit" to t
      delay 0.2
      send key "enter" to t
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

private let goldenSendQuote = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      input text "say \"hi\" \\ back\\slash" to t
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

// 지금 동작: 줄바꿈은 막지 않고 그대로 넣는다(AppleScript 글자는 줄바꿈을 품는다)
private let goldenSendNewline = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      input text "line1
line2" to t
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

private let goldenSendKorean = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      input text "보드에서 일시중지했던 세션을 재개합니다." to t
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

private let goldenSendEmptyText = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      input text "" to t
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

// 단계가 없으면 그 줄이 빈칸 여섯 개로 남는다 — 편집기가 줄 끝 빈칸을 지우지 않게 줄 목록으로 적는다
private let goldenSendEmpty = [
    #"tell application "Ghostty""#,
    #"  repeat with t in terminals"#,
    #"    if (id of t as text) is "ID-1" then"#,
    "      ",
    #"      return "ok""#,
    #"    end if"#,
    #"  end repeat"#,
    #"  return """#,
    #"end tell"#,
].joined(separator: "\n")

// 지금 동작: wait(과 모르는 종류)의 값은 막지 않고 그대로 delay 뒤에 붙는다
private let goldenSendWaitRaw = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      delay abc
      delay 1
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

private let goldenSendKeyQuote = #"""
tell application "Ghostty"
  repeat with t in terminals
    if (id of t as text) is "ID-1" then
      send key "a\"b" to t
      return "ok"
    end if
  end repeat
  return ""
end tell
"""#

struct SendRow: Row {
    let note: String
    let steps: [String]   // "종류=값"
    let golden: String
    var parsed: TerminalSteps {
        steps.map { s in
            let p = s.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            return (String(p[0]), p.count > 1 ? String(p[1]) : "")
        }
    }
}

private let sendRows: [SendRow] = [
    .init(note: "끝내기 단계 — key·wait·text", steps: ["key=escape", "wait=0.4", "text=/exit", "wait=0.2", "key=enter"], golden: goldenSendExit),
    .init(note: "따옴표·역슬래시가 든 text", steps: [#"text=say "hi" \ back\slash"#], golden: goldenSendQuote),
    .init(note: "줄바꿈이 든 text", steps: ["text=line1\nline2"], golden: goldenSendNewline),
    .init(note: "한글 text", steps: ["text=보드에서 일시중지했던 세션을 재개합니다."], golden: goldenSendKorean),
    .init(note: "빈 text", steps: ["text="], golden: goldenSendEmptyText),
    .init(note: "단계 없음", steps: [], golden: goldenSendEmpty),
    .init(note: "wait 값·모르는 종류는 그대로 delay", steps: ["wait=abc", "other=1"], golden: goldenSendWaitRaw),
    .init(note: "따옴표가 든 key", steps: [#"key=a"b"#], golden: goldenSendKeyQuote),
]

@Suite("다른 터미널: AppleScript 글자") struct ScriptTests {
    @Test("iTerm2·Terminal 의 tty 포커스 — 그 밖의 앱 id 는 Terminal 것") func tty_포커스() {
        #expect(ttyFocusScript(app: TerminalBundle.iterm, tty: "ttys003") == goldenITermFocus)
        #expect(ttyFocusScript(app: TerminalBundle.terminal, tty: "ttys004") == goldenTerminalFocus)
        #expect(ttyFocusScript(app: "com.other", tty: "ttys004") == goldenTerminalFocus)
        #expect(ttyFocusScript(app: TerminalBundle.iterm, tty: "tt\"y") == goldenITermFocusQuote)
        #expect(ttyFocusScript(app: TerminalBundle.terminal, tty: #"a\b"#)
                == goldenTerminalFocus.replacingOccurrences(of: #""/dev/ttys004""#, with: #""/dev/a\\b""#))
    }

    @Test("iTerm2·Terminal 의 tty 세기 — 그 밖의 앱 id 는 Terminal 것") func tty_세기() {
        #expect(ttyCountScript(app: TerminalBundle.iterm, tty: "ttys003") == goldenITermCount)
        #expect(ttyCountScript(app: TerminalBundle.terminal, tty: "ttys004") == goldenTerminalCount)
        #expect(ttyCountScript(app: "com.other", tty: "ttys005") == goldenOtherCount)
    }

    @Test("tty 세기 — 따옴표·역슬래시가 든 tty 는 막아 넣는다") func tty_세기_막기() {
        #expect(ttyCountScript(app: TerminalBundle.iterm, tty: #"t"t"#)
                == #"tell application id "com.googlecode.iterm2" to return (count of (every session of every tab of every window whose tty is "/dev/t\"t")) as text"#)
        #expect(ttyCountScript(app: TerminalBundle.terminal, tty: #"t\t"#)
                == #"tell application id "com.apple.Terminal" to return (count of (every tab of every window whose tty is "/dev/t\\t")) as text"#)
    }

    @Test("Ghostty 목록·표지 찾기·표지 포커스") func ghostty() {
        #expect(ghosttyTerminalsScript == goldenGhosttyList)
        #expect(ghosttyFindScript(marker: "CBFIND-XXXXXXXX") == goldenGhosttyFind)
        #expect(ghosttyFocusScript(marker: "CBFOCUS-XXXXXXXX") == goldenGhosttyFocus)
    }

    @Test("Ghostty 에 단계 보내기", arguments: sendRows)
    func 보내기(_ row: SendRow) {
        #expect(ghosttySendScript(terminal: "ID-1", steps: row.parsed) == row.golden)
    }

    @Test("Ghostty 에 단계 보내기 — 따옴표·역슬래시가 든 터미널 id 는 막아 넣는다(보통 id 는 그대로)") func 보내기_id_막기() {
        #expect(ghosttySendScript(terminal: #"I"D\1"#, steps: SessionSteps.exit)
                == goldenSendExit.replacingOccurrences(of: #"is "ID-1" then"#, with: #"is "I\"D\\1" then"#))
        #expect(ghosttySendScript(terminal: "6A1F2B3C-0000-4D5E-8F90-ABCDEF012345", steps: SessionSteps.exit)
                == goldenSendExit.replacingOccurrences(of: "ID-1", with: "6A1F2B3C-0000-4D5E-8F90-ABCDEF012345"))
    }

    @Test("AppleScript 글자 막기 — 역슬래시를 먼저, 다음 큰따옴표. 줄바꿈·작은따옴표·한글은 그대로", arguments: [
        ("", ""), ("abc", "abc"), (#"a"b"#, #"a\"b"#), (#"a\b"#, #"a\\b"#), (#"\""#, #"\\\""#),
        ("a\nb", "a\nb"), ("it's", "it's"), ("한글", "한글"),
    ])
    func 막기(_ input: String, _ out: String) {
        #expect(appleScriptEscaped(input) == out)
    }

    @Test("tty 이름 확인 — 비지 않은 ASCII 영숫자만(/dev/ 아래 터미널 이름). 경로·공백·따옴표·한글·전각은 아니다", arguments: [
        ("ttys003", true), ("ttyp1", true), ("ttys012", true), ("ttyqa", true), ("console", true), ("TTYS003", true),
        ("", false), ("../etc/x", false), ("ttys003/../x", false), ("/dev/ttys003", false), ("tty/s003", false),
        ("..", false), (".", false), ("ttys.003", false), ("ttys 003", false), (" ttys003", false), ("ttys003\n", false),
        ("tt\"y", false), ("tt'y", false), (#"tty\s"#, false), ("ttys-003", false), ("ttys_003", false), ("ttys003\u{0}", false),
        ("한글", false), ("ttys003한", false), ("ttys００３", false), ("ｔｔｙｓ003", false),
    ])
    func tty_이름(_ tty: String, _ ok: Bool) {
        #expect(isTtyName(tty) == ok)
    }

    @Test("표지 제목 — 앞말-UUID 앞 8글자") func 표지() {
        let u = UUID(uuidString: "1A2B3C4D-0000-0000-0000-000000000000")!
        #expect(titleMarker("CBFIND", u) == "CBFIND-1A2B3C4D")
        #expect(titleMarker("CBFOCUS", u) == "CBFOCUS-1A2B3C4D")
    }

    @Test("Ghostty 목록 풀기 — 줄마다 첫 탭 문자에서 나누고, 탭 없는 줄은 뺀다", arguments: [
        (String?.none, [String: String]()),
        ("", [:]),
        ("A1\tt1\nB2\told title\n", ["A1": "t1", "B2": "old title"]),
        ("A1\tt\twith tab\n", ["A1": "t\twith tab"]),
        ("no tab\nA1\t\n", ["A1": ""]),
        ("\tempty id\n", ["": "empty id"]),
        ("A1\tfirst\nA1\tsecond", ["A1": "second"]),
        ("\n\nA1\tx\n\n", ["A1": "x"]),
    ])
    func 목록_풀기(_ list: String?, _ titles: [String: String]) {
        #expect(ghosttyTitles(list) == titles)
    }
}

// MARK: - Ghostty·iTerm2·Terminal 조작

private let marker = "CBFIND-TEST0001"

@Suite("다른 터미널: Ghostty 터미널 찾기·보내기·포커스, tty 포커스") struct GhosttyTests {
    @Test("두 번째에 찾으면 원래 제목으로 되돌리고 그 id") func 찾음() {
        let w = World()
        w.apps.replies = ["list": ["A1\tt1\nB2\told title\n"], "find": ["", "B2"]]
        #expect(w.control.ghosttyTerminalId(session(tty: "ttys009"), marker: marker) == "B2")
        #expect(w.log.calls == ["run list", "title ttys009=\(marker)", "wait 0.05", "run find", "wait 0.05", "run find",
                                "title ttys009=old title"])
        #expect(w.apps.scripts == [ghosttyTerminalsScript, ghosttyFindScript(marker: marker), ghosttyFindScript(marker: marker)])
    }

    @Test("열 번 못 찾으면 표지를 폴더 이름으로 바꾸고 nil") func 못_찾음() {
        let w = World()
        w.apps.replies = ["list": ["A1\tt1\n"]]
        w.apps.fallback = ["find": ""]
        #expect(w.control.ghosttyTerminalId(session(tty: "ttys009", cwd: "/w/proj"), marker: marker) == nil)
        let polls = Array(repeating: ["wait 0.05", "run find"], count: 10).flatMap { $0 }
        #expect(w.log.calls == ["run list", "title ttys009=\(marker)"] + polls + ["title ttys009=proj"])
    }

    @Test("찾았어도 표지를 쓰기 전 목록에 없던 터미널이면 nil") func 목록에_없음() {
        let w = World()
        w.apps.replies = ["list": ["A1\tt1\n"], "find": ["Z9"]]
        #expect(w.control.ghosttyTerminalId(session(tty: "ttys009", cwd: "/w/proj"), marker: marker) == nil)
        #expect(w.log.calls == ["run list", "title ttys009=\(marker)", "wait 0.05", "run find", "title ttys009=proj"])
    }

    @Test("목록을 못 받으면(AppleScript 오류) 찾아도 nil") func 목록_오류() {
        let w = World()
        w.apps.replies = ["find": ["A1"]]
        #expect(w.control.ghosttyTerminalId(session(tty: "ttys009", cwd: "/a/b/c"), marker: marker) == nil)
        #expect(w.log.calls.last == "title ttys009=c")
    }

    @Test("ghosttyDrive — 찾으면 그 id 로 보내고 ok 면 true, 못 찾으면 보내지 않는다", arguments: [
        (String?.some("ok"), true), ("", false), (nil, false),
    ])
    func 보내기(_ reply: String?, _ ok: Bool) {
        let w = World()
        w.apps.replies = ["list": ["B2\told\n"], "find": ["B2"], "send": [reply]]
        #expect(w.control.ghosttyDrive(session(), SessionSteps.exit, marker: marker) == ok)
        #expect(w.log.calls.suffix(2) == ["title ttys001=old", "run send"])
        #expect(w.apps.scripts.last == ghosttySendScript(terminal: "B2", steps: SessionSteps.exit))
    }

    @Test("ghosttyDrive — 터미널을 못 찾으면 보내지 않고 false") func 보내기_못_찾음() {
        let w = World()
        w.apps.fallback = ["find": ""]
        #expect(w.control.ghosttyDrive(session(), SessionSteps.pause, marker: marker) == false)
        #expect(!w.log.calls.contains("run send"))
    }

    @Test("focusGhostty — 찾으면 포커스 스크립트로 앞으로 오고 원래 제목으로 되돌려 true") func 포커스() {
        let w = World()
        w.apps.replies = ["list": ["A1\tt1\nB2\told\n"], "focus": ["B2"]]
        #expect(w.control.focusGhostty(session(tty: "ttys009"), marker: "CBFOCUS-1"))
        #expect(w.log.calls == ["run list", "title ttys009=CBFOCUS-1", "wait 0.05", "run focus", "title ttys009=old"])
        #expect(w.apps.scripts[1] == ghosttyFocusScript(marker: "CBFOCUS-1"))
    }

    @Test("focusGhostty — 못 찾으면 표지를 폴더 이름으로 바꾸고 false(앱이 기본 터미널을 앞으로)") func 포커스_못_찾음() {
        let w = World()
        w.apps.replies = ["list": ["A1\tt1\n"]]
        w.apps.fallback = ["focus": ""]
        #expect(!w.control.focusGhostty(session(tty: "ttys009", cwd: "/w/proj"), marker: "CBFOCUS-1"))
        #expect(w.log.calls.filter { $0 == "run focus" }.count == 10)
        #expect(w.log.calls.last == "title ttys009=proj")
    }

    @Test("focusGhostty 도 찾은 글자가 비었으면 못 찾은 것 — 목록에 id 가 빈 줄이 있어도 표지를 폴더 이름으로 바꾸고 false(ghosttyTerminalId 와 같다)")
    func 빈_id() {
        let w = World()
        w.apps.replies = ["list": ["\tempty\n"]]
        w.apps.fallback = ["focus": ""]
        #expect(!w.control.focusGhostty(session(tty: "ttys009", cwd: "/w/proj"), marker: "CBFOCUS-1"))
        #expect(w.log.calls.filter { $0 == "run focus" }.count == 10)
        #expect(w.log.calls.last == "title ttys009=proj")
        let v = World()
        v.apps.replies = ["list": ["\tempty\n"]]
        v.apps.fallback = ["find": ""]
        #expect(v.control.ghosttyTerminalId(session(tty: "ttys009", cwd: "/w/proj"), marker: marker) == nil)
        #expect(v.log.calls.last == "title ttys009=proj")
    }

    @Test("focusByTty — 그 앱의 tty 포커스 스크립트, ok 일 때만 true", arguments: [
        (String?.some("ok"), true), ("", false), ("OK", false), (nil, false),
    ])
    func tty_포커스(_ reply: String?, _ ok: Bool) {
        let w = World()
        w.apps.replies = ["tty iterm": [reply]]
        #expect(w.control.focusByTty(TerminalBundle.iterm, "ttys003") == ok)
        #expect(w.apps.scripts == [goldenITermFocus])
    }

    @Test("focusByTty·ttyExists — tty 가 tty 이름 꼴이 아니면 AppleScript 를 보내지 않고 못 찾음·없음",
          arguments: ["", "../etc/x", "ttys003/../x", "tt\"y", "ttys003 "])
    func tty_이름_아님(_ tty: String) {
        let w = World()
        w.apps.fallback = ["tty iterm": "ok", "count terminal": "1"]
        #expect(!w.control.focusByTty(TerminalBundle.iterm, tty))
        #expect(!w.control.ttyExists(TerminalBundle.terminal, tty))
        #expect(w.apps.scripts.isEmpty)
        #expect(w.log.calls.isEmpty)
    }

    @Test("ttyExists — 수가 0 보다 크면", arguments: [
        (String?.some("1"), true), ("3", true), ("0", false), ("-1", false), ("", false), ("x", false), (nil, false),
    ])
    func tty_있음(_ reply: String?, _ exists: Bool) {
        let w = World()
        w.apps.replies = ["count terminal": [reply]]
        #expect(w.control.ttyExists(TerminalBundle.terminal, "ttys004") == exists)
        #expect(w.apps.scripts == [goldenTerminalCount])
    }
}

// MARK: - 세션 조작

private let exitSteps = "key:escape wait:0.4 text:/exit wait:0.2 key:enter"
private let carrySteps = "text:\n\(resumePrompt) wait:0.2 key:enter"
private let carried = "claude --resume s1 +md '\(carryOnPrompt)'"
private let noResponse = "toast proj: 지금 멈출 응답이 없어 일시중지하지 않았습니다"

@Suite("세션 조작: 일시중지·재개") struct PauseResumeTests {
    @Test("일시중지 — Esc 를 넣고, 넣었으면 그 뒤 시각과 주제로 표지") func 일시중지() throws {
        let w = World()
        w.sessions.pause(session(topic: "할 일"))
        #expect(w.log.calls == ["drive s1 key:escape", "paused-write s1"])
        let mark = try #require(w.marks.paused["s1"].flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] })
        #expect(mark["paused_at"] as? Double == c0.timeIntervalSince1970)
        #expect(mark["topic"] as? String == "할 일")
        #expect(mark.count == 2)
    }

    @Test("일시중지 — 못 넣으면 표지를 남기지 않는다") func 일시중지_실패() {
        let w = World()
        w.terminals.driveOK = false
        w.sessions.pause(session())
        #expect(w.log.calls == ["drive s1 key:escape"])
    }

    // 대기 세션에 Esc 와 표지를 남기면 멈춘 것이 없는데 일시중지 카드가 되고, 재개가 이어서 진행 요청을 넣어 지난 작업을 다시 시작하게 한다
    @Test("일시중지 — 멈출 응답이 있는 세션(작업 중·승인 대기)에만, 메뉴와 같은 판정(--action pause 도 여기로 온다)", arguments: [
        ("working", ["drive s1 key:escape", "paused-write s1"]),
        ("waiting", ["drive s1 key:escape", "paused-write s1"]),
        ("idle", [noResponse]),
        ("paused", [noResponse]),
        ("ended", [noResponse]),
    ])
    func 일시중지_상태(_ state: String, _ calls: [String]) {
        let w = World()
        let s = session(state: state, paused: state == "paused")
        w.sessions.pause(s)
        #expect(w.log.calls == calls)
        #expect(isPausable(s) == (calls.count == 2))
    }

    @Test("재개", arguments: resumeRows)
    func 재개(_ row: ResumeRow) {
        let w = World()
        if row.alive { w.processes.alive = [42] }
        w.terminals.driveOK = row.driveOK
        w.sessions.resumePaused(session(state: row.state, pid: row.pid, paused: true))
        #expect(w.log.calls == row.calls)
    }
}

struct ResumeRow: Row {
    let note: String
    var state = "paused"
    var pid: Int32 = 42
    var alive = true
    var driveOK = true
    let calls: [String]
}

private let reopenCarry = ["command s1 claude --resume s1", "paused-clear s1", "prompt s1 /w/proj", "launch /w/proj s1 off=false \(carried)"]
private let stillRunning = "toast proj: 세션이 아직 돌고 있지만 그 터미널에 키를 넣지 못했습니다. 그 터미널에서 이어 가 주세요"

private let resumeRows: [ResumeRow] = [
    .init(note: "살아 있고 넣으면 표지만 지운다", calls: ["alive 42", "drive s1 \(carrySteps)", "paused-clear s1"]),
    .init(note: "살아 있지만 못 넣으면 이어 열지 않고(claude 가 둘이 된다) 안내만, 표지는 그대로", driveOK: false,
          calls: ["alive 42", "drive s1 \(carrySteps)", stillRunning]),
    .init(note: "pid 가 있고 꺼졌으면 넣지 않고 이어 연다(보드가 다시 뜬 직후 판정이 아직 끝남으로 바꾸지 못한 때)", alive: false,
          calls: ["alive 42"] + reopenCarry),
    .init(note: "끝난 세션은 살아 있는지 보지 않고 이어 연다", state: "ended", calls: reopenCarry),
    .init(note: "끝난 세션은 pid 0 이어도 이어 연다", state: "ended", pid: 0, calls: reopenCarry),
    .init(note: "pid 0 이고 끝나지 않았으면(판정이 Claude Code 세션 파일로 생사를 본 세션) 살아 있다고 보고 요청을 넣는다", pid: 0,
          calls: ["drive s1 \(carrySteps)", "paused-clear s1"]),
    .init(note: "pid 0 이고 끝나지 않았는데 못 넣으면 안내만, 표지는 그대로", pid: 0, driveOK: false,
          calls: ["drive s1 \(carrySteps)", stillRunning]),
]

struct StopRow: Row {
    let note: String
    let place: SessionPlace
    var pid: Int32 = 42
    var state = "working"
    var driveOK = true
    var alive = true        // 기록의 pid 가 살아 있다
    let calls: [String]
}

private let stopRows: [StopRow] = [
    .init(note: "진짜 백그라운드 세션은 표지만 지우고 건드리지 않는다", place: .background, calls: ["paused-clear s1", "place s1"]),
    .init(note: "끝내기 단계를 넣으면 신호는 보내지 않는다", place: .terminal, calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)"]),
    .init(note: "못 넣으면(Ghostty 밖 터미널 등) pid 가 살아 있을 때 SIGTERM", place: .terminal, driveOK: false,
          calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)", "alive 42", "terminate 42"]),
    .init(note: "못 넣었는데 pid 가 이미 없으면 신호를 보내지 않는다", place: .terminal, driveOK: false, alive: false,
          calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)", "alive 42"]),
    .init(note: "보드 탭 안의 claude -p — 넣는 길이 없어 살아 있으면 SIGTERM", place: .tab(tabA, headless: true), driveOK: false,
          calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)", "alive 42", "terminate 42"]),
    .init(note: "일시중지 세션도 못 넣고 살아 있으면 SIGTERM", place: .terminal, state: "paused", driveOK: false,
          calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)", "alive 42", "terminate 42"]),
    .init(note: "pid 가 없으면 못 넣어도 살아 있는지 보지 않고 신호를 보내지 않는다", place: .terminal, pid: 0, driveOK: false,
          calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)"]),
    .init(note: "끝난 세션(--action stop 으로 올 수 있다)은 표지만 지우고 넣지도 신호를 보내지도 않는다 — 카드가 이틀 남는 동안 기록의 pid 를 다른 프로세스가 다시 쓸 수 있다",
          place: .ended, state: "ended", driveOK: false, calls: ["paused-clear s1", "place s1"]),
    .init(note: "기록은 끝났어도 보드 탭이 그 세션의 claude 를 돌리고 있으면(place 가 탭) 그 탭에 넣는다 — 가르는 것은 place",
          place: .tab(tabA, headless: false), state: "ended", calls: ["paused-clear s1", "place s1", "drive s1 \(exitSteps)"]),
]

struct MoveRow: Row {
    let note: String
    var driveOK = true
    var exited = true
    let calls: [String]
}

private let moveRows: [MoveRow] = [
    .init(note: "끝내고 끝나면 보드 새 탭에서 이어 연다(이어서 진행 요청은 붙이지 않는다)",
          calls: ["drive s1 \(exitSteps)", "wait-exit 42 8.0", "command s1 claude --resume s1", "prompt s1 /w/proj",
                  "launch /w/proj s1 off=false claude --resume s1 +md"]),
    .init(note: "끝내기 단계를 못 넣으면 안내만", driveOK: false,
          calls: ["drive s1 \(exitSteps)", "toast proj: 세션 터미널을 찾지 못해 보드로 옮기지 못했습니다"]),
    .init(note: "8초 안에 끝나지 않으면 안내만", exited: false,
          calls: ["drive s1 \(exitSteps)", "wait-exit 42 8.0", "toast proj: 세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요"]),
]

@Suite("세션 조작: 중단·보드로 옮기기") struct StopMoveTests {
    @Test("중단(확인 뒤)", arguments: stopRows)
    func 중단(_ row: StopRow) {
        let w = World()
        w.tabs.places["s1"] = row.place
        w.terminals.driveOK = row.driveOK
        if row.alive { w.processes.alive = [row.pid] }
        w.sessions.stop(session(state: row.state, pid: row.pid))
        #expect(w.log.calls == row.calls)
    }

    @Test("보드로 옮기기(확인 뒤)", arguments: moveRows)
    func 옮기기(_ row: MoveRow) {
        let w = World()
        w.terminals.driveOK = row.driveOK
        w.terminals.exited = row.exited
        w.sessions.moveToBoard(session(cwd: "/w/proj/sub/../proj"))
        #expect(w.log.calls == row.calls)
    }

    @Test("옮기기 전에 묻는 것 — 작업 중·승인 대기만", arguments: [
        ("working", true), ("waiting", true), ("idle", false), ("paused", false), ("ended", false),
    ])
    func 확인(_ state: String, _ ask: Bool) {
        #expect(moveNeedsConfirm(session(state: state)) == ask)
    }
}

struct ReopenRow: Row {
    let note: String
    var resumeCmd = ""
    var carryOn = false
    var prompt = false      // 그 세션의 셸 프롬프트로 남은 화면이 있다
    var off = false
    var projectDir = "/w/proj"
    let calls: [String]
}

private let reopenRows: [ReopenRow] = [
    .init(note: "resume_cmd 가 없으면 claude --resume <id>, 화면이 없으면 프로젝트 경로에 새 탭",
          calls: ["command s1 claude --resume s1", "prompt s1 /w/proj", "launch /w/proj s1 off=false claude --resume s1 +md"]),
    .init(note: "resume_cmd 그대로(꾸미기는 포트가 고친다)", resumeCmd: "claude --model opus --resume s1",
          calls: ["command s1 claude --model opus --resume s1", "prompt s1 /w/proj",
                  "launch /w/proj s1 off=false claude --model opus --resume s1 +md"]),
    .init(note: "셸 프롬프트로 남은 화면이 있으면 그 화면에 넣고 보인다", prompt: true,
          calls: ["command s1 claude --resume s1", "prompt s1 /w/proj", "run B s1 off=false claude --resume s1 +md", "reveal B"]),
    .init(note: "이어서 진행 — 꾸민 명령 뒤에 요청을 붙이고 표지를 지운 뒤 화면을 찾는다", carryOn: true, prompt: true,
          calls: ["command s1 claude --resume s1", "paused-clear s1", "prompt s1 /w/proj", "run B s1 off=false \(carried)", "reveal B"]),
    .init(note: "꾸미기를 끈 세션은 끈 채로", off: true,
          calls: ["command s1 claude --resume s1", "prompt s1 /w/proj", "launch /w/proj s1 off=true claude --resume s1 +md"]),
    .init(note: "cwd 가 아니라 프로젝트 경로(claude 를 띄운 폴더)", projectDir: "/w",
          calls: ["command s1 claude --resume s1", "prompt s1 /w", "launch /w s1 off=false claude --resume s1 +md"]),
]

struct OpenRow: Row {
    let note: String
    let place: SessionPlace
    var parked = false
    var double = false
    var headless = false
    var paused = false
    var mode: String? = nil
    let calls: [String]
}

private let openRows: [OpenRow] = [
    .init(note: "넘긴 대화를 붙여 보여 주는 화면이 먼저(어디서 도는지도 보지 않는다)", place: .terminal, parked: true, calls: ["parked s1", "reveal P"]),
    .init(note: "보드 탭 세션은 그 화면으로", place: .tab(tabA, headless: false), calls: ["parked s1", "place s1", "reveal A"]),
    .init(note: "보드 탭 안의 claude -p 도 그 화면으로", place: .tab(tabA, headless: true), calls: ["parked s1", "place s1", "reveal A"]),
    .init(note: "끝난 세션은 보드 새 탭에서 이어 연다", place: .ended,
          calls: ["parked s1", "place s1", "command s1 claude --resume s1", "prompt s1 /w/proj", "launch /w/proj s1 off=false claude --resume s1 +md"]),
    .init(note: "끝난 일시중지 세션은 이어서 진행", place: .ended, paused: true,
          calls: ["parked s1", "place s1", "command s1 claude --resume s1", "paused-clear s1", "prompt s1 /w/proj",
                  "launch /w/proj s1 off=false \(carried)"]),
    .init(note: "끝난 백그라운드 세션은 아무것도 하지 않는다", place: .ended, headless: true, calls: ["parked s1", "place s1"]),
    .init(note: "백그라운드 세션은 안내만 — 실행 방식", place: .background, mode: "--bg",
          calls: ["parked s1", "place s1", "mode s1",
                  "toast 주제: claude --bg 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다."]),
    .init(note: "백그라운드 세션 — 실행 방식을 모르면 -p", place: .background,
          calls: ["parked s1", "place s1", "mode s1",
                  "toast 주제: claude -p 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다."]),
    .init(note: "다른 터미널 — 한 번은 앱만", place: .terminal, calls: ["parked s1", "place s1", "bring s1"]),
    .init(note: "다른 터미널 — 두 번은 터미널 창까지", place: .terminal, double: true, calls: ["parked s1", "place s1", "focus s1"]),
]

struct ReopenEndedRow: Row {
    let note: String
    let place: SessionPlace
    var state = "ended"
    var headless = false
    var paused = false
    let calls: [String]
}

private let reopenPlain = ["command s1 claude --resume s1", "prompt s1 /w/proj", "launch /w/proj s1 off=false claude --resume s1 +md"]
private let notEnded = "toast proj: 세션이 아직 끝나지 않아 다시 열지 않았습니다"

private let reopenEndedRows: [ReopenEndedRow] = [
    .init(note: "끝난 세션은 이어 열되 일시중지하지 않았으면 이어서 진행 요청을 붙이지 않는다(메뉴의 세션 다시 열기)", place: .ended,
          calls: ["place s1"] + reopenPlain),
    .init(note: "끝난 일시중지 세션은 이어서 진행(메뉴의 세션 재개)", place: .ended, paused: true, calls: ["place s1"] + reopenCarry),
    .init(note: "끝난 백그라운드 세션도 대화형으로 이어 연다(메뉴에 있다, 한 번 누르기와 다름)", place: .ended, headless: true,
          calls: ["place s1"] + reopenPlain),
    .init(note: "기록은 끝났어도 보드 탭이 그 세션의 claude 를 돌리고 있으면(place 가 탭) 열지 않는다(같은 대화의 claude 가 둘이 된다)",
          place: .tab(tabA, headless: false), calls: ["place s1", notEnded]),
    .init(note: "다른 터미널에서 도는 세션은 열지 않는다", place: .terminal, state: "working", calls: ["place s1", notEnded]),
    .init(note: "살아 있는 일시중지 세션도 열지 않는다(재개를 쓴다)", place: .terminal, state: "paused", paused: true,
          calls: ["place s1", notEnded]),
    .init(note: "도는 백그라운드 세션도 열지 않는다", place: .background, state: "working", headless: true, calls: ["place s1", notEnded]),
]

@Suite("세션 조작: 이어 열기·한 번 누르기") struct ReopenOpenTests {
    @Test("이어 열기", arguments: reopenRows)
    func 이어_열기(_ row: ReopenRow) {
        let w = World()
        if row.prompt { w.tabs.prompts["s1|\(row.projectDir)"] = tabB }
        w.tabs.mdRenderOff = row.off
        w.sessions.reopen(session(state: "ended", cwd: "/w/proj/sub", projectDir: row.projectDir, resumeCmd: row.resumeCmd), carryOn: row.carryOn)
        #expect(w.log.calls == row.calls)
    }

    @Test("한 번 누르기의 갈래", arguments: openRows)
    func 누르기(_ row: OpenRow) {
        let w = World()
        w.tabs.places["s1"] = row.place
        if row.parked { w.tabs.parked["s1"] = parkedP }
        if let m = row.mode { w.tabs.modes["s1"] = m }
        let state = row.place == .ended ? "ended" : "working"
        w.sessions.open(session(state: state, headless: row.headless, paused: row.paused), double: row.double)
        #expect(w.log.calls == row.calls)
    }

    @Test("명령줄 다시 열기(--action reopen)", arguments: reopenEndedRows)
    func 명령줄_다시_열기(_ row: ReopenEndedRow) {
        let w = World()
        w.tabs.places["s1"] = row.place
        w.sessions.reopenEnded(session(state: row.state, headless: row.headless, paused: row.paused))
        #expect(w.log.calls == row.calls)
    }

    @Test("이어서 진행 요청 붙이기 — 작은따옴표로 감싸고 안의 ' 는 '\\''", arguments: [
        ("claude --resume s1", "이어 가요", "claude --resume s1 '이어 가요'"),
        ("c", "it's", #"c 'it'\''s'"#),
        ("c", "", "c ''"),
        ("c", "''", #"c ''\'''\'''"#),
    ])
    func 요청_붙이기(_ command: String, _ prompt: String, _ out: String) {
        #expect(carryOnCommand(command, prompt: prompt) == out)
    }

    @Test("이어서 진행 요청 — 기본 글자") func 기본_요청() {
        #expect(carryOnCommand("claude") == "claude '보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요.'")
    }

    // 실측(Claude Code 2.1.312): 첫 출력 전에 Esc 로 멈추면 그 요청은 대화 맥락에서 빠지고 입력 칸에 되돌아온다. 재개 글을 그 뒤에 바로
    // 붙이면 두 글이 이어 붙고, 지우면 할 일이 사라진다 — 줄을 바꿔 한 메시지로 보내고, 위에 요청이 있는 경우와 없는 경우를 모두 받는 글로
    @Test("살아 있는 세션의 재개 글 — 줄바꿈으로 시작하고, 이어서 진행하되 위에 되돌아온 요청이 있으면 그것이 중단된 작업") func 재개_글() {
        #expect(SessionSteps.resume.map { "\($0.0):\($0.1)" } == ["text:\n\(resumePrompt)", "wait:0.2", "key:enter"])
        #expect(resumePrompt == "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요. 이 메시지에 이 문장보다 먼저 적힌 요청이 있으면 그 요청이 중단된 작업입니다.")
    }

    @Test("이어 열 명령의 바탕", arguments: [("", "claude --resume s1"), ("claude --resume s1 --model x", "claude --resume s1 --model x")])
    func 바탕(_ resumeCmd: String, _ out: String) {
        #expect(resumeBaseCommand(session(resumeCmd: resumeCmd)) == out)
    }

    @Test("백그라운드 안내의 제목 — 주제, 없으면 마지막 말, 없으면 요청 전", arguments: [
        ("주제", "말", "주제"), ("", "말", "말"), ("", "", "요청 전"),
    ])
    func 안내_제목(_ topic: String, _ message: String, _ title: String) {
        #expect(backgroundNote(session(topic: topic, message: message), mode: nil).hasPrefix("\(title): claude -p 로"))
    }
}

// MARK: - 끝나기 기다리기

@Suite("세션 조작: 끝나기 기다리기") struct ExitWaitTests {
    @Test("한 번의 판단", arguments: [
        (Int32(0), true, 8.0, ExitWait.exited, false),
        (-1, true, 8.0, .exited, false),
        (42, false, 8.0, .exited, true),
        (42, true, 8.0, .poll(after: 0.2, remaining: 8.0 - 0.2), true),
        (42, true, 0.0, .timedOut, true),
        (42, true, -0.1, .timedOut, true),
        (42, false, 0.0, .exited, true),   // 끝났는지를 먼저 본다
    ])
    func 판단(_ pid: Int32, _ alive: Bool, _ timeout: Double, _ out: ExitWait, _ asked: Bool) {
        let w = World()
        if alive { w.processes.alive = [pid] }
        #expect(exitWait(pid: pid, timeout: timeout, processes: w.processes) == out)
        #expect(w.log.calls == (asked ? ["alive \(pid)"] : []))
    }

    @Test("8초 — 끝나지 않으면 0.2초씩 40번 다시 본 뒤 시간 초과(부동소수 그대로), 중간에 끝나면 바로") func 제한_시간() {
        let w = World()
        w.processes.alive = [42]
        var timeout = 8.0, polls = 0
        var out = exitWait(pid: 42, timeout: timeout, processes: w.processes)
        while case .poll(let after, let remaining) = out {
            #expect(after == 0.2)
            polls += 1
            timeout = remaining
            out = exitWait(pid: 42, timeout: timeout, processes: w.processes)
        }
        #expect(out == .timedOut)
        #expect(polls == 40)
        #expect(w.log.calls.count == 41)

        let v = World()
        v.processes.alive = [42]
        out = exitWait(pid: 42, timeout: 8, processes: v.processes)
        if case .poll(_, let remaining) = out {
            v.processes.alive = []
            #expect(exitWait(pid: 42, timeout: remaining, processes: v.processes) == .exited)
        } else {
            Issue.record("처음에는 다시 봐야 한다")
        }
    }
}
