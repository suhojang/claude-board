// 자율주행 규칙(AutopilotRules) — 바깥에 올릴 글의 형식 변환(4단계 ⑩-1, 2026-10-30, 설계 5장 6절 "정해진 틀 몇 개의 형식 변환").
// 올릴 글은 받는 곳과 상관없는 작은 문서 꼴 APDoc(제목·문단·목록·코드·가로줄, 글 조각은 굵게·https 링크)로 만들고,
// 올릴 때 Jira 문서 형식(ADF), Confluence 저장 형식(XHTML), Notion 블록(글 한 칸 2,000자, 한 번에 100블록)으로 바꾼다.
// 글은 모델이 쓴 결과 기록에서 오므로 믿지 않는다: 만들 때 다듬고(apDocText: 긴 줄표 바꿈·화면을 속이는 글자 지움·길이 상한),
// 저장 형식은 모든 글을 이스케이프하고 매크로를 쓰지 않으며, 링크는 https 만(apSafeLink) 건다. 보드 검수 화면도 같은 APDoc 을 그린다
import Foundation

public enum APSpan: Equatable, Sendable {
    case text(String, bold: Bool)
    case link(String, url: String)    // url 은 apSafeLink 를 지난 https 주소
}

public enum APBlock: Equatable, Sendable {
    case heading(Int, String)        // 수준 1~3
    case paragraph([APSpan])
    case bullets([[APSpan]])         // 줄마다 조각들
    case code(String)
    case rule
}

public struct APDoc: Equatable, Sendable {
    public var blocks: [APBlock]
    public init(_ blocks: [APBlock] = []) { self.blocks = blocks }
}

public let apDocMarker = "오토파일럿 작성"     // 바깥 글 첫 줄(사용자 확정)
public let apDocTextMax = 4000               // 글 조각 하나의 글자 수 상한
public let apDocBlockMax = 400               // 문서 하나의 블록 수 상한
public let apDocListMax = 200                // 목록 하나의 줄 수 상한
public let apNotionTextMax = 2000            // Notion 글 한 칸(rich_text 하나)의 글자 수 상한
public let apNotionBatchMax = 100            // Notion 에 한 번에 보내는 블록 수 상한

// 글 다듬기 — 긴 줄표는 쉼표로(apPlain), 줄바꿈·탭은 빈칸으로, 화면을 속이는 글자(제어·방향 바꾸기·줄 구분자)는 지우고, 상한에서 자른다
public func apDocText(_ s: String, _ n: Int = apDocTextMax, trim: Bool = true) -> String {
    var o = ""
    for u in apPlain(s).unicodeScalars {
        let v = u.value
        if u == "\n" || u == "\t" || u == "\r" || v == 0x2028 || v == 0x2029 { o += " "; continue }
        if v < 0x20 || (0x7F...0x9F).contains(v) || (0x202A...0x202E).contains(v) || (0x2066...0x2069).contains(v) || v == 0xFEFF { continue }
        o.unicodeScalars.append(u)
    }
    let c = o.replacingOccurrences(of: " {2,}", with: " ", options: .regularExpression)
    let t = trim ? c.trimmingCharacters(in: .whitespaces) : c   // 글 조각(span)은 앞뒤 빈칸을 남긴다(조각 사이 띄어쓰기)
    return t.count > n ? String(t.prefix(n)) + "…" : t
}

// 코드 칸 — 줄바꿈은 남기고 나머지는 apDocText 와 같게
public func apDocCode(_ s: String, _ n: Int = apDocTextMax) -> String {
    s.components(separatedBy: "\n").map { apDocText($0, n) }.joined(separator: "\n").prefix(n).description
}

// 문서 다듬기 — 블록 수·목록 줄 수 상한, 빈 조각 빼기, 링크 다시 확인(만드는 쪽이 이미 했어도 한 번 더)
public func apDocClamp(_ d: APDoc) -> APDoc {
    func spans(_ a: [APSpan]) -> [APSpan] {
        a.compactMap { s in
            switch s {
            case .text(let t, let b): let x = apDocText(t, trim: false); return x.trimmingCharacters(in: .whitespaces).isEmpty ? nil : .text(x, bold: b)
            case .link(let t, let u):
                let x = apDocText(t, 300, trim: false)
                guard let ok = apSafeLink(u) else { return x.isEmpty ? nil : .text(x, bold: false) }
                return .link(x.isEmpty ? ok : x, url: ok)
            }
        }
    }
    var out: [APBlock] = []
    for b in d.blocks.prefix(apDocBlockMax) {
        switch b {
        case .heading(let l, let t): let x = apDocText(t, 300); if !x.isEmpty { out.append(.heading(min(max(l, 1), 3), x)) }
        case .paragraph(let a): let s = spans(a); if !s.isEmpty { out.append(.paragraph(s)) }
        case .bullets(let rows): let r = rows.prefix(apDocListMax).map(spans).filter { !$0.isEmpty }; if !r.isEmpty { out.append(.bullets(Array(r))) }
        case .code(let t): let x = apDocCode(t); if !x.isEmpty { out.append(.code(x)) }
        case .rule: out.append(.rule)
        }
    }
    return APDoc(out)
}

// MARK: - Jira 문서 형식(ADF)

private func adfSpans(_ a: [APSpan]) -> [[String: Any]] {
    a.compactMap { s -> [String: Any]? in
        switch s {
        case .text(let t, let b):
            guard !t.isEmpty else { return nil }   // ADF 는 빈 글 마디를 받지 않는다
            var n: [String: Any] = ["type": "text", "text": t]
            if b { n["marks"] = [["type": "strong"]] }
            return n
        case .link(let t, let u):
            guard !t.isEmpty else { return nil }
            return ["type": "text", "text": t, "marks": [["type": "link", "attrs": ["href": u]]]]
        }
    }
}

public func apADF(_ d0: APDoc) -> [String: Any] {
    let d = apDocClamp(d0)
    var content: [[String: Any]] = []
    for b in d.blocks {
        switch b {
        case .heading(let l, let t): content.append(["type": "heading", "attrs": ["level": l], "content": [["type": "text", "text": t]]])
        case .paragraph(let a): content.append(["type": "paragraph", "content": adfSpans(a)])
        case .bullets(let rows):
            content.append(["type": "bulletList", "content": rows.map { ["type": "listItem", "content": [["type": "paragraph", "content": adfSpans($0)]]] }])
        case .code(let t): content.append(["type": "codeBlock", "content": [["type": "text", "text": t]]])
        case .rule: content.append(["type": "rule"])
        }
    }
    return ["type": "doc", "version": 1, "content": content]
}

// MARK: - Confluence 저장 형식(XHTML)

private func storageSpans(_ a: [APSpan]) -> String {
    a.map { s in
        switch s {
        case .text(let t, let b): return b ? "<strong>\(apEscapeHTML(t))</strong>" : apEscapeHTML(t)
        case .link(let t, let u): return "<a href=\"\(apEscapeHTML(u))\">\(apEscapeHTML(t))</a>"
        }
    }.joined()
}

// 매크로(ac:…)·CDATA 를 쓰지 않는다 — 글에 든 ]]> 나 매크로 꼴이 그대로 글로 남게. 코드는 <pre>
public func apStorage(_ d0: APDoc) -> String {
    let d = apDocClamp(d0)
    var o = ""
    for b in d.blocks {
        switch b {
        case .heading(let l, let t): o += "<h\(l)>\(apEscapeHTML(t))</h\(l)>"
        case .paragraph(let a): o += "<p>\(storageSpans(a))</p>"
        case .bullets(let rows): o += "<ul>" + rows.map { "<li>\(storageSpans($0))</li>" }.joined() + "</ul>"
        case .code(let t): o += "<pre>\(apEscapeHTML(t))</pre>"
        case .rule: o += "<hr />"
        }
    }
    return o
}

// MARK: - Notion 블록

// 글 하나를 rich_text 칸들로 — 칸마다 2,000자 이하(Notion 상한). 유니코드 글자(확장 자소) 경계에서 자른다
public func apNotionRich(_ s: String, bold: Bool = false, link: String? = nil) -> [[String: Any]] {
    var out: [[String: Any]] = []
    var rest = Substring(s)
    while !rest.isEmpty {
        let piece = rest.prefix(apNotionTextMax)
        rest = rest.dropFirst(piece.count)
        var text: [String: Any] = ["content": String(piece)]
        if let link { text["link"] = ["url": link] }
        var item: [String: Any] = ["type": "text", "text": text]
        if bold { item["annotations"] = ["bold": true] }
        out.append(item)
    }
    return out
}

private func notionSpans(_ a: [APSpan]) -> [[String: Any]] {
    Array(a.flatMap { s -> [[String: Any]] in
        switch s {
        case .text(let t, let b): return apNotionRich(t, bold: b)
        case .link(let t, let u): return apNotionRich(t, link: u)
        }
    }.prefix(100))   // 블록 하나의 rich_text 는 100칸까지(Notion 상한)
}

private func notionBlock(_ type: String, _ rich: [[String: Any]], extra: [String: Any] = [:]) -> [String: Any] {
    var inner: [String: Any] = ["rich_text": rich]
    for (k, v) in extra { inner[k] = v }
    return ["object": "block", "type": type, type: inner]
}

public func apNotionBlocks(_ d0: APDoc) -> [[String: Any]] {
    let d = apDocClamp(d0)
    var out: [[String: Any]] = []
    for b in d.blocks {
        switch b {
        case .heading(let l, let t): out.append(notionBlock("heading_\(l)", apNotionRich(t)))
        case .paragraph(let a): out.append(notionBlock("paragraph", notionSpans(a)))
        case .bullets(let rows): out += rows.map { notionBlock("bulleted_list_item", notionSpans($0)) }
        case .code(let t): out.append(notionBlock("code", apNotionRich(t), extra: ["language": "plain text"]))
        case .rule: out.append(["object": "block", "type": "divider", "divider": [String: Any]()])
        }
    }
    return out
}

// 한 번에 보낼 묶음 — n 개씩(Notion: 처음 묶음은 페이지 만들기와 함께, 나머지는 블록 덧붙이기로)
public func apBatches<T>(_ a: [T], _ n: Int = apNotionBatchMax) -> [[T]] {
    guard n > 0 else { return [a] }
    return stride(from: 0, to: a.count, by: n).map { Array(a[$0..<min($0 + n, a.count)]) }
}

// MARK: - 문서 ↔ 파일 칸(outbox 글 파일의 doc)

public func apDocJSON(_ d: APDoc) -> [[String: Any]] {
    func sp(_ a: [APSpan]) -> [[String: Any]] {
        a.map { s in
            switch s {
            case .text(let t, let b): return b ? ["x": t, "b": true] : ["x": t]
            case .link(let t, let u): return ["x": t, "l": u]
            }
        }
    }
    return d.blocks.map { b in
        switch b {
        case .heading(let l, let t): return ["t": "h", "n": l, "x": t]
        case .paragraph(let a): return ["t": "p", "s": sp(a)]
        case .bullets(let r): return ["t": "ul", "i": r.map(sp)]
        case .code(let t): return ["t": "code", "x": t]
        case .rule: return ["t": "hr"]
        }
    }
}

// 파일 칸 → 문서(모르는 블록은 버리고 다듬기를 다시 한다)
public func apDocParse(_ v: Any?) -> APDoc {
    func sp(_ v: Any?) -> [APSpan] {
        (v as? [[String: Any]] ?? []).compactMap { e in
            let x = e["x"] as? String ?? ""
            if let u = e["l"] as? String { return .link(x, url: u) }
            return .text(x, bold: e["b"] as? Bool ?? false)
        }
    }
    let blocks: [APBlock] = (v as? [[String: Any]] ?? []).compactMap { e in
        switch e["t"] as? String {
        case "h": return .heading((e["n"] as? NSNumber)?.intValue ?? 2, e["x"] as? String ?? "")
        case "p": return .paragraph(sp(e["s"]))
        case "ul": return .bullets((e["i"] as? [Any] ?? []).map(sp))
        case "code": return .code(e["x"] as? String ?? "")
        case "hr": return .rule
        default: return nil
        }
    }
    return apDocClamp(APDoc(blocks))
}

// 문서를 쉬운 글로(검수 화면의 한 줄 요약, 시험) — 블록마다 한 줄
public func apDocPlainLines(_ d: APDoc) -> [String] {
    func sp(_ a: [APSpan]) -> String { a.map { if case .text(let t, _) = $0 { return t } else if case .link(let t, _) = $0 { return t } else { return "" } }.joined() }
    return d.blocks.flatMap { b -> [String] in
        switch b {
        case .heading(_, let t): return [t]
        case .paragraph(let a): return [sp(a)]
        case .bullets(let r): return r.map { "· " + sp($0) }
        case .code(let t): return [t]
        case .rule: return ["(가로줄)"]
        }
    }
}
