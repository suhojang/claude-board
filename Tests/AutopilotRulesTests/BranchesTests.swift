// 자율주행 가지 기록·합치기 상태 규칙(Sources/AutopilotRules/Branches.swift) 시험.
// 2026-10-29 보드 Autopilot.swift 에서 옮길 때, 옮기기 전 함수(4f57c38)를 같은 입력으로 돌린 덤프의 입력·기대값이다(옮긴 뒤 함수와 줄마다 같았다).
// 진짜 입력은 시험 빌드 뿌리(~/Library/Application Support/Claude Board Dev/autopilot)의 장난감 실행 run.json·기록 저장소 runs.branches 26개를
// 판정에 쓰는 칸만 남기고 경로를 /w/<폴더>/<저장소> 로, 커밋을 7자로 줄인 것이다(줄인 입력도 옮기기 전 함수로 원래 입력과 같은 결과가 나왔다).
// 승격 차례는 그 실행의 프로젝트 설정(projects/<프로젝트>.json promotion), withAutopilot 은 보드가 넘기는 값(Jira 실행이면 스토리 없는 단위가 있는가, 아니면 늘 참).
// 기대값은 지금 동작 그대로다 — 5-3b 에서 고칠 것(② 버린 스토리 하나만 있어도 실행 전체가 버림, ④ 합치기 기다림이 사라지지 않음)도 고치지 않은 채로 적었다.
// 고치면 그 줄의 기대값을 바꾸고 note 에 까닭을 적는다
import Foundation
import Testing
import AutopilotRules

// 표의 줄 — 시험 결과에 note 가 보인다(BoardRulesTests 와 같은 꼴)
protocol Row: Sendable, CustomTestStringConvertible { var note: String { get } }
extension Row { var testDescription: String { note } }

private func object(_ json: String) -> [String: Any] {
    (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any] ?? [:]
}
// 결과를 키 차례로 적은 JSON 글자로 — 기대값과 글자로 견준다
private func text(_ o: Any) -> String {
    guard let d = try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys, .withoutEscapingSlashes]) else { return "?" }
    return String(decoding: d, as: UTF8.self)
}
// 승격 차례 줄임 글자: "autopilot>dev autopilot>live@dev live>master@live"(@ 뒤는 after)
private func steps(_ s: String) -> [APStep] {
    s.split(separator: " ").map { t in
        let ft = t.split(separator: "@", maxSplits: 1), fi = ft[0].split(separator: ">", maxSplits: 1)
        return APStep(from: String(fi[0]), into: String(fi[1]), after: ft.count > 1 ? String(ft[1]) : nil)
    }
}

// MARK: - 장난감 기록(줄인 branches)

private let toys: [String: String] = [
    "20261027-1909-zz-ap52-todo": #"{"discarded":[{"repo":"/w/zz-ap52-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/todo":{}},"stories":{}}"#,
    "20261027-1918-zz-ap52-jira": #"{"discarded":[{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/jira":{}},"stories":{"SHOP-12":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]},"SHOP-20":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#,
    "20261028-1043-zz-ap53-jira": #"{"discarded":[{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"autopilot"}],"promoted":[{"at":1793152437.734,"into":"dev","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793152443.526,"into":"live","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793152494.429,"into":"master","repo":"/w/zz-ap53-toys/jira","source":"branch:live"}],"repos":{"/w/zz-ap53-toys/jira":{}},"stories":{"SHOP-12":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-12/parent/SHOP-12","head":"68142a1","merged":[],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-20":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-20/parent/SHOP-20","head":"68142a1","merged":[],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#,
    "merge-check-1": #"{"discarded":[{"repo":"/w/toy/repo","source":"autopilot"}],"promoted":[],"repos":{"/w/toy/repo":{}},"stories":{}}"#,
    "zz-ap51-inplace": #"{"repos":{"/w/zz-ap51-toys/shop":{}},"stories":{}}"#,
    "zz-ap51-jira": #"{"repos":{"/w/zz-ap51-toys/jira":{}},"stories":{"SHOP-12":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"fix/_SHOP-12/parent/SHOP-12","head":"cb03493","merged":["fix/_SHOP-12/SHOP-13"],"repo":"/w/zz-ap51-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기(일부러 미통과)」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"feature/_SHOP-16/parent/SHOP-16","head":"2123d05","merged":["feature/_SHOP-16/SHOP-17"],"repo":"/w/zz-ap51-toys/jira","unmerged":[]}}}"#,
    "zz-ap51-judge": #"{"repos":{"/w/zz-ap51-toys/shop":{}},"stories":{}}"#,
    "zz-ap51-long": #"{"repos":{"/w/zz-ap51-toys/blog":{}},"stories":{}}"#,
    "zz-ap51-wt": #"{"repos":{"/w/zz-ap51-toys/todo":{}},"stories":{}}"#,
    "zz-ap52-p1": #"{"discarded":[{"repo":"/w/zz-ap52-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/todo":{}},"stories":{}}"#,
    "zz-ap52-pj": #"{"discarded":[{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/jira":{}},"stories":{"SHOP-12":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":["fix/_SHOP-12/SHOP-13"],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]},"SHOP-20":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#,
    "zz-ap53-cpu": #"{"discarded":[{"repo":"/w/zz-ap53-toys/notes","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap53-toys/notes":{}},"stories":{}}"#,
    "zz-ap53-inplace": #"{"promoted":[{"at":1793151315.679,"into":"dev","repo":"/w/shop/shop-core","source":"autopilot"},{"at":1793151315.779,"into":"dev","repo":"/w/shop/shop-web","source":"autopilot"},{"at":1793151466.062,"into":"live","repo":"/w/shop/shop-core","source":"autopilot"},{"at":1793151466.171,"into":"live","repo":"/w/shop/shop-web","source":"autopilot"},{"at":1793151469.738,"into":"master","repo":"/w/shop/shop-core","source":"branch:live"},{"at":1793151469.847,"into":"master","repo":"/w/shop/shop-web","source":"branch:live"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{}}"#,
    "zz-ap53-jira": #"{"discarded":[{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"autopilot"}],"promoted":[{"at":1793151645.249,"into":"dev","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-16"},{"at":1793151648.79,"into":"live","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-16"},{"at":1793151683.177,"into":"dev","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793151708.494,"into":"live","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793151757.678,"into":"master","repo":"/w/zz-ap53-toys/jira","source":"branch:live"}],"repos":{"/w/zz-ap53-toys/jira":{}},"stories":{"SHOP-12":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-12/parent/SHOP-12","head":"e9f07ed","merged":["fix/_SHOP-12/SHOP-13"],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"feature/_SHOP-16/parent/SHOP-16","head":"d15100a","merged":["feature/_SHOP-16/SHOP-17","feature/_SHOP-16/SHOP-18"],"repo":"/w/zz-ap53-toys/jira","unmerged":[]},"SHOP-20":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-20/parent/SHOP-20","head":"3034f74","merged":[],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#,
    "zz-ap53-wt": #"{"discarded":[{"repo":"/w/zz-ap53-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap53-toys/todo":{}},"stories":{}}"#,
    "zz-ap53-wt2": #"{"promoted":[{"at":1793151235.151,"into":"dev","repo":"/w/zz-ap53-toys/notes","source":"autopilot"},{"at":1793151246.682,"into":"live","repo":"/w/zz-ap53-toys/notes","source":"autopilot"},{"at":1793151250.104,"into":"master","repo":"/w/zz-ap53-toys/notes","source":"branch:live"}],"repos":{"/w/zz-ap53-toys/notes":{}},"stories":{}}"#,
    "zz-ap53-wt3": #"{"discarded":[{"repo":"/w/zz-ap53-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap53-toys/todo":{}},"stories":{}}"#,
    "zz-cpu-ap": #"{"discarded":[{"repo":"/w/cpu/toy2","source":"autopilot"}],"promoted":[],"repos":{"/w/cpu/toy2":{}},"stories":{}}"#,
    "zz-cpu-ap2": #"{"discarded":[{"repo":"/w/cpu/toy2","source":"autopilot"}],"promoted":[],"repos":{"/w/cpu/toy2":{}},"stories":{}}"#,
    "zz-cpu-run": #"{"discarded":[{"repo":"/w/cpu/toy","source":"autopilot"}],"promoted":[],"repos":{"/w/cpu/toy":{}},"stories":{}}"#,
    "zz-merge-gui": #"{"discarded":[{"repo":"/w/toy/repo2","source":"autopilot"}],"promoted":[],"repos":{"/w/toy/repo2":{}},"stories":{}}"#,
    "zz-mr-before": #"{"discarded":[{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"story:SHOP-1"},{"repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"repo":"/w/shop/shop-web","source":"story:SHOP-1"},{"repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"},{"repo":"/w/shop/shop-core","source":"story:SHOP-6"},{"repo":"/w/shop/shop-core","source":"story:SHOP-6"}],"promoted":[],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-1":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"3afdae8","merged":[],"repo":"/w/shop/shop-web","unmerged":[{"task":"features/SHOP-3","why":"미통과"},{"task":"features/SHOP-9","why":"미통과"}]},"SHOP-4":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"879eaea","merged":["features/SHOP-5"],"repo":"/w/shop/shop-web","unmerged":[]},"SHOP-6":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"repo":"/w/shop/shop-core","unmerged":[{"task":"features/SHOP-7","why":"미통과"},{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}}"#,
    "zz-mr-jira": #"{"discarded":[{"repo":"/w/shop/shop-core","source":"story:SHOP-6"},{"repo":"/w/shop/shop-core","source":"story:SHOP-6"},{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"}],"failed":[{"at":1793155257.15,"into":"live","op":"merge","repo":"/w/shop/shop-web","source":"story:SHOP-1"}],"promoted":[{"at":1793155244.736,"into":"dev","repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"at":1793155245.016,"into":"dev","repo":"/w/shop/shop-web","source":"story:SHOP-1"},{"at":1793155256.808,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"at":1793155260.138,"into":"master","repo":"/w/shop/shop-core","source":"branch:live"},{"at":1793155260.138,"into":"master","repo":"/w/shop/shop-web","source":"branch:live"},{"at":1793155263.43,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"at":1793155263.672,"into":"live","repo":"/w/shop/shop-web","source":"story:SHOP-1"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-1":{"repos":{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-1","head":"9b14bde","merged":["features/SHOP-2","features/SHOP-9"],"unmerged":[]},"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"4ebe25d","merged":["features/SHOP-3"],"unmerged":[]}}},"SHOP-4":{"repos":{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"1a34fd1","merged":["features/SHOP-5"],"unmerged":[]}}},"SHOP-6":{"repos":{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"unmerged":[{"task":"features/SHOP-7","why":"미통과"}]},"/w/shop/shop-web":{"branch":"features/parent/SHOP-6","merged":[],"unmerged":[{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}}}}"#,
    "zz-mr-jira2": #"{"discarded":[{"repo":"/w/shop/shop-core","source":"story:SHOP-21"},{"repo":"/w/shop/shop-web","source":"story:SHOP-21"},{"repo":"/w/shop/shop-core","source":"story:SHOP-21"},{"repo":"/w/shop/shop-web","source":"story:SHOP-21"},{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"story:SHOP-14"},{"repo":"/w/shop/shop-web","source":"story:SHOP-14"}],"failed":[{"at":1793156028.982,"into":"live","op":"merge","repo":"/w/shop/shop-web","source":"story:SHOP-11"}],"promoted":[{"at":1793156001.433,"into":"dev","repo":"/w/shop/shop-core","source":"story:SHOP-11"},{"at":1793156001.545,"into":"dev","repo":"/w/shop/shop-web","source":"story:SHOP-11"},{"at":1793156028.713,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-11"},{"at":1793156061.999,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-11"},{"at":1793156062.09,"into":"live","repo":"/w/shop/shop-web","source":"story:SHOP-11"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-11":{"repos":{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-11","head":"5b7764b","merged":["features/SHOP-12"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-11","head":"8fad31a","merged":["features/SHOP-13"],"unmerged":[]}}},"SHOP-14":{"repos":{"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-14","head":"f6f9134","merged":["features/SHOP-15"],"unmerged":[]}}},"SHOP-21":{"repos":{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-21","head":"85d9f53","merged":["features/SHOP-22"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-21","head":"378b7b1","merged":["features/SHOP-23"],"unmerged":[]}}}}}"#,
    "zz-mr-jira3": #"{"discarded":[{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"}],"promoted":[{"at":1793156997.223,"into":"dev","repo":"/w/shop/shop-core","source":"story:SHOP-41"},{"at":1793156997.496,"into":"dev","repo":"/w/shop/shop-web","source":"story:SHOP-41"},{"at":1793157001.213,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-41"},{"at":1793157001.467,"into":"live","repo":"/w/shop/shop-web","source":"story:SHOP-41"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-41":{"repos":{"/w/shop/shop-core":{"base":"5b7764ba22717d93f035be308bb17e0ffa1d935b","branch":"features/parent/SHOP-41","head":"c9f4b55","merged":["features/SHOP-42"],"unmerged":[]},"/w/shop/shop-web":{"base":"8fad31a3a517151a6bb69037cc95afff17c28990","branch":"features/parent/SHOP-41","head":"a672aac","merged":["features/SHOP-43"],"unmerged":[]}}}}}"#,
    "zz-mr-old": #"{"discarded":[{"repo":"/w/zz-mr-toys/solo","source":"autopilot"}],"promoted":[{"at":1793156173.444,"into":"dev","repo":"/w/zz-mr-toys/solo","source":"story:SHOP-31"},{"at":1793156181.645,"into":"live","repo":"/w/zz-mr-toys/solo","source":"story:SHOP-31"}],"repos":{"/w/zz-mr-toys/solo":{}},"stories":{"SHOP-31":{"base":"695135148dec74c4cd5647391f6e5225380f3924","branch":"features/parent/SHOP-31","head":"c1ceaa4","merged":["features/SHOP-32","features/SHOP-33"],"repo":"/w/zz-mr-toys/solo","unmerged":[]}}}"#,
]

// MARK: - 합치기·승격에 남은 것이 있는가 (apMergeOpen — 보드 본체의 끝난 실행 줄 "합치기 기다림")

struct OpenRow: Row {
    let note: String
    let branches: String    // branches JSON, 또는 "toy:<실행 id>"
    let steps: String
    let withAutopilot: Bool
    let open: Bool
}

private let openRows: [OpenRow] = [
    .init(note: "가지 기록이 비었음 → 자율주행 가지가 남음", branches: #"{}"#, steps: "", withAutopilot: true, open: true),
    .init(note: "스토리가 없으면 withAutopilot 이 거짓이어도 자율주행 가지를 넣는다", branches: #"{}"#, steps: "", withAutopilot: false, open: true),
    .init(note: "자율주행 가지를 버림 → 남은 것 없음", branches: #"{"discarded":[{"repo":"/r/a","source":"autopilot"}]}"#, steps: "", withAutopilot: true, open: false),
    .init(note: "승격 차례가 없으면 어디든 합친 줄이 있으면 닿음", branches: #"{"promoted":[{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "", withAutopilot: true, open: false),
    .init(note: "마지막 자율주행 단계(dev)에 합침 → 남은 것 없음", branches: #"{"promoted":[{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev", withAutopilot: true, open: false),
    .init(note: "dev 에만 합쳤는데 마지막 자율주행 단계는 live → 남음", branches: #"{"promoted":[{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev autopilot>live@dev", withAutopilot: true, open: true),
    .init(note: "live 에 합쳤는데 기존 가지끼리의 마지막 단계(live → master)를 안 함 → 남음", branches: #"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: true),
    .init(note: "live 와 master 까지 → 남은 것 없음", branches: #"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":20,"into":"master","repo":"/r/a","source":"branch:live"}]}"#, steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: false),
    .init(note: "버려서 닿은 합칠 쪽이 없으면 master 단계는 보지 않는다", branches: #"{"discarded":[{"repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live live>master@live", withAutopilot: true, open: false),
    .init(note: "한 저장소만 합치고 다른 저장소는 못 함(failed) → 단계가 끝나지 않음", branches: #"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live", withAutopilot: true, open: true),
    .init(note: "못 한 뒤 같은 저장소를 다시 합침(at 이 뒤) → 끝남", branches: #"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":12,"into":"live","repo":"/r/b","source":"autopilot"}]}"#, steps: "autopilot>live", withAutopilot: true, open: false),
    .init(note: "다시 합친 시각이 못 한 시각과 같아도 끝남(>=)", branches: #"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":11,"into":"live","repo":"/r/b","source":"autopilot"}]}"#, steps: "autopilot>live", withAutopilot: true, open: false),
    .init(note: "못 하기 전에 합친 줄은 다시 합친 것으로 세지 않는다", branches: #"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":5,"into":"live","repo":"/r/b","source":"autopilot"},{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live", withAutopilot: true, open: true),
    .init(note: "merge·promote 가 아닌 못 한 일(cleanup)은 단계를 막지 않는다", branches: #"{"failed":[{"at":11,"into":"live","op":"cleanup","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live", withAutopilot: true, open: false),
    .init(note: "시각이 없으면 0 으로 보고 견준다(다시 합친 것으로 셈)", branches: #"{"failed":[{"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"into":"live","repo":"/r/a","source":"autopilot"},{"into":"live","repo":"/r/b","source":"autopilot"}]}"#, steps: "autopilot>live", withAutopilot: true, open: false),
    .init(note: "master 올리기를 한 저장소만 못 함 → 남음", branches: #"{"failed":[{"at":21,"into":"master","op":"promote","repo":"/r/b","source":"branch:live"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":20,"into":"master","repo":"/r/a","source":"branch:live"}]}"#, steps: "autopilot>live live>master@live", withAutopilot: true, open: true),
    .init(note: "Jira: 한 스토리는 버리고 한 스토리는 live 까지 → 남은 것 없음", branches: #"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{},"S2":{}}}"#, steps: "autopilot>live", withAutopilot: false, open: false),
    .init(note: "Jira: 스토리 하나가 아직 → 남음", branches: #"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{},"S2":{}}}"#, steps: "autopilot>live", withAutopilot: false, open: true),
    .init(note: "스토리는 합쳤지만 withAutopilot 이라 자율주행 가지가 남음", branches: #"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S1"}],"stories":{"S1":{}}}"#, steps: "autopilot>live", withAutopilot: true, open: true),
    .init(note: "stories 가 빈 객체면 자율주행 가지를 넣는다", branches: #"{"stories":{}}"#, steps: "", withAutopilot: false, open: true),
    .init(note: "stories 가 객체가 아니면 스토리 없음으로 본다", branches: #"{"promoted":[{"at":1,"into":"dev","repo":"/r/a","source":"autopilot"}],"stories":["S1"]}"#, steps: "", withAutopilot: false, open: false),
    .init(note: "자율주행 단계 없는 차례(live → master 뿐): 합친 줄은 닿음, master 를 안 함 → 남음", branches: #"{"promoted":[{"at":1,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "live>master", withAutopilot: true, open: true),
    .init(note: "자율주행 단계 없는 차례에서 master 까지 함 → 남은 것 없음", branches: #"{"promoted":[{"at":1,"into":"dev","repo":"/r/a","source":"autopilot"},{"at":2,"into":"master","repo":"/r/a","source":"branch:live"}]}"#, steps: "live>master", withAutopilot: true, open: false),
    .init(note: "source 없는 버린 줄은 아무것도 버리지 않는다", branches: #"{"discarded":[{"repo":"/r/a"}]}"#, steps: "", withAutopilot: true, open: true),
    .init(note: "discarded 가 배열이 아니면 버린 것 없음", branches: #"{"discarded":{"source":"autopilot"}}"#, steps: "", withAutopilot: true, open: true),
    .init(note: "into 없는 합친 줄은 빈 가지로 읽혀 dev 단계에 닿지 않음", branches: #"{"promoted":[{"repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev", withAutopilot: true, open: true),
    .init(note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:20261027-1909-zz-ap52-todo", steps: "autopilot>dev autopilot>live@dev live>main@live", withAutopilot: true, open: false),
    .init(note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:20261027-1918-zz-ap52-jira", steps: "autopilot>dev autopilot>live@dev live>main@live", withAutopilot: false, open: false),
    .init(note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:20261028-1043-zz-ap53-jira", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap51-inplace(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-inplace", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 보드가 넘기는 값: 차례 0단계 · 자율주행 가지 뺌", branches: "toy:zz-ap51-jira", steps: "", withAutopilot: false, open: true),
    .init(note: "장난감 zz-ap51-judge(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-judge", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-long(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-long", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-wt(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-wt", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap52-p1", steps: "autopilot>dev autopilot>live@dev live>main@live", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-ap52-pj", steps: "autopilot>dev autopilot>live@dev live>main@live", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-cpu", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-inplace", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-ap53-jira", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-wt", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-wt2", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-wt3", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: true, open: false),
    .init(note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-cpu-ap", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-cpu-ap2", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-cpu-run(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-cpu-run", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-before", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira2", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: true),
    .init(note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira3", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: true),
    .init(note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-old", steps: "autopilot>dev autopilot>live@dev live>master@live", withAutopilot: false, open: true),
    .init(note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:20261027-1909-zz-ap52-todo", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:20261027-1909-zz-ap52-todo", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:20261027-1918-zz-ap52-jira", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:20261027-1918-zz-ap52-jira", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:20261028-1043-zz-ap53-jira", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:20261028-1043-zz-ap53-jira", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 merge-check-1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:merge-check-1", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 merge-check-1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:merge-check-1", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap51-inplace(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-inplace", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-inplace(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-inplace", steps: "", withAutopilot: false, open: true),
    .init(note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-jira", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-jira", steps: "", withAutopilot: false, open: true),
    .init(note: "장난감 zz-ap51-judge(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-judge", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-judge(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-judge", steps: "", withAutopilot: false, open: true),
    .init(note: "장난감 zz-ap51-long(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-long", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-long(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-long", steps: "", withAutopilot: false, open: true),
    .init(note: "장난감 zz-ap51-wt(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-wt", steps: "", withAutopilot: true, open: true),
    .init(note: "장난감 zz-ap51-wt(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-wt", steps: "", withAutopilot: false, open: true),
    .init(note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap52-p1", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap52-p1", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap52-pj", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap52-pj", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-cpu", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-cpu", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-inplace", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-inplace", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-jira", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-jira", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-wt", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-wt", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-wt2", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-wt2", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-wt3", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-wt3", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-cpu-ap", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-cpu-ap", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-cpu-ap2", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-cpu-ap2", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-cpu-run(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-cpu-run", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-cpu-run(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-cpu-run", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-merge-gui(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-merge-gui", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-merge-gui(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-merge-gui", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-before", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-before", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-jira", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-jira2", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira2", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-jira3", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira3", steps: "", withAutopilot: false, open: false),
    .init(note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-old", steps: "", withAutopilot: true, open: false),
    .init(note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-old", steps: "", withAutopilot: false, open: false),
]

@Suite("자율주행 합치기 상태: 남은 단계") struct MergeOpenTests {
    @Test("버리지도 마지막 자율주행 단계에 합치지도 않은 합칠 쪽, 또는 안 한 마지막 기존 가지 단계가 있으면 남았다", arguments: openRows)
    func 표(_ r: OpenRow) {
        let br = r.branches.hasPrefix("toy:") ? object(toys[String(r.branches.dropFirst(4))] ?? "") : object(r.branches)
        #expect(!br.isEmpty || r.branches == "{}", "장난감 기록을 찾지 못했다")
        #expect(apMergeOpen(br, steps: steps(r.steps), stories: r.withAutopilot) == r.open)
    }
}

// MARK: - 지난 실행 목록의 합치기 상태 (apMergeMark)

struct MarkRow: Row {
    let note: String
    let branches: String    // branches JSON, 또는 "toy:<실행 id>"
    let mark: APMergeMark?
    let label: String?
}

private let markRows: [MarkRow] = [
    .init(note: "repos 가 없으면 가지 기록 없는 실행(nil)", branches: #"{"promoted":[{"into":"dev","source":"autopilot"}]}"#, mark: nil, label: nil),
    .init(note: "repos 가 비었으면 버린 줄이 있어도 nil", branches: #"{"discarded":[{"source":"autopilot"}],"repos":{}}"#, mark: nil, label: nil),
    .init(note: "repos 가 객체가 아니면 nil", branches: #"{"repos":["/r/a"]}"#, mark: nil, label: nil),
    .init(note: "가지는 있고 합친 줄·버린 줄이 없으면 합치기 기다림", branches: #"{"repos":{"/r/a":{}}}"#, mark: .waiting, label: "합치기 기다림"),
    .init(note: "빈 promoted·discarded 도 합치기 기다림", branches: #"{"discarded":[],"promoted":[],"repos":{"/r/a":{}}}"#, mark: .waiting, label: "합치기 기다림"),
    .init(note: "버린 줄이 있으면 버림", branches: #"{"discarded":[{"repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .discarded, label: "버림"),
    .init(note: "지금 동작: 버린 스토리 하나만 있어도 master 까지 올린 실행 전체가 버림(5-3b ②)", branches: #"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"promoted":[{"into":"master","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .discarded, label: "버림"),
    .init(note: "discarded 의 줄 꼴은 보지 않는다(무엇이든 하나 있으면 버림)", branches: #"{"discarded":["x"],"repos":{"/r/a":{}}}"#, mark: .discarded, label: "버림"),
    .init(note: "discarded 가 배열이 아니면 버린 것 없음", branches: #"{"discarded":{"source":"autopilot"},"repos":{"/r/a":{}}}"#, mark: .waiting, label: "합치기 기다림"),
    .init(note: "dev 에 합침", branches: #"{"promoted":[{"into":"dev","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .promoted("dev"), label: "dev 에 합침"),
    .init(note: "마지막으로 받은 가지가 master 면 까지 올림", branches: #"{"promoted":[{"into":"dev","repo":"/r/a","source":"autopilot"},{"into":"master","repo":"/r/a","source":"branch:dev"}],"repos":{"/r/a":{}}}"#, mark: .promoted("master"), label: "master 까지 올림"),
    .init(note: "main 도 까지 올림", branches: #"{"promoted":[{"into":"main","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .promoted("main"), label: "main 까지 올림"),
    .init(note: "마지막은 promoted 의 차례로 고른다(시각이 아니다)", branches: #"{"promoted":[{"at":20,"into":"master","repo":"/r/a","source":"autopilot"},{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .promoted("dev"), label: "dev 에 합침"),
    .init(note: "into 가 없는 줄은 건너뛰고 그 앞 줄", branches: #"{"promoted":[{"into":"live","repo":"/r/a","source":"autopilot"},{"repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .promoted("live"), label: "live 에 합침"),
    .init(note: "into 가 있는 줄이 없으면 합치기 기다림", branches: #"{"promoted":[{"repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .waiting, label: "합치기 기다림"),
    .init(note: "가지 이름은 대소문자를 가린다(Master 는 에 합침)", branches: #"{"promoted":[{"into":"Master","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: .promoted("Master"), label: "Master 에 합침"),
    .init(note: "여러 저장소 스토리: 한 저장소의 스토리를 버렸으면 실행 전체가 버림(지금 동작)", branches: #"{"discarded":[{"repo":"/r/b","source":"story:S1"}],"promoted":[{"into":"dev","repo":"/r/a","source":"story:S1"},{"into":"live","repo":"/r/a","source":"story:S1"}],"repos":{"/r/a":{},"/r/b":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1","merged":["T1","T2"]},"/r/b":{"branch":"f/S1","unmerged":["T3"]}}}}}"#, mark: .discarded, label: "버림"),
    .init(note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1)", branches: "toy:20261027-1909-zz-ap52-todo", mark: .discarded, label: "버림"),
    .init(note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7)", branches: "toy:20261027-1918-zz-ap52-jira", mark: .discarded, label: "버림"),
    .init(note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3)", branches: "toy:20261028-1043-zz-ap53-jira", mark: .discarded, label: "버림"),
    .init(note: "장난감 merge-check-1(저장소 1·버린 줄 1)", branches: "toy:merge-check-1", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-ap51-inplace(저장소 1)", branches: "toy:zz-ap51-inplace", mark: .waiting, label: "합치기 기다림"),
    .init(note: "장난감 zz-ap51-jira(저장소 1·스토리 2)", branches: "toy:zz-ap51-jira", mark: .waiting, label: "합치기 기다림"),
    .init(note: "장난감 zz-ap51-judge(저장소 1)", branches: "toy:zz-ap51-judge", mark: .waiting, label: "합치기 기다림"),
    .init(note: "장난감 zz-ap51-long(저장소 1)", branches: "toy:zz-ap51-long", mark: .waiting, label: "합치기 기다림"),
    .init(note: "장난감 zz-ap51-wt(저장소 1)", branches: "toy:zz-ap51-wt", mark: .waiting, label: "합치기 기다림"),
    .init(note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1)", branches: "toy:zz-ap52-p1", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8)", branches: "toy:zz-ap52-pj", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1)", branches: "toy:zz-ap53-cpu", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6)", branches: "toy:zz-ap53-inplace", mark: .promoted("master"), label: "master 까지 올림"),
    .init(note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3)", branches: "toy:zz-ap53-jira", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1)", branches: "toy:zz-ap53-wt", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3)", branches: "toy:zz-ap53-wt2", mark: .promoted("master"), label: "master 까지 올림"),
    .init(note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1)", branches: "toy:zz-ap53-wt3", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1)", branches: "toy:zz-cpu-ap", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1)", branches: "toy:zz-cpu-ap2", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-cpu-run(저장소 1·버린 줄 1)", branches: "toy:zz-cpu-run", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-merge-gui(저장소 1·버린 줄 1)", branches: "toy:zz-merge-gui", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10)", branches: "toy:zz-mr-before", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6)", branches: "toy:zz-mr-jira", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8)", branches: "toy:zz-mr-jira2", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2)", branches: "toy:zz-mr-jira3", mark: .discarded, label: "버림"),
    .init(note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1)", branches: "toy:zz-mr-old", mark: .discarded, label: "버림"),
]

@Suite("자율주행 합치기 상태: 지난 실행 표지") struct MergeMarkTests {
    @Test("가지 기록 없음은 nil, 버린 줄이 있으면 버림, 합친 줄이 있으면 마지막으로 받은 가지, 아니면 합치기 기다림", arguments: markRows)
    func 표(_ r: MarkRow) {
        let br = r.branches.hasPrefix("toy:") ? object(toys[String(r.branches.dropFirst(4))] ?? "") : object(r.branches)
        #expect(!br.isEmpty, "장난감 기록을 찾지 못했다")
        let m = apMergeMark(br)
        #expect(m == r.mark)
        #expect(m?.label == r.label)
    }
}

// MARK: - 스토리 기록 읽기 (apStoryRepos — 엔진·보드가 함께 쓴다)

struct StoryRow: Row {
    let note: String
    let story: String       // stories.<키> 의 JSON("nil" 은 값 없음), 또는 "toy:<실행 id>:<스토리 키>"
    let repos: String       // 결과를 키 차례 JSON 으로
}

private let storyRows: [StoryRow] = [
    .init(note: "값 없음 → 빈 결과", story: #"nil"#, repos: #"{}"#),
    .init(note: "객체가 아님 → 빈 결과", story: #""x""#, repos: #"{}"#),
    .init(note: "빈 객체 → 빈 결과", story: #"{}"#, repos: #"{}"#),
    .init(note: "옛 꼴인데 repo 가 빈 글자 → 빈 결과", story: #"{"branch":"f/S1","repo":""}"#, repos: #"{}"#),
    .init(note: "옛 꼴(스토리 하나에 저장소 하나)은 repo 를 떼고 그 저장소 하나로", story: #"{"base":"live","branch":"f/S1","head":"h1","merged":["T1"],"repo":"/r/a","unmerged":[]}"#, repos: #"{"/r/a":{"base":"live","branch":"f/S1","head":"h1","merged":["T1"],"unmerged":[]}}"#),
    .init(note: "새 꼴(repos)은 저장소마다 그대로", story: #"{"repos":{"/r/a":{"branch":"f/S1","head":"h1"},"/r/b":{"branch":"f/S1"}}}"#, repos: #"{"/r/a":{"branch":"f/S1","head":"h1"},"/r/b":{"branch":"f/S1"}}"#),
    .init(note: "새 꼴에서 객체가 아닌 저장소 칸은 뺀다", story: #"{"repos":{"/r/a":{"branch":"f/S1"},"/r/b":"x"}}"#, repos: #"{"/r/a":{"branch":"f/S1"}}"#),
    .init(note: "repos 가 빈 객체면 repo 가 있어도 빈 결과(새 꼴이 먼저)", story: #"{"repo":"/r/a","repos":{}}"#, repos: #"{}"#),
    .init(note: "repos 가 객체가 아니면 옛 꼴로 읽어 repos 칸까지 그대로 남긴다", story: #"{"branch":"f/S1","repo":"/r/a","repos":"x"}"#, repos: #"{"/r/a":{"branch":"f/S1","repos":"x"}}"#),
    .init(note: "장난감 20261027-1918-zz-ap52-jira 스토리 SHOP-12(옛 꼴)", story: "toy:20261027-1918-zz-ap52-jira:SHOP-12", repos: #"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":[],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 20261027-1918-zz-ap52-jira 스토리 SHOP-16(옛 꼴)", story: "toy:20261027-1918-zz-ap52-jira:SHOP-16", repos: #"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]}}"#),
    .init(note: "장난감 20261027-1918-zz-ap52-jira 스토리 SHOP-20(옛 꼴)", story: "toy:20261027-1918-zz-ap52-jira:SHOP-20", repos: #"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"#),
    .init(note: "장난감 20261028-1043-zz-ap53-jira 스토리 SHOP-12(옛 꼴)", story: "toy:20261028-1043-zz-ap53-jira:SHOP-12", repos: #"{"/w/zz-ap53-toys/jira":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-12/parent/SHOP-12","head":"68142a1","merged":[],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 20261028-1043-zz-ap53-jira 스토리 SHOP-20(옛 꼴)", story: "toy:20261028-1043-zz-ap53-jira:SHOP-20", repos: #"{"/w/zz-ap53-toys/jira":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-20/parent/SHOP-20","head":"68142a1","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"#),
    .init(note: "장난감 zz-ap51-jira 스토리 SHOP-12(옛 꼴)", story: "toy:zz-ap51-jira:SHOP-12", repos: #"{"/w/zz-ap51-toys/jira":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"fix/_SHOP-12/parent/SHOP-12","head":"cb03493","merged":["fix/_SHOP-12/SHOP-13"],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기(일부러 미통과)」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 zz-ap51-jira 스토리 SHOP-16(옛 꼴)", story: "toy:zz-ap51-jira:SHOP-16", repos: #"{"/w/zz-ap51-toys/jira":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"feature/_SHOP-16/parent/SHOP-16","head":"2123d05","merged":["feature/_SHOP-16/SHOP-17"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-ap52-pj 스토리 SHOP-12(옛 꼴)", story: "toy:zz-ap52-pj:SHOP-12", repos: #"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":["fix/_SHOP-12/SHOP-13"],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 zz-ap52-pj 스토리 SHOP-16(옛 꼴)", story: "toy:zz-ap52-pj:SHOP-16", repos: #"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]}}"#),
    .init(note: "장난감 zz-ap52-pj 스토리 SHOP-20(옛 꼴)", story: "toy:zz-ap52-pj:SHOP-20", repos: #"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"#),
    .init(note: "장난감 zz-ap53-jira 스토리 SHOP-12(옛 꼴)", story: "toy:zz-ap53-jira:SHOP-12", repos: #"{"/w/zz-ap53-toys/jira":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-12/parent/SHOP-12","head":"e9f07ed","merged":["fix/_SHOP-12/SHOP-13"],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 zz-ap53-jira 스토리 SHOP-16(옛 꼴)", story: "toy:zz-ap53-jira:SHOP-16", repos: #"{"/w/zz-ap53-toys/jira":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"feature/_SHOP-16/parent/SHOP-16","head":"d15100a","merged":["feature/_SHOP-16/SHOP-17","feature/_SHOP-16/SHOP-18"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-ap53-jira 스토리 SHOP-20(옛 꼴)", story: "toy:zz-ap53-jira:SHOP-20", repos: #"{"/w/zz-ap53-toys/jira":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-20/parent/SHOP-20","head":"3034f74","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"#),
    .init(note: "장난감 zz-mr-before 스토리 SHOP-1(옛 꼴)", story: "toy:zz-mr-before:SHOP-1", repos: #"{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"3afdae8","merged":[],"unmerged":[{"task":"features/SHOP-3","why":"미통과"},{"task":"features/SHOP-9","why":"미통과"}]}}"#),
    .init(note: "장난감 zz-mr-before 스토리 SHOP-4(옛 꼴)", story: "toy:zz-mr-before:SHOP-4", repos: #"{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"879eaea","merged":["features/SHOP-5"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-before 스토리 SHOP-6(옛 꼴)", story: "toy:zz-mr-before:SHOP-6", repos: #"{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"unmerged":[{"task":"features/SHOP-7","why":"미통과"},{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 zz-mr-jira 스토리 SHOP-1(새 꼴)", story: "toy:zz-mr-jira:SHOP-1", repos: #"{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-1","head":"9b14bde","merged":["features/SHOP-2","features/SHOP-9"],"unmerged":[]},"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"4ebe25d","merged":["features/SHOP-3"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-jira 스토리 SHOP-4(새 꼴)", story: "toy:zz-mr-jira:SHOP-4", repos: #"{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"1a34fd1","merged":["features/SHOP-5"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-jira 스토리 SHOP-6(새 꼴)", story: "toy:zz-mr-jira:SHOP-6", repos: #"{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"unmerged":[{"task":"features/SHOP-7","why":"미통과"}]},"/w/shop/shop-web":{"branch":"features/parent/SHOP-6","merged":[],"unmerged":[{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}"#),
    .init(note: "장난감 zz-mr-jira2 스토리 SHOP-11(새 꼴)", story: "toy:zz-mr-jira2:SHOP-11", repos: #"{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-11","head":"5b7764b","merged":["features/SHOP-12"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-11","head":"8fad31a","merged":["features/SHOP-13"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-jira2 스토리 SHOP-14(새 꼴)", story: "toy:zz-mr-jira2:SHOP-14", repos: #"{"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-14","head":"f6f9134","merged":["features/SHOP-15"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-jira2 스토리 SHOP-21(새 꼴)", story: "toy:zz-mr-jira2:SHOP-21", repos: #"{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-21","head":"85d9f53","merged":["features/SHOP-22"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-21","head":"378b7b1","merged":["features/SHOP-23"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-jira3 스토리 SHOP-41(새 꼴)", story: "toy:zz-mr-jira3:SHOP-41", repos: #"{"/w/shop/shop-core":{"base":"5b7764ba22717d93f035be308bb17e0ffa1d935b","branch":"features/parent/SHOP-41","head":"c9f4b55","merged":["features/SHOP-42"],"unmerged":[]},"/w/shop/shop-web":{"base":"8fad31a3a517151a6bb69037cc95afff17c28990","branch":"features/parent/SHOP-41","head":"a672aac","merged":["features/SHOP-43"],"unmerged":[]}}"#),
    .init(note: "장난감 zz-mr-old 스토리 SHOP-31(옛 꼴)", story: "toy:zz-mr-old:SHOP-31", repos: #"{"/w/zz-mr-toys/solo":{"base":"695135148dec74c4cd5647391f6e5225380f3924","branch":"features/parent/SHOP-31","head":"c1ceaa4","merged":["features/SHOP-32","features/SHOP-33"],"unmerged":[]}}"#),
]

@Suite("자율주행 가지 기록: 스토리 저장소별") struct StoryReposTests {
    @Test("새 꼴 repos 는 저장소마다, 옛 꼴 repo 는 그 저장소 하나로", arguments: storyRows)
    func 표(_ r: StoryRow) {
        let v: Any?
        if r.story.hasPrefix("toy:") {
            let p = r.story.dropFirst(4).split(separator: ":", maxSplits: 1).map(String.init)
            v = (object(toys[p[0]] ?? "")["stories"] as? [String: Any])?[p[1]]
            #expect(v != nil, "장난감 기록을 찾지 못했다")
        } else {
            v = r.story == "nil" ? nil : try? JSONSerialization.jsonObject(with: Data(r.story.utf8), options: .fragmentsAllowed)
        }
        #expect(text(apStoryRepos(v)) == r.repos)
    }
}
