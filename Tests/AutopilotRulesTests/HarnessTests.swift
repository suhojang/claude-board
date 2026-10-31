// 자율주행 7단계 ⑫-7b 하네스 읽기 일반판 — 확인 지점 찾기(놓침·지나침 표), 읽는 파일 고르기(접은 이름·깊이·건너뛰는 자리),
// 하네스 파일 모으기(가짜 어댑터: 바로 가기·큰 파일·상한), 지시가 다뤘나(경계·범위), 정책 카드, 지문 견주기와 세트 기준, 지시 인용 빼기,
// 세트 이어 쓰기의 하네스·시나리오, 가져오기의 판정 시나리오
import Foundation
import Testing
@testable import AutopilotRules

// 줄 하나 → 찾을 종류(nil 이면 지나쳐야 함). file 은 그 줄이 든 하네스 파일
struct HarnessLineRow: Row {
    let note: String
    var file = "CLAUDE.md"
    let line: String
    let kind: String?
}
// 놓침을 줄인 줄(옛 낱말 꼴이 놓치던 것)과 지나침을 줄인 줄(옛 꼴이 잡던 것), 남은 한계는 아래 limitRows
let harnessLineRows: [HarnessLineRow] = [
    // 잡아야 하는 줄
    .init(note: "확인받기", line: "배포 전에는 사람에게 확인받고 진행한다.", kind: "confirm"),
    .init(note: "묻기(문장 끝)", line: "모호하면 사용자에게 묻는다.", kind: "ask"),
    .init(note: "묻고 진행", line: "작업 전에 사람에게 묻고 진행한다", kind: "ask"),
    .init(note: "물어보기", line: "범위가 넓으면 먼저 물어본다", kind: "ask"),
    .init(note: "AskUserQuestion 도구", file: ".claude/skills/t/SKILL.md", line: "2. AskUserQuestion 으로 담당자를 고른다.", kind: "ask"),
    .init(note: "영어 묻기", line: "Ask the user before deleting files.", kind: "ask"),
    .init(note: "사람의 확인(놓치던 꼴)", line: "삭제하기 전에 담당자의 확인을 거친다.", kind: "confirm"),
    .init(note: "검토 받기(놓치던 꼴)", line: "스키마 변경은 리뷰를 받은 뒤 합친다.", kind: "confirm"),
    .init(note: "허락(놓치던 꼴)", line: "새 의존성은 허락을 받고 넣는다.", kind: "confirm"),
    .init(note: "영어 확인", line: "Confirm with the user before running migrations.", kind: "confirm"),
    .init(note: "승인 받은 뒤", line: "- PR 은 리뷰어의 승인을 받은 뒤 머지한다.", kind: "approve"),
    .init(note: "승인 없이 + 부정(이중 부정은 확인 지점)", line: "- 승인 없이 main 에 합치지 않는다.", kind: "approve"),
    .init(note: "영어 승인", line: "Merging requires approval from a maintainer.", kind: "approve"),
    .init(note: "미리보기로 보여 줌", line: "- 릴리스 노트는 미리보기로 사람에게 보여 준 뒤 올린다.", kind: "preview"),
    .init(note: "사람 답 기다리기", line: "3. 사용자의 답을 기다린 뒤 다음으로 간다.", kind: "wait"),
    .init(note: "멈추고 기다리기", line: "막히면 멈추고 기다린다.", kind: "wait"),
    .init(note: "영어 기다리기", line: "Wait for the user to confirm the plan.", kind: "wait"),
    .init(note: "Jira 쓰기 도구", line: "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.", kind: "write"),
    .init(note: "Jira 쓰기 말(놓치던 꼴)", line: "끝나면 지라에 티켓을 만든다.", kind: "write"),
    .init(note: "메신저 알림(놓치던 꼴)", line: "배포가 끝나면 슬랙으로 알린다.", kind: "write"),
    .init(note: "git push", line: "작업이 끝나면 git push 로 올린다.", kind: "write"),
    .init(note: "gh pr create", line: "gh pr create 로 PR 을 만든다.", kind: "write"),
    .init(note: "울타리 코드 안의 도구 이름은 잡음", line: "git push origin main", kind: "write"),
    .init(note: "hook exit 2", file: ".claude/hooks/guard.sh", line: "  exit 2", kind: "block"),
    .init(note: "hook decision block", file: ".claude/hooks/guard.py", line: "print(json.dumps({\"decision\": \"block\", \"reason\": \"x\"}))", kind: "block"),
    .init(note: "hook permissionDecision deny", file: ".claude/hooks/g.js", line: "out({permissionDecision: \"deny\"})", kind: "block"),
    .init(note: "설정의 PreToolUse hook", file: ".claude/settings.json", line: "\"PreToolUse\": [", kind: "block"),
    .init(note: "설정의 권한 묻기", file: ".claude/settings.json", line: "\"ask\": [\"Bash(git push:*)\"]", kind: "ask"),
    // 지나쳐야 하는 줄
    .init(note: "대기열 낱말", line: "대기열 파일(queue.txt)을 먼저 읽는다.", kind: nil),
    .init(note: "자주 묻는 질문(옛 꼴이 잡던 것)", line: "자주 묻는 질문은 docs/faq.md 에 있다.", kind: nil),
    .init(note: "묻지 않고 진행", line: "사람에게 묻지 않고 끝까지 진행한다.", kind: nil),
    .init(note: "물어보지 않음", line: "작은 결정은 물어보지 않고 고른다.", kind: nil),
    .init(note: "결제 승인 API(옛 꼴이 잡던 이름)", line: "결제 승인 API 는 src/pay.py 에 있다.", kind: nil),
    .init(note: "승인 없이 진행(부정 하나)", line: "문서 고침은 승인 없이 진행한다.", kind: nil),
    .init(note: "빌드 기다리기(옛 꼴이 잡던 것)", line: "빌드가 끝날 때까지 기다린다.", kind: nil),
    .init(note: "미리보기 기능 이름", line: "미리보기 화면 컴포넌트는 src/preview.tsx 에 있다.", kind: nil),
    .init(note: "스스로 확인", line: "테스트가 통과하는지 확인한다.", kind: nil),
    .init(note: "영어 부정", line: "Don't ask the user for small choices.", kind: nil),
    .init(note: "읽기 도구 이름", line: "mcp__atlassian__getJiraIssue 로 티켓을 읽는다.", kind: nil),
    .init(note: "push 하지 않음", line: "git push 는 하지 않는다.", kind: nil),
    .init(note: "커밋 메시지의 지라 키", line: "커밋 메시지에 지라 티켓 번호를 남긴다.", kind: nil),
    .init(note: "hook 스크립트의 설명 줄은 사람 말 꼴을 보지 않음", file: ".claude/hooks/guard.sh", line: "# 배포 전에는 사람에게 확인받는다", kind: nil),
    .init(note: "설정의 허용 목록 도구 이름", file: ".claude/settings.json", line: "\"allow\": [\"Bash(git push:*)\"]", kind: nil),
    .init(note: "exit 0", file: ".claude/hooks/guard.sh", line: "exit 0", kind: nil),
]

@Suite("자율주행 하네스 읽기: 확인 지점 찾기") struct HarnessFinderTests {
    @Test("줄 꼴 표(놓침·지나침)", arguments: harnessLineRows)
    func 줄(r: HarnessLineRow) {
        let fenced = r.note.hasPrefix("울타리")
        let text = fenced ? "# 규칙\n```sh\n\(r.line)\n```\n" : r.line
        let p = apHarnessPoints([(path: r.file, text: text)])
        #expect(p.first?.kind == r.kind, "\(r.note): \(p.map(\.kind))")
        if r.kind != nil { #expect(p.first?.line == (fenced ? 3 : 1) && p.first?.file == r.file) }
    }

    @Test("울타리 코드 안에서는 사람 말 꼴을 잡지 않는다")
    func 울타리() {
        let p = apHarnessPoints([(path: "CLAUDE.md", text: "```\n사람에게 확인받고 진행한다\n```\n밖의 줄: 사람에게 묻는다\n")])
        #expect(p.map(\.place) == ["CLAUDE.md:4"])
    }

    @Test("남은 한계: 낱말 꼴이라 놓치는 줄")
    func 한계() {
        // 사람·답 낱말 없이 쓴 기다리기, 줄을 넘는 문장, 스크립트가 바깥 파일을 불러 막는 hook 은 놓친다(진행 상황 ⑫-7b 의 한계 표)
        let missed = ["다음 단계로 가기 전에 잠깐 멈춘다.", "결정은 사람이\n한다.", "python3 .claude/hooks/check.py"]
        for t in missed { #expect(apHarnessPoints([(path: "CLAUDE.md", text: t)]).isEmpty, "\(t)") }
    }
}

struct HarnessFileRow: Row {
    let note: String
    let path: String
    let wanted: Bool
}
let harnessFileRows: [HarnessFileRow] = [
    .init(note: "뿌리 CLAUDE.md", path: "CLAUDE.md", wanted: true),
    .init(note: "대소문자가 다른 이름(APFS 는 같은 파일)", path: "Claude.local.md", wanted: true),
    .init(note: "NFD 로 쓴 폴더 이름 아래", path: "문서/CLAUDE.md".decomposedStringWithCanonicalMapping, wanted: true),
    .init(note: "깊이 4 의 CLAUDE.md(지문과 같은 자리)", path: "a/b/c/d/CLAUDE.md", wanted: true),
    .init(note: "깊이 5 는 보지 않음", path: "a/b/c/d/e/CLAUDE.md", wanted: false),
    .init(note: ".mcp.json", path: ".mcp.json", wanted: true),
    .init(note: ".claude 규칙", path: ".claude/rules/a.md", wanted: true),
    .init(note: ".claude 설정", path: ".claude/settings.json", wanted: true),
    .init(note: ".claude 개인 설정", path: ".claude/settings.local.json", wanted: true),
    .init(note: ".claude hook 스크립트(확장자 없음)", path: ".claude/hooks/guard", wanted: true),
    .init(note: ".claude 안의 build 이름 폴더도 읽음", path: ".claude/skills/build/SKILL.md", wanted: true),
    .init(note: "대문자 .CLAUDE", path: ".CLAUDE/Rules/A.md", wanted: true),
    .init(note: "하위 폴더의 .claude", path: "sub/.claude/rules/a.md", wanted: true),
    .init(note: ".claude/worktrees 는 뺌", path: ".claude/worktrees/w/CLAUDE.md", wanted: false),
    .init(note: ".claude/.cc-writes 는 뺌", path: ".claude/.cc-writes/x.md", wanted: false),
    .init(note: ".claude 안의 그림은 글이 아님", path: ".claude/skills/a/logo.png", wanted: false),
    .init(note: "node_modules 안", path: "node_modules/x/CLAUDE.md", wanted: false),
    .init(note: "하네스가 아닌 문서", path: "docs/faq.md", wanted: false),
    .init(note: "상위 폴더 꼴", path: "../CLAUDE.md", wanted: false),
]

// 가짜 하네스 자리: 경로 → 글 또는 종류(link·large·fifo)
private struct FakeHarness: APHarnessSource {
    var files: [String: String]
    var kinds: [String: String] = [:]
    func walk(_ visit: (String) -> APWalkStep) {
        var skipped: [String] = []
        let all = Set(files.keys).union(kinds.keys)
        // 폴더 이름들도 넘긴다(어댑터의 enumerator 처럼)
        var dirs = Set<String>()
        for p in all { var parts = p.split(separator: "/").map(String.init); parts.removeLast(); var cur = ""; for x in parts { cur = cur.isEmpty ? x : cur + "/" + x; dirs.insert(cur) } }
        for p in Array(dirs.union(all)).sorted() {
            if skipped.contains(where: { p.hasPrefix($0 + "/") }) { continue }
            switch visit(p) {
            case .next: continue
            case .skip: skipped.append(p)
            case .stop: return
            }
        }
    }
    func read(_ relative: String, max: Int) -> Data? { files[relative].map { Data($0.utf8) } }
    func readFile(_ relative: String, max: Int) -> APHarnessFileRead? {
        if let k = kinds[relative] { return .other(k) }
        guard let t = files[relative] else { return nil }
        return t.utf8.count > max ? .other("large:\(t.utf8.count)") : .data(Data(t.utf8))
    }
}

@Suite("자율주행 하네스 읽기: 읽는 파일과 지문") struct HarnessReadTests {
    @Test("읽는 파일 고르기 표", arguments: harnessFileRows)
    func 파일(r: HarnessFileRow) { #expect(apHarnessFileWanted(r.path) == r.wanted) }

    @Test("모으기: 바로 가기·큰 파일은 지문에만, 건너뛰는 자리는 내려가지 않음")
    func 모으기() {
        let src = FakeHarness(files: ["CLAUDE.md": "사람에게 묻는다", ".claude/rules/big.md": String(repeating: "가", count: 100_000), ".claude/settings.json": "{}",
                                      "node_modules/x/CLAUDE.md": "묻는다", ".claude/worktrees/w/CLAUDE.md": "묻는다", "a/b/c/d/e/f/CLAUDE.md": "묻는다", "src/a.txt": "x"],
                              kinds: [".claude/rules/link.md": "link:/etc/hosts"])
        let r = apHarnessCollect(src)
        #expect(Set(r.texts.keys) == ["CLAUDE.md", ".claude/settings.json"])
        #expect(r.marks.map(\.path) == [".claude/rules/big.md", ".claude/rules/link.md", ".claude/settings.json", "CLAUDE.md"])
        #expect(r.marks.first { $0.path == ".claude/rules/link.md" }?.digest == "link:/etc/hosts")
        #expect(r.skipped.count == 2 && r.skipped.contains { $0.contains("바로 가기") } && r.skipped.contains { $0.contains("크기 상한") })
        #expect(apHarnessTexts(src).map(\.path) == [".claude/settings.json", "CLAUDE.md"])
    }

    @Test("모으기 상한: 300개에서 멈춤")
    func 상한() {
        var f: [String: String] = [:]
        for i in 0..<320 { f[".claude/rules/r\(i).md"] = "x" }
        let r = apHarnessCollect(FakeHarness(files: f))
        #expect(r.marks.count == apHarnessFileMax && r.capped)
    }

    @Test("출발 전 자율주행 가지에 시나리오로 커밋해도 되는 자리: 하네스·.git 은 아님(접은 꼴로)")
    func 시나리오자리() {
        #expect(apSeedScenarioPathOK(".autopilot/scenarios/u1/check.sh") && apSeedScenarioPathOK("sub/.autopilot/scenarios/p/u1/a.py"))
        for bad in [".claude/hooks/guard.sh", ".Claude/settings.json", "CLAUDE.md", "docs/Claude.md", ".mcp.json", ".git/hooks/pre-commit", ".GIT/config",
                    "../x.sh", "/etc/x", "a/.claude/rules/r.md", ".gitattributes"] {
            #expect(!apSeedScenarioPathOK(bad), "\(bad)")
        }
    }

    @Test("지문 견주기: 접은 경로, 바뀜·더함·뺌, 세트 기준 없으면 그대로")
    func 견주기() {
        let a = [APHarnessFileMark(path: "CLAUDE.md", digest: "1", size: 3), APHarnessFileMark(path: ".claude/rules/a.md", digest: "2", size: 3), APHarnessFileMark(path: "x.md", digest: "9", size: 1)]
        let b = [APHarnessFileMark(path: "claude.md", digest: "1", size: 3), APHarnessFileMark(path: ".claude/rules/a.md", digest: "3", size: 3), APHarnessFileMark(path: ".mcp.json", digest: "4", size: 2)]
        #expect(apHarnessChanges(old: a, new: b) == [APHarnessChange(path: ".claude/rules/a.md", change: "changed"), APHarnessChange(path: ".mcp.json", change: "added"),
                                                     APHarnessChange(path: "x.md", change: "removed")])
        #expect(apHarnessChanges(old: a, new: a).isEmpty)
        let base = APHarnessBaseline(marks: a, points: 4, at: 1_793_340_000, run: "zz-toy-1")
        let back = apHarnessBaselineParse(try! JSONSerialization.data(withJSONObject: apHarnessBaselineJSON(base)))
        #expect(back == base)
        #expect(apHarnessBaselineParse(Data("{\"format\":\"다른 것\",\"files\":[]}".utf8)) == nil)
        var read = APHarnessRead(); read.marks = b; read.texts = ["claude.md": "묻는다"]
        let same = apHarnessState(read, baseline: APHarnessBaseline(marks: b, points: 1, at: 0, run: ""))
        #expect(!same.changed && same.hasBaseline && same.points.count == 1)
        let ch = apHarnessState(read, baseline: base)
        #expect(ch.changed && ch.changes.count == 3)
        let none = apHarnessState(read, baseline: nil)
        #expect(!none.changed && !none.hasBaseline)
        #expect(apHarnessStateNote(none, setHasInstruction: true).contains("기준이 없어 그대로로 봅니다"))
        #expect(apHarnessStateNote(ch, setHasInstruction: true).contains("달라진 파일 3개"))
        #expect(apFNV64Data(Data("a".utf8)) == apFNV64("a"))
    }
}

struct HarnessCoverRow: Row {
    let note: String
    let instruction: String
    let missing: [String]
}
private let coverPoints = [APHarnessPoint(file: "CLAUDE.md", line: 3, kind: "confirm", text: "x"), APHarnessPoint(file: ".claude/rules/release.md", line: 4, kind: "approve", text: "y")]
let harnessCoverRows: [HarnessCoverRow] = [
    .init(note: "둘 다 이름으로", instruction: "- CLAUDE.md:3 보류\n- .claude/rules/release.md:4 보류", missing: []),
    .init(note: "대소문자를 접음", instruction: "claude.md:3 · .Claude/Rules/Release.md:4", missing: []),
    .init(note: "CLAUDE.md:30 은 :3 이 아님", instruction: "CLAUDE.md:30 · .claude/rules/release.md:4", missing: ["CLAUDE.md:3"]),
    .init(note: "docs/CLAUDE.md:3 은 뿌리 CLAUDE.md 가 아님", instruction: "docs/CLAUDE.md:3 · .claude/rules/release.md:4", missing: ["CLAUDE.md:3"]),
    .init(note: "줄 범위", instruction: "CLAUDE.md:2-4 · .claude/rules/release.md:3~5", missing: []),
    .init(note: "쉼표 목록", instruction: "CLAUDE.md:1,3 · .claude/rules/release.md:9", missing: [".claude/rules/release.md:4"]),
    .init(note: "파일 이름만", instruction: "CLAUDE.md 와 release.md 를 따른다", missing: ["CLAUDE.md:3", ".claude/rules/release.md:4"]),
]

@Suite("자율주행 하네스 읽기: 지시가 다뤘나·정책 카드·인용") struct HarnessCoverTests {
    @Test("파일:줄 경계 표", arguments: harnessCoverRows)
    func 경계(r: HarnessCoverRow) { #expect(apHarnessMissing(coverPoints, instruction: r.instruction).map(\.place) == r.missing) }

    @Test("정책 카드: 바깥 쓰기·배포 확인만, id 는 줄이 밀려도 같음")
    func 정책() {
        let w = APHarnessPoint(file: ".claude/skills/t/SKILL.md", line: 5, kind: "write", text: "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.")
        let d = APHarnessPoint(file: "CLAUDE.md", line: 4, kind: "confirm", text: "배포 전에는 사람에게 확인받고 진행한다.")
        let a = APHarnessPoint(file: "CLAUDE.md", line: 5, kind: "ask", text: "모호하면 사용자에게 묻는다.")
        #expect(apHarnessPolicyClass(w) == "write" && apHarnessPolicyClass(d) == "deploy" && apHarnessPolicyClass(a) == nil)
        var moved = w; moved.line = 9
        #expect(apHarnessPolicyId(w) == apHarnessPolicyId(moved) && apHarnessPolicyId(w).hasPrefix(apHarnessPolicyPrefix))
        #expect(apHarnessPolicyId(w) != apHarnessPolicyId(d))
        let c = apHarnessPolicyCard(w, missing: true)!
        #expect(c.recommended == "defer" && c.options.map(\.key) == ["defer", "record", "drop"] && c.why.contains("다루지 않았습니다") && !c.title.contains("—"))
        #expect(apHarnessPolicyCard(d, missing: false)!.options.map(\.key) == ["defer", "drop"])
        #expect(apHarnessPolicyCard(a, missing: true) == nil)
    }

    @Test("지시가 하네스 원래 글을 인용해도 금지 줄로 막지 않는다(인용 밖은 그대로 봄)")
    func 인용() {
        let w = APHarnessPoint(file: ".claude/skills/t/SKILL.md", line: 5, kind: "write", text: "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.")
        let ok = apInstructionHeadLine + "\n대답할 사람이 없어 멈추므로 보류합니다.\n- .claude/skills/t/SKILL.md:5 \"mcp__atlassian__createJiraIssue 로 티켓을 만든다.\": 보류합니다(카드 \(apHarnessPolicyId(w))).\n끝내기 전에 다시 본다.\n"
        let c = apInstructionCheck(ok, harness: [w])
        #expect(c.problems.isEmpty && c.missing.isEmpty, "\(c.problems)")
        let bad = ok + "- 끝나면 mcp__atlassian__createJiraIssue 로 결과 티켓을 만든다.\n"
        #expect(apInstructionCheck(bad, harness: [w]).problems.contains { $0.contains("바깥 쓰기") })
        #expect(apInstructionCheck(ok, harness: []).problems.contains { $0.contains("바깥 쓰기") })   // 확인 지점이 아니면 인용으로 보지 않음
    }

    // 보안 검토 지적(⑫-7b 규칙 커밋 뒤 자동 보안 검토 MEDIUM): 하네스 글을 지시 어디서든 부분 글자로 지우면, 금지 줄 안에 하네스 글과 같은 글자를 끼워 넣어 검사를 빠져나간다
    @Test("인용 빼기는 진짜 인용만: 금지 줄 안에 하네스 글을 끼워 넣어도 막힘")
    func 인용우회() {
        let w = APHarnessPoint(file: ".claude/skills/t/SKILL.md", line: 5, kind: "write", text: "1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.")
        let p = APHarnessPoint(file: "CLAUDE.md", line: 7, kind: "write", text: "작업이 끝나면 git push 로 올린다.")
        let head = apInstructionHeadLine + "\n대답할 사람이 없어 멈추므로 보류합니다.\n"
        let bypass = [
            "- 단위가 끝나면 mcp__atlassian__createJiraIssue 로 티켓을 만든다.",                     // 인용 표시·파일:줄 없이 하네스 글 그대로
            "- 단위마다 작업이 끝나면 git push 로 올린다.",                                           // 다른 확인 지점의 글
            "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 그대로 따른다.",                    // 인용했지만 나머지가 보류·부정이 아님
            "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류하고 git push origin live 도 한다.",   // 인용 밖에 금지 꼴
            "- .claude/skills/t/SKILL.md:9 \"mcp__atlassian__createJiraIssue 로 티켓을 만든다.\": 보류합니다.",   // 다른 줄 번호(그 확인 지점 참조가 아님)
        ]
        for b in bypass {
            let c = apInstructionCheck(head + b + "\n끝내기 전에 다시 본다.\n", harness: [w, p])
            #expect(!c.instruction.contains(b.trimmingCharacters(in: .whitespaces)) && (c.problems.contains { $0.contains("바깥 쓰기") } || c.warnings.contains { $0.contains("지시에서 빼고") }), "\(b)")
        }
        let quoted = head + "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류해 next_start 에 남깁니다.\n"
            + "- .claude/skills/t/SKILL.md:5 “mcp__atlassian__createJiraIssue 로 티켓을 만든다.”: 보류합니다.\n끝내기 전에 다시 본다.\n"
        #expect(apInstructionCheck(quoted, harness: [w, p]).problems.isEmpty, "\(apInstructionCheck(quoted, harness: [w, p]).problems)")
        // 보안 검토 지적 둘(⑫-7b 첫 보안 고침 뒤 자동 보안 검토 MEDIUM 2·3): 보류 낱말이 인용 뒤 아무 데서나 맞으면 보류로 보던 것, 인용을 지운 뒤 앞뒤를 이어 부정 창이 인용을 넘던 것
        let more = [
            "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 그대로 따른다, 멈추지 않는다.",          // 긍정 + 이중 부정
            "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류 없이 한다.",                       // 보류 낱말만 있음
            "- CLAUDE.md:7 git push \"작업이 끝나면 git push 로 올린다.\" 않는다.",                      // 인용 앞 금지 동작을 인용 뒤 부정이 덮음
            "- 먼저 git push 하고 CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류합니다.",          // 인용 앞에 참조 말고 다른 글
        ]
        // 보안 검토 지적 둘 더(MEDIUM 4·5): 카드 꼴이 넓어 금지 동작을 끼울 수 있던 것, 인용 앞이 그 지점 파일이 아닌 아무 글자 덩어리여도 받던 것
        let more2 = [
            "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류합니다(카드 x, git push origin live).",
            "- CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류합니다(카드 harness-CLAUDE-md-8).",     // 다른 지점의 카드 id
            "- update-ref;CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류합니다.",
            "- createJiraIssue/CLAUDE.md:7 \"작업이 끝나면 git push 로 올린다.\": 보류합니다.",
        ]
        for b in more + more2 {   // 확인 지점을 가리키는 줄은 지시에서 빠지고(그 지점은 카드), 가리키지 않는 줄은 막힌다 — 어느 쪽이든 지시에 남지 않는다
            let c = apInstructionCheck(head + b + "\n끝내기 전에 다시 본다.\n", harness: [w, p])
            #expect(!c.instruction.contains(b.trimmingCharacters(in: .whitespaces)) && (c.problems.contains { $0.contains("바깥 쓰기") } || c.warnings.contains { $0.contains("지시에서 빼고") }), "\(b)")
        }
        // 하네스 고치기 꼴에 걸리는 인용(확인 지점 줄)은 막지 않고 빼서 카드로, 확인 지점과 상관없는 금지 줄은 막음
        let edit = APHarnessPoint(file: "a/b/c/d/CLAUDE.md", line: 2, kind: "confirm", text: "여기 파일을 지우기 전에는 확인을 받는다.")
        let ec = apInstructionCheck(head + "- a/b/c/d/CLAUDE.md:2 \"여기 파일을 지우기 전에는 확인을 받는다.\": 그 폴더 파일은 그대로 둡니다.\n", harness: [edit])
        #expect(ec.problems.isEmpty && ec.missing.map(\.place) == ["a/b/c/d/CLAUDE.md:2"] && !ec.instruction.contains("a/b/c/d") && ec.warnings.contains { $0.contains("지시에서 빼고") })
        let ins = head + "- CLAUDE.md:7 보류\n- 단위마다 작업이 끝나면 git push 로 올린다.\n"
        #expect(apReviseCheck(ReviseOut(files: ["autopilot.md": ins]), units: [], harness: [w, p]).problems.contains { $0.contains("바깥 쓰기") })
    }
}

private struct ReviseOut: APDraftFiles {
    var files: [String: String]
    func read(_ relative: String) -> Data? { files[relative].map { Data($0.utf8) } }
    func list(_ relative: String) -> [String] {
        files.keys.filter { $0.hasPrefix(relative + "/") }.map { String($0.dropFirst(relative.count + 1)) }.filter { !$0.contains("/") }
    }
}

@Suite("자율주행 세트 이어 쓰기: 바뀐 하네스와 시나리오(⑫-7b)") struct ReviseHarnessTests {
    let units = [APReviseUnit(id: "u1", title: "인사", prompt: "p", check: "", scenarios: [".autopilot/scenarios/u1/check.sh"])]
    let orig = "#!/bin/sh\ntest -f hello.txt\n"

    @Test("시나리오: 고친 것만 받고, 지움·#! 없음·#! 만·비밀 꼴·새 파일은 받지 않음")
    func 시나리오() {
        let o = ["u1/check.sh": Data(orig.utf8)]
        let fixed = "#!/bin/sh\ntest \"$(cat hello.txt)\" = 안녕\n"
        let r = apReviseCheck(ReviseOut(files: ["scenarios/u1/check.sh": fixed]), units: units, originals: o)
        #expect(r.ok && r.scenarios.keys.sorted() == ["u1/check.sh"])
        #expect(apReviseCheck(ReviseOut(files: ["scenarios/u1/check.sh": orig]), units: units, originals: o).scenarios.isEmpty)
        #expect(apReviseCheck(ReviseOut(files: [:]), units: units, originals: o).problems.first?.contains("지웠습니다") == true)
        #expect(apReviseCheck(ReviseOut(files: ["scenarios/u1/check.sh": "test -f a\n"]), units: units, originals: o).problems.first?.contains("#!") == true)
        #expect(apReviseCheck(ReviseOut(files: ["scenarios/u1/check.sh": "#!/bin/sh\n"]), units: units, originals: o).problems.first?.contains("#! 줄밖에") == true)
        let tok = "gh" + "p_" + String(repeating: "A1", count: 18)
        #expect(apReviseCheck(ReviseOut(files: ["scenarios/u1/check.sh": "#!/bin/sh\necho \(tok)\n"]), units: units, originals: o).problems.first?.contains("비밀") == true)
        let extra = apReviseCheck(ReviseOut(files: ["scenarios/u1/check.sh": orig, "scenarios/u1/new.sh": "#!/bin/sh\nexit 0\n"]), units: units, originals: o)
        #expect(extra.ok && extra.scenarios.isEmpty && extra.warnings.first?.contains("받지 않았습니다") == true)
    }

    @Test("바뀐 하네스: 글에 바뀐 파일·확인 지점 전체·빠진 자리, 고친 지시가 다루지 않은 자리는 카드로")
    func 하네스() {
        let pts = [APHarnessPoint(file: "CLAUDE.md", line: 4, kind: "confirm", text: "배포 전 확인"),
                   APHarnessPoint(file: ".claude/rules/release.md", line: 5, kind: "confirm", text: "마이그레이션 확인")]
        let hz = APReviseHarness(files: [APReviseHarnessFile(path: ".claude/rules/release.md", change: "changed", copy: "/P/revise-in/harness/.claude/rules/release.md")],
                                 points: pts, missing: [pts[1]])
        let p = apRevisePrompt(units: units, instruction: "옛 지시", misses: "", reasons: ["하네스가 출발 때와 달라졌습니다"], out: "/O", harness: hz)
        #expect(p.contains("읽기 전용 사본 /P/revise-in/harness/.claude/rules/release.md") && p.contains(".claude/rules/release.md:5 [confirm]")
                && p.contains("지난 지시가 다루지 않은 자리: .claude/rules/release.md:5") && p.contains("scenarios/u1/check.sh") && !p.contains("—"))
        let ins = apInstructionHeadLine + "\n까닭.\n- CLAUDE.md:4 보류\n끝내기 전에 다시 본다.\n"
        let r = apReviseCheck(ReviseOut(files: ["autopilot.md": ins]), units: units, harness: pts)
        #expect(r.ok && r.harnessMissing.map(\.place) == [".claude/rules/release.md:5"])
        #expect(!apRevisePrompt(units: [], instruction: nil, misses: "", reasons: ["x"], out: "/O").contains("scenarios/"))
    }
}

@Suite("자율주행 가져오기: 판정 시나리오(⑫-7b)") struct ImportScenarioTests {
    @Test("시나리오만 있는 단위도 받고, 대기열 자리는 준비 id 아래")
    func 시나리오() {
        let units = #"[{"id":"u1","item":"i1","title":"인사","kind":"dev","prompt":"hello.txt 를 쓴다","est_min":10,"scenarios":[{"file":"check.sh","kind":"shell"}],"why":"근거"}]"#
        let files = ReviseOut(files: ["units.json": units, "scenarios/u1/check.sh": "#!/bin/sh\ntest -s hello.txt\n", "skipped.json": "[]"])
        let c = apImportCheck(files: files, items: [APImportPromptItem(id: "i1", key: "todo:TODO.md#인사", title: "인사", state: "open")], harness: [], needInstruction: false, setInstruction: nil)
        #expect(c.ok, "\(c.problems)")
        #expect(c.units.first?.unit.scenarios.map(\.file) == ["check.sh"])
        var it = APImportItem(); it.key = "todo:TODO.md#인사"; it.place = "TODO.md"
        let q = apImportQueueUnit(c.units[0], item: it, repo: "/R", context: nil, prep: "20261031-1200-zz-toy")
        #expect(q["check"] == nil)
        #expect(((q["scenarios"] as? [[String: Any]])?.first?["file"] as? String) == ".autopilot/scenarios/20261031-1200-zz-toy/u1/check.sh")
        let none = ReviseOut(files: ["units.json": units.replacingOccurrences(of: #""scenarios":[{"file":"check.sh","kind":"shell"}],"#, with: "")])
        #expect(apImportCheck(files: none, items: [APImportPromptItem(id: "i1", key: "k", title: "인사", state: "open")], harness: [], needInstruction: false, setInstruction: nil)
                    .problems.contains { $0.contains("판정이 없습니다") })
        #expect(apImportPrompt(APImportInput(source: .todo, origin: "TODO.md", request: "", repo: "/R", bundle: nil, out: "/O", items: [], harness: [], needInstruction: false))
                    .contains("scenarios/<단위 id>/<파일>"))
    }
}
