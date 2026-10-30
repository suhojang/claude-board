// 자율주행 규칙(AutopilotRules) — 내장 읽기(4단계 ⑩-2, 2026-10-30, 설계 5장 6절 "읽기"): Jira·Confluence·Notion·Figma 를 REST 로 읽어 메타 형식 하나로.
// 링크 → 읽을 대상(apReadTarget: Atlassian 은 연결한 사이트와 같은 호스트일 때만, Notion 은 주소 끝 32자리, Figma 는 파일 키와 node-id),
// 읽기 요청 꼴(GET 만), 응답 → 메타(마크업을 뺀 글·목차·핵심 칸·첨부와 링크·요약 몇 줄·가리킨 절, Figma 는 프레임 표와 그림 한 장),
// 유스케이스 apReadLink(⑩-1 의 apPostItem 처럼 안에서 apSameSite 를 걸어 연결한 바탕 주소 밖으로는 요청하지 않는다).
// 요청은 늘 연결의 바탕 주소와 id 로 만든다 — 링크 글자에 든 주소로 토큰을 보내지 않는다. Figma 그림은 /v1/images 가 돌려준 다른 호스트 주소라
// 허용 목록(apImageURLOK)을 지날 때만, 인증 머리 없이 받는다
import Foundation

// MARK: - 링크 → 읽을 대상

public enum APReadTarget: Equatable, Sendable {
    case jira(key: String)
    case confluence(page: String)
    case notion(page: String)              // 8-4-4-4-12 꼴
    case figma(file: String, node: String?) // node 는 API 꼴(12:4)
    case web                                // 내장하지 않음 — MCP 읽기 세션으로
    case bad(String)                        // 주소 꼴이 틀림

    public var service: APService? {
        switch self {
        case .jira, .confluence: .atlassian
        case .notion: .notion
        case .figma: .figma
        case .web, .bad: nil
        }
    }
    public var label: String {
        switch self {
        case .jira: "Jira"
        case .confluence: "Confluence"
        case .notion: "Notion"
        case .figma: "Figma"
        case .web: "웹"
        case .bad: "웹"
        }
    }
    public var sourceKey: String {
        switch self {
        case .jira: "jira"
        case .confluence: "confluence"
        case .notion: "notion"
        case .figma: "figma"
        case .web, .bad: "web"
        }
    }
}

// Notion 주소 끝(또는 ?p=)의 32자리 16진수 → 8-4-4-4-12
public func apNotionID(_ s: String) -> String? {
    let hex = s.lowercased().filter { $0.isHexDigit }
    guard hex.count == 32 else { return nil }
    let a = Array(hex)
    return [String(a[0..<8]), String(a[8..<12]), String(a[12..<16]), String(a[16..<20]), String(a[20..<32])].joined(separator: "-")
}

// 링크 하나를 읽을 대상으로. Atlassian 링크는 연결한 사이트(시험 빌드는 http://127.0.0.1:<포트> 도)와 같은 호스트일 때만 Confluence·Jira 로 본다 —
// 다른 사이트의 Atlassian 링크는 종류만 알리고(연결 필요) 그 사이트로 요청하지 않는다
public func apReadTarget(_ raw: String, atlassianSite: String?) -> APReadTarget {
    let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let u = URLComponents(string: s), let sc = u.scheme?.lowercased(), sc == "https" || sc == "http", let host = u.host?.lowercased(), !host.isEmpty else {
        return .bad("웹 주소 꼴이 아닙니다(https:// 로 시작하는 주소를 붙여 넣습니다)")
    }
    guard u.user == nil, u.password == nil else { return .bad("사용자 이름·비밀번호가 든 주소는 받지 않습니다") }
    let path = u.path
    let atl = host.hasSuffix(".atlassian.net") || (atlassianSite.map { apSameSite(s, base: $0) } ?? false)
    if atl {
        let p = path.split(separator: "/").map(String.init)
        if let i = p.firstIndex(of: "browse"), i + 1 < p.count, p[i + 1].range(of: apIssueKeyPattern, options: .regularExpression) != nil { return .jira(key: p[i + 1]) }
        if let k = u.queryItems?.first(where: { $0.name == "selectedIssue" })?.value, k.range(of: apIssueKeyPattern, options: .regularExpression) != nil { return .jira(key: k) }
        if path.hasPrefix("/wiki") {
            if let i = p.firstIndex(of: "pages"), i + 1 < p.count, p[i + 1].range(of: "^[0-9]{1,20}$", options: .regularExpression) != nil { return .confluence(page: p[i + 1]) }
            if let id = u.queryItems?.first(where: { $0.name == "pageId" })?.value, id.range(of: "^[0-9]{1,20}$", options: .regularExpression) != nil { return .confluence(page: id) }
            return .bad("Confluence 페이지 주소에서 페이지 번호를 찾지 못했습니다(…/pages/<번호>/… 꼴로 붙여 넣습니다. 짧은 주소 /wiki/x/… 는 아직 읽지 않습니다)")
        }
        return .bad("Jira 이슈(/browse/<키>)나 Confluence 페이지(/wiki/…/pages/<번호>) 주소가 아닙니다")
    }
    if host == "notion.so" || host.hasSuffix(".notion.so") || host.hasSuffix(".notion.site") {
        if let p = u.queryItems?.first(where: { $0.name == "p" })?.value, let id = apNotionID(p) { return .notion(page: id) }
        let last = path.split(separator: "/").last.map(String.init) ?? ""
        let tail = last.split(separator: "-").last.map(String.init) ?? last
        if let id = apNotionID(tail) { return .notion(page: id) }
        return .bad("Notion 페이지 주소 끝의 32자리 id 를 찾지 못했습니다")
    }
    if host == "figma.com" || host.hasSuffix(".figma.com") {
        let p = path.split(separator: "/").map(String.init)
        guard p.count >= 2, ["design", "file", "proto", "board"].contains(p[0]) else { return .bad("Figma 파일 주소(/design/<키>/…)가 아닙니다") }
        var key = p[1]
        if p.count >= 4, p[2] == "branch" { key = p[3] }
        guard key.range(of: "^[A-Za-z0-9]{10,64}$", options: .regularExpression) != nil else { return .bad("Figma 파일 키 꼴이 아닙니다") }
        var node: String?
        if let n = u.queryItems?.first(where: { $0.name == "node-id" })?.value {
            let api = n.replacingOccurrences(of: "-", with: ":")
            if api.range(of: "^[0-9]{1,9}:[0-9]{1,9}$", options: .regularExpression) != nil { node = api }
        }
        return .figma(file: key, node: node)
    }
    return .web
}

// 읽을 수 있는 연결인가 — 연결이 있고 범위에 "읽기"가 있고 토큰이 키체인에 있고 만료되지 않고 바탕 주소가 규칙 안일 때 nil, 아니면 까닭(요청 0)
public func apReadBlocker(_ service: APService, connection c: APConnection?, hasSecret: Bool, dev: Bool, now: Date, tz: TimeZone) -> String? {
    guard let c else { return "\(service.label) 연결이 없습니다(설정 › 연결)" }
    guard c.scopes.contains("read") else { return "\(service.label) 연결의 범위에 '읽기'가 없습니다(설정 › 연결)" }
    guard hasSecret else { return "\(service.label) 토큰이 키체인에 없습니다(설정 › 연결)" }
    if case .expired = apExpiry(c.expires, now: now, tz: tz) { return "\(service.label) 토큰 만료일(\(c.expires))이 지났습니다" }
    guard apServiceBase(c, dev: dev) != nil else { return "\(service.label) 사이트 주소가 규칙에 맞지 않습니다" }
    if case .basic(let u) = apAuthFor(c), u.isEmpty { return "Atlassian 계정 메일이 없습니다" }
    return nil
}

// 보드의 자료 링크 줄 — 종류와 읽기 상태(connected·needs_connection·unreadable·mcp)와 까닭. 연결 목록·키체인 있음 여부·고른 MCP 서버로 매긴다
public func apLinkState(_ raw: String, connections: [APConnection], secrets: Set<String>, mcp: String?, dev: Bool, now: Date, tz: TimeZone) -> APLink {
    let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    let site = connections.first { $0.service == .atlassian }.flatMap { apServiceBase($0, dev: dev) }
    let t = apReadTarget(s, atlassianSite: site)
    switch t {
    case .bad(let why): return APLink(url: s, kind: apLinkInfo(s).kind, state: "unreadable", why: why)
    case .web:
        if let m = mcp, apMCPServerNameOK(m) { return APLink(url: s, kind: "웹", state: "mcp", why: "MCP 서버 \(m) 의 읽기 도구로 읽습니다(그 서버 하나만 켠 짧은 세션)") }
        return APLink(url: s, kind: "웹", state: "needs_connection", why: "내장하지 않은 주소라 MCP 서버를 골라 읽습니다(읽기 도구만 켠 짧은 세션)")
    default:
        let svc = t.service!
        let c = connections.first { $0.service == svc }
        if svc == .atlassian, let c, let base = apServiceBase(c, dev: dev), !apSameSite(s, base: base) {
            return APLink(url: s, kind: t.label, state: "needs_connection", why: "연결한 Atlassian 사이트(\(apHost(base)))와 다른 사이트의 주소입니다")
        }
        if let why = apReadBlocker(svc, connection: c, hasSecret: c.map { secrets.contains($0.id) } ?? false, dev: dev, now: now, tz: tz) {
            return APLink(url: s, kind: t.label, state: "needs_connection", why: why)
        }
        return APLink(url: s, kind: t.label, state: "connected", why: "\(t.label) 연결로 메타 정보만 읽습니다")
    }
}

// MARK: - 읽기 요청 꼴(GET 만)

private let readHeaders = ["Accept": "application/json"]
private let unreserved = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
private func q(_ s: String) -> String { s.addingPercentEncoding(withAllowedCharacters: unreserved) ?? "" }   // 주소 한 칸(경로 조각·물음 값)

public let apJiraReadFields = "summary,status,issuetype,assignee,reporter,priority,labels,updated,created,description,attachment,issuelinks,parent,fixVersions,components"

public func apJiraIssueRequest(_ key: String, base: String, auth: APAuth, fields: String = apJiraReadFields) -> APHTTPRequest {
    APHTTPRequest(url: base + "/rest/api/3/issue/\(q(key))?fields=\(fields.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? "")", headers: readHeaders, auth: auth)
}
public func apConfluenceReadRequest(_ page: String, base: String, auth: APAuth) -> APHTTPRequest {
    APHTTPRequest(url: base + "/wiki/rest/api/content/\(q(page))?expand=body.storage,version,space,history,children.attachment,metadata.labels",
                  headers: readHeaders, auth: auth)
}
public func apNotionPageGetRequest(_ page: String, base: String) -> APHTTPRequest {
    APHTTPRequest(url: base + "/v1/pages/\(q(page))", headers: ["Accept": "application/json", "Notion-Version": apNotionVersion], auth: .bearer)
}
public func apNotionChildrenRequest(_ block: String, base: String, cursor: String?) -> APHTTPRequest {
    APHTTPRequest(url: base + "/v1/blocks/\(q(block))/children?page_size=100" + (cursor.map { "&start_cursor=\(q($0))" } ?? ""),
                  headers: ["Accept": "application/json", "Notion-Version": apNotionVersion], auth: .bearer)
}
public func apFigmaReadRequest(_ file: String, node: String?, base: String) -> APHTTPRequest {
    let url = node.map { base + "/v1/files/\(q(file))/nodes?ids=\(q($0))" } ?? base + "/v1/files/\(q(file))?depth=2"
    return APHTTPRequest(url: url, headers: readHeaders, auth: .figma)
}
public func apFigmaImageRequest(_ file: String, node: String, base: String) -> APHTTPRequest {
    APHTTPRequest(url: base + "/v1/images/\(q(file))?ids=\(q(node))&format=png&scale=1", headers: readHeaders, auth: .figma)
}

// Figma 그림 주소 허용 — /v1/images 가 돌려주는 내보낸 그림 자리(https 의 정한 호스트: figma-alpha-api.s3.us-west-2.amazonaws.com 과 *.figma.com)만.
// 시험 빌드는 가짜 서버 http://127.0.0.1(포트 아무것)도. 사용자 정보·다른 포트(https 는 443)·다른 호스트는 받지 않는다. 이 요청에는 인증 머리를 붙이지 않는다
public let apFigmaImageHosts: Set<String> = ["figma-alpha-api.s3.us-west-2.amazonaws.com"]
public let apFigmaImageHostSuffix = ".figma.com"
public func apImageURLOK(_ url: String, dev: Bool) -> Bool {
    guard url.count <= 4000, url.unicodeScalars.allSatisfy({ $0.value > 0x20 && $0.value < 0x7F && !"\"'<>\\`{}|^".unicodeScalars.contains($0) }),
          let c = URLComponents(string: url), let sc = c.scheme?.lowercased(), let h = c.host?.lowercased(), !h.isEmpty,
          c.user == nil, c.password == nil else { return false }
    if sc == "https" {
        guard c.port == nil || c.port == 443 else { return false }
        return apFigmaImageHosts.contains(h) || (h.hasSuffix(apFigmaImageHostSuffix) && h.count > apFigmaImageHostSuffix.count)
    }
    return dev && sc == "http" && h == "127.0.0.1"
}
public let apImageMaxBytes = 8 << 20
public func apIsPNG(_ d: Data) -> Bool { d.count > 8 && d.prefix(8) == Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) }

// MARK: - 응답 → 메타

private func s(_ v: Any?) -> String { (v as? String) ?? ((v as? NSNumber).map { "\($0)" } ?? "") }
private func dict(_ v: Any?) -> [String: Any] { v as? [String: Any] ?? [:] }
private func arr(_ v: Any?) -> [[String: Any]] { (v as? [Any] ?? []).compactMap { $0 as? [String: Any] } }

// Jira 문서 형식(ADF) → 글 덩어리와 링크. 머리(heading)로 나누고, 글 마디·멘션·카드 주소·줄바꿈을 줄로
public func apADFChunks(_ doc: Any?) -> (chunks: [APTextChunk], links: [APMetaLink]) {
    var chunks: [APTextChunk] = [], cur = APTextChunk(0, "", ""), lines: [String] = [], links: [APMetaLink] = []
    var budget = 20000
    func inline(_ n: [String: Any]) -> String {
        budget -= 1
        guard budget > 0 else { return "" }
        switch s(n["type"]) {
        case "text":
            let t = s(n["text"])
            for m in arr(n["marks"]) where s(m["type"]) == "link" { links.append(APMetaLink(t, s(dict(m["attrs"])["href"]), "link")) }
            return t
        case "hardBreak": return "\n"
        case "mention": return s(dict(n["attrs"])["text"])
        case "emoji": return s(dict(n["attrs"])["text"]).isEmpty ? s(dict(n["attrs"])["shortName"]) : s(dict(n["attrs"])["text"])
        case "inlineCard", "blockCard", "embedCard":
            let u = s(dict(n["attrs"])["url"]); if !u.isEmpty { links.append(APMetaLink(u, u, "link")) }; return u
        case "date": return s(dict(n["attrs"])["timestamp"])
        case "status": return s(dict(n["attrs"])["text"])
        default: return arr(n["content"]).map(inline).joined()
        }
    }
    func block(_ n: [String: Any]) {
        budget -= 1
        guard budget > 0 else { return }
        switch s(n["type"]) {
        case "heading":
            cur.text = lines.joined(separator: "\n"); lines = []
            if cur.level > 0 || !cur.text.isEmpty { chunks.append(cur) }
            cur = APTextChunk((dict(n["attrs"])["level"] as? NSNumber)?.intValue ?? 1, arr(n["content"]).map(inline).joined().replacingOccurrences(of: "\n", with: " "), "")
        case "paragraph", "codeBlock":
            let t = arr(n["content"]).map(inline).joined()
            lines += t.components(separatedBy: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        case "mediaSingle", "mediaGroup", "media", "rule": break
        case "blockCard", "embedCard": _ = inline(n)
        default: for c in arr(n["content"]) { block(c) }   // 목록·표·인용·패널·확장 칸은 안의 덩이를 차례로
        }
    }
    for c in arr(dict(doc)["content"]) { block(c) }
    cur.text = lines.joined(separator: "\n")
    if cur.level > 0 || !cur.text.isEmpty { chunks.append(cur) }
    return (chunks, links)
}

private func name(_ v: Any?) -> String { let d = dict(v); return s(d["displayName"]).isEmpty ? s(d["name"]) : s(d["displayName"]) }

public func apMetaFromJira(_ o: [String: Any], key: String, base: String, want: [String], prompt: String, dev: Bool) -> APMeta {
    let f = dict(o["fields"])
    var m = APMeta()
    m.source = "jira"
    m.id = s(o["key"]).range(of: apIssueKeyPattern, options: .regularExpression) != nil ? s(o["key"]) : key
    m.url = base + "/browse/" + m.id
    m.title = s(f["summary"])
    m.kind = name(f["issuetype"])
    m.modified = s(f["updated"])
    m.author = name(f["reporter"])
    let (chunks, links) = apADFChunks(f["description"])
    let c = apChunked(chunks, want: want, prompt: prompt)
    m.toc = c.toc; m.length = c.length; m.summary = c.summary; m.sections = c.sections
    var fields: [APMetaField] = [APMetaField("상태", name(f["status"])), APMetaField("담당", name(f["assignee"])),
                                 APMetaField("우선순위", name(f["priority"]))]
    if let p = f["parent"] as? [String: Any] { fields.append(APMetaField("상위", s(p["key"]) + " " + s(dict(p["fields"])["summary"]))) }
    let labels = (f["labels"] as? [Any] ?? []).compactMap { $0 as? String }
    if !labels.isEmpty { fields.append(APMetaField("레이블", labels.joined(separator: ", "))) }
    let comps = arr(f["components"]).map { s($0["name"]) }.filter { !$0.isEmpty }
    if !comps.isEmpty { fields.append(APMetaField("구성 요소", comps.joined(separator: ", "))) }
    let fix = arr(f["fixVersions"]).map { s($0["name"]) }.filter { !$0.isEmpty }
    if !fix.isEmpty { fields.append(APMetaField("수정 버전", fix.joined(separator: ", "))) }
    if !s(f["created"]).isEmpty { fields.append(APMetaField("만든 때", s(f["created"]))) }
    m.fields = fields.filter { !$0.value.trimmingCharacters(in: .whitespaces).isEmpty }
    var ls: [APMetaLink] = arr(f["attachment"]).map { APMetaLink(s($0["filename"]), s($0["content"]), "attachment") }
    for l in arr(f["issuelinks"]) {
        let t = dict(l["type"])
        if let x = l["outwardIssue"] as? [String: Any] { ls.append(APMetaLink("\(s(t["outward"])) \(s(x["key"])) \(s(dict(x["fields"])["summary"]))", base + "/browse/" + s(x["key"]), "issue")) }
        if let x = l["inwardIssue"] as? [String: Any] { ls.append(APMetaLink("\(s(t["inward"])) \(s(x["key"])) \(s(dict(x["fields"])["summary"]))", base + "/browse/" + s(x["key"]), "issue")) }
    }
    m.links = ls + links
    return apMetaClamp(m, dev: dev)
}

// Confluence 저장 형식에서 링크·첨부 이름
private func storageLinks(_ x: String, base: String) -> [APMetaLink] {
    var out: [APMetaLink] = []
    for g in reAllCI("<a\\b[^>]*\\bhref=\"([^\"]*)\"[^>]*>(.*?)</a\\s*>", x) { out.append(APMetaLink(apStripMarkup(g[2]), apDecodeEntities(g[1]), "link")) }
    for g in reAllCI("<ri:url\\b[^>]*\\bri:value=\"([^\"]*)\"", x) { let u = apDecodeEntities(g[1]); out.append(APMetaLink(u, u, "link")) }
    for g in reAllCI("<ri:page\\b[^>]*\\bri:content-title=\"([^\"]*)\"", x) { out.append(APMetaLink(apDecodeEntities(g[1]), "", "page")) }
    for g in reAllCI("<ri:attachment\\b[^>]*\\bri:filename=\"([^\"]*)\"", x) { out.append(APMetaLink(apDecodeEntities(g[1]), "", "attachment")) }
    return out.map { l in
        var u = l.url
        if u.hasPrefix("/") { u = base + u }
        return APMetaLink(l.title, u, l.kind)
    }
}
private func reAllCI(_ p: String, _ s: String) -> [[String]] {
    guard let re = try? NSRegularExpression(pattern: p, options: [.caseInsensitive, .dotMatchesLineSeparators]) else { return [] }
    return re.matches(in: s, range: NSRange(s.startIndex..., in: s)).prefix(300).map { m in
        (0..<m.numberOfRanges).map { k in Range(m.range(at: k), in: s).map { String(s[$0]) } ?? "" }
    }
}

public func apMetaFromConfluence(_ o: [String: Any], page: String, base: String, want: [String], prompt: String, dev: Bool) -> APMeta {
    var m = APMeta()
    m.source = "confluence"
    // 주소는 응답의 주소 칸을 믿지 않고 사이트와 번호로 만든다(⑩-1 과 같은 꼴)
    m.id = s(o["id"]).range(of: "^[0-9]{1,20}$", options: .regularExpression) != nil ? s(o["id"]) : page
    let space = dict(o["space"])
    let sk = s(space["key"])
    m.url = sk.range(of: apSpaceKeyPattern, options: .regularExpression) != nil ? base + "/wiki/spaces/\(sk)/pages/\(m.id)" : base + "/wiki/pages/viewpage.action?pageId=\(m.id)"
    m.title = s(o["title"])
    m.kind = "Confluence " + (s(o["type"]) == "blogpost" ? "블로그 글" : "페이지")
    let ver = dict(o["version"])
    m.modified = s(ver["when"])
    m.author = name(dict(o["history"])["createdBy"])
    let storage = s(dict(dict(o["body"])["storage"])["value"])
    let c = apChunked(apMarkupChunks(storage), want: want, prompt: prompt)
    m.toc = c.toc; m.length = c.length; m.summary = c.summary; m.sections = c.sections
    var fields = [APMetaField("스페이스", [s(space["key"]), s(space["name"])].filter { !$0.isEmpty }.joined(separator: " · ")),
                  APMetaField("상태", s(o["status"])), APMetaField("판", s(ver["number"])), APMetaField("마지막 고친 사람", name(ver["by"]))]
    let labels = arr(dict(dict(o["metadata"])["labels"])["results"]).map { s($0["name"]) }.filter { !$0.isEmpty }
    if !labels.isEmpty { fields.append(APMetaField("레이블", labels.joined(separator: ", "))) }
    m.fields = fields.filter { !$0.value.isEmpty }
    let atts = arr(dict(dict(o["children"])["attachment"])["results"]).map { a -> APMetaLink in
        let d = s(dict(a["_links"])["download"])
        return APMetaLink(s(a["title"]), d.hasPrefix("/") ? base + "/wiki" + d : d, "attachment")
    }
    m.links = atts + storageLinks(storage, base: base)
    return apMetaClamp(m, dev: dev)
}

// Notion 글 칸(rich_text 배열) → 글과 링크
private func notionRich(_ v: Any?, _ links: inout [APMetaLink]) -> String {
    arr(v).map { r in
        let t = s(r["plain_text"]).isEmpty ? s(dict(r["text"])["content"]) : s(r["plain_text"])
        let h = s(r["href"])
        if !h.isEmpty { links.append(APMetaLink(t, h, "link")) }
        return t
    }.joined()
}

// Notion 속성 하나 → 보일 글(제목 칸은 따로)
private func notionProp(_ p: [String: Any]) -> String {
    var none: [APMetaLink] = []
    switch s(p["type"]) {
    case "select", "status": return s(dict(p[s(p["type"])])["name"])
    case "multi_select": return arr(p["multi_select"]).map { s($0["name"]) }.joined(separator: ", ")
    case "people": return arr(p["people"]).map { s($0["name"]) }.filter { !$0.isEmpty }.joined(separator: ", ")
    case "date": let d = dict(p["date"]); return [s(d["start"]), s(d["end"])].filter { !$0.isEmpty }.joined(separator: " ~ ")
    case "checkbox": return (p["checkbox"] as? Bool ?? false) ? "예" : "아니요"
    case "number": return s(p["number"])
    case "url", "email", "phone_number": return s(p[s(p["type"])])
    case "rich_text": return notionRich(p["rich_text"], &none)
    case "created_time", "last_edited_time": return s(p[s(p["type"])])
    default: return ""
    }
}

public func apMetaFromNotion(page o: [String: Any], blocks: [[String: Any]], id: String, link: String, want: [String], prompt: String, dev: Bool) -> APMeta {
    var m = APMeta()
    m.source = "notion"
    m.id = apNotionID(s(o["id"])) ?? id
    let pu = s(o["url"])
    m.url = ["www.notion.so", "notion.so"].contains(apHost(pu)) || apHost(pu).hasSuffix(".notion.site") ? pu : link
    var links: [APMetaLink] = []
    var fields: [APMetaField] = []
    for (k, v) in dict(o["properties"]).sorted(by: { $0.key < $1.key }) {
        let p = dict(v)
        if s(p["type"]) == "title" { m.title = notionRich(p["title"], &links); continue }
        let x = notionProp(p)
        if !x.isEmpty { fields.append(APMetaField(k, x)) }
    }
    m.fields = fields
    m.kind = s(o["object"]) == "page" ? "Notion 페이지" : "Notion " + s(o["object"])
    m.modified = s(o["last_edited_time"])
    m.author = s(dict(o["created_by"])["name"])
    var chunks: [APTextChunk] = [], cur = APTextChunk(0, "", ""), lines: [String] = []
    for b in blocks {
        let t = s(b["type"])
        let body = dict(b[t])
        switch t {
        case "heading_1", "heading_2", "heading_3":
            cur.text = lines.joined(separator: "\n"); lines = []
            if cur.level > 0 || !cur.text.isEmpty { chunks.append(cur) }
            cur = APTextChunk(Int(t.suffix(1)) ?? 1, notionRich(body["rich_text"], &links), "")
        case "child_page", "child_database": links.append(APMetaLink(s(body["title"]), "", "page"))
        case "bookmark", "link_preview", "embed":
            let u = s(body["url"]), cap = notionRich(body["caption"], &links)
            links.append(APMetaLink(cap.isEmpty ? u : cap, u, "link"))
        case "image", "file", "pdf", "video":
            let u = s(dict(body["external"])["url"]).isEmpty ? s(dict(body["file"])["url"]) : s(dict(body["external"])["url"])
            let cap = notionRich(body["caption"], &links)
            links.append(APMetaLink(cap.isEmpty ? (s(body["name"]).isEmpty ? t : s(body["name"])) : cap, u, t == "image" ? "image" : "attachment"))
        default:
            let x = notionRich(body["rich_text"], &links)
            if !x.isEmpty { lines += x.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty } }
        }
    }
    cur.text = lines.joined(separator: "\n")
    if cur.level > 0 || !cur.text.isEmpty { chunks.append(cur) }
    let c = apChunked(chunks, want: want, prompt: prompt)
    m.toc = c.toc; m.length = c.length; m.summary = c.summary; m.sections = c.sections
    m.links = links
    return apMetaClamp(m, dev: dev)
}

// Figma 색 → #RRGGBB(불투명이 아니면 AA 를 붙인다)
private func hex(_ c: [String: Any], opacity: Double) -> String? {
    guard let r = (c["r"] as? NSNumber)?.doubleValue, let g = (c["g"] as? NSNumber)?.doubleValue, let b = (c["b"] as? NSNumber)?.doubleValue else { return nil }
    let a = ((c["a"] as? NSNumber)?.doubleValue ?? 1) * opacity
    func h(_ x: Double) -> String { String(format: "%02X", Int((min(max(x, 0), 1) * 255).rounded())) }
    return "#" + h(r) + h(g) + h(b) + (a < 0.999 ? h(a) : "")
}

private let frameTypes: Set<String> = ["FRAME", "COMPONENT", "COMPONENT_SET", "INSTANCE"]

// 프레임 하나의 표 — 크기, 안의 글(TEXT), 컴포넌트(INSTANCE 의 이름), 칠한 색(SOLID), 글꼴. 마디는 5,000개까지만 본다
private func frameOf(_ n: [String: Any], components: [String: Any]) -> APFrame {
    var f = APFrame(id: s(n["id"]), name: s(n["name"]))
    let bb = dict(n["absoluteBoundingBox"])
    f.width = (bb["width"] as? NSNumber)?.doubleValue ?? 0
    f.height = (bb["height"] as? NSNumber)?.doubleValue ?? 0
    var seen = 0
    var colors: [String] = [], fonts: [String] = [], comps: [String] = []
    func walk(_ x: [String: Any], top: Bool) {
        seen += 1
        guard seen <= 5000, x["visible"] as? Bool != false else { return }
        let t = s(x["type"])
        if t == "TEXT" {
            let ch = s(x["characters"])
            if !ch.isEmpty { f.texts.append(ch) }
            let st = dict(x["style"])
            let font = [s(st["fontFamily"]), s(st["fontWeight"])].filter { !$0.isEmpty }.joined(separator: " ")
            if !font.isEmpty, !fonts.contains(font) { fonts.append(font) }
        }
        if t == "INSTANCE", !top {
            let cn = s(dict(components[s(x["componentId"])])["name"])
            let label = cn.isEmpty ? s(x["name"]) : cn
            if !label.isEmpty, !comps.contains(label) { comps.append(label) }
        }
        for fill in arr(x["fills"]) where s(fill["type"]) == "SOLID" && fill["visible"] as? Bool != false {
            if let h = hex(dict(fill["color"]), opacity: (fill["opacity"] as? NSNumber)?.doubleValue ?? 1), !colors.contains(h) { colors.append(h) }
        }
        for c in arr(x["children"]) { walk(c, top: false) }
    }
    walk(n, top: true)
    f.colors = colors; f.fonts = fonts; f.components = comps
    return f
}

// Figma 응답(nodes 또는 파일 depth=2) → 메타와 그림을 받을 프레임 id
public func apMetaFromFigma(_ o: [String: Any], file: String, node: String?, link: String, dev: Bool) -> (meta: APMeta, imageNode: String?) {
    var m = APMeta()
    m.source = "figma"
    m.url = link
    m.modified = s(o["lastModified"])
    m.kind = "Figma 프레임"
    var frames: [APFrame] = []
    var page = ""
    if let node {
        m.id = file + "/" + node
        let entry = dict(dict(o["nodes"])[node])
        let doc = dict(entry["document"])
        let comps = dict(entry["components"])
        m.title = [s(o["name"]), s(doc["name"])].filter { !$0.isEmpty }.joined(separator: " › ")
        if frameTypes.contains(s(doc["type"])) { frames = [frameOf(doc, components: comps)] }
        else {
            page = s(doc["name"])
            frames = arr(doc["children"]).filter { frameTypes.contains(s($0["type"])) }.prefix(APMetaLimit.frames).map { frameOf($0, components: comps) }
        }
    } else {
        m.id = file
        m.title = s(o["name"])
        m.kind = "Figma 파일"
        let pages = arr(dict(o["document"])["children"])
        page = pages.map { s($0["name"]) }.joined(separator: ", ")
        for p in pages {
            for c in arr(p["children"]) where frameTypes.contains(s(c["type"])) && frames.count < APMetaLimit.frames {
                frames.append(frameOf(c, components: [:]))
            }
        }
    }
    m.frames = frames
    m.toc = frames.map { APTocItem(1, $0.name) }
    m.length = frames.reduce(0) { $0 + $1.texts.reduce(0) { $0 + $1.count } }
    var fields = [APMetaField("프레임", "\(frames.count)개")]
    if !page.isEmpty { fields.insert(APMetaField(node == nil ? "페이지" : "자리", page), at: 0) }
    if !s(o["version"]).isEmpty { fields.append(APMetaField("판", s(o["version"]))) }
    m.fields = fields
    m.summary = ["프레임 \(frames.count)개 · 글 \(frames.reduce(0) { $0 + $1.texts.count })개" + (frames.isEmpty ? "" : " · " + frames.prefix(5).map(\.name).joined(separator: ", "))]
    return (apMetaClamp(m, dev: dev), frames.first.map(\.id).flatMap { $0.isEmpty ? nil : $0 })
}

// MARK: - 읽기 유스케이스

public struct APReadResult: Equatable, Sendable {
    public var target: APReadTarget = .web
    public var meta: APMeta?
    public var image: Data?
    public var imageHost = ""
    public var imageNote = ""
    public var requests = 0
    public var redacted = 0
    public var note = ""
    public var connection = ""      // 쓴 연결 id(없으면 빈 글자)
    public var ok: Bool { meta != nil }
    public init() {}
}

// 읽기 실패 까닭(사람 말) — 받는 쪽 본문은 믿지 않아 상태 번호로만
public func apReadFailNote(_ r: APHTTPResponse) -> String {
    if let e = r.error { return e }
    switch r.status {
    case 401: return "읽을 수 없음: 거부됨(401). 토큰이 틀렸거나 만료·취소됐습니다"
    case 403: return "읽을 수 없음: 권한 없음(403). 연결한 계정이 그 자리를 볼 수 없습니다"
    case 404: return "읽을 수 없음: 찾지 못했습니다(404). 주소의 번호·키를 확인합니다"
    case 410: return "읽을 수 없음: 지워진 글입니다(410)"
    case 429: return "읽을 수 없음: 요청이 많다며 거절했습니다(429). 잠시 뒤 다시 읽습니다"
    case 300...399: return "읽을 수 없음: 다른 주소로 보내려 해 따라가지 않았습니다(\(r.status))"
    case 500...599: return "읽을 수 없음: 받는 쪽 오류(\(r.status))"
    default: return "읽을 수 없음: 받는 쪽이 \(r.status) 로 답했습니다"
    }
}

private func json(_ r: APHTTPResponse) -> [String: Any]? {
    guard r.error == nil, r.status == 200, r.body.count <= 4 << 20 else { return nil }
    return (try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any]
}

// 링크 하나를 내장 REST 로 읽는다. 연결이 없거나 읽기 범위·토큰이 없거나 만료면 요청 0. 요청은 연결의 바탕 주소 아래로만(apSameSite).
// 메타는 상한 안으로 자르고 비밀을 가린 뒤(redacted) 형식 검사(apMetaCheck)를 지난 것만 돌려준다
public func apReadLink(_ link: String, want: [String] = [], prompt: String = "", connections: [APConnection], dev: Bool,
                       secrets: APSecrets, http: APHTTP, clock: APClock, waiter: APWaiter, tz: TimeZone, budget: Double = 40) -> APReadResult {
    var res = APReadResult()
    let site = connections.first { $0.service == .atlassian }.flatMap { apServiceBase($0, dev: dev) }
    let t = apReadTarget(link, atlassianSite: site)
    res.target = t
    switch t {
    case .bad(let why): res.note = why; return res
    case .web: res.note = "내장하지 않은 주소입니다. MCP 서버를 골라 읽습니다"; return res
    default: break
    }
    let svc = t.service!
    guard let c = connections.first(where: { $0.service == svc }) else { res.note = "\(svc.label) 연결이 없어 요청하지 않았습니다(설정 › 연결)"; return res }
    if let why = apReadBlocker(svc, connection: c, hasSecret: secrets.exists(c.id), dev: dev, now: clock.now, tz: tz) { res.note = why + ". 요청하지 않았습니다"; return res }
    let base = apServiceBase(c, dev: dev)!
    if svc == .atlassian, !apSameSite(link, base: base) { res.note = "연결한 Atlassian 사이트와 다른 사이트의 주소라 요청하지 않았습니다"; return res }
    guard let token = secrets.read(c.id), !token.isEmpty else { res.note = "\(svc.label) 토큰이 키체인에 없어 요청하지 않았습니다"; return res }
    res.connection = c.id
    let auth = apAuthFor(c), header = apAuthHeader(auth, token: token)
    let start = clock.now.timeIntervalSince1970
    func send(_ req: APHTTPRequest, withAuth: Bool = true) -> APHTTPResponse {
        guard req.method == "GET" else { return APHTTPResponse(error: "읽기는 GET 만 보냅니다") }
        if withAuth, !apSameSite(req.url, base: base) { return APHTTPResponse(error: "연결한 사이트 밖 주소라 보내지 않았습니다") }
        var attempt = 0
        while true {
            res.requests += 1
            let r = http.send(req, auth: withAuth ? header : nil)
            guard let w = apRetryWait(status: r.status, retryAfter: r.headers["retry-after"], attempt: attempt, now: clock.now),
                  clock.now.timeIntervalSince1970 + w <= start + budget else { return r }
            waiter.wait(w)
            attempt += 1
        }
    }
    var meta: APMeta
    switch t {
    case .jira(let key):
        let r = send(apJiraIssueRequest(key, base: base, auth: auth))
        guard let o = json(r) else { res.note = r.status == 200 ? "읽을 수 없음: 응답 꼴이 예상과 다릅니다" : apReadFailNote(r); return res }
        meta = apMetaFromJira(o, key: key, base: base, want: want, prompt: prompt, dev: dev)
    case .confluence(let page):
        let r = send(apConfluenceReadRequest(page, base: base, auth: auth))
        guard let o = json(r) else { res.note = r.status == 200 ? "읽을 수 없음: 응답 꼴이 예상과 다릅니다" : apReadFailNote(r); return res }
        meta = apMetaFromConfluence(o, page: page, base: base, want: want, prompt: prompt, dev: dev)
    case .notion(let page):
        let r = send(apNotionPageGetRequest(page, base: base))
        guard let o = json(r) else {
            res.note = r.status == 404 ? "읽을 수 없음: 찾지 못했습니다(404). Notion 은 통합을 그 페이지에 초대해야 읽힙니다(데이터베이스 주소는 아직 읽지 않습니다)" : apReadFailNote(r)
            return res
        }
        var blocks: [[String: Any]] = [], cursor: String? = nil
        for _ in 0..<5 {   // 한 번에 100블록, 다섯 번(500블록)까지. 안쪽 블록(has_children)은 내려가지 않는다
            let rb = send(apNotionChildrenRequest(page, base: base, cursor: cursor))
            guard let ob = json(rb) else { res.note = "본문 블록을 읽지 못했습니다: " + apReadFailNote(rb); return res }
            blocks += arr(ob["results"])
            guard ob["has_more"] as? Bool == true, let next = ob["next_cursor"] as? String, !next.isEmpty else { break }
            cursor = next
        }
        meta = apMetaFromNotion(page: o, blocks: blocks, id: page, link: link, want: want, prompt: prompt, dev: dev)
    case .figma(let file, let node):
        let r = send(apFigmaReadRequest(file, node: node, base: base))
        guard let o = json(r) else { res.note = r.status == 200 ? "읽을 수 없음: 응답 꼴이 예상과 다릅니다" : apReadFailNote(r); return res }
        if let node, dict(o["nodes"])[node] == nil || dict(o["nodes"])[node] is NSNull { res.note = "읽을 수 없음: 파일에 그 프레임(\(node))이 없습니다"; return res }
        let (m, frame) = apMetaFromFigma(o, file: file, node: node, link: link, dev: dev)
        meta = m
        // 프레임 그림 한 장 — /v1/images 가 내보낸 그림 주소(다른 호스트)를 돌려준다. 허용 목록 안일 때만, 인증 머리 없이 받는다
        if let frame {
            let ri = send(apFigmaImageRequest(file, node: frame, base: base))
            let url = s(dict(json(ri)?["images"])[frame])
            if url.isEmpty { res.imageNote = "그림 주소를 받지 못했습니다" + (ri.status == 200 ? "" : "(" + apReadFailNote(ri) + ")") }
            else if !apImageURLOK(url, dev: dev) { res.imageNote = "그림 주소가 허용한 호스트 밖이라 받지 않았습니다(\(apHost(url)))" }
            else {
                let rg = send(APHTTPRequest(url: url, headers: ["Accept": "image/png"], auth: .none), withAuth: false)
                if rg.error == nil, rg.status == 200, rg.body.count <= apImageMaxBytes, apIsPNG(rg.body) { res.image = rg.body; res.imageHost = apHost(url) }
                else { res.imageNote = rg.status == 200 ? "그림이 PNG 가 아니거나 너무 큽니다" : "그림을 받지 못했습니다: " + apReadFailNote(rg) }
            }
        }
    default: return res
    }
    let (red, n) = apMetaRedact(meta)
    let checked = apMetaCheck(apMetaJSON(red))
    guard let ok = checked.meta else { res.note = "형식 검사에 걸렸습니다: " + checked.problems.prefix(3).joined(separator: " · "); return res }
    res.meta = ok
    res.redacted = n
    res.note = "읽었습니다"
    return res
}

// MARK: - 에픽 제목·상태(출발 준비의 에픽 칸, 5-2c 의 읽기 몫)

public struct APEpicInfo: Equatable, Sendable {
    public var key = ""
    public var found = false
    public var isEpic = false
    public var title = ""
    public var status = ""
    public var type = ""
    public var note = ""
    public var requests = 0
    public var read = false       // 요청해서 답을 받았는가(연결이 없어 요청하지 않았으면 false)
    public init() {}
}

// 에픽 판정 — 이슈 종류의 계층 수준이 1(에픽 자리)이면 에픽, 그 값이 없으면 종류 이름이 Epic·에픽
public func apIsEpicType(_ t: [String: Any]) -> Bool {
    if let lv = (t["hierarchyLevel"] as? NSNumber)?.intValue { return lv == 1 }
    let n = s(t["name"]).lowercased()
    return n == "epic" || n == "에픽"
}

public func apReadEpic(_ key: String, connections: [APConnection], dev: Bool, secrets: APSecrets, http: APHTTP, clock: APClock, tz: TimeZone) -> APEpicInfo {
    var e = APEpicInfo()
    e.key = key
    guard key.range(of: apIssueKeyPattern, options: .regularExpression) != nil else { e.note = "에픽 번호 꼴이 아닙니다(예: SHOP-10)"; return e }
    let c = connections.first { $0.service == .atlassian }
    if let why = apReadBlocker(.atlassian, connection: c, hasSecret: c.map { secrets.exists($0.id) } ?? false, dev: dev, now: clock.now, tz: tz) {
        e.note = why + ". 번호 꼴만 봤습니다"; return e
    }
    let base = apServiceBase(c!, dev: dev)!
    guard let token = secrets.read(c!.id), !token.isEmpty else { e.note = "Atlassian 토큰이 키체인에 없습니다. 번호 꼴만 봤습니다"; return e }
    let req = apJiraIssueRequest(key, base: base, auth: apAuthFor(c!), fields: "summary,status,issuetype")
    guard apSameSite(req.url, base: base) else { e.note = "사이트 주소가 규칙에 맞지 않습니다"; return e }
    e.requests = 1
    let r = http.send(req, auth: apAuthHeader(apAuthFor(c!), token: token))
    if r.status == 404 { e.read = true; e.note = "\(key) 이슈가 없습니다(404). 에픽 번호를 확인합니다"; return e }
    guard let o = json(r) else { e.note = apReadFailNote(r); return e }
    let f = dict(o["fields"])
    e.read = true
    e.found = true
    e.title = apMetaText(s(f["summary"]), 200)
    e.status = apMetaText(name(f["status"]), 80)
    e.type = apMetaText(name(f["issuetype"]), 80)
    e.isEpic = apIsEpicType(dict(f["issuetype"]))
    e.note = e.isEpic ? "에픽입니다" : "\(key) 은 \(e.type.isEmpty ? "에픽이 아닌 이슈" : e.type)입니다. 에픽 번호를 넣어 주세요"
    return e
}
