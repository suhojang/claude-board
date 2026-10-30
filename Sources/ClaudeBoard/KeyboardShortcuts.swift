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

// MARK: - 단축키

// 판정(키 하나 KeyCombo·글자 label·menuKey·동작 표·사용자가 바꾼 키의 해석과 고치기·겹침·키 받기·시스템 단축키와 겹침)은 BoardRules/Shortcuts.swift,
// 포트는 KeyboardLayout(가짜 어댑터는 Tests 에만). 여기는 어댑터와 NSEvent·Carbon 변환, @Published 상태·UserDefaults·메뉴·전역 단축키다

// 지금 ASCII 자판의 글자 — Carbon UCKeyTranslate(asciiKeyCharacters)
struct SystemKeyboardLayout: KeyboardLayout {
    func characters(forKeyCode code: UInt16, shift: Bool) -> String? { asciiKeyCharacters(code, shift: shift) }
}

extension KeyModifiers {
    // raw 값이 NSEvent.ModifierFlags 와 같은 비트라 그대로 옮긴다(⌘⌃⌥⇧ 밖 비트는 KeyCombo 가 버린다)
    init(_ flags: NSEvent.ModifierFlags) { self.init(rawValue: flags.rawValue) }
    var eventFlags: NSEvent.ModifierFlags { NSEvent.ModifierFlags(rawValue: rawValue) }
}

extension KeyCombo {
    init(_ e: NSEvent) { self.init(e.keyCode, KeyModifiers(e.modifierFlags)) }

    // 화면에 보이는 키·메뉴 keyEquivalent — 글자는 지금 ASCII 자판에서
    var label: String { label(layout: SystemKeyboardLayout()) }
    var menuKey: String { menuKey(layout: SystemKeyboardLayout()) }

    // 전역 단축키(RegisterEventHotKey)·시스템 단축키 목록의 수정키
    var carbonMods: UInt32 {
        var m = 0
        if mods.contains(.command) { m |= cmdKey }
        if mods.contains(.control) { m |= controlKey }
        if mods.contains(.option) { m |= optionKey }
        if mods.contains(.shift) { m |= shiftKey }
        return UInt32(m)
    }
}

// 보드 단축키 — 표(ShortcutTable)에 사용자가 바꾼 키를 얹어 쥐고, 바뀌면 보드 설정 shortcuts 에 남기고 메뉴·전역 단축키를 다시 만든다.
// 화면의 안내 글자는 @Published 로 따라 바뀐다
@MainActor final class Shortcuts: ObservableObject {
    static let shared = Shortcuts()

    static var actions: [ShortcutAction] { ShortcutTable.actions }

    // 저장값은 항목마다 따로 검증한다 — 이상한 항목은 그 동작만 기본값으로 읽고(다음 저장 때 사라진다) 멀쩡한 항목은 지킨다
    @Published private(set) var table = ShortcutTable(overrides: parseShortcutOverrides(UserDefaults.standard.dictionary(forKey: "shortcuts")))
    var overrides: [String: [Int]] { table.overrides }
    @Published private(set) var hotKeyFailed: Set<String> = []   // 전역 단축키로 등록하지 못한 동작
    private var items: [String: NSMenuItem] = [:]
    private var hotKeyRefs: [EventHotKeyRef] = []
    private var handlerInstalled = false

    func action(_ id: String) -> ShortcutAction? { ShortcutTable.action(id) }

    func combo(_ id: String) -> KeyCombo? { table.combo(id) }

    func label(_ id: String) -> String? { combo(id)?.label }

    // 기본값과 같은 키로 돌아오면 남긴 것을 지운다. nil 은 비우기
    func set(_ id: String, _ c: KeyCombo?) {
        table.set(id, c)
        save()
    }

    func resetAll() {
        table.resetAll()
        save()
    }

    // 메뉴를 다시 만들고 전역 단축키를 다시 등록한다 — 화면의 안내 글자는 @Published 로 따라 바뀐다
    private func save() {
        UserDefaults.standard.set(overrides, forKey: "shortcuts")
        (NSApp.delegate as? AppDelegate)?.buildMainMenu()
        registerHotKeys(paused: ShortcutSettings.shared.recording != nil)   // 키를 받는 중에 되돌리기 등을 눌렀으면 풀어 둔 채로
    }

    // 메뉴 항목에 그 동작의 키를 단다. 앱 안 키 감시(handle)도 이 항목의 동작을 부른다
    func bind(_ item: NSMenuItem, _ id: String) {
        let c = combo(id)
        item.keyEquivalent = c?.menuKey ?? ""
        item.keyEquivalentModifierMask = c?.mods.eventFlags ?? []
        items[id] = item
    }

    // 같은 키를 쓰는 다른 동작 — 메뉴는 앞의 것만 받는다
    func conflicts(_ id: String) -> [String] { table.conflicts(id) }

    // 앱 안 키 감시 — 표의 키를 키 번호로 맞춰 그 메뉴 동작을 바로 부른다. 그래서 한글 입력기에서 ⌃⌥ + 글자 키가 메뉴에 맞지 않는 일도,
    // 터미널에 포커스가 있을 때 엔진이 사용자가 고른 키를 먼저 가져가는 일도 없다(엔진의 unbind 목록을 사용자 키마다 늘리지 않는다).
    // 설정 화면이 키를 받는 중이면 그쪽으로. 모달 창(NSAlert 등)·⋯ 메뉴 패널(SessionMenuPanel, 키를 스스로 받는다)이 떠 있거나
    // 글자 칸에서 한글을 조합하는 중이면 그대로 둔다
    func handle(_ e: NSEvent) -> NSEvent? {
        if ShortcutSettings.shared.recording != nil { return ShortcutSettings.shared.record(e) }
        guard NSApp.modalWindow == nil, !(e.window is NSPanel),
              (e.window?.firstResponder as? NSTextView)?.hasMarkedText() != true else { return e }
        let c = KeyCombo(e)
        // ⌘, 는 엔진 기본값(open_config)이 먼저 가져가므로 여기서 받는다
        if c == ShortcutTable.settingsKey { SettingsScreen.shared.toggle(); return nil }
        // 설정 화면의 Esc — 검색어를 지우고, 없으면 닫는다(키 받기 취소는 위에서)
        if c == KeyCombo(53, []), SettingsScreen.shared.shown, e.window === window { SettingsScreen.shared.escape(); return nil }
        // 설정 화면의 ⌘W — 보드 창을 닫지 않고 설정 화면을 닫는다
        if c == KeyCombo(13, .command), SettingsScreen.shared.shown, e.window === window { SettingsScreen.shared.close(); return nil }
        guard let id = table.actionID(for: c), let item = items[id], let sel = item.action else { return e }
        NSApp.sendAction(sel, to: item.target, from: item)
        return nil
    }

    // 전역 단축키 — 처음 크기로 붙이기(기본 ⌃⌥B): 어느 앱에서든 보드를 처음 크기로 보드 쪽 끝(오른쪽·왼쪽)에 붙이고 앞으로 꺼낸다(포커스는
    // 지금 앱에 남긴다). 터미널 펼치기·접기(기본 ⌃⌥T). Carbon 단축키는 손쉬운 사용 권한이 필요 없다.
    // 표가 바뀔 때마다 다시 등록하고, 설정 화면이 키를 받는 동안은 풀어 둔다(paused — 누른 키가 동작하지 않고 그쪽에 들어가게)
    func registerHotKeys(paused: Bool = false) {
        if !handlerInstalled {
            handlerInstalled = true
            var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
            InstallEventHandler(GetApplicationEventTarget(), { _, event, _ in
                var key = EventHotKeyID()
                GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
                                  MemoryLayout<EventHotKeyID>.size, nil, &key)
                DispatchQueue.main.async {
                    if key.id == 2 {
                        toggleTerminal()
                    } else {
                        window.orderFrontRegardless()
                        snapToSide()
                    }
                }
                return noErr
            }, 1, &spec, nil, nil)
        }
        hotKeyRefs.forEach { UnregisterEventHotKey($0) }
        hotKeyRefs = []
        guard !paused else { return }
        var failed: Set<String> = []
        for (n, id) in [(UInt32(1), "snap"), (UInt32(2), "terminal")] {
            guard let c = combo(id) else { continue }
            var ref: EventHotKeyRef?
            let hid = EventHotKeyID(signature: OSType(0x43424F44), id: n)   // 'CBOD'
            let status = RegisterEventHotKey(UInt32(c.code), c.carbonMods, hid, GetApplicationEventTarget(), 0, &ref)
            // 다른 앱이 먼저 잡은 단축키면 등록에 실패한다 — 보드는 그대로 동작하고 로그를 남기고 설정 화면에 알린다
            if status == noErr, let ref { hotKeyRefs.append(ref) } else {
                failed.insert(id)
                boardLog("전역 단축키 \(c.label) 등록 실패 (\(status))")
            }
        }
        hotKeyFailed = failed
    }
}

// macOS 시스템 단축키(시스템 설정 > 키보드 > 키보드 단축키 가운데 켜 둔 것) — [키 번호, Carbon 수정키]. 다른 앱이 잡은 키는 알 길이 없다
func systemHotKeys() -> [[Int]] {
    var ref: Unmanaged<CFArray>?
    guard CopySymbolicHotKeys(&ref) == noErr, let list = ref?.takeRetainedValue() as? [[String: Any]] else { return [] }
    let mask = cmdKey | shiftKey | optionKey | controlKey
    return list.compactMap { d in
        guard (d[kHISymbolicHotKeyEnabled as String] as? Bool) == true, let code = d[kHISymbolicHotKeyCode as String] as? Int,
              let mods = d[kHISymbolicHotKeyModifiers as String] as? Int else { return nil }
        return [code, mods & mask]
    }
}

// 단축키 받기(설정 > 단축키) — 줄의 키를 누르면 새 키를 받는다(Esc 취소, ⌫ 비우기). 바꾸면 바로 메뉴·전역 단축키·화면 안내 글자에
// 반영된다. 보드 안에서 겹치면 받아 두고 상대를 비울지 그 줄에서 묻고, 시스템 단축키와 겹치면 알린다
@MainActor final class ShortcutSettings: ObservableObject {
    static let shared = ShortcutSettings()
    @Published private(set) var recording: String?   // 새 키를 받는 중인 동작
    @Published private(set) var refused: String?     // 받지 않은 키 안내
    private(set) var systemKeys: [[Int]] = []         // 설정 화면을 열 때 읽는다

    func load() {
        refused = nil
        systemKeys = systemHotKeys()
    }

    func systemTaken(_ c: KeyCombo) -> Bool { systemShortcutTaken(c, carbonMods: c.carbonMods, system: systemKeys) }

    // 받는 동안은 전역 단축키를 풀어 둔다 — 누른 키가 동작하지 않고 여기로 오게
    func startRecording(_ id: String) {
        recording = id
        refused = nil
        Shortcuts.shared.registerHotKeys(paused: true)
    }

    func stopRecording() {
        recording = nil
        Shortcuts.shared.registerHotKeys()
    }

    // 받는 중에 누른 키 — Esc 취소, ⌫·⌦ 비우기. ⌘·⌃·⌥ 가 없는 키(F1~F12 는 된다)와 고를 수 없는 키는 받지 않고 알린다(판정은 recordShortcut)
    func record(_ e: NSEvent) -> NSEvent? {
        guard e.type == .keyDown, let id = recording else { return e }
        switch recordShortcut(KeyCombo(e), layout: SystemKeyboardLayout()) {
        case .cancel:
            stopRecording()
        case .clear:
            stopRecording()
            Shortcuts.shared.set(id, nil)
        case .refuse(let text):
            refused = text
        case .accept(let c):
            refused = nil
            recording = nil
            Shortcuts.shared.set(id, c)   // 전역 단축키도 여기서 다시 등록된다
        }
        return nil
    }
}
