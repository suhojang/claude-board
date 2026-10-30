// 자율주행 규칙(AutopilotRules) — 올릴 글(outbox)과 게시 방식(4단계 ⑩-1, 2026-10-30, 설계 5장 2절 outbox/·5장 6절).
// 엔진이 단위 판정 뒤(Jira 댓글)·끝 정리 때(보고서)에 실행 폴더 outbox/<글 id>.json 으로 글을 만들고, 게시 방식이 "자동으로 올림"이면 바로 올린다.
// "검수 뒤 올림"(기본)이면 글은 기다리고, 보드 검수 화면에서 사람이 누를 때만 올린다(보드는 앱 안 엔진의 outbox 명령을 부른다).
// 여기에는 순수한 것만 둔다: 게시 설정 읽기, 어떤 글을 만들지, 글 본문(APDoc), 파일 꼴, 올릴지 판정, 요청 꼴과 응답 해석, 429 다시 보내기,
// 그리고 포트(APOutboxStore·APHTTP·APSecrets·APClock·APWaiter)로만 바깥을 부르는 올리기 유스케이스 apPostItem.
// 다시 눌러도 하나만 생기게: 요청을 보내기 전에 글을 "올리는 중"(posting)으로 쓰고, 단계마다 받은 곳 id 를 바로 적고, 끝나면 "올림"(posted).
// 잠금 안에서 posting 을 보면 앞 올리기가 끝나지 못하고 죽은 것이라 "확인 필요"(uncertain)로 바꾸고, 그 글은 자동으로 다시 올리지 않는다
import Foundation

// MARK: - 게시 설정(프로젝트 설정 publish 칸 → run.json publish → 고정 설정)

public enum APPublishMode: String, Equatable, Sendable { case review, auto }
public enum APTargetService: String, Equatable, Sendable { case jira, confluence, notion }

// 받을 곳 — Jira 는 이슈, Confluence 는 스페이스(와 상위 페이지, 없으면 스페이스 맨 위), Notion 은 상위 페이지
public struct APOutboxTarget: Equatable, Sendable {
    public var service = APTargetService.confluence
    public var issue = ""
    public var space = ""
    public var parent = ""
    public init(service: APTargetService = .confluence, issue: String = "", space: String = "", parent: String = "") {
        self.service = service; self.issue = issue; self.space = space; self.parent = parent
    }
}

public struct APPublish: Equatable, Sendable {
    public var mode = APPublishMode.review     // 새 프로젝트 기본은 검수 뒤 올림(사용자 결정)
    public var report: APOutboxTarget?          // 보고서 받을 곳(confluence·notion), 없으면 보고서 글을 만들지 않음
    public var jiraComments = false             // 단위 티켓에 결과 댓글
    public init(mode: APPublishMode = .review, report: APOutboxTarget? = nil, jiraComments: Bool = false) {
        self.mode = mode; self.report = report; self.jiraComments = jiraComments
    }
    public var makesAny: Bool { report != nil || jiraComments }
}

public let apIssueKeyPattern = "^[A-Z][A-Z0-9_]*-[0-9]{1,9}$"
public let apSpaceKeyPattern = "^~?[A-Za-z0-9_-]{1,100}$"
public let apConfluenceIDPattern = "^[0-9]{1,20}$"
public let apNotionIDPattern = "^[0-9a-fA-F]{8}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{12}$"

func apMatch(_ s: String, _ p: String) -> Bool { s.range(of: p, options: .regularExpression) != nil }

// 게시 설정 읽기 — 없으면 기본(검수 뒤 올림, 받을 곳 없음). 꼴이 틀린 칸은 까닭을 남기고 그 칸은 쓰지 않는다
public func apPublishParse(_ v: Any?) -> (publish: APPublish, problems: [String]) {
    var p = APPublish(), probs: [String] = []
    guard let v, !(v is NSNull) else { return (p, []) }
    guard let o = v as? [String: Any] else { return (p, ["publish 는 객체여야 합니다"]) }
    if let m = o["mode"] {
        if let s = m as? String, let mode = APPublishMode(rawValue: s) { p.mode = mode } else { probs.append("publish.mode 는 review 또는 auto 입니다") }
    }
    if let j = o["jira_comments"] {
        if let b = j as? Bool { p.jiraComments = b } else { probs.append("publish.jira_comments 는 true/false 입니다") }
    }
    if let r = o["report"], !(r is NSNull) {
        let d = r as? [String: Any] ?? [:]
        let space = (d["space"] as? String ?? "").trimmingCharacters(in: .whitespaces)
        let parent = (d["parent"] as? String ?? "").trimmingCharacters(in: .whitespaces)
        switch d["service"] as? String {
        case "confluence":
            if !apMatch(space, apSpaceKeyPattern) { probs.append("Confluence 스페이스 키 꼴이 아닙니다: \(space.prefix(40))") }
            else if !parent.isEmpty, !apMatch(parent, apConfluenceIDPattern) { probs.append("Confluence 상위 페이지 id 는 숫자입니다: \(parent.prefix(40))") }
            else { p.report = APOutboxTarget(service: .confluence, space: space, parent: parent) }
        case "notion":
            if !apMatch(parent, apNotionIDPattern) { probs.append("Notion 상위 페이지 id 꼴이 아닙니다(32자리 16진수): \(parent.prefix(40))") }
            else { p.report = APOutboxTarget(service: .notion, parent: parent.replacingOccurrences(of: "-", with: "").lowercased()) }
        default: probs.append("publish.report.service 는 confluence 또는 notion 입니다")
        }
    }
    return (p, probs)
}

public func apPublishJSON(_ p: APPublish) -> [String: Any] {
    var d: [String: Any] = ["mode": p.mode.rawValue, "jira_comments": p.jiraComments]
    if let r = p.report {
        var t: [String: Any] = ["service": r.service.rawValue]
        if !r.space.isEmpty { t["space"] = r.space }
        if !r.parent.isEmpty { t["parent"] = r.parent }
        d["report"] = t
    } else { d["report"] = NSNull() }
    return d
}

// 받을 곳을 쓰는 연결과 그 범위
public func apTargetConnection(_ s: APTargetService) -> (id: String, scope: String) {
    switch s {
    case .jira: ("atlassian", "jira")
    case .confluence: ("atlassian", "confluence")
    case .notion: ("notion", "write")
    }
}

public func apTargetLabel(_ t: APOutboxTarget) -> String {
    switch t.service {
    case .jira: return "Jira 댓글 · \(t.issue)"
    case .confluence: return "Confluence · 스페이스 \(t.space)" + (t.parent.isEmpty ? " 맨 위" : " › 상위 페이지 \(t.parent)") + " › 새 페이지"
    case .notion: return "Notion · 상위 페이지 \(t.parent.prefix(8))… › 새 페이지"
    }
}

// 출발 확인 "게시 방식" 줄의 알림 — 받을 곳이 있는데 연결·범위·토큰이 없거나 만료면 "글은 outbox 에 쌓입니다". 출발은 막지 않는다
public func apPublishNotes(_ p: APPublish, connections: [APConnection], secrets: Set<String>, dev: Bool, now: Date, tz: TimeZone) -> [String] {
    var out: [String] = []
    var targets: [APTargetService] = []
    if let r = p.report { targets.append(r.service) }
    if p.jiraComments { targets.append(.jira) }
    for s in targets {
        let need = apTargetConnection(s)
        let name = s == .jira ? "Jira 댓글" : s == .confluence ? "Confluence 보고서" : "Notion 보고서"
        guard let c = connections.first(where: { $0.id == need.id }) else {
            out.append("\(name): \(need.id == "notion" ? "Notion" : "Atlassian") 연결이 없습니다. 요청은 0건이고 글은 올리지 않은 채 쌓입니다(연결한 뒤 검수 화면에서 올림)"); continue
        }
        if !c.scopes.contains(need.scope) { out.append("\(name): 연결의 범위에 '\(need.scope)' 가 없어 올리지 않습니다") }
        if !secrets.contains(c.id) { out.append("\(name): 키체인에 토큰이 없어 올리지 않습니다") }
        if apServiceBase(c, dev: dev) == nil { out.append("\(name): 사이트 주소가 규칙에 맞지 않아 올리지 않습니다") }
        switch apExpiry(c.expires, now: now, tz: tz) {
        case .expired: out.append("\(name): 토큰 만료일이 지났습니다. 글은 쌓이고 일은 계속합니다")
        case .soon(let d): out.append("\(name): 토큰 만료가 \(d)일 남았습니다")
        default: break
        }
    }
    return out
}

// MARK: - 글

public enum APOutboxState: String, Equatable, Sendable { case waiting, posting, posted, skipped, failed, uncertain }

public struct APAttempt: Equatable, Sendable {
    public var at = 0.0
    public var what = ""      // held(요청 0)·failed·posted·skipped·later·uncertain
    public var code = 0       // HTTP 상태(요청이 없었으면 0)
    public var note = ""
    public init(at: Double = 0, what: String = "", code: Int = 0, note: String = "") { self.at = at; self.what = what; self.code = code; self.note = note }
}

public struct APRemote: Equatable, Sendable {
    public var id = ""
    public var url = ""
    public init(id: String = "", url: String = "") { self.id = id; self.url = url }
}

public struct APOutboxItem: Equatable, Sendable {
    // 정의 — 엔진이 만들 때 정하고 바꾸지 않는다
    public var id = ""
    public var run = ""
    public var unit: Int?
    public var kind = "report"                 // report · comment
    public var target = APOutboxTarget()
    public var connection = ""
    public var title = ""
    public var doc = APDoc()
    public var attachment: String?             // 실행 폴더 기준 파일 이름(report.html), Confluence 보고서만
    public var mode = APPublishMode.review     // 만들 때의 게시 방식
    public var created = 0.0
    // 상태
    public var state = APOutboxState.waiting
    public var later: Double?                  // "나중에"를 누른 시각(목록에 남김, 자동으로 올리지 않음)
    public var site = ""                       // 올리기 시작한 사이트(받은 곳 id 가 이 사이트의 것)
    public var remote: APRemote?
    public var steps: [String] = []            // 끝난 단계: comment · page · attachment · blocks:<보낸 블록 수>
    public var last: APAttempt?
    public var retryAfter: Double?
    public var tries = 0
    public var updated = 0.0
    public init() {}
}

public let apOutboxFormat = "claude-board-outbox"
public let apOutboxMaxBytes = 2 << 20

// 글 id — 파일 이름이 되므로 좁게(엔진이 만든 id 만 이 꼴)
public func apOutboxIDOK(_ id: String) -> Bool { apMatch(id, "^(run|u[0-9]{1,4})-[a-z]{1,12}(-[A-Z][A-Z0-9_]*-[0-9]{1,9})?$") }

public func apOutboxJSON(_ it: APOutboxItem) -> [String: Any] {
    var t: [String: Any] = ["service": it.target.service.rawValue]
    if !it.target.issue.isEmpty { t["issue"] = it.target.issue }
    if !it.target.space.isEmpty { t["space"] = it.target.space }
    if !it.target.parent.isEmpty { t["parent"] = it.target.parent }
    var d: [String: Any] = ["format": apOutboxFormat, "version": 1, "id": it.id, "run": it.run, "kind": it.kind, "target": t,
                            "connection": it.connection, "title": it.title, "doc": apDocJSON(it.doc), "mode": it.mode.rawValue,
                            "created": it.created, "state": it.state.rawValue, "steps": it.steps, "tries": it.tries, "updated": it.updated]
    if let u = it.unit { d["unit"] = u }
    if let a = it.attachment { d["attachment"] = a }
    if let l = it.later { d["later"] = l }
    if !it.site.isEmpty { d["site"] = it.site }
    if let r = it.remote { d["remote"] = ["id": r.id, "url": r.url] }
    if let a = it.last { d["last"] = ["at": a.at, "what": a.what, "code": a.code, "note": a.note] }
    if let r = it.retryAfter { d["retry_after"] = r }
    return d
}

public func apOutboxData(_ it: APOutboxItem) -> Data {
    (try? JSONSerialization.data(withJSONObject: apOutboxJSON(it), options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])) ?? Data()
}

private func dbl(_ v: Any?) -> Double? {
    guard let x = v as? NSNumber, CFGetTypeID(x) != CFBooleanGetTypeID(), x.doubleValue.isFinite else { return nil }
    return x.doubleValue
}

// 파일 → 글. 꼴이 틀리면 nil(보드 검수 화면은 그 파일을 넘긴다)
public func apOutboxParse(_ data: Data) -> APOutboxItem? {
    guard data.count <= apOutboxMaxBytes, let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          o["format"] as? String == apOutboxFormat, let id = o["id"] as? String, apOutboxIDOK(id),
          let tg = o["target"] as? [String: Any], let svc = APTargetService(rawValue: tg["service"] as? String ?? ""),
          let st = APOutboxState(rawValue: o["state"] as? String ?? "") else { return nil }
    var it = APOutboxItem()
    it.id = id
    it.run = o["run"] as? String ?? ""
    it.unit = (o["unit"] as? NSNumber)?.intValue
    it.kind = o["kind"] as? String == "comment" ? "comment" : "report"
    it.target = APOutboxTarget(service: svc, issue: tg["issue"] as? String ?? "", space: tg["space"] as? String ?? "", parent: tg["parent"] as? String ?? "")
    it.connection = o["connection"] as? String ?? ""
    it.title = apDocText(o["title"] as? String ?? "", 300)
    it.doc = apDocParse(o["doc"])
    it.attachment = (o["attachment"] as? String).flatMap { $0 == "report.html" ? $0 : nil }
    it.mode = APPublishMode(rawValue: o["mode"] as? String ?? "") ?? .review
    it.created = dbl(o["created"]) ?? 0
    it.state = st
    it.later = dbl(o["later"])
    it.site = o["site"] as? String ?? ""
    if let r = o["remote"] as? [String: Any] { it.remote = APRemote(id: r["id"] as? String ?? "", url: r["url"] as? String ?? "") }
    it.steps = (o["steps"] as? [Any] ?? []).compactMap { $0 as? String }
    if let a = o["last"] as? [String: Any] {
        it.last = APAttempt(at: dbl(a["at"]) ?? 0, what: a["what"] as? String ?? "", code: (a["code"] as? NSNumber)?.intValue ?? 0,
                            note: apDocText(a["note"] as? String ?? "", 500))
    }
    it.retryAfter = dbl(o["retry_after"])
    it.tries = (o["tries"] as? NSNumber)?.intValue ?? 0
    it.updated = dbl(o["updated"]) ?? 0
    return it
}

// MARK: - 어떤 글을 만들지와 그 본문

private let statusLabel = ["done": "통과", "failed": "미통과", "capped": "상한", "skipped": "건너뜀", "stopped": "중단", "pending": "대기", "running": "진행 중"]
public func apUnitStatusLabel(_ s: String) -> String { statusLabel[s] ?? s }

private func usdText(_ x: Double) -> String { String(format: x < 1 ? "%.3f" : "%.2f", x) }
private func day(_ ts: Double?, _ f: String, _ tz: TimeZone) -> String {
    guard let ts, ts.isFinite, ts > 0 else { return "" }
    let d = DateFormatter()
    d.locale = Locale(identifier: "ko_KR"); d.timeZone = tz; d.calendar = Calendar(identifier: .gregorian); d.dateFormat = f
    return d.string(from: Date(timeIntervalSince1970: ts))
}

// 보고서 글 본문(Confluence·Notion) — 첫 줄 표지, Notion 이면 요청자, 요약·단위 목록·넘김, 첨부 안내, 글 번호
public func apReportDoc(_ r: APReportRun, notion: Bool, attachment: Bool, requester: String, itemID: String, title: String) -> APDoc {
    var b: [APBlock] = [.paragraph([.text(apDocMarker, bold: true)])]
    if notion {
        let who = apDocText(requester, 80)
        b.append(.paragraph([.text(who.isEmpty ? "요청자: 연결에 이름이 없습니다(Notion 작성자 칸은 통합 이름입니다)" : "요청자: \(who)", bold: false)]))
    }
    // 제목은 받는 쪽 페이지 제목 칸으로 간다(본문에 다시 쓰지 않음 — 첫 줄은 늘 표지)
    let n = r.units.count
    let k = { (s: String) in r.units.filter { $0.status == s }.count }
    var line = "대기열 \(n)개 가운데 \(k("done"))개 통과"
    if k("failed") > 0 { line += ", \(k("failed"))개 미통과" }
    if k("capped") > 0 { line += ", 상한 \(k("capped"))" }
    if k("skipped") > 0 { line += ", 건너뜀 \(k("skipped"))" }
    if k("stopped") > 0 { line += ", 중단 \(k("stopped"))" }
    let sec = r.units.reduce(0) { $0 + $1.activeSec }, cost = r.units.reduce(0) { $0 + $1.cost }
    line += ". 일한 시간 \(apReportDur(sec)), \(usdText(cost)) USD."
    b.append(.paragraph([.text(line, bold: false)]))
    var who = "실행 \(r.id)"
    if !r.project.isEmpty { who += " · 프로젝트 \(r.project)" }
    if !r.model.isEmpty { who += " · 모델 \(r.model)" }
    if !r.epic.isEmpty { who += " · 에픽 \(r.epic)" }
    b.append(.paragraph([.text(who, bold: false)]))
    if !r.units.isEmpty {
        b.append(.bullets(r.units.map { u in
            var s: [APSpan] = [.text((u.title.isEmpty ? "단위 \(u.n)" : u.title) + ": ", bold: false), .text(apUnitStatusLabel(u.status), bold: true)]
            if !u.task.isEmpty { s.append(.text(" · \(u.task)", bold: false)) }
            return s
        }))
    }
    let parked = r.units.reduce(0) { $0 + ($1.result?.nextStart.count ?? 0) }, found = r.units.reduce(0) { $0 + ($1.result?.newWork.count ?? 0) }
    let harness = r.harnessSame.map { $0 ? "하네스 출발 때와 같음" : "하네스 출발 때와 다름" } ?? "하네스 확인 기록 없음"
    b.append(.paragraph([.text("미룬 사람 몫 \(parked) · 새로 찾은 일 \(found) · \(harness)", bold: false)]))
    b.append(.paragraph([.text(attachment ? "흐름 그림과 단위별 기록은 첨부한 보고서(report.html)에 있습니다."
                                : "흐름 그림과 단위별 기록은 보드의 실행 보고서에 있습니다(이 글에는 첨부하지 않았습니다).", bold: false)]))
    b.append(.paragraph([.text("보드 글 번호 \(r.id)/\(itemID)", bold: false)]))
    return apDocClamp(APDoc(b))
}

// 단위 티켓 댓글 본문(Jira) — 첫 줄 표지, 결과 한 줄, 모델이 쓴 요약 몇 줄, 글 번호
public func apCommentDoc(_ r: APReportRun, unit u: APReportUnit, itemID: String) -> APDoc {
    var b: [APBlock] = [.paragraph([.text(apDocMarker, bold: true)])]
    let title = u.title.isEmpty ? "단위 \(u.n)" : u.title
    b.append(.paragraph([.text(title + ": ", bold: false), .text(apUnitStatusLabel(u.status), bold: true),
                         .text(" (단위 \(u.n)/\(r.units.count), 일한 시간 \(apReportDur(u.activeSec)), \(usdText(u.cost)) USD)", bold: false)]))
    if let res = u.result {
        if !res.headline.isEmpty { b.append(.paragraph([.text(res.headline, bold: false)])) }
        let rows = res.summary.prefix(10).map { [APSpan.text($0, bold: false)] }
        if !rows.isEmpty { b.append(.bullets(Array(rows))) }
    } else if !u.reason.isEmpty {
        b.append(.paragraph([.text(u.reason, bold: false)]))
    }
    b.append(.paragraph([.text("자율주행 실행 \(r.id) · 보드 글 번호 \(r.id)/\(itemID)", bold: false)]))
    return apDocClamp(APDoc(b))
}

private let commentStates: Set<String> = ["done", "failed", "capped", "skipped"]

// 단위 판정 뒤 만들 글 — 단위 티켓 댓글(게시 설정이 켰고 단위에 티켓 키가 있고 끝난 단위일 때)
public func apUnitOutbox(_ r: APReportRun, unit n: Int, ticket: String?, publish: APPublish, now: Double) -> [APOutboxItem] {
    guard publish.jiraComments, let key = ticket, apMatch(key, apIssueKeyPattern),
          let u = r.units.first(where: { $0.n == n }), commentStates.contains(u.status) else { return [] }
    var it = APOutboxItem()
    it.id = "u\(n)-jira-\(key)"
    it.run = r.id; it.unit = n; it.kind = "comment"
    it.target = APOutboxTarget(service: .jira, issue: key)
    it.connection = "atlassian"
    it.title = apDocText("\(key) \(u.title.isEmpty ? "단위 \(n)" : u.title): \(apUnitStatusLabel(u.status))", 300)
    it.doc = apCommentDoc(r, unit: u, itemID: it.id)
    it.mode = publish.mode
    it.created = now; it.updated = now
    return [it]
}

// 끝 정리 때 만들 글 — 보고서(받을 곳이 있을 때). Confluence 는 report.html 을 첨부하고 Notion 은 요약만(설계 판단: Notion 파일 올리기는 요청 세 단계라 다음 조각)
public func apRunOutbox(_ r: APReportRun, publish: APPublish, requester: String, hasReport: Bool, now: Double, tz: TimeZone) -> [APOutboxItem] {
    guard let t = publish.report else { return [] }
    var it = APOutboxItem()
    it.id = "run-" + t.service.rawValue
    it.run = r.id; it.kind = "report"; it.target = t
    it.connection = apTargetConnection(t.service).id
    let name = r.project.isEmpty ? (r.title.isEmpty ? r.id : r.title) : r.project
    let when = day(r.started ?? r.ended ?? now, "M월 d일 HH:mm", tz)
    it.title = apDocText("자율주행 보고: \(name) \(when)", 300)
    let attach = t.service == .confluence && hasReport
    it.attachment = attach ? "report.html" : nil
    it.doc = apReportDoc(r, notion: t.service == .notion, attachment: attach, requester: requester, itemID: it.id, title: it.title)
    it.mode = publish.mode
    it.created = now; it.updated = now
    return [it]
}

// MARK: - 올릴지 판정

public enum APPostTrigger: Equatable, Sendable {
    case auto     // 엔진이 스스로(자동으로 올림)
    case press    // 사람이 검수 화면에서 누름
}

public enum APPostDecision: Equatable, Sendable {
    case go(base: String)    // 보낸다(그 바탕 주소로만)
    case nothing(String)     // 요청 없음, 상태 그대로
    case hold(String)        // 요청 없음, 마지막 시도에 "held" 와 까닭을 남김
}

public func apPostDecide(_ it: APOutboxItem, trigger: APPostTrigger, connection c: APConnection?, hasSecret: Bool, dev: Bool, now: Date, tz: TimeZone) -> APPostDecision {
    switch it.state {
    case .posted: return .nothing("이미 올렸습니다")
    case .posting: return .nothing("올리는 중입니다")
    case .skipped where trigger == .auto: return .nothing("올리지 않기로 했습니다(로컬에만)")
    case .uncertain where trigger == .auto: return .nothing("앞 올리기가 끝났는지 몰라 사람이 확인한 뒤에만 올립니다")
    default: break
    }
    if trigger == .auto {
        if it.mode == .review { return .nothing("검수 뒤 올림: 사람이 올리기를 눌러야 올라갑니다") }
        if it.state == .failed, it.retryAfter == nil { return .nothing("올리지 못한 까닭을 사람이 본 뒤에 다시 올립니다") }
        if let r = it.retryAfter, r > now.timeIntervalSince1970 { return .nothing("받는 쪽이 잠시 뒤에 보내라고 해 기다립니다") }
    }
    let need = apTargetConnection(it.target.service)
    guard let c, c.id == it.connection, c.id == need.id else { return .hold("연결이 없어 요청하지 않았습니다. 글은 로컬에 남습니다") }
    guard c.scopes.contains(need.scope) else { return .hold("연결의 범위에 '\(need.scope)' 가 없어 요청하지 않았습니다") }
    guard let base = apServiceBase(c, dev: dev) else { return .hold("사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다") }
    if !it.site.isEmpty, it.site != base, it.remote != nil || !it.steps.isEmpty {
        return .hold("이 글을 올리기 시작한 사이트(\(it.site))와 지금 연결의 사이트(\(base))가 달라 요청하지 않았습니다")
    }
    if c.service == .atlassian, c.account.isEmpty { return .hold("Atlassian 계정 메일이 없어 요청하지 않았습니다") }
    guard hasSecret else { return .hold("토큰이 키체인에 없어 요청하지 않았습니다") }
    if case .expired = apExpiry(c.expires, now: now, tz: tz) { return .hold("토큰 만료일(\(c.expires))이 지나 요청하지 않았습니다. 새 토큰을 넣고 올립니다") }
    return .go(base: base)
}

// MARK: - 다시 보내기

public let apRetryMax = 3          // 한 번 올리기에서 429·503 뒤 다시 보내는 횟수
public let apRetryWaitMax = 20.0   // 한 번 기다리는 초 상한

// 다시 보낼지와 기다릴 초 — 429·503 만, Retry-After(초 또는 HTTP 날짜)를 따르되 1~20초, 없으면 1·2·4초. 횟수를 넘으면 nil
public func apRetryWait(status: Int, retryAfter: String?, attempt: Int, now: Date) -> Double? {
    guard status == 429 || status == 503, attempt < apRetryMax else { return nil }
    var w = pow(2.0, Double(attempt))
    if let h = retryAfter?.trimmingCharacters(in: .whitespaces), !h.isEmpty {
        if let s = Double(h), s.isFinite { w = s }
        else {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX"); f.timeZone = TimeZone(identifier: "GMT"); f.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
            if let d = f.date(from: h) { w = d.timeIntervalSince(now) }
        }
    }
    return min(max(w, 1), apRetryWaitMax)
}

// 잠깐의 실패인가 — 자동으로 올림에서 다음 단위 끝·끝 정리 때 다시 보낸다
public func apTransient(_ r: APHTTPResponse) -> Bool {
    if r.status == 0 { return !(r.error ?? "").contains("사이트 밖") && !(r.error ?? "").contains("실행 폴더에 없습니다") }
    return r.status == 429 || (500...599).contains(r.status)
}

// 실패 까닭(사람 말) — 받는 쪽 본문은 믿지 않아 상태 번호로만
public func apFailNote(_ r: APHTTPResponse) -> String {
    if let e = r.error { return e }
    switch r.status {
    case 401, 403: return "거부됨(\(r.status)): 토큰이 만료·취소됐거나 그 자리에 쓸 권한이 없습니다"
    case 404: return "받을 곳을 찾지 못했습니다(404): 이슈·스페이스·페이지 id 를 확인합니다"
    case 400: return "받는 쪽이 글 꼴을 받지 않았습니다(400): 같은 제목의 페이지가 있거나 받을 곳 값이 틀렸을 수 있습니다"
    case 409: return "받는 쪽에서 부딪혔습니다(409)"
    case 413: return "글이 너무 큽니다(413)"
    case 429: return "요청이 많다며 거절했습니다(429). 잠시 뒤 다시 올립니다"
    case 300...399: return "다른 주소로 보내려 해 따라가지 않았습니다(\(r.status))"
    case 500...599: return "받는 쪽 오류(\(r.status))"
    default: return "받는 쪽이 \(r.status) 로 답했습니다"
    }
}

// MARK: - 요청 꼴

private func body(_ o: Any) -> Data { (try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys, .withoutEscapingSlashes])) ?? Data() }
private let jsonHeaders = ["Content-Type": "application/json", "Accept": "application/json"]

public func apJiraCommentRequest(_ it: APOutboxItem, base: String, auth: APAuth) -> APHTTPRequest {
    APHTTPRequest(method: "POST", url: base + "/rest/api/3/issue/\(it.target.issue)/comment", headers: jsonHeaders, body: body(["body": apADF(it.doc)]), auth: auth)
}

public func apConfluencePageRequest(_ it: APOutboxItem, base: String, auth: APAuth) -> APHTTPRequest {
    var o: [String: Any] = ["type": "page", "title": it.title, "space": ["key": it.target.space],
                            "body": ["storage": ["value": apStorage(it.doc), "representation": "storage"]]]
    if !it.target.parent.isEmpty { o["ancestors"] = [["id": it.target.parent]] }
    return APHTTPRequest(method: "POST", url: base + "/wiki/rest/api/content", headers: jsonHeaders, body: body(o), auth: auth)
}

// 첨부 — v1 API 의 "만들거나 바꾸기"(PUT): 같은 이름이 이미 있으면 새 판으로 올라가 다시 눌러도 첨부가 둘이 되지 않는다
public func apConfluenceAttachmentRequest(_ it: APOutboxItem, base: String, page: String, file: Data, auth: APAuth) -> APHTTPRequest {
    let boundary = "claude-board-" + String(it.id.unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) }.map(Character.init))
    var d = Data()
    d.append(Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"file\"; filename=\"\(it.attachment ?? "report.html")\"\r\nContent-Type: text/html; charset=utf-8\r\n\r\n".utf8))
    d.append(file)
    d.append(Data("\r\n--\(boundary)\r\nContent-Disposition: form-data; name=\"minorEdit\"\r\n\r\ntrue\r\n--\(boundary)--\r\n".utf8))
    return APHTTPRequest(method: "PUT", url: base + "/wiki/rest/api/content/\(page)/child/attachment",
                         headers: ["Content-Type": "multipart/form-data; boundary=\(boundary)", "X-Atlassian-Token": "no-check", "Accept": "application/json"],
                         body: d, auth: auth)
}

public func apNotionPageRequest(_ it: APOutboxItem, base: String, children: [[String: Any]]) -> APHTTPRequest {
    let o: [String: Any] = ["parent": ["page_id": it.target.parent],
                            "properties": ["title": ["title": [["type": "text", "text": ["content": it.title]]]]],
                            "children": children]
    return APHTTPRequest(method: "POST", url: base + "/v1/pages", headers: jsonHeaders.merging(["Notion-Version": apNotionVersion]) { $1 }, body: body(o), auth: .bearer)
}

public func apNotionAppendRequest(base: String, page: String, children: [[String: Any]]) -> APHTTPRequest {
    APHTTPRequest(method: "PATCH", url: base + "/v1/blocks/\(page)/children", headers: jsonHeaders.merging(["Notion-Version": apNotionVersion]) { $1 },
                  body: body(["children": children]), auth: .bearer)
}

// 받은 곳 id — 응답 본문(믿지 않는 값)의 id 칸이 정한 꼴일 때만
public func apCreatedID(_ s: APTargetService, _ r: APHTTPResponse) -> String? {
    guard r.error == nil, (200...299).contains(r.status), r.body.count <= 4 << 20,
          let o = try? JSONSerialization.jsonObject(with: r.body) as? [String: Any], let id = o["id"] as? String else { return nil }
    switch s {
    case .jira, .confluence: return apMatch(id, apConfluenceIDPattern) ? id : nil
    case .notion: return apMatch(id, apNotionIDPattern) ? id.lowercased() : nil
    }
}

// 받은 곳 주소 — 응답의 주소 칸은 믿지 않고 사이트와 id 로 만든다. Notion 은 정한 API 주소면 notion.so, 시험 빌드의 가짜 서버면 그 서버 아래
public func apRemoteURL(_ it: APOutboxItem, base: String, id: String) -> String {
    switch it.target.service {
    case .jira: return base + "/browse/\(it.target.issue)?focusedCommentId=\(id)"
    case .confluence: return base + "/wiki/pages/viewpage.action?pageId=\(id)"
    case .notion:
        let hex = id.replacingOccurrences(of: "-", with: "")
        return base == apNotionAPI ? "https://www.notion.so/\(hex)" : base + "/p/\(hex)"
    }
}

// MARK: - 올리기 유스케이스

public struct APPostResult: Equatable, Sendable {
    public var item: APOutboxItem?
    public var requests = 0      // 이번에 보낸 요청 수(다시 보낸 것 포함)
    public var note = ""
    public var busy = false      // 잠금을 잡지 못함(다른 곳에서 올리는 중)
    public init(item: APOutboxItem? = nil, requests: Int = 0, note: String = "", busy: Bool = false) {
        self.item = item; self.requests = requests; self.note = note; self.busy = busy
    }
}

// 글 하나를 올린다(잠금 안에서). budget 은 429 기다림까지 합친 초 상한 — 넘으면 "올리지 못함"과 다시 보낼 때(retry_after)를 남기고 끝낸다
public func apPostItem(_ id: String, store: APOutboxStore, trigger: APPostTrigger, connections: [APConnection], dev: Bool,
                       secrets: APSecrets, http: APHTTP, clock: APClock, waiter: APWaiter, tz: TimeZone, budget: Double = 30) -> APPostResult {
    let r: APPostResult? = store.withLock(id) {
        guard var it = store.read(id) else { return APPostResult(note: "글을 찾지 못했습니다") }
        let start = clock.now.timeIntervalSince1970
        if it.state == .posting {
            it.state = .uncertain
            it.last = APAttempt(at: start, what: "uncertain", note: "앞 올리기가 끝나지 못했습니다. 받을 곳에 이미 생겼는지 확인한 뒤 올립니다")
            it.updated = start
            store.write(it)
        }
        let conn = connections.first { $0.id == it.connection }
        let decision = apPostDecide(it, trigger: trigger, connection: conn, hasSecret: conn.map { secrets.exists($0.id) } ?? false,
                                    dev: dev, now: clock.now, tz: tz)
        let base: String
        switch decision {
        case .nothing(let why): return APPostResult(item: it, note: why)
        case .hold(let why):
            it.last = APAttempt(at: start, what: "held", note: why); it.updated = start
            store.write(it)
            return APPostResult(item: it, note: why)
        case .go(let b): base = b
        }
        let c = conn!
        guard let token = secrets.read(c.id), !token.isEmpty else {
            it.last = APAttempt(at: start, what: "held", note: "토큰이 키체인에 없어 요청하지 않았습니다"); it.updated = start
            store.write(it)
            return APPostResult(item: it, note: it.last!.note)
        }
        let auth = apAuthFor(c), header = apAuthHeader(auth, token: token)
        // 보내기 전에 올리는 중으로(죽으면 다음에 잠금을 잡은 쪽이 "확인 필요"로 본다)
        it.state = .posting; it.site = base; it.tries += 1; it.retryAfter = nil; it.later = nil; it.updated = start
        store.write(it)
        var requests = 0, lastWait: Double?
        func send(_ req: APHTTPRequest) -> APHTTPResponse {
            guard apSameSite(req.url, base: base) else { return APHTTPResponse(error: "연결한 사이트 밖 주소라 보내지 않았습니다") }
            var attempt = 0
            while true {
                requests += 1
                let resp = http.send(req, auth: header)
                guard let w = apRetryWait(status: resp.status, retryAfter: resp.headers["retry-after"], attempt: attempt, now: clock.now) else { return resp }
                if clock.now.timeIntervalSince1970 + w > start + budget { lastWait = w; return resp }
                waiter.wait(w)
                attempt += 1
            }
        }
        func fail(_ resp: APHTTPResponse, _ step: String) -> APPostResult {
            let t = clock.now.timeIntervalSince1970
            it.state = .failed
            // 잠깐의 실패(429·5xx·응답 없음)만 자동으로 다시 보낼 때를 남긴다. 거부(401·403)·꼴(400)·다른 곳으로 보냄(3xx)은 사람이 본 뒤에만
            if apTransient(resp) { it.retryAfter = t + (lastWait ?? 60) }
            it.last = APAttempt(at: t, what: "failed", code: resp.status, note: "\(step): \(apFailNote(resp))")
            it.updated = t
            store.write(it)
            return APPostResult(item: it, requests: requests, note: it.last!.note)
        }
        func done(_ step: String, remote: APRemote? = nil) {
            if let remote { it.remote = remote }
            if step.hasPrefix("blocks:") { it.steps.removeAll { $0.hasPrefix("blocks:") } }
            if !it.steps.contains(step) { it.steps.append(step) }
            it.updated = clock.now.timeIntervalSince1970
            store.write(it)
        }
        switch it.target.service {
        case .jira:
            if it.remote == nil {
                let resp = send(apJiraCommentRequest(it, base: base, auth: auth))
                guard let rid = apCreatedID(.jira, resp) else { return fail(resp, "댓글") }
                done("comment", remote: APRemote(id: rid, url: apRemoteURL(it, base: base, id: rid)))
            }
        case .confluence:
            if it.remote == nil {
                let resp = send(apConfluencePageRequest(it, base: base, auth: auth))
                guard let rid = apCreatedID(.confluence, resp) else { return fail(resp, "페이지") }
                done("page", remote: APRemote(id: rid, url: apRemoteURL(it, base: base, id: rid)))
            }
            if let name = it.attachment, !it.steps.contains("attachment"), let page = it.remote?.id {
                guard let file = store.attachment(name) else { return fail(APHTTPResponse(error: "첨부할 \(name) 이 실행 폴더에 없습니다"), "첨부") }
                let resp = send(apConfluenceAttachmentRequest(it, base: base, page: page, file: file, auth: auth))
                guard (200...299).contains(resp.status), resp.error == nil else { return fail(resp, "첨부") }
                done("attachment")
            }
        case .notion:
            let batches = apBatches(apNotionBlocks(it.doc))
            var sent = it.steps.first { $0.hasPrefix("blocks:") }.flatMap { Int($0.dropFirst(7)) } ?? 0
            if it.remote == nil {
                let first = batches.first ?? []
                let resp = send(apNotionPageRequest(it, base: base, children: first))
                guard let rid = apCreatedID(.notion, resp) else { return fail(resp, "페이지") }
                sent = first.count
                done("page", remote: APRemote(id: rid, url: apRemoteURL(it, base: base, id: rid)))
                done("blocks:\(sent)")
            }
            let all = batches.flatMap { $0 }
            for batch in apBatches(Array(all.dropFirst(sent))) {
                let resp = send(apNotionAppendRequest(base: base, page: it.remote!.id, children: batch))
                guard (200...299).contains(resp.status), resp.error == nil else { return fail(resp, "블록 덧붙이기") }
                sent += batch.count
                done("blocks:\(sent)")
            }
        }
        let t = clock.now.timeIntervalSince1970
        it.state = .posted
        it.last = APAttempt(at: t, what: "posted", code: 0, note: "올렸습니다")
        it.updated = t
        store.write(it)
        return APPostResult(item: it, requests: requests, note: "올렸습니다")
    }
    return r ?? APPostResult(item: store.read(id), note: "다른 곳에서 이 글을 올리는 중입니다", busy: true)
}

// 사람이 고른 상태 바꾸기(올리지 않기·나중에) — 잠금 안에서, 올린 글·올리는 중인 글은 바꾸지 않는다
public func apMarkItem(_ id: String, _ what: String, store: APOutboxStore, clock: APClock) -> APPostResult {
    let r: APPostResult? = store.withLock(id) {
        guard var it = store.read(id) else { return APPostResult(note: "글을 찾지 못했습니다") }
        guard it.state != .posted, it.state != .posting else { return APPostResult(item: it, note: it.state == .posted ? "이미 올렸습니다" : "올리는 중입니다") }
        let t = clock.now.timeIntervalSince1970
        switch what {
        case "skip": it.state = .skipped; it.later = nil; it.last = APAttempt(at: t, what: "skipped", note: "올리지 않기(로컬에만)")
        case "later": it.later = t; it.last = APAttempt(at: t, what: "later", note: "나중에(목록에 남김, 자동으로 올리지 않음)")
        case "unskip": if it.state == .skipped { it.state = .waiting }; it.last = APAttempt(at: t, what: "waiting", note: "다시 기다림")
        default: return APPostResult(item: it, note: "모르는 바꾸기입니다")
        }
        it.updated = t
        store.write(it)
        return APPostResult(item: it, note: it.last!.note)
    }
    return r ?? APPostResult(item: store.read(id), note: "다른 곳에서 이 글을 올리는 중입니다", busy: true)
}

// 글 만들기 — 같은 id 가 있으면 두지 않는다(다시 떠서 끝 정리를 다시 해도 하나)
public func apCreateItems(_ items: [APOutboxItem], store: APOutboxStore) -> [String] {
    items.filter { store.create($0) }.map(\.id)
}

// 검수 화면 한 줄 상태
public func apOutboxStateLabel(_ it: APOutboxItem) -> String {
    switch it.state {
    case .waiting: return it.later != nil ? "나중에" : (it.mode == .review ? "검수 기다림" : (it.last?.what == "held" ? "올리지 못함(요청 0)" : "올리기 기다림"))
    case .posting: return "올리는 중"
    case .posted: return "올림"
    case .skipped: return "올리지 않음(로컬에만)"
    case .failed: return "올리지 못함"
    case .uncertain: return "확인 필요"
    }
}

// "모두 올리기" 확인 글의 수 — 받을 곳 종류마다
public func apOutboxCounts(_ items: [APOutboxItem]) -> String {
    let order: [(APTargetService, String)] = [(.jira, "Jira 댓글"), (.confluence, "Confluence"), (.notion, "Notion")]
    return order.compactMap { s, name in
        let n = items.filter { $0.target.service == s }.count
        return n > 0 ? "\(name) \(n)" : nil
    }.joined(separator: " · ")
}
