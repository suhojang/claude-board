// 자율주행 규칙(AutopilotRules) — 주제로 시작의 자료 링크 줄(설계 2장 "주제로 시작": 피그마·Confluence·Notion·Jira·그 밖 웹 주소, 줄마다 종류·주소·읽기 상태
// 연결됨·연결 필요·읽을 수 없음). 2026-10-30 2단계 5-2b 에서 새로 씀. 링크 읽기는 바깥 연결(4단계)과 함께라 지금은 주소 꼴과 종류만 보고
// 읽지 않는다(요청 0) — 꼴이 맞는 주소는 모두 "연결 필요"
import Foundation

public struct APLink: Equatable, Sendable, Identifiable {
    public var id: String { url }
    public let url: String
    public let kind: String       // Figma·Confluence·Notion·Jira·웹
    public let state: String      // needs_connection·unreadable
    public let why: String
    public init(url: String, kind: String, state: String, why: String) { self.url = url; self.kind = kind; self.state = state; self.why = why }
}

// 붙여 넣은 주소 하나 — 앞뒤 빈칸을 자르고, http(s) 와 호스트가 있는 주소만 받는다. 종류는 호스트와 경로로.
// 사용자·비밀번호가 든 주소(https://u:p@…)는 받지 않는다(근거 묶음과 화면에 비밀이 남지 않게)
public func apLinkInfo(_ raw: String) -> APLink {
    let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let u = URLComponents(string: s), let scheme = u.scheme?.lowercased(), ["http", "https"].contains(scheme),
          let host = u.host?.lowercased(), !host.isEmpty, host.contains(".") else {
        return APLink(url: s, kind: "웹", state: "unreadable", why: "웹 주소 꼴이 아닙니다(https:// 로 시작하는 주소를 붙여 넣습니다)")
    }
    if u.user != nil || u.password != nil {
        return APLink(url: s.replacingOccurrences(of: (u.password.map { ":" + $0 } ?? "") + "@", with: "@"), kind: "웹", state: "unreadable",
                      why: "사용자 이름·비밀번호가 든 주소는 받지 않습니다")
    }
    let path = u.path.lowercased()
    let kind: String
    if host == "figma.com" || host.hasSuffix(".figma.com") { kind = "Figma" }
    else if host.hasSuffix(".atlassian.net") { kind = path.hasPrefix("/wiki") ? "Confluence" : (path.hasPrefix("/browse") || path.hasPrefix("/jira") ? "Jira" : "웹") }
    else if host == "notion.so" || host.hasSuffix(".notion.so") || host.hasSuffix(".notion.site") { kind = "Notion" }
    else { kind = "웹" }
    let why = kind == "웹" ? "그 밖 웹 주소는 사용자가 등록한 MCP 읽기 세션으로 읽습니다(바깥 연결, 4단계)"
                           : "\(kind) 연결(설정 › 연결, 4단계) 뒤에 메타 정보만 읽습니다"
    return APLink(url: s, kind: kind, state: "needs_connection", why: why)
}

// 줄 목록에 더해도 되는가 — 같은 주소는 한 번만, 20줄까지. 되면 nil
public func apLinkAddProblem(_ link: APLink, to list: [APLink]) -> String? {
    if link.url.isEmpty { return "주소를 붙여 넣어 주세요" }
    if list.contains(where: { $0.url == link.url }) { return "이미 있는 주소입니다" }
    if list.count >= 20 { return "자료 링크는 20줄까지입니다" }
    return nil
}
