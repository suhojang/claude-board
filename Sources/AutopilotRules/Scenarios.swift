// 자율주행 규칙(AutopilotRules) — 시험 실행기(5단계 ⑪-1, 2026-10-30, 설계 2장 "판정 시나리오 잠금"·4장 "시험 환경 확인, 고정 단계, 판정").
// 단위의 판정 시나리오(실행 파일)를 엔진이 돌려 통과·실패를 가린다. 여기는 그 순수 규칙이다:
// - 대기열 칸 읽기: 시나리오 `scenarios`(작업 폴더 기준 상대 경로·종류·짝지은 수용 기준), 수용 기준 `criteria`, 판정 샌드박스 `check_sandbox`
// - 돌리는 법과 필요한 도구: 첫 줄 #! 이나 확장자로 해석기를 고르고, 종류(셸·브라우저)마다 필요한 도구를 낸다
// - 환경 확인: 도구가 있는지(포트 APToolProbe), 없으면 준비 화면의 실패 줄과 고칠 것 글
// - 판정 차례와 결과: 시나리오들(고정본) → 판정 명령. 모두 0 이면 통과. 고정본 지문이 출발 때와 다르면 돌리지 않는다(포트 APJudgeRunner)
// - 증거 요약(summary.json)과 판정 미리 돌리기의 시나리오 카드, 실행 파일이 없는 수용 기준 카드
// 프로세스·파일·샌드박스 적용은 엔진(진짜 어댑터)이, 가짜 어댑터는 Tests/AutopilotRulesTests 에만 둔다.
// 사람이 읽는 글(준비 화면·진행 기록·모델에게 넘기는 글)에는 긴 줄표를 쓰지 않는다
import Foundation

// MARK: - 시나리오와 수용 기준 (대기열 칸)

public enum APScenarioKind: String, Equatable, Sendable, CaseIterable {
    case shell      // 명령·HTTP 요청을 단언하는 실행 파일(셸·파이썬·노드 등, 해석기는 첫 줄 #! 이나 확장자)
    case browser    // 브라우저 시나리오: 해석기 + 브라우저 드라이버
}

public enum APBrowserDriver: String, Equatable, Sendable, CaseIterable {
    case chrome       // 깔려 있는 Chrome 계열 브라우저를 headless 로(엔진이 $CLAUDE_BOARD_BROWSER·$CLAUDE_BOARD_BROWSER_FLAGS 로 넘김)
    case playwright   // node 모듈 playwright(보드는 깔지 않는다)
}

public struct APScenario: Equatable, Sendable {
    public var file: String               // 작업 폴더(cwd) 기준 상대 경로
    public var kind: APScenarioKind
    public var criterion: String?         // 짝지은 수용 기준 id
    public var driver: APBrowserDriver?   // 브라우저만(기본 chrome)
    public init(file: String, kind: APScenarioKind = .shell, criterion: String? = nil, driver: APBrowserDriver? = nil) {
        self.file = file; self.kind = kind; self.criterion = criterion
        self.driver = kind == .browser ? (driver ?? .chrome) : nil
    }
}

public struct APCriterion: Equatable, Sendable {
    public var id: String
    public var text: String
    public init(id: String, text: String) { self.id = id; self.text = text }
}

public let apScenarioMax = 30   // 단위 하나의 시나리오 수 상한

// 작업 폴더 기준 상대 경로로 쓸 수 있는가(절대 경로·~·.. ·빈 조각·조절 글자·따옴표는 받지 않는다 — 고정본 자리와 샌드박스 글에 들어가므로)
public func apScenarioPathOK(_ p: String) -> Bool {
    guard !p.isEmpty, p.count <= 300, !p.hasPrefix("/"), !p.hasPrefix("~") else { return false }
    if p.unicodeScalars.contains(where: { $0.value < 0x20 || $0 == "\"" || $0 == "\\" || $0.value == 0x7f }) { return false }
    let parts = p.split(separator: "/", omittingEmptySubsequences: false)
    return !parts.contains { $0.isEmpty || $0 == ".." || $0 == "." }
}

// 수용 기준 id 꼴: 영문·숫자·.-_ (카드 id 와 파일 글에 들어간다)
public func apCriterionIdOK(_ s: String) -> Bool {
    !s.isEmpty && s.count <= 40 && s.range(of: "^[A-Za-z0-9._-]+$", options: .regularExpression) != nil
}

// 단위의 scenarios 칸 읽기. 고칠 것은 "단위 n 의 …" 문장으로(엔진 출발 전 확인의 고칠 것)
public func apParseScenarios(_ v: Any?, unit n: Int) -> (items: [APScenario], problems: [String]) {
    guard let v else { return ([], []) }
    guard let arr = v as? [Any] else { return ([], ["단위 \(n) 의 scenarios 는 배열이어야 합니다"]) }
    var items: [APScenario] = [], problems: [String] = []
    if arr.count > apScenarioMax { problems.append("단위 \(n) 의 시나리오가 \(arr.count)개입니다(\(apScenarioMax)개까지)") }
    for (i, x) in arr.prefix(apScenarioMax).enumerated() {
        let k = i + 1
        // 글자 하나면 셸 시나리오 파일 경로로 본다
        let o: [String: Any] = (x as? String).map { ["file": $0] } ?? (x as? [String: Any]) ?? [:]
        guard let file = o["file"] as? String, apScenarioPathOK(file) else {
            problems.append("단위 \(n) 의 시나리오 \(k) 의 file 은 작업 폴더 기준 상대 경로(.. 없이)여야 합니다"); continue
        }
        var kind = APScenarioKind.shell
        if let ks = o["kind"] {
            guard let s = ks as? String, let kk = APScenarioKind(rawValue: s) else {
                problems.append("단위 \(n) 의 시나리오 \(k) 의 kind 는 shell 또는 browser 입니다"); continue
            }
            kind = kk
        }
        var driver: APBrowserDriver?
        if let ds = o["driver"] {
            guard kind == .browser, let s = ds as? String, let dd = APBrowserDriver(rawValue: s) else {
                problems.append("단위 \(n) 의 시나리오 \(k) 의 driver 는 브라우저 시나리오에만, chrome 또는 playwright 입니다"); continue
            }
            driver = dd
        }
        var crit: String?
        if let c = o["criterion"] {
            guard let s = c as? String, apCriterionIdOK(s) else {
                problems.append("단위 \(n) 의 시나리오 \(k) 의 criterion 은 수용 기준 id(영문·숫자·.-_)입니다"); continue
            }
            crit = s
        }
        if items.contains(where: { $0.file == file }) { problems.append("단위 \(n) 의 시나리오 \(file) 가 두 번 있습니다"); continue }
        items.append(APScenario(file: file, kind: kind, criterion: crit, driver: driver))
    }
    return (items, problems)
}

// 단위의 criteria 칸 읽기(수용 기준: [{id, text}])
public func apParseCriteria(_ v: Any?, unit n: Int) -> (items: [APCriterion], problems: [String]) {
    guard let v else { return ([], []) }
    guard let arr = v as? [Any] else { return ([], ["단위 \(n) 의 criteria 는 배열이어야 합니다"]) }
    var items: [APCriterion] = [], problems: [String] = []
    for (i, x) in arr.enumerated() {
        guard let o = x as? [String: Any], let id = o["id"] as? String, apCriterionIdOK(id) else {
            problems.append("단위 \(n) 의 수용 기준 \(i + 1) 에 id(영문·숫자·.-_)가 없습니다"); continue
        }
        let text = (o["text"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { problems.append("단위 \(n) 의 수용 기준 \(id) 에 text 가 없습니다"); continue }
        if items.contains(where: { $0.id == id }) { problems.append("단위 \(n) 의 수용 기준 \(id) 가 두 번 있습니다"); continue }
        items.append(APCriterion(id: id, text: text))
    }
    return (items, problems)
}

// 시나리오가 가리킨 수용 기준이 목록에 있는가(수용 기준을 적은 단위만 본다)
public func apScenarioCriterionProblems(_ scenarios: [APScenario], _ criteria: [APCriterion], unit n: Int) -> [String] {
    guard !criteria.isEmpty else { return [] }
    let ids = Set(criteria.map(\.id))
    return scenarios.compactMap { s in
        s.criterion.flatMap { ids.contains($0) ? nil : "단위 \(n) 의 시나리오 \(s.file) 가 가리킨 수용 기준 \($0) 가 criteria 에 없습니다" }
    }
}

// 실행 파일(시나리오)이 짝지어지지 않은 수용 기준 — 출발 카드로 묻는다(동결 뒤로는 묻지 않는다)
public func apCriteriaGaps(_ criteria: [APCriterion], _ scenarios: [APScenario]) -> [APCriterion] {
    let covered = Set(scenarios.compactMap(\.criterion))
    return criteria.filter { !covered.contains($0.id) }
}

public func apScenarioJSON(_ s: APScenario) -> [String: Any] {
    var o: [String: Any] = ["file": s.file, "kind": s.kind.rawValue]
    if let c = s.criterion { o["criterion"] = c }
    if let d = s.driver { o["driver"] = d.rawValue }
    return o
}

public func apCriterionJSON(_ c: APCriterion) -> [String: Any] { ["id": c.id, "text": c.text] }

// MARK: - 수용 기준 카드(실행 파일이 없는 기준)

public let apCriterionModelJudge = "model_judge"     // 모델 판정으로 두기: 단위에 넣고, 그 기준은 결과 기록의 근거로 본다
public let apCriterionDrop = "drop_criterion"         // 범위에서 빼기(추천): 이 기준만 이번 범위에서 뺀다(단위는 남는다)

public func apCriterionCardId(unit: String, criterion: String) -> String { "criterion-\(unit)-\(criterion)" }

public struct APCardText: Equatable, Sendable {
    public var label: String, title: String, why: String
    public var options: [(key: String, label: String, note: String)]
    public var recommended: String
    public static func == (a: APCardText, b: APCardText) -> Bool {
        a.label == b.label && a.title == b.title && a.why == b.why && a.recommended == b.recommended
            && a.options.map { "\($0.key)|\($0.label)|\($0.note)" } == b.options.map { "\($0.key)|\($0.label)|\($0.note)" }
    }
}

// 실행 파일이 없는 수용 기준 카드의 글. 추천은 범위에서 빼기(실행으로 확인하지 못한 통과를 남기지 않게. 설계 1장 "기준을 낮춰야만 통과하면 미통과"와 같은 뿌리)
public func apCriterionCard(unitTitle: String, criterion c: APCriterion) -> APCardText {
    APCardText(label: "실행 파일이 없는 수용 기준",
               title: "「\(unitTitle)」 의 수용 기준 \(c.id) 를 볼 판정 시나리오가 없습니다",
               why: "\(c.text) (이 기준은 실행 파일로 판정하지 않아 통과·실패를 기계로 가리지 못합니다.)",
               options: [(apCriterionDrop, "범위에서 빼기", "이 기준만 이번 실행에서 하지 않습니다. 단위는 남고 다음 출발에서 다시 보입니다"),
                         (apCriterionModelJudge, "모델 판정으로 두기", "단위에 넣고 그 기준은 결과 기록의 근거로 봅니다. 사람이 합치기·승격 때 확인합니다")],
               recommended: apCriterionDrop)
}

// 출발 때 고정할 수용 기준의 답: 시나리오가 있으면 "scenario", 없으면 카드 답(없거나 꼴이 틀리면 추천 = 범위에서 빼기, defaulted 참)
public func apCriterionAnswer(_ c: APCriterion, scenarios: [APScenario], answer: String?) -> (answer: String, defaulted: Bool) {
    if scenarios.contains(where: { $0.criterion == c.id }) { return ("scenario", false) }
    if let a = answer, a == apCriterionModelJudge || a == apCriterionDrop { return (a, false) }
    return (apCriterionDrop, true)
}

// MARK: - 돌리는 법과 필요한 도구

public enum APToolNeed: Hashable, Sendable {
    case program(String)      // 절대 경로의 실행 파일(첫 줄 #! 의 해석기 등)
    case command(String)      // 로그인 셸 PATH 에서 찾는 명령
    case browser              // Chrome 계열 브라우저(드라이버 chrome)
    case nodeModule(String)   // 작업 폴더에서 node 가 찾는 모듈(드라이버 playwright)
}

public func apToolLabel(_ n: APToolNeed) -> String {
    switch n {
    case .program(let p): return p
    case .command(let c): return "\(c)(PATH)"
    case .browser: return "브라우저(Google Chrome·Chromium·Microsoft Edge)"
    case .nodeModule(let m): return "node 모듈 \(m)"
    }
}

public struct APScenarioRun: Equatable, Sendable {
    public var interpreter: [String]   // 파일 경로 앞에 붙는 해석기와 인자
    public var needs: [APToolNeed]
    public var problem: String?        // 돌리는 법을 모름
}

private let extInterpreters: [String: [String]] = [
    "sh": ["/bin/sh"], "bash": ["/bin/bash"], "zsh": ["/bin/zsh"],
    "py": ["/usr/bin/env", "python3"],
    "js": ["/usr/bin/env", "node"], "mjs": ["/usr/bin/env", "node"], "cjs": ["/usr/bin/env", "node"],
    "ts": ["/usr/bin/env", "node"], "mts": ["/usr/bin/env", "node"],
]

// 해석기 argv 에서 필요한 도구: 첫 조각은 절대 경로 실행 파일, env 면 다음 이름(-S·-i 같은 선택 칸은 건너뜀)을 PATH 에서
private func interpreterNeeds(_ argv: [String]) -> [APToolNeed] {
    guard let first = argv.first else { return [] }
    var needs: [APToolNeed] = [.program(first)]
    if (first as NSString).lastPathComponent == "env", let name = argv.dropFirst().first(where: { !$0.hasPrefix("-") }) { needs.append(.command(name)) }
    return needs
}

// 시나리오를 어떻게 돌릴까: 첫 줄이 #! 면 그 해석기(절대 경로여야 함), 아니면 확장자. 브라우저는 드라이버 도구를 더한다
public func apScenarioRun(_ s: APScenario, head: String) -> APScenarioRun {
    var argv: [String] = []
    var problem: String?
    let first = head.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? ""
    if first.hasPrefix("#!") {
        argv = first.dropFirst(2).split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "\r" }).map(String.init)
        if argv.first.map({ !$0.hasPrefix("/") }) ?? true {
            problem = "첫 줄 #! 의 해석기가 절대 경로가 아닙니다(\(first.prefix(60)))"; argv = []
        }
    } else {
        let ext = (s.file as NSString).pathExtension.lowercased()
        if let a = extInterpreters[ext] { argv = a }
        else { problem = "돌리는 법을 모릅니다. 첫 줄에 #! 해석기를 쓰거나 확장자를 .sh·.bash·.zsh·.py·.js·.mjs·.cjs·.ts 로 씁니다" }
    }
    var needs = interpreterNeeds(argv)
    if s.kind == .browser {
        switch s.driver ?? .chrome {
        case .chrome: needs.append(.browser)
        case .playwright:
            if !needs.contains(.command("node")) { needs.append(.command("node")) }
            needs.append(.nodeModule("playwright"))
        }
    }
    return APScenarioRun(interpreter: argv, needs: needs, problem: problem)
}

// MARK: - 환경 확인

// 도구 찾기는 포트 APToolProbe(Ports.swift)

public struct APToolStatus: Equatable, Sendable {
    public var need: APToolNeed
    public var found: String?   // 찾은 자리(모듈은 "있음"), 없으면 nil
    public var ok: Bool { found != nil }
}

public struct APEnvRow: Equatable, Sendable {
    public var unit: Int
    public var file: String
    public var kind: APScenarioKind
    public var driver: APBrowserDriver?
    public var tools: [APToolStatus]
    public var problem: String?   // 돌리는 법을 모름
    public var ok: Bool { problem == nil && tools.allSatisfy(\.ok) }
}

public func apEnvCheck(unit n: Int, scenario s: APScenario, run: APScenarioRun, cwd: String, probe: APToolProbe) -> APEnvRow {
    let tools = run.needs.map { need -> APToolStatus in
        switch need {
        case .program(let p): return APToolStatus(need: need, found: probe.program(p) ? p : nil)
        case .command(let c): return APToolStatus(need: need, found: probe.command(c))
        case .browser: return APToolStatus(need: need, found: probe.browser())
        case .nodeModule(let m): return APToolStatus(need: need, found: probe.nodeModule(m, from: cwd) ? "있음" : nil)
        }
    }
    return APEnvRow(unit: n, file: s.file, kind: s.kind, driver: s.driver, tools: tools, problem: run.problem)
}

public func apKindLabel(_ k: APScenarioKind) -> String { k == .shell ? "셸" : "브라우저" }

// 환경 확인이 실패한 줄의 고칠 것 글(엔진 출발 전 확인의 고칠 것 = 출발 잠금, 준비 화면 실패 줄의 까닭). 통과면 nil
public func apEnvProblem(_ r: APEnvRow) -> String? {
    guard !r.ok else { return nil }
    let head = "단위 \(r.unit) 의 시나리오 \(r.file)(\(apKindLabel(r.kind)))"
    if let p = r.problem { return head + "를 돌리지 못합니다: " + p }
    let missing = r.tools.filter { !$0.ok }.map { apToolLabel($0.need) }
    return head + "를 돌릴 도구가 없습니다: " + missing.joined(separator: ", ") + " 을(를) 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 도구를 깐 뒤 다시 확인하거나 시나리오를 바꿉니다"
}

public func apEnvRowJSON(_ r: APEnvRow) -> [String: Any] {
    var o: [String: Any] = ["unit": r.unit, "file": r.file, "kind": r.kind.rawValue, "ok": r.ok,
                            "tools": r.tools.map { ["need": apToolLabel($0.need), "found": $0.found ?? NSNull()] as [String: Any] }]
    if let d = r.driver { o["driver"] = d.rawValue }
    if let p = apEnvProblem(r) { o["why"] = p }
    return o
}

// MARK: - 판정 차례와 결과

public struct APJudgeStep: Equatable, Sendable {
    public var name: String          // "scenario-1" … · "check"
    public var label: String         // 사람이 읽는 이름
    public var argv: [String]        // 돌릴 것(판정 명령이면 [셸, "-l", "-c", 명령])
    public var evidence: String      // 증거 폴더 안의 자리("" 는 맨 위 — 판정 명령, 옛 자리 그대로)
    public var kind: String          // "check" · "shell" · "browser"
    public var file: String?         // 시나리오 상대 경로
    public var criterion: String?
    public var pinned: String?       // 고정본 경로
    public var sha256: String?       // 고정본 지문(출발 때). 있으면 돌리기 전에 견준다
    public init(name: String, label: String, argv: [String], evidence: String, kind: String, file: String? = nil, criterion: String? = nil,
                pinned: String? = nil, sha256: String? = nil) {
        self.name = name; self.label = label; self.argv = argv; self.evidence = evidence; self.kind = kind; self.file = file
        self.criterion = criterion; self.pinned = pinned; self.sha256 = sha256
    }
}

// 고정본 하나: 시나리오·돌리는 법·고정본 경로·지문
public struct APPinnedScenario: Equatable, Sendable {
    public var scenario: APScenario
    public var run: APScenarioRun
    public var pinned: String
    public var sha256: String?
    public init(scenario: APScenario, run: APScenarioRun, pinned: String, sha256: String?) {
        self.scenario = scenario; self.run = run; self.pinned = pinned; self.sha256 = sha256
    }
}

public func apScenarioLabel(_ s: APScenario) -> String {
    "시나리오 \(s.file)(\(apKindLabel(s.kind))" + (s.criterion.map { " · 수용 기준 \($0)" } ?? "") + ")"
}

// 판정 차례: 시나리오들(고정본, 적은 차례) → 판정 명령(있으면). 판정 명령이 없고 시나리오도 없으면 빈 차례(엔진 출발 전 확인이 막는다)
public func apJudgeSteps(check: String?, shell: String, scenarios: [APPinnedScenario]) -> [APJudgeStep] {
    var out: [APJudgeStep] = []
    for (i, p) in scenarios.enumerated() {
        out.append(APJudgeStep(name: "scenario-\(i + 1)", label: apScenarioLabel(p.scenario), argv: p.run.interpreter + [p.pinned],
                               evidence: "scenario-\(i + 1)", kind: p.scenario.kind.rawValue, file: p.scenario.file,
                               criterion: p.scenario.criterion, pinned: p.pinned, sha256: p.sha256))
    }
    if let c = check, !c.isEmpty {
        out.append(APJudgeStep(name: "check", label: "판정 명령", argv: [shell, "-l", "-c", c], evidence: "", kind: "check"))
    }
    return out
}

public struct APStepResult: Equatable, Sendable {
    public var code: Int32
    public var sec: Double
    public var timedOut: Bool
    public var interrupted: Bool
    public var tail: String
    public var files: [String]
    public var note: String?   // 돌리지 않은 까닭(보안 등)
    public init(code: Int32, sec: Double = 0, timedOut: Bool = false, interrupted: Bool = false, tail: String = "", files: [String] = [], note: String? = nil) {
        self.code = code; self.sec = sec; self.timedOut = timedOut; self.interrupted = interrupted; self.tail = tail; self.files = files; self.note = note
    }
}

// 걸음 돌리기는 포트 APJudgeRunner(Ports.swift)

public struct APJudgeOutcome: Equatable, Sendable {
    public var steps: [APJudgeStep]
    public var results: [APStepResult]
    public init(steps: [APJudgeStep], results: [APStepResult]) { self.steps = steps; self.results = results }
    public var passed: Bool { !interrupted && !results.isEmpty && results.count == steps.count && results.allSatisfy { $0.code == 0 && !$0.timedOut } }
    public var interrupted: Bool { results.contains { $0.interrupted } }
    public var timedOut: Bool { results.contains { $0.timedOut } }
    /// 실패로 볼 종료 코드: 처음 실패한 걸음의 것(통과면 0)
    public var code: Int32 { passed ? 0 : (results.first { $0.code != 0 || $0.timedOut }?.code ?? -1) }
    /// 모델에게 넘기는 출력 끝: 걸음 하나(옛 판정 명령만)면 그 출력 그대로, 여럿이면 실패한 걸음마다 머리 줄과 끝 20줄
    public var tail: String {
        if steps.count == 1, let r = results.first { return r.tail }
        let failed = zip(steps, results).filter { $0.1.code != 0 || $0.1.timedOut }
        return failed.map { s, r in
            let head = "[\(s.label)] " + (r.note ?? (r.timedOut ? "시간 상한에 닿음" : "종료 코드 \(r.code)"))
            let t = r.tail.split(separator: "\n", omittingEmptySubsequences: false).suffix(20).joined(separator: "\n")
            return t.isEmpty ? head : head + "\n" + t
        }.joined(separator: "\n\n")
    }
    /// 모델·진행 기록에 쓰는 판정 이름: 걸음 하나면 그 명령, 여럿이면 "시나리오 n개 + 판정 명령"
    public func describe(check: String?) -> String {
        let n = steps.filter { $0.kind != "check" }.count
        if n == 0 { return check ?? "" }
        return "시나리오 \(n)개" + ((check ?? "").isEmpty ? "" : " + 판정 명령 `\(check!)`")
    }
}

// 판정을 돌린다: 차례대로, 일시정지·중단·상한(interrupted)이면 거기서 멈춘다. 실패해도 나머지를 돌려 무엇이 실패했는지 모두 남긴다.
// 고정본 지문이 출발 때와 다르면 그 걸음은 돌리지 않고 실패(보안)
public func apRunJudge(_ steps: [APJudgeStep], runner: APJudgeRunner) -> APJudgeOutcome {
    var results: [APStepResult] = []
    for s in steps {
        if let want = s.sha256, let p = s.pinned, runner.digest(p) != want {
            results.append(APStepResult(code: -1, tail: "보안: 판정 시나리오 고정본이 출발 때와 달라 돌리지 않았습니다(\(s.file ?? p))",
                                        note: "보안: 고정본이 출발 때와 달라 돌리지 않음"))
            continue
        }
        let r = runner.run(s)
        results.append(r)
        if r.interrupted { break }
    }
    return APJudgeOutcome(steps: steps, results: results)
}

private let imageExt: Set<String> = ["png", "jpg", "jpeg", "webp"]

// 증거 요약(증거 폴더 summary.json, engine.json checks[] 의 parts). 브라우저 걸음은 남긴 그림 파일을 screenshots 로, 없으면 note
public func apJudgeSummary(_ o: APJudgeOutcome, sandbox: String, at: Double) -> [String: Any] {
    var parts: [[String: Any]] = []
    for (s, r) in zip(o.steps, o.results) {
        var p: [String: Any] = ["name": s.name, "label": s.label, "kind": s.kind, "exit": Int(r.code), "sec": (r.sec * 1000).rounded() / 1000,
                                "timed_out": r.timedOut, "interrupted": r.interrupted, "passed": r.code == 0 && !r.timedOut,
                                "evidence": s.evidence, "files": r.files]
        if let f = s.file { p["file"] = f }
        if let c = s.criterion { p["criterion"] = c }
        if let pin = s.pinned { p["pinned"] = pin }
        if let h = s.sha256 { p["sha256"] = h }
        if let n = r.note { p["note"] = n }
        if s.kind == "browser" {
            let shots = r.files.filter { imageExt.contains(($0 as NSString).pathExtension.lowercased()) }
            p["screenshots"] = shots
            if shots.isEmpty, r.note == nil { p["note"] = "브라우저 시나리오가 스크린샷을 남기지 않았습니다" }
        }
        parts.append(p)
    }
    return ["at": (at * 1000).rounded() / 1000, "passed": o.passed, "sandbox": sandbox, "parts": parts]
}

// MARK: - 판정 미리 돌리기의 시나리오 카드

// 단위 전체는 실패했지만 시나리오 하나가 일하기 전부터 통과(이미 통과)했을 때의 출발 카드 감. 단위 전체가 통과하면 apPrerunCard(단위 카드)를 쓴다.
// 추천은 판정 고치기(그 시나리오가 뜻을 보지 않을 수 있다. 설계 2장 ① 반대 탐침 카드와 같은 꼴)
public func apPrerunScenarioCard(n: Int, id: String, title: String, file: String, criterion: String?) -> [String: Any] {
    ["id": "prerun-\(id)-\(file)", "unit": n, "unit_id": id, "kind": "prerun", "scenario": file,
     "title": "「\(title)」 의 시나리오 \(file)\(criterion.map { "(수용 기준 \($0))" } ?? "") 가 일하기 전부터 통과합니다",
     "why": "그 시나리오가 일한 결과의 뜻을 보지 않거나 이미 된 기준이라는 뜻입니다. 그대로 두면 그 기준은 일하지 않아도 통과로 남습니다.",
     "options": [["key": "fix_check", "label": "판정 고치기"], ["key": "keep", "label": "그대로"], ["key": "drop", "label": "빼기"]],
     "recommended": "fix_check"]
}
