// 단위가 끝난 자리에 남은 변경 규칙(Sources/AutopilotRules/Leftovers.swift) 시험.
// 2026-10-29 5-3b 에서 엔진 switchTo 에서 옮길 때, 옮기기 전 판정(46d9a0f)과 옮긴 함수를 같은 입력(상태 줄 다섯 꼴 × 출발 때 다섯 꼴 × 스토리 가지 × 만든 가지,
// 200줄)으로 돌린 덤프가 모두 같았다. 아래는 그 가운데 갈래마다 고른 줄이다. 같은 차례에 읽기 전용 자리를 보관(git stash)으로 고친 줄은
// note 에 "5-3b ①" 과 까닭을 적었다
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
    .init(note: "5-3b ①: 읽기 전용 스토리 가지에 새 변경, 출발 때 깨끗 → 모두 보관하고 이어 감(전에는 바꾸지 않아 뒤 단위를 건너뜀)", dirty: ["?? stray.txt"], start: [], story: true, made: true, want: .stash(nil)),
    .init(note: "읽기 전용 스토리 가지는 만든 가지인지 보지 않는다", dirty: [" M a.txt"], start: [], story: true, made: false, want: .stash(nil)),
    .init(note: "5-3b ①: 출발 때부터 사람의 변경이 있으면 새 변경의 경로만 보관(사람의 변경은 남김)", dirty: [" M a.txt", "?? stray.txt", " M src/b.txt"], start: [" M a.txt"], story: true, made: true, want: .stash(["stray.txt", "src/b.txt"])),
    .init(note: "5-3b ①: 사람이 바꾼 파일을 모델이 더 바꿈(같은 파일) → 갈라 낼 수 없어 바꾸지 않음", dirty: ["MM a.txt"], start: [" M a.txt"], story: true, made: true, want: .refuse(.readOnly)),
    .init(note: "5-3b ①: 출발 때 있던 변경이 없어짐 → 보관으로 되돌릴 수 없어 바꾸지 않음", dirty: ["?? stray.txt"], start: [" M a.txt"], story: true, made: true, want: .refuse(.readOnly)),
]

@Suite("자율주행 남은 변경: 가지를 바꾸기 전") struct LeftoverTests {
    @Test("출발 때와 같으면 그대로, 이 실행이 만든 가지면 커밋, 읽기 전용 자리·그 밖은 바꾸지 않음", arguments: leftoverRows)
    func 표(_ r: LeftoverRow) {
        #expect(apLeftover(dirty: r.dirty, dirtyStart: r.start, onStoryBranch: r.story, madeByRun: r.made) == r.want)
    }
}

// MARK: - 새 변경의 경로 (apStashPaths — 출발 때부터 변경이 있던 읽기 전용 자리)

struct StashPathsRow: Row {
    let note: String
    let dirty: [String]
    let start: [String]
    let want: [String]?
}

private let stashPathsRows: [StashPathsRow] = [
    .init(note: "새 줄의 경로만, 상태 줄 차례대로", dirty: ["?? z.txt", " M a.txt", " D old.txt"], start: [" M a.txt"], want: ["z.txt", "old.txt"]),
    .init(note: "추적 안 하는 폴더(dir/)도 경로 그대로", dirty: [" M a.txt", "?? notes/"], start: [" M a.txt"], want: ["notes/"]),
    .init(note: "새 변경이 사람이 만든 추적 안 하는 폴더 안 → 갈라 낼 수 없음", dirty: ["?? mine/", "?? mine/x.txt"], start: ["?? mine/"], want: nil),
    .init(note: "새 폴더가 사람의 파일을 품음 → 갈라 낼 수 없음", dirty: [" M d/a.txt", "?? d/"], start: [" M d/a.txt"], want: nil),
    .init(note: "이름 바꿈(R) → 갈라 낼 수 없음", dirty: [" M a.txt", "R  b.txt -> c.txt"], start: [" M a.txt"], want: nil),
    .init(note: "복사(C) → 갈라 낼 수 없음", dirty: [" M a.txt", "C  b.txt -> c.txt"], start: [" M a.txt"], want: nil),
    .init(note: "따옴표로 싼 경로(특수 글자) → 갈라 낼 수 없음", dirty: [" M a.txt", "?? \"sp ace\\t.txt\""], start: [" M a.txt"], want: nil),
    .init(note: "출발 때 줄이 없어짐 → 갈라 낼 수 없음", dirty: ["?? z.txt"], start: [" M a.txt"], want: nil),
    .init(note: "새 줄이 없음 → 없음(보관할 것 없음)", dirty: [" M a.txt"], start: [" M a.txt"], want: nil),
    .init(note: "짧은 줄(꼴이 아님) → 갈라 낼 수 없음", dirty: [" M a.txt", "??"], start: [" M a.txt"], want: nil),
]

@Suite("자율주행 남은 변경: 보관할 새 경로") struct StashPathsTests {
    @Test("출발 때부터 있던 변경과 겹치지 않는 새 줄의 경로만, 겹치거나 고를 수 없으면 nil", arguments: stashPathsRows)
    func 표(_ r: StashPathsRow) {
        #expect(apStashPaths(dirty: r.dirty, dirtyStart: r.start) == r.want)
    }
}

// MARK: - 보관 메시지와 누구 몫 (apStashMessage·apStashUnit)

struct StashUnitRow: Row {
    let note: String
    let statuses: [String]
    let before: Int?
    let want: Int?
}

private let stashUnitRows: [StashUnitRow] = [
    .init(note: "단위 3 을 시작하기 전 → 앞에서 마지막으로 돈 단위 2", statuses: ["done", "failed", "pending"], before: 3, want: 2),
    .init(note: "건너뛴 단위는 돌지 않았으므로 세지 않는다", statuses: ["done", "skipped", "pending"], before: 3, want: 1),
    .init(note: "첫 단위 앞 → 없음", statuses: ["pending", "pending"], before: 1, want: nil),
    .init(note: "끝 정리(before 없음) → 모든 단위 가운데 마지막으로 돈 단위", statuses: ["done", "capped", "skipped"], before: nil, want: 2),
    .init(note: "중단·도는 중도 돈 단위", statuses: ["done", "stopped"], before: nil, want: 2),
    .init(note: "before 가 대기열보다 크면 끝까지 본다", statuses: ["done"], before: 5, want: 1),
    .init(note: "빈 대기열 → 없음", statuses: [], before: nil, want: nil),
]

@Suite("자율주행 남은 변경: 보관 기록") struct StashRecordTests {
    @Test("누구 몫 — 판정 전이 아니면 앞에서 마지막으로 돈 단위", arguments: stashUnitRows)
    func 단위(_ r: StashUnitRow) {
        #expect(apStashUnit(statuses: r.statuses, before: r.before) == r.want)
    }
    @Test("보관 메시지에 실행 id·단위·저장소·가지")
    func 메시지() {
        #expect(apStashMessage(run: "zz-run", unit: 3, repo: "shop-core", branch: "features/parent/SHOP-1")
                == "자율주행 zz-run 단위 3 · shop-core: 읽기 전용으로 걸어 둔 features/parent/SHOP-1 에 남은 변경(엔진이 보관)")
        #expect(apStashMessage(run: "zz-run", unit: nil, repo: "web", branch: "f/S") == "자율주행 zz-run 단위 ? · web: 읽기 전용으로 걸어 둔 f/S 에 남은 변경(엔진이 보관)")
    }
}
