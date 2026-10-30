// 출발 때 만드는 티켓 나무와 실행 중에 새로 필요해진 티켓 시험(Tickets.swift) — 2026-10-30 4단계 ⑩-3 에서 새로 씀.
// 실제 Atlassian 으로는 요청하지 않는다: HTTP·키체인·상태 파일·시각은 가짜 어댑터(PubHTTP·PubSecrets·MemTicketStore·PubClock)다.
// 요청 꼴(POST /rest/api/3/issue 의 fields, GET /rest/api/3/project 의 issueTypes, PUT 의 parent)은 공식 문서를 보고 정한 이 조각의 이해이고,
// 골든(Publish/ticket-*.json)은 그 이해와 코드가 같은지를 본다(진짜 서버가 받는지는 사람이 처음 출발할 때 본다)
import Foundation
import Testing
@testable import AutopilotRules

private let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
private let seoul = TimeZone(identifier: "Asia/Seoul")!
private let fakeToken = "fake-token-ZZ9-not-real"
private let site = "https://zz-toy.atlassian.net"

private func golden(_ name: String, _ text: String) {
    let url = here.appendingPathComponent("Publish").appendingPathComponent(name)
    if ProcessInfo.processInfo.environment["AP_REPORT_GOLDEN"] == "write" { try? Data(text.utf8).write(to: url); return }
    let want = (try? String(contentsOf: url, encoding: .utf8)) ?? "(골든 \(name) 없음)"
    if text != want {
        let a = text.components(separatedBy: "\n"), b = want.components(separatedBy: "\n")
        let k = zip(a, b).enumerated().first { $0.element.0 != $0.element.1 }?.offset ?? min(a.count, b.count)
        Issue.record("골든 \(name) 와 다름: \(k + 1)번째 줄\n  지금: \(k < a.count ? String(a[k].prefix(300)) : "(없음)")\n  골든: \(k < b.count ? String(b[k].prefix(300)) : "(없음)")")
    }
}
private func pretty(_ d: Data?) -> String {
    guard let d, let o = try? JSONSerialization.jsonObject(with: d) else { return "(본문 없음)\n" }
    return String(decoding: (try? JSONSerialization.data(withJSONObject: o, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])) ?? Data(), as: UTF8.self) + "\n"
}

// 가짜 상태 파일 — 메모리, 쓴 차례의 상태를 남긴다. locked 면 잠금을 못 잡는다
final class MemTicketStore: APTicketStore {
    var book = APTicketBook()
    var writes: [APTicketBook] = []
    var locked = false
    func read() -> APTicketBook { book }
    func write(_ b: APTicketBook) -> Bool { book = b; writes.append(b); return true }
    func withLock<T>(_ body: () -> T) -> T? { locked ? nil : body() }
}

private func conn(scopes: [String] = ["jira", "read"], expires: String = "") -> APConnection {
    APConnection(id: "atlassian", service: .atlassian, site: site, account: "kim@example.com", scopes: scopes, expires: expires)
}
private let typesBody = """
{"key":"SHOP","issueTypes":[{"id":"10000","name":"Epic","subtask":false,"hierarchyLevel":1},{"id":"10001","name":"Story","subtask":false,"hierarchyLevel":0},
{"id":"10002","name":"Task","subtask":false,"hierarchyLevel":0},{"id":"10003","name":"Subtask","subtask":true,"hierarchyLevel":-1}]}
"""
private func types() -> APHTTPResponse { APHTTPResponse(status: 200, body: Data(typesBody.utf8)) }
private func made(_ key: String) -> APHTTPResponse { APHTTPResponse(status: 201, body: Data("{\"id\":\"1\",\"key\":\"\(key)\"}".utf8)) }

// 새 스토리 하나(단위 둘), 그 스토리의 새 작업 하나, 이미 있는 스토리 아래 새 작업 하나, 다른 스토리로 옮긴 작업 하나
private func plan() -> APTicketPlan {
    APTicketPlan(run: "20261030-1500-zz", epic: "SHOP-10", entries: [
        APTicketEntry(id: "story:new-1", kind: .story, title: "쿠폰 만료 묶음", parent: "SHOP-10", units: ["쿠폰 만료일 검사", "만료 쿠폰 안내"]),
        APTicketEntry(id: "task:u2", kind: .task, title: "쿠폰 만료일 검사", parent: "story:new-1", units: ["쿠폰 만료일 검사"]),
        APTicketEntry(id: "task:u4", kind: .task, title: "장바구니 합계 고침", parent: "SHOP-12", units: ["장바구니 합계 고침"]),
        APTicketEntry(id: "move:SHOP-13", kind: .move, parent: "story:new-1", key: "SHOP-13"),
    ])
}

@Suite("티켓 나무(⑩-3)") struct TicketsTests {
    // 고칠 것 표 — 맞는 계획을 하나씩 망가뜨린다(이름 → 기대 까닭)
    static let broken: [(String, [String])] = [
        ("맞음", []), ("에픽 꼴", ["에픽 번호 꼴이 아닙니다"]), ("모르는 부모", ["부모 story:new-9 가 계획에 없습니다"]),
        ("스토리 부모가 에픽이 아님", ["부모가 에픽(SHOP-10)이 아닙니다"]), ("옮길 키와 이름이 다름", ["옮길 작업 키 꼴이 아닙니다"]),
        ("같은 항목 둘", ["같은 항목이 둘입니다"]), ("빈 제목", ["제목이 비었습니다"]), ("종류와 이름이 다름", ["종류가 이름과 다릅니다"]),
    ]
    @Test("계획 파일 읽기와 고칠 것", arguments: broken)
    func planParse(_ c: (String, [String])) throws {
        var o = apTicketPlanJSON(plan())
        var e = o["entries"] as! [[String: Any]]
        switch c.0 {
        case "에픽 꼴": o["epic"] = "shop 10"
        case "모르는 부모": e[1]["parent"] = "story:new-9"
        case "스토리 부모가 에픽이 아님": e[0]["parent"] = "SHOP-11"
        case "옮길 키와 이름이 다름": e[3]["key"] = "SHOP-14"
        case "같은 항목 둘": e.append(e[1])
        case "빈 제목": e[1]["title"] = " \u{202E} "
        case "종류와 이름이 다름": e[1]["kind"] = "story"
        default: break
        }
        o["entries"] = e
        let (p, probs) = apTicketPlanParse(try JSONSerialization.data(withJSONObject: o))
        if c.1.isEmpty {
            #expect(probs.isEmpty, "\(c.0): \(probs)")
            #expect(p == plan())
            #expect(p.project == "SHOP")
        } else {
            for w in c.1 { #expect(probs.contains { $0.contains(w) }, "\(c.0): \(probs)") }
        }
    }

    @Test("이슈 종류 고르기", arguments: [
        ("영문 기본", [("1", "Story", false, 0), ("2", "Task", false, 0), ("3", "Subtask", true, -1)], "", "", "1", "3"),
        ("한글 이름", [("1", "작업", false, 0), ("2", "스토리", false, 0), ("3", "하위 작업", true, -1)], "", "", "2", "3"),
        ("옛 하위 작업 이름", [("1", "Story", false, 0), ("3", "Sub-task", true, nil)], "", "", "1", "3"),
        ("이름 없이 수준만", [("1", "Feature", false, 0), ("3", "Child", false, -1)], "", "", "1", "3"),
        ("프로젝트 설정 이름", [("1", "Story", false, 0), ("2", "User Story", false, 0), ("3", "Subtask", true, -1), ("4", "개발 하위", true, -1)], "user story", "개발 하위", "2", "4"),
        ("설정 이름이 없으면 고르지 않음", [("1", "Story", false, 0), ("3", "Subtask", true, -1)], "Feature", "", "", "3"),
        ("하위 작업 종류 없음", [("1", "Story", false, 0), ("2", "Task", false, 0)], "", "", "1", ""),
    ] as [(String, [(String, String, Bool, Int?)], String, String, String, String)])
    func pickType(_ c: (String, [(String, String, Bool, Int?)], String, String, String, String)) {
        let ts = c.1.map { APIssueType(id: $0.0, name: $0.1, subtask: $0.2, level: $0.3) }
        #expect((apPickIssueType(ts, for: .story, name: c.2)?.id ?? "") == c.4, "\(c.0) 스토리")
        #expect((apPickIssueType(ts, for: .task, name: c.3)?.id ?? "") == c.5, "\(c.0) 작업")
    }

    @Test("한꺼번에 만들기: 이슈 종류 읽기 하나 → 스토리 → 작업 → 옮기기, 만든 키가 대기열 값으로")
    func makeAll() {
        let store = MemTicketStore(), clock = PubClock(1_793_300_000)
        let http = PubHTTP([types(), made("SHOP-101"), made("SHOP-102"), made("SHOP-103"), APHTTPResponse(status: 204)])
        let r = apMakeTickets(plan(), store: store, connections: [conn()], dev: false, secrets: PubSecrets(["atlassian": fakeToken]), http: http,
                              clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r.requests == 5)
        #expect(r.left == 0)
        #expect(r.made == ["story:new-1", "task:u2", "task:u4", "move:SHOP-13"])
        #expect(http.got.map { "\($0.method) \($0.url.replacingOccurrences(of: site, with: ""))" } == [
            "GET /rest/api/3/project/SHOP", "POST /rest/api/3/issue", "POST /rest/api/3/issue", "POST /rest/api/3/issue", "PUT /rest/api/3/issue/SHOP-13"])
        // 작업은 만든 스토리 키를 부모로, 옮기기도 만든 스토리 키로
        let bodies = http.got.map { (try? JSONSerialization.jsonObject(with: $0.body ?? Data())) as? [String: Any] }
        let f = { (i: Int) in bodies[i]?["fields"] as? [String: Any] ?? [:] }
        #expect((f(1)["parent"] as? [String: Any])?["key"] as? String == "SHOP-10")
        #expect((f(1)["issuetype"] as? [String: Any])?["id"] as? String == "10001")
        #expect((f(2)["parent"] as? [String: Any])?["key"] as? String == "SHOP-101")
        #expect((f(2)["issuetype"] as? [String: Any])?["id"] as? String == "10003")
        #expect((f(3)["parent"] as? [String: Any])?["key"] as? String == "SHOP-12")
        #expect((f(4)["parent"] as? [String: Any])?["key"] as? String == "SHOP-101")
        let keys = apTicketKeys(plan(), r.book)
        #expect(keys.stories == ["new-1": "SHOP-101"])
        #expect(keys.tasks == ["u2": "SHOP-102", "u4": "SHOP-103"])
        #expect(r.book.entries["story:new-1"]?.url == site + "/browse/SHOP-101")
        #expect(r.book.site == site)
        // 보내기 전에 만드는 중으로 썼다(죽으면 다음에 확인 필요)
        #expect(store.writes.contains { $0.entries["story:new-1"]?.state == .creating })
        // 토큰은 인증 머리로만(본문·주소·상태 파일에 없음)
        #expect(http.auths.allSatisfy { $0.hasPrefix("Authorization: Basic ") })
        #expect(!http.got.contains { String(decoding: $0.body ?? Data(), as: UTF8.self).contains(fakeToken) || $0.url.contains(fakeToken) })
        #expect(!String(decoding: (try? JSONSerialization.data(withJSONObject: apTicketBookJSON(r.book))) ?? Data(), as: UTF8.self).contains(fakeToken))
        // 다시 누르면 요청 0
        let again = apMakeTickets(plan(), store: store, connections: [conn()], dev: false, secrets: PubSecrets(["atlassian": fakeToken]), http: PubHTTP(),
                                  clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(again.requests == 0)
        #expect(again.note.contains("모두 있습니다"))
    }

    @Test("실패하면 남기고 다시 누르면 남은 것만, 스토리를 못 만들면 그 아래는 보내지 않음")
    func retryLeft() {
        let store = MemTicketStore(), clock = PubClock(1_793_300_000), sec = PubSecrets(["atlassian": fakeToken])
        // 첫 번: 스토리 400 → 그 스토리의 작업·옮기기는 보내지 않고, 이미 있는 스토리 아래 작업은 만든다
        let h1 = PubHTTP([types(), APHTTPResponse(status: 400, body: Data("{\"errors\":{\"summary\":\"x\"}}".utf8)), made("SHOP-103")])
        let r1 = apMakeTickets(plan(), store: store, connections: [conn()], dev: false, secrets: sec, http: h1, clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r1.requests == 3)
        #expect(r1.made == ["task:u4"])
        #expect(r1.failed == ["story:new-1"])
        #expect(r1.left == 3)
        #expect(r1.book.entries["task:u2"]?.state == .waiting)
        #expect(r1.book.entries["task:u2"]?.last?.note.contains("먼저 만들어야") == true)
        #expect(r1.book.entries["story:new-1"]?.last?.note.contains("400") == true)
        // 둘째 번: 남은 셋만(이미 만든 task:u4 는 요청 0)
        let h2 = PubHTTP([types(), made("SHOP-104"), made("SHOP-105"), APHTTPResponse(status: 204)])
        let r2 = apMakeTickets(plan(), store: store, connections: [conn()], dev: false, secrets: sec, http: h2, clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r2.requests == 4)
        #expect(r2.left == 0)
        #expect(apTicketKeys(plan(), r2.book).tasks == ["u2": "SHOP-105", "u4": "SHOP-103"])
        #expect(h2.got.filter { $0.method == "POST" }.count == 2)
    }

    @Test("만드는 중에 죽었으면 확인 필요(요청 0), 사람이 확인하면 그것만 다시")
    func uncertain() {
        let store = MemTicketStore(), clock = PubClock(1_793_300_000), sec = PubSecrets(["atlassian": fakeToken])
        var b = APTicketBook(); b.site = site
        var rec = APTicketRecord(); rec.state = .creating; b.entries["story:new-1"] = rec
        store.book = b
        let h = PubHTTP()
        let r = apMakeTickets(plan(), store: store, connections: [conn()], dev: false, secrets: sec, http: h, clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r.requests == 0)
        #expect(r.book.entries["story:new-1"]?.state == .uncertain)
        #expect(r.note.contains("확인 필요 1개"))
        let h2 = PubHTTP([types(), made("SHOP-201"), made("SHOP-202"), made("SHOP-203"), APHTTPResponse(status: 204)])
        let r2 = apMakeTickets(plan(), store: store, connections: [conn()], dev: false, secrets: sec, http: h2, clock: clock, waiter: PubWaiter(clock), tz: seoul,
                               confirm: ["story:new-1"])
        #expect(r2.left == 0)
        #expect(r2.requests == 5)
    }

    @Test("연결로 만들 수 없으면 요청 0", arguments: [
        ("연결 없음", nil, true, "Atlassian 연결이 없습니다"),
        ("Jira 쓰기 범위 없음", conn(scopes: ["confluence", "read"]), true, "'Jira 쓰기'가 없습니다"),
        ("토큰 없음", conn(), false, "토큰이 키체인에 없습니다"),
        ("만료", conn(expires: "2026-10-01"), true, "만료일(2026-10-01)이 지났습니다"),
        ("http 사이트", APConnection(id: "atlassian", service: .atlassian, site: "http://zz.example", account: "a@b", scopes: ["jira"]), true, "사이트 주소가 규칙에"),
        ("계정 메일 없음", APConnection(id: "atlassian", service: .atlassian, site: site, scopes: ["jira"]), true, "계정 메일이 없습니다"),
    ] as [(String, APConnection?, Bool, String)])
    func blocked(_ c: (String, APConnection?, Bool, String)) {
        let clock = PubClock(1_793_300_000), h = PubHTTP()
        let r = apMakeTickets(plan(), store: MemTicketStore(), connections: c.1.map { [$0] } ?? [], dev: false,
                              secrets: PubSecrets(c.2 ? ["atlassian": fakeToken] : [:]), http: h, clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r.requests == 0, "\(c.0)")
        #expect(h.got.isEmpty)
        #expect(r.note.contains(c.3), "\(c.0): \(r.note)")
        #expect(r.left == 4)
    }

    @Test("429 는 기다렸다 다시, 사이트가 바뀌면 요청 0, 잠금을 못 잡으면 busy")
    func waitsAndGuards() {
        let clock = PubClock(1_793_300_000), sec = PubSecrets(["atlassian": fakeToken]), w = PubWaiter(clock)
        let one = APTicketPlan(run: "r", epic: "SHOP-10", entries: [APTicketEntry(id: "task:u4", kind: .task, title: "합계", parent: "SHOP-12")])
        let h = PubHTTP([types(), APHTTPResponse(status: 429, headers: ["retry-after": "2"]), made("SHOP-300")])
        let store = MemTicketStore()
        let r = apMakeTickets(one, store: store, connections: [conn()], dev: false, secrets: sec, http: h, clock: clock, waiter: w, tz: seoul)
        #expect(r.left == 0)
        #expect(w.waits == [2])
        // 사이트가 바뀜: 만든 것이 있는 계획을 다른 사이트로 이어 만들지 않는다
        let two = APTicketPlan(run: "r", epic: "SHOP-10", entries: one.entries + [APTicketEntry(id: "task:u5", kind: .task, title: "둘", parent: "SHOP-12")])
        var other = conn(); other.site = "https://zz-other.atlassian.net"
        let h2 = PubHTTP()
        let r2 = apMakeTickets(two, store: store, connections: [other], dev: false, secrets: sec, http: h2, clock: clock, waiter: w, tz: seoul)
        #expect(h2.got.isEmpty)
        #expect(r2.note.contains("사이트") && r2.left == 1)
        let locked = MemTicketStore(); locked.locked = true
        let r3 = apMakeTickets(one, store: locked, connections: [conn()], dev: false, secrets: sec, http: PubHTTP(), clock: clock, waiter: w, tz: seoul)
        #expect(r3.busy && r3.requests == 0)
    }

    @Test("이슈 종류를 못 읽거나 고를 수 없으면 실패 줄, 다른 프로젝트 키가 오면 받지 않음")
    func typesAndKeys() {
        let clock = PubClock(1_793_300_000), sec = PubSecrets(["atlassian": fakeToken])
        let r = apMakeTickets(plan(), store: MemTicketStore(), connections: [conn()], dev: false, secrets: sec,
                              http: PubHTTP([APHTTPResponse(status: 404)]), clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r.requests == 1)
        #expect(r.book.entries["story:new-1"]?.state == .failed)
        #expect(r.note.contains("프로젝트 SHOP 를 읽지 못했습니다"))
        // 하위 작업 종류가 없는 프로젝트
        let noSub = APHTTPResponse(status: 200, body: Data("{\"issueTypes\":[{\"id\":\"1\",\"name\":\"Story\",\"hierarchyLevel\":0}]}".utf8))
        let r2 = apMakeTickets(plan(), store: MemTicketStore(), connections: [conn()], dev: false, secrets: sec,
                               http: PubHTTP([noSub, made("SHOP-101")]), clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(r2.book.entries["task:u4"]?.last?.note.contains("하위 작업 종류가 없습니다") == true)
        #expect(apCreatedIssueKey(made("OTHER-1"), project: "SHOP") == nil)
        #expect(apCreatedIssueKey(made("SHOP-1x"), project: "SHOP") == nil)
        #expect(apCreatedIssueKey(APHTTPResponse(status: 400, body: Data("{\"key\":\"SHOP-1\"}".utf8)), project: "SHOP") == nil)
        #expect(apCreatedIssueKey(made("SHOP-77"), project: "SHOP") == "SHOP-77")
    }

    @Test("요청 꼴 골든: 스토리 만들기·하위 작업 만들기·부모 바꾸기(모델 글은 다듬고 자름)")
    func requestGolden() {
        var e = plan().entries[0]
        e.title = "쿠폰 — 만료\u{202E}묶음 " + String(repeating: "가", count: 300)
        let a = apJiraCreateIssueRequest(project: "SHOP", typeID: "10001", parent: "SHOP-10", summary: e.title, doc: apTicketDoc(e, run: "20261030-1500-zz"),
                                         base: site, auth: .basic(user: "kim@example.com"))
        let t = apJiraCreateIssueRequest(project: "SHOP", typeID: "10003", parent: "SHOP-101", summary: "쿠폰 만료일 검사",
                                         doc: apTicketDoc(plan().entries[1], run: "20261030-1500-zz"), base: site, auth: .basic(user: "kim@example.com"))
        let m = apJiraParentRequest(key: "SHOP-13", parent: "SHOP-101", base: site, auth: .basic(user: "kim@example.com"))
        golden("ticket-create-story.json", pretty(a.body))
        golden("ticket-create-task.json", pretty(t.body))
        golden("ticket-move.json", pretty(m.body))
        let f = ((try? JSONSerialization.jsonObject(with: a.body ?? Data())) as? [String: Any])?["fields"] as? [String: Any] ?? [:]
        let s = f["summary"] as? String ?? ""
        #expect(s.count <= apTicketTitleMax + 1)
        #expect(!s.contains("—") && !s.unicodeScalars.contains { $0.value == 0x202E })
        #expect(a.method == "POST" && m.method == "PUT" && m.url == site + "/rest/api/3/issue/SHOP-13")
    }

    @Test("상태 파일 읽기: 꼴이 틀린 항목은 버리고, 키 없이 만들었다는 항목은 확인 필요로")
    func bookParse() {
        let o: [String: Any] = ["format": apTicketBookFormat, "version": 1, "site": site, "entries": [
            "story:new-1": ["state": "made", "key": "SHOP-101", "tries": 1, "updated": 1],
            "task:u2": ["state": "made", "key": "<b>x</b>"],
            "../evil": ["state": "made", "key": "SHOP-1"],
            "task:u4": ["state": "zz"]]]
        let b = apTicketBookParse(try? JSONSerialization.data(withJSONObject: o))
        #expect(b.entries.keys.sorted() == ["story:new-1", "task:u2"])
        #expect(b.entries["task:u2"]?.state == .uncertain)
        #expect(apTicketBookParse(apTicketBookJSON(b).isEmpty ? nil : try? JSONSerialization.data(withJSONObject: apTicketBookJSON(b))) == b)
    }
}

// MARK: - 실행 중에 새로 필요해진 티켓(임시 번호 → 검수 뒤 만들기)

private func runWithTickets(_ tickets: [[String: String]], status: String = "done") -> APReportRun {
    var r = APReportRun()
    r.id = "20261030-1500-zz"; r.project = "zz-shop"
    var u = APReportUnit(); u.n = 3; u.title = "쿠폰 만료일 검사"; u.status = status; u.story = "SHOP-12"; u.task = "SHOP-13"
    var rec = APResultRecord()
    rec.newTickets = tickets.map { APResultTicket(temp: $0["temp"] ?? "", title: $0["title"] ?? "", why: $0["why"] ?? "") }
    u.result = rec
    r.units = [u]
    return r
}

@Suite("실행 중 새 티켓(⑩-3)") struct NewTicketTests {
    @Test("카드 답이 temp 일 때만, 이 단위의 임시 번호만, 늘 검수 뒤")
    func outbox() {
        let r = runWithTickets([["temp": "TMP-3-1", "title": "만료 알림 메일 문구", "why": "만료 안내가 따로 필요"],
                                ["temp": "TMP-4-1", "title": "다른 단위 번호"], ["temp": "SHOP-99", "title": "진짜 키 꼴"],
                                ["temp": "TMP-3-1", "title": "같은 번호 둘"], ["temp": "TMP-3-2", "title": " "]])
        let items = apNewTicketOutbox(r, unit: 3, story: "SHOP-12", task: "SHOP-13", answer: "temp", now: 1)
        #expect(items.map(\.id) == ["u3-ticket-TMP-3-1"])
        let it = items[0]
        #expect(apOutboxIDOK(it.id))
        #expect(it.kind == "ticket" && it.mode == .review)
        #expect(it.target == APOutboxTarget(service: .jira, issue: "TMP-3-1", space: "SHOP", parent: "SHOP-12"))
        #expect(apDocPlainLines(it.doc).first == apDocMarker)
        #expect(apDocPlainLines(it.doc).contains { $0.contains("임시 번호 TMP-3-1") && $0.contains("SHOP-13") })
        #expect(apItemLabel(it).contains("TMP-3-1 → SHOP-12"))
        #expect(apNewTicketOutbox(r, unit: 3, story: "SHOP-12", task: "SHOP-13", answer: "defer", now: 1).isEmpty)
        #expect(apNewTicketOutbox(r, unit: 3, story: nil, task: nil, answer: "temp", now: 1).isEmpty)
        #expect(apNewTicketOutbox(runWithTickets([["temp": "TMP-3-1", "title": "x"]], status: "skipped"), unit: 3, story: "SHOP-12", task: nil, answer: "temp", now: 1).isEmpty)
        // 파일 꼴로 돌아와도 같다. 티켓 글의 받을 곳이 임시 번호·이슈 키 꼴이 아니면 받지 않는다
        #expect(apOutboxParse(apOutboxData(it)) == it)
        var bad = it; bad.target.issue = "SHOP-14"
        #expect(apOutboxParse(apOutboxData(bad)) == nil)
    }

    @Test("검수 뒤 사람이 누르면 하위 작업 종류를 읽고 그 스토리 아래 만든다, 엔진 자동은 요청 0")
    func postTicket() {
        let r = runWithTickets([["temp": "TMP-3-1", "title": "만료 알림 메일 문구"]])
        var it = apNewTicketOutbox(r, unit: 3, story: "SHOP-12", task: "SHOP-13", answer: "temp", now: 1)[0]
        it.mode = .auto   // 자동으로 올림이어도(만들 때 늘 review 이지만, 파일을 바꿔도) 아래 자동 판정은 review 로 막힘을 본다
        it.mode = .review
        let box = MemOutbox(); box.items[it.id] = it
        let clock = PubClock(1_793_300_000), sec = PubSecrets(["atlassian": fakeToken])
        let auto = apPostItem(it.id, store: box, trigger: .auto, connections: [conn()], dev: false, secrets: sec, http: PubHTTP(), clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(auto.requests == 0)
        let h = PubHTTP([types(), made("SHOP-140")])
        let p = apPostItem(it.id, store: box, trigger: .press, connections: [conn()], dev: false, secrets: sec, http: h, clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(p.requests == 2)
        #expect(p.item?.state == .posted)
        #expect(p.item?.remote == APRemote(id: "SHOP-140", url: site + "/browse/SHOP-140"))
        let f = ((try? JSONSerialization.jsonObject(with: h.got[1].body ?? Data())) as? [String: Any])?["fields"] as? [String: Any] ?? [:]
        #expect((f["parent"] as? [String: Any])?["key"] as? String == "SHOP-12")
        #expect((f["issuetype"] as? [String: Any])?["id"] as? String == "10003")
        // 두 번째 누름은 요청 0
        let again = apPostItem(it.id, store: box, trigger: .press, connections: [conn()], dev: false, secrets: sec, http: PubHTTP(), clock: clock, waiter: PubWaiter(clock), tz: seoul)
        #expect(again.requests == 0)
    }
}
