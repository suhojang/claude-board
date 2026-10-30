// 실행 보고서 틀(Report.swift)·흐름 그림(Sequence.swift)·글 다듬기와 이스케이프(Prose.swift) 시험 — 2026-10-30 3단계에서 새로 씀.
// 견본 기록(Report/sample-run.json, 장난감 값)과 악성 견본(Report/evil-run.json)은 엔진이 읽는 꼴(run·queue·engines·results)이다.
// 골든(Report/*.html)은 그 기록으로 그린 글자 그대로다. 틀을 고쳐 골든이 달라지면 화면으로 확인한 뒤
// `AP_REPORT_GOLDEN=write swift test …` 로 다시 쓰고 바뀐 줄을 함께 커밋한다(사람이 보고 고른 출력이라 시험이 저절로 고치지 않는다)
import Foundation
import Testing
import AutopilotRules

// MARK: - 견본 기록

private let reportDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("Report")
private let seoul = TimeZone(identifier: "Asia/Seoul")!

private func fixture(_ name: String) -> [String: Any] {
    guard let d = try? Data(contentsOf: reportDir.appendingPathComponent(name)),
          let j = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any] else { return [:] }
    return j
}

// 가짜 읽을 곳 — 견본 기록의 사전을 그대로. 결과 기록은 글(객체면 JSON 글로, 글이면 그대로)
final class FixtureSource: APReportSource {
    let j: [String: Any]
    let origin: APReportOrigin
    var reads: [String] = []
    init(_ j: [String: Any], origin: APReportOrigin = .folder) { self.j = j; self.origin = origin }
    func run() -> [String: Any]? { reads.append("run"); return j["run"] as? [String: Any] }
    func queue() -> [[String: Any]] { reads.append("queue"); return j["queue"] as? [[String: Any]] ?? [] }
    func engine(_ n: Int) -> [String: Any]? { reads.append("engine \(n)"); return (j["engines"] as? [String: Any])?[String(n)] as? [String: Any] }
    func result(_ n: Int) -> String? {
        reads.append("result \(n)")
        guard let o = (j["results"] as? [String: Any])?[String(n)] else { return nil }
        if let s = o as? String { return s }
        return (try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys])).map { String(decoding: $0, as: UTF8.self) }
    }
}

final class FakeSink: APReportSink {
    var fail: Set<APReportFile> = []
    var wrote: [APReportFile: String] = [:]
    func write(_ file: APReportFile, _ html: String) -> String? {
        if fail.contains(file) { return "쓰지 못했습니다(가짜)" }
        wrote[file] = html
        return nil
    }
}

private struct ReportClock: APClock { let now: Date }

private func render(_ name: String, only: Int? = nil, origin: APReportOrigin = .folder) -> String {
    let j = fixture(name)
    guard let r = apReportRun(id: j["id"] as? String ?? "?", source: FixtureSource(j, origin: origin)) else { return "" }
    return apReportHTML(r, only: only, at: (j["at"] as? NSNumber)?.doubleValue ?? 0, tz: seoul, home: j["home"] as? String)
}

// 골든과 견준다. AP_REPORT_GOLDEN=write 면 골든을 다시 쓴다
private func golden(_ name: String, _ html: String) {
    let url = reportDir.appendingPathComponent(name)
    if ProcessInfo.processInfo.environment["AP_REPORT_GOLDEN"] == "write" {
        try? Data(html.utf8).write(to: url)
        return
    }
    let want = (try? String(contentsOf: url, encoding: .utf8)) ?? "(골든 \(name) 없음)"
    if html != want {
        let a = html.components(separatedBy: "\n"), b = want.components(separatedBy: "\n")
        let k = zip(a, b).enumerated().first { $0.element.0 != $0.element.1 }?.offset ?? min(a.count, b.count)
        Issue.record("골든 \(name) 와 다름: \(k + 1)번째 줄\n  지금: \(k < a.count ? String(a[k].prefix(300)) : "(없음)")\n  골든: \(k < b.count ? String(b[k].prefix(300)) : "(없음)")")
    }
}

// MARK: - 글 다듬기(긴 줄표)

struct PlainRow: Row { let note: String; let s: String; let want: String }
private let plainRows: [PlainRow] = [
    .init(note: "빈 글", s: "", want: ""),
    .init(note: "줄표 없음", s: "그냥 글", want: "그냥 글"),
    .init(note: "앞뒤 빈칸 있는 줄표", s: "A — B", want: "A, B"),
    .init(note: "붙은 줄표", s: "A—B", want: "A, B"),
    .init(note: "3단계 고침: 앞에만 빈칸(전에는 \"A , B\")", s: "A —B", want: "A, B"),
    .init(note: "3단계 고침: 뒤에만 빈칸(전에는 빈칸 둘)", s: "A— B", want: "A, B"),
    .init(note: "3단계 고침: 줄 맨 앞 줄표는 지움(전에는 \",  앞\")", s: "— 앞", want: "앞"),
    .init(note: "3단계 고침: 줄 끝 줄표는 지움(전에는 \"끝 , \")", s: "끝 —", want: "끝"),
    .init(note: "3단계 고침: 이어 쓴 줄표는 하나로", s: "A —— B", want: "A, B"),
    .init(note: "3단계 고침: 둘째 줄 맨 앞", s: "줄 하나\n— 둘째 줄", want: "줄 하나\n둘째 줄"),
    .init(note: "여러 개", s: "x — y — z", want: "x, y, z"),
    .init(note: "줄표 하나뿐", s: "—", want: ""),
    .init(note: "엔 대시(범위)는 그대로", s: "1–3 단계", want: "1–3 단계"),
    .init(note: "가로 막대(U+2015)는 그대로", s: "A ― B", want: "A ― B"),
]

@Suite("자율주행 보고서: 모델 글의 긴 줄표") struct PlainTests {
    @Test("쉼표로 바꾸고 줄 앞뒤는 지움", arguments: plainRows)
    func 표(_ r: PlainRow) { #expect(apPlain(r.s) == r.want) }
}

// MARK: - 이스케이프와 링크

struct EscapeRow: Row { let note: String; let s: String; let want: String }
private let escapeRows: [EscapeRow] = [
    .init(note: "스크립트 태그", s: "<script>alert('x')</script>", want: "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"),
    .init(note: "속성 탈출(큰따옴표)", s: "\" onmouseover=\"x", want: "&quot; onmouseover=&quot;x"),
    .init(note: "속성 탈출(작은따옴표)", s: "' onfocus='x", want: "&#39; onfocus=&#39;x"),
    .init(note: "& 는 먼저", s: "a & b &amp;", want: "a &amp; b &amp;amp;"),
    .init(note: "탭·줄바꿈은 그대로", s: "가\t나\n다", want: "가\t나\n다"),
    .init(note: "방향 바꾸기 글자는 지움", s: "\u{202E}txt.exe\u{2066}x\u{2069}", want: "txt.exex"),
    .init(note: "제어 문자·DEL·C1 은 지움", s: "\u{07}벨\u{7F}\u{85}끝", want: "벨끝"),
    .init(note: "CR 은 지우고 LF 만", s: "a\r\nb", want: "a\nb"),
    .init(note: "줄·문단 구분자는 줄바꿈", s: "a\u{2028}b\u{2029}c", want: "a\nb\nc"),
    .init(note: "그림 글자·한글은 그대로", s: "👍 좋음", want: "👍 좋음"),
]

struct LinkRow: Row { let note: String; let s: String; let want: String? }
private let linkRows: [LinkRow] = [
    .init(note: "https 주소", s: "https://example.com/a?b=1&c=2#x", want: "https://example.com/a?b=1&c=2#x"),
    .init(note: "앞뒤 빈칸은 다듬음", s: "  https://example.com/  ", want: "https://example.com/"),
    .init(note: "대문자 꼴", s: "HTTPS://Example.com", want: "HTTPS://Example.com"),
    .init(note: "http 는 글로만", s: "http://example.com", want: nil),
    .init(note: "javascript:", s: "javascript:alert(1)", want: nil),
    .init(note: "대소문자 섞은 javascript:", s: " JaVaScRiPt:alert(1)", want: nil),
    .init(note: "data:", s: "data:text/html,<script>alert(1)</script>", want: nil),
    .init(note: "file:", s: "file:///etc/passwd", want: nil),
    .init(note: "꾸러미 없는 상대 주소", s: "//example.com/x", want: nil),
    .init(note: "호스트 없음", s: "https://", want: nil),
    .init(note: "사용자 정보", s: "https://user:pw@example.com/", want: nil),
    .init(note: "빈칸", s: "https://exa mple.com/", want: nil),
    .init(note: "속성 탈출 따옴표", s: "https://example.com/\"onmouseover=\"x", want: nil),
    .init(note: "역슬래시", s: "https://example.com\\@evil.example/", want: nil),
    .init(note: "꺾쇠", s: "https://example.com/<x>", want: nil),
    .init(note: "ASCII 밖 글자", s: "https://예시.com/", want: nil),
    .init(note: "빈 글", s: "", want: nil),
]

@Suite("자율주행 보고서: 이스케이프와 링크") struct EscapeTests {
    @Test("HTML 글·속성 값", arguments: escapeRows)
    func 이스케이프(_ r: EscapeRow) { #expect(apEscapeHTML(r.s) == r.want) }

    @Test("링크로 걸 주소는 https 만", arguments: linkRows)
    func 링크(_ r: LinkRow) { #expect(apSafeLink(r.s) == r.want) }
}

// MARK: - 흐름 그림(sequenceDiagram)

// 그림 값을 견주기 쉬운 글로: "P:A,@U=사용자 | 0->>1:글 · N over 0,1:글 · [alt 라벨 · |else 라벨 · ] · #"
private func describe(_ p: APSeqParse) -> String {
    switch p {
    case .unsupported(let line, let why): return "못 그림 \(line): \(why)"
    case .ok(let d):
        let ps = d.participants.map { ($0.actor ? "@" : "") + $0.id + ($0.label == $0.id ? "" : "=" + $0.label) }.joined(separator: ",")
        let items = d.items.map { it -> String in
            switch it {
            case .message(let a, let b, let t, let dotted, let head, let both):
                let arrow = (both ? "<" : "") + (dotted ? "--" : "-") + ["none": "", "arrow": ">>", "cross": "x", "open": ")"][head.rawValue]!
                return "\(a)\(arrow)\(b):\(t)"
            case .note(let at, let side, let t): return "N \(side.rawValue) \(at.map(String.init).joined(separator: ",")):\(t)"
            case .begin(let k, let l): return "[\(k)" + (l.isEmpty ? "" : " " + l)
            case .divide(let k, let l): return "|\(k)" + (l.isEmpty ? "" : " " + l)
            case .end: return "]"
            }
        }.joined(separator: " · ")
        return "P:\(ps) | \(items)" + (d.autonumber ? " | #" : "") + (d.title.isEmpty ? "" : " | 제목 " + d.title)
    }
}

struct SeqRow: Row { let note: String; let src: String; let want: String }
private let seqRows: [SeqRow] = [
    .init(note: "가장 작은 꼴", src: "sequenceDiagram\nA->>B: 안녕", want: "P:A,B | 0->>1:안녕"),
    .init(note: "선언한 차례와 이름, actor", src: "sequenceDiagram\n actor U as 사용자\n participant A as API 서버\n A->>U: 답",
          want: "P:@U=사용자,A=API 서버 | 1->>0:답"),
    .init(note: "선언 없이 나온 차례", src: "sequenceDiagram\nB->>A: x\nC-->>B: y", want: "P:B,A,C | 0->>1:x · 2-->>0:y"),
    .init(note: "화살표 열 가지", src: "sequenceDiagram\nA->B: 1\nA-->B: 2\nA->>B: 3\nA-->>B: 4\nA-xB: 5\nA--xB: 6\nA-)B: 7\nA--)B: 8\nA<<->>B: 9\nA<<-->>B: 10",
          want: "P:A,B | 0-1:1 · 0--1:2 · 0->>1:3 · 0-->>1:4 · 0-x1:5 · 0--x1:6 · 0-)1:7 · 0--)1:8 · 0<->>1:9 · 0<-->>1:10"),
    .init(note: "켜기 표지 +·- 는 무시", src: "sequenceDiagram\nA->>+B: 가\nB-->>-A: 나", want: "P:A,B | 0->>1:가 · 1-->>0:나"),
    .init(note: "쌍점 뒤의 화살표 꼴은 글", src: "sequenceDiagram\nA->>B: a->b 로", want: "P:A,B | 0->>1:a->b 로"),
    .init(note: "글 없는 메시지", src: "sequenceDiagram\nA->>B", want: "P:A,B | 0->>1:"),
    .init(note: "Note 셋", src: "sequenceDiagram\nparticipant A\nparticipant B\nNote left of A: 왼\nnote right of B: 오\nNote over A,B: 둘",
          want: "P:A,B | N left 0:왼 · N right 1:오 · N over 0,1:둘"),
    .init(note: "묶음과 갈래, 빈 줄·주석·끝 쌍반점", src: "sequenceDiagram\n%% 주석\n\nalt 맞음;\nA->>B: x\nelse 틀림\nA-xB: y\nend\nloop 매시\nB->>B: 다시\nend",
          want: "P:A,B | [alt 맞음 · 0->>1:x · |else 틀림 · 0-x1:y · ] · [loop 매시 · 1->>1:다시 · ]"),
    .init(note: "par/and·critical/option·break·opt", src: "sequenceDiagram\npar 가\nA->>B: 1\nand 나\nA->>B: 2\nend\ncritical 꼭\nA->>B: 3\noption 실패\nA->>B: 4\nend\nbreak 멈춤\nA->>B: 5\nend\nopt 골라\nA->>B: 6\nend",
          want: "P:A,B | [par 가 · 0->>1:1 · |and 나 · 0->>1:2 · ] · [critical 꼭 · 0->>1:3 · |option 실패 · 0->>1:4 · ] · [break 멈춤 · 0->>1:5 · ] · [opt 골라 · 0->>1:6 · ]"),
    .init(note: "rect·box 의 색 글자는 버림", src: "sequenceDiagram\nbox rgb(1,2,3) 묶음\nparticipant A\nend\nrect rgba(0,0,255,.1)\nA->>A: x\nend",
          want: "P:A | [box · ] · [rect · 0->>0:x · ]"),
    .init(note: "닫지 않은 묶음은 끝에서 닫음", src: "sequenceDiagram\nloop 계속\nA->>B: x", want: "P:A,B | [loop 계속 · 0->>1:x · ]"),
    .init(note: "autonumber·title·activate·destroy·create", src: "sequenceDiagram\ntitle: 제목\nautonumber\ncreate participant C as 새것\nactivate A\nA->>C: x\ndeactivate A\ndestroy C",
          want: "P:C=새것,A | 1->>0:x | # | 제목 제목"),
    .init(note: "다른 그림 꼴", src: "graph TD; A-->B; click A \"javascript:alert(1)\"", want: "못 그림 1: sequenceDiagram 으로 시작하지 않습니다"),
    .init(note: "빈 원문", src: "  \n", want: "못 그림 1: 비어 있습니다"),
    .init(note: "참여자 없음", src: "sequenceDiagram\nautonumber", want: "못 그림 1: 참여자가 없습니다"),
    .init(note: "읽지 못한 줄은 그리지 않음(말없이 빠지지 않게)", src: "sequenceDiagram\nA->>B: x\nclick A call()", want: "못 그림 3: 읽지 못한 줄입니다"),
    .init(note: "link 줄", src: "sequenceDiagram\nA->>B: x\nlink A: 대시보드 @ https://example.com", want: "못 그림 3: 읽지 못한 줄입니다"),
    .init(note: "alt 밖의 else", src: "sequenceDiagram\nA->>B: x\nelse 틀림", want: "못 그림 3: else 가 맞는 묶음 안에 있지 않습니다"),
    .init(note: "여는 묶음 없는 end", src: "sequenceDiagram\nA->>B: x\nend", want: "못 그림 3: 여는 묶음 없이 end 가 있습니다"),
    .init(note: "Note 대상 셋", src: "sequenceDiagram\nNote over A,B,C: x", want: "못 그림 2: Note 줄을 읽지 못했습니다"),
    .init(note: "Note 쌍점 없음", src: "sequenceDiagram\nNote over A x", want: "못 그림 2: Note 줄에 쌍점(:)이 없습니다"),
    .init(note: "참여자 이름에 꺾쇠·따옴표", src: "sequenceDiagram\nparticipant \"A\"", want: "못 그림 2: 참여자 이름을 읽지 못했습니다"),
    .init(note: "참여자 25명", src: "sequenceDiagram\n" + (1...25).map { "participant P\($0)" }.joined(separator: "\n"), want: "못 그림 26: 참여자가 너무 많습니다(24까지)"),
    .init(note: "줄 301개", src: "sequenceDiagram\n" + String(repeating: "A->>B: x\n", count: 301), want: "못 그림 302: 줄이 너무 많습니다(300줄까지 그립니다)"),
]

@Suite("자율주행 보고서: 흐름 그림") struct SequenceTests {
    @Test("sequenceDiagram 의 작은 부분만 읽고 그 밖은 원문으로", arguments: seqRows)
    func 읽기(_ r: SeqRow) { #expect(describe(apParseSequence(r.src)) == r.want) }

    @Test("SVG: 모든 글은 이스케이프, 바깥 참조·id 없음, 같은 값이면 같은 글자")
    func 그리기() throws {
        guard case .ok(let d) = apParseSequence("sequenceDiagram\nparticipant A as <script>x</script>\nA->>B: <img src=x onerror=alert(1)>\nNote over A: </text><b>\nB->>B: 혼자 — 다시\nalt 갈래\nA-xB: 끝\nend") else {
            Issue.record("읽지 못함"); return
        }
        let s = apSequenceSVG(d, label: "그림 \"하나\"")
        #expect(s == apSequenceSVG(d, label: "그림 \"하나\""))
        #expect(s.hasPrefix("<svg class=\"sq\" viewBox=\"0 0 ") && s.hasSuffix("</svg>"))
        #expect(!s.contains("<script") && !s.contains("<img") && !s.contains("</text><b>") && !s.contains("—"))
        #expect(s.contains("&lt;script&gt;x&lt;/script&gt;") && s.contains("혼자, 다시") && s.contains("aria-label=\"그림 &quot;하나&quot;\""))
        #expect(!s.contains(" id=") && !s.contains("href") && !s.contains("url(") && !s.contains("marker"))
        #expect(s.contains("<polyline class=\"ml\" points=") && s.contains("class=\"fr\"") && s.contains("class=\"nb\""))
        #expect(apSequenceSVG(APSeqDiagram(), label: "빈 그림") == "")
    }

    @Test("긴 메시지는 참여자 사이를 넓힌다")
    func 넓히기() throws {
        func width(_ src: String) -> Int {
            guard case .ok(let d) = apParseSequence(src) else { return -1 }
            let s = apSequenceSVG(d, label: "")
            guard let r = s.range(of: "width=\""), let e = s[r.upperBound...].firstIndex(of: "\"") else { return -1 }
            return Int(s[r.upperBound..<e]) ?? -1
        }
        let short = width("sequenceDiagram\nA->>B: 짧음"), long = width("sequenceDiagram\nA->>B: " + String(repeating: "아주 긴 메시지 ", count: 8))
        #expect(short > 0 && long > short + 300)
    }

    @Test("글자 폭 어림: 한글 1em, 그 밖 0.6em")
    func 폭() {
        #expect(apTextWidth("가나", size: 10) == 20)
        #expect(apTextWidth("ab", size: 10) == 12)
        #expect(apTextWidth("", size: 10) == 0)
    }
}

// MARK: - 결과 기록 읽기

@Suite("자율주행 보고서: 결과 기록 읽기") struct ResultParseTests {
    @Test("JSON 객체가 아니면 nil", arguments: [nil, "", "[1,2]", "글", "{\"headline\":"])
    func 아님(_ t: String?) { #expect(apParseResult(t) == nil) }

    @Test("칸의 꼴이 틀리면 그 칸만 비우고, 참고는 키 차례")
    func 꼴() throws {
        let r = try #require(apParseResult(#"""
            {"unit": "글", "outcome": 3, "headline": {"x": 1}, "summary": ["가", 2, null, {"y": 1}, true, ""],
             "tests": [{"name": "시험", "result": "pass"}, "글"], "changes": {"what": "x"}, "refs": {"z": "끝", "a": "https://example.com", "m": ["x"], "b": 7},
             "sequence": [{"title": "그림"}]}
            """#))
        #expect(r.title == "" && r.outcome == "3" && r.headline == "")
        #expect(r.summary == ["가", "2"])
        #expect(r.tests == [APResultTest(name: "시험", result: "pass", note: "")])
        #expect(r.changes.isEmpty)
        #expect(r.refs.map(\.key) == ["a", "b", "z"] && r.refs[1].value == "7")
        #expect(r.flows == [APResultFlow(title: "그림", source: "")])
    }

    @Test("긴 글과 긴 목록은 자른다")
    func 상한() throws {
        let long = String(repeating: "가", count: apReportTextMax + 10)
        let many = (0..<(apReportListMax + 5)).map { "\"\($0)\"" }.joined(separator: ",")
        let r = try #require(apParseResult("{\"headline\": \"\(long)\", \"summary\": [\(many)]}"))
        #expect(r.headline.count == apReportTextMax + 1 && r.headline.hasSuffix("…"))
        #expect(r.summary.count == apReportListMax)
    }
}

// MARK: - 기록 → 보고서 값

@Suite("자율주행 보고서: 기록 읽기") struct ReportRunTests {
    @Test("견본 기록: 단위 상태는 queue 가 먼저, 판정·거부·결과 기록 상태")
    func 견본() throws {
        let r = try #require(apReportRun(id: "x", source: FixtureSource(fixture("sample-run.json"))))
        #expect(r.units.map(\.status) == ["done", "failed", "capped", "skipped"])
        #expect(r.units.map(\.resultState) == [.ok, .ok, .missing, .missing])
        #expect(r.units[1].checks.map(\.n) == [1, 2, 3] && r.units[1].checks.map(\.exit) == [1, 1, 1] && r.units[1].lastExit == 1)
        #expect(r.units[2].denials == ["Bash: git push origin live"])
        #expect(r.mode == "worktree" && r.repos == ["todo-api"] && r.bases == ["todo-api: live"] && r.work == "autopilot/20261030-0900-todo-api")
        #expect(r.harnessSame == true && r.harnessFiles == 3 && r.moved.isEmpty && r.stashed == 0)
    }

    @Test("작업 키는 스토리가 있을 때만, 종류가 없으면 결과 기록의 것, 깨진 결과 기록")
    func 칸() throws {
        let src = FixtureSource([
            "run": ["title": "t", "state": "running"],
            "queue": [["title": "a", "status": "", "tickets": ["SHOP-2"]], ["title": "b", "story": "SHOP-1", "tickets": ["SHOP-3"]]],
            "engines": ["1": ["status": "running", "checks": [["exit": 0, "tail": String(repeating: "x", count: apReportTailMax + 50)]]]],
            "results": ["1": "{\"unit\": {\"kind\": \"fix\"}}", "2": "이건 JSON 이 아님"],
        ])
        let r = try #require(apReportRun(id: "x", source: src))
        #expect(r.units[0].status == "running" && r.units[0].task == "" && r.units[0].kind == "fix")
        #expect(r.units[0].checks[0].n == 1 && r.units[0].checks[0].tail.count == apReportTailMax + 1 && r.units[0].checks[0].tail.hasPrefix("…"))
        #expect(r.units[1].status == "pending" && r.units[1].task == "SHOP-3" && r.units[1].story == "SHOP-1")
        #expect(r.units[1].resultState == .broken && r.units[1].result == nil)
        #expect(src.reads == ["run", "queue", "engine 1", "result 1", "engine 2", "result 2"])
    }

    @Test("run 이 없으면 nil")
    func 없음() { #expect(apReportRun(id: "x", source: FixtureSource([:])) == nil) }
}

// MARK: - 보고서 HTML

// 태그·속성 허용 목록 — 틀이 쓰는 것만. 모델 글이 태그·속성을 하나라도 더 만들면 걸린다
private let allowedTags: Set<String> = ["html", "head", "meta", "title", "style", "body", "header", "nav", "section", "footer", "h1", "h2", "h3", "p",
                                        "span", "div", "dl", "dt", "dd", "ol", "ul", "li", "a", "table", "thead", "tbody", "tr", "th", "td", "code", "pre",
                                        "details", "summary", "figure", "figcaption", "svg", "rect", "line", "polyline", "polygon", "path", "text"]
private let allowedAttrs: Set<String> = ["lang", "charset", "name", "content", "http-equiv", "class", "id", "href", "rel", "aria-label", "role",
                                         "viewBox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "points", "d", "rx", "text-anchor"]

private func safety(_ html: String) -> [String] {
    var bad: [String] = []
    let tag = try! NSRegularExpression(pattern: "<([a-zA-Z][a-zA-Z0-9-]*)([^>]*)>")
    let attr = try! NSRegularExpression(pattern: "\\s([^\\s=/>]+)(=\"([^\"]*)\")?")
    let ns = html as NSString
    for m in tag.matches(in: html, range: NSRange(location: 0, length: ns.length)) {
        let name = ns.substring(with: m.range(at: 1)).lowercased()
        if !allowedTags.contains(name) { bad.append("태그 \(name)") }
        let rest = ns.substring(with: m.range(at: 2)) as NSString
        for a in attr.matches(in: rest as String, range: NSRange(location: 0, length: rest.length)) {
            let an = rest.substring(with: a.range(at: 1))
            if !allowedAttrs.contains(an) { bad.append("속성 \(an)") }
            if an == "href", a.range(at: 3).location != NSNotFound {
                let v = rest.substring(with: a.range(at: 3))
                if !(v.hasPrefix("#unit-") || v.hasPrefix("https://")) { bad.append("href \(v)") }
            }
        }
    }
    if html.contains("—") { bad.append("긴 줄표") }
    if html.unicodeScalars.contains(where: { (0x202A...0x202E).contains($0.value) || (0x2066...0x2069).contains($0.value) }) { bad.append("방향 바꾸기 글자") }
    if html.components(separatedBy: "<style>").count != 2 { bad.append("style 태그 수") }
    if html.lowercased().components(separatedBy: "<script").count > 1 { bad.append("script") }
    if !html.contains("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\">") {
        bad.append("CSP 없음")
    }
    // 바깥 요청이 될 꼴(글이 아닌 자리): CSS 의 url(·@import, src= 속성
    let style = html.components(separatedBy: "<style>").dropFirst().first?.components(separatedBy: "</style>").first ?? ""
    if style.contains("url(") || style.contains("@import") { bad.append("CSS 바깥 참조") }
    return bad
}

@Suite("자율주행 보고서: HTML") struct ReportHTMLTests {
    @Test("견본 기록 실행 보고서 = 골든")
    func 견본() { golden("sample-report.html", render("sample-run.json")) }

    @Test("견본 기록 단위 보고서 = 골든")
    func 단위() { golden("sample-unit-2.html", render("sample-run.json", only: 2)) }

    @Test("악성 견본 = 골든")
    func 악성골든() { golden("evil-report.html", render("evil-run.json")) }

    @Test("악성 견본: 태그·속성은 틀의 것만, 링크는 https 만, 긴 줄표·방향 글자·스크립트 없음, CSP")
    func 악성() {
        let h = render("evil-run.json")
        #expect(safety(h) == [])
        #expect(h.contains("&lt;script&gt;alert(1)&lt;/script&gt; 실행, 줄표"))
        #expect(h.contains("<a href=\"https://example.com/ok?q=1&amp;r=2\" rel=\"noopener noreferrer\">"))
        #expect(!h.contains("href=\"https://evil.example") && !h.contains("href=\"http://") && !h.contains("user:pw@example.com/\""))
        #expect(h.contains("그리지 못해 원문을 보입니다(1번째 줄: sequenceDiagram 으로 시작하지 않습니다)"))
        #expect(!h.contains("/Users/toy/"))   // 홈 폴더는 ~ 로
    }

    @Test("견본 기록도 같은 안전 검사를 지난다")
    func 견본안전() {
        #expect(safety(render("sample-run.json")) == [])
        #expect(safety(render("sample-run.json", only: 1)) == [])
        #expect(safety(render("sample-run.json", origin: .records)) == [])
    }

    @Test("같은 기록이면 같은 글자, 시간대만 바꾸면 시각만 다름")
    func 결정적() {
        let a = render("sample-run.json"), b = render("sample-run.json")
        #expect(a == b)
        let j = fixture("sample-run.json")
        let r = apReportRun(id: "x", source: FixtureSource(j))!
        let utc = apReportHTML(r, at: 1_793_330_000, tz: TimeZone(identifier: "UTC")!, home: nil)
        #expect(utc.contains("2026년 10월 30일 00:00") && !utc.contains("2026년 10월 30일 09:00"))
    }

    @Test("기록 저장소 바탕이면 바닥 글이 알려 준다, 단위가 없으면 그 말")
    func 바탕() {
        #expect(render("sample-run.json", origin: .records).contains("바탕 기록 저장소(실행 폴더를 지워 판정 출력은 없습니다)"))
        var r = APReportRun(); r.id = "빈"
        let h = apReportHTML(r, at: 0, tz: seoul)
        #expect(h.contains("단위 기록이 없습니다") && !h.contains("<nav") && safety(h) == [])
    }
}

// MARK: - 유스케이스: 보고서 그리기

@Suite("자율주행 보고서: 그리기(가짜 어댑터)") struct ReportDrawTests {
    private let clock = ReportClock(now: Date(timeIntervalSince1970: 1_793_330_000))

    @Test("고른 단위와 실행 보고서를 쓴다")
    func 쓰기() {
        let sink = FakeSink()
        let d = apDrawReports(id: "20261030-0900-todo-api", source: FixtureSource(fixture("sample-run.json")), sink: sink, units: [2], clock: clock, tz: seoul, home: "/Users/toy")
        #expect(d.written == [.unit(2), .run] && d.errors.isEmpty)
        #expect(sink.wrote[.unit(2)]?.contains("<section id=\"unit-2\">") == true && sink.wrote[.unit(2)]?.contains("id=\"unit-1\"") == false)
        #expect(sink.wrote[.run]?.contains("<section id=\"unit-4\">") == true)
        #expect(sink.wrote[.run]?.contains("그린 때 2026년 10월 30일 12:13") == true)
    }

    @Test("쓸 곳이 실패하면 까닭을 남기고 나머지는 쓴다")
    func 실패() {
        let sink = FakeSink(); sink.fail = [.unit(1)]
        let d = apDrawReports(id: "x", source: FixtureSource(fixture("sample-run.json")), sink: sink, units: [1, 9], clock: clock, tz: seoul, home: nil)
        #expect(d.written == [.run])
        #expect(d.errors == ["단위 1 보고서: 쓰지 못했습니다(가짜)", "대기열에 없는 단위(9)라 그 단위 보고서를 그리지 않았습니다"])
    }

    @Test("run.json 이 없으면 아무것도 쓰지 않는다")
    func 없음() {
        let sink = FakeSink()
        let d = apDrawReports(id: "x", source: FixtureSource([:]), sink: sink, units: [1], clock: clock, tz: seoul, home: nil)
        #expect(d.written.isEmpty && sink.wrote.isEmpty && d.errors == ["실행 기록(run.json)을 읽지 못해 보고서를 그리지 않았습니다"])
    }
}
