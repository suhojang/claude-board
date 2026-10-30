// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import BoardRules
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI
import WebKit

// MARK: - 데이터

// 파일 수정 시각 — FileManager.attributesOfItem 은 확장 속성까지 읽어 느리므로(1초 주기 reload 의 가장 큰 몫이었다) stat 만 부른다
func mtime(_ path: String) -> Date? {
    var st = stat()
    guard stat(path, &st) == 0 else { return nil }
    return Date(timeIntervalSince1970: TimeInterval(st.st_mtimespec.tv_sec) + TimeInterval(st.st_mtimespec.tv_nsec) / 1e9)
}

// 세션·서브에이전트·카드 모델과 세션 기록 해석·카드 묶음 규칙은 BoardRules 모듈(Sources/BoardRules)에 있다 — Tests/BoardRulesTests 가 시험한다.
// SwiftUI 에도 Group 이 있어 그대로 쓰면 모호하다 — 이 파일의 별칭이 가져온 이름보다 앞선다(SwiftUI 의 것은 SwiftUI.Group 으로 쓴다)
typealias Group = BoardRules.Group

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

// 대화를 백그라운드로 보낸 세션 — Claude Code 는 대화를 새 세션 id 로 넘기고 원래 대화 기록 끝에
// {"type":"continued-in","continuedInSessionId":"<새 id>",…} 줄을 남긴다(2.1.312 실측). 그 뒤 hook 은 모두 새 id 로 와서
// 원래 세션 파일은 대기로 멈춘다. 꼬리에서 그 줄 뒤에 대화 줄(사이드체인 아닌 user·assistant)이 없으면 (새 id, 넘긴 시각)을 돌려준다 —
// 대화 줄이 있으면 사람이 원래 세션을 다시 이어 쓴 것이다. InterruptReader 와 같이 파일이 바뀔 때만, 그 표시가 있을 때만 JSON 을 푼다.
// InterruptReader 에 합치지 않은 것은 그쪽의 건너뛰는 줄(isMeta·< 로 시작하는 사용자 줄) 규칙이 달라서다 — 작업 중 세션은 꼬리를 두 번 읽는다
final class HandoffReader {
    private var parsed: [String: (mtime: Date, to: (id: String, at: Date)?)] = [:]

    func read(_ path: String) -> (id: String, at: Date)? {
        guard let changed = mtime(path) else { return nil }
        if let c = parsed[path], c.mtime == changed { return c.to }
        var to: (id: String, at: Date)? = nil
        if let h = FileHandle(forReadingAtPath: path) {
            let end = (try? h.seekToEnd()) ?? 0
            try? h.seek(toOffset: end > 256_000 ? end - 256_000 : 0)
            let data = (try? h.readToEnd()) ?? Data()
            try? h.close()
            // 따옴표까지 넣어 찾는다 — 대화 글 속의 continued-in 은 \" 로 감싸여 걸리지 않는다
            if data.range(of: Data("\"continued-in\"".utf8)) != nil {
                for line in data.split(separator: UInt8(ascii: "\n")).reversed() {
                    guard let j = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any],
                          let type = j["type"] as? String else { continue }
                    if type == "continued-in" {
                        if let id = j["continuedInSessionId"] as? String, !id.isEmpty {
                            to = (id, isoDate(j["timestamp"] as? String) ?? changed)
                        }
                        break
                    }
                    if (type == "user" || type == "assistant"), (j["isSidechain"] as? Bool) != true { break }   // 넘긴 뒤 다시 이어 썼다
                }
            }
        }
        parsed[path] = (changed, to)
        return to
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

    // 대화를 백그라운드 작업으로 넘기고 그 작업을 붙여 보여 주는 중인 claude 의 작업 id(이어 받은 세션 id 앞 8글자) — Claude Code 는
    // 넘길 때 parkedJobId 를 쓰고 풀 때 지운다(2.1.312 실측). 그 파일의 sessionId 가 이 세션일 때만(/clear 하면 같은 pid 가 새 id 를 쓴다)
    func parkedJob(_ pid: Int32, session: String) -> String? {
        guard let j = record(pid), j["sessionId"] as? String == session,
              let job = j["parkedJobId"] as? String, job.count >= 8 else { return nil }
        return job
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

    // sessionId → pid 색인 — 세션 기록에 pid 가 없는 세션(state.py 가 claude 를 못 찾음)의 생사를 보려고 이 폴더를 sessionId 로 찾는다.
    // 파일(<pid>.json)마다 mtime 과 sessionId 만 들고 파일이 바뀔 때만 다시 푼다. Claude Code 는 이 파일을 제자리에 고쳐 써
    // 폴더 mtime 이 바뀌지 않으므로 파일마다 mtime 을 본다. 위 cache 와 따로 두는 것은 prune 이 세션의 pid 만 남겨, 거기 넣으면 매초 모두 다시 읽기 때문이다.
    // 훑기는 판정마다 한 번이다 — scanSessions 가 판정 끝에 prune 을 부를 때 낡음으로 표시한다. pid 없는 끝나지 않은 세션이 없으면 훑지 않는다
    private var ids: [Int32: (mtime: Date, session: String?)] = [:]
    private var bySession: [String: Int32] = [:]
    private var idsFresh = false

    func pid(forSession session: String) -> Int32? {
        if !idsFresh {
            idsFresh = true
            var seen = Set<Int32>()
            for f in (try? FileManager.default.contentsOfDirectory(atPath: dir)) ?? [] where f.hasSuffix(".json") {
                guard let pid = Int32(f.dropLast(5)), pid > 0, let m = mtime(path(pid)) else { continue }
                seen.insert(pid)
                if ids[pid]?.mtime == m { continue }
                let j = FileManager.default.contents(atPath: path(pid)).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
                ids[pid] = (m, j?["sessionId"] as? String)
            }
            ids = ids.filter { seen.contains($0.key) }
            // 같은 sessionId 가 둘이면(강제로 죽은 claude 의 파일이 남은 채 같은 세션을 다시 연 경우) 가장 최근에 고쳐 쓴 것 — 남은 옛 파일의 pid 로
            // 살아 있는 세션을 끝났다고 보지 않게
            bySession = [:]
            for (pid, v) in ids {
                guard let sid = v.session, !sid.isEmpty else { continue }
                if let other = bySession[sid], let o = ids[other], o.mtime >= v.mtime { continue }
                bySession[sid] = pid
            }
        }
        return bySession[session]
    }

    func prune(keep: Set<Int32>) {
        cache = cache.filter { keep.contains($0.key) }
        idsFresh = false
    }
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

    // 목록에 남은 세션 몫만 남긴다 — 상태줄 기록(lines)은 세션 id, 대화 기록에서 센 컨텍스트(parsed)는 대화 기록 자리가 키다.
    // 배운 모델별 크기(windows)는 세션 몫이 아니라 그대로 둔다
    func prune(keep: Set<String>, keepTranscripts: Set<String>) {
        lines = lines.filter { keep.contains($0.key) }
        parsed = parsed.filter { keepTranscripts.contains($0.key) }
    }
}

// MARK: 세션 상태 판정의 진짜 어댑터 — 판정 규칙은 BoardRules.scanSessions, 포트는 BoardRules/Ports.swift(가짜 어댑터는 Tests 에만).
// 모두 Store.reload 가 메인 스레드에서만 부르므로 캐시에 잠금이 없다 — 포트가 Sendable 이라 @unchecked Sendable 로 적는다

// 세션 기록 — sessions/<sid>.json 과 보드가 남기는 표지(.paused·.mute·mute-all)
final class SessionFiles: SessionRecords, @unchecked Sendable {
    private let dir = sessionsDir
    // 매초 30여 개 파일을 다시 읽어 풀던 것을, 파일이 바뀔 때만 다시 읽는다
    private var records: [String: (mtime: Date, j: [String: Any])] = [:]

    func load() -> [SessionRecord] {
        let fm = FileManager.default
        var out: [SessionRecord] = []
        var seen = Set<String>()
        for name in (try? fm.contentsOfDirectory(atPath: dir)) ?? [] where name.hasSuffix(".json") {
            let path = (dir as NSString).appendingPathComponent(name)
            guard let m = mtime(path) else { continue }
            seen.insert(path)
            if let c = records[path], c.mtime == m {
                out.append(SessionRecord(name: name, file: path, json: c.j))
                continue
            }
            guard let data = fm.contents(atPath: path),
                  let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
            records[path] = (m, parsed)
            out.append(SessionRecord(name: name, file: path, json: parsed))
        }
        records = records.filter { seen.contains($0.key) }
        return out
    }

    func remove(_ record: SessionRecord) { try? FileManager.default.removeItem(atPath: record.file) }
    func isPaused(_ sid: String) -> Bool { FileManager.default.fileExists(atPath: (dir as NSString).appendingPathComponent("\(sid).paused")) }
    func isMuted(_ sid: String) -> Bool { FileManager.default.fileExists(atPath: (dir as NSString).appendingPathComponent("\(sid).mute")) }
    func allMuted() -> Bool { FileManager.default.fileExists(atPath: muteAllPath) }
}

// 프로세스 — 셸 수는 Store.refreshShells 가 뒤에서 센 것을 들고 있다가 준다(여기서 ps 를 돌리지 않는다)
final class SystemProcesses: ProcessProbe, @unchecked Sendable {
    var shellCounts: [Int32: Int] = [:]
    func isGone(_ pid: Int32) -> Bool { kill(pid, 0) != 0 && errno == ESRCH }
    func backgroundShells(_ pid: Int32) -> Int { shellCounts[pid] ?? 0 }
}

// 대화 기록 — 꼬리를 읽는 세 reader 를 묶는다
final class TranscriptFiles: Transcripts, @unchecked Sendable {
    private let models = ModelReader()
    private let interrupts = InterruptReader()
    private let handoffs = HandoffReader()

    func locate(given: String?, cwd: String, session: String) -> String { transcriptPath(given, cwd, session) }
    func exists(_ path: String) -> Bool { FileManager.default.fileExists(atPath: path) }
    func interruptedAt(_ path: String) -> Date? { interrupts.read(path) }
    func handoff(_ path: String) -> (id: String, at: Date)? { handoffs.read(path) }
    func brain(_ path: String, fallbackModel: String, now: Date) -> Brain? { models.read(path, fallbackModel: fallbackModel, now: now) }
    func prune(keep: Set<String>, keepHandoffs: Set<String>) {
        models.prune(keep: keep)
        interrupts.prune(keep: keep)
        handoffs.prune(keep: keepHandoffs)
    }
}

extension ClaudeNames: ClaudeLive, @unchecked Sendable {}
extension ContextReader: ContextUsage, @unchecked Sendable {}

struct SystemClock: Clock {
    var now: Date { Date() }
}

// 요금제 사용 한도(Limit)·사용량 해석·토큰 상태·캐시를 믿을지·받은 결과 처리는 BoardRules(Usage.swift)에 있다. isoDate 도 거기서 온다.
// 여기는 30초 타이머·백그라운드 받기·계정 전환 세대 확인·@Published 와 애니메이션만 쥔다
final class UsageStore: ObservableObject {
    @Published var limits: [Limit] = []
    @Published var failed = false
    // 로그인 토큰이 만료된 채로 있다(계정을 막 바꿨거나 한동안 세션이 없었다) — 세션이 다음 요청을 보내며 갱신해야 사용량을 받을 수 있다
    @Published var tokenExpired = false
    // 상태줄 스크립트와 같은 캐시 파일을 함께 쓴다 — 사용량 API 호출을 줄인다
    private let cache: UsageCache
    private let credentials: LoginCredentials
    private let api: UsageAPI
    private let clock: Clock
    private var fetching = false
    private var generation = 0
    // 계정을 바꾼 직후 이 시각 전에 쓰인 캐시는 믿지 않는다 — 상태줄이 앞 계정 토큰으로 받던 값을 늦게 써 넣을 수 있다
    private var trustCacheAfter = Date.distantPast
    private var timer: Timer?

    init(cache: UsageCache = UsageCacheFile(), credentials: LoginCredentials = KeychainCredentials(),
         api: UsageAPI = OAuthUsageAPI(), clock: Clock = SystemClock()) {
        self.cache = cache
        self.credentials = credentials
        self.api = api
        self.clock = clock
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in self?.refresh() }
    }

    // 계정을 바꾼 뒤: 앞 계정 값이 든 캐시를 버리고 바로 새로 받는다 — 상태줄도 같은 캐시를 쓰므로 함께 새 계정 값이 된다
    func reset() {
        generation += 1
        trustCacheAfter = clock.now.addingTimeInterval(5)
        cache.remove()
        failed = false
        withAnimation(.easeOut(duration: 0.2)) { limits = [] }
        refresh()
    }

    func refresh() {
        let check = checkUsageCache(cache: cache, clock: clock, trustAfter: trustCacheAfter, fetching: fetching)
        if let cached = check.cached { show(cached) }
        guard check.fetch else { return }
        fetching = true
        let gen = generation
        let (credentials, api, clock) = (self.credentials, self.api, self.clock)
        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let self else { return }
            let fetched = fetchUsage(credentials: credentials, api: api, clock: clock)
            DispatchQueue.main.async {
                self.fetching = false
                // 받는 사이 계정을 바꿨으면 앞 계정 값이므로 버리고 다시 받는다(캐시에도 쓰지 않는다)
                guard gen == self.generation else { self.refresh(); return }
                let settled = settleUsage(fetched, current: self.limits, trustAfter: self.trustCacheAfter, cache: self.cache)
                self.tokenExpired = settled.tokenExpired
                if let limits = settled.limits { self.show(limits) }
                self.trustCacheAfter = settled.trustAfter
                if let failed = settled.failed { self.failed = failed }
            }
        }
    }

    private func show(_ out: [Limit]) {
        if out != limits { withAnimation(.spring(response: 0.6, dampingFraction: 0.85)) { limits = out } }
    }
}

// MARK: 사용량의 진짜 어댑터 — 규칙은 BoardRules/Usage.swift, 포트는 BoardRules/Ports.swift(가짜 어댑터는 Tests 에만).
// 셋 다 상태가 없는 struct 라 그대로 Sendable 이다

// 시험 빌드만(보드 설정 usageOffline): 사용량 API·키체인 로그인 정보를 부르지 않고 공용 캐시를 읽기만 한다 — 자율주행 멈출 선 시험(6단계 ⑫-6)이
// 설치된 보드·상태줄과 함께 쓰는 캐시와 사용량 API 를 건드리지 않게. 설치판은 이 설정을 보지 않는다
let usageOffline = Bundle.main.bundleIdentifier == "com.claude.board.dev" && UserDefaults.standard.bool(forKey: "usageOffline")
struct ReadOnlyUsageCache: UsageCache {
    let inner = UsageCacheFile()
    func modifiedAt() -> Date? { inner.modifiedAt() }
    func read() -> Data? { inner.read() }
    func write(_ data: Data) {}
    func remove() {}
}
struct NoLoginCredentials: LoginCredentials { func read() -> Data? { nil } }

// 사용량 캐시 — 상태줄 스크립트와 함께 쓰는 파일. 자리·형식을 바꾸지 않는다
struct UsageCacheFile: UsageCache {
    let path = "/tmp/.claude-usage-cache.json"
    func modifiedAt() -> Date? { mtime(path) }
    func read() -> Data? { FileManager.default.contents(atPath: path) }
    func write(_ data: Data) { try? data.write(to: URL(fileURLWithPath: path), options: .atomic) }
    func remove() { try? FileManager.default.removeItem(atPath: path) }
}

// Claude Code 로그인 정보 — 키체인에서 읽기만 한다. 계정 전환이 쓰는 키체인 어댑터(SystemKeychain)·항목 주소(claudeLoginItem)를 그대로 쓴다.
// LoginCredentials 포트는 사용량 쪽 읽기 전용 역할로 따로 둔다 — 사용량 규칙에 쓰기·지우기를 열지 않는다
struct KeychainCredentials: LoginCredentials {
    func read() -> Data? { SystemKeychain().read(claudeLoginItem()) }
}

// 키체인의 Claude Code 로그인 토큰으로 사용량 API 를 부른다 — 토큰은 메모리에만 두고 기록하지 않는다
struct OAuthUsageAPI: UsageAPI {
    func fetch(token: String) -> Data? {
        var req = URLRequest(url: URL(string: "https://api.anthropic.com/api/oauth/usage")!, timeoutInterval: 10)
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("oauth-2025-04-20", forHTTPHeaderField: "anthropic-beta")
        let sem = DispatchSemaphore(value: 0)
        let result = FetchedBody()
        URLSession.shared.dataTask(with: req) { data, resp, _ in
            if (resp as? HTTPURLResponse)?.statusCode == 200 { result.put(data) }
            sem.signal()
        }.resume()
        _ = sem.wait(timeout: .now() + 12)
        return result.take()
    }
}

// 받은 본문 상자 — URLSession 콜백 스레드가 쓰고 기다린 스레드가 읽으므로 둘 다 잠금 안에서만 만진다.
// 기다린 쪽이 꺼낼 때 닫는다: 12초 기다림이 끝난 뒤 늦게 온 응답은 버린다(돌려준 뒤에 쓰지 않는다)
private final class FetchedBody: @unchecked Sendable {
    private let lock = NSLock()
    private var data: Data?
    private var closed = false
    func put(_ d: Data?) { lock.withLock { if !closed { data = d } } }
    func take() -> Data? { lock.withLock { closed = true; return data } }
}
