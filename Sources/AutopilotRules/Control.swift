// 자율주행 규칙(AutopilotRules) — 사람이 끼어드는 요청 control.json 의 판정(2026-10-30 2단계 5-4 에서 엔진 readControl·pollControl·pausedLoop·
// cmdControl 과 보드 AutopilotStore.send 에서 옮김). 파일 읽기·지우기·run.json 의 control_seen 쓰기·상태 바꾸기는 엔진에 남는다.
// 꼴은 설계 5장 2절: {"action":"pause|resume|stop|takeover|giveback|restart_from_intake","ts"}. 보드·엔진 control 명령이 쓰고 엔진이 1초마다 본다
import Foundation

// 끝난 실행의 상태 — 이 실행에는 control.json 을 쓰지 않는다(보드·엔진 control 명령)
public let apEndedStates: Set<String> = ["done", "stopped", "error"]
public func apRunEnded(_ state: String) -> Bool { apEndedStates.contains(state) }

// 보드·엔진 control 명령이 쓰는 요청과 사람이 읽는 이름. restart_from_intake(멈추고 다시 출발)는 2단계 5-4 에서 더함
public let apControlActions = ["pause", "resume", "stop", "restart_from_intake"]
public let apControlNames = ["pause": "일시정지", "resume": "재개", "stop": "중단", "restart_from_intake": "멈추고 다시 출발"]

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
    case restart    // 멈추고 다시 출발 — 언제 멈출지는 apRestartPlan(5-4)
    case none       // 그대로 간다(진행 기록 한 줄만)
}
public struct APControlOutcome: Equatable, Sendable {
    public let step: APControlStep
    public let text: String?    // 진행 기록(events.jsonl, who board) 한 줄. nil 이면 엔진이 그 일을 하며 따로 적는다
    public init(_ step: APControlStep, _ text: String?) { self.step = step; self.text = text }
}
public func apControlOutcome(_ action: String, paused: Bool) -> APControlOutcome {
    if action == "restart_from_intake" { return .init(.restart, nil) }
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

// MARK: - 멈추고 다시 출발(2단계 5-4, 설계 머리말 "목표를 바꾸려면 멈추고 다시 출발")

// 받았을 때 언제 멈추는가 — 지금 단위(일시정지·한도 대기 중인 단위도)가 있으면 그 단위를 마친 뒤(새 단위는 시작하지 않음), 없으면(단위 사이·멈출 선 대기·
// 시험 환경 대기) 바로. 일시정지 중이면 기다린다(재개하면 그 단위를 마치고 멈춤, 바로 멈추려면 중단 — 사람이 멈춘 일을 엔진이 스스로 다시 돌리지 않게).
// 이미 받은 요청이면 다시 하지 않는다
public enum APRestartPlan: Equatable, Sendable {
    case afterUnit(Int)
    case now
    case already
}
public func apRestartPlan(statuses: [String], asked: Bool) -> APRestartPlan {
    if asked { return .already }
    if let i = statuses.firstIndex(of: "running") { return .afterUnit(i + 1) }
    return .now
}
// 진행 기록 한 줄(who board)
public func apRestartText(_ p: APRestartPlan, paused: Bool, unit: Int?) -> String {
    switch p {
    case .afterUnit(let n):
        return paused ? "멈추고 다시 출발 요청을 받았습니다. 일시정지 중이라 재개하면 그 단위(단위 \(n))를 마친 뒤 멈춥니다(바로 멈추려면 중단합니다). 새 단위는 시작하지 않습니다."
                      : "멈추고 다시 출발 요청을 받았습니다. 지금 단위(단위 \(n))를 마친 뒤 멈추고, 새 단위는 시작하지 않습니다."
    case .now: return "멈추고 다시 출발 요청을 받았습니다. 도는 단위가 없어 바로 멈춥니다."
    case .already: return "멈추고 다시 출발 요청을 이미 받았습니다\(unit.map { "(그 단위(단위 \($0))를 마친 뒤 멈춤)" } ?? "")."
    }
}
// 엔진이 새 단위를 고를 때: 요청을 받았고 남은(대기) 단위가 있으면 시작하지 않고 멈춘다. 남은 단위가 없으면 보통처럼 끝낸다(끝남, 표지는 남음)
public func apRestartStopsBefore(asked: Bool, hasPending: Bool) -> Bool { asked && hasPending }
// 멈추고 다시 출발로 멈춘 실행의 끝 상태 이유 — 상태는 stopped(보드·finish·기록 저장소가 끝난 상태로 다룬다), 이유 머리로 가른다
public let apRestartReason = "멈추고 다시 출발: 사람이 요청해 멈췄습니다. 보드에서 이 실행을 이어 가는 출발 준비를 다시 엽니다."
public func apIsRestartStop(state: String, reason: String) -> Bool { state == "stopped" && reason.hasPrefix("멈추고 다시 출발") }
// 보드가 출발 준비를 저절로 다시 열 때 — 이 보드가 요청을 보냈고(asked), 실행이 끝나 엔진이 끝 정리(기록 저장소 쓰기)를 마쳤고, 엔진이 요청을 받았고(flag),
// 아직 열지 않았을 때 한 번. 이어 가기는 프로젝트가 있는 실행만(출발 준비가 기록 저장소의 프로젝트로 지난 실행을 고른다)
public func apReopenIntake(state: String, wrapped: Bool, flag: Bool, asked: Bool, opened: Bool, project: String?) -> Bool {
    asked && !opened && flag && wrapped && apRunEnded(state) && !(project ?? "").isEmpty
}
