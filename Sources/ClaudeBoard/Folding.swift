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

// MARK: - 접기·펼치기

let foldSpring = Animation.spring(response: 0.38, dampingFraction: 0.86)

// 펼칠 때 줄마다 조금씩 늦게 위에서 내려오며 나타난다
func foldIn(_ i: Int) -> AnyTransition {
    .asymmetric(insertion: .opacity.combined(with: .offset(y: -10))
                    .animation(foldSpring.delay(Double(min(i, 8)) * 0.035)),
                removal: .opacity.animation(.easeOut(duration: 0.15)))
}

// 마우스를 올리면 오른쪽에 나타나는 접기·펼치기 안내
// 그룹 카드 머리의 작은 아이콘 단추(상단 고정·목록에서 지우기)
struct HeaderButton: View {
    let icon: String
    let tint: Color?
    let help: String
    let action: @MainActor () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(tint ?? .primary.opacity(hover ? 0.95 : 0.55))
                .frame(width: 26, height: 26)
                .background(Circle().fill(Color.white.opacity(hover ? 0.16 : 0.06)))
                .contentShape(Circle())
                .scaleEffect(hover ? 1.3 : 1)   // 마우스를 올리면 커져서 누르기 쉽다
                .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hover)
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(help)
        .transition(.opacity.combined(with: .scale(scale: 0.8)))
    }
}

// "종료된 세션 N개" 줄에 마우스를 올리면 나오는 휴지통 — 그 작업경로의 종료된 세션을 한 번에 지운다
struct ClearEndedButton: View {
    let count: Int
    let action: () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            HStack(spacing: 3) {
                Image(systemName: "trash").font(.system(size: 9, weight: .bold))
                if hover { Text("모두 지우기").boardFont(10, weight: .semibold) }
            }
            .foregroundStyle(hover ? Color.red : .primary.opacity(0.8))
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(Capsule().fill(hover ? Color.red.opacity(0.18) : Color.white.opacity(0.10)))
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("종료된 세션 \(count)개를 목록에서 지우기")
        .transition(.opacity.combined(with: .move(edge: .trailing)))
    }
}

// 조건부 clickable — 잠긴 단추에는 손가락 커서를 보이지 않는다
struct ClickableIf: ViewModifier {
    let active: Bool
    @Binding var hover: Bool
    func body(content: Content) -> some View {
        if active { content.clickable($hover) } else { content }
    }
}

extension View {
    // 클릭할 수 있는 영역: 손가락 커서를 보이고 hover 상태를 갱신한다
    func clickable(_ hover: Binding<Bool>) -> some View {
        onContinuousHover { phase in
            var inside = false
            if case .active = phase { inside = true }
            (inside ? NSCursor.pointingHand : NSCursor.arrow).set()
            if hover.wrappedValue != inside {
                withAnimation(.easeOut(duration: 0.15)) { hover.wrappedValue = inside }
            }
        }
    }

    // 마우스를 올리면 곧 뜨는 짧은 설명 — 시스템 툴팁(.help) 대신 쓴다
    func hint(_ text: String) -> some View { modifier(HintArea(text: text)) }

    // 보드 글자 — 크기에 보드 글자 비율(BoardFontScale)을 곱한다. 보드 창의 SwiftUI 글자는 이것으로 쓴다(아이콘은 .font 그대로)
    func boardFont(_ size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default) -> some View {
        modifier(BoardFont(size: size, weight: weight, design: design))
    }
}
