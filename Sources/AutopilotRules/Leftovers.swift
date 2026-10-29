// 자율주행 규칙(AutopilotRules) — 단위가 끝난 자리에 남은 변경을 어떻게 할지(엔진 switchTo, 2026-10-29 5-3b 에서 엔진 main.swift 에서 옮김)와
// 읽기 전용 자리에 남은 변경의 보관(2026-10-28 사용자 결정 "엔진이 따로 보관(git stash)하고 이어 감", 5-3b ①).
// git 은 부르지 않는다. 엔진이 읽은 작업 트리 상태 줄(git status --porcelain)과 출발 때 상태 줄을 값으로 받는다
import Foundation

// 가지를 바꾸기 전(또는 판정 전) 작업 트리에 남은 변경을 어떻게 하는가
public enum APLeftover: Equatable, Sendable {
    case clean                  // 출발 때와 같다 — 그대로 바꾼다
    case commit                 // 이 실행이 만든 가지에 엔진이 커밋해 둔다(다음 작업 가지에 섞이지 않게)
    case stash([String]?)       // 읽기 전용으로 걸어 둔 스토리 가지: 엔진이 따로 보관(git stash)하고 이어 간다. nil 이면 모두, 아니면 그 경로만
                                // (출발 때부터 있던 사람의 변경은 남긴다)
    case refuse(APLeftoverWhy)  // 바꾸지 않는다(그 단위를 건너뛴다)
}

public enum APLeftoverWhy: Equatable, Sendable {
    case readOnly   // 읽기 전용 자리인데 출발 때부터 있던 변경과 갈라 보관할 수 없다(같은 파일·이름 바꿈·따옴표 경로·없어진 변경)
    case dirty      // 출발 때부터 변경이 있었거나 이 실행이 만든 가지가 아니다
}

// dirty·dirtyStart 는 git status --porcelain 의 줄(엔진 porcelain), onStoryBranch 는 지금 걸린 가지가 이 실행의 스토리 가지인가(같은 스토리의
// 다른 저장소 작업 동안 읽기 전용으로 걸어 둔 자리), madeByRun 은 지금 걸린 가지를 이 실행이 만들었는가.
// 스토리 가지에는 엔진도 커밋하지 않는다(스토리에는 판정을 통과한 작업만 합친다) — 대신 보관해 판정·다음 단위에 들어가지 않게 한다
public func apLeftover(dirty: [String], dirtyStart: [String], onStoryBranch: Bool, madeByRun: Bool) -> APLeftover {
    guard dirty != dirtyStart else { return .clean }
    if onStoryBranch {
        if dirtyStart.isEmpty { return .stash(nil) }
        return apStashPaths(dirty: dirty, dirtyStart: dirtyStart).map { .stash($0) } ?? .refuse(.readOnly)
    }
    guard dirtyStart.isEmpty, madeByRun else { return .refuse(.dirty) }
    return .commit
}

// 출발 때부터 변경이 있던 자리에서 새로 생긴 변경의 경로만 — 갈라 낼 수 없으면 nil.
// 상태 줄은 "XY 경로"(XY 두 글자 뒤 빈칸)다. 이름 바꿈·복사(R·C, "옛 -> 새")와 따옴표로 싼 경로(특수 글자)는 pathspec 으로 정확히 고를 수 없어 갈라 내지 않는다.
// 출발 때 있던 줄이 없어졌거나 새 줄이 출발 때 바뀐 파일과 같은 파일이면 보관하면 사람의 변경까지 가져가므로 갈라 내지 않는다
public func apStashPaths(dirty: [String], dirtyStart: [String]) -> [String]? {
    let have = Set(dirty)
    guard dirtyStart.allSatisfy(have.contains) else { return nil }
    func path(_ line: String) -> String? {
        guard line.count > 3 else { return nil }
        let xy = line.prefix(2), p = String(line.dropFirst(3))
        if xy.contains("R") || xy.contains("C") || p.hasPrefix("\"") || p.isEmpty { return nil }
        return p
    }
    let startPaths = dirtyStart.compactMap(path)
    var out: [String] = []
    for line in dirty where !dirtyStart.contains(line) {
        guard let p = path(line) else { return nil }
        // 같은 파일이거나 한쪽이 다른 쪽의 폴더(추적 안 하는 폴더 "?? dir/")면 갈라 낼 수 없다
        if startPaths.contains(where: { s in s == p || (s.hasSuffix("/") && p.hasPrefix(s)) || (p.hasSuffix("/") && s.hasPrefix(p)) }) { return nil }
        out.append(p)
    }
    return out.isEmpty ? nil : out
}

// 보관(git stash) 메시지 — 실행 id·단위·저장소·가지(사용자 결정). git stash list 에 "On <가지>: <메시지>" 로 보인다
public func apStashMessage(run: String, unit: Int?, repo: String, branch: String) -> String {
    "자율주행 \(run) 단위 \(unit.map(String.init) ?? "?") · \(repo): 읽기 전용으로 걸어 둔 \(branch) 에 남은 변경(엔진이 보관)"
}

// 보관한 변경을 누구 몫으로 적을까 — 판정 전이면 그 단위, 단위를 시작하기 전·끝 정리 때는 그 앞에서 마지막으로 돈 단위.
// statuses 는 대기열 차례대로의 단위 상태(엔진 queue.json status), before 는 이제 시작할 단위 번호(nil 이면 끝 정리, 모든 단위를 본다).
// 대기·건너뜀은 돌지 않았으므로 세지 않는다
public func apStashUnit(statuses: [String], before: Int?) -> Int? {
    let upTo = min(before.map { $0 - 1 } ?? statuses.count, statuses.count)
    guard upTo > 0 else { return nil }
    return (0..<upTo).last { !["pending", "skipped"].contains(statuses[$0]) }.map { $0 + 1 }
}
