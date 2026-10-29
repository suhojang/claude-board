// 자율주행 규칙(AutopilotRules) — 저장소 가지의 자리(dev·live·master)와 승격 차례(projects/<프로젝트>.json promotion)를 저장소 가지로 채우기.
// 2026-10-30 5-2b 에서 보드 Autopilot.swift 에서 동작 그대로 옮겼다(APStart.roles, 출발 준비 pickDefaultBases 와 합치기·승격 시트에 같은 식이 둘 있던 것).
// git 은 부르지 않는다. 보드가 읽은 가지 이름 목록을 값으로 받는다
import Foundation

// 가지 자리 — dev(develop), live(staging), master(main). 승격 차례를 저장소 가지로 채울 때 같은 자리로 짝짓는다(설계 2장 끝)
public func apBranchRoles(_ b: [String]) -> (dev: String?, live: String?, master: String?) {
    (["dev", "develop"].first { b.contains($0) }, ["live", "staging"].first { b.contains($0) }, ["master", "main"].first { b.contains($0) })
}

// 프로젝트 설정에 승격 차례가 없을 때 저장소 가지로 채운 차례 — dev ← 자율주행 가지, live ← 자율주행 가지(dev 뒤),
// master ← live(live 뒤). live 가 없으면 master ← 자율주행 가지(dev 뒤). 없는 자리의 단계는 없다
public func apDefaultPromotion(_ branches: [String]) -> [APStep] {
    let n = apBranchRoles(branches)
    var st: [APStep] = []
    if let d = n.dev { st.append(APStep(from: "autopilot", into: d)) }
    if let l = n.live { st.append(APStep(from: "autopilot", into: l, after: n.dev)) }
    if let m = n.master { st.append(n.live.map { APStep(from: $0, into: m, after: $0) } ?? APStep(from: "autopilot", into: m, after: n.dev)) }
    return st
}

// MARK: - 출발 확인에서 승격 차례 바꾸기(2026-10-30 2단계 5-2b)

// 바꾼 승격 차례의 고칠 것 — branches 는 저장소 가지 이름(묶음이면 모든 저장소에 있는 가지). 엔진 finish 의 차례 규칙과 맞춘다:
// 자율주행 가지를 합치는 단계(from autopilot)와 기존 가지끼리 올리는 단계(from <가지>), after 는 먼저 끝나야 할 단계의 받는 가지.
// 고칠 것이 있으면 출발을 잠근다(엔진은 차례가 틀려도 출발은 하지만 끝난 뒤 합치기·승격이 막히므로 출발 전에 고친다)
public func apPromotionProblems(_ steps: [APStep], branches: [String]) -> [String] {
    var out: [String] = []
    var seen: [String] = [], intos: [String] = []
    for (i, s) in steps.enumerated() {
        let n = "단계 \(i + 1)"
        if !branches.contains(s.into) { out.append("\(n): 받는 가지 \(s.into) 가 저장소에 없습니다") }
        if s.from != "autopilot" {
            if s.from == s.into { out.append("\(n): 같은 가지(\(s.into))끼리는 올리지 않습니다") }
            else if !branches.contains(s.from) { out.append("\(n): 올릴 가지 \(s.from) 가 저장소에 없습니다") }
            else if !intos.contains(s.from) { out.append("\(n): \(s.from) 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)") }
        }
        if let a = s.after {
            if a == s.into { out.append("\(n): 뒤 조건이 받는 가지 자신(\(a))입니다") }
            else if !intos.contains(a) { out.append("\(n): \(a) 뒤라는 조건의 단계가 앞에 없습니다") }
        }
        let key = s.from + ">" + s.into
        if seen.contains(key) { out.append("\(n): 같은 단계(\(s.from == "autopilot" ? "자율주행 가지" : s.from) → \(s.into))가 두 번 있습니다") }
        seen.append(key)
        if !intos.contains(s.into) { intos.append(s.into) }
    }
    return out
}

// 바꾸기 화면의 고를 수 있는 것 — 단계 i 의 받는 가지(저장소 가지에서 그 단계의 올릴 가지 말고), 올릴 것(자율주행 가지와 앞 단계가 받는 가지),
// 뒤 조건(없음과 앞 단계가 받는 가지)
public func apPromotionChoices(_ steps: [APStep], at i: Int, branches: [String]) -> (into: [String], from: [String], after: [String]) {
    let before = steps.prefix(max(0, min(i, steps.count))).map(\.into)
    var prev: [String] = []
    for b in before where !prev.contains(b) { prev.append(b) }
    let from = i < steps.count ? steps[i].from : "autopilot"
    return (branches.filter { $0 != from }, ["autopilot"] + prev, prev)
}
