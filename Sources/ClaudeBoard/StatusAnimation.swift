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

// MARK: - 상태 표시 (애니메이션)

// 늘 도는 애니메이션의 시계 — SwiftUI 반복 애니메이션은 화면 주사율만큼 창 전체를 다시 그려 클릭이 버벅이므로 초당 20번으로 묶는다.
// 한 번 돌 때마다 보드 화면 전체 배치를 다시 계산하므로(30번일 때 쉬는 보드 CPU 의 절반 남짓, 2026-10-26 실측) 회전이 끊겨 보이지 않는 만큼만 돈다
// 늘 도는 애니메이션은 시계 하나를 함께 본다 — 따로 돌면 시계마다 창 전체를 따로 다시 그린다
final class AmbientTicker: ObservableObject {
    static let shared = AmbientTicker()
    @Published private(set) var now = Date().timeIntervalSinceReferenceDate
    private var timer: Timer?
    // 창이 다른 창에 다 가려졌거나 숨었으면 멈춘다 — 보이지 않는 감독·회전 표시를 초당 20번 다시 그리지 않게
    var running = true { didSet { if running != oldValue { schedule() } } }
    // 보이는 세션 가운데 작업 중·승인 대기가 없으면 초당 5번으로 늦춘다 — 회전·파문·반짝임은 그때만 나오고, 쉬는 동안은 캐릭터가 조는 것만 움직인다
    var busy = true { didSet { if busy != oldValue { schedule() } } }
    private init() { schedule() }
    private func schedule() {
        timer?.invalidate()
        timer = nil
        guard running else { return }
        let t = Timer(timeInterval: busy ? 1.0 / 20 : 1.0 / 5, repeats: true) { [weak self] _ in self?.now = Date().timeIntervalSinceReferenceDate }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }
}
struct Ambient<Content: View>: View {
    @ObservedObject private var ticker = AmbientTicker.shared
    @ViewBuilder let content: (TimeInterval) -> Content
    var body: some View { content(ticker.now) }
}
func cycle(_ t: TimeInterval, _ period: Double) -> Double { (t / period).truncatingRemainder(dividingBy: 1) }
// 0 → 1 → 0 을 부드럽게 오가는 값 (period 동안 한 번 왕복)
func swing(_ t: TimeInterval, _ period: Double) -> Double { (1 - cos(2 * .pi * cycle(t, period))) / 2 }

// 작업 중: 그라데이션 호가 계속 회전한다
struct Spinner: View {
    let color: Color
    var body: some View {
        Ambient { now in
        ZStack {
            Circle().stroke(color.opacity(0.18), lineWidth: 2.5)
            Circle()
                .trim(from: 0, to: 0.72)
                .stroke(AngularGradient(colors: [color.opacity(0), color], center: .center),
                        style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                .rotationEffect(.degrees(cycle(now, 0.9) * 360))
        }
        }
    }
}

// 승인 대기: 점 둘레로 파문이 퍼진다
struct Pulse: View {
    let color: Color
    var body: some View {
        Ambient { now in
            let p = cycle(now, 1.1)
            let e = 1 - (1 - p) * (1 - p)   // easeOut
            ZStack {
                Circle().fill(color.opacity(0.35)).scaleEffect(0.8 + 1.1 * e).opacity(1 - e)
                Circle().fill(color).frame(width: 9, height: 9)
            }
        }
    }
}

// 대기: 체크 표시가 통통 튀며 나타난다
struct Check: View {
    let color: Color
    @State private var shown = false
    var body: some View {
        Image(systemName: "checkmark.circle.fill")
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(color)
            .scaleEffect(shown ? 1 : 0.3)
            .opacity(shown ? 1 : 0)
            .onAppear { withAnimation(.spring(response: 0.35, dampingFraction: 0.5)) { shown = true } }
    }
}

struct StateIcon: View {
    let state: String
    var body: some View {
        let c = Palette.color(state)
        ZStack {
            switch state {
            case "working": Spinner(color: c)
            case "waiting": Pulse(color: c)
            case "idle": Check(color: c)
            case "paused": Image(systemName: "pause.circle.fill").font(.system(size: 16, weight: .semibold)).foregroundStyle(c)
            default:
                Image(systemName: "minus.circle.fill").font(.system(size: 15)).foregroundStyle(Color(red: 0.93, green: 0.33, blue: 0.33))
            }
        }
        .frame(width: 18, height: 18)
        .id(state)   // 상태가 바뀌면 새로 그려 등장 애니메이션을 다시 낸다
        .transition(.scale.combined(with: .opacity))
    }
}

// 작업 중인 줄 위를 은은한 빛이 지나간다
struct Shimmer: ViewModifier {
    let active: Bool
    let color: Color
    func body(content: Content) -> some View {
        content.overlay {
            if active {
                GeometryReader { geo in
                    Ambient { now in
                        let phase = -1 + 2 * cycle(now, 2.4)
                        LinearGradient(colors: [.clear, color.opacity(0.10), .clear], startPoint: .leading, endPoint: .trailing)
                            .frame(width: geo.size.width * 0.5)
                            .offset(x: phase * geo.size.width * 1.5)
                    }
                }
                .allowsHitTesting(false)
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }
        }
    }
}
