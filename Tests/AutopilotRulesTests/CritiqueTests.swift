// 판정 비판 일반판 규칙(Sources/AutopilotRules/Critique.swift·Misses.swift·Records.swift, 5단계 ⑪-3) 표 시험.
// 가짜 어댑터(버리는 작업 트리·비판 결과 폴더)는 이 파일에만 둔다. 값은 장난감(zz-toy·example.com).
// 가짜 작업 트리는 파일 사전이고, 가짜 판정은 걸음마다 "그 파일 사전으로 통과하는가"를 정한 함수다(형식만 보는 판정·뜻을 보는 판정)
import Foundation
import SQLite3
import Testing
import AutopilotRules

// MARK: 가짜 어댑터

private final class FakeTree: APProbeWorkspace {
    var base: [String: String]                       // 출발 기준의 파일
    var files: [String: String] = [:]
    var resets = 0, judged: [String] = []
    var putFail: String?                             // 이 경로에 쓰면 실패
    var steps: [APJudgeStep]
    var verdict: ([String: String]) -> [String: Bool]  // 걸음 이름 → 통과
    var sec = 1.0
    init(base: [String: String] = [:], steps: [APJudgeStep], verdict: @escaping ([String: String]) -> [String: Bool]) {
        self.base = base; self.files = base; self.steps = steps; self.verdict = verdict
    }
    func reset(unit n: Int) -> String? { resets += 1; files = base; return nil }
    func put(unit n: Int, _ relative: String, _ data: Data) -> String? {
        if relative == putFail { return "링크라 쓰지 않았습니다" }
        files[relative] = String(decoding: data, as: UTF8.self); return nil
    }
    func judge(unit n: Int, label: String) -> APJudgeOutcome {
        judged.append(label)
        let v = verdict(files)
        return APJudgeOutcome(steps: steps, results: steps.map { APStepResult(code: v[$0.name] == true ? 0 : 1, sec: sec) })
    }
}

private struct FakeOut: APDraftFiles {
    var files: [String: String]
    func read(_ relative: String) -> Data? { files[relative].map { Data($0.utf8) } }
    func list(_ relative: String) -> [String] { [] }
}

private func step(_ name: String, file: String? = nil, crit: String? = nil, kind: String = "shell") -> APJudgeStep {
    APJudgeStep(name: name, label: file.map { "시나리오 \($0)" } ?? "판정 명령", argv: [], evidence: name, kind: kind, file: file, criterion: crit)
}

// 견본 초안(docs/자율주행/시험 도구/견본 초안)의 형식만 보는 시나리오 둘과 같은 꼴
private let listSh = """
#!/bin/sh
# AC-2: GET /todos 가 있다
test -f app/todos.py || { echo "app/todos.py 가 없습니다"; exit 1; }
grep -q "GET /todos" app/todos.py
"""
private let routesPy = """
#!/usr/bin/env python3
# AC-1: POST /todos 가 있다(장난감: app/todos.py 의 ROUTES 를 본다)
import pathlib, sys
p = pathlib.Path("app/todos.py")
if not p.exists():
    print("app/todos.py 가 없습니다"); sys.exit(1)
sys.exit(0 if "POST /todos" in p.read_text() else 1)
"""
private let toyView = APJudgeView(unit: 1, unitId: "u1", title: "할 일 만들기·목록 API", check: "",
                                  scenarios: [APJudgeText(file: ".autopilot/scenarios/u1/routes.py", text: routesPy),
                                              APJudgeText(file: ".autopilot/scenarios/u1/list.sh", text: listSh)])
private let toySteps = [step("scenario-1", file: ".autopilot/scenarios/u1/routes.py", crit: "AC-1"), step("scenario-2", file: ".autopilot/scenarios/u1/list.sh", crit: "AC-2")]
// 형식만 보는 판정: 글자만 있으면 통과
private func weakVerdict(_ f: [String: String]) -> [String: Bool] {
    let t = f["app/todos.py"]
    return ["scenario-1": t?.contains("POST /todos") ?? false, "scenario-2": t?.contains("GET /todos") ?? false]
}
// 뜻을 보는 판정: 진짜로 ROUTES 목록이 그 경로를 담아야 통과(장난감: 정확한 구현 글만)
private let realImpl = "ROUTES = ['POST /todos', 'GET /todos']\n"
private func strongVerdict(_ f: [String: String]) -> [String: Bool] {
    let ok = f["app/todos.py"] == realImpl
    return ["scenario-1": ok, "scenario-2": ok]
}
private func failedBaseline(_ steps: [APJudgeStep], passing: Set<String> = []) -> APJudgeOutcome {
    APJudgeOutcome(steps: steps, results: steps.map { APStepResult(code: passing.contains($0.name) ? 0 : 1) })
}

@Suite("자율주행 판정 비판: 판정이 보는 자리") struct JudgePlaceTests {
    struct PathRow: CustomTestStringConvertible, Sendable {
        let text: String, paths: [String]
        var testDescription: String { text }
    }
    static let pathRows: [PathRow] = [
        .init(text: "test -f app/todos.py || exit 1", paths: ["app/todos.py"]),
        .init(text: "grep -q '할 일' NOTES.md", paths: ["NOTES.md"]),
        .init(text: "curl -s http://127.0.0.1:18431/index.html | grep -q ok", paths: []),
        .init(text: "p = pathlib.Path(\"out/data.json\")", paths: ["out/data.json"]),
        .init(text: "cat ../other/a.txt /etc/hosts.txt ~/x.md", paths: []),
        .init(text: "test -s done && test -e build/out", paths: ["done", "build/out"]),
        .init(text: "python3 tests/check.py && test -f result.csv", paths: ["tests/check.py", "result.csv"]),
    ]
    @Test("글 안의 상대 경로 꼴", arguments: pathRows)
    func 경로(_ r: PathRow) { #expect(apJudgePaths(r.text) == r.paths) }

    @Test("판정 명령이 돌리는 스크립트(판정 자체)")
    func 스크립트() {
        #expect(apJudgeRunScripts("python3 tests/check.py && test -f out.json") == ["tests/check.py"])
        #expect(apJudgeRunScripts("sh ./run.sh") == ["run.sh"])
        #expect(apJudgeRunScripts("./check.sh; node -e 1") == ["check.sh"])
        #expect(apJudgeRunScripts("grep -q x a.md").isEmpty)
    }

    @Test("탐침·반례가 쓰지 않는 자리", arguments: [
        (".git/config", true), ("sub/.git/HEAD", true), (".autopilot/scenarios/u1/list.sh", true), ("CLAUDE.md", true), ("docs/CLAUDE.md", true),
        (".claude/settings.json", true), (".mcp.json", true), ("AGENTS.md", true), ("tests/test_app.py", true), ("app/todos.test.ts", true),
        ("app/todos.py", false), ("NOTES.md", false), ("check.sh", true), ("README.md", false),
        // 대소문자·유니코드 정규화(⑪-3 자동 보안 검토 MEDIUM): APFS 는 가리지 않으므로 접어서 견준다
        (".GIT/config", true), (".Git", true), ("sub/.Git/HEAD", true), (".CLAUDE/settings.json", true), (".Claude/settings.json", true), ("Claude.md", true),
        ("claude.md", true), ("docs/Claude.Local.md", true), ("Agents.md", true), (".MCP.json", true), (".Autopilot/scenarios/u1/list.sh", true), ("Check.sh", true),
        ("TESTS/x.py", true), ("App/\u{D310}\u{C815}.sh", true), ("app/\u{1111}\u{1161}\u{11AB}\u{110C}\u{1165}\u{11BC}.sh", true),
    ])
    func 보호(_ p: String, _ protected: Bool) {
        #expect(apProtectedJudgePath(p, judgeFiles: ["check.sh", "app/\u{D310}\u{C815}.sh"]) == protected)
        #expect(apCasePathOK(p, judgeFiles: ["check.sh", "app/\u{D310}\u{C815}.sh"]) == !protected)
    }

    @Test("탐침 대상은 판정 자체·보호 자리를 뺀다")
    func 대상() {
        #expect(apProbeTargets(toyView) == ["app/todos.py"])
        let v = APJudgeView(unit: 2, unitId: "u2", title: "안내", check: "python3 tests/check.py && grep -q '할 일' NOTES.md && test -f .autopilot/x.json",
                            scenarios: [APJudgeText(file: "s/a.sh", text: "test -f s/a.sh && test -f CLAUDE.md && test -f out.json")])
        #expect(apProbeTargets(v) == ["out.json", "NOTES.md"])
    }

    @Test("찾는 글자: 따옴표 안, 차례대로, 겹치지 않게")
    func 글자() {
        #expect(apJudgeLiterals([listSh]) == ["app/todos.py 가 없습니다", "GET /todos"])
        #expect(apJudgeLiterals([routesPy, listSh]) == ["app/todos.py", "app/todos.py 가 없습니다", "POST /todos", "GET /todos"])
        #expect(apJudgeLiterals(["echo '$X' \"--\" '  '"]) == ["$X"])
    }
}

@Suite("자율주행 판정 비판: ① 반대 탐침") struct ProbeTests {
    @Test("탐침 계획은 대상 꼴마다 정해진 글(같은 판정이면 같은 글)")
    func 계획() {
        let p = apProbePlan(toyView)
        #expect(p.targets == ["app/todos.py"])
        #expect(p.probes.map(\.kind) == [.empty, .junk, .truncated])
        let junk = p.probes[1].files[0].text
        #expect(junk.contains("ZZ_PROBE = [\"app/todos.py\", \"app/todos.py 가 없습니다\", \"POST /todos\", \"GET /todos\"]"))
        #expect(junk.hasSuffix("raise SystemExit(3)\n"))
        #expect(p.probes[2].files[0].text.contains("def zz_probe(:"))
        #expect(apProbePlan(toyView) == p)
        // JSON 대상·서버 포트가 있으면 목록·가짜 서버
        let v = APJudgeView(unit: 1, unitId: "u1", title: "t", check: "python3 -c 'import json; d=json.load(open(\"out/todo.json\")); assert d[\"title\"]' && test -f app/server.py",
                            scenarios: [], checkPorts: [18431])
        let q = apProbePlan(v)
        #expect(q.targets == ["out/todo.json", "app/server.py"])
        #expect(q.probes.map(\.kind) == [.empty, .junk, .junkList, .truncated, .stubServer])
        #expect(q.probes[1].files[0].text == "{\"title\": \"zz-probe\"}\n")
        #expect(q.probes[2].files[0].text == "[{\"title\": \"zz-probe\"}]\n")
        #expect(q.probes[3].files[0].text == "{\"title\": \"zz-pro")
        let stub = q.probes[4].files[1].text
        #expect(stub.contains("ThreadingHTTPServer((\"127.0.0.1\", p), _ZZ) for p in [18431]"))
        #expect(stub.contains("if __name__ == \"__main__\":"))
        #expect(q.probes[4].files[0].text == "{\"title\": \"zz-probe\"}\n")   // 자료 대상은 엉터리 값
    }

    @Test("형식만 보는 판정은 망친 결과물로 통과해 걸리고, 뜻을 보는 판정은 막는다")
    func 돌리기() {
        let weak = FakeTree(steps: toySteps, verdict: weakVerdict)
        let r = apRunProbes(toyView, baseline: failedBaseline(toySteps), ws: weak, budgetSec: 600)
        #expect(r.note == nil)
        #expect(r.runs.map(\.kind) == [.empty, .junk, .truncated])
        #expect(r.hits.map(\.step) == ["scenario-1", "scenario-2"])
        #expect(r.hits.first?.kinds == [.junk, .truncated])
        #expect(weak.files == [:])   // 끝나면 깨끗한 기준으로 되돌림
        let strong = FakeTree(steps: toySteps, verdict: strongVerdict)
        #expect(apRunProbes(toyView, baseline: failedBaseline(toySteps), ws: strong, budgetSec: 600).hits.isEmpty)
    }

    @Test("일하기 전에 이미 통과한 걸음은 보지 않고, 전체가 통과하면 돌리지 않는다")
    func 이미통과() {
        let t = FakeTree(steps: toySteps, verdict: weakVerdict)
        let r = apRunProbes(toyView, baseline: failedBaseline(toySteps, passing: ["scenario-2"]), ws: t, budgetSec: 600)
        #expect(r.hits.map(\.step) == ["scenario-1"])
        let all = apRunProbes(toyView, baseline: failedBaseline(toySteps, passing: ["scenario-1", "scenario-2"]), ws: t, budgetSec: 600)
        #expect(all.runs.isEmpty && all.note?.contains("판정 미리 돌리기 카드") == true)
    }

    @Test("시간 상한·쓰기 실패·대상 없음은 까닭")
    func 까닭() {
        let t = FakeTree(steps: toySteps, verdict: weakVerdict)
        t.sec = 200   // 걸음 둘 = 탐침마다 400초: 둘째까지 돌고 셋째는 상한
        let r = apRunProbes(toyView, baseline: failedBaseline(toySteps), ws: t, budgetSec: 600)
        #expect(r.runs.map { $0.note == nil } == [true, true, false])
        let u = FakeTree(steps: toySteps, verdict: weakVerdict)
        u.putFail = "app/todos.py"
        #expect(apRunProbes(toyView, baseline: failedBaseline(toySteps), ws: u, budgetSec: 600).runs.allSatisfy { $0.note == "링크라 쓰지 않았습니다" })
        let none = APJudgeView(unit: 1, unitId: "u1", title: "t", check: "true", scenarios: [])
        #expect(apRunProbes(none, baseline: failedBaseline([step("check")]), ws: t, budgetSec: 60).note?.contains("찾지 못해") == true)
    }

    @Test("탐침 카드: 판정 고치기 추천, id 는 단위와 판정 걸음")
    func 카드() {
        let hit = APProbeHit(step: "scenario-2", label: "x", file: ".autopilot/scenarios/u1/list.sh", criterion: "AC-2", kinds: [.junk])
        let c = apProbeCard(n: 1, unitId: "u1", title: "할 일", hit: hit, targets: ["app/todos.py"])
        #expect(c["id"] as? String == "probe-u1-.autopilot/scenarios/u1/list.sh")
        #expect(c["recommended"] as? String == "fix_check")
        #expect((c["options"] as? [[String: String]])?.map { $0["key"] ?? "" } == ["fix_check", "keep", "drop"])
        #expect((c["title"] as? String)?.contains("수용 기준 AC-2") == true)
        #expect(!((c["why"] as? String) ?? "").contains("—"))
    }
}

private let criticUnits = [
    APCriticUnit(id: "u1", title: "할 일 만들기·목록 API", prompt: "할 일 경로를 만든다", criteria: [APCriterion(id: "AC-1", text: "POST"), APCriterion(id: "AC-2", text: "GET")],
                 check: "", scenarios: [APCriticScenario(file: ".autopilot/scenarios/u1/routes.py", kind: .shell, criterion: "AC-1", copy: "/J/u1/1-routes.py"),
                                         APCriticScenario(file: ".autopilot/scenarios/u1/list.sh", kind: .shell, criterion: "AC-2", copy: "/J/u1/2-list.sh")]),
    APCriticUnit(id: "u2", title: "README 안내", prompt: "안내를 쓴다", criteria: [APCriterion(id: "AC-3", text: "읽힌다")], check: "grep -q '할 일' NOTES.md", scenarios: []),
]
private let criticTargets = ["u1": ["app/todos.py"], "u2": ["NOTES.md"]]
private let findingsJSON = """
[{"id":"f1","unit":"u1","criterion":"AC-2","judge":".autopilot/scenarios/u1/list.sh","kind":"counterexample","why":"GET 글자만 있고 경로가 없는 파일도 통과합니다","files":["app/todos.py"]},
 {"id":"f2","unit":"u2","criterion":"AC-3","kind":"unverifiable","why":"읽기 쉬운지는 반례 파일로 가리지 못합니다"},
 {"id":"f3","unit":"u1","criterion":"AC-1","kind":"interpretation","why":"경로만 있으면 되는지 만들기가 되는지 갈립니다","readings":[{"key":"route","label":"경로만 있으면 됨"},{"key":"works","label":"만들기가 실제로 됨"}],"recommended":"works"},
 {"id":"f4","unit":"u1","judge":".autopilot/scenarios/u1/list.sh","kind":"counterexample","why":"시나리오를 바꾸면 통과합니다","files":[".autopilot/scenarios/u1/list.sh"]},
 {"id":"f5","unit":"u1","kind":"counterexample","why":"반례 파일이 없습니다","files":["app/none.py"]},
 {"id":"f6","unit":"u9","kind":"unverifiable","why":"없는 단위"},
 {"id":"x7","unit":"u1","kind":"unverifiable","why":"id 꼴"},
 {"id":"f8","unit":"u1","judge":"check","kind":"unverifiable","why":"판정 명령 없는 단위"},
 {"id":"f9","unit":"u1","kind":"interpretation","why":"읽기 하나","readings":[{"key":"a","label":"하나"}]}]
"""
private let caseFiles: [String: String] = ["findings.json": findingsJSON, "cases/f1/app/todos.py": "# GET /todos 는 아직 만들지 않음\nROUTES = []\n",
                                           "cases/f4/.autopilot/scenarios/u1/list.sh": "exit 0\n"]

@Suite("자율주행 판정 비판: ② 비판 세션") struct CriticTests {
    @Test("작성과 다른 모델", arguments: [("opus", "sonnet"), ("sonnet", "opus"), ("haiku", "sonnet"), ("claude-opus-4-1", "sonnet"), ("claude-sonnet-4-5", "opus"), ("zz-model", "sonnet")])
    func 모델(_ author: String, _ critic: String) {
        #expect(apCriticModel(author: author) == critic)
        #expect(apModelFamily(apCriticModel(author: author)) != apModelFamily(author))
    }

    @Test("사람이 정한 비판 모델이 작성 모델과 같은 계열이면 받지 않는다")
    func 모델고르기() {
        #expect(apCriticModelPick(author: "opus", asked: nil) == ("sonnet", nil))
        #expect(apCriticModelPick(author: "opus", asked: "haiku") == ("haiku", nil))
        let same = apCriticModelPick(author: "opus", asked: "claude-opus-4-1")
        #expect(same.model == "sonnet" && same.note?.contains("같은 계열") == true)
        #expect(apCriticModelPick(author: "sonnet", asked: "a b").note?.contains("꼴이 틀려") == true)
    }

    @Test("세션 글: 첫 줄 고정·기준표·단위 글과 판정만(작성 세션 글 없음)·믿지 않는 자료")
    func 글() {
        let x = APCriticInput(units: criticUnits, topic: "할 일 API 를 만든다", judgeDir: "/J", tree: "/J/tree.txt", bundle: "/B", bundleFiles: ["link-1.json"], out: "/O", misses: nil)
        let p = apCriticPrompt(x)
        #expect(p.hasPrefix(apCriticHeadLine))
        #expect(p.contains(apCriticRubric))
        #expect(p.contains("/J/u1/2-list.sh") && p.contains("grep -q '할 일' NOTES.md") && p.contains("할 일 API 를 만든다"))
        #expect(p.contains("믿지 않는 자료") && p.contains("결과 폴더 /O 하나뿐"))
        #expect(!p.contains(apDraftHeadLine) && !p.contains("—"))
    }

    @Test("결과 읽기: 틀린 줄은 빼고, 판정 자체를 바꾸는 반례·없는 반례 파일은 확인 안 됨으로")
    func 읽기() {
        let (fs, problems) = apCriticParse(FakeOut(files: caseFiles), units: criticUnits, targets: criticTargets)
        #expect(fs.map(\.id) == ["f1", "f2", "f3", "f4", "f5"])
        #expect(fs.map(\.kind) == [.counterexample, .unverifiable, .interpretation, .unverifiable, .unverifiable])
        #expect(fs[3].refusal?.contains("판정·하네스 자리라") == true && fs[3].why == "시나리오를 바꾸면 통과합니다")
        #expect(fs[4].refusal?.contains("판정이 보는 자리(app/todos.py)가 아닌 파일") == true)
        #expect(fs[2].recommended == "works")
        #expect(problems.count == 4)
        #expect(apCriticParse(FakeOut(files: [:]), units: criticUnits, targets: criticTargets).problems == ["결과 파일 findings.json 이 없습니다"])
    }

    @Test("반례 확인: 형식만 보는 판정은 통과(확인된 지적), 뜻을 보는 판정은 막음, 이미 통과는 확인 안 됨")
    func 확인() {
        let (fs, _) = apCriticParse(FakeOut(files: caseFiles), units: criticUnits, targets: criticTargets)
        let read = { (p: String) in caseFiles[p].map { Data($0.utf8) } }
        let views = ["u1": toyView]
        let weak = FakeTree(steps: toySteps, verdict: weakVerdict)
        let r = apVerifyFindings(fs, views: views, baseline: ["u1": failedBaseline(toySteps)], read: read, ws: weak)
        #expect(r.map { apVerdictKey($0.verdict) } == ["confirmed", "unverified", "reading", "unverified", "unverified"])
        #expect(r[3].verdict == .unverified(fs[3].refusal ?? "") && r[1].verdict == .unverified("비판 세션이 반례를 만들지 못했다고 적었습니다"))
        #expect(r[0].step == "scenario-2")
        #expect(weak.files == [:])
        let strong = FakeTree(steps: toySteps, verdict: strongVerdict)
        #expect(apVerdictKey(apVerifyFindings([fs[0]], views: views, baseline: ["u1": failedBaseline(toySteps)], read: read, ws: strong)[0].verdict) == "refuted")
        let pre = apVerifyFindings([fs[0]], views: views, baseline: ["u1": failedBaseline(toySteps, passing: ["scenario-2"])], read: read, ws: weak)
        #expect(pre[0].verdict == .unverified("그 판정이 일하기 전부터 통과해 반례로 가리지 못합니다(판정 미리 돌리기 카드로)"))
    }

    @Test("카드 셋: 확인된 지적(판정 고치기 추천)·확인 안 됨(그대로 추천)·해석 고르기, 막힌 반례는 카드 없음")
    func 카드() {
        let f = APCriticFinding(id: "f1", unit: "u1", criterion: "AC-2", judge: ".autopilot/scenarios/u1/list.sh", kind: .counterexample, why: "글자만", files: ["app/todos.py"])
        let c1 = apCriticCard(APCriticResult(finding: f, verdict: .confirmed), n: 1, unitTitle: "할 일")
        #expect(c1?["kind"] as? String == "critic" && c1?["recommended"] as? String == "fix_check" && c1?["id"] as? String == "critic-f1")
        let c2 = apCriticCard(APCriticResult(finding: f, verdict: .unverified("까닭")), n: 1, unitTitle: "할 일")
        #expect(c2?["kind"] as? String == "critic_unverified" && c2?["recommended"] as? String == "keep" && (c2?["title"] as? String)?.hasPrefix("확인 안 됨") == true)
        var g = f; g.kind = .interpretation; g.readings = [APDraftOption(key: "a", label: "가"), APDraftOption(key: "b", label: "나")]; g.recommended = "b"
        let c3 = apCriticCard(APCriticResult(finding: g, verdict: .reading), n: 1, unitTitle: "할 일")
        #expect(c3?["kind"] as? String == "critic_reading" && c3?["recommended"] as? String == "b")
        #expect(apCriticCard(APCriticResult(finding: f, verdict: .refuted), n: 1, unitTitle: "할 일") == nil)
    }
}

@Suite("자율주행 판정 비판: 판정 고치기 고리") struct FixLoopTests {
    @Test("최대 2번, 확인된 지적이 없으면 멈춤", arguments: [(0, 0, "stop"), (0, 2, "offer1"), (1, 1, "offer2"), (1, 0, "stop"), (2, 3, "stop")])
    func 차례(_ done: Int, _ confirmed: Int, _ want: String) {
        switch apFixNext(roundsDone: done, confirmedNow: confirmed) {
        case .offer(let r): #expect(want == "offer\(r)")
        case .stop(let why):
            #expect(want == "stop")
            if done == 1 { #expect(why.contains("새로 확인된 지적이 없어")) }
            if done == 2 { #expect(why.contains("2번을 다 썼습니다")) }
        }
    }

    @Test("고치기 글: 초안 세션 첫 줄 그대로 + 고치기 표지 + 고칠 것 + 원래 글")
    func 글() {
        let base = APDraftInput(topic: "할 일 API", stack: APStackSet(), newFolder: true, repo: "/R", bundle: nil, bundleFiles: [], out: "/O", skeleton: [], firstUnit: false, harness: [])
        let p = apDraftFixPrompt(base, round: 1, items: [APFixItem(source: "critic", unit: "u1", judge: ".autopilot/scenarios/u1/list.sh", why: "글자만", files: ["f1/app/todos.py"])], inputDir: "/F")
        #expect(p.hasPrefix(apDraftHeadLine + " " + apDraftFixMarker))
        #expect(p.contains("1. 단위 u1 · 시나리오 .autopilot/scenarios/u1/list.sh · 비판 세션 반례: 글자만"))
        #expect(p.contains("/F") && p.contains("약하게 바꾸지 않습니다") && p.contains("units.json"))
        #expect(!p.contains("—"))
    }

    @Test("약해진 고침: 단위·수용 기준·기준의 시나리오·판정 명령·시나리오가 빠지면")
    func 약해짐() {
        let old = [APDraftedUnit(id: "u1", title: "a", prompt: "p", check: "test -f a", criteria: [APCriterion(id: "AC-1", text: "x"), APCriterion(id: "AC-2", text: "y")],
                                 scenarios: [APDraftScenario(file: "r.py", kind: .shell, criterion: "AC-1"), APDraftScenario(file: "l.sh", kind: .shell, criterion: "AC-2")]),
                   APDraftedUnit(id: "u2", title: "b", prompt: "p", check: "x")]
        #expect(apJudgeWeakening(old: old, new: old).isEmpty)
        var renamed = old; renamed[0].scenarios[1].file = "l2.sh"
        #expect(apJudgeWeakening(old: old, new: renamed).isEmpty)   // 같은 기준을 보는 시나리오로 바꾼 것은 약해진 것이 아님
        var weak = old; weak[0].check = ""; weak[0].scenarios.removeLast(); weak.removeLast()
        #expect(apJudgeWeakening(old: old, new: weak) == ["단위 u1 의 수용 기준 AC-2 를 보던 시나리오가 없어졌습니다", "단위 u1 의 판정 명령이 빠졌습니다",
                                                         "단위 u1 의 시나리오 l.sh 가 빠졌습니다", "단위 u2(b)가 빠졌습니다"])
        let c = apWeakCard(round: 1, reasons: ["x"])
        #expect(c["recommended"] as? String == "reject" && c["id"] as? String == "fix-weak-1")
    }
}

@Suite("자율주행 판정 놓침 되먹임") struct MissTests {
    @Test("버린 합칠 쪽에 든 단위", arguments: [(nil as String?, ["autopilot"], true), (nil, ["story:SHOP-1"], false), ("SHOP-1", ["story:SHOP-1"], true), ("SHOP-2", ["story:SHOP-1", "autopilot"], false)])
    func 버림(_ story: String?, _ src: [String], _ want: Bool) { #expect(apDiscardedUnit(story: story, discardedSources: Set(src)) == want) }

    @Test("단위 신호: 통과·버림·미룬 몫·거부·묻기·지시 밖 근거, id 는 같은 신호면 같다")
    func 신호() {
        let res: [String: Any] = ["next_start": [["title": "정렬 차례", "why": "사람 몫"]],
                                  "decisions_applied": [["what": "목록 차례", "basis": "출발 카드 c1 · CLAUDE.md"], ["what": "커밋 꼴", "basis": "CLAUDE.md 의 커밋 규칙"]]]
        let den: [[String: Any]] = [["tool": "AskUserQuestion"], ["tool": "WebFetch"], ["tool": "WebFetch"]]
        let a = apUnitMisses(project: "zz-toy", run: "r1", n: 1, unitId: "u1", sourceKey: "k1", title: "안내 글", status: "done", story: nil, discardedSources: ["autopilot"],
                             result: res, denials: den, judge: "grep -q '할 일' NOTES.md", judgeDigest: "d1", setId: "s1", at: 100)
        #expect(a.map(\.signal) == [.discard, .nextStart, .asked, .denial, .decisionBasis])
        #expect(a.map(\.id) == ["r1/1/discard/1", "r1/1/next_start/1", "r1/1/asked/1", "r1/1/denial/1", "r1/1/decision_basis/1"])
        #expect(a[0].judgeDigest == "d1" && a[2].judgeDigest == nil)
        #expect(a[3].why == "권한 거부 2번: WebFetch")
        #expect(a[4].why.contains("커밋 꼴"))
        #expect(a.map(\.side) == ["judge", "judge", "instruction", "instruction", "instruction"])
        let b = apUnitMisses(project: "zz-toy", run: "r1", n: 2, unitId: "u2", sourceKey: nil, title: "목록", status: "failed", story: nil, discardedSources: ["autopilot"],
                             result: ["next_start": [["title": "정렬"]]], denials: [], judge: nil, judgeDigest: nil, setId: nil, at: 100)
        #expect(b.map(\.signal) == [.parked])
    }

    @Test("출발 준비의 확인된 지적: 같은 준비에서 고친 것은 fixed, 마지막 차례에도 남은 것은 open 한 줄")
    func 준비() {
        let critic: [String: Any] = ["rounds": [
            ["results": [["id": "f1", "unit": "u1", "judge": "l.sh", "verdict": "confirmed", "why": "글자만"], ["id": "f2", "unit": "u2", "judge": "check", "verdict": "confirmed", "why": "x"]]],
            ["results": [["id": "f1", "unit": "u1", "judge": "l.sh", "verdict": "refuted"], ["id": "f2", "unit": "u2", "judge": "check", "verdict": "confirmed", "why": "x"]]]]]
        let prerun: [String: Any] = ["probe_history": [["cards": [["id": "probe-u1-l.sh", "unit_id": "u1", "file": "l.sh", "probes": ["junk"]]]]],
                                     "probe_cards": [] as [Any]]
        let units = [(n: 1, id: "u1", sourceKey: "k1" as String?, title: "가"), (n: 2, id: "u2", sourceKey: nil as String?, title: "나")]
        let m = apPrepMisses(project: "p", run: "r", prerun: prerun, critic: critic, answers: [:], units: units, setId: nil, at: 1)
        #expect(m.map(\.id) == ["r/prep/critic/0/f1", "r/prep/critic/1/f2", "r/prep/probe/0/probe-u1-l.sh"])
        #expect(m.map(\.state) == ["fixed", "open", "fixed"])
        #expect(m.map(\.signal) == [.critic, .critic, .probe])
    }

    @Test("요약: 열린 것·최근 것 먼저, 판정 쪽·지시 쪽, 줄·글자 상한")
    func 요약() {
        var rows: [APMiss] = []
        for i in 1...40 {
            let sig: APMissSignal = i % 2 == 0 ? .discard : .denial
            let st = i == 40 ? "used" : "open"
            rows.append(APMiss(id: "r/\(i)", project: "p", run: "r", unitId: "u\(i)", signal: sig, why: "까닭 \(i)", state: st, created: Double(i), title: "단위 \(i)"))
        }
        let d = apMissDigest(rows)
        #expect(d.hasPrefix("판정 쪽:\n- [통과했지만 버림] 단위 38"))
        #expect(d.contains("지시 쪽:"))
        #expect(d.hasSuffix("(그 밖에 10건은 줄였습니다)"))
        #expect(d.components(separatedBy: "\n").filter { $0.hasPrefix("- ") }.count == 30)
        #expect(apMissDigest(rows, maxRows: 100, maxChars: 200).count <= 260)
        #expect(apMissDigest([]).isEmpty)
    }

    @Test("세트 이어 쓰기를 다시 돌리는 조건", arguments: [(false, false, 0, false), (false, false, 1, true), (true, false, 0, true), (false, true, 0, true)])
    func 이어쓰기(_ h: Bool, _ n: Bool, _ m: Int, _ want: Bool) {
        let r = apSetRewrite(harnessChanged: h, newRequest: n, openMisses: m)
        #expect(r.rewrite == want)
        if m > 0 { #expect(r.reasons == ["판정 놓침 \(m)건이 있습니다"]) }
    }

    @Test("세트 이어 쓰기 결과 검사: 판정 명령만, 지우기·push·받지 않는 칸·첫 줄")
    func 이어쓰기검사() {
        let us = [APReviseUnit(id: "u1", title: "안내", prompt: "p", check: "grep -q '할 일' NOTES.md", scenarios: []), APReviseUnit(id: "u2", title: "목록", prompt: "p", check: "test -f a", scenarios: [])]
        let good = FakeOut(files: ["units.json": "[{\"id\":\"u1\",\"check\":\"test \\\"$(sed -n 's/^할 일: //p' NOTES.md)\\\" = 우유\",\"why\":\"글자만 보던 것\"},{\"id\":\"u2\",\"check\":\"test -f a\"}]",
                                   "autopilot.md": apInstructionHeadLine + "\n대답할 사람이 없습니다.\n"])
        let r = apReviseCheck(good, units: us)
        #expect(r.ok && Array(r.checks.keys) == ["u1"] && r.instruction?.hasPrefix(apInstructionHeadLine) == true && r.whys["u1"] == "글자만 보던 것")
        let bad = FakeOut(files: ["units.json": "[{\"id\":\"u1\",\"check\":\"\"},{\"id\":\"u2\",\"check\":\"git push origin live\"},{\"id\":\"u9\",\"check\":\"x\"},{\"id\":\"u1\",\"check\":\"x\",\"prompt\":\"y\"}]",
                                  "autopilot.md": "첫 줄이 틀림\n"])
        let b = apReviseCheck(bad, units: us)
        #expect(b.problems.count == 5 && b.instruction == nil)
        #expect(b.problems[0].contains("지웠습니다"))
        #expect(apReviseCheck(FakeOut(files: [:]), units: us).ok)
        let p = apRevisePrompt(units: us, instruction: nil, misses: "판정 쪽:\n- x", reasons: ["판정 놓침 1건이 있습니다"], out: "/O")
        #expect(p.hasPrefix(apReviseHeadLine) && p.contains("판정 놓침 1건") && p.contains("grep -q '할 일' NOTES.md") && !p.contains("—"))
    }

    @Test("판정 차이: 단위 id 로 짝지어 판정 명령·시나리오 목록")
    func 차이() {
        let old: [[String: Any]] = [["id": "u1", "title": "안내", "check": "grep -q a N.md"], ["id": "u2", "title": "목록", "scenarios": [["file": "s.sh", "criterion": "AC-1"]]]]
        let new: [[String: Any]] = [["id": "u1", "title": "안내", "check": "test x"], ["id": "u2", "title": "목록", "scenarios": [["file": "s2.sh", "criterion": "AC-1"]]], ["id": "u3", "check": "y"]]
        let c = apJudgeChanges(old: old, new: new)
        #expect(c == [APJudgeChange(unit: "u1", title: "안내", field: "check", old: "grep -q a N.md", new: "test x"),
                      APJudgeChange(unit: "u2", title: "목록", field: "scenarios", old: "s.sh(AC-1)", new: "s2.sh(AC-1)")])
        #expect(apJudgeChanges(old: old, new: old).isEmpty)
    }
}

@Suite("자율주행 실행 기록 저장소 판 올리기") struct RecordsMigrationTests {
    private func exec(_ db: OpaquePointer?, _ sql: String) -> Int32 { sqlite3_exec(db, sql, nil, nil, nil) }
    private func one(_ db: OpaquePointer?, _ sql: String) -> String? {
        var st: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &st, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(st) }
        guard sqlite3_step(st) == SQLITE_ROW, let c = sqlite3_column_text(st, 0) else { return nil }
        return String(cString: c)
    }
    private func migrate(_ db: OpaquePointer?) -> Int32 {
        let v = Int32(one(db, "PRAGMA user_version") ?? "0") ?? 0
        for s in apRecordsSteps(from: v) {
            guard exec(db, s.sql) == SQLITE_OK, exec(db, "PRAGMA user_version=\(s.version)") == SQLITE_OK else { return -1 }
        }
        return Int32(one(db, "PRAGMA user_version") ?? "0") ?? 0
    }

    @Test("옛 판(1) 저장소를 열어 판 2 로 올리면 행은 그대로이고 판정 놓침 표가 생긴다")
    func 옛판() throws {
        let path = NSTemporaryDirectory() + "zz-records-v1-\(getpid()).sqlite"
        unlink(path)
        defer { unlink(path); unlink(path + "-wal"); unlink(path + "-shm") }
        var db: OpaquePointer?
        #expect(sqlite3_open(path, &db) == SQLITE_OK)
        #expect(exec(db, apRecordsSchemaV1) == SQLITE_OK)
        #expect(exec(db, "PRAGMA user_version=1") == SQLITE_OK)
        #expect(exec(db, "INSERT INTO projects(id,name,root) VALUES('zz-toy','zz-toy','/w/zz-toy'); INSERT INTO runs(id,project_id,state) VALUES('r1','zz-toy','done')") == SQLITE_OK)
        sqlite3_close(db)
        #expect(sqlite3_open(path, &db) == SQLITE_OK)
        #expect(migrate(db) == apRecordsVersion)
        #expect(apRecordsVersion == 2)
        #expect(one(db, "SELECT state FROM runs WHERE id='r1'") == "done")
        #expect(exec(db, "INSERT INTO judge_misses(id,project_id,run_id,signal,side,why,state,created) VALUES('r1/1/discard/1','zz-toy','r1','discard','judge','x','open',1)") == SQLITE_OK)
        #expect(one(db, "SELECT COUNT(*) FROM judge_misses WHERE project_id='zz-toy' AND state='open'") == "1")
        #expect(apRecordsSteps(from: 2).isEmpty)
        sqlite3_close(db)
    }

    @Test("빈 저장소는 판 1·2 를 차례로")
    func 새저장소() {
        var db: OpaquePointer?
        #expect(sqlite3_open(":memory:", &db) == SQLITE_OK)
        #expect(apRecordsSteps(from: 0).map(\.version) == [1, 2])
        #expect(migrate(db) == 2)
        for t in apRecordsTables { #expect(one(db, "SELECT name FROM sqlite_master WHERE type='table' AND name='\(t)'") == t) }
        sqlite3_close(db)
    }
}

@Suite("자율주행 판정 놓침: 초안 세션 글") struct DraftMissesTests {
    @Test("지난 판정 놓침 요약이 있으면 초안 세션 글의 입력에 따로 칸으로(근거 묶음과 섞지 않음), 없으면 그대로")
    func 칸() {
        var x = APDraftInput(topic: "할 일", stack: APStackSet(), newFolder: false, repo: "/R", bundle: "/B", bundleFiles: ["a.json"], out: "/O", skeleton: [], firstUnit: false, harness: [])
        let plain = apDraftPrompt(x)
        #expect(!plain.contains("지난 판정 놓침"))
        x.misses = "판정 쪽:\n- [통과했지만 버림] 안내 글"
        let p = apDraftPrompt(x)
        #expect(p.contains("이 프로젝트의 지난 판정 놓침") && p.contains("- [통과했지만 버림] 안내 글"))
        #expect(p.replacingOccurrences(of: "\n- 이 프로젝트의 지난 판정 놓침(엔진 기록 요약, 지난 판정이 통과시켰지만 틀렸던 것과 지시가 멈추게 한 자리. 판정·지시를 쓸 때 같은 빈틈을 남기지 않습니다):\n" + x.misses!, with: "") == plain)
    }
}

@Suite("자율주행 판정 비판: 반례 허용 목록") struct CaseAllowTests {
    @Test("반례는 판정이 보는 자리만, 설정 파일 꼴·보호 자리는 받지 않는다(대소문자·정규화를 접어 견줌)", arguments: [
        ("app/todos.py", nil as String?), ("App/Todos.PY", nil), ("NOTES.md", "보는 자리"), ("conftest.py", "설정 파일"), ("package.json", "설정 파일"),
        ("Makefile", "설정 파일"), ("pytest.ini", "설정 파일"), ("pyproject.toml", "설정 파일"), ("setup.cfg", "설정 파일"), ("tox.ini", "설정 파일"),
        ("lib/sitecustomize.py", "설정 파일"), (".npmrc", "설정 파일"), ("x.pth", "설정 파일"), ("jest.config.js", "설정 파일"), (".GIT/config", "판정·하네스"),
        ("../x.py", "상대 경로"),
    ])
    func 허용(_ p: String, _ want: String?) {
        let why = apCasePathProblem(p, judgeFiles: [".autopilot/scenarios/u1/list.sh"], targets: ["app/todos.py", "package.json"])
        if let w = want { #expect(why?.contains(w) == true) } else { #expect(why == nil) }
    }

    @Test("시험 설정을 바꿔 통과시키는 반례는 확인된 지적이 되지 않는다")
    func 설정반례() {
        let u = [APCriticUnit(id: "u1", title: "t", prompt: "p", criteria: [], check: "python3 -m pytest -q", scenarios: [])]
        let files = FakeOut(files: ["findings.json": "[{\"id\":\"f1\",\"unit\":\"u1\",\"judge\":\"check\",\"kind\":\"counterexample\",\"why\":\"시험을 건너뜀\",\"files\":[\"conftest.py\"]}]",
                                    "cases/f1/conftest.py": "collect_ignore_glob = ['*']\n"])
        let (fs, _) = apCriticParse(files, units: u, targets: ["u1": []])
        #expect(fs.map(\.kind) == [.unverifiable] && fs[0].refusal?.contains("conftest.py") == true && fs[0].refusal?.contains("설정 파일") == true)
    }

    @Test("탐침 대상도 접은 꼴로 겹침을 뺀다")
    func 겹침() {
        let v = APJudgeView(unit: 1, unitId: "u1", title: "t", check: "test -f out.json && test -f OUT.json && test -f .Git", scenarios: [])
        #expect(apProbeTargets(v) == ["out.json"])
    }
}
