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

// MARK: - 힌트

// 시스템 툴팁(.help)은 1초 넘게 기다려야 뜨고, 보드가 초당 20번 다시 그려지면 그 기다림이 자꾸 처음부터 다시 시작돼
// 몇 초씩 걸리거나 아예 뜨지 않는다. 카드·스크롤 영역에 잘리지 않게 보드 위의 작은 패널 창에 직접 띄운다
final class HintPanel {
    static let shared = HintPanel()
    static let delay: TimeInterval = 0.35
    private let panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
    private let host = NSHostingController(rootView: HintLabel(text: ""))
    // 마우스가 들어가 있는 힌트 영역 — 안쪽 영역은 바깥 영역보다 나중에 들어가므로 마지막 것을 보인다
    private var stack: [(id: UUID, text: String)] = []
    private var pending: DispatchWorkItem?
    private var lastShown = Date.distantPast   // 방금까지 힌트가 떠 있었으면 옆 단추로 옮겨도 기다리지 않는다

    private init() {
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.level = .popUpMenu   // 위에 고정한 보드(.floating)보다 위
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        host.sizingOptions = []
        panel.contentView = host.view
        // 누르거나 스크롤하면 시스템 툴팁처럼 사라진다
        NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .scrollWheel]) { [weak self] e in
            self?.hide()
            return e
        }
    }

    func hover(_ id: UUID, _ text: String, _ inside: Bool) {
        // 마우스가 없던 영역이 사라질 때(줄 폭에 맞춰 라벨을 바꾸는 등)는 떠 있는 힌트를 건드리지 않는다
        if !inside, !stack.contains(where: { $0.id == id }) { return }
        stack.removeAll { $0.id == id }
        if inside { stack.append((id, text)) }
        hide()
        guard let top = stack.last else { return }
        let w = DispatchWorkItem { [weak self] in self?.show(top.text) }
        pending = w
        let warm = Date().timeIntervalSince(lastShown) < 0.4
        DispatchQueue.main.asyncAfter(deadline: .now() + (warm ? 0 : Self.delay), execute: w)
    }

    // 마우스가 보드 창 밖으로 나갔다 — 빠르게 나가면 영역마다의 나감 신호를 놓칠 수 있다
    func leaveAll() {
        stack.removeAll()
        hide()
    }

    func hide() {
        pending?.cancel()
        pending = nil
        if panel.isVisible {
            panel.orderOut(nil)
            lastShown = Date()
        }
    }

    private func show(_ text: String) {
        host.rootView = HintLabel(text: text)
        let size = host.sizeThatFits(in: CGSize(width: HintLabel.maxWidth, height: 1000))
        let m = NSEvent.mouseLocation
        guard let vf = (NSScreen.screens.first { $0.frame.contains(m) } ?? NSScreen.main)?.visibleFrame else { return }
        var o = NSPoint(x: m.x + 2, y: m.y - 22 - size.height)   // 시스템 툴팁처럼 커서 오른쪽 아래
        if o.y < vf.minY { o.y = m.y + 10 }                         // 아래 자리가 없으면 커서 위로
        o.x = max(vf.minX + 4, min(o.x, vf.maxX - size.width - 4))
        panel.setFrame(NSRect(origin: o, size: size), display: true)
        panel.invalidateShadow()       // 그림자를 새 크기에 맞춘다
        panel.orderFrontRegardless()   // 보드를 활성화하지 않는다 — 포커스는 쓰던 앱에 남긴다
    }
}

struct HintLabel: View {
    static let maxWidth: CGFloat = 280   // 긴 설명은 이 폭에서 줄을 바꾼다
    let text: String
    var body: some View {
        // 터미널 독의 힌트와 같은 색
        Text(text)
            .boardFont(11, weight: .medium)
            .foregroundStyle(Color.white.opacity(0.92))
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color(red: 0.10, green: 0.10, blue: 0.15).opacity(0.96)))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
    }
}

struct HintArea: ViewModifier {
    let text: String
    @State private var id = UUID()
    func body(content: Content) -> some View {
        content
            .onHover { HintPanel.shared.hover(id, text, $0) }
            // 상태가 바뀌어 뷰가 사라지면 마우스가 그 자리에 있어도 나감 신호가 오지 않는다
            .onDisappear { HintPanel.shared.hover(id, text, false) }
            .accessibilityHint(text)
    }
}

struct SummaryChip: View {
    let state: String
    let n: Int
    var filtering = false
    var tap: () -> Void = {}
    @State private var hover = false
    var body: some View {
        HStack(spacing: 5) {
            Circle().fill(Palette.color(state)).frame(width: 7, height: 7)
            Text("\(n)").boardFont(12, weight: .bold).monospacedDigit().contentTransition(.numericText())
        }
        .padding(.horizontal, 9)
        .padding(.vertical, 4)
        .background(Capsule().fill(Palette.color(state).opacity(filtering ? 0.35 : hover ? 0.25 : 0.15)))
        .overlay(Capsule().stroke(Palette.color(state).opacity(filtering ? 0.9 : 0), lineWidth: 1))
        .fixedSize()   // 윗줄이 좁아도 숫자가 잘리지 않게 — 모자라면 제목이 먼저 줄어든다
        .contentShape(Capsule())
        .onTapGesture(perform: tap)
        .clickable($hover)
        .hint(filtering ? "\(Palette.name(state))만 보는 중: 누르면 모두 보기"
                        : "\(Palette.name(state)) \(n)개: 누르면 \(Palette.name(state))만 보기")
    }
}

// 창 바탕: 짙은 남색 그라데이션 위에 위쪽에서 강조색 빛이 번진다 (블러가 비쳐 보이도록 살짝 투명)
struct Backdrop: View {
    var body: some View {
        ZStack {
            LinearGradient(colors: [Color(red: 0.12, green: 0.12, blue: 0.19), Color(red: 0.07, green: 0.07, blue: 0.11)],
                           startPoint: .top, endPoint: .bottom)
                .opacity(0.94)
            RadialGradient(colors: [Palette.accent.opacity(0.22), .clear], center: .topLeading,
                           startRadius: 0, endRadius: 360)
            RadialGradient(colors: [Color(red: 0.45, green: 0.50, blue: 1.0).opacity(0.14), .clear], center: .bottomTrailing,
                           startRadius: 0, endRadius: 420)
        }
        .ignoresSafeArea()
    }
}

// 설치된 Claude Code 버전 — 제목을 그릴 때마다 부르므로 1분 동안은 읽어 둔 값을 쓴다
private var versionCache: (at: Date, value: String?) = (.distantPast, nil)

func claudeVersion() -> String? {
    if Date().timeIntervalSince(versionCache.at) > 60 { versionCache = (Date(), readClaudeVersion()) }
    return versionCache.value
}

// ~/.local/bin/claude 가 가리키는 versions/<버전> 의 이름, 못 읽으면 versions 안 가장 높은 것
private func readClaudeVersion() -> String? {
    let home = NSHomeDirectory()
    if let t = try? FileManager.default.destinationOfSymbolicLink(atPath: home + "/.local/bin/claude") {
        let v = (t as NSString).lastPathComponent
        if v.first?.isNumber == true { return v }
    }
    let vs = (try? FileManager.default.contentsOfDirectory(atPath: home + "/.local/share/claude/versions")) ?? []
    return vs.filter { $0.first?.isNumber == true }.max { $0.compare($1, options: .numeric) == .orderedAscending }
}

// 감독의 말풍선 교체·눈 깜빡임 시계 — 보드가 1초마다 다시 그려져도 타이머가 새로 만들어지지 않게 객체에 둔다
final class ForemanClock: ObservableObject {
    @Published var line = 0
    @Published var blink = false
    // 말풍선이 바뀐 시각 — 감독이 그 순간 폴짝 뛴다
    private(set) var changedAt = Date().timeIntervalSinceReferenceDate
    private var timers: [Timer] = []
    init() {
        timers.append(Timer.scheduledTimer(withTimeInterval: 3.2, repeats: true) { [weak self] _ in self?.next() })
        timers.append(Timer.scheduledTimer(withTimeInterval: 3.7, repeats: true) { [weak self] _ in
            withAnimation(.easeIn(duration: 0.08)) { self?.blink = true }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.14) { withAnimation(.easeOut(duration: 0.08)) { self?.blink = false } }
        })
    }
    func next() {
        changedAt = Date().timeIntervalSinceReferenceDate
        withAnimation(.spring(response: 0.38, dampingFraction: 0.55)) { line += 1 }
    }
    deinit { timers.forEach { $0.invalidate() } }
}

// 감독의 기분 — 세션 목록에서 정한다
enum Mood { case urgent, busy, idle, sleepy }

// 말풍선 한 줄 — 기분과 함께
struct Quip: Equatable {
    let text: String
    let mood: Mood
}

// 지금 세션 목록·사용 한도로 감독이 할 말을 만든다
func foremanQuips(groups: [Group], limits: [Limit], now: Date) -> [Quip] {
    let live = groups.flatMap(\.sessions).filter { $0.state != "ended" }
    let name = { (s: Session) in (s.cwd as NSString).lastPathComponent }
    let short = { (t: String, n: Int) in t.count > n ? String(t.prefix(n)) + "…" : t }
    let waiting = live.filter { $0.state == "waiting" }
    let working = live.filter { $0.state == "working" }
    let idle = live.filter { $0.state == "idle" }
    let paused = live.filter { $0.state == "paused" }
    var out: [Quip] = []

    if !waiting.isEmpty {
        out.append(Quip(text: waiting.count == 1 ? "승인 기다리는 세션이 있어요!" : "승인 대기 \(waiting.count)개! 확인해 줘요", mood: .urgent))
        for s in waiting.prefix(3) {
            out.append(Quip(text: s.topic.isEmpty ? "\(name(s)) 에서 기다려요!" : "\(name(s)): \(short(s.topic, 12)), 승인 부탁!", mood: .urgent))
        }
        return out   // 승인 대기가 있으면 그것만 재촉한다
    }
    if !working.isEmpty {
        out.append(Quip(text: working.count == 1 ? "\(name(working[0])) 열일 중! 힘내!" : "세션 \(working.count)개 동시에 달리는 중!", mood: .busy))
        for s in working.prefix(2) {
            let mins = Int(now.timeIntervalSince(s.since) / 60)
            if mins >= 10 { out.append(Quip(text: "\(name(s)) \(mins)분째 작업 중… 조금만 더!", mood: .busy)) }
            else if !s.topic.isEmpty { out.append(Quip(text: "지금은 \(short(s.topic, 14)) 처리 중", mood: .busy)) }
        }
        let subs = working.reduce(0) { $0 + $1.subagents.filter { $0.state != "done" }.count }
        if subs > 0 { out.append(Quip(text: "서브에이전트 \(subs)명 출동!", mood: .busy)) }
        let shells = working.reduce(0) { $0 + $1.shells }
        if shells > 0 { out.append(Quip(text: "백그라운드 셸 \(shells)개 지켜보는 중", mood: .busy)) }
    }
    for s in live.filter({ ($0.ctx?.pct ?? 0) >= 80 }).prefix(2) {
        out.append(Quip(text: "\(name(s)) 컨텍스트 \(s.ctx!.pct)%! 정리할 때예요", mood: working.isEmpty ? .idle : .busy))
    }
    for l in limits where l.pct >= 80 {
        out.append(Quip(text: "\(l.label) 한도 \(Int(l.pct.rounded()))%, 아껴 써요!", mood: working.isEmpty ? .idle : .busy))
    }
    if !paused.isEmpty { out.append(Quip(text: "일시중지 \(paused.count)개, 잊지 마세요", mood: .idle)) }
    if working.isEmpty {
        if let s = idle.max(by: { $0.updated < $1.updated }) {
            let quiet = now.timeIntervalSince(s.updated)
            if quiet > 30 * 60 {
                out.append(Quip(text: "다들 쉬는 중… 할 일 없나요?", mood: .sleepy))
                out.append(Quip(text: "Zzz… 다음 지시 기다려요", mood: .sleepy))
            } else {
                out.append(Quip(text: "\(name(s)) 끝났어요! 확인해 볼까요?", mood: .idle))
                out.append(Quip(text: idle.count == 1 ? "다음 작업 뭐 할까요?" : "세션 \(idle.count)개 대기 중, 다음 지시!", mood: .idle))
            }
        } else if out.isEmpty {
            out.append(Quip(text: "세션을 열어 일을 맡겨 주세요!", mood: .sleepy))
            out.append(Quip(text: "오늘도 화이팅!", mood: .sleepy))
        }
    }
    return out
}

// 꼬리가 오른쪽(감독 쪽)으로 난 말풍선
struct SpeechBubble: Shape {
    func path(in r: CGRect) -> Path {
        let tail: CGFloat = 7
        let body = CGRect(x: r.minX, y: r.minY, width: r.width - tail, height: r.height)
        var p = Path(roundedRect: body, cornerRadius: min(10, body.height / 2))
        let y = body.midY + 2
        p.move(to: CGPoint(x: body.maxX - 1, y: y - 5))
        p.addQuadCurve(to: CGPoint(x: r.maxX, y: y + 3), control: CGPoint(x: body.maxX + 2, y: y))
        p.addQuadCurve(to: CGPoint(x: body.maxX - 1, y: y + 4), control: CGPoint(x: body.maxX + 1, y: y + 4))
        p.closeSubpath()
        return p
    }
}

// 제목 오른쪽의 작은 캐릭터 — 세션 상태에 따라 움직이고, 재촉하고, 졸고, 말풍선으로 상황을 알려 준다. 누르면 다음 말.
// 그림은 고른 캐릭터(브레인롯 7종 또는 현장 감독)로 그린다 — 오른쪽 클릭이나 설정 > 일반 에서 고른다
struct Foreman: View {
    let groups: [Group]
    let limits: [Limit]
    var compact = false     // 미니 화면 — 말풍선은 한 줄, 캐릭터는 조금 작게
    @StateObject private var clock = ForemanClock()
    @ObservedObject private var ticker = SecondTicker.shared   // "N분째 작업 중"·쉰 시간
    @ObservedObject private var fontScale = BoardFontScale.shared
    @AppStorage("mascot") private var mascot: Mascot = .tralalero

    // 승인 대기가 없으면 돌아가는 말 끝에 캐릭터 유행어를 한 번 끼운다
    private func withCatchphrase(_ quips: [Quip]) -> [Quip] {
        guard let c = mascot.catchphrase, let first = quips.first, first.mood != .urgent else { return quips }
        return quips + [Quip(text: c, mood: first.mood)]
    }

    var body: some View {
        let quips = withCatchphrase(foremanQuips(groups: groups, limits: limits, now: ticker.now))
        let q = quips[clock.line % max(quips.count, 1)]
        let tint: Color = switch q.mood {
        case .urgent: Palette.color("waiting")
        case .busy: Palette.color("working")
        case .idle: Palette.accent
        case .sleepy: Color(white: 0.45)
        }
        HStack(alignment: .center, spacing: 2) {
            Text(q.text)
                .boardFont(11, weight: .bold)
                .foregroundStyle(.white)
                .multilineTextAlignment(.leading)
                .lineLimit(compact ? 1 : 2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.leading, 9).padding(.trailing, 16).padding(.vertical, 5)
                .background(SpeechBubble().fill(tint.opacity(0.92)))
                .overlay(SpeechBubble().stroke(Color.white.opacity(0.18), lineWidth: 0.6))
                // 보드 글자를 키우면 제목(layoutPriority)이 자리를 다 가져가 말풍선이 글자 없이 좁아졌다 — 키웠을 때만 몇 글자는 보이게 최소 폭
                .frame(minWidth: compact || fontScale.scale <= 1 ? nil : 100, maxWidth: compact ? 230 : 170, alignment: .trailing)
                .id(q.text)
                .transition(.asymmetric(
                    insertion: .scale(scale: 0.4, anchor: .trailing).combined(with: .opacity),
                    removal: .scale(scale: 0.8, anchor: .trailing).combined(with: .opacity)))
            Ambient { t in
                ZStack {
                    if mascot == .foreman {
                        Self.figure(mood: q.mood, t: t, sinceChange: t - clock.changedAt, blink: clock.blink)
                    } else {
                        BrainrotFigure(kind: mascot, mood: q.mood, t: t, sinceChange: t - clock.changedAt, blink: clock.blink)
                    }
                }
                .scaleEffect(compact ? 0.75 : 1)
                .frame(width: compact ? 35 : 46, height: compact ? 33 : 44)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { clock.next() }
        .contextMenu {
            Picker("캐릭터", selection: $mascot) {
                ForEach(Mascot.allCases) { Text($0.name).tag($0) }
            }
            .pickerStyle(.inline)
        }
        .hint("\(mascot.name): 누르면 다음 말 · 오른쪽 클릭으로 캐릭터 바꾸기")
    }

    // 동작 — 모두 시계 t 의 함수라 반복 애니메이션 없이 공유 시계 하나로 움직인다
    static func figure(mood: Mood, t: TimeInterval, sinceChange: Double, blink: Bool) -> some View {
        let skin = mood == .urgent ? Color(red: 0.93, green: 0.42, blue: 0.32) : Color(red: 0.85, green: 0.47, blue: 0.34)
        // 말풍선이 바뀌면 폴짝 — 0.45초 동안 뛰었다 내려와 착지에 찌그러진다
        let hopP = min(max(sinceChange / 0.45, 0), 1)
        var hop = sinceChange < 0.45 ? -9 * sin(.pi * hopP) : 0
        var land = sinceChange >= 0.45 && sinceChange < 0.62 ? sin(.pi * (sinceChange - 0.45) / 0.17) : 0
        var tilt = 0.0, armDeg = 0.0, dx = 0.0, breathe = 0.0
        switch mood {
        case .urgent:
            // 제자리에서 연달아 뛰며 지휘봉을 머리 위로 흔든다
            let p = cycle(t, 0.55)
            hop = min(hop, -7 * sin(.pi * p))
            land = max(land, p > 0.85 ? sin(.pi * (p - 0.85) / 0.15) : 0)
            armDeg = 70 + 35 * sin(2 * .pi * t / 0.3)
            dx = 1.5 * sin(2 * .pi * t / 0.12)
        case .busy:
            // 지휘 — 지휘봉이 8자를 그리고 몸이 박자에 맞춰 기운다
            armDeg = 18 + 30 * sin(2 * .pi * t / 0.9) + 10 * sin(4 * .pi * t / 0.9)
            tilt = 7 * sin(2 * .pi * t / 0.9)
            hop = min(hop, -2.5 * abs(sin(2 * .pi * t / 0.9)))
        case .idle:
            // 말풍선 쪽을 두 번 콕콕 찌르고 쉰다
            let p = cycle(t, 2.4)
            let jab = p < 0.12 ? sin(.pi * p / 0.12) : (p > 0.18 && p < 0.30 ? sin(.pi * (p - 0.18) / 0.12) : 0)
            armDeg = 6 + 8 * jab
            dx = -2.5 * jab
            tilt = 5 * sin(2 * .pi * t / 2.4)
            breathe = swing(t, 1.6)
        case .sleepy:
            armDeg = -25
            tilt = 9 * sin(2 * .pi * t / 3.2)
            breathe = swing(t, 3.2)
        }
        let squash = land * 0.16
        return ZStack {
            // 지휘봉 든 팔 — 어깨를 축으로 돈다(양수 = 위로 든다)
            Capsule().fill(skin)
                .frame(width: 16, height: 4)
                .overlay(alignment: .leading) {
                    ZStack(alignment: .leading) {
                        Capsule().fill(Color(white: 0.95)).frame(width: 13, height: 2.5)
                        Circle().fill(mood == .urgent ? Color.yellow : Color.white).frame(width: 4, height: 4).offset(x: -2)
                    }
                    .offset(x: -11)
                }
                .offset(x: -9)
                .rotationEffect(.degrees(armDeg), anchor: .trailing)
                .offset(x: -6, y: 4)
            // 얼굴
            Circle().fill(skin)
                .frame(width: 28, height: 28)
                .overlay { eyes(mood: mood, t: t, blink: blink) }
                .overlay(alignment: .bottom) { mouth(mood: mood, t: t).offset(y: -5) }
                .overlay {
                    HStack(spacing: 13) {
                        ForEach(0..<2, id: \.self) { _ in
                            Ellipse().fill(Color(red: 1.0, green: 0.55, blue: 0.55).opacity(mood == .urgent ? 0.95 : 0.7)).frame(width: 5, height: 3)
                        }
                    }
                    .offset(y: 6)
                }
                .overlay(alignment: .top) {
                    // 안전모 — 착지할 때 살짝 들썩인다
                    UnevenRoundedRectangle(topLeadingRadius: 10, bottomLeadingRadius: 2, bottomTrailingRadius: 2, topTrailingRadius: 10)
                        .fill(Color(red: 1.0, green: 0.80, blue: 0.25))
                        .frame(width: 30, height: 10)
                        .offset(y: -4 - 2 * land)
                }
            // 머리 위 표시: 재촉 "!" · 졸음 "z"
            if mood == .urgent {
                Text("!").font(.system(size: 13, weight: .black)).foregroundStyle(Color.yellow)
                    .scaleEffect(1 + 0.25 * swing(t, 0.4))
                    .offset(x: 14, y: -19)
            } else if mood == .sleepy {
                ForEach(0..<2, id: \.self) { i in
                    let p = cycle(t + Double(i) * 1.2, 2.4)
                    Text("z").font(.system(size: 8 + 3 * p, weight: .bold)).foregroundStyle(.white.opacity(1 - p))
                        .offset(x: 12 + 6 * p, y: -10 - 14 * p)
                }
            }
        }
        .frame(width: 46, height: 44)
        .scaleEffect(x: 1 + squash + 0.03 * breathe, y: 1 - squash + 0.03 * breathe, anchor: .bottom)
        .rotationEffect(.degrees(tilt), anchor: .bottom)
        .offset(x: dx, y: hop + 2)
    }

    @ViewBuilder private static func eyes(mood: Mood, t: TimeInterval, blink: Bool) -> some View {
        HStack(spacing: 7) {
            ForEach(0..<2, id: \.self) { _ in
                switch mood {
                case .sleepy:
                    Capsule().fill(Color(white: 0.12)).frame(width: 5, height: 1.2)
                case .urgent:
                    Circle().fill(Color(white: 0.12)).frame(width: 5, height: 5)
                default:
                    Capsule().fill(Color(white: 0.12)).frame(width: 3.5, height: blink ? 0.8 : 5)
                }
            }
        }
        // 말풍선 쪽(왼쪽)을 흘끗 본다
        .offset(x: mood == .sleepy ? 0 : -1.5, y: 1)
    }

    @ViewBuilder private static func mouth(mood: Mood, t: TimeInterval) -> some View {
        switch mood {
        case .urgent:
            // 소리치는 입 — 열렸다 닫힌다
            Ellipse().fill(Color(white: 0.12)).frame(width: 7, height: 3 + 3 * swing(t, 0.3))
        case .busy:
            Capsule().fill(Color(white: 0.12)).frame(width: 8, height: 2)
        case .idle:
            // 웃는 입
            Path { p in p.move(to: CGPoint(x: 0, y: 0)); p.addQuadCurve(to: CGPoint(x: 8, y: 0), control: CGPoint(x: 4, y: 4)) }
                .stroke(Color(white: 0.12), style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
                .frame(width: 8, height: 4)
        case .sleepy:
            Circle().fill(Color(white: 0.12)).frame(width: 3, height: 3)
        }
    }
}
