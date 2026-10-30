// 여러 실행 함께 돌리기 규칙(Sources/AutopilotRules/Multirun.swift) 시험 — 2026-10-30 6단계 ⑫-6.
// 사용률 읽을 곳(설치판은 장난감 파일 덮어쓰기를 듣지 않고 시험 빌드는 사용량 API 로 떨어지지 않음), 멈출 선 자리 판정 표,
// 가짜 자리 표(잠금 하나)로 두 엔진이 같은 때 자리를 잡는 경우, 같은 경로 자리(겹침·살아 있는지·남은 자리 치우기·같은 때 두 출발), 판 2 → 3 저장소
import Foundation
import SQLite3
import Testing
import AutopilotRules

private let T = 1_793_000_000.0
private let utc = TimeZone(identifier: "UTC")!
private struct MRClock: APClock { let t: Double; var now: Date { Date(timeIntervalSince1970: t) } }

// MARK: 사용률 읽을 곳

struct UsageStepRow: Row {
    let note: String
    let dev: Bool
    let toy: String?
    let want: [APUsageStep]
}
private let usageStepRows: [UsageStepRow] = [
    .init(note: "설치판은 지금 그대로(stream → 캐시 → API)", dev: false, toy: nil, want: [.stream, .cache, .api]),
    .init(note: "설치판은 장난감 사용량 파일 덮어쓰기를 듣지 않는다", dev: false, toy: "/w/toy-usage.json", want: [.stream, .cache, .api]),
    .init(note: "설치판은 빈 덮어쓰기도 그대로", dev: false, toy: "", want: [.stream, .cache, .api]),
    .init(note: "시험 빌드에 장난감 파일이 있으면 그것을 먼저, stream 은 보조, 캐시·API 는 보지 않는다", dev: true, toy: "/w/toy-usage.json", want: [.toy("/w/toy-usage.json"), .stream]),
    .init(note: "시험 빌드에 장난감 파일이 없으면 stream → 캐시(읽기만), 사용량 API 로 떨어지지 않는다", dev: true, toy: nil, want: [.stream, .cache]),
    .init(note: "시험 빌드의 빈 덮어쓰기는 없는 것", dev: true, toy: "", want: [.stream, .cache]),
]

// MARK: 멈출 선 자리

private func hold(_ run: String, pp: Double, used: Double = 0, since: Double = T - 100, beat: Double = T - 10) -> APSlot {
    APSlot(run: run, project: "zz-" + run, unit: 1, kind: .hold, pp: pp, used: used, pid: 100, since: since, beat: beat)
}
private func wait(_ run: String, pp: Double, since: Double, beat: Double = T - 10) -> APSlot {
    APSlot(run: run, project: "zz-" + run, unit: 2, kind: .wait, pp: pp, used: 0, pid: 101, since: since, beat: beat)
}

struct SlotRow: Row {
    let note: String
    let v7: Double?            // nil 이면 사용률을 모름
    let line: Double
    let slots: [APSlot]
    let want: String           // "go" · "wait:<until-T>:<글 앞부분>"
    let put: String            // 넣는 줄 "<run>:<kind>" 쉼표
    let drop: [String]
}
private let slotRows: [SlotRow] = [
    .init(note: "다른 실행이 없으면 6단계 전과 같이 지나가고 내 자리를 잡는다", v7: 0.25, line: 0.3, slots: [], want: "go", put: "me:hold", drop: []),
    .init(note: "다른 실행이 없고 사용률만으로 닿으면 지금처럼 초기화까지(내 줄 없음)", v7: 0.3, line: 0.3, slots: [], want: "wait:150:계정 주간 사용률 30% 가", put: "", drop: []),
    .init(note: "다른 실행의 자리를 더하면 닿아 기다린다(1분 뒤, 차례 줄)", v7: 0.27, line: 0.3, slots: [hold("a", pp: 5)],
          want: "wait:60:계정 주간 사용률 27% 에 다른 실행 1개가 잡아 둔 자리 5%p 를 더하면 멈출 선 30% 에 닿아", put: "me:wait", drop: []),
    .init(note: "다른 실행의 자리를 더해도 닿지 않으면 시작", v7: 0.2, line: 0.3, slots: [hold("a", pp: 5)], want: "go", put: "me:hold", drop: []),
    .init(note: "그 단위가 이미 쓴 만큼은 사용률에 들어 있어 빼고 더한다", v7: 0.27, line: 0.3, slots: [hold("a", pp: 5, used: 3)], want: "go", put: "me:hold", drop: []),
    .init(note: "다 쓴 자리는 0", v7: 0.29, line: 0.3, slots: [hold("a", pp: 5, used: 9)], want: "go", put: "me:hold", drop: []),
    .init(note: "두 실행의 자리를 함께 더한다", v7: 0.2, line: 0.3, slots: [hold("a", pp: 5), hold("b", pp: 5)],
          want: "wait:60:계정 주간 사용률 20% 에 다른 실행 2개가 잡아 둔 자리 10%p", put: "me:wait", drop: []),
    .init(note: "죽은 엔진의 자리(15분 지남)는 보지 않고 지운다", v7: 0.27, line: 0.3, slots: [hold("a", pp: 5, beat: T - 900)], want: "go", put: "me:hold", drop: ["a"]),
    .init(note: "14분 지난 자리는 아직 산 것", v7: 0.27, line: 0.3, slots: [hold("a", pp: 5, beat: T - 840)], want: "wait:60:계정 주간 사용률 27% 에", put: "me:wait", drop: []),
    .init(note: "먼저 기다리던 실행에 차례를 넘긴다", v7: 0.27, line: 0.3, slots: [wait("a", pp: 5, since: T - 50)],
          want: "wait:60:먼저 기다리던 실행 1개(자리 5%p)에 차례를 넘겨", put: "me:wait", drop: []),
    .init(note: "내가 먼저 기다렸으면 내가 간다", v7: 0.27, line: 0.3, slots: [wait("me", pp: 5, since: T - 90), wait("a", pp: 5, since: T - 50)],
          want: "go", put: "me:hold", drop: []),
    .init(note: "같은 때 기다렸으면 실행 id 차례", v7: 0.27, line: 0.3, slots: [wait("me", pp: 5, since: T - 50), wait("a", pp: 5, since: T - 50)],
          want: "wait:60:먼저 기다리던 실행 1개", put: "me:wait@50", drop: []),
    .init(note: "먼저 기다리던 실행이 있어도 둘 다 들어가면 함께 간다", v7: 0.1, line: 0.3, slots: [wait("a", pp: 5, since: T - 50)], want: "go", put: "me:hold", drop: []),
    .init(note: "기다리던 차례는 그대로(since 를 지킨다)", v7: 0.27, line: 0.3, slots: [hold("a", pp: 5), wait("me", pp: 5, since: T - 300)],
          want: "wait:60:계정 주간 사용률 27% 에 다른 실행 1개", put: "me:wait@300", drop: []),
    .init(note: "계정 사용률만으로 닿으면 내 기다림 줄을 지운다(모두 초기화까지)", v7: 0.31, line: 0.3, slots: [hold("a", pp: 5), wait("me", pp: 5, since: T - 300)],
          want: "wait:150:계정 주간 사용률 31% 가", put: "", drop: ["me"]),
    .init(note: "사용률을 모르면 10분 뒤(지금처럼)", v7: nil, line: 0.3, slots: [hold("a", pp: 5)], want: "wait:600:계정 사용률을 알 수 없어", put: "", drop: []),
    .init(note: "소수 자리 글자", v7: 0.2, line: 0.3, slots: [hold("a", pp: 12.5, used: 2.4)], want: "wait:60:계정 주간 사용률 20% 에 다른 실행 1개가 잡아 둔 자리 10.1%p", put: "me:wait", drop: []),
]

// 잠금 하나로 줄을 지키는 가짜 자리 표 — 엔진의 BEGIN IMMEDIATE 처럼 update 는 한 번에 하나
private final class FakeSlotBook: APSlotBook, @unchecked Sendable {
    private let lock = NSLock()
    var rows: [APSlot]
    var fail = false
    init(_ rows: [APSlot] = []) { self.rows = rows }
    func update(_ body: ([APSlot]) -> APSlotChange) throws {
        if fail { throw NSError(domain: "zz", code: 5, userInfo: [NSLocalizedDescriptionKey: "database is locked"]) }
        lock.lock(); defer { lock.unlock() }
        let c = body(rows)
        rows.removeAll { c.drop.contains($0.run) }
        for p in c.put { rows.removeAll { $0.run == p.run }; rows.append(p) }
    }
}

// MARK: 같은 경로

struct OverlapRow: Row {
    let note: String
    let a: String
    let b: String
    let want: Bool
}
private let overlapRows: [OverlapRow] = [
    .init(note: "같은 저장소", a: "/w/zz-toy-a", b: "/w/zz-toy-a", want: true),
    .init(note: "다른 저장소", a: "/w/zz-toy-a", b: "/w/zz-toy-b", want: false),
    .init(note: "이름 앞부분만 같으면 겹치지 않는다", a: "/w/zz-toy", b: "/w/zz-toy-b", want: false),
    .init(note: "저장소를 품은 폴더", a: "/w/shop", b: "/w/shop/shop-core", want: true),
    .init(note: "품은 쪽이 뒤", a: "/w/shop/shop-web", b: "/w/shop", want: true),
    .init(note: "홈 폴더는 품은 자리로 보지 않는다", a: "/users/zz", b: "/users/zz/w/zz-toy-a", want: false),
    .init(note: "홈 폴더끼리는 같다", a: "/users/zz", b: "/users/zz", want: true),
]

private final class FakeClaimBook: APClaimBook, @unchecked Sendable {
    private let lock = NSLock()
    var rows: [APClaim]
    init(_ rows: [APClaim] = []) { self.rows = rows }
    func update(_ body: ([APClaim]) -> (put: [APClaim], drop: [String])) throws {
        lock.lock(); defer { lock.unlock() }
        let c = body(rows)
        rows.removeAll { c.drop.contains($0.run) }
        for p in c.put {
            if rows.contains(where: { $0.key == p.key }) { throw NSError(domain: "zz", code: 19, userInfo: [NSLocalizedDescriptionKey: "UNIQUE constraint failed"]) }
            rows.append(p)
        }
    }
}
private struct FakeProbe: APClaimProbe {
    let views: [String: APClaimRunView]
    func view(_ c: APClaim) -> APClaimRunView { views[c.run] ?? APClaimRunView(exists: false, state: nil, wrapped: false, engineAlive: false, jobLoaded: false, starterAlive: false) }
}
private func claim(_ run: String, _ path: String) -> APClaim {
    APClaim(key: apPathKey(path), path: path, run: run, runDir: "/r/runs/" + run, label: "com.claude.board.dev.autopilot." + run, pid: 4242, claimed: T - 60)
}

struct AliveRow: Row {
    let note: String
    let view: APClaimRunView
    let want: Bool
}
private func v(_ state: String?, exists: Bool = true, wrapped: Bool = false, engine: Bool = false, job: Bool = false, starter: Bool = false) -> APClaimRunView {
    APClaimRunView(exists: exists, state: state, wrapped: wrapped, engineAlive: engine, jobLoaded: job, starterAlive: starter)
}
private let aliveRows: [AliveRow] = [
    .init(note: "실행 폴더가 없으면 남은 자리", view: v("running", exists: false, engine: true), want: false),
    .init(note: "출발 명령이 상태를 쓰기 전: 그 명령이 도는 동안 산 것", view: v(nil, starter: true), want: true),
    .init(note: "출발 명령이 상태를 쓰지 못하고 끝났으면 남은 자리", view: v(nil), want: false),
    .init(note: "starting: LaunchAgent 가 실렸으면 산 것", view: v("starting", job: true), want: true),
    .init(note: "starting: 출발 명령이 아직 돌면 산 것", view: v("starting", starter: true), want: true),
    .init(note: "도는 실행: 엔진이 살아 있음", view: v("running", engine: true), want: true),
    .init(note: "도는 실행: 엔진이 죽어도 LaunchAgent 가 다시 띄움", view: v("waiting_stopline", job: true), want: true),
    .init(note: "도는 상태인데 엔진도 LaunchAgent 도 없으면 남은 자리", view: v("running"), want: false),
    .init(note: "일시정지도 엔진이 살아 있으면 산 것", view: v("paused", engine: true, job: true), want: true),
    .init(note: "끝났지만 정리 중(가지 되돌리기)은 엔진이 살아 있으면 산 것", view: v("done", engine: true), want: true),
    .init(note: "끝나고 정리를 마쳤으면 남은 자리", view: v("stopped", wrapped: true, engine: true, job: true), want: false),
    .init(note: "끝났고 엔진이 죽었으면 남은 자리", view: v("error"), want: false),
]

@Suite("자율주행 여러 실행 함께 돌리기") struct MultirunTests {
    @Test("사용률 읽을 곳", arguments: usageStepRows)
    func 사용률(_ r: UsageStepRow) { #expect(apUsageSteps(dev: r.dev, toyPath: r.toy) == r.want) }

    @Test("잡을 자리와 글자")
    func 자리크기() {
        #expect(apSlotSize(estPP: nil, capPP: 5) == 5)
        #expect(apSlotSize(estPP: 2.5, capPP: 5) == 2.5)
        #expect(apSlotSize(estPP: 9, capPP: 5) == 5)       // 단위 상한을 넘지 않는다
        #expect(apSlotSize(estPP: 0, capPP: 5) == 5)       // 추정 0 은 없는 것
        #expect(apSlotLeft(hold("a", pp: 5, used: 7)) == 0)
        #expect(apPPText(5) == "5" && apPPText(2.46) == "2.5" && apPPText(0.04) == "0")
    }

    @Test("멈출 선 자리 판정", arguments: slotRows)
    func 판정(_ r: SlotRow) {
        let usage: [String: Any]? = r.v7.map { ["seven_day": $0, "seven_day_resets": T + 30, "five_hour": 0.1] }
        let g = apSlotDecide(me: "me", project: "zz-me", unit: 3, pp: 5, pid: 7, usage: usage, stopline: ["seven_day": r.line], slots: r.slots, now: T, tz: utc)
        let got = g.wait.map { "wait:\(Int($0.until - T)):" + $0.reason } ?? "go"
        if r.want == "go" { #expect(got == "go") } else { #expect(got.hasPrefix(r.want), "\(got)") }
        let put = g.change.put.map { p in "\(p.run):\(p.kind.rawValue)" + (p.kind == .wait && p.since != T ? "@\(Int(T - p.since))" : "") }.joined(separator: ",")
        #expect(put == r.put)
        #expect(g.change.drop == r.drop)
        if let p = g.change.put.first { #expect(p.unit == 3 && p.pp == 5 && p.beat == T && p.pid == 7) }
        #expect(!(g.wait?.reason.contains("—") ?? false))
        #expect(g.byOthers == (r.want.hasPrefix("wait:60:")))   // 다른 실행 때문에 기다릴 때만(1분마다 다시 봄)
    }

    @Test("두 엔진이 같은 때 자리를 잡으면 하나만 시작하고 다른 하나는 기다린다(잠금 하나)")
    func 같은때() async {
        for _ in 0..<20 {
            let book = FakeSlotBook()
            let results = await withTaskGroup(of: (String, Bool).self) { g -> [(String, Bool)] in
                for me in ["a", "b"] {
                    g.addTask {
                        let usage: [String: Any] = ["seven_day": 0.27, "seven_day_resets": T + 3600]
                        let r = apTakeSlot(me: me, project: nil, unit: 1, pp: 5, pid: nil, usage: usage, stopline: ["seven_day": 0.3], book: book, clock: MRClock(t: T), tz: utc)
                        return (me, r.wait == nil)
                    }
                }
                var out: [(String, Bool)] = []
                for await x in g { out.append(x) }
                return out
            }
            #expect(results.filter(\.1).count == 1)
            #expect(book.rows.filter { $0.kind == .hold }.count == 1 && book.rows.filter { $0.kind == .wait }.count == 1)
        }
    }

    @Test("자리를 풀면 기다리던 실행이 시작하고, 앞 실행의 다음 단위는 차례를 넘긴다(번갈아)")
    func 번갈아() {
        let book = FakeSlotBook()
        let usage: [String: Any] = ["seven_day": 0.27, "seven_day_resets": T + 9000]
        let line: [String: Any] = ["seven_day": 0.3]
        func take(_ me: String, _ t: Double) -> Bool {
            apTakeSlot(me: me, project: nil, unit: 1, pp: 5, pid: nil, usage: usage, stopline: line, book: book, clock: MRClock(t: t), tz: utc).wait == nil
        }
        #expect(take("a", T))
        #expect(!take("b", T + 1))                      // a 의 자리 때문에 기다림
        #expect(throws: Never.self) { try apReleaseSlot(me: "a", book: book) }   // a 의 단위 끝
        #expect(!take("a", T + 2))                      // b 가 먼저 기다렸다
        #expect(take("b", T + 61))                      // b 가 다시 보면 시작
        #expect(throws: Never.self) { try apReleaseSlot(me: "b", book: book) }
        #expect(!take("b", T + 70))                     // 이번엔 a 가 먼저 기다렸다
        #expect(take("a", T + 62 + 60))
    }

    @Test("잠긴 저장소를 다시 하는 간격")
    func 다시하기() {
        #expect((1...apBusyRetries).compactMap(apBusyRetryDelay) == [0.1, 0.2, 0.4, 0.8, 1, 1, 1, 1])
        #expect(apBusyRetryDelay(0) == nil && apBusyRetryDelay(apBusyRetries + 1) == nil)
    }

    @Test("자리 다시 적기·풀기·표를 못 쓰면 사용률만으로")
    func 다시적기() throws {
        let book = FakeSlotBook([hold("old", pp: 5, beat: T - 2000)])
        try apBeatSlot(me: "me", project: "zz", unit: 2, pp: 4, used: 1.5, pid: 9, book: book, clock: MRClock(t: T))
        #expect(book.rows.map(\.run) == ["me"])           // 죽은 엔진의 자리를 함께 지움
        #expect(book.rows.first?.used == 1.5 && book.rows.first?.kind == .hold)
        try apBeatSlot(me: "me", project: "zz", unit: 2, pp: 4, used: 2, pid: 9, book: book, clock: MRClock(t: T + 60))
        #expect(book.rows.first?.since == T && book.rows.first?.beat == T + 60)   // 잡은 때는 그대로
        try apReleaseSlot(me: "me", book: book)
        #expect(book.rows.isEmpty)
        book.fail = true
        let r = apTakeSlot(me: "me", project: nil, unit: 1, pp: 5, pid: nil, usage: ["seven_day": 0.1], stopline: [:], book: book, clock: MRClock(t: T), tz: utc)
        #expect(r.wait == nil && r.note?.contains("사용률만으로") == true)
    }

    @Test("경로가 겹치는가", arguments: overlapRows)
    func 겹침(_ r: OverlapRow) { #expect(apPathOverlap(apPathKey(r.a), apPathKey(r.b), home: "/Users/zz") == r.want) }

    @Test("자리 줄은 접은 꼴로(대소문자·유니코드 정규화)")
    func 접기() {
        let nfd = "/w/zz-토이".decomposedStringWithCanonicalMapping
        let k = apClaimKeys(["/w/ZZ-Toy-A", "/w/zz-toy-a/", nfd, "/w/zz-토이", "rel/x", ""])
        #expect(k.map(\.key) == ["/w/zz-toy-a", "/w/zz-토이".precomposedStringWithCanonicalMapping])
        #expect(k.first?.path == "/w/ZZ-Toy-A")
    }

    @Test("자리를 잡은 실행이 살아 있는가", arguments: aliveRows)
    func 살아있나(_ r: AliveRow) { #expect(apClaimAlive(r.view) == r.want) }

    @Test("같은 경로의 두 번째 출발은 거부, 남은 자리는 치우고 잡는다, 저장소 묶음은 하나라도 겹치면 거부")
    func 출발() {
        let home = "/Users/zz"
        let book = FakeClaimBook([claim("a", "/w/zz-toy-a"), claim("dead", "/w/zz-toy-b"), claim("shop", "/w/shop/shop-core"), claim("shop", "/w/shop/shop-web")])
        let probe = FakeProbe(views: ["a": v("running", engine: true), "dead": v("running"), "shop": v("waiting_limit", job: true)])
        let c = MRClock(t: T)
        // 대소문자를 바꿔 적은 같은 경로
        let r1 = apClaimPaths(run: "x", runDir: "/r/runs/x", label: "l", pid: 1, paths: ["/w/ZZ-TOY-A"], home: home, book: book, probe: probe, clock: c)
        #expect(r1 == .busy(run: "a", path: "/w/zz-toy-a", mine: "/w/ZZ-TOY-A"))
        #expect(apClaimBusyText(r1, home: home) == "이 경로에서 자율주행이 돌고 있습니다(a, /w/zz-toy-a). 그 실행이 끝난 뒤에 출발합니다")
        #expect(!book.rows.contains { $0.run == "x" })
        // 엔진도 LaunchAgent 도 없는 실행의 남은 자리는 치우고 잡는다
        let r2 = apClaimPaths(run: "y", runDir: "/r/runs/y", label: "l", pid: 2, paths: ["/w/zz-toy-b"], home: home, book: book, probe: probe, clock: c)
        #expect(r2 == .ok(cleared: ["dead"]))
        #expect(book.rows.filter { $0.run == "y" }.map(\.path) == ["/w/zz-toy-b"] && !book.rows.contains { $0.run == "dead" })
        // 묶음의 저장소 하나만 겹쳐도 거부, 묶음을 품은 폴더도 거부
        let r3 = apClaimPaths(run: "z", runDir: "/r/runs/z", label: "l", pid: 3, paths: ["/w/zz-toy-c", "/w/shop/shop-web"], home: home, book: book, probe: probe, clock: c)
        #expect(r3 == .busy(run: "shop", path: "/w/shop/shop-web", mine: "/w/shop/shop-web"))
        let r4 = apClaimPaths(run: "z", runDir: "/r/runs/z", label: "l", pid: 3, paths: ["/w/shop"], home: home, book: book, probe: probe, clock: c)
        if case .busy(let other, _, _) = r4 { #expect(other == "shop") } else { Issue.record("품은 폴더를 받았다: \(r4)") }
        // 겹치지 않는 경로는 함께
        let r5 = apClaimPaths(run: "z", runDir: "/r/runs/z", label: "l", pid: 3, paths: ["/w/zz-toy-c"], home: home, book: book, probe: probe, clock: c)
        #expect(r5 == .ok(cleared: []))
        // 끝 정리를 마치면 푼다
        #expect(throws: Never.self) { try apReleaseClaims(run: "a", book: book) }
        let r6 = apClaimPaths(run: "x", runDir: "/r/runs/x", label: "l", pid: 1, paths: ["/w/zz-toy-a"], home: home, book: book, probe: probe, clock: c)
        #expect(r6 == .ok(cleared: []))
    }

    @Test("거의 같은 때 두 출발이 같은 경로를 잡으면 하나만 지나간다(앞 출발 명령이 도는 중)")
    func 같은때출발() async {
        for _ in 0..<20 {
            let book = FakeClaimBook()
            // 상태를 아직 쓰지 않은 실행 — 출발 명령이 도는 중이라 산 것
            let probe = FakeProbe(views: ["p": v(nil, starter: true), "q": v(nil, starter: true)])
            let got = await withTaskGroup(of: APClaimResult.self) { g -> [APClaimResult] in
                for id in ["p", "q"] {
                    g.addTask { apClaimPaths(run: id, runDir: "/r/runs/" + id, label: "l", pid: 5, paths: ["/w/zz-toy-a"], home: "/Users/zz", book: book, probe: probe, clock: MRClock(t: T)) }
                }
                var out: [APClaimResult] = []
                for await x in g { out.append(x) }
                return out
            }
            #expect(got.filter { if case .ok = $0 { return true } else { return false } }.count == 1)
            #expect(got.filter { if case .busy = $0 { return true } else { return false } }.count == 1)
            #expect(book.rows.count == 1)
        }
    }
}

@Suite("자율주행 실행 기록 저장소 판 3") struct RecordsV3Tests {
    private func exec(_ db: OpaquePointer?, _ sql: String) -> Int32 { sqlite3_exec(db, sql, nil, nil, nil) }
    private func one(_ db: OpaquePointer?, _ sql: String) -> String? {
        var st: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &st, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(st) }
        guard sqlite3_step(st) == SQLITE_ROW, let c = sqlite3_column_text(st, 0) else { return nil }
        return String(cString: c)
    }

    @Test("판 2 저장소를 판 3 으로 올리면 행은 그대로이고 자리 표 둘이 생긴다. 같은 경로 자리는 한 줄만")
    func 판3() {
        var db: OpaquePointer?
        #expect(sqlite3_open(":memory:", &db) == SQLITE_OK)
        for s in apRecordsSteps(from: 0) where s.version <= 2 { #expect(exec(db, s.sql) == SQLITE_OK) }
        #expect(exec(db, "PRAGMA user_version=2; INSERT INTO runs(id,state) VALUES('r1','done'); INSERT INTO units(run_id,n,status) VALUES('r1',1,'done')") == SQLITE_OK)
        #expect(apRecordsSteps(from: 2).map(\.version).first == 3)   // 판 4(⑫-7a)는 RecordsV4Tests
        for s in apRecordsSteps(from: 2) where s.version <= 3 { #expect(exec(db, s.sql) == SQLITE_OK); #expect(exec(db, "PRAGMA user_version=\(s.version)") == SQLITE_OK) }
        #expect(one(db, "PRAGMA user_version") == "3")
        #expect(one(db, "SELECT COUNT(*) FROM units") == "1")
        #expect(exec(db, "INSERT INTO path_claims(key,path,run_id,claimed) VALUES('/w/zz-toy-a','/w/zz-toy-a','r2',1)") == SQLITE_OK)
        #expect(exec(db, "INSERT INTO path_claims(key,path,run_id,claimed) VALUES('/w/zz-toy-a','/w/ZZ-TOY-A','r3',2)") != SQLITE_OK)   // 같은 접은 경로는 하나만
        #expect(exec(db, "INSERT INTO stopline_slots(run_id,kind,pp,used,since,beat) VALUES('r2','hold',5,0,1,1)") == SQLITE_OK)
        #expect(exec(db, "INSERT INTO stopline_slots(run_id,kind,pp,used,since,beat) VALUES('r2','wait',5,0,1,1)") != SQLITE_OK)   // 실행마다 한 줄
        sqlite3_close(db)
    }
}
