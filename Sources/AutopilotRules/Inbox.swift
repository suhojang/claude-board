// 자율주행 규칙(AutopilotRules) — 방향을 바꿀 말(inbox/<시각>.md, 2026-10-30 2단계 5-4 에서 새로 씀. 설계 5장 2절, 3장 "사람이 끼어듦").
// 사람이 자율주행 중 화면에서 한마디를 써서 "지금 단위에"(to: now) 또는 "다음 단위부터"(to: next)로 보낸다. 보드가 inbox/ 에 파일 하나로 쓰고
// 엔진이 1초마다 읽어 run.json 의 inbox 에 받은 것·넘긴 것을 적고 진행 기록(events.jsonl)에 한 줄씩 남긴다.
// 출발 뒤 queue.json 은 동결이라 이 말은 범위(단위·판정·쓰기 범위·가지 규칙)를 바꾸지 않고 방향만 바꾼다 — 모델에게 넘기는 글에도 그렇게 적는다.
// 이 규칙은 글 꼴·어느 단위에 붙일지·언제 넘길지·모델에게 넘길 글을 정한다. 파일 읽기·쓰기는 포트(APInboxFiles·APInboxOut)로만 한다
import Foundation

public enum APInboxTo: String, Equatable, Sendable, Codable { case now, next }

// 파일 하나의 글 — 머리(빈 줄 앞까지 "이름: 값" 줄)와 본문
public struct APInboxNote: Equatable, Sendable {
    public var to: APInboxTo
    public var unit: Int?       // 보드가 보낼 때 본 지금 단위(그 단위가 돌고 있을 때만 적는다)
    public var at: Double?      // 보낸 시각(유닉스 초)
    public var text: String
    public init(to: APInboxTo, unit: Int? = nil, at: Double? = nil, text: String) { self.to = to; self.unit = unit; self.at = at; self.text = text }
}

public let apInboxMaxChars = 2000       // 본문 글자 수 상한(보드 입력 칸과 같음)
public let apInboxMaxBytes = 16_384     // 파일 크기 상한(엔진이 이보다 크면 읽지 않는다)

// 보드가 쓰는 글. 본문은 앞뒤 빈칸을 다듬고 줄 끝은 \n 으로
public func apInboxCompose(_ n: APInboxNote) -> String {
    var h = "to: \(n.to.rawValue)\n"
    if let u = n.unit { h += "unit: \(u)\n" }
    if let a = n.at { h += "at: \(String(format: "%.3f", a))\n" }
    return h + "\n" + n.text.replacingOccurrences(of: "\r\n", with: "\n").trimmingCharacters(in: .whitespacesAndNewlines) + "\n"
}

// 파일 이름 꼴 — 보드는 <yyyyMMdd-HHmmss-SSS>[-k].md 로 쓴다. 엔진은 이 꼴만 읽는다(임시 파일 .tmp-…·숨은 파일·다른 확장자는 넘김)
public func apInboxNameOK(_ name: String) -> Bool {
    name.range(of: "^[0-9A-Za-z][0-9A-Za-z._-]{0,80}\\.md$", options: .regularExpression) != nil
}

public enum APInboxParse: Equatable, Sendable {
    case note(APInboxNote)
    case bad(String)            // 읽지 않는 까닭(쉬운 말)
}
// 엔진이 읽는 글. 머리 to 는 꼭 있어야 하고(now·next), unit 은 1 이상의 정수, at 은 수. 모르는 머리 칸은 넘긴다(뒤에 칸이 늘어도 읽게).
// 본문이 비었거나 상한보다 길면 읽지 않는다
public func apInboxParse(_ raw: String) -> APInboxParse {
    let s = raw.replacingOccurrences(of: "\r\n", with: "\n")
    let lines = s.components(separatedBy: "\n")
    guard let blank = lines.firstIndex(where: { $0.trimmingCharacters(in: .whitespaces).isEmpty }) else {
        return .bad("머리와 본문 사이 빈 줄이 없습니다")
    }
    var to: APInboxTo?, unit: Int?, at: Double?
    for l in lines[..<blank] {
        guard let c = l.firstIndex(of: ":") else { return .bad("머리 줄 꼴이 아닙니다: \(l.prefix(40))") }
        let k = l[..<c].trimmingCharacters(in: .whitespaces).lowercased(), v = l[l.index(after: c)...].trimmingCharacters(in: .whitespaces)
        switch k {
        case "to":
            guard let t = APInboxTo(rawValue: v) else { return .bad("머리 to 는 now 또는 next 입니다(\(v.prefix(20)))") }
            to = t
        case "unit":
            guard let u = Int(v), u >= 1 else { return .bad("머리 unit 은 1 이상의 정수입니다(\(v.prefix(20)))") }
            unit = u
        case "at":
            guard let a = Double(v), a.isFinite else { return .bad("머리 at 은 수입니다(\(v.prefix(20)))") }
            at = a
        default: continue
        }
    }
    guard let to else { return .bad("머리에 to: now 또는 to: next 가 없습니다") }
    let text = lines[(blank + 1)...].joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return .bad("본문이 비었습니다") }
    guard text.count <= apInboxMaxChars else { return .bad("본문이 \(apInboxMaxChars)자보다 깁니다(\(text.count)자)") }
    return .note(APInboxNote(to: to, unit: unit, at: at, text: text))
}

// run.json 의 inbox 한 줄 — 받은 말 하나와 그 말이 어떻게 됐는지(보드가 그린다)
public struct APInboxEntry: Equatable, Sendable, Codable {
    public var file: String          // inbox/ 안 파일 이름(같은 파일을 다시 읽지 않는 열쇠)
    public var to: String            // now·next(읽지 못한 파일은 빈 글자)
    public var text: String
    public var sent: Double?         // 머리 at
    public var seen: Int?            // 머리 unit(보낼 때 본 단위)
    public var received: Double      // 엔진이 받은 시각
    public var status: String        // 아래 apInboxStatuses
    public var unit: Int?            // now: 넘길 단위
    public var from: Int?            // next: 붙이기 시작할 단위
    public var units: [Int]          // 넘긴(now)·붙인(next) 단위
    public var done: Double?         // 처음 넘긴·붙인 시각
    public var why: String?          // late·dropped·unused·bad 의 까닭
    public var source: String?       // 없으면 사람이 보드에서 보낸 말, takeover 면 돌려줄 때 엔진이 만든 이어받기 지시(⑫-8, Takeover.swift). 없는 칸은 옛 꼴 그대로 읽힌다
    public init(file: String, to: String, text: String, sent: Double? = nil, seen: Int? = nil, received: Double, status: String,
                unit: Int? = nil, from: Int? = nil, units: [Int] = [], done: Double? = nil, why: String? = nil) {
        self.file = file; self.to = to; self.text = text; self.sent = sent; self.seen = seen; self.received = received; self.status = status
        self.unit = unit; self.from = from; self.units = units; self.done = done; self.why = why
    }
}
// waiting: 넘길·붙일 차례를 기다림, delivered: now 를 세션에 넘김, applied: next 를 단위 첫 입력에 붙임(뒤 단위에도 이어 붙는다),
// late: 받았을 때 이미 넘길 곳이 없음, dropped: now 를 넘기기 전에 그 단위가 끝남, unused: next 를 붙일 단위가 시작하지 않고 실행이 끝남, bad: 읽지 못함
public let apInboxStatuses = ["waiting", "delivered", "applied", "late", "dropped", "unused", "bad"]

// 받은 말을 어느 단위에 붙일지 — statuses 는 queue.json 차례의 단위 상태(running 은 일시정지·한도 대기 중인 단위도)
//  now: 보낼 때 본 단위(seen)가 있으면 그 단위가 아직 돌거나 시작 전일 때만 그 단위, 끝났으면 넘기지 않는다(다른 단위의 일에 그 말이 섞이지 않게).
//       없으면 지금 도는 단위, 도는 단위가 없으면(멈출 선 대기·단위 사이) 다음에 시작할 단위의 첫 입력
//  next: 지금 도는 단위 뒤(도는 단위가 없으면 처음부터)의 첫 대기 단위부터 남은 단위 모두
public struct APInboxBind: Equatable, Sendable {
    public var status: String        // waiting 또는 late
    public var unit: Int?
    public var from: Int?
    public var why: String?
    public init(status: String, unit: Int? = nil, from: Int? = nil, why: String? = nil) { self.status = status; self.unit = unit; self.from = from; self.why = why }
}
public func apInboxBind(_ n: APInboxNote, statuses: [String]) -> APInboxBind {
    let running = statuses.firstIndex(of: "running").map { $0 + 1 }
    func firstPending(after k: Int) -> Int? {
        statuses.indices.first { $0 + 1 > k && statuses[$0] == "pending" }.map { $0 + 1 }
    }
    switch n.to {
    case .now:
        if let s = n.unit {
            guard s <= statuses.count else { return .init(status: "late", why: "보낼 때 본 단위(단위 \(s))가 이 실행에 없어 넘기지 않았습니다") }
            let st = statuses[s - 1]
            if st == "running" || st == "pending" { return .init(status: "waiting", unit: s) }
            return .init(status: "late", why: "보낼 때 본 단위(단위 \(s))가 이미 끝나(\(apInboxUnitWord(st))) 넘기지 않았습니다. 다음 단위부터로 다시 보내거나 멈추고 다시 출발합니다")
        }
        if let r = running { return .init(status: "waiting", unit: r) }
        if let p = firstPending(after: 0) { return .init(status: "waiting", unit: p) }
        return .init(status: "late", why: "남은 단위가 없어 넘기지 않았습니다")
    case .next:
        if let p = firstPending(after: running ?? 0) { return .init(status: "waiting", from: p) }
        return .init(status: "late", why: "뒤에 남은 단위가 없어 붙일 곳이 없습니다")
    }
}
func apInboxUnitWord(_ s: String) -> String {
    ["done": "통과", "failed": "미통과", "capped": "상한에서 멈춤", "stopped": "중단", "skipped": "건너뜀"][s] ?? s
}

// 단위 n 의 세션에 넘길 말(now, 기다림, 그 단위) — 받은 차례대로 자리 번호
public func apInboxNowFor(_ entries: [APInboxEntry], unit n: Int) -> [Int] {
    entries.indices.filter { entries[$0].to == "now" && entries[$0].status == "waiting" && entries[$0].unit == n }
}
// 단위 n 의 첫 입력에 붙일 방향(next, 기다림·붙임, 붙이기 시작할 단위가 n 이하) — 받은 차례대로
public func apInboxNextFor(_ entries: [APInboxEntry], unit n: Int) -> [Int] {
    entries.indices.filter { i in
        let e = entries[i]
        return e.to == "next" && (e.status == "waiting" || e.status == "applied") && (e.from ?? Int.max) <= n && !e.units.contains(n)
    }
}

// 모델에게 넘기는 글 — 사람의 글은 줄마다 들여 쓰고, 범위는 그대로라는 것과 범위 밖의 말은 하지 않고 적는다는 것을 붙인다
func apInboxQuote(_ texts: [String]) -> String {
    texts.map { "- " + $0.components(separatedBy: "\n").joined(separator: "\n  ") }.joined(separator: "\n")
}
let apInboxScope = "이 단위의 일·판정 명령·쓰기 범위·가지 규칙은 출발 때 고정한 그대로입니다. 그 밖의 일(다른 단위의 일, 다른 가지 맞추기, 판정 바꾸기 등)을 하라는 말이면 하지 않고 result.json 의 next_start 에 적습니다."
public func apInboxNowText(_ texts: [String]) -> String {
    "사람이 실행을 지켜보다 이 단위에 한마디를 보냈습니다(보드의 \"지금 단위에\"):\n" + apInboxQuote(texts) + "\n"
        + "이 말을 반영해 하던 일을 이어 가세요. " + apInboxScope + " 마치면 result.json 을 고쳐 쓰고 턴을 끝냅니다."
}
public func apInboxNextText(_ texts: [String]) -> String {
    "사람이 실행 중에 보낸 방향(보드의 \"다음 단위부터\", 이 단위와 뒤 단위에 모두 붙습니다):\n" + apInboxQuote(texts) + "\n"
        + "이 방향은 일하는 방식만 바꿉니다. " + apInboxScope
}
// 판정은 통과했는데 그 사이 지금 단위에 말이 온 경우 — 다시 맡기는 글의 머리(말은 apInboxNowText 로 뒤에 붙는다)
public let apInboxPassedText = "판정 명령은 통과했지만 그 사이 사람이 이 단위에 한마디를 보냈습니다. 아래 말을 반영한 뒤 판정 명령을 다시 돌려 보고 result.json 을 고쳐 쓰세요."

// 턴이 끝난 뒤: 그 사이 이 단위에 말이 왔으면 판정 전에 같은 세션으로 한 번 더(와이어프레임 "이 세션의 다음 차례에 들어갑니다")
public enum APAfterTurn: Equatable, Sendable { case check, again }
public func apAfterTurn(waitingNow: Bool) -> APAfterTurn { waitingNow ? .again : .check }

// 판정 뒤: 통과·말 없음 → 통과, 통과·말 있음 → 말을 넘기고 다시(시도 수는 세지 않음), 실패 → 시도가 남았거나 말이 있으면 다시, 아니면 미통과.
// 시도가 다 찼어도 그 사이 말이 왔으면 한 번 더 맡긴다(사람이 끼어든 것이라)
public enum APAfterCheck: Equatable, Sendable { case pass, note, retry, fail }
public func apAfterCheck(passed: Bool, tries: Int, retries: Int, waitingNow: Bool) -> APAfterCheck {
    if passed { return waitingNow ? .note : .pass }
    return tries < retries || waitingNow ? .retry : .fail
}

// 단위가 끝날 때(통과·미통과·상한·중단·건너뜀) 그 단위에 아직 넘기지 못한 now 는 dropped. 바뀐 줄 수
@discardableResult
public func apInboxCloseUnit(_ entries: inout [APInboxEntry], unit n: Int, ended: String) -> Int {
    var k = 0
    for i in apInboxNowFor(entries, unit: n) {
        entries[i].status = "dropped"; entries[i].why = "넘기기 전에 그 단위(단위 \(n))가 끝나(\(apInboxUnitWord(ended))) 넘기지 못했습니다"; k += 1
    }
    return k
}
// 실행이 끝날 때 남은 것: 기다리던 now 는 dropped, 한 번도 붙이지 못한 next 는 unused. 바뀐 줄 수
@discardableResult
public func apInboxCloseRun(_ entries: inout [APInboxEntry]) -> Int {
    var k = 0
    for i in entries.indices where entries[i].status == "waiting" {
        if entries[i].to == "now" { entries[i].status = "dropped"; entries[i].why = "넘기기 전에 실행이 끝났습니다" }
        else { entries[i].status = "unused"; entries[i].why = "붙일 단위가 시작하지 않고 실행이 끝났습니다" }
        k += 1
    }
    return k
}

// MARK: - 포트와 유스케이스

// 엔진이 읽는 inbox/ — 폴더가 없거나 링크면 빈 목록. 파일은 링크가 아닌 보통 파일만, 크기 상한 안에서만 글로
public enum APInboxRead: Equatable, Sendable { case text(String), notFile, tooBig(Int), unreadable }
public protocol APInboxFiles: Sendable {
    func names() -> [String]
    func read(_ name: String) -> APInboxRead
}

// 유스케이스(엔진): 새 파일만 읽어 받은 줄을 만든다 — 이름 꼴이 맞고 known 에 없는 것, 이름 차례로. 읽지 못한 파일도 bad 줄로 남겨 다시 읽지 않는다
public func apInboxReceive(files: APInboxFiles, known: [APInboxEntry], statuses: [String], clock: APClock) -> [APInboxEntry] {
    let seen = Set(known.map(\.file))
    let now = clock.now.timeIntervalSince1970
    var out: [APInboxEntry] = []
    for name in files.names().filter({ apInboxNameOK($0) && !seen.contains($0) }).sorted() {
        func bad(_ why: String) -> APInboxEntry { APInboxEntry(file: name, to: "", text: "", received: now, status: "bad", why: why) }
        switch files.read(name) {
        case .notFile: out.append(bad("보통 파일이 아니라(링크·폴더) 읽지 않았습니다"))
        case .tooBig(let b): out.append(bad("파일이 \(apInboxMaxBytes)바이트보다 커서(\(b)바이트) 읽지 않았습니다"))
        case .unreadable: out.append(bad("글(UTF-8)로 읽지 못했습니다"))
        case .text(let t):
            switch apInboxParse(t) {
            case .bad(let why): out.append(bad(why))
            case .note(let n):
                let b = apInboxBind(n, statuses: statuses)
                out.append(APInboxEntry(file: name, to: n.to.rawValue, text: n.text, sent: n.at, seen: n.unit, received: now, status: b.status,
                                        unit: b.unit, from: b.from, why: b.why))
            }
        }
    }
    return out
}

// 보드가 쓰는 inbox/ — 있는지와 쓰기(임시 파일에 쓰고 이름을 바꿔 엔진이 반쯤 쓴 파일을 읽지 않게)
public protocol APInboxOut: Sendable {
    func exists(_ name: String) -> Bool
    func write(_ name: String, _ data: Data) -> Bool
}
public enum APInboxSend: Equatable, Sendable { case sent(String), refused(String) }
// 유스케이스(보드): 한마디 보내기 — 끝나지 않은(출발한) 실행에만, 빈 글·상한보다 긴 글은 보내지 않는다. 이름은 stamp(.md), 있으면 -2, -3 …
public func apInboxSend(_ n: APInboxNote, runState: String, stamp: String, out: APInboxOut) -> APInboxSend {
    guard !runState.isEmpty, !apRunEnded(runState) else { return .refused("끝난 실행에는 보내지 않습니다") }
    let text = n.text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return .refused("보낼 말을 써 주세요") }
    guard text.count <= apInboxMaxChars else { return .refused("\(apInboxMaxChars)자까지 보낼 수 있습니다(지금 \(text.count)자)") }
    var name = stamp + ".md", k = 1
    while out.exists(name) { k += 1; name = "\(stamp)-\(k).md" }
    guard apInboxNameOK(name) else { return .refused("파일 이름 꼴이 아닙니다(\(name))") }
    var m = n; m.text = text
    return out.write(name, Data(apInboxCompose(m).utf8)) ? .sent(name) : .refused("inbox 에 쓰지 못했습니다")
}

// 보드에 보일 말의 한 줄 — 상태 이름과 자세한 말
public func apInboxLabel(_ e: APInboxEntry) -> (status: String, detail: String) {
    let to = e.to == "next" ? "다음 단위부터" : "지금 단위에"
    switch e.status {
    case "waiting":
        if e.to == "next" { return ("기다림", "단위 \(e.from.map(String.init) ?? "?") 부터 첫 입력에 붙입니다") }
        return ("기다림", "단위 \(e.unit.map(String.init) ?? "?") 의 다음 차례에 넘깁니다")
    case "delivered": return ("넘김", "단위 \(e.units.map(String.init).joined(separator: "·")) 의 세션에 넘겼습니다")
    case "applied": return ("붙임", "단위 \(e.units.map(String.init).joined(separator: "·")) 의 첫 입력에 붙였습니다" + (e.to == "next" ? "(뒤 단위에도 붙습니다)" : ""))
    case "late": return ("넘기지 않음", e.why ?? "\(to) 넘길 곳이 없었습니다")
    case "dropped": return ("넘기지 못함", e.why ?? "")
    case "unused": return ("쓰이지 않음", e.why ?? "")
    case "bad": return ("읽지 못함", e.why ?? "")
    default: return (e.status, e.why ?? "")
    }
}

// 엔진이 받았을 때 진행 기록(who board) 한 줄. unitRunning 은 now 를 붙일 단위가 지금 도는 단위인가(아니면 그 단위가 시작할 때 첫 입력에 붙는다)
public func apInboxReceivedText(_ e: APInboxEntry, unitRunning: Bool) -> String {
    let t = e.text.replacingOccurrences(of: "\n", with: " ")
    let q = "«" + (t.count > 80 ? String(t.prefix(80)) + "…" : t) + "»"
    switch e.status {
    case "waiting" where e.to == "now":
        return "사람이 보낸 말을 받았습니다(지금 단위에 · 단위 \(e.unit ?? 0)): \(q). " + (unitRunning ? "이 세션의 다음 차례에 넘깁니다." : "그 단위가 시작할 때 첫 입력에 붙입니다.")
    case "waiting":
        return "사람이 보낸 방향을 받았습니다(다음 단위부터 · 단위 \(e.from ?? 0) 부터): \(q). 대기열(범위)은 그대로이고 뒤 단위의 첫 입력에 붙입니다."
    case "bad":
        return "inbox 의 \(e.file) 을 읽지 못해 넘겼습니다: \(e.why ?? "")"
    default:
        return "사람이 보낸 말을 받았지만 넘기지 않습니다(\(e.to == "next" ? "다음 단위부터" : "지금 단위에")): \(q). \(e.why ?? "")"
    }
}
