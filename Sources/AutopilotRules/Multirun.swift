// 자율주행 규칙(AutopilotRules) — 여러 실행 함께 돌리기(설계 6장 6단계, 2026-10-30 ⑫-6 에서 새로 씀).
// ① 사용률을 어디서 읽는가(apUsageSteps: 설치판은 지금 그대로, 시험 빌드는 사용량 API 로 떨어지지 않음·두 엔진이 함께 읽는 장난감 사용량 파일)
// ② 계정 멈출 선을 실행들이 함께 봄: 새 단위를 시작하기 전에 기록 저장소의 자리 표(stopline_slots, 판 3)에서 자리를 잡는다(apTakeSlot).
//    지금 사용률에 다른 실행이 잡아 둔 자리(단위 추정 %p 가운데 아직 쓰지 않은 만큼)를 더해 그 실행의 멈출 선과 견준다. 먼저 기다리던 실행에 차례를 넘긴다
// ③ 같은 경로의 두 번째 출발 거부: 출발할 때 기록 저장소의 경로 자리 표(path_claims, 판 3)에 저장소 경로를 잡는다(apClaimPaths).
//    경로는 실제 경로로 푼 뒤(어댑터) 조각마다 NFC·소문자로 접어(apPathKey, ⑪-3) 같거나 한쪽이 다른 쪽 안이면 겹친다. 하나라도 겹치고 그 실행이 살아 있으면 거부
// 표 읽기·쓰기는 포트(APSlotBook·APClaimBook: 엔진의 SQLite BEGIN IMMEDIATE 한 transaction), 실행이 살아 있는지는 포트(APClaimProbe)로만 본다
import Foundation

// MARK: - ① 사용률 읽을 곳

// 사용률을 읽는 걸음 — 엔진이 차례로 보고 처음 값이 나온 것을 쓴다
public enum APUsageStep: Equatable, Sendable {
    case toy(String)   // 시험 빌드만: 장난감 사용량 파일(캐시와 같은 퍼센트 꼴, 두 엔진이 함께 읽음). 쓴 시각을 따지지 않는다
    case stream        // 이 엔진의 run.json usage(stream rate_limit_event·앞서 읽은 값)가 15분 안이면
    case cache         // 보드·상태줄이 함께 쓰는 /tmp/.claude-usage-cache.json 이 15분 안에 쓰였으면(읽기만)
    case api           // 키체인 토큰으로 사용량 API(받으면 캐시에 씀) — 설치판만
}

// 설치판(dev false)은 지금 그대로(stream → 캐시 → API)이고 장난감 파일 덮어쓰기를 듣지 않는다.
// 시험 빌드는 사용량 API 로 떨어지지 않는다: 장난감 파일이 있으면 그것 → stream(한 실행 안의 보조), 없으면 stream → 캐시(읽기만)
public func apUsageSteps(dev: Bool, toyPath: String?) -> [APUsageStep] {
    guard dev else { return [.stream, .cache, .api] }
    if let p = toyPath, !p.isEmpty { return [.toy(p), .stream] }
    return [.stream, .cache]
}

// MARK: - ② 멈출 선 자리

public enum APSlotKind: String, Equatable, Sendable { case hold, wait }   // 자리를 잡고 단위가 도는 중 · 자리가 없어 기다리는 중(차례)

// 자리 표 한 줄(실행마다 하나) — pp·used 는 주간 %p
public struct APSlot: Equatable, Sendable {
    public var run: String
    public var project: String?
    public var unit: Int
    public var kind: APSlotKind
    public var pp: Double          // 잡은 자리(단위 추정 %p, apSlotSize)
    public var used: Double        // 그 단위가 지금까지 쓴 주간 %p(엔진이 1분마다 적음)
    public var pid: Int32?
    public var since: Double       // 잡은 때(기다림이면 기다리기 시작한 때 — 차례)
    public var beat: Double        // 엔진이 마지막으로 적은 때. apSlotTTL 이 지나면 죽은 엔진의 자리로 보고 푼다
    public init(run: String, project: String?, unit: Int, kind: APSlotKind, pp: Double, used: Double, pid: Int32?, since: Double, beat: Double) {
        self.run = run; self.project = project; self.unit = unit; self.kind = kind; self.pp = pp; self.used = used; self.pid = pid; self.since = since; self.beat = beat
    }
}

// 엔진이 죽어 남은 자리가 풀리는 때 — 도는 엔진은 1분마다(pollControl) 자리를 다시 적는다. 사용률을 믿는 15분과 같게
public let apSlotTTL: Double = 900
// 자리 때문에 기다릴 때 다시 보는 간격(초) — 다른 실행의 단위가 끝나 자리가 풀리면 곧 시작한다
public let apSlotRecheck: Double = 60
public func apSlotLive(_ s: APSlot, now: Double) -> Bool { now - s.beat < apSlotTTL && s.beat <= now + 60 }

// 잡을 자리 — 단위 추정 %p(출발 확인이 넣은 est_pp, 보정한 추정)가 있으면 그것(단위 상한을 넘지 않게), 없으면 단위 주간 %p 상한(기본 5)
public func apSlotSize(estPP: Double?, capPP: Double) -> Double {
    if let e = estPP, e > 0 { return min(e, capPP) }
    return capPP
}
// 아직 쓰지 않은 자리 — 지금 사용률에 이미 든 만큼(used)은 빼고 더한다
public func apSlotLeft(_ s: APSlot) -> Double { max(s.pp - s.used, 0) }

// %p 글자 — 정수면 "5", 아니면 소수 한 자리 "2.5"
public func apPPText(_ v: Double) -> String {
    let r = (v * 10).rounded() / 10
    return r == r.rounded() ? String(Int(r)) : String(format: "%.1f", r)
}

// 자리 표 바꿈 — 넣거나 바꿀 줄(실행마다 하나)과 지울 실행
public struct APSlotChange: Equatable, Sendable {
    public var put: [APSlot] = []
    public var drop: [String] = []
    public init(put: [APSlot] = [], drop: [String] = []) { self.put = put; self.drop = drop }
}

// 자리 표 판정 한 번 — 기다리면 그 글과 다시 볼 때, 바꿈은 같은 transaction 에서 쓴다
public struct APSlotGate: Equatable, Sendable {
    public var wait: APStoplineWait?
    public var change: APSlotChange
    public var others: Int          // 견준 다른 실행의 자리 수(hold)
    public var held: Double         // 그 자리의 아직 쓰지 않은 %p 합
    public var byOthers = false     // 다른 실행의 자리·차례 때문에 기다림(1분마다 다시 봄 — 엔진은 상태를 그대로 두고 진행 기록도 한 번만)
}

// 새 단위를 시작해도 되는가(자리 표를 함께 봄). 1) 계정 사용률만으로 멈출 선에 닿거나 모르면 지금처럼(apStoplineWait) 기다리고 내 기다림 줄도 지운다(모두 초기화까지 기다림).
// 2) 지금 주간 사용률 + 다른 실행이 잡아 둔 자리(쓰지 않은 만큼)가 내 멈출 선에 닿으면 기다리고 기다림 줄(차례)을 남긴다.
// 3) 나보다 먼저 기다리던 실행이 있고 그 자리까지 더하면 닿으면 차례를 넘기고 기다린다(한 실행이 단위를 이어 잡아 다른 실행이 굶지 않게).
// 4) 아니면 내 자리(hold)를 잡는다. 죽은 엔진의 자리(apSlotTTL 지남)는 늘 지운다. 다른 실행이 없으면 1) 과 같다(6단계 전 동작)
public func apSlotDecide(me: String, project: String?, unit: Int, pp: Double, pid: Int32?, usage: [String: Any]?, stopline: [String: Any],
                         slots: [APSlot], now t: Double, tz: TimeZone) -> APSlotGate {
    var change = APSlotChange(drop: slots.filter { !apSlotLive($0, now: t) && $0.run != me }.map(\.run))
    let live = slots.filter { $0.run != me && apSlotLive($0, now: t) }
    let holds = live.filter { $0.kind == .hold }
    let held = holds.reduce(0) { $0 + apSlotLeft($1) }
    let mine = slots.first { $0.run == me }
    if let w = apStoplineWait(usage: usage, stopline: stopline, now: t, tz: tz) {
        if mine != nil { change.drop.append(me) }
        return APSlotGate(wait: w, change: change, others: holds.count, held: held)
    }
    let l7 = apStopNum(stopline["seven_day"]) ?? 0.80
    let r7 = apStopNum(usage?["seven_day_resets"]) ?? 0
    let v7 = r7 > 0 && r7 <= t ? 0 : apStopNum(usage?["seven_day"]) ?? 0
    let since = mine?.kind == .wait ? mine!.since : t
    let waitRow = APSlot(run: me, project: project, unit: unit, kind: .wait, pp: pp, used: 0, pid: pid, since: since, beat: t)
    let pct = { (x: Double) in "\(Int((x * 100).rounded()))%" }
    if v7 + held / 100 >= l7 - 1e-9 {
        change.put.append(waitRow)
        let reason = "계정 주간 사용률 \(pct(v7)) 에 다른 실행 \(holds.count)개가 잡아 둔 자리 \(apPPText(held))%p 를 더하면 멈출 선 \(pct(l7)) 에 닿아, 새 단위를 시작하지 않고 그 단위가 끝나기를 기다립니다(1분마다 다시 봅니다)."
        return APSlotGate(wait: APStoplineWait(until: t + apSlotRecheck, reason: reason), change: change, others: holds.count, held: held, byOthers: true)
    }
    let ahead = live.filter { $0.kind == .wait && ($0.since, $0.run) < (since, me) }
    let aheadPP = ahead.reduce(0) { $0 + $1.pp }
    if !ahead.isEmpty, v7 + (held + aheadPP) / 100 >= l7 - 1e-9 {
        change.put.append(waitRow)
        let reason = "먼저 기다리던 실행 \(ahead.count)개(자리 \(apPPText(aheadPP))%p)에 차례를 넘겨, 새 단위를 시작하지 않고 기다립니다(1분마다 다시 봅니다)."
        return APSlotGate(wait: APStoplineWait(until: t + apSlotRecheck, reason: reason), change: change, others: holds.count, held: held, byOthers: true)
    }
    change.put.append(APSlot(run: me, project: project, unit: unit, kind: .hold, pp: pp, used: 0, pid: pid, since: t, beat: t))
    return APSlotGate(wait: nil, change: change, others: holds.count, held: held)
}

// 자리 표 — 다른 엔진과 한 번에 하나(엔진: 기록 저장소의 BEGIN IMMEDIATE 한 transaction). body 가 지금 줄을 보고 돌려준 바꿈을 같은 잠금 안에서 쓴다
public protocol APSlotBook {
    func update(_ body: ([APSlot]) -> APSlotChange) throws
}

// 자리 잡기 결과 — 시작(go)·기다림, 자리 표를 쓰지 못했으면 사용률만으로 정한 결과와 까닭(note)
public struct APSlotTake: Equatable, Sendable {
    public var wait: APStoplineWait?
    public var others: Int
    public var held: Double
    public var note: String?
    public var byOthers = false
}

// 유스케이스: 새 단위 앞에서 자리를 잡는다. 표를 쓰지 못하면(저장소 잠김·깨짐) 6단계 전처럼 사용률만으로 정하고 까닭을 남긴다(실행은 멈추지 않음)
public func apTakeSlot(me: String, project: String?, unit: Int, pp: Double, pid: Int32?, usage: [String: Any]?, stopline: [String: Any],
                       book: APSlotBook, clock: APClock, tz: TimeZone) -> APSlotTake {
    let t = clock.now.timeIntervalSince1970
    var gate: APSlotGate?
    do {
        try book.update { slots in
            let g = apSlotDecide(me: me, project: project, unit: unit, pp: pp, pid: pid, usage: usage, stopline: stopline, slots: slots, now: t, tz: tz)
            gate = g
            return g.change
        }
    } catch {
        let w = apStoplineWait(usage: usage, stopline: stopline, now: t, tz: tz)
        return APSlotTake(wait: w, others: 0, held: 0, note: "멈출 선 자리 표를 쓰지 못해(\(error)) 다른 실행의 자리 없이 사용률만으로 정했습니다.")
    }
    guard let g = gate else { return APSlotTake(wait: apStoplineWait(usage: usage, stopline: stopline, now: t, tz: tz), others: 0, held: 0, note: nil) }
    return APSlotTake(wait: g.wait, others: g.others, held: g.held, note: nil, byOthers: g.byOthers)
}

// 유스케이스: 도는 단위의 자리를 다시 적는다(1분마다, 다시 뜬 엔진이 이어 가는 단위도 이것으로 자리를 되찾는다). 죽은 엔진의 자리도 함께 지운다
public func apBeatSlot(me: String, project: String?, unit: Int, pp: Double, used: Double, pid: Int32?, book: APSlotBook, clock: APClock) throws {
    let t = clock.now.timeIntervalSince1970
    try book.update { slots in
        var c = APSlotChange(drop: slots.filter { !apSlotLive($0, now: t) && $0.run != me }.map(\.run))
        let since = slots.first { $0.run == me && $0.kind == .hold }?.since ?? t
        c.put.append(APSlot(run: me, project: project, unit: unit, kind: .hold, pp: pp, used: used, pid: pid, since: since, beat: t))
        return c
    }
}

// 유스케이스: 자리를 푼다(단위 끝·일시정지·실행 끝)
public func apReleaseSlot(me: String, book: APSlotBook) throws {
    try book.update { slots in slots.contains { $0.run == me } ? APSlotChange(drop: [me]) : APSlotChange() }
}

// MARK: - ③ 같은 경로 자리

// 경로 자리 한 줄(저장소 경로마다) — key 는 접은 경로(apPathKey), path 는 실제 경로
public struct APClaim: Equatable, Sendable {
    public var key: String
    public var path: String
    public var run: String
    public var runDir: String
    public var label: String
    public var pid: Int32?          // 자리를 잡은 출발 명령(start)의 pid — 출발이 끝나기 전에 그 실행이 살아 있는지 보려고
    public var claimed: Double
    public init(key: String, path: String, run: String, runDir: String, label: String, pid: Int32?, claimed: Double) {
        self.key = key; self.path = path; self.run = run; self.runDir = runDir; self.label = label; self.pid = pid; self.claimed = claimed
    }
}

// 자리를 잡은 실행의 지금 — 포트가 실행 폴더의 run.json·엔진 pid·LaunchAgent·출발 명령 pid 를 보고 채운다
public struct APClaimRunView: Equatable, Sendable {
    public var exists: Bool          // 실행 폴더의 run.json 을 읽었나
    public var state: String?        // run.json state(출발 명령이 쓰기 전에는 nil)
    public var wrapped: Bool         // 끝 정리를 마쳤나
    public var engineAlive: Bool     // run.json engine_pid 가 살아 있고 그 실행의 엔진인가
    public var jobLoaded: Bool       // 그 LaunchAgent 가 launchd 에 실려 있나(죽어도 다시 띄움)
    public var starterAlive: Bool    // 자리를 잡은 출발 명령이 아직 도는가
    public init(exists: Bool, state: String?, wrapped: Bool, engineAlive: Bool, jobLoaded: Bool, starterAlive: Bool) {
        self.exists = exists; self.state = state; self.wrapped = wrapped; self.engineAlive = engineAlive; self.jobLoaded = jobLoaded; self.starterAlive = starterAlive
    }
}

public protocol APClaimProbe {
    func view(_ c: APClaim) -> APClaimRunView
}

// 그 자리를 아직 쓰는가. 실행 폴더가 없으면 아님. 출발 명령이 아직 상태를 쓰지 않았으면 그 명령이 도는 동안만.
// 끝났고 정리도 마쳤으면 아님. 끝났지만 정리 중(가지 되돌리기)이거나 도는 상태면 엔진이 살아 있거나 LaunchAgent 가 실려 있을 때만(엔진이 죽어 LaunchAgent 도 없으면 남은 자리)
public func apClaimAlive(_ v: APClaimRunView) -> Bool {
    guard v.exists else { return false }
    guard let s = v.state, !s.isEmpty else { return v.starterAlive }
    if apRunEnded(s), v.wrapped { return false }
    return v.engineAlive || v.jobLoaded || (s == "starting" && v.starterAlive)
}

// 두 접은 경로가 겹치는가 — 같거나 한쪽이 다른 쪽 안(조각 단위). 홈 폴더(접은 꼴)는 같을 때만(홈을 품은 자리로 보지 않음)
public func apPathOverlap(_ a: String, _ b: String, home: String) -> Bool {
    if a == b { return true }
    let h = apPathKey(home)
    if a == h || b == h { return false }
    return a.hasPrefix(b == "/" ? "/" : b + "/") || b.hasPrefix(a == "/" ? "/" : a + "/")
}

// 실제 경로 목록 → 자리 줄(접은 꼴로 겹친 것은 하나로). 빈 글자·상대 경로는 뺀다
public func apClaimKeys(_ paths: [String]) -> [(key: String, path: String)] {
    var out: [(key: String, path: String)] = []
    for p in paths where p.hasPrefix("/") {
        let k = apPathKey(p.count > 1 && p.hasSuffix("/") ? String(p.dropLast()) : p)
        if !out.contains(where: { $0.key == k }) { out.append((k, p)) }
    }
    return out
}

public protocol APClaimBook {
    func update(_ body: ([APClaim]) -> (put: [APClaim], drop: [String])) throws   // drop 은 실행 id(그 실행의 줄 모두)
}

public enum APClaimResult: Equatable, Sendable {
    case ok(cleared: [String])                                // 잡았다. 치운 남은 자리의 실행 id
    case busy(run: String, path: String, mine: String)        // 그 실행이 겹치는 자리에서 돈다(그 실행의 경로, 내 경로)
    case failed(String)                                       // 표를 쓰지 못함(출발하지 않는다 — 닫힌 쪽)
}

// 유스케이스: 출발할 때 경로 자리를 잡는다(한 transaction: 겹치는 다른 실행의 자리를 보고, 살아 있으면 거부, 남은 자리면 그 실행의 줄을 지우고 잡는다).
// 두 출발이 거의 같은 때 와도 잠금이 하나씩 지나가게 해, 뒤에 온 쪽은 앞 쪽의 자리를 보고 거부한다(앞 쪽 출발 명령이 아직 돌면 살아 있음)
public func apClaimPaths(run: String, runDir: String, label: String, pid: Int32?, paths: [String], home: String,
                         book: APClaimBook, probe: APClaimProbe, clock: APClock) -> APClaimResult {
    let t = clock.now.timeIntervalSince1970
    let mine = apClaimKeys(paths)
    var result: APClaimResult = .ok(cleared: [])
    do {
        try book.update { claims in
            var dead: [String] = [], aliveCache: [String: Bool] = [:]
            for c in claims where c.run != run {
                guard let m = mine.first(where: { apPathOverlap($0.key, c.key, home: home) }) else { continue }
                let alive = aliveCache[c.run] ?? apClaimAlive(probe.view(c))
                aliveCache[c.run] = alive
                if alive { result = .busy(run: c.run, path: c.path, mine: m.path); return ([], []) }
                if !dead.contains(c.run) { dead.append(c.run) }
            }
            result = .ok(cleared: dead)
            let put = mine.map { APClaim(key: $0.key, path: $0.path, run: run, runDir: runDir, label: label, pid: pid, claimed: t) }
            return (put, dead + (claims.contains { $0.run == run } ? [run] : []))
        }
    } catch {
        return .failed("\(error)")
    }
    return result
}

// 유스케이스: 경로 자리를 푼다(끝 정리를 마친 뒤, 출발하지 못했을 때)
public func apReleaseClaims(run: String, book: APClaimBook) throws {
    try book.update { claims in claims.contains { $0.run == run } ? ([], [run]) : ([], []) }
}

// 출발을 거부할 때의 글(보드 출발 준비의 잠금 글과 같은 말)
public func apClaimBusyText(_ r: APClaimResult, home: String) -> String? {
    guard case .busy(let other, let path, _) = r else { return nil }
    let shown = path.hasPrefix(home + "/") ? "~/" + path.dropFirst(home.count + 1) : path
    return "이 경로에서 자율주행이 돌고 있습니다(\(other), \(shown)). 그 실행이 끝난 뒤에 출발합니다"
}

// MARK: - ④ 여러 엔진이 같은 기록 저장소를 쓸 때

// 잠겨 있어(SQLITE_BUSY·LOCKED) 실패한 걸음을 다시 할 때 쉬는 초 — attempt 는 1 부터. 여덟 번까지(0.1·0.2·0.4·0.8 뒤로는 1초), 그 뒤는 nil(포기).
// 엔진은 busy_timeout 5초로 한 번 더 기다리므로 다른 엔진이 오래 잡아도 몇십 초까지 다시 한다. 열기의 journal_mode·판 읽기와 쓰기 transaction 이 함께 쓴다
public let apBusyRetries = 8
public func apBusyRetryDelay(_ attempt: Int) -> Double? {
    guard attempt >= 1, attempt <= apBusyRetries else { return nil }
    return min(0.1 * Double(1 << min(attempt - 1, 4)), 1.0)
}

