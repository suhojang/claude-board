// 계정 멈출 선 규칙(Sources/AutopilotRules/Stopline.swift) 시험 — 2026-10-30 6단계 ⑫-6.
// 옮기기 때 옮기기 전 식(da27a66 의 엔진 stoplineGate 의 판정·usageFrom(apiJSON:))과 같은 입력 6,578줄
// (사용률 꼴 열셋 × 초기화 시각 일곱 × 5시간 넷 × 5시간 초기화 셋 + 빈 값 둘, 멈출 선 여섯, 퍼센트 꼴 일곱 × 시각 둘)의 덤프가 같았다.
// 표는 그 가운데 갈래마다 고른 줄(시각은 UTC 로 고정, 비율은 JSONSerialization 이 쓰는 글자 그대로)
import Foundation
import Testing
import AutopilotRules

private let T = 1_793_000_000.0      // 2026-10-26 07:33:20 UTC
private let utc = TimeZone(identifier: "UTC")!

struct StopWaitRow: Row {
    let note: String
    let usage: [String: Double]?   // nil 이면 사용률을 모름
    let line: [String: Double]
    let want: String               // "go" 또는 "<until> <글>"
}
private let stopWaitRows: [StopWaitRow] = [
    .init(note: "사용률을 모르면 10분 뒤 다시", usage: nil, line: [:],
          want: "\(T + 600) 계정 사용률을 알 수 없어 새 단위를 시작하지 않고 10분 뒤 다시 확인합니다."),
    .init(note: "빈 사용량은 0 으로 보고 지나간다", usage: [:], line: [:], want: "go"),
    .init(note: "기본 멈출 선(주간 80%) 아래는 지나간다", usage: ["seven_day": 0.79, "five_hour": 0.5], line: [:], want: "go"),
    .init(note: "주간이 멈출 선과 같으면 기다린다(초기화 시각 + 2분)", usage: ["seven_day": 0.8, "seven_day_resets": T + 86_400], line: [:],
          want: "\(T + 86_400 + 120) 계정 주간 사용률 80% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 27일 07:35 까지 기다립니다."),
    .init(note: "초기화 시각이 없으면 10분 + 2분 뒤", usage: ["seven_day": 0.95], line: [:],
          want: "\(T + 720) 계정 주간 사용률 95% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 26일 07:45 까지 기다립니다."),
    .init(note: "초기화 시각이 지난 창은 0 으로 본다", usage: ["seven_day": 0.95, "seven_day_resets": T - 1], line: [:], want: "go"),
    .init(note: "초기화 시각이 지금이면 지난 것", usage: ["seven_day": 1, "seven_day_resets": T], line: [:], want: "go"),
    .init(note: "낮춘 멈출 선(30%)", usage: ["seven_day": 0.29, "seven_day_resets": T + 30], line: ["seven_day": 0.3], want: "go"),
    .init(note: "낮춘 멈출 선에 닿음", usage: ["seven_day": 0.3, "seven_day_resets": T + 30], line: ["seven_day": 0.3],
          want: "\(T + 150) 계정 주간 사용률 30% 가 멈출 선 30% 에 닿아 새 단위를 시작하지 않고 10월 26일 07:35 까지 기다립니다."),
    .init(note: "5시간 멈출 선(기본 95%)", usage: ["seven_day": 0.1, "five_hour": 0.95, "five_hour_resets": T + 3600], line: [:],
          want: "\(T + 3720) 계정 5시간 사용률 95% 가 멈출 선 95% 에 닿아 새 단위를 시작하지 않고 10월 26일 08:35 까지 기다립니다."),
    .init(note: "주간이 먼저", usage: ["seven_day": 0.9, "five_hour": 0.97], line: [:],
          want: "\(T + 720) 계정 주간 사용률 90% 가 멈출 선 80% 에 닿아 새 단위를 시작하지 않고 10월 26일 07:45 까지 기다립니다."),
    .init(note: "5시간 창이 초기화됐으면 0", usage: ["five_hour": 0.97, "five_hour_resets": T - 5], line: [:], want: "go"),
    .init(note: "멈출 선 100% 는 사용률 100% 에서만", usage: ["seven_day": 0.99], line: ["seven_day": 1.0, "five_hour": 1.0], want: "go"),
]

struct PercentRow: Row {
    let note: String
    let json: String
    let want: String
}
private let percentRows: [PercentRow] = [
    .init(note: "five_hour 가 없으면 풀지 않는다", json: #"{"seven_day":{"utilization":50}}"#, want: "nil"),
    .init(note: "빈 five_hour 는 시각과 출처만", json: #"{"five_hour":{}}"#, want: #"{"at":1793000000,"from":"cache"}"#),
    .init(note: "퍼센트를 비율로, 소수 초가 긴 시각도 읽는다", json: #"{"five_hour":{"utilization":84.0,"resets_at":"2026-10-30T15:00:00.318890+00:00"}}"#,
          want: #"{"at":1793000000,"five_hour":0.83999999999999997,"five_hour_resets":1793372400,"from":"cache"}"#),
    .init(note: "글자 퍼센트·주간 창", json: #"{"five_hour":{"utilization":"12"},"seven_day":{"utilization":29,"resets_at":"2026-11-02T00:00:00Z"}}"#,
          want: #"{"at":1793000000,"five_hour":0.12,"from":"cache","seven_day":0.28999999999999998,"seven_day_resets":1793577600}"#),
    .init(note: "못 읽는 시각은 뺀다", json: #"{"five_hour":{"utilization":3,"resets_at":"bad"}}"#, want: #"{"at":1793000000,"five_hour":0.029999999999999999,"from":"cache"}"#),
]

private func text(_ o: Any?) -> String {
    guard let o, let d = try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys]) else { return "nil" }
    return String(decoding: d, as: UTF8.self)
}

@Suite("자율주행 계정 멈출 선") struct StoplineTests {
    @Test("새 단위를 시작해도 되는가(옮긴 판정)", arguments: stopWaitRows)
    func 멈출선(_ r: StopWaitRow) {
        let got = apStoplineWait(usage: r.usage, stopline: r.line, now: T, tz: utc).map { "\($0.until) \($0.reason)" } ?? "go"
        #expect(got == r.want)
    }

    @Test("퍼센트 꼴 사용량 풀기", arguments: percentRows)
    func 퍼센트(_ r: PercentRow) {
        let j = (try? JSONSerialization.jsonObject(with: Data(r.json.utf8))) as? [String: Any] ?? [:]
        #expect(text(apUsageFromPercent(j, from: "cache", at: T)) == r.want)
    }
}
