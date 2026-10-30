// 자율주행 규칙(AutopilotRules) — 실행 보고서 틀(2026-10-30 3단계. 설계 6장 3단계, 5장 2·3절, 3장 "마감").
// 결과 기록(result.json, 모델이 씀)과 엔진 기록(run.json·queue.json·units/<n>/engine.json)을 값으로 받아 한 파일로 완결된 HTML 글자를 만든다.
// 바깥 글꼴·스크립트·스타일·그림을 쓰지 않는다(그림은 인라인 SVG, CSP 가 모든 바깥 요청을 막음). 밝은·어두운 화면은 CSS 변수와 prefers-color-scheme.
// 모델이 쓴 글은 모두 apPlain(긴 줄표) → apEscapeHTML 을 거치고, 링크는 apSafeLink(https 만)를 지난 주소에만 건다.
// 시각·시간대·홈 폴더는 값으로 받는다(규칙 안에서 Date()·TimeZone.current·NSHomeDirectory 를 부르지 않는다 — 같은 기록이면 같은 글자).
// 엔진(실행 폴더 → 파일, 보드 창이 꺼져 있어도)과 보드 뷰어(실행 폴더 또는 기록 저장소 → 화면)가 같은 틀을 쓴다. 읽기·쓰기는 포트(APReportSource·APReportSink)로만
import Foundation

// MARK: - 결과 기록 (result.json, 설계 5장 3절)

public struct APResultTest: Equatable, Sendable {
    public var name = "", result = "", note = ""
    public init(name: String = "", result: String = "", note: String = "") { self.name = name; self.result = result; self.note = note }
}
public struct APResultChange: Equatable, Sendable {
    public var what = "", place = ""
    public init(what: String = "", place: String = "") { self.what = what; self.place = place }
}
public struct APResultDecision: Equatable, Sendable {
    public var what = "", basis = ""
    public init(what: String = "", basis: String = "") { self.what = what; self.basis = basis }
}
public struct APResultFlow: Equatable, Sendable {
    public var title = "", source = ""
    public init(title: String = "", source: String = "") { self.title = title; self.source = source }
}
public struct APResultCarry: Equatable, Sendable {
    public var title = "", why = ""
    public init(title: String = "", why: String = "") { self.title = title; self.why = why }
}
// 실행 중에 새로 필요해진 티켓(4단계 ⑩-3) — 모델이 임시 번호(TMP-<단위>-<차례>)로 쓰고 결과 기록 new_tickets 에 남긴 것. 보드가 검수 뒤에 만든다
public struct APResultTicket: Equatable, Sendable {
    public var temp = "", title = "", why = ""
    public init(temp: String = "", title: String = "", why: String = "") { self.temp = temp; self.title = title; self.why = why }
}
public struct APResultRef: Equatable, Sendable {
    public var key = "", value = ""
    public init(key: String = "", value: String = "") { self.key = key; self.value = value }
}

public struct APResultRecord: Equatable, Sendable {
    public var title = "", kind = "", ref = ""          // unit 칸
    public var outcome = "", headline = "", next = ""
    public var summary: [String] = [], risks: [String] = []
    public var tests: [APResultTest] = []
    public var changes: [APResultChange] = []
    public var decisions: [APResultDecision] = []
    public var flows: [APResultFlow] = []
    public var nextStart: [APResultCarry] = [], newWork: [APResultCarry] = []
    public var newTickets: [APResultTicket] = []
    public var refs: [APResultRef] = []                 // 키 차례
    public init() {}
}

public let apReportTextMax = 4000      // 글 한 칸의 글자 수 상한(넘으면 자르고 …)
public let apReportListMax = 200       // 목록 한 칸의 줄 수 상한
public let apReportFlowMax = 20_000    // 흐름 그림 원문 글자 수 상한
public let apReportTailMax = 2000      // 판정 출력 끝부분 글자 수 상한

func apCut(_ s: String, _ n: Int) -> String { s.count > n ? String(s.prefix(n)) + "…" : s }

// 글 칸 하나 — 글자는 그대로, 숫자는 글로, 그 밖(배열·객체·null)은 빈 글자
func apReportText(_ v: Any?, _ n: Int = apReportTextMax) -> String {
    if let s = v as? String { return apCut(s, n) }
    if let x = v as? NSNumber, CFGetTypeID(x) != CFBooleanGetTypeID() { return x.stringValue }
    return ""
}

// 결과 기록 글 → 값. JSON 객체가 아니면 nil. 칸의 꼴이 틀리면(글 자리에 객체 등) 그 칸만 비운다. 긴 줄표는 그릴 때(apPlain) 바꾼다
public func apParseResult(_ text: String?) -> APResultRecord? {
    guard let t = text, let j = (try? JSONSerialization.jsonObject(with: Data(t.utf8))) as? [String: Any] else { return nil }
    func list(_ k: String) -> [[String: Any]] { Array((j[k] as? [Any] ?? []).prefix(apReportListMax)).compactMap { $0 as? [String: Any] } }
    var r = APResultRecord()
    let u = j["unit"] as? [String: Any] ?? [:]
    r.title = apReportText(u["title"], 300); r.kind = apReportText(u["kind"], 40); r.ref = apReportText(u["ref"], 300)
    r.outcome = apReportText(j["outcome"], 40)
    r.headline = apReportText(j["headline"])
    r.next = apReportText(j["next"])
    r.summary = Array((j["summary"] as? [Any] ?? []).prefix(apReportListMax)).map { apReportText($0) }.filter { !$0.isEmpty }
    r.risks = Array((j["risks"] as? [Any] ?? []).prefix(apReportListMax)).map { apReportText($0) }.filter { !$0.isEmpty }
    r.tests = list("tests").map { APResultTest(name: apReportText($0["name"], 500), result: apReportText($0["result"], 40), note: apReportText($0["note"])) }
    r.changes = list("changes").map { APResultChange(what: apReportText($0["what"]), place: apReportText($0["where"], 500)) }
    r.decisions = list("decisions_applied").map { APResultDecision(what: apReportText($0["what"]), basis: apReportText($0["basis"])) }
    r.flows = list("sequence").map { APResultFlow(title: apReportText($0["title"], 300), source: apReportText($0["mermaid"], apReportFlowMax)) }
    r.nextStart = list("next_start").map { APResultCarry(title: apReportText($0["title"]), why: apReportText($0["why"])) }
    r.newWork = list("new_work").map { APResultCarry(title: apReportText($0["title"]), why: apReportText($0["why"])) }
    r.newTickets = list("new_tickets").prefix(20).map { APResultTicket(temp: apReportText($0["temp"], 20), title: apReportText($0["title"], 300), why: apReportText($0["why"], 1000)) }
    r.refs = (j["refs"] as? [String: Any] ?? [:]).keys.sorted().prefix(50).compactMap { k in
        let v = apReportText((j["refs"] as? [String: Any])?[k], 1000)
        return v.isEmpty ? nil : APResultRef(key: apCut(k, 60), value: v)
    }
    return r
}

// MARK: - 보고서 값

public enum APResultState: String, Equatable, Sendable { case ok, missing, broken }

public struct APReportCheck: Equatable, Sendable {
    public var n = 0
    public var exit: Int?
    public var sec = 0.0
    public var tail = ""
    public var timedOut = false
    public var interrupted = false
    public init() {}
}

public struct APReportUnit: Equatable, Sendable {
    public var n = 0
    public var title = "", kind = "", status = "pending", reason = ""
    public var task = "", story = "", branch = "", merge = ""
    public var activeSec = 0.0, cost = 0.0
    public var turns = 0, retries = 0
    public var started: Double?, ended: Double?
    public var checks: [APReportCheck] = []
    public var lastExit: Int?
    public var denials: [String] = []
    public var result: APResultRecord?
    public var resultState = APResultState.missing
    public init() {}
}

public struct APReportRun: Equatable, Sendable {
    public var id = "", title = "", project = "", model = "", state = "", reason = ""
    public var started: Double?, ended: Double?
    public var mode = ""                 // worktree · in_place · 빈 글자(기록 없음)
    public var work = ""                 // 자율주행 가지
    public var repos: [String] = []      // 저장소 이름(마지막 경로 조각)
    public var bases: [String] = []      // "저장소: 출발 가지"
    public var harnessSame: Bool?
    public var harnessFiles = 0
    public var harnessChanged: [String] = []
    public var moved: [String] = []      // 움직인 기존 가지(0 이어야 함)
    public var stashed = 0
    public var epic = ""
    public var units: [APReportUnit] = []
    public var origin = APReportOrigin.folder
    public init() {}
}

// 읽을 곳·쓸 곳 포트(APReportSource·APReportSink)와 APReportFile 은 Ports.swift 에 있다

// MARK: - 기록 → 보고서 값

private func num(_ v: Any?) -> Double? {
    guard let x = v as? NSNumber, CFGetTypeID(x) != CFBooleanGetTypeID() else { return nil }
    return x.doubleValue
}
private func int(_ v: Any?) -> Int? { num(v).flatMap { $0.isFinite && abs($0) < 1e15 ? Int($0) : nil } }
private func str(_ v: Any?) -> String { v as? String ?? "" }
private func lastPart(_ p: String) -> String { (p as NSString).lastPathComponent }

// 기록을 읽어 보고서 값으로. run 이 없으면 nil. 단위는 queue 의 차례(1부터)이고, 상태는 queue 의 status(엔진 판정)가 먼저다
public func apReportRun(id: String, source: APReportSource) -> APReportRun? {
    guard let run = source.run() else { return nil }
    var r = APReportRun()
    r.origin = source.origin
    r.id = id
    r.title = apReportText(run["title"], 300)
    r.project = apReportText(run["project"], 120)
    r.model = apReportText(run["model"], 80)
    r.state = apReportText(run["state"], 40)
    r.reason = apReportText(run["reason"], 1000)
    r.started = num(run["started"]); r.ended = num(run["ended"])
    r.epic = apReportText(run["epic"], 80)
    let wt = run["worktree"] as? [String: Any] ?? [:]
    r.mode = str(wt["mode"])
    r.work = str(wt["branch"])
    let br = run["branches"] as? [String: Any] ?? [:]
    let repos = br["repos"] as? [String: Any] ?? [:]
    for k in repos.keys.sorted() {
        r.repos.append(lastPart(k))
        let e = repos[k] as? [String: Any] ?? [:]
        if let b = (e["base"] as? [String: Any])?["branch"] as? String { r.bases.append("\(lastPart(k)): \(b)") }
        if r.work.isEmpty, let w = e["work"] as? String { r.work = w }
    }
    if r.repos.isEmpty, let rp = wt["repo"] as? String { r.repos = [lastPart(rp)] }
    if r.repos.isEmpty { r.repos = (wt["repos"] as? [Any] ?? []).compactMap { $0 as? String }.map(lastPart).sorted() }
    r.moved = (br["moved"] as? [[String: Any]] ?? []).map { "\(lastPart(str($0["repo"]))) \(str($0["branch"]))" }
    r.stashed = (br["stashed"] as? [Any] ?? []).count
    if let h = run["harness"] as? [String: Any] {
        r.harnessFiles = int(h["files"]) ?? 0
        r.harnessSame = h["same"] as? Bool
        r.harnessChanged = (h["changed"] as? [[String: Any]] ?? []).prefix(30).map { c in
            let what = ["changed": "바뀜", "added": "더함", "removed": "없어짐"][str(c["change"])] ?? str(c["change"])
            return "\(str(c["path"])) (\(what))"
        }
    }
    for (i, q) in source.queue().enumerated() {
        let n = i + 1
        let e = source.engine(n) ?? [:]
        var u = APReportUnit()
        u.n = n
        u.title = apReportText(q["title"] ?? e["title"], 300)
        u.kind = apReportText(q["kind"], 40)
        u.status = str(q["status"]).isEmpty ? (str(e["status"]).isEmpty ? "pending" : str(e["status"])) : str(q["status"])
        u.reason = apReportText(e["reason"], 1000)
        u.story = apReportText(q["story"], 80)
        if !u.story.isEmpty, let t = (q["tickets"] as? [Any])?.first { u.task = apReportText(t, 80) }
        u.branch = str(e["branch"]).isEmpty ? str(q["branch"]) : str(e["branch"])
        u.merge = str((e["merge"] as? [String: Any])?["state"])
        u.activeSec = num(e["active_sec"]) ?? 0
        u.cost = num(e["cost_usd"]) ?? 0
        u.turns = int(e["turns"]) ?? 0
        u.retries = int(e["try"]) ?? 0
        u.started = num(e["started"]); u.ended = num(e["ended"])
        u.checks = (e["checks"] as? [[String: Any]] ?? []).prefix(50).enumerated().map { k, c in
            var x = APReportCheck()
            x.n = int(c["try"]).map { $0 + 1 } ?? k + 1
            x.exit = int(c["exit"])
            x.sec = num(c["sec"]) ?? 0
            let tail = apReportText(c["tail"], 100_000)
            x.tail = tail.count > apReportTailMax ? "…" + String(tail.suffix(apReportTailMax)) : tail
            x.timedOut = c["timed_out"] as? Bool ?? false
            x.interrupted = c["interrupted"] as? Bool ?? false
            return x
        }
        u.lastExit = u.checks.last?.exit ?? int(e["check_exit"])
        u.denials = (e["denials"] as? [[String: Any]] ?? []).prefix(30).map { d in
            [str(d["tool"]), apReportText(d["what"], 300)].filter { !$0.isEmpty }.joined(separator: ": ")
        }
        if let t = source.result(n) {
            if let p = apParseResult(t) { u.result = p; u.resultState = .ok } else { u.resultState = .broken }
        }
        if u.kind.isEmpty, let k = u.result?.kind { u.kind = k }
        r.units.append(u)
    }
    return r
}

// MARK: - 보고서 HTML

private let unitLabel = ["pending": "대기", "running": "진행 중", "done": "통과", "failed": "미통과", "capped": "상한에서 멈춤", "stopped": "중단", "skipped": "건너뜀"]
private let unitClass = ["done": "ok", "failed": "fail", "capped": "cap", "running": "run"]
private let runLabel = ["starting": "출발 중", "running": "도는 중", "waiting_limit": "한도 대기", "waiting_stopline": "멈출 선 대기", "waiting_precheck": "환경 확인 대기",
                        "paused": "일시정지", "stopped": "중단됨", "done": "끝남", "error": "오류로 멈춤"]
private let kindLabel = ["fix": "고침", "dev": "개발", "feature": "개발", "story": "개발", "test": "시험", "doc": "문서", "docs": "문서", "env": "환경"]
private let outcomeLabel = ["done": "끝냄", "failed": "끝내지 못함", "parked": "보류", "capped": "상한"]
private let testLabel = ["pass": ("통과", "ok"), "fail": ("실패", "fail"), "skip": ("건너뜀", "")]
private let mergeLabel = ["merged": "스토리 가지에 합침", "nothing": "합칠 커밋 없음", "not_merged": "스토리 가지에 합치지 않음"]

// 걸린 시간 — 1분 안이면 초, 한 시간 안이면 분, 그 위는 시간·분(보드 apDur 과 같은 꼴)
public func apReportDur(_ sec: Double) -> String {
    let s = max(0, Int(sec.isFinite ? sec : 0))
    if s == 0 { return "0분" }
    if s < 60 { return "\(s)초" }
    return s >= 3600 ? "\(s / 3600)시간 \(s % 3600 / 60)분" : "\(s / 60)분"
}

private func kind(_ k: String) -> String { kindLabel[k.lowercased()] ?? k }

private struct Writer {
    let tz: TimeZone
    let home: String?
    var o = ""
    // 모델·사람이 쓴 글(긴 줄표 바꿈) / 경로·가지 같은 이름(그대로)
    func t(_ s: String) -> String { apEscapeHTML(apPlain(s)) }
    func c(_ s: String) -> String { apEscapeHTML(short(s)) }
    // 홈 폴더를 ~ 로(보고서가 바깥에 첨부될 수 있어 사용자 이름이 든 경로를 줄인다)
    func short(_ s: String) -> String {
        guard let h = home, h.count > 1 else { return s }
        return s.replacingOccurrences(of: h + "/", with: "~/")
    }
    func date(_ ts: Double?, _ f: String) -> String {
        guard let ts, ts.isFinite else { return "때 모름" }
        let d = DateFormatter()
        d.locale = Locale(identifier: "ko_KR")
        d.timeZone = tz
        d.calendar = Calendar(identifier: .gregorian)
        d.dateFormat = f
        return d.string(from: Date(timeIntervalSince1970: ts))
    }
    mutating func add(_ s: String) { o += s }
}

private func chip(_ text: String, _ cls: String = "") -> String {
    "<span class=\"chip\(cls.isEmpty ? "" : " " + cls)\">\(text)</span>"
}

private let css = """
:root{color-scheme:light dark;--bg:#f6f7f8;--card:#ffffff;--fg:#1f2328;--muted:#57606a;--line:#d8dee4;--soft:#eef1f4;--code:#f1f3f5;--accent:#0b62d6;\
--ok:#1a7f37;--ok-bg:#dcf5e3;--fail:#c4252f;--fail-bg:#ffe9e8;--cap:#8a5a00;--cap-bg:#fff3c4;--run:#0b62d6;--run-bg:#e2edff;--chip:#e9edf1;\
--note:#fff6cc;--note-line:#c9a227;--frame:#8c959f;--tag:#e9edf1}
@media (prefers-color-scheme:dark){:root{--bg:#101215;--card:#181b20;--fg:#e6edf3;--muted:#9aa4af;--line:#30363d;--soft:#20252c;--code:#1c2027;\
--accent:#6aa8ff;--ok:#56d37f;--ok-bg:#132a1b;--fail:#ff8078;--fail-bg:#311719;--cap:#e3a93a;--cap-bg:#2e2410;--run:#6aa8ff;--run-bg:#14243d;\
--chip:#232830;--note:#2d2712;--note-line:#8f7425;--frame:#76808b;--tag:#232830}}
*{box-sizing:border-box}
html{background:var(--bg)}
body{margin:0 auto;max-width:960px;padding:24px 16px 40px;background:var(--bg);color:var(--fg);\
font:14px/1.6 -apple-system,BlinkMacSystemFont,"Apple SD Gothic Neo","Malgun Gothic","Segoe UI",sans-serif;word-break:keep-all;overflow-wrap:anywhere}
header,nav,section{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:18px 20px;margin:0 0 14px}
h1{font-size:21px;line-height:1.35;margin:2px 0 4px}
h2{font-size:16.5px;line-height:1.4;margin:0 0 4px}
h3{font-size:12.5px;color:var(--muted);font-weight:600;margin:16px 0 4px}h3.top{margin-top:0}
p{margin:4px 0}
.mark{color:var(--muted);font-size:12px;margin:0}
.meta{color:var(--muted);font-size:12.5px}
.chips{display:flex;flex-wrap:wrap;gap:6px;margin:10px 0 4px}
.chip{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12px;line-height:1.7;background:var(--chip);white-space:nowrap}
.chip.ok{background:var(--ok-bg);color:var(--ok)}.chip.fail{background:var(--fail-bg);color:var(--fail)}
.chip.cap{background:var(--cap-bg);color:var(--cap)}.chip.run{background:var(--run-bg);color:var(--run)}
dl{display:grid;grid-template-columns:max-content 1fr;gap:3px 14px;margin:10px 0 0;font-size:13px}
dt{color:var(--muted)}dd{margin:0}
nav ol{margin:4px 0 0;padding-left:24px}nav li{margin:2px 0}
a{color:var(--accent)}nav a{text-decoration:none}
.headline{font-size:15px;font-weight:600;margin:10px 0 2px}
ul{margin:4px 0;padding-left:20px}li{margin:2px 0}
ul.risk li::marker{color:var(--cap)}
table{border-collapse:collapse;width:100%;font-size:13px;margin:2px 0}
th,td{border-bottom:1px solid var(--line);padding:5px 8px;text-align:left;vertical-align:top}
th{color:var(--muted);font-weight:600}
code,pre{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px}
code{background:var(--code);border-radius:5px;padding:1px 5px}
pre{background:var(--code);border-radius:8px;padding:10px 12px;margin:6px 0;overflow:auto;white-space:pre-wrap}
details{margin:6px 0}summary{color:var(--muted);font-size:12.5px;cursor:pointer}
figure{margin:8px 0 12px}figcaption{color:var(--muted);font-size:12.5px;margin:0 0 4px}
.pic{overflow-x:auto;border:1px solid var(--line);border-radius:8px;padding:8px;background:var(--bg)}
.pic svg{display:block;max-width:none}.pic svg.tl{width:100%;height:auto}
svg text{font-family:-apple-system,BlinkMacSystemFont,"Apple SD Gothic Neo","Malgun Gothic","Segoe UI",sans-serif}
.sq .pb{fill:var(--soft);stroke:var(--frame)}.sq .pt{fill:var(--fg);font-size:13px}
.sq .ll{stroke:var(--frame);stroke-dasharray:3 4}
.sq .ml{stroke:var(--fg);stroke-width:1.3;fill:none}.sq .ml.d{stroke-dasharray:5 4}
.sq .ah{fill:var(--fg)}.sq .mt{fill:var(--fg);font-size:12.5px}
.sq .nb{fill:var(--note);stroke:var(--note-line)}.sq .nt{fill:var(--fg);font-size:12.5px}
.sq .fr{fill:none;stroke:var(--frame)}.sq .fr.rc{fill:var(--soft);fill-opacity:.5;stroke-dasharray:2 3}
.sq .ft{fill:var(--tag);stroke:var(--frame)}.sq .fl{fill:var(--fg);font-size:11.5px;font-weight:600}
.sq .fc{fill:var(--muted);font-size:11.5px}.sq .fs{stroke:var(--frame);stroke-dasharray:4 3}
.tl .ax{stroke:var(--line)}.tl .lb{fill:var(--muted);font-size:11px}.tl .bar{fill:var(--frame)}
.tl .ok{fill:var(--ok)}.tl .fail{fill:var(--fail)}.tl .cap{fill:var(--cap)}.tl .run{fill:var(--run)}
.empty{color:var(--muted)}
footer{color:var(--muted);font-size:12px;margin:16px 4px 0}
@media (max-width:560px){header,nav,section{padding:14px}dl{grid-template-columns:1fr}dt{margin-top:6px}}
"""

// 실행 보고서(only 가 nil) 또는 단위 하나의 보고서. at 은 그린 때(유닉스 초), tz 는 보일 시간대, home 은 ~ 로 줄일 홈 폴더
public func apReportHTML(_ r: APReportRun, only: Int? = nil, at: Double, tz: TimeZone, home: String? = nil) -> String {
    var w = Writer(tz: tz, home: home)
    let units = only.map { k in r.units.filter { $0.n == k } } ?? r.units
    let name = r.title.isEmpty ? (r.project.isEmpty ? r.id : r.project) : r.title
    let docTitle = (only == nil ? "자율주행 보고서: " : "자율주행 단위 보고서: ") + name + (only.map { " 단위 \($0)" } ?? "")
    w.add("<!doctype html>\n<html lang=\"ko\">\n<head>\n<meta charset=\"utf-8\">\n")
    w.add("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n")
    // 바깥 요청을 모두 막는다(스크립트·글꼴·그림·연결·폼·base). 이 파일만 받아 다른 브라우저로 열어도 같다
    w.add("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\">\n")
    w.add("<meta name=\"referrer\" content=\"no-referrer\">\n")
    w.add("<meta name=\"generator\" content=\"Claude Board 자율주행 보고서 틀 1\">\n")
    w.add("<title>\(w.t(docTitle))</title>\n<style>\n\(css)\n</style>\n</head>\n<body>\n")

    // 머리 — 실행 이름, 프로젝트·때·모델, 결과 칩, 사실 목록
    let all = r.units
    func count(_ s: String) -> Int { all.filter { $0.status == s }.count }
    w.add("<header>\n<p class=\"mark\">\(only == nil ? "자율주행 보고서" : "자율주행 단위 보고서")</p>\n")
    w.add("<h1>\(w.t(name))</h1>\n")
    var meta: [String] = []
    if !r.project.isEmpty, !r.title.isEmpty { meta.append(w.t(r.project)) }
    meta.append(w.date(r.started, "yyyy년 M월 d일 HH:mm") + (r.ended.map { " ~ " + w.date($0, "M월 d일 HH:mm") } ?? ""))
    if !r.model.isEmpty { meta.append("모델 " + w.t(r.model)) }
    meta.append("실행 <code>\(w.c(r.id))</code>")
    w.add("<p class=\"meta\">\(meta.joined(separator: " · "))</p>\n<div class=\"chips\">")
    let st = r.state
    w.add(chip(w.t(runLabel[st] ?? (st.isEmpty ? "상태 모름" : st)), st == "done" ? "ok" : st == "error" ? "fail" : ["stopped", ""].contains(st) ? "" : "run"))
    w.add(chip("\(all.count)개 중 \(count("done"))개 통과", count("done") == all.count && !all.isEmpty ? "ok" : ""))
    for (s, l, cls) in [("failed", "미통과", "fail"), ("capped", "상한에서 멈춤", "cap"), ("skipped", "건너뜀", ""), ("stopped", "중단", ""), ("pending", "대기", ""), ("running", "진행 중", "run")]
    where count(s) > 0 { w.add(chip("\(l) \(count(s))", cls)) }
    w.add("</div>\n")
    if !r.reason.isEmpty { w.add("<p>\(w.t(r.reason))</p>\n") }
    w.add("<dl>\n")
    func fact(_ k: String, _ v: String) { w.add("<dt>\(k)</dt><dd>\(v)</dd>\n") }
    let active = all.reduce(0) { $0 + $1.activeSec }, cost = all.reduce(0) { $0 + $1.cost }
    if let s = r.started { fact("걸린 시간", apReportDur((r.ended ?? at) - s) + (r.ended == nil ? " (아직 도는 중)" : "")) }
    fact("일한 시간", apReportDur(active) + " (단위 시계 합, 대기·일시정지 뺌)")
    fact("비용", String(format: "%.2f USD", cost))
    if !r.mode.isEmpty {
        let where_ = r.mode == "worktree" ? "따로 작업 트리" : r.mode == "in_place" ? "원래 작업 트리" : r.mode
        fact("어디서", w.t(where_) + (r.repos.isEmpty ? "" : " · 저장소 " + r.repos.map { "<code>\(w.c($0))</code>" }.joined(separator: " ")))
    }
    if !r.work.isEmpty { fact("자율주행 가지", "<code>\(w.c(r.work))</code>" + (r.bases.isEmpty ? "" : " (출발 " + r.bases.map { w.c($0) }.joined(separator: ", ") + ")")) }
    if !r.epic.isEmpty { fact("에픽", w.t(r.epic)) }
    if let same = r.harnessSame {
        fact("하네스", same ? "출발 때와 같음 (파일 \(r.harnessFiles)개)" : "출발 때와 다름 \(r.harnessChanged.count)곳")
    } else { fact("하네스", "<span class=\"empty\">끝 검사 기록 없음</span>") }
    if !r.moved.isEmpty { fact("움직인 기존 가지", r.moved.map { "<code>\(w.c($0))</code>" }.joined(separator: " ")) }
    if r.stashed > 0 { fact("따로 보관한 변경", "\(r.stashed)번 (읽기 전용 자리에 남은 변경, git stash list 로 봅니다)") }
    w.add("</dl>\n")
    if r.harnessSame == false, !r.harnessChanged.isEmpty {
        w.add("<details><summary>바뀐 하네스 파일</summary><ul>" + r.harnessChanged.map { "<li><code>\(w.c($0))</code></li>" }.joined() + "</ul></details>\n")
    }
    w.add("</header>\n")

    if only == nil {
        // 차례와 단위가 돈 때(그림)
        if !units.isEmpty {
            w.add("<nav aria-label=\"단위 차례\">\n<h3 class=\"top\">단위</h3>\n<ol>\n")
            for u in units {
                w.add("<li><a href=\"#unit-\(u.n)\">\(w.t(u.title.isEmpty ? "단위 \(u.n)" : u.title))</a> \(chip(w.t(unitLabel[u.status] ?? u.status), unitClass[u.status] ?? ""))</li>\n")
            }
            w.add("</ol>\n")
            if let tl = timeline(r, at: at, w: w) { w.add("<figure><figcaption>단위가 돈 때</figcaption><div class=\"pic\">\(tl)</div></figure>\n") }
            w.add("</nav>\n")
        }
    }

    for u in units { unitSection(u, &w) }
    if units.isEmpty { w.add("<section><p class=\"empty\">단위 기록이 없습니다.</p></section>\n") }

    w.add("<footer>Claude Board 가 결과 기록(모델이 씀)과 엔진 기록으로 그렸습니다. 모델을 부르지 않았고, 바깥 글꼴·스크립트·그림 없이 이 파일 하나로 보입니다. ")
    w.add("단위의 통과·미통과는 엔진의 판정입니다. 모델 글의 긴 줄표는 쉼표로 바꿔 옮겼습니다. ")
    w.add("그린 때 " + w.date(at, "yyyy년 M월 d일 HH:mm") + " · 바탕 " + (r.origin == .folder ? "실행 폴더" : "기록 저장소(실행 폴더를 지워 판정 출력은 없습니다)") + ".</footer>\n")
    w.add("</body>\n</html>\n")
    return w.o
}

private func unitSection(_ u: APReportUnit, _ w: inout Writer) {
    w.add("<section id=\"unit-\(u.n)\">\n")
    w.add("<h2>단위 \(u.n). \(w.t(u.title.isEmpty ? "제목 없음" : u.title)) \(chip(w.t(unitLabel[u.status] ?? u.status), unitClass[u.status] ?? ""))</h2>\n")
    var meta: [String] = []
    if !u.kind.isEmpty { meta.append(w.t(kind(u.kind))) }
    if !u.task.isEmpty { meta.append("작업 " + w.t(u.task) + (u.story.isEmpty ? "" : " · 스토리 " + w.t(u.story))) }
    if !u.branch.isEmpty { meta.append("가지 <code>\(w.c(u.branch))</code>") }
    if u.started != nil || u.activeSec > 0 { meta.append("일한 시간 " + apReportDur(u.activeSec)) }
    meta.append(String(format: "%.2f USD", u.cost))
    if u.turns > 0 { meta.append("턴 \(u.turns)") }
    if let s = u.started { meta.append(w.date(s, "HH:mm") + (u.ended.map { " ~ " + w.date($0, "HH:mm") } ?? "")) }
    w.add("<p class=\"meta\">\(meta.joined(separator: " · "))</p>\n")
    if !u.reason.isEmpty, u.status != "done" { w.add("<p class=\"meta\">엔진: \(w.t(u.reason))</p>\n") }
    if let m = mergeLabel[u.merge] { w.add("<p class=\"meta\">\(m)</p>\n") }

    switch u.resultState {
    case .missing: w.add("<p class=\"empty\">결과 기록이 없습니다\(u.status == "skipped" || u.status == "pending" ? "(단위를 시작하지 않았습니다)" : "(모델이 남기지 않았습니다)").</p>\n")
    case .broken: w.add("<p class=\"empty\">결과 기록을 읽지 못했습니다(JSON 객체가 아닙니다).</p>\n")
    case .ok: break
    }
    if let r = u.result {
        if !r.headline.isEmpty { w.add("<p class=\"headline\">\(w.t(r.headline))</p>\n") }
        if let o = outcomeLabel[r.outcome] { w.add("<p class=\"meta\">모델이 적은 결론: \(o)</p>\n") }
        list("한 일", r.summary.map { w.t($0) }, &w)
        if !r.tests.isEmpty {
            w.add("<h3>시험</h3>\n<table><thead><tr><th>시험</th><th>결과</th><th>메모</th></tr></thead><tbody>\n")
            for x in r.tests {
                let (l, cls) = testLabel[x.result] ?? (x.result, "")
                w.add("<tr><td>\(w.t(x.name))</td><td>\(chip(w.t(l), cls))</td><td>\(w.t(x.note))</td></tr>\n")
            }
            w.add("</tbody></table>\n")
        }
        list("바꾼 것", r.changes.map { w.t($0.what) + ($0.place.isEmpty ? "" : " <code>\(w.c(apPlain($0.place)))</code>") }, &w)
        list("따른 결정", r.decisions.map { w.t($0.what) + ($0.basis.isEmpty ? "" : " <span class=\"meta\">(근거: \(w.t($0.basis)))</span>") }, &w)
        if !r.flows.isEmpty {
            w.add("<h3>흐름 그림</h3>\n")
            for (k, f) in r.flows.enumerated() { flow(f, unit: u.n, k: k + 1, &w) }
        }
        if !r.risks.isEmpty {
            w.add("<h3>알아 둘 위험</h3>\n<ul class=\"risk\">" + r.risks.map { "<li>\(w.t($0))</li>" }.joined() + "</ul>\n")
        }
        list("사람 몫이라 미룬 것(다음 출발 카드)", r.nextStart.map { w.t($0.title) + ($0.why.isEmpty ? "" : ": " + w.t($0.why)) }, &w)
        list("새로 찾은 일(다음 출발 대기열 후보)", r.newWork.map { w.t($0.title) + ($0.why.isEmpty ? "" : ": " + w.t($0.why)) }, &w)
        if !r.next.isEmpty { w.add("<h3>다음에 할 일</h3>\n<p>\(w.t(r.next))</p>\n") }
        if !r.refs.isEmpty {
            w.add("<h3>참고</h3>\n<ul>")
            for ref in r.refs {
                let v = apPlain(ref.value)
                if let link = apSafeLink(ref.value) {
                    w.add("<li>\(w.t(ref.key)): <a href=\"\(apEscapeHTML(link))\" rel=\"noopener noreferrer\">\(w.c(link))</a></li>")
                } else {
                    w.add("<li>\(w.t(ref.key)): \(w.t(v))</li>")
                }
            }
            w.add("</ul>\n")
        }
    }

    // 판정(엔진) — 차례마다 종료 코드·시간, 통과하지 못한 마지막 판정의 출력 끝부분
    if !u.checks.isEmpty || u.lastExit != nil {
        w.add("<h3>판정</h3>\n")
        if u.checks.isEmpty, let e = u.lastExit {
            w.add("<p>마지막 판정 종료 코드 \(e)" + (u.retries > 0 ? " · 다시 한 횟수 \(u.retries)" : "") + "</p>\n")
        } else {
            w.add("<ul>")
            for c in u.checks {
                var s = "\(c.n)번째: " + (c.exit.map { $0 == 0 ? "통과(종료 코드 0)" : "종료 코드 \($0)" } ?? "끝나지 않음") + " · " + (c.sec < 1 ? "1초 안" : apReportDur(c.sec))
                if c.timedOut { s += " · 시간 상한에서 멈춤" }
                if c.interrupted { s += " · 도중에 끊김" }
                w.add("<li>\(s)</li>")
            }
            w.add("</ul>\n")
            if let last = u.checks.last, last.exit != 0, !last.tail.isEmpty {
                w.add("<details><summary>마지막 판정 출력 끝부분</summary><pre>\(w.c(apPlain(last.tail)))</pre></details>\n")
            }
        }
    }
    if !u.denials.isEmpty {
        w.add("<h3>허용 범위 밖이라 거부한 일</h3>\n<ul>" + u.denials.map { "<li>\(w.t($0))</li>" }.joined() + "</ul>\n")
    }
    w.add("</section>\n")
}

private func list(_ title: String, _ items: [String], _ w: inout Writer) {
    guard !items.isEmpty else { return }
    w.add("<h3>\(title)</h3>\n<ul>" + items.map { "<li>\($0)</li>" }.joined() + "</ul>\n")
}

// 흐름 그림 하나 — 읽히면 SVG 와 접힌 원문, 아니면 원문과 까닭
private func flow(_ f: APResultFlow, unit: Int, k: Int, _ w: inout Writer) {
    let title = f.title.isEmpty ? "흐름 그림 \(k)" : f.title
    w.add("<figure>\n<figcaption>\(w.t(title))</figcaption>\n")
    switch apParseSequence(f.source) {
    case .ok(let d):
        w.add("<div class=\"pic\">\(apSequenceSVG(d, label: "흐름 그림: " + title))</div>\n")
        w.add("<details><summary>원문 보기</summary><pre>\(w.t(f.source))</pre></details>\n")
    case .unsupported(let line, let why):
        w.add("<p class=\"empty\">그리지 못해 원문을 보입니다(\(line)번째 줄: \(w.t(why))).</p>\n<pre>\(w.t(f.source))</pre>\n")
    }
    w.add("</figure>\n")
}

// 단위가 돈 때 — 출발부터 끝(도는 중이면 그린 때)까지 가로 막대. 시작 시각이 있는 단위만
private func timeline(_ r: APReportRun, at: Double, w: Writer) -> String? {
    let us = r.units.filter { $0.started != nil }
    guard let t0 = r.started ?? us.compactMap(\.started).min() else { return nil }
    let t1 = max(r.ended ?? us.compactMap(\.ended).max() ?? at, t0 + 1)
    guard !us.isEmpty, t1 > t0 else { return nil }
    let W = 640.0, L = 70.0, R = 12.0, row = 20.0
    let h = Double(us.count) * row + 34
    func x(_ t: Double) -> Double { L + (min(max(t, t0), t1) - t0) / (t1 - t0) * (W - L - R) }
    func n(_ v: Double) -> String { String(Int(v.rounded())) }
    var s = "<svg class=\"tl\" viewBox=\"0 0 \(n(W)) \(n(h))\" width=\"\(n(W))\" height=\"\(n(h))\" role=\"img\" aria-label=\"단위가 돈 때\">"
    for (k, u) in us.enumerated() {
        let y = 6 + Double(k) * row
        let a = x(u.started ?? t0), b = max(x(u.ended ?? (u.status == "running" ? at : (u.started ?? t0))), a + 3)
        s += "<text class=\"lb\" x=\"0\" y=\"\(n(y + 11))\">단위 \(u.n)</text>"
        s += "<rect class=\"bar \(unitClass[u.status] ?? "")\" x=\"\(n(a))\" y=\"\(n(y + 2))\" width=\"\(n(b - a))\" height=\"12\" rx=\"3\"/>"
    }
    let ay = Double(us.count) * row + 10
    s += "<line class=\"ax\" x1=\"\(n(L))\" y1=\"\(n(ay))\" x2=\"\(n(W - R))\" y2=\"\(n(ay))\"/>"
    s += "<text class=\"lb\" x=\"\(n(L))\" y=\"\(n(ay + 16))\">\(w.date(t0, "HH:mm"))</text>"
    s += "<text class=\"lb\" x=\"\(n(W - R))\" y=\"\(n(ay + 16))\" text-anchor=\"end\">\(w.date(t1, "HH:mm"))</text>"
    return s + "</svg>"
}

// MARK: - 유스케이스: 보고서 그리기

public struct APReportDraw: Equatable, Sendable {
    public var written: [APReportFile] = []
    public var errors: [String] = []
    public init() {}
}

// 기록을 읽어 실행 보고서와 고른 단위의 보고서를 쓴다. run.json 이 없으면 아무것도 쓰지 않는다.
// 엔진이 단위 판정 뒤(그 단위와 실행)·건너뛴 뒤·끝 정리 때 부르고, 엔진 report 하위 명령이 끝난 실행을 다시 그릴 때 부른다
public func apDrawReports(id: String, source: APReportSource, sink: APReportSink, units: [Int], clock: APClock, tz: TimeZone, home: String?) -> APReportDraw {
    var d = APReportDraw()
    guard let r = apReportRun(id: id, source: source) else { d.errors.append("실행 기록(run.json)을 읽지 못해 보고서를 그리지 않았습니다"); return d }
    let at = clock.now.timeIntervalSince1970
    for n in units {
        guard r.units.contains(where: { $0.n == n }) else { d.errors.append("대기열에 없는 단위(\(n))라 그 단위 보고서를 그리지 않았습니다"); continue }
        if let e = sink.write(.unit(n), apReportHTML(r, only: n, at: at, tz: tz, home: home)) { d.errors.append("단위 \(n) 보고서: " + e) } else { d.written.append(.unit(n)) }
    }
    if let e = sink.write(.run, apReportHTML(r, at: at, tz: tz, home: home)) { d.errors.append("실행 보고서: " + e) } else { d.written.append(.run) }
    return d
}
