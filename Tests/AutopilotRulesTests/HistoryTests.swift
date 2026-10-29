// 지난 실행 화면 규칙(Sources/AutopilotRules/History.swift) 시험.
// 2026-10-29 5-3b 에서 보드 readHistory 에서 옮길 때 옮기기 전 식(46d9a0f)과 같은 입력(목록 넷 × 고른 것 넷 × 거른 프로젝트 둘)으로 돌린 덤프가 같았다.
// 같은 차례에 거른 목록 안에서 고르게 고친 줄(5-3b ⑥)은 note 에 적었다
import Testing
import AutopilotRules

struct PickRow: Row {
    let note: String
    let runs: [(String, String)]   // (id, 프로젝트) 최근 것부터
    let selected: String?
    let project: String?
    let want: String?
}

private let pickRows: [PickRow] = [
    .init(note: "목록이 비었고 고른 것도 없음 → 없음", runs: [], selected: nil, project: nil, want: nil),
    .init(note: "고른 것이 없으면 첫 실행", runs: [("r2", "a"), ("r1", "b")], selected: nil, project: nil, want: "r2"),
    .init(note: "고른 실행", runs: [("r2", "a"), ("r1", "b")], selected: "r1", project: nil, want: "r1"),
    .init(note: "5-3b ⑥: 프로젝트를 거르면 거른 목록의 첫 실행(전에는 전체 목록의 첫 실행)", runs: [("r2", "a"), ("r1", "b")], selected: nil, project: "b", want: "r1"),
    .init(note: "5-3b ⑥: 고른 실행이 거른 목록에 없으면 거른 목록의 첫 실행", runs: [("r3", "a"), ("r2", "b"), ("r1", "b")], selected: "r3", project: "b", want: "r2"),
    .init(note: "고른 실행이 거른 목록에 있으면 그것", runs: [("r3", "a"), ("r2", "b"), ("r1", "b")], selected: "r1", project: "b", want: "r1"),
    .init(note: "거른 목록이 비면 없음", runs: [("r2", "a")], selected: nil, project: "c", want: nil),
    .init(note: "5-3b ⑥: 목록에 없는 고른 실행은 첫 실행으로(전에는 그대로 두어 상세가 비었다)", runs: [("r2", "a")], selected: "zz", project: nil, want: "r2"),
]

@Suite("자율주행 지난 실행: 상세에 보일 실행") struct HistoryPickTests {
    @Test("거른 목록 안에서 고른 실행, 없으면 거른 목록의 첫 실행", arguments: pickRows)
    func 표(_ r: PickRow) {
        #expect(apHistoryPick(selected: r.selected, runs: r.runs.map { APRunTag(id: $0.0, project: $0.1) }, project: r.project) == r.want)
    }
}
