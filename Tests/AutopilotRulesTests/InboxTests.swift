// 방향을 바꿀 말 규칙(Sources/AutopilotRules/Inbox.swift)과 멈추고 다시 출발 규칙(Control.swift 끝) 시험 — 2026-10-30 2단계 5-4 에서 새로 씀.
// 가짜 어댑터(inbox/ 폴더·보드 쓰기·시계)는 이 파일에만 둔다
import Foundation
import Testing
import AutopilotRules

// MARK: - 글 꼴

struct ParseRow: Row {
    let note: String
    let raw: String
    let want: String    // "to unit at | 본문" 또는 "bad: 까닭"
}
private func show(_ p: APInboxParse) -> String {
    switch p {
    case .note(let n): return "\(n.to.rawValue) \(n.unit.map(String.init) ?? "-") \(n.at.map { "\($0)" } ?? "-") | \(n.text)"
    case .bad(let w): return "bad: " + w
    }
}
private let parseRows: [ParseRow] = [
    .init(note: "보드가 쓰는 꼴", raw: "to: now\nunit: 3\nat: 1761800000.500\n\n정렬은 화면 순서와 같게\n", want: "now 3 1761800000.5 | 정렬은 화면 순서와 같게"),
    .init(note: "다음 단위부터, unit 없음, 여러 줄 본문", raw: "to: next\n\n첫 줄\n둘째 줄\n", want: "next - - | 첫 줄\n둘째 줄"),
    .init(note: "머리 이름은 대소문자·앞뒤 빈칸을 가리지 않는다, 모르는 칸은 넘김", raw: " To : now \nfrom: board\n\n말\n", want: "now - - | 말"),
    .init(note: "CRLF 도 읽는다", raw: "to: now\r\n\r\n말\r\n", want: "now - - | 말"),
    .init(note: "to 없음", raw: "unit: 3\n\n말\n", want: "bad: 머리에 to: now 또는 to: next 가 없습니다"),
    .init(note: "to 가 다른 값", raw: "to: later\n\n말\n", want: "bad: 머리 to 는 now 또는 next 입니다(later)"),
    .init(note: "unit 이 0", raw: "to: now\nunit: 0\n\n말\n", want: "bad: 머리 unit 은 1 이상의 정수입니다(0)"),
    .init(note: "at 이 수가 아님", raw: "to: now\nat: 어제\n\n말\n", want: "bad: 머리 at 은 수입니다(어제)"),
    .init(note: "빈 줄이 없음", raw: "to: now", want: "bad: 머리와 본문 사이 빈 줄이 없습니다"),
    .init(note: "머리 줄 꼴이 아님", raw: "그냥 글\n\n말\n", want: "bad: 머리 줄 꼴이 아닙니다: 그냥 글"),
    .init(note: "본문이 빈칸뿐", raw: "to: next\n\n   \n\n", want: "bad: 본문이 비었습니다"),
    .init(note: "본문이 상한보다 김", raw: "to: now\n\n" + String(repeating: "가", count: 2001), want: "bad: 본문이 2000자보다 깁니다(2001자)"),
    .init(note: "본문이 상한과 같으면 받는다", raw: "to: now\n\n" + String(repeating: "가", count: 2000), want: "now - - | " + String(repeating: "가", count: 2000)),
]

@Suite("자율주행 방향을 바꿀 말: 글 꼴") struct InboxParseTests {
    @Test("엔진이 읽는 글", arguments: parseRows)
    func 읽기(_ r: ParseRow) { #expect(show(apInboxParse(r.raw)) == r.want) }

    @Test("보드가 쓴 글을 엔진이 그대로 읽는다", arguments: [
        APInboxNote(to: .now, unit: 2, at: 1761800000.25, text: "  한 줄  \n"),
        APInboxNote(to: .next, text: "여러\r\n줄"),
    ])
    func 되읽기(_ n: APInboxNote) {
        guard case .note(let back) = apInboxParse(apInboxCompose(n)) else { Issue.record("읽지 못함"); return }
        #expect(back.to == n.to && back.unit == n.unit && back.at == n.at)
        #expect(back.text == n.text.replacingOccurrences(of: "\r\n", with: "\n").trimmingCharacters(in: .whitespacesAndNewlines))
    }

    @Test("파일 이름 꼴", arguments: [
        ("20261030-140512-123.md", true), ("20261030-140512-123-2.md", true), ("a.md", true),
        (".tmp-20261030-140512-123.md", false), ("note.txt", false), ("../x.md", false), ("한글.md", false), (".md", false),
    ])
    func 이름(_ a: (String, Bool)) { #expect(apInboxNameOK(a.0) == a.1) }
}

// MARK: - 어느 단위에 붙일지

struct BindRow: Row {
    let note: String
    let to: APInboxTo
    let seen: Int?
    let statuses: String     // 단위 상태를 빈칸으로(d done, r running, p pending, f failed, s skipped)
    let want: String         // "waiting unit:n" · "waiting from:n" · "late"
}
private func sts(_ s: String) -> [String] {
    s.split(separator: " ").map { ["d": "done", "r": "running", "p": "pending", "f": "failed", "s": "skipped", "c": "capped"][String($0)] ?? String($0) }
}
private let bindRows: [BindRow] = [
    .init(note: "지금 도는 단위", to: .now, seen: nil, statuses: "d r p", want: "waiting unit:2"),
    .init(note: "보낼 때 본 단위가 아직 돈다", to: .now, seen: 2, statuses: "d r p", want: "waiting unit:2"),
    .init(note: "보낼 때 본 단위가 끝났으면 다른 단위에 넘기지 않는다", to: .now, seen: 1, statuses: "d r p", want: "late"),
    .init(note: "보낼 때 본 단위가 아직 시작 전이면 그 단위", to: .now, seen: 3, statuses: "d d p", want: "waiting unit:3"),
    .init(note: "없는 단위", to: .now, seen: 9, statuses: "d r", want: "late"),
    .init(note: "도는 단위가 없으면(단위 사이·멈출 선 대기) 다음에 시작할 단위", to: .now, seen: nil, statuses: "d f p p", want: "waiting unit:3"),
    .init(note: "남은 단위가 없음", to: .now, seen: nil, statuses: "d d", want: "late"),
    .init(note: "다음 단위부터: 도는 단위 뒤의 첫 대기", to: .next, seen: nil, statuses: "d r p p", want: "waiting from:3"),
    .init(note: "다음 단위부터: 뒤 단위가 건너뜀이면 그 뒤 대기", to: .next, seen: 2, statuses: "d r s p", want: "waiting from:4"),
    .init(note: "다음 단위부터: 도는 단위가 없으면 첫 대기", to: .next, seen: nil, statuses: "d d p", want: "waiting from:3"),
    .init(note: "다음 단위부터: 뒤에 남은 단위가 없음", to: .next, seen: nil, statuses: "d r", want: "late"),
]

@Suite("자율주행 방향을 바꿀 말: 어느 단위에") struct InboxBindTests {
    @Test("받을 때 정하는 단위", arguments: bindRows)
    func 단위(_ r: BindRow) {
        let b = apInboxBind(APInboxNote(to: r.to, unit: r.seen, text: "말"), statuses: sts(r.statuses))
        let got = b.status == "late" ? "late" : (b.unit.map { "waiting unit:\($0)" } ?? b.from.map { "waiting from:\($0)" } ?? "?")
        #expect(got == r.want)
        if b.status == "late" { #expect(!(b.why ?? "").isEmpty) }
    }

    @Test("늦은 말의 까닭은 끝난 꼴을 쉬운 말로")
    func 까닭() {
        let b = apInboxBind(APInboxNote(to: .now, unit: 1, text: "말"), statuses: ["capped", "running"])
        #expect(b.why == "보낼 때 본 단위(단위 1)가 이미 끝나(상한에서 멈춤) 넘기지 않았습니다. 다음 단위부터로 다시 보내거나 멈추고 다시 출발합니다")
    }
}

// MARK: - 언제 넘길지

private func e(_ to: String, _ status: String, unit: Int? = nil, from: Int? = nil, units: [Int] = [], file: String = "f.md") -> APInboxEntry {
    APInboxEntry(file: file, to: to, text: "말 " + file, received: 0, status: status, unit: unit, from: from, units: units)
}

@Suite("자율주행 방향을 바꿀 말: 넘기기와 닫기") struct InboxDeliverTests {
    @Test("단위 n 에 넘길 지금 말과 붙일 방향")
    func 고르기() {
        let es = [e("now", "waiting", unit: 2, file: "a"), e("now", "delivered", unit: 2, units: [2], file: "b"), e("now", "waiting", unit: 3, file: "c"),
                  e("next", "waiting", from: 3, file: "d"), e("next", "applied", from: 2, units: [2], file: "e"), e("next", "late", file: "f"),
                  e("next", "unused", from: 2, file: "g")]
        #expect(apInboxNowFor(es, unit: 2) == [0])
        #expect(apInboxNowFor(es, unit: 3) == [2])
        #expect(apInboxNextFor(es, unit: 2) == [])          // e 는 이미 단위 2 에 붙였다, d 는 3 부터
        #expect(apInboxNextFor(es, unit: 3) == [3, 4])      // 붙인 방향은 뒤 단위에도
        #expect(apInboxNextFor(es, unit: 4) == [3, 4])
    }

    @Test("턴 뒤·판정 뒤", arguments: [
        (true, 0, 2, false, APAfterCheck.pass), (true, 0, 2, true, .note), (true, 2, 2, true, .note),
        (false, 0, 2, false, .retry), (false, 2, 2, false, .fail), (false, 2, 2, true, .retry), (false, 3, 2, false, .fail),
    ] as [(Bool, Int, Int, Bool, APAfterCheck)])
    func 판정뒤(_ a: (Bool, Int, Int, Bool, APAfterCheck)) {
        #expect(apAfterCheck(passed: a.0, tries: a.1, retries: a.2, waitingNow: a.3) == a.4)
    }

    @Test("턴 뒤: 그 사이 말이 왔으면 판정 전에 한 번 더")
    func 턴뒤() {
        #expect(apAfterTurn(waitingNow: true) == .again)
        #expect(apAfterTurn(waitingNow: false) == .check)
    }

    @Test("단위가 끝나면 그 단위에 넘기지 못한 말을 닫고, 실행이 끝나면 남은 것을 닫는다")
    func 닫기() {
        var es = [e("now", "waiting", unit: 2, file: "a"), e("now", "waiting", unit: 3, file: "b"), e("next", "waiting", from: 4, file: "c"),
                  e("next", "applied", from: 2, units: [2], file: "d"), e("now", "delivered", unit: 1, units: [1], file: "x")]
        #expect(apInboxCloseUnit(&es, unit: 2, ended: "capped") == 1)
        #expect(es[0].status == "dropped" && es[0].why == "넘기기 전에 그 단위(단위 2)가 끝나(상한에서 멈춤) 넘기지 못했습니다")
        #expect(es[1].status == "waiting")
        #expect(apInboxCloseRun(&es) == 2)
        #expect(es.map(\.status) == ["dropped", "dropped", "unused", "applied", "delivered"])
    }

    @Test("모델에게 넘기는 글 — 사람의 글을 들여 쓰고, 범위는 그대로라는 것을 붙인다")
    func 글() {
        let now = apInboxNowText(["정렬은 화면 순서", "둘째\n줄"])
        #expect(now.hasPrefix("사람이 실행을 지켜보다 이 단위에 한마디를 보냈습니다(보드의 \"지금 단위에\"):\n- 정렬은 화면 순서\n- 둘째\n  줄\n"))
        #expect(now.contains("판정 명령·쓰기 범위·가지 규칙은 출발 때 고정한 그대로"))
        #expect(now.contains("다른 가지 맞추기"))
        let next = apInboxNextText(["한국어로"])
        #expect(next.hasPrefix("사람이 실행 중에 보낸 방향(보드의 \"다음 단위부터\", 이 단위와 뒤 단위에 모두 붙습니다):\n- 한국어로\n"))
        #expect(next.contains("next_start"))
        #expect(!(now + next + apInboxPassedText).contains("—"))
    }

    @Test("보드에 보일 줄", arguments: [
        (e("now", "waiting", unit: 2), "기다림", "단위 2 의 다음 차례에 넘깁니다"),
        (e("next", "waiting", from: 3), "기다림", "단위 3 부터 첫 입력에 붙입니다"),
        (e("now", "delivered", unit: 2, units: [2]), "넘김", "단위 2 의 세션에 넘겼습니다"),
        (e("next", "applied", from: 2, units: [2, 3]), "붙임", "단위 2·3 의 첫 입력에 붙였습니다(뒤 단위에도 붙습니다)"),
    ] as [(APInboxEntry, String, String)])
    func 줄(_ a: (APInboxEntry, String, String)) {
        let l = apInboxLabel(a.0)
        #expect(l.status == a.1 && l.detail == a.2)
    }
}

// MARK: - 포트와 유스케이스(가짜 어댑터)

final class FakeInboxFiles: APInboxFiles, @unchecked Sendable {
    var files: [String: APInboxRead]
    init(_ files: [String: APInboxRead]) { self.files = files }
    func names() -> [String] { Array(files.keys) }
    func read(_ name: String) -> APInboxRead { files[name] ?? .notFile }
}
struct FixedClock: APClock { let now: Date }
final class FakeInboxOut: APInboxOut, @unchecked Sendable {
    var written: [String: String] = [:]
    var fail = false
    init(_ existing: [String] = []) { for n in existing { written[n] = "" } }
    func exists(_ name: String) -> Bool { written[name] != nil }
    func write(_ name: String, _ data: Data) -> Bool { if fail { return false }; written[name] = String(decoding: data, as: UTF8.self); return true }
}

@Suite("자율주행 방향을 바꿀 말: 받기와 보내기") struct InboxUseCaseTests {
    @Test("엔진이 새 파일만 이름 차례로 받고, 읽지 못한 파일도 bad 로 남긴다")
    func 받기() {
        let files = FakeInboxFiles([
            "20261030-000002-000.md": .text("to: next\n\n방향\n"),
            "20261030-000001-000.md": .text("to: now\n\n지금\n"),
            "20261030-000003-000.md": .tooBig(99_999),
            "20261030-000004-000.md": .notFile,
            "20261030-000005-000.md": .text("to: now\n"),
            ".tmp-20261030-000006-000.md": .text("to: now\n\n반쯤\n"),
            "old.md": .text("to: now\n\n이미 받음\n"),
        ])
        let known = [APInboxEntry(file: "old.md", to: "now", text: "이미 받음", received: 1, status: "delivered", unit: 1, units: [1])]
        let got = apInboxReceive(files: files, known: known, statuses: ["done", "running", "pending"], clock: FixedClock(now: Date(timeIntervalSince1970: 50)))
        #expect(got.map(\.file) == ["20261030-000001-000.md", "20261030-000002-000.md", "20261030-000003-000.md", "20261030-000004-000.md", "20261030-000005-000.md"])
        #expect(got.map(\.status) == ["waiting", "waiting", "bad", "bad", "bad"])
        #expect(got[0].unit == 2 && got[1].from == 3 && got.allSatisfy { $0.received == 50 })
        #expect(got[2].why == "파일이 16384바이트보다 커서(99999바이트) 읽지 않았습니다")
        #expect(got[4].why == "본문이 비었습니다")   // "to: now\n" 은 빈 줄 뒤 본문이 없는 것
    }

    @Test("보드가 보내기 — 이름이 겹치면 -2, 끝난 실행·빈 글·긴 글은 보내지 않는다")
    func 보내기() {
        let out = FakeInboxOut(["20261030-140512-123.md"])
        let r = apInboxSend(APInboxNote(to: .now, unit: 2, at: 5, text: "  말  "), runState: "running", stamp: "20261030-140512-123", out: out)
        #expect(r == .sent("20261030-140512-123-2.md"))
        #expect(out.written["20261030-140512-123-2.md"] == "to: now\nunit: 2\nat: 5.000\n\n말\n")
        #expect(apInboxSend(APInboxNote(to: .now, text: "말"), runState: "done", stamp: "x", out: out) == .refused("끝난 실행에는 보내지 않습니다"))
        #expect(apInboxSend(APInboxNote(to: .now, text: "말"), runState: "", stamp: "x", out: out) == .refused("끝난 실행에는 보내지 않습니다"))
        #expect(apInboxSend(APInboxNote(to: .next, text: " \n "), runState: "paused", stamp: "x", out: out) == .refused("보낼 말을 써 주세요"))
        #expect(apInboxSend(APInboxNote(to: .next, text: String(repeating: "a", count: 2001)), runState: "paused", stamp: "x", out: out)
                == .refused("2000자까지 보낼 수 있습니다(지금 2001자)"))
        out.fail = true
        #expect(apInboxSend(APInboxNote(to: .next, text: "말"), runState: "running", stamp: "y", out: out) == .refused("inbox 에 쓰지 못했습니다"))
    }

    @Test("받았을 때 진행 기록 한 줄")
    func 기록() {
        var w = e("now", "waiting", unit: 2); w.text = "정렬\n순서"
        #expect(apInboxReceivedText(w, unitRunning: true) == "사람이 보낸 말을 받았습니다(지금 단위에 · 단위 2): «정렬 순서». 이 세션의 다음 차례에 넘깁니다.")
        #expect(apInboxReceivedText(w, unitRunning: false).hasSuffix("그 단위가 시작할 때 첫 입력에 붙입니다."))
        var long = e("next", "waiting", from: 3); long.text = String(repeating: "가", count: 90)
        #expect(apInboxReceivedText(long, unitRunning: false).contains("«" + String(repeating: "가", count: 80) + "…»"))
    }
}

// MARK: - 멈추고 다시 출발

@Suite("자율주행 멈추고 다시 출발") struct RestartTests {
    @Test("언제 멈추는가", arguments: [
        ("d r p", false, APRestartPlan.afterUnit(2)), ("d d p", false, .now), ("d d", false, .now), ("r p", true, .already), ("", false, .now),
    ] as [(String, Bool, APRestartPlan)])
    func 계획(_ a: (String, Bool, APRestartPlan)) { #expect(apRestartPlan(statuses: sts(a.0), asked: a.1) == a.2) }

    @Test("진행 기록 글")
    func 글() {
        #expect(apRestartText(.afterUnit(2), paused: false, unit: nil) == "멈추고 다시 출발 요청을 받았습니다. 지금 단위(단위 2)를 마친 뒤 멈추고, 새 단위는 시작하지 않습니다.")
        #expect(apRestartText(.afterUnit(2), paused: true, unit: nil).contains("재개하면 그 단위(단위 2)를 마친 뒤 멈춥니다"))
        #expect(apRestartText(.already, paused: false, unit: 3) == "멈추고 다시 출발 요청을 이미 받았습니다(그 단위(단위 3)를 마친 뒤 멈춤).")
    }

    @Test("새 단위 앞에서 멈추는가", arguments: [(true, true, true), (true, false, false), (false, true, false)] as [(Bool, Bool, Bool)])
    func 멈춤(_ a: (Bool, Bool, Bool)) { #expect(apRestartStopsBefore(asked: a.0, hasPending: a.1) == a.2) }

    @Test("멈추고 다시 출발로 멈춘 실행인가")
    func 가르기() {
        #expect(apIsRestartStop(state: "stopped", reason: apRestartReason))
        #expect(!apIsRestartStop(state: "stopped", reason: "사람이 중단했습니다."))
        #expect(!apIsRestartStop(state: "done", reason: apRestartReason))
    }

    struct ReopenRow: Row {
        let note: String
        var state = "stopped", wrapped = true, flag = true, asked = true, opened = false
        var project: String? = "todo"
        let want: Bool
    }
    @Test("보드가 출발 준비를 저절로 다시 여는 때", arguments: [
        ReopenRow(note: "보낸 보드, 끝 정리까지 마침", want: true),
        ReopenRow(note: "마지막 단위 중에 보내 끝남(done)이어도 연다", state: "done", want: true),
        ReopenRow(note: "아직 도는 중", state: "running", want: false),
        ReopenRow(note: "끝 정리 전(기록 저장소에 아직 없음)", wrapped: false, want: false),
        ReopenRow(note: "엔진이 받지 않음(표지 없음)", flag: false, want: false),
        ReopenRow(note: "다른 보드가 보냈거나 보드를 껐다 켬", asked: false, want: false),
        ReopenRow(note: "이미 열었음", opened: true, want: false),
        ReopenRow(note: "프로젝트 없는 실행은 이어 갈 수 없음", project: nil, want: false),
    ])
    func 다시열기(_ r: ReopenRow) {
        #expect(apReopenIntake(state: r.state, wrapped: r.wrapped, flag: r.flag, asked: r.asked, opened: r.opened, project: r.project) == r.want)
    }
}
