// 출발 확인 그 자리 바꾸기 규칙(StartSettings.swift 의 설정 줄, Promotion.swift 의 승격 차례 바꾸기, Instruction.swift 의 지시 고치기·차이)과
// 지난 실행의 원문 지우기(Purge.swift)·이어 간 줄 밖의 열린 미룬 몫(Carry.swift) 시험 — 2026-10-30 2단계 5-2b 에서 새로 씀
import Foundation
import Testing
import AutopilotRules

// MARK: - 설정 줄(모델·멈출 선)

struct SettingsRow: Row {
    let note: String
    let model: String
    let week: Double
    let five: Double
    let want: [String]
}
private let settingsRows: [SettingsRow] = [
    .init(note: "별명과 기본 멈출 선", model: "opus", week: 0.8, five: 0.95, want: []),
    .init(note: "전체 모델 이름도 꼴이면 된다", model: "claude-sonnet-4-6[1m]", week: 0.6, five: 1, want: []),
    .init(note: "빈 모델", model: "", week: 0.8, five: 0.95, want: ["모델이 비었습니다"]),
    .init(note: "빈칸이 든 모델", model: "opus 4", week: 0.8, five: 0.95, want: ["모델 이름 꼴이 아닙니다: opus 4"]),
    .init(note: "멈출 선 0%·101%", model: "haiku", week: 0, five: 1.01, want: ["멈출 선 주간 값은 1~100% 입니다", "멈출 선 5시간 값은 1~100% 입니다"]),
]

@Suite("자율주행 출발 확인: 설정 줄 바꾸기") struct SettingsTests {
    @Test("바꾼 값의 고칠 것", arguments: settingsRows)
    func 표(_ r: SettingsRow) { #expect(apSettingsProblems(model: r.model, week: r.week, five: r.five) == r.want) }

    @Test("고를 수 있는 값 — 지금 값이 목록 밖이면 함께")
    func 고를것() {
        #expect(apModelOptions(current: "haiku") == ["opus", "sonnet", "haiku"])
        #expect(apModelOptions(current: "claude-opus-4-7") == ["claude-opus-4-7", "opus", "sonnet", "haiku"])
        #expect(apStopOptions(apStopWeekChoices, current: 80) == [60, 70, 80, 90])
        #expect(apStopOptions(apStopWeekChoices, current: 85) == [60, 70, 80, 85, 90])
    }

    @Test("출발 준비가 쓸 값: 프로젝트 설정 → 지난 실행 → 기본", arguments: [
        (#"{}"#, #"{}"#, "default nil nil nil"),
        (#"{}"#, #"{"model":"haiku","stopline":{"seven_day":0.8,"five_hour":0.95}}"#, "past haiku 0.8 0.95"),
        (#"{"model":"opus"}"#, #"{"model":"haiku","stopline":{"seven_day":0.8,"five_hour":0.95}}"#, "project opus 0.8 0.95"),
        (#"{"stopline":{"seven_day":0.7}}"#, #"{"model":"haiku","stopline":{"seven_day":0.8,"five_hour":0.95}}"#, "project haiku 0.7 0.95"),
        (#"{"model":"","promotion":[]}"#, #"{"model":"sonnet"}"#, "past sonnet nil nil"),
    ])
    func 차례(_ a: (String, String, String)) {
        func o(_ s: String) -> [String: Any] { (try? JSONSerialization.jsonObject(with: Data(s.utf8))) as? [String: Any] ?? [:] }
        let p = apStartSettings(project: o(a.0), past: o(a.1))
        #expect("\(p.from) \(p.model ?? "nil") \(p.week.map { "\($0)" } ?? "nil") \(p.five.map { "\($0)" } ?? "nil")" == a.2)
    }
}

// MARK: - 승격 차례 바꾸기

private func steps(_ s: String) -> [APStep] {
    s.split(separator: " ").map { t in
        let ft = t.split(separator: "@", maxSplits: 1), fi = ft[0].split(separator: ">", maxSplits: 1)
        return APStep(from: String(fi[0]), into: String(fi[1]), after: ft.count > 1 ? String(ft[1]) : nil)
    }
}
struct PromoRow: Row {
    let note: String
    let steps: String
    var branches = ["dev", "live", "master"]
    let want: [String]
}
private let promoRows: [PromoRow] = [
    .init(note: "저장소 가지로 채운 기본 차례는 고칠 것이 없다", steps: "autopilot>dev autopilot>live@dev live>master@live", want: []),
    .init(note: "main 하나", steps: "autopilot>main", branches: ["main"], want: []),
    .init(note: "없는 가지로 받기", steps: "autopilot>develop", want: ["단계 1: 받는 가지 develop 가 저장소에 없습니다"]),
    .init(note: "앞 단계가 받지 않은 가지를 올리기", steps: "autopilot>dev live>master@dev",
          want: ["단계 2: live 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)"]),
    .init(note: "뒤 조건이 앞에 없는 가지", steps: "autopilot>live@dev", want: ["단계 1: dev 뒤라는 조건의 단계가 앞에 없습니다"]),
    .init(note: "뒤 조건이 받는 가지 자신", steps: "autopilot>dev@dev", want: ["단계 1: 뒤 조건이 받는 가지 자신(dev)입니다"]),
    .init(note: "같은 단계 두 번", steps: "autopilot>dev autopilot>dev", want: ["단계 2: 같은 단계(자율주행 가지 → dev)가 두 번 있습니다"]),
    .init(note: "같은 가지끼리", steps: "autopilot>dev dev>dev@dev", want: ["단계 2: 같은 가지(dev)끼리는 올리지 않습니다", "단계 2: 뒤 조건이 받는 가지 자신(dev)입니다"]),
    .init(note: "live 를 빼고 master 로 바로(dev 뒤)", steps: "autopilot>dev autopilot>master@dev", want: []),
    .init(note: "빈 차례는 고칠 것이 아니다(엔진은 차례 없이 합치기만 하고 기존 가지끼리는 거부)", steps: "", want: []),
]

@Suite("자율주행 출발 확인: 승격 차례 바꾸기") struct PromotionEditTests {
    @Test("바꾼 차례의 고칠 것", arguments: promoRows)
    func 표(_ r: PromoRow) { #expect(apPromotionProblems(r.steps.isEmpty ? [] : steps(r.steps), branches: r.branches) == r.want) }

    @Test("단계마다 고를 수 있는 것")
    func 고를것() {
        let s = steps("autopilot>dev autopilot>live@dev live>master@live")
        let c0 = apPromotionChoices(s, at: 0, branches: ["dev", "live", "master"])
        #expect(c0.from == ["autopilot"] && c0.after.isEmpty && c0.into == ["dev", "live", "master"])
        let c2 = apPromotionChoices(s, at: 2, branches: ["dev", "live", "master"])
        #expect(c2.from == ["autopilot", "dev", "live"] && c2.after == ["dev", "live"] && c2.into == ["dev", "master"])
        // 새 단계(끝 다음)
        #expect(apPromotionChoices(s, at: 3, branches: ["dev", "live", "master"]).after == ["dev", "live", "master"])
    }
}

// MARK: - 자율주행 지시 고치기와 지난 판 차이

struct DiffRow: Row {
    let note: String
    let old: String
    let new: String
    let want: String     // 줄마다 " a" "+b" "-c" 를 | 로
}
private let diffRows: [DiffRow] = [
    .init(note: "같음", old: "a\nb", new: "a\nb", want: " a| b"),
    .init(note: "앞뒤 빈 줄은 견주지 않는다(엔진이 자른다)", old: "\n\na\nb\n", new: "a\nb", want: " a| b"),
    .init(note: "한 줄 바꿈: 뺀 줄이 더한 줄보다 앞", old: "a\nb\nc", new: "a\nB\nc", want: " a|-b|+B| c"),
    .init(note: "끝에 더함", old: "a", new: "a\nb", want: " a|+b"),
    .init(note: "처음 판(지난 판이 빔)", old: "", new: "a\nb", want: "+a|+b"),
    .init(note: "와이어프레임 예: 40행을 바꾸고 21행 규칙을 더함", old: "앞선다\n14행\n40행 파일로\n47행", new: "앞선다\n14행\n40행 result.json\n47행\n21행 느린 시험",
          want: " 앞선다| 14행|-40행 파일로|+40행 result.json| 47행|+21행 느린 시험"),
    .init(note: "CRLF 는 같은 줄로", old: "a\r\nb", new: "a\nb", want: " a| b"),
]

@Suite("자율주행 출발 확인: 지시 차이") struct InstructionTests {
    @Test("줄 차이", arguments: diffRows)
    func 표(_ r: DiffRow) {
        let s = apLineDiff(r.old, r.new).map { ($0.kind == .add ? "+" : $0.kind == .del ? "-" : " ") + $0.text }.joined(separator: "|")
        #expect(s == r.want)
    }

    @Test("머리 줄과 저장 확인")
    func 머리() {
        #expect(apInstructionHead(old: "a\nb", new: "a\nc\nd", date: "10월 20일") == "지난 판(10월 20일)과 차이 +2 −1")
        #expect(apInstructionHead(old: "a", new: "a\n", date: "10월 20일") == "지난 판(10월 20일)과 같음")
        #expect(apInstructionHead(old: nil, new: "a", date: "") == "지난 판 없음 · 이 프로젝트에서 처음 쓰는 지시")
        #expect(apInstructionHead(old: nil, new: nil, date: "") == "지시 없음 · 공통 규칙만으로 돕니다")
        #expect(apInstructionSaveProblem("  \n ") == "빈 지시는 저장하지 않습니다. 고친 글을 넣어 주세요")
        #expect(apInstructionSaveProblem("이 글이 앞선다") == nil)
        #expect(apInstructionSaveProblem(String(repeating: "가", count: 100_001)) == "지시가 너무 깁니다(100001자, 10만 자까지)")
    }

    @Test("줄이 아주 많으면 모두 빼고 모두 더한 것으로")
    func 큰글() {
        let a = (0..<2100).map { "a\($0)" }.joined(separator: "\n"), b = (0..<2100).map { "b\($0)" }.joined(separator: "\n")
        let c = apDiffCount(apLineDiff(a, b))
        #expect(c.add == 2100 && c.del == 2100)
    }
}

// MARK: - 원문 지우기

struct PurgeRow: Row {
    let note: String
    var run = "20261030-0100-zz-todo"
    var runDir = "/R/autopilot/runs/20261030-0100-zz-todo"
    var state = "done"
    var wrapped = true
    var merge = false
    var worktree = false
    var draft = false
    var exists = true
    let want: String     // "ok" / "gone" / 까닭
}
private let purgeRows: [PurgeRow] = [
    .init(note: "끝나고 정리를 마친 실행", want: "ok"),
    .init(note: "중단·오류로 끝난 실행도", state: "stopped", want: "ok"),
    .init(note: "이미 지움", exists: false, want: "gone"),
    .init(note: "도는 실행", state: "running", want: "끝나지 않은 실행입니다"),
    .init(note: "끝 정리 전", wrapped: false, want: "엔진이 끝 정리를 마치지 않았습니다(하네스 검사·기록 저장소 쓰기가 실행 폴더를 씁니다)"),
    .init(note: "합치기 기다림", merge: true, want: "합치기·승격이 남았습니다. 합치기·승격이 이 실행 폴더의 기록을 쓰므로 합치거나 버린 뒤에 지웁니다"),
    .init(note: "따로 작업 트리가 남음", worktree: true,
          want: "따로 작업 트리(~/.claude-board/worktrees/20261030-0100-zz-todo)가 남아 있습니다. 합치기·승격에서 버리거나 합친 뒤에 지웁니다"),
    .init(note: "출발 준비가 이어 가는 중", draft: true, want: "출발 준비가 이 실행을 이어 가는 중입니다(남은 단위의 정의를 이 실행 폴더에서 읽습니다)"),
    .init(note: "기록의 실행 폴더가 뿌리 밖(옛 자리에서 옮겨 담은 실행)", runDir: "/Users/u/.claude/notifier/autopilot/runs/20261030-0100-zz-todo",
          want: "실행 폴더가 이 보드의 자율주행 뿌리 밖(/Users/u/.claude/notifier/autopilot/runs/20261030-0100-zz-todo)이라 보드에서 지우지 않습니다"),
    .init(note: "id 와 폴더 이름이 다름", runDir: "/R/autopilot/runs/other", want: "실행 폴더가 이 보드의 자율주행 뿌리 밖(/R/autopilot/runs/other)이라 보드에서 지우지 않습니다"),
    .init(note: "id 에 / (다른 자리로)", run: "../private", runDir: "/R/autopilot/runs/../private", want: "실행 id 꼴이 아니라 지우지 않습니다"),
    .init(note: "id 가 ..", run: "..", runDir: "/R/autopilot/runs/..", want: "실행 id 꼴이 아니라 지우지 않습니다"),
]

private func input(_ r: PurgeRow) -> APPurgeInput {
    APPurgeInput(run: r.run, root: "/R/autopilot", runDir: r.runDir, state: r.state, wrapped: r.wrapped, mergeWaiting: r.merge, worktreeLeft: r.worktree, inDraft: r.draft, exists: r.exists)
}

private final class FakeRaw: APRawFiles, @unchecked Sendable {
    var dirs: Set<String>
    var removed: [String] = []
    var records: [String: Data] = [:]
    var failRemove: String?
    var link = false
    init(_ d: Set<String>) { dirs = d }
    func isRealDir(_ path: String) -> Bool { !link && dirs.contains(path) }
    func usage(_ path: String) -> (files: Int, bytes: Int64) { (12, 34_567) }
    func remove(_ path: String) -> String? { if let f = failRemove { return f }; removed.append(path); dirs.remove(path); return nil }
    func writeRecord(_ run: String, _ data: Data) -> Bool { records[run] = data; return true }
}
private struct FakeClock: APClock { let now: Date }

@Suite("자율주행 지난 실행: 원문 지우기") struct PurgeTests {
    @Test("지울 수 있는가", arguments: purgeRows)
    func 표(_ r: PurgeRow) {
        let s: String
        switch apPurgeCheck(input(r)) {
        case .ok(let t): s = "ok"; #expect(t == "/R/autopilot/runs/" + r.run)
        case .gone: s = "gone"
        case .refuse(let w): s = w
        }
        #expect(s == r.want)
    }

    @Test("유스케이스: 실행 폴더만 지우고 기록을 쓴다")
    func 지우기() throws {
        let r = PurgeRow(note: "", want: "ok")
        let f = FakeRaw(["/R/autopilot/runs/20261030-0100-zz-todo", "/R/autopilot/private/20261030-0100-zz-todo"])
        let x = apPurgeRaw(input(r), files: f, clock: FakeClock(now: Date(timeIntervalSince1970: 1_793_300_000)))
        #expect(x.ok && x.error == nil)
        #expect(f.removed == ["/R/autopilot/runs/20261030-0100-zz-todo"])
        #expect(f.dirs.contains("/R/autopilot/private/20261030-0100-zz-todo"))
        let rec = try JSONDecoder().decode(APPurgeRecord.self, from: try #require(f.records[r.run]))
        #expect(rec.files == 12 && rec.bytes == 34_567 && rec.at == 1_793_300_000 && rec.error == nil)
        #expect(rec.kept.contains("엔진 전용 private/20261030-0100-zz-todo/(하네스 사본)"))
    }

    @Test("유스케이스: 링크면 지우지 않고, 지우다 실패해도 기록은 쓴다, 다시 확인해 거부")
    func 갈래() throws {
        let r = PurgeRow(note: "", want: "ok")
        let f = FakeRaw(["/R/autopilot/runs/20261030-0100-zz-todo"]); f.link = true
        let a = apPurgeRaw(input(r), files: f, clock: FakeClock(now: Date()))
        #expect(!a.ok && f.removed.isEmpty && f.records.isEmpty && a.error == "실행 폴더가 링크이거나 폴더가 아니라 지우지 않습니다")
        let g = FakeRaw(["/R/autopilot/runs/20261030-0100-zz-todo"]); g.failRemove = "권한 없음"
        let b = apPurgeRaw(input(r), files: g, clock: FakeClock(now: Date()))
        #expect(!b.ok && b.error == "권한 없음")
        #expect(try JSONDecoder().decode(APPurgeRecord.self, from: try #require(g.records[r.run])).error == "권한 없음")
        let h = FakeRaw(["/R/autopilot/runs/20261030-0100-zz-todo"])
        var m = r; m.merge = true
        #expect(apPurgeRaw(input(m), files: h, clock: FakeClock(now: Date())).error?.hasPrefix("합치기·승격이 남았습니다") == true && h.removed.isEmpty)
    }
}

// MARK: - 이어 간 줄 밖의 열린 미룬 몫

struct OpenParkedRow: Row {
    let note: String
    let rows: [(String, String, String, String, Double)]   // (id, 실행, kind, 제목, 실행 출발)
    let answers: [String: String]
    let chain: Set<String>
    let shown: [(String, String)]
    let want: [String]   // id
}
private let openRows: [OpenParkedRow] = [
    .init(note: "줄 밖 실행의 미룬 몫은 보인다, 줄 안은 뺀다", rows: [("A/1/next_start/0", "A", "next_start", "재시도 횟수", 10), ("C/2/next_start/0", "C", "next_start", "알림 주소", 30)],
          answers: [:], chain: ["C"], shown: [], want: ["A/1/next_start/0"]),
    .init(note: "새로 찾은 일은 미룬 몫이 아니다", rows: [("A/1/new_work/0", "A", "new_work", "페이지 나누기", 10)], answers: [:], chain: [], shown: [], want: []),
    .init(note: "어느 실행에서든 정함·닫음으로 답했으면 뺀다, 다시 미룸은 열린 채", rows: [("A/1/next_start/0", "A", "next_start", "가", 10),
          ("A/1/next_start/1", "A", "next_start", "나", 10), ("A/2/next_start/0", "A", "next_start", "다", 10)],
          answers: ["A/1/next_start/0": "decide", "A/1/next_start/1": "close", "A/2/next_start/0": "park"], chain: [], shown: [], want: ["A/2/next_start/0"]),
    .init(note: "이미 카드로 가져온 것(같은 id·같은 제목)은 뺀다", rows: [("A/1/next_start/0", "A", "next_start", "재시도 횟수", 10), ("B/1/next_start/0", "B", "next_start", "알림 주소", 20)],
          answers: [:], chain: [], shown: [("A/1/next_start/0", "x"), ("C/9/next_start/0", "알림 주소")], want: []),
    .init(note: "같은 제목이 여럿이면 최근 실행의 것 하나, 최근부터", rows: [("A/1/next_start/0", "A", "next_start", "재시도 횟수", 10), ("B/3/next_start/0", "B", "next_start", "재시도 횟수", 20),
          ("A/2/next_start/0", "A", "next_start", "운영 값", 10)], answers: [:], chain: [], shown: [], want: ["B/3/next_start/0", "A/2/next_start/0"]),
]

@Suite("자율주행 이어 가기: 이어 간 줄 밖의 열린 미룬 몫") struct OpenParkedTests {
    @Test("열린 것만", arguments: openRows)
    func 표(_ r: OpenParkedRow) {
        let rows = r.rows.map { APCarryRow(id: $0.0, run: $0.1, unit: 1, kind: $0.2, title: $0.3, why: "", started: $0.4) }
        #expect(apOpenParked(rows: rows, answers: r.answers, chain: r.chain, shown: r.shown.map { (id: $0.0, title: $0.1) }).map(\.id) == r.want)
    }
}
