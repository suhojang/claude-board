// 자율주행 규칙(AutopilotRules) — 출발 때 한꺼번에 만드는 Jira 티켓 나무와, 실행 중에 새로 필요해진 티켓(4단계 ⑩-3, 2026-10-30, 설계 2장 끝·5장 6절).
// 출발 확인의 "만들 티켓"(에픽 아래 새 스토리·새 작업, 다른 스토리로 옮긴 작업의 부모 바꾸기)은 보드가 계획 파일(tickets-plan.json)로 쓰고,
// 출발을 누르면 보드가 앱 안 엔진의 tickets 명령을 불러 만든다(보드는 HTTP 를 하지 않는다, ⑩-1). 만든 키는 엔진이 상태 파일(tickets.json)에 남기고
// 보드가 그 키를 대기열(queue.json 의 story·tickets)에 넣은 뒤 출발한다. 다시 누르면 남은 것만 만든다(만든 항목은 요청 0).
// 같은 것을 두 번 만들지 않게: 요청을 보내기 전에 항목을 "만드는 중"(creating)으로 쓰고, 받은 키를 바로 적고, 잠금 안에서 creating 을 보면
// 앞 만들기가 끝나지 못하고 죽은 것이라 "확인 필요"(uncertain)로 바꾸고 사람이 확인하기 전에는 다시 만들지 않는다(⑩-1 올릴 글과 같은 꼴).
// 실행 중에 새로 필요해진 티켓은 출발 카드 답대로 임시 번호(TMP-<단위>-<차례>)로 진행하고, 단위 판정 뒤 엔진이 올릴 글(kind ticket)로 남겨
// 검수 뒤에 사람이 만든다(게시 방식이 자동으로 올림이어도 늘 검수 뒤 — 실행 중 티켓 만들기 요청 0).
// 제목·본문은 ⑩-1 의 형식 변환(apDocText·APDoc → apADF)을 다시 쓴다. 요청은 포트(APHTTP·APSecrets·APTicketStore·APClock·APWaiter)로만
import Foundation

// MARK: - 계획(보드가 씀)

public enum APTicketKind: String, Equatable, Sendable { case story, task, move }

// 만들 티켓 하나 — id 는 출발 준비 안에서 바뀌지 않는 이름(단위 위치가 아니라): story:<보드 스토리 id>·task:<단위 id>·move:<작업 키>
public struct APTicketEntry: Equatable, Sendable {
    public var id = ""
    public var kind = APTicketKind.task
    public var title = ""
    public var parent = ""          // 에픽 키·이미 있는 스토리 키, 또는 같은 계획의 story:… 항목
    public var key = ""             // move: 부모를 바꿀 이미 있는 작업 키
    public var units: [String] = [] // 본문에 넣을 단위 제목(스토리는 그 스토리의 단위들, 작업은 그 단위 하나)
    public init(id: String = "", kind: APTicketKind = .task, title: String = "", parent: String = "", key: String = "", units: [String] = []) {
        self.id = id; self.kind = kind; self.title = title; self.parent = parent; self.key = key; self.units = units
    }
}

public struct APTicketPlan: Equatable, Sendable {
    public var run = ""             // 출발 준비 id(출발하면 실행 id)
    public var epic = ""
    public var storyType = ""       // 프로젝트 설정 tickets.issue_types.story(비우면 응답의 이슈 종류에서 고름)
    public var taskType = ""        // 프로젝트 설정 tickets.issue_types.task(비우면 하위 작업 종류에서 고름)
    public var entries: [APTicketEntry] = []
    public init(run: String = "", epic: String = "", storyType: String = "", taskType: String = "", entries: [APTicketEntry] = []) {
        self.run = run; self.epic = epic; self.storyType = storyType; self.taskType = taskType; self.entries = entries
    }
    // 새 티켓의 프로젝트 — 에픽 키 앞(SHOP-10 → SHOP)
    public var project: String { epic.split(separator: "-").first.map(String.init) ?? "" }
}

public let apTicketPlanFormat = "claude-board-tickets-plan"
public let apTicketBookFormat = "claude-board-tickets"
public let apTicketTitleMax = 240          // Jira 요약 칸 255자 안
public let apTicketEntryMax = 200          // 한 번에 만들 티켓 수 상한

public func apTicketEntryIDOK(_ id: String) -> Bool {
    apMatch(id, "^(story:[A-Za-z0-9._-]{1,64}|task:[A-Za-z0-9._-]{1,64}|move:[A-Z][A-Z0-9_]*-[0-9]{1,9})$")
}

public func apTicketPlanJSON(_ p: APTicketPlan) -> [String: Any] {
    var d: [String: Any] = ["format": apTicketPlanFormat, "version": 1, "run": p.run, "epic": p.epic,
                            "entries": p.entries.map { e -> [String: Any] in
                                var o: [String: Any] = ["id": e.id, "kind": e.kind.rawValue, "title": e.title, "parent": e.parent, "units": e.units]
                                if !e.key.isEmpty { o["key"] = e.key }
                                return o
                            }]
    if !p.storyType.isEmpty || !p.taskType.isEmpty { d["issue_types"] = ["story": p.storyType, "task": p.taskType] }
    return d
}

// 계획 파일 → 계획과 고칠 것. 고칠 것이 하나라도 있으면 아무것도 만들지 않는다(요청 0)
public func apTicketPlanParse(_ data: Data?) -> (plan: APTicketPlan, problems: [String]) {
    var p = APTicketPlan(), probs: [String] = []
    guard let data, data.count <= 1 << 20, let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          o["format"] as? String == apTicketPlanFormat else { return (p, ["만들 티켓 계획 파일을 읽지 못했습니다"]) }
    p.run = o["run"] as? String ?? ""
    p.epic = (o["epic"] as? String ?? "").trimmingCharacters(in: .whitespaces)
    if let t = o["issue_types"] as? [String: Any] {
        p.storyType = apDocText(t["story"] as? String ?? "", 80); p.taskType = apDocText(t["task"] as? String ?? "", 80)
    }
    if !apMatch(p.epic, apIssueKeyPattern) { probs.append("에픽 번호 꼴이 아닙니다: \(p.epic.prefix(40))") }
    let list = o["entries"] as? [Any] ?? []
    if list.count > apTicketEntryMax { probs.append("한 번에 만들 티켓은 \(apTicketEntryMax)개까지입니다(\(list.count)개)") }
    var seen = Set<String>()
    for x in list.prefix(apTicketEntryMax) {
        guard let e = x as? [String: Any], let id = e["id"] as? String, apTicketEntryIDOK(id),
              let k = APTicketKind(rawValue: e["kind"] as? String ?? "") else { probs.append("꼴이 틀린 항목이 있습니다"); continue }
        guard seen.insert(id).inserted else { probs.append("같은 항목이 둘입니다: \(id)"); continue }
        var t = APTicketEntry(id: id, kind: k)
        // 제목·단위 제목은 모델·바깥이 쓴 글일 수 있다 — 알림과 같은 원칙(비밀 가리기·다듬기·멘션 무력화·상한, Notify.swift apNotifyClean)
        t.title = apNotifyClean(e["title"] as? String ?? "", apTicketTitleMax)
        t.parent = (e["parent"] as? String ?? "").trimmingCharacters(in: .whitespaces)
        t.key = (e["key"] as? String ?? "").trimmingCharacters(in: .whitespaces)
        t.units = (e["units"] as? [Any] ?? []).prefix(50).compactMap { $0 as? String }.map { apNotifyClean($0, 200) }.filter { !$0.isEmpty }
        let prefix = id.split(separator: ":").first.map(String.init) ?? ""
        if prefix != k.rawValue { probs.append("항목 \(id) 의 종류가 이름과 다릅니다") }
        if k != .move, t.title.isEmpty { probs.append("항목 \(id) 의 제목이 비었습니다") }
        if k == .move, !apMatch(t.key, apIssueKeyPattern) || id != "move:" + t.key { probs.append("옮길 작업 키 꼴이 아닙니다: \(t.key.prefix(40))") }
        p.entries.append(t)
    }
    // 부모: 스토리는 에픽 아래, 작업·옮기기는 이미 있는 스토리 키 또는 같은 계획의 새 스토리
    let stories = Set(p.entries.filter { $0.kind == .story }.map(\.id))
    for e in p.entries {
        switch e.kind {
        case .story: if e.parent != p.epic { probs.append("스토리 \(e.id) 의 부모가 에픽(\(p.epic))이 아닙니다") }
        case .task, .move:
            if e.parent.hasPrefix("story:") { if !stories.contains(e.parent) { probs.append("항목 \(e.id) 의 부모 \(e.parent) 가 계획에 없습니다") } }
            else if !apMatch(e.parent, apIssueKeyPattern) { probs.append("항목 \(e.id) 의 부모 꼴이 아닙니다: \(e.parent.prefix(40))") }
        }
    }
    return (p, probs)
}

// MARK: - 상태(엔진이 씀, 보드는 읽기만)

public enum APTicketState: String, Equatable, Sendable { case waiting, creating, made, failed, uncertain }

public struct APTicketRecord: Equatable, Sendable {
    public var state = APTicketState.waiting
    public var key = ""             // 만든 키(move 는 옮긴 작업 키)
    public var url = ""
    public var last: APAttempt?
    public var tries = 0
    public var updated = 0.0
    public init() {}
}

public struct APTicketBook: Equatable, Sendable {
    public var site = ""            // 만들기 시작한 사이트(만든 키는 이 사이트의 것)
    public var entries: [String: APTicketRecord] = [:]
    public init() {}
}

public func apTicketBookJSON(_ b: APTicketBook) -> [String: Any] {
    var es: [String: Any] = [:]
    for (k, r) in b.entries {
        var d: [String: Any] = ["state": r.state.rawValue, "tries": r.tries, "updated": r.updated]
        if !r.key.isEmpty { d["key"] = r.key }
        if !r.url.isEmpty { d["url"] = r.url }
        if let a = r.last { d["last"] = ["at": a.at, "what": a.what, "code": a.code, "note": a.note] }
        es[k] = d
    }
    var o: [String: Any] = ["format": apTicketBookFormat, "version": 1, "entries": es]
    if !b.site.isEmpty { o["site"] = b.site }
    return o
}

public func apTicketBookParse(_ data: Data?) -> APTicketBook {
    var b = APTicketBook()
    guard let data, data.count <= 1 << 20, let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          o["format"] as? String == apTicketBookFormat else { return b }
    b.site = o["site"] as? String ?? ""
    for (k, v) in o["entries"] as? [String: Any] ?? [:] {
        guard apTicketEntryIDOK(k), let d = v as? [String: Any], let st = APTicketState(rawValue: d["state"] as? String ?? "") else { continue }
        var r = APTicketRecord()
        r.state = st
        let key = d["key"] as? String ?? ""
        r.key = apMatch(key, apIssueKeyPattern) ? key : ""
        r.url = apDocText(d["url"] as? String ?? "", 500)
        if let a = d["last"] as? [String: Any] {
            r.last = APAttempt(at: (a["at"] as? NSNumber)?.doubleValue ?? 0, what: a["what"] as? String ?? "", code: (a["code"] as? NSNumber)?.intValue ?? 0,
                               note: apDocText(a["note"] as? String ?? "", 500))
        }
        r.tries = (d["tries"] as? NSNumber)?.intValue ?? 0
        r.updated = (d["updated"] as? NSNumber)?.doubleValue ?? 0
        if r.state == .made, r.key.isEmpty { r.state = .uncertain }   // 만들었다는데 키가 없으면 믿지 않는다
        b.entries[k] = r
    }
    return b
}

// 만든 키 — 보드가 대기열에 넣는다: 보드 스토리 id → 스토리 키, 단위 id → 작업 키
public func apTicketKeys(_ p: APTicketPlan, _ b: APTicketBook) -> (stories: [String: String], tasks: [String: String]) {
    var s: [String: String] = [:], t: [String: String] = [:]
    for e in p.entries {
        guard let r = b.entries[e.id], r.state == .made, !r.key.isEmpty else { continue }
        let tail = String(e.id.drop { $0 != ":" }.dropFirst())
        switch e.kind {
        case .story: s[tail] = r.key
        case .task: t[tail] = r.key
        case .move: break
        }
    }
    return (s, t)
}

// 출발 준비 동안 대기열에 넣는 자리표 — 아직 만들지 않은 스토리·작업의 키 자리(엔진 status 가 가지 이름 꼴을 미리 보게).
// 이슈 키 꼴(숫자)이 아니라 단위 시작 전 티켓 읽기·Jira 댓글이 요청하지 않고, 엔진 start 는 자리표가 남은 대기열로 출발하지 않는다.
// story:new-3 → SHOP-NEW3, 그 밖의 항목 → SHOP-NEW-<항목 꼬리>
public let apTicketPlaceholderPattern = "^[A-Z][A-Z0-9_]*-NEW([0-9]{1,4}|-[A-Za-z0-9._]{1,64})$"
public func apTicketPlaceholder(project: String, entry: String) -> String {
    let tail = String(entry.drop { $0 != ":" }.dropFirst())
    if entry.hasPrefix("story:"), tail.hasPrefix("new-"), let k = Int(tail.dropFirst(4)), (0...9999).contains(k) { return "\(project)-NEW\(k)" }
    let safe = String(tail.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) && $0.isASCII || $0 == "." || $0 == "_" ? Character($0) : "_" }).prefix(64)
    return "\(project)-NEW-\(safe)"
}
public func apIsTicketPlaceholder(_ s: String) -> Bool { apMatch(s, apTicketPlaceholderPattern) }

// 출발하면 안 되는 키 — 대기열 단위의 story·tickets 에 자리표나 임시 번호(TMP-…)가 남았으면 그 값들
public func apUnmadeKeys(_ units: [[String: Any]]) -> [String] {
    var out: [String] = []
    for u in units {
        let vals = [u["story"] as? String].compactMap { $0 } + (u["tickets"] as? [Any] ?? []).compactMap { $0 as? String }
        for v in vals where (apIsTicketPlaceholder(v) || apMatch(v, apTempTicketPattern)) && !out.contains(v) { out.append(v) }
    }
    return out
}

// 남은 항목(만들지 않았거나 실패·확인 필요) — 출발은 이것이 0 일 때만
public func apTicketsLeft(_ p: APTicketPlan, _ b: APTicketBook) -> [APTicketEntry] {
    p.entries.filter { b.entries[$0.id]?.state != .made }
}

// MARK: - 요청 꼴(Jira Cloud REST v3, 공식 문서를 보고 정한 이 조각의 이해 — 진짜 서버는 처음 출발 때 확인)

public struct APIssueType: Equatable, Sendable {
    public var id = "", name = ""
    public var subtask = false
    public var level: Int?          // hierarchyLevel: 에픽 1, 스토리·작업 0, 하위 작업 -1
    public init(id: String = "", name: String = "", subtask: Bool = false, level: Int? = nil) { self.id = id; self.name = name; self.subtask = subtask; self.level = level }
}

private let jsonHeaders = ["Content-Type": "application/json", "Accept": "application/json"]
private func body(_ o: Any) -> Data { (try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys, .withoutEscapingSlashes])) ?? Data() }

// 프로젝트의 이슈 종류 — 종류 이름은 사이트·언어마다 달라(Story·스토리, Subtask·Sub-task·하위 작업) 이름을 박지 않고 응답에서 고른다
public func apJiraProjectRequest(_ project: String, base: String, auth: APAuth) -> APHTTPRequest {
    APHTTPRequest(url: base + "/rest/api/3/project/\(project)", headers: ["Accept": "application/json"], auth: auth)
}

public func apJiraIssueTypes(_ r: APHTTPResponse) -> [APIssueType] {
    guard r.error == nil, r.status == 200, r.body.count <= 4 << 20,
          let o = try? JSONSerialization.jsonObject(with: r.body) as? [String: Any] else { return [] }
    return (o["issueTypes"] as? [Any] ?? []).prefix(100).compactMap { x in
        guard let t = x as? [String: Any], let id = t["id"] as? String, apMatch(id, "^[0-9]{1,20}$") else { return nil }
        return APIssueType(id: id, name: apDocText(t["name"] as? String ?? "", 80), subtask: t["subtask"] as? Bool ?? false,
                           level: (t["hierarchyLevel"] as? NSNumber)?.intValue)
    }
}

// 이슈 종류 고르기 — 프로젝트 설정에 이름이 있으면 그 이름(대소문자 무시), 없으면 스토리는 하위 작업이 아닌 수준 0 의 Story·스토리,
// 작업은 하위 작업 종류(subtask 또는 수준 -1)의 Subtask·Sub-task·하위 작업, 그것도 없으면 그 무리의 첫 종류
public func apPickIssueType(_ types: [APIssueType], for kind: APTicketKind, name: String) -> APIssueType? {
    let want = name.trimmingCharacters(in: .whitespaces).lowercased()
    if !want.isEmpty { return types.first { $0.name.lowercased() == want } }
    switch kind {
    case .story:
        let pool = types.filter { !$0.subtask && ($0.level ?? 0) == 0 }
        return pool.first { ["story", "스토리"].contains($0.name.lowercased()) } ?? pool.first { $0.name.lowercased() != "task" && $0.name != "작업" && $0.name.lowercased() != "bug" && $0.name != "버그" } ?? pool.first
    case .task, .move:
        let pool = types.filter { $0.subtask || $0.level == -1 }
        return pool.first { ["subtask", "sub-task", "하위 작업"].contains($0.name.lowercased()) } ?? pool.first
    }
}

// 티켓 본문 — 첫 줄 표지, 무엇을 위한 티켓인지, 단위 제목, 보드 티켓 번호(사람이 Jira 에서 찾게)
public func apTicketDoc(_ e: APTicketEntry, run: String, temp: String = "", why: String = "", task: String = "") -> APDoc {
    var b: [APBlock] = [.paragraph([.text(apDocMarker, bold: true)])]
    if !temp.isEmpty {   // 실행 중에 새로 필요해진 티켓 — 가지는 이 번호로 만들지 않았다(임시 번호로 진행)
        b.append(.paragraph([.text("자율주행 실행 \(run) 의 단위" + (task.isEmpty ? "" : "(작업 \(task))") + "가 일하다가 새로 필요해진 티켓입니다. 실행 중에는 임시 번호 \(temp) 로 진행했고, 사람이 검수한 뒤 만든 티켓입니다.", bold: false)]))
        if !why.isEmpty { b.append(.paragraph([.text("까닭: " + why, bold: false)])) }
    } else {
        switch e.kind {
        case .story: b.append(.paragraph([.text("자율주행 실행 \(run) 의 출발 확인에서 승인한 스토리입니다. 아래 작업 티켓의 단위를 묶습니다.", bold: false)]))
        case .task, .move: b.append(.paragraph([.text("자율주행 실행 \(run) 의 단위 하나(작업 티켓)입니다. 엔진이 이 번호로 작업 가지를 만듭니다.", bold: false)]))
        }
    }
    if !e.units.isEmpty { b.append(.bullets(e.units.prefix(50).map { [APSpan.text($0, bold: false)] })) }
    b.append(.paragraph([.text("보드 티켓 번호 \(run)/\(e.id)", bold: false)]))
    return apDocClamp(APDoc(b))
}

// 만들기 — POST /rest/api/3/issue. 스토리는 parent 에 에픽, 작업(하위 작업 종류)은 parent 에 스토리
public func apJiraCreateIssueRequest(project: String, typeID: String, parent: String, summary: String, doc: APDoc, base: String, auth: APAuth) -> APHTTPRequest {
    let fields: [String: Any] = ["project": ["key": project], "issuetype": ["id": typeID], "parent": ["key": parent],
                                 "summary": apDocText(summary, apTicketTitleMax), "description": apADF(doc)]
    return APHTTPRequest(method: "POST", url: base + "/rest/api/3/issue", headers: jsonHeaders, body: body(["fields": fields]), auth: auth)
}

// 부모 바꾸기 — PUT /rest/api/3/issue/<키>(다른 스토리로 옮긴 이미 있는 작업). 하위 작업의 부모 바꾸기는 사이트 설정에 따라 거부될 수 있다(그때 실패 줄)
public func apJiraParentRequest(key: String, parent: String, base: String, auth: APAuth) -> APHTTPRequest {
    APHTTPRequest(method: "PUT", url: base + "/rest/api/3/issue/\(key)", headers: jsonHeaders, body: body(["fields": ["parent": ["key": parent]]]), auth: auth)
}

// 만든 키 — 응답(믿지 않는 값)의 key 칸이 이슈 키 꼴이고 그 프로젝트의 것일 때만
public func apCreatedIssueKey(_ r: APHTTPResponse, project: String) -> String? {
    guard r.error == nil, (200...299).contains(r.status), r.body.count <= 1 << 20,
          let o = try? JSONSerialization.jsonObject(with: r.body) as? [String: Any], let k = o["key"] as? String,
          apMatch(k, apIssueKeyPattern), k.hasPrefix(project + "-") else { return nil }
    return k
}

// 만들기 실패 까닭 — 받는 쪽 본문은 믿지 않아 상태 번호로만(Jira 티켓에 맞춘 말, 그 밖은 apFailNote)
public func apTicketFailNote(_ r: APHTTPResponse) -> String {
    if r.error != nil { return apFailNote(r) }
    switch r.status {
    case 400: return "Jira 가 요청 꼴을 받지 않았습니다(400): 이슈 종류·부모·필수 칸을 확인합니다(프로젝트 설정 tickets.issue_types)"
    case 401, 403: return "거부됨(\(r.status)): 토큰이 만료·취소됐거나 그 프로젝트에 티켓을 만들 권한이 없습니다"
    case 404: return "프로젝트나 부모 이슈를 찾지 못했습니다(404)"
    default: return apFailNote(r)
    }
}

// MARK: - 만들어도 되는가

// Atlassian 연결로 티켓을 만들 수 없는 까닭(요청 0) — 연결·범위 jira·사이트·계정 메일·토큰·만료. nil 이면 만들 수 있다
public func apTicketBlocker(_ c: APConnection?, hasSecret: Bool, dev: Bool, now: Date, tz: TimeZone) -> String? {
    guard let c, c.service == .atlassian else { return "Atlassian 연결이 없습니다(설정 › 연결)" }
    guard c.scopes.contains("jira") else { return "Atlassian 연결의 범위에 'Jira 쓰기'가 없습니다" }
    guard apServiceBase(c, dev: dev) != nil else { return "Atlassian 사이트 주소가 규칙에 맞지 않습니다" }
    if c.account.isEmpty { return "Atlassian 계정 메일이 없습니다" }
    guard hasSecret else { return "Atlassian 토큰이 키체인에 없습니다" }
    if case .expired = apExpiry(c.expires, now: now, tz: tz) { return "Atlassian 토큰 만료일(\(c.expires))이 지났습니다" }
    return nil
}

// MARK: - 만들기 유스케이스

// 계획 파일 자리의 상태 파일(tickets.json)과 그 잠금 — 쓰기는 임시 파일 + rename, 잠금은 flock(프로세스가 죽으면 풀림)
public protocol APTicketStore {
    func read() -> APTicketBook
    @discardableResult func write(_ b: APTicketBook) -> Bool
    func withLock<T>(_ body: () -> T) -> T?
}

public struct APTicketsResult: Equatable, Sendable {
    public var book = APTicketBook()
    public var requests = 0
    public var made: [String] = []          // 이번에 만든(옮긴) 항목
    public var failed: [String] = []        // 이번에 실패한 항목
    public var note = ""
    public var busy = false
    public var left = 0                     // 끝난 뒤 남은 항목 수(0 이면 출발할 수 있다)
    public init() {}
}

// 남은 것만 차례로(스토리 → 작업 → 옮기기). confirm 은 사람이 "그래도 만들기"로 확인한 확인 필요 항목.
// 스토리를 만들지 못하면 그 스토리의 작업·옮기기는 보내지 않고 기다린다. budget 은 429 기다림까지 합친 초 상한
public func apMakeTickets(_ p: APTicketPlan, store: APTicketStore, connections: [APConnection], dev: Bool, secrets: APSecrets, http: APHTTP,
                          clock: APClock, waiter: APWaiter, tz: TimeZone, confirm: Set<String> = [], budget: Double = 120) -> APTicketsResult {
    let r: APTicketsResult? = store.withLock {
        var out = APTicketsResult()
        var book = store.read()
        let start = clock.now.timeIntervalSince1970
        // 앞 만들기가 끝나지 못하고 죽었다 — 확인 필요로(사람이 Jira 에서 생겼는지 본 뒤 그래도 만들기)
        var changed = false
        for (k, v) in book.entries where v.state == .creating {
            book.entries[k]?.state = .uncertain
            book.entries[k]?.last = APAttempt(at: start, what: "uncertain", note: "앞 만들기가 끝나지 못했습니다. Jira 에 이미 생겼는지 본 뒤 그래도 만들기를 누릅니다")
            changed = true
        }
        if changed { store.write(book) }
        func finish(_ note: String) -> APTicketsResult {
            out.book = book; out.note = note; out.left = apTicketsLeft(p, book).count
            return out
        }
        let todo = apTicketsLeft(p, book)
        if todo.isEmpty { return finish("만들 티켓이 모두 있습니다(요청 0)") }
        let conn = connections.first { $0.id == "atlassian" }
        if let why = apTicketBlocker(conn, hasSecret: conn.map { secrets.exists($0.id) } ?? false, dev: dev, now: clock.now, tz: tz) {
            return finish(why + ". 요청하지 않았습니다")
        }
        let c = conn!, base = apServiceBase(c, dev: dev)!
        if !book.site.isEmpty, book.site != base, book.entries.values.contains(where: { $0.state == .made }) {
            return finish("이 계획을 만들기 시작한 사이트(\(book.site))와 지금 연결의 사이트(\(base))가 달라 요청하지 않았습니다")
        }
        let held = todo.filter { book.entries[$0.id]?.state == .uncertain && !confirm.contains($0.id) }
        if !held.isEmpty { return finish("확인 필요 \(held.count)개: 앞 만들기가 끝났는지 모릅니다. Jira 에서 확인한 뒤 그래도 만들기를 누릅니다(요청 0)") }
        guard let token = secrets.read(c.id), !token.isEmpty else { return finish("Atlassian 토큰이 키체인에 없습니다. 요청하지 않았습니다") }
        let auth = apAuthFor(c), header = apAuthHeader(auth, token: token)
        book.site = base
        var lastWait: Double?
        func send(_ req: APHTTPRequest) -> APHTTPResponse {
            guard apSameSite(req.url, base: base) else { return APHTTPResponse(error: "연결한 사이트 밖 주소라 보내지 않았습니다") }
            var attempt = 0
            while true {
                out.requests += 1
                let resp = http.send(req, auth: header)
                guard let w = apRetryWait(status: resp.status, retryAfter: resp.headers["retry-after"], attempt: attempt, now: clock.now) else { return resp }
                if clock.now.timeIntervalSince1970 + w > start + budget { lastWait = w; return resp }
                waiter.wait(w)
                attempt += 1
            }
        }
        func mark(_ id: String, _ st: APTicketState, key: String = "", code: Int = 0, note: String) {
            var rec = book.entries[id] ?? APTicketRecord()
            let t = clock.now.timeIntervalSince1970
            rec.state = st; rec.updated = t
            if !key.isEmpty { rec.key = key; rec.url = base + "/browse/" + key }
            rec.last = APAttempt(at: t, what: st.rawValue, code: code, note: note)
            book.entries[id] = rec
            store.write(book)
        }
        // 이슈 종류는 만들 것이 있을 때 한 번 읽는다(읽기 요청)
        var storyType: APIssueType?, taskType: APIssueType?
        if todo.contains(where: { $0.kind != .move }) {
            let resp = send(apJiraProjectRequest(p.project, base: base, auth: auth))
            let types = apJiraIssueTypes(resp)
            if types.isEmpty {
                let why = resp.status == 200 ? "프로젝트의 이슈 종류를 읽지 못했습니다" : "프로젝트 \(p.project) 를 읽지 못했습니다: \(apTicketFailNote(resp))"
                for e in todo where e.kind != .move { mark(e.id, .failed, code: resp.status, note: why); out.failed.append(e.id) }
                return finish(why)
            }
            storyType = apPickIssueType(types, for: .story, name: p.storyType)
            taskType = apPickIssueType(types, for: .task, name: p.taskType)
        }
        func parentKey(_ e: APTicketEntry) -> String? {
            guard e.parent.hasPrefix("story:") else { return e.parent }
            guard let r = book.entries[e.parent], r.state == .made, !r.key.isEmpty else { return nil }
            return r.key
        }
        let order: [APTicketKind] = [.story, .task, .move]
        for k in order {
            for e in todo where e.kind == k {
                guard clock.now.timeIntervalSince1970 < start + budget else {
                    return finish("시간 상한(\(Int(budget))초)이 지나 남은 \(apTicketsLeft(p, book).count)개는 다시 누를 때 만듭니다")
                }
                guard let parent = parentKey(e) else {
                    mark(e.id, .waiting, note: "스토리(\(e.parent))를 먼저 만들어야 합니다(요청 0)"); continue
                }
                let type = k == .story ? storyType : taskType
                if k != .move, type == nil {
                    mark(e.id, .failed, note: k == .story ? "프로젝트 \(p.project) 에 스토리로 쓸 이슈 종류가 없습니다(프로젝트 설정 tickets.issue_types.story 로 이름을 정합니다)"
                                                          : "프로젝트 \(p.project) 에 하위 작업 종류가 없습니다(프로젝트 설정 tickets.issue_types.task 로 이름을 정합니다)")
                    out.failed.append(e.id); continue
                }
                var rec = book.entries[e.id] ?? APTicketRecord()
                rec.state = .creating; rec.tries += 1; rec.updated = clock.now.timeIntervalSince1970
                book.entries[e.id] = rec
                store.write(book)   // 보내기 전에 만드는 중으로(죽으면 다음에 잠금을 잡은 쪽이 확인 필요로 본다)
                lastWait = nil
                if k == .move {
                    let resp = send(apJiraParentRequest(key: e.key, parent: parent, base: base, auth: auth))
                    if (200...299).contains(resp.status), resp.error == nil {
                        mark(e.id, .made, key: e.key, code: resp.status, note: "부모를 \(parent) 로 바꿨습니다"); out.made.append(e.id)
                    } else {
                        mark(e.id, .failed, code: resp.status, note: "부모 바꾸기: " + apTicketFailNote(resp) + (lastWait != nil ? " 다시 누르면 남은 것만 만듭니다" : ""))
                        out.failed.append(e.id)
                    }
                    continue
                }
                let resp = send(apJiraCreateIssueRequest(project: p.project, typeID: type!.id, parent: parent, summary: e.title,
                                                         doc: apTicketDoc(e, run: p.run), base: base, auth: auth))
                if let key = apCreatedIssueKey(resp, project: p.project) {
                    mark(e.id, .made, key: key, code: resp.status, note: "만들었습니다"); out.made.append(e.id)
                } else {
                    let why = (200...299).contains(resp.status) ? "응답에 만든 키가 없습니다(\(resp.status)). Jira 에 생겼는지 확인합니다" : apTicketFailNote(resp)
                    mark(e.id, (200...299).contains(resp.status) ? .uncertain : .failed, code: resp.status, note: why)
                    out.failed.append(e.id)
                }
            }
        }
        let left = apTicketsLeft(p, book).count
        return finish(left == 0 ? "티켓 \(out.made.count)개를 만들었습니다" : "티켓 \(out.made.count)개를 만들었고 \(left)개가 남았습니다. 다시 누르면 남은 것만 만듭니다")
    }
    if let r { return r }
    var busy = APTicketsResult()
    busy.busy = true; busy.note = "다른 곳에서 이 티켓들을 만드는 중입니다"; busy.book = store.read(); busy.left = apTicketsLeft(p, busy.book).count
    return busy
}

// MARK: - 실행 중에 새로 필요해진 티켓(임시 번호)

public let apTempTicketPattern = "^TMP-[0-9]{1,4}-[0-9]{1,2}$"
public let apNewTicketCard = "jira-new-ticket"     // 출발 카드 id — 답 temp(임시 번호로 진행, 추천)·defer(보류하고 다음 출발로)

// 단위 판정 뒤 만들 올릴 글 — 결과 기록의 new_tickets 가운데 이 단위의 임시 번호 꼴인 것만, 그 스토리 아래 하위 작업으로.
// 게시 방식과 상관없이 늘 검수 뒤 올림(실행 중 티켓 만들기 요청 0). 카드 답이 temp 가 아니면 만들지 않는다
public func apNewTicketOutbox(_ r: APReportRun, unit n: Int, story: String?, task: String?, answer: String?, now: Double) -> [APOutboxItem] {
    guard answer == "temp", let st = story, apMatch(st, apIssueKeyPattern), let u = r.units.first(where: { $0.n == n }),
          let res = u.result, ["done", "failed", "capped"].contains(u.status) else { return [] }
    let project = String(st.split(separator: "-").first ?? "")
    var out: [APOutboxItem] = [], seen = Set<String>()
    for t in res.newTickets.prefix(20) {
        guard apMatch(t.temp, apTempTicketPattern), t.temp.hasPrefix("TMP-\(n)-"), seen.insert(t.temp).inserted else { continue }
        let title = apNotifyClean(t.title, apTicketTitleMax)
        guard !title.isEmpty else { continue }
        var it = APOutboxItem()
        it.id = "u\(n)-ticket-\(t.temp)"
        it.run = r.id; it.unit = n; it.kind = "ticket"
        it.target = APOutboxTarget(service: .jira, issue: t.temp, space: project, parent: st)
        it.connection = "atlassian"
        it.title = title
        let e = APTicketEntry(id: "task:" + t.temp, kind: .task, title: title, parent: st, units: [apNotifyClean(u.title.isEmpty ? "단위 \(n)" : u.title, 200)])
        it.doc = apTicketDoc(e, run: r.id, temp: t.temp, why: apNotifyClean(t.why, 500), task: task ?? "")
        it.mode = .review   // 늘 검수 뒤
        it.created = now; it.updated = now
        out.append(it)
    }
    return out
}
