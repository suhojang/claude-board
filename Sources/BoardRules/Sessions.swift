// 보드 규칙 — 세션·서브에이전트·카드 모델, 세션 기록 해석, 카드 묶음·정렬. AppKit·SwiftUI·엔진 없이 Foundation 만 쓴다.
// 보드 앱(ClaudeBoard.swift)에서 떼어 냈고 Tests/BoardRulesTests 가 시험한다(이슈 #9 B안 첫 단계).
// 모듈 밖에서 쓰므로 공개 멤버·init 을 손으로 적고(멤버별 init 은 모듈 밖에 보이지 않는다), 공개 struct 는 Sendable 을 적는다
import Foundation

public struct Session: Identifiable, Equatable, Sendable {
    public let id: String
    public let cwd: String
    public let projectDir: String  // claude 를 띄운 폴더(state.py 의 project_dir, 없으면 cwd) — cd·워크트리에도 그대로라 카드는 이것으로 묶는다
    public let state: String
    public let since: Date
    public let updated: Date
    public let headless: Bool
    public let tty: String
    public let topic: String
    public let message: String
    public let resumeCmd: String
    public let file: String
    public let ctx: Ctx?
    public let brain: Brain?       // 마지막 응답의 모델·effort
    public let pid: Int32
    public let subagents: [Sub]
    public let shells: Int         // 세션에 붙어 도는 백그라운드 셸 수
    public let paused: Bool        // 보드에서 일시중지한 세션 — sessions/<id>.paused 가 있다
    public let muted: Bool         // 보드에서 알림을 끈 세션 — sessions/<id>.mute 가 있으면 notify.py 가 패널을 띄우지 않는다
    public let limitAt: Date?      // 사용 한도에 걸려 응답이 끊긴 시각 — state.py 가 기록하고, 세션이 다시 움직이면 지운다
    public let name: String?       // Claude Code 의 세션 이름(ClaudeLive 포트 — pid 로 찾음). 끝난 세션·못 읽으면 nil

    public init(id: String, cwd: String, projectDir: String, state: String, since: Date, updated: Date, headless: Bool,
                tty: String, topic: String, message: String, resumeCmd: String, file: String, ctx: Ctx?, brain: Brain?,
                pid: Int32, subagents: [Sub], shells: Int, paused: Bool, muted: Bool, limitAt: Date?, name: String?) {
        self.id = id; self.cwd = cwd; self.projectDir = projectDir; self.state = state; self.since = since
        self.updated = updated; self.headless = headless; self.tty = tty; self.topic = topic; self.message = message
        self.resumeCmd = resumeCmd; self.file = file; self.ctx = ctx; self.brain = brain; self.pid = pid
        self.subagents = subagents; self.shells = shells; self.paused = paused; self.muted = muted
        self.limitAt = limitAt; self.name = name
    }

    public var rank: Int { ["waiting": 0, "working": 1, "paused": 2, "idle": 2, "ended": 3][state] ?? 9 }
    public var title: String { topic.isEmpty ? (message.isEmpty ? "요청 전" : message) : topic }
}

// 세션 파일(sessions/<sid>.json)의 세션 id — 기록의 session_id, 없으면 파일 이름에서 .json 을 뗀 것.
// 예전에는 카드의 세션 id 를 `session_id ?? 파일 이름` 으로 잡아 session_id 가 없으면 "<sid>.json" 이 되었다(일시중지·알림 끄기 표지 이름과 어긋남)
public func sessionID(_ record: [String: Any], fileName: String) -> String {
    if let id = record["session_id"] as? String { return id }
    return fileName.hasSuffix(".json") ? String(fileName.dropLast(5)) : fileName
}

// 세션이 호출한 서브에이전트 — state.py 가 SubagentStart·SubagentStop·도구 hook 으로 기록한다
public struct Sub: Identifiable, Equatable, Sendable {
    public let id: String
    public let type: String
    public let desc: String
    public let state: String       // running | waiting | done
    public let started: Date
    public let ended: Date?
    public let tool: String
    public let background: Bool
    public var brain: Brain? = nil  // 서브에이전트 대화 기록의 마지막 응답 모델·effort(scanSessions 가 채운다)

    public init(id: String, type: String, desc: String, state: String, started: Date, ended: Date?, tool: String,
                background: Bool, brain: Brain? = nil) {
        self.id = id; self.type = type; self.desc = desc; self.state = state; self.started = started
        self.ended = ended; self.tool = tool; self.background = background; self.brain = brain
    }

    // 보드 상태 이름으로 바꿔 아이콘·색을 세션과 같이 쓴다
    public var boardState: String { ["running": "working", "waiting": "waiting", "done": "idle"][state] ?? "working" }

    public static func parse(_ j: [String: Any]?, alive: Bool, now: Date) -> [Sub] {
        guard alive, let j else { return [] }
        return j.compactMap { id, v -> Sub? in
            guard let v = v as? [String: Any] else { return nil }
            let ended = (v["ended"] as? Double).map { Date(timeIntervalSince1970: $0) }
            // 끝난 서브에이전트는 10초 동안만 보여 준다
            if let ended, now.timeIntervalSince(ended) > 10 { return nil }
            // 시작 hook 없이 생긴 기록(끝 hook 이나 도구 hook 만 옴)은 실제 서브에이전트가 아니다 — 전사 파일도 종류도 없고,
            // 끝 hook 이 끝내 안 오면 실행 중으로 남아 세션을 계속 작업 중으로 보이게 하므로 목록에서 뺀다
            if !(v["started_seen"] as? Bool ?? false) { return nil }
            return Sub(id: id,
                       type: v["type"] as? String ?? "",
                       desc: v["description"] as? String ?? "",
                       state: v["state"] as? String ?? "running",
                       started: Date(timeIntervalSince1970: v["started"] as? Double ?? 0),
                       ended: ended,
                       tool: v["tool"] as? String ?? "",
                       background: v["background"] as? Bool ?? false)
        }
        .sorted { ($0.state == "done" ? 1 : 0, $0.started) < ($1.state == "done" ? 1 : 0, $1.started) }
    }
}

// 화면에 보이는 경로 — 홈 폴더면 "~", 홈 아래면 앞의 홈만 "~" 로 바꾼다.
// 예전에는 홈 문자열을 어디서든 바꿔 홈이 /Users/me 일 때 /Users/meow/x 가 "~ow/x" 가 되고 경로 중간의 홈도 바뀌었다
// (Windows 판 store.rs display_path 와 같은 뜻 — 거기는 대소문자를 가리지 않고 \ 도 구분자로 본다)
public func displayPath(_ path: String, home: String) -> String {
    guard !home.isEmpty else { return path }
    if path == home { return "~" }
    if path.hasPrefix(home + "/") { return "~" + path.dropFirst(home.count) }
    return path
}

public struct Group: Identifiable, Equatable, Sendable {
    public let id: String          // 세션들의 projectDir
    public let sessions: [Session]
    public init(id: String, sessions: [Session]) { self.id = id; self.sessions = sessions }
    public var name: String { (id as NSString).lastPathComponent }
    public var path: String { displayPath(id, home: NSHomeDirectory()) }
    public var urgent: String { sessions.min { $0.rank < $1.rank }?.state ?? "idle" }
    // 작업 중·승인 대기 세션 — sessions 는 급한 순이라 첫째가 대표다
    public var active: [Session] { sessions.filter { $0.state == "waiting" || $0.state == "working" } }
}

// 세션이 지금 하는 일 — 승인 대기면 요청 내용, 아니면 세션 제목. 접힌 카드와 미니 화면이 쓴다
public func doing(_ s: Session) -> String { s.state == "waiting" && !s.message.isEmpty ? s.message : s.title }

// 컨텍스트 사용량: 마지막 요청에 실린 입력 토큰 / 모델의 컨텍스트 크기
public struct Ctx: Equatable, Sendable {
    public let used: Int
    public let size: Int
    public init(used: Int, size: Int) { self.used = used; self.size = size }
    public var pct: Int { size > 0 ? min(100, used * 100 / size) : 0 }
}

// 응답을 만든 모델과 effort — 대화 기록의 assistant 줄에 message.model 과 effort 가 실린다
public struct Brain: Equatable, Sendable {
    public let model: String       // claude-opus-5-5 등
    public let effort: String      // low | medium | high | xhigh | max, 모르면 ""
    public init(model: String, effort: String) { self.model = model; self.effort = effort }

    // 모델 계열 이름 — 라벨에 짧게 쓴다
    public var family: String {
        for f in ["opus", "sonnet", "haiku", "fable"] where model.contains(f) { return f.prefix(1).uppercased() + f.dropFirst() }
        return model.replacingOccurrences(of: "claude-", with: "")
    }
    // 계열 뒤 버전 숫자 — claude-opus-5-5 → 5.5, claude-fable-5-1 → 5.1
    public var version: String {
        guard let r = model.range(of: "-(\\d+)-(\\d+)", options: .regularExpression) else { return "" }
        return model[r].dropFirst().replacingOccurrences(of: "-", with: ".")
    }
    public static let efforts = ["low", "medium", "high", "xhigh", "max"]
    public var effortLevel: Int { (Self.efforts.firstIndex(of: effort) ?? -1) + 1 }   // 0 = 모름
}

// 경과 시간 글자 — 줄마다 매초 바뀐다
public func elapsed(_ from: Date, _ now: Date) -> String {
    let s = max(0, Int(now.timeIntervalSince(from)))
    if s < 60 { return "\(s)초" }
    if s < 3600 { return "\(s / 60)분 \(s % 60)초" }
    return "\(s / 3600)시간 \(s % 3600 / 60)분"
}

// MARK: - 카드 묶음 (Store.reload 끝부분)

// 카드에 보일 세션 — 대화를 백그라운드로 넘긴 세션(handedTo: 넘긴 세션 id → 이어 받은 세션 id)은 이어 받은 세션이 목록에 있을 때만 빼고
// (그 세션 파일이 없으면 지금처럼 보인다), 보드 탭이 이어 받은 작업을 따라가는 동안의 탭 claude 세션(parkedFrom)도 뺀다.
// 탭 짝짓기는 이 목록이 아니라 모든 세션을 본다
public func visibleSessions(_ sessions: [Session], handedTo: [String: String], parkedFrom: Set<String>) -> [Session] {
    guard !handedTo.isEmpty || !parkedFrom.isEmpty else { return sessions }
    let ids = Set(sessions.map(\.id))
    return sessions.filter { s in
        !parkedFrom.contains(s.id) && (handedTo[s.id].map { !ids.contains($0) } ?? true)
    }
}

// 세션을 claude 를 띄운 폴더(projectDir)별 카드로 묶는다. 카드 안은 급한 순(rank), 같으면 최근에 상태가 바뀐 것(since) 먼저.
// 카드 순서는 정하지 않는다(sortGroups)
public func groupSessions(_ sessions: [Session]) -> [Group] {
    Dictionary(grouping: sessions, by: \.projectDir).map { dir, list in
        Group(id: dir, sessions: list.sorted { $0.rank != $1.rank ? $0.rank < $1.rank : $0.since > $1.since })
    }
}

// 카드 순서 — 고정한 카드(고정한 순서) > 가장 급한 세션의 rank > 마지막 활동(updated)이 최근인 것
public func sortGroups(_ groups: [Group], pinned: [String]) -> [Group] {
    groups.sorted {
        let pa = pinned.firstIndex(of: $0.id) ?? Int.max, pb = pinned.firstIndex(of: $1.id) ?? Int.max
        if pa != pb { return pa < pb }
        let a = $0.sessions.map(\.rank).min() ?? 9, b = $1.sessions.map(\.rank).min() ?? 9
        if a != b { return a < b }
        return ($0.sessions.map(\.updated).max() ?? .distantPast) > ($1.sessions.map(\.updated).max() ?? .distantPast)
    }
}
