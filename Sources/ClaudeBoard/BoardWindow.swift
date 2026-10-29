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

// MARK: - 창

var window: NSWindow!

// 보드가 다른 앱 뒤에 있을 때도 첫 클릭이 창 활성화에만 쓰이지 않고 바로 단추에 닿게 한다
final class FirstClickHostingView<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

// 창을 손으로 줄일 때 최소 크기 아래로 내려가지 않게 한다
final class SizeGuard: NSObject, NSWindowDelegate {
    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        let min = sender.frameRect(forContentRect: NSRect(origin: .zero, size: sender.contentMinSize)).size
        return NSSize(width: max(frameSize.width, min.width), height: max(frameSize.height, min.height))
    }
    // 초록 단추(확대) — 터미널을 펼쳤으면 제목 막대 더블클릭과 같은 창 크기 전환. 접혔으면 시스템 확대 그대로
    func windowShouldZoom(_ window: NSWindow, toFrame newFrame: NSRect) -> Bool {
        return MainActor.assumeIsolated {
            guard TerminalHub.shared.expanded else { return true }
            toggleWindowSize()
            return false
        }
    }
}
let sizeGuard = SizeGuard()
let boardMinWidth: CGFloat = 360        // 전체·중간 화면이 무너지지 않는 폭 — 더 좁으면 미니로 그린다
let boardLaunchWidth: CGFloat = 454   // 실행할 때의 가로 폭 — 2026-10-13 사용자가 맞춰 둔 폭
