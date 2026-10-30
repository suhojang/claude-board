// 바깥 연결·형식 변환·올릴 글 시험(Connections.swift·Formats.swift·Outbox.swift) — 2026-10-30 4단계 ⑩-1 에서 새로 씀.
// 실제 Atlassian·Notion·Figma 로는 요청하지 않는다: HTTP·키체인·파일·시각은 가짜 어댑터(PubHTTP·PubSecrets·MemOutbox·PubClock)다.
// 골든(Publish/*)은 견본 기록(Report/sample-run.json·evil-run.json)으로 만든 글자 그대로다. 틀을 고쳐 골든이 달라지면 화면으로 확인한 뒤
// `AP_REPORT_GOLDEN=write swift test …` 로 다시 쓴다. 요청 꼴(ADF·저장 형식·Notion 블록·경로)은 공식 문서를 보고 정한 이 조각의 이해이고,
// 이 시험은 그 이해와 코드가 같은지를 본다(받는 쪽이 정말 받는지는 사람이 처음 연결할 때 "연결 확인"과 첫 올리기로 본다)
import Foundation
import Testing
@testable import AutopilotRules

private let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
private let seoul = TimeZone(identifier: "Asia/Seoul")!
private let fakeToken = "fake-token-ZZ9-not-real"

private func fixture(_ name: String) -> [String: Any] {
    guard let d = try? Data(contentsOf: here.appendingPathComponent("Report").appendingPathComponent(name)),
          let j = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any] else { return [:] }
    return j
}
private func run(_ name: String) -> APReportRun {
    let j = fixture(name)
    return apReportRun(id: j["id"] as? String ?? "?", source: FixtureSource(j)) ?? APReportRun()
}
private func pretty(_ o: Any) -> String {
    let d = (try? JSONSerialization.data(withJSONObject: o, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])) ?? Data()
    return String(decoding: d, as: UTF8.self) + "\n"
}
private func golden(_ name: String, _ text: String) {
    let url = here.appendingPathComponent("Publish").appendingPathComponent(name)
    if ProcessInfo.processInfo.environment["AP_REPORT_GOLDEN"] == "write" {
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? Data(text.utf8).write(to: url)
        return
    }
    let want = (try? String(contentsOf: url, encoding: .utf8)) ?? "(골든 \(name) 없음)"
    if text != want {
        let a = text.components(separatedBy: "\n"), b = want.components(separatedBy: "\n")
        let k = zip(a, b).enumerated().first { $0.element.0 != $0.element.1 }?.offset ?? min(a.count, b.count)
        Issue.record("골든 \(name) 와 다름: \(k + 1)번째 줄\n  지금: \(k < a.count ? String(a[k].prefix(300)) : "(없음)")\n  골든: \(k < b.count ? String(b[k].prefix(300)) : "(없음)")")
    }
}

// MARK: - 가짜 어댑터

final class PubClock: APClock, @unchecked Sendable {
    var t: Double
    init(_ t: Double) { self.t = t }
    var now: Date { Date(timeIntervalSince1970: t) }
}
final class PubWaiter: APWaiter {
    let clock: PubClock
    var waits: [Double] = []
    init(_ c: PubClock) { clock = c }
    func wait(_ s: Double) { waits.append(s); clock.t += s }
}
final class PubSecrets: APSecrets {
    var items: [String: String]
    var reads = 0
    init(_ items: [String: String]) { self.items = items }
    func read(_ id: String) -> String? { reads += 1; return items[id] }
    func exists(_ id: String) -> Bool { items[id] != nil }
}
// 가짜 HTTP — 차례대로 정한 답을 돌려주고 받은 요청과 인증 머리를 남긴다(남는 답이 없으면 200 {})
final class PubHTTP: APHTTP {
    var answers: [APHTTPResponse]
    var got: [APHTTPRequest] = []
    var auths: [String] = []
    init(_ a: [APHTTPResponse] = []) { answers = a }
    func send(_ r: APHTTPRequest, auth: (name: String, value: String)?) -> APHTTPResponse {
        got.append(r)
        auths.append(auth.map { "\($0.name): \($0.value)" } ?? "")
        return answers.isEmpty ? APHTTPResponse(status: 200, body: Data("{}".utf8)) : answers.removeFirst()
    }
}
private func ok(_ id: String, _ status: Int = 200) -> APHTTPResponse { APHTTPResponse(status: status, body: Data("{\"id\":\"\(id)\"}".utf8)) }

// 가짜 outbox — 메모리, 쓴 글자(JSON)를 모두 남긴다. locked 면 잠금을 못 잡는다
final class MemOutbox: APOutboxStore {
    var items: [String: APOutboxItem] = [:]
    var writes: [String] = []
    var locked = false
    var files: [String: Data] = ["report.html": Data("<html>보고서</html>".utf8)]
    func ids() -> [String] { items.keys.sorted() }
    func read(_ id: String) -> APOutboxItem? { items[id] }
    func write(_ it: APOutboxItem) -> Bool { items[it.id] = it; writes.append(String(decoding: apOutboxData(it), as: UTF8.self)); return true }
    func create(_ it: APOutboxItem) -> Bool { guard items[it.id] == nil else { return false }; return write(it) }
    func withLock<T>(_ id: String, _ body: () -> T) -> T? { locked ? nil : body() }
    func attachment(_ name: String) -> Data? { files[name] }
}

private let atl = APConnection(id: "atlassian", service: .atlassian, site: "https://example.atlassian.net", account: "kim@example.com",
                               scopes: ["jira", "confluence"], expires: "2027-01-31")
private let notion = APConnection(id: "notion", service: .notion, account: "김요청", scopes: ["write"])
private let t0 = 1_793_340_000.0   // 2026-10-30 즈음(서울)

private func item(_ s: APTargetService, mode: APPublishMode = .review) -> APOutboxItem {
    let r = run("sample-run.json")
    switch s {
    case .jira:
        var u = apUnitOutbox(r, unit: 1, ticket: "TODO-41", publish: APPublish(mode: mode, jiraComments: true), now: t0)[0]
        u.mode = mode
        return u
    case .confluence:
        return apRunOutbox(r, publish: APPublish(mode: mode, report: APOutboxTarget(service: .confluence, space: "TODO", parent: "12345")),
                           requester: "", hasReport: true, now: t0, tz: seoul)[0]
    case .notion:
        return apRunOutbox(r, publish: APPublish(mode: mode, report: APOutboxTarget(service: .notion, parent: "0123456789abcdef0123456789abcdef")),
                           requester: "김요청", hasReport: true, now: t0, tz: seoul)[0]
    }
}

private func post(_ store: MemOutbox, _ id: String, _ trigger: APPostTrigger = .press, conns: [APConnection] = [atl, notion],
                  secrets: PubSecrets = PubSecrets(["atlassian": fakeToken, "notion": fakeToken]), http: PubHTTP, clock: PubClock = PubClock(t0),
                  dev: Bool = false, budget: Double = 30) -> (APPostResult, PubWaiter) {
    let w = PubWaiter(clock)
    return (apPostItem(id, store: store, trigger: trigger, connections: conns, dev: dev, secrets: secrets, http: http, clock: clock, waiter: w, tz: seoul, budget: budget), w)
}

// MARK: - 사이트 주소 규칙

struct SiteRow: Row { let note: String; let raw: String; let dev: Bool; let want: String }
private let siteRows: [SiteRow] = [
    .init(note: "Atlassian 클라우드", raw: "https://example.atlassian.net", dev: false, want: "ok https://example.atlassian.net"),
    .init(note: "끝 빗금·대문자는 다듬음", raw: " HTTPS://Example.Atlassian.net/ ", dev: false, want: "ok https://example.atlassian.net"),
    .init(note: "/wiki 까지 넣어도 호스트만", raw: "https://example.atlassian.net/wiki", dev: false, want: "ok https://example.atlassian.net"),
    .init(note: "포트는 남김", raw: "https://jira.example.com:8443", dev: false, want: "ok https://jira.example.com:8443"),
    .init(note: "설치판 http 는 거절", raw: "http://example.atlassian.net", dev: false, want: "bad https 주소만 받습니다"),
    .init(note: "설치판 127.0.0.1 도 거절", raw: "http://127.0.0.1:8080", dev: false, want: "bad https 주소만 받습니다"),
    .init(note: "시험 빌드는 http://127.0.0.1 만", raw: "http://127.0.0.1:8080", dev: true, want: "ok http://127.0.0.1:8080"),
    .init(note: "시험 빌드라도 localhost 는 거절", raw: "http://localhost:8080", dev: true, want: "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다"),
    .init(note: "시험 빌드라도 다른 http 는 거절", raw: "http://10.0.0.1", dev: true, want: "bad http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다"),
    .init(note: "아이디·암호 든 주소", raw: "https://user:pw@example.atlassian.net", dev: false, want: "bad 주소에 아이디·암호를 넣지 않습니다"),
    .init(note: "경로", raw: "https://example.atlassian.net/jira/x", dev: false, want: "bad 사이트 주소는 호스트까지만 씁니다(경로 없이)"),
    .init(note: "물음", raw: "https://example.atlassian.net/?a=1", dev: false, want: "bad 사이트 주소에 ? 나 # 을 넣지 않습니다"),
    .init(note: "다른 꼴", raw: "ftp://example.com", dev: true, want: "bad https 주소만 받습니다"),
    .init(note: "빈칸 든 주소", raw: "https://exa mple.com", dev: false, want: "bad 주소에 쓸 수 없는 글자가 있습니다"),
    .init(note: "빈 글자", raw: "  ", dev: false, want: "bad 주소가 비었습니다"),
    .init(note: "꼴 아님", raw: "example.atlassian.net", dev: false, want: "bad 주소 꼴이 아닙니다(https://… 로 씁니다)"),
]
private func siteText(_ c: APSiteCheck) -> String { switch c { case .ok(let s): "ok " + s; case .bad(let s): "bad " + s } }

struct WebhookRow: Row { let note: String; let raw: String; let dev: Bool; let ok: Bool }
private let webhookRows: [WebhookRow] = [
    .init(note: "Slack 꼴", raw: "https://hooks.slack.com/services/T0/B0/xyz", dev: false, ok: true),
    .init(note: "물음에 열쇠", raw: "https://chat.googleapis.com/v1/spaces/x/messages?key=a&token=b", dev: false, ok: true),
    .init(note: "경로 없음", raw: "https://hooks.example.com", dev: false, ok: false),
    .init(note: "설치판 http", raw: "http://127.0.0.1:9/hook", dev: false, ok: false),
    .init(note: "시험 빌드 127.0.0.1", raw: "http://127.0.0.1:9/hook", dev: true, ok: true),
    .init(note: "조각", raw: "https://hooks.example.com/a#b", dev: false, ok: false),
]

@Suite("자율주행 연결: 주소·만료·키체인 이름") struct ConnectionRuleTests {
    @Test("사이트 주소 규칙(설치판 https 만, 시험 빌드만 http://127.0.0.1)", arguments: siteRows)
    func 사이트(_ r: SiteRow) { #expect(siteText(apSiteCheck(r.raw, dev: r.dev)) == r.want) }

    @Test("webhook 주소 규칙", arguments: webhookRows)
    func 웹훅(_ r: WebhookRow) {
        if case .ok = apWebhookCheck(r.raw, dev: r.dev) { #expect(r.ok) } else { #expect(!r.ok) }
    }

    @Test("바탕 주소 — Notion·Figma 는 정한 API, 시험 빌드만 127.0.0.1 로 바꿈")
    func 바탕() {
        #expect(apServiceBase(atl, dev: false) == "https://example.atlassian.net")
        #expect(apServiceBase(APConnection(id: "atlassian", service: .atlassian, site: "http://127.0.0.1:9"), dev: false) == nil)
        #expect(apServiceBase(APConnection(id: "atlassian", service: .atlassian, site: "http://127.0.0.1:9"), dev: true) == "http://127.0.0.1:9")
        #expect(apServiceBase(notion, dev: false) == apNotionAPI)
        #expect(apServiceBase(APConnection(id: "notion", service: .notion, site: "http://127.0.0.1:9"), dev: false) == apNotionAPI)
        #expect(apServiceBase(APConnection(id: "notion", service: .notion, site: "http://127.0.0.1:9"), dev: true) == "http://127.0.0.1:9")
        #expect(apServiceBase(APConnection(id: "figma", service: .figma, site: "https://evil.example"), dev: true) == apFigmaAPI)
        #expect(apServiceBase(APConnection(id: "webhook-1", service: .webhook), dev: true) == nil)
    }

    @Test("같은 사이트 — 다른 호스트·포트·꼴로는 보내지 않는다")
    func 같은사이트() {
        #expect(apSameSite("https://example.atlassian.net/rest/api/3/myself", base: "https://example.atlassian.net"))
        #expect(!apSameSite("https://evil.example/rest", base: "https://example.atlassian.net"))
        #expect(!apSameSite("http://example.atlassian.net/rest", base: "https://example.atlassian.net"))
        #expect(!apSameSite("http://127.0.0.1:9/x", base: "http://127.0.0.1:8"))
        #expect(!apSameSite("https://u:p@example.atlassian.net/x", base: "https://example.atlassian.net"))
    }

    @Test("만료일 — 오늘(서울)부터 남은 날, 14일 안은 임박, 당일은 오늘 만료",
          arguments: [("", "none"), ("2026-13-01", "none"), ("2027-02-30", "none"), ("2027-01-31", "ok 93"), ("2026-11-13", "soon 14"),
                      ("2026-11-14", "ok 15"), ("2026-10-30", "soon 0"), ("2026-10-29", "expired 1"), ("2026-01-01", "expired 302")])
    func 만료(_ a: (String, String)) {
        let now = Date(timeIntervalSince1970: 1_793_340_000)   // 2026-10-30 13:40 서울
        let got: String
        switch apExpiry(a.0, now: now, tz: seoul) {
        case .none: got = "none"
        case .ok(let d): got = "ok \(d)"
        case .soon(let d): got = "soon \(d)"
        case .expired(let d): got = "expired \(d)"
        }
        #expect(got == a.1, "\(a.0)")
    }

    @Test("만료 글")
    func 만료글() {
        #expect(apExpiryLabel(.soon(days: 0), "2026-10-30") == "오늘 만료(2026-10-30)")
        #expect(apExpiryLabel(.expired(days: 2), "2026-10-28") == "만료됨 · 2일 지남(2026-10-28)")
        #expect(!apExpiryLabel(.ok(days: 3), "x").contains("—"))
    }

    @Test("키체인 항목 이름 — 설치판과 시험 빌드가 다름, 계정 이름은 연결 id")
    func 키체인() {
        #expect(apSecretService(dev: false) == "Claude Board-connection")
        #expect(apSecretService(dev: true) == "Claude Board Dev-connection")
        #expect(apSecretService(dev: false) != "Claude Board-account")   // 계정 전환 저장 계정과 다른 항목
        for (id, okk) in [("atlassian", true), ("notion", true), ("figma", true), ("webhook-1", true), ("webhook-120", true),
                         ("webhook-0", false), ("webhook-1000", false), ("jira", false), ("Atlassian", false), ("../x", false)] {
            #expect(apConnectionIDOK(id) == okk, "\(id)")
        }
    }

    @Test("연결 목록 파일 — 꼴이 틀린 줄·같은 id·범위 밖 값은 버리고 비밀 칸은 없다")
    func 파일() {
        let raw = """
        {"format":"claude-board-connections","version":1,"connections":[
         {"id":"atlassian","service":"atlassian","site":"https://example.atlassian.net","account":"kim@example.com","scopes":["jira","bogus"],"expires":"2027-01-31","token":"새면 안 됨"},
         {"id":"atlassian","service":"atlassian","site":"https://second.example"},
         {"id":"notion","service":"figma"},
         {"id":"webhook-1","service":"webhook","name":"팀\\u202e방","host":"hooks.example.com","expires":"내일"},
         {"id":"../x","service":"notion"}]}
        """
        let l = apParseConnections(Data(raw.utf8))
        #expect(l.map(\.id) == ["atlassian", "webhook-1"])
        #expect(l[0].scopes == ["jira"] && l[0].site == "https://example.atlassian.net")
        #expect(l[1].name == "팀방" && l[1].expires == "")
        let back = pretty(apConnectionsJSON(l))
        #expect(!back.contains("token") && !back.contains("새면"))
        #expect(apParseConnections(Data(back.utf8)) == l)
        #expect(apParseConnections(Data("{\"format\":\"x\"}".utf8)).isEmpty)
    }

    @Test("인증 머리 — 보내기 직전에만 만든다")
    func 인증() {
        #expect(apAuthHeader(.basic(user: "kim@example.com"), token: "t")! == ("Authorization", "Basic " + Data("kim@example.com:t".utf8).base64EncodedString()))
        #expect(apAuthHeader(.bearer, token: "t")! == ("Authorization", "Bearer t"))
        #expect(apAuthHeader(.figma, token: "t")! == ("X-Figma-Token", "t"))
        #expect(apAuthHeader(.none, token: "t") == nil)
    }
}

@Suite("자율주행 연결: 연결 확인(읽기 요청 하나)") struct ConnectionCheckTests {
    @Test("토큰이 없거나 주소가 규칙 밖이면 요청 0")
    func 요청없음() {
        let h = PubHTTP()
        #expect(apCheckConnection(atl, dev: false, secrets: PubSecrets([:]), http: h) == .local("토큰이 키체인에 없어 요청하지 않았습니다"))
        let local = APConnection(id: "atlassian", service: .atlassian, site: "http://127.0.0.1:9", account: "a@b")
        #expect(apCheckConnection(local, dev: false, secrets: PubSecrets(["atlassian": fakeToken]), http: h) == .local("사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다"))
        let hook = APConnection(id: "webhook-1", service: .webhook)
        if case .local = apCheckConnection(hook, dev: true, secrets: PubSecrets(["webhook-1": "https://x/y"]), http: h) {} else { Issue.record("webhook 은 요청 없이") }
        #expect(h.got.isEmpty)
    }

    @Test("서비스마다 GET 하나와 응답 해석", arguments: [("atlassian", "/rest/api/3/myself", "{\"displayName\":\"김 시험\"}", "김 시험"),
                                                   ("notion", "/v1/users/me", "{\"name\":\"Claude Board\",\"bot\":{\"owner\":{\"user\":{\"name\":\"김\"}}}}", "Claude Board · 김"),
                                                   ("figma", "/v1/me", "{\"email\":\"kim@example.com\"}", "kim@example.com")])
    func 확인(_ a: (String, String, String, String)) {
        let c = APConnection(id: a.0, service: APService(rawValue: a.0)!, site: a.0 == "atlassian" ? "https://example.atlassian.net" : "", account: "kim@example.com")
        let h = PubHTTP([APHTTPResponse(status: 200, body: Data(a.2.utf8))])
        #expect(apCheckConnection(c, dev: false, secrets: PubSecrets([a.0: fakeToken]), http: h) == .ok(who: a.3))
        #expect(h.got.count == 1 && h.got[0].method == "GET" && h.got[0].url.hasSuffix(a.1))
        #expect(!h.got[0].headers.values.contains { $0.contains(fakeToken) })   // 토큰은 요청 값이 아니라 인증 머리로만
        #expect(h.auths[0].contains(a.0 == "atlassian" ? Data("kim@example.com:\(fakeToken)".utf8).base64EncodedString() : fakeToken))
    }

    @Test("거부·다른 곳으로 보냄·429")
    func 실패() {
        for (st, want) in [(401, "rejected"), (403, "rejected"), (302, "failed"), (429, "failed"), (500, "failed")] {
            let h = PubHTTP([APHTTPResponse(status: st)])
            let o = apCheckConnection(atl, dev: false, secrets: PubSecrets(["atlassian": fakeToken]), http: h)
            switch o { case .rejected: #expect(want == "rejected"); case .failed: #expect(want == "failed"); default: Issue.record("\(st): \(o)") }
        }
    }
}

// MARK: - 형식 변환

@Suite("자율주행 올릴 글: 형식 변환") struct FormatTests {
    @Test("글 다듬기", arguments: [("A — B", "A, B"), ("줄\n바꿈\t탭", "줄 바꿈 탭"), ("거\u{202E}꾸로", "거꾸로"), ("  빈칸   여럿  ", "빈칸 여럿"), ("\u{0007}종", "종")])
    func 다듬기(_ a: (String, String)) { #expect(apDocText(a.0) == a.1) }

    @Test("Notion 글 한 칸 2,000자 — 2000 은 한 칸, 2001 은 두 칸")
    func 노션글() {
        #expect(apNotionRich(String(repeating: "가", count: 2000)).count == 1)
        let two = apNotionRich(String(repeating: "가", count: 2001))
        #expect(two.count == 2)
        #expect(((two[1]["text"] as? [String: Any])?["content"] as? String)?.count == 1)
        #expect(apNotionRich("").isEmpty)
    }

    @Test("Notion 한 번에 100블록 — 100 은 한 묶음, 101 은 둘")
    func 묶음() {
        #expect(apBatches(Array(0..<100)).map(\.count) == [100])
        #expect(apBatches(Array(0..<101)).map(\.count) == [100, 1])
        #expect(apBatches([Int]()).isEmpty)
    }

    @Test("견본 보고서 글 = 골든(ADF·저장 형식·Notion 블록)")
    func 골든() {
        let c = item(.confluence), n = item(.notion), j = item(.jira)
        golden("sample-report-storage.html", apStorage(c.doc) + "\n")
        golden("sample-report-notion.json", pretty(apNotionBlocks(n.doc)))
        golden("sample-comment-adf.json", pretty(apADF(j.doc)))
        #expect(c.title == "자율주행 보고: todo-api 10월 30일 09:00" || !c.title.isEmpty)
    }

    @Test("첫 줄은 늘 '오토파일럿 작성', Notion 은 요청자 이름이 둘째 줄")
    func 표지() {
        for s in [APTargetService.jira, .confluence, .notion] {
            let lines = apDocPlainLines(item(s).doc)
            #expect(lines.first == apDocMarker, "\(s)")
            #expect(!lines.joined().contains("—"))
        }
        #expect(apDocPlainLines(item(.notion).doc)[1] == "요청자: 김요청")
        #expect(apDocPlainLines(item(.confluence).doc).contains("흐름 그림과 단위별 기록은 첨부한 보고서(report.html)에 있습니다."))
        #expect(apDocPlainLines(item(.notion).doc).contains("흐름 그림과 단위별 기록은 보드의 실행 보고서에 있습니다(이 글에는 첨부하지 않았습니다)."))
    }

    @Test("악성 견본 — 저장 형식에 태그·매크로·위험한 링크가 새지 않는다")
    func 악성() {
        let r = run("evil-run.json")
        var doc = apReportDoc(r, notion: false, attachment: true, requester: "", itemID: "run-confluence", title: "<script>t</script>")
        doc.blocks.append(.paragraph([.link("나쁜", url: "javascript:alert(1)"), .link("데이터", url: "data:text/html,x"), .link("좋은", url: "https://example.com/a?b=1&c=2")]))
        doc.blocks.append(.code("]]></ac:plain-text-body><ac:structured-macro ac:name=\"html\">"))
        let s = apStorage(doc)
        golden("evil-report-storage.html", s + "\n")
        #expect(!s.lowercased().contains("<script") && !s.contains("<ac:") && !s.contains("javascript:") && !s.contains("data:text"))
        #expect(s.contains("<a href=\"https://example.com/a?b=1&amp;c=2\">좋은</a>"))
        let tags = Set(s.matches(of: /<\/?([a-z0-9]+)/).map { String($0.1) })
        #expect(tags.isSubset(of: ["h2", "p", "strong", "ul", "li", "pre", "a", "hr"]), "\(tags)")
        let adf = pretty(apADF(doc))
        #expect(!adf.contains("javascript:") && adf.contains("\"href\" : \"https://example.com/a?b=1&c=2\""))
        let no = pretty(apNotionBlocks(doc))
        #expect(!no.contains("javascript:") && !no.contains("—"))
    }

    @Test("문서 상한 — 블록 400, 목록 200줄, 글 조각 4000자")
    func 상한() {
        let d = APDoc(Array(repeating: APBlock.paragraph([.text(String(repeating: "x", count: 5000), bold: false)]), count: 450)
                      + [.bullets(Array(repeating: [.text("줄", bold: false)], count: 300))])
        let c = apDocClamp(d)
        #expect(c.blocks.count == 400)
        if case .paragraph(let s) = c.blocks[0], case .text(let t, _) = s[0] { #expect(t.count == 4001) } else { Issue.record("문단") }
        let b = apDocClamp(APDoc([.bullets(Array(repeating: [.text("줄", bold: false)], count: 300))]))
        if case .bullets(let rows) = b.blocks[0] { #expect(rows.count == 200) } else { Issue.record("목록") }
    }

    @Test("파일 칸 왕복")
    func 왕복() {
        let it = item(.confluence)
        let back = apOutboxParse(apOutboxData(it))
        #expect(back == it)
        #expect(apOutboxParse(Data("{\"format\":\"claude-board-outbox\",\"id\":\"../../x\"}".utf8)) == nil)
    }
}

// MARK: - 게시 설정과 만들 글

struct PublishRow: Row { let note: String; let json: String; let want: String }
private let publishRows: [PublishRow] = [
    .init(note: "없음 = 검수 뒤 올림, 받을 곳 없음", json: "null", want: "review - false | "),
    .init(note: "자동·Confluence", json: #"{"mode":"auto","report":{"service":"confluence","space":"TODO","parent":"123"},"jira_comments":true}"#,
          want: "auto confluence TODO 123 true | "),
    .init(note: "Confluence 상위 페이지 없음 = 스페이스 맨 위", json: #"{"report":{"service":"confluence","space":"~abc"}}"#, want: "review confluence ~abc  false | "),
    .init(note: "Notion id 는 줄표 빼고 소문자", json: #"{"report":{"service":"notion","parent":"0123ABCD-89ab-cdef-0123-456789ABCDEF"}}"#,
          want: "review notion  0123abcd89abcdef0123456789abcdef false | "),
    .init(note: "틀린 꼴은 그 칸만 버림", json: #"{"mode":"later","report":{"service":"confluence","space":"a b"},"jira_comments":"yes"}"#,
          want: "review - false | publish.mode 는 review 또는 auto 입니다; publish.jira_comments 는 true/false 입니다; Confluence 스페이스 키 꼴이 아닙니다: a b"),
    .init(note: "상위 페이지 id 숫자만", json: #"{"report":{"service":"confluence","space":"TODO","parent":"../1"}}"#, want: "review - false | Confluence 상위 페이지 id 는 숫자입니다: ../1"),
    .init(note: "모르는 서비스", json: #"{"report":{"service":"slack"}}"#, want: "review - false | publish.report.service 는 confluence 또는 notion 입니다"),
]

@Suite("자율주행 올릴 글: 게시 설정·만들 글") struct PublishPlanTests {
    @Test("게시 설정 읽기", arguments: publishRows)
    func 설정(_ r: PublishRow) {
        let v = try? JSONSerialization.jsonObject(with: Data(r.json.utf8), options: .fragmentsAllowed)
        let (p, probs) = apPublishParse(v is NSNull ? nil : v)
        let t = p.report.map { "\($0.service.rawValue) \($0.space) \($0.parent)" } ?? "-"
        #expect("\(p.mode.rawValue) \(t) \(p.jiraComments) | \(probs.joined(separator: "; "))" == r.want)
        #expect(apPublishParse(apPublishJSON(p)).publish == p)
    }

    @Test("단위 댓글 — 켰고 티켓 키가 있고 끝난 단위만")
    func 댓글() {
        let r = run("sample-run.json")
        let on = APPublish(jiraComments: true)
        #expect(apUnitOutbox(r, unit: 1, ticket: "TODO-41", publish: on, now: t0).map(\.id) == ["u1-jira-TODO-41"])
        #expect(apUnitOutbox(r, unit: 1, ticket: "TODO-41", publish: APPublish(), now: t0).isEmpty)
        #expect(apUnitOutbox(r, unit: 1, ticket: nil, publish: on, now: t0).isEmpty)
        #expect(apUnitOutbox(r, unit: 1, ticket: "../x", publish: on, now: t0).isEmpty)
        #expect(apUnitOutbox(r, unit: 4, ticket: "TODO-44", publish: on, now: t0).count == 1)   // 건너뜀도 댓글
        #expect(apOutboxIDOK("u1-jira-TODO-41") && apOutboxIDOK("run-confluence") && !apOutboxIDOK("run-x/../y"))
    }

    @Test("보고서 글 — 받을 곳이 있을 때 하나, Confluence 만 첨부")
    func 보고서() {
        let r = run("sample-run.json")
        #expect(apRunOutbox(r, publish: APPublish(), requester: "", hasReport: true, now: t0, tz: seoul).isEmpty)
        let c = item(.confluence), n = item(.notion)
        #expect(c.id == "run-confluence" && c.attachment == "report.html" && c.connection == "atlassian")
        #expect(n.id == "run-notion" && n.attachment == nil && n.connection == "notion")
        let noFile = apRunOutbox(r, publish: APPublish(report: APOutboxTarget(service: .confluence, space: "T")), requester: "", hasReport: false, now: t0, tz: seoul)[0]
        #expect(noFile.attachment == nil)
    }

    @Test("만든 글은 다시 만들지 않는다(같은 id)")
    func 한번() {
        let s = MemOutbox()
        #expect(apCreateItems([item(.jira), item(.confluence)], store: s) == ["u1-jira-TODO-41", "run-confluence"])
        #expect(apCreateItems([item(.jira)], store: s).isEmpty)
        #expect(s.items.count == 2)
    }

    @Test("출발 확인 알림 — 연결 없음·범위·토큰·만료")
    func 알림() {
        let p = APPublish(report: APOutboxTarget(service: .confluence, space: "T"), jiraComments: true)
        let now = Date(timeIntervalSince1970: t0)
        #expect(apPublishNotes(p, connections: [], secrets: [], dev: false, now: now, tz: seoul).count == 2)
        #expect(apPublishNotes(p, connections: [atl], secrets: ["atlassian"], dev: false, now: now, tz: seoul).isEmpty)
        var old = atl; old.expires = "2026-10-01"
        #expect(apPublishNotes(p, connections: [old], secrets: ["atlassian"], dev: false, now: now, tz: seoul).filter { $0.contains("만료일이 지났습니다") }.count == 2)
        #expect(apPublishNotes(APPublish(), connections: [], secrets: [], dev: false, now: now, tz: seoul).isEmpty)
    }
}

// MARK: - 올릴지 판정·다시 보내기

struct DecideRow: Row { let note: String; let state: APOutboxState; let mode: APPublishMode; let trigger: APPostTrigger; let conn: String; let want: String }
private let decideRows: [DecideRow] = [
    .init(note: "검수 뒤 올림 + 엔진 = 요청 0", state: .waiting, mode: .review, trigger: .auto, conn: "ok", want: "nothing"),
    .init(note: "검수 뒤 올림 + 사람이 누름 = 보냄", state: .waiting, mode: .review, trigger: .press, conn: "ok", want: "go"),
    .init(note: "자동으로 올림 + 엔진 = 보냄", state: .waiting, mode: .auto, trigger: .auto, conn: "ok", want: "go"),
    .init(note: "연결 없음 = 요청 0, 남김", state: .waiting, mode: .auto, trigger: .auto, conn: "none", want: "hold"),
    .init(note: "토큰 없음", state: .waiting, mode: .auto, trigger: .press, conn: "nosecret", want: "hold"),
    .init(note: "만료일 지남", state: .waiting, mode: .auto, trigger: .auto, conn: "expired", want: "hold"),
    .init(note: "범위 밖", state: .waiting, mode: .auto, trigger: .press, conn: "noscope", want: "hold"),
    .init(note: "설치판의 127.0.0.1 사이트", state: .waiting, mode: .auto, trigger: .press, conn: "local", want: "hold"),
    .init(note: "이미 올림 = 두 번째 누름도 요청 0", state: .posted, mode: .review, trigger: .press, conn: "ok", want: "nothing"),
    .init(note: "올리지 않기 + 엔진", state: .skipped, mode: .auto, trigger: .auto, conn: "ok", want: "nothing"),
    .init(note: "올리지 않기 + 사람이 다시 누름", state: .skipped, mode: .review, trigger: .press, conn: "ok", want: "go"),
    .init(note: "확인 필요 + 엔진 = 요청 0", state: .uncertain, mode: .auto, trigger: .auto, conn: "ok", want: "nothing"),
    .init(note: "확인 필요 + 사람", state: .uncertain, mode: .auto, trigger: .press, conn: "ok", want: "go"),
    .init(note: "거부된 실패 + 엔진 = 사람이 본 뒤", state: .failed, mode: .auto, trigger: .auto, conn: "ok", want: "nothing"),
]

@Suite("자율주행 올릴 글: 판정·다시 보내기") struct DecideTests {
    @Test("올릴지 판정", arguments: decideRows)
    func 판정(_ r: DecideRow) {
        var it = item(.jira, mode: r.mode); it.state = r.state
        var c: APConnection? = atl, secret = true
        switch r.conn {
        case "none": c = nil
        case "nosecret": secret = false
        case "expired": c!.expires = "2026-10-01"
        case "noscope": c!.scopes = ["confluence"]
        case "local": c!.site = "http://127.0.0.1:9"
        default: break
        }
        let d = apPostDecide(it, trigger: r.trigger, connection: c, hasSecret: secret, dev: false, now: Date(timeIntervalSince1970: t0), tz: seoul)
        switch d { case .go: #expect(r.want == "go"); case .nothing: #expect(r.want == "nothing"); case .hold: #expect(r.want == "hold") }
    }

    @Test("다시 보낼 때", arguments: [(429, "3", 0, 3.0), (429, nil, 0, 1.0), (429, nil, 2, 4.0), (429, "120", 0, 20.0), (429, "0", 1, 1.0),
                                    (503, nil, 1, 2.0), (429, nil, 3, -1), (500, "3", 0, -1), (401, nil, 0, -1)] as [(Int, String?, Int, Double)])
    func 다시(_ a: (Int, String?, Int, Double)) {
        let w = apRetryWait(status: a.0, retryAfter: a.1, attempt: a.2, now: Date(timeIntervalSince1970: t0))
        #expect((w ?? -1) == a.3)
    }

    @Test("Retry-After 가 HTTP 날짜")
    func 날짜() {
        let now = Date(timeIntervalSince1970: 1_793_340_000)
        let f = DateFormatter(); f.locale = Locale(identifier: "en_US_POSIX"); f.timeZone = TimeZone(identifier: "GMT"); f.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
        #expect(apRetryWait(status: 429, retryAfter: f.string(from: now.addingTimeInterval(5)), attempt: 0, now: now) == 5)
    }
}

// MARK: - 올리기 유스케이스

@Suite("자율주행 올릴 글: 올리기") struct PostTests {
    @Test("연결이 없으면 요청 0, 글은 남고 까닭")
    func 연결없음() {
        let s = MemOutbox(); _ = s.create(item(.confluence, mode: .auto))
        let h = PubHTTP()
        let (r, _) = post(s, "run-confluence", .auto, conns: [], http: h)
        #expect(h.got.isEmpty && r.requests == 0)
        #expect(s.items["run-confluence"]?.state == .waiting && s.items["run-confluence"]?.last?.what == "held")
    }

    @Test("검수 뒤 올림: 엔진이 불러도 요청 0, 사람이 눌러야 하나")
    func 검수() {
        let s = MemOutbox(); _ = s.create(item(.jira))
        let h = PubHTTP([ok("10001", 201)])
        _ = post(s, "u1-jira-TODO-41", .auto, http: h)
        #expect(h.got.isEmpty)
        let (r, _) = post(s, "u1-jira-TODO-41", .press, http: h)
        #expect(r.note == "올렸습니다" && h.got.count == 1)
        #expect(h.got[0].method == "POST" && h.got[0].url == "https://example.atlassian.net/rest/api/3/issue/TODO-41/comment")
        #expect(s.items["u1-jira-TODO-41"]?.remote == APRemote(id: "10001", url: "https://example.atlassian.net/browse/TODO-41?focusedCommentId=10001"))
    }

    @Test("같은 글을 두 번 눌러도 하나 — 두 번째는 요청 0")
    func 두번() {
        let s = MemOutbox(); _ = s.create(item(.jira))
        let h = PubHTTP([ok("10001", 201), ok("10002", 201)])
        _ = post(s, "u1-jira-TODO-41", http: h)
        let (r2, _) = post(s, "u1-jira-TODO-41", http: h)
        #expect(h.got.count == 1 && r2.requests == 0 && r2.note == "이미 올렸습니다")
        s.locked = true
        let (r3, _) = post(s, "u1-jira-TODO-41", http: h)
        #expect(r3.busy && h.got.count == 1)
    }

    @Test("보내기 전에 '올리는 중' — 죽은 뒤 다음 잠금은 '확인 필요', 엔진은 다시 올리지 않음")
    func 죽음() {
        let s = MemOutbox(); var it = item(.jira, mode: .auto); it.state = .posting; _ = s.create(it)
        let h = PubHTTP([ok("10001", 201)])
        let (r, _) = post(s, it.id, .auto, http: h)
        #expect(h.got.isEmpty && s.items[it.id]?.state == .uncertain && r.requests == 0)
        _ = post(s, it.id, .press, http: h)
        #expect(h.got.count == 1 && s.items[it.id]?.state == .posted)
        // 올리는 동안 쓴 판 가운데 "올리는 중"이 요청보다 먼저 있다
        let states = s.writes.compactMap { apOutboxParse(Data($0.utf8))?.state }
        #expect(states.suffix(2) == [.posting, .posted])
    }

    @Test("Confluence: 페이지 → 첨부(PUT), 첨부만 실패하면 다시 누를 때 첨부만")
    func 컨플() {
        let s = MemOutbox(); _ = s.create(item(.confluence))
        let h = PubHTTP([ok("98765"), APHTTPResponse(status: 500), APHTTPResponse(status: 500), APHTTPResponse(status: 500)])
        let (r, _) = post(s, "run-confluence", http: h)
        #expect(r.item?.state == .failed && r.item?.remote?.id == "98765" && r.item?.steps == ["page"])
        #expect(h.got.map(\.method) == ["POST", "PUT"])
        #expect(h.got[1].url == "https://example.atlassian.net/wiki/rest/api/content/98765/child/attachment" && h.got[1].headers["X-Atlassian-Token"] == "no-check")
        let h2 = PubHTTP([APHTTPResponse(status: 200, body: Data("{}".utf8))])
        let (r2, _) = post(s, "run-confluence", http: h2)
        #expect(h2.got.count == 1 && h2.got[0].method == "PUT" && r2.item?.state == .posted && r2.item?.steps == ["page", "attachment"])
        let page = (try? JSONSerialization.jsonObject(with: h.got[0].body ?? Data())) as? [String: Any]
        #expect((page?["space"] as? [String: Any])?["key"] as? String == "TODO" && (page?["ancestors"] as? [[String: Any]])?.first?["id"] as? String == "12345")
        let body = String(decoding: h.got[1].body ?? Data(), as: UTF8.self)
        #expect(body.contains("filename=\"report.html\"") && body.contains("<html>보고서</html>"))
    }

    @Test("Notion: 150블록 = 페이지(100) + 덧붙이기(50)")
    func 노션() {
        let s = MemOutbox()
        var it = item(.notion)
        it.doc = APDoc(it.doc.blocks + Array(repeating: .paragraph([.text("줄", bold: false)]), count: 150 - apNotionBlocks(it.doc).count))   // 목록 줄은 Notion 블록 하나씩
        _ = s.create(it)
        let h = PubHTTP([ok("0123456789abcdef0123456789abcdef"), APHTTPResponse(status: 200, body: Data("{}".utf8))])
        let (r, _) = post(s, it.id, http: h)
        #expect(r.item?.state == .posted && h.got.map(\.method) == ["POST", "PATCH"])
        #expect(h.got[0].headers["Notion-Version"] == apNotionVersion && h.got[1].url == "https://api.notion.com/v1/blocks/0123456789abcdef0123456789abcdef/children")
        let first = ((try? JSONSerialization.jsonObject(with: h.got[0].body ?? Data())) as? [String: Any])?["children"] as? [Any]
        let rest = ((try? JSONSerialization.jsonObject(with: h.got[1].body ?? Data())) as? [String: Any])?["children"] as? [Any]
        #expect(first?.count == 100 && rest?.count == 50 && r.item?.steps == ["page", "blocks:150"])
        #expect(r.item?.remote?.url == "https://www.notion.so/0123456789abcdef0123456789abcdef")
    }

    @Test("429 는 Retry-After 만큼 기다렸다 다시, 상한을 넘으면 '올리지 못함'과 다시 보낼 때")
    func 기다림() {
        let s = MemOutbox(); _ = s.create(item(.jira, mode: .auto))
        let h = PubHTTP([APHTTPResponse(status: 429, headers: ["retry-after": "2"]), APHTTPResponse(status: 429, headers: ["retry-after": "2"]), ok("10001", 201)])
        let (r, w) = post(s, "u1-jira-TODO-41", .auto, http: h)
        #expect(r.item?.state == .posted && h.got.count == 3 && w.waits == [2, 2])
        let s2 = MemOutbox(); _ = s2.create(item(.jira, mode: .auto))
        let clock = PubClock(t0)
        let h2 = PubHTTP([APHTTPResponse(status: 429, headers: ["retry-after": "20"]), APHTTPResponse(status: 429, headers: ["retry-after": "20"])])
        let (r2, w2) = post(s2, "u1-jira-TODO-41", .auto, http: h2, clock: clock, budget: 30)
        #expect(r2.item?.state == .failed && w2.waits == [20] && h2.got.count == 2)
        #expect(r2.item?.retryAfter == clock.t + 20)
        // 다시 보낼 때 전에는 엔진이 요청하지 않고, 지나면 다시 보낸다
        let h3 = PubHTTP([ok("10009", 201)])
        _ = post(s2, "u1-jira-TODO-41", .auto, http: h3, clock: PubClock(clock.t + 5))
        #expect(h3.got.isEmpty)
        _ = post(s2, "u1-jira-TODO-41", .auto, http: h3, clock: PubClock(clock.t + 25))
        #expect(h3.got.count == 1 && s2.items["u1-jira-TODO-41"]?.state == .posted)
    }

    @Test("거부(401)·다른 곳으로 보냄(302) — 다시 보낼 때 없음, 엔진은 다시 요청하지 않음")
    func 거부() {
        for st in [401, 302] {
            let s = MemOutbox(); _ = s.create(item(.jira, mode: .auto))
            let h = PubHTTP([APHTTPResponse(status: st)])
            let (r, _) = post(s, "u1-jira-TODO-41", .auto, http: h)
            #expect(r.item?.state == .failed && r.item?.retryAfter == nil && r.item?.last?.code == st)
            _ = post(s, "u1-jira-TODO-41", .auto, http: h)
            #expect(h.got.count == 1, "\(st)")
        }
    }

    @Test("만료일이 지나면 요청 0, 사람이 눌러도")
    func 만료() {
        var old = atl; old.expires = "2026-10-01"
        let s = MemOutbox(); _ = s.create(item(.jira, mode: .auto))
        let h = PubHTTP()
        let (r, _) = post(s, "u1-jira-TODO-41", .press, conns: [old], http: h)
        #expect(h.got.isEmpty && r.note.contains("만료일"))
    }

    @Test("올리기 시작한 사이트와 연결의 사이트가 다르면 요청 0")
    func 사이트바뀜() {
        let s = MemOutbox(); var it = item(.confluence); it.state = .failed; it.site = "https://old.atlassian.net"; it.remote = APRemote(id: "1", url: ""); it.steps = ["page"]
        _ = s.create(it)
        let h = PubHTTP()
        _ = post(s, it.id, http: h)
        #expect(h.got.isEmpty && s.items[it.id]?.last?.what == "held")
    }

    @Test("토큰은 글 파일·요청 값 어디에도 없다(인증 머리로만)")
    func 토큰() {
        let s = MemOutbox()
        _ = apCreateItems([item(.jira), item(.confluence), item(.notion)], store: s)
        // ids 차례: run-confluence(페이지·첨부) → run-notion → u1-jira-TODO-41
        let h = PubHTTP([ok("98765"), APHTTPResponse(status: 200, body: Data("{}".utf8)), ok("0123456789abcdef0123456789abcdef"), ok("10001", 201)])
        for id in s.ids() { _ = post(s, id, http: h) }
        #expect(s.items.values.allSatisfy { $0.state == .posted })
        #expect(!s.writes.contains { $0.contains(fakeToken) || $0.contains(Data("kim@example.com:\(fakeToken)".utf8).base64EncodedString()) })
        #expect(!h.got.contains { r in r.headers.values.contains { $0.contains(fakeToken) } || String(decoding: r.body ?? Data(), as: UTF8.self).contains(fakeToken) })
        #expect(h.auths.allSatisfy { !$0.isEmpty })
    }

    @Test("올리지 않기·나중에 — 올린 글은 바꾸지 않음")
    func 표시() {
        let s = MemOutbox(); _ = s.create(item(.jira)); let c = PubClock(t0)
        #expect(apMarkItem("u1-jira-TODO-41", "later", store: s, clock: c).item?.later == t0)
        #expect(apOutboxStateLabel(s.items["u1-jira-TODO-41"]!) == "나중에")
        #expect(apMarkItem("u1-jira-TODO-41", "skip", store: s, clock: c).item?.state == .skipped)
        _ = post(s, "u1-jira-TODO-41", .auto, http: PubHTTP())
        #expect(s.items["u1-jira-TODO-41"]?.state == .skipped)
        var p = item(.confluence); p.state = .posted; _ = s.create(p)
        #expect(apMarkItem(p.id, "skip", store: s, clock: c).note == "이미 올렸습니다")
        #expect(apOutboxCounts([item(.jira), item(.jira), item(.confluence), item(.notion)]) == "Jira 댓글 2 · Confluence 1 · Notion 1")
    }
}
