// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import AutopilotRules
import BoardRules
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI
import WebKit

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
    var storyBranches: [String] = []    // 스토리 가지 이름(저장소마다 같은 이름)
    var storyRepos: [String: [String: String]] = [:]   // 스토리 키 → 저장소 → 스토리 가지(만든 저장소만, 2026-10-28 여러 저장소에 걸친 스토리)
    var on: [String: String] = [:]      // 저장소 → 엔진이 지금 걸어 둔 가지(branches.repos.<저장소>.on)
    var checkRetries = 2
    var checkTimeout = 10.0
    var controlSeen = 0.0
    var units: [APUnit] = []
    var wrapped = false                 // 끝 정리(하네스 끝 검사·기록)를 마침 — 엔진 finish 가 받는 실행
    var mergeWaiting = false            // 끝났고 합치기·승격에 남은 단계가 있음(보드 본체의 끝난 실행 줄, 2단계 5-3)

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
    func repoName(_ u: APUnit) -> String { ((repoPath(u) ?? u.cwd) as NSString).lastPathComponent }
    func repoPath(_ u: APUnit) -> String? {
        if let w = worktreePath, u.cwd == w || u.cwd.hasPrefix(w + "/"), let r = repos.first { return r }
        return repos.filter { u.cwd == $0 || u.cwd.hasPrefix($0 + "/") }.max { $0.count < $1.count }
    }
    // 이 단위가 도는 동안 같은 스토리의 스토리 가지에 읽기 전용으로 걸린 다른 저장소(엔진 unitBranchGate, 2026-10-28) — 저장소 이름과 가지
    func readOnlyRepos(_ u: APUnit) -> [(name: String, branch: String)] {
        guard let k = u.story, let sr = storyRepos[k] else { return [] }
        let mine = repoPath(u)
        return on.keys.sorted().compactMap { r in
            guard r != mine, let b = on[r], sr[r] == b else { return nil }
            return ((r as NSString).lastPathComponent, b)
        }
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
            if let o = i["on"] as? String { x.on[k] = o }
        }
        for (k, v) in br["stories"] as? [String: Any] ?? [:] {
            for (r, e) in apStoryRepos(v) where e["head"] != nil { if let b = e["branch"] as? String, !b.isEmpty { x.storyRepos[k, default: [:]][r] = b } }
        }
        x.storyBranches = Set(x.storyRepos.values.flatMap(\.values)).sorted()
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
        x.wrapped = r["wrapped"] != nil
        if x.wrapped, !repos.isEmpty, !x.active {
            x.mergeWaiting = apMergeOpen(br, steps: apPromotion(x.project), stories: x.jira ? x.units.contains { $0.story == nil } : true)
        }
        return x
    }
}

// 프로젝트 설정의 승격 차례(projects/<프로젝트>.json promotion). 없으면 빈 목록
func apPromotion(_ project: String?) -> [APStep] {
    guard let p = project, let o = apRead(autopilotRoot + "/projects/\(p).json") as? [String: Any], let list = o["promotion"] as? [[String: Any]] else { return [] }
    return list.compactMap { s in (s["from"] as? String).flatMap { f in (s["into"] as? String).map { APStep(from: f, into: $0, after: s["after"] as? String) } } }
}

// 가지 기록 읽기·합치기 상태(APStep·APPromoted·APFailed·apStoryRepos·apPromotedRows·apFailedRows·apUnresolved·apStepDone·apMergeOpen·apMergeMark)는
// 규칙 라이브러리 AutopilotRules(Sources/AutopilotRules/Branches.swift)로 옮겼다 — 엔진과 함께 쓰고 표 시험이 붙어 있다

// 작업 폴더가 든 저장소(branches.repos 의 키, 따로 작업 트리면 그 작업 트리 자리도 본다) — 엔진 repoFor 와 같다
func apRepoFor(_ path: String, _ repos: [String: Any]) -> String? {
    repos.keys.filter { r in
        let w = (repos[r] as? [String: Any])?["work_tree"] as? String
        return path == r || path.hasPrefix(r + "/") || (w.map { path == $0 || path.hasPrefix($0 + "/") } ?? false)
    }.max { $0.count < $1.count }
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
    @Published private(set) var awaiting: [APRun] = []    // 끝났고 합치기·승격을 기다리는 실행(최근 것부터) — 보드 본체의 끝난 실행 줄
    @Published var screen: APScreen = .board { didSet { if screen != oldValue { tick() } } }
    private var cache: [String: (stamp: [Double], run: APRun?)] = [:]
    // 합치기·승격의 "나중에" — 실행 id → 다시 보일 시각. 하루 동안 보드 본체의 끝난 실행 줄에서 숨긴다(다시 알리는 알림은 4단계)
    private var later: [String: Double] = UserDefaults.standard.dictionary(forKey: "autopilotMergeLater") as? [String: Double] ?? [:]
    func snooze(_ id: String) {
        let now = Date().timeIntervalSince1970
        later = later.filter { $0.value > now }
        later[id] = now + 86_400
        UserDefaults.standard.set(later, forKey: "autopilotMergeLater")
        tick()
    }
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
        var live: [APRun] = [], seen = Set<String>(), view: APRun?, waiting: [APRun] = []
        let nowTs = Date().timeIntervalSince1970
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
            // 끝난 실행 줄은 저장소가 아직 있는 것만(장난감·지운 저장소의 옛 실행은 그리지 않는다) — 파일 있는지만 본다
            if !r.active, r.mergeWaiting, (later[r.id] ?? 0) < nowTs, r.repos.allSatisfy({ FileManager.default.fileExists(atPath: $0) }) { waiting.append(r) }
            if name == viewing { view = r }
        }
        cache = cache.filter { seen.contains($0.key) }
        live.sort { ($0.started ?? .infinity, $0.id) < ($1.started ?? .infinity, $1.id) }
        if live != runs { runs = live }
        waiting.sort { ($0.ended ?? 0, $0.id) > ($1.ended ?? 0, $1.id) }
        if waiting != awaiting { awaiting = waiting }
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
    var merge: APMergeMark?                             // 합치기 상태(규칙 apMergeMark). 가지 기록이 없는 실행은 nil
    var epic: String?
    var storyRepos: [String: [APPastStoryRepo]] = [:]   // 스토리 키 → 저장소마다 스토리 가지·합친 작업 수·마지막으로 받은 가지(기록 저장소 runs.branches)
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
    // 합치기 상태 글자와 색 — 어느 상태인지는 규칙 apMergeMark(AutopilotRules)가 정한다
    var mergeLabel: (String, Color)? {
        guard let m = merge else { return nil }
        switch m {
        case .discarded: return (m.label, APColor.muted)
        case .promoted: return (m.label, APColor.ok)
        case .waiting: return (m.label, APColor.wait)
        }
    }
    var stateLabel: (String, Color)? {
        switch state {
        case "stopped": ("중단됨", APColor.muted)
        case "error": ("오류로 멈춤", APColor.fail)
        default: nil
        }
    }
}

struct APPastStoryRepo: Equatable { var name = "", branch = ""; var made = false; var merged = 0; var into: String?; var discarded = false }

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
                r.merge = apMergeMark(b)
                let pro = apPromotedRows(b)
                let gone = (b["discarded"] as? [[String: Any]] ?? []).map { (($0["source"] as? String) ?? "") + "|" + (($0["repo"] as? String) ?? "") }
                for (k, v) in b["stories"] as? [String: Any] ?? [:] {
                    r.storyRepos[k] = apStoryRepos(v).sorted { $0.key < $1.key }.map { repo, e in
                        APPastStoryRepo(name: (repo as NSString).lastPathComponent, branch: e["branch"] as? String ?? "", made: e["head"] != nil,
                                        merged: (e["merged"] as? [Any])?.count ?? 0, into: pro.last { $0.source == "story:" + k && $0.repo == repo }?.into,
                                        discarded: gone.contains("story:" + k + "|" + repo))
                    }
                }
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
// 보드 본체에 두면 이 시계가 보드의 1초 갱신과 따로 보드 전체 배치를 다시 계산하게 해 CPU 가 늘었다(보드 본체의 시계 글자는 EverySecond·TickingText — SecondTicker)
struct APTick<Content: View>: View {
    @ViewBuilder let content: (Date) -> Content
    var body: some View { TimelineView(.periodic(from: .now, by: 1)) { ctx in content(ctx.date) } }
}

// 매초 바뀌는 글자 — 자리는 숫자를 모두 0 으로 바꾼 숨은 글자가 잡고 실제 글자는 겹쳐 그린다(자릿수가 바뀔 때만 크기를 다시 잰다).
// main 의 TickingText 와 같은 꼴이다(시계만 TimelineView). main 을 합친 뒤에도 자율주행 중 화면의 시계를 바꾸지 않으려고 따로 두었다 — 하나로 줄이는 것은 그 화면을 다시 볼 때
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
// 단위 시계는 부모가 1초 시계(EverySecond — Store.reload 가 바꾸는 SecondTicker)의 시각으로 셈한 분(minutes)으로 받는다 — 줄마다 TimelineView 를 두면 그 시계가 따로
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

// 끝났고 합치기·승격을 기다리는 실행의 줄(2단계 5-3, 와이어프레임 합치기·승격 ⑪ "보드의 실행 줄에서 엽니다") — 누르면 합치기·승격 시트.
// 시계가 없고 실행 값이 같으면 다시 그리지 않는다
struct APEndedLine: View, Equatable {
    let run: APRun
    var compact = false
    @State private var hover = false
    static func == (a: APEndedLine, b: APEndedLine) -> Bool { a.run == b.run && a.compact == b.compact }
    var body: some View {
        let sc = apStateChip(run)
        Button { APMerge.shared.open(run.dir) } label: {
            SwiftUI.Group {
                if compact {
                    HStack(spacing: 6) {
                        APMark()
                        APChip(text: "합치기 기다림", color: APColor.wait, strong: true)
                        Text(run.name).boardFont(11.5).lineLimit(1)
                        Spacer(minLength: 0)
                        Text("합치기·승격 →").boardFont(11, weight: .semibold).foregroundStyle(Palette.accent).fixedSize()
                    }
                } else {
                    // 보드 폭이 좁아(360pt 안팎) 두 줄 — 위: 표지·이름·끝 상태, 아래: 결과·합치기 기다림·들어가기
                    VStack(alignment: .leading, spacing: 5) {
                        HStack(spacing: 6) {
                            APMark()
                            Text(run.name).boardFont(12.5, weight: .bold).lineLimit(1)
                            APChip(text: sc.0, color: sc.1, strong: true)
                            Spacer(minLength: 0)
                        }
                        HStack(spacing: 6) {
                            Text("\(run.units.count)개 중 \(run.passed)개 통과").boardFont(10.5).monospacedDigit().foregroundStyle(.secondary).lineLimit(1)
                            Spacer(minLength: 4)
                            APChip(text: "합치기 기다림", color: APColor.wait, strong: true)
                            Text("합치기·승격 →").boardFont(11, weight: .semibold).foregroundStyle(Palette.accent).fixedSize()
                        }
                    }
                }
            }
            .padding(compact ? 6 : 9)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hover ? 0.08 : 0.04)))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(APColor.wait.opacity(0.30), lineWidth: 0.5))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("\(run.name) 실행이 끝났습니다. 눌러서 바뀐 줄을 보고 합치기·승격")
        .accessibilityIdentifier("ap-ended-\(run.id)")
    }
}

// 보드 머리 아래 줄 — 보기 고르기(모두·기본·자율주행), 지난 실행, 새 자율주행(출발 준비 시트). 닫은 준비가 있으면 "출발 준비" 단추로 다시 연다
struct APBar: View {
    @ObservedObject var ap: AutopilotStore
    @ObservedObject private var start = APStart.shared
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
            if start.hasDraft, !start.shown {
                APButton(title: start.preparing ? "출발 준비 중" : (start.step >= 3 || start.prepared ? "출발 준비 끝 · 이어 가기" : "출발 준비"),
                         icon: "steeringwheel", tint: Palette.accent, help: "닫아 둔 출발 준비를 다시 엽니다", axid: "ap-draft") { APStart.shared.open() }
            } else {
                APButton(title: "새 자율주행", icon: "plus", help: "출발 준비를 엽니다(지금은 지난 실행 이어 가기만)", axid: "ap-new") { APStart.shared.open() }
            }
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
                // 같은 스토리의 앞 작업이 다른 저장소에서 일했으면 그 저장소는 스토리 가지에 읽기 전용으로 걸려 있다(뒤 작업이 상대 경로로 본다)
                let ro = run.active && u.status == "running" ? run.readOnlyRepos(u) : []
                if !ro.isEmpty {
                    Text("읽기만: " + ro.map { "\($0.name) · \($0.branch)" }.joined(separator: "  ") + " (같은 스토리의 앞 작업)")
                        .boardFont(10, design: .monospaced).foregroundStyle(.secondary).lineLimit(2).truncationMode(.middle)
                        .accessibilityIdentifier("ap-unit-readonly")
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
                    if run.wrapped, !run.work.isEmpty || !run.storyBranches.isEmpty {
                        APBack(title: "합치기·승격 →", axid: "ap-to-merge") { APMerge.shared.open(run.dir) }
                    }
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
            // 스토리 가지는 저장소마다(한 스토리가 여러 저장소에 걸치면 같은 이름이 저장소마다 있다)
            let byBranch = Dictionary(grouping: run.storyRepos.values.flatMap { $0.map { (repo: $0.key, branch: $0.value) } }, by: \.branch)
            if !byBranch.isEmpty {
                s.append("스토리 가지 " + byBranch.keys.sorted().map { b in
                    b + "(" + (byBranch[b] ?? []).map { ($0.repo as NSString).lastPathComponent }.sorted().joined(separator: "·") + ")"
                }.joined(separator: " · "))
            }
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
            actionButtons(d.run)
        }
    }

    // 이 실행 이어 가기(출발 준비를 이 실행으로 엶, 2단계 5-2a) · 보고서 보기(3단계 보고서 틀이라 잠금)
    private func actionButtons(_ r: APPastRun) -> some View {
        WrapRow(spacing: 6) {
            APButton(title: "이 실행 이어 가기", primary: true, enabled: r.project != nil,
                     help: r.project == nil ? "프로젝트가 없는 실행이라 이어 갈 수 없습니다" : "남은 단위와 미룬 사람 몫을 불러와 출발 준비를 엽니다",
                     axid: "ap-continue-\(r.id)") { APStart.shared.open(continuing: r.id) }
            APButton(title: "합치기·승격", icon: "arrow.triangle.merge", primary: r.merge == .waiting, enabled: r.hasBranches && FileManager.default.fileExists(atPath: r.runDir + "/run.json"),
                     help: r.hasBranches ? "바뀐 줄을 보고 승격 차례대로 합치거나 버립니다" : "엔진이 만든 가지가 없는 실행입니다(또는 실행 폴더를 지웠습니다)",
                     axid: "ap-merge-\(r.id)") { APMerge.shared.open(r.runDir) }
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
            actionButtons(r)
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
                        Text(storyNote(d.run, g.key, g.units)).boardFont(10.5).foregroundStyle(.secondary)
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
                        Text(storyNote(d.run, g.key, g.units)).boardFont(10.5).foregroundStyle(.secondary)
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

    // 스토리 머리 줄 — 종류 · 합친 작업 수. 스토리가 여러 저장소에 걸치면 저장소마다 합친 작업 수와 마지막으로 받은 가지
    private func storyNote(_ run: APPastRun, _ key: String?, _ units: [APPastUnit]) -> String {
        let kind = units.first.map { apKind($0.kind) } ?? ""
        let merged = units.filter { $0.merged == "merged" }.count
        var parts = [kind, key == nil ? "" : "합친 작업 \(merged)/\(units.count)"]
        if let k = key, let rs = run.storyRepos[k], rs.count > 1 {
            parts += rs.map { $0.name + " " + (!$0.made ? "가지 없음" : $0.discarded ? "버림" : "\($0.merged)" + ($0.into.map { " → \($0)" } ?? "")) }
        }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
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

// MARK: 자율주행 · 출발 화면

// 2단계 5-2a: 출발 준비 네 단계(원천 고르기 → 보드가 준비 → 단위 제안·출발 카드 → 출발 확인)와 출발(설계 2장, 와이어프레임 5판 start·confirm).
// 원천은 모델이 필요 없는 "지난 실행 이어 가기"만 연다. 보드는 출발 전 실행 폴더를 drafts/<실행 id>/ 에 쓰고(run.json·queue.json, 출발 때 cards.json·answers.json)
// 엔진 status --json·prerun 을 불러 준비 현황을 그린다. 출발을 누르면 그 폴더를 runs/ 로 옮기고 엔진 start 를 부른다(거부 문장은 그대로 보인다).
// 출발 가지·승격 차례는 projects/<프로젝트>.json 에 남긴다. 기록 저장소는 그대로 읽기 전용이다(카드와 답은 엔진이 출발 때 옮겨 담는다)

let apDevBuild = Bundle.main.bundleIdentifier == "com.claude.board.dev"
func apShort(_ s: String, _ n: Int = 300) -> String { s.count > n ? String(s.prefix(n)) + "…" : s }
var apDraftsRoot: String { autopilotRoot + "/drafts" }

// 앱 안 엔진(claude-autopilot)을 부른다 — 표준 출력과 오류를 한데 받는다. 오래 걸릴 수 있어 메인 스레드에서 부르지 않는다
func apEngine(_ args: [String]) -> (code: Int32, out: String) {
    let exe = Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("claude-autopilot").path ?? ""
    guard FileManager.default.isExecutableFile(atPath: exe) else { return (-1, "앱 안에 자율주행 엔진(claude-autopilot)이 없습니다: \(exe)") }
    let p = Process()
    p.executableURL = URL(fileURLWithPath: exe)
    p.arguments = args
    let pipe = Pipe()
    p.standardOutput = pipe
    p.standardError = pipe
    p.standardInput = FileHandle.nullDevice
    guard (try? p.run()) != nil else { return (-1, "엔진을 띄우지 못했습니다: \(exe)") }
    let d = pipe.fileHandleForReading.readDataToEndOfFile()
    p.waitUntilExit()
    return (p.terminationStatus, String(decoding: d, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
}

// 보드가 읽는 git(지금 가지·가지 목록·머리·커밋 안 한 변경·커밋 수). 엔진 밖 git 과 같은 보호: 저장소 설정의 fsmonitor·hooks 를 쓰지 않고
// 시스템·전역 설정·attributes·replace 를 읽지 않는다(사용자 저장소 설정에 심은 명령이 보드에서 돌지 않게). 메인 스레드에서 부르지 않는다
func apGit(_ args: [String], _ cwd: String) -> String? {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
    p.arguments = ["--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.attributesFile=/dev/null"] + args
    p.currentDirectoryURL = URL(fileURLWithPath: cwd)
    var env = ProcessInfo.processInfo.environment
    for (k, v) in [("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")] { env[k] = v }
    p.environment = env
    let out = Pipe()
    p.standardOutput = out
    p.standardError = FileHandle.nullDevice
    guard (try? p.run()) != nil else { return nil }
    let d = out.fileHandleForReading.readDataToEndOfFile()
    p.waitUntilExit()
    return p.terminationStatus == 0 ? String(decoding: d, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines) : nil
}

// JSON 을 임시 파일을 거쳐 바꿔 넣는다(엔진이 읽는 파일이 반쯤 쓰인 채 보이지 않게)
@discardableResult func apWriteJSON(_ path: String, _ obj: Any) -> Bool {
    guard let d = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]) else { return false }
    let tmp = path + ".tmp-\(getpid())"
    guard FileManager.default.createFile(atPath: tmp, contents: d + Data("\n".utf8)), rename(tmp, path) == 0 else { unlink(tmp); return false }
    return true
}

// 따로 작업 트리로 돈 실행의 단위 정의는 엔진이 작업 트리 경로로 바꿔 고정했다 — 이어 갈 때 원래 저장소 경로로 되돌린다.
// 엔진 remapToWorktree 의 거꾸로이고 경계 규칙도 같다(경로 뒤에 /·끝·따옴표·공백이 올 때만, toy 와 toy2 를 가른다)
func apUnmap(_ s: String, from wt: String, to repo: String) -> String {
    var out = "", i = s.startIndex
    while let r = s.range(of: wt, range: i..<s.endIndex) {
        out += s[i..<r.lowerBound]
        let after: Character = r.upperBound < s.endIndex ? s[r.upperBound] : " "
        out += "/ \"'\n`".contains(after) ? repo : wt
        i = r.upperBound
    }
    return out + s[i...]
}

struct APOption: Equatable { let key: String; let label: String; var note = "" }

// 출발 카드(설계 1장 "사람 몫은 출발 카드로만"). 5-2a 에서 생기는 카드는 판정 미리 통과(엔진 prerun), 지난 실행에서 미룬 사람 몫, 단위 크기로
// 모두 실행마다의 답이다. policy 카드(정책 답)는 출발 때 projects/<프로젝트>.json policy_answers 에 남기고 다음 출발에 미리 고른다 —
// 그런 카드(하네스 확인 지점 7단계, 새 티켓 4단계)는 아직 만들지 않는다
struct APCard: Equatable, Identifiable {
    let id: String
    var kind = "", label = "", title = "", why = ""
    var options: [APOption] = []
    var recommended = ""
    var unit: String?            // 이 카드가 정하는 단위 id(빼기 답이면 대기열에서 뺀다)
    var policy = false
    var json: [String: Any] {
        var o: [String: Any] = ["id": id, "kind": kind, "title": title, "why": why, "recommended": recommended, "policy": policy,
                                "options": options.map { ["key": $0.key, "label": $0.label, "note": $0.note] }]
        if let unit { o["unit"] = unit }
        return o
    }
}

// 출발 준비의 단위 하나 — 지난 실행 queue.json 의 정의(작업 트리 경로는 원래 저장소로 되돌림)와 그 실행에서 끝난 상태
struct APDraftUnit: Equatable, Identifiable {
    let id: String               // 단위 id(depends 가 가리킨다)
    var n = 0                    // 지난 실행에서의 번호
    var title = "", kind = "", last = "", check = "", sourceKey = ""
    var est: Double?             // 출발 때 예상(분, 보정 전)
    var capMin = 120.0
    var depends: [String] = []
    var story: String?, origStory: String?, task: String?
    var def = Data()             // queue.json 에 쓸 정의(JSON)
    var blocked: String?         // 기계 검사에 걸려 고를 수 없는 까닭
    var excluded: String?        // 준비에서 뺀 까닭(이미 끝남) — 되살리기 전까지 후보 밖
    var checked = true
}

struct APPrepLine: Equatable, Identifiable {
    let id: String
    var title: String
    var state = "wait"           // wait·run·done·warn·fail·skip
    var started: Double?
    var sec: Double?
    var note = ""
    var detail: [String] = []
}

struct APRepoInfo: Equatable {
    var current: String?
    var branches: [String] = []
    var heads: [String: String] = [:]
    var dirty: [String] = []
    var devAhead: Int?           // dev 에만 있는 커밋 수(live 기준) — dev 를 출발 가지로 고르면 함께 간다
}

// 보정한 예상 — 계수는 안에서만 쓰고 사람에게는 보정한 값이라는 것과 표본 수만 보인다(설계 2장, 5장 7절)
struct APEstimate: Equatable {
    var min: Double?
    var pp: Double?
    var usd: Double?
    var samples = 0
    var wide = false             // 같은 프로젝트 표본이 모자라 모든 프로젝트로 넓힘
    var short: Bool { samples < 3 }
}

struct APStory: Equatable, Identifiable {
    let id: String               // 스토리 키 또는 새 스토리(new-k)
    var key: String?
    var kind = ""
    var title = ""
    var isNew: Bool { key == nil }
}

extension APRecords {
    struct Sample { let project: String; let kind: String; let model: String; let est: Double; let min: Double; let usd: Double; let pp: Double }

    // 추정 보정 표본 — 통과한 단위 가운데 출발 때 예상과 단위 시계가 있는 것
    static func samples(_ path: String) -> [Sample] {
        guard let db = open(path) else { return [] }
        defer { sqlite3_close_v2(db) }
        var err: String?
        let sql = """
            SELECT COALESCE(r.project_id, ''), COALESCE(u.kind, ''), COALESCE(u.model, r.model, ''), u.est_min, u.active_sec, COALESCE(u.cost_usd, 0), COALESCE(u.weekly_pp, 0)
            FROM units u JOIN runs r ON r.id = u.run_id WHERE u.status = 'done' AND u.est_min > 0 AND u.active_sec > 0
            """
        return rows(db, sql, error: &err).map { c in
            Sample(project: c[0] as? String ?? "", kind: c[1] as? String ?? "", model: c[2] as? String ?? "", est: c[3] as? Double ?? 0,
                   min: (c[4] as? Double ?? 0) / 60, usd: c[5] as? Double ?? 0, pp: c[6] as? Double ?? 0)
        }
    }

    // 이어 가기의 가리기 — 지난 실행 뒤로 같은 프로젝트에서 통과한 원천 항목 키(키 → 실행 id)와 답한 카드(카드 id → 답).
    // 그리고 지난 실행이 이어 간 실행들에서 다시 미룬(park) 사람 몫(이어 간 줄을 따라 열 단계까지)
    static func later(_ path: String, project: String, run: APPastRun) -> (passed: [String: String], answered: [String: String], parked: [APCarry], root: String?) {
        guard let db = open(path) else { return ([:], [:], [], nil) }
        defer { sqlite3_close_v2(db) }
        var err: String?
        var passed: [String: String] = [:], answered: [String: String] = [:], parked: [APCarry] = []
        let after = String(run.started ?? 0)
        for c in rows(db, """
            SELECT u.source_key, u.run_id FROM units u JOIN runs r ON r.id = u.run_id
            WHERE r.project_id = ? AND r.id <> ? AND COALESCE(r.started, r.updated) > CAST(? AS REAL) AND u.status = 'done' AND u.source_key IS NOT NULL
            """, [project, run.id, after], error: &err) {
            if let k = c[0] as? String { passed[k] = c[1] as? String ?? "" }
        }
        for c in rows(db, """
            SELECT k.card_id, k.answer FROM cards k JOIN runs r ON r.id = k.run_id
            WHERE r.project_id = ? AND r.id <> ? AND COALESCE(r.started, r.updated) > CAST(? AS REAL) AND k.answer IS NOT NULL
            """, [project, run.id, after], error: &err) {
            if let k = c[0] as? String { answered[k] = c[1] as? String ?? "" }
        }
        var cur = run.id
        for _ in 0..<10 {
            guard let prev = rows(db, "SELECT continues FROM runs WHERE id = ?", [cur], error: &err).first?.first as? String, !prev.isEmpty else { break }
            for c in rows(db, """
                SELECT c.id, c.unit_n, c.kind, c.title, c.why FROM cards k JOIN carry c ON c.id = k.card_id
                WHERE k.run_id = ? AND k.answer = 'park' AND c.kind = 'next_start'
                """, [cur], error: &err) {
                parked.append(APCarry(id: c[0] as? String ?? "", unit: Int(c[1] as? Double ?? 0), kind: "next_start",
                                      title: apPlain(c[3] as? String ?? ""), why: apPlain(c[4] as? String ?? "")))
            }
            cur = prev
        }
        let root = rows(db, "SELECT root FROM projects WHERE id = ?", [project], error: &err).first?.first as? String
        return (passed, answered, parked, root)
    }
}

// 출발 준비 하나 — 시트를 닫아도 남아 뒤에서 이어 간다(설계 2장 "준비하는 중에 시트를 닫아도"). 상태가 바뀔 때만 알린다(시계·애니메이션 없음)
@MainActor final class APStart: ObservableObject {
    static let shared = APStart()

    @Published var shown = false
    @Published var step = 1
    @Published private(set) var past: [APPastRun] = []
    @Published private(set) var pastId: String?
    @Published private(set) var root = ""
    @Published private(set) var project: String?
    @Published private(set) var jira = false
    @Published private(set) var setText: String?
    @Published private(set) var setAt: Double?
    @Published var units: [APDraftUnit] = []
    @Published private(set) var carryPark: [APCarry] = []
    @Published private(set) var carryNew: [APCarry] = []
    @Published private(set) var noDefs: String?
    @Published private(set) var lines: [APPrepLine] = []
    @Published private(set) var preparing = false
    @Published private(set) var prepared = false
    @Published private(set) var rechecking = false
    @Published private(set) var problems: [String] = []
    @Published private(set) var mode = ""
    @Published private(set) var modeReason = ""
    @Published private(set) var repos: [String] = []
    @Published private(set) var repoInfo: [String: APRepoInfo] = [:]
    @Published private(set) var storyBranch: [String: String] = [:]   // 단위 id → 엔진이 정한 스토리 가지
    @Published private(set) var prerunPassed: Set<String> = []
    private var prerunIds: Set<String> = []          // 판정 미리 돌리기를 거친 단위(되살린 단위는 출발 확인으로 갈 때 마저 돌린다)
    @Published private(set) var prerunNote = ""
    @Published var cards: [APCard] = []
    @Published var answers: [String: String] = [:]
    @Published private(set) var policyLoaded: Set<String> = []
    @Published var bases: [String: String] = [:]
    @Published private(set) var baseNote = ""
    @Published private(set) var promotion: [APStep] = []
    @Published private(set) var promotionSaved = false
    @Published var epic = ""
    @Published var stories: [APStory] = []
    @Published private(set) var estimates: [String: APEstimate] = [:]
    @Published private(set) var starting = false
    @Published private(set) var startError: String?
    @Published private(set) var changedBy: String?         // 카드 답이 고른 단위를 바꿨을 때(접힌 "고른 단위" 머리 강조)
    @Published private(set) var flash = 0
    // 이어 갈 지난 Jira 실행이 만든 스토리·작업 가지 가운데 아직 남아 출발을 막는 것(2026-10-28 사용자 결정: 먼저 합치거나 버린 뒤 이어 감, 2단계 5-3)
    @Published private(set) var leftBranches: [String] = []
    private(set) var id = ""
    private(set) var dir = ""
    private var settings: [String: Any] = [:]
    private var tickets: [String: Any] = [:]
    private var needNewId = false
    private var proposedOnce = false
    private let queue = DispatchQueue(label: "claude-board.autopilot.start")
    private var sheet: NSWindow?
    private var resizeWatch: NSObjectProtocol?

    var hasDraft: Bool { pastId != nil }
    var pastRun: APPastRun? { past.first { $0.id == pastId } }
    var model: String { settings["model"] as? String ?? "?" }
    var stop7: Double { ((settings["stopline"] as? [String: Any]).flatMap { apNum($0["seven_day"]) } ?? 0.80) * 100 }
    var stop5: Double { ((settings["stopline"] as? [String: Any]).flatMap { apNum($0["five_hour"]) } ?? 0.95) * 100 }
    var title: String { settings["title"] as? String ?? "" }
    var choosable: [APDraftUnit] { units.filter { $0.excluded == nil && $0.blocked == nil } }
    // 같은 경로·같은 프로젝트에서 도는 실행 — 끝날 때까지 출발을 잠근다(엔진은 이것을 보지 않는다. 같은 저장소의 두 실행이 부딪히지 않게)
    var activeHere: String? {
        guard hasDraft else { return nil }
        return AutopilotStore.shared.runs.first { (!root.isEmpty && $0.covers(root)) || ($0.project != nil && $0.project == project) }?.id
    }

    // 카드가 단위에 내린 결정 — 답하지 않은 카드가 있으면 "pending", 빼기·판정 고치기 답이면 "out"
    func cardDecision(_ u: APDraftUnit) -> String {
        let mine = cards.filter { $0.unit == u.id }
        if mine.contains(where: { ["drop", "fix_check"].contains(answers[$0.id] ?? "") }) { return "out" }
        if mine.contains(where: { answers[$0.id] == nil }) { return "pending" }
        return "in"
    }
    // 대기열에 들어갈 단위(체크했고, 뺀 것·막힌 것이 아니고, 카드가 빼지 않음). 답하지 않은 카드가 걸린 단위는 아직 세지 않는다
    var chosen: [APDraftUnit] { choosable.filter { $0.checked && cardDecision($0) == "in" } }
    var heldByCards: [APDraftUnit] { choosable.filter { $0.checked && cardDecision($0) == "pending" } }

    func estimate(_ u: APDraftUnit) -> APEstimate { estimates[u.id] ?? APEstimate(min: u.est) }
    func sum(_ list: [APDraftUnit]) -> (pp: Double?, min: Double, usd: Double?) {
        var pp: Double? = 0, usd: Double? = 0, mins = 0.0
        for u in list {
            let e = estimate(u)
            mins += e.min ?? 0
            pp = pp.flatMap { a in e.pp.map { a + $0 } }
            usd = usd.flatMap { a in e.usd.map { a + $0 } }
        }
        return (pp, mins, usd)
    }
    var usage7: Double? { (NSApp.delegate as? AppDelegate)?.usage.limits.first { $0.id == "7d" }?.pct }
    var budget: Double? { usage7.map { max(0, stop7 - $0) } }

    // 시트 — 창 전체를 덮는 시트(와이어프레임 "창 전체 시트"). 보드를 앞으로 꺼내지 않는다(시험 중 사용자가 치던 글자가 보드로 새지 않게)
    func open(continuing runId: String? = nil) {
        if !sweptDrafts { sweepDrafts() }
        if let runId, runId != pastId {
            if hasDraft, !preparing, !starting { dropFolder(); reset() }
            if !hasDraft { load(select: runId) }
        } else if !hasDraft { reset(); load(select: nil) }
        checkLeftovers()
        present()
    }
    // 보드를 끄면 준비는 사라진다(메모리에만) — 보드가 처음 출발 준비를 열 때 drafts/ 에 남은 출발하지 않은 폴더를 치운다(drafts/ 는 보드만 쓴다)
    private var sweptDrafts = false
    private func sweepDrafts() {
        sweptDrafts = true
        for n in (try? FileManager.default.contentsOfDirectory(atPath: apDraftsRoot)) ?? [] where !n.hasPrefix(".") {
            let d = apDraftsRoot + "/" + n
            if (apRead(d + "/run.json") as? [String: Any])?["state"] == nil { try? FileManager.default.removeItem(atPath: d) }
        }
    }
    private func dropFolder() {
        if !dir.isEmpty, (apRead(dir + "/run.json") as? [String: Any])?["state"] == nil { try? FileManager.default.removeItem(atPath: dir) }
    }

    private func present() {
        guard window != nil, window.attachedSheet == nil else { return }
        let size = sheetSize()
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        let host = NSHostingController(rootView: APStartSheet(s: self))
        host.sizingOptions = []   // 시트 크기는 창에 맞춘다(SwiftUI 이상 크기로 창이 줄거나 늘지 않게)
        w.contentViewController = host
        w.setContentSize(size)
        sheet = w
        shown = true
        window.beginSheet(w)
        resizeWatch = NotificationCenter.default.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { if let s = self?.sheet { s.setContentSize(self?.sheetSize() ?? s.frame.size) } }
        }
    }
    private func sheetSize() -> NSSize {
        let c = window.contentLayoutRect.size
        return NSSize(width: max(c.width - 8, 480), height: max(c.height - 8, 460))
    }
    // 닫기 — 준비는 남아 뒤에서 이어 간다(보드 머리 아래 줄의 "출발 준비"로 다시 연다)
    func close() {
        if let s = sheet { window.endSheet(s) }
        sheet = nil
        if let r = resizeWatch { NotificationCenter.default.removeObserver(r) }
        resizeWatch = nil
        shown = false
    }
    // 준비 버리기 — 출발하지 않은 실행 폴더(state 가 없는 것)만 지운다
    func discard() {
        guard !starting else { return }
        dropFolder()
        close()
        reset()
    }

    private func reset() {
        step = 1; past = []; pastId = nil; root = ""; project = nil; jira = false; setText = nil; setAt = nil
        units = []; carryPark = []; carryNew = []; noDefs = nil; lines = []; preparing = false; prepared = false; rechecking = false
        problems = []; mode = ""; modeReason = ""; repos = []; repoInfo = [:]; storyBranch = [:]; prerunPassed = []; prerunIds = []; prerunNote = ""
        cards = []; answers = [:]; policyLoaded = []; bases = [:]; baseNote = ""; promotion = []; promotionSaved = false
        epic = ""; stories = []; estimates = [:]; starting = false; startError = nil; changedBy = nil; leftBranches = []
        id = ""; dir = ""; settings = [:]; tickets = [:]; needNewId = false; proposedOnce = false
    }

    private var db: String { autopilotRoot + "/private/records.sqlite" }

    // 이어 갈 수 있는 지난 실행 — 기록 저장소의 끝난 실행 가운데 프로젝트가 있는 것
    private func load(select: String?) {
        past = APRecords.list(db).runs.filter { $0.project != nil }
        choose(select ?? past.first?.id)
    }

    // 지난 실행 하나를 고른다 — 남은·미통과·상한·중단·건너뛴 단위(정의는 그 실행 폴더의 queue.json), 미룬 사람 몫, 새로 찾은 일, 실행 설정
    func choose(_ runId: String?) {
        guard !preparing, !starting, let runId, let run = past.first(where: { $0.id == runId }), let proj = run.project else { return }
        dropFolder()
        reset()
        past = APRecords.list(db).runs.filter { $0.project != nil }
        pastId = runId
        project = proj
        let detail = APRecords.detail(db, run) ?? APPastDetail(run: run)
        let later = APRecords.later(db, project: proj, run: run)
        let pr = apRead(run.runDir + "/run.json") as? [String: Any] ?? [:]
        let q = apRead(run.runDir + "/queue.json") as? [[String: Any]] ?? []
        let wt = pr["worktree"] as? [String: Any] ?? [:]
        let wtPath = wt["mode"] as? String == "worktree" ? wt["path"] as? String : nil, wtRepo = wt["repo"] as? String
        func unmap(_ s: String) -> String { (wtPath != nil && wtRepo != nil) ? apUnmap(s, from: wtPath!, to: wtRepo!) : s }
        // 실행 설정은 지난 실행을 그대로 잇는다(설계 판단: 바꾸기는 출발 확인의 설정 줄, 다음 조각). 출발 가지는 새로 고르고, 시험용 칸은 시험 빌드만
        var keys = ["title", "model", "effort", "stopline", "caps", "check_timeout_min", "check_retries", "precheck_retry_min", "background_quiet_sec",
                    "allowed_tools", "disallowed_tools", "write_roots", "network_allow", "local_binding", "env_pass", "unix_sockets", "read_deny",
                    "sandbox_exclude", "project", "project_root", "worktree_mode", "worktree_files"]
        if apDevBuild { keys += ["claude_args", "claude_path"] }
        for k in keys { if let v = pr[k], !(v is NSNull) { settings[k] = v } }
        settings["project"] = proj
        if let roots = settings["write_roots"] as? [String] { settings["write_roots"] = roots.map(unmap) }
        let t = (pr["title"] as? String ?? run.title).replacingOccurrences(of: " 이어 가기", with: "")
        settings["title"] = (t.isEmpty ? proj : t) + " 이어 가기"
        root = later.root ?? wtRepo ?? (wt["repos"] as? [String])?.first ?? ""
        let ps = apRead(autopilotRoot + "/projects/\(proj).json") as? [String: Any] ?? [:]
        tickets = ps["tickets"] as? [String: Any] ?? [:]
        epic = (pr["epic"] as? String) ?? run.epic ?? ""   // 에픽은 이어 가기일 때만 지난 실행의 것을 채운다(2026-10-27 사용자 결정)
        if q.isEmpty { noDefs = "지난 실행 폴더(\(run.runDir.replacingOccurrences(of: NSHomeDirectory(), with: "~")))의 queue.json 이 없어 남은 단위의 정의(프롬프트·판정)를 읽을 수 없습니다" }
        let open = ["pending", "running", "failed", "capped", "stopped", "skipped"]
        let defKeys = ["id", "title", "prompt", "check", "precheck", "action", "context", "cwd", "caps", "check_timeout_min", "action_timeout_min",
                       "tickets", "review", "source_key", "kind", "est_min", "est_basis", "story", "depends", "prerun"]
        let defaultCap = APCaps.from(pr["caps"]).minutes
        for (i, d0) in q.enumerated() {
            let n = i + 1
            let st = detail.units.first { $0.n == n }?.status ?? d0["status"] as? String ?? "pending"
            guard open.contains(st) else { continue }
            var d: [String: Any] = [:]
            for k in defKeys { if let v = d0[k], !(v is NSNull) { d[k] = v } }
            for k in ["cwd", "prompt", "check", "precheck", "action"] { if let s = d[k] as? String { d[k] = unmap(s) } }
            let uid = d["id"] as? String ?? "u\(n)"
            d["id"] = uid
            let key = d["source_key"] as? String ?? "\(runId)/\(n)"
            d["source_key"] = key
            var u = APDraftUnit(id: uid)
            u.n = n
            u.title = d["title"] as? String ?? "단위 \(n)"
            u.kind = d["kind"] as? String ?? detail.units.first { $0.n == n }?.kind ?? ""
            if !u.kind.isEmpty { d["kind"] = u.kind }
            u.last = st
            u.check = d["check"] as? String ?? ""
            u.sourceKey = key
            u.est = apNum(d["est_min"]) ?? detail.units.first { $0.n == n }?.estMin
            u.capMin = APCaps.from(d["caps"], APCaps(minutes: defaultCap)).minutes
            u.depends = apStrings(d["depends"])
            u.story = apText(d["story"]); u.origStory = u.story
            u.task = u.story == nil ? nil : apStrings(d["tickets"]).first
            if let by = later.passed[key] { u.excluded = "이미 끝남: \(by) 에서 통과" }
            // 기계 검사(보드 몫): 다른 가지를 맞추라는 지시(가지 강제 이동·ref 고치기·push 꼴)는 단위로 받지 않는다(설계 2장 끝)
            let text = [d["prompt"], d["check"], d["precheck"], d["action"]].compactMap { $0 as? String }.joined(separator: "\n")
            if let hit = ["git branch -f", "git update-ref", "update-ref ", "git push", "git checkout -B"].first(where: { text.contains($0) }) {
                u.blocked = "다른 가지를 맞추는 명령(\(hit.trimmingCharacters(in: .whitespaces)))이 들어 있어 단위로 받지 않습니다"
            }
            u.def = (try? JSONSerialization.data(withJSONObject: d, options: [.sortedKeys])) ?? Data()
            units.append(u)
        }
        // 이어 간 실행에서 다시 미룬 몫은 그 실행의 모델이 같은 일을 다시 미뤘으면(같은 제목) 한 번만 보인다
        let carried = detail.carry + later.parked.filter { p in !detail.carry.contains { $0.id == p.id || ($0.kind == p.kind && $0.title == p.title) } }
        carryPark = carried.filter { $0.kind == "next_start" && !["decide", "close"].contains(later.answered[$0.id] ?? "") }
        carryNew = carried.filter { $0.kind == "new_work" }
        jira = !tickets.isEmpty || units.contains { $0.story != nil }
        var seen = Set<String>()
        for u in units { if let s = u.story, seen.insert(s).inserted { stories.append(APStory(id: s, key: s, kind: u.kind)) } }
        let setPath = autopilotRoot + "/private/projects/\(proj)/set/autopilot.md"
        setText = (try? String(contentsOfFile: setPath, encoding: .utf8)).flatMap { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : $0 }
        setAt = mtime(setPath)?.timeIntervalSince1970
        estimates = calibrate()
        checkLeftovers()
    }

    // 지난 Jira 실행의 남은 가지 — 엔진은 이 실행이 만든 가지만 움직이므로, 같은 스토리·작업 키를 쓰는 이어 가기는 그 가지가 남아 있으면 출발 전에 거부한다.
    // 끝나지 않은 작업이 있는 스토리의 스토리 가지와 그 끝나지 않은 작업의 작업 가지 가운데 지금 있는 것을 본다(git 은 백그라운드)
    func checkLeftovers() {
        guard let run = pastRun, jira else { if !leftBranches.isEmpty { leftBranches = [] }; return }
        let rd = run.runDir, rid = run.id
        DispatchQueue.global(qos: .userInitiated).async {
            let left = APStart.leftovers(rd)
            DispatchQueue.main.async { MainActor.assumeIsolated { if self.pastId == rid, self.leftBranches != left { self.leftBranches = left } } }
        }
    }
    nonisolated static func leftovers(_ runDir: String) -> [String] {
        guard let r = apRead(runDir + "/run.json") as? [String: Any] else { return [] }
        let br = r["branches"] as? [String: Any] ?? [:]
        let stories = br["stories"] as? [String: Any] ?? [:], repos = br["repos"] as? [String: Any] ?? [:]
        let q = apRead(runDir + "/queue.json") as? [[String: Any]] ?? []
        let open = q.filter { $0["story"] is String && ($0["status"] as? String ?? "pending") != "done" }
        var want: [(repo: String, branch: String)] = []
        for k in Set(open.compactMap { $0["story"] as? String }).sorted() {
            // 스토리 가지는 그 스토리가 가지를 만든 저장소마다, 작업 가지는 그 작업 폴더가 든 저장소에서(2026-10-28 여러 저장소에 걸친 스토리)
            let rs = apStoryRepos(stories[k])
            guard !rs.isEmpty else { continue }
            for (repo, e) in rs.sorted(by: { $0.key < $1.key }) { if let b = e["branch"] as? String, !b.isEmpty { want.append((repo, b)) } }
            // 엔진은 이어 갈 작업마다 그 저장소에 스토리 가지·작업 가지가 있으면 출발을 거부한다 — 작업의 저장소에서 스토리 가지 이름도 본다
            let sb = rs.values.compactMap { $0["branch"] as? String }.first { !$0.isEmpty }
            for u in open where u["story"] as? String == k {
                guard let repo = apRepoFor(u["cwd"] as? String ?? "", repos) ?? (rs.count == 1 ? rs.keys.first : nil) else { continue }
                if let sb { want.append((repo, sb)) }
                if let t = u["branch"] as? String { want.append((repo, t)) }
            }
        }
        var have: [String: Set<String>] = [:], found: [(repo: String, branch: String)] = []
        for w in want where FileManager.default.fileExists(atPath: w.repo) {
            if have[w.repo] == nil {
                have[w.repo] = Set((apGit(["for-each-ref", "--format=%(refname)", "refs/heads/"], w.repo) ?? "").split(separator: "\n").map { String($0.dropFirst(11)) })
            }
            if have[w.repo]?.contains(w.branch) == true, !found.contains(where: { $0 == w }) { found.append(w) }
        }
        // 저장소가 여럿이면 가지 앞에 저장소 이름을 붙인다
        let many = Set(found.map(\.repo)).count > 1
        return found.map { many ? "\(($0.repo as NSString).lastPathComponent) \($0.branch)" : $0.branch }
    }

    // 남은 가지를 치우러 합치기·승격으로 — 출발 준비는 닫아도 남는다(합치기·승격 시트의 "출발 준비로 돌아가기"로 돌아온다)
    func toMerge() {
        guard let rd = pastRun?.runDir else { return }
        APMerge.shared.open(rd, fromStart: true)
    }

    // 추정 보정(2026-10-22 사용자 결정, 설계 5장 7절): 같은 모델의 통과 단위만, 같은 프로젝트·같은 종류 표본이 3개 이상이면 그것, 아니면 모든 프로젝트의
    // 같은 종류, 그래도 3개 미만이면 원래 추정과 "표본 부족". 계수는 실제 단위 시계 ÷ 예상의 가운데값. %p·비용은 보정한 시간에 그 모델의 분당 값을 곱한다
    private func calibrate() -> [String: APEstimate] {
        let all = APRecords.samples(db).filter { $0.model == model }
        let mins = all.reduce(0) { $0 + $1.min }
        let ppPer = mins > 0 ? all.reduce(0) { $0 + $1.pp } / mins : nil
        let usdPer = mins > 0 ? all.reduce(0) { $0 + $1.usd } / mins : nil
        var out: [String: APEstimate] = [:]
        for u in units {
            var e = APEstimate(min: u.est)
            let same = all.filter { $0.project == project && $0.kind == u.kind }
            let wide = all.filter { $0.kind == u.kind }
            let pick = same.count >= 3 ? same : wide
            e.samples = pick.count
            e.wide = same.count < 3 && wide.count >= 3
            if let est = u.est, pick.count >= 3 {
                let r = pick.map { $0.min / $0.est }.sorted()
                let med = r.count % 2 == 1 ? r[r.count / 2] : (r[r.count / 2 - 1] + r[r.count / 2]) / 2
                e.min = est * med
            }
            e.pp = e.min.flatMap { m in ppPer.map { m * $0 } }
            e.usd = e.min.flatMap { m in usdPer.map { m * $0 } }
            out[u.id] = e
        }
        return out
    }

    // 2단계 "보드가 준비" — 모으기 → 가리기 → 지난 세트 → 엔진 출발 전 확인(status --json) → 따로 작업 트리 확인 → 판정 미리 돌리기(prerun)
    func prepare() {
        guard !preparing, let run = pastRun else { return }
        step = 2
        preparing = true; prepared = false
        problems = []; prerunPassed = []
        let cand = units.filter { $0.excluded == nil }
        let gone = units.filter { $0.excluded != nil }
        lines = [
            APPrepLine(id: "collect", title: "모으기", state: "done", sec: 0,
                       note: "\(apDay(run.started)) 실행 · 남은 단위 \(units.count) · 미룬 사람 몫 \(carryPark.count) · 새로 찾은 일 \(carryNew.count)"),
            APPrepLine(id: "hide", title: "이미 끝난 것·진행 중인 것 가리기", state: activeHere == nil ? "done" : "fail", sec: 0,
                       note: activeHere.map { "이 경로에서 자율주행이 돌고 있습니다(\($0)). 그 실행이 끝난 뒤에 출발합니다" }
                        ?? (gone.isEmpty ? "뺀 것 없음" : "\(gone.count)개 뺌: 이미 끝남 \(gone.count). 되살리기는 다음 단계(단위 제안)에서 합니다"),
                       detail: gone.map { "\($0.title): \($0.excluded ?? "")" }),
            APPrepLine(id: "set", title: "지난 자율주행 세트", state: "done", sec: 0,
                       note: setText.map { "지시 \($0.count)자를 불러왔습니다(\(apDay(setAt))). 출발 때 엔진이 고정해 자율주행 세션에만 얹습니다" }
                        ?? "이 프로젝트의 세트에 지시(autopilot.md)가 없어 공통 규칙만으로 돕니다"),
            APPrepLine(id: "check", title: "단위마다 기계 검사 · 엔진 출발 전 확인"),
            APPrepLine(id: "where", title: "따로 작업 트리 확인"),
            APPrepLine(id: "prerun", title: "판정 미리 돌리기"),
        ]
        if cand.isEmpty {
            setLine("check", "skip", note: "이어 갈 단위가 없습니다")
            setLine("where", "skip", note: "")
            setLine("prerun", "skip", note: "")
            preparing = false; prepared = true
            buildCards()
            return
        }
        setLine("check", "run")
        let rootNow = root, settingsNow = settings, projectNow = project
        queue.async {
            let info = APStart.readRepos(cand, root: rootNow)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.repos = info.repos; self.repoInfo = info.info
                    self.pickDefaultBases(projectNow, settingsNow)
                    self.writeDraft(cand.filter { $0.blocked == nil })
                    self.runStatus(prerun: true, ids: Set(cand.filter { $0.blocked == nil }.map(\.id)))
                }
            }
        }
    }

    // 단위 작업 폴더가 든 저장소(엔진 gitRepos 와 같이 git 뿌리)와 그 지금 가지·가지 목록·머리·커밋 안 한 변경, dev 에만 있는 커밋 수
    nonisolated static func readRepos(_ units: [APDraftUnit], root: String) -> (repos: [String], info: [String: APRepoInfo]) {
        var repos: [String] = []
        for u in units {
            guard let d = (try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any], let cwd = d["cwd"] as? String,
                  FileManager.default.fileExists(atPath: cwd), let top = apGit(["rev-parse", "--show-toplevel"], cwd), !top.isEmpty else { continue }
            if !repos.contains(top) { repos.append(top) }
        }
        var info: [String: APRepoInfo] = [:]
        for r in repos {
            var i = APRepoInfo()
            i.current = apGit(["symbolic-ref", "-q", "--short", "HEAD"], r).flatMap { $0.isEmpty ? nil : $0 }
            for line in (apGit(["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/heads/"], r) ?? "").split(separator: "\n") {
                let f = line.split(separator: "\t")
                if f.count == 2, f[0].hasPrefix("refs/heads/") { let b = String(f[0].dropFirst(11)); i.branches.append(b); i.heads[b] = String(f[1]) }
            }
            i.dirty = (apGit(["status", "--porcelain"], r) ?? "").split(separator: "\n").map(String.init)
            let names = APStart.roles(i.branches)
            if let dev = names.dev, let live = names.live { i.devAhead = Int(apGit(["rev-list", "--count", "\(live)..\(dev)"], r) ?? "") }
            info[r] = i
        }
        return (repos, info)
    }

    // 가지 자리 — dev(develop), live(staging), master(main). 승격 차례를 저장소 가지로 채울 때 같은 자리로 짝짓는다(설계 2장 끝)
    nonisolated static func roles(_ b: [String]) -> (dev: String?, live: String?, master: String?) {
        (["dev", "develop"].first { b.contains($0) }, ["live", "staging"].first { b.contains($0) }, ["master", "main"].first { b.contains($0) })
    }

    // 출발 가지 기본값(설계 판단): 프로젝트 설정 last_base → 지난 실행에서 고른 출발 가지 → 처음이면 승격 차례에서 master 바로 앞 가지.
    // 승격 차례는 프로젝트 설정 promotion, 없으면 저장소 가지로 채운다(출발할 때 프로젝트 설정에 남긴다)
    private func pickDefaultBases(_ proj: String?, _ s: [String: Any]) {
        let ps = proj.flatMap { apRead(autopilotRoot + "/projects/\($0).json") as? [String: Any] } ?? [:]
        let last = ps["last_base"] as? [String: Any] ?? [:]
        let pr = pastRun.flatMap { apRead($0.runDir + "/run.json") as? [String: Any] } ?? [:]
        let pastBases = ((pr["branches"] as? [String: Any])?["repos"] as? [String: Any] ?? [:]).compactMapValues { (($0 as? [String: Any])?["base"] as? [String: Any])?["branch"] as? String }
        var notes: [String] = []
        for r in repos {
            let i = repoInfo[r] ?? APRepoInfo()
            let n = APStart.roles(i.branches)
            if let b = last[r] as? String, i.branches.contains(b) { bases[r] = b; notes.append("지난 실행에서 고른 가지(\(b))") }
            else if let b = pastBases[r], i.branches.contains(b) { bases[r] = b; notes.append("지난 실행에서 고른 가지(\(b))") }
            else if let b = n.live ?? n.dev ?? n.master ?? i.current { bases[r] = b; notes.append("처음이라 master 바로 앞 가지") }
        }
        baseNote = notes.first.map { "기본: " + $0 } ?? ""
        if let p = ps["promotion"] as? [[String: Any]], !p.isEmpty {
            promotion = p.compactMap { s in (s["from"] as? String).flatMap { f in (s["into"] as? String).map { APStep(from: f, into: $0, after: s["after"] as? String) } } }
            promotionSaved = true
        } else {
            let n = APStart.roles(repos.first.flatMap { repoInfo[$0]?.branches } ?? [])
            var st: [APStep] = []
            if let d = n.dev { st.append(APStep(from: "autopilot", into: d)) }
            if let l = n.live { st.append(APStep(from: "autopilot", into: l, after: n.dev)) }
            if let m = n.master { st.append(n.live.map { APStep(from: $0, into: m, after: $0) } ?? APStep(from: "autopilot", into: m, after: n.dev)) }
            promotion = st
            promotionSaved = false
        }
    }

    // 출발 전 실행 폴더를 쓴다 — 처음에는 drafts/<실행 id>/(runs/ 를 1초마다 읽는 보드 읽기와 섞이지 않게), 출발할 때 runs/ 로 옮긴다
    private func writeDraft(_ list: [APDraftUnit], extra: [String: Any] = [:]) {
        if id.isEmpty || needNewId {
            let stamp = formatter("yyyyMMdd-HHmm").string(from: Date())
            let base = "\(stamp)-" + String((project ?? "run").map { $0.isLetter || $0.isNumber || "._-".contains($0) ? $0 : "-" }).prefix(40)
            var cand = base, k = 2
            while FileManager.default.fileExists(atPath: apDraftsRoot + "/" + cand) || FileManager.default.fileExists(atPath: autopilotRoot + "/runs/" + cand) {
                cand = "\(base)-\(k)"; k += 1
            }
            id = cand
            dir = apDraftsRoot + "/" + cand
            needNewId = false
        }
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        var run = settings
        for (k, v) in extra { run[k] = v }
        run["source"] = "continue"
        run["continues"] = pastId ?? ""
        if jira, !epic.isEmpty { run["epic"] = epic }
        let chosenBases = repos.compactMap { r in bases[r].map { (r, $0) } }
        if Set(chosenBases.map(\.1)).count == 1, chosenBases.count == repos.count, let b = chosenBases.first?.1 { run["base_branch"] = b }
        else if !chosenBases.isEmpty { run["base_branch"] = Dictionary(chosenBases, uniquingKeysWith: { a, _ in a }) }
        apWriteJSON(dir + "/run.json", run)
        apWriteJSON(dir + "/queue.json", queueJSON(list))
    }

    // queue.json — 고른 차례대로. 대기열에 없는 단위를 가리키는 depends 는 뺀다(엔진은 앞선 단위 id 만 받는다. 어긋남은 단위 제안에서 경고만 했다).
    // Jira 단위는 지금 스토리 키와 작업 키, 작업 가지(branch)는 엔진이 프로젝트 설정의 꼴로 다시 채운다
    private func queueJSON(_ list: [APDraftUnit]) -> [[String: Any]] {
        var ids = Set<String>(), out: [[String: Any]] = []
        for u in list {
            guard var d = (try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any] else { continue }
            let deps = u.depends.filter { ids.contains($0) }
            if deps.isEmpty { d.removeValue(forKey: "depends") } else { d["depends"] = deps }
            if let s = u.story { d["story"] = stories.first { $0.id == s }?.key ?? s; d["tickets"] = u.task.map { [$0] } ?? [] }
            ids.insert(u.id)
            out.append(d)
        }
        return out
    }

    private func setLine(_ id: String, _ state: String, note: String? = nil, detail: [String]? = nil) {
        guard let i = lines.firstIndex(where: { $0.id == id }) else { return }
        let now = Date().timeIntervalSince1970
        if state == "run" { lines[i].started = now } else if let s = lines[i].started { lines[i].sec = now - s }
        lines[i].state = state
        if let note { lines[i].note = note }
        if let detail { lines[i].detail = detail }
    }

    // 엔진 출발 전 확인(status --json)과 판정 미리 돌리기(prerun). 준비(2단계)와 출발 확인의 다시 확인이 함께 쓴다
    private func runStatus(prerun: Bool, ids: Set<String>) {
        let d = dir, dev = apDevBuild
        let byId = Dictionary(units.map { ($0.id, $0.title) }, uniquingKeysWith: { a, _ in a })
        queue.async {
            let st = apEngine(["status", d, "--json"] + (dev ? ["--dev"] : []))
            let j = (try? JSONSerialization.jsonObject(with: Data(st.out.utf8))) as? [String: Any]
            let problems = j.map { apStrings($0["problems"]) } ?? ["엔진 출발 전 확인을 읽지 못했습니다: \(apShort(st.out))"]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { return }   // 그사이 버렸거나 다른 실행을 골랐다
                    self.problems = problems
                    self.mode = j?["worktree_mode"] as? String ?? ""
                    self.modeReason = j?["worktree_reason"] as? String ?? ""
                    var sb: [String: String] = [:]
                    for u in j?["units"] as? [[String: Any]] ?? [] { if let i = u["id"] as? String, let b = u["story_branch"] as? String { sb[i] = b } }
                    self.storyBranch = sb
                    let n = ids.count
                    self.setLine("check", problems.isEmpty ? "done" : "fail",
                                 note: problems.isEmpty ? "단위 \(n)개 모두 통과(판정 명령·작업 폴더·상한·가지 꼴 · 다른 가지 지시 없음)" : "엔진이 고칠 것 \(problems.count)개를 찾았습니다",
                                 detail: problems + self.units.compactMap { u in u.blocked.map { "\(u.title): \($0)" } })
                    let hasPre = self.units.contains { u in ids.contains(u.id) && ((try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any])?["precheck"] as? String != nil }
                    if self.mode == "worktree" {
                        self.setLine("where", "done", note: hasPre ? "①~③ 통과. ④ 시험 환경 확인은 출발 때 엔진이 작업 트리에서 봅니다(실패하면 원래 작업 트리로)"
                                                             : "넷 다 통과(시험 환경 확인이 있는 단위는 없습니다). ~/.claude-board/worktrees/\(self.id)/ 에서 돕니다")
                    } else if self.mode == "in_place" {
                        let forced = self.modeReason.contains("설정으로")
                        self.setLine("where", forced ? "done" : "warn", note: self.modeReason + (forced ? "" : ". 원래 작업 트리에서 돌고 보드에 알립니다"))
                    } else {
                        self.setLine("where", "skip", note: "엔진 출발 전 확인을 마치지 못해 정하지 못했습니다")
                    }
                    if prerun, problems.isEmpty {
                        self.setLine("prerun", "run", note: "")
                        self.runPrerun(d, ids: ids, byId: byId)
                        return
                    }
                    if prerun { self.setLine("prerun", "skip", note: "출발 전 확인에 고칠 것이 있어 돌리지 않았습니다") }
                    self.finishCheck()
                }
            }
        }
    }

    // 판정 미리 돌리기 — 단위마다 판정 시간 상한까지 걸릴 수 있어 확인 줄과 따로 알린다
    private func runPrerun(_ d: String, ids: Set<String>, byId: [String: String]) {
        let dev = apDevBuild
        queue.async {
            let pre = apEngine(["prerun", d] + (dev ? ["--dev"] : []))
            let pj = apRead(d + "/prerun.json") as? [String: Any]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { return }
                    if pre.code != 0 || pj == nil {
                        self.setLine("prerun", "fail", note: "판정 미리 돌리기가 실패했습니다", detail: [apShort(pre.out, 400)])
                    } else if let pj {
                        let rows = pj["units"] as? [[String: Any]] ?? []
                        let passed = Set(rows.filter { $0["passed"] as? Bool == true }.compactMap { $0["id"] as? String })
                        let ran = rows.filter { $0["ran"] as? Bool == true }.count
                        let skipped = rows.filter { $0["skipped"] != nil }.count
                        self.prerunPassed = passed.union(self.prerunPassed.subtracting(ids))   // 이번에 돌리지 않은 단위의 앞 결과는 둔다
                        self.prerunIds.formUnion(ids)
                        var note = "단위 \(rows.count)개 가운데 실패 \(ran - passed.count)(예상대로)"
                        if !passed.isEmpty { note += " · 일하기 전부터 통과 \(passed.count): " + passed.compactMap { byId[$0] }.joined(separator: ", ") + "(출발 카드로)" }
                        if skipped > 0 { note += " · 미리 돌리지 않음 \(skipped)" }
                        self.prerunNote = note
                        self.setLine("prerun", "done", note: note, detail: rows.compactMap { r in (r["skipped"] as? String).map { "\(r["title"] ?? ""): \($0)" } })
                    }
                    self.finishCheck()
                }
            }
        }
    }

    private func finishCheck() {
        let wasPreparing = preparing
        preparing = false
        rechecking = false
        prepared = true
        buildCards()
        if wasPreparing, !shown { TerminalHub.shared.toast("자율주행 출발 준비가 끝났습니다. 보드 위 '출발 준비 끝'을 눌러 단위 제안으로 갑니다") }
    }

    // 출발 카드를 만든다 — 이미 고른 답은 그대로 둔다(같은 카드 id). 정책 답은 프로젝트 설정에서 미리 고른다
    private func buildCards() {
        var out: [APCard] = []
        for u in units where u.excluded == nil && u.blocked == nil && prerunPassed.contains(u.id) {
            out.append(APCard(id: "prerun-\(u.id)", kind: "prerun", label: "판정 미리 돌리기", title: "판정이 일하기 전부터 통과합니다: \(u.title)",
                              why: "일을 하지 않아도 통과하므로, 이미 끝난 일이거나 판정이 약합니다.",
                              options: [APOption(key: "drop", label: "이번 실행에서 빼기"), APOption(key: "fix_check", label: "판정을 고치고 다음 출발로"),
                                        APOption(key: "keep", label: "그대로 두기", note: "단위에 넣습니다. 일하지 않고 통과로 남을 수 있습니다")],
                              recommended: "drop", unit: u.id))
        }
        for c in carryPark {
            out.append(APCard(id: c.id, kind: "next_start", label: "지난 실행에서 미룬 사람 몫", title: c.title,
                              why: (c.why.isEmpty ? "" : c.why + " ") + "(지난 실행의 단위 \(c.unit) 이 정하지 못해 보류했습니다)",
                              options: [APOption(key: "park", label: "다시 미룸", note: "이번 실행에서도 그 부분은 보류하고 결과 기록에 남깁니다"),
                                        APOption(key: "decide", label: "근거 문서로 정해도 됨", note: "사람 몫이 아니라고 보고 모델이 근거 문서와 대조해 정합니다"),
                                        APOption(key: "close", label: "닫음", note: "더 묻지 않습니다. 이번 실행은 그 부분을 하지 않습니다")],
                              recommended: "park"))
        }
        for u in units where u.excluded == nil && u.blocked == nil {
            guard let m = estimate(u).min, m >= u.capMin * 2 / 3 else { continue }
            out.append(APCard(id: "size-\(u.id)", kind: "size", label: "단위 크기", title: "\(u.title) 단위가 단위 상한에 가깝습니다",
                              why: "예상 \(Int(m.rounded()))분으로 상한 \(Int(u.capMin))분의 \(Int((m / u.capMin * 100).rounded()))%입니다. 넘으면 '상한에서 멈춤'으로 남고 다음 출발 카드로 넘어갑니다.",
                              options: [APOption(key: "keep", label: "그대로"), APOption(key: "drop", label: "이번엔 빼기", note: "대기열에 남아 다음 출발에서 다시 보입니다")],
                              recommended: "keep", unit: u.id))
        }
        let ps = project.flatMap { apRead(autopilotRoot + "/projects/\($0).json") as? [String: Any] } ?? [:]
        let pol = ps["policy_answers"] as? [String: Any] ?? [:]
        for c in out where c.policy && answers[c.id] == nil {
            if let a = pol[c.id] as? String, c.options.contains(where: { $0.key == a }) { answers[c.id] = a; policyLoaded.insert(c.id) }
        }
        cards = out
        answers = answers.filter { k, _ in out.contains { $0.id == k } }
    }

    // 카드 답 — 답이 고른 단위를 바꾸면 접힌 "고른 단위" 머리를 잠깐 강조한다(와이어프레임 출발 확인 ⑦)
    func answer(_ card: APCard, _ key: String) {
        let before = chosen.map(\.id)
        answers[card.id] = key
        if chosen.map(\.id) != before, let i = cards.firstIndex(of: card) { changedBy = "카드 \(i + 1) 답으로 바뀜"; flash += 1 }
    }
    // 모두 추천대로 — 아직 안 고른 카드만 추천안으로 채운다(이미 고른 답은 그대로, 사용자 결정)
    func answerAllRecommended() {
        let before = chosen.map(\.id)
        for c in cards where answers[c.id] == nil { answers[c.id] = c.recommended }
        if chosen.map(\.id) != before { changedBy = "추천대로 고른 답으로 바뀜"; flash += 1 }
    }

    // 단위 제안 — 앞뒤 순서와 우선순위(지난 실행 차례)로 줄 세우고 멈출 선 안에서 앞에서부터 묶는다. 사용률을 모르면 모두 넣는다
    func propose() {
        var used = 0.0
        for i in units.indices {
            guard units[i].excluded == nil, units[i].blocked == nil else { units[i].checked = false; continue }
            let pp = estimate(units[i]).pp ?? 0
            if let b = budget { units[i].checked = used + pp <= b + 0.0001; if units[i].checked { used += pp } } else { units[i].checked = true }
        }
    }
    // 단위 제안으로 — 처음 한 번만 멈출 선 안에서 묶어 제안하고, 그 뒤로는 사람이 고친 체크를 그대로 둔다
    func toProposal() {
        if !proposedOnce { propose(); proposedOnce = true }
        step = 3
    }
    // − 하나 빼기 · ＋ 하나 더 — 앞에서부터 k개
    func setCount(_ k: Int) {
        var left = max(0, k)
        for i in units.indices where units[i].excluded == nil && units[i].blocked == nil {
            units[i].checked = left > 0
            if left > 0 { left -= 1 }
        }
    }
    func toggle(_ id: String) { if let i = units.firstIndex(where: { $0.id == id }) { units[i].checked.toggle() } }
    func revive(_ id: String) { if let i = units.firstIndex(where: { $0.id == id }) { units[i].excluded = nil; units[i].checked = true; buildCards() } }

    // 앞뒤 순서 경고(경고만, 막지 않음) — 고른 단위가 기대는 앞 단위가 이번 대기열에 없을 때
    var orderWarnings: [String] {
        let picked = choosable.filter(\.checked)
        let ids = Set(picked.map(\.id))
        var out: [String] = []
        for u in picked {
            for d in u.depends where !ids.contains(d) {
                guard let p = units.first(where: { $0.id == d }) else { continue }   // 지난 실행에서 이미 통과한 앞 단위면 괜찮다
                out.append("\(u.title)(\(u.n))은 \(p.title)(\(p.n))이 있어야 합니다. 알리기만 하고 막지 않습니다.")
            }
        }
        return out
    }

    // Jira: 단위와 함께 옮길 단위 — 기대는 앞뒤 단위를 따라간다(2026-10-27 사용자 결정: 기대는 단위는 함께 같은 스토리로)
    func moveGroup(_ id: String) -> [APDraftUnit] {
        var set: Set<String> = [id], changed = true
        while changed {
            changed = false
            for u in units where u.excluded == nil {
                if set.contains(u.id) {
                    for d in u.depends where !set.contains(d) && units.contains(where: { $0.id == d && $0.excluded == nil }) { set.insert(d); changed = true }
                } else if u.depends.contains(where: { set.contains($0) }) {
                    set.insert(u.id); changed = true
                }
            }
        }
        return units.filter { set.contains($0.id) }
    }
    // 옮기기 — 같은 종류 스토리로만(2026-10-27 사용자 결정). to 가 nil 이면 새 스토리를 만든다
    func move(_ id: String, to story: String?) {
        guard let u = units.first(where: { $0.id == id }) else { return }
        var target = story
        if target == nil {
            let k = (stories.filter(\.isNew).count) + 1
            let s = APStory(id: "new-\(k)", key: nil, kind: u.kind, title: "")
            stories.append(s)
            target = s.id
        }
        guard let t = target, stories.first(where: { $0.id == t })?.kind == u.kind else { return }
        let group = Set(moveGroup(id).map(\.id))
        for i in units.indices where group.contains(units[i].id) { units[i].story = t }
        stories.removeAll { s in s.isNew && !units.contains { $0.story == s.id } }
    }
    func storyLabel(_ s: APStory) -> String {
        s.key ?? "새 스토리 \(s.id.dropFirst(4))"
    }
    // 만들 티켓 — 새 스토리, 작업 키가 없는 단위, 다른 스토리로 옮긴 이미 있는 작업(부모를 바꿔야 함). 티켓 만들기는 4단계 연결과 함께
    var ticketsToMake: (stories: Int, tasks: Int, moved: Int) {
        let picked = chosen + heldByCards
        let newStories = Set(picked.compactMap { $0.story }.filter { s in stories.first { $0.id == s }?.isNew ?? false }).count
        return (newStories, picked.filter { $0.story != nil && $0.task == nil }.count,
                picked.filter { $0.task != nil && $0.story != $0.origStory }.count)
    }

    func storyPattern(_ kind: String) -> String {
        let names = tickets["branch_names"] as? [String: Any] ?? [:]
        let sp = names["story"] as? String ?? "features/parent/{story}"
        let type = (tickets["type_by_kind"] as? [String: Any])?[kind] as? String ?? kind
        return sp.replacingOccurrences(of: "{type}", with: type)
    }

    // 출발 확인으로 — 고른 단위로 엔진 출발 전 확인을 다시 본다(빠름). 출발 가지를 바꾸면 판정 미리 돌리기도 다시(설계 2장: 카드가 바뀔 수 있다)
    func toConfirm() {
        step = 4
        recheck(prerun: !Set((chosen + heldByCards).map(\.id)).isSubset(of: prerunIds))
    }
    func setBase(_ repo: String, _ b: String) {
        guard bases[repo] != b else { return }
        bases[repo] = b
        recheck(prerun: true)
    }
    private func recheck(prerun: Bool) {
        guard !preparing, !starting, !dir.isEmpty else { return }
        rechecking = true
        startError = nil
        let list = chosen + heldByCards
        writeDraft(list)
        runStatus(prerun: prerun, ids: Set(list.map(\.id)))
    }

    // 출발을 막는 까닭(모두 풀려야 출발 단추가 풀린다)
    var locks: [String] {
        var l: [String] = []
        if preparing || rechecking { l.append(preparing ? "보드가 아직 준비하는 중입니다" : "고른 단위와 출발 가지로 엔진 확인을 다시 보는 중입니다") }
        if let a = activeHere { l.append("이 경로에서 자율주행이 돌고 있습니다(\(a)). 그 실행이 끝난 뒤에 출발합니다") }
        if chosen.isEmpty, heldByCards.isEmpty { l.append("고른 단위가 없습니다") }
        let left = cards.filter { answers[$0.id] == nil }.count
        if left > 0 { l.append("카드 \(cards.count)장 가운데 \(left)장이 남았습니다. 다 고르면 출발할 수 있습니다") }
        if !problems.isEmpty { l.append("엔진의 출발 전 확인에 고칠 것이 \(problems.count)개 있습니다") }
        if !leftBranches.isEmpty { l.append("지난 실행의 스토리·작업 가지 \(leftBranches.count)개가 남아 있습니다. 합치기·승격에서 먼저 합치거나 버린 뒤 이어 갑니다") }
        if jira {
            if epic.isEmpty { l.append("에픽 번호를 넣어 주세요(출발 준비 1단계)") }
            else if epic.range(of: "^[A-Z][A-Z0-9_]*-[0-9]+$", options: .regularExpression) == nil { l.append("에픽 번호 꼴이 아닙니다(예: SHOP-10)") }
            let t = ticketsToMake
            if t.stories + t.tasks + t.moved > 0 {
                l.append("만들거나 고칠 티켓이 \(t.stories + t.tasks + t.moved)건 있습니다. 티켓 만들기는 Jira 연결(4단계)과 함께 만듭니다. 지금은 이미 있는 티켓의 단위만 출발합니다")
            }
        }
        return l
    }

    // 출발 — 실행 폴더를 마저 쓰고(고른 단위·출발 가지·카드와 답) runs/ 로 옮긴 뒤 엔진 start. 거부되면 그 문장을 그대로 보이고 준비는 남긴다
    func start() {
        guard locks.isEmpty, !starting else { return }
        starting = true
        startError = nil
        let list = chosen
        if (apRead(dir + "/run.json") as? [String: Any])?["state"] != nil { needNewId = true }
        writeDraft(list)
        apWriteJSON(dir + "/cards.json", cards.map(\.json))
        apWriteJSON(dir + "/answers.json", answers)
        let src = dir, dst = autopilotRoot + "/runs/" + id, dev = apDevBuild, runId = id
        queue.async {
            var moved = src == dst
            if !moved {
                try? FileManager.default.createDirectory(atPath: autopilotRoot + "/runs", withIntermediateDirectories: true)
                moved = rename(src, dst) == 0
            }
            let r: (code: Int32, out: String) = moved ? apEngine(["start", dst] + (dev ? ["--dev"] : [])) : (-1, "실행 폴더를 runs/ 로 옮기지 못했습니다: \(String(cString: strerror(errno)))")
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if moved { self.dir = dst }
                    self.starting = false
                    if r.code == 0 {
                        self.remember()
                        self.close()
                        self.reset()
                        AutopilotStore.shared.screen = .running(runId)
                    } else {
                        self.startError = r.out.isEmpty ? "엔진이 출발하지 않았습니다(종료 코드 \(r.code))" : r.out
                        if (apRead(dst + "/run.json") as? [String: Any])?["state"] != nil { self.needNewId = true }
                    }
                }
            }
        }
    }

    // 출발 가지·승격 차례·정책 답을 프로젝트 설정에 남긴다(다른 칸은 그대로). 출발했을 때만
    private func remember() {
        guard let p = project else { return }
        let path = autopilotRoot + "/projects/\(p).json"
        var o = apRead(path) as? [String: Any] ?? [:]
        var lb = o["last_base"] as? [String: Any] ?? [:]
        for (r, b) in bases { lb[r] = b }
        o["last_base"] = lb
        if o["promotion"] == nil, !promotion.isEmpty {
            o["promotion"] = promotion.map { s -> [String: Any] in var d: [String: Any] = ["from": s.from, "into": s.into]; if let a = s.after { d["after"] = a }; return d }
        }
        let pol = cards.filter(\.policy).compactMap { c in answers[c.id].map { (c.id, $0) } }
        if !pol.isEmpty {
            var pa = o["policy_answers"] as? [String: Any] ?? [:]
            for (k, v) in pol { pa[k] = v }
            o["policy_answers"] = pa
        }
        try? FileManager.default.createDirectory(atPath: autopilotRoot + "/projects", withIntermediateDirectories: true)
        if !apWriteJSON(path, o) { boardLog("자율주행: 프로젝트 설정을 쓰지 못했습니다 \(path)") }
    }
}

// 출발 화면의 상자 — 제목과 작은 설명, 안에 줄들
struct APBox<Content: View>: View {
    let title: String
    var note = ""
    var tint: Color? = nil
    var axid = ""
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(title).boardFont(12.5, weight: .bold).fixedSize()
                if !note.isEmpty { Text(note).boardFont(10.5).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                Spacer(minLength: 0)
            }
            content()
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).fill((tint ?? Color.white).opacity(tint == nil ? 0.04 : 0.08)))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke((tint ?? Color.white).opacity(tint == nil ? 0.08 : 0.32), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(axid)
    }
}

func apNote(_ s: String, _ color: Color = .secondary) -> some View {
    Text(s).boardFont(10.5).foregroundStyle(color).fixedSize(horizontal: false, vertical: true)
}

// 고르는 줄(동그라미 또는 네모) — 잠긴 줄은 흐리게, 도움말에 까닭
struct APPickRow<Label: View>: View {
    let on: Bool
    var square = false
    var enabled = true
    var help = ""
    let axid: String
    let action: @MainActor () -> Void
    @ViewBuilder let label: () -> Label
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            HStack(alignment: .top, spacing: 8) {
                Image(systemName: square ? (on ? "checkmark.square.fill" : "square") : (on ? "largecircle.fill.circle" : "circle"))
                    .font(.system(size: 12)).foregroundStyle(on ? Palette.accent : .secondary).padding(.top, 1)
                label()
                Spacer(minLength: 0)
            }
            .padding(8)
            .background(RoundedRectangle(cornerRadius: 9).fill(Color.white.opacity(on ? 0.08 : (hover && enabled ? 0.05 : 0.02))))
            .overlay(RoundedRectangle(cornerRadius: 9).stroke(on ? Palette.accent.opacity(0.5) : Color.white.opacity(0.06), lineWidth: on ? 1 : 0.5))
            .contentShape(Rectangle())
            .opacity(enabled ? 1 : 0.55)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .modifier(ClickableIf(active: enabled, hover: $hover))
        .hint(help)
        .accessibilityIdentifier(axid)
    }
}

// 접히는 줄 — 머리에 요약(접혀 있어도 보임)
struct APFold<Content: View>: View {
    let title: String
    let summary: String
    let axid: String
    @Binding var open: Bool
    var tint: Color? = nil
    var flash = false
    var tag: String? = nil
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button { withAnimation(foldSpring) { open.toggle() } } label: {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: open ? "chevron.down" : "chevron.right").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary).frame(width: 10)
                    Text(title).boardFont(12, weight: .bold).fixedSize()
                    Text(summary).boardFont(11).foregroundStyle(flash ? Palette.accent : .secondary).lineLimit(2)
                    if let tag { APChip(text: tag, color: Palette.accent, strong: true) }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier(axid)
            if open { content().padding(.leading, 18) }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 11).fill(flash ? Palette.accent.opacity(0.16) : (tint ?? Color.white).opacity(tint == nil ? 0.035 : 0.07)))
        .overlay(RoundedRectangle(cornerRadius: 11).stroke((tint ?? Color.white).opacity(tint == nil ? 0.07 : 0.28), lineWidth: 0.5))
    }
}

func apPrepChip(_ state: String) -> APChip {
    switch state {
    case "run": APChip(text: "하는 중", color: APColor.work, strong: true)
    case "done": APChip(text: "끝", color: APColor.ok, strong: true)
    case "warn": APChip(text: "알림", color: APColor.limit, strong: true)
    case "fail": APChip(text: "실패", color: APColor.fail, strong: true)
    case "skip": APChip(text: "하지 않음", color: APColor.muted, strong: true)
    default: APChip(text: "기다림", color: APColor.muted)
    }
}

func apPP(_ v: Double?) -> String { v.map { String(format: "+%.1f%%p", $0) } ?? "%p 모름" }
func apMin(_ v: Double?) -> String { v.map { apDur($0 * 60) } ?? "예상 없음" }

// 출발 준비 시트 — 창 전체를 덮는다(와이어프레임 출발 준비 ㉓). 머리·단계 표시·내용·아래 줄
struct APStartSheet: View {
    @ObservedObject var s: APStart
    var body: some View {
        VStack(spacing: 0) {
            head
            steps
            Divider().opacity(0.35)
            ScrollView(showsIndicators: true) {
                SwiftUI.Group {
                    switch s.step {
                    case 1: APStep1(s: s)
                    case 2: APStep2(s: s)
                    case 3: APStep3(s: s)
                    default: APConfirm(s: s)
                    }
                }
                .padding(16)
                .frame(maxWidth: 1000, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            Divider().opacity(0.35)
            APStartFoot(s: s).padding(.horizontal, 16).padding(.vertical, 10)
        }
        .frame(minWidth: 460, minHeight: 440)
        .background(Backdrop())
        .preferredColorScheme(.dark)
        .boardFont(13)
    }

    private var head: some View {
        HStack(spacing: 8) {
            APMark()
            Text(s.step == 4 ? "출발 확인" : "새 자율주행").boardFont(15, weight: .bold).fixedSize()
            if let p = s.project { APChip(text: p, color: APColor.board) }
            if s.jira { APChip(text: "Jira 연결", color: APColor.jira) }
            Spacer(minLength: 4)
            if s.hasDraft { APButton(title: "준비 버리기", icon: "trash", enabled: !s.starting, help: "출발하지 않은 이 준비와 실행 폴더를 지웁니다", axid: "ap-start-discard") { s.discard() } }
            APButton(title: "닫기", help: "준비는 남아 뒤에서 이어 갑니다. 보드 위 '출발 준비'로 다시 엽니다", axid: "ap-start-close") { s.close() }
        }
        .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 8)
    }

    private var steps: some View {
        let names = ["원천 고르기", "보드가 준비", "단위 제안·출발 카드", "출발 확인"]
        return WrapRow(spacing: 6) {
            ForEach(0..<4, id: \.self) { i in
                let n = i + 1
                let can = n == 1 || (n == 2 && (s.preparing || s.prepared)) || (n >= 3 && s.prepared && !s.preparing)
                Button {
                    guard can, n != s.step, !s.starting else { return }
                    if n == 4 { s.toConfirm() } else if n == 3 { s.toProposal() } else { s.step = n }
                } label: {
                    HStack(spacing: 5) {
                        Text("\(n)").boardFont(10, weight: .bold, design: .monospaced)
                            .frame(width: 16, height: 16)
                            .background(RoundedRectangle(cornerRadius: 4).fill(n == s.step ? Palette.accent : Color.white.opacity(can ? 0.14 : 0.06)))
                        Text(names[i]).boardFont(11, weight: n == s.step ? .bold : .regular).fixedSize()
                    }
                    .foregroundStyle(n == s.step ? .primary : (can ? .secondary : Color.secondary.opacity(0.5)))
                    .padding(.horizontal, 6).padding(.vertical, 3)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("ap-step-\(n)")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16).padding(.bottom, 8)
    }
}

// 아래 줄 — 단계마다 이전·다음, 4단계는 출발과 잠금 까닭(와이어프레임 출발 확인 ⑩: 고정 안내는 누르기 전부터 단추 옆에)
struct APStartFoot: View {
    @ObservedObject var s: APStart
    @ObservedObject private var ap = AutopilotStore.shared   // 같은 경로에서 도는 실행이 끝나면 잠금이 풀리게
    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            if s.step > 1 { APButton(title: "이전", enabled: !s.starting, axid: "ap-prev") { s.step == 4 ? s.toProposal() : (s.step = s.step - 1) } }
            message.frame(maxWidth: .infinity, alignment: .leading)
            next
        }
    }

    @ViewBuilder private var message: some View {
        switch s.step {
        case 1: apNote(s.hasDraft ? "원천은 하나만 고릅니다" : "이어 갈 지난 실행이 아직 없습니다")
        case 2:
            if s.preparing { apNote("준비하는 중입니다. 끝나면 다음으로 갈 수 있습니다. 시트를 닫아도 뒤에서 이어 갑니다") }
            else if !s.problems.isEmpty { apNote("엔진이 출발 전 확인에서 고칠 것을 찾았습니다. 단위 제안은 볼 수 있고 출발은 잠깁니다", APColor.limit) }
            else if s.mode == "in_place", !s.modeReason.contains("설정으로") { apNote("따로 작업 트리 확인이 실패해 원래 작업 트리에서 돕니다", APColor.limit) }
            else { apNote("준비가 끝났습니다") }
        case 3: apNote(s.cards.isEmpty ? "출발 카드가 없습니다" : "출발 카드 \(s.cards.count)장은 출발 확인에서 답합니다")
        default:
            let l = s.locks
            VStack(alignment: .leading, spacing: 2) {
                if s.starting { apNote("출발하는 중입니다. 엔진이 가지·작업 트리를 만들고 LaunchAgent 를 올립니다", APColor.work) }
                else if let first = l.first { apNote(first, APColor.limit) }
                apNote("출발하면 대기열과 자율주행 세트가 고정됩니다. 목표를 바꾸려면 멈추고 다시 출발합니다.")
            }
        }
    }

    @ViewBuilder private var next: some View {
        switch s.step {
        case 1:
            APButton(title: "다음: 보드가 준비", primary: true, enabled: s.hasDraft && !s.units.isEmpty && s.noDefs == nil,
                     help: s.units.isEmpty ? "이어 갈 단위가 없습니다" : "보드가 혼자 준비합니다", axid: "ap-next") {
                if s.prepared || s.preparing { s.step = 2 } else { s.prepare() }
            }
        case 2: APButton(title: "다음: 단위 제안", primary: true, enabled: s.prepared && !s.preparing, axid: "ap-next") { s.toProposal() }
        case 3: APButton(title: "다음: 출발 확인", primary: true, enabled: s.prepared, axid: "ap-next") { s.toConfirm() }
        default:
            APButton(title: s.starting ? "출발하는 중" : "출발", icon: "steeringwheel", primary: true, enabled: s.locks.isEmpty && !s.starting,
                     help: s.locks.first ?? "엔진을 출발시킵니다", axid: "ap-start-go") { s.start() }
        }
    }
}

// 1단계 원천 고르기 — 원천 다섯(이어 가기 말고는 잠금과 까닭), 이어 갈 실행, 작업 경로, 에픽(Jira 연결 프로젝트), 가져올 것
struct APStep1: View {
    @ObservedObject var s: APStart
    @State private var pickRun = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            APBox(title: "어디서 일을 가져올까요?", note: "하나만 고릅니다") {
                source("continue", "지난 실행 이어 가기", continueSummary, lock: nil)
                source("todo", "저장소 할 일 문서", "docs/TODO.md 같은 파일과 그 안의 절을 고릅니다", lock: "가져오기 세션이 항목마다 단위 초안을 쓰는 7단계에서 엽니다")
                source("list", "바깥 목록", "Jira 필터나 Notion 데이터베이스. 연결이 있을 때만 고를 수 있습니다", lock: "바깥 연결(4단계) 뒤 7단계에서 엽니다")
                source("legacy", "옛 자동화 옮기기", "다른 도구로 돌던 자동화의 멈춘 자리 기록과 원장을 가져옵니다", lock: "7단계에서 엽니다")
                source("topic", "주제로 시작", "주제 글과 자료 링크를 주면 초안 세션이 명세와 단위를 씁니다. 새 폴더에서도 됩니다",
                       lock: "입력 판(주제 글·자료 링크·작업 경로·기술 스택)은 다음 조각, 초안 세션은 5단계에서 엽니다")
            }
            if !s.past.isEmpty { pastList }
            if s.hasDraft { pathBox }
            if !s.leftBranches.isEmpty { APLeftBranches(s: s) }
            if s.jira { APEpicBox(s: s) }
            if s.hasDraft { importBox }
        }
    }

    private var continueSummary: String {
        guard let r = s.pastRun else { return "끝난 자율주행 실행이 아직 없습니다" }
        return "\(apDay(r.started)) 실행 · 남은 단위 \(s.units.count) · 미룬 사람 몫 \(s.carryPark.count) · 새로 찾은 일 \(s.carryNew.count)"
    }

    private func source(_ key: String, _ title: String, _ sub: String, lock: String?) -> some View {
        APPickRow(on: key == "continue" && s.hasDraft, enabled: lock == nil && !s.past.isEmpty, help: lock ?? "", axid: "ap-src-\(key)", action: {}) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(title).boardFont(12, weight: .semibold).fixedSize()
                    if lock != nil { APChip(text: "잠금", color: APColor.muted) }
                }
                apNote(sub)
                if let lock { apNote(lock, APColor.limit.opacity(0.85)) }
            }
        }
    }

    private var pastList: some View {
        let cur = s.pastRun.map { "\($0.projectName) · \(apDate($0.started)) · \($0.result)" } ?? "고르지 않음"
        return APFold(title: "이어 갈 실행", summary: cur, axid: "ap-fold-past", open: $pickRun) {
            apNote("끝난 실행 가운데 하나 · 최근 것부터")
            ForEach(s.past.prefix(12)) { r in
                APPickRow(on: r.id == s.pastId, enabled: !s.preparing && !s.starting, axid: "ap-past-\(r.id)", action: { s.choose(r.id) }) {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text("\(r.projectName) · \(apDate(r.started))").boardFont(11.5, weight: .semibold).lineLimit(1)
                            if let st = r.stateLabel { APChip(text: st.0, color: st.1, strong: true) }
                        }
                        apNote(r.result + (r.title.isEmpty ? "" : " · " + r.title))
                    }
                }
            }
        }
    }

    private var pathBox: some View {
        let r = s.pastRun
        return APBox(title: "작업 경로", note: "이어 가기는 지난 실행이 돈 경로에서 잇습니다") {
            HStack(spacing: 6) {
                Text(s.root.replacingOccurrences(of: NSHomeDirectory(), with: "~")).boardFont(12, design: .monospaced).lineLimit(1).truncationMode(.head)
                if let r { APChip(text: r.mode == "worktree" ? "지난번: 따로 작업 트리" : "지난번: 원래 작업 트리", color: APColor.muted) }
            }
            apNote(s.setText != nil ? "이 경로의 지난 자율주행 세트(\(apDay(s.setAt)))를 불러왔습니다. 출발 가지와 어디서 도는지는 보드가 준비한 뒤 출발 확인에서 봅니다."
                                    : "이 프로젝트에는 자율주행 세트의 지시가 없어 공통 규칙만으로 돕니다. 출발 가지와 어디서 도는지는 출발 확인에서 봅니다.")
            if let a = s.activeHere { apNote("이 경로에서 자율주행이 돌고 있습니다(\(a)). 그 실행이 끝난 뒤에 출발합니다", APColor.limit) }
        }
    }

    private var importBox: some View {
        APBox(title: "가져올 것", note: "고른 실행에서") {
            if let w = s.noDefs { apNote(w, APColor.fail) }
            item("남은 단위", s.units.count, s.units.map { "\($0.title)(\(apUnitLabel($0.last)))" })
            item("미룬 사람 몫", s.carryPark.count, s.carryPark.map(\.title))
            item("새로 찾은 일", s.carryNew.count, s.carryNew.map(\.title))
            if !s.carryNew.isEmpty {
                apNote("새로 찾은 일은 아직 판정 명령이 없어 이번 실행에 넣지 못하고 대기열 후보로 남습니다(단위 초안은 가져오기 세션이 쓰는 뒤 조각).")
            }
        }
    }
    private func item(_ name: String, _ n: Int, _ titles: [String]) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("\(name) \(n)").boardFont(11.5, weight: .semibold).frame(minWidth: 90, alignment: .leading).fixedSize()
            Text(titles.isEmpty ? "없음" : titles.prefix(4).joined(separator: ", ") + (titles.count > 4 ? " 외 \(titles.count - 4)" : ""))
                .boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }
}

// 지난 Jira 실행의 남은 가지 안내(2026-10-28 사용자 결정, 2단계 5-3) — 엔진은 그대로 거부하므로 출발 준비에서 미리 알리고 합치기·승격으로 잇는다
struct APLeftBranches: View {
    @ObservedObject var s: APStart
    var body: some View {
        APBox(title: "먼저 합치거나 버릴 가지 \(s.leftBranches.count)", note: "지난 실행이 만든 스토리·작업 가지", tint: APColor.limit, axid: "ap-left-branches") {
            ForEach(s.leftBranches.prefix(8), id: \.self) { b in Text(b).boardFont(11, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle) }
            if s.leftBranches.count > 8 { apNote("… \(s.leftBranches.count - 8)개 더") }
            apNote("이어 가기는 같은 스토리·작업 키를 씁니다. 엔진은 이 실행이 만든 가지만 움직이므로 지난 실행의 가지가 남아 있으면 출발하지 않습니다. "
                   + "합치기·승격에서 그 스토리를 버리거나(dev 에 합치기 전), dev·live 에 합친 뒤 '합친 가지 지우기'와 '합치지 못한 작업 가지도 지우기'로 치운 뒤 이어 갑니다. "
                   + "뒤 실행이 같은 이름으로 다시 만든 가지면 그 실행의 합치기·승격에서 다룹니다.")
            APButton(title: "합치기·승격 →", primary: true, help: "지난 실행의 합치기·승격을 엽니다. 이 준비는 남습니다", axid: "ap-left-merge") { s.toMerge() }
        }
    }
}

// 에픽 칸(Jira 연결 프로젝트만, 와이어프레임 출발 준비 ㉔) — 이어 가기일 때만 지난 실행의 에픽을 채운다. 번호 꼴만 보고(제목 읽기는 4단계) 꼴이 아니면 출발을 잠근다
struct APEpicBox: View {
    @ObservedObject var s: APStart
    var body: some View {
        let ok = s.epic.range(of: "^[A-Z][A-Z0-9_]*-[0-9]+$", options: .regularExpression) != nil
        let past = s.pastRun?.epic ?? ""
        return APBox(title: "에픽", note: "Jira 연결 프로젝트만 · 이번 실행의 스토리를 이 에픽 아래에 만듭니다", tint: ok ? nil : APColor.limit, axid: "ap-epic-box") {
            HStack(spacing: 8) {
                TextField("에픽 번호(예: SHOP-10)", text: $s.epic)
                    .textFieldStyle(.roundedBorder)
                    .boardFont(12, design: .monospaced)
                    .frame(maxWidth: 220)
                    .accessibilityIdentifier("ap-epic")
                if ok { APChip(text: "번호 꼴 맞음", color: APColor.ok, strong: true) }
                else { APChip(text: s.epic.isEmpty ? "비어 있음" : "에픽 번호 꼴이 아님", color: APColor.fail, strong: true) }
            }
            if !ok { apNote("에픽 번호를 넣어 주세요. 그때까지 출발이 잠깁니다.", APColor.limit) }
            apNote(past.isEmpty ? "지난 실행에 에픽 기록이 없어 빈 칸으로 열었습니다. 지난 실행의 에픽은 이어 가기일 때만 채웁니다."
                   : s.epic == past ? "이어 가기라 지난 실행의 에픽을 채워 열었습니다. 다른 번호로 바꿀 수 있습니다."
                   : "지난 실행의 에픽(\(past))에서 바꿨습니다.")
            apNote("에픽의 제목과 상태는 Jira 연결(4단계) 뒤에 보드가 읽어 보입니다. 지금은 번호 꼴만 봅니다.")
        }
    }
}

// 2단계 보드가 준비 — 보드가 혼자 하는 일을 줄마다(이름·상태 꼬리표·걸린 시간·짧은 결과). 하는 중인 줄의 시계만 1초마다(시트 안에서만 돈다)
struct APStep2: View {
    @ObservedObject var s: APStart
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            APBox(title: s.preparing ? "보드가 준비하는 중" : "보드가 준비한 일", note: "사람은 기다리기만 합니다") {
                ForEach(s.lines) { l in line(l) }
            }
            if !s.problems.isEmpty { APProblems(problems: s.problems) }
        }
    }
    private func line(_ l: APPrepLine) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 8) {
                apPrepChip(l.state)
                Text(l.title).boardFont(12, weight: .semibold).lineLimit(1)
                Spacer(minLength: 4)
                if l.state == "run", let t = l.started {
                    APTickText { now in apDur(now.timeIntervalSince1970 - t) }.boardFont(10.5).foregroundStyle(.secondary)
                } else if let sec = l.sec, l.state != "wait" {
                    Text(sec < 1 ? "1초 안" : apDur(sec)).boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                }
            }
            if !l.note.isEmpty { apNote(l.note) }
            ForEach(Array(l.detail.prefix(8).enumerated()), id: \.offset) { _, d in
                Text("· " + d).boardFont(10.5).foregroundStyle(l.state == "fail" ? APColor.fail : .secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 9).fill(Color.white.opacity(0.03)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-prep-\(l.id)")
    }
}

// 엔진 출발 전 확인의 고칠 것 — 엔진 문장 그대로
struct APProblems: View {
    let problems: [String]
    var body: some View {
        APBox(title: "엔진의 출발 전 확인: 고칠 것 \(problems.count)개", note: "엔진 문장 그대로입니다. 이대로는 엔진이 출발하지 않아 출발이 잠깁니다", tint: APColor.fail, axid: "ap-problems") {
            ForEach(Array(problems.enumerated()), id: \.offset) { _, p in
                Text("· " + p).boardFont(11).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

// 3단계 단위 제안·출발 카드 — 줄 세운 단위(Jira 면 스토리별), 사용률 예산, 개수, 앞뒤 순서 경고, 뺀 항목 되살리기, 고를 수 없는 후보, 카드 수
struct APStep3: View {
    @ObservedObject var s: APStart
    @State private var moving: String?
    @State private var showGone = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            APBox(title: "제안한 단위", note: "앞뒤 순서와 우선순위로 줄 세움 · 예상 주간 사용률, 그 아래 시간 · 예상 비용", axid: "ap-proposal") {
                if s.jira {
                    ForEach(s.stories) { st in storyGroup(st) }
                    let loose = s.choosable.filter { $0.story == nil }
                    if !loose.isEmpty {
                        apNote("스토리 없는 단위")
                        ForEach(loose) { u in unitRow(u) }
                    }
                } else {
                    ForEach(s.choosable) { u in unitRow(u) }
                }
                if s.choosable.isEmpty { apNote("고를 수 있는 단위가 없습니다") }
                apNote("예상은 지난 실행으로 보정한 값입니다(같은 모델의 통과 단위. 같은 프로젝트·같은 종류 표본이 3개보다 적으면 모든 프로젝트로 넓힙니다).")
            }
            budgetBox
            countRow
            ForEach(s.orderWarnings, id: \.self) { w in
                Label(w, systemImage: "exclamationmark.triangle.fill").boardFont(11).foregroundStyle(APColor.limit).fixedSize(horizontal: false, vertical: true)
            }
            goneBox
            otherBox
            cardsLine
        }
    }

    private func unitRow(_ u: APDraftUnit) -> some View {
        let e = s.estimate(u)
        let dec = s.cardDecision(u)
        return VStack(alignment: .leading, spacing: 4) {
            APPickRow(on: u.checked, square: true, axid: "ap-unit-\(u.id)", action: { s.toggle(u.id) }) {
                HStack(alignment: .top, spacing: 8) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(u.n). \(u.title)").boardFont(12, weight: .semibold).fixedSize(horizontal: false, vertical: true)
                        WrapRow(spacing: 4) {
                            if !u.kind.isEmpty { APChip(text: apKind(u.kind), color: APColor.board) }
                            APChip(text: "지난번 " + apUnitLabel(u.last), color: APColor.unit(u.last))
                            if let t = u.task { APTicket(key: t) }
                            if !u.depends.isEmpty {
                                APChip(text: "앞: " + u.depends.map { d in s.units.first { $0.id == d }.map { "\($0.n)" } ?? "지난 실행" }.joined(separator: "·"), color: APColor.muted)
                            }
                            APChip(text: e.short ? "표본 부족" : "표본 \(e.samples)" + (e.wide ? " · 모든 프로젝트" : ""), color: e.short ? APColor.limit : APColor.muted)
                            if s.prerunPassed.contains(u.id) { APChip(text: "일하기 전부터 통과", color: APColor.judge, strong: true) }
                            if dec == "pending", u.checked { APChip(text: "출발 카드에서 정합니다", color: APColor.wait) }
                            if dec == "out", u.checked { APChip(text: "카드 답으로 뺌", color: APColor.muted, strong: true) }
                        }
                        if !u.check.isEmpty { Text("판정 " + u.check).boardFont(10, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle) }
                    }
                    Spacer(minLength: 6)
                    VStack(alignment: .trailing, spacing: 1) {
                        Text(apPP(e.pp)).boardFont(12, weight: .bold).monospacedDigit().fixedSize()
                        Text(apMin(e.min)).boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                        Text(e.usd.map { apUSD($0) + " USD" } ?? "비용 모름").boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                    }
                }
            }
            if s.jira, u.story != nil {
                HStack(spacing: 6) {
                    Spacer(minLength: 0)
                    APButton(title: moving == u.id ? "옮기기 닫기" : "옮기기", icon: "arrow.left.arrow.right", axid: "ap-move-\(u.id)") {
                        moving = moving == u.id ? nil : u.id
                    }
                }
                if moving == u.id { moveList(u) }
            }
        }
    }

    private func storyGroup(_ st: APStory) -> some View {
        let list = s.choosable.filter { $0.story == st.id }
        let sum = s.sum(list.filter(\.checked))
        let type = (s.storyPattern(st.kind).components(separatedBy: "/").first ?? st.kind)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if let k = st.key { APTicket(key: k) } else { APChip(text: s.storyLabel(st), color: APColor.jira, strong: true) }
                APChip(text: "\(apKind(st.kind)) · \(type)", color: APColor.board)
                if st.isNew, let i = s.stories.firstIndex(of: st) {
                    TextField("스토리 제목", text: Binding(get: { s.stories[i].title }, set: { s.stories[i].title = $0 }))
                        .textFieldStyle(.roundedBorder).boardFont(11.5).frame(maxWidth: 260)
                        .accessibilityIdentifier("ap-story-title-\(st.id)")
                } else {
                    Text("제목은 Jira 연결(4단계) 뒤에 읽습니다").boardFont(10.5).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                Text("단위 \(list.count) · \(apPP(sum.pp))").boardFont(11).foregroundStyle(.secondary).fixedSize()
            }
            Text("스토리 가지 " + s.storyPattern(st.kind).replacingOccurrences(of: "{story}", with: st.key ?? "‹키›")
                 + (st.isNew ? " · 스토리와 작업 티켓은 출발할 때 만들어 키를 받습니다(4단계)" : ""))
                .boardFont(10, design: .monospaced).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            ForEach(list) { u in unitRow(u) }
            if list.isEmpty { apNote("이 스토리에 남은 단위가 없습니다") }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 10).stroke(APColor.jira.opacity(0.25), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-story-\(st.id)")
    }

    // 옮길 스토리 — 같은 종류만(가지 이름 앞의 종류가 스토리마다 하나여야 스토리 가지가 갈리지 않는다), 기대는 단위는 함께
    private func moveList(_ u: APDraftUnit) -> some View {
        let group = s.moveGroup(u.id).filter { $0.id != u.id }
        let type = s.storyPattern(u.kind).components(separatedBy: "/").first ?? u.kind
        return VStack(alignment: .leading, spacing: 4) {
            apNote("옮길 스토리 · 같은 종류(\(type))만")
            ForEach(s.stories.filter { $0.id != u.story }) { st in
                if st.kind == u.kind {
                    APButton(title: s.storyLabel(st) + (st.title.isEmpty ? "" : " · " + st.title), axid: "ap-move-\(u.id)-\(st.id)") { s.move(u.id, to: st.id); moving = nil }
                } else {
                    apNote("\(s.storyLabel(st)) · \(apKind(st.kind)) 스토리라 옮길 수 없음", APColor.muted)
                }
            }
            APButton(title: "＋ 새 스토리 만들기 · \(type)", axid: "ap-move-\(u.id)-new") { s.move(u.id, to: nil); moving = nil }
            if !group.isEmpty { apNote("함께 옮김: " + group.map { "단위 \($0.n) · \($0.title)" }.joined(separator: ", ") + " (기대는 앞뒤 단위)", APColor.jira) }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 9).fill(APColor.jira.opacity(0.06)))
    }

    private var budgetBox: some View {
        let picked = s.choosable.filter(\.checked)
        let sum = s.sum(picked)
        return APBox(title: "사용률 예산", note: "고른 단위의 예상 합계와 멈출 선까지 남은 주간 사용률", axid: "ap-budget") {
            HStack(spacing: 10) {
                Text("이번 묶음 예상 " + apPP(sum.pp)).boardFont(13, weight: .bold).monospacedDigit()
                if let b = s.budget { Text(String(format: "멈출 선(%.0f%%)까지 %.1f%%p 남음", s.stop7, b)).boardFont(12).foregroundStyle(.secondary) }
            }
            apNote((s.usage7.map { String(format: "주간 %.0f%% 에서 출발", $0) } ?? "주간 사용률을 몰라 멈출 선 없이 모두 넣었습니다")
                   + " · 예상 시간 \(apDur(sum.min * 60))" + (sum.usd.map { " · 약 \(apUSD($0)) USD" } ?? ""))
        }
    }

    private var countRow: some View {
        let n = s.choosable.filter(\.checked).count
        return HStack(spacing: 8) {
            APButton(title: "− 하나 빼기", enabled: n > 0, axid: "ap-less") { s.setCount(n - 1) }
            APButton(title: "＋ 하나 더", enabled: n < s.choosable.count, axid: "ap-more") { s.setCount(n + 1) }
            apNote("고른 \(n)개 · 대기열에 남는 후보 \(s.choosable.count - n)")
        }
    }

    @ViewBuilder private var goneBox: some View {
        let gone = s.units.filter { $0.excluded != nil }
        if !gone.isEmpty {
            APFold(title: "뺀 항목 \(gone.count)", summary: "이미 끝남 \(gone.count) · 펼쳐서 되살립니다", axid: "ap-gone", open: $showGone) {
                ForEach(gone) { u in
                    HStack(spacing: 8) {
                        Text(u.title).boardFont(11.5).lineLimit(1)
                        apNote(u.excluded ?? "")
                        Spacer(minLength: 4)
                        APButton(title: "되살리기", axid: "ap-revive-\(u.id)") { s.revive(u.id) }
                    }
                }
            }
        }
    }

    @ViewBuilder private var otherBox: some View {
        let blocked = s.units.filter { $0.blocked != nil && $0.excluded == nil }
        if !blocked.isEmpty || !s.carryNew.isEmpty {
            APBox(title: "고를 수 없는 후보", note: "대기열에 남아 다음 출발에서 다시 보입니다") {
                ForEach(blocked) { u in apNote("\(u.title): \(u.blocked ?? "")", APColor.limit) }
                ForEach(s.carryNew) { c in apNote("새로 찾은 일 · \(c.title)" + (c.why.isEmpty ? "" : ": \(c.why)") + ". 판정 명령이 없어 가져오기 세션(뒤 조각)이 단위 초안을 써야 합니다") }
            }
        }
    }

    private var cardsLine: some View {
        let kinds = Dictionary(grouping: s.cards, by: \.kind).mapValues(\.count)
        let parts = [("prerun", "판정 미리 통과"), ("next_start", "미룬 사람 몫"), ("size", "단위 크기")].compactMap { k, l in kinds[k].map { "\(l) \($0)" } }
        return HStack(spacing: 8) {
            Text(s.cards.isEmpty ? "출발 카드가 없습니다" : "출발 카드 \(s.cards.count)장이 생겼습니다").boardFont(12, weight: .bold)
            if !parts.isEmpty { apNote(parts.joined(separator: " · ")) }
            Spacer(minLength: 4)
            APButton(title: "출발 확인으로 →", primary: true, axid: "ap-to-confirm") { s.toConfirm() }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 11).fill(Palette.accent.opacity(0.08)))
    }
}

// 4단계 출발 확인 — 늘 보이는 프로젝트와 어디서·출발 가지·승격 차례, 그 바로 아래 출발 카드, 그 아래 접힌 줄(커밋 안 한 변경·고른 단위·설정·자율주행 지시),
// Jira 연결이면 스토리 가지 줄과 만들 티켓 나무(와이어프레임 출발 확인 ①~⑬)
struct APConfirm: View {
    @ObservedObject var s: APStart
    @ObservedObject private var ap = AutopilotStore.shared
    @State private var open: Set<String> = []
    @State private var card = 0
    @State private var hl = false
    private func bind(_ k: String) -> Binding<Bool> { Binding(get: { open.contains(k) }, set: { if $0 { open.insert(k) } else { open.remove(k) } }) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            whereBox
            if !s.problems.isEmpty { APProblems(problems: s.problems) }
            if let e = s.startError { startErrorBox(e) }
            if s.jira { ticketTree }
            cardsBox
            dirtyFold
            unitsFold
            settingsFold
            instructionFold
            if s.locks.count > 1 {
                APBox(title: "출발을 막는 것 \(s.locks.count)", tint: APColor.limit, axid: "ap-locks") {
                    ForEach(s.locks, id: \.self) { apNote("· " + $0) }
                }
            }
        }
        .onChange(of: s.flash) { _, _ in
            hl = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.6) { withAnimation(.easeOut(duration: 0.6)) { hl = false } }
        }
    }

    // 프로젝트와 어디서 — 접지 않고 늘 보인다
    private var whereBox: some View {
        let wt = s.mode == "worktree"
        return APBox(title: "프로젝트와 어디서", note: "늘 보입니다", axid: "ap-where") {
            HStack(spacing: 6) {
                Text(s.project ?? "?").boardFont(12.5, weight: .bold).fixedSize()
                Text(s.root.replacingOccurrences(of: NSHomeDirectory(), with: "~")).boardFont(11, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                if s.repos.count > 1 { APChip(text: "저장소 묶음 \(s.repos.count)", color: APColor.muted) }
                if s.jira, !s.epic.isEmpty { APChip(text: "에픽 \(s.epic)", color: APColor.jira) }
            }
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("어디서").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 52, alignment: .leading)
                VStack(alignment: .leading, spacing: 2) {
                    if s.rechecking { APChip(text: "다시 확인하는 중", color: APColor.work, strong: true) }
                    if wt {
                        HStack(spacing: 6) { APChip(text: "따로 작업 트리", color: APColor.ok, strong: true); apNote("확인 ①~③ 통과") }
                        Text("~/.claude-board/worktrees/\(s.id)/\((s.repos.first.map { ($0 as NSString).lastPathComponent }) ?? "")")
                            .boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                        apNote("원래 작업 트리와 기존 가지는 실행 중에도 마음대로 쓸 수 있습니다. 시험 환경 확인이 작업 트리에서 실패하면 엔진이 원래 작업 트리로 돌립니다.")
                    } else if s.mode == "in_place" {
                        HStack(spacing: 6) { APChip(text: "원래 작업 트리", color: APColor.wait, strong: true) }
                        apNote("이유: " + s.modeReason, APColor.limit)
                        apNote("실행 중 보드에 표시하고, 이 경로에서 새 세션을 열면 경고합니다. 출발 때 원래 작업 트리를 새 가지로 바꾸고, 끝나면 깨끗할 때만 출발 때 가지로 되돌립니다.")
                    } else {
                        apNote("엔진 확인을 마치지 못해 아직 모릅니다", APColor.limit)
                    }
                }
            }
            ForEach(Array(s.repos.enumerated()), id: \.element) { i, r in baseRow(i, r) }
            if !s.baseNote.isEmpty { apNote(s.baseNote + (s.repos.count > 1 ? ". 묶음은 저장소마다 고르고, 처음에는 같은 이름입니다" : "")) }
            newBranchRow
            promotionRow
        }
    }

    private func baseRow(_ i: Int, _ r: String) -> some View {
        let info = s.repoInfo[r] ?? APRepoInfo()
        let roles = APStart.roles(info.branches)
        var cand: [String] = []
        for b in [roles.dev, roles.live, roles.master, info.current, s.bases[r]].compactMap({ $0 }) where info.branches.contains(b) && !cand.contains(b) { cand.append(b) }
        let chosen = s.bases[r] ?? ""
        return VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(s.repos.count > 1 ? (r as NSString).lastPathComponent : "출발 가지").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 52, alignment: .leading)
                ForEach(cand, id: \.self) { b in
                    APButton(title: b, tint: b == chosen ? Palette.accent : nil, enabled: !s.rechecking && !s.starting,
                             help: "이 가지에서 새 가지를 만듭니다", axid: "ap-base-\(i)-\(b)") { s.setBase(r, b) }
                }
                if let c = info.current { apNote("지금 걸린 가지 \(c)") }
            }
            if chosen == roles.dev, let live = roles.live, let n = info.devAhead, n > 0 {
                apNote("\(chosen) 에만 있는 커밋 \(n)개가 \(live) 에 합칠 때 함께 갑니다. 경고만 합니다. \(live) 로 되돌리면 이 경고가 없어집니다.", APColor.limit)
            }
        }
    }

    @ViewBuilder private var newBranchRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("새 가지").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 52, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                if s.jira {
                    let picked = s.chosen + s.heldByCards
                    let st = Set(picked.compactMap(\.story))
                    Text("출발 가지 \(s.bases.values.first ?? "?") → 스토리 가지 \(st.count)개 → 작업 가지 \(picked.count)개 (단위마다 하나)").boardFont(11.5, weight: .semibold)
                    ForEach(s.stories.filter { st.contains($0.id) }) { story in
                        let real = story.isNew ? nil : picked.first { $0.story == story.id }.flatMap { s.storyBranch[$0.id] }   // 새 스토리는 키를 받기 전이라 꼴만
                        Text("\(s.storyLabel(story)) · " + (real ?? s.storyPattern(story.kind).replacingOccurrences(of: "{story}", with: story.key ?? "‹키›")))
                            .boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    }
                    apNote("스토리 가지는 그 스토리의 첫 작업 전에 출발 가지 머리에서, 작업 가지는 단위를 시작할 때 스토리 가지 머리에서 엔진이 만듭니다. autopilot 가지는 만들지 않습니다.")
                } else {
                    let r = s.repos.first ?? ""
                    let b = s.bases[r] ?? "?"
                    let head = s.repoInfo[r]?.heads[b].map { String($0.prefix(7)) } ?? "?"
                    Text("autopilot/\(s.id)").boardFont(11.5, weight: .semibold, design: .monospaced).lineLimit(1).truncationMode(.middle)
                    apNote("(\(b) 머리 \(head) 에서" + (s.repos.count > 1 ? ", 저장소마다" : "") + ") 모델은 이 가지에만 커밋합니다.")
                }
            }
        }
    }

    private var promotionRow: some View {
        let nums = ["①", "②", "③", "④", "⑤"]
        let text = s.promotion.enumerated().map { i, st in
            let from = st.from == "autopilot" ? (s.jira ? "스토리 가지" : "autopilot 가지") : st.from
            return "\(i < nums.count ? nums[i] : "\(i + 1).") \(st.into) ← \(from)" + (st.after.map { " (\($0) 뒤)" } ?? "")
        }.joined(separator: " · ")
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("승격 차례").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 52, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                Text((s.jira ? "스토리마다 " : "") + (text.isEmpty ? "승격할 가지(dev·live·master)가 없습니다" : text)).boardFont(11.5).fixedSize(horizontal: false, vertical: true)
                apNote(s.promotionSaved ? "프로젝트 설정의 차례입니다." : "저장소 가지로 채웠습니다. 출발하면 프로젝트 설정에 남깁니다.")
                APButton(title: "프로젝트 설정 바꾸기", enabled: false, help: "승격 차례를 이 자리에서 바꾸는 것은 다음 조각에서 엽니다") {}
            }
            Spacer(minLength: 0)
        }
    }

    private func startErrorBox(_ e: String) -> some View {
        APBox(title: "엔진이 출발하지 않았습니다", note: "엔진 문장 그대로입니다. 고친 뒤 출발을 다시 누르면 같은 준비로 다시 합니다", tint: APColor.fail, axid: "ap-start-error") {
            Text(e).boardFont(11, design: .monospaced).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        }
    }

    // 출발 카드 — 가로 한 줄로 넘긴다. 위에 지금 카드와 안 고른 수, ‹ ›, 모두 추천대로. 아래에 카드마다 점
    private var cardsBox: some View {
        let left = s.cards.filter { s.answers[$0.id] == nil }.count
        return APBox(title: "출발 카드 \(s.cards.count)장", note: "객관식, 추천안 표시", axid: "ap-cards") {
            if s.cards.isEmpty {
                apNote("이번 출발에는 사람이 정할 카드가 없습니다")
            } else {
                ScrollViewReader { proxy in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 6) {
                            apNote("카드 \(min(card + 1, s.cards.count))/\(s.cards.count) · 안 고른 카드 \(left)")
                            Spacer(minLength: 4)
                            APButton(title: "모두 추천대로", enabled: left > 0, help: "아직 안 고른 카드만 추천안으로 채웁니다", axid: "ap-cards-all") { s.answerAllRecommended() }
                            APButton(title: "‹", enabled: card > 0, axid: "ap-cards-prev") { card -= 1; withAnimation { proxy.scrollTo(card, anchor: .leading) } }
                            APButton(title: "›", enabled: card + 1 < s.cards.count, axid: "ap-cards-next") { card += 1; withAnimation { proxy.scrollTo(card, anchor: .leading) } }
                        }
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(alignment: .top, spacing: 10) {
                                ForEach(Array(s.cards.enumerated()), id: \.element.id) { i, c in cardView(i, c).frame(width: 300).id(i) }
                            }
                        }
                        HStack(spacing: 5) {
                            ForEach(Array(s.cards.enumerated()), id: \.element.id) { i, c in
                                Circle().fill(s.answers[c.id] != nil ? Palette.accent : Color.white.opacity(i == card ? 0.5 : 0.18)).frame(width: 6, height: 6)
                            }
                        }
                    }
                }
            }
        }
    }

    private func cardView(_ i: Int, _ c: APCard) -> some View {
        let a = s.answers[c.id]
        let letters = ["가", "나", "다", "라", "마"]
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 5) {
                Text(c.label).boardFont(10.5, weight: .semibold).foregroundStyle(.secondary).lineLimit(1)
                Spacer(minLength: 2)
                APChip(text: a == nil ? "답 기다림" : "고름", color: a == nil ? APColor.wait : APColor.ok, strong: true)
            }
            Text("\(i + 1). \(c.title)").boardFont(12, weight: .bold).fixedSize(horizontal: false, vertical: true)
            if !c.why.isEmpty { apNote(c.why) }
            ForEach(Array(c.options.enumerated()), id: \.element.key) { j, o in
                APPickRow(on: a == o.key, axid: "ap-card-\(i)-\(o.key)", action: { s.answer(c, o.key) }) {
                    VStack(alignment: .leading, spacing: 1) {
                        HStack(spacing: 5) {
                            Text("\(j < letters.count ? letters[j] : "\(j + 1)") \(o.label)").boardFont(11.5, weight: .semibold).fixedSize(horizontal: false, vertical: true)
                            if o.key == c.recommended { APChip(text: "추천", color: Palette.accent, strong: true) }
                        }
                        if !o.note.isEmpty { apNote(o.note) }
                    }
                }
            }
            if s.policyLoaded.contains(c.id) { apNote("프로젝트 설정에 저장된 답을 불러왔습니다. 바꿀 수 있습니다.") }
        }
        .padding(10)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 11).fill(Color.white.opacity(0.05)))
        .overlay(RoundedRectangle(cornerRadius: 11).stroke(a == nil ? APColor.wait.opacity(0.35) : Color.white.opacity(0.1), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-card-\(i)")
    }

    // 커밋 안 한 변경 — 두 모드 모두 경고만(와이어프레임 출발 확인 ⑥)
    private var dirtyFold: some View {
        let files = s.repos.flatMap { r in (s.repoInfo[r]?.dirty ?? []).map { s.repos.count > 1 ? "\((r as NSString).lastPathComponent): \($0)" : $0 } }
        let wt = s.mode == "worktree"
        let sum = files.isEmpty ? "없음" : "\(files.count)개 · " + (wt ? "자율주행에 들어가지 않음" : "자율주행과 같은 자리") + " · 경고만"
        return APFold(title: "커밋 안 한 변경", summary: sum, axid: "ap-fold-dirty", open: bind("dirty"), tint: files.isEmpty ? nil : APColor.limit) {
            if files.isEmpty { apNote("원래 작업 트리에 커밋하지 않은 변경이 없습니다.") }
            else {
                apNote(wt ? "자율주행에 들어가지 않습니다. 따로 작업 트리는 출발 가지(\(s.bases.values.first ?? "?")) 머리에서 시작하고, 하네스도 커밋된 판을 읽습니다."
                          : "자율주행과 같은 자리에 있습니다. 가지가 바뀌어도 이 변경은 작업 트리에 남고, 모델이 함께 커밋할 수 있습니다. 경고만 합니다.")
                ForEach(files.prefix(20), id: \.self) { Text($0).boardFont(10.5, design: .monospaced).foregroundStyle(.secondary) }
            }
        }
    }

    // 고른 단위 — 머리에 개수·%p·시간·비용, 카드 답이 바꾸면 잠깐 강조(와이어프레임 출발 확인 ⑦)
    private var unitsFold: some View {
        let picked = s.chosen
        let total = s.choosable.count
        let sum = s.sum(picked)
        let head = "\(total)개 중 \(picked.count)개 · 예상 \(apPP(sum.pp)) · \(apDur(sum.min * 60))" + (sum.usd.map { " · 약 \(apUSD($0)) USD" } ?? "")
        return APFold(title: "고른 단위", summary: head, axid: "ap-fold-units", open: bind("units"), flash: hl, tag: hl ? s.changedBy : nil) {
            ForEach(picked) { u in
                HStack(spacing: 6) {
                    if let t = u.task { APTicket(key: t) }
                    Text("\(u.n). \(u.title)").boardFont(11.5).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(apPP(s.estimate(u).pp) + " · " + apMin(s.estimate(u).min)).boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                }
            }
            let held = s.heldByCards
            if !held.isEmpty { apNote("출발 카드에서 정합니다: " + held.map(\.title).joined(separator: ", "), APColor.wait) }
            let rest = s.choosable.filter { u in !picked.contains(u) && !held.contains(u) }
            if !rest.isEmpty { apNote("대기열에 남는 후보 \(rest.count): " + rest.map(\.title).joined(separator: ", ")) }
            HStack(spacing: 6) {
                apNote("예상은 지난 실행으로 보정한 값입니다.")
                APButton(title: "단위 고치기", axid: "ap-edit-units") { s.toProposal() }
            }
        }
    }

    // 설정 — 프로젝트(이어 가기는 지난 실행)에 저장된 값. 이 자리에서 바꾸기는 다음 조각, 게시 방식·보고서·알림은 4단계
    private var settingsFold: some View {
        let t = s.ticketsToMake
        let head = "모델 \(s.model) · 멈출 선 주간 \(Int(s.stop7))% · 5시간 \(Int(s.stop5))%" + (s.jira ? " · 만들 티켓 \(t.stories + t.tasks + t.moved)" : "")
        return APFold(title: "설정", summary: head, axid: "ap-fold-settings", open: bind("settings")) {
            apNote("지난 실행의 실행 설정을 그대로 잇습니다.")
            row("모델", s.model)
            row("멈출 선", "주간 \(Int(s.stop7))% · 5시간 \(Int(s.stop5))%")
            row("게시 방식", "바깥 연결(4단계)과 함께 엽니다. 지금은 바깥에 올리는 글이 없습니다")
            row("보고서·알림", "보고서 틀(3단계)·연결(4단계)과 함께 엽니다")
            if s.jira { row("만들 티켓", "\(t.stories + t.tasks + t.moved)건 · 출발 확인 위 '만들 티켓' 나무") }
        }
    }
    private func row(_ k: String, _ v: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(k).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            Text(v).boardFont(11).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 4)
            APButton(title: "바꾸기", enabled: false, help: "이 자리에서 바꾸는 것은 다음 조각에서 엽니다") {}
        }
    }

    // 자율주행 지시 — 펼치면 그 자리에서 지시 글(읽기만), 나머지 칸은 접혀 있다. 고치기·지난 판 차이·하네스 확인 지점은 뒤 조각(7단계)
    private var instructionFold: some View {
        let head = s.setText.map { "지시 \($0.count)자 · \(apDay(s.setAt)) 판" } ?? "지시 없음 · 공통 규칙만으로 돕니다"
        return APFold(title: "자율주행 지시", summary: head, axid: "ap-fold-set", open: bind("set")) {
            if let t = s.setText {
                HStack(spacing: 6) {
                    apNote("지시 글 · 펼쳐 둡니다")
                    Spacer(minLength: 4)
                    APButton(title: "이 자리에서 고치기", enabled: false, help: "지시 글을 이 자리에서 고치는 것은 다음 조각에서 엽니다") {}
                }
                Text(t).boardFont(11, design: .monospaced).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
            } else {
                apNote("이 프로젝트의 자율주행 세트(private/projects/\(s.project ?? "")/set/autopilot.md)가 없습니다. 엔진이 공통 규칙만 얹습니다.")
            }
            APFold(title: "판정 명령 \(s.chosen.count)", summary: "읽기 전용 · 출발 뒤에는 고정본으로 판정합니다", axid: "ap-fold-checks", open: bind("checks")) {
                ForEach(s.chosen) { u in Text("\(u.n). " + u.check).boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(2) }
            }
            apNote("하네스 지문: 출발 때 엔진이 하네스 사본과 지문을 남기고, 끝나면 같은지 확인합니다. 저장소 하네스는 고치지 않습니다.")
            apNote("하네스 확인 지점 읽기와 지시 초안은 하네스 읽기(7단계)에서 엽니다.")
        }
    }

    // 만들 티켓 나무(Jira 연결 프로젝트, 와이어프레임 출발 확인 ⑬) — 에픽 아래 스토리, 스토리 아래 작업. 티켓 만들기는 4단계라 새로 만들 것이 있으면 출발이 잠긴다
    private var ticketTree: some View {
        let picked = s.chosen + s.heldByCards
        let ids = Set(picked.compactMap(\.story))
        let st = s.stories.filter { ids.contains($0.id) }
        let t = s.ticketsToMake
        let make = t.stories + t.tasks + t.moved
        return APFold(title: "만들 티켓", summary: "에픽 \(s.epic.isEmpty ? "?" : s.epic) 아래 스토리 \(st.count) · 작업 \(picked.count)"
                      + (make > 0 ? " · 새로 만들거나 고칠 것 \(make)" : " · 모두 이미 있는 티켓"),
                      axid: "ap-fold-tickets", open: bind("tickets"), tint: make > 0 ? APColor.limit : nil) {
            HStack(spacing: 6) {
                APTicket(key: s.epic.isEmpty ? "에픽 없음" : s.epic)
                apNote("에픽 · 번호만 확인했습니다(제목은 연결 뒤)")
            }
            ForEach(st) { story in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        if let k = story.key { APTicket(key: k); APChip(text: "있음", color: APColor.ok) } else { APChip(text: s.storyLabel(story), color: APColor.jira, strong: true); APChip(text: "새로 만듦", color: APColor.limit) }
                        APChip(text: apKind(story.kind), color: APColor.board)
                        if !story.title.isEmpty { Text(story.title).boardFont(11.5).lineLimit(1) }
                    }
                    ForEach(picked.filter { $0.story == story.id }) { u in
                        HStack(spacing: 6) {
                            if let k = u.task { APTicket(key: k) } else { APChip(text: "새 작업", color: APColor.limit) }
                            if u.task != nil, u.story != u.origStory { APChip(text: "다른 스토리로 옮김", color: APColor.limit) }
                            Text(u.title).boardFont(11).lineLimit(1)
                            if !u.depends.isEmpty { apNote("앞: " + u.depends.map { d in s.units.first { $0.id == d }.map { "\($0.n)" } ?? "?" }.joined(separator: "·")) }
                        }
                        .padding(.leading, 16)
                    }
                }
            }
            apNote("작업 티켓은 스토리 아래 둘 수 있는 하위 작업 종류로 만듭니다. 번호는 대기열과 함께 고정되어 실행 중에는 Jira 요청이 없습니다.")
            if make > 0 { apNote("티켓 만들기(새 스토리·새 작업, 옮긴 작업의 부모 바꾸기)는 Jira 연결(4단계)과 함께 만듭니다. 그때까지 출발이 잠깁니다. 이미 있는 티켓의 단위만 고르면 출발할 수 있습니다.", APColor.limit) }
        }
    }
}

// MARK: 자율주행 · 합치기·승격

// 2단계 5-3: 끝나고 정리를 마친 실행(wrapped)의 가지 승격(설계 2장 끝 "승격", 와이어프레임 5판 merge). 단계 목록은 프로젝트 설정의
// 승격 차례(promotion)대로이고 두 모드(따로 작업 트리·원래 작업 트리)가 같다. Jira 연결 실행은 스토리마다 한 묶음이고 기존 가지끼리의 단계(master ← live)는
// 맨 아래 한 번이다. 보드는 git 을 읽기만 하고(커밋 수·바뀐 줄·충돌 미리 보기·받는 가지가 걸린 작업 트리), 가지를 움직이는 일은 모두 사람이 누를 때
// 엔진 finish(merge·promote·discard·cleanup)로 한다. 엔진 문장은 그대로 보인다. 무거운 읽기는 시트를 열 때와 엔진이 한 일 뒤에만 백그라운드에서 하고
// 시계·애니메이션은 두지 않는다(시트는 따로 창이라 보드 배치를 다시 계산하지 않는다)

// git 을 부르고 종료 코드와 표준 출력을 함께 받는다(merge-tree 는 충돌이면 1 로 끝나며 충돌 파일을 낸다). apGit 과 같은 보호
func apGitRun(_ args: [String], _ cwd: String) -> (code: Int32, out: String) {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
    p.arguments = ["--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.attributesFile=/dev/null"] + args
    p.currentDirectoryURL = URL(fileURLWithPath: cwd)
    var env = ProcessInfo.processInfo.environment
    for (k, v) in [("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")] { env[k] = v }
    p.environment = env
    let out = Pipe()
    p.standardOutput = out
    p.standardError = FileHandle.nullDevice
    guard (try? p.run()) != nil else { return (-1, "") }
    let d = out.fileHandleForReading.readDataToEndOfFile()
    p.waitUntilExit()
    return (p.terminationStatus, String(decoding: d, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
}

struct APDiffFile: Equatable { let path: String; let add: String; let del: String }

// 한 단계의 저장소 하나(또는 바뀐 파일 한 범위) — 보드가 읽기만 하는 git 으로 미리 본 것. 범위는 받는 가지 머리(a)...합칠 가지 머리(b)
struct APMergePlan: Equatable {
    var state = "nosrc"     // nosrc(합칠 가지 없음)·nointo(받는 가지 없음, 건너뜀)·same(이미 들어 있음)·ff·merge·conflict·error
    var commits = 0         // 받는 가지에 없는 커밋(a..b)
    var behind = 0          // 받는 가지에만 있는 커밋(b..a) — 병합 커밋이 되는 까닭
    var files: [APDiffFile] = []
    var conflicts: [String] = []
    var error = ""
    var wt: String?         // 받는 가지가 걸린 작업 트리 — 깨끗하면 엔진이 그 자리에서 앞으로 감기로 함께 올린다
    var wtDirty = false     // 그 작업 트리에 커밋하지 않은 추적 파일 변경이 있음 — 엔진이 합치지 않는다
    var withBase = 0        // 출발 가지(dev)에만 있던 커밋 가운데 함께 가는 수(받는 가지..출발 머리)
    var a = "", b = ""
}

struct APMergeTask: Equatable, Identifiable {
    let n: Int
    var id: Int { n }
    var key: String?
    var title = "", status = "", branch = "", merge = "", why = ""
    var repo = ""                // 작업 폴더가 든 저장소(작업 가지가 있는 곳)
    var depends: [String] = []   // 기댄 앞 작업(작업 키, 없으면 "단위 n")
    var spanKey: String { "task|" + repo + "|" + branch }
}

// 스토리의 저장소 하나 — 엔진 branches.stories.<키>.repos.<저장소>(옛 꼴은 저장소 하나). made 는 그 저장소에 스토리 가지를 만들었는가(head 가 있음)
struct APStoryRepo: Equatable, Identifiable {
    let repo: String
    var id: String { repo }
    var branch = ""
    var made = false
    var merged = 0
    var name: String { (repo as NSString).lastPathComponent }
}

struct APMergeStory: Equatable, Identifiable {
    let key: String
    var id: String { key }
    var kind = ""
    var repos: [APStoryRepo] = []   // 스토리 가지가 있는(또는 작업이 건너뛰어 가지 없이 기록만 있는) 저장소들(2026-10-28 여러 저장소에 걸친 스토리)
    var tasks: [APMergeTask] = []
    var made: [APStoryRepo] { repos.filter(\.made) }
    var branch: String { made.first?.branch ?? repos.first?.branch ?? "" }   // 스토리 가지 이름은 저장소마다 같다
    var many: Bool { repos.count > 1 }
    func storyBranch(_ repo: String) -> String? { repos.first { $0.repo == repo }?.branch }
    // 이 스토리의 가지(저장소, 가지): 저장소마다 스토리 가지와 그 저장소에서 일한 작업 가지
    var pairs: [(repo: String, branch: String)] {
        var out: [(repo: String, branch: String)] = []
        for p in repos.map({ ($0.repo, $0.branch) }) + tasks.map({ ($0.repo, $0.branch) }) where !p.0.isEmpty && !p.1.isEmpty && !out.contains(where: { $0 == p }) { out.append(p) }
        return out
    }
}

// 단계 하나 — 승격 차례 한 줄을 이 실행(Jira 면 스토리)에 맞춘 것. source 는 엔진 promoted 의 source(autopilot·story:<키>·branch:<가지>)와 같다
struct APMergeStep: Equatable, Identifiable {
    let num: Int
    let source: String
    let from: String        // 보이는 합칠 쪽 이름(autopilot/<id>·스토리 가지·live)
    let into: String
    let after: String?
    let refs: [String: String]   // 저장소 → 합칠 가지
    var id: String { source + ">" + into }
    var promote: Bool { source.hasPrefix("branch:") }
    var story: String? { source.hasPrefix("story:") ? String(source.dropFirst(6)) : nil }
    var ax: String { (story.map { "\($0)-" } ?? "") + "\(num)" }
}

// run.json·프로젝트 설정에서 읽은 것(시트를 열 때·엔진이 한 일 뒤)
struct APMergeView: Equatable {
    var run: APRun
    var epic: String?
    var steps: [APStep] = []
    var promotionSaved = false
    var repos: [String] = []                 // 엔진이 가지를 만든 저장소(branches.repos)
    var baseHeads: [String: String] = [:]
    var restored: [String: Bool] = [:]
    var restoreNotes: [String: String] = [:]
    var made: [String: [String]] = [:]
    var endHeads: [String: [String: String]] = [:]   // 실행 끝의 가지 머리(branches.end) — 같은 이름으로 다시 만들어진 가지 가리기
    var storyHeads: [String: String] = [:]           // "<스토리 키>|<저장소>" → 스토리 기록의 head
    var promoted: [APPromoted] = []
    var failed: [APFailed] = []                      // 엔진이 저장소 하나에서 하지 못한 것(branches.failed)
    var discarded: Set<String> = []
    var cleaned = 0
    var moved: [String] = []
    var createdOther: [String] = []
    var harnessSame: Bool?
    var harnessFiles = 0
    var harnessFolders: [String] = []
    var harnessChanged: [APChange] = []
    var stories: [APMergeStory] = []
    var withAutopilot = true                 // 자율주행 가지 묶음을 보일지(Jira 실행은 스토리 없는 단위가 있을 때만)
    var jira: Bool { !stories.isEmpty }
    // 스토리·자율주행 가지가 마지막으로 합쳐지는 가지(승격 차례에서 자율주행 가지를 받는 마지막 단계, 예 live)
    var lastAuto: String? { steps.last { $0.from == "autopilot" }?.into }
}

struct APMergeResult: Equatable { let ok: Bool; let title: String; let text: String; var partial = false }   // partial: 저장소 일부만 함(엔진 종료 코드 1)
struct APRepoPlan: Identifiable { let repo: String; let plan: APMergePlan?; var id: String { repo } }

@MainActor final class APMerge: ObservableObject {
    static let shared = APMerge()
    @Published private(set) var v: APMergeView?
    @Published private(set) var plans: [String: APMergePlan] = [:]     // "<단계 id>|<저장소>"
    @Published private(set) var spans: [String: APMergePlan] = [:]     // "all|<저장소>" 출발 머리...자율주행 가지, "task|<가지>" 스토리 가지...작업 가지
    @Published private(set) var existing: [String: Set<String>] = [:]  // 저장소 → 지금 있는 가지
    @Published private(set) var contained: Set<String> = []            // 마지막 자율주행 단계의 가지(live)에 다 들어간 가지("<저장소>|<가지>")
    @Published private(set) var foreign: Set<String> = []              // 이 실행이 끝난 뒤 같은 이름으로 다시 만들어졌거나 바뀐 스토리·작업 가지("<저장소>|<가지>")
    @Published private(set) var reading = false
    @Published private(set) var busy: String?
    @Published private(set) var result: APMergeResult?
    @Published var confirm: String?                                    // 화면 안 확인 중인 것(discard·discard:<키>·unmerged:<키>)
    @Published var openDiff: Set<String> = []
    @Published private(set) var picked: [String: String] = [:]
    @Published private(set) var patches: [String: String] = [:]
    @Published private(set) var fromStart = false
    @Published private(set) var shown = false
    private(set) var dir = ""
    private let queue = DispatchQueue(label: "claude-board.autopilot.merge")
    private var gen = 0
    private var sheet: NSWindow?
    private var resizeWatch: NSObjectProtocol?

    // 시트를 연다 — 출발 준비 시트가 떠 있으면 닫고(준비는 남는다) 연다. 보드를 앞으로 꺼내지 않는다
    func open(_ runDir: String, fromStart: Bool = false) {
        guard window != nil, !runDir.isEmpty else { return }
        if sheet != nil {
            if runDir == dir { return }
            close()
        }
        if window.attachedSheet != nil {
            guard APStart.shared.shown else { return }
            APStart.shared.close()
        }
        if runDir != dir { plans = [:]; spans = [:]; existing = [:]; contained = []; result = nil; confirm = nil; openDiff = []; picked = [:]; patches = [:] }
        dir = runDir
        self.fromStart = fromStart
        reload()
        DispatchQueue.main.async { MainActor.assumeIsolated { self.present() } }
    }

    // 앞 시트가 아직 붙어 있으면(닫는 중) 조금 뒤 다시 본다
    private func present(_ tries: Int = 0) {
        guard window != nil, sheet == nil, !dir.isEmpty else { return }
        if window.attachedSheet != nil {
            if tries < 20 { DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { MainActor.assumeIsolated { self.present(tries + 1) } } }
            return
        }
        let size = sheetSize()
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        let host = NSHostingController(rootView: APMergeSheet(m: self))
        host.sizingOptions = []
        w.contentViewController = host
        w.setContentSize(size)
        sheet = w
        shown = true
        window.beginSheet(w)
        resizeWatch = NotificationCenter.default.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { if let s = self?.sheet { s.setContentSize(self?.sheetSize() ?? s.frame.size) } }
        }
    }
    private func sheetSize() -> NSSize {
        let c = window.contentLayoutRect.size
        return NSSize(width: max(c.width - 8, 480), height: max(c.height - 8, 460))
    }
    func close() {
        // 닫은 시트의 SwiftUI 보기를 놓는다(창이 남아 옛 단추가 접근성 나무에 남지 않게)
        if let s = sheet { window.endSheet(s); s.orderOut(nil); s.contentViewController = nil }
        sheet = nil
        if let r = resizeWatch { NotificationCenter.default.removeObserver(r) }
        resizeWatch = nil
        shown = false
        if APStart.shared.hasDraft { APStart.shared.checkLeftovers() }
    }
    // 나중에 — 보드 본체의 끝난 실행 줄을 하루 숨긴다(다시 알리는 알림은 4단계)
    func later() {
        if let id = v?.run.id { AutopilotStore.shared.snooze(id) }
        close()
    }
    func backToStart() {
        close()
        apWhenNoSheet { APStart.shared.open() }
    }

    // run.json 을 다시 읽고 git 을 다시 본다
    func reload() {
        guard let run = APRun.parse(id: (dir as NSString).lastPathComponent, dir: dir), let r = apRead(dir + "/run.json") as? [String: Any] else { v = nil; return }
        var m = APMergeView(run: run)
        m.epic = apText(r["epic"])
        let br = r["branches"] as? [String: Any] ?? [:]
        let reposInfo = br["repos"] as? [String: Any] ?? [:]
        m.repos = reposInfo.keys.sorted()
        for (k, val) in reposInfo {
            let i = val as? [String: Any] ?? [:]
            if let h = (i["base"] as? [String: Any])?["head"] as? String { m.baseHeads[k] = h }
            if let b = i["restored"] as? Bool { m.restored[k] = b }
            if let n = apText(i["restore_note"]) { m.restoreNotes[k] = n }
            m.made[k] = apStrings(i["made"])
        }
        for (k, val) in br["end"] as? [String: Any] ?? [:] { m.endHeads[k] = (val as? [String: Any] ?? [:]).compactMapValues { $0 as? String } }
        for (k, val) in br["stories"] as? [String: Any] ?? [:] {
            for (r, e) in apStoryRepos(val) { if let h = e["head"] as? String { m.storyHeads[k + "|" + r] = h } }
        }
        m.promoted = apPromotedRows(br)
        m.failed = apFailedRows(br)
        m.discarded = Set((br["discarded"] as? [[String: Any]] ?? []).compactMap { $0["source"] as? String })
        m.cleaned = (br["cleaned"] as? [Any] ?? []).count
        m.moved = (br["moved"] as? [[String: Any]] ?? []).map { "\((($0["repo"] as? String ?? "") as NSString).lastPathComponent) \($0["branch"] as? String ?? "?")" }
        m.createdOther = (br["created"] as? [[String: Any]] ?? []).filter { $0["by"] as? String == "other" }.compactMap { $0["branch"] as? String }
        if let hs = r["harness"] as? [String: Any] {
            m.harnessFiles = Int(apNum(hs["files"]) ?? 0)
            m.harnessFolders = apStrings(hs["folders"])
            if let same = hs["same"] as? Bool {
                m.harnessSame = same
                m.harnessChanged = (hs["changed"] as? [[String: Any]] ?? []).map { APChange(path: $0["path"] as? String ?? "?", change: $0["change"] as? String ?? "") }
            }
        }
        // 승격 차례 — 프로젝트 설정, 없으면 저장소 가지로 채운 꼴(보이기만. 엔진은 차례 없이 합치기는 하고 기존 가지끼리는 거부한다)
        m.steps = apPromotion(run.project)
        m.promotionSaved = !m.steps.isEmpty
        if m.steps.isEmpty, let first = m.repos.first {
            // 저장소 가지는 엔진이 출발 때 남긴 가지 머리(branches.start)로 본다(메인 스레드에서 git 을 부르지 않는다)
            let b = (((br["start"] as? [String: Any])?[first] as? [String: Any]) ?? [:]).keys.sorted()
            let n = APStart.roles(b)
            if let d = n.dev { m.steps.append(APStep(from: "autopilot", into: d)) }
            if let l = n.live { m.steps.append(APStep(from: "autopilot", into: l, after: n.dev)) }
            if let ms = n.master { m.steps.append(n.live.map { APStep(from: $0, into: ms, after: $0) } ?? APStep(from: "autopilot", into: ms, after: n.dev)) }
        }
        // Jira 스토리 — 엔진 branches.stories(저장소마다, 옛 꼴은 저장소 하나)와 대기열의 작업
        let st = br["stories"] as? [String: Any] ?? [:]
        let idToKey = Dictionary(run.units.map { ($0.unitId, $0.task ?? "단위 \($0.n)") }, uniquingKeysWith: { a, _ in a })
        var order: [String] = []
        for u in run.units { if let s = u.story, !order.contains(s) { order.append(s) } }
        for k in st.keys.sorted() where !order.contains(k) { order.append(k) }
        m.stories = order.map { k in
            let rs = apStoryRepos(st[k])
            var x = APMergeStory(key: k)
            x.repos = rs.keys.sorted().map { r in
                let e = rs[r] ?? [:]
                return APStoryRepo(repo: r, branch: e["branch"] as? String ?? "", made: e["head"] != nil, merged: (e["merged"] as? [Any])?.count ?? 0)
            }
            let mine = run.units.filter { $0.story == k }
            x.kind = mine.first?.kind ?? ""
            x.tasks = mine.map { u in
                // 작업의 저장소: 작업 폴더가 든 저장소. 옛 꼴(저장소 하나)이면 그 저장소
                let repo = run.repoPath(u) ?? (x.repos.count == 1 ? x.repos[0].repo : "")
                return APMergeTask(n: u.n, key: u.task, title: u.title, status: u.status, branch: u.branch ?? "", merge: u.merge ?? "", why: apPlain(u.mergeWhy),
                                   repo: repo, depends: u.depends.map { idToKey[$0] ?? $0 })
            }
            return x
        }
        m.withAutopilot = m.stories.isEmpty || run.units.contains { $0.story == nil }
        v = m
        readGit()
    }

    // 단계 — 자율주행 가지 묶음, 스토리 묶음, 기존 가지끼리의 단계
    func autopilotSteps(_ m: APMergeView) -> [APMergeStep] {
        m.steps.enumerated().compactMap { i, s in
            guard s.from == "autopilot", !m.run.work.isEmpty else { return nil }
            return APMergeStep(num: i + 1, source: "autopilot", from: m.run.work, into: s.into, after: s.after,
                               refs: Dictionary(m.repos.map { ($0, m.run.work) }, uniquingKeysWith: { a, _ in a }))
        }
    }
    // 스토리 단계 — 그 스토리가 가지를 만든 저장소마다 함께(엔진 finish --story 가 저장소를 모두 먼저 보고 한다)
    func storySteps(_ m: APMergeView, _ s: APMergeStory) -> [APMergeStep] {
        let refs = Dictionary(s.made.map { ($0.repo, $0.branch) }, uniquingKeysWith: { a, _ in a })
        return m.steps.enumerated().compactMap { i, st in
            guard st.from == "autopilot", !refs.isEmpty, !s.branch.isEmpty else { return nil }
            return APMergeStep(num: i + 1, source: "story:" + s.key, from: s.branch, into: st.into, after: st.after, refs: refs)
        }
    }
    func promoteSteps(_ m: APMergeView) -> [APMergeStep] {
        m.steps.enumerated().compactMap { i, s in
            guard s.from != "autopilot" else { return nil }
            return APMergeStep(num: i + 1, source: "branch:" + s.from, from: s.from, into: s.into, after: s.after,
                               refs: Dictionary(m.repos.map { ($0, s.from) }, uniquingKeysWith: { a, _ in a }))
        }
    }
    func allSteps(_ m: APMergeView) -> [APMergeStep] {
        (m.withAutopilot ? autopilotSteps(m) : []) + m.stories.flatMap { storySteps(m, $0) } + promoteSteps(m)
    }

    // 차례 — 엔진 finish 와 같은 규칙(합치기는 같은 합칠 쪽이 after 에 합쳐졌을 때, 기존 가지끼리는 무엇이든 after 에 합쳐졌을 때)
    // 일부 저장소만 합친 단계(엔진 branches.failed 가 남음)는 끝나지 않은 것으로 본다 — 다시 누르면 합친 저장소는 엔진이 "그대로"로 넘긴다
    func done(_ st: APMergeStep) -> APPromoted? {
        guard let m = v, leftover(st).isEmpty else { return nil }
        return m.promoted.last { $0.into == st.into && $0.source == st.source }
    }
    func leftover(_ st: APMergeStep) -> [APFailed] { v.map { apUnresolved($0.promoted, $0.failed, source: st.source, into: st.into) } ?? [] }
    func doneRows(_ st: APMergeStep) -> [APPromoted] {   // 저장소마다 실제로 합친 줄(다시 눌러 "그대로"가 뒤에 적혀도 앞서 합친 방식을 보인다)
        guard let m = v else { return [] }
        return st.refs.keys.sorted().compactMap { r in
            let rows = m.promoted.filter { $0.into == st.into && $0.source == st.source && $0.repo == r }
            return rows.last { $0.how != "same" } ?? rows.last
        }
    }
    func afterDone(_ st: APMergeStep) -> Bool {
        guard let a = st.after else { return true }
        guard let m = v else { return false }
        return apStepDone(m.promoted, m.failed, source: st.promote ? nil : st.source, into: a)
    }
    func reached(_ source: String, _ into: String?) -> Bool {
        guard let m = v else { return false }
        guard let into else { return m.promoted.contains { $0.source == source } }
        return apStepDone(m.promoted, m.failed, source: source, into: into)
    }
    func plansOf(_ st: APMergeStep) -> [APRepoPlan] { st.refs.keys.sorted().map { APRepoPlan(repo: $0, plan: plans[st.id + "|" + $0]) } }

    // 누를 수 없는 까닭(없으면 nil) — 엔진이 거부할 것을 미리 막는다. 막지 못한 것(그사이 바뀜)은 엔진 문장이 그대로 보인다.
    // orderLock 은 미리 본 git 밖의 까닭(차례·끝 정리·하는 중)이고, 미리 본 것의 까닭은 저장소 줄이 따로 보인다
    func orderLock(_ st: APMergeStep) -> String? {
        guard let m = v else { return "실행을 읽지 못했습니다" }
        if !m.run.wrapped { return "끝 정리를 마치지 않아 엔진이 받지 않습니다" }
        if busy != nil { return "엔진이 하는 중입니다" }
        if m.discarded.contains(st.source) { return "버린 가지입니다" }
        if !afterDone(st) {
            if st.promote, m.jira { return "\(st.after ?? "") 에 스토리를 합치면 누를 수 있습니다." }
            let prev = m.steps.firstIndex { $0.into == st.after }.map { apCircled($0 + 1) } ?? (st.after ?? "")
            return "\(prev) 을 마치면 누를 수 있습니다."
        }
        if st.promote, !m.promotionSaved { return "프로젝트 설정에 승격 차례가 없어 엔진이 기존 가지끼리는 합치지 않습니다" }
        if let k = st.story, let s = m.stories.first(where: { $0.key == k }) {
            let bad = s.made.filter { foreign.contains($0.repo + "|" + $0.branch) }
            if !bad.isEmpty {
                return (s.many ? bad.map(\.name).joined(separator: "·") + " 의 " : "")
                    + "스토리 가지 \(s.branch) 는 이 실행이 끝난 뒤 같은 이름으로 다시 만들어졌거나 바뀌어(이어 간 실행 등) 이 실행에서는 합치지 않습니다"
            }
        }
        return nil
    }
    func lockReason(_ st: APMergeStep) -> String? {
        if let o = orderLock(st) { return o }
        let ps = plansOf(st)
        if reading || ps.contains(where: { $0.plan == nil }) { return "가지를 읽는 중입니다" }
        let named = { (r: String) in ps.count > 1 ? (r as NSString).lastPathComponent + ": " : "" }
        if let c = ps.first(where: { $0.plan?.state == "conflict" }) { return named(c.repo) + "합치면 충돌합니다. 보드 탭에서 그 저장소를 열어 직접 풉니다" }
        if let c = ps.first(where: { $0.plan?.state == "error" }) { return named(c.repo) + "합칠 tree 를 만들지 못했습니다: \(c.plan?.error ?? "")" }
        if let c = ps.first(where: { $0.plan?.wtDirty == true }) { return named(c.repo) + "\(st.into) 가 걸린 작업 트리에 커밋하지 않은 변경이 있어 엔진이 합치지 않습니다. 커밋하거나 치운 뒤 '다시 보기'를 누릅니다" }
        if let c = ps.first(where: { $0.plan?.state == "nosrc" }) { return named(c.repo) + "합칠 가지 \(st.from) 가 없습니다(버렸거나 지워짐)" }
        return nil
    }
    func discardLock(_ source: String) -> String? {
        guard let m = v else { return "실행을 읽지 못했습니다" }
        if !m.run.wrapped { return "끝 정리를 마치지 않아 엔진이 받지 않습니다" }
        if busy != nil { return "엔진이 하는 중입니다" }
        if m.discarded.contains(source) { return "이미 버렸습니다" }
        if let p = m.promoted.first(where: { $0.source == source }) { return "\(p.into) 에 합쳐서 버릴 수 없습니다. 버리기는 합치기 전에만 합니다" }
        return nil
    }

    // 엔진 부르기 — 한 번에 하나. 끝나면 엔진 문장을 그대로 보이고 run.json·git 을 다시 본다
    func press(_ st: APMergeStep) {
        guard lockReason(st) == nil else { return }
        var args = ["finish", dir]
        if st.promote { args += ["promote", st.from, "--into", st.into] }
        else { args += ["merge", "--into", st.into] + (st.story.map { ["--story", $0] } ?? []) }
        engine(args, what: "\(st.into) ← \(st.from)")
    }
    func discard(_ story: String?) {
        guard discardLock(story.map { "story:" + $0 } ?? "autopilot") == nil else { return }
        engine(["finish", dir, "discard"] + (story.map { ["--story", $0] } ?? []), what: story.map { "스토리 \($0) 버리기" } ?? "버리기")
    }
    func cleanup(_ story: String, unmerged: Bool) {
        engine(["finish", dir, "cleanup", "--story", story] + (unmerged ? ["--unmerged"] : []),
               what: unmerged ? "스토리 \(story) 의 합치지 못한 작업 가지까지 지우기" : "스토리 \(story) 의 합친 가지 지우기")
    }
    private func engine(_ args: [String], what: String) {
        guard busy == nil, !dir.isEmpty else { return }
        busy = what; result = nil; confirm = nil
        let d = dir
        queue.async {
            let r = apEngine(args)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.busy = nil
                    guard self.dir == d else { return }
                    self.result = APMergeResult(ok: r.code == 0, title: what, text: r.out.isEmpty ? "(엔진 출력 없음, 종료 코드 \(r.code))" : r.out,
                                                partial: r.code == 1 && r.out.contains("일부만 했습니다"))
                    self.picked = [:]; self.patches = [:]
                    self.reload()
                    AutopilotStore.shared.tick()
                }
            }
        }
    }

    // 보드가 읽는 git(백그라운드) — 단계마다 저장소마다 미리 보기, 바뀐 파일 범위, 지금 있는 가지, live 에 다 들어간 가지
    private func readGit() {
        guard let m = v else { return }
        gen += 1
        let g = gen, steps = allSteps(m), lastAuto = m.lastAuto
        reading = true
        queue.async {
            var plans: [String: APMergePlan] = [:], spans: [String: APMergePlan] = [:], have: [String: Set<String>] = [:], contained = Set<String>()
            var wts: [String: [String: String]] = [:], heads: [String: [String: String]] = [:], foreign = Set<String>()
            for r in Set(m.repos + m.stories.flatMap { $0.repos.map(\.repo) + $0.tasks.map(\.repo) }) where !r.isEmpty && FileManager.default.fileExists(atPath: r) {
                var hs: [String: String] = [:]
                for line in (apGit(["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/heads/"], r) ?? "").split(separator: "\n") {
                    let f = line.split(separator: "\t")
                    if f.count == 2, f[0].hasPrefix("refs/heads/") { hs[String(f[0].dropFirst(11))] = String(f[1]) }
                }
                heads[r] = hs
                have[r] = Set(hs.keys)
                wts[r] = APMerge.checkedOut(r)
            }
            // 엔진 finish 와 같은 기준: 실행 끝의 머리와 지금 머리가 다르면 이 실행의 가지가 아니다
            for s in m.stories {
                for (r, b) in s.pairs {
                    guard let h = heads[r]?[b] else { continue }
                    let want = m.endHeads[r]?[b] ?? (b == s.storyBranch(r) ? m.storyHeads[s.key + "|" + r] : nil)
                    if want != h { foreign.insert(r + "|" + b) }
                }
            }
            for st in steps {
                for (r, src) in st.refs {
                    let base = st.after != nil && m.run.bases[r] == st.after ? m.baseHeads[r] : nil
                    plans[st.id + "|" + r] = have[r] == nil ? APMergePlan(state: "nosrc") : APMerge.plan(r, src: src, into: st.into, wts: wts[r] ?? [:], baseHead: base)
                }
            }
            if !m.run.work.isEmpty {
                for r in m.repos where have[r] != nil { if let b = m.baseHeads[r] { spans["all|" + r] = APMerge.span(r, b, m.run.work) } }
            }
            for s in m.stories {
                // 작업 가지는 그 저장소의 스토리 가지에서 본다. 스토리 가지를 지운 뒤(합친 가지 지우기)에는 마지막 자율주행 단계의 가지(live)에 없는 커밋을 센다(엔진 cleanup 과 같은 기준)
                for t in s.tasks where !t.branch.isEmpty && have[t.repo]?.contains(t.branch) == true && t.merge != "merged" {
                    let sb = s.storyBranch(t.repo) ?? s.branch
                    spans[t.spanKey] = APMerge.span(t.repo, have[t.repo]?.contains(sb) == true ? sb : (lastAuto ?? sb), t.branch)
                }
                guard let live = lastAuto else { continue }
                for r in Set(s.pairs.map(\.repo)) where have[r] != nil {
                    guard let lh = apGit(["rev-parse", "-q", "--verify", "refs/heads/\(live)^{commit}"], r), !lh.isEmpty else { continue }
                    for (_, b) in s.pairs.filter({ $0.repo == r }) where have[r]?.contains(b) == true {
                        if apGitRun(["merge-base", "--is-ancestor", "refs/heads/" + b, lh], r).code == 0 { contained.insert(r + "|" + b) }
                    }
                }
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.gen == g else { return }
                    self.plans = plans; self.spans = spans; self.existing = have; self.contained = contained; self.foreign = foreign
                    self.reading = false
                }
            }
        }
    }

    nonisolated static func checkedOut(_ r: String) -> [String: String] {
        var m: [String: String] = [:], wt = ""
        for line in (apGit(["worktree", "list", "--porcelain"], r) ?? "").split(separator: "\n") {
            if line.hasPrefix("worktree ") { wt = String(line.dropFirst(9)) }
            if line.hasPrefix("branch refs/heads/") { m[String(line.dropFirst(18))] = wt }
        }
        return m
    }
    nonisolated static func head(_ r: String, _ b: String) -> String? {
        apGit(["rev-parse", "-q", "--verify", "refs/heads/\(b)^{commit}"], r).flatMap { $0.isEmpty ? nil : $0 }
    }
    nonisolated static func numstat(_ r: String, _ a: String, _ b: String) -> [APDiffFile] {
        (apGit(["diff", "--numstat", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", "\(a)...\(b)"], r) ?? "").split(separator: "\n").compactMap { l in
            let f = l.split(separator: "\t", maxSplits: 2).map(String.init)
            return f.count == 3 ? APDiffFile(path: f[2], add: f[0], del: f[1]) : nil
        }
    }
    // 합치기 미리 보기 — 엔진 mergeRefs 와 같은 판단(같은 merge-tree 명령이라 미리 본 것과 엔진 판정이 어긋나지 않는다. merge-tree 는 ref 를 움직이지 않고
    // 이름 없는 tree 객체만 남긴다)
    nonisolated static func plan(_ r: String, src: String, into: String, wts: [String: String], baseHead: String?) -> APMergePlan {
        var p = APMergePlan()
        guard let sh = head(r, src) else { p.state = "nosrc"; return p }
        guard let ih = head(r, into) else { p.state = "nointo"; return p }
        p.a = ih; p.b = sh
        p.commits = Int(apGit(["rev-list", "--count", "\(ih)..\(sh)"], r) ?? "") ?? 0
        p.behind = Int(apGit(["rev-list", "--count", "\(sh)..\(ih)"], r) ?? "") ?? 0
        if p.commits == 0 { p.state = "same" }
        else if p.behind == 0 { p.state = "ff" }
        else {
            let mt = apGitRun(["merge-tree", "--write-tree", "--name-only", "--no-messages", ih, sh], r)
            let lines = mt.out.split(separator: "\n").map(String.init)
            switch mt.code {
            case 0: p.state = "merge"
            case 1: p.state = "conflict"; p.conflicts = Array(lines.dropFirst())
            default: p.state = "error"; p.error = lines.first ?? "종료 코드 \(mt.code)"
            }
        }
        if p.state != "same" { p.files = numstat(r, ih, sh) }
        if let w = wts[into] {
            p.wt = w
            if p.state != "same" { p.wtDirty = !(apGit(["status", "--porcelain", "--untracked-files=no"], w) ?? "x").isEmpty }
        }
        if let bh = baseHead { p.withBase = Int(apGit(["rev-list", "--count", "\(ih)..\(bh)"], r) ?? "") ?? 0 }
        return p
    }
    // 바뀐 파일 범위(a...b) — 커밋 수와 파일마다 더한 줄·뺀 줄
    nonisolated static func span(_ r: String, _ a: String, _ b: String) -> APMergePlan {
        var p = APMergePlan()
        guard let bh = head(r, b) else { p.state = "nosrc"; return p }
        let ah = head(r, a) ?? a
        p.state = "span"; p.a = ah; p.b = bh
        p.commits = Int(apGit(["rev-list", "--count", "\(ah)..\(bh)"], r) ?? "") ?? 0
        p.files = numstat(r, ah, bh)
        return p
    }

    // 바뀐 줄 — 파일을 누를 때만 읽는다(400줄까지)
    func toggleDiff(_ key: String) { if openDiff.contains(key) { openDiff.remove(key) } else { openDiff.insert(key) } }
    func pick(_ key: String, _ file: String, repo: String, _ p: APMergePlan) {
        picked[key] = file
        let pk = key + "|" + file
        guard patches[pk] == nil, !p.a.isEmpty, !p.b.isEmpty else { return }
        queue.async {
            let t = apGit(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", "\(p.a)...\(p.b)", "--", file], repo) ?? ""
            let lines = t.split(separator: "\n", omittingEmptySubsequences: false)
            let text = t.isEmpty ? "(바뀐 줄을 읽지 못했습니다)" : lines.prefix(400).joined(separator: "\n") + (lines.count > 400 ? "\n… \(lines.count - 400)줄 더 있음" : "")
            DispatchQueue.main.async { MainActor.assumeIsolated { self.patches[pk] = text } }
        }
    }

    // 충돌 — 그 저장소를 보드 탭(셸)으로 열어 사람이 직접 푼다
    func openTab(_ repo: String) {
        close()
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                if !TerminalHub.shared.expanded { setTerminalExpanded(true) }
                TerminalHub.shared.openShell(in: repo)   // 사람이 시트에서 누른 것이라 보드는 이미 앞에 있다(앞으로 꺼내지 않는다)
            }
        }
    }
}

// 창에 붙은 시트가 떨어진 뒤에 한다(닫는 중인 시트가 있으면 조금 뒤 다시 본다)
@MainActor func apWhenNoSheet(_ tries: Int = 0, _ f: @escaping @MainActor () -> Void) {
    if window.attachedSheet == nil || tries >= 20 { f(); return }
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { MainActor.assumeIsolated { apWhenNoSheet(tries + 1, f) } }
}

func apCircled(_ n: Int) -> String { n >= 1 && n <= 10 ? String(Array("①②③④⑤⑥⑦⑧⑨⑩")[n - 1]) : "\(n)." }
func apHome(_ p: String) -> String { p.replacingOccurrences(of: NSHomeDirectory(), with: "~") }

// 합치기·승격 시트 — 창 전체를 덮는다(와이어프레임 합치기·승격 ⑪, 크기와 상관없음)
struct APMergeSheet: View {
    @ObservedObject var m: APMerge
    var body: some View {
        VStack(spacing: 0) {
            head
            Divider().opacity(0.35)
            ScrollView(showsIndicators: true) {
                SwiftUI.Group {
                    if let v = m.v { content(v) } else { apNote("이 실행을 읽지 못했습니다. 실행 폴더가 지워졌을 수 있습니다.", APColor.fail) }
                }
                .padding(16)
                .frame(maxWidth: 1000, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            Divider().opacity(0.35)
            foot.padding(.horizontal, 16).padding(.vertical, 10)
        }
        .frame(minWidth: 460, minHeight: 440)
        .background(Backdrop())
        .preferredColorScheme(.dark)
        .boardFont(13)
    }

    private var head: some View {
        let v = m.v
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                APMark()
                Text("합치기·승격").boardFont(15, weight: .bold).fixedSize()
                if let r = v?.run {
                    let sc = apStateChip(r)
                    APChip(text: sc.0, color: sc.1, strong: true)
                }
                Spacer(minLength: 4)
                if m.reading { APChip(text: "가지를 읽는 중", color: APColor.muted) }
                if let b = m.busy { APChip(text: "엔진이 하는 중: \(b)", color: APColor.work, strong: true).lineLimit(1) }
                if m.fromStart, APStart.shared.hasDraft {
                    APButton(title: "출발 준비로 돌아가기", enabled: m.busy == nil, help: "닫아 둔 출발 준비를 다시 엽니다", axid: "ap-merge-to-start") { m.backToStart() }
                }
                APButton(title: "다시 보기", icon: "arrow.clockwise", enabled: m.busy == nil && !m.reading, help: "run.json 과 가지를 다시 읽습니다(저장소에서 사람이 고친 뒤)", axid: "ap-merge-reload") { m.reload() }
                APButton(title: "닫기", enabled: m.busy == nil, help: "가지는 그대로 남습니다", axid: "ap-merge-close") { m.close() }
            }
            if let r = v?.run {
                WrapRow(spacing: 6) {
                    Text(r.name).boardFont(12.5, weight: .bold).lineLimit(1)
                    Text("\(apDate(r.started)) → \(r.ended.map { apDay($0) == apDay(r.started) ? apClock($0) : apDate($0) } ?? "?")").boardFont(11, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                    APChip(text: r.modeLabel, color: r.inPlace ? APColor.wait : APColor.muted)
                    if v?.jira == true { APChip(text: "Jira 연결" + (v?.epic.map { " · 에픽 \($0)" } ?? ""), color: APColor.jira) }
                    if let p = r.project, p != r.name { APChip(text: p, color: APColor.board) }
                }
                apNote("실행이 끝나도 가지는 그대로 남고, 사람이 바뀐 줄을 본 뒤 단계마다 합칩니다. 자동으로 합치지 않습니다.")
            }
        }
        .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 8)
    }

    private var foot: some View {
        HStack(spacing: 8) {
            apNote(m.v?.jira == true ? "자동으로 합치지 않습니다 · 버리기는 스토리마다, dev 에 합치기 전에만" : "자동으로 합치지 않습니다 · 버리기는 합치기 전에만")
                .frame(maxWidth: .infinity, alignment: .leading)
            APButton(title: "나중에", enabled: m.busy == nil, help: "보드의 끝난 실행 줄을 하루 동안 숨깁니다. 하루 뒤 다시 보입니다", axid: "ap-merge-later") { m.later() }
        }
    }

    @ViewBuilder private func content(_ v: APMergeView) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            if let r = m.result { resultBox(r) }
            if v.run.active { apNote("이 실행은 아직 끝나지 않았습니다. 끝난 뒤에 합칩니다.", APColor.limit) }
            else if !v.run.wrapped { apNote("이 실행은 끝 정리(하네스 끝 검사·기록)를 마치지 않아 엔진이 합치기·버리기를 받지 않습니다. 엔진이 다시 뜨면 이어서 정리합니다.", APColor.limit) }
            if v.repos.isEmpty {
                apNote("이 실행에는 엔진이 만든 가지가 없습니다(git 저장소에서 돌지 않았거나 가지 기능 전의 실행).", APColor.limit)
            } else if !v.repos.allSatisfy({ FileManager.default.fileExists(atPath: $0) }) {
                apNote("저장소를 찾지 못했습니다: " + v.repos.filter { !FileManager.default.fileExists(atPath: $0) }.map(apHome).joined(separator: ", "), APColor.fail)
            }
            summaryBox(v)
            harnessBox(v)
            headsBox(v)
            if v.jira {
                ForEach(v.stories) { s in storyBox(v, s) }
                if v.withAutopilot { stepsBox(v, title: "스토리 없는 단위 · 자율주행 가지", steps: m.autopilotSteps(v)); discardBox(v) }
                else { unusedAutopilot(v) }
                let ps = m.promoteSteps(v)
                if !ps.isEmpty {
                    APBox(title: "기존 가지 올리기", note: "스토리와 상관없이 한 번 · \(v.lastAuto ?? "live") 에 스토리를 하나라도 합친 뒤에만", axid: "ap-merge-promote") {
                        ForEach(ps) { st in stepView(v, st) }
                    }
                }
            } else {
                if !v.discarded.contains("autopilot") { filesBox(v) }
                unpassedBox(v)
                stepsBox(v, title: "합치기·승격", steps: m.autopilotSteps(v) + m.promoteSteps(v))
                discardBox(v)
            }
        }
    }

    private func resultBox(_ r: APMergeResult) -> some View {
        APBox(title: r.ok ? "엔진이 한 일" : r.partial ? "엔진이 일부만 했습니다" : "엔진이 하지 않았습니다", note: r.title + " · 엔진 문장 그대로", tint: r.ok ? APColor.ok : APColor.fail, axid: "ap-merge-result") {
            Text(r.text).boardFont(11, design: .monospaced).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        }
    }

    // ① 요약 — 단위 결과 수, 일한 시간, 비용, 보고서 보기(3단계라 잠금)
    private func summaryBox(_ v: APMergeView) -> some View {
        let u = v.run.units
        func n(_ s: String) -> Int { u.filter { $0.status == s }.count }
        return APBox(title: "요약", axid: "ap-merge-summary") {
            WrapRow(spacing: 6) {
                APChip(text: "통과 \(n("done"))", color: APColor.ok, strong: true)
                APChip(text: "미통과 \(n("failed"))", color: n("failed") > 0 ? APColor.fail : APColor.muted)
                APChip(text: "상한에서 멈춤 \(n("capped"))", color: n("capped") > 0 ? APColor.cap : APColor.muted)
                if n("skipped") > 0 { APChip(text: "건너뜀 \(n("skipped"))") }
                if n("stopped") + n("pending") + n("running") > 0 { APChip(text: "끝까지 못 감 \(n("stopped") + n("pending") + n("running"))") }
                Text("일한 시간 \(apDur(u.reduce(0) { $0 + $1.activeSec })) · \(apUSD(v.run.totalCost)) USD").boardFont(11).monospacedDigit().foregroundStyle(.secondary).fixedSize()
                APButton(title: "보고서 보기", enabled: false, help: "아직 만들지 않았습니다(보고서 틀과 함께)") {}
            }
        }
    }

    // ② 하네스 — 출발 때 지문과 끝을 견준 엔진 기록. 다르면 되돌리지 않고 바뀐 파일만
    private func harnessBox(_ v: APMergeView) -> some View {
        APBox(title: "하네스", axid: "ap-merge-harness") {
            if let same = v.harnessSame {
                if same {
                    Label("출발 때와 같음 · " + (v.run.mode == "worktree" ? "자율주행 가지 기준 · 파일 \(v.harnessFiles)개"
                                              : (v.harnessFolders.count > 1 ? "저장소 \(v.harnessFolders.count)개 모두 같음 · " + v.harnessFolders.map { ($0 as NSString).lastPathComponent }.joined(separator: " · ") : "파일 \(v.harnessFiles)개")),
                          systemImage: "checkmark.shield").boardFont(11.5).foregroundStyle(APColor.ok)
                } else {
                    Label("출발 때와 다릅니다(\(v.harnessChanged.count)곳). 실행 중에 사람이 고쳤을 수 있어 되돌리지 않았습니다. 합치기 전에 바뀐 줄을 봅니다.", systemImage: "exclamationmark.shield")
                        .boardFont(11.5).foregroundStyle(APColor.limit)
                    ForEach(v.harnessChanged.prefix(10), id: \.path) { c in
                        Text("\(["changed": "바뀜", "added": "더함", "removed": "없어짐"][c.change] ?? c.change) · \(apHome(c.path))").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary)
                    }
                }
            } else {
                apNote("하네스 기록 없음(하네스 지문 전 실행이거나 끝 검사를 하지 못함)")
            }
        }
    }

    // ③ 가지 머리 — 움직인 기존 가지(0 이어야 함), 새 가지, 출발 가지. 원래 작업 트리면 ⑨ 원래 가지로 되돌렸는지
    private func headsBox(_ v: APMergeView) -> some View {
        let bases = Set(v.run.bases.values).sorted().joined(separator: "·")
        let commits = v.repos.compactMap { m.spans["all|" + $0]?.commits }.reduce(0, +)
        return APBox(title: "가지 머리", axid: "ap-merge-heads") {
            HStack(spacing: 6) {
                APChip(text: "움직인 기존 가지 \(v.moved.count)", color: v.moved.isEmpty ? APColor.ok : APColor.fail, strong: true)
                if !v.moved.isEmpty { apNote(v.moved.joined(separator: ", "), APColor.fail) }
            }
            if v.jira {
                let made = { (r: String, b: String) in (v.made[r] ?? []).contains(b) }
                let storyN = v.stories.flatMap(\.repos).filter { made($0.repo, $0.branch) }.count
                let taskN = v.stories.flatMap(\.tasks).filter { made($0.repo, $0.branch) }.count
                let spread = v.stories.filter { $0.made.count > 1 }.count
                apNote("새 가지: 스토리 가지 \(storyN) · 작업 가지 \(taskN) · 출발 가지 \(bases)" + (spread > 0 ? " · 여러 저장소에 걸친 스토리 \(spread)" : ""))
            } else if !v.run.work.isEmpty {
                Text(v.run.work).boardFont(11.5, weight: .semibold, design: .monospaced).lineLimit(1).truncationMode(.middle)
                apNote("새 가지" + (v.repos.count > 1 ? " · 저장소 \(v.repos.count)개" : "") + (m.spans.isEmpty ? "" : " · 커밋 \(commits)개") + " · 출발 가지 \(bases)")
            }
            if !v.createdOther.isEmpty { apNote("엔진이 만들지 않은 새 가지 \(v.createdOther.count)개: " + v.createdOther.joined(separator: ", ") + ". 엔진은 움직이지 않습니다.", APColor.limit) }
            if v.run.inPlace, !v.restored.isEmpty {
                let back = v.repos.filter { v.restored[$0] == true }
                if back.count == v.repos.count {
                    apNote("끝나서 원래 가지로 되돌렸습니다: " + v.repos.map { "\(($0 as NSString).lastPathComponent) · \(v.run.originals[$0] ?? "?")" }.joined(separator: "  ")
                           + ". 자율주행 커밋은 이 실행의 가지에 남아 있습니다.")
                } else {
                    ForEach(v.repos.filter { v.restored[$0] != true }, id: \.self) { r in
                        apNote("\((r as NSString).lastPathComponent): 원래 가지(\(v.run.originals[r] ?? "?"))로 되돌리지 않았습니다. " + (v.restoreNotes[r] ?? ""), APColor.limit)
                    }
                }
            }
        }
    }

    // ④⑤ 바뀐 파일 — 출발 가지 머리에서 자율주행 가지까지(저장소마다). 파일을 누르면 바뀐 줄
    private func filesBox(_ v: APMergeView) -> some View {
        APBox(title: "바뀐 파일", note: "출발 가지에서 자율주행 가지까지 · 파일을 누르면 바뀐 줄", axid: "ap-merge-files") {
            if m.spans.isEmpty, m.reading { apNote("읽는 중입니다") }
            ForEach(v.repos, id: \.self) { r in
                if let p = m.spans["all|" + r] {
                    diffPanel("all|" + r, repo: r, p, title: v.repos.count > 1 ? (r as NSString).lastPathComponent : nil, ax: "all-" + (r as NSString).lastPathComponent)
                }
            }
        }
    }

    // ⑥ 통과하지 못한 단위 — 그때까지 한 것은 자율주행 가지에 커밋되어 합치면 함께 간다(설계 판단: 와이어프레임의 "상한에서 멈춘 단위"를 미통과·중단까지)
    @ViewBuilder private func unpassedBox(_ v: APMergeView) -> some View {
        let bad = v.run.units.filter { ["failed", "capped", "stopped"].contains($0.status) }
        if !bad.isEmpty {
            APBox(title: "통과하지 못한 단위 \(bad.count)", tint: APColor.limit, axid: "ap-merge-unpassed") {
                ForEach(bad) { u in
                    HStack(spacing: 6) {
                        APChip(text: apUnitLabel(u.status), color: APColor.unit(u.status), strong: true)
                        Text(u.title).boardFont(11.5).lineLimit(1)
                        if let c = u.caps ?? v.run.unitCaps, u.status == "capped" { Text("\(Int(c.minutes))분 상한").boardFont(10.5).foregroundStyle(.secondary).fixedSize() }
                    }
                }
                apNote("그때까지 한 것은 자율주행 가지에 커밋되어 있어 합치면 함께 갑니다. 더 쪼갤지, 상한을 늘릴지, 다시 할지는 다음 출발 카드로 넘어갑니다.")
            }
        }
    }

    private func stepsBox(_ v: APMergeView, title: String, steps: [APMergeStep]) -> some View {
        APBox(title: title, note: "프로젝트 설정 '승격 차례'대로" + (v.repos.count > 1 ? ", 저장소마다 함께" : "") + ", 사람이 누를 때만", axid: "ap-merge-steps") {
            if steps.contains(where: { v.discarded.contains($0.source) }) {
                apNote("이 실행의 가지를 버렸습니다. 실행 기록과 보고서는 지난 실행에 남습니다.")
            } else if steps.isEmpty { apNote("승격할 가지(dev·live·master)가 없습니다", APColor.limit) }
            if !v.promotionSaved, !steps.isEmpty { apNote("프로젝트 설정에 승격 차례가 없어 저장소 가지로 채웠습니다. 엔진은 차례 없이 합치기는 하지만 기존 가지끼리 올리기는 거부합니다.", APColor.limit) }
            ForEach(steps.filter { !v.discarded.contains($0.source) }) { st in stepView(v, st) }
            if !steps.isEmpty, steps.allSatisfy({ m.done($0) != nil }) { apNote("모든 단계를 마쳤습니다.", APColor.ok) }
        }
    }

    // 단계 하나 — 머리(번호·받는 가지 ← 합칠 쪽·커밋 수·차례), 바뀐 줄 보기, 합치기 단추 또는 한 일, 저장소마다 미리 본 것
    private func stepView(_ v: APMergeView, _ st: APMergeStep) -> some View {
        let did = m.done(st)
        let lock = m.lockReason(st), shownLock = m.orderLock(st)
        let ps = m.plansOf(st)
        let total = ps.compactMap { $0.plan?.commits }.reduce(0, +)
        let many = ps.count > 1
        var info: [String] = []
        if !m.plans.isEmpty, did == nil { info.append("커밋 \(total)" + (many ? " · " + ps.map { "\(($0.repo as NSString).lastPathComponent) \($0.plan?.commits ?? 0)" }.joined(separator: " · ") : "개")) }
        if let a = st.after { info.append("\(a) 에 합친 뒤에만") }
        return VStack(alignment: .leading, spacing: 5) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(apCircled(st.num)).boardFont(13, weight: .bold).foregroundStyle(did != nil ? APColor.ok : (lock == nil ? Palette.accent : .secondary))
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(st.into) ← \(st.from)").boardFont(12, weight: .semibold, design: .monospaced).lineLimit(1).truncationMode(.middle)
                    if !info.isEmpty {
                        Text(info.joined(separator: " · "))
                            .boardFont(10.5).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 4)
                APButton(title: m.openDiff.contains(st.id) ? "바뀐 줄 닫기" : "바뀐 줄 보기", enabled: !m.plans.isEmpty, help: "이 단계에서 \(st.into) 로 갈 바뀐 줄", axid: "ap-merge-diff-\(st.ax)") { m.toggleDiff(st.id) }
                if did != nil {
                    APChip(text: st.promote ? "올림" : "합침", color: APColor.ok, strong: true)
                } else {
                    APButton(title: st.promote ? "올리기" : "합치기", primary: true, enabled: lock == nil, help: lock ?? "엔진 finish 로 \(st.into) 에 합칩니다", axid: "ap-merge-do-\(st.ax)") { m.press(st) }
                }
            }
            if let d = did {
                let howOf = { (p: APPromoted) in (["ff": "앞으로 감기", "merge": "병합 커밋", "same": "이미 들어 있어 그대로"][p.how] ?? p.how) + (p.how == "merge" ? " \(p.to.prefix(7))" : "") }
                let rows = m.doneRows(st)
                apNote("\(d.at.map(apClock) ?? "?") 에 \(st.promote ? "올렸습니다" : "합쳤습니다") · "
                       + (many && rows.count > 1 ? rows.map { "\(($0.repo as NSString).lastPathComponent) \(howOf($0))" }.joined(separator: " · ") : howOf(d)), APColor.ok)
            } else if let shownLock { apNote(shownLock, shownLock.hasSuffix("누를 수 있습니다.") || shownLock.contains("읽는 중") ? .secondary : APColor.limit) }
            // 지난번에 일부 저장소만 했다(엔진 branches.failed). 다시 누르면 한 저장소는 그대로로 넘어가고 남은 저장소만 한다
            let left = did == nil ? m.leftover(st) : []
            if !left.isEmpty {
                let doneNames = m.doneRows(st).filter { r in !left.contains { $0.repo == r.repo } }.map { ($0.repo as NSString).lastPathComponent }
                apNote("일부만 했습니다" + (doneNames.isEmpty ? "" : "(한 저장소: " + doneNames.joined(separator: "·") + ")") + ". 하지 못한 저장소: "
                       + left.map { "\(($0.repo as NSString).lastPathComponent)(\($0.why))" }.joined(separator: " · ") + ". 까닭을 고친 뒤 다시 누르면 남은 저장소만 합칩니다.", APColor.fail)
                    .accessibilityIdentifier("ap-merge-partial-\(st.ax)")
            }
            // 차례에 막힌 단계의 "그대로"는 앞 단계 뒤에 달라지므로 보이지 않는다
            ForEach(ps) { rp in
                if let p = rp.plan, did == nil, !(p.state == "same" && !m.afterDone(st)) { planLine(st, rp.repo, p, named: many) }
            }
            if did == nil, ps.contains(where: { ($0.plan?.withBase ?? 0) > 0 }) {
                let parts = ps.filter { ($0.plan?.withBase ?? 0) > 0 }
                apNote("출발 가지가 \(st.after ?? "dev") 라 \(st.after ?? "dev") 에만 있던 커밋 \(parts.map { $0.plan!.withBase }.reduce(0, +))개"
                       + (many ? "(" + parts.map { "\(($0.repo as NSString).lastPathComponent) \($0.plan!.withBase)" }.joined(separator: " · ") + ")" : "")
                       + "가 함께 갑니다. 경고만 합니다. '바뀐 줄 보기'에 그 커밋도 함께 보입니다.", APColor.limit)
            }
            if m.openDiff.contains(st.id) {
                ForEach(ps) { rp in
                    if let p = rp.plan { diffPanel(st.id + "|" + rp.repo, repo: rp.repo, p, title: many ? (rp.repo as NSString).lastPathComponent : nil, ax: st.ax + (many ? "-" + (rp.repo as NSString).lastPathComponent : "")) }
                }
            }
        }
        .padding(9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(did != nil ? 0.03 : 0.05)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-merge-step-\(st.ax)")
    }

    @ViewBuilder private func planLine(_ st: APMergeStep, _ r: String, _ p: APMergePlan, named: Bool) -> some View {
        let name = named ? (r as NSString).lastPathComponent + ": " : ""
        switch p.state {
        case "nosrc": apNote(name + "\(st.from) 가지가 없습니다(버렸거나 지워짐)", APColor.limit)
        case "nointo": apNote(name + "\(st.into) 가지가 없어 이 저장소는 건너뜁니다")
        case "same": apNote(name + "바뀐 것이 없어(이미 들어 있음) 엔진이 '그대로'로 적고 이 단계를 마친 것으로 칩니다")
        case "ff", "merge":
            VStack(alignment: .leading, spacing: 3) {
                apNote(name + (p.state == "ff" ? "앞으로 감기로 합칠 수 있습니다. \(st.into) 가 그대로라 커밋 \(p.commits)개를 그대로 얹습니다."
                                               : "병합 커밋으로 합칩니다. \(st.into) 에 이 가지에 없는 커밋 \(p.behind)개가 있어 앞으로 감기가 안 됩니다. 단위 커밋 \(p.commits)개는 하나로 뭉치지 않고 그대로 남깁니다."))
                if let w = p.wt {
                    if p.wtDirty { apNote("\(st.into) 가 걸린 작업 트리(\(apHome(w)))에 커밋하지 않은 변경이 있어 엔진이 합치지 않습니다. 커밋하거나 치운 뒤 '다시 보기'를 누릅니다.", APColor.fail) }
                    else { apNote("\(st.into) 가 걸린 작업 트리(\(apHome(w)))도 그 자리에서 앞으로 감기로 함께 바뀝니다.") }
                }
            }
        case "conflict":
            VStack(alignment: .leading, spacing: 3) {
                apNote(name + "충돌 \(p.conflicts.count)개 파일이라 합치지 않습니다: " + p.conflicts.prefix(6).joined(separator: " · "), APColor.fail)
                HStack(spacing: 6) {
                    APButton(title: "보드 탭에서 열기", icon: "terminal", help: "그 저장소를 보드 탭(셸)으로 열어 직접 풉니다. 이 시트는 닫힙니다", axid: "ap-merge-opentab-\(st.ax)") { m.openTab(r) }
                    apNote("그 저장소를 보드 탭으로 열어 직접 풉니다. 푼 뒤 합치기·승격을 다시 열면 이 단계를 다시 봅니다.")
                }
            }
        case "error": apNote(name + "합칠 tree 를 만들지 못했습니다: \(p.error)", APColor.fail)
        default: EmptyView()
        }
    }

    // 바뀐 파일 목록과 고른 파일의 바뀐 줄(와이어프레임 ④⑤)
    private func diffPanel(_ key: String, repo: String, _ p: APMergePlan, title: String?, ax: String) -> some View {
        let add = p.files.compactMap { Int($0.add) }.reduce(0, +), del = p.files.compactMap { Int($0.del) }.reduce(0, +)
        return VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                if let title { Text(title).boardFont(11, weight: .bold) }
                Text("파일 \(p.files.count)개 · 커밋 \(p.commits)개").boardFont(11, weight: .semibold)
                Text("+\(add)").boardFont(10.5, design: .monospaced).foregroundStyle(APColor.ok)
                Text("−\(del)").boardFont(10.5, design: .monospaced).foregroundStyle(APColor.fail)
            }
            if p.state == "nosrc" { apNote("가지가 없습니다(버렸거나 지워짐)") }
            else if p.files.isEmpty { apNote("바뀐 파일이 없습니다") }
            ForEach(Array(p.files.prefix(60).enumerated()), id: \.offset) { i, f in
                Button { m.pick(key, f.path, repo: repo, p) } label: {
                    HStack(spacing: 6) {
                        Text(f.path).boardFont(11, design: .monospaced).lineLimit(1).truncationMode(.middle)
                        Spacer(minLength: 4)
                        Text(f.add == "-" ? "바이너리" : "+\(f.add)").boardFont(10.5, design: .monospaced).foregroundStyle(APColor.ok).fixedSize()
                        if f.add != "-" { Text("−\(f.del)").boardFont(10.5, design: .monospaced).foregroundStyle(APColor.fail).fixedSize() }
                    }
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(RoundedRectangle(cornerRadius: 5).fill(Color.white.opacity(m.picked[key] == f.path ? 0.12 : 0.03)))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("ap-merge-file-\(ax)-\(i)")
            }
            if p.files.count > 60 { apNote("… 파일 \(p.files.count - 60)개 더") }
            if let f = m.picked[key] {
                VStack(alignment: .leading, spacing: 2) {
                    Text(f).boardFont(11, weight: .bold, design: .monospaced)
                    if let t = m.patches[key + "|" + f] { APPatch(text: t) } else { apNote("읽는 중입니다") }
                }
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("ap-merge-patch-\(ax)")
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.03)))
    }

    // ⑧ 버리기 — 어느 단계도 합치기 전에만, 화면 안 확인 뒤
    private func discardBox(_ v: APMergeView) -> some View {
        let lock = m.discardLock("autopilot")
        let commits = v.repos.compactMap { m.spans["all|" + $0]?.commits }.reduce(0, +)
        return APBox(title: "버리기", note: "어느 단계도 합치기 전에만", axid: "ap-merge-discard-box") {
            if m.confirm == "discard" {
                VStack(alignment: .leading, spacing: 6) {
                    Text(v.run.mode == "worktree" ? "가지와 작업 트리를 지웁니다." : "가지를 지웁니다.").boardFont(12, weight: .bold)
                    apNote((v.repos.count > 1 ? "저장소 \(v.repos.count)개의 " : "") + "\(v.run.work) 가지의 커밋 \(commits)개" + (v.run.mode == "worktree" ? "와 작업 트리가" : "가")
                           + " 사라지고 되돌릴 수 없습니다. " + (v.run.inPlace ? "원래 작업 트리는 이미 출발 때 가지로 돌아와 있습니다. " : "") + "실행 기록과 보고서는 지난 실행에 남습니다.")
                    HStack(spacing: 6) {
                        APButton(title: "지우기", tint: APColor.fail, enabled: lock == nil, axid: "ap-merge-discard-yes") { m.discard(nil) }
                        APButton(title: "취소", axid: "ap-merge-discard-no") { m.confirm = nil }
                    }
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 8).fill(APColor.fail.opacity(0.10)))
            } else {
                HStack(spacing: 8) {
                    APButton(title: "버리기", icon: "trash", tint: APColor.fail, enabled: lock == nil, help: lock ?? "가지(따로 작업 트리면 작업 트리도)를 지웁니다. 확인을 한 번 더 받습니다", axid: "ap-merge-discard") { m.confirm = "discard" }
                    apNote(lock ?? "자율주행 가지\(v.run.mode == "worktree" ? "와 작업 트리" : "")를 지웁니다. 실행 기록은 남습니다.")
                }
            }
        }
    }

    // Jira 단위만 있는 실행의 autopilot/<실행 id> — 엔진이 저장소마다 만들지만 커밋 없이 출발 가지 머리에 남는다. 남아 있으면 지울 길(엔진 discard)을 준다
    @ViewBuilder private func unusedAutopilot(_ v: APMergeView) -> some View {
        let left = v.repos.filter { m.existing[$0]?.contains(v.run.work) == true }
        let commits = left.compactMap { m.spans["all|" + $0]?.commits }.reduce(0, +)
        // 엔진 discard 는 실행의 따로 작업 트리를 강제로 지우므로, 작업 트리가 남아 있으면(스토리를 아직 합치거나 버리지 않음) 누르지 못하게 한다
        let wtLeft = v.run.worktreePath.map { FileManager.default.fileExists(atPath: $0) } ?? false
        if !v.run.work.isEmpty, !left.isEmpty, !v.discarded.contains("autopilot"), m.discardLock("autopilot") == nil {
            HStack(spacing: 8) {
                APButton(title: "이 가지 지우기", icon: "trash", enabled: commits == 0 && !wtLeft && m.busy == nil && !m.reading,
                         help: commits > 0 ? "커밋이 있어 여기서 지우지 않습니다" : wtLeft ? "작업 트리가 남아 있습니다. 스토리를 합치거나 버린 뒤 지웁니다" : "엔진 finish discard 로 지웁니다(커밋 없음)",
                         axid: "ap-merge-unused") { m.discard(nil) }
                apNote("\(v.run.work) 는 Jira 단위만 있는 실행이라 쓰이지 않았습니다(커밋 \(commits)개). 엔진이 만든 가지라 지워도 됩니다." + (wtLeft ? " 작업 트리가 남아 있어 스토리를 합치거나 버린 뒤 지웁니다." : ""))
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.03)))
        }
    }

    // 스토리 묶음(와이어프레임 ⑫⑬⑭) — 머리, ① dev ② live, 합치지 못한 작업, 스토리 버리기 또는 합친 가지 지우기
    private func storyBox(_ v: APMergeView, _ s: APMergeStory) -> some View {
        let merged = s.tasks.filter { $0.merge == "merged" }.count
        let left = s.tasks.filter { $0.merge != "merged" && $0.merge != "nothing" }
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                APTicket(key: s.key)
                if !s.kind.isEmpty { APChip(text: apKind(s.kind)) }
                Text(s.branch.isEmpty ? "스토리 가지 없음" : s.branch).boardFont(11, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                if s.many { APChip(text: "저장소 \(s.repos.count)") }
                Spacer(minLength: 4)
                Text("합친 작업 \(merged)/\(s.tasks.count)").boardFont(10.5).foregroundStyle(.secondary).fixedSize()
            }
            // 여러 저장소에 걸친 스토리 — 저장소마다 스토리 가지와 합친 작업(엔진은 단계마다 모든 저장소를 먼저 보고 함께 합친다)
            if s.many {
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(s.repos) { sr in
                        let have = m.existing[sr.repo]?.contains(sr.branch) == true
                        let tasksHere = s.tasks.filter { $0.repo == sr.repo }
                        HStack(spacing: 6) {
                            Text(sr.name).boardFont(11, weight: .semibold).frame(minWidth: 70, alignment: .leading)
                            Text(sr.made ? sr.branch : "스토리 가지 없음").boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                            Spacer(minLength: 4)
                            Text(!sr.made ? "작업이 시작하지 않아 합칠 것 없음" : "합친 작업 \(sr.merged)/\(tasksHere.count)" + (have || m.existing[sr.repo] == nil ? "" : " · 가지 지움"))
                                .boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("ap-merge-story-\(s.key)-repo-\(sr.name)")
                    }
                }
                .padding(.horizontal, 8).padding(.vertical, 6)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.03)))
            }
            if !v.discarded.contains("story:" + s.key) {
                ForEach(m.storySteps(v, s)) { st in stepView(v, st) }
            }
            if !left.isEmpty, !v.discarded.contains("story:" + s.key) {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text("합치지 못한 작업 \(left.count)").boardFont(11.5, weight: .bold)
                        apNote("스토리 가지에 들어가지 않았습니다")
                    }
                    ForEach(left) { t in taskRow(s, t) }
                    apNote("이 티켓들은 열린 채 남고, 이 실행을 이어 가면 같은 티켓으로 다시 돕니다. 이어 가려면 먼저 이 스토리를 합치거나 버려 남은 가지를 치웁니다.")
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 8).fill(APColor.limit.opacity(0.07)))
            }
            storyFoot(v, s)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.white.opacity(0.04)))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(APColor.jira.opacity(0.22), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-merge-story-\(s.key)")
    }

    private func taskRow(_ s: APMergeStory, _ t: APMergeTask) -> some View {
        let other = m.foreign.contains(t.repo + "|" + t.branch)
        let has = !t.branch.isEmpty && m.existing[t.repo]?.contains(t.branch) == true && !other
        let key = t.spanKey
        let p = m.spans[key]
        return VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                if let k = t.key { APTicket(key: k) }
                if s.many, !t.repo.isEmpty { APChip(text: (t.repo as NSString).lastPathComponent) }
                Text(t.title).boardFont(11.5).lineLimit(1)
                APChip(text: apUnitLabel(t.status), color: APColor.unit(t.status), strong: true)
                Spacer(minLength: 4)
                if has, p != nil { APButton(title: m.openDiff.contains(key) ? "바뀐 줄 닫기" : "바뀐 줄 보기", axid: "ap-merge-task-\(t.key ?? "\(t.n)")") { m.toggleDiff(key) } }
            }
            if t.status == "skipped" {
                apNote((t.depends.isEmpty ? "시작하지 않음" : "\(t.depends.joined(separator: ", ")) 에 기대어 시작하지 않음") + (has ? " · \(t.branch)" : " · 가지 없음"))
            } else if has {
                apNote("\(t.branch) · 커밋 \(p?.commits ?? 0)" + (t.why.isEmpty || t.why == apUnitLabel(t.status) ? "" : " · " + t.why))
            } else if other {
                apNote("\(t.branch) · 이 실행이 끝난 뒤 같은 이름으로 다시 만들어진 가지(이 실행에서 움직이지 않음)")
            } else {
                apNote(t.branch.isEmpty ? "가지 없음" : "\(t.branch) · 지웠음")
            }
            if has, let p, m.openDiff.contains(key) { diffPanel(key, repo: t.repo, p, title: nil, ax: "task-\(t.key ?? "\(t.n)")") }
        }
    }

    // 스토리 버리기(dev 에 합치기 전에만) 또는 합친 가지 지우기(스토리가 live 에 합쳐진 뒤, 누를 때만)
    @ViewBuilder private func storyFoot(_ v: APMergeView, _ s: APMergeStory) -> some View {
        let source = "story:" + s.key
        let target = v.promotionSaved ? v.lastAuto : v.promoted.last(where: { $0.source == source })?.into
        let live = target.map { m.reached(source, $0) } ?? false
        // 저장소마다 스토리 가지와 그 저장소의 작업 가지(여러 저장소에 걸친 스토리면 저장소 이름을 붙여 보인다)
        let pairs = s.pairs
        let label = { (p: (repo: String, branch: String)) in s.many ? "\((p.repo as NSString).lastPathComponent) \(p.branch)" : p.branch }
        let isStory = { (p: (repo: String, branch: String)) in p.branch == s.storyBranch(p.repo) }
        let mine = pairs.filter { m.existing[$0.repo]?.contains($0.branch) == true && (v.made[$0.repo] ?? []).contains($0.branch) && !m.foreign.contains($0.repo + "|" + $0.branch) }
        let others = pairs.filter { m.foreign.contains($0.repo + "|" + $0.branch) }
        if !others.isEmpty {
            apNote("같은 이름의 가지 \(others.count)개(\(others.map(label).joined(separator: ", ")))는 이 실행이 끝난 뒤 다시 만들어진 것이라 이 실행에서 움직이지 않습니다. 이어 간 실행의 합치기·승격에서 다룹니다.", APColor.limit)
        }
        let mergedLeft = mine.filter { m.contained.contains($0.repo + "|" + $0.branch) }
        let unmergedLeft = mine.filter { !m.contained.contains($0.repo + "|" + $0.branch) && !isStory($0) }
        let unmergedCommits = unmergedLeft.compactMap { p in m.spans["task|" + p.repo + "|" + p.branch]?.commits }.reduce(0, +)
        let storyCount = mine.filter(isStory).count
        let where_ = s.made.count > 1 ? "저장소 \(s.made.count)곳의 " : ""
        if v.discarded.contains(source) {
            apNote("이 스토리를 버렸습니다(스토리 가지와 작업 가지를 지움).")
        } else if !m.reached(source, nil), mine.isEmpty {
            apNote("이 실행이 만든 이 스토리의 가지가 남아 있지 않습니다.")
        } else if !m.reached(source, nil) {
            if m.confirm == "discard:" + s.key {
                VStack(alignment: .leading, spacing: 6) {
                    Text("스토리 \(s.key) 의 가지를 지웁니다.").boardFont(12, weight: .bold)
                    apNote(where_ + "스토리 가지\(storyCount > 1 ? " \(storyCount)개" : "")와 작업 가지 \(mine.count - storyCount)개(합치지 못한 작업 가지 포함)가 사라지고 되돌릴 수 없습니다. 실행 기록은 지난 실행에 남습니다.")
                    HStack(spacing: 6) {
                        APButton(title: "지우기", tint: APColor.fail, enabled: m.discardLock(source) == nil, axid: "ap-merge-discard-\(s.key)-yes") { m.discard(s.key) }
                        APButton(title: "취소", axid: "ap-merge-discard-\(s.key)-no") { m.confirm = nil }
                    }
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 8).fill(APColor.fail.opacity(0.10)))
            } else {
                HStack(spacing: 8) {
                    APButton(title: "이 스토리 버리기", icon: "trash", tint: APColor.fail, enabled: m.discardLock(source) == nil, help: m.discardLock(source) ?? "확인을 한 번 더 받습니다", axid: "ap-merge-discard-\(s.key)") { m.confirm = "discard:" + s.key }
                    apNote("dev 에 합치기 전에만 · " + where_ + "스토리 가지\(storyCount > 1 ? " \(storyCount)개" : "")와 작업 가지 \(mine.count - storyCount)개를 지웁니다")
                }
            }
        } else if !live {
            apNote("\(v.promoted.first { $0.source == source }?.into ?? "dev") 에 합쳐서 이 스토리는 버릴 수 없습니다. \(target ?? "live") 에 합치면 합친 가지를 지울 수 있습니다.")
        } else if mine.isEmpty {
            apNote("\(target ?? "live") 에 합쳤고 이 스토리의 가지를 모두 지웠습니다.", APColor.ok)
        } else {
            VStack(alignment: .leading, spacing: 6) {
                if !mergedLeft.isEmpty {
                    HStack(spacing: 8) {
                        APButton(title: "합친 가지 지우기", icon: "scissors", enabled: m.busy == nil && !m.reading, help: "엔진 finish cleanup: \(target ?? "live") 에 다 들어간 가지만 지웁니다", axid: "ap-merge-cleanup-\(s.key)") { m.cleanup(s.key, unmerged: false) }
                        let ms = mergedLeft.filter(isStory).count
                        apNote("\(target ?? "live") 에 합쳤습니다. 누를 때만 " + where_ + (ms > 0 ? "스토리 가지\(ms > 1 ? " \(ms)개" : "")와 " : "") + "작업 가지 \(mergedLeft.count - ms)개를 지웁니다.")
                    }
                }
                if !unmergedLeft.isEmpty {
                    if m.confirm == "unmerged:" + s.key {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("합치지 못한 작업 가지 \(unmergedLeft.count)개도 지웁니다.").boardFont(12, weight: .bold)
                            apNote(unmergedLeft.map(label).joined(separator: ", ") + " 의 커밋 \(unmergedCommits)개가 사라지고 되돌릴 수 없습니다. 이 실행을 이어 가면 같은 티켓으로 스토리 가지를 새로 만들어 다시 합니다.")
                            HStack(spacing: 6) {
                                APButton(title: "지우기", tint: APColor.fail, enabled: m.busy == nil, axid: "ap-merge-unmerged-\(s.key)-yes") { m.cleanup(s.key, unmerged: true) }
                                APButton(title: "취소", axid: "ap-merge-unmerged-\(s.key)-no") { m.confirm = nil }
                            }
                        }
                        .padding(8)
                        .background(RoundedRectangle(cornerRadius: 8).fill(APColor.fail.opacity(0.10)))
                    } else {
                        HStack(spacing: 8) {
                            APButton(title: "합치지 못한 작업 가지도 지우기", icon: "trash", tint: APColor.fail, enabled: m.busy == nil && !m.reading, help: "확인을 한 번 더 받습니다", axid: "ap-merge-unmerged-\(s.key)") { m.confirm = "unmerged:" + s.key }
                            apNote("남은 작업 가지 \(unmergedLeft.count)개(커밋 \(unmergedCommits)개)는 \(target ?? "live") 에 없습니다. 이어 가려면 지워야 엔진이 같은 티켓으로 출발합니다.")
                        }
                    }
                }
            }
        }
    }
}

// 바뀐 줄(diff) — 더한 줄 초록, 뺀 줄 빨강, 조각 머리 강조. 글자 하나로 그린다(줄마다 보기를 만들지 않음)
struct APPatch: View {
    let text: String
    var body: some View {
        var a = AttributedString()
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            var piece = AttributedString(String(line) + "\n")
            if ["+++", "---", "diff ", "index ", "new file", "deleted file", "old mode", "new mode", "similarity", "rename ", "Binary"].contains(where: { line.hasPrefix($0) }) { piece.foregroundColor = .secondary }
            else if line.hasPrefix("+") { piece.foregroundColor = APColor.ok }
            else if line.hasPrefix("-") { piece.foregroundColor = APColor.fail }
            else if line.hasPrefix("@@") { piece.foregroundColor = Palette.accent }
            a += piece
        }
        return Text(a).boardFont(10.5, design: .monospaced).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
    }
}
