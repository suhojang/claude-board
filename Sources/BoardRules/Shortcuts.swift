// 보드 단축키 판정 — 키 하나(KeyCombo)와 그 글자, 바꿀 수 있는 동작 표, 사용자가 바꾼 키(보드 설정 shortcuts)의 해석·고치기·겹침, 키 받기 판정,
// 시스템 단축키와 겹치는지(앱의 // MARK: - 단축키 에 있던 것을 옮김, 동작은 그대로). 바깥(지금 ASCII 자판의 글자)은 포트(KeyboardLayout)로만 부른다.
//
// 앱(Shortcuts·ShortcutSettings)에 남는 것: @Published 상태와 UserDefaults 읽고 쓰기, 메뉴 다시 만들기·NSMenuItem 에 키 달기,
// Carbon 전역 단축키 등록과 Carbon 수정키(carbonMods), NSEvent 와 오가는 변환, 키 감시(handle)의 NSEvent 분기, 시스템 단축키 목록 읽기(CopySymbolicHotKeys).
// 사용자가 바꾼 키는 값([id: [키 번호, 수정키 raw]])으로 받고 고친 값을 돌려준다 — 앱이 이미 @Published 로 쥐고 저장하므로
// 읽고 쓰기를 포트로 빼면 결정 없는 UserDefaults 한 줄씩만 감싸게 되고, 해석·고치기 규칙은 값만으로 모두 시험할 수 있다.
// 옮긴 뒤 고친 것(이슈 #9 2차): 저장값은 항목마다 검증해(parseShortcutOverrides) 이상한 항목만 기본값으로 읽고, 범위 밖 숫자에 멈추지 않는다
import Foundation

// MARK: - 키 하나

/// 수정키 — ⌘⌃⌥⇧ 만. raw 값은 NSEvent.ModifierFlags 와 같은 비트다.
/// 보드 설정 shortcuts 에 이 raw 값이 [키 번호, 수정키 raw] 로 남아 있다 — 동작 id 를 바꾸면 사용자가 바꾼 키가 사라지는 것과 같은 까닭으로
/// raw 값도 1비트라도 바꾸면 안 된다(시험이 숫자로 고정한다)
public struct KeyModifiers: OptionSet, Hashable, Sendable {
    public let rawValue: UInt
    public init(rawValue: UInt) { self.rawValue = rawValue }

    public static let shift = KeyModifiers(rawValue: 1 << 17)     // 131072 = NSEvent.ModifierFlags.shift
    public static let control = KeyModifiers(rawValue: 1 << 18)   // 262144 = NSEvent.ModifierFlags.control
    public static let option = KeyModifiers(rawValue: 1 << 19)    // 524288 = NSEvent.ModifierFlags.option
    public static let command = KeyModifiers(rawValue: 1 << 20)   // 1048576 = NSEvent.ModifierFlags.command
    /// 키 하나에 남기는 수정키 — 캡스록·fn·숫자판 같은 다른 비트는 버린다
    public static let keys: KeyModifiers = [.command, .control, .option, .shift]
}

/// 키 하나 — 키 번호(자판 위치)와 수정키(⌘⌃⌥⇧ 만). 글자가 아니라 키 번호로 맞추므로 한글 입력기에서도 같은 키다
public struct KeyCombo: Hashable, Sendable {
    public var code: UInt16
    public var mods: KeyModifiers

    /// 수정키는 ⌘⌃⌥⇧ 만 남긴다
    public init(_ code: UInt16, _ mods: KeyModifiers) {
        self.code = code
        self.mods = mods.intersection(.keys)
    }

    /// 글자로 나오지 않는 키 — 화면에 보일 기호와 메뉴 keyEquivalent 글자. 글자는 AppKit 의 NS…FunctionKey 값(시험이 AppKit 상수와 견준다)
    public static let specialKeys: [Int: (label: String, key: String)] = {
        func fk(_ v: Int) -> String { String(Character(UnicodeScalar(v)!)) }
        var m: [Int: (label: String, key: String)] = [
            36: ("↩", "\r"), 76: ("⌤", "\u{3}"), 48: ("⇥", "\t"), 49: ("Space", " "), 51: ("⌫", "\u{8}"),
            117: ("⌦", fk(0xF728)), 115: ("↖", fk(0xF729)), 119: ("↘", fk(0xF72B)),    // Delete·Home·End
            116: ("⇞", fk(0xF72C)), 121: ("⇟", fk(0xF72D)),                             // PageUp·PageDown
            123: ("←", fk(0xF702)), 124: ("→", fk(0xF703)),                             // 화살표 왼쪽·오른쪽
            125: ("↓", fk(0xF701)), 126: ("↑", fk(0xF700)),                             // 화살표 아래·위
        ]
        for (i, code) in functionKeys.enumerated() { m[code] = ("F\(i + 1)", fk(0xF704 + i)) }   // NSF1FunctionKey 부터
        return m
    }()
    public static let functionKeys = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111]   // F1~F12

    /// 화면에 보이는 키 — macOS 메뉴와 같은 순서(⌃⌥⇧⌘). 글자는 ASCII 자판에서 Shift 없이 읽어 대문자로, 없으면 #키 번호
    public func label(layout: KeyboardLayout) -> String {
        var s = ""
        if mods.contains(.control) { s += "⌃" }
        if mods.contains(.option) { s += "⌥" }
        if mods.contains(.shift) { s += "⇧" }
        if mods.contains(.command) { s += "⌘" }
        return s + (KeyCombo.specialKeys[Int(code)]?.label ?? layout.characters(forKeyCode: code, shift: false)?.uppercased() ?? "#\(code)")
    }

    /// 메뉴 항목의 keyEquivalent — 글자는 ASCII 자판에서 읽는다(Shift 기호는 Shift 글자 그대로 ⇧⌘[ = "{", 영문자는 소문자 + Shift).
    /// ⇧⇥ 는 AppKit 이 backtab 글자로 보낸다. 글자가 없으면 빈 글자
    public func menuKey(layout: KeyboardLayout) -> String {
        if code == 48, mods.contains(.shift) { return "\u{19}" }
        if let k = KeyCombo.specialKeys[Int(code)] { return k.key }
        return (layout.characters(forKeyCode: code, shift: mods.contains(.shift)) ?? "").lowercased()
    }
}

// MARK: - 동작 표

/// 바꿀 수 있는 동작 하나 — id 는 보드 설정 shortcuts 의 열쇠라 바꾸면 사용자가 바꾼 키가 사라진다
public struct ShortcutAction: Sendable {
    public let id: String
    public let group: String
    public let title: String
    public let initial: KeyCombo?
    public init(id: String, group: String, title: String, initial: KeyCombo?) {
        self.id = id; self.group = group; self.title = title; self.initial = initial
    }
}

/// 보드 단축키 표와 사용자가 바꾼 키 — 설정 > 단축키(⌘,)에서 바꾼다. 메뉴의 keyEquivalent·전역 단축키(Carbon)·화면의 안내 글자를 모두 여기서 만든다.
/// 바꾼 것만 보드 설정 shortcuts 에 [id: [키 번호, 수정키 raw]] 로 남기고, 비운 것은 [] 다.
/// ⌘C·⌘Q 같은 macOS 표준 키와 ⌘W(엔진이 화면을 닫는다)는 표에 없고 고를 수도 없다(reserved)
public struct ShortcutTable: Equatable, Sendable {
    /// 표의 차례가 뜻을 가진다 — 같은 키를 쓰는 동작이 여럿이면 앞의 것이 받는다(메뉴도, 키 감시도)
    public static let actions: [ShortcutAction] = {
        var a: [ShortcutAction] = []
        func add(_ group: String, _ id: String, _ title: String, _ code: UInt16?, _ mods: KeyModifiers = []) {
            a.append(ShortcutAction(id: id, group: group, title: title, initial: code.map { KeyCombo($0, mods) }))
        }
        let co: KeyModifiers = [.control, .option]
        add("보드", "showBoard", "세션 보드 보기", 29, .command)
        add("보드", "pin", "항상 위에 고정", 35, .command)
        add("보드", "snap", "처음 크기로 붙이기 (어느 앱에서든)", 11, co)
        add("보드", "terminal", "터미널 펼치기·접기 (어느 앱에서든)", 17, co)
        add("터미널", "newSession", "새 세션…", 17, .command)
        add("터미널", "rename", "탭 이름 바꾸기", 15, [.command, .shift])
        for (n, code) in zip(1...9, [18, 19, 20, 21, 23, 22, 26, 28, 25] as [UInt16]) { add("탭으로 가기", "tab\(n)", "탭 \(n)", code, .command) }
        add("탭으로 가기", "prevTab", "이전 탭", 123, co)
        add("탭으로 가기", "nextTab", "다음 탭", 124, co)
        add("탭으로 가기", "prevTab2", "이전 탭 (보조)", 33, [.command, .shift])
        add("탭으로 가기", "nextTab2", "다음 탭 (보조)", 30, [.command, .shift])
        add("탭으로 가기", "prevTab3", "이전 탭 (보조 2)", 48, [.control, .shift])
        add("탭으로 가기", "nextTab3", "다음 탭 (보조 2)", 48, .control)
        add("탭으로 가기", "prevWorkspace", "이전 경로", 126, co)
        add("탭으로 가기", "nextWorkspace", "다음 경로", 125, co)
        add("화면 나누기", "splitRight", "오른쪽으로 나누기", 2, .command)
        add("화면 나누기", "splitDown", "아래로 나누기", 2, [.command, .shift])
        add("화면 나누기", "prevSurface", "이전 화면", 33, .command)
        add("화면 나누기", "nextSurface", "다음 화면", 30, .command)
        let arrows: [(String, String, UInt16)] = [("Up", "위로", 126), ("Down", "아래로", 125), ("Left", "왼쪽으로", 123), ("Right", "오른쪽으로", 124)]
        for (id, name, code) in arrows { add("화면 나누기", "focus\(id)", "\(name) 화면 고르기", code, [.command, .option]) }
        for (id, name, code) in arrows { add("화면 나누기", "resize\(id)", "경계를 \(name) 옮기기", code, [.command, .control]) }
        add("화면 나누기", "equalize", "크기 고르게", 24, [.command, .control])
        add("화면 나누기", "zoom", "화면 하나 크게 보기", 36, [.command, .shift])
        add("글자 크기", "terminalBigger", "터미널 글자 크게", 24, [.command, .shift])
        add("글자 크기", "terminalBigger2", "터미널 글자 크게 (보조)", 24, .command)
        add("글자 크기", "terminalSmaller", "터미널 글자 작게", 27, .command)
        add("글자 크기", "terminalReset", "터미널 글자 원래 크기", nil)
        add("글자 크기", "boardBigger", "보드 글자 크게", 24, [.command, .option, .shift])
        add("글자 크기", "boardBigger2", "보드 글자 크게 (보조)", 24, [.command, .option])
        add("글자 크기", "boardSmaller", "보드 글자 작게", 27, [.command, .option])
        add("글자 크기", "boardReset", "보드 글자 원래 크기", 29, [.command, .option])
        return a
    }()

    /// 고를 수 없는 키 — macOS 표준 편집·창 키(⌘Q·⌘H·⌥⌘H·⌘M·⌘W·⌘C·⌘V·⌘X·⌘A·⌘Z·⇧⌘Z)와 설정 화면을 여는 ⌘,
    public static let reserved = ([12, 4, 46, 13, 8, 9, 7, 0, 6, 43] as [UInt16]).map { KeyCombo($0, .command) }
        + [KeyCombo(6, [.command, .shift]), KeyCombo(4, [.command, .option])]
    /// 설정 화면 — 표에 없고 바꿀 수 없다(엔진 기본값 open_config 가 먼저 가져가므로 키 감시가 받는다)
    public static let settingsKey = KeyCombo(43, .command)

    public static func action(_ id: String) -> ShortcutAction? { actions.first { $0.id == id } }

    /// 사용자가 바꾼 키 — 보드 설정 shortcuts 의 값 그대로
    public private(set) var overrides: [String: [Int]]

    public init(overrides: [String: [Int]] = [:]) { self.overrides = overrides }

    /// 그 동작의 지금 키 — 바꾼 것이 없으면 기본값, [] 이면 비움(nil), [키 번호, 수정키 raw] 면 그 키(⌘⌃⌥⇧ 밖 비트는 버림).
    /// 이상한 저장값(길이 1·3 이상, 키 번호 0...65535 밖, 음수 수정키)도 기본값이다 — 검증(parseShortcutOverrides)을 거치지 않고
    /// 표에 바로 들어와도 멈추지 않는다. 표에 없는 id 도 바꾼 것이 있으면 그대로 읽는다
    public func combo(_ id: String) -> KeyCombo? {
        guard let v = overrides[id], let s = Self.decode(v) else { return Self.action(id)?.initial }
        return s
    }

    /// 저장값 하나 — [] 은 비움(.some(nil)), [키 번호 0...65535, 수정키 0 이상] 은 그 키, 그 밖은 nil(이상함 → 기본값).
    /// 키 번호·수정키는 UInt16(exactly:)·UInt(exactly:) 로만 담는다(못 담으면 멈추지 않고 이상함)
    static func decode(_ v: [Int]) -> KeyCombo?? {
        if v.isEmpty { return .some(nil) }
        guard v.count == 2, let code = UInt16(exactly: v[0]), let raw = UInt(exactly: v[1]) else { return nil }
        return .some(KeyCombo(code, KeyModifiers(rawValue: raw)))
    }

    /// 기본값과 같은 키로 돌아오면 남긴 것을 지운다. nil 은 비우기([]) — 다만 기본값이 없는 동작(과 표에 없는 id)은 nil 이 기본값과 같아 지운다
    public mutating func set(_ id: String, _ c: KeyCombo?) {
        if c == Self.action(id)?.initial { overrides[id] = nil } else { overrides[id] = c.map { [Int($0.code), Int($0.mods.rawValue)] } ?? [] }
    }

    public mutating func resetAll() { overrides = [:] }

    /// 같은 키를 쓰는 다른 동작(표 차례) — 메뉴는 앞의 것만 받는다. 키가 없으면 없다
    public func conflicts(_ id: String) -> [String] {
        guard let c = combo(id) else { return [] }
        return Self.actions.filter { $0.id != id && combo($0.id) == c }.map(\.id)
    }

    /// 이 키에 맞는 동작 — 표에서 첫 번째(키 감시가 그 메뉴 동작을 부른다)
    public func actionID(for c: KeyCombo) -> String? {
        Self.actions.first { combo($0.id) == c }?.id
    }
}

// MARK: - 저장값 검증

/// 보드 설정 shortcuts 원본(UserDefaults.dictionary(forKey:) 그대로) → 사용자가 바꾼 키. 항목마다 따로 본다:
/// [] (비움)과 [키 번호 0...65535, 수정키 0 이상] 만 남기고, 그 밖(길이 1·3 이상, 음수, 범위 밖, 문자열, 정수가 아닌 원소가 섞인 배열,
/// 배열 아님)은 그 항목만 버려 기본값으로 읽는다 — 앞에는 사전 전체를 [String: [Int]] 로 바꿔 하나라도 어긋나면 바꾼 키를 모두 잃었고
/// (다음 저장에서 덮어씀), 범위 밖 숫자는 메뉴를 만들다 멈췄다. 버린 항목은 앱이 다음에 저장할 때 사라진다.
/// 정수로 보는 것은 Foundation 이 [Int] 로 넘겨 주는 것 그대로다(앞과 같다): 딱 떨어지는 실수 11.0 은 11, 참/거짓은 1·0,
/// 11.5·Int 밖 수·글자는 넘기지 않는다. 표에 없는 id 의 저장값도 멀쩡하면 남긴다 — 그 동작이 있는 다른 버전으로 갔다 돌아올 때
/// 사용자가 바꾼 키를 지키려는 의도다(이슈 #9 3차에 확인, 고치지 않음). 표에 없는 id 는 메뉴·키 감시가 읽지 않아 남아도 듣지 않는다
public func parseShortcutOverrides(_ raw: [String: Any]?) -> [String: [Int]] {
    var out: [String: [Int]] = [:]
    for (id, value) in raw ?? [:] {
        guard let v = value as? [Int], ShortcutTable.decode(v) != nil else { continue }
        out[id] = v
    }
    return out
}

// MARK: - 키 받기 판정 (설정 > 단축키)

/// 받는 중에 누른 키를 어떻게 할지 — 앱(ShortcutSettings.record)이 그대로 실행한다
public enum KeyRecording: Equatable, Sendable {
    case cancel             // Esc — 받기를 그만둔다
    case clear              // ⌫·⌦ — 받기를 그만두고 그 동작의 키를 비운다
    case refuse(String)     // 받지 않고 안내한다(받기는 이어 간다)
    case accept(KeyCombo)   // 그 키로 바꾼다
}

/// Esc 취소, ⌫·⌦ 비우기(셋 다 수정키 없이). ⌘·⌃·⌥ 가 없는 키(⇧ 만도)는 받지 않는다 — F1~F12 는 수정키 없이도 된다.
/// 고를 수 없는 키(reserved)는 받지 않고 그 키 글자로 알린다. 그 밖은 받는다(보드 안 겹침·시스템 단축키 겹침은 받은 뒤 그 줄에서 알린다)
public func recordShortcut(_ c: KeyCombo, layout: KeyboardLayout) -> KeyRecording {
    if c.mods.isEmpty, c.code == 53 { return .cancel }
    if c.mods.isEmpty, c.code == 51 || c.code == 117 { return .clear }
    if c.mods.subtracting(.shift).isEmpty, !KeyCombo.functionKeys.contains(Int(c.code)) {
        return .refuse("⌘·⌃·⌥ 가운데 하나를 함께 눌러 주세요")
    }
    if ShortcutTable.reserved.contains(c) { return .refuse("\(c.label(layout: layout)): macOS 기본 키라 쓸 수 없습니다") }
    return .accept(c)
}

// MARK: - 시스템 단축키

/// macOS 시스템 단축키(시스템 설정 > 키보드 > 키보드 단축키 가운데 켜 둔 것)와 겹치는지 — system 은 [키 번호, Carbon 수정키] 목록(앱이 읽는다),
/// carbonMods 는 그 키의 Carbon 수정키(앱이 바꾼다 — Carbon 상수는 앱에 둔다). 다른 앱이 잡은 키는 알 길이 없다
public func systemShortcutTaken(_ c: KeyCombo, carbonMods: UInt32, system: [[Int]]) -> Bool {
    system.contains([Int(c.code), Int(carbonMods)])
}
