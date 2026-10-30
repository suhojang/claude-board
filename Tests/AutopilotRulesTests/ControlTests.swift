// 사람이 끼어드는 요청 control.json 규칙(Sources/AutopilotRules/Control.swift) 시험 — 2026-10-30 2단계 5-4.
// 옮기기 때 옮기기 전 식(b6cb0eb 의 엔진 readControl·pollControl·pausedLoop·cmdControl, 보드 send)과 같은 입력 234줄
// (control.json 꼴 열 × 받은 ts 여섯 × 파일 시각 둘, 요청 여덟 × 도는 중·일시정지, 요청 여덟 × 상태 열)의 덤프가 같았다. 표는 그 가운데 갈래마다 고른 줄.
// 같은 차례의 고치기에서 멈추고 다시 출발(restart_from_intake)을 받게 바꾼 줄은 note 에 "5-4" 를 적었다
import Foundation
import Testing
import AutopilotRules

struct ControlNewRow: Row {
    let note: String
    let json: String?      // control.json 글(nil 이면 읽지 못함)
    let mtime: Double
    let seen: Double?
    let want: String       // "action ts" 또는 "nil"
}
private let controlNewRows: [ControlNewRow] = [
    .init(note: "읽지 못한 파일은 요청이 아니다", json: nil, mtime: 9, seen: nil, want: "nil"),
    .init(note: "JSON 물체가 아니면 요청이 아니다", json: #""pause""#, mtime: 9, seen: nil, want: "nil"),
    .init(note: "action 이 글자가 아니면 요청이 아니다", json: #"{"action":3}"#, mtime: 9, seen: nil, want: "nil"),
    .init(note: "ts 가 없으면 파일 시각", json: #"{"action":"pause"}"#, mtime: 9, seen: nil, want: "pause 9.0"),
    .init(note: "ts 가 받은 것보다 크면 새 요청", json: #"{"action":"pause","ts":5.5}"#, mtime: 1, seen: 5, want: "pause 5.5"),
    .init(note: "이미 받은 ts 와 같으면 다시 하지 않는다", json: #"{"action":"pause","ts":5.5}"#, mtime: 9, seen: 5.5, want: "nil"),
    .init(note: "글자 ts 도 수로 읽는다", json: #"{"action":"stop","ts":"7"}"#, mtime: 1, seen: 6, want: "stop 7.0"),
    .init(note: "수가 아닌 글자 ts 는 파일 시각", json: #"{"action":"pause","ts":"x"}"#, mtime: 9, seen: 8, want: "pause 9.0"),
    .init(note: "받은 ts 가 없으면 0 보다 크기만 하면 된다", json: #"{"action":"resume","ts":10}"#, mtime: 1, seen: nil, want: "resume 10.0"),
]

struct OutcomeRow: Row {
    let note: String
    let action: String
    let paused: Bool
    let step: APControlStep
    let text: String?
}
private let outcomeRows: [OutcomeRow] = [
    .init(note: "도는 중 일시정지", action: "pause", paused: false, step: .pause, text: "일시정지 요청을 받았습니다."),
    .init(note: "도는 중 중단", action: "stop", paused: false, step: .stop, text: "중단 요청을 받았습니다."),
    .init(note: "도는 중 재개는 그대로", action: "resume", paused: false, step: .none, text: "재개 요청을 받았지만 일시정지 상태가 아니라 그대로 갑니다."),
    .init(note: "도는 중 모르는 요청은 넘긴다", action: "takeover", paused: false, step: .none, text: "아직 엔진이 하지 못하는 요청(takeover)이라 넘깁니다."),
    .init(note: "일시정지 중 재개", action: "resume", paused: true, step: .resume, text: "재개합니다. 멈춘 자리에서 같은 세션으로 이어 갑니다."),
    .init(note: "일시정지 중 중단은 엔진이 마치며 적는다", action: "stop", paused: true, step: .stop, text: nil),
    .init(note: "일시정지 중 일시정지", action: "pause", paused: true, step: .none, text: "이미 일시정지 상태입니다."),
    .init(note: "일시정지 중 모르는 요청", action: "giveback", paused: true, step: .none, text: "아직 엔진이 하지 못하는 요청(giveback)이라 넘깁니다."),
    .init(note: "5-4: 멈추고 다시 출발(전에는 넘김), 언제 멈출지는 apRestartPlan", action: "restart_from_intake", paused: false, step: .restart, text: nil),
    .init(note: "5-4: 일시정지 중 멈추고 다시 출발", action: "restart_from_intake", paused: true, step: .restart, text: nil),
]

struct BoardControlRow: Row {
    let note: String
    let action: String
    let state: String
    let want: Bool
}
private let boardControlRows: [BoardControlRow] = [
    .init(note: "도는 실행에 일시정지", action: "pause", state: "running", want: true),
    .init(note: "한도 대기에도 중단", action: "stop", state: "waiting_limit", want: true),
    .init(note: "일시정지한 실행에 재개", action: "resume", state: "paused", want: true),
    .init(note: "출발하지 않은(상태 없는) 실행에는 쓰지 않는다", action: "pause", state: "", want: false),
    .init(note: "끝난 실행에는 쓰지 않는다", action: "stop", state: "done", want: false),
    .init(note: "중단된 실행에는 쓰지 않는다", action: "resume", state: "stopped", want: false),
    .init(note: "모르는 요청은 쓰지 않는다", action: "takeover", state: "running", want: false),
    .init(note: "5-4: 멈추고 다시 출발(전에는 쓰지 않음)", action: "restart_from_intake", state: "waiting_stopline", want: true),
    .init(note: "5-4: 끝난 실행에는 멈추고 다시 출발도 쓰지 않는다", action: "restart_from_intake", state: "stopped", want: false),
]

@Suite("자율주행 control.json: 사람이 끼어드는 요청") struct ControlTests {
    @Test("새 요청인가", arguments: controlNewRows)
    func 새요청(_ r: ControlNewRow) {
        let j = r.json.flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8), options: [.fragmentsAllowed]) }
        let got = apControlNew(j, mtime: r.mtime, seen: r.seen).map { "\($0.action) \($0.ts)" } ?? "nil"
        #expect(got == r.want)
    }

    @Test("받은 요청으로 엔진이 할 일", arguments: outcomeRows)
    func 할일(_ r: OutcomeRow) {
        #expect(apControlOutcome(r.action, paused: r.paused) == APControlOutcome(r.step, r.text))
    }

    @Test("보드가 써도 되는가", arguments: boardControlRows)
    func 보드(_ r: BoardControlRow) { #expect(apBoardCanControl(r.action, state: r.state) == r.want) }

    @Test("끝난 상태와 요청 이름")
    func 이름() {
        #expect(["done", "stopped", "error"].allSatisfy(apRunEnded))
        #expect(!["", "starting", "running", "paused", "waiting_limit", "waiting_stopline", "waiting_precheck"].contains(where: apRunEnded))
        #expect(apControlActions.map { apControlNames[$0] ?? "" } == ["일시정지", "재개", "중단", "멈추고 다시 출발"])   // 5-4 에서 넷째
    }
}
