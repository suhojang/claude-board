// 자율주행 규칙(AutopilotRules) — 바깥 연결(4단계 ⑩-1, 2026-10-30, 설계 5장 6절).
// 보드 설정 › 연결이 쓰는 연결 목록(autopilot/connections.json, 비밀 없음)의 꼴, 사이트 주소 규칙(설치판은 https 만, 시험 빌드만 http://127.0.0.1),
// 토큰 만료일 판정, 키체인 항목 이름(설치판·시험 빌드가 다름), "연결 확인" 읽기 요청 하나와 그 응답 해석.
// 토큰은 키체인에만 있고 이 파일의 값(APConnection·APHTTPRequest)에는 들어가지 않는다 — 인증 머리는 보내기 직전에 apAuthHeader 로 만든다
import Foundation

// 서비스 — Jira·Confluence 는 Atlassian 연결 하나를 함께 쓴다(같은 사이트·같은 API 토큰)
public enum APService: String, CaseIterable, Equatable, Sendable {
    case atlassian, notion, figma, webhook
    public var label: String {
        switch self {
        case .atlassian: "Atlassian"
        case .notion: "Notion"
        case .figma: "Figma"
        case .webhook: "알림 webhook"
        }
    }
    // 설정 화면에 보일 범위(쓰임) — 읽기는 ⑩-2 에서 쓴다. 값은 connections.json 의 scopes 에 그대로 남는다
    public var scopeChoices: [(key: String, label: String)] {
        switch self {
        case .atlassian: [("jira", "Jira 쓰기(티켓 만들기·댓글)"), ("confluence", "Confluence 쓰기(보고서)"), ("read", "읽기")]
        case .notion: [("write", "Notion 쓰기(보고서)"), ("read", "읽기")]
        case .figma: [("read", "읽기")]
        case .webhook: [("notify", "알림")]
        }
    }
}

// 마지막 "연결 확인" — 보드가 남긴다(엔진 연결 확인 명령의 결과)
public struct APCheckRecord: Equatable, Sendable {
    public var at = 0.0
    public var ok = false
    public var who = ""      // 받는 쪽이 알려 준 계정 이름(믿지 않는 값, 자르고 다듬어 둠)
    public var note = ""
    public init(at: Double = 0, ok: Bool = false, who: String = "", note: String = "") { self.at = at; self.ok = ok; self.who = who; self.note = note }
}

// 연결 하나(비밀 없음). id 는 키체인 항목의 계정 이름이기도 하다
public struct APConnection: Equatable, Sendable {
    public var id = ""
    public var service = APService.atlassian
    public var site = ""          // Atlassian 사이트(https://<이름>.atlassian.net). Notion·Figma 는 빈 글자 = 기본 API 주소(시험 빌드만 127.0.0.1 로 바꿈)
    public var account = ""       // Atlassian·Figma 계정 메일, Notion 은 요청자 이름(본문에 씀: Notion 작성자는 통합 이름이라서)
    public var name = ""          // webhook 이름(대화방)
    public var host = ""          // webhook 주소의 호스트(보이기만 함, 주소 전체는 키체인)
    public var scopes: [String] = []
    public var expires = ""       // 토큰 만료일 yyyy-MM-dd(사람이 넣음, 없으면 빈 글자)
    public var checked: APCheckRecord?
    public var format = APNotifyFormat.json   // webhook 글 꼴(4단계 ⑩-3) — 받는 쪽마다 정한다. 다른 서비스는 쓰지 않는다
    public init(id: String = "", service: APService = .atlassian, site: String = "", account: String = "", name: String = "", host: String = "",
                scopes: [String] = [], expires: String = "", checked: APCheckRecord? = nil, format: APNotifyFormat = .json) {
        self.id = id; self.service = service; self.site = site; self.account = account; self.name = name; self.host = host
        self.scopes = scopes; self.expires = expires; self.checked = checked; self.format = format
    }
}

public let apConnectionsFormat = "claude-board-connections"

// 연결 id 꼴 — 키체인 계정 이름과 파일 칸으로 쓰므로 좁게
public func apConnectionIDOK(_ id: String) -> Bool {
    id.range(of: "^(atlassian|notion|figma|webhook-[1-9][0-9]{0,2})$", options: .regularExpression) != nil
}

// 키체인 항목(일반 암호) — 서비스 이름은 설치판·시험 빌드가 달라 시험이 설치판 토큰을 건드리지 않는다. 계정 이름은 연결 id
public func apSecretService(dev: Bool) -> String { dev ? "Claude Board Dev-connection" : "Claude Board-connection" }

private func clean(_ v: Any?, _ n: Int) -> String {
    guard let s = v as? String else { return "" }
    var o = ""
    for u in s.unicodeScalars {
        let x = u.value
        if x < 0x20 || (0x7F...0x9F).contains(x) || (0x202A...0x202E).contains(x) || (0x2066...0x2069).contains(x) || x == 0x2028 || x == 0x2029 { continue }
        o.unicodeScalars.append(u)
    }
    let t = o.trimmingCharacters(in: .whitespaces)
    return t.count > n ? String(t.prefix(n)) : t
}

// connections.json → 연결 목록. 꼴이 틀린 줄과 같은 id 의 두 번째 줄은 버린다
public func apParseConnections(_ data: Data?) -> [APConnection] {
    guard let data, data.count <= 256 << 10, let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          o["format"] as? String == apConnectionsFormat, let list = o["connections"] as? [[String: Any]] else { return [] }
    var out: [APConnection] = [], seen = Set<String>()
    for e in list.prefix(50) {
        let id = e["id"] as? String ?? ""
        guard apConnectionIDOK(id), let s = APService(rawValue: e["service"] as? String ?? ""), seen.insert(id).inserted else { continue }
        guard (s == .webhook) == id.hasPrefix("webhook-"), s == .webhook || id == s.rawValue else { continue }
        var c = APConnection(id: id, service: s)
        c.site = clean(e["site"], 300)
        c.account = clean(e["account"], 200)
        c.name = clean(e["name"], 80)
        c.host = clean(e["host"], 200)
        let allowed = Set(s.scopeChoices.map(\.key))
        c.scopes = (e["scopes"] as? [Any] ?? []).compactMap { $0 as? String }.filter { allowed.contains($0) }
        let ex = clean(e["expires"], 10)
        c.expires = apDay(ex) != nil ? ex : ""
        if let k = e["checked"] as? [String: Any] {
            c.checked = APCheckRecord(at: (k["at"] as? NSNumber)?.doubleValue ?? 0, ok: k["ok"] as? Bool ?? false,
                                      who: clean(k["who"], 120), note: clean(k["note"], 300))
        }
        if s == .webhook { c.format = APNotifyFormat(rawValue: e["format"] as? String ?? "") ?? .json }
        out.append(c)
    }
    return out
}

public func apConnectionsJSON(_ list: [APConnection]) -> [String: Any] {
    ["format": apConnectionsFormat, "version": 1, "connections": list.map { c -> [String: Any] in
        var d: [String: Any] = ["id": c.id, "service": c.service.rawValue, "scopes": c.scopes]
        if !c.site.isEmpty { d["site"] = c.site }
        if !c.account.isEmpty { d["account"] = c.account }
        if !c.name.isEmpty { d["name"] = c.name }
        if !c.host.isEmpty { d["host"] = c.host }
        if !c.expires.isEmpty { d["expires"] = c.expires }
        if let k = c.checked { d["checked"] = ["at": k.at, "ok": k.ok, "who": k.who, "note": k.note] }
        if c.service == .webhook { d["format"] = c.format.rawValue }
        return d
    }]
}

// MARK: - 사이트 주소 규칙

public enum APSiteCheck: Equatable, Sendable {
    case ok(String)      // 다듬은 주소
    case bad(String)     // 받지 않는 까닭
}

// 주소 글자의 공통 확인 — 받으면 URLComponents. 설치판은 https 만, 시험 빌드(dev)만 http://127.0.0.1(포트 아무것). localhost·다른 http 는 늘 거절
private func baseComponents(_ raw: String, dev: Bool) -> (URLComponents?, String) {
    let t = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !t.isEmpty else { return (nil, "주소가 비었습니다") }
    guard t.count <= 2000, t.unicodeScalars.allSatisfy({ $0.value > 0x20 && $0.value < 0x7F && !"\"'<>\\`{}|^".unicodeScalars.contains($0) }) else {
        return (nil, "주소에 쓸 수 없는 글자가 있습니다")
    }
    guard let c = URLComponents(string: t), let scheme = c.scheme?.lowercased(), let host = c.host?.lowercased(), !host.isEmpty else {
        return (nil, "주소 꼴이 아닙니다(https://… 로 씁니다)")
    }
    guard c.user == nil, c.password == nil else { return (nil, "주소에 아이디·암호를 넣지 않습니다") }
    if scheme == "https" { return (c, "") }
    if scheme == "http" {
        if dev && host == "127.0.0.1" { return (c, "") }
        return (nil, dev ? "http 주소는 시험 빌드에서도 http://127.0.0.1 만 받습니다" : "https 주소만 받습니다")
    }
    return (nil, "https 주소만 받습니다")
}

// Atlassian 사이트(그리고 시험 빌드에서 Notion·Figma 를 가짜 서버로 바꿀 때) — 호스트(와 포트)까지만, 경로·물음·조각은 받지 않는다. 결과는 scheme://host[:port]
public func apSiteCheck(_ raw: String, dev: Bool) -> APSiteCheck {
    let (c0, why) = baseComponents(raw, dev: dev)
    guard let c = c0 else { return .bad(why) }
    guard c.path.isEmpty || c.path == "/" || c.path == "/wiki" || c.path == "/wiki/" else { return .bad("사이트 주소는 호스트까지만 씁니다(경로 없이)") }
    guard c.query == nil, c.fragment == nil else { return .bad("사이트 주소에 ? 나 # 을 넣지 않습니다") }
    var out = "\(c.scheme!.lowercased())://\(c.host!.lowercased())"
    if let p = c.port { out += ":\(p)" }
    return .ok(out)
}

// 알림 webhook 주소 — 경로·물음은 받는다(대화방 주소에 열쇠가 들어 있어 주소 전체가 비밀). 조각(#)은 받지 않는다
public func apWebhookCheck(_ raw: String, dev: Bool) -> APSiteCheck {
    let (c0, why) = baseComponents(raw, dev: dev)
    guard let c = c0 else { return .bad(why) }
    guard c.fragment == nil else { return .bad("webhook 주소에 # 을 넣지 않습니다") }
    guard !c.path.isEmpty, c.path != "/" else { return .bad("webhook 주소에 경로가 없습니다") }
    return .ok(raw.trimmingCharacters(in: .whitespacesAndNewlines))
}

// 주소의 호스트(보이기만)
public func apHost(_ url: String) -> String { URLComponents(string: url)?.host?.lowercased() ?? "" }

public let apNotionAPI = "https://api.notion.com"
public let apFigmaAPI = "https://api.figma.com"

// 요청 바탕 주소 — Atlassian 은 사이트(규칙을 다시 봄), Notion·Figma 는 정한 API 주소이고 시험 빌드만 사이트 칸의 http://127.0.0.1 로 바꾼다.
// webhook 은 주소가 비밀이라 nil. 규칙에 맞지 않으면 nil(요청하지 않음)
public func apServiceBase(_ c: APConnection, dev: Bool) -> String? {
    switch c.service {
    case .atlassian:
        if case .ok(let s) = apSiteCheck(c.site, dev: dev) { return s }
        return nil
    case .notion, .figma:
        let fixed = c.service == .notion ? apNotionAPI : apFigmaAPI
        guard dev, !c.site.isEmpty else { return fixed }
        if case .ok(let s) = apSiteCheck(c.site, dev: true), s.hasPrefix("http://127.0.0.1") { return s }
        return fixed
    case .webhook:
        return nil
    }
}

// MARK: - 만료일

public enum APExpiry: Equatable, Sendable {
    case none                 // 만료일을 넣지 않음
    case ok(days: Int)        // 남은 날
    case soon(days: Int)      // apExpirySoonDays 안(0 이면 오늘이 만료일)
    case expired(days: Int)   // 지난 날
}
public let apExpirySoonDays = 14

// yyyy-MM-dd → 그 날(그레고리력·UTC 자정). 꼴이 틀리면 nil
public func apDay(_ s: String) -> DateComponents? {
    guard s.range(of: "^[0-9]{4}-[0-9]{2}-[0-9]{2}$", options: .regularExpression) != nil else { return nil }
    let p = s.split(separator: "-").compactMap { Int($0) }
    guard p.count == 3, (2000...2200).contains(p[0]), (1...12).contains(p[1]), (1...31).contains(p[2]) else { return nil }
    var cal = Calendar(identifier: .gregorian)
    cal.timeZone = TimeZone(identifier: "UTC")!
    let dc = DateComponents(year: p[0], month: p[1], day: p[2])
    guard let d = cal.date(from: dc), cal.component(.day, from: d) == p[2] else { return nil }   // 2월 30일 같은 날은 nil
    return dc
}

// 만료일까지 남은 날(그 사람의 시간대로 오늘을 센다). 만료일 당일은 soon(0)
public func apExpiry(_ expires: String, now: Date, tz: TimeZone) -> APExpiry {
    guard let e = apDay(expires) else { return .none }
    var local = Calendar(identifier: .gregorian)
    local.timeZone = tz
    let t = local.dateComponents([.year, .month, .day], from: now)
    var utc = Calendar(identifier: .gregorian)
    utc.timeZone = TimeZone(identifier: "UTC")!
    guard let a = utc.date(from: DateComponents(year: t.year, month: t.month, day: t.day)), let b = utc.date(from: e) else { return .none }
    let days = utc.dateComponents([.day], from: a, to: b).day ?? 0
    if days < 0 { return .expired(days: -days) }
    if days <= apExpirySoonDays { return .soon(days: days) }
    return .ok(days: days)
}

public func apExpiryLabel(_ e: APExpiry, _ expires: String) -> String {
    switch e {
    case .none: "만료일 없음"
    case .ok(let d): "만료 \(expires) · \(d)일 남음"
    case .soon(0): "오늘 만료(\(expires))"
    case .soon(let d): "만료 임박 · \(d)일 남음(\(expires))"
    case .expired(let d): "만료됨 · \(d)일 지남(\(expires))"
    }
}

// MARK: - 요청 값과 인증 머리

// 보낼 요청 — 토큰은 들어가지 않는다(auth 는 어떤 꼴의 인증인지만). 어댑터는 리다이렉트를 따르지 않는다
public enum APAuth: Equatable, Sendable {
    case none
    case basic(user: String)   // Atlassian: 메일:토큰
    case bearer                // Notion: Bearer 토큰
    case figma                 // Figma: X-Figma-Token
}

public struct APHTTPRequest: Equatable, Sendable {
    public var method = "GET"
    public var url = ""
    public var headers: [String: String] = [:]
    public var body: Data?
    public var auth = APAuth.none
    public init(method: String = "GET", url: String = "", headers: [String: String] = [:], body: Data? = nil, auth: APAuth = .none) {
        self.method = method; self.url = url; self.headers = headers; self.body = body; self.auth = auth
    }
}

public struct APHTTPResponse: Equatable, Sendable {
    public var status = 0              // 0 = 응답 없음(error 에 까닭)
    public var headers: [String: String] = [:]   // 이름은 소문자
    public var body = Data()
    public var error: String?
    public init(status: Int = 0, headers: [String: String] = [:], body: Data = Data(), error: String? = nil) {
        self.status = status; self.headers = headers; self.body = body; self.error = error
    }
}

// 인증 머리 — 보내기 직전에만 만든다(값을 남기지 않는다)
public func apAuthHeader(_ a: APAuth, token: String) -> (name: String, value: String)? {
    switch a {
    case .none: return nil
    case .basic(let user): return ("Authorization", "Basic " + Data("\(user):\(token)".utf8).base64EncodedString())
    case .bearer: return ("Authorization", "Bearer " + token)
    case .figma: return ("X-Figma-Token", token)
    }
}

public let apNotionVersion = "2022-06-28"

// 요청의 주소가 그 연결의 바탕 주소 아래인가 — 아니면 보내지 않는다(받는 쪽 응답이나 글에 든 주소로 토큰을 보내지 않게)
public func apSameSite(_ url: String, base: String) -> Bool {
    guard let u = URLComponents(string: url), let b = URLComponents(string: base) else { return false }
    return u.scheme?.lowercased() == b.scheme?.lowercased() && u.host?.lowercased() == b.host?.lowercased() && u.port == b.port
        && u.user == nil && u.password == nil
}

// 서비스마다 인증 꼴
public func apAuthFor(_ c: APConnection) -> APAuth {
    switch c.service {
    case .atlassian: .basic(user: c.account)
    case .notion: .bearer
    case .figma: .figma
    case .webhook: .none
    }
}

// "연결 확인" 읽기 요청 하나(설계 5장 6절). webhook 은 읽는 길이 없어 nil(주소 꼴만 본다 — 시험 글 보내기는 ⑩-3 알림과 함께)
public func apCheckRequest(_ c: APConnection, dev: Bool) -> APHTTPRequest? {
    guard let base = apServiceBase(c, dev: dev) else { return nil }
    switch c.service {
    case .atlassian:
        return APHTTPRequest(url: base + "/rest/api/3/myself", headers: ["Accept": "application/json"], auth: apAuthFor(c))
    case .notion:
        return APHTTPRequest(url: base + "/v1/users/me", headers: ["Accept": "application/json", "Notion-Version": apNotionVersion], auth: .bearer)
    case .figma:
        return APHTTPRequest(url: base + "/v1/me", headers: ["Accept": "application/json"], auth: .figma)
    case .webhook:
        return nil
    }
}

public enum APCheckOutcome: Equatable, Sendable {
    case ok(who: String)          // 읽기 요청이 됨(받는 쪽이 알려 준 이름)
    case rejected(String)         // 401·403 — 토큰이 틀렸거나 만료·취소됨
    case failed(String)           // 그 밖(응답 없음·다른 곳으로 보냄·꼴이 틀린 응답)
    case local(String)            // 요청 없이 끝남(토큰 없음·주소 규칙·webhook)
}

// 응답 본문(믿지 않는 값)에서 이름 한 칸 — JSON 객체의 정한 칸만, 자르고 다듬는다
private func who(_ c: APConnection, _ body: Data) -> String {
    guard body.count <= 1 << 20, let o = try? JSONSerialization.jsonObject(with: body) as? [String: Any] else { return "" }
    switch c.service {
    case .atlassian: return clean(o["displayName"] ?? o["emailAddress"], 120)
    case .notion:
        let owner = ((o["bot"] as? [String: Any])?["owner"] as? [String: Any])?["user"] as? [String: Any]
        let n = clean(o["name"], 120)
        let u = clean(owner?["name"], 120)
        return u.isEmpty ? n : (n.isEmpty ? u : "\(n) · \(u)")
    case .figma: return clean(o["email"] ?? o["handle"], 120)
    case .webhook: return ""
    }
}

public func apCheckOutcome(_ c: APConnection, _ r: APHTTPResponse) -> APCheckOutcome {
    if let e = r.error { return .failed(e) }
    switch r.status {
    case 200: let w = who(c, r.body); return w.isEmpty ? .failed("응답 꼴이 예상과 다릅니다") : .ok(who: w)
    case 401, 403: return .rejected("거부됨(\(r.status)): 토큰이 틀렸거나 만료·취소됐습니다")
    case 429: return .failed("요청이 많다며 거절했습니다(429). 잠시 뒤 다시 확인합니다")
    case 300...399: return .failed("다른 주소로 보내려 해 따라가지 않았습니다(\(r.status))")
    default: return .failed("받는 쪽이 \(r.status) 로 답했습니다")
    }
}

// 연결 확인 유스케이스 — 토큰이 없거나 주소가 규칙 밖이면 요청 0
public func apCheckConnection(_ c: APConnection, dev: Bool, secrets: APSecrets, http: APHTTP) -> APCheckOutcome {
    if c.service == .webhook {
        guard secrets.exists(c.id) else { return .local("webhook 주소가 키체인에 없습니다") }
        return .local("webhook 은 읽는 요청이 없어 주소 꼴만 봤습니다. 대화방에 닿는지는 '시험 글 보내기'로 봅니다")
    }
    guard let req = apCheckRequest(c, dev: dev), let base = apServiceBase(c, dev: dev), apSameSite(req.url, base: base) else {
        return .local("사이트 주소가 규칙에 맞지 않아 요청하지 않았습니다")
    }
    if case .basic(let u) = req.auth, u.isEmpty { return .local("계정 메일이 없어 요청하지 않았습니다") }
    guard let token = secrets.read(c.id), !token.isEmpty else { return .local("토큰이 키체인에 없어 요청하지 않았습니다") }
    return apCheckOutcome(c, http.send(req, auth: apAuthHeader(req.auth, token: token)))
}
