import AppKit
import AutopilotRules
import BoardRules
import SwiftUI

// MARK: 자율주행 · 가져오기(7단계 ⑫-7a)
// 원천 셋(저장소 할 일 문서·바깥 목록·옛 자동화 옮기기)의 1단계 입력과 3·4단계 칸. 준비 차례(모으기 → 가리기 → 가져오기 세션 → 기계 검사 → 이어 가기와 같은 확인)는
// Autopilot.swift 의 APStart 확장(prepareImport)이 하고, 원천 읽기·가리기·근거 묶음·세션은 앱 안 엔진(import gather·session)이 한다. 규칙은 AutopilotRules/Import.swift

@MainActor final class APImportInput: ObservableObject {
    @Published var path = "" { didSet { if path != oldValue { pathChanged() } } }
    @Published private(set) var repo: APTopicRepo?
    @Published private(set) var reading = false
    @Published private(set) var files: [String] = []        // 할 일 문서 후보(저장소 안 글 문서, 이름이 할 일 같은 것 먼저)
    @Published var file = "" { didSet { if file != oldValue { fileChanged() } } }
    @Published private(set) var sections: [String] = []
    @Published private(set) var fileNote: String?
    @Published var section = ""                              // 빈 글자 = 문서 전체
    @Published var service = "atlassian"
    @Published var query = ""
    @Published var folder = ""
    @Published var request = ""
    private let queue = DispatchQueue(label: "claude-board.autopilot.import")
    private var work: DispatchWorkItem?

    func reset() {
        work?.cancel()
        path = ""; repo = nil; reading = false; files = []; file = ""; sections = []; fileNote = nil; section = ""
        service = "atlassian"; query = ""; folder = ""; request = ""
    }

    var root: String { repo?.top ?? "" }
    var projectName: String? { repo?.project }

    // 바깥 목록을 읽을 수 있는 연결인가(연결·읽기 범위·토큰·만료, 규칙 apReadBlocker) — 까닭이 있으면 그 서비스는 잠금
    func serviceLock(_ svc: APService) -> String? {
        let c = APConnections.shared
        c.loadOnce()
        let conn = c.list.first { $0.service == svc }
        return apReadBlocker(svc, connection: conn, hasSecret: conn.map { c.secrets.contains($0.id) } ?? false, dev: apDevBuild, now: Date(), tz: .current)
    }
    // 바깥 목록 원천 자체의 잠금 — Jira·Notion 둘 다 읽을 수 없으면
    var listLock: String? {
        let a = serviceLock(.atlassian), n = serviceLock(.notion)
        return a != nil && n != nil ? "Jira·Notion 연결(읽기 범위)이 없어 고를 수 없습니다(설정 › 연결). \(a!)" : nil
    }

    func blocker(_ source: String) -> String? {
        if reading { return "작업 경로를 읽는 중입니다" }
        if path.trimmingCharacters(in: .whitespaces).isEmpty || repo == nil { return "작업 경로(기존 저장소)를 넣어 주세요" }
        if let e = repo?.error { return e }
        switch source {
        case "todo":
            if file.isEmpty { return "할 일 문서를 골라 주세요" }
            if !apTodoPathOK(file) { return "할 일 문서는 저장소 안 글 문서(md·txt)여야 합니다" }
            return fileNote
        case "list":
            guard let svc = APService(rawValue: service) else { return "서비스를 골라 주세요" }
            if let why = serviceLock(svc) { return why }
            if case .bad(let why) = apListQuery(query, service: svc) { return why }
            return nil
        case "legacy":
            let f = expand(folder)
            if f.isEmpty { return "옛 자동화 폴더를 넣어 주세요" }
            var d: ObjCBool = false
            if !f.hasPrefix("/") || !FileManager.default.fileExists(atPath: f, isDirectory: &d) || !d.boolValue { return "그 경로에 폴더가 없습니다" }
            return nil
        default: return "원천을 골라 주세요"
        }
    }
    private func expand(_ p: String) -> String {
        let t = p.trimmingCharacters(in: .whitespaces)
        return t.hasPrefix("~") ? NSHomeDirectory() + t.dropFirst() : t
    }
    // 같은 입력이면 1단계 "다음"이 다시 준비하지 않는다
    func key(_ source: String) -> String { [source, root, file, section, service, query, expand(folder), request].joined(separator: "\u{1}") }
    // 엔진 import 가 읽는 입력(import.json) — 프로젝트 이름(name)과 되살린 키(include)는 준비가 채운다
    func json(_ source: String) -> [String: Any] {
        var o: [String: Any] = ["source": source, "repo": root, "request": request]
        switch source {
        case "todo": o["todo"] = ["path": file, "section": section]
        case "list": o["list"] = ["service": service, "query": query.trimmingCharacters(in: .whitespacesAndNewlines)]
        case "legacy": o["legacy"] = ["folder": expand(folder)]
        default: break
        }
        return o
    }
    func originText(_ source: String) -> String {
        switch source {
        case "todo": return file + (section.isEmpty ? " · 문서 전체" : " · 절 \(section)")
        case "list": return (APService(rawValue: service)?.label ?? service) + " · " + query
        case "legacy": return apHome(expand(folder))
        default: return ""
        }
    }

    private func pathChanged() {
        work?.cancel()
        let input = path.trimmingCharacters(in: .whitespaces)
        guard !input.isEmpty else { repo = nil; reading = false; files = []; return }
        reading = true
        let w = DispatchWorkItem { [weak self] in
            let r = APTopic.readRepo(input)
            let found = r.top.map { APImportInput.scanFiles($0) } ?? []
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, self.path.trimmingCharacters(in: .whitespaces) == input else { return }
                    self.repo = r
                    self.files = found
                    self.reading = false
                    if !self.file.isEmpty, !found.contains(self.file) { self.fileChanged() }
                }
            }
        }
        work = w
        queue.asyncAfter(deadline: .now() + 0.3, execute: w)
    }
    // 저장소 안 글 문서(깊이 3, 숨은 자리·빌드 자리 빼고, 링크 따라가지 않음, 80개까지) — 이름이 할 일 같은 것 먼저
    nonisolated static func scanFiles(_ top: String) -> [String] {
        var out: [String] = []
        guard let e = FileManager.default.enumerator(atPath: top) else { return [] }
        while let rel = e.nextObject() as? String {
            let name = (rel as NSString).lastPathComponent
            if name.hasPrefix(".") || ["node_modules", "build", "dist", "target", ".build", "vendor"].contains(name) { e.skipDescendants(); continue }
            if rel.split(separator: "/").count > 3 { e.skipDescendants(); continue }
            guard apTodoPathOK(rel), (try? FileManager.default.attributesOfItem(atPath: top + "/" + rel)[.type] as? FileAttributeType) == .typeRegular else { continue }
            out.append(rel)
            if out.count >= 200 { break }
        }
        return Array(out.sorted { (apTodoLikely($0) ? 0 : 1, $0) < (apTodoLikely($1) ? 0 : 1, $1) }.prefix(80))
    }
    // 고른 문서의 절(머리 제목) — 저장소 안 보통 파일만, 512 KB 까지(엔진이 출발 준비 때 같은 규칙으로 다시 읽는다)
    private func fileChanged() {
        sections = []; fileNote = nil
        let top = root, rel = file
        guard !top.isEmpty, !rel.isEmpty else { return }
        guard apTodoPathOK(rel) else { fileNote = "저장소 안 글 문서(md·txt)만 고릅니다"; return }
        queue.async {
            let p = top + "/" + rel
            var st = stat()
            var note: String?, heads: [String] = []
            if lstat(p, &st) != 0 { note = "그 파일이 없습니다" }
            else if st.st_mode & S_IFMT != S_IFREG { note = "보통 파일이 아닙니다(바로 가기는 읽지 않습니다)" }
            else if Int(st.st_size) > apTodoMaxBytes { note = "\(apTodoMaxBytes >> 10) KB 보다 큽니다" }
            else if let t = try? String(contentsOfFile: p, encoding: .utf8) {
                var seen = Set<String>()
                heads = apTodoHeadings(t).map(\.title).filter { seen.insert($0).inserted }
            } else { note = "UTF-8 글이 아닙니다" }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard self.file == rel else { return }
                    self.sections = heads; self.fileNote = note
                    if !heads.contains(self.section) { self.section = "" }
                }
            }
        }
    }
}

// 1단계: 작업 경로 · 원천 칸 · 덧붙일 말
struct APImportStep1: View {
    @ObservedObject var s: APStart
    @ObservedObject var m: APImportInput
    @ObservedObject private var cn = APConnections.shared   // 연결 목록·키체인 있음 여부가 늦게 읽혀도 서비스 잠금을 다시 그린다
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            pathBox
            switch s.source {
            case "todo": todoBox
            case "list": listBox
            default: legacyBox
            }
            if s.jira { APEpicBox(s: s) }
            requestBox
        }
    }

    private var pathBox: some View {
        APBox(title: "작업 경로", note: "기존 저장소 · 단위는 이 저장소 뿌리에서 돕니다", axid: "ap-imp-path-box") {
            TextField("저장소 경로(예: ~/workspace/todo-api)", text: $m.path)
                .textFieldStyle(.roundedBorder).boardFont(12)
                .accessibilityIdentifier("ap-imp-path")
            if m.reading { apNote("읽는 중…") }
            else if let r = m.repo {
                if let e = r.error { apNote(e, APColor.fail) }
                else {
                    apNote("프로젝트 \(r.project ?? "?")" + (r.known ? " · 지난 실행 \(r.pastRuns)" : " · 처음") + (r.setAt != nil ? " · 자율주행 세트 있음(\(apDay(r.setAt)))" : " · 세트의 지시 없음(가져오기 세션이 초안을 씀)"))
                }
            }
            if let a = s.activeHere { apNote("이 경로에서 자율주행이 돌고 있습니다(\(a)). 그 실행이 끝난 뒤에 출발합니다", APColor.limit) }
        }
    }

    private var todoBox: some View {
        APBox(title: "할 일 문서", note: "파일과 그 안의 절을 고릅니다 · 체크박스·목록 항목이 항목이고 [x] 는 끝난 것", axid: "ap-imp-todo-box") {
            TextField("저장소 안 경로(예: docs/TODO.md)", text: $m.file)
                .textFieldStyle(.roundedBorder).boardFont(12)
                .accessibilityIdentifier("ap-imp-file-input")
            if !m.files.isEmpty {
                WrapRow(spacing: 5) {
                    ForEach(Array(m.files.prefix(12).enumerated()), id: \.offset) { i, f in
                        APButton(title: f, tint: m.file == f ? Palette.accent : nil, help: "이 문서를 고릅니다", axid: "ap-imp-file-\(i)") { m.file = f }
                    }
                }
            } else if m.repo?.top != nil, !m.reading { apNote("저장소에서 글 문서(md·txt)를 찾지 못했습니다. 경로를 직접 넣습니다") }
            if let n = m.fileNote { apNote(n, APColor.fail) }
            if !m.file.isEmpty, m.fileNote == nil {
                apNote("절")
                APPickRow(on: m.section.isEmpty, axid: "ap-imp-section-0", action: { m.section = "" }) { Text("문서 전체").boardFont(11.5) }
                ForEach(Array(m.sections.enumerated()), id: \.offset) { i, h in
                    APPickRow(on: m.section == h, axid: "ap-imp-section-\(i + 1)", action: { m.section = h }) { Text(h).boardFont(11.5) }
                }
            }
            apNote("파일은 저장소 안에서만 읽고 바로 가기(링크)는 따라가지 않습니다. 끝난 표시([x]·취소선·(완료))와 진행 중([~])인 항목은 가립니다.")
        }
    }

    private var listBox: some View {
        APBox(title: "바깥 목록", note: "연결이 있는 서비스만 · 메타 정보만 읽습니다", axid: "ap-imp-list-box") {
            ForEach([APService.atlassian, .notion], id: \.self) { svc in
                let lock = m.serviceLock(svc)
                APPickRow(on: m.service == svc.rawValue, enabled: lock == nil, help: lock ?? "", axid: "ap-imp-svc-\(svc.rawValue)", action: { m.service = svc.rawValue }) {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(svc == .atlassian ? "Jira 필터·에픽" : "Notion 데이터베이스").boardFont(12, weight: .semibold)
                            if lock != nil { APChip(text: "잠금", color: APColor.muted) }
                        }
                        if let lock { apNote(lock, APColor.limit.opacity(0.85)) }
                    }
                }
            }
            TextField(m.service == "notion" ? "데이터베이스 주소나 32자리 id" : "에픽 키(TOY-10)·필터 번호·필터 주소·JQL", text: $m.query)
                .textFieldStyle(.roundedBorder).boardFont(12)
                .accessibilityIdentifier("ap-imp-query")
            if let svc = APService(rawValue: m.service), !m.query.isEmpty {
                let q = apListQuery(m.query, service: svc)
                if case .bad(let why) = q { apNote(why, APColor.fail) } else { apNote("읽을 것: " + q.label) }
            }
            apNote("읽기 요청만 보냅니다(Jira 는 검색 GET, Notion 은 데이터베이스 질의). 끝난 상태(Done·Complete)와 진행 중(In Progress)인 항목은 가리고, Jira 번호는 단위의 티켓으로 그대로 넘깁니다.")
        }
    }

    private var legacyBox: some View {
        APBox(title: "옛 자동화 폴더", note: "멈춘 자리 기록과 원장(글 파일)을 읽습니다", axid: "ap-imp-legacy-box") {
            TextField("폴더 경로(예: ~/old-automation/state)", text: $m.folder)
                .textFieldStyle(.roundedBorder).boardFont(12)
                .accessibilityIdentifier("ap-imp-folder")
            apNote("이 폴더 안 글 파일(md·txt·json·jsonl·yaml·log 등, 깊이 3·40개·파일마다 256 KB 까지)만 읽고 바로 가기는 따라가지 않습니다. 비밀이 흔한 이름(.env·key·secret·token 등)은 읽지 않고, 읽은 글의 비밀 꼴은 가린 뒤 근거 묶음으로만 넘깁니다. 원장의 줄(제목·상태·id)이 항목입니다.")
        }
    }

    private var requestBox: some View {
        APBox(title: "덧붙일 말(선택)", note: "가져오기 세션에 넘깁니다 · 결정이 아니라 요청입니다", axid: "ap-imp-request-box") {
            TextEditor(text: $m.request)
                .font(.system(size: 12))
                .scrollContentBackground(.hidden)
                .frame(minHeight: 44, maxHeight: 110)
                .padding(4)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
                .accessibilityIdentifier("ap-imp-request")
        }
    }
}

// 3단계: 가린 항목 — 까닭과 되살리기(고른 뒤 다시 준비, 가져오기 세션·비판 세션이 다시 돈다)
struct APImportHiddenBox: View {
    @ObservedObject var s: APStart
    @State private var open = false
    var body: some View {
        let hidden = s.importHidden
        if !hidden.isEmpty {
            APFold(title: "가린 항목 \(hidden.count)", summary: s.importRevive.isEmpty ? "펼쳐서 되살릴 항목을 고릅니다" : "되살림 \(s.importRevive.count) · 다시 준비하면 넣습니다",
                   axid: "ap-imp-hidden", open: $open) {
                apNote("가져오기 전에 가린 항목입니다. 되살리면 다시 준비할 때 가리지 않고 가져오기 세션에 넘깁니다(대기열에 정의가 남은 줄이면 그 줄을 잇습니다).")
                ForEach(Array(hidden.enumerated()), id: \.offset) { i, h in
                    let key = h["key"] as? String ?? ""
                    APPickRow(on: s.importRevive.contains(key), square: true, enabled: !s.preparing && !s.starting, axid: "ap-imp-revive-\(i)", action: { s.toggleRevive(key) }) {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(h["title"] as? String ?? "").boardFont(11.5, weight: .semibold).fixedSize(horizontal: false, vertical: true)
                            apNote((h["why"] as? String ?? "") + " · " + (h["place"] as? String ?? ""))
                        }
                    }
                }
                HStack(spacing: 6) {
                    APButton(title: "되살린 \(s.importRevive.count)개로 다시 준비", primary: true, enabled: !s.importRevive.isEmpty && !s.preparing && !s.starting,
                             help: "가져오기를 처음부터 다시 합니다(모으기·가져오기 세션·확인·판정 미리 돌리기·비판 세션)", axid: "ap-imp-revive-go") { s.reviveImport() }
                    apNote("가져오기 세션과 비판 세션이 다시 돌아 모델 비용이 듭니다")
                }
            }
        }
    }
}

// 4단계: 원천 줄 — 원천·항목 수·뺀 수(까닭별)·이어 간 줄·세션 결과
struct APImportSourceBox: View {
    @ObservedObject var s: APStart
    var body: some View {
        let g = s.importGather, c = g["counts"] as? [String: Any] ?? [:]
        let hidden = (c["hidden"] as? [String: Any] ?? [:]).mapValues { ($0 as? NSNumber)?.intValue ?? 0 }
        let r = s.importResult
        func n(_ k: String) -> Int { (c[k] as? NSNumber)?.intValue ?? 0 }
        let parts = [("source_done", "원천에서 끝남"), ("source_progress", "원천에서 진행 중"), ("records_done", "이미 끝남"), ("running", "도는 실행에서 진행 중"),
                     ("queue_state", "대기열에서 뺀·보류한 줄")].compactMap { k, l in (hidden[k] ?? 0) > 0 ? "\(l) \(hidden[k]!)" : nil }
        return APBox(title: "원천", note: "늘 보입니다", axid: "ap-imp-source") {
            HStack(spacing: 6) {
                APChip(text: s.importSource?.label ?? s.source, color: Palette.accent, strong: true)
                Text(g["origin"] as? String ?? "").boardFont(11, design: .monospaced).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
            apNote("항목 \(n("total")) · 뺀 것 \(hidden.values.reduce(0, +))" + (parts.isEmpty ? "" : "(" + parts.joined(separator: " · ") + ")")
                   + " · 대기열 줄을 이음 \(n("continue"))" + (n("changed") > 0 ? "(바뀐 본문 \(n("changed")))" : "") + " · 새 항목 \(n("new"))")
            let units = (r["units"] as? [Any])?.count ?? 0, cards = (r["cards"] as? [Any])?.count ?? 0, skipped = (r["skipped"] as? [Any])?.count ?? 0
            apNote("가져오기 세션: " + (r["session"] as? Bool == false ? (r["note"] as? String ?? "돌리지 않음") : "단위 \(units) · 카드 \(cards) · 건너뜀 \(skipped)")
                   + " · 근거 묶음 \((g["manifest"] as? NSNumber)?.intValue ?? 0)개 · 가린 비밀 \((g["redacted"] as? NSNumber)?.intValue ?? 0)"
                   + (((g["requests"] as? NSNumber)?.intValue ?? 0) > 0 ? " · 바깥 요청 \((g["requests"] as? NSNumber)!.intValue)건" : ""))
            if !s.importRevive.isEmpty { apNote("되살린 항목 \(s.importRevive.count)개를 넣어 준비했습니다") }
        }
    }
}

// 3단계 단위 줄의 덧붙임: 이어 간 줄의 바뀐 본문(+ 새 줄 · − 옛 줄)
struct APImportChangedNote: View {
    let lines: [String]
    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            apNote("원천 본문이 앞 가져오기 뒤에 바뀌었습니다(대기열 줄의 정의는 그대로 잇습니다. 바뀐 일이면 이 단위를 빼고 다음에 되살려 다시 가져옵니다)", APColor.limit)
            ForEach(Array(lines.prefix(10).enumerated()), id: \.offset) { _, l in
                Text(l).boardFont(10.5, design: .monospaced).foregroundStyle(l.hasPrefix("+") ? APColor.ok : APColor.limit).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(6)
        .background(RoundedRectangle(cornerRadius: 8).fill(APColor.limit.opacity(0.06)))
        .accessibilityElement(children: .combine)
    }
}
