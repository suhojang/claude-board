// 자율주행 규칙(AutopilotRules) — 보드 지난 실행 화면의 판정(2026-10-29 5-3b 에서 보드 Autopilot.swift 에서 옮김).
// 기록 저장소는 읽지 않는다. 보드가 읽은 실행 목록(최근 것부터)을 값으로 받는다
import Foundation

// 지난 실행 목록의 실행 하나 — 판정에 쓰는 칸만(id, 프로젝트 묶음 이름)
public struct APRunTag: Equatable, Sendable {
    public let id: String
    public let project: String
    public init(id: String, project: String) { self.id = id; self.project = project }
}

// 상세에 보일 실행. selected 는 사람이 고른 실행(nil 이면 고르지 않음), runs 는 최근 것부터, project 는 거른 프로젝트(nil 이면 모든 프로젝트).
// 거른 목록 안에서 고른다(5-3b ⑥): 고른 실행이 거른 목록에 있으면 그것, 아니면 거른 목록의 첫 실행(거른 목록이 비면 없음)
public func apHistoryPick(selected: String?, runs: [APRunTag], project: String?) -> String? {
    let shown = runs.filter { project == nil || $0.project == project }
    if let s = selected, shown.contains(where: { $0.id == s }) { return s }
    return shown.first?.id
}
