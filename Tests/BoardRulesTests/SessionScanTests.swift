// 세션 상태 판정(scanSessions) 시험 — 포트마다 가짜 어댑터를 넣어 파일·프로세스·Claude Code 없이 돌린다.
// 가짜는 이 파일에만 둔다(Sources/BoardRules 에 두지 않는다). 가짜는 부른 것을 적어 두어, 지운 기록·캐시 정리 목록·
// 대화 기록을 언제 읽는지(1초마다 도는 길이라 곧 CPU)까지 확인한다. 기대값은 Store.reload 에 있던 지금 동작 그대로다
import Foundation
import Testing
import BoardRules

private let t0 = Date(timeIntervalSince1970: 1_000_000)
private let day: Double = 86400

// MARK: - 가짜 어댑터 (시험마다 새로 만들고 한 스레드에서만 쓴다 — 포트가 Sendable 이라 @unchecked)

private final class FakeRecords: SessionRecords, @unchecked Sendable {
    var items: [SessionRecord] = []
    var paused = Set<String>(), muted = Set<String>()
    var muteAll = false
    private(set) var removed: [String] = []          // 지운 기록의 파일 이름
    private(set) var loads = 0                        // 세션 기록을 읽은 횟수 — 판정마다 한 번
    func load() -> [SessionRecord] { loads += 1; return items }
    func remove(_ record: SessionRecord) { removed.append(record.name) }
    func isPaused(_ sid: String) -> Bool { paused.contains(sid) }
    func isMuted(_ sid: String) -> Bool { muted.contains(sid) }
    func allMuted() -> Bool { muteAll }
}

private final class FakeProcesses: ProcessProbe, @unchecked Sendable {
    var gone = Set<Int32>()                           // 확실히 없는 pid(ESRCH)
    var denied = Set<Int32>()                         // 권한이 없어 못 보는 pid(EPERM) — 진짜 어댑터처럼 없다고 하지 않는다
    var shells: [Int32: Int] = [:]
    private(set) var goneAsked: [Int32] = [], shellsAsked: [Int32] = []
    func isGone(_ pid: Int32) -> Bool { goneAsked.append(pid); return gone.contains(pid) && !denied.contains(pid) }
    func backgroundShells(_ pid: Int32) -> Int { shellsAsked.append(pid); return shells[pid] ?? 0 }
}

private final class FakeLive: ClaudeLive, @unchecked Sendable {
    var status: [Int32: (session: String, status: String, at: Date)] = [:]
    var names: [Int32: String] = [:]
    var sessionPids: [String: Int32] = [:]            // ~/.claude/sessions/<pid>.json 의 sessionId → pid
    private(set) var pruned: [Set<Int32>] = []
    private(set) var pidAsked: [String] = []          // sessionId 로 pid 를 찾은 세션
    func statusChange(_ pid: Int32, session: String) -> (status: String, at: Date)? {
        guard let s = status[pid], s.session == session else { return nil }   // /clear 뒤 같은 pid 의 다른 세션은 모른다
        return (s.status, s.at)
    }
    func name(_ pid: Int32) -> String? { names[pid] }
    func pid(forSession session: String) -> Int32? { pidAsked.append(session); return sessionPids[session] }
    func prune(keep: Set<Int32>) { pruned.append(keep) }
    func parkedJob(_ pid: Int32, session: String) -> String? { nil }   // 세션 상태 판정은 부르지 않는다(보드 탭 짝짓기 몫)
}

private final class FakeTranscripts: Transcripts, @unchecked Sendable {
    var existing = Set<String>()
    var interrupts: [String: Date] = [:]
    var handoffs: [String: (id: String, at: Date)] = [:]
    var brains: [String: Brain] = [:]
    private(set) var calls: [String] = []            // "exists /t/a.jsonl" 처럼 부른 차례
    private(set) var pruned: [(keep: Set<String>, keepHandoffs: Set<String>)] = []
    func locate(given: String?, cwd: String, session: String) -> String { given ?? "/t/\(session).jsonl" }
    func exists(_ path: String) -> Bool { calls.append("exists \(path)"); return existing.contains(path) }
    func interruptedAt(_ path: String) -> Date? { calls.append("interrupt \(path)"); return interrupts[path] }
    func handoff(_ path: String) -> (id: String, at: Date)? { calls.append("handoff \(path)"); return handoffs[path] }
    func brain(_ path: String, fallbackModel: String, now: Date) -> Brain? {
        calls.append("brain \(path)")
        return brains[path] ?? (fallbackModel.isEmpty ? nil : Brain(model: fallbackModel, effort: ""))
    }
    func prune(keep: Set<String>, keepHandoffs: Set<String>) { pruned.append((keep, keepHandoffs)) }
    func called(_ kind: String) -> [String] { calls.filter { $0.hasPrefix(kind + " ") }.map { String($0.dropFirst(kind.count + 1)) } }
}

private final class FakeContexts: ContextUsage, @unchecked Sendable {
    var ctx: [String: Ctx] = [:]
    var efforts: [String: String] = [:]
    private(set) var effortAsked: [String] = []
    private(set) var pruned: [(keep: Set<String>, keepTranscripts: Set<String>)] = []
    func read(_ sid: String, transcript: String, model: String, now: Date) -> Ctx? { ctx[sid] }
    func liveEffort(_ sid: String, transcript: String) -> String? { effortAsked.append(sid); return efforts[sid] }
    func prune(keep: Set<String>, keepTranscripts: Set<String>) { pruned.append((keep, keepTranscripts)) }
}

private struct FakeClock: Clock { let now: Date }

// 가짜 어댑터 한 벌과 세션 기록 만들기
private final class World {
    let records = FakeRecords(), processes = FakeProcesses(), live = FakeLive()
    let transcripts = FakeTranscripts(), contexts = FakeContexts()
    var now = t0

    // 세션 기록 하나 — 시각은 t0 에서 몇 초 전·뒤(초)로 적고, 진짜처럼 JSON 으로 한 번 풀어 숫자가 NSNumber 가 되게 한다.
    // 기본: 대기, 1분 전에 갱신, 대화 기록 파일 있음
    @discardableResult
    func add(_ sid: String, state: String = "idle", pid: Int32? = nil, since: Double = -60, updated: Double = -60,
             transcript: Bool = true, extra: [String: Any] = [:], withID: Bool = true) -> SessionRecord {
        var j: [String: Any] = ["state": state, "cwd": "/p", "since": (t0 + since).timeIntervalSince1970,
                                "updated": (t0 + updated).timeIntervalSince1970]
        if withID { j["session_id"] = sid }
        if let pid { j["pid"] = Int(pid) }
        for (k, v) in extra { j[k] = v }
        let data = try! JSONSerialization.data(withJSONObject: j)
        let r = SessionRecord(name: "\(sid).json", file: "/s/\(sid).json",
                              json: try! JSONSerialization.jsonObject(with: data) as! [String: Any])
        records.items.append(r)
        if transcript { transcripts.existing.insert("/t/\(sid).jsonl") }
        return r
    }

    func scan() -> SessionScan {
        scanSessions(records: records, processes: processes, live: live, transcripts: transcripts,
                     contexts: contexts, clock: FakeClock(now: now))
    }
}

private func at(_ offset: Double) -> Date { t0 + offset }
private func epoch(_ offset: Double) -> Double { (t0 + offset).timeIntervalSince1970 }

// 표의 줄 — 시험 결과에 note 가 보인다
protocol Row: Sendable, CustomTestStringConvertible { var note: String { get } }
extension Row { var testDescription: String { note } }

// MARK: - 낡은 기록 지우기

struct RemoveRow: Row {
    let note: String
    let state: String
    let updatedAgo: Double
    let transcript: Bool
    var pid: Pid = .absent
    let removed: Bool
    let existsAsked: Bool          // 대화 기록이 있는지 물었나 — 끝난 지 이틀 안 된 끝난 세션만 묻는다
}
enum Pid: Sendable { case absent, alive, gone, denied }

private let removeRows: [RemoveRow] = [
    .init(note: "끝남 + 이틀 넘음 → 지움(대화 기록은 묻지 않는다)", state: "ended", updatedAgo: 3 * day, transcript: true, removed: true, existsAsked: false),
    .init(note: "끝남 + 대화 기록 없음 → 지움", state: "ended", updatedAgo: 3600, transcript: false, removed: true, existsAsked: true),
    .init(note: "끝남 + 이틀 안 + 대화 기록 있음 → 남김", state: "ended", updatedAgo: 3600, transcript: true, removed: false, existsAsked: true),
    .init(note: "끝남 + 꼭 이틀 → 남김(넘어야 지운다)", state: "ended", updatedAgo: 2 * day, transcript: true, removed: false, existsAsked: true),
    .init(note: "끝남 + 이틀 하고 1초 → 지움", state: "ended", updatedAgo: 2 * day + 1, transcript: true, removed: true, existsAsked: false),
    .init(note: "작업 중 + 대화 기록 없음 + 오래됨 → 남김(끝나지 않은 세션은 지우지 않는다)", state: "working", updatedAgo: 3 * day, transcript: false, removed: false, existsAsked: false),
    .init(note: "대기 + 대화 기록 없음(첫 요청 전) → 남김", state: "idle", updatedAgo: 3 * day, transcript: false, removed: false, existsAsked: false),
    .init(note: "승인 대기 + 대화 기록 없음 → 남김", state: "waiting", updatedAgo: 0, transcript: false, removed: false, existsAsked: false),
    .init(note: "작업 중이지만 pid 가 확실히 없음 + 대화 기록 없음 → 끝난 것으로 보고 지움", state: "working", updatedAgo: 3600, transcript: false, pid: .gone, removed: true, existsAsked: true),
    .init(note: "대기지만 pid 가 확실히 없음 + 이틀 넘음 → 지움", state: "idle", updatedAgo: 3 * day, transcript: true, pid: .gone, removed: true, existsAsked: false),
    .init(note: "pid 를 볼 권한 없음 + 대화 기록 없음 → 남김(살아 있다고 본다)", state: "idle", updatedAgo: 3 * day, transcript: false, pid: .denied, removed: false, existsAsked: false),
]

@Suite("세션 상태 판정: 낡은 기록 지우기") struct ScanRemoveTests {
    @Test("끝난 세션이면서 (이틀 넘음 또는 대화 기록 없음)일 때만 지운다", arguments: removeRows)
    func 표(row: RemoveRow) {
        let w = World()
        let pid: Int32 = 42
        switch row.pid {
        case .absent, .alive: break
        case .gone: w.processes.gone.insert(pid)
        case .denied: w.processes.gone.insert(pid); w.processes.denied.insert(pid)
        }
        w.add("a", state: row.state, pid: row.pid == .absent ? nil : pid, updated: -row.updatedAgo, transcript: row.transcript)
        let scan = w.scan()
        #expect(w.records.removed == (row.removed ? ["a.json"] : []))
        #expect(scan.sessions.map(\.id) == (row.removed ? [] : ["a"]))
        #expect(w.transcripts.called("exists") == (row.existsAsked ? ["/t/a.jsonl"] : []))
    }
}

// MARK: - pid

struct PidRow: Row {
    let note: String
    let state: String
    let pid: Pid
    let expected: String
    let asked: Bool                // pid 를 물었나
}

private let pidRows: [PidRow] = [
    .init(note: "작업 중 + pid 확실히 없음 → 끝남", state: "working", pid: .gone, expected: "ended", asked: true),
    .init(note: "승인 대기 + pid 확실히 없음 → 끝남", state: "waiting", pid: .gone, expected: "ended", asked: true),
    .init(note: "대기 + pid 확실히 없음 → 끝남", state: "idle", pid: .gone, expected: "ended", asked: true),
    .init(note: "작업 중 + pid 권한 없음 → 그대로", state: "working", pid: .denied, expected: "working", asked: true),
    .init(note: "대기 + pid 살아 있음 → 그대로", state: "idle", pid: .alive, expected: "idle", asked: true),
    .init(note: "pid 가 기록에 없음 → 묻지 않고 그대로", state: "working", pid: .absent, expected: "working", asked: false),
    .init(note: "이미 끝남 → 묻지 않는다", state: "ended", pid: .gone, expected: "ended", asked: false),
]

@Suite("세션 상태 판정: pid") struct ScanPidTests {
    @Test("pid 가 확실히 없을 때만 끝난 것으로", arguments: pidRows)
    func 표(row: PidRow) throws {
        let w = World()
        let pid: Int32 = 42
        if row.pid == .gone || row.pid == .denied { w.processes.gone.insert(pid) }
        if row.pid == .denied { w.processes.denied.insert(pid) }
        w.add("a", state: row.state, pid: row.pid == .absent ? nil : pid)
        let s = try #require(w.scan().sessions.first)
        #expect(s.state == row.expected)
        #expect(w.processes.goneAsked == (row.asked ? [pid] : []))
    }

    @Test("pid 가 없어 끝난 세션은 이름·서브에이전트·모델을 보지 않는다") func 끝난_세션() throws {
        let w = World()
        w.processes.gone.insert(7)
        w.live.names[7] = "이름"
        w.add("a", state: "working", pid: 7,
              extra: ["model": "claude-opus-5-5", "subagents": ["x": ["started_seen": true, "state": "running", "started": epoch(-5)]]])
        let s = try #require(w.scan().sessions.first)
        #expect((s.state, s.name, s.subagents.count, s.brain) == ("ended", nil, 0, nil))
        #expect(w.transcripts.called("brain").isEmpty)
    }
}

// MARK: - pid 없는 세션

// state.py 가 claude 를 못 찾으면 세션 기록의 pid 가 null 이다. 그런 세션은 Claude Code 의 ~/.claude/sessions/<pid>.json 가운데
// sessionId 가 같은 것의 pid 로 생사를 본다. 못 찾으면 끝났다고 단정하지 않는다(claude -p 처럼 그 파일을 쓰지 않는 세션)
enum Found: Sendable { case none, alive, gone, denied }

struct NoPidRow: Row {
    let note: String
    let state: String
    var recordPid = false          // 세션 기록에 pid 가 있나(있으면 그 pid 7 이 살아 있다)
    let found: Found               // Claude Code 파일에서 찾은 pid 21 의 처지
    let expected: String
    let looked: Bool               // sessionId 로 pid 를 찾았나
    let goneAsked: [Int32]
}

private let noPidRows: [NoPidRow] = [
    .init(note: "작업 중 + pid 없음 + 찾은 pid 확실히 없음 → 끝남", state: "working", found: .gone, expected: "ended", looked: true, goneAsked: [21]),
    .init(note: "승인 대기 + pid 없음 + 찾은 pid 확실히 없음 → 끝남", state: "waiting", found: .gone, expected: "ended", looked: true, goneAsked: [21]),
    .init(note: "대기 + pid 없음 + 찾은 pid 확실히 없음 → 끝남", state: "idle", found: .gone, expected: "ended", looked: true, goneAsked: [21]),
    .init(note: "작업 중 + pid 없음 + 찾은 pid 살아 있음 → 그대로", state: "working", found: .alive, expected: "working", looked: true, goneAsked: [21]),
    .init(note: "작업 중 + pid 없음 + 찾은 pid 를 볼 권한 없음 → 그대로", state: "working", found: .denied, expected: "working", looked: true, goneAsked: [21]),
    .init(note: "작업 중 + pid 없음 + Claude Code 파일 없음 → 그대로(끝났다고 단정하지 않는다)", state: "working", found: .none, expected: "working", looked: true, goneAsked: []),
    .init(note: "pid 없음 + 이미 끝남 → 찾지 않는다", state: "ended", found: .gone, expected: "ended", looked: false, goneAsked: []),
    .init(note: "기록에 pid 있음 → 찾지 않고 기록의 pid 로", state: "working", recordPid: true, found: .gone, expected: "working", looked: false, goneAsked: [7]),
]

@Suite("세션 상태 판정: pid 없는 세션") struct ScanNoPidTests {
    @Test("기록에 pid 가 없으면 Claude Code 세션 파일의 sessionId 로 pid 를 찾아 생사를 본다", arguments: noPidRows)
    func 표(row: NoPidRow) throws {
        let w = World()
        let found: Int32 = 21
        if row.found != .none { w.live.sessionPids["a"] = found }
        if row.found == .gone || row.found == .denied { w.processes.gone.insert(found) }
        if row.found == .denied { w.processes.denied.insert(found) }
        w.add("a", state: row.state, pid: row.recordPid ? 7 : nil)
        let s = try #require(w.scan().sessions.first)
        #expect(s.state == row.expected)
        #expect(w.live.pidAsked == (row.looked ? ["a"] : []))
        #expect(w.processes.goneAsked == row.goneAsked)
    }

    @Test("찾은 pid 가 확실히 없고 대화 기록도 없으면 끝난 세션처럼 지운다") func 지우기() {
        let w = World()
        w.live.sessionPids["a"] = 21
        w.processes.gone.insert(21)
        w.add("a", state: "working", transcript: false)
        #expect(w.scan().sessions.isEmpty)
        #expect(w.records.removed == ["a.json"])
    }

    @Test("찾은 pid 로 Claude Code status·세션 이름을 보고 캐시에도 남긴다(Session.pid 는 기록 그대로 0)") func status_이름() throws {
        let w = World()
        w.live.sessionPids["a"] = 21
        w.live.status[21] = ("a", "busy", at(-70))
        w.live.names[21] = "보드-2b"
        w.add("a", state: "waiting", since: -100)
        let s = try #require(w.scan().sessions.first)
        #expect((s.state, s.since, s.name) == ("working", at(-70), "보드-2b"))   // 승인 창이 닫혀 busy → 작업 중
        #expect(s.pid == 0)
        #expect(w.live.pruned == [[21]])
    }

    @Test("찾은 pid 의 백그라운드 셸도 센다") func 셸() throws {
        let w = World()
        w.live.sessionPids["a"] = 21
        w.processes.shells[21] = 2
        w.add("a", state: "idle")
        let s = try #require(w.scan().sessions.first)
        #expect((s.state, s.shells, s.pid) == ("working", 2, 0))
    }

    @Test("이미 끝난 pid 없는 세션은 찾지 않으므로 캐시 목록의 pid 는 0") func 끝난_세션() {
        let w = World()
        w.live.sessionPids["a"] = 21
        w.add("a", state: "ended")
        _ = w.scan()
        #expect(w.live.pidAsked.isEmpty)
        #expect(w.live.pruned == [[0]])
    }
}

// MARK: - Esc 중단·Claude Code status

struct StatusRow: Row {
    let note: String
    let state: String
    var interrupt: Double? = nil                  // 대화 기록 끝의 Esc 중단 시각(t0 기준 초)
    var live: (status: String, at: Double)? = nil // Claude Code status 와 바뀐 시각(since 는 늘 -100)
    var otherSession = false                      // status 파일이 다른 세션 것(/clear 뒤)
    let expected: String
    let since: Double
}

private let statusRows: [StatusRow] = [
    .init(note: "작업 중 + Esc 중단 → 대기, since 는 중단 시각", state: "working", interrupt: -50, expected: "idle", since: -50),
    .init(note: "승인 대기 + Esc 중단 → 대기", state: "waiting", interrupt: -50, expected: "idle", since: -50),
    .init(note: "대기 + Esc 중단 → 그대로(묻지 않는다)", state: "idle", interrupt: -50, expected: "idle", since: -100),
    .init(note: "작업 중 + 이번 상태 뒤에 status idle → 대기", state: "working", live: ("idle", -80), expected: "idle", since: -80),
    .init(note: "승인 대기 + 이번 상태 뒤에 status idle → 대기", state: "waiting", live: ("idle", -80), expected: "idle", since: -80),
    .init(note: "작업 중 + status idle 이 since 와 같은 시각 → 그대로(뒤여야 한다)", state: "working", live: ("idle", -100), expected: "working", since: -100),
    .init(note: "작업 중 + status idle 이 이번 상태 앞 → 그대로", state: "working", live: ("idle", -150), expected: "working", since: -100),
    .init(note: "Esc 중단이 status 보다 먼저", state: "working", interrupt: -50, live: ("idle", -80), expected: "idle", since: -50),
    .init(note: "승인 대기 + status busy → 작업 중, since 는 busy 시각", state: "waiting", live: ("busy", -70), expected: "working", since: -70),
    .init(note: "작업 중 + status busy → 그대로", state: "working", live: ("busy", -70), expected: "working", since: -100),
    .init(note: "승인 대기 + status busy 가 이번 상태 앞 → 그대로", state: "waiting", live: ("busy", -150), expected: "waiting", since: -100),
    .init(note: "승인 대기 + status waiting → 그대로", state: "waiting", live: ("waiting", -70), expected: "waiting", since: -100),
    .init(note: "대기 + status busy → 그대로", state: "idle", live: ("busy", -70), expected: "idle", since: -100),
    .init(note: "status 파일이 다른 세션 것(/clear 뒤) → 보지 않는다", state: "waiting", live: ("busy", -70), otherSession: true, expected: "waiting", since: -100),
]

@Suite("세션 상태 판정: Esc 중단과 Claude Code status") struct ScanStatusTests {
    @Test("작업 중·승인 대기를 대기·작업 중으로 바로잡기", arguments: statusRows)
    func 표(row: StatusRow) throws {
        let w = World()
        w.add("a", state: row.state, pid: 9, since: -100)
        if let i = row.interrupt { w.transcripts.interrupts["/t/a.jsonl"] = at(i) }
        if let l = row.live { w.live.status[9] = (row.otherSession ? "b" : "a", l.status, at(l.at)) }
        let s = try #require(w.scan().sessions.first)
        #expect(s.state == row.expected)
        #expect(s.since == at(row.since))
        // 대화 기록 끝의 중단 표시는 작업 중·승인 대기일 때만 읽는다
        #expect(w.transcripts.called("interrupt") == (["working", "waiting"].contains(row.state) ? ["/t/a.jsonl"] : []))
    }
}

// MARK: - 서브에이전트·백그라운드 셸

enum SubKind: Sendable { case absent, running, waiting, doneRecently, noStart }

struct BusyRow: Row {
    let note: String
    let state: String
    var sub: SubKind = .absent
    var shells = 0
    var interrupted = false
    let expected: String
    let shellsShown: Int
}

// 서브에이전트 기록 하나("x")
private func subRecord(_ kind: SubKind) -> [String: Any] {
    switch kind {
    case .absent: return [:]
    case .running: return ["x": ["started_seen": true, "state": "running", "started": epoch(-5)]]
    case .waiting: return ["x": ["started_seen": true, "state": "waiting", "started": epoch(-5)]]
    case .doneRecently: return ["x": ["started_seen": true, "state": "done", "started": epoch(-9), "ended": epoch(-3)]]
    case .noStart: return ["x": ["state": "running", "started": epoch(-5)]]
    }
}

private let busyRows: [BusyRow] = [
    .init(note: "대기 + 도는 서브에이전트 → 작업 중", state: "idle",
          sub: .running, expected: "working", shellsShown: 0),
    .init(note: "대기 + 승인 기다리는 서브에이전트 → 작업 중", state: "idle",
          sub: .waiting, expected: "working", shellsShown: 0),
    .init(note: "대기 + 끝난 지 3초 된 서브에이전트 → 대기", state: "idle",
          sub: .doneRecently, expected: "idle", shellsShown: 0),
    .init(note: "대기 + 시작 기록 없는 서브에이전트 → 대기", state: "idle",
          sub: .noStart, expected: "idle", shellsShown: 0),
    .init(note: "대기 + 백그라운드 셸 2 → 작업 중, 셸 2", state: "idle", shells: 2, expected: "working", shellsShown: 2),
    .init(note: "작업 중의 셸은 세지 않는다", state: "working", shells: 2, expected: "working", shellsShown: 0),
    .init(note: "승인 대기의 셸도 세지 않는다", state: "waiting", shells: 2, expected: "waiting", shellsShown: 0),
    .init(note: "Esc 로 끊겨 대기가 된 뒤에도 셸이 돌면 작업 중", state: "working", shells: 1, interrupted: true, expected: "working", shellsShown: 1),
    .init(note: "끝난 세션의 셸은 세지 않는다", state: "ended", shells: 1, expected: "ended", shellsShown: 0),
]

@Suite("세션 상태 판정: 서브에이전트와 백그라운드 셸") struct ScanBusyTests {
    @Test("응답은 끝났어도 서브에이전트·셸이 돌면 작업 중", arguments: busyRows)
    func 표(row: BusyRow) throws {
        let w = World()
        w.processes.shells[5] = row.shells
        if row.interrupted { w.transcripts.interrupts["/t/a.jsonl"] = at(-10) }
        w.add("a", state: row.state, pid: 5, extra: row.sub == .absent ? [:] : ["subagents": subRecord(row.sub)])
        let s = try #require(w.scan().sessions.first)
        #expect(s.state == row.expected)
        #expect(s.shells == row.shellsShown)
    }

    @Test("서브에이전트 기록 해석이 그대로 — 시작 기록 없는 것·끝난 지 10초 지난 것을 빼고, 모델은 서브에이전트 대화 기록에서")
    func 서브에이전트() throws {
        let w = World()
        w.transcripts.brains["/t/a/subagents/agent-run.jsonl"] = Brain(model: "claude-haiku-4-5", effort: "low")
        w.add("a", state: "idle", extra: ["subagents": [
            "run": ["started_seen": true, "state": "running", "started": epoch(-30), "type": "Explore"],
            "no-start": ["state": "running", "started": epoch(-20)],
            "old-done": ["started_seen": true, "state": "done", "started": epoch(-60), "ended": epoch(-11)],
            "new-done": ["started_seen": true, "state": "done", "started": epoch(-40), "ended": epoch(-10)],
        ]])
        let s = try #require(w.scan().sessions.first)
        #expect(s.subagents.map(\.id) == ["run", "new-done"])
        #expect(s.subagents.first?.brain == Brain(model: "claude-haiku-4-5", effort: "low"))
        #expect(s.subagents.last?.brain == nil)
        #expect(Set(w.transcripts.called("brain")).isSuperset(of: ["/t/a/subagents/agent-run.jsonl", "/t/a/subagents/agent-new-done.jsonl"]))
    }

    @Test("서브에이전트 대화 기록 자리", arguments: [
        ("/h/.claude/projects/-p/s1.jsonl", "x", "/h/.claude/projects/-p/s1/subagents/agent-x.jsonl"),
        ("/t/s1", "x", "/t/s1/subagents/agent-x.jsonl"),
        ("/t/s1.json", "y", "/t/s1.json/subagents/agent-y.jsonl"),
    ])
    func 자리(transcript: String, agent: String, expected: String) {
        #expect(subagentTranscript(transcript, agent: agent) == expected)
    }
}

// MARK: - 일시중지

struct PausedRow: Row {
    let note: String
    let state: String
    var gone = false
    var running = false
    let expected: String
}

private let pausedRows: [PausedRow] = [
    .init(note: "대기 + 표지 → 일시중지", state: "idle", expected: "paused"),
    .init(note: "작업 중 + 표지 → 일시중지", state: "working", expected: "paused"),
    .init(note: "승인 대기 + 표지 → 일시중지", state: "waiting", expected: "paused"),
    .init(note: "서브에이전트가 돌아도 표지가 있으면 일시중지", state: "idle", running: true, expected: "paused"),
    .init(note: "끝남 + 표지 → 끝남(일시중지가 되지 않는다)", state: "ended", expected: "ended"),
    .init(note: "pid 가 없어 끝남 + 표지 → 끝남", state: "working", gone: true, expected: "ended"),
]

@Suite("세션 상태 판정: 일시중지") struct ScanPausedTests {
    @Test("일시중지 표지는 끝나지 않은 세션만 일시중지로", arguments: pausedRows)
    func 표(row: PausedRow) throws {
        let w = World()
        if row.gone { w.processes.gone.insert(3) }
        w.records.paused.insert("a")
        w.add("a", state: row.state, pid: 3, extra: row.running ? ["subagents": subRecord(.running)] : [:])
        let s = try #require(w.scan().sessions.first)
        #expect(s.state == row.expected)
        #expect(s.paused)
    }

    @Test("표지가 없으면 그대로") func 표지_없음() throws {
        let w = World()
        w.add("a", state: "idle")
        let s = try #require(w.scan().sessions.first)
        #expect((s.state, s.paused) == ("idle", false))
    }
}

// MARK: - 대화를 넘긴 세션

struct HandoffRow: Row {
    let note: String
    var state = "waiting"
    let started: Double?           // 세션 기록의 started(t0 기준 초), nil 이면 없음
    let handedAt: Double
    var to = "b"
    let handed: Bool
}

private let handoffRows: [HandoffRow] = [
    .init(note: "넘기기 전에 시작한 세션 → 넘김", started: -100, handedAt: -50, handed: true),
    .init(note: "넘긴 시각과 같은 시각에 시작 → 넘김(started <= 넘긴 시각)", started: -50, handedAt: -50, handed: true),
    .init(note: "넘긴 뒤 다시 열린 세션 → 넘김이 아니다", started: -10, handedAt: -50, handed: false),
    .init(note: "started 가 없음 → 넘김", started: nil, handedAt: -50, handed: true),
    .init(note: "자기 자신으로 넘김 → 무시", started: -100, handedAt: -50, to: "a", handed: false),
    .init(note: "끝난 세션도 넘김", state: "ended", started: -100, handedAt: -50, handed: true),
]

@Suite("세션 상태 판정: 대화를 넘긴 세션") struct ScanHandoffTests {
    @Test("넘긴 세션 → 이어 받은 세션", arguments: handoffRows)
    func 표(row: HandoffRow) {
        let w = World()
        w.transcripts.handoffs["/t/a.jsonl"] = (row.to, at(row.handedAt))
        w.add("a", state: row.state, extra: row.started.map { ["started": epoch($0)] } ?? [:])
        #expect(w.scan().handedTo == (row.handed ? ["a": row.to] : [:]))
    }
}

// MARK: - 지운 기록·캐시 정리·포트를 부르는 조건

@Suite("세션 상태 판정: 지운 기록과 캐시 정리") struct ScanPruneTests {
    @Test("지운 기록, 캐시에 남길 목록, 대화 기록을 읽는 조건") func 정리() throws {
        let w = World()
        w.add("live", state: "working", pid: 11,
              extra: ["subagents": ["x": ["started_seen": true, "state": "running", "started": epoch(-5)]]])
        w.add("recent", state: "ended", pid: 12, updated: -3600)
        w.add("old", state: "ended", pid: 13, updated: -3 * day)
        w.add("blank", state: "ended", pid: 14, transcript: false)
        w.add("idle", state: "idle", pid: 15)
        let scan = w.scan()

        #expect(scan.sessions.map(\.id) == ["live", "recent", "idle"])   // 세션 기록 차례 그대로
        #expect(w.records.loads == 1)                                    // 세션 기록은 판정마다 한 번만 읽는다
        #expect(w.records.removed == ["old.json", "blank.json"])
        #expect(w.live.pruned == [[11, 12, 15]])
        let pruned = try #require(w.transcripts.pruned.first)
        #expect(w.transcripts.pruned.count == 1)
        // 모델·중단 캐시: 끝나지 않은 세션과 그 서브에이전트의 대화 기록
        #expect(pruned.keep == ["/t/live.jsonl", "/t/live/subagents/agent-x.jsonl", "/t/idle.jsonl"])
        // 넘김 캐시: 목록에 남은 모든 세션의 대화 기록(끝난 것도)
        #expect(pruned.keepHandoffs == ["/t/live.jsonl", "/t/recent.jsonl", "/t/idle.jsonl"])

        // 1초마다 도는 길이라 부르는 조건이 곧 CPU 다 — 대화 기록 stat 은 끝난 지 이틀 안 된 끝난 세션만,
        // 중단 표시는 작업 중·승인 대기만, 모델은 끝나지 않은 세션만, 넘김은 남은 세션 모두
        #expect(w.transcripts.called("exists") == ["/t/recent.jsonl", "/t/blank.jsonl"])
        #expect(w.transcripts.called("interrupt") == ["/t/live.jsonl"])
        #expect(w.transcripts.called("brain") == ["/t/live/subagents/agent-x.jsonl", "/t/live.jsonl", "/t/idle.jsonl"])
        #expect(w.transcripts.called("handoff") == ["/t/live.jsonl", "/t/recent.jsonl", "/t/idle.jsonl"])
        #expect(w.contexts.effortAsked == [])                         // 모델을 모르면 effort 도 묻지 않는다
        #expect(w.processes.goneAsked == [11, 15])                   // 끝난 세션은 pid 를 묻지 않는다
        #expect(w.processes.shellsAsked == [15])                     // 셸 수는 대기 세션만
        #expect(w.live.pidAsked == [])                               // 기록에 pid 가 있으면 Claude Code 파일에서 찾지 않는다
        // 컨텍스트 캐시: 목록에 남은 모든 세션(끝난 것도 — 컨텍스트는 끝난 세션 카드에도 보인다). 지운 기록은 뺀다
        let ctx = try #require(w.contexts.pruned.first)
        #expect(w.contexts.pruned.count == 1)
        #expect(ctx.keep == ["live", "recent", "idle"])
        #expect(ctx.keepTranscripts == ["/t/live.jsonl", "/t/recent.jsonl", "/t/idle.jsonl"])
    }

    @Test("사라진 세션은 다음 판정의 컨텍스트 캐시 목록에서 빠진다") func 컨텍스트_다음_판정() {
        let w = World()
        w.add("a", state: "working", pid: 1)
        w.add("b", state: "idle", pid: 2)
        _ = w.scan()
        w.records.items.removeAll { $0.name == "b.json" }   // state.py prune 이나 보드가 기록을 지웠다
        _ = w.scan()
        #expect(w.contexts.pruned.map(\.keep) == [["a", "b"], ["a"]])
        #expect(w.contexts.pruned.map(\.keepTranscripts) == [["/t/a.jsonl", "/t/b.jsonl"], ["/t/a.jsonl"]])
    }

    @Test("시계는 한 번 읽은 것을 돌려준다") func 시계() {
        let w = World()
        w.now = at(123)
        #expect(w.scan().now == at(123))
    }
}

// MARK: - 세션 기록 → 세션 값

@Suite("세션 상태 판정: 세션 값") struct ScanFieldsTests {
    @Test("기록의 필드를 세션 값으로") func 필드() throws {
        let w = World()
        w.records.muted.insert("a")
        w.live.names[8] = "보드-1a"
        w.contexts.ctx["a"] = Ctx(used: 50, size: 200)
        w.contexts.efforts["a"] = "max"
        w.add("a", state: "idle", pid: 8, extra: [
            "cwd": "/p/sub", "project_dir": "/p", "headless": true, "tty": "/dev/ttys003",
            "topic": "## **보드** `포트`  ", "last_message": "*끝*", "resume_cmd": "claude --resume a",
            "model": "claude-opus-5-5", "limit_hit": ["at": epoch(-30)],
        ])
        let s = try #require(w.scan().sessions.first)
        #expect((s.cwd, s.projectDir, s.headless, s.tty) == ("/p/sub", "/p", true, "/dev/ttys003"))
        #expect((s.topic, s.message, s.resumeCmd) == ("보드 포트", "끝", "claude --resume a"))
        #expect((s.file, s.pid, s.muted, s.name) == ("/s/a.json", 8, true, "보드-1a"))
        #expect(s.updated == at(-60))
        #expect(s.limitAt == at(-30))
        #expect(s.ctx == Ctx(used: 50, size: 200))
        // 대화 기록에 모델이 없으면 기록의 model, effort 는 상태줄의 지금 effort
        #expect(s.brain == Brain(model: "claude-opus-5-5", effort: "max"))
    }

    @Test("값이 빠지면 기본값 — 상태는 대기, project_dir 은 cwd, cwd 는 ?") func 기본값() throws {
        let w = World()
        w.add("a", extra: ["state": NSNull(), "cwd": NSNull()])
        w.add("b")
        let scan = w.scan()
        let a = try #require(scan.sessions.first), b = try #require(scan.sessions.last)
        #expect((a.state, a.cwd, a.projectDir) == ("idle", "?", "?"))
        #expect((b.cwd, b.projectDir) == ("/p", "/p"))
        #expect((a.headless, a.tty, a.topic, a.message, a.resumeCmd) == (false, "", "", "", ""))
        #expect((a.pid, a.name, a.brain, a.ctx, a.limitAt) == (0, nil, nil, nil, nil))
    }

    @Test("session_id 없는 기록은 파일 이름에서 세션 id 를 — 표지·대화 기록도 그 id 로") func 세션_id() throws {
        let w = World()
        w.records.paused.insert("noid")
        w.add("noid", state: "idle", withID: false)
        let s = try #require(w.scan().sessions.first)
        #expect((s.id, s.state) == ("noid", "paused"))
        #expect(w.transcripts.called("handoff") == ["/t/noid.jsonl"])
    }
}

// MARK: - 공유 규칙 표(rules-table/session-scan.json·sessions-group.json) — RulesTableTests 가 이 파일의 가짜(World)로 돌린다.
// World·FakeProcesses·FakeLive 는 다른 시험 파일의 private 이름과 겹쳐 internal 로 넓히면 다시 선언 오류가 나므로, 넘겨 주는 함수만 둔다.
// 기록(record)은 세션 파일 JSON 그대로이고 since·updated 만 표의 꼴(지금 = 0 기준 초)에서 1970 기준 초(t0 + 값)로 바꾼다

/// 표의 세션 기록 하나를 World 에 넣는다 — 진짜처럼 JSON 으로 한 번 풀어 넣고, 대화 기록이 있으면 가짜 locate 가 주는 자리를 있는 것으로
private func addTableRecord(_ w: World, file: String, record: [String: Any], transcript: Bool) {
    var j = record
    for k in ["since", "updated"] { if let v = j[k] as? NSNumber { j[k] = epoch(v.doubleValue) } }
    let json = try! JSONSerialization.jsonObject(with: JSONSerialization.data(withJSONObject: j)) as! [String: Any]
    w.records.items.append(SessionRecord(name: file, file: "/s/\(file)", json: json))
    if transcript {
        w.transcripts.existing.insert(w.transcripts.locate(given: json["transcript_path"] as? String, cwd: json["cwd"] as? String ?? "?",
                                                           session: sessionID(json, fileName: file)))
    }
}

/// scan 한 줄 — 세션 기록 하나를 판정해 목록에 남았는지와 그 상태.
/// procs 는 pid → .alive·.gone(ESRCH)·.denied(EPERM — 위 표의 Pid.denied 처럼 gone·denied 둘 다 넣어 isGone 이 false), 표에 없는 pid 는 gone.
/// claudeSessions 는 ~/.claude/sessions/<pid>.json(그 파일의 sessionId, mtime 이 지금보다 age 초 전). 같은 sessionId 가 여럿이면 가장 최근에
/// 고쳐 쓴 것(age 가 작은 것, 같으면 앞의 것) — 이 고르기는 BoardRules 가 아니라 앱의 진짜 어댑터 ClaudeNames.pid(forSession:)(BoardData.swift)
/// 몫이라 가짜가 그대로 흉내 낸다(포트 ClaudeLive.pid(forSession:) 는 pid 하나만 준다)
func runScanRow(file: String, record: [String: Any], transcript: Bool, claudeSessions: [(pid: Int32, sessionId: String, age: Double)],
                procs: [Int32: Pid], paused: Bool) -> (kept: Bool, state: String?) {
    let w = World()
    addTableRecord(w, file: file, record: record, transcript: transcript)
    var newest: [String: Double] = [:]
    for c in claudeSessions where !c.sessionId.isEmpty {
        if let age = newest[c.sessionId], age <= c.age { continue }
        newest[c.sessionId] = c.age
        w.live.sessionPids[c.sessionId] = c.pid
    }
    var pids = Set(claudeSessions.map(\.pid)).union(procs.keys)
    if let p = (record["pid"] as? NSNumber)?.int32Value { pids.insert(p) }
    for pid in pids {
        switch procs[pid] ?? .gone {
        case .gone: w.processes.gone.insert(pid)
        case .denied: w.processes.gone.insert(pid); w.processes.denied.insert(pid)
        case .alive, .absent: break
        }
    }
    if paused { w.records.paused.insert(sessionID(w.records.items[0].json, fileName: file)) }
    let s = w.scan().sessions.first { $0.file == "/s/\(file)" }
    return (s != nil, s?.state)
}

/// group 한 판 — 앱(Store.reload)과 같은 차례로 scanSessions → visibleSessions(넘긴 세션·탭이 따라가는 세션 없음) → groupSessions → sortGroups.
/// 앱의 지운 카드 숨기기(hiddenGroups)·조용한 카드 접기(foldQuiet)는 앱 상태라 뺀다. 대화 기록은 모두 있다
func runGroupRows(records: [(file: String, record: [String: Any])], pinned: [String]) -> [(id: String, sessions: [String])] {
    let w = World()
    for r in records { addTableRecord(w, file: r.file, record: r.record, transcript: true) }
    let scan = w.scan()
    let groups = sortGroups(groupSessions(visibleSessions(scan.sessions, handedTo: scan.handedTo, parkedFrom: [])), pinned: pinned)
    return groups.map { ($0.id, $0.sessions.map(\.id)) }
}
