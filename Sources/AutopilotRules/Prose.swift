// 자율주행 규칙(AutopilotRules) — 모델이 쓴 글 다듬기(2026-10-30 3단계에서 보드 Autopilot.swift 에서 옮김).
// 보드 화면과 보고서 틀(Report.swift)이 함께 쓴다. 엔진 고침 1차에서 "모델이 결과 기록에 쓴 긴 줄표는 보고서를 그릴 때 바꾼다"로 미룬 일의 규칙이다
import Foundation

// 모델이 쓴 글의 긴 줄표는 보드에 그릴 때 쉼표로 바꾼다(설계: 보고서를 그릴 때 바꾼다, 사용자에게 보이는 글에는 긴 줄표를 쓰지 않는다)
public func apPlain(_ s: String) -> String {
    s.replacingOccurrences(of: " — ", with: ", ").replacingOccurrences(of: "—", with: ", ")
}
