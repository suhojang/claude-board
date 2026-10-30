// 자율주행 규칙(AutopilotRules) — 알림 webhook 의 상태 알림·3시간 요약과 끝 알림 흐름(4단계 ⑩-3, 2026-10-30, 설계 5장 6절 "알림").
// 보내는 쪽은 엔진이다(보드 창이 꺼져 있어도, ⑩-1 의 자동으로 올림과 같은 까닭): 상태 알림은 그 실행의 엔진이 상태가 바뀔 때, 3시간 요약은
// 도는 실행의 엔진이 뿌리의 요약 기록(잠금)으로 하나만 보낸다. 보드는 HTTP 를 하지 않고 "시험 글 보내기"도 앱 안 엔진 명령을 부른다.
// 정한 때에만: 출발(처음 도는 때), 대기에 들어감(단위를 시작한 뒤 첫 대기만 — 다시 확인하며 대기를 오가도 한 번), 끝(실행마다 한 번), 3시간 요약.
// 글 꼴은 받는 쪽(webhook 연결)마다: 일반 JSON·Slack 꼴·Google Chat 꼴. 바깥·모델이 쓴 글(실행·단위 제목, 엔진 까닭에 든 제목)은
// 비밀 가리기(apRedact) → 다듬기(apDocText: 제어·방향 글자·긴 줄표) → 멘션 무력화(@ → 전각 ＠) → 받는 쪽 꼴의 이스케이프 → 길이 상한을 거친다.
// webhook 주소는 비밀(키체인)이라 이 파일의 값·기록·글 어디에도 넣지 않는다 — 보낸 결과는 연결 id·이름으로만 남긴다
import Foundation

// MARK: - 받는 쪽 꼴

public enum APNotifyFormat: String, CaseIterable, Equatable, Sendable {
    case json       // 일반 JSON — {text, event, run, …}(받는 쪽이 text 를 보여 주는 대화방은 대개 이것으로 된다)
    case slack      // Slack 꼴 — {text}(mrkdwn, & < > 는 엔티티), 미리보기 끔
    case gchat      // Google Chat 꼴 — {text}
    public var label: String {
        switch self {
        case .json: "일반 JSON"
        case .slack: "Slack 꼴"
        case .gchat: "Google Chat 꼴"
        }
    }
}

public let apNotifyLineMax = 200        // 한 줄 상한
public let apNotifyTextMax = 1800       // 글 전체 상한
public let apNotifyLinesMax = 12        // 줄 수 상한(머리 아래)

// 바깥·모델 글 다듬기 — 가리기 → 다듬기 → 멘션 무력화. 꼴의 이스케이프는 글을 만들 때(apNotifyEscape)
public func apNotifyClean(_ s: String, _ n: Int = apNotifyLineMax) -> String {
    let red = apRedact(s).text
    // 너비 없는 글자(멘션을 쪼개 숨기는 데 쓰임)도 지운다. @ 를 전각으로 바꾸면 @channel·@here·@everyone·@all 이 멘션으로 읽히지 않는다
    let z = String(String.UnicodeScalarView(red.unicodeScalars.filter { ![0x200B, 0x200C, 0x200D, 0x2060].contains($0.value) }))
    return apDocText(z, n).replacingOccurrences(of: "@", with: "＠")
}

// 받는 쪽 꼴의 이스케이프 — Slack 은 공식 엔티티(<!here>·<@U…>·<#C…> 같은 꼴을 죽인다), Google Chat·일반 JSON 은 꺾쇠를 전각으로(<users/all> 등)
public func apNotifyEscape(_ s: String, _ f: APNotifyFormat) -> String {
    switch f {
    case .slack: return s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
    case .gchat, .json: return s.replacingOccurrences(of: "<", with: "＜").replacingOccurrences(of: ">", with: "＞")
    }
}

// MARK: - 알림 하나

public enum APNoticeKind: String, Equatable, Sendable { case start, wait, end, summary, test }

public struct APNotice: Equatable, Sendable {
    public var kind = APNoticeKind.test
    public var id = ""                      // 다시 보내지 않으려는 이름(start·end·wait-<k>·summary-<k>·test)
    public var headline = ""                // 표지 다음 줄(다듬은 글)
    public var lines: [String] = []         // 그 아래 줄(다듬은 글)
    public var run = ""                     // 실행 id(요약·시험은 빈 글자)
    public var fields: [String: String] = [:]   // 일반 JSON 꼴에 더 넣는 칸(다듬은 글)
    public init(kind: APNoticeKind = .test, id: String = "", headline: String = "", lines: [String] = [], run: String = "", fields: [String: String] = [:]) {
        self.kind = kind; self.id = id; self.headline = headline; self.lines = lines; self.run = run; self.fields = fields
    }
}

// 글 — 첫 줄 표지(오토파일럿 작성), 머리, 줄마다 "- ". 전체 상한에서 줄 단위로 자른다
public func apNoticeText(_ n: APNotice) -> String {
    var out = apDocMarker + "\n" + n.headline
    for l in n.lines.prefix(apNotifyLinesMax) {
        let add = "\n- " + l
        if out.count + add.count > apNotifyTextMax { out += "\n- …"; break }
        out += add
    }
    return out
}

private func jsonData(_ o: Any) -> Data { (try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys, .withoutEscapingSlashes])) ?? Data() }

// 보낼 본문 — 받는 쪽 꼴마다
public func apNotifyBody(_ n: APNotice, _ f: APNotifyFormat) -> Data {
    let text = apNotifyEscape(apNoticeText(n), f)
    switch f {
    case .slack: return jsonData(["text": text, "unfurl_links": false, "unfurl_media": false])
    case .gchat: return jsonData(["text": text])
    case .json:
        var o: [String: Any] = ["source": "claude-board-autopilot", "marker": apDocMarker, "event": n.kind.rawValue, "id": n.id, "text": text,
                                "title": apNotifyEscape(n.headline, .json), "lines": n.lines.prefix(apNotifyLinesMax).map { apNotifyEscape($0, .json) }]
        if !n.run.isEmpty { o["run"] = n.run }
        for (k, v) in n.fields where o[k] == nil { o[k] = apNotifyEscape(v, .json) }
        return jsonData(o)
    }
}

// MARK: - 알림 글 만들기

// 실행 하나의 알림 재료 — 보고서 값에서(엔진은 메모리의 run·queue 로 만든다)
public struct APNoticeRun: Equatable, Sendable {
    public var id = "", name = "", state = "", reason = "", model = ""
    public var total = 0, done = 0, failed = 0, capped = 0, skipped = 0, stopped = 0, finished = 0
    public var current = ""                 // 지금 도는 단위 제목
    public var started: Double?
    public init() {}
}

public func apNoticeRunFacts(_ r: APReportRun) -> APNoticeRun {
    var f = APNoticeRun()
    f.id = r.id
    f.name = apNotifyClean(!r.project.isEmpty ? r.project : (!r.title.isEmpty ? r.title : r.id), 80)
    f.state = r.state; f.reason = apNotifyClean(r.reason, 160); f.model = apNotifyClean(r.model, 40)
    f.total = r.units.count
    let k = { (s: String) in r.units.filter { $0.status == s }.count }
    f.done = k("done"); f.failed = k("failed"); f.capped = k("capped"); f.skipped = k("skipped"); f.stopped = k("stopped")
    f.finished = f.done + f.failed + f.capped + f.skipped + f.stopped
    if let u = r.units.first(where: { $0.status == "running" }) { f.current = apNotifyClean(u.title.isEmpty ? "단위 \(u.n)" : u.title, 80) }
    f.started = r.started
    return f
}

private func result(_ f: APNoticeRun) -> String {
    var s = "단위 \(f.total)개 가운데 통과 \(f.done)"
    if f.failed > 0 { s += " · 미통과 \(f.failed)" }
    if f.capped > 0 { s += " · 상한 \(f.capped)" }
    if f.skipped > 0 { s += " · 건너뜀 \(f.skipped)" }
    if f.stopped > 0 { s += " · 중단 \(f.stopped)" }
    return s
}
private func fields(_ f: APNoticeRun, waiting: Int) -> [String: String] {
    ["state": f.state, "name": f.name, "units_total": "\(f.total)", "units_done": "\(f.done)", "units_failed": "\(f.failed)",
     "units_capped": "\(f.capped)", "outbox_waiting": "\(waiting)"]
}
private func waitingLine(_ n: Int) -> String? { n > 0 ? "검수를 기다리는 올릴 글 \(n)개(보드의 '올릴 글'에서 봅니다)" : nil }

public func apStartNotice(_ f: APNoticeRun, waiting: Int) -> APNotice {
    var l = ["단위 \(f.total)개를 차례로 합니다" + (f.model.isEmpty ? "" : " · 모델 \(f.model)")]
    if let w = waitingLine(waiting) { l.append(w) }
    l.append("실행 \(f.id)")
    return APNotice(kind: .start, id: "start", headline: "[자율주행] \(f.name) 출발했습니다", lines: l, run: f.id, fields: fields(f, waiting: waiting))
}

public func apWaitNotice(_ f: APNoticeRun, waiting: Int, k: Int) -> APNotice {
    var l = [f.reason.isEmpty ? "새 단위를 시작하지 않고 기다립니다" : f.reason, result(f) + " · 끝난 단위 \(f.finished)"]
    if let w = waitingLine(waiting) { l.append(w) }
    l.append("실행 \(f.id)")
    return APNotice(kind: .wait, id: "wait-\(k)", headline: "[자율주행] \(f.name) 쉬는 중", lines: l, run: f.id, fields: fields(f, waiting: waiting))
}

public func apEndNotice(_ f: APNoticeRun, waiting: Int, mergeWaiting: Bool) -> APNotice {
    let how: String
    switch f.state {
    case "done": how = "끝났습니다"
    case "error": how = "멈췄습니다(엔진 오류)"
    default: how = "멈췄습니다(사람이 중단)"
    }
    var l = [result(f)]
    if f.state != "done", !f.reason.isEmpty { l.append(f.reason) }
    if let w = waitingLine(waiting) { l.append(w) }
    if mergeWaiting { l.append("합치기·승격을 기다립니다(보드에서 바뀐 줄을 보고 합칩니다)") }
    l.append("실행 \(f.id)")
    return APNotice(kind: .end, id: "end", headline: "[자율주행] \(f.name) \(how)", lines: l, run: f.id, fields: fields(f, waiting: waiting))
}

// 합치기를 기다리는 끝난 실행(요약 줄) — 이름과 끝난 지 며칠
public struct APMergeWait: Equatable, Sendable {
    public var name = ""
    public var days = 0
    public init(name: String = "", days: Int = 0) { self.name = name; self.days = days }
}

private func clock(_ ts: Double, _ tz: TimeZone) -> String {
    let d = DateFormatter()
    d.locale = Locale(identifier: "ko_KR"); d.timeZone = tz; d.calendar = Calendar(identifier: .gregorian); d.dateFormat = "M월 d일 HH:mm"
    return d.string(from: Date(timeIntervalSince1970: ts))
}

// 3시간 요약 — 도는 실행마다 한 줄, 기다리는 올릴 글 수, 합치기 기다림 n일, 토큰 만료 임박. 도는 실행이 없으면 nil(보내지 않음)
public func apSummaryNotice(_ runs: [APNoticeRun], waiting: Int, merge: [APMergeWait], expiring: [String], k: Int, at: Double, tz: TimeZone) -> APNotice? {
    guard !runs.isEmpty else { return nil }
    var l: [String] = []
    for f in runs.prefix(6) {
        let st: String
        switch f.state {
        case "running": st = f.current.isEmpty ? "도는 중" : "도는 중 · 지금 \(f.current)"
        case "paused": st = "일시정지"
        case "waiting_limit": st = "사용량 한도로 쉬는 중"
        case "waiting_stopline": st = "멈출 선에서 쉬는 중"
        case "waiting_precheck": st = "시험 환경을 기다리는 중"
        default: st = f.state
        }
        l.append("\(f.name): \(st) · 끝난 단위 \(f.finished)/\(f.total)(통과 \(f.done))")
    }
    if let w = waitingLine(waiting) { l.append(w) }
    for m in merge.prefix(3) { l.append("합치기 기다림: \(apNotifyClean(m.name, 80)) \(m.days)일") }
    for e in expiring.prefix(3) { l.append(apNotifyClean(e, 120)) }
    return APNotice(kind: .summary, id: "summary-\(k)", headline: "[자율주행] 3시간 요약 · \(clock(at, tz))", lines: l,
                    fields: ["runs": "\(runs.count)", "outbox_waiting": "\(waiting)", "merge_waiting": "\(merge.count)"])
}

public func apTestNotice(name: String, at: Double, tz: TimeZone) -> APNotice {
    APNotice(kind: .test, id: "test", headline: "[자율주행] 시험 글", lines: ["\(apNotifyClean(name, 80)) 대화방으로 보드의 알림이 닿는지 보는 시험 글입니다", "보낸 때 \(clock(at, tz))"])
}

// 토큰 만료 임박 줄(요약) — 쓰기·읽기 연결의 만료일이 14일 안이거나 지났을 때
public func apExpiringLines(_ connections: [APConnection], now: Date, tz: TimeZone) -> [String] {
    connections.filter { $0.service != .webhook }.compactMap { c in
        switch apExpiry(c.expires, now: now, tz: tz) {
        case .soon(let d): return "\(c.service.label) 토큰 만료 임박: \(d == 0 ? "오늘 만료" : "\(d)일 남음")"
        case .expired(let d): return "\(c.service.label) 토큰 만료됨: \(d)일 지남(바깥 글은 쌓이고 일은 계속합니다)"
        default: return nil
        }
    }
}

// MARK: - 정한 때

public let apSummaryEvery = 3.0 * 3600

// 상태가 바뀔 때 보낼 알림 — 출발(처음 도는 때), 대기에 들어감(armed: 단위를 시작한 뒤 아직 대기 알림을 안 보냄). 끝은 apEndNoticeDue
public func apStateNotice(from old: String, to new: String, armed: Bool, sent: Set<String>) -> APNoticeKind? {
    if new == "running", old == "starting" || old.isEmpty, !sent.contains("start") { return .start }
    if new.hasPrefix("waiting_"), !old.hasPrefix("waiting_"), armed { return .wait }
    return nil
}

// 끝 알림(webhook) — 끝난 상태이고 아직 보내지 않았을 때 한 번
public func apEndWebhookDue(state: String, sent: Set<String>) -> Bool { apRunEnded(state) && !sent.contains("end") }

// 3시간 요약을 보낼 때인가 — since 는 지금 도는 실행 가운데 가장 이른 출발(없으면 보내지 않음), last 는 마지막 요약.
// 첫 요약은 출발 3시간 뒤, 그 뒤로는 3시간마다(앞 요약이 지금 도는 실행들보다 앞이면 출발부터 다시 센다)
public func apSummaryDue(last: Double?, since: Double?, now: Double) -> Bool {
    guard let since else { return false }
    return now - max(last ?? 0, since) >= apSummaryEvery
}

// 받을 webhook — 게시 설정의 notify 가운데 알림 범위가 있는 webhook 연결만(없는 id 는 빼고)
public func apNotifyTargets(_ ids: [String]?, connections: [APConnection]) -> [APConnection] {
    (ids ?? []).compactMap { id in connections.first { $0.id == id && $0.service == .webhook && $0.scopes.contains("notify") } }
}

// MARK: - 보내기 유스케이스

public struct APNotifySent: Equatable, Sendable {
    public var id = "", name = ""
    public var ok = false
    public var code = 0
    public var note = ""
    public var requests = 0
    public init(id: String = "", name: String = "", ok: Bool = false, code: Int = 0, note: String = "", requests: Int = 0) {
        self.id = id; self.name = name; self.ok = ok; self.code = code; self.note = note; self.requests = requests
    }
}

// 실패 까닭 — 상태 번호로만(받는 쪽 본문·주소는 남기지 않는다)
public func apNotifyFailNote(_ r: APHTTPResponse) -> String {
    if let e = r.error { return e }
    switch r.status {
    case 300...399: return "다른 주소로 보내려 해 따라가지 않았습니다(\(r.status))"
    case 400: return "받는 쪽이 글 꼴을 받지 않았습니다(400): 연결의 글 꼴(일반 JSON·Slack·Google Chat)을 확인합니다"
    case 401, 403: return "거부됨(\(r.status)): webhook 주소가 취소됐거나 권한이 없습니다"
    case 404: return "받는 쪽에 webhook 이 없습니다(404)"
    case 429: return "요청이 많다며 거절했습니다(429)"
    case 500...599: return "받는 쪽 오류(\(r.status))"
    default: return "받는 쪽이 \(r.status) 로 답했습니다"
    }
}

// 알림 하나를 webhook 마다 보낸다(429·503 은 ⑩-1 과 같은 기다림, 모두 합쳐 budget 초 안). 주소는 보내기 직전에 키체인에서 읽고 규칙을 다시 본다
public func apSendNotice(_ n: APNotice, to targets: [APConnection], dev: Bool, secrets: APSecrets, http: APHTTP, clock: APClock,
                         waiter: APWaiter, budget: Double = 20) -> [APNotifySent] {
    let start = clock.now.timeIntervalSince1970
    var out: [APNotifySent] = []
    for c in targets {
        var s = APNotifySent(id: c.id, name: c.name.isEmpty ? c.id : c.name)
        guard let url = secrets.read(c.id), !url.isEmpty else { s.note = "webhook 주소가 키체인에 없어 보내지 않았습니다(요청 0)"; out.append(s); continue }
        guard case .ok = apWebhookCheck(url, dev: dev) else { s.note = "webhook 주소가 규칙에 맞지 않아 보내지 않았습니다(요청 0)"; out.append(s); continue }
        guard clock.now.timeIntervalSince1970 < start + budget else { s.note = "시간 상한(\(Int(budget))초)이 지나 보내지 않았습니다(요청 0)"; out.append(s); continue }
        let req = APHTTPRequest(method: "POST", url: url, headers: ["Content-Type": "application/json; charset=utf-8"], body: apNotifyBody(n, c.format))
        var attempt = 0, resp = APHTTPResponse()
        while true {
            s.requests += 1
            resp = http.send(req, auth: nil)
            guard let w = apRetryWait(status: resp.status, retryAfter: resp.headers["retry-after"], attempt: attempt, now: clock.now),
                  clock.now.timeIntervalSince1970 + w <= start + budget else { break }
            waiter.wait(w)
            attempt += 1
        }
        s.code = resp.status
        if resp.error == nil, (200...299).contains(resp.status) { s.ok = true; s.note = "보냈습니다" } else { s.note = apNotifyFailNote(resp) }
        out.append(s)
    }
    return out
}

// MARK: - 끝 알림 흐름(보드)

// 끝 알림을 누르면 무엇을 여나 — 기다리는 올릴 글이 있으면 검수(칸 위 실행 줄의 "합치기·승격 →"으로 잇는다), 없고 합치기가 남았으면 승격 화면,
// 둘 다 없으면 지난 실행 상세(설계 5장 6절, 2026-10-22 사용자 결정과 설계 판단)
public enum APEndOpen: String, Equatable, Sendable { case review, merge, history }
public func apEndOpen(waiting: Int, mergeWaiting: Bool) -> APEndOpen {
    waiting > 0 ? .review : (mergeWaiting ? .merge : .history)
}

// 기다리는 올릴 글 — 검수 기다림·올리지 못함·확인 필요(보드 검수 화면의 "기다림"과 같은 기준)
public func apOutboxWaiting(_ items: [APOutboxItem]) -> Int {
    items.filter { [.waiting, .failed, .uncertain].contains($0.state) }.count
}

// 끝 알림을 띄울 때인가 — 끝 정리를 마친 실행마다 한 번. 이 판의 보드가 처음 뜬 때(seeded) 앞에 끝난 실행은 띄우지 않는다
// (처음 띄울 때 지난 실행 수십 개의 알림이 한꺼번에 뜨지 않게). 보드가 꺼져 있던 동안 끝난 실행은 다음에 뜰 때 한 번
public func apEndNoticeDue(ended: Double?, wrapped: Bool, notified: Bool, seeded: Double) -> Bool {
    guard wrapped, !notified, let e = ended else { return false }
    return e >= seeded
}

// "나중에" 뒤 하루 지나 다시 오는 합치기 알림 — 한 번만, 그때도 합치기가 남았을 때
public func apMergeReminderDue(later: Double?, reminded: Bool, mergeWaiting: Bool, now: Double) -> Bool {
    guard let l = later, !reminded, mergeWaiting else { return false }
    return now >= l
}

// 끝난 실행이 합치기·승격을 기다리는가(엔진의 3시간 요약 줄) — 보드 APRun.parse 와 같은 기준: 끝 정리를 마쳤고 가지 기록이 있는 실행에
// apMergeOpen(승격 차례는 프로젝트 설정 promotion, Jira 실행은 스토리 없는 단위가 있을 때만 자율주행 가지도 합칠 쪽)
public func apRunMergeOpen(run: [String: Any], queue: [[String: Any]], promotion: Any?) -> Bool {
    guard apRunEnded(run["state"] as? String ?? ""), run["wrapped"] != nil else { return false }
    let br = run["branches"] as? [String: Any] ?? [:]
    guard !(br["repos"] as? [String: Any] ?? [:]).isEmpty else { return false }
    let steps = (promotion as? [[String: Any]] ?? []).compactMap { s in
        (s["from"] as? String).flatMap { f in (s["into"] as? String).map { APStep(from: f, into: $0, after: s["after"] as? String) } }
    }
    let jira = queue.contains { $0["story"] != nil }
    return apMergeOpen(br, steps: steps, stories: jira ? queue.contains { $0["story"] == nil } : true)
}
