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
