// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import AutopilotRules
import BoardRules
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI
import WebKit

// MARK: - 자율주행 · 연결과 올릴 글

// 4단계 ⑩-1(2026-10-30, 설계 5장 6절): 설정 › 연결(Atlassian·Notion·Figma·알림 webhook 의 서비스·계정·범위·만료일, 비밀은 키체인에만)과
// 올릴 글 검수 시트(받을 곳·계정·첫 줄 표지·요약·첨부를 올릴 모양대로, 올리기·올리지 않기·나중에·모두 올리기).
// 보드는 HTTP 를 직접 하지 않는다: "연결 확인"과 "올리기"는 앱 안 엔진(claude-autopilot connection check · outbox post)을 부르고,
// 보드가 하는 쓰기는 연결 목록 파일(connections.json, 비밀 없음)과 키체인 항목(/usr/bin/security 의 표준 입력으로)뿐이다.
// 규칙(주소·만료·판정·형식)은 AutopilotRules, 올리기 어댑터(HTTP·키체인 읽기·outbox 쓰기)는 엔진 한 곳에 있다(설계 판단, 설계 5장 6절)

// 연결 목록 — 보드가 쓰고 엔진이 읽는다(뿌리의 connections.json). 키체인 항목 있음 여부는 security 로 본다(값은 읽지 않는다)
@MainActor final class APConnections: ObservableObject {
    static let shared = APConnections()
    @Published private(set) var list: [APConnection] = []
    @Published private(set) var secrets: Set<String> = []
    @Published private(set) var checking: Set<String> = []
    @Published private(set) var notes: [String: (text: String, bad: Bool)] = [:]
    private var loaded = false
    var path: String { autopilotRoot + "/connections.json" }
    var service: String { apSecretService(dev: apDevBuild) }

    func loadOnce() { if !loaded { load() } }
    func load() {
        loaded = true
        list = apParseConnections(APFolderReportSource.plainFile(path))
        refreshSecrets()
    }
    func refreshSecrets() {
        let ids = ["atlassian", "notion", "figma"] + list.filter { $0.service == .webhook }.map(\.id)
        let svc = service
        DispatchQueue.global(qos: .userInitiated).async {
            let have = Set(ids.filter { Keychain.exists(svc, account: $0) })
            DispatchQueue.main.async { MainActor.assumeIsolated { self.secrets = have } }
        }
    }
    func conn(_ id: String) -> APConnection? { list.first { $0.id == id } }
    var webhooks: [APConnection] { list.filter { $0.service == .webhook } }

    private func write() -> Bool {
        try? FileManager.default.createDirectory(atPath: autopilotRoot, withIntermediateDirectories: true)
        guard apWriteJSON(path, apConnectionsJSON(list)) else { return false }
        chmod(path, 0o600)
        return true
    }
    // 토큰·webhook 주소 꼴 — 보이는 ASCII 만(키체인에서 읽을 때 16진수로 바뀌지 않게), 4000자까지
    static func secretOK(_ s: String) -> Bool {
        !s.isEmpty && s.count <= 4000 && s.unicodeScalars.allSatisfy { $0.value > 0x20 && $0.value < 0x7F }
    }
    // 저장 — 칸을 다시 보고, 토큰을 새로 넣었으면 키체인에(표준 입력으로). 잘못이면 까닭
    func save(_ c0: APConnection, secret: String) -> String? {
        var c = c0
        c.account = c.account.trimmingCharacters(in: .whitespaces)
        c.expires = c.expires.trimmingCharacters(in: .whitespaces)
        if !c.expires.isEmpty, apDay(c.expires) == nil { return "만료일은 yyyy-MM-dd 꼴로 씁니다(예: 2027-01-31)" }
        switch c.service {
        case .atlassian:
            switch apSiteCheck(c.site, dev: apDevBuild) { case .ok(let s): c.site = s; case .bad(let why): return "사이트: " + why }
            if !c.account.contains("@") { return "Atlassian 계정 메일을 넣습니다(API 토큰과 함께 씀)" }
        case .notion, .figma:
            if c.site.trimmingCharacters(in: .whitespaces).isEmpty { c.site = "" }
            else if !apDevBuild { c.site = "" }
            else if case .ok(let s) = apSiteCheck(c.site, dev: true), s.hasPrefix("http://127.0.0.1") { c.site = s }
            else { return "시험 빌드의 바탕 주소는 http://127.0.0.1:<포트> 만 씁니다" }
        case .webhook:
            if !secret.isEmpty {
                switch apWebhookCheck(secret, dev: apDevBuild) { case .ok: c.host = apHost(secret); case .bad(let why): return "webhook 주소: " + why }
            }
        }
        let t = secret.trimmingCharacters(in: .whitespacesAndNewlines)
        if !t.isEmpty {
            guard Self.secretOK(t) else { return "토큰(주소)에 빈칸이나 ASCII 밖 글자가 있습니다" }
            guard Keychain.write(service, account: c.id, Data(t.utf8)) else { return "키체인에 쓰지 못했습니다" }
        } else if !secrets.contains(c.id) {
            return c.service == .webhook ? "webhook 주소를 넣습니다" : "토큰을 넣습니다(저장하면 키체인에만 둡니다)"
        }
        c.checked = nil   // 칸이 바뀌면 지난 확인은 지운다
        if let i = list.firstIndex(where: { $0.id == c.id }) { list[i] = c } else { list.append(c) }
        guard write() else { return "연결 목록 파일을 쓰지 못했습니다" }
        notes[c.id] = ("저장했습니다. 토큰은 키체인(\(service))에만 있습니다.", false)
        refreshSecrets()
        return nil
    }
    func remove(_ id: String) {
        Keychain.delete(service, account: id)
        list.removeAll { $0.id == id }
        _ = write()
        notes[id] = ("지웠습니다(키체인 항목과 연결 목록 줄).", false)
        refreshSecrets()
    }
    func nextWebhookID() -> String {
        let used = Set(webhooks.map(\.id))
        return (1...999).map { "webhook-\($0)" }.first { !used.contains($0) } ?? "webhook-999"
    }
    // 연결 확인 — 앱 안 엔진이 읽기 요청 하나(토큰이 없거나 주소 규칙 밖이면 요청 0). 결과를 연결 목록에 적는다
    func check(_ id: String) {
        guard !checking.contains(id) else { return }
        checking.insert(id)
        notes[id] = ("확인하는 중…", false)
        let dev = apDevBuild
        DispatchQueue.global(qos: .userInitiated).async {
            let r = apEngine(["connection", "check", id] + (dev ? ["--dev"] : []))
            let o = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [String: Any] ?? [:]
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.checking.remove(id)
                    let ok = o["ok"] as? Bool ?? false
                    let note = (o["note"] as? String).map { apDocText($0, 300) } ?? (r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : apDocText(r.out, 300))
                    let who = apDocText(o["who"] as? String ?? "", 120)
                    self.notes[id] = (ok ? "연결됨 · \(who)" : note, !ok)
                    if let i = self.list.firstIndex(where: { $0.id == id }), o["kind"] as? String != "local" || ok {
                        self.list[i].checked = APCheckRecord(at: Date().timeIntervalSince1970, ok: ok, who: who, note: note)
                        _ = self.write()
                    }
                }
            }
        }
    }
}

// 설정 › 연결의 항목들(설정 화면 표 SettingsItem)
@MainActor func apConnectionItems() -> [SettingsItem] {
    APConnections.shared.loadOnce()
    return [
        SettingsItem(id: "conn.about", section: .connections, card: "연결", title: "바깥 연결",
                     detail: "자율주행이 바깥에 쓸 때(검수 뒤 올림·자동으로 올림)와 읽을 때 쓰는 계정입니다. 토큰은 키체인에만 두고, 연결 목록에는 서비스·계정·범위·만료일만 남습니다. 모델 세션에는 넘기지 않습니다.",
                     keywords: "Atlassian Jira Confluence Notion Figma webhook 토큰 키체인", kind: .block) { AnyView(APConnectionsHelp()) },
        SettingsItem(id: "conn.atlassian", section: .connections, card: "서비스", title: "Atlassian (Jira·Confluence)",
                     detail: "사이트·계정 메일·API 토큰. 연결 확인은 내 계정 읽기 요청 하나입니다.", keywords: "jira confluence api 토큰 사이트", kind: .block) {
            AnyView(APConnectionCard(service: .atlassian))
        },
        SettingsItem(id: "conn.notion", section: .connections, card: "서비스", title: "Notion",
                     detail: "내부 통합 토큰. 받을 페이지에 그 통합을 초대해야 씁니다. 작성자 칸은 통합 이름으로 남아 첫 줄 표지와 요청자 이름으로 보완합니다.",
                     keywords: "notion 통합 integration 토큰", kind: .block) { AnyView(APConnectionCard(service: .notion)) },
        SettingsItem(id: "conn.figma", section: .connections, card: "서비스", title: "Figma",
                     detail: "개인 액세스 토큰(읽기). 프레임 읽기는 다음 조각에서 씁니다.", keywords: "figma 토큰 읽기", kind: .block) { AnyView(APConnectionCard(service: .figma)) },
        SettingsItem(id: "conn.webhook", section: .connections, card: "알림", title: "알림 webhook",
                     detail: "대화방 webhook 주소(주소 전체가 비밀이라 키체인에). 상태 알림·3시간 요약은 다음 조각에서 보냅니다.",
                     keywords: "webhook slack 두레이 google chat 알림", kind: .block) { AnyView(APWebhookCard()) },
    ]
}

struct APConnectionsHelp: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            apNote("· 키체인 항목: \(apSecretService(dev: apDevBuild)) (계정 이름은 서비스 id)." + (apDevBuild ? " 시험 빌드라 설치판 항목과 따로입니다." : ""))
            apNote("· 연결이 없거나 토큰이 없거나 만료일이 지난 서비스로는 요청을 보내지 않습니다(0건). 그동안 쓸 글은 실행의 올릴 글에 쌓이고 일은 계속합니다.")
            apNote(apDevBuild ? "· 사이트 주소는 https 만 받습니다. 시험 빌드만 http://127.0.0.1 (가짜 서버)을 받습니다." : "· 사이트 주소는 https 만 받습니다.")
            apNote("· 받는 쪽이 다른 주소로 보내도(리다이렉트) 따라가지 않습니다. 토큰은 연결한 사이트로만 갑니다.")
        }
    }
}

// 서비스 하나 — 상태 꼬리표, 칸(사이트·계정·토큰·만료일·범위), 저장·연결 확인·지우기
struct APConnectionCard: View {
    let service: APService
    @ObservedObject private var m = APConnections.shared
    @State private var site = ""
    @State private var account = ""
    @State private var token = ""
    @State private var expires = ""
    @State private var scopes: Set<String> = []
    @State private var problem: String?
    @State private var filled = false

    private var id: String { service.rawValue }
    private var saved: APConnection? { m.conn(id) }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                status
                Spacer(minLength: 4)
                if let c = saved, let k = c.checked, k.at > 0 {
                    Text("마지막 확인 " + apConnDay(k.at)).boardFont(10.5).foregroundStyle(.secondary)
                }
            }
            if service == .atlassian {
                field("사이트", "https://<이름>.atlassian.net", $site, axid: "conn-\(id)-site")
            } else if apDevBuild {
                field("바탕 주소", "비우면 \(service == .notion ? apNotionAPI : apFigmaAPI) · 시험 빌드만 http://127.0.0.1:<포트>", $site, axid: "conn-\(id)-site")
            }
            field(service == .notion ? "요청자 이름" : "계정 메일", service == .notion ? "본문 둘째 줄에 씀(작성자 칸 보완)" : "kim@example.com", $account, axid: "conn-\(id)-account")
            HStack(spacing: 8) {
                Text("토큰").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 74, alignment: .leading)
                SecureField(m.secrets.contains(id) ? "키체인에 있음 · 바꾸려면 새로 넣기" : "API 토큰", text: $token)
                    .textFieldStyle(.roundedBorder).font(.system(size: 11.5)).accessibilityIdentifier("conn-\(id)-token")
            }
            field("만료일", "yyyy-MM-dd · 토큰을 만들 때 고른 날(없으면 비움)", $expires, axid: "conn-\(id)-expires")
            HStack(spacing: 8) {
                Text("범위").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 74, alignment: .leading)
                WrapRow(spacing: 4) {
                    ForEach(service.scopeChoices, id: \.key) { s in
                        APButton(title: (scopes.contains(s.key) ? "✓ " : "") + s.label, tint: scopes.contains(s.key) ? Palette.accent : nil, axid: "conn-\(id)-scope-\(s.key)") {
                            if scopes.contains(s.key) { scopes.remove(s.key) } else { scopes.insert(s.key) }
                        }
                    }
                }
            }
            HStack(spacing: 6) {
                APButton(title: "저장", primary: true, axid: "conn-\(id)-save") { save() }
                APButton(title: m.checking.contains(id) ? "확인하는 중" : "연결 확인", enabled: saved != nil && !m.checking.contains(id),
                         help: "읽기 요청 하나로 토큰과 사이트를 확인합니다(토큰이 없거나 주소가 규칙 밖이면 요청하지 않음)", axid: "conn-\(id)-check") { m.check(id) }
                APButton(title: "지우기", tint: APColor.fail, enabled: saved != nil || m.secrets.contains(id), help: "키체인 항목과 연결 목록 줄을 지웁니다", axid: "conn-\(id)-remove") {
                    m.remove(id); token = ""; filled = false; fill()
                }
                Spacer(minLength: 0)
            }
            if let p = problem { apNote(p, APColor.fail).accessibilityIdentifier("conn-\(id)-problem") }
            if let n = m.notes[id] { apNote(n.text, n.bad ? APColor.fail : APColor.ok).accessibilityIdentifier("conn-\(id)-note") }
        }
        .onAppear { fill() }
    }
    private func fill() {
        guard !filled else { return }
        filled = true
        let c = saved
        site = c?.site ?? ""; account = c?.account ?? ""; expires = c?.expires ?? ""
        scopes = Set(c?.scopes ?? service.scopeChoices.filter { $0.key != "read" || service == .figma }.map(\.key))
    }
    private func save() {
        var c = saved ?? APConnection(id: id, service: service)
        c.site = site; c.account = account; c.expires = expires
        c.scopes = service.scopeChoices.map(\.key).filter { scopes.contains($0) }
        problem = m.save(c, secret: token)
        if problem == nil { token = ""; filled = false; fill() }
    }
    private var status: some View {
        let has = m.secrets.contains(id)
        let (t, col): (String, Color) = {
            guard let c = saved else { return ("연결 안 함", APColor.muted) }
            if !has { return ("토큰 없음", APColor.fail) }
            switch apExpiry(c.expires, now: Date(), tz: .current) {
            case .expired: return (apExpiryLabel(apExpiry(c.expires, now: Date(), tz: .current), c.expires), APColor.fail)
            case .soon: return (apExpiryLabel(apExpiry(c.expires, now: Date(), tz: .current), c.expires), APColor.limit)
            default: break
            }
            if let k = c.checked { return k.ok ? ("연결됨 · \(k.who)", APColor.ok) : ("확인 실패", APColor.fail) }
            return ("저장됨 · 확인 전", APColor.wait)
        }()
        return HStack(spacing: 6) {
            APChip(text: t, color: col, strong: true).accessibilityIdentifier("conn-\(id)-status")
            if let c = saved, !c.expires.isEmpty, case .ok = apExpiry(c.expires, now: Date(), tz: .current) {
                Text(apExpiryLabel(apExpiry(c.expires, now: Date(), tz: .current), c.expires)).boardFont(10.5).foregroundStyle(.secondary)
            }
        }
    }
    private func field(_ k: String, _ ph: String, _ b: Binding<String>, axid: String) -> some View {
        HStack(spacing: 8) {
            Text(k).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 74, alignment: .leading)
            TextField(ph, text: b).textFieldStyle(.roundedBorder).font(.system(size: 11.5)).accessibilityIdentifier(axid)
        }
    }
}

func apConnDay(_ ts: Double) -> String {
    let f = DateFormatter(); f.locale = Locale(identifier: "ko_KR"); f.dateFormat = "M월 d일 HH:mm"
    return f.string(from: Date(timeIntervalSince1970: ts))
}

// 알림 webhook — 여러 개(이름 + 주소). 주소는 키체인에, 목록에는 이름·호스트만. 읽는 요청이 없어 연결 확인은 주소 꼴만(요청 0)
struct APWebhookCard: View {
    @ObservedObject private var m = APConnections.shared
    @State private var name = ""
    @State private var url = ""
    @State private var problem: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if m.webhooks.isEmpty { apNote("아직 없습니다.") }
            ForEach(m.webhooks, id: \.id) { h in
                HStack(spacing: 6) {
                    APChip(text: m.secrets.contains(h.id) ? "주소 있음" : "주소 없음", color: m.secrets.contains(h.id) ? APColor.ok : APColor.fail)
                    Text(h.name.isEmpty ? h.id : h.name).boardFont(11.5, weight: .semibold)
                    Text(h.host).boardFont(10.5, design: .monospaced).foregroundStyle(.secondary)
                    Spacer(minLength: 4)
                    APButton(title: "지우기", tint: APColor.fail, axid: "conn-\(h.id)-remove") { m.remove(h.id) }
                }
            }
            HStack(spacing: 8) {
                TextField("이름(예: 팀 대화방)", text: $name).textFieldStyle(.roundedBorder).font(.system(size: 11.5)).frame(maxWidth: 180)
                    .accessibilityIdentifier("conn-webhook-name")
                SecureField(apDevBuild ? "https://… (시험 빌드는 http://127.0.0.1 도)" : "https://…", text: $url).textFieldStyle(.roundedBorder).font(.system(size: 11.5))
                    .accessibilityIdentifier("conn-webhook-url")
                APButton(title: "더하기", primary: true, axid: "conn-webhook-add") {
                    let c = APConnection(id: m.nextWebhookID(), service: .webhook, name: apDocText(name, 80), scopes: ["notify"])
                    problem = m.save(c, secret: url)
                    if problem == nil { name = ""; url = "" }
                }
            }
            if let p = problem { apNote(p, APColor.fail) }
            apNote("webhook 은 읽는 요청이 없어 연결 확인을 하지 않습니다(주소 꼴만 봅니다). 시험 글 보내기·상태 알림·3시간 요약은 다음 조각에서 만듭니다.")
        }
    }
}

// MARK: 올릴 글 검수

// 보드가 읽는 올릴 글 한 줄 — 실행 폴더의 outbox/<id>.json(엔진이 쓴 것, 보통 파일·상한 아래일 때만)
struct APOutboxEntry: Identifiable, Equatable {
    let item: APOutboxItem
    let runID: String
    let runDir: String
    let runTitle: String
    let attachBytes: Int?
    var id: String { runID + "/" + item.id }
}

@MainActor final class APOutboxReview: ObservableObject {
    static let shared = APOutboxReview()
    @Published private(set) var entries: [APOutboxEntry] = []
    @Published var selected: String?
    @Published var filterRun: String?
    @Published private(set) var busy: Set<String> = []
    @Published var confirmAll = false
    @Published var confirmOne: String?
    @Published private(set) var note: (text: String, bad: Bool)?
    @Published private(set) var shown = false
    @Published var showDetail = false          // 작은 크기: 목록 → 미리보기
    private var stamps: [String: Date] = [:]
    private var cache: [String: [APOutboxEntry]] = [:]
    private var timer: Timer?
    private var scanning = false
    private var sheet: NSWindow?
    private var resizeWatch: NSObjectProtocol?

    // 3초마다 runs/*/outbox 폴더 시각만 보고 바뀐 실행만 다시 읽는다(보드 CPU)
    func startWatching() {
        guard timer == nil else { return }
        scan()
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { _ in MainActor.assumeIsolated { APOutboxReview.shared.scan() } }
    }
    func scan() {
        guard !scanning else { return }
        scanning = true
        let old = stamps, oldCache = cache
        DispatchQueue.global(qos: .utility).async {
            let fm = FileManager.default, runs = autopilotRoot + "/runs"
            var st: [String: Date] = [:], out: [String: [APOutboxEntry]] = [:]
            for rid in ((try? fm.contentsOfDirectory(atPath: runs)) ?? []).sorted() where !rid.hasPrefix(".") {
                let dir = runs + "/" + rid, box = dir + "/outbox"
                guard let m = (try? fm.attributesOfItem(atPath: box))?[.modificationDate] as? Date else { continue }
                st[rid] = m
                if old[rid] == m, let c = oldCache[rid] { out[rid] = c; continue }
                let title = ((APFolderReportSource.plainFile(dir + "/run.json")).flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any])?["title"] as? String ?? rid
                let att = ((try? fm.attributesOfItem(atPath: dir + "/report.html"))?[.size] as? NSNumber)?.intValue
                out[rid] = ((try? fm.contentsOfDirectory(atPath: box)) ?? []).sorted().compactMap { n -> APOutboxEntry? in
                    guard n.hasSuffix(".json"), apOutboxIDOK(String(n.dropLast(5))), let d = APFolderReportSource.plainFile(box + "/" + n),
                          let it = apOutboxParse(d), it.run == rid else { return nil }
                    return APOutboxEntry(item: it, runID: rid, runDir: dir, runTitle: apDocText(title, 200), attachBytes: att)
                }
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.scanning = false
                    self.stamps = st; self.cache = out
                    let all = out.values.flatMap { $0 }.sorted { a, b in
                        let ra = APOutboxReview.rank(a.item), rb = APOutboxReview.rank(b.item)
                        return ra != rb ? ra < rb : a.item.created > b.item.created
                    }
                    if all != self.entries { self.entries = all }
                }
            }
        }
    }
    static func rank(_ it: APOutboxItem) -> Int {
        switch it.state {
        case .waiting: return it.later == nil ? 0 : 2
        case .failed, .uncertain, .posting: return 1
        case .posted: return 3
        case .skipped: return 4
        }
    }
    static func isWaiting(_ it: APOutboxItem) -> Bool { [.waiting, .failed, .uncertain].contains(it.state) }
    var waitingCount: Int { entries.filter { Self.isWaiting($0.item) }.count }
    func count(run: String) -> Int { entries.filter { $0.runID == run && Self.isWaiting($0.item) }.count }
    func total(run: String) -> Int { entries.filter { $0.runID == run }.count }
    var visible: [APOutboxEntry] { filterRun.map { r in entries.filter { $0.runID == r } } ?? entries }
    var current: APOutboxEntry? { visible.first { $0.id == selected } ?? visible.first }

    // 올릴지 — 규칙 apPostDecide 로(사람이 누름). 보드는 엔진과 같은 연결 목록·키체인 있음 여부로 본다
    func decision(_ e: APOutboxEntry) -> APPostDecision {
        let cs = APConnections.shared
        let c = cs.conn(e.item.connection)
        return apPostDecide(e.item, trigger: .press, connection: c, hasSecret: c.map { cs.secrets.contains($0.id) } ?? false,
                            dev: apDevBuild, now: Date(), tz: .current)
    }
    func canPost(_ e: APOutboxEntry) -> Bool { if case .go = decision(e) { return !busy.contains(e.id) } ; return false }
    var postable: [APOutboxEntry] { visible.filter { [.waiting, .failed].contains($0.item.state) && canPost($0) } }

    func open(run: String? = nil) {
        APConnections.shared.load()
        filterRun = run
        selected = nil; showDetail = false; confirmAll = false; confirmOne = nil; note = nil
        scan()
        present()
    }
    private func present(_ tries: Int = 0) {
        guard window != nil, sheet == nil else { sheet?.makeKeyAndOrderFront(nil); return }
        if window.attachedSheet != nil {
            if tries < 20 { DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { MainActor.assumeIsolated { self.present(tries + 1) } } }
            return
        }
        let c = window.contentLayoutRect.size
        let size = NSSize(width: max(c.width - 8, 480), height: max(c.height - 8, 460))
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        let host = NSHostingController(rootView: APOutboxSheet(m: self))
        host.sizingOptions = []
        w.contentViewController = host
        w.setContentSize(size)
        sheet = w
        shown = true
        window.beginSheet(w)
        resizeWatch = NotificationCenter.default.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let s = self?.sheet else { return }
                let c = window.contentLayoutRect.size
                s.setContentSize(NSSize(width: max(c.width - 8, 480), height: max(c.height - 8, 460)))
            }
        }
    }
    func close() {
        if let s = sheet { window.endSheet(s); s.orderOut(nil); s.contentViewController = nil }
        sheet = nil
        if let r = resizeWatch { NotificationCenter.default.removeObserver(r) }
        resizeWatch = nil
        shown = false
    }

    // 엔진 부르기 — 글마다 한 번에 하나(누르는 동안 그 글 단추 잠금). 엔진도 글마다 잠금 안에서 하고, 이미 올린 글은 요청 0
    private func engine(_ e: [APOutboxEntry], _ verb: String) {
        let todo = e.filter { !busy.contains($0.id) }
        guard !todo.isEmpty else { return }
        for x in todo { busy.insert(x.id) }
        note = (verb == "post" ? "올리는 중… (글 \(todo.count)개)" : "바꾸는 중…", false)
        let dev = apDevBuild
        DispatchQueue.global(qos: .userInitiated).async {
            var lines: [String] = [], bad = false
            for (dir, group) in Dictionary(grouping: todo, by: \.runDir) {
                let r = apEngine(["outbox", dir, verb] + group.map(\.item.id) + ["--json"] + (dev ? ["--dev"] : []))
                let list = (try? JSONSerialization.jsonObject(with: Data(r.out.utf8))) as? [[String: Any]]
                if let list {
                    for o in list {
                        let st = o["state"] as? String ?? ""
                        if verb == "post", st != "posted" { bad = true }
                        lines.append(apDocText(o["note"] as? String ?? "", 200))
                    }
                } else { bad = true; lines.append(r.out.isEmpty ? "엔진이 답하지 않았습니다(\(r.code))" : apDocText(r.out, 300)) }
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    for x in todo { self.busy.remove(x.id) }
                    self.note = (Array(Set(lines)).sorted().joined(separator: " · "), bad)
                    self.stamps = [:]   // 바로 다시 읽기
                    self.scan()
                }
            }
        }
    }
    func post(_ e: APOutboxEntry) {
        if e.item.state == .uncertain, confirmOne != e.id { confirmOne = e.id; return }
        confirmOne = nil
        engine([e], "post")
    }
    func postAll() { confirmAll = false; engine(postable, "post") }
    func mark(_ e: APOutboxEntry, _ verb: String) { engine([e], verb) }
}

// 시트 — 머리(수·모두 올리기·닫기), 실행 줄, 목록, 미리보기(받을 곳·계정·표지·본문·첨부·단추). 좁으면 목록 → 미리보기
struct APOutboxSheet: View {
    @ObservedObject var m: APOutboxReview
    @ObservedObject private var conns = APConnections.shared
    @ObservedObject private var ap = AutopilotStore.shared
    var body: some View {
        GeometryReader { g in
            let narrow = g.size.width < 620
            VStack(alignment: .leading, spacing: 0) {
                head
                Divider().opacity(0.4)
                if m.visible.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        apNote("올릴 글이 없습니다. 게시 방식의 받을 곳(보고서·Jira 댓글)을 정한 실행이 단위를 마치거나 끝나면 여기에 쌓입니다.")
                    }.padding(14)
                    Spacer(minLength: 0)
                } else if narrow {
                    if m.showDetail, let e = m.current {
                        VStack(alignment: .leading, spacing: 6) {
                            APBack(title: "← 목록", axid: "ap-outbox-back") { m.showDetail = false }
                            ScrollView { detail(e).padding(.bottom, 12) }
                        }.padding(12)
                    } else { ScrollView { list.padding(12) } }
                } else {
                    HStack(alignment: .top, spacing: 0) {
                        ScrollView { list.padding(12) }.frame(width: min(320, g.size.width * 0.4))
                        Divider().opacity(0.4)
                        ScrollView { if let e = m.current { detail(e).padding(14) } }
                    }
                }
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))   // 겉에 접근성 id 를 달지 않는다(안의 단추 id 를 덮었다, GUI 시험에서 봄)
    }

    private var head: some View {
        let wait = m.visible.filter { APOutboxReview.isWaiting($0.item) }.count
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: "paperplane").font(.system(size: 12, weight: .bold)).foregroundStyle(Palette.accent)
                Text("올릴 글 \(m.visible.count)개").boardFont(13, weight: .bold).accessibilityIdentifier("ap-outbox-count")
                if wait > 0 { APChip(text: "기다림 \(wait)", color: APColor.wait) }
                Spacer(minLength: 4)
                APButton(title: "모두 올리기", primary: true, enabled: !m.postable.isEmpty,
                         help: m.postable.isEmpty ? "올릴 수 있는 글이 없습니다(연결이 없거나 이미 올림)" : "기다리는 글을 한 번에 올립니다. 화면 안에서 한 번 확인합니다",
                         axid: "ap-outbox-all") { m.confirmAll = true }
                APButton(title: "닫기", axid: "ap-outbox-close") { m.close() }
            }
            if m.confirmAll {
                let p = m.postable
                HStack(spacing: 6) {
                    Text("글 \(p.count)개를 올립니다: \(apOutboxCounts(p.map(\.item))). 올린 뒤에는 보드에서 되돌릴 수 없습니다.").boardFont(11.5)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    APButton(title: "올리기", primary: true, axid: "ap-outbox-all-yes") { m.postAll() }
                    APButton(title: "취소", axid: "ap-outbox-all-no") { m.confirmAll = false }
                }
                .padding(8).background(RoundedRectangle(cornerRadius: 8).fill(Palette.accent.opacity(0.10)))
            }
            runLine
            if let n = m.note { apNote(n.text, n.bad ? APColor.fail : APColor.ok).accessibilityIdentifier("ap-outbox-note") }
        }
        .padding(12)
    }

    // 실행 줄 — 보고 있는 실행의 지금 상태와 합치기·승격으로 잇기(검수를 기다리는 동안에도 자율주행은 계속)
    @ViewBuilder private var runLine: some View {
        if let rid = m.filterRun ?? (m.selected != nil ? m.current?.runID : nil) {
            let live = ap.runs.first { $0.id == rid }
            HStack(spacing: 6) {
                Text(m.entries.first { $0.runID == rid }?.runTitle ?? rid).boardFont(11, weight: .semibold).lineLimit(1)
                if let r = live {
                    APChip(text: apStateChip(r).0, color: apStateChip(r).1)
                    if r.active { Text("검수를 기다리는 동안에도 계속합니다").boardFont(10.5).foregroundStyle(.secondary).lineLimit(1) }
                }
                Spacer(minLength: 4)
                APButton(title: "합치기·승격 →", help: "끝난 실행의 바뀐 줄을 보고 승격 차례대로 합칩니다", axid: "ap-outbox-merge") {
                    let dir = autopilotRoot + "/runs/" + rid
                    m.close()
                    APMerge.shared.open(dir)
                }
            }
        }
    }

    private var list: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(m.visible) { e in
                let on = e.id == m.current?.id
                Button {
                    m.selected = e.id; m.showDetail = true; m.confirmOne = nil
                } label: {
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 5) {
                            APChip(text: apServiceName(e.item.target.service), color: APColor.jira)
                            Text(e.item.title).boardFont(11.5, weight: .semibold).lineLimit(2).multilineTextAlignment(.leading)
                            Spacer(minLength: 0)
                        }
                        HStack(spacing: 5) {
                            APChip(text: apOutboxStateLabel(e.item), color: apOutboxColor(e.item))
                            Text(apOutboxAge(e.item)).boardFont(10.5).foregroundStyle(.secondary)
                            if m.filterRun == nil { Text(e.runTitle).boardFont(10.5).foregroundStyle(.tertiary).lineLimit(1) }
                        }
                    }
                    .padding(7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(on ? 0.09 : 0.03)))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("ap-outbox-row-\(e.item.id)")
            }
        }
    }

    private func detail(_ e: APOutboxEntry) -> some View {
        let it = e.item
        let c = conns.conn(it.connection)
        let d = m.decision(e)
        return VStack(alignment: .leading, spacing: 10) {
            kv("받을 곳", apTargetLabel(it.target), axid: "ap-outbox-target")
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("올릴 계정").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
                if let c, conns.secrets.contains(c.id) {
                    Text("\(c.account.isEmpty ? "(계정 칸 없음)" : c.account) · \(c.service.label) 연결" + (c.site.isEmpty ? "" : " · " + c.site.replacingOccurrences(of: "https://", with: "")))
                        .boardFont(11.5).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("ap-outbox-account")
                } else {
                    APChip(text: "연결 없음", color: APColor.fail, strong: true)
                    Text("올릴 수 없습니다. 이 글은 보고서와 함께 로컬에만 남습니다.").boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    APButton(title: "연결 설정", axid: "ap-outbox-settings") { m.close(); SettingsScreen.shared.open(.connections) }
                }
                Spacer(minLength: 0)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("미리보기 · 올라갈 모습 그대로입니다(여기서 글을 고치지 않습니다)").boardFont(10.5).foregroundStyle(.secondary)
                if it.target.service != .jira { Text(it.title).boardFont(13, weight: .bold).fixedSize(horizontal: false, vertical: true) }
                APDocView(doc: it.doc)
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.04)))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("ap-outbox-preview")
            if let a = it.attachment {
                HStack(spacing: 6) {
                    Text("첨부").boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
                    Text("\(a) · " + (e.attachBytes.map { apBytes($0) } ?? "파일 없음")).boardFont(11.5, design: .monospaced).accessibilityIdentifier("ap-outbox-attach")
                    APButton(title: "열어 보기", enabled: e.attachBytes != nil, help: "같은 실행의 보고서를 엽니다. 흐름 그림은 이 HTML 안에만 있습니다", axid: "ap-outbox-open-report") {
                        APReportViewer.show(run: e.runID, runDir: e.runDir, title: e.runTitle)
                    }
                    Spacer(minLength: 0)
                }
            } else if it.kind == "report" {
                apNote("Notion 에는 보고서 파일을 첨부하지 않습니다(요약만). 흐름 그림과 단위별 기록은 보드의 보고서에 있습니다.")
            }
            if let r = it.remote, !r.url.isEmpty {
                kv(it.state == .posted ? "올린 주소" : "만든 곳", r.url, axid: "ap-outbox-url", mono: true)
            }
            if let l = it.last, !l.note.isEmpty {
                apNote("마지막: \(apConnDay(l.at)) · \(l.note)" + (l.code > 0 ? " (\(l.code))" : ""), it.state == .failed || it.state == .uncertain ? APColor.fail : .secondary)
                    .accessibilityIdentifier("ap-outbox-last")
            }
            if case .hold(let why) = d { apNote(why, APColor.fail).accessibilityIdentifier("ap-outbox-why") }
            if m.confirmOne == e.id {
                HStack(spacing: 6) {
                    Text("앞 올리기가 끝났는지 모릅니다. 받을 곳에 같은 글이 이미 있는지 먼저 보세요. 그래도 올릴까요?").boardFont(11.5).fixedSize(horizontal: false, vertical: true)
                    APButton(title: "그래도 올리기", tint: APColor.limit, axid: "ap-outbox-post-anyway") { m.post(e) }
                    APButton(title: "그만두기", axid: "ap-outbox-post-cancel") { m.confirmOne = nil }
                }
                .padding(8).background(RoundedRectangle(cornerRadius: 8).fill(APColor.limit.opacity(0.10)))
            }
            HStack(spacing: 6) {
                let busy = m.busy.contains(e.id)
                APButton(title: busy ? "올리는 중" : (it.state == .posted ? "올렸습니다" : "올리기"), primary: true, enabled: m.canPost(e),
                         help: { if case .nothing(let w) = d { return w }; if case .hold(let w) = d { return w }; return "이 글을 받을 곳에 올립니다" }(),
                         axid: "ap-outbox-post") { m.post(e) }
                APButton(title: "올리지 않기(로컬에만)", enabled: !busy && it.state != .posted && it.state != .skipped, axid: "ap-outbox-skip") { m.mark(e, "skip") }
                APButton(title: "나중에", enabled: !busy && it.state != .posted && it.state != .skipped, help: "목록에 남깁니다. 오래 두어도 자동으로 올리지 않습니다", axid: "ap-outbox-later") { m.mark(e, "later") }
                if it.state == .skipped { APButton(title: "다시 기다림으로", enabled: !busy, axid: "ap-outbox-unskip") { m.mark(e, "unskip") } }
                Spacer(minLength: 0)
            }
            apNote(it.mode == .review ? "검수 뒤 올림: 누를 때만 올라갑니다. 오래 두어도 자동으로 올리지 않습니다." : "자동으로 올림으로 만든 글입니다. 올리지 못했으면 여기서 다시 올립니다.")
        }
    }
    private func kv(_ k: String, _ v: String, axid: String, mono: Bool = false) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(k).boardFont(11, weight: .semibold).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            Text(v).boardFont(11.5, design: mono ? .monospaced : .default).textSelection(.enabled).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier(axid)
            Spacer(minLength: 0)
        }
    }
}

func apServiceName(_ s: APTargetService) -> String {
    switch s { case .jira: "Jira 댓글"; case .confluence: "Confluence"; case .notion: "Notion" }
}
func apOutboxColor(_ it: APOutboxItem) -> Color {
    switch it.state {
    case .posted: APColor.ok
    case .failed, .uncertain: APColor.fail
    case .skipped: APColor.muted
    case .posting: APColor.work
    case .waiting: it.later != nil ? APColor.muted : APColor.wait
    }
}
func apOutboxAge(_ it: APOutboxItem) -> String {
    let d = Date(timeIntervalSince1970: it.created)
    let days = Calendar.current.dateComponents([.day], from: Calendar.current.startOfDay(for: d), to: Calendar.current.startOfDay(for: Date())).day ?? 0
    let f = DateFormatter(); f.locale = Locale(identifier: "ko_KR"); f.dateFormat = "HH:mm"
    return days <= 0 ? "오늘 " + f.string(from: d) : "\(days + 1)일째"
}
func apBytes(_ n: Int) -> String { n < 1024 ? "\(n) B" : n < 1 << 20 ? "\(n / 1024) KB" : String(format: "%.1f MB", Double(n) / Double(1 << 20)) }

// 올릴 글 본문 미리보기 — APDoc 을 그대로 그린다(웹 보기 없이, 글은 늘 글자 그대로: 마크다운·HTML 로 읽지 않음)
struct APDocView: View {
    let doc: APDoc
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(doc.blocks.enumerated()), id: \.offset) { _, b in block(b) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    @ViewBuilder private func block(_ b: APBlock) -> some View {
        switch b {
        case .heading(let l, let t): Text(verbatim: t).boardFont(l == 1 ? 15 : l == 2 ? 13.5 : 12, weight: .bold).fixedSize(horizontal: false, vertical: true)
        case .paragraph(let s): spans(s)
        case .bullets(let rows):
            VStack(alignment: .leading, spacing: 3) {
                ForEach(rows.indices, id: \.self) { i in
                    HStack(alignment: .firstTextBaseline, spacing: 5) { Text(verbatim: "•").boardFont(11.5); spans(rows[i]) }
                }
            }
        case .code(let t):
            Text(verbatim: t).font(.system(size: 11, design: .monospaced)).fixedSize(horizontal: false, vertical: true)
                .padding(6).background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.05)))
        case .rule: Divider().opacity(0.5)
        }
    }
    private func spans(_ a: [APSpan]) -> some View {
        a.reduce(Text(verbatim: "")) { acc, s in
            switch s {
            case .text(let t, let b): return acc + Text(verbatim: t).fontWeight(b ? .bold : .regular)
            case .link(let t, _): return acc + Text(verbatim: t).underline().foregroundColor(Palette.accent)
            }
        }
        .boardFont(11.5).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
    }
}
