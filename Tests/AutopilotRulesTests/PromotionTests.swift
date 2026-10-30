// 가지 자리·승격 차례 채우기 규칙(Sources/AutopilotRules/Promotion.swift) 시험.
// 2026-10-30 5-2b 에서 보드 Autopilot.swift(APStart.roles 와 출발 준비·합치기 시트의 같은 채우기 식)에서 옮길 때, 옮기기 전 식(e6679ac)과
// 같은 입력 270줄(가지 여덟 가지의 모든 부분집합 256, 차례를 바꾼 것 하나, 시험 빌드 뿌리 장난감 실행 run.json 의 branches.start 가지 목록 13)로
// 돌린 덤프가 모두 같았다. 아래는 그 가운데 갈래마다 고른 줄이다
import Testing
import AutopilotRules

struct RolesRow: Row {
    let note: String
    let branches: [String]
    let want: String     // "dev/live/master"(없으면 -)
    let steps: String    // "autopilot>dev autopilot>live@dev live>master@live"
}

private let rolesRows: [RolesRow] = [
    .init(note: "가지가 없음 → 자리도 단계도 없음", branches: [], want: "-/-/-", steps: ""),
    .init(note: "dev·live·master → 세 단계(장난감 zz-ap53)", branches: ["autopilot/zz-ap53-jira", "dev", "live", "main", "master"], want: "dev/live/master",
          steps: "autopilot>dev autopilot>live@dev live>master@live"),
    .init(note: "master 가 없으면 main 이 그 자리(장난감 zz-ap52)", branches: ["autopilot/zz-ap52-p1", "dev", "live", "main"], want: "dev/live/main",
          steps: "autopilot>dev autopilot>live@dev live>main@live"),
    .init(note: "main 하나 → 자율주행 가지 → main 한 단계", branches: ["main"], want: "-/-/main", steps: "autopilot>main"),
    .init(note: "develop·staging·main 은 같은 자리로 짝짓는다", branches: ["develop", "staging", "main"], want: "develop/staging/main",
          steps: "autopilot>develop autopilot>staging@develop staging>main@staging"),
    .init(note: "dev 와 develop 이 함께 있으면 dev", branches: ["develop", "dev", "main"], want: "dev/-/main", steps: "autopilot>dev autopilot>main@dev"),
    .init(note: "live 와 staging 이 함께 있으면 live", branches: ["staging", "live"], want: "-/live/-", steps: "autopilot>live"),
    .init(note: "master 와 main 이 함께 있으면 master", branches: ["main", "master"], want: "-/-/master", steps: "autopilot>master"),
    .init(note: "live 없이 dev·master → master ← 자율주행 가지(dev 뒤)", branches: ["dev", "master"], want: "dev/-/master", steps: "autopilot>dev autopilot>master@dev"),
    .init(note: "dev 없이 live·master → live 는 뒤 조건 없음", branches: ["live", "master"], want: "-/live/master", steps: "autopilot>live live>master@live"),
    .init(note: "다른 이름 가지는 자리가 아니다", branches: ["feature/x", "autopilot/zz"], want: "-/-/-", steps: ""),
]

@Suite("자율주행 승격 차례: 가지 자리와 저장소 가지로 채우기") struct PromotionFillTests {
    @Test("자리 셋과 채운 단계", arguments: rolesRows)
    func 표(_ r: RolesRow) {
        let n = apBranchRoles(r.branches)
        #expect("\(n.dev ?? "-")/\(n.live ?? "-")/\(n.master ?? "-")" == r.want)
        let s = apDefaultPromotion(r.branches).map { "\($0.from)>\($0.into)" + ($0.after.map { "@" + $0 } ?? "") }.joined(separator: " ")
        #expect(s == r.steps)
    }
}
