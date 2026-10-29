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

// MARK: - 창 크기 단계

// 창 크기에 따라 보드를 세 단계로 그린다.
// 전체 = 기본 화면, 중간 = 사용 한도는 한 줄·경로 카드는 접힌 한 줄 요약, 미니 = 캐릭터 한 줄과 경로마다 한 줄.
// 제목 오른쪽 캐릭터는 세 단계 모두에서 보인다
enum BoardTier { case full, medium, mini }

let miniWidth: CGFloat = 300            // 최대축소 폭이자 창의 최소 폭
let miniMinHeight: CGFloat = 144        // 창의 최소 높이 — 미니에서 캐릭터 줄·경로 두 줄·아래 줄이 보이는 높이
let miniRowHeight: CGFloat = 26         // 미니의 경로 한 줄 높이 — 최대축소할 때 경로 수만큼 창 높이를 잡는다
let fullListMin: CGFloat = 120          // 전체로 그리려면 남아야 하는 목록 칸 — 펼친 카드 하나쯤
let mediumListMin: CGFloat = 56         // 중간으로 그리려면 남아야 하는 목록 칸 — 접힌 카드 하나

@MainActor final class BoardFit: ObservableObject {
    static let shared = BoardFit()
    @Published private(set) var tier: BoardTier = .full
    // 단계마다 목록 칸을 뺀 보드 내용 높이 — 그 단계로 그려졌을 때만 잴 수 있어 잰 값을 기억한다(재기 전엔 어림값)
    private var fixed: [BoardTier: CGFloat] = [.full: 330, .medium: 200]

    // 그려진 단계의 보드 높이와 목록 칸 높이를 받는다 — 지금 단계로 그린 값만 쓴다
    func measured(_ t: BoardTier, board: CGFloat, list: CGFloat) {
        guard t == tier, t != .mini, board > 0, abs((fixed[t] ?? 0) - (board - list)) > 0.5 else { return }
        fixed[t] = board - list
        update()
    }

    // 창 크기나 잰 값이 바뀌면 단계를 다시 고른다 — 창 크기는 바꾸지 않는다
    func update() {
        guard let w = window else { return }
        let h = w.frame.height, titlebar = h - w.contentLayoutRect.height   // 보드 내용은 제목 막대 아래에서 시작한다
        let fits = { (t: BoardTier, list: CGFloat) in h >= (self.fixed[t] ?? 0) + list + titlebar }
        let t: BoardTier = boardPaneWidth() < boardMinWidth ? .mini   // 터미널을 펼쳤으면 창이 아니라 보드 칸의 폭
            : fits(.full, fullListMin) ? .full : fits(.medium, mediumListMin) ? .medium : .mini
        guard t != tier else { return }
        // 미니를 벗어남 — 되돌리기 단추가 아닌 길(손으로 키움·펼치기·처음 크기로 붙이기)이었으면 최대축소 때의 펼침 기억을 지운다
        if tier == .mini { UserDefaults.standard.removeObject(forKey: "miniFromExpanded") }
        tier = t
        UserDefaults.standard.set(t == .mini, forKey: "boardMini")   // 미니로 둔 채 끄면 다음에도 그 크기로 연다
    }
}

// 미니에서 ⤢ 로 돌아갈 창 위치 — 최대축소할 때, 미니가 아닌 크기로 손으로 바꾼 뒤, 처음 크기로 붙일 때, 미니가 아닌 채 실행할 때 기억한다.
// 창이 줄어드는 애니메이션 중간 크기가 기억되지 않게 크기가 바뀔 때마다 기억하지는 않는다
func saveRestoreFrame() { UserDefaults.standard.set(NSStringFromRect(window.frame), forKey: "boardRestoreFrame") }

// 최대축소 — 보드 쪽 위 모서리(보드 위치가 오른쪽이면 오른쪽 위, 왼쪽이면 왼쪽 위)는 그대로 두고 폭은 miniWidth, 높이는 경로 수(최대 6줄)에 맞춘다
@MainActor func enterMini(rows: Int) {
    // 펼쳐 있었으면 기억해 두었다가 원래 크기로 되돌릴 때 다시 펼친다(다시 띄워도 맞게 보드 설정에). 손으로 미니를 벗어나면 지운다(BoardFit)
    UserDefaults.standard.set(TerminalHub.shared.expanded, forKey: "miniFromExpanded")
    setTerminalExpanded(false, animated: false)   // 터미널을 펼쳤으면 먼저 접는다 — 미니에서 돌아갈 위치는 접힌 보드
    saveRestoreFrame()
    let f = window.frame
    // 최소 높이에 두 줄이 들어간다. 보드 글자를 키웠으면 그만큼 높인다(150% 에서 둘째 줄이 아래 줄에 가려졌다)
    let h = (miniMinHeight + CGFloat(min(max(rows, 2), 6) - 2) * miniRowHeight) * max(1, BoardFontScale.shared.scale)
    let x = TerminalHub.shared.boardOnLeft ? f.minX : f.maxX - miniWidth
    window.setFrame(NSRect(x: x, y: f.maxY - h, width: miniWidth, height: h), display: true, animate: true)
}

// 미니에서 원래 크기로 — 최대축소할 때 터미널을 펼쳐 있었으면 다시 펼친다. 아니면 접힌 보드 위치로(기억한 위치가 없거나
// 지금 연결된 화면 밖이면 처음 크기로 보드 쪽 끝에 붙인다)
@MainActor func leaveMini() {
    if UserDefaults.standard.bool(forKey: "miniFromExpanded") {
        UserDefaults.standard.removeObject(forKey: "miniFromExpanded")
        setTerminalExpanded(true)
        return
    }
    if let s = UserDefaults.standard.string(forKey: "boardRestoreFrame") {
        let r = NSRectFromString(s)
        if r.width >= boardMinWidth, NSScreen.screens.contains(where: { $0.visibleFrame.intersects(r) }) {
            window.setFrame(r, display: true, animate: true)
            return
        }
    }
    snapToSide()
}

// 제목 막대 오른쪽 — 미니에서는 상태별 세션 수와 원래 크기로 되돌리기 단추, 전체·중간에서는 같은 자리에 최대축소 단추
func addMiniTitlebar(_ store: Store) {
    let vc = NSTitlebarAccessoryViewController()
    let host = FirstClickHostingView(rootView: MiniTitleBar(store: store))
    host.frame = NSRect(x: 0, y: 0, width: MiniTitleBar.width, height: 28)   // 제목 막대 높이와 같게 — 더 높으면 제목 막대가 커진다
    vc.view = host
    vc.layoutAttribute = .trailing
    window.addTitlebarAccessoryViewController(vc)
}

struct MiniTitleBar: View {
    static let width: CGFloat = 176     // 미니 폭 300 에서 창 단추·붙이기 단추를 뺀 자리
    @ObservedObject var store: Store
    @ObservedObject private var fit = BoardFit.shared
    @State private var hover = false

    // 상태별 세션 수 — 자리가 모자라면 글자를 빼고 수만
    private func chips(labels: Bool) -> some View {
        HStack(spacing: 4) {
            ForEach(["waiting", "working", "idle"], id: \.self) { st in
                let n = store.count(st)
                if n > 0 {
                    HStack(spacing: 3) {
                        Circle().fill(Palette.color(st)).frame(width: 6, height: 6)
                        Text(labels ? "\(n) \(["waiting": "승인", "working": "작업", "idle": "대기"][st] ?? "")" : "\(n)")
                            .monospacedDigit()
                    }
                    .font(.system(size: 10.5, weight: .bold))
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(Capsule().fill(Palette.color(st).opacity(0.15)))
                    .fixedSize()
                    .hint("\(Palette.name(st)) \(n)개")
                }
            }
        }
    }

    var body: some View {
        let mini = fit.tier == .mini
        HStack(spacing: 5) {
            Spacer(minLength: 0)
            if mini {
                ViewThatFits(in: .horizontal) { chips(labels: true); chips(labels: false) }
            }
            // 미니 = 원래 크기로, 전체·중간 = 최대축소(창을 가장 작게 줄여 경로마다 한 줄로 본다)
            Button { if mini { leaveMini() } else { enterMini(rows: store.groups.count) } } label: {
                Image(systemName: mini ? "arrow.up.left.and.arrow.down.right" : "arrow.down.right.and.arrow.up.left")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(hover ? .primary : .secondary)
                    .frame(width: 24, height: 20)
                    .background(Capsule().fill(Color.white.opacity(hover ? 0.16 : 0.08)))
                    .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .clickable($hover)
            .hint(mini ? "원래 크기로 되돌리기" : "최대축소: 창을 가장 작게 줄이고 경로마다 한 줄로 봅니다")
        }
        .padding(.trailing, 8)
        .frame(width: Self.width, height: 28)
        .preferredColorScheme(.dark)
    }
}

// 미니 화면 — 경로마다 한 줄(상태 · 이름 · 지금 하는 일), 아래 줄에 사용 한도와 투명도 슬라이더
struct MiniBoard: View {
    @ObservedObject var store: Store
    @ObservedObject var usage: UsageStore
    @ObservedObject private var autopilot = AutopilotStore.shared

    // 사용 한도 한 줄 — 앞에서 n 개만
    private func limits(_ n: Int) -> some View {
        HStack(spacing: 6) {
            ForEach(Array(usage.limits.prefix(n).enumerated()), id: \.element.id) { i, l in
                if i > 0 { Rectangle().fill(Color.white.opacity(0.14)).frame(width: 0.5, height: 10) }
                CompactLimit(limit: l, level: 2)
            }
        }
        .fixedSize()
    }

    var body: some View {
        VStack(spacing: 4) {
            // 캐릭터와 말풍선 — 최소 크기에서도 보인다
            HStack(spacing: 0) {
                Spacer(minLength: 0)
                Foreman(groups: store.groups, limits: usage.limits, compact: true)
            }
            if store.groups.isEmpty && autopilot.runs.isEmpty && autopilot.awaiting.isEmpty {
                Text("아직 기록된 세션이 없습니다").boardFont(11).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
            } else {
                ScrollView(showsIndicators: false) {
                    VStack(spacing: 2) {
                        // 작은 크기의 실행 줄 — 상태 꼬리표와 "2/4 제목" 한 줄, 막대는 뺌(와이어프레임 보드 본체 ⑧)
                        ForEach(autopilot.runs) { r in
                            EverySecond { now in APRunLine(run: r, minutes: r.unitMinutes(now), compact: true).equatable() }
                        }
                        ForEach(autopilot.awaiting) { r in APEndedLine(run: r, compact: true).equatable() }
                        ForEach(store.groups) { g in MiniRow(g: g) }
                    }
                }
            }
            HStack(spacing: 6) {
                // 자리가 모자라면 한도를 앞의 둘만 — 투명도 슬라이더는 늘 보인다
                ViewThatFits(in: .horizontal) { limits(usage.limits.count); limits(2); Color.clear.frame(width: 0, height: 0) }
                Spacer(minLength: 4)
                HeaderButton(icon: "clock.arrow.circlepath", tint: nil, help: "자율주행 지난 실행") { autopilot.screen = .history(nil) }
                    .scaleEffect(0.8)
                    .accessibilityIdentifier("ap-history-mini")
                HeaderButton(icon: "steeringwheel", tint: nil, help: "자율주행 출발 준비(닫아 둔 준비가 있으면 그것을 엽니다)") { APStart.shared.open() }
                    .scaleEffect(0.8)
                    .accessibilityIdentifier("ap-new-mini")
                OpacitySlider(compact: true)
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 2)
        .padding(.bottom, 6)
        .animation(.spring(response: 0.45, dampingFraction: 0.82), value: store.groups)
    }
}

// 미니 화면의 경로 한 줄 — 누르면 대표 세션의 터미널 앱을, 두 번 누르면 그 세션의 터미널 창을 앞으로
struct MiniRow: View {
    let g: Group
    @State private var hover = false

    var body: some View {
        let top = g.active.first
        let first = g.sessions.first
        let live = top ?? g.sessions.first { $0.state != "ended" }
        HStack(spacing: 8) {
            StateIcon(state: g.urgent).scaleEffect(0.8).frame(width: 15, height: 15)
            Text(g.name).boardFont(12, weight: .semibold).lineLimit(1).layoutPriority(1)
            HStack(spacing: 4) {
                // 줄에 보이는 세션이 터미널에서 도는지 백그라운드로 도는지 — 아이콘만
                if let s = top ?? first { KindLabel(headless: s.headless && TerminalHub.shared.parkedTab(s) == nil, compact: true) }
                if let top {
                    Text(doing(top)).lineLimit(1).foregroundStyle(Palette.color(top.state))
                    if g.active.count > 1 { Text("+\(g.active.count - 1)").fixedSize().foregroundStyle(Palette.color(top.state)) }
                } else if let s = first {
                    EverySecond { now in
                        Text("\(Palette.name(s.state)) · " + (s.state == "idle" ? "\(elapsed(s.since, now)) 전"
                                                              : s.state == "ended" ? clock(s.updated) : s.title))
                            .lineLimit(1).foregroundStyle(.secondary)
                    }
                }
            }
            .boardFont(11)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .frame(height: miniRowHeight - 2)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(hover ? 0.07 : 0)))
        .modifier(Shimmer(active: g.urgent == "working", color: Palette.color("working")))
        .contentShape(Rectangle())
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
        // 세션 줄과 같게 클릭 횟수로 나눈다 — 두 제스처로 나누면 한 번 클릭이 0.5초 늦는다
        .onTapGesture {
            guard let s = live else { return }
            openSession(s, double: (NSApp.currentEvent?.clickCount ?? 1) >= 2)
        }
        .hint(live.map { s in
            switch TerminalHub.shared.shownPlace(s) {
            case .tab: return "\(g.path)\n눌러서 터미널을 펼치고 그 탭으로 이동"
            case .terminal: return "\(g.path)\n눌러서 터미널 앱, 두 번 눌러서 그 세션으로 이동"
            case .background, .ended: return g.path
            }
        } ?? g.path)
    }
}

// 처음 실행할 때의 크기(폭 boardLaunchWidth, 높이는 화면의 쓸 수 있는 높이 전체)로 되돌려 지금 화면의 보드 쪽 끝(보드 위치 — 오른쪽·왼쪽)에 붙인다.
// 터미널을 펼쳤으면 접는다 — "보드만 한쪽에" 라는 뜻을 그대로 둔다(펼치기는 ⌃⌥T)
@MainActor func snapToSide() {
    setTerminalExpanded(false, animated: false)   // 바로 아래에서 제 애니메이션으로 붙인다
    guard let vf = (window.screen ?? NSScreen.main)?.visibleFrame else { return }
    let x = TerminalHub.shared.boardOnLeft ? vf.minX : vf.maxX - boardLaunchWidth
    let f = NSRect(x: x, y: vf.minY, width: boardLaunchWidth, height: vf.height)
    window.setFrame(f, display: true, animate: true)
    saveRestoreFrame()
}

// 창 단추(닫기·최소화·확대) 바로 오른쪽에 붙는 "처음 크기로 오른쪽(왼쪽)에 붙이기" 단추 — 보드 위치에 따라 그림·설명이 바뀐다
struct SnapButton: View {
    @ObservedObject private var hub = TerminalHub.shared
    @ObservedObject var keys = Shortcuts.shared   // 안내 글자의 단축키
    @State private var hover = false
    var body: some View {
        Button(action: snapToSide) {
            Image(systemName: hub.boardOnLeft ? "rectangle.lefthalf.inset.filled" : "rectangle.righthalf.inset.filled")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.primary.opacity(hover ? 1 : 0.8))
                .frame(width: 26, height: 18)
                .background(Capsule().fill(Color.white.opacity(hover ? 0.30 : 0.18)))
                .overlay(Capsule().stroke(Color.white.opacity(0.18), lineWidth: 0.5))
                .scaleEffect(hover ? 1.12 : 1)
                .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hover)
                .frame(width: 34, height: 28)   // 누를 수 있는 영역은 캡슐보다 넉넉하게
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("처음 크기로 되돌려 화면 \(hub.boardOnLeft ? "왼쪽" : "오른쪽")에 붙이기" + (keys.label("snap").map { " (\($0))" } ?? ""))
        .padding(.leading, 4)
    }
}
