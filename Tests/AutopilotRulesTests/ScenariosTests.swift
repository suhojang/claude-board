// 시험 실행기 규칙(Sources/AutopilotRules/Scenarios.swift·JudgeSandbox.swift, 5단계 ⑪-1) 표 시험. 가짜 어댑터(도구 찾기·판정 돌리기)는 이 파일에만 둔다
import Foundation
import Testing
import AutopilotRules

// MARK: 대기열 칸

struct ScenarioParseRow: Row {
    let note: String
    let json: String
    var raw: [Any] { (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [Any] ?? [] }
    let files: [String]
    let problems: Int
}

private let parseRows: [ScenarioParseRow] = [
    .init(note: "글자 하나는 셸 시나리오", json: #"["t/a.sh"]"#, files: ["t/a.sh"], problems: 0),
    .init(note: "브라우저·기준·드라이버", json: #"[{"file": "t/p.mjs", "kind": "browser", "criterion": "AC-2", "driver": "playwright"}]"#, files: ["t/p.mjs"], problems: 0),
    .init(note: "절대 경로는 받지 않음", json: #"["/etc/passwd"]"#, files: [], problems: 1),
    .init(note: "..·~·빈 조각·따옴표·. 조각은 받지 않음", json: #"["../x.sh", "~/x.sh", "a//b.sh", "a\"b.sh", "./a.sh"]"#, files: [], problems: 5),
    .init(note: "kind 꼴", json: #"[{"file": "a.sh", "kind": "gui"}]"#, files: [], problems: 1),
    .init(note: "driver 는 브라우저만", json: #"[{"file": "a.sh", "driver": "chrome"}]"#, files: [], problems: 1),
    .init(note: "criterion 꼴", json: #"[{"file": "a.sh", "criterion": "기준 1"}]"#, files: [], problems: 1),
    .init(note: "같은 파일 두 번", json: #"["a.sh", {"file": "a.sh"}]"#, files: ["a.sh"], problems: 1),
    .init(note: "상한 30개", json: "[" + (1...31).map { "\"s\($0).sh\"" }.joined(separator: ",") + "]", files: (1...30).map { "s\($0).sh" }, problems: 1),
]

// MARK: 가짜 어댑터

private struct FakeProbe: APToolProbe {
    var programs: Set<String> = ["/bin/sh", "/bin/bash", "/usr/bin/env"]
    var commands: [String: String] = ["python3": "/opt/homebrew/bin/python3", "node": "/opt/homebrew/bin/node"]
    var browserPath: String? = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    var modules: Set<String> = []
    func program(_ path: String) -> Bool { programs.contains(path) }
    func command(_ name: String) -> String? { commands[name] }
    func browser() -> String? { browserPath }
    func nodeModule(_ name: String, from dir: String) -> Bool { modules.contains(name) }
}

private final class FakeRunner: APJudgeRunner {
    var codes: [String: Int32]
    var digests: [String: String] = [:]
    var interruptAt: String?
    var ran: [String] = []
    init(_ codes: [String: Int32]) { self.codes = codes }
    func digest(_ path: String) -> String? { digests[path] }
    func run(_ step: APJudgeStep) -> APStepResult {
        ran.append(step.name)
        if step.name == interruptAt { return APStepResult(code: -1, interrupted: true) }
        let c = codes[step.name] ?? 0
        return APStepResult(code: c, sec: 1.25, tail: "\(step.name) 출력\n끝 줄", files: step.kind == "browser" && c == 0 ? ["shot.png", "dom.html"] : [])
    }
}

private func pinned(_ file: String, kind: APScenarioKind = .shell, crit: String? = nil, sha: String? = nil, head: String = "#!/bin/sh\n") -> APPinnedScenario {
    let s = APScenario(file: file, kind: kind, criterion: crit)
    return APPinnedScenario(scenario: s, run: apScenarioRun(s, head: head), pinned: "/R/pinned/checks/scenarios/1/\(file.split(separator: "/").last!)", sha256: sha)
}

@Suite("자율주행 시험 실행기: 시나리오·수용 기준") struct ScenarioRuleTests {
    @Test("scenarios 칸 읽기", arguments: parseRows)
    func 읽기(_ r: ScenarioParseRow) {
        let (items, problems) = apParseScenarios(r.raw, unit: 2)
        #expect(items.map(\.file) == r.files)
        #expect(problems.count == r.problems)
        #expect(problems.allSatisfy { $0.hasPrefix("단위 2 의 ") })
    }

    @Test("배열이 아니면 고칠 것, 없으면 빈 것")
    func 배열() {
        #expect(apParseScenarios("a.sh", unit: 1).problems == ["단위 1 의 scenarios 는 배열이어야 합니다"])
        #expect(apParseScenarios(nil, unit: 1).items.isEmpty)
        #expect(apParseScenarios([["file": "p.js", "kind": "browser"]], unit: 1).items.first?.driver == .chrome)
    }

    @Test("수용 기준 읽기·짝 없는 기준·카드 답")
    func 수용기준() {
        let (cs, ps) = apParseCriteria([["id": "AC-1", "text": "예약을 만들면 201"], ["id": "AC-2", "text": " 겹치면 409 "], ["id": "AC-2", "text": "x"], ["text": "id 없음"], ["id": "AC-3", "text": ""]], unit: 1)
        #expect(cs == [APCriterion(id: "AC-1", text: "예약을 만들면 201"), APCriterion(id: "AC-2", text: "겹치면 409")])
        #expect(ps == ["단위 1 의 수용 기준 AC-2 가 두 번 있습니다", "단위 1 의 수용 기준 4 에 id(영문·숫자·.-_)가 없습니다", "단위 1 의 수용 기준 AC-3 에 text 가 없습니다"])
        let ss = [APScenario(file: "a.sh", criterion: "AC-1"), APScenario(file: "b.sh", criterion: "AC-9")]
        #expect(apCriteriaGaps(cs, ss) == [APCriterion(id: "AC-2", text: "겹치면 409")])
        #expect(apScenarioCriterionProblems(ss, cs, unit: 1) == ["단위 1 의 시나리오 b.sh 가 가리킨 수용 기준 AC-9 가 criteria 에 없습니다"])
        #expect(apScenarioCriterionProblems(ss, [], unit: 1).isEmpty)   // 기준을 적지 않은 단위는 이름표로만
        #expect(apCriterionAnswer(cs[0], scenarios: ss, answer: "drop_criterion") == ("scenario", false))
        #expect(apCriterionAnswer(cs[1], scenarios: ss, answer: "model_judge") == ("model_judge", false))
        #expect(apCriterionAnswer(cs[1], scenarios: ss, answer: nil) == ("drop_criterion", true))
        #expect(apCriterionAnswer(cs[1], scenarios: ss, answer: "keep") == ("drop_criterion", true))
        let card = apCriterionCard(unitTitle: "예약 API", criterion: cs[1])
        #expect(card.recommended == "drop_criterion")
        #expect(card.options.map(\.key) == ["drop_criterion", "model_judge"])
        #expect(!(card.title + card.why + card.options.map { $0.label + $0.note }.joined()).contains("—"))
        #expect(apCriterionCardId(unit: "u2", criterion: "AC-2") == "criterion-u2-AC-2")
    }
}

// MARK: 돌리는 법·환경 확인

struct InterpreterRow: Row {
    let note: String
    let file: String
    var kind: APScenarioKind = .shell
    var driver: APBrowserDriver? = nil
    let head: String
    let argv: [String]
    let needs: [APToolNeed]
    var problem = false
}

private let interpreterRows: [InterpreterRow] = [
    .init(note: "#! 절대 경로", file: "a", head: "#!/bin/bash -e\necho", argv: ["/bin/bash", "-e"], needs: [.program("/bin/bash")]),
    .init(note: "#! env 는 다음 이름을 PATH 에서", file: "a", head: "#!/usr/bin/env python3\n", argv: ["/usr/bin/env", "python3"], needs: [.program("/usr/bin/env"), .command("python3")]),
    .init(note: "#! env -S", file: "a", head: "#!/usr/bin/env -S node --no-warnings\n", argv: ["/usr/bin/env", "-S", "node", "--no-warnings"], needs: [.program("/usr/bin/env"), .command("node")]),
    .init(note: "#! 상대 해석기는 받지 않음", file: "a.sh", head: "#!bash\n", argv: [], needs: [], problem: true),
    .init(note: "확장자 .sh", file: "t/a.sh", head: "curl …", argv: ["/bin/sh"], needs: [.program("/bin/sh")]),
    .init(note: "확장자 .mjs", file: "t/a.mjs", head: "import x", argv: ["/usr/bin/env", "node"], needs: [.program("/usr/bin/env"), .command("node")]),
    .init(note: "모르는 확장자", file: "t/a.rb", head: "puts 1", argv: [], needs: [], problem: true),
    .init(note: "브라우저 chrome", file: "p.sh", kind: .browser, head: "#!/bin/sh\n", argv: ["/bin/sh"], needs: [.program("/bin/sh"), .browser]),
    .init(note: "브라우저 playwright", file: "p.mjs", kind: .browser, driver: .playwright, head: "import { chromium } from 'playwright'",
          argv: ["/usr/bin/env", "node"], needs: [.program("/usr/bin/env"), .command("node"), .nodeModule("playwright")]),
]

@Suite("자율주행 시험 실행기: 돌리는 법·환경 확인") struct ScenarioEnvTests {
    @Test("해석기와 필요한 도구", arguments: interpreterRows)
    func 해석기(_ r: InterpreterRow) {
        let x = apScenarioRun(APScenario(file: r.file, kind: r.kind, driver: r.driver), head: r.head)
        #expect(x.interpreter == r.argv)
        #expect(x.needs == r.needs)
        #expect((x.problem != nil) == r.problem)
    }

    @Test("환경 확인: 있으면 통과, 없으면 실패 줄과 까닭(깔지 않음)")
    func 환경() {
        let shell = APScenario(file: "t/a.py")
        let ok = apEnvCheck(unit: 1, scenario: shell, run: apScenarioRun(shell, head: ""), cwd: "/w", probe: FakeProbe())
        #expect(ok.ok && apEnvProblem(ok) == nil)
        #expect(ok.tools.map(\.found) == ["/usr/bin/env", "/opt/homebrew/bin/python3"])

        let pw = APScenario(file: "t/p.mjs", kind: .browser, driver: .playwright)
        let miss = apEnvCheck(unit: 2, scenario: pw, run: apScenarioRun(pw, head: ""), cwd: "/w", probe: FakeProbe())
        #expect(!miss.ok)
        #expect(apEnvProblem(miss) == "단위 2 의 시나리오 t/p.mjs(브라우저)를 돌릴 도구가 없습니다: node 모듈 playwright 을(를) 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 도구를 깐 뒤 다시 확인하거나 시나리오를 바꿉니다")

        let ch = APScenario(file: "t/p.sh", kind: .browser)
        var none = FakeProbe(); none.browserPath = nil
        let nb = apEnvCheck(unit: 3, scenario: ch, run: apScenarioRun(ch, head: "#!/bin/sh"), cwd: "/w", probe: none)
        #expect(apEnvProblem(nb)?.contains("브라우저(Google Chrome·Chromium·Microsoft Edge) 을(를) 찾지 못했습니다") == true)
        let j = apEnvRowJSON(nb)
        #expect(j["ok"] as? Bool == false && j["driver"] as? String == "chrome" && (j["why"] as? String)?.isEmpty == false)

        let rb = APScenario(file: "a.rb")
        let unk = apEnvCheck(unit: 4, scenario: rb, run: apScenarioRun(rb, head: ""), cwd: "/w", probe: FakeProbe())
        #expect(apEnvProblem(unk)?.hasPrefix("단위 4 의 시나리오 a.rb(셸)를 돌리지 못합니다: 돌리는 법을 모릅니다") == true)
        for r in [miss, nb, unk] { #expect(apEnvProblem(r)?.contains("—") == false) }
    }
}

// MARK: 판정 차례와 결과

@Suite("자율주행 시험 실행기: 판정 차례와 결과") struct JudgeRunTests {
    @Test("옛 대기열(판정 명령만): 걸음 하나, 종료 코드·출력 그대로")
    func 옛판정() {
        let steps = apJudgeSteps(check: "test -f a.txt", shell: "/bin/zsh", scenarios: [])
        #expect(steps.map(\.argv) == [["/bin/zsh", "-l", "-c", "test -f a.txt"]])
        #expect(steps.first?.evidence == "")
        let r = FakeRunner(["check": 3])
        let o = apRunJudge(steps, runner: r)
        #expect(!o.passed && o.code == 3 && o.tail == "check 출력\n끝 줄")
        #expect(o.describe(check: "test -f a.txt") == "test -f a.txt")
        #expect(apRunJudge(steps, runner: FakeRunner([:])).passed)
    }

    @Test("시나리오 → 판정 명령 차례, 하나라도 실패면 실패, 실패한 걸음마다 머리 줄")
    func 여럿() {
        let steps = apJudgeSteps(check: "true", shell: "/bin/sh", scenarios: [pinned("s/a.sh", crit: "AC-1"), pinned("s/b.sh", kind: .browser)])
        #expect(steps.map(\.name) == ["scenario-1", "scenario-2", "check"])
        #expect(steps[0].argv == ["/bin/sh", "/R/pinned/checks/scenarios/1/a.sh"])
        #expect(steps[0].label == "시나리오 s/a.sh(셸 · 수용 기준 AC-1)")
        let r = FakeRunner(["scenario-1": 1])
        let o = apRunJudge(steps, runner: r)
        #expect(r.ran == ["scenario-1", "scenario-2", "check"])   // 실패해도 나머지를 돌린다
        #expect(!o.passed && o.code == 1)
        #expect(o.tail == "[시나리오 s/a.sh(셸 · 수용 기준 AC-1)] 종료 코드 1\nscenario-1 출력\n끝 줄")
        #expect(o.describe(check: "true") == "시나리오 2개 + 판정 명령 `true`")
        #expect(apRunJudge(steps, runner: FakeRunner([:])).passed)
    }

    @Test("일시정지·중단(interrupted)이면 거기서 멈춤")
    func 멈춤() {
        let steps = apJudgeSteps(check: "true", shell: "/bin/sh", scenarios: [pinned("a.sh"), pinned("b.sh")])
        let r = FakeRunner([:]); r.interruptAt = "scenario-1"
        let o = apRunJudge(steps, runner: r)
        #expect(r.ran == ["scenario-1"] && o.interrupted && !o.passed)
    }

    @Test("고정본 지문이 출발 때와 다르면 돌리지 않고 실패(보안)")
    func 지문() {
        let steps = apJudgeSteps(check: nil, shell: "/bin/sh", scenarios: [pinned("a.sh", sha: "aaa"), pinned("b.sh", sha: "bbb")])
        let r = FakeRunner([:]); r.digests = ["/R/pinned/checks/scenarios/1/a.sh": "aaa", "/R/pinned/checks/scenarios/1/b.sh": "바뀜"]
        let o = apRunJudge(steps, runner: r)
        #expect(r.ran == ["scenario-1"])
        #expect(!o.passed && o.code == -1)
        #expect(o.tail.hasPrefix("[시나리오 b.sh(셸)] 보안: 고정본이 출발 때와 달라 돌리지 않음\n보안: 판정 시나리오 고정본이 출발 때와 달라 돌리지 않았습니다(b.sh)"))
    }

    @Test("증거 요약: 걸음마다 종료 코드·시간·파일, 브라우저는 스크린샷 또는 없다는 글")
    func 요약() {
        let steps = apJudgeSteps(check: nil, shell: "/bin/sh", scenarios: [pinned("p.sh", kind: .browser), pinned("q.sh", kind: .browser)])
        let o = apRunJudge(steps, runner: FakeRunner(["scenario-2": 2]))
        let s = apJudgeSummary(o, sandbox: "샌드박스 안", at: 10.12345)
        #expect(s["passed"] as? Bool == false && s["at"] as? Double == 10.123 && s["sandbox"] as? String == "샌드박스 안")
        let parts = s["parts"] as? [[String: Any]] ?? []
        #expect(parts.count == 2)
        #expect(parts[0]["screenshots"] as? [String] == ["shot.png"] && parts[0]["passed"] as? Bool == true && parts[0]["sec"] as? Double == 1.25)
        #expect(parts[1]["screenshots"] as? [String] == [] && parts[1]["note"] as? String == "브라우저 시나리오가 스크린샷을 남기지 않았습니다")
        #expect(parts[1]["exit"] as? Int == 2 && parts[1]["evidence"] as? String == "scenario-2")
    }

    @Test("미리 돌리기 시나리오 카드: 추천은 판정 고치기")
    func 카드() {
        let c = apPrerunScenarioCard(n: 2, id: "u2", title: "예약 API", file: "s/a.sh", criterion: "AC-1")
        #expect(c["id"] as? String == "prerun-u2-s/a.sh" && c["recommended"] as? String == "fix_check" && c["scenario"] as? String == "s/a.sh")
        #expect(c["unit_id"] as? String == "u2" && c["unit"] as? Int == 2)
        #expect(c["title"] as? String == "「예약 API」 의 시나리오 s/a.sh(수용 기준 AC-1) 가 일하기 전부터 통과합니다")
    }
}

// MARK: 판정 샌드박스 프로필

@Suite("자율주행 판정 샌드박스") struct JudgeSandboxTests {
    @Test("거르개: 폴더는 subpath, 글롭은 정규식(조각 하나), 나타낼 수 없는 경로는 nil")
    func 거르개() {
        #expect(apSBPLFilter("/Users/a/Library/Application Support/Claude Board/autopilot/") == "(subpath \"/Users/a/Library/Application Support/Claude Board/autopilot\")")
        #expect(apSBPLFilter("/R/autopilot/runs/*/engine") == "(regex #\"^/R/autopilot/runs/[^/]+/engine(/|$)\")")
        #expect(apSBPLFilter("/R/a.b (1)/*") == "(regex #\"^/R/a\\.b \\(1\\)/[^/]+(/|$)\")")
        #expect(apSBPLFilter("relative") == nil && apSBPLFilter("/a\"b") == nil && apSBPLFilter("/a\nb") == nil)
    }

    @Test("프로필 골든: IPC 막기 · 쓰기 차례(모두 막기 → 열기 → 다시 막기 → 증거 폴더) · 읽기 금지 · 정한 포트만 · socket")
    func 골든() {
        let s = APJudgeSandbox(write: ["/W/repo", "/Users/a/.gradle", "/private/var/folders/x/T/claude-board-judge-r-1"],
                               denyWrite: ["/Users/a/Library/Application Support/Claude Board Dev/autopilot", "/Users/a/.claude", "/W/repo/.git/hooks", "/W/repo/s/a.sh", "bad"],
                               evidence: "/Users/a/Library/Application Support/Claude Board Dev/autopilot/runs/r/units/1/evidence/check-0",
                               denyRead: ["/Users/a/Library/Keychains", "/R/runs/*/engine"],
                               socketDirs: ["/private/var/folders/x/T/claude-board-judge-r-1"], unixSockets: ["/var/run/docker.sock"],
                               network: .ports(out: [5173, 18431], bind: [18431]))
        let (text, skipped) = apJudgeProfile(s)
        #expect(skipped == ["bad"])
        #expect(text == """
        ;; Claude Board 자율주행 판정 샌드박스(엔진이 만든다, 고치지 않는다)
        (version 1)
        (allow default)
        ;; IPC: 앱 열기·AppleEvent 는 막고, mach 서비스는 기본으로 막은 뒤 필요한 것만
        (deny lsopen)
        (deny appleevent-send)
        (deny mach-lookup)
        (allow mach-lookup
          (global-name "com.apple.system.opendirectoryd.libinfo")
          (global-name "com.apple.system.opendirectoryd.membership")
          (global-name "com.apple.bsd.dirhelper")
          (global-name "com.apple.system.notification_center")
          (global-name "com.apple.system.logger")
          (global-name "com.apple.logd")
          (global-name "com.apple.diagnosticd")
          (global-name "com.apple.cfprefsd.daemon")
          (global-name "com.apple.cfprefsd.agent")
          (global-name "com.apple.SystemConfiguration.configd")
          (global-name "com.apple.SystemConfiguration.DNSConfiguration")
          (global-name "com.apple.trustd")
          (global-name "com.apple.trustd.agent")
          (global-name "com.apple.timed.xpc")
          (global-name "com.apple.fonts")
          (global-name "com.apple.FontObjectsServer"))
        ;; 쓰기: 모두 막고 정한 곳만 연다
        (deny file-write* (subpath "/"))
        (allow file-write*
          (literal "/dev/null")
          (literal "/dev/zero")
          (literal "/dev/tty")
          (literal "/dev/dtracehelper")
          (literal "/dev/random")
          (literal "/dev/urandom")
          (literal "/dev/stdout")
          (literal "/dev/stderr")
          (regex #"^/dev/fd/[0-9]+$")
          (regex #"^/dev/ttys[0-9]+$"))
        (allow file-write*
          (subpath "/W/repo")
          (subpath "/Users/a/.gradle")
          (subpath "/private/var/folders/x/T/claude-board-judge-r-1"))
        ;; 쓰기 범위 안에서도 막는 곳(자율주행 뿌리·하네스 자리·다른 가지·시나리오 원본 등)
        (deny file-write*
          (subpath "/Users/a/Library/Application Support/Claude Board Dev/autopilot")
          (subpath "/Users/a/.claude")
          (subpath "/W/repo/.git/hooks")
          (subpath "/W/repo/s/a.sh"))
        ;; 이번 증거 폴더(뒤 규칙이 이긴다)
        (allow file-write*
          (subpath "/Users/a/Library/Application Support/Claude Board Dev/autopilot/runs/r/units/1/evidence/check-0"))
        ;; 읽기 금지(키체인·엔진 전용 자리·올릴 글 등)
        (deny file-read*
          (subpath "/Users/a/Library/Keychains")
          (regex #"^/R/runs/[^/]+/engine(/|$)"))
        ;; 네트워크: localhost 의 정한 포트만(나가기 5173,18431, 묶기·받기 18431)
        (deny network*)
        (allow network-outbound
          (remote ip "localhost:5173")
          (remote ip "localhost:18431"))
        (allow network-bind network-inbound
          (local ip "localhost:18431"))
        ;; unix socket: 판정 임시 폴더·증거 폴더 안만(브라우저의 SingletonSocket 등)
        (allow network-bind network-outbound network-inbound
          (local unix-socket (subpath "/private/var/folders/x/T/claude-board-judge-r-1"))
          (remote unix-socket (subpath "/private/var/folders/x/T/claude-board-judge-r-1")))
        ;; 사람이 정한 unix socket(check_unix_sockets)
        (allow network-outbound
          (remote unix-socket (path-literal "/var/run/docker.sock")))

        """)
    }

    @Test("증거 폴더 열기는 쓰기 금지보다 뒤에 온다(뒤 규칙이 이긴다)")
    func 차례() {
        let (t, _) = apJudgeProfile(APJudgeSandbox(write: ["/W"], denyWrite: ["/R"], evidence: "/R/ev", denyRead: [], socketDirs: []))
        let deny = t.range(of: "(deny file-write*\n  (subpath \"/R\"))")!.lowerBound
        let ev = t.range(of: "(subpath \"/R/ev\")")!.lowerBound
        #expect(deny < ev)
        #expect(!t.contains("unix-socket"))
        #expect(!apJudgeSandboxNote(on: true).contains("—") && !apJudgeSandboxNote(on: false).contains("—"))
    }

    @Test("⑪-1 보안 검토 2: 판정 네트워크는 실행 설정에서(기본 없음), localhost 를 통째로 열지 않음")
    func 네트워크() {
        #expect(apJudgeNet(networkAllow: ["repo.maven.apache.org", "services.gradle.org"], localBinding: false, checkPorts: []) == .none)
        #expect(apJudgeNet(networkAllow: ["localhost:5173", "127.0.0.1:8080", "[::1]:9", "localhost:x", "localhost:70000"], localBinding: false, checkPorts: [18431, 0])
                == .ports(out: [9, 5173, 8080, 18431], bind: [18431]))
        #expect(apJudgeNet(networkAllow: [], localBinding: true, checkPorts: [1]) == .allLocal)
        let none = apJudgeProfile(APJudgeSandbox(write: ["/W"], denyWrite: [], evidence: "/E", denyRead: [], socketDirs: [])).text
        #expect(none.contains(";; 네트워크: 없음") && none.contains("(deny network*)\n") && !none.contains("localhost"))
        let all = apJudgeProfile(APJudgeSandbox(write: ["/W"], denyWrite: [], evidence: "/E", denyRead: [], socketDirs: [], network: .allLocal)).text
        #expect(all.contains("(allow network-outbound (remote ip \"localhost:*\"))"))
        #expect(apJudgeSandboxNote(on: true, network: .none).contains("네트워크: 없음"))
    }

    @Test("⑪-1 보안 검토 1: 앱 열기·AppleEvent 막기, mach 는 기본 막고 목록만, 브라우저 걸음만 조금 더")
    func ipc() {
        let shell = apJudgeProfile(APJudgeSandbox(write: ["/W"], denyWrite: [], evidence: "/E", denyRead: [], socketDirs: [])).text
        for x in ["(deny lsopen)", "(deny appleevent-send)", "(deny mach-lookup)"] { #expect(shell.contains(x)) }
        #expect(shell.range(of: "(deny mach-lookup)")!.lowerBound < shell.range(of: "(allow mach-lookup")!.lowerBound)
        for x in ["launchservicesd", "lsd.", "SecurityServer", "securityd", "distributed_notifications", "pasteboard", "tccd", "windowserver"] { #expect(!shell.contains(x)) }
        let br = apJudgeProfile(APJudgeSandbox(write: ["/W"], denyWrite: [], evidence: "/E", denyRead: [], socketDirs: [], browser: true)).text
        #expect(br.contains("(global-name \"com.apple.coreservices.launchservicesd\")") && br.contains("(global-name-prefix \"com.google.Chrome.MachPortRendezvousServer.\")"))
        #expect(br.contains("(deny lsopen)") && !br.contains("lsd.") && !br.contains("securityd"))
    }
}
