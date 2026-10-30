// 알림 webhook·3시간 요약·끝 알림 흐름 시험(Notify.swift) — 2026-10-30 4단계 ⑩-3 에서 새로 씀.
// 실제 메신저로는 보내지 않는다: HTTP·키체인·시각은 가짜 어댑터(PubHTTP·PubSecrets·PubClock)다. 골든(Publish/notify-*.json)은
// 악성 견본(Report/evil-run.json)과 멘션 꼴 제목으로 만든 본문 그대로다 — 받는 쪽 꼴의 이스케이프·멘션 무력화·길이 상한을 고정한다
import Foundation
import Testing
@testable import AutopilotRules

private let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
private let seoul = TimeZone(identifier: "Asia/Seoul")!
private let t0 = 1_793_340_000.0
private let hookURL = "https://hooks.example.com/services/T0FAKE/B0FAKE/" + String(repeating: "z", count: 24)

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
private func pretty(_ d: Data) -> String {
    guard let o = try? JSONSerialization.jsonObject(with: d) else { return "(JSON 아님)\n" }
    return String(decoding: (try? JSONSerialization.data(withJSONObject: o, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])) ?? Data(), as: UTF8.self) + "\n"
}
private func evil() -> APReportRun {
    guard let d = try? Data(contentsOf: here.appendingPathComponent("Report/evil-run.json")),
          let j = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any] else { return APReportRun() }
    return apReportRun(id: j["id"] as? String ?? "?", source: FixtureSource(j)) ?? APReportRun()
}
// 멘션 꼴과 비밀이 든 실행 — 제목에 <!channel>·@here·<users/all>·가짜 webhook 주소·너비 없는 글자
private func mentionRun() -> APReportRun {
    var r = APReportRun()
    r.id = "20261030-1500-zz"; r.project = "zz <!channel> @here 팀\u{200B}"; r.state = "running"; r.model = "haiku"; r.started = t0
    var u = APReportUnit(); u.n = 1; u.title = "쿠폰 <users/all> @everyone " + hookURL; u.status = "running"
    var v = APReportUnit(); v.n = 2; v.title = "합계"; v.status = "done"
    r.units = [v, u]
    return r
}
private func hook(_ id: String, _ f: APNotifyFormat, name: String = "팀 대화방") -> APConnection {
    APConnection(id: id, service: .webhook, name: name, host: "hooks.example.com", scopes: ["notify"], format: f)
}

@Suite("알림(⑩-3)") struct NotifyTests {
    @Test("다듬기: 비밀 가리기·멘션 무력화·너비 없는 글자·긴 줄표·상한")
    func clean() {
        let s = apNotifyClean("@channel 보세요 — " + hookURL + " \u{200B}@here " + String(repeating: "가", count: 400))
        #expect(!s.contains("@"))
        #expect(s.contains("＠channel") && s.contains("＠here"))
        #expect(!s.contains("hooks.example.com/services"))
        #expect(!s.contains("—") && !s.unicodeScalars.contains { $0.value == 0x200B })
        #expect(s.count <= apNotifyLineMax + 1)
    }

    @Test("받는 쪽 꼴의 이스케이프", arguments: [
        (APNotifyFormat.slack, "<!here> & <@U1> <#C1|x>", "&lt;!here&gt; &amp; &lt;@U1&gt; &lt;#C1|x&gt;"),
        (APNotifyFormat.gchat, "<users/all> <https://x|y>", "＜users/all＞ ＜https://x|y＞"),
        (APNotifyFormat.json, "<b>x</b> & y", "＜b＞x＜/b＞ & y"),
    ])
    func escape(_ f: APNotifyFormat, _ input: String, _ want: String) {
        #expect(apNotifyEscape(input, f) == want)
    }

    @Test("본문 골든: 악성 견본 끝 알림·멘션 꼴 출발 알림을 꼴마다")
    func bodyGolden() {
        let e = apEndNotice(apNoticeRunFacts(evil()), waiting: 2, mergeWaiting: true)
        let s = apStartNotice(apNoticeRunFacts(mentionRun()), waiting: 0)
        for f in APNotifyFormat.allCases {
            let a = apNotifyBody(e, f), b = apNotifyBody(s, f)
            golden("notify-end-\(f.rawValue).json", pretty(a))
            golden("notify-start-\(f.rawValue).json", pretty(b))
            for d in [a, b] {
                let t = String(decoding: d, as: UTF8.self)
                #expect(!t.contains("<script") && !t.contains("<img") && !t.contains("<!channel") && !t.contains("<users/all"), "\(f)")
                #expect(!t.contains("@here") && !t.contains("@everyone"), "\(f)")
                #expect(!t.contains("hooks.example.com/services"), "\(f)")
                #expect(t.contains(apDocMarker))
                #expect(d.count < 4000)
            }
        }
        #expect(apNoticeText(e).hasPrefix(apDocMarker + "\n[자율주행] "))
    }

    @Test("글 상한 — 줄이 많아도 전체 1,800자 안")
    func textCap() {
        let n = APNotice(kind: .summary, id: "summary-1", headline: "머리", lines: (0..<30).map { _ in String(repeating: "가", count: 200) })
        #expect(apNoticeText(n).count <= apNotifyTextMax + 4)
    }

    @Test("상태 알림의 정한 때", arguments: [
        ("starting", "running", true, [String](), APNoticeKind?.some(.start)),
        ("", "running", true, [], .some(.start)),
        ("starting", "running", true, ["start"], nil),          // 다시 뜬 엔진은 두 번 보내지 않음
        ("running", "running", true, [], nil),                  // 단위가 바뀌어도
        ("running", "waiting_stopline", true, ["start"], .some(.wait)),
        ("running", "waiting_limit", true, ["start"], .some(.wait)),
        ("running", "waiting_precheck", true, ["start"], .some(.wait)),
        ("running", "waiting_stopline", false, ["start"], nil), // 다시 확인하며 오가는 대기는 단위를 시작하기 전까지 한 번
        ("waiting_stopline", "waiting_stopline", true, ["start"], nil),
        ("running", "paused", true, ["start"], nil),            // 사람이 한 일시정지는 알리지 않음
        ("paused", "running", true, ["start"], nil),
    ] as [(String, String, Bool, [String], APNoticeKind?)])
    func stateNotice(_ c: (String, String, Bool, [String], APNoticeKind?)) {
        #expect(apStateNotice(from: c.0, to: c.1, armed: c.2, sent: Set(c.3)) == c.4, "\(c.0) → \(c.1)")
    }

    @Test("끝 알림(webhook)은 실행마다 한 번")
    func endDue() {
        #expect(apEndWebhookDue(state: "done", sent: ["start"]))
        #expect(apEndWebhookDue(state: "error", sent: []))
        #expect(!apEndWebhookDue(state: "done", sent: ["end"]))
        #expect(!apEndWebhookDue(state: "running", sent: []))
    }

    @Test("3시간 요약의 정한 때", arguments: [
        (Double?.none, Double?.none, 0.0, false),               // 도는 실행이 없음
        (nil, t0, t0 + 3 * 3600 - 1, false),
        (nil, t0, t0 + 3 * 3600, true),                          // 출발 3시간 뒤 첫 요약
        (t0 + 3 * 3600, t0, t0 + 5 * 3600, false),
        (t0 + 3 * 3600, t0, t0 + 6 * 3600, true),
        (t0 - 10 * 3600, t0, t0 + 1 * 3600, false),              // 앞 요약이 지금 실행들보다 앞이면 출발부터 다시 센다
    ] as [(Double?, Double?, Double, Bool)])
    func summaryDue(_ c: (Double?, Double?, Double, Bool)) {
        #expect(apSummaryDue(last: c.0, since: c.1, now: c.2) == c.3)
    }

    @Test("3시간 요약 글 — 도는 실행이 없으면 보내지 않음, 올릴 글·합치기 기다림·만료 임박")
    func summaryText() {
        #expect(apSummaryNotice([], waiting: 3, merge: [], expiring: [], k: 1, at: t0, tz: seoul) == nil)
        let exp = apExpiringLines([APConnection(id: "atlassian", service: .atlassian, expires: "2026-11-04"), hook("webhook-1", .json)],
                                  now: Date(timeIntervalSince1970: t0), tz: seoul)
        #expect(exp == ["Atlassian 토큰 만료 임박: 5일 남음"])
        let n = apSummaryNotice([apNoticeRunFacts(mentionRun())], waiting: 3, merge: [APMergeWait(name: "todo-api <b>", days: 2)], expiring: exp, k: 4, at: t0, tz: seoul)!
        #expect(n.id == "summary-4")
        #expect(n.lines.contains("검수를 기다리는 올릴 글 3개(보드의 '올릴 글'에서 봅니다)"))
        #expect(n.lines.contains { $0.hasPrefix("합치기 기다림: todo-api") && $0.hasSuffix("2일") })
        #expect(n.lines.contains("Atlassian 토큰 만료 임박: 5일 남음"))
        #expect(n.lines[0].contains("도는 중 · 지금 쿠폰") && n.lines[0].contains("끝난 단위 1/2"))
        golden("notify-summary-slack.json", pretty(apNotifyBody(n, .slack)))
    }

    @Test("받을 webhook — 게시 설정의 notify 가운데 알림 범위가 있는 webhook 연결만")
    func targets() {
        var noScope = hook("webhook-3", .json); noScope.scopes = []
        let cs = [hook("webhook-1", .slack), hook("webhook-2", .gchat), noScope, APConnection(id: "atlassian", service: .atlassian, scopes: ["jira"])]
        #expect(apNotifyTargets(["webhook-2", "webhook-3", "webhook-9", "atlassian"], connections: cs).map(\.id) == ["webhook-2"])
        #expect(apNotifyTargets(nil, connections: cs).isEmpty)
    }

    @Test("보내기: 꼴마다 본문, 주소는 키체인에서 보내기 직전에, 인증 머리 없음, 결과에 주소 없음")
    func send() {
        let clock = PubClock(t0), w = PubWaiter(clock)
        let sec = PubSecrets(["webhook-1": hookURL, "webhook-2": "http://127.0.0.1:9/hook/b", "webhook-3": "http://example.com/hook"])
        let h = PubHTTP([APHTTPResponse(status: 429, headers: ["retry-after": "1"]), APHTTPResponse(status: 200), APHTTPResponse(status: 302, headers: ["location": "https://evil.example/x"])])
        let n = apTestNotice(name: "팀 대화방", at: t0, tz: seoul)
        let r = apSendNotice(n, to: [hook("webhook-1", .slack), hook("webhook-2", .gchat, name: "시험방"), hook("webhook-3", .json), hook("webhook-4", .json)],
                             dev: true, secrets: sec, http: h, clock: clock, waiter: w)
        #expect(r.map(\.ok) == [true, false, false, false])
        #expect(r[0].requests == 2 && w.waits == [1])
        #expect(r[1].note.contains("따라가지 않았습니다(302)"))
        #expect(r[2].note.contains("규칙에 맞지 않아") && r[2].requests == 0)     // 시험 빌드라도 http 는 127.0.0.1 만
        #expect(r[3].note.contains("키체인에 없어") && r[3].requests == 0)
        #expect(h.got.count == 3 && h.got.allSatisfy { $0.method == "POST" })
        #expect(h.auths.allSatisfy { $0.isEmpty })
        #expect(String(decoding: h.got[0].body ?? Data(), as: UTF8.self).contains("\"unfurl_links\":false"))
        for s in r { #expect(!s.note.contains("hooks.example.com") && !s.note.contains("127.0.0.1") && !s.note.contains("example.com")) }
        // 설치판은 http 를 늘 거절
        let r2 = apSendNotice(n, to: [hook("webhook-2", .gchat)], dev: false, secrets: sec, http: PubHTTP(), clock: clock, waiter: w)
        #expect(r2[0].requests == 0)
    }

    @Test("끝 알림을 누르면 무엇을 여나", arguments: [
        (2, true, APEndOpen.review), (1, false, .review), (0, true, .merge), (0, false, .history),
    ])
    func endOpen(_ w: Int, _ m: Bool, _ want: APEndOpen) {
        #expect(apEndOpen(waiting: w, mergeWaiting: m) == want)
    }

    @Test("끝 알림을 띄울 때 — 실행마다 한 번, 처음 띄울 때 이미 끝난 옛 실행은 아님", arguments: [
        (Double?.some(t0 + 10), true, false, t0, true),
        (.some(t0 + 10), true, true, t0, false),
        (.some(t0 - 10), true, false, t0, false),
        (.some(t0 + 10), false, false, t0, false),       // 끝 정리 전
        (nil, true, false, t0, false),
    ] as [(Double?, Bool, Bool, Double, Bool)])
    func endNoticeDue(_ c: (Double?, Bool, Bool, Double, Bool)) {
        #expect(apEndNoticeDue(ended: c.0, wrapped: c.1, notified: c.2, seeded: c.3) == c.4)
    }

    @Test("나중에 뒤 하루 지나 다시 오는 합치기 알림", arguments: [
        (Double?.some(t0), false, true, t0 + 1, true),
        (.some(t0), false, true, t0 - 1, false),
        (.some(t0), true, true, t0 + 1, false),          // 한 번만
        (.some(t0), false, false, t0 + 1, false),        // 그사이 합침·버림
        (nil, false, true, t0 + 1, false),
    ] as [(Double?, Bool, Bool, Double, Bool)])
    func reminder(_ c: (Double?, Bool, Bool, Double, Bool)) {
        #expect(apMergeReminderDue(later: c.0, reminded: c.1, mergeWaiting: c.2, now: c.3) == c.4)
    }

    @Test("기다리는 올릴 글 수")
    func waiting() {
        var a = APOutboxItem(); a.state = .waiting
        var b = APOutboxItem(); b.state = .posted
        var c = APOutboxItem(); c.state = .failed
        var d = APOutboxItem(); d.state = .skipped
        var e = APOutboxItem(); e.state = .uncertain
        #expect(apOutboxWaiting([a, b, c, d, e]) == 3)
    }

    @Test("끝난 실행이 합치기를 기다리는가(요약 줄)")
    func runMergeOpen() {
        let br: [String: Any] = ["repos": ["/w/todo": ["work": "autopilot/x"]]]
        let ended: [String: Any] = ["state": "done", "wrapped": t0, "branches": br]
        let promo: [[String: Any]] = [["from": "autopilot", "into": "dev"], ["from": "autopilot", "into": "live", "after": "dev"]]
        #expect(apRunMergeOpen(run: ended, queue: [[:]], promotion: promo))
        var merged = ended
        merged["branches"] = ["repos": br["repos"]!, "promoted": [["source": "autopilot", "into": "dev"], ["source": "autopilot", "into": "live"]]]
        #expect(!apRunMergeOpen(run: merged, queue: [[:]], promotion: promo))
        var running = ended; running["state"] = "running"
        #expect(!apRunMergeOpen(run: running, queue: [[:]], promotion: promo))
        var noBranch = ended; noBranch["branches"] = [:] as [String: Any]
        #expect(!apRunMergeOpen(run: noBranch, queue: [[:]], promotion: promo))
    }

    @Test("게시 설정의 notify 와 연결의 글 꼴")
    func settings() {
        let (p, probs) = apPublishParse(["mode": "review", "notify": ["webhook-2", "webhook-1", "webhook-2"]])
        #expect(probs.isEmpty && p.notify == ["webhook-2", "webhook-1"])
        #expect(apPublishParse(apPublishJSON(p)).publish == p)
        #expect(apPublishParse(["notify": ["atlassian"]]).problems.count == 1)
        #expect(apPublishParse(["notify": "webhook-1"]).problems.count == 1)
        #expect(apPublishParse(["mode": "auto"]).publish.notify == nil)
        let list = [hook("webhook-1", .slack), hook("webhook-2", .gchat), APConnection(id: "atlassian", service: .atlassian)]
        let back = apParseConnections(try? JSONSerialization.data(withJSONObject: apConnectionsJSON(list)))
        #expect(back.map(\.format) == [.slack, .gchat, .json])
        var raw = apConnectionsJSON([hook("webhook-1", .json)])
        var cs = raw["connections"] as! [[String: Any]]; cs[0]["format"] = "teams"; raw["connections"] = cs
        #expect(apParseConnections(try? JSONSerialization.data(withJSONObject: raw)).first?.format == .json)
    }
}
