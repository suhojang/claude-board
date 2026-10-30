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
// 읽어 그리고, 사람이 누른 일시정지·재개·중단·멈추고 다시 출발은 control.json 으로, 방향을 바꿀 말은 inbox/<시각>.md 로 남긴다(2단계 5-4) —
// 실행 폴더에 보드가 쓰는 것은 이 둘이고 끝난 실행에는 쓰지 않는다(엔진 control 명령과 같은 규칙).
// 엔진 전용 자리(private/<실행 id>/, runs/*/engine/, pinned/)는 읽지 않는다

// 자율주행 뿌리 — 엔진의 autopilotRoot 와 같은 자리. 시험 빌드(번들 id com.claude.board.dev)는 Claude Board Dev 뿌리라
// 설치판의 실제 실행과 섞이지 않는다(엔진도 --dev 출발은 그 뿌리를 쓴다). 맨 위 코드의 전역 값(instanceName)보다 먼저 불릴 수 있어 계산 값으로 둔다
var autopilotRoot: String {
    NSHomeDirectory() + "/Library/Application Support/"
        + (Bundle.main.bundleIdentifier == "com.claude.board.dev" ? "Claude Board Dev" : "Claude Board") + "/autopilot"
}

func apRead(_ path: String) -> Any? {
    FileManager.default.contents(atPath: path).flatMap { try? JSONSerialization.jsonObject(with: $0) }
}
func apNum(_ v: Any?) -> Double? { (v as? NSNumber)?.doubleValue }
// 실제 경로(바로 가기를 풂, 엔진 canonical 과 같음). 없는 경로는 글자 그대로
func apRealPath(_ p: String) -> String {
    guard let r = realpath((p as NSString).expandingTildeInPath, nil) else { return p }
    defer { free(r) }
    return String(cString: r)
}
func apStamp(_ path: String) -> Double { mtime(path)?.timeIntervalSince1970 ?? 0 }
func apStrings(_ v: Any?) -> [String] { (v as? [Any] ?? []).map { "\($0)" } }
func apText(_ v: Any?) -> String? { (v as? String).flatMap { $0.isEmpty ? nil : $0 } }

// 모델이 쓴 글의 긴 줄표 바꾸기 apPlain 은 규칙 AutopilotRules(Prose.swift)에 있다 — 보고서 틀과 함께 쓴다(3단계)

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

// 멈추고 다시 출발을 엔진이 받은 표지(run.json restart): 받은 시각, 마친 뒤 멈출 단위(없으면 바로), 멈춘 시각
struct APRestartMark: Equatable { let asked: Double; let unit: Int?; let stopped: Double? }
// 보드가 inbox/ 에 쓴 말 — 엔진이 받기 전까지 자율주행 중 화면에 "엔진이 받기를 기다립니다"로 보인다
struct APSaid: Equatable { let file: String; let to: String; let text: String; let at: Double }

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
    var stashes: [APStash] = []         // 읽기 전용 자리에 남은 변경을 엔진이 보관한 것(branches.stashed, 5-3b ①)
    var inbox: [APInboxEntry] = []      // 방향을 바꿀 말 — 엔진이 받은 것과 어떻게 됐는지(run.json inbox, 2단계 5-4)
    var restart: APRestartMark?         // 멈추고 다시 출발 — 엔진이 받은 것(run.json restart, 5-4)

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
        if let a = r["inbox"] as? [Any], let d = try? JSONSerialization.data(withJSONObject: a) {
            x.inbox = (try? JSONDecoder().decode([APInboxEntry].self, from: d)) ?? []
        }
        if let rs = r["restart"] as? [String: Any] {
            x.restart = APRestartMark(asked: apNum(rs["asked"]) ?? 0, unit: apNum(rs["unit"]).map { Int($0) }, stopped: apNum(rs["stopped"]))
        }
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
        x.stashes = apStashes(br)
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

// 읽기 전용으로 걸어 둔 자리(같은 스토리의 다른 저장소 작업 동안 스토리 가지)에 모델이 남긴 변경을 엔진이 따로 보관한 것(엔진 branches.stashed,
// 2026-10-29 5-3b ①) — 자율주행 중·지난 실행에 한 줄씩. 사람이 그 저장소에서 git stash list 로 찾아 쓸지 정한다
struct APStash: Equatable, Identifiable {
    let sha: String
    var id: String { sha }
    var unit: Int?, repo = "", branch = "", files = 0
}
func apStashes(_ br: [String: Any]) -> [APStash] {
    (br["stashed"] as? [[String: Any]] ?? []).compactMap { x in
        guard let sha = x["stash"] as? String, !sha.isEmpty else { return nil }
        return APStash(sha: sha, unit: apNum(x["unit"]).map { Int($0) }, repo: ((x["repo"] as? String ?? "") as NSString).lastPathComponent,
                       branch: x["branch"] as? String ?? "", files: Int(apNum(x["files"]) ?? 0))
    }
}
struct APStashNote: View {
    let stashes: [APStash]
    var body: some View {
        if !stashes.isEmpty {
            VStack(alignment: .leading, spacing: 2) {
                Label("읽기 전용으로 걸어 둔 자리에 남은 변경을 엔진이 따로 보관했습니다(판정·다음 단위에 넣지 않음). 그 저장소에서 git stash list 로 봅니다.",
                      systemImage: "tray.and.arrow.down")
                    .boardFont(11).foregroundStyle(APColor.limit).fixedSize(horizontal: false, vertical: true)
                ForEach(stashes) { x in
                    Text("단위 \(x.unit.map(String.init) ?? "?") · \(x.repo) \(x.branch) · 변경 \(x.files)개 · git stash \(x.sha.prefix(7))")
                        .boardFont(10, design: .monospaced).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.leading, 18)
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("ap-stashed")
        }
    }
}

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

// 보드가 쓰는 inbox/ 의 진짜 어댑터(규칙 apInboxSend 의 포트 APInboxOut) — 임시 파일에 쓰고 이름을 바꿔 엔진이 반쯤 쓴 파일을 읽지 않게
struct APInboxBoardOut: APInboxOut {
    let dir: String
    func exists(_ name: String) -> Bool { FileManager.default.fileExists(atPath: dir + "/" + name) }
    func write(_ name: String, _ data: Data) -> Bool {
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let tmp = dir + "/.tmp-\(getpid())-" + name
        guard FileManager.default.createFile(atPath: tmp, contents: data), rename(tmp, dir + "/" + name) == 0 else { unlink(tmp); return false }
        return true
    }
}
let apInboxStamp = formatter("yyyyMMdd-HHmmss-SSS")

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
    @Published private(set) var said: [String: [APSaid]] = [:]   // 보낸 말 가운데 엔진이 아직 받지 않은 것(실행 id → 차례대로, 5-4)
    @Published private(set) var sayError: [String: String] = [:]
    // 이 보드가 멈추고 다시 출발을 보낸 실행 — 끝 정리까지 마치면 한 번 출발 준비를 저절로 연다(규칙 apReopenIntake, 5-4). 보드를 끄면 잊고 화면의 단추로 연다
    private var restartAsked: Set<String> = [], reopened: Set<String> = []
    @Published private(set) var history = APHistory()
    @Published private(set) var detail: APPastDetail?
    // 지난 실행에서 거른 프로젝트(nil 이면 모든 프로젝트)와 상세에 보이는 실행 — 상세는 거른 목록 안에서 고른다(규칙 apHistoryPick, 5-3b ⑥)
    @Published var historyProject: String? { didSet { if historyProject != oldValue { tick() } } }
    @Published private(set) var historyShown: String?
    @Published private(set) var awaiting: [APRun] = []    // 끝났고 합치기·승격을 기다리는 실행(최근 것부터) — 보드 본체의 끝난 실행 줄
    @Published var screen: APScreen = .board { didSet { if screen != oldValue { tick() } } }
    private var cache: [String: (stamp: [Double], run: APRun?)] = [:]
    // 합치기·승격의 "나중에" — 실행 id → 다시 보일 시각. 하루 동안 보드 본체의 끝난 실행 줄에서 숨기고, 하루 뒤 한 번 다시 알린다(4단계 ⑩-3, APNotices)
    private var later: [String: Double] = UserDefaults.standard.dictionary(forKey: "autopilotMergeLater") as? [String: Double] ?? [:]
    func snooze(_ id: String) {
        let now = Date().timeIntervalSince1970
        let reminded = APNotices.shared.remindedIDs
        later = later.filter { $0.value > now || !reminded.contains($0.key) }   // 아직 다시 알리지 않은 것은 남긴다
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
        // 엔진이 받은 말은 보낸 말 목록에서 뺀다(run.json inbox 에 같은 파일 이름)
        var pruned: [String: [APSaid]] = [:]
        for (id, list) in said {
            let r = live.first { $0.id == id } ?? (view?.id == id ? view : nil) ?? cache.values.compactMap(\.run).first { $0.id == id }
            let got = Set(r?.inbox.map(\.file) ?? [])
            let left = list.filter { !got.contains($0.file) }
            if !left.isEmpty { pruned[id] = left }
        }
        if pruned != said { said = pruned }
        // 멈추고 다시 출발로 멈춘 실행 — 끝 정리를 마치면 이 실행을 이어 가는 출발 준비를 한 번 연다. 다른 출발 준비가 열려 있으면 그것을 버리지 않고 단추로 남긴다
        for id in restartAsked where !reopened.contains(id) {
            guard let r = cache.values.compactMap(\.run).first(where: { $0.id == id }) else { continue }
            guard apReopenIntake(state: r.state, wrapped: r.wrapped, flag: r.restart != nil, asked: true, opened: false, project: r.project) else { continue }
            reopened.insert(id)
            let s = APStart.shared
            if !s.shown, !s.preparing, !s.starting, s.source != "topic", !s.isImport, !s.hasDraft || s.pastId == id {
                DispatchQueue.main.async { MainActor.assumeIsolated { APStart.shared.open(continuing: id) } }
            }
        }
        if let v = viewing { readEvents(base + "/" + v) } else if !evDir.isEmpty { evDir = ""; evOffset = 0; evRest = Data(); events = [] }
        if case .history(let sel) = screen { readHistory(sel) }
        APNotices.shared.check(cache.values.compactMap(\.run), later: later)   // 끝 알림·나중에 뒤 다시 알림(4단계 ⑩-3)
    }
    // 실행 하나(끝난 것 포함) — 끝 알림을 누를 때
    func anyRun(_ id: String) -> APRun? {
        cache.values.compactMap(\.run).first { $0.id == id } ?? APRun.parse(id: id, dir: autopilotRoot + "/runs/" + id)
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
            var h = APRecords.list(db)
            let purged = apPurgeRecords()   // 원문을 지운 실행(5-2b) — 기록 저장소는 그대로라 보드가 쓴 지운 기록을 붙인다
            for i in h.runs.indices { h.runs[i].purged = purged[h.runs[i].id] }
            history = h
            detailKey = ""
        }
        // 상세에 보일 실행은 규칙 apHistoryPick(AutopilotRules/History.swift) — 거른 프로젝트 안에서
        let pick = apHistoryPick(selected: sel, runs: history.runs.map { APRunTag(id: $0.id, project: $0.projectName) }, project: historyProject)
        if pick != historyShown { historyShown = pick }
        let id = pick ?? ""
        guard id != detailKey else { return }
        detailKey = id
        detail = id.isEmpty ? nil : history.runs.first { $0.id == id }.flatMap { APRecords.detail(db, $0) }
    }

    // 원문 지우기(2026-10-22 사용자 결정: 자동으로 지우지 않고 지난 실행 화면에서 사람이, 2단계 5-2b) — 규칙 apPurgeCheck 의 입력.
    // 끝 정리·합치기 기다림은 실행 폴더의 run.json 으로(APRun.parse), 따로 작업 트리는 그 자리가 있는지, 출발 준비가 이어 가는 중인지
    func purgeInput(_ r: APPastRun) -> APPurgeInput {
        let parsed = APRun.parse(id: r.id, dir: r.runDir)
        return APPurgeInput(run: r.id, root: autopilotRoot, runDir: r.runDir, state: parsed?.state ?? r.state, wrapped: parsed?.wrapped ?? false,
                            mergeWaiting: parsed?.mergeWaiting ?? false, worktreeLeft: FileManager.default.fileExists(atPath: NSHomeDirectory() + "/.claude-board/worktrees/" + r.id),
                            inDraft: APStart.shared.pastId == r.id, exists: FileManager.default.fileExists(atPath: r.runDir))
    }
    @Published private(set) var purging: String?
    @Published private(set) var purgeError: [String: String] = [:]
    func purge(_ r: APPastRun) {
        guard purging == nil else { return }
        let input = purgeInput(r)
        purging = r.id
        purgeError[r.id] = nil
        DispatchQueue.global(qos: .userInitiated).async {
            let x = apPurgeRaw(input, files: APRawRunFiles(), clock: APSystemClock())
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.purging = nil
                    if let e = x.error { self.purgeError[r.id] = e }
                    if let rec = x.record { boardLog("자율주행 \(r.id): 원문 지움 \(rec.path) 파일 \(rec.files)개 \(rec.bytes)바이트" + (rec.error.map { " · 오류 \($0)" } ?? "")) }
                    self.cache[r.runDir] = nil
                    self.dbStamp = []   // 지운 기록을 붙여 목록을 다시 읽는다
                    self.tick()
                }
            }
        }
    }

    // 경로 카드에 들어갈 실행
    func runs(at path: String) -> [APRun] { runs.filter { $0.covers(path) } }
    // 원래 작업 트리로 그 경로를 쓰는 실행 — 보드 탭 새 세션 경고
    func inPlaceRun(covering dir: String) -> APRun? { runs.first { $0.inPlace && $0.covers(dir) } }
    // 자율주행 보기의 사용량 상자 멈출 선(%, UsagePanel 의 한도 id 로)
    // 실행마다 멈출 선이 따로라(6단계 ⑫-6 설계 판단: 출발 때 고정한 실행의 선) 가장 먼저 걸리는 선, 곧 가장 낮은 선을 보인다
    var stoplines: [String: Double]? {
        guard !runs.isEmpty else { return nil }
        return ["5h": (runs.map(\.stop5).min() ?? 0.95) * 100, "7d": (runs.map(\.stop7).min() ?? 0.80) * 100]
    }

    // 일시정지·재개·중단 — control.json 을 엔진 control 명령과 같은 꼴({"action","ts"}, 임시 파일을 바꿔 넣음)로 쓴다.
    // 쓰기 직전에 run.json 을 다시 읽어 끝난 실행이면 쓰지 않는다
    func send(_ action: String, _ run: APRun) {
        guard sent[run.id] == nil else { return }
        // 아는 요청이고 출발한·끝나지 않은 실행인지는 규칙 apBoardCanControl(AutopilotRules/Control.swift)
        let state = (apRead(run.dir + "/run.json") as? [String: Any])?["state"] as? String ?? ""
        guard apBoardCanControl(action, state: state) else { return }
        let ts = (Date().timeIntervalSince1970 * 1000).rounded() / 1000
        guard let data = try? JSONSerialization.data(withJSONObject: ["action": action, "ts": ts], options: [.prettyPrinted, .sortedKeys]) else { return }
        let path = run.dir + "/control.json", tmp = path + ".tmp-\(getpid())"
        guard FileManager.default.createFile(atPath: tmp, contents: data + Data("\n".utf8)), rename(tmp, path) == 0 else {
            unlink(tmp)
            boardLog("자율주행 \(run.id): control.json 을 쓰지 못했습니다(\(action))")
            return
        }
        sent[run.id] = APSent(action: action, ts: ts)
        if action == "restart_from_intake" { restartAsked.insert(run.id) }
    }

    // 방향을 바꿀 말 보내기(2단계 5-4, 와이어프레임 자율주행 중 ③ "한마디") — 규칙 apInboxSend 가 확인하고 inbox/<yyyyMMdd-HHmmss-SSS>.md 로 쓴다.
    // "지금 단위에"는 지금 도는 단위 번호를 머리에 적는다(엔진이 받을 때 그 단위가 끝났으면 다른 단위에 넘기지 않게). 되면 true
    func say(_ text: String, to: APInboxTo, _ run: APRun) -> Bool {
        let state = (apRead(run.dir + "/run.json") as? [String: Any])?["state"] as? String ?? ""
        let unit = to == .now ? run.units.first { $0.status == "running" }?.n : nil
        let now = Date()
        let at = (now.timeIntervalSince1970 * 1000).rounded() / 1000
        switch apInboxSend(APInboxNote(to: to, unit: unit, at: at, text: text), runState: state, stamp: apInboxStamp.string(from: now),
                           out: APInboxBoardOut(dir: run.dir + "/inbox")) {
        case .sent(let name):
            said[run.id, default: []].append(APSaid(file: name, to: to.rawValue, text: text.trimmingCharacters(in: .whitespacesAndNewlines), at: at))
            sayError[run.id] = nil
            return true
        case .refused(let why):
            sayError[run.id] = why
            boardLog("자율주행 \(run.id): 말을 보내지 못했습니다(\(why))")
            return false
        }
    }

    // 진행 기록을 단위별로 묶는다. events.jsonl 에는 단위 번호가 없어 단위의 시작·끝 시각(engine.json)으로 가른다:
    // 단위 창(시작 ~ 끝 + 1초) 안이면 그 단위, 다음 단위 시작 3초 앞의 보드 줄(스토리·작업 가지 만들기)이면 다음 단위, 그 밖의 틈(멈출 선·환경 확인 대기)은
    // 앞 단위. 첫 단위 앞은 출발(0), 실행이 끝났다는 줄부터는 끝(-1)
    static func groups(_ run: APRun, _ events: [APEvent]) -> [APLogGroup] {
        let starts = run.units.compactMap { u in u.started.map { (u.n, $0, u.ended) } }.sorted { $0.1 < $1.1 }
        let finishes = ["자율주행을 마쳤습니다", "사람이 중단해 실행을 마칩니다", "엔진 오류로 멈춥니다", "고정한 실행 설정이 출발 때와 달라",
                        "멈추고 다시 출발: 사람이 요청해 여기서 멈춥니다"]
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
    var stashes: [APStash] = []                         // 읽기 전용 자리에 남은 변경을 엔진이 보관한 것(5-3b ①)
    var epic: String?
    var storyRepos: [String: [APPastStoryRepo]] = [:]   // 스토리 키 → 저장소마다 스토리 가지·합친 작업 수·마지막으로 받은 가지(기록 저장소 runs.branches)
    var root: String?                                   // 프로젝트의 작업 경로(기록 저장소 projects.root) — 경로 카드에서 연 출발 준비가 이 경로의 실행을 찾는다(5-2b)
    var setId: String?                                  // 이 실행이 쓴 자율주행 세트(기록 저장소 runs.set_id)
    var purged: APPurgeRecord?                          // 원문을 지운 기록(자율주행 뿌리 purged/<실행 id>.json, 5-2b)
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
        case .mixed(_, _, let w): return (m.label, w > 0 ? APColor.wait : APColor.ok)
        }
    }
    var stateLabel: (String, Color)? {
        switch state {
        case "stopped": apIsRestartStop(state: state, reason: reason) ? ("멈추고 다시 출발", APColor.stop) : ("중단됨", APColor.muted)
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
    var evidence: APEvidence?      // 마지막 판정의 증거(5단계 ⑪-1) — 실행 폴더의 units/<n>/engine.json 이 남아 있을 때만
}

// 판정 증거(5단계 ⑪-1 시험 실행기): 마지막 판정의 증거 폴더와 걸음(시나리오·판정 명령)마다 종료 코드·시간·남긴 파일
struct APEvidence: Equatable {
    struct Part: Equatable { var label = "", exit = 0, sec = 0.0, passed = false, files: [String] = [], shots: [String] = [], note = "" }
    var dir = ""          // 실행 폴더 안의 증거 폴더(절대 경로)
    var tryN = 0, exit = 0, sandbox = false
    var parts: [Part] = []
    // engine.json 의 checks 마지막 줄. 증거 폴더가 그 실행 폴더 안(링크 아님)일 때만
    static func load(runDir: String, n: Int) -> APEvidence? {
        guard !runDir.isEmpty, let e = apRead(runDir + "/units/\(n)/engine.json") as? [String: Any],
              let c = (e["checks"] as? [[String: Any]])?.last, let rel = c["evidence"] as? String, !rel.contains("..") else { return nil }
        let dir = runDir + "/units/\(n)/" + rel
        let real = URL(fileURLWithPath: dir).resolvingSymlinksInPath().path, base = URL(fileURLWithPath: runDir).resolvingSymlinksInPath().path
        guard real.hasPrefix(base + "/") else { return nil }
        var ev = APEvidence(dir: dir, tryN: Int(apNum(c["try"]) ?? 0), exit: Int(apNum(c["exit"]) ?? 0), sandbox: c["sandbox"] as? Bool ?? false)
        let parts = c["parts"] as? [[String: Any]] ?? []
        if parts.isEmpty {
            ev.parts = [Part(label: "판정 명령", exit: ev.exit, sec: apNum(c["sec"]) ?? 0, passed: ev.exit == 0, files: apStrings(c["files"]))]
        } else {
            ev.parts = parts.map { p in Part(label: p["label"] as? String ?? "", exit: Int(apNum(p["exit"]) ?? 0), sec: apNum(p["sec"]) ?? 0,
                                             passed: p["passed"] as? Bool ?? false, files: apStrings(p["files"]), shots: apStrings(p["screenshots"]),
                                             note: p["note"] as? String ?? "") }
        }
        return ev
    }
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
    static let knownVersion = Int(apRecordsVersion)   // 판 2(5단계 ⑪-3 판정 놓침 표)

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
              (SELECT COUNT(DISTINCT u.story_key) FROM units u WHERE u.run_id = r.id AND u.story_key IS NOT NULL),
              p.root, r.set_id
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
            r.skipped = Int(c[21] as? Double ?? 0)
            r.jiraUnits = Int(c[22] as? Double ?? 0)
            r.stories = Int(c[23] as? Double ?? 0)
            if let b = json(c[17]) as? [String: Any] {
                r.hasBranches = !((b["repos"] as? [String: Any]) ?? [:]).isEmpty
                // 합칠 쪽마다(Jira 면 스토리마다) 정한다. 자율주행 가지는 Jira 실행이면 스토리 없는 단위가 있을 때만 합칠 쪽(보드 본체 줄과 같은 기준)
                r.merge = apMergeMark(b, stories: r.jiraUnits == 0 || r.jiraUnits < r.total)
                r.stashes = apStashes(b)
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
            r.root = c[24] as? String
            r.setId = c[25] as? String
            h.runs.append(r)
        }
        h.error = err
        return h
    }

    // 작업 경로(저장소 뿌리)의 프로젝트와 그 실행 수(5-2b 주제로 시작의 기존 저장소)
    static func project(_ path: String, root: String) -> (id: String, runs: Int)? {
        guard let db = open(path) else { return nil }
        defer { sqlite3_close_v2(db) }
        var err: String?
        guard let c = rows(db, "SELECT p.id, (SELECT COUNT(*) FROM runs r WHERE r.project_id = p.id) FROM projects p WHERE p.root = ?", [root], error: &err).first,
              let id = c[0] as? String else { return nil }
        return (id, Int(c[1] as? Double ?? 0))
    }

    // 지난 판 자율주행 지시 — 그 프로젝트에서 가장 최근에 출발한 실행이 쓴 세트(sets.instruction, 엔진이 출발 때 앞뒤를 잘라 남김). 5-2b 지시 차이
    static func lastSet(_ path: String, project: String) -> (text: String, at: Double, run: String)? {
        guard let db = open(path) else { return nil }
        defer { sqlite3_close_v2(db) }
        var err: String?
        let sql = """
            SELECT s.instruction, COALESCE(r.started, s.created), r.id FROM runs r JOIN sets s ON s.id = r.set_id
            WHERE r.project_id = ? AND s.instruction IS NOT NULL ORDER BY COALESCE(r.started, r.updated) DESC LIMIT 1
            """
        guard let c = rows(db, sql, [project], error: &err).first, let t = c[0] as? String else { return nil }
        return (t, c[1] as? Double ?? 0, c[2] as? String ?? "")
    }

    // 그 프로젝트의 미룬 몫(next_start) 모두와 카드 답(정함·닫음이 하나라도 있으면 그것, 아니면 마지막 답), 고른 실행이 이어 간 줄(continues 를 거슬러 열 단계)
    static func projectCarry(_ path: String, project: String, from run: String) -> (rows: [APCarryRow], answers: [String: String], chain: Set<String>) {
        guard let db = open(path) else { return ([], [:], []) }
        defer { sqlite3_close_v2(db) }
        var err: String?
        let rs = rows(db, """
            SELECT c.id, c.run_id, c.unit_n, c.kind, c.title, c.why, COALESCE(r.started, r.updated) FROM carry c JOIN runs r ON r.id = c.run_id
            WHERE r.project_id = ? AND c.kind = 'next_start'
            """, [project], error: &err).map { c in
            APCarryRow(id: c[0] as? String ?? "", run: c[1] as? String ?? "", unit: Int(c[2] as? Double ?? 0), kind: c[3] as? String ?? "",
                       title: apPlain(c[4] as? String ?? ""), why: apPlain(c[5] as? String ?? ""), started: c[6] as? Double)
        }
        var answers: [String: String] = [:]
        for c in rows(db, """
            SELECT k.card_id, k.answer FROM cards k JOIN runs r ON r.id = k.run_id WHERE r.project_id = ? AND k.answer IS NOT NULL
            ORDER BY COALESCE(r.started, r.updated)
            """, [project], error: &err) {
            guard let k = c[0] as? String, let a = c[1] as? String else { continue }
            if !["decide", "close"].contains(answers[k] ?? "") { answers[k] = a }
        }
        var chain: Set<String> = [run], cur = run
        for _ in 0..<10 {
            guard let prev = rows(db, "SELECT continues FROM runs WHERE id = ?", [cur], error: &err).first?.first as? String, !prev.isEmpty, !chain.contains(prev) else { break }
            chain.insert(prev); cur = prev
        }
        return (rs, answers, chain)
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
        for i in d.units.indices { d.units[i].evidence = APEvidence.load(runDir: run.runDir, n: d.units[i].n) }
        for c in rows(db, "SELECT id, unit_n, kind, title, why FROM carry WHERE run_id = ? ORDER BY unit_n, id", [run.id], error: &err) {
            d.carry.append(APCarry(id: c[0] as? String ?? UUID().uuidString, unit: Int(c[1] as? Double ?? 0), kind: c[2] as? String ?? "",
                                   title: apPlain(c[3] as? String ?? ""), why: apPlain(c[4] as? String ?? "")))
        }
        return d
    }

    // 보고서를 그릴 기록(3단계) — 원문을 지운 실행도 그리도록 행을 run.json·queue.json·engine.json 꼴 사전으로 바꾼다(결과 기록은 전문).
    // 판정 출력·거부한 일의 자세한 줄은 실행 폴더에만 있어 이 바탕에서는 마지막 판정 종료 코드와 거부한 일 목록까지다
    static func reportSource(_ path: String, run id: String) -> APRecordsReportSource? {
        guard let db = open(path) else { return nil }
        defer { sqlite3_close_v2(db) }
        var err: String?
        let rsql = """
            SELECT r.title, COALESCE(p.name, r.project_id), r.model, r.state, r.reason, r.started, r.ended, r.worktree, r.branches, r.harness, r.epic
            FROM runs r LEFT JOIN projects p ON p.id = r.project_id WHERE r.id = ?
            """
        guard let c = rows(db, rsql, [id], error: &err).first else { return nil }
        var run: [String: Any] = [:]
        for (k, i) in [("title", 0), ("project", 1), ("model", 2), ("state", 3), ("reason", 4), ("started", 5), ("ended", 6), ("epic", 10)] { run[k] = c[i] ?? nil }
        for (k, i) in [("worktree", 7), ("branches", 8), ("harness", 9)] { run[k] = json(c[i]) }
        let usql = """
            SELECT n, title, kind, story_key, task_key, branch, status, merged, active_sec, cost_usd, turns, tries, check_exit, denials, result, started, ended
            FROM units WHERE run_id = ? ORDER BY n
            """
        var queue: [[String: Any]] = [], engines: [Int: [String: Any]] = [:], results: [Int: String] = [:]
        for u in rows(db, usql, [id], error: &err) {
            let n = Int(u[0] as? Double ?? 0)
            guard n == queue.count + 1 else { continue }   // 단위 번호는 1부터 빈틈 없이(엔진이 출발 때 모든 단위를 쓴다)
            var q: [String: Any] = ["title": u[1] as? String ?? "", "kind": u[2] as? String ?? "", "status": u[6] as? String ?? "pending"]
            if let st = u[3] as? String { q["story"] = st; if let t = u[4] as? String { q["tickets"] = [t] } }
            if let b = u[5] as? String { q["branch"] = b }
            queue.append(q)
            var e: [String: Any] = [:]
            for (k, i) in [("active_sec", 8), ("cost_usd", 9), ("turns", 10), ("try", 11), ("check_exit", 12), ("started", 15), ("ended", 16)] { if let v = u[i] as? Double { e[k] = v } }
            if let m = u[7] as? String { e["merge"] = ["state": m] }
            if let d = json(u[13]) { e["denials"] = d }
            if let b = u[5] as? String { e["branch"] = b }
            engines[n] = e
            if let r = u[14] as? String { results[n] = r }
        }
        return APRecordsReportSource(runValue: run, queueValue: queue, engines: engines, results: results)
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
    case "stopped": return r.restart?.stopped != nil ? ("멈추고 다시 출발", APColor.stop) : ("중단됨", APColor.muted)
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
            // 실행이 여럿이면 줄마다 어느 실행인지(6단계 ⑫-6) — 작은 크기에서도 실행 이름을 앞에
            Text(run.name).boardFont(11, weight: .semibold).lineLimit(1).truncationMode(.tail).frame(maxWidth: 90, alignment: .leading).fixedSize(horizontal: true, vertical: false)
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
    @ObservedObject private var outbox = APOutboxReview.shared
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
            if outbox.waitingCount > 0 {
                APButton(title: "올릴 글 \(outbox.waitingCount)", icon: "paperplane", tint: APColor.wait,
                         help: "바깥에 올릴 글이 기다립니다(검수 뒤 올림). 받을 곳·계정·본문을 보고 올립니다", axid: "ap-outbox-bar") { APOutboxReview.shared.open() }
            }
            APButton(title: "지난 실행", icon: "clock.arrow.circlepath", help: "끝난 자율주행 실행을 프로젝트별로 봅니다", axid: "ap-history") {
                AutopilotStore.shared.screen = .history(nil)
            }
            if start.hasDraft, !start.shown {
                APButton(title: start.preparing ? "출발 준비 중" : (start.step >= 3 || start.prepared ? "출발 준비 끝 · 이어 가기" : "출발 준비"),
                         icon: "steeringwheel", tint: Palette.accent, help: "닫아 둔 출발 준비를 다시 엽니다", axid: "ap-draft") { APStart.shared.open() }
            } else {
                APButton(title: "새 자율주행", icon: "plus", help: "출발 준비를 엽니다(지난 실행 이어 가기·주제로 시작)", axid: "ap-new") { APStart.shared.open() }
            }
        }
        .onAppear { APOutboxReview.shared.startWatching() }   // 올릴 글(4단계): 3초마다 runs/*/outbox 폴더 시각만 본다
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
            switcher(run, compact: true)
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
                if run.active || !run.inbox.isEmpty || !(ap.said[run.id] ?? []).isEmpty { APSayBox(ap: ap, run: run) }
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
            switcher(run, compact: false)
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

    // 함께 도는 다른 실행(6단계 ⑫-6) — 누르면 그 실행의 자율주행 중 화면으로 바뀐다. 실행이 하나면 없음
    @ViewBuilder private func switcher(_ run: APRun, compact: Bool) -> some View {
        let others = ap.runs.filter { $0.id != run.id }
        if !others.isEmpty {
            WrapRow(spacing: 5) {
                Text(compact ? "다른 실행" : "함께 도는 실행 \(others.count)").boardFont(10.5).foregroundStyle(.secondary).fixedSize()
                ForEach(others) { o in
                    let (st, c) = apStateChip(o)
                    Button { ap.screen = .running(o.id) } label: {
                        HStack(spacing: 4) {
                            Text(o.name).boardFont(10.5, weight: .semibold).lineLimit(1)
                            Text(st).boardFont(9.5, weight: .semibold).foregroundStyle(c)
                        }
                        .fixedSize()
                        .padding(.horizontal, 7).frame(height: 20)
                        .background(Capsule().fill(Color.white.opacity(0.07)))
                        .overlay(Capsule().stroke(c.opacity(0.35), lineWidth: 0.5))
                        .contentShape(Capsule())
                    }
                    .buttonStyle(.plain)
                    .hint("\(o.name) 실행의 자율주행 중 화면으로(\(st))")
                    .accessibilityIdentifier("ap-switch-\(o.id)")
                }
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

    // 단추: 일시정지(또는 재개)·중단(확인 없이, 사용자 결정)·멈추고 다시 출발(지금 단위를 마친 뒤 멈추고 이 실행을 이어 가는 출발 준비를 다시 엶, 2단계 5-4)
    private func controls(_ run: APRun, compact: Bool) -> some View {
        let wait = ap.sent[run.id]
        let can = run.active && wait == nil
        let restartWhy = run.project == nil ? "프로젝트가 없는 실행이라 이어 가는 출발 준비를 열 수 없습니다(출발 화면으로 출발한 실행만)"
            : run.restart != nil ? "이미 받았습니다. 지금 단위를 마친 뒤 멈춥니다" : ""
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
                if APOutboxReview.shared.total(run: run.id) > 0 {
                    let n = APOutboxReview.shared.count(run: run.id)
                    APButton(title: n > 0 ? "올릴 글 \(n)" : "올릴 글", icon: "paperplane", tint: n > 0 ? APColor.wait : nil,
                             help: "이 실행이 바깥에 올릴 글(검수 뒤 올림이면 여기서 올립니다). 검수를 기다리는 동안에도 자율주행은 계속합니다",
                             axid: "ap-outbox-open") { APOutboxReview.shared.open(run: run.id) }
                }
                APButton(title: "멈추고 다시 출발", enabled: can && restartWhy.isEmpty,
                         help: restartWhy.isEmpty ? "지금 단위를 마친 뒤 멈추고, 이 실행을 이어 가는 출발 준비를 다시 엽니다. 목표나 범위를 바꿀 때 씁니다" : restartWhy,
                         axid: "ap-restart") {
                    ap.send("restart_from_intake", run)
                }
                if !compact {
                    Spacer(minLength: 4)
                    Text("실행 합계 \(apUSD(run.totalCost)) USD").boardFont(10.5).monospacedDigit().foregroundStyle(.secondary).fixedSize()
                }
            }
            if let w = wait {
                Text("\(apControlNames[w.action] ?? w.action) 요청을 보냈습니다. 엔진이 받기를 기다립니다…")
                    .boardFont(10.5).foregroundStyle(APColor.stop)
                    .accessibilityIdentifier("ap-sent")
            } else if let rs = run.restart, run.active {
                Text(restartNote(rs, paused: run.state == "paused"))
                    .boardFont(10.5).foregroundStyle(APColor.stop).fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("ap-restart-note")
            }
            if !run.active, wait == nil {
                if run.restart != nil, run.project != nil {
                    WrapRow(spacing: 6) {
                        Text(run.restart?.stopped != nil ? "멈추고 다시 출발로 멈췄습니다." : "멈추고 다시 출발을 받은 뒤 끝났습니다.").boardFont(10.5).foregroundStyle(APColor.stop)
                        if run.wrapped {
                            APBack(title: "출발 준비 열기 →", axid: "ap-restart-open") { APStart.shared.open(continuing: run.id) }
                        } else {
                            Text("엔진이 끝 정리를 마치면 출발 준비를 엽니다.").boardFont(10.5).foregroundStyle(.secondary)
                        }
                    }
                }
                WrapRow(spacing: 6) {
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

    private func restartNote(_ rs: APRestartMark, paused: Bool) -> String {
        let when: String = rs.unit.map { "지금 단위(단위 \($0))를 마친 뒤" } ?? "곧"
        let tail: String = paused ? " 일시정지 중이라 재개해야 그 단위를 마칩니다. 바로 멈추려면 중단합니다." : ""
        return "멈추고 다시 출발을 받았습니다. \(when) 멈추고, 멈추면 이 실행을 이어 가는 출발 준비를 엽니다.\(tail)"
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
            APStashNote(stashes: run.stashes)
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

// 방향을 바꿀 말(와이어프레임 자율주행 중 ③ "한마디", 2단계 5-4) — "지금 단위에"(이 세션의 다음 차례)·"다음 단위부터"(뒤 단위의 첫 입력)를 고르고 보낸다.
// 아래에 보낸 말과 엔진이 한 일(받음·넘김·붙임·넘기지 못함과 까닭)을 최근 것부터. 작은 크기에서는 그리지 않는다(와이어프레임 ⑨)
struct APSayBox: View {
    @ObservedObject var ap: AutopilotStore
    let run: APRun
    @State private var to: APInboxTo = .now
    @State private var text = ""

    private struct Line: Identifiable { let id: String; let at: Double; let to: String; let text: String; let status: String; let detail: String; let color: Color }

    var body: some View {
        let running = run.units.first { $0.status == "running" }
        let next = run.units.first { $0.status == "pending" && $0.n > (running?.n ?? 0) }
        let count = text.trimmingCharacters(in: .whitespacesAndNewlines).count
        VStack(alignment: .leading, spacing: 6) {
            if run.active {
                WrapRow(spacing: 6) {
                    Text("한마디").boardFont(11.5, weight: .bold)
                    APButton(title: "지금 단위에", tint: to == .now ? Palette.accent : nil, help: "이 세션의 다음 차례에 넘깁니다", axid: "ap-say-now") { to = .now }
                    APButton(title: "다음 단위부터", tint: to == .next ? Palette.accent : nil, help: "뒤에 시작하는 단위의 첫 입력에 모두 붙입니다", axid: "ap-say-next") { to = .next }
                }
                ZStack(alignment: .topLeading) {
                    TextEditor(text: $text)
                        .font(.system(size: 12))
                        .scrollContentBackground(.hidden)
                        .frame(minHeight: 44, maxHeight: 110)
                        .padding(4)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
                        .accessibilityIdentifier("ap-say-text")
                    if text.isEmpty {
                        Text("예: 정렬은 화면에 보이는 순서와 같아야 합니다").boardFont(11.5).foregroundStyle(.secondary.opacity(0.7))
                            .padding(.horizontal, 9).padding(.vertical, 8).allowsHitTesting(false)
                    }
                }
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    apNote(to == .now
                           ? (running.map { "지금 단위에 보내면 이 세션의 다음 차례에 들어갑니다(단위 \($0.n))." }
                              ?? (next.map { "지금 도는 단위가 없어 다음에 시작하는 단위 \($0.n) 의 첫 입력에 붙습니다." } ?? "남은 단위가 없습니다."))
                           : (next.map { "다음 단위부터 보내면 단위 \($0.n) 부터 뒤 단위의 첫 입력에 모두 붙습니다." } ?? "뒤에 남은 단위가 없습니다."))
                        .accessibilityIdentifier("ap-say-hint")
                    Spacer(minLength: 4)
                    Text(verbatim: "\(count)/\(apInboxMaxChars)").boardFont(10).monospacedDigit().foregroundStyle(count > apInboxMaxChars ? APColor.fail : .secondary).fixedSize()
                    APButton(title: "보내기", primary: true, enabled: count > 0 && count <= apInboxMaxChars,
                             help: count == 0 ? "보낼 말을 써 주세요" : count > apInboxMaxChars ? "\(apInboxMaxChars)자까지 보낼 수 있습니다" : "inbox 에 써서 엔진에 넘깁니다", axid: "ap-say-send") {
                        if ap.say(text, to: to, run) { text = "" }
                    }
                }
                apNote("말은 방향만 바꿉니다. 대기열(단위)·판정·쓰기 범위·가지 보호는 출발 때 그대로이고, 범위 밖의 말은 모델이 하지 않고 미룬 일로 적습니다.")
                if let e = ap.sayError[run.id] { apNote(e, APColor.fail).accessibilityIdentifier("ap-say-error") }
            }
            let lines = lines()
            if !lines.isEmpty {
                Text("보낸 말 \(lines.count)").boardFont(11, weight: .semibold).padding(.top, run.active ? 4 : 0)
                ForEach(lines) { l in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(apClock(l.at)).boardFont(10, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                            APChip(text: l.to == "next" ? "다음 단위부터" : "지금 단위에")
                            APChip(text: l.status, color: l.color, strong: true)
                            Spacer(minLength: 0)
                        }
                        Text(l.text).boardFont(11).lineLimit(3).fixedSize(horizontal: false, vertical: true)
                        if !l.detail.isEmpty { apNote(l.detail) }
                    }
                    .padding(.leading, 4)
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("ap-said-\(l.id)")
                }
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.white.opacity(0.04)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-say")
    }

    // 엔진이 받은 말(run.json inbox)과 아직 받지 않은 말(보드가 쓴 것), 최근 것부터
    private func lines() -> [Line] {
        var out: [Line] = run.inbox.filter { $0.status != "bad" || !$0.file.isEmpty }.map { e in
            let l = apInboxLabel(e)
            let c: Color = ["waiting": APColor.wait, "delivered": APColor.ok, "applied": APColor.ok, "bad": APColor.fail][e.status] ?? APColor.muted
            return Line(id: e.file, at: e.sent ?? e.received, to: e.to, text: e.status == "bad" ? "(읽지 못한 파일 \(e.file))" : e.text, status: l.status, detail: l.detail, color: c)
        }
        for s in ap.said[run.id] ?? [] where !out.contains(where: { $0.id == s.file }) {
            out.append(Line(id: s.file, at: s.at, to: s.to, text: s.text, status: run.active ? "보냄" : "받지 못함",
                            detail: run.active ? "엔진이 받기를 기다립니다(1초 안에 받습니다)" : "실행이 끝나 엔진이 받지 못했습니다", color: run.active ? APColor.stop : APColor.muted))
        }
        return out.sorted { $0.at > $1.at }
    }
}

// MARK: 자율주행 · 지난 실행

struct APHistoryScreen: View {
    @ObservedObject var ap: AutopilotStore
    let selected: String?
    @ObservedObject private var fit = BoardFit.shared
    @State private var folded: Set<String> = []     // 접은 프로젝트 묶음
    @State private var reports: Set<Int> = []       // 보고서를 펼친 단위
    @State private var confirmPurge: String?        // 원문 지우기 확인을 연 실행(화면 안 확인, 5-2b)

    private var shown: [APPastRun] { ap.history.runs.filter { ap.historyProject == nil || $0.projectName == ap.historyProject } }
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
            Button("모든 프로젝트") { ap.historyProject = nil }
            ForEach(Array(Set(ap.history.runs.map(\.projectName))).sorted(), id: \.self) { n in Button(n) { ap.historyProject = n } }
        } label: {
            Text(ap.historyProject ?? "모든 프로젝트").boardFont(11, weight: .semibold)
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

    private func isOn(_ r: APPastRun) -> Bool { ap.historyShown == r.id }

    private func row(_ r: APPastRun) -> some View {
        Button { ap.screen = .history(r.id) } label: {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(apDate(r.started)).boardFont(11.5, weight: .semibold).monospacedDigit().fixedSize()
                    if !r.mode.isEmpty { APChip(text: r.mode == "worktree" ? "따로 작업 트리" : "원래 작업 트리") }
                    if r.jira { APChip(text: "스토리 \(r.stories)") }
                    Spacer(minLength: 0)
                    if let s = r.stateLabel { APChip(text: s.0, color: s.1, strong: true) }
                    if r.purged != nil { APChip(text: "원문 지움") }
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
            purgeBlock(d.run)
        }
    }

    // 원문 지우기(2026-10-22 사용자 결정: 자동으로 지우지 않고 사람이, 2단계 5-2b) — 지울 수 있는지는 규칙 apPurgeCheck, 화면 안 확인 뒤에만 지운다.
    // 지우는 것은 실행 폴더 runs/<실행 id>/ 하나이고, 지운 기록은 자율주행 뿌리 purged/<실행 id>.json
    @ViewBuilder private func purgeBlock(_ r: APPastRun) -> some View {
        if let rec = r.purged {
            Text("원문을 지웠습니다(\(apDate(rec.at)) · 파일 \(rec.files)개 · \(ByteCountFormatter.string(fromByteCount: rec.bytes, countStyle: .file))). 지난 실행 목록과 단위 결과는 기록 저장소에 남았습니다."
                 + (rec.error.map { " 지우다 생긴 일: \($0)" } ?? ""))
                .boardFont(10.5).foregroundStyle(rec.error == nil ? Color.secondary : APColor.limit).fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("ap-purged-note")
        }
        if let e = ap.purgeError[r.id] { apNote("원문을 지우지 못했습니다: \(e)", APColor.fail) }
        if confirmPurge == r.id, case .ok(let path) = apPurgeCheck(ap.purgeInput(r)) {
            APBox(title: "원문 지우기", note: "되돌릴 수 없습니다", tint: APColor.fail, axid: "ap-purge-confirm") {
                Text(apHome(path)).boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                apNote("이 실행 폴더(stream 원문·결과 기록 파일·대기열 정의·진행 기록)를 지웁니다. 기록 저장소의 지난 실행 목록과 단위 결과(결과 기록 전문)는 남고, "
                       + "엔진 전용 private/\(r.id)/(하네스 사본)와 프로젝트 설정·세트도 남깁니다. 지운 뒤에는 이 실행을 이어 가거나(남은 단위의 정의가 없어집니다) 합칠 수 없습니다.")
                HStack(spacing: 6) {
                    APButton(title: ap.purging == r.id ? "지우는 중" : "지우기", icon: "trash", tint: APColor.fail, enabled: ap.purging == nil, axid: "ap-purge-go") {
                        ap.purge(r); confirmPurge = nil
                    }
                    APButton(title: "그만두기", axid: "ap-purge-cancel") { confirmPurge = nil }
                }
            }
        }
    }

    // 이 실행 이어 가기(출발 준비를 이 실행으로 엶, 2단계 5-2a) · 원문 지우기(5-2b) · 보고서 보기(3단계, 보고서 창 APReportViewer)
    private func actionButtons(_ r: APPastRun) -> some View {
        let pc = apPurgeCheck(ap.purgeInput(r))
        var purgeOK = false, purgeWhy = ""
        switch pc {
        case .ok: purgeOK = true; purgeWhy = "이 실행 폴더(원문)를 지웁니다. 화면 안에서 한 번 더 확인합니다"
        case .gone: purgeWhy = r.purged == nil ? "실행 폴더가 이미 없습니다" : "원문을 이미 지웠습니다"
        case .refuse(let w): purgeWhy = w
        }
        return WrapRow(spacing: 6) {
            APButton(title: "이 실행 이어 가기", primary: true, enabled: r.project != nil && r.purged == nil,
                     help: r.project == nil ? "프로젝트가 없는 실행이라 이어 갈 수 없습니다"
                        : r.purged != nil ? "원문을 지워 남은 단위의 정의가 없습니다. 이 실행의 열린 미룬 몫은 같은 프로젝트의 다른 실행을 이어 갈 때 '이어 간 줄 밖'으로 보입니다"
                        : "남은 단위와 미룬 사람 몫을 불러와 출발 준비를 엽니다",
                     axid: "ap-continue-\(r.id)") { APStart.shared.open(continuing: r.id) }
            APButton(title: "합치기·승격", icon: "arrow.triangle.merge", primary: r.merge?.hasWaiting == true, enabled: r.hasBranches && FileManager.default.fileExists(atPath: r.runDir + "/run.json"),
                     help: r.hasBranches ? "바뀐 줄을 보고 승격 차례대로 합치거나 버립니다" : "엔진이 만든 가지가 없는 실행입니다(또는 실행 폴더를 지웠습니다)",
                     axid: "ap-merge-\(r.id)") { APMerge.shared.open(r.runDir) }
            APButton(title: "원문 지우기", icon: "trash", enabled: purgeOK && ap.purging == nil, help: purgeWhy, axid: "ap-purge-\(r.id)") { confirmPurge = r.id }
            APButton(title: "보고서 보기", icon: "doc.richtext",
                     help: r.purged == nil ? "결과 기록과 엔진 기록으로 그린 보고서(흐름 그림·단위별 기록)를 엽니다. 파일 하나로 저장할 수 있습니다"
                        : "원문을 지워 기록 저장소의 결과 기록으로 그립니다(판정 출력은 없습니다)",
                     axid: "ap-report-\(r.id)") { APReportViewer.show(run: r.id, runDir: r.runDir, title: r.title.isEmpty ? r.projectName : r.title) }
            if APOutboxReview.shared.total(run: r.id) > 0 {
                let n = APOutboxReview.shared.count(run: r.id)
                APButton(title: n > 0 ? "올릴 글 \(n)" : "올린 글", icon: "paperplane", tint: n > 0 ? APColor.wait : nil,
                         help: "이 실행의 바깥 글: 받을 곳·계정·본문을 보고 올리거나 올린 주소를 봅니다", axid: "ap-outbox-\(r.id)") { APOutboxReview.shared.open(run: r.id) }
            }
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
            APStashNote(stashes: r.stashes)
            carryBlock(d)
            actionButtons(r)
            purgeBlock(r)
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
            if let ev = unit.evidence { evidence(ev) }
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
    // 판정 증거(5단계 ⑪-1): 걸음마다 결과와 남긴 파일, 증거 폴더 열기(Finder 에서 그 폴더를 고른 채로)
    @ViewBuilder private func evidence(_ ev: APEvidence) -> some View {
        Divider().padding(.vertical, 2)
        HStack(spacing: 6) {
            Text("판정 증거 · \(ev.tryN == 0 ? "첫 판정" : "\(ev.tryN + 1)번째 판정") · \(ev.sandbox ? "샌드박스 안" : "샌드박스 밖")")
                .boardFont(10.5, weight: .semibold)
            Spacer(minLength: 4)
            Button("증거 폴더 열기") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: ev.dir)]) }
                .buttonStyle(.link).boardFont(10.5)
                .accessibilityIdentifier("ap-evidence-open-\(unit.n)")
                .help(ev.dir.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
        }
        VStack(alignment: .leading, spacing: 2) { ForEach(Array(ev.parts.enumerated()), id: \.offset) { _, p in
            let files = p.files.filter { $0 != "output.log" }
            Text("\(p.passed ? "통과" : "실패") · \(p.label) · 종료 코드 \(p.exit) · \(p.sec < 1 ? "1초 미만" : apDur(p.sec))"
                 + (p.shots.isEmpty ? "" : " · 스크린샷 \(p.shots.joined(separator: ", "))")
                 + (files.isEmpty ? "" : " · 파일 \(files.prefix(4).joined(separator: ", "))\(files.count > 4 ? " 외 \(files.count - 4)" : "")")
                 + (p.note.isEmpty ? "" : " · \(p.note)"))
                .boardFont(10.5).foregroundStyle(p.passed ? Color.secondary : APColor.fail).fixedSize(horizontal: false, vertical: true)
        } }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-evidence-\(unit.n)")
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

// 보드가 돌리는 git 의 공통 보호(엔진 gitHardenC·gitHardenEnv 와 같은 꼴, ⑦ 보드 git 받침). -c: 저장소 설정의 fsmonitor·hooks·attributes 를
// 쓰지 않고, bare 저장소 꼴 폴더를 스스로 저장소로 쓰지 않으며(safe.bareRepository), 하위 모듈(모델이 심은 중첩 git)로 status·diff 가
// 들어가지 않는다(diff.ignoreSubmodules). env: 시스템·전역 설정·attributes·replace 를 읽지 않는다. 메인 스레드에서 부르지 않는다
let apGitHardenC = ["--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.attributesFile=/dev/null",
                    "-c", "safe.bareRepository=explicit", "-c", "diff.ignoreSubmodules=all"]
// 엔진 canonical 과 같게 경로를 푼다(실행 기록의 common_dir 과 글자로 견주려면 같은 꼴이어야 한다)
func apCanonical(_ path: String) -> String {
    let p = (path as NSString).expandingTildeInPath
    guard let r = realpath(p, nil) else { return URL(fileURLWithPath: p).standardizedFileURL.path }
    defer { free(r) }
    return String(cString: r)
}
// git 폴더의 "장치:inode"(엔진 gitStat 과 같은 꼴, 바꿔치기 가리기)
func apGitStat(_ p: String) -> String? {
    var st = stat()
    return lstat(p, &st) == 0 ? "\(st.st_dev):\(st.st_ino)" : nil
}
func apGitEnv(_ extra: [String]) -> [String: String] {
    var env = ProcessInfo.processInfo.environment
    for (k, v) in [("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")] { env[k] = v }
    // extra: "GIT_DIR=…"·"GIT_WORK_TREE=…"·"GIT_CEILING_DIRECTORIES=…" 꼴로 실행 기록이 있는 자리를 박는다(apBoardRefsPin)
    for s in extra { if let eq = s.firstIndex(of: "=") { env[String(s[..<eq])] = String(s[s.index(after: eq)...]) } }
    return env
}
// 보드가 읽는 git(지금 가지·가지 목록·머리·커밋 안 한 변경·커밋 수). env 로 GIT_DIR 등을 박는다(실행 기록이 있는 자리)
func apGit(_ args: [String], _ cwd: String, env: [String] = []) -> String? {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
    p.arguments = apGitHardenC + args
    p.currentDirectoryURL = URL(fileURLWithPath: cwd)
    p.environment = apGitEnv(env)
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
// 4단계 ⑩-3 의 "실행 중에 새로 필요해진 티켓"(Jira 연결 실행)이 첫 정책 카드다. 하네스 확인 지점 카드는 7단계
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

// 지난 판 자율주행 지시(기록 저장소 sets.instruction) — 그 판을 쓴 실행과 출발 시각
struct APSetVersion: Equatable { let text: String; let at: Double; let run: String }

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
    // 시험 실행기(5단계 ⑪-1): 이미 통과한 시나리오 카드(엔진 prerun.json 의 시나리오 카드 감), 실행 파일이 없는 수용 기준 카드(엔진 status --json 의 criteria_gaps)
    @Published private(set) var scenarioCards: [APCard] = []
    @Published private(set) var criterionCards: [APCard] = []
    // 5단계 ⑪-3 — 판정 비판: 반대 탐침 카드(prerun.json probe_cards)·비판 세션의 마지막 차례(critic.json)와 카드, 판정 고치기 차례(주제로 시작만),
    // 약해진 고침 카드, 판정 놓침(misses.json)과 세트 이어 쓰기(revise.json, 이어 가기)·판정 차이(출발 확인)
    @Published private(set) var probeCards: [APCard] = []
    @Published private(set) var criticCards: [APCard] = []
    @Published private(set) var criticSummary: [String: Any] = [:]
    @Published private(set) var weakCard: APCard?
    @Published private(set) var fixRound = 0
    @Published private(set) var fixing = false
    @Published private(set) var missInfo: [String: Any] = [:]
    @Published private(set) var judgeChanges: [[String: Any]] = []
    @Published private(set) var judgeChangeFrom = ""
    private var missesUsed: [String] = []
    private var pendingSet: String?
    private var critRound = 0
    private var keepHistory = false
    @Published var cards: [APCard] = []
    @Published var answers: [String: String] = [:]
    @Published private(set) var policyLoaded: Set<String> = []
    @Published var bases: [String: String] = [:]
    @Published private(set) var baseNote = ""
    @Published private(set) var promotion: [APStep] = []
    @Published private(set) var promotionSaved = false
    @Published var epic = "" { didSet { if epic != oldValue { refreshEpic() } } }
    // 4단계 ⑩-2 — 에픽 칸의 제목·상태(앱 안 엔진 read epic, 연결이 없으면 번호 꼴만), 주제로 시작의 링크 읽기 줄
    @Published private(set) var epicInfo: APEpicSeen?
    private var epicWork: DispatchWorkItem?
    private var topicOtherDone = false
    private(set) var topicBundle = ""
    // 5단계 ⑪-2 — 주제로 시작의 초안 세션 결과(엔진 draft 가 쓴 draft-check.json)·고른 스택·새 폴더인지·새 폴더 가지 꼴(dlm: dev·live·master, main: main 하나)
    @Published private(set) var draftResult: [String: Any] = [:]
    @Published private(set) var topicStack = APStackSet()
    @Published private(set) var topicNew = false
    @Published private(set) var topicLayout = "dlm"
    @Published private(set) var topicReseeding = false
    private(set) var draftCards: [APCard] = []
    private(set) var harnessCards: [APCard] = []
    private var topicKey = ""            // 준비한 주제 입력(같으면 1단계 "다음"이 다시 준비하지 않는다)
    private var linkURLs: [String: String] = [:]     // 준비 줄 id(link-<n>) → 주소
    @Published var stories: [APStory] = []
    // 4단계 ⑩-3 — 만들 티켓 나무: 엔진 tickets 명령이 쓴 상태(tickets.json, 보드는 읽기만), 사람이 "그래도 만들기"로 확인한 확인 필요 항목, 만들기 결과 한 줄
    @Published private(set) var ticketBook = APTicketBook()
    @Published var ticketConfirm: Set<String> = []
    private var ticketFrom: String?   // 물려받은 티켓 기록 자리(새 준비 폴더를 처음 만들 때 그 폴더로 옮겨 적는다)
    @Published private(set) var ticketNote: (text: String, bad: Bool)?
    @Published private(set) var estimates: [String: APEstimate] = [:]
    @Published private(set) var starting = false
    @Published private(set) var startError: String?
    @Published private(set) var changedBy: String?         // 카드 답이 고른 단위를 바꿨을 때(접힌 "고른 단위" 머리 강조)
    @Published private(set) var flash = 0
    // 이어 갈 지난 Jira 실행이 만든 스토리·작업 가지 가운데 아직 남아 출발을 막는 것(2026-10-28 사용자 결정: 먼저 합치거나 버린 뒤 이어 감, 2단계 5-3)
    @Published private(set) var leftBranches: [String] = []
    // 2단계 5-2b — 원천(continue·topic, 고르지 않았으면 빈 글자), 주제로 시작의 입력 판, 경로 카드 오른쪽 클릭 "이 경로에서 자율주행…"으로 연 경로
    @Published private(set) var source = ""
    let topic = APTopic()
    // 가져오기(7단계 ⑫-7a): 원천 셋의 입력, 모으기 결과(import/gather.json), 가져오기 세션 결과(import-check.json), 이어 간 줄의 바뀐 본문(단위 id → 줄 차이),
    // 단위 제안에서 되살린 키(다시 준비할 때 가리지 않음), 이 준비가 세트에 지시가 없어 초안 지시를 썼는지
    let imp = APImportInput()
    @Published private(set) var importGather: [String: Any] = [:]
    @Published private(set) var importResult: [String: Any] = [:]
    @Published private(set) var importChanged: [String: [String]] = [:]
    @Published var importRevive: Set<String> = []
    private(set) var importKey = ""
    private(set) var importDraftedSet = false
    @Published private(set) var presetRoot: String?
    // 5-2b — 출발 확인의 설정 줄(모델·멈출 선)·승격 차례·자율주행 지시를 그 자리에서 바꾸기, 이어 간 줄 밖의 열린 미룬 몫
    @Published private(set) var settingsFrom = "past"            // 설정 값이 어디서 왔나(project·past·default, 규칙 apStartSettings)
    @Published private(set) var settingsEdited = false
    @Published private(set) var settingsProblems: [String] = []
    @Published private(set) var promotionEdited = false
    private var promotionBefore: [APStep] = []
    @Published private(set) var lastSet: APSetVersion?
    @Published var editingSet = false
    @Published var setDraft = ""
    @Published private(set) var setNote: String?
    @Published private(set) var openParked: [APCarryRow] = []
    @Published private(set) var includeParked: Set<String> = []
    // 4단계 ⑩-1 — 게시 방식·보고서 받을 곳·Jira 댓글(프로젝트 설정 publish → 지난 실행 → 기본: 검수 뒤 올림·받을 곳 없음).
    // 이 준비의 run.json 에 쓰고, 바꿨거나 프로젝트 설정에 없으면 출발할 때 프로젝트 설정에 남긴다
    @Published private(set) var publish = APPublish()
    @Published private(set) var publishEdited = false
    @Published private(set) var publishProblems: [String] = []
    @Published var pubService = "none"
    @Published var pubSpace = ""
    @Published var pubParent = ""
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
    // 같은 경로·같은 프로젝트에서 도는 실행 — 끝날 때까지 출발을 잠근다. 6단계 ⑫-6 부터 엔진도 출발할 때 기록 저장소의 경로 자리로 거부한다(거의 같은 때 둘을 띄워도).
    // 경로는 실제 경로로 풀고 대소문자·유니코드 정규화를 접어 견준다(규칙 apPathOverlap, 엔진과 같은 규칙)
    var activeHere: String? {
        guard hasDraft || isImport else { return nil }
        let key = root.isEmpty ? nil : apPathKey(apRealPath(root))
        return AutopilotStore.shared.runs.first { r in
            (key.map { k in r.repos.contains { apPathOverlap(k, apPathKey(apRealPath($0)), home: NSHomeDirectory()) } } ?? false)
                || (r.project != nil && r.project == project)
        }?.id
    }

    // 카드가 단위에 내린 결정 — 답하지 않은 카드가 있으면 "pending", 빼기·판정 고치기 답이면 "out"
    func cardDecision(_ u: APDraftUnit) -> String {
        let mine = cards.filter { $0.unit == u.id }
        if mine.contains(where: { ["drop", "fix_check"].contains(answers[$0.id] ?? "") }) { return "out" }   // 판정 고치기: 이어 가기는 다음 출발로, 주제로 시작은 고친 뒤 새 카드

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
            if (hasDraft || freshSource), !preparing, !starting { dropFolder(); clearAll() }
            if !hasDraft, !freshSource { load(select: runId) }
        } else if !hasDraft, !freshSource { reset(); load(select: nil) }
        checkLeftovers()
        present()
    }
    // 경로 카드 오른쪽 클릭 "이 경로에서 자율주행…"(와이어프레임 보드 본체 ②, 설계 2장) — 그 경로를 작업 경로로 채운 출발 준비 1단계.
    // 그 경로에 지난 실행이 있으면 가장 최근 실행을 이어 갈 원천으로, 없으면 주제로 시작(기존 저장소)으로 연다(설계 판단). 준비하는 중이면 그 준비를 연다
    func open(at path: String) {
        if !sweptDrafts { sweepDrafts() }
        if preparing || starting { present(); return }
        dropFolder()
        clearAll()
        presetRoot = path
        past = APRecords.list(db).runs.filter { $0.project != nil }
        if let r = pastAt(path).first { choose(r.id) } else { chooseSource("topic") }
        checkLeftovers()
        present()
    }
    // 그 경로의 지난 실행(최근 것부터) — 프로젝트 작업 경로가 그 경로이거나, 그 경로가 작업 경로 안이거나, 작업 경로를 품은 폴더(홈은 뺌). APRun.covers 와 같은 규칙
    func pastAt(_ path: String) -> [APPastRun] {
        past.filter { r in
            guard let root = r.root else { return false }
            return path == root || path.hasPrefix(root + "/") || (root.hasPrefix(path + "/") && path != NSHomeDirectory())
        }
    }
    // 원천 고르기 — 지난 실행 이어 가기·주제로 시작(나머지 셋은 잠금). 준비하는 중에는 바꾸지 않는다
    func chooseSource(_ key: String) {
        guard !preparing, !starting, key != source else { return }
        switch key {
        case "continue":
            let cand = presetRoot.map { pastAt($0) } ?? past
            if let r = cand.first ?? past.first { choose(r.id) }
        case "topic":
            let from = presetRoot ?? (root.isEmpty ? nil : root)
            let keep = past   // 이어 갈 실행 목록은 둔다(다시 이어 가기로 돌아올 수 있게)
            if hasDraft || isImport { dropFolder(); reset() }
            past = keep
            source = "topic"
            step = 1
            if topic.existingPath.isEmpty, topic.pathMode == "existing", let f = from { topic.existingPath = f }
        case "todo", "list", "legacy":   // 가져오기(7단계 ⑫-7a) — 작업 경로는 경로 카드 경로 → 지금 경로 → 가장 최근 실행의 경로
            let from = presetRoot ?? (root.isEmpty ? nil : root) ?? past.first?.root
            let keep = past
            if hasDraft || freshSource { dropFolder(); reset() }
            past = keep
            source = key
            step = 1
            if imp.path.isEmpty, let f = from { imp.path = f }
        default: break
        }
    }
    // 보드를 끄면 준비는 사라진다(메모리에만) — 보드가 처음 출발 준비를 열 때 drafts/ 에 남은 출발하지 않은 폴더를 치운다(drafts/ 는 보드만 쓴다)
    private var sweptDrafts = false
    // 주제로 시작(5단계 ⑪-2): 준비가 만든 자율주행 가지 autopilot/<준비 id> 도 규칙(apDraftSweepDecide)대로 앱 안 엔진 seed drop 으로 지운다
    // (가지 머리가 준비가 커밋한 그대로이고 어느 작업 트리에도 걸려 있지 않을 때만, 엔진이 한 번 더 본다). 새 폴더(저장소)는 지우지 않는다
    private func sweepDrafts() {
        sweptDrafts = true
        for n in (try? FileManager.default.contentsOfDirectory(atPath: apDraftsRoot)) ?? [] where !n.hasPrefix(".") {
            let d = apDraftsRoot + "/" + n
            let k = apDraftSweepDecide(Self.draftFolder(d, inUse: false))
            Self.dropDraft(d, seed: k.removeSeed, folder: k.removeFolder)
        }
    }
    static func draftFolder(_ d: String, inUse: Bool) -> APDraftFolder {
        let run = apRead(d + "/run.json") as? [String: Any]
        let sd = apSeedParse(run?["seed"])
        var head: String?, used = false
        if let s = sd, FileManager.default.fileExists(atPath: s.repo) {
            head = apGit(["rev-parse", "-q", "--verify", "refs/heads/" + s.branch], s.repo).flatMap { $0.isEmpty ? nil : $0 }
            let wt = (apGit(["worktree", "list", "--porcelain"], s.repo) ?? "").components(separatedBy: "\n\n")
            used = wt.contains { $0.contains("branch refs/heads/" + s.branch) && !$0.contains("/.claude-board/worktrees/.seed-") }
        }
        return APDraftFolder(name: (d as NSString).lastPathComponent, started: run?["state"] != nil, madeTickets: madeTickets(d), inUse: inUse,
                             seedRepo: sd?.repo, seedHead: sd?.head, seedBranchHead: head, seedCheckedOut: used)
    }
    static func dropDraft(_ d: String, seed: Bool, folder: Bool) {
        guard seed || folder else { return }
        let dev = apDevBuild
        DispatchQueue.global().async {
            if seed { _ = apEngine(["seed", d, "drop"] + (dev ? ["--dev"] : [])) }
            if folder { try? FileManager.default.removeItem(atPath: d) }
        }
    }
    // 이미 만든 티켓 기록(tickets.json 에 만든 항목)이 있는 준비 폴더는 지우지 않는다 — 같은 지난 실행을 다시 이어 갈 때 그 기록을 물려받아
    // 같은 티켓을 또 만들지 않는다(4단계 ⑩-3, 사용자 결정 "만든 티켓은 남기고 다시 누르면 남은 것만")
    static func madeTickets(_ d: String) -> Bool { apTicketBookParse(FileManager.default.contents(atPath: d + "/tickets.json")).entries.values.contains { $0.state == .made } }
    private func dropFolder() {
        guard !dir.isEmpty else { return }
        let k = apDraftSweepDecide(Self.draftFolder(dir, inUse: false))
        Self.dropDraft(dir, seed: k.removeSeed, folder: k.removeFolder)
    }
    // 같은 지난 실행을 이어 가던 앞 준비가 만든 티켓 기록 — 가장 최근 것
    private func inheritedTickets(_ runId: String) -> String? {
        let fm = FileManager.default
        return ((try? fm.contentsOfDirectory(atPath: apDraftsRoot)) ?? []).filter { !$0.hasPrefix(".") }.map { apDraftsRoot + "/" + $0 }
            .filter { d in (apRead(d + "/run.json") as? [String: Any]).map { $0["state"] == nil && $0["continues"] as? String == runId } ?? false && Self.madeTickets(d) }
            .max { apStamp($0 + "/tickets.json") < apStamp($1 + "/tickets.json") }
            .map { $0 + "/tickets.json" }
    }

    private func present() {
        APConnections.shared.loadOnce()   // 자료 링크 줄·만들 티켓 나무의 읽기 상태(4단계 ⑩-2)가 연결 목록·키체인 있음 여부를 본다
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
        clearAll()
    }

    private func reset() {
        step = 1; past = []; pastId = nil; root = ""; project = nil; jira = false; setText = nil; setAt = nil
        units = []; carryPark = []; carryNew = []; noDefs = nil; lines = []; preparing = false; prepared = false; rechecking = false
        problems = []; mode = ""; modeReason = ""; repos = []; repoInfo = [:]; storyBranch = [:]; prerunPassed = []; prerunIds = []; prerunNote = ""; scenarioCards = []; criterionCards = []
        cards = []; answers = [:]; policyLoaded = []; bases = [:]; baseNote = ""; promotion = []; promotionSaved = false
        probeCards = []; criticCards = []; criticSummary = [:]; weakCard = nil; fixRound = 0; fixing = false; missInfo = [:]; judgeChanges = []; judgeChangeFrom = ""
        missesUsed = []; pendingSet = nil; critRound = 0; keepHistory = false
        epic = ""; stories = []; estimates = [:]; starting = false; startError = nil; changedBy = nil; leftBranches = []
        epicInfo = nil; topicOtherDone = false; topicBundle = ""; linkURLs = [:]
        draftResult = [:]; topicStack = APStackSet(); topicNew = false; topicLayout = "dlm"; topicReseeding = false; draftCards = []; harnessCards = []; topicKey = ""
        ticketBook = APTicketBook(); ticketConfirm = []; ticketNote = nil; ticketFrom = nil
        id = ""; dir = ""; settings = [:]; tickets = [:]; needNewId = false; proposedOnce = false
        settingsFrom = "past"; settingsEdited = false; settingsProblems = []; promotionEdited = false; promotionBefore = []
        lastSet = nil; editingSet = false; setDraft = ""; setNote = nil; openParked = []; includeParked = []
        importGather = [:]; importResult = [:]; importChanged = [:]; importRevive = []; importKey = ""; importDraftedSet = false
        if source == "continue" { source = "" }
    }
    // 원천·주제 입력·경로 카드 경로까지 모두(준비 버리기·출발 뒤)
    private func clearAll() {
        reset()
        source = ""
        presetRoot = nil
        topic.reset()
        imp.reset()
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
        source = "continue"
        // 앞 준비가 이 실행을 이어 가며 이미 만든 티켓(출발하지 못한 채 보드를 껐거나 준비를 버림) — 물려받아 남은 것만 만든다
        if let t = inheritedTickets(runId) { ticketBook = apTicketBookParse(FileManager.default.contents(atPath: t)); ticketFrom = t }
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
        if apDevBuild { keys += ["claude_args", "claude_path", "usage_path"] }   // usage_path: 장난감 사용량 파일(6단계 ⑫-6, 시험 빌드만)
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
                       "tickets", "review", "source_key", "kind", "est_min", "est_basis", "story", "depends", "prerun",
                       "scenarios", "criteria", "check_ports", "check_sandbox"]
        let defaultCap = APCaps.from(pr["caps"]).minutes
        // 판정을 통과했지만 합치기·승격 화면에서 버린 단위는 일이 저장소에 없으므로 다시 후보로(판정 놓침, 5단계 ⑪-3, 규칙 apDiscardedUnit)
        let gone = Set(((pr["branches"] as? [String: Any])?["discarded"] as? [[String: Any]] ?? []).compactMap { $0["source"] as? String })
        for (i, d0) in q.enumerated() {
            let n = i + 1
            let st = detail.units.first { $0.n == n }?.status ?? d0["status"] as? String ?? "pending"
            let discardedPass = st == "done" && apDiscardedUnit(story: apText(d0["story"]), discardedSources: gone)
            guard open.contains(st) || discardedPass else { continue }
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
            u.last = discardedPass ? "통과했지만 버림" : st
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
        refreshEpic()   // 에픽 칸은 jira 가 정해진 뒤에 읽는다(위에서 epic 을 채울 때는 아직 jira 가 앞 값이라 건너뜀)
        var seen = Set<String>()
        for u in units { if let s = u.story, seen.insert(s).inserted { stories.append(APStory(id: s, key: s, kind: u.kind)) } }
        let setPath = autopilotRoot + "/private/projects/\(proj)/set/autopilot.md"
        setText = (try? String(contentsOfFile: setPath, encoding: .utf8)).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.flatMap { $0.isEmpty ? nil : $0 }
        setAt = mtime(setPath)?.timeIntervalSince1970
        lastSet = APRecords.lastSet(db, project: proj).map { APSetVersion(text: $0.text, at: $0.at, run: $0.run) }
        // 모델·멈출 선: 출발 확인에서 바꿔 프로젝트 설정에 남긴 값이 있으면 그것(규칙 apStartSettings, 5-2b). 없으면 지난 실행 그대로(5-2a)
        let pick = apStartSettings(project: ps, past: pr)
        settingsFrom = pick.from
        if pick.from == "project" {
            if let m = pick.model { settings["model"] = m }
            var sl = settings["stopline"] as? [String: Any] ?? [:]
            if let w = pick.week { sl["seven_day"] = w }
            if let f = pick.five { sl["five_hour"] = f }
            if !sl.isEmpty { settings["stopline"] = sl }
        }
        // 게시 방식(4단계 ⑩-1) — 프로젝트 설정에 있으면 그것, 없으면 지난 실행의 것, 그것도 없으면 검수 뒤 올림
        let pubRaw = ps["publish"] ?? pr["publish"]
        var pp = apPublishParse(pubRaw is NSNull ? nil : pubRaw).publish
        // 알림 webhook(4단계 ⑩-3) — 정한 적이 없으면(⑩-1 까지의 설정 포함) 지금 있는 알림 webhook 모두. 출발하면 프로젝트 설정에 남는다
        if pp.notify == nil { APConnections.shared.loadOnce(); pp.notify = APConnections.shared.webhooks.filter { $0.scopes.contains("notify") }.map(\.id) }
        publish = pp; publishEdited = false; publishProblems = []
        pubService = pp.report?.service.rawValue ?? "none"; pubSpace = pp.report?.space ?? ""; pubParent = pp.report?.parent ?? ""
        settings["publish"] = apPublishJSON(pp)
        // 이어 간 줄 밖의 열린 미룬 몫(5-2b) — 같은 프로젝트의 다른 실행이 미뤄 아직 정함·닫음으로 답하지 않은 것. 보이고, 고르면 출발 카드로
        let pc = APRecords.projectCarry(db, project: proj, from: runId)
        openParked = apOpenParked(rows: pc.rows, answers: pc.answers, chain: pc.chain, shown: carryPark.map { (id: $0.id, title: $0.title) })
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
            APPrepLine(id: "misses", title: "판정 놓침(이 프로젝트)"),
            APPrepLine(id: "revise", title: "세트 이어 쓰기"),
            APPrepLine(id: "check", title: "단위마다 기계 검사 · 엔진 출발 전 확인"),
            APPrepLine(id: "env", title: "시험 실행기 환경 확인"),
            APPrepLine(id: "where", title: "따로 작업 트리 확인"),
            APPrepLine(id: "prerun", title: "판정 미리 돌리기"),
            APPrepLine(id: "probe", title: "반대 탐침"),
            APPrepLine(id: "critic", title: "판정 비판 세션"),
        ]
        if cand.isEmpty {
            for l in ["misses", "revise", "env", "where", "prerun", "probe", "critic"] { setLine(l, "skip", note: "") }
            setLine("check", "skip", note: "이어 갈 단위가 없습니다")
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
                    // 판정 놓침(⑪-3) → 있으면 세트 이어 쓰기(판정 명령·지시를 고침) → 엔진 확인·판정 미리 돌리기·반대 탐침·비판 세션
                    self.runMisses {
                        self.runRevise {
                            self.writeDraft(self.units.filter { u in cand.contains { $0.id == u.id } && u.blocked == nil })
                            self.runStatus(prerun: true, ids: Set(cand.filter { $0.blocked == nil }.map(\.id)), critic: true)
                        }
                    }
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
        return (repos, repoInfos(repos))
    }
    // 주제로 시작(⑪-2): 작업 경로(git 뿌리)로 바로 — 단위가 아직 없다
    nonisolated static func readRepos(roots: [String]) -> (repos: [String], info: [String: APRepoInfo]) {
        let repos = roots.compactMap { r in apGit(["rev-parse", "--show-toplevel"], r).flatMap { $0.isEmpty ? nil : $0 } }
        return (repos, repoInfos(repos))
    }
    nonisolated static func repoInfos(_ repos: [String]) -> [String: APRepoInfo] {
        var info: [String: APRepoInfo] = [:]
        for r in repos {
            var i = APRepoInfo()
            i.current = apGit(["symbolic-ref", "-q", "--short", "HEAD"], r).flatMap { $0.isEmpty ? nil : $0 }
            for line in (apGit(["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/heads/"], r) ?? "").split(separator: "\n") {
                let f = line.split(separator: "\t")
                if f.count == 2, f[0].hasPrefix("refs/heads/") { let b = String(f[0].dropFirst(11)); i.branches.append(b); i.heads[b] = String(f[1]) }
            }
            i.dirty = (apGit(["status", "--porcelain", "--ignore-submodules=all"], r) ?? "").split(separator: "\n").map(String.init)
            let names = apBranchRoles(i.branches)
            if let dev = names.dev, let live = names.live { i.devAhead = Int(apGit(["rev-list", "--count", "\(live)..\(dev)"], r) ?? "") }
            info[r] = i
        }
        return info
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
            let n = apBranchRoles(i.branches)
            if let b = last[r] as? String, i.branches.contains(b) { bases[r] = b; notes.append("지난 실행에서 고른 가지(\(b))") }
            else if let b = pastBases[r], i.branches.contains(b) { bases[r] = b; notes.append("지난 실행에서 고른 가지(\(b))") }
            else if let b = n.live ?? n.dev ?? n.master ?? i.current { bases[r] = b; notes.append("처음이라 master 바로 앞 가지") }
        }
        baseNote = notes.first.map { "기본: " + $0 } ?? ""
        if let p = ps["promotion"] as? [[String: Any]], !p.isEmpty {
            promotion = p.compactMap { s in (s["from"] as? String).flatMap { f in (s["into"] as? String).map { APStep(from: f, into: $0, after: s["after"] as? String) } } }
            promotionSaved = true
        } else {
            promotion = apDefaultPromotion(repos.first.flatMap { repoInfo[$0]?.branches } ?? [])
            promotionSaved = false
        }
    }

    // 출발 전 실행 폴더를 쓴다 — 처음에는 drafts/<실행 id>/(runs/ 를 1초마다 읽는 보드 읽기와 섞이지 않게), 출발할 때 runs/ 로 옮긴다
    private func writeDraft(_ list: [APDraftUnit], extra: [String: Any] = [:]) {
        assignId()
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let prev = apRead(dir + "/run.json") as? [String: Any]
        var run = settings
        for (k, v) in extra { run[k] = v }
        if source == "topic" {
            run["source"] = "topic"
            if let sd = prev?["seed"] { run["seed"] = sd }   // 엔진(seed)이 쓴 출발 전 자율주행 가지 기록은 그대로 잇는다(5단계 ⑪-2)
        } else if isImport {
            run["source"] = source   // 가져오기(7단계 ⑫-7a): todo·list·legacy, 이어 간 실행은 없다
        } else {
            run["source"] = "continue"
            run["continues"] = pastId ?? ""
        }
        writeRunRest(&run, list)
    }
    // 실행 id 와 출발 전 폴더 — 처음이거나 앞 폴더로 엔진이 출발해 버렸으면 새로(id 는 <yyyyMMdd-HHmm>-<프로젝트>)
    private func assignId() {
        if id.isEmpty || needNewId {
            let oldDir = dir
            defer {   // 앞 폴더에서 이미 만든 티켓의 기록은 새 폴더로 옮긴다(다시 누르면 남은 것만 만들게, ⑩-3)
                if !oldDir.isEmpty, oldDir != dir, FileManager.default.fileExists(atPath: oldDir + "/tickets.json") {
                    try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
                    try? FileManager.default.copyItem(atPath: oldDir + "/tickets.json", toPath: dir + "/tickets.json")
                }
            }
            let stamp = formatter("yyyyMMdd-HHmm").string(from: Date())
            let base = "\(stamp)-" + String((project ?? "run").map { $0.isLetter || $0.isNumber || "._-".contains($0) ? $0 : "-" }).prefix(40)
            var cand = base, k = 2
            while FileManager.default.fileExists(atPath: apDraftsRoot + "/" + cand) || FileManager.default.fileExists(atPath: autopilotRoot + "/runs/" + cand) {
                cand = "\(base)-\(k)"; k += 1
            }
            id = cand
            dir = apDraftsRoot + "/" + cand
            needNewId = false
            if oldDir.isEmpty, let t = ticketFrom, FileManager.default.fileExists(atPath: t) {   // 앞 준비가 만든 티켓 기록을 물려받는다
                try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
                try? FileManager.default.copyItem(atPath: t, toPath: dir + "/tickets.json")
            }
        }
    }
    private func writeRunRest(_ run: inout [String: Any], _ list: [APDraftUnit]) {
        if jira, !epic.isEmpty { run["epic"] = epic }
        if !missesUsed.isEmpty { run["misses_used"] = missesUsed }   // 이 준비의 세션(초안·비판·세트 이어 쓰기)에 넘긴 판정 놓침 — 출발하면 엔진이 used 로(⑪-3)
        let chosenBases = repos.compactMap { r in bases[r].map { (r, $0) } }
        if Set(chosenBases.map(\.1)).count == 1, chosenBases.count == repos.count, let b = chosenBases.first?.1 { run["base_branch"] = b }
        else if !chosenBases.isEmpty { run["base_branch"] = Dictionary(chosenBases, uniquingKeysWith: { a, _ in a }) }
        apWriteJSON(dir + "/run.json", run)
        apWriteJSON(dir + "/queue.json", queueJSON(list))
        // 만들 티켓 계획(4단계 ⑩-3) — Jira 연결 실행이고 만들 것이 있을 때만. 상태(tickets.json)는 엔진이 쓴다
        let plan = ticketPlan(list)
        if jira, !plan.entries.isEmpty { apWriteJSON(dir + "/tickets-plan.json", apTicketPlanJSON(plan)) }
        else { try? FileManager.default.removeItem(atPath: dir + "/tickets-plan.json") }
    }

    // queue.json — 고른 차례대로. 대기열에 없는 단위를 가리키는 depends 는 뺀다(엔진은 앞선 단위 id 만 받는다. 어긋남은 단위 제안에서 경고만 했다).
    // Jira 단위는 지금 스토리 키와 작업 키, 작업 가지(branch)는 엔진이 프로젝트 설정의 꼴로 다시 채운다
    private func queueJSON(_ list: [APDraftUnit]) -> [[String: Any]] {
        var ids = Set<String>(), out: [[String: Any]] = []
        for u in list {
            guard var d = (try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any] else { continue }
            let deps = u.depends.filter { ids.contains($0) }
            if deps.isEmpty { d.removeValue(forKey: "depends") } else { d["depends"] = deps }
            if let s = u.story { d["story"] = storyKey(s); d["tickets"] = [taskKey(u)] }
            // 단위 추정 주간 %p(보정한 추정, 6단계 ⑫-6): 엔진이 멈출 선 자리 크기로 쓴다(없으면 단위 주간 %p 상한)
            if let pp = estimate(u).pp, pp > 0 { d["est_pp"] = (pp * 10).rounded() / 10 } else { d.removeValue(forKey: "est_pp") }
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
    // critic: 판정 미리 돌리기 뒤 비판 세션도(출발 준비와 판정 고치기 차례. 출발 확인의 다시 확인은 앞 비판 결과를 그대로 둔다 — 모델 비용, 출발마다 한 번)
    private func runStatus(prerun: Bool, ids: Set<String>, critic: Bool = false) {
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
                    self.setEnvLine(j?["env"] as? [[String: Any]] ?? [], units: j?["units"] as? [[String: Any]] ?? [])
                    self.criterionCards = (j?["criteria_gaps"] as? [[String: Any]] ?? []).compactMap(APStart.criterionCard)
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
                        self.runPrerun(d, ids: ids, byId: byId, critic: critic)
                        return
                    }
                    if prerun {
                        self.setLine("prerun", "skip", note: "출발 전 확인에 고칠 것이 있어 돌리지 않았습니다")
                        self.setLine("probe", "skip", note: "출발 전 확인에 고칠 것이 있어 돌리지 않았습니다")
                        self.setLine("critic", "skip", note: "출발 전 확인에 고칠 것이 있어 돌리지 않았습니다")
                    }
                    self.finishCheck()
                }
            }
        }
    }

    // 판정 미리 돌리기 — 단위마다 판정 시간 상한까지 걸릴 수 있어 확인 줄과 따로 알린다
    private func runPrerun(_ d: String, ids: Set<String>, byId: [String: String], critic: Bool = false) {
        let dev = apDevBuild, keep = keepHistory
        keepHistory = false   // 판정 고치기 바로 뒤 한 번만(다시 확인마다 앞 탐침 카드를 쌓지 않게)
        if critic { setLine("probe", "run", note: "") }
        queue.async {
            let pre = apEngine(["prerun", d] + (dev ? ["--dev"] : []) + (keep ? ["--keep-history"] : []))
            let pj = apRead(d + "/prerun.json") as? [String: Any]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { return }
                    if pre.code != 0 || pj == nil {
                        self.setLine("prerun", "fail", note: "판정 미리 돌리기가 실패했습니다", detail: [apShort(pre.out, 400)])
                    } else if let pj {
                        let rows = pj["units"] as? [[String: Any]] ?? []
                        let passed = Set(rows.filter { $0["passed"] as? Bool == true }.compactMap { $0["id"] as? String })
                        self.scenarioCards = (pj["cards"] as? [[String: Any]] ?? []).filter { $0["scenario"] != nil }.compactMap(APStart.scenarioCard)
                        let ran = rows.filter { $0["ran"] as? Bool == true }.count
                        let skipped = rows.filter { $0["skipped"] != nil }.count
                        self.prerunPassed = passed.union(self.prerunPassed.subtracting(ids))   // 이번에 돌리지 않은 단위의 앞 결과는 둔다
                        self.prerunIds.formUnion(ids)
                        var note = "단위 \(rows.count)개 가운데 실패 \(ran - passed.count)(예상대로)"
                        if !passed.isEmpty { note += " · 일하기 전부터 통과 \(passed.count): " + passed.compactMap { byId[$0] }.joined(separator: ", ") + "(출발 카드로)" }
                        if skipped > 0 { note += " · 미리 돌리지 않음 \(skipped)" }
                        // 시나리오(⑪-1): 이미 통과한 시나리오 수와 시나리오마다 결과·증거 자리, 판정 샌드박스 한 줄
                        let already = rows.flatMap { $0["already"] as? [String] ?? [] }
                        if !already.isEmpty { note += " · 이미 통과한 시나리오 \(already.count)(출발 카드로)" }
                        // 주제로 시작(⑪-2): 뼈대·시나리오만 있는 상태라 시나리오가 모두 실패해야 맞다(통과하는 것은 위 카드)
                        let nScen = rows.reduce(0) { $0 + (($1["scenarios"] as? [Any])?.count ?? 0) }
                        if self.source == "topic", nScen > 0 { note += already.isEmpty ? " · 시나리오 \(nScen)개 모두 실패(아직 일하기 전이라 맞습니다)" : " · 시나리오 \(nScen)개 가운데 \(already.count)개가 이미 통과" }
                        var detail = rows.compactMap { r in (r["skipped"] as? String).map { "\(r["title"] ?? ""): \($0)" } }
                        for r in rows {
                            for sc in r["scenarios"] as? [[String: Any]] ?? [] {
                                let ok = sc["passed"] as? Bool == true
                                detail.append("\(r["title"] ?? "") · 시나리오 \(sc["file"] ?? "")\((sc["criterion"] as? String).map { "(수용 기준 \($0))" } ?? ""): "
                                              + (ok ? "이미 통과" : "실패(종료 코드 \(sc["exit"] ?? "?"), 일하기 전이라 맞습니다)")
                                              + ((sc["screenshots"] as? [String]).map { $0.isEmpty ? "" : " · 스크린샷 \($0.count)" } ?? "")
                                              + " · 증거 prerun/\(r["n"] ?? "")/\(sc["evidence"] ?? "")")
                            }
                        }
                        if let sb = rows.first(where: { $0["sandbox"] != nil })?["sandbox"] as? String { detail.append(sb) }
                        self.prerunNote = note
                        self.setLine("prerun", "done", note: note, detail: detail)
                        self.setProbeLine(pj)
                        if critic { self.runCritic(d); return }
                    }
                    self.finishCheck()
                }
            }
        }
    }

    // 시험 실행기 환경 확인 줄(⑪-1): 시나리오마다 필요한 도구가 있는지. 없으면 실패 줄과 까닭(엔진이 고칠 것에도 넣어 출발이 잠긴다)
    private func setEnvLine(_ rows: [[String: Any]], units: [[String: Any]]) {
        let n = units.reduce(0) { $0 + (($1["scenarios"] as? [Any])?.count ?? 0) }
        guard !rows.isEmpty || n > 0 else {
            setLine("env", "skip", note: "판정 시나리오가 없습니다(판정 명령만 돌립니다)"); return
        }
        let bad = rows.filter { $0["ok"] as? Bool != true }
        let kinds = Dictionary(grouping: rows, by: { $0["kind"] as? String ?? "shell" }).map { k, v in "\(k == "browser" ? "브라우저" : "셸") \(v.count)" }.sorted()
        let detail = rows.map { r -> String in
            let tools = (r["tools"] as? [[String: Any]] ?? []).map { t in "\(t["need"] ?? "?") \(t["found"] is String ? "있음" : "없음")" }.joined(separator: " · ")
            return "단위 \(r["unit"] ?? "") · \(r["file"] ?? ""): " + ((r["why"] as? String) ?? tools)
        }
        if bad.isEmpty {
            setLine("env", "done", note: "시나리오 \(rows.count)개(\(kinds.joined(separator: " · "))) 모두 도구가 있습니다", detail: detail)
        } else {
            setLine("env", "fail", note: "시나리오 \(bad.count)개를 돌릴 도구가 없습니다. 보드는 도구를 깔지 않습니다", detail: detail)
        }
    }

    nonisolated static func scenarios(_ u: APDraftUnit) -> [[String: Any]] {
        ((try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any])?["scenarios"] as? [[String: Any]] ?? []
    }
    nonisolated static func scenarioCard(_ c: [String: Any]) -> APCard? {
        guard let id = c["id"] as? String, c["unit_id"] is String else { return nil }
        return APCard(id: id, kind: "prerun", label: "판정 미리 돌리기 · 이미 통과한 시나리오", title: c["title"] as? String ?? "",
                      why: c["why"] as? String ?? "",
                      options: [APOption(key: "fix_check", label: "판정을 고치고 다음 출발로", note: "이 단위는 이번 실행에서 빼고 판정(시나리오)을 고칩니다"),
                                APOption(key: "keep", label: "그대로 두기", note: "단위에 넣습니다. 그 기준은 일하지 않아도 통과로 남을 수 있습니다"),
                                APOption(key: "drop", label: "이번 실행에서 빼기")],
                      recommended: c["recommended"] as? String ?? "fix_check", unit: c["unit_id"] as? String)
    }
    nonisolated static func criterionCard(_ g: [String: Any]) -> APCard? {
        guard let id = g["card"] as? String else { return nil }
        let opts = (g["options"] as? [[String: Any]] ?? []).map { APOption(key: $0["key"] as? String ?? "", label: $0["label"] as? String ?? "", note: $0["note"] as? String ?? "") }
        return APCard(id: id, kind: "criterion", label: "실행 파일이 없는 수용 기준", title: g["title"] as? String ?? "", why: g["why"] as? String ?? "",
                      options: opts, recommended: g["recommended"] as? String ?? apCriterionDrop)
    }

    private func finishCheck() {
        let wasPreparing = preparing
        preparing = false
        fixing = false
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
        // 시험 실행기(⑪-1): 이미 통과한 시나리오 카드(빼기·판정 고치기 답이면 그 단위를 뺀다)와 실행 파일이 없는 수용 기준 카드(단위는 남고 기준만)
        for c in scenarioCards where units.contains(where: { $0.id == c.unit && $0.excluded == nil && $0.blocked == nil && !prerunPassed.contains($0.id) }) {
            out.append(c)
        }
        out += criterionCards
        // 판정 비판(⑪-3): 반대 탐침·비판 세션 카드(뺀 단위의 것은 빼고), 약해진 고침 카드
        let live = Set(units.filter { $0.excluded == nil && $0.blocked == nil }.map(\.id))
        out += (probeCards + criticCards).filter { $0.unit.map { live.contains($0) } ?? true }
        if let w = weakCard { out.append(w) }
        if source == "topic" || isImport { out += draftCards + harnessCards }   // 주제로 시작(⑪-2)·가져오기(⑫-7a): 세션이 물은 것·지시가 다루지 않은 하네스 확인 지점
        let outside = openParked.filter { includeParked.contains($0.id) }.map { APCarry(id: $0.id, unit: $0.unit, kind: $0.kind, title: $0.title, why: $0.why) }
        for c in carryPark + outside {
            out.append(APCard(id: c.id, kind: "next_start", label: carryPark.contains(c) ? "지난 실행에서 미룬 사람 몫" : "이어 간 줄 밖에서 미룬 사람 몫", title: c.title,
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
        // 실행 중에 새로 필요해진 티켓(4단계 ⑩-3, 설계 5장 6절) — Jira 연결 실행의 정책 카드. 답은 answers.json 으로 엔진·모델에 간다
        if units.contains(where: { $0.story != nil && $0.excluded == nil }) {
            out.append(APCard(id: apNewTicketCard, kind: "policy", label: "실행 중에 새로 필요해진 티켓",
                              title: "단위가 일하다가 이 단위 티켓 밖의 새 Jira 티켓이 있어야 할 때",
                              why: "실행 중에는 티켓을 만들지 않습니다(출발 때 만든 티켓만 씁니다). 그 부분을 어떻게 할지 정합니다.",
                              options: [APOption(key: "temp", label: "임시 번호로 진행하고 검수 뒤에 만들어 잇기", note: "모델이 TMP-<단위>-<차례> 로 진행하고, 단위가 끝나면 그 스토리 아래 하위 작업으로 만들 글이 '올릴 글'에 쌓입니다. 사람이 검수하고 누를 때만 만듭니다"),
                                        APOption(key: "defer", label: "그 부분은 보류하고 다음 출발로", note: "그 부분은 하지 않고 다음 출발 카드로 넘깁니다")],
                              recommended: "temp", policy: true))
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
        if card.kind == "fix_weak", key == "accept" { acceptFix() }   // 약해진 고침을 받기로 골랐을 때만 바꿔 끼운다(⑪-3)
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
            // 새 스토리 id 는 그 스토리를 연 단위로 정한다(new-<단위 id>) — 같은 단위로 다시 열면 같은 id 라, 앞 준비가 이미 만든 스토리 티켓을
            // 물려받아 다시 만들지 않는다(4단계 ⑩-3). 지금 목록에 같은 id 가 있으면 뒤에 차례를 붙인다
            let base = "new-" + String(u.id.unicodeScalars.map { $0.isASCII && (CharacterSet.alphanumerics.contains($0) || "._-".unicodeScalars.contains($0)) ? Character($0) : "_" }).prefix(40)
            var sid = base, k = 2
            while stories.contains(where: { $0.id == sid }) { sid = "\(base)-\(k)"; k += 1 }
            let s = APStory(id: sid, key: nil, kind: u.kind, title: "")
            stories.append(s)
            target = s.id
        }
        guard let t = target, stories.first(where: { $0.id == t })?.kind == u.kind else { return }
        let group = Set(moveGroup(id).map(\.id))
        for i in units.indices where group.contains(units[i].id) { units[i].story = t }
        stories.removeAll { s in s.isNew && !units.contains { $0.story == s.id } }
    }
    func storyLabel(_ s: APStory) -> String {
        s.key ?? "새 스토리 \((stories.filter(\.isNew).firstIndex(of: s) ?? 0) + 1)"
    }
    // 만들 티켓 — 새 스토리, 작업 키가 없는 단위, 다른 스토리로 옮긴 이미 있는 작업(부모를 바꿔야 함) 가운데 아직 만들지 않은 것(4단계 ⑩-3, 규칙 apTicketsLeft)
    var ticketsToMake: (stories: Int, tasks: Int, moved: Int) {
        let left = apTicketsLeft(ticketPlan(chosen + heldByCards), ticketBook)
        return (left.filter { $0.kind == .story }.count, left.filter { $0.kind == .task }.count, left.filter { $0.kind == .move }.count)
    }
    var epicProject: String { epic.split(separator: "-").first.map(String.init) ?? "" }
    // 만들 티켓 계획(보드가 tickets-plan.json 으로 쓰고 엔진 tickets 명령이 만든다) — 항목 이름은 보드 스토리 id·단위 id·작업 키라 단위 차례가 바뀌어도 같다
    func ticketPlan(_ list: [APDraftUnit]) -> APTicketPlan {
        var p = APTicketPlan(run: id, epic: epic)
        if let it = tickets["issue_types"] as? [String: Any] { p.storyType = it["story"] as? String ?? ""; p.taskType = it["task"] as? String ?? "" }
        func parent(_ sid: String) -> String { stories.first { $0.id == sid }?.key ?? "story:" + sid }
        for st in stories where st.isNew {
            let mine = list.filter { $0.story == st.id }
            guard !mine.isEmpty else { continue }
            let title = st.title.isEmpty ? (mine.count == 1 ? mine[0].title : "\(mine[0].title) 외 \(mine.count - 1)") : st.title
            p.entries.append(APTicketEntry(id: "story:" + st.id, kind: .story, title: title, parent: epic, units: mine.map(\.title)))
        }
        for u in list {
            guard let sid = u.story else { continue }
            if u.task == nil { p.entries.append(APTicketEntry(id: "task:" + u.id, kind: .task, title: u.title, parent: parent(sid), units: [u.title])) }
            else if let k = u.task, u.story != u.origStory { p.entries.append(APTicketEntry(id: "move:" + k, kind: .move, title: u.title, parent: parent(sid), key: k, units: [u.title])) }
        }
        return p
    }
    // 대기열에 넣을 스토리·작업 키 — 이미 있는 키, 만든 키(tickets.json), 아직 없으면 자리표(엔진 status 가 가지 이름 꼴을 미리 보고, start 는 자리표로 출발하지 않음)
    func storyKey(_ sid: String) -> String {
        stories.first { $0.id == sid }?.key ?? ticketBook.entries["story:" + sid].flatMap { $0.state == .made && !$0.key.isEmpty ? $0.key : nil }
            ?? apTicketPlaceholder(project: epicProject, entry: "story:" + sid)
    }
    func taskKey(_ u: APDraftUnit) -> String {
        u.task ?? ticketBook.entries["task:" + u.id].flatMap { $0.state == .made && !$0.key.isEmpty ? $0.key : nil }
            ?? apTicketPlaceholder(project: epicProject, entry: "task:" + u.id)
    }
    // 만들 티켓을 막는 까닭(연결로 만들 수 없음, 요청 0) — nil 이면 출발을 누를 때 만든다
    var ticketBlocker: String? {
        let cn = APConnections.shared, c = cn.conn("atlassian")
        return apTicketBlocker(c, hasSecret: cn.secrets.contains("atlassian"), dev: apDevBuild, now: Date(), tz: .current)
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
        if source == "topic" { reseedTopic(); return }   // 주제로 시작(⑪-2): 출발 전 자율주행 가지를 새 출발 가지에서 다시 만든다
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

    // MARK: 5-2b 이어 간 줄 밖의 열린 미룬 몫 — 고르면 출발 카드로(같은 꼴, 답은 그 미룬 몫의 id 로 남아 다음 출발에서 다시 묻지 않는다)
    func toggleParked(_ id: String) {
        guard !preparing, !starting else { return }
        if includeParked.contains(id) { includeParked.remove(id) } else { includeParked.insert(id) }
        if prepared { buildCards() }
    }

    // MARK: 4단계 ⑩-1 게시 방식 그 자리에서 바꾸기 — 칸 글을 규칙(apPublishParse)으로 다시 읽고, 고칠 것이 없으면 이 준비의 run.json 에
    func setPublish(mode: APPublishMode? = nil, service: String? = nil, comments: Bool? = nil, notify: [String]? = nil) {
        guard !preparing, !starting, !rechecking else { return }
        var d = apPublishJSON(publish)
        if let mode { d["mode"] = mode.rawValue }
        if let comments { d["jira_comments"] = comments }
        if let notify { d["notify"] = notify }
        if let service { pubService = service }
        switch pubService {
        case "confluence": d["report"] = ["service": "confluence", "space": pubSpace.trimmingCharacters(in: .whitespaces), "parent": pubParent.trimmingCharacters(in: .whitespaces)]
        case "notion": d["report"] = ["service": "notion", "parent": pubParent.trimmingCharacters(in: .whitespaces)]
        default: d["report"] = NSNull()
        }
        let (p, probs) = apPublishParse(d)
        publishProblems = probs
        publishEdited = true
        guard probs.isEmpty else { return }
        publish = p
        settings["publish"] = apPublishJSON(p)
        if !dir.isEmpty { writeDraft(chosen + heldByCards) }
    }

    // MARK: 5-2b 설정 줄(모델·멈출 선) 그 자리에서 바꾸기 — 이 준비의 run.json 에 쓰고, 출발하면 프로젝트 설정에 남긴다
    func setModel(_ m: String) {
        applySettings(model: m, week: stop7 / 100, five: stop5 / 100)
    }
    func setStop(week: Int? = nil, five: Int? = nil) {
        applySettings(model: model, week: Double(week ?? Int(stop7.rounded())) / 100, five: Double(five ?? Int(stop5.rounded())) / 100)
    }
    private func applySettings(model m: String, week: Double, five: Double) {
        guard !preparing, !starting, !rechecking else { return }
        let p = apSettingsProblems(model: m, week: week, five: five)
        settingsProblems = p
        guard p.isEmpty else { return }
        let changedModel = m != model
        settings["model"] = m
        var sl = settings["stopline"] as? [String: Any] ?? [:]
        sl["seven_day"] = week
        sl["five_hour"] = five
        settings["stopline"] = sl
        settingsEdited = true
        if changedModel { estimates = calibrate(); buildCards() }   // 추정 보정은 같은 모델의 표본만 쓴다
        if !dir.isEmpty { writeDraft(chosen + heldByCards) }
    }

    // MARK: 5-2b 승격 차례 그 자리에서 바꾸기 — 바꾼 차례는 규칙 apPromotionProblems 로 보고, 고칠 것이 있으면 출발을 잠근다. 출발하면 프로젝트 설정에 덮어쓴다
    // (끝난 같은 프로젝트 실행의 합치기·승격도 그 차례를 따른다)
    var promotionBranches: [String] {
        // 묶음이면 모든 저장소에 있는 가지만(엔진은 받는 가지가 없는 저장소를 건너뛴다 — 차례는 모두에 있는 가지로 짠다)
        let lists = repos.compactMap { repoInfo[$0]?.branches }
        guard var common = lists.first else { return [] }
        for l in lists.dropFirst() { common = common.filter(l.contains) }
        return common.filter { !$0.hasPrefix("autopilot/") }
    }
    var promotionProblems: [String] { apPromotionProblems(promotion, branches: promotionBranches) }
    private func editPromotion(_ f: (inout [APStep]) -> Void) {
        guard !starting else { return }
        if !promotionEdited { promotionBefore = promotion }
        var p = promotion
        f(&p)
        promotion = p
        promotionEdited = true
    }
    func setPromotion(_ i: Int, from: String? = nil, into: String? = nil, after: String?? = nil) {
        editPromotion { p in
            guard i < p.count else { return }
            let s = p[i]
            p[i] = APStep(from: from ?? s.from, into: into ?? s.into, after: after ?? s.after)
        }
    }
    func addPromotionStep() {
        editPromotion { p in
            let used = Set(p.map(\.into))
            let into = promotionBranches.first { !used.contains($0) } ?? promotionBranches.first ?? "dev"
            p.append(APStep(from: p.last?.into ?? "autopilot", into: into, after: p.last?.into))
        }
    }
    func removePromotionStep(_ i: Int) { editPromotion { p in if i < p.count { p.remove(at: i) } } }
    func refillPromotion() { editPromotion { p in p = apDefaultPromotion(promotionBranches) } }
    func undoPromotion() {
        guard promotionEdited else { return }
        promotion = promotionBefore
        promotionEdited = false
    }

    // MARK: 5-2b 자율주행 지시 그 자리에서 고치기 — 세트 private/projects/<프로젝트>/set/autopilot.md 를 고친다(설계 5장 5절: 보드가 출발 확인에서 고친 판).
    // 엔진이 출발 때 고정하고 기록 저장소 sets 에 남긴다(보드는 저장소를 쓰지 않는다). 고치기 전 글은 세트 밖 set-backup/ 에 남긴다(엔진이 고정하지 않는 자리)
    func beginEditSet() { setDraft = setText ?? ""; setNote = nil; editingSet = true }
    func cancelEditSet() { editingSet = false; setNote = nil }
    func saveSet() {
        guard let p = project, !starting else { return }
        if let why = apInstructionSaveProblem(setDraft) { setNote = why; return }
        // 주제로 시작(⑪-2 보안 검토)·가져오기 세션이 쓴 초안 지시(⑫-7a): 세트는 출발을 누를 때만 쓴다. 여기서는 출발 때 쓸 글만 바꾼다(준비를 버리면 세트는 그대로)
        if source == "topic" || (isImport && importDraftedSet) {
            setText = setDraft.trimmingCharacters(in: .whitespacesAndNewlines)
            editingSet = false
            setNote = "저장했습니다. 출발을 누를 때 이 글을 세트에 씁니다(지금 세트는 그대로입니다)."
            return
        }
        let dirSet = autopilotRoot + "/private/projects/\(p)/set", path = dirSet + "/autopilot.md"
        let text = setDraft.trimmingCharacters(in: .whitespacesAndNewlines) + "\n"
        let fm = FileManager.default
        try? fm.createDirectory(atPath: dirSet, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        if let old = fm.contents(atPath: path), String(decoding: old, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines) != setDraft.trimmingCharacters(in: .whitespacesAndNewlines) {
            let bdir = autopilotRoot + "/private/projects/\(p)/set-backup"
            try? fm.createDirectory(atPath: bdir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let b = bdir + "/autopilot-" + formatter("yyyyMMdd-HHmmss").string(from: Date()) + ".md"
            if !fm.createFile(atPath: b, contents: old, attributes: [.posixPermissions: 0o600]) { setNote = "고치기 전 글을 남기지 못해 저장하지 않았습니다(\(apHome(b)))"; return }
        }
        let tmp = path + ".tmp-\(getpid())"
        guard fm.createFile(atPath: tmp, contents: Data(text.utf8), attributes: [.posixPermissions: 0o600]), rename(tmp, path) == 0 else {
            unlink(tmp); setNote = "지시를 쓰지 못했습니다(\(apHome(path)))"; return
        }
        boardLog("자율주행: 자율주행 지시를 고쳤습니다 \(path)")
        setText = text.trimmingCharacters(in: .whitespacesAndNewlines)
        setAt = mtime(path)?.timeIntervalSince1970
        editingSet = false
        setNote = "저장했습니다. 이 판이 다음 출발의 기준이 됩니다(출발할 때 엔진이 고정하고 기록 저장소에 남깁니다)."
    }

    // MARK: 주제로 시작의 2단계 "보드가 준비"(2단계 5-2b → 5단계 ⑪-2) — 링크 읽기(4단계 ⑩-2) → 버전 목록·스택 → 새 폴더 만들기 →
    // 뼈대 만들기(새 폴더, 앱 안 엔진 seed base: 출발 가지에서 autopilot/<실행 id> 를 만들고 공식 생성기·보드 틀로 첫 커밋) 또는 하네스 읽기(기존 저장소) →
    // 초안 세션(엔진 draft: 명세·단위·판정 시나리오·출발 카드·지시 초안) → 기계 검사 → 판정 시나리오 커밋(seed scenarios) → 이어 가기와 같은
    // 단위마다 기계 검사(status --json)·따로 작업 트리 확인·판정 미리 돌리기(새 폴더는 시나리오가 모두 실패해야 맞음). 실패한 줄에서 멈추고 까닭을 보인다
    // 링크 결정만 기다리는 준비(뼈대 전, 읽는 중인 링크 없음)는 1단계에서 고쳐 다시 준비할 수 있다
    var topicRedoable: Bool {
        source == "topic" && preparing && lines.first(where: { $0.id == "skeleton" })?.state == "wait" && !lines.contains { $0.id.hasPrefix("link-") && $0.state == "run" }
    }
    func prepareTopic() {
        guard source == "topic", !preparing || topicRedoable, topic.ready else { return }
        if preparing { preparing = false; topicOtherDone = false }
        let t = topic
        let key = [t.text, t.links.map(\.url).joined(separator: "\n"), t.root, t.pathMode, t.summary()].joined(separator: "\u{1}")
        if prepared, key == topicKey, !dir.isEmpty { step = 2; return }   // 같은 입력이면 다시 준비하지 않는다(초안 세션은 모델 비용이 든다)
        if !dir.isEmpty { dropFolder(); id = ""; dir = "" }               // 다시 준비: 앞 준비 폴더와 그 자율주행 가지는 규칙대로 치운다
        topicKey = key
        step = 2
        preparing = true; prepared = false
        problems = []; prerunPassed = []; units = []; cards = []; draftResult = [:]; draftCards = []; harnessCards = []; scenarioCards = []; criterionCards = []
        probeCards = []; criticCards = []; criticSummary = [:]; weakCard = nil; fixRound = 0; fixing = false; missInfo = [:]; judgeChanges = []; judgeChangeFrom = ""
        missesUsed = []; pendingSet = nil; critRound = 0; keepHistory = false
        let new = t.pathMode == "new"
        topicNew = new
        project = t.projectName
        root = t.root
        assignId()   // 준비 폴더 drafts/<실행 id>/ — 근거 묶음·초안 결과·출발 전 자율주행 가지 이름이 이 id 를 쓴다
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        var ls: [APPrepLine] = []
        topicOtherDone = false
        linkURLs = [:]
        topicBundle = dir + "/context/links"
        if t.links.isEmpty {
            ls.append(APPrepLine(id: "links", title: "링크 읽기", state: "skip", note: "자료 링크가 없습니다"))
        } else {
            try? FileManager.default.createDirectory(atPath: topicBundle, withIntermediateDirectories: true)
            for (i, l) in t.links.enumerated() {
                let v = t.linkView(l)
                let lid = "link-\(i + 1)"
                linkURLs[lid] = l.url
                ls.append(APPrepLine(id: lid, title: "링크 읽기 · \(v.kind) \(apShortURL(l.url))", state: "wait"))
            }
        }
        if new || !t.locked {
            ls.append(APPrepLine(id: "versions", title: "버전 목록 받기", state: t.catalog == nil ? "wait" : "done", sec: 0,
                                 note: t.catalog == nil ? "" : t.catalogLabel + " · " + Set(APFeed.allCases.map(\.label)).sorted().joined(separator: " · ")))
        } else {
            ls.append(APPrepLine(id: "versions", title: "버전 목록 받기", state: "skip", note: "기존 프로젝트의 규격을 따르므로 받지 않습니다"))
        }
        if new {
            ls.append(APPrepLine(id: "stack", title: "기술 스택", state: "done", sec: 0, note: "고른 스택: " + t.summary()))
            ls.append(APPrepLine(id: "folder", title: "새 폴더 만들기", state: "run", started: Date().timeIntervalSince1970))
            ls.append(APPrepLine(id: "skeleton", title: "뼈대 만들기"))
        } else {
            let d = t.detected
            ls.append(APPrepLine(id: "stack", title: "기술 스택 감지", state: t.locked ? "done" : "warn", sec: 0,
                                 note: t.locked ? t.summary() + " (" + [d?.back, d?.front, d?.db].compactMap { $0?.sources.first }.joined(separator: " · ") + ")"
                                                : "감지된 것 없음 · 고른 스택: " + t.summary(),
                                 detail: t.conflicts))
            ls.append(APPrepLine(id: "skeleton", title: "저장소 보기(출발 가지)"))
            ls.append(APPrepLine(id: "harness", title: "하네스 읽기"))
        }
        ls.append(APPrepLine(id: "misses", title: "판정 놓침(이 프로젝트)"))
        ls.append(APPrepLine(id: "draft", title: "초안 세션"))
        ls.append(APPrepLine(id: "draftcheck", title: "초안 기계 검사"))
        ls.append(APPrepLine(id: "scenarios", title: "판정 시나리오 커밋"))
        ls.append(APPrepLine(id: "check", title: "단위마다 기계 검사 · 엔진 출발 전 확인"))
        ls.append(APPrepLine(id: "env", title: "시험 실행기 환경 확인"))
        ls.append(APPrepLine(id: "where", title: "따로 작업 트리 확인"))
        ls.append(APPrepLine(id: "prerun", title: "판정 미리 돌리기"))
        ls.append(APPrepLine(id: "probe", title: "반대 탐침"))
        ls.append(APPrepLine(id: "critic", title: "판정 비판 세션"))
        lines = ls
        for lid in linkURLs.keys.sorted(by: { $0.localizedStandardCompare($1) == .orderedAscending }) { readLink(lid) }
        let finishLater: @MainActor () -> Void = {
            self.topicOtherDone = true
            self.checkTopicDone()
        }
        // 버전 목록이 아직이면 받은 뒤 그 줄을 채우고 끝낸다(새 폴더·감지에 실패한 기존 저장소)
        let afterFolder: @MainActor () -> Void = {
            guard new || !t.locked else { finishLater(); return }
            if t.catalog == nil { self.setLine("versions", "run") }
            t.whenCatalog {
                guard self.source == "topic" else { return }
                self.setLine("versions", t.catalog?.origin == .builtin || t.catalog?.origin == .stale ? "warn" : "done",
                             note: t.catalogLabel + " · " + Set(APFeed.allCases.map(\.label)).sorted().joined(separator: " · "),
                             detail: (t.catalog?.failed ?? []).map { "받지 못함: \($0)" })
                finishLater()
            }
        }
        guard new else { afterFolder(); return }
        t.createRepo { r in
            guard self.source == "topic" else { return }
            if r.ok {
                self.setLine("folder", "done", note: apHome(t.newPlan?.path ?? "") + " · " + r.done.joined(separator: " · "))
                self.root = t.newPlan?.path ?? self.root
                afterFolder()
            } else {
                self.setLine("folder", "fail", note: r.error ?? "새 폴더를 만들지 못했습니다", detail: r.done)
                self.topicStop("새 폴더를 만들지 못해 여기서 멈춥니다")
            }
        }
    }

    // 주제로 시작 준비가 실패한 줄에서 멈춘다 — 뒤 줄은 하지 않음과 까닭. 1단계로 돌아가 고친 뒤 다시 준비한다
    private func topicStop(_ why: String) {
        for i in lines.indices where ["wait", "run"].contains(lines[i].state) && !lines[i].id.hasPrefix("link-") {
            lines[i].state = "skip"; lines[i].note = why
        }
        preparing = false; prepared = false
        topicKey = ""
        if !shown { TerminalHub.shared.toast("자율주행 출발 준비(주제로 시작)가 멈췄습니다: \(why)") }
    }
    // 링크 하나 읽기 — 연결이 없거나 읽을 수 없는 주소는 요청 없이 실패 줄. 읽은 것은 엔진이 근거 묶음에 넣고(형식 검사·비밀 가리기를 지난 것만) 그 요약을 줄에 보인다
    func readLink(_ lid: String) {
        guard let url = linkURLs[lid], !topicBundle.isEmpty else { return }
        let t = topic
        let v = t.linkView(APLink(url: url, kind: "", state: "", why: ""))
        if v.state == "unreadable" || v.state == "needs_connection" {
            setLine(lid, "fail", note: "읽을 수 없음: \(v.why) (요청 0)", detail: [])
            checkTopicDone(); return
        }
        setLine(lid, "wait", note: "", detail: [])
        var args = ["read", topicBundle, "--json"] + (root.isEmpty ? [] : ["--cwd", root]) + (apDevBuild ? ["--dev"] : [])
        if v.state == "mcp", let m = t.linkMCP[url] { args += ["--mcp", m] + APTopic.devMCPArgs + APTopic.devClaudeArgs }
        args.append(url)
        let server = t.linkMCP[url].flatMap { m in t.servers.first { $0.name == m } }
        queue.async {
            DispatchQueue.main.async { MainActor.assumeIsolated { if self.linkURLs[lid] == url { self.setLine(lid, "run") } } }
            let r = apEngine(args)
            let d = ((try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [[String: Any]])?.first ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.source == "topic", self.linkURLs[lid] == url else { return }
                    let note = apDocText(d["note"] as? String ?? (r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : r.out), 300)
                    var detail: [String] = []
                    let req = (d["requests"] as? NSNumber)?.intValue ?? 0
                    if d["via"] as? String == "mcp", let sv = server { detail.append("MCP 서버 \(sv.name)(\(sv.scope)) · \(sv.allow)") }
                    if d["ok"] as? Bool == true {
                        detail.append("근거 묶음 \(d["file"] as? String ?? "")" + ((d["image"] as? String).map { " · 그림 \($0)" } ?? ""))
                        detail.append((d["via"] as? String == "mcp" ? "MCP 읽기 세션" + ((d["cost_usd"] as? NSNumber).map { String(format: " · %.3f USD", $0.doubleValue) } ?? "")
                                       : "요청 \(req)건") + " · 가린 비밀 \((d["redacted"] as? NSNumber)?.intValue ?? 0)개")
                        if let im = d["image_note"] as? String { detail.append(apDocText(im, 200)) }
                        self.setLine(lid, "done", note: "'\(apDocText(d["title"] as? String ?? "", 80))' · \(apDocText(d["summary"] as? String ?? "", 120))", detail: detail)
                    } else {
                        if req > 0 { detail.append("요청 \(req)건") }
                        self.setLine(lid, "fail", note: note, detail: detail)
                    }
                    self.checkTopicDone()
                }
            }
        }
    }
    func retryLink(_ lid: String) { readLink(lid) }
    func skipLink(_ lid: String) {
        setLine(lid, "skip", note: "이 링크를 빼고 계속합니다(근거 묶음에 넣지 않습니다)")
        checkTopicDone()
    }
    var linkUndecided: Bool { lines.contains { $0.id.hasPrefix("link-") && $0.state == "fail" } }
    // 주제로 시작 준비의 앞 줄이 다 끝났나 — 다른 줄이 끝나고, 링크 줄이 모두 끝·뺌이면(실패한 링크는 사람이 다시 읽기나 빼고 계속을 골라야) 뼈대·초안 세션으로
    private func checkTopicDone() {
        guard source == "topic" else { return }
        let pending = lines.contains { $0.id.hasPrefix("link-") && ["wait", "run", "fail"].contains($0.state) }
        if let i = lines.firstIndex(where: { $0.id == "draft" }), lines[i].state == "wait" {
            lines[i].note = pending && !lines.filter({ $0.id.hasPrefix("link-") }).isEmpty
                ? "링크 읽기를 정하면 시작합니다(못 읽은 링크는 다시 읽거나 빼고 계속)" : ""
        }
        guard topicOtherDone, !pending, preparing, lines.first(where: { $0.id == "skeleton" })?.state == "wait" else { return }
        topicChain()
    }

    // MARK: 5단계 ⑪-2 주제로 시작의 뼈대·초안 세션·시나리오 커밋 — 모두 앱 안 엔진(seed·draft)을 백그라운드 한 줄에서 차례로 부른다
    private func topicChain() {
        let t = topic
        topicStack = t.stackSet()
        // 실행 설정: 프로젝트 설정에 남긴 모델·멈출 선이 있으면 그것(규칙 apStartSettings), 없으면 기본(opus · 주간 80% · 5시간 95%)
        let proj = project ?? "topic"
        let ps = apRead(autopilotRoot + "/projects/\(proj).json") as? [String: Any] ?? [:]
        let pick = apStartSettings(project: ps, past: [:])
        settingsFrom = pick.from
        let firstLine = t.text.split(separator: "\n").first.map { String($0).trimmingCharacters(in: .whitespaces) } ?? proj
        settings = ["model": pick.model ?? apDraftModel, "title": String(firstLine.prefix(60)), "project": proj,
                    "stopline": ["seven_day": pick.week ?? 0.80, "five_hour": pick.five ?? 0.95]]
        if apDevBuild {   // 시험 빌드만: 가짜 claude(설정 값 autopilotReadClaude)·생성기 도구 찾기 PATH(autopilotToolPath)·바깥 요청 생성기 허용(autopilotExternalGenerators)
            if let c = UserDefaults.standard.string(forKey: "autopilotReadClaude"), !c.isEmpty { settings["claude_path"] = c }
            if let tp = UserDefaults.standard.string(forKey: "autopilotToolPath"), !tp.isEmpty { settings["tool_path"] = tp }
            if UserDefaults.standard.bool(forKey: "autopilotExternalGenerators") { settings["external_generators"] = true }
        }
        var pp = apPublishParse(ps["publish"]).publish
        if pp.notify == nil { APConnections.shared.loadOnce(); pp.notify = APConnections.shared.webhooks.filter { $0.scopes.contains("notify") }.map(\.id) }
        publish = pp; publishEdited = false; publishProblems = []
        pubService = pp.report?.service.rawValue ?? "none"; pubSpace = pp.report?.space ?? ""; pubParent = pp.report?.parent ?? ""
        settings["publish"] = apPublishJSON(pp)
        lastSet = APRecords.lastSet(db, project: proj).map { APSetVersion(text: $0.text, at: $0.at, run: $0.run) }
        let rootNow = root
        setLine("skeleton", "run")
        queue.async {
            let info = APStart.readRepos(roots: [rootNow])
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.source == "topic", self.preparing else { return }
                    self.repos = info.repos; self.repoInfo = info.info
                    self.pickDefaultBases(self.project, self.settings)
                    if self.topicNew { self.promotion = apDefaultPromotion(self.repoInfo[rootNow]?.branches ?? []) }
                    self.writeTopic()
                    self.seedBase { ok in if ok { self.runMisses { self.runDraft() } } }   // 초안 세션 글에 이 프로젝트의 지난 판정 놓침 요약(⑪-3)
                }
            }
        }
    }

    // topic.json(엔진 seed·draft 가 읽는 주제로 시작 입력)과 run.json(실행 설정·출발 가지) — 준비 폴더에
    private func writeTopic() {
        let t = topic
        let stackJSON = (try? JSONSerialization.jsonObject(with: (try? JSONEncoder().encode(topicStack)) ?? Data())) ?? [:]
        let base = bases[root] ?? "live"
        apWriteJSON(dir + "/topic.json", ["text": t.text, "repo": root, "new": topicNew, "name": project ?? "", "base": base, "stack": stackJSON,
                                         "links": t.links.map(\.url), "layout": topicLayout])
        writeDraft([])
    }

    // 뼈대(새 폴더)·저장소 보기(기존) — 출발 가지에서 autopilot/<실행 id> 와 그 임시 작업 트리. 새 폴더면 생성기 계획의 도구를 먼저 보고 없으면 실패 줄과 까닭
    private func seedBase(_ then: @escaping @MainActor (Bool) -> Void) {
        let d = dir, dev = apDevBuild, new = topicNew
        setLine("skeleton", "run")
        queue.async {
            let r = apEngine(["seed", d, "base"] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d, self.source == "topic" else { return }
                    let plan = o?["plan"] as? [[String: Any]] ?? []
                    var detail = plan.map { "\($0["label"] as? String ?? ""): \($0["command"] as? String ?? "")" + (($0["external"] as? Bool) == true ? " (바깥 요청)" : "") }
                    detail += (o?["notes"] as? [String] ?? [])
                    guard r.code == 0, o?["ok"] as? Bool == true else {
                        let probs = o.map { apStrings($0["problems"]) } ?? [apShort(r.out)]
                        self.setLine("skeleton", "fail", note: probs.first ?? "뼈대를 만들지 못했습니다", detail: Array(probs.dropFirst()) + detail)
                        self.topicStop(o?["step"] as? String == "tools" ? "생성기에 필요한 도구가 없어 여기서 멈춥니다" : "뼈대를 만들지 못해 여기서 멈춥니다")
                        then(false); return
                    }
                    let head = (o?["head"] as? String ?? "").prefix(7)
                    if new {
                        let done = o?["done"] as? [String] ?? []
                        self.setLine("skeleton", "done", note: done.isEmpty ? "만들 뼈대가 없습니다(\((o?["notes"] as? [String])?.first ?? "고른 스택 없음"))"
                                                                : done.joined(separator: " · ") + " · autopilot/\(self.id) 첫 커밋 \(head)",
                                     detail: detail + ["바깥 요청을 한 걸음 \((o?["external"] as? NSNumber)?.intValue ?? 0)개 · 모델 0"])
                    } else {
                        self.setLine("skeleton", "done", note: "출발 가지 \(self.bases[self.root] ?? "?")(\(head))에서 autopilot/\(self.id) 를 만들어 읽기 전용 보기로 둡니다", detail: detail)
                        let n = (o?["harness_points"] as? NSNumber)?.intValue ?? 0
                        self.setLine("harness", n > 0 ? "done" : "skip", note: n > 0 ? "확인 지점 \(n)곳을 찾았습니다(초안 지시가 모두 다뤄야 합니다)" : "하네스 확인 지점이 없습니다")
                    }
                    then(true)
                }
            }
        }
    }

    // 초안 세션 — 엔진 draft(읽기 도구만, 결과 폴더에만 씀) → 기계 검사 결과(draft-check.json)를 줄 둘에. 지나면 시나리오 커밋으로
    private func runDraft() {
        let d = dir, dev = apDevBuild, devClaude = APTopic.devClaudeArgs
        setLine("draft", "run", note: "명세·단위·판정 시나리오·출발 카드·지시 초안을 씁니다(모델 \(settings["model"] as? String ?? apDraftModel), 읽기 도구만 · Bash·웹·MCP 없음)")
        queue.async {
            let r = apEngine(["draft", d] + (dev ? ["--dev"] + devClaude : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d, self.source == "topic" else { return }
                    self.draftResult = o
                    let sec = (o["sec"] as? NSNumber)?.doubleValue ?? 0, usd = (o["cost_usd"] as? NSNumber)?.doubleValue
                    let spec = o["spec"] as? [String: Any] ?? [:]
                    let units = o["units"] as? [[String: Any]] ?? [], cards = o["cards"] as? [[String: Any]] ?? []
                    let note = o["note"] as? String ?? (r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : apShort(r.out))
                    let sessionFailed = o.isEmpty || o["session"] as? Bool == false
                    self.setLine("draft", sessionFailed ? "fail" : "done",
                                 note: sessionFailed ? note : "요구사항 \((spec["requirements"] as? [Any])?.count ?? 0) · 수용 기준 \((spec["criteria"] as? [Any])?.count ?? 0) · 단위 \(units.count) · 시나리오 \((o["scenarios"] as? NSNumber)?.intValue ?? 0) · 카드 \(cards.count) · 지시 \((o["instruction_chars"] as? NSNumber)?.intValue ?? 0)자",
                                 detail: [String(format: "%.0f초", sec) + (usd.map { String(format: " · %.3f USD", $0) } ?? "") + " · 모델 \(o["model"] as? String ?? "?")"])
                    if sessionFailed { self.setLine("draftcheck", "skip", note: "초안 세션이 결과를 쓰지 못했습니다"); self.topicStop("초안 세션이 끝나지 않아 여기서 멈춥니다"); return }
                    let probs = apStrings(o["problems"]), warns = apStrings(o["warnings"])
                    guard o["ok"] as? Bool == true else {
                        self.setLine("draftcheck", "fail", note: "고칠 것 \(probs.count)개: \(probs.first ?? "")", detail: probs + warns)
                        self.topicStop("초안 결과가 기계 검사를 지나지 않아 여기서 멈춥니다(1단계에서 주제를 고쳐 다시 준비합니다)")
                        return
                    }
                    let missing = o["harness_missing"] as? [[String: Any]] ?? []
                    self.setLine("draftcheck", warns.isEmpty ? "done" : "warn",
                                 note: "칸·상한·HTML 없음·비밀 꼴 없음·수용 기준마다 시나리오나 카드" + ((o["harness_points"] as? NSNumber)?.intValue ?? 0 > 0 ? " · 하네스 확인 지점 \(missing.isEmpty ? "모두 다룸" : "\(missing.count)곳 빠짐(출발 카드로)")" : ""),
                                 detail: warns)
                    self.draftCards = cards.filter { $0["criterion"] == nil }.compactMap(APStart.draftCard)
                    self.harnessCards = missing.compactMap(APStart.harnessCard)
                    self.setText = (try? String(contentsOfFile: d + "/draft/autopilot.md", encoding: .utf8)).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.flatMap { $0.isEmpty ? nil : $0 }
                    self.setAt = Date().timeIntervalSince1970
                    // 지금 세트에 지시가 있으면 그것과 견준다(출발을 눌러야 세트를 바꾸므로, 무엇이 바뀌는지 출발 확인에서 본다. ⑪-2 보안 검토)
                    let cur = autopilotRoot + "/private/projects/\(self.project ?? "")/set/autopilot.md"
                    if let t = (try? String(contentsOfFile: cur, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty {
                        self.lastSet = APSetVersion(text: t, at: mtime(cur)?.timeIntervalSince1970 ?? 0, run: "지금 세트")
                    }
                    self.seedScenarios { ok in if ok { self.topicCheck() } }
                }
            }
        }
    }

    // 판정 시나리오 커밋 — 기계 검사를 지난 시나리오를 autopilot/<실행 id> 의 .autopilot/scenarios/<단위 id>/ 에(뼈대 다음 커밋). 임시 작업 트리는 엔진이 지운다
    private func seedScenarios(_ then: @escaping @MainActor (Bool) -> Void) {
        let d = dir, dev = apDevBuild
        setLine("scenarios", "run")
        queue.async {
            let r = apEngine(["seed", d, "scenarios"] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d, self.source == "topic" else { return }
                    guard r.code == 0, o?["ok"] as? Bool == true else {
                        let probs = o.map { apStrings($0["problems"]) } ?? [apShort(r.out)]
                        self.setLine("scenarios", "fail", note: probs.first ?? "시나리오를 커밋하지 못했습니다", detail: Array(probs.dropFirst()))
                        self.topicStop("판정 시나리오를 커밋하지 못해 여기서 멈춥니다"); then(false); return
                    }
                    let n = (o?["scenarios"] as? NSNumber)?.intValue ?? 0
                    self.setLine("scenarios", "done", note: n == 0 ? "시나리오가 없습니다(판정 명령만)" : "시나리오 \(n)개를 autopilot/\(self.id) 에 커밋했습니다(\(self.topicNew ? "뼈대 다음" : "출발 가지 다음") · \(((o?["head"] as? String) ?? "").prefix(7)))",
                                 detail: ["출발 가지 위 커밋 \(o?["commits"] ?? "?")개 · 출발하면 엔진이 이 가지를 그대로 이어받고, 시나리오는 고정본으로 판정합니다"])
                    then(true)
                }
            }
        }
    }

    // 초안 단위(대기열 꼴) → 출발 준비의 단위, 그 뒤는 이어 가기와 같은 확인(status --json)·판정 미리 돌리기
    private func topicCheck() {
        // 초안 단위는 엔진이 허용 목록 칸만 내놓지만, 샌드박스 밖에서 돌거나 범위를 넓히는 칸은 여기서 한 번 더 버린다(⑪-2 보안 검토, 작업 폴더·근거 묶음은 엔진이 정한 값)
        let qs = (draftResult["units"] as? [[String: Any]] ?? []).map { $0.filter { k, _ in k == "cwd" || k == "context" || !apDraftWideningKeys.contains(k) } }
        units = qs.enumerated().map { i, d in
            var u = APDraftUnit(id: d["id"] as? String ?? "u\(i + 1)")
            u.n = i + 1
            u.title = d["title"] as? String ?? ""; u.kind = d["kind"] as? String ?? ""; u.check = d["check"] as? String ?? ""
            u.sourceKey = d["source_key"] as? String ?? ""
            u.est = apNum(d["est_min"])
            u.depends = d["depends"] as? [String] ?? []
            u.last = "새 단위"
            u.def = (try? JSONSerialization.data(withJSONObject: d, options: [.sortedKeys])) ?? Data()
            return u
        }
        estimates = calibrate()
        setLine("check", "run")
        writeDraft(units)
        runStatus(prerun: true, ids: Set(units.map(\.id)), critic: true)
    }

    // 초안 세션 카드(모호한 점, 객관식) → 출발 카드. 답은 answers.json 으로 엔진·모델에 간다(단위를 빼지는 않는다)
    nonisolated static func draftCard(_ c: [String: Any]) -> APCard? {
        guard let id = c["id"] as? String else { return nil }
        let opts = (c["options"] as? [[String: Any]] ?? []).map { APOption(key: $0["key"] as? String ?? "", label: $0["label"] as? String ?? "", note: $0["note"] as? String ?? "") }
        return APCard(id: "draft-" + id, kind: "draft", label: "초안 세션이 물음", title: c["title"] as? String ?? "", why: c["why"] as? String ?? "",
                      options: opts, recommended: c["recommended"] as? String ?? "")
    }
    // 지시가 다루지 않은 하네스 확인 지점 → 출발 카드(막지 않고 묻는다)
    nonisolated static func harnessCard(_ m: [String: Any]) -> APCard? {
        guard let id = m["card"] as? String else { return nil }
        let opts = (m["options"] as? [[String: Any]] ?? []).map { APOption(key: $0["key"] as? String ?? "", label: $0["label"] as? String ?? "", note: $0["note"] as? String ?? "") }
        return APCard(id: id, kind: "harness", label: "하네스 확인 지점", title: m["title"] as? String ?? "", why: m["why"] as? String ?? "",
                      options: opts, recommended: m["recommended"] as? String ?? "defer")
    }

    // 출발 가지를 바꾸면(기존 저장소) 출발 전 자율주행 가지를 새 출발 가지에서 다시 만든다 — 뼈대(새 폴더)·시나리오 커밋을 다시 하고(모델 0) 확인·미리 돌리기
    func reseedTopic() {
        guard source == "topic", !preparing, !starting, !dir.isEmpty else { return }
        topicReseeding = true; rechecking = true
        writeTopic()
        seedBase { ok in
            guard ok else { self.topicReseeding = false; self.rechecking = false; return }
            self.seedScenarios { ok2 in
                self.topicReseeding = false
                guard ok2 else { self.rechecking = false; return }
                self.rechecking = false
                self.recheck(prerun: true)
            }
        }
    }

    // 새 폴더의 가지 꼴(출발 확인에서 출발 전에 바꿈): dev·live·master(기본) 또는 main 하나. 없는 가지는 빈 첫 커밋에 만들고(지우지는 않음),
    // 승격 차례를 그 꼴로 채운 뒤 출발 가지(master 바로 앞)에서 자율주행 가지를 다시 만든다
    func setTopicLayout(_ k: String) {
        guard source == "topic", topicNew, k != topicLayout, !preparing, !starting, !rechecking, let r = repos.first else { return }
        let names = k == "main" ? ["main"] : ["dev", "live", "master"]
        let rootCommit = apGit(["rev-list", "--max-parents=0", "HEAD"], r)?.split(separator: "\n").first.map(String.init) ?? ""
        for b in names where !(repoInfo[r]?.branches.contains(b) ?? false) { _ = apGit(["branch", b, rootCommit], r) }
        topicLayout = k
        let info = APStart.readRepos(roots: [r])
        repoInfo = info.info
        promotion = apDefaultPromotion(names)
        promotionEdited = true
        bases[r] = k == "main" ? "main" : "live"
        reseedTopic()
    }

    // 에픽 칸 — 번호 꼴이 맞고 Jira 연결 프로젝트면 0.5초 뒤 앱 안 엔진이 제목·상태를 읽는다(연결이 없으면 요청 0, 번호 꼴만)
    func refreshEpic() {
        epicWork?.cancel()
        let k = epic.trimmingCharacters(in: .whitespaces)
        guard jira, k.range(of: "^[A-Z][A-Z0-9_]*-[0-9]+$", options: .regularExpression) != nil else { epicInfo = nil; return }
        if epicInfo?.key == k, epicInfo?.loading == false { return }
        epicInfo = APEpicSeen(key: k, loading: true)
        let dev = apDevBuild
        let w = DispatchWorkItem {
            let r = apEngine(["read", "epic", k] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.epic.trimmingCharacters(in: .whitespaces) == k else { return }
                    self.epicInfo = APEpicSeen(key: k, read: o["read"] as? Bool ?? false, found: o["found"] as? Bool ?? false, isEpic: o["epic"] as? Bool ?? false,
                                               title: apDocText(o["title"] as? String ?? "", 200), status: apDocText(o["status"] as? String ?? "", 80),
                                               note: apDocText(o["note"] as? String ?? r.out, 300), loading: false)
                }
            }
        }
        epicWork = w
        DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 0.5, execute: w)
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
        if promotionEdited, !promotionProblems.isEmpty { l.append("바꾼 승격 차례에 고칠 것이 \(promotionProblems.count)개 있습니다") }
        if editingSet { l.append("자율주행 지시를 고치는 중입니다. 저장하거나 그만둔 뒤 출발합니다") }
        if topicReseeding { l.append("출발 전 자율주행 가지를 새 출발 가지에서 다시 만드는 중입니다") }
        if case .offer(let k)? = fixNext, !fixPicked.isEmpty {
            l.append("판정 고치기를 고른 카드 \(fixPicked.count)장이 있습니다. 출발 카드 상자의 '판정 고치기 \(k)차 돌리기'를 누르거나 답을 그대로·빼기로 바꿉니다")
        }
        if !publishProblems.isEmpty { l.append("게시 방식의 받을 곳에 고칠 것이 \(publishProblems.count)개 있습니다") }
        if jira {
            if epic.isEmpty { l.append("에픽 번호를 넣어 주세요(출발 준비 1단계)") }
            else if epic.range(of: "^[A-Z][A-Z0-9_]*-[0-9]+$", options: .regularExpression) == nil { l.append("에픽 번호 꼴이 아닙니다(예: SHOP-10)") }
            else if let e = epicInfo, e.key == epic, e.read, !(e.found && e.isEpic) { l.append(e.found ? "에픽이 아닌 번호입니다: \(e.note)" : "없는 에픽 번호입니다(\(e.key))") }
            // 만들 티켓(4단계 ⑩-3): 출발을 누를 때 엔진이 만든다. 연결로 만들 수 없으면(요청 0) 잠그고, 확인 필요는 사람이 확인한 뒤에만
            let left = apTicketsLeft(ticketPlan(chosen + heldByCards), ticketBook)
            if !left.isEmpty, let why = ticketBlocker {
                l.append("만들 티켓 \(left.count)건을 만들 수 없습니다: \(why). 설정 › 연결에서 Atlassian 을 연결하거나 이미 있는 티켓의 단위만 고릅니다")
            }
            let held = left.filter { ticketBook.entries[$0.id]?.state == .uncertain && !ticketConfirm.contains($0.id) }
            if !held.isEmpty { l.append("확인 필요 티켓 \(held.count)개: 앞 만들기가 끝났는지 모릅니다. Jira 에서 확인한 뒤 만들 티켓 나무의 '그래도 만들기'를 누릅니다") }
        }
        return l
    }

    // 출발 — 실행 폴더를 마저 쓰고(고른 단위·출발 가지·카드와 답), 만들 티켓이 있으면 먼저 엔진 tickets 로 한꺼번에 만들고(4단계 ⑩-3, 게시 방식과 상관없이)
    // 만든 키를 대기열에 넣은 뒤 runs/ 로 옮겨 엔진 start. 티켓을 다 만들지 못하면 출발하지 않고 실패한 티켓과 까닭을 나무에 보인다(다시 누르면 남은 것만).
    // 거부되면 그 문장을 그대로 보이고 준비는 남긴다
    func start() {
        guard locks.isEmpty, !starting else { return }
        starting = true
        startError = nil
        let list = chosen
        if (apRead(dir + "/run.json") as? [String: Any])?["state"] != nil { needNewId = true }
        if source == "topic", needNewId {   // 출발 전 자율주행 가지 이름이 이 준비 id 에 묶여 있다(⑪-2)
            starting = false
            startError = "엔진이 이 준비 폴더로 이미 출발을 시도해 같은 준비로 다시 출발할 수 없습니다. 1단계로 돌아가 다시 준비합니다."
            return
        }
        writeDraft(list)
        let plan = ticketPlan(list)
        guard jira, !apTicketsLeft(plan, ticketBook).isEmpty else { launch(list); return }
        let d = dir, dev = apDevBuild, confirm = ticketConfirm.sorted()
        ticketNote = ("티켓 \(apTicketsLeft(plan, ticketBook).count)개를 만드는 중입니다…", false)
        queue.async {
            let r = apEngine(["tickets", d, "make", "--json"] + (confirm.isEmpty ? [] : ["--confirm", confirm.joined(separator: ",")]) + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any]
            let book = apTicketBookParse(FileManager.default.contents(atPath: d + "/tickets.json"))
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { self.starting = false; return }
                    self.ticketBook = book
                    self.ticketConfirm = self.ticketConfirm.filter { book.entries[$0]?.state == .uncertain }
                    let left = apTicketsLeft(plan, book)
                    let note = apDocText(o?["note"] as? String ?? (r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : r.out), 400)
                    let req = (o?["requests"] as? NSNumber)?.intValue ?? 0
                    let probs = (o?["problems"] as? [Any] ?? []).compactMap { $0 as? String }
                    self.ticketNote = (note + " (요청 \(req)건)" + (probs.isEmpty ? "" : ": " + probs.prefix(3).joined(separator: " · ")), !left.isEmpty)
                    guard left.isEmpty else {
                        self.starting = false
                        self.startError = "티켓을 다 만들지 못해 출발하지 않았습니다(남은 것 \(left.count)개). 만든 티켓은 그대로 두고, 출발을 다시 누르면 남은 것만 만듭니다."
                        boardLog("자율주행 \(self.id): 티켓 만들기 남은 것 \(left.count)개 · \(note)")
                        return
                    }
                    self.writeDraft(list)   // 만든 키로 대기열을 다시 쓴다(자리표가 남으면 엔진이 출발하지 않는다)
                    self.launch(list)
                }
            }
        }
    }
    private func launch(_ list: [APDraftUnit]) {
        apWriteJSON(dir + "/cards.json", cards.map(\.json))
        apWriteJSON(dir + "/answers.json", answers)
        if source == "topic", let why = writeTopicSet() { starting = false; startError = why; return }
        if source != "topic", pendingSet != nil, let why = writeRevisedSet() { starting = false; startError = why; return }
        if isImport {   // 가져오기(⑫-7a): 세션이 쓴 초안 지시는 출발할 때만 세트에, 고른 단위·고르지 않은 후보는 가져오기 줄로(엔진 start 가 기록 저장소에)
            if importDraftedSet, let t = setText, !t.isEmpty, let why = writeSetFiles([("autopilot.md", Data((t + "\n").utf8))]) { starting = false; startError = why; return }
            writeImportQueue()
        }
        let src = dir, dst = autopilotRoot + "/runs/" + id, dev = apDevBuild, runId = id, topicRun = source == "topic" || isImport
        queue.async {
            var moved = src == dst
            if !moved {
                try? FileManager.default.createDirectory(atPath: autopilotRoot + "/runs", withIntermediateDirectories: true)
                moved = rename(src, dst) == 0
            }
            // 주제로 시작(⑪-2): 단위 근거 묶음(context)은 준비 폴더 안 context/links 라, 옮긴 자리로 고쳐 쓴다(엔진이 출발 때 pinned/context/<n>/ 로 사본을 뜬다)
            if moved, topicRun, src != dst, var q = apRead(dst + "/queue.json") as? [[String: Any]] {
                for i in q.indices { if let c = q[i]["context"] as? String, c.hasPrefix(src + "/") { q[i]["context"] = dst + String(c.dropFirst(src.count)) } }
                apWriteJSON(dst + "/queue.json", q)
            }
            let r: (code: Int32, out: String) = moved ? apEngine(["start", dst] + (dev ? ["--dev"] : [])) : (-1, "실행 폴더를 runs/ 로 옮기지 못했습니다: \(String(cString: strerror(errno)))")
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if moved { self.dir = dst }
                    self.starting = false
                    if r.code == 0 {
                        self.remember()
                        self.close()
                        self.clearAll()
                        AutopilotStore.shared.screen = .running(runId)
                    } else {
                        self.startError = r.out.isEmpty ? "엔진이 출발하지 않았습니다(종료 코드 \(r.code))" : r.out
                        if (apRead(dst + "/run.json") as? [String: Any])?["state"] != nil { self.needNewId = true }
                    }
                }
            }
        }
    }

    // 주제로 시작의 세트(5단계 ⑪-2, 설계 5장 5절): 출발할 때(동결 직전) 명세 spec.md·기술 스택 stack.json·자율주행 지시 autopilot.md 를
    // private/projects/<프로젝트>/set/ 에 쓴다(0600, 고치기 전 글은 set-backup/). 엔진 start 가 이 세트를 고정 설정과 pinned/set/ 으로 뜬다. 못 쓰면 까닭
    private func writeTopicSet() -> String? {
        let spec = (try? String(contentsOfFile: dir + "/draft/spec.md", encoding: .utf8)) ?? ""
        let stack = (try? JSONEncoder().encode(topicStack)).flatMap { try? JSONSerialization.jsonObject(with: $0) }.flatMap { try? JSONSerialization.data(withJSONObject: $0, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]) } ?? Data("{}".utf8)
        var files: [(String, Data)] = [("spec.md", Data(spec.utf8)), ("stack.json", stack + Data("\n".utf8))]
        if let t = setText, !t.isEmpty { files.append(("autopilot.md", Data((t + "\n").utf8))) }
        return writeSetFiles(files)
    }
    // 세트 파일 쓰기(주제로 시작·가져오기 세션 초안 지시) — 0600, 고치기 전 글은 set-backup/, 임시 파일 + rename. 못 쓰면 까닭
    private func writeSetFiles(_ files: [(String, Data)]) -> String? {
        guard let p = project else { return "프로젝트 이름이 없어 세트를 쓰지 못했습니다" }
        let fm = FileManager.default
        let dirSet = autopilotRoot + "/private/projects/\(p)/set", bdir = autopilotRoot + "/private/projects/\(p)/set-backup"
        try? fm.createDirectory(atPath: dirSet, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let stamp = formatter("yyyyMMdd-HHmmss").string(from: Date())
        for (name, data) in files {
            let path = dirSet + "/" + name
            if let old = fm.contents(atPath: path), old != data {
                try? fm.createDirectory(atPath: bdir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                let ext = (name as NSString).pathExtension, base = (name as NSString).deletingPathExtension
                guard fm.createFile(atPath: bdir + "/\(base)-\(stamp).\(ext)", contents: old, attributes: [.posixPermissions: 0o600]) else { return "고치기 전 세트 \(name) 를 남기지 못해 출발하지 않았습니다" }
            }
            let tmp = path + ".tmp-\(getpid())"
            guard fm.createFile(atPath: tmp, contents: data, attributes: [.posixPermissions: 0o600]), rename(tmp, path) == 0 else { unlink(tmp); return "세트 \(name) 를 쓰지 못해 출발하지 않았습니다(\(apHome(path)))" }
        }
        return nil
    }

    // 출발 가지·승격 차례·정책 답을 프로젝트 설정에 남긴다(다른 칸은 그대로). 출발했을 때만
    private func remember() {
        guard let p = project else { return }
        let path = autopilotRoot + "/projects/\(p).json"
        var o = apRead(path) as? [String: Any] ?? [:]
        var lb = o["last_base"] as? [String: Any] ?? [:]
        for (r, b) in bases { lb[r] = b }
        o["last_base"] = lb
        if (o["promotion"] == nil && !promotion.isEmpty) || promotionEdited {
            o["promotion"] = promotion.map { s -> [String: Any] in var d: [String: Any] = ["from": s.from, "into": s.into]; if let a = s.after { d["after"] = a }; return d }
        }
        // 출발 확인에서 바꾼 모델·멈출 선은 프로젝트 설정으로(다음 출발의 기본값, 규칙 apStartSettings). 바꾸지 않았으면 쓰지 않는다
        if settingsEdited {
            o["model"] = settings["model"]
            var sl = o["stopline"] as? [String: Any] ?? [:]
            for (k, v) in settings["stopline"] as? [String: Any] ?? [:] { sl[k] = v }
            o["stopline"] = sl
        }
        // 게시 방식(4단계 ⑩-1)과 알림 webhook(⑩-3) — 바꿨거나, 프로젝트 설정에 없거나, 알림을 정한 적이 없으면
        if publishEdited || o["publish"] == nil || (o["publish"] as? [String: Any])?["notify"] == nil { o["publish"] = apPublishJSON(publish) }
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

// 판정 차이 칸 이름: check 판정 명령 · scenarios 시나리오 목록 · scenario:<파일> 그 시나리오 글
func apJudgeFieldLabel(_ f: String) -> String {
    f == "check" ? "판정 명령" : f == "scenarios" ? "시나리오 목록" : f.hasPrefix("scenario:") ? "시나리오 \(f.dropFirst(9)) 글" : f
}

// MARK: 5단계 ⑪-3 판정 비판 — 반대 탐침 줄·비판 세션·판정 고치기 차례(주제로 시작)·판정 놓침·세트 이어 쓰기(이어 가기).
// 모두 앱 안 엔진(prerun·critic·draft --fix·misses·revise)을 백그라운드 한 줄에서 부르고 보드는 결과 파일을 읽어 줄·카드로 그린다(기록 저장소는 엔진만 씀)
extension APStart {
    // 판정 고치기 다음 차례(주제로 시작만, 규칙 apFixNext): 지금 확인된 지적(탐침 카드 + 비판 세션의 확인된 지적) 수로
    var fixNext: APFixNext? {
        guard source == "topic", !fixing, prepared else { return nil }
        return apFixNext(roundsDone: fixRound, confirmedNow: probeCards.count + criticCards.filter { $0.kind == "critic" }.count)
    }
    // "판정 고치기"를 고른 확인된 지적(탐침·비판 세션)
    var fixPicked: [APCard] { (probeCards + criticCards).filter { ["probe", "critic"].contains($0.kind) && answers[$0.id] == apFixCheck } }

    nonisolated static func critCard(_ c: [String: Any], topic: Bool) -> APCard? {
        guard let id = c["id"] as? String else { return nil }
        let kind = c["kind"] as? String ?? ""
        let label = ["probe": "반대 탐침 · 망친 결과물로도 통과", "critic": "판정 비판 · 확인된 지적", "critic_unverified": "판정 비판 · 확인 안 됨",
                     "critic_reading": "판정 비판 · 해석 고르기", "fix_weak": "판정 고치기 · 약해진 고침"][kind] ?? "판정 비판"
        let notes: [String: String] = [
            apFixCheck: topic ? "작성 세션(초안)이 판정을 고치고 탐침·반례를 다시 돌립니다(출발 전, 최대 2번). 고치기 전에는 이 단위가 출발에서 빠집니다"
                              : "이 단위는 이번 실행에서 빼고 판정 놓침으로 남겨 다음 출발에서 고칩니다",
            "keep": kind == "critic_unverified" ? "실행으로 확인하지 못한 지적이라 판정은 그대로 둡니다" : "판정을 그대로 두고 단위에 넣습니다. 그 틀린 결과물도 통과할 수 있습니다",
            "drop": "이번 실행에서 뺍니다",
            "reject": "앞 판정 그대로 둡니다(판정을 약하게 하는 고침은 자동으로 들이지 않습니다)", "accept": "약해진 판정으로 바꿔 끼우고 탐침·반례를 다시 돌립니다"]
        let opts = (c["options"] as? [[String: Any]] ?? []).map { o -> APOption in
            let k = o["key"] as? String ?? ""
            return APOption(key: k, label: o["label"] as? String ?? "", note: o["note"] as? String ?? notes[k] ?? "")
        }
        return APCard(id: id, kind: kind, label: label, title: c["title"] as? String ?? "", why: c["why"] as? String ?? "", options: opts,
                      recommended: c["recommended"] as? String ?? "", unit: kind == "fix_weak" ? nil : c["unit_id"] as? String)
    }

    // 반대 탐침 줄과 카드(prerun.json 의 units[].probes·probe_cards)
    func setProbeLine(_ pj: [String: Any]) {
        let cs = (pj["probe_cards"] as? [[String: Any]] ?? []).compactMap { APStart.critCard($0, topic: source == "topic") }
        probeCards = cs
        if pj["probes"] as? Bool == false { setLine("probe", "skip", note: "반대 탐침을 끄고 돌렸습니다"); return }
        var ran = 0, detail: [String] = []
        for r in pj["units"] as? [[String: Any]] ?? [] {
            guard let p = r["probes"] as? [String: Any] else { continue }
            let t = r["title"] as? String ?? ""
            if let n = p["note"] as? String { detail.append("\(t): \(n)"); continue }
            let runs = p["runs"] as? [[String: Any]] ?? []
            ran += runs.count
            detail.append("\(t) · 대상 \((p["targets"] as? [String] ?? []).joined(separator: ", ")): " + runs.map { x -> String in
                let n = (x["passed_steps"] as? [Any])?.count ?? 0
                return "\(x["label"] as? String ?? "") " + ((x["note"] as? String).map { "(\($0))" } ?? (n == 0 ? "막힘" : "통과 \(n)곳"))
            }.joined(separator: " · ") + " · 증거 \(p["evidence"] as? String ?? "")")
        }
        let hist = (pj["probe_history"] as? [Any])?.count ?? 0
        let note = ran == 0 ? "망친 결과물을 넣을 판정이 없습니다" : cs.isEmpty ? "망친 결과물 \(ran)개를 판정이 모두 막았습니다"
            : "판정 \(cs.count)곳이 망친 결과물로도 통과합니다(출발 카드로, 판정 고치기 추천)"
        setLine("probe", ran == 0 ? "skip" : cs.isEmpty ? "done" : "warn",
                note: note + (hist > 0 ? " · 판정 고치기 \(hist)차 뒤" : "") + " · 모델 0 · 버리는 작업 트리에서(저장소에는 쓰지 않음)", detail: detail)
    }

    // 판정 비판 세션(엔진 critic) — 작성 세션과 다른 모델, 결과는 critic.json 의 마지막 차례
    func runCritic(_ d: String) {
        let dev = apDevBuild, devClaude = APTopic.devClaudeArgs, round = critRound
        let author = settings["model"] as? String ?? apDraftModel
        setLine("critic", "run", note: "작성 모델 \(author) 와 다른 모델 \(apCriticModel(author: author)) 로 단위 글·수용 기준·판정·근거 묶음만 읽습니다(읽기 도구만 · 결과 자리만 씀 · Bash·웹·MCP 없음)")
        queue.async {
            let r = apEngine(["critic", d, "--round", "\(round)"] + (dev ? ["--dev"] + devClaude : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { return }
                    self.criticSummary = o
                    self.criticCards = (o["cards"] as? [[String: Any]] ?? []).compactMap { APStart.critCard($0, topic: self.source == "topic") }
                    let c = o["counts"] as? [String: Any] ?? [:]
                    func n(_ k: String) -> Int { (c[k] as? NSNumber)?.intValue ?? 0 }
                    if o["ok"] as? Bool != true {
                        self.setLine("critic", "warn", note: "비판 세션이 끝나지 않았습니다: \(o["note"] as? String ?? apShort(r.out, 200)). 출발은 막지 않습니다",
                                     detail: apStrings(o["problems"]))
                    } else {
                        let res = o["results"] as? [[String: Any]] ?? []
                        let label = ["confirmed": "확인된 지적(판정을 통과함)", "refuted": "판정이 반례를 막음", "unverified": "확인 안 됨", "reading": "해석이 갈림"]
                        var detail = res.map { x in
                            "\(x["unit"] as? String ?? "") · \((x["judge"] as? String).map { $0 == "check" ? "판정 명령" : $0 } ?? "판정"): \(label[x["verdict"] as? String ?? ""] ?? "") · \(x["why"] as? String ?? "")"
                                + ((x["note"] as? String).map { " (\($0))" } ?? "")
                        }
                        detail += apStrings(o["skipped"]) + apStrings(o["problems"]).map { "결과 꼴: " + $0 }
                        detail.append("모델 \(o["model"] as? String ?? "?")(작성 \(o["author_model"] as? String ?? "?"))" + ((o["model_note"] as? String).map { " · " + $0 } ?? "")
                                      + " · 도구 " + apStrings(o["tools"]).joined(separator: ",") + " · 증거 \(o["evidence"] as? String ?? "")")
                        let same = o["repo_same"] as? Bool == true
                        self.setLine("critic", n("confirmed") > 0 ? "warn" : "done",
                                     note: "모델 \(o["model"] as? String ?? "?")(작성 \(o["author_model"] as? String ?? "?")) · 지적 \(res.count): 확인 \(n("confirmed")) · 확인 안 됨 \(n("unverified")) · 해석 \(n("reading")) · 판정이 막음 \(n("refuted"))"
                                        + (same ? " · 저장소·하네스 그대로" : " · 저장소·하네스가 앞뒤로 다릅니다"),
                                     detail: detail)
                    }
                    self.fixing = false
                    self.finishCheck()
                }
            }
        }
    }

    // 판정 놓침(엔진 misses → misses.json) — 열린 것이 있으면 다음 세션(초안·비판·세트 이어 쓰기)에 근거로 넘긴다
    func runMisses(_ then: @escaping @MainActor () -> Void) {
        let d = dir, dev = apDevBuild
        setLine("misses", "run")
        queue.async {
            let r = apEngine(["misses", d] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { return }
                    self.missInfo = o
                    let open = (o["open"] as? NSNumber)?.intValue ?? 0, oj = (o["open_judge"] as? NSNumber)?.intValue ?? 0
                    let rows = o["rows"] as? [[String: Any]] ?? []
                    if r.code != 0, o.isEmpty {
                        self.setLine("misses", "warn", note: "판정 놓침을 읽지 못했습니다: \(apShort(r.out, 200))")
                    } else {
                        self.setLine("misses", open > 0 ? "warn" : "done",
                                     note: open > 0 ? "열린 판정 놓침 \(open)건(판정 쪽 \(oj) · 지시 쪽 \(open - oj)) · 다음 세션에 근거로 넘깁니다"
                                                    : (o["project"] is String ? "열린 판정 놓침이 없습니다" : "이 프로젝트의 기록이 아직 없습니다"),
                                     detail: rows.map { "[\($0["label"] as? String ?? "")] \($0["title"] as? String ?? "") · \($0["why"] as? String ?? "")" })
                        self.missesUsed = open > 0 ? rows.compactMap { $0["id"] as? String } : []
                    }
                    then()
                }
            }
        }
    }

    // 세트 이어 쓰기(이어 가기, 규칙 apSetRewrite): 하네스 그대로·새 요구 없음·판정 놓침 없음이면 그대로(모델 0), 판정 놓침이 있으면 엔진 revise
    func runRevise(_ then: @escaping @MainActor () -> Void) {
        let open = (missInfo["open"] as? NSNumber)?.intValue ?? 0
        let need = apSetRewrite(harnessChanged: false, newRequest: false, openMisses: open)
        guard need.rewrite else { setLine("revise", "done", note: "하네스 그대로 · 새 요구 없음 · 판정 놓침 없음: 지난 세트를 그대로 씁니다(모델 0)"); then(); return }
        let d = dir, dev = apDevBuild, devClaude = APTopic.devClaudeArgs
        setLine("revise", "run", note: need.reasons.joined(separator: " · ") + ": 지난 판정·놓친 기록·지난 지시만 읽고 고친 초안을 씁니다(모델 \(model), 읽기 도구만, 저장소·하네스는 읽지 않음)")
        queue.async {
            let r = apEngine(["revise", d] + (dev ? ["--dev"] + devClaude : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d else { return }
                    guard o["ok"] as? Bool == true else {
                        self.setLine("revise", "warn", note: "세트 이어 쓰기가 끝나지 않았거나 검사에 걸려 지난 세트를 그대로 씁니다",
                                     detail: apStrings(o["problems"]).isEmpty ? [apShort(r.out, 300)] : apStrings(o["problems"]))
                        then(); return
                    }
                    let changes = o["changes"] as? [[String: Any]] ?? []
                    for c in changes where c["field"] as? String == "check" {
                        guard let uid = c["unit"] as? String, let i = self.units.firstIndex(where: { $0.id == uid }), let nc = c["new"] as? String,
                              var def = (try? JSONSerialization.jsonObject(with: self.units[i].def)) as? [String: Any] else { continue }
                        def["check"] = nc
                        self.units[i].check = nc
                        self.units[i].def = (try? JSONSerialization.data(withJSONObject: def, options: [.sortedKeys])) ?? self.units[i].def
                    }
                    self.judgeChanges = changes; self.judgeChangeFrom = "revise"
                    if let ins = o["instruction"] as? String, !ins.isEmpty { self.pendingSet = ins; self.setText = ins; self.setAt = Date().timeIntervalSince1970 }
                    self.setLine("revise", "done", note: "판정 명령 \(changes.count)곳을 고쳤습니다" + (self.pendingSet != nil ? " · 지시도 고침" : "")
                                    + " · 출발 확인에서 지난 판과의 차이로 보입니다(출발할 때 세트에 씀)",
                                 detail: changes.map { "\($0["title"] as? String ?? ""): \($0["old"] as? String ?? "") → \($0["new"] as? String ?? "")" }
                                    + ["모델 \(o["model"] as? String ?? "?") · " + String(format: "%.0f초", (o["sec"] as? NSNumber)?.doubleValue ?? 0)])
                    then()
                }
            }
        }
    }

    // 판정 고치기(주제로 시작) — 고른 지적을 엔진 draft --fix 로 작성 세션(초안)에 넘기고, 고친 판정이면 시나리오 다시 커밋 → 확인·미리 돌리기·탐침·비판 세션.
    // 약해졌으면 받지 않고 카드(받지 않기 추천)
    func runFix() {
        guard source == "topic", case .offer(let k)? = fixNext, !fixPicked.isEmpty, !preparing, !starting else { return }
        let ids = fixPicked.map(\.id).joined(separator: ","), picked = fixPicked.count
        let d = dir, dev = apDevBuild, devClaude = APTopic.devClaudeArgs
        fixing = true; preparing = true; prepared = false; step = 2
        if !lines.contains(where: { $0.id == "fix" }), let i = lines.firstIndex(where: { $0.id == "scenarios" }) { lines.insert(APPrepLine(id: "fix", title: "판정 고치기"), at: i) }
        for l in ["scenarios", "check", "env", "where", "prerun", "probe", "critic"] { setLine(l, "wait", note: "") }
        setLine("fix", "run", note: "\(k)차: 작성 세션(초안, 모델 \(model))이 고른 지적 \(picked)건을 막도록 판정을 고칩니다(읽기 도구만)")
        let before = draftResult["units"] as? [[String: Any]] ?? []
        queue.async {
            let r = apEngine(["draft", d, "--fix", "\(k)", "--cards", ids] + (dev ? ["--dev"] + devClaude : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async { MainActor.assumeIsolated { guard self.dir == d else { return }; self.afterFix(o, round: k, before: before, raw: r.out) } }
        }
    }
    func acceptFix() {
        guard source == "topic", let w = weakCard, !preparing, !starting else { return }
        let k = Int(w.id.split(separator: "-").last ?? "") ?? fixRound
        let d = dir, dev = apDevBuild
        fixing = true; preparing = true; prepared = false; step = 2
        setLine("fix", "run", note: "\(k)차: 사람이 약해진 고침을 받았습니다. 바꿔 끼웁니다(모델 0)")
        let before = draftResult["units"] as? [[String: Any]] ?? []
        queue.async {
            let r = apEngine(["draft", d, "--accept-fix", "\(k)"] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async { MainActor.assumeIsolated { guard self.dir == d else { return }; self.weakCard = nil; self.afterFix(o, round: k, before: before, raw: r.out) } }
        }
    }
    private func afterFix(_ o: [String: Any], round k: Int, before: [[String: Any]], raw: String) {
        fixRound = max(fixRound, k)
        if let wc = o["weak_card"] as? [String: Any] {
            weakCard = APStart.critCard(wc, topic: true)
            setLine("fix", "warn", note: "\(k)차: 판정이 약해져 자동으로 들이지 않았습니다(출발 카드: 받지 않기 추천) · 앞 판정 그대로", detail: apStrings(o["weak"]))
            for l in ["scenarios", "check", "env", "where", "prerun", "probe", "critic"] { setLine(l, "skip", note: "앞 판정 그대로라 다시 돌리지 않았습니다") }
            fixing = false; preparing = false; prepared = true; buildCards(); return
        }
        guard o["ok"] as? Bool == true else {
            setLine("fix", "fail", note: "\(k)차: " + (o["note"] as? String ?? apShort(raw, 200)) + " · 앞 판정 그대로", detail: apStrings(o["problems"]))
            for l in ["scenarios", "check", "env", "where", "prerun", "probe", "critic"] { setLine(l, "skip", note: "앞 판정 그대로라 다시 돌리지 않았습니다") }
            fixing = false; preparing = false; prepared = true; buildCards(); return
        }
        draftResult = o
        let cards = o["cards"] as? [[String: Any]] ?? []
        draftCards = cards.filter { $0["criterion"] == nil }.compactMap(APStart.draftCard)
        harnessCards = (o["harness_missing"] as? [[String: Any]] ?? []).compactMap(APStart.harnessCard)
        let after = o["units"] as? [[String: Any]] ?? []
        let ch = apJudgeChanges(old: before, new: after)
        judgeChanges = ch.map { ["unit": $0.unit, "title": $0.title, "field": $0.field, "old": $0.old, "new": $0.new] } + (o["scenario_changes"] as? [[String: Any]] ?? [])
        judgeChangeFrom = "fix"
        let items = o["fix_items"] as? [[String: Any]] ?? []
        let weakTaken = apStrings(o["accepted_weak"])
        setLine("fix", weakTaken.isEmpty ? "done" : "warn",
                note: "\(k)차: " + (weakTaken.isEmpty ? "고친 판정을 받았습니다(판정 \(judgeChanges.count)곳 바뀜 · 약해지지 않음)"
                                                    : "사람이 약해진 고침을 받았습니다(판정 \(judgeChanges.count)곳 바뀜 · \(weakTaken.first ?? "")\(weakTaken.count > 1 ? " 외 \(weakTaken.count - 1)" : ""))")
                    + " · 시나리오 다시 커밋 → 탐침·반례를 다시 돌립니다",
                detail: items.map { "\($0["unit"] as? String ?? "") · \(($0["judge"] as? String).map { $0 == "check" ? "판정 명령" : $0 } ?? "판정"): \($0["why"] as? String ?? "")" }
                    + judgeChanges.map { "\($0["title"] as? String ?? "") \(apJudgeFieldLabel($0["field"] as? String ?? "")): \($0["old"] as? String ?? "") → \($0["new"] as? String ?? "")" })
        critRound = k; keepHistory = true
        seedScenarios { ok in if ok { self.topicCheck() } else { self.fixing = false } }
    }

    // 세트 이어 쓰기가 고친 지시를 출발할 때 세트에(이어 가기, 고치기 전 글은 set-backup/). 못 쓰면 까닭
    func writeRevisedSet() -> String? {
        guard let p = project, let t = pendingSet else { return nil }
        let fm = FileManager.default
        let dirSet = autopilotRoot + "/private/projects/\(p)/set", bdir = autopilotRoot + "/private/projects/\(p)/set-backup"
        try? fm.createDirectory(atPath: dirSet, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let path = dirSet + "/autopilot.md", data = Data((t + "\n").utf8)
        if let old = fm.contents(atPath: path), old != data {
            try? fm.createDirectory(atPath: bdir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let stamp = formatter("yyyyMMdd-HHmmss").string(from: Date())
            guard fm.createFile(atPath: bdir + "/autopilot-\(stamp).md", contents: old, attributes: [.posixPermissions: 0o600]) else { return "고치기 전 지시를 남기지 못해 출발하지 않았습니다" }
        }
        let tmp = path + ".tmp-\(getpid())"
        guard fm.createFile(atPath: tmp, contents: data, attributes: [.posixPermissions: 0o600]), rename(tmp, path) == 0 else { unlink(tmp); return "세트 지시를 쓰지 못해 출발하지 않았습니다" }
        return nil
    }
}

// 출발 화면의 상자 — 제목과 작은 설명, 안에 줄들
// MARK: 7단계 ⑫-7a 가져오기 — 원천 셋(저장소 할 일 문서·바깥 목록·옛 자동화 옮기기)의 2단계 "보드가 준비": 모으기(앱 안 엔진 import gather: 원천 읽기·
// 이미 끝난 것·진행 중인 것 가리기·근거 묶음과 비밀 가리기·하네스 확인 지점) → 지난 자율주행 세트 → 판정 놓침 → 가져오기 세션(엔진 import session: 읽기 도구만,
// 단위·카드 초안 → 기계 검사) → 이어 가기와 같은 단위마다 기계 검사(status --json)·환경·따로 작업 트리·판정 미리 돌리기·반대 탐침·비판 세션.
// 같은 항목이 다시 오면(대기열에 정의가 남은 열린 줄) 가져오기 세션을 거치지 않고 그 줄의 정의로 잇는다(모델 0, 바뀐 본문은 단위 제안에 줄 차이).
// 가린 항목은 준비 화면에 수와 까닭만, 되살리기는 단위 제안에서 고른 뒤 다시 준비한다(되살린 키는 가리지 않음)

extension APStart {
    var isImport: Bool { ["todo", "list", "legacy"].contains(source) }
    var freshSource: Bool { source == "topic" || isImport }
    var importSource: APImportSource? { APImportSource(rawValue: source) }
    // 모은 항목(gather.json items) — 가린 것·이어 가는 것·새 것
    var importItems: [[String: Any]] { importGather["items"] as? [[String: Any]] ?? [] }
    var importHidden: [[String: Any]] { importItems.filter { $0["decision"] as? String == "hidden" } }

    func prepareImport() {
        guard isImport, !preparing, !starting, imp.blocker(source) == nil, let src = importSource else { return }
        let key = imp.key(source) + "\u{1}" + importRevive.sorted().joined(separator: "\u{2}")
        if prepared, key == importKey, !dir.isEmpty { step = 2; return }   // 같은 입력이면 다시 준비하지 않는다(가져오기 세션은 모델 비용이 든다)
        if !dir.isEmpty { dropFolder(); id = ""; dir = "" }
        importKey = key
        step = 2
        preparing = true; prepared = false
        problems = []; prerunPassed = []; prerunIds = []; units = []; cards = []; draftCards = []; harnessCards = []; scenarioCards = []; criterionCards = []
        probeCards = []; criticCards = []; criticSummary = [:]; weakCard = nil; fixRound = 0; fixing = false; missInfo = [:]; judgeChanges = []; judgeChangeFrom = ""
        missesUsed = []; pendingSet = nil; critRound = 0; keepHistory = false
        importGather = [:]; importResult = [:]; importChanged = [:]; importDraftedSet = false
        project = imp.projectName; root = imp.root
        // 실행 설정(주제로 시작과 같은 꼴): 프로젝트 설정에 남긴 모델·멈출 선이 있으면 그것(규칙 apStartSettings), 없으면 기본
        let proj = project ?? "import"
        let ps = apRead(autopilotRoot + "/projects/\(proj).json") as? [String: Any] ?? [:]
        let pick = apStartSettings(project: ps, past: [:])
        settingsFrom = pick.from
        settings = ["model": pick.model ?? apDraftModel, "title": src.label + " 가져오기", "project": proj,
                    "stopline": ["seven_day": pick.week ?? 0.80, "five_hour": pick.five ?? 0.95]]
        if apDevBuild {   // 시험 빌드만: 가짜 claude(설정 값 autopilotReadClaude)·장난감 사용량 파일(autopilotUsagePath, 6단계 ⑫-6 의 usage_path)
            if let c = UserDefaults.standard.string(forKey: "autopilotReadClaude"), !c.isEmpty { settings["claude_path"] = c }
            if let u = UserDefaults.standard.string(forKey: "autopilotUsagePath"), !u.isEmpty { settings["usage_path"] = u }
        }
        var pp = apPublishParse(ps["publish"]).publish
        if pp.notify == nil { APConnections.shared.loadOnce(); pp.notify = APConnections.shared.webhooks.filter { $0.scopes.contains("notify") }.map(\.id) }
        publish = pp; publishEdited = false; publishProblems = []
        pubService = pp.report?.service.rawValue ?? "none"; pubSpace = pp.report?.space ?? ""; pubParent = pp.report?.parent ?? ""
        settings["publish"] = apPublishJSON(pp)
        tickets = ps["tickets"] as? [String: Any] ?? [:]
        jira = !tickets.isEmpty
        lastSet = APRecords.lastSet(db, project: proj).map { APSetVersion(text: $0.text, at: $0.at, run: $0.run) }
        let setPath = autopilotRoot + "/private/projects/\(proj)/set/autopilot.md"
        setText = (try? String(contentsOfFile: setPath, encoding: .utf8)).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.flatMap { $0.isEmpty ? nil : $0 }
        setAt = mtime(setPath)?.timeIntervalSince1970
        assignId()
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        lines = [
            APPrepLine(id: "collect", title: "모으기 · " + src.label),
            APPrepLine(id: "hide", title: "이미 끝난 것·진행 중인 것 가리기"),
            APPrepLine(id: "set", title: "지난 자율주행 세트"),
            APPrepLine(id: "harness", title: "하네스 읽기"),
            APPrepLine(id: "misses", title: "판정 놓침(이 프로젝트)"),
            APPrepLine(id: "import", title: "가져오기 세션"),
            APPrepLine(id: "importcheck", title: "가져오기 기계 검사"),
            APPrepLine(id: "check", title: "단위마다 기계 검사 · 엔진 출발 전 확인"),
            APPrepLine(id: "env", title: "시험 실행기 환경 확인"),
            APPrepLine(id: "where", title: "따로 작업 트리 확인"),
            APPrepLine(id: "prerun", title: "판정 미리 돌리기"),
            APPrepLine(id: "probe", title: "반대 탐침"),
            APPrepLine(id: "critic", title: "판정 비판 세션"),
        ]
        setLine("collect", "run")
        let rootNow = root, d = dir
        var cfg = imp.json(source)
        cfg["name"] = proj
        cfg["include"] = importRevive.sorted()
        queue.async {
            let info = APStart.readRepos(roots: [rootNow])
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d, self.isImport, self.preparing else { return }
                    self.repos = info.repos; self.repoInfo = info.info
                    self.pickDefaultBases(self.project, self.settings)
                    apWriteJSON(d + "/import.json", cfg)
                    self.writeDraft([])
                    self.runGather()
                }
            }
        }
    }

    // 실패한 줄에서 멈춘다 — 뒤 줄은 하지 않음과 까닭. 1단계로 돌아가 고친 뒤 다시 준비한다
    private func importStop(_ why: String) {
        for i in lines.indices where ["wait", "run"].contains(lines[i].state) { lines[i].state = "skip"; lines[i].note = why }
        preparing = false; prepared = false
        importKey = ""
        if !shown { TerminalHub.shared.toast("자율주행 출발 준비(가져오기)가 멈췄습니다: \(why)") }
    }

    private func runGather() {
        let d = dir, dev = apDevBuild
        queue.async {
            let r = apEngine(["import", d, "gather"] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d, self.isImport else { return }
                    self.importGather = o
                    guard r.code == 0, o["ok"] as? Bool == true else {
                        let probs = apStrings(o["problems"])
                        self.setLine("collect", "fail", note: probs.first ?? (r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : apShort(r.out)), detail: Array(probs.dropFirst()))
                        self.importStop("원천을 모으지 못해 여기서 멈춥니다(1단계에서 고쳐 다시 준비합니다)")
                        return
                    }
                    let c = o["counts"] as? [String: Any] ?? [:]
                    func n(_ k: String) -> Int { (c[k] as? NSNumber)?.intValue ?? 0 }
                    let hidden = (c["hidden"] as? [String: Any] ?? [:]).mapValues { ($0 as? NSNumber)?.intValue ?? 0 }
                    let req = (o["requests"] as? NSNumber)?.intValue ?? 0, red = (o["redacted"] as? NSNumber)?.intValue ?? 0
                    let skippedFiles = apStrings(o["legacy_skipped"]), refused = apStrings(o["evidence_refused"])
                    self.setLine("collect", "done",
                                 note: "\(o["origin"] as? String ?? "") · 항목 \(n("total"))" + (req > 0 ? " · 바깥 요청 \(req)건" : "") + " · 근거 묶음 \((o["manifest"] as? NSNumber)?.intValue ?? 0)개 · 가린 비밀 \(red)",
                                 detail: apStrings(o["notes"]) + skippedFiles.map { "읽지 않은 파일 · \($0)" } + refused.map { "근거 묶음에 넣지 않음 · \($0)" })
                    let hideKinds: [(String, String)] = [("source_done", "원천에서 끝남"), ("source_progress", "원천에서 진행 중"), ("records_done", "이미 끝남"),
                                                         ("running", "도는 실행에서 진행 중"), ("queue_state", "대기열에서 뺀·보류한 줄")]
                    let parts = hideKinds.compactMap { k, l in (hidden[k] ?? 0) > 0 ? "\(l) \(hidden[k]!)" : nil }
                    let hiddenN = hidden.values.reduce(0, +)
                    var note = hiddenN == 0 ? "뺀 것 없음" : "\(hiddenN)개 뺌: " + parts.joined(separator: " · ") + ". 되살리기는 다음 단계(단위 제안)에서 합니다"
                    if n("continue") > 0 { note += " · 대기열 줄을 이음 \(n("continue"))" + (n("changed") > 0 ? "(바뀐 본문 \(n("changed")))" : "") }
                    if let a = self.activeHere { note = "이 경로에서 자율주행이 돌고 있습니다(\(a)). 그 실행이 끝난 뒤에 출발합니다 · " + note }
                    self.setLine("hide", self.activeHere == nil ? "done" : "warn", note: note,
                                 detail: self.importHidden.map { "\($0["title"] as? String ?? ""): \($0["why"] as? String ?? "")" })
                    let need = o["need_instruction"] as? Bool ?? false
                    self.setLine("set", "done", note: need ? "이 프로젝트 세트에 지시(autopilot.md)가 없어 가져오기 세션이 지시 초안을 씁니다(출발할 때만 세트에 씀)"
                                                         : "지시 \(self.setText?.count ?? 0)자를 불러왔습니다(\(apDay(self.setAt))). 출발 때 엔진이 고정해 자율주행 세션에만 얹습니다")
                    let hp = (o["harness_points"] as? NSNumber)?.intValue ?? 0
                    self.setLine("harness", hp > 0 ? "done" : "skip", note: hp > 0 ? "확인 지점 \(hp)곳을 찾았습니다(" + (need ? "초안 지시가 모두 다뤄야 합니다)" : "세트의 지시가 다루지 않은 곳은 출발 카드로 묻습니다)")
                                                                          : "하네스 확인 지점이 없습니다")
                    self.runMisses { self.runImportSession() }
                }
            }
        }
    }

    private func runImportSession() {
        let d = dir, dev = apDevBuild, devClaude = APTopic.devClaudeArgs
        let fresh = (importGather["counts"] as? [String: Any]).flatMap { ($0["new"] as? NSNumber)?.intValue } ?? 0
        setLine("import", "run", note: fresh == 0 ? "" : "새 항목 \(fresh)개로 단위 초안을 씁니다(모델 \(settings["model"] as? String ?? apDraftModel), 읽기 도구만 · Bash·웹·MCP 없음)")
        queue.async {
            let r = apEngine(["import", d, "session"] + (dev ? ["--dev"] + devClaude : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.dir == d, self.isImport else { return }
                    self.importResult = o
                    let note = o["note"] as? String ?? (r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : apShort(r.out))
                    if o["session"] as? Bool == false, o["ok"] as? Bool == true {
                        self.setLine("import", "skip", note: note)
                        self.setLine("importcheck", "skip", note: "세션 결과가 없습니다")
                        self.importUnits()
                        return
                    }
                    let sessionFailed = o.isEmpty || o["session"] as? Bool == false
                    let units = o["units"] as? [[String: Any]] ?? [], cards = o["cards"] as? [[String: Any]] ?? [], skipped = o["skipped"] as? [[String: Any]] ?? []
                    let sec = (o["sec"] as? NSNumber)?.doubleValue ?? 0, usd = (o["cost_usd"] as? NSNumber)?.doubleValue
                    self.setLine("import", sessionFailed ? "fail" : "done",
                                 note: sessionFailed ? note : "항목 \((o["items"] as? NSNumber)?.intValue ?? 0) → 단위 \(units.count) · 카드 \(cards.count) · 건너뜀 \(skipped.count)"
                                    + ((o["instruction_chars"] as? NSNumber).map { $0.intValue > 0 ? " · 지시 초안 \($0)자" : "" } ?? ""),
                                 detail: [String(format: "%.0f초", sec) + (usd.map { String(format: " · %.3f USD", $0) } ?? "") + " · 모델 \(o["model"] as? String ?? "?")"]
                                    + skipped.map { "건너뜀 · \($0["key"] as? String ?? ""): \($0["why"] as? String ?? "")" })
                    if sessionFailed { self.setLine("importcheck", "skip", note: "가져오기 세션이 결과를 쓰지 못했습니다"); self.importStop("가져오기 세션이 끝나지 않아 여기서 멈춥니다"); return }
                    let probs = apStrings(o["problems"]), warns = apStrings(o["warnings"])
                    guard o["ok"] as? Bool == true else {
                        self.setLine("importcheck", "fail", note: "고칠 것 \(probs.count)개: \(probs.first ?? "")", detail: probs + warns)
                        self.importStop("가져오기 결과가 기계 검사를 지나지 않아 여기서 멈춥니다(1단계에서 덧붙일 말을 고쳐 다시 준비합니다)")
                        return
                    }
                    let missing = o["harness_missing"] as? [[String: Any]] ?? []
                    self.setLine("importcheck", warns.isEmpty ? "done" : "warn",
                                 note: "칸·상한·HTML 없음·비밀 꼴 없음·판정 명령 있음·항목마다 단위·카드·건너뜀 · 원천 항목 키 겹침 없음"
                                    + ((o["harness_points"] as? NSNumber)?.intValue ?? 0 > 0 ? " · 하네스 확인 지점 \(missing.isEmpty ? "모두 다룸" : "\(missing.count)곳 빠짐(출발 카드로)")" : ""),
                                 detail: warns)
                    self.importUnits()
                }
            }
        }
    }

    // 이어 가는 줄(대기열에 남은 정의) + 가져오기 세션 단위 → 출발 준비의 단위, 그 뒤는 이어 가기와 같은 확인
    private func importUnits() {
        var list: [APDraftUnit] = [], changed: [String: [String]] = [:]
        let bundle = importGather["bundle"] as? String
        var k = 0
        func unit(_ d: [String: Any], _ last: String) -> APDraftUnit {
            var u = APDraftUnit(id: d["id"] as? String ?? "u\(list.count + 1)")
            u.n = list.count + 1
            u.title = d["title"] as? String ?? ""; u.kind = d["kind"] as? String ?? ""; u.check = d["check"] as? String ?? ""
            u.sourceKey = d["source_key"] as? String ?? ""
            u.est = apNum(d["est_min"])
            u.depends = d["depends"] as? [String] ?? []
            u.story = apText(d["story"]); u.origStory = u.story
            u.task = u.story == nil ? nil : apStrings(d["tickets"]).first
            u.last = last
            // 판정 범위를 넓히는 칸은 한 번 더 버린다(⑪-2 보안 검토와 같은 꼴, 작업 폴더·근거 묶음·티켓은 엔진이 정한 값)
            let keep = d.filter { k, _ in ["cwd", "context", "tickets", "story", "source_key", "source_digest"].contains(k) || !apDraftWideningKeys.contains(k) }
            u.def = (try? JSONSerialization.data(withJSONObject: keep, options: [.sortedKeys])) ?? Data()
            return u
        }
        for it in importItems where it["decision"] as? String == "continue" {
            var item = APImportItem()
            item.key = it["key"] as? String ?? ""
            for c in it["carry"] as? [[String: Any]] ?? [] {
                guard let raw = c["def"] as? String, let def = (try? JSONSerialization.jsonObject(with: Data(raw.utf8))) as? [String: Any],
                      var q = apCarryQueueUnit(def: def, id: "c\(k + 1)", item: item, context: bundle) else { continue }
                k += 1
                q["source_digest"] = it["digest"] as? String ?? ""
                let u = unit(q, "이어 감")
                if let ch = it["changed"] as? [String], !ch.isEmpty { changed[u.id] = ch }
                list.append(u)
            }
        }
        for d in importResult["units"] as? [[String: Any]] ?? [] { list.append(unit(d, "새 단위")) }
        units = list
        importChanged = changed
        // Jira 하위 작업(상위 스토리가 있는 항목)은 그 스토리로 묶는다(원천이 Jira 목록이면 이미 있는 스토리를 그대로, 설계 2장 끝)
        stories = []
        var seen = Set<String>()
        for u in units { if let st = u.story, seen.insert(st).inserted { stories.append(APStory(id: st, key: st, kind: u.kind)) } }
        if !stories.isEmpty { jira = true; refreshEpic() }
        draftCards = (importResult["cards"] as? [[String: Any]] ?? []).compactMap { c in
            APStart.draftCard(c).map { var x = $0; x.label = "가져오기 세션이 물음"; return x }
        }
        harnessCards = (importResult["harness_missing"] as? [[String: Any]] ?? []).compactMap(APStart.harnessCard)
        if importResult["need_instruction"] as? Bool == true,
           let t = (try? String(contentsOfFile: dir + "/import/out/autopilot.md", encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty {
            setText = t; setAt = Date().timeIntervalSince1970; importDraftedSet = true
        }
        estimates = calibrate()
        if units.isEmpty {
            for l in ["env", "where", "prerun", "probe", "critic"] { setLine(l, "skip", note: "") }
            setLine("check", "skip", note: "가져올 단위가 없습니다(모두 가렸거나 카드·건너뜀)")
            preparing = false; prepared = true
            buildCards()
            return
        }
        setLine("check", "run")
        writeDraft(units)
        runStatus(prerun: true, ids: Set(units.map(\.id)), critic: true)
    }

    // 단위 제안의 "되살리기" — 가린 항목을 고른 뒤 다시 준비한다(되살린 키는 가리지 않음, 가져오기 세션·비판 세션은 다시 돈다)
    func toggleRevive(_ key: String) { if importRevive.contains(key) { importRevive.remove(key) } else { importRevive.insert(key) } }
    func reviveImport() {
        guard isImport, !importRevive.isEmpty, !preparing, !starting else { return }
        prepareImport()
    }

    // 출발할 때 가져오기 줄(import-queue.json) — 고를 수 있는 단위 모두(고르지 않은 후보도 대기열에 남는다). 엔진 start 가 기록 저장소 대기열에 한 줄씩
    private func writeImportQueue() {
        guard let p = project, let src = importSource else { return }
        let items = importItems
        func item(_ key: String) -> [String: Any]? { items.first { apKeyFamily($0["key"] as? String ?? "", key) } }
        let lines = choosable.compactMap { u -> APImportQueueLine? in
            guard !u.sourceKey.isEmpty, let d = (try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any],
                  let def = try? JSONSerialization.data(withJSONObject: d, options: [.sortedKeys]) else { return nil }
            let it = item(u.sourceKey)
            return APImportQueueLine(key: u.sourceKey, title: u.title, kind: u.kind, estMin: u.est, digest: d["source_digest"] as? String ?? "",
                                     summary: (it?["body_lines"] as? [String] ?? []).joined(separator: "\n"), source: it?["source"] as? String ?? src.rawValue,
                                     def: String(decoding: def, as: UTF8.self))
        }
        apWriteJSON(dir + "/import-queue.json", apImportQueueJSON(project: p, source: src, lines: lines))
    }
}

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
            if s.source == "topic" { APChip(text: "주제로 시작", color: Palette.accent) }
            if let src = s.importSource { APChip(text: src.label, color: Palette.accent) }
            if let p = s.project, !s.freshSource || s.step > 1 { APChip(text: p, color: APColor.board) }
            if s.jira { APChip(text: "Jira 연결", color: APColor.jira) }
            Spacer(minLength: 4)
            if s.hasDraft || s.freshSource {
                APButton(title: "준비 버리기", icon: "trash", enabled: !s.starting && !s.preparing,
                         help: s.source == "topic" ? "이 준비(주제 글·링크·고른 스택·초안 결과)와 준비가 만든 자율주행 가지를 버립니다. 보드가 만든 새 폴더는 지우지 않습니다" : "출발하지 않은 이 준비와 실행 폴더를 지웁니다",
                         axid: "ap-start-discard") { s.discard() }
            }
            APButton(title: "닫기", help: "준비는 남아 뒤에서 이어 갑니다. 보드 위 '출발 준비'로 다시 엽니다", axid: "ap-start-close") { s.close() }
        }
        .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 8)
    }

    private var steps: some View {
        let names = ["원천 고르기", "보드가 준비", "단위 제안·출발 카드", "출발 확인"]
        return WrapRow(spacing: 6) {
            ForEach(0..<4, id: \.self) { i in
                let n = i + 1
                let can = n == 1 || (n == 2 && (s.preparing || s.prepared || (s.freshSource && !s.lines.isEmpty))) || (n >= 3 && s.prepared && !s.preparing)
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
    @ObservedObject private var tp = APStart.shared.topic    // 주제로 시작 입력이 바뀌면 다음 단추가 풀리게
    @ObservedObject private var im = APStart.shared.imp      // 가져오기 입력이 바뀌면 다음 단추가 풀리게(7단계 ⑫-7a)
    @ObservedObject private var cn = APConnections.shared
    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            if s.step > 1 { APButton(title: "이전", enabled: !s.starting, axid: "ap-prev") { s.step == 4 ? s.toProposal() : (s.step = s.step - 1) } }
            message.frame(maxWidth: .infinity, alignment: .leading)
            next
        }
    }

    @ViewBuilder private var message: some View {
        switch s.step {
        case 1:
            if s.source == "topic" { apNote(tp.blocker ?? "원천은 하나만 고릅니다", tp.blocker == nil ? .secondary : APColor.limit) }
            else if s.isImport { let b = im.blocker(s.source); apNote(b ?? "원천은 하나만 고릅니다", b == nil ? .secondary : APColor.limit) }
            else { apNote(s.hasDraft ? "원천은 하나만 고릅니다" : (s.past.isEmpty ? "이어 갈 지난 실행이 아직 없습니다. 주제로 시작을 고를 수 있습니다" : "원천을 하나 고릅니다")) }
        case 2:
            if s.preparing, s.linkUndecided { apNote("못 읽은 링크가 있습니다. 줄마다 다시 읽거나 이 링크를 빼고 계속합니다", APColor.limit) }
            else if s.preparing { apNote("준비하는 중입니다. 끝나면 다음으로 갈 수 있습니다. 시트를 닫아도 뒤에서 이어 갑니다") }
            else if s.source == "topic", !s.prepared { apNote("주제로 시작 준비가 멈췄습니다. 실패한 줄의 까닭을 보고 1단계에서 고친 뒤 다시 준비합니다", APColor.limit) }
            else if s.isImport, !s.prepared { apNote("가져오기 준비가 멈췄습니다. 실패한 줄의 까닭을 보고 1단계에서 고친 뒤 다시 준비합니다", APColor.limit) }
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
            if s.source == "topic" {
                APButton(title: "다음: 보드가 준비", primary: true, enabled: tp.ready && (!s.preparing || s.topicRedoable), help: tp.blocker ?? "보드가 혼자 준비합니다(새 폴더면 이때 만듭니다)", axid: "ap-next") {
                    s.prepareTopic()
                }
            } else if s.isImport {
                let b = im.blocker(s.source)
                APButton(title: "다음: 보드가 준비", primary: true, enabled: b == nil && !s.preparing, help: b ?? "보드가 원천을 모으고 가져오기 세션으로 단위 초안을 씁니다", axid: "ap-next") {
                    s.prepareImport()
                }
            } else {
                APButton(title: "다음: 보드가 준비", primary: true, enabled: s.hasDraft && !s.units.isEmpty && s.noDefs == nil,
                         help: s.units.isEmpty ? "이어 갈 단위가 없습니다" : "보드가 혼자 준비합니다", axid: "ap-next") {
                    if s.prepared || s.preparing { s.step = 2 } else { s.prepare() }
                }
            }
        case 2:
            APButton(title: "다음: 단위 제안", primary: true, enabled: s.prepared && !s.preparing,
                     help: s.source == "topic" && !s.prepared ? "초안 세션이 단위를 쓰고 기계 검사를 지나야 단위 제안으로 갑니다" : "", axid: "ap-next") { s.toProposal() }
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
    @ObservedObject private var cn = APConnections.shared   // 바깥 목록 원천의 잠금(연결·키체인 있음)이 늦게 읽혀도 다시 그리게(7단계 ⑫-7a)
    @State private var pickRun = false
    @State private var openParked = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let p = s.presetRoot {
                HStack(spacing: 6) {
                    Image(systemName: "folder").font(.system(size: 11)).foregroundStyle(Palette.accent)
                    apNote("경로 카드에서 열었습니다: \(apHome(p)). 이 경로가 작업 경로로 채워져 있고 원천만 고르면 됩니다.")
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("ap-preset-root")
            }
            APBox(title: "어디서 일을 가져올까요?", note: "하나만 고릅니다") {
                source("continue", "지난 실행 이어 가기", continueSummary, lock: continueLock)
                source("todo", "저장소 할 일 문서", "docs/TODO.md 같은 파일과 그 안의 절을 고릅니다. 항목마다 가져오기 세션이 단위 초안을 씁니다", lock: nil)
                source("list", "바깥 목록", "Jira 필터·에픽이나 Notion 데이터베이스. 연결이 있을 때만 고를 수 있습니다", lock: s.imp.listLock)
                source("legacy", "옛 자동화 옮기기", "다른 도구로 돌던 자동화의 멈춘 자리 기록과 원장을 가져옵니다. 비밀은 가린 근거 묶음으로만 넘깁니다", lock: nil)
                source("topic", "주제로 시작", "주제 글과 자료 링크를 주면 초안 세션이 명세와 단위를 씁니다. 새 폴더에서도 됩니다", lock: nil,
                       after: "자료 링크는 연결이 있으면 다음 단계(보드가 준비)에서 메타 정보만 읽고, 그 근거 묶음으로 초안 세션이 명세·단위·판정 시나리오를 씁니다")
            }
            if s.source == "topic" {
                APTopicStep1(s: s, t: s.topic)
            } else if s.isImport {
                APImportStep1(s: s, m: s.imp)
            } else {
                if !s.past.isEmpty, s.source == "continue" { pastList }
                if s.hasDraft { pathBox }
                if !s.leftBranches.isEmpty { APLeftBranches(s: s) }
                if !s.jira, let r = s.pastRun, r.merge?.hasWaiting == true { APUnmergedNote(s: s, run: r) }
                if s.jira { APEpicBox(s: s) }
                if s.hasDraft { importBox }
            }
        }
    }

    private var continueLock: String? {
        if s.past.isEmpty { return "끝난 자율주행 실행이 아직 없습니다" }
        if let p = s.presetRoot, s.pastAt(p).isEmpty { return "이 경로(\(apHome(p)))의 지난 실행이 없습니다" }
        return nil
    }

    private var continueSummary: String {
        guard let r = s.pastRun else {
            if s.past.isEmpty { return "끝난 자율주행 실행이 아직 없습니다" }
            if let p = s.presetRoot { let n = s.pastAt(p).count; return n == 0 ? "남은 단위·미룬 사람 몫을 이어 갑니다" : "이 경로의 끝난 실행 \(n)개 가운데 하나를 이어 갑니다" }
            return "끝난 실행 \(s.past.count)개 가운데 하나를 이어 갑니다"
        }
        return "\(apDay(r.started)) 실행 · 남은 단위 \(s.units.count) · 미룬 사람 몫 \(s.carryPark.count) · 새로 찾은 일 \(s.carryNew.count)"
    }

    private func source(_ key: String, _ title: String, _ sub: String, lock: String?, after: String? = nil) -> some View {
        APPickRow(on: s.source == key, enabled: lock == nil && !s.preparing && !s.starting, help: lock ?? "", axid: "ap-src-\(key)", action: { s.chooseSource(key) }) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(title).boardFont(12, weight: .semibold).fixedSize()
                    if lock != nil { APChip(text: "잠금", color: APColor.muted) }
                }
                apNote(sub)
                if let lock { apNote(lock, APColor.limit.opacity(0.85)) }
                if let after { apNote(after) }
            }
        }
    }

    private var pastList: some View {
        let cur = s.pastRun.map { "\($0.projectName) · \(apDate($0.started)) · \($0.result)" } ?? "고르지 않음"
        return APFold(title: "이어 갈 실행", summary: cur, axid: "ap-fold-past", open: $pickRun) {
            apNote("끝난 실행 가운데 하나 · 최근 것부터" + (s.presetRoot == nil ? "" : " · 경로 카드의 경로에서 돈 실행만"))
            ForEach((s.presetRoot.map { s.pastAt($0) } ?? s.past).prefix(12)) { r in
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
            if !s.openParked.isEmpty { openParkedBox }
        }
    }

    // 이어 간 줄 밖의 열린 미룬 몫(5-2b) — 같은 프로젝트의 다른 실행이 미뤄 아직 정하거나 닫지 않은 것. 고르면 출발 카드로 넣는다(기본은 넣지 않음)
    private var openParkedBox: some View {
        APFold(title: "이어 간 줄 밖에서 미룬 사람 몫 \(s.openParked.count)", summary: s.includeParked.isEmpty ? "보이기만 · 카드에 넣지 않음" : "카드에 넣음 \(s.includeParked.count)",
               axid: "ap-fold-open-parked", open: $openParked) {
            apNote("이 실행이 이어 간 줄 밖의 실행(이어 가지 않은 옛 실행 등)이 미룬 몫입니다. 아직 어느 출발 카드에서도 정하거나 닫지 않았습니다. 고르면 이번 출발 카드에 넣습니다.")
            ForEach(s.openParked, id: \.id) { c in
                APPickRow(on: s.includeParked.contains(c.id), square: true, enabled: !s.preparing && !s.starting, axid: "ap-open-parked-\(c.id)", action: { s.toggleParked(c.id) }) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(c.title).boardFont(11.5, weight: .semibold).fixedSize(horizontal: false, vertical: true)
                        apNote("\(apDay(c.started)) 실행 \(c.run) · 단위 \(c.unit)" + (c.why.isEmpty ? "" : " · " + c.why))
                    }
                }
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

// 이어 갈 실행의 자율주행 가지를 아직 합치지 않음(Jira 아닌 실행, 2단계 5-4 — 멈추고 다시 출발 뒤가 흔한 때) — 새 실행은 고른 출발 가지에서
// 시작하므로 지난 실행이 한 일을 보지 않는다. 막지는 않고 알리며 합치기·승격으로 잇는다(지난 가지는 그대로 남는다)
struct APUnmergedNote: View {
    @ObservedObject var s: APStart
    let run: APPastRun
    var body: some View {
        APBox(title: "지난 실행의 일을 아직 합치지 않았습니다", note: "자율주행 가지 autopilot/\(run.id)", tint: APColor.wait, axid: "ap-unmerged") {
            apNote("새 실행은 출발 확인에서 고른 출발 가지(dev 등)에서 시작하므로, 지난 실행이 통과시킨 단위의 커밋을 보지 않습니다. "
                   + "이어서 하려면 합치기·승격에서 먼저 받는 가지에 합친 뒤 출발합니다. 그대로 출발해도 되고, 지난 가지는 남습니다.")
            APButton(title: "합치기·승격 →", primary: true, help: "지난 실행의 합치기·승격을 엽니다. 이 준비는 남습니다", axid: "ap-unmerged-merge") { s.toMerge() }
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
                if !ok { APChip(text: s.epic.isEmpty ? "비어 있음" : "에픽 번호 꼴이 아님", color: APColor.fail, strong: true) }
                else if let e = s.epicInfo, e.key == s.epic, !e.loading, e.read {
                    if e.found && e.isEpic { APChip(text: "에픽", color: APColor.ok, strong: true); Text(e.title).boardFont(12, weight: .semibold).lineLimit(1); if !e.status.isEmpty { APChip(text: "상태 \(e.status)", color: APColor.board) } }
                    else { APChip(text: e.found ? "에픽이 아님" : "없는 번호", color: APColor.fail, strong: true) }
                } else if s.epicInfo?.loading == true { APChip(text: "읽는 중", color: APColor.board) }
                else { APChip(text: "번호 꼴 맞음", color: APColor.ok, strong: true) }
            }
            .accessibilityElement(children: .contain).accessibilityIdentifier("ap-epic-row")
            if !ok { apNote("에픽 번호를 넣어 주세요. 그때까지 출발이 잠깁니다.", APColor.limit) }
            if let e = s.epicInfo, e.key == s.epic, !e.loading {
                if e.read, !(e.found && e.isEpic) { apNote(e.note + (e.note.hasSuffix(".") ? " " : ". ") + "그때까지 출발이 잠깁니다.", APColor.limit) }
                else if !e.read { apNote(e.note) }
            }
            apNote(past.isEmpty ? "지난 실행에 에픽 기록이 없어 빈 칸으로 열었습니다. 지난 실행의 에픽은 이어 가기일 때만 채웁니다."
                   : s.epic == past ? "이어 가기라 지난 실행의 에픽을 채워 열었습니다. 다른 번호로 바꿀 수 있습니다."
                   : "지난 실행의 에픽(\(past))에서 바꿨습니다.")
            apNote("Atlassian 연결에 읽기 범위가 있으면 보드가 에픽의 제목과 상태를 읽어 보입니다(이슈 하나 읽기 요청). 연결이 없으면 번호 꼴만 봅니다.")
        }
        .onAppear { s.refreshEpic() }
    }
}

// 에픽 칸이 읽은 것(앱 안 엔진 read epic)
struct APEpicSeen: Equatable {
    var key = ""
    var read = false
    var found = false
    var isEpic = false
    var title = ""
    var status = ""
    var note = ""
    var loading = false
}
func apShortURL(_ u: String) -> String {
    let c = URLComponents(string: u)
    let t = (c?.host ?? "") + (c?.path ?? "")
    return t.isEmpty ? apShort(u, 50) : apShort(t, 50)
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
            if !l.note.isEmpty { apNote(l.note, l.state == "fail" ? APColor.fail : .secondary) }
            ForEach(Array(l.detail.prefix(8).enumerated()), id: \.offset) { _, d in
                Text("· " + d).boardFont(10.5).foregroundStyle(l.state == "fail" ? APColor.fail : .secondary).fixedSize(horizontal: false, vertical: true)
            }
            if l.id.hasPrefix("link-"), l.state == "fail" {   // 못 읽은 링크(와이어프레임 출발 준비 ⑭) — 출발 전이라 사람이 고른다
                HStack(spacing: 6) {
                    APButton(title: "다시 읽기", axid: "ap-link-retry-\(l.id.dropFirst(5))") { s.retryLink(l.id) }
                    APButton(title: "이 링크 빼고 계속", axid: "ap-link-skip-\(l.id.dropFirst(5))") { s.skipLink(l.id) }
                }
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
            if s.isImport { APImportHiddenBox(s: s) }
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
                            APChip(text: u.last == "새 단위" ? (s.isImport ? "새 단위(가져오기 세션)" : "새 단위(초안 세션)") : u.last == "이어 감" ? "대기열 줄을 이음" : "지난번 " + apUnitLabel(u.last),
                                   color: APColor.unit(u.last))
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
                        if s.isImport, !u.sourceKey.isEmpty { Text("원천 " + u.sourceKey).boardFont(10, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle) }
                        if let ch = s.importChanged[u.id] { APImportChangedNote(lines: ch) }
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
    @State private var editPromo = false
    @State private var card = 0
    @State private var hl = false
    private func bind(_ k: String) -> Binding<Bool> { Binding(get: { open.contains(k) }, set: { if $0 { open.insert(k) } else { open.remove(k) } }) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            whereBox
            if s.isImport { APImportSourceBox(s: s) }
            if !s.problems.isEmpty { APProblems(problems: s.problems) }
            if let e = s.startError { startErrorBox(e) }
            if s.jira { ticketTree }
            cardsBox
            dirtyFold
            unitsFold
            if s.source == "topic" { specFold; stackFold }
            settingsFold
            instructionFold
            if s.locks.count > 1 {
                APBox(title: "출발을 막는 것 \(s.locks.count)", tint: APColor.limit, axid: "ap-locks") {
                    ForEach(s.locks, id: \.self) { apNote("· " + $0) }
                }
            }
        }
        .onAppear {
            if s.source == "topic" || (s.isImport && s.importDraftedSet) { open.insert("set"); open.insert("checks") }
            if !s.judgeChanges.isEmpty { open.insert("set"); open.insert("judgediff") }   // 판정 차이(⑪-3)는 펼친 채로
        }   // 주제로 시작: 초안 지시와 판정을 펼친 채로(⑪-2 보안 검토)
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
        let roles = apBranchRoles(info.branches)
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
                        let madeKey = s.ticketBook.entries["story:" + story.id].flatMap { $0.state == .made ? $0.key : nil }   // 출발을 누를 때 만든 스토리(4단계 ⑩-3)
                        Text("\(s.storyLabel(story)) · " + (real ?? s.storyPattern(story.kind).replacingOccurrences(of: "{story}", with: story.key ?? madeKey ?? "‹키›")))
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
        let probs = s.promotionProblems
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("승격 차례").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 52, alignment: .leading)
            VStack(alignment: .leading, spacing: 3) {
                Text((s.jira ? "스토리마다 " : "") + (text.isEmpty ? "승격할 가지(dev·live·master)가 없습니다" : text)).boardFont(11.5).fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("ap-promotion-text")
                if s.promotionEdited {
                    apNote("바꾼 차례입니다. 출발하면 프로젝트 설정에 덮어씁니다(같은 프로젝트의 끝난 실행 합치기·승격도 이 차례를 따릅니다).", APColor.limit)
                } else {
                    apNote(s.promotionSaved ? "프로젝트 설정의 차례입니다." : "저장소 가지로 채웠습니다. 출발하면 프로젝트 설정에 남깁니다.")
                }
                ForEach(probs, id: \.self) { apNote("· " + $0, s.promotionEdited ? APColor.fail : APColor.limit) }
                HStack(spacing: 6) {
                    APButton(title: editPromo ? "바꾸기 닫기" : "프로젝트 설정 바꾸기", enabled: !s.starting && !s.repos.isEmpty,
                             help: s.repos.isEmpty ? "저장소 가지를 읽은 뒤에 바꿀 수 있습니다" : "승격 차례를 이 자리에서 바꿉니다", axid: "ap-promo-edit") { editPromo.toggle() }
                    if s.promotionEdited { APButton(title: "되돌리기", help: "바꾸기 전 차례로", axid: "ap-promo-undo") { s.undoPromotion() } }
                }
                if editPromo { promotionEditor }
                if s.source == "topic", s.topicNew {   // 새 폴더의 가지 꼴(5단계 ⑪-2): 출발 전에 바꿈
                    HStack(spacing: 6) {
                        apNote("새 폴더 가지 꼴")
                        APButton(title: "dev·live·master", tint: s.topicLayout == "dlm" ? Palette.accent : nil, enabled: !s.rechecking && !s.starting, axid: "ap-layout-dlm") { s.setTopicLayout("dlm") }
                        APButton(title: "main 하나", tint: s.topicLayout == "main" ? Palette.accent : nil, enabled: !s.rechecking && !s.starting, axid: "ap-layout-main") { s.setTopicLayout("main") }
                    }
                    apNote("보드가 만든 새 폴더라 출발 전에 가지 꼴을 바꿀 수 있습니다. 없는 가지는 빈 첫 커밋에 만들고(있는 가지는 지우지 않음) 출발 가지에서 자율주행 가지와 뼈대·시나리오 커밋을 다시 만듭니다(모델 0).")
                }
            }
            Spacer(minLength: 0)
        }
    }

    // 승격 차례 바꾸기 — 단계마다 받는 가지·올릴 것·뒤 조건을 고른다(객관식, 고를 것은 규칙 apPromotionChoices). 고칠 것은 위에(규칙 apPromotionProblems)
    private var promotionEditor: some View {
        let br = s.promotionBranches
        return VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(s.promotion.enumerated()), id: \.offset) { i, st in
                let c = apPromotionChoices(s.promotion, at: i, branches: br)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 5) {
                        Text(apCircled(i + 1)).boardFont(11, weight: .bold)
                        Text("\(st.into) ← \(st.from == "autopilot" ? (s.jira ? "스토리 가지" : "autopilot 가지") : st.from)" + (st.after.map { " (\($0) 뒤)" } ?? ""))
                            .boardFont(11, weight: .semibold).lineLimit(1)
                        Spacer(minLength: 0)
                        APButton(title: "빼기", axid: "ap-promo-\(i)-remove") { s.removePromotionStep(i) }
                    }
                    WrapRow(spacing: 5) {
                        Text("받는 가지").boardFont(10.5).foregroundStyle(.secondary)
                        ForEach(c.into, id: \.self) { b in
                            APButton(title: b, tint: st.into == b ? Palette.accent : nil, axid: "ap-promo-\(i)-into-\(b)") { s.setPromotion(i, into: b) }
                        }
                    }
                    .padding(.leading, 18)
                    WrapRow(spacing: 5) {
                        Text("올릴 것").boardFont(10.5).foregroundStyle(.secondary)
                        ForEach(c.from, id: \.self) { f in
                            APButton(title: f == "autopilot" ? (s.jira ? "스토리 가지" : "autopilot 가지") : f, tint: st.from == f ? Palette.accent : nil,
                                     axid: "ap-promo-\(i)-from-\(f)") { s.setPromotion(i, from: f) }
                        }
                        Text("뒤 조건").boardFont(10.5).foregroundStyle(.secondary).padding(.leading, 6)
                        APButton(title: "없음", tint: st.after == nil ? Palette.accent : nil, axid: "ap-promo-\(i)-after-none") { s.setPromotion(i, after: .some(nil)) }
                        ForEach(c.after, id: \.self) { a in
                            APButton(title: "\(a) 뒤", tint: st.after == a ? Palette.accent : nil, axid: "ap-promo-\(i)-after-\(a)") { s.setPromotion(i, after: .some(a)) }
                        }
                    }
                    .padding(.leading, 18)
                }
                .padding(6)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.03)))
            }
            HStack(spacing: 6) {
                APButton(title: "＋ 단계 더하기", enabled: !br.isEmpty, axid: "ap-promo-add") { s.addPromotionStep() }
                APButton(title: "저장소 가지로 다시 채우기", enabled: !br.isEmpty, axid: "ap-promo-refill") { s.refillPromotion() }
            }
            apNote("받을 수 있는 가지: " + (br.isEmpty ? "없음" : br.joined(separator: " · ")) + (s.repos.count > 1 ? "(묶음의 모든 저장소에 있는 가지)" : ""))
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-promo-editor")
    }

    private func startErrorBox(_ e: String) -> some View {
        APBox(title: s.ticketNote?.bad == true ? "출발하지 않았습니다" : "엔진이 출발하지 않았습니다",
              note: s.ticketNote?.bad == true ? "만들 티켓 나무에 실패한 티켓과 까닭이 있습니다. 출발을 다시 누르면 남은 것만 만듭니다" : "엔진 문장 그대로입니다. 고친 뒤 출발을 다시 누르면 같은 준비로 다시 합니다",
              tint: APColor.fail, axid: "ap-start-error") {
            Text(e).boardFont(11, design: .monospaced).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        }
    }

    // 출발 카드 — 가로 한 줄로 넘긴다. 위에 지금 카드와 안 고른 수, ‹ ›, 모두 추천대로. 아래에 카드마다 점
    private var cardsBox: some View {
        let left = s.cards.filter { s.answers[$0.id] == nil }.count
        return APBox(title: "출발 카드 \(s.cards.count)장", note: "객관식, 추천안 표시", axid: "ap-cards") {
            critiqueRow
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

    // 판정 비판(⑪-3): 반대 탐침·비판 세션 요약과 판정 고치기 차례(주제로 시작만 작성 세션이 고침, 최대 2번, 확인된 지적이 없으면 멈춤)
    @ViewBuilder private var critiqueRow: some View {
        let probes = s.probeCards.count, conf = s.criticCards.filter { $0.kind == "critic" }.count
        let unv = s.criticCards.filter { $0.kind == "critic_unverified" }.count, rd = s.criticCards.filter { $0.kind == "critic_reading" }.count
        let cs = s.criticSummary
        if !s.lines.filter({ ["probe", "critic"].contains($0.id) }).isEmpty {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    APChip(text: "판정 비판", color: probes + conf > 0 ? APColor.limit : APColor.ok, strong: true)
                    apNote("반대 탐침 \(probes)곳 · 비판 세션 확인 \(conf) · 확인 안 됨 \(unv) · 해석 \(rd)"
                           + ((cs["model"] as? String).map { " · 비판 모델 \($0)(작성 \(cs["author_model"] as? String ?? "?"))" } ?? ""))
                }
                .accessibilityElement(children: .combine).accessibilityIdentifier("ap-critique-sum")
                if s.source == "topic" {
                    switch s.fixNext {
                    case .offer(let k)?:
                        HStack(spacing: 6) {
                            APButton(title: "판정 고치기 \(k)차 돌리기" + (s.fixPicked.isEmpty ? "" : " (\(s.fixPicked.count)건)"), enabled: !s.fixPicked.isEmpty && !s.starting,
                                     help: "판정 고치기를 고른 확인된 지적을 작성 세션(초안)에 넘겨 판정을 고치고 탐침·반례를 다시 돌립니다(최대 2번)", axid: "ap-fix-run") { s.runFix() }
                            apNote(s.fixPicked.isEmpty ? "확인된 지적 카드에서 판정 고치기를 고르면 작성 세션이 고칩니다(\(k)차 · 최대 \(apFixRoundMax)번)" : "고른 지적을 막도록 고칩니다. 판정을 약하게 하는 고침은 자동으로 들이지 않습니다")
                        }
                    case .stop(let why)?:
                        apNote(why + (s.fixRound > 0 ? " (판정 고치기 \(s.fixRound)번 함)" : ""), s.fixRound > 0 ? APColor.ok : Color.secondary)
                            .accessibilityIdentifier("ap-fix-stop")
                    case nil:
                        if s.fixing { APChip(text: "판정을 고치는 중", color: APColor.work, strong: true) }
                    }
                } else if probes + conf > 0 {
                    apNote("이어 가기에는 판정을 고칠 작성 세션이 없어, 판정 고치기는 그 단위를 이번 실행에서 빼고 판정 놓침으로 남겨 다음 출발에서 고칩니다")
                }
            }
            .padding(.bottom, 4)
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

    // 명세(주제로 시작, 5단계 ⑪-2) — 머리에 요구사항·수용 기준·시나리오 수, 펼치면 수용 기준마다 짝지은 시나리오(없으면 카드). 명세는 출발할 때 세트 spec.md 로
    private var specFold: some View {
        let spec = s.draftResult["spec"] as? [String: Any] ?? [:]
        let crit = spec["criteria"] as? [[String: Any]] ?? []
        let units = s.draftResult["units"] as? [[String: Any]] ?? []
        let nScen = units.reduce(0) { $0 + (($1["scenarios"] as? [Any])?.count ?? 0) }
        return APFold(title: "명세", summary: "요구사항 \((spec["requirements"] as? [Any])?.count ?? 0) · 수용 기준 \(crit.count) · 시나리오 \(nScen)", axid: "ap-fold-spec", open: bind("spec")) {
            apNote("초안 세션이 쓴 명세입니다. 출발하면 세트(private/projects/\(s.project ?? "")/set/spec.md)에 남고 저장소에는 넣지 않습니다. 판정 시나리오는 자율주행 가지(autopilot/\(s.id))에 커밋했습니다.")
            ForEach(Array(crit.enumerated()), id: \.offset) { _, c in
                let cid = c["id"] as? String ?? ""
                let owner = units.first { (($0["criteria"] as? [[String: Any]]) ?? []).contains { $0["id"] as? String == cid } }
                let scs = ((owner?["scenarios"] as? [[String: Any]]) ?? []).filter { $0["criterion"] as? String == cid }.compactMap { $0["file"] as? String }
                VStack(alignment: .leading, spacing: 1) {
                    Text("\(cid) · \(c["text"] as? String ?? "")").boardFont(11).fixedSize(horizontal: false, vertical: true)
                    Text(scs.isEmpty ? "  시나리오 없음 → 출발 카드(실행 파일이 없는 수용 기준)" : scs.map { "  시나리오 " + $0 }.joined(separator: "\n"))
                        .boardFont(10.5, design: .monospaced).foregroundStyle(scs.isEmpty ? APColor.limit : Color.secondary)
                        .accessibilityIdentifier("ap-spec-\(cid)")
                }
            }
        }
    }
    // 기술 스택(주제로 시작) — 고른 칸(감지됨·주제에서 읽음·고름·직접 입력). 출발하면 세트 stack.json 으로 남고 단위 세션 첫 입력에 들어간다
    private var stackFold: some View {
        let lines = apStackLines(s.topicStack)
        return APFold(title: "기술 스택", summary: lines.isEmpty ? "정하지 않음" : lines.joined(separator: " / "), axid: "ap-fold-stack", open: bind("stack")) {
            ForEach(lines, id: \.self) { Text($0).boardFont(11).fixedSize(horizontal: false, vertical: true) }
            apNote("출발하면 세트(private/projects/\(s.project ?? "")/set/stack.json)에 남고 단위 세션 첫 입력에 들어갑니다." + (s.topicNew ? " 새 폴더라 바꾸려면 1단계에서 고친 뒤 다시 준비합니다." : " 기존 저장소의 규격이라 바꾸지 않습니다."))
        }
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

    // 설정 — 모델·멈출 선은 그 자리에서 바꾼다(5-2b, 객관식, 출발하면 프로젝트 설정에 남김). 게시 방식·보고서·알림은 4단계
    private var settingsFold: some View {
        let t = s.ticketsToMake
        let w = Int(s.stop7.rounded()), f = Int(s.stop5.rounded())
        let pubHead = (s.publish.mode == .auto ? "자동으로 올림" : "검수 뒤 올림") + (s.publish.report.map { " · " + apServiceName($0.service) } ?? "")
        let head = "모델 \(s.model) · 멈출 선 주간 \(w)% · 5시간 \(f)% · \(pubHead)" + (s.jira ? " · 만들 티켓 \(t.stories + t.tasks + t.moved)" : "")
        return APFold(title: "설정", summary: head, axid: "ap-fold-settings", open: bind("settings"), tag: s.settingsEdited || s.publishEdited ? "바꿈" : nil) {
            apNote(s.settingsEdited ? "바꾼 값은 이 출발에 쓰고, 출발하면 프로젝트 설정에 남깁니다(다음 출발의 기본값)."
                   : s.settingsFrom == "project" ? "프로젝트 설정에 저장된 값입니다. 그 자리에서 바꿉니다." : "지난 실행의 실행 설정을 그대로 잇습니다. 그 자리에서 바꿉니다.")
            pickRow("모델", apModelOptions(current: s.model), s.model, axid: "ap-set-model") { s.setModel($0) }
            pickRow("멈출 선 주간", apStopOptions(apStopWeekChoices, current: w).map(String.init), String(w), suffix: "%", axid: "ap-set-week") { s.setStop(week: Int($0)) }
            pickRow("5시간", apStopOptions(apStopFiveChoices, current: f).map(String.init), String(f), suffix: "%", axid: "ap-set-five") { s.setStop(five: Int($0)) }
            ForEach(s.settingsProblems, id: \.self) { apNote("· " + $0, APColor.fail) }
            apNote("모델을 바꾸면 단위 예상(보정)도 그 모델의 지난 값으로 다시 셉니다.")
            publishRows
            notifyRows
            if s.jira { row("만들 티켓", "\(t.stories + t.tasks + t.moved)건 · 출발 확인 위 '만들 티켓' 나무") }
        }
    }
    // 게시 방식(4단계 ⑩-1, 와이어프레임 출발 확인 ⑧): 검수 뒤 올림(기본)·자동으로 올림, 보고서 받을 곳, Jira 댓글. 연결이 없으면 알림만(출발은 막지 않음)
    @ViewBuilder private var publishRows: some View {
        let cs = APConnections.shared
        pickRow("게시 방식", ["검수 뒤 올림", "자동으로 올림"], s.publish.mode == .auto ? "자동으로 올림" : "검수 뒤 올림", axid: "ap-pub-mode") {
            s.setPublish(mode: $0 == "자동으로 올림" ? .auto : .review)
        }
        pickRow("보고서 받을 곳", ["없음", "Confluence", "Notion"], ["none": "없음", "confluence": "Confluence", "notion": "Notion"][s.pubService] ?? "없음", axid: "ap-pub-target") {
            s.setPublish(service: ["없음": "none", "Confluence": "confluence", "Notion": "notion"][$0] ?? "none")
        }
        if s.pubService == "confluence" {
            pubField("스페이스 키", "예: TODO", $s.pubSpace, axid: "ap-pub-space")
            pubField("상위 페이지 id", "숫자 · 비우면 스페이스 맨 위", $s.pubParent, axid: "ap-pub-parent")
        } else if s.pubService == "notion" {
            pubField("상위 페이지 id", "32자리 16진수(통합을 초대한 페이지)", $s.pubParent, axid: "ap-pub-parent")
        }
        pickRow("Jira 댓글", ["끔", "켬"], s.publish.jiraComments ? "켬" : "끔", axid: "ap-pub-comments") { s.setPublish(comments: $0 == "켬") }
        ForEach(s.publishProblems, id: \.self) { apNote("· " + $0, APColor.fail) }
        ForEach(apPublishNotes(s.publish, connections: cs.list, secrets: cs.secrets, dev: apDevBuild, now: Date(), tz: .current), id: \.self) { apNote("· " + $0, APColor.wait) }
        apNote(s.publish.mode == .review
               ? "검수 뒤 올림: 실행이 끝나도 바깥에 올라가지 않습니다. 보드의 '올릴 글'에서 받을 곳·계정·본문을 보고 올리기를 눌러야 올라가고, 오래 두어도 자동으로 올리지 않습니다."
               : "자동으로 올림: 연결이 있으면 엔진이 바로 올립니다(보드 창이 꺼져 있어도). 연결이 없거나 만료면 글은 쌓이고 일은 계속합니다.")
        apNote("Jira 댓글은 티켓 키가 있는 단위에만 판정 뒤 하나씩 만듭니다. 바깥 글의 첫 줄은 늘 '오토파일럿 작성'입니다.")
    }
    private func pubField(_ k: String, _ ph: String, _ b: Binding<String>, axid: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(k).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            TextField(ph, text: b).textFieldStyle(.roundedBorder).font(.system(size: 11.5)).frame(maxWidth: 280)
                .accessibilityIdentifier(axid)
                .onSubmit { s.setPublish() }
            APButton(title: "적용", axid: axid + "-apply") { s.setPublish() }
            Spacer(minLength: 0)
        }
    }
    private func pickRow(_ k: String, _ options: [String], _ cur: String, suffix: String = "", axid: String, _ act: @escaping @MainActor (String) -> Void) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(k).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            WrapRow(spacing: 4) {
                ForEach(options, id: \.self) { o in
                    APButton(title: o + suffix, tint: o == cur ? Palette.accent : nil, enabled: !s.starting && !s.preparing && !s.rechecking, axid: "\(axid)-\(o)") { act(o) }
                }
            }
            Spacer(minLength: 0)
        }
    }
    private func row(_ k: String, _ v: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(k).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            Text(v).boardFont(11).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 4)
        }
    }
    // 알림(4단계 ⑩-3, 와이어프레임 출발 확인 ⑧) — 이 프로젝트의 상태 알림·3시간 요약을 받을 webhook(켬·끔). 고르면 게시 설정 notify 로 고정된다
    @ViewBuilder private var notifyRows: some View {
        let hooks = APConnections.shared.webhooks.filter { $0.scopes.contains("notify") }
        let on = Set(s.publish.notify ?? [])
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("알림").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            if hooks.isEmpty {
                Text("알림 webhook 이 없습니다(설정 › 연결에서 더함). 상태 알림·3시간 요약을 보내지 않습니다").boardFont(11).fixedSize(horizontal: false, vertical: true)
            } else {
                WrapRow(spacing: 4) {
                    ForEach(hooks, id: \.id) { h in
                        APButton(title: (on.contains(h.id) ? "켬 · " : "끔 · ") + (h.name.isEmpty ? h.id : h.name) + " · " + h.format.label,
                                 tint: on.contains(h.id) ? Palette.accent : nil, enabled: !s.starting && !s.preparing && !s.rechecking, axid: "ap-notify-\(h.id)") {
                            var n = s.publish.notify ?? []
                            if on.contains(h.id) { n.removeAll { $0 == h.id } } else { n.append(h.id) }
                            s.setPublish(notify: n)
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
        if !hooks.isEmpty {
            apNote("켠 webhook 에 정한 때에만 보냅니다: 출발, 대기에 들어감(단위를 시작한 뒤 한 번), 끝(실행마다 한 번), 3시간 요약(기다리는 올릴 글 수·합치기 기다림 포함). 엔진이 보내 보드 창이 꺼져 있어도 갑니다.")
        }
    }

    // 자율주행 지시 — 펼치면 그 자리에서 지시 글(읽기만), 나머지 칸은 접혀 있다. 고치기·지난 판 차이·하네스 확인 지점은 뒤 조각(7단계)
    private var instructionFold: some View {
        let diffHead = apInstructionHead(old: s.lastSet?.text, new: s.setText, date: apDay(s.lastSet?.at))
        let head = (s.setText.map { "지시 \($0.count)자 · " } ?? "") + diffHead
        let diff = s.lastSet.map { apLineDiff($0.text, s.setText ?? "") } ?? []
        let counts = apDiffCount(diff)
        return APFold(title: "자율주행 지시", summary: head, axid: "ap-fold-set", open: bind("set"), tag: s.editingSet ? "고치는 중" : nil) {
            if s.editingSet {
                apNote(s.source == "topic" || (s.isImport && s.importDraftedSet) ? "지시 글 · 이 자리에서 고치는 중입니다. 저장하면 출발 때 세트에 쓸 글이 바뀝니다(출발을 누르기 전에는 세트를 바꾸지 않습니다)."
                                           : "지시 글 · 이 자리에서 고치는 중입니다. 저장하면 세트(private/projects/\(s.project ?? "")/set/autopilot.md)가 바뀌고 다음 출발의 기준이 됩니다.")
                TextEditor(text: $s.setDraft)
                    .font(.system(size: 11, design: .monospaced))
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 220, maxHeight: 520)
                    .padding(6)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.3)))
                    .accessibilityIdentifier("ap-set-editor")
                HStack(spacing: 6) {
                    APButton(title: "저장", primary: true, axid: "ap-set-save") { s.saveSet() }
                    APButton(title: "그만두기", axid: "ap-set-cancel") { s.cancelEditSet() }
                    let c = s.lastSet.map { apDiffCount(apLineDiff($0.text, s.setDraft)) }
                    if let c { apNote("지난 판과 +\(c.add) −\(c.del)") }
                }
            } else if let t = s.setText {
                HStack(spacing: 6) {
                    apNote("지시 글 · 펼쳐 둡니다")
                    Spacer(minLength: 4)
                    APButton(title: "이 자리에서 고치기", enabled: s.project != nil && !s.starting, help: "지시 글을 이 자리에서 고칩니다. 고친 판이 다음 출발의 기준이 됩니다", axid: "ap-set-edit") { s.beginEditSet() }
                }
                Text(t).boardFont(11, design: .monospaced).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
                    .accessibilityIdentifier("ap-set-text")
            } else {
                apNote("이 프로젝트의 자율주행 세트(private/projects/\(s.project ?? "")/set/autopilot.md)가 없습니다. 엔진이 공통 규칙만 얹습니다.")
                APButton(title: "이 자리에서 쓰기", enabled: s.project != nil && !s.starting, help: "이 프로젝트의 자율주행 지시를 새로 씁니다", axid: "ap-set-edit") { s.beginEditSet() }
            }
            if let n = s.setNote { apNote(n, n.hasPrefix("저장했습니다") ? APColor.ok : APColor.fail) }
            if let l = s.lastSet, counts.add + counts.del > 0 {
                APFold(title: "지난 판과 차이 +\(counts.add) −\(counts.del)", summary: l.run == "지금 세트" ? "\(apDay(l.at)) 판 · 지금 세트 파일(출발하면 바뀜)" : "\(apDay(l.at)) 판(실행 \(l.run)) · 기록 저장소", axid: "ap-fold-set-diff", open: bind("setdiff")) {
                    ForEach(Array(diff.prefix(300).enumerated()), id: \.offset) { _, d in
                        Text((d.kind == .add ? "+ " : d.kind == .del ? "− " : "  ") + d.text)
                            .boardFont(10.5, design: .monospaced)
                            .foregroundStyle(d.kind == .add ? APColor.ok : d.kind == .del ? APColor.fail : Color.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    if diff.count > 300 { apNote("… \(diff.count - 300)줄 더") }
                }
            }
            let scen = s.chosen.reduce(0) { $0 + APStart.scenarios($1).count }
            APFold(title: "판정 명령 \(s.chosen.count)" + (scen > 0 ? " · 시나리오 \(scen)" : ""),
                   summary: "읽기 전용 · 출발 뒤에는 고정본으로 판정합니다 · 판정은 샌드박스 안에서", axid: "ap-fold-checks", open: bind("checks")) {
                ForEach(s.chosen) { u in
                    VStack(alignment: .leading, spacing: 1) {
                        if !u.check.isEmpty || APStart.scenarios(u).isEmpty {
                            Text("\(u.n). " + u.check).boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(2)
                        }
                        // 시험 실행기(⑪-1): 시나리오 목록(작업 폴더 기준, 출발 때 고정본으로)과 짝지은 수용 기준
                        ForEach(Array(APStart.scenarios(u).enumerated()), id: \.offset) { _, sc in
                            Text("\(u.n). 시나리오 \(sc["file"] as? String ?? "")(\(sc["kind"] as? String == "browser" ? "브라우저" : "셸")\((sc["criterion"] as? String).map { " · 수용 기준 \($0)" } ?? ""))")
                                .boardFont(10.5, design: .monospaced).foregroundStyle(.secondary).lineLimit(2)
                        }
                        // 판정이 스스로 띄우는 서버 포트(⑪-2 보안 검토: 주제로 시작이면 초안 세션이 적은 값이라 사람이 보고 출발한다)
                        if let ports = ((try? JSONSerialization.jsonObject(with: u.def)) as? [String: Any])?["check_ports"] as? [Int], !ports.isEmpty {
                            Text("\(u.n). 판정이 여는 localhost 포트 " + ports.map(String.init).joined(separator: ", ") + (s.source == "topic" ? " (초안 세션이 적음, 판정 샌드박스는 이 포트만 엶)" : ""))
                                .boardFont(10.5, design: .monospaced).foregroundStyle(s.source == "topic" ? APColor.limit : Color.secondary).lineLimit(2)
                                .accessibilityIdentifier("ap-check-ports-\(u.id)")
                        }
                    }
                }
                apNote("판정은 샌드박스 안에서 돕니다. 쓰기는 작업 폴더·쓰기 범위·증거 폴더만, 네트워크는 실행의 localhost 설정과 단위 check_ports 만 엽니다.")
            }
            // 판정 차이(⑪-3): 세트 이어 쓰기(판정 놓침이 있어 다시 씀) 또는 판정 고치기로 바뀐 판정. 하네스가 그대로여도 보인다
            if !s.judgeChanges.isEmpty {
                APFold(title: "판정 차이 \(s.judgeChanges.count)곳",
                       summary: s.judgeChangeFrom == "revise" ? "세트 이어 쓰기 · 판정 놓침 \((s.missInfo["open"] as? NSNumber)?.intValue ?? 0)건이 있어 다시 씀(하네스 그대로)"
                                                              : "판정 고치기 \(s.fixRound)차 · 지난 초안과 견줌",
                       axid: "ap-fold-judge-diff", open: bind("judgediff")) {
                    ForEach(Array(s.judgeChanges.enumerated()), id: \.offset) { _, c in
                        VStack(alignment: .leading, spacing: 1) {
                            Text("\(c["title"] as? String ?? "") · \(apJudgeFieldLabel(c["field"] as? String ?? ""))").boardFont(10.5, weight: .semibold)
                            Text("− " + (c["old"] as? String ?? "")).boardFont(10.5, design: .monospaced).foregroundStyle(APColor.fail).fixedSize(horizontal: false, vertical: true)
                            Text("+ " + (c["new"] as? String ?? "")).boardFont(10.5, design: .monospaced).foregroundStyle(APColor.ok).fixedSize(horizontal: false, vertical: true)
                            if let w = c["why"] as? String, !w.isEmpty { apNote(w) }
                        }
                        .accessibilityElement(children: .combine).accessibilityIdentifier("ap-judge-diff-\(c["unit"] as? String ?? "")")
                    }
                }
            }
            apNote("하네스 지문: 출발 때 엔진이 하네스 사본과 지문을 남기고, 끝나면 같은지 확인합니다. 저장소 하네스는 고치지 않습니다.")
            if s.source == "topic" {
                apNote("주제로 시작: 이 지시는 초안 세션(모델)이 쓴 초안이고 지금은 준비 폴더(drafts/\(s.id)/draft/)에만 있습니다. 출발을 누를 때만 세트(private/projects/\(s.project ?? "")/set/autopilot.md)에 쓰고(고치기 전 글은 set-backup/), 준비를 버리면 세트는 그대로입니다. 하네스 확인 지점 가운데 지시가 다루지 않은 곳은 출발 카드로 묻습니다.")
            } else if s.isImport {
                apNote(s.importDraftedSet ? "가져오기: 이 프로젝트 세트에 지시가 없어 가져오기 세션(모델)이 쓴 초안입니다. 지금은 준비 폴더(drafts/\(s.id)/import/out/)에만 있고 출발을 누를 때만 세트에 씁니다. 하네스 확인 지점 가운데 지시가 다루지 않은 곳은 출발 카드로 묻습니다."
                                          : "가져오기: 이 프로젝트 세트의 지시를 그대로 씁니다. 하네스 확인 지점 가운데 그 지시가 다루지 않은 곳은 출발 카드로 묻습니다(하네스가 바뀌면 다시 쓰는 일반판은 7단계 뒤 조각).")
            } else {
                apNote("하네스 확인 지점 읽기와 지시 초안은 하네스 읽기(7단계)에서 엽니다.")
            }
        }
    }

    // 만들 티켓 나무(Jira 연결 프로젝트, 와이어프레임 출발 확인 ⑬) — 에픽 아래 스토리, 스토리 아래 작업. 출발을 누르면 보드(앱 안 엔진 tickets)가
    // 이 나무를 Jira 에 한꺼번에 만든다(4단계 ⑩-3, 게시 방식과 상관없이). 항목마다 엔진이 남긴 상태(만듦·실패·확인 필요)를 보이고, 다시 누르면 남은 것만
    private var ticketTree: some View {
        let picked = s.chosen + s.heldByCards
        let ids = Set(picked.compactMap(\.story))
        let st = s.stories.filter { ids.contains($0.id) }
        let t = s.ticketsToMake
        let make = t.stories + t.tasks + t.moved
        let book = s.ticketBook
        let plan = s.ticketPlan(picked)
        let made = plan.entries.filter { book.entries[$0.id]?.state == .made }.count
        return APFold(title: "만들 티켓", summary: "에픽 \(s.epic.isEmpty ? "?" : s.epic) 아래 스토리 \(st.count) · 작업 \(picked.count)"
                      + (make > 0 ? " · 새로 만들거나 고칠 것 \(make)" : (made > 0 ? " · 만든 티켓 \(made)" : " · 모두 이미 있는 티켓")),
                      axid: "ap-fold-tickets", open: bind("tickets"), tint: make > 0 ? APColor.limit : nil) {
            HStack(spacing: 6) {
                APTicket(key: s.epic.isEmpty ? "에픽 없음" : s.epic)
                if let e = s.epicInfo, e.key == s.epic, e.read, e.found, e.isEpic { APChip(text: "에픽 · 있음", color: APColor.ok); Text(e.title).boardFont(11.5, weight: .semibold).lineLimit(1) }
                else { apNote("에픽 · 번호만 확인했습니다(Atlassian 연결에 읽기 범위가 있으면 제목을 읽습니다)") }
            }
            // 단위 시작 전 티켓 읽기(4단계 ⑩-2) — 어느 단위가 어느 티켓을 읽는지는 아래 나무의 작업 줄(단위 하나에 작업 티켓 하나)
            let cn = APConnections.shared
            let why = apReadBlocker(.atlassian, connection: cn.conn("atlassian"), hasSecret: cn.secrets.contains("atlassian"), dev: apDevBuild, now: Date(), tz: .current)
            apNote(why == nil ? "단위를 시작하기 전에 엔진이 그 단위의 작업 티켓을 읽어 단위 근거 묶음에 넣습니다(메타 칸과 단위 글이 이름으로 가리킨 절만, 믿지 않는 자료 표시로 감쌈)"
                              : "단위 시작 전 티켓 읽기: \(why ?? ""). 읽지 않고 단위를 시작합니다(요청 0)", why == nil ? .secondary : APColor.limit)
                .accessibilityIdentifier("ap-ticket-read-note")
            ForEach(st) { story in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        if let k = story.key { APTicket(key: k); APChip(text: "있음", color: APColor.ok) }
                        else { APChip(text: s.storyLabel(story), color: APColor.jira, strong: true); entryState("story:" + story.id, book) }
                        APChip(text: apKind(story.kind), color: APColor.board)
                        if !story.title.isEmpty { Text(story.title).boardFont(11.5).lineLimit(1) }
                    }
                    ForEach(picked.filter { $0.story == story.id }) { u in
                        HStack(spacing: 6) {
                            if let k = u.task { APTicket(key: k) } else { APChip(text: "새 작업", color: APColor.limit); entryState("task:" + u.id, book) }
                            if let k = u.task, u.story != u.origStory { APChip(text: "다른 스토리로 옮김", color: APColor.limit); entryState("move:" + k, book) }
                            Text(u.title).boardFont(11).lineLimit(1)
                            if !u.depends.isEmpty { apNote("앞: " + u.depends.map { d in s.units.first { $0.id == d }.map { "\($0.n)" } ?? "?" }.joined(separator: "·")) }
                        }
                        .padding(.leading, 16)
                    }
                }
            }
            // 실패·확인 필요 줄 — 까닭 그대로(엔진이 상태 번호로만 쓴 말), 확인 필요는 "그래도 만들기"
            let bad = plan.entries.filter { [.failed, .uncertain, .waiting].contains(book.entries[$0.id]?.state) && book.entries[$0.id]?.last != nil }
            ForEach(bad, id: \.id) { e in
                let r = book.entries[e.id]
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    APChip(text: r?.state == .uncertain ? "확인 필요" : (r?.state == .failed ? "실패" : "기다림"), color: r?.state == .waiting ? APColor.wait : APColor.fail, strong: true)
                    Text("\(e.title.isEmpty ? e.id : e.title): \(r?.last?.note ?? "")").boardFont(11).fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("ap-ticket-fail-\(e.id)")
                    Spacer(minLength: 4)
                    if r?.state == .uncertain {
                        let on = s.ticketConfirm.contains(e.id)
                        APButton(title: on ? "확인함" : "그래도 만들기", tint: APColor.limit, enabled: !s.starting, help: "Jira 에 같은 티켓이 이미 생겼는지 먼저 봅니다", axid: "ap-ticket-confirm-\(e.id)") {
                            if on { s.ticketConfirm.remove(e.id) } else { s.ticketConfirm.insert(e.id) }
                        }
                    }
                }
            }
            if let n = s.ticketNote { apNote(n.text, n.bad ? APColor.fail : APColor.ok).accessibilityIdentifier("ap-ticket-note") }
            apNote("작업 티켓은 스토리 아래 둘 수 있는 하위 작업 종류로 만듭니다. 번호는 대기열과 함께 고정되어 실행 중에는 티켓을 만들지 않습니다(실행 중 Jira 요청은 단위 시작 전 티켓 읽기와, 켰으면 댓글뿐. 새로 필요해진 티켓은 출발 카드 답대로).")
            if make > 0 {
                if let why = s.ticketBlocker { apNote("티켓을 만들 수 없습니다: \(why). 그때까지 출발이 잠깁니다. 이미 있는 티켓의 단위만 고르면 출발할 수 있습니다.", APColor.limit) }
                else { apNote("출발을 누르면 보드가 이 나무를 Jira 에 한꺼번에 만들고(첫 줄 '오토파일럿 작성'), 만든 번호를 대기열과 가지 이름에 넣은 뒤 출발합니다. 다 만들지 못하면 출발하지 않고 실패한 티켓과 까닭이 여기 보이며, 다시 누르면 남은 것만 만듭니다.", APColor.wait) }
            }
        }
    }
    @ViewBuilder private func entryState(_ id: String, _ book: APTicketBook) -> some View {
        switch book.entries[id]?.state {
        case .made: APChip(text: "만듦 " + (book.entries[id]?.key ?? ""), color: APColor.ok, strong: true).accessibilityIdentifier("ap-ticket-made-\(id)")
        case .failed: APChip(text: "실패", color: APColor.fail, strong: true)
        case .uncertain: APChip(text: "확인 필요", color: APColor.fail, strong: true)
        case .creating: APChip(text: "만드는 중", color: APColor.work)
        default: APChip(text: id.hasPrefix("move:") ? "부모 바꿈" : "새로 만듦", color: APColor.limit)
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
func apGitRun(_ args: [String], _ cwd: String, env: [String] = []) -> (code: Int32, out: String) {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
    p.arguments = apGitHardenC + args
    p.currentDirectoryURL = URL(fileURLWithPath: cwd)
    p.environment = apGitEnv(env)
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
    var gitRec: [String: APGitRecord] = [:]  // 저장소 → 출발 때 git 자리(⑦ 보드 git 받침, GIT_DIR 박기·바꿔치기 가리기)
    var runWorktree: [String: String] = [:]  // 이 실행의 작업 트리 경로 → 저장소(finding 5: 그 자리 status 는 기록된 git dir 로 박아 돌린다)
    var gitBlocked: [String: String] = [:]   // 출발 때와 git 폴더가 달라져 보드가 돌리지 않은 저장소 → 까닭(readGit 가 채움)
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
            m.gitRec[k] = APGitRecord(commonDir: apText(i["common_dir"]), commonIno: apText(i["common_ino"]),
                                      workGitDir: apText(i["work_git_dir"]), workGitIno: apText(i["work_git_ino"]))
            if let wt = apText(i["work_tree"]) { m.runWorktree[wt] = k }   // 이 실행의 작업 트리(모델이 쓸 수 있던 자리) → 저장소. finding 5: 그 자리 status 는 박아 돌린다
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
            m.steps = apDefaultPromotion((((br["start"] as? [String: Any])?[first] as? [String: Any]) ?? [:]).keys.sorted())
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
        // 하위 모듈·바꿔치기로 보드가 그 저장소의 git 을 돌리지 않았으면(⑦ 보드 git 받침) 승격을 잠근다
        if let b = v?.gitBlocked, let r = st.refs.keys.first(where: { b[$0] != nil }) {
            return (st.refs.count > 1 ? (r as NSString).lastPathComponent + ": " : "") + (b[r] ?? "") + " 보드가 가지를 읽지 않았습니다. 사람이 확인하세요"
        }
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
            let r = apEngine(args + (apDevBuild ? ["--dev"] : []))   // 엔진은 그 실행의 뿌리와 맞는지만 본다(5-3b ⑨)
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
        // 앞 읽기 값(앞 실행이나 엔진이 한 일 전의 가지)을 그대로 보이지 않게 비운다(5-3b ⑦) — 읽는 동안 단계·저장소 줄·지우기 글은 "읽는 중"
        plans = [:]; spans = [:]; existing = [:]; contained = []; foreign = []
        let gitRec = m.gitRec, runWorktree = m.runWorktree
        queue.async {
            var plans: [String: APMergePlan] = [:], spans: [String: APMergePlan] = [:], have: [String: Set<String>] = [:], contained = Set<String>()
            var wts: [String: [String: String]] = [:], heads: [String: [String: String]] = [:], foreign = Set<String>()
            // 저장소마다 git 자리를 박는 env 와 막은 까닭(⑦ 보드 git 받침). 실행 기록(gitRec)이 있으면 그때 git 폴더가 지금과 같은지 보고(공통·작업 트리 둘 다, 보안 검토 4),
            // 다르면 그 저장소는 돌리지 않는다(사람이 확인). 같으면 GIT_DIR·GIT_CEILING 을 박아 ref·객체를 읽는다. 기록이 없으면(옛 실행) 공통 보호로만
            var pin: [String: [String]] = [:], blocked: [String: String] = [:]
            var wtPin: [String: [String]] = [:]   // 이 실행의 작업 트리 경로 → status 를 박을 env(finding 5)
            func gitPin(_ r: String) -> [String] { pin[r] ?? [] }
            for r in Set(m.repos + m.stories.flatMap { $0.repos.map(\.repo) + $0.tasks.map(\.repo) }) where !r.isEmpty && FileManager.default.fileExists(atPath: r) {
                let rec = gitRec[r] ?? APGitRecord(commonDir: nil, commonIno: nil, workGitDir: nil, workGitIno: nil)
                let live = apGit(["rev-parse", "--git-common-dir"], r).map { $0.hasPrefix("/") ? apCanonical($0) : apCanonical(r + "/" + $0) }
                if let why = apBoardGitChanged(rec, liveCommon: live, liveCommonIno: live.flatMap(apGitStat), liveWorkIno: rec.workGitDir.flatMap(apGitStat)) {
                    blocked[r] = why; continue   // 바꿔치기 의심: 이 저장소의 가지를 읽지 않는다(시트가 까닭을 보이고 단추를 잠근다)
                }
                pin[r] = apBoardRefsPin(rec, repoRoot: r)
                // finding 5: 이 실행의 작업 트리(모델이 쓸 수 있던 자리)에서 status 를 돌릴 때는 기록된 git dir 로 GIT_DIR·GIT_WORK_TREE 를 박는다 —
                // 그러면 설정은 엔진이 믿는 .git 의 것만 읽혀, 모델이 쓴 .gitattributes 는 이미 있는 filter 이름을 고를 수만 있고 filter 를 새로 정의하지 못한다.
                // 승격 대상 가지가 걸린 사람의 작업 트리는 박지 않고 공통 보호로 둔다(사람이 자기 저장소에서 git status 를 치는 것과 같다, 진행 상황.md 설계 판단)
                if let wg = rec.workGitDir { for (wtPath, repo) in runWorktree where repo == r { wtPin[wtPath] = ["GIT_DIR=" + wg, "GIT_WORK_TREE=" + wtPath] } }
                var hs: [String: String] = [:]
                for line in (apGit(["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/heads/"], r, env: gitPin(r)) ?? "").split(separator: "\n") {
                    let f = line.split(separator: "\t")
                    if f.count == 2, f[0].hasPrefix("refs/heads/") { hs[String(f[0].dropFirst(11))] = String(f[1]) }
                }
                heads[r] = hs
                have[r] = Set(hs.keys)
                wts[r] = APMerge.checkedOut(r)   // worktree list 는 공통 보호로만(작업 트리 자리를 GIT_DIR 로 박지 않는다)
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
                    let base = apCarriesBase(source: st.source, after: st.after, base: m.run.bases[r]) ? m.baseHeads[r] : nil   // 규칙 AutopilotRules/Branches.swift
                    plans[st.id + "|" + r] = have[r] == nil ? APMergePlan(state: "nosrc") : APMerge.plan(r, src: src, into: st.into, wts: wts[r] ?? [:], baseHead: base, env: gitPin(r), wtPin: wtPin)
                }
            }
            if !m.run.work.isEmpty {
                for r in m.repos where have[r] != nil { if let b = m.baseHeads[r] { spans["all|" + r] = APMerge.span(r, b, m.run.work, env: gitPin(r)) } }
            }
            for s in m.stories {
                // 작업 가지는 그 저장소의 스토리 가지에서 본다. 스토리 가지를 지운 뒤(합친 가지 지우기)에는 마지막 자율주행 단계의 가지(live)에 없는 커밋을 센다(엔진 cleanup 과 같은 기준)
                for t in s.tasks where !t.branch.isEmpty && have[t.repo]?.contains(t.branch) == true && t.merge != "merged" {
                    let sb = s.storyBranch(t.repo) ?? s.branch
                    spans[t.spanKey] = APMerge.span(t.repo, have[t.repo]?.contains(sb) == true ? sb : (lastAuto ?? sb), t.branch, env: gitPin(t.repo))
                }
                guard let live = lastAuto else { continue }
                for r in Set(s.pairs.map(\.repo)) where have[r] != nil {
                    guard let lh = apGit(["rev-parse", "-q", "--verify", "refs/heads/\(live)^{commit}"], r, env: gitPin(r)), !lh.isEmpty else { continue }
                    for (_, b) in s.pairs.filter({ $0.repo == r }) where have[r]?.contains(b) == true {
                        if apGitRun(["merge-base", "--is-ancestor", "refs/heads/" + b, lh], r, env: gitPin(r)).code == 0 { contained.insert(r + "|" + b) }
                    }
                }
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.gen == g else { return }
                    self.plans = plans; self.spans = spans; self.existing = have; self.contained = contained; self.foreign = foreign
                    if var vv = self.v { vv.gitBlocked = blocked; self.v = vv }
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
    // env: 실행 기록이 있는 저장소면 GIT_DIR·GIT_CEILING 을 박는다(⑦ 보드 git 받침). ref·객체만 다루므로 GIT_WORK_TREE 는 넣지 않는다
    nonisolated static func head(_ r: String, _ b: String, env: [String] = []) -> String? {
        apGit(["rev-parse", "-q", "--verify", "refs/heads/\(b)^{commit}"], r, env: env).flatMap { $0.isEmpty ? nil : $0 }
    }
    nonisolated static func numstat(_ r: String, _ a: String, _ b: String, env: [String] = []) -> [APDiffFile] {
        (apGit(["diff", "--numstat", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", "--ignore-submodules=all", "\(a)...\(b)"], r, env: env) ?? "").split(separator: "\n").compactMap { l in
            let f = l.split(separator: "\t", maxSplits: 2).map(String.init)
            return f.count == 3 ? APDiffFile(path: f[2], add: f[0], del: f[1]) : nil
        }
    }
    // 합치기 미리 보기 — 엔진 mergeRefs 와 같은 판단(같은 merge-tree 명령이라 미리 본 것과 엔진 판정이 어긋나지 않는다. merge-tree 는 ref 를 움직이지 않고
    // 이름 없는 tree 객체만 남긴다). env 는 받는 가지 저장소의 GIT_DIR 박기(ref·객체). 작업 트리 status 는 그 작업 트리 자리에서 공통 보호로만 돈다
    nonisolated static func plan(_ r: String, src: String, into: String, wts: [String: String], baseHead: String?, env: [String] = [], wtPin: [String: [String]] = [:]) -> APMergePlan {
        var p = APMergePlan()
        guard let sh = head(r, src, env: env) else { p.state = "nosrc"; return p }
        guard let ih = head(r, into, env: env) else { p.state = "nointo"; return p }
        p.a = ih; p.b = sh
        p.commits = Int(apGit(["rev-list", "--count", "\(ih)..\(sh)"], r, env: env) ?? "") ?? 0
        p.behind = Int(apGit(["rev-list", "--count", "\(sh)..\(ih)"], r, env: env) ?? "") ?? 0
        if p.commits == 0 { p.state = "same" }
        else if p.behind == 0 { p.state = "ff" }
        else {
            let mt = apGitRun(["merge-tree", "--write-tree", "--name-only", "--no-messages", ih, sh], r, env: env)
            let lines = mt.out.split(separator: "\n").map(String.init)
            switch mt.code {
            case 0: p.state = "merge"
            case 1: p.state = "conflict"; p.conflicts = Array(lines.dropFirst())
            default: p.state = "error"; p.error = lines.first ?? "종료 코드 \(mt.code)"
            }
        }
        if p.state != "same" { p.files = numstat(r, ih, sh, env: env) }
        if let w = wts[into] {
            p.wt = w
            // 받는 가지가 걸린 작업 트리의 더러움. 이 실행의 작업 트리(모델이 쓸 수 있던 자리)면 기록된 git dir 로 박아 돌리고(wtPin, finding 5),
            // 승격 대상 가지가 걸린 사람의 작업 트리면 박지 않고 공통 보호로만 돈다
            if p.state != "same" { p.wtDirty = !(apGit(["status", "--porcelain", "--untracked-files=no", "--ignore-submodules=all"], w, env: wtPin[w] ?? []) ?? "x").isEmpty }
        }
        if let bh = baseHead { p.withBase = Int(apGit(["rev-list", "--count", "\(ih)..\(bh)"], r, env: env) ?? "") ?? 0 }
        return p
    }
    // 바뀐 파일 범위(a...b) — 커밋 수와 파일마다 더한 줄·뺀 줄
    nonisolated static func span(_ r: String, _ a: String, _ b: String, env: [String] = []) -> APMergePlan {
        var p = APMergePlan()
        guard let bh = head(r, b, env: env) else { p.state = "nosrc"; return p }
        let ah = head(r, a, env: env) ?? a
        p.state = "span"; p.a = ah; p.b = bh
        p.commits = Int(apGit(["rev-list", "--count", "\(ah)..\(bh)"], r, env: env) ?? "") ?? 0
        p.files = numstat(r, ah, bh, env: env)
        return p
    }

    // 바뀐 줄 — 파일을 누를 때만 읽는다(400줄까지)
    func toggleDiff(_ key: String) { if openDiff.contains(key) { openDiff.remove(key) } else { openDiff.insert(key) } }
    func pick(_ key: String, _ file: String, repo: String, _ p: APMergePlan) {
        picked[key] = file
        let pk = key + "|" + file
        guard patches[pk] == nil, !p.a.isEmpty, !p.b.isEmpty else { return }
        queue.async {
            let t = apGit(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", "--ignore-submodules=all", "\(p.a)...\(p.b)", "--", file], repo) ?? ""
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
                APButton(title: "다시 보기", icon: "arrow.clockwise", enabled: m.busy == nil && !m.reading, help: "run.json 과 가지를 다시 읽습니다(저장소에서 사람이 고친 뒤)", axid: "ap-merge-reload") { m.reload() }
                APButton(title: "닫기", enabled: m.busy == nil, help: "가지는 그대로 남습니다", axid: "ap-merge-close") { m.close() }
            }
            // 첫 줄에는 늘 같은 폭의 것만 둔다 — 엔진이 하는 일(스토리 키·"합치지 못한 작업 가지까지 지우기" 등)·읽는 중 꼬리표·출발 준비로 돌아가기를
            // 첫 줄에 두면 줄이 시트보다 넓어져 머리 전체가 밀리고 실행 이름이 잘렸다(5-3b ⑤). 돌아가기는 둘째 줄(접힘), 하는 일은 셋째 줄(줄 바꿈)
            if let r = v?.run {
                WrapRow(spacing: 6) {
                    Text(r.name).boardFont(12.5, weight: .bold).lineLimit(1)
                    Text("\(apDate(r.started)) → \(r.ended.map { apDay($0) == apDay(r.started) ? apClock($0) : apDate($0) } ?? "?")").boardFont(11, design: .monospaced).foregroundStyle(.secondary).fixedSize()
                    APChip(text: r.modeLabel, color: r.inPlace ? APColor.wait : APColor.muted)
                    if v?.jira == true { APChip(text: "Jira 연결" + (v?.epic.map { " · 에픽 \($0)" } ?? ""), color: APColor.jira) }
                    if let p = r.project, p != r.name { APChip(text: p, color: APColor.board) }
                    if m.fromStart, APStart.shared.hasDraft {
                        APButton(title: "출발 준비로 돌아가기", enabled: m.busy == nil, help: "닫아 둔 출발 준비를 다시 엽니다", axid: "ap-merge-to-start") { m.backToStart() }
                    }
                }
            }
            if let b = m.busy {
                Text("엔진이 하는 중: \(b)").boardFont(11, weight: .semibold).foregroundStyle(APColor.work)
                    .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("ap-merge-busy")
            } else if m.reading {
                Text("가지를 읽는 중").boardFont(11).foregroundStyle(.secondary).accessibilityIdentifier("ap-merge-reading")
            }
            if v?.run != nil {
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
            if !v.gitBlocked.isEmpty {   // ⑦ 보드 git 받침: 바꿔치기·하위 모듈로 보드가 git 을 돌리지 않은 저장소
                APBox(title: "보드가 git 을 돌리지 않은 저장소 \(v.gitBlocked.count)", note: "git 폴더가 출발 때와 다르거나 하위 모듈이 있습니다. 승격이 잠깁니다. 사람이 확인하세요", tint: APColor.fail, axid: "ap-merge-gitblocked") {
                    ForEach(v.gitBlocked.sorted(by: { $0.key < $1.key }), id: \.key) { r, why in
                        Text("\((r as NSString).lastPathComponent): \(why)").boardFont(11).foregroundStyle(APColor.fail).fixedSize(horizontal: false, vertical: true)
                    }
                }
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

    // ① 요약 — 단위 결과 수, 일한 시간, 비용, 보고서 보기(3단계, 보고서 창 APReportViewer)
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
                APButton(title: "보고서 보기", icon: "doc.richtext", help: "결과 기록과 엔진 기록으로 그린 보고서(흐름 그림·단위별 기록)를 엽니다",
                         axid: "ap-merge-report") { APReportViewer.show(run: v.run.id, runDir: v.run.dir, title: v.run.title.isEmpty ? v.run.name : v.run.title) }
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
        // 자율주행 가지를 보드 밖에서 지웠다(저장소를 다 읽었고 어디에도 없음, 작업 트리도 없음) — 지울 것이 없어 버린 것으로만 적는다(5-3b ④)
        let gone = !m.reading && !v.run.work.isEmpty && !v.repos.isEmpty
            && v.repos.allSatisfy { m.existing[$0].map { !$0.contains(v.run.work) } ?? false }
            && !(v.run.worktreePath.map { FileManager.default.fileExists(atPath: $0) } ?? false)
        return APBox(title: "버리기", note: "어느 단계도 합치기 전에만", axid: "ap-merge-discard-box") {
            if gone, lock == nil {
                HStack(spacing: 8) {
                    APButton(title: "버린 것으로 적기", icon: "checkmark.circle", help: "엔진 finish discard: 지울 가지가 없으면 버린 것으로만 적습니다", axid: "ap-merge-discard-gone") { m.discard(nil) }
                    apNote("\(v.run.work) 가지가 이미 없습니다(보드 밖에서 지움). 버린 것으로 적으면 합치기 기다림이 닫힙니다.")
                }
            } else if m.confirm == "discard" {
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
            if !t.repo.isEmpty, m.existing[t.repo] == nil, m.reading {
                apNote(t.branch.isEmpty ? "가지를 읽는 중" : "\(t.branch) · 가지를 읽는 중")
            } else if t.status == "skipped" {
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
            apNote("이 스토리를 버렸습니다(남은 스토리 가지와 작업 가지를 지우고 버린 것으로 적음).")
        } else if m.reading || pairs.contains(where: { m.existing[$0.repo] == nil && FileManager.default.fileExists(atPath: $0.repo) }) {
            apNote("가지를 읽는 중입니다")   // 앞 읽기 값으로 지우기 글을 보이지 않는다(5-3b ⑦)
        } else if s.made.isEmpty {
            // 어느 저장소에도 스토리 가지를 만들지 않음(첫 작업이 시작하지 않음) — 합칠 것도 버릴 것도 없어 합치기 기다림에 세지 않는다(5-3b ④)
            apNote("작업이 시작하지 않아 스토리 가지가 없습니다. 합칠 것도 버릴 것도 없습니다.")
        } else if !m.reached(source, nil), mine.isEmpty {
            // 보드 밖에서 지웠거나 같은 이름으로 다시 만든 가지만 남음 — 엔진 discard 가 버린 것으로 적어 합치기 기다림을 닫는다(5-3b ④)
            HStack(spacing: 8) {
                APButton(title: "버린 것으로 적기", icon: "checkmark.circle", enabled: m.discardLock(source) == nil,
                         help: m.discardLock(source) ?? "엔진 finish discard: 지울 가지가 없으면 버린 것으로만 적습니다(가지는 건드리지 않음)", axid: "ap-merge-gone-\(s.key)") { m.discard(s.key) }
                apNote("이 실행이 만든 이 스토리의 가지가 남아 있지 않습니다(보드 밖에서 지웠거나 다시 만들어짐). 버린 것으로 적으면 합치기 기다림이 닫힙니다.")
            }
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
