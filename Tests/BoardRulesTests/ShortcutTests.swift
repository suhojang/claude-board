// 단축키 판정(Shortcuts.swift) 시험 — 지금 ASCII 자판을 가짜 어댑터(미국 자판 일부를 적은 표)로 넣어 Carbon 없이 돌린다.
// 가짜는 이 파일에만 둔다(Sources/BoardRules 에 두지 않는다). 가짜는 물은 키 번호·Shift 를 적어 두어 특수키는 자판을 묻지 않는지까지 본다.
// 기대값은 앱의 // MARK: - 단축키 에 있던 지금 동작 그대로다(옮기기 전 코드로 만든 덤프와 옮긴 뒤 덤프가 바이트까지 같은 것도 확인했다).
// 다만 저장값 검증은 고친 동작이다(이슈 #9 2차): 이상한 항목 하나는 그 항목만 버려 기본값으로 읽고(앞에는 사전 전체를 잃거나 멈췄다),
// 길이가 2 가 아닌 저장값도 비움이 아니라 기본값이다.
// 저장값(보드 설정 shortcuts 의 [키 번호, 수정키 raw])은 숫자 리터럴로 박는다 — 리팩터가 인코딩을 조용히 바꾸면 사용자가 바꾼 키가 사라진다
import Foundation
import Testing
import BoardRules
#if canImport(AppKit)
import AppKit
#endif

// MARK: - 가짜 어댑터

// 물은 것 — 여러 시험이 함께 쓰는 자판(us)은 매개변수 시험이 동시에 부르므로 잠금 안에서 적고 읽는다
private final class LayoutLog: @unchecked Sendable {
    private let lock = NSLock()
    private var calls: [String] = []    // "키 번호" 또는 "키 번호⇧"
    var asked: [String] { lock.withLock { calls } }
    func append(_ s: String) { lock.withLock { calls.append(s) } }
}

// 미국(ABC) 자판 일부 — 키 번호 → (Shift 없이, Shift). 표에 없는 키는 글자가 없다(nil)
private struct FakeLayout: KeyboardLayout {
    let log = LayoutLog()
    var keys: [UInt16: (String, String)] = [
        0: ("a", "A"), 2: ("d", "D"), 4: ("h", "H"), 6: ("z", "Z"), 7: ("x", "X"), 8: ("c", "C"), 9: ("v", "V"),
        11: ("b", "B"), 12: ("q", "Q"), 13: ("w", "W"), 15: ("r", "R"), 17: ("t", "T"),
        18: ("1", "!"), 19: ("2", "@"), 20: ("3", "#"), 21: ("4", "$"), 22: ("6", "^"), 23: ("5", "%"),
        24: ("=", "+"), 25: ("9", "("), 26: ("7", "&"), 27: ("-", "_"), 28: ("8", "*"), 29: ("0", ")"),
        30: ("]", "}"), 33: ("[", "{"), 35: ("p", "P"), 43: (",", "<"), 45: ("n", "N"), 46: ("m", "M"),
    ]
    func characters(forKeyCode code: UInt16, shift: Bool) -> String? {
        log.append("\(code)\(shift ? "⇧" : "")")
        return keys[code].map { shift ? $0.1 : $0.0 }
    }
}

private let us = FakeLayout()

// 수정키 raw — 숫자로 적는다(NSEvent.ModifierFlags 와 같은 비트)
private let C: UInt = 1_048_576      // ⌘
private let K: UInt = 262_144        // ⌃
private let O: UInt = 524_288        // ⌥
private let S: UInt = 131_072        // ⇧
private let capsLock: UInt = 65_536, function: UInt = 8_388_608, numericPad: UInt = 2_097_152

private func key(_ code: Int, _ raw: UInt) -> KeyCombo { KeyCombo(UInt16(code), KeyModifiers(rawValue: raw)) }
/// 저장 꼴 — ShortcutTable.set 이 남기는 [키 번호, 수정키 raw]
private func stored(_ c: KeyCombo?) -> [Int]? { c.map { [Int($0.code), Int($0.mods.rawValue)] } }
private func hex(_ s: String) -> String { s.unicodeScalars.map { String(format: "%04X", $0.value) }.joined(separator: " ") }

// MARK: - 수정키 raw 값 (저장값 호환)

@Suite("단축키: 수정키 raw 값") struct ShortcutModifierTests {
    @Test("⌘⌃⌥⇧ raw 값은 NSEvent.ModifierFlags 와 같은 숫자 — 1비트라도 바뀌면 사용자가 바꾼 키가 사라진다")
    func raw값() {
        #expect(KeyModifiers.command.rawValue == 1_048_576)
        #expect(KeyModifiers.control.rawValue == 262_144)
        #expect(KeyModifiers.option.rawValue == 524_288)
        #expect(KeyModifiers.shift.rawValue == 131_072)
        #expect(KeyModifiers.keys.rawValue == 1_966_080)
        #if canImport(AppKit)
        #expect(KeyModifiers.command.rawValue == NSEvent.ModifierFlags.command.rawValue)
        #expect(KeyModifiers.control.rawValue == NSEvent.ModifierFlags.control.rawValue)
        #expect(KeyModifiers.option.rawValue == NSEvent.ModifierFlags.option.rawValue)
        #expect(KeyModifiers.shift.rawValue == NSEvent.ModifierFlags.shift.rawValue)
        #endif
    }

    @Test("키 하나는 ⌘⌃⌥⇧ 만 남긴다 — 캡스록·fn·숫자판 비트는 버리고 같은 키로 본다")
    func 수정키정리() {
        let c = key(0, C | capsLock | function | numericPad)
        #expect(c.mods.rawValue == 1_048_576)
        #expect(c == key(0, C))
        #expect(key(11, K | O | S | C | capsLock).mods.rawValue == 1_966_080)
        #expect(key(11, capsLock).mods.isEmpty)
    }
}

// MARK: - 동작 표

struct ActionRow: Row {
    let note: String
    let group: String
    let title: String
    let saved: [Int]?      // 기본값의 저장 꼴, nil 이면 기본 키 없음
    let label: String?
    let menuKey: String?
}

private func row(_ id: String, _ group: String, _ title: String, _ saved: [Int]?, _ label: String?, _ menuKey: String?) -> ActionRow {
    ActionRow(note: id, group: group, title: title, saved: saved, label: label, menuKey: menuKey)
}

// 표의 차례·id·묶음·제목·기본 키(저장 꼴 숫자)·글자 — 차례가 뜻을 가진다(같은 키면 앞의 것이 받는다)
private let actionRows: [ActionRow] = [
    row("showBoard", "보드", "세션 보드 보기", [29, 1048576], "⌘0", "0"),
    row("pin", "보드", "항상 위에 고정", [35, 1048576], "⌘P", "p"),
    row("snap", "보드", "처음 크기로 붙이기 (어느 앱에서든)", [11, 786432], "⌃⌥B", "b"),
    row("terminal", "보드", "터미널 펼치기·접기 (어느 앱에서든)", [17, 786432], "⌃⌥T", "t"),
    row("newSession", "터미널", "새 세션…", [17, 1048576], "⌘T", "t"),
    row("rename", "터미널", "탭 이름 바꾸기", [15, 1179648], "⇧⌘R", "r"),
    row("tab1", "탭으로 가기", "탭 1", [18, 1048576], "⌘1", "1"),
    row("tab2", "탭으로 가기", "탭 2", [19, 1048576], "⌘2", "2"),
    row("tab3", "탭으로 가기", "탭 3", [20, 1048576], "⌘3", "3"),
    row("tab4", "탭으로 가기", "탭 4", [21, 1048576], "⌘4", "4"),
    row("tab5", "탭으로 가기", "탭 5", [23, 1048576], "⌘5", "5"),
    row("tab6", "탭으로 가기", "탭 6", [22, 1048576], "⌘6", "6"),
    row("tab7", "탭으로 가기", "탭 7", [26, 1048576], "⌘7", "7"),
    row("tab8", "탭으로 가기", "탭 8", [28, 1048576], "⌘8", "8"),
    row("tab9", "탭으로 가기", "탭 9", [25, 1048576], "⌘9", "9"),
    row("prevTab", "탭으로 가기", "이전 탭", [123, 786432], "⌃⌥←", "\u{F702}"),
    row("nextTab", "탭으로 가기", "다음 탭", [124, 786432], "⌃⌥→", "\u{F703}"),
    row("prevTab2", "탭으로 가기", "이전 탭 (보조)", [33, 1179648], "⇧⌘[", "{"),
    row("nextTab2", "탭으로 가기", "다음 탭 (보조)", [30, 1179648], "⇧⌘]", "}"),
    row("prevTab3", "탭으로 가기", "이전 탭 (보조 2)", [48, 393216], "⌃⇧⇥", "\u{19}"),
    row("nextTab3", "탭으로 가기", "다음 탭 (보조 2)", [48, 262144], "⌃⇥", "\t"),
    row("prevWorkspace", "탭으로 가기", "이전 경로", [126, 786432], "⌃⌥↑", "\u{F700}"),
    row("nextWorkspace", "탭으로 가기", "다음 경로", [125, 786432], "⌃⌥↓", "\u{F701}"),
    row("splitRight", "화면 나누기", "오른쪽으로 나누기", [2, 1048576], "⌘D", "d"),
    row("splitDown", "화면 나누기", "아래로 나누기", [2, 1179648], "⇧⌘D", "d"),
    row("prevSurface", "화면 나누기", "이전 화면", [33, 1048576], "⌘[", "["),
    row("nextSurface", "화면 나누기", "다음 화면", [30, 1048576], "⌘]", "]"),
    row("focusUp", "화면 나누기", "위로 화면 고르기", [126, 1572864], "⌥⌘↑", "\u{F700}"),
    row("focusDown", "화면 나누기", "아래로 화면 고르기", [125, 1572864], "⌥⌘↓", "\u{F701}"),
    row("focusLeft", "화면 나누기", "왼쪽으로 화면 고르기", [123, 1572864], "⌥⌘←", "\u{F702}"),
    row("focusRight", "화면 나누기", "오른쪽으로 화면 고르기", [124, 1572864], "⌥⌘→", "\u{F703}"),
    row("resizeUp", "화면 나누기", "경계를 위로 옮기기", [126, 1310720], "⌃⌘↑", "\u{F700}"),
    row("resizeDown", "화면 나누기", "경계를 아래로 옮기기", [125, 1310720], "⌃⌘↓", "\u{F701}"),
    row("resizeLeft", "화면 나누기", "경계를 왼쪽으로 옮기기", [123, 1310720], "⌃⌘←", "\u{F702}"),
    row("resizeRight", "화면 나누기", "경계를 오른쪽으로 옮기기", [124, 1310720], "⌃⌘→", "\u{F703}"),
    row("equalize", "화면 나누기", "크기 고르게", [24, 1310720], "⌃⌘=", "="),
    row("zoom", "화면 나누기", "화면 하나 크게 보기", [36, 1179648], "⇧⌘↩", "\r"),
    row("terminalBigger", "글자 크기", "터미널 글자 크게", [24, 1179648], "⇧⌘=", "+"),
    row("terminalBigger2", "글자 크기", "터미널 글자 크게 (보조)", [24, 1048576], "⌘=", "="),
    row("terminalSmaller", "글자 크기", "터미널 글자 작게", [27, 1048576], "⌘-", "-"),
    row("terminalReset", "글자 크기", "터미널 글자 원래 크기", nil, nil, nil),
    row("boardBigger", "글자 크기", "보드 글자 크게", [24, 1703936], "⌥⇧⌘=", "+"),
    row("boardBigger2", "글자 크기", "보드 글자 크게 (보조)", [24, 1572864], "⌥⌘=", "="),
    row("boardSmaller", "글자 크기", "보드 글자 작게", [27, 1572864], "⌥⌘-", "-"),
    row("boardReset", "글자 크기", "보드 글자 원래 크기", [29, 1572864], "⌥⌘0", "0"),
]

@Suite("단축키: 동작 표") struct ShortcutActionTableTests {
    @Test("id 차례가 그대로다(빠지거나 늘거나 바뀐 차례 없음)")
    func 차례() {
        #expect(ShortcutTable.actions.map(\.id) == actionRows.map(\.note))
    }

    @Test("동작마다 묶음·제목·기본 키(저장 꼴)·글자·메뉴 키", arguments: actionRows)
    func 동작(_ r: ActionRow) {
        let a = ShortcutTable.action(r.note)
        #expect(a?.id == r.note)
        #expect(a?.group == r.group)
        #expect(a?.title == r.title)
        #expect(stored(a?.initial) == r.saved)
        #expect(a?.initial?.label(layout: us) == r.label)
        #expect(a?.initial.map { hex($0.menuKey(layout: us)) } == r.menuKey.map(hex))
    }

    @Test("기본 표에는 같은 키를 쓰는 동작이 없다")
    func 기본겹침없음() {
        let t = ShortcutTable()
        for a in ShortcutTable.actions { #expect(t.conflicts(a.id) == [], "\(a.id)") }
    }

    @Test("고를 수 없는 키 — ⌘Q·⌘H·⌘M·⌘W·⌘C·⌘V·⌘X·⌘A·⌘Z·⌘, 와 ⇧⌘Z·⌥⌘H, 그 차례대로")
    func 예약() {
        #expect(ShortcutTable.reserved.map { stored($0)! } == [
            [12, 1048576], [4, 1048576], [46, 1048576], [13, 1048576], [8, 1048576], [9, 1048576], [7, 1048576], [0, 1048576],
            [6, 1048576], [43, 1048576], [6, 1179648], [4, 1572864],
        ])
        #expect(ShortcutTable.reserved.map { $0.label(layout: us) } == ["⌘Q", "⌘H", "⌘M", "⌘W", "⌘C", "⌘V", "⌘X", "⌘A", "⌘Z", "⌘,", "⇧⌘Z", "⌥⌘H"])
        #expect(stored(ShortcutTable.settingsKey) == [43, 1048576])
    }

    @Test("없는 id 는 nil")
    func 없는id() {
        #expect(ShortcutTable.action("unknown") == nil)
    }
}

// MARK: - 글자 (label·menuKey)

struct LabelRow: Row {
    let note: String
    let code: Int
    let raw: UInt
    let label: String
    let menuKey: String
    let asked: [String]      // 자판에 물은 것(차례대로) — label 다음 menuKey
}

private let labelRows: [LabelRow] = [
    .init(note: "수정키 차례는 ⌃⌥⇧⌘", code: 0, raw: C | K | O | S, label: "⌃⌥⇧⌘A", menuKey: "a", asked: ["0", "0⇧"]),
    .init(note: "영문자는 화면에 대문자, 메뉴에는 소문자 + Shift", code: 15, raw: C | S, label: "⇧⌘R", menuKey: "r", asked: ["15", "15⇧"]),
    .init(note: "⇧ 없는 영문자", code: 15, raw: C, label: "⌘R", menuKey: "r", asked: ["15", "15"]),
    .init(note: "Shift 기호 — 화면은 Shift 없는 글자, 메뉴는 Shift 글자", code: 33, raw: C | S, label: "⇧⌘[", menuKey: "{", asked: ["33", "33⇧"]),
    .init(note: "⇧⌘= 는 메뉴에 +", code: 24, raw: C | S, label: "⇧⌘=", menuKey: "+", asked: ["24", "24⇧"]),
    .init(note: "⇧⇥ 는 backtab 글자 — 자판을 묻지 않는다", code: 48, raw: S, label: "⇧⇥", menuKey: "\u{19}", asked: []),
    .init(note: "⌃⇧⇥ 도 backtab", code: 48, raw: K | S, label: "⌃⇧⇥", menuKey: "\u{19}", asked: []),
    .init(note: "⌃⇥ 는 탭 글자", code: 48, raw: K, label: "⌃⇥", menuKey: "\t", asked: []),
    .init(note: "↩", code: 36, raw: C | S, label: "⇧⌘↩", menuKey: "\r", asked: []),
    .init(note: "숫자판 ⌤", code: 76, raw: C, label: "⌘⌤", menuKey: "\u{3}", asked: []),
    .init(note: "Space", code: 49, raw: C, label: "⌘Space", menuKey: " ", asked: []),
    .init(note: "⌫", code: 51, raw: C, label: "⌘⌫", menuKey: "\u{8}", asked: []),
    .init(note: "⌦ 는 AppKit Delete 기능키 글자", code: 117, raw: C, label: "⌘⌦", menuKey: "\u{F728}", asked: []),
    .init(note: "Home", code: 115, raw: 0, label: "↖", menuKey: "\u{F729}", asked: []),
    .init(note: "End", code: 119, raw: 0, label: "↘", menuKey: "\u{F72B}", asked: []),
    .init(note: "PageUp", code: 116, raw: 0, label: "⇞", menuKey: "\u{F72C}", asked: []),
    .init(note: "PageDown", code: 121, raw: 0, label: "⇟", menuKey: "\u{F72D}", asked: []),
    .init(note: "화살표 ←→↓↑", code: 123, raw: K | O, label: "⌃⌥←", menuKey: "\u{F702}", asked: []),
    .init(note: "→", code: 124, raw: 0, label: "→", menuKey: "\u{F703}", asked: []),
    .init(note: "↓", code: 125, raw: 0, label: "↓", menuKey: "\u{F701}", asked: []),
    .init(note: "↑", code: 126, raw: 0, label: "↑", menuKey: "\u{F700}", asked: []),
    .init(note: "F1", code: 122, raw: 0, label: "F1", menuKey: "\u{F704}", asked: []),
    .init(note: "F5 에 ⇧", code: 96, raw: S, label: "⇧F5", menuKey: "\u{F708}", asked: []),
    .init(note: "F12", code: 111, raw: C, label: "⌘F12", menuKey: "\u{F70F}", asked: []),
    .init(note: "자판에 글자가 없으면 화면은 #키 번호, 메뉴는 빈 글자", code: 50, raw: C, label: "⌘#50", menuKey: "", asked: ["50", "50"]),
    .init(note: "수정키 없는 글자 키", code: 27, raw: 0, label: "-", menuKey: "-", asked: ["27", "27"]),
]

@Suite("단축키: 글자") struct ShortcutLabelTests {
    @Test("화면 글자·메뉴 keyEquivalent·자판에 묻는 것", arguments: labelRows)
    func 글자(_ r: LabelRow) {
        let layout = FakeLayout()
        let c = key(r.code, r.raw)
        #expect(c.label(layout: layout) == r.label)
        #expect(hex(c.menuKey(layout: layout)) == hex(r.menuKey))
        #expect(layout.log.asked == r.asked)
    }

    @Test("F1~F12 의 키 번호와 글자")
    func 기능키() {
        #expect(KeyCombo.functionKeys == [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111])
        for (i, code) in KeyCombo.functionKeys.enumerated() {
            #expect(KeyCombo.specialKeys[code]?.label == "F\(i + 1)")
            #expect(KeyCombo.specialKeys[code]?.key.unicodeScalars.map(\.value) == [UInt32(0xF704 + i)])
        }
        #expect(KeyCombo.specialKeys.count == 26)
    }

    #if canImport(AppKit)
    @Test("특수키 글자는 AppKit 의 NS…FunctionKey 값과 같다")
    func AppKit상수() {
        func k(_ code: Int) -> UInt32? { KeyCombo.specialKeys[code]?.key.unicodeScalars.first?.value }
        #expect(k(117) == UInt32(NSDeleteFunctionKey))
        #expect(k(115) == UInt32(NSHomeFunctionKey))
        #expect(k(119) == UInt32(NSEndFunctionKey))
        #expect(k(116) == UInt32(NSPageUpFunctionKey))
        #expect(k(121) == UInt32(NSPageDownFunctionKey))
        #expect(k(123) == UInt32(NSLeftArrowFunctionKey))
        #expect(k(124) == UInt32(NSRightArrowFunctionKey))
        #expect(k(125) == UInt32(NSDownArrowFunctionKey))
        #expect(k(126) == UInt32(NSUpArrowFunctionKey))
        for (i, code) in KeyCombo.functionKeys.enumerated() { #expect(k(code) == UInt32(NSF1FunctionKey + i)) }
    }
    #endif
}

// MARK: - 사용자가 바꾼 키 해석

struct ComboRow: Row {
    let note: String
    let overrides: [String: [Int]]
    let id: String
    let expected: [Int]?     // 해석한 키의 저장 꼴, nil 이면 키 없음
}

private let comboRows: [ComboRow] = [
    .init(note: "바꾼 것이 없으면 기본값", overrides: [:], id: "snap", expected: [11, 786432]),
    .init(note: "기본값이 없는 동작", overrides: [:], id: "terminalReset", expected: nil),
    .init(note: "[] 는 비움", overrides: ["snap": []], id: "snap", expected: nil),
    .init(note: "[키 번호, 수정키] 는 그 키", overrides: ["pin": [11, 786432]], id: "pin", expected: [11, 786432]),
    .init(note: "저장값의 fn 비트는 버린다", overrides: ["newSession": [17, 9437184]], id: "newSession", expected: [17, 1048576]),
    .init(note: "저장값의 캡스록 비트는 버린다", overrides: ["zoom": [36, 1245184]], id: "zoom", expected: [36, 1179648]),
    .init(note: "수정키 없는 저장값", overrides: ["boardSmaller": [27, 0]], id: "boardSmaller", expected: [27, 0]),
    .init(note: "다른 동작의 저장값은 보지 않는다", overrides: ["pin": [11, 786432]], id: "snap", expected: [11, 786432]),
    .init(note: "길이가 1 이면 기본값(비움이 아니다)", overrides: ["tab1": [1]], id: "tab1", expected: [18, 1048576]),
    .init(note: "길이가 3 이면 기본값(비움이 아니다)", overrides: ["tab2": [1, 2, 3]], id: "tab2", expected: [19, 1048576]),
    .init(note: "기본값이 없는 동작에 길이 1 → 키 없음", overrides: ["terminalReset": [1]], id: "terminalReset", expected: nil),
    .init(note: "키 번호 경계 0·65535 는 그 키", overrides: ["pin": [65535, 0], "snap": [0, 786432]], id: "pin", expected: [65535, 0]),
    .init(note: "UInt 에 담기는 큰 수정키는 ⌘⌃⌥⇧ 만 남긴다", overrides: ["pin": [35, Int.max]], id: "pin", expected: [35, 1966080]),
    .init(note: "표에 없는 id 도 저장값이 있으면 읽는다", overrides: ["unknown": [0, 1048576]], id: "unknown", expected: [0, 1048576]),
    .init(note: "표에 없는 id 에 저장값이 없으면 키 없음", overrides: [:], id: "unknown", expected: nil),
]

@Suite("단축키: 사용자가 바꾼 키") struct ShortcutOverrideTests {
    @Test("저장값 해석 — 없음은 기본값, [] 는 비움, [키 번호, 수정키] 는 그 키", arguments: comboRows)
    func 해석(_ r: ComboRow) {
        #expect(stored(ShortcutTable(overrides: r.overrides).combo(r.id)) == r.expected)
    }

    @Test("고치기 차례 — 기본값과 같으면 지우고, nil 은 [] 로 비운다(기본값이 없으면 nil 이 기본값과 같아 지운다)")
    func 고치기() {
        var t = ShortcutTable()
        func step(_ id: String, _ c: KeyCombo?, _ expected: [String: [Int]], _ note: String) {
            t.set(id, c)
            #expect(t.overrides == expected, "\(note)")
        }
        step("snap", key(11, K | O), [:], "기본값과 같은 키 → 남기지 않음")
        step("snap", nil, ["snap": []], "비우기 → []")
        step("snap", key(8, K | O), ["snap": [8, 786432]], "다른 키 → [키 번호, 수정키]")
        step("snap", key(11, K | O), [:], "기본값으로 돌아오면 지움")
        step("terminalReset", nil, [:], "기본값이 없는 동작을 비우면 지움(nil == nil)")
        step("terminalReset", key(25, C), ["terminalReset": [25, 1048576]], "기본값이 없는 동작에 키")
        step("pin", key(35, C | S | capsLock), ["pin": [35, 1179648], "terminalReset": [25, 1048576]], "캡스록은 버리고 남긴다")
        step("unknown", nil, ["pin": [35, 1179648], "terminalReset": [25, 1048576]], "표에 없는 id 를 비우면 그대로")
        step("unknown", key(0, C), ["pin": [35, 1179648], "terminalReset": [25, 1048576], "unknown": [0, 1048576]], "표에 없는 id 에도 남긴다")
        step("tab1", key(18, C | capsLock), ["pin": [35, 1179648], "terminalReset": [25, 1048576], "unknown": [0, 1048576]],
             "캡스록이 섞여도 기본값과 같으면 남기지 않음")
        step("pin", nil, ["pin": [], "terminalReset": [25, 1048576], "unknown": [0, 1048576]], "남긴 키를 비우기로 바꿈")
        t.resetAll()
        #expect(t.overrides == [:])
        #expect(stored(t.combo("pin")) == [35, 1048576])
    }

    @Test("고친 값을 다시 해석하면 그 키 — 모든 동작에 ⌃⌥⇧⌘F12 를 넣고 읽는다")
    func 왕복() {
        var t = ShortcutTable()
        let c = key(111, K | O | S | C)
        for a in ShortcutTable.actions { t.set(a.id, c) }
        for a in ShortcutTable.actions {
            #expect(t.overrides[a.id] == [111, 1966080])
            #expect(t.combo(a.id) == c)
        }
    }
}

// 범위 밖 저장값이 표에 바로 들어온 경우(검증을 거치지 않은 길) — 앞에는 키 번호 UInt16(…)·수정키 UInt(…) 에서 멈췄다.
// 멈추면 시험 프로세스가 죽으므로 이 줄들은 따로 둔다
private let outOfRangeRows: [ComboRow] = [
    .init(note: "음수 수정키 → 기본값", overrides: ["pin": [35, -1]], id: "pin", expected: [35, 1048576]),
    .init(note: "Int.min 수정키 → 기본값", overrides: ["pin": [35, Int.min]], id: "pin", expected: [35, 1048576]),
    .init(note: "키 번호 70000 → 기본값", overrides: ["pin": [70000, 1048576]], id: "pin", expected: [35, 1048576]),
    .init(note: "키 번호 65536 → 기본값", overrides: ["pin": [65536, 1048576]], id: "pin", expected: [35, 1048576]),
    .init(note: "음수 키 번호 → 기본값", overrides: ["pin": [-1, 1048576]], id: "pin", expected: [35, 1048576]),
    .init(note: "기본값이 없는 동작에 음수 수정키 → 키 없음", overrides: ["terminalReset": [25, -1]], id: "terminalReset", expected: nil),
    .init(note: "표에 없는 id 에 범위 밖 키 번호 → 키 없음", overrides: ["unknown": [70000, 0]], id: "unknown", expected: nil),
]

@Suite("단축키: 범위 밖 저장값") struct ShortcutOutOfRangeTests {
    @Test("범위 밖 저장값이 표에 바로 들어와도 멈추지 않고 기본값으로 읽는다", arguments: outOfRangeRows)
    func 해석(_ r: ComboRow) {
        #expect(stored(ShortcutTable(overrides: r.overrides).combo(r.id)) == r.expected)
    }

    @Test("범위 밖 저장값이 있어도 겹침·찾기가 멈추지 않는다 — 그 동작은 기본 키로 본다")
    func 겹침찾기() {
        let t = ShortcutTable(overrides: ["pin": [35, -1], "snap": [70000, 786432]])
        #expect(t.conflicts("pin") == [])
        #expect(t.conflicts("showBoard") == [])
        #expect(t.actionID(for: key(35, C)) == "pin")
        #expect(t.actionID(for: key(11, K | O)) == "snap")
    }
}

// MARK: - 저장값 검증 (보드 설정 shortcuts 원본)

/// 보드 설정 shortcuts 원본 — UserDefaults.dictionary 가 돌려주는 것과 같은 꼴(NSNumber 정수·실수·참거짓, NSString …)을 plist 에서 만든다.
/// entries 는 <dict> 안쪽, nil 이면 저장값 없음(dictionary(forKey:) 가 nil)
private func defaultsValue(_ entries: String?) -> [String: Any]? {
    guard let entries else { return nil }
    let xml = #"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>"# + entries + "</dict></plist>"
    return try! PropertyListSerialization.propertyList(from: Data(xml.utf8), format: nil) as? [String: Any]
}
private func entry(_ id: String, _ value: String) -> String { "<key>\(id)</key>\(value)" }
private func ints(_ v: Int...) -> String { "<array>" + v.map { "<integer>\($0)</integer>" }.joined() + "</array>" }

struct StoredRow: Row {
    let note: String
    let plist: String?               // shortcuts 의 plist <dict> 안쪽, nil 이면 저장값 없음
    let expected: [String: [Int]]
}

// Foundation 의 숫자 읽기(2026-10, macOS 26 에서 확인): 정수는 Int, 딱 떨어지는 실수(11.0)도 Int 로 넘기고, 11.5·Int 밖 수는 넘기지 않는다.
// 참/거짓도 1·0 으로 넘긴다. 정수로 보는 것은 이 읽기 그대로다(앞에 사전 전체를 [String: [Int]] 로 읽을 때와 같다)
private let storedRows: [StoredRow] = [
    .init(note: "저장값 없음 → 빈 사전", plist: nil, expected: [:]),
    .init(note: "빈 사전", plist: "", expected: [:]),
    .init(note: "[키 번호, 수정키] 는 그대로", plist: entry("pin", ints(11, 786432)), expected: ["pin": [11, 786432]]),
    .init(note: "[] 는 비움 그대로", plist: entry("snap", ints()), expected: ["snap": []]),
    .init(note: "수정키 0·fn 비트가 든 수정키도 그대로(⌘⌃⌥⇧ 밖 비트는 읽을 때 버린다)", plist: entry("boardSmaller", ints(27, 0)) + entry("newSession", ints(17, 9437184)),
          expected: ["boardSmaller": [27, 0], "newSession": [17, 9437184]]),
    .init(note: "키 번호 경계 0·65535 는 그대로", plist: entry("pin", ints(0, 1048576)) + entry("snap", ints(65535, 786432)),
          expected: ["pin": [0, 1048576], "snap": [65535, 786432]]),
    .init(note: "음수 수정키 → 그 항목만 버림", plist: entry("pin", ints(35, -1)), expected: [:]),
    .init(note: "키 번호 70000(UInt16 밖) → 버림", plist: entry("pin", ints(70000, 1048576)), expected: [:]),
    .init(note: "키 번호 65536 → 버림", plist: entry("pin", ints(65536, 1048576)), expected: [:]),
    .init(note: "음수 키 번호 → 버림", plist: entry("pin", ints(-1, 1048576)), expected: [:]),
    .init(note: "길이 1 → 버림", plist: entry("tab1", ints(1)), expected: [:]),
    .init(note: "길이 3 → 버림", plist: entry("tab2", ints(1, 2, 3)), expected: [:]),
    .init(note: "문자열 값 → 버림", plist: entry("pin", "<string>⌘P</string>"), expected: [:]),
    .init(note: "문자열이 섞인 배열 → 버림", plist: entry("pin", "<array><string>35</string><integer>1048576</integer></array>"), expected: [:]),
    .init(note: "숫자 글자만 든 배열(defaults write 로 넣은 꼴) → 버림", plist: entry("pin", "<array><string>35</string><string>1048576</string></array>"), expected: [:]),
    .init(note: "정수 값(배열 아님) → 버림", plist: entry("pin", "<integer>35</integer>"), expected: [:]),
    .init(note: "사전 값 → 버림", plist: entry("pin", "<dict/>"), expected: [:]),
    .init(note: "실수 11.5 → Foundation 이 Int 로 넘기지 않아 버림", plist: entry("snap", "<array><real>11.5</real><integer>786432</integer></array>"), expected: [:]),
    .init(note: "딱 떨어지는 실수 11.0 → Foundation 이 11 로 넘겨 그 키(앞과 같음)", plist: entry("snap", "<array><real>11.0</real><real>786432.0</real></array>"),
          expected: ["snap": [11, 786432]]),
    .init(note: "Int 에 담기지 않는 수정키(UInt64 최대) → 버림", plist: entry("pin", "<array><integer>35</integer><integer>18446744073709551615</integer></array>"),
          expected: [:]),
    .init(note: "참/거짓은 Foundation 이 1·0 으로 넘긴다 → 그 키(앞과 같음, 앱은 쓰지 않는 꼴)", plist: entry("pin", "<array><true/><integer>1048576</integer></array>"),
          expected: ["pin": [1, 1048576]]),
    .init(note: "멀쩡한 둘 + 음수 수정키 하나 → 둘은 그대로", plist: entry("pin", ints(11, 786432)) + entry("snap", ints()) + entry("terminal", ints(17, -786432)),
          expected: ["pin": [11, 786432], "snap": []]),
    .init(note: "멀쩡한 둘 + 문자열 하나 → 둘은 그대로", plist: entry("pin", ints(11, 786432)) + entry("snap", ints()) + entry("terminal", "<string>x</string>"),
          expected: ["pin": [11, 786432], "snap": []]),
    .init(note: "멀쩡한 둘 + 실수 11.5 하나 → 둘은 그대로", plist: entry("pin", ints(11, 786432)) + entry("snap", ints()) + entry("terminal", "<array><real>11.5</real><integer>786432</integer></array>"),
          expected: ["pin": [11, 786432], "snap": []]),
    .init(note: "멀쩡한 둘 + 길이 3 하나 → 둘은 그대로", plist: entry("pin", ints(11, 786432)) + entry("snap", ints()) + entry("terminal", ints(17, 786432, 0)),
          expected: ["pin": [11, 786432], "snap": []]),
    .init(note: "표에 없는 id 의 멀쩡한 저장값은 남긴다(지금처럼)", plist: entry("unknown", ints(0, 1048576)), expected: ["unknown": [0, 1048576]]),
]

@Suite("단축키: 저장값 검증") struct ShortcutStoredTests {
    @Test("항목마다 따로 본다 — [] 와 [키 번호 0...65535, 수정키 0 이상] 만 남기고 그 밖은 그 항목만 버린다", arguments: storedRows)
    func 검증(_ r: StoredRow) {
        #expect(parseShortcutOverrides(defaultsValue(r.plist)) == r.expected)
    }

    @Test("깨진 항목 하나가 있어도 바꾼 키를 모두 잃지 않는다 — 깨진 동작만 기본 키")
    func 깨진항목() {
        let t = ShortcutTable(overrides: parseShortcutOverrides(defaultsValue(
            entry("pin", ints(11, 786432)) + entry("snap", ints()) + entry("terminal", "<string>x</string>"))))
        #expect(stored(t.combo("pin")) == [11, 786432])
        #expect(t.combo("snap") == nil)
        #expect(stored(t.combo("terminal")) == [17, 786432])
        #expect(stored(t.combo("showBoard")) == [29, 1048576])
    }

    @Test("버린 항목은 다음 저장 때 사라진다 — 앱은 고친 값(overrides)을 통째로 다시 쓴다")
    func 다음저장() {
        var t = ShortcutTable(overrides: parseShortcutOverrides(defaultsValue(
            entry("pin", ints(11, 786432)) + entry("terminal", ints(17, -1)) + entry("tab1", "<string>x</string>"))))
        t.set("zoom", key(36, C))
        #expect(t.overrides == ["pin": [11, 786432], "zoom": [36, 1048576]])
    }

    @Test("멀쩡한 저장값의 해석은 그대로 — 기본 표 45개를 저장 꼴로 넣은 사전과 몇 개를 바꾼 사전을 plist 로 오가도 같다")
    func 멀쩡한저장값() throws {
        var all: [String: [Int]] = [:]
        for a in ShortcutTable.actions { all[a.id] = stored(a.initial) ?? [] }
        #expect(all.count == 45)
        var changed: [String: [Int]] = [:]
        changed["pin"] = [11, 786432]
        changed["snap"] = []
        changed["tab1"] = [111, 1966080]
        changed["newSession"] = [17, 9437184]
        changed["unknown"] = [0, 1048576]
        for overrides in [all, changed] {
            let data = try PropertyListSerialization.data(fromPropertyList: overrides, format: .binary, options: 0)
            let raw = try #require(try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any])
            let parsed = parseShortcutOverrides(raw)
            #expect(parsed == overrides)
            let before = ShortcutTable(overrides: overrides), after = ShortcutTable(overrides: parsed)
            for a in ShortcutTable.actions + [ShortcutAction(id: "unknown", group: "", title: "", initial: nil)] {
                #expect(after.combo(a.id) == before.combo(a.id), "\(a.id)")
                #expect(after.conflicts(a.id) == before.conflicts(a.id), "\(a.id)")
            }
        }
        for a in ShortcutTable.actions { #expect(ShortcutTable(overrides: all).combo(a.id) == a.initial, "\(a.id)") }
    }
}

// MARK: - 겹침과 찾기

@Suite("단축키: 겹침·찾기") struct ShortcutConflictTests {
    @Test("같은 키를 쓰는 다른 동작 — 자기는 빼고 표 차례로")
    func 겹침() {
        let t = ShortcutTable(overrides: ["pin": [29, 1048576], "boardReset": [29, 1048576], "prevTab": [124, 786432]])
        #expect(t.conflicts("showBoard") == ["pin", "boardReset"])
        #expect(t.conflicts("pin") == ["showBoard", "boardReset"])
        #expect(t.conflicts("boardReset") == ["showBoard", "pin"])
        #expect(t.conflicts("prevTab") == ["nextTab"])
        #expect(t.conflicts("nextTab") == ["prevTab"])
        #expect(t.conflicts("snap") == [])
    }

    @Test("키가 없는 동작은 겹침이 없다 — 비운 것끼리도")
    func 비움겹침() {
        let t = ShortcutTable(overrides: ["snap": [], "terminal": []])
        #expect(t.conflicts("snap") == [])
        #expect(t.conflicts("terminalReset") == [])
    }

    @Test("표에 없는 id 의 겹침 — 표의 동작과 견준다")
    func 없는id겹침() {
        #expect(ShortcutTable(overrides: ["unknown": [29, 1048576]]).conflicts("unknown") == ["showBoard"])
    }

    @Test("이 키에 맞는 동작 — 표에서 첫 번째")
    func 찾기() {
        let t = ShortcutTable()
        #expect(t.actionID(for: key(29, C)) == "showBoard")
        #expect(t.actionID(for: key(11, K | O)) == "snap")
        #expect(t.actionID(for: key(11, K | O | capsLock)) == "snap")
        #expect(t.actionID(for: key(12, C)) == nil)
        #expect(t.actionID(for: key(29, 0)) == nil)
        let moved = ShortcutTable(overrides: ["pin": [29, 1048576]])
        #expect(moved.actionID(for: key(29, C)) == "showBoard")
        #expect(moved.actionID(for: key(35, C)) == nil)
        let cleared = ShortcutTable(overrides: ["showBoard": [], "pin": [29, 1048576]])
        #expect(cleared.actionID(for: key(29, C)) == "pin")
        #expect(ShortcutTable(overrides: ["unknown": [0, 1048576]]).actionID(for: key(0, C)) == nil)
    }

    @Test("기본 표의 모든 키는 자기 동작으로 찾아진다")
    func 기본찾기() {
        let t = ShortcutTable()
        for a in ShortcutTable.actions {
            guard let c = a.initial else { continue }
            #expect(t.actionID(for: c) == a.id)
        }
    }
}

// MARK: - 키 받기 판정

struct RecordRow: Row {
    let note: String
    let code: Int
    let raw: UInt
    let expected: KeyRecording
}

private let needMod = KeyRecording.refuse("⌘·⌃·⌥ 가운데 하나를 함께 눌러 주세요")
private func reservedText(_ label: String) -> KeyRecording { .refuse("\(label): macOS 기본 키라 쓸 수 없습니다") }

private let recordRows: [RecordRow] = [
    .init(note: "Esc 는 취소", code: 53, raw: 0, expected: .cancel),
    .init(note: "Esc 에 캡스록만 섞여도 취소", code: 53, raw: capsLock, expected: .cancel),
    .init(note: "⇧Esc 는 취소가 아니고 수정키 없음으로 거절", code: 53, raw: S, expected: needMod),
    .init(note: "⌘Esc 는 받는다", code: 53, raw: C, expected: .accept(key(53, C))),
    .init(note: "⌫ 는 비우기", code: 51, raw: 0, expected: .clear),
    .init(note: "⌦ 는 비우기", code: 117, raw: 0, expected: .clear),
    .init(note: "⌘⌫ 는 받는다", code: 51, raw: C, expected: .accept(key(51, C))),
    .init(note: "⇧⌫ 는 수정키 없음으로 거절", code: 51, raw: S, expected: needMod),
    .init(note: "수정키 없는 글자 키는 거절", code: 0, raw: 0, expected: needMod),
    .init(note: "⇧ 만은 거절", code: 0, raw: S, expected: needMod),
    .init(note: "수정키 없는 Return 은 거절", code: 36, raw: 0, expected: needMod),
    .init(note: "F5 단독은 받는다", code: 96, raw: 0, expected: .accept(key(96, 0))),
    .init(note: "⇧F5 도 받는다", code: 96, raw: S, expected: .accept(key(96, S))),
    .init(note: "F12 단독은 받는다", code: 111, raw: 0, expected: .accept(key(111, 0))),
    .init(note: "⌘Q 는 macOS 기본 키", code: 12, raw: C, expected: reservedText("⌘Q")),
    .init(note: "⌘H", code: 4, raw: C, expected: reservedText("⌘H")),
    .init(note: "⌘W", code: 13, raw: C, expected: reservedText("⌘W")),
    .init(note: "⇧⌘Z", code: 6, raw: C | S, expected: reservedText("⇧⌘Z")),
    .init(note: "⌥⌘H", code: 4, raw: C | O, expected: reservedText("⌥⌘H")),
    .init(note: "⌘, 는 설정 화면 키", code: 43, raw: C, expected: reservedText("⌘,")),
    .init(note: "⌘Q 에 캡스록이 섞여도 거절", code: 12, raw: C | capsLock, expected: reservedText("⌘Q")),
    .init(note: "⌥⌘Q 는 받는다(⌘Q 만 예약)", code: 12, raw: C | O, expected: .accept(key(12, C | O))),
    .init(note: "⌃⌘H 는 받는다", code: 4, raw: C | K, expected: .accept(key(4, C | K))),
    .init(note: "⇧⌘H 는 받는다", code: 4, raw: C | S, expected: .accept(key(4, C | S))),
    .init(note: "⌃⌥B 는 받는다", code: 11, raw: K | O, expected: .accept(key(11, K | O))),
    .init(note: "⌃ 만 있어도 받는다", code: 0, raw: K, expected: .accept(key(0, K))),
    .init(note: "⌥ 만 있어도 받는다", code: 0, raw: O, expected: .accept(key(0, O))),
    .init(note: "보드 안 다른 동작과 겹치는 키도 받는다(겹침은 그 줄에서 알린다)", code: 29, raw: C, expected: .accept(key(29, C))),
]

@Suite("단축키: 키 받기") struct ShortcutRecordTests {
    @Test("Esc 취소·⌫⌦ 비우기·수정키 없음과 예약 키 거절·그 밖 받기", arguments: recordRows)
    func 받기(_ r: RecordRow) {
        #expect(recordShortcut(key(r.code, r.raw), layout: us) == r.expected)
    }

    @Test("예약 키 안내 글자는 자판 글자로 — 자판에 글자가 없으면 #키 번호")
    func 예약안내자판() {
        let blank = FakeLayout(keys: [:])
        #expect(recordShortcut(key(12, C), layout: blank) == reservedText("⌘#12"))
    }
}

// MARK: - 시스템 단축키

@Suite("단축키: 시스템 단축키와 겹침") struct ShortcutSystemTests {
    @Test("[키 번호, Carbon 수정키] 가 목록에 그대로 있을 때만", arguments: [
        (11, K | O, UInt32(6144), true),     // ⌃⌥B — Carbon ⌃ 4096 + ⌥ 2048
        (11, K | O, UInt32(4096), false),    // 수정키가 다르면 아니다
        (49, C, UInt32(256), true),          // ⌘Space — Carbon ⌘ 256
        (49, C | S, UInt32(768), false),     // 목록에 없는 수정키
        (50, C, UInt32(256), false),         // 키 번호가 다르면 아니다
    ])
    func 겹침(_ code: Int, _ raw: UInt, _ carbon: UInt32, _ expected: Bool) {
        let system = [[11, 6144], [49, 256], [126, 4096]]
        #expect(systemShortcutTaken(key(code, raw), carbonMods: carbon, system: system) == expected)
    }

    @Test("목록이 비면 겹치지 않는다")
    func 빈목록() {
        #expect(systemShortcutTaken(key(11, K | O), carbonMods: 6144, system: []) == false)
    }
}
