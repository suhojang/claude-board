// 보드 탭 짝짓기·자리(TabPairing.swift) 시험 — 화면 값(TabScreen)과 가짜 포트(화면 probe·부모 pid·넘긴 작업 id)로 진짜 엔진·프로세스·파일 없이 돌린다.
// 가짜는 이 파일에만 둔다. 가짜 포트는 부른 것을 한 줄씩 적어 포트 호출 차례까지 견준다(ProcessControl 의 신호·살아 있는지, ClaudeLive 의
// 세션 상태 판정 몫은 부르면 UNEXPECTED 로 적힌다).
// 표본은 옮기기 전 TerminalHub.pair·place·parkedTab·shownPlace(0b309cd 의 ClaudeBoard.swift)를 줄 그대로 떼어 가짜 화면·OS 로 돌린 덤프 도구와
// 같은 것이고, 기대 글(expected)은 그 덤프에서 앱에 남은 몫(다른 터미널 앱 이름·프로세스 인자 캐시·@Published 쓰기 기록)만 뺀 줄 그대로다.
// 화면의 맨 위 프로세스(leader)는 덤프의 앞 프로세스와 getsid 표에서 앱 어댑터(BoardSurface.screen)가 낼 값을 적었다.
// 덤프 뒤에 고친 것은 고친 동작으로 적었다: 짝 없는 세션을 세션 id 로 화면에 붙이는 길은 그 화면에서 claude 가 뜨는 중이거나 돌 때만
// (6 은 다른 터미널, 11 은 그 화면이지만 runsClaude 를 묻는다), 같은 tty 인 끝나지 않은 세션이 여럿이면 updated → since → 세션 id 차례로
// 정해진 하나가 화면의 세션(4a 가 4b 와 같아진다), 경로 상태 점은 일시중지를 대기보다 먼저(9 의 /w/d)
import Foundation
import Testing
import BoardRules

// MARK: - 가짜 포트 (판마다 새로 만들고 한 스레드에서만 쓴다 — 포트가 Sendable 이라 @unchecked)

private final class Log: @unchecked Sendable {
    var calls: [String] = []
}

private struct ParkedJob: Sendable {
    let session: String   // ~/.claude/sessions/<pid>.json 의 sessionId
    let job: String       // parkedJobId
}

private struct World: Sendable {
    var parents: [Int32: Int32] = [:]          // 부모 pid — 없으면 못 읽음(nil)
    var parkedJobs: [Int32: ParkedJob] = [:]   // claude pid → 넘긴 작업
    var modes: [String: String] = [:]          // 세션 id → 실행 방식(-p·--bg), 없으면 대화형
}

private final class FakeProbe: TabProbe, @unchecked Sendable {
    let log: Log
    let names: [UUID: String]
    let running: Set<UUID>
    let modes: [String: String]
    init(log: Log, names: [UUID: String], running: Set<UUID>, modes: [String: String]) {
        self.log = log; self.names = names; self.running = running; self.modes = modes
    }
    func runsClaude(_ screen: UUID) -> Bool { log.calls.append("runsClaude \(names[screen] ?? "?")"); return running.contains(screen) }
    func mode(_ s: Session) -> String? { log.calls.append("mode \(s.id)"); return modes[s.id] }
}

private final class FakeProcesses: ProcessControl, @unchecked Sendable {
    let log: Log
    let parents: [Int32: Int32]
    init(log: Log, parents: [Int32: Int32]) { self.log = log; self.parents = parents }
    func parentPID(_ pid: Int32) -> Int32? { log.calls.append("parent \(pid)"); return parents[pid] }
    func ancestorBundleID(_ pid: Int32) -> String? { log.calls.append("UNEXPECTED ancestor \(pid)"); return nil }
    func isAlive(_ pid: Int32) -> Bool { log.calls.append("UNEXPECTED alive \(pid)"); return false }
    func terminate(_ pid: Int32) { log.calls.append("UNEXPECTED terminate \(pid)") }
}

private final class FakeLive: ClaudeLive, @unchecked Sendable {
    let log: Log
    let jobs: [Int32: ParkedJob]
    init(log: Log, jobs: [Int32: ParkedJob]) { self.log = log; self.jobs = jobs }
    func parkedJob(_ pid: Int32, session: String) -> String? {
        log.calls.append("parkedJob \(pid) \(session)")
        guard let j = jobs[pid], j.session == session else { return nil }   // /clear 뒤 같은 pid 의 다른 세션은 모른다
        return j.job
    }
    func statusChange(_ pid: Int32, session: String) -> (status: String, at: Date)? { log.calls.append("UNEXPECTED status"); return nil }
    func name(_ pid: Int32) -> String? { log.calls.append("UNEXPECTED name"); return nil }
    func pid(forSession session: String) -> Int32? { log.calls.append("UNEXPECTED pid"); return nil }
    func prune(keep: Set<Int32>) { log.calls.append("UNEXPECTED prune") }
}

// MARK: - 표본

private struct ScreenSpec: Sendable {
    var name: String
    var workspace: String = "/w/a"
    var tty: String = ""
    var leader: Int32 = 0
    var sessionId: String? = nil
    var command: String? = nil
    var mdOff = false
    var runs = false
}

struct SessSpec: Sendable {   // 공유 규칙 표(RulesTableTests)도 세션을 이것으로 넘긴다
    var id: String
    var state: String
    var tty: String = ""
    var headless = false
    var pid: Int32 = 0
    var updated: Double = 0   // 마지막 hook 기록 시각 — t0 에서 초
    var since: Double = 0     // 이 상태가 된 시각 — t0 에서 초
}

private struct ScreenChange: Sendable {
    var runs: Bool? = nil
    var command: String?? = nil
}

private struct RoundSpec: Sendable {
    var sessions: [SessSpec]
    var changes: [String: ScreenChange] = [:]
    var probes: [SessSpec] = []    // 목록 밖에서 자리만 물어볼 세션(같은 id 의 다른 상태 등)
}

private struct PairingCase: Sendable, CustomTestStringConvertible {
    var name: String
    var screens: [ScreenSpec]
    var world = World()
    var rounds: [RoundSpec]
    var testDescription: String { name }
}

private let t0 = Date(timeIntervalSince1970: 1_800_000_000)
private func session(_ s: SessSpec) -> Session {
    Session(id: s.id, cwd: "/w", projectDir: "/w", state: s.state, since: t0 + s.since, updated: t0 + s.updated, headless: s.headless, tty: s.tty,
            topic: "", message: "", resumeCmd: "", file: "", ctx: nil, brain: nil, pid: s.pid, subagents: [], shells: 0,
            paused: false, muted: false, limitAt: nil, name: nil)
}

// from → base+1 → … → base+steps-1 → end : end 가 steps 번째 부모
private func chain(_ from: Int32, steps: Int, to end: Int32, base: Int32) -> [Int32: Int32] {
    var m: [Int32: Int32] = [:]
    var p = from
    for i in 1..<steps { m[p] = base + Int32(i); p = base + Int32(i) }
    m[p] = end
    return m
}

private let cases: [PairingCase] = [
    PairingCase(name: "1 tty 같음·다른 터미널 앱 이름",   // 앱 이름은 앱 몫 — 여기서는 tty 짝과 다른 터미널·백그라운드 자리만
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "s1", command: "claude --session-id s1", runs: true)],
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "s2", state: "working", tty: "ttys009", pid: 4001),
                    SessSpec(id: "s3", state: "idle", tty: "ttys010", pid: 4002),
                    SessSpec(id: "s4", state: "waiting", tty: "ttys011", pid: 4003),
                    SessSpec(id: "s5", state: "idle", tty: "ttys012", pid: 4004),
                    SessSpec(id: "s6", state: "idle", tty: "ttys013", pid: 4005),
                    SessSpec(id: "s7", state: "idle", tty: "", pid: 0),
                    SessSpec(id: "s8", state: "ended", tty: "ttys014", pid: 4006),
                    SessSpec(id: "s9", state: "idle", tty: "ttys015", pid: 4007),
                    SessSpec(id: "h1", state: "working", headless: true, pid: 4008),
                ]),
                RoundSpec(sessions: [
                    SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "s2", state: "working", tty: "ttys009", pid: 4001),
                    SessSpec(id: "s6", state: "idle", tty: "ttys013", pid: 4005),
                ])]),
    PairingCase(name: "2 tty 다름·tty 없음",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000), ScreenSpec(name: "A2")],
                world: World(parents: [3001: 3000, 3000: 1]),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "s1", state: "working", tty: "ttys005", pid: 2001),
                    SessSpec(id: "s2", state: "idle", tty: "", pid: 2002),
                    SessSpec(id: "s3", state: "waiting", tty: "", headless: true, pid: 0),
                    SessSpec(id: "s4", state: "working", tty: "", headless: true, pid: 3001),
                ])]),
    PairingCase(name: "3 headless 조상",   // h1 은 세 칸 위, h6 은 12번째 부모가 화면 맨 위 프로세스, h7 은 13번째라 못 찾음
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "s1", command: "claude", runs: true),
                          ScreenSpec(name: "B1", workspace: "/w/b", tty: "ttys002", leader: 8000)],
                world: World(parents: [3001: 2001, 2001: 1002, 1002: 1000, 3101: 1, 3301: 1000, 3401: 8000]
                    .merging(chain(5000, steps: 12, to: 1000, base: 5000)) { a, _ in a }
                    .merging(chain(6000, steps: 13, to: 1000, base: 6000)) { a, _ in a }),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "h1", state: "working", headless: true, pid: 3001),
                    SessSpec(id: "h2", state: "working", headless: true, pid: 3101),
                    SessSpec(id: "h3", state: "working", headless: true, pid: 3201),
                    SessSpec(id: "h4", state: "ended", headless: true, pid: 3301),
                    SessSpec(id: "h5", state: "working", headless: true, pid: 0),
                    SessSpec(id: "h6", state: "working", headless: true, pid: 5000),
                    SessSpec(id: "h7", state: "working", headless: true, pid: 6000),
                    SessSpec(id: "h8", state: "idle", headless: true, pid: 3401),
                ])]),
    // 고침: 같은 tty 인 끝나지 않은 세션이 둘이면 updated 가 늦은 것, 같으면 since 가 늦은 것, 그것도 같으면 세션 id 글자 차례의 뒤 것이
    // 화면의 세션·세션 id 다(여기는 셋 다 같아 s1). 4a·4b 는 목록 차례만 달라 결과가 같다(고치기 전에는 목록 차례의 뒤 것). paired 에는 둘 다 넣는다
    PairingCase(name: "4a 같은 tty 두 세션(대화형 먼저)",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000)],
                world: World(parents: [3001: 1002, 1002: 1000], modes: ["h1": "-p"]),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "h1", state: "working", tty: "ttys001", headless: true, pid: 3001),
                ])]),
    PairingCase(name: "4b 같은 tty 두 세션(headless 먼저)",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000)],
                world: World(parents: [3001: 1002, 1002: 1000], modes: ["h1": "-p"]),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "h1", state: "working", tty: "ttys001", headless: true, pid: 3001),
                    SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001),
                ])]),
    PairingCase(name: "5 끝난 세션",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "e1", command: "claude --resume e1", mdOff: true, runs: true),
                          ScreenSpec(name: "A2", tty: "ttys002", leader: 1100, sessionId: "e2", command: "claude", mdOff: true)],
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "e1", state: "ended", tty: "ttys001", pid: 2001),
                    SessSpec(id: "e2", state: "ended", tty: "ttys002", pid: 2002),
                    SessSpec(id: "e3", state: "ended", tty: "", pid: 0),
                ])]),
    // 고침: 화면에서 끝난 세션을 다른 터미널에서 이어 열면(tty 가 다름) 화면에 세션 id 가 남아 있어도 그 화면에서 claude 가 돌지 않으므로 다른 터미널
    // (고치기 전에는 그 화면을 주어 누르면 보드 탭으로 가고 키 넣기가 막혔다)
    PairingCase(name: "6 셸 프롬프트·다른 터미널에서 이어 연 세션",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "s1", command: "claude --session-id s1", mdOff: true)],
                rounds: [RoundSpec(sessions: [SessSpec(id: "s1", state: "idle", tty: "ttys007", pid: 2001)])]),
    PairingCase(name: "7 /clear·사람이 친 claude·실행 방식",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "old", command: "claude --session-id old", mdOff: true, runs: true),
                          ScreenSpec(name: "A2", tty: "ttys002", leader: 1100, runs: true),
                          ScreenSpec(name: "A3", tty: "ttys003", leader: 1200),
                          ScreenSpec(name: "A4", tty: "ttys004", leader: 1300),
                          ScreenSpec(name: "A5", tty: "ttys005", leader: 1400)],
                world: World(parents: [2005: 1400], modes: ["s3": "--bg", "s5": "-p"]),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "old", state: "ended", tty: "ttys001", pid: 2001),
                    SessSpec(id: "new1", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "s2", state: "idle", tty: "ttys002", pid: 2002),
                    SessSpec(id: "s3", state: "working", tty: "ttys003", pid: 2003),
                    SessSpec(id: "s4", state: "working", tty: "ttys004", pid: 2004),
                    SessSpec(id: "s5", state: "working", tty: "ttys005", headless: true, pid: 2005),
                ]),
                RoundSpec(sessions: [
                    SessSpec(id: "old", state: "ended", tty: "ttys001", pid: 2001),
                    SessSpec(id: "new1", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "s2", state: "idle", tty: "ttys002", pid: 2002),
                    SessSpec(id: "s3", state: "working", tty: "ttys003", pid: 2003),
                    SessSpec(id: "s4", state: "working", tty: "ttys004", pid: 2004),
                    SessSpec(id: "s5", state: "working", tty: "ttys005", headless: true, pid: 2005),
                ])]),
    PairingCase(name: "8 넘긴 대화·이어 받은 작업",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "abcd1234-self", command: "claude", runs: true),
                          ScreenSpec(name: "A2", tty: "ttys002", leader: 1100, sessionId: "s2", command: "claude", runs: true),
                          ScreenSpec(name: "A3", tty: "ttys003", leader: 1200, sessionId: "s3", command: "claude --resume s3", runs: true),
                          ScreenSpec(name: "A4", tty: "ttys004", leader: 1300, sessionId: "s4", command: "claude", runs: true)],
                world: World(parents: [7001: 1, 7002: 1, 7003: 1],
                             parkedJobs: [2001: ParkedJob(session: "abcd1234-self", job: "abcd1234"), 2002: ParkedJob(session: "s2", job: "ffff0000"),
                                          2004: ParkedJob(session: "other", job: "abcd1234")]),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "abcd1234-self", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "abcd1234-job1", state: "working", headless: true, pid: 7001),
                    SessSpec(id: "abcd1234-job2", state: "waiting", headless: true, pid: 7002),
                    SessSpec(id: "s2", state: "idle", tty: "ttys002", pid: 2002),
                    SessSpec(id: "ffff0000-job", state: "ended", headless: true, pid: 7003),
                    SessSpec(id: "s3", state: "ended", tty: "ttys003", pid: 2003),
                    SessSpec(id: "s4", state: "working", tty: "ttys004", pid: 2004),
                ], probes: [SessSpec(id: "abcd1234-job1", state: "ended", headless: true, pid: 7001)]),
                RoundSpec(sessions: [
                    SessSpec(id: "abcd1234-self", state: "working", tty: "ttys001", pid: 2001),
                    SessSpec(id: "abcd1234-job1", state: "ended", headless: true, pid: 7001),
                    SessSpec(id: "s2", state: "idle", tty: "ttys002", pid: 2002),
                ])]),
    // 고침: 경로 점은 승인 대기 > 작업 중 > 일시중지 > 대기 — /w/c·/w/d 모두 일시중지(고치기 전에는 같은 순위라 화면 차례의 앞 것, /w/d 는 대기)
    PairingCase(name: "9 탭 여러 개·화면 나눔·경로 상태 점",
                screens: [ScreenSpec(name: "A1", tty: "ttys101", leader: 2100, sessionId: "a-wait", command: "claude", runs: true),
                          ScreenSpec(name: "A2", tty: "ttys102", leader: 2200, sessionId: "a-work", command: "claude", runs: true),
                          ScreenSpec(name: "B1", tty: "ttys103", leader: 2300, sessionId: "b-idle", command: "claude", runs: true),
                          ScreenSpec(name: "C1", workspace: "/w/c", tty: "ttys104", leader: 2400, sessionId: "c-paused", command: "claude", runs: true),
                          ScreenSpec(name: "C2", workspace: "/w/c", tty: "ttys105", leader: 2500, sessionId: "c-idle", command: "claude", runs: true),
                          ScreenSpec(name: "D1", workspace: "/w/d", tty: "ttys106", leader: 2510, sessionId: "d-idle", command: "claude", runs: true),
                          ScreenSpec(name: "D2", workspace: "/w/d", tty: "ttys107", leader: 2520, sessionId: "d-paused", command: "claude", runs: true),
                          ScreenSpec(name: "E1", workspace: "/w/e", tty: "ttys108", leader: 2530),
                          ScreenSpec(name: "F1", workspace: "/w/f", tty: "ttys020", leader: 2600, command: "claude"),
                          ScreenSpec(name: "F2", workspace: "/w/f", tty: "ttys020", leader: 2600)],
                world: World(parents: [2602: 2600]),
                rounds: [RoundSpec(sessions: [
                    SessSpec(id: "a-wait", state: "waiting", tty: "ttys101", pid: 2101),
                    SessSpec(id: "a-work", state: "working", tty: "ttys102", pid: 2102),
                    SessSpec(id: "b-idle", state: "idle", tty: "ttys103", pid: 2103),
                    SessSpec(id: "c-paused", state: "paused", tty: "ttys104", pid: 2104),
                    SessSpec(id: "c-idle", state: "idle", tty: "ttys105", pid: 2105),
                    SessSpec(id: "d-idle", state: "idle", tty: "ttys106", pid: 2106),
                    SessSpec(id: "d-paused", state: "paused", tty: "ttys107", pid: 2107),
                    SessSpec(id: "f1", state: "idle", tty: "ttys020", pid: 2601),
                    SessSpec(id: "fh", state: "working", headless: true, pid: 2602),
                ])]),
    PairingCase(name: "10 짝이 바뀌고 풀림",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "s1", command: "claude --session-id s1", runs: true)],
                rounds: [RoundSpec(sessions: [SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001)]),
                         RoundSpec(sessions: [SessSpec(id: "s1", state: "ended", tty: "ttys001", pid: 2001)],
                                   changes: ["A1": ScreenChange(runs: false)]),
                         RoundSpec(sessions: [SessSpec(id: "s1", state: "ended", tty: "ttys001", pid: 2001)],
                                   changes: ["A1": ScreenChange(runs: true, command: .some("claude --resume s1"))]),
                         RoundSpec(sessions: [SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2003)]),
                         RoundSpec(sessions: [SessSpec(id: "s1", state: "ended", tty: "ttys001", pid: 2003),
                                              SessSpec(id: "s1b", state: "working", tty: "ttys001", pid: 2003)])]),
    PairingCase(name: "11 tty 를 아직 안 쓴 새 세션",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "n1", command: "claude --session-id n1", runs: true),
                          ScreenSpec(name: "A2", tty: "ttys002", sessionId: "n2", command: "claude -p hi", runs: true)],
                rounds: [RoundSpec(sessions: [SessSpec(id: "n1", state: "working", tty: "", pid: 0),
                                              SessSpec(id: "n2", state: "working", tty: "", headless: true, pid: 0)])]),
    PairingCase(name: "12 앞 프로세스 없음·0·getsid 실패",   // 앞 프로세스가 없거나 0 이거나 getsid 가 실패한 화면은 맨 위 프로세스 0
                screens: [ScreenSpec(name: "A1", tty: "ttys001"), ScreenSpec(name: "A2", tty: "ttys002"),
                          ScreenSpec(name: "A3", tty: "ttys003"), ScreenSpec(name: "A4", tty: "ttys004", leader: 2400)],
                world: World(parents: [3001: 2400, 3002: 9999, 3003: 0]),
                rounds: [RoundSpec(sessions: [SessSpec(id: "h1", state: "working", headless: true, pid: 3001),
                                              SessSpec(id: "h2", state: "working", headless: true, pid: 3002),
                                              SessSpec(id: "h3", state: "working", headless: true, pid: 3003)])]),
    PairingCase(name: "13 headless 의 tty 는 다른 화면",
                screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "s1", command: "claude", runs: true),
                          ScreenSpec(name: "A2", tty: "ttys002", leader: 1100)],
                world: World(parents: [3001: 1000], modes: ["h1": "-p"]),
                rounds: [RoundSpec(sessions: [SessSpec(id: "s1", state: "working", tty: "ttys001", pid: 2001),
                                              SessSpec(id: "h1", state: "working", tty: "ttys002", headless: true, pid: 3001)])]),
]

// MARK: - 돌리기 — 판마다 짝짓고, 앱처럼 화면의 세션 id·명령·끈 표시를 다음 판에 넘긴다. 자리는 그 판의 짝으로 묻는다

private func fmt(_ p: SessionPlace, _ names: [UUID: String]) -> String {
    switch p {
    case .tab(let id, let headless): return "tab \(names[id] ?? "?") headless=\(headless)"
    case .terminal: return "terminal"
    case .background: return "background"
    case .ended: return "ended"
    }
}

private func run(_ c: PairingCase) -> String {
    var ids: [String: UUID] = [:], names: [UUID: String] = [:]
    for (i, s) in c.screens.enumerated() {
        let id = UUID(uuidString: String(format: "00000000-0000-0000-0000-%012d", i + 1))!
        ids[s.name] = id; names[id] = s.name
    }
    func name(_ id: UUID?) -> String { id.map { names[$0] ?? "?" } ?? "nil" }
    func dict(_ d: [String: UUID]) -> String { d.sorted { $0.key < $1.key }.map { "\($0.key)→\(name($0.value))" }.joined(separator: " ") }
    var out: [String] = []
    var specs = c.screens
    for (k, round) in c.rounds.enumerated() {
        for (n, ch) in round.changes {
            let i = specs.firstIndex { $0.name == n }!
            if let r = ch.runs { specs[i].runs = r }
            if let cmd = ch.command { specs[i].command = cmd }
        }
        let log = Log()
        let probe = FakeProbe(log: log, names: names, running: Set(specs.filter(\.runs).map { ids[$0.name]! }), modes: c.world.modes)
        let screens = specs.map {
            TabScreen(id: ids[$0.name]!, workspace: $0.workspace, tty: $0.tty, leader: $0.leader, sessionId: $0.sessionId,
                      command: $0.command, mdRenderOff: $0.mdOff)
        }
        let sessions = round.sessions.map(session)
        let r = pairTabs(sessions, screens: screens, probe: probe, processes: FakeProcesses(log: log, parents: c.world.parents),
                         live: FakeLive(log: log, jobs: c.world.parkedJobs))
        out.append("=== \(c.name) / 판 \(k + 1)")
        out.append("pair 조회: " + log.calls.joined(separator: " | "))
        out.append("끈 세션: " + r.screens.compactMap(\.mdRenderOffSession).joined(separator: " "))
        out.append("paired: " + dict(r.paired))
        out.append("owned: " + r.ownedHeadless.sorted().joined(separator: " "))
        out.append("parkedTabs: " + dict(r.parkedTabs))
        out.append("parkedFrom: " + r.parkedFrom.sorted().joined(separator: " "))
        out.append("states: " + r.workspaceStates.sorted { $0.key < $1.key }.map { "\($0.key)→\($0.value)" }.joined(separator: " "))
        for p in r.screens {
            out.append("화면 \(name(p.id)): sid=\(p.sessionId ?? "nil") cmd=\(p.command ?? "nil") off=\(p.mdRenderOff) session=\(p.session?.id ?? "nil") parked=\(p.parked?.id ?? "nil")")
        }
        for (i, p) in r.screens.enumerated() {
            specs[i].sessionId = p.sessionId; specs[i].command = p.command; specs[i].mdOff = p.mdRenderOff
        }
        let placeScreens = r.screens.map { ScreenSession(id: $0.id, sessionId: $0.sessionId) }
        for s in sessions + round.probes.map(session) {
            log.calls = []
            let place = sessionPlace(s, paired: r.paired, ownedHeadless: r.ownedHeadless, screens: placeScreens, probe: probe)
            let shown = shownSessionPlace(parked: parkedScreen(s, parkedTabs: r.parkedTabs),
                                          place: sessionPlace(s, paired: r.paired, ownedHeadless: r.ownedHeadless, screens: placeScreens, probe: probe))
            let parked = parkedScreen(s, parkedTabs: r.parkedTabs)
            out.append("자리 \(s.id)[\(s.state)]: place=\(fmt(place, names)) shown=\(fmt(shown, names)) parkedTab=\(name(parked)) 조회: "
                       + log.calls.joined(separator: " | "))
        }
    }
    // 빈 목록 뒤 끝 공백은 기대 글에 남기지 않는다(편집기가 지워도 같게)
    return out.map { $0.replacingOccurrences(of: #"\s+$"#, with: "", options: .regularExpression) }.joined(separator: "\n")
}

// MARK: - 자리 표 — 짝(paired)이 없는 세션을 같은 세션 id 로 연 화면에 붙이는 길은 그 화면에서 claude 가 뜨는 중이거나 돌 때만(끝난 세션과 같은 조건).
// 화면 값은 이름·세션 id·runs 만 본다. paired·owned 는 화면 이름으로 적는다

private struct PlaceCase: Sendable, CustomTestStringConvertible {
    var name: String
    var session: SessSpec
    var screens: [ScreenSpec]
    var paired: [String: String] = [:]   // 세션 id → 화면 이름
    var owned: Set<String> = []
    var place: String                    // fmt 결과
    var calls: [String]                  // 자리 한 번에 부른 포트(runsClaude)
    var testDescription: String { name }
}

private let placeCases: [PlaceCase] = [
    PlaceCase(name: "보드 탭에서 끝나 셸만 남은 화면 + 다른 터미널에서 이어 연 같은 세션(다른 tty) → 다른 터미널",
              session: SessSpec(id: "x", state: "idle", tty: "ttys007", pid: 4001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x")],
              place: "terminal", calls: ["runsClaude A1"]),
    PlaceCase(name: "같은 장면에서 이어 연 세션이 작업 중이어도 다른 터미널",
              session: SessSpec(id: "x", state: "working", tty: "ttys007", pid: 4001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x")],
              place: "terminal", calls: ["runsClaude A1"]),
    PlaceCase(name: "같은 장면에서 이어 연 세션이 headless(tty 없음)면 백그라운드",
              session: SessSpec(id: "x", state: "working", headless: true, pid: 4001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x")],
              place: "background", calls: ["runsClaude A1"]),
    PlaceCase(name: "이어 열기를 넣은 직후(화면에서 claude 가 뜨는 중, 세션 파일 tty 아직 없음) → 그 화면",
              session: SessSpec(id: "x", state: "idle", tty: "", pid: 0),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x", runs: true)],
              place: "tab A1 headless=false", calls: ["runsClaude A1"]),
    PlaceCase(name: "같은 tty 로 짝지은 세션 → 그 화면(화면 목록을 만들지 않고 묻지 않는다)",
              session: SessSpec(id: "x", state: "working", tty: "ttys001", pid: 2001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x")], paired: ["x": "A1"],
              place: "tab A1 headless=false", calls: []),
    PlaceCase(name: "화면 안에서 시작된 headless → 그 화면(headless 표시)",
              session: SessSpec(id: "h", state: "working", headless: true, pid: 3001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "s1", runs: true)], paired: ["h": "A1"], owned: ["h"],
              place: "tab A1 headless=true", calls: []),
    PlaceCase(name: "끝난 세션 — 같은 세션 id 화면에서 claude 가 뜨는 중이면 그 화면(그대로)",
              session: SessSpec(id: "x", state: "ended", tty: "ttys001", pid: 2001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x", runs: true)],
              place: "tab A1 headless=false", calls: ["runsClaude A1"]),
    PlaceCase(name: "끝난 세션 — 셸만 남은 화면이면 끝남(그대로)",
              session: SessSpec(id: "x", state: "ended", tty: "ttys001", pid: 2001),
              screens: [ScreenSpec(name: "A1", tty: "ttys001", leader: 1000, sessionId: "x")],
              place: "ended", calls: ["runsClaude A1"]),
    PlaceCase(name: "세션 id 가 다른 화면은 claude 가 돌아도 묻지 않고, 같은 세션 id 화면 가운데 claude 가 도는 첫 화면",
              session: SessSpec(id: "x", state: "idle", tty: "", pid: 0),
              screens: [ScreenSpec(name: "B1", tty: "ttys002", sessionId: "other", runs: true),
                        ScreenSpec(name: "A1", tty: "ttys001", sessionId: "x"),
                        ScreenSpec(name: "A2", tty: "ttys003", sessionId: "x", runs: true)],
              place: "tab A2 headless=false", calls: ["runsClaude A1", "runsClaude A2"]),
]

// MARK: - 같은 tty 세션 고르기 — 같은 tty 에 끝나지 않은 세션이 여럿이면 화면의 세션은 updated(마지막 hook 기록)가 가장 늦은 것,
// 같으면 since 가 늦은 것, 그것도 같으면 세션 id 글자 차례의 뒤 것. 표마다 목록 차례를 뒤집어 두 번 돌린다

private struct TtyPick: Sendable, CustomTestStringConvertible {
    var name: String
    var sessions: [SessSpec]   // 모두 ttys001
    var winner: String
    var testDescription: String { name }
}

private let ttyPicks: [TtyPick] = [
    TtyPick(name: "updated 가 늦은 것(세션 id 차례와 반대여도)",
            sessions: [SessSpec(id: "a", state: "idle", tty: "ttys001", pid: 2001, updated: 10),
                       SessSpec(id: "b", state: "working", tty: "ttys001", pid: 2002, updated: 0, since: 20)],
            winner: "a"),
    TtyPick(name: "updated 가 같으면 since 가 늦은 것",
            sessions: [SessSpec(id: "a", state: "idle", tty: "ttys001", pid: 2001, updated: 5, since: 3),
                       SessSpec(id: "b", state: "idle", tty: "ttys001", pid: 2002, updated: 5, since: 1)],
            winner: "a"),
    TtyPick(name: "updated·since 가 같으면 세션 id 글자 차례의 뒤 것",
            sessions: [SessSpec(id: "a", state: "working", tty: "ttys001", pid: 2001),
                       SessSpec(id: "b", state: "idle", tty: "ttys001", pid: 2002)],
            winner: "b"),
    TtyPick(name: "끝난 세션은 견주지 않는다(updated 가 늦어도)",
            sessions: [SessSpec(id: "a", state: "idle", tty: "ttys001", pid: 2001),
                       SessSpec(id: "z", state: "ended", tty: "ttys001", pid: 2002, updated: 100)],
            winner: "a"),
]

// MARK: - 경로 줄 상태 점 — 승인 대기 > 작업 중 > 일시중지 > 대기 > 끝남, 같은 상태끼리는 화면 차례의 앞 것.
// Session.rank(카드 정렬)는 일시중지·대기가 같은 순위라 이 점에서만 따로 본다

private struct DotCase: Sendable, CustomTestStringConvertible {
    var name: String
    var states: [String]   // 같은 경로의 화면 차례대로 그 화면의 세션 상태
    var dot: String
    var testDescription: String { name }
}

private let dotCases: [DotCase] = [
    DotCase(name: "대기 화면이 앞·일시중지 화면이 뒤 → 일시중지", states: ["idle", "paused"], dot: "paused"),
    DotCase(name: "일시중지 화면이 앞·대기 화면이 뒤 → 일시중지", states: ["paused", "idle"], dot: "paused"),
    DotCase(name: "승인 대기가 가장 먼저", states: ["paused", "working", "waiting", "idle"], dot: "waiting"),
    DotCase(name: "작업 중은 일시중지보다 먼저", states: ["paused", "working"], dot: "working"),
    DotCase(name: "작업 중은 일시중지보다 먼저(차례 반대)", states: ["working", "paused"], dot: "working"),
    DotCase(name: "대기는 끝남(claude 가 뜨는 중인 화면의 끝난 세션)보다 먼저", states: ["ended", "idle"], dot: "idle"),
    DotCase(name: "끝난 세션뿐이면 끝남", states: ["ended"], dot: "ended"),
]

// 경로 /w/a 의 화면마다 그 화면의 세션 하나(상태 차례대로)를 두고 짝짓는다 — 경로 상태 점 표와 공유 규칙 표(runWorkspaceDotRow)가 함께 쓴다
private func dotPairing(_ states: [String]) -> TabPairing {
    let log = Log()
    var names: [UUID: String] = [:], screens: [TabScreen] = [], sessions: [SessSpec] = []
    for (i, state) in states.enumerated() {
        let id = UUID(uuidString: String(format: "00000000-0000-0000-0000-%012d", i + 1))!
        names[id] = "A\(i + 1)"
        // 끝난 세션은 tty 로 짝짓지 않으므로 같은 세션 id 로 연 화면에서 claude 가 뜨는 중이어야 화면의 세션이 된다
        screens.append(TabScreen(id: id, workspace: "/w/a", tty: "ttys10\(i)", leader: Int32(1000 + i), sessionId: "s\(i)",
                                 command: "claude", mdRenderOff: false))
        sessions.append(SessSpec(id: "s\(i)", state: state, tty: "ttys10\(i)", pid: Int32(2000 + i)))
    }
    return pairTabs(sessions.map(session), screens: screens, probe: FakeProbe(log: log, names: names, running: Set(names.keys), modes: [:]),
                    processes: FakeProcesses(log: log, parents: [:]), live: FakeLive(log: log, jobs: [:]))
}

@Suite("보드 탭 짝짓기·자리") struct TabPairingTests {
    @Test("경로 상태 점: 승인 대기 > 작업 중 > 일시중지 > 대기 > 끝남", arguments: dotCases)
    fileprivate func workspaceDot(_ c: DotCase) {
        #expect(dotPairing(c.states).workspaceStates == ["/w/a": c.dot])
    }

    @Test("짝짓기: 같은 tty 의 끝나지 않은 세션이 여럿이면 updated → since → 세션 id 차례로 하나, 목록 차례와 관계없고 paired 에는 모두", arguments: ttyPicks)
    fileprivate func sameTtyPick(_ c: TtyPick) {
        let screen = UUID()
        for order in [c.sessions, Array(c.sessions.reversed())] {
            let log = Log()
            let label = order.map(\.id).joined(separator: ",")
            let r = pairTabs(order.map(session),
                             screens: [TabScreen(id: screen, workspace: "/w/a", tty: "ttys001", leader: 1000, sessionId: nil, command: nil, mdRenderOff: false)],
                             probe: FakeProbe(log: log, names: [screen: "A1"], running: [], modes: [:]),
                             processes: FakeProcesses(log: log, parents: [:]), live: FakeLive(log: log, jobs: [:]))
            #expect(r.screens.first?.session?.id == c.winner, "\(label)")
            #expect(r.screens.first?.sessionId == c.winner, "\(label)")
            #expect(r.paired == Dictionary(uniqueKeysWithValues: c.sessions.filter { $0.state != "ended" }.map { ($0.id, screen) }), "\(label)")
        }
    }

    @Test("자리: 짝 없는 세션은 같은 세션 id 화면에서 claude 가 뜨는 중이거나 돌 때만 그 화면", arguments: placeCases)
    fileprivate func placeBySessionId(_ c: PlaceCase) {
        var ids: [String: UUID] = [:], names: [UUID: String] = [:]
        for (i, s) in c.screens.enumerated() {
            let id = UUID(uuidString: String(format: "00000000-0000-0000-0000-%012d", i + 1))!
            ids[s.name] = id; names[id] = s.name
        }
        let log = Log()
        let probe = FakeProbe(log: log, names: names, running: Set(c.screens.filter(\.runs).map { ids[$0.name]! }), modes: [:])
        let screens = c.screens.map { ScreenSession(id: ids[$0.name]!, sessionId: $0.sessionId) }
        let place = sessionPlace(session(c.session), paired: c.paired.mapValues { ids[$0]! }, ownedHeadless: c.owned,
                                 screens: screens, probe: probe)
        #expect(fmt(place, names) == c.place)
        #expect(log.calls == c.calls)
    }

    @Test("보드 탭 짝짓기·자리 — 옮기기 전 덤프와 같다(포트 호출 차례까지)", arguments: cases)
    fileprivate func pairingMatchesDump(_ c: PairingCase) {
        #expect(run(c) == expected[c.name])
    }

    @Test("표본마다 기대 글이 있고, 짝짓기는 ProcessControl·ClaudeLive 의 다른 몫을 부르지 않는다")
    func pairingCoverage() {
        #expect(Set(cases.map(\.name)) == Set(expected.keys))
        for c in cases { #expect(!run(c).contains("UNEXPECTED"), "\(c.name)") }
    }

    @Test("자리: 짝이 있으면 화면 목록을 만들지 않고, 이어 받은 작업이면 place 를 묻지 않는다")
    func placeIsLazy() {
        let log = Log()
        let a = UUID(), b = UUID()
        let probe = FakeProbe(log: log, names: [a: "A", b: "B"], running: [b], modes: [:])
        let s = session(SessSpec(id: "s1", state: "ended", headless: true, pid: 7))
        var built = 0
        func screens() -> [ScreenSession] { built += 1; return [ScreenSession(id: b, sessionId: "s1")] }
        // #expect 가 인자를 먼저 풀어 보일 수 있어 값을 먼저 받아 둔다
        let pairedHit = sessionPlace(s, paired: ["s1": a], ownedHeadless: ["s1"], screens: screens(), probe: probe)
        #expect(pairedHit == .tab(a, headless: true))
        #expect(built == 0 && log.calls.isEmpty)
        // 짝이 없으면 같은 세션 id 화면 — 끝난 세션은 그 화면에서 claude 가 뜨는 중일 때만, headless 표시는 ownedHeadless 로만
        let fallback = sessionPlace(s, paired: [:], ownedHeadless: [], screens: screens(), probe: probe)
        #expect(fallback == .tab(b, headless: false))
        #expect(built == 1 && log.calls == ["runsClaude B"])
        var asked = false
        func place() -> SessionPlace { asked = true; return .terminal }
        let parkedHit = shownSessionPlace(parked: a, place: place())
        #expect(parkedHit == .tab(a, headless: false))
        #expect(!asked)
        let notParked = shownSessionPlace(parked: nil, place: place())
        #expect(notParked == .terminal && asked)
        // 끝난 작업은 parkedTabs 에 남아 있어도 이어 받은 화면이 없다
        #expect(parkedScreen(s, parkedTabs: ["s1": a]) == nil)
        #expect(parkedScreen(session(SessSpec(id: "s1", state: "working")), parkedTabs: ["s1": a]) == a)
    }
}

// MARK: - 기대 글 — 옮기기 전 덤프(0b309cd)에서 앱 몫을 뺀 줄 그대로. 덤프의 "pair 효과" 줄은 끈 세션으로 기억한 id 만 남겼다

private let expected: [String: String] = [
    "1 tty 같음·다른 터미널 앱 이름": """
    === 1 tty 같음·다른 터미널 앱 이름 / 판 1
    pair 조회: parent 4008 | parkedJob 2001 s1
    끈 세션:
    paired: s1→A1
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude --session-id s1 off=false session=s1 parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 s2[working]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s3[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s4[waiting]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s5[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s6[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s7[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s8[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 s9[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 h1[working]: place=background shown=background parkedTab=nil 조회:
    === 1 tty 같음·다른 터미널 앱 이름 / 판 2
    pair 조회: parkedJob 2001 s1
    끈 세션:
    paired: s1→A1
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude --session-id s1 off=false session=s1 parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 s2[working]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s6[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    """,
    "2 tty 다름·tty 없음": """
    === 2 tty 다름·tty 없음 / 판 1
    pair 조회: parent 3001 | parent 3000 | runsClaude A1 | runsClaude A2
    끈 세션:
    paired:
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→idle
    화면 A1: sid=nil cmd=nil off=false session=nil parked=nil
    화면 A2: sid=nil cmd=nil off=false session=nil parked=nil
    자리 s1[working]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s2[idle]: place=terminal shown=terminal parkedTab=nil 조회:
    자리 s3[waiting]: place=background shown=background parkedTab=nil 조회:
    자리 s4[working]: place=background shown=background parkedTab=nil 조회:
    """,
    "3 headless 조상": """
    === 3 headless 조상 / 판 1
    pair 조회: parent 3001 | parent 2001 | parent 1002 | parent 3101 | parent 3201 | parent 5000 | parent 5001 | parent 5002 | parent 5003 | parent 5004 | parent 5005 | parent 5006 | parent 5007 | parent 5008 | parent 5009 | parent 5010 | parent 5011 | parent 6000 | parent 6001 | parent 6002 | parent 6003 | parent 6004 | parent 6005 | parent 6006 | parent 6007 | parent 6008 | parent 6009 | parent 6010 | parent 6011 | parent 3401 | parkedJob 2001 s1 | runsClaude B1
    끈 세션:
    paired: h1→A1 h6→A1 h8→B1 s1→A1
    owned: h1 h6 h8
    parkedTabs:
    parkedFrom:
    states: /w/a→working /w/b→idle
    화면 A1: sid=s1 cmd=claude off=false session=s1 parked=nil
    화면 B1: sid=nil cmd=nil off=false session=nil parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 h1[working]: place=tab A1 headless=true shown=tab A1 headless=true parkedTab=nil 조회:
    자리 h2[working]: place=background shown=background parkedTab=nil 조회:
    자리 h3[working]: place=background shown=background parkedTab=nil 조회:
    자리 h4[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 h5[working]: place=background shown=background parkedTab=nil 조회:
    자리 h6[working]: place=tab A1 headless=true shown=tab A1 headless=true parkedTab=nil 조회:
    자리 h7[working]: place=background shown=background parkedTab=nil 조회:
    자리 h8[idle]: place=tab B1 headless=true shown=tab B1 headless=true parkedTab=nil 조회:
    """,
    "4a 같은 tty 두 세션(대화형 먼저)": """
    === 4a 같은 tty 두 세션(대화형 먼저) / 판 1
    pair 조회: parent 3001 | parent 1002 | mode s1 | parkedJob 2001 s1
    끈 세션:
    paired: h1→A1 s1→A1
    owned: h1
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude off=false session=s1 parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 h1[working]: place=tab A1 headless=true shown=tab A1 headless=true parkedTab=nil 조회:
    """,
    "4b 같은 tty 두 세션(headless 먼저)": """
    === 4b 같은 tty 두 세션(headless 먼저) / 판 1
    pair 조회: parent 3001 | parent 1002 | mode s1 | parkedJob 2001 s1
    끈 세션:
    paired: h1→A1 s1→A1
    owned: h1
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude off=false session=s1 parked=nil
    자리 h1[working]: place=tab A1 headless=true shown=tab A1 headless=true parkedTab=nil 조회:
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    """,
    "5 끝난 세션": """
    === 5 끝난 세션 / 판 1
    pair 조회: runsClaude A1 | runsClaude A2
    끈 세션: e1
    paired:
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→ended
    화면 A1: sid=e1 cmd=claude --resume e1 off=true session=e1 parked=nil
    화면 A2: sid=e2 cmd=nil off=false session=nil parked=nil
    자리 e1[ended]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회: runsClaude A1 | runsClaude A1
    자리 e2[ended]: place=ended shown=ended parkedTab=nil 조회: runsClaude A2 | runsClaude A2
    자리 e3[ended]: place=ended shown=ended parkedTab=nil 조회:
    """,
    "6 셸 프롬프트·다른 터미널에서 이어 연 세션": """
    === 6 셸 프롬프트·다른 터미널에서 이어 연 세션 / 판 1
    pair 조회: runsClaude A1
    끈 세션:
    paired:
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→idle
    화면 A1: sid=s1 cmd=nil off=false session=nil parked=nil
    자리 s1[idle]: place=terminal shown=terminal parkedTab=nil 조회: runsClaude A1 | runsClaude A1
    """,
    "7 /clear·사람이 친 claude·실행 방식": """
    === 7 /clear·사람이 친 claude·실행 방식 / 판 1
    pair 조회: parent 2005 | parkedJob 2001 new1 | mode s2 | parkedJob 2002 s2 | mode s3 | parkedJob 2003 s3 | mode s4 | parkedJob 2004 s4 | mode s5 | parkedJob 2005 s5
    끈 세션: new1
    paired: new1→A1 s2→A2 s3→A3 s4→A4 s5→A5
    owned: s5
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=new1 cmd=claude --session-id old off=true session=new1 parked=nil
    화면 A2: sid=s2 cmd=claude off=false session=s2 parked=nil
    화면 A3: sid=s3 cmd=nil off=false session=s3 parked=nil
    화면 A4: sid=s4 cmd=claude off=false session=s4 parked=nil
    화면 A5: sid=s5 cmd=nil off=false session=s5 parked=nil
    자리 old[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 new1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 s2[idle]: place=tab A2 headless=false shown=tab A2 headless=false parkedTab=nil 조회:
    자리 s3[working]: place=tab A3 headless=false shown=tab A3 headless=false parkedTab=nil 조회:
    자리 s4[working]: place=tab A4 headless=false shown=tab A4 headless=false parkedTab=nil 조회:
    자리 s5[working]: place=tab A5 headless=true shown=tab A5 headless=true parkedTab=nil 조회:
    === 7 /clear·사람이 친 claude·실행 방식 / 판 2
    pair 조회: parent 2005 | parkedJob 2001 new1 | parkedJob 2002 s2 | mode s3 | parkedJob 2003 s3 | parkedJob 2004 s4 | mode s5 | parkedJob 2005 s5
    끈 세션: new1
    paired: new1→A1 s2→A2 s3→A3 s4→A4 s5→A5
    owned: s5
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=new1 cmd=claude --session-id old off=true session=new1 parked=nil
    화면 A2: sid=s2 cmd=claude off=false session=s2 parked=nil
    화면 A3: sid=s3 cmd=nil off=false session=s3 parked=nil
    화면 A4: sid=s4 cmd=claude off=false session=s4 parked=nil
    화면 A5: sid=s5 cmd=nil off=false session=s5 parked=nil
    자리 old[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 new1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 s2[idle]: place=tab A2 headless=false shown=tab A2 headless=false parkedTab=nil 조회:
    자리 s3[working]: place=tab A3 headless=false shown=tab A3 headless=false parkedTab=nil 조회:
    자리 s4[working]: place=tab A4 headless=false shown=tab A4 headless=false parkedTab=nil 조회:
    자리 s5[working]: place=tab A5 headless=true shown=tab A5 headless=true parkedTab=nil 조회:
    """,
    "8 넘긴 대화·이어 받은 작업": """
    === 8 넘긴 대화·이어 받은 작업 / 판 1
    pair 조회: parent 7001 | parent 7002 | parkedJob 2001 abcd1234-self | parkedJob 2002 s2 | runsClaude A3 | parkedJob 2004 s4
    끈 세션:
    paired: abcd1234-self→A1 s2→A2 s4→A4
    owned:
    parkedTabs: abcd1234-job1→A1
    parkedFrom: abcd1234-self
    states: /w/a→working
    화면 A1: sid=abcd1234-self cmd=claude off=false session=abcd1234-self parked=abcd1234-job1
    화면 A2: sid=s2 cmd=claude off=false session=s2 parked=nil
    화면 A3: sid=s3 cmd=claude --resume s3 off=false session=s3 parked=nil
    화면 A4: sid=s4 cmd=claude off=false session=s4 parked=nil
    자리 abcd1234-self[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 abcd1234-job1[working]: place=background shown=tab A1 headless=false parkedTab=A1 조회:
    자리 abcd1234-job2[waiting]: place=background shown=background parkedTab=nil 조회:
    자리 s2[idle]: place=tab A2 headless=false shown=tab A2 headless=false parkedTab=nil 조회:
    자리 ffff0000-job[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 s3[ended]: place=tab A3 headless=false shown=tab A3 headless=false parkedTab=nil 조회: runsClaude A3 | runsClaude A3
    자리 s4[working]: place=tab A4 headless=false shown=tab A4 headless=false parkedTab=nil 조회:
    자리 abcd1234-job1[ended]: place=ended shown=ended parkedTab=nil 조회:
    === 8 넘긴 대화·이어 받은 작업 / 판 2
    pair 조회: parkedJob 2001 abcd1234-self | parkedJob 2002 s2 | runsClaude A3 | runsClaude A4
    끈 세션:
    paired: abcd1234-self→A1 s2→A2
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=abcd1234-self cmd=claude off=false session=abcd1234-self parked=nil
    화면 A2: sid=s2 cmd=claude off=false session=s2 parked=nil
    화면 A3: sid=s3 cmd=claude --resume s3 off=false session=nil parked=nil
    화면 A4: sid=s4 cmd=claude off=false session=nil parked=nil
    자리 abcd1234-self[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 abcd1234-job1[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 s2[idle]: place=tab A2 headless=false shown=tab A2 headless=false parkedTab=nil 조회:
    """,
    "9 탭 여러 개·화면 나눔·경로 상태 점": """
    === 9 탭 여러 개·화면 나눔·경로 상태 점 / 판 1
    pair 조회: parent 2602 | parkedJob 2101 a-wait | parkedJob 2102 a-work | parkedJob 2103 b-idle | parkedJob 2104 c-paused | parkedJob 2105 c-idle | parkedJob 2106 d-idle | parkedJob 2107 d-paused | runsClaude E1 | runsClaude F1 | mode f1 | parkedJob 2601 f1
    끈 세션:
    paired: a-wait→A1 a-work→A2 b-idle→B1 c-idle→C2 c-paused→C1 d-idle→D1 d-paused→D2 f1→F2 fh→F2
    owned: fh
    parkedTabs:
    parkedFrom:
    states: /w/a→waiting /w/c→paused /w/d→paused /w/e→idle /w/f→idle
    화면 A1: sid=a-wait cmd=claude off=false session=a-wait parked=nil
    화면 A2: sid=a-work cmd=claude off=false session=a-work parked=nil
    화면 B1: sid=b-idle cmd=claude off=false session=b-idle parked=nil
    화면 C1: sid=c-paused cmd=claude off=false session=c-paused parked=nil
    화면 C2: sid=c-idle cmd=claude off=false session=c-idle parked=nil
    화면 D1: sid=d-idle cmd=claude off=false session=d-idle parked=nil
    화면 D2: sid=d-paused cmd=claude off=false session=d-paused parked=nil
    화면 E1: sid=nil cmd=nil off=false session=nil parked=nil
    화면 F1: sid=nil cmd=nil off=false session=nil parked=nil
    화면 F2: sid=f1 cmd=claude off=false session=f1 parked=nil
    자리 a-wait[waiting]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 a-work[working]: place=tab A2 headless=false shown=tab A2 headless=false parkedTab=nil 조회:
    자리 b-idle[idle]: place=tab B1 headless=false shown=tab B1 headless=false parkedTab=nil 조회:
    자리 c-paused[paused]: place=tab C1 headless=false shown=tab C1 headless=false parkedTab=nil 조회:
    자리 c-idle[idle]: place=tab C2 headless=false shown=tab C2 headless=false parkedTab=nil 조회:
    자리 d-idle[idle]: place=tab D1 headless=false shown=tab D1 headless=false parkedTab=nil 조회:
    자리 d-paused[paused]: place=tab D2 headless=false shown=tab D2 headless=false parkedTab=nil 조회:
    자리 f1[idle]: place=tab F2 headless=false shown=tab F2 headless=false parkedTab=nil 조회:
    자리 fh[working]: place=tab F2 headless=true shown=tab F2 headless=true parkedTab=nil 조회:
    """,
    "10 짝이 바뀌고 풀림": """
    === 10 짝이 바뀌고 풀림 / 판 1
    pair 조회: parkedJob 2001 s1
    끈 세션:
    paired: s1→A1
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude --session-id s1 off=false session=s1 parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    === 10 짝이 바뀌고 풀림 / 판 2
    pair 조회: runsClaude A1
    끈 세션:
    paired:
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→idle
    화면 A1: sid=s1 cmd=nil off=false session=nil parked=nil
    자리 s1[ended]: place=ended shown=ended parkedTab=nil 조회: runsClaude A1 | runsClaude A1
    === 10 짝이 바뀌고 풀림 / 판 3
    pair 조회: runsClaude A1
    끈 세션:
    paired:
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→ended
    화면 A1: sid=s1 cmd=claude --resume s1 off=false session=s1 parked=nil
    자리 s1[ended]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회: runsClaude A1 | runsClaude A1
    === 10 짝이 바뀌고 풀림 / 판 4
    pair 조회: parkedJob 2003 s1
    끈 세션:
    paired: s1→A1
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude --resume s1 off=false session=s1 parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    === 10 짝이 바뀌고 풀림 / 판 5
    pair 조회: parkedJob 2003 s1b
    끈 세션:
    paired: s1b→A1
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1b cmd=claude --resume s1 off=false session=s1b parked=nil
    자리 s1[ended]: place=ended shown=ended parkedTab=nil 조회:
    자리 s1b[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    """,
    "11 tty 를 아직 안 쓴 새 세션": """
    === 11 tty 를 아직 안 쓴 새 세션 / 판 1
    pair 조회: runsClaude A1 | parkedJob 0 n1 | runsClaude A2 | parkedJob 0 n2
    끈 세션:
    paired:
    owned:
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=n1 cmd=claude --session-id n1 off=false session=n1 parked=nil
    화면 A2: sid=n2 cmd=claude -p hi off=false session=n2 parked=nil
    자리 n1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회: runsClaude A1 | runsClaude A1
    자리 n2[working]: place=tab A2 headless=false shown=tab A2 headless=false parkedTab=nil 조회: runsClaude A2 | runsClaude A2
    """,
    "12 앞 프로세스 없음·0·getsid 실패": """
    === 12 앞 프로세스 없음·0·getsid 실패 / 판 1
    pair 조회: parent 3001 | parent 3002 | parent 9999 | parent 3003 | runsClaude A1 | runsClaude A2 | runsClaude A3 | runsClaude A4
    끈 세션:
    paired: h1→A4
    owned: h1
    parkedTabs:
    parkedFrom:
    states: /w/a→idle
    화면 A1: sid=nil cmd=nil off=false session=nil parked=nil
    화면 A2: sid=nil cmd=nil off=false session=nil parked=nil
    화면 A3: sid=nil cmd=nil off=false session=nil parked=nil
    화면 A4: sid=nil cmd=nil off=false session=nil parked=nil
    자리 h1[working]: place=tab A4 headless=true shown=tab A4 headless=true parkedTab=nil 조회:
    자리 h2[working]: place=background shown=background parkedTab=nil 조회:
    자리 h3[working]: place=background shown=background parkedTab=nil 조회:
    """,
    "13 headless 의 tty 는 다른 화면": """
    === 13 headless 의 tty 는 다른 화면 / 판 1
    pair 조회: parent 3001 | parkedJob 2001 s1 | mode h1 | parkedJob 3001 h1
    끈 세션:
    paired: h1→A1 s1→A1
    owned: h1
    parkedTabs:
    parkedFrom:
    states: /w/a→working
    화면 A1: sid=s1 cmd=claude off=false session=s1 parked=nil
    화면 A2: sid=h1 cmd=nil off=false session=h1 parked=nil
    자리 s1[working]: place=tab A1 headless=false shown=tab A1 headless=false parkedTab=nil 조회:
    자리 h1[working]: place=tab A1 headless=true shown=tab A1 headless=true parkedTab=nil 조회:
    """,
]

// MARK: - 공유 규칙 표(rules-table/pairing.json) — RulesTableTests 가 이 파일의 가짜 포트(FakeProbe·FakeProcesses·FakeLive)로 돌린다.
// 가짜 이름이 다른 시험 파일의 private 이름과 겹쳐 internal 로 넓히면 다시 선언 오류가 나므로, 넘겨 주는 함수만 둔다.
// 화면은 표 차례대로 UUID 를 받고 경로는 모두 /w/a, 맨 위 프로세스(leader)는 1000 + 차례, 명령은 없다(짝·자리 판정은 명령을 보지 않는다).
// 세션 pid 는 2000 + 차례(진짜처럼 0 이 아니게), 시각(updated·since)은 t0 기준 초. 넘긴 작업은 없다

private func tablePairing(screens: [(name: String, tty: String, sessionId: String?)], sessions specs: [SessSpec],
                          runsClaude: Set<String>, owned: [String: String])
    -> (pairing: TabPairing, sessions: [Session], probe: FakeProbe, names: [UUID: String]) {
    var ids: [String: UUID] = [:], names: [UUID: String] = [:], leaders: [String: Int32] = [:]
    for (i, s) in screens.enumerated() {
        let id = UUID(uuidString: String(format: "00000000-0000-0000-0000-%012d", i + 1))!
        ids[s.name] = id; names[id] = s.name; leaders[s.name] = Int32(1000 + i)
    }
    var parents: [Int32: Int32] = [:], sessions: [Session] = []
    for (i, spec) in specs.enumerated() {
        var spec = spec
        spec.pid = Int32(2000 + i)
        // 화면 안에서 시작된 headless — 부모가 그 화면의 맨 위 프로세스라 짝짓기의 조상 찾기(tabAncestor)가 한 칸 올라가 만난다
        // (위 표본 "화면 안 headless" 들과 같은 꼴). 다른 세션의 부모는 못 읽음(nil)
        if let screen = owned[spec.id], let leader = leaders[screen] { parents[spec.pid] = leader }
        sessions.append(session(spec))
    }
    let log = Log()
    let probe = FakeProbe(log: log, names: names, running: Set(runsClaude.compactMap { ids[$0] }), modes: [:])
    let tabScreens = screens.map {
        TabScreen(id: ids[$0.name]!, workspace: "/w/a", tty: $0.tty, leader: leaders[$0.name]!, sessionId: $0.sessionId, command: nil, mdRenderOff: false)
    }
    let r = pairTabs(sessions, screens: tabScreens, probe: probe, processes: FakeProcesses(log: log, parents: parents),
                     live: FakeLive(log: log, jobs: [:]))
    return (r, sessions, probe, names)
}

/// screen-session 한 판 — 화면 이름마다 그 화면의 세션(ScreenPairing.session 의 id, 없으면 nil)과 짝 표(세션 id → 화면 이름)
func runScreenSessionRow(screens: [(name: String, tty: String, sessionId: String?)],
                         sessions: [SessSpec]) -> (screenSession: [String: String?], paired: [String: String]) {
    let t = tablePairing(screens: screens, sessions: sessions, runsClaude: [], owned: [:])
    var screenSession: [String: String?] = [:]
    for p in t.pairing.screens { screenSession.updateValue(p.session?.id, forKey: t.names[p.id] ?? "?") }
    return (screenSession, t.pairing.paired.mapValues { t.names[$0] ?? "?" })
}

/// place 한 판 — 짝짓기(pairTabs)의 짝·화면 안 headless·화면의 세션 id 로 세션마다 sessionPlace(앱의 TerminalHub.place 와 같은 값).
/// owned 는 세션 id → 그 headless 세션을 띄운 화면 이름. 결과는 "tab:<화면 이름>"·"terminal"·"background"·"ended"(tab 의 headless 표시는 글자에 없다)
func runPlaceRow(screens: [(name: String, tty: String, sessionId: String?)], sessions: [SessSpec], runsClaude: Set<String>,
                 owned: [String: String]) -> [String: String] {
    let t = tablePairing(screens: screens, sessions: sessions, runsClaude: runsClaude, owned: owned)
    let placeScreens = t.pairing.screens.map { ScreenSession(id: $0.id, sessionId: $0.sessionId) }
    var out: [String: String] = [:]
    for s in t.sessions {
        switch sessionPlace(s, paired: t.pairing.paired, ownedHeadless: t.pairing.ownedHeadless, screens: placeScreens, probe: t.probe) {
        case .tab(let id, _): out[s.id] = "tab:\(t.names[id] ?? "?")"
        case .terminal: out[s.id] = "terminal"
        case .background: out[s.id] = "background"
        case .ended: out[s.id] = "ended"
        }
    }
    return out
}

/// workspace-dot 한 판 — 경로 /w/a 의 화면 차례대로 그 화면의 세션 상태(위 경로 상태 점 표와 같은 dotPairing).
/// 빈 목록은 세션이 없는 셸 화면 하나뿐인 경로다 — 경로는 화면이 있어야 생기므로(화면이 없으면 점을 물을 경로도 없다)
func runWorkspaceDotRow(states: [String]) -> String? {
    if !states.isEmpty { return dotPairing(states).workspaceStates["/w/a"] }
    let log = Log(), id = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
    let r = pairTabs([], screens: [TabScreen(id: id, workspace: "/w/a", tty: "ttys100", leader: 1000, sessionId: nil, command: nil, mdRenderOff: false)],
                     probe: FakeProbe(log: log, names: [id: "A1"], running: [], modes: [:]),
                     processes: FakeProcesses(log: log, parents: [:]), live: FakeLive(log: log, jobs: [:]))
    return r.workspaceStates["/w/a"]
}
