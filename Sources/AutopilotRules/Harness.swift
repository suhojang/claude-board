// 자율주행 규칙(AutopilotRules) — 하네스 읽기(설계 2장 끝 "자율주행 세트와 사용자 하네스"의 "하네스 읽기").
// 저장소 하네스(CLAUDE.md·.claude/ 등)에서 사람 확인·승인·묻기·미리보기·기다리기·바깥 쓰기·hook 막기 자리(확인 지점)를 기계로 찾고, 자율주행 지시가 그 자리를
// 이름(파일:줄)으로 다뤘는지 보고, 빠진 자리는 출발 카드로 묻는다. 5단계 ⑪-2 에서 Draft.swift 에 두었던 것을 7단계 ⑫-7b 에서 이 파일로 옮기고 일반판으로 다듬었다:
//   읽는 파일은 엔진 하네스 지문(scanHarness)과 같은 자리(깊이 4 까지의 CLAUDE.md·CLAUDE.local.md·.mcp.json, .claude/ 안 모두. 글 파일만),
//   이름은 대소문자·유니코드 정규화를 접어 견주고(APFS), 낱말 꼴은 사람·답이 걸린 자리만 잡고 바로 뒤가 부정이면 지나간다(이중 부정은 잡음),
//   울타리 코드 안은 도구 이름만, hook 막기(exit 2·decision block)와 권한 묻기(permissions.ask)는 설정·hook 파일에서만.
// 하네스 지문(파일마다 FNV-1a 64)을 지난 세트 기준과 견주어 바뀐 파일만 세트 이어 쓰기에 넘긴다(⑫-7b, 설계 2장 "세트 기록과 이어 쓰기").
// 하네스 파일 읽기는 포트(APHarnessSource)로만 한다 — 진짜 어댑터는 엔진, 가짜 어댑터는 Tests/AutopilotRulesTests 에만 둔다.
// 사람이 읽는 글에는 긴 줄표를 쓰지 않는다
import Foundation

// MARK: - 하네스 확인 지점 (설계 2장 끝 "하네스 읽기")

// 저장소 하네스에서 기계로 찾은 확인 지점 하나 — 사람에게 묻기·확인받기·승인·미리보기·기다리기·바깥 쓰기·hook 막기
public struct APHarnessPoint: Equatable, Sendable {
    public var file: String      // 저장소 뿌리 기준 상대 경로
    public var line: Int         // 1부터
    public var kind: String      // ask·confirm·approve·preview·wait·write·block
    public var text: String      // 그 줄(앞뒤 빈칸을 다듬고 200자까지)
    public init(file: String, line: Int, kind: String, text: String) { self.file = file; self.line = line; self.kind = kind; self.text = text }
    public var place: String { "\(file):\(line)" }
}

public let apHarnessKindLabels: [String: String] = ["ask": "묻기", "confirm": "확인받기", "approve": "승인", "preview": "미리보기", "wait": "기다리기",
                                                     "write": "바깥 쓰기", "block": "hook 막기"]

// 하네스 파일 이름(접은 꼴) — CLAUDE.md·CLAUDE.local.md·.mcp.json 은 폴더 깊이 4 까지, .claude/ 는 깊이 4 까지의 그 폴더 안 모두(엔진 scanHarness 와 같은 자리)
private let harnessNames: Set<String> = ["claude.md", "claude.local.md", ".mcp.json"]
private let harnessDepth = 4
// .claude/ 안에서 내려가지 않는 자리(프로젝트 흐름이 만든 git 작업 트리·claude 의 임시 자리)
private let claudeSkip: Set<String> = ["worktrees", ".cc-writes"]
// .claude/ 안에서 읽는 글 파일 꼴(그림·압축·바이너리는 지문에만 들지 않는다). 확장자 없는 이름(hook 스크립트)도 읽는다
private let textExts: Set<String> = ["md", "markdown", "txt", "json", "jsonc", "yaml", "yml", "toml", "sh", "bash", "zsh", "py", "js", "mjs", "cjs", "ts", "rb", "pl"]

// 읽을 하네스 파일인가(저장소 뿌리 기준 상대 경로, 대소문자·유니코드 정규화를 접어 봄)
public func apHarnessFileWanted(_ rel: String) -> Bool {
    let raw = rel.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
    guard !raw.isEmpty, !raw.contains(where: { $0 == ".." || $0 == "." || $0.isEmpty }) else { return false }
    let parts = apPathKey(rel).split(separator: "/").map(String.init)
    guard let last = parts.last else { return false }
    if let ci = parts.firstIndex(of: ".claude") {
        guard ci <= harnessDepth, ci < parts.count - 1, !parts[..<ci].contains(where: { apHarnessWalkSkip.contains($0) }) else { return false }
        if claudeSkip.contains(parts[ci + 1]) { return false }
        let ext = last.contains(".") ? String(last.split(separator: ".").last ?? "") : ""
        return ext.isEmpty || textExts.contains(ext)
    }
    guard harnessNames.contains(last), parts.count <= harnessDepth + 1 else { return false }
    return !parts.dropLast().contains { apHarnessWalkSkip.contains($0) }
}

// 줄 꼴 — 한 줄에 처음 맞은 종류 하나(바깥 쓰기 → hook 막기 → 묻기 → 확인 → 승인 → 미리보기 → 기다리기 차례).
// tool: 도구·명령 이름(울타리 코드 안에서도 봄), prose: 사람 말 꼴(울타리 코드·JSON 밖에서만), hook: 설정·hook 스크립트에서만
private struct Shape { let kind: String; let re: NSRegularExpression; let scope: String }
private func shape(_ kind: String, _ scope: String, _ p: String) -> Shape {
    Shape(kind: kind, re: try! NSRegularExpression(pattern: p, options: [.caseInsensitive]), scope: scope)
}
private let person = "(사람|사용자|유저|담당자|리뷰어|팀장|관리자|요청자|운영자|기획자|PM|리드)"
private let shapes: [Shape] = [
    // 바깥 쓰기 도구·명령(설계 2장: Jira·Confluence 쓰기 도구 이름 등). 읽기 도구(getJiraIssue 등)는 잡지 않는다
    shape("write", "tool", "createJiraIssue|editJiraIssue|addCommentToJiraIssue|transitionJiraIssue|createConfluence(Page|Content|FooterComment|InlineComment)|updateConfluence(Page|Content)"
          + "|mcp__[A-Za-z0-9_.-]+__[A-Za-z0-9_-]*(create|update|edit|add|delete|remove|post|send|publish|transition|comment|write|upload|archive|merge|assign)"
          + "|\\bgit\\s+(-C\\s+\\S+\\s+)?push\\b|\\bgh\\s+(pr|issue|release)\\s+(create|merge|comment|edit|close)\\b|\\bglab\\s+mr\\s+(create|merge)\\b"),
    shape("write", "prose", "(jira|지라|confluence|컨플루언스|컨플|notion|노션)[^.。!?\\n]{0,20}?(만들|만든|생성|올리|올린|댓글|코멘트|게시|업데이트|갱신|등록|전환|바꾸|바꾼)"
          + "|(slack|슬랙|메신저|이메일|메일|webhook|웹훅)[^.。!?\\n]{0,12}?(알리|알린|보내|보낸|올리|올린|공유|게시)"),
    // hook 막기·권한 묻기(설정·hook 스크립트)
    shape("block", "hook", "\\bexit\\s+2\\b|sys\\.exit\\(\\s*2\\s*\\)|process\\.exit\\(\\s*2\\s*\\)|\"decision\"\\s*:\\s*\"block\"|permissionDecision\"?\\s*[:=]\\s*\"?(deny|ask)\\b|\"continue\"\\s*:\\s*false"
          + "|\"(PreToolUse|PermissionRequest|UserPromptSubmit)\"\\s*:"),
    shape("ask", "hook", "\"ask\"\\s*:\\s*\\["),
    // 묻기
    shape("ask", "tool", "AskUserQuestion"),
    shape("ask", "prose", "묻(는다|습니다|고 |고서|기 전|어 보|어보|어야)|물어(보|본|봐)|여쭤|되물|질문(하고|한 뒤|한 후|해서|해야|한다|합니다|을 하|을 남기고)"
          + "|\\bask (the )?(user|human|me|for (clarification|input|permission))\\b|\\bask before\\b|clarifying question"),
    // 확인받기
    shape("confirm", "prose", "확인(을)? ?받|확인(을)? ?(요청|구하|거치|거친|거쳐)|컨펌|검토(를)? ?받|리뷰(를)? ?받|허락(을)? ?(받|구)|동의(를)? ?(받|구)|(OK|오케이)(를|을)? ?받"
          + "|" + person + "(에게|한테|께|의)[^.。!?\\n]{0,15}?확인"
          + "|confirm (with|before)|ask for confirmation|get (explicit )?confirmation|human review|sign[- ]?off"),
    // 승인("결제 승인 API" 같은 이름 꼴은 잡지 않는다: 받다·요청·거침·전후·없이·대기만)
    shape("approve", "prose", "승인(을|이)? ?(받|요청|구하|거치|거친|거쳐|필요|난 뒤|된 뒤|후|뒤|전|없이|대기)|결재(를)? ?(받|올리)"
          + "|approv(al|ed?)[^.\\n]{0,20}?(from|by|before|required|needed)|(get|require|need|wait for|request)s? (an? )?approval"),
    // 미리보기(사람에게 보여 줌만. "미리보기 화면을 만든다" 같은 기능 이름은 잡지 않는다)
    shape("preview", "prose", "미리 ?보(여|이)|미리보기(를|로)? ?(보여|보이|띄우|확인|공유)|미리보기로[^.。!?\\n]{0,20}?(보여|보이|확인|공유)"
          + "|show (a |the )?preview|preview (it |them |this )?(to|for|with) (the )?(user|human|reviewer)"),
    // 기다리기(사람·답이 걸린 자리만. "빌드가 끝날 때까지 기다린다" 는 잡지 않는다)
    shape("wait", "prose", person + "(의|이|가)? ?(답|대답|응답|회신|확인|승인|결정|지시|입력)[^.。!?\\n]{0,12}?(기다|대기)"
          + "|(답|대답|회신|승인|확인|검토|리뷰|지시|허락|결정)(을|를|이|가)? ?(기다리|기다린|기다려|기다릴|받을 때까지)"
          + "|멈추고 ?(기다|대기)|멈춰 ?(서 ?)?기다|" + person + "[^.。!?\\n]{0,10}?(기다리|기다린|기다려)"
          + "|wait for (the )?(user|human|approval|confirmation|review|response|reply|input|sign[- ]?off)|pause (and|until) (the )?(user|human)"),
]
// 맞은 꼴 바로 뒤(같은 마디 6자 안)의 부정 — "묻지 않고", "확인받지 않은", "기다리지 말고"
private let negAfter = try! NSRegularExpression(pattern: "^[^.。!?;\\n]{0,6}?(지 ?않|지 ?말|지 ?마|지 ?못|않는|않고|않아|않은|않습)", options: [])
// 맞은 꼴 바로 앞의 영어 부정 — "don't ask", "never wait for", "without approval"
private let negBefore = try! NSRegularExpression(pattern: "(don'?t|do not|never|no need to|without)\\s+([a-z]+\\s+){0,2}$", options: [.caseInsensitive])
// 나머지 마디의 부정(있으면 이중 부정이라 확인 지점으로 남긴다 — "승인 없이 합치지 않는다", "확인받지 않은 변경은 커밋하지 않는다")
private let negRest = try! NSRegularExpression(pattern: "않|말고|말아|금지|못 |안 된|안된|\\bnot\\b|\\bnever\\b|n't", options: [.caseInsensitive])
private let clauseEnd = CharacterSet(charactersIn: ".。!?;\n")

private func clause(_ s: String) -> String {
    guard let r = s.rangeOfCharacter(from: clauseEnd) else { return s }
    return String(s[..<r.lowerBound])
}
private func hits(_ re: NSRegularExpression, _ s: String) -> Bool { re.firstMatch(in: s, range: NSRange(location: 0, length: (s as NSString).length)) != nil }

// 이 줄에서 맞은 확인 지점 종류(없으면 nil). scopes: 이 줄에 볼 꼴(tool·prose·hook)
private func lineKind(_ line: String, scopes: Set<String>) -> String? {
    let ns = line as NSString
    for sh in shapes where scopes.contains(sh.scope) {
        for m in sh.re.matches(in: line, range: NSRange(location: 0, length: ns.length)) {
            if sh.scope == "hook" || !scopes.contains("prose") { return sh.kind }   // hook 막기·스크립트 안 도구 이름은 부정을 보지 않는다
            let matched = ns.substring(with: m.range)
            let after = ns.substring(from: m.range.location + m.range.length)
            let before = clause(String(ns.substring(to: m.range.location).reversed()))
            let beforeText = String(before.reversed())
            let negA = hits(negAfter, after)
            let negB = hits(negBefore, beforeText)
            let without = matched.hasSuffix("없이") || matched.lowercased().contains("without")
            if negA || negB || without {
                // 이중 부정이면 확인 지점: 부정 뒤 같은 마디에 다른 부정이 있음
                var rest = clause(after)
                if negA, let r = negAfter.firstMatch(in: rest, range: NSRange(location: 0, length: (rest as NSString).length)) {
                    rest = (rest as NSString).substring(from: r.range.location + r.range.length)
                }
                if hits(negRest, rest) { return sh.kind }
                continue
            }
            return sh.kind
        }
    }
    return nil
}

public let apHarnessPointMax = 200

// 파일 꼴에 따라 볼 꼴 — 설정(json)은 도구 이름·hook·권한 묻기, hook 자리(.claude/hooks/ 또는 스크립트 확장자)는 도구 이름·hook 막기, 글 문서는 도구 이름·사람 말 꼴
private func scopes(_ rel: String) -> Set<String> {
    let k = apPathKey(rel)
    let last = k.split(separator: "/").last.map(String.init) ?? k
    let ext = last.contains(".") ? String(last.split(separator: ".").last ?? "") : ""
    if ["md", "markdown", "txt"].contains(ext) { return ["tool", "prose"] }
    if ["json", "jsonc"].contains(ext) { return ["hook"] }      // 설정의 허용·거부 목록에 든 도구 이름은 지시가 아니라 설정이다
    return ["tool", "hook"]                                      // hook 스크립트·확장자 없는 이름·yaml 등
}

// 하네스 파일 글에서 확인 지점을 찾는다(줄마다 처음 맞은 종류 하나). 파일 차례(접은 경로)·줄 차례, 200곳까지. 울타리 코드(```) 안은 도구 이름만
public func apHarnessPoints(_ files: [(path: String, text: String)]) -> [APHarnessPoint] {
    var out: [APHarnessPoint] = []
    for f in files.sorted(by: { apPathKey($0.path) < apPathKey($1.path) }) where apHarnessFileWanted(f.path) {
        let sc = scopes(f.path)
        var fence = false
        for (i, raw) in f.text.components(separatedBy: "\n").enumerated() {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if sc.contains("prose"), line.hasPrefix("```") || line.hasPrefix("~~~") { fence.toggle(); continue }
            guard !line.isEmpty else { continue }
            guard let k = lineKind(line, scopes: fence ? sc.subtracting(["prose"]) : sc) else { continue }
            out.append(APHarnessPoint(file: f.path, line: i + 1, kind: k, text: String(line.prefix(200))))
            if out.count >= apHarnessPointMax { return out }
        }
    }
    return out
}

// 지시 글이 그 확인 지점을 이름으로 다뤘나 — "파일:줄" 이 글에 있어야 한다(설계 판단: 이름으로 하나씩).
// 대소문자·유니코드 정규화를 접어 견주고, 앞이 경로 글자면(docs/CLAUDE.md 는 CLAUDE.md 가 아님) 뒤가 숫자로 이어지면(CLAUDE.md:30 은 :3 이 아님) 다룬 것이 아니다.
// 줄 범위(CLAUDE.md:3-5 · 3~5)와 쉼표 목록(CLAUDE.md:3,7)도 받는다
private let placeTail = try! NSRegularExpression(pattern: "^:([0-9]+)(\\s*[-~–]\\s*([0-9]+))?((\\s*,\\s*[0-9]+(\\s*[-~–]\\s*[0-9]+)?)*)", options: [])
private func covers(_ text: String, file: String, line: Int) -> Bool {
    let t = text.precomposedStringWithCanonicalMapping.lowercased()
    let f = file.precomposedStringWithCanonicalMapping.lowercased()
    var from = t.startIndex
    while let r = t.range(of: f, range: from..<t.endIndex) {
        from = r.upperBound
        if r.lowerBound > t.startIndex {
            let prev = t[t.index(before: r.lowerBound)]
            if prev.isLetter || prev.isNumber || "/._-".contains(prev) { continue }
        }
        let tail = String(t[r.upperBound...].prefix(80))
        guard let m = placeTail.firstMatch(in: tail, range: NSRange(location: 0, length: (tail as NSString).length)) else { continue }
        let ns = tail as NSString
        var ranges: [(Int, Int)] = []
        let a = Int(ns.substring(with: m.range(at: 1))) ?? -1
        let b = m.range(at: 3).location != NSNotFound ? (Int(ns.substring(with: m.range(at: 3))) ?? a) : a
        ranges.append((a, b))
        if m.range(at: 4).location != NSNotFound, m.range(at: 4).length > 0 {
            for part in ns.substring(with: m.range(at: 4)).split(separator: ",") {
                let nums = part.split(whereSeparator: { "-~–".contains($0) }).compactMap { Int($0.trimmingCharacters(in: .whitespaces)) }
                if let x = nums.first { ranges.append((x, nums.count > 1 ? nums[1] : x)) }
            }
        }
        if ranges.contains(where: { min($0.0, $0.1) <= line && line <= max($0.0, $0.1) }) { return true }
    }
    return false
}
public func apHarnessMissing(_ points: [APHarnessPoint], instruction: String) -> [APHarnessPoint] {
    points.filter { !covers(instruction, file: $0.file, line: $0.line) }
}

// 지시 글의 금지 줄(바깥 쓰기·하네스 고치기·push 를 시키는 줄) — 쓰는 법 ②("확인 지점마다 파일:줄, 원래 글")대로 하네스 줄을 인용한 줄만 뺀다(7단계 ⑫-7b).
// 하네스가 바깥 쓰기·push 를 시키는 줄을 지시가 인용하면 그 원래 글 때문에 지시가 막히던 것. 뺄 수 있는 줄은 완전히 고정된 꼴 하나뿐이다:
//   [목록 머리] <그 지점 파일>:<줄> "<그 지점 글>": <정한 보류 글>[(출발 카드 <그 지점의 실제 카드 id>)][.]
// 그 밖의 꼴은 금지 줄로 걸린다(초안·가져오기는 준비가 멈추고 사람이 고침, 세트 이어 쓰기는 지난 지시 그대로). 인용 앞·뒤 글은 따로 금지 꼴 검사를 한 번 더 한다.
// 보안 검토(⑫-7b 자동 보안 검토 MEDIUM 다섯): 하네스 글을 어디서든 부분 글자로 지움 → 보류 낱말이 인용 뒤 아무 데서나 맞음·인용을 지운 앞뒤를 이어 부정 창이 넘음
// → 카드 꼴이 넓어 금지 동작을 끼울 수 있음·인용 앞이 그 지점 파일이 아닌 아무 글자 덩어리여도 받음. 이 검사는 모델이 쓴 지시 초안을 보는 덧댄 층이고,
// 실제로 막는 것은 샌드박스·도구 허용 목록(바깥 쓰기 도구 없음, push 거부)·하네스 지문이다
private let listHead = try! NSRegularExpression(pattern: "^([-*+]|[0-9]+[.)])\\s+", options: [])
private let quotePairs: [(String, String)] = [("\"", "\""), ("“", "”"), ("「", "」"), ("『", "』"), ("'", "'"), ("‘", "’")]
public let apHarnessDeferPhrases = ["보류해 next_start 에 남깁니다", "보류해 next_start 에 남긴다", "하지 않고 보류합니다", "하지 않고 보류한다", "보류합니다", "보류한다", "하지 않습니다", "하지 않는다"]
private func esc(_ s: String) -> String { NSRegularExpression.escapedPattern(for: s) }
private func quotedHarnessLine(_ line: String, harness: [APHarnessPoint]) -> Bool {
    for p in harness {
        // 인용 앞: 목록 머리 + 그 지점 파일 경로 그대로 + 그 줄(또는 그 줄을 덮는 범위). 인용 뒤: 정한 보류 글 + 그 지점의 실제 카드 id(선택)
        guard let before = try? NSRegularExpression(pattern: "^\\s*(([-*+]|[0-9]+[.)])\\s+)?" + esc(p.file) + ":([0-9]+)(\\s*[-~–]\\s*([0-9]+))?\\s*$", options: [.caseInsensitive]),
              let after = try? NSRegularExpression(pattern: "^\\s*[:：]\\s*(" + apHarnessDeferPhrases.map(esc).joined(separator: "|") + ")(\\s*\\((출발 )?카드 ("
                                                    + esc(apHarnessCardId(p)) + "|" + esc(apHarnessPolicyId(p)) + ")\\))?\\s*[.。]?\\s*$", options: []) else { continue }
        var texts = [p.text]
        let ns = p.text as NSString
        if let m = listHead.firstMatch(in: p.text, range: NSRange(location: 0, length: ns.length)) {
            texts.append(ns.substring(from: m.range.length).trimmingCharacters(in: .whitespaces))
        }
        for q in texts where q.count >= 6 {
            for (o, c) in quotePairs {
                guard let r = line.range(of: o + q + c) else { continue }
                let b = String(line[..<r.lowerBound]), a = String(line[r.upperBound...])
                let bn = b as NSString
                guard let m = before.firstMatch(in: b, range: NSRange(location: 0, length: bn.length)) else { continue }
                let lo = Int(bn.substring(with: m.range(at: 3))) ?? -1
                let hi = m.range(at: 5).location != NSNotFound ? (Int(bn.substring(with: m.range(at: 5))) ?? lo) : lo
                guard min(lo, hi) <= p.line, p.line <= max(lo, hi) else { continue }
                guard hits(after, a), apForbiddenLines(b).isEmpty, apForbiddenLines(a).isEmpty else { continue }
                return true
            }
        }
    }
    return false
}
public func apInstructionForbiddenLines(_ t: String, harness: [APHarnessPoint]) -> [String] {
    t.components(separatedBy: "\n").flatMap { line -> [String] in
        let bad = apForbiddenLines(line)
        return bad.isEmpty || quotedHarnessLine(line, harness: harness) ? [] : bad
    }
}

// 빠진 확인 지점의 출발 카드(막지 않고 묻는다, 2026-10-21 저녁 사용자 결정). 추천은 보류해 다음 출발로
public let apHarnessCardPrefix = "harness-"
public func apHarnessCard(_ p: APHarnessPoint) -> APCardText {
    APCardText(label: "하네스 확인 지점",
               title: "이 확인 지점을 자율주행에서 어떻게 할까요: \(p.place)",
               why: "\(p.text) (초안 세션이 쓴 자율주행 지시가 이 자리를 다루지 않았습니다.)",
               options: [("defer", "보류해 다음 출발로", "이 자리에 닿으면 그 부분만 보류하고 next_start 에 남깁니다"),
                         ("follow", "그대로 따름", "하네스 글대로 합니다. 사람이 필요한 자리면 멈춰 보류합니다"),
                         ("drop", "범위에서 뺌", "이 자리가 걸린 일은 이번 실행에서 하지 않습니다")],
               recommended: "defer")
}
public func apHarnessCardId(_ p: APHarnessPoint) -> String {
    apHarnessCardPrefix + String((p.file + "-" + String(p.line)).map { $0.isLetter || $0.isNumber || $0 == "-" ? $0 : "-" }.prefix(80))
}

// 출발 전 자율주행 가지에 판정 시나리오로 커밋해도 되는 자리(7단계 ⑫-7b: 세트 이어 쓰기가 고친 시나리오·가져오기 세션 시나리오·버린 실행에서 되살린 시나리오).
// 저장소 안 상대 경로이고, 하네스(CLAUDE.md·.claude/·.mcp.json 등, 접은 꼴)·.git 자리가 아니어야 한다 — 이어 갈 단위 정의의 시나리오 자리가 하네스를 가리키면
// 시나리오 고침이 하네스를 바꾸는 길이 되므로(사용자 하네스는 출발 준비·실행·끝 어디서도 고치지 않는다, 설계 1장)
public func apSeedScenarioPathOK(_ rel: String) -> Bool {
    guard apScenarioPathOK(rel) else { return false }
    let parts = apPathKey(rel).split(separator: "/").map(String.init)
    if parts.contains(where: { [".git", ".claude"].contains($0) }) { return false }
    if let last = parts.last, ["claude.md", "claude.local.md", ".mcp.json", "agents.md", ".gitmodules", ".gitattributes"].contains(last) { return false }
    return !apHarnessFileWanted(rel)
}

// MARK: - 정책 카드 (7단계 ⑫-7b): 확인 지점 가운데 사람 몫 정책이 필요한 것 — 바깥 쓰기, 배포·운영 확인

// 정책 카드는 프로젝트 설정 policy_answers 에 답이 남아 다음 출발에 미리 골라진다(2단계 정책 답 길). 그래서 id 는 줄 번호가 아니라
// 파일·종류·줄 글의 지문으로 둔다(앞에 줄이 늘어도 같은 답을 잇는다). 같은 글이 다른 줄로 옮겨도 같은 카드
public let apHarnessPolicyPrefix = "harness-policy-"
private let deployWords = try! NSRegularExpression(pattern: "배포|릴리스|릴리즈|운영|프로덕션|deploy|release|production|\\bprod\\b|마이그레이션|migration", options: [.caseInsensitive])
public func apHarnessPolicyClass(_ p: APHarnessPoint) -> String? {
    if p.kind == "write" { return "write" }
    if ["confirm", "approve", "block", "wait"].contains(p.kind), hits(deployWords, p.text) { return "deploy" }
    return nil
}
public func apHarnessPolicyId(_ p: APHarnessPoint) -> String {
    apHarnessPolicyPrefix + apFNV64(apPathKey(p.file) + "\u{1}" + p.kind + "\u{1}" + p.text.precomposedStringWithCanonicalMapping)
}
public func apHarnessPolicyCard(_ p: APHarnessPoint, missing: Bool) -> APCardText? {
    guard let cls = apHarnessPolicyClass(p) else { return nil }
    let tail = missing ? " 자율주행 지시가 이 자리를 다루지 않았습니다." : " 자율주행 지시가 다루지만, 사람 몫 정책이라 답을 프로젝트 설정에 남겨 다음 출발에도 씁니다."
    if cls == "write" {
        return APCardText(label: "하네스 정책: 바깥 쓰기",
                          title: "하네스가 바깥 쓰기를 시키는 자리를 자율주행에서 어떻게 할까요: \(p.place)",
                          why: "\(p.text) (자율주행 세션에는 바깥 쓰기 도구가 없고 쓰기는 보드 내장 길로만 합니다.\(tail))",
                          options: [("defer", "보류해 다음 출발로", "그 부분은 하지 않고 next_start 에 남깁니다"),
                                    ("record", "결과 기록에만 남김", "올릴 내용을 결과 기록에 쉬운 말로 남기고, 사람이 보고 올립니다"),
                                    ("drop", "범위에서 뺌", "이 자리가 걸린 일은 이번 실행에서 하지 않습니다")],
                          recommended: "defer")
    }
    return APCardText(label: "하네스 정책: 배포·운영 확인",
                      title: "하네스가 배포·운영 전에 사람 확인을 요구하는 자리를 자율주행에서 어떻게 할까요: \(p.place)",
                      why: "\(p.text) (대답할 사람이 없어 자율주행은 그 확인을 받을 수 없습니다.\(tail))",
                      options: [("defer", "그 단계 앞에서 보류", "배포·운영 반영은 하지 않고 next_start 에 남깁니다"),
                                ("drop", "범위에서 뺌", "이 자리가 걸린 일은 이번 실행에서 하지 않습니다")],
                      recommended: "defer")
}

// MARK: - 하네스 파일 글 모으기와 지문 (7단계 ⑫-7b)

// 내려가지 않는 이름(.claude/ 밖의 폴더·파일)과 상한 — 파일마다 256 KB, 300개, 합계 4 MB. 엔진 scanHarness 의 harnessWalkSkip 과 같은 목록
public let apHarnessWalkSkip: Set<String> = [".git", "node_modules", "build", "dist", ".gradle", "target", ".venv", ".build", "out", ".idea", "deriveddata", "pods"]
public let apHarnessFileBytes = 256 << 10
public let apHarnessFileMax = 300
public let apHarnessTotalBytes = 4 << 20

// 어댑터가 읽은 파일 하나 — 보통 파일이면 글, 아니면 종류(link:<대상>·fifo·socket·device·large:<크기>)
public enum APHarnessFileRead: Equatable, Sendable { case data(Data), other(String) }

// 읽은 하네스 파일 하나와 그 지문(FNV-1a 64 — 규칙 라이브러리는 Foundation 만이라 암호 해시 대신, 견주기에만)
public struct APHarnessFileMark: Equatable, Sendable, Codable {
    public var path: String
    public var digest: String
    public var size: Int
    public init(path: String, digest: String, size: Int) { self.path = path; self.digest = digest; self.size = size }
}
public struct APHarnessRead: Equatable, Sendable {
    public var texts: [String: String] = [:]          // 상대 경로 → 글(보통 파일이고 상한 안)
    public var marks: [APHarnessFileMark] = []         // 지문(읽지 않은 링크·큰 파일도 종류로 든다)
    public var skipped: [String] = []                  // "경로: 까닭"
    public var capped = false                          // 개수·합계 상한에 닿아 멈춤
    public var files: [(path: String, text: String)] { texts.keys.sorted().map { ($0, texts[$0]!) } }
    public init() {}
    public static func == (a: APHarnessRead, b: APHarnessRead) -> Bool { a.texts == b.texts && a.marks == b.marks && a.skipped == b.skipped && a.capped == b.capped }
}

public func apFNV64Data(_ d: Data) -> String {
    var h: UInt64 = 0xcbf29ce484222325
    for b in d { h ^= UInt64(b); h = h &* 0x100000001b3 }
    return String(format: "%016llx", h)
}

// 하네스 파일 글과 지문 — apHarnessFileWanted 가 고른 것만, 링크를 따라가지 않고(어댑터), 상한 안에서
public func apHarnessCollect(_ src: APHarnessSource) -> APHarnessRead {
    var r = APHarnessRead(), total = 0
    src.walk { rel in
        let parts = apPathKey(rel).split(separator: "/").map(String.init)
        let inClaude = parts.dropLast().contains(".claude")
        if let name = parts.last {
            if !inClaude, apHarnessWalkSkip.contains(name) { return .skip }
            if inClaude, let ci = parts.firstIndex(of: ".claude"), parts.count == ci + 2, claudeSkip.contains(name) { return .skip }
            if parts.count > harnessDepth + 1, !inClaude, name != ".claude" { return .skip }
        }
        guard apHarnessFileWanted(rel) else { return .next }
        guard r.marks.count < apHarnessFileMax else { r.capped = true; return .stop }
        switch src.readFile(rel, max: min(apHarnessFileBytes, max(apHarnessTotalBytes - total, 0))) {
        case .data(let d)?:
            total += d.count
            r.marks.append(APHarnessFileMark(path: rel, digest: apFNV64Data(d), size: d.count))
            r.texts[rel] = String(decoding: d, as: UTF8.self)
        case .other(let k)?:
            r.marks.append(APHarnessFileMark(path: rel, digest: k, size: 0))
            r.skipped.append("\(rel): " + (k.hasPrefix("link") ? "바로 가기라 따라가지 않습니다" : k.hasPrefix("large") ? "크기 상한(256 KB·합계 4 MB)을 넘습니다" : "보통 파일이 아닙니다(\(k))"))
            if k.hasPrefix("large"), total >= apHarnessTotalBytes { r.capped = true }
        case nil: break
        }
        return .next
    }
    r.marks.sort { apPathKey($0.path) < apPathKey($1.path) }
    return r
}

// 옮기기 커밋의 이름 — 글만(엔진 harnessTexts)
public func apHarnessTexts(_ src: APHarnessSource) -> [(path: String, text: String)] { apHarnessCollect(src).files }

// 지난 세트 기준과 견주기 — 경로는 접어서(대소문자·정규화), 바뀜·더함·뺌
public struct APHarnessChange: Equatable, Sendable {
    public var path: String
    public var change: String     // added·removed·changed
    public init(path: String, change: String) { self.path = path; self.change = change }
}
public func apHarnessChanges(old: [APHarnessFileMark], new: [APHarnessFileMark]) -> [APHarnessChange] {
    var o: [String: APHarnessFileMark] = [:], n: [String: APHarnessFileMark] = [:]
    for m in old { o[apPathKey(m.path)] = m }
    for m in new { n[apPathKey(m.path)] = m }
    var out: [APHarnessChange] = []
    for k in Set(o.keys).union(n.keys).sorted() {
        switch (o[k], n[k]) {
        case (nil, let b?): out.append(APHarnessChange(path: b.path, change: "added"))
        case (let a?, nil): out.append(APHarnessChange(path: a.path, change: "removed"))
        case (let a?, let b?) where a.digest != b.digest || a.size != b.size: out.append(APHarnessChange(path: b.path, change: "changed"))
        default: break
        }
    }
    return out
}

// 세트의 하네스 기준(set/harness.json) — 그 지시가 마지막으로 맞춰진 하네스 지문. 보드가 출발할 때 쓰고 엔진 harness 가 읽는다
public let apHarnessBaselineFormat = "claude-board-harness-baseline"
public struct APHarnessBaseline: Equatable, Sendable {
    public var marks: [APHarnessFileMark]
    public var points: Int
    public var at: Double
    public var run: String
    public init(marks: [APHarnessFileMark], points: Int, at: Double, run: String) { self.marks = marks; self.points = points; self.at = at; self.run = run }
}
public func apHarnessBaselineJSON(_ b: APHarnessBaseline) -> [String: Any] {
    ["format": apHarnessBaselineFormat, "version": 1, "points": b.points, "at": b.at, "run": b.run,
     "files": b.marks.map { ["path": $0.path, "digest": $0.digest, "size": $0.size] as [String: Any] }]
}
public func apHarnessBaselineParse(_ d: Data?) -> APHarnessBaseline? {
    guard let d, d.count <= 1 << 20, let o = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any], o["format"] as? String == apHarnessBaselineFormat,
          let fs = o["files"] as? [[String: Any]] else { return nil }
    let marks = fs.prefix(apHarnessFileMax * 2).compactMap { f -> APHarnessFileMark? in
        guard let p = f["path"] as? String, apScenarioPathOK(p), let g = f["digest"] as? String, g.count <= 300 else { return nil }
        return APHarnessFileMark(path: p, digest: g, size: (f["size"] as? NSNumber)?.intValue ?? 0)
    }
    return APHarnessBaseline(marks: marks, points: (o["points"] as? NSNumber)?.intValue ?? 0, at: (o["at"] as? NSNumber)?.doubleValue ?? 0, run: o["run"] as? String ?? "")
}

// 하네스 읽기 결과(엔진 harness → 준비 폴더 harness.json): 확인 지점·지문·지난 세트 기준과의 차이. 기준이 없으면 그대로로 본다(이번 출발 때 기준이 생김)
public struct APHarnessState: Equatable, Sendable {
    public var points: [APHarnessPoint]
    public var marks: [APHarnessFileMark]
    public var changes: [APHarnessChange]
    public var hasBaseline: Bool
    public var changed: Bool { hasBaseline && !changes.isEmpty }
    public init(points: [APHarnessPoint], marks: [APHarnessFileMark], changes: [APHarnessChange], hasBaseline: Bool) {
        self.points = points; self.marks = marks; self.changes = changes; self.hasBaseline = hasBaseline
    }
}
public func apHarnessState(_ read: APHarnessRead, baseline: APHarnessBaseline?) -> APHarnessState {
    APHarnessState(points: apHarnessPoints(read.files), marks: read.marks, changes: baseline.map { apHarnessChanges(old: $0.marks, new: read.marks) } ?? [],
                   hasBaseline: baseline != nil)
}
// 준비 화면 한 줄
public func apHarnessStateNote(_ s: APHarnessState, setHasInstruction: Bool) -> String {
    var kinds: [String: Int] = [:]
    for p in s.points { kinds[p.kind, default: 0] += 1 }
    let ks = ["ask", "confirm", "approve", "preview", "wait", "write", "block"].compactMap { k in kinds[k].map { "\(apHarnessKindLabels[k] ?? k) \($0)" } }
    var t = s.points.isEmpty ? "확인 지점이 없습니다" : "확인 지점 \(s.points.count)곳(" + ks.joined(separator: " · ") + ")"
    t += " · 파일 \(s.marks.count)개"
    if !setHasInstruction { return t + " · 세트에 지시가 없습니다" }
    if !s.hasBaseline { return t + " · 지난 세트의 하네스 기준이 없어 그대로로 봅니다(이번 출발 때 기준을 남깁니다)" }
    return t + (s.changes.isEmpty ? " · 지난 세트 기준과 같습니다" : " · 지난 세트 기준과 달라진 파일 \(s.changes.count)개")
}
