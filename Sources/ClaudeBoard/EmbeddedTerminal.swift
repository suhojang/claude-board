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

// MARK: - 내장 터미널

// 보드가 직접 쥐는 터미널(pty). 펼치면 창이 화면의 쓸 수 있는 영역 전체로 넓어져 [터미널 | 경계 | 보드], 접으면 지금처럼 보드만 남는다.
// 터미널 뷰는 창에서 빼지 않는다 — 창에 붙은 터미널이 하나도 없으면 엔진이 메일함 처리를 멈춰, 제목 변경이 쌓이면 pty 읽기가 막히고
// 세션 출력이 멈춘다(실측 19초). 숨길 때는 실제 크기 그대로 isHidden + setSurfaceVisible(false)(isHidden 만 하면 계속 그린다).
// 탭을 닫을 때는 controller 를 떼어 엔진 surface 를 내린다(그래야 셸이 끝난다)

let paneDividerWidth: CGFloat = 8       // 터미널과 보드 사이 경계 — 끌어서 두 영역 폭을 바꾼다
let terminalMinWidth: CGFloat = 480     // 펼쳤을 때 터미널 영역의 최소 폭
let terminalTabBarHeight: CGFloat = 40
let terminalWorkspaceBarHeight: CGFloat = 32   // 탭 줄 위 경로 줄 — 탭이 있을 때만
let terminalAreaColor = NSColor(red: 0.071, green: 0.075, blue: 0.094, alpha: 1)   // 탭이 없을 때 터미널 영역 바탕
let terminalBarColor = NSColor(red: 0.10, green: 0.10, blue: 0.13, alpha: 1)        // 탭 줄과 그 위 제목 막대 띠

// 작업 경로를 견주는 꼴 — /tmp ↔ /private/tmp 같은 링크, 끝 / 차이로 같은 폴더가 어긋나지 않게(없는 경로는 글자만 다듬는다).
// 보드 카드는 1초마다 다시 그려지므로 경로마다 한 번만 푼다
var workspaceKeys: [String: String] = [:]
func workspaceKey(_ path: String) -> String {
    if let k = workspaceKeys[path] { return k }
    let k = (path as NSString).resolvingSymlinksInPath
    workspaceKeys[path] = k
    return k
}

// 경로 줄 이름 — 폴더 이름(홈은 ~). 같은 이름이 둘 이상이면 부모 폴더를 붙여 가른다
func workspaceLabel(_ ws: String, among list: [String]) -> String {
    func name(_ p: String) -> String { p == NSHomeDirectory() ? "~" : (p as NSString).lastPathComponent }
    let n = name(ws)
    guard ws != NSHomeDirectory(), list.filter({ name($0) == n }).count > 1 else { return n }
    return ((ws as NSString).deletingLastPathComponent as NSString).lastPathComponent + "/" + n
}

// 한글 조합 입력 고침 — 입력기는 조합 중 Return·Esc 에 "글자 확정" 과 "줄바꿈·취소 명령" 을 함께 보내는데 래퍼는 확정 글자가 있으면
// 명령을 버려 Return 을 두 번 눌러야 한다(Ghostty.app 1.3.1 도 같다). 입력기가 실제로 그 명령을 보냈을 때만 원래 키를 수정키 그대로
// 다시 보낸다 — 일본어 변환 확정처럼 명령이 없으면 그대로 둔다. 엔진 버전을 올리면 래퍼가 고쳤는지 다시 시험한다(고쳤으면 Enter 가 두 번 간다)
final class BoardTerminalView: AppTerminalView {
    private var composingAtKeyDown = false
    private var insertedDuringKey = false
    private var commandDuringKey: Selector?

    // 창은 배경을 끌어 옮길 수 있게 되어 있고 엔진 뷰는 불투명이 아니라 기본값이 true — 끌어서 글자를 고르면 창이 따라 움직였다(사용자 보고)
    override var mouseDownCanMoveWindow: Bool { false }

    // 키보드 초점을 받았다(화면을 누르면 래퍼의 mouseDown 이 첫 응답자로 세운다, ⌘[ 등으로 옮겨도) — 그 탭의 고른 화면으로 삼는다
    var onFocus: (() -> Void)?
    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFocus?() }
        return ok
    }

    private func mods(_ event: NSEvent) -> TerminalInputModifiers {
        var m: TerminalInputModifiers = []
        let f = event.modifierFlags
        if f.contains(.shift) { m.insert(.shift) }
        if f.contains(.control) { m.insert(.ctrl) }
        if f.contains(.option) { m.insert(.alt) }
        if f.contains(.command) { m.insert(.super_) }
        return m
    }

    // ⌘ 단축키: 한글·러시아어 등 ASCII 가 아닌 입력기에서는 키 이벤트의 글자가 'ㅈ' 처럼 나와 엔진이 단축키(super+w 등, 글자로
    // 맞춘다)인지 가리지 못할 수 있다 — 같은 키의 ASCII 글자로 다시 만든 이벤트로 묻는다. 엔진에 실제로 보내는 것은 아래 keyDown
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        super.performKeyEquivalent(with: asciiCommandEvent(event) ?? event)
    }

    override func keyDown(with event: NSEvent) {
        // ⌘ 단축키(위 performKeyEquivalent 가 다시 만든 이벤트도): 래퍼는 엔진에 넘기는 키의 글자를 지금 입력기로 다시 읽고('ㅈ')
        // 한글 입력기는 ⌘ 키에 글자를 내지 않아 엔진이 super+ㅈ 로 받는다(2026-10-19 실측) → ⌘W 가 탭을 닫지 않고 claude 에는
        // 'ㅈ' 로 보인다(사용자 보고). ASCII 자판의 같은 키로 보낸다 — ABC 에서와 같은 키 이벤트(kitty 로 ⌘J 가 같은 바이트, 실측)
        if event.modifierFlags.contains(.command),
           event.characters(byApplyingModifiers: [])?.unicodeScalars.contains(where: { !$0.isASCII }) == true,
           let ch = asciiKeyCharacters(event.keyCode, shift: event.modifierFlags.contains(.shift))?.first,
           let press = TerminalKeyPress(typing: ch, modifiers: mods(event)) {
            sendKey(press)
            return
        }
        // 수정키가 붙은 Return(Shift+Enter 등): 래퍼는 확정 글자를 그 키 이벤트에 실어 보내 키 지정이 글자를 덮는다 →
        // sendKey 로 조합 글자를 먼저(수정키 없는 이벤트로) 확정하고 키를 따로 보낸다
        if hasMarkedText(), event.keyCode == 36 || event.keyCode == 76, !mods(event).isEmpty {
            sendKey(.enter, modifiers: mods(event))
            return
        }
        composingAtKeyDown = hasMarkedText()
        insertedDuringKey = false
        commandDuringKey = nil
        super.keyDown(with: event)
        defer { composingAtKeyDown = false; commandDuringKey = nil }
        guard composingAtKeyDown, insertedDuringKey, let sel = commandDuringKey else { return }
        let key: TerminalKey
        switch sel {
        case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.insertLineBreak(_:)),
             #selector(NSResponder.insertNewlineIgnoringFieldEditor(_:)): key = .enter
        case #selector(NSResponder.cancelOperation(_:)): key = .escape
        default: return
        }
        sendKey(key, modifiers: mods(event))
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        if composingAtKeyDown { insertedDuringKey = true }
        super.insertText(string, replacementRange: replacementRange)
    }

    override func doCommand(by selector: Selector) {
        if composingAtKeyDown { commandDuringKey = selector }
        super.doCommand(by: selector)
    }
}

// ⌘ 가 눌린 키 이벤트의 글자가 ASCII 가 아니면(한글·러시아어 입력기 등) 같은 키 번호의 ASCII 글자로 다시 만든 이벤트 — 글자는 지금의
// ASCII 자판(ABC 등)에서 읽는다. 엔진의 메아리 중복 제거가 timestamp 를 견주므로 timestamp 는 그대로 둔다. 바꿀 것이 없으면 nil
func asciiCommandEvent(_ event: NSEvent) -> NSEvent? {
    guard event.type == .keyDown, event.modifierFlags.contains(.command),
          let ign = event.charactersIgnoringModifiers, ign.unicodeScalars.contains(where: { !$0.isASCII }),
          let ascii = asciiKeyCharacters(event.keyCode, shift: event.modifierFlags.contains(.shift)) else { return nil }
    return NSEvent.keyEvent(with: .keyDown, location: event.locationInWindow, modifierFlags: event.modifierFlags,
                            timestamp: event.timestamp, windowNumber: event.windowNumber, context: nil,
                            characters: ascii, charactersIgnoringModifiers: ascii, isARepeat: event.isARepeat, keyCode: event.keyCode)
}

// 키 번호가 ASCII 자판에서 치는 글자(Shift 만 반영). 글자가 없거나 ASCII 가 아니면 nil
func asciiKeyCharacters(_ keyCode: UInt16, shift: Bool) -> String? {
    guard let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue(),
          let raw = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return nil }
    let data = Unmanaged<CFData>.fromOpaque(raw).takeUnretainedValue()
    guard let bytes = CFDataGetBytePtr(data) else { return nil }
    var dead: UInt32 = 0
    var chars = [UniChar](repeating: 0, count: 4)
    var length = 0
    let status = bytes.withMemoryRebound(to: UCKeyboardLayout.self, capacity: 1) {
        UCKeyTranslate($0, keyCode, UInt16(kUCKeyActionDown), shift ? UInt32(shiftKey >> 8) : 0, UInt32(LMGetKbdType()),
                       OptionBits(kUCKeyTranslateNoDeadKeysBit), &dead, chars.count, &length, &chars)
    }
    guard status == noErr, length > 0 else { return nil }
    let s = String(utf16CodeUnits: chars, count: length)
    return s.unicodeScalars.allSatisfy({ $0.isASCII && $0.value >= 0x20 && $0.value != 0x7F }) ? s : nil
}

// 터미널 설정은 보드가 직접 만든다 — 사용자 Ghostty 설정을 바탕으로 하되 theme 줄은 뺀다(엔진 번들에 테마 폴더가 없어 theme 한 줄 때문에
// 파일 전체가 조용히 거부된다). 같은 이름 테마는 엔진의 테마 목록에서 찾아 넘긴다(넘기지 않으면 래퍼 기본 테마가 사용자 색을 덮는다).
// 보드에서 고른 테마(보드 설정 terminalTheme, 설정 > 터미널 > 테마)가 있으면 Ghostty 설정의 theme 대신 그것
@MainActor func makeTerminalController() -> TerminalController {
    var lines: [String] = []
    var theme: String?
    for f in ghosttyConfigFiles {
        guard let text = try? String(contentsOfFile: f, encoding: .utf8) else { continue }
        for line in text.components(separatedBy: .newlines) {
            let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if parts.count == 2, parts[0] == "theme" {
                theme = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "\""))
                continue
            }
            // Ghostty.app 의 전역 단축키(빠른 터미널 등)는 보드 터미널과 상관없다
            if parts.count == 2, parts[0] == "keybind", parts[1].hasPrefix("global:") { continue }
            lines.append(line)
        }
    }
    lines.append("keybind = shift+enter=text:\\x1b\\r")   // Shift+Enter = 사용자 Ghostty 와 같은 바이트(1b 0d) — 기본값은 CSI 27;2;13~
    lines.append("keybind = super+q=unbind")              // ⌘Q 는 보드 종료로 — 엔진이 받으면 처리할 곳이 없어 삼킨다
    lines.append("keybind = super+t=unbind")              // ⌘T 는 보드의 새 세션(윈도우 메뉴)으로 — 엔진의 new_tab 도 처리할 곳이 없어 삼킨다
    // ⌘1~⌘9 는 보드의 탭 고르기(윈도우 메뉴)로 — 엔진 기본값은 같은 키를 글자(super+1)와 물리 키(super+digit_1) 두 가지로 goto_tab·last_tab 에 묶는다
    for n in 1...9 { lines.append("keybind = super+\(n)=unbind"); lines.append("keybind = super+digit_\(n)=unbind") }
    // 화면 나누기 키(Ghostty 와 같은 키)와 Ghostty 의 탭 옮기기 키는 보드의 윈도우 메뉴(화면 나누기·탭으로 가기)로 — 엔진 기본값은 new_split·
    // goto_split·resize_split·equalize_splits·toggle_split_zoom·previous_tab·next_tab 에 묶는데 래퍼에 받는 곳이 없다(보드가 화면 트리를 직접 쥔다).
    // ⌘W(close_surface)는 엔진이 받아 그 화면의 닫기를 청한다(terminalDidClose)
    for k in ["arrow_left", "arrow_right", "arrow_up", "arrow_down"] {
        lines.append("keybind = super+alt+\(k)=unbind")
        lines.append("keybind = super+ctrl+\(k)=unbind")
    }
    for k in ["super+d", "super+shift+d", "super+[", "super+]", "super+ctrl+=", "super+shift+enter",
              "super+shift+[", "super+shift+]", "ctrl+tab", "ctrl+shift+tab"] { lines.append("keybind = \(k)=unbind") }
    // ⌘=·⌘+·⌘- 는 보드의 터미널 글자 크기(윈도우 > 글자 크기), ⌘0 은 세션 보드 보기로 — 엔진 기본값은 이 넷을 글자 크기 키(increase·decrease·
    // reset_font_size)로 묶는다(Ghostty 1.3.1 +list-keybinds --default). "super+==unbind" 처럼 써도 +validate-config 를 통과한다
    for k in ["=", "+", "-", "0"] { lines.append("keybind = super+\(k)=unbind") }
    let dir = NSTemporaryDirectory() + (Bundle.main.bundleIdentifier ?? instanceName)
    try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
    let path = dir + "/board-terminal.conf"
    try? lines.joined(separator: "\n").write(toFile: path, atomically: true, encoding: .utf8)
    let c = TerminalController(configSource: .file(path), theme: terminalTheme(UserDefaults.standard.string(forKey: "terminalTheme") ?? theme),
                               terminalConfiguration: terminalFontConfiguration())
    // 한 줄이라도 거부되면 파일 전체가 조용히 기본값으로 돌아간다 — 원인을 찾을 수 있게 남긴다
    if let issue = c.lastConfigurationIssue { boardLog("터미널 설정 거부됨: \(issue)") }
    return c
}

// 사용자 Ghostty 설정 파일 — Ghostty 가 읽는 순서(뒤의 파일이 앞을 덮는다)
let ghosttyConfigFiles = ["\(NSHomeDirectory())/.config/ghostty/config", "\(NSHomeDirectory())/.config/ghostty/config.ghostty",
                          "\(NSHomeDirectory())/Library/Application Support/com.mitchellh.ghostty/config",
                          "\(NSHomeDirectory())/Library/Application Support/com.mitchellh.ghostty/config.ghostty"]

// 사용자 Ghostty 설정에서 그 키의 마지막 값(따옴표는 뗀다) — 없으면 nil
func ghosttyConfigValue(_ key: String) -> String? {
    var value: String?
    for f in ghosttyConfigFiles {
        guard let text = try? String(contentsOfFile: f, encoding: .utf8) else { continue }
        for line in text.components(separatedBy: .newlines) {
            let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if parts.count == 2, parts[0] == key { value = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "\"")) }
        }
    }
    return value
}

// 사용자 Ghostty 설정의 글자 크기(마지막 font-size 줄) — 없으면 엔진 기본(macOS 13). 터미널 글자 크기를 처음 바꿀 때의 기준
func ghosttyConfigFontSize() -> Double { ghosttyConfigValue("font-size").flatMap(Double.init) ?? 13 }

// 나눈 화면 가운데 고르지 않은 화면의 불투명도 — Ghostty 의 unfocused-split-opacity(기본 0.7, 0.15~1)
let unfocusedSplitOpacity = CGFloat(min(1, max(0.15, ghosttyConfigValue("unfocused-split-opacity").flatMap(Double.init) ?? 0.7)))

// 나눈 화면 사이 경계 색 — Ghostty 의 split-divider-color(#RRGGBB). 없으면 Ghostty 처럼 터미널 바탕색을 어둡게
func splitDividerColor(background: NSColor) -> NSColor {
    if let v = ghosttyConfigValue("split-divider-color"), let n = UInt32(v.replacingOccurrences(of: "#", with: ""), radix: 16), v.count >= 6 {
        return NSColor(red: CGFloat(n >> 16 & 0xFF) / 255, green: CGFloat(n >> 8 & 0xFF) / 255, blue: CGFloat(n & 0xFF) / 255, alpha: 1)
    }
    return background.blended(withFraction: 0.4, of: .black) ?? background
}

// 보드가 정한 터미널 글자 크기(보드 설정 terminalFontSize) — 래퍼가 사용자 설정 → 이것 → 테마 순으로 한 설정을 만들어 모든 탭에 준다.
// 없으면 빈 설정(사용자 Ghostty 설정 크기)
func terminalFontConfiguration() -> TerminalConfiguration {
    guard let size = UserDefaults.standard.object(forKey: "terminalFontSize") as? Double else { return TerminalConfiguration() }
    return TerminalConfiguration().fontSize(Float(size))
}

// ~/.claude/notifier/board.log 에 한 줄 남긴다 — 보드는 그대로 동작해야 하는 문제(단축키 등록 실패 등)를 적는다
func boardLog(_ message: String) {
    if let h = FileHandle(forWritingAtPath: NSHomeDirectory() + "/.claude/notifier/board.log") {
        h.seekToEndOfFile(); h.write("\(Date()) \(message)\n".data(using: .utf8)!); h.closeFile()
    }
}

// theme 값: "이름" 또는 "light:이름,dark:이름" → 엔진 목록의 라이트·다크 테마(한쪽만 있으면 둘 다 그것). 목록에 없는 이름이면 nil
func themeDefinitions(_ spec: String?) -> (light: GhosttyThemeDefinition, dark: GhosttyThemeDefinition)? {
    guard let spec else { return nil }
    var light = spec, dark = spec
    for part in spec.split(separator: ",") {
        let kv = part.split(separator: ":", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
        guard kv.count == 2 else { continue }
        if kv[0] == "light" { light = kv[1] } else if kv[0] == "dark" { dark = kv[1] }
    }
    guard let d = GhosttyThemeCatalog.theme(named: dark) ?? GhosttyThemeCatalog.theme(named: light) else { return nil }
    return (GhosttyThemeCatalog.theme(named: light) ?? d, d)
}

// 엔진에 넘길 테마 — 목록에 없는 이름이면 래퍼 기본(보드 창은 어두우므로 Afterglow)
func terminalTheme(_ spec: String?) -> TerminalTheme {
    guard let t = themeDefinitions(spec) else { return .default }
    return TerminalTheme(light: t.light.toTerminalConfiguration(), dark: t.dark.toTerminalConfiguration())
}

// "Ghostty 설정 따르기" 일 때 보이는 테마 이름 — 보드 창은 늘 다크라 Ghostty 설정 theme 의 다크 쪽. 없거나 목록에 없으면 래퍼 기본
func ghosttyThemeName() -> String { themeDefinitions(ghosttyConfigValue("theme"))?.dark.name ?? "기본 테마(Afterglow)" }

// 밝은 바탕인가 — 엔진의 GhosttyThemeDefinition.isDark 와 같은 휘도 식(0.299R+0.587G+0.114B 가 0.5 이상이면 밝음)
func isLightColor(_ c: NSColor) -> Bool { 0.299 * c.redComponent + 0.587 * c.greenComponent + 0.114 * c.blueComponent >= 0.5 }

// 보드 터미널 화면 하나(엔진 surface 하나) — 탭은 화면 하나 이상을 나눠(split) 보인다(Ghostty 처럼). 셸·claude 명령·세션 짝짓기는 화면 단위다
final class BoardSurface: NSObject, ObservableObject, Identifiable, TerminalSurfaceTitleDelegate, TerminalSurfaceCloseDelegate,
                          TerminalSurfacePwdDelegate, TerminalSurfaceClipboardConfirmationDelegate, TerminalSurfaceLifecycleDelegate {
    let id = UUID()
    let view: BoardTerminalView
    weak var tab: BoardTab?         // 이 화면이 든 탭
    // 화면은 모두 로그인 셸이다. claude 화면은 보드가 그 셸에 claude 명령을 쳐 넣은 화면 — claude 가 끝나면 셸 프롬프트가 남는다(Ghostty 처럼)
    var command: String?            // 보드가 셸에 넣은 claude 명령 — nil 이면 셸 화면(claude 가 끝나 셸만 남은 화면도)
    var sessionId: String?          // 이 화면의 세션 id(새 세션의 --session-id, 이어 연 세션, tty 로 짝지은 지금 세션) — 끝난 뒤에도 남는다
    var mdRenderOff = false         // 답변 꾸미기를 끄고 띄운 claude 화면 — 이 화면의 세션 id 는 모두 끈 세션으로 기억한다
    var held: [String: String]?     // 다시 뜰 때 셸만 띄우고 들어갈 때 이어 열 claude 화면의 남겨 둔 값(TerminalHub.resumeHeld) — 그때까지 쥔 목록에 그대로 남긴다
    @Published var title = ""
    @Published var cwd: String
    @Published var session: Session?   // 이 화면에서 도는 세션(세션 파일의 tty 로 짝짓는다) — 탭의 상태 점·제목
    // 이 화면의 claude 가 대화를 백그라운드 작업으로 넘겨 그 작업을 붙여 보여 주는 중이면 이어 받은 세션(TerminalHub.pair, parkedJobId).
    // 탭의 상태 점·지금 하는 일만 이것을 따른다 — 키 넣기·이름 바꾸기·탭 이름·쥔 목록은 session(화면의 claude) 그대로
    @Published var parked: Session?
    @Published var renameTo: String?   // ⇧⌘R 로 바꿀 세션 이름 — /rename 으로 Claude Code 이름이 그렇게 될 때까지(TerminalHub.applyRenames)
    private var shellReady = false     // 셸이 첫 프롬프트를 띄웠다(셸 통합이 경로·제목을 알림)
    private var pending: (() -> String)?   // 셸이 뜨면 넣을 명령 — 넣을 때 만든다(세션 이름은 그때 쓰는 이름을 보고 고른다)
    private var typedAt = Date.distantPast

    init(_ options: TerminalSurfaceOptions, frame: NSRect, controller: TerminalController) {
        cwd = options.workingDirectory ?? NSHomeDirectory()
        view = BoardTerminalView(frame: frame)
        super.init()
        view.delegate = self   // 약한 참조 — 탭이 화면을 쥔다
        view.controller = controller
        view.configuration = options
        view.onFocus = { [weak self] in if let self { TerminalHub.shared.surfaceFocused(self) } }
    }

    var folderName: String { cwd == NSHomeDirectory() ? "~" : (cwd as NSString).lastPathComponent }
    // 세션 파일의 tty 와 같은 꼴(/dev/ 를 뗀 이름)
    var tty: String { (view.ttyName ?? "").replacingOccurrences(of: "/dev/", with: "") }

    // 짝짓기(BoardRules pairTabs)에 넘길 지금 값 — 맨 위 프로세스는 앞 프로세스의 세션 리더(엔진이 띄운 login), 앞 프로세스가 없거나 못 읽으면 0
    func screen(in workspace: String) -> TabScreen {
        var leader: Int32 = 0
        if let fg = view.foregroundPid, fg > 0 { let sid = getsid(fg); if sid > 0 { leader = sid } }
        return TabScreen(id: id, workspace: workspace, tty: tty, leader: leader, sessionId: sessionId, command: command, mdRenderOff: mdRenderOff)
    }

    // 셸 통합(엔진 리소스)이 첫 프롬프트에서 경로(OSC 7)·제목을 알린다 — 그때 셸이 입력을 받을 준비가 됐다
    func terminalDidChangeTitle(_ title: String) { self.title = title; shellStarted() }
    func terminalDidChangeWorkingDirectory(_ path: String) { cwd = path; shellStarted() }
    func terminalDidDetachSurface() {}

    // 경로·제목을 알리지 않는 셸(셸 통합이 없는 셸)에는 5초 뒤 미리 쳐 둔다 — 셸이 뜨면 쌓인 입력을 읽는다
    func terminalDidAttachSurface(_ surface: TerminalSurface) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in self?.shellStarted() }
    }

    private func shellStarted() {
        guard !shellReady else { return }
        shellReady = true
        sendPending(tries: 30)
    }

    // 셸에 명령을 사람이 친 것처럼 넣는다(붙여넣기 + Return — 셸 기록에 남아 ↑ 로 보인다). 셸이 아직 뜨지 않았으면 뜬 뒤에
    func type(_ command: @escaping () -> String) {
        pending = command
        if shellReady { sendPending(tries: 30) }
    }

    // 셸의 줄 편집기가 입력을 읽기 시작하면(tty 의 정규 모드·되울림이 꺼짐) 넣는다 — 경로를 알린 직후(precmd)에 넣으면 tty 가
    // 글자를 프롬프트 위에 한 번 더 찍는다(실측: 그때 정규 모드는 꺼졌지만 되울림은 켜져 있었다). 3초를 기다려도 그대로면 그냥 넣는다(셸이 뜨면 쌓인 입력을 읽는다)
    private func sendPending(tries: Int) {
        guard let make = pending else { return }
        guard tries <= 0 || lineEditing else {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in self?.sendPending(tries: tries - 1) }
            return
        }
        pending = nil
        typedAt = Date()
        drive([("text", make()), ("wait", "0.1"), ("key", "enter")]) { _ in }
    }

    private var lineEditing: Bool {
        let fd = Darwin.open(view.ttyName ?? "", O_RDONLY | O_NOCTTY | O_NONBLOCK)
        guard fd >= 0 else { return true }   // tty 를 못 열면 기다리지 않는다
        defer { Darwin.close(fd) }
        var t = termios()
        return tcgetattr(fd, &t) != 0 || t.c_lflag & tcflag_t(ICANON | ECHO) == 0
    }

    // 보드가 넣은 claude 가 셸이 뜨기를 기다리거나 막 넣었거나(3초) 앞에서 돌고 있다
    var runsClaude: Bool {
        pending != nil || Date().timeIntervalSince(typedAt) < 3 || view.foregroundPid.map(isLiveClaude) == true
    }

    // 셸이 프롬프트에 있다(셸 위에 도는 프로그램이 없다) — 앞 프로세스 그룹이 셸 자신(엔진이 띄운 login 의 자식)이면
    var atPrompt: Bool {
        guard let fg = view.foregroundPid, fg > 0 else { return false }
        let leader = getsid(fg)
        return leader > 0 && fg != leader && parentPid(fg) == leader
    }

    // pid 가 화면의 앞(포그라운드) 프로그램인가 — 그 프로세스 그룹이 tty 의 포그라운드 그룹이면. 셸에서 띄운 claude 는 제 작업 그룹이
    // 포그라운드다. 셸 화면에서 & 로 띄운 claude -p 는 아니다(키를 넣으면 셸이 받는다)
    func isForeground(_ pid: Int32) -> Bool {
        guard pid > 0, let fg = view.foregroundPid, fg > 0 else { return false }
        return getpgid(pid) == fg
    }

    // 화면의 프로그램에 키·글자를 넣는다(driveSession 의 단계) — 키는 키 경로, 글자는 붙여넣기
    func drive(_ steps: [(String, String)], done: @escaping @MainActor (Bool) -> Void) {
        guard let (kind, value) = steps.first else { return done(true) }
        let rest = Array(steps.dropFirst())
        switch kind {
        case "key": _ = view.sendKey(value == "escape" ? .escape : .enter)
        case "text": _ = view.paste(text: value)
        default:
            DispatchQueue.main.asyncAfter(deadline: .now() + (Double(value) ?? 0)) { self.drive(rest, done: done) }
            return
        }
        drive(rest, done: done)
    }

    // 셸이 끝났거나(processAlive false) ⌘W 처럼 엔진이 닫기를 청했을 때
    func terminalDidClose(processAlive: Bool) { TerminalHub.shared.close(self, confirm: processAlive) }

    // 붙여넣기 보호(줄바꿈이 든 내용을 bracketed paste 가 꺼진 프로그램에)·프로그램의 클립보드 읽기(OSC 52)를 묻는다 —
    // 처리기가 없으면 붙여넣기는 알림 없이 버려진다
    func terminalDidRequestClipboardConfirmation(_ request: TerminalClipboardConfirmationRequest) {
        let a = NSAlert()
        switch request.kind {
        case .paste:
            a.messageText = "여러 줄을 붙여넣을까요?"
            a.informativeText = "줄바꿈이 들어 있어 터미널의 프로그램이 줄마다 바로 실행할 수 있습니다.\n\n" + String(request.contents.prefix(400))
            a.addButton(withTitle: "붙여넣기")
        case .osc52Read:
            a.messageText = "터미널의 프로그램이 클립보드를 읽으려고 합니다"
            a.informativeText = "허용하면 지금 클립보드 내용이 그 프로그램에 전달됩니다."
            a.addButton(withTitle: "허용")
        case .osc52Write:
            a.messageText = "터미널의 프로그램이 클립보드에 쓰려고 합니다"
            a.informativeText = String(request.contents.prefix(400))
            a.addButton(withTitle: "허용")
        }
        a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"   // NSAlert 는 영어 "Cancel" 에만 Esc 를 붙인다
        MainActor.assumeIsolated { whenPaneSettled { a.beginSheetModal(for: window) { request.respond(allow: $0 == .alertFirstButtonReturn) } } }
    }
}

enum SplitDirection { case left, right, up, down }

// 탭의 화면 나누기 트리(Ghostty 의 SplitTree 와 같은 꼴) — 잎은 화면 하나, 가지는 둘로 나눈 것: right 면 왼쪽|오른쪽(⌘D), 아니면 위/아래(⇧⌘D),
// ratio 는 앞쪽(왼쪽·위)의 몫. 경계를 끌거나 ⌃⌘방향키로 ratio 가 바뀐다. 나누기·닫기는 마디를 그 자리에서 바꾼다(부모가 쥔 마디 그대로)
final class SplitNode {
    private(set) var surface: BoardSurface?       // 잎이면 그 화면
    private(set) var children: [SplitNode] = []   // 가지면 [앞, 뒤]
    private(set) var right = true
    var ratio: CGFloat = 0.5
    private(set) weak var parent: SplitNode?

    init(_ surface: BoardSurface) { self.surface = surface }
    init(right: Bool, ratio: CGFloat, _ a: SplitNode, _ b: SplitNode) {
        self.right = right
        self.ratio = ratio
        children = [a, b]
        a.parent = self
        b.parent = self
    }

    // 화면들 — 트리 순서(왼쪽·위가 앞). ⌘[·⌘] 가 이 순서로 돈다
    var surfaces: [BoardSurface] { surface.map { [$0] } ?? children.flatMap(\.surfaces) }

    func leaf(_ s: BoardSurface) -> SplitNode? { surface === s ? self : children.lazy.compactMap { $0.leaf(s) }.first }

    // 이 잎을 둘로 — 앞은 지금 화면, 뒤(오른쪽·아래)는 새 화면, 50:50
    func split(_ s: BoardSurface, right: Bool) {
        guard let old = surface else { return }
        surface = nil
        self.right = right
        ratio = 0.5
        children = [SplitNode(old), SplitNode(s)]
        children.forEach { $0.parent = self }
    }

    // 이 잎을 빼고 부모 자리를 형제가 차지한다. 돌려주는 값은 그다음 고를 화면 — 형제 쪽에서 뺀 화면에 가장 가까운 것
    // (앞이 빠지면 형제의 첫 화면, 뒤가 빠지면 형제의 마지막 화면). 탭의 마지막 화면(부모 없음)이면 nil
    func remove() -> BoardSurface? {
        guard let p = parent, let i = p.children.firstIndex(where: { $0 === self }) else { return nil }
        let sib = p.children[1 - i]
        p.surface = sib.surface
        p.right = sib.right
        p.ratio = sib.ratio
        p.children = sib.children
        p.children.forEach { $0.parent = p }
        return i == 0 ? p.surfaces.first : p.surfaces.last
    }

    // ⌃⌘= — 몫을 같은 방향으로 나눈 화면 수대로(Ghostty 의 equalize_splits 와 같은 무게)
    func equalize() {
        guard children.count == 2 else { return }
        let a = children[0].weight(right), b = children[1].weight(right)
        ratio = CGFloat(a) / CGFloat(a + b)
        children.forEach { $0.equalize() }
    }
    private func weight(_ dir: Bool) -> Int { children.count == 2 && right == dir ? children[0].weight(dir) + children[1].weight(dir) : 1 }

    // 화면마다 자리와 가지마다 (가지 자리, 경계 선 자리) — 경계 선 1pt 를 빼고 몫대로 나눈다. 좌표는 AppKit(아래가 0)
    func layout(in r: NSRect, _ frames: inout [(BoardSurface, NSRect)], _ lines: inout [(SplitNode, NSRect, NSRect)]) {
        if let surface { frames.append((surface, r)); return }
        guard children.count == 2 else { return }
        if right {
            let w = max(0, ((r.width - 1) * ratio).rounded())
            children[0].layout(in: NSRect(x: r.minX, y: r.minY, width: w, height: r.height), &frames, &lines)
            lines.append((self, r, NSRect(x: r.minX + w, y: r.minY, width: 1, height: r.height)))
            children[1].layout(in: NSRect(x: r.minX + w + 1, y: r.minY, width: max(0, r.width - w - 1), height: r.height), &frames, &lines)
        } else {
            let h = max(0, ((r.height - 1) * ratio).rounded())
            children[0].layout(in: NSRect(x: r.minX, y: r.maxY - h, width: r.width, height: h), &frames, &lines)
            lines.append((self, r, NSRect(x: r.minX, y: r.maxY - h - 1, width: r.width, height: 1)))
            children[1].layout(in: NSRect(x: r.minX, y: r.minY, width: r.width, height: max(0, r.height - h - 1)), &frames, &lines)
        }
    }

    // 몫을 바꾼다 — 양쪽 화면이 80pt 아래로 줄지 않게(length 는 이 가지의 폭·높이)
    func setRatio(_ value: CGFloat, length: CGFloat) {
        let m = min(0.5, 80 / max(length, 1))
        ratio = min(max(value, m), 1 - m)
    }
}

// 보드 터미널 탭 하나 — 화면 나누기 트리 하나와 그 가운데 고른 화면. 탭 이름·⌘ 번호·경로 줄·카드 강조는 탭 단위다
@MainActor final class BoardTab: ObservableObject, Identifiable {
    let id = UUID()
    // 탭의 작업 경로(경로 줄의 묶음, workspaceKey 꼴) — 탭을 연 경로(이어 열면 세션 기록의 project_dir, 없으면 cwd). 셸에서 cd 해도 바뀌지 않는다(cwd 는 따라간다)
    let workspace: String
    let root: SplitNode
    @Published var focused: BoardSurface   // 고른 화면 — 키 입력·탭의 "지금 하는 일"·⌘D 로 나눌 화면
    @Published var zoomed = false          // 고른 화면 하나 크게 보기(⇧⌘↩) — 나머지 화면은 숨긴다
    @Published var customName: String? // 사용자가 붙인 탭 이름(⇧⌘R) — 없으면 폴더 이름. 쥔 탭 목록에 남아 다시 떠도 그대로
    @Published var renaming = false    // 탭 이름 칸을 고치는 중
    private var watching: [ObjectIdentifier: AnyCancellable] = [:]

    init(_ root: SplitNode, focused: BoardSurface? = nil, workspace: String) {
        self.root = root
        self.focused = focused ?? root.surfaces[0]
        self.workspace = workspaceKey(workspace)
        root.surfaces.forEach(watch)
    }

    var surfaces: [BoardSurface] { root.surfaces }

    // 화면의 제목·경로·세션이 바뀌면 탭 줄도 다시 그린다
    func watch(_ s: BoardSurface) {
        s.tab = self
        watching[ObjectIdentifier(s)] = s.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
    }
    func unwatch(_ s: BoardSurface) { watching[ObjectIdentifier(s)] = nil }

    // 탭 이름 — 사용자가 붙인 이름, 없으면 폴더 이름: 화면이 하나면 그 화면 셸의 경로(cd 를 따라감, 예전 그대로),
    // 나눈 탭이면 탭을 연 경로로 고정(고른 화면이 다른 폴더여도 그대로 — 사용자 결정)
    var name: String { customName ?? folderName }
    // 탭 줄에 보이는 이름 — 화면이 하나이고 그 화면에 claude 세션이 돌면 그 세션의 Claude Code 이름(번호가 붙었거나 /rename 한 이름도
    // 그대로 — 탭과 세션 이름이 같은 글자로 보이게. 이름 바꾸기를 미뤄 둔 동안은 바꿀 이름), 아니면 name. 나눈 탭은 name 그대로
    var shownName: String { surfaces.count == 1 ? focused.renameTo ?? focused.session?.name ?? name : name }
    var folderName: String {
        guard surfaces.count > 1 else { return focused.folderName }
        return workspace == NSHomeDirectory() ? "~" : (workspace as NSString).lastPathComponent
    }
    // 탭 줄의 상태 점 — 화면들의 세션(넘긴 작업을 붙여 보는 화면은 그 작업) 가운데 가장 급한 상태(세션이 없으면 대기)
    var state: String { surfaces.compactMap { $0.parked ?? $0.session }.min { $0.rank < $1.rank }?.state ?? "idle" }
}

@MainActor final class TerminalHub: ObservableObject {
    static let shared = TerminalHub()
    @Published private(set) var tabs: [BoardTab] = []   // 같은 경로의 탭끼리 붙여 둔다(openShell)
    @Published private(set) var selected: UUID?
    // 지금 경로 — 고른 탭의 작업 경로(select). 탭 줄은 이 경로의 탭만 보인다
    @Published private(set) var workspace: String?
    private var lastTab: [String: UUID] = [:]   // 경로마다 마지막에 고른 탭 — 경로를 바꾸면 그 탭으로
    // 경로 줄의 상태 점 — 경로마다 그 탭들의 세션 가운데 가장 급한 상태(짝지을 때 고친다)
    @Published private(set) var workspaceStates: [String: String] = [:]
    // 펼침 — 기본값은 펼침
    @Published private(set) var expanded = UserDefaults.standard.object(forKey: "terminalExpanded") as? Bool ?? true
    // 펼쳤을 때 보드 폭 — 경계를 끌어 바꾼 폭을 기억한다
    var boardWidth = CGFloat(UserDefaults.standard.object(forKey: "boardPaneWidth") as? Double ?? Double(boardLaunchWidth))
    // 보드 위치 — 화면 오른쪽(기본)·왼쪽(보드 설정 boardSide). 왼쪽이면 펼친 배치가 [보드 | 경계 | 터미널] 이고
    // 접기·최대축소·처음 크기로 붙이기도 화면 왼쪽 기준이다
    @Published private(set) var boardOnLeft = UserDefaults.standard.string(forKey: "boardSide") == "left"
    // 펼치기·접기 애니메이션이 도는 중이면 그 애니메이션 — 그동안 BoardRootView 가 터미널 영역 크기를 고정한다
    var slide: PaneSlide?
    var tabBarEnd: CGFloat = .infinity   // 탭 줄에서 탭·"새 세션" 단추가 끝나는 x — 그 오른쪽이 빈 곳
    var workspaceBarEnd: CGFloat = .infinity   // 경로 줄에서 경로 항목이 끝나는 x — 그 오른쪽이 빈 곳
    // ⌘ 를 누르고 있다(앱 안 flagsChanged 감시) — 그동안 탭 줄에 ⌘번호와 이름 바꾸기 단축키를 보인다. 앱이 비활성이 되면 끈다
    @Published private(set) var commandHeld = false
    weak var area: TerminalAreaView?
    weak var store: Store?
    // 세션 id → 그 세션이 도는 보드 탭(tty 로 짝지은 세션, 탭 안에서 시작된 claude -p) — 세션 목록을 다시 읽을 때마다 고친다
    @Published private(set) var paired: [String: UUID] = [:]
    @Published private(set) var tabOwnedHeadless: Set<String> = []
    // 탭 claude 가 넘긴 대화를 이어 받은 백그라운드 작업의 세션 id → 그 화면(BoardSurface.parked), 넘긴 쪽(탭 claude)의 세션 id.
    // paired 에 넣지 않는다 — place 를 쓰는 동작(일시중지·중단·키 넣기·종료 확인)은 그 작업을 지금처럼 백그라운드 세션으로 다룬다
    @Published private(set) var parkedTabs: [String: UUID] = [:]
    private(set) var parkedFrom: Set<String> = []
    // 다른 터미널 세션의 앱 이름(칩) — 세션마다 한 번 조상 프로세스로 찾는다(AppleScript 없이)
    @Published private(set) var appNames: [String: String] = [:]
    private var modes: [Int32: String] = [:]   // 백그라운드 세션 pid → 실행 방식(-p, --bg …)
    // 보드 아래쪽에 잠깐 뜨는 안내 — 백그라운드 세션을 눌렀을 때 등
    @Published private(set) var notice: String?
    private var noticeHide: DispatchWorkItem?
    // 고른 탭·터미널 영역 바탕 — 엔진을 띄운 뒤에는 터미널 테마의 바탕색(탭이 터미널과 이어져 보이게)
    @Published private(set) var background = terminalAreaColor
    private var dividerColor = NSColor.gray   // 나눈 화면 사이 경계 색 — 엔진을 띄울 때 정한다
    private var engine: TerminalController?

    // 엔진은 첫 탭을 열 때 띄운다
    var controller: TerminalController {
        if let engine { return engine }
        let c = makeTerminalController()
        engine = c
        // 엔진은 라이트로 시작하고 화면이 창에 붙을 때에야 창 외관(늘 다크)을 따른다 — 바탕색을 읽기 전에 다크로 맞춘다.
        // 안 그러면 라이트·다크 바탕이 다른 테마(theme 줄이 없을 때의 래퍼 기본 등)에서 탭·경로 줄만 라이트 바탕으로 남는다
        c.setColorScheme(.dark)
        readBackground(c)
        return c
    }

    // 고른 탭·경로 칸·터미널 칸 바탕과 나눈 화면 경계 색을 엔진의 지금 바탕색으로 — 엔진을 띄울 때, 테마를 바꿀 때
    private func readBackground(_ c: TerminalController) {
        let bg = c.backgroundColor
        background = NSColor(red: CGFloat(bg.red) / 255, green: CGFloat(bg.green) / 255, blue: CGFloat(bg.blue) / 255, alpha: 1)
        dividerColor = splitDividerColor(background: background)
        area?.container.layer?.backgroundColor = background.cgColor
    }

    func setExpanded(_ on: Bool) {
        expanded = on
        UserDefaults.standard.set(on, forKey: "terminalExpanded")
    }

    // 터미널 글자 크기(윈도우 > 글자 크기, ⌘+·⌘-) — step +1·-1 은 1pt씩(Ghostty 와 같음, 8~40), nil 은 원래 크기(보드 설정을 지워 Ghostty 설정 크기로).
    // 열린 탭 모두 바로 바뀐다 — 엔진 설정을 다시 읽히면 글자 크기를 손대지 않은 surface 는 설정 크기를 따른다(탭마다 키로 바꾸는 엔진 단축키는 풀어 두었다).
    // 새로 여는 탭·다시 뜰 때 되살린 탭도 같은 설정이다
    func changeTerminalFont(_ step: Int?) {
        let d = UserDefaults.standard
        if let step {
            let now = d.object(forKey: "terminalFontSize") as? Double ?? ghosttyConfigFontSize()
            let next = min(40, max(8, now + Double(step)))
            // Ghostty 설정 크기로 돌아왔으면 설정을 지운다 — 원래 크기와 같은데 바꾼 것으로 남지 않게(설정 화면의 되돌리기 단추)
            if next == ghosttyConfigFontSize() { d.removeObject(forKey: "terminalFontSize") } else { d.set(next, forKey: "terminalFontSize") }
        } else {
            d.removeObject(forKey: "terminalFontSize")
        }
        engine?.setTerminalConfiguration(terminalFontConfiguration())   // 사용자 설정·테마는 래퍼가 그대로 다시 얹는다
        toast("터미널 글자 \(String(format: "%g", d.object(forKey: "terminalFontSize") as? Double ?? ghosttyConfigFontSize()))pt")
    }

    // 터미널 테마(설정 > 터미널 > 테마) — 보드 설정 terminalTheme(테마 이름). nil 은 지워 Ghostty 설정의 theme 을 따른다.
    // 열린 탭 모두 바로 바뀌고 고른 탭·경로 칸 바탕·경계 색도 다시 읽는다. 엔진을 아직 띄우지 않았으면 설정만 남아 처음 띄울 때 쓴다
    func setTerminalTheme(_ name: String?) {
        let d = UserDefaults.standard
        if let name { d.set(name, forKey: "terminalTheme") } else { d.removeObject(forKey: "terminalTheme") }
        if let engine {
            engine.setTheme(terminalTheme(name ?? ghosttyConfigValue("theme")))
            readBackground(engine)
            layoutSurfaces()   // 나눈 화면 경계 색
        }
        toast("터미널 테마 \(name ?? "Ghostty 설정 따르기(\(ghosttyThemeName()))")")
    }

    func setBoardOnLeft(_ on: Bool) {
        boardOnLeft = on
        UserDefaults.standard.set(on ? "left" : "right", forKey: "boardSide")
    }

    // 셸 탭 — 엔진이 사용자의 로그인 셸(passwd 의 셸)을 login 으로 띄운다. login 이 환경을 스스로 꾸리므로 환경 변수를 넘기지 않는다.
    // 셸이 끝나면(exit) 엔진이 닫기를 청해 그 화면을 닫는다(terminalDidClose — 탭의 마지막 화면이면 탭째)
    @discardableResult
    func openShell(in dir: String) -> BoardTab? {
        guard area != nil else { return nil }
        let tab = BoardTab(SplitNode(makeSurface(in: dir)), workspace: dir)
        add(tab)
        return tab
    }

    // 화면 하나 — 그 폴더의 로그인 셸. 창에 붙일 때 셸이 뜬다
    private func makeSurface(in dir: String) -> BoardSurface {
        BoardSurface(TerminalSurfaceOptions(backend: .exec, workingDirectory: dir), frame: .zero, controller: controller)
    }

    // 탭을 탭 목록에 넣고 화면들을 창에 붙인 뒤 고른다 — 자리를 먼저 정하고 붙인다(크기가 0 인 뷰는 셸을 띄우지 않는다)
    private func add(_ tab: BoardTab) {
        guard let area else { return }
        area.workspaceBarShown = true             // 첫 탭이면 경로 줄이 생겨 터미널 칸이 줄어든다 — 탭을 만들기 전에 맞춘다
        area.superview?.layoutSubtreeIfNeeded()
        // 같은 경로 탭들의 끝에(새 경로면 맨 끝에) — 경로끼리 붙어 있어 경로 줄 순서가 앞 탭을 닫아도 그대로다
        if let last = tabs.lastIndex(where: { $0.workspace == tab.workspace }) { tabs.insert(tab, at: last + 1) } else { tabs.append(tab) }
        layoutSurfaces()
        tab.surfaces.forEach(attach)
        select(tab.id)
        saveTabs()
    }

    // 화면 뷰를 터미널 칸 맨 아래에(경계 뷰는 그 위) 숨긴 채 붙인다 — 보이기는 applyVisibility
    private func attach(_ s: BoardSurface) {
        s.view.isHidden = true
        area?.container.addSubview(s.view, positioned: .below, relativeTo: nil)
    }

    // 경로 줄의 경로들 — 탭 목록에 처음 나타난 순서
    var workspaces: [String] {
        var list: [String] = []
        for t in tabs where !list.contains(t.workspace) { list.append(t.workspace) }
        return list
    }

    func tabs(in ws: String?) -> [BoardTab] { tabs.filter { $0.workspace == ws } }

    // 경로에서 보일 탭 — 그 경로에서 마지막에 고른 탭(닫혔으면 첫 탭). 탭이 없으면 nil
    private func shownTab(_ ws: String) -> UUID? {
        if let id = lastTab[ws], tabs.contains(where: { $0.id == id }) { return id }
        return tabs.first { $0.workspace == ws }?.id
    }

    // 경로 줄 항목·보드 카드 머리를 눌렀을 때 — 그 경로에 탭이 있으면 그 경로로(마지막에 고른 탭). 터미널 포커스는 펼쳐 있을 때만(select).
    // 접혀 있으면 펼치지 않고 바꿔만 둔다 — 펼치면 그 경로가 보인다
    func showWorkspace(_ path: String) {
        if let id = shownTab(workspaceKey(path)) { select(id) }
    }

    // ⌃⌥↑·⌃⌥↓ — 경로 줄의 앞·뒤 경로로(끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다)
    func stepWorkspace(_ step: Int) {
        let list = workspaces
        guard !list.isEmpty else { return }
        let i = workspace.flatMap { list.firstIndex(of: $0) } ?? 0
        if let id = shownTab(list[(i + step + list.count) % list.count]) { reveal(id) }
    }

    // 명령(claude …)을 보드 새 탭에서 띄운다 — 그 작업 경로의 셸 탭을 열고 셸에 명령을 쳐 넣는다. claude 가 끝나면 그 셸 프롬프트로 돌아온다.
    // 셸이 띄우므로 프로세스 이름이 claude 그대로(hook 이 알아봄)이고 셸 설정의 PATH 등을 그대로 쓴다.
    // mdRenderOff — 답변 꾸미기를 끈 탭(명령은 부르는 쪽이 mdRenderCommand 로 고쳐 넘긴다). 그 세션 id 를 끈 세션으로 기억한다
    func launch(_ command: String, in dir: String, sessionId: String?, mdRenderOff: Bool = false) {
        guard let tab = openShell(in: dir) else { return }
        run(command, in: tab.focused, sessionId: sessionId, mdRenderOff: mdRenderOff)
        reveal(tab.id)
    }

    // 화면의 셸에 claude 명령을 넣고 그 화면을 claude 화면으로 삼는다(새 탭, 셸 프롬프트로 남은 화면에서 이어 열기, 다시 뜰 때 되살리기).
    // 세션 이름(--name)은 셸에 칠 때 붙인다 — 되살릴 때는 그때야 탭과 탭 이름이 생긴다. 쥔 탭 목록의 명령에는 남기지 않는다(되살릴 때 다시 고른다)
    func run(_ command: String, in s: BoardSurface, sessionId: String?, mdRenderOff: Bool) {
        s.held = nil
        s.command = command
        s.sessionId = sessionId
        s.mdRenderOff = mdRenderOff
        if mdRenderOff { rememberMdRenderOff(sessionId) }
        s.type { [weak self, weak s] in
            MainActor.assumeIsolated { s.flatMap { s in self?.named(command, in: s) } ?? command }
        }
        saveTabs()
    }

    // 세션 이름(사용자 결정 2026-10-20) — 보드 탭 claude 는 탭 이름(⇧⌘R 이름, 없으면 폴더 이름)을 세션 이름으로 쓰고, 그 이름을 쓰는 다른
    // 세션이 있으면 -2, -3 … 을 붙인다(나눈 탭의 화면들도 이렇게 나뉜다). Claude Code 는 같은 이름도 그대로 받으므로(2.1.304 실측) 보드가 고른다.
    // 탭 줄·카드 라벨은 고른 번호가 아니라 ~/.claude/sessions 의 이름을 보인다(탭 줄은 미뤄 둔 이름 바꾸기가 있으면 그 이름)
    private var reservedNames: [UUID: (name: String, at: Date)] = [:]   // 화면 → 방금 --name 으로 붙여 아직 파일이 없을 수 있는 이름(15초)
    // 화면 → 이름 바꾸기를 어디까지 쳤나(없으면 아직 안 침 — 바꿀 이름은 BoardSurface.renameTo): 치는 중, 글은 붙였고 Return 만 남음, Return 친 시각
    private enum RenameStep { case typing, typed, sent(Date) }
    private var renameSteps: [UUID: RenameStep] = [:]

    // 쓰는 이름 = 살아 있는 claude 의 이름 + 방금 붙인 이름 + 미뤄 둔 이름 바꾸기 + taken. 그 화면 자신의 것(지금 도는 claude 의 이름·제 몫)은
    // 빼고 본다. 대소문자는 가리지 않는다(Claude Code 는 가리지 않고 둘 다 받지만 세션끼리 부르는 주소라 헷갈리지 않게)
    private func pickName(_ base: String, for s: BoardSurface, taken: Set<String> = []) -> String {
        let own = s.session?.pid
        var used = Set(ClaudeNames.shared.live().filter { $0.key != own }.values.map { $0.lowercased() })
        for (id, r) in reservedNames where id != s.id && Date().timeIntervalSince(r.at) < 15 { used.insert(r.name.lowercased()) }
        for o in surfaces where o !== s { if let n = o.renameTo { used.insert(n.lowercased()) } }
        used.formUnion(taken.map { $0.lowercased() })
        var name = base, i = 2
        while used.contains(name.lowercased()) { name = "\(base)-\(i)"; i += 1 }
        return name
    }

    // 셸에 칠 claude 명령에 --name <이름> 을 claude 바로 뒤에 붙인다(셸에 치므로 shlexQuote — 한글·공백 이름). claude 가 아니거나
    // 사용자가 -n·--name 을 적었으면 그대로. --resume 과 함께 주면 --name 이 이긴다(실측 — 없으면 예전 대화 이름을 되살린다)
    private func named(_ command: String, in s: BoardSurface) -> String {
        guard let r = command.range(of: #"^\s*(\S*/)?claude(?=\s|$)"#, options: .regularExpression),
              command.range(of: #"(^|\s)(-n|--name)(=|\s|$)"#, options: .regularExpression) == nil else { return command }
        let name = pickName(s.tab?.name ?? s.folderName, for: s)
        reservedNames[s.id] = (name, Date())
        return command.replacingCharacters(in: r.upperBound..<r.upperBound, with: " --name " + shlexQuote(name))
    }

    // ⇧⌘R 로 바꾼 탭 이름을 그 탭의 claude 세션들에 — 화면마다 /rename <이름>(나눈 탭은 번호로 나뉜다, 사용자 결정). 셸만 남은 화면·
    // claude -p 는 빼고, 이미 그 이름이면 치지 않는다. 치는 것은 applyRenames
    private func renameSessions(_ tab: BoardTab) {
        tab.surfaces.forEach(clearRename)
        var taken: Set<String> = []
        for s in tab.surfaces {
            guard let ss = s.session, ss.state != "ended", s.isForeground(ss.pid), mode(ss) == nil else { continue }
            let target = pickName(tab.name, for: s, taken: taken)
            taken.insert(target)
            if ClaudeNames.shared.name(ss.pid) != target { s.renameTo = target }
        }
        applyRenames()
    }

    private func clearRename(_ s: BoardSurface) {
        if s.renameTo != nil { s.renameTo = nil }
        renameSteps[s.id] = nil
    }

    // 미뤄 둔 이름 바꾸기를 친다(⇧⌘R 직후, 그 뒤 세션 목록을 읽을 때마다). 승인 창·질문 창이 떠 있을 때 치면 Return 이 그 창에 들어가
    // 도구 실행을 허락해 버릴 수 있다 — 세션이 작업 중·대기일 때만 치고(승인 대기·일시중지는 미룬다), 글을 붙인 뒤 Return 직전에 지금
    // 파일로 한 번 더 본다. 그 사이 창이 떴으면 Return 만 남겨 두었다가 창이 닫히면 친다(빈 입력 칸의 Return 은 아무 일도 하지 않는다).
    // 요청 전 세션(대화 기록이 아직 없음)도 첫 요청이 들어갈 때까지 미룬다(사용자 결정) — /rename 이 대화 기록을 만들어, 요청 없이 끝나도
    // 목록에 "요청 전" 카드가 남는다. 미루는 동안 탭 줄은 바꿀 이름, 카드 라벨은 실제 이름(옛 이름)이다.
    // Return 을 친 뒤에는 Claude Code 이름이 바뀌면 끝, 5초가 지나도 그대로면 그만둔다(탭 줄이 실제 이름으로 돌아간다).
    // 사용자가 입력 칸에 쓰던 글이 있으면 섞일 수 있다(사용자가 알고 고름)
    func applyRenames() {
        for s in surfaces {
            guard let target = s.renameTo else { continue }
            guard let ss = s.session, ss.state != "ended", s.isForeground(ss.pid), ClaudeNames.shared.name(ss.pid) != target else {
                clearRename(s)   // 바뀌었거나 claude 가 끝났다
                continue
            }
            var typed = false
            switch renameSteps[s.id] {
            case .typing: continue
            case .sent(let at): if Date().timeIntervalSince(at) > 5 { clearRename(s) }; continue
            case .typed: typed = true
            case nil: break
            }
            // 대화를 백그라운드 작업으로 넘겨 그 작업을 붙여 보여 주는 화면은 친 글이 작업 쪽으로 갈 수 있어 풀릴 때까지 미룬다
            guard s.parked == nil, renameSafe(ss), hasTranscript(ss) else { continue }
            renameSteps[s.id] = .typing
            s.drive(typed ? [] : [("text", "/rename " + target), ("wait", "0.2")]) { [weak self] _ in
                guard let self, s.renameTo == target else { return }   // 그 사이 다시 바꿨거나 화면이 닫혔다
                guard let now = s.session, s.parked == nil, self.renameSafe(now) else { self.renameSteps[s.id] = .typed; return }
                s.drive([("key", "enter")]) { _ in }
                self.renameSteps[s.id] = .sent(Date())
            }
        }
    }

    // 요청이 들어간 세션인가 — 대화 기록 파일이 있다. Claude Code 는 첫 요청을 보낼 때 이 파일을 만든다(--name·/model 만으로는 생기지 않고
    // 첫 요청 0.1초 뒤 생김 — 2.1.304 실측). 보드 목록도 이 파일이 없는 끝난 세션은 지운다(Store.reload, 같은 경로 계산)
    private func hasTranscript(_ s: Session) -> Bool {
        FileManager.default.fileExists(atPath: transcriptPath(sessionRecord(s.id)?["transcript_path"] as? String, s.cwd, s.id))
    }

    // 이름 바꾸기를 쳐도 되는가 — 보드 상태가 작업 중·대기이고 승인 창·질문 창이 없다. 보드 상태는 1초마다 읽은 것이라 Claude Code 의
    // status(창을 띄우면 바로 waiting, Esc 로 닫으면 idle — 2.1.304 실측)를 지금 읽어 본다. status 가 없으면 hook 이 쓴 세션 파일의 상태를 지금
    private func renameSafe(_ s: Session) -> Bool {
        guard s.state == "working" || s.state == "idle" else { return false }
        if let status = ClaudeNames.shared.status(s.pid) { return status != "waiting" }
        return sessionRecord(s.id)?["state"] as? String != "waiting"
    }

    // 쥔 탭 목록 — 보드 설정 terminalTabs 에 탭마다 [workspace, name, selected, last] 와 화면 값 [cwd, command, mdoff, session, resume] 을 남긴다
    // (셸 화면은 경로만). 화면이 하나인 탭은 화면 값을 탭 항목에 바로 넣어 3.0.x 목록과 같은 꼴이고, 나눈 탭은 root 에 화면 트리
    // (가지 {split: right|down, ratio, a, b}, 잎은 화면 값 — 고른 화면에 focused)를 넣는다. 크게 보기는 남기지 않는다.
    // workspace 는 탭의 작업 경로, selected 는 고른 탭(곧 지금 경로), last 는 경로마다 마지막에 고른 탭.
    // 종료 때만이 아니라 바뀔 때마다(열기·닫기·나누기·경계 옮기기·화면 고르기·세션 id 정해짐·끝남, 1초마다 짝지을 때 견줘서) 남긴다 —
    // pkill·충돌로 꺼져도 다시 뜰 때 되살린다. claude 가 끝나 셸만 남은 화면은 셸 화면으로 남긴다. 되살리기 전에는 남기지 않는다(지난 목록을 빈 목록으로 덮지 않게)
    private var restored = false
    private var savedTabs: NSArray?

    func saveTabs() {
        guard restored else { return }
        let list = tabs.map { t -> [String: Any] in
            var e: [String: Any] = ["workspace": t.workspace]
            if let n = t.customName { e["name"] = n }
            if t.id == selected { e["selected"] = "1" }
            if lastTab[t.workspace] == t.id { e["last"] = "1" }
            if let s = t.root.surface { for (k, v) in entry(s) { e[k] = v } } else { e["root"] = savedNode(t.root, focused: t.focused) }
            return e
        } as NSArray
        guard !list.isEqual(savedTabs) else { return }
        savedTabs = list
        UserDefaults.standard.set(list, forKey: "terminalTabs")
    }

    // 화면 값 — 셸 화면은 경로만, claude 화면은 명령·세션까지. 들어가지 않아 아직 이어 열지 않은 화면은 남겨 둔 값 그대로
    private func entry(_ s: BoardSurface) -> [String: String] {
        if let held = s.held { return held }
        var e = ["cwd": s.cwd]
        if let command = s.command {
            e["command"] = command
            if s.mdRenderOff { e["mdoff"] = "1" }   // 대화 없이 꺼져 세션 파일·끈 세션 기억이 치워져도 끈 채로 되살린다
            // 화면 안에서 /clear 등으로 세션 id 가 바뀌면 tty 로 짝지은 세션이 지금 것이다
            if let id = s.session?.id ?? s.sessionId {
                e["session"] = id
                if let ss = s.session, ss.id == id, !ss.resumeCmd.isEmpty { e["resume"] = ss.resumeCmd }
            }
        }
        return e
    }

    private func savedNode(_ n: SplitNode, focused: BoardSurface) -> [String: Any] {
        if let s = n.surface {
            var e: [String: Any] = entry(s)
            if s === focused { e["focused"] = "1" }
            return e
        }
        return ["split": n.right ? "right" : "down", "ratio": Double(n.ratio),
                "a": savedNode(n.children[0], focused: focused), "b": savedNode(n.children[1], focused: focused)]
    }

    // 다시 뜰 때 — 지난번에 쥐고 있던 탭을 나눈 모양 그대로 되살린다. 셸 화면은 그 경로에서 셸, claude 화면도 우선 셸만 띄우고(held)
    // 그 탭에 들어갈 때 세션 파일의 resume_cmd 를 넣어 이어 연다(사용자 결정 2026-10-21) — 마지막에 고른 탭은 되살린 뒤 바로 고르므로 바로 열린다.
    // 작업 경로는 남겨 둔 것, 없으면(3.0.x 목록) 되살릴 때 여는 경로. 지금 경로·경로마다 마지막 탭도 되살린다. 보드가 뜰 때 한 번 부른다.
    // 업데이트처럼 보드를 끄고 바로 다시 띄우면 옛 탭의 claude 가 아직 끝나는 중일 수 있다 — 같은 세션을 두 프로세스가 쥐지 않게 최대 5초 기다린다
    func restoreTabs(waited: Double = 0) {
        guard !restored else { return }
        let saved = UserDefaults.standard.array(forKey: "terminalTabs") as? [[String: Any]] ?? []
        let running = saved.flatMap { leaves($0["root"] as? [String: Any] ?? $0) }.contains { e in
            (e["session"] as? String).flatMap(sessionRecord).flatMap { ($0["pid"] as? NSNumber)?.int32Value }.map(isLiveClaude) ?? false
        }
        if running, waited < 5 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { self.restoreTabs(waited: waited + 0.25) }
            return
        }
        var pick: UUID?
        var lasts: [BoardTab] = []
        for e in saved {
            var focused: BoardSurface?
            guard let (root, dir) = restoreNode(e["root"] as? [String: Any] ?? e, focused: &focused) else { continue }
            let tab = BoardTab(root, focused: focused, workspace: e["workspace"] as? String ?? dir)
            tab.customName = e["name"] as? String
            add(tab)
            if e["selected"] != nil { pick = tab.id }
            if e["last"] != nil { lasts.append(tab) }
        }
        restored = true
        // 탭을 열 때마다 고른 것은 지우고 남겨 둔 표시로 — 표시가 없는 경로(3.0.x 목록)는 첫 탭
        lastTab = [:]
        for t in lasts { lastTab[t.workspace] = t.id }
        if let id = pick ?? selected { select(id) }
        saveTabs()
    }

    // 남겨 둔 화면 트리의 잎(화면 값)들
    private func leaves(_ n: [String: Any]) -> [[String: Any]] {
        guard let a = n["a"] as? [String: Any], let b = n["b"] as? [String: Any] else { return [n] }
        return leaves(a) + leaves(b)
    }

    // 남겨 둔 화면 트리를 다시 만든다(3.0.x 목록의 탭 항목은 잎 하나) — 셸 화면은 그 경로(없으면 홈)에서 셸, claude 화면은 셸에 이어 열
    // 명령을 넣는다(restoreCommand). 되살리지 않는 화면은 빼고 형제가 그 자리를 차지한다. 돌려주는 경로는 첫 화면을 연 폴더 —
    // 작업 경로를 남기지 않은 3.0.x 목록에서는 이것이 탭의 경로다(명령 탭은 restoreCommand 의 경로, 셸 탭은 cwd 또는 홈)
    private func restoreNode(_ n: [String: Any], focused: inout BoardSurface?) -> (SplitNode, String)? {
        if let a = n["a"] as? [String: Any], let b = n["b"] as? [String: Any] {
            let ra = restoreNode(a, focused: &focused), rb = restoreNode(b, focused: &focused)
            guard let ra, let rb else { return ra ?? rb }
            return (SplitNode(right: n["split"] as? String != "down", ratio: CGFloat(n["ratio"] as? Double ?? 0.5), ra.0, rb.0), ra.1)
        }
        let e = n.compactMapValues { $0 as? String }
        guard let cwd = e["cwd"] else { return nil }
        let s: BoardSurface, dir: String
        if e["command"] != nil {
            let off = e["mdoff"] != nil || isMdRenderOff(e["session"])
            guard let (_, id, d) = restoreCommand(e, off: off) else { return nil }
            dir = d
            s = makeSurface(in: dir)
            s.held = e.filter { ["cwd", "command", "mdoff", "session", "resume"].contains($0.key) }
            s.sessionId = e["session"] ?? id   // 끝난 세션 줄을 누르면 이 화면에서 이어 연다(resumeSession)
        } else {
            dir = FileManager.default.fileExists(atPath: cwd) ? cwd : NSHomeDirectory()
            s = makeSurface(in: dir)
        }
        if e["focused"] != nil { focused = s }
        return (SplitNode(s), dir)
    }

    // 되살린 뒤 아직 들어가지 않은 화면의 claude 를 이어 연다 — 그 탭을 고를 때(select). 명령은 그때 다시 고른다:
    // 그사이 다른 터미널에서 그 세션을 이어 열었으면 restoreCommand 가 nil 이라 셸로 둔다
    private func resumeHeld(_ s: BoardSurface) {
        guard let e = s.held else { return }
        s.held = nil
        let off = e["mdoff"] != nil || isMdRenderOff(e["session"])
        if let (cmd, id, _) = restoreCommand(e, off: off) { run(cmd, in: s, sessionId: id, mdRenderOff: off) } else { saveTabs() }
    }

    // 되살릴 명령 화면의 (명령, 세션 id, 작업 경로). 대화가 남은 세션은 세션 파일의 resume_cmd(파일이 지워졌으면 남겨 둔 것),
    // 대화 없이 끝난 세션(대화 기록 없음 — 보드가 세션 파일도 지운다)은 처음 명령을 새 세션 id 로.
    // 세션 파일의 pid 가 아직 claude 로 살아 있으면(/bg·nohup 으로 떨어져 나갔거나 다른 터미널에서 이어 연 세션) 이어 열지 않는다 — nil.
    // 답변 꾸미기는 off(끈 탭·끈 세션)를 따라 mdRenderCommand 로 고친다 — 세션 파일이 없으면 남겨 둔 명령·resume_cmd 문자열로
    private func restoreCommand(_ e: [String: String], off: Bool) -> (String, String?, String)? {
        guard let command = e["command"].map({ mdRenderCommand($0, off: off) }), let cwd = e["cwd"] else { return nil }
        guard let id = e["session"] else {
            let (c, i) = freshSessionCommand(command)
            return FileManager.default.fileExists(atPath: cwd) ? (c, i, cwd) : nil
        }
        let rec = sessionRecord(id)
        if let pid = (rec?["pid"] as? NSNumber)?.int32Value, isLiveClaude(pid) {
            boardLog("탭을 되살리지 않음: 세션 \(id) 이 보드 밖(pid \(pid))에서 돌고 있습니다")
            return nil
        }
        let dir = rec?["project_dir"] as? String ?? rec?["cwd"] as? String ?? cwd   // 보드 카드와 같은 경로(워크트리에 들어간 세션도 프로젝트 폴더에서)
        guard FileManager.default.fileExists(atPath: dir) else {
            boardLog("탭을 되살리지 않음: 작업 경로가 없습니다 \(dir)")
            return nil
        }
        if FileManager.default.fileExists(atPath: transcriptPath(rec?["transcript_path"] as? String, dir, id)) {
            let resume = (rec?["resume_cmd"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? e["resume"] ?? "claude --resume \(id)"
            return (mdRenderCommand(resume, args: rec?["args"] as? [String], off: off), id, dir)
        }
        let (c, i) = freshSessionCommand(command)
        return (c, i, dir)
    }

    // 새 세션 — claude 면 보드가 세션 id 를 붙여 미리 안다. 답변 꾸미기는 새 세션 창의 체크(경로 카드 "+" 는 늘 켬) — 끄면 그 세션 id 를 기억한다
    func startSession(_ command: String, in dir: String, mdRender: Bool = UserDefaults.standard.bool(forKey: "newSessionMdRender"),
                      warned: Bool = false) {
        // 원래 작업 트리로 자율주행이 도는 경로면 먼저 경고한다(막지는 않는다) — 경로 카드 "+"·새 세션 창 모두 여기로 온다
        if !warned, let r = AutopilotStore.shared.inPlaceRun(covering: dir) {
            warnAutopilotPath(r, dir: dir) { self.startSession(command, in: dir, mdRender: mdRender, warned: true) }
            return
        }
        let (cmd, id) = freshSessionCommand(mdRenderCommand(command, off: !mdRender))
        launch(cmd, in: dir, sessionId: id, mdRenderOff: !mdRender)
    }

    // "+ 새 세션" 창 — 작업 폴더는 보드에 있는 경로(지금 경로를 먼저 고른다) + 다른 폴더
    func newSessionSheet() {
        SettingsScreen.shared.close(keep: true)
        if !expanded { setTerminalExpanded(true) }
        showBoard()
        whenPaneSettled { self.beginNewSessionSheet() }
    }

    private func beginNewSessionSheet() {
        guard window.attachedSheet == nil else { return }   // 이미 새 세션 창(또는 다른 시트)이 떠 있으면 겹쳐 띄우지 않는다(⌘T 를 또 누름)
        let paths = (store?.groups.map(\.id) ?? []).filter { FileManager.default.fileExists(atPath: $0) }
        // 지금 경로와 같은 보드 경로(창의 줄은 보드 경로 글자 그대로 견준다). 보드에 없으면 첫 경로
        let current = workspace.flatMap { ws in paths.first { workspaceKey($0) == ws } }
        let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 460, height: 420), styleMask: [.titled], backing: .buffered, defer: false)
        let end = { window.endSheet(sheet) }
        sheet.contentViewController = NSHostingController(rootView: NewSessionSheet(
            paths: paths, initial: current ?? paths.first ?? NSHomeDirectory(),
            cancel: end, start: { cmd, dir, md in end(); self.startSession(cmd, in: dir, mdRender: md) }))
        window.beginSheet(sheet)
    }

    // 탭을 보이게 한다 — 접혀 있거나 미니면 펼치고, 그 탭을 고르고, 보드를 앞으로 꺼내 터미널에 포커스.
    // id 가 화면이면(세션 줄 클릭 — place 의 화면) 그 탭의 그 화면을 고른다(크게 보는 중이면 푼다)
    func reveal(_ id: UUID) {
        SettingsScreen.shared.close(keep: true)
        if !expanded { setTerminalExpanded(true) }
        if let s = surface(id), let t = s.tab {
            if t.focused !== s { t.zoomed = false; t.focused = s }
            select(t.id)
        } else {
            select(id)
        }
        showBoard()
        focusSelected()
    }

    func toast(_ text: String) {
        withAnimation(.easeOut(duration: 0.2)) { notice = text }
        noticeHide?.cancel()
        let w = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.3)) { self?.notice = nil } }
        noticeHide = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 6, execute: w)
    }

    func dismissNotice() {
        noticeHide?.cancel()
        withAnimation(.easeOut(duration: 0.2)) { notice = nil }
    }

    // 세션이 어디서 도는가 — 보드 탭의 화면(그 안에서 시작된 claude -p 포함)·다른 터미널·백그라운드·끝남. 규칙은 BoardRules 의 sessionPlace
    // (TabPairing.swift) — 여기서는 짝짓기 결과와 화면의 세션 id 를 넘긴다(화면 목록은 짝이 없을 때만 만든다)
    func place(_ s: Session) -> SessionPlace {
        sessionPlace(s, paired: paired, ownedHeadless: tabOwnedHeadless,
                     screens: surfaces.map { ScreenSession(id: $0.id, sessionId: $0.sessionId) }, probe: tabProbe)
    }

    // 보드 탭 claude 가 넘긴 대화를 이어 받은 백그라운드 작업이면 그 화면. 카드가 보이는 자리(터미널 구역·CB 칩)와 누르면 갈 곳만
    // 이것을 따른다(shownPlace) — 탭에 쳐 넣는 동작은 place 그대로라 그 작업 때문에 탭에 아무것도 넣지 않는다. 규칙은 BoardRules
    func parkedTab(_ s: Session) -> UUID? { parkedScreen(s, parkedTabs: parkedTabs) }
    func shownPlace(_ s: Session) -> SessionPlace { shownSessionPlace(parked: parkedTab(s), place: place(s)) }

    // 세션 목록을 다시 읽을 때마다 탭의 화면들과 짝짓는다 — 규칙(tty·headless 조상·셸만 남은 화면·넘긴 작업 따라가기·경로 상태 점)은
    // BoardRules 의 pairTabs(TabPairing.swift). 여기서는 화면의 지금 값을 떠서 넘기고 결과를 화면·@Published 에 쓴다(바뀐 것만)
    func pair(_ sessions: [Session]) {
        let all = surfaces
        let r = pairTabs(sessions, screens: tabs.flatMap { t in t.surfaces.map { $0.screen(in: t.workspace) } },
                         probe: tabProbe, processes: SystemProcessControl(), live: ClaudeNames.shared)
        for (t, p) in zip(all, r.screens) {
            if t.sessionId != p.sessionId { t.sessionId = p.sessionId }
            if t.command != p.command { t.command = p.command }
            if t.mdRenderOff != p.mdRenderOff { t.mdRenderOff = p.mdRenderOff }
            if t.session != p.session { t.session = p.session }
            rememberMdRenderOff(p.mdRenderOffSession)   // 끈 화면에서 세션 id 가 바뀌어도(/clear 등) 끈 채로 이어 열게
            if t.parked != p.parked { t.parked = p.parked }
        }
        if r.paired != paired { paired = r.paired }
        if r.ownedHeadless != tabOwnedHeadless { tabOwnedHeadless = r.ownedHeadless }
        if r.parkedTabs != parkedTabs { parkedTabs = r.parkedTabs }
        parkedFrom = r.parkedFrom
        if r.workspaceStates != workspaceStates { workspaceStates = r.workspaceStates }
        saveTabs()   // 세션 id·resume_cmd·셸 경로·고른 탭이 바뀌었으면 남긴다
        // 다른 터미널 세션의 앱 이름 — 조상 프로세스로만(AppleScript 를 쓰는 TerminalControl.owner 는 부르지 않는다)
        var names = appNames
        for s in sessions where s.state != "ended" && !s.headless && r.paired[s.id] == nil && names[s.id] == nil && s.pid > 0 {
            if let id = ancestorApp(s.pid) {
                names[s.id] = terminalApps.first { $0.id == id }?.name
                    ?? NSRunningApplication.runningApplications(withBundleIdentifier: id).first?.localizedName ?? "터미널"
            }
        }
        if names != appNames { appNames = names }
        reservedNames = reservedNames.filter { Date().timeIntervalSince($0.value.at) < 15 }
        applyRenames()   // 승인 대기를 벗어났거나 첫 요청이 들어간 세션의 미뤄 둔 이름 바꾸기
    }

    // 세션의 실행 방식 — 프로세스 인자에서(-p·--print → -p, --bg·--background → --bg, 대화형이면 nil). 끝난 세션은 읽을 수 없다
    func mode(_ s: Session) -> String? {
        if let m = modes[s.pid] { return m.isEmpty ? nil : m }
        let args = processArgs(s.pid)
        // --bg-pty-host — 탭에서 /bg 로 보낸 세션을 데몬 아래에서 이어 가는 프로세스(Claude Code 2.1.302 실측)
        let m = args.contains { $0 == "--bg" || $0 == "--background" || $0 == "--bg-pty-host" } ? "--bg"
            : args.contains { $0 == "-p" || $0 == "--print" } ? "-p" : ""
        if s.pid > 0, !args.isEmpty { modes[s.pid] = m }
        return m.isEmpty ? nil : m
    }

    // 탭 고르기 — 그 탭의 경로가 지금 경로가 된다(세션 줄 클릭·이어 열기·새 세션도 이 길로 그 경로에 간다). 경로마다 마지막 탭을 기억한다
    func select(_ id: UUID) {
        selected = id
        if let t = tabs.first(where: { $0.id == id }) {
            lastTab[t.workspace] = id
            if workspace != t.workspace { workspace = t.workspace }
            if restored { t.surfaces.forEach(resumeHeld) }   // 되살리는 동안 탭마다 고르는 것은 들어간 것이 아니다
        }
        applyVisibility()
        focusSelected()
    }

    // 고른 탭의 화면만(크게 보는 중이면 고른 화면만) 그리고 나머지는 실제 크기 그대로 숨긴다. 접혀 있거나 창이 안 보이면(가려짐·최소화·
    // 닫힘·다른 데스크톱) 모두 그리지 않는다. 나눈 탭에서 고르지 않은 화면은 Ghostty 처럼 흐리게(unfocused-split-opacity)
    func applyVisibility() {
        // 접는 애니메이션 동안에는 터미널이 밀려 나가는 모습이 보이도록 그린다
        let drawing = (expanded || slide != nil) && window.occlusionState.contains(.visible)
        for t in tabs {
            let dim = t.surfaces.count > 1 && !t.zoomed
            for s in t.surfaces {
                let shown = t.id == selected && (!t.zoomed || s === t.focused)
                s.view.isHidden = !shown
                s.view.setSurfaceVisible(shown && drawing)
                let alpha = dim && s !== t.focused ? unfocusedSplitOpacity : 1
                if s.view.alphaValue != alpha { s.view.alphaValue = alpha }
            }
        }
        area?.empty.isHidden = !tabs.isEmpty
        area?.workspaceBarShown = !tabs.isEmpty
        layoutSurfaces()
    }

    // 화면 자리 — 탭마다 트리대로 터미널 칸을 나눈다(보이지 않는 탭도 실제 크기 그대로 둔다). 크게 보는 탭은 고른 화면이 칸 전체.
    // 고른 탭의 경계는 끌 수 있는 뷰로 화면들 위에 둔다. 칸 크기가 바뀔 때·트리나 고른 탭이 바뀔 때 부른다
    func layoutSurfaces() {
        guard let area else { return }
        let bounds = area.container.bounds
        var shown: [(SplitNode, NSRect, NSRect)] = []
        for t in tabs {
            var frames: [(BoardSurface, NSRect)] = [], lines: [(SplitNode, NSRect, NSRect)] = []
            t.root.layout(in: bounds, &frames, &lines)
            for (s, r) in frames {
                let f = t.zoomed && s === t.focused ? bounds : r
                if s.view.frame != f { s.view.frame = f }   // 같은 크기를 다시 넣어도 엔진이 크기를 다시 맞춘다
            }
            if t.id == selected, !t.zoomed { shown = lines }
        }
        area.container.showDividers(shown, color: dividerColor)
    }

    // 이름을 고치는 탭이면 입력 칸에서 포커스를 뺏지 않는다(그 탭을 눌렀을 때, 펼치기 애니메이션이 끝났을 때)
    func focusSelected() {
        guard expanded, !SettingsScreen.shared.shown, let t = selectedTab, !t.renaming else { return }
        window.makeFirstResponder(t.focused.view)
    }

    var selectedTab: BoardTab? { tabs.first { $0.id == selected } }
    var surfaces: [BoardSurface] { tabs.flatMap(\.surfaces) }
    func surface(_ id: UUID) -> BoardSurface? { surfaces.first { $0.id == id } }

    // 화면이 키보드 초점을 받았다(누름·⌘[ 등) — 그 탭의 고른 화면으로. 크게 보기는 여기서 풀지 않는다(화면을 옮기는 쪽이 푼다)
    func surfaceFocused(_ s: BoardSurface) {
        guard let t = s.tab, t.focused !== s, tabs.contains(where: { $0 === t }) else { return }
        t.focused = s
        applyVisibility()
        saveTabs()
    }

    // 화면 나누기 키(윈도우 > 화면 나누기)는 터미널을 보이며 한다 — 접혀 있으면 펼치고 보드를 앞으로
    private func showTerminal() {
        if !expanded { setTerminalExpanded(true) }
        showBoard()
    }

    // ⌘D·⇧⌘D — 고른 화면을 둘로 나눠 뒤쪽(오른쪽·아래)에 같은 폴더의 셸을 연다(Ghostty 처럼 50:50, 새 화면에 포커스, 크게 보기는 풀린다).
    // 폴더는 고른 화면 셸의 지금 경로(OSC 7), 그 폴더가 없으면 탭의 경로. claude 는 띄우지 않는다
    func split(right: Bool) {
        guard let t = selectedTab, let node = t.root.leaf(t.focused) else { return }
        showTerminal()
        let cwd = t.focused.cwd
        let s = makeSurface(in: FileManager.default.fileExists(atPath: cwd) ? cwd : t.workspace)
        node.split(s, right: right)
        t.watch(s)
        t.zoomed = false
        t.focused = s
        layoutSurfaces()
        attach(s)
        applyVisibility()
        focusSelected()
        saveTabs()
    }

    // ⌘[·⌘] — 트리 순서(왼쪽·위가 앞)로 앞·뒤 화면(끝에서는 반대쪽 끝으로)
    func focusSurface(step: Int) {
        guard let t = selectedTab, t.surfaces.count > 1 else { return }
        let list = t.surfaces, i = list.firstIndex { $0 === t.focused } ?? 0
        moveFocus(t, to: list[(i + step + list.count) % list.count])
    }

    // ⌥⌘방향키 — 그 방향에 있는 화면 가운데 가장 가까운 것(두 화면의 왼쪽 위 모서리 사이 거리 — Ghostty 와 같은 방식). 없으면 그대로
    func focusSurface(_ d: SplitDirection) {
        guard let t = selectedTab, let area else { return }
        var frames: [(BoardSurface, NSRect)] = [], lines: [(SplitNode, NSRect, NSRect)] = []
        t.root.layout(in: area.container.bounds, &frames, &lines)
        guard let me = frames.first(where: { $0.0 === t.focused })?.1 else { return }
        let found = frames.filter { f in
            switch d {
            case .left: return f.1.maxX <= me.minX
            case .right: return f.1.minX >= me.maxX
            case .up: return f.1.minY >= me.maxY
            case .down: return f.1.maxY <= me.minY
            }
        }.min { hypot($0.1.minX - me.minX, $0.1.maxY - me.maxY) < hypot($1.1.minX - me.minX, $1.1.maxY - me.maxY) }
        if let found { moveFocus(t, to: found.0) }
    }

    // 다른 화면으로 — 크게 보기는 풀린다(Ghostty 와 같게)
    private func moveFocus(_ t: BoardTab, to s: BoardSurface) {
        showTerminal()
        t.zoomed = false
        t.focused = s
        applyVisibility()
        focusSelected()
        saveTabs()
    }

    // ⌃⌘방향키 — 고른 화면에서 가장 가까운, 그 방향으로 나눈 경계를 그 방향으로 10pt 옮긴다(Ghostty 의 resize_split:<방향>,10).
    // 크게 보는 중이면 그대로
    func resizeSplit(_ d: SplitDirection) {
        guard let t = selectedTab, !t.zoomed, let area else { return }
        let across = d == .left || d == .right
        var node = t.root.leaf(t.focused)?.parent
        while let n = node, n.right != across { node = n.parent }
        var frames: [(BoardSurface, NSRect)] = [], lines: [(SplitNode, NSRect, NSRect)] = []
        t.root.layout(in: area.container.bounds, &frames, &lines)
        guard let node, let r = lines.first(where: { $0.0 === node })?.1 else { return }
        let length = across ? r.width : r.height
        node.setRatio(node.ratio + (d == .left || d == .up ? -10 : 10) / max(length, 1), length: length)
        showTerminal()
        layoutSurfaces()
        saveTabs()
    }

    // ⌃⌘= — 고른 탭의 나눈 크기를 고르게(같은 방향으로 나눈 화면 수대로)
    func equalizeSplits() {
        guard let t = selectedTab, t.surfaces.count > 1 else { return }
        t.root.equalize()
        showTerminal()
        layoutSurfaces()
        saveTabs()
    }

    // ⇧⌘↩ — 고른 화면 하나 크게 보기 켜고 끄기(화면이 둘 이상일 때만). 크게 보는 동안 탭 줄에 표시
    func toggleZoom() {
        guard let t = selectedTab, t.surfaces.count > 1 else { return }
        showTerminal()
        t.zoomed.toggle()
        applyVisibility()
        focusSelected()
    }

    // ⌃⌥←·⌃⌥→(Ghostty 의 탭 키 ⇧⌘[·⇧⌘]·⌃⇧⇥·⌃⇥ 도) — 탭 줄에서 왼쪽·오른쪽 탭으로(지금 경로 안에서, 끝에서는 반대쪽 끝으로, 접혀 있으면 펼친다)
    func stepTab(_ step: Int) {
        let list = tabs(in: workspace)
        guard !list.isEmpty else { return }
        let i = list.firstIndex { $0.id == selected } ?? 0
        reveal(list[(i + step + list.count) % list.count].id)
    }

    // ⇧⌘R — 고른 탭의 이름 자리를 입력 칸으로 바꾼다. 접혀 있으면 펼친 뒤(⌘1~⌘9 의 reveal 과 같이)
    func beginRename() {
        guard let t = tabs.first(where: { $0.id == selected }) else { return }
        if !expanded { setTerminalExpanded(true) }
        showBoard()
        t.renaming = true
    }

    // 이름 바꾸기 끝 — name 이 nil 이면 취소(Esc). 비우거나 폴더 이름과 같게 저장하면 사용자 이름을 지워 폴더 이름(자동)으로 돌아간다.
    // 입력 칸의 처음 값(탭 줄에 보이던 이름)을 고치지 않고 저장하면 그대로 둔다 — 번호가 붙은 세션 이름이 사용자 이름으로 남지 않게.
    // 바꾸면 그 탭의 claude 세션 이름도 바꾼다(renameSessions). 저장·취소 모두 터미널에 포커스를 돌려준다
    func endRename(_ tab: BoardTab, name: String?) {
        guard tab.renaming else { return }
        tab.renaming = false
        if let name {
            let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
            if n != tab.shownName {
                tab.customName = n.isEmpty || n == tab.folderName ? nil : n
                saveTabs()
                renameSessions(tab)
            }
        }
        focusSelected()
    }

    // ⌘ 를 누름·놓음(flagsChanged 마다 오므로 ⌘ 가 바뀔 때만 본다) — 누르는 즉시 보이고(⌘C 를 칠 때도 잠깐 보인다, 사용자 결정), 놓거나 앱이 비활성이 되면 끈다
    func commandFlag(_ down: Bool) {
        guard down != commandHeld else { return }
        commandHeld = down
    }

    // 탭의 × — 화면이 하나면 ⌘W 와 같은 엔진 동작으로 보내 셸 프롬프트처럼 물을 필요가 없으면 바로, 프로그램이 돌고 있으면 물어서 닫는다.
    // 나눈 탭은 탭째 닫는다 — 셸 프롬프트가 아닌(프로그램이 도는) 화면이 하나라도 있으면 한 번 묻는다
    func requestClose(_ tab: BoardTab) {
        if let s = tab.root.surface {
            if !s.view.performBindingAction("close_surface") { close(s, confirm: false) }
            return
        }
        let busy = tab.surfaces.filter { !$0.atPrompt }.count
        guard busy > 0 else { return remove(tab) }
        confirmClose("탭을 닫을까요?", "\(tab.name) 탭의 화면 \(busy)개에서 도는 프로그램이 함께 끝납니다.") { self.remove(tab) }
    }

    // 화면 닫기(셸이 끝남·⌘W) — 프로그램이 아직 돌고 있으면 먼저 묻는다. 탭의 마지막 화면이면 탭을 닫는다
    func close(_ s: BoardSurface, confirm: Bool) {
        guard let t = s.tab, tabs.contains(where: { $0 === t }) else { return }
        guard confirm else { remove(s); return }
        if t.surfaces.count == 1 {
            confirmClose("탭을 닫을까요?", "\(t.shownName) 탭에서 도는 프로그램이 함께 끝납니다.") { self.remove(s) }
        } else {
            confirmClose("화면을 닫을까요?", "\(t.name) 탭의 이 화면에서 도는 프로그램이 함께 끝납니다.") { self.remove(s) }
        }
    }

    private func confirmClose(_ title: String, _ text: String, then run: @escaping @MainActor () -> Void) {
        let a = NSAlert()
        a.messageText = title
        a.informativeText = text
        a.addButton(withTitle: "닫기")
        a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"
        whenPaneSettled { a.beginSheetModal(for: window) { if $0 == .alertFirstButtonReturn { run() } } }
    }

    // 화면 하나를 뺀다 — 탭의 마지막 화면이면 탭째. 고른 화면이었으면 옆 화면(형제 쪽에서 가장 가까운 것)을 고르고 크게 보기는 풀린다
    private func remove(_ s: BoardSurface) {
        guard let t = s.tab, let node = t.root.leaf(s) else { return }
        guard let next = node.remove() else { return remove(t) }
        t.unwatch(s)
        // 엔진 surface 를 내려 셸·프로그램을 끝낸다
        s.view.controller = nil
        s.view.removeFromSuperview()
        if t.focused === s { t.zoomed = false; t.focused = next }
        t.objectWillChange.send()
        applyVisibility()
        if t.id == selected { focusSelected() }
        saveTabs()
    }

    private func remove(_ tab: BoardTab) {
        guard let i = tabs.firstIndex(where: { $0 === tab }) else { return }
        let ws = tab.workspace
        let at = tabs(in: ws).firstIndex { $0 === tab } ?? 0   // 경로 안에서의 자리
        let order = workspaces                                 // 닫기 전 경로 줄
        tabs.remove(at: i)
        // 엔진 surface 를 내려 셸·프로그램을 끝낸다 — 창에서 빼는 것만으로는 뷰가 바로 풀리지 않아 셸이 남는다(실측)
        for s in tab.surfaces {
            s.view.controller = nil
            s.view.removeFromSuperview()
        }
        if lastTab[ws] == tab.id { lastTab[ws] = nil }
        if selected == tab.id {
            // 같은 경로의 옆 탭 → 그 경로에 탭이 없으면 경로 줄에서 옆 경로(그 경로의 마지막 탭) → 다른 경로도 없으면 빈 화면
            let rest = tabs(in: ws), others = order.filter { $0 != ws }
            let next = !rest.isEmpty ? rest[min(at, rest.count - 1)].id
                : others.isEmpty ? nil : shownTab(others[min(order.firstIndex(of: ws) ?? 0, others.count - 1)])
            if let next { select(next) } else { selected = nil; workspace = nil }
        }
        applyVisibility()
        focusSelected()
        saveTabs()
    }
}

// 세션이 도는 곳(SessionPlace — 칩·클릭 흐름·카드 구역이 쓴다)은 BoardRules/Terminals.swift 에 있다. 정하는 규칙은 BoardRules/TabPairing.swift
// (sessionPlace·pairTabs — headless 의 조상 찾기도), 부르는 것은 TerminalHub.place·pair

// 부모 pid — 읽지 못하면 0
func parentPid(_ pid: Int32) -> Int32 {
    var info = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.stride
    var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
    guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return 0 }
    return info.kp_eproc.e_ppid
}

// 프로세스 실행 인자(sysctl KERN_PROCARGS2 — state.py proc_args 와 같은 방식). 읽지 못하면 빈 배열
func processArgs(_ pid: Int32) -> [String] {
    guard pid > 0 else { return [] }
    var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
    var size = 1 << 18
    var buf = [UInt8](repeating: 0, count: size)
    guard sysctl(&mib, 3, &buf, &size, nil, 0) == 0, size > 4 else { return [] }
    let argc = Int(buf.withUnsafeBytes { $0.load(as: Int32.self) })
    var i = 4
    while i < size, buf[i] != 0 { i += 1 }   // 실행 파일 경로를 건너뛴다
    while i < size, buf[i] == 0 { i += 1 }
    var args: [String] = []
    while i < size, args.count < argc {
        var j = i
        while j < size, buf[j] != 0 { j += 1 }
        args.append(String(decoding: buf[i..<j], as: UTF8.self))
        i = j + 1
    }
    return args
}

// 살아 있는 claude 프로세스인가 — 끝난 뒤 pid 가 다른 프로그램에 다시 쓰였으면 아니다. 실행 파일 이름(p_comm)은 버전 번호라
// state.py find_claude 처럼 첫 인자의 이름으로 본다(좀비·남의 프로세스는 인자를 읽지 못해 아니다)
func isLiveClaude(_ pid: Int32) -> Bool {
    guard pid > 0, kill(pid, 0) == 0, let first = processArgs(pid).first else { return false }
    return (first as NSString).lastPathComponent == "claude"
}

// 세션 파일(sessions/<id>.json)을 그대로 읽는다 — 보드 목록(Store)은 숨긴 경로를 빼고 상태를 고쳐 보이므로
func sessionRecord(_ id: String) -> [String: Any]? {
    guard let data = FileManager.default.contents(atPath: (sessionsDir as NSString).appendingPathComponent("\(id).json")) else { return nil }
    return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
}

// 업데이트·종료로 함께 꺼지는 보드 탭 세션 가운데 일하는 중인 것과 그 까닭 — 작업 중·승인 대기, 서브에이전트가 도는 세션,
// Bash 백그라운드 셸이 도는 세션(보드 목록은 대기일 때만 세지만 여기서는 상태와 상관없이 센다), 탭 안의 claude -p.
// 백그라운드·다른 터미널 세션은 꺼지지 않으므로 넣지 않는다
@MainActor func busyTabSessions() -> [(Session, String)] {
    let hub = TerminalHub.shared
    let shells = backgroundShells()
    return (hub.store?.groups.flatMap(\.sessions) ?? []).compactMap { s in
        guard s.state != "ended", case .tab(_, let headless) = hub.place(s) else { return nil }
        var why: [String] = []
        if headless { why.append("claude -p") }
        else if s.state == "waiting" { why.append("승인 대기") }
        else if s.state == "working" { why.append("작업 중") }
        let subs = s.subagents.filter { $0.state != "done" }.count
        if subs > 0 { why.append("서브에이전트 \(subs)개") }
        if let n = shells[s.pid], n > 0 { why.append("셸 \(n)개") }
        return why.isEmpty ? nil : (s, why.joined(separator: " · "))
    }
}

// 업데이트·종료 전 확인 — 꺼지는 보드 탭 세션 가운데 일하는 중인 것이 있으면 묻고, 없으면 묻지 않는다.
// 계속을 고르면 탭 목록을 남기고 그 세션들이 도는 탭의 주 세션(사람이 입력하는 대화형 claude)에 일시중지 표지를 남긴다 —
// 탭 안의 claude -p 는 입력 자리가 없어 표지를 남기지 않고, 그것·서브에이전트·셸만 돌고 주 세션은 대기였어도 주 세션에 남긴다(주 세션이
// 시킨 일이라 이어 가기 문구로 다시 시킨다). 다시 뜰 때 이어 연 탭은 일시중지로 보이고 "세션 재개"로 이어 간다
// (state.py 는 새 요청 때만 표지를 지운다). 돌려주는 값: nil = 취소, 아니면 새로 남긴 표지 파일
@MainActor func confirmShutdown(update: Bool) -> [String]? {
    TerminalHub.shared.slide?.finish()   // 확인 창은 바로 답해야 하므로 펼치기·접기 애니메이션을 끝 상태로 마치고 묻는다
    let busy = busyTabSessions()
    guard !busy.isEmpty else { TerminalHub.shared.saveTabs(); return [] }
    let a = NSAlert()
    a.messageText = "작업 중인 세션 \(busy.count)개가 꺼집니다"
    a.informativeText = "보드가 터미널을 쥐고 있어서 \(update ? "업데이트하면" : "보드를 끄면") 진행 중인 응답과 탭에서 도는 셸·서브에이전트가 끊깁니다. "
        + "보드가 다시 뜨면 대화를 이어 엽니다. 끊긴 세션은 일시중지로 남아 \"세션 재개\"로 이어 갈 수 있습니다."
    let list = NSHostingView(rootView: ShutdownList(rows: busy))
    list.frame.size = list.fittingSize
    a.accessoryView = list
    a.addButton(withTitle: update ? "업데이트" : "종료").hasDestructiveAction = true
    a.addButton(withTitle: "취소").keyEquivalent = "\u{1b}"
    NSApp.activate(ignoringOtherApps: true)
    guard a.runModal() == .alertFirstButtonReturn else { return nil }
    TerminalHub.shared.saveTabs()
    // 창을 띄워 둔 동안 끝났거나 새로 일을 시작한 세션이 있을 수 있어 고른 순간의 것으로 다시 센다
    (NSApp.delegate as? AppDelegate)?.store.reload()
    let hub = TerminalHub.shared
    var mains: [String: Session] = [:]
    for (s, _) in busyTabSessions() {
        guard case .tab(let id, _) = hub.place(s), let main = hub.surface(id)?.session,
              main.state != "ended", hub.mode(main) == nil else { continue }   // 셸 탭에서 친 claude -p 처럼 주 세션이 -p 면 남기지 않는다
        mains[main.id] = main
    }
    var marks: [String] = []
    for s in mains.values where !FileManager.default.fileExists(atPath: pausedMark(s)) {
        if let data = pausedMarkData(topic: s.topic, at: Date()), FileManager.default.createFile(atPath: pausedMark(s), contents: data) {
            marks.append(pausedMark(s))
        }
    }
    return marks
}

// 확인 창의 세션 목록(상태 색 · 경로 — 제목 · 까닭)과 영향 없는 세션 안내
struct ShutdownList: View {
    let rows: [(Session, String)]
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    HStack(spacing: 8) {
                        Circle().fill(Palette.color(row.0.state)).frame(width: 8, height: 8)
                        Text("\((row.0.cwd as NSString).lastPathComponent) · \(row.0.title)").lineLimit(1).truncationMode(.tail)
                        Spacer(minLength: 6)
                        Text(row.1).foregroundStyle(.secondary).fixedSize()
                    }
                    .font(.system(size: 12))
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.06)))
            Text("백그라운드 세션과 다른 터미널의 세션은 영향 없이 그대로 돕니다.")
                .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .frame(width: 320)
    }
}

// 새로 띄우는 claude 에 세션 id 를 붙인다(--session-id) — 보드가 그 세션을 미리 안다. claude 가 아니거나 이어 열기·이어 하기면 그대로.
// 이미 --session-id 가 있으면 새 id 로 바꾼다(되살리기가 대화 없이 끝난 세션을 다시 띄울 때)
func freshSessionCommand(_ command: String) -> (String, String?) {
    let has = { (pattern: String) in command.range(of: pattern, options: .regularExpression) != nil }
    guard has(#"^\s*(\S*/)?claude(\s|$)"#), !has(#"(^|\s)(--resume|-r|--continue|-c)(=|\s|$)"#) else { return (command, nil) }
    // 따옴표 안 공백이 상하지 않게 문자열을 쪼개지 않고 고친다
    let base = command.replacingOccurrences(of: #"\s--session-id(=|\s+)\S+"#, with: "", options: .regularExpression)
    let id = UUID().uuidString.lowercased()
    return (base + " --session-id " + id, id)
}

// 답변 마크다운 꾸미기(md-render) — 보드 탭 claude 에 --plugin-dir 로 넘기는 경로. 앱 안 Contents/Resources/plugins/md-render 를
// ~/.claude/notifier/plugins/md-render 로 옮겨 두고 그쪽을 넘긴다 — 대화형 claude 는 사용자 소유 플러그인 폴더에 tsconfig.json·
// .claude-plugin/types/ 를 써서(실측) 앱 안 경로를 넘기면 서명된 앱이 바뀐다. 앱이 싣는 파일만 내용이 다를 때 덮고 Claude Code 가 만든 것은 둔다.
// 앱 밖(.build)에서 띄워 원본이 없거나 옮기지 못하면 nil — 어떤 길에도 붙이지 않는다
let mdRenderDir: String? = {
    let fm = FileManager.default
    guard let src = Bundle.main.resourceURL?.appendingPathComponent("plugins/md-render").path, fm.fileExists(atPath: src) else { return nil }
    let dest = NSHomeDirectory() + "/.claude/notifier/plugins/md-render"
    for f in (try? fm.subpathsOfDirectory(atPath: src)) ?? [] {
        guard let data = fm.contents(atPath: "\(src)/\(f)") else { continue }   // 폴더
        let to = "\(dest)/\(f)"
        if fm.contents(atPath: to) == data { continue }
        do {
            try fm.createDirectory(atPath: (to as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            try data.write(to: URL(fileURLWithPath: to), options: .atomic)
        } catch {
            boardLog("답변 꾸미기 플러그인을 옮기지 못했습니다: \(to) \(error.localizedDescription)")
            return nil
        }
    }
    return dest
}()

// state.py 가 resume_cmd 를 만들 때 쓰는 shlex.quote 와 같은 인용 — resume_cmd 안의 토큰을 그대로 찾고, 보드가 붙이는 경로를
// 엔진이 명령을 넘기는 bash -c 에 안전하게 넘긴다
func shlexQuote(_ s: String) -> String {
    if !s.isEmpty, s.range(of: "[^A-Za-z0-9_@%+=:,./-]", options: .regularExpression) == nil { return s }
    return "'" + s.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
}

// shlex.quote 꼴 토큰 가운데 md-render 의 --plugin-dir(값이 /plugins/md-render 로 끝남) — 따옴표 없는 꼴, '…' 꼴(안의 ' 는 '"'"'),
// '--plugin-dir=…' 꼴. 앞의 공백까지 잡아 지운다
private let mdRenderTokenPattern = #"\s+(?:--plugin-dir(?:\s+|=)[A-Za-z0-9_@%+=:,./-]*/plugins/md-render|--plugin-dir\s+'[^']*(?:'"'"'[^']*)*/plugins/md-render'|'--plugin-dir=[^']*(?:'"'"'[^']*)*/plugins/md-render')(?=\s|$)"#

// 보드 탭에서 띄우거나 이어 여는 claude 의 답변 꾸미기(사용자 결정 2026-10-17) — 기본으로 켜고(Ghostty·claude -p 에서 시작해 보드로 옮기거나
// 이어 연 세션 포함), 새 세션 창에서 끄고 띄운 세션(isMdRenderOff)만 끈다. 명령에 든 md-render 의 --plugin-dir(옛 경로·옛 설치본·사용자가 적은 것)은
// 빼고, 켜져 있으면 지금 경로를 한 번 붙인다. 사용자의 다른 --plugin-dir 은 둔다. 뺄 토큰은 세션 파일의 args(state.py 가 shlex.join 해
// resume_cmd 를 만든 재료)를 알면 그 shlex.quote 꼴 그대로 찾고, 남은 것·args 를 모를 때(저장해 둔 명령·resume_cmd)는 shlex.quote 꼴 정규식으로.
// 그래도 md-render 경로가 남으면(큰따옴표 등 다른 꼴로 사용자가 적음) 더 붙이지 않는다. claude 가 아닌 명령은 그대로
func mdRenderCommand(_ command: String, args: [String]? = nil, off: Bool) -> String {
    guard command.range(of: #"^\s*(\S*/)?claude(\s|$)"#, options: .regularExpression) != nil else { return command }
    var out = command, i = 0
    let args = args ?? []
    while i < args.count {
        var token: String?
        if args[i].hasPrefix("--plugin-dir="), args[i].hasSuffix("/plugins/md-render") {
            token = shlexQuote(args[i])
        } else if args[i] == "--plugin-dir", i + 1 < args.count, args[i + 1].hasSuffix("/plugins/md-render") {
            token = "--plugin-dir " + shlexQuote(args[i + 1])
            i += 1
        }
        if let token { out = out.replacingOccurrences(of: " \(token) ", with: " ") }   // resume_cmd 는 --resume <id> 로 끝나 인자 뒤에 늘 공백이 있다
        i += 1
    }
    out = out.replacingOccurrences(of: mdRenderTokenPattern, with: "", options: .regularExpression)
    guard !off, let dir = mdRenderDir, !out.contains("/plugins/md-render") else { return out }
    return out + " --plugin-dir " + shlexQuote(dir)
}

// 새 세션 창에서 꾸미기를 끄고 띄운 세션 id — 보드 설정 mdRenderOff. 그 세션은 이어 열 때도 끈 채로 연다. 끈 탭의 세션 id 가 바뀌면
// (/clear, 되살리기가 대화 없는 세션을 새 id 로 띄움) 그 id 도 더한다(launch·pair). 보드가 시작할 때 세션 파일이 없어진 id 는 치운다
func isMdRenderOff(_ id: String?) -> Bool {
    guard let id else { return false }
    return UserDefaults.standard.stringArray(forKey: "mdRenderOff")?.contains(id) ?? false
}

func rememberMdRenderOff(_ id: String?) {
    guard let id, !isMdRenderOff(id) else { return }
    UserDefaults.standard.set((UserDefaults.standard.stringArray(forKey: "mdRenderOff") ?? []) + [id], forKey: "mdRenderOff")
}

func pruneMdRenderOff() {
    let list = UserDefaults.standard.stringArray(forKey: "mdRenderOff") ?? []
    let kept = list.filter { FileManager.default.fileExists(atPath: (sessionsDir as NSString).appendingPathComponent("\($0).json")) }
    if kept.count != list.count { UserDefaults.standard.set(kept, forKey: "mdRenderOff") }
}

// 펼쳤을 때 보드 폭 — 창이 좁으면 터미널 최소 폭을 남기고 줄인다. 펼치기·접기 애니메이션 중에는 그 애니메이션의 보드 폭
@MainActor func boardPaneWidth() -> CGFloat {
    if let slide = TerminalHub.shared.slide { return slide.board }
    guard TerminalHub.shared.expanded else { return window.frame.width }
    return max(boardMinWidth, min(TerminalHub.shared.boardWidth, window.frame.width - paneDividerWidth - terminalMinWidth))
}

let terminalExpandedMinSize = NSSize(width: boardMinWidth + paneDividerWidth + terminalMinWidth, height: 300)

// 펼치기: 창을 화면의 쓸 수 있는 영역 전체로. 접기: 보드만 화면 한쪽(보드 위치 — 오른쪽·왼쪽)에 끈 폭으로.
// 펼쳐 화면 전체를 덮은 동안에는 "위에 고정"·투명도를 쓰지 않는다 — 화면 전체를 덮은 창이 늘 위에 있으면 다른 앱을 쓸 수 없다(expandedToFullScreen).
// animated — 설정 > 창 의 "펼치기·접기 애니메이션"(보드 설정 terminalAnimation, 기본 끔). 처음 크기로 붙이기·최대축소는 접은 뒤
// 제 애니메이션을 바로 이어 돌리므로 false 로 부른다. 도는 애니메이션이 있으면 끝 상태로 마친 뒤 새 요청을 받는다
@MainActor func setTerminalExpanded(_ on: Bool, animated: Bool = UserDefaults.standard.bool(forKey: "terminalAnimation")) {
    let hub = TerminalHub.shared
    hub.slide?.finish()
    if !on { SettingsScreen.shared.dismissForCollapse() }
    guard hub.expanded != on, let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    let target = on ? vf : NSRect(x: hub.boardOnLeft ? vf.minX : vf.maxX - hub.boardWidth, y: vf.minY, width: hub.boardWidth, height: vf.height)
    // 시트가 붙어 있으면 애니메이션 없이 — 시트가 붙은 창은 프레임이 멈췄다 건너뛴다
    guard animated, window.isVisible, window.attachedSheet == nil, let root = window.contentView as? BoardRootView else {
        hub.setExpanded(on)
        if !on { window.makeFirstResponder(nil) }   // 숨는 터미널에 키 입력이 가지 않게 포커스를 거둔다
        window.contentMinSize = on ? terminalExpandedMinSize : NSSize(width: miniWidth, height: miniMinHeight)
        window.setFrame(target, display: true)
        if !on { saveRestoreFrame() }
        settleTerminalPane()
        return
    }
    // 애니메이션 — 터미널 영역은 펼친 크기로 고정하고 자리만 옮긴다(펼칠 때 크기가 다르면 시작 전에 한 번만 맞춘다)
    let slide: PaneSlide
    if on {
        let bw = max(boardMinWidth, min(hub.boardWidth, vf.width - paneDividerWidth - terminalMinWidth))   // 펼친 뒤 boardPaneWidth
        slide = PaneSlide(from: window.frame, to: target, board: (window.frame.width, bw),
                          terminal: NSSize(width: vf.width - bw - paneDividerWidth, height: vf.height)) { settleTerminalPane() }
        hub.slide = slide
        hub.setExpanded(true)
        applyPin(UserDefaults.standard.bool(forKey: "pinned"))   // 위에 고정·투명도는 펼치기 시작할 때 바로 내린다
        WindowOpacity.shared.apply()
        hub.focusSelected()
    } else {
        window.makeFirstResponder(nil)
        slide = PaneSlide(from: window.frame, to: target, board: (boardPaneWidth(), hub.boardWidth), terminal: root.terminal.frame.size) {
            saveRestoreFrame()
            settleTerminalPane()
        }
        hub.slide = slide
        hub.setExpanded(false)
    }
    window.contentMinSize = NSSize(width: miniWidth, height: miniMinHeight)   // 중간 크기가 최소 크기에 막히지 않게 — 끝나면 맞춘다
    root.needsLayout = true
    root.layoutSubtreeIfNeeded()
    hub.applyVisibility()
    slide.start()
}

// 펼치기·접기가 끝난 뒤 — 최소 크기·위에 고정·투명도·크기 단계·터미널 그리기·포커스를 펼침 상태에 맞춘다
@MainActor func settleTerminalPane() {
    let hub = TerminalHub.shared
    window.contentMinSize = hub.expanded ? terminalExpandedMinSize : NSSize(width: miniWidth, height: miniMinHeight)
    applyPin(UserDefaults.standard.bool(forKey: "pinned"))
    window.contentView?.needsLayout = true
    BoardFit.shared.update()
    WindowOpacity.shared.apply()
    hub.applyVisibility()
    hub.focusSelected()
}

// 펼치기·접기 애니메이션 — 창 프레임을 화면 주사율에 맞춰 옮긴다(0.55초, ease-in-out). 그동안 터미널 영역은 크기를 그대로 두고
// 자리만 옮겨(BoardRootView.place) pty 크기가 바뀌지 않는다 — 프레임마다 바뀌면 안의 claude 화면이 그때마다 다시 그려진다
@MainActor final class PaneSlide: NSObject {
    static let duration: CFTimeInterval = 0.55
    let terminal: NSSize               // 터미널 영역 크기 — 애니메이션 동안 그대로
    private(set) var board: CGFloat    // 지금 보드 폭 — 처음 폭에서 끝 폭으로(보통 같다. 미니·손으로 줄인 창에서 펼칠 때만 다르다)
    private let from: NSRect, to: NSRect, boardFrom: CGFloat, boardTo: CGFloat
    private var started: CFTimeInterval = 0
    private var link: CADisplayLink?
    private var done: (() -> Void)?
    var after: [@MainActor () -> Void] = []   // 끝난 뒤 띄울 시트·확인 창(whenPaneSettled)

    init(from: NSRect, to: NSRect, board: (CGFloat, CGFloat), terminal: NSSize, done: @escaping () -> Void) {
        self.from = from
        self.to = to
        boardFrom = board.0
        boardTo = board.1
        self.board = board.0
        self.terminal = terminal
        self.done = done
    }

    func start() {
        started = CACurrentMediaTime()
        link = window.displayLink(target: self, selector: #selector(tick(_:)))
        link?.add(to: .main, forMode: .common)
        // 화면이 그리기를 멈춰 링크가 돌지 않아도(잠자기 등) 끝 상태로 마친다
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.duration + 0.5) { [weak self] in self?.finish() }
    }

    @objc private func tick(_ link: CADisplayLink) {
        let t = min(1, (CACurrentMediaTime() - started) / Self.duration)
        guard t < 1 else { return finish() }
        let e = CGFloat(t < 0.5 ? 4 * t * t * t : 1 - pow(-2 * t + 2, 3) / 2)   // ease-in-out(3차)
        board = boardFrom + (boardTo - boardFrom) * e
        window.setFrame(NSRect(x: from.minX + (to.minX - from.minX) * e, y: from.minY + (to.minY - from.minY) * e,
                               width: from.width + (to.width - from.width) * e, height: from.height + (to.height - from.height) * e),
                        display: true)
    }

    // 끝 상태로 마친다 — 두 번 불러도 한 번만
    func finish() {
        guard let done else { return }
        self.done = nil
        link?.invalidate()
        link = nil
        board = boardTo
        window.setFrame(to, display: true)
        if TerminalHub.shared.slide === self { TerminalHub.shared.slide = nil }
        done()
        // 다음 차례에 — 그사이 새 애니메이션이 시작했으면 그것이 끝난 뒤로 다시 미룬다
        for run in after { DispatchQueue.main.async { whenPaneSettled(run) } }
        after = []
    }
}

// 시트·확인 창은 펼치기·접기 애니메이션이 끝난 뒤에 띄운다 — 도는 중에 붙이면 시트가 먼저 붙고 창 프레임이 0.3초쯤 멈췄다가
// 건너뛴다(접는 중이면 접힌 창 가운데에 먼저 붙는다, 2026-10-18 실측). 애니메이션이 없으면 바로
@MainActor func whenPaneSettled(_ run: @escaping @MainActor () -> Void) {
    if let slide = TerminalHub.shared.slide { slide.after.append(run) } else { run() }
}

// 보드 위치 바꾸기(메뉴) — 바로 적용한다. 펼쳤으면 배치만 바뀌고, 접혔으면 같은 크기로 화면 반대쪽 끝으로 옮긴다(미니도 높이·위치 그대로).
// 미니에서 돌아갈 자리도 반대쪽 끝으로 옮겨 둔다
@MainActor func setBoardSide(left: Bool) {
    let hub = TerminalHub.shared
    hub.slide?.finish()
    guard hub.boardOnLeft != left else { return }
    hub.setBoardOnLeft(left)
    if let s = UserDefaults.standard.string(forKey: "boardRestoreFrame") {
        var r = NSRectFromString(s)
        if let vf = NSScreen.screens.first(where: { $0.visibleFrame.intersects(r) })?.visibleFrame {
            r.origin.x = left ? vf.minX : vf.maxX - r.width
            UserDefaults.standard.set(NSStringFromRect(r), forKey: "boardRestoreFrame")
        }
    }
    if hub.expanded {
        window.contentView?.needsLayout = true
        return
    }
    guard let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    var f = window.frame
    f.origin.x = left ? vf.minX : vf.maxX - f.width
    window.setFrame(f, display: true)
    if BoardFit.shared.tier != .mini { saveRestoreFrame() }
}

// ⌃⌥T·제목 막대 단추 — 펼쳐 있어도 보드가 앞에 없으면(다른 앱을 쓰는 중) 접지 않고 앞으로 꺼내 터미널에 포커스를 준다
@MainActor func toggleTerminal() {
    let hub = TerminalHub.shared
    if hub.expanded, !(NSApp.isActive && window.isKeyWindow) {
        showBoard()
        hub.focusSelected()
        return
    }
    setTerminalExpanded(!hub.expanded)
    if hub.expanded { showBoard() }
}

// 창이 화면의 쓸 수 있는 영역(메뉴 막대·Dock 을 뺀 곳, 펼치기와 같은 기준) 전체인가 — 2pt 오차까지 같은 것으로 본다
@MainActor func windowFillsScreen() -> Bool {
    guard let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return false }
    let f = window.frame
    return abs(f.minX - vf.minX) <= 2 && abs(f.minY - vf.minY) <= 2 && abs(f.width - vf.width) <= 2 && abs(f.height - vf.height) <= 2
}

// 터미널을 펼쳐 창이 화면 전체를 덮었나 — 그동안은 위에 고정·투명도를 쓰지 않는다. 펼치는 애니메이션 중에는 끝 크기(화면 전체)로 본다
@MainActor func expandedToFullScreen() -> Bool {
    let hub = TerminalHub.shared
    return hub.expanded && (hub.slide != nil || windowFillsScreen())
}

// 창 크기 전환(터미널을 펼쳤을 때 제목 막대 더블클릭·초록 단추) — 창이 화면의 쓸 수 있는 영역(펼치기와 같은 기준)과 같으면 가로·세로
// 60% 크기로 그 영역 가운데에, 그 밖의 크기면 그 영역 전체로. 크기는 기억하지 않는다(펼치기·다시 띄우기는 늘 화면 전체).
// 시스템 설정 "제목 막대 더블클릭"과 상관없이 늘 이 동작이다
@MainActor func toggleWindowSize() {
    let hub = TerminalHub.shared
    hub.slide?.finish()
    guard hub.expanded, let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    var target = vf
    if windowFillsScreen() {
        // setFrame 은 최소 크기(SizeGuard)를 거치지 않으므로 직접 맞춘다
        let min = window.frameRect(forContentRect: NSRect(origin: .zero, size: window.contentMinSize)).size
        let w = max((vf.width * 0.6).rounded(), min.width), h = max((vf.height * 0.6).rounded(), min.height)
        target = NSRect(x: (vf.midX - w / 2).rounded(), y: (vf.midY - h / 2).rounded(), width: w, height: h)
    }
    // 애니메이션 없이 — animate: true 는 프레임마다 터미널 크기가 바뀌어 pty 크기 변화가 0.34초에 11번 갔다(안의 claude 화면이
    // 그때마다 다시 그려진다, 2026-10-19 실측). 펼치기·접기 애니메이션처럼 크기를 고정할 수 없는 전환이다
    window.setFrame(target, display: true)
}

// 창 크기 전환을 받는 더블클릭 — 창 맨 위 제목 막대 띠의 빈 곳(터미널 쪽·보드 쪽)과 터미널 탭 줄의 빈 곳. 신호등 단추·제목 막대
// 부속(붙이기·터미널 펼치기·최대축소 단추)은 창 내용 밖(제목 막대 뷰)에 있어 그 단추 동작만 한다. 제목 막대 띠도 SwiftUI 호스팅 뷰가
// 먼저 받아 시스템의 더블클릭 확대가 일어나지 않으므로 앱 안 이벤트 감시로 받는다(2026-10-19 실측)
@MainActor func isWindowSizeDoubleClick(_ e: NSEvent) -> Bool {
    guard e.clickCount == 2, e.window === window, TerminalHub.shared.expanded,
          let content = window.contentView, let frameView = content.superview,
          let hit = frameView.hitTest(e.locationInWindow), hit.isDescendant(of: content) else { return false }
    let band = window.frame.height - window.contentLayoutRect.height
    if e.locationInWindow.y >= window.frame.height - band { return true }
    return TerminalHub.shared.area?.isTabBarBlank(e.locationInWindow) == true
}

// 창 내용: [터미널 영역 | 경계 | 보드]. 터미널 영역은 제목 막대 띠까지 덮고(창 바탕이 투명하다) 탭 줄은 그 아래에 둔다.
// 접힌 동안에도 터미널 영역은 창에 붙은 채 펼쳤을 때 크기로 둔다 —
// 크기가 0 이면 셸이 뜨지 않고, 크기가 바뀌면 안의 프로그램이 다시 그린다
final class BoardRootView: NSView {
    let terminal = TerminalAreaView()
    let divider = PaneDivider()
    let board: NSView

    init(board: NSView) {
        self.board = board
        super.init(frame: .zero)
        addSubview(terminal)
        addSubview(divider)
        addSubview(board)
    }
    required init?(coder: NSCoder) { fatalError() }

    override func layout() { super.layout(); place() }
    override func resizeSubviews(withOldSize oldSize: NSSize) { place() }

    private func place() {
        guard let w = window else { return }
        let hub = TerminalHub.shared, left = hub.boardOnLeft
        let below = w.contentLayoutRect.height   // 제목 막대 아래 높이
        terminal.titlebar = bounds.height - below
        if let slide = hub.slide {
            // 펼치기·접기 애니메이션 중 — 보드는 보드 쪽 창 가장자리에 붙여 화면의 제자리에 두고, 터미널 영역은 고정 크기(윗변을 창 윗변에)로
            // 보드 옆에 붙인다. 창 밖으로 나간 부분은 잘린다. 크기가 그대로라 pty 크기도 바뀌지 않는다
            let bw = slide.board, ts = slide.terminal
            let bx = left ? 0 : bounds.width - bw
            board.frame = NSRect(x: bx, y: 0, width: bw, height: bounds.height)
            divider.frame = NSRect(x: left ? bw : bx - paneDividerWidth, y: 0, width: paneDividerWidth, height: bounds.height)
            terminal.frame = NSRect(x: left ? bw + paneDividerWidth : bx - paneDividerWidth - ts.width, y: bounds.height - ts.height,
                                    width: ts.width, height: ts.height)
            terminal.isHidden = false
            divider.isHidden = false
            return
        }
        if hub.expanded {
            let bw = boardPaneWidth(), tw = bounds.width - bw - paneDividerWidth
            if left {
                board.frame = NSRect(x: 0, y: 0, width: bw, height: bounds.height)
                divider.frame = NSRect(x: bw, y: 0, width: paneDividerWidth, height: bounds.height)
                terminal.frame = NSRect(x: bw + paneDividerWidth, y: 0, width: tw, height: bounds.height)
            } else {
                terminal.frame = NSRect(x: 0, y: 0, width: tw, height: bounds.height)
                divider.frame = NSRect(x: tw, y: 0, width: paneDividerWidth, height: bounds.height)
                board.frame = NSRect(x: bounds.width - bw, y: 0, width: bw, height: bounds.height)
            }
        } else {
            var size = terminal.frame.size
            if size.width < terminalMinWidth, let vf = (w.screen ?? NSScreen.main)?.visibleFrame {
                size = NSSize(width: vf.width - TerminalHub.shared.boardWidth - paneDividerWidth, height: vf.height)
            }
            terminal.frame = NSRect(x: -size.width - 100, y: 0, width: size.width, height: size.height)
            board.frame = bounds
        }
        terminal.isHidden = !TerminalHub.shared.expanded
        divider.isHidden = !TerminalHub.shared.expanded
    }
}

// 누르면 단추는 동작하되 키보드 초점은 가져가지 않는 SwiftUI 호스팅 뷰 — 탭을 누른 뒤에도 입력이 터미널로 가게
final class PassiveHostingView<Content: View>: NSHostingView<Content> {
    override var acceptsFirstResponder: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

// 터미널 영역 — 맨 위 제목 막대 띠, 경로 줄(탭이 있을 때), 탭 줄(지금 경로의 탭), 아래에 탭마다 화면 뷰들(고른 탭의 것만 보임)
final class TerminalAreaView: NSView {
    let container = SurfaceContainerView()
    let empty = PassiveHostingView(rootView: TerminalEmptyView())
    private let workspaceBar = PassiveHostingView(rootView: TerminalWorkspaceBar(hub: TerminalHub.shared))
    private let tabBar = PassiveHostingView(rootView: TerminalTabBar(hub: TerminalHub.shared))
    let settings = SettingsHostingView(rootView: SettingsView())   // 설정 화면 — 제목 막대 띠 아래 전체를 덮는다
    private var settingsMoving = 0   // 전환 효과 차례 — 도는 동안 layout 이 자리를 덮어쓰지 않게
    var titlebar: CGFloat = 0 { didSet { if titlebar != oldValue { needsLayout = true } } }
    var workspaceBarShown = false { didSet { if workspaceBarShown != oldValue { needsLayout = true } } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = terminalBarColor.cgColor
        container.wantsLayer = true
        container.layer?.backgroundColor = terminalAreaColor.cgColor
        workspaceBar.sizingOptions = []
        tabBar.sizingOptions = []
        empty.sizingOptions = []
        addSubview(container)
        addSubview(empty)
        addSubview(workspaceBar)
        addSubview(tabBar)
        settings.sizingOptions = []
        settings.isHidden = true
        addSubview(settings)
    }

    private var settingsFrame: NSRect { NSRect(x: 0, y: 0, width: bounds.width, height: max(0, bounds.height - titlebar)) }

    // 설정 화면 올리기·내리기 — 밀기는 보드 반대쪽 가장자리에서 들어와 그쪽으로 나간다(투명도도 함께). 뒤의 탭은 그대로 돈다
    func present(_ on: Bool, style: SettingsTransition) {
        let base = settingsFrame
        let away = base.offsetBy(dx: TerminalHub.shared.boardOnLeft ? base.width : -base.width, dy: 0)
        settingsMoving += 1
        let turn = settingsMoving
        if on { SettingsScreen.shared.visible = true }
        if on, settings.isHidden {
            settings.frame = style == .slide ? away : base
            settings.alphaValue = style == .none ? 1 : 0
            settings.isHidden = false
        }
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = style == .none ? 0 : style == .slide ? 0.34 : 0.2
            ctx.timingFunction = on ? CAMediaTimingFunction(controlPoints: 0.2, 0.9, 0.3, 1)   // 빠르게 들어와 부드럽게 멈춘다
                                    : CAMediaTimingFunction(controlPoints: 0.4, 0, 0.8, 0.4)
            settings.animator().frame = on || style != .slide ? base : away
            settings.animator().alphaValue = on || style == .none ? 1 : 0
        }, completionHandler: { [weak self] in
            MainActor.assumeIsolated {
                guard let self, turn == self.settingsMoving else { return }
                self.settingsMoving = 0
                if !on {
                    self.settings.isHidden = true
                    SettingsScreen.shared.visible = false
                }
                self.needsLayout = true
            }
        })
    }
    required init?(coder: NSCoder) { fatalError() }

    // 탭 줄의 빈 곳(탭·"새 세션" 단추 오른쪽)이나 경로 줄의 빈 곳(경로 항목 오른쪽)인가 — 창 좌표
    func isTabBarBlank(_ point: NSPoint) -> Bool {
        let p = tabBar.convert(point, from: nil)
        if tabBar.bounds.contains(p) { return p.x > TerminalHub.shared.tabBarEnd }
        let q = workspaceBar.convert(point, from: nil)
        return workspaceBarShown && workspaceBar.bounds.contains(q) && q.x > TerminalHub.shared.workspaceBarEnd
    }

    override func layout() {
        super.layout()
        let top = bounds.height - titlebar
        let ws = workspaceBarShown ? terminalWorkspaceBarHeight : 0
        workspaceBar.frame = NSRect(x: 0, y: top - ws, width: bounds.width, height: ws)
        workspaceBar.isHidden = !workspaceBarShown
        tabBar.frame = NSRect(x: 0, y: top - ws - terminalTabBarHeight, width: bounds.width, height: terminalTabBarHeight)
        container.frame = NSRect(x: 0, y: 0, width: bounds.width, height: max(0, top - ws - terminalTabBarHeight))
        empty.frame = container.frame
        if settingsMoving == 0 { settings.frame = settingsFrame }
    }
}

// 터미널 칸 — 탭마다 화면 뷰들과 그 위에 고른 탭의 나눈 경계. 칸 크기가 바뀌면 화면 자리를 다시 나눈다
final class SurfaceContainerView: NSView {
    private var dividers: [ObjectIdentifier: SplitDividerView] = [:]

    override func resizeSubviews(withOldSize oldSize: NSSize) { TerminalHub.shared.layoutSurfaces() }

    // 고른 탭의 경계들(가지, 가지 자리, 선 자리) — 끄는 동안 경계 뷰가 바뀌지 않게 가지마다 같은 뷰를 쓴다
    func showDividers(_ lines: [(SplitNode, NSRect, NSRect)], color: NSColor) {
        var keep: [ObjectIdentifier: SplitDividerView] = [:]
        for (node, area, line) in lines {
            let key = ObjectIdentifier(node)
            let v = dividers[key] ?? SplitDividerView()
            if v.superview == nil { addSubview(v) }   // 화면 뷰들 위에
            v.node = node
            v.area = area
            v.color = color
            // 잡는 자리는 선 양옆으로 3pt 씩 더(Ghostty 처럼 보이는 선은 1pt)
            let f = node.right ? line.insetBy(dx: -3, dy: 0) : line.insetBy(dx: 0, dy: -3)
            if v.frame != f { v.frame = f; window?.invalidateCursorRects(for: v) }
            keep[key] = v
        }
        for (key, v) in dividers where keep[key] == nil { v.removeFromSuperview() }
        dividers = keep
    }
}

// 나눈 화면 사이 경계 — 끌면 그 가지의 몫이 바뀌고(화면이 80pt 아래로 줄지 않게) 놓으면 남긴다
final class SplitDividerView: NSView {
    weak var node: SplitNode?
    var area: NSRect = .zero   // 그 가지가 차지하는 자리(터미널 칸 좌표)
    var color = NSColor.gray { didSet { if color != oldValue { needsDisplay = true } } }

    override var mouseDownCanMoveWindow: Bool { false }   // 창은 배경을 끌어 옮길 수 있게 되어 있다
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: node?.right == false ? .resizeUpDown : .resizeLeftRight) }

    override func draw(_ dirtyRect: NSRect) {
        color.setFill()
        (node?.right == false ? NSRect(x: 0, y: bounds.midY - 0.5, width: bounds.width, height: 1)
                              : NSRect(x: bounds.midX - 0.5, y: 0, width: 1, height: bounds.height)).fill()
    }

    override func mouseDown(with event: NSEvent) {}

    override func mouseDragged(with event: NSEvent) {
        guard let node, let p = superview?.convert(event.locationInWindow, from: nil) else { return }
        if node.right {
            node.setRatio((p.x - area.minX) / max(area.width - 1, 1), length: area.width)
        } else {
            node.setRatio((area.maxY - p.y) / max(area.height - 1, 1), length: area.height)
        }
        TerminalHub.shared.layoutSurfaces()
    }

    override func mouseUp(with event: NSEvent) { TerminalHub.shared.saveTabs() }
}

// 터미널과 보드 사이 경계 — 끌어서 보드 폭을 바꾸고, 놓으면 기억한다
final class PaneDivider: NSView {
    private var startX: CGFloat = 0
    private var startWidth: CGFloat = 0
    private var dragging = false { didSet { needsDisplay = true } }

    override var mouseDownCanMoveWindow: Bool { false }   // 창은 배경을 끌어 옮길 수 있게 되어 있다
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeLeftRight) }

    override func draw(_ dirtyRect: NSRect) {
        NSColor(red: 0.11, green: 0.11, blue: 0.14, alpha: 1).setFill()
        bounds.fill()
        (dragging ? NSColor(red: 0.48, green: 0.64, blue: 0.97, alpha: 1) : NSColor(white: 0.36, alpha: 1)).setFill()
        NSBezierPath(roundedRect: NSRect(x: bounds.midX - 1.5, y: bounds.midY - 22, width: 3, height: 44), xRadius: 1.5, yRadius: 1.5).fill()
    }

    override func mouseDown(with event: NSEvent) {
        startX = event.locationInWindow.x
        startWidth = boardPaneWidth()
        dragging = true
    }

    override func mouseDragged(with event: NSEvent) {
        let hub = TerminalHub.shared
        let dx = event.locationInWindow.x - startX   // 보드가 왼쪽이면 오른쪽으로 끌수록, 오른쪽이면 왼쪽으로 끌수록 넓어진다
        hub.boardWidth = max(boardMinWidth, min(hub.boardOnLeft ? startWidth + dx : startWidth - dx,
                                                 (window?.frame.width ?? 0) - paneDividerWidth - terminalMinWidth))
        superview?.needsLayout = true
        BoardFit.shared.update()
    }

    override func mouseUp(with event: NSEvent) {
        dragging = false
        UserDefaults.standard.set(Double(TerminalHub.shared.boardWidth), forKey: "boardPaneWidth")
    }
}

struct TabBarEnd: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

struct TabBarNewWidth: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

struct TerminalTabBar: View {
    @ObservedObject var hub: TerminalHub
    @ObservedObject var keys = Shortcuts.shared   // 안내 글자의 단축키
    @State private var newHover = false
    @State private var newWidth: CGFloat = 0   // "새 세션" 단추 폭(앞 여백 포함) — 탭 칸에 줄 폭을 셀 때

    var body: some View {
        GeometryReader { g in
            // 지금 경로의 탭만. 탭 칸은 탭이 모두 들어가면 탭마다 가장 좁게~넓게(120~250) 나눠 갖고, 가장 좁게 해도 넘치면 가로로 밀어 본다
            // (넘친 HStack 은 가운데 맞춤이라 맨 앞의 가장 오래된 탭이 왼쪽 밖으로 잘렸다). 새 세션 단추는 칸 밖이라 늘 보인다
            let tabs = hub.tabs(in: hub.workspace)
            let n = CGFloat(tabs.count)
            let shown = max(0, min(g.size.width - 16 - newWidth, n * (TerminalTabButton.maxWidth + 2) - 2))
            let least = max(0, n * (TerminalTabButton.minWidth + 2) - 2)
            HStack(spacing: 0) {
                ScrollViewReader { proxy in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 2) {
                            ForEach(Array(tabs.enumerated()), id: \.element.id) { i, t in
                                // ⌘ 를 누르고 있으면 탭 줄 순서의 ⌘번호(⌘1~⌘9, 열 번째 탭부터는 없음)
                                TerminalTabButton(tab: t, selected: t.id == hub.selected, background: hub.background,
                                                  shortcut: hub.commandHeld && i < 9 ? i + 1 : nil, commandHeld: hub.commandHeld)
                                    .id(t.id)
                            }
                        }
                        .frame(width: max(shown, least))
                        .frame(maxHeight: .infinity)
                    }
                    .frame(width: shown)
                    // 고른 탭(⌘ 번호·탭 이동 키·새 탭 포함)이 칸 밖이면 보이게 민다. 칸 폭이 바뀔 때도 — 다시 뜰 때는 탭이 그려지기 전에
                    // 고른 탭이 정해져 그때 민 것이 헛돈다(시험 빌드 실측). 창·보드 칸 크기를 바꿀 때도 고른 탭을 놓치지 않게
                    .onAppear { scrollToSelected(proxy) }
                    .onChange(of: hub.selected) { scrollToSelected(proxy, animated: true) }
                    .onChange(of: shown) { scrollToSelected(proxy) }
                }
                newSessionButton
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .frame(width: g.size.width, height: g.size.height)
        }
        .coordinateSpace(name: "tabBar")
        .onPreferenceChange(TabBarEnd.self) { hub.tabBarEnd = $0 }
        .onPreferenceChange(TabBarNewWidth.self) { newWidth = $0 }
        .background(Color(nsColor: terminalBarColor))
        .overlay(alignment: .bottom) { Rectangle().fill(Color.white.opacity(0.07)).frame(height: 1) }
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }

    // 탭이 막 붙었거나 칸 폭이 막 바뀐 때는 그 배치가 끝난 뒤에 민다
    private func scrollToSelected(_ proxy: ScrollViewProxy, animated: Bool = false) {
        guard let id = hub.selected else { return }
        DispatchQueue.main.async {
            if animated { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(id) } } else { proxy.scrollTo(id) }
        }
    }

    private var newSessionButton: some View {
        // 새 세션 — 글자 대신 + 와 단축키(⌘T), 이름은 마우스를 올리면
        Button { hub.newSessionSheet() } label: {
            HStack(spacing: 4) {
                Image(systemName: "plus").font(.system(size: 10, weight: .bold))
                if let key = keys.label("newSession") { Text(key).boardFont(11, weight: .medium).opacity(0.75) }
            }
            .boardFont(12, weight: .semibold)
            .foregroundStyle(.primary.opacity(newHover ? 1 : 0.8))
            .padding(.horizontal, 11)
            .frame(height: 26)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(newHover ? 0.08 : 0)))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.white.opacity(0.22), style: StrokeStyle(lineWidth: 1, dash: [3, 2])))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($newHover)
        .hint("새 세션")
        .padding(.leading, 6)
        // 탭 줄의 빈 곳이 어디부터인지 — 그 오른쪽 더블클릭은 창 크기 전환(isWindowSizeDoubleClick)
        .background(GeometryReader { g in
            Color.clear.preference(key: TabBarEnd.self, value: g.frame(in: .named("tabBar")).maxX)
                .preference(key: TabBarNewWidth.self, value: g.size.width)
        })
    }
}

// 경로 줄 — 탭이 있는 작업 경로들(탭 목록에 처음 나타난 순서). 항목은 상태 점(그 경로 탭들의 세션 가운데 가장 급한 상태)·폴더 이름·탭 수,
// 누르면 그 경로로(마지막에 고른 탭). 빈 곳 더블클릭은 탭 줄처럼 창 크기 전환. 경로를 닫는 단추는 두지 않는다
struct TerminalWorkspaceBar: View {
    @ObservedObject var hub: TerminalHub

    var body: some View {
        GeometryReader { g in
            // 경로가 넘치면 가로로 밀어 본다 — 넘친 HStack 은 가운데 맞춤이라 맨 앞 경로가 왼쪽 밖으로 잘렸다(탭 줄과 같은 문제).
            // 항목은 이름을 줄이지 않고 왼쪽부터 늘어서고, 다 들어가면 그 오른쪽이 빈 곳
            let list = hub.workspaces
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(list, id: \.self) { ws in
                            WorkspaceButton(label: workspaceLabel(ws, among: list), path: ws, count: hub.tabs(in: ws).count,
                                            state: hub.workspaceStates[ws] ?? "idle", current: ws == hub.workspace, background: hub.background)
                                .id(ws)
                        }
                    }
                    // 경로 줄의 빈 곳이 어디부터인지 — 탭 줄과 따로 그리는 호스팅 뷰라 같은 키로 잰다
                    .background(GeometryReader { g in Color.clear.preference(key: TabBarEnd.self, value: g.frame(in: .named("workspaceBar")).maxX) })
                    .frame(minWidth: max(0, g.size.width - 16), maxHeight: .infinity, alignment: .leading)
                }
                // 지금 경로(⌃⌥↑↓·세션 줄 누르기·새 경로 포함)가 칸 밖이면 보이게 민다. 탭 줄처럼 줄 폭이 바뀔 때도
                .onAppear { scrollToCurrent(proxy) }
                .onChange(of: hub.workspace) { scrollToCurrent(proxy, animated: true) }
                .onChange(of: g.size.width) { scrollToCurrent(proxy) }
            }
            .padding(.horizontal, 8)
        }
        .coordinateSpace(name: "workspaceBar")
        .onPreferenceChange(TabBarEnd.self) { hub.workspaceBarEnd = $0 }
        .background(Color(nsColor: terminalBarColor))
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자도 보드 글자 비율을 따른다
    }

    // 경로가 막 붙었거나 줄 폭이 막 바뀐 때는 그 배치가 끝난 뒤에 민다
    private func scrollToCurrent(_ proxy: ScrollViewProxy, animated: Bool = false) {
        guard let ws = hub.workspace else { return }
        DispatchQueue.main.async {
            if animated { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(ws) } } else { proxy.scrollTo(ws) }
        }
    }
}

struct WorkspaceButton: View {
    let label: String
    let path: String
    let count: Int
    let state: String
    let current: Bool        // 지금 경로 — 탭 줄의 고른 탭처럼 터미널 바탕색에 작업 중 색 테두리
    let background: NSColor
    @State private var hover = false

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(Palette.color(state)).frame(width: 7, height: 7)
            Text(label).boardFont(12, weight: .semibold).lineLimit(1)
            Text("\(count)").boardFont(11, weight: .semibold).monospacedDigit().foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .frame(height: 24)
        .background(RoundedRectangle(cornerRadius: 7).fill(current ? Color(nsColor: background) : Color.white.opacity(hover ? 0.06 : 0)))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(current ? Palette.color("working").opacity(0.5) : .clear, lineWidth: 1))
        .foregroundStyle(current ? .primary : .secondary)
        .environment(\.colorScheme, current && isLightColor(background) ? .light : .dark)   // 밝은 테마 바탕이면 글자를 어둡게
        .contentShape(Rectangle())
        .onTapGesture { TerminalHub.shared.showWorkspace(path) }
        .clickable($hover)
        .hint(displayPath(path, home: NSHomeDirectory()))
    }
}

struct TerminalTabButton: View {
    static let minWidth: CGFloat = 120, maxWidth: CGFloat = 250   // 탭 폭 — 탭 줄이 넘칠지 셀 때도 쓴다
    @ObservedObject var tab: BoardTab
    @ObservedObject var keys = Shortcuts.shared   // 안내 글자의 단축키
    let selected: Bool
    let background: NSColor
    let shortcut: Int?       // ⌘ 를 누르고 있는 동안 × 자리에 보일 ⌘번호
    let commandHeld: Bool
    @State private var hover = false
    @State private var closeHover = false

    var body: some View {
        HStack(spacing: 8) {
            // 상태 색은 화면들의 세션 가운데 가장 급한 것, 제목은 고른 화면의 세션 — 셸 화면이나 아직 요청 전이면 그 화면의 터미널 제목
            Circle().fill(Palette.color(tab.state)).frame(width: 8, height: 8)
            if tab.renaming {
                TabNameField(tab: tab).frame(width: 120)
            } else {
                Text(tab.shownName).boardFont(13, weight: .semibold).lineLimit(1).fixedSize()
            }
            // 고른 화면 하나 크게 보는 중(⇧⌘↩)
            if tab.zoomed {
                Image(systemName: "arrow.up.left.and.arrow.down.right").font(.system(size: 10, weight: .bold))
                    .foregroundStyle(Palette.color("working"))
                    .hint("화면 하나 크게 보는 중: \(keys.label("zoom").map { "\($0) 로 " } ?? "윈도우 > 화면 나누기에서 ")되돌립니다")
            }
            // ⌘ 를 누르고 있는 동안 고른 탭은 지금 하는 일 대신 이름 바꾸기 단축키
            if commandHeld && selected, let key = keys.label("rename") {
                Text("\(key) 이름 바꾸기").boardFont(11).foregroundStyle(.secondary).lineLimit(1)
            } else {
                Text((tab.focused.parked ?? tab.focused.session).map(doing) ?? tab.focused.title).boardFont(12).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
            if let n = shortcut, let key = keys.label("tab\(n)") {
                // 폭은 × 와 같게 두어 탭 폭이 흔들리지 않게(글자는 넘쳐 그린다)
                Text(key).boardFont(10, weight: .semibold).foregroundStyle(.secondary)
                    .fixedSize().frame(width: 16, height: 16)
            } else {
                Button { TerminalHub.shared.requestClose(tab) } label: {
                    Image(systemName: "xmark").font(.system(size: 8, weight: .bold))
                        .foregroundStyle(closeHover ? .primary : .secondary)
                        .frame(width: 16, height: 16)
                        .background(Circle().fill(Color.white.opacity(closeHover ? 0.14 : 0)))
                }
                .buttonStyle(.plain)
                .opacity(hover || selected ? 1 : 0)
                .onHover { closeHover = $0 }
                .hint("탭 닫기")
            }
        }
        .padding(.leading, 14).padding(.trailing, 8)
        .frame(minWidth: Self.minWidth, maxWidth: Self.maxWidth, maxHeight: .infinity)
        .background(selected ? Color(nsColor: background) : Color.white.opacity(hover ? 0.04 : 0))
        .overlay(alignment: .top) { Rectangle().fill(selected ? Palette.color("working") : .clear).frame(height: 2) }
        .foregroundStyle(selected ? .primary : .secondary)
        .environment(\.colorScheme, selected && isLightColor(background) ? .light : .dark)   // 밝은 테마 바탕이면 글자·아이콘을 어둡게
        .contentShape(Rectangle())
        .onTapGesture { if !tab.renaming { TerminalHub.shared.select(tab.id) } }
        .onHover { hover = $0 }
    }
}

// 탭 이름 입력 칸 — 탭 줄은 키 초점을 받지 않는 호스팅 뷰(PassiveHostingView)라 AppKit 입력 칸을 직접 첫 응답자로 세운다
// (처음 값은 탭 줄에 보이는 이름, 첫 응답자가 되면 전체 선택). Return 저장, Esc 취소, 다른 곳을 눌러 포커스를 잃으면 저장
struct TabNameField: NSViewRepresentable {
    let tab: BoardTab

    func makeNSView(context: Context) -> NSTextField {
        let f = NSTextField(string: tab.shownName)
        f.isBordered = false
        f.drawsBackground = false
        f.focusRingType = .none
        f.font = .systemFont(ofSize: 13 * BoardFontScale.shared.scale, weight: .semibold)   // 탭 이름 글자와 같게(보드 글자 비율)
        f.cell?.isScrollable = true
        f.cell?.wraps = false
        f.delegate = context.coordinator
        DispatchQueue.main.async { f.window?.makeFirstResponder(f) }   // 창에 붙은 뒤에
        return f
    }

    func updateNSView(_ f: NSTextField, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(tab: tab) }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        let tab: BoardTab
        init(tab: BoardTab) { self.tab = tab }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.insertNewline(_:)): end(control.stringValue)
            case #selector(NSResponder.cancelOperation(_:)): end(nil)
            default: return false
            }
            return true
        }

        // 포커스를 잃음 — 포커스가 옮겨 가는 도중이라 끝난 뒤에 저장한다(저장이 다시 포커스를 옮긴다). Return·Esc 로 이미 끝났으면 아무 일 없다
        func controlTextDidEndEditing(_ note: Notification) {
            let name = (note.object as? NSTextField)?.stringValue
            DispatchQueue.main.async { self.end(name) }
        }

        private func end(_ name: String?) { MainActor.assumeIsolated { TerminalHub.shared.endRename(tab, name: name) } }
    }
}

// 새 세션 기본 명령 — 승인 대기가 거의 없게 권한 확인을 건너뛰는 것도 의도한 것(2026-10-17 사용자).
// 설정 > 터미널 > 새 세션 의 기본 명령(newSessionCommand)이 있으면 그것을 쓴다
let builtinSessionCommand = "claude --dangerously-skip-permissions"
var defaultSessionCommand: String {
    let s = UserDefaults.standard.string(forKey: "newSessionCommand")?.trimmingCharacters(in: .whitespaces) ?? ""
    return s.isEmpty ? builtinSessionCommand : s
}

struct NewSessionSheet: View {
    let paths: [String]
    let cancel: () -> Void
    let start: (String, String, Bool) -> Void   // 명령, 작업 폴더, 답변 마크다운 꾸미기
    @ObservedObject private var hub = TerminalHub.shared
    @ObservedObject private var fontScale = BoardFontScale.shared
    @State private var dir: String
    @State private var others: [String] = []
    @State private var command = defaultSessionCommand
    @State private var mdRender = UserDefaults.standard.bool(forKey: "newSessionMdRender")   // 명령처럼 열 때마다 기본값(설정 > 터미널)

    init(paths: [String], initial: String, cancel: @escaping () -> Void, start: @escaping (String, String, Bool) -> Void) {
        self.paths = paths
        self.cancel = cancel
        self.start = start
        _dir = State(initialValue: initial)
        _others = State(initialValue: paths.contains(initial) ? [] : [initial])
    }

    private func row(_ p: String) -> some View {
        let on = p == dir
        return Button { dir = p } label: {
            HStack(spacing: 10) {
                Image(systemName: on ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(on ? Palette.color("working") : .secondary)
                VStack(alignment: .leading, spacing: 1) {
                    Text((p as NSString).lastPathComponent).boardFont(13, weight: .semibold)
                    Text(displayPath(p, home: NSHomeDirectory())).boardFont(11).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 8).fill(on ? Palette.color("working").opacity(0.16) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func pick() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.directoryURL = URL(fileURLWithPath: dir)
        guard panel.runModal() == .OK, let p = panel.url?.path else { return }
        if !paths.contains(p), !others.contains(p) { others.append(p) }
        dir = p
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("새 세션").boardFont(16, weight: .bold)
            VStack(alignment: .leading, spacing: 6) {
                Text("작업 폴더 (보드에 있는 경로)").boardFont(12, weight: .semibold).foregroundStyle(.secondary)
                ScrollView {
                    VStack(spacing: 2) { ForEach(paths + others, id: \.self) { row($0) } }
                }
                // 스크롤 영역은 높이를 정해 주지 않으면 0 으로 접힌다. 줄 높이는 보드 글자 비율을 따른다(150% 에서 둘째 줄이 잘렸다)
                .frame(height: min(CGFloat(paths.count + others.count) * 46, 230) * fontScale.scale)
                Button("다른 폴더 고르기…", action: pick)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("실행할 명령").boardFont(12, weight: .semibold).foregroundStyle(.secondary)
                TextField("", text: $command).boardFont(13, design: .monospaced).textFieldStyle(.roundedBorder)
                Text("작업 경로의 셸에 쳐 넣어 실행합니다. claude 면 보드가 --session-id 를 붙입니다")
                    .boardFont(11).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 4) {
                Toggle("답변 마크다운 꾸미기", isOn: $mdRender).toggleStyle(.checkbox).boardFont(13)
                    .disabled(mdRenderDir == nil)
                Text("답변이 끝나면 제목·굵게·인라인 코드·목록 등을 색으로 다시 그립니다(md-render). claude 면 --plugin-dir 로 붙입니다")
                    .boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                Button("취소", action: cancel).keyboardShortcut(.cancelAction)
                Button("새 탭에서 시작") { start(command.trimmingCharacters(in: .whitespaces), dir, mdRender) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(command.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(18)
        .frame(width: 460)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }
}

// 터미널 테마 목록(설정 > 터미널 > 테마) — 엔진에 든 테마(485개)를 이름으로 찾고 색 띠·견본으로 미리 본다.
// 줄을 누르면 바로 그 테마로 바꾼다(열린 탭 모두). "Ghostty 설정 따르기" 는 보드 설정을 지워 Ghostty 설정의 theme 을 쓴다
@MainActor final class ThemeList: ObservableObject {
    static let shared = ThemeList()
    // 이름순(대소문자 무시). GhosttyThemeCatalog.search("") 는 빈 글자를 Foundation 의 contains 로 견주어 하나도 돌려주지 않는다(실측)
    let themes = GhosttyThemeCatalog.allThemes.sorted { $0.name.caseInsensitiveCompare($1.name) == .orderedAscending }
    @Published var query = ""
    @Published private(set) var current: String?    // 지금 쓰는 테마(보드 설정 terminalTheme) — nil 은 "Ghostty 설정 따르기"
    private(set) var followed: GhosttyThemeDefinition?   // Ghostty 설정을 따를 때의 테마(색 띠·미리보기) — 설정 화면을 열 때 읽는다
    private(set) var followedName = ""

    // 보이는 줄 — nil 은 "Ghostty 설정 따르기"(검색어가 있으면 숨긴다). 이름 대소문자 무시 부분 일치
    var rows: [GhosttyThemeDefinition?] {
        let q = query.trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? [nil] + themes : themes.filter { $0.name.localizedCaseInsensitiveContains(q) }
    }

    func definition(_ name: String?) -> GhosttyThemeDefinition? { name.map { n in themes.first { $0.name == n } } ?? followed }

    func load() {
        current = UserDefaults.standard.string(forKey: "terminalTheme")
        query = ""
        followed = themeDefinitions(ghosttyConfigValue("theme"))?.dark ?? GhosttyThemeCatalog.theme(named: "Afterglow")
        followedName = ghosttyThemeName()
    }

    func pick(_ name: String?) {
        guard name != current else { return }
        current = name
        TerminalHub.shared.setTerminalTheme(name)
    }
}

// 테마 정의의 색 글자(RRGGBB, 앞의 # 은 있어도 된다) → 색. 없거나 읽지 못하면 회색
func themeColor(_ hex: String?) -> Color {
    guard let hex, let n = UInt32(hex.replacingOccurrences(of: "#", with: "").prefix(6), radix: 16) else { return .gray }
    return Color(red: Double(n >> 16 & 0xFF) / 255, green: Double(n >> 8 & 0xFF) / 255, blue: Double(n & 0xFF) / 255)
}

// 테마 목록(왼쪽, 검색·스크롤) + 고른 테마의 견본(오른쪽). 폭이 좁으면 견본을 아래로
struct ThemeListView: View {
    @ObservedObject var model: ThemeList

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 12) {
                list.frame(minWidth: 240)
                preview(model.definition(model.current)).frame(width: 250)
            }
            VStack(spacing: 12) {
                list
                preview(model.definition(model.current))
            }
        }
    }

    private var list: some View {
        VStack(spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass").font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
                TextField("테마 이름 검색", text: $model.query).textFieldStyle(.plain).boardFont(12)
            }
            .padding(.horizontal, 8).frame(height: 26)
            .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(0.06)))
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 1) { ForEach(model.rows, id: \.?.name) { row($0) } }.padding(3)
                }
                .frame(height: 236)
                .onAppear { proxy.scrollTo(model.current, anchor: .center) }
            }
            .background(RoundedRectangle(cornerRadius: 9).fill(Color.black.opacity(0.2)))
            .overlay(RoundedRectangle(cornerRadius: 9).stroke(Color.white.opacity(0.07), lineWidth: 1))
        }
    }

    // 이름(+ "Ghostty 설정 따르기" 는 지금 그것이 무엇인지), 지금 쓰는 것에 체크, 오른쪽에 바탕색 칩 + palette 0~7 칩
    private func row(_ t: GhosttyThemeDefinition?) -> some View {
        ThemeRow(theme: t, colors: t ?? model.followed, followedName: model.followedName, on: t?.name == model.current) { model.pick(t?.name) }
    }

    private struct ThemeRow: View {
        let theme: GhosttyThemeDefinition?
        let colors: GhosttyThemeDefinition?
        let followedName: String
        let on: Bool
        let pick: () -> Void
        @State private var hover = false
        var body: some View {
            HStack(spacing: 8) {
                Image(systemName: "checkmark").font(.system(size: 9, weight: .bold)).foregroundStyle(SettingsStyle.accent)
                    .opacity(on ? 1 : 0).frame(width: 10)
                VStack(alignment: .leading, spacing: 1) {
                    Text(theme?.name ?? "Ghostty 설정 따르기").boardFont(12, weight: on ? .semibold : .regular).lineLimit(1)
                    if theme == nil { Text(followedName).boardFont(10.5).foregroundStyle(.secondary).lineLimit(1) }
                }
                Spacer(minLength: 8)
                HStack(spacing: 3) {
                    RoundedRectangle(cornerRadius: 3).fill(themeColor(colors?.background)).frame(width: 16, height: 12)
                        .overlay(RoundedRectangle(cornerRadius: 3).stroke(Color.white.opacity(0.2), lineWidth: 0.5))
                    HStack(spacing: 0) {
                        ForEach(0..<8, id: \.self) { i in Rectangle().fill(themeColor(colors?.palette[i])).frame(width: 7, height: 12) }
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 3))
                }
            }
            .padding(.horizontal, 7).padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 6).fill(on ? SettingsStyle.accent.opacity(0.18) : Color.white.opacity(hover ? 0.05 : 0)))
            .contentShape(Rectangle())
            .onTapGesture(perform: pick)
            .clickable($hover)
            .id(theme?.name)
        }
    }

    // 고른 테마의 색으로 그린 터미널 견본 — 프롬프트, 색 있는 ls 출력, diff 의 - · + 줄, 커서
    private func preview(_ t: GhosttyThemeDefinition?) -> some View {
        let fg = themeColor(t?.foreground)
        func p(_ i: Int) -> Color { themeColor(t?.palette[i] ?? t?.foreground) }
        func prompt(_ command: String) -> some View {
            HStack(spacing: 0) {
                Text("~/board").foregroundStyle(p(4))
                Text(" on ").foregroundStyle(fg)
                Text("main").foregroundStyle(p(5))
                Text(" ❯ ").foregroundStyle(p(2))
                Text(command).foregroundStyle(fg)
            }
        }
        return VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 5) {
                ForEach([Color(red: 1, green: 0.37, blue: 0.34), Color(red: 1, green: 0.74, blue: 0.18), Color(red: 0.16, green: 0.79, blue: 0.25)], id: \.self) {
                    Circle().fill($0).frame(width: 7, height: 7)
                }
                Spacer()
                Text(t?.name ?? "").boardFont(10, weight: .semibold).foregroundStyle(fg.opacity(0.6)).lineLimit(1)
            }
            .padding(.bottom, 6)
            prompt("ls")
            HStack(spacing: 0) {
                Text("docs").foregroundStyle(p(4)).bold()
                Text("  build.sh").foregroundStyle(p(2))
                Text("  README.md").foregroundStyle(fg)
            }
            prompt("git diff")
            Text("- theme = \"Ghostty\"").foregroundStyle(p(1))
            Text("+ theme = \"\(t?.name ?? "")\"").foregroundStyle(p(2))
            HStack(spacing: 0) {
                prompt("")
                Text(" ").background(themeColor(t?.cursorColor ?? t?.foreground))
            }
        }
        .lineLimit(1)
        .boardFont(11, design: .monospaced)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(themeColor(t?.background)))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .animation(.easeOut(duration: 0.2), value: t?.name)
    }
}

// 보드 아래쪽에 잠깐 뜨는 안내 — 백그라운드 세션을 눌렀을 때 등
struct TerminalNotice: View {
    @ObservedObject var hub: TerminalHub
    @State private var hover = false
    var body: some View {
        if let text = hub.notice {
            HStack(alignment: .top, spacing: 10) {
                Text(text).boardFont(12).fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
                Button { hub.dismissNotice() } label: {
                    Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                        .frame(width: 22, height: 22)
                        .background(Circle().fill(Color.white.opacity(hover ? 0.2 : 0.1)))
                }
                .buttonStyle(.plain).clickable($hover)
            }
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 12).fill(Color(red: 0.18, green: 0.16, blue: 0.24)))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color(red: 0.36, green: 0.30, blue: 0.48), lineWidth: 1))
            .shadow(color: .black.opacity(0.4), radius: 12, y: 6)
            .padding(.horizontal, 14).padding(.bottom, 34)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .preferredColorScheme(.dark)
        }
    }
}

// 탭이 하나도 없을 때
struct TerminalEmptyView: View {
    @State private var hover = false
    var body: some View {
        VStack(spacing: 12) {
            Text("보드에서 띄운 세션이 없습니다").boardFont(14, weight: .semibold)
            Text("보드에서 시작하거나 이어 연 세션이 여기 탭으로 열립니다").boardFont(12).foregroundStyle(.secondary)
            Button { TerminalHub.shared.newSessionSheet() } label: {
                HStack(spacing: 6) { Image(systemName: "plus"); Text("새 세션") }
                    .boardFont(12, weight: .semibold)
                    .padding(.horizontal, 14).frame(height: 30)
                    .background(Capsule().fill(Color.white.opacity(hover ? 0.16 : 0.1)))
            }
            .buttonStyle(.plain)
            .clickable($hover)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }
}

// 제목 막대의 "터미널" 단추 — 미니 제목 막대(폭 300)에는 자리가 없어 숨긴다
struct TerminalToggleButton: View {
    @ObservedObject private var hub = TerminalHub.shared
    @ObservedObject var keys = Shortcuts.shared   // 안내 글자의 단축키
    @State private var hover = false
    var body: some View {
        let on = hub.expanded
        Button(action: toggleTerminal) {
            HStack(spacing: 5) {
                Image(systemName: hub.boardOnLeft ? "rectangle.trailinghalf.inset.filled" : "rectangle.leadinghalf.inset.filled")
                Text(on ? "터미널 펼침" : "터미널 접힘").fixedSize()
                if let key = keys.label("terminal") {
                    Text(key).font(.system(size: 10, weight: .medium))
                        .padding(.horizontal, 4)
                        .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(0.08)))
                }
            }
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(on ? Palette.color("working") : .primary.opacity(hover ? 1 : 0.8))
            .padding(.horizontal, 8)
            .frame(height: 20)
            .background(Capsule().fill(on ? Palette.color("working").opacity(0.18) : Color.white.opacity(hover ? 0.30 : 0.18)))
            .overlay(Capsule().stroke(on ? Palette.color("working").opacity(0.45) : Color.white.opacity(0.18), lineWidth: 0.5))
            .frame(height: 28)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint((on ? "터미널 접기: 보드만 \(hub.boardOnLeft ? "왼쪽" : "오른쪽")에 남깁니다"
                  : "터미널 펼치기: 보드에서 띄운 세션을 \(hub.boardOnLeft ? "오른쪽" : "왼쪽") 탭에서 봅니다") + (keys.label("terminal").map { " (\($0))" } ?? ""))
        .fixedSize()
    }
}
let terminalToggleWidth: CGFloat = 146
var terminalToggleSink: AnyCancellable?

@MainActor func addTerminalToggle() {
    let vc = NSTitlebarAccessoryViewController()
    let host = FirstClickHostingView(rootView: TerminalToggleButton())
    host.frame = NSRect(x: 0, y: 0, width: terminalToggleWidth, height: 28)
    vc.view = host
    vc.layoutAttribute = .leading
    window.addTitlebarAccessoryViewController(vc)
    // 왼쪽 부속에는 vc.isHidden 이 듣지 않는다 — 뷰를 숨기고 폭을 0 으로 줄여 자리도 비운다
    terminalToggleSink = BoardFit.shared.$tier.sink { t in
        host.isHidden = t == .mini
        host.frame.size.width = t == .mini ? 0 : terminalToggleWidth
    }
}
