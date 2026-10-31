// 자율주행 규칙(AutopilotRules) — 하네스 읽기(설계 2장 끝 "자율주행 세트와 사용자 하네스"의 "하네스 읽기").
// 저장소 하네스(CLAUDE.md·.claude/ 등)에서 사람 확인·승인·묻기·미리보기·기다리기·바깥 쓰기 자리(확인 지점)를 기계로 찾고, 자율주행 지시가 그 자리를
// 이름(파일:줄)으로 다뤘는지 보고, 빠진 자리는 출발 카드로 묻는다. 5단계 ⑪-2 에서 Draft.swift 에 두었던 것을 7단계 ⑫-7b 에서 이 파일로 옮겼다(옮기기만).
// 하네스 파일 읽기는 포트(APHarnessSource)로만 한다 — 진짜 어댑터는 엔진, 가짜 어댑터는 Tests/AutopilotRulesTests 에만 둔다.
// 사람이 읽는 글에는 긴 줄표를 쓰지 않는다
import Foundation

// MARK: - 하네스 확인 지점 (설계 2장 끝 "하네스 읽기")

// 저장소 하네스에서 기계로 찾은 확인 지점 하나 — 사람에게 묻기·확인받기·승인·미리보기·기다리기·바깥 쓰기 도구 이름
public struct APHarnessPoint: Equatable, Sendable {
    public var file: String      // 저장소 뿌리 기준 상대 경로
    public var line: Int         // 1부터
    public var kind: String      // ask·confirm·approve·preview·wait·write
    public var text: String      // 그 줄(앞뒤 빈칸을 다듬고 200자까지)
    public init(file: String, line: Int, kind: String, text: String) { self.file = file; self.line = line; self.kind = kind; self.text = text }
    public var place: String { "\(file):\(line)" }
}

// 읽을 하네스 파일인가(저장소 뿌리 기준 상대 경로) — CLAUDE.md·CLAUDE.local.md(깊이 3 까지), .claude/ 의 규칙·스킬·에이전트·명령 글
public func apHarnessFileWanted(_ rel: String) -> Bool {
    let parts = rel.split(separator: "/").map(String.init)
    guard let last = parts.last, !parts.contains(where: { $0 == ".." || $0 == "." || $0.isEmpty }) else { return false }
    if (last == "CLAUDE.md" || last == "CLAUDE.local.md") && parts.count <= 3 && !parts.contains(where: { ["node_modules", ".git", "build", "dist"].contains($0) }) { return true }
    guard parts.first == ".claude", parts.count >= 3, last.hasSuffix(".md") else { return false }
    return ["rules", "skills", "agents", "commands"].contains(parts[1])
}

private let harnessPatterns: [(kind: String, re: NSRegularExpression)] = [
    ("write", "createJiraIssue|editJiraIssue|addCommentToJiraIssue|transitionJiraIssue|createConfluence(Page|Content)|updateConfluence(Page|Content)|mcp__[A-Za-z0-9_-]*atlassian"),
    ("ask", "AskUserQuestion|묻(는|고|어|습|기)|물어|질문(하|을)|ask (the )?user"),
    ("confirm", "확인(을)? ?받|컨펌|confirm (with|before)"),
    ("approve", "승인|approv"),
    ("preview", "미리 ?보기|preview"),
    ("wait", "기다(리|린|려|릴|렸)|대기(하|해|한)|wait for"),
].map { ($0.0, try! NSRegularExpression(pattern: $0.1, options: [.caseInsensitive])) }

public let apHarnessPointMax = 200

// 하네스 파일 글에서 확인 지점을 찾는다(줄마다 처음 맞은 종류 하나). 파일 차례·줄 차례, 200곳까지
public func apHarnessPoints(_ files: [(path: String, text: String)]) -> [APHarnessPoint] {
    var out: [APHarnessPoint] = []
    for f in files.sorted(by: { $0.path < $1.path }) where apHarnessFileWanted(f.path) {
        for (i, raw) in f.text.components(separatedBy: "\n").enumerated() {
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty else { continue }
            let r = NSRange(line.startIndex..., in: line)
            guard let k = harnessPatterns.first(where: { $0.re.firstMatch(in: line, range: r) != nil })?.kind else { continue }
            out.append(APHarnessPoint(file: f.path, line: i + 1, kind: k, text: String(line.prefix(200))))
            if out.count >= apHarnessPointMax { return out }
        }
    }
    return out
}

// 지시 글이 그 확인 지점을 이름으로 다뤘나 — "파일:줄" 이 글에 그대로 있어야 한다(설계 판단: 이름으로 하나씩)
public func apHarnessMissing(_ points: [APHarnessPoint], instruction: String) -> [APHarnessPoint] {
    points.filter { !instruction.contains($0.place) }
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

// MARK: - 하네스 파일 글 모으기 (엔진 harnessTexts 를 그대로 옮김, 7단계 ⑫-7b)

// 내려가지 않는 이름(폴더·파일 모두 건너뜀)과 상한 — 파일마다 256 KB, 300개까지
public let apHarnessWalkSkip: Set<String> = [".git", "node_modules", "build", "dist", ".gradle", "target", ".venv", ".build"]
public let apHarnessFileBytes = 256 << 10
public let apHarnessFileMax = 300

// 하네스 파일 글(출발 가지의 커밋된 판 = 임시 작업 트리, 또는 작업 경로) — apHarnessFileWanted 가 고른 것만
public func apHarnessTexts(_ src: APHarnessSource) -> [(path: String, text: String)] {
    var out: [(path: String, text: String)] = []
    src.walk { rel in
        let name = (rel as NSString).lastPathComponent
        if apHarnessWalkSkip.contains(name) { return .skip }
        guard apHarnessFileWanted(rel) else { return .next }
        guard let d = src.read(rel, max: apHarnessFileBytes) else { return .next }
        out.append((rel, String(decoding: d, as: UTF8.self)))
        return out.count >= apHarnessFileMax ? .stop : .next
    }
    return out
}
