// 자율주행 규칙 라이브러리(AutopilotRules) — 보드 자율주행 화면(Sources/ClaudeBoard/Autopilot.swift)과 엔진(Sources/ClaudeAutopilot/)이 함께
// import 하는 판정 규칙. Foundation 만 쓴다. 바깥(파일·프로세스·git·HTTP·시각·홈 폴더)은 값으로 받거나 포트(프로토콜, 생기면 Ports.swift)로만 부르고,
// 진짜 어댑터는 보드·엔진에, 가짜 어댑터는 Tests/AutopilotRulesTests 에만 둔다(BoardRules 와 같은 꼴, 이슈 #9). 이미 있는 코드는 손대는 부분부터
// 옮기고, 옮길 때는 옮기기 전 출력과 견준 입력·기대값을 표 시험으로 남긴다. 자율주행 규칙은 맨 위 rules-table/(Windows 공용 표)에 넣지 않는다.
// 이름의 AP 는 보드 자율주행 절의 이름 짓기를 그대로 따른다(보드에서 옮긴 이름을 바꾸지 않았다)
//
// 이 파일: 끝난 실행의 가지 기록(엔진 run.json 의 branches — 기록 저장소 runs.branches 도 같은 꼴)을 읽고 합치기 상태를 정하는 규칙.
// 엔진 finish 가 남긴 promoted·failed·discarded 로만 보고 git 은 부르지 않는다. 2026-10-29 보드 Autopilot.swift 에서 동작 그대로 옮겼다
// (스토리 기록 읽기 apStoryRepos 는 엔진의 같은 함수 storyRepos 도 대신한다)
import Foundation

// JSON 숫자(NSNumber)만 읽는다 — 보드 apNum 과 같다(엔진 num 은 글자로 적힌 수도 읽어 다르다)
private func apNum(_ v: Any?) -> Double? { (v as? NSNumber)?.doubleValue }

// MARK: - 가지 기록 읽기

// 승격 차례 한 단계(projects/<프로젝트>.json promotion: from 이 autopilot 이면 이 실행의 자율주행 가지)
public struct APStep: Equatable, Sendable {
    public let from: String
    public let into: String
    public var after: String?
    public init(from: String, into: String, after: String? = nil) { self.from = from; self.into = into; self.after = after }
}

// 엔진 finish 가 합친 줄(branches.promoted) — source 는 autopilot·story:<키>·branch:<가지>
public struct APPromoted: Equatable, Sendable {
    public var source, repo, branch, into, how, to: String
    public var at: Double?
    public init(source: String = "", repo: String = "", branch: String = "", into: String = "", how: String = "", to: String = "", at: Double? = nil) {
        self.source = source; self.repo = repo; self.branch = branch; self.into = into; self.how = how; self.to = to; self.at = at
    }
}

// 엔진 finish 가 저장소 하나에서 하지 못한 것(먼저 본 확인을 지난 뒤의 중간 실패, branches.failed)
public struct APFailed: Equatable, Sendable {
    public var op, source, repo, branch, into, why: String
    public var at: Double?
    public init(op: String = "", source: String = "", repo: String = "", branch: String = "", into: String = "", why: String = "", at: Double? = nil) {
        self.op = op; self.source = source; self.repo = repo; self.branch = branch; self.into = into; self.why = why; self.at = at
    }
}

// 엔진 run.json 의 branches.stories.<키> 를 저장소별로 읽는다(2026-10-28, 한 스토리가 여러 저장소에 걸침) — 엔진·보드가 함께 쓴다.
// 새 꼴 {repos: {<저장소>: {branch, base, head, merged, unmerged}}}, 옛 꼴(스토리 하나에 저장소 하나 {repo, branch, base, head, merged, unmerged},
// 2026-10-22~28 실행)은 그 저장소 하나로 본다. 저장소 기록에 head 가 없으면 그 저장소에는 스토리 가지를 만들지 않았다
public func apStoryRepos(_ s: Any?) -> [String: [String: Any]] {
    guard let s = s as? [String: Any] else { return [:] }
    if let rs = s["repos"] as? [String: Any] { return rs.compactMapValues { $0 as? [String: Any] } }
    guard let r = s["repo"] as? String, !r.isEmpty else { return [:] }
    var o = s; o["repo"] = nil
    return [r: o]
}

public func apPromotedRows(_ br: [String: Any]) -> [APPromoted] {
    (br["promoted"] as? [[String: Any]] ?? []).map {
        APPromoted(source: $0["source"] as? String ?? "", repo: $0["repo"] as? String ?? "", branch: $0["branch"] as? String ?? "",
                   into: $0["into"] as? String ?? "", how: $0["how"] as? String ?? "", to: $0["to"] as? String ?? "", at: apNum($0["at"]))
    }
}

public func apFailedRows(_ br: [String: Any]) -> [APFailed] {
    (br["failed"] as? [[String: Any]] ?? []).map {
        APFailed(op: $0["op"] as? String ?? "", source: $0["source"] as? String ?? "", repo: $0["repo"] as? String ?? "", branch: $0["branch"] as? String ?? "",
                 into: $0["into"] as? String ?? "", why: $0["why"] as? String ?? "", at: apNum($0["at"]))
    }
}

// 합치기·올리기 가운데 하지 못하고 아직 다시 하지 않은 것 — 그 뒤 같은 합칠 쪽·받는 가지·저장소로 합친 줄이 없으면 남았다
public func apUnresolved(_ promoted: [APPromoted], _ failed: [APFailed], source: String?, into: String) -> [APFailed] {
    failed.filter { f in
        ["merge", "promote"].contains(f.op) && f.into == into && (source == nil || f.source == source)
            && !promoted.contains { $0.source == f.source && $0.into == f.into && $0.repo == f.repo && ($0.at ?? 0) >= (f.at ?? 0) }
    }
}

// 단계가 끝났는가 — 엔진 finish 의 차례 규칙: 그 합칠 쪽(source 가 없으면 무엇이든)을 into 에 합친 줄이 있고, 일부 저장소만 하고 남긴 것이 없다
public func apStepDone(_ promoted: [APPromoted], _ failed: [APFailed], source: String?, into: String) -> Bool {
    promoted.contains { $0.into == into && (source == nil || $0.source == source) } && apUnresolved(promoted, failed, source: source, into: into).isEmpty
}

// MARK: - 합치기 상태

// 합칠 쪽(엔진 finish 의 source) — 스토리마다 "story:<키>", 스토리가 없거나 withAutopilot 이면 자율주행 가지 "autopilot" 도.
// 어느 저장소에도 스토리 가지를 만들지 않은 스토리(그 스토리의 첫 작업이 가지를 만들기 전에 건너뜀, 저장소 기록에 head 가 없음)는 합칠 것도
// 버릴 것도 없어 넣지 않는다 — 엔진 finish --story 의 합칠 쪽(head 있는 저장소)과 같은 기준(2026-10-29 5-3b ④)
public func apMergeSources(_ br: [String: Any], stories withAutopilot: Bool) -> [String] {
    let st = br["stories"] as? [String: Any] ?? [:]
    var sources = st.keys.sorted().filter { k in apStoryRepos(st[k]).values.contains { $0["head"] != nil } }.map { "story:" + $0 }
    if sources.isEmpty || withAutopilot { sources.insert("autopilot", at: 0) }
    return sources
}

// 버린 합칠 쪽(branches.discarded 의 source)
private func apDiscardedSources(_ br: [String: Any]) -> Set<String> {
    Set((br["discarded"] as? [[String: Any]] ?? []).compactMap { $0["source"] as? String })
}

// 합치기·승격에 남은 것이 있는가(설계 판단, 2단계 5-3) — 보드 본체의 끝난 실행 줄("합치기 기다림"). 엔진 finish 의 promoted·discarded 로만 본다.
// 합칠 쪽(자율주행 가지, Jira 면 스토리마다 — apMergeSources)이 버려지지도 승격 차례의 마지막 자율주행 단계(예 live)에 합쳐지지도 않았거나,
// 하나라도 그 단계에 합쳤는데 기존 가지끼리의 마지막 단계(예 master ← live)를 아직 안 했으면 남았다.
// withAutopilot: 스토리가 있어도 자율주행 가지를 합칠 쪽에 넣는가(보드는 Jira 실행이면 스토리 없는 단위가 있을 때, 아니면 늘 넣는다)
public func apMergeOpen(_ br: [String: Any], steps: [APStep], stories withAutopilot: Bool) -> Bool {
    let promoted = apPromotedRows(br), failed = apFailedRows(br)
    let discarded = apDiscardedSources(br)
    // 일부 저장소만 합친 단계(엔진 branches.failed 가 남고 그 뒤 그 저장소를 합치지 않음)는 끝나지 않은 것으로 본다 — 엔진 finish 와 같은 규칙
    func done(_ src: String?, _ into: String) -> Bool { apStepDone(promoted, failed, source: src, into: into) }
    let sources = apMergeSources(br, stories: withAutopilot)
    let lastAuto = steps.last { $0.from == "autopilot" }?.into
    func reached(_ s: String) -> Bool { lastAuto.map { done(s, $0) } ?? promoted.contains { $0.source == s } }
    if sources.contains(where: { !discarded.contains($0) && !reached($0) }) { return true }
    if let last = steps.last, last.from != "autopilot", sources.contains(where: reached), !done(nil, last.into) { return true }
    return false
}

// 지난 실행 목록의 합치기 상태 표지. 색은 보드가 정한다(버림 흐림·합침 초록·기다림 노랑, 섞임은 기다림이 있으면 노랑 아니면 초록)
public enum APMergeMark: Equatable, Sendable {
    case discarded          // 버림
    case promoted(String)   // 마지막으로 받은 가지
    case waiting            // 합치기 기다림
    case mixed(promoted: Int, discarded: Int, waiting: Int)   // 합칠 쪽(Jira 면 스토리)마다 결과가 다름 — 그 수

    public var label: String {
        switch self {
        case .discarded: "버림"
        case .promoted(let last): ["master", "main"].contains(last) ? "\(last) 까지 올림" : "\(last) 에 합침"
        case .waiting: "합치기 기다림"
        case .mixed(let p, let d, let w):
            [(p, "합침"), (d, "버림"), (w, "기다림")].filter { $0.0 > 0 }.map { "\($0.1) \($0.0)" }.joined(separator: " · ")
        }
    }
    // 아직 합치거나 버리지 않은 합칠 쪽이 있다(지난 실행의 "합치기·승격" 단추를 강조)
    public var hasWaiting: Bool {
        switch self {
        case .waiting: true
        case .mixed(_, _, let w): w > 0
        default: false
        }
    }
}

// 합치기 상태(설계 판단, 2026-10-29 5-3b ② 에서 합칠 쪽마다로): 합칠 쪽(apMergeSources)마다 버림·합침(합친 줄이 있음)·기다림을 정하고,
// 모두 버렸으면 버림, 모두 합쳤으면 마지막으로 받은 가지, 모두 기다리면 합치기 기다림, 섞이면 섞인 수. 가지 기록(branches.repos)이 없는 실행은 nil.
// 마지막으로 받은 가지는 promoted 의 차례로 마지막이면서 into 가 있는 줄이다(시각으로 고르지 않는다. 기존 가지끼리 올린 master 도 셈).
// withAutopilot 은 apMergeOpen 과 같다 — Jira 단위만 있는 실행이 저장소마다 남기는 쓰이지 않은 자율주행 가지를 지운 줄은 버린 합칠 쪽으로 세지 않는다
public func apMergeMark(_ br: [String: Any], stories withAutopilot: Bool) -> APMergeMark? {
    guard !((br["repos"] as? [String: Any]) ?? [:]).isEmpty else { return nil }
    let discarded = apDiscardedSources(br)
    let promoted = Set(apPromotedRows(br).filter { !$0.into.isEmpty }.map(\.source))
    var p = 0, d = 0, w = 0
    for s in apMergeSources(br, stories: withAutopilot) {
        if discarded.contains(s) { d += 1 } else if promoted.contains(s) { p += 1 } else { w += 1 }
    }
    if p == 0, w == 0 { return .discarded }
    if d == 0, w == 0, let last = (br["promoted"] as? [[String: Any]] ?? []).compactMap({ $0["into"] as? String }).last { return .promoted(last) }
    if p == 0, d == 0 { return .waiting }
    return .mixed(promoted: p, discarded: d, waiting: w)
}

// MARK: - 버리기 기록

// 엔진 finish discard 가 가지 하나를 어떻게 했는가
public enum APRefFate: Equatable, Sendable {
    case deleted(head: String)   // 지웠다(지우기 전 머리)
    case failed(String)          // 지우지 못했다(git 이 실패한 까닭)
    case missing                 // 이미 없다(보드 밖에서 지웠거나 앞서 버림)
    case foreign                 // 이 실행이 끝난 뒤 같은 이름으로 다시 만들어진 가지라 남겼다
}

// 버리기 한 가지의 기록 줄(2026-10-29 5-3b 에서 엔진 cmdFinish 에서 옮김): 지운 가지는 branches.discarded, 지우지 못한 가지는 branches.failed.
// isSource 는 그 가지가 합칠 쪽 자체(자율주행 가지, 스토리 가지)인가 — 작업 가지는 false.
// 합칠 쪽 가지가 이미 없거나(보드 밖에서 지움) 같은 이름으로 다시 만들어져 남겼어도 버린 줄을 left 와 함께 적는다(5-3b ④) — 그 합칠 쪽은
// 더 합칠 것이 없으니 끝난 것으로 보게(보드 본체의 "합치기 기다림"이 사라짐). 작업 가지는 지운 것만 적는다.
// recorded 는 그 가지(합칠 쪽·저장소·가지)에 버린 줄이 이미 있는가 — 있으면 없음·남김을 다시 적지 않는다(버리기를 다시 눌러도 줄이 늘지 않게)
public func apDiscardRows(source: String, repo: String, branch: String, fate: APRefFate, isSource: Bool, recorded: Bool = false,
                          at: Double) -> (discarded: [String: Any]?, failed: [String: Any]?) {
    switch fate {
    case .deleted(let h): return (["source": source, "repo": repo, "branch": branch, "head": h, "at": at], nil)
    case .failed(let why): return (nil, ["op": "discard", "source": source, "repo": repo, "branch": branch, "why": why, "at": at])
    case .missing: return (isSource && !recorded ? ["source": source, "repo": repo, "branch": branch, "left": "missing", "at": at] : nil, nil)
    case .foreign: return (isSource && !recorded ? ["source": source, "repo": repo, "branch": branch, "left": "foreign", "at": at] : nil, nil)
    }
}

// MARK: - 합치기·승격 시트

// 단계의 "출발 가지에만 있던 커밋 n개가 함께 갑니다" 경고를 셀 단계인가(보드 합치기·승격 시트, 2026-10-29 5-3b 에서 보드 readGit 에서 옮김).
// 세면 받는 가지에 없는 출발 머리(출발 때 출발 가지 머리)의 커밋 수(받는 가지..출발 머리)를 보인다.
// 이 실행의 가지(자율주행·스토리 가지)를 합치는 단계에 앞 단계(after)가 있고 그 가지가 출발 가지일 때만 — 이 실행의 가지가 출발 가지에서
// 갈라져 출발 가지의 커밋을 함께 싣기 때문이다. 기존 가지끼리 올리는 단계(branch:<가지>, 예 master ← live)는 그 가지 전부를 올리므로
// 세지 않는다(세면 앞 실행이 live 에 합친 커밋까지 "출발 가지에만 있던 커밋"으로 셌다, 5-3b ③)
public func apCarriesBase(source: String, after: String?, base: String?) -> Bool {
    !source.hasPrefix("branch:") && after != nil && base == after
}

// MARK: - finish 의 뿌리

// finish 가 쓰는 뿌리(기록 저장소·프로젝트 설정의 승격 차례)는 그 실행이 출발한 뿌리다(run.json dev, start 가 적는다. 설계 판단 5-3b ⑨).
// --dev 는 받되 맞는지만 본다: 설치판 뿌리에서 출발한 실행에 --dev 면 거부한다(다른 뿌리의 차례·기록으로 가지를 움직이지 않게).
// 시험 빌드 실행에 --dev 가 없어도 run.json 을 따른다(보드·옛 손 명령). run.json 에 dev 가 없으면(옛 실행) --dev 를 따른다
public func apFinishRoot(flag: Bool, runDev: Bool?) -> (dev: Bool, refuse: Bool) {
    guard let r = runDev else { return (flag, false) }
    return (r, flag && !r)
}
