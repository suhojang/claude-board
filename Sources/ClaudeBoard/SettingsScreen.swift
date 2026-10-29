// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import BoardRules
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI
import WebKit

// MARK: - 설정 화면

// 설정 화면(앱 메뉴 > 설정…, ⌘,) — 보드는 그대로 두고 터미널 영역(제목 막대 띠 아래)을 설정 화면으로 덮는다. 왼쪽 묶음 목록·검색, 오른쪽 항목.
// 전환 효과는 settingsTransition(옆으로 밀기 기본). 접혀 있으면 펼치는 것을 전환으로 삼고 닫을 때 다시 접는다 — 잠깐 펼친 것은
// terminalExpanded 에 남기지 않는다(열린 채 끄고 다시 켜도 접힌 채로 뜬다). 열린 동안 탭은 뒤에서 그대로 돌고, 탭을 다루는
// 동작(⌘T·⌘1·⌘D·세션 줄 누르기 등)은 설정 화면을 닫고 한다. Esc 는 키 받기 취소 → 검색어 지우기 → 닫기 차례(Shortcuts.handle)
enum SettingsSection: String, CaseIterable, Identifiable {
    case general, window, notify, terminal, board, account, shortcuts, update
    var id: String { rawValue }

    var title: String {
        switch self {
        case .general: "일반"
        case .window: "창"
        case .notify: "알림"
        case .terminal: "터미널"
        case .board: "보드"
        case .account: "계정"
        case .shortcuts: "단축키"
        case .update: "업데이트"
        }
    }

    var icon: String {
        switch self {
        case .general: "gearshape"
        case .window: "macwindow"
        case .notify: "bell"
        case .terminal: "terminal"
        case .board: "rectangle.grid.1x2"
        case .account: "person.crop.circle"
        case .shortcuts: "keyboard"
        case .update: "arrow.down.circle"
        }
    }

    var subtitle: String {
        switch self {
        case .general: "보드를 띄우는 방식과 제목 옆 캐릭터"
        case .window: "보드 자리, 투명도, 펼치기·접기와 설정 화면의 움직임"
        case .notify: "작업 완료·승인 대기 알림 패널"
        case .terminal: "내장 터미널의 글자·테마와 새 세션 기본값"
        case .board: "세션 보드의 글자 크기와 사용 한도 칸"
        case .account: "Claude 계정 전환과 한도 자동 전환"
        case .shortcuts: "키 칸을 누르고 새 키를 누르면 바로 바뀝니다"
        case .update: "새 버전 확인과 설치"
        }
    }
}

enum SettingsTransition: String, CaseIterable, Identifiable {
    case slide, fade, none
    var id: String { rawValue }
    var name: String {
        switch self {
        case .slide: "옆으로 밀기"
        case .fade: "서서히 바뀌기"
        case .none: "효과 없음"
        }
    }
    static var current: SettingsTransition {
        SettingsTransition(rawValue: UserDefaults.standard.string(forKey: "settingsTransition") ?? "") ?? .slide
    }
}

@MainActor final class SettingsScreen: ObservableObject {
    static let shared = SettingsScreen()
    @Published private(set) var shown = false
    @Published var section: SettingsSection = .general
    @Published var query = ""
    @Published private(set) var focusSearch = 0   // 바뀌면 검색 칸에 포커스
    @Published var visible = false                 // 화면에 올라와 있다(내려가는 전환 중에도) — 아니면 내용을 그리지 않는다
    private var collapseOnClose = false            // 접힌 보드에서 열었다 — 닫을 때 다시 접는다
    private var collapsedFrame: NSRect?            // 그때의 창 프레임(미니·손으로 줄인 크기 포함) — 닫을 때 그대로 돌린다
    private var observer: NSObjectProtocol?

    func toggle() { shown ? close() : open() }

    func open(_ s: SettingsSection? = nil) {
        if let s { section = s }
        showBoard()
        let hub = TerminalHub.shared
        guard !shown, let area = hub.area else { focusSearch += 1; return }
        hub.slide?.finish()
        query = ""
        ThemeList.shared.load()
        ShortcutSettings.shared.load()
        LoginItem.shared.refresh()
        shown = true
        collapseOnClose = !hub.expanded
        collapsedFrame = collapseOnClose ? window.frame : nil
        // 접혀 있으면 설정 화면을 먼저 깔고 펼친다 — 펼치기(애니메이션 설정을 따른다)가 전환 효과다
        area.present(true, style: collapseOnClose ? .none : .current)
        if collapseOnClose {
            setTerminalExpanded(true)
            UserDefaults.standard.set(false, forKey: "terminalExpanded")
        }
        window.makeFirstResponder(area.settings)
        focusSearch += 1
        // 다른 앱·다른 창으로 옮기면 키 받기를 그만둔다 — 풀어 둔 전역 단축키가 그대로 꺼져 있지 않게
        observer = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: window, queue: .main) { _ in
            MainActor.assumeIsolated { if ShortcutSettings.shared.recording != nil { ShortcutSettings.shared.stopRecording() } }
        }
    }

    // keep — 탭을 다루는 동작으로 닫을 때: 잠깐 펼쳤어도 펼친 채로 남긴다
    func close(keep: Bool = false) {
        guard shown else { return }
        finish()
        let hub = TerminalHub.shared
        if collapseOnClose, !keep {
            collapseOnClose = false
            let frame = collapsedFrame
            collapsedFrame = nil
            setTerminalExpanded(false)   // 접는 것이 전환 효과다 — 다 접힌 뒤 내리고 열 때의 프레임으로
            whenPaneSettled {
                guard !self.shown else { return }
                hub.area?.present(false, style: .none)
                if let frame, frame != window.frame {
                    window.setFrame(frame, display: true)
                    BoardFit.shared.update()
                }
            }
            return
        }
        if collapseOnClose { hub.setExpanded(true) }
        collapseOnClose = false
        collapsedFrame = nil
        hub.area?.present(false, style: .current)
        hub.focusSelected()
    }

    // 터미널 영역이 접혔다(⌃⌥T·처음 크기로 붙이기 등) — 효과 없이 내린다
    func dismissForCollapse() {
        guard shown else { return }
        finish()
        collapseOnClose = false
        collapsedFrame = nil
        TerminalHub.shared.area?.present(false, style: .none)
    }

    private func finish() {
        shown = false
        if ShortcutSettings.shared.recording != nil { ShortcutSettings.shared.stopRecording() }
        if let observer { NotificationCenter.default.removeObserver(observer) }
        observer = nil
    }

    func escape() { if query.isEmpty { close() } else { query = "" } }

    // 끄기·업데이트 직전 — 접힌 보드에서 잠깐 펼쳐 둔 것이면 접힌 프레임으로 돌려 둔다(다시 뜰 때 창 자리가 펼친 프레임을 따르지 않게)
    func restoreBeforeQuit() {
        guard shown, collapseOnClose, let f = collapsedFrame else { return }
        TerminalHub.shared.setExpanded(false)
        window.setFrame(f, display: false)
    }

    func go(_ s: SettingsSection) {
        if ShortcutSettings.shared.recording != nil { ShortcutSettings.shared.stopRecording() }
        withAnimation(.easeOut(duration: 0.15)) {
            section = s
            query = ""
        }
    }
}

// 로그인할 때 열기(설정 > 일반) — 설치가 등록하는 LaunchAgent(~/Library/LaunchAgents/<번들 id>.plist, user-setup.sh 6)를 켜고 끈다.
// 끄면 plist 를 지우고 표지 파일 login-launch-off 를 남겨 업데이트(user-setup.sh)가 다시 등록하지 않게 한다. 지금 도는 보드는 그대로 둔다
// (bootout 하면 launchd 가 띄운 이 보드가 꺼진다). 켜면 plist 를 다시 쓴다 — 다음 로그인부터. 시험 빌드는 번들 id 가 달라 설치판 것을 건드리지 않는다
@MainActor final class LoginItem: ObservableObject {
    static let shared = LoginItem()
    private let label = Bundle.main.bundleIdentifier ?? "com.claude.board"
    private var agent: String { NSHomeDirectory() + "/Library/LaunchAgents/\(label).plist" }
    private var offMark: String? { label == "com.claude.board" ? NSHomeDirectory() + "/.claude/notifier/login-launch-off" : nil }
    @Published private(set) var on = false

    init() { refresh() }

    func refresh() { on = FileManager.default.fileExists(atPath: agent) }

    func set(_ v: Bool) {
        let fm = FileManager.default
        if v {
            let plist: NSDictionary = ["Label": label, "ProgramArguments": [Bundle.main.executablePath ?? ""], "RunAtLoad": true]
            try? fm.createDirectory(atPath: (agent as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            if !plist.write(toFile: agent, atomically: true) { boardLog("로그인 항목 등록 실패: \(agent)") }
            if let offMark { try? fm.removeItem(atPath: offMark) }
        } else {
            try? fm.removeItem(atPath: agent)
            if let offMark { fm.createFile(atPath: offMark, contents: Data()) }
        }
        refresh()
    }
}

// 알림 위치 — 표지 파일 notify-position(notify.py 가 읽는다). 중앙(기본)은 파일을 지운다
let notifyPositions = [("top-left", "왼쪽 위"), ("top-right", "오른쪽 위"), ("center", "가운데"), ("bottom-left", "왼쪽 아래"), ("bottom-right", "오른쪽 아래")]

func notifyPosition() -> String {
    (try? String(contentsOfFile: notifyPositionPath, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "center"
}

func setNotifyPosition(_ value: String) {
    if value == "center" {
        try? FileManager.default.removeItem(atPath: notifyPositionPath)
    } else {
        try? value.write(toFile: notifyPositionPath, atomically: true, encoding: .utf8)
    }
}

// 설정 화면 색 — 터미널 영역과 같은 짙은 바탕 위에 옅은 카드
enum SettingsStyle {
    static let sidebar = Color(nsColor: terminalBarColor)
    static let content = Color(nsColor: terminalAreaColor)
    static let card = Color.white.opacity(0.035)
    static let cardLine = Color.white.opacity(0.07)
    static let line = Color.white.opacity(0.06)
    static let accent = Palette.color("working")
}

// 설정 화면 호스팅 뷰 — 글자 칸이 포커스를 받아야 해서 PassiveHostingView 가 아니다. 창 바탕 끌기(isMovableByWindowBackground)로 창이 움직이지 않게
// 빈 곳(SwiftUI 가 받지 않는 자리)을 눌러도 뒤에 가려진 터미널 화면으로 가지 않게 자기가 받는다
final class SettingsHostingView<Content: View>: NSHostingView<Content> {
    override var mouseDownCanMoveWindow: Bool { false }
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard !isHidden else { return nil }
        return super.hitTest(point) ?? (frame.contains(point) ? self : nil)
    }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

// 설정 한 줄 — 검색은 제목·설명·덧붙인 낱말로 맞춘다. kind 가 row 면 [제목·설명 … 조절기], block 이면 내용이 줄 전체를 쓴다
struct SettingsItem: Identifiable {
    enum Kind { case row, block }
    let id: String
    let section: SettingsSection
    let card: String
    let title: String
    var detail: String? = nil
    var keywords = ""
    var kind = Kind.row
    let content: () -> AnyView

    func matches(_ q: String) -> Bool {
        [title, detail ?? "", keywords, card].contains { $0.localizedCaseInsensitiveContains(q) }
    }
}

struct SettingsView: View {
    @ObservedObject private var model = SettingsScreen.shared
    @ObservedObject private var keys = Shortcuts.shared
    @ObservedObject private var recorder = ShortcutSettings.shared
    @ObservedObject private var themes = ThemeList.shared
    @ObservedObject private var opacity = WindowOpacity.shared
    @ObservedObject private var fontScale = BoardFontScale.shared
    @ObservedObject private var updater = Updater.shared
    @ObservedObject private var login = LoginItem.shared
    @AppStorage("boardSide") private var boardSide = "right"
    @AppStorage("pinned") private var pinned = true
    @AppStorage("mascot") private var mascot: Mascot = .tralalero
    @AppStorage("terminalAnimation") private var animation = false
    @AppStorage("settingsTransition") private var transition: SettingsTransition = .slide
    @AppStorage("terminalFontSize") private var terminalFont: Double = 0   // 0 = 설정 없음(Ghostty 설정 크기)
    @AppStorage("newSessionCommand") private var sessionCommand = ""
    @AppStorage("newSessionMdRender") private var sessionMdRender = true
    @AppStorage("usageCompact") private var usageCompact = false
    @AppStorage("autoUpdateCheck") private var autoUpdate = true
    @State private var position = notifyPosition()
    @State private var settle: DispatchWorkItem?
    @State private var width: CGFloat = 900   // 설정 화면 폭 — 좁으면(터미널 최소 폭 480 근처) 목록을 줄이고 조절기를 설명 아래로
    @FocusState private var searching: Bool

    private var sidebarWidth: CGFloat { width < 720 ? 164 : 200 }
    private var compact: Bool { width - sidebarWidth < 520 }

    private var app: AppDelegate? { NSApp.delegate as? AppDelegate }
    private var hub: TerminalHub { .shared }   // 지켜보지 않는다 — 탭·⌘ 누름마다 바뀌어 설정 화면을 다시 그리게 된다

    var body: some View {
        if model.visible { screen } else { Color.clear }
    }

    private var screen: some View {
        HStack(spacing: 0) {
            sidebar
            Rectangle().fill(SettingsStyle.line).frame(width: 1)
            content
        }
        .background(SettingsStyle.content)
        .background(GeometryReader { geo in
            Color.clear.onAppear { width = geo.size.width }.onChange(of: geo.size.width) { _, w in width = w }
        })
        .preferredColorScheme(.dark)
        .boardFont(13)
        .onAppear {
            position = notifyPosition()
            DispatchQueue.main.async { searching = true }
        }
        .onChange(of: model.focusSearch) { _, _ in
            position = notifyPosition()
            DispatchQueue.main.async { searching = true }
        }
    }

    // MARK: 왼쪽 — 돌아가기, 검색, 묶음

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            SettingsBackButton(showKey: sidebarWidth >= 200) { model.close() }
                .padding(.horizontal, 10).padding(.top, 10).padding(.bottom, 8)
            searchField.padding(.horizontal, 10).padding(.bottom, 12)
            ScrollView {
                VStack(spacing: 2) {
                    ForEach(SettingsSection.allCases) { s in
                        SettingsSectionButton(section: s, on: model.query.isEmpty && model.section == s,
                                              dim: !model.query.isEmpty && !items.contains { $0.section == s && $0.matches(trimmedQuery) }) {
                            model.go(s)
                        }
                    }
                }
                .padding(.horizontal, 10)
            }
            Spacer(minLength: 0)
            Text("Claude Board \(updater.current)")
                .boardFont(10.5).foregroundStyle(.tertiary)
                .padding(.horizontal, 20).padding(.vertical, 12)
        }
        .frame(width: sidebarWidth)
        .background(SettingsStyle.sidebar)
    }

    private var searchField: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
            TextField("설정 검색", text: $model.query)
                .textFieldStyle(.plain)
                .focused($searching)
            if !model.query.isEmpty {
                Button { model.query = "" } label: {
                    Image(systemName: "xmark.circle.fill").font(.system(size: 11)).foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .boardFont(12.5)
        .padding(.horizontal, 9).frame(height: 30)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.06)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(searching ? SettingsStyle.accent.opacity(0.7) : Color.white.opacity(0.08), lineWidth: 1))
        .animation(.easeOut(duration: 0.15), value: searching)
    }

    private var trimmedQuery: String { model.query.trimmingCharacters(in: .whitespaces) }

    // MARK: 오른쪽 — 고른 묶음, 또는 모든 묶음의 검색 결과

    private var content: some View {
        let q = trimmedQuery
        return ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if q.isEmpty {
                    header(model.section)
                    sectionBody(items.filter { $0.section == model.section })
                        .id(model.section)
                        .transition(.opacity)
                } else {
                    searchResults(q)
                }
            }
            .frame(maxWidth: 660, alignment: .leading)
            .padding(.horizontal, compact ? 18 : 32).padding(.top, 26).padding(.bottom, 40)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.automatic)
    }

    private func header(_ s: SettingsSection) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(s.title).boardFont(22, weight: .bold)
            Text(s.subtitle).boardFont(12).foregroundStyle(.secondary)
            Rectangle().fill(SettingsStyle.line).frame(height: 1).padding(.top, 14)
        }
        .padding(.bottom, 20)
    }

    // 카드 이름별로 묶어 차례대로
    private func sectionBody(_ list: [SettingsItem]) -> some View {
        let cards = list.map(\.card).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
        return VStack(alignment: .leading, spacing: 22) {
            ForEach(cards, id: \.self) { c in
                SettingsCard(title: c, items: list.filter { $0.card == c }, compact: compact)
            }
            if model.section == .shortcuts, model.query.isEmpty { shortcutFooter }
        }
    }

    private func searchResults(_ q: String) -> some View {
        let found = items.filter { $0.matches(q) }
        return VStack(alignment: .leading, spacing: 26) {
            Text("\"\(q)\" 검색 결과 \(found.count)개").boardFont(12).foregroundStyle(.secondary)
            if found.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "magnifyingglass").font(.system(size: 26, weight: .light)).foregroundStyle(.tertiary)
                    Text("맞는 설정이 없습니다").boardFont(13, weight: .semibold)
                    Text("다른 낱말로 찾아 보세요").boardFont(11.5).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity).padding(.top, 60)
            }
            ForEach(SettingsSection.allCases.filter { s in found.contains { $0.section == s } }) { s in
                VStack(alignment: .leading, spacing: 12) {
                    Button { model.go(s) } label: {
                        HStack(spacing: 7) {
                            Image(systemName: s.icon).font(.system(size: 12, weight: .semibold)).foregroundStyle(SettingsStyle.accent)
                            Text(s.title).boardFont(15, weight: .bold)
                            Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold)).foregroundStyle(.tertiary)
                        }
                    }
                    .buttonStyle(.plain)
                    sectionBody(found.filter { $0.section == s })
                }
            }
        }
    }

    // MARK: 항목

    private var items: [SettingsItem] {
        general + windowItems + notifyItems + terminalItems + boardItems + accountItems + shortcutItems + updateItems
    }

    private var general: [SettingsItem] {
        [
            SettingsItem(id: "pin", section: .general, card: "보드", title: "항상 위에 고정",
                         detail: "다른 앱을 눌러도 보드가 위에 남습니다. 화면 전체로 펼친 동안에는 쉽니다", keywords: "⌘P 플로팅") {
                AnyView(SettingsSwitch(isOn: Binding(get: { pinned }, set: { setPinned($0) })))
            },
            SettingsItem(id: "login", section: .general, card: "보드", title: "로그인할 때 열기",
                         detail: "Mac 에 로그인하면 보드를 띄웁니다. 바꾸면 다음 로그인부터 적용됩니다", keywords: "로그인 시 실행 시작 자동 실행") {
                AnyView(SettingsSwitch(isOn: Binding(get: { login.on }, set: { login.set($0) })))
            },
            SettingsItem(id: "mascot", section: .general, card: "캐릭터", title: "제목 옆 캐릭터",
                         detail: "세션 상태를 말풍선으로 알려 주는 캐릭터. 캐릭터를 오른쪽 클릭해도 고를 수 있습니다",
                         keywords: Mascot.allCases.map(\.name).joined(separator: " "), kind: .block) {
                AnyView(MascotGrid(selection: $mascot, columns: compact ? 2 : 4))
            },
        ]
    }

    private var windowItems: [SettingsItem] {
        [
            SettingsItem(id: "side", section: .window, card: "배치", title: "보드 위치",
                         detail: "펼치면 터미널 반대쪽, 접으면 화면 그쪽 끝에 붙습니다", keywords: "오른쪽 왼쪽") {
                AnyView(BoardSidePicker(left: Binding(get: { boardSide == "left" }, set: { setBoardSide(left: $0) })))
            },
            SettingsItem(id: "opacity", section: .window, card: "배치", title: "투명도",
                         detail: "마우스가 보드 창 밖에 있을 때의 투명도. 화면 전체로 펼친 창은 늘 불투명합니다", keywords: "투명 불투명") {
                AnyView(HStack(spacing: 10) {
                    Slider(value: $opacity.level, in: WindowOpacity.range) { editing in
                        settle?.cancel()
                        if editing { opacity.previewing = true; return }
                        let w = DispatchWorkItem { opacity.previewing = false }
                        settle = w
                        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: w)
                    }
                    .controlSize(.small)
                    .tint(SettingsStyle.accent)
                    .frame(width: 150)
                    Text("\(Int((opacity.level * 100).rounded()))%").boardFont(12, weight: .semibold).monospacedDigit()
                        .frame(width: 40, alignment: .trailing)
                })
            },
            SettingsItem(id: "animation", section: .window, card: "움직임", title: "펼치기·접기 애니메이션",
                         detail: "터미널을 펼치고 접을 때 창이 미끄러지듯 움직입니다") {
                AnyView(SettingsSwitch(isOn: $animation))
            },
            SettingsItem(id: "transition", section: .window, card: "움직임", title: "설정 화면 전환",
                         detail: "터미널 영역이 설정 화면으로 바뀔 때의 효과", keywords: "밀기 서서히 효과 애니메이션") {
                AnyView(SettingsSegments(options: SettingsTransition.allCases.map { ($0, $0.name) }, selection: $transition))
            },
        ]
    }

    private var notifyItems: [SettingsItem] {
        guard let app else { return [] }
        return [
            SettingsItem(id: "allMute", section: .notify, card: "알림", title: "모든 알림 끄기",
                         detail: "켜면 모든 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다. 끄면 세션마다 정해 둔 대로 돌아갑니다",
                         keywords: "음소거 종") {
                AnyView(AllMuteSwitch(store: app.store))
            },
            SettingsItem(id: "position", section: .notify, card: "알림", title: "알림 위치",
                         detail: "패널이 뜨는 화면 자리. \(notifyPositions.first { $0.0 == position }?.1 ?? "가운데")",
                         keywords: notifyPositions.map(\.1).joined(separator: " ")) {
                AnyView(NotifyPositionPicker(selection: Binding(get: { position }, set: { position = $0; setNotifyPosition($0) })))
            },
        ]
    }

    private var terminalItems: [SettingsItem] {
        let size = terminalFont > 0 ? terminalFont : ghosttyConfigFontSize()
        return [
            SettingsItem(id: "terminalFont", section: .terminal, card: "글자", title: "터미널 글자 크기",
                         detail: terminalFont > 0 ? "모든 탭에 바로 적용됩니다" : "Ghostty 설정 크기를 따르는 중",
                         keywords: "폰트 pt") {
                AnyView(SettingsStepper(value: "\(String(format: "%g", size))pt", changed: terminalFont > 0,
                                        minus: { hub.changeTerminalFont(-1) }, plus: { hub.changeTerminalFont(1) },
                                        reset: { hub.changeTerminalFont(nil) }))
            },
            SettingsItem(id: "theme", section: .terminal, card: "테마", title: "터미널 테마",
                         detail: "누르면 열린 탭 모두 바로 바뀝니다", keywords: "색 Ghostty 컬러", kind: .block) {
                AnyView(ThemeListView(model: themes))
            },
            SettingsItem(id: "sessionCommand", section: .terminal, card: "새 세션", title: "기본 명령",
                         detail: "새 세션 창과 카드의 + 가 쓰는 명령. 비우면 \(builtinSessionCommand)", keywords: "claude 실행 명령어",
                         kind: .block) {
                AnyView(SettingsTextField(text: $sessionCommand, placeholder: builtinSessionCommand))
            },
            SettingsItem(id: "sessionMd", section: .terminal, card: "새 세션", title: "답변 마크다운 꾸미기",
                         detail: "새 세션을 열 때 기본으로 켭니다(md-render). 새 세션 창에서 세션마다 바꿀 수 있습니다", keywords: "md-render") {
                AnyView(SettingsSwitch(isOn: $sessionMdRender).disabled(mdRenderDir == nil))
            },
        ]
    }

    private var boardItems: [SettingsItem] {
        [
            SettingsItem(id: "boardFont", section: .board, card: "보드", title: "보드 글자 크기",
                         detail: "80%부터 150%까지 10%씩", keywords: "폰트 확대 축소") {
                AnyView(SettingsStepper(value: "\(Int((fontScale.scale * 100).rounded()))%", changed: fontScale.scale != 1,
                                        minus: { fontScale.change(-1) }, plus: { fontScale.change(1) }, reset: { fontScale.change(nil) }))
            },
            SettingsItem(id: "usageCompact", section: .board, card: "보드", title: "사용 한도 간략히 보기",
                         detail: "5시간·주간 한도 칸을 한 줄로 줄입니다", keywords: "사용량 한도") {
                AnyView(SettingsSwitch(isOn: $usageCompact))
            },
        ]
    }

    private var accountItems: [SettingsItem] {
        guard let app else { return [] }
        return [
            SettingsItem(id: "accounts", section: .account, card: "Claude 계정", title: "저장한 계정",
                         detail: "고르면 모든 세션이 다음 요청부터 그 계정으로 이어 갑니다", keywords: "로그인 전환 추가 지우기 이메일",
                         kind: .block) {
                AnyView(AccountList(accounts: app.accounts, busy: {
                    app.store.groups.flatMap(\.sessions).filter { $0.state == "working" || $0.state == "waiting" }
                }))
            },
            SettingsItem(id: "autoSwitch", section: .account, card: "자동 전환", title: "한도가 차면 다른 계정으로 자동 전환",
                         detail: "5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 멈춘 세션을 이어서 진행합니다") {
                AnyView(AccountAutoSwitch(accounts: app.accounts))
            },
        ]
    }

    private var shortcutItems: [SettingsItem] {
        Shortcuts.actions.map { a in
            SettingsItem(id: "key." + a.id, section: .shortcuts, card: a.group, title: a.title,
                         keywords: (keys.label(a.id) ?? "") + " 단축키 키") {
                AnyView(ShortcutRow(action: a, keys: keys, recorder: recorder))
            }
        }
    }

    // 모두 기본값으로 · 받지 않은 키 안내
    private var shortcutFooter: some View {
        HStack(spacing: 10) {
            SettingsLinkButton("모두 기본값으로", enabled: !keys.overrides.isEmpty) { keys.resetAll() }
            if let r = recorder.refused {
                Label(r, systemImage: "exclamationmark.circle.fill").boardFont(11.5).foregroundStyle(.orange)
            }
            Spacer()
            Text("Esc 취소 · ⌫ 비우기").boardFont(11).foregroundStyle(.tertiary)
        }
    }

    private var updateItems: [SettingsItem] {
        [
            SettingsItem(id: "version", section: .update, card: "버전", title: "Claude Board \(updater.current)",
                         detail: updater.latest.map { "새 버전 \($0) 이 나왔습니다" } ?? updater.notice?.text ?? "업데이트 확인을 누르면 지금 바로 확인합니다",
                         keywords: "업데이트 확인 새 버전 설치") {
                AnyView(HStack(spacing: 8) {
                    SettingsLinkButton("바뀐 점") { NSWorkspace.shared.open(changelogURL) }
                    if updater.latest != nil {
                        SettingsButton(updater.running ? "설치하는 중…" : "업데이트", prominent: true, enabled: !updater.running) { updater.run() }
                    } else {
                        SettingsButton(updater.checking ? "확인하는 중…" : "업데이트 확인", enabled: !updater.checking) { updater.check(manual: true) }
                    }
                })
            },
            SettingsItem(id: "autoUpdate", section: .update, card: "버전", title: "자동으로 업데이트 확인",
                         detail: "1시간마다 새 버전이 있는지 보고 보드 위에 알립니다. 설치는 직접 누를 때만 합니다", keywords: "자동 업데이트") {
                AnyView(SettingsSwitch(isOn: Binding(get: { autoUpdate }, set: { autoUpdate = $0; if $0 { updater.check() } })))
            },
        ]
    }
}

// 카드 — 위에 작은 제목, 안에 줄들(줄 사이 가는 선)
struct SettingsCard: View {
    let title: String
    let items: [SettingsItem]
    var compact = false   // 좁으면 줄의 조절기를 설명 아래로

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).boardFont(11.5, weight: .semibold).foregroundStyle(.secondary).padding(.leading, 4)
            VStack(spacing: 0) {
                ForEach(Array(items.enumerated()), id: \.element.id) { i, item in
                    if i > 0 { Rectangle().fill(SettingsStyle.line).frame(height: 1).padding(.leading, 16) }
                    row(item)
                }
            }
            .background(RoundedRectangle(cornerRadius: 12).fill(SettingsStyle.card))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(SettingsStyle.cardLine, lineWidth: 1))
        }
    }

    @ViewBuilder private func row(_ item: SettingsItem) -> some View {
        switch item.kind {
        case .row where compact:
            VStack(alignment: .leading, spacing: 10) {
                label(item)
                item.content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
        case .row:
            HStack(alignment: .center, spacing: 16) {
                label(item)
                Spacer(minLength: 8)
                item.content()
            }
            .padding(.horizontal, 16).padding(.vertical, item.detail == nil ? 10 : 13)
        case .block:
            VStack(alignment: .leading, spacing: 12) {
                label(item)
                item.content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
        }
    }

    private func label(_ item: SettingsItem) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(item.title).boardFont(13, weight: .medium)
            if let d = item.detail {
                Text(d).boardFont(11.5).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

// MARK: 조절기

struct SettingsBackButton: View {
    var showKey = true   // 좁은 목록에서는 esc 표시를 뺀다
    let action: () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: "chevron.left").font(.system(size: 10, weight: .bold))
                Text("터미널로 돌아가기").boardFont(12, weight: .medium)
                    .lineLimit(1)
                Spacer(minLength: 0)
                if showKey {
                    Text("esc").boardFont(10, weight: .semibold).foregroundStyle(.tertiary)
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background(RoundedRectangle(cornerRadius: 4).stroke(Color.white.opacity(0.14), lineWidth: 1))
                }
            }
            .foregroundStyle(hover ? .primary : .secondary)
            .padding(.horizontal, 8).frame(height: 28)
            .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(hover ? 0.07 : 0)))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
    }
}

struct SettingsSectionButton: View {
    let section: SettingsSection
    let on: Bool
    let dim: Bool
    let action: () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                Image(systemName: section.icon)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(on ? SettingsStyle.accent : .secondary)
                    .frame(width: 18)
                Text(section.title).boardFont(12.5, weight: on ? .semibold : .regular)
                Spacer(minLength: 0)
            }
            .foregroundStyle(on ? .primary : .secondary)
            .padding(.horizontal, 9).frame(height: 30)
            .background(RoundedRectangle(cornerRadius: 7).fill(on ? SettingsStyle.accent.opacity(0.16) : Color.white.opacity(hover ? 0.04 : 0)))
            .overlay(RoundedRectangle(cornerRadius: 7).stroke(on ? SettingsStyle.accent.opacity(0.25) : .clear, lineWidth: 1))
            .contentShape(Rectangle())
            .opacity(dim ? 0.4 : 1)
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .animation(.easeOut(duration: 0.12), value: on)
    }
}

// 스위치 — BoardSwitch 처럼 직접 그린다(macOS 기본 스위치는 창이 비활성이면 회색이 된다)
struct SettingsSwitch: View {
    @Binding var isOn: Bool
    var tint = SettingsStyle.accent
    @Environment(\.isEnabled) private var enabled
    @State private var hover = false
    var body: some View {
        Button { withAnimation(.spring(response: 0.25, dampingFraction: 0.75)) { isOn.toggle() } } label: {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule().fill(isOn ? tint : Color.white.opacity(hover ? 0.2 : 0.13))
                    .overlay(Capsule().stroke(Color.white.opacity(isOn ? 0 : 0.14), lineWidth: 0.5))
                Circle().fill(Color.white).shadow(color: .black.opacity(0.3), radius: 1.5, y: 0.5).padding(2.5)
            }
            .frame(width: 38, height: 22)
            .opacity(enabled ? 1 : 0.4)
        }
        .buttonStyle(.plain)
        .clickable($hover)
    }
}

struct SettingsButton: View {
    let title: String
    var prominent = false
    var enabled = true
    let action: () -> Void
    @State private var hover = false
    init(_ title: String, prominent: Bool = false, enabled: Bool = true, action: @escaping () -> Void) {
        self.title = title
        self.prominent = prominent
        self.enabled = enabled
        self.action = action
    }
    var body: some View {
        Button(action: action) {
            Text(title).boardFont(12, weight: .semibold)
                .foregroundStyle(prominent ? Color.white : .primary)
                .padding(.horizontal, 12).frame(height: 26)
                .background(RoundedRectangle(cornerRadius: 7).fill(prominent ? SettingsStyle.accent.opacity(hover ? 1 : 0.85)
                                                                    : Color.white.opacity(hover ? 0.14 : 0.08)))
                .overlay(RoundedRectangle(cornerRadius: 7).stroke(Color.white.opacity(prominent ? 0 : 0.1), lineWidth: 1))
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.5)
        .clickable($hover)
    }
}

struct SettingsLinkButton: View {
    let title: String
    var enabled = true
    let action: () -> Void
    @State private var hover = false
    init(_ title: String, enabled: Bool = true, action: @escaping () -> Void) {
        self.title = title
        self.enabled = enabled
        self.action = action
    }
    var body: some View {
        Button(action: action) {
            Text(title).underline(hover).boardFont(12, weight: .medium)
                .foregroundStyle(SettingsStyle.accent.opacity(hover ? 1 : 0.85))
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
        .clickable($hover)
    }
}

// 고르기 칸 — 둥근 바탕 위에 고른 것만 밝은 알약이 미끄러져 간다
struct SettingsSegments<T: Hashable>: View {
    let options: [(T, String)]
    @Binding var selection: T
    @Namespace private var ns
    var body: some View {
        HStack(spacing: 2) {
            ForEach(options, id: \.0) { value, name in
                let on = value == selection
                Button { withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { selection = value } } label: {
                    Text(name).boardFont(11.5, weight: on ? .semibold : .regular)
                        .foregroundStyle(on ? .primary : .secondary)
                        .padding(.horizontal, 10).frame(height: 24)
                        .background {
                            if on { RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.14)).matchedGeometryEffect(id: "on", in: ns) }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(2)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.06)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.white.opacity(0.08), lineWidth: 1))
    }
}

// − 값 + (바꿨으면 되돌리기)
struct SettingsStepper: View {
    let value: String
    let changed: Bool
    let minus: () -> Void
    let plus: () -> Void
    let reset: () -> Void
    var body: some View {
        HStack(spacing: 10) {
            if changed {
                Button(action: reset) {
                    Image(systemName: "arrow.uturn.backward").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .transition(.opacity)
            }
            HStack(spacing: 0) {
                step("minus", minus)
                Text(value).boardFont(12, weight: .semibold).monospacedDigit().frame(minWidth: 52)
                    .contentTransition(.numericText())
                step("plus", plus)
            }
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.06)))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.white.opacity(0.08), lineWidth: 1))
        }
        .animation(.easeOut(duration: 0.15), value: value)
    }

    private func step(_ icon: String, _ run: @escaping () -> Void) -> some View {
        StepButton(icon: icon, run: run)
    }

    private struct StepButton: View {
        let icon: String
        let run: () -> Void
        @State private var hover = false
        var body: some View {
            Button(action: run) {
                Image(systemName: icon).font(.system(size: 10, weight: .bold))
                    .frame(width: 28, height: 26)
                    .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(hover ? 0.08 : 0)))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .clickable($hover)
        }
    }
}

struct SettingsTextField: View {
    @Binding var text: String
    let placeholder: String
    @FocusState private var focused: Bool
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "chevron.right").font(.system(size: 10, weight: .bold)).foregroundStyle(SettingsStyle.accent)
            TextField(placeholder, text: $text)
                .textFieldStyle(.plain)
                .boardFont(12.5, design: .monospaced)
                .focused($focused)
            if !text.isEmpty {
                SettingsLinkButton("기본값으로") { text = "" }
            }
        }
        .padding(.horizontal, 10).frame(height: 32)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(focused ? SettingsStyle.accent.opacity(0.7) : Color.white.opacity(0.1), lineWidth: 1))
    }
}

// 보드 위치 — 창 모양 두 개(터미널 │ 보드)
struct BoardSidePicker: View {
    @Binding var left: Bool
    var body: some View {
        HStack(spacing: 8) {
            tile(false, "오른쪽")
            tile(true, "왼쪽")
        }
    }

    private func tile(_ isLeft: Bool, _ name: String) -> some View {
        let on = left == isLeft
        return Button { withAnimation(.easeOut(duration: 0.15)) { left = isLeft } } label: {
            VStack(spacing: 5) {
                HStack(spacing: 2) {
                    if isLeft { pane(board: true, on: on) }
                    pane(board: false, on: on)
                    if !isLeft { pane(board: true, on: on) }
                }
                .padding(3)
                .frame(width: 66, height: 40)
                .background(RoundedRectangle(cornerRadius: 6).fill(Color.black.opacity(0.3)))
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(on ? SettingsStyle.accent : Color.white.opacity(0.12), lineWidth: on ? 1.5 : 1))
                Text(name).boardFont(10.5, weight: on ? .semibold : .regular).foregroundStyle(on ? .primary : .secondary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func pane(board: Bool, on: Bool) -> some View {
        RoundedRectangle(cornerRadius: 2.5)
            .fill(board ? (on ? SettingsStyle.accent.opacity(0.75) : Color.white.opacity(0.28)) : Color.white.opacity(0.08))
            .frame(width: board ? 18 : nil)
    }
}

// 알림 위치 — 화면 모양 위의 다섯 자리
struct NotifyPositionPicker: View {
    @Binding var selection: String
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 7).fill(Color.black.opacity(0.3))
                .overlay(RoundedRectangle(cornerRadius: 7).stroke(Color.white.opacity(0.12), lineWidth: 1))
            VStack(spacing: 0) {
                Rectangle().fill(Color.white.opacity(0.08)).frame(height: 5)   // 메뉴 막대
                Spacer()
            }
            .clipShape(RoundedRectangle(cornerRadius: 7))
            ForEach(notifyPositions, id: \.0) { value, name in spot(value, name) }
        }
        .frame(width: 132, height: 82)
    }

    private func spot(_ value: String, _ name: String) -> some View {
        let on = selection == value
        let x: CGFloat = value.hasSuffix("left") ? -44 : value.hasSuffix("right") ? 44 : 0
        let y: CGFloat = value.hasPrefix("top") ? -21 : value.hasPrefix("bottom") ? 27 : 3
        return SpotButton(on: on) { withAnimation(.spring(response: 0.3, dampingFraction: 0.75)) { selection = value } }
            .offset(x: x, y: y)
    }

    private struct SpotButton: View {
        let on: Bool
        let action: () -> Void
        @State private var hover = false
        var body: some View {
            Button(action: action) {
                RoundedRectangle(cornerRadius: 3)
                    .fill(on ? SettingsStyle.accent : Color.white.opacity(hover ? 0.32 : 0.16))
                    .frame(width: 30, height: 14)
                    .shadow(color: on ? SettingsStyle.accent.opacity(0.6) : .clear, radius: 4)
                    .padding(4)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .clickable($hover)
        }
    }
}

// 캐릭터 고르기 — 8명이 제자리에서 움직인다. 고른 캐릭터는 강조 테두리
struct MascotGrid: View {
    @Binding var selection: Mascot
    var columns = 4   // 8명이 고르게 나뉘게 4열, 좁으면 2열
    var body: some View {
        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: columns), spacing: 8) {
            ForEach(Mascot.allCases) { m in MascotTile(mascot: m, on: m == selection) { selection = m } }
        }
    }

    private struct MascotTile: View {
        let mascot: Mascot
        let on: Bool
        let action: () -> Void
        @State private var hover = false
        var body: some View {
            Button(action: action) {
                VStack(spacing: 6) {
                    Ambient { t in
                        ZStack {
                            if mascot == .foreman {
                                Foreman.figure(mood: on ? .busy : .idle, t: t, sinceChange: 9, blink: false)
                            } else {
                                BrainrotFigure(kind: mascot, mood: on ? .busy : .idle, t: t, sinceChange: 9, blink: false)
                            }
                        }
                        .frame(width: 46, height: 44)
                    }
                    Text(mascot.name).boardFont(10.5, weight: on ? .semibold : .regular)
                        .foregroundStyle(on ? .primary : .secondary)
                        .multilineTextAlignment(.center).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                        .frame(height: 28, alignment: .top)
                }
                .frame(maxWidth: .infinity).padding(.top, 10).padding(.bottom, 6).padding(.horizontal, 4)
                .background(RoundedRectangle(cornerRadius: 10).fill(on ? SettingsStyle.accent.opacity(0.14) : Color.white.opacity(hover ? 0.06 : 0.025)))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(on ? SettingsStyle.accent.opacity(0.8) : Color.white.opacity(0.07), lineWidth: on ? 1.5 : 1))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .clickable($hover)
            .animation(.easeOut(duration: 0.15), value: on)
        }
    }
}

// 계정 목록 — 지금 계정 표시, 다른 계정은 전환·지우기, 아래에 계정 추가
struct AccountList: View {
    @ObservedObject var accounts: AccountStore
    let busy: () -> [Session]

    var body: some View {
        var list = accounts.saved
        if let c = accounts.current, !list.contains(where: { $0.id == c.id }) { list.insert(c, at: 0) }
        return VStack(spacing: 6) {
            ForEach(list, id: \.id) { a in AccountRow(account: a, accounts: accounts, busy: busy) }
            if list.isEmpty {
                Text("로그인 정보가 없습니다").boardFont(12).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
            }
            if accounts.adding {
                AccountLoginPanel(accounts: accounts)
                    .padding(.top, 4)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            } else {
                HStack {
                    SettingsButton("계정 추가…") { accounts.beginAdd() }
                    Text("브라우저에서 추가할 계정으로 로그인합니다").boardFont(11).foregroundStyle(.tertiary)
                    Spacer()
                }
                .padding(.top, 4)
            }
            if let n = accounts.notice {
                Label(n.text, systemImage: n.error ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .boardFont(11.5).foregroundStyle(n.error ? Color.orange : .secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    // 계정 추가 중 — 1) 브라우저에서 로그인(열리지 않았으면 열기·링크 복사) 2) 브라우저가 코드를 보여 주면 붙여넣기
    private struct AccountLoginPanel: View {
        @ObservedObject var accounts: AccountStore
        @State private var code = ""
        @State private var copied = false
        @FocusState private var focused: Bool

        var body: some View {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("새 계정 로그인").boardFont(12.5, weight: .semibold)
                    Spacer()
                    SettingsButton("취소") { accounts.cancelAdd() }
                }
                step(1, "열린 로그인 창에서 추가할 계정으로 로그인하세요", detail: accounts.loginURL == nil ? "로그인 주소를 받는 중…"
                     : "로그인을 마치면 저절로 추가되고 창이 닫힙니다. 창을 닫으면 계정 추가를 취소합니다. Google 로그인이 막히면 다른 브라우저에서 열고 코드를 받아 아래에 넣습니다") {
                    if let url = accounts.loginURL {
                        HStack(spacing: 8) {
                            SettingsButton("다른 브라우저에서 열기") { NSWorkspace.shared.open(url) }
                            SettingsLinkButton(copied ? "복사했습니다" : "링크 복사") {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(url.absoluteString, forType: .string)
                                copied = true
                            }
                        }
                    }
                }
                step(2, "브라우저가 코드를 보여 주면 여기에 붙여넣으세요", detail: "로그인 뒤 \"코드를 복사해 Claude Code 에 붙여넣으라\"는 화면이 나올 때만 필요합니다") {
                    HStack(spacing: 8) {
                        TextField("인증 코드", text: $code)
                            .textFieldStyle(.plain)
                            .boardFont(12.5, design: .monospaced)
                            .focused($focused)
                            .onSubmit(send)
                            .padding(.horizontal, 10).frame(height: 30)
                            .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.25)))
                            .overlay(RoundedRectangle(cornerRadius: 8).stroke(focused ? SettingsStyle.accent.opacity(0.7) : Color.white.opacity(0.1), lineWidth: 1))
                        SettingsButton("확인", prominent: true, enabled: !code.trimmingCharacters(in: .whitespaces).isEmpty, action: send)
                    }
                }
                if let n = accounts.loginNote {
                    Label(n, systemImage: "hourglass").boardFont(11.5).foregroundStyle(.secondary)
                }
            }
            .padding(14)
            .background(RoundedRectangle(cornerRadius: 10).fill(SettingsStyle.accent.opacity(0.07)))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(SettingsStyle.accent.opacity(0.25), lineWidth: 1))
        }

        private func send() {
            accounts.submitLoginCode(code)
            code = ""
        }

        private func step<C: View>(_ n: Int, _ title: String, detail: String, @ViewBuilder _ content: () -> C) -> some View {
            HStack(alignment: .top, spacing: 10) {
                Text("\(n)").boardFont(11, weight: .bold).foregroundStyle(SettingsStyle.accent)
                    .frame(width: 20, height: 20)
                    .background(Circle().fill(SettingsStyle.accent.opacity(0.16)))
                VStack(alignment: .leading, spacing: 6) {
                    Text(title).boardFont(12.5, weight: .medium)
                    Text(detail).boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    content()
                }
            }
        }
    }

    private struct AccountRow: View {
        let account: Account
        @ObservedObject var accounts: AccountStore
        let busy: () -> [Session]
        @State private var hover = false

        var body: some View {
            let current = account.id == accounts.current?.id
            HStack(spacing: 10) {
                Image(systemName: "person.crop.circle.fill").font(.system(size: 20))
                    .foregroundStyle(current ? Palette.accent : Color.white.opacity(0.3))
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(account.email).boardFont(12.5, weight: .semibold).lineLimit(1).truncationMode(.middle)
                        if !account.plan.isEmpty {
                            Text(account.plan).boardFont(9.5, weight: .bold).foregroundStyle(.secondary)
                                .padding(.horizontal, 5).padding(.vertical, 1)
                                .background(Capsule().fill(Color.white.opacity(0.08))).fixedSize()
                        }
                    }
                    if !account.org.isEmpty { Text(account.org).boardFont(11).foregroundStyle(.secondary).lineLimit(1) }
                }
                Spacer(minLength: 8)
                if current {
                    Text("사용 중").boardFont(11, weight: .semibold).foregroundStyle(Palette.color("idle"))
                        .padding(.horizontal, 8).frame(height: 22)
                        .background(Capsule().fill(Palette.color("idle").opacity(0.12)))
                } else {
                    if hover {
                        Button { accounts.confirmRemove(account) } label: {
                            Image(systemName: "trash").font(.system(size: 11)).foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                        .transition(.opacity)
                    }
                    if account.isExpired(at: Date()) {
                        SettingsButton("다시 로그인", enabled: !accounts.switching && !accounts.adding) { accounts.beginAdd(email: account.email) }
                    } else {
                        SettingsButton("전환", enabled: !accounts.switching) { accounts.confirmSwitch(account, busy: busy()) }
                    }
                }
            }
            .padding(.horizontal, 10).padding(.vertical, 8)
            .background(RoundedRectangle(cornerRadius: 9).fill(Color.white.opacity(current ? 0.05 : hover ? 0.04 : 0)))
            .onHover { h in withAnimation(.easeOut(duration: 0.12)) { hover = h } }
        }
    }
}

// 매초 바뀌는 Store 는 이 스위치만 지켜본다 — 설정 화면 전체가 매초 다시 그려지지 않게
struct AllMuteSwitch: View {
    @ObservedObject var store: Store
    var body: some View {
        SettingsSwitch(isOn: Binding(get: { store.allMuted }, set: { store.setAllMuted($0) }), tint: Color(red: 1, green: 0.45, blue: 0.45))
    }
}

struct AccountAutoSwitch: View {
    @ObservedObject var accounts: AccountStore
    var body: some View { SettingsSwitch(isOn: $accounts.autoSwitch) }
}

// 단축키 한 줄 — (바꿨으면 되돌리기) · 키 칸. 아래에 겹침·시스템 단축키·전역 등록 실패 안내
struct ShortcutRow: View {
    let action: ShortcutAction
    @ObservedObject var keys: Shortcuts
    @ObservedObject var recorder: ShortcutSettings
    @State private var hover = false

    var body: some View {
        let a = action
        let listening = recorder.recording == a.id
        let changed = keys.overrides[a.id] != nil
        let accent = SettingsStyle.accent
        VStack(alignment: .trailing, spacing: 4) {
            HStack(spacing: 8) {
                if changed {
                    Button { keys.set(a.id, a.initial) } label: {
                        Image(systemName: "arrow.uturn.backward").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .hint("기본값(\(a.initial?.label ?? "없음"))으로 되돌리기")
                }
                Button { listening ? recorder.stopRecording() : recorder.startRecording(a.id) } label: {
                    Text(listening ? "새 키를 누르세요…" : keys.label(a.id) ?? "없음")
                        .font(.system(size: 12, weight: .medium, design: .monospaced))
                        .foregroundStyle(listening ? accent : changed ? Color.yellow : keys.combo(a.id) == nil ? .secondary : .primary)
                        .padding(.horizontal, 10)
                        .frame(minWidth: 84, minHeight: 26)
                        .background(RoundedRectangle(cornerRadius: 7).fill(listening ? accent.opacity(0.14) : Color.white.opacity(hover ? 0.1 : 0.06)))
                        .overlay(RoundedRectangle(cornerRadius: 7).stroke(listening ? accent : Color.white.opacity(0.12), lineWidth: 1))
                }
                .buttonStyle(.plain)
                .clickable($hover)
            }
            ForEach(keys.conflicts(a.id), id: \.self) { other in
                HStack(spacing: 6) {
                    Text("\"\(keys.action(other)?.title ?? other)\" 항목과 같은 키입니다").foregroundStyle(.orange)
                    SettingsLinkButton("그쪽 비우기") { keys.set(other, nil) }
                }
                .boardFont(11)
            }
            if let c = keys.combo(a.id), recorder.systemTaken(c) {
                Text("macOS 시스템 단축키와 겹칩니다").boardFont(11).foregroundStyle(.orange)
            }
            if keys.hotKeyFailed.contains(a.id) {
                Text("전역 단축키로 등록하지 못했습니다").boardFont(11).foregroundStyle(.orange)
            }
        }
        // 받는 중인 줄이 화면에서 사라지면(검색어로 걸러짐 등) 받기를 그만둔다 — 보이지 않는 줄에 키가 들어가지 않게
        .onDisappear { if recorder.recording == action.id { recorder.stopRecording() } }
    }
}

func addSnapButton() {
    let vc = NSTitlebarAccessoryViewController()
    let host = NSHostingView(rootView: SnapButton())
    host.frame = NSRect(x: 0, y: 0, width: 38, height: 28)
    vc.view = host
    vc.layoutAttribute = .leading
    window.addTitlebarAccessoryViewController(vc)
}

// MARK: 다른 터미널·세션 조작의 진짜 어댑터 — 규칙(터미널 가르기·조작 길 고르기·AppleScript 글자·Ghostty 찾기·세션 조작 순서·끝나기 기다리기 판단)은
// BoardRules/Terminals.swift·SessionControl.swift, 포트는 BoardRules/Ports.swift(가짜 어댑터는 Tests 에만).
// 여기는 어댑터와, 확인 창·애니메이션 기다리기·큐(ghosttyQueue·백그라운드·메인)·타이머를 쥔 앱 함수다

// Ghostty 조작(표지 제목으로 터미널 찾기·키 보내기·포커스)은 이 큐에서 차례로 — 표지 제목을 쓰고 기다리는 동안 메인을 막지 않는다
let ghosttyQueue = DispatchQueue(label: "com.claude.board.ghostty")

// 다른 터미널 앱 — NSRunningApplication·NSWorkspace·NSAppleScript. 상태 없는 struct 라 어느 큐에서나 부른다(activate·openGhostty 는 메인에서)
struct SystemTerminalApps: TerminalApps {
    func isRunning(_ bundleId: String) -> Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).isEmpty
    }

    func activate(_ bundleId: String) {
        if let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).first {
            app.activate()
        } else if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleId) {
            NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
        }
    }

    func openGhostty() {
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: TerminalBundle.ghostty) {
            NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
        }
    }

    func run(_ script: String) -> String? {
        var err: NSDictionary?
        let r = NSAppleScript(source: script)?.executeAndReturnError(&err)
        // 권한 거부 같은 AppleScript 오류는 board.log 에 남긴다 — 보드 화면에는 드러나지 않기 때문
        if let err {
            let line = "\(Date()) \(err[NSAppleScript.errorNumber] ?? "") \(err[NSAppleScript.errorMessage] ?? "")\n"
            let log = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/board.log")
            if let h = FileHandle(forWritingAtPath: log) { h.seekToEndOfFile(); h.write(Data(line.utf8)); try? h.close() }
            else { FileManager.default.createFile(atPath: log, contents: Data(line.utf8)) }
        }
        return r?.stringValue
    }

    func wait(_ seconds: TimeInterval) { Thread.sleep(forTimeInterval: seconds) }
}

// 프로세스 — 조상 앱 찾기는 아래 ancestorApp(TerminalHub.pair 도 쓴다), 부모 pid 는 sysctl(보드 탭 짝짓기의 headless 조상 찾기),
// 살아 있는지·끝내기는 kill
struct SystemProcessControl: ProcessControl {
    func ancestorBundleID(_ pid: Int32) -> String? { ancestorApp(pid) }

    func parentPID(_ pid: Int32) -> Int32? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        return info.kp_eproc.e_ppid
    }
    func isAlive(_ pid: Int32) -> Bool { kill(pid, 0) == 0 }
    func terminate(_ pid: Int32) { kill(pid, SIGTERM) }
}

// 세션 tty 의 창 제목(OSC 2) — tty 는 세션 파일에서 온 바깥 값이라 tty 이름 꼴(isTtyName, ASCII 영숫자)일 때만 /dev/<tty> 를 연다
// ("../" 같은 값으로 /dev 밖의 파일에 쓰지 않게). 아니면 지금처럼 조용히 넘어간다
struct TtyTitles: TerminalTitles {
    func write(tty: String, title: String) {
        guard isTtyName(tty), let h = FileHandle(forWritingAtPath: "/dev/\(tty)") else { return }
        h.write("\u{1b}]2;\(title)\u{07}".data(using: .utf8)!)
        try? h.close()
    }
}

// 보드 탭 — TerminalHub(@MainActor). 조작 길 고르기(driveRoute)와 세션 조작이 메인에서만 부른다
struct HubTabs: BoardTabs {
    func place(_ s: Session) -> SessionPlace { MainActor.assumeIsolated { TerminalHub.shared.place(s) } }
    func parkedTab(_ s: Session) -> UUID? { MainActor.assumeIsolated { TerminalHub.shared.parkedTab(s) } }

    func isForeground(_ screen: UUID, pid: Int32) -> Bool {
        MainActor.assumeIsolated { TerminalHub.shared.surface(screen)?.isForeground(pid) == true }
    }

    func drive(_ screen: UUID, _ steps: TerminalSteps, done: @escaping (Bool) -> Void) {
        MainActor.assumeIsolated {
            guard let surface = TerminalHub.shared.surface(screen) else { return done(false) }
            surface.drive(steps) { done($0) }
        }
    }

    func reveal(_ id: UUID) { MainActor.assumeIsolated { TerminalHub.shared.reveal(id) } }

    func promptScreen(session: String, dir: String) -> UUID? {
        MainActor.assumeIsolated { TerminalHub.shared.surfaces.first { $0.sessionId == session && $0.atPrompt && $0.cwd == dir }?.id }
    }

    func run(_ command: String, in screen: UUID, session: String, mdRenderOff: Bool) {
        MainActor.assumeIsolated {
            let hub = TerminalHub.shared
            if let surface = hub.surface(screen) { hub.run(command, in: surface, sessionId: session, mdRenderOff: mdRenderOff) }
        }
    }

    func launch(_ command: String, in dir: String, session: String, mdRenderOff: Bool) {
        MainActor.assumeIsolated { TerminalHub.shared.launch(command, in: dir, sessionId: session, mdRenderOff: mdRenderOff) }
    }

    // 답변 꾸미기는 기본으로 켠다(보드 밖에서 시작한 세션도) — 새 세션 창에서 끄고 띄운 세션만 끈 채로
    func claudeCommand(_ base: String, session: String) -> (command: String, mdRenderOff: Bool) {
        let off = isMdRenderOff(session)
        return (mdRenderCommand(base, args: sessionRecord(session)?["args"] as? [String], off: off), off)
    }

    func mode(_ s: Session) -> String? { MainActor.assumeIsolated { TerminalHub.shared.mode(s) } }
    func toast(_ text: String) { MainActor.assumeIsolated { TerminalHub.shared.toast(text) } }
}

// 보드 탭 화면 — TerminalHub(@MainActor). 짝짓기·자리 판정(pairTabs·sessionPlace)이 메인에서만 부른다
struct HubProbe: TabProbe {
    func runsClaude(_ screen: UUID) -> Bool { MainActor.assumeIsolated { TerminalHub.shared.surface(screen)?.runsClaude == true } }
    func mode(_ s: Session) -> String? { MainActor.assumeIsolated { TerminalHub.shared.mode(s) } }
}

// 세션 표지 — sessions/<sid>.paused·<sid>.mute. 알림 끄기 표지는 세션 기록이 지워질 때 state.py prune 이 함께 치운다
struct SessionMarkFiles: SessionMarks {
    private func path(_ sid: String, _ ext: String) -> String { (sessionsDir as NSString).appendingPathComponent("\(sid).\(ext)") }
    func writePaused(_ sid: String, _ data: Data) { FileManager.default.createFile(atPath: path(sid, "paused"), contents: data) }
    func clearPaused(_ sid: String) { try? FileManager.default.removeItem(atPath: path(sid, "paused")) }
    func setMuted(_ sid: String, _ on: Bool) {
        if on {
            FileManager.default.createFile(atPath: path(sid, "mute"), contents: Data())
        } else {
            try? FileManager.default.removeItem(atPath: path(sid, "mute"))
        }
    }
}

// 세션 터미널 — 아래 앱 함수(큐·타이머를 쥔다). 세션 조작이 메인에서만 부른다
struct AppSessionTerminals: SessionTerminals {
    func drive(_ s: Session, _ steps: TerminalSteps, done: @escaping (Bool) -> Void) {
        MainActor.assumeIsolated { driveSession(s, steps) { done($0) } }
    }

    func waitForExit(_ pid: Int32, timeout: Double, done: @escaping (Bool) -> Void) {
        MainActor.assumeIsolated { waitExit(pid, timeout: timeout) { done($0) } }
    }

    func focus(_ s: Session) { focusSession(s) }
    func bringForward(_ s: Session) { activateOwner(s) }
}

let systemTerminals = SystemTerminalApps()
let boardTabs = HubTabs()
let tabProbe = HubProbe()
// 세션을 띄운 앱의 기억(세션마다)을 쥔다 — 보드가 떠 있는 동안 하나
let terminalControl = TerminalControl(apps: systemTerminals, processes: SystemProcessControl(), titles: TtyTitles(), tabs: boardTabs)
let sessionControl = SessionControl(terminals: AppSessionTerminals(), tabs: boardTabs, processes: SystemProcessControl(),
                                    marks: SessionMarkFiles(), clock: SystemClock())

// claude 프로세스의 조상을 따라 올라가 그 세션을 띄운 앱을 찾는다.
// iTerm2 는 셸을 별도 서버 프로세스(부모가 launchd)에서 돌린다 — 그 서버 실행 경로로 iTerm2 로 판별한다. 못 찾으면 nil
func ancestorApp(_ pid: Int32) -> String? {
    var p = pid
    for _ in 0..<12 where p > 1 {
        if let id = NSRunningApplication(processIdentifier: p)?.bundleIdentifier { return id }
        // 앱 안의 보조 프로세스(예: Orca Helper)는 실행 파일 경로에서 가장 바깥 .app 번들을 찾는다
        var buf = [CChar](repeating: 0, count: 4096)
        if proc_pidpath(p, &buf, UInt32(buf.count)) > 0, let r = String(cString: buf).range(of: ".app/"),
           let id = Bundle(path: String(String(cString: buf)[..<r.lowerBound]) + ".app")?.bundleIdentifier {
            return id
        }
        // iTerm2 의 셸 서버는 앱 번들 밖(~/Library/Application Support/iTerm2/iTermServer-x.y.z)에 있다
        if String(cString: buf).contains("/Application Support/iTerm2/iTermServer") { return TerminalBundle.iterm }
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, p]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        p = info.kp_eproc.e_ppid
    }
    return nil
}

// 한 번 클릭: 그 세션의 터미널 앱만 앞으로 가져온다 — 백그라운드·끝난 세션이나 앱을 못 찾으면 기본 터미널(Ghostty)
func activateOwner(_ s: Session) {
    DispatchQueue.global(qos: .userInitiated).async {
        let app = terminalControl.ownerToActivate(s)
        DispatchQueue.main.async {
            if let app { systemTerminals.activate(app) } else { systemTerminals.openGhostty() }
        }
    }
}

// 두 번 클릭: 그 세션의 터미널 창까지 — iTerm2·Terminal 은 tty 로 그 탭을(못 찾으면 앱만), 그 밖의 앱(VS Code 등)은 앱만,
// Ghostty 이거나 앱을 못 찾으면 표지 제목으로 그 터미널을. 터미널이 없는 세션은 기본 터미널(Ghostty)만
func focusSession(_ s: Session) {
    guard terminalControl.canFocus(s) else { return systemTerminals.openGhostty() }
    DispatchQueue.global(qos: .userInitiated).async {
        switch terminalControl.focusRoute(s) {
        case .tty(let app):
            if terminalControl.focusByTty(app, s.tty) { return }
            DispatchQueue.main.async { systemTerminals.activate(app) }
        case .app(let app):
            DispatchQueue.main.async { systemTerminals.activate(app) }
        case .ghostty:
            focusGhostty(s)
        }
    }
}

// 더블 클릭한 세션의 Ghostty 터미널로 포커스를 옮긴다 — Ghostty AppleScript 사전에는 tty 가 없으므로 그 tty 에 잠시 표지 제목을 써서
// 터미널을 찾은 뒤 원래 제목으로 되돌린다. 못 찾으면 Ghostty 밖(다른 터미널)에서 도는 세션이다 — 앱만 앞으로 가져온다
func focusGhostty(_ s: Session) {
    ghosttyQueue.async {
        if !terminalControl.focusGhostty(s, marker: titleMarker("CBFOCUS", UUID())) {
            DispatchQueue.main.async { systemTerminals.openGhostty() }
        }
    }
}

// Ghostty 에서 도는 터미널 세션인가 — AppleScript 로 키를 보낼 수 있다
func canDrive(_ s: Session) -> Bool { terminalControl.canDrive(s) }

// 세션 터미널에 직접 키를 넣을 수 있는가 — 보드 탭 세션(그 탭의 대화형 claude)과 Ghostty 세션
@MainActor func canDriveSession(_ s: Session) -> Bool { terminalControl.driveRoute(s) != .unavailable }

// 세션 터미널에 키·글자를 넣는다. 단계: ("key", "escape"|"enter") | ("text", 붙여 넣을 글자) | ("wait", 초).
// 보드 탭 세션은 그 탭에 직접, Ghostty 세션은 AppleScript(ghosttyQueue). 넣을 수 없으면 done(false) — done 은 메인에서
@MainActor func driveSession(_ s: Session, _ steps: [(String, String)], done: @escaping @MainActor (Bool) -> Void = { _ in }) {
    switch terminalControl.driveRoute(s) {
    case .tab(let id):
        boardTabs.drive(id, steps) { ok in MainActor.assumeIsolated { done(ok) } }
    case .ghostty:
        ghosttyQueue.async {
            let ok = terminalControl.ghosttyDrive(s, steps, marker: titleMarker("CBFIND", UUID()))
            DispatchQueue.main.async { done(ok) }
        }
    case .unavailable:
        done(false)
    }
}

// 프로세스가 끝나기를 기다린다(최대 timeout 초) — 끝나면 done(true). 0.2초마다 다시 본다(판단은 BoardRules.exitWait)
@MainActor func waitExit(_ pid: Int32, timeout: Double, done: @escaping @MainActor (Bool) -> Void) {
    switch exitWait(pid: pid, timeout: timeout, processes: SystemProcessControl()) {
    case .exited: done(true)
    case .timedOut: done(false)
    case .poll(let after, let remaining):
        DispatchQueue.main.asyncAfter(deadline: .now() + after) { waitExit(pid, timeout: remaining, done: done) }
    }
}

// 다른 터미널(Ghostty)에서 도는 세션을 보드로 — 그쪽을 /exit 로 끝내고, 끝나면 보드 새 탭에서 resume_cmd 로 이어 연다(대화 그대로)
@MainActor func moveToBoard(_ s: Session) {
    guard TerminalHub.shared.slide == nil else { return whenPaneSettled { moveToBoard(s) } }
    if moveNeedsConfirm(s) {
        let a = NSAlert()
        a.messageText = "보드로 옮길까요?"
        a.informativeText = "\(s.title)\n\n지금 터미널의 세션을 끝내고 보드 새 탭에서 같은 대화를 이어 엽니다. 진행 중인 응답은 끊깁니다."
        a.addButton(withTitle: "옮기기")
        a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"
        NSApp.activate(ignoringOtherApps: true)
        guard a.runModal() == .alertFirstButtonReturn else { return }
    }
    sessionControl.moveToBoard(s)
}

let sessionsDir = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/sessions")
let muteAllPath = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/mute-all")
// 알림 패널 위치 — 모서리를 고르면 그 값(top-left 등)을 남기고, notify.py 가 읽어 claude-notifier --position 으로 넘긴다. 없으면 가운데
let notifyPositionPath = (NSHomeDirectory() as NSString).appendingPathComponent(".claude/notifier/notify-position")

// 전체 알림이 꺼졌는지 — 세션 스위치를 잠그는 데 쓴다
private struct NotifyLockedKey: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var notifyLocked: Bool {
        get { self[NotifyLockedKey.self] }
        set { self[NotifyLockedKey.self] = newValue }
    }
}

extension Notification.Name { static let boardReload = Notification.Name("com.claude.board.reload") }

// 알림 끄기 표지는 세션 기록이 지워질 때 state.py prune 이 함께 치운다
func setMuted(_ s: Session, _ on: Bool) {
    // 화면은 MuteToggle 이 먼저 바꾸고, 표지 파일 반영과 다시 읽기는 다음 틱에 한다
    DispatchQueue.main.async {
        SessionMarkFiles().setMuted(s.id, on)
        NotificationCenter.default.post(name: .boardReload, object: nil)
    }
}

func pausedMark(_ s: Session) -> String { (sessionsDir as NSString).appendingPathComponent("\(s.id).paused") }

// 일시중지: Esc 로 지금 응답을 멈추고 표지를 남긴다(BoardRules.SessionControl.pause)
@MainActor func pauseSession(_ s: Session) { sessionControl.pause(s) }

// 재개: 살아 있는 세션(끝나지 않았고 pid 가 없거나 살아 있음)은 그 터미널에 이어서 진행하라는 요청을 넣고, 꺼진 세션은 보드 새 탭에서 이어 연다.
// 살아 있는데 키를 넣지 못하면 이어 열지 않고(claude 가 둘이 된다) 그 터미널에서 이어 가라고 알린다(SessionControl.resumePaused)
@MainActor func resumePaused(_ s: Session) { sessionControl.resumePaused(s) }

// 중단: 확인을 받은 뒤 세션을 끝낸다. 터미널 세션(보드 탭·Ghostty)은 Esc → /exit, 그 밖의 터미널 세션과 보드 탭 안의
// claude -p 는 pid 가 살아 있을 때만 SIGTERM. 진짜 백그라운드 세션에는 신호를 보내지 않는다(메뉴에도 없다 — 보드는 백그라운드 세션을 건드리지 않는다).
// 끝난 세션(--action stop 으로 올 수 있다)은 일시중지 표지만 지우고 신호를 보내지 않는다 — 남은 pid 를 다른 프로세스가 다시 쓸 수 있다.
// 대화 기록은 남으므로 종료 세션 메뉴의 "세션 다시 열기"로 이어 열 수 있다. 순서와 판단은 SessionControl.stop
@MainActor func stopSession(_ s: Session, confirm: Bool = true) {
    if confirm, TerminalHub.shared.slide != nil { return whenPaneSettled { stopSession(s, confirm: confirm) } }
    if confirm {
    let alert = NSAlert()
    alert.messageText = "세션을 중단할까요?"
    alert.informativeText = "\(s.topic.isEmpty ? s.id : s.topic)\n\n진행 중인 응답을 멈추고 세션을 끝냅니다. 대화 기록은 남아서 나중에 다시 열 수 있습니다."
    alert.addButton(withTitle: "중단")
    alert.addButton(withTitle: "취소")
    alert.buttons.first?.hasDestructiveAction = true
    NSApp.activate(ignoringOtherApps: true)
    guard alert.runModal() == .alertFirstButtonReturn else { return }
    }
    sessionControl.stop(s)
}

// 대화 기록 위치 — hook 이 알려 준 경로가 없으면 ~/.claude/projects/<작업 경로의 영숫자 외 문자를 - 로 바꾼 이름>/<세션 ID>.jsonl
func transcriptPath(_ given: String?, _ cwd: String, _ id: String) -> String {
    if let g = given, !g.isEmpty { return g }
    let slug = cwd.replacingOccurrences(of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
    return NSHomeDirectory() + "/.claude/projects/\(slug)/\(id).jsonl"
}

// 끝난 세션을 보드 탭에서 그 세션의 프로젝트 경로(claude 를 띄운 폴더 — 보드 카드·탭의 경로와 같다. cd·워크트리로 옮긴 cwd 가 아니다)로
// 이어 연다 — 그 세션이 마지막으로 돌던 화면이 같은 경로의 셸 프롬프트로 남아 있으면 그 화면에, 아니면 새 탭에. 대화 이력은 claude --resume 이 불러오고,
// 실행 인자·마지막 권한 모드·모델은 state.py 가 기록한 resume_cmd 에 담겨 있다(-p 는 넘기지 않아 대화형으로 열린다).
// 일시중지했던 세션(carryOn)은 첫 요청으로 이어서 진행하라는 문구를 넘겨 바로 다음 작업을 시작하게 한다. 순서는 SessionControl.reopen
@MainActor func resumeSession(_ s: Session, carryOn: Bool = false) { sessionControl.reopen(s, carryOn: carryOn) }

// 명령줄 다시 열기(--action reopen) — 메뉴처럼 끝난 세션만 이어 열고 이어서 진행 요청은 일시중지했던 세션에만. 끝나지 않은 세션은 알리기만(SessionControl.reopenEnded)
@MainActor func reopenEnded(_ s: Session) { sessionControl.reopenEnded(s) }

// 세션을 한 번 눌렀을 때(목록·미니·알림 패널 "세션으로 이동") — 보드 탭이면 펼치고 그 탭의 그 화면으로, 끝난 세션은 보드 새 탭에서 이어 열기,
// 백그라운드 세션은 안내만, 다른 터미널 세션은 지금처럼 그 터미널로(두 번 누르면 그 세션의 터미널 창까지). 갈래는 SessionControl.open
@MainActor func openSession(_ s: Session, double: Bool = false) { sessionControl.open(s, double: double) }

// 터미널을 펼쳐 창이 화면 전체를 덮은 동안에는 고정하지 않는다 — 화면 전체를 덮은 창이 늘 위에 있으면 다른 앱을 쓸 수 없다.
// 펼친 채 창 크기 전환(60%)·손으로 줄인 창과 접은 창은 고정
@MainActor func applyPin(_ pinned: Bool) {
    let level: NSWindow.Level = pinned && !expandedToFullScreen() ? .floating : .normal
    if window.level != level { window.level = level }   // 창 크기가 바뀔 때마다 불린다
}

@MainActor func setPinned(_ pinned: Bool) {
    UserDefaults.standard.set(pinned, forKey: "pinned")
    applyPin(pinned)
}

func showBoard() {
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    let store = Store()
    let usage = UsageStore()
    let accounts = AccountStore()
    lazy var autoSwitcher = AutoSwitcher(store: store, usage: usage, accounts: accounts)
    var statusItem: NSStatusItem!
    private var statusShown = ""

    func applicationDidFinishLaunching(_ notification: Notification) {
        UserDefaults.standard.register(defaults: ["pinned": true, "mascot": Mascot.tralalero.rawValue,
                                                  "autoUpdateCheck": true, "newSessionMdRender": true])
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: boardLaunchWidth, height: 620),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isMovableByWindowBackground = true
        window.isOpaque = false
        window.backgroundColor = .clear
        window.appearance = NSAppearance(named: .darkAqua)
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.hidesOnDeactivate = false
        window.isReleasedWhenClosed = false   // 닫아도 앱은 남고, Dock·메뉴 막대 아이콘으로 다시 연다
        window.title = instanceName == "claude-board" ? "Claude 세션 보드" : "Claude 세션 보드 (시험 빌드)"

        let blur = NSVisualEffectView()
        blur.material = .hudWindow
        blur.blendingMode = .behindWindow
        blur.state = .active
        let host = FirstClickHostingView(rootView: BoardView(store: store, usage: usage, accounts: accounts))
        host.translatesAutoresizingMaskIntoConstraints = false
        // 화면이 바뀔 때마다 SwiftUI 전체의 이상적 크기를 재서 창에 알리지 않는다 — 창 크기·최소 크기는 직접 정한다.
        // 켜 두면 반짝임·회전 애니메이션 프레임마다 창 전체 배치를 다시 계산해 클릭 반응이 늦어진다
        host.sizingOptions = []
        blur.addSubview(host)
        NSLayoutConstraint.activate([
            host.leadingAnchor.constraint(equalTo: blur.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: blur.trailingAnchor),
            host.topAnchor.constraint(equalTo: blur.topAnchor),
            host.bottomAnchor.constraint(equalTo: blur.bottomAnchor),
        ])
        let root = BoardRootView(board: blur)
        window.contentView = root
        TerminalHub.shared.area = root.terminal
        TerminalHub.shared.store = store
        PointerTracker.shared.attach(blur)
        PointerTracker.shared.attachWindow(root)
        _ = mdRenderDir   // 답변 꾸미기 플러그인을 ~/.claude/notifier 로 옮겨 둔다 — 되살릴 탭이 쓴다
        pruneMdRenderOff()
        DispatchQueue.main.async { TerminalHub.shared.restoreTabs() }   // 지난번에 쥐고 있던 탭을 되살린다
        // 손으로 바꿔도 자동 전환이 전환 시각을 알아야 한다 — 그 뒤 잠시 난 한도 오류는 이전 계정 토큰 탓이다
        accounts.onSwitch = { [usage, autoSwitcher] in
            usage.reset()
            autoSwitcher.switched()
        }
        let nc = NotificationCenter.default
        // 가려지거나 숨은 동안에는 늘 도는 애니메이션 시계를 멈춘다
        // 터미널도 창이 안 보이는 동안(가려짐·최소화·닫힘·다른 데스크톱)은 그리지 않는다
        nc.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { _ in
            AmbientTicker.shared.running = window.occlusionState.contains(.visible)
            MainActor.assumeIsolated { TerminalHub.shared.applyVisibility() }
        }
        // 미니 화면 크기보다 창을 더 줄이지 못하게 한다 — 그보다 크면 창 크기에 따라 전체·중간·미니로 그린다
        // 터미널을 펼쳤으면 터미널 최소 폭과 보드 최소 폭이 들어가는 크기까지
        window.contentMinSize = TerminalHub.shared.expanded ? terminalExpandedMinSize : NSSize(width: miniWidth, height: miniMinHeight)
        window.delegate = sizeGuard   // contentMinSize 만으로는 손으로 줄일 때 막히지 않아 한 번 더 막는다
        // 터미널을 펼쳤을 때 제목 막대·탭 줄 빈 곳 더블클릭 = 창 크기 전환(화면 전체 ↔ 60%). 그 클릭은 삼킨다
        NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { e in
            guard isWindowSizeDoubleClick(e) else { return e }
            toggleWindowSize()
            return nil
        }
        // ⌘ 를 누르고 있으면 탭 줄에 ⌘번호·이름 바꾸기 단축키를 보인다.
        // ⌘Tab 등으로 앱이 비활성이 되면 놓는 신호가 오지 않으므로 끈다
        NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { e in
            TerminalHub.shared.commandFlag(e.modifierFlags.contains(.command))
            return e
        }
        nc.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { TerminalHub.shared.commandFlag(false) }
        }

        // 저장된 창 위치 끝에 macOS 타일 배치 기록({"tilingState":…})이 붙어 있으면 창을 띄우는 순간 macOS 가 그 타일 크기로
        // 되돌린다(2026-10-16 확인 — 미니로 둔 300×144 가 화면 높이로, 폭 454 가 타일 폭 385 로 열렸다). 크기·위치 부분만 남긴다
        let frameKey = "NSWindow Frame ClaudeBoard"
        if let saved = UserDefaults.standard.string(forKey: frameKey), let brace = saved.firstIndex(of: "{") {
            UserDefaults.standard.set(String(saved[..<brace]), forKey: frameKey)
        }
        // 저장된 가로 위치 그대로 — setFrameUsingName 은 화면 오른쪽 끝(Dock 옆)에 붙여 둔 접힌 보드를 46pt 왼쪽으로 옮겨 연다(2026-10-17 실측)
        let savedX = UserDefaults.standard.string(forKey: frameKey)?.split(separator: " ").first.flatMap { Double($0) }
        if !window.setFrameUsingName("ClaudeBoard") { window.center() }
        window.setFrameAutosaveName("ClaudeBoard")   // 저장된 프레임을 다시 적용하므로 폭 조정은 이 뒤에 한다
        // 실행할 때는 가로 위치만 마지막 상태로 두고, 폭은 접을 때와 같게 경계를 끌어 정한 폭(끌지 않았으면 boardLaunchWidth),
        // 높이는 그 화면의 쓸 수 있는 높이 전체(메뉴 막대·Dock 제외)로 연다
        // (화면 분할 상태가 섞여 전체 폭으로 열리는 것도 막는다). 미니로 두고 껐으면 그 크기·위치 그대로 연다.
        // 터미널을 펼친 채로 껐으면(기본값) 화면의 쓸 수 있는 영역 전체로 연다
        if TerminalHub.shared.expanded, let vf = (window.screen ?? NSScreen.main)?.visibleFrame {
            window.setFrame(vf, display: false)
        } else if !UserDefaults.standard.bool(forKey: "boardMini") {
            var f = window.frame
            f.size.width = TerminalHub.shared.boardWidth
            if let vf = (window.screen ?? NSScreen.main)?.visibleFrame {
                f.origin.y = vf.minY
                f.size.height = vf.height
            }
            if let x = savedX, NSScreen.screens.contains(where: { $0.visibleFrame.intersects(NSRect(x: x, y: f.minY, width: f.width, height: f.height)) }) {
                f.origin.x = x
            }
            window.setFrame(f, display: false)
        }
        addSnapButton()
        addTerminalToggle()
        addMiniTitlebar(store)
        BoardFit.shared.update()   // 창을 띄우기 전에 단계를 정한다 — 미니로 열 때 전체 화면이 한 번 비치지 않게
        // 미니에서 돌아갈 위치는 접힌 보드의 위치만 기억한다
        if BoardFit.shared.tier != .mini, !TerminalHub.shared.expanded { saveRestoreFrame() }
        // 실행 크기를 정한 뒤부터 창 크기를 따라 단계를 고른다.
        // 펼친 창이 화면 전체가 되거나 벗어나면(창 크기 전환·초록 단추·손으로 바꿈·시스템 타일 배치) 위에 고정·투명도도 다시 맞춘다 —
        // 펼치기·접기 애니메이션 중에는 끝난 뒤 settleTerminalPane 이 맞춘다
        nc.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) { _ in
            MainActor.assumeIsolated {
                BoardFit.shared.update()
                if TerminalHub.shared.slide == nil {
                    applyPin(UserDefaults.standard.bool(forKey: "pinned"))
                    WindowOpacity.shared.apply()
                }
            }
        }
        nc.addObserver(forName: NSWindow.didEndLiveResizeNotification, object: window, queue: .main) { _ in
            MainActor.assumeIsolated { if BoardFit.shared.tier != .mini, !TerminalHub.shared.expanded { saveRestoreFrame() } }
        }
        Shortcuts.shared.registerHotKeys()
        // 보드 단축키는 메뉴보다 먼저 키 번호로 맞춘다(Shortcuts.handle)
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in Shortcuts.shared.handle(e) }
        applyPin(UserDefaults.standard.bool(forKey: "pinned"))
        WindowOpacity.shared.apply()
        buildMainMenu()
        buildStatusItem()
        // 셸 설정에서 --background 로 띄우면 창만 앞에 보이고 포커스는 터미널에 남긴다
        if CommandLine.arguments.contains("--background") { window.orderFrontRegardless() } else { showBoard() }

        DistributedNotificationCenter.default().addObserver(forName: .init("\(instanceName).show"), object: nil,
                                                            queue: .main) { _ in showBoard() }
        DistributedNotificationCenter.default().addObserver(forName: .init("\(instanceName).action"), object: nil,
                                                            queue: .main) { n in
            if let a = n.userInfo?["action"] as? String, let id = n.userInfo?["id"] as? String { MainActor.assumeIsolated { performAction(a, id) } }
        }
        // Ghostty 가 실행되면 보드 창도 함께 띄운다 — 포커스는 Ghostty 에 그대로 둔다
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didLaunchApplicationNotification,
                                                          object: nil, queue: .main) { note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            if app?.bundleIdentifier == "com.mitchellh.ghostty" { window.orderFrontRegardless() }
        }
    }

    // 상단 앱 메뉴: 보드 보기 · 항상 위에 고정 · 종료
    @MainActor func buildMainMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        main.addItem(appItem)
        let appMenu = NSMenu()
        let keys = Shortcuts.shared
        keys.bind(appMenu.addItem(withTitle: "세션 보드 보기", action: #selector(showBoardAction), keyEquivalent: ""), "showBoard")
        keys.bind(appMenu.addItem(withTitle: "항상 위에 고정", action: #selector(togglePin), keyEquivalent: ""), "pin")
        // 설정 화면 — 터미널 영역을 설정 화면으로 바꾼다. ⌘, 는 표에 없고 바꿀 수 없다(엔진이 먼저 가져가는 키라 Shortcuts.handle 이 받는다)
        let settings = ActionItem("설정…") { SettingsScreen.shared.open() }
        settings.keyEquivalent = ","
        appMenu.addItem(settings)
        appMenu.addItem(withTitle: "업데이트 확인…", action: #selector(checkForUpdates), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Claude Board 가리기", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(withTitle: "Claude Board 종료", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        // 편집 — 첫 응답자에게 보낸다(target nil). 탭 이름 칸·새 세션 창 명령 칸의 ⌘A·⌘C·⌘V 는 이 메뉴가 있어야 듣는다.
        // 터미널에 포커스가 있으면 엔진이 묶은 ⌘C·⌘V·⌘A·⌘Z·⇧⌘Z 는 뷰의 performKeyEquivalent 가 메뉴보다 먼저 받는다
        let editItem = NSMenuItem()
        main.addItem(editItem)
        let editMenu = NSMenu(title: "편집")
        // 실행 취소·복귀는 직접 보낸다 — target nil 이면 AppKit 이 제목을 앱 언어(지역 선언이 없어 영어)의 "Undo …" 로 바꿔 쓴다(실측).
        // 앱 지역을 ko 로 선언하면 고쳐지지만 엔진이 셸 LANG 을 앱 로캘로 정해 터미널까지 바뀐다. 할 것이 없어도 꺼지지 않는다
        for (title, sel, mods) in [("실행 취소", "undo:", NSEvent.ModifierFlags.command), ("실행 복귀", "redo:", [.command, .shift])] {
            let item = ActionItem(title) { NSApp.sendAction(Selector(sel), to: nil, from: nil) }
            item.keyEquivalent = "z"
            item.keyEquivalentModifierMask = mods
            editMenu.addItem(item)
        }
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "잘라내기", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "복사하기", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "붙여넣기", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(withTitle: "모두 선택", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = editMenu

        let windowItem = NSMenuItem()
        main.addItem(windowItem)
        let windowMenu = NSMenu(title: "윈도우")
        windowMenu.addItem(withTitle: "닫기", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(withTitle: "최소화", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(.separator())
        // 단축키 표시용 — 실제 동작은 전역 단축키(registerSnapHotKey)가 받는다. 제목(오른쪽·왼쪽)은 validateMenuItem 이 보드 위치에 맞춘다
        let snap = NSMenuItem(title: "처음 크기로 오른쪽에 붙이기", action: #selector(snapAction), keyEquivalent: "")
        keys.bind(snap, "snap")
        windowMenu.addItem(snap)
        let term = ActionItem("터미널 펼치기·접기") { toggleTerminal() }
        keys.bind(term, "terminal")
        windowMenu.addItem(term)
        // 탭 줄의 "+ 새 세션" 과 같다(접혀 있으면 펼친다). 터미널에 포커스가 있어도 오도록 엔진의 ⌘T(new_tab)는 풀어 두었다
        let newSession = ActionItem("새 세션…") { TerminalHub.shared.newSessionSheet() }
        keys.bind(newSession, "newSession")
        windowMenu.addItem(newSession)
        // ⌘1~⌘9 — 탭 줄(지금 경로의 탭) 순서대로 그 탭으로(접혀 있으면 펼친다). 그 자리에 탭이 없으면 그대로. 엔진의 ⌘1~⌘9(goto_tab·last_tab)는 풀어 두었다
        let tabMenu = NSMenu(title: "탭으로 가기")
        for n in 1...9 {
            let item = ActionItem("탭 \(n)") {
                let hub = TerminalHub.shared, list = hub.tabs(in: hub.workspace)
                if list.indices.contains(n - 1) { hub.reveal(list[n - 1].id) }
            }
            keys.bind(item, "tab\(n)")
            tabMenu.addItem(item)
        }
        tabMenu.addItem(.separator())
        // 키는 단축키 표(id)에서 — 숨긴 항목은 메뉴에 보이지 않는 보조 키
        // 설정 화면이 떠 있으면 닫고 한다(잠깐 펼쳤어도 펼친 채로)
        func keyItem(_ menu: NSMenu, _ title: String, _ id: String, hidden: Bool = false, run: @escaping @MainActor () -> Void) {
            let item = ActionItem(title) { SettingsScreen.shared.close(keep: true); run() }
            keys.bind(item, id)
            item.isHidden = hidden
            item.allowsKeyEquivalentWhenHidden = hidden
            menu.addItem(item)
        }
        // ⌃⌥←·⌃⌥→ — 탭 줄에서 왼쪽·오른쪽 탭으로(지금 경로 안에서, 끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다). Ghostty 의 탭 키
        // ⇧⌘[·⇧⌘]·⌃⇧⇥·⌃⇥ 도 숨긴 항목으로 받는다(엔진의 같은 키 previous_tab·next_tab 은 풀어 두었다)
        keyItem(tabMenu, "이전 탭", "prevTab") { TerminalHub.shared.stepTab(-1) }
        keyItem(tabMenu, "다음 탭", "nextTab") { TerminalHub.shared.stepTab(1) }
        keyItem(tabMenu, "이전 탭", "prevTab2", hidden: true) { TerminalHub.shared.stepTab(-1) }
        keyItem(tabMenu, "다음 탭", "nextTab2", hidden: true) { TerminalHub.shared.stepTab(1) }
        keyItem(tabMenu, "이전 탭", "prevTab3", hidden: true) { TerminalHub.shared.stepTab(-1) }
        keyItem(tabMenu, "다음 탭", "nextTab3", hidden: true) { TerminalHub.shared.stepTab(1) }
        tabMenu.addItem(.separator())
        // ⌃⌥↑·⌃⌥↓ — 경로 줄에서 앞·뒤 경로로(그 경로에서 마지막에 고른 탭, 끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다)
        keyItem(tabMenu, "이전 경로", "prevWorkspace") { TerminalHub.shared.stepWorkspace(-1) }
        keyItem(tabMenu, "다음 경로", "nextWorkspace") { TerminalHub.shared.stepWorkspace(1) }
        windowMenu.addItem(withTitle: "탭으로 가기", action: nil, keyEquivalent: "").submenu = tabMenu
        // 화면 나누기 — Ghostty 와 같은 키. 보드가 탭마다 화면 트리를 쥐고, 엔진의 같은 키는 풀어 두었다(⌘W 는 엔진이 그 화면을 닫는다)
        let splitMenu = NSMenu(title: "화면 나누기")
        keyItem(splitMenu, "오른쪽으로 나누기", "splitRight") { TerminalHub.shared.split(right: true) }
        keyItem(splitMenu, "아래로 나누기", "splitDown") { TerminalHub.shared.split(right: false) }
        splitMenu.addItem(.separator())
        keyItem(splitMenu, "이전 화면", "prevSurface") { TerminalHub.shared.focusSurface(step: -1) }
        keyItem(splitMenu, "다음 화면", "nextSurface") { TerminalHub.shared.focusSurface(step: 1) }
        let directions: [(String, String, SplitDirection)] = [("위로", "Up", .up), ("아래로", "Down", .down),
                                                             ("왼쪽으로", "Left", .left), ("오른쪽으로", "Right", .right)]
        for (name, id, d) in directions {
            keyItem(splitMenu, "\(name) 화면 고르기", "focus\(id)") { TerminalHub.shared.focusSurface(d) }
        }
        splitMenu.addItem(.separator())
        for (name, id, d) in directions {
            keyItem(splitMenu, "경계를 \(name) 옮기기", "resize\(id)") { TerminalHub.shared.resizeSplit(d) }
        }
        keyItem(splitMenu, "크기 고르게", "equalize") { TerminalHub.shared.equalizeSplits() }
        splitMenu.addItem(.separator())
        keyItem(splitMenu, "화면 하나 크게 보기", "zoom") { TerminalHub.shared.toggleZoom() }
        windowMenu.addItem(withTitle: "화면 나누기", action: nil, keyEquivalent: "").submenu = splitMenu
        // ⇧⌘R — 고른 탭의 이름을 탭 안에서 고친다(접혀 있으면 펼친다). 탭이 없으면 그대로. 엔진 기본값은 이 키를 묶지 않는다
        let rename = ActionItem("탭 이름 바꾸기") { SettingsScreen.shared.close(keep: true); TerminalHub.shared.beginRename() }
        keys.bind(rename, "rename")
        windowMenu.addItem(rename)
        // 글자 크기 — 터미널(모든 탭, 1pt씩)과 보드(10%씩)를 따로 정한다. 둘 다 다시 켜도 남는다. 엔진의 ⌘+·⌘=·⌘-·⌘0 은 풀어 두어
        // 터미널 포커스에서도 여기로 온다. ⌘0 은 이미 "세션 보드 보기" 라 터미널 원래 크기는 단축키가 없다
        let fontMenu = NSMenu(title: "글자 크기")
        // 보이는 항목은 +, 같은 키의 = (Shift 없이)는 숨긴 항목으로 받는다
        func sizeItem(_ title: String, _ id: String, hidden: Bool = false, run: @escaping @MainActor () -> Void) {
            let item = ActionItem(title, run: run)
            keys.bind(item, id)
            item.isHidden = hidden
            item.allowsKeyEquivalentWhenHidden = hidden
            fontMenu.addItem(item)
        }
        // + 는 Shift 까지 넣은 키(⇧⌘=)다 — 예전처럼 ⌘ 만 넣은 "+" 는 AppKit 이 ⌥⇧⌘= 에서 ⌥ 를 빼고 터미널 쪽으로 보냈다(실측).
        // 지금은 Shortcuts.handle 이 키 번호로 먼저 맞춰 메뉴까지 가지 않는다
        sizeItem("터미널 글자 크게", "terminalBigger") { TerminalHub.shared.changeTerminalFont(1) }
        sizeItem("터미널 글자 크게", "terminalBigger2", hidden: true) { TerminalHub.shared.changeTerminalFont(1) }
        sizeItem("터미널 글자 작게", "terminalSmaller") { TerminalHub.shared.changeTerminalFont(-1) }
        sizeItem("터미널 글자 원래 크기", "terminalReset") { TerminalHub.shared.changeTerminalFont(nil) }
        fontMenu.addItem(.separator())
        sizeItem("보드 글자 크게", "boardBigger") { BoardFontScale.shared.change(1) }
        sizeItem("보드 글자 크게", "boardBigger2", hidden: true) { BoardFontScale.shared.change(1) }
        sizeItem("보드 글자 작게", "boardSmaller") { BoardFontScale.shared.change(-1) }
        sizeItem("보드 글자 원래 크기", "boardReset") { BoardFontScale.shared.change(nil) }
        windowMenu.addItem(withTitle: "글자 크기", action: nil, keyEquivalent: "").submenu = fontMenu
        windowItem.submenu = windowMenu
        NSApp.mainMenu = main
    }

    // 상단 메뉴 막대 아이콘: 누르면 보드를 열고 닫는다. 승인 대기·작업 중 세션 수를 함께 보여 준다
    func buildStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            // 공유 이미지의 크기를 바꾸면 보드 제목 옆 아이콘에도 번지므로 복사해서 줄인다
            let icon = appIcon.copy() as! NSImage
            icon.size = NSSize(width: 18, height: 18)
            icon.accessibilityDescription = "Claude Board"
            button.image = icon
            button.toolTip = "Claude Board: 누르면 보드를 열고 닫습니다"   // 보드 밖이라 시스템 툴팁
            button.imagePosition = .imageLeading
            button.action = #selector(toggleBoard)
            button.target = self
        }
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.updateStatusTitle() }
        RunLoop.main.add(timer, forMode: .common)
        updateStatusTitle()
    }

    func updateStatusTitle() {
        let waiting = store.count("waiting"), working = store.count("working")
        // 매초 같은 제목을 다시 넣으면 메뉴 막대가 매초 다시 그려진다 — 숫자가 바뀔 때만 넣는다
        guard "\(waiting)|\(working)" != statusShown else { return }
        statusShown = "\(waiting)|\(working)"
        let title = NSMutableAttributedString()
        let font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .semibold)
        if waiting > 0 {
            title.append(NSAttributedString(string: " \(waiting)", attributes: [
                .font: font, .foregroundColor: NSColor(red: 1.0, green: 0.66, blue: 0.36, alpha: 1)]))
        }
        if working > 0 {
            title.append(NSAttributedString(string: " \(working)", attributes: [
                .font: font, .foregroundColor: NSColor(red: 0.45, green: 0.66, blue: 1.0, alpha: 1)]))
        }
        statusItem.button?.attributedTitle = title
    }

    @objc func toggleBoard() {
        if window.isVisible && NSApp.isActive && window.isKeyWindow {
            window.orderOut(nil)
        } else {
            showBoard()
        }
    }

    @objc func showBoardAction() { showBoard() }

    @objc func togglePin() { MainActor.assumeIsolated { setPinned(!UserDefaults.standard.bool(forKey: "pinned")) } }

    @objc func snapAction() { MainActor.assumeIsolated { snapToSide() } }

    // 결과는 보드의 알림 줄에 뜨므로 보드를 먼저 연다
    @objc func checkForUpdates() {
        showBoard()
        Updater.shared.check(manual: true)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(togglePin) { item.state = UserDefaults.standard.bool(forKey: "pinned") ? .on : .off }
        let left = UserDefaults.standard.string(forKey: "boardSide") == "left"
        if item.action == #selector(snapAction) { item.title = left ? "처음 크기로 왼쪽에 붙이기" : "처음 크기로 오른쪽에 붙이기" }
        return true
    }

    // 끌 때 — 계정 추가 로그인 명령을 끝내고, 설정 화면 때문에 잠깐 펼친 창은 접힌 프레임으로
    func applicationWillTerminate(_ notification: Notification) {
        accounts.cancelAdd()
        MainActor.assumeIsolated { SettingsScreen.shared.restoreBeforeQuit() }
    }

    // Dock 아이콘을 누르면 닫아 둔 창을 다시 연다
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showBoard()
        return true
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    // 끄기 전 확인 — 보드 탭 세션 가운데 일하는 중인 것이 있으면 묻는다(⌘Q·메뉴·Dock·install.sh 가 청한 종료). pkill(업데이트 단추 뒤 등)은 여기를 거치지 않는다
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let update = quitRequested
        quitRequested = false
        let ok = MainActor.assumeIsolated { confirmShutdown(update: update) } != nil
        if !ok, update { writeQuitAck("cancel") }
        return ok ? .terminateNow : .terminateCancel
    }

    // 보드 밖에서 업데이트하는 install.sh 가 청한 종료(--action quit) — 받았다는 표지(asking)를 남기고 ⌘Q 와 같은 길로 끈다(확인 창 문구는 업데이트).
    // 확인 창에서 취소하면 cancel 을 써서 기다리던 install.sh 가 아무것도 바꾸지 않고 멈추게 한다. 표지는 사용자 임시 폴더의 <instanceName>.quit(시험 빌드와 따로)
    private var quitRequested = false
    func requestQuit() {
        writeQuitAck("asking")
        quitRequested = true
        NSApp.terminate(nil)
    }
    private func writeQuitAck(_ state: String) {
        try? state.write(toFile: NSTemporaryDirectory() + "\(instanceName).quit", atomically: true, encoding: .utf8)
    }
}
