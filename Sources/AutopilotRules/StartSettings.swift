// 자율주행 규칙(AutopilotRules) — 출발 확인의 설정 줄(모델·멈출 선)을 그 자리에서 바꾸기(설계 2장: 연결·알림·모델·멈출 선·승격 차례는
// 프로젝트 설정에 두고 실행마다 묻지 않는다, 출발 확인에 한 줄씩 보여 그 자리에서 바꾸게 한다). 2026-10-30 2단계 5-2b 에서 새로 씀.
// 고를 수 있는 값(객관식)과 확인, 출발 준비가 쓸 값의 차례(프로젝트 설정 → 지난 실행 → 기본)를 정한다. 파일은 보드가 읽고 쓴다
import Foundation

// 모델 — 엔진은 run.json 의 model 을 claude --model 에 그대로 넘긴다. 고르는 것은 별명 셋과 지금 값(목록 밖이면 앞에 둔다)
public let apModelChoices = ["opus", "sonnet", "haiku"]
public func apModelOptions(current: String) -> [String] {
    current.isEmpty || apModelChoices.contains(current) ? apModelChoices : [current] + apModelChoices
}

// 멈출 선(백분율) — 주간·5시간. 고르는 것은 정해 둔 값과 지금 값. 기본은 주간 80%·5시간 95%(설계 판단, 설계 머리말)
public let apStopWeekChoices = [60, 70, 80, 90]
public let apStopFiveChoices = [80, 90, 95]
public func apStopOptions(_ choices: [Int], current: Int) -> [Int] { (choices.contains(current) ? choices : choices + [current]).sorted() }

// 바꾼 설정의 고칠 것 — 모델은 영문 소문자·숫자·.-_[] (claude --model 에 넘길 꼴), 멈출 선은 1~100%
public func apSettingsProblems(model: String, week: Double, five: Double) -> [String] {
    var out: [String] = []
    if model.isEmpty { out.append("모델이 비었습니다") }
    else if model.range(of: "^[a-z0-9][a-z0-9._\\[\\]-]*$", options: .regularExpression) == nil { out.append("모델 이름 꼴이 아닙니다: \(model)") }
    for (n, v) in [("주간", week), ("5시간", five)] where !(v > 0 && v <= 1) { out.append("멈출 선 \(n) 값은 1~100% 입니다") }
    return out
}

// 출발 준비가 쓸 모델·멈출 선 — 프로젝트 설정(출발 확인에서 바꾼 값, projects/<프로젝트>.json 의 model·stopline)이 있으면 그것,
// 없으면 지난 실행(이어 가기)의 값, 그것도 없으면 nil(보드 기본). 어디서 왔는지도 돌려준다(설정 줄에 보인다)
public struct APStartPick: Equatable, Sendable {
    public var model: String?
    public var week: Double?
    public var five: Double?
    public var from: String          // project·past·default
    public init(model: String?, week: Double?, five: Double?, from: String) { self.model = model; self.week = week; self.five = five; self.from = from }
}
public func apStartSettings(project: [String: Any], past: [String: Any]) -> APStartPick {
    func line(_ o: [String: Any]) -> (Double?, Double?) {
        let s = o["stopline"] as? [String: Any] ?? [:]
        return ((s["seven_day"] as? NSNumber)?.doubleValue, (s["five_hour"] as? NSNumber)?.doubleValue)
    }
    let pm = (project["model"] as? String).flatMap { $0.isEmpty ? nil : $0 }, (pw, pf) = line(project)
    if pm != nil || pw != nil || pf != nil {
        let (qw, qf) = line(past)
        return APStartPick(model: pm ?? (past["model"] as? String), week: pw ?? qw, five: pf ?? qf, from: "project")
    }
    let (qw, qf) = line(past)
    let qm = (past["model"] as? String).flatMap { $0.isEmpty ? nil : $0 }
    return APStartPick(model: qm, week: qw, five: qf, from: qm != nil || qw != nil || qf != nil ? "past" : "default")
}
