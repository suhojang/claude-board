// 단위가 끝난 자리에 남은 변경 규칙(Sources/AutopilotRules/Leftovers.swift) 시험.
// 2026-10-29 5-3b 에서 엔진 switchTo 에서 옮길 때, 옮기기 전 판정(46d9a0f)과 옮긴 함수를 같은 입력(상태 줄 다섯 꼴 × 출발 때 다섯 꼴 × 스토리 가지 × 만든 가지,
// 200줄)으로 돌린 덤프가 모두 같았다. 아래는 그 가운데 갈래마다 고른 줄이다. 기대값은 지금 동작 그대로다
import Testing
import AutopilotRules

struct LeftoverRow: Row {
    let note: String
    let dirty: [String]
    let start: [String]
    let story: Bool
    let made: Bool
    let want: APLeftover
}

private let leftoverRows: [LeftoverRow] = [
    .init(note: "변경 없음 → 그대로", dirty: [], start: [], story: false, made: true, want: .clean),
    .init(note: "출발 때와 같은 변경 → 그대로(스토리 가지여도)", dirty: [" M a.txt"], start: [" M a.txt"], story: true, made: false, want: .clean),
    .init(note: "이 실행이 만든 가지에 새 변경, 출발 때 깨끗 → 커밋", dirty: ["?? b.txt"], start: [], story: false, made: true, want: .commit),
    .init(note: "이 실행이 만든 가지가 아님 → 바꾸지 않음", dirty: ["?? b.txt"], start: [], story: false, made: false, want: .refuse(.dirty)),
    .init(note: "출발 때부터 변경이 있었고 더 바뀜 → 바꾸지 않음", dirty: [" M a.txt", "?? b.txt"], start: [" M a.txt"], story: false, made: true, want: .refuse(.dirty)),
    .init(note: "줄 차례만 달라도 다른 것으로 본다", dirty: ["?? b.txt", " M a.txt"], start: [" M a.txt", "?? b.txt"], story: false, made: true, want: .refuse(.dirty)),
    .init(note: "출발 때 있던 변경이 없어짐 → 바꾸지 않음", dirty: [], start: [" M a.txt"], story: false, made: true, want: .refuse(.dirty)),
    .init(note: "지금 동작: 읽기 전용 스토리 가지에 새 변경 → 바꾸지 않음(뒤 단위 건너뜀, 5-3b ①)", dirty: ["?? stray.txt"], start: [], story: true, made: true, want: .refuse(.readOnly)),
    .init(note: "읽기 전용 스토리 가지는 만든 가지인지 보지 않는다", dirty: [" M a.txt"], start: [], story: true, made: false, want: .refuse(.readOnly)),
]

@Suite("자율주행 남은 변경: 가지를 바꾸기 전") struct LeftoverTests {
    @Test("출발 때와 같으면 그대로, 이 실행이 만든 가지면 커밋, 읽기 전용 자리·그 밖은 바꾸지 않음", arguments: leftoverRows)
    func 표(_ r: LeftoverRow) {
        #expect(apLeftover(dirty: r.dirty, dirtyStart: r.start, onStoryBranch: r.story, madeByRun: r.made) == r.want)
    }
}
