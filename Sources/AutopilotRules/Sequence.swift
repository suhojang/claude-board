// 자율주행 규칙(AutopilotRules) — 결과 기록의 흐름 그림(result.json 의 sequence[].mermaid)을 인라인 SVG 로 그린다(2026-10-30 3단계).
// 보고서는 바깥 스크립트를 쓰지 않으므로(인터넷 없이 보이고, 보드 뷰어는 JavaScript 를 끈다) Mermaid 를 쓰지 않고 sequenceDiagram 의 작은 부분만 읽는다:
// participant·actor(as 이름), 메시지 화살표(-> --> ->> -->> -x --x -) --) <<->> <<-->>, 끝의 +·- 켜기 표지는 무시), Note left of·right of·over,
// 묶음 loop·alt/else·opt·par/and·critical/option·break·rect·box(그리지 않음) … end, autonumber, title. activate·deactivate·destroy·%% 주석은 넘긴다.
// 그 밖의 줄이 하나라도 있으면 그리지 않고(unsupported) 보고서가 원문을 그대로 보인다 — 말없이 빠지는 줄이 없게.
// 글자 폭은 글꼴을 모르므로 어림한다(한글·한자 1em, 그 밖 0.6em). 색은 CSS 변수(보고서 틀의 .sq 규칙)로 밝은·어두운 화면을 따른다.
// 모든 글은 apEscapeHTML 을 거친다. id·외부 참조(marker·use·href)는 쓰지 않는다(한 문서에 그림이 여럿이어도 겹치지 않게)
import Foundation

public enum APSeqHead: String, Equatable, Sendable { case none, arrow, cross, open }
public enum APSeqSide: String, Equatable, Sendable { case left, right, over }

public struct APSeqParticipant: Equatable, Sendable {
    public var id: String
    public var label: String
    public var actor: Bool
    public init(id: String, label: String, actor: Bool) { self.id = id; self.label = label; self.actor = actor }
}

public enum APSeqItem: Equatable, Sendable {
    case message(from: Int, to: Int, text: String, dotted: Bool, head: APSeqHead, both: Bool)
    case note(at: [Int], side: APSeqSide, text: String)
    case begin(kind: String, label: String)     // loop·alt·opt·par·critical·break·rect·box
    case divide(kind: String, label: String)    // else·and·option
    case end
}

public struct APSeqDiagram: Equatable, Sendable {
    public var title = ""
    public var participants: [APSeqParticipant] = []
    public var items: [APSeqItem] = []
    public var autonumber = false
    public init() {}
}

public enum APSeqParse: Equatable, Sendable {
    case ok(APSeqDiagram)
    case unsupported(line: Int, why: String)   // 그리지 못한 첫 줄(1부터)과 까닭
}

public let apSeqMaxParticipants = 24
public let apSeqMaxItems = 300
public let apSeqMaxLabel = 300

private let arrows: [(String, Bool, APSeqHead, Bool)] = [   // 글자, 점선, 머리, 양쪽 — 긴 것부터
    ("<<-->>", true, .arrow, true), ("<<->>", false, .arrow, true),
    ("-->>", true, .arrow, false), ("--x", true, .cross, false), ("--)", true, .open, false),
    ("->>", false, .arrow, false), ("-->", true, .none, false), ("-x", false, .cross, false), ("-)", false, .open, false),
    ("->", false, .none, false),
]
private let beginKinds: Set<String> = ["loop", "alt", "opt", "par", "critical", "break", "rect", "box"]
private let divideOf: [String: String] = ["else": "alt", "and": "par", "option": "critical"]

private func firstWord(_ s: String) -> (String, String) {
    guard let i = s.firstIndex(where: { $0 == " " || $0 == "\t" }) else { return (s, "") }
    return (String(s[..<i]), String(s[i...]).trimmingCharacters(in: .whitespaces))
}

// sequenceDiagram 원문 → 그림 값. 첫 의미 있는 줄은 sequenceDiagram 이어야 한다
public func apParseSequence(_ src: String) -> APSeqParse {
    var d = APSeqDiagram()
    var index: [String: Int] = [:]
    var stack: [String] = []
    var started = false
    func participant(_ id: String, _ label: String? = nil, actor: Bool = false) -> Int? {
        if let i = index[id] {
            if let label { d.participants[i].label = label }
            if actor { d.participants[i].actor = true }
            return i
        }
        guard d.participants.count < apSeqMaxParticipants else { return nil }
        d.participants.append(APSeqParticipant(id: id, label: label ?? id, actor: actor))
        index[id] = d.participants.count - 1
        return d.participants.count - 1
    }
    func validID(_ s: String) -> Bool {
        !s.isEmpty && s.count <= 80 && !s.contains(where: { " \t:,;<>\"".contains($0) })
    }
    let lines = src.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n").components(separatedBy: "\n")
    for (k, raw) in lines.enumerated() {
        let no = k + 1
        var line = raw.trimmingCharacters(in: .whitespaces)
        if line.hasSuffix(";") { line = String(line.dropLast()).trimmingCharacters(in: .whitespaces) }
        if line.isEmpty || line.hasPrefix("%%") { continue }
        if !started {
            guard line == "sequenceDiagram" else { return .unsupported(line: no, why: "sequenceDiagram 으로 시작하지 않습니다") }
            started = true
            continue
        }
        guard d.items.count < apSeqMaxItems else { return .unsupported(line: no, why: "줄이 너무 많습니다(\(apSeqMaxItems)줄까지 그립니다)") }
        let (w0, rest) = firstWord(line)
        let w = w0.lowercased()
        switch w {
        case "participant", "actor":
            var id = rest, label: String?
            if let r = rest.range(of: " as ") {
                id = String(rest[..<r.lowerBound]).trimmingCharacters(in: .whitespaces)
                label = String(rest[r.upperBound...]).trimmingCharacters(in: .whitespaces)
            }
            guard validID(id), (label ?? "").count <= apSeqMaxLabel else { return .unsupported(line: no, why: "참여자 이름을 읽지 못했습니다") }
            guard participant(id, label, actor: w == "actor") != nil else { return .unsupported(line: no, why: "참여자가 너무 많습니다(\(apSeqMaxParticipants)까지)") }
            continue
        case "create":
            let (w1, r1) = firstWord(rest)
            guard ["participant", "actor"].contains(w1.lowercased()) else { return .unsupported(line: no, why: "읽지 못한 줄입니다") }
            var id = r1, label: String?
            if let r = r1.range(of: " as ") { id = String(r1[..<r.lowerBound]).trimmingCharacters(in: .whitespaces); label = String(r1[r.upperBound...]).trimmingCharacters(in: .whitespaces) }
            guard validID(id), participant(id, label, actor: w1.lowercased() == "actor") != nil else { return .unsupported(line: no, why: "참여자 이름을 읽지 못했습니다") }
            continue
        case "autonumber": d.autonumber = true; continue
        case "activate", "deactivate", "destroy": continue
        case "title", "title:":
            d.title = String((rest.hasPrefix(":") ? String(rest.dropFirst()) : rest).trimmingCharacters(in: .whitespaces).prefix(apSeqMaxLabel))
            continue
        case "acctitle:", "accdescr:": continue
        case "end":
            guard !rest.isEmpty || !stack.isEmpty else { return .unsupported(line: no, why: "여는 묶음 없이 end 가 있습니다") }
            guard rest.isEmpty else { break }
            stack.removeLast()
            d.items.append(.end)
            continue
        case "note":
            let (side0, r1) = firstWord(rest)
            let side = side0.lowercased()
            var targets = r1, s: APSeqSide
            if side == "over" { s = .over } else if side == "left" || side == "right" {
                let (of, r2) = firstWord(r1)
                guard of.lowercased() == "of" else { return .unsupported(line: no, why: "Note 줄을 읽지 못했습니다") }
                s = side == "left" ? .left : .right
                targets = r2
            } else { return .unsupported(line: no, why: "Note 줄을 읽지 못했습니다") }
            guard let colon = targets.firstIndex(of: ":") else { return .unsupported(line: no, why: "Note 줄에 쌍점(:)이 없습니다") }
            let names = targets[..<colon].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
            let text = String(targets[targets.index(after: colon)...]).trimmingCharacters(in: .whitespaces)
            guard (1...2).contains(names.count), names.allSatisfy(validID), text.count <= apSeqMaxLabel, s == .over || names.count == 1 else {
                return .unsupported(line: no, why: "Note 줄을 읽지 못했습니다")
            }
            var at: [Int] = []
            for n in names { guard let i = participant(n) else { return .unsupported(line: no, why: "참여자가 너무 많습니다(\(apSeqMaxParticipants)까지)") }; at.append(i) }
            d.items.append(.note(at: at, side: s, text: text))
            continue
        default:
            if beginKinds.contains(w) {
                guard rest.count <= apSeqMaxLabel else { return .unsupported(line: no, why: "묶음 이름이 너무 깁니다") }
                stack.append(w)
                d.items.append(.begin(kind: w, label: w == "rect" || w == "box" ? "" : rest))   // rect·box 의 색 글자는 그리지 않는다
                continue
            }
            if let parent = divideOf[w] {
                guard stack.last == parent else { return .unsupported(line: no, why: "\(w0) 가 맞는 묶음 안에 있지 않습니다") }
                guard rest.count <= apSeqMaxLabel else { return .unsupported(line: no, why: "묶음 이름이 너무 깁니다") }
                d.items.append(.divide(kind: w, label: rest))
                continue
            }
        }
        // 메시지: 보내는 쪽 화살표 받는 쪽: 글
        var hit: (range: Range<String.Index>, dotted: Bool, head: APSeqHead, both: Bool)?
        var i = line.startIndex
        while i < line.endIndex, hit == nil {
            for (tok, dotted, head, both) in arrows where line[i...].hasPrefix(tok) {
                hit = (i..<line.index(i, offsetBy: tok.count), dotted, head, both)
                break
            }
            if line[i] == ":" { break }
            i = line.index(after: i)
        }
        guard let h = hit else { return .unsupported(line: no, why: "읽지 못한 줄입니다") }
        let from = line[..<h.range.lowerBound].trimmingCharacters(in: .whitespaces)
        var after = line[h.range.upperBound...].trimmingCharacters(in: .whitespaces)
        if after.hasPrefix("+") || after.hasPrefix("-") { after = String(after.dropFirst()).trimmingCharacters(in: .whitespaces) }
        var to = after, text = ""
        if let c = after.firstIndex(of: ":") {
            to = after[..<c].trimmingCharacters(in: .whitespaces)
            text = after[after.index(after: c)...].trimmingCharacters(in: .whitespaces)
        }
        guard validID(from), validID(to), text.count <= apSeqMaxLabel else { return .unsupported(line: no, why: "메시지 줄을 읽지 못했습니다") }
        guard let a = participant(from), let b = participant(to) else { return .unsupported(line: no, why: "참여자가 너무 많습니다(\(apSeqMaxParticipants)까지)") }
        d.items.append(.message(from: a, to: b, text: text, dotted: h.dotted, head: h.head, both: h.both))
    }
    guard started else { return .unsupported(line: 1, why: "비어 있습니다") }
    guard !d.participants.isEmpty else { return .unsupported(line: 1, why: "참여자가 없습니다") }
    while !stack.isEmpty { stack.removeLast(); d.items.append(.end) }   // 닫지 않은 묶음은 끝에서 닫는다
    return .ok(d)
}

// 글자 폭 어림(px) — 한글·한자·전각·그림 글자는 1em, 그 밖은 0.6em
public func apTextWidth(_ s: String, size: Double) -> Double {
    var w = 0.0
    for u in s.unicodeScalars {
        let v = u.value
        let wide = (0x1100...0x11FF).contains(v) || (0x2E80...0xA4CF).contains(v) || (0xAC00...0xD7A3).contains(v)
            || (0xF900...0xFAFF).contains(v) || (0xFE30...0xFE4F).contains(v) || (0xFF00...0xFF60).contains(v) || (0xFFE0...0xFFE6).contains(v)
            || (0x1F300...0x1FAFF).contains(v)
        w += size * (wide ? 1.0 : 0.6)
    }
    return w
}

// 줄바꿈 표지 <br>·<br/>·<br /> 로 나눈 줄
func apSeqLines(_ s: String) -> [String] {
    let parts = s.replacingOccurrences(of: "<br\\s*/?>", with: "\n", options: [.regularExpression, .caseInsensitive]).components(separatedBy: "\n")
    let l = parts.map { $0.trimmingCharacters(in: .whitespaces) }
    return l.isEmpty ? [""] : l
}

private func n(_ x: Double) -> String { String(Int(x.rounded())) }

// 그림 값 → SVG 글자. 글은 apPlain(긴 줄표) → apEscapeHTML 을 거친다. label 은 화면 읽기 프로그램용 이름
public func apSequenceSVG(_ d: APSeqDiagram, label: String) -> String {
    let fs = 13.0, ms = 12.5, lh = 16.0
    let P = d.participants
    let count = P.count
    guard count > 0 else { return "" }
    func t(_ s: String) -> String { apEscapeHTML(apPlain(s)) }
    func width(_ s: String, _ size: Double) -> Double { apSeqLines(s).map { apTextWidth($0, size: size) }.max() ?? 0 }
    let plines = P.map { apSeqLines($0.label) }
    let boxW: [Double] = P.map { p -> Double in max(84.0, width(p.label, fs) + 24.0) }
    let tallest: Int = plines.map { $0.count }.max() ?? 1
    let boxH: Double = Double(tallest) * lh + 18
    // 가로: 이웃 사이 간격 gap[i] = x[i+1]-x[i]. 메시지·Note 가 들어갈 만큼 넓힌다
    var gap: [Double] = []
    for k in 0..<max(count - 1, 0) {
        let base: Double = boxW[k] / 2 + boxW[k + 1] / 2 + 28
        gap.append(max(base, 120))
    }
    var leftExtra = 0.0, rightExtra = 0.0
    func need(_ lo: Int, _ hi: Int, _ w: Double) {
        guard hi > lo else { return }
        let have = gap[lo..<hi].reduce(0, +)
        if have < w { let add = (w - have) / Double(hi - lo); for k in lo..<hi { gap[k] += add } }
    }
    for it in d.items {
        switch it {
        case .message(let a, let b, let text, _, _, _):
            let tw = width(text, ms) + (d.autonumber ? 22 : 0)
            if a == b {
                if a < count - 1 { need(a, a + 1, tw + 56) } else { rightExtra = max(rightExtra, tw + 56 - boxW[a] / 2) }
            } else { need(min(a, b), max(a, b), tw + 32) }
        case .note(let at, let side, let text):
            let w = max(width(text, ms) + 20, 70)
            switch side {
            case .over where at.count == 2: need(min(at[0], at[1]), max(at[0], at[1]), w - 40)
            case .over: break
            case .right: if at[0] < count - 1 { need(at[0], at[0] + 1, w + 18) } else { rightExtra = max(rightExtra, w + 12 - boxW[at[0]] / 2) }
            case .left: if at[0] > 0 { need(at[0] - 1, at[0], w + 18) } else { leftExtra = max(leftExtra, w + 12 - boxW[0] / 2) }
            }
        default: break
        }
    }
    var x = [Double](repeating: 0, count: count)
    x[0] = max(boxW[0] / 2, 0) + 12 + max(leftExtra, 0)
    for k in 1..<max(count, 1) { x[k] = x[k - 1] + gap[k - 1] }

    // 세로로 내려가며 그린다. 묶음 틀은 닫힐 때 그 안에서 쓴 참여자 폭으로
    var body = "", frames = ""
    var y = 10 + boxH + 18
    var num = 0
    struct Open { var kind: String; var label: String; var top: Double; var used: Set<Int>; var divides: [(Double, String)]; var depth: Int }
    var open: [Open] = []
    func use(_ ps: [Int]) { for k in open.indices { open[k].used.formUnion(ps) } }
    func textLines(_ lines: [String], x: Double, top: Double, cls: String, anchor: String) -> String {
        var o = ""
        for (k, l) in lines.enumerated() {
            o += "<text class=\"\(cls)\" x=\"\(n(x))\" y=\"\(n(top + Double(k) * lh + 12))\" text-anchor=\"\(anchor)\">\(t(l))</text>"
        }
        return o
    }
    var minX = 0.0, maxX = (x.last ?? 0) + (boxW.last ?? 0) / 2 + 12 + max(rightExtra, 0)
    for it in d.items {
        switch it {
        case .message(let a, let b, let text, let dotted, let head, let both):
            use([a, b])
            num += 1
            let lines = text.isEmpty ? [] : apSeqLines(text)
            let cls = dotted ? "ml d" : "ml"
            let tag = d.autonumber ? "\(num). " : ""
            if a == b {
                let top = y, ly = y + Double(lines.count) * lh + 6
                if !lines.isEmpty || d.autonumber {
                    var ls = lines.isEmpty ? [""] : lines
                    ls[0] = tag + ls[0]
                    body += textLines(ls, x: x[a] + 10, top: top, cls: "mt", anchor: "start")
                    maxX = max(maxX, x[a] + 10 + width(text, ms) + 30)
                }
                let r = x[a] + 40
                body += "<polyline class=\"\(cls)\" points=\"\(n(x[a])),\(n(ly)) \(n(r)),\(n(ly)) \(n(r)),\(n(ly + 20)) \(n(x[a] + 2)),\(n(ly + 20))\"/>"
                body += arrowHead(head, tipX: x[a] + 2, y: ly + 20, dir: -1)
                maxX = max(maxX, r + 8)
                y = ly + 20 + 16
            } else {
                let top = y, ly = y + Double(lines.count) * lh + 6
                let mid = (x[a] + x[b]) / 2
                if !lines.isEmpty || d.autonumber {
                    var ls = lines.isEmpty ? [""] : lines
                    ls[0] = tag + ls[0]
                    body += textLines(ls, x: mid, top: top, cls: "mt", anchor: "middle")
                }
                let dir: Double = x[b] > x[a] ? 1 : -1
                let tip = x[b] - dir * 2
                body += "<line class=\"\(cls)\" x1=\"\(n(x[a]))\" y1=\"\(n(ly))\" x2=\"\(n(tip))\" y2=\"\(n(ly))\"/>"
                body += arrowHead(head, tipX: tip, y: ly, dir: dir)
                if both { body += arrowHead(.arrow, tipX: x[a] + dir * 2, y: ly, dir: -dir) }
                y = ly + 16
            }
        case .note(let at, let side, let text):
            use(at)
            let lines = apSeqLines(text)
            let w = max(width(text, ms) + 20, 70), h = Double(lines.count) * lh + 10
            let left: Double
            switch side {
            case .over where at.count == 2:
                let lo = min(x[at[0]], x[at[1]]), hi = max(x[at[0]], x[at[1]])
                let ww = max(hi - lo + 40, w)
                left = (lo + hi) / 2 - ww / 2
                body += "<rect class=\"nb\" x=\"\(n(left))\" y=\"\(n(y))\" width=\"\(n(ww))\" height=\"\(n(h))\" rx=\"3\"/>"
                body += textLines(lines, x: (lo + hi) / 2, top: y + 3, cls: "nt", anchor: "middle")
                minX = min(minX, left - 6); maxX = max(maxX, left + ww + 6)
            default:
                left = side == .over ? x[at[0]] - w / 2 : side == .right ? x[at[0]] + 10 : x[at[0]] - 10 - w
                body += "<rect class=\"nb\" x=\"\(n(left))\" y=\"\(n(y))\" width=\"\(n(w))\" height=\"\(n(h))\" rx=\"3\"/>"
                body += textLines(lines, x: left + w / 2, top: y + 3, cls: "nt", anchor: "middle")
                minX = min(minX, left - 6); maxX = max(maxX, left + w + 6)
            }
            y += h + 12
        case .begin(let kind, let label):
            open.append(Open(kind: kind, label: label, top: y, used: [], divides: [], depth: open.count))
            y += kind == "box" ? 0 : (label.isEmpty ? 14 : 24)
        case .divide(_, let label):
            if !open.isEmpty { open[open.count - 1].divides.append((y, label)) }
            y += 24
        case .end:
            guard let o = open.popLast() else { break }
            if o.kind == "box" { break }
            let pad = max(56 - Double(o.depth) * 8, 24)
            let ps = o.used.isEmpty ? Set(0..<count) : o.used
            let lo = (ps.min().map { x[$0] } ?? 0) - pad, hi = (ps.max().map { x[$0] } ?? 0) + pad
            let bottom = y + 2
            var f = "<rect class=\"fr\(o.kind == "rect" ? " rc" : "")\" x=\"\(n(lo))\" y=\"\(n(o.top))\" width=\"\(n(hi - lo))\" height=\"\(n(bottom - o.top))\" rx=\"2\"/>"
            if o.kind != "rect" {
                let tw = apTextWidth(o.kind, size: 11.5) + 14
                f += "<path class=\"ft\" d=\"M\(n(lo)) \(n(o.top))h\(n(tw))v12l-6 6h-\(n(tw - 6))z\"/>"
                f += "<text class=\"fl\" x=\"\(n(lo + 6))\" y=\"\(n(o.top + 13))\">\(t(o.kind))</text>"
                if !o.label.isEmpty { f += "<text class=\"fc\" x=\"\(n(lo + tw + 8))\" y=\"\(n(o.top + 14))\">[\(t(o.label))]</text>" }
            }
            for (dy, dl) in o.divides {
                f += "<line class=\"fs\" x1=\"\(n(lo))\" y1=\"\(n(dy))\" x2=\"\(n(hi))\" y2=\"\(n(dy))\"/>"
                if !dl.isEmpty { f += "<text class=\"fc\" x=\"\(n(lo + 8))\" y=\"\(n(dy + 15))\">[\(t(dl))]</text>" }
            }
            frames += f
            minX = min(minX, lo - 4); maxX = max(maxX, hi + 4)
            y = bottom + 10
        }
    }
    let bottomTop = y + 6
    var heads = "", lifes = ""
    for (k, p) in P.enumerated() {
        lifes += "<line class=\"ll\" x1=\"\(n(x[k]))\" y1=\"\(n(10 + boxH))\" x2=\"\(n(x[k]))\" y2=\"\(n(bottomTop))\"/>"
        for top in [10.0, bottomTop] {
            heads += "<rect class=\"pb\(p.actor ? " ac" : "")\" x=\"\(n(x[k] - boxW[k] / 2))\" y=\"\(n(top))\" width=\"\(n(boxW[k]))\" height=\"\(n(boxH))\" rx=\"\(p.actor ? 14 : 4)\"/>"
            let ls = plines[k]
            let first = top + (boxH - Double(ls.count) * lh) / 2
            heads += textLines(ls, x: x[k], top: first - 1, cls: "pt", anchor: "middle")
        }
        minX = min(minX, x[k] - boxW[k] / 2 - 4)
    }
    let h = bottomTop + boxH + 10
    let vx = floor(minX), vw = ceil(maxX - minX)
    return "<svg class=\"sq\" viewBox=\"\(n(vx)) 0 \(n(vw)) \(n(h))\" width=\"\(n(vw))\" height=\"\(n(h))\" role=\"img\" aria-label=\"\(t(label))\">"
        + frames + lifes + heads + body + "</svg>"
}

private func arrowHead(_ head: APSeqHead, tipX: Double, y: Double, dir: Double) -> String {
    switch head {
    case .none: return ""
    case .arrow: return "<polygon class=\"ah\" points=\"\(n(tipX)),\(n(y)) \(n(tipX - dir * 9)),\(n(y - 4.5)) \(n(tipX - dir * 9)),\(n(y + 4.5))\"/>"
    case .open: return "<polyline class=\"ml\" points=\"\(n(tipX - dir * 9)),\(n(y - 5)) \(n(tipX)),\(n(y)) \(n(tipX - dir * 9)),\(n(y + 5))\"/>"
    case .cross:
        let c = tipX - dir * 5
        return "<line class=\"ml\" x1=\"\(n(c - 4))\" y1=\"\(n(y - 4))\" x2=\"\(n(c + 4))\" y2=\"\(n(y + 4))\"/><line class=\"ml\" x1=\"\(n(c - 4))\" y1=\"\(n(y + 4))\" x2=\"\(n(c + 4))\" y2=\"\(n(y - 4))\"/>"
    }
}
