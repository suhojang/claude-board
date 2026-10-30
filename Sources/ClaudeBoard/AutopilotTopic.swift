import AppKit
import AutopilotRules
import BoardRules
import Combine
import SwiftUI

// MARK: - 자율주행 · 주제로 시작

// 2단계 5-2b(2026-10-30): 출발 준비 1단계의 "주제로 시작" 입력 판(주제 글·자료 링크·작업 경로(기존 저장소·새 폴더)·기술 스택 세 칸)과
// 그 준비(2단계 줄: 링크 읽기·버전 목록·스택 감지·새 폴더 만들기, 뼈대·초안 세션·시나리오 커밋은 5단계 ⑪-2 에서 Autopilot.swift 의 준비 줄로), 그리고 5-2b 규칙(AutopilotRules)의 진짜 어댑터.
// 판정은 규칙이 하고 이 파일은 화면·파일·git·HTTP 만 한다(설계 2장 "주제로 시작"·"기술 스택", 와이어프레임 5판 출발 준비 ⑤~⑨·⑭).
// 주제로 시작의 준비는 보드 메모리와 준비 폴더(drafts/<실행 id>/)에만 있고, 세트(spec.md·stack.json·autopilot.md)는 출발할 때 쓴다(5단계 ⑪-2)

// MARK: 진짜 어댑터

struct APSystemClock: APClock { var now: Date { Date() } }

// 폴더 상태 — 링크를 따라가 폴더면 폴더로 본다(위치가 링크인 ~/workspace 등). 새 자리가 링크면 무엇이 든 폴더로 보아 만들지 않는다
struct APFolderFiles: APFolders {
    func state(_ path: String) -> APPathState {
        var st = stat()
        guard lstat(path, &st) == 0 else { return .missing }
        if (st.st_mode & S_IFMT) == S_IFLNK {
            var t = stat()
            return stat(path, &t) == 0 && (t.st_mode & S_IFMT) == S_IFDIR ? .dir : .other
        }
        guard (st.st_mode & S_IFMT) == S_IFDIR else { return .other }
        let names = (try? FileManager.default.contentsOfDirectory(atPath: path)) ?? []
        return names.allSatisfy { $0 == ".DS_Store" } ? .emptyDir : .dir
    }
    func makeDir(_ path: String) -> Bool { mkdir(path, 0o755) == 0 }
}

// 보드가 부르는 git(apGit 과 같은 보호: 저장소·전역·시스템 설정의 hooks·fsmonitor·attributes·replace 를 쓰지 않음). 실패하면 오류 글도 함께
struct APBoardGit: APGitRunner {
    func run(_ args: [String], in dir: String) -> (code: Int32, out: String) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        p.arguments = ["--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.attributesFile=/dev/null"] + args
        p.currentDirectoryURL = URL(fileURLWithPath: dir)
        var env = ProcessInfo.processInfo.environment
        for (k, v) in [("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")] { env[k] = v }
        p.environment = env
        let out = Pipe(), err = Pipe()
        p.standardOutput = out
        p.standardError = err
        p.standardInput = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return (-1, "git 을 띄우지 못했습니다") }
        let d = out.fileHandleForReading.readDataToEndOfFile(), e = err.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let o = String(decoding: d, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return (p.terminationStatus, p.terminationStatus == 0 ? o : apShort(String(decoding: e, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines), 300))
    }
    // 사용자 전역 git 설정의 이름·메일 — 설정 글을 읽기만 한다(명령·hook 이 돌지 않는 git config --get)
    func identity() -> (name: String, email: String)? {
        func get(_ k: String) -> String? {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
            p.arguments = ["config", "--global", "--get", k]
            p.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
            let out = Pipe()
            p.standardOutput = out
            p.standardError = FileHandle.nullDevice
            guard (try? p.run()) != nil else { return nil }
            let d = out.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            let s = String(decoding: d, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            return p.terminationStatus == 0 && !s.isEmpty ? s : nil
        }
        guard let n = get("user.name"), let e = get("user.email") else { return nil }
        return (n, e)
    }
}

// 저장소의 빌드·설정 파일 — 규칙이 고른 상대 경로만, 링크는 읽지 않고(저장소 밖을 가리킬 수 있어서) 앞 256KB 까지
struct APRepoFiles: APStackFiles {
    func read(_ root: String, _ relative: String) -> String? {
        let p = root + "/" + relative
        var st = stat()
        guard !relative.contains(".."), lstat(p, &st) == 0, (st.st_mode & S_IFMT) == S_IFREG, let h = FileHandle(forReadingAtPath: p) else { return nil }
        defer { try? h.close() }
        let d = (try? h.read(upToCount: 256 * 1024)) ?? Data()
        return String(data: d, encoding: .utf8)
    }
}

// 공식 버전 목록 받기 — 고정한 공개 주소(APFeed)만 GET, 쿠키·캐시 없는 세션, 10초, 200 이고 4MB 아래일 때만. 사용자 자료를 보내지 않는다
struct APHTTPFeeds: APVersionFeeds {
    private static let session: URLSession = {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 10
        c.timeoutIntervalForResource = 15
        c.httpCookieAcceptPolicy = .never
        c.httpShouldSetCookies = false
        c.urlCache = nil
        return URLSession(configuration: c)
    }()
    func fetch(_ feed: APFeed) -> Data? {
        guard let url = URL(string: feed.url) else { return nil }
        var req = URLRequest(url: url)
        req.httpMethod = "GET"
        req.setValue(feed.accept, forHTTPHeaderField: "Accept")
        final class Box: @unchecked Sendable { var data: Data? }
        let done = DispatchSemaphore(value: 0), box = Box()
        APHTTPFeeds.session.dataTask(with: req) { d, r, _ in
            if (r as? HTTPURLResponse)?.statusCode == 200, let d, d.count < 4 * 1024 * 1024 { box.data = d }
            done.signal()
        }.resume()
        _ = done.wait(timeout: .now() + 20)
        return box.data
    }
}

// 버전 목록 하루 캐시 — 자율주행 뿌리 cache/stack-versions.json(보드만 쓴다)
struct APCatalogCacheFile: APVersionCache {
    var path: String { autopilotRoot + "/cache/stack-versions.json" }
    func read() -> (data: Data, at: Date)? {
        guard let d = FileManager.default.contents(atPath: path), let m = mtime(path) else { return nil }
        return (d, m)
    }
    func write(_ data: Data) {
        try? FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        let tmp = path + ".tmp-\(getpid())"
        if FileManager.default.createFile(atPath: tmp, contents: data), rename(tmp, path) == 0 { return }
        unlink(tmp)
    }
}

// 끝난 실행의 실행 폴더와 지운 기록(자율주행 뿌리 purged/<실행 id>.json, 보드만 쓴다)
struct APRawRunFiles: APRawFiles {
    func isRealDir(_ path: String) -> Bool {
        var st = stat()
        return lstat(path, &st) == 0 && (st.st_mode & S_IFMT) == S_IFDIR
    }
    func usage(_ path: String) -> (files: Int, bytes: Int64) {
        var n = 0, b: Int64 = 0
        let e = FileManager.default.enumerator(at: URL(fileURLWithPath: path), includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey, .isSymbolicLinkKey])
        while let u = e?.nextObject() as? URL {
            guard let v = try? u.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey, .isSymbolicLinkKey]), v.isSymbolicLink != true, v.isRegularFile == true else { continue }
            n += 1
            b += Int64(v.fileSize ?? 0)
        }
        return (n, b)
    }
    func remove(_ path: String) -> String? {
        do { try FileManager.default.removeItem(atPath: path); return nil } catch { return error.localizedDescription }
    }
    func writeRecord(_ run: String, _ data: Data) -> Bool {
        let dir = autopilotRoot + "/purged"
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let path = dir + "/\(run).json", tmp = path + ".tmp-\(getpid())"
        guard FileManager.default.createFile(atPath: tmp, contents: data + Data("\n".utf8)), rename(tmp, path) == 0 else { unlink(tmp); return false }
        return true
    }
}

// 지운 기록 읽기 — 지난 실행 화면이 "원문 지움(날짜)"을 보인다
func apPurgeRecords() -> [String: APPurgeRecord] {
    let dir = autopilotRoot + "/purged"
    var out: [String: APPurgeRecord] = [:]
    for n in (try? FileManager.default.contentsOfDirectory(atPath: dir)) ?? [] where n.hasSuffix(".json") {
        if let d = FileManager.default.contents(atPath: dir + "/" + n), let r = try? JSONDecoder().decode(APPurgeRecord.self, from: d) { out[r.run] = r }
    }
    return out
}

// MARK: 주제로 시작의 준비 상태

// 기술 스택 칸 하나의 고른 것 — 없음·목록·직접 입력. 판은 칸(java·spring-boot·kotlin·node·nest·python·fastapi·react·vite·next·vue·postgresql·mysql·build)마다
struct APSlotPick: Equatable {
    var mode = "none"            // none·list·custom
    var key = ""                 // 목록 항목 키
    var vers: [String: String] = [:]
    var name = "", version = ""  // 직접 입력
    var origin = "picked"        // topic·picked·custom
}

// 목록에 있는 칸마다의 항목과 판 칸
let apSlotChoices: [String: [(key: String, label: String)]] = [
    "back": [("spring-java", "Java · Spring Boot"), ("spring-kotlin", "Kotlin · Spring Boot"), ("nest", "TypeScript · NestJS"), ("fastapi", "Python · FastAPI")],
    "front": [("react", "React (Vite)"), ("next", "Next.js"), ("vue", "Vue")],
    "db": [("postgresql", "PostgreSQL"), ("mysql", "MySQL")],
]
let apSlotFields: [String: [(field: String, label: String)]] = [
    "spring-java": [("java", "Java"), ("spring-boot", "Spring Boot"), ("build", "빌드 도구")],
    "spring-kotlin": [("kotlin", "Kotlin"), ("java", "Java"), ("spring-boot", "Spring Boot"), ("build", "빌드 도구")],
    "nest": [("nest", "NestJS"), ("node", "Node.js")],
    "fastapi": [("python", "Python"), ("fastapi", "FastAPI")],
    "react": [("react", "React"), ("vite", "Vite"), ("node", "Node.js")],
    "next": [("next", "Next.js"), ("node", "Node.js")],
    "vue": [("vue", "Vue"), ("vite", "Vite"), ("node", "Node.js")],
    "postgresql": [("postgresql", "판")],
    "mysql": [("mysql", "판")],
]
let apBuildTools = ["Gradle Groovy", "Gradle Kotlin", "Maven"]
func apSlotTitle(_ slot: String) -> String { ["front": "프론트", "back": "백엔드", "db": "DB"][slot] ?? slot }

// 기존 저장소 읽은 것 — git 뿌리·지금 가지·가지 목록, 기록 저장소의 프로젝트, 세트 지시
struct APTopicRepo: Equatable {
    var input = ""
    var top: String?
    var branch: String?
    var branches: [String] = []
    var error: String?
    var project: String?
    var known = false            // 기록 저장소에 이 뿌리의 프로젝트가 있음
    var setAt: Double?
    var pastRuns = 0
}

// 사용자 MCP 설정의 서버 하나(엔진 read servers 가 읽음, 값은 이름·범위·띄울 것·승인 여부·허용 꼴만 — 설정의 env·머리는 보드로 오지 않는다)
struct APMCPServerInfo: Equatable, Identifiable {
    var id: String { name }
    var name = "", scope = "", transport = "", target = "", note = "", allow = ""
    var approved = true
}

@MainActor final class APTopic: ObservableObject {
    @Published var text = "" { didSet { if text != oldValue { textChanged() } } }
    @Published private(set) var links: [APLink] = []
    // 4단계 ⑩-2 — 그 밖 웹 주소를 읽을 MCP 서버(주소 → 서버 이름)와 사용자 MCP 설정의 서버 목록
    @Published var linkMCP: [String: String] = [:]
    @Published private(set) var servers: [APMCPServerInfo] = []
    @Published private(set) var serversNote: String?
    @Published private(set) var serversLoading = false
    @Published var linkInput = ""
    @Published private(set) var linkError: String?
    @Published var pathMode = "existing" { didSet { if pathMode != oldValue { modeChanged() } } }
    @Published var existingPath = "" { didSet { if existingPath != oldValue { pathChanged() } } }
    @Published var newParent = NSHomeDirectory() + "/workspace" { didSet { if newParent != oldValue { checkNew() } } }
    @Published var newName = "" { didSet { if newName != oldValue { checkNew() } } }
    @Published private(set) var repo: APTopicRepo?
    @Published private(set) var reading = false
    @Published private(set) var newProblems: [String] = []
    @Published private(set) var newPlan: APNewRepoPlan?
    @Published private(set) var detected: APStackSet?
    @Published private(set) var conflicts: [String] = []
    @Published private(set) var hits: [APTopicHit] = []
    @Published var slots: [String: APSlotPick] = ["front": APSlotPick(), "back": APSlotPick(), "db": APSlotPick()]
    @Published private(set) var touched: Set<String> = []      // 사람이 손댄 칸(주제 글이 바뀌어도 다시 채우지 않는다)
    @Published private(set) var catalog: APCatalogLoad?
    @Published private(set) var catalogLoading = false
    @Published private(set) var created: APCreateResult?
    private var createdPath: String?
    private var catalogWaiters: [@MainActor () -> Void] = []
    private var pathWork: DispatchWorkItem?
    private let queue = DispatchQueue(label: "claude-board.autopilot.topic")

    func reset() {
        linkMCP = [:]
        text = ""; links = []; linkInput = ""; linkError = nil; pathMode = "existing"; existingPath = ""; newParent = NSHomeDirectory() + "/workspace"; newName = ""
        repo = nil; reading = false; newProblems = []; newPlan = nil; detected = nil; conflicts = []; hits = []
        slots = ["front": APSlotPick(), "back": APSlotPick(), "db": APSlotPick()]; touched = []; created = nil; createdPath = nil
        checkNew()
    }

    // 작업 경로가 정해졌나(다음 단계로 갈 수 있나)
    var pathReady: Bool { pathMode == "new" ? newPlan != nil : (repo?.top != nil && !reading) }
    var root: String { pathMode == "new" ? (newPlan?.path ?? "") : (repo?.top ?? "") }
    var projectName: String? {
        if pathMode == "new" { return newPlan.map { ($0.path as NSString).lastPathComponent } }
        return repo?.project
    }
    // 기존 저장소에서 감지한 것이 있으면 잠근다(감지에 실패했을 때만 고른다, 설계 2장)
    var locked: Bool { pathMode == "existing" && !(detected?.isEmpty ?? true) }
    var ready: Bool { !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && pathReady }
    var blocker: String? {
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "주제 글을 넣어 주세요" }
        if pathMode == "new" { return newProblems.first ?? (newPlan == nil ? "새 폴더 이름을 넣어 주세요" : nil) }
        if reading { return "작업 경로를 읽는 중입니다" }
        if repo == nil || existingPath.isEmpty { return "작업 경로(기존 저장소)를 골라 주세요" }
        return repo?.error
    }

    // MARK: 자료 링크
    func addLink() {
        let l = apLinkInfo(linkInput)
        if let p = apLinkAddProblem(l, to: links) { linkError = p; return }
        links.append(l)
        linkInput = ""
        linkError = nil
        if linkView(l).kind == "웹", servers.isEmpty, !serversLoading { loadServers() }
    }
    func removeLink(_ url: String) { links.removeAll { $0.url == url }; linkMCP[url] = nil }
    // 줄의 종류·읽기 상태(연결 목록·키체인 있음·고른 MCP 서버로 매김, 규칙 apLinkState) — 시험 빌드의 가짜 서버 사이트(127.0.0.1)도 Atlassian 으로 본다
    func linkView(_ l: APLink) -> APLink {
        let c = APConnections.shared
        c.loadOnce()
        return apLinkState(l.url, connections: c.list, secrets: c.secrets, mcp: linkMCP[l.url], dev: apDevBuild, now: Date(), tz: .current)
    }
    // 시험 빌드만: 장난감 MCP 설정(설정 값 autopilotMCPFrom)과 가짜 claude(autopilotReadClaude) — 엔진도 --dev 일 때만 받는다
    static var devMCPArgs: [String] {
        guard apDevBuild else { return [] }
        var a: [String] = []
        if let f = UserDefaults.standard.string(forKey: "autopilotMCPFrom"), !f.isEmpty { a += ["--mcp-from", f] }
        return a
    }
    static var devClaudeArgs: [String] {
        guard apDevBuild, let c = UserDefaults.standard.string(forKey: "autopilotReadClaude"), !c.isEmpty else { return [] }
        return ["--claude", c]
    }
    // 사용자 MCP 설정의 서버 — 앱 안 엔진(read servers)이 읽기만 한다. 저장소 .mcp.json 서버는 그 프로젝트에서 승인한 것만 고를 수 있다
    func loadServers() {
        serversLoading = true
        let cwd = root
        let args = ["read", "servers"] + (cwd.isEmpty ? [] : ["--cwd", cwd]) + (apDevBuild ? ["--dev"] : []) + APTopic.devMCPArgs
        DispatchQueue.global(qos: .userInitiated).async {
            let r = apEngine(args)
            let list = ((try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [[String: Any]] ?? []).map { d in
                APMCPServerInfo(name: apDocText(d["name"] as? String ?? "", 64), scope: d["scope"] as? String ?? "", transport: d["transport"] as? String ?? "",
                                target: apDocText(d["target"] as? String ?? "", 200), note: apDocText(d["note"] as? String ?? "", 300),
                                allow: apDocText(d["allow"] as? String ?? "", 400), approved: d["approved"] as? Bool ?? false)
            }.filter { apMCPServerNameOK($0.name) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.serversLoading = false
                    self.servers = list
                    self.serversNote = r.code == 0 ? (list.isEmpty ? "사용자 MCP 설정에 서버가 없습니다" : nil) : apDocText(r.out, 200)
                }
            }
        }
    }

    // MARK: 주제 글 → 낱말 대조·경고·칸 미리 채우기
    private func textChanged() {
        hits = apTopicHits(text)
        if let d = detected, pathMode == "existing" { conflicts = apStackConflicts(detected: d, topic: text) } else { conflicts = [] }
        prefill()
    }
    // 새 폴더(또는 감지에 실패한 기존 저장소)면 주제에서 읽은 칸을 채운다 — 사람이 손댄 칸은 그대로. 판은 글에 있으면 그것, 없으면 추천
    func prefill() {
        guard !locked else { return }
        let t = apTopicStack(text)
        for (slot, x) in [("front", t.front), ("back", t.back), ("db", t.db)] where !touched.contains(slot) {
            guard let s = x else { if slots[slot]?.origin == "topic" { slots[slot] = APSlotPick() }; continue }
            var p = APSlotPick(mode: "list", key: s.key, origin: "topic")
            for (f, _) in apSlotFields[s.key] ?? [] {
                if f == "build" { p.vers[f] = s.build.isEmpty ? apBuildTools[0] : s.build; continue }
                p.vers[f] = catalog?.catalog.recommended(f) ?? ""
            }
            // 글에 적힌 판(프레임워크·언어)
            let fw = ["spring-java": "spring-boot", "spring-kotlin": "spring-boot", "nest": "nest", "fastapi": "fastapi", "react": "react", "next": "next", "vue": "vue",
                      "postgresql": "postgresql", "mysql": "mysql"][s.key] ?? ""
            if !s.frameworkVersion.isEmpty, !fw.isEmpty { p.vers[fw] = s.frameworkVersion }
            let lf = ["Java": "java", "Kotlin": "kotlin", "Python": "python"][s.lang] ?? ""
            if !s.langVersion.isEmpty, !lf.isEmpty { p.vers[lf] = s.langVersion }
            slots[slot] = p
        }
    }
    func pick(_ slot: String, _ p: APSlotPick) {
        touched.insert(slot)
        var q = p
        if q.origin == "topic" { q.origin = "picked" }
        slots[slot] = q
    }
    func choose(_ slot: String, mode: String, key: String = "") {
        var p = APSlotPick(mode: mode, key: key, origin: mode == "custom" ? "custom" : "picked")
        if mode == "list" {
            for (f, _) in apSlotFields[key] ?? [] { p.vers[f] = f == "build" ? apBuildTools[0] : (catalog?.catalog.recommended(f) ?? "") }
        }
        if mode == "custom" { p.name = slots[slot]?.name ?? ""; p.version = slots[slot]?.version ?? "" }
        pick(slot, p)
    }
    func setVersion(_ slot: String, _ field: String, _ v: String) {
        guard var p = slots[slot] else { return }
        p.vers[field] = v
        pick(slot, p)
    }

    // 고른 스택 한 줄(출발 확인의 "기술 스택" 줄·세트 stack.json 으로 갈 것)
    func summary() -> String {
        func one(_ slot: String) -> String {
            if locked, let d = detected {
                let s = slot == "front" ? d.front : slot == "back" ? d.back : d.db
                guard let s else { return "\(apSlotTitle(slot)) 없음" }
                return [s.lang + (s.langVersion.isEmpty ? "" : " " + s.langVersion), s.framework + (s.frameworkVersion.isEmpty ? "" : " " + s.frameworkVersion), s.build]
                    .filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.joined(separator: " · ")
            }
            guard let p = slots[slot] else { return "" }
            switch p.mode {
            case "list":
                let label = apSlotChoices[slot]?.first { $0.key == p.key }?.label ?? p.key
                let v = (apSlotFields[p.key] ?? []).compactMap { f in
                    p.vers[f.field].flatMap { $0.isEmpty ? nil : (f.field == "build" ? $0 : "\(f.label == "판" ? label : f.label) \($0)") }
                }
                return v.isEmpty ? label : v.joined(separator: " · ")
            case "custom": return p.name.isEmpty ? "\(apSlotTitle(slot)) 직접 입력(이름 없음)" : p.name + (p.version.isEmpty ? "" : " " + p.version)
            default: return "\(apSlotTitle(slot)) 없음"
            }
        }
        return ["back", "front", "db"].map(one).filter { !$0.isEmpty }.joined(separator: " / ")
    }

    // 고른 스택 → 세트 stack.json 의 꼴(규칙 APStackSet, 5단계 ⑪-2). 기존 저장소에서 감지해 잠갔으면 감지한 것 그대로
    func stackSet() -> APStackSet {
        if locked, let d = detected { return d }
        func one(_ slot: String) -> APStackSlot? {
            guard let p = slots[slot] else { return nil }
            switch p.mode {
            case "list":
                let label = apSlotChoices[slot]?.first { $0.key == p.key }?.label ?? p.key
                let v = { (k: String) in p.vers[k] ?? "" }
                var s = APStackSlot(key: p.key, name: label, origin: p.origin == "topic" ? "topic" : "picked")
                switch p.key {
                case "spring-java": s.lang = "Java"; s.langVersion = v("java"); s.framework = "Spring Boot"; s.frameworkVersion = v("spring-boot"); s.build = v("build").isEmpty ? "Gradle Groovy" : v("build")
                case "spring-kotlin": s.lang = "Kotlin"; s.langVersion = v("kotlin"); s.framework = "Spring Boot"; s.frameworkVersion = v("spring-boot"); s.build = v("build").isEmpty ? "Gradle Groovy" : v("build")
                case "nest": s.lang = "Node.js"; s.langVersion = v("node"); s.framework = "NestJS"; s.frameworkVersion = v("nest"); s.build = "npm"
                case "fastapi": s.lang = "Python"; s.langVersion = v("python"); s.framework = "FastAPI"; s.frameworkVersion = v("fastapi")
                case "react": s.lang = "Node.js"; s.langVersion = v("node"); s.framework = "React"; s.frameworkVersion = v("react"); s.build = v("vite").isEmpty ? "Vite" : "Vite " + v("vite")
                case "next": s.lang = "Node.js"; s.langVersion = v("node"); s.framework = "Next.js"; s.frameworkVersion = v("next"); s.build = "npm"
                case "vue": s.lang = "Node.js"; s.langVersion = v("node"); s.framework = "Vue"; s.frameworkVersion = v("vue"); s.build = v("vite").isEmpty ? "Vite" : "Vite " + v("vite")
                case "postgresql": s.framework = "PostgreSQL"; s.frameworkVersion = v("postgresql")
                case "mysql": s.framework = "MySQL"; s.frameworkVersion = v("mysql")
                default: break
                }
                return s
            case "custom":
                return p.name.isEmpty ? nil : APStackSlot(key: "", name: p.name, frameworkVersion: p.version, origin: "custom")
            default: return nil
            }
        }
        return APStackSet(front: one("front"), back: one("back"), db: one("db"))
    }

    // MARK: 작업 경로
    private func modeChanged() {
        if pathMode == "new" { checkNew(); detected = nil; conflicts = []; touched = []; prefill(); loadCatalog() }
        else { pathChanged() }
    }
    func checkNew() {
        let fs = APFolderFiles()
        let par = newParent.hasPrefix("~") ? NSHomeDirectory() + newParent.dropFirst() : newParent
        let parent = par.trimmingCharacters(in: .whitespaces)
        let target = (parent.hasSuffix("/") ? String(parent.dropLast()) : parent) + "/" + newName.trimmingCharacters(in: .whitespaces)
        // 이 준비가 이미 만든 폴더는 빈 자리로 본다(준비를 다시 볼 때 "이미 무엇이 든 폴더"로 막히지 않게)
        let mine = created?.ok == true && createdPath == target
        let c = apNewRepoCheck(parent: parent, name: newName, home: NSHomeDirectory(), parentState: fs.state(parent),
                               targetState: newName.isEmpty || mine ? .missing : fs.state(target))
        newPlan = c.plan
        newProblems = newName.isEmpty ? [] : c.problems
    }
    // 기존 저장소 경로 — 글자를 바꾸면 잠깐 기다렸다가(0.4초) 뒤에서 git 뿌리·가지·기록 저장소의 프로젝트·세트와 기술 스택을 읽는다
    private func pathChanged() {
        pathWork?.cancel()
        let input = existingPath.trimmingCharacters(in: .whitespaces)
        guard pathMode == "existing", !input.isEmpty else { repo = nil; detected = nil; conflicts = []; reading = false; return }
        reading = true
        let w = DispatchWorkItem { [weak self] in
            let r = APTopic.readRepo(input)
            let det = r.top.map { apDetectStack(root: $0, files: APRepoFiles()) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, self.existingPath.trimmingCharacters(in: .whitespaces) == input, self.pathMode == "existing" else { return }
                    self.repo = r
                    self.detected = det
                    self.reading = false
                    self.conflicts = det.map { apStackConflicts(detected: $0, topic: self.text) } ?? []
                    if det?.isEmpty ?? false { self.prefill(); self.loadCatalog() }
                }
            }
        }
        pathWork = w
        queue.asyncAfter(deadline: .now() + 0.4, execute: w)
    }
    nonisolated static func readRepo(_ input: String) -> APTopicRepo {
        var r = APTopicRepo(input: input)
        let p = input.hasPrefix("~") ? NSHomeDirectory() + input.dropFirst() : input
        var isDir: ObjCBool = false
        guard p.hasPrefix("/"), FileManager.default.fileExists(atPath: p, isDirectory: &isDir), isDir.boolValue else {
            r.error = "그 경로에 폴더가 없습니다"; return r
        }
        guard let top = apGit(["rev-parse", "--show-toplevel"], p), !top.isEmpty else {
            r.error = "git 저장소가 아닙니다. 저장소를 고르거나 '새 폴더 만들기'로 만듭니다"; return r
        }
        r.top = top
        r.branch = apGit(["symbolic-ref", "-q", "--short", "HEAD"], top).flatMap { $0.isEmpty ? nil : $0 }
        r.branches = (apGit(["for-each-ref", "--format=%(refname:short)", "refs/heads/"], top) ?? "").split(separator: "\n").map(String.init)
        let known = APRecords.project(autopilotRoot + "/private/records.sqlite", root: top)
        r.known = known != nil
        r.project = known?.id ?? String((top as NSString).lastPathComponent.map { $0.isASCII && ($0.isLetter || $0.isNumber || "._-".contains($0)) ? $0 : "-" }).prefix(40).description
        r.pastRuns = known?.runs ?? 0
        if let pid = r.project { r.setAt = mtime(autopilotRoot + "/private/projects/\(pid)/set/autopilot.md")?.timeIntervalSince1970 }
        return r
    }
    // 최근 경로 — 보드 경로 카드와 지난 실행의 작업 경로(홈은 뺌)
    func recent() -> [String] {
        var out: [String] = []
        let groups = (NSApp.delegate as? AppDelegate)?.store.groups.map(\.id) ?? []
        for p in groups + APStart.shared.past.compactMap(\.root) where p != NSHomeDirectory() && !out.contains(p) { out.append(p) }
        return Array(out.prefix(6))
    }
    func browse(new: Bool) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = new
        panel.prompt = "고르기"
        panel.directoryURL = URL(fileURLWithPath: new ? newParent : (existingPath.isEmpty ? NSHomeDirectory() + "/workspace" : existingPath))
        guard panel.runModal() == .OK, let u = panel.url else { return }
        if new { newParent = u.path } else { existingPath = u.path }
    }

    // MARK: 버전 목록
    func loadCatalog() {
        guard !catalogLoading, catalog == nil || catalog?.origin == .stale || catalog?.origin == .builtin else { return }
        catalogLoading = true
        // 시험 빌드만: 받기를 끄는 설정(오래된 목록 화면을 네트워크 없이 보려고)
        let offline = apDevBuild && UserDefaults.standard.bool(forKey: "autopilotVersionsOffline")
        queue.async {
            let l = apLoadCatalog(feeds: offline ? nil : APHTTPFeeds(), cache: APCatalogCacheFile(), clock: APSystemClock(), builtin: apBuiltinCatalogData)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.catalog = l
                    self.catalogLoading = false
                    self.prefill()
                    let w = self.catalogWaiters
                    self.catalogWaiters = []
                    w.forEach { $0() }
                    // 목록이 와서 빈 판 칸이 있으면 추천으로 채운다(사람이 고른 판은 그대로)
                    for (slot, p) in self.slots where p.mode == "list" {
                        var q = p
                        for (f, _) in apSlotFields[p.key] ?? [] where (q.vers[f] ?? "").isEmpty && f != "build" { q.vers[f] = l?.catalog.recommended(f) ?? "" }
                        if q != p { self.slots[slot] = q }
                    }
                }
            }
        }
    }
    var catalogLabel: String {
        guard let c = catalog else { return catalogLoading ? "버전 목록을 받는 중" : "버전 목록 아직 없음" }
        let d = Date(timeIntervalSince1970: c.catalog.at)
        let day = Calendar.current.isDateInToday(d) ? "오늘 " + clock(d) : apDay(c.catalog.at)
        return apCatalogLabel(c, date: c.origin == .fresh || c.origin == .cached ? day : apDay(c.catalog.at))
    }

    // MARK: 새 폴더 만들기(2단계 준비 줄)
    func createRepo(_ done: @escaping @MainActor (APCreateResult) -> Void) {
        checkNew()
        guard let plan = newPlan else {
            var r = APCreateResult(); r.error = newProblems.first ?? "새 폴더 계획이 없습니다"
            done(r); return
        }
        // 이 준비가 이미 만든 폴더면 다시 만들지 않는다
        if let c = created, c.ok, createdPath == plan.path { var r = c; r.done = ["이 준비가 이미 만든 폴더입니다"] + c.done; done(r); return }
        queue.async {
            let r = apCreateRepo(plan, folders: APFolderFiles(), git: APBoardGit())
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.created = r
                    if r.ok || r.made { self.createdPath = plan.path }
                    if !r.ok { self.createdPath = nil }
                    done(r)
                }
            }
        }
    }
    // 버전 목록이 오면 부른다(이미 있으면 바로)
    func whenCatalog(_ f: @escaping @MainActor () -> Void) {
        if catalog != nil, !catalogLoading { f(); return }
        catalogWaiters.append(f)
        loadCatalog()
    }
}

// MARK: 화면 — 1단계 주제로 시작 입력 판

struct APTopicStep1: View {
    @ObservedObject var s: APStart
    @ObservedObject var t: APTopic
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            topicBox
            linksBox
            pathBox
            stackBox
        }
    }

    // 주제 — 자유 입력 한 칸(와이어프레임 ⑤). 찾은 낱말은 아래에 꼬리표로
    private var topicBox: some View {
        APBox(title: "주제", note: "자유 입력 한 칸 · 주제는 결정이 아니라 요청입니다", axid: "ap-topic-box") {
            ZStack(alignment: .topLeading) {
                TextEditor(text: $t.text)
                    .font(.system(size: 12))
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 64, maxHeight: 140)
                    .padding(4)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
                    .accessibilityIdentifier("ap-topic-text")
                if t.text.isEmpty {
                    Text("예: 회의실 예약 API 를 Spring Boot 로 만든다. 예약·취소·겹침 막기, 관리자 승인.").boardFont(11.5).foregroundStyle(.secondary.opacity(0.7))
                        .padding(.horizontal, 9).padding(.vertical, 8).allowsHitTesting(false)
                }
            }
            if !t.hits.isEmpty {
                WrapRow(spacing: 5) {
                    apNote("찾은 낱말")
                    ForEach(Array(t.hits.enumerated()), id: \.offset) { _, h in
                        APChip(text: h.word + (h.version.isEmpty ? "" : " " + h.version), color: Palette.accent, strong: true)
                    }
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("ap-topic-hits")
            }
            apNote("보드가 주제 글에서 기술 스택 이름과 버전 꼴을 낱말 대조로 찾습니다(모델을 부르지 않습니다). 명세·단위는 다음 단계의 초안 세션이 씁니다.")
        }
    }

    // 자료 링크 — 줄마다 종류·주소·읽기 상태(연결됨·연결 필요·읽을 수 없음, 그 밖 웹 주소는 MCP 서버 고르기). 읽기는 2단계 "보드가 준비"에서 줄마다(와이어프레임 ⑥, 4단계 ⑩-2)
    private var linksBox: some View {
        APBox(title: "자료 링크", note: "메타 정보만 읽습니다", axid: "ap-links-box") {
            ForEach(t.links) { l0 in
                let l = t.linkView(l0)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        APChip(text: l.kind, color: APColor.board)
                        Text(l.url).boardFont(10.5, design: .monospaced).lineLimit(1).truncationMode(.middle)
                        Spacer(minLength: 4)
                        switch l.state {
                        case "connected": APChip(text: "연결됨", color: APColor.ok, strong: true)
                        case "mcp": APChip(text: "MCP 읽기", color: APColor.ok, strong: true)
                        case "unreadable": APChip(text: "읽을 수 없음", color: APColor.fail, strong: true)
                        default: APChip(text: "연결 필요", color: APColor.limit, strong: true)
                        }
                        if l.kind == "웹", l.state != "unreadable" { mcpMenu(l0.url) }
                        else if l.state == "needs_connection" {
                            APButton(title: "설정 › 연결", help: "연결 설정을 엽니다(준비는 남아 있습니다)", axid: "ap-link-settings") {
                                APStart.shared.close(); SettingsScreen.shared.open(.connections)
                            }
                        }
                        APButton(title: "빼기", help: "이 링크를 뺍니다", axid: "ap-link-remove") { t.removeLink(l.url) }
                    }
                    if l.state != "connected" { apNote(l.why, l.state == "unreadable" ? APColor.fail : .secondary) }
                    if let m = t.linkMCP[l0.url], let sv = t.servers.first(where: { $0.name == m }) {
                        apNote("\(sv.name) · \(sv.scope == "project" ? "저장소 .mcp.json" : sv.scope == "local" ? "이 프로젝트(local)" : "사용자(user)") · \(sv.transport) · \(sv.target)")
                        apNote(sv.allow)
                    }
                }
                .hint(l.why)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("ap-link-\(l.kind)")
            }
            HStack(spacing: 6) {
                TextField("＋ 주소 붙여 넣기 (Figma · Confluence · Notion · Jira · 그 밖 웹 주소)", text: $t.linkInput)
                    .textFieldStyle(.roundedBorder).boardFont(11.5).accessibilityIdentifier("ap-link-input")
                    .onSubmit { t.addLink() }
                APButton(title: "더하기", enabled: !t.linkInput.trimmingCharacters(in: .whitespaces).isEmpty, axid: "ap-link-add") { t.addLink() }
            }
            if let e = t.linkError { apNote(e, APColor.fail) }
            apNote("연결이 없는 서비스의 링크에는 '연결 필요', 열리지 않는 주소에는 '읽을 수 없음'이 붙습니다. 링크는 다음 단계(보드가 준비)에서 줄마다 읽고, 본문 전체가 아니라 메타 정보(제목·목차·핵심 칸·링크·요약, 가리킨 절)만 근거 묶음에 남깁니다. 연결이 없는 링크는 요청하지 않습니다.")
        }
    }

    // 그 밖 웹 주소를 읽을 MCP 서버 고르기 — 사용자 MCP 설정의 서버 이름만(저장소 .mcp.json 서버는 그 프로젝트에서 승인한 것만)
    private func mcpMenu(_ url: String) -> some View {
        Menu {
            if t.servers.isEmpty { Text(t.serversLoading ? "읽는 중…" : (t.serversNote ?? "서버 없음")) }
            ForEach(t.servers) { sv in
                Button(sv.approved ? "\(sv.name) (\(sv.scope))" : "\(sv.name) (승인 안 됨)") { t.linkMCP[url] = sv.name }
                    .disabled(!sv.approved)
                    .help(sv.approved ? sv.allow : sv.note)
            }
            if t.linkMCP[url] != nil { Divider(); Button("고르지 않음") { t.linkMCP[url] = nil } }
            Divider()
            Button("서버 목록 다시 읽기") { t.loadServers() }
        } label: {
            Text(t.linkMCP[url].map { "MCP: \($0)" } ?? "MCP 서버 고르기").boardFont(11)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityIdentifier("ap-link-mcp")
        .onAppear { if t.servers.isEmpty, !t.serversLoading { t.loadServers() } }
    }

    // 작업 경로 — 기존 저장소 또는 새 폴더 만들기(와이어프레임 ⑦)
    private var pathBox: some View {
        APBox(title: "작업 경로", note: "저장소 뿌리", axid: "ap-topic-path") {
            HStack(spacing: 6) {
                APButton(title: "기존 저장소", tint: t.pathMode == "existing" ? Palette.accent : nil, axid: "ap-path-existing") { t.pathMode = "existing" }
                APButton(title: "새 폴더 만들기", tint: t.pathMode == "new" ? Palette.accent : nil, axid: "ap-path-new") { t.pathMode = "new" }
                Spacer(minLength: 0)
            }
            if t.pathMode == "existing" { existing } else { newFolder }
        }
    }
    @ViewBuilder private var existing: some View {
        HStack(spacing: 6) {
            TextField("저장소 경로(예: ~/workspace/todo-api)", text: $t.existingPath)
                .textFieldStyle(.roundedBorder).boardFont(11.5, design: .monospaced).accessibilityIdentifier("ap-path-input")
            APButton(title: "고르기…", axid: "ap-path-browse") { t.browse(new: false) }
        }
        let rec = t.recent()
        if !rec.isEmpty {
            WrapRow(spacing: 5) {
                apNote("최근:")
                ForEach(rec, id: \.self) { p in
                    APButton(title: (p as NSString).lastPathComponent, help: apHome(p), axid: "ap-path-recent-\((p as NSString).lastPathComponent)") { t.existingPath = p }
                }
            }
        }
        if t.reading { apNote("경로를 읽는 중입니다", APColor.work) }
        else if let r = t.repo {
            if let e = r.error { apNote(e, APColor.fail) }
            else if let top = r.top {
                HStack(spacing: 6) {
                    Text(apHome(top)).boardFont(11, design: .monospaced).lineLimit(1).truncationMode(.head)
                    APChip(text: "git 저장소", color: APColor.ok)
                    if let b = r.branch { APChip(text: "가지 \(b)") }
                    if let p = r.project { APChip(text: p, color: APColor.board) }
                }
                if top != (r.input.hasPrefix("~") ? NSHomeDirectory() + r.input.dropFirst() : r.input) { apNote("저장소 뿌리(\(apHome(top)))를 작업 경로로 씁니다.") }
                apNote(r.known ? "이 경로의 지난 실행 \(r.pastRuns)개가 있습니다." + (r.setAt.map { " 지난 자율주행 세트(\(apDay($0)))를 불러옵니다." } ?? " 세트에 지시가 없습니다.")
                       : "이 경로에서 처음 출발합니다.")
                apNote("단일 저장소인지 묶음인지(따로 작업 트리로 도는지)는 보드가 준비할 때 엔진이 봅니다.")
            }
        }
    }
    @ViewBuilder private var newFolder: some View {
        HStack(spacing: 6) {
            Text("위치").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 34, alignment: .leading)
            TextField("~/workspace", text: $t.newParent).textFieldStyle(.roundedBorder).boardFont(11.5, design: .monospaced).accessibilityIdentifier("ap-new-parent")
            APButton(title: "고르기…", axid: "ap-new-browse") { t.browse(new: true) }
        }
        HStack(spacing: 6) {
            Text("이름").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 34, alignment: .leading)
            TextField("room-booking", text: $t.newName).textFieldStyle(.roundedBorder).boardFont(11.5, design: .monospaced).accessibilityIdentifier("ap-new-name")
        }
        ForEach(t.newProblems, id: \.self) { apNote($0, APColor.fail) }
        if let p = t.newPlan {
            Text(apHome(p.path)).boardFont(11, design: .monospaced).foregroundStyle(.secondary)
            apNote("보드가 폴더를 만들고 git init, 빈 첫 커밋, 그 커밋에 가지 \(p.branches.joined(separator: "·")) 를 만듭니다(처음 걸린 가지 \(p.head), 출발 가지 \(p.base)). 단일 저장소라 따로 작업 트리에서 돕니다.")
            if let c = t.created, c.ok { apNote("이 준비가 만든 폴더입니다(빈 첫 커밋 \(c.commit.map { String($0.prefix(7)) } ?? "?")). 준비를 버려도 지우지 않습니다.", APColor.ok) }
            else { apNote("폴더는 '다음: 보드가 준비'를 누를 때 만듭니다. 준비를 버려도 만든 폴더는 지우지 않습니다.") }
        }
    }

    // 기술 스택 세 칸 — 기존 프로젝트는 감지해 잠금, 새 폴더는 주제에서 읽거나 고르거나 직접 입력(와이어프레임 ⑧⑨)
    private var stackBox: some View {
        APBox(title: "기술 스택", note: "프론트 · 백엔드 · DB", axid: "ap-stack-box") {
            if t.pathMode == "existing" && (t.repo?.top == nil || t.reading) {
                apNote("기존 저장소를 고르면 빌드·설정 파일을 읽어 감지합니다.")
            } else if t.locked, let d = t.detected {
                HStack(spacing: 6) { APChip(text: "감지됨", color: APColor.ok, strong: true); APChip(text: "잠금"); apNote("기존 프로젝트의 규격을 따릅니다. 바꿀 수 없습니다.") }
                detectedRow("front", d.front)
                detectedRow("back", d.back)
                detectedRow("db", d.db)
                ForEach(t.conflicts, id: \.self) { apNote($0, APColor.limit) }
            } else {
                headLine
                ForEach(["front", "back", "db"], id: \.self) { slot in APSlotEditor(t: t, slot: slot) }
                if !t.summary().isEmpty { apNote("고른 스택: " + t.summary()) }
                apNote("고른 스택은 초안 세션과 단위 세션에 넘기고 출발할 때 자율주행 세트 stack.json 에 남깁니다. 목록에 있는 스택은 보드가 공식 생성기로 뼈대를 만들고(바깥 레지스트리를 부르는 생성기도 있습니다), 직접 입력한 스택은 첫 단위 '프로젝트 구성'을 모델이 합니다.")
            }
        }
        .onAppear { if t.pathMode == "new" || (t.detected?.isEmpty ?? false) { t.loadCatalog() } }
    }
    @ViewBuilder private var headLine: some View {
        let fromTopic = t.slots.values.contains { $0.origin == "topic" }
        WrapRow(spacing: 6) {
            if t.pathMode == "existing" { APChip(text: "감지된 것 없음", color: APColor.limit, strong: true) }
            if fromTopic { APChip(text: "주제에서 읽음", color: Palette.accent, strong: true) }
            APChip(text: t.catalogLabel, color: t.catalog?.origin == .stale || t.catalog?.origin == .builtin ? APColor.limit : APColor.muted)
                .accessibilityIdentifier("ap-catalog-label")
        }
        if fromTopic { apNote("주제에 버전이 없으면 추천 판을 넣었습니다. 확인해 주세요.") }
        else if t.pathMode == "existing" { apNote("빌드·설정 파일에서 스택을 찾지 못했습니다. 칸마다 고르거나 직접 적습니다.") }
        else { apNote("주제에 기술 스택이 없습니다. 칸마다 고르거나 직접 적습니다.") }
        if let f = t.catalog?.failed, !f.isEmpty, t.catalog?.origin != .fresh { apNote("받지 못한 곳: " + f.joined(separator: ", "), APColor.limit) }
    }
    private func detectedRow(_ slot: String, _ s: APStackSlot?) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(apSlotTitle(slot)).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 46, alignment: .leading)
            if let s {
                VStack(alignment: .leading, spacing: 1) {
                    Text([s.lang.isEmpty ? "" : s.lang + (s.langVersion.isEmpty ? "" : " " + s.langVersion),
                          s.framework.isEmpty ? s.name : s.framework + (s.frameworkVersion.isEmpty ? "" : " " + s.frameworkVersion), s.build]
                            .filter { !$0.isEmpty }.joined(separator: " · ")).boardFont(11.5, weight: .semibold)
                    Text(s.sources.joined(separator: " · ")).boardFont(10, design: .monospaced).foregroundStyle(.secondary)
                }
            } else {
                Text("없음 (감지된 것 없음)").boardFont(11.5).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("ap-detected-\(slot)")
    }
}

// 칸 하나 — 없음 · 목록에서 고르기 · 직접 입력, 목록이면 항목과 판 칸
struct APSlotEditor: View {
    @ObservedObject var t: APTopic
    let slot: String
    var body: some View {
        let p = t.slots[slot] ?? APSlotPick()
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 6) {
                Text(apSlotTitle(slot)).boardFont(11.5, weight: .bold).frame(width: 46, alignment: .leading)
                APButton(title: "없음", tint: p.mode == "none" ? Palette.accent : nil, axid: "ap-slot-\(slot)-none") { t.choose(slot, mode: "none") }
                APButton(title: "목록에서 고르기", tint: p.mode == "list" ? Palette.accent : nil, axid: "ap-slot-\(slot)-list") {
                    t.choose(slot, mode: "list", key: p.mode == "list" ? p.key : (apSlotChoices[slot]?.first?.key ?? ""))
                }
                APButton(title: "직접 입력", tint: p.mode == "custom" ? Palette.accent : nil, axid: "ap-slot-\(slot)-custom") { t.choose(slot, mode: "custom") }
                if p.origin == "topic" { APChip(text: "주제에서 읽음", color: Palette.accent) }
                Spacer(minLength: 0)
            }
            if p.mode == "list" { list(p) }
            if p.mode == "custom" {
                HStack(spacing: 6) {
                    TextField("이름(예: Go · Gin)", text: Binding(get: { p.name }, set: { var q = p; q.name = $0; t.pick(slot, q) }))
                        .textFieldStyle(.roundedBorder).boardFont(11.5).accessibilityIdentifier("ap-slot-\(slot)-name")
                    TextField("판", text: Binding(get: { p.version }, set: { var q = p; q.version = $0; t.pick(slot, q) }))
                        .textFieldStyle(.roundedBorder).boardFont(11.5).frame(maxWidth: 90).accessibilityIdentifier("ap-slot-\(slot)-version")
                }
                apNote("보드가 생성기를 모르면 첫 단위 '프로젝트 구성'을 모델이 합니다.")
            }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 9).fill(Color.white.opacity(0.03)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-slot-\(slot)")
    }

    @ViewBuilder private func list(_ p: APSlotPick) -> some View {
        WrapRow(spacing: 5) {
            ForEach(apSlotChoices[slot] ?? [], id: \.key) { c in
                APButton(title: c.label, tint: p.key == c.key ? Palette.accent : nil, axid: "ap-slot-\(slot)-\(c.key)") { t.choose(slot, mode: "list", key: c.key) }
            }
        }
        ForEach(apSlotFields[p.key] ?? [], id: \.field) { f in
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(f.label).boardFont(10.5, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
                WrapRow(spacing: 4) {
                    if f.field == "build" {
                        ForEach(apBuildTools, id: \.self) { b in
                            APButton(title: b + (b == apBuildTools[0] ? " 추천" : ""), tint: p.vers["build"] == b ? Palette.accent : nil, axid: "ap-ver-\(slot)-build-\(b)") {
                                t.setVersion(slot, "build", b)
                            }
                        }
                    } else {
                        let list = t.catalog?.catalog.list(f.field) ?? []
                        let cur = p.vers[f.field] ?? ""
                        ForEach(list, id: \.v) { v in
                            APButton(title: v.v + (v.lts ? " LTS" : "") + (v.rec ? " 추천" : "") + (v.until.map { " · 지원 \($0)" } ?? ""),
                                     tint: cur == v.v ? Palette.accent : nil, axid: "ap-ver-\(slot)-\(f.field)-\(v.v)") { t.setVersion(slot, f.field, v.v) }
                        }
                        if !cur.isEmpty, !list.contains(where: { $0.v == cur }) { APChip(text: cur + " · 목록에 없는 판(주제에 적힌 판)", color: APColor.limit, strong: true) }
                        if f.field == "nest" { apNote("TypeScript 는 NestJS 가 고른 판") }
                        if list.isEmpty { apNote(t.catalogLoading ? "받는 중" : "목록 없음") }
                    }
                }
            }
        }
        if p.key.hasPrefix("spring"), let n = t.catalog?.catalog.notes["java"] { apNote(n) }
        apNote("목록 출처 " + source(p.key) + (p.key == "fastapi" ? ". 공식 생성기가 없어 보드가 든 틀로 뼈대를 만듭니다(바깥 요청 없음)." : ""))
        if p.key == "postgresql" || p.key == "mysql" { apNote("시험은 뼈대의 docker compose 로 띄우는 로컬 DB 로 합니다. 운영 DB 연결과 배포는 범위 밖입니다.") }
    }
    private func source(_ key: String) -> String {
        switch key {
        case "spring-java", "spring-kotlin": "start.spring.io · endoflife.date"
        case "nest", "react", "next", "vue": "npm · endoflife.date"
        case "fastapi": "PyPI · endoflife.date"
        default: "endoflife.date"
        }
    }
}
