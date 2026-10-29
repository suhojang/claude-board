// 자율주행 규칙(AutopilotRules) — 단위가 끝난 자리에 남은 변경을 어떻게 할지(엔진 switchTo, 2026-10-29 5-3b 에서 엔진 main.swift 에서 옮김).
// git 은 부르지 않는다. 엔진이 읽은 작업 트리 상태 줄(git status --porcelain)과 출발 때 상태 줄을 값으로 받는다
import Foundation

// 가지를 바꾸기 전에 작업 트리에 남은 변경을 어떻게 하는가
public enum APLeftover: Equatable, Sendable {
    case clean                  // 출발 때와 같다 — 그대로 바꾼다
    case commit                 // 이 실행이 만든 가지에 엔진이 커밋해 둔다(다음 작업 가지에 섞이지 않게)
    case refuse(APLeftoverWhy)  // 바꾸지 않는다(그 단위를 건너뛴다)
}

public enum APLeftoverWhy: Equatable, Sendable {
    case readOnly   // 읽기 전용으로 걸어 둔 스토리 가지에 남은 변경 — 스토리에는 판정을 통과한 작업만 합치므로 엔진도 커밋하지 않는다
    case dirty      // 출발 때부터 변경이 있었거나 이 실행이 만든 가지가 아니다
}

// dirty·dirtyStart 는 git status --porcelain 의 줄(엔진 porcelain), onStoryBranch 는 지금 걸린 가지가 이 실행의 스토리 가지인가,
// madeByRun 은 지금 걸린 가지를 이 실행이 만들었는가
public func apLeftover(dirty: [String], dirtyStart: [String], onStoryBranch: Bool, madeByRun: Bool) -> APLeftover {
    guard dirty != dirtyStart else { return .clean }
    if onStoryBranch { return .refuse(.readOnly) }
    guard dirtyStart.isEmpty, madeByRun else { return .refuse(.dirty) }
    return .commit
}
