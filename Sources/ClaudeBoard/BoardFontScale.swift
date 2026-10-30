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

// MARK: - 보드 글자 크기

// 보드 글자 비율 — 보드 설정 boardFontScale(기본 1, 80%~150%, 10%씩. 윈도우 메뉴 ⌥⌘+·⌥⌘-·⌥⌘0).
// 바뀌면 .boardFont 를 쓴 글자만 다시 그려진다 — 펼친 카드 같은 화면 상태는 그대로다(뿌리에 .id 를 바꾸는 방식은 @State 가 풀린다)
@MainActor final class BoardFontScale: ObservableObject {
    static let shared = BoardFontScale()
    @Published private(set) var scale = CGFloat(min(1.5, max(0.8, UserDefaults.standard.object(forKey: "boardFontScale") as? Double ?? 1)))

    // step: +1 크게, -1 작게, nil 원래 크기. 바뀐 비율을 보드 아래쪽에 잠깐 알린다
    func change(_ step: Int?) {
        let tenths = step.map { min(15, max(8, Int((scale * 10).rounded()) + $0)) } ?? 10
        scale = CGFloat(tenths) / 10
        if tenths == 10 { UserDefaults.standard.removeObject(forKey: "boardFontScale") }   // 원래 크기는 설정을 남기지 않는다
        else { UserDefaults.standard.set(Double(tenths) / 10, forKey: "boardFontScale") }
        TerminalHub.shared.toast("보드 글자 \(tenths * 10)%")
    }
}

struct BoardFont: ViewModifier {
    @ObservedObject var fontScale = BoardFontScale.shared
    let size: CGFloat
    let weight: Font.Weight
    let design: Font.Design
    func body(content: Content) -> some View {
        content.font(.system(size: size * fontScale.scale, weight: weight, design: design))
    }
}
