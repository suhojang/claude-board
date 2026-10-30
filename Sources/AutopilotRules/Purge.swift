// 자율주행 규칙(AutopilotRules) — 지난 실행의 "원문 지우기"(2026-10-22 사용자 결정: 지난 실행 원문은 자동으로 지우지 않고 지난 실행 화면에서 사람이 지운다).
// 2026-10-30 2단계 5-2b 에서 새로 씀. 지울 수 있는지(apPurgeCheck)를 값으로 정하고, 지우기·기록은 포트(APRawFiles)로만 한다.
// 지우는 것은 실행 폴더 runs/<실행 id>/ 하나다(stream 원문·결과 기록·대기열 정의·진행 기록). 기록 저장소의 행(결과 기록 전문 포함)은 남아
// 지난 실행 목록·단위 결과는 그대로 보이고, 엔진 전용 private/<실행 id>/(하네스 사본)도 남긴다(설계 판단)
import Foundation

public struct APPurgeInput: Equatable, Sendable {
    public var run: String           // 실행 id
    public var root: String          // 자율주행 뿌리(…/autopilot)
    public var runDir: String        // 기록 저장소 runs.run_dir
    public var state: String         // runs.state
    public var wrapped: Bool         // 엔진이 끝 정리를 마쳤나(run.json 의 wrapped)
    public var mergeWaiting: Bool    // 합치기·승격이 남았나(규칙 apMergeOpen)
    public var worktreeLeft: Bool    // 따로 작업 트리(~/.claude-board/worktrees/<실행 id>)가 남았나
    public var inDraft: Bool         // 출발 준비가 이 실행을 이어 가는 중인가
    public var exists: Bool          // 실행 폴더가 아직 있나
    public init(run: String, root: String, runDir: String, state: String, wrapped: Bool, mergeWaiting: Bool, worktreeLeft: Bool, inDraft: Bool, exists: Bool) {
        self.run = run; self.root = root; self.runDir = runDir; self.state = state; self.wrapped = wrapped; self.mergeWaiting = mergeWaiting
        self.worktreeLeft = worktreeLeft; self.inDraft = inDraft; self.exists = exists
    }
}

public enum APPurgeDecision: Equatable, Sendable {
    case ok(String)      // 지울 자리
    case gone            // 이미 없다
    case refuse(String)  // 지우지 않는 까닭
}

// 지울 수 있는가 — 끝나고 정리를 마친 실행, 합치기·승격이 남지 않음(시트가 실행 폴더의 run.json 을 쓴다), 따로 작업 트리가 남지 않음,
// 출발 준비가 이어 가는 중이 아님. 자리는 뿌리 안 runs/<실행 id> 와 글자 그대로 같을 때만(기록의 경로가 다른 자리를 가리키면 지우지 않는다)
public func apPurgeCheck(_ i: APPurgeInput) -> APPurgeDecision {
    guard i.run.range(of: "^[A-Za-z0-9._-]+$", options: .regularExpression) != nil, i.run != ".", i.run != ".." else { return .refuse("실행 id 꼴이 아니라 지우지 않습니다") }
    let want = i.root + "/runs/" + i.run
    guard i.runDir == want else { return .refuse("실행 폴더가 이 보드의 자율주행 뿌리 밖(\(i.runDir))이라 보드에서 지우지 않습니다") }
    guard i.exists else { return .gone }
    guard ["done", "stopped", "error"].contains(i.state) else { return .refuse("끝나지 않은 실행입니다") }
    guard i.wrapped else { return .refuse("엔진이 끝 정리를 마치지 않았습니다(하네스 검사·기록 저장소 쓰기가 실행 폴더를 씁니다)") }
    if i.mergeWaiting { return .refuse("합치기·승격이 남았습니다. 합치기·승격이 이 실행 폴더의 기록을 쓰므로 합치거나 버린 뒤에 지웁니다") }
    if i.worktreeLeft { return .refuse("따로 작업 트리(~/.claude-board/worktrees/\(i.run))가 남아 있습니다. 합치기·승격에서 버리거나 합친 뒤에 지웁니다") }
    if i.inDraft { return .refuse("출발 준비가 이 실행을 이어 가는 중입니다(남은 단위의 정의를 이 실행 폴더에서 읽습니다)") }
    return .ok(want)
}

// 지운 기록(purged/<실행 id>.json) — 언제, 어디를, 파일 몇 개·몇 바이트, 남긴 것, 지우다 생긴 오류
public struct APPurgeRecord: Equatable, Sendable, Codable {
    public var run: String
    public var path: String
    public var at: Double
    public var files: Int
    public var bytes: Int64
    public var kept: [String]
    public var error: String?
    public init(run: String, path: String, at: Double, files: Int, bytes: Int64, kept: [String], error: String? = nil) {
        self.run = run; self.path = path; self.at = at; self.files = files; self.bytes = bytes; self.kept = kept; self.error = error
    }
}

public let apPurgeKept = ["기록 저장소의 실행·단위 행(결과 기록 전문)", "엔진 전용 private/<실행 id>/(하네스 사본)", "프로젝트 설정과 자율주행 세트"]

public struct APPurgeResult: Equatable, Sendable {
    public var ok = false
    public var record: APPurgeRecord?
    public var error: String?
    public init() {}
}

// 유스케이스: 원문 지우기 — 다시 확인(apPurgeCheck)하고, 링크가 아닌 진짜 폴더일 때만 크기를 센 뒤 지우고 기록을 쓴다.
// 지우다 실패해도 기록은 쓴다(무엇이 남았는지 사람이 볼 수 있게)
public func apPurgeRaw(_ input: APPurgeInput, files: APRawFiles, clock: APClock) -> APPurgeResult {
    var r = APPurgeResult()
    let target: String
    switch apPurgeCheck(input) {
    case .ok(let t): target = t
    case .gone: r.error = "실행 폴더가 이미 없습니다"; return r
    case .refuse(let why): r.error = why; return r
    }
    guard files.isRealDir(target) else { r.error = "실행 폴더가 링크이거나 폴더가 아니라 지우지 않습니다"; return r }
    let u = files.usage(target)
    let err = files.remove(target)
    var rec = APPurgeRecord(run: input.run, path: target, at: clock.now.timeIntervalSince1970, files: u.files, bytes: u.bytes,
                            kept: apPurgeKept.map { $0.replacingOccurrences(of: "<실행 id>", with: input.run) }, error: err)
    let e = JSONEncoder()
    e.outputFormatting = [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes]
    if !files.writeRecord(input.run, (try? e.encode(rec)) ?? Data()) { rec.error = (err.map { $0 + " · " } ?? "") + "지운 기록을 쓰지 못했습니다" }
    r.record = rec
    r.ok = err == nil
    r.error = rec.error
    return r
}
