// 공유 규칙 표(rules-table/*.json) 시험 — macOS 보드와 Windows 판이 같은 줄을 읽고 같은 판정을 하는지 견준다. 꼴과 의무는 rules-table/README.md 가 소유한다.
// 줄은 사람이 쓴 의도한 동작이다 — Swift 출력에 맞춰 줄을 고치거나 어댑터를 구부리지 않는다. 어긋나면 그 줄이 실패로 남는다.
// 돌리는 쪽 의무(README): 파일의 모든 줄을 돌린다 · 모르는 rule 은 실패 · 실패한 id 를 모두 보인다(줄마다 따로 돌아 id 가 따로 찍힌다) ·
// 돌린 줄이 0 이면 실패(파일마다 따로 센다) · only 가 "windows" 인 줄은 건너뛴다. 여기에 더해 rule 마다 정한 input·expect 키 밖의 키는 실패다
// (안쪽 객체 — sessions[]·limits[]·screens[]·records[]·claude_sessions[]·pause·expect 의 groups[]·limits[] — 도 본다.
// 세션 파일 JSON 그대로인 record 와 API 응답 그대로인 body 는 안쪽 키를 보지 않는다).
// 어댑터는 진짜 BoardRules 함수를 각 시험 파일의 가짜로 부른다. 그 파일들의 가짜·도우미(World·FakeProcesses·FakeLive·FixedClock·id·account·
// epoch·session …)는 서로 이름이 겹쳐 internal 로 넓히면 다시 선언 오류가 나므로, 파일마다 넘겨 주는 함수(evaluateDecideRow·evaluateFailedRow·
// runScanRow·runGroupRows·runCacheRow·runSettleRow·runScreenSessionRow·runPlaceRow·runWorkspaceDotRow)를 두고 여기서는 표를 풀어 넘기기만 한다.
//
// 시각은 지금 = 0 기준 초(음수 = 과거)다 — 각 파일의 고정 시계(가짜 Clock·n0·t0·u0)에 줄의 초를 더한다. 다만 parse 의 기대 resets 는
// 1970 기준 밀리초다: README 는 "한도 resets 도 초" 라고 적었지만 parse 줄은 isoDate 결과의 밀리초를 적었다(README 는 따로 고친다).
// decide 의 limits[].resets 는 지금 기준 초다
import Foundation
import Testing
import BoardRules

// MARK: - 표 읽기

/// 표 한 줄 — 시험 인자는 Sendable 이어야 해 줄 객체를 JSON 바이트로 들고 시험 안에서 다시 푼다
struct RuleRow: Sendable, CustomTestStringConvertible {
    let family: String
    let id: String
    let json: Data
    var testDescription: String { id }
}

/// 표 파일 하나 — 돌릴 줄(only "windows" 는 뺐다), 뺀 줄 수, 파일 꼴 오류
struct RuleFile: Sendable {
    let family: String
    var rows: [RuleRow] = []
    var skipped = 0
    var errors: [String] = []
}

struct TableError: Error, CustomStringConvertible {
    let description: String
    init(_ d: String) { description = d }
}

enum RulesTable {
    /// README 의 파일(family)과 그 파일에서 쓸 수 있는 rule
    static let rules: [String: Set<String>] = [
        "sessions-group": ["group"],
        "session-scan": ["scan"],
        "usage": ["parse", "cache", "settle"],
        "autoswitch": ["decide", "failed"],
        "pairing": ["screen-session", "place", "workspace-dot"],
    ]
    static let families = rules.keys.sorted()

    /// 이 파일(#filePath)에서 위로 올라가며 rules-table/ 과 Package.swift 가 함께 있는 저장소 맨 위를 찾는다 — 시험을 돌린 자리(cwd)와 관계없다
    static let dir: URL? = {
        let fm = FileManager.default
        var url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        while url.path(percentEncoded: false) != "/" {
            let table = url.appendingPathComponent("rules-table")
            var isDir: ObjCBool = false
            if fm.fileExists(atPath: table.path(percentEncoded: false), isDirectory: &isDir), isDir.boolValue,
               fm.fileExists(atPath: url.appendingPathComponent("Package.swift").path(percentEncoded: false)) {
                return table
            }
            url.deleteLastPathComponent()
        }
        return nil
    }()

    /// 표 파일 하나를 읽는다. 파일을 못 읽거나 꼴이 틀려도 죽지 않고 오류로 모은다(파일 시험이 실패로 보인다)
    static func load(_ family: String) -> RuleFile {
        var f = RuleFile(family: family)
        guard let dir else { f.errors.append("rules-table/ 을 \(#filePath) 위에서 찾지 못했다"); return f }
        let url = dir.appendingPathComponent("\(family).json")
        guard let data = try? Data(contentsOf: url) else { f.errors.append("\(url.path(percentEncoded: false)) 를 읽지 못했다"); return f }
        guard let top = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            f.errors.append("\(family).json 이 JSON 객체가 아니다"); return f
        }
        let extra = Set(top.keys).subtracting(["family", "schema", "cases"]).sorted()
        if !extra.isEmpty { f.errors.append("파일의 모르는 키: \(extra.joined(separator: ", "))") }
        if top["family"] as? String != family { f.errors.append("family 가 \(family) 가 아니다: \(top["family"] ?? "없음")") }
        if let schema = top["schema"] as? NSNumber, !J.isBool(schema), schema.doubleValue == 1 {} else {
            f.errors.append("schema 가 1 이 아니다: \(top["schema"] ?? "없음")")
        }
        guard let cases = top["cases"] as? [Any] else { f.errors.append("cases 가 배열이 아니다"); return f }
        var seen = Set<String>()
        for (i, item) in cases.enumerated() {
            guard let c = item as? [String: Any] else { f.errors.append("cases[\(i)] 가 객체가 아니다"); continue }
            let id = (c["id"] as? String).flatMap { $0.isEmpty ? nil : $0 }
            if let id, !seen.insert(id).inserted { f.errors.append("id 가 겹친다: \(id)") }
            if c["only"] as? String == "windows" { f.skipped += 1; continue }   // 의무 5 — mac 쪽은 windows 줄을 건너뛴다
            guard let json = try? JSONSerialization.data(withJSONObject: c) else { f.errors.append("cases[\(i)] 를 다시 쓰지 못했다"); continue }
            f.rows.append(RuleRow(family: family, id: id ?? "\(family) cases[\(i)](id 없음)", json: json))
        }
        return f
    }

    // MARK: 한 줄 돌리기

    static func run(_ row: RuleRow) {
        do {
            let c = try J.object(try JSONSerialization.jsonObject(with: row.json), "줄")
            try J.allow(c, ["id", "rule", "why", "source", "input", "expect", "only"], "줄")
            _ = try J.string(c["id"], "id")
            let why = try J.string(c["why"], "why")
            if why.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { throw TableError("why 가 비었다") }
            if let s = c["source"] { _ = try J.string(s, "source") }
            if let o = c["only"], try J.string(o, "only") != "mac" { throw TableError("only 는 \"mac\"·\"windows\" 만 쓴다: \(o)") }
            let rule = try J.string(c["rule"], "rule")
            guard RulesTable.rules[row.family]?.contains(rule) == true else {
                Issue.record("모르는 rule \"\(rule)\" — \(row.family).json 의 rule 은 \((rules[row.family] ?? []).sorted())")
                return
            }
            let input = try J.object(c["input"], "input"), expect = try J.object(c["expect"], "expect")
            switch rule {
            case "group": try group(input, expect, why)
            case "scan": try scan(input, expect, why)
            case "parse": try parse(input, expect, why)
            case "cache": try cache(input, expect, why)
            case "settle": try settle(input, expect, why)
            case "decide": try decide(row.id, input, expect, why)
            case "failed": try failed(input, expect, why)
            case "screen-session": try screenSession(input, expect, why)
            case "place": try place(input, expect, why)
            case "workspace-dot": try workspaceDot(input, expect, why)
            default: Issue.record("모르는 rule \"\(rule)\"")
            }
        } catch {
            Issue.record("표 꼴: \(error)")
        }
    }

    // MARK: sessions-group

    // records 의 record 는 세션 파일 JSON 그대로(since·updated 만 지금 기준 초) → 앱과 같은 차례로 묶고 줄 세운 카드(id, 세션 id 차례)
    private static func group(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["records", "pinned"], "input")
        try J.allow(expect, ["groups"], "expect")
        let records = try J.array(input["records"], "input.records").enumerated().map { i, v -> (file: String, record: [String: Any]) in
            let at = "input.records[\(i)]"
            let o = try J.object(v, at)
            try J.allow(o, ["file", "record"], at)
            return (try J.string(o["file"], "\(at).file"), try J.object(o["record"], "\(at).record"))
        }
        let pinned = try J.optional(input, "pinned", "input", J.strings) ?? []
        let want = try J.array(expect["groups"], "expect.groups").enumerated().map { i, v -> String in
            let at = "expect.groups[\(i)]"
            let o = try J.object(v, at)
            try J.allow(o, ["id", "sessions"], at)
            return "\(try J.string(o["id"], "\(at).id")): \(try J.strings(o["sessions"], "\(at).sessions").joined(separator: ", "))"
        }
        let got = runGroupRows(records: records, pinned: pinned).map { "\($0.id): \($0.sessions.joined(separator: ", "))" }
        #expect(got == want, "why: \(why)")
    }

    // MARK: session-scan

    private static func scan(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["file", "record", "transcript", "claude_sessions", "procs", "paused"], "input")
        try J.allow(expect, ["kept", "state"], "expect")
        let file = try J.string(input["file"], "input.file")
        let record = try J.object(input["record"], "input.record")
        let transcript = try J.optional(input, "transcript", "input", J.bool) ?? true   // 없으면 대화 기록이 있다(README 에 기본값이 없어 정함)
        let paused = try J.optional(input, "paused", "input", J.bool) ?? false
        let claude = try (J.optional(input, "claude_sessions", "input", J.array) ?? []).enumerated()
            .map { i, v -> (pid: Int32, sessionId: String, age: Double) in
                let at = "input.claude_sessions[\(i)]"
                let o = try J.object(v, at)
                try J.allow(o, ["pid", "sessionId", "age"], at)
                return (try J.pid(o["pid"], "\(at).pid"), try J.string(o["sessionId"], "\(at).sessionId"), try J.number(o["age"], "\(at).age"))
            }
        var procs: [Int32: Pid] = [:]
        let procTable: [String: Any] = try J.optional(input, "procs", "input", J.object) ?? [:]
        for (k, v) in procTable {
            guard let pid = Int32(k) else { throw TableError("input.procs 의 키가 pid 가 아니다: \(k)") }
            switch try J.string(v, "input.procs.\(k)") {
            case "alive": procs[pid] = .alive
            case "gone": procs[pid] = .gone
            case "denied": procs[pid] = .denied   // EPERM — ProcessProbe.isGone 은 false(살아 있다고 본다), ESRCH 인 gone 과 다르다
            case let s: throw TableError("input.procs.\(k) 는 alive·gone·denied 만 쓴다: \(s)")
            }
        }
        let wantKept = try J.bool(expect["kept"], "expect.kept")
        let wantState = try J.optional(expect, "state", "expect", J.string)
        if !wantKept, wantState != nil { throw TableError("expect.kept 가 false 인데 state 가 있다") }
        let got = runScanRow(file: file, record: record, transcript: transcript, claudeSessions: claude, procs: procs, paused: paused)
        let wantText = wantKept ? (wantState.map { "kept state=\($0)" } ?? "kept") : "removed"
        let gotText = got.kept ? (wantState != nil ? "kept state=\(got.state ?? "nil")" : "kept") : "removed"
        #expect(gotText == wantText, "why: \(why)")
    }

    // MARK: usage

    // 기대 resets 는 1970 기준 밀리초(파일 머리 주석) — UsageTests 의 L 과 같은 꼴로 견준다
    private static func parse(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["body", "body_raw"], "input")
        try J.allow(expect, ["limits"], "expect")
        guard let data = try J.body(input) else { throw TableError("input 에 body·body_raw 가 없다") }
        guard expect.keys.contains("limits") else { throw TableError("expect.limits 가 없다") }
        let want: [L]? = try J.optional(expect, "limits", "expect") { v, at in
            try J.array(v, at).enumerated().map { i, item -> L in
                let at = "\(at)[\(i)]"
                let o = try J.object(item, at)
                try J.allow(o, ["id", "label", "pct", "resets"], at)
                return L(try J.string(o["id"], "\(at).id"), try J.string(o["label"], "\(at).label"), try J.number(o["pct"], "\(at).pct"),
                         try J.optional(o, "resets", at, J.int64))
            }
        }
        let got = parseUsage(data)?.map(L.init)
        #expect(got == want, "why: \(why)")
    }

    private static func cache(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["modified", "trust_after", "body", "body_raw", "fetching"], "input")
        try J.allow(expect, ["cached", "fetch"], "expect")
        guard expect.keys.contains("cached") else { throw TableError("expect.cached 가 없다") }
        let modified = try J.optional(input, "modified", "input", J.number)       // null(또는 없음) = 캐시 파일 없음
        let trustAfter = try J.optional(input, "trust_after", "input", J.number)  // 없으면 아주 먼 과거
        let fetching = try J.optional(input, "fetching", "input", J.bool) ?? false
        let content = try J.body(input)                                            // 없으면 내용을 못 읽음
        let wantCached = try J.optional(expect, "cached", "expect", J.strings)
        let wantFetch = try J.bool(expect["fetch"], "expect.fetch")
        let got = runCacheRow(modified: modified, body: content, trustAfter: trustAfter, fetching: fetching)
        #expect(cacheText(got.cached, got.fetch) == cacheText(wantCached, wantFetch), "why: \(why)")
    }

    private static func cacheText(_ cached: [String]?, _ fetch: Bool) -> String {
        "cached=\(cached.map { "[\($0.joined(separator: ","))]" } ?? "nil") fetch=\(fetch)"
    }

    private static func settle(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["body", "body_raw", "current_empty"], "input")
        try J.allow(expect, ["applied", "cache_written", "failed"], "expect")
        guard expect.keys.contains("failed") else { throw TableError("expect.failed 가 없다") }
        let content = try J.body(input)                                            // 없으면 200 응답이 없다
        let currentEmpty = try J.bool(input["current_empty"], "input.current_empty")
        let want = settleText(try J.bool(expect["applied"], "expect.applied"), try J.bool(expect["cache_written"], "expect.cache_written"),
                              try J.optional(expect, "failed", "expect", J.bool))
        let got = runSettleRow(body: content, currentEmpty: currentEmpty)
        #expect(settleText(got.applied, got.cacheWritten, got.failed) == want, "why: \(why)")
    }

    private static func settleText(_ applied: Bool, _ written: Bool, _ failed: Bool?) -> String {
        "applied=\(applied) cache_written=\(written) failed=\(failed.map(String.init) ?? "null(그대로)")"
    }

    // MARK: autoswitch

    // 이름은 AutoSwitchTests 의 DecideRow·PauseSpec 과 같다(README). 줄을 DecideRow 로 옮겨 그 표와 같은 길(evaluateDecideRow)로 돌린다
    private static func decide(_ id: String, _ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["sessions", "limits", "current", "saved", "expired", "full", "resumed", "last_switch", "pending", "switching", "pause"],
                    "input")
        try J.allow(expect, ["move", "marks", "resume"], "expect")
        var r = DecideRow(note: id, move: try J.string(expect["move"], "expect.move"))
        r.sessions = try (J.optional(input, "sessions", "input", J.array) ?? []).enumerated().map { i, v -> SessionSpec in
            let at = "input.sessions[\(i)]"
            let o = try J.object(v, at)
            try J.allow(o, ["id", "state", "limit_at", "cwd", "message"], at)
            var s = SessionSpec(id: try J.string(o["id"], "\(at).id"))
            if let state = try J.optional(o, "state", at, J.string) { s.state = state }
            s.limitAt = try J.optional(o, "limit_at", at, J.number)
            s.cwd = try J.optional(o, "cwd", at, J.string)
            if let m = try J.optional(o, "message", at, J.string) { s.message = m }
            return s
        }
        r.limits = try (J.optional(input, "limits", "input", J.array) ?? []).enumerated().map { i, v -> LimitSpec in
            let at = "input.limits[\(i)]"
            let o = try J.object(v, at)
            try J.allow(o, ["id", "pct", "resets"], at)
            return LimitSpec(try J.string(o["id"], "\(at).id"), try J.number(o["pct"], "\(at).pct"), try J.optional(o, "resets", at, J.number))
        }
        // 없으면 DecideRow 의 기본("A"), null 이면 지금 계정을 모름
        if input.keys.contains("current") { r.current = try J.optional(input, "current", "input", J.string) }
        if let saved = try J.optional(input, "saved", "input", J.strings) { r.saved = saved }
        r.expired = try J.optional(input, "expired", "input", J.strings) ?? []
        r.full = try J.optional(input, "full", "input", J.numbers) ?? [:]
        r.resumed = try J.optional(input, "resumed", "input", J.strings) ?? []
        if let last = try J.optional(input, "last_switch", "input", J.number) { r.lastSwitch = last }
        r.pending = try J.optional(input, "pending", "input", J.bool) ?? false
        r.switching = try J.optional(input, "switching", "input", J.bool) ?? false
        r.pause = try J.optional(input, "pause", "input", pauseSpec)
        r.marks = try J.optional(expect, "marks", "expect", J.numbers)     // 없거나 null 이면 표시는 그대로
        r.resume = try J.optional(expect, "resume", "expect", J.strings) ?? []
        let got = evaluateDecideRow(r)
        #expect(got.move == r.move, "why: \(why)")
        #expect(marksText(got.marks) == marksText(r.marks), "marks — why: \(why)")
        #expect(got.resume == r.resume, "resume — why: \(why)")
        #expect(got.keysOK, "이어 가기 표지가 \"세션 id@한도 시각\" 이 아니다")
    }

    private static func pauseSpec(_ v: Any?, _ at: String) throws -> PauseSpec {
        let o = try J.object(v, at)
        try J.allow(o, ["account", "until"], at)
        return PauseSpec(try J.string(o["account"], "\(at).account"), try J.number(o["until"], "\(at).until"))
    }

    // 실패의 쪽은 BoardRules 의 SwitchFailureSide 그대로 — 30초·1시간은 autoSwitchFailed(AutoSwitchRule)가 정한다
    private static func failed(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["side", "target", "current", "full"], "input")
        try J.allow(expect, ["marks", "pause"], "expect")
        let side: SwitchFailureSide
        switch try J.string(input["side"], "input.side") {
        case "target": side = .target
        case "other": side = .other
        case "busy": side = .busy
        case let s: throw TableError("input.side 는 target·other·busy 만 쓴다: \(s)")
        }
        let wantMarks = try J.numbers(expect["marks"], "expect.marks")
        let wantPause = try J.optional(expect, "pause", "expect", pauseSpec)   // 없거나 null 이면 쉬지 않는다
        let got = evaluateFailedRow(side, target: try J.string(input["target"], "input.target"), current: try J.string(input["current"], "input.current"),
                                    full: try J.optional(input, "full", "input", J.numbers) ?? [:])
        #expect(marksText(got.marks) == marksText(wantMarks), "marks — why: \(why)")
        #expect(got.pause.map { "\($0.account) until \(J.num($0.until))" } == wantPause.map { "\($0.account) until \(J.num($0.until))" },
                "pause — why: \(why)")
    }

    private static func marksText(_ m: [String: Double]?) -> String {
        m.map { "{" + $0.sorted { $0.key < $1.key }.map { "\($0.key): \(J.num($0.value))" }.joined(separator: ", ") + "}" } ?? "nil(그대로)"
    }

    // MARK: pairing

    private static func readScreens(_ input: [String: Any]) throws -> [(name: String, tty: String, sessionId: String?)] {
        let list = try J.array(input["screens"], "input.screens").enumerated().map { i, v -> (name: String, tty: String, sessionId: String?) in
            let at = "input.screens[\(i)]"
            let o = try J.object(v, at)
            try J.allow(o, ["id", "tty", "session_id"], at)
            return (try J.string(o["id"], "\(at).id"), try J.string(o["tty"], "\(at).tty"), try J.optional(o, "session_id", at, J.string))
        }
        if Set(list.map(\.name)).count != list.count { throw TableError("input.screens 의 id 가 겹친다") }
        return list
    }

    private static func readSessions(_ input: [String: Any]) throws -> [SessSpec] {
        let list = try J.array(input["sessions"], "input.sessions").enumerated().map { i, v -> SessSpec in
            let at = "input.sessions[\(i)]"
            let o = try J.object(v, at)
            try J.allow(o, ["id", "state", "tty", "updated", "since", "headless"], at)
            var s = SessSpec(id: try J.string(o["id"], "\(at).id"), state: try J.string(o["state"], "\(at).state"))
            s.tty = try J.optional(o, "tty", at, J.string) ?? ""
            s.headless = try J.optional(o, "headless", at, J.bool) ?? false
            s.updated = try J.optional(o, "updated", at, J.number) ?? 0
            s.since = try J.optional(o, "since", at, J.number) ?? 0
            return s
        }
        if Set(list.map(\.id)).count != list.count { throw TableError("input.sessions 의 id 가 겹친다") }
        return list
    }

    private static func screenSession(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["screens", "sessions"], "input")
        try J.allow(expect, ["screen_session", "paired"], "expect")
        let screens = try readScreens(input), sessions = try readSessions(input)
        var wantScreens: [String: String?] = [:]
        for (k, v) in try J.object(expect["screen_session"], "expect.screen_session") {
            if v is NSNull { wantScreens.updateValue(nil, forKey: k) } else { wantScreens[k] = try J.string(v, "expect.screen_session.\(k)") }
        }
        let wantPaired = try J.stringMap(expect["paired"], "expect.paired")
        let got = runScreenSessionRow(screens: screens, sessions: sessions)
        #expect(mapText(got.screenSession) == mapText(wantScreens), "screen_session — why: \(why)")
        #expect(mapText(got.paired) == mapText(wantPaired), "paired — why: \(why)")
    }

    private static func place(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["screens", "sessions", "runs_claude", "owned"], "input")
        try J.allow(expect, ["place"], "expect")
        let screens = try readScreens(input), sessions = try readSessions(input)
        let runs = try J.optional(input, "runs_claude", "input", J.strings) ?? []
        for name in runs where !screens.contains(where: { $0.name == name }) { throw TableError("input.runs_claude 의 \(name) 는 화면에 없다") }
        // owned(화면 안에서 시작된 headless 세션)는 그 세션을 띄운 화면을 부모 pid 로 잇는다 — 그 세션 id 를 기억한 화면,
        // 없으면 화면이 하나일 때 그 화면. 그 밖은 표가 화면을 말하지 않아 어댑터가 정할 수 없다
        var owned: [String: String] = [:]
        for sid in try J.optional(input, "owned", "input", J.strings) ?? [] {
            guard sessions.contains(where: { $0.id == sid }) else { throw TableError("input.owned 의 \(sid) 는 세션에 없다") }
            if let s = screens.first(where: { $0.sessionId == sid }) { owned[sid] = s.name }
            else if screens.count == 1 { owned[sid] = screens[0].name }
            else { throw TableError("input.owned 의 \(sid) 를 띄운 화면을 정할 수 없다") }
        }
        let want = try J.stringMap(expect["place"], "expect.place")
        let got = runPlaceRow(screens: screens, sessions: sessions, runsClaude: Set(runs), owned: owned)
        #expect(mapText(got) == mapText(want), "why: \(why)")
    }

    private static func workspaceDot(_ input: [String: Any], _ expect: [String: Any], _ why: String) throws {
        try J.allow(input, ["states"], "input")
        try J.allow(expect, ["state"], "expect")
        let states = try J.strings(input["states"], "input.states")
        let want = try J.string(expect["state"], "expect.state")
        #expect(runWorkspaceDotRow(states: states) == want, "why: \(why)")
    }

    private static func mapText(_ m: [String: String?]) -> String {
        m.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value ?? "null")" }.joined(separator: " ")
    }
    private static func mapText(_ m: [String: String]) -> String { mapText(m.mapValues { Optional($0) }) }
}

// MARK: - JSON 값 읽기 — 꼴이 틀리면 던진다(줄이 "표 꼴" 실패로 보인다). JSONSerialization 의 참·거짓은 NSNumber 라 숫자와 가른다

enum J {
    static func isBool(_ v: Any?) -> Bool { (v as? NSNumber).map { CFGetTypeID($0) == CFBooleanGetTypeID() } ?? false }

    static func allow(_ o: [String: Any], _ keys: Set<String>, _ at: String) throws {
        let extra = Set(o.keys).subtracting(keys).sorted()
        if !extra.isEmpty { throw TableError("\(at) 의 모르는 키: \(extra.joined(separator: ", "))") }
    }
    static func object(_ v: Any?, _ at: String) throws -> [String: Any] {
        guard let o = v as? [String: Any] else { throw TableError("\(at) 가 객체가 아니다") }
        return o
    }
    static func array(_ v: Any?, _ at: String) throws -> [Any] {
        guard let a = v as? [Any] else { throw TableError("\(at) 가 배열이 아니다") }
        return a
    }
    static func string(_ v: Any?, _ at: String) throws -> String {
        guard let s = v as? String else { throw TableError("\(at) 가 글자가 아니다") }
        return s
    }
    static func number(_ v: Any?, _ at: String) throws -> Double {
        guard let n = v as? NSNumber, !isBool(n) else { throw TableError("\(at) 가 수가 아니다") }
        return n.doubleValue
    }
    static func bool(_ v: Any?, _ at: String) throws -> Bool {
        guard isBool(v), let n = v as? NSNumber else { throw TableError("\(at) 가 true·false 가 아니다") }
        return n.boolValue
    }
    static func int64(_ v: Any?, _ at: String) throws -> Int64 {
        guard let n = Int64(exactly: try number(v, at)) else { throw TableError("\(at) 가 정수가 아니다") }
        return n
    }
    static func pid(_ v: Any?, _ at: String) throws -> Int32 {
        guard let n = Int32(exactly: try number(v, at)) else { throw TableError("\(at) 가 pid 가 아니다") }
        return n
    }
    static func strings(_ v: Any?, _ at: String) throws -> [String] {
        try array(v, at).enumerated().map { try string($1, "\(at)[\($0)]") }
    }
    static func numbers(_ v: Any?, _ at: String) throws -> [String: Double] {
        try object(v, at).reduce(into: [:]) { $0[$1.key] = try number($1.value, "\(at).\($1.key)") }
    }
    static func stringMap(_ v: Any?, _ at: String) throws -> [String: String] {
        try object(v, at).reduce(into: [:]) { $0[$1.key] = try string($1.value, "\(at).\($1.key)") }
    }
    /// 없거나 null 이면 nil
    static func optional<T>(_ o: [String: Any], _ key: String, _ at: String, _ read: (Any?, String) throws -> T) throws -> T? {
        guard let v = o[key], !(v is NSNull) else { return nil }
        return try read(v, "\(at).\(key)")
    }
    /// body(JSON 값 그대로 바이트로) 또는 body_raw(JSON 이 아닌 글자 그대로). 둘 다 없으면 nil
    static func body(_ input: [String: Any]) throws -> Data? {
        switch (input["body"], input["body_raw"]) {
        case (nil, nil): return nil
        case (let b?, nil): return try JSONSerialization.data(withJSONObject: b, options: [.fragmentsAllowed])
        case (nil, let raw?): return Data(try string(raw, "input.body_raw").utf8)
        default: throw TableError("input 에 body 와 body_raw 가 함께 있다")
        }
    }
    /// 정수면 정수로(3600), 아니면 그대로(99.9)
    static func num(_ v: Double) -> String { v.rounded() == v && abs(v) < 1e15 ? String(Int64(v)) : String(v) }
}

// MARK: - 시험

@Suite("공유 규칙 표(rules-table)") struct RulesTableTests {
    @Test("sessions-group.json", arguments: RulesTable.load("sessions-group").rows)
    func sessionsGroup(_ row: RuleRow) { RulesTable.run(row) }

    @Test("session-scan.json", arguments: RulesTable.load("session-scan").rows)
    func sessionScan(_ row: RuleRow) { RulesTable.run(row) }

    @Test("usage.json", arguments: RulesTable.load("usage").rows)
    func usage(_ row: RuleRow) { RulesTable.run(row) }

    @Test("autoswitch.json", arguments: RulesTable.load("autoswitch").rows)
    func autoswitch(_ row: RuleRow) { RulesTable.run(row) }

    @Test("pairing.json", arguments: RulesTable.load("pairing").rows)
    func pairing(_ row: RuleRow) { RulesTable.run(row) }

    // 의무 4 — 돌린 줄이 0 이면 실패. 파일을 못 읽거나 꼴이 틀려도(줄 시험은 0개로 조용히 지나간다) 여기서 실패한다
    @Test("파일마다 꼴이 맞고 돌린 줄이 0 보다 많다", arguments: RulesTable.families)
    func 파일(_ family: String) {
        let f = RulesTable.load(family)
        #expect(f.errors.isEmpty, "\(family).json: \(f.errors.joined(separator: " · "))")
        #expect(f.rows.count > 0, "\(family).json 에서 돌린 줄이 0 이다(건너뛴 windows 줄 \(f.skipped))")
    }

    // 모르는 표 파일이 생기면 아무도 돌리지 않은 채 지나가지 않게
    @Test("rules-table/ 의 표 파일은 모두 이 시험이 아는 파일이다")
    func 아는_파일만() throws {
        let dir = try #require(RulesTable.dir)
        let files = try FileManager.default.contentsOfDirectory(atPath: dir.path(percentEncoded: false)).filter { $0.hasSuffix(".json") }
        #expect(Set(files) == Set(RulesTable.families.map { "\($0).json" }))
    }
}
