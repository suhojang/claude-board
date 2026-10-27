// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI

// MARK: - 데이터

// 파일 수정 시각 — FileManager.attributesOfItem 은 확장 속성까지 읽어 느리므로(1초 주기 reload 의 가장 큰 몫이었다) stat 만 부른다
func mtime(_ path: String) -> Date? {
    var st = stat()
    guard stat(path, &st) == 0 else { return nil }
    return Date(timeIntervalSince1970: TimeInterval(st.st_mtimespec.tv_sec) + TimeInterval(st.st_mtimespec.tv_nsec) / 1e9)
}

struct Session: Identifiable, Equatable {
    let id: String
    let cwd: String
    let projectDir: String  // claude 를 띄운 폴더(state.py 의 project_dir, 없으면 cwd) — cd·워크트리에도 그대로라 카드는 이것으로 묶는다
    let state: String
    let since: Date
    let updated: Date
    let headless: Bool
    let tty: String
    let topic: String
    let message: String
    let resumeCmd: String
    let file: String
    let ctx: Ctx?
    let brain: Brain?       // 마지막 응답의 모델·effort
    let pid: Int32
    let subagents: [Sub]
    let shells: Int         // 세션에 붙어 도는 백그라운드 셸 수
    let paused: Bool        // 보드에서 일시중지한 세션 — sessions/<id>.paused 가 있다
    let muted: Bool         // 보드에서 알림을 끈 세션 — sessions/<id>.mute 가 있으면 notify.py 가 패널을 띄우지 않는다
    let limitAt: Date?      // 사용 한도에 걸려 응답이 끊긴 시각 — state.py 가 기록하고, 세션이 다시 움직이면 지운다
    let name: String?       // Claude Code 의 세션 이름(ClaudeNames — pid 로 찾음). 끝난 세션·못 읽으면 nil

    var rank: Int { ["waiting": 0, "working": 1, "paused": 2, "idle": 2, "ended": 3][state] ?? 9 }
    var title: String { topic.isEmpty ? (message.isEmpty ? "요청 전" : message) : topic }
}

// 세션이 호출한 서브에이전트 — state.py 가 SubagentStart·SubagentStop·도구 hook 으로 기록한다
struct Sub: Identifiable, Equatable {
    let id: String
    let type: String
    let desc: String
    let state: String       // running | waiting | done
    let started: Date
    let ended: Date?
    let tool: String
    let background: Bool
    var brain: Brain? = nil  // 서브에이전트 대화 기록의 마지막 응답 모델·effort

    // 보드 상태 이름으로 바꿔 아이콘·색을 세션과 같이 쓴다
    var boardState: String { ["running": "working", "waiting": "waiting", "done": "idle"][state] ?? "working" }

    static func parse(_ j: [String: Any]?, alive: Bool, now: Date) -> [Sub] {
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

struct Group: Identifiable, Equatable {
    let id: String          // 세션들의 projectDir
    let sessions: [Session]
    var name: String { (id as NSString).lastPathComponent }
    var path: String { id.replacingOccurrences(of: NSHomeDirectory(), with: "~") }
    var urgent: String { sessions.min { $0.rank < $1.rank }?.state ?? "idle" }
    // 작업 중·승인 대기 세션 — sessions 는 급한 순이라 첫째가 대표다
    var active: [Session] { sessions.filter { $0.state == "waiting" || $0.state == "working" } }
}

// 세션이 지금 하는 일 — 승인 대기면 요청 내용, 아니면 세션 제목. 접힌 카드와 미니 화면이 쓴다
func doing(_ s: Session) -> String { s.state == "waiting" && !s.message.isEmpty ? s.message : s.title }

// 컨텍스트 사용량: 마지막 요청에 실린 입력 토큰 / 모델의 컨텍스트 크기
struct Ctx: Equatable {
    let used: Int
    let size: Int
    var pct: Int { size > 0 ? min(100, used * 100 / size) : 0 }
}

// 응답을 만든 모델과 effort — 대화 기록의 assistant 줄에 message.model 과 effort 가 실린다
struct Brain: Equatable {
    let model: String       // claude-opus-5-5 등
    let effort: String      // low | medium | high | xhigh | max, 모르면 ""

    // 모델 계열 이름 — 라벨에 짧게 쓴다
    var family: String {
        for f in ["opus", "sonnet", "haiku", "fable"] where model.contains(f) { return f.prefix(1).uppercased() + f.dropFirst() }
        return model.replacingOccurrences(of: "claude-", with: "")
    }
    // 계열 뒤 버전 숫자 — claude-opus-5-5 → 5.5, claude-fable-5-1 → 5.1
    var version: String {
        guard let r = model.range(of: "-(\\d+)-(\\d+)", options: .regularExpression) else { return "" }
        return model[r].dropFirst().replacingOccurrences(of: "-", with: ".")
    }
    static let efforts = ["low", "medium", "high", "xhigh", "max"]
    var effortLevel: Int { (Self.efforts.firstIndex(of: effort) ?? -1) + 1 }   // 0 = 모름
}

// 대화 기록 끝에서 마지막 응답의 모델·effort 를 읽는다 — 파일이 바뀌어도 3초에 한 번만 다시 읽는다
final class ModelReader {
    private var parsed: [String: (mtime: Date, checked: Date, brain: Brain?)] = [:]

    func read(_ path: String, fallbackModel: String = "", now: Date) -> Brain? {
        let changed = mtime(path) ?? .distantPast
        if let c = parsed[path], c.mtime == changed || now.timeIntervalSince(c.checked) < 3 {
            return c.brain ?? (fallbackModel.isEmpty ? nil : Brain(model: fallbackModel, effort: ""))
        }
        var brain: Brain? = nil
        if let h = FileHandle(forReadingAtPath: path) {
            let end = (try? h.seekToEnd()) ?? 0
            try? h.seek(toOffset: end > 400_000 ? end - 400_000 : 0)
            let data = (try? h.readToEnd()) ?? Data()
            try? h.close()
            let marker = Data("\"assistant\"".utf8)
            for line in data.split(separator: UInt8(ascii: "\n")).reversed() where line.range(of: marker) != nil {
                guard let j = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any],
                      j["type"] as? String == "assistant",
                      let msg = j["message"] as? [String: Any],
                      let m = msg["model"] as? String, m != "<synthetic>" else { continue }
                brain = Brain(model: m, effort: j["effort"] as? String ?? "")
                break
            }
        }
        parsed[path] = (changed, now, brain)
        return brain ?? (fallbackModel.isEmpty ? nil : Brain(model: fallbackModel, effort: ""))
    }

    // 목록에서 사라진 기록은 잊는다
    func prune(keep: Set<String>) { parsed = parsed.filter { keep.contains($0.key) } }
}

// Esc 로 끊은 응답 — Claude Code 는 사용자가 끊으면 Stop hook 을 부르지 않아 세션 파일이 작업 중·승인 대기로 남는다.
// 대화 기록 끝의 마지막 대화 줄이 중단 표시([Request interrupted by user], … for tool use])면 그 시각을 돌려준다
final class InterruptReader {
    private var parsed: [String: (mtime: Date, at: Date?)] = [:]

    func read(_ path: String) -> Date? {
        guard let changed = mtime(path) else { return nil }
        if let c = parsed[path], c.mtime == changed { return c.at }
        var at: Date? = nil
        if let h = FileHandle(forReadingAtPath: path) {
            let end = (try? h.seekToEnd()) ?? 0
            try? h.seek(toOffset: end > 256_000 ? end - 256_000 : 0)
            let data = (try? h.readToEnd()) ?? Data()
            try? h.close()
            // 중단 표시가 없으면 JSON 을 풀지 않는다 — 작업 중 세션은 대부분 여기서 끝난다
            if data.range(of: Data("[Request interrupted by user".utf8)) != nil {
                // 중단 뒤에는 cost-state·last-prompt 같은 대화 아닌 줄이 붙고, ! 명령·슬래시 명령 출력은 < 로 시작하는 사용자 줄로 남는다
                for line in data.split(separator: UInt8(ascii: "\n")).reversed() {
                    guard let j = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any],
                          let type = j["type"] as? String, type == "user" || type == "assistant",
                          (j["isSidechain"] as? Bool) != true, (j["isMeta"] as? Bool) != true else { continue }
                    let content = (j["message"] as? [String: Any])?["content"]
                    let text = content as? String ?? (content as? [[String: Any]])?.first?["text"] as? String ?? ""
                    if type == "user", text.hasPrefix("<") { continue }
                    if type == "user", text.hasPrefix("[Request interrupted by user") { at = isoDate(j["timestamp"] as? String) ?? changed }
                    break
                }
            }
        }
        parsed[path] = (changed, at)
        return at
    }

    func prune(keep: Set<String>) { parsed = parsed.filter { keep.contains($0.key) } }
}

// Claude Code 의 세션 이름 — Claude Code 는 도는 세션마다 ~/.claude/sessions/<pid>.json 에 name(자동 이름은 "<작업 폴더 이름>-<16진수 두 글자>",
// --name·/rename 으로 바꾼 이름)과 status(idle·busy, 승인 창·질문 창이 떠 있으면 waiting)를 쓰고, 끝나면(/exit·탭 닫기의 SIGHUP) 지운다
// (2.1.304 실측). hook 입력에는 이름이 없어 이 파일로만 안다. 세션 파일의 pid 로 찾는다 — /clear 해도 pid·이름은 그대로다
final class ClaudeNames {
    static let shared = ClaudeNames()
    private let dir = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/sessions")
    private var cache: [Int32: (mtime: Date, j: [String: Any]?)] = [:]

    private func path(_ pid: Int32) -> String { (dir as NSString).appendingPathComponent("\(pid).json") }

    // 1초마다 읽는 카드 라벨·탭 줄 — 파일이 바뀔 때만 다시 푼다
    private func record(_ pid: Int32) -> [String: Any]? {
        guard pid > 0, let m = mtime(path(pid)) else { cache[pid] = nil; return nil }
        if cache[pid]?.mtime != m {
            cache[pid] = (m, FileManager.default.contents(atPath: path(pid)).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] })
        }
        return cache[pid]?.j
    }

    func name(_ pid: Int32) -> String? {
        (record(pid)?["name"] as? String).flatMap { $0.isEmpty ? nil : $0 }
    }

    // status 와 그것이 바뀐 시각(statusUpdatedAt, ms) — 그 파일의 sessionId 가 이 세션일 때만(/clear 하면 같은 pid 가 새 id 를 쓴다)
    func statusChange(_ pid: Int32, session: String) -> (status: String, at: Date)? {
        guard let j = record(pid), j["sessionId"] as? String == session, let status = j["status"] as? String,
              let ms = (j["statusUpdatedAt"] as? NSNumber)?.doubleValue else { return nil }
        return (status, Date(timeIntervalSince1970: ms / 1000))
    }

    // 지금 status — 이름 바꾸기를 치기 직전에 보므로 캐시 없이 읽는다. 파일·값이 없으면 nil
    func status(_ pid: Int32) -> String? {
        guard pid > 0, let data = FileManager.default.contents(atPath: path(pid)) else { return nil }
        return ((try? JSONSerialization.jsonObject(with: data)) as? [String: Any])?["status"] as? String
    }

    // 쓰고 있는 이름 — 파일의 pid 가 살아 있는 claude 인 것만(강제로 죽은 claude 의 파일은 남을 수 있다). pid → 이름
    func live() -> [Int32: String] {
        var out: [Int32: String] = [:]
        for f in (try? FileManager.default.contentsOfDirectory(atPath: dir)) ?? [] where f.hasSuffix(".json") {
            if let pid = Int32(f.dropLast(5)), isLiveClaude(pid), let n = name(pid) { out[pid] = n }
        }
        return out
    }

    func prune(keep: Set<Int32>) { cache = cache.filter { keep.contains($0.key) } }
}

// 세션별 컨텍스트 사용량을 읽는다.
// 1순위: 상태줄 스크립트가 남긴 ~/.claude/notifier/context/<세션 ID>.json (Claude Code 화면의 값과 같다)
// 2순위: 상태줄이 없는 백그라운드 세션은 대화 기록 끝의 마지막 응답 usage 로 계산한다
final class ContextReader {
    private let dir = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/context")
    private var parsed: [String: (mtime: Date, checked: Date, ctx: Ctx?)] = [:]
    // 상태줄에서 배운 모델별 컨텍스트 크기 — 보드를 다시 켜도 쓰도록 파일에 남긴다
    private let windowsPath = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/context-windows.json")
    private var windows: [String: Int] = [:]

    init() {
        if let data = FileManager.default.contents(atPath: windowsPath),
           let j = try? JSONSerialization.jsonObject(with: data) as? [String: Int] { windows = j }
    }

    private func learn(_ model: String, _ size: Int) {
        guard windows[model] != size else { return }
        windows[model] = size
        if let data = try? JSONSerialization.data(withJSONObject: windows) {
            try? data.write(to: URL(fileURLWithPath: windowsPath), options: .atomic)
        }
    }

    // 배운 값이 없을 때의 기본 크기 — 5 세대 모델(Opus·Sonnet·Haiku 5.5, Fable 5.1)은 1M(2026-10-14 실측), 그 밖은 200K
    private func defaultWindow(_ model: String) -> Int {
        if model.contains("[1m]") { return 1_000_000 }
        let oneM = ["claude-opus-5", "claude-sonnet-5", "claude-haiku-5", "claude-fable-5"]
        return oneM.contains { model.hasPrefix($0) } ? 1_000_000 : 200_000
    }

    // 상태줄 기록 — 매초 세션마다 읽어 풀던 것을, 파일이 바뀔 때만 다시 읽는다
    private var lines: [String: (mtime: Date, j: [String: Any]?)] = [:]

    private func statusLine(_ sid: String) -> [String: Any]? {
        let path = (dir as NSString).appendingPathComponent("\(sid).json")
        guard let m = mtime(path) else { lines[sid] = nil; return nil }
        if let c = lines[sid], c.mtime == m { return c.j }
        let j = FileManager.default.contents(atPath: path).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
        lines[sid] = (m, j)
        return j
    }

    func read(_ sid: String, transcript: String, model: String, now: Date) -> Ctx? {
        let tMtime = mtime(transcript) ?? .distantPast
        if let j = statusLine(sid),
           let cw = j["context_window"] as? [String: Any],
           let size = (cw["context_window_size"] as? NSNumber)?.intValue, size > 0 {
            if let m = j["model"] as? String, !m.isEmpty { learn(m, size) }
            let at = Date(timeIntervalSince1970: (j["at"] as? NSNumber)?.doubleValue ?? 0)
            // 상태줄 기록 뒤에 대화가 더 진행됐으면(백그라운드로 다시 열린 경우 등) 대화 기록 쪽을 쓴다
            if tMtime.timeIntervalSince(at) < 60 {
                let u = cw["current_usage"] as? [String: Any] ?? [:]
                let used = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                    .reduce(0) { $0 + ((u[$1] as? NSNumber)?.intValue ?? 0) }
                if used > 0 { return Ctx(used: used, size: size) }
                if let p = (cw["used_percentage"] as? NSNumber)?.intValue { return Ctx(used: size * p / 100, size: size) }
            }
        }
        return fromTranscript(transcript, mtime: tMtime, model: model, now: now)
    }

    // 상태줄이 남긴 세션의 현재 effort — /effort 로 바꾸면 다음 응답 전에도 바로 바뀐다. 컨텍스트와 같은 기준으로 신선할 때만 쓴다
    func liveEffort(_ sid: String, transcript: String) -> String? {
        guard let j = statusLine(sid), let e = j["effort"] as? String, !e.isEmpty else { return nil }
        let tMtime = mtime(transcript) ?? .distantPast
        let at = Date(timeIntervalSince1970: (j["at"] as? NSNumber)?.doubleValue ?? 0)
        return tMtime.timeIntervalSince(at) < 60 ? e : nil
    }

    private func fromTranscript(_ path: String, mtime: Date, model: String, now: Date) -> Ctx? {
        if let c = parsed[path], c.mtime == mtime || now.timeIntervalSince(c.checked) < 5 { return c.ctx }
        var used = 0, lastModel = model
        if let h = FileHandle(forReadingAtPath: path) {
            let end = (try? h.seekToEnd()) ?? 0
            try? h.seek(toOffset: end > 1_500_000 ? end - 1_500_000 : 0)
            let data = (try? h.readToEnd()) ?? Data()
            try? h.close()
            for line in data.split(separator: UInt8(ascii: "\n")).reversed() {
                guard let j = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any],
                      j["type"] as? String == "assistant", (j["isSidechain"] as? Bool) != true,
                      let msg = j["message"] as? [String: Any], msg["model"] as? String != "<synthetic>",
                      let u = msg["usage"] as? [String: Any] else { continue }
                used = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                    .reduce(0) { $0 + ((u[$1] as? NSNumber)?.intValue ?? 0) }
                if lastModel.isEmpty, let m = msg["model"] as? String { lastModel = m }
                break
            }
        }
        var ctx: Ctx? = nil
        if used > 0 {
            var size = windows[lastModel] ?? defaultWindow(lastModel)
            if used > size { size = 1_000_000 }
            ctx = Ctx(used: used, size: size)
        }
        parsed[path] = (mtime, now, ctx)
        return ctx
    }
}

// 요금제 사용 한도: 5시간·주간(전체)·주간(모델별 — 예: Fable)
struct Limit: Equatable, Identifiable {
    let id: String
    let label: String
    let pct: Double
    let resets: Date?

    // 간략 모드 이름 — 5시간 · 주간 · 모델 이름(Fable). 더 좁으면 5h · 7d · 첫 글자
    var short: String {
        id == "5h" ? "5시간" : id == "7d" ? "주간" : (label.components(separatedBy: "·").last ?? label).trimmingCharacters(in: .whitespaces)
    }
    var tiny: String { id == "5h" ? "5h" : id == "7d" ? "7d" : String(short.prefix(1)) }
}

final class UsageStore: ObservableObject {
    @Published var limits: [Limit] = []
    @Published var failed = false
    // 로그인 토큰이 만료된 채로 있다(계정을 막 바꿨거나 한동안 세션이 없었다) — 세션이 다음 요청을 보내며 갱신해야 사용량을 받을 수 있다
    @Published var tokenExpired = false
    // 상태줄 스크립트와 같은 캐시 파일을 함께 쓴다 — 사용량 API 호출을 줄인다
    private let cache = "/tmp/.claude-usage-cache.json"
    private var fetching = false
    private var generation = 0
    // 계정을 바꾼 직후 이 시각 전에 쓰인 캐시는 믿지 않는다 — 상태줄이 앞 계정 토큰으로 받던 값을 늦게 써 넣을 수 있다
    private var trustCacheAfter = Date.distantPast
    private var timer: Timer?

    init() {
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in self?.refresh() }
    }

    // 계정을 바꾼 뒤: 앞 계정 값이 든 캐시를 버리고 바로 새로 받는다 — 상태줄도 같은 캐시를 쓰므로 함께 새 계정 값이 된다
    func reset() {
        generation += 1
        trustCacheAfter = Date().addingTimeInterval(5)
        try? FileManager.default.removeItem(atPath: cache)
        failed = false
        withAnimation(.easeOut(duration: 0.2)) { limits = [] }
        refresh()
    }

    func refresh() {
        let written = mtime(cache).flatMap { $0 > trustCacheAfter ? $0 : nil }
        let age = Date().timeIntervalSince(written ?? .distantPast)
        if written != nil, let data = FileManager.default.contents(atPath: cache) { apply(data) }
        guard age > 120, !fetching else { return }
        fetching = true
        let gen = generation
        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let self else { return }
            let (data, expired) = Self.fetch()
            DispatchQueue.main.async {
                self.fetching = false
                // 받는 사이 계정을 바꿨으면 앞 계정 값이므로 버리고 다시 받는다
                guard gen == self.generation else { self.refresh(); return }
                self.tokenExpired = expired
                if let data, self.apply(data) {
                    try? data.write(to: URL(fileURLWithPath: self.cache), options: .atomic)
                    self.trustCacheAfter = .distantPast   // 새 계정 토큰으로 직접 받은 값이다
                    self.failed = false
                } else if self.limits.isEmpty {
                    self.failed = true
                }
            }
        }
    }

    // 키체인의 Claude Code 로그인 토큰으로 사용량 API 를 부른다 — 토큰은 메모리에만 두고 기록하지 않는다
    static func fetch() -> (data: Data?, expired: Bool) {
        guard let raw = Keychain.read(credentialsService()),
              let j = try? JSONSerialization.jsonObject(with: raw) as? [String: Any],
              let oauth = j["claudeAiOauth"] as? [String: Any],
              let token = oauth["accessToken"] as? String else { return (nil, false) }
        if let exp = (oauth["expiresAt"] as? NSNumber)?.doubleValue, exp / 1000 < Date().timeIntervalSince1970 { return (nil, true) }
        var req = URLRequest(url: URL(string: "https://api.anthropic.com/api/oauth/usage")!, timeoutInterval: 10)
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("oauth-2025-04-20", forHTTPHeaderField: "anthropic-beta")
        let sem = DispatchSemaphore(value: 0)
        var result: Data?
        URLSession.shared.dataTask(with: req) { data, resp, _ in
            if (resp as? HTTPURLResponse)?.statusCode == 200 { result = data }
            sem.signal()
        }.resume()
        _ = sem.wait(timeout: .now() + 12)
        return (result, false)
    }

    @discardableResult
    func apply(_ data: Data) -> Bool {
        guard let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let five = j["five_hour"] as? [String: Any] else { return false }
        var out: [Limit] = []
        func add(_ id: String, _ label: String, _ o: [String: Any]?, _ key: String) {
            guard let o, let v = (o[key] as? NSNumber)?.doubleValue else { return }
            out.append(Limit(id: id, label: label, pct: v, resets: isoDate(o["resets_at"] as? String)))
        }
        add("5h", "5시간", five, "utilization")
        add("7d", "주간 · 전체", j["seven_day"] as? [String: Any], "utilization")
        for l in j["limits"] as? [[String: Any]] ?? [] where l["kind"] as? String == "weekly_scoped" {
            let name = ((l["scope"] as? [String: Any])?["model"] as? [String: Any])?["display_name"] as? String ?? "모델"
            add("7d-\(name)", "주간 · \(name)", l, "percent")
        }
        if out != limits { withAnimation(.spring(response: 0.6, dampingFraction: 0.85)) { limits = out } }
        return true
    }
}

// 날짜 형식기는 만들기가 비싸다 — 매초 다시 그리는 줄마다 새로 만들지 않게 한 번만 만든다
private let isoFractional: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return f
}()
private let isoPlain = ISO8601DateFormatter()

func isoDate(_ s: String?) -> Date? {
    guard let s else { return nil }
    return isoFractional.date(from: s) ?? isoPlain.date(from: s)
}

// MARK: - 계정 전환

// Claude Code 로그인 정보는 키체인 일반 암호 항목(서비스 "Claude Code-credentials", 계정 $USER)에 JSON 으로 있다.
// /usr/bin/security 로 읽고 쓴다 — Claude Code 도 security 로 만들고 고치므로 허용 창이 뜨지 않는다.
// 쓸 값은 명령줄 대신 표준 입력(security -i)으로 넘겨 ps 에 드러나지 않게 한다
enum Keychain {
    // Claude Code 와 같은 계정 이름 규칙
    static let user: String = {
        let u = ProcessInfo.processInfo.environment["USER"] ?? NSUserName()
        return u.range(of: "^[a-zA-Z0-9._-]+$", options: .regularExpression) != nil ? u : "claude-code-user"
    }()

    @discardableResult
    private static func security(_ args: [String], input: String? = nil) -> (status: Int32, out: Data) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        p.arguments = args
        let out = Pipe(), inp = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        if input != nil { p.standardInput = inp }
        guard (try? p.run()) != nil else { return (-1, Data()) }
        if let input {
            inp.fileHandleForWriting.write(Data(input.utf8))
            try? inp.fileHandleForWriting.close()
        }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return (p.terminationStatus, data)
    }

    static func read(_ service: String, account: String = user) -> Data? {
        let r = security(["find-generic-password", "-a", account, "-s", service, "-w"])
        guard r.status == 0 else { return nil }
        var d = r.out
        while d.last == 0x0a { d.removeLast() }
        // ASCII 가 아닌 바이트가 섞인 값은 16진수로 나온다 — JSON 은 { 로 시작하므로 16진수 글자만 있으면 풀어서 돌려준다
        let hexDigits = Set("0123456789abcdef".utf8)
        if d.first != UInt8(ascii: "{"), d.count % 2 == 0, d.allSatisfy(hexDigits.contains) {
            let s = Array(d)
            d = Data(stride(from: 0, to: s.count, by: 2).map { UInt8(String(decoding: s[$0..<$0 + 2], as: UTF8.self), radix: 16)! })
        }
        return d.isEmpty ? nil : d
    }

    static func exists(_ service: String, account: String = user) -> Bool {
        security(["find-generic-password", "-a", account, "-s", service]).status == 0
    }

    static func write(_ service: String, account: String = user, _ data: Data) -> Bool {
        let hex = data.map { String(format: "%02x", $0) }.joined()
        let line = "add-generic-password -U -a \"\(account)\" -s \"\(service)\" -X \"\(hex)\"\n"
        // security -i 는 한 줄 4KB 까지 받는다 — 넘으면 Claude Code 처럼 명령줄로 넘긴다
        if line.utf8.count <= 4032 { return security(["-i"], input: line).status == 0 }
        return security(["add-generic-password", "-U", "-a", account, "-s", service, "-X", hex]).status == 0
    }

    static func delete(_ service: String, account: String = user) {
        security(["delete-generic-password", "-a", account, "-s", service])
    }

    // Claude Code 는 security -w 출력을 그대로 JSON 으로 읽는다 — 16진수로 나오지 않게 ASCII 밖 글자는 \uXXXX 로 쓴다
    static func asciiJSON(_ obj: Any) throws -> Data {
        let s = String(decoding: try JSONSerialization.data(withJSONObject: obj, options: [.withoutEscapingSlashes]), as: UTF8.self)
        var out = ""
        for u in s.utf16 {
            if u < 0x80 { out.unicodeScalars.append(Unicode.Scalar(UInt8(u))) } else { out += String(format: "\\u%04x", u) }
        }
        return Data(out.utf8)
    }

    static func json(_ service: String, account: String = user) -> [String: Any]? {
        read(service, account: account).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }
}

// Claude Code 의 로그인 정보 키체인 서비스 이름 — CLAUDE_CONFIG_DIR 을 정해 띄우면 "-<그 경로(NFC) sha256 앞 8자>" 가 붙는다 (2.1.300 실측)
func credentialsService(_ configDir: String? = nil) -> String {
    guard let dir = configDir else { return "Claude Code-credentials" }
    let hex = SHA256.hash(data: Data(dir.precomposedStringWithCanonicalMapping.utf8)).map { String(format: "%02x", $0) }.joined()
    return "Claude Code-credentials-\(hex.prefix(8))"
}

struct AccountError: LocalizedError {
    let errorDescription: String?
    init(_ m: String) { errorDescription = m }
}

// Claude Code 와 같은 잠금 — proper-lockfile 처럼 "<파일>.lock" 폴더를 만들어 잡는다. 15초 넘게 갱신되지 않은 잠금은 버려진 것으로 본다
func withFileLock<T>(_ file: String, _ body: () throws -> T) throws -> T {
    let lock = file + ".lock"
    let deadline = Date().addingTimeInterval(5)
    while mkdir(lock, 0o755) != 0 {
        guard errno == EEXIST else { throw AccountError("잠금을 만들지 못했습니다: \(lock)") }
        if let m = mtime(lock), Date().timeIntervalSince(m) > 15 { rmdir(lock); continue }
        guard Date() < deadline else { throw AccountError("Claude Code 가 로그인 정보를 쓰는 중입니다. 잠시 뒤 다시 시도해 주세요") }
        Thread.sleep(forTimeInterval: 0.1)
    }
    defer { rmdir(lock) }
    return try body()
}

// JSON 설정 파일의 몇 키만 바꿔 쓴다 — Claude Code 와 같은 잠금 아래에서 읽고 고친 뒤 임시 파일을 거쳐 바꿔 넣는다. 권한(0600)은 그대로 둔다
func patchJSON(_ path: String, _ change: (inout [String: Any]) -> Void) throws {
    try withFileLock(path) {
        guard let data = FileManager.default.contents(atPath: path),
              var j = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AccountError("\((path as NSString).lastPathComponent) 를 읽지 못했습니다")
        }
        change(&j)
        let out = try JSONSerialization.data(withJSONObject: j, options: [.prettyPrinted, .withoutEscapingSlashes])
        let perms = (try? FileManager.default.attributesOfItem(atPath: path)[.posixPermissions]) ?? 0o600
        try out.write(to: URL(fileURLWithPath: path), options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: perms], ofItemAtPath: path)
    }
}

// 저장해 둔 Claude 계정 — 토큰은 키체인(서비스 "Claude Board-account", 계정 이름 = id)에만 두고 여기에는 화면에 보일 정보만 담는다
struct Account: Identifiable, Equatable {
    let id: String                  // accountUuid|organizationUuid — 같은 사람도 조직이 다르면 토큰이 따로다
    let oauth: [String: Any]        // ~/.claude.json 의 oauthAccount 그대로 — 전환할 때 이 값으로 바꿔 넣는다
    var refreshExpires: Date?       // 이 시각이 지나면 다시 로그인해야 한다

    init?(oauth: [String: Any], refreshExpires: Date? = nil) {
        guard let uuid = oauth["accountUuid"] as? String, !uuid.isEmpty else { return nil }
        id = uuid + "|" + (oauth["organizationUuid"] as? String ?? "")
        self.oauth = oauth
        self.refreshExpires = refreshExpires
    }

    var email: String { oauth["emailAddress"] as? String ?? "?" }
    var org: String { oauth["organizationName"] as? String ?? "" }
    // 요금제 — organizationType claude_max + 한도 등급 …_5x → "Max 5x"
    var plan: String {
        let t = (oauth["organizationType"] as? String ?? "").replacingOccurrences(of: "claude_", with: "")
        var p = t.prefix(1).uppercased() + t.dropFirst()
        if let tier = oauth["organizationRateLimitTier"] as? String,
           let r = tier.range(of: "_\\d+x$", options: .regularExpression) { p += " " + tier[r].dropFirst() }
        return p
    }
    var expired: Bool { refreshExpires.map { $0 < Date() } ?? false }

    static func == (a: Account, b: Account) -> Bool {
        a.id == b.id && a.email == b.email && a.org == b.org && a.plan == b.plan && a.refreshExpires == b.refreshExpires
    }
}

// 계정 전환: 계정마다 브라우저 로그인을 한 번 받아 토큰을 저장해 두고, 고르면 Claude Code 로그인 항목의 토큰만 바꿔 넣는다.
// 실행 중인 세션은 요청을 보낼 때마다 키체인을 다시 읽으므로(최대 30초 캐시) 모든 세션이 다음 요청부터 새 계정으로 이어 간다
final class AccountStore: ObservableObject {
    struct Notice: Equatable { let text: String; let error: Bool }

    @Published private(set) var current: Account?          // 지금 로그인된 계정 — ~/.claude.json 의 oauthAccount
    @Published private(set) var saved: [Account] = []      // 전환할 수 있게 저장해 둔 계정
    @Published private(set) var adding = false              // 계정 추가 — 브라우저 로그인을 기다리는 중
    @Published private(set) var switching = false
    @Published private(set) var notice: Notice?
    // 사용 한도가 차면 다른 계정으로 자동 전환(AutoSwitcher) — 계정 메뉴에서 켜고 끈다. 기본은 꺼짐
    @Published var autoSwitch = UserDefaults.standard.bool(forKey: "autoSwitch") {
        didSet { UserDefaults.standard.set(autoSwitch, forKey: "autoSwitch") }
    }
    var onSwitch: (() -> Void)?

    static let vault = "Claude Board-account"
    // Claude Code 가 /login 으로 다른 계정에 들어갈 때 ~/.claude.json 에서 비우는 계정별 캐시(2.1.300 기준) —
    // 같이 비워야 새 계정의 모델 목록·조직 기본 모델·사용량 표시를 다시 받는다
    static let accountCaches = ["additionalModelOptionsCache", "additionalModelOptionsAnsweredAt", "additionalModelCostsCache",
                                "modelAccessCache", "orgModelDefaultCache", "cachedArtifactRoster", "artifactRosterDenied",
                                "lastSeenOrgDefaultUpdatedAt", "clientDataCache", "clientDataCacheSlots", "autoCompactWindowsCache",
                                "cachedUsageUtilization", "metricsStatusCache", "metricsStatusCacheByPrincipal",
                                "githubWebConnectionStatusCache", "startupPrefetchedAt"]
    private let configDir = NSHomeDirectory() + "/.claude"
    private let globalConfig = NSHomeDirectory() + "/.claude.json"
    private let metaPath = NSHomeDirectory() + "/.claude/notifier/accounts.json"
    private let loginRoot = NSHomeDirectory() + "/.claude/notifier/accounts"
    private let queue = DispatchQueue(label: "com.claude.board.accounts")
    private var currentMtime: Date?
    private var timer: Timer?
    private var login: (slot: String, started: Date)?
    private var tokenOnlySince: Date?       // 로그인 토큰은 생겼는데 계정 정보가 아직 없는 때
    private var polling = false
    private var noticeWork: DispatchWorkItem?

    init() {
        loadSaved()
        loadCurrent()
        // 터미널에서 /login 으로 바꿔도 따라가도록 계정 정보 파일을 지켜본다 — 바뀔 때만 다시 읽는다
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            self?.loadCurrent()
            self?.pollLogin()
        }
        // 지난번에 끝나지 않은 로그인 자리를 치운다
        queue.async { [loginRoot] in
            for name in (try? FileManager.default.contentsOfDirectory(atPath: loginRoot)) ?? [] where name.hasPrefix("login-") {
                Self.dropSlot(loginRoot + "/" + name)
            }
        }
    }

    static func oauthAccount(at path: String) -> [String: Any]? {
        guard let data = FileManager.default.contents(atPath: path),
              let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return j["oauthAccount"] as? [String: Any]
    }

    func loadCurrent(force: Bool = false) {
        let m = mtime(globalConfig)
        guard force || m != currentMtime else { return }
        currentMtime = m
        let a = Self.oauthAccount(at: globalConfig).flatMap { Account(oauth: $0) }
        if a != current { current = a }
    }

    private static func readMeta(_ path: String) -> [[String: Any]] {
        guard let d = FileManager.default.contents(atPath: path),
              let j = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { return [] }
        return j["accounts"] as? [[String: Any]] ?? []
    }

    private static func metaId(_ e: [String: Any]) -> String? {
        (e["oauthAccount"] as? [String: Any]).flatMap { Account(oauth: $0) }?.id
    }

    private func writeMeta(_ list: [[String: Any]]) throws {
        let data = try JSONSerialization.data(withJSONObject: ["accounts": list], options: [.prettyPrinted, .withoutEscapingSlashes])
        try data.write(to: URL(fileURLWithPath: metaPath), options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: metaPath)   // 이메일·조직 이름이 들어 있다
    }

    private func loadSaved() {
        let list = Self.readMeta(metaPath).compactMap { e -> Account? in
            guard let oa = e["oauthAccount"] as? [String: Any] else { return nil }
            let exp = (e["refreshTokenExpiresAt"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue / 1000) }
            return Account(oauth: oa, refreshExpires: exp)
        }
        if list != saved { saved = list }
    }

    // 계정 토큰(claudeAiOauth)은 키체인에, 보일 정보는 accounts.json 에 넣는다 — 같은 계정은 제자리에서 바꾼다. queue 에서 부른다
    private func store(_ oa: [String: Any], _ oauth: [String: Any]) throws {
        guard let a = Account(oauth: oa) else { throw AccountError("계정 정보가 없습니다") }
        guard Keychain.write(Self.vault, account: a.id, try Keychain.asciiJSON(["claudeAiOauth": oauth])) else {
            throw AccountError("키체인에 저장하지 못했습니다")
        }
        var e: [String: Any] = ["oauthAccount": oa, "savedAt": Date().timeIntervalSince1970]
        if let r = oauth["refreshTokenExpiresAt"] { e["refreshTokenExpiresAt"] = r }
        var list = Self.readMeta(metaPath)
        if let i = list.firstIndex(where: { Self.metaId($0) == a.id }) { list[i] = e } else { list.append(e) }
        try writeMeta(list)
    }

    // 지금 로그인된 계정의 최신 토큰을 저장한다 — 쓰는 동안 갱신되며 바뀐 토큰이라, 처음 저장했던 것은 이미 못 쓸 수 있다.
    // .storage-write 잠금 안에서 부른다
    private func saveCurrent(except skip: String? = nil) throws {
        guard let oa = Self.oauthAccount(at: globalConfig), Account(oauth: oa)?.id != skip,
              let co = Keychain.json(credentialsService())?["claudeAiOauth"] as? [String: Any] else { return }
        try store(oa, co)
    }

    // note: 전환했을 때 띄울 안내(자동 전환), done: 전환했는지 — 자동 전환이 결과를 받는다
    func switchTo(_ a: Account, note: String? = nil, done: ((Bool) -> Void)? = nil) {
        guard !switching else { done?(false); return }
        switching = true
        queue.async { [weak self] in
            guard let self else { return }
            let r = Result { try self.performSwitch(a) }
            DispatchQueue.main.async {
                self.switching = false
                self.loadCurrent(force: true)
                self.loadSaved()
                switch r {
                case .success(let warning):
                    self.onSwitch?()
                    self.flash(warning ?? note ?? "\(a.email) 계정으로 전환했습니다. 실행 중인 세션도 다음 요청부터 이 계정을 씁니다",
                               error: warning != nil)
                    done?(true)
                case .failure(let e):
                    self.flash("전환하지 못했습니다: \(e.localizedDescription)", error: true)
                    done?(false)
                }
            }
        }
    }

    // queue 에서 부른다. 고칠 수 없었던 부분이 있으면 그 안내를 돌려준다
    private func performSwitch(_ target: Account) throws -> String? {
        guard let oauth = Keychain.json(Self.vault, account: target.id)?["claudeAiOauth"] as? [String: Any] else {
            throw AccountError("저장된 로그인 정보가 없습니다. 계정을 다시 추가해 주세요")
        }
        // CLAUDE_CONFIG_DIR 을 따로 정해 띄운 세션은 이름 뒤에 경로 해시가 붙은 다른 항목을 쓴다 — 그 항목은 건드리지 않는다.
        // 같은 토큰을 두 항목에 넣으면 한쪽이 갱신(회전)할 때 다른 쪽 토큰이 무효가 되기 때문
        let main = credentialsService()
        let previous = Self.oauthAccount(at: globalConfig).flatMap { Account(oauth: $0) }?.id
        // Claude Code 가 토큰을 갱신해 쓸 때 잡는 잠금 — 그 사이에 끼어 앞 계정 토큰으로 덮이지 않게 한다
        try withFileLock(configDir + "/.storage-write") {
            var cur: [String: Any] = [:]
            if let raw = Keychain.read(main) {
                guard let j = try? JSONSerialization.jsonObject(with: raw) as? [String: Any] else {
                    throw AccountError("지금 로그인 정보를 읽지 못했습니다")
                }
                cur = j
            } else if Keychain.exists(main) {
                throw AccountError("지금 로그인 정보를 읽지 못했습니다")
            }
            try saveCurrent(except: target.id)
            // 계정 토큰만 바꿔 넣는다 — MCP 서버 로그인(mcpOAuth) 같은 다른 값은 그대로 둔다
            cur["claudeAiOauth"] = oauth
            guard Keychain.write(main, try Keychain.asciiJSON(cur)) else { throw AccountError("키체인에 쓰지 못했습니다") }
        }
        // 계정 정보 — /status·claude auth status·보드 계정 표시가 새 계정으로 보이게. 다른 계정이면 앞 계정의 캐시도 비운다
        do {
            try patchJSON(globalConfig) { j in
                j["oauthAccount"] = target.oauth
                if previous != target.id { for k in Self.accountCaches { j[k] = nil } }
            }
        } catch {
            return "토큰은 바꿨지만 계정 정보 파일을 고치지 못했습니다: \(error.localizedDescription)"
        }
        return nil
    }

    // 계정 추가: 따로 정한 설정 폴더(CLAUDE_CONFIG_DIR)로 claude auth login 을 터미널에서 띄운다 — 지금 로그인은 건드리지 않는다.
    // 로그인이 끝나면 그 폴더 몫 키체인 항목에서 토큰을 가져와 저장하고 자리를 치운다
    func beginAdd(email: String? = nil) {
        guard login == nil else { return }
        guard let claude = findCommand("claude") else { return flash("claude 명령을 찾지 못했습니다", error: true) }
        let slot = loginRoot + "/login-" + UUID().uuidString.prefix(8).lowercased()
        do { try FileManager.default.createDirectory(atPath: slot, withIntermediateDirectories: true) } catch {
            return flash("로그인 자리를 만들지 못했습니다: \(error.localizedDescription)", error: true)
        }
        var cmd = "CLAUDE_CONFIG_DIR=\(shellQuote(slot)) \(shellQuote(claude.path)) auth login --claudeai"
        if let email { cmd += " --email \(shellQuote(email))" }
        cmd += " && echo && echo 'Claude Board 에 계정을 추가했습니다. 이 창은 닫아도 됩니다.'"
        runInTerminal(cmd)
        login = (slot, Date())
        adding = true
        flash("터미널과 브라우저에서 추가할 계정으로 로그인해 주세요")
    }

    func cancelAdd() {
        guard let l = login else { return }
        login = nil
        tokenOnlySince = nil
        adding = false
        queue.async { Self.dropSlot(l.slot) }
    }

    private static func dropSlot(_ slot: String) {
        Keychain.delete(credentialsService(slot))
        try? FileManager.default.removeItem(atPath: slot)
    }

    // 로그인이 끝났는지 본다 — 그 설정 폴더에 계정 정보가, 키체인에 그 폴더 몫 토큰이 생기면 끝난 것
    private func pollLogin() {
        guard let l = login, !polling else { return }
        if Date().timeIntervalSince(l.started) > 15 * 60 {
            cancelAdd()
            return flash("로그인을 15분 동안 기다리다 그만뒀습니다", error: true)
        }
        polling = true
        queue.async { [weak self] in
            guard let self else { return }
            var result: Result<Account, Error>?
            var tokenOnly = false
            if let oa = Self.oauthAccount(at: l.slot + "/.claude.json"), let a = Account(oauth: oa),
               let oauth = Keychain.json(credentialsService(l.slot))?["claudeAiOauth"] as? [String: Any] {
                result = Result {
                    try self.store(oa, oauth)
                    // 지금 계정도 함께 저장해 둔다 — 목록에 나란히 보이고 언제든 돌아올 수 있게
                    try? withFileLock(self.configDir + "/.storage-write") { try self.saveCurrent(except: a.id) }
                    Self.dropSlot(l.slot)
                    return a
                }
            } else {
                tokenOnly = Keychain.exists(credentialsService(l.slot))
            }
            DispatchQueue.main.async {
                self.polling = false
                // 프로필을 받지 못하면 Claude Code 가 계정 정보를 남기지 않는다 — 30초 넘게 그러면 다시 해 보라고 알린다
                if tokenOnly, self.login?.slot == l.slot {
                    let since = self.tokenOnlySince ?? Date()
                    self.tokenOnlySince = since
                    if Date().timeIntervalSince(since) > 30 {
                        self.cancelAdd()
                        self.flash("로그인은 됐지만 계정 정보를 받지 못했습니다. 계정 추가를 다시 해 주세요", error: true)
                    }
                    return
                }
                guard let result, self.login?.slot == l.slot else { return }
                self.login = nil
                self.tokenOnlySince = nil
                self.adding = false
                self.loadSaved()
                switch result {
                case .success(let a): self.flash("\(a.email) 계정을 추가했습니다. 계정 메뉴에서 골라 바로 전환할 수 있습니다")
                case .failure(let e): self.flash("계정을 추가하지 못했습니다: \(e.localizedDescription)", error: true)
                }
            }
        }
    }

    func remove(_ a: Account) {
        queue.async { [weak self] in
            guard let self else { return }
            Keychain.delete(Self.vault, account: a.id)
            try? self.writeMeta(Self.readMeta(self.metaPath).filter { Self.metaId($0) != a.id })
            DispatchQueue.main.async {
                self.loadSaved()
                self.flash("\(a.email) 계정을 목록에서 지웠습니다")
            }
        }
    }

    func flash(_ text: String, error: Bool = false) {
        withAnimation(.easeOut(duration: 0.2)) { notice = Notice(text: text, error: error) }
        noticeWork?.cancel()
        let w = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.3)) { self?.notice = nil } }
        noticeWork = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 7, execute: w)
    }
}

// MARK: - 사용 한도 자동 전환

// 지금 계정의 5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 한도로 멈췄던 세션에 이어서 진행하라는 요청을 넣는다.
// 계정 메뉴에서 켜고 끄며 기본은 꺼짐. 다른 계정의 남은 한도는 미리 알 수 없어(저장한 토큰은 그 계정으로 전환해야 갱신된다)
// 그 계정이 지금 계정이었을 때 한도가 찼던 기록으로 고른다. 결정마다 ~/.claude/notifier/auto-switch.log 에 한 줄씩 남긴다
final class AutoSwitcher {
    // 실행 중인 세션은 로그인 정보를 최대 30초 동안 기억해 두고 쓴다 — 전환한 뒤 이만큼은 이전 계정으로 요청이 나갈 수 있다
    static let settle: TimeInterval = 40
    // 이보다 오래된 한도 기록은 보지 않는다 — 오래전에 멈춘 세션을 계정을 바꿀 때마다 깨우지 않게
    static let recent: TimeInterval = 6 * 3600
    static let prompt = "사용 한도로 멈췄던 세션을 다른 계정으로 전환해 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요."
    private let store: Store
    private let usage: UsageStore
    private let accounts: AccountStore
    private var timer: Timer?
    // 마지막으로 계정을 바꾼 시각(손으로 바꾼 것 포함) — 보드를 다시 켜도 이어지게 저장한다
    private var lastSwitch = Date(timeIntervalSince1970: UserDefaults.standard.double(forKey: "autoSwitchLast")) {
        didSet { UserDefaults.standard.set(lastSwitch.timeIntervalSince1970, forKey: "autoSwitchLast") }
    }
    private var pending = false              // 자동 전환을 요청하고 결과를 기다리는 중
    private var resumed = Set<String>()      // 이어 가기를 넣은 "세션 id@한도 시각" — 한 번만 넣는다
    private var noCandidate = false          // 전환할 계정이 없다고 이미 알렸다 — 계정이 바뀌기 전에는 다시 알리지 않는다
    // 한도가 찬 계정 id 와 풀리는 시각 — 그 전에는 고르지 않는다
    private var full = UserDefaults.standard.dictionary(forKey: "autoSwitchFull") as? [String: Double] ?? [:] {
        didSet { UserDefaults.standard.set(full, forKey: "autoSwitchFull") }
    }
    private let logPath = NSHomeDirectory() + "/.claude/notifier/auto-switch.log"

    init(store: Store, usage: UsageStore, accounts: AccountStore) {
        self.store = store
        self.usage = usage
        self.accounts = accounts
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.tick() }
    }

    // 계정이 바뀌었다 — 이때부터 settle 동안 난 한도 오류는 이전 계정 토큰으로 나간 요청이다
    func switched() {
        lastSwitch = Date()
        noCandidate = false
    }

    private func tick() {
        guard accounts.autoSwitch else { return }
        let now = Date()
        let settled = lastSwitch.addingTimeInterval(Self.settle)
        // 승인 대기 세션은 Esc 가 승인을 거절하므로 건드리지 않는다
        let limited = store.groups.flatMap(\.sessions).filter {
            guard let at = $0.limitAt else { return false }
            return !["ended", "paused", "waiting"].contains($0.state) && now.timeIntervalSince(at) < Self.recent
        }
        // 1) 전환 — 지금 계정으로 새로 한도에 걸린 세션이 있거나, 지금 계정의 5시간·주간 한도가 찼다
        if !pending, !accounts.switching, now > settled, let cur = accounts.current {
            let fullLimit = usage.limits.first { ($0.id == "5h" || $0.id == "7d") && $0.pct >= 100 && ($0.resets ?? .distantFuture) > now }
            if let hit = limited.first(where: { $0.limitAt! > settled }) {
                switchAway(cur, because: "\((hit.cwd as NSString).lastPathComponent) 세션이 한도에 걸림(\(hit.message))", now: now)
            } else if let fullLimit {
                switchAway(cur, because: "\(fullLimit.label) 한도 \(Int(fullLimit.pct))%", now: now)
            }
        }
        // 2) 이어 가기 — 전환하고 settle 이 지났으면, 그 전(이전 계정)에 한도로 멈춘 세션에 이어서 진행하라는 요청을 넣는다
        guard now > settled else { return }
        let todo = limited.filter { $0.limitAt! <= settled && !resumed.contains("\($0.id)@\($0.limitAt!.timeIntervalSince1970)") }
        guard !todo.isEmpty else { return }
        var manual: [String] = []
        for s in todo {
            resumed.insert("\(s.id)@\(s.limitAt!.timeIntervalSince1970)")
            let name = (s.cwd as NSString).lastPathComponent
            // 1초 타이머(메인 스레드)에서 부른다
            guard MainActor.assumeIsolated({ canDriveSession(s) }) else {
                manual.append(name)
                log("\(name): 보드 탭·Ghostty 세션이 아니라 이어 가지 못함")
                continue
            }
            // Esc 를 먼저 — Claude Code 가 띄운 한도 선택 창을 닫고 "한도가 풀리면 저절로 이어 가기" 예약을 취소한다.
            // 선택 창이 열린 채 Enter 를 보내면 그 창의 항목이 골라진다
            MainActor.assumeIsolated {
                driveSession(s, [("key", "escape"), ("wait", "0.5"), ("text", Self.prompt), ("wait", "0.2"), ("key", "enter")]) { [weak self] ok in
                    self?.log("\(name): \(ok ? "이어 가기 요청을 넣음" : "터미널을 찾지 못해 이어 가지 못함")")
                }
            }
        }
        let n = todo.count - manual.count
        accounts.flash((n > 0 ? "한도로 멈췄던 세션 \(n)개를 이어서 진행합니다" : "")
                       + (manual.isEmpty ? "" : (n > 0 ? ". " : "") + "\(manual.joined(separator: ", ")) 세션은 보드 탭·Ghostty 밖이라 직접 이어 주세요"),
                       error: !manual.isEmpty)
    }

    // 지금 계정은 한도가 풀릴 때까지 고르지 않게 적어 두고, 저장한 계정 가운데 지금 계정 다음부터 쓸 수 있는 계정으로 바꾼다
    private func switchAway(_ cur: Account, because reason: String, now: Date) {
        // 풀리는 시각은 지금 계정의 찬 한도에서 읽는다 — 모르면 1시간 뒤 다시 본다
        let resets = usage.limits.filter { $0.pct >= 100 }.compactMap(\.resets).filter { $0 > now }.max()
        var marks = full.filter { $0.value > now.timeIntervalSince1970 }
        marks[cur.id] = (resets ?? now.addingTimeInterval(3600)).timeIntervalSince1970
        full = marks
        let saved = accounts.saved
        let start = saved.firstIndex { $0.id == cur.id }.map { $0 + 1 } ?? 0
        guard let next = (saved[start...] + saved[..<start]).first(where: { $0.id != cur.id && !$0.expired && marks[$0.id] == nil }) else {
            if !noCandidate {
                noCandidate = true
                log("\(reason): 전환할 계정 없음")
                accounts.flash("사용 한도에 도달했지만 전환할 다른 계정이 없습니다. 저장한 계정이 모두 한도에 도달했거나 다시 로그인이 필요합니다", error: true)
            }
            return
        }
        pending = true
        log("\(reason): \(cur.email) 에서 \(next.email) 로 전환")
        let stopped = store.groups.flatMap(\.sessions).contains { $0.limitAt != nil && $0.state != "ended" }
        let note = "사용 한도에 도달해 \(next.email) 계정으로 자동 전환했습니다"
            + (stopped ? ". 한도로 멈춘 세션은 \(Int(Self.settle))초 뒤 이어서 진행합니다" : "")
        accounts.switchTo(next, note: note) { [weak self] ok in
            self?.pending = false
            guard !ok else { return }
            self?.log("\(next.email) 로 전환하지 못함 (1시간 동안 고르지 않음)")
            self?.full[next.id] = now.addingTimeInterval(3600).timeIntervalSince1970
        }
    }

    private func log(_ text: String) {
        let line = "\(Date()) \(text)\n"
        if let h = FileHandle(forWritingAtPath: logPath) {
            h.seekToEndOfFile()
            h.write(Data(line.utf8))
            try? h.close()
        } else {
            FileManager.default.createFile(atPath: logPath, contents: Data(line.utf8))
        }
    }
}

// 앱 아이콘(AppIcon.icns) — 메뉴 막대와 보드 제목 옆에도 같은 그림을 쓴다. 크기별 그림 중 화면 배율에 맞는 것이 골라진다
let appIcon = NSImage(named: "AppIcon") ?? NSImage()

enum Palette {
    static func color(_ state: String) -> Color {
        switch state {
        case "working": return Color(red: 0.45, green: 0.66, blue: 1.00)
        case "waiting": return Color(red: 1.00, green: 0.66, blue: 0.36)
        case "idle": return Color(red: 0.70, green: 0.75, blue: 0.86)   // 유휴 — 완료로 읽히지 않게 초록 대신 차분한 회청색
        case "paused": return Color(red: 0.95, green: 0.80, blue: 0.35)
        default: return Color(white: 0.55)
        }
    }

    static func name(_ state: String) -> String {
        ["working": "작업 중", "waiting": "승인 대기", "idle": "대기", "paused": "일시중지", "ended": "종료"][state] ?? state
    }

    static let accent = Color(red: 0.85, green: 0.47, blue: 0.34)
}

final class Store: ObservableObject {
    @Published var groups: [Group] = []
    @Published var now = Date()
    // 접힌 그룹의 경로(Group.id) — 앱을 다시 켜도 유지한다
    @Published var collapsed = Set(UserDefaults.standard.stringArray(forKey: "collapsed") ?? [])
    // 전체 알림 끄기 — ~/.claude/notifier/mute-all 이 있으면 notify.py 가 모든 세션의 알림을 띄우지 않는다
    @Published var allMuted = FileManager.default.fileExists(atPath: muteAllPath)
    private let dir = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/sessions")
    private var timer: Timer?
    private let contexts = ContextReader()
    private let models = ModelReader()
    private let interrupts = InterruptReader()
    // 백그라운드 셸 수 — ps 를 메인 스레드에서 돌리면 클릭 처리가 밀리므로 뒤에서 세고 다음 reload 가 쓴다
    private var shellCounts: [Int32: Int] = [:]
    private var shellsBusy = false
    private var shellsAt = Date.distantPast
    // 세션 기록 — 매초 30여 개 파일을 다시 읽어 풀던 것을, 파일이 바뀔 때만 다시 읽는다
    private var records: [String: (mtime: Date, j: [String: Any])] = [:]

    // ps 는 프로세스 수백 개를 훑으므로 3초에 한 번만 센다
    private func refreshShells() {
        guard !shellsBusy, now.timeIntervalSince(shellsAt) >= 3 else { return }
        shellsBusy = true
        shellsAt = now
        DispatchQueue.global(qos: .utility).async {
            let counts = backgroundShells()
            DispatchQueue.main.async { [weak self] in
                self?.shellCounts = counts
                self?.shellsBusy = false
            }
        }
    }

    init() {
        reload()
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.reload() }
        // 세션 줄에서 표지 파일을 바꾸면 1초 주기를 기다리지 않고 바로 다시 읽는다
        NotificationCenter.default.addObserver(forName: .boardReload, object: nil, queue: .main) { [weak self] _ in
            self?.reload(force: true)
        }
    }

    func count(_ state: String) -> Int { groups.reduce(0) { $0 + $1.sessions.filter { $0.state == state }.count } }

    // 직전 reload 때 승인 대기였던 세션 — 새로 승인 대기가 된 세션만 골라 그 그룹을 펼친다
    private var waitingIds: Set<String> = []

    func toggle(_ cwd: String) {
        if collapsed.contains(cwd) { collapsed.remove(cwd) } else { collapsed.insert(cwd) }
        UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
    }

    // 종료 세션 목록을 펼쳐 둔 그룹의 경로(Group.id) — 기본은 접힘
    @Published var endedOpen = Set(UserDefaults.standard.stringArray(forKey: "endedOpen") ?? [])

    func toggleEnded(_ cwd: String) {
        if endedOpen.contains(cwd) { endedOpen.remove(cwd) } else { endedOpen.insert(cwd) }
        UserDefaults.standard.set(Array(endedOpen), forKey: "endedOpen")
    }

    // 상단에 고정한 작업경로 — 고정한 순서대로 맨 위에 둔다
    @Published var pinnedGroups = UserDefaults.standard.stringArray(forKey: "pinnedGroups") ?? []

    func togglePinGroup(_ cwd: String) {
        if let i = pinnedGroups.firstIndex(of: cwd) { pinnedGroups.remove(at: i) } else { pinnedGroups.append(cwd) }
        UserDefaults.standard.set(pinnedGroups, forKey: "pinnedGroups")
        reload(force: true)
    }

    // 목록에서 지운 작업경로와 지운 시각 — 그 뒤에 새 활동(세션 기록 갱신)이 생기면 다시 나타난다
    private var hiddenGroups = UserDefaults.standard.dictionary(forKey: "hiddenGroups") as? [String: Double] ?? [:]

    func hideGroup(_ g: Group) {
        // 종료된 세션 기록은 함께 지운다
        for s in g.sessions where s.state == "ended" { try? FileManager.default.removeItem(atPath: s.file) }
        hiddenGroups[g.id] = Date().timeIntervalSince1970
        UserDefaults.standard.set(hiddenGroups, forKey: "hiddenGroups")
        pinnedGroups.removeAll { $0 == g.id }
        UserDefaults.standard.set(pinnedGroups, forKey: "pinnedGroups")
        reload(force: true)
    }

    // 그 작업경로의 종료된 세션 기록을 한 번에 지운다 — 대화 기록(jsonl)은 남는다
    func removeEnded(_ g: Group) {
        for s in g.sessions where s.state == "ended" { try? FileManager.default.removeItem(atPath: s.file) }
        reload(force: true)
    }

    // 자동으로 접은 작업경로와 그때의 마지막 활동 시각 — 사람이 다시 펼치면 새 활동이 생기기 전까지 또 접지 않는다
    private var autoFolded = UserDefaults.standard.dictionary(forKey: "autoFolded") as? [String: Double] ?? [:]
    static let quietFoldAfter: TimeInterval = 30 * 60

    // 승인 대기·작업 중 세션이 없고 마지막 활동이 30분 넘게 지난 카드를 한 번 접는다
    private func foldQuiet(_ groups: [Group]) {
        var changed = false
        for g in groups {
            let last = (g.sessions.map(\.updated).max() ?? .distantPast).timeIntervalSince1970
            let quiet = !g.sessions.contains { $0.state == "waiting" || $0.state == "working" }
            guard quiet, now.timeIntervalSince1970 - last > Store.quietFoldAfter else { continue }
            if autoFolded[g.id] == last { continue }
            autoFolded[g.id] = last
            changed = true
            if !collapsed.contains(g.id) {
                withAnimation(foldSpring) { _ = collapsed.insert(g.id) }
                UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
            }
        }
        let ids = Set(groups.map(\.id))
        if autoFolded.keys.contains(where: { !ids.contains($0) }) {
            autoFolded = autoFolded.filter { ids.contains($0.key) }
            changed = true
        }
        if changed { UserDefaults.standard.set(autoFolded, forKey: "autoFolded") }
    }

    // 요약 칩으로 찾아갈 때: 카드와 그 안의 터미널·백그라운드 구역을 펼친다
    func reveal(_ cwd: String) {
        if collapsed.contains(cwd) {
            collapsed.remove(cwd)
            UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
        }
        let d = UserDefaults.standard
        let keys = (d.string(forKey: "kindCollapsed") ?? "").split(separator: "\n").map(String.init)
        let kept = keys.filter { !$0.hasPrefix(cwd + "|") }
        if kept.count != keys.count { d.set(kept.joined(separator: "\n"), forKey: "kindCollapsed") }
    }

    func setAll(collapse: Bool) {
        collapsed = collapse ? Set(groups.map(\.id)) : []
        UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
    }

    private func expandNewlyWaiting(_ sessions: [Session]) {
        let waiting = sessions.filter { $0.state == "waiting" }
        let opened = Set(waiting.filter { !waitingIds.contains($0.id) }.map(\.projectDir)).intersection(collapsed)
        waitingIds = Set(waiting.map(\.id))
        guard !opened.isEmpty else { return }
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { collapsed.subtract(opened) }
        UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
    }

    // 세션별 .mute 표지는 건드리지 않으므로 다시 켜면 세션마다 정해 둔 상태가 그대로 돌아온다
    func setAllMuted(_ on: Bool) {
        if on {
            FileManager.default.createFile(atPath: muteAllPath, contents: Data())
        } else {
            try? FileManager.default.removeItem(atPath: muteAllPath)
        }
        withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { allMuted = on }
    }

    func reload(force: Bool = false) {
        now = Date()
        let fm = FileManager.default
        let shells = shellCounts
        refreshShells()
        var sessions: [Session] = []
        var brainPaths = Set<String>()
        var seen = Set<String>()
        for name in (try? fm.contentsOfDirectory(atPath: dir)) ?? [] where name.hasSuffix(".json") {
            let path = (dir as NSString).appendingPathComponent(name)
            guard let m = mtime(path) else { continue }
            seen.insert(path)
            let j: [String: Any]
            if let c = records[path], c.mtime == m {
                j = c.j
            } else {
                guard let data = fm.contents(atPath: path),
                      let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
                records[path] = (m, parsed)
                j = parsed
            }
            var state = j["state"] as? String ?? "idle"
            let updated = Date(timeIntervalSince1970: j["updated"] as? Double ?? 0)
            if state != "ended", let pid = (j["pid"] as? NSNumber)?.int32Value, kill(pid, 0) != 0, errno == ESRCH {
                state = "ended"
            }
            // 질문 없이 끝난 세션은 대화 기록 파일이 없어 이어 열 수 없으므로 목록에서 뺀다
            let sid = j["session_id"] as? String ?? String(name.dropLast(5))
            let cwdPath = j["cwd"] as? String ?? "?"
            let transcript = transcriptPath(j["transcript_path"] as? String, cwdPath, sid)
            if state == "ended", now.timeIntervalSince(updated) > 2 * 86400
                || !fm.fileExists(atPath: transcript) {
                try? fm.removeItem(atPath: path)
                continue
            }
            // Esc 로 끊긴 세션은 입력을 기다리는 중이다 — 그 뒤에도 도는 서브에이전트·셸이 있으면 아래에서 다시 작업 중이 된다.
            // 첫 응답이 오기 전에 끊으면 대화 기록에 중단 표시가 남지 않으므로 Claude Code 의 status 가 이번 상태 뒤에 idle 이 된 것도 본다
            let pid = (j["pid"] as? NSNumber)?.int32Value ?? 0
            var since = Date(timeIntervalSince1970: j["since"] as? Double ?? 0)
            let live = ClaudeNames.shared.statusChange(pid, session: sid).flatMap { $0.at > since ? $0 : nil }
            if state == "working" || state == "waiting",
               let at = interrupts.read(transcript) ?? (live?.status == "idle" ? live?.at : nil) {
                state = "idle"
                since = at
            } else if state == "waiting", let live, live.status == "busy" {
                // 승인한 도구가 도는 동안은 hook 이 없다(PostToolUse 까지) — 승인 창이 닫혀 Claude Code 가 다시 busy 면 작업 중이다
                state = "working"
                since = live.at
            }
            // 응답은 끝났어도 서브에이전트나 백그라운드 셸이 돌고 있으면 작업 중으로 보인다
            let subDir = ((transcript.hasSuffix(".jsonl") ? String(transcript.dropLast(6)) : transcript) as NSString)
                .appendingPathComponent("subagents")
            let subs = Sub.parse(j["subagents"] as? [String: Any], alive: state != "ended", now: now).map { sub -> Sub in
                var sub = sub
                let p = (subDir as NSString).appendingPathComponent("agent-\(sub.id).jsonl")
                brainPaths.insert(p)
                sub.brain = models.read(p, now: now)
                return sub
            }
            let shellCount = state == "idle" ? shells[pid] ?? 0 : 0   // 응답 중에 도는 셸은 보통 도구 호출이라 세지 않는다
            if state == "idle", shellCount > 0 || subs.contains(where: { $0.state != "done" }) {
                state = "working"
            }
            // 일시중지 표지는 보드가 남기고, 새 요청이 들어오면 state.py 가 지운다
            let pausedFile = (dir as NSString).appendingPathComponent("\(sid).paused")
            let paused = fm.fileExists(atPath: pausedFile)
            if paused, state != "ended" { state = "paused" }
            let muted = fm.fileExists(atPath: (dir as NSString).appendingPathComponent("\(sid).mute"))
            var brain: Brain? = nil
            if state != "ended" {
                brainPaths.insert(transcript)
                brain = models.read(transcript, fallbackModel: j["model"] as? String ?? "", now: now)
                if let b = brain, let e = contexts.liveEffort(sid, transcript: transcript) { brain = Brain(model: b.model, effort: e) }
            }
            let clean = { (s: String) in
                s.replacingOccurrences(of: "[#*`]+", with: "", options: .regularExpression)
                    .trimmingCharacters(in: .whitespaces)
            }
            sessions.append(Session(id: j["session_id"] as? String ?? name,
                                    cwd: j["cwd"] as? String ?? "?",
                                    projectDir: j["project_dir"] as? String ?? j["cwd"] as? String ?? "?",
                                    state: state,
                                    since: since,
                                    updated: updated,
                                    headless: j["headless"] as? Bool ?? false,
                                    tty: j["tty"] as? String ?? "",
                                    topic: clean(j["topic"] as? String ?? ""),
                                    message: clean(j["last_message"] as? String ?? ""),
                                    resumeCmd: j["resume_cmd"] as? String ?? "",
                                    file: path,
                                    ctx: contexts.read(sid, transcript: transcript, model: j["model"] as? String ?? "", now: now),
                                    brain: brain,
                                    pid: pid,
                                    subagents: subs,
                                    shells: shellCount,
                                    paused: paused,
                                    muted: muted,
                                    limitAt: ((j["limit_hit"] as? [String: Any])?["at"] as? Double).map { Date(timeIntervalSince1970: $0) },
                                    name: state == "ended" ? nil : ClaudeNames.shared.name(pid)))
        }
        ClaudeNames.shared.prune(keep: Set(sessions.map(\.pid)))
        models.prune(keep: brainPaths)
        interrupts.prune(keep: brainPaths)
        records = records.filter { seen.contains($0.key) }
        MainActor.assumeIsolated { TerminalHub.shared.pair(sessions) }   // 1초 타이머·알림 관찰자 모두 메인 스레드
        expandNewlyWaiting(sessions)
        let muted = fm.fileExists(atPath: muteAllPath)
        if muted != allMuted { allMuted = muted }
        let grouped = Dictionary(grouping: sessions, by: \.projectDir).map { dir, list in
            Group(id: dir, sessions: list.sorted { $0.rank != $1.rank ? $0.rank < $1.rank : $0.since > $1.since })
        }
        // 지운 뒤 새 활동이 없는 작업경로는 빼고, 새 활동이 생긴 작업경로는 지운 목록에서 꺼낸다
        var visible: [Group] = []
        for g in grouped {
            if let at = hiddenGroups[g.id] {
                if g.sessions.allSatisfy({ $0.updated.timeIntervalSince1970 <= at }) { continue }
                hiddenGroups[g.id] = nil
                UserDefaults.standard.set(hiddenGroups, forKey: "hiddenGroups")
            }
            visible.append(g)
        }
        foldQuiet(visible)
        let pins = pinnedGroups
        let sorted = visible.sorted {
            let pa = pins.firstIndex(of: $0.id) ?? Int.max, pb = pins.firstIndex(of: $1.id) ?? Int.max
            if pa != pb { return pa < pb }
            let a = $0.sessions.map(\.rank).min() ?? 9, b = $1.sessions.map(\.rank).min() ?? 9
            if a != b { return a < b }
            return ($0.sessions.map(\.updated).max() ?? .distantPast) > ($1.sessions.map(\.updated).max() ?? .distantPast)
        }
        if force || sorted != groups {
            withAnimation(.spring(response: 0.45, dampingFraction: 0.82)) { groups = sorted }
        }
    }
}

// claude 프로세스별 백그라운드 셸 수 — Claude Code 는 Bash 도구를 shell-snapshots 를 읽는 zsh/bash 자식으로 띄운다.
// 응답이 끝난 뒤에도 남아 있는 이 자식은 run_in_background 로 띄운 셸이다
func backgroundShells() -> [Int32: Int] {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/bin/ps")
    p.arguments = ["-axo", "ppid=,command="]
    let pipe = Pipe()
    p.standardOutput = pipe
    p.standardError = FileHandle.nullDevice
    guard (try? p.run()) != nil else { return [:] }
    let out = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    p.waitUntilExit()
    var counts: [Int32: Int] = [:]
    for line in out.split(separator: "\n") where line.contains("/.claude/shell-snapshots/snapshot-") {
        let t = line.trimmingCharacters(in: .whitespaces)
        if let ppid = Int32(t.prefix { $0.isNumber }) { counts[ppid, default: 0] += 1 }
    }
    return counts
}

private func formatter(_ format: String) -> DateFormatter {
    let f = DateFormatter()
    f.locale = Locale(identifier: "ko_KR")
    f.dateFormat = format
    return f
}
private let hourMinute = formatter("HH:mm"), dayHourMinute = formatter("M/d(E) HH:mm")

// 종료된 세션은 흐르는 시간 대신 끝난 시각을 고정해서 보여 준다
func clock(_ date: Date) -> String { hourMinute.string(from: date) }

func elapsed(_ from: Date, _ now: Date) -> String {
    let s = max(0, Int(now.timeIntervalSince(from)))
    if s < 60 { return "\(s)초" }
    if s < 3600 { return "\(s / 60)분 \(s % 60)초" }
    return "\(s / 3600)시간 \(s % 3600 / 60)분"
}

// MARK: - 상태 표시 (애니메이션)

// 늘 도는 애니메이션의 시계 — SwiftUI 반복 애니메이션은 화면 주사율만큼 창 전체를 다시 그려 클릭이 버벅이므로 초당 30번으로 묶는다
// 늘 도는 애니메이션은 시계 하나를 함께 본다 — 따로 돌면 시계마다 창 전체를 따로 다시 그린다
final class AmbientTicker: ObservableObject {
    static let shared = AmbientTicker()
    @Published private(set) var now = Date().timeIntervalSinceReferenceDate
    private var timer: Timer?
    // 창이 다른 창에 다 가려졌거나 숨었으면 멈춘다 — 보이지 않는 감독·회전 표시를 초당 30번 다시 그리지 않게
    var running = true { didSet { if running != oldValue { schedule() } } }
    private init() { schedule() }
    private func schedule() {
        timer?.invalidate()
        timer = nil
        guard running else { return }
        let t = Timer(timeInterval: 1.0 / 30, repeats: true) { [weak self] _ in self?.now = Date().timeIntervalSinceReferenceDate }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }
}
struct Ambient<Content: View>: View {
    @ObservedObject private var ticker = AmbientTicker.shared
    @ViewBuilder let content: (TimeInterval) -> Content
    var body: some View { content(ticker.now) }
}
func cycle(_ t: TimeInterval, _ period: Double) -> Double { (t / period).truncatingRemainder(dividingBy: 1) }
// 0 → 1 → 0 을 부드럽게 오가는 값 (period 동안 한 번 왕복)
func swing(_ t: TimeInterval, _ period: Double) -> Double { (1 - cos(2 * .pi * cycle(t, period))) / 2 }

// 작업 중: 그라데이션 호가 계속 회전한다
struct Spinner: View {
    let color: Color
    var body: some View {
        Ambient { now in
        ZStack {
            Circle().stroke(color.opacity(0.18), lineWidth: 2.5)
            Circle()
                .trim(from: 0, to: 0.72)
                .stroke(AngularGradient(colors: [color.opacity(0), color], center: .center),
                        style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                .rotationEffect(.degrees(cycle(now, 0.9) * 360))
        }
        }
    }
}

// 승인 대기: 점 둘레로 파문이 퍼진다
struct Pulse: View {
    let color: Color
    var body: some View {
        Ambient { now in
            let p = cycle(now, 1.1)
            let e = 1 - (1 - p) * (1 - p)   // easeOut
            ZStack {
                Circle().fill(color.opacity(0.35)).scaleEffect(0.8 + 1.1 * e).opacity(1 - e)
                Circle().fill(color).frame(width: 9, height: 9)
            }
        }
    }
}

// 대기: 체크 표시가 통통 튀며 나타난다
struct Check: View {
    let color: Color
    @State private var shown = false
    var body: some View {
        Image(systemName: "checkmark.circle.fill")
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(color)
            .scaleEffect(shown ? 1 : 0.3)
            .opacity(shown ? 1 : 0)
            .onAppear { withAnimation(.spring(response: 0.35, dampingFraction: 0.5)) { shown = true } }
    }
}

struct StateIcon: View {
    let state: String
    var body: some View {
        let c = Palette.color(state)
        ZStack {
            switch state {
            case "working": Spinner(color: c)
            case "waiting": Pulse(color: c)
            case "idle": Check(color: c)
            case "paused": Image(systemName: "pause.circle.fill").font(.system(size: 16, weight: .semibold)).foregroundStyle(c)
            default:
                Image(systemName: "minus.circle.fill").font(.system(size: 15)).foregroundStyle(Color(red: 0.93, green: 0.33, blue: 0.33))
            }
        }
        .frame(width: 18, height: 18)
        .id(state)   // 상태가 바뀌면 새로 그려 등장 애니메이션을 다시 낸다
        .transition(.scale.combined(with: .opacity))
    }
}

// 작업 중인 줄 위를 은은한 빛이 지나간다
struct Shimmer: ViewModifier {
    let active: Bool
    let color: Color
    func body(content: Content) -> some View {
        content.overlay {
            if active {
                GeometryReader { geo in
                    Ambient { now in
                        let phase = -1 + 2 * cycle(now, 2.4)
                        LinearGradient(colors: [.clear, color.opacity(0.10), .clear], startPoint: .leading, endPoint: .trailing)
                            .frame(width: geo.size.width * 0.5)
                            .offset(x: phase * geo.size.width * 1.5)
                    }
                }
                .allowsHitTesting(false)
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }
        }
    }
}

// MARK: - 화면

// 세션 실행 방식 라벨: 터미널 창에서 도는 세션인지, 터미널 없이 백그라운드로 도는 세션인지
struct KindLabel: View {
    var headless = false
    var subagent = false
    var compact = false     // 줄이 좁으면 글자를 빼고 아이콘만 보인다
    var body: some View {
        let c = subagent ? Color(red: 0.95, green: 0.55, blue: 0.75)
            : headless ? Color(red: 0.72, green: 0.55, blue: 1.00) : Color(red: 0.32, green: 0.78, blue: 0.86)
        let name = subagent ? "서브에이전트" : headless ? "백그라운드" : "터미널"
        Label(name, systemImage: subagent ? "person.2.fill" : headless ? "moon.stars.fill" : "terminal.fill")
            .boardFont(10, weight: .semibold)
            .labelStyle(compact ? AnyLabelStyle(.iconOnly) : AnyLabelStyle(.titleAndIcon))
            .fixedSize()
            .hint(name)
            .foregroundStyle(c)
            .padding(.horizontal, 6)
            .padding(.vertical, 1.5)
            .background(Capsule().fill(c.opacity(0.15)))
            .overlay(Capsule().stroke(c.opacity(0.25), lineWidth: 0.5))
    }
}

// 세션 칩 — 어디서 도는지: 보드 탭 CB(탭 안에서 시작된 claude -p 는 CB -p), 다른 터미널은 앱 이름,
// 백그라운드는 실제 실행 방식($ claude -p·--bg). 끝난 세션은 누르면 보드에서 이어 연다는 표시
struct PlaceChip: View {
    let s: Session
    let place: SessionPlace
    let appName: String?
    let mode: String?   // -p·--bg — 대화형이면 nil

    private func chip(_ text: String, _ bg: Color, _ fg: Color, mono: Bool = false, hint: String) -> some View {
        Text(text)
            .boardFont(10, weight: .semibold, design: mono ? .monospaced : .default)
            .foregroundStyle(fg)
            .padding(.horizontal, 6).padding(.vertical, 1.5)
            .background(Capsule().fill(bg))
            .fixedSize()
            .hint(hint)
    }

    var body: some View {
        switch place {
        case .tab(_, let headless):
            chip(mode.map { "CB \($0)" } ?? "CB", Color(red: 0.18, green: 0.23, blue: 0.34), Color(red: 0.66, green: 0.76, blue: 1.0),
                 hint: headless || mode != nil ? "보드 탭 안에서 시작된 claude \(mode ?? "-p") (탭이 닫히면 함께 끝납니다)" : "보드 탭에서 도는 세션: 누르면 그 탭으로")
        case .terminal:
            if let appName {
                chip(appName, Color(red: 0.14, green: 0.25, blue: 0.29), Color(red: 0.56, green: 0.85, blue: 0.91), hint: "\(appName) 에서 도는 세션")
            }
        case .background:
            chip("$ claude \(mode ?? "-p")", Color(red: 0.23, green: 0.20, blue: 0.27), Color(red: 0.83, green: 0.72, blue: 1.0), mono: true,
                 hint: "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다")
        case .ended:
            if s.headless {
                chip("$ claude -p", Color.white.opacity(0.08), .secondary, mono: true, hint: "끝난 백그라운드 세션입니다. 메뉴의 세션 다시 열기로 보드 탭에서 엽니다")
            } else {
                chip("보드에서 이어 열기", Color.white.opacity(0.08), .secondary, hint: "누르면 보드 새 탭에서 이 대화를 이어 엽니다")
            }
        }
    }
}

// 세션 이름 라벨 — Claude Code 의 세션 이름(세션끼리 메시지를 보낼 때 주소). 보드 탭 세션은 탭 줄 이름과 같다.
// 긴 이름은 가운데를 줄여 끝의 번호(-2 등)가 보이게 한다
struct SessionNameLabel: View {
    let name: String
    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: "tag").font(.system(size: 8, weight: .semibold))
            Text(name.count > 24 ? "\(name.prefix(11))…\(name.suffix(11))" : name)
        }
        .boardFont(10, weight: .semibold)
        .foregroundStyle(Color.white.opacity(0.72))
        .padding(.horizontal, 6)
        .padding(.vertical, 1.5)
        .background(Capsule().fill(Color.white.opacity(0.08)))
        .fixedSize()
        .overlay(Capsule().stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .hint("세션 이름 \(name)")
    }
}

func tokens(_ n: Int) -> String {
    n >= 1_000_000 ? String(format: "%.1fM", Double(n) / 1_000_000) : n >= 1000 ? "\(n / 1000)K" : "\(n)"
}

// 컨텍스트 사용량 라벨: 작은 막대 + 비율 + 토큰 수 (30% 미만 초록, 70% 미만 노랑, 그 이상 빨강 — 상태줄과 같은 기준)
struct ContextLabel: View {
    let ctx: Ctx
    var level = 0           // 0 막대·비율·토큰, 1 막대 없이 비율·토큰, 2 비율만 — 줄이 좁을수록 줄인다
    var body: some View {
        let c = ctx.pct < 30 ? Color(red: 0.42, green: 0.85, blue: 0.60)
            : ctx.pct < 70 ? Color(red: 1.00, green: 0.80, blue: 0.35) : Color(red: 1.00, green: 0.42, blue: 0.42)
        HStack(spacing: 4) {
            Image(systemName: "brain.head.profile").font(.system(size: 9, weight: .semibold))
            if level == 0 {
                ZStack(alignment: .leading) {
                    Capsule().fill(c.opacity(0.18))
                    Capsule().fill(c).frame(width: max(2, 26 * CGFloat(ctx.pct) / 100))
                }
                .frame(width: 26, height: 4)
            }
            Text(level == 2 ? "\(ctx.pct)%" : "\(ctx.pct)% · \(tokens(ctx.used))").monospacedDigit().contentTransition(.numericText())
        }
        .boardFont(10, weight: .semibold)
        .foregroundStyle(c)
        .padding(.horizontal, 6)
        .padding(.vertical, 1.5)
        .background(Capsule().fill(c.opacity(0.12)))
        .fixedSize()   // 정보 줄이 길어져도 라벨 안 글자가 두 줄로 꺾이지 않게
        .overlay(Capsule().stroke(c.opacity(0.22), lineWidth: 0.5))
        .hint("컨텍스트 \(ctx.used.formatted()) / \(ctx.size.formatted()) 토큰")
        .animation(.easeInOut(duration: 0.4), value: ctx)
    }
}

// 모델·effort 라벨: 계열 이름(색으로 계열 구분) + effort 막대 5칸(low 1 … max 5)
struct ModelLabel: View {
    let brain: Brain
    var level = 0           // 0 계열·버전, 1 계열만, 2 계열 첫 글자 — 줄이 좁을수록 줄인다
    var body: some View {
        let c = Self.color(brain.family)
        HStack(spacing: 4) {
            Text(level == 0 ? [brain.family, brain.version].filter { !$0.isEmpty }.joined(separator: " ")
                 : level == 1 ? brain.family : String(brain.family.prefix(1)))
            if brain.effortLevel > 0 { EffortBars(level: brain.effortLevel, color: c) }
        }
        .boardFont(10, weight: .semibold)
        .foregroundStyle(c)
        .padding(.horizontal, 6)
        .padding(.vertical, 1.5)
        .background(Capsule().fill(c.opacity(0.12)))
        .fixedSize()
        .overlay(Capsule().stroke(c.opacity(0.22), lineWidth: 0.5))
        .hint("모델 \(brain.model)" + (brain.effort.isEmpty ? "" : " · effort \(brain.effort)"))
    }

    static func color(_ family: String) -> Color {
        switch family {
        case "Opus": Color(red: 1.00, green: 0.60, blue: 0.45)
        case "Sonnet": Color(red: 0.48, green: 0.70, blue: 1.00)
        case "Haiku": Color(red: 0.55, green: 0.88, blue: 0.80)
        case "Fable": Color(red: 0.88, green: 0.70, blue: 1.00)
        default: Color(white: 0.75)
        }
    }
}

// effort 막대 — 키가 점점 커지는 5칸, 채운 칸 수 = effort 단계
struct EffortBars: View {
    let level: Int
    let color: Color
    var body: some View {
        HStack(alignment: .bottom, spacing: 1.2) {
            ForEach(1...5, id: \.self) { i in
                RoundedRectangle(cornerRadius: 0.8)
                    .fill(i <= level ? color : color.opacity(0.22))
                    .frame(width: 2.2, height: 3 + CGFloat(i) * 1.3)
            }
        }
        .frame(height: 9.5, alignment: .bottom)
    }
}

// 남은 시간 + 초기화 시각 — 하루 넘게 남으면 날짜를 붙인다
func resetText(_ d: Date?, _ now: Date) -> String {
    guard let d else { return "" }
    let s = Int(d.timeIntervalSince(now))
    if s <= 0 { return "곧 초기화" }
    let left = s >= 86400 ? "\(s / 86400)일 \(s % 86400 / 3600)시간" : s >= 3600 ? "\(s / 3600)시간 \(s % 3600 / 60)분" : "\(max(1, s / 60))분"
    return "\(left) 뒤 · \((s >= 86400 ? dayHourMinute : hourMinute).string(from: d))"
}

// 사용 한도 카드: 5시간·주간 전체·주간 모델별 막대. 머리에 로그인 계정(눌러서 전환), 오른쪽 위 단추로 간략(한 줄)·상세를 바꾼다
struct UsagePanel: View {
    @ObservedObject var usage: UsageStore
    @ObservedObject var accounts: AccountStore
    let now: Date
    var forceCompact = false    // 창이 낮아 중간 화면일 때 — 저장한 간략·상세 설정은 두고 한 줄로만 보인다
    var stoplines: [String: Double]? = nil   // 자율주행 보기: 한도 id(5h·7d) → 멈출 선(%) 눈금
    let busy: () -> [Session]
    @AppStorage("usageCompact") private var compactSetting = false
    // 자율주행 보기는 멈출 선 눈금이 보이게 상세로(간략 한 줄은 좁으면 막대를 뺀다). 중간 화면은 그래도 한 줄
    private var compact: Bool { (compactSetting && stoplines == nil) || forceCompact }

    static func color(_ p: Double) -> Color {
        p < 50 ? Color(red: 0.42, green: 0.85, blue: 0.60) : p < 80 ? Color(red: 1.00, green: 0.70, blue: 0.36)
            : Color(red: 1.00, green: 0.42, blue: 0.42)
    }

    private var status: some View {
        Label(usage.tokenExpired ? "로그인 토큰 갱신 전: 세션이 다음 요청을 보내면 보입니다"
              : usage.failed ? "사용량을 불러오지 못했습니다" : "사용량 불러오는 중…",
              systemImage: "gauge.with.dots.needle.33percent")
            .boardFont(11).foregroundStyle(.secondary).lineLimit(1)
    }

    // 간략 모드 한 줄 — 넓으면 이름·막대·비율, 좁으면 막대를 빼고, 더 좁으면 이름을 줄인다
    private func line(_ level: Int) -> some View {
        HStack(spacing: 8) {
            ForEach(Array(usage.limits.enumerated()), id: \.element.id) { i, l in
                if i > 0 { Rectangle().fill(Color.white.opacity(0.14)).frame(width: 0.5, height: 12) }
                CompactLimit(limit: l, level: level, now: now, stopline: stoplines?[l.id])
            }
        }
        .fixedSize()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 0 : 9) {
            if compact {
                HStack(spacing: 8) {
                    AccountChip(accounts: accounts, compact: true, busy: busy)
                    if usage.limits.isEmpty {
                        status
                    } else {
                        ViewThatFits(in: .horizontal) { line(0); line(1); line(2) }
                    }
                    Spacer(minLength: 0)
                    if !forceCompact { UsageModeButton(compact: $compactSetting) }   // 중간 화면에서는 바꿔도 보이지 않으므로 숨긴다
                }
            } else {
                HStack(spacing: 8) {
                    AccountChip(accounts: accounts, compact: false, busy: busy)
                    Spacer(minLength: 4)
                    UsageModeButton(compact: $compactSetting)
                }
                if usage.limits.isEmpty { status }
                limitRows
            }
            if let n = accounts.notice {
                Label(n.text, systemImage: n.error ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .boardFont(10.5, weight: .medium)
                    .foregroundStyle(n.error ? Color(red: 1.00, green: 0.58, blue: 0.45) : Color(red: 0.42, green: 0.85, blue: 0.60))
                    .lineLimit(2)
                    .padding(.top, compact ? 6 : 0)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(.horizontal, compact ? 8 : 12)
        .padding(.vertical, compact ? 6 : 12)
        .background(RoundedRectangle(cornerRadius: 14).fill(Color.white.opacity(0.05)))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Color.white.opacity(0.08), lineWidth: 0.5))
        .animation(.spring(response: 0.6, dampingFraction: 0.85), value: usage.limits)
        .animation(.easeOut(duration: 0.2), value: accounts.notice)
    }

    private var limitRows: some View {
        ForEach(usage.limits) { l in
            let c = Self.color(l.pct)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(l.label).boardFont(12, weight: .semibold).lineLimit(1).layoutPriority(1)
                    Spacer(minLength: 4)
                    if let line = stoplines?[l.id] {
                        Text("멈출 선 \(Int(line.rounded()))%").boardFont(10.5).foregroundStyle(.secondary).lineLimit(1).fixedSize()
                    }
                    Text(resetText(l.resets, now)).boardFont(10.5).foregroundStyle(.secondary)
                        .monospacedDigit()
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                    Text("\(Int(l.pct.rounded()))%")
                        .boardFont(13, weight: .bold).monospacedDigit()
                        .foregroundStyle(c)
                        .frame(minWidth: 38, alignment: .trailing)
                        .contentTransition(.numericText())
                }
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule().fill(Color.white.opacity(0.08))
                        Capsule()
                            .fill(LinearGradient(colors: [c.opacity(0.65), c], startPoint: .leading, endPoint: .trailing))
                            .frame(width: max(4, geo.size.width * CGFloat(min(l.pct, 100)) / 100))
                            .shadow(color: c.opacity(0.45), radius: 4)
                        if let line = stoplines?[l.id] {   // 자율주행 멈출 선 — 닿으면 지금 단위는 마치고 새 단위는 초기화 뒤에
                            Rectangle().fill(Color.white.opacity(0.85)).frame(width: 1.5, height: 11)
                                .offset(x: geo.size.width * CGFloat(min(line, 100)) / 100 - 0.75)
                        }
                    }
                }
                .frame(height: 6)
            }
            .transition(.opacity)
        }
    }
}

// 간략 모드의 한도 하나 — 이름 · 작은 막대 · 비율. 마우스를 올리면 초기화 시각
struct CompactLimit: View {
    let limit: Limit
    let level: Int          // 0 이름·막대·비율, 1 이름·비율, 2 짧은 이름·비율
    let now: Date
    var stopline: Double? = nil   // 자율주행 보기의 멈출 선(%) — 작은 막대에 눈금
    var body: some View {
        let c = UsagePanel.color(limit.pct)
        HStack(spacing: 4) {
            Text(level == 2 ? limit.tiny : limit.short)
                .boardFont(11, weight: .semibold)
                .foregroundStyle(.secondary)
            if level == 0 {
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.white.opacity(0.10))
                    Capsule().fill(c).frame(width: max(2, 26 * CGFloat(min(limit.pct, 100)) / 100))
                    if let s = stopline {
                        Rectangle().fill(Color.white.opacity(0.85)).frame(width: 1, height: 7).offset(x: 26 * CGFloat(min(s, 100)) / 100 - 0.5)
                    }
                }
                .frame(width: 26, height: 4)
            }
            Text("\(Int(limit.pct.rounded()))%")
                .boardFont(12, weight: .bold).monospacedDigit()
                .foregroundStyle(c)
                .contentTransition(.numericText())
        }
        .hint("\(limit.label) \(Int(limit.pct.rounded()))% · \(resetText(limit.resets, now))" + (stopline.map { " · 자율주행 멈출 선 \(Int($0.rounded()))%" } ?? ""))
    }
}

// 사용 한도 카드 오른쪽 위의 간략·상세 바꾸기 단추
struct UsageModeButton: View {
    @Binding var compact: Bool
    @State private var hover = false
    var body: some View {
        Button { withAnimation(foldSpring) { compact.toggle() } } label: {
            Image(systemName: compact ? "rectangle.expand.vertical" : "rectangle.compress.vertical")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(hover ? .primary : .secondary)
                .frame(width: 24, height: 20)
                .background(Capsule().fill(Color.white.opacity(hover ? 0.14 : 0.06)))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(compact ? "자세히 보기: 한도마다 막대와 초기화 시각" : "간략히 보기: 한 줄로")
    }
}

// 지금 로그인 계정 — 누르면 저장해 둔 계정으로 전환하거나 계정을 추가하는 메뉴
struct AccountChip: View {
    @ObservedObject var accounts: AccountStore
    var compact = false     // 간략 모드에서는 아이콘만 — 계정은 도움말로 보인다
    let busy: () -> [Session]
    @State private var hover = false

    var body: some View {
        let c = accounts.current
        Button(action: showMenu) {
            HStack(spacing: 5) {
                Image(systemName: accounts.adding ? "person.crop.circle.badge.plus"
                      : accounts.switching ? "arrow.triangle.2.circlepath.circle.fill" : "person.crop.circle.fill")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Palette.accent)
                    .contentTransition(.symbolEffect(.replace))
                if !compact {
                    Text(accounts.adding ? "로그인 기다리는 중…" : c?.email ?? "로그인 정보 없음")
                        .boardFont(11.5, weight: .semibold)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if !accounts.adding, let p = c?.plan, !p.isEmpty {
                        Text(p)
                            .boardFont(9.5, weight: .bold)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .background(Capsule().fill(Color.white.opacity(0.08)))
                            .fixedSize()
                    }
                }
                if accounts.autoSwitch {
                    Image(systemName: "arrow.triangle.swap").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                }
                Image(systemName: "chevron.down").font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            }
            .padding(.leading, compact ? 4 : 5).padding(.trailing, compact ? 6 : 8).padding(.vertical, 3)
            .background(Capsule().fill(Color.white.opacity(hover ? 0.12 : 0.05)))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("로그인 계정 \(c?.email ?? "없음")\(c.map { $0.org.isEmpty ? "" : " · \($0.org)" } ?? "")"
              + (accounts.autoSwitch ? " · 한도 자동 전환 켜짐" : "") + "\n눌러서 다른 계정으로 전환하거나 계정을 추가합니다")
    }

    private func showMenu() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let head = NSMenuItem(title: "Claude 계정: 고르면 모든 세션이 그 계정으로 이어 갑니다", action: nil, keyEquivalent: "")
        head.isEnabled = false
        menu.addItem(head)
        var list = accounts.saved
        if let c = accounts.current, !list.contains(where: { $0.id == c.id }) { list.insert(c, at: 0) }
        for a in list {
            let isCurrent = a.id == accounts.current?.id
            let title = [a.email, a.plan].filter { !$0.isEmpty }.joined(separator: "  ·  ")
                + (!isCurrent && a.expired ? "  (다시 로그인 필요)" : "")
            let item = ActionItem(title, enabled: !accounts.switching) {
                guard !isCurrent else { return }
                a.expired ? accounts.beginAdd(email: a.email) : confirmSwitch(a)
            }
            item.state = isCurrent ? .on : .off
            if !a.org.isEmpty { item.toolTip = a.org }
            menu.addItem(item)
        }
        menu.addItem(.separator())
        if accounts.adding {
            menu.addItem(ActionItem("계정 추가 취소 (로그인 기다리는 중)") { accounts.cancelAdd() })
        } else {
            menu.addItem(ActionItem("계정 추가… (터미널·브라우저에서 로그인)") { accounts.beginAdd() })
        }
        let removable = accounts.saved.filter { $0.id != accounts.current?.id }
        if !removable.isEmpty {
            let sub = NSMenu()
            for a in removable { sub.addItem(ActionItem(a.email) { accounts.remove(a) }) }
            let item = NSMenuItem(title: "저장한 계정 지우기", action: nil, keyEquivalent: "")
            item.submenu = sub
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let auto = ActionItem("한도가 차면 다른 계정으로 자동 전환") { accounts.autoSwitch.toggle() }
        auto.state = accounts.autoSwitch ? .on : .off
        auto.toolTip = "5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 멈춘 세션(Ghostty)을 이어서 진행합니다"
        menu.addItem(auto)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }

    // 작업 중·승인 대기 세션이 있으면 이전 계정으로 일하는 중이라고 알리고 한 번 더 묻는다
    private func confirmSwitch(_ a: Account) {
        guard TerminalHub.shared.slide == nil else { return whenPaneSettled { confirmSwitch(a) } }
        let working = busy()
        if !working.isEmpty {
            let names = working.prefix(5).map { s in
                "• \((s.cwd as NSString).lastPathComponent) · \(s.topic.isEmpty ? Palette.name(s.state) : s.topic)"
            }
            let alert = NSAlert()
            alert.messageText = "이전 계정으로 작업 중인 세션이 있습니다"
            alert.informativeText = """
            \(accounts.current?.email ?? "지금") 계정으로 작업 중이거나 승인을 기다리는 세션이 \(working.count)개 있습니다.
            \(names.joined(separator: "\n"))\(working.count > 5 ? "\n… 외 \(working.count - 5)개" : "")

            전환하면 모든 세션이 다음 요청부터 \(a.email) 계정으로 이어서 동작합니다(30초 안에 반영). 지금 받고 있는 응답은 이전 계정으로 마무리됩니다.

            계정을 전환할까요?
            """
            alert.addButton(withTitle: "전환")
            alert.addButton(withTitle: "취소")
            NSApp.activate(ignoringOtherApps: true)
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        accounts.switchTo(a)
    }
}

// 정보 라벨을 HStack 처럼 늘어놓다가 줄 폭을 넘는 라벨부터 다음 줄로 — 세션·서브에이전트 줄의 가장 줄인 모양도 안 들어갈 때.
// 폭을 정하지 않고 물으면(ViewThatFits 가 이상적인 크기를 잴 때) 한 줄 크기
struct WrapRow: Layout {
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(proposal.width, subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (v, p) in zip(subviews, arrange(bounds.width, subviews).points) {
            v.place(at: CGPoint(x: bounds.minX + p.x, y: bounds.minY + p.y), proposal: .unspecified)
        }
    }

    // 라벨마다 왼쪽 위 자리와 전체 크기. 줄 안에서는 HStack 처럼 세로 가운데에 맞춘다
    private func arrange(_ width: CGFloat?, _ subviews: Subviews) -> (points: [CGPoint], size: CGSize) {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        var lines: [Range<Int>] = [], start = 0, x: CGFloat = 0
        for (i, s) in sizes.enumerated() {
            if let width, i > start, x + spacing + s.width > width {
                lines.append(start..<i)
                start = i
            }
            x = i == start ? s.width : x + spacing + s.width
        }
        if !sizes.isEmpty { lines.append(start..<sizes.count) }
        var points = [CGPoint](repeating: .zero, count: sizes.count)
        var y: CGFloat = 0, w: CGFloat = 0
        for line in lines {
            let h = line.map { sizes[$0].height }.max() ?? 0
            var x: CGFloat = 0
            for i in line {
                points[i] = CGPoint(x: x, y: y + (h - sizes[i].height) / 2)
                x += sizes[i].width + spacing
            }
            w = max(w, x - spacing)
            y += h + 3
        }
        return (points, CGSize(width: w, height: max(0, y - 3)))
    }
}

struct SessionRow: View {
    let s: Session
    let now: Date
    @ObservedObject private var hub = TerminalHub.shared
    @State private var hover = false
    // 서브에이전트 트리를 접어 둔 세션 id(쉼표로 이음) — 앱을 다시 켜도 유지한다
    @AppStorage("subCollapsed") private var subCollapsed = ""

    private var subOpen: Bool { !subCollapsed.split(separator: ",").contains(Substring(s.id)) }

    private func toggleSubs() {
        var ids = Set(subCollapsed.split(separator: ",").map(String.init))
        if ids.contains(s.id) { ids.remove(s.id) } else { ids.insert(s.id) }
        withAnimation(foldSpring) { subCollapsed = ids.sorted().joined(separator: ",") }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            row
            if subOpen {
                ForEach(Array(s.subagents.enumerated()), id: \.element.id) { i, sub in
                    SubagentRow(sub: sub, now: now, last: i == s.subagents.count - 1)
                        .transition(foldIn(i))
                }
            }
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: s.subagents)
    }

    private func meta(level: Int, wrap: Bool = false, name: Bool = true) -> some View {
        let compact = level > 0
        return (wrap ? AnyLayout(WrapRow(spacing: 5)) : AnyLayout(HStackLayout(spacing: 5))) {
            Text(s.state == "ended" ? "\(clock(s.updated)) 종료"
                 : s.state == "idle" ? "\(elapsed(s.since, now)) 전" : elapsed(s.since, now))
                .monospacedDigit()
                .fixedSize()
                .contentTransition(.numericText())
            PlaceChip(s: s, place: hub.place(s), appName: hub.appNames[s.id], mode: s.state == "ended" ? nil : hub.mode(s))
            // 세션 이름 — 자리가 모자라면 먼저 뺀다(아래 ViewThatFits). 미니 화면은 세션 줄이 없어 보이지 않는다
            if name, let n = s.name { SessionNameLabel(name: n) }
            // 터미널·백그라운드 구분은 작업경로 카드 안 구역 머리글이 맡는다
            if !s.subagents.isEmpty || s.shells > 0 || s.ctx != nil || s.brain != nil { Text("·") }
            if !s.subagents.isEmpty {
                SubToggle(count: s.subagents.count,
                          running: s.subagents.filter { $0.state != "done" }.count,
                          open: subOpen,
                          toggle: toggleSubs,
                          compact: compact)
            }
            if s.shells > 0 {
                Label(compact ? "\(s.shells)" : "셸 \(s.shells)개 실행 중", systemImage: "apple.terminal.on.rectangle")
                    .labelStyle(.titleAndIcon)
                    .foregroundStyle(Palette.color("working"))
                    .fixedSize()
                    .hint("셸 \(s.shells)개 실행 중")
            }
            if let b = s.brain { ModelLabel(brain: b, level: level) }
            if let ctx = s.ctx { ContextLabel(ctx: ctx, level: level) }
        }
        .boardFont(11)
        .foregroundStyle(.secondary)
    }

    private var row: some View {
        let c = Palette.color(s.state)
        return HStack(alignment: .top, spacing: 10) {
            HStack(spacing: 4) {
                // 서브에이전트가 있는 세션만 접기·펼치기 화살표를 보인다 — 없는 세션도 자리는 비워 줄을 맞춘다
                if s.subagents.isEmpty {
                    Color.clear.frame(width: 14, height: 18)
                } else {
                    FoldChevron(open: subOpen, toggle: toggleSubs)
                }
                StateIcon(state: s.state)
                    .hint(Palette.name(s.state))   // 상태 배지를 뺐으므로 아이콘에 상태 이름을 단다
            }
            .padding(.top, 1)
            VStack(alignment: .leading, spacing: 3) {
                // 첫 줄: 제목 + 오른쪽 단추들. 정보 라벨은 둘째 줄을 가로로 다 쓴다 — 창이 좁아도 넘치지 않게
                HStack(alignment: .center, spacing: 8) {
                    Text(s.title)
                        .boardFont(13, weight: .medium)
                        .foregroundStyle(s.state == "ended" ? .secondary : .primary)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    if s.state != "ended" { MuteToggle(s: s) }
                    // 도는 백그라운드 세션은 메뉴가 없다 — 보드는 그 세션을 멈추거나 끝내지 않는다(일시중지도 터미널이 없어 안 된다)
                    if !(s.state != "ended" && hub.place(s) == .background) { SessionMenu(s: s) }
                    if s.state == "ended" {
                        Button { try? FileManager.default.removeItem(atPath: s.file) } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 14))
                                .foregroundStyle(.secondary.opacity(hover ? 1 : 0.5))
                        }
                        .buttonStyle(.plain)
                        .hint("목록에서 지우기")
                    }
                }
                // 다 들어가면 글자까지, 좁으면 종류 라벨은 아이콘만·하위 작업은 아이콘 + 개수만, 그래도 좁으면 세션 이름을 빼고, 더 좁으면 컨텍스트 막대·토큰을 뺀다.
                // 그래도 안 들어가면(보드 최소 폭·큰 보드 글자) 다음 줄로 넘긴다 — 한 줄을 고집하면 세션 줄이 넓어져 카드가 보드 밖으로 밀린다
                ViewThatFits(in: .horizontal) {
                    meta(level: 0)
                    meta(level: 1)
                    meta(level: 1, name: false)
                    meta(level: 2, name: false)
                    meta(level: 2, wrap: true, name: false)
                }
                .boardFont(11)
                .foregroundStyle(.secondary)
                if s.state == "waiting", !s.message.isEmpty {
                    Label(s.message, systemImage: "hand.raised.fill")
                        .boardFont(11, weight: .medium)
                        .foregroundStyle(c)
                        .lineLimit(2)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hover ? 0.07 : 0)))
        .modifier(Shimmer(active: s.state == "working", color: c))
        .contentShape(Rectangle())
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
        // 한 번 클릭·더블 클릭을 제스처 둘로 나누면 SwiftUI 가 더블 클릭 여부를 기다리느라 줄 안의 스위치·단추까지 0.5초 늦게 반응한다.
        // 제스처 하나로 받고 macOS 클릭 횟수로 나눈다 — 더블 클릭은 첫 클릭(앱 활성화) 뒤에 둘째 클릭에서 포커스·재개
        .onTapGesture { openSession(s, double: (NSApp.currentEvent?.clickCount ?? 1) >= 2) }
        .animation(.spring(response: 0.4, dampingFraction: 0.8), value: s.state)
    }
}

// 세션 줄 오른쪽 위의 … 메뉴 — 일시중지·재개·중단. 누르면 보드 모양의 메뉴 패널(SessionMenuPanel)을 단추 아래에 띄운다
struct SessionMenu: View {
    let s: Session
    @State private var hover = false
    @State private var anchor = MenuAnchor()

    // SwiftUI Menu(borderlessButton) 는 AppKit 팝업 단추라 확대하면 마우스 감지 영역까지 커졌다 줄어 떨리고,
    // 커서도 스스로 화살표로 되돌린다 — 별·X 단추와 같은 일반 버튼으로 두고 누르면 메뉴 패널을 직접 띄운다
    var body: some View {
        Button { SessionMenuPanel.shared.toggle(s, from: anchor.view) } label: {
            Image(systemName: "ellipsis")
                .font(.system(size: 12, weight: .bold))
                .foregroundStyle(.primary.opacity(hover ? 0.95 : 0.55))
                .frame(width: 26, height: 22)
                .background(Capsule().fill(Color.white.opacity(hover ? 0.16 : 0.06)))
                .contentShape(Capsule())
                .scaleEffect(hover ? 1.3 : 1)
                .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hover)
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("세션 일시중지·재개·중단")
        .background(MenuAnchorView(anchor: anchor))
    }
}

// 메뉴 패널을 붙일 자리 — 단추 뒤에 깔린 빈 NSView 를 기억한다(클릭은 받지 않는다)
final class MenuAnchor { weak var view: NSView? }

struct MenuAnchorView: NSViewRepresentable {
    let anchor: MenuAnchor
    final class Spot: NSView { override func hitTest(_ point: NSPoint) -> NSView? { nil } }
    func makeNSView(context: Context) -> NSView {
        let v = Spot()
        anchor.view = v
        return v
    }
    func updateNSView(_ v: NSView, context: Context) { anchor.view = v }
}

// … 메뉴 패널 — 보드와 같은 어두운 둥근 패널. 맨 위에 세션 이름·상태·칩, 그 아래 항목마다 아이콘 + 제목 + 한 줄 설명, 세션 중단은 구분선 아래 빨간색.
// 마우스를 올리거나 ↑↓ 로 고르고 Return·클릭으로 실행, Esc·바깥 클릭·스크롤·다른 앱으로 넘어감·보드 창 이동이면 닫는다.
// 패널은 열린 동안 키 창이 된다(SwiftUI 의 마우스 올림은 키 창에서만 온다). 키는 앱 안 이벤트 감시로 먼저 받고, 닫으면 보드 창이 다시
// 키 창이 되어 터미널 포커스가 돌아온다. 열린 동안 보드는 불투명하게 둔다(WindowOpacity).
// 항목과 조건은 예전 NSMenu 와 같고 동작 함수(resumeSession·resumePaused·pauseSession·moveToBoard·stopSession)를 그대로 부른다
@MainActor final class SessionMenuPanel: ObservableObject {
    static let shared = SessionMenuPanel()
    struct Item: Identifiable {
        let id: Int
        let icon: String
        let title: String
        let detail: String
        var enabled = true
        var destructive = false
        var tint = Palette.color("working")
        let run: @MainActor () -> Void
    }
    // 연 순간의 세션과 그 칩 — 1초마다 다시 읽는 목록과 따로 둔다(canDrive 는 프로세스를 살피므로 열 때 한 번만)
    private(set) var session: Session?
    private(set) var place: SessionPlace = .ended
    private(set) var appName: String?
    private(set) var mode: String?
    private(set) var items: [Item] = []
    @Published var highlighted: Int?
    final class Panel: NSPanel { override var canBecomeKey: Bool { true } }
    private let panel = Panel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
    private var host: NSHostingView<SessionMenuView>?
    private var monitors: [Any] = []
    private var observers: [NSObjectProtocol] = []
    private var closed: (id: String, at: Date)?   // 열린 메뉴의 ⋯ 를 다시 누른 클릭은 닫기만 한다

    private init() {
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .popUpMenu   // 위에 고정한 보드(.floating)보다 위
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.title = "세션 메뉴"
        panel.isReleasedWhenClosed = false
    }

    var isOpen: Bool { panel.isVisible }

    func toggle(_ s: Session, from view: NSView?) {
        if let c = closed, c.id == s.id, Date().timeIntervalSince(c.at) < 0.4 { closed = nil; return }
        if panel.isVisible { close() }
        open(s, from: view)
    }

    private func open(_ s: Session, from view: NSView?) {
        HintPanel.shared.leaveAll()
        let hub = TerminalHub.shared
        session = s
        place = hub.place(s)
        appName = hub.appNames[s.id]
        mode = s.state == "ended" ? nil : hub.mode(s)
        items = Self.items(for: s, place: place)
        highlighted = nil
        let host = self.host ?? FirstClickHostingView(rootView: SessionMenuView(model: self))
        host.rootView = SessionMenuView(model: self)
        if self.host == nil {
            self.host = host
            panel.contentView = host
        }
        let size = host.fittingSize
        // 단추 아래에 오른쪽 끝을 맞춰 붙인다 — 아래 자리가 없으면 단추 위로. 단추를 못 찾으면 마우스 자리
        let anchor: NSRect
        if let v = view, let w = v.window {
            anchor = w.convertToScreen(v.convert(v.bounds, to: nil))
        } else {
            let m = NSEvent.mouseLocation
            anchor = NSRect(x: m.x, y: m.y, width: 0, height: 0)
        }
        let vf = (NSScreen.screens.first { $0.frame.intersects(anchor) } ?? NSScreen.main)?.visibleFrame ?? anchor
        var o = NSPoint(x: anchor.maxX - size.width, y: anchor.minY - 6 - size.height)
        if o.y < vf.minY + 4 { o.y = anchor.maxY + 6 }
        o.x = max(vf.minX + 4, min(o.x, vf.maxX - size.width - 4))
        o.y = max(vf.minY + 4, min(o.y, vf.maxY - size.height - 4))
        panel.setFrame(NSRect(origin: o, size: size), display: true)
        panel.invalidateShadow()
        // 손쉬운 사용 등으로 보드가 앞에 없을 때 눌렸으면 앞으로 — 키(↑↓·Return·Esc)가 보드로 와야 한다
        if !NSApp.isActive { NSApp.activate(ignoringOtherApps: true) }
        panel.makeKeyAndOrderFront(nil)
        watch()
    }

    private func watch() {
        if let m = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel],
                                                    handler: { [weak self] e in self?.handle(e) ?? e }) { monitors.append(m) }
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown],
                                                     handler: { [weak self] _ in MainActor.assumeIsolated { self?.close() } }) { monitors.append(m) }
        let nc = NotificationCenter.default
        for (name, object) in [(NSApplication.didResignActiveNotification, nil), (NSWindow.didMoveNotification, window),
                               (NSWindow.didResizeNotification, window), (NSWindow.didResignKeyNotification, panel)] as [(Notification.Name, Any?)] {
            observers.append(nc.addObserver(forName: name, object: object, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.close() }
            })
        }
    }

    // 앱 안 이벤트 — ↑↓·Return·Esc 는 메뉴가 쓰고, 패널 밖을 누르거나 굴리면 닫고 그 이벤트는 그대로 보낸다
    private func handle(_ e: NSEvent) -> NSEvent? {
        switch e.type {
        case .keyDown:
            if e.modifierFlags.contains(.command) { close(); return e }   // ⌘Q 등은 그대로
            switch e.keyCode {
            case 125: move(1)
            case 126: move(-1)
            case 36, 76, 49: if let h = highlighted { run(h) }   // Return·Enter·Space
            case 53: close()
            default: break
            }
            return nil
        default:
            if e.window !== panel { close() }
            return e
        }
    }

    func move(_ d: Int) {
        let ids = items.filter(\.enabled).map(\.id)
        guard !ids.isEmpty else { return }
        if let h = highlighted, let i = ids.firstIndex(of: h) {
            highlighted = ids[(i + d + ids.count) % ids.count]
        } else {
            highlighted = d > 0 ? ids.first : ids.last
        }
    }

    func hover(_ id: Int, _ inside: Bool) {
        guard items.first(where: { $0.id == id })?.enabled == true else { return }
        if inside { highlighted = id } else if highlighted == id { highlighted = nil }
    }

    // 고른 항목 — 패널을 먼저 닫고 다음 차례에 부른다(중단·옮기기는 확인 창을 띄운다)
    func run(_ id: Int) {
        guard let item = items.first(where: { $0.id == id }), item.enabled else { return }
        close()
        DispatchQueue.main.async { item.run() }
    }

    func close() {
        guard panel.isVisible else { return }
        monitors.forEach(NSEvent.removeMonitor)
        monitors = []
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        panel.orderOut(nil)
        closed = (session?.id ?? "", Date())
        highlighted = nil
        if NSApp.isActive, window.isVisible, !window.isKeyWindow { window.makeKey() }   // 키 창·터미널 포커스를 보드로 돌린다
        WindowOpacity.shared.apply()
    }

    // 항목과 조건 — 끝난 세션: 다시 열기(일시중지였으면 재개), 일시중지: 재개·중단, 그 밖: 일시중지·(Ghostty 세션이면) 보드로 옮기기·중단
    static func items(for s: Session, place: SessionPlace) -> [Item] {
        let red = Color(red: 1.0, green: 0.45, blue: 0.45)
        let stop = Item(id: 9, icon: "stop.fill", title: "세션 중단", detail: "응답을 멈추고 끝냅니다. 대화 기록은 남습니다",
                        destructive: true, tint: red) { stopSession(s) }
        switch s.state {
        case "ended":
            // 보드 새 탭에서 이어 연다 — 백그라운드(claude -p)였던 세션은 -p 없이 대화형으로 열리고 그때부터 보드 탭 세션
            if s.paused {
                return [Item(id: 0, icon: "play.fill", title: "세션 재개 (이어서 진행)", detail: "보드 새 탭에서 열고 멈춘 작업을 이어 갑니다") {
                    resumeSession(s, carryOn: true)
                }]
            }
            return [Item(id: 0, icon: "arrow.clockwise", title: "세션 다시 열기",
                         detail: s.headless ? "보드 새 탭에서 대화형으로 이어 엽니다" : "보드 새 탭에서 이 대화를 이어 엽니다") {
                resumeSession(s, carryOn: false)
            }]
        case "paused":
            return [Item(id: 0, icon: "play.fill", title: "세션 재개", detail: "멈춘 지점부터 이어서 진행하게 합니다") { resumePaused(s) }, stop]
        default:
            let canPause = s.state != "idle" && canDriveSession(s)
            var list = [Item(id: 0, icon: "pause.fill", title: "일시중지",
                             detail: canPause ? "지금 응답을 멈춥니다. 재개하면 이어 갑니다"
                                 : s.state == "idle" ? "대기 중이라 멈출 응답이 없습니다" : "이 터미널에는 키를 넣을 수 없습니다",
                             enabled: canPause, tint: Palette.color("paused")) { pauseSession(s) }]
            // 다른 터미널(Ghostty)의 세션을 보드 탭으로 — 그쪽을 /exit 로 끝내고 보드에서 이어 연다
            if place == .terminal, canDrive(s) {
                list.append(Item(id: 1, icon: "arrow.right.square", title: "보드로 옮기기", detail: "그쪽을 끝내고 보드 새 탭에서 이어 엽니다",
                                 tint: Color(red: 0.66, green: 0.76, blue: 1.0)) { moveToBoard(s) })
            }
            return list + [stop]
        }
    }
}

struct SessionMenuView: View {
    @ObservedObject var model: SessionMenuPanel
    static let width: CGFloat = 300

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let s = model.session { header(s) }
            Rectangle().fill(Color.white.opacity(0.08)).frame(height: 0.5)
            VStack(alignment: .leading, spacing: 2) {
                ForEach(model.items) { item in
                    if item.destructive {
                        Rectangle().fill(Color.white.opacity(0.08)).frame(height: 0.5).padding(.horizontal, 8).padding(.vertical, 4)
                    }
                    row(item)
                }
            }
            .padding(6)
        }
        .frame(width: Self.width)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color(red: 0.11, green: 0.11, blue: 0.17).opacity(0.9))
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
        )
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
        .accessibilityElement(children: .contain)
        .accessibilityLabel("세션 메뉴")
    }

    // 세션 이름, 그 아래 상태 점·상태 이름·칩·작업 폴더
    private func header(_ s: Session) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(s.title)
                .boardFont(13, weight: .semibold)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 6) {
                Circle().fill(Palette.color(s.state)).frame(width: 7, height: 7)
                Text(Palette.name(s.state)).boardFont(11, weight: .semibold).foregroundStyle(Palette.color(s.state)).fixedSize()
                PlaceChip(s: s, place: model.place, appName: model.appName, mode: model.mode)
                Text((s.cwd as NSString).lastPathComponent).boardFont(11).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 12)
        .padding(.bottom, 10)
        .accessibilityElement(children: .combine)
    }

    private func row(_ item: SessionMenuPanel.Item) -> some View {
        let on = model.highlighted == item.id
        return Button { model.run(item.id) } label: {
            HStack(spacing: 10) {
                Image(systemName: item.icon)
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(item.tint)
                    .frame(width: 26, height: 26)
                    .background(RoundedRectangle(cornerRadius: 7).fill(item.tint.opacity(on ? 0.26 : 0.14)))
                VStack(alignment: .leading, spacing: 1) {
                    Text(item.title).boardFont(13, weight: .semibold).foregroundStyle(item.destructive ? item.tint : .primary)
                    Text(item.detail).boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 9).fill(on ? (item.destructive ? item.tint.opacity(0.16) : Color.white.opacity(0.10)) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!item.enabled)
        .opacity(item.enabled ? 1 : 0.45)
        .onHover { model.hover(item.id, $0) }
        .accessibilityLabel(item.title)
        .accessibilityHint(item.detail)
    }
}

// 누르면 클로저를 실행하는 메뉴 항목
final class ActionItem: NSMenuItem {
    private let run: @MainActor () -> Void
    init(_ title: String, enabled: Bool = true, run: @escaping @MainActor () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
        isEnabled = enabled
    }
    required init(coder: NSCoder) { fatalError() }
    @objc private func fire() { MainActor.assumeIsolated { run() } }   // 메뉴 동작은 메인 스레드에서 온다
}

// 트리 연결선: 위에서 내려와 오른쪽으로 꺾이고, 마지막 줄이 아니면 아래로 계속 이어진다
struct TreeBranch: Shape {
    let last: Bool
    func path(in r: CGRect) -> Path {
        var p = Path()
        let x = r.midX, y = r.midY
        p.move(to: CGPoint(x: x, y: r.minY))
        p.addLine(to: CGPoint(x: x, y: last ? y - 5 : r.maxY))
        if last { p.addQuadCurve(to: CGPoint(x: x + 5, y: y), control: CGPoint(x: x, y: y)) } else {
            p.move(to: CGPoint(x: x, y: y))
        }
        p.addLine(to: CGPoint(x: r.maxX, y: y))
        return p
    }
}

// 세션 줄 왼쪽의 접기·펼치기 화살표 — 그룹 카드 머리의 화살표와 같은 모양
struct FoldChevron: View {
    let open: Bool
    let toggle: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: toggle) {
            Image(systemName: "chevron.right")
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(.primary.opacity(hover ? 1 : 0.65))
                .rotationEffect(.degrees(open ? 90 : 0))
                .frame(width: 14, height: 18)
                .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(hover ? 0.12 : 0)))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(open ? "서브에이전트 목록 접기" : "서브에이전트 목록 펼치기")
    }
}

// 세션 줄의 서브에이전트 트리 접기·펼치기 단추
// 세션별 알림 켜기·끄기 — 끄면 작업 완료·승인 대기 패널과 소리를 모두 띄우지 않는다
struct MuteToggle: View {
    let s: Session
    @Environment(\.notifyLocked) private var locked
    @State private var hover = false
    // 누른 순간의 값 — 표지 파일을 다시 읽어 s.muted 가 따라올 때까지 이 값으로 바로 보인다
    @State private var pending: Bool?

    var body: some View {
        let on = pending ?? !s.muted
        Toggle("", isOn: Binding(get: { on }, set: { v in
            withAnimation(.spring(response: 0.22, dampingFraction: 0.8)) { pending = v }
            setMuted(s, !v)
        }))
            .toggleStyle(BoardSwitch(tint: Palette.color("working"), hover: hover, locked: locked))
            .labelsHidden()
            .disabled(locked)
            .modifier(ClickableIf(active: !locked, hover: $hover))
            .onChange(of: s.muted) { pending = nil }
            .hint(locked ? "전체 알림이 꺼져 있어 바꿀 수 없습니다. 윗줄 종 단추로 전체 알림을 켜면 이 세션의 설정(\(on ? "켜짐" : "꺼짐"))대로 돌아갑니다"
                  : !on ? "알림 꺼짐: 켜면 작업 완료·승인 대기 알림을 다시 띄웁니다" : "알림 켜짐: 끄면 이 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다")
    }
}

// 직접 그린 스위치 — macOS 기본 스위치는 창이 비활성이면 회색으로 바뀌어 켜짐·꺼짐이 안 보인다
struct BoardSwitch: ToggleStyle {
    let tint: Color
    var hover = false
    var locked = false      // 전체 알림 꺼짐 — 회색으로 흐리게, 손잡이는 세션 설정 위치 그대로(다시 켜면 돌아갈 상태)

    func makeBody(configuration: Configuration) -> some View {
        let on = configuration.isOn
        let fill = locked ? Color.white.opacity(0.12) : on ? tint : Color.white.opacity(hover ? 0.22 : 0.14)
        // 탭 제스처 대신 버튼 — 세션 줄의 더블 클릭 판별을 기다리지 않고 누르는 즉시 반응한다
        Button { configuration.isOn.toggle() } label: {
        ZStack(alignment: on ? .trailing : .leading) {
            Capsule()
                .fill(fill)
                .overlay(Capsule().stroke(Color.white.opacity(locked ? 0.35 : on ? 0 : 0.18),
                                          style: StrokeStyle(lineWidth: 0.5, dash: locked ? [2, 2] : [])))
            // 손잡이 안의 종 아이콘으로 알림 스위치임을 알린다 — 켜짐 종, 꺼짐 빗금 종
            Circle()
                .fill(Color.white.opacity(locked ? 0.45 : on ? 1 : 0.75))
                .shadow(color: .black.opacity(locked ? 0 : 0.3), radius: 1, y: 0.5)
                .overlay(
                    Image(systemName: locked || !on ? "bell.slash.fill" : "bell.fill")
                        .font(.system(size: 8, weight: .bold))
                        .foregroundStyle(locked ? Color.black.opacity(0.6) : on ? tint : Color.black.opacity(0.55))
                        .contentTransition(.symbolEffect(.replace))
                )
                .padding(2)
        }
        .frame(width: 36, height: 20)
        .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .animation(.spring(response: 0.22, dampingFraction: 0.8), value: on)
    }
}

// 윗줄 전체 알림 단추 — 꺼지면 붉은 캡슐 "알림 꺼짐"(좁으면 아이콘만)으로 눈에 띄게
struct AllMuteButton: View {
    let muted: Bool
    let toggle: () -> Void
    @State private var hover = false

    var body: some View {
        let c = Color(red: 1.00, green: 0.45, blue: 0.45)
        Button(action: toggle) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 4) {
                    Image(systemName: muted ? "bell.slash.fill" : "bell.fill")
                    if muted { Text("알림 꺼짐").fixedSize() }
                }
                Image(systemName: muted ? "bell.slash.fill" : "bell.fill")
            }
            .boardFont(11, weight: .semibold)
            .foregroundStyle(muted ? c : hover ? .primary : .secondary)
            .contentTransition(.symbolEffect(.replace))
            .padding(.horizontal, 8)
            .frame(minWidth: 28, minHeight: 24)
            .background(Capsule().fill(muted ? c.opacity(0.16) : Color.white.opacity(hover ? 0.12 : 0.07)))
            .overlay(Capsule().stroke(muted ? c.opacity(0.4) : .clear, lineWidth: 0.5))
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(muted ? "전체 알림 꺼짐: 누르면 다시 켜고, 세션마다 정해 둔 알림 설정으로 돌아갑니다"
              : "전체 알림 끄기: 모든 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다")
    }
}

// 작업경로 카드 안 구역 머리글 — 접기 화살표·종류 아이콘·이름, 상태별 색 점·개수(있는 상태만)와 가는 선. 누르면 구역을 접고 편다
struct KindSection: View {
    let headless: Bool
    let sessions: [Session]
    let collapsed: Bool
    let toggle: () -> Void
    @State private var hover = false
    var body: some View {
        let c = headless ? Color(red: 0.72, green: 0.55, blue: 1.00) : Color(red: 0.32, green: 0.78, blue: 0.86)
        HStack(spacing: 6) {
            Image(systemName: "chevron.right")
                .font(.system(size: 8, weight: .bold))
                .rotationEffect(.degrees(collapsed ? 0 : 90))
                .frame(width: 8)
            Image(systemName: headless ? "moon.stars.fill" : "terminal.fill").font(.system(size: 9, weight: .semibold))
            Text(headless ? "백그라운드" : "터미널").boardFont(10.5, weight: .semibold)
            ForEach(["waiting", "working", "paused", "idle"], id: \.self) { st in
                let n = sessions.filter { $0.state == st }.count
                if n > 0 {
                    HStack(spacing: 3) {
                        Circle().fill(Palette.color(st)).frame(width: 6, height: 6)
                        Text("\(n)").boardFont(10, weight: .semibold).monospacedDigit()
                            .foregroundStyle(.secondary)
                            .contentTransition(.numericText())
                    }
                    .hint("\(Palette.name(st)) \(n)개")
                }
            }
            Rectangle().fill(c.opacity(0.22)).frame(height: 0.5)
        }
        .foregroundStyle(c.opacity(hover ? 1 : 0.85))
        .padding(.horizontal, 10)
        .padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(hover ? 0.05 : 0)))
        .padding(.top, 1)
        .contentShape(Rectangle())
        .clickable($hover)
        .onTapGesture { withAnimation(foldSpring) { toggle() } }
        .hint((headless ? "터미널 없이 도는 세션이라 이동할 터미널이 없습니다" : "터미널에서 도는 세션입니다. 보드 탭 세션은 눌러서 그 탭으로, 다른 터미널 세션은 눌러서 그 앱, 두 번 눌러서 그 세션으로 이동합니다")
              + (collapsed ? " (누르면 펼치기)" : " (누르면 접기)"))
    }
}

struct SubToggle: View {
    let count: Int
    let running: Int
    let open: Bool
    let toggle: () -> Void
    var compact = false     // 줄이 좁으면 아이콘 + 개수만 보인다
    @State private var hover = false

    var body: some View {
        let c = Color(red: 0.95, green: 0.55, blue: 0.75)
        Button(action: toggle) {
            HStack(spacing: 4) {
                Image(systemName: "person.2.fill")
                    .font(.system(size: 9))
                let n = running > 0 && running < count ? "\(running)/\(count)" : "\(count)"
                Text(compact ? n : "하위 작업 \(n)")
                    .monospacedDigit()
                if !compact {
                    Text(open ? "· 접기" : "· 펼치기").foregroundStyle(.primary.opacity(hover ? 0.9 : 0.6))
                }
            }
            .fixedSize()
            .boardFont(10, weight: .semibold)
            .foregroundStyle(c)
            .padding(.horizontal, 6)
            .padding(.vertical, 1.5)
            .background(Capsule().fill(c.opacity(hover ? 0.28 : 0.15)))
            .overlay(Capsule().stroke(c.opacity(0.25), lineWidth: 0.5))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("하위 작업 \(count)개 · " + (open ? "서브에이전트 목록 접기" : "서브에이전트 목록 펼치기"))
    }
}

// labelStyle 을 조건에 따라 고르기 위한 형식 지우개
struct AnyLabelStyle: LabelStyle {
    private let make: (Configuration) -> AnyView
    init<S: LabelStyle>(_ style: S) { make = { AnyView(style.makeBody(configuration: $0)) } }
    func makeBody(configuration: Configuration) -> some View { make(configuration) }
}

struct SubagentRow: View {
    let sub: Sub
    let now: Date
    let last: Bool

    var body: some View {
        let done = sub.state == "done"
        HStack(spacing: 8) {
            TreeBranch(last: last)
                .stroke(Color.white.opacity(0.22), style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
                .frame(width: 16)
            StateIcon(state: sub.boardState).scaleEffect(0.8)
            VStack(alignment: .leading, spacing: 2) {
                Text(sub.desc.isEmpty ? sub.type : sub.desc)
                    .boardFont(12, weight: .medium)
                    .foregroundStyle(done ? .secondary : .primary)
                    .lineLimit(1)
                // 좁으면 라벨을 아이콘만, 더 좁으면 에이전트 종류·도구 이름을 뺀다 — 글자가 잘게 끊기지 않게. 그래도 안 들어가면 다음 줄로
                ViewThatFits(in: .horizontal) {
                    meta(level: 0)
                    meta(level: 1)
                    meta(level: 2)
                    meta(level: 2, wrap: true)
                }
                .boardFont(10.5)
                .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(.leading, 36)          // 부모 줄의 상태 아이콘 아래에 연결선이 오도록
        .padding(.trailing, 10)
        .frame(minHeight: 38)
        .opacity(done ? 0.6 : 1)
        .hint(sub.background ? "백그라운드로 실행된 서브에이전트" : "세션이 결과를 기다리는 서브에이전트")
 
    }

    private func meta(level: Int, wrap: Bool = false) -> some View {
        let done = sub.state == "done"
        return (wrap ? AnyLayout(WrapRow(spacing: 5)) : AnyLayout(HStackLayout(spacing: 5))) {
            KindLabel(subagent: true, compact: level > 0)
            if level < 2, !sub.type.isEmpty { Text(sub.type).fixedSize() }
            if level < 2 { Text("·") }
            Text(done ? "\(elapsed(sub.started, sub.ended ?? now)) 걸려 완료"
                 : sub.state == "waiting" ? "승인 대기" : elapsed(sub.started, now))
                .monospacedDigit()
                .fixedSize()
                .contentTransition(.numericText())
            if let b = sub.brain { ModelLabel(brain: b, level: level) }
            if !done, !sub.tool.isEmpty {
                Text("·")
                Label(level < 2 ? sub.tool : "", systemImage: "wrench.and.screwdriver.fill")
                    .labelStyle(level < 2 ? AnyLabelStyle(.titleAndIcon) : AnyLabelStyle(.iconOnly))
                    .fixedSize()
                    .hint(sub.tool)
            }
        }
    }
}

// 경로 카드 폴더 모서리 표지 — 펼친 터미널이 지금 이 경로를 보이고 있다(아이콘은 제목 막대의 터미널 단추와 같은 쪽).
// 글자는 붙이지 않는다 — 좁은 보드에서 카드 이름이 잘리고 바로 아래 "터미널" 구역 머리글과 겹쳐 읽힌다. 설명은 폴더 아이콘 힌트에
struct TerminalHereMark: View {
    let boardOnLeft: Bool
    var body: some View {
        Image(systemName: boardOnLeft ? "rectangle.trailinghalf.inset.filled" : "rectangle.leadinghalf.inset.filled")
            .font(.system(size: 8, weight: .bold))
            .foregroundStyle(.white)
            .padding(.horizontal, 4).padding(.vertical, 2.5)
            .background(Capsule().fill(Color(nsColor: terminalBarColor)))
            .overlay(Capsule().stroke(Color.white.opacity(0.7), lineWidth: 1))
            .fixedSize()
    }
}

// 카드 테두리 — 승인 대기가 있으면 테두리와 빛이 천천히 숨 쉰다
struct CardBorder: View {
    let color: Color
    let waiting: Bool
    var body: some View {
        if waiting {
            Ambient { now in
                let k = swing(now, 2.4)
                RoundedRectangle(cornerRadius: 16)
                    .stroke(color.opacity(0.25 + 0.5 * k), lineWidth: 1)
                    .shadow(color: color.opacity(0.35 * k), radius: 12, y: 3)
            }
        } else {
            RoundedRectangle(cornerRadius: 16).stroke(color.opacity(0.22), lineWidth: 1)
        }
    }
}

struct GroupCard: View {
    let g: Group
    let now: Date
    let collapsed: Bool
    let toggle: () -> Void
    let endedOpen: Bool
    let toggleEnded: () -> Void
    let pinned: Bool
    let togglePin: () -> Void
    let remove: () -> Void
    let clearEnded: () -> Void
    @ObservedObject private var hub = TerminalHub.shared
    // 이 경로를 쓰는 자율주행 실행 — 실행 줄(기본 보기에서는 숨김)과 원래 작업 트리 표지(늘)
    @ObservedObject private var autopilot = AutopilotStore.shared
    @AppStorage("autopilotView") private var autopilotView = "all"
    @State private var hover = false
    @State private var endedHover = false
    // 접어 둔 구역 "<작업경로>|term" / "<작업경로>|bg" (줄바꿈으로 이음) — 앱을 다시 켜도 유지한다
    @AppStorage("kindCollapsed") private var kindCollapsed = ""

    private func kindFolded(_ kind: String) -> Bool {
        kindCollapsed.split(separator: "\n").contains(Substring("\(g.id)|\(kind)"))
    }

    private func toggleKind(_ kind: String) {
        let key = "\(g.id)|\(kind)"
        var keys = Set(kindCollapsed.split(separator: "\n").map(String.init))
        if keys.contains(key) { keys.remove(key) } else { keys.insert(key) }
        kindCollapsed = keys.sorted().joined(separator: "\n")
    }

    var body: some View {
        let c = Palette.color(g.urgent)
        let here = hub.expanded && hub.workspace == workspaceKey(g.id)   // 펼친 터미널이 이 경로를 보이는 중
        let apRuns = autopilot.runs(at: g.id)
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.secondary.opacity(hover ? 1 : 0.6))
                    .rotationEffect(.degrees(collapsed ? 0 : 90))
                    .frame(width: 10)
                    .hint(collapsed ? "경로 카드 펼치기" : "경로 카드 접기")
                // 접혀 있어도 작업 중(도는 링)·승인 대기(퍼지는 파문)가 보이게 폴더 자리에 상태 표시를 그린다
                ZStack {
                    if collapsed, let top = g.active.first {
                        StateIcon(state: top.state)
                    } else {
                        Image(systemName: "folder.fill")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(c)
                    }
                }
                .frame(width: 30, height: 30)
                .background(RoundedRectangle(cornerRadius: 8).fill(c.opacity(0.16)))
                // 펼친 터미널이 지금 보이는 경로 — 상태 색과 헷갈리지 않게 흰 표지를 폴더 모서리에(카드 이름 폭을 줄이지 않게)
                .overlay(alignment: .bottomTrailing) { if here { TerminalHereMark(boardOnLeft: hub.boardOnLeft).offset(x: 6, y: 5) } }
                .hint((here ? "터미널에서 지금 보는 경로 · " : "")
                      + (collapsed ? g.active.first.map { "\(Palette.name($0.state)) · 누르면 펼치기" } ?? "작업 경로 · 누르면 펼치기"
                         : "작업 경로 · 누르면 접기"))
                VStack(alignment: .leading, spacing: 1) {
                    Text(g.name).boardFont(15, weight: .semibold).lineLimit(1)
                    // 접혀 있으면 경로 대신 지금 하는 일을 상태 색으로 — 여럿이면 나머지 개수를 붙인다
                    if collapsed, let top = g.active.first {
                        HStack(spacing: 4) {
                            Text("\(Palette.name(top.state)) · \(doing(top))").lineLimit(1)
                            if g.active.count > 1 { Text("+\(g.active.count - 1)").fixedSize() }
                        }
                        .boardFont(11, weight: .medium)
                        .foregroundStyle(Palette.color(top.state))
                    } else {
                        Text(g.path).boardFont(11).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                    }
                }
                Spacer()
                // 이 경로에서 바로 새 세션 — 기본 명령으로 보드 탭에서 띄운다
                HeaderButton(icon: "plus", tint: nil, help: "이 경로에서 새 세션 (\(defaultSessionCommand))") {
                    TerminalHub.shared.startSession(defaultSessionCommand, in: g.id)
                }
                HeaderButton(icon: pinned ? "star.fill" : "star", tint: pinned ? Color(red: 1.0, green: 0.78, blue: 0.28) : nil,
                             help: pinned ? "상단 고정 풀기" : "상단에 고정", action: togglePin)
                HeaderButton(icon: "xmark", tint: nil, help: "목록에서 지우기 (새 활동이 생기면 다시 나타납니다)", action: remove)
                HStack(spacing: 6) {
                    ForEach(["waiting", "working", "idle"], id: \.self) { st in
                        let n = g.sessions.filter { $0.state == st }.count
                        if n > 0 {
                            HStack(spacing: 3) {
                                Circle().fill(Palette.color(st)).frame(width: 6, height: 6)
                                Text("\(n)").boardFont(11, weight: .semibold).monospacedDigit()
                                    .contentTransition(.numericText())
                            }
                            .hint("\(Palette.name(st)) \(n)개")
                        }
                    }
                }
                .foregroundStyle(.secondary)
            }
            .padding(6)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hover ? 0.06 : 0)))
            .modifier(Shimmer(active: collapsed && g.urgent == "working", color: c))   // 접힌 채 작업 중이면 세션 줄처럼 빛이 지나간다
            .padding(-6)
            .padding(.bottom, collapsed ? 0 : 2)
            .contentShape(Rectangle())
            .clickable($hover)
            .onTapGesture {
                withAnimation(foldSpring) { toggle() }
                hub.showWorkspace(g.id)   // 그 경로에 보드 탭이 있으면 터미널도 그 경로로(접힌 터미널은 펼치지 않는다)
            }
            .contextMenu {
                Button(pinned ? "상단 고정 풀기" : "상단에 고정", action: togglePin)
                Button("목록에서 지우기", role: .destructive, action: remove)
            }

            // 원래 작업 트리로 자율주행이 도는 경로의 표지 — 기본 보기에서도 남긴다. 좁은 카드에서 이름이 밀리지 않게 머리 아래 줄에 둔다
            if let r = apRuns.first(where: \.inPlace) { APInPlaceRow(run: r) }
            // 자율주행 실행 줄 — 그 경로의 세션 카드 무리 안(와이어프레임 보드 본체 ③). 접힌 카드에서는 한 줄로
            if autopilotView != "basic" {
                ForEach(apRuns) { r in APRunLine(run: r, minutes: r.unitMinutes(now), compact: collapsed).equatable().transition(.opacity) }
            }

            if !collapsed {
                let ended = g.sessions.filter { $0.state == "ended" }
                VStack(alignment: .leading, spacing: 6) {
                    // 터미널 세션과 백그라운드 세션을 구역으로 나눈다 — 백그라운드는 더블 클릭으로 이동할 터미널이 없다.
                    // 세션이 있는 종류마다 머리글을 붙인다 — 한 종류만 있어도 터미널인지 백그라운드인지 보인다
                    let live = g.sessions.filter { $0.state != "ended" }
                    // 보드 탭 안에서 시작된 claude -p 는 탭과 함께 끝나므로 터미널 구역(탭 쪽)에 둔다
                    let terminal = live.filter { hub.place($0) != .background }
                    let background = live.filter { hub.place($0) == .background }
                    if !terminal.isEmpty {
                        KindSection(headless: false, sessions: terminal,
                                    collapsed: kindFolded("term"), toggle: { toggleKind("term") })
                    }
                    if !kindFolded("term") {
                        ForEach(Array(terminal.enumerated()), id: \.element.id) { i, s in
                            SessionRow(s: s, now: now)
                                .transition(foldIn(i))
                        }
                    }
                    if !background.isEmpty {
                        KindSection(headless: true, sessions: background,
                                    collapsed: kindFolded("bg"), toggle: { toggleKind("bg") })
                    }
                    if !kindFolded("bg") {
                        ForEach(Array(background.enumerated()), id: \.element.id) { i, s in
                            SessionRow(s: s, now: now)
                                .transition(foldIn(terminal.count + i))
                        }
                    }
                    if !ended.isEmpty {
                        // 화살표 크기·칸 폭·간격을 구역 머리글(KindSection)과 같게 — 화살표가 같은 세로줄에 선다
                        HStack(spacing: 6) {
                            Image(systemName: "chevron.right")
                                .font(.system(size: 8, weight: .bold))
                                .rotationEffect(.degrees(endedOpen ? 90 : 0))
                                .frame(width: 8)
                            Text("종료된 세션 \(ended.count)개")
                                .boardFont(11, weight: .medium)
                                .contentTransition(.numericText())
                            Spacer()
                            if endedHover {
                                ClearEndedButton(count: ended.count, action: clearEnded)
                            }
                        }
                        .foregroundStyle(.secondary.opacity(endedHover ? 1 : 0.7))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(endedHover ? 0.06 : 0)))
                        .contentShape(Rectangle())
                        .clickable($endedHover)
                        .hint(endedOpen ? "종료된 세션 접기" : "종료된 세션 펼치기")
                        .onTapGesture { withAnimation(foldSpring) { toggleEnded() } }
                        .transition(.opacity)
                        if endedOpen {
                            VStack(alignment: .leading, spacing: 6) {
                                ForEach(Array(ended.enumerated()), id: \.element.id) { i, s in
                                    SessionRow(s: s, now: now)
                                        .transition(foldIn(i))
                                }
                            }
                            .transition(.opacity)
                        }
                    }
                }
                .transition(.opacity.combined(with: .scale(scale: 0.97, anchor: .top)))
            }
        }
        .padding(12)
        .clipShape(RoundedRectangle(cornerRadius: 16))   // 접고 펼치는 동안 줄이 카드 밖으로 비치지 않게
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(LinearGradient(colors: [c.opacity(0.10), Color.white.opacity(0.03)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing))
        )
        .overlay(CardBorder(color: c, waiting: g.urgent == "waiting"))
        .shadow(color: g.urgent == "waiting" ? .clear : .black.opacity(0.25), radius: 8, y: 3)
        .animation(.easeInOut(duration: 0.4), value: g.urgent)
    }
}

// MARK: - 접기·펼치기

let foldSpring = Animation.spring(response: 0.38, dampingFraction: 0.86)

// 펼칠 때 줄마다 조금씩 늦게 위에서 내려오며 나타난다
func foldIn(_ i: Int) -> AnyTransition {
    .asymmetric(insertion: .opacity.combined(with: .offset(y: -10))
                    .animation(foldSpring.delay(Double(min(i, 8)) * 0.035)),
                removal: .opacity.animation(.easeOut(duration: 0.15)))
}

// 마우스를 올리면 오른쪽에 나타나는 접기·펼치기 안내
// 그룹 카드 머리의 작은 아이콘 단추(상단 고정·목록에서 지우기)
struct HeaderButton: View {
    let icon: String
    let tint: Color?
    let help: String
    let action: @MainActor () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(tint ?? .primary.opacity(hover ? 0.95 : 0.55))
                .frame(width: 26, height: 26)
                .background(Circle().fill(Color.white.opacity(hover ? 0.16 : 0.06)))
                .contentShape(Circle())
                .scaleEffect(hover ? 1.3 : 1)   // 마우스를 올리면 커져서 누르기 쉽다
                .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hover)
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(help)
        .transition(.opacity.combined(with: .scale(scale: 0.8)))
    }
}

// "종료된 세션 N개" 줄에 마우스를 올리면 나오는 휴지통 — 그 작업경로의 종료된 세션을 한 번에 지운다
struct ClearEndedButton: View {
    let count: Int
    let action: () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            HStack(spacing: 3) {
                Image(systemName: "trash").font(.system(size: 9, weight: .bold))
                if hover { Text("모두 지우기").boardFont(10, weight: .semibold) }
            }
            .foregroundStyle(hover ? Color.red : .primary.opacity(0.8))
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(Capsule().fill(hover ? Color.red.opacity(0.18) : Color.white.opacity(0.10)))
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("종료된 세션 \(count)개를 목록에서 지우기")
        .transition(.opacity.combined(with: .move(edge: .trailing)))
    }
}

// 조건부 clickable — 잠긴 단추에는 손가락 커서를 보이지 않는다
struct ClickableIf: ViewModifier {
    let active: Bool
    @Binding var hover: Bool
    func body(content: Content) -> some View {
        if active { content.clickable($hover) } else { content }
    }
}

extension View {
    // 클릭할 수 있는 영역: 손가락 커서를 보이고 hover 상태를 갱신한다
    func clickable(_ hover: Binding<Bool>) -> some View {
        onContinuousHover { phase in
            var inside = false
            if case .active = phase { inside = true }
            (inside ? NSCursor.pointingHand : NSCursor.arrow).set()
            if hover.wrappedValue != inside {
                withAnimation(.easeOut(duration: 0.15)) { hover.wrappedValue = inside }
            }
        }
    }

    // 마우스를 올리면 곧 뜨는 짧은 설명 — 시스템 툴팁(.help) 대신 쓴다
    func hint(_ text: String) -> some View { modifier(HintArea(text: text)) }

    // 보드 글자 — 크기에 보드 글자 비율(BoardFontScale)을 곱한다. 보드 창의 SwiftUI 글자는 이것으로 쓴다(아이콘은 .font 그대로)
    func boardFont(_ size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default) -> some View {
        modifier(BoardFont(size: size, weight: weight, design: design))
    }
}

// MARK: - 보드 글자 크기

// 보드 글자 비율 — 보드 설정 boardFontScale(기본 1, 80%~150%, 10%씩. 윈도우 메뉴 ⌥⌘+·⌥⌘-·⌥⌘0).
// 바뀌면 .boardFont 를 쓴 글자만 다시 그려진다 — 펼친 카드 같은 화면 상태는 그대로다(뿌리에 .id 를 바꾸는 방식은 @State 가 풀린다)
@MainActor final class BoardFontScale: ObservableObject {
    static let shared = BoardFontScale()
    @Published private(set) var scale = CGFloat(min(1.5, max(0.8, UserDefaults.standard.object(forKey: "boardFontScale") as? Double ?? 1)))

    // step: +1 크게, -1 작게, nil 원래 크기. 바뀐 비율을 보드 아래쪽에 잠깐 알린다
    func change(_ step: Int?) {
        let tenths = step.map { min(15, max(8, Int((scale * 10).rounded()) + $0)) } ?? 10
        scale = CGFloat(tenths) / 10
        UserDefaults.standard.set(Double(tenths) / 10, forKey: "boardFontScale")
        TerminalHub.shared.toast("보드 글자 \(tenths * 10)%")
    }
}

struct BoardFont: ViewModifier {
    @ObservedObject var fontScale = BoardFontScale.shared
    let size: CGFloat
    let weight: Font.Weight
    let design: Font.Design
    func body(content: Content) -> some View {
        content.font(.system(size: size * fontScale.scale, weight: weight, design: design))
    }
}

// MARK: - 힌트

// 시스템 툴팁(.help)은 1초 넘게 기다려야 뜨고, 보드가 초당 30번 다시 그려지면 그 기다림이 자꾸 처음부터 다시 시작돼
// 몇 초씩 걸리거나 아예 뜨지 않는다. 카드·스크롤 영역에 잘리지 않게 보드 위의 작은 패널 창에 직접 띄운다
final class HintPanel {
    static let shared = HintPanel()
    static let delay: TimeInterval = 0.35
    private let panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
    private let host = NSHostingController(rootView: HintLabel(text: ""))
    // 마우스가 들어가 있는 힌트 영역 — 안쪽 영역은 바깥 영역보다 나중에 들어가므로 마지막 것을 보인다
    private var stack: [(id: UUID, text: String)] = []
    private var pending: DispatchWorkItem?
    private var lastShown = Date.distantPast   // 방금까지 힌트가 떠 있었으면 옆 단추로 옮겨도 기다리지 않는다

    private init() {
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.level = .popUpMenu   // 위에 고정한 보드(.floating)보다 위
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        host.sizingOptions = []
        panel.contentView = host.view
        // 누르거나 스크롤하면 시스템 툴팁처럼 사라진다
        NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .scrollWheel]) { [weak self] e in
            self?.hide()
            return e
        }
    }

    func hover(_ id: UUID, _ text: String, _ inside: Bool) {
        // 마우스가 없던 영역이 사라질 때(줄 폭에 맞춰 라벨을 바꾸는 등)는 떠 있는 힌트를 건드리지 않는다
        if !inside, !stack.contains(where: { $0.id == id }) { return }
        stack.removeAll { $0.id == id }
        if inside { stack.append((id, text)) }
        hide()
        guard let top = stack.last else { return }
        let w = DispatchWorkItem { [weak self] in self?.show(top.text) }
        pending = w
        let warm = Date().timeIntervalSince(lastShown) < 0.4
        DispatchQueue.main.asyncAfter(deadline: .now() + (warm ? 0 : Self.delay), execute: w)
    }

    // 마우스가 보드 창 밖으로 나갔다 — 빠르게 나가면 영역마다의 나감 신호를 놓칠 수 있다
    func leaveAll() {
        stack.removeAll()
        hide()
    }

    func hide() {
        pending?.cancel()
        pending = nil
        if panel.isVisible {
            panel.orderOut(nil)
            lastShown = Date()
        }
    }

    private func show(_ text: String) {
        host.rootView = HintLabel(text: text)
        let size = host.sizeThatFits(in: CGSize(width: HintLabel.maxWidth, height: 1000))
        let m = NSEvent.mouseLocation
        guard let vf = (NSScreen.screens.first { $0.frame.contains(m) } ?? NSScreen.main)?.visibleFrame else { return }
        var o = NSPoint(x: m.x + 2, y: m.y - 22 - size.height)   // 시스템 툴팁처럼 커서 오른쪽 아래
        if o.y < vf.minY { o.y = m.y + 10 }                         // 아래 자리가 없으면 커서 위로
        o.x = max(vf.minX + 4, min(o.x, vf.maxX - size.width - 4))
        panel.setFrame(NSRect(origin: o, size: size), display: true)
        panel.invalidateShadow()       // 그림자를 새 크기에 맞춘다
        panel.orderFrontRegardless()   // 보드를 활성화하지 않는다 — 포커스는 쓰던 앱에 남긴다
    }
}

struct HintLabel: View {
    static let maxWidth: CGFloat = 280   // 긴 설명은 이 폭에서 줄을 바꾼다
    let text: String
    var body: some View {
        // 터미널 독의 힌트와 같은 색
        Text(text)
            .boardFont(11, weight: .medium)
            .foregroundStyle(Color.white.opacity(0.92))
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color(red: 0.10, green: 0.10, blue: 0.15).opacity(0.96)))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
    }
}

struct HintArea: ViewModifier {
    let text: String
    @State private var id = UUID()
    func body(content: Content) -> some View {
        content
            .onHover { HintPanel.shared.hover(id, text, $0) }
            // 상태가 바뀌어 뷰가 사라지면 마우스가 그 자리에 있어도 나감 신호가 오지 않는다
            .onDisappear { HintPanel.shared.hover(id, text, false) }
            .accessibilityHint(text)
    }
}

struct SummaryChip: View {
    let state: String
    let n: Int
    var filtering = false
    var tap: () -> Void = {}
    @State private var hover = false
    var body: some View {
        HStack(spacing: 5) {
            Circle().fill(Palette.color(state)).frame(width: 7, height: 7)
            Text("\(n)").boardFont(12, weight: .bold).monospacedDigit().contentTransition(.numericText())
        }
        .padding(.horizontal, 9)
        .padding(.vertical, 4)
        .background(Capsule().fill(Palette.color(state).opacity(filtering ? 0.35 : hover ? 0.25 : 0.15)))
        .overlay(Capsule().stroke(Palette.color(state).opacity(filtering ? 0.9 : 0), lineWidth: 1))
        .fixedSize()   // 윗줄이 좁아도 숫자가 잘리지 않게 — 모자라면 제목이 먼저 줄어든다
        .contentShape(Capsule())
        .onTapGesture(perform: tap)
        .clickable($hover)
        .hint(filtering ? "\(Palette.name(state))만 보는 중: 누르면 모두 보기"
                        : "\(Palette.name(state)) \(n)개: 누르면 \(Palette.name(state))만 보기")
    }
}

// 창 바탕: 짙은 남색 그라데이션 위에 위쪽에서 강조색 빛이 번진다 (블러가 비쳐 보이도록 살짝 투명)
struct Backdrop: View {
    var body: some View {
        ZStack {
            LinearGradient(colors: [Color(red: 0.12, green: 0.12, blue: 0.19), Color(red: 0.07, green: 0.07, blue: 0.11)],
                           startPoint: .top, endPoint: .bottom)
                .opacity(0.94)
            RadialGradient(colors: [Palette.accent.opacity(0.22), .clear], center: .topLeading,
                           startRadius: 0, endRadius: 360)
            RadialGradient(colors: [Color(red: 0.45, green: 0.50, blue: 1.0).opacity(0.14), .clear], center: .bottomTrailing,
                           startRadius: 0, endRadius: 420)
        }
        .ignoresSafeArea()
    }
}

// 설치된 Claude Code 버전 — 제목을 그릴 때마다 부르므로 1분 동안은 읽어 둔 값을 쓴다
private var versionCache: (at: Date, value: String?) = (.distantPast, nil)

func claudeVersion() -> String? {
    if Date().timeIntervalSince(versionCache.at) > 60 { versionCache = (Date(), readClaudeVersion()) }
    return versionCache.value
}

// ~/.local/bin/claude 가 가리키는 versions/<버전> 의 이름, 못 읽으면 versions 안 가장 높은 것
private func readClaudeVersion() -> String? {
    let home = NSHomeDirectory()
    if let t = try? FileManager.default.destinationOfSymbolicLink(atPath: home + "/.local/bin/claude") {
        let v = (t as NSString).lastPathComponent
        if v.first?.isNumber == true { return v }
    }
    let vs = (try? FileManager.default.contentsOfDirectory(atPath: home + "/.local/share/claude/versions")) ?? []
    return vs.filter { $0.first?.isNumber == true }.max { $0.compare($1, options: .numeric) == .orderedAscending }
}

// 감독의 말풍선 교체·눈 깜빡임 시계 — 보드가 1초마다 다시 그려져도 타이머가 새로 만들어지지 않게 객체에 둔다
final class ForemanClock: ObservableObject {
    @Published var line = 0
    @Published var blink = false
    // 말풍선이 바뀐 시각 — 감독이 그 순간 폴짝 뛴다
    private(set) var changedAt = Date().timeIntervalSinceReferenceDate
    private var timers: [Timer] = []
    init() {
        timers.append(Timer.scheduledTimer(withTimeInterval: 3.2, repeats: true) { [weak self] _ in self?.next() })
        timers.append(Timer.scheduledTimer(withTimeInterval: 3.7, repeats: true) { [weak self] _ in
            withAnimation(.easeIn(duration: 0.08)) { self?.blink = true }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.14) { withAnimation(.easeOut(duration: 0.08)) { self?.blink = false } }
        })
    }
    func next() {
        changedAt = Date().timeIntervalSinceReferenceDate
        withAnimation(.spring(response: 0.38, dampingFraction: 0.55)) { line += 1 }
    }
    deinit { timers.forEach { $0.invalidate() } }
}

// 감독의 기분 — 세션 목록에서 정한다
enum Mood { case urgent, busy, idle, sleepy }

// 말풍선 한 줄 — 기분과 함께
struct Quip: Equatable {
    let text: String
    let mood: Mood
}

// 지금 세션 목록·사용 한도로 감독이 할 말을 만든다
func foremanQuips(groups: [Group], limits: [Limit], now: Date) -> [Quip] {
    let live = groups.flatMap(\.sessions).filter { $0.state != "ended" }
    let name = { (s: Session) in (s.cwd as NSString).lastPathComponent }
    let short = { (t: String, n: Int) in t.count > n ? String(t.prefix(n)) + "…" : t }
    let waiting = live.filter { $0.state == "waiting" }
    let working = live.filter { $0.state == "working" }
    let idle = live.filter { $0.state == "idle" }
    let paused = live.filter { $0.state == "paused" }
    var out: [Quip] = []

    if !waiting.isEmpty {
        out.append(Quip(text: waiting.count == 1 ? "승인 기다리는 세션이 있어요!" : "승인 대기 \(waiting.count)개! 확인해 줘요", mood: .urgent))
        for s in waiting.prefix(3) {
            out.append(Quip(text: s.topic.isEmpty ? "\(name(s)) 에서 기다려요!" : "\(name(s)): \(short(s.topic, 12)), 승인 부탁!", mood: .urgent))
        }
        return out   // 승인 대기가 있으면 그것만 재촉한다
    }
    if !working.isEmpty {
        out.append(Quip(text: working.count == 1 ? "\(name(working[0])) 열일 중! 힘내!" : "세션 \(working.count)개 동시에 달리는 중!", mood: .busy))
        for s in working.prefix(2) {
            let mins = Int(now.timeIntervalSince(s.since) / 60)
            if mins >= 10 { out.append(Quip(text: "\(name(s)) \(mins)분째 작업 중… 조금만 더!", mood: .busy)) }
            else if !s.topic.isEmpty { out.append(Quip(text: "지금은 \(short(s.topic, 14)) 처리 중", mood: .busy)) }
        }
        let subs = working.reduce(0) { $0 + $1.subagents.filter { $0.state != "done" }.count }
        if subs > 0 { out.append(Quip(text: "서브에이전트 \(subs)명 출동!", mood: .busy)) }
        let shells = working.reduce(0) { $0 + $1.shells }
        if shells > 0 { out.append(Quip(text: "백그라운드 셸 \(shells)개 지켜보는 중", mood: .busy)) }
    }
    for s in live.filter({ ($0.ctx?.pct ?? 0) >= 80 }).prefix(2) {
        out.append(Quip(text: "\(name(s)) 컨텍스트 \(s.ctx!.pct)%! 정리할 때예요", mood: working.isEmpty ? .idle : .busy))
    }
    for l in limits where l.pct >= 80 {
        out.append(Quip(text: "\(l.label) 한도 \(Int(l.pct.rounded()))%, 아껴 써요!", mood: working.isEmpty ? .idle : .busy))
    }
    if !paused.isEmpty { out.append(Quip(text: "일시중지 \(paused.count)개, 잊지 마세요", mood: .idle)) }
    if working.isEmpty {
        if let s = idle.max(by: { $0.updated < $1.updated }) {
            let quiet = now.timeIntervalSince(s.updated)
            if quiet > 30 * 60 {
                out.append(Quip(text: "다들 쉬는 중… 할 일 없나요?", mood: .sleepy))
                out.append(Quip(text: "Zzz… 다음 지시 기다려요", mood: .sleepy))
            } else {
                out.append(Quip(text: "\(name(s)) 끝났어요! 확인해 볼까요?", mood: .idle))
                out.append(Quip(text: idle.count == 1 ? "다음 작업 뭐 할까요?" : "세션 \(idle.count)개 대기 중, 다음 지시!", mood: .idle))
            }
        } else if out.isEmpty {
            out.append(Quip(text: "세션을 열어 일을 맡겨 주세요!", mood: .sleepy))
            out.append(Quip(text: "오늘도 화이팅!", mood: .sleepy))
        }
    }
    return out
}

// 꼬리가 오른쪽(감독 쪽)으로 난 말풍선
struct SpeechBubble: Shape {
    func path(in r: CGRect) -> Path {
        let tail: CGFloat = 7
        let body = CGRect(x: r.minX, y: r.minY, width: r.width - tail, height: r.height)
        var p = Path(roundedRect: body, cornerRadius: min(10, body.height / 2))
        let y = body.midY + 2
        p.move(to: CGPoint(x: body.maxX - 1, y: y - 5))
        p.addQuadCurve(to: CGPoint(x: r.maxX, y: y + 3), control: CGPoint(x: body.maxX + 2, y: y))
        p.addQuadCurve(to: CGPoint(x: body.maxX - 1, y: y + 4), control: CGPoint(x: body.maxX + 1, y: y + 4))
        p.closeSubpath()
        return p
    }
}

// 제목 오른쪽의 작은 캐릭터 — 세션 상태에 따라 움직이고, 재촉하고, 졸고, 말풍선으로 상황을 알려 준다. 누르면 다음 말.
// 그림은 고른 캐릭터(브레인롯 7종 또는 현장 감독)로 그린다 — 오른쪽 클릭이나 앱 메뉴 "캐릭터" 에서 고른다
struct Foreman: View {
    let groups: [Group]
    let limits: [Limit]
    let now: Date
    var compact = false     // 미니 화면 — 말풍선은 한 줄, 캐릭터는 조금 작게
    @StateObject private var clock = ForemanClock()
    @ObservedObject private var fontScale = BoardFontScale.shared
    @AppStorage("mascot") private var mascot: Mascot = .tralalero

    // 승인 대기가 없으면 돌아가는 말 끝에 캐릭터 유행어를 한 번 끼운다
    private func withCatchphrase(_ quips: [Quip]) -> [Quip] {
        guard let c = mascot.catchphrase, let first = quips.first, first.mood != .urgent else { return quips }
        return quips + [Quip(text: c, mood: first.mood)]
    }

    var body: some View {
        let quips = withCatchphrase(foremanQuips(groups: groups, limits: limits, now: now))
        let q = quips[clock.line % max(quips.count, 1)]
        let tint: Color = switch q.mood {
        case .urgent: Palette.color("waiting")
        case .busy: Palette.color("working")
        case .idle: Palette.accent
        case .sleepy: Color(white: 0.45)
        }
        HStack(alignment: .center, spacing: 2) {
            Text(q.text)
                .boardFont(11, weight: .bold)
                .foregroundStyle(.white)
                .multilineTextAlignment(.leading)
                .lineLimit(compact ? 1 : 2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.leading, 9).padding(.trailing, 16).padding(.vertical, 5)
                .background(SpeechBubble().fill(tint.opacity(0.92)))
                .overlay(SpeechBubble().stroke(Color.white.opacity(0.18), lineWidth: 0.6))
                // 보드 글자를 키우면 제목(layoutPriority)이 자리를 다 가져가 말풍선이 글자 없이 좁아졌다 — 키웠을 때만 몇 글자는 보이게 최소 폭
                .frame(minWidth: compact || fontScale.scale <= 1 ? nil : 100, maxWidth: compact ? 230 : 170, alignment: .trailing)
                .id(q.text)
                .transition(.asymmetric(
                    insertion: .scale(scale: 0.4, anchor: .trailing).combined(with: .opacity),
                    removal: .scale(scale: 0.8, anchor: .trailing).combined(with: .opacity)))
            Ambient { t in
                ZStack {
                    if mascot == .foreman {
                        figure(mood: q.mood, t: t, sinceChange: t - clock.changedAt)
                    } else {
                        BrainrotFigure(kind: mascot, mood: q.mood, t: t, sinceChange: t - clock.changedAt, blink: clock.blink)
                    }
                }
                .scaleEffect(compact ? 0.75 : 1)
                .frame(width: compact ? 35 : 46, height: compact ? 33 : 44)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { clock.next() }
        .contextMenu {
            Picker("캐릭터", selection: $mascot) {
                ForEach(Mascot.allCases) { Text($0.name).tag($0) }
            }
            .pickerStyle(.inline)
        }
        .hint("\(mascot.name): 누르면 다음 말 · 오른쪽 클릭으로 캐릭터 바꾸기")
    }

    // 동작 — 모두 시계 t 의 함수라 반복 애니메이션 없이 공유 시계 하나로 움직인다
    private func figure(mood: Mood, t: TimeInterval, sinceChange: Double) -> some View {
        let skin = mood == .urgent ? Color(red: 0.93, green: 0.42, blue: 0.32) : Color(red: 0.85, green: 0.47, blue: 0.34)
        // 말풍선이 바뀌면 폴짝 — 0.45초 동안 뛰었다 내려와 착지에 찌그러진다
        let hopP = min(max(sinceChange / 0.45, 0), 1)
        var hop = sinceChange < 0.45 ? -9 * sin(.pi * hopP) : 0
        var land = sinceChange >= 0.45 && sinceChange < 0.62 ? sin(.pi * (sinceChange - 0.45) / 0.17) : 0
        var tilt = 0.0, armDeg = 0.0, dx = 0.0, breathe = 0.0
        switch mood {
        case .urgent:
            // 제자리에서 연달아 뛰며 지휘봉을 머리 위로 흔든다
            let p = cycle(t, 0.55)
            hop = min(hop, -7 * sin(.pi * p))
            land = max(land, p > 0.85 ? sin(.pi * (p - 0.85) / 0.15) : 0)
            armDeg = 70 + 35 * sin(2 * .pi * t / 0.3)
            dx = 1.5 * sin(2 * .pi * t / 0.12)
        case .busy:
            // 지휘 — 지휘봉이 8자를 그리고 몸이 박자에 맞춰 기운다
            armDeg = 18 + 30 * sin(2 * .pi * t / 0.9) + 10 * sin(4 * .pi * t / 0.9)
            tilt = 7 * sin(2 * .pi * t / 0.9)
            hop = min(hop, -2.5 * abs(sin(2 * .pi * t / 0.9)))
        case .idle:
            // 말풍선 쪽을 두 번 콕콕 찌르고 쉰다
            let p = cycle(t, 2.4)
            let jab = p < 0.12 ? sin(.pi * p / 0.12) : (p > 0.18 && p < 0.30 ? sin(.pi * (p - 0.18) / 0.12) : 0)
            armDeg = 6 + 8 * jab
            dx = -2.5 * jab
            tilt = 5 * sin(2 * .pi * t / 2.4)
            breathe = swing(t, 1.6)
        case .sleepy:
            armDeg = -25
            tilt = 9 * sin(2 * .pi * t / 3.2)
            breathe = swing(t, 3.2)
        }
        let squash = land * 0.16
        return ZStack {
            // 지휘봉 든 팔 — 어깨를 축으로 돈다(양수 = 위로 든다)
            Capsule().fill(skin)
                .frame(width: 16, height: 4)
                .overlay(alignment: .leading) {
                    ZStack(alignment: .leading) {
                        Capsule().fill(Color(white: 0.95)).frame(width: 13, height: 2.5)
                        Circle().fill(mood == .urgent ? Color.yellow : Color.white).frame(width: 4, height: 4).offset(x: -2)
                    }
                    .offset(x: -11)
                }
                .offset(x: -9)
                .rotationEffect(.degrees(armDeg), anchor: .trailing)
                .offset(x: -6, y: 4)
            // 얼굴
            Circle().fill(skin)
                .frame(width: 28, height: 28)
                .overlay { eyes(mood: mood, t: t) }
                .overlay(alignment: .bottom) { mouth(mood: mood, t: t).offset(y: -5) }
                .overlay {
                    HStack(spacing: 13) {
                        ForEach(0..<2, id: \.self) { _ in
                            Ellipse().fill(Color(red: 1.0, green: 0.55, blue: 0.55).opacity(mood == .urgent ? 0.95 : 0.7)).frame(width: 5, height: 3)
                        }
                    }
                    .offset(y: 6)
                }
                .overlay(alignment: .top) {
                    // 안전모 — 착지할 때 살짝 들썩인다
                    UnevenRoundedRectangle(topLeadingRadius: 10, bottomLeadingRadius: 2, bottomTrailingRadius: 2, topTrailingRadius: 10)
                        .fill(Color(red: 1.0, green: 0.80, blue: 0.25))
                        .frame(width: 30, height: 10)
                        .offset(y: -4 - 2 * land)
                }
            // 머리 위 표시: 재촉 "!" · 졸음 "z"
            if mood == .urgent {
                Text("!").font(.system(size: 13, weight: .black)).foregroundStyle(Color.yellow)
                    .scaleEffect(1 + 0.25 * swing(t, 0.4))
                    .offset(x: 14, y: -19)
            } else if mood == .sleepy {
                ForEach(0..<2, id: \.self) { i in
                    let p = cycle(t + Double(i) * 1.2, 2.4)
                    Text("z").font(.system(size: 8 + 3 * p, weight: .bold)).foregroundStyle(.white.opacity(1 - p))
                        .offset(x: 12 + 6 * p, y: -10 - 14 * p)
                }
            }
        }
        .frame(width: 46, height: 44)
        .scaleEffect(x: 1 + squash + 0.03 * breathe, y: 1 - squash + 0.03 * breathe, anchor: .bottom)
        .rotationEffect(.degrees(tilt), anchor: .bottom)
        .offset(x: dx, y: hop + 2)
    }

    @ViewBuilder private func eyes(mood: Mood, t: TimeInterval) -> some View {
        HStack(spacing: 7) {
            ForEach(0..<2, id: \.self) { _ in
                switch mood {
                case .sleepy:
                    Capsule().fill(Color(white: 0.12)).frame(width: 5, height: 1.2)
                case .urgent:
                    Circle().fill(Color(white: 0.12)).frame(width: 5, height: 5)
                default:
                    Capsule().fill(Color(white: 0.12)).frame(width: 3.5, height: clock.blink ? 0.8 : 5)
                }
            }
        }
        // 말풍선 쪽(왼쪽)을 흘끗 본다
        .offset(x: mood == .sleepy ? 0 : -1.5, y: 1)
    }

    @ViewBuilder private func mouth(mood: Mood, t: TimeInterval) -> some View {
        switch mood {
        case .urgent:
            // 소리치는 입 — 열렸다 닫힌다
            Ellipse().fill(Color(white: 0.12)).frame(width: 7, height: 3 + 3 * swing(t, 0.3))
        case .busy:
            Capsule().fill(Color(white: 0.12)).frame(width: 8, height: 2)
        case .idle:
            // 웃는 입
            Path { p in p.move(to: CGPoint(x: 0, y: 0)); p.addQuadCurve(to: CGPoint(x: 8, y: 0), control: CGPoint(x: 4, y: 4)) }
                .stroke(Color(white: 0.12), style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
                .frame(width: 8, height: 4)
        case .sleepy:
            Circle().fill(Color(white: 0.12)).frame(width: 3, height: 3)
        }
    }
}

// MARK: - 브레인롯 캐릭터

// 제목 오른쪽 캐릭터 — 브레인롯 7종과 기존 현장 감독. 고른 값은 "mascot" 에 남아 다시 켜도 유지된다
enum Mascot: String, CaseIterable, Identifiable {
    case tralalero, bombardiro, tungtung, lirili, patapim, boneca, cappuccino, foreman
    var id: String { rawValue }

    var name: String {
        switch self {
        case .tralalero: "트랄랄레로 트랄랄라"
        case .bombardiro: "봄바르디로 크로코딜로"
        case .tungtung: "퉁 퉁 퉁 사후르"
        case .lirili: "리릴리 라릴라"
        case .patapim: "브르르 브르르 파타핌"
        case .boneca: "보네카 암발라부"
        case .cappuccino: "카푸치노 아사시노"
        case .foreman: "현장 감독"
        }
    }

    // 말풍선에 가끔 끼우는 유행어 — 감독은 없다
    var catchphrase: String? {
        switch self {
        case .tralalero: "트랄랄레로 트랄랄라~"
        case .bombardiro: "봄바르디로 크로코딜로!"
        case .tungtung: "퉁 퉁 퉁 퉁 사후르!"
        case .lirili: "리릴리 라릴라~"
        case .patapim: "브르르 브르르 파타핌!"
        case .boneca: "보네카 암발라부!"
        case .cappuccino: "카푸치노 아사시노!"
        case .foreman: nil
        }
    }
}

private let ink = Color(white: 0.12)

// 0~1 비율 좌표의 꼭짓점을 이은 다각형 — 지느러미·이빨·꼬리 같은 작은 조각
struct Poly: Shape {
    let pts: [(CGFloat, CGFloat)]
    init(_ pts: [(CGFloat, CGFloat)]) { self.pts = pts }
    func path(in r: CGRect) -> Path {
        Path { p in
            p.addLines(pts.map { CGPoint(x: r.minX + $0.0 * r.width, y: r.minY + $0.1 * r.height) })
            p.closeSubpath()
        }
    }
}

// 만화 눈 — 흰자에 눈동자. 졸리거나 깜빡이면 감고, 재촉할 땐 눈동자가 작아진다(놀란 눈). 눈동자는 말풍선 쪽(왼쪽)을 본다
struct CartoonEye: View {
    let size: CGFloat
    let mood: Mood
    let blink: Bool
    var body: some View {
        if mood == .sleepy || blink {
            Capsule().fill(ink).frame(width: size, height: 1.2)
        } else {
            Circle().fill(Color.white).frame(width: size, height: size)
                .overlay(Circle().fill(ink).frame(width: size * (mood == .urgent ? 0.42 : 0.56)).offset(x: -size * 0.12))
        }
    }
}

// 아래로 뾰족한 흰 이빨 줄
struct Teeth: View {
    let n: Int
    let width: CGFloat
    let height: CGFloat
    var body: some View {
        HStack(spacing: 0) {
            ForEach(0..<n, id: \.self) { _ in
                Poly([(0, 0), (1, 0), (0.5, 1)]).fill(Color.white).frame(width: width / CGFloat(n), height: height)
            }
        }
    }
}

// 운동화 — 흰 신발에 빨간 곡선, 앞코가 왼쪽
struct Sneaker: View {
    var body: some View {
        RoundedRectangle(cornerRadius: 1.5).fill(Color.white).frame(width: 7, height: 3.5)
            .overlay(Path { p in
                p.move(to: CGPoint(x: 1.5, y: 1.4))
                p.addQuadCurve(to: CGPoint(x: 6, y: 0.8), control: CGPoint(x: 3, y: 3.3))
            }.stroke(Color(red: 0.95, green: 0.30, blue: 0.25), lineWidth: 0.8))
            .offset(x: -1.5)
    }
}

// 브레인롯 캐릭터 그림과 동작 — 감독처럼 모두 공유 시계 t 의 함수다.
// 말풍선이 바뀌면 폴짝 뛰고, 기분에 따라 박자(beat)가 빨라지거나 느려진다. 머리 위 "!"·"z" 와 크기(46×44)는 감독과 같다
struct BrainrotFigure: View {
    let kind: Mascot
    let mood: Mood
    let t: TimeInterval
    let sinceChange: Double
    let blink: Bool

    // 기분별 박자 — 재촉 0.3초 · 작업 0.6초 · 대기 1.6초 · 졸음 3초에 한 번
    private var period: Double {
        switch mood { case .urgent: 0.3; case .busy: 0.6; case .idle: 1.6; case .sleepy: 3.0 }
    }
    private var beat: Double { cycle(t, period) }          // 0 → 1
    private var wave: Double { sin(2 * .pi * beat) }       // -1 ~ 1

    var body: some View {
        // 말풍선이 바뀌면 폴짝 — 0.45초 동안 뛰었다 내려와 착지에 찌그러진다(감독과 같은 곡선)
        let hopP = min(max(sinceChange / 0.45, 0), 1)
        var hop = sinceChange < 0.45 ? -9 * sin(.pi * hopP) : 0
        var land = sinceChange >= 0.45 && sinceChange < 0.62 ? sin(.pi * (sinceChange - 0.45) / 0.17) : 0
        var tilt = 0.0, dx = 0.0, breathe = 0.0
        switch mood {
        case .urgent:
            let p = cycle(t, 0.55)
            hop = min(hop, -6 * sin(.pi * p))
            land = max(land, p > 0.85 ? sin(.pi * (p - 0.85) / 0.15) : 0)
            dx = 1.5 * sin(2 * .pi * t / 0.12)
        case .busy:
            tilt = 5 * sin(2 * .pi * t / 0.9)
            hop = min(hop, -2 * abs(sin(2 * .pi * t / 0.9)))
        case .idle:
            tilt = 4 * sin(2 * .pi * t / 2.4)
            breathe = swing(t, 1.6)
        case .sleepy:
            tilt = 8 * sin(2 * .pi * t / 3.2)
            breathe = swing(t, 3.2)
        }
        let squash = land * 0.16
        return ZStack {
            character
            if mood == .urgent {
                Text("!").font(.system(size: 13, weight: .black)).foregroundStyle(Color.yellow)
                    .scaleEffect(1 + 0.25 * swing(t, 0.4))
                    .offset(x: 14, y: -19)
            } else if mood == .sleepy {
                ForEach(0..<2, id: \.self) { i in
                    let p = cycle(t + Double(i) * 1.2, 2.4)
                    Text("z").font(.system(size: 8 + 3 * p, weight: .bold)).foregroundStyle(.white.opacity(1 - p))
                        .offset(x: 12 + 6 * p, y: -10 - 14 * p)
                }
            }
        }
        .frame(width: 46, height: 44)
        .scaleEffect(x: 1 + squash + 0.03 * breathe, y: 1 - squash + 0.03 * breathe, anchor: .bottom)
        .rotationEffect(.degrees(tilt), anchor: .bottom)
        .offset(x: dx, y: hop + 2)
    }

    @ViewBuilder private var character: some View {
        switch kind {
        case .tralalero: tralalero
        case .bombardiro: bombardiro
        case .tungtung: tungtung
        case .lirili: lirili
        case .patapim: patapim
        case .boneca: boneca
        case .cappuccino: cappuccino
        case .foreman: EmptyView()   // 감독은 Foreman.figure 가 그린다
        }
    }

    // 트랄랄레로 트랄랄라 — 파란 상어가 세 다리에 운동화를 신고 걷는다. 재촉하면 제자리 뛰기, 졸리면 멈춘다
    private var tralalero: some View {
        let blue = Color(red: 0.36, green: 0.58, blue: 0.86), belly = Color(red: 0.90, green: 0.95, blue: 1.0)
        let lift: Double = mood == .sleepy ? 0 : mood == .idle ? 1.5 : 3
        return ZStack {
            ForEach(0..<3, id: \.self) { i in
                let up = max(0, sin(2 * .pi * beat + Double(i) * 2.1))
                VStack(spacing: -1) {
                    Capsule().fill(blue).frame(width: 2.6, height: 8)
                    Sneaker()
                }
                .offset(x: Double(i - 1) * 7 - 2, y: 13 - lift * up)
            }
            // 꼬리 — 박자에 맞춰 흔든다
            Poly([(0, 0.5), (1, 0), (0.7, 0.5), (1, 1)]).fill(blue)
                .frame(width: 10, height: 13)
                .rotationEffect(.degrees(12 * wave), anchor: .leading)
                .offset(x: 18, y: -5)
            Poly([(0, 1), (0.55, 0), (1, 1)]).fill(blue).frame(width: 9, height: 8).offset(x: 2, y: -14)   // 등지느러미
            Capsule().fill(blue).frame(width: 32, height: 15).offset(y: -4)
            Capsule().fill(belly).frame(width: 22, height: 6).offset(x: -3, y: 0.5)
            HStack(spacing: 1.5) {   // 아가미
                ForEach(0..<3, id: \.self) { _ in Capsule().fill(ink.opacity(0.35)).frame(width: 0.8, height: 5) }
            }
            .offset(x: 1, y: -5)
            CartoonEye(size: 5.5, mood: mood, blink: blink).offset(x: -9, y: -7)
            Capsule().fill(ink).frame(width: 9, height: 1.2).offset(x: -11, y: -1.5)   // 입
            Teeth(n: 4, width: 8, height: 2.5).offset(x: -11, y: 0.3)
        }
    }

    // 봄바르디로 크로코딜로 — 악어 머리를 단 폭격기. 프로펠러가 돌고, 재촉하면 폭탄을 떨어뜨린다
    private var bombardiro: some View {
        let green = Color(red: 0.38, green: 0.64, blue: 0.34), dark = Color(red: 0.26, green: 0.46, blue: 0.24)
        let metal = Color(red: 0.62, green: 0.66, blue: 0.70)
        let spin = abs(cos(2 * .pi * t / (mood == .sleepy ? 1.2 : mood == .idle ? 0.25 : 0.1)))
        let drop = cycle(t, 0.6)
        return ZStack {
            if mood == .urgent {
                Capsule().fill(Color(white: 0.22)).frame(width: 4, height: 6)
                    .offset(x: 4, y: 6 + 16 * drop)
                    .opacity(1 - drop)
            }
            Poly([(0.3, 1), (0.8, 1), (1, 0), (0.6, 0)]).fill(metal.opacity(0.7))   // 먼 쪽 날개 — 몸 뒤로 비스듬히
                .frame(width: 18, height: 8).offset(x: 7, y: -9)
            Poly([(0, 1), (0.35, 0), (1, 0), (0.9, 1)]).fill(metal).frame(width: 8, height: 9).offset(x: 17, y: -8)   // 꼬리날개
            Capsule().fill(green).frame(width: 30, height: 12).offset(x: 3)
            HStack(spacing: 2) {   // 등 돌기
                ForEach(0..<3, id: \.self) { _ in Poly([(0, 1), (0.5, 0), (1, 1)]).fill(dark).frame(width: 2.5, height: 2) }
            }
            .offset(x: 9, y: -6.5)
            RoundedRectangle(cornerRadius: 3).fill(green).frame(width: 15, height: 6).offset(x: -15, y: 2)   // 주둥이
            Capsule().fill(dark).frame(width: 12, height: 1).offset(x: -15, y: 3)
            Teeth(n: 4, width: 10, height: 2).offset(x: -15, y: 4.5)
            Circle().fill(dark).frame(width: 1.5, height: 1.5).offset(x: -21, y: 0)   // 콧구멍
            Circle().fill(green).frame(width: 8, height: 8)   // 툭 튀어나온 눈두덩
                .overlay(CartoonEye(size: 5, mood: mood, blink: blink))
                .offset(x: -8, y: -5)
            Poly([(0.15, 0), (0.75, 0), (0.6, 1), (0, 1)]).fill(metal)   // 가까운 쪽 날개 — 몸 아래 앞으로
                .frame(width: 22, height: 8).offset(x: 3, y: 8)
            Capsule().fill(Color(white: 0.35)).frame(width: 7, height: 4.5).offset(x: -3, y: 10)   // 날개 위 엔진
            Capsule().fill(Color(white: 0.9).opacity(0.85)).frame(width: 2, height: 12)   // 프로펠러 — 길이가 줄었다 늘며 도는 것처럼 보인다
                .scaleEffect(y: 0.15 + 0.85 * spin)
                .offset(x: -7.5, y: 10)
        }
    }

    // 퉁 퉁 퉁 사후르 — 통나무 몸에 야구방망이. 박자에 맞춰 방망이로 바닥을 퉁퉁 내려친다
    private var tungtung: some View {
        let wood = Color(red: 0.78, green: 0.57, blue: 0.35), top = Color(red: 0.92, green: 0.76, blue: 0.52)
        let bark = Color(red: 0.52, green: 0.34, blue: 0.19), batC = Color(red: 0.95, green: 0.84, blue: 0.62)
        // 바닥을 치는 정도(0~1) — 박자 가운데서 짧게 내려친다. 졸리면 방망이를 내려놓는다
        let hit = mood == .sleepy ? 0 : pow(abs(sin(.pi * beat)), mood == .urgent ? 2 : 4)
        let angle = mood == .sleepy ? 160 : 20 + 130 * hit
        return ZStack {
            HStack(spacing: 5) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(bark).frame(width: 3, height: 7) } }
                .offset(x: -4, y: 18)
            RoundedRectangle(cornerRadius: 5).fill(wood).frame(width: 20, height: 30).offset(x: -4, y: 1)
            HStack(spacing: 4) {   // 나뭇결
                ForEach(0..<3, id: \.self) { _ in Capsule().fill(bark.opacity(0.25)).frame(width: 0.8, height: 22) }
            }
            .offset(x: -4, y: 4)
            Ellipse().fill(top).frame(width: 20, height: 6)   // 윗면 나이테
                .overlay(Ellipse().stroke(bark.opacity(0.45), lineWidth: 0.6).frame(width: 11, height: 3))
                .offset(x: -4, y: -14)
            HStack(spacing: 3) {
                CartoonEye(size: 6.5, mood: mood, blink: blink)
                CartoonEye(size: 6.5, mood: mood, blink: blink)
            }
            .offset(x: -5, y: -6)
            if mood == .urgent {
                Ellipse().fill(ink).frame(width: 7, height: 3 + 3 * swing(t, 0.3)).offset(x: -5, y: 4)
            } else {
                Path { p in p.move(to: CGPoint(x: 0, y: 0)); p.addQuadCurve(to: CGPoint(x: 10, y: 0), control: CGPoint(x: 5, y: 5)) }
                    .stroke(ink, style: StrokeStyle(lineWidth: 1.4, lineCap: .round))
                    .frame(width: 10, height: 5)
                    .offset(x: -5, y: 5)
            }
            // 방망이 — 아래 절반을 비워 손이 회전 중심이 되게 한다
            Poly([(0.3, 1), (0, 0.06), (0.5, 0), (1, 0.06), (0.7, 1)]).fill(batC)
                .frame(width: 5, height: 17)
                .frame(width: 5, height: 34, alignment: .top)
                .rotationEffect(.degrees(angle))
                .offset(x: 8, y: 2)
            Circle().fill(bark).frame(width: 4, height: 4).offset(x: 8, y: 2)   // 손
            if hit > 0.7 {
                Text("퉁").font(.system(size: 7, weight: .black)).foregroundStyle(Color.yellow)
                    .opacity((hit - 0.7) / 0.3)
                    .offset(x: 19, y: 19)
            }
        }
    }

    // 리릴리 라릴라 — 코끼리 머리에 선인장 몸, 샌들, 자명종. 귀가 펄럭이고 코가 흔들리며, 재촉하면 자명종이 울린다
    private var lirili: some View {
        let cactus = Color(red: 0.36, green: 0.68, blue: 0.40), spine = Color(red: 0.86, green: 0.95, blue: 0.80)
        let grey = Color(red: 0.64, green: 0.66, blue: 0.72), pink = Color(red: 0.95, green: 0.72, blue: 0.76)
        let sandal = Color(red: 0.62, green: 0.40, blue: 0.22), bell = Color(red: 0.92, green: 0.30, blue: 0.28)
        let sway = (mood == .sleepy ? 4 : mood == .urgent ? 22 : 14) * wave
        let ring = mood == .urgent ? 14 * sin(2 * .pi * t / 0.08) : 0
        let spines: [(Double, Double)] = [(-3, 4), (2, 7), (-2, 12), (3, 14), (-10, 2)]
        return ZStack {
            HStack(spacing: 4) {   // 샌들 신은 발
                ForEach(0..<2, id: \.self) { _ in
                    VStack(spacing: 0) {
                        Capsule().fill(cactus).frame(width: 3, height: 5)
                        Capsule().fill(sandal).frame(width: 7, height: 2)
                    }
                }
            }
            .offset(y: 19)
            Capsule().fill(cactus).frame(width: 13, height: 20).offset(y: 8)   // 선인장 몸통
            Capsule().fill(cactus).frame(width: 4, height: 9).offset(x: -10, y: 4)   // 왼쪽 가지
            Capsule().fill(cactus).frame(width: 6, height: 3.5).offset(x: -8, y: 8)
            Capsule().fill(cactus).frame(width: 7, height: 3.5).offset(x: 8, y: 7)   // 자명종 든 오른쪽 가지
            ForEach(0..<spines.count, id: \.self) { i in
                Circle().fill(spine).frame(width: 1.2, height: 1.2).offset(x: spines[i].0, y: spines[i].1)
            }
            Ellipse().fill(grey).frame(width: 10, height: 14)   // 귀 — 펄럭인다
                .overlay(Ellipse().fill(pink).frame(width: 6, height: 9))
                .scaleEffect(x: 1 + 0.12 * wave, anchor: .trailing)
                .offset(x: -10, y: -10)
            Ellipse().fill(grey).frame(width: 10, height: 14)
                .overlay(Ellipse().fill(pink).frame(width: 6, height: 9))
                .scaleEffect(x: 1 + 0.12 * wave, anchor: .leading)
                .offset(x: 10, y: -10)
            Circle().fill(grey).frame(width: 18, height: 18).offset(y: -11)
            HStack(spacing: 5) {
                CartoonEye(size: 4.5, mood: mood, blink: blink)
                CartoonEye(size: 4.5, mood: mood, blink: blink)
            }
            .offset(y: -13)
            // 코 — 위 끝을 축으로 흔들린다(아래 절반을 비워 회전 중심을 위로)
            Capsule().fill(grey).frame(width: 5, height: 13)
                .frame(width: 5, height: 26, alignment: .bottom)
                .rotationEffect(.degrees(sway))
                .offset(y: -6)
            ZStack {   // 자명종
                HStack(spacing: 4) { Circle().fill(bell).frame(width: 4, height: 4); Circle().fill(bell).frame(width: 4, height: 4) }
                    .offset(y: -4.5)
                Circle().fill(Color.white).frame(width: 9, height: 9).overlay(Circle().stroke(bell, lineWidth: 1.2))
                Capsule().fill(ink).frame(width: 0.9, height: 3).offset(y: -1.2)
                Capsule().fill(ink).frame(width: 2.5, height: 0.9).offset(x: 1)
            }
            .rotationEffect(.degrees(ring))
            .offset(x: 13, y: 4)
        }
    }

    // 브르르 브르르 파타핌 — 큰 코의 원숭이 머리에 나무 몸. 잎이 살랑이고 코가 출렁이며, 재촉하면 부르르 떤다
    private var patapim: some View {
        let fur = Color(red: 0.72, green: 0.46, blue: 0.26), face = Color(red: 0.96, green: 0.82, blue: 0.64)
        let nose = Color(red: 0.94, green: 0.60, blue: 0.46), bark = Color(red: 0.50, green: 0.33, blue: 0.20)
        let leaf = Color(red: 0.42, green: 0.72, blue: 0.36)
        let shake = mood == .urgent ? 1.2 * sin(2 * .pi * t / 0.05) : 0
        let leaves: [(Double, Double)] = [(-10, 3), (-7, -1), (10, 3), (7, -1)]
        return ZStack {
            HStack(spacing: 6) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(bark).frame(width: 8, height: 3.5) } }   // 큰 발
                .offset(y: 20)
            HStack(spacing: 5) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(bark).frame(width: 3, height: 8) } }
                .offset(y: 15)
            RoundedRectangle(cornerRadius: 4).fill(bark).frame(width: 13, height: 14).offset(y: 6)   // 나무 몸통
            ForEach(0..<leaves.count, id: \.self) { i in
                Circle().fill(leaf).frame(width: 7, height: 7)
                    .offset(x: leaves[i].0, y: leaves[i].1 + 0.8 * wave * (i % 2 == 0 ? 1 : -1))
            }
            Circle().fill(fur).frame(width: 22, height: 22).offset(y: -9)
            Ellipse().fill(face).frame(width: 16, height: 14).offset(y: -7)
            HStack(spacing: 6) {
                CartoonEye(size: 4, mood: mood, blink: blink)
                CartoonEye(size: 4, mood: mood, blink: blink)
            }
            .offset(y: -11)
            Ellipse().fill(nose).frame(width: 9, height: 10)   // 큰 코 — 아래로 늘어져 출렁인다
                .scaleEffect(y: 1 + 0.08 * wave, anchor: .top)
                .offset(y: -2)
        }
        .offset(x: shake)
    }

    // 보네카 암발라부 — 개구리 머리, 타이어 몸통, 사람 다리. 타이어가 구르며 걷고, 재촉하면 입을 벌린다
    private var boneca: some View {
        let frog = Color(red: 0.44, green: 0.74, blue: 0.34), frogDark = Color(red: 0.30, green: 0.55, blue: 0.24)
        let skin = Color(red: 0.93, green: 0.74, blue: 0.58)
        let roll = mood == .sleepy ? 0 : t / period * 90
        return ZStack {
            ForEach(0..<2, id: \.self) { i in   // 사람 다리 — 번갈아 든다
                VStack(spacing: -1) {
                    Capsule().fill(skin).frame(width: 3, height: 9)
                    Capsule().fill(ink).frame(width: 6, height: 2.5).offset(x: -1.5)
                }
                .offset(x: Double(i) * 7 - 3.5, y: 16 - (mood == .sleepy ? 0 : 2 * max(0, sin(2 * .pi * beat + Double(i) * .pi))))
            }
            ZStack {   // 타이어 몸통 — 굴러간다
                Circle().fill(Color(white: 0.17))
                ForEach(0..<10, id: \.self) { k in
                    Rectangle().fill(Color(white: 0.32)).frame(width: 2, height: 3)
                        .offset(y: -8.5).rotationEffect(.degrees(Double(k) * 36))
                }
                Circle().fill(Color(white: 0.55)).frame(width: 8, height: 8)
                Circle().fill(Color(white: 0.30)).frame(width: 3, height: 3)
            }
            .frame(width: 20, height: 20)
            .rotationEffect(.degrees(roll))
            .offset(y: 4)
            Ellipse().fill(frog).frame(width: 26, height: 13).offset(y: -9)   // 개구리 머리
            ForEach([-7.0, 7.0], id: \.self) { x in   // 툭 튀어나온 눈
                Circle().fill(frog).frame(width: 9, height: 9)
                    .overlay(CartoonEye(size: 6, mood: mood, blink: blink))
                    .offset(x: x, y: -15)
            }
            Circle().fill(frogDark).frame(width: 2, height: 2).offset(x: -9, y: -8)   // 점
            Circle().fill(frogDark).frame(width: 1.5, height: 1.5).offset(x: 9, y: -7)
            if mood == .urgent {
                Ellipse().fill(Color(red: 0.55, green: 0.15, blue: 0.20)).frame(width: 12, height: 3 + 2 * swing(t, 0.3)).offset(y: -6)
            } else {
                Path { p in p.move(to: CGPoint(x: 0, y: 0)); p.addQuadCurve(to: CGPoint(x: 16, y: 0), control: CGPoint(x: 8, y: 3)) }
                    .stroke(frogDark, style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
                    .frame(width: 16, height: 3)
                    .offset(y: -6)
            }
        }
    }

    // 카푸치노 아사시노 — 닌자 복면을 쓴 카푸치노 잔. 카타나를 휘두르고, 바쁠 땐 속도선을 남기며 오간다
    private var cappuccino: some View {
        let cup = Color(white: 0.96), coffee = Color(red: 0.55, green: 0.36, blue: 0.22)
        let foam = Color(red: 0.97, green: 0.89, blue: 0.76), band = Color(red: 0.88, green: 0.22, blue: 0.24)
        let slash: Double = switch mood {
        case .urgent: -70 + 110 * swing(t, 0.25)
        case .busy: -40 + 60 * swing(t, 0.6)
        case .idle: -35 + 6 * wave
        case .sleepy: 40
        }
        let dash = mood == .busy ? 3 * sin(2 * .pi * t / 0.6) : 0
        let flutter = 18 * sin(2 * .pi * t / (mood == .sleepy ? 2.0 : 0.35))
        return ZStack {
            if mood == .busy || mood == .urgent {   // 속도선
                ForEach(0..<3, id: \.self) { i in
                    Capsule().fill(Color.white.opacity(0.35)).frame(width: 6 + 3 * Double(i % 2), height: 1)
                        .offset(x: 20, y: Double(i) * 5 - 4)
                }
            }
            HStack(spacing: 5) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(ink).frame(width: 2.5, height: 7) } }   // 다리
                .offset(y: 18)
            ForEach(0..<2, id: \.self) { i in   // 머리띠 꼬리 — 바람에 날린다
                Capsule().fill(band).frame(width: 8, height: 2)
                    .rotationEffect(.degrees(flutter + Double(i) * 25 - 10), anchor: .leading)
                    .offset(x: 14, y: -9 + Double(i) * 2)
            }
            Circle().stroke(cup, lineWidth: 2.5).frame(width: 8, height: 9).offset(x: 11, y: 4)   // 손잡이
            UnevenRoundedRectangle(topLeadingRadius: 2, bottomLeadingRadius: 7, bottomTrailingRadius: 7, topTrailingRadius: 2)
                .fill(cup).frame(width: 22, height: 21).offset(y: 3)
            Ellipse().fill(coffee).frame(width: 22, height: 6).offset(y: -7.5)
            Ellipse().fill(foam).frame(width: 15, height: 3.5).offset(y: -7.5)
            Rectangle().fill(Color(white: 0.14)).frame(width: 22, height: 7).offset(y: -1)   // 닌자 복면 — 눈만 드러난다
            Rectangle().fill(band).frame(width: 22, height: 1.5).offset(y: -4.5)
            HStack(spacing: 4) {
                CartoonEye(size: 4.5, mood: mood, blink: blink)
                CartoonEye(size: 4.5, mood: mood, blink: blink)
            }
            .offset(y: -1)
            Capsule().fill(ink).frame(width: 5, height: 2).offset(x: -12, y: 6)   // 팔
            // 카타나 — 손잡이 가운데(손)가 회전 중심이 되게 위로 붙여 둔다
            VStack(spacing: 0) {
                Capsule().fill(Color(white: 0.88)).frame(width: 2, height: 16)
                Capsule().fill(Color(red: 0.85, green: 0.70, blue: 0.30)).frame(width: 6, height: 1.6)
                Capsule().fill(Color(white: 0.15)).frame(width: 2.4, height: 6)
            }
            .frame(width: 6, height: 41, alignment: .top)
            .rotationEffect(.degrees(slash))
            .offset(x: -14, y: 6)
        }
        .offset(x: dash)
    }
}

// viewport = 목록 칸(스크롤 또는 빈 화면 안내) 높이, board = 보드 내용 전체 높이 — 창 크기 단계를 고를 때 쓴다
final class CardTopsBox { var values: [String: CGFloat] = [:]; var viewport: CGFloat = 0; var board: CGFloat = 0 }

// 오른쪽 독의 터미널 열기 단추 — 설치된 앱은 그 앱 아이콘으로 열 수 있고, 없으면 흐리게 막아 둔다
struct TerminalApp: Identifiable {
    let id: String      // 번들 ID
    let name: String
}
let terminalApps = [TerminalApp(id: "com.mitchellh.ghostty", name: "Ghostty"),
                    TerminalApp(id: "com.googlecode.iterm2", name: "iTerm2"),
                    TerminalApp(id: "com.apple.Terminal", name: "터미널"),
                    TerminalApp(id: "com.stablyai.orca", name: "Orca")]

// 명령줄 도구를 찾는 곳 — 보드는 로그인 셸의 PATH 를 모르고 실행되므로 흔한 설치 위치를 직접 본다
func findCommand(_ name: String) -> URL? {
    let home = NSHomeDirectory()
    let dirs = ["\(home)/.local/bin", "\(home)/.\(name)/bin", "\(home)/.cargo/bin", "\(home)/bin",
                "/opt/homebrew/bin", "/usr/local/bin"]
    return dirs.map { URL(fileURLWithPath: "\($0)/\(name)") }
        .first { FileManager.default.isExecutableFile(atPath: $0.path) }
}

final class TerminalAppStore: ObservableObject {
    static let shared = TerminalAppStore()
    @Published private(set) var urls: [String: URL] = [:]       // 설치된 앱 위치
    @Published private(set) var running: Set<String> = []
    // 앱 아이콘 — iconForFile 은 느려서 화면을 그릴 때마다 부르면 보드가 멈칫한다. 설치 위치가 바뀔 때만 다시 읽는다
    private(set) var icons: [String: NSImage] = [:]
    private var timer: Timer?
    private init() {
        refresh()
        // 설치·삭제는 드물어 1분마다, 실행·종료는 알림으로 바로 반영한다
        timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in self?.refresh() }
        let nc = NSWorkspace.shared.notificationCenter
        for n in [NSWorkspace.didLaunchApplicationNotification, NSWorkspace.didTerminateApplicationNotification] {
            nc.addObserver(forName: n, object: nil, queue: .main) { [weak self] _ in self?.refreshRunning() }
        }
    }
    func refresh() {
        var u: [String: URL] = [:]
        for a in terminalApps {
            u[a.id] = NSWorkspace.shared.urlForApplication(withBundleIdentifier: a.id)
        }
        if u != urls {
            var ic: [String: NSImage] = [:]
            for a in terminalApps where u[a.id] != nil {
                ic[a.id] = NSWorkspace.shared.icon(forFile: u[a.id]!.path)
            }
            icons = ic
            urls = u
        }
        refreshRunning()
    }
    private func refreshRunning() {
        let r = Set(NSWorkspace.shared.runningApplications.compactMap(\.bundleIdentifier)).intersection(terminalApps.map(\.id))
        if r != running { running = r }
    }
    func open(_ id: String) {
        guard let url = urls[id] else { return }
        let cfg = NSWorkspace.OpenConfiguration()
        cfg.activates = true
        NSWorkspace.shared.openApplication(at: url, configuration: cfg)
    }
}

func shellQuote(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }

// 셸 명령줄을 터미널 새 창에서 실행한다 — Ghostty 가 있으면 Ghostty, 없으면 macOS 터미널
func runInTerminal(_ shellCmd: String) {
    let quote = { (t: String) in "\"" + t.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\"" }
    let script: String
    if NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.mitchellh.ghostty") != nil {
        script = """
        tell application "Ghostty"
          set cfg to new surface configuration
          set initial working directory of cfg to \(quote(NSHomeDirectory()))
          set initial input of cfg to \(quote(shellCmd)) & linefeed
          new window with configuration cfg
          activate
        end tell
        """
    } else {
        script = """
        tell application "Terminal"
          do script \(quote(shellCmd))
          activate
        end tell
        """
    }
    ghosttyQueue.async { _ = runScript(script) }
}

// 오른쪽 아래 독: Ghostty·iTerm2·터미널·Orca 를 바로 연다.
// 평소엔 창 오른쪽 아래 끝의 탭만 보이다가 그 탭(창 오른쪽 끝 아래쪽)에 마우스를 대면 보드 위로 미끄러져 나오고, 독에서 벗어나면 다시 숨는다.
// 보드 위치가 왼쪽이면 보드의 왼쪽 끝(화면 바깥쪽)에 거울처럼 붙는다 — 오른쪽 끝은 터미널과의 경계라 경계를 잡으러 가다 열리지 않게
struct EdgeDock: View {
    static let size: CGFloat = 36, gap: CGFloat = 8, pad: CGFloat = 8
    static let trigger: CGFloat = 16    // 창 오른쪽 끝에서 이 폭(탭 폭) 안에 마우스가 오면 연다
    static let bottomInset: CGFloat = 34    // 창 아래 끝과 독 사이 — 오른쪽 아래 투명도 슬라이더를 가리지 않게
    static let hintHeight: CGFloat = 26
    static var width: CGFloat { size + pad * 2 }
    static var height: CGFloat { CGFloat(terminalApps.count) * (size + gap) - gap + pad * 2 }
    // 오른쪽 끝에 마우스를 대면 열리는 높이 — 창 아래에서 독 윗변 조금 위까지만. 위쪽 끝에서는 열지 않는다
    static var triggerHeight: CGFloat { bottomInset + height + 12 }
    @ObservedObject private var pointer = PointerTracker.shared
    @ObservedObject private var store = TerminalAppStore.shared
    @ObservedObject private var hub = TerminalHub.shared
    @State private var open = false
    @State private var overPanel = false
    @State private var closing: DispatchWorkItem?
    @State private var hovered: Int?        // 마우스가 올라간 아이콘 순서

    // 오른쪽 끝이나 독 위에 있으면 연다. 벗어나면 조금 기다렸다 닫는다 — 끝에서 독으로 옮겨 가는 사이에 닫히지 않게
    private func sync() {
        closing?.cancel()
        if pointer.atEdge || overPanel {
            if !open { open = true }
        } else if open {
            let w = DispatchWorkItem { open = false; hovered = nil }
            closing = w
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35, execute: w)
        }
    }

    private func index(at y: CGFloat) -> Int? {
        let k = Int(((y - Self.pad) / (Self.size + Self.gap)).rounded(.down))
        guard y >= Self.pad, k >= 0, k < terminalApps.count,
              y - Self.pad - CGFloat(k) * (Self.size + Self.gap) <= Self.size else { return nil }
        return k
    }

    var body: some View {
        let edge: Edge = hub.boardOnLeft ? .leading : .trailing
        ZStack(alignment: hub.boardOnLeft ? .bottomLeading : .bottomTrailing) {
            if open {
                panel.transition(.move(edge: edge).combined(with: .opacity))
            } else {
                tab.transition(.move(edge: edge).combined(with: .opacity))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: hub.boardOnLeft ? .bottomLeading : .bottomTrailing)
        .padding(.bottom, Self.bottomInset)
        .animation(.spring(response: 0.32, dampingFraction: 0.86), value: open)
        .animation(.easeOut(duration: 0.2), value: pointer.inside)
        .onChange(of: pointer.atEdge) { sync() }
    }

    // 숨어 있을 때 창 오른쪽 아래 끝에 붙은 탭 — 여기에 마우스를 대면 터미널 독이 열린다는 표시.
    // 보드에 마우스가 들어오면 밝아지며 끝까지 튀어나온다. 열고 닫기는 창 전체 추적(PointerTracker)이 맡으므로 누르지는 않는다
    private var tab: some View {
        let left = hub.boardOnLeft
        let shape = left ? UnevenRoundedRectangle(bottomTrailingRadius: 8, topTrailingRadius: 8)
                         : UnevenRoundedRectangle(topLeadingRadius: 8, bottomLeadingRadius: 8)
        return VStack(spacing: 6) {
            Image(systemName: left ? "chevron.right" : "chevron.left").font(.system(size: 8, weight: .heavy))
            Image(systemName: "terminal.fill").font(.system(size: 11, weight: .semibold))
        }
        .foregroundStyle(Color.white.opacity(pointer.inside ? 0.95 : 0.6))
        .frame(width: Self.trigger, height: 58)
        .background(shape.fill(Palette.accent.opacity(pointer.inside ? 0.5 : 0.25)))
        .overlay(shape.stroke(Color.white.opacity(0.18), lineWidth: 0.5))
        .offset(x: pointer.inside ? 0 : left ? -4 : 4)
        .allowsHitTesting(false)
    }

    private var panel: some View {
        let left = hub.boardOnLeft
        // hover 는 아이콘마다가 아니라 독 전체에서 받는다 — 아이콘이 커지며 추적 영역이 바뀌면 들어옴·나감이 2ms 마다 번갈아 와 보드가 멈춘다
        return VStack(spacing: Self.gap) {
            ForEach(Array(terminalApps.enumerated()), id: \.element.id) { i, app in
                LauncherButton(app: app, icon: store.icons[app.id], running: store.running.contains(app.id), hovered: hovered == i, left: left)
            }
        }
        .padding(Self.pad)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(Color(red: 0.11, green: 0.11, blue: 0.17).opacity(0.82))
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
        )
        .overlay(RoundedRectangle(cornerRadius: 16).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .shadow(color: .black.opacity(0.45), radius: 14, x: left ? 3 : -3, y: 4)
        .contentShape(Rectangle())
        .onContinuousHover { phase in
            var i: Int?
            if case .active(let p) = phase { i = index(at: p.y) }
            if hovered != i { hovered = i }
            let over = phase != .ended
            if over != overPanel { overPanel = over; sync() }
        }
        .onTapGesture { location in
            if let i = index(at: location.y) { TerminalAppStore.shared.open(terminalApps[i].id) }
        }
        // 힌트는 독 안쪽 옆(보드 쪽)에 오버레이로 띄운다 — 아이콘을 옮겨 다니면 사라졌다 나타나지 않고 그 아이콘 높이로 미끄러진다
        .overlay(alignment: left ? .topLeading : .topTrailing) { hint.allowsHitTesting(false) }
        .animation(.spring(response: 0.26, dampingFraction: 0.86), value: hovered)
        .padding(left ? .leading : .trailing, 8)
    }

    @ViewBuilder private var hint: some View {
        if let i = hovered {
            let app = terminalApps[i]
            let installed = store.urls[app.id] != nil
            let running = store.running.contains(app.id)
            HStack(spacing: 5) {
                Image(systemName: installed ? (running ? "arrow.up.forward.app" : "plus.app") : "nosign")
                    .font(.system(size: 11, weight: .semibold))
                Text(installed ? (running ? "\(app.name) 창으로 이동" : "\(app.name) 실행")
                               : "\(app.name) · 설치되지 않음")
                    .boardFont(12, weight: .semibold)
                    .lineLimit(1)
                    .contentTransition(.opacity)
            }
            .fixedSize()
            .foregroundStyle(Color.white.opacity(installed ? 0.95 : 0.55))
            .padding(.horizontal, 10)
            .frame(height: Self.hintHeight)
            .background(Capsule().fill(Color(red: 0.10, green: 0.10, blue: 0.15).opacity(0.96)))
            .overlay(Capsule().stroke(Color.white.opacity(0.14), lineWidth: 0.5))
            .shadow(color: .black.opacity(0.4), radius: 8, y: 2)
            // 힌트 가운데를 그 아이콘 가운데 높이에, 오른쪽 끝을 독 왼쪽 8pt 앞에 둔다(보드가 왼쪽이면 왼쪽 끝을 독 오른쪽 8pt 뒤에)
            .offset(x: hub.boardOnLeft ? Self.width + 8 : -(Self.width + 8),
                    y: Self.pad + CGFloat(i) * (Self.size + Self.gap) + (Self.size - Self.hintHeight) / 2)
            .transition(.opacity.combined(with: .offset(x: hub.boardOnLeft ? -6 : 6)))
        }
    }
}

// 창 안 마우스 위치 — 투명 모드(마우스가 올라오면 불투명)와 오른쪽 독(오른쪽 끝에 대면 열림)이 함께 본다.
// SwiftUI hover 는 단추·줄마다 따로 잡혀 경계를 지날 때 끊기므로, 창 전체에 AppKit 추적 영역 하나를 둔다
final class PointerTracker: NSResponder, ObservableObject {
    static let shared = PointerTracker()
    @Published private(set) var inside = false
    @Published private(set) var atEdge = false
    private(set) var inWindow = false   // 창 어디든(펼친 창의 터미널 쪽·제목 막대 띠 포함) — 투명도가 쓴다
    private weak var view: NSView?
    private var windowArea: NSTrackingArea?

    private override init() { super.init() }
    required init?(coder: NSCoder) { fatalError() }

    func attach(_ v: NSView) {
        view = v
        v.addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .mouseMoved, .activeAlways, .inVisibleRect],
                                         owner: self))
    }

    // 창 전체 추적 — 보드 쪽 추적과 따로 둔다(독은 보드 쪽에 들어와야 튀어나온다)
    func attachWindow(_ v: NSView) {
        let a = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self)
        windowArea = a
        v.addTrackingArea(a)
    }

    override func mouseEntered(with event: NSEvent) {
        if event.trackingArea === windowArea { return setInWindow(true) }
        mouseMoved(with: event)
    }

    override func mouseExited(with event: NSEvent) {
        if event.trackingArea === windowArea { return setInWindow(false) }
        if atEdge { atEdge = false }
        setInside(false)
        HintPanel.shared.leaveAll()
    }

    override func mouseMoved(with event: NSEvent) {
        guard let v = view else { return }
        setInside(true)
        let p = v.convert(event.locationInWindow, from: nil)   // 왼쪽 아래가 원점
        // 독은 보드의 화면 바깥쪽 끝에 있다 — 보드 위치가 왼쪽이면 왼쪽 끝
        let outer = MainActor.assumeIsolated { TerminalHub.shared.boardOnLeft } ? p.x <= v.bounds.minX + EdgeDock.trigger
                                                                                  : p.x >= v.bounds.maxX - EdgeDock.trigger
        let edge = outer && p.y - v.bounds.minY <= EdgeDock.triggerHeight
        if edge != atEdge { atEdge = edge }
    }

    private func setInside(_ v: Bool) {
        guard v != inside else { return }
        inside = v
    }

    private func setInWindow(_ v: Bool) {
        guard v != inWindow else { return }
        inWindow = v
        WindowOpacity.shared.apply()
    }
}

// 투명 모드 — 카카오톡 PC 처럼 오른쪽 아래 슬라이더로 창 전체(글자 포함)의 투명도를 정한다.
// 마우스가 창 안에 있으면 불투명하게 보이고, 슬라이더를 움직이는 동안은 고른 투명도를 미리 보여 준다.
// 보드가 활성 창이어도 마우스가 창 밖이면 고른 투명도다(사용자 결정)
@MainActor final class WindowOpacity: ObservableObject {
    static let shared = WindowOpacity()
    static let range: ClosedRange<Double> = 0.3...1
    @Published var level = min(max(UserDefaults.standard.object(forKey: "opacity") as? Double ?? 1, WindowOpacity.range.lowerBound),
                               WindowOpacity.range.upperBound) {
        didSet { UserDefaults.standard.set(level, forKey: "opacity"); apply() }
    }
    @Published var previewing = false { didSet { apply() } }

    func apply() {
        guard let w = window else { return }
        // 터미널을 펼쳐 화면 전체를 덮은 동안에는 늘 불투명 — 화면 전체의 터미널이 비쳐 보이게 할 까닭이 없다
        let solid = !previewing && (PointerTracker.shared.inWindow || expandedToFullScreen() || SessionMenuPanel.shared.isOpen)
        let target = solid ? 1 : CGFloat(level)
        guard abs(w.alphaValue - target) > 0.001 else { return }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = previewing ? 0 : 0.22
            w.animator().alphaValue = target
        }
    }
}

// 오른쪽 아래 투명도 슬라이더 — 왼쪽으로 갈수록 투명하다
struct OpacitySlider: View {
    var compact = false     // 미니 화면에서는 막대를 짧게
    @ObservedObject private var opacity = WindowOpacity.shared
    @State private var hover = false
    @State private var settle: DispatchWorkItem?

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "circle.lefthalf.filled").font(.system(size: 10, weight: .semibold))
            Slider(value: $opacity.level, in: WindowOpacity.range) { editing in
                settle?.cancel()
                if editing { opacity.previewing = true; return }
                // 놓은 뒤 잠깐 고른 투명도를 더 보여 주고 불투명으로 돌아간다(마우스가 보드 위에 있으므로)
                let w = DispatchWorkItem { opacity.previewing = false }
                settle = w
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: w)
            }
            .controlSize(.mini)
            .tint(Palette.accent)
            .frame(width: compact ? 48 : 84)
            Text("\(Int((opacity.level * 100).rounded()))%")
                .boardFont(10, weight: .semibold).monospacedDigit()
                .fixedSize().frame(minWidth: 32, alignment: .trailing)   // 보드 글자를 키우면 "100%" 가 32 에 안 들어가 두 줄이 됐다
        }
        .foregroundStyle(.secondary)
        .opacity(hover ? 1 : 0.6)
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
        .hint("투명도: 마우스가 보드 창 밖에 있으면 보드를 쓰는 중이어도 이 투명도로 보입니다. 마우스를 올리면 불투명해집니다. 화면 전체로 펼친 창은 늘 불투명합니다")
    }
}

// 손가락 커서 — 보드는 초당 30번 다시 그려서 NSCursor.set() 이 곧 화살표로 되돌아간다. macOS 15 부터는 SwiftUI 포인터 모양을 쓴다
struct HandPointer: ViewModifier {
    let on: Bool
    func body(content: Content) -> some View {
        if #available(macOS 15.0, *) { content.pointerStyle(on ? .link : nil) } else { content }
    }
}

struct LauncherButton: View {
    let app: TerminalApp
    let icon: NSImage?
    let running: Bool
    let hovered: Bool
    var left = false        // 보드 위치가 왼쪽이면 독도 왼쪽 끝이라 바깥쪽이 왼쪽
    var body: some View {
        let installed = icon != nil
        ZStack {
            if let icon {
                Image(nsImage: icon).resizable().interpolation(.high)
            } else {
                RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.08))
                    .overlay(Image(systemName: "terminal").font(.system(size: 15, weight: .semibold)).foregroundStyle(.secondary))
                    .padding(3)
            }
        }
        .frame(width: EdgeDock.size, height: EdgeDock.size)
        .modifier(HandPointer(on: installed))
        .saturation(installed ? 1 : 0)
        .opacity(installed ? 1 : 0.35)
        .scaleEffect(installed && hovered ? 1.12 : 1)
        .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hovered)
        // 실행 중이면 Dock 처럼 바깥쪽(오른쪽, 보드가 왼쪽이면 왼쪽)에 점
        .overlay(alignment: left ? .leading : .trailing) {
            Circle().fill(Color.white.opacity(running ? 0.8 : 0)).frame(width: 4, height: 4)
                .offset(x: left ? -(EdgeDock.pad / 2 + 2) : EdgeDock.pad / 2 + 2)
        }
    }
}

struct ScrollRequest: Equatable { let id: String; let n: Int }

// 목록 카드마다 스크롤 좌표의 윗변 위치
struct CardTops: PreferenceKey {
    static var defaultValue: [String: CGFloat] = [:]
    static func reduce(value: inout [String: CGFloat], nextValue: () -> [String: CGFloat]) {
        value.merge(nextValue()) { $1 }
    }
}

struct BoardView: View {
    @ObservedObject var store: Store
    @ObservedObject var usage: UsageStore
    @ObservedObject var accounts: AccountStore
    @AppStorage("pinned") private var pinned = true
    @AppStorage("usageCompact") private var usageCompact = false
    @State private var foldHover = false
    @State private var checkHover = false
    @State private var pinHover = false
    // 요약 칩: 걸러 보는 상태·스크롤 요청
    @State private var filter: String?
    @State private var scrollRequest = ScrollRequest(id: "", n: 0)
    // 목록 아래로 가려진 카드 — 카드 위치는 스크롤 좌표로 모은다
    // 스크롤·애니메이션 프레임마다 위치가 바뀌어도 화면은 "가려진 카드 목록" 이 달라질 때만 다시 그린다
    @State private var tops = CardTopsBox()
    @State private var hiddenIds: [String] = []
    @State private var moreHover = false
    // 창 크기 단계 — 중간에서는 카드를 접힌 한 줄로 보이고, 그 단계에서 눌러 펼친 카드만 따로 기억한다(저장한 접힘 상태는 그대로)
    @ObservedObject private var fit = BoardFit.shared
    @State private var mediumOpen: Set<String> = []
    @ObservedObject private var updater = Updater.shared
    // 자율주행 — 실행 줄과 자율주행 중·지난 실행 화면(보드 쪽 전체를 바꿈). 보기: 모두·기본(실행 줄 숨김)·자율주행(실행 줄만)
    @ObservedObject private var autopilot = AutopilotStore.shared
    @AppStorage("autopilotView") private var autopilotView = "all"

    private func updateHidden() {
        guard tops.viewport > 0 else { return }
        let ids = shownGroups.map(\.id).filter { (tops.values[$0] ?? 0) > tops.viewport - 24 }
        if ids != hiddenIds { hiddenIds = ids }
    }

    // 목록 칸·보드 높이는 한 번의 배치에서 함께 바뀌므로 둘 다 갱신된 뒤에 단계 판정에 넘긴다.
    // 그때 그려진 단계(tier)를 함께 넘겨, 단계가 바뀌는 사이에 앞 단계의 높이가 새 단계 값으로 잡히지 않게 한다
    private func measure(_ tier: BoardTier) {
        DispatchQueue.main.async { BoardFit.shared.measured(tier, board: tops.board, list: tops.viewport) }
    }

    // 목록 칸의 높이 — 가려진 카드 계산과 단계 판정이 함께 쓴다
    private var listHeightReader: some View {
        let tier = fit.tier
        return GeometryReader { geo in
            Color.clear.onAppear { tops.viewport = geo.size.height; updateHidden(); measure(tier) }
                .onChange(of: geo.size.height) { _, h in tops.viewport = h; updateHidden(); measure(tier) }
        }
    }

    // 칩 한 번 = 그 상태만 보기(해당 카드를 펼치고 맨 위로), 같은 칩 다시 = 모두 보기
    private func chipTapped(_ st: String) {
        if filter == st { withAnimation(.easeOut(duration: 0.18)) { filter = nil }; return }
        let ids = store.groups.filter { $0.sessions.contains { $0.state == st } }.map(\.id)
        withAnimation(.easeOut(duration: 0.18)) {
            filter = st
            ids.forEach(store.reveal)
        }
        if let first = ids.first { scrollRequest = ScrollRequest(id: first, n: scrollRequest.n + 1) }
    }

    // 걸러 보는 중이면 그 상태 세션만 남긴 카드
    private var shownGroups: [Group] {
        guard let f = filter else { return store.groups }
        return store.groups.compactMap { g in
            let ss = g.sessions.filter { $0.state == f }
            return ss.isEmpty ? nil : Group(id: g.id, sessions: ss)
        }
    }

    // 화면 아래로 가려진 카드 수와 그중 승인 대기가 있는지
    private var below: (n: Int, waiting: Bool, first: String?) {
        let ids = Set(hiddenIds)
        let hidden = shownGroups.filter { ids.contains($0.id) }
        return (hidden.count, hidden.contains { $0.sessions.contains { $0.state == "waiting" } }, hidden.first?.id)
    }

    private var titleBlock: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Image(nsImage: appIcon).resizable().interpolation(.high).frame(width: 24, height: 24)
                Text("Claude Board").boardFont(19, weight: .bold).lineLimit(1)
                if !updater.current.isEmpty {
                    // build-board.sh 로 만든 개발용 빌드는 버전이 1.0 이다
                    Text(updater.current == "1.0" ? "dev" : updater.current)
                        .boardFont(12, weight: .semibold, design: .monospaced)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6).padding(.vertical, 2)
                        .background(Capsule().fill(Color.white.opacity(0.08)))
                        .lineLimit(1).fixedSize()
                        .hint("Claude Board 버전")
                }
            }
            let active = store.groups.reduce(0) { $0 + $1.sessions.filter { $0.state != "ended" }.count }
            // Claude Code 버전은 부가 정보라 세션 수 옆에 흐리게 붙인다
            HStack(spacing: 0) {
                Text("활성 세션 \(active)개").contentTransition(.numericText())
                if let v = claudeVersion() {
                    Text(" · Claude Code \(v)").foregroundStyle(.tertiary)
                        .hint("설치된 Claude Code 버전")
                }
            }
            .boardFont(12).foregroundStyle(.secondary).lineLimit(1)
        }
    }

    private var headerControls: some View {
        HStack {
            HStack(spacing: 6) {
                ForEach(["waiting", "working", "idle"], id: \.self) { st in
                    let n = store.count(st)
                    if n > 0 {
                        SummaryChip(state: st, n: n, filtering: filter == st) { chipTapped(st) }
                            .transition(.scale.combined(with: .opacity))
                    }
                }
            }
            Spacer(minLength: 6)
            AllMuteButton(muted: store.allMuted) { store.setAllMuted(!store.allMuted) }
            // 업데이트 확인 — 새 버전이 있으면 아래 알림 줄에 업데이트 단추가, 없으면 "최신 버전입니다" 가 잠깐 뜬다
            Button { updater.check(manual: true) } label: {
                Image(systemName: "arrow.triangle.2.circlepath")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(checkHover ? .primary : .secondary)
                    .opacity(updater.checking ? 0.4 : 1)
                    .padding(.horizontal, 8)
                    .frame(minWidth: 28, minHeight: 24)
                    .background(Capsule().fill(Color.white.opacity(checkHover ? 0.12 : 0.07)))
            }
            .buttonStyle(.plain)
            .clickable($checkHover)
            .disabled(updater.checking)
            .hint("업데이트 확인: GitHub 에 새 버전이 있는지 확인합니다")
            // 모두 접기 = 경로 카드를 다 접고 사용 한도를 간략(한 줄)으로, 모두 펼치기 = 다 펴고 상세로.
            // 중간 화면은 이미 카드가 접힌 한 줄·사용 한도가 한 줄이라 숨긴다
            let all = usageCompact && store.groups.allSatisfy { store.collapsed.contains($0.id) }
            if fit.tier == .full {
            Button {
                withAnimation(foldSpring) {
                    store.setAll(collapse: !all)
                    usageCompact = !all
                }
            } label: {
                // 마우스를 올려도 폭이 변하지 않게 글자는 도움말로만 보인다 — 폭이 바뀌면 "위에 고정" 이 줄었다 늘었다 한다
                Image(systemName: all ? "rectangle.expand.vertical" : "rectangle.compress.vertical")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(foldHover ? .primary : .secondary)
                    .padding(.horizontal, 8)
                    .frame(minWidth: 28, minHeight: 24)
                    .background(Capsule().fill(Color.white.opacity(foldHover ? 0.12 : 0.07)))
            }
            .buttonStyle(.plain)
            .clickable($foldHover)
            .hint(all ? "모두 펼치기: 경로 카드를 펴고 사용 한도를 자세히" : "모두 접기: 경로 카드를 접고 사용 한도를 한 줄로")
            }
            Button {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.6)) { setPinned(!pinned) }
            } label: {
                // 윗줄이 좁으면 글자를 빼고 압정 아이콘만 보인다
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 4) {
                        Image(systemName: pinned ? "pin.fill" : "pin.slash")
                            .rotationEffect(.degrees(pinned ? 0 : 45))
                        Text(pinned ? "위에 고정" : "고정 안 함").fixedSize()
                    }
                    Image(systemName: pinned ? "pin.fill" : "pin.slash")
                        .rotationEffect(.degrees(pinned ? 0 : 45))
                }
                .boardFont(11, weight: .semibold)
                .foregroundStyle(pinned ? Palette.accent : .secondary)
                .padding(.horizontal, 9)
                .frame(height: 24)
                .background(Capsule().fill(pinned ? Palette.accent.opacity(0.16) : Color.white.opacity(0.07)))
                .overlay(Capsule().stroke(pinned ? Palette.accent.opacity(0.4) : .clear, lineWidth: 0.5))
            }
            .buttonStyle(.plain)
            .clickable($pinHover)
            .hint(pinned ? "항상 위에 표시 끄기" : "항상 위에 표시")
        }
    }

    // 창 크기 단계에 따라 미니 화면 또는 전체·중간 화면
    var body: some View {
        ZStack(alignment: .bottom) {
            switch autopilot.screen {
            case .running(let id):
                APRunningScreen(ap: autopilot, id: id)
            case .history(let selected):
                APHistoryScreen(ap: autopilot, selected: selected)
            case .board:
                if fit.tier == .mini {
                    MiniBoard(store: store, usage: usage)
                        .background(Backdrop())
                        .preferredColorScheme(.dark)
                } else {
                    board
                }
            }
            TerminalNotice(hub: TerminalHub.shared)
        }
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }

    private var board: some View {
        let tier = fit.tier
        let medium = tier == .medium
        let runs = autopilot.runs
        let autoOnly = autopilotView == "auto" && !runs.isEmpty   // 도는 실행이 없으면 모두 보기로 그린다
        // 경로 카드가 없는 실행 — 자율주행 세션은 세션 파일을 남기지 않아 사람 세션이 없던 경로에는 카드가 없다
        let orphans = autopilotView == "basic" || filter != nil ? [] : runs.filter { r in !store.groups.contains { r.covers($0.id) } }
        return VStack(alignment: .leading, spacing: 14) {
            // 윗줄은 늘 두 줄 — 제목 아래 줄에 상태 칩(왼쪽)과 단추(오른쪽).
            // 한 줄·두 줄을 폭에 따라 바꾸면 "알림 꺼짐" 글자가 생길 때마다 단추 줄이 위아래로 오간다
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .center, spacing: 8) {
                    titleBlock.layoutPriority(1)
                    Spacer(minLength: 0)
                    Foreman(groups: store.groups, limits: usage.limits, now: store.now)
                }
                headerControls
                APBar(ap: autopilot, view: $autopilotView)
            }
            .padding(.top, 6)

            if updater.latest != nil || updater.notice != nil {
                UpdateBanner(updater: updater).transition(.move(edge: .top).combined(with: .opacity))
            }

            UsagePanel(usage: usage, accounts: accounts, now: store.now, forceCompact: medium,
                       stoplines: autoOnly ? autopilot.stoplines : nil) {
                store.groups.flatMap(\.sessions).filter { $0.state == "working" || $0.state == "waiting" }
            }

            if store.groups.isEmpty && runs.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "moon.zzz.fill").font(.system(size: 30)).foregroundStyle(.secondary)
                    Text("아직 기록된 세션이 없습니다").boardFont(14, weight: .medium)
                    Text("Claude Code 세션에서 요청을 보내면 여기에 나타납니다")
                        .boardFont(12).foregroundStyle(.secondary)
                }
                // 창이 낮으면 안내가 줄어들며 잘린다 — 줄지 않으면 보드가 넘쳐 아래 투명도 슬라이더가 밀려난다
                .frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
                .clipped()
                .background(listHeightReader)
                .transition(.opacity)
            } else {
                if let f = filter {
                    HStack(spacing: 6) {
                        Circle().fill(Palette.color(f)).frame(width: 7, height: 7)
                        Text("\(Palette.name(f))만 보는 중").boardFont(12, weight: .semibold)
                        Spacer()
                        Button("모두 보기") { withAnimation(foldSpring) { filter = nil } }
                            .buttonStyle(.plain).boardFont(12, weight: .semibold).foregroundStyle(Palette.accent)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 6)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Palette.color(f).opacity(0.12)))
                    .transition(.move(edge: .top).combined(with: .opacity))
                }
                ScrollViewReader { proxy in
                ScrollView(showsIndicators: true) {
                    VStack(spacing: 12) {
                        // 자율주행 보기는 실행 카드만, 다른 보기는 카드 없는 실행을 맨 위에
                        ForEach(autoOnly ? runs : orphans) { r in
                            APRunCard(run: r, minutes: r.unitMinutes(store.now), meters: !autoOnly).equatable().transition(.opacity)
                        }
                        ForEach(autoOnly ? [] : shownGroups) { g in
                            GroupCard(g: g, now: store.now,
                                      collapsed: medium ? !mediumOpen.contains(g.id) : store.collapsed.contains(g.id),
                                      toggle: { if medium { mediumOpen.formSymmetricDifference([g.id]) } else { store.toggle(g.id) } },
                                      endedOpen: store.endedOpen.contains(g.id), toggleEnded: { store.toggleEnded(g.id) },
                                      pinned: store.pinnedGroups.contains(g.id), togglePin: { store.togglePinGroup(g.id) },
                                      remove: { withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { store.hideGroup(g) } },
                                      clearEnded: { withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { store.removeEnded(g) } })
                                .transition(.asymmetric(insertion: .move(edge: .top).combined(with: .opacity),
                                                        removal: .opacity))
                                .id(g.id)
                                .background(GeometryReader { geo in
                                    Color.clear.preference(key: CardTops.self,
                                                           value: [g.id: geo.frame(in: .named("boardScroll")).minY])
                                })
                        }
                    }
                    .padding(.bottom, 12)
                }
                .coordinateSpace(name: "boardScroll")
                .background(listHeightReader)
                .onPreferenceChange(CardTops.self) { tops.values = $0; updateHidden() }
                .onChange(of: scrollRequest) { _, r in
                    proxy.scrollTo(r.id, anchor: .top)   // 걸러 보기 전환과 겹치지 않게 애니메이션 없이
                }
                .overlay(alignment: .bottom) {
                    let b = below
                    if b.n > 0 {
                        ZStack(alignment: .bottom) {
                            // 아래 끝을 흐리게 — 목록이 더 있다는 표시
                            LinearGradient(colors: [.clear, Color(red: 0.07, green: 0.07, blue: 0.11).opacity(0.9)],
                                           startPoint: .top, endPoint: .bottom)
                                .frame(height: 56)
                                .allowsHitTesting(false)
                            let tint = b.waiting ? Palette.color("waiting") : Color.white
                            HStack(spacing: 5) {
                                Image(systemName: "chevron.down").font(.system(size: 10, weight: .bold))
                                Text(b.waiting ? "아래에 경로 \(b.n)개 더 · 승인 대기 있음" : "아래에 경로 \(b.n)개 더")
                                    .boardFont(11, weight: .semibold)
                            }
                            .foregroundStyle(b.waiting ? Color.black.opacity(0.85) : Color.white.opacity(0.9))
                            .padding(.horizontal, 10).padding(.vertical, 5)
                            .background(Capsule().fill(b.waiting ? tint.opacity(moreHover ? 1 : 0.9) : tint.opacity(moreHover ? 0.22 : 0.14)))
                            .overlay(Capsule().stroke(tint.opacity(0.35), lineWidth: 0.5))
                            .contentShape(Capsule())
                            .onTapGesture {
                                if let id = b.first {
                                    withAnimation(.spring(response: 0.5, dampingFraction: 0.86)) { proxy.scrollTo(id, anchor: .top) }
                                }
                            }
                            .clickable($moreHover)
                            .hint("눌러서 아래 카드로 이동")
                            .padding(.bottom, 8)
                        }
                        .transition(.opacity)
                    }
                }
                }
                .environment(\.notifyLocked, store.allMuted)
            }

            // 카카오톡 PC 처럼 창 오른쪽 아래에 투명도 슬라이더
            HStack {
                Spacer()
                OpacitySlider()
            }
            .padding(.top, -8)
            .padding(.bottom, 6)
        }
        .padding(.horizontal, 16)
        .padding(.top, 22)
        // 최소 높이는 주지 않는다 — 주면 제목 막대 높이만큼 넘쳐 아래 투명도 슬라이더가 잘린다. 낮으면 단계가 바뀐다
        .frame(minWidth: boardMinWidth)
        .background(GeometryReader { geo in
            Color.clear.onAppear { tops.board = geo.size.height; measure(tier) }
                .onChange(of: geo.size.height) { _, h in tops.board = h; measure(tier) }
        })
        .background(Backdrop())
        // 터미널 열기 독 — 보드 위에 겹쳐 뜨므로 아래 배치는 그대로다. 붙는 쪽(보드 위치)은 독이 스스로 정한다
        .overlay { EdgeDock() }
        .onChange(of: store.groups) { _, _ in
            if let f = filter, store.count(f) == 0 { withAnimation(foldSpring) { filter = nil } }
        }
        .preferredColorScheme(.dark)
        .animation(.spring(response: 0.45, dampingFraction: 0.82), value: store.groups)
    }
}

// MARK: - 업데이트

// GitHub 최신 릴리스의 VERSION 과 이 앱 버전을 비교해 새 버전이 있으면 보드 위에 알린다.
// 업데이트 단추는 설치와 같은 install.sh 를 curl 로 받아 실행한다 — 스크립트가 앱을 바꿔 끼우고 보드를 다시 띄운다
let releaseRepo = "suhojang/claude-board"   // packaging/install.sh·release.sh 의 REPO 와 같게
let releaseBase = "https://github.com/\(releaseRepo)/releases/latest/download"
let changelogURL = URL(string: "https://github.com/\(releaseRepo)/blob/main/CHANGELOG.md")!

final class Updater: ObservableObject {
    static let shared = Updater()
    @Published var latest: String?   // 이 앱보다 새 버전이 나왔으면 그 버전
    @Published var running = false
    @Published var failed = false
    @Published var checking = false   // 손으로 누른 확인이 도는 중
    @Published var notice: (icon: String, text: String)?   // 손으로 확인한 결과 — 새 버전이 없을 때 알림 줄에 잠깐 뜬다
    let current = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""   // 제목 옆 버전 표시도 쓴다
    private let logPath = NSHomeDirectory() + "/.claude/notifier/update.log"
    private var timer: Timer?
    private var noticeHide: DispatchWorkItem?
    // build-board.sh 로 만든 개발용 빌드(1.0)는 배포판으로 덮어쓰지 않게 확인하지 않는다
    private var eligible: Bool { !current.isEmpty && current != "1.0" }

    private init() {
        guard eligible else { return }
        check()
        timer = Timer.scheduledTimer(withTimeInterval: 3600, repeats: true) { [weak self] _ in self?.check() }
    }

    // manual — 업데이트 확인 단추·메뉴에서 불렀을 때. 새 버전이 없거나 확인하지 못해도 결과를 알림 줄에 보인다
    func check(manual: Bool = false) {
        if manual {
            guard !checking else { return }
            guard eligible else { show("hammer.fill", "개발용 빌드(\(current))는 업데이트를 확인하지 않습니다"); return }
            checking = true
        }
        let req = URLRequest(url: URL(string: releaseBase + "/VERSION")!, cachePolicy: .reloadIgnoringLocalCacheData,
                             timeoutInterval: 15)
        URLSession.shared.dataTask(with: req) { [weak self] data, resp, _ in
            guard let self else { return }
            var v: String?
            if (resp as? HTTPURLResponse)?.statusCode == 200, let data,
               let s = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
               s.range(of: "^[0-9]+(\\.[0-9]+)*$", options: .regularExpression) != nil { v = s }
            DispatchQueue.main.async {
                if let v {
                    let newer = v.compare(self.current, options: .numeric) == .orderedDescending
                    withAnimation(.easeOut(duration: 0.2)) { self.latest = newer ? v : nil }
                }
                guard manual else { return }
                self.checking = false
                if v == nil { self.show("exclamationmark.triangle.fill", "업데이트를 확인하지 못했습니다") }
                else if self.latest == nil { self.show("checkmark.circle.fill", "최신 버전입니다 (\(self.current))") }
            }
        }.resume()
    }

    private func show(_ icon: String, _ text: String) {
        withAnimation(.easeOut(duration: 0.2)) { notice = (icon, text) }
        noticeHide?.cancel()
        let hide = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.2)) { self?.notice = nil } }
        noticeHide = hide
        DispatchQueue.main.asyncAfter(deadline: .now() + 4, execute: hide)
    }

    func run() {
        guard !running else { return }
        // 보드 탭 세션은 보드와 함께 꺼진다 — 일하는 중인 것이 있으면 먼저 묻는다. 스크립트가 보드를 pkill 로 끄므로(종료 확인을 거치지 않음)
        // 목록·일시중지 표지는 여기서 남기고, 보드를 끄기 전에 스크립트가 멈추면(아래로 돌아오면) 남긴 표지를 지운다 — 세션은 그대로 도니까
        guard let marks = MainActor.assumeIsolated({ confirmShutdown(update: true) }) else { return }
        running = true
        failed = false
        FileManager.default.createFile(atPath: logPath, contents: nil)
        let out = FileHandle(forWritingAtPath: logPath) ?? .nullDevice
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/bash")
        // pipefail — 받기에 실패하면 빈 스크립트가 성공으로 끝나지 않게 한다.
        // 출력은 파일로 받는다 — 스크립트가 도중에 이 앱을 끄므로 파이프로 받으면 쓰다가 끊겨 스크립트도 멈춘다
        p.arguments = ["-c", "set -o pipefail; /usr/bin/curl -fsSL '\(releaseBase)/install.sh' | /bin/bash"]
        p.standardInput = FileHandle.nullDevice
        p.standardOutput = out
        p.standardError = out
        // 성공하면 스크립트가 이 앱을 끄고 새 앱을 띄우므로 여기로 돌아오지 않는다 — 돌아오면 끄기 전에 멈춘 것이다
        p.terminationHandler = { [weak self] p in
            DispatchQueue.main.async {
                for m in marks { try? FileManager.default.removeItem(atPath: m) }
                self?.running = false
                self?.failed = p.terminationStatus != 0
            }
        }
        do { try p.run() } catch {
            for m in marks { try? FileManager.default.removeItem(atPath: m) }
            running = false
            failed = true
        }
    }
}

// 새 버전 알림 줄 — 전체·중간 화면의 윗줄 아래에 뜬다
struct UpdateBanner: View {
    @ObservedObject var updater: Updater
    @State private var hover = false
    @State private var notesHover = false

    var body: some View {
        let tint = updater.failed ? Palette.color("waiting") : Palette.accent
        // 새 버전이 없으면 손으로 확인한 결과(최신·확인 실패)만 단추 없이 보인다
        let notice = updater.latest == nil ? updater.notice : nil
        HStack(spacing: 6) {
            Image(systemName: notice?.icon ?? (updater.failed ? "exclamationmark.triangle.fill" : "arrow.down.circle.fill"))
                .font(.system(size: 12)).foregroundStyle(tint)
            Text(notice?.text ?? (updater.failed ? "업데이트하지 못했습니다 · ~/.claude/notifier/update.log"
                 : updater.running ? "업데이트 중… 끝나면 보드가 다시 열립니다"
                 : "새 버전 \(updater.latest ?? "") 이 나왔습니다"))
                .boardFont(12, weight: .semibold).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 6)
            if updater.running {
                ProgressView().controlSize(.small)
            } else if notice == nil {
                // 무엇이 바뀌었는지 — 브라우저에서 GitHub 의 CHANGELOG.md 를 연다
                Button("변경 내용") { NSWorkspace.shared.open(changelogURL) }
                    .buttonStyle(.plain).boardFont(12, weight: .semibold)
                    .foregroundStyle(notesHover ? .primary : .secondary)
                    .clickable($notesHover)
                    .hint("GitHub 에서 CHANGELOG.md 를 엽니다")
                Rectangle().fill(Color.white.opacity(0.14)).frame(width: 0.5, height: 10).padding(.horizontal, 2)
                Button(updater.failed ? "다시 시도" : "업데이트") { updater.run() }
                    .buttonStyle(.plain).boardFont(12, weight: .semibold)
                    .foregroundStyle(tint.opacity(hover ? 1 : 0.85))
                    .clickable($hover)
                    .hint("새 버전을 받아 바꿔 끼웁니다. 끝나면 보드가 다시 열립니다")
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 8).fill(tint.opacity(0.12)))
    }
}

// MARK: - 내장 터미널

// 보드가 직접 쥐는 터미널(pty). 펼치면 창이 화면의 쓸 수 있는 영역 전체로 넓어져 [터미널 | 경계 | 보드], 접으면 지금처럼 보드만 남는다.
// 터미널 뷰는 창에서 빼지 않는다 — 창에 붙은 터미널이 하나도 없으면 엔진이 메일함 처리를 멈춰, 제목 변경이 쌓이면 pty 읽기가 막히고
// 세션 출력이 멈춘다(실측 19초). 숨길 때는 실제 크기 그대로 isHidden + setSurfaceVisible(false)(isHidden 만 하면 계속 그린다).
// 탭을 닫을 때는 controller 를 떼어 엔진 surface 를 내린다(그래야 셸이 끝난다)

let paneDividerWidth: CGFloat = 8       // 터미널과 보드 사이 경계 — 끌어서 두 영역 폭을 바꾼다
let terminalMinWidth: CGFloat = 480     // 펼쳤을 때 터미널 영역의 최소 폭
let terminalTabBarHeight: CGFloat = 40
let terminalWorkspaceBarHeight: CGFloat = 32   // 탭 줄 위 경로 줄 — 탭이 있을 때만
let terminalAreaColor = NSColor(red: 0.071, green: 0.075, blue: 0.094, alpha: 1)   // 탭이 없을 때 터미널 영역 바탕
let terminalBarColor = NSColor(red: 0.10, green: 0.10, blue: 0.13, alpha: 1)        // 탭 줄과 그 위 제목 막대 띠

// 작업 경로를 견주는 꼴 — /tmp ↔ /private/tmp 같은 링크, 끝 / 차이로 같은 폴더가 어긋나지 않게(없는 경로는 글자만 다듬는다).
// 보드 카드는 1초마다 다시 그려지므로 경로마다 한 번만 푼다
var workspaceKeys: [String: String] = [:]
func workspaceKey(_ path: String) -> String {
    if let k = workspaceKeys[path] { return k }
    let k = (path as NSString).resolvingSymlinksInPath
    workspaceKeys[path] = k
    return k
}

// 경로 줄 이름 — 폴더 이름(홈은 ~). 같은 이름이 둘 이상이면 부모 폴더를 붙여 가른다
func workspaceLabel(_ ws: String, among list: [String]) -> String {
    func name(_ p: String) -> String { p == NSHomeDirectory() ? "~" : (p as NSString).lastPathComponent }
    let n = name(ws)
    guard ws != NSHomeDirectory(), list.filter({ name($0) == n }).count > 1 else { return n }
    return ((ws as NSString).deletingLastPathComponent as NSString).lastPathComponent + "/" + n
}

// 한글 조합 입력 고침 — 입력기는 조합 중 Return·Esc 에 "글자 확정" 과 "줄바꿈·취소 명령" 을 함께 보내는데 래퍼는 확정 글자가 있으면
// 명령을 버려 Return 을 두 번 눌러야 한다(Ghostty.app 1.3.1 도 같다). 입력기가 실제로 그 명령을 보냈을 때만 원래 키를 수정키 그대로
// 다시 보낸다 — 일본어 변환 확정처럼 명령이 없으면 그대로 둔다. 엔진 버전을 올리면 래퍼가 고쳤는지 다시 시험한다(고쳤으면 Enter 가 두 번 간다)
final class BoardTerminalView: AppTerminalView {
    private var composingAtKeyDown = false
    private var insertedDuringKey = false
    private var commandDuringKey: Selector?

    // 창은 배경을 끌어 옮길 수 있게 되어 있고 엔진 뷰는 불투명이 아니라 기본값이 true — 끌어서 글자를 고르면 창이 따라 움직였다(사용자 보고)
    override var mouseDownCanMoveWindow: Bool { false }

    // 키보드 초점을 받았다(화면을 누르면 래퍼의 mouseDown 이 첫 응답자로 세운다, ⌘[ 등으로 옮겨도) — 그 탭의 고른 화면으로 삼는다
    var onFocus: (() -> Void)?
    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFocus?() }
        return ok
    }

    private func mods(_ event: NSEvent) -> TerminalInputModifiers {
        var m: TerminalInputModifiers = []
        let f = event.modifierFlags
        if f.contains(.shift) { m.insert(.shift) }
        if f.contains(.control) { m.insert(.ctrl) }
        if f.contains(.option) { m.insert(.alt) }
        if f.contains(.command) { m.insert(.super_) }
        return m
    }

    // ⌘ 단축키: 한글·러시아어 등 ASCII 가 아닌 입력기에서는 키 이벤트의 글자가 'ㅈ' 처럼 나와 엔진이 단축키(super+w 등, 글자로
    // 맞춘다)인지 가리지 못할 수 있다 — 같은 키의 ASCII 글자로 다시 만든 이벤트로 묻는다. 엔진에 실제로 보내는 것은 아래 keyDown
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        super.performKeyEquivalent(with: asciiCommandEvent(event) ?? event)
    }

    override func keyDown(with event: NSEvent) {
        // ⌘ 단축키(위 performKeyEquivalent 가 다시 만든 이벤트도): 래퍼는 엔진에 넘기는 키의 글자를 지금 입력기로 다시 읽고('ㅈ')
        // 한글 입력기는 ⌘ 키에 글자를 내지 않아 엔진이 super+ㅈ 로 받는다(2026-10-19 실측) → ⌘W 가 탭을 닫지 않고 claude 에는
        // 'ㅈ' 로 보인다(사용자 보고). ASCII 자판의 같은 키로 보낸다 — ABC 에서와 같은 키 이벤트(kitty 로 ⌘J 가 같은 바이트, 실측)
        if event.modifierFlags.contains(.command),
           event.characters(byApplyingModifiers: [])?.unicodeScalars.contains(where: { !$0.isASCII }) == true,
           let ch = asciiKeyCharacters(event.keyCode, shift: event.modifierFlags.contains(.shift))?.first,
           let press = TerminalKeyPress(typing: ch, modifiers: mods(event)) {
            sendKey(press)
            return
        }
        // 수정키가 붙은 Return(Shift+Enter 등): 래퍼는 확정 글자를 그 키 이벤트에 실어 보내 키 지정이 글자를 덮는다 →
        // sendKey 로 조합 글자를 먼저(수정키 없는 이벤트로) 확정하고 키를 따로 보낸다
        if hasMarkedText(), event.keyCode == 36 || event.keyCode == 76, !mods(event).isEmpty {
            sendKey(.enter, modifiers: mods(event))
            return
        }
        composingAtKeyDown = hasMarkedText()
        insertedDuringKey = false
        commandDuringKey = nil
        super.keyDown(with: event)
        defer { composingAtKeyDown = false; commandDuringKey = nil }
        guard composingAtKeyDown, insertedDuringKey, let sel = commandDuringKey else { return }
        let key: TerminalKey
        switch sel {
        case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.insertLineBreak(_:)),
             #selector(NSResponder.insertNewlineIgnoringFieldEditor(_:)): key = .enter
        case #selector(NSResponder.cancelOperation(_:)): key = .escape
        default: return
        }
        sendKey(key, modifiers: mods(event))
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        if composingAtKeyDown { insertedDuringKey = true }
        super.insertText(string, replacementRange: replacementRange)
    }

    override func doCommand(by selector: Selector) {
        if composingAtKeyDown { commandDuringKey = selector }
        super.doCommand(by: selector)
    }
}

// ⌘ 가 눌린 키 이벤트의 글자가 ASCII 가 아니면(한글·러시아어 입력기 등) 같은 키 번호의 ASCII 글자로 다시 만든 이벤트 — 글자는 지금의
// ASCII 자판(ABC 등)에서 읽는다. 엔진의 메아리 중복 제거가 timestamp 를 견주므로 timestamp 는 그대로 둔다. 바꿀 것이 없으면 nil
func asciiCommandEvent(_ event: NSEvent) -> NSEvent? {
    guard event.type == .keyDown, event.modifierFlags.contains(.command),
          let ign = event.charactersIgnoringModifiers, ign.unicodeScalars.contains(where: { !$0.isASCII }),
          let ascii = asciiKeyCharacters(event.keyCode, shift: event.modifierFlags.contains(.shift)) else { return nil }
    return NSEvent.keyEvent(with: .keyDown, location: event.locationInWindow, modifierFlags: event.modifierFlags,
                            timestamp: event.timestamp, windowNumber: event.windowNumber, context: nil,
                            characters: ascii, charactersIgnoringModifiers: ascii, isARepeat: event.isARepeat, keyCode: event.keyCode)
}

// 키 번호가 ASCII 자판에서 치는 글자(Shift 만 반영). 글자가 없거나 ASCII 가 아니면 nil
func asciiKeyCharacters(_ keyCode: UInt16, shift: Bool) -> String? {
    guard let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue(),
          let raw = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return nil }
    let data = Unmanaged<CFData>.fromOpaque(raw).takeUnretainedValue()
    guard let bytes = CFDataGetBytePtr(data) else { return nil }
    var dead: UInt32 = 0
    var chars = [UniChar](repeating: 0, count: 4)
    var length = 0
    let status = bytes.withMemoryRebound(to: UCKeyboardLayout.self, capacity: 1) {
        UCKeyTranslate($0, keyCode, UInt16(kUCKeyActionDown), shift ? UInt32(shiftKey >> 8) : 0, UInt32(LMGetKbdType()),
                       OptionBits(kUCKeyTranslateNoDeadKeysBit), &dead, chars.count, &length, &chars)
    }
    guard status == noErr, length > 0 else { return nil }
    let s = String(utf16CodeUnits: chars, count: length)
    return s.unicodeScalars.allSatisfy({ $0.isASCII && $0.value >= 0x20 && $0.value != 0x7F }) ? s : nil
}

// 터미널 설정은 보드가 직접 만든다 — 사용자 Ghostty 설정을 바탕으로 하되 theme 줄은 뺀다(엔진 번들에 테마 폴더가 없어 theme 한 줄 때문에
// 파일 전체가 조용히 거부된다). 같은 이름 테마는 엔진의 테마 목록에서 찾아 넘긴다(넘기지 않으면 래퍼 기본 테마가 사용자 색을 덮는다).
// 보드에서 고른 테마(보드 설정 terminalTheme, 윈도우 > 터미널 테마…)가 있으면 Ghostty 설정의 theme 대신 그것
@MainActor func makeTerminalController() -> TerminalController {
    var lines: [String] = []
    var theme: String?
    for f in ghosttyConfigFiles {
        guard let text = try? String(contentsOfFile: f, encoding: .utf8) else { continue }
        for line in text.components(separatedBy: .newlines) {
            let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if parts.count == 2, parts[0] == "theme" {
                theme = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "\""))
                continue
            }
            // Ghostty.app 의 전역 단축키(빠른 터미널 등)는 보드 터미널과 상관없다
            if parts.count == 2, parts[0] == "keybind", parts[1].hasPrefix("global:") { continue }
            lines.append(line)
        }
    }
    lines.append("keybind = shift+enter=text:\\x1b\\r")   // Shift+Enter = 사용자 Ghostty 와 같은 바이트(1b 0d) — 기본값은 CSI 27;2;13~
    lines.append("keybind = super+q=unbind")              // ⌘Q 는 보드 종료로 — 엔진이 받으면 처리할 곳이 없어 삼킨다
    lines.append("keybind = super+t=unbind")              // ⌘T 는 보드의 새 세션(윈도우 메뉴)으로 — 엔진의 new_tab 도 처리할 곳이 없어 삼킨다
    // ⌘1~⌘9 는 보드의 탭 고르기(윈도우 메뉴)로 — 엔진 기본값은 같은 키를 글자(super+1)와 물리 키(super+digit_1) 두 가지로 goto_tab·last_tab 에 묶는다
    for n in 1...9 { lines.append("keybind = super+\(n)=unbind"); lines.append("keybind = super+digit_\(n)=unbind") }
    // 화면 나누기 키(Ghostty 와 같은 키)와 Ghostty 의 탭 옮기기 키는 보드의 윈도우 메뉴(화면 나누기·탭으로 가기)로 — 엔진 기본값은 new_split·
    // goto_split·resize_split·equalize_splits·toggle_split_zoom·previous_tab·next_tab 에 묶는데 래퍼에 받는 곳이 없다(보드가 화면 트리를 직접 쥔다).
    // ⌘W(close_surface)는 엔진이 받아 그 화면의 닫기를 청한다(terminalDidClose)
    for k in ["arrow_left", "arrow_right", "arrow_up", "arrow_down"] {
        lines.append("keybind = super+alt+\(k)=unbind")
        lines.append("keybind = super+ctrl+\(k)=unbind")
    }
    for k in ["super+d", "super+shift+d", "super+[", "super+]", "super+ctrl+=", "super+shift+enter",
              "super+shift+[", "super+shift+]", "ctrl+tab", "ctrl+shift+tab"] { lines.append("keybind = \(k)=unbind") }
    // ⌘=·⌘+·⌘- 는 보드의 터미널 글자 크기(윈도우 > 글자 크기), ⌘0 은 세션 보드 보기로 — 엔진 기본값은 이 넷을 글자 크기 키(increase·decrease·
    // reset_font_size)로 묶는다(Ghostty 1.3.1 +list-keybinds --default). "super+==unbind" 처럼 써도 +validate-config 를 통과한다
    for k in ["=", "+", "-", "0"] { lines.append("keybind = super+\(k)=unbind") }
    let dir = NSTemporaryDirectory() + (Bundle.main.bundleIdentifier ?? instanceName)
    try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
    let path = dir + "/board-terminal.conf"
    try? lines.joined(separator: "\n").write(toFile: path, atomically: true, encoding: .utf8)
    let c = TerminalController(configSource: .file(path), theme: terminalTheme(UserDefaults.standard.string(forKey: "terminalTheme") ?? theme),
                               terminalConfiguration: terminalFontConfiguration())
    // 한 줄이라도 거부되면 파일 전체가 조용히 기본값으로 돌아간다 — 원인을 찾을 수 있게 남긴다
    if let issue = c.lastConfigurationIssue { boardLog("터미널 설정 거부됨: \(issue)") }
    return c
}

// 사용자 Ghostty 설정 파일 — Ghostty 가 읽는 순서(뒤의 파일이 앞을 덮는다)
let ghosttyConfigFiles = ["\(NSHomeDirectory())/.config/ghostty/config", "\(NSHomeDirectory())/.config/ghostty/config.ghostty",
                          "\(NSHomeDirectory())/Library/Application Support/com.mitchellh.ghostty/config",
                          "\(NSHomeDirectory())/Library/Application Support/com.mitchellh.ghostty/config.ghostty"]

// 사용자 Ghostty 설정에서 그 키의 마지막 값(따옴표는 뗀다) — 없으면 nil
func ghosttyConfigValue(_ key: String) -> String? {
    var value: String?
    for f in ghosttyConfigFiles {
        guard let text = try? String(contentsOfFile: f, encoding: .utf8) else { continue }
        for line in text.components(separatedBy: .newlines) {
            let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if parts.count == 2, parts[0] == key { value = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "\"")) }
        }
    }
    return value
}

// 사용자 Ghostty 설정의 글자 크기(마지막 font-size 줄) — 없으면 엔진 기본(macOS 13). 터미널 글자 크기를 처음 바꿀 때의 기준
func ghosttyConfigFontSize() -> Double { ghosttyConfigValue("font-size").flatMap(Double.init) ?? 13 }

// 나눈 화면 가운데 고르지 않은 화면의 불투명도 — Ghostty 의 unfocused-split-opacity(기본 0.7, 0.15~1)
let unfocusedSplitOpacity = CGFloat(min(1, max(0.15, ghosttyConfigValue("unfocused-split-opacity").flatMap(Double.init) ?? 0.7)))

// 나눈 화면 사이 경계 색 — Ghostty 의 split-divider-color(#RRGGBB). 없으면 Ghostty 처럼 터미널 바탕색을 어둡게
func splitDividerColor(background: NSColor) -> NSColor {
    if let v = ghosttyConfigValue("split-divider-color"), let n = UInt32(v.replacingOccurrences(of: "#", with: ""), radix: 16), v.count >= 6 {
        return NSColor(red: CGFloat(n >> 16 & 0xFF) / 255, green: CGFloat(n >> 8 & 0xFF) / 255, blue: CGFloat(n & 0xFF) / 255, alpha: 1)
    }
    return background.blended(withFraction: 0.4, of: .black) ?? background
}

// 보드가 정한 터미널 글자 크기(보드 설정 terminalFontSize) — 래퍼가 사용자 설정 → 이것 → 테마 순으로 한 설정을 만들어 모든 탭에 준다.
// 없으면 빈 설정(사용자 Ghostty 설정 크기)
func terminalFontConfiguration() -> TerminalConfiguration {
    guard let size = UserDefaults.standard.object(forKey: "terminalFontSize") as? Double else { return TerminalConfiguration() }
    return TerminalConfiguration().fontSize(Float(size))
}

// ~/.claude/notifier/board.log 에 한 줄 남긴다 — 보드는 그대로 동작해야 하는 문제(단축키 등록 실패 등)를 적는다
func boardLog(_ message: String) {
    if let h = FileHandle(forWritingAtPath: NSHomeDirectory() + "/.claude/notifier/board.log") {
        h.seekToEndOfFile(); h.write("\(Date()) \(message)\n".data(using: .utf8)!); h.closeFile()
    }
}

// theme 값: "이름" 또는 "light:이름,dark:이름" → 엔진 목록의 라이트·다크 테마(한쪽만 있으면 둘 다 그것). 목록에 없는 이름이면 nil
func themeDefinitions(_ spec: String?) -> (light: GhosttyThemeDefinition, dark: GhosttyThemeDefinition)? {
    guard let spec else { return nil }
    var light = spec, dark = spec
    for part in spec.split(separator: ",") {
        let kv = part.split(separator: ":", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
        guard kv.count == 2 else { continue }
        if kv[0] == "light" { light = kv[1] } else if kv[0] == "dark" { dark = kv[1] }
    }
    guard let d = GhosttyThemeCatalog.theme(named: dark) ?? GhosttyThemeCatalog.theme(named: light) else { return nil }
    return (GhosttyThemeCatalog.theme(named: light) ?? d, d)
}

// 엔진에 넘길 테마 — 목록에 없는 이름이면 래퍼 기본(보드 창은 어두우므로 Afterglow)
func terminalTheme(_ spec: String?) -> TerminalTheme {
    guard let t = themeDefinitions(spec) else { return .default }
    return TerminalTheme(light: t.light.toTerminalConfiguration(), dark: t.dark.toTerminalConfiguration())
}

// "Ghostty 설정 따르기" 일 때 보이는 테마 이름 — 보드 창은 늘 다크라 Ghostty 설정 theme 의 다크 쪽. 없거나 목록에 없으면 래퍼 기본
func ghosttyThemeName() -> String { themeDefinitions(ghosttyConfigValue("theme"))?.dark.name ?? "기본 테마(Afterglow)" }

// 밝은 바탕인가 — 엔진의 GhosttyThemeDefinition.isDark 와 같은 휘도 식(0.299R+0.587G+0.114B 가 0.5 이상이면 밝음)
func isLightColor(_ c: NSColor) -> Bool { 0.299 * c.redComponent + 0.587 * c.greenComponent + 0.114 * c.blueComponent >= 0.5 }

// 보드 터미널 화면 하나(엔진 surface 하나) — 탭은 화면 하나 이상을 나눠(split) 보인다(Ghostty 처럼). 셸·claude 명령·세션 짝짓기는 화면 단위다
final class BoardSurface: NSObject, ObservableObject, Identifiable, TerminalSurfaceTitleDelegate, TerminalSurfaceCloseDelegate,
                          TerminalSurfacePwdDelegate, TerminalSurfaceClipboardConfirmationDelegate, TerminalSurfaceLifecycleDelegate {
    let id = UUID()
    let view: BoardTerminalView
    weak var tab: BoardTab?         // 이 화면이 든 탭
    // 화면은 모두 로그인 셸이다. claude 화면은 보드가 그 셸에 claude 명령을 쳐 넣은 화면 — claude 가 끝나면 셸 프롬프트가 남는다(Ghostty 처럼)
    var command: String?            // 보드가 셸에 넣은 claude 명령 — nil 이면 셸 화면(claude 가 끝나 셸만 남은 화면도)
    var sessionId: String?          // 이 화면의 세션 id(새 세션의 --session-id, 이어 연 세션, tty 로 짝지은 지금 세션) — 끝난 뒤에도 남는다
    var mdRenderOff = false         // 답변 꾸미기를 끄고 띄운 claude 화면 — 이 화면의 세션 id 는 모두 끈 세션으로 기억한다
    var held: [String: String]?     // 다시 뜰 때 셸만 띄우고 들어갈 때 이어 열 claude 화면의 남겨 둔 값(TerminalHub.resumeHeld) — 그때까지 쥔 목록에 그대로 남긴다
    @Published var title = ""
    @Published var cwd: String
    @Published var session: Session?   // 이 화면에서 도는 세션(세션 파일의 tty 로 짝짓는다) — 탭의 상태 점·제목
    @Published var renameTo: String?   // ⇧⌘R 로 바꿀 세션 이름 — /rename 으로 Claude Code 이름이 그렇게 될 때까지(TerminalHub.applyRenames)
    private var shellReady = false     // 셸이 첫 프롬프트를 띄웠다(셸 통합이 경로·제목을 알림)
    private var pending: (() -> String)?   // 셸이 뜨면 넣을 명령 — 넣을 때 만든다(세션 이름은 그때 쓰는 이름을 보고 고른다)
    private var typedAt = Date.distantPast

    init(_ options: TerminalSurfaceOptions, frame: NSRect, controller: TerminalController) {
        cwd = options.workingDirectory ?? NSHomeDirectory()
        view = BoardTerminalView(frame: frame)
        super.init()
        view.delegate = self   // 약한 참조 — 탭이 화면을 쥔다
        view.controller = controller
        view.configuration = options
        view.onFocus = { [weak self] in if let self { TerminalHub.shared.surfaceFocused(self) } }
    }

    var folderName: String { cwd == NSHomeDirectory() ? "~" : (cwd as NSString).lastPathComponent }
    // 세션 파일의 tty 와 같은 꼴(/dev/ 를 뗀 이름)
    var tty: String { (view.ttyName ?? "").replacingOccurrences(of: "/dev/", with: "") }

    // 셸 통합(엔진 리소스)이 첫 프롬프트에서 경로(OSC 7)·제목을 알린다 — 그때 셸이 입력을 받을 준비가 됐다
    func terminalDidChangeTitle(_ title: String) { self.title = title; shellStarted() }
    func terminalDidChangeWorkingDirectory(_ path: String) { cwd = path; shellStarted() }
    func terminalDidDetachSurface() {}

    // 경로·제목을 알리지 않는 셸(셸 통합이 없는 셸)에는 5초 뒤 미리 쳐 둔다 — 셸이 뜨면 쌓인 입력을 읽는다
    func terminalDidAttachSurface(_ surface: TerminalSurface) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in self?.shellStarted() }
    }

    private func shellStarted() {
        guard !shellReady else { return }
        shellReady = true
        sendPending(tries: 30)
    }

    // 셸에 명령을 사람이 친 것처럼 넣는다(붙여넣기 + Return — 셸 기록에 남아 ↑ 로 보인다). 셸이 아직 뜨지 않았으면 뜬 뒤에
    func type(_ command: @escaping () -> String) {
        pending = command
        if shellReady { sendPending(tries: 30) }
    }

    // 셸의 줄 편집기가 입력을 읽기 시작하면(tty 의 정규 모드·되울림이 꺼짐) 넣는다 — 경로를 알린 직후(precmd)에 넣으면 tty 가
    // 글자를 프롬프트 위에 한 번 더 찍는다(실측: 그때 정규 모드는 꺼졌지만 되울림은 켜져 있었다). 3초를 기다려도 그대로면 그냥 넣는다(셸이 뜨면 쌓인 입력을 읽는다)
    private func sendPending(tries: Int) {
        guard let make = pending else { return }
        guard tries <= 0 || lineEditing else {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in self?.sendPending(tries: tries - 1) }
            return
        }
        pending = nil
        typedAt = Date()
        drive([("text", make()), ("wait", "0.1"), ("key", "enter")]) { _ in }
    }

    private var lineEditing: Bool {
        let fd = Darwin.open(view.ttyName ?? "", O_RDONLY | O_NOCTTY | O_NONBLOCK)
        guard fd >= 0 else { return true }   // tty 를 못 열면 기다리지 않는다
        defer { Darwin.close(fd) }
        var t = termios()
        return tcgetattr(fd, &t) != 0 || t.c_lflag & tcflag_t(ICANON | ECHO) == 0
    }

    // 보드가 넣은 claude 가 셸이 뜨기를 기다리거나 막 넣었거나(3초) 앞에서 돌고 있다
    var runsClaude: Bool {
        pending != nil || Date().timeIntervalSince(typedAt) < 3 || view.foregroundPid.map(isLiveClaude) == true
    }

    // 셸이 프롬프트에 있다(셸 위에 도는 프로그램이 없다) — 앞 프로세스 그룹이 셸 자신(엔진이 띄운 login 의 자식)이면
    var atPrompt: Bool {
        guard let fg = view.foregroundPid, fg > 0 else { return false }
        let leader = getsid(fg)
        return leader > 0 && fg != leader && parentPid(fg) == leader
    }

    // pid 가 화면의 앞(포그라운드) 프로그램인가 — 그 프로세스 그룹이 tty 의 포그라운드 그룹이면. 셸에서 띄운 claude 는 제 작업 그룹이
    // 포그라운드다. 셸 화면에서 & 로 띄운 claude -p 는 아니다(키를 넣으면 셸이 받는다)
    func isForeground(_ pid: Int32) -> Bool {
        guard pid > 0, let fg = view.foregroundPid, fg > 0 else { return false }
        return getpgid(pid) == fg
    }

    // 화면의 프로그램에 키·글자를 넣는다(driveSession 의 단계) — 키는 키 경로, 글자는 붙여넣기
    func drive(_ steps: [(String, String)], done: @escaping @MainActor (Bool) -> Void) {
        guard let (kind, value) = steps.first else { return done(true) }
        let rest = Array(steps.dropFirst())
        switch kind {
        case "key": _ = view.sendKey(value == "escape" ? .escape : .enter)
        case "text": _ = view.paste(text: value)
        default:
            DispatchQueue.main.asyncAfter(deadline: .now() + (Double(value) ?? 0)) { self.drive(rest, done: done) }
            return
        }
        drive(rest, done: done)
    }

    // 셸이 끝났거나(processAlive false) ⌘W 처럼 엔진이 닫기를 청했을 때
    func terminalDidClose(processAlive: Bool) { TerminalHub.shared.close(self, confirm: processAlive) }

    // 붙여넣기 보호(줄바꿈이 든 내용을 bracketed paste 가 꺼진 프로그램에)·프로그램의 클립보드 읽기(OSC 52)를 묻는다 —
    // 처리기가 없으면 붙여넣기는 알림 없이 버려진다
    func terminalDidRequestClipboardConfirmation(_ request: TerminalClipboardConfirmationRequest) {
        let a = NSAlert()
        switch request.kind {
        case .paste:
            a.messageText = "여러 줄을 붙여넣을까요?"
            a.informativeText = "줄바꿈이 들어 있어 터미널의 프로그램이 줄마다 바로 실행할 수 있습니다.\n\n" + String(request.contents.prefix(400))
            a.addButton(withTitle: "붙여넣기")
        case .osc52Read:
            a.messageText = "터미널의 프로그램이 클립보드를 읽으려고 합니다"
            a.informativeText = "허용하면 지금 클립보드 내용이 그 프로그램에 전달됩니다."
            a.addButton(withTitle: "허용")
        case .osc52Write:
            a.messageText = "터미널의 프로그램이 클립보드에 쓰려고 합니다"
            a.informativeText = String(request.contents.prefix(400))
            a.addButton(withTitle: "허용")
        }
        a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"   // NSAlert 는 영어 "Cancel" 에만 Esc 를 붙인다
        MainActor.assumeIsolated { whenPaneSettled { a.beginSheetModal(for: window) { request.respond(allow: $0 == .alertFirstButtonReturn) } } }
    }
}

enum SplitDirection { case left, right, up, down }

// 탭의 화면 나누기 트리(Ghostty 의 SplitTree 와 같은 꼴) — 잎은 화면 하나, 가지는 둘로 나눈 것: right 면 왼쪽|오른쪽(⌘D), 아니면 위/아래(⇧⌘D),
// ratio 는 앞쪽(왼쪽·위)의 몫. 경계를 끌거나 ⌃⌘방향키로 ratio 가 바뀐다. 나누기·닫기는 마디를 그 자리에서 바꾼다(부모가 쥔 마디 그대로)
final class SplitNode {
    private(set) var surface: BoardSurface?       // 잎이면 그 화면
    private(set) var children: [SplitNode] = []   // 가지면 [앞, 뒤]
    private(set) var right = true
    var ratio: CGFloat = 0.5
    private(set) weak var parent: SplitNode?

    init(_ surface: BoardSurface) { self.surface = surface }
    init(right: Bool, ratio: CGFloat, _ a: SplitNode, _ b: SplitNode) {
        self.right = right
        self.ratio = ratio
        children = [a, b]
        a.parent = self
        b.parent = self
    }

    // 화면들 — 트리 순서(왼쪽·위가 앞). ⌘[·⌘] 가 이 순서로 돈다
    var surfaces: [BoardSurface] { surface.map { [$0] } ?? children.flatMap(\.surfaces) }

    func leaf(_ s: BoardSurface) -> SplitNode? { surface === s ? self : children.lazy.compactMap { $0.leaf(s) }.first }

    // 이 잎을 둘로 — 앞은 지금 화면, 뒤(오른쪽·아래)는 새 화면, 50:50
    func split(_ s: BoardSurface, right: Bool) {
        guard let old = surface else { return }
        surface = nil
        self.right = right
        ratio = 0.5
        children = [SplitNode(old), SplitNode(s)]
        children.forEach { $0.parent = self }
    }

    // 이 잎을 빼고 부모 자리를 형제가 차지한다. 돌려주는 값은 그다음 고를 화면 — 형제 쪽에서 뺀 화면에 가장 가까운 것
    // (앞이 빠지면 형제의 첫 화면, 뒤가 빠지면 형제의 마지막 화면). 탭의 마지막 화면(부모 없음)이면 nil
    func remove() -> BoardSurface? {
        guard let p = parent, let i = p.children.firstIndex(where: { $0 === self }) else { return nil }
        let sib = p.children[1 - i]
        p.surface = sib.surface
        p.right = sib.right
        p.ratio = sib.ratio
        p.children = sib.children
        p.children.forEach { $0.parent = p }
        return i == 0 ? p.surfaces.first : p.surfaces.last
    }

    // ⌃⌘= — 몫을 같은 방향으로 나눈 화면 수대로(Ghostty 의 equalize_splits 와 같은 무게)
    func equalize() {
        guard children.count == 2 else { return }
        let a = children[0].weight(right), b = children[1].weight(right)
        ratio = CGFloat(a) / CGFloat(a + b)
        children.forEach { $0.equalize() }
    }
    private func weight(_ dir: Bool) -> Int { children.count == 2 && right == dir ? children[0].weight(dir) + children[1].weight(dir) : 1 }

    // 화면마다 자리와 가지마다 (가지 자리, 경계 선 자리) — 경계 선 1pt 를 빼고 몫대로 나눈다. 좌표는 AppKit(아래가 0)
    func layout(in r: NSRect, _ frames: inout [(BoardSurface, NSRect)], _ lines: inout [(SplitNode, NSRect, NSRect)]) {
        if let surface { frames.append((surface, r)); return }
        guard children.count == 2 else { return }
        if right {
            let w = max(0, ((r.width - 1) * ratio).rounded())
            children[0].layout(in: NSRect(x: r.minX, y: r.minY, width: w, height: r.height), &frames, &lines)
            lines.append((self, r, NSRect(x: r.minX + w, y: r.minY, width: 1, height: r.height)))
            children[1].layout(in: NSRect(x: r.minX + w + 1, y: r.minY, width: max(0, r.width - w - 1), height: r.height), &frames, &lines)
        } else {
            let h = max(0, ((r.height - 1) * ratio).rounded())
            children[0].layout(in: NSRect(x: r.minX, y: r.maxY - h, width: r.width, height: h), &frames, &lines)
            lines.append((self, r, NSRect(x: r.minX, y: r.maxY - h - 1, width: r.width, height: 1)))
            children[1].layout(in: NSRect(x: r.minX, y: r.minY, width: r.width, height: max(0, r.height - h - 1)), &frames, &lines)
        }
    }

    // 몫을 바꾼다 — 양쪽 화면이 80pt 아래로 줄지 않게(length 는 이 가지의 폭·높이)
    func setRatio(_ value: CGFloat, length: CGFloat) {
        let m = min(0.5, 80 / max(length, 1))
        ratio = min(max(value, m), 1 - m)
    }
}

// 보드 터미널 탭 하나 — 화면 나누기 트리 하나와 그 가운데 고른 화면. 탭 이름·⌘ 번호·경로 줄·카드 강조는 탭 단위다
@MainActor final class BoardTab: ObservableObject, Identifiable {
    let id = UUID()
    // 탭의 작업 경로(경로 줄의 묶음, workspaceKey 꼴) — 탭을 연 경로(이어 열면 세션 기록의 project_dir, 없으면 cwd). 셸에서 cd 해도 바뀌지 않는다(cwd 는 따라간다)
    let workspace: String
    let root: SplitNode
    @Published var focused: BoardSurface   // 고른 화면 — 키 입력·탭의 "지금 하는 일"·⌘D 로 나눌 화면
    @Published var zoomed = false          // 고른 화면 하나 크게 보기(⇧⌘↩) — 나머지 화면은 숨긴다
    @Published var customName: String? // 사용자가 붙인 탭 이름(⇧⌘R) — 없으면 폴더 이름. 쥔 탭 목록에 남아 다시 떠도 그대로
    @Published var renaming = false    // 탭 이름 칸을 고치는 중
    private var watching: [ObjectIdentifier: AnyCancellable] = [:]

    init(_ root: SplitNode, focused: BoardSurface? = nil, workspace: String) {
        self.root = root
        self.focused = focused ?? root.surfaces[0]
        self.workspace = workspaceKey(workspace)
        root.surfaces.forEach(watch)
    }

    var surfaces: [BoardSurface] { root.surfaces }

    // 화면의 제목·경로·세션이 바뀌면 탭 줄도 다시 그린다
    func watch(_ s: BoardSurface) {
        s.tab = self
        watching[ObjectIdentifier(s)] = s.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
    }
    func unwatch(_ s: BoardSurface) { watching[ObjectIdentifier(s)] = nil }

    // 탭 이름 — 사용자가 붙인 이름, 없으면 폴더 이름: 화면이 하나면 그 화면 셸의 경로(cd 를 따라감, 예전 그대로),
    // 나눈 탭이면 탭을 연 경로로 고정(고른 화면이 다른 폴더여도 그대로 — 사용자 결정)
    var name: String { customName ?? folderName }
    // 탭 줄에 보이는 이름 — 화면이 하나이고 그 화면에 claude 세션이 돌면 그 세션의 Claude Code 이름(번호가 붙었거나 /rename 한 이름도
    // 그대로 — 탭과 세션 이름이 같은 글자로 보이게. 이름 바꾸기를 미뤄 둔 동안은 바꿀 이름), 아니면 name. 나눈 탭은 name 그대로
    var shownName: String { surfaces.count == 1 ? focused.renameTo ?? focused.session?.name ?? name : name }
    var folderName: String {
        guard surfaces.count > 1 else { return focused.folderName }
        return workspace == NSHomeDirectory() ? "~" : (workspace as NSString).lastPathComponent
    }
    // 탭 줄의 상태 점 — 화면들의 세션 가운데 가장 급한 상태(세션이 없으면 대기)
    var state: String { surfaces.compactMap(\.session).min { $0.rank < $1.rank }?.state ?? "idle" }
}

@MainActor final class TerminalHub: ObservableObject {
    static let shared = TerminalHub()
    @Published private(set) var tabs: [BoardTab] = []   // 같은 경로의 탭끼리 붙여 둔다(openShell)
    @Published private(set) var selected: UUID?
    // 지금 경로 — 고른 탭의 작업 경로(select). 탭 줄은 이 경로의 탭만 보인다
    @Published private(set) var workspace: String?
    private var lastTab: [String: UUID] = [:]   // 경로마다 마지막에 고른 탭 — 경로를 바꾸면 그 탭으로
    // 경로 줄의 상태 점 — 경로마다 그 탭들의 세션 가운데 가장 급한 상태(짝지을 때 고친다)
    @Published private(set) var workspaceStates: [String: String] = [:]
    // 펼침 — 기본값은 펼침
    @Published private(set) var expanded = UserDefaults.standard.object(forKey: "terminalExpanded") as? Bool ?? true
    // 펼쳤을 때 보드 폭 — 경계를 끌어 바꾼 폭을 기억한다
    var boardWidth = CGFloat(UserDefaults.standard.object(forKey: "boardPaneWidth") as? Double ?? Double(boardLaunchWidth))
    // 보드 위치 — 화면 오른쪽(기본)·왼쪽(보드 설정 boardSide). 왼쪽이면 펼친 배치가 [보드 | 경계 | 터미널] 이고
    // 접기·최대축소·처음 크기로 붙이기도 화면 왼쪽 기준이다
    @Published private(set) var boardOnLeft = UserDefaults.standard.string(forKey: "boardSide") == "left"
    // 펼치기·접기 애니메이션이 도는 중이면 그 애니메이션 — 그동안 BoardRootView 가 터미널 영역 크기를 고정한다
    var slide: PaneSlide?
    var tabBarEnd: CGFloat = .infinity   // 탭 줄에서 탭·"새 세션" 단추가 끝나는 x — 그 오른쪽이 빈 곳
    var workspaceBarEnd: CGFloat = .infinity   // 경로 줄에서 경로 항목이 끝나는 x — 그 오른쪽이 빈 곳
    // ⌘ 를 누르고 있다(앱 안 flagsChanged 감시) — 그동안 탭 줄에 ⌘번호와 이름 바꾸기 단축키를 보인다. 앱이 비활성이 되면 끈다
    @Published private(set) var commandHeld = false
    weak var area: TerminalAreaView?
    weak var store: Store?
    // 세션 id → 그 세션이 도는 보드 탭(tty 로 짝지은 세션, 탭 안에서 시작된 claude -p) — 세션 목록을 다시 읽을 때마다 고친다
    @Published private(set) var paired: [String: UUID] = [:]
    @Published private(set) var tabOwnedHeadless: Set<String> = []
    // 다른 터미널 세션의 앱 이름(칩) — 세션마다 한 번 조상 프로세스로 찾는다(AppleScript 없이)
    @Published private(set) var appNames: [String: String] = [:]
    private var modes: [Int32: String] = [:]   // 백그라운드 세션 pid → 실행 방식(-p, --bg …)
    // 보드 아래쪽에 잠깐 뜨는 안내 — 백그라운드 세션을 눌렀을 때 등
    @Published private(set) var notice: String?
    private var noticeHide: DispatchWorkItem?
    // 고른 탭·터미널 영역 바탕 — 엔진을 띄운 뒤에는 터미널 테마의 바탕색(탭이 터미널과 이어져 보이게)
    @Published private(set) var background = terminalAreaColor
    private var dividerColor = NSColor.gray   // 나눈 화면 사이 경계 색 — 엔진을 띄울 때 정한다
    private var engine: TerminalController?

    // 엔진은 첫 탭을 열 때 띄운다
    var controller: TerminalController {
        if let engine { return engine }
        let c = makeTerminalController()
        engine = c
        // 엔진은 라이트로 시작하고 화면이 창에 붙을 때에야 창 외관(늘 다크)을 따른다 — 바탕색을 읽기 전에 다크로 맞춘다.
        // 안 그러면 라이트·다크 바탕이 다른 테마(theme 줄이 없을 때의 래퍼 기본 등)에서 탭·경로 줄만 라이트 바탕으로 남는다
        c.setColorScheme(.dark)
        readBackground(c)
        return c
    }

    // 고른 탭·경로 칸·터미널 칸 바탕과 나눈 화면 경계 색을 엔진의 지금 바탕색으로 — 엔진을 띄울 때, 테마를 바꿀 때
    private func readBackground(_ c: TerminalController) {
        let bg = c.backgroundColor
        background = NSColor(red: CGFloat(bg.red) / 255, green: CGFloat(bg.green) / 255, blue: CGFloat(bg.blue) / 255, alpha: 1)
        dividerColor = splitDividerColor(background: background)
        area?.container.layer?.backgroundColor = background.cgColor
    }

    func setExpanded(_ on: Bool) {
        expanded = on
        UserDefaults.standard.set(on, forKey: "terminalExpanded")
    }

    // 터미널 글자 크기(윈도우 > 글자 크기, ⌘+·⌘-) — step +1·-1 은 1pt씩(Ghostty 와 같음, 8~40), nil 은 원래 크기(보드 설정을 지워 Ghostty 설정 크기로).
    // 열린 탭 모두 바로 바뀐다 — 엔진 설정을 다시 읽히면 글자 크기를 손대지 않은 surface 는 설정 크기를 따른다(탭마다 키로 바꾸는 엔진 단축키는 풀어 두었다).
    // 새로 여는 탭·다시 뜰 때 되살린 탭도 같은 설정이다
    func changeTerminalFont(_ step: Int?) {
        let d = UserDefaults.standard
        if let step {
            let now = d.object(forKey: "terminalFontSize") as? Double ?? ghosttyConfigFontSize()
            d.set(min(40, max(8, now + Double(step))), forKey: "terminalFontSize")
        } else {
            d.removeObject(forKey: "terminalFontSize")
        }
        engine?.setTerminalConfiguration(terminalFontConfiguration())   // 사용자 설정·테마는 래퍼가 그대로 다시 얹는다
        toast("터미널 글자 \(String(format: "%g", d.object(forKey: "terminalFontSize") as? Double ?? ghosttyConfigFontSize()))pt")
    }

    // 터미널 테마(윈도우 > 터미널 테마…) — 보드 설정 terminalTheme(테마 이름). nil 은 지워 Ghostty 설정의 theme 을 따른다.
    // 열린 탭 모두 바로 바뀌고 고른 탭·경로 칸 바탕·경계 색도 다시 읽는다. 엔진을 아직 띄우지 않았으면 설정만 남아 처음 띄울 때 쓴다
    func setTerminalTheme(_ name: String?) {
        let d = UserDefaults.standard
        if let name { d.set(name, forKey: "terminalTheme") } else { d.removeObject(forKey: "terminalTheme") }
        if let engine {
            engine.setTheme(terminalTheme(name ?? ghosttyConfigValue("theme")))
            readBackground(engine)
            layoutSurfaces()   // 나눈 화면 경계 색
        }
        toast("터미널 테마 \(name ?? "Ghostty 설정 따르기(\(ghosttyThemeName()))")")
    }

    func setBoardOnLeft(_ on: Bool) {
        boardOnLeft = on
        UserDefaults.standard.set(on ? "left" : "right", forKey: "boardSide")
    }

    // 셸 탭 — 엔진이 사용자의 로그인 셸(passwd 의 셸)을 login 으로 띄운다. login 이 환경을 스스로 꾸리므로 환경 변수를 넘기지 않는다.
    // 셸이 끝나면(exit) 엔진이 닫기를 청해 그 화면을 닫는다(terminalDidClose — 탭의 마지막 화면이면 탭째)
    @discardableResult
    func openShell(in dir: String) -> BoardTab? {
        guard area != nil else { return nil }
        let tab = BoardTab(SplitNode(makeSurface(in: dir)), workspace: dir)
        add(tab)
        return tab
    }

    // 화면 하나 — 그 폴더의 로그인 셸. 창에 붙일 때 셸이 뜬다
    private func makeSurface(in dir: String) -> BoardSurface {
        BoardSurface(TerminalSurfaceOptions(backend: .exec, workingDirectory: dir), frame: .zero, controller: controller)
    }

    // 탭을 탭 목록에 넣고 화면들을 창에 붙인 뒤 고른다 — 자리를 먼저 정하고 붙인다(크기가 0 인 뷰는 셸을 띄우지 않는다)
    private func add(_ tab: BoardTab) {
        guard let area else { return }
        area.workspaceBarShown = true             // 첫 탭이면 경로 줄이 생겨 터미널 칸이 줄어든다 — 탭을 만들기 전에 맞춘다
        area.superview?.layoutSubtreeIfNeeded()
        // 같은 경로 탭들의 끝에(새 경로면 맨 끝에) — 경로끼리 붙어 있어 경로 줄 순서가 앞 탭을 닫아도 그대로다
        if let last = tabs.lastIndex(where: { $0.workspace == tab.workspace }) { tabs.insert(tab, at: last + 1) } else { tabs.append(tab) }
        layoutSurfaces()
        tab.surfaces.forEach(attach)
        select(tab.id)
        saveTabs()
    }

    // 화면 뷰를 터미널 칸 맨 아래에(경계 뷰는 그 위) 숨긴 채 붙인다 — 보이기는 applyVisibility
    private func attach(_ s: BoardSurface) {
        s.view.isHidden = true
        area?.container.addSubview(s.view, positioned: .below, relativeTo: nil)
    }

    // 경로 줄의 경로들 — 탭 목록에 처음 나타난 순서
    var workspaces: [String] {
        var list: [String] = []
        for t in tabs where !list.contains(t.workspace) { list.append(t.workspace) }
        return list
    }

    func tabs(in ws: String?) -> [BoardTab] { tabs.filter { $0.workspace == ws } }

    // 경로에서 보일 탭 — 그 경로에서 마지막에 고른 탭(닫혔으면 첫 탭). 탭이 없으면 nil
    private func shownTab(_ ws: String) -> UUID? {
        if let id = lastTab[ws], tabs.contains(where: { $0.id == id }) { return id }
        return tabs.first { $0.workspace == ws }?.id
    }

    // 경로 줄 항목·보드 카드 머리를 눌렀을 때 — 그 경로에 탭이 있으면 그 경로로(마지막에 고른 탭). 터미널 포커스는 펼쳐 있을 때만(select).
    // 접혀 있으면 펼치지 않고 바꿔만 둔다 — 펼치면 그 경로가 보인다
    func showWorkspace(_ path: String) {
        if let id = shownTab(workspaceKey(path)) { select(id) }
    }

    // ⌃⌥↑·⌃⌥↓ — 경로 줄의 앞·뒤 경로로(끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다)
    func stepWorkspace(_ step: Int) {
        let list = workspaces
        guard !list.isEmpty else { return }
        let i = workspace.flatMap { list.firstIndex(of: $0) } ?? 0
        if let id = shownTab(list[(i + step + list.count) % list.count]) { reveal(id) }
    }

    // 명령(claude …)을 보드 새 탭에서 띄운다 — 그 작업 경로의 셸 탭을 열고 셸에 명령을 쳐 넣는다. claude 가 끝나면 그 셸 프롬프트로 돌아온다.
    // 셸이 띄우므로 프로세스 이름이 claude 그대로(hook 이 알아봄)이고 셸 설정의 PATH 등을 그대로 쓴다.
    // mdRenderOff — 답변 꾸미기를 끈 탭(명령은 부르는 쪽이 mdRenderCommand 로 고쳐 넘긴다). 그 세션 id 를 끈 세션으로 기억한다
    func launch(_ command: String, in dir: String, sessionId: String?, mdRenderOff: Bool = false) {
        guard let tab = openShell(in: dir) else { return }
        run(command, in: tab.focused, sessionId: sessionId, mdRenderOff: mdRenderOff)
        reveal(tab.id)
    }

    // 화면의 셸에 claude 명령을 넣고 그 화면을 claude 화면으로 삼는다(새 탭, 셸 프롬프트로 남은 화면에서 이어 열기, 다시 뜰 때 되살리기).
    // 세션 이름(--name)은 셸에 칠 때 붙인다 — 되살릴 때는 그때야 탭과 탭 이름이 생긴다. 쥔 탭 목록의 명령에는 남기지 않는다(되살릴 때 다시 고른다)
    func run(_ command: String, in s: BoardSurface, sessionId: String?, mdRenderOff: Bool) {
        s.held = nil
        s.command = command
        s.sessionId = sessionId
        s.mdRenderOff = mdRenderOff
        if mdRenderOff { rememberMdRenderOff(sessionId) }
        s.type { [weak self, weak s] in
            MainActor.assumeIsolated { s.flatMap { s in self?.named(command, in: s) } ?? command }
        }
        saveTabs()
    }

    // 세션 이름(사용자 결정 2026-10-20) — 보드 탭 claude 는 탭 이름(⇧⌘R 이름, 없으면 폴더 이름)을 세션 이름으로 쓰고, 그 이름을 쓰는 다른
    // 세션이 있으면 -2, -3 … 을 붙인다(나눈 탭의 화면들도 이렇게 나뉜다). Claude Code 는 같은 이름도 그대로 받으므로(2.1.304 실측) 보드가 고른다.
    // 탭 줄·카드 라벨은 고른 번호가 아니라 ~/.claude/sessions 의 이름을 보인다(탭 줄은 미뤄 둔 이름 바꾸기가 있으면 그 이름)
    private var reservedNames: [UUID: (name: String, at: Date)] = [:]   // 화면 → 방금 --name 으로 붙여 아직 파일이 없을 수 있는 이름(15초)
    // 화면 → 이름 바꾸기를 어디까지 쳤나(없으면 아직 안 침 — 바꿀 이름은 BoardSurface.renameTo): 치는 중, 글은 붙였고 Return 만 남음, Return 친 시각
    private enum RenameStep { case typing, typed, sent(Date) }
    private var renameSteps: [UUID: RenameStep] = [:]

    // 쓰는 이름 = 살아 있는 claude 의 이름 + 방금 붙인 이름 + 미뤄 둔 이름 바꾸기 + taken. 그 화면 자신의 것(지금 도는 claude 의 이름·제 몫)은
    // 빼고 본다. 대소문자는 가리지 않는다(Claude Code 는 가리지 않고 둘 다 받지만 세션끼리 부르는 주소라 헷갈리지 않게)
    private func pickName(_ base: String, for s: BoardSurface, taken: Set<String> = []) -> String {
        let own = s.session?.pid
        var used = Set(ClaudeNames.shared.live().filter { $0.key != own }.values.map { $0.lowercased() })
        for (id, r) in reservedNames where id != s.id && Date().timeIntervalSince(r.at) < 15 { used.insert(r.name.lowercased()) }
        for o in surfaces where o !== s { if let n = o.renameTo { used.insert(n.lowercased()) } }
        used.formUnion(taken.map { $0.lowercased() })
        var name = base, i = 2
        while used.contains(name.lowercased()) { name = "\(base)-\(i)"; i += 1 }
        return name
    }

    // 셸에 칠 claude 명령에 --name <이름> 을 claude 바로 뒤에 붙인다(셸에 치므로 shlexQuote — 한글·공백 이름). claude 가 아니거나
    // 사용자가 -n·--name 을 적었으면 그대로. --resume 과 함께 주면 --name 이 이긴다(실측 — 없으면 예전 대화 이름을 되살린다)
    private func named(_ command: String, in s: BoardSurface) -> String {
        guard let r = command.range(of: #"^\s*(\S*/)?claude(?=\s|$)"#, options: .regularExpression),
              command.range(of: #"(^|\s)(-n|--name)(=|\s|$)"#, options: .regularExpression) == nil else { return command }
        let name = pickName(s.tab?.name ?? s.folderName, for: s)
        reservedNames[s.id] = (name, Date())
        return command.replacingCharacters(in: r.upperBound..<r.upperBound, with: " --name " + shlexQuote(name))
    }

    // ⇧⌘R 로 바꾼 탭 이름을 그 탭의 claude 세션들에 — 화면마다 /rename <이름>(나눈 탭은 번호로 나뉜다, 사용자 결정). 셸만 남은 화면·
    // claude -p 는 빼고, 이미 그 이름이면 치지 않는다. 치는 것은 applyRenames
    private func renameSessions(_ tab: BoardTab) {
        tab.surfaces.forEach(clearRename)
        var taken: Set<String> = []
        for s in tab.surfaces {
            guard let ss = s.session, ss.state != "ended", s.isForeground(ss.pid), mode(ss) == nil else { continue }
            let target = pickName(tab.name, for: s, taken: taken)
            taken.insert(target)
            if ClaudeNames.shared.name(ss.pid) != target { s.renameTo = target }
        }
        applyRenames()
    }

    private func clearRename(_ s: BoardSurface) {
        if s.renameTo != nil { s.renameTo = nil }
        renameSteps[s.id] = nil
    }

    // 미뤄 둔 이름 바꾸기를 친다(⇧⌘R 직후, 그 뒤 세션 목록을 읽을 때마다). 승인 창·질문 창이 떠 있을 때 치면 Return 이 그 창에 들어가
    // 도구 실행을 허락해 버릴 수 있다 — 세션이 작업 중·대기일 때만 치고(승인 대기·일시중지는 미룬다), 글을 붙인 뒤 Return 직전에 지금
    // 파일로 한 번 더 본다. 그 사이 창이 떴으면 Return 만 남겨 두었다가 창이 닫히면 친다(빈 입력 칸의 Return 은 아무 일도 하지 않는다).
    // 요청 전 세션(대화 기록이 아직 없음)도 첫 요청이 들어갈 때까지 미룬다(사용자 결정) — /rename 이 대화 기록을 만들어, 요청 없이 끝나도
    // 목록에 "요청 전" 카드가 남는다. 미루는 동안 탭 줄은 바꿀 이름, 카드 라벨은 실제 이름(옛 이름)이다.
    // Return 을 친 뒤에는 Claude Code 이름이 바뀌면 끝, 5초가 지나도 그대로면 그만둔다(탭 줄이 실제 이름으로 돌아간다).
    // 사용자가 입력 칸에 쓰던 글이 있으면 섞일 수 있다(사용자가 알고 고름)
    func applyRenames() {
        for s in surfaces {
            guard let target = s.renameTo else { continue }
            guard let ss = s.session, ss.state != "ended", s.isForeground(ss.pid), ClaudeNames.shared.name(ss.pid) != target else {
                clearRename(s)   // 바뀌었거나 claude 가 끝났다
                continue
            }
            var typed = false
            switch renameSteps[s.id] {
            case .typing: continue
            case .sent(let at): if Date().timeIntervalSince(at) > 5 { clearRename(s) }; continue
            case .typed: typed = true
            case nil: break
            }
            guard renameSafe(ss), hasTranscript(ss) else { continue }
            renameSteps[s.id] = .typing
            s.drive(typed ? [] : [("text", "/rename " + target), ("wait", "0.2")]) { [weak self] _ in
                guard let self, s.renameTo == target else { return }   // 그 사이 다시 바꿨거나 화면이 닫혔다
                guard let now = s.session, self.renameSafe(now) else { self.renameSteps[s.id] = .typed; return }
                s.drive([("key", "enter")]) { _ in }
                self.renameSteps[s.id] = .sent(Date())
            }
        }
    }

    // 요청이 들어간 세션인가 — 대화 기록 파일이 있다. Claude Code 는 첫 요청을 보낼 때 이 파일을 만든다(--name·/model 만으로는 생기지 않고
    // 첫 요청 0.1초 뒤 생김 — 2.1.304 실측). 보드 목록도 이 파일이 없는 끝난 세션은 지운다(Store.reload, 같은 경로 계산)
    private func hasTranscript(_ s: Session) -> Bool {
        FileManager.default.fileExists(atPath: transcriptPath(sessionRecord(s.id)?["transcript_path"] as? String, s.cwd, s.id))
    }

    // 이름 바꾸기를 쳐도 되는가 — 보드 상태가 작업 중·대기이고 승인 창·질문 창이 없다. 보드 상태는 1초마다 읽은 것이라 Claude Code 의
    // status(창을 띄우면 바로 waiting, Esc 로 닫으면 idle — 2.1.304 실측)를 지금 읽어 본다. status 가 없으면 hook 이 쓴 세션 파일의 상태를 지금
    private func renameSafe(_ s: Session) -> Bool {
        guard s.state == "working" || s.state == "idle" else { return false }
        if let status = ClaudeNames.shared.status(s.pid) { return status != "waiting" }
        return sessionRecord(s.id)?["state"] as? String != "waiting"
    }

    // 쥔 탭 목록 — 보드 설정 terminalTabs 에 탭마다 [workspace, name, selected, last] 와 화면 값 [cwd, command, mdoff, session, resume] 을 남긴다
    // (셸 화면은 경로만). 화면이 하나인 탭은 화면 값을 탭 항목에 바로 넣어 3.0.x 목록과 같은 꼴이고, 나눈 탭은 root 에 화면 트리
    // (가지 {split: right|down, ratio, a, b}, 잎은 화면 값 — 고른 화면에 focused)를 넣는다. 크게 보기는 남기지 않는다.
    // workspace 는 탭의 작업 경로, selected 는 고른 탭(곧 지금 경로), last 는 경로마다 마지막에 고른 탭.
    // 종료 때만이 아니라 바뀔 때마다(열기·닫기·나누기·경계 옮기기·화면 고르기·세션 id 정해짐·끝남, 1초마다 짝지을 때 견줘서) 남긴다 —
    // pkill·충돌로 꺼져도 다시 뜰 때 되살린다. claude 가 끝나 셸만 남은 화면은 셸 화면으로 남긴다. 되살리기 전에는 남기지 않는다(지난 목록을 빈 목록으로 덮지 않게)
    private var restored = false
    private var savedTabs: NSArray?

    func saveTabs() {
        guard restored else { return }
        let list = tabs.map { t -> [String: Any] in
            var e: [String: Any] = ["workspace": t.workspace]
            if let n = t.customName { e["name"] = n }
            if t.id == selected { e["selected"] = "1" }
            if lastTab[t.workspace] == t.id { e["last"] = "1" }
            if let s = t.root.surface { for (k, v) in entry(s) { e[k] = v } } else { e["root"] = savedNode(t.root, focused: t.focused) }
            return e
        } as NSArray
        guard !list.isEqual(savedTabs) else { return }
        savedTabs = list
        UserDefaults.standard.set(list, forKey: "terminalTabs")
    }

    // 화면 값 — 셸 화면은 경로만, claude 화면은 명령·세션까지. 들어가지 않아 아직 이어 열지 않은 화면은 남겨 둔 값 그대로
    private func entry(_ s: BoardSurface) -> [String: String] {
        if let held = s.held { return held }
        var e = ["cwd": s.cwd]
        if let command = s.command {
            e["command"] = command
            if s.mdRenderOff { e["mdoff"] = "1" }   // 대화 없이 꺼져 세션 파일·끈 세션 기억이 치워져도 끈 채로 되살린다
            // 화면 안에서 /clear 등으로 세션 id 가 바뀌면 tty 로 짝지은 세션이 지금 것이다
            if let id = s.session?.id ?? s.sessionId {
                e["session"] = id
                if let ss = s.session, ss.id == id, !ss.resumeCmd.isEmpty { e["resume"] = ss.resumeCmd }
            }
        }
        return e
    }

    private func savedNode(_ n: SplitNode, focused: BoardSurface) -> [String: Any] {
        if let s = n.surface {
            var e: [String: Any] = entry(s)
            if s === focused { e["focused"] = "1" }
            return e
        }
        return ["split": n.right ? "right" : "down", "ratio": Double(n.ratio),
                "a": savedNode(n.children[0], focused: focused), "b": savedNode(n.children[1], focused: focused)]
    }

    // 다시 뜰 때 — 지난번에 쥐고 있던 탭을 나눈 모양 그대로 되살린다. 셸 화면은 그 경로에서 셸, claude 화면도 우선 셸만 띄우고(held)
    // 그 탭에 들어갈 때 세션 파일의 resume_cmd 를 넣어 이어 연다(사용자 결정 2026-10-21) — 마지막에 고른 탭은 되살린 뒤 바로 고르므로 바로 열린다.
    // 작업 경로는 남겨 둔 것, 없으면(3.0.x 목록) 되살릴 때 여는 경로. 지금 경로·경로마다 마지막 탭도 되살린다. 보드가 뜰 때 한 번 부른다.
    // 업데이트처럼 보드를 끄고 바로 다시 띄우면 옛 탭의 claude 가 아직 끝나는 중일 수 있다 — 같은 세션을 두 프로세스가 쥐지 않게 최대 5초 기다린다
    func restoreTabs(waited: Double = 0) {
        guard !restored else { return }
        let saved = UserDefaults.standard.array(forKey: "terminalTabs") as? [[String: Any]] ?? []
        let running = saved.flatMap { leaves($0["root"] as? [String: Any] ?? $0) }.contains { e in
            (e["session"] as? String).flatMap(sessionRecord).flatMap { ($0["pid"] as? NSNumber)?.int32Value }.map(isLiveClaude) ?? false
        }
        if running, waited < 5 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { self.restoreTabs(waited: waited + 0.25) }
            return
        }
        var pick: UUID?
        var lasts: [BoardTab] = []
        for e in saved {
            var focused: BoardSurface?
            guard let (root, dir) = restoreNode(e["root"] as? [String: Any] ?? e, focused: &focused) else { continue }
            let tab = BoardTab(root, focused: focused, workspace: e["workspace"] as? String ?? dir)
            tab.customName = e["name"] as? String
            add(tab)
            if e["selected"] != nil { pick = tab.id }
            if e["last"] != nil { lasts.append(tab) }
        }
        restored = true
        // 탭을 열 때마다 고른 것은 지우고 남겨 둔 표시로 — 표시가 없는 경로(3.0.x 목록)는 첫 탭
        lastTab = [:]
        for t in lasts { lastTab[t.workspace] = t.id }
        if let id = pick ?? selected { select(id) }
        saveTabs()
    }

    // 남겨 둔 화면 트리의 잎(화면 값)들
    private func leaves(_ n: [String: Any]) -> [[String: Any]] {
        guard let a = n["a"] as? [String: Any], let b = n["b"] as? [String: Any] else { return [n] }
        return leaves(a) + leaves(b)
    }

    // 남겨 둔 화면 트리를 다시 만든다(3.0.x 목록의 탭 항목은 잎 하나) — 셸 화면은 그 경로(없으면 홈)에서 셸, claude 화면은 셸에 이어 열
    // 명령을 넣는다(restoreCommand). 되살리지 않는 화면은 빼고 형제가 그 자리를 차지한다. 돌려주는 경로는 첫 화면을 연 폴더 —
    // 작업 경로를 남기지 않은 3.0.x 목록에서는 이것이 탭의 경로다(명령 탭은 restoreCommand 의 경로, 셸 탭은 cwd 또는 홈)
    private func restoreNode(_ n: [String: Any], focused: inout BoardSurface?) -> (SplitNode, String)? {
        if let a = n["a"] as? [String: Any], let b = n["b"] as? [String: Any] {
            let ra = restoreNode(a, focused: &focused), rb = restoreNode(b, focused: &focused)
            guard let ra, let rb else { return ra ?? rb }
            return (SplitNode(right: n["split"] as? String != "down", ratio: CGFloat(n["ratio"] as? Double ?? 0.5), ra.0, rb.0), ra.1)
        }
        let e = n.compactMapValues { $0 as? String }
        guard let cwd = e["cwd"] else { return nil }
        let s: BoardSurface, dir: String
        if e["command"] != nil {
            let off = e["mdoff"] != nil || isMdRenderOff(e["session"])
            guard let (_, id, d) = restoreCommand(e, off: off) else { return nil }
            dir = d
            s = makeSurface(in: dir)
            s.held = e.filter { ["cwd", "command", "mdoff", "session", "resume"].contains($0.key) }
            s.sessionId = e["session"] ?? id   // 끝난 세션 줄을 누르면 이 화면에서 이어 연다(resumeSession)
        } else {
            dir = FileManager.default.fileExists(atPath: cwd) ? cwd : NSHomeDirectory()
            s = makeSurface(in: dir)
        }
        if e["focused"] != nil { focused = s }
        return (SplitNode(s), dir)
    }

    // 되살린 뒤 아직 들어가지 않은 화면의 claude 를 이어 연다 — 그 탭을 고를 때(select). 명령은 그때 다시 고른다:
    // 그사이 다른 터미널에서 그 세션을 이어 열었으면 restoreCommand 가 nil 이라 셸로 둔다
    private func resumeHeld(_ s: BoardSurface) {
        guard let e = s.held else { return }
        s.held = nil
        let off = e["mdoff"] != nil || isMdRenderOff(e["session"])
        if let (cmd, id, _) = restoreCommand(e, off: off) { run(cmd, in: s, sessionId: id, mdRenderOff: off) } else { saveTabs() }
    }

    // 되살릴 명령 화면의 (명령, 세션 id, 작업 경로). 대화가 남은 세션은 세션 파일의 resume_cmd(파일이 지워졌으면 남겨 둔 것),
    // 대화 없이 끝난 세션(대화 기록 없음 — 보드가 세션 파일도 지운다)은 처음 명령을 새 세션 id 로.
    // 세션 파일의 pid 가 아직 claude 로 살아 있으면(/bg·nohup 으로 떨어져 나갔거나 다른 터미널에서 이어 연 세션) 이어 열지 않는다 — nil.
    // 답변 꾸미기는 off(끈 탭·끈 세션)를 따라 mdRenderCommand 로 고친다 — 세션 파일이 없으면 남겨 둔 명령·resume_cmd 문자열로
    private func restoreCommand(_ e: [String: String], off: Bool) -> (String, String?, String)? {
        guard let command = e["command"].map({ mdRenderCommand($0, off: off) }), let cwd = e["cwd"] else { return nil }
        guard let id = e["session"] else {
            let (c, i) = freshSessionCommand(command)
            return FileManager.default.fileExists(atPath: cwd) ? (c, i, cwd) : nil
        }
        let rec = sessionRecord(id)
        if let pid = (rec?["pid"] as? NSNumber)?.int32Value, isLiveClaude(pid) {
            boardLog("탭을 되살리지 않음: 세션 \(id) 이 보드 밖(pid \(pid))에서 돌고 있습니다")
            return nil
        }
        let dir = rec?["project_dir"] as? String ?? rec?["cwd"] as? String ?? cwd   // 보드 카드와 같은 경로(워크트리에 들어간 세션도 프로젝트 폴더에서)
        guard FileManager.default.fileExists(atPath: dir) else {
            boardLog("탭을 되살리지 않음: 작업 경로가 없습니다 \(dir)")
            return nil
        }
        if FileManager.default.fileExists(atPath: transcriptPath(rec?["transcript_path"] as? String, dir, id)) {
            let resume = (rec?["resume_cmd"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? e["resume"] ?? "claude --resume \(id)"
            return (mdRenderCommand(resume, args: rec?["args"] as? [String], off: off), id, dir)
        }
        let (c, i) = freshSessionCommand(command)
        return (c, i, dir)
    }

    // 새 세션 — claude 면 보드가 세션 id 를 붙여 미리 안다. 답변 꾸미기는 새 세션 창의 체크(경로 카드 "+" 는 늘 켬) — 끄면 그 세션 id 를 기억한다
    func startSession(_ command: String, in dir: String, mdRender: Bool = true, warned: Bool = false) {
        // 원래 작업 트리로 자율주행이 도는 경로면 먼저 경고한다(막지는 않는다) — 경로 카드 "+"·새 세션 창 모두 여기로 온다
        if !warned, let r = AutopilotStore.shared.inPlaceRun(covering: dir) {
            warnAutopilotPath(r, dir: dir) { self.startSession(command, in: dir, mdRender: mdRender, warned: true) }
            return
        }
        let (cmd, id) = freshSessionCommand(mdRenderCommand(command, off: !mdRender))
        launch(cmd, in: dir, sessionId: id, mdRenderOff: !mdRender)
    }

    // "+ 새 세션" 창 — 작업 폴더는 보드에 있는 경로(지금 경로를 먼저 고른다) + 다른 폴더
    func newSessionSheet() {
        if !expanded { setTerminalExpanded(true) }
        showBoard()
        whenPaneSettled { self.beginNewSessionSheet() }
    }

    private func beginNewSessionSheet() {
        guard window.attachedSheet == nil else { return }   // 이미 새 세션 창(또는 다른 시트)이 떠 있으면 겹쳐 띄우지 않는다(⌘T 를 또 누름)
        let paths = (store?.groups.map(\.id) ?? []).filter { FileManager.default.fileExists(atPath: $0) }
        // 지금 경로와 같은 보드 경로(창의 줄은 보드 경로 글자 그대로 견준다). 보드에 없으면 첫 경로
        let current = workspace.flatMap { ws in paths.first { workspaceKey($0) == ws } }
        let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 460, height: 420), styleMask: [.titled], backing: .buffered, defer: false)
        let end = { window.endSheet(sheet) }
        sheet.contentViewController = NSHostingController(rootView: NewSessionSheet(
            paths: paths, initial: current ?? paths.first ?? NSHomeDirectory(),
            cancel: end, start: { cmd, dir, md in end(); self.startSession(cmd, in: dir, mdRender: md) }))
        window.beginSheet(sheet)
    }

    // 탭을 보이게 한다 — 접혀 있거나 미니면 펼치고, 그 탭을 고르고, 보드를 앞으로 꺼내 터미널에 포커스.
    // id 가 화면이면(세션 줄 클릭 — place 의 화면) 그 탭의 그 화면을 고른다(크게 보는 중이면 푼다)
    func reveal(_ id: UUID) {
        if !expanded { setTerminalExpanded(true) }
        if let s = surface(id), let t = s.tab {
            if t.focused !== s { t.zoomed = false; t.focused = s }
            select(t.id)
        } else {
            select(id)
        }
        showBoard()
        focusSelected()
    }

    func toast(_ text: String) {
        withAnimation(.easeOut(duration: 0.2)) { notice = text }
        noticeHide?.cancel()
        let w = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.3)) { self?.notice = nil } }
        noticeHide = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 6, execute: w)
    }

    func dismissNotice() {
        noticeHide?.cancel()
        withAnimation(.easeOut(duration: 0.2)) { notice = nil }
    }

    // 세션이 어디서 도는가 — 보드 탭의 화면(그 안에서 시작된 claude -p 포함)·다른 터미널·백그라운드·끝남.
    // 끝난 세션은 그 화면에서 claude 가 막 뜨는 중일 때만 탭 소속(이어 열기를 넣은 직후) — claude 가 끝나 셸만 남은 화면이면 끝남
    func place(_ s: Session) -> SessionPlace {
        if let id = paired[s.id] ?? surfaces.first(where: { $0.sessionId == s.id && (s.state != "ended" || $0.runsClaude) })?.id {
            return .tab(id, headless: tabOwnedHeadless.contains(s.id))
        }
        if s.state == "ended" { return .ended }
        return s.headless ? .background : .terminal
    }

    // 세션 목록을 다시 읽을 때마다 탭의 화면들과 짝짓는다. tty 가 같으면 그 화면의 세션, headless 세션은 조상에 화면의 claude 가 있으면
    // 그 화면 소속(화면이 닫히면 함께 끝난다 — Claude 가 Bash 도구로 띄운 claude -p 등). 끝난 세션은 같은 세션 id 로 연 화면만
    func pair(_ sessions: [Session]) {
        let all = surfaces
        var byTty: [String: BoardSurface] = [:]
        for t in all where !t.tty.isEmpty { byTty[t.tty] = t }
        var map: [String: UUID] = [:]
        // 화면의 맨 위 프로세스(엔진이 띄운 login — 그 tty 의 세션 리더). 셸 화면에서 사람이 띄운 claude -p 도 그 아래에 있다
        var tabPids: [Int32: BoardSurface] = [:]
        for t in all { if let fg = t.view.foregroundPid, fg > 0, getsid(fg) > 0 { tabPids[getsid(fg)] = t } }
        var main: [UUID: Session] = [:]
        for s in sessions where s.state != "ended" && !s.tty.isEmpty {
            guard let t = byTty[s.tty] else { continue }
            map[s.id] = t.id
            main[t.id] = s
            // /clear 등으로 id 가 바뀌었거나 셸에서 사람이 띄운 claude — 끝난 뒤에도 그 세션이 돌던 화면으로 찾는다(resumeSession)
            if t.sessionId != s.id { t.sessionId = s.id }
        }
        var owned: Set<String> = []
        for s in sessions where s.state != "ended" && s.headless && s.pid > 0 {
            if let t = tabAncestor(s.pid, tabPids) { map[s.id] = t.id; owned.insert(s.id) }
        }
        for t in all {
            let running = main[t.id] != nil || t.runsClaude
            // 셸 프롬프트에서 사람이 친 대화형 claude 도 claude 화면으로 — 쥔 목록에 세션이 남아 다시 뜰 때 이어 연다(-p·--bg 는 빼고)
            if t.command == nil, let s = main[t.id], mode(s) == nil { t.command = "claude" }
            // 보드가 넣은 claude 가 끝나 셸만 남았다 — 셸 화면으로(쥔 목록·상태 점). 끈 꾸미기는 끈 세션 기억에 이미 남아 있다
            if !running, t.command != nil { t.command = nil; t.mdRenderOff = false }
            let s = main[t.id] ?? (running ? t.sessionId.flatMap { id in sessions.first { $0.id == id } } : nil)
            if t.session != s { t.session = s }
            if t.mdRenderOff { rememberMdRenderOff(s?.id) }   // 끈 화면에서 세션 id 가 바뀌어도(/clear 등) 끈 채로 이어 열게
        }
        if map != paired { paired = map }
        if owned != tabOwnedHeadless { tabOwnedHeadless = owned }
        // 경로 줄의 상태 점 — 그 경로 탭들의 모든 화면에서 승인 대기 > 작업 중 > 그 밖(세션이 없으면 대기)
        var states: [String: String] = [:]
        for ws in workspaces { states[ws] = tabs(in: ws).flatMap(\.surfaces).compactMap(\.session).min { $0.rank < $1.rank }?.state ?? "idle" }
        if states != workspaceStates { workspaceStates = states }
        saveTabs()   // 세션 id·resume_cmd·셸 경로·고른 탭이 바뀌었으면 남긴다
        // 다른 터미널 세션의 앱 이름 — 조상 프로세스로만(AppleScript 를 쓰는 ownerApp 은 부르지 않는다)
        var names = appNames
        for s in sessions where s.state != "ended" && !s.headless && map[s.id] == nil && names[s.id] == nil && s.pid > 0 {
            if let id = ancestorApp(s.pid) {
                names[s.id] = terminalApps.first { $0.id == id }?.name
                    ?? NSRunningApplication.runningApplications(withBundleIdentifier: id).first?.localizedName ?? "터미널"
            }
        }
        if names != appNames { appNames = names }
        reservedNames = reservedNames.filter { Date().timeIntervalSince($0.value.at) < 15 }
        applyRenames()   // 승인 대기를 벗어났거나 첫 요청이 들어간 세션의 미뤄 둔 이름 바꾸기
    }

    // 세션의 실행 방식 — 프로세스 인자에서(-p·--print → -p, --bg·--background → --bg, 대화형이면 nil). 끝난 세션은 읽을 수 없다
    func mode(_ s: Session) -> String? {
        if let m = modes[s.pid] { return m.isEmpty ? nil : m }
        let args = processArgs(s.pid)
        // --bg-pty-host — 탭에서 /bg 로 보낸 세션을 데몬 아래에서 이어 가는 프로세스(Claude Code 2.1.302 실측)
        let m = args.contains { $0 == "--bg" || $0 == "--background" || $0 == "--bg-pty-host" } ? "--bg"
            : args.contains { $0 == "-p" || $0 == "--print" } ? "-p" : ""
        if s.pid > 0, !args.isEmpty { modes[s.pid] = m }
        return m.isEmpty ? nil : m
    }

    // 탭 고르기 — 그 탭의 경로가 지금 경로가 된다(세션 줄 클릭·이어 열기·새 세션도 이 길로 그 경로에 간다). 경로마다 마지막 탭을 기억한다
    func select(_ id: UUID) {
        selected = id
        if let t = tabs.first(where: { $0.id == id }) {
            lastTab[t.workspace] = id
            if workspace != t.workspace { workspace = t.workspace }
            if restored { t.surfaces.forEach(resumeHeld) }   // 되살리는 동안 탭마다 고르는 것은 들어간 것이 아니다
        }
        applyVisibility()
        focusSelected()
    }

    // 고른 탭의 화면만(크게 보는 중이면 고른 화면만) 그리고 나머지는 실제 크기 그대로 숨긴다. 접혀 있거나 창이 안 보이면(가려짐·최소화·
    // 닫힘·다른 데스크톱) 모두 그리지 않는다. 나눈 탭에서 고르지 않은 화면은 Ghostty 처럼 흐리게(unfocused-split-opacity)
    func applyVisibility() {
        // 접는 애니메이션 동안에는 터미널이 밀려 나가는 모습이 보이도록 그린다
        let drawing = (expanded || slide != nil) && window.occlusionState.contains(.visible)
        for t in tabs {
            let dim = t.surfaces.count > 1 && !t.zoomed
            for s in t.surfaces {
                let shown = t.id == selected && (!t.zoomed || s === t.focused)
                s.view.isHidden = !shown
                s.view.setSurfaceVisible(shown && drawing)
                let alpha = dim && s !== t.focused ? unfocusedSplitOpacity : 1
                if s.view.alphaValue != alpha { s.view.alphaValue = alpha }
            }
        }
        area?.empty.isHidden = !tabs.isEmpty
        area?.workspaceBarShown = !tabs.isEmpty
        layoutSurfaces()
    }

    // 화면 자리 — 탭마다 트리대로 터미널 칸을 나눈다(보이지 않는 탭도 실제 크기 그대로 둔다). 크게 보는 탭은 고른 화면이 칸 전체.
    // 고른 탭의 경계는 끌 수 있는 뷰로 화면들 위에 둔다. 칸 크기가 바뀔 때·트리나 고른 탭이 바뀔 때 부른다
    func layoutSurfaces() {
        guard let area else { return }
        let bounds = area.container.bounds
        var shown: [(SplitNode, NSRect, NSRect)] = []
        for t in tabs {
            var frames: [(BoardSurface, NSRect)] = [], lines: [(SplitNode, NSRect, NSRect)] = []
            t.root.layout(in: bounds, &frames, &lines)
            for (s, r) in frames {
                let f = t.zoomed && s === t.focused ? bounds : r
                if s.view.frame != f { s.view.frame = f }   // 같은 크기를 다시 넣어도 엔진이 크기를 다시 맞춘다
            }
            if t.id == selected, !t.zoomed { shown = lines }
        }
        area.container.showDividers(shown, color: dividerColor)
    }

    // 이름을 고치는 탭이면 입력 칸에서 포커스를 뺏지 않는다(그 탭을 눌렀을 때, 펼치기 애니메이션이 끝났을 때)
    func focusSelected() {
        guard expanded, let t = selectedTab, !t.renaming else { return }
        window.makeFirstResponder(t.focused.view)
    }

    var selectedTab: BoardTab? { tabs.first { $0.id == selected } }
    var surfaces: [BoardSurface] { tabs.flatMap(\.surfaces) }
    func surface(_ id: UUID) -> BoardSurface? { surfaces.first { $0.id == id } }

    // 화면이 키보드 초점을 받았다(누름·⌘[ 등) — 그 탭의 고른 화면으로. 크게 보기는 여기서 풀지 않는다(화면을 옮기는 쪽이 푼다)
    func surfaceFocused(_ s: BoardSurface) {
        guard let t = s.tab, t.focused !== s, tabs.contains(where: { $0 === t }) else { return }
        t.focused = s
        applyVisibility()
        saveTabs()
    }

    // 화면 나누기 키(윈도우 > 화면 나누기)는 터미널을 보이며 한다 — 접혀 있으면 펼치고 보드를 앞으로
    private func showTerminal() {
        if !expanded { setTerminalExpanded(true) }
        showBoard()
    }

    // ⌘D·⇧⌘D — 고른 화면을 둘로 나눠 뒤쪽(오른쪽·아래)에 같은 폴더의 셸을 연다(Ghostty 처럼 50:50, 새 화면에 포커스, 크게 보기는 풀린다).
    // 폴더는 고른 화면 셸의 지금 경로(OSC 7), 그 폴더가 없으면 탭의 경로. claude 는 띄우지 않는다
    func split(right: Bool) {
        guard let t = selectedTab, let node = t.root.leaf(t.focused) else { return }
        showTerminal()
        let cwd = t.focused.cwd
        let s = makeSurface(in: FileManager.default.fileExists(atPath: cwd) ? cwd : t.workspace)
        node.split(s, right: right)
        t.watch(s)
        t.zoomed = false
        t.focused = s
        layoutSurfaces()
        attach(s)
        applyVisibility()
        focusSelected()
        saveTabs()
    }

    // ⌘[·⌘] — 트리 순서(왼쪽·위가 앞)로 앞·뒤 화면(끝에서는 반대쪽 끝으로)
    func focusSurface(step: Int) {
        guard let t = selectedTab, t.surfaces.count > 1 else { return }
        let list = t.surfaces, i = list.firstIndex { $0 === t.focused } ?? 0
        moveFocus(t, to: list[(i + step + list.count) % list.count])
    }

    // ⌥⌘방향키 — 그 방향에 있는 화면 가운데 가장 가까운 것(두 화면의 왼쪽 위 모서리 사이 거리 — Ghostty 와 같은 방식). 없으면 그대로
    func focusSurface(_ d: SplitDirection) {
        guard let t = selectedTab, let area else { return }
        var frames: [(BoardSurface, NSRect)] = [], lines: [(SplitNode, NSRect, NSRect)] = []
        t.root.layout(in: area.container.bounds, &frames, &lines)
        guard let me = frames.first(where: { $0.0 === t.focused })?.1 else { return }
        let found = frames.filter { f in
            switch d {
            case .left: return f.1.maxX <= me.minX
            case .right: return f.1.minX >= me.maxX
            case .up: return f.1.minY >= me.maxY
            case .down: return f.1.maxY <= me.minY
            }
        }.min { hypot($0.1.minX - me.minX, $0.1.maxY - me.maxY) < hypot($1.1.minX - me.minX, $1.1.maxY - me.maxY) }
        if let found { moveFocus(t, to: found.0) }
    }

    // 다른 화면으로 — 크게 보기는 풀린다(Ghostty 와 같게)
    private func moveFocus(_ t: BoardTab, to s: BoardSurface) {
        showTerminal()
        t.zoomed = false
        t.focused = s
        applyVisibility()
        focusSelected()
        saveTabs()
    }

    // ⌃⌘방향키 — 고른 화면에서 가장 가까운, 그 방향으로 나눈 경계를 그 방향으로 10pt 옮긴다(Ghostty 의 resize_split:<방향>,10).
    // 크게 보는 중이면 그대로
    func resizeSplit(_ d: SplitDirection) {
        guard let t = selectedTab, !t.zoomed, let area else { return }
        let across = d == .left || d == .right
        var node = t.root.leaf(t.focused)?.parent
        while let n = node, n.right != across { node = n.parent }
        var frames: [(BoardSurface, NSRect)] = [], lines: [(SplitNode, NSRect, NSRect)] = []
        t.root.layout(in: area.container.bounds, &frames, &lines)
        guard let node, let r = lines.first(where: { $0.0 === node })?.1 else { return }
        let length = across ? r.width : r.height
        node.setRatio(node.ratio + (d == .left || d == .up ? -10 : 10) / max(length, 1), length: length)
        showTerminal()
        layoutSurfaces()
        saveTabs()
    }

    // ⌃⌘= — 고른 탭의 나눈 크기를 고르게(같은 방향으로 나눈 화면 수대로)
    func equalizeSplits() {
        guard let t = selectedTab, t.surfaces.count > 1 else { return }
        t.root.equalize()
        showTerminal()
        layoutSurfaces()
        saveTabs()
    }

    // ⇧⌘↩ — 고른 화면 하나 크게 보기 켜고 끄기(화면이 둘 이상일 때만). 크게 보는 동안 탭 줄에 표시
    func toggleZoom() {
        guard let t = selectedTab, t.surfaces.count > 1 else { return }
        showTerminal()
        t.zoomed.toggle()
        applyVisibility()
        focusSelected()
    }

    // ⌃⌥←·⌃⌥→(Ghostty 의 탭 키 ⇧⌘[·⇧⌘]·⌃⇧⇥·⌃⇥ 도) — 탭 줄에서 왼쪽·오른쪽 탭으로(지금 경로 안에서, 끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다)
    func stepTab(_ step: Int) {
        let list = tabs(in: workspace)
        guard !list.isEmpty else { return }
        let i = list.firstIndex { $0.id == selected } ?? 0
        reveal(list[(i + step + list.count) % list.count].id)
    }

    // ⇧⌘R — 고른 탭의 이름 자리를 입력 칸으로 바꾼다. 접혀 있으면 펼친 뒤(⌘1~⌘9 의 reveal 과 같이)
    func beginRename() {
        guard let t = tabs.first(where: { $0.id == selected }) else { return }
        if !expanded { setTerminalExpanded(true) }
        showBoard()
        t.renaming = true
    }

    // 이름 바꾸기 끝 — name 이 nil 이면 취소(Esc). 비우거나 폴더 이름과 같게 저장하면 사용자 이름을 지워 폴더 이름(자동)으로 돌아간다.
    // 입력 칸의 처음 값(탭 줄에 보이던 이름)을 고치지 않고 저장하면 그대로 둔다 — 번호가 붙은 세션 이름이 사용자 이름으로 남지 않게.
    // 바꾸면 그 탭의 claude 세션 이름도 바꾼다(renameSessions). 저장·취소 모두 터미널에 포커스를 돌려준다
    func endRename(_ tab: BoardTab, name: String?) {
        guard tab.renaming else { return }
        tab.renaming = false
        if let name {
            let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
            if n != tab.shownName {
                tab.customName = n.isEmpty || n == tab.folderName ? nil : n
                saveTabs()
                renameSessions(tab)
            }
        }
        focusSelected()
    }

    // ⌘ 를 누름·놓음(flagsChanged 마다 오므로 ⌘ 가 바뀔 때만 본다) — 누르는 즉시 보이고(⌘C 를 칠 때도 잠깐 보인다, 사용자 결정), 놓거나 앱이 비활성이 되면 끈다
    func commandFlag(_ down: Bool) {
        guard down != commandHeld else { return }
        commandHeld = down
    }

    // 탭의 × — 화면이 하나면 ⌘W 와 같은 엔진 동작으로 보내 셸 프롬프트처럼 물을 필요가 없으면 바로, 프로그램이 돌고 있으면 물어서 닫는다.
    // 나눈 탭은 탭째 닫는다 — 셸 프롬프트가 아닌(프로그램이 도는) 화면이 하나라도 있으면 한 번 묻는다
    func requestClose(_ tab: BoardTab) {
        if let s = tab.root.surface {
            if !s.view.performBindingAction("close_surface") { close(s, confirm: false) }
            return
        }
        let busy = tab.surfaces.filter { !$0.atPrompt }.count
        guard busy > 0 else { return remove(tab) }
        confirmClose("탭을 닫을까요?", "\(tab.name) 탭의 화면 \(busy)개에서 도는 프로그램이 함께 끝납니다.") { self.remove(tab) }
    }

    // 화면 닫기(셸이 끝남·⌘W) — 프로그램이 아직 돌고 있으면 먼저 묻는다. 탭의 마지막 화면이면 탭을 닫는다
    func close(_ s: BoardSurface, confirm: Bool) {
        guard let t = s.tab, tabs.contains(where: { $0 === t }) else { return }
        guard confirm else { remove(s); return }
        if t.surfaces.count == 1 {
            confirmClose("탭을 닫을까요?", "\(t.shownName) 탭에서 도는 프로그램이 함께 끝납니다.") { self.remove(s) }
        } else {
            confirmClose("화면을 닫을까요?", "\(t.name) 탭의 이 화면에서 도는 프로그램이 함께 끝납니다.") { self.remove(s) }
        }
    }

    private func confirmClose(_ title: String, _ text: String, then run: @escaping @MainActor () -> Void) {
        let a = NSAlert()
        a.messageText = title
        a.informativeText = text
        a.addButton(withTitle: "닫기")
        a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"
        whenPaneSettled { a.beginSheetModal(for: window) { if $0 == .alertFirstButtonReturn { run() } } }
    }

    // 화면 하나를 뺀다 — 탭의 마지막 화면이면 탭째. 고른 화면이었으면 옆 화면(형제 쪽에서 가장 가까운 것)을 고르고 크게 보기는 풀린다
    private func remove(_ s: BoardSurface) {
        guard let t = s.tab, let node = t.root.leaf(s) else { return }
        guard let next = node.remove() else { return remove(t) }
        t.unwatch(s)
        // 엔진 surface 를 내려 셸·프로그램을 끝낸다
        s.view.controller = nil
        s.view.removeFromSuperview()
        if t.focused === s { t.zoomed = false; t.focused = next }
        t.objectWillChange.send()
        applyVisibility()
        if t.id == selected { focusSelected() }
        saveTabs()
    }

    private func remove(_ tab: BoardTab) {
        guard let i = tabs.firstIndex(where: { $0 === tab }) else { return }
        let ws = tab.workspace
        let at = tabs(in: ws).firstIndex { $0 === tab } ?? 0   // 경로 안에서의 자리
        let order = workspaces                                 // 닫기 전 경로 줄
        tabs.remove(at: i)
        // 엔진 surface 를 내려 셸·프로그램을 끝낸다 — 창에서 빼는 것만으로는 뷰가 바로 풀리지 않아 셸이 남는다(실측)
        for s in tab.surfaces {
            s.view.controller = nil
            s.view.removeFromSuperview()
        }
        if lastTab[ws] == tab.id { lastTab[ws] = nil }
        if selected == tab.id {
            // 같은 경로의 옆 탭 → 그 경로에 탭이 없으면 경로 줄에서 옆 경로(그 경로의 마지막 탭) → 다른 경로도 없으면 빈 화면
            let rest = tabs(in: ws), others = order.filter { $0 != ws }
            let next = !rest.isEmpty ? rest[min(at, rest.count - 1)].id
                : others.isEmpty ? nil : shownTab(others[min(order.firstIndex(of: ws) ?? 0, others.count - 1)])
            if let next { select(next) } else { selected = nil; workspace = nil }
        }
        applyVisibility()
        focusSelected()
        saveTabs()
    }
}

// 세션이 도는 곳 — 칩·클릭 흐름·카드 구역이 쓴다
enum SessionPlace: Equatable {
    case tab(UUID, headless: Bool)   // 보드 탭의 화면(headless = 그 화면 안에서 시작된 claude -p)
    case terminal                    // 다른 터미널 앱
    case background                  // claude -p·--bg 등 터미널 없이 도는 세션
    case ended
}

// pid 의 조상 가운데 화면의 맨 위 프로세스가 있으면 그 화면
func tabAncestor(_ pid: Int32, _ tabPids: [Int32: BoardSurface]) -> BoardSurface? {
    var p = pid
    for _ in 0..<12 where p > 1 {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, p]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        p = info.kp_eproc.e_ppid
        if let t = tabPids[p] { return t }
    }
    return nil
}

// 부모 pid — 읽지 못하면 0
func parentPid(_ pid: Int32) -> Int32 {
    var info = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.stride
    var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
    guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return 0 }
    return info.kp_eproc.e_ppid
}

// 프로세스 실행 인자(sysctl KERN_PROCARGS2 — state.py proc_args 와 같은 방식). 읽지 못하면 빈 배열
func processArgs(_ pid: Int32) -> [String] {
    guard pid > 0 else { return [] }
    var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
    var size = 1 << 18
    var buf = [UInt8](repeating: 0, count: size)
    guard sysctl(&mib, 3, &buf, &size, nil, 0) == 0, size > 4 else { return [] }
    let argc = Int(buf.withUnsafeBytes { $0.load(as: Int32.self) })
    var i = 4
    while i < size, buf[i] != 0 { i += 1 }   // 실행 파일 경로를 건너뛴다
    while i < size, buf[i] == 0 { i += 1 }
    var args: [String] = []
    while i < size, args.count < argc {
        var j = i
        while j < size, buf[j] != 0 { j += 1 }
        args.append(String(decoding: buf[i..<j], as: UTF8.self))
        i = j + 1
    }
    return args
}

// 살아 있는 claude 프로세스인가 — 끝난 뒤 pid 가 다른 프로그램에 다시 쓰였으면 아니다. 실행 파일 이름(p_comm)은 버전 번호라
// state.py find_claude 처럼 첫 인자의 이름으로 본다(좀비·남의 프로세스는 인자를 읽지 못해 아니다)
func isLiveClaude(_ pid: Int32) -> Bool {
    guard pid > 0, kill(pid, 0) == 0, let first = processArgs(pid).first else { return false }
    return (first as NSString).lastPathComponent == "claude"
}

// 세션 파일(sessions/<id>.json)을 그대로 읽는다 — 보드 목록(Store)은 숨긴 경로를 빼고 상태를 고쳐 보이므로
func sessionRecord(_ id: String) -> [String: Any]? {
    guard let data = FileManager.default.contents(atPath: (sessionsDir as NSString).appendingPathComponent("\(id).json")) else { return nil }
    return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
}

// 업데이트·종료로 함께 꺼지는 보드 탭 세션 가운데 일하는 중인 것과 그 까닭 — 작업 중·승인 대기, 서브에이전트가 도는 세션,
// Bash 백그라운드 셸이 도는 세션(보드 목록은 대기일 때만 세지만 여기서는 상태와 상관없이 센다), 탭 안의 claude -p.
// 백그라운드·다른 터미널 세션은 꺼지지 않으므로 넣지 않는다
@MainActor func busyTabSessions() -> [(Session, String)] {
    let hub = TerminalHub.shared
    let shells = backgroundShells()
    return (hub.store?.groups.flatMap(\.sessions) ?? []).compactMap { s in
        guard s.state != "ended", case .tab(_, let headless) = hub.place(s) else { return nil }
        var why: [String] = []
        if headless { why.append("claude -p") }
        else if s.state == "waiting" { why.append("승인 대기") }
        else if s.state == "working" { why.append("작업 중") }
        let subs = s.subagents.filter { $0.state != "done" }.count
        if subs > 0 { why.append("서브에이전트 \(subs)개") }
        if let n = shells[s.pid], n > 0 { why.append("셸 \(n)개") }
        return why.isEmpty ? nil : (s, why.joined(separator: " · "))
    }
}

// 업데이트·종료 전 확인 — 꺼지는 보드 탭 세션 가운데 일하는 중인 것이 있으면 묻고, 없으면 묻지 않는다.
// 계속을 고르면 탭 목록을 남기고 그 세션들이 도는 탭의 주 세션(사람이 입력하는 대화형 claude)에 일시중지 표지를 남긴다 —
// 탭 안의 claude -p 는 입력 자리가 없어 표지를 남기지 않고, 그것·서브에이전트·셸만 돌고 주 세션은 대기였어도 주 세션에 남긴다(주 세션이
// 시킨 일이라 이어 가기 문구로 다시 시킨다). 다시 뜰 때 이어 연 탭은 일시중지로 보이고 "세션 재개"로 이어 간다
// (state.py 는 새 요청 때만 표지를 지운다). 돌려주는 값: nil = 취소, 아니면 새로 남긴 표지 파일
@MainActor func confirmShutdown(update: Bool) -> [String]? {
    TerminalHub.shared.slide?.finish()   // 확인 창은 바로 답해야 하므로 펼치기·접기 애니메이션을 끝 상태로 마치고 묻는다
    let busy = busyTabSessions()
    guard !busy.isEmpty else { TerminalHub.shared.saveTabs(); return [] }
    let a = NSAlert()
    a.messageText = "작업 중인 세션 \(busy.count)개가 꺼집니다"
    a.informativeText = "보드가 터미널을 쥐고 있어서 \(update ? "업데이트하면" : "보드를 끄면") 진행 중인 응답과 탭에서 도는 셸·서브에이전트가 끊깁니다. "
        + "보드가 다시 뜨면 대화를 이어 엽니다. 끊긴 세션은 일시중지로 남아 \"세션 재개\"로 이어 갈 수 있습니다."
    let list = NSHostingView(rootView: ShutdownList(rows: busy))
    list.frame.size = list.fittingSize
    a.accessoryView = list
    a.addButton(withTitle: update ? "업데이트" : "종료").hasDestructiveAction = true
    a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"
    NSApp.activate(ignoringOtherApps: true)
    guard a.runModal() == .alertFirstButtonReturn else { return nil }
    TerminalHub.shared.saveTabs()
    // 창을 띄워 둔 동안 끝났거나 새로 일을 시작한 세션이 있을 수 있어 고른 순간의 것으로 다시 센다
    (NSApp.delegate as? AppDelegate)?.store.reload()
    let hub = TerminalHub.shared
    var mains: [String: Session] = [:]
    for (s, _) in busyTabSessions() {
        guard case .tab(let id, _) = hub.place(s), let main = hub.surface(id)?.session,
              main.state != "ended", hub.mode(main) == nil else { continue }   // 셸 탭에서 친 claude -p 처럼 주 세션이 -p 면 남기지 않는다
        mains[main.id] = main
    }
    var marks: [String] = []
    for s in mains.values where !FileManager.default.fileExists(atPath: pausedMark(s)) {
        let mark = ["paused_at": Date().timeIntervalSince1970, "topic": s.topic] as [String: Any]
        if let data = try? JSONSerialization.data(withJSONObject: mark), FileManager.default.createFile(atPath: pausedMark(s), contents: data) {
            marks.append(pausedMark(s))
        }
    }
    return marks
}

// 확인 창의 세션 목록(상태 색 · 경로 — 제목 · 까닭)과 영향 없는 세션 안내
struct ShutdownList: View {
    let rows: [(Session, String)]
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    HStack(spacing: 8) {
                        Circle().fill(Palette.color(row.0.state)).frame(width: 8, height: 8)
                        Text("\((row.0.cwd as NSString).lastPathComponent) · \(row.0.title)").lineLimit(1).truncationMode(.tail)
                        Spacer(minLength: 6)
                        Text(row.1).foregroundStyle(.secondary).fixedSize()
                    }
                    .font(.system(size: 12))
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.06)))
            Text("백그라운드 세션과 다른 터미널의 세션은 영향 없이 그대로 돕니다.")
                .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .frame(width: 320)
    }
}

// 새로 띄우는 claude 에 세션 id 를 붙인다(--session-id) — 보드가 그 세션을 미리 안다. claude 가 아니거나 이어 열기·이어 하기면 그대로.
// 이미 --session-id 가 있으면 새 id 로 바꾼다(되살리기가 대화 없이 끝난 세션을 다시 띄울 때)
func freshSessionCommand(_ command: String) -> (String, String?) {
    let has = { (pattern: String) in command.range(of: pattern, options: .regularExpression) != nil }
    guard has(#"^\s*(\S*/)?claude(\s|$)"#), !has(#"(^|\s)(--resume|-r|--continue|-c)(=|\s|$)"#) else { return (command, nil) }
    // 따옴표 안 공백이 상하지 않게 문자열을 쪼개지 않고 고친다
    let base = command.replacingOccurrences(of: #"\s--session-id(=|\s+)\S+"#, with: "", options: .regularExpression)
    let id = UUID().uuidString.lowercased()
    return (base + " --session-id " + id, id)
}

// 답변 마크다운 꾸미기(md-render) — 보드 탭 claude 에 --plugin-dir 로 넘기는 경로. 앱 안 Contents/Resources/plugins/md-render 를
// ~/.claude/notifier/plugins/md-render 로 옮겨 두고 그쪽을 넘긴다 — 대화형 claude 는 사용자 소유 플러그인 폴더에 tsconfig.json·
// .claude-plugin/types/ 를 써서(실측) 앱 안 경로를 넘기면 서명된 앱이 바뀐다. 앱이 싣는 파일만 내용이 다를 때 덮고 Claude Code 가 만든 것은 둔다.
// 앱 밖(.build)에서 띄워 원본이 없거나 옮기지 못하면 nil — 어떤 길에도 붙이지 않는다
let mdRenderDir: String? = {
    let fm = FileManager.default
    guard let src = Bundle.main.resourceURL?.appendingPathComponent("plugins/md-render").path, fm.fileExists(atPath: src) else { return nil }
    let dest = NSHomeDirectory() + "/.claude/notifier/plugins/md-render"
    for f in (try? fm.subpathsOfDirectory(atPath: src)) ?? [] {
        guard let data = fm.contents(atPath: "\(src)/\(f)") else { continue }   // 폴더
        let to = "\(dest)/\(f)"
        if fm.contents(atPath: to) == data { continue }
        do {
            try fm.createDirectory(atPath: (to as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            try data.write(to: URL(fileURLWithPath: to), options: .atomic)
        } catch {
            boardLog("답변 꾸미기 플러그인을 옮기지 못했습니다: \(to) \(error.localizedDescription)")
            return nil
        }
    }
    return dest
}()

// state.py 가 resume_cmd 를 만들 때 쓰는 shlex.quote 와 같은 인용 — resume_cmd 안의 토큰을 그대로 찾고, 보드가 붙이는 경로를
// 엔진이 명령을 넘기는 bash -c 에 안전하게 넘긴다
func shlexQuote(_ s: String) -> String {
    if !s.isEmpty, s.range(of: "[^A-Za-z0-9_@%+=:,./-]", options: .regularExpression) == nil { return s }
    return "'" + s.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
}

// shlex.quote 꼴 토큰 가운데 md-render 의 --plugin-dir(값이 /plugins/md-render 로 끝남) — 따옴표 없는 꼴, '…' 꼴(안의 ' 는 '"'"'),
// '--plugin-dir=…' 꼴. 앞의 공백까지 잡아 지운다
private let mdRenderTokenPattern = #"\s+(?:--plugin-dir(?:\s+|=)[A-Za-z0-9_@%+=:,./-]*/plugins/md-render|--plugin-dir\s+'[^']*(?:'"'"'[^']*)*/plugins/md-render'|'--plugin-dir=[^']*(?:'"'"'[^']*)*/plugins/md-render')(?=\s|$)"#

// 보드 탭에서 띄우거나 이어 여는 claude 의 답변 꾸미기(사용자 결정 2026-10-17) — 기본으로 켜고(Ghostty·claude -p 에서 시작해 보드로 옮기거나
// 이어 연 세션 포함), 새 세션 창에서 끄고 띄운 세션(isMdRenderOff)만 끈다. 명령에 든 md-render 의 --plugin-dir(옛 경로·옛 설치본·사용자가 적은 것)은
// 빼고, 켜져 있으면 지금 경로를 한 번 붙인다. 사용자의 다른 --plugin-dir 은 둔다. 뺄 토큰은 세션 파일의 args(state.py 가 shlex.join 해
// resume_cmd 를 만든 재료)를 알면 그 shlex.quote 꼴 그대로 찾고, 남은 것·args 를 모를 때(저장해 둔 명령·resume_cmd)는 shlex.quote 꼴 정규식으로.
// 그래도 md-render 경로가 남으면(큰따옴표 등 다른 꼴로 사용자가 적음) 더 붙이지 않는다. claude 가 아닌 명령은 그대로
func mdRenderCommand(_ command: String, args: [String]? = nil, off: Bool) -> String {
    guard command.range(of: #"^\s*(\S*/)?claude(\s|$)"#, options: .regularExpression) != nil else { return command }
    var out = command, i = 0
    let args = args ?? []
    while i < args.count {
        var token: String?
        if args[i].hasPrefix("--plugin-dir="), args[i].hasSuffix("/plugins/md-render") {
            token = shlexQuote(args[i])
        } else if args[i] == "--plugin-dir", i + 1 < args.count, args[i + 1].hasSuffix("/plugins/md-render") {
            token = "--plugin-dir " + shlexQuote(args[i + 1])
            i += 1
        }
        if let token { out = out.replacingOccurrences(of: " \(token) ", with: " ") }   // resume_cmd 는 --resume <id> 로 끝나 인자 뒤에 늘 공백이 있다
        i += 1
    }
    out = out.replacingOccurrences(of: mdRenderTokenPattern, with: "", options: .regularExpression)
    guard !off, let dir = mdRenderDir, !out.contains("/plugins/md-render") else { return out }
    return out + " --plugin-dir " + shlexQuote(dir)
}

// 새 세션 창에서 꾸미기를 끄고 띄운 세션 id — 보드 설정 mdRenderOff. 그 세션은 이어 열 때도 끈 채로 연다. 끈 탭의 세션 id 가 바뀌면
// (/clear, 되살리기가 대화 없는 세션을 새 id 로 띄움) 그 id 도 더한다(launch·pair). 보드가 시작할 때 세션 파일이 없어진 id 는 치운다
func isMdRenderOff(_ id: String?) -> Bool {
    guard let id else { return false }
    return UserDefaults.standard.stringArray(forKey: "mdRenderOff")?.contains(id) ?? false
}

func rememberMdRenderOff(_ id: String?) {
    guard let id, !isMdRenderOff(id) else { return }
    UserDefaults.standard.set((UserDefaults.standard.stringArray(forKey: "mdRenderOff") ?? []) + [id], forKey: "mdRenderOff")
}

func pruneMdRenderOff() {
    let list = UserDefaults.standard.stringArray(forKey: "mdRenderOff") ?? []
    let kept = list.filter { FileManager.default.fileExists(atPath: (sessionsDir as NSString).appendingPathComponent("\($0).json")) }
    if kept.count != list.count { UserDefaults.standard.set(kept, forKey: "mdRenderOff") }
}

// 펼쳤을 때 보드 폭 — 창이 좁으면 터미널 최소 폭을 남기고 줄인다. 펼치기·접기 애니메이션 중에는 그 애니메이션의 보드 폭
@MainActor func boardPaneWidth() -> CGFloat {
    if let slide = TerminalHub.shared.slide { return slide.board }
    guard TerminalHub.shared.expanded else { return window.frame.width }
    return max(boardMinWidth, min(TerminalHub.shared.boardWidth, window.frame.width - paneDividerWidth - terminalMinWidth))
}

let terminalExpandedMinSize = NSSize(width: boardMinWidth + paneDividerWidth + terminalMinWidth, height: 300)

// 펼치기: 창을 화면의 쓸 수 있는 영역 전체로. 접기: 보드만 화면 한쪽(보드 위치 — 오른쪽·왼쪽)에 끈 폭으로.
// 펼쳐 화면 전체를 덮은 동안에는 "위에 고정"·투명도를 쓰지 않는다 — 화면 전체를 덮은 창이 늘 위에 있으면 다른 앱을 쓸 수 없다(expandedToFullScreen).
// animated — 메뉴의 "펼치기·접기 애니메이션"(보드 설정 terminalAnimation, 기본 끔). 처음 크기로 붙이기·최대축소는 접은 뒤
// 제 애니메이션을 바로 이어 돌리므로 false 로 부른다. 도는 애니메이션이 있으면 끝 상태로 마친 뒤 새 요청을 받는다
@MainActor func setTerminalExpanded(_ on: Bool, animated: Bool = UserDefaults.standard.bool(forKey: "terminalAnimation")) {
    let hub = TerminalHub.shared
    hub.slide?.finish()
    guard hub.expanded != on, let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    let target = on ? vf : NSRect(x: hub.boardOnLeft ? vf.minX : vf.maxX - hub.boardWidth, y: vf.minY, width: hub.boardWidth, height: vf.height)
    // 시트가 붙어 있으면 애니메이션 없이 — 시트가 붙은 창은 프레임이 멈췄다 건너뛴다
    guard animated, window.isVisible, window.attachedSheet == nil, let root = window.contentView as? BoardRootView else {
        hub.setExpanded(on)
        if !on { window.makeFirstResponder(nil) }   // 숨는 터미널에 키 입력이 가지 않게 포커스를 거둔다
        window.contentMinSize = on ? terminalExpandedMinSize : NSSize(width: miniWidth, height: miniMinHeight)
        window.setFrame(target, display: true)
        if !on { saveRestoreFrame() }
        settleTerminalPane()
        return
    }
    // 애니메이션 — 터미널 영역은 펼친 크기로 고정하고 자리만 옮긴다(펼칠 때 크기가 다르면 시작 전에 한 번만 맞춘다)
    let slide: PaneSlide
    if on {
        let bw = max(boardMinWidth, min(hub.boardWidth, vf.width - paneDividerWidth - terminalMinWidth))   // 펼친 뒤 boardPaneWidth
        slide = PaneSlide(from: window.frame, to: target, board: (window.frame.width, bw),
                          terminal: NSSize(width: vf.width - bw - paneDividerWidth, height: vf.height)) { settleTerminalPane() }
        hub.slide = slide
        hub.setExpanded(true)
        applyPin(UserDefaults.standard.bool(forKey: "pinned"))   // 위에 고정·투명도는 펼치기 시작할 때 바로 내린다
        WindowOpacity.shared.apply()
        hub.focusSelected()
    } else {
        window.makeFirstResponder(nil)
        slide = PaneSlide(from: window.frame, to: target, board: (boardPaneWidth(), hub.boardWidth), terminal: root.terminal.frame.size) {
            saveRestoreFrame()
            settleTerminalPane()
        }
        hub.slide = slide
        hub.setExpanded(false)
    }
    window.contentMinSize = NSSize(width: miniWidth, height: miniMinHeight)   // 중간 크기가 최소 크기에 막히지 않게 — 끝나면 맞춘다
    root.needsLayout = true
    root.layoutSubtreeIfNeeded()
    hub.applyVisibility()
    slide.start()
}

// 펼치기·접기가 끝난 뒤 — 최소 크기·위에 고정·투명도·크기 단계·터미널 그리기·포커스를 펼침 상태에 맞춘다
@MainActor func settleTerminalPane() {
    let hub = TerminalHub.shared
    window.contentMinSize = hub.expanded ? terminalExpandedMinSize : NSSize(width: miniWidth, height: miniMinHeight)
    applyPin(UserDefaults.standard.bool(forKey: "pinned"))
    window.contentView?.needsLayout = true
    BoardFit.shared.update()
    WindowOpacity.shared.apply()
    hub.applyVisibility()
    hub.focusSelected()
}

// 펼치기·접기 애니메이션 — 창 프레임을 화면 주사율에 맞춰 옮긴다(0.55초, ease-in-out). 그동안 터미널 영역은 크기를 그대로 두고
// 자리만 옮겨(BoardRootView.place) pty 크기가 바뀌지 않는다 — 프레임마다 바뀌면 안의 claude 화면이 그때마다 다시 그려진다
@MainActor final class PaneSlide: NSObject {
    static let duration: CFTimeInterval = 0.55
    let terminal: NSSize               // 터미널 영역 크기 — 애니메이션 동안 그대로
    private(set) var board: CGFloat    // 지금 보드 폭 — 처음 폭에서 끝 폭으로(보통 같다. 미니·손으로 줄인 창에서 펼칠 때만 다르다)
    private let from: NSRect, to: NSRect, boardFrom: CGFloat, boardTo: CGFloat
    private var started: CFTimeInterval = 0
    private var link: CADisplayLink?
    private var done: (() -> Void)?
    var after: [@MainActor () -> Void] = []   // 끝난 뒤 띄울 시트·확인 창(whenPaneSettled)

    init(from: NSRect, to: NSRect, board: (CGFloat, CGFloat), terminal: NSSize, done: @escaping () -> Void) {
        self.from = from
        self.to = to
        boardFrom = board.0
        boardTo = board.1
        self.board = board.0
        self.terminal = terminal
        self.done = done
    }

    func start() {
        started = CACurrentMediaTime()
        link = window.displayLink(target: self, selector: #selector(tick(_:)))
        link?.add(to: .main, forMode: .common)
        // 화면이 그리기를 멈춰 링크가 돌지 않아도(잠자기 등) 끝 상태로 마친다
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.duration + 0.5) { [weak self] in self?.finish() }
    }

    @objc private func tick(_ link: CADisplayLink) {
        let t = min(1, (CACurrentMediaTime() - started) / Self.duration)
        guard t < 1 else { return finish() }
        let e = CGFloat(t < 0.5 ? 4 * t * t * t : 1 - pow(-2 * t + 2, 3) / 2)   // ease-in-out(3차)
        board = boardFrom + (boardTo - boardFrom) * e
        window.setFrame(NSRect(x: from.minX + (to.minX - from.minX) * e, y: from.minY + (to.minY - from.minY) * e,
                               width: from.width + (to.width - from.width) * e, height: from.height + (to.height - from.height) * e),
                        display: true)
    }

    // 끝 상태로 마친다 — 두 번 불러도 한 번만
    func finish() {
        guard let done else { return }
        self.done = nil
        link?.invalidate()
        link = nil
        board = boardTo
        window.setFrame(to, display: true)
        if TerminalHub.shared.slide === self { TerminalHub.shared.slide = nil }
        done()
        // 다음 차례에 — 그사이 새 애니메이션이 시작했으면 그것이 끝난 뒤로 다시 미룬다
        for run in after { DispatchQueue.main.async { whenPaneSettled(run) } }
        after = []
    }
}

// 시트·확인 창은 펼치기·접기 애니메이션이 끝난 뒤에 띄운다 — 도는 중에 붙이면 시트가 먼저 붙고 창 프레임이 0.3초쯤 멈췄다가
// 건너뛴다(접는 중이면 접힌 창 가운데에 먼저 붙는다, 2026-10-18 실측). 애니메이션이 없으면 바로
@MainActor func whenPaneSettled(_ run: @escaping @MainActor () -> Void) {
    if let slide = TerminalHub.shared.slide { slide.after.append(run) } else { run() }
}

// 보드 위치 바꾸기(메뉴) — 바로 적용한다. 펼쳤으면 배치만 바뀌고, 접혔으면 같은 크기로 화면 반대쪽 끝으로 옮긴다(미니도 높이·위치 그대로).
// 미니에서 돌아갈 자리도 반대쪽 끝으로 옮겨 둔다
@MainActor func setBoardSide(left: Bool) {
    let hub = TerminalHub.shared
    hub.slide?.finish()
    guard hub.boardOnLeft != left else { return }
    hub.setBoardOnLeft(left)
    if let s = UserDefaults.standard.string(forKey: "boardRestoreFrame") {
        var r = NSRectFromString(s)
        if let vf = NSScreen.screens.first(where: { $0.visibleFrame.intersects(r) })?.visibleFrame {
            r.origin.x = left ? vf.minX : vf.maxX - r.width
            UserDefaults.standard.set(NSStringFromRect(r), forKey: "boardRestoreFrame")
        }
    }
    if hub.expanded {
        window.contentView?.needsLayout = true
        return
    }
    guard let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    var f = window.frame
    f.origin.x = left ? vf.minX : vf.maxX - f.width
    window.setFrame(f, display: true)
    if BoardFit.shared.tier != .mini { saveRestoreFrame() }
}

// ⌃⌥T·제목 막대 단추 — 펼쳐 있어도 보드가 앞에 없으면(다른 앱을 쓰는 중) 접지 않고 앞으로 꺼내 터미널에 포커스를 준다
@MainActor func toggleTerminal() {
    let hub = TerminalHub.shared
    if hub.expanded, !(NSApp.isActive && window.isKeyWindow) {
        showBoard()
        hub.focusSelected()
        return
    }
    setTerminalExpanded(!hub.expanded)
    if hub.expanded { showBoard() }
}

// 창이 화면의 쓸 수 있는 영역(메뉴 막대·Dock 을 뺀 곳, 펼치기와 같은 기준) 전체인가 — 2pt 오차까지 같은 것으로 본다
@MainActor func windowFillsScreen() -> Bool {
    guard let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return false }
    let f = window.frame
    return abs(f.minX - vf.minX) <= 2 && abs(f.minY - vf.minY) <= 2 && abs(f.width - vf.width) <= 2 && abs(f.height - vf.height) <= 2
}

// 터미널을 펼쳐 창이 화면 전체를 덮었나 — 그동안은 위에 고정·투명도를 쓰지 않는다. 펼치는 애니메이션 중에는 끝 크기(화면 전체)로 본다
@MainActor func expandedToFullScreen() -> Bool {
    let hub = TerminalHub.shared
    return hub.expanded && (hub.slide != nil || windowFillsScreen())
}

// 창 크기 전환(터미널을 펼쳤을 때 제목 막대 더블클릭·초록 단추) — 창이 화면의 쓸 수 있는 영역(펼치기와 같은 기준)과 같으면 가로·세로
// 60% 크기로 그 영역 가운데에, 그 밖의 크기면 그 영역 전체로. 크기는 기억하지 않는다(펼치기·다시 띄우기는 늘 화면 전체).
// 시스템 설정 "제목 막대 더블클릭"과 상관없이 늘 이 동작이다
@MainActor func toggleWindowSize() {
    let hub = TerminalHub.shared
    hub.slide?.finish()
    guard hub.expanded, let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    var target = vf
    if windowFillsScreen() {
        // setFrame 은 최소 크기(SizeGuard)를 거치지 않으므로 직접 맞춘다
        let min = window.frameRect(forContentRect: NSRect(origin: .zero, size: window.contentMinSize)).size
        let w = max((vf.width * 0.6).rounded(), min.width), h = max((vf.height * 0.6).rounded(), min.height)
        target = NSRect(x: (vf.midX - w / 2).rounded(), y: (vf.midY - h / 2).rounded(), width: w, height: h)
    }
    // 애니메이션 없이 — animate: true 는 프레임마다 터미널 크기가 바뀌어 pty 크기 변화가 0.34초에 11번 갔다(안의 claude 화면이
    // 그때마다 다시 그려진다, 2026-10-19 실측). 펼치기·접기 애니메이션처럼 크기를 고정할 수 없는 전환이다
    window.setFrame(target, display: true)
}

// 창 크기 전환을 받는 더블클릭 — 창 맨 위 제목 막대 띠의 빈 곳(터미널 쪽·보드 쪽)과 터미널 탭 줄의 빈 곳. 신호등 단추·제목 막대
// 부속(붙이기·터미널 펼치기·최대축소 단추)은 창 내용 밖(제목 막대 뷰)에 있어 그 단추 동작만 한다. 제목 막대 띠도 SwiftUI 호스팅 뷰가
// 먼저 받아 시스템의 더블클릭 확대가 일어나지 않으므로 앱 안 이벤트 감시로 받는다(2026-10-19 실측)
@MainActor func isWindowSizeDoubleClick(_ e: NSEvent) -> Bool {
    guard e.clickCount == 2, e.window === window, TerminalHub.shared.expanded,
          let content = window.contentView, let frameView = content.superview,
          let hit = frameView.hitTest(e.locationInWindow), hit.isDescendant(of: content) else { return false }
    let band = window.frame.height - window.contentLayoutRect.height
    if e.locationInWindow.y >= window.frame.height - band { return true }
    return TerminalHub.shared.area?.isTabBarBlank(e.locationInWindow) == true
}

// 창 내용: [터미널 영역 | 경계 | 보드]. 터미널 영역은 제목 막대 띠까지 덮고(창 바탕이 투명하다) 탭 줄은 그 아래에 둔다.
// 접힌 동안에도 터미널 영역은 창에 붙은 채 펼쳤을 때 크기로 둔다 —
// 크기가 0 이면 셸이 뜨지 않고, 크기가 바뀌면 안의 프로그램이 다시 그린다
final class BoardRootView: NSView {
    let terminal = TerminalAreaView()
    let divider = PaneDivider()
    let board: NSView

    init(board: NSView) {
        self.board = board
        super.init(frame: .zero)
        addSubview(terminal)
        addSubview(divider)
        addSubview(board)
    }
    required init?(coder: NSCoder) { fatalError() }

    override func layout() { super.layout(); place() }
    override func resizeSubviews(withOldSize oldSize: NSSize) { place() }

    private func place() {
        guard let w = window else { return }
        let hub = TerminalHub.shared, left = hub.boardOnLeft
        let below = w.contentLayoutRect.height   // 제목 막대 아래 높이
        terminal.titlebar = bounds.height - below
        if let slide = hub.slide {
            // 펼치기·접기 애니메이션 중 — 보드는 보드 쪽 창 가장자리에 붙여 화면의 제자리에 두고, 터미널 영역은 고정 크기(윗변을 창 윗변에)로
            // 보드 옆에 붙인다. 창 밖으로 나간 부분은 잘린다. 크기가 그대로라 pty 크기도 바뀌지 않는다
            let bw = slide.board, ts = slide.terminal
            let bx = left ? 0 : bounds.width - bw
            board.frame = NSRect(x: bx, y: 0, width: bw, height: bounds.height)
            divider.frame = NSRect(x: left ? bw : bx - paneDividerWidth, y: 0, width: paneDividerWidth, height: bounds.height)
            terminal.frame = NSRect(x: left ? bw + paneDividerWidth : bx - paneDividerWidth - ts.width, y: bounds.height - ts.height,
                                    width: ts.width, height: ts.height)
            terminal.isHidden = false
            divider.isHidden = false
            return
        }
        if hub.expanded {
            let bw = boardPaneWidth(), tw = bounds.width - bw - paneDividerWidth
            if left {
                board.frame = NSRect(x: 0, y: 0, width: bw, height: bounds.height)
                divider.frame = NSRect(x: bw, y: 0, width: paneDividerWidth, height: bounds.height)
                terminal.frame = NSRect(x: bw + paneDividerWidth, y: 0, width: tw, height: bounds.height)
            } else {
                terminal.frame = NSRect(x: 0, y: 0, width: tw, height: bounds.height)
                divider.frame = NSRect(x: tw, y: 0, width: paneDividerWidth, height: bounds.height)
                board.frame = NSRect(x: bounds.width - bw, y: 0, width: bw, height: bounds.height)
            }
        } else {
            var size = terminal.frame.size
            if size.width < terminalMinWidth, let vf = (w.screen ?? NSScreen.main)?.visibleFrame {
                size = NSSize(width: vf.width - TerminalHub.shared.boardWidth - paneDividerWidth, height: vf.height)
            }
            terminal.frame = NSRect(x: -size.width - 100, y: 0, width: size.width, height: size.height)
            board.frame = bounds
        }
        terminal.isHidden = !TerminalHub.shared.expanded
        divider.isHidden = !TerminalHub.shared.expanded
    }
}

// 누르면 단추는 동작하되 키보드 초점은 가져가지 않는 SwiftUI 호스팅 뷰 — 탭을 누른 뒤에도 입력이 터미널로 가게
final class PassiveHostingView<Content: View>: NSHostingView<Content> {
    override var acceptsFirstResponder: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

// 터미널 영역 — 맨 위 제목 막대 띠, 경로 줄(탭이 있을 때), 탭 줄(지금 경로의 탭), 아래에 탭마다 화면 뷰들(고른 탭의 것만 보임)
final class TerminalAreaView: NSView {
    let container = SurfaceContainerView()
    let empty = PassiveHostingView(rootView: TerminalEmptyView())
    private let workspaceBar = PassiveHostingView(rootView: TerminalWorkspaceBar(hub: TerminalHub.shared))
    private let tabBar = PassiveHostingView(rootView: TerminalTabBar(hub: TerminalHub.shared))
    var titlebar: CGFloat = 0 { didSet { if titlebar != oldValue { needsLayout = true } } }
    var workspaceBarShown = false { didSet { if workspaceBarShown != oldValue { needsLayout = true } } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = terminalBarColor.cgColor
        container.wantsLayer = true
        container.layer?.backgroundColor = terminalAreaColor.cgColor
        workspaceBar.sizingOptions = []
        tabBar.sizingOptions = []
        empty.sizingOptions = []
        addSubview(container)
        addSubview(empty)
        addSubview(workspaceBar)
        addSubview(tabBar)
    }
    required init?(coder: NSCoder) { fatalError() }

    // 탭 줄의 빈 곳(탭·"새 세션" 단추 오른쪽)이나 경로 줄의 빈 곳(경로 항목 오른쪽)인가 — 창 좌표
    func isTabBarBlank(_ point: NSPoint) -> Bool {
        let p = tabBar.convert(point, from: nil)
        if tabBar.bounds.contains(p) { return p.x > TerminalHub.shared.tabBarEnd }
        let q = workspaceBar.convert(point, from: nil)
        return workspaceBarShown && workspaceBar.bounds.contains(q) && q.x > TerminalHub.shared.workspaceBarEnd
    }

    override func layout() {
        super.layout()
        let top = bounds.height - titlebar
        let ws = workspaceBarShown ? terminalWorkspaceBarHeight : 0
        workspaceBar.frame = NSRect(x: 0, y: top - ws, width: bounds.width, height: ws)
        workspaceBar.isHidden = !workspaceBarShown
        tabBar.frame = NSRect(x: 0, y: top - ws - terminalTabBarHeight, width: bounds.width, height: terminalTabBarHeight)
        container.frame = NSRect(x: 0, y: 0, width: bounds.width, height: max(0, top - ws - terminalTabBarHeight))
        empty.frame = container.frame
    }
}

// 터미널 칸 — 탭마다 화면 뷰들과 그 위에 고른 탭의 나눈 경계. 칸 크기가 바뀌면 화면 자리를 다시 나눈다
final class SurfaceContainerView: NSView {
    private var dividers: [ObjectIdentifier: SplitDividerView] = [:]

    override func resizeSubviews(withOldSize oldSize: NSSize) { TerminalHub.shared.layoutSurfaces() }

    // 고른 탭의 경계들(가지, 가지 자리, 선 자리) — 끄는 동안 경계 뷰가 바뀌지 않게 가지마다 같은 뷰를 쓴다
    func showDividers(_ lines: [(SplitNode, NSRect, NSRect)], color: NSColor) {
        var keep: [ObjectIdentifier: SplitDividerView] = [:]
        for (node, area, line) in lines {
            let key = ObjectIdentifier(node)
            let v = dividers[key] ?? SplitDividerView()
            if v.superview == nil { addSubview(v) }   // 화면 뷰들 위에
            v.node = node
            v.area = area
            v.color = color
            // 잡는 자리는 선 양옆으로 3pt 씩 더(Ghostty 처럼 보이는 선은 1pt)
            let f = node.right ? line.insetBy(dx: -3, dy: 0) : line.insetBy(dx: 0, dy: -3)
            if v.frame != f { v.frame = f; window?.invalidateCursorRects(for: v) }
            keep[key] = v
        }
        for (key, v) in dividers where keep[key] == nil { v.removeFromSuperview() }
        dividers = keep
    }
}

// 나눈 화면 사이 경계 — 끌면 그 가지의 몫이 바뀌고(화면이 80pt 아래로 줄지 않게) 놓으면 남긴다
final class SplitDividerView: NSView {
    weak var node: SplitNode?
    var area: NSRect = .zero   // 그 가지가 차지하는 자리(터미널 칸 좌표)
    var color = NSColor.gray { didSet { if color != oldValue { needsDisplay = true } } }

    override var mouseDownCanMoveWindow: Bool { false }   // 창은 배경을 끌어 옮길 수 있게 되어 있다
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: node?.right == false ? .resizeUpDown : .resizeLeftRight) }

    override func draw(_ dirtyRect: NSRect) {
        color.setFill()
        (node?.right == false ? NSRect(x: 0, y: bounds.midY - 0.5, width: bounds.width, height: 1)
                              : NSRect(x: bounds.midX - 0.5, y: 0, width: 1, height: bounds.height)).fill()
    }

    override func mouseDown(with event: NSEvent) {}

    override func mouseDragged(with event: NSEvent) {
        guard let node, let p = superview?.convert(event.locationInWindow, from: nil) else { return }
        if node.right {
            node.setRatio((p.x - area.minX) / max(area.width - 1, 1), length: area.width)
        } else {
            node.setRatio((area.maxY - p.y) / max(area.height - 1, 1), length: area.height)
        }
        TerminalHub.shared.layoutSurfaces()
    }

    override func mouseUp(with event: NSEvent) { TerminalHub.shared.saveTabs() }
}

// 터미널과 보드 사이 경계 — 끌어서 보드 폭을 바꾸고, 놓으면 기억한다
final class PaneDivider: NSView {
    private var startX: CGFloat = 0
    private var startWidth: CGFloat = 0
    private var dragging = false { didSet { needsDisplay = true } }

    override var mouseDownCanMoveWindow: Bool { false }   // 창은 배경을 끌어 옮길 수 있게 되어 있다
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeLeftRight) }

    override func draw(_ dirtyRect: NSRect) {
        NSColor(red: 0.11, green: 0.11, blue: 0.14, alpha: 1).setFill()
        bounds.fill()
        (dragging ? NSColor(red: 0.48, green: 0.64, blue: 0.97, alpha: 1) : NSColor(white: 0.36, alpha: 1)).setFill()
        NSBezierPath(roundedRect: NSRect(x: bounds.midX - 1.5, y: bounds.midY - 22, width: 3, height: 44), xRadius: 1.5, yRadius: 1.5).fill()
    }

    override func mouseDown(with event: NSEvent) {
        startX = event.locationInWindow.x
        startWidth = boardPaneWidth()
        dragging = true
    }

    override func mouseDragged(with event: NSEvent) {
        let hub = TerminalHub.shared
        let dx = event.locationInWindow.x - startX   // 보드가 왼쪽이면 오른쪽으로 끌수록, 오른쪽이면 왼쪽으로 끌수록 넓어진다
        hub.boardWidth = max(boardMinWidth, min(hub.boardOnLeft ? startWidth + dx : startWidth - dx,
                                                 (window?.frame.width ?? 0) - paneDividerWidth - terminalMinWidth))
        superview?.needsLayout = true
        BoardFit.shared.update()
    }

    override func mouseUp(with event: NSEvent) {
        dragging = false
        UserDefaults.standard.set(Double(TerminalHub.shared.boardWidth), forKey: "boardPaneWidth")
    }
}

struct TabBarEnd: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

struct TabBarNewWidth: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

struct TerminalTabBar: View {
    @ObservedObject var hub: TerminalHub
    @State private var newHover = false
    @State private var newWidth: CGFloat = 0   // "새 세션" 단추 폭(앞 여백 포함) — 탭 칸에 줄 폭을 셀 때

    var body: some View {
        GeometryReader { g in
            // 지금 경로의 탭만. 탭 칸은 탭이 모두 들어가면 탭마다 가장 좁게~넓게(120~250) 나눠 갖고, 가장 좁게 해도 넘치면 가로로 밀어 본다
            // (넘친 HStack 은 가운데 맞춤이라 맨 앞의 가장 오래된 탭이 왼쪽 밖으로 잘렸다). 새 세션 단추는 칸 밖이라 늘 보인다
            let tabs = hub.tabs(in: hub.workspace)
            let n = CGFloat(tabs.count)
            let shown = max(0, min(g.size.width - 16 - newWidth, n * (TerminalTabButton.maxWidth + 2) - 2))
            let least = max(0, n * (TerminalTabButton.minWidth + 2) - 2)
            HStack(spacing: 0) {
                ScrollViewReader { proxy in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 2) {
                            ForEach(Array(tabs.enumerated()), id: \.element.id) { i, t in
                                // ⌘ 를 누르고 있으면 탭 줄 순서의 ⌘번호(⌘1~⌘9, 열 번째 탭부터는 없음)
                                TerminalTabButton(tab: t, selected: t.id == hub.selected, background: hub.background,
                                                  shortcut: hub.commandHeld && i < 9 ? i + 1 : nil, commandHeld: hub.commandHeld)
                                    .id(t.id)
                            }
                        }
                        .frame(width: max(shown, least))
                        .frame(maxHeight: .infinity)
                    }
                    .frame(width: shown)
                    // 고른 탭(⌘ 번호·탭 이동 키·새 탭 포함)이 칸 밖이면 보이게 민다. 칸 폭이 바뀔 때도 — 다시 뜰 때는 탭이 그려지기 전에
                    // 고른 탭이 정해져 그때 민 것이 헛돈다(시험 빌드 실측). 창·보드 칸 크기를 바꿀 때도 고른 탭을 놓치지 않게
                    .onAppear { scrollToSelected(proxy) }
                    .onChange(of: hub.selected) { scrollToSelected(proxy, animated: true) }
                    .onChange(of: shown) { scrollToSelected(proxy) }
                }
                newSessionButton
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .frame(width: g.size.width, height: g.size.height)
        }
        .coordinateSpace(name: "tabBar")
        .onPreferenceChange(TabBarEnd.self) { hub.tabBarEnd = $0 }
        .onPreferenceChange(TabBarNewWidth.self) { newWidth = $0 }
        .background(Color(nsColor: terminalBarColor))
        .overlay(alignment: .bottom) { Rectangle().fill(Color.white.opacity(0.07)).frame(height: 1) }
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }

    // 탭이 막 붙었거나 칸 폭이 막 바뀐 때는 그 배치가 끝난 뒤에 민다
    private func scrollToSelected(_ proxy: ScrollViewProxy, animated: Bool = false) {
        guard let id = hub.selected else { return }
        DispatchQueue.main.async {
            if animated { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(id) } } else { proxy.scrollTo(id) }
        }
    }

    private var newSessionButton: some View {
        // 새 세션 — 글자 대신 + 와 단축키(⌘T), 이름은 마우스를 올리면
        Button { hub.newSessionSheet() } label: {
            HStack(spacing: 4) {
                Image(systemName: "plus").font(.system(size: 10, weight: .bold))
                Text("⌘T").boardFont(11, weight: .medium).opacity(0.75)
            }
            .boardFont(12, weight: .semibold)
            .foregroundStyle(.primary.opacity(newHover ? 1 : 0.8))
            .padding(.horizontal, 11)
            .frame(height: 26)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(newHover ? 0.08 : 0)))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.white.opacity(0.22), style: StrokeStyle(lineWidth: 1, dash: [3, 2])))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($newHover)
        .hint("새 세션")
        .padding(.leading, 6)
        // 탭 줄의 빈 곳이 어디부터인지 — 그 오른쪽 더블클릭은 창 크기 전환(isWindowSizeDoubleClick)
        .background(GeometryReader { g in
            Color.clear.preference(key: TabBarEnd.self, value: g.frame(in: .named("tabBar")).maxX)
                .preference(key: TabBarNewWidth.self, value: g.size.width)
        })
    }
}

// 경로 줄 — 탭이 있는 작업 경로들(탭 목록에 처음 나타난 순서). 항목은 상태 점(그 경로 탭들의 세션 가운데 가장 급한 상태)·폴더 이름·탭 수,
// 누르면 그 경로로(마지막에 고른 탭). 빈 곳 더블클릭은 탭 줄처럼 창 크기 전환. 경로를 닫는 단추는 두지 않는다
struct TerminalWorkspaceBar: View {
    @ObservedObject var hub: TerminalHub

    var body: some View {
        GeometryReader { g in
            // 경로가 넘치면 가로로 밀어 본다 — 넘친 HStack 은 가운데 맞춤이라 맨 앞 경로가 왼쪽 밖으로 잘렸다(탭 줄과 같은 문제).
            // 항목은 이름을 줄이지 않고 왼쪽부터 늘어서고, 다 들어가면 그 오른쪽이 빈 곳
            let list = hub.workspaces
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(list, id: \.self) { ws in
                            WorkspaceButton(label: workspaceLabel(ws, among: list), path: ws, count: hub.tabs(in: ws).count,
                                            state: hub.workspaceStates[ws] ?? "idle", current: ws == hub.workspace, background: hub.background)
                                .id(ws)
                        }
                    }
                    // 경로 줄의 빈 곳이 어디부터인지 — 탭 줄과 따로 그리는 호스팅 뷰라 같은 키로 잰다
                    .background(GeometryReader { g in Color.clear.preference(key: TabBarEnd.self, value: g.frame(in: .named("workspaceBar")).maxX) })
                    .frame(minWidth: max(0, g.size.width - 16), maxHeight: .infinity, alignment: .leading)
                }
                // 지금 경로(⌃⌥↑↓·세션 줄 누르기·새 경로 포함)가 칸 밖이면 보이게 민다. 탭 줄처럼 줄 폭이 바뀔 때도
                .onAppear { scrollToCurrent(proxy) }
                .onChange(of: hub.workspace) { scrollToCurrent(proxy, animated: true) }
                .onChange(of: g.size.width) { scrollToCurrent(proxy) }
            }
            .padding(.horizontal, 8)
        }
        .coordinateSpace(name: "workspaceBar")
        .onPreferenceChange(TabBarEnd.self) { hub.workspaceBarEnd = $0 }
        .background(Color(nsColor: terminalBarColor))
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자도 보드 글자 비율을 따른다
    }

    // 경로가 막 붙었거나 줄 폭이 막 바뀐 때는 그 배치가 끝난 뒤에 민다
    private func scrollToCurrent(_ proxy: ScrollViewProxy, animated: Bool = false) {
        guard let ws = hub.workspace else { return }
        DispatchQueue.main.async {
            if animated { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(ws) } } else { proxy.scrollTo(ws) }
        }
    }
}

struct WorkspaceButton: View {
    let label: String
    let path: String
    let count: Int
    let state: String
    let current: Bool        // 지금 경로 — 탭 줄의 고른 탭처럼 터미널 바탕색에 작업 중 색 테두리
    let background: NSColor
    @State private var hover = false

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(Palette.color(state)).frame(width: 7, height: 7)
            Text(label).boardFont(12, weight: .semibold).lineLimit(1)
            Text("\(count)").boardFont(11, weight: .semibold).monospacedDigit().foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .frame(height: 24)
        .background(RoundedRectangle(cornerRadius: 7).fill(current ? Color(nsColor: background) : Color.white.opacity(hover ? 0.06 : 0)))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(current ? Palette.color("working").opacity(0.5) : .clear, lineWidth: 1))
        .foregroundStyle(current ? .primary : .secondary)
        .environment(\.colorScheme, current && isLightColor(background) ? .light : .dark)   // 밝은 테마 바탕이면 글자를 어둡게
        .contentShape(Rectangle())
        .onTapGesture { TerminalHub.shared.showWorkspace(path) }
        .clickable($hover)
        .hint(path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
    }
}

struct TerminalTabButton: View {
    static let minWidth: CGFloat = 120, maxWidth: CGFloat = 250   // 탭 폭 — 탭 줄이 넘칠지 셀 때도 쓴다
    @ObservedObject var tab: BoardTab
    let selected: Bool
    let background: NSColor
    let shortcut: Int?       // ⌘ 를 누르고 있는 동안 × 자리에 보일 ⌘번호
    let commandHeld: Bool
    @State private var hover = false
    @State private var closeHover = false

    var body: some View {
        HStack(spacing: 8) {
            // 상태 색은 화면들의 세션 가운데 가장 급한 것, 제목은 고른 화면의 세션 — 셸 화면이나 아직 요청 전이면 그 화면의 터미널 제목
            Circle().fill(Palette.color(tab.state)).frame(width: 8, height: 8)
            if tab.renaming {
                TabNameField(tab: tab).frame(width: 120)
            } else {
                Text(tab.shownName).boardFont(13, weight: .semibold).lineLimit(1).fixedSize()
            }
            // 고른 화면 하나 크게 보는 중(⇧⌘↩)
            if tab.zoomed {
                Image(systemName: "arrow.up.left.and.arrow.down.right").font(.system(size: 10, weight: .bold))
                    .foregroundStyle(Palette.color("working"))
                    .hint("화면 하나 크게 보는 중: ⇧⌘↩ 로 되돌립니다")
            }
            // ⌘ 를 누르고 있는 동안 고른 탭은 지금 하는 일 대신 이름 바꾸기 단축키
            if commandHeld && selected {
                Text("⇧⌘R 이름 바꾸기").boardFont(11).foregroundStyle(.secondary).lineLimit(1)
            } else {
                Text(tab.focused.session.map(doing) ?? tab.focused.title).boardFont(12).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
            if let n = shortcut {
                // 폭은 × 와 같게 두어 탭 폭이 흔들리지 않게(글자는 넘쳐 그린다)
                Text("⌘\(n)").boardFont(10, weight: .semibold).foregroundStyle(.secondary)
                    .fixedSize().frame(width: 16, height: 16)
            } else {
                Button { TerminalHub.shared.requestClose(tab) } label: {
                    Image(systemName: "xmark").font(.system(size: 8, weight: .bold))
                        .foregroundStyle(closeHover ? .primary : .secondary)
                        .frame(width: 16, height: 16)
                        .background(Circle().fill(Color.white.opacity(closeHover ? 0.14 : 0)))
                }
                .buttonStyle(.plain)
                .opacity(hover || selected ? 1 : 0)
                .onHover { closeHover = $0 }
                .hint("탭 닫기")
            }
        }
        .padding(.leading, 14).padding(.trailing, 8)
        .frame(minWidth: Self.minWidth, maxWidth: Self.maxWidth, maxHeight: .infinity)
        .background(selected ? Color(nsColor: background) : Color.white.opacity(hover ? 0.04 : 0))
        .overlay(alignment: .top) { Rectangle().fill(selected ? Palette.color("working") : .clear).frame(height: 2) }
        .foregroundStyle(selected ? .primary : .secondary)
        .environment(\.colorScheme, selected && isLightColor(background) ? .light : .dark)   // 밝은 테마 바탕이면 글자·아이콘을 어둡게
        .contentShape(Rectangle())
        .onTapGesture { if !tab.renaming { TerminalHub.shared.select(tab.id) } }
        .onHover { hover = $0 }
    }
}

// 탭 이름 입력 칸 — 탭 줄은 키 초점을 받지 않는 호스팅 뷰(PassiveHostingView)라 AppKit 입력 칸을 직접 첫 응답자로 세운다
// (처음 값은 탭 줄에 보이는 이름, 첫 응답자가 되면 전체 선택). Return 저장, Esc 취소, 다른 곳을 눌러 포커스를 잃으면 저장
struct TabNameField: NSViewRepresentable {
    let tab: BoardTab

    func makeNSView(context: Context) -> NSTextField {
        let f = NSTextField(string: tab.shownName)
        f.isBordered = false
        f.drawsBackground = false
        f.focusRingType = .none
        f.font = .systemFont(ofSize: 13 * BoardFontScale.shared.scale, weight: .semibold)   // 탭 이름 글자와 같게(보드 글자 비율)
        f.cell?.isScrollable = true
        f.cell?.wraps = false
        f.delegate = context.coordinator
        DispatchQueue.main.async { f.window?.makeFirstResponder(f) }   // 창에 붙은 뒤에
        return f
    }

    func updateNSView(_ f: NSTextField, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(tab: tab) }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        let tab: BoardTab
        init(tab: BoardTab) { self.tab = tab }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.insertNewline(_:)): end(control.stringValue)
            case #selector(NSResponder.cancelOperation(_:)): end(nil)
            default: return false
            }
            return true
        }

        // 포커스를 잃음 — 포커스가 옮겨 가는 도중이라 끝난 뒤에 저장한다(저장이 다시 포커스를 옮긴다). Return·Esc 로 이미 끝났으면 아무 일 없다
        func controlTextDidEndEditing(_ note: Notification) {
            let name = (note.object as? NSTextField)?.stringValue
            DispatchQueue.main.async { self.end(name) }
        }

        private func end(_ name: String?) { MainActor.assumeIsolated { TerminalHub.shared.endRename(tab, name: name) } }
    }
}

// 새 세션 기본 명령 — 승인 대기가 거의 없게 권한 확인을 건너뛰는 것도 의도한 것(2026-10-17 사용자)
let defaultSessionCommand = "claude --dangerously-skip-permissions"

struct NewSessionSheet: View {
    let paths: [String]
    let cancel: () -> Void
    let start: (String, String, Bool) -> Void   // 명령, 작업 폴더, 답변 마크다운 꾸미기
    @ObservedObject private var hub = TerminalHub.shared
    @ObservedObject private var fontScale = BoardFontScale.shared
    @State private var dir: String
    @State private var others: [String] = []
    @State private var command = defaultSessionCommand
    @State private var mdRender = true   // 명령처럼 열 때마다 기본값(켬)

    init(paths: [String], initial: String, cancel: @escaping () -> Void, start: @escaping (String, String, Bool) -> Void) {
        self.paths = paths
        self.cancel = cancel
        self.start = start
        _dir = State(initialValue: initial)
        _others = State(initialValue: paths.contains(initial) ? [] : [initial])
    }

    private func row(_ p: String) -> some View {
        let on = p == dir
        return Button { dir = p } label: {
            HStack(spacing: 10) {
                Image(systemName: on ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(on ? Palette.color("working") : .secondary)
                VStack(alignment: .leading, spacing: 1) {
                    Text((p as NSString).lastPathComponent).boardFont(13, weight: .semibold)
                    Text(p.replacingOccurrences(of: NSHomeDirectory(), with: "~")).boardFont(11).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 8).fill(on ? Palette.color("working").opacity(0.16) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func pick() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.directoryURL = URL(fileURLWithPath: dir)
        guard panel.runModal() == .OK, let p = panel.url?.path else { return }
        if !paths.contains(p), !others.contains(p) { others.append(p) }
        dir = p
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("새 세션").boardFont(16, weight: .bold)
            VStack(alignment: .leading, spacing: 6) {
                Text("작업 폴더 (보드에 있는 경로)").boardFont(12, weight: .semibold).foregroundStyle(.secondary)
                ScrollView {
                    VStack(spacing: 2) { ForEach(paths + others, id: \.self) { row($0) } }
                }
                // 스크롤 영역은 높이를 정해 주지 않으면 0 으로 접힌다. 줄 높이는 보드 글자 비율을 따른다(150% 에서 둘째 줄이 잘렸다)
                .frame(height: min(CGFloat(paths.count + others.count) * 46, 230) * fontScale.scale)
                Button("다른 폴더 고르기…", action: pick)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("실행할 명령").boardFont(12, weight: .semibold).foregroundStyle(.secondary)
                TextField("", text: $command).boardFont(13, design: .monospaced).textFieldStyle(.roundedBorder)
                Text("작업 경로의 셸에 쳐 넣어 실행합니다. claude 면 보드가 --session-id 를 붙입니다")
                    .boardFont(11).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 4) {
                Toggle("답변 마크다운 꾸미기", isOn: $mdRender).toggleStyle(.checkbox).boardFont(13)
                    .disabled(mdRenderDir == nil)
                Text("답변이 끝나면 제목·굵게·인라인 코드·목록 등을 색으로 다시 그립니다(md-render). claude 면 --plugin-dir 로 붙입니다")
                    .boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                Button("취소", action: cancel).keyboardShortcut(.cancelAction)
                Button("새 탭에서 시작") { start(command.trimmingCharacters(in: .whitespaces), dir, mdRender) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(command.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(18)
        .frame(width: 460)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }
}

// 터미널 테마 창(윈도우 > 터미널 테마…) — 엔진에 든 테마(485개)를 검색·미리보기하고 고른다. 한 번에 하나만(열려 있으면 앞으로).
// 줄을 누르거나 ↑↓ 로 옮기면 바로 그 테마로 바꾼다(열린 탭 모두). Return·닫기 단추·창 닫기는 그대로 두고 닫고, Esc 는 창을 열 때의
// 설정으로 되돌리고 닫는다. 키는 SessionMenuPanel 처럼 앱 안 이벤트 감시로 검색 칸보다 먼저 받는다(한글 조합 중이면 입력기 몫)
@MainActor final class ThemePicker: ObservableObject {
    static let shared = ThemePicker()
    // 이름순(대소문자 무시). GhosttyThemeCatalog.search("") 는 빈 글자를 Foundation 의 contains 로 견주어 하나도 돌려주지 않는다(실측)
    let themes = GhosttyThemeCatalog.allThemes.sorted { $0.name.caseInsensitiveCompare($1.name) == .orderedAscending }
    @Published var query = ""
    @Published private(set) var current: String?    // 지금 쓰는 테마(보드 설정 terminalTheme) — nil 은 "Ghostty 설정 따르기"
    private(set) var followed: GhosttyThemeDefinition?   // Ghostty 설정을 따를 때의 테마(색 띠·미리보기) — 열 때 읽는다
    private(set) var followedName = ""
    private var original: String?                    // 창을 열 때의 설정 — Esc 로 돌아간다
    private var win: NSWindow?
    private var monitor: Any?
    private var observer: NSObjectProtocol?

    // 보이는 줄 — nil 은 "Ghostty 설정 따르기"(검색어가 있으면 숨긴다). 이름 대소문자 무시 부분 일치
    var rows: [GhosttyThemeDefinition?] {
        let q = query.trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? [nil] + themes : themes.filter { $0.name.localizedCaseInsensitiveContains(q) }
    }

    func definition(_ name: String?) -> GhosttyThemeDefinition? { name.map { n in themes.first { $0.name == n } } ?? followed }

    func open() {
        if let win { win.makeKeyAndOrderFront(nil); return }
        original = UserDefaults.standard.string(forKey: "terminalTheme")
        current = original
        query = ""
        followed = themeDefinitions(ghosttyConfigValue("theme"))?.dark ?? GhosttyThemeCatalog.theme(named: "Afterglow")
        followedName = ghosttyThemeName()
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 420, height: 540), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        w.title = "터미널 테마"
        w.appearance = NSAppearance(named: .darkAqua)
        w.level = window.level   // 위에 고정한 보드(.floating)에 가리지 않게 — 고정하지 않았으면 다른 앱 위에 남지 않게 보통 층
        w.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        w.isReleasedWhenClosed = false
        w.contentViewController = NSHostingController(rootView: ThemePickerView(model: self))
        w.setContentSize(NSSize(width: 420, height: 540))   // 붙이는 순간 창이 아직 그리지 않은 뷰 크기(0)로 줄어든다 — 그대로 띄우면 키 창이 못 된다
        // 보드 쪽 끝에 붙여 연다 — 펼친 터미널이 가려지지 않게
        let f = window.frame
        w.setFrameOrigin(NSPoint(x: TerminalHub.shared.boardOnLeft ? f.minX + 20 : f.maxX - w.frame.width - 20, y: f.midY - w.frame.height / 2))
        win = w
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] e in self?.handle(e) ?? e }
        observer = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: w, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.closed() }
        }
        w.makeKeyAndOrderFront(nil)
    }

    private func handle(_ e: NSEvent) -> NSEvent? {
        guard let win, e.window === win, !e.modifierFlags.contains(.command) else { return e }
        if (win.firstResponder as? NSTextView)?.hasMarkedText() == true { return e }
        switch e.keyCode {
        case 125: move(1)
        case 126: move(-1)
        case 36, 76: close(revert: false)   // Return·Enter
        case 53: close(revert: true)        // Esc
        default: return e
        }
        return nil
    }

    // ↑↓ — 보이는 줄에서 한 줄씩(끝에서 멈춘다). 지금 것이 안 보이면 첫 줄·끝 줄로
    func move(_ d: Int) {
        let names = rows.map { $0?.name }
        guard !names.isEmpty else { return }
        if let i = names.firstIndex(of: current) { pick(names[max(0, min(names.count - 1, i + d))]) } else { pick(d > 0 ? names[0] : names[names.count - 1]) }
    }

    func pick(_ name: String?) {
        guard name != current else { return }
        current = name
        TerminalHub.shared.setTerminalTheme(name)
    }

    func close(revert: Bool) {
        if revert, current != original { TerminalHub.shared.setTerminalTheme(original) }
        win?.close()   // 정리는 closed
    }

    // 창이 닫혔다(Return·Esc·닫기 단추·창 닫기 단추·⌘W)
    private func closed() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        if let observer { NotificationCenter.default.removeObserver(observer) }
        monitor = nil
        observer = nil
        win = nil
        if NSApp.isActive, window.isVisible { window.makeKey() }   // 키 창·터미널 포커스를 보드로 돌린다
    }
}

// 테마 정의의 색 글자(RRGGBB, 앞의 # 은 있어도 된다) → 색. 없거나 읽지 못하면 회색
func themeColor(_ hex: String?) -> Color {
    guard let hex, let n = UInt32(hex.replacingOccurrences(of: "#", with: "").prefix(6), radix: 16) else { return .gray }
    return Color(red: Double(n >> 16 & 0xFF) / 255, green: Double(n >> 8 & 0xFF) / 255, blue: Double(n & 0xFF) / 255)
}

struct ThemePickerView: View {
    @ObservedObject var model: ThemePicker
    @FocusState private var searching: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            TextField("테마 이름 검색", text: $model.query)
                .textFieldStyle(.roundedBorder)
                .focused($searching)
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 1) { ForEach(model.rows, id: \.?.name) { row($0) } }.padding(4)
                }
                .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.04)))
                .onAppear { proxy.scrollTo(model.current, anchor: .center) }
                .onChange(of: model.current) { _, name in proxy.scrollTo(name) }   // ↑↓ 로 옮긴 줄이 보이게(보이면 그대로)
            }
            preview(model.definition(model.current))
            HStack {
                Text("↑↓ 고르기 · Return 닫기 · Esc 되돌리고 닫기").boardFont(11).foregroundStyle(.secondary)
                Spacer()
                Button("닫기") { model.close(revert: false) }
            }
        }
        .padding(14)
        .frame(width: 420, height: 540)
        .background(Color(red: 0.11, green: 0.11, blue: 0.17))
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
        .onAppear { DispatchQueue.main.async { searching = true } }
    }

    // 이름(+ "Ghostty 설정 따르기" 는 지금 그것이 무엇인지), 지금 쓰는 것에 체크, 오른쪽에 바탕색 칩 + palette 0~7 칩
    private func row(_ t: GhosttyThemeDefinition?) -> some View {
        let on = t?.name == model.current
        let colors = t ?? model.followed
        return HStack(spacing: 8) {
            Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(Palette.color("working"))
                .opacity(on ? 1 : 0).frame(width: 12)
            VStack(alignment: .leading, spacing: 1) {
                Text(t?.name ?? "Ghostty 설정 따르기").boardFont(13, weight: on ? .semibold : .regular).lineLimit(1)
                if t == nil { Text(model.followedName).boardFont(11).foregroundStyle(.secondary).lineLimit(1) }
            }
            Spacer(minLength: 8)
            HStack(spacing: 3) {
                RoundedRectangle(cornerRadius: 3).fill(themeColor(colors?.background)).frame(width: 18, height: 14)
                    .overlay(RoundedRectangle(cornerRadius: 3).stroke(Color.white.opacity(0.2), lineWidth: 0.5))
                HStack(spacing: 0) {
                    ForEach(0..<8, id: \.self) { i in Rectangle().fill(themeColor(colors?.palette[i])).frame(width: 9, height: 14) }
                }
                .clipShape(RoundedRectangle(cornerRadius: 3))
            }
        }
        .padding(.horizontal, 8).padding(.vertical, 5)
        .background(RoundedRectangle(cornerRadius: 7).fill(on ? Palette.color("working").opacity(0.18) : .clear))
        .contentShape(Rectangle())
        .onTapGesture { model.pick(t?.name) }
    }

    // 고른 테마의 색으로 그린 터미널 견본 — 프롬프트, 색 있는 ls 출력, diff 의 - · + 줄, 커서
    private func preview(_ t: GhosttyThemeDefinition?) -> some View {
        let fg = themeColor(t?.foreground)
        func p(_ i: Int) -> Color { themeColor(t?.palette[i] ?? t?.foreground) }
        func prompt(_ command: String) -> some View {
            HStack(spacing: 0) {
                Text("~/claude-board").foregroundStyle(p(4))
                Text(" on ").foregroundStyle(fg)
                Text("main").foregroundStyle(p(5))
                Text(" ❯ ").foregroundStyle(p(2))
                Text(command).foregroundStyle(fg)
            }
        }
        return VStack(alignment: .leading, spacing: 2) {
            prompt("ls")
            HStack(spacing: 0) {
                Text("docs  plugins").foregroundStyle(p(4)).bold()
                Text("  build-board.sh").foregroundStyle(p(2))
                Text("  README.md").foregroundStyle(fg)
            }
            prompt("git diff --cached")
            Text("- let theme = \"Ghostty\"").foregroundStyle(p(1))
            Text("+ let theme = \"\(t?.name ?? "")\"").foregroundStyle(p(2)).lineLimit(1)
            HStack(spacing: 0) {
                prompt("")
                Text(" ").background(themeColor(t?.cursorColor ?? t?.foreground))
            }
        }
        .boardFont(12, design: .monospaced)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(themeColor(t?.background)))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
    }
}

// 보드 아래쪽에 잠깐 뜨는 안내 — 백그라운드 세션을 눌렀을 때 등
struct TerminalNotice: View {
    @ObservedObject var hub: TerminalHub
    @State private var hover = false
    var body: some View {
        if let text = hub.notice {
            HStack(alignment: .top, spacing: 10) {
                Text(text).boardFont(12).fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
                Button { hub.dismissNotice() } label: {
                    Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                        .frame(width: 22, height: 22)
                        .background(Circle().fill(Color.white.opacity(hover ? 0.2 : 0.1)))
                }
                .buttonStyle(.plain).clickable($hover)
            }
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 12).fill(Color(red: 0.18, green: 0.16, blue: 0.24)))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color(red: 0.36, green: 0.30, blue: 0.48), lineWidth: 1))
            .shadow(color: .black.opacity(0.4), radius: 12, y: 6)
            .padding(.horizontal, 14).padding(.bottom, 34)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .preferredColorScheme(.dark)
        }
    }
}

// 탭이 하나도 없을 때
struct TerminalEmptyView: View {
    @State private var hover = false
    var body: some View {
        VStack(spacing: 12) {
            Text("보드에서 띄운 세션이 없습니다").boardFont(14, weight: .semibold)
            Text("보드에서 시작하거나 이어 연 세션이 여기 탭으로 열립니다").boardFont(12).foregroundStyle(.secondary)
            Button { TerminalHub.shared.newSessionSheet() } label: {
                HStack(spacing: 6) { Image(systemName: "plus"); Text("새 세션") }
                    .boardFont(12, weight: .semibold)
                    .padding(.horizontal, 14).frame(height: 30)
                    .background(Capsule().fill(Color.white.opacity(hover ? 0.16 : 0.1)))
            }
            .buttonStyle(.plain)
            .clickable($hover)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }
}

// 제목 막대의 "터미널" 단추 — 미니 제목 막대(폭 300)에는 자리가 없어 숨긴다
struct TerminalToggleButton: View {
    @ObservedObject private var hub = TerminalHub.shared
    @State private var hover = false
    var body: some View {
        let on = hub.expanded
        Button(action: toggleTerminal) {
            HStack(spacing: 5) {
                Image(systemName: hub.boardOnLeft ? "rectangle.trailinghalf.inset.filled" : "rectangle.leadinghalf.inset.filled")
                Text(on ? "터미널 펼침" : "터미널 접힘").fixedSize()
                Text("⌃⌥T").font(.system(size: 10, weight: .medium))
                    .padding(.horizontal, 4)
                    .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(0.08)))
            }
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(on ? Palette.color("working") : .primary.opacity(hover ? 1 : 0.8))
            .padding(.horizontal, 8)
            .frame(height: 20)
            .background(Capsule().fill(on ? Palette.color("working").opacity(0.18) : Color.white.opacity(hover ? 0.30 : 0.18)))
            .overlay(Capsule().stroke(on ? Palette.color("working").opacity(0.45) : Color.white.opacity(0.18), lineWidth: 0.5))
            .frame(height: 28)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(on ? "터미널 접기: 보드만 \(hub.boardOnLeft ? "왼쪽" : "오른쪽")에 남깁니다 (⌃⌥T)"
                 : "터미널 펼치기: 보드에서 띄운 세션을 \(hub.boardOnLeft ? "오른쪽" : "왼쪽") 탭에서 봅니다 (⌃⌥T)")
        .fixedSize()
    }
}
let terminalToggleWidth: CGFloat = 146
var terminalToggleSink: AnyCancellable?

@MainActor func addTerminalToggle() {
    let vc = NSTitlebarAccessoryViewController()
    let host = FirstClickHostingView(rootView: TerminalToggleButton())
    host.frame = NSRect(x: 0, y: 0, width: terminalToggleWidth, height: 28)
    vc.view = host
    vc.layoutAttribute = .leading
    window.addTitlebarAccessoryViewController(vc)
    // 왼쪽 부속에는 vc.isHidden 이 듣지 않는다 — 뷰를 숨기고 폭을 0 으로 줄여 자리도 비운다
    terminalToggleSink = BoardFit.shared.$tier.sink { t in
        host.isHidden = t == .mini
        host.frame.size.width = t == .mini ? 0 : terminalToggleWidth
    }
}

// MARK: - 자율주행

// 보드 자율주행 모드 2단계 첫 조각: 읽기 기초·자율주행 중·지난 실행(설계 docs/자율주행/설계.md 6장 2단계, 화면은 와이어프레임 5판).
// 실행 상태의 주인은 엔진(claude-autopilot)이다. 보드는 엔진이 남긴 실행 폴더(runs/<실행 id>/)와 기록 저장소(private/records.sqlite)를
// 읽어 그리고, 사람이 누른 일시정지·재개·중단만 control.json 으로 남긴다 — 보드의 유일한 쓰기이고 끝난 실행에는 쓰지 않는다(엔진 control 명령과 같은 규칙).
// 엔진 전용 자리(private/<실행 id>/, runs/*/engine/, pinned/)는 읽지 않는다

// 자율주행 뿌리 — 엔진의 autopilotRoot 와 같은 자리. 시험 빌드(번들 id com.claude.board.dev)는 Claude Board Dev 뿌리라
// 설치판의 실제 실행과 섞이지 않는다(엔진도 --dev 출발은 그 뿌리를 쓴다). 맨 위 코드의 전역 값(instanceName)보다 먼저 불릴 수 있어 계산 값으로 둔다
var autopilotRoot: String {
    NSHomeDirectory() + "/Library/Application Support/"
        + (Bundle.main.bundleIdentifier == "com.claude.board.dev" ? "Claude Board Dev" : "Claude Board") + "/autopilot"
}

private func apRead(_ path: String) -> Any? {
    FileManager.default.contents(atPath: path).flatMap { try? JSONSerialization.jsonObject(with: $0) }
}
private func apNum(_ v: Any?) -> Double? { (v as? NSNumber)?.doubleValue }
private func apStamp(_ path: String) -> Double { mtime(path)?.timeIntervalSince1970 ?? 0 }
private func apStrings(_ v: Any?) -> [String] { (v as? [Any] ?? []).map { "\($0)" } }
private func apText(_ v: Any?) -> String? { (v as? String).flatMap { $0.isEmpty ? nil : $0 } }

// 모델이 쓴 글의 긴 줄표는 보드에 그릴 때 쉼표로 바꾼다(설계: 보고서를 그릴 때 바꾼다, 사용자에게 보이는 글에는 긴 줄표를 쓰지 않는다)
func apPlain(_ s: String) -> String {
    s.replacingOccurrences(of: " — ", with: ", ").replacingOccurrences(of: "—", with: ", ")
}

private let apDayTime = formatter("M월 d일 HH:mm"), apDayOnly = formatter("M월 d일")
func apDate(_ ts: Double?) -> String { ts.map { apDayTime.string(from: Date(timeIntervalSince1970: $0)) } ?? "날짜 모름" }
func apDay(_ ts: Double?) -> String { ts.map { apDayOnly.string(from: Date(timeIntervalSince1970: $0)) } ?? "날짜 모름" }
func apClock(_ ts: Double) -> String { clock(Date(timeIntervalSince1970: ts)) }
// 걸린 시간 — 1분 안이면 초, 한 시간 안이면 분, 그 위는 시간·분
func apDur(_ sec: Double) -> String {
    let s = max(0, Int(sec))
    if s == 0 { return "0분" }
    if s < 60 { return "\(s)초" }
    return s >= 3600 ? "\(s / 3600)시간 \(s % 3600 / 60)분" : "\(s / 60)분"
}
func apUSD(_ x: Double) -> String { String(format: "%.2f", x) }
func apKind(_ k: String) -> String {
    ["fix": "고침", "dev": "개발", "feature": "개발", "story": "개발", "test": "시험", "doc": "문서", "docs": "문서", "env": "환경"][k.lowercased()] ?? k
}
func apUnitLabel(_ s: String) -> String {
    ["pending": "대기", "running": "진행 중", "done": "통과", "failed": "미통과", "capped": "상한에서 멈춤", "stopped": "중단", "skipped": "건너뜀"][s] ?? s
}

enum APColor {
    static let work = Palette.color("working")
    static let wait = Palette.color("waiting")
    static let judge = Color(red: 0.74, green: 0.60, blue: 1.00)
    static let limit = Color(red: 1.00, green: 0.62, blue: 0.36)
    static let stop = Color(red: 0.98, green: 0.80, blue: 0.35)
    static let pause = Palette.color("paused")
    static let ok = Color(red: 0.42, green: 0.85, blue: 0.60)
    static let fail = Color(red: 1.00, green: 0.45, blue: 0.45)
    static let cap = Color(red: 1.00, green: 0.70, blue: 0.36)
    static let muted = Color(white: 0.64)
    static let jira = Color(red: 0.40, green: 0.78, blue: 0.95)
    static let board = Color(red: 0.66, green: 0.76, blue: 1.00)
    static let model = Color(red: 0.55, green: 0.88, blue: 0.80)

    static func unit(_ s: String) -> Color {
        switch s {
        case "done": ok
        case "failed": fail
        case "capped": cap
        case "running": work
        default: muted
        }
    }
}

// 단위 상한 — 단위 시계(분)·누적 비용(USD)·주간 사용률(%p). 엔진 기본 120분·100 USD·5%p
struct APCaps: Equatable {
    var minutes = 120.0, usd = 100.0, pp = 5.0
    static func from(_ j: Any?, _ base: APCaps = APCaps()) -> APCaps {
        let o = j as? [String: Any] ?? [:]
        return APCaps(minutes: apNum(o["minutes"]) ?? base.minutes, usd: apNum(o["usd"]) ?? base.usd, pp: apNum(o["weekly_pp"]) ?? base.pp)
    }
}

// 단위 하나 — 정의·상태는 queue.json(엔진이 출발 때 동결), 진행은 units/<n>/engine.json(엔진이 보드에 내놓는 사본)
struct APUnit: Equatable, Identifiable {
    let n: Int
    var id: Int { n }
    var title = "", kind = "", status = "pending", cwd = "", check = "", unitId = ""
    var tickets: [String] = []
    var story: String?
    var branch: String?
    var estMin: Double?
    var depends: [String] = []
    var phase = "", reason = ""
    var activeSec = 0.0, cost = 0.0, weeklyPP = 0.0
    var tries = 0
    var started: Double?, ended: Double?
    var savedAt = 0.0          // engine.json 을 쓴 시각 — 단위 시계는 엔진이 5초마다(판정 중에는 시작·끝에만) 쓰므로 그 뒤로 흐른 만큼 더해 보인다
    var caps: APCaps?
    var merge: String?         // Jira 작업 → 스토리: merged·nothing·not_merged
    var mergeWhy = ""
    // Jira 작업 키 — 스토리가 있는 단위의 tickets 첫 값
    var task: String? { story == nil ? nil : tickets.first }
}

struct APEvent: Equatable { let ts: Double; let who: String; let text: String }

// 실행 하나 — run.json·queue.json·units/<n>/engine.json 을 합친 것
struct APRun: Equatable, Identifiable {
    let id: String
    let dir: String
    var title = "", state = "", reason = ""
    var project: String?
    var unit = 0
    var caps = APCaps()
    var unitCaps: APCaps?
    var waitUntil: Double?
    var pid: Int32 = 0
    var alive = false
    var started: Double?, ended: Double?
    var five: Double?, fiveResets: Double?, seven: Double?, sevenResets: Double?
    var stop7 = 0.80, stop5 = 0.95
    var mode = ""                       // worktree·in_place — 없으면(저장소가 아님) 원래 자리에서 돈다
    var repos: [String] = []            // 원래 저장소(따로 작업 트리면 그 원래 저장소 하나)
    var worktreePath: String?
    var work = ""                       // autopilot/<실행 id>
    var bases: [String: String] = [:]   // 저장소 → 출발 가지
    var originals: [String: String] = [:]
    var existing: [String] = []         // 출발 때 있던 가지(쓰기 금지)
    var storyBranches: [String] = []
    var checkRetries = 2
    var checkTimeout = 10.0
    var controlSeen = 0.0
    var units: [APUnit] = []

    var active: Bool { !state.isEmpty && !["done", "stopped", "error"].contains(state) }
    var jira: Bool { units.contains { $0.story != nil } }
    var current: APUnit? { unit >= 1 && unit <= units.count ? units[unit - 1] : nil }
    var totalCost: Double { units.reduce(0) { $0 + $1.cost } }
    var passed: Int { units.filter { $0.status == "done" }.count }
    var remaining: Int { units.filter { $0.status == "pending" || $0.status == "running" }.count }
    var inPlace: Bool { mode != "worktree" }
    var name: String { project ?? repos.first.map { ($0 as NSString).lastPathComponent } ?? id }
    var modeLabel: String { mode == "worktree" ? "따로 작업 트리" : "원래 작업 트리" }

    // 이 실행이 그 경로를 쓰는가 — 저장소 안이거나 같은 자리, 또는 그 경로가 저장소를 품은 폴더(홈은 빼고). 글자 그대로 견준다
    func covers(_ path: String) -> Bool {
        repos.contains { r in path == r || path.hasPrefix(r + "/") || (r.hasPrefix(path + "/") && path != NSHomeDirectory()) }
    }
    // 단위가 도는 저장소 이름 — 따로 작업 트리면 작업 트리 경로를 원래 저장소로 읽는다
    func repoName(_ u: APUnit) -> String {
        if let w = worktreePath, u.cwd == w || u.cwd.hasPrefix(w + "/"), let r = repos.first { return (r as NSString).lastPathComponent }
        let r = repos.first { u.cwd == $0 || u.cwd.hasPrefix($0 + "/") } ?? u.cwd
        return (r as NSString).lastPathComponent
    }
    // 보드 본체의 실행 줄이 쓰는 지금 단위의 단위 시계(분)
    func unitMinutes(_ now: Date) -> Int { current.map { Int(activeSec($0, now) / 60) } ?? 0 }
    // 단위 시계 — 엔진이 쓴 값에 마지막 저장 뒤로 흐른 시간을 더한다(일하는 중이고 엔진이 살아 있을 때만, 대기·일시정지는 세지 않는다)
    func activeSec(_ u: APUnit, _ now: Date) -> Double {
        guard state == "running", alive, u.status == "running", ["model", "check", "action"].contains(u.phase), u.savedAt > 0 else { return u.activeSec }
        // 판정 중에는 엔진이 시작·끝에만 쓰므로 판정 시간 상한까지, 그 밖(5초마다 씀)은 15분까지만 더한다
        let cap = u.phase == "check" ? checkTimeout * 60 + 60 : 900
        return u.activeSec + min(max(0, now.timeIntervalSince1970 - u.savedAt), cap)
    }

    static func parse(id: String, dir: String) -> APRun? {
        guard let r = apRead(dir + "/run.json") as? [String: Any] else { return nil }
        var x = APRun(id: id, dir: dir)
        x.title = r["title"] as? String ?? ""
        x.state = r["state"] as? String ?? ""
        x.reason = r["reason"] as? String ?? ""
        x.project = apText(r["project"])
        x.unit = Int(apNum(r["unit"]) ?? 0)
        x.caps = APCaps.from(r["caps"])
        if r["unit_caps"] is [String: Any] { x.unitCaps = APCaps.from(r["unit_caps"], x.caps) }
        x.waitUntil = apNum(r["wait_until"])
        x.pid = Int32(apNum(r["engine_pid"]) ?? 0)
        x.started = apNum(r["started"])
        x.ended = apNum(r["ended"])
        let use = r["usage"] as? [String: Any] ?? [:]
        x.five = apNum(use["five_hour"]); x.fiveResets = apNum(use["five_hour_resets"])
        x.seven = apNum(use["seven_day"]); x.sevenResets = apNum(use["seven_day_resets"])
        let line = r["stopline"] as? [String: Any] ?? [:]
        x.stop7 = apNum(line["seven_day"]) ?? 0.80
        x.stop5 = apNum(line["five_hour"]) ?? 0.95
        x.checkRetries = Int(apNum(r["check_retries"]) ?? 2)
        x.checkTimeout = apNum(r["check_timeout_min"]) ?? 10
        x.controlSeen = apNum(r["control_seen"]) ?? 0
        let br = r["branches"] as? [String: Any] ?? [:]
        let repos = br["repos"] as? [String: Any] ?? [:]
        for (k, v) in repos {
            let i = v as? [String: Any] ?? [:]
            if let b = (i["base"] as? [String: Any])?["branch"] as? String { x.bases[k] = b }
            if let o = i["original"] as? String { x.originals[k] = o }
            if let w = i["work"] as? String { x.work = w }
        }
        x.storyBranches = (br["stories"] as? [String: Any] ?? [:]).values.compactMap { ($0 as? [String: Any])?["branch"] as? String }.sorted()
        let start = br["start"] as? [String: Any] ?? [:]
        x.existing = Set(start.values.flatMap { ($0 as? [String: Any])?.keys.map { String($0) } ?? [] })
            .subtracting([x.work]).subtracting(x.storyBranches).sorted()
        let wt = r["worktree"] as? [String: Any] ?? [:]
        x.mode = wt["mode"] as? String ?? ""
        if x.mode == "worktree" {
            x.repos = [wt["repo"] as? String].compactMap { $0 }
            x.worktreePath = wt["path"] as? String
        } else {
            x.repos = (wt["repos"] as? [String]) ?? repos.keys.sorted()
        }
        if x.work.isEmpty, let b = wt["branch"] as? String { x.work = b }
        let q = apRead(dir + "/queue.json") as? [[String: Any]] ?? []
        for (i, d) in q.enumerated() {
            var u = APUnit(n: i + 1)
            u.title = d["title"] as? String ?? "단위 \(i + 1)"
            u.kind = d["kind"] as? String ?? ""
            u.status = d["status"] as? String ?? "pending"
            u.cwd = d["cwd"] as? String ?? ""
            u.check = d["check"] as? String ?? ""
            u.unitId = d["id"] as? String ?? ""
            u.tickets = apStrings(d["tickets"])
            u.story = apText(d["story"])
            u.branch = apText(d["branch"])
            u.estMin = apNum(d["est_min"])
            u.depends = apStrings(d["depends"])
            let ep = dir + "/units/\(i + 1)/engine.json"
            if let e = apRead(ep) as? [String: Any] {
                u.phase = e["phase"] as? String ?? ""
                u.reason = e["reason"] as? String ?? ""
                u.activeSec = apNum(e["active_sec"]) ?? 0
                u.cost = apNum(e["cost_usd"]) ?? 0
                u.weeklyPP = apNum(e["weekly_pp"]) ?? 0
                u.tries = Int(apNum(e["try"]) ?? 0)
                u.started = apNum(e["started"])
                u.ended = apNum(e["ended"])
                if e["caps"] is [String: Any] { u.caps = APCaps.from(e["caps"], x.caps) }
                if let m = e["merge"] as? [String: Any] { u.merge = m["state"] as? String; u.mergeWhy = m["why"] as? String ?? "" }
                if let b = apText(e["branch"]) { u.branch = b }
                u.savedAt = apStamp(ep)
            }
            x.units.append(u)
        }
        // 저장소가 아닌 작업 폴더에서 도는 실행은 가지 기록이 없다 — 단위 작업 폴더를 경로로 쓴다
        if x.repos.isEmpty { x.repos = Array(Set(x.units.map(\.cwd).filter { !$0.isEmpty })).sorted() }
        return x
    }
}

enum APScreen: Equatable {
    case board
    case running(String)        // 실행 id
    case history(String?)       // 고른 지난 실행(작은 크기에서 nil 이면 목록)
}

struct APSent: Equatable { let action: String; let ts: Double }

// 진행 기록 묶음 — 단위 번호(출발 0, 끝 -1)와 그 줄들(오래된 것부터)
struct APLogGroup: Identifiable {
    let id: Int
    var lines: [APEvent]
}

// 자율주행 읽기 — 1초마다 runs/ 를 훑는다. 파일마다 수정 시각을 기억해 바뀐 실행만 다시 풀고, 끝난 실행은 run.json 만 본다(보고 있는 실행은 빼고).
// 진행 기록(events.jsonl)은 자율주행 중 화면이 보는 실행만 이어 읽고, 기록 저장소는 지난 실행 화면이 떠 있을 때 저장소·WAL 파일이 바뀌었을 때만 읽는다
@MainActor final class AutopilotStore: ObservableObject {
    static let shared = AutopilotStore()
    @Published private(set) var runs: [APRun] = []        // 끝나지 않은 실행(출발 순)
    @Published private(set) var viewed: APRun?            // 자율주행 중 화면이 보는 실행 — 끝난 뒤에도 그 화면에서는 끝 상태로 남는다
    @Published private(set) var events: [APEvent] = []
    @Published private(set) var sent: [String: APSent] = [:]   // 보낸 요청 — 엔진이 받을 때(run.json control_seen)까지 단추를 잠근다
    @Published private(set) var history = APHistory()
    @Published private(set) var detail: APPastDetail?
    @Published var screen: APScreen = .board { didSet { if screen != oldValue { tick() } } }
    private var cache: [String: (stamp: [Double], run: APRun?)] = [:]
    private var evDir = "", evOffset: UInt64 = 0, evRest = Data()
    private var dbStamp: [Double] = [], detailKey = ""
    private var timer: Timer?

    private init() {
        tick()
        let t = Timer(timeInterval: 1, repeats: true) { [weak self] _ in MainActor.assumeIsolated { self?.tick() } }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    private func stampOf(_ dir: String, units: Int) -> [Double] {
        [apStamp(dir + "/run.json"), apStamp(dir + "/queue.json")] + (0..<units).map { apStamp(dir + "/units/\($0 + 1)/engine.json") }
    }

    func tick() {
        let base = autopilotRoot + "/runs"
        let names = ((try? FileManager.default.contentsOfDirectory(atPath: base)) ?? []).filter { !$0.hasPrefix(".") }
        var viewing: String?
        if case .running(let id) = screen { viewing = id }
        var live: [APRun] = [], seen = Set<String>(), view: APRun?
        for name in names {
            let dir = base + "/" + name
            let runM = apStamp(dir + "/run.json")
            guard runM > 0 else { continue }
            seen.insert(dir)
            var run: APRun?
            if let c = cache[dir], c.stamp.first == runM, let r = c.run, !r.active, name != viewing {
                run = r   // 끝난 실행 — 대기열·단위 파일은 보지 않는다
            } else {
                let st = stampOf(dir, units: cache[dir]?.run?.units.count ?? 0)
                if let c = cache[dir], c.stamp == st {
                    run = c.run
                } else {
                    run = APRun.parse(id: name, dir: dir)
                    cache[dir] = (stampOf(dir, units: run?.units.count ?? 0), run)
                }
            }
            guard var r = run else { continue }
            r.alive = r.pid > 0 && (kill(r.pid, 0) == 0 || errno == EPERM)
            if r.active { live.append(r) }
            if name == viewing { view = r }
        }
        cache = cache.filter { seen.contains($0.key) }
        live.sort { ($0.started ?? .infinity, $0.id) < ($1.started ?? .infinity, $1.id) }
        if live != runs { runs = live }
        if view != viewed { viewed = view }
        // 엔진이 받았거나(control_seen) 20초가 지나도 안 받으면(엔진이 꺼짐) 단추를 다시 푼다
        let now = Date().timeIntervalSince1970
        let keep = sent.filter { id, s in
            guard let r = live.first(where: { $0.id == id }) ?? (view?.id == id ? view : nil), r.active else { return false }
            return r.controlSeen + 0.0005 < s.ts && now - s.ts < 20
        }
        if keep != sent { sent = keep }
        if let v = viewing { readEvents(base + "/" + v) } else if !evDir.isEmpty { evDir = ""; evOffset = 0; evRest = Data(); events = [] }
        if case .history(let sel) = screen { readHistory(sel) }
    }

    // events.jsonl 을 이어 읽는다 — 앞에서 읽은 자리부터 새 줄만. 파일이 줄었으면 처음부터
    private func readEvents(_ dir: String) {
        if evDir != dir { evDir = dir; evOffset = 0; evRest = Data(); if !events.isEmpty { events = [] } }
        guard let h = FileHandle(forReadingAtPath: dir + "/events.jsonl") else { return }
        defer { try? h.close() }
        let end = (try? h.seekToEnd()) ?? 0
        if end < evOffset { evOffset = 0; evRest = Data(); events = [] }
        guard end > evOffset else { return }
        try? h.seek(toOffset: evOffset)
        let chunk = (try? h.readToEnd()) ?? Data()
        evOffset += UInt64(chunk.count)
        var data = evRest + chunk
        var add: [APEvent] = []
        if let last = data.lastIndex(of: UInt8(ascii: "\n")) {
            evRest = data[data.index(after: last)...]
            data = data[..<last]
        } else {
            evRest = data
            return
        }
        for line in data.split(separator: UInt8(ascii: "\n")) {
            guard let j = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any], let text = j["text"] as? String else { continue }
            add.append(APEvent(ts: apNum(j["ts"]) ?? 0, who: j["who"] as? String ?? "", text: text))
        }
        guard !add.isEmpty else { return }
        var all = events + add
        if all.count > 5000 { all.removeFirst(all.count - 5000) }
        events = all
    }

    private func readHistory(_ sel: String?) {
        let db = autopilotRoot + "/private/records.sqlite"
        let st = [apStamp(db), apStamp(db + "-wal")]   // WAL 이라 체크포인트 전까지 본 파일 시각이 그대로다 — 둘 다 본다
        if st != dbStamp {
            dbStamp = st
            history = APRecords.list(db)
            detailKey = ""
        }
        let id = sel ?? history.runs.first?.id ?? ""
        guard id != detailKey else { return }
        detailKey = id
        detail = id.isEmpty ? nil : history.runs.first { $0.id == id }.flatMap { APRecords.detail(db, $0) }
    }

    // 경로 카드에 들어갈 실행
    func runs(at path: String) -> [APRun] { runs.filter { $0.covers(path) } }
    // 원래 작업 트리로 그 경로를 쓰는 실행 — 보드 탭 새 세션 경고
    func inPlaceRun(covering dir: String) -> APRun? { runs.first { $0.inPlace && $0.covers(dir) } }
    // 자율주행 보기의 사용량 상자 멈출 선(%, UsagePanel 의 한도 id 로)
    var stoplines: [String: Double]? {
        runs.first.map { ["5h": $0.stop5 * 100, "7d": $0.stop7 * 100] }
    }

    // 일시정지·재개·중단 — control.json 을 엔진 control 명령과 같은 꼴({"action","ts"}, 임시 파일을 바꿔 넣음)로 쓴다.
    // 쓰기 직전에 run.json 을 다시 읽어 끝난 실행이면 쓰지 않는다
    func send(_ action: String, _ run: APRun) {
        guard ["pause", "resume", "stop"].contains(action), sent[run.id] == nil else { return }
        let state = (apRead(run.dir + "/run.json") as? [String: Any])?["state"] as? String ?? ""
        guard !state.isEmpty, !["done", "stopped", "error"].contains(state) else { return }
        let ts = (Date().timeIntervalSince1970 * 1000).rounded() / 1000
        guard let data = try? JSONSerialization.data(withJSONObject: ["action": action, "ts": ts], options: [.prettyPrinted, .sortedKeys]) else { return }
        let path = run.dir + "/control.json", tmp = path + ".tmp-\(getpid())"
        guard FileManager.default.createFile(atPath: tmp, contents: data + Data("\n".utf8)), rename(tmp, path) == 0 else {
            unlink(tmp)
            boardLog("자율주행 \(run.id): control.json 을 쓰지 못했습니다(\(action))")
            return
        }
        sent[run.id] = APSent(action: action, ts: ts)
    }

    // 진행 기록을 단위별로 묶는다. events.jsonl 에는 단위 번호가 없어 단위의 시작·끝 시각(engine.json)으로 가른다:
    // 단위 창(시작 ~ 끝 + 1초) 안이면 그 단위, 다음 단위 시작 3초 앞의 보드 줄(스토리·작업 가지 만들기)이면 다음 단위, 그 밖의 틈(멈출 선·환경 확인 대기)은
    // 앞 단위. 첫 단위 앞은 출발(0), 실행이 끝났다는 줄부터는 끝(-1)
    static func groups(_ run: APRun, _ events: [APEvent]) -> [APLogGroup] {
        let starts = run.units.compactMap { u in u.started.map { (u.n, $0, u.ended) } }.sorted { $0.1 < $1.1 }
        let finishes = ["자율주행을 마쳤습니다", "사람이 중단해 실행을 마칩니다", "엔진 오류로 멈춥니다", "고정한 실행 설정이 출발 때와 달라"]
        var out: [Int: [APEvent]] = [:], order: [Int] = []
        var finished = false
        for e in events {
            if !run.active, !finished, finishes.contains(where: { e.text.hasPrefix($0) }) { finished = true }
            var g = 0
            if finished {
                g = -1
            } else if let i = starts.lastIndex(where: { $0.1 <= e.ts }) {
                let s = starts[i]
                if s.2 == nil || e.ts <= s.2! + 1 {
                    g = s.0
                } else if i + 1 < starts.count, e.who != "engine", starts[i + 1].1 - e.ts <= 3 {
                    g = starts[i + 1].0
                } else {
                    g = s.0
                }
            } else if let f = starts.first, e.who != "engine", f.1 - e.ts <= 3 {
                g = f.0
            }
            if out[g] == nil { order.append(g) }
            out[g, default: []].append(e)
        }
        return order.map { APLogGroup(id: $0, lines: out[$0] ?? []) }
    }
}

// MARK: 자율주행 · 기록 저장소(읽기 전용)

private let apSQLiteTransient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

struct APChange: Equatable { let path: String; let change: String }

struct APResult: Equatable {
    var headline = ""
    var summary: [String] = [], risks: [String] = [], tests: [String] = [], changes: [String] = []
    static func parse(_ text: String?) -> APResult? {
        guard let t = text, let j = (try? JSONSerialization.jsonObject(with: Data(t.utf8))) as? [String: Any] else { return nil }
        func list(_ k: String) -> [String] { (j[k] as? [Any] ?? []).map { apPlain("\($0)") } }
        var r = APResult()
        r.headline = apPlain(j["headline"] as? String ?? "")
        r.summary = list("summary")
        r.risks = list("risks")
        r.tests = (j["tests"] as? [[String: Any]] ?? []).map { t in
            let res = ["pass": "통과", "fail": "실패", "skip": "건너뜀"][t["result"] as? String ?? ""] ?? (t["result"] as? String ?? "")
            return apPlain("\(t["name"] as? String ?? "시험") \(res)")
        }
        r.changes = (j["changes"] as? [[String: Any]] ?? []).map { c in
            apPlain([c["what"] as? String, c["where"] as? String].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
        }
        return r
    }
}

struct APPastRun: Equatable, Identifiable {
    let id: String
    var project: String?
    var projectName = "", title = "", state = "", reason = "", model = "", runDir = ""
    var started: Double?, ended: Double?
    var total = 0, done = 0, failed = 0, capped = 0, stopped = 0, skipped = 0, stories = 0, jiraUnits = 0
    var activeSec = 0.0, cost = 0.0
    var mode = ""
    var hasBranches = false
    var harnessSame: Bool?
    var harnessFiles = 0
    var harnessChanged: [APChange] = []
    var promotedInto: [String] = []
    var discarded = false
    var epic: String?
    var jira: Bool { jiraUnits > 0 }

    // 결과 한 줄 — "4개 중 3개 통과 · 상한 1"
    var result: String {
        var s = "\(total)개 중 \(done)개 통과"
        for (n, label) in [(failed, "미통과"), (capped, "상한"), (skipped, "건너뜀"), (stopped, "중단")] where n > 0 { s += " · \(label) \(n)" }
        return s
    }
    var harnessLabel: String {
        guard let same = harnessSame else { return "하네스 기록 없음" }
        return same ? "하네스 같음" : "하네스 바뀜 \(harnessChanged.count)"
    }
    // 합치기 상태(설계 판단): 버렸으면 버림, 합친 단계가 있으면 마지막으로 받은 가지, 아니면 합치기 기다림. 가지 기록이 없는 실행은 없음
    var mergeLabel: (String, Color)? {
        guard hasBranches else { return nil }
        if discarded { return ("버림", APColor.muted) }
        if let last = promotedInto.last {
            return (["master", "main"].contains(last) ? "\(last) 까지 올림" : "\(last) 에 합침", APColor.ok)
        }
        return ("합치기 기다림", APColor.wait)
    }
    var stateLabel: (String, Color)? {
        switch state {
        case "stopped": ("중단됨", APColor.muted)
        case "error": ("오류로 멈춤", APColor.fail)
        default: nil
        }
    }
}

struct APPastUnit: Equatable, Identifiable {
    let n: Int
    var id: Int { n }
    var title = "", kind = "", status = "", branch = ""
    var story: String?, task: String?, merged: String?
    var estMin: Double?
    var activeSec = 0.0, cost = 0.0
    var tries = 0
    var result: APResult?
    var dependsOn: [String] = []   // 기대는 단위(작업 키, 없으면 "단위 n") — 실행 폴더의 queue.json 이 남아 있을 때만
}

struct APCarry: Equatable, Identifiable {
    let id: String
    var unit = 0, kind = "", title = "", why = ""
}

struct APPastDetail: Equatable {
    var run: APPastRun
    var units: [APPastUnit] = []
    var carry: [APCarry] = []
}

struct APHistory: Equatable {
    var version = 0
    var error: String?
    var runs: [APPastRun] = []   // 끝난 실행(완료·중단·오류), 최근 것부터
}

// 기록 저장소 읽기 — 읽기 전용으로 열고(SQLITE_OPEN_READONLY, 만들지 않음) 읽을 때마다 닫는다(긴 읽기 트랜잭션이 엔진의 체크포인트를 막지 않게).
// 판(user_version)이 이 보드가 아는 판보다 높으면 읽을 수 있는 만큼 읽고 "보드를 업데이트하세요"를 보인다(설계 5장 7절)
enum APRecords {
    static let knownVersion = 1

    private static func open(_ path: String) -> OpaquePointer? {
        guard FileManager.default.fileExists(atPath: path) else { return nil }
        var db: OpaquePointer?
        guard sqlite3_open_v2(path, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else { sqlite3_close_v2(db); return nil }
        sqlite3_busy_timeout(db, 1000)
        return db
    }

    private static func rows(_ db: OpaquePointer?, _ sql: String, _ args: [String] = [], error: inout String?) -> [[Any?]] {
        var st: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &st, nil) == SQLITE_OK else {
            error = String(cString: sqlite3_errmsg(db))
            return []
        }
        defer { sqlite3_finalize(st) }
        for (i, a) in args.enumerated() { sqlite3_bind_text(st, Int32(i + 1), a, -1, apSQLiteTransient) }
        var out: [[Any?]] = []
        while sqlite3_step(st) == SQLITE_ROW {
            var row: [Any?] = []
            for c in 0..<sqlite3_column_count(st) {
                switch sqlite3_column_type(st, c) {
                case SQLITE_INTEGER: row.append(Double(sqlite3_column_int64(st, c)))
                case SQLITE_FLOAT: row.append(sqlite3_column_double(st, c))
                case SQLITE_TEXT: row.append(sqlite3_column_text(st, c).map { String(cString: $0) })
                default: row.append(nil)
                }
            }
            out.append(row)
        }
        return out
    }

    private static func json(_ v: Any?) -> Any? { (v as? String).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) } }

    static func list(_ path: String) -> APHistory {
        var h = APHistory()
        guard let db = open(path) else { return h }
        defer { sqlite3_close_v2(db) }
        var err: String?
        h.version = Int(rows(db, "PRAGMA user_version", error: &err).first?.first.flatMap { $0 as? Double } ?? 0)
        let sql = """
            SELECT r.id, r.project_id, COALESCE(p.name, r.project_id), r.title, r.state, r.reason, r.model, r.started, r.ended,
              r.units_total, r.units_done, r.units_failed, r.units_capped, r.units_stopped, r.active_sec, r.cost_usd,
              r.worktree, r.branches, r.harness, r.epic, r.run_dir,
              (SELECT COUNT(*) FROM units u WHERE u.run_id = r.id AND u.status = 'skipped'),
              (SELECT COUNT(*) FROM units u WHERE u.run_id = r.id AND u.story_key IS NOT NULL),
              (SELECT COUNT(DISTINCT u.story_key) FROM units u WHERE u.run_id = r.id AND u.story_key IS NOT NULL)
            FROM runs r LEFT JOIN projects p ON p.id = r.project_id
            WHERE r.state IN ('done', 'stopped', 'error')
            ORDER BY COALESCE(r.started, r.updated) DESC
            """
        for c in rows(db, sql, error: &err) {
            var r = APPastRun(id: c[0] as? String ?? "?")
            r.project = c[1] as? String
            r.projectName = c[2] as? String ?? "프로젝트 없음"
            r.title = c[3] as? String ?? ""
            r.state = c[4] as? String ?? ""
            r.reason = c[5] as? String ?? ""
            r.model = c[6] as? String ?? ""
            r.started = c[7] as? Double
            r.ended = c[8] as? Double
            r.total = Int(c[9] as? Double ?? 0)
            r.done = Int(c[10] as? Double ?? 0)
            r.failed = Int(c[11] as? Double ?? 0)
            r.capped = Int(c[12] as? Double ?? 0)
            r.stopped = Int(c[13] as? Double ?? 0)
            r.activeSec = c[14] as? Double ?? 0
            r.cost = c[15] as? Double ?? 0
            r.mode = (json(c[16]) as? [String: Any])?["mode"] as? String ?? ""
            if let b = json(c[17]) as? [String: Any] {
                r.hasBranches = !((b["repos"] as? [String: Any]) ?? [:]).isEmpty
                r.promotedInto = (b["promoted"] as? [[String: Any]] ?? []).compactMap { $0["into"] as? String }
                r.discarded = !(b["discarded"] as? [Any] ?? []).isEmpty
            }
            if let hs = json(c[18]) as? [String: Any] {
                r.harnessFiles = Int(apNum(hs["files"]) ?? 0)
                if let same = hs["same"] as? Bool {
                    r.harnessSame = same
                    r.harnessChanged = (hs["changed"] as? [[String: Any]] ?? []).map {
                        APChange(path: $0["path"] as? String ?? "?", change: $0["change"] as? String ?? "")
                    }
                }
            }
            r.epic = c[19] as? String
            r.runDir = c[20] as? String ?? ""
            r.skipped = Int(c[21] as? Double ?? 0)
            r.jiraUnits = Int(c[22] as? Double ?? 0)
            r.stories = Int(c[23] as? Double ?? 0)
            h.runs.append(r)
        }
        h.error = err
        return h
    }

    static func detail(_ path: String, _ run: APPastRun) -> APPastDetail? {
        guard let db = open(path) else { return nil }
        defer { sqlite3_close_v2(db) }
        var err: String?
        var d = APPastDetail(run: run)
        // 기대는 단위는 기록 저장소에 없다 — 실행 폴더가 남아 있으면 그 queue.json 에서 읽는다(원문을 지운 실행은 비어 있다)
        let q = apRead(run.runDir + "/queue.json") as? [[String: Any]] ?? []
        let idToN = Dictionary(q.enumerated().compactMap { i, u in (u["id"] as? String).map { ($0, i + 1) } }, uniquingKeysWith: { a, _ in a })
        let sql = """
            SELECT n, title, kind, story_key, task_key, branch, status, merged, est_min, active_sec, cost_usd, tries, result
            FROM units WHERE run_id = ? ORDER BY n
            """
        for c in rows(db, sql, [run.id], error: &err) {
            var u = APPastUnit(n: Int(c[0] as? Double ?? 0))
            u.title = c[1] as? String ?? ""
            u.kind = c[2] as? String ?? ""
            u.story = c[3] as? String
            u.task = c[4] as? String
            u.branch = c[5] as? String ?? ""
            u.status = c[6] as? String ?? ""
            u.merged = c[7] as? String
            u.estMin = c[8] as? Double
            u.activeSec = c[9] as? Double ?? 0
            u.cost = c[10] as? Double ?? 0
            u.tries = Int(c[11] as? Double ?? 0)
            u.result = APResult.parse(c[12] as? String)
            d.units.append(u)
        }
        for i in d.units.indices {
            let n = d.units[i].n
            guard n >= 1, n <= q.count else { continue }
            let deps = apStrings(q[n - 1]["depends"]).compactMap { idToN[$0] }
            d.units[i].dependsOn = deps.map { k in d.units.first { $0.n == k }?.task ?? "단위 \(k)" }
        }
        for c in rows(db, "SELECT id, unit_n, kind, title, why FROM carry WHERE run_id = ? ORDER BY unit_n, id", [run.id], error: &err) {
            d.carry.append(APCarry(id: c[0] as? String ?? UUID().uuidString, unit: Int(c[1] as? Double ?? 0), kind: c[2] as? String ?? "",
                                   title: apPlain(c[3] as? String ?? ""), why: apPlain(c[4] as? String ?? "")))
        }
        return d
    }
}

// MARK: 자율주행 · 작은 부품

// 매초 다시 그리는 잎 — 시계 글자만 다시 그리고 바깥(카드·목록)은 그대로 둔다. 자율주행 중 화면 전용이다:
// 보드 본체에 두면 이 시계가 보드의 1초 갱신과 따로 보드 전체 배치를 다시 계산하게 해 CPU 가 늘었다(보드 본체는 Store.now 로 그린다)
struct APTick<Content: View>: View {
    @ViewBuilder let content: (Date) -> Content
    var body: some View { TimelineView(.periodic(from: .now, by: 1)) { ctx in content(ctx.date) } }
}

// 매초 바뀌는 글자 — 자리는 숫자를 모두 0 으로 바꾼 숨은 글자가 잡고 실제 글자는 겹쳐 그린다(자릿수가 바뀔 때만 크기를 다시 잰다).
// 보드 CPU 고침의 TickingText(가지 board-cpu, 이 가지에는 없음)와 같은 꼴이다 — main 을 합칠 때 하나로 줄인다
struct APTickText: View {
    let text: (Date) -> String
    var body: some View {
        APTick { now in
            let s = text(now)
            Text(String(s.map { $0.isWholeNumber ? "0" : $0 })).hidden()
                .overlay(alignment: .leading) { Text(s).fixedSize() }
        }
    }
}

struct APChip: View {
    let text: String
    var color: Color = APColor.muted
    var strong = false      // 상태 꼬리표는 진하게
    var mono = false
    var body: some View {
        Text(text)
            .boardFont(10, weight: .semibold, design: mono ? .monospaced : .default)
            .foregroundStyle(color)
            .padding(.horizontal, 6).padding(.vertical, 1.5)
            .background(Capsule().fill(color.opacity(strong ? 0.20 : 0.09)))
            .overlay(Capsule().stroke(color.opacity(strong ? 0.38 : 0.18), lineWidth: 0.5))
            .lineLimit(1)
            .fixedSize()
    }
}

// 자율주행 표지 — 실행 줄 앞머리
struct APMark: View {
    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: "steeringwheel").font(.system(size: 9, weight: .bold))
            Text("자율주행")
        }
        .boardFont(10, weight: .bold)
        .foregroundStyle(Palette.accent)
        .padding(.horizontal, 6).padding(.vertical, 1.5)
        .background(Capsule().fill(Palette.accent.opacity(0.16)))
        .fixedSize()
    }
}

struct APTicket: View {
    let key: String
    var body: some View { APChip(text: key, color: APColor.jira, mono: true) }
}

// 이름 · 값 · 막대(멈출 선 눈금)
struct APMeter: View {
    let label: String
    let value: String
    var fraction: Double? = nil
    var tick: Double? = nil
    var color: Color = APColor.work
    var note: String? = nil
    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Text(label).foregroundStyle(.secondary).lineLimit(1)
                Spacer(minLength: 4)
                Text(value).fontWeight(.semibold).monospacedDigit().lineLimit(1)
            }
            .boardFont(10.5)
            if let f = fraction {
                // 막대·눈금을 Canvas 하나로 그린다 — GeometryReader 와 겹친 보기 넷보다 배치가 가볍다(보드는 매초 다시 그린다)
                Canvas { ctx, size in
                    let y: CGFloat = tick == nil ? 0 : 2, h: CGFloat = 5, w = size.width
                    ctx.fill(Path(roundedRect: CGRect(x: 0, y: y, width: w, height: h), cornerRadius: h / 2), with: .color(.white.opacity(0.08)))
                    let fw = max(3, w * CGFloat(min(max(f, 0), 1)))
                    ctx.fill(Path(roundedRect: CGRect(x: 0, y: y, width: fw, height: h), cornerRadius: h / 2), with: .color(color))
                    if let t = tick {
                        ctx.fill(Path(CGRect(x: w * CGFloat(min(max(t, 0), 1)) - 0.75, y: 0, width: 1.5, height: 9)), with: .color(.white.opacity(0.8)))
                    }
                }
                .frame(height: tick == nil ? 5 : 9)
            } else if let note {
                Text(note).boardFont(10).foregroundStyle(.secondary).monospacedDigit().lineLimit(1)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

// 작은 글자 단추(지난 실행·새 자율주행·일시정지 등) — 잠긴 단추는 흐리게, 도움말에 까닭
struct APButton: View {
    let title: String
    var icon: String? = nil
    var tint: Color? = nil
    var primary = false
    var enabled = true
    var help = ""
    var axid = ""
    let action: @MainActor () -> Void
    @State private var hover = false
    var body: some View {
        let c = tint ?? (primary ? Palette.accent : Color.white)
        Button(action: action) {
            HStack(spacing: 4) {
                if let icon { Image(systemName: icon).font(.system(size: 10, weight: .bold)) }
                Text(title).boardFont(11, weight: .semibold).lineLimit(1)
            }
            .fixedSize()
            .foregroundStyle(enabled ? (tint ?? (primary ? Palette.accent : .primary)) : .secondary)
            .padding(.horizontal, 9).frame(height: 24)
            .background(Capsule().fill(c.opacity(enabled ? (hover ? 0.24 : (primary || tint != nil ? 0.16 : 0.08)) : 0.04)))
            .overlay(Capsule().stroke(c.opacity(enabled ? 0.30 : 0.10), lineWidth: 0.5))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .modifier(ClickableIf(active: enabled, hover: $hover))
        .hint(help.isEmpty ? title : help)
        .accessibilityIdentifier(axid)
    }
}

// "← 보드" 같은 돌아가기 글자 단추
struct APBack: View {
    let title: String
    let axid: String
    let action: @MainActor () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            Text(title).boardFont(12, weight: .semibold).foregroundStyle(Palette.accent.opacity(hover ? 1 : 0.85))
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .accessibilityIdentifier(axid)
    }
}

// 실행 상태 꼬리표 — run.json state 와 지금 단위의 단계(engine.json phase)를 합친다
func apStateChip(_ r: APRun) -> (String, Color) {
    if r.active, r.pid > 0, !r.alive { return ("엔진 꺼짐", APColor.fail) }
    switch r.state {
    case "paused": return ("일시정지", APColor.pause)
    case "waiting_limit": return ("한도 대기", APColor.limit)
    case "waiting_stopline": return ("멈출 선 대기", APColor.stop)
    case "waiting_precheck": return ("환경 확인 대기", APColor.stop)
    case "done": return ("끝남", APColor.ok)
    case "stopped": return ("중단됨", APColor.muted)
    case "error": return ("오류로 멈춤", APColor.fail)
    case "starting": return ("출발 중", APColor.work)
    default:
        switch r.current?.phase {
        case "check": return ("판정 중", APColor.judge)
        case "action": return ("고정 단계", APColor.judge)
        case "limit_wait": return ("한도 대기", APColor.limit)
        default: return ("작업 중", APColor.work)
        }
    }
}

// 원래 작업 트리로 도는 경로의 표지 — 기본 보기에서도 남는다(와이어프레임 보드 본체 ①)
struct APInPlaceChip: View {
    let run: APRun
    var body: some View {
        let u = run.current
        APChip(text: "자율주행 중 · 원래 작업 트리", color: APColor.wait, strong: true)
            .hint("이 경로는 지금 자율주행이 원래 작업 트리로 쓰고 있습니다. 가지 \(run.work)"
                  + (run.originals.values.first.map { " (출발 때 \($0))" } ?? "")
                  + (u.map { " · 단위 \($0.n)/\(run.units.count) \(apStateChip(run).0)" } ?? ""))
    }
}

// 표지 줄 — 표지와 지금 걸린 가지
struct APInPlaceRow: View {
    let run: APRun
    var body: some View {
        HStack(spacing: 6) {
            APInPlaceChip(run: run)
            if !run.work.isEmpty {
                Text(run.work).boardFont(10, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
        }
    }
}

// MARK: 자율주행 · 보드 본체

// 보드 본체의 실행 줄 — 경로 카드 안(카드가 없으면 실행 카드 안). 누르면 자율주행 중 화면으로.
// 단위 시계는 보드가 매초 다시 그릴 때의 시각으로 부모가 셈한 분(minutes)으로 받는다 — 줄마다 TimelineView 를 두면 그 시계가 따로
// 보드 전체 배치를 다시 계산해 실행이 도는 동안 보드 CPU 가 약 10%p 늘었다(2026-10-27 실측). 실행 값과 분이 같으면 다시 그리지 않는다(.equatable())
struct APRunLine: View, Equatable {
    let run: APRun
    let minutes: Int        // 지금 단위의 단위 시계(분)
    var meters = true       // 계정 사용률 막대 — 자율주행 보기에서는 위 사용량 상자로 옮긴다(와이어프레임 보드 본체 ⑤)
    var compact = false     // 접힌 카드·작은 크기: 상태 꼬리표와 "2/4 제목" 한 줄
    @State private var hover = false

    static func == (a: APRunLine, b: APRunLine) -> Bool {
        a.run == b.run && a.minutes == b.minutes && a.meters == b.meters && a.compact == b.compact
    }

    var body: some View {
        Button { AutopilotStore.shared.screen = .running(run.id) } label: {
            SwiftUI.Group { if compact { line } else { full } }
                .padding(compact ? 6 : 9)
                .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hover ? 0.08 : 0.04)))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(Palette.accent.opacity(0.28), lineWidth: 0.5))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("눌러서 자율주행 중 화면으로")
        .accessibilityIdentifier("ap-runline-\(run.id)")
    }

    private var line: some View {
        let (st, c) = apStateChip(run)
        return HStack(spacing: 6) {
            APMark()
            APChip(text: st, color: c, strong: true)
            if let u = run.current {
                Text("\(u.n)/\(run.units.count)").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                Text(u.title).boardFont(11.5).lineLimit(1)
            } else {
                Text(run.reason).boardFont(11).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
        }
    }

    private var full: some View {
        let (st, c) = apStateChip(run)
        let u = run.current
        let caps = u?.caps ?? run.unitCaps ?? run.caps
        return VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 6) {
                APMark()
                Text(run.name).boardFont(12.5, weight: .bold).lineLimit(1)
                APChip(text: st, color: c, strong: true)
                APChip(text: run.modeLabel, color: run.inPlace ? APColor.wait : APColor.muted)
                Spacer(minLength: 0)
            }
            if let u {
                HStack(spacing: 6) {
                    Text("\(u.n)/\(run.units.count)").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                    if let t = u.task { APTicket(key: t) }
                    Text(u.title).boardFont(12).lineLimit(1)
                }
            } else if !run.reason.isEmpty {
                Text(run.reason).boardFont(11).foregroundStyle(.secondary).lineLimit(2)
            }
            APMeter(label: "단위 시계", value: "\(minutes)/\(Int(caps.minutes))분", fraction: Double(minutes) / max(caps.minutes, 1))
            HStack(spacing: 6) {
                Text("비용 \(apUSD(run.totalCost)) USD" + (u.map { " · 이번 단위 \(apUSD($0.cost))/\(Int(caps.usd))" } ?? "")).lineLimit(1)
                Spacer(minLength: 4)
                Text("\(run.passed) 통과 · \(run.remaining) 남음").fixedSize()
            }
            .boardFont(10.5).monospacedDigit().foregroundStyle(.secondary)
            if meters { APUsageMeters(run: run, small: true) }
        }
    }
}

// 계정 사용률 두 막대(주간·5시간)와 멈출 선 눈금 — 엔진이 run.json 에 남긴 값(stream·캐시·API 가운데 엔진이 본 것)
struct APUsageMeters: View {
    let run: APRun
    var small = false       // 실행 줄: 나란히, 초기화 시각 없이. 자율주행 중 화면: 위아래로, 초기화 시각까지
    var body: some View {
        if small {
            HStack(alignment: .top, spacing: 10) {
                meter("주간", run.seven, run.sevenResets, run.stop7)
                meter("5시간", run.five, run.fiveResets, run.stop5)
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                meter("주간", run.seven, run.sevenResets, run.stop7)
                meter("5시간", run.five, run.fiveResets, run.stop5)
            }
        }
    }
    private func meter(_ name: String, _ v: Double?, _ resets: Double?, _ line: Double) -> some View {
        let pct = v.map { Int(($0 * 100).rounded()) }
        let reset = small ? "" : resets.map { " · \(apDate($0)) 초기화" } ?? ""
        return APMeter(label: pct.map { "\(name) \($0)%" } ?? "\(name) 모름", value: "멈출 선 \(Int((line * 100).rounded()))%\(reset)",
                       fraction: v ?? 0, tick: line, color: UsagePanel.color((v ?? 0) * 100))
    }
}

// 경로 카드가 없는 실행의 카드(자율주행 세션은 세션 파일을 남기지 않아 사람 세션이 없던 경로에는 카드가 없다)와 자율주행 보기의 카드
struct APRunCard: View, Equatable {
    let run: APRun
    let minutes: Int
    var meters = true
    var body: some View {
        let path = run.repos.first ?? run.dir
        let c = apStateChip(run).1
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Image(systemName: "folder.fill")
                    .font(.system(size: 13, weight: .semibold)).foregroundStyle(c)
                    .frame(width: 30, height: 30)
                    .background(RoundedRectangle(cornerRadius: 8).fill(c.opacity(0.16)))
                VStack(alignment: .leading, spacing: 2) {
                    Text((path as NSString).lastPathComponent + (run.repos.count > 1 ? " 외 \(run.repos.count - 1)곳" : ""))
                        .boardFont(15, weight: .semibold).lineLimit(1)
                    Text(path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                        .boardFont(11).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                }
                Spacer(minLength: 0)
                HeaderButton(icon: "plus", tint: nil, help: "이 경로에서 새 세션 (\(defaultSessionCommand))") {
                    TerminalHub.shared.startSession(defaultSessionCommand, in: path)
                }
                .accessibilityIdentifier("ap-newsession-\(run.id)")
            }
            if run.inPlace { APInPlaceRow(run: run) }
            APRunLine(run: run, minutes: minutes, meters: meters).equatable()
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 16)
            .fill(LinearGradient(colors: [Palette.accent.opacity(0.08), Color.white.opacity(0.03)], startPoint: .topLeading, endPoint: .bottomTrailing)))
        .overlay(RoundedRectangle(cornerRadius: 16).stroke(Palette.accent.opacity(0.22), lineWidth: 1))
    }
}

// 보드 머리 아래 줄 — 보기 고르기(모두·기본·자율주행), 지난 실행, 새 자율주행(출발 화면은 다음 조각이라 잠금)
struct APBar: View {
    @ObservedObject var ap: AutopilotStore
    @Binding var view: String
    var body: some View {
        HStack(spacing: 6) {
            if !ap.runs.isEmpty {
                HStack(spacing: 2) {
                    seg("all", "모두")
                    seg("basic", "기본")
                    seg("auto", "자율주행 \(ap.runs.count)")
                }
                .padding(2)
                .background(Capsule().fill(Color.white.opacity(0.06)))
            }
            Spacer(minLength: 4)
            APButton(title: "지난 실행", icon: "clock.arrow.circlepath", help: "끝난 자율주행 실행을 프로젝트별로 봅니다", axid: "ap-history") {
                AutopilotStore.shared.screen = .history(nil)
            }
            APButton(title: "새 자율주행", icon: "plus", enabled: false, help: "출발 화면은 아직 만들지 않았습니다") {}
        }
    }
    private func seg(_ v: String, _ title: String) -> some View {
        Button { withAnimation(foldSpring) { view = v } } label: {
            Text(title).boardFont(11, weight: .semibold).lineLimit(1).fixedSize()
                .foregroundStyle(view == v ? .primary : .secondary)
                .padding(.horizontal, 9).frame(height: 20)
                .background(Capsule().fill(view == v ? Color.white.opacity(0.14) : .clear))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .hint(["all": "세션과 자율주행 실행 줄을 함께", "basic": "지금 보드 그대로(실행 줄 숨김)", "auto": "자율주행 실행 줄만"][v] ?? "")
        .accessibilityIdentifier("ap-view-\(v)")
    }
}

// 원래 작업 트리로 자율주행이 도는 경로에서 보드 탭 새 세션을 열 때 경고 — 막지는 않는다(설계 2장 끝 "어디서 도는가")
@MainActor func warnAutopilotPath(_ r: APRun, dir: String, open: @escaping @MainActor () -> Void) {
    let a = NSAlert()
    a.messageText = "이 경로는 지금 자율주행이 쓰고 있습니다."
    var where_ = [dir.replacingOccurrences(of: NSHomeDirectory(), with: "~")]
    if !r.work.isEmpty { where_.append("가지 \(r.work)" + (r.originals.values.first.map { " (출발 때 \($0))" } ?? "")) }
    if let u = r.current { where_.append("단위 \(u.n)/\(r.units.count) \(apStateChip(r).0)") }
    a.informativeText = "새 세션은 같은 파일과 가지를 함께 씁니다.\n\n" + where_.joined(separator: " · ")
    a.addButton(withTitle: "그래도 열기")
    a.addButton(withTitle: "취소")
    DispatchQueue.main.async {
        MainActor.assumeIsolated {
            whenPaneSettled {
                if window.attachedSheet == nil {
                    a.beginSheetModal(for: window) { resp in MainActor.assumeIsolated { if resp == .alertFirstButtonReturn { open() } } }
                } else if a.runModal() == .alertFirstButtonReturn {
                    open()
                }
            }
        }
    }
}

// MARK: 자율주행 · 자율주행 중 화면

// 보드 쪽 전체를 바꾸는 자율주행 화면들의 틀 — 보드와 같은 바탕·여백
struct APScreenFrame<Content: View>: View {
    let mini: Bool
    @ViewBuilder let content: () -> Content
    var body: some View {
        content()
            .padding(.horizontal, mini ? 10 : 16)
            .padding(.top, mini ? 2 : 22)
            .padding(.bottom, 6)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(Backdrop())
            .preferredColorScheme(.dark)
    }
}

struct APRunningScreen: View {
    @ObservedObject var ap: AutopilotStore
    let id: String
    @ObservedObject private var fit = BoardFit.shared
    @State private var opened: Set<Int> = []    // 사람이 펼친 진행 기록 묶음
    @State private var closed: Set<Int> = []    // 사람이 접은 지금 단위 묶음

    var body: some View {
        let mini = fit.tier == .mini
        APScreenFrame(mini: mini) {
            if let run = ap.viewed, run.id == id {
                if mini { small(run) } else { large(run) }
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    APBack(title: "← 보드", axid: "ap-back") { ap.screen = .board }
                    Text("이 실행을 찾지 못했습니다. 실행 폴더가 지워졌을 수 있습니다.").boardFont(12).foregroundStyle(.secondary)
                }
            }
        }
    }

    // 작은 크기: 지금 단위 카드(제목·상태 꼬리표·단위 시계)와 단추만(와이어프레임 자율주행 중 ⑨)
    private func small(_ run: APRun) -> some View {
        let (st, c) = apStateChip(run)
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                APBack(title: "← 보드", axid: "ap-back") { ap.screen = .board }
                Text("자율주행 · \(run.name)").boardFont(12, weight: .bold).lineLimit(1)
                Spacer(minLength: 0)
                APChip(text: st, color: c, strong: true)
            }
            if let u = run.current {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 6) {
                        Text("단위 \(u.n)/\(run.units.count)").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                        Text(u.title).boardFont(12, weight: .semibold).lineLimit(1)
                    }
                    clockMeter(run, u)
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.05)))
            }
            controls(run, compact: true)
            Spacer(minLength: 0)
        }
    }

    private func large(_ run: APRun) -> some View {
        ScrollView(showsIndicators: true) {
            VStack(alignment: .leading, spacing: 12) {
                header(run)
                if let u = run.current { unitCard(run, u) }
                controls(run, compact: false)
                nextBlock(run)
                logBlock(run)
                usageBlock(run)
                guardBlock(run)
            }
            .padding(.bottom, 12)
        }
    }

    private func header(_ run: APRun) -> some View {
        let (st, c) = apStateChip(run)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                APBack(title: "← 보드", axid: "ap-back") { ap.screen = .board }
                Text("자율주행 · \(run.name)").boardFont(16, weight: .bold).lineLimit(1)
                Spacer(minLength: 0)
            }
            WrapRow(spacing: 6) {
                APChip(text: st, color: c, strong: true)
                APChip(text: run.modeLabel, color: run.inPlace ? APColor.wait : APColor.muted)
                if run.jira { APChip(text: "Jira 연결", color: APColor.jira) }
                if !run.work.isEmpty && !run.jira {
                    Text(run.work).boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                }
                if let s = run.started {
                    APTickText { now in
                        let end = run.ended ?? now.timeIntervalSince1970
                        return "\(apClock(s)) 출발 · \(apDur(end - s))" + (run.active ? "째" : " 걸림")
                    }
                    .boardFont(10.5).monospacedDigit().foregroundStyle(.secondary)
                }
            }
            if !run.title.isEmpty {
                Text(run.title).boardFont(11).foregroundStyle(.secondary).lineLimit(2)
            }
        }
    }

    private func clockMeter(_ run: APRun, _ u: APUnit) -> some View {
        let caps = u.caps ?? run.unitCaps ?? run.caps
        return APTick { now in
            let a = run.activeSec(u, now)
            APMeter(label: "단위 시계", value: "\(Int(a / 60)) / \(Int(caps.minutes))분", fraction: a / 60 / max(caps.minutes, 1))
        }
    }

    // 지금 단위 카드 — 단위 번호·종류·제목, Jira 작업이면 작업 키·스토리·작업 가지, 단계 꼬리표, 단위 상한 셋
    private func unitCard(_ run: APRun, _ u: APUnit) -> some View {
        let caps = u.caps ?? run.unitCaps ?? run.caps
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text("단위 \(u.n)/\(run.units.count)").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                if !u.kind.isEmpty { APChip(text: apKind(u.kind)) }
                Text(u.title).boardFont(13, weight: .bold).lineLimit(2)
            }
            if let t = u.task {
                HStack(spacing: 5) {
                    APTicket(key: t)
                    Text("스토리").boardFont(10.5).foregroundStyle(.secondary)
                    if let s = u.story { APTicket(key: s) }
                    Spacer(minLength: 4)
                    Text("\(run.repoName(u)) · \(u.branch ?? "")").boardFont(10, design: .monospaced).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.head)
                }
            }
            stageRow(run, u)
            Text("단위 상한").boardFont(10.5, weight: .semibold).foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 7) {
                clockMeter(run, u)
                APMeter(label: "비용", value: "\(apUSD(u.cost)) / \(Int(caps.usd)) USD", fraction: u.cost / max(caps.usd, 0.01))
                APMeter(label: "주간 사용", value: String(format: "+%.1f / %g%%p", u.weeklyPP, caps.pp), fraction: u.weeklyPP / max(caps.pp, 0.01))
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.white.opacity(0.05)))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(apStateChip(run).1.opacity(0.35), lineWidth: 1))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-unitcard")
    }

    private func arrow() -> some View { Text("→").boardFont(10.5).foregroundStyle(.secondary) }

    // 단계 꼬리표 — 모델 작업 → 판정(→ 스토리로 합치기), 막히면 한도 대기·멈출 선 대기·일시정지.
    // 꼬리표만 WrapRow 에 두고 설명 글은 아래 줄에 — WrapRow 는 칸마다 원래 크기로 재므로 긴 글이 그 안에 있으면 줄을 바꾸지 않고 화면을 넘는다
    @ViewBuilder private func stageRow(_ run: APRun, _ u: APUnit) -> some View {
        let note = { (s: String) in
            Text(s).boardFont(10.5).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        if !run.active || u.status != "running" {
            VStack(alignment: .leading, spacing: 4) {
                APChip(text: apUnitLabel(u.status), color: APColor.unit(u.status), strong: true)
                if !u.reason.isEmpty { note(apPlain(u.reason)) }
            }
        } else {
            switch run.state {
            case "paused", "waiting_limit", "waiting_stopline", "waiting_precheck":
                let sc = apStateChip(run)
                VStack(alignment: .leading, spacing: 4) {
                    APChip(text: sc.0, color: sc.1, strong: true)
                    note(run.reason)
                }
            default:
                let judging = u.phase == "check"
                let tries = "시도 \(u.tries + 1)/\(run.checkRetries + 1)"
                VStack(alignment: .leading, spacing: 4) {
                    WrapRow(spacing: 6) {
                        APChip(text: u.phase == "action" ? "고정 단계" : "모델 작업", color: judging ? APColor.muted : APColor.work, strong: !judging)
                        arrow()
                        APChip(text: "판정", color: judging ? APColor.judge : APColor.muted, strong: judging)
                        if run.jira { arrow(); APChip(text: "스토리로 합치기") }
                        if !judging { Text(tries).boardFont(10.5).foregroundStyle(.secondary).fixedSize() }
                    }
                    if judging {
                        let since = ap.events.last { $0.text.hasPrefix("판정 명령을 돌립니다") && $0.ts >= (u.started ?? 0) }?.ts
                        APTick { now in
                            note("\(u.check.count > 40 ? String(u.check.prefix(40)) + "…" : u.check) 돌리는 중"
                                 + (since.map { " · \(apDur(now.timeIntervalSince1970 - $0))" } ?? "")
                                 + " · 판정 시간 상한 \(Int(run.checkTimeout))분")
                        }
                    } else if u.tries > 0 {
                        note("판정 \(u.tries)번째 실패 뒤 다시 맡겼습니다")
                    }
                }
            }
        }
    }

    // 단추: 일시정지(또는 재개)·중단(확인 없이, 사용자 결정)·멈추고 다시 출발(엔진과 함께 바뀌는 다음 조각이라 잠금)
    private func controls(_ run: APRun, compact: Bool) -> some View {
        let wait = ap.sent[run.id]
        let can = run.active && wait == nil
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if run.state == "paused" {
                    APButton(title: "재개", primary: true, enabled: can, help: "같은 세션으로 이어 갑니다", axid: "ap-resume") { ap.send("resume", run) }
                } else {
                    APButton(title: "일시정지", enabled: can, help: "지금 단위를 멈추고 쉽니다. 재개하면 같은 세션으로 이어 갑니다", axid: "ap-pause") {
                        ap.send("pause", run)
                    }
                }
                APButton(title: "중단", tint: APColor.fail, enabled: can, help: "실행을 마칩니다. 다시 띄우지 않습니다", axid: "ap-stop") {
                    ap.send("stop", run)
                }
                APButton(title: "멈추고 다시 출발", enabled: false, help: "아직 만들지 않았습니다(출발 화면과 함께)") {}
                if !compact {
                    Spacer(minLength: 4)
                    Text("실행 합계 \(apUSD(run.totalCost)) USD").boardFont(10.5).monospacedDigit().foregroundStyle(.secondary).fixedSize()
                }
            }
            if let w = wait {
                Text("\(["pause": "일시정지", "resume": "재개", "stop": "중단"][w.action] ?? w.action) 요청을 보냈습니다. 엔진이 받기를 기다립니다…")
                    .boardFont(10.5).foregroundStyle(APColor.stop)
                    .accessibilityIdentifier("ap-sent")
            } else if !run.active {
                HStack(spacing: 6) {
                    Text("이 실행은 끝났습니다.").boardFont(10.5).foregroundStyle(.secondary)
                    APBack(title: "지난 실행에서 보기 →", axid: "ap-to-history") { ap.screen = .history(run.id) }
                }
            }
        }
        .padding(compact ? 0 : 10)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.white.opacity(compact ? 0 : 0.04)))
    }

    private func blockTitle(_ s: String, _ note: String = "") -> some View {
        HStack(spacing: 6) {
            Text(s).boardFont(11.5, weight: .bold)
            if !note.isEmpty { Text(note).boardFont(10).foregroundStyle(.secondary).lineLimit(1) }
        }
    }

    // 다음 단위 — 예상 시간, Jira 면 작업 키·스토리·저장소
    @ViewBuilder private func nextBlock(_ run: APRun) -> some View {
        let next = run.units.filter { $0.status == "pending" && $0.n != run.unit }
        if !next.isEmpty {
            VStack(alignment: .leading, spacing: 5) {
                blockTitle("다음")
                ForEach(next) { u in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text("\(u.n)/\(run.units.count)").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                            if let t = u.task { APTicket(key: t) }
                            Text(u.title).boardFont(11.5).lineLimit(1)
                            if !u.kind.isEmpty { APChip(text: apKind(u.kind)) }
                        }
                        let note = nextNote(run, u)
                        if !note.isEmpty { Text(note).boardFont(10).foregroundStyle(.secondary).lineLimit(1).padding(.leading, 26) }
                    }
                }
            }
        }
    }

    private func nextNote(_ run: APRun, _ u: APUnit) -> String {
        var s: [String] = []
        if let st = u.story { s.append("스토리 \(st) · \(run.repoName(u))") } else if !u.tickets.isEmpty { s.append("티켓 \(u.tickets.joined(separator: ", "))") }
        if let e = u.estMin { s.append("예상 \(Int(e))분") }
        return s.joined(separator: " · ")
    }

    // 진행 기록 — 단위별로 접고 지금 단위만 펼친다. 최근 것이 위(와이어프레임 자율주행 중 ⑤)
    private func logBlock(_ run: APRun) -> some View {
        let groups = AutopilotStore.groups(run, ap.events).reversed()
        return VStack(alignment: .leading, spacing: 5) {
            blockTitle("진행 기록", "단위별로 접음 · 지금 단위만 펼침 · 최근 것이 위")
            if ap.events.isEmpty {
                Text("아직 기록이 없습니다").boardFont(11).foregroundStyle(.secondary)
            }
            ForEach(Array(groups)) { g in
                let isOpen = g.id == run.unit && run.active ? !closed.contains(g.id) : opened.contains(g.id)
                VStack(alignment: .leading, spacing: 3) {
                    Button {
                        withAnimation(foldSpring) {
                            if g.id == run.unit && run.active { closed.formSymmetricDifference([g.id]) } else { opened.formSymmetricDifference([g.id]) }
                        }
                    } label: { groupHead(run, g, open: isOpen) }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("ap-log-\(g.id)")
                    if isOpen {
                        let lines = g.lines.suffix(300).reversed()
                        ForEach(Array(lines.enumerated()), id: \.offset) { _, e in logLine(e) }
                        if g.lines.count > 300 {
                            Text("앞의 \(g.lines.count - 300)줄은 줄였습니다").boardFont(10).foregroundStyle(.secondary).padding(.leading, 16)
                        }
                    }
                }
            }
        }
    }

    private func groupHead(_ run: APRun, _ g: APLogGroup, open: Bool) -> some View {
        let u = g.id >= 1 && g.id <= run.units.count ? run.units[g.id - 1] : nil
        return HStack(spacing: 6) {
            Image(systemName: "chevron.right").font(.system(size: 8, weight: .bold)).rotationEffect(.degrees(open ? 90 : 0)).frame(width: 8)
            if let u {
                Text("단위 \(u.n)").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                if let t = u.task { APTicket(key: t) }
                Text(u.title).boardFont(11.5, weight: .semibold).lineLimit(1)
                if u.status == "running", run.active {
                    let sc = apStateChip(run)
                    APChip(text: sc.0, color: sc.1, strong: true)
                } else {
                    APChip(text: apUnitLabel(u.status), color: APColor.unit(u.status), strong: u.status != "pending")
                }
                if u.story != nil, u.status != "skipped", let m = u.merge {
                    APChip(text: ["merged": "스토리로 합침", "nothing": "합칠 커밋 없음"][m] ?? "합치지 않음", color: m == "merged" ? APColor.ok : APColor.muted)
                }
            } else {
                Text(g.id == 0 ? "출발" : "끝").boardFont(11.5, weight: .semibold)
            }
            Text("\(g.lines.count)줄").boardFont(10).foregroundStyle(.secondary).fixedSize()
            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
    }

    private func logLine(_ e: APEvent) -> some View {
        let who = ["board": ("보드", APColor.board), "model": ("모델", APColor.model), "engine": ("엔진", APColor.muted)][e.who] ?? (e.who, APColor.muted)
        return HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(apClock(e.ts)).boardFont(10, design: .monospaced).foregroundStyle(.secondary).fixedSize()
            APChip(text: who.0, color: who.1)
            Text(e.who == "model" ? apPlain(e.text) : e.text).boardFont(11).fixedSize(horizontal: false, vertical: true)
        }
        .padding(.leading, 16)
    }

    private func usageBlock(_ run: APRun) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            blockTitle("계정 사용률")
            APUsageMeters(run: run)
        }
    }

    // 지킴 — 실행 중에 막혀 있는 것(엔진 거부 목록·샌드박스, 설계 4장)
    private func guardBlock(_ run: APRun) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            WrapRow(spacing: 6) {
                Text("지킴").boardFont(11.5, weight: .bold)
                if run.jira {
                    APChip(text: "커밋은 지금 작업 가지에만")
                    APChip(text: "스토리 가지·기존 가지 쓰기 금지")
                } else if !run.existing.isEmpty {
                    APChip(text: "기존 가지 \(run.existing.count)개 쓰기 금지")
                }
                APChip(text: "저장소 하네스 고치기 금지")
                APChip(text: "git push 거부")
                APChip(text: "바깥 쓰기 도구 없음")
            }
            Text(guardLine(run)).boardFont(10, design: .monospaced).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func guardLine(_ run: APRun) -> String {
        var s: [String] = []
        if run.jira {
            if let u = run.current, let b = u.branch { s.append("지금 작업 가지: \(run.repoName(u)) \(b)") }
            if !run.storyBranches.isEmpty { s.append("스토리 가지 \(run.storyBranches.joined(separator: " · "))") }
        }
        if !run.existing.isEmpty { s.append("기존 가지: \(run.existing.prefix(8).joined(separator: " · "))\(run.existing.count > 8 ? " 등" : "")") }
        if !run.jira, !run.work.isEmpty { s.append("커밋은 \(run.work) 에만") }
        return s.joined(separator: " · ")
    }
}

// MARK: 자율주행 · 지난 실행

struct APHistoryScreen: View {
    @ObservedObject var ap: AutopilotStore
    let selected: String?
    @ObservedObject private var fit = BoardFit.shared
    @State private var folded: Set<String> = []     // 접은 프로젝트 묶음
    @State private var project: String?             // 고른 프로젝트(nil 이면 모든 프로젝트)
    @State private var reports: Set<Int> = []       // 보고서를 펼친 단위

    private var shown: [APPastRun] { ap.history.runs.filter { project == nil || $0.projectName == project } }
    // 프로젝트별 묶음 — 마지막 실행이 최근인 프로젝트부터
    private var byProject: [(name: String, runs: [APPastRun])] {
        var order: [String] = [], out: [String: [APPastRun]] = [:]
        for r in shown {
            if out[r.projectName] == nil { order.append(r.projectName) }
            out[r.projectName, default: []].append(r)
        }
        return order.map { ($0, out[$0] ?? []) }
    }

    var body: some View {
        let mini = fit.tier == .mini
        APScreenFrame(mini: mini) {
            if mini {
                if let id = selected, let d = ap.detail, d.run.id == id { smallDetail(d) } else { list(mini: true) }
            } else {
                ScrollView(showsIndicators: true) {
                    VStack(alignment: .leading, spacing: 12) {
                        list(mini: false)
                        if let d = ap.detail { detail(d) }
                    }
                    .padding(.bottom, 12)
                }
            }
        }
        .onChange(of: selected) { _, _ in reports = [] }
    }

    private func notices() -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if ap.history.version > APRecords.knownVersion {
                Label("기록 저장소 판(\(ap.history.version))이 이 보드가 아는 판(\(APRecords.knownVersion))보다 새롭습니다. 보드를 업데이트하세요.",
                      systemImage: "exclamationmark.triangle.fill")
                    .boardFont(11).foregroundStyle(APColor.limit)
            }
            if let e = ap.history.error {
                Text("기록 저장소를 다 읽지 못했습니다: \(e)").boardFont(11).foregroundStyle(APColor.fail).lineLimit(3)
            }
        }
    }

    private func list(mini: Bool) -> some View {
        VStack(alignment: .leading, spacing: mini ? 4 : 8) {
            HStack(spacing: 8) {
                APBack(title: "← 보드", axid: "ap-back") { ap.screen = .board }
                Text("지난 실행").boardFont(mini ? 12 : 16, weight: .bold)
                if mini { Text("프로젝트별").boardFont(10.5).foregroundStyle(.secondary) }
                Spacer(minLength: 0)
                if !mini { projectMenu }
            }
            notices()
            if ap.history.runs.isEmpty {
                Text("끝난 자율주행 실행이 아직 없습니다").boardFont(11.5).foregroundStyle(.secondary)
            } else if !mini {
                HStack(spacing: 6) {
                    Text("실행 \(shown.count)개").boardFont(11.5, weight: .bold)
                    Text("프로젝트별로 묶음").boardFont(10).foregroundStyle(.secondary)
                }
            }
            if mini {
                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 3) {
                        ForEach(byProject, id: \.name) { p in
                            Text("\(p.name) · 실행 \(p.runs.count)").boardFont(10.5, weight: .semibold).foregroundStyle(.secondary).padding(.top, 3)
                            ForEach(p.runs) { r in miniRow(r) }
                        }
                    }
                }
            } else {
                ForEach(byProject, id: \.name) { p in projectGroup(p.name, p.runs) }
            }
        }
    }

    private var projectMenu: some View {
        Menu {
            Button("모든 프로젝트") { project = nil }
            ForEach(Array(Set(ap.history.runs.map(\.projectName))).sorted(), id: \.self) { n in Button(n) { project = n } }
        } label: {
            Text(project ?? "모든 프로젝트").boardFont(11, weight: .semibold)
        }
        .menuStyle(.button)
        .buttonStyle(.borderless)
        .fixedSize()
        .accessibilityIdentifier("ap-project-menu")
    }

    private func projectGroup(_ name: String, _ runs: [APPastRun]) -> some View {
        let open = !folded.contains(name)
        let jira = runs.contains(where: \.jira)
        let epic = runs.compactMap(\.epic).first
        return VStack(alignment: .leading, spacing: 4) {
            Button { withAnimation(foldSpring) { folded.formSymmetricDifference([name]) } } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right").font(.system(size: 8, weight: .bold)).rotationEffect(.degrees(open ? 90 : 0)).frame(width: 8)
                    Text(name).boardFont(12.5, weight: .bold).lineLimit(1)
                    Text("실행 \(runs.count) · 마지막 \(apDay(runs.first?.started))").boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                    if jira { APChip(text: "Jira 연결" + (epic.map { " · 에픽 \($0)" } ?? ""), color: APColor.jira) }
                    Spacer(minLength: 0)
                    if !open, let m = runs.first?.mergeLabel { APChip(text: m.0, color: m.1, strong: true) }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("ap-project-\(name)")
            if open {
                ForEach(runs) { r in row(r) }
            }
        }
    }

    private func isOn(_ r: APPastRun) -> Bool { (selected ?? ap.history.runs.first?.id) == r.id }

    private func row(_ r: APPastRun) -> some View {
        Button { ap.screen = .history(r.id) } label: {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(apDate(r.started)).boardFont(11.5, weight: .semibold).monospacedDigit().fixedSize()
                    if !r.mode.isEmpty { APChip(text: r.mode == "worktree" ? "따로 작업 트리" : "원래 작업 트리") }
                    if r.jira { APChip(text: "스토리 \(r.stories)") }
                    Spacer(minLength: 0)
                    if let s = r.stateLabel { APChip(text: s.0, color: s.1, strong: true) }
                    if let m = r.mergeLabel { APChip(text: m.0, color: m.1, strong: true) }
                }
                HStack(spacing: 10) {
                    Text(r.result)
                    Text(apDur(r.activeSec))
                    Text("\(apUSD(r.cost)) USD")
                    Text(r.harnessLabel).foregroundStyle(r.harnessSame == false ? APColor.limit : .secondary)
                }
                .boardFont(10.5).monospacedDigit().foregroundStyle(.secondary).lineLimit(1)
            }
            .padding(8)
            .background(RoundedRectangle(cornerRadius: 9).fill(Color.white.opacity(isOn(r) ? 0.10 : 0.03)))
            .overlay(RoundedRectangle(cornerRadius: 9).stroke(isOn(r) ? Palette.accent.opacity(0.45) : .clear, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("ap-hist-\(r.id)")
    }

    private func miniRow(_ r: APPastRun) -> some View {
        Button { ap.screen = .history(r.id) } label: {
            HStack(spacing: 6) {
                Text("\(apDate(r.started)) · \(r.total)개 중 \(r.done)개 통과").boardFont(11).monospacedDigit().lineLimit(1)
                Spacer(minLength: 0)
                if let m = r.mergeLabel { APChip(text: m.0, color: m.1, strong: true) } else if let s = r.stateLabel { APChip(text: s.0, color: s.1, strong: true) }
            }
            .padding(.horizontal, 6).frame(height: miniRowHeight - 2)
            .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(0.04)))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("ap-hist-\(r.id)")
    }

    // 작은 크기의 상세 — 단위와 판정만, 위에 "← 목록"
    private func smallDetail(_ d: APPastDetail) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            APBack(title: "← 목록", axid: "ap-back-list") { ap.screen = .history(nil) }
            HStack(spacing: 6) {
                Text("\(apDay(d.run.started)) · \(d.run.projectName)").boardFont(12, weight: .bold).lineLimit(1)
                Spacer(minLength: 0)
                if let m = d.run.mergeLabel { APChip(text: m.0, color: m.1, strong: true) }
            }
            ScrollView(showsIndicators: false) {
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(d.units) { u in
                        HStack(spacing: 6) {
                            if let t = u.task { APTicket(key: t) }
                            Text(u.title).boardFont(11).lineLimit(1)
                            Spacer(minLength: 4)
                            APChip(text: apUnitLabel(u.status), color: APColor.unit(u.status), strong: true)
                        }
                    }
                }
            }
            lockedButtons
        }
    }

    // 이어 가기·보고서 보기 — 다음 조각(출발 화면, 3단계 보고서 틀)이라 잠가 둔다
    private var lockedButtons: some View {
        HStack(spacing: 6) {
            APButton(title: "이 실행 이어 가기", primary: true, enabled: false, help: "아직 만들지 않았습니다(출발 화면과 함께)") {}
            APButton(title: "보고서 보기", enabled: false, help: "아직 만들지 않았습니다(보고서 틀과 함께)") {}
        }
    }

    // 고른 실행의 단위별 상세 — 예상·실제·비용·판정(Jira 면 스토리별 묶음과 "스토리로"), 단위를 누르면 결과 기록(보고서)
    private func detail(_ d: APPastDetail) -> some View {
        let r = d.run
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Text("\(apDay(r.started)) · \(r.projectName) 단위별").boardFont(12.5, weight: .bold).lineLimit(1)
                if r.jira { Text("스토리별로 묶음").boardFont(10).foregroundStyle(.secondary).fixedSize() }
            }
            if !r.title.isEmpty { Text(r.title).boardFont(11).foregroundStyle(.secondary).lineLimit(2) }
            // 넓으면 표(와이어프레임 지난 실행 ②), 보드 폭이 좁으면 단위마다 두 줄 — 일곱 칸 표는 360pt 에서 제목 칸이 한 글자로 눌렸다
            ViewThatFits(in: .horizontal) {
                unitTable(d)
                unitRows(d)
            }
            harnessLine(r)
            carryBlock(d)
            lockedButtons
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.white.opacity(0.04)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-detail")
    }

    private func cell(_ s: String, mono: Bool = false) -> some View {
        Text(s).boardFont(10.5, design: mono ? .monospaced : .default).monospacedDigit().lineLimit(1).fixedSize()
    }

    private func judge(_ u: APPastUnit) -> some View {
        HStack(spacing: 3) {
            APChip(text: apUnitLabel(u.status), color: APColor.unit(u.status), strong: true)
            if u.status == "done", u.tries > 0 { Text("\(u.tries + 1)번째").boardFont(10).foregroundStyle(.secondary).fixedSize() }
            if u.status == "failed" { Text("\(u.tries + 1)번").boardFont(10).foregroundStyle(.secondary).fixedSize() }
        }
    }

    private func toStory(_ u: APPastUnit) -> some View {
        SwiftUI.Group {
            if u.status == "skipped" {
                Text(u.dependsOn.isEmpty ? "건너뜀" : "\(u.dependsOn.joined(separator: ", ")) 에 기댐").boardFont(10).foregroundStyle(.secondary).fixedSize()
            } else {
                let m = u.merged ?? "not_merged"
                APChip(text: ["merged": "합침", "nothing": "합칠 커밋 없음"][m] ?? "합치지 않음", color: m == "merged" ? APColor.ok : APColor.muted)
            }
        }
    }

    // Jira 실행은 스토리 키 차례(처음 나온 차례)로 묶는다. 스토리 없는 단위는 "스토리 없음"
    private func storyGroups(_ d: APPastDetail) -> [(key: String?, units: [APPastUnit])] {
        guard d.run.jira else { return [(nil, d.units)] }
        var stories: [String] = []
        for u in d.units where !stories.contains(u.story ?? "") { stories.append(u.story ?? "") }
        return stories.map { k in (k.isEmpty ? nil : k, d.units.filter { ($0.story ?? "") == k }) }
    }

    private func reportToggle(_ u: APPastUnit, lines: Int) -> some View {
        Button { withAnimation(foldSpring) { reports.formSymmetricDifference([u.n]) } } label: {
            HStack(spacing: 4) {
                Image(systemName: "chevron.right").font(.system(size: 7, weight: .bold))
                    .rotationEffect(.degrees(reports.contains(u.n) ? 90 : 0))
                Text(u.title).boardFont(11).lineLimit(lines)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .hint("눌러서 결과 기록 보기")
        .accessibilityIdentifier("ap-unit-\(u.n)")
    }

    // 좁은 보드의 단위별 상세 — 단위마다 두 줄(작업 키·제목 / 예상·실제·비용·판정·스토리로)
    private func unitRows(_ d: APPastDetail) -> some View {
        let jira = d.run.jira
        let est = d.units.compactMap(\.estMin).reduce(0, +)
        return VStack(alignment: .leading, spacing: 7) {
            ForEach(Array(storyGroups(d).enumerated()), id: \.offset) { _, g in
                if jira {
                    HStack(spacing: 5) {
                        if let k = g.key { APTicket(key: k) } else { Text("스토리 없음").boardFont(10.5, weight: .semibold) }
                        Text(storyNote(g.key, g.units)).boardFont(10.5).foregroundStyle(.secondary)
                    }
                    .padding(.top, 3)
                }
                ForEach(g.units) { u in
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 5) {
                            SwiftUI.Group { if let t = u.task { cell(t, mono: true) } else { cell("\(u.n)", mono: true) } }.foregroundStyle(.secondary)
                            reportToggle(u, lines: 2)
                        }
                        WrapRow(spacing: 6) {
                            Text("예상 \(u.estMin.map { apDur($0 * 60) } ?? "모름") · 실제 \(apDur(u.activeSec)) · \(apUSD(u.cost)) USD")
                                .boardFont(10.5).monospacedDigit().foregroundStyle(.secondary).fixedSize()
                            judge(u)
                            if jira { toStory(u) }
                        }
                        .padding(.leading, 14)
                        if reports.contains(u.n) { APReport(unit: u).padding(.leading, 14) }
                    }
                }
            }
            Divider()
            Text("합계 · 예상 \(est > 0 ? apDur(est * 60) : "모름") · 실제 \(apDur(d.units.reduce(0) { $0 + $1.activeSec })) · \(apUSD(d.units.reduce(0) { $0 + $1.cost })) USD")
                .boardFont(10.5, weight: .semibold).monospacedDigit()
        }
    }

    private func unitTable(_ d: APPastDetail) -> some View {
        let jira = d.run.jira
        let groups = storyGroups(d)
        let est = d.units.compactMap(\.estMin).reduce(0, +)
        return Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 5) {
            GridRow {
                head(jira ? "작업" : "")
                head("단위")
                head("예상").gridColumnAlignment(.trailing)
                head("실제").gridColumnAlignment(.trailing)
                head("비용 USD").gridColumnAlignment(.trailing)
                head("판정")
                if jira { head("스토리로") }
            }
            ForEach(Array(groups.enumerated()), id: \.offset) { _, g in
                if jira {
                    // 스토리 머리 줄 — 칸 밖 보기는 Grid 의 모든 열을 차지한다
                    HStack(spacing: 5) {
                        if let k = g.key { APTicket(key: k) } else { Text("스토리 없음").boardFont(10.5, weight: .semibold) }
                        Text(storyNote(g.key, g.units)).boardFont(10.5).foregroundStyle(.secondary)
                    }
                    .padding(.top, 4)
                }
                ForEach(g.units) { u in
                    GridRow {
                        SwiftUI.Group { if let t = u.task { cell(t, mono: true) } else { cell("\(u.n)", mono: true) } }.foregroundStyle(.secondary)
                        reportToggle(u, lines: 1)
                            .frame(minWidth: 140, maxWidth: .infinity, alignment: .leading)
                        cell(u.estMin.map { apDur($0 * 60) } ?? "모름")
                        cell(apDur(u.activeSec))
                        cell(apUSD(u.cost))
                        judge(u)
                        if jira { toStory(u) }
                    }
                    if reports.contains(u.n) {
                        APReport(unit: u).padding(.leading, 16)
                    }
                }
            }
            Divider().gridCellUnsizedAxes(.horizontal)
            GridRow {
                Text("")
                Text("합계").boardFont(10.5, weight: .semibold)
                cell(est > 0 ? apDur(est * 60) : "모름")
                cell(apDur(d.units.reduce(0) { $0 + $1.activeSec }))
                cell(apUSD(d.units.reduce(0) { $0 + $1.cost }))
                Text("")
                if jira { Text("") }
            }
        }
    }

    private func head(_ s: String) -> some View {
        Text(s).boardFont(10, weight: .semibold).foregroundStyle(.secondary).lineLimit(1).fixedSize()
    }

    // 스토리 머리 줄 — 종류 · 합친 작업 수
    private func storyNote(_ key: String?, _ units: [APPastUnit]) -> String {
        let kind = units.first.map { apKind($0.kind) } ?? ""
        let merged = units.filter { $0.merged == "merged" }.count
        return [kind, key == nil ? "" : "합친 작업 \(merged)/\(units.count)"].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    // 하네스가 출발 때와 같은지 — 다르면 바뀐 파일(설계 2장 끝 "하네스 사본과 지문")
    private func harnessLine(_ r: APPastRun) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            if let same = r.harnessSame {
                if same {
                    Label("하네스가 출발 때와 같습니다(파일 \(r.harnessFiles)개)", systemImage: "checkmark.shield")
                        .boardFont(11).foregroundStyle(APColor.ok)
                } else {
                    Label("하네스가 출발 때와 다릅니다(\(r.harnessChanged.count)곳). 실행 중에 사람이 고쳤을 수 있어 되돌리지 않았습니다.",
                          systemImage: "exclamationmark.shield")
                        .boardFont(11).foregroundStyle(APColor.limit)
                    ForEach(r.harnessChanged.prefix(10), id: \.path) { c in
                        Text("\(["changed": "바뀜", "added": "더함", "removed": "없어짐"][c.change] ?? c.change) · \(c.path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))")
                            .boardFont(10, design: .monospaced).foregroundStyle(.secondary).padding(.leading, 18)
                    }
                }
            } else {
                Text("하네스 기록 없음(하네스 지문 전 실행이거나 끝 검사를 하지 못함)").boardFont(11).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-harness")
    }

    // 다음 출발로 넘어갈 것 — 미룬 사람 몫(다음 출발 카드), 새로 찾은 일(대기열 후보), Jira 면 열린 채 남은 티켓
    @ViewBuilder private func carryBlock(_ d: APPastDetail) -> some View {
        let parked = d.carry.filter { $0.kind == "next_start" }
        let found = d.carry.filter { $0.kind == "new_work" }
        let open = d.units.filter { $0.task != nil && $0.status != "done" }
        if !parked.isEmpty || !found.isEmpty || !open.isEmpty {
            VStack(alignment: .leading, spacing: 5) {
                Text("다음 출발로 넘어갈 것").boardFont(11.5, weight: .bold)
                if d.run.jira, !open.isEmpty {
                    Text("열린 채 남은 티켓 \(open.count)").boardFont(10.5, weight: .semibold).foregroundStyle(.secondary)
                    ForEach(open) { u in
                        HStack(spacing: 5) {
                            if let t = u.task { APTicket(key: t) }
                            Text(u.title).boardFont(11).lineLimit(1)
                            APChip(text: apUnitLabel(u.status), color: APColor.unit(u.status), strong: true)
                        }
                    }
                    Text("끝까지 가지 못한 단위의 티켓은 닫지 않고 둡니다.").boardFont(10).foregroundStyle(.secondary)
                }
                if !parked.isEmpty {
                    Text("미룬 사람 몫 \(parked.count)").boardFont(10.5, weight: .semibold).foregroundStyle(.secondary)
                    ForEach(parked) { c in carryLine(c) }
                }
                if !found.isEmpty {
                    Text("새로 찾은 일 \(found.count)").boardFont(10.5, weight: .semibold).foregroundStyle(.secondary)
                    ForEach(found) { c in carryLine(c) }
                }
            }
        }
    }

    private func carryLine(_ c: APCarry) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text("· \(c.title)").boardFont(11).fixedSize(horizontal: false, vertical: true)
            if !c.why.isEmpty { Text(c.why).boardFont(10).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.leading, 10) }
        }
    }
}

// 단위의 결과 기록(result.json, 기록 저장소에 전문) — 한 문장 결론, 한 일, 시험, 바꾼 것, 위험. Jira 작업이면 작업 가지와 합친 결과
struct APReport: View {
    let unit: APPastUnit
    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            if let r = unit.result {
                if !r.headline.isEmpty { Text(r.headline).boardFont(11, weight: .semibold).fixedSize(horizontal: false, vertical: true) }
                ForEach(Array(r.summary.enumerated()), id: \.offset) { _, s in line("· \(s)") }
                if !r.tests.isEmpty { line("시험: " + r.tests.joined(separator: ", ")) }
                if !r.changes.isEmpty { line("바꾼 것: " + r.changes.joined(separator: ", ")) }
                ForEach(Array(r.risks.enumerated()), id: \.offset) { _, s in line("알아 둘 위험: \(s)").foregroundStyle(APColor.limit) }
            } else {
                line("결과 기록이 없습니다")
            }
            if !unit.branch.isEmpty { Text("가지 \(unit.branch)").boardFont(10, design: .monospaced).foregroundStyle(.secondary) }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.05)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-report-\(unit.n)")
    }
    private func line(_ s: String) -> some View {
        Text(s).boardFont(10.5).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}

// MARK: - 창

var window: NSWindow!

// 보드가 다른 앱 뒤에 있을 때도 첫 클릭이 창 활성화에만 쓰이지 않고 바로 단추에 닿게 한다
final class FirstClickHostingView<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

// 창을 손으로 줄일 때 최소 크기 아래로 내려가지 않게 한다
final class SizeGuard: NSObject, NSWindowDelegate {
    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        let min = sender.frameRect(forContentRect: NSRect(origin: .zero, size: sender.contentMinSize)).size
        return NSSize(width: max(frameSize.width, min.width), height: max(frameSize.height, min.height))
    }
    // 초록 단추(확대) — 터미널을 펼쳤으면 제목 막대 더블클릭과 같은 창 크기 전환. 접혔으면 시스템 확대 그대로
    func windowShouldZoom(_ window: NSWindow, toFrame newFrame: NSRect) -> Bool {
        return MainActor.assumeIsolated {
            guard TerminalHub.shared.expanded else { return true }
            toggleWindowSize()
            return false
        }
    }
}
let sizeGuard = SizeGuard()
let boardMinWidth: CGFloat = 360        // 전체·중간 화면이 무너지지 않는 폭 — 더 좁으면 미니로 그린다
let boardLaunchWidth: CGFloat = 454   // 실행할 때의 가로 폭 — 2026-10-13 사용자가 맞춰 둔 폭

// MARK: - 창 크기 단계

// 창 크기에 따라 보드를 세 단계로 그린다.
// 전체 = 기본 화면, 중간 = 사용 한도는 한 줄·경로 카드는 접힌 한 줄 요약, 미니 = 캐릭터 한 줄과 경로마다 한 줄.
// 제목 오른쪽 캐릭터는 세 단계 모두에서 보인다
enum BoardTier { case full, medium, mini }

let miniWidth: CGFloat = 300            // 최대축소 폭이자 창의 최소 폭
let miniMinHeight: CGFloat = 144        // 창의 최소 높이 — 미니에서 캐릭터 줄·경로 두 줄·아래 줄이 보이는 높이
let miniRowHeight: CGFloat = 26         // 미니의 경로 한 줄 높이 — 최대축소할 때 경로 수만큼 창 높이를 잡는다
let fullListMin: CGFloat = 120          // 전체로 그리려면 남아야 하는 목록 칸 — 펼친 카드 하나쯤
let mediumListMin: CGFloat = 56         // 중간으로 그리려면 남아야 하는 목록 칸 — 접힌 카드 하나

@MainActor final class BoardFit: ObservableObject {
    static let shared = BoardFit()
    @Published private(set) var tier: BoardTier = .full
    // 단계마다 목록 칸을 뺀 보드 내용 높이 — 그 단계로 그려졌을 때만 잴 수 있어 잰 값을 기억한다(재기 전엔 어림값)
    private var fixed: [BoardTier: CGFloat] = [.full: 330, .medium: 200]

    // 그려진 단계의 보드 높이와 목록 칸 높이를 받는다 — 지금 단계로 그린 값만 쓴다
    func measured(_ t: BoardTier, board: CGFloat, list: CGFloat) {
        guard t == tier, t != .mini, board > 0, abs((fixed[t] ?? 0) - (board - list)) > 0.5 else { return }
        fixed[t] = board - list
        update()
    }

    // 창 크기나 잰 값이 바뀌면 단계를 다시 고른다 — 창 크기는 바꾸지 않는다
    func update() {
        guard let w = window else { return }
        let h = w.frame.height, titlebar = h - w.contentLayoutRect.height   // 보드 내용은 제목 막대 아래에서 시작한다
        let fits = { (t: BoardTier, list: CGFloat) in h >= (self.fixed[t] ?? 0) + list + titlebar }
        let t: BoardTier = boardPaneWidth() < boardMinWidth ? .mini   // 터미널을 펼쳤으면 창이 아니라 보드 칸의 폭
            : fits(.full, fullListMin) ? .full : fits(.medium, mediumListMin) ? .medium : .mini
        guard t != tier else { return }
        // 미니를 벗어남 — 되돌리기 단추가 아닌 길(손으로 키움·펼치기·처음 크기로 붙이기)이었으면 최대축소 때의 펼침 기억을 지운다
        if tier == .mini { UserDefaults.standard.removeObject(forKey: "miniFromExpanded") }
        tier = t
        UserDefaults.standard.set(t == .mini, forKey: "boardMini")   // 미니로 둔 채 끄면 다음에도 그 크기로 연다
    }
}

// 미니에서 ⤢ 로 돌아갈 창 위치 — 최대축소할 때, 미니가 아닌 크기로 손으로 바꾼 뒤, 처음 크기로 붙일 때, 미니가 아닌 채 실행할 때 기억한다.
// 창이 줄어드는 애니메이션 중간 크기가 기억되지 않게 크기가 바뀔 때마다 기억하지는 않는다
func saveRestoreFrame() { UserDefaults.standard.set(NSStringFromRect(window.frame), forKey: "boardRestoreFrame") }

// 최대축소 — 보드 쪽 위 모서리(보드 위치가 오른쪽이면 오른쪽 위, 왼쪽이면 왼쪽 위)는 그대로 두고 폭은 miniWidth, 높이는 경로 수(최대 6줄)에 맞춘다
@MainActor func enterMini(rows: Int) {
    // 펼쳐 있었으면 기억해 두었다가 원래 크기로 되돌릴 때 다시 펼친다(다시 띄워도 맞게 보드 설정에). 손으로 미니를 벗어나면 지운다(BoardFit)
    UserDefaults.standard.set(TerminalHub.shared.expanded, forKey: "miniFromExpanded")
    setTerminalExpanded(false, animated: false)   // 터미널을 펼쳤으면 먼저 접는다 — 미니에서 돌아갈 위치는 접힌 보드
    saveRestoreFrame()
    let f = window.frame
    // 최소 높이에 두 줄이 들어간다. 보드 글자를 키웠으면 그만큼 높인다(150% 에서 둘째 줄이 아래 줄에 가려졌다)
    let h = (miniMinHeight + CGFloat(min(max(rows, 2), 6) - 2) * miniRowHeight) * max(1, BoardFontScale.shared.scale)
    let x = TerminalHub.shared.boardOnLeft ? f.minX : f.maxX - miniWidth
    window.setFrame(NSRect(x: x, y: f.maxY - h, width: miniWidth, height: h), display: true, animate: true)
}

// 미니에서 원래 크기로 — 최대축소할 때 터미널을 펼쳐 있었으면 다시 펼친다. 아니면 접힌 보드 위치로(기억한 위치가 없거나
// 지금 연결된 화면 밖이면 처음 크기로 보드 쪽 끝에 붙인다)
@MainActor func leaveMini() {
    if UserDefaults.standard.bool(forKey: "miniFromExpanded") {
        UserDefaults.standard.removeObject(forKey: "miniFromExpanded")
        setTerminalExpanded(true)
        return
    }
    if let s = UserDefaults.standard.string(forKey: "boardRestoreFrame") {
        let r = NSRectFromString(s)
        if r.width >= boardMinWidth, NSScreen.screens.contains(where: { $0.visibleFrame.intersects(r) }) {
            window.setFrame(r, display: true, animate: true)
            return
        }
    }
    snapToSide()
}

// 제목 막대 오른쪽 — 미니에서는 상태별 세션 수와 원래 크기로 되돌리기 단추, 전체·중간에서는 같은 자리에 최대축소 단추
func addMiniTitlebar(_ store: Store) {
    let vc = NSTitlebarAccessoryViewController()
    let host = FirstClickHostingView(rootView: MiniTitleBar(store: store))
    host.frame = NSRect(x: 0, y: 0, width: MiniTitleBar.width, height: 28)   // 제목 막대 높이와 같게 — 더 높으면 제목 막대가 커진다
    vc.view = host
    vc.layoutAttribute = .trailing
    window.addTitlebarAccessoryViewController(vc)
}

struct MiniTitleBar: View {
    static let width: CGFloat = 176     // 미니 폭 300 에서 창 단추·붙이기 단추를 뺀 자리
    @ObservedObject var store: Store
    @ObservedObject private var fit = BoardFit.shared
    @State private var hover = false

    // 상태별 세션 수 — 자리가 모자라면 글자를 빼고 수만
    private func chips(labels: Bool) -> some View {
        HStack(spacing: 4) {
            ForEach(["waiting", "working", "idle"], id: \.self) { st in
                let n = store.count(st)
                if n > 0 {
                    HStack(spacing: 3) {
                        Circle().fill(Palette.color(st)).frame(width: 6, height: 6)
                        Text(labels ? "\(n) \(["waiting": "승인", "working": "작업", "idle": "대기"][st] ?? "")" : "\(n)")
                            .monospacedDigit()
                    }
                    .font(.system(size: 10.5, weight: .bold))
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(Capsule().fill(Palette.color(st).opacity(0.15)))
                    .fixedSize()
                    .hint("\(Palette.name(st)) \(n)개")
                }
            }
        }
    }

    var body: some View {
        let mini = fit.tier == .mini
        HStack(spacing: 5) {
            Spacer(minLength: 0)
            if mini {
                ViewThatFits(in: .horizontal) { chips(labels: true); chips(labels: false) }
            }
            // 미니 = 원래 크기로, 전체·중간 = 최대축소(창을 가장 작게 줄여 경로마다 한 줄로 본다)
            Button { if mini { leaveMini() } else { enterMini(rows: store.groups.count) } } label: {
                Image(systemName: mini ? "arrow.up.left.and.arrow.down.right" : "arrow.down.right.and.arrow.up.left")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(hover ? .primary : .secondary)
                    .frame(width: 24, height: 20)
                    .background(Capsule().fill(Color.white.opacity(hover ? 0.16 : 0.08)))
                    .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .clickable($hover)
            .hint(mini ? "원래 크기로 되돌리기" : "최대축소: 창을 가장 작게 줄이고 경로마다 한 줄로 봅니다")
        }
        .padding(.trailing, 8)
        .frame(width: Self.width, height: 28)
        .preferredColorScheme(.dark)
    }
}

// 미니 화면 — 경로마다 한 줄(상태 · 이름 · 지금 하는 일), 아래 줄에 사용 한도와 투명도 슬라이더
struct MiniBoard: View {
    @ObservedObject var store: Store
    @ObservedObject var usage: UsageStore
    @ObservedObject private var autopilot = AutopilotStore.shared

    // 사용 한도 한 줄 — 앞에서 n 개만
    private func limits(_ n: Int) -> some View {
        HStack(spacing: 6) {
            ForEach(Array(usage.limits.prefix(n).enumerated()), id: \.element.id) { i, l in
                if i > 0 { Rectangle().fill(Color.white.opacity(0.14)).frame(width: 0.5, height: 10) }
                CompactLimit(limit: l, level: 2, now: store.now)
            }
        }
        .fixedSize()
    }

    var body: some View {
        VStack(spacing: 4) {
            // 캐릭터와 말풍선 — 최소 크기에서도 보인다
            HStack(spacing: 0) {
                Spacer(minLength: 0)
                Foreman(groups: store.groups, limits: usage.limits, now: store.now, compact: true)
            }
            if store.groups.isEmpty && autopilot.runs.isEmpty {
                Text("아직 기록된 세션이 없습니다").boardFont(11).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
            } else {
                ScrollView(showsIndicators: false) {
                    VStack(spacing: 2) {
                        // 작은 크기의 실행 줄 — 상태 꼬리표와 "2/4 제목" 한 줄, 막대는 뺌(와이어프레임 보드 본체 ⑧)
                        ForEach(autopilot.runs) { r in APRunLine(run: r, minutes: r.unitMinutes(store.now), compact: true).equatable() }
                        ForEach(store.groups) { g in MiniRow(g: g, now: store.now) }
                    }
                }
            }
            HStack(spacing: 6) {
                // 자리가 모자라면 한도를 앞의 둘만 — 투명도 슬라이더는 늘 보인다
                ViewThatFits(in: .horizontal) { limits(usage.limits.count); limits(2); Color.clear.frame(width: 0, height: 0) }
                Spacer(minLength: 4)
                HeaderButton(icon: "clock.arrow.circlepath", tint: nil, help: "자율주행 지난 실행") { autopilot.screen = .history(nil) }
                    .scaleEffect(0.8)
                    .accessibilityIdentifier("ap-history-mini")
                OpacitySlider(compact: true)
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 2)
        .padding(.bottom, 6)
        .animation(.spring(response: 0.45, dampingFraction: 0.82), value: store.groups)
    }
}

// 미니 화면의 경로 한 줄 — 누르면 대표 세션의 터미널 앱을, 두 번 누르면 그 세션의 터미널 창을 앞으로
struct MiniRow: View {
    let g: Group
    let now: Date
    @State private var hover = false

    var body: some View {
        let top = g.active.first
        let first = g.sessions.first
        let live = top ?? g.sessions.first { $0.state != "ended" }
        HStack(spacing: 8) {
            StateIcon(state: g.urgent).scaleEffect(0.8).frame(width: 15, height: 15)
            Text(g.name).boardFont(12, weight: .semibold).lineLimit(1).layoutPriority(1)
            HStack(spacing: 4) {
                // 줄에 보이는 세션이 터미널에서 도는지 백그라운드로 도는지 — 아이콘만
                if let s = top ?? first { KindLabel(headless: s.headless, compact: true) }
                if let top {
                    Text(doing(top)).lineLimit(1).foregroundStyle(Palette.color(top.state))
                    if g.active.count > 1 { Text("+\(g.active.count - 1)").fixedSize().foregroundStyle(Palette.color(top.state)) }
                } else if let s = first {
                    Text("\(Palette.name(s.state)) · " + (s.state == "idle" ? "\(elapsed(s.since, now)) 전"
                                                          : s.state == "ended" ? clock(s.updated) : s.title))
                        .lineLimit(1).foregroundStyle(.secondary)
                }
            }
            .boardFont(11)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .frame(height: miniRowHeight - 2)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(hover ? 0.07 : 0)))
        .modifier(Shimmer(active: g.urgent == "working", color: Palette.color("working")))
        .contentShape(Rectangle())
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
        // 세션 줄과 같게 클릭 횟수로 나눈다 — 두 제스처로 나누면 한 번 클릭이 0.5초 늦는다
        .onTapGesture {
            guard let s = live else { return }
            openSession(s, double: (NSApp.currentEvent?.clickCount ?? 1) >= 2)
        }
        .hint(live.map { s in
            switch TerminalHub.shared.place(s) {
            case .tab: return "\(g.path)\n눌러서 터미널을 펼치고 그 탭으로 이동"
            case .terminal: return "\(g.path)\n눌러서 터미널 앱, 두 번 눌러서 그 세션으로 이동"
            case .background, .ended: return g.path
            }
        } ?? g.path)
    }
}

// 처음 실행할 때의 크기(폭 boardLaunchWidth, 높이는 화면의 쓸 수 있는 높이 전체)로 되돌려 지금 화면의 보드 쪽 끝(보드 위치 — 오른쪽·왼쪽)에 붙인다.
// 터미널을 펼쳤으면 접는다 — "보드만 한쪽에" 라는 뜻을 그대로 둔다(펼치기는 ⌃⌥T)
@MainActor func snapToSide() {
    setTerminalExpanded(false, animated: false)   // 바로 아래에서 제 애니메이션으로 붙인다
    guard let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    let x = TerminalHub.shared.boardOnLeft ? vf.minX : vf.maxX - boardLaunchWidth
    let f = NSRect(x: x, y: vf.minY, width: boardLaunchWidth, height: vf.height)
    window.setFrame(f, display: true, animate: true)
    saveRestoreFrame()
}

// 창 단추(닫기·최소화·확대) 바로 오른쪽에 붙는 "처음 크기로 오른쪽(왼쪽)에 붙이기" 단추 — 보드 위치에 따라 그림·설명이 바뀐다
struct SnapButton: View {
    @ObservedObject private var hub = TerminalHub.shared
    @State private var hover = false
    var body: some View {
        Button(action: snapToSide) {
            Image(systemName: hub.boardOnLeft ? "rectangle.lefthalf.inset.filled" : "rectangle.righthalf.inset.filled")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.primary.opacity(hover ? 1 : 0.8))
                .frame(width: 26, height: 18)
                .background(Capsule().fill(Color.white.opacity(hover ? 0.30 : 0.18)))
                .overlay(Capsule().stroke(Color.white.opacity(0.18), lineWidth: 0.5))
                .scaleEffect(hover ? 1.12 : 1)
                .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hover)
                .frame(width: 34, height: 28)   // 누를 수 있는 영역은 캡슐보다 넉넉하게
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("처음 크기로 되돌려 화면 \(hub.boardOnLeft ? "왼쪽" : "오른쪽")에 붙이기 (⌃⌥B)")
        .padding(.leading, 4)
    }
}

// 전역 단축키 — ⌃⌥B: 어느 앱에서든 보드를 처음 크기로 보드 쪽 끝(오른쪽·왼쪽)에 붙이고 앞으로 꺼낸다(포커스는 지금 앱에 남긴다).
// ⌃⌥T: 터미널 펼치기·접기(toggleTerminal). Carbon 단축키는 손쉬운 사용 권한이 필요 없다
func registerHotKeys() {
    var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
    InstallEventHandler(GetApplicationEventTarget(), { _, event, _ in
        var key = EventHotKeyID()
        GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
                          MemoryLayout<EventHotKeyID>.size, nil, &key)
        DispatchQueue.main.async {
            if key.id == 2 {
                toggleTerminal()
            } else {
                window.orderFrontRegardless()
                snapToSide()
            }
        }
        return noErr
    }, 1, &spec, nil, nil)
    for (code, n, name) in [(kVK_ANSI_B, UInt32(1), "⌃⌥B"), (kVK_ANSI_T, UInt32(2), "⌃⌥T")] {
        var ref: EventHotKeyRef?
        let id = EventHotKeyID(signature: OSType(0x43424F44), id: n)   // 'CBOD'
        let status = RegisterEventHotKey(UInt32(code), UInt32(controlKey | optionKey), id, GetApplicationEventTarget(), 0, &ref)
        // 다른 앱이 먼저 잡은 단축키면 등록에 실패한다 — 보드는 그대로 동작하고 로그만 남긴다
        if status != noErr { boardLog("전역 단축키 \(name) 등록 실패 (\(status))") }
    }
}

func addSnapButton() {
    let vc = NSTitlebarAccessoryViewController()
    let host = NSHostingView(rootView: SnapButton())
    host.frame = NSRect(x: 0, y: 0, width: 38, height: 28)
    vc.view = host
    vc.layoutAttribute = .leading
    window.addTitlebarAccessoryViewController(vc)
}

func activateTerminal() {
    if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.mitchellh.ghostty") {
        NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
    }
}

// 더블 클릭한 세션의 Ghostty 터미널로 포커스를 옮긴다.
// Ghostty AppleScript 사전에는 tty 가 없으므로, 그 tty 에 잠시 표지 제목을 써서 터미널을 찾은 뒤 원래 제목으로 되돌린다.
let ghosttyQueue = DispatchQueue(label: "com.claude.board.ghostty")

func runScript(_ source: String) -> NSAppleEventDescriptor? {
    var err: NSDictionary?
    let r = NSAppleScript(source: source)?.executeAndReturnError(&err)
    // 권한 거부 같은 AppleScript 오류는 board.log 에 남긴다 — 보드 화면에는 드러나지 않기 때문
    if let err {
        let line = "\(Date()) \(err[NSAppleScript.errorNumber] ?? "") \(err[NSAppleScript.errorMessage] ?? "")\n"
        let log = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/board.log")
        if let h = FileHandle(forWritingAtPath: log) { h.seekToEndOfFile(); h.write(Data(line.utf8)); try? h.close() }
        else { FileManager.default.createFile(atPath: log, contents: Data(line.utf8)) }
    }
    return r
}

func writeTitle(_ tty: String, _ title: String) {
    guard let h = FileHandle(forWritingAtPath: "/dev/\(tty)") else { return }
    h.write("\u{1b}]2;\(title)\u{07}".data(using: .utf8)!)
    try? h.close()
}

// 지원하는 터미널 앱 — Ghostty 는 AppleScript 에 tty 가 없어 표지 제목으로 찾고, iTerm2·Terminal 은 tty 로 바로 찾는다
let ghosttyId = "com.mitchellh.ghostty", itermId = "com.googlecode.iterm2", terminalId = "com.apple.Terminal"

func activateApp(_ bundleId: String) {
    if let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).first {
        app.activate()
    } else if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleId) {
        NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
    }
}

func isRunning(_ bundleId: String) -> Bool {
    !NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).isEmpty
}

// claude 프로세스의 조상을 따라 올라가 그 세션을 띄운 앱을 찾는다.
// iTerm2 는 셸을 별도 서버 프로세스(부모가 launchd)에서 돌린다 — 그 서버 실행 경로로 iTerm2 로 판별한다. 못 찾으면 nil
func ancestorApp(_ pid: Int32) -> String? {
    var p = pid
    for _ in 0..<12 where p > 1 {
        if let id = NSRunningApplication(processIdentifier: p)?.bundleIdentifier { return id }
        // 앱 안의 보조 프로세스(예: Orca Helper)는 실행 파일 경로에서 가장 바깥 .app 번들을 찾는다
        var buf = [CChar](repeating: 0, count: 4096)
        if proc_pidpath(p, &buf, UInt32(buf.count)) > 0, let r = String(cString: buf).range(of: ".app/"),
           let id = Bundle(path: String(String(cString: buf)[..<r.lowerBound]) + ".app")?.bundleIdentifier {
            return id
        }
        // iTerm2 의 셸 서버는 앱 번들 밖(~/Library/Application Support/iTerm2/iTermServer-x.y.z)에 있다
        if String(cString: buf).contains("/Application Support/iTerm2/iTermServer") { return itermId }
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, p]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        p = info.kp_eproc.e_ppid
    }
    return nil
}

// tty 로 iTerm2·Terminal 의 탭을 찾아 앞으로 가져온다 — 찾으면 true
func focusByTty(_ bundleId: String, _ tty: String) -> Bool {
    let dev = "/dev/" + tty
    let script: String
    if bundleId == itermId {
        script = """
        tell application id "\(itermId)"
          repeat with w in windows
            repeat with t in tabs of w
              repeat with s in sessions of t
                if tty of s is "\(dev)" then
                  select w
                  tell t to select
                  tell s to select
                  activate
                  return "ok"
                end if
              end repeat
            end repeat
          end repeat
          return ""
        end tell
        """
    } else {
        script = """
        tell application id "\(terminalId)"
          repeat with w in windows
            repeat with t in tabs of w
              if tty of t is "\(dev)" then
                set selected tab of w to t
                set index of w to 1
                activate
                return "ok"
              end if
            end repeat
          end repeat
          return ""
        end tell
        """
    }
    return runScript(script)?.stringValue == "ok"
}

// 세션을 띄운 터미널 앱 — 조상으로 못 찾으면 실행 중인 iTerm2·Terminal 에서 tty 를 찾는다. 세션마다 기억한다
// ghosttyQueue 와 따로 잠근다 — ghosttyQueue 안(중단 등)에서 ownerApp 을 불러도 같은 큐에 sync 를 걸어 교착하지 않게
var ownerCache: [String: String] = [:]
let ownerLock = NSLock()

func ownerApp(_ s: Session) -> String? {
    if let c = ownerLock.withLock({ ownerCache[s.id] }) { return c }
    var found = s.pid > 0 ? ancestorApp(s.pid) : nil
    if found == nil, !s.tty.isEmpty {
        found = [itermId, terminalId].first { isRunning($0) && ttyExists($0, s.tty) }
    }
    if let f = found { ownerLock.withLock { ownerCache[s.id] = f } }
    return found
}

func ttyExists(_ bundleId: String, _ tty: String) -> Bool {
    let dev = "/dev/" + tty
    let script = bundleId == itermId
        ? "tell application id \"\(itermId)\" to return (count of (every session of every tab of every window whose tty is \"\(dev)\")) as text"
        : "tell application id \"\(terminalId)\" to return (count of (every tab of every window whose tty is \"\(dev)\")) as text"
    return (Int(runScript(script)?.stringValue ?? "") ?? 0) > 0
}

// 한 번 클릭: 그 세션의 터미널 앱만 앞으로 가져온다
func activateOwner(_ s: Session) {
    DispatchQueue.global(qos: .userInitiated).async {
        let app = s.headless || s.state == "ended" ? nil : ownerApp(s)
        DispatchQueue.main.async { app.map(activateApp) ?? activateTerminal() }
    }
}

func focusSession(_ s: Session) {
    guard !s.headless, !s.tty.isEmpty, s.state != "ended" else { return activateTerminal() }
    DispatchQueue.global(qos: .userInitiated).async {
        let app = ownerApp(s)
        if app == itermId || app == terminalId {
            if focusByTty(app!, s.tty) { return }
            DispatchQueue.main.async { activateApp(app!) }
            return
        }
        if let app, app != ghosttyId {   // 그 밖의 앱(VS Code 등)은 앱만 앞으로 가져온다
            DispatchQueue.main.async { activateApp(app) }
            return
        }
        focusGhostty(s)
    }
}

// 세션이 도는 Ghostty 터미널의 id 를 찾는다 — 그 tty 에 잠깐 표지 제목을 써서 찾고 원래 제목으로 되돌린다. ghosttyQueue 안에서 부른다
func ghosttyTerminalId(_ s: Session) -> String? {
    // tell 안에서 tab 은 Ghostty 의 탭 객체 이름으로 읽혀 글자 "tab" 이 들어간다 — 탭 문자는 character id 9 로 쓴다
    let list = """
    tell application "Ghostty"
      set out to ""
      repeat with t in terminals
        set out to out & (id of t) & (character id 9) & (name of t) & linefeed
      end repeat
      return out
    end tell
    """
    var before: [String: String] = [:]
    for line in (runScript(list)?.stringValue ?? "").split(separator: "\n") {
        let parts = line.split(separator: "\t", maxSplits: 1, omittingEmptySubsequences: false)
        if parts.count == 2 { before[String(parts[0])] = String(parts[1]) }
    }
    let marker = "CBFIND-\(UUID().uuidString.prefix(8))"
    writeTitle(s.tty, marker)
    var found: String?
    for _ in 0..<10 {
        Thread.sleep(forTimeInterval: 0.05)
        found = runScript("""
        tell application "Ghostty"
          repeat with t in terminals
            if name of t is "\(marker)" then return id of t
          end repeat
          return ""
        end tell
        """)?.stringValue
        if let f = found, !f.isEmpty { break }
    }
    if let f = found, !f.isEmpty, let old = before[f] {
        writeTitle(s.tty, old)
        return f
    }
    writeTitle(s.tty, (s.cwd as NSString).lastPathComponent)
    return nil
}

// Ghostty 터미널에 키·글자를 보낸다. 단계: ("key", 키 이름) | ("text", 붙여 넣을 글자) | ("wait", 초)
func ghosttySend(_ id: String, _ steps: [(String, String)]) -> Bool {
    let esc = { (t: String) in t.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") }
    let body = steps.map { kind, v in
        switch kind {
        case "key": return "send key \"\(esc(v))\" to t"
        case "text": return "input text \"\(esc(v))\" to t"
        default: return "delay \(v)"
        }
    }.joined(separator: "\n      ")
    return runScript("""
    tell application "Ghostty"
      repeat with t in terminals
        if (id of t as text) is "\(id)" then
          \(body)
          return "ok"
        end if
      end repeat
      return ""
    end tell
    """)?.stringValue == "ok"
}

// Ghostty 에서 도는 터미널 세션인가 — AppleScript 로 키를 보낼 수 있다
func canDrive(_ s: Session) -> Bool {
    !s.headless && !s.tty.isEmpty && s.state != "ended" && ownerApp(s) == ghosttyId
}

// 세션 터미널에 직접 키를 넣을 수 있는가 — 보드 탭 세션(그 탭의 대화형 claude)과 Ghostty 세션
@MainActor func canDriveSession(_ s: Session) -> Bool {
    let hub = TerminalHub.shared
    switch hub.place(s) {
    case .tab(let id, let headless): return !headless && hub.surface(id).map { $0.isForeground(s.pid) } == true
    case .terminal: return canDrive(s)
    default: return false
    }
}

// 세션 터미널에 키·글자를 넣는다. 단계: ("key", "escape"|"enter") | ("text", 붙여 넣을 글자) | ("wait", 초).
// 보드 탭 세션은 그 탭에 직접, Ghostty 세션은 AppleScript. 넣을 수 없으면 done(false) — done 은 메인에서
@MainActor func driveSession(_ s: Session, _ steps: [(String, String)], done: @escaping @MainActor (Bool) -> Void = { _ in }) {
    let hub = TerminalHub.shared
    if case .tab(let id, false) = hub.place(s), let surface = hub.surface(id), surface.isForeground(s.pid) {
        surface.drive(steps, done: done)
        return
    }
    guard hub.place(s) == .terminal, canDrive(s) else { return done(false) }
    ghosttyQueue.async {
        let ok = ghosttyTerminalId(s).map { ghosttySend($0, steps) } ?? false
        DispatchQueue.main.async { done(ok) }
    }
}

// 프로세스가 끝나기를 기다린다(최대 timeout 초) — 끝나면 done(true)
@MainActor func waitExit(_ pid: Int32, timeout: Double, done: @escaping @MainActor (Bool) -> Void) {
    guard pid > 0, kill(pid, 0) == 0 else { return done(true) }
    guard timeout > 0 else { return done(false) }
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { waitExit(pid, timeout: timeout - 0.2, done: done) }
}

// 다른 터미널(Ghostty)에서 도는 세션을 보드로 — 그쪽을 /exit 로 끝내고, 끝나면 보드 새 탭에서 resume_cmd 로 이어 연다(대화 그대로)
@MainActor func moveToBoard(_ s: Session) {
    guard TerminalHub.shared.slide == nil else { return whenPaneSettled { moveToBoard(s) } }
    if s.state == "working" || s.state == "waiting" {
        let a = NSAlert()
        a.messageText = "보드로 옮길까요?"
        a.informativeText = "\(s.title)\n\n지금 터미널의 세션을 끝내고 보드 새 탭에서 같은 대화를 이어 엽니다. 진행 중인 응답은 끊깁니다."
        a.addButton(withTitle: "옮기기")
        a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"
        NSApp.activate(ignoringOtherApps: true)
        guard a.runModal() == .alertFirstButtonReturn else { return }
    }
    let name = (s.cwd as NSString).lastPathComponent
    driveSession(s, [("key", "escape"), ("wait", "0.4"), ("text", "/exit"), ("wait", "0.2"), ("key", "enter")]) { ok in
        guard ok else { return TerminalHub.shared.toast("\(name): 세션 터미널을 찾지 못해 보드로 옮기지 못했습니다") }
        waitExit(s.pid, timeout: 8) { exited in
            if exited { resumeSession(s) } else { TerminalHub.shared.toast("\(name): 세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요") }
        }
    }
}

let sessionsDir = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/sessions")
let muteAllPath = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/mute-all")
// 알림 패널 위치 — 모서리를 고르면 그 값(top-left 등)을 남기고, notify.py 가 읽어 claude-notifier --position 으로 넘긴다. 없으면 가운데
let notifyPositionPath = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/notify-position")

// 전체 알림이 꺼졌는지 — 세션 스위치를 잠그는 데 쓴다
private struct NotifyLockedKey: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var notifyLocked: Bool {
        get { self[NotifyLockedKey.self] }
        set { self[NotifyLockedKey.self] = newValue }
    }
}

func muteMark(_ s: Session) -> String { (sessionsDir as NSString).appendingPathComponent("\(s.id).mute") }

extension Notification.Name { static let boardReload = Notification.Name("com.claude.board.reload") }

// 알림 끄기 표지는 세션 기록이 지워질 때 state.py prune 이 함께 치운다
func setMuted(_ s: Session, _ on: Bool) {
    // 화면은 MuteToggle 이 먼저 바꾸고, 표지 파일 반영과 다시 읽기는 다음 틱에 한다
    DispatchQueue.main.async {
        if on {
            FileManager.default.createFile(atPath: muteMark(s), contents: Data())
        } else {
            try? FileManager.default.removeItem(atPath: muteMark(s))
        }
        NotificationCenter.default.post(name: .boardReload, object: nil)
    }
}

func pausedMark(_ s: Session) -> String { (sessionsDir as NSString).appendingPathComponent("\(s.id).paused") }
let carryOnPrompt = "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요."

// 일시중지: Esc 로 지금 응답을 멈춘다. 그때까지 한 작업은 대화 기록에 남아 있으므로 재개하면 바로 이어 간다
@MainActor func pauseSession(_ s: Session) {
    driveSession(s, [("key", "escape")]) { ok in
        guard ok else { return }
        let mark = ["paused_at": Date().timeIntervalSince1970, "topic": s.topic] as [String: Any]
        if let data = try? JSONSerialization.data(withJSONObject: mark) {
            FileManager.default.createFile(atPath: pausedMark(s), contents: data)
        }
    }
}

// 재개: 살아 있는 세션은 그 터미널에 이어서 진행하라는 요청을 넣고, 꺼진 세션은 보드 새 탭에서 이어 연다
@MainActor func resumePaused(_ s: Session) {
    guard s.pid > 0, kill(s.pid, 0) == 0 else { return resumeSession(s, carryOn: true) }
    driveSession(s, [("text", carryOnPrompt), ("wait", "0.2"), ("key", "enter")]) { ok in
        if ok { try? FileManager.default.removeItem(atPath: pausedMark(s)) } else { resumeSession(s, carryOn: true) }
    }
}

// 중단: 확인을 받은 뒤 세션을 끝낸다. 터미널 세션(보드 탭·Ghostty)은 Esc → /exit, 그 밖의 터미널 세션과 보드 탭 안의
// claude -p 는 SIGTERM. 진짜 백그라운드 세션에는 신호를 보내지 않는다(메뉴에도 없다 — 보드는 백그라운드 세션을 건드리지 않는다).
// 대화 기록은 남으므로 종료 세션 메뉴의 "세션 다시 열기"로 이어 열 수 있다
@MainActor func stopSession(_ s: Session, confirm: Bool = true) {
    if confirm, TerminalHub.shared.slide != nil { return whenPaneSettled { stopSession(s, confirm: confirm) } }
    if confirm {
    let alert = NSAlert()
    alert.messageText = "세션을 중단할까요?"
    alert.informativeText = "\(s.topic.isEmpty ? s.id : s.topic)\n\n진행 중인 응답을 멈추고 세션을 끝냅니다. 대화 기록은 남아서 나중에 다시 열 수 있습니다."
    alert.addButton(withTitle: "중단")
    alert.addButton(withTitle: "취소")
    alert.buttons.first?.hasDestructiveAction = true
    NSApp.activate(ignoringOtherApps: true)
    guard alert.runModal() == .alertFirstButtonReturn else { return }
    }
    try? FileManager.default.removeItem(atPath: pausedMark(s))
    if TerminalHub.shared.place(s) == .background { return }
    driveSession(s, [("key", "escape"), ("wait", "0.4"), ("text", "/exit"), ("wait", "0.2"), ("key", "enter")]) { ok in
        if !ok, s.pid > 0 { kill(s.pid, SIGTERM) }
    }
}

func focusGhostty(_ s: Session) {
    ghosttyQueue.async {
        // 표지를 쓰기 전 제목을 기억해 둔다 — 터미널 id 와 제목을 줄 단위로 받는다
        let list = """
        tell application "Ghostty"
          set out to ""
          repeat with t in terminals
            set out to out & (id of t) & (character id 9) & (name of t) & linefeed
          end repeat
          return out
        end tell
        """
        var before: [String: String] = [:]
        for line in (runScript(list)?.stringValue ?? "").split(separator: "\n") {
            let parts = line.split(separator: "\t", maxSplits: 1, omittingEmptySubsequences: false)
            if parts.count == 2 { before[String(parts[0])] = String(parts[1]) }
        }
        let marker = "CBFOCUS-\(UUID().uuidString.prefix(8))"
        writeTitle(s.tty, marker)
        var found: String?
        for _ in 0..<10 {
            Thread.sleep(forTimeInterval: 0.05)
            found = runScript("""
            tell application "Ghostty"
              repeat with t in terminals
                if name of t is "\(marker)" then
                  focus t
                  activate
                  return id of t
                end if
              end repeat
              return ""
            end tell
            """)?.stringValue
            if let f = found, !f.isEmpty { break }
        }
        if let f = found, let old = before[f] {
            writeTitle(s.tty, old)
        } else {
            // Ghostty 밖(다른 터미널)에서 돌고 있는 세션이다 — 표지 제목만 지우고 앱만 앞으로 가져온다
            writeTitle(s.tty, (s.cwd as NSString).lastPathComponent)
            DispatchQueue.main.async { activateTerminal() }
        }
    }
}

// 대화 기록 위치 — hook 이 알려 준 경로가 없으면 ~/.claude/projects/<작업 경로의 영숫자 외 문자를 - 로 바꾼 이름>/<세션 ID>.jsonl
func transcriptPath(_ given: String?, _ cwd: String, _ id: String) -> String {
    if let g = given, !g.isEmpty { return g }
    let slug = cwd.replacingOccurrences(of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
    return NSHomeDirectory() + "/.claude/projects/\(slug)/\(id).jsonl"
}

// 끝난 세션을 보드 탭에서 그 세션의 프로젝트 경로(claude 를 띄운 폴더 — 보드 카드·탭의 경로와 같다. cd·워크트리로 옮긴 cwd 가 아니다)로
// 이어 연다 — 그 세션이 마지막으로 돌던 화면이 같은 경로의 셸 프롬프트로 남아 있으면 그 화면에, 아니면 새 탭에. 대화 이력은 claude --resume 이 불러오고,
// 실행 인자·마지막 권한 모드·모델은 state.py 가 기록한 resume_cmd 에 담겨 있다(-p 는 넘기지 않아 대화형으로 열린다)
@MainActor func resumeSession(_ s: Session, carryOn: Bool = false) {
    // 답변 꾸미기는 기본으로 켠다(보드 밖에서 시작한 세션도) — 새 세션 창에서 끄고 띄운 세션만 끈 채로. 재개 문구보다 앞에
    let off = isMdRenderOff(s.id)
    var cmd = mdRenderCommand(s.resumeCmd.isEmpty ? "claude --resume \(s.id)" : s.resumeCmd, args: sessionRecord(s.id)?["args"] as? [String], off: off)
    // 일시중지했던 세션은 첫 요청으로 이어서 진행하라는 문구를 넘겨 바로 다음 작업을 시작하게 한다
    if carryOn {
        cmd += " '" + carryOnPrompt.replacingOccurrences(of: "'", with: "'\\''") + "'"
        try? FileManager.default.removeItem(atPath: pausedMark(s))
    }
    let hub = TerminalHub.shared
    if let surface = hub.surfaces.first(where: { $0.sessionId == s.id && $0.atPrompt && $0.cwd == s.projectDir }) {
        hub.run(cmd, in: surface, sessionId: s.id, mdRenderOff: off)
        hub.reveal(surface.id)
    } else {
        hub.launch(cmd, in: s.projectDir, sessionId: s.id, mdRenderOff: off)
    }
}

// 세션을 한 번 눌렀을 때(목록·미니·알림 패널 "세션으로 이동") — 보드 탭이면 펼치고 그 탭의 그 화면으로, 끝난 세션은 보드 새 탭에서 이어 열기,
// 백그라운드 세션은 안내만, 다른 터미널 세션은 지금처럼 그 터미널로(두 번 누르면 그 세션의 터미널 창까지)
@MainActor func openSession(_ s: Session, double: Bool = false) {
    let hub = TerminalHub.shared
    switch hub.place(s) {
    case .tab(let id, _):
        hub.reveal(id)
    case .ended:
        // 끝난 백그라운드 세션은 누르면 아무것도 하지 않는다 — 메뉴의 "세션 다시 열기"로만 보드 탭에서 연다
        if !s.headless { resumeSession(s, carryOn: s.paused) }
    case .background:
        hub.toast("\(s.title): claude \(hub.mode(s) ?? "-p") 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다.")
    case .terminal:
        double ? focusSession(s) : activateOwner(s)
    }
}

// 터미널을 펼쳐 창이 화면 전체를 덮은 동안에는 고정하지 않는다 — 화면 전체를 덮은 창이 늘 위에 있으면 다른 앱을 쓸 수 없다.
// 펼친 채 창 크기 전환(60%)·손으로 줄인 창과 접은 창은 고정
@MainActor func applyPin(_ pinned: Bool) {
    let level: NSWindow.Level = pinned && !expandedToFullScreen() ? .floating : .normal
    if window.level != level { window.level = level }   // 창 크기가 바뀔 때마다 불린다
}

@MainActor func setPinned(_ pinned: Bool) {
    UserDefaults.standard.set(pinned, forKey: "pinned")
    applyPin(pinned)
}

func showBoard() {
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    let store = Store()
    let usage = UsageStore()
    let accounts = AccountStore()
    lazy var autoSwitcher = AutoSwitcher(store: store, usage: usage, accounts: accounts)
    var statusItem: NSStatusItem!
    private var statusShown = ""

    func applicationDidFinishLaunching(_ notification: Notification) {
        UserDefaults.standard.register(defaults: ["pinned": true, "mascot": Mascot.tralalero.rawValue])
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: boardLaunchWidth, height: 620),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isMovableByWindowBackground = true
        window.isOpaque = false
        window.backgroundColor = .clear
        window.appearance = NSAppearance(named: .darkAqua)
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.hidesOnDeactivate = false
        window.isReleasedWhenClosed = false   // 닫아도 앱은 남고, Dock·메뉴 막대 아이콘으로 다시 연다
        window.title = instanceName == "claude-board" ? "Claude 세션 보드" : "Claude 세션 보드 (시험 빌드)"

        let blur = NSVisualEffectView()
        blur.material = .hudWindow
        blur.blendingMode = .behindWindow
        blur.state = .active
        let host = FirstClickHostingView(rootView: BoardView(store: store, usage: usage, accounts: accounts))
        host.translatesAutoresizingMaskIntoConstraints = false
        // 화면이 바뀔 때마다 SwiftUI 전체의 이상적 크기를 재서 창에 알리지 않는다 — 창 크기·최소 크기는 직접 정한다.
        // 켜 두면 반짝임·회전 애니메이션 프레임마다 창 전체 배치를 다시 계산해 클릭 반응이 늦어진다
        host.sizingOptions = []
        blur.addSubview(host)
        NSLayoutConstraint.activate([
            host.leadingAnchor.constraint(equalTo: blur.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: blur.trailingAnchor),
            host.topAnchor.constraint(equalTo: blur.topAnchor),
            host.bottomAnchor.constraint(equalTo: blur.bottomAnchor),
        ])
        let root = BoardRootView(board: blur)
        window.contentView = root
        TerminalHub.shared.area = root.terminal
        TerminalHub.shared.store = store
        PointerTracker.shared.attach(blur)
        PointerTracker.shared.attachWindow(root)
        _ = mdRenderDir   // 답변 꾸미기 플러그인을 ~/.claude/notifier 로 옮겨 둔다 — 되살릴 탭이 쓴다
        pruneMdRenderOff()
        DispatchQueue.main.async { TerminalHub.shared.restoreTabs() }   // 지난번에 쥐고 있던 탭을 되살린다
        // 손으로 바꿔도 자동 전환이 전환 시각을 알아야 한다 — 그 뒤 잠시 난 한도 오류는 이전 계정 토큰 탓이다
        accounts.onSwitch = { [usage, autoSwitcher] in
            usage.reset()
            autoSwitcher.switched()
        }
        let nc = NotificationCenter.default
        // 가려지거나 숨은 동안에는 늘 도는 애니메이션 시계를 멈춘다
        // 터미널도 창이 안 보이는 동안(가려짐·최소화·닫힘·다른 데스크톱)은 그리지 않는다
        nc.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { _ in
            AmbientTicker.shared.running = window.occlusionState.contains(.visible)
            MainActor.assumeIsolated { TerminalHub.shared.applyVisibility() }
        }
        // 미니 화면 크기보다 창을 더 줄이지 못하게 한다 — 그보다 크면 창 크기에 따라 전체·중간·미니로 그린다
        // 터미널을 펼쳤으면 터미널 최소 폭과 보드 최소 폭이 들어가는 크기까지
        window.contentMinSize = TerminalHub.shared.expanded ? terminalExpandedMinSize : NSSize(width: miniWidth, height: miniMinHeight)
        window.delegate = sizeGuard   // contentMinSize 만으로는 손으로 줄일 때 막히지 않아 한 번 더 막는다
        // 터미널을 펼쳤을 때 제목 막대·탭 줄 빈 곳 더블클릭 = 창 크기 전환(화면 전체 ↔ 60%). 그 클릭은 삼킨다
        NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { e in
            guard isWindowSizeDoubleClick(e) else { return e }
            toggleWindowSize()
            return nil
        }
        // ⌘ 를 누르고 있으면 탭 줄에 ⌘번호·이름 바꾸기 단축키를 보인다.
        // ⌘Tab 등으로 앱이 비활성이 되면 놓는 신호가 오지 않으므로 끈다
        NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { e in
            TerminalHub.shared.commandFlag(e.modifierFlags.contains(.command))
            return e
        }
        nc.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { TerminalHub.shared.commandFlag(false) }
        }

        // 저장된 창 위치 끝에 macOS 타일 배치 기록({"tilingState":…})이 붙어 있으면 창을 띄우는 순간 macOS 가 그 타일 크기로
        // 되돌린다(2026-10-16 확인 — 미니로 둔 300×144 가 화면 높이로, 폭 454 가 타일 폭 385 로 열렸다). 크기·위치 부분만 남긴다
        let frameKey = "NSWindow Frame ClaudeBoard"
        if let saved = UserDefaults.standard.string(forKey: frameKey), let brace = saved.firstIndex(of: "{") {
            UserDefaults.standard.set(String(saved[..<brace]), forKey: frameKey)
        }
        // 저장된 가로 위치 그대로 — setFrameUsingName 은 화면 오른쪽 끝(Dock 옆)에 붙여 둔 접힌 보드를 46pt 왼쪽으로 옮겨 연다(2026-10-17 실측)
        let savedX = UserDefaults.standard.string(forKey: frameKey)?.split(separator: " ").first.flatMap { Double($0) }
        if !window.setFrameUsingName("ClaudeBoard") { window.center() }
        window.setFrameAutosaveName("ClaudeBoard")   // 저장된 프레임을 다시 적용하므로 폭 조정은 이 뒤에 한다
        // 실행할 때는 가로 위치만 마지막 상태로 두고, 폭은 접을 때와 같게 경계를 끌어 정한 폭(끌지 않았으면 boardLaunchWidth),
        // 높이는 그 화면의 쓸 수 있는 높이 전체(메뉴 막대·Dock 제외)로 연다
        // (화면 분할 상태가 섞여 전체 폭으로 열리는 것도 막는다). 미니로 두고 껐으면 그 크기·위치 그대로 연다.
        // 터미널을 펼친 채로 껐으면(기본값) 화면의 쓸 수 있는 영역 전체로 연다
        if TerminalHub.shared.expanded, let vf = (window.screen ?? NSScreen.main)?.visibleFrame {
            window.setFrame(vf, display: false)
        } else if !UserDefaults.standard.bool(forKey: "boardMini") {
            var f = window.frame
            f.size.width = TerminalHub.shared.boardWidth
            if let vf = (window.screen ?? NSScreen.main)?.visibleFrame {
                f.origin.y = vf.minY
                f.size.height = vf.height
            }
            if let x = savedX, NSScreen.screens.contains(where: { $0.visibleFrame.intersects(NSRect(x: x, y: f.minY, width: f.width, height: f.height)) }) {
                f.origin.x = x
            }
            window.setFrame(f, display: false)
        }
        addSnapButton()
        addTerminalToggle()
        addMiniTitlebar(store)
        BoardFit.shared.update()   // 창을 띄우기 전에 단계를 정한다 — 미니로 열 때 전체 화면이 한 번 비치지 않게
        // 미니에서 돌아갈 위치는 접힌 보드의 위치만 기억한다
        if BoardFit.shared.tier != .mini, !TerminalHub.shared.expanded { saveRestoreFrame() }
        // 실행 크기를 정한 뒤부터 창 크기를 따라 단계를 고른다.
        // 펼친 창이 화면 전체가 되거나 벗어나면(창 크기 전환·초록 단추·손으로 바꿈·시스템 타일 배치) 위에 고정·투명도도 다시 맞춘다 —
        // 펼치기·접기 애니메이션 중에는 끝난 뒤 settleTerminalPane 이 맞춘다
        nc.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) { _ in
            MainActor.assumeIsolated {
                BoardFit.shared.update()
                if TerminalHub.shared.slide == nil {
                    applyPin(UserDefaults.standard.bool(forKey: "pinned"))
                    WindowOpacity.shared.apply()
                }
            }
        }
        nc.addObserver(forName: NSWindow.didEndLiveResizeNotification, object: window, queue: .main) { _ in
            MainActor.assumeIsolated { if BoardFit.shared.tier != .mini, !TerminalHub.shared.expanded { saveRestoreFrame() } }
        }
        registerHotKeys()
        applyPin(UserDefaults.standard.bool(forKey: "pinned"))
        WindowOpacity.shared.apply()
        buildMainMenu()
        buildStatusItem()
        // 셸 설정에서 --background 로 띄우면 창만 앞에 보이고 포커스는 터미널에 남긴다
        if CommandLine.arguments.contains("--background") { window.orderFrontRegardless() } else { showBoard() }

        DistributedNotificationCenter.default().addObserver(forName: .init("\(instanceName).show"), object: nil,
                                                            queue: .main) { _ in showBoard() }
        DistributedNotificationCenter.default().addObserver(forName: .init("\(instanceName).action"), object: nil,
                                                            queue: .main) { n in
            if let a = n.userInfo?["action"] as? String, let id = n.userInfo?["id"] as? String { MainActor.assumeIsolated { performAction(a, id) } }
        }
        // Ghostty 가 실행되면 보드 창도 함께 띄운다 — 포커스는 Ghostty 에 그대로 둔다
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didLaunchApplicationNotification,
                                                          object: nil, queue: .main) { note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            if app?.bundleIdentifier == "com.mitchellh.ghostty" { window.orderFrontRegardless() }
        }
    }

    // 상단 앱 메뉴: 보드 보기 · 항상 위에 고정 · 종료
    func buildMainMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        main.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "세션 보드 보기", action: #selector(showBoardAction), keyEquivalent: "0")
        appMenu.addItem(withTitle: "항상 위에 고정", action: #selector(togglePin), keyEquivalent: "p")
        // 제목 오른쪽 캐릭터 고르기 — 캐릭터를 오른쪽 클릭해도 같은 목록이 나온다. 체크 표시는 validateMenuItem 이 단다
        let mascotMenu = NSMenu(title: "캐릭터")   // 하위 메뉴를 달면 항목 이름이 하위 메뉴 제목을 따른다
        for m in Mascot.allCases {
            mascotMenu.addItem(withTitle: m.name, action: #selector(pickMascot(_:)), keyEquivalent: "").representedObject = m.rawValue
        }
        appMenu.addItem(withTitle: "캐릭터", action: nil, keyEquivalent: "").submenu = mascotMenu
        // 작업 완료·승인 대기 알림 패널이 뜨는 자리 — 체크 표시는 validateMenuItem 이 단다
        let positionMenu = NSMenu(title: "알림 위치")
        for (value, name) in [("center", "중앙 (기본)"), ("top-left", "왼쪽 상단"), ("top-right", "오른쪽 상단"),
                              ("bottom-left", "왼쪽 하단"), ("bottom-right", "오른쪽 하단")] {
            positionMenu.addItem(withTitle: name, action: #selector(pickNotifyPosition(_:)), keyEquivalent: "").representedObject = value
        }
        appMenu.addItem(withTitle: "알림 위치", action: nil, keyEquivalent: "").submenu = positionMenu
        appMenu.addItem(withTitle: "업데이트 확인…", action: #selector(checkForUpdates), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Claude Board 가리기", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(withTitle: "Claude Board 종료", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        // 편집 — 첫 응답자에게 보낸다(target nil). 탭 이름 칸·새 세션 창 명령 칸의 ⌘A·⌘C·⌘V 는 이 메뉴가 있어야 듣는다.
        // 터미널에 포커스가 있으면 엔진이 묶은 ⌘C·⌘V·⌘A·⌘Z·⇧⌘Z 는 뷰의 performKeyEquivalent 가 메뉴보다 먼저 받는다
        let editItem = NSMenuItem()
        main.addItem(editItem)
        let editMenu = NSMenu(title: "편집")
        // 실행 취소·복귀는 직접 보낸다 — target nil 이면 AppKit 이 제목을 앱 언어(지역 선언이 없어 영어)의 "Undo …" 로 바꿔 쓴다(실측).
        // 앱 지역을 ko 로 선언하면 고쳐지지만 엔진이 셸 LANG 을 앱 로캘로 정해 터미널까지 바뀐다. 할 것이 없어도 꺼지지 않는다
        for (title, sel, mods) in [("실행 취소", "undo:", NSEvent.ModifierFlags.command), ("실행 복귀", "redo:", [.command, .shift])] {
            let item = ActionItem(title) { NSApp.sendAction(Selector(sel), to: nil, from: nil) }
            item.keyEquivalent = "z"
            item.keyEquivalentModifierMask = mods
            editMenu.addItem(item)
        }
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "잘라내기", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "복사하기", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "붙여넣기", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(withTitle: "모두 선택", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = editMenu

        let windowItem = NSMenuItem()
        main.addItem(windowItem)
        let windowMenu = NSMenu(title: "윈도우")
        windowMenu.addItem(withTitle: "닫기", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(withTitle: "최소화", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(.separator())
        // 단축키 표시용 — 실제 동작은 전역 단축키(registerSnapHotKey)가 받는다. 제목(오른쪽·왼쪽)은 validateMenuItem 이 보드 위치에 맞춘다
        let snap = NSMenuItem(title: "처음 크기로 오른쪽에 붙이기", action: #selector(snapAction), keyEquivalent: "b")
        snap.keyEquivalentModifierMask = [.control, .option]
        windowMenu.addItem(snap)
        let term = ActionItem("터미널 펼치기·접기") { toggleTerminal() }
        term.keyEquivalent = "t"
        term.keyEquivalentModifierMask = [.control, .option]
        windowMenu.addItem(term)
        // 탭 줄의 "+ 새 세션" 과 같다(접혀 있으면 펼친다). 터미널에 포커스가 있어도 오도록 엔진의 ⌘T(new_tab)는 풀어 두었다
        let newSession = ActionItem("새 세션…") { TerminalHub.shared.newSessionSheet() }
        newSession.keyEquivalent = "t"
        windowMenu.addItem(newSession)
        // ⌘1~⌘9 — 탭 줄(지금 경로의 탭) 순서대로 그 탭으로(접혀 있으면 펼친다). 그 자리에 탭이 없으면 그대로. 엔진의 ⌘1~⌘9(goto_tab·last_tab)는 풀어 두었다
        let tabMenu = NSMenu(title: "탭으로 가기")
        for n in 1...9 {
            let item = ActionItem("탭 \(n)") {
                let hub = TerminalHub.shared, list = hub.tabs(in: hub.workspace)
                if list.indices.contains(n - 1) { hub.reveal(list[n - 1].id) }
            }
            item.keyEquivalent = "\(n)"
            tabMenu.addItem(item)
        }
        tabMenu.addItem(.separator())
        func arrow(_ key: Int) -> String { String(Character(UnicodeScalar(key)!)) }
        func keyItem(_ menu: NSMenu, _ title: String, _ key: String, _ mods: NSEvent.ModifierFlags, hidden: Bool = false,
                     run: @escaping @MainActor () -> Void) {
            let item = ActionItem(title, run: run)
            item.keyEquivalent = key
            item.keyEquivalentModifierMask = mods
            item.isHidden = hidden
            item.allowsKeyEquivalentWhenHidden = hidden
            menu.addItem(item)
        }
        // ⌃⌥←·⌃⌥→ — 탭 줄에서 왼쪽·오른쪽 탭으로(지금 경로 안에서, 끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다). Ghostty 의 탭 키
        // ⇧⌘[·⇧⌘]·⌃⇧⇥·⌃⇥ 도 숨긴 항목으로 받는다(엔진의 같은 키 previous_tab·next_tab 은 풀어 두었다)
        keyItem(tabMenu, "이전 탭", arrow(NSLeftArrowFunctionKey), [.control, .option]) { TerminalHub.shared.stepTab(-1) }
        keyItem(tabMenu, "다음 탭", arrow(NSRightArrowFunctionKey), [.control, .option]) { TerminalHub.shared.stepTab(1) }
        keyItem(tabMenu, "이전 탭", "{", [.command, .shift], hidden: true) { TerminalHub.shared.stepTab(-1) }
        keyItem(tabMenu, "다음 탭", "}", [.command, .shift], hidden: true) { TerminalHub.shared.stepTab(1) }
        keyItem(tabMenu, "이전 탭", "\u{19}", [.control, .shift], hidden: true) { TerminalHub.shared.stepTab(-1) }
        keyItem(tabMenu, "다음 탭", "\t", [.control], hidden: true) { TerminalHub.shared.stepTab(1) }
        tabMenu.addItem(.separator())
        // ⌃⌥↑·⌃⌥↓ — 경로 줄에서 앞·뒤 경로로(그 경로에서 마지막에 고른 탭, 끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다)
        keyItem(tabMenu, "이전 경로", arrow(NSUpArrowFunctionKey), [.control, .option]) { TerminalHub.shared.stepWorkspace(-1) }
        keyItem(tabMenu, "다음 경로", arrow(NSDownArrowFunctionKey), [.control, .option]) { TerminalHub.shared.stepWorkspace(1) }
        windowMenu.addItem(withTitle: "탭으로 가기", action: nil, keyEquivalent: "").submenu = tabMenu
        // 화면 나누기 — Ghostty 와 같은 키. 보드가 탭마다 화면 트리를 쥐고, 엔진의 같은 키는 풀어 두었다(⌘W 는 엔진이 그 화면을 닫는다)
        let splitMenu = NSMenu(title: "화면 나누기")
        keyItem(splitMenu, "오른쪽으로 나누기", "d", [.command]) { TerminalHub.shared.split(right: true) }
        keyItem(splitMenu, "아래로 나누기", "d", [.command, .shift]) { TerminalHub.shared.split(right: false) }
        splitMenu.addItem(.separator())
        keyItem(splitMenu, "이전 화면", "[", [.command]) { TerminalHub.shared.focusSurface(step: -1) }
        keyItem(splitMenu, "다음 화면", "]", [.command]) { TerminalHub.shared.focusSurface(step: 1) }
        let directions: [(String, Int, SplitDirection)] = [("위로", NSUpArrowFunctionKey, .up), ("아래로", NSDownArrowFunctionKey, .down),
                                                          ("왼쪽으로", NSLeftArrowFunctionKey, .left), ("오른쪽으로", NSRightArrowFunctionKey, .right)]
        for (name, key, d) in directions {
            keyItem(splitMenu, "\(name) 화면 고르기", arrow(key), [.command, .option]) { TerminalHub.shared.focusSurface(d) }
        }
        splitMenu.addItem(.separator())
        for (name, key, d) in directions {
            keyItem(splitMenu, "경계를 \(name) 옮기기", arrow(key), [.command, .control]) { TerminalHub.shared.resizeSplit(d) }
        }
        keyItem(splitMenu, "크기 고르게", "=", [.command, .control]) { TerminalHub.shared.equalizeSplits() }
        splitMenu.addItem(.separator())
        keyItem(splitMenu, "화면 하나 크게 보기", "\r", [.command, .shift]) { TerminalHub.shared.toggleZoom() }
        windowMenu.addItem(withTitle: "화면 나누기", action: nil, keyEquivalent: "").submenu = splitMenu
        // ⇧⌘R — 고른 탭의 이름을 탭 안에서 고친다(접혀 있으면 펼친다). 탭이 없으면 그대로. 엔진 기본값은 이 키를 묶지 않는다
        let rename = ActionItem("탭 이름 바꾸기") { TerminalHub.shared.beginRename() }
        rename.keyEquivalent = "r"
        rename.keyEquivalentModifierMask = [.command, .shift]
        windowMenu.addItem(rename)
        // 글자 크기 — 터미널(모든 탭, 1pt씩)과 보드(10%씩)를 따로 정한다. 둘 다 다시 켜도 남는다. 엔진의 ⌘+·⌘=·⌘-·⌘0 은 풀어 두어
        // 터미널 포커스에서도 여기로 온다. ⌘0 은 이미 "세션 보드 보기" 라 터미널 원래 크기는 단축키가 없다
        let fontMenu = NSMenu(title: "글자 크기")
        // 보이는 항목은 +, 같은 키의 = (Shift 없이)는 숨긴 항목으로 받는다
        func sizeItem(_ title: String, _ key: String, _ mods: NSEvent.ModifierFlags, hidden: Bool = false, run: @escaping @MainActor () -> Void) {
            let item = ActionItem(title, run: run)
            item.keyEquivalent = key
            item.keyEquivalentModifierMask = mods
            item.isHidden = hidden
            item.allowsKeyEquivalentWhenHidden = hidden
            fontMenu.addItem(item)
        }
        sizeItem("터미널 글자 크게", "+", [.command]) { TerminalHub.shared.changeTerminalFont(1) }
        sizeItem("터미널 글자 크게", "=", [.command], hidden: true) { TerminalHub.shared.changeTerminalFont(1) }
        sizeItem("터미널 글자 작게", "-", [.command]) { TerminalHub.shared.changeTerminalFont(-1) }
        sizeItem("터미널 글자 원래 크기", "", []) { TerminalHub.shared.changeTerminalFont(nil) }
        fontMenu.addItem(.separator())
        sizeItem("보드 글자 크게", "+", [.command, .option]) { BoardFontScale.shared.change(1) }
        sizeItem("보드 글자 크게", "=", [.command, .option], hidden: true) { BoardFontScale.shared.change(1) }
        // ⌥⇧⌘= 는 Shift 까지 넣은 항목에만 맞는다 — 없으면 AppKit 이 ⌥ 를 빼고 "터미널 글자 크게"(⌘+)로 보냈다(실측)
        sizeItem("보드 글자 크게", "+", [.command, .option, .shift], hidden: true) { BoardFontScale.shared.change(1) }
        sizeItem("보드 글자 작게", "-", [.command, .option]) { BoardFontScale.shared.change(-1) }
        sizeItem("보드 글자 원래 크기", "0", [.command, .option]) { BoardFontScale.shared.change(nil) }
        windowMenu.addItem(withTitle: "글자 크기", action: nil, keyEquivalent: "").submenu = fontMenu
        // 터미널 테마 — 엔진에 든 테마를 고르는 창(보드 설정 terminalTheme, 고르지 않았으면 Ghostty 설정의 theme). 단축키 없음
        windowMenu.addItem(ActionItem("터미널 테마…") { ThemePicker.shared.open() })
        windowMenu.addItem(.separator())
        // 보드 위치(오른쪽·왼쪽)와 펼치기·접기 애니메이션 — 체크 표시는 validateMenuItem 이 단다
        let sideMenu = NSMenu(title: "보드 위치")
        for (value, name) in [("right", "오른쪽 (기본)"), ("left", "왼쪽")] {
            sideMenu.addItem(withTitle: name, action: #selector(pickBoardSide(_:)), keyEquivalent: "").representedObject = value
        }
        windowMenu.addItem(withTitle: "보드 위치", action: nil, keyEquivalent: "").submenu = sideMenu
        windowMenu.addItem(withTitle: "펼치기·접기 애니메이션", action: #selector(toggleTerminalAnimation), keyEquivalent: "")
        windowItem.submenu = windowMenu
        NSApp.mainMenu = main
    }

    // 상단 메뉴 막대 아이콘: 누르면 보드를 열고 닫는다. 승인 대기·작업 중 세션 수를 함께 보여 준다
    func buildStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            // 공유 이미지의 크기를 바꾸면 보드 제목 옆 아이콘에도 번지므로 복사해서 줄인다
            let icon = appIcon.copy() as! NSImage
            icon.size = NSSize(width: 18, height: 18)
            icon.accessibilityDescription = "Claude Board"
            button.image = icon
            button.toolTip = "Claude Board: 누르면 보드를 열고 닫습니다"   // 보드 밖이라 시스템 툴팁
            button.imagePosition = .imageLeading
            button.action = #selector(toggleBoard)
            button.target = self
        }
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.updateStatusTitle() }
        RunLoop.main.add(timer, forMode: .common)
        updateStatusTitle()
    }

    func updateStatusTitle() {
        let waiting = store.count("waiting"), working = store.count("working")
        // 매초 같은 제목을 다시 넣으면 메뉴 막대가 매초 다시 그려진다 — 숫자가 바뀔 때만 넣는다
        guard "\(waiting)|\(working)" != statusShown else { return }
        statusShown = "\(waiting)|\(working)"
        let title = NSMutableAttributedString()
        let font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .semibold)
        if waiting > 0 {
            title.append(NSAttributedString(string: " \(waiting)", attributes: [
                .font: font, .foregroundColor: NSColor(red: 1.0, green: 0.66, blue: 0.36, alpha: 1)]))
        }
        if working > 0 {
            title.append(NSAttributedString(string: " \(working)", attributes: [
                .font: font, .foregroundColor: NSColor(red: 0.45, green: 0.66, blue: 1.0, alpha: 1)]))
        }
        statusItem.button?.attributedTitle = title
    }

    @objc func toggleBoard() {
        if window.isVisible && NSApp.isActive && window.isKeyWindow {
            window.orderOut(nil)
        } else {
            showBoard()
        }
    }

    @objc func showBoardAction() { showBoard() }

    @objc func togglePin() { MainActor.assumeIsolated { setPinned(!UserDefaults.standard.bool(forKey: "pinned")) } }

    @objc func pickMascot(_ item: NSMenuItem) { UserDefaults.standard.set(item.representedObject as? String, forKey: "mascot") }

    @objc func snapAction() { MainActor.assumeIsolated { snapToSide() } }

    @objc func pickBoardSide(_ item: NSMenuItem) { MainActor.assumeIsolated { setBoardSide(left: item.representedObject as? String == "left") } }

    // 펼치기·접기 애니메이션 켜고 끄기 — 기본 끔(바로 펼치고 접는다)
    @objc func toggleTerminalAnimation() {
        UserDefaults.standard.set(!UserDefaults.standard.bool(forKey: "terminalAnimation"), forKey: "terminalAnimation")
    }

    // 중앙(기본)을 고르면 표지 파일을 지운다 — 파일이 없으면 notify.py 가 가운데로 띄운다
    @objc func pickNotifyPosition(_ item: NSMenuItem) {
        guard let value = item.representedObject as? String else { return }
        if value == "center" {
            try? FileManager.default.removeItem(atPath: notifyPositionPath)
        } else {
            try? value.write(toFile: notifyPositionPath, atomically: true, encoding: .utf8)
        }
    }

    // 결과는 보드의 알림 줄에 뜨므로 보드를 먼저 연다
    @objc func checkForUpdates() {
        showBoard()
        Updater.shared.check(manual: true)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(togglePin) { item.state = UserDefaults.standard.bool(forKey: "pinned") ? .on : .off }
        let left = UserDefaults.standard.string(forKey: "boardSide") == "left"
        if item.action == #selector(snapAction) { item.title = left ? "처음 크기로 왼쪽에 붙이기" : "처음 크기로 오른쪽에 붙이기" }
        if item.action == #selector(pickBoardSide(_:)) { item.state = (item.representedObject as? String == "left") == left ? .on : .off }
        if item.action == #selector(toggleTerminalAnimation) { item.state = UserDefaults.standard.bool(forKey: "terminalAnimation") ? .on : .off }
        if item.action == #selector(pickMascot(_:)) {
            let current = UserDefaults.standard.string(forKey: "mascot") ?? Mascot.tralalero.rawValue
            item.state = item.representedObject as? String == current ? .on : .off
        }
        if item.action == #selector(pickNotifyPosition(_:)) {
            let current = (try? String(contentsOfFile: notifyPositionPath, encoding: .utf8))?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? "center"
            item.state = item.representedObject as? String == current ? .on : .off
        }
        return true
    }

    // Dock 아이콘을 누르면 닫아 둔 창을 다시 연다
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showBoard()
        return true
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    // 끄기 전 확인 — 보드 탭 세션 가운데 일하는 중인 것이 있으면 묻는다(⌘Q·메뉴·Dock). pkill(업데이트 스크립트 등)은 여기를 거치지 않는다
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        MainActor.assumeIsolated { confirmShutdown(update: false) } == nil ? .terminateCancel : .terminateNow
    }
}

// 시험 빌드(build-board.sh --dev, 번들 id com.claude.board.dev)는 설치된 보드와 함께 떠야 하므로 잠금 파일·분산 알림 이름을 따로 쓴다
let instanceName = Bundle.main.bundleIdentifier == "com.claude.board.dev" ? "claude-board-dev" : "claude-board"

// 명령줄 동작: claude-board --action pause|resume|stop|reopen|focus <세션 id>
// 떠 있는 보드에 알림으로 넘긴다 — Ghostty 제어 권한을 가진 그 보드가 실행한다
if let i = CommandLine.arguments.firstIndex(of: "--action"), i + 2 < CommandLine.arguments.count {
    DistributedNotificationCenter.default().postNotificationName(
        .init("\(instanceName).action"), object: nil,
        userInfo: ["action": CommandLine.arguments[i + 1], "id": CommandLine.arguments[i + 2]], deliverImmediately: true)
    exit(0)
}

@MainActor func performAction(_ action: String, _ id: String) {
    // 떠 있는 보드의 Store 를 쓴다 — 새로 만들면 매번 1초 타이머와 알림 관찰자가 남아 계속 돈다
    guard let s = (NSApp.delegate as? AppDelegate)?.store.groups.flatMap(\.sessions).first(where: { $0.id == id }) else { return }
    switch action {
    case "pause": pauseSession(s)
    case "resume": resumePaused(s)
    case "stop": stopSession(s, confirm: false)
    case "reopen": resumeSession(s, carryOn: true)
    case "focus": openSession(s, double: true)   // 보드 탭 세션은 펼치고 그 탭으로, 다른 터미널은 그 세션의 터미널 창으로
    default: break
    }
}

// 이미 떠 있으면 새로 띄우지 않는다
let lockPath = NSTemporaryDirectory() + "\(instanceName).lock"
let lockFd = open(lockPath, O_CREAT | O_RDWR, 0o600)
if lockFd >= 0, flock(lockFd, LOCK_EX | LOCK_NB) != 0 {
    // 이미 떠 있는 창을 앞으로 불러오고 끝낸다
    DistributedNotificationCenter.default().postNotificationName(.init("\(instanceName).show"), object: nil,
                                                                 userInfo: nil, deliverImmediately: true)
    exit(0)
}

// 터미널 엔진의 리소스 번들(terminfo·셸 통합)은 .app 의 Contents/Resources 에 있다 — .app 맨 위는 서명이 거부한다.
// SwiftPM 이 만든 Bundle.module 은 .app 맨 위와 빌드한 컴퓨터의 빌드 경로만 찾으므로(못 찾으면 첫 터미널에서 죽는다,
// 빌드 경로가 남은 개발 Mac 에서는 멀쩡해 보인다), Bundle.module 이 한 번 정해지는 동안만 main 번들의 bundleURL 을
// Contents/Resources 로 돌려준다. 한 번 정해지면 바뀌지 않으므로 첫 TerminalController 전에 한 번 부른다
func resolveResourceBundleInApp() {
    guard Bundle.main.bundlePath.hasSuffix(".app"), let res = Bundle.main.resourceURL else { return }
    let sel = #selector(getter: Bundle.bundleURL)
    guard let m = class_getInstanceMethod(Bundle.self, sel) else { return }
    typealias Fn = @convention(c) (AnyObject, Selector) -> NSURL
    let original = method_getImplementation(m)
    let origFn = unsafeBitCast(original, to: Fn.self)
    let main = Bundle.main
    let block: @convention(block) (AnyObject) -> NSURL = { obj in obj === main ? res as NSURL : origFn(obj, sel) }
    method_setImplementation(m, imp_implementationWithBlock(block))
    _ = GhosttyRuntimeResources.directoryURL   // 여기서 Bundle.module 이 한 번 정해진다
    method_setImplementation(m, original)
}
resolveResourceBundleInApp()

// Claude Code 세션 안에서 보드를 띄우면(open -a·업데이트 단추) 그 세션의 표지 변수(CLAUDECODE·CLAUDE_CODE_CHILD_SESSION·
// CLAUDE_CODE_SESSION_ID 등)가 보드 환경에 그대로 들어온다(open 은 띄운 쪽 환경을 넘긴다, 실측). 보드 탭의 claude·셸이 그것을
// 물려받으면 claude 가 자식 세션으로 여겨 대화 기록을 저장하지 않는다 — 시작할 때 지운다.
// 사용자가 셸 설정에 둔 CLAUDE_* 값은 탭의 셸이 뜰 때 다시 들어온다
func inheritedSessionVariable(_ key: String) -> Bool {
    key.hasPrefix("CLAUDE") || key == "AI_AGENT" || key.hasPrefix("CODEX_COMPANION_")
}
for key in ProcessInfo.processInfo.environment.keys where inheritedSessionVariable(key) { unsetenv(key) }

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = AppDelegate()
app.delegate = delegate
app.run()
