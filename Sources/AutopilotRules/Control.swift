// 자율주행 규칙(AutopilotRules) — 사람이 끼어드는 요청 control.json 의 판정(2026-10-30 2단계 5-4 에서 엔진 readControl·pollControl·pausedLoop·
// cmdControl 과 보드 AutopilotStore.send 에서 옮김). 파일 읽기·지우기·run.json 의 control_seen 쓰기·상태 바꾸기는 엔진에 남는다.
// 꼴은 설계 5장 2절: {"action":"pause|resume|stop|takeover|giveback|restart_from_intake","ts"}. 보드·엔진 control 명령이 쓰고 엔진이 1초마다 본다
import Foundation

// 끝난 실행의 상태 — 이 실행에는 control.json 을 쓰지 않는다(보드·엔진 control 명령)
public let apEndedStates: Set<String> = ["done", "stopped", "error"]
public func apRunEnded(_ state: String) -> Bool { apEndedStates.contains(state) }

// 보드·엔진 control 명령이 쓰는 요청과 사람이 읽는 이름
public let apControlActions = ["pause", "resume", "stop"]
public let apControlNames = ["pause": "일시정지", "resume": "재개", "stop": "중단"]

// 보드가 이 요청을 그 실행에 써도 되는가 — 아는 요청이고, 출발한(상태가 있는) 끝나지 않은 실행만
public func apBoardCanControl(_ action: String, state: String) -> Bool {
    apControlActions.contains(action) && !state.isEmpty && !apRunEnded(state)
}

// control.json 하나가 새 요청인가 — action 이 글자일 때만, ts 가 없으면 파일 수정 시각, 이미 받은 ts(control_seen) 보다 클 때만.
// 새 요청이면 (action, ts), 아니면 nil(엔진은 그래도 파일을 지운다 — 다시 뜬 엔진이 같은 요청을 또 하지 않게)
public func apControlNew(_ json: Any?, mtime: Double, seen: Double?) -> (action: String, ts: Double)? {
    let c = json as? [String: Any]
    guard let a = c?["action"] as? String else { return nil }
    let ts = (c?["ts"] as? NSNumber)?.doubleValue ?? (c?["ts"] as? String).flatMap { Double($0) } ?? mtime
    guard ts > seen ?? 0 else { return nil }
    return (a, ts)
}

// 받은 요청으로 엔진이 할 일 — 도는 중(paused false)과 일시정지 중(true)
public enum APControlStep: Equatable, Sendable {
    case pause      // 지금 하던 일(claude·판정·대기)을 멈추고 일시정지로
    case stop       // 실행을 마친다(사람이 중단)
    case resume     // 일시정지를 풀고 이어 간다
    case none       // 그대로 간다(진행 기록 한 줄만)
}
public struct APControlOutcome: Equatable, Sendable {
    public let step: APControlStep
    public let text: String?    // 진행 기록(events.jsonl, who board) 한 줄. nil 이면 엔진이 그 일을 하며 따로 적는다
    public init(_ step: APControlStep, _ text: String?) { self.step = step; self.text = text }
}
public func apControlOutcome(_ action: String, paused: Bool) -> APControlOutcome {
    if paused {
        switch action {
        case "resume": return .init(.resume, "재개합니다. 멈춘 자리에서 같은 세션으로 이어 갑니다.")
        case "stop": return .init(.stop, nil)
        case "pause": return .init(.none, "이미 일시정지 상태입니다.")
        default: return .init(.none, "아직 엔진이 하지 못하는 요청(\(action))이라 넘깁니다.")
        }
    }
    switch action {
    case "pause": return .init(.pause, "일시정지 요청을 받았습니다.")
    case "stop": return .init(.stop, "중단 요청을 받았습니다.")
    case "resume": return .init(.none, "재개 요청을 받았지만 일시정지 상태가 아니라 그대로 갑니다.")
    default: return .init(.none, "아직 엔진이 하지 못하는 요청(\(action))이라 넘깁니다.")
    }
}
