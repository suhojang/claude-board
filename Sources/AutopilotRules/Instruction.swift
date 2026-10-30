// 자율주행 규칙(AutopilotRules) — 출발 확인의 자율주행 지시(세트 private/projects/<프로젝트>/set/autopilot.md)를 그 자리에서 고치기와
// 지난 판(기록 저장소 sets 의 instruction)과의 차이(설계 2장 끝 "자율주행 지시", 와이어프레임 자율주행 지시 ②). 2026-10-30 2단계 5-2b 에서 새로 씀.
// 기록 저장소·파일은 보드가 읽고 쓰고, 규칙은 글 두 개를 받는다
import Foundation

public enum APDiffKind: String, Equatable, Sendable { case same, add, del }
public struct APDiffLine: Equatable, Sendable {
    public let kind: APDiffKind
    public let text: String
    public init(_ kind: APDiffKind, _ text: String) { self.kind = kind; self.text = text }
}

// 줄 차이 — 엔진이 지시를 읽을 때처럼 앞뒤 빈 줄·빈칸을 자른 두 글을 줄로 견준다(가장 긴 공통 줄, 뺀 줄을 더한 줄보다 앞에).
// 줄이 너무 많으면(둘의 곱이 400만 넘음) 모두 빼고 모두 더한 것으로 본다(보드가 멈추지 않게)
public func apLineDiff(_ old: String, _ new: String) -> [APDiffLine] {
    func lines(_ s: String) -> [String] {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? [] : t.components(separatedBy: "\n").map { $0.hasSuffix("\r") ? String($0.dropLast()) : $0 }
    }
    let a = lines(old), b = lines(new)
    if a.count * b.count > 4_000_000 { return a.map { APDiffLine(.del, $0) } + b.map { APDiffLine(.add, $0) } }
    var t = Array(repeating: Array(repeating: 0, count: b.count + 1), count: a.count + 1)
    for i in stride(from: a.count - 1, through: 0, by: -1) {
        for j in stride(from: b.count - 1, through: 0, by: -1) {
            t[i][j] = a[i] == b[j] ? t[i + 1][j + 1] + 1 : max(t[i + 1][j], t[i][j + 1])
        }
    }
    var out: [APDiffLine] = [], i = 0, j = 0
    while i < a.count || j < b.count {
        if i < a.count, j < b.count, a[i] == b[j] { out.append(APDiffLine(.same, a[i])); i += 1; j += 1 }
        else if i < a.count, j == b.count || t[i + 1][j] >= t[i][j + 1] { out.append(APDiffLine(.del, a[i])); i += 1 }
        else { out.append(APDiffLine(.add, b[j])); j += 1 }
    }
    return out
}

public func apDiffCount(_ d: [APDiffLine]) -> (add: Int, del: Int) {
    (d.filter { $0.kind == .add }.count, d.filter { $0.kind == .del }.count)
}

// 지시 머리 줄 — "지난 판(10월 20일)과 차이 +2 −1" / "지난 판과 같음" / "지난 판 없음"(날짜 글자는 보드가 만든다)
public func apInstructionHead(old: String?, new: String?, date: String) -> String {
    guard let o = old else { return new == nil ? "지시 없음 · 공통 규칙만으로 돕니다" : "지난 판 없음 · 이 프로젝트에서 처음 쓰는 지시" }
    guard let n = new else { return "지난 판(\(date))이 있으나 지금 세트에 지시가 없습니다" }
    let c = apDiffCount(apLineDiff(o, n))
    return c.add + c.del == 0 ? "지난 판(\(date))과 같음" : "지난 판(\(date))과 차이 +\(c.add) −\(c.del)"
}

// 고친 지시를 저장해도 되는가 — 빈 글은 저장하지 않는다(지시 없이 돌리는 것은 세트 파일을 지우는 일이라 이 자리에서 하지 않는다).
// 엔진이 지시를 시스템 프롬프트 인자로 넘기므로 길이 상한(10만 자). 되면 nil
public func apInstructionSaveProblem(_ text: String) -> String? {
    let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
    if t.isEmpty { return "빈 지시는 저장하지 않습니다. 고친 글을 넣어 주세요" }
    if t.count > 100_000 { return "지시가 너무 깁니다(\(t.count)자, 10만 자까지)" }
    return nil
}
