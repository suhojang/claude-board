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

// MARK: - 화면

// 세션 실행 방식 라벨: 터미널 창에서 도는 세션인지, 터미널 없이 백그라운드로 도는 세션인지
struct KindLabel: View {
    var headless = false
    var subagent = false
    var compact = false     // 줄이 좁으면 글자를 빼고 아이콘만 보인다
    var body: some View {
        let c = subagent ? Color(red: 0.95, green: 0.55, blue: 0.75)
            : headless ? Color(red: 0.72, green: 0.55, blue: 1.00) : Color(red: 0.32, green: 0.78, blue: 0.86)
        let name = subagent ? "서브에이전트" : headless ? "백그라운드" : "터미널"
        Label(name, systemImage: subagent ? "person.2.fill" : headless ? "moon.stars.fill" : "terminal.fill")
            .boardFont(10, weight: .semibold)
            .labelStyle(compact ? AnyLabelStyle(.iconOnly) : AnyLabelStyle(.titleAndIcon))
            .fixedSize()
            .hint(name)
            .foregroundStyle(c)
            .padding(.horizontal, 6)
            .padding(.vertical, 1.5)
            .background(Capsule().fill(c.opacity(0.15)))
            .overlay(Capsule().stroke(c.opacity(0.25), lineWidth: 0.5))
    }
}

// 세션 칩 — 어디서 도는지: 보드 탭 CB(탭 안에서 시작된 claude -p 는 CB -p), 다른 터미널은 앱 이름,
// 백그라운드는 실제 실행 방식($ claude -p·--bg). 끝난 세션은 누르면 보드에서 이어 연다는 표시
struct PlaceChip: View {
    let s: Session
    let place: SessionPlace
    let appName: String?
    let mode: String?   // -p·--bg — 대화형이면 nil

    private func chip(_ text: String, _ bg: Color, _ fg: Color, mono: Bool = false, hint: String) -> some View {
        Text(text)
            .boardFont(10, weight: .semibold, design: mono ? .monospaced : .default)
            .foregroundStyle(fg)
            .padding(.horizontal, 6).padding(.vertical, 1.5)
            .background(Capsule().fill(bg))
            .fixedSize()
            .hint(hint)
    }

    var body: some View {
        switch place {
        case .tab(_, let headless):
            chip(mode.map { "CB \($0)" } ?? "CB", Color(red: 0.18, green: 0.23, blue: 0.34), Color(red: 0.66, green: 0.76, blue: 1.0),
                 hint: headless || mode != nil ? "보드 탭 안에서 시작된 claude \(mode ?? "-p") (탭이 닫히면 함께 끝납니다)" : "보드 탭에서 도는 세션: 누르면 그 탭으로")
        case .terminal:
            if let appName {
                chip(appName, Color(red: 0.14, green: 0.25, blue: 0.29), Color(red: 0.56, green: 0.85, blue: 0.91), hint: "\(appName) 에서 도는 세션")
            }
        case .background:
            chip("$ claude \(mode ?? "-p")", Color(red: 0.23, green: 0.20, blue: 0.27), Color(red: 0.83, green: 0.72, blue: 1.0), mono: true,
                 hint: "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다")
        case .ended:
            if s.headless {
                chip("$ claude -p", Color.white.opacity(0.08), .secondary, mono: true, hint: "끝난 백그라운드 세션입니다. 메뉴의 세션 다시 열기로 보드 탭에서 엽니다")
            } else {
                chip("보드에서 이어 열기", Color.white.opacity(0.08), .secondary, hint: "누르면 보드 새 탭에서 이 대화를 이어 엽니다")
            }
        }
    }
}

// 세션 이름 라벨 — Claude Code 의 세션 이름(세션끼리 메시지를 보낼 때 주소). 보드 탭 세션은 탭 줄 이름과 같다.
// 긴 이름은 가운데를 줄여 끝의 번호(-2 등)가 보이게 한다
struct SessionNameLabel: View {
    let name: String
    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: "tag").font(.system(size: 8, weight: .semibold))
            Text(name.count > 24 ? "\(name.prefix(11))…\(name.suffix(11))" : name)
        }
        .boardFont(10, weight: .semibold)
        .foregroundStyle(Color.white.opacity(0.72))
        .padding(.horizontal, 6)
        .padding(.vertical, 1.5)
        .background(Capsule().fill(Color.white.opacity(0.08)))
        .fixedSize()
        .overlay(Capsule().stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .hint("세션 이름 \(name)")
    }
}

func tokens(_ n: Int) -> String {
    n >= 1_000_000 ? String(format: "%.1fM", Double(n) / 1_000_000) : n >= 1000 ? "\(n / 1000)K" : "\(n)"
}

// 컨텍스트 사용량 라벨: 작은 막대 + 비율 + 토큰 수 (30% 미만 초록, 70% 미만 노랑, 그 이상 빨강 — 상태줄과 같은 기준)
struct ContextLabel: View {
    let ctx: Ctx
    var level = 0           // 0 막대·비율·토큰, 1 막대 없이 비율·토큰, 2 비율만 — 줄이 좁을수록 줄인다
    var body: some View {
        let c = ctx.pct < 30 ? Color(red: 0.42, green: 0.85, blue: 0.60)
            : ctx.pct < 70 ? Color(red: 1.00, green: 0.80, blue: 0.35) : Color(red: 1.00, green: 0.42, blue: 0.42)
        HStack(spacing: 4) {
            Image(systemName: "brain.head.profile").font(.system(size: 9, weight: .semibold))
            if level == 0 {
                ZStack(alignment: .leading) {
                    Capsule().fill(c.opacity(0.18))
                    Capsule().fill(c).frame(width: max(2, 26 * CGFloat(ctx.pct) / 100))
                }
                .frame(width: 26, height: 4)
            }
            Text(level == 2 ? "\(ctx.pct)%" : "\(ctx.pct)% · \(tokens(ctx.used))").monospacedDigit().contentTransition(.numericText())
        }
        .boardFont(10, weight: .semibold)
        .foregroundStyle(c)
        .padding(.horizontal, 6)
        .padding(.vertical, 1.5)
        .background(Capsule().fill(c.opacity(0.12)))
        .fixedSize()   // 정보 줄이 길어져도 라벨 안 글자가 두 줄로 꺾이지 않게
        .overlay(Capsule().stroke(c.opacity(0.22), lineWidth: 0.5))
        .hint("컨텍스트 \(ctx.used.formatted()) / \(ctx.size.formatted()) 토큰")
        .animation(.easeInOut(duration: 0.4), value: ctx)
    }
}

// 모델·effort 라벨: 계열 이름(색으로 계열 구분) + effort 막대 5칸(low 1 … max 5)
struct ModelLabel: View {
    let brain: Brain
    var level = 0           // 0 계열·버전, 1 계열만, 2 계열 첫 글자 — 줄이 좁을수록 줄인다
    var body: some View {
        let c = Self.color(brain.family)
        HStack(spacing: 4) {
            Text(level == 0 ? [brain.family, brain.version].filter { !$0.isEmpty }.joined(separator: " ")
                 : level == 1 ? brain.family : String(brain.family.prefix(1)))
            if brain.effortLevel > 0 { EffortBars(level: brain.effortLevel, color: c) }
        }
        .boardFont(10, weight: .semibold)
        .foregroundStyle(c)
        .padding(.horizontal, 6)
        .padding(.vertical, 1.5)
        .background(Capsule().fill(c.opacity(0.12)))
        .fixedSize()
        .overlay(Capsule().stroke(c.opacity(0.22), lineWidth: 0.5))
        .hint("모델 \(brain.model)" + (brain.effort.isEmpty ? "" : " · effort \(brain.effort)"))
    }

    static func color(_ family: String) -> Color {
        switch family {
        case "Opus": Color(red: 1.00, green: 0.60, blue: 0.45)
        case "Sonnet": Color(red: 0.48, green: 0.70, blue: 1.00)
        case "Haiku": Color(red: 0.55, green: 0.88, blue: 0.80)
        case "Fable": Color(red: 0.88, green: 0.70, blue: 1.00)
        default: Color(white: 0.75)
        }
    }
}

// effort 막대 — 키가 점점 커지는 5칸, 채운 칸 수 = effort 단계
struct EffortBars: View {
    let level: Int
    let color: Color
    var body: some View {
        HStack(alignment: .bottom, spacing: 1.2) {
            ForEach(1...5, id: \.self) { i in
                RoundedRectangle(cornerRadius: 0.8)
                    .fill(i <= level ? color : color.opacity(0.22))
                    .frame(width: 2.2, height: 3 + CGFloat(i) * 1.3)
            }
        }
        .frame(height: 9.5, alignment: .bottom)
    }
}

// 남은 시간 + 초기화 시각 — 하루 넘게 남으면 날짜를 붙인다
func resetText(_ d: Date?, _ now: Date) -> String {
    guard let d else { return "" }
    let s = Int(d.timeIntervalSince(now))
    if s <= 0 { return "곧 초기화" }
    let left = s >= 86400 ? "\(s / 86400)일 \(s % 86400 / 3600)시간" : s >= 3600 ? "\(s / 3600)시간 \(s % 3600 / 60)분" : "\(max(1, s / 60))분"
    return "\(left) 뒤 · \((s >= 86400 ? dayHourMinute : hourMinute).string(from: d))"
}

// 사용 한도 카드: 5시간·주간 전체·주간 모델별 막대. 머리에 로그인 계정(눌러서 전환), 오른쪽 위 단추로 간략(한 줄)·상세를 바꾼다
struct UsagePanel: View {
    @ObservedObject var usage: UsageStore
    @ObservedObject var accounts: AccountStore
    var forceCompact = false    // 창이 낮아 중간 화면일 때 — 저장한 간략·상세 설정은 두고 한 줄로만 보인다
    var stoplines: [String: Double]? = nil   // 자율주행 보기: 한도 id(5h·7d) → 멈출 선(%) 눈금
    let busy: () -> [Session]
    @AppStorage("usageCompact") private var compactSetting = false
    // 자율주행 보기는 멈출 선 눈금이 보이게 상세로(간략 한 줄은 좁으면 막대를 뺀다). 중간 화면은 그래도 한 줄
    private var compact: Bool { (compactSetting && stoplines == nil) || forceCompact }

    static func color(_ p: Double) -> Color {
        p < 50 ? Color(red: 0.42, green: 0.85, blue: 0.60) : p < 80 ? Color(red: 1.00, green: 0.70, blue: 0.36)
            : Color(red: 1.00, green: 0.42, blue: 0.42)
    }

    private var status: some View {
        Label(usage.tokenExpired ? "로그인 토큰 갱신 전: 세션이 다음 요청을 보내면 보입니다"
              : usage.failed ? "사용량을 불러오지 못했습니다" : "사용량 불러오는 중…",
              systemImage: "gauge.with.dots.needle.33percent")
            .boardFont(11).foregroundStyle(.secondary).lineLimit(1)
    }

    // 간략 모드 한 줄 — 넓으면 이름·막대·비율, 좁으면 막대를 빼고, 더 좁으면 이름을 줄인다
    private func line(_ level: Int) -> some View {
        HStack(spacing: 8) {
            ForEach(Array(usage.limits.enumerated()), id: \.element.id) { i, l in
                if i > 0 { Rectangle().fill(Color.white.opacity(0.14)).frame(width: 0.5, height: 12) }
                CompactLimit(limit: l, level: level, stopline: stoplines?[l.id])
            }
        }
        .fixedSize()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 0 : 9) {
            if compact {
                HStack(spacing: 8) {
                    AccountChip(accounts: accounts, compact: true, busy: busy)
                    if usage.limits.isEmpty {
                        status
                    } else {
                        ViewThatFits(in: .horizontal) { line(0); line(1); line(2) }
                    }
                    Spacer(minLength: 0)
                    if !forceCompact { UsageModeButton(compact: $compactSetting) }   // 중간 화면에서는 바꿔도 보이지 않으므로 숨긴다
                }
            } else {
                HStack(spacing: 8) {
                    AccountChip(accounts: accounts, compact: false, busy: busy)
                    Spacer(minLength: 4)
                    UsageModeButton(compact: $compactSetting)
                }
                if usage.limits.isEmpty { status }
                limitRows
            }
            if let n = accounts.notice {
                Label(n.text, systemImage: n.error ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .boardFont(10.5, weight: .medium)
                    .foregroundStyle(n.error ? Color(red: 1.00, green: 0.58, blue: 0.45) : Color(red: 0.42, green: 0.85, blue: 0.60))
                    .lineLimit(2)
                    .padding(.top, compact ? 6 : 0)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(.horizontal, compact ? 8 : 12)
        .padding(.vertical, compact ? 6 : 12)
        .background(RoundedRectangle(cornerRadius: 14).fill(Color.white.opacity(0.05)))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Color.white.opacity(0.08), lineWidth: 0.5))
        .animation(.spring(response: 0.6, dampingFraction: 0.85), value: usage.limits)
        .animation(.easeOut(duration: 0.2), value: accounts.notice)
    }

    private var limitRows: some View {
        ForEach(usage.limits) { l in
            let c = Self.color(l.pct)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(l.label).boardFont(12, weight: .semibold).lineLimit(1).layoutPriority(1)
                    Spacer(minLength: 4)
                    if let line = stoplines?[l.id] {
                        Text("멈출 선 \(Int(line.rounded()))%").boardFont(10.5).foregroundStyle(.secondary).lineLimit(1).fixedSize()
                    }
                    EverySecond { now in
                        Text(resetText(l.resets, now)).boardFont(10.5).foregroundStyle(.secondary)
                            .monospacedDigit()
                            .lineLimit(1)
                            .minimumScaleFactor(0.8)
                    }
                    Text("\(Int(l.pct.rounded()))%")
                        .boardFont(13, weight: .bold).monospacedDigit()
                        .foregroundStyle(c)
                        .frame(minWidth: 38, alignment: .trailing)
                        .contentTransition(.numericText())
                }
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule().fill(Color.white.opacity(0.08))
                        Capsule()
                            .fill(LinearGradient(colors: [c.opacity(0.65), c], startPoint: .leading, endPoint: .trailing))
                            .frame(width: max(4, geo.size.width * CGFloat(min(l.pct, 100)) / 100))
                            .shadow(color: c.opacity(0.45), radius: 4)
                        if let line = stoplines?[l.id] {   // 자율주행 멈출 선 — 닿으면 지금 단위는 마치고 새 단위는 초기화 뒤에
                            Rectangle().fill(Color.white.opacity(0.85)).frame(width: 1.5, height: 11)
                                .offset(x: geo.size.width * CGFloat(min(line, 100)) / 100 - 0.75)
                        }
                    }
                }
                .frame(height: 6)
            }
            .transition(.opacity)
        }
    }
}

// 간략 모드의 한도 하나 — 이름 · 작은 막대 · 비율. 마우스를 올리면 초기화 시각
struct CompactLimit: View {
    let limit: Limit
    let level: Int          // 0 이름·막대·비율, 1 이름·비율, 2 짧은 이름·비율
    @ObservedObject private var ticker = SecondTicker.shared   // 힌트의 남은 시간
    var stopline: Double? = nil   // 자율주행 보기의 멈출 선(%) — 작은 막대에 눈금
    var body: some View {
        let c = UsagePanel.color(limit.pct)
        HStack(spacing: 4) {
            Text(level == 2 ? limit.tiny : limit.short)
                .boardFont(11, weight: .semibold)
                .foregroundStyle(.secondary)
            if level == 0 {
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.white.opacity(0.10))
                    Capsule().fill(c).frame(width: max(2, 26 * CGFloat(min(limit.pct, 100)) / 100))
                    if let s = stopline {
                        Rectangle().fill(Color.white.opacity(0.85)).frame(width: 1, height: 7).offset(x: 26 * CGFloat(min(s, 100)) / 100 - 0.5)
                    }
                }
                .frame(width: 26, height: 4)
            }
            Text("\(Int(limit.pct.rounded()))%")
                .boardFont(12, weight: .bold).monospacedDigit()
                .foregroundStyle(c)
                .contentTransition(.numericText())
        }
        .hint("\(limit.label) \(Int(limit.pct.rounded()))% · \(resetText(limit.resets, ticker.now))" + (stopline.map { " · 자율주행 멈출 선 \(Int($0.rounded()))%" } ?? ""))
    }
}

// 사용 한도 카드 오른쪽 위의 간략·상세 바꾸기 단추
struct UsageModeButton: View {
    @Binding var compact: Bool
    @State private var hover = false
    var body: some View {
        Button { withAnimation(foldSpring) { compact.toggle() } } label: {
            Image(systemName: compact ? "rectangle.expand.vertical" : "rectangle.compress.vertical")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(hover ? .primary : .secondary)
                .frame(width: 24, height: 20)
                .background(Capsule().fill(Color.white.opacity(hover ? 0.14 : 0.06)))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(compact ? "자세히 보기: 한도마다 막대와 초기화 시각" : "간략히 보기: 한 줄로")
    }
}

// 지금 로그인 계정 — 누르면 저장해 둔 계정으로 전환하거나 계정을 추가하는 메뉴
struct AccountChip: View {
    @ObservedObject var accounts: AccountStore
    var compact = false     // 간략 모드에서는 아이콘만 — 계정은 도움말로 보인다
    let busy: () -> [Session]
    @State private var hover = false

    var body: some View {
        let c = accounts.current
        Button(action: showMenu) {
            HStack(spacing: 5) {
                Image(systemName: accounts.adding ? "person.crop.circle.badge.plus"
                      : accounts.switching ? "arrow.triangle.2.circlepath.circle.fill" : "person.crop.circle.fill")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Palette.accent)
                    .contentTransition(.symbolEffect(.replace))
                if !compact {
                    Text(accounts.adding ? "로그인 기다리는 중…" : c?.email ?? "로그인 정보 없음")
                        .boardFont(11.5, weight: .semibold)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if !accounts.adding, let p = c?.plan, !p.isEmpty {
                        Text(p)
                            .boardFont(9.5, weight: .bold)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .background(Capsule().fill(Color.white.opacity(0.08)))
                            .fixedSize()
                    }
                }
                if accounts.autoSwitch {
                    Image(systemName: "arrow.triangle.swap").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                }
                Image(systemName: "chevron.down").font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            }
            .padding(.leading, compact ? 4 : 5).padding(.trailing, compact ? 6 : 8).padding(.vertical, 3)
            .background(Capsule().fill(Color.white.opacity(hover ? 0.12 : 0.05)))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("로그인 계정 \(c?.email ?? "없음")\(c.map { $0.org.isEmpty ? "" : " · \($0.org)" } ?? "")"
              + (accounts.autoSwitch ? " · 한도 자동 전환 켜짐" : "") + "\n눌러서 다른 계정으로 전환하거나 계정을 추가합니다")
    }

    // 계정 추가는 설정 > 계정 에서 로그인 링크·코드 칸을 보며 한다 — 열고 바로 시작
    private func addInSettings(email: String? = nil) {
        SettingsScreen.shared.open(.account)
        accounts.beginAdd(email: email)
    }

    private func showMenu() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let head = NSMenuItem(title: "Claude 계정: 고르면 모든 세션이 그 계정으로 이어 갑니다", action: nil, keyEquivalent: "")
        head.isEnabled = false
        menu.addItem(head)
        var list = accounts.saved
        if let c = accounts.current, !list.contains(where: { $0.id == c.id }) { list.insert(c, at: 0) }
        for a in list {
            let isCurrent = a.id == accounts.current?.id
            let title = [a.email, a.plan].filter { !$0.isEmpty }.joined(separator: "  ·  ")
                + (!isCurrent && a.isExpired(at: Date()) ? "  (다시 로그인 필요)" : "")
            let item = ActionItem(title, enabled: !accounts.switching) {
                guard !isCurrent else { return }
                a.isExpired(at: Date()) ? addInSettings(email: a.email) : accounts.confirmSwitch(a, busy: busy())
            }
            item.state = isCurrent ? .on : .off
            if !a.org.isEmpty { item.toolTip = a.org }
            menu.addItem(item)
        }
        menu.addItem(.separator())
        if accounts.adding {
            menu.addItem(ActionItem("계정 추가 계속 (설정 > 계정)") { SettingsScreen.shared.open(.account) })
            menu.addItem(ActionItem("계정 추가 취소") { accounts.cancelAdd() })
        } else {
            menu.addItem(ActionItem("계정 추가… (설정 > 계정에서 로그인)") { addInSettings() })
        }
        let removable = accounts.saved.filter { $0.id != accounts.current?.id }
        if !removable.isEmpty {
            let sub = NSMenu()
            for a in removable { sub.addItem(ActionItem(a.email) { accounts.confirmRemove(a) }) }
            let item = NSMenuItem(title: "저장한 계정 지우기", action: nil, keyEquivalent: "")
            item.submenu = sub
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let auto = ActionItem("한도가 차면 다른 계정으로 자동 전환") { accounts.autoSwitch.toggle() }
        auto.state = accounts.autoSwitch ? .on : .off
        auto.toolTip = "5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 멈춘 세션(Ghostty)을 이어서 진행합니다"
        menu.addItem(auto)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

extension AccountStore {
    @MainActor func confirmRemove(_ a: Account) {
        let alert = NSAlert()
        alert.messageText = "\(a.email) 계정을 지울까요?"
        alert.informativeText = "보드에 저장한 이 계정의 로그인 정보를 지웁니다. 다시 쓰려면 계정을 다시 추가해야 합니다."
        alert.addButton(withTitle: "지우기")
        alert.addButton(withTitle: "취소")
        NSApp.activate(ignoringOtherApps: true)
        if alert.runModal() == .alertFirstButtonReturn { remove(a) }
    }

    // 계정 전환(계정 칩 메뉴·설정 > 계정) — 작업 중·승인 대기 세션이 있으면 이전 계정으로 일하는 중이라고 알리고 한 번 더 묻는다
    @MainActor func confirmSwitch(_ a: Account, busy working: [Session]) {
        guard TerminalHub.shared.slide == nil else { return whenPaneSettled { self.confirmSwitch(a, busy: working) } }
        if !working.isEmpty {
            let names = working.prefix(5).map { s in
                "• \((s.cwd as NSString).lastPathComponent) · \(s.topic.isEmpty ? Palette.name(s.state) : s.topic)"
            }
            let alert = NSAlert()
            alert.messageText = "이전 계정으로 작업 중인 세션이 있습니다"
            alert.informativeText = """
            \(current?.email ?? "지금") 계정으로 작업 중이거나 승인을 기다리는 세션이 \(working.count)개 있습니다.
            \(names.joined(separator: "\n"))\(working.count > 5 ? "\n… 외 \(working.count - 5)개" : "")

            전환하면 모든 세션이 다음 요청부터 \(a.email) 계정으로 이어서 동작합니다(30초 안에 반영). 지금 받고 있는 응답은 이전 계정으로 마무리됩니다.

            계정을 전환할까요?
            """
            alert.addButton(withTitle: "전환")
            alert.addButton(withTitle: "취소")
            NSApp.activate(ignoringOtherApps: true)
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        switchTo(a)
    }
}

// 정보 라벨을 HStack 처럼 늘어놓다가 줄 폭을 넘는 라벨부터 다음 줄로 — 세션·서브에이전트 줄의 가장 줄인 모양도 안 들어갈 때.
// 폭을 정하지 않고 물으면(ViewThatFits 가 이상적인 크기를 잴 때) 한 줄 크기
struct WrapRow: Layout {
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(proposal.width, subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (v, p) in zip(subviews, arrange(bounds.width, subviews).points) {
            v.place(at: CGPoint(x: bounds.minX + p.x, y: bounds.minY + p.y), proposal: .unspecified)
        }
    }

    // 라벨마다 왼쪽 위 자리와 전체 크기. 줄 안에서는 HStack 처럼 세로 가운데에 맞춘다
    private func arrange(_ width: CGFloat?, _ subviews: Subviews) -> (points: [CGPoint], size: CGSize) {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        var lines: [Range<Int>] = [], start = 0, x: CGFloat = 0
        for (i, s) in sizes.enumerated() {
            if let width, i > start, x + spacing + s.width > width {
                lines.append(start..<i)
                start = i
            }
            x = i == start ? s.width : x + spacing + s.width
        }
        if !sizes.isEmpty { lines.append(start..<sizes.count) }
        var points = [CGPoint](repeating: .zero, count: sizes.count)
        var y: CGFloat = 0, w: CGFloat = 0
        for line in lines {
            let h = line.map { sizes[$0].height }.max() ?? 0
            var x: CGFloat = 0
            for i in line {
                points[i] = CGPoint(x: x, y: y + (h - sizes[i].height) / 2)
                x += sizes[i].width + spacing
            }
            w = max(w, x - spacing)
            y += h + 3
        }
        return (points, CGSize(width: w, height: max(0, y - 3)))
    }
}

struct SessionRow: View {
    let s: Session
    @ObservedObject private var hub = TerminalHub.shared
    @State private var hover = false
    // 서브에이전트 트리를 접어 둔 세션 id(쉼표로 이음) — 앱을 다시 켜도 유지한다
    @AppStorage("subCollapsed") private var subCollapsed = ""

    private var subOpen: Bool { !subCollapsed.split(separator: ",").contains(Substring(s.id)) }

    private func toggleSubs() {
        var ids = Set(subCollapsed.split(separator: ",").map(String.init))
        if ids.contains(s.id) { ids.remove(s.id) } else { ids.insert(s.id) }
        withAnimation(foldSpring) { subCollapsed = ids.sorted().joined(separator: ",") }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            row
            // 서브에이전트 줄은 한 묶음으로 그린다 — 세션 줄의 회전·반짝임이 초당 20번 바뀔 때마다 화면 갱신이 서브에이전트 줄을 하나하나 다시 훑지 않게
            // (32개일 때 보드 CPU 약 2%p, 2026-10-26 실측). 펼칠 때 줄마다 전환이 돌도록 접기 조건은 묶음 안에 둔다
            VStack(alignment: .leading, spacing: 0) {
                if subOpen {
                    ForEach(Array(s.subagents.enumerated()), id: \.element.id) { i, sub in
                        SubagentRow(sub: sub, last: i == s.subagents.count - 1)
                            .transition(foldIn(i))
                    }
                }
            }
            .compositingGroup()
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: s.subagents)
    }

    private func meta(level: Int, wrap: Bool = false, name: Bool = true) -> some View {
        let compact = level > 0
        return (wrap ? AnyLayout(WrapRow(spacing: 5)) : AnyLayout(HStackLayout(spacing: 5))) {
            TickingText { now in
                s.state == "ended" ? "\(clock(s.updated)) 종료"
                    : s.state == "idle" ? "\(elapsed(s.since, now)) 전" : elapsed(s.since, now)
            }
            .monospacedDigit()
            .fixedSize()
            // 탭 claude 가 넘긴 대화를 이어 받은 작업은 그 탭의 CB 칩(shownPlace)
            PlaceChip(s: s, place: hub.shownPlace(s), appName: hub.appNames[s.id], mode: s.state == "ended" || hub.parkedTab(s) != nil ? nil : hub.mode(s))
            // 세션 이름 — 자리가 모자라면 먼저 뺀다(아래 ViewThatFits). 미니 화면은 세션 줄이 없어 보이지 않는다
            if name, let n = s.name { SessionNameLabel(name: n) }
            // 터미널·백그라운드 구분은 작업경로 카드 안 구역 머리글이 맡는다
            if !s.subagents.isEmpty || s.shells > 0 || s.ctx != nil || s.brain != nil { Text("·") }
            if !s.subagents.isEmpty {
                SubToggle(count: s.subagents.count,
                          running: s.subagents.filter { $0.state != "done" }.count,
                          open: subOpen,
                          toggle: toggleSubs,
                          compact: compact)
            }
            if s.shells > 0 {
                Label(compact ? "\(s.shells)" : "셸 \(s.shells)개 실행 중", systemImage: "apple.terminal.on.rectangle")
                    .labelStyle(.titleAndIcon)
                    .foregroundStyle(Palette.color("working"))
                    .fixedSize()
                    .hint("셸 \(s.shells)개 실행 중")
            }
            if let b = s.brain { ModelLabel(brain: b, level: level) }
            if let ctx = s.ctx { ContextLabel(ctx: ctx, level: level) }
        }
        .boardFont(11)
        .foregroundStyle(.secondary)
    }

    private var row: some View {
        let c = Palette.color(s.state)
        return HStack(alignment: .top, spacing: 10) {
            HStack(spacing: 4) {
                // 서브에이전트가 있는 세션만 접기·펼치기 화살표를 보인다 — 없는 세션도 자리는 비워 줄을 맞춘다
                if s.subagents.isEmpty {
                    Color.clear.frame(width: 14, height: 18)
                } else {
                    FoldChevron(open: subOpen, toggle: toggleSubs)
                }
                StateIcon(state: s.state)
                    .hint(Palette.name(s.state))   // 상태 배지를 뺐으므로 아이콘에 상태 이름을 단다
            }
            .padding(.top, 1)
            VStack(alignment: .leading, spacing: 3) {
                // 첫 줄: 제목 + 오른쪽 단추들. 정보 라벨은 둘째 줄을 가로로 다 쓴다 — 창이 좁아도 넘치지 않게
                HStack(alignment: .center, spacing: 8) {
                    Text(s.title)
                        .boardFont(13, weight: .medium)
                        .foregroundStyle(s.state == "ended" ? .secondary : .primary)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    if s.state != "ended" { MuteToggle(s: s) }
                    // 도는 백그라운드 세션은 메뉴가 없다 — 보드는 그 세션을 멈추거나 끝내지 않는다(일시중지도 터미널이 없어 안 된다)
                    if !(s.state != "ended" && hub.place(s) == .background) { SessionMenu(s: s) }
                    if s.state == "ended" {
                        Button { try? FileManager.default.removeItem(atPath: s.file) } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 14))
                                .foregroundStyle(.secondary.opacity(hover ? 1 : 0.5))
                        }
                        .buttonStyle(.plain)
                        .hint("목록에서 지우기")
                    }
                }
                // 다 들어가면 글자까지, 좁으면 종류 라벨은 아이콘만·하위 작업은 아이콘 + 개수만, 그래도 좁으면 세션 이름을 빼고, 더 좁으면 컨텍스트 막대·토큰을 뺀다.
                // 그래도 안 들어가면(보드 최소 폭·큰 보드 글자) 다음 줄로 넘긴다 — 한 줄을 고집하면 세션 줄이 넓어져 카드가 보드 밖으로 밀린다
                ViewThatFits(in: .horizontal) {
                    meta(level: 0)
                    meta(level: 1)
                    meta(level: 1, name: false)
                    meta(level: 2, name: false)
                    meta(level: 2, wrap: true, name: false)
                }
                .boardFont(11)
                .foregroundStyle(.secondary)
                if s.state == "waiting", !s.message.isEmpty {
                    Label(s.message, systemImage: "hand.raised.fill")
                        .boardFont(11, weight: .medium)
                        .foregroundStyle(c)
                        .lineLimit(2)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hover ? 0.07 : 0)))
        .modifier(Shimmer(active: s.state == "working", color: c))
        .contentShape(Rectangle())
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
        // 한 번 클릭·더블 클릭을 제스처 둘로 나누면 SwiftUI 가 더블 클릭 여부를 기다리느라 줄 안의 스위치·단추까지 0.5초 늦게 반응한다.
        // 제스처 하나로 받고 macOS 클릭 횟수로 나눈다 — 더블 클릭은 첫 클릭(앱 활성화) 뒤에 둘째 클릭에서 포커스·재개
        .onTapGesture { openSession(s, double: (NSApp.currentEvent?.clickCount ?? 1) >= 2) }
        .animation(.spring(response: 0.4, dampingFraction: 0.8), value: s.state)
    }
}

// 세션 줄 오른쪽 위의 … 메뉴 — 일시중지·재개·중단. 누르면 보드 모양의 메뉴 패널(SessionMenuPanel)을 단추 아래에 띄운다
struct SessionMenu: View {
    let s: Session
    @State private var hover = false
    @State private var anchor = MenuAnchor()

    // SwiftUI Menu(borderlessButton) 는 AppKit 팝업 단추라 확대하면 마우스 감지 영역까지 커졌다 줄어 떨리고,
    // 커서도 스스로 화살표로 되돌린다 — 별·X 단추와 같은 일반 버튼으로 두고 누르면 메뉴 패널을 직접 띄운다
    var body: some View {
        Button { SessionMenuPanel.shared.toggle(s, from: anchor.view) } label: {
            Image(systemName: "ellipsis")
                .font(.system(size: 12, weight: .bold))
                .foregroundStyle(.primary.opacity(hover ? 0.95 : 0.55))
                .frame(width: 26, height: 22)
                .background(Capsule().fill(Color.white.opacity(hover ? 0.16 : 0.06)))
                .contentShape(Capsule())
                .scaleEffect(hover ? 1.3 : 1)
                .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hover)
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("세션 일시중지·재개·중단")
        .background(MenuAnchorView(anchor: anchor))
    }
}

// 메뉴 패널을 붙일 자리 — 단추 뒤에 깔린 빈 NSView 를 기억한다(클릭은 받지 않는다)
final class MenuAnchor { weak var view: NSView? }

struct MenuAnchorView: NSViewRepresentable {
    let anchor: MenuAnchor
    final class Spot: NSView { override func hitTest(_ point: NSPoint) -> NSView? { nil } }
    func makeNSView(context: Context) -> NSView {
        let v = Spot()
        anchor.view = v
        return v
    }
    func updateNSView(_ v: NSView, context: Context) { anchor.view = v }
}

// … 메뉴 패널 — 보드와 같은 어두운 둥근 패널. 맨 위에 세션 이름·상태·칩, 그 아래 항목마다 아이콘 + 제목 + 한 줄 설명, 세션 중단은 구분선 아래 빨간색.
// 마우스를 올리거나 ↑↓ 로 고르고 Return·클릭으로 실행, Esc·바깥 클릭·스크롤·다른 앱으로 넘어감·보드 창 이동이면 닫는다.
// 패널은 열린 동안 키 창이 된다(SwiftUI 의 마우스 올림은 키 창에서만 온다). 키는 앱 안 이벤트 감시로 먼저 받고, 닫으면 보드 창이 다시
// 키 창이 되어 터미널 포커스가 돌아온다. 열린 동안 보드는 불투명하게 둔다(WindowOpacity).
// 항목과 조건은 예전 NSMenu 와 같고 동작 함수(resumeSession·resumePaused·pauseSession·moveToBoard·stopSession)를 그대로 부른다
@MainActor final class SessionMenuPanel: ObservableObject {
    static let shared = SessionMenuPanel()
    struct Item: Identifiable {
        let id: Int
        let icon: String
        let title: String
        let detail: String
        var enabled = true
        var destructive = false
        var tint = Palette.color("working")
        let run: @MainActor () -> Void
    }
    // 연 순간의 세션과 그 칩 — 1초마다 다시 읽는 목록과 따로 둔다(canDrive 는 프로세스를 살피므로 열 때 한 번만)
    private(set) var session: Session?
    private(set) var place: SessionPlace = .ended
    private(set) var appName: String?
    private(set) var mode: String?
    private(set) var items: [Item] = []
    @Published var highlighted: Int?
    final class Panel: NSPanel { override var canBecomeKey: Bool { true } }
    private let panel = Panel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
    private var host: NSHostingView<SessionMenuView>?
    private var monitors: [Any] = []
    private var observers: [NSObjectProtocol] = []
    private var closed: (id: String, at: Date)?   // 열린 메뉴의 ⋯ 를 다시 누른 클릭은 닫기만 한다

    private init() {
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .popUpMenu   // 위에 고정한 보드(.floating)보다 위
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.title = "세션 메뉴"
        panel.isReleasedWhenClosed = false
    }

    var isOpen: Bool { panel.isVisible }

    func toggle(_ s: Session, from view: NSView?) {
        if let c = closed, c.id == s.id, Date().timeIntervalSince(c.at) < 0.4 { closed = nil; return }
        if panel.isVisible { close() }
        open(s, from: view)
    }

    private func open(_ s: Session, from view: NSView?) {
        HintPanel.shared.leaveAll()
        let hub = TerminalHub.shared
        session = s
        place = hub.place(s)
        appName = hub.appNames[s.id]
        mode = s.state == "ended" ? nil : hub.mode(s)
        items = Self.items(for: s, place: place)
        highlighted = nil
        let host = self.host ?? FirstClickHostingView(rootView: SessionMenuView(model: self))
        host.rootView = SessionMenuView(model: self)
        if self.host == nil {
            self.host = host
            panel.contentView = host
        }
        let size = host.fittingSize
        // 단추 아래에 오른쪽 끝을 맞춰 붙인다 — 아래 자리가 없으면 단추 위로. 단추를 못 찾으면 마우스 자리
        let anchor: NSRect
        if let v = view, let w = v.window {
            anchor = w.convertToScreen(v.convert(v.bounds, to: nil))
        } else {
            let m = NSEvent.mouseLocation
            anchor = NSRect(x: m.x, y: m.y, width: 0, height: 0)
        }
        let vf = (NSScreen.screens.first { $0.frame.intersects(anchor) } ?? NSScreen.main)?.visibleFrame ?? anchor
        var o = NSPoint(x: anchor.maxX - size.width, y: anchor.minY - 6 - size.height)
        if o.y < vf.minY + 4 { o.y = anchor.maxY + 6 }
        o.x = max(vf.minX + 4, min(o.x, vf.maxX - size.width - 4))
        o.y = max(vf.minY + 4, min(o.y, vf.maxY - size.height - 4))
        panel.setFrame(NSRect(origin: o, size: size), display: true)
        panel.invalidateShadow()
        // 손쉬운 사용 등으로 보드가 앞에 없을 때 눌렸으면 앞으로 — 키(↑↓·Return·Esc)가 보드로 와야 한다
        if !NSApp.isActive { NSApp.activate(ignoringOtherApps: true) }
        panel.makeKeyAndOrderFront(nil)
        watch()
    }

    private func watch() {
        if let m = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel],
                                                    handler: { [weak self] e in self?.handle(e) ?? e }) { monitors.append(m) }
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown],
                                                     handler: { [weak self] _ in MainActor.assumeIsolated { self?.close() } }) { monitors.append(m) }
        let nc = NotificationCenter.default
        for (name, object) in [(NSApplication.didResignActiveNotification, nil), (NSWindow.didMoveNotification, window),
                               (NSWindow.didResizeNotification, window), (NSWindow.didResignKeyNotification, panel)] as [(Notification.Name, Any?)] {
            observers.append(nc.addObserver(forName: name, object: object, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.close() }
            })
        }
    }

    // 앱 안 이벤트 — ↑↓·Return·Esc 는 메뉴가 쓰고, 패널 밖을 누르거나 굴리면 닫고 그 이벤트는 그대로 보낸다
    private func handle(_ e: NSEvent) -> NSEvent? {
        switch e.type {
        case .keyDown:
            if e.modifierFlags.contains(.command) { close(); return e }   // ⌘Q 등은 그대로
            switch e.keyCode {
            case 125: move(1)
            case 126: move(-1)
            case 36, 76, 49: if let h = highlighted { run(h) }   // Return·Enter·Space
            case 53: close()
            default: break
            }
            return nil
        default:
            if e.window !== panel { close() }
            return e
        }
    }

    func move(_ d: Int) {
        let ids = items.filter(\.enabled).map(\.id)
        guard !ids.isEmpty else { return }
        if let h = highlighted, let i = ids.firstIndex(of: h) {
            highlighted = ids[(i + d + ids.count) % ids.count]
        } else {
            highlighted = d > 0 ? ids.first : ids.last
        }
    }

    func hover(_ id: Int, _ inside: Bool) {
        guard items.first(where: { $0.id == id })?.enabled == true else { return }
        if inside { highlighted = id } else if highlighted == id { highlighted = nil }
    }

    // 고른 항목 — 패널을 먼저 닫고 다음 차례에 부른다(중단·옮기기는 확인 창을 띄운다)
    func run(_ id: Int) {
        guard let item = items.first(where: { $0.id == id }), item.enabled else { return }
        close()
        DispatchQueue.main.async { item.run() }
    }

    func close() {
        guard panel.isVisible else { return }
        monitors.forEach(NSEvent.removeMonitor)
        monitors = []
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        panel.orderOut(nil)
        closed = (session?.id ?? "", Date())
        highlighted = nil
        if NSApp.isActive, window.isVisible, !window.isKeyWindow { window.makeKey() }   // 키 창·터미널 포커스를 보드로 돌린다
        WindowOpacity.shared.apply()
    }

    // 항목과 조건 — 끝난 세션: 다시 열기(일시중지였으면 재개), 일시중지: 재개·중단, 그 밖: 일시중지·(Ghostty 세션이면) 보드로 옮기기·중단
    static func items(for s: Session, place: SessionPlace) -> [Item] {
        let red = Color(red: 1.0, green: 0.45, blue: 0.45)
        let stop = Item(id: 9, icon: "stop.fill", title: "세션 중단", detail: "응답을 멈추고 끝냅니다. 대화 기록은 남습니다",
                        destructive: true, tint: red) { stopSession(s) }
        switch s.state {
        case "ended":
            // 보드 새 탭에서 이어 연다 — 백그라운드(claude -p)였던 세션은 -p 없이 대화형으로 열리고 그때부터 보드 탭 세션
            if s.paused {
                return [Item(id: 0, icon: "play.fill", title: "세션 재개 (이어서 진행)", detail: "보드 새 탭에서 열고 멈춘 작업을 이어 갑니다") {
                    resumeSession(s, carryOn: true)
                }]
            }
            return [Item(id: 0, icon: "arrow.clockwise", title: "세션 다시 열기",
                         detail: s.headless ? "보드 새 탭에서 대화형으로 이어 엽니다" : "보드 새 탭에서 이 대화를 이어 엽니다") {
                resumeSession(s, carryOn: false)
            }]
        case "paused":
            return [Item(id: 0, icon: "play.fill", title: "세션 재개", detail: "멈춘 지점부터 이어서 진행하게 합니다") { resumePaused(s) }, stop]
        default:
            let canPause = s.state != "idle" && canDriveSession(s)
            var list = [Item(id: 0, icon: "pause.fill", title: "일시중지",
                             detail: canPause ? "지금 응답을 멈춥니다. 재개하면 이어 갑니다"
                                 : s.state == "idle" ? "대기 중이라 멈출 응답이 없습니다" : "이 터미널에는 키를 넣을 수 없습니다",
                             enabled: canPause, tint: Palette.color("paused")) { pauseSession(s) }]
            // 다른 터미널(Ghostty)의 세션을 보드 탭으로 — 그쪽을 /exit 로 끝내고 보드에서 이어 연다
            if place == .terminal, canDrive(s) {
                list.append(Item(id: 1, icon: "arrow.right.square", title: "보드로 옮기기", detail: "그쪽을 끝내고 보드 새 탭에서 이어 엽니다",
                                 tint: Color(red: 0.66, green: 0.76, blue: 1.0)) { moveToBoard(s) })
            }
            return list + [stop]
        }
    }
}

struct SessionMenuView: View {
    @ObservedObject var model: SessionMenuPanel
    static let width: CGFloat = 300

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let s = model.session { header(s) }
            Rectangle().fill(Color.white.opacity(0.08)).frame(height: 0.5)
            VStack(alignment: .leading, spacing: 2) {
                ForEach(model.items) { item in
                    if item.destructive {
                        Rectangle().fill(Color.white.opacity(0.08)).frame(height: 0.5).padding(.horizontal, 8).padding(.vertical, 4)
                    }
                    row(item)
                }
            }
            .padding(6)
        }
        .frame(width: Self.width)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color(red: 0.11, green: 0.11, blue: 0.17).opacity(0.9))
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
        )
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .preferredColorScheme(.dark)
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
        .accessibilityElement(children: .contain)
        .accessibilityLabel("세션 메뉴")
    }

    // 세션 이름, 그 아래 상태 점·상태 이름·칩·작업 폴더
    private func header(_ s: Session) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(s.title)
                .boardFont(13, weight: .semibold)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 6) {
                Circle().fill(Palette.color(s.state)).frame(width: 7, height: 7)
                Text(Palette.name(s.state)).boardFont(11, weight: .semibold).foregroundStyle(Palette.color(s.state)).fixedSize()
                PlaceChip(s: s, place: model.place, appName: model.appName, mode: model.mode)
                Text((s.cwd as NSString).lastPathComponent).boardFont(11).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 12)
        .padding(.bottom, 10)
        .accessibilityElement(children: .combine)
    }

    private func row(_ item: SessionMenuPanel.Item) -> some View {
        let on = model.highlighted == item.id
        return Button { model.run(item.id) } label: {
            HStack(spacing: 10) {
                Image(systemName: item.icon)
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(item.tint)
                    .frame(width: 26, height: 26)
                    .background(RoundedRectangle(cornerRadius: 7).fill(item.tint.opacity(on ? 0.26 : 0.14)))
                VStack(alignment: .leading, spacing: 1) {
                    Text(item.title).boardFont(13, weight: .semibold).foregroundStyle(item.destructive ? item.tint : .primary)
                    Text(item.detail).boardFont(11).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 9).fill(on ? (item.destructive ? item.tint.opacity(0.16) : Color.white.opacity(0.10)) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!item.enabled)
        .opacity(item.enabled ? 1 : 0.45)
        .onHover { model.hover(item.id, $0) }
        .accessibilityLabel(item.title)
        .accessibilityHint(item.detail)
    }
}

// 누르면 클로저를 실행하는 메뉴 항목
final class ActionItem: NSMenuItem {
    private let run: @MainActor () -> Void
    init(_ title: String, enabled: Bool = true, run: @escaping @MainActor () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
        isEnabled = enabled
    }
    required init(coder: NSCoder) { fatalError() }
    @objc private func fire() { MainActor.assumeIsolated { run() } }   // 메뉴 동작은 메인 스레드에서 온다
}

// 트리 연결선: 위에서 내려와 오른쪽으로 꺾이고, 마지막 줄이 아니면 아래로 계속 이어진다
struct TreeBranch: Shape {
    let last: Bool
    func path(in r: CGRect) -> Path {
        var p = Path()
        let x = r.midX, y = r.midY
        p.move(to: CGPoint(x: x, y: r.minY))
        p.addLine(to: CGPoint(x: x, y: last ? y - 5 : r.maxY))
        if last { p.addQuadCurve(to: CGPoint(x: x + 5, y: y), control: CGPoint(x: x, y: y)) } else {
            p.move(to: CGPoint(x: x, y: y))
        }
        p.addLine(to: CGPoint(x: r.maxX, y: y))
        return p
    }
}

// 세션 줄 왼쪽의 접기·펼치기 화살표 — 그룹 카드 머리의 화살표와 같은 모양
struct FoldChevron: View {
    let open: Bool
    let toggle: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: toggle) {
            Image(systemName: "chevron.right")
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(.primary.opacity(hover ? 1 : 0.65))
                .rotationEffect(.degrees(open ? 90 : 0))
                .frame(width: 14, height: 18)
                .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(hover ? 0.12 : 0)))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(open ? "서브에이전트 목록 접기" : "서브에이전트 목록 펼치기")
    }
}

// 세션 줄의 서브에이전트 트리 접기·펼치기 단추
// 세션별 알림 켜기·끄기 — 끄면 작업 완료·승인 대기 패널과 소리를 모두 띄우지 않는다
struct MuteToggle: View {
    let s: Session
    @Environment(\.notifyLocked) private var locked
    @State private var hover = false
    // 누른 순간의 값 — 표지 파일을 다시 읽어 s.muted 가 따라올 때까지 이 값으로 바로 보인다
    @State private var pending: Bool?

    var body: some View {
        let on = pending ?? !s.muted
        Toggle("", isOn: Binding(get: { on }, set: { v in
            withAnimation(.spring(response: 0.22, dampingFraction: 0.8)) { pending = v }
            setMuted(s, !v)
        }))
            .toggleStyle(BoardSwitch(tint: Palette.color("working"), hover: hover, locked: locked))
            .labelsHidden()
            .disabled(locked)
            .modifier(ClickableIf(active: !locked, hover: $hover))
            .onChange(of: s.muted) { pending = nil }
            .hint(locked ? "전체 알림이 꺼져 있어 바꿀 수 없습니다. 윗줄 종 단추로 전체 알림을 켜면 이 세션의 설정(\(on ? "켜짐" : "꺼짐"))대로 돌아갑니다"
                  : !on ? "알림 꺼짐: 켜면 작업 완료·승인 대기 알림을 다시 띄웁니다" : "알림 켜짐: 끄면 이 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다")
    }
}

// 직접 그린 스위치 — macOS 기본 스위치는 창이 비활성이면 회색으로 바뀌어 켜짐·꺼짐이 안 보인다
struct BoardSwitch: ToggleStyle {
    let tint: Color
    var hover = false
    var locked = false      // 전체 알림 꺼짐 — 회색으로 흐리게, 손잡이는 세션 설정 위치 그대로(다시 켜면 돌아갈 상태)

    func makeBody(configuration: Configuration) -> some View {
        let on = configuration.isOn
        let fill = locked ? Color.white.opacity(0.12) : on ? tint : Color.white.opacity(hover ? 0.22 : 0.14)
        // 탭 제스처 대신 버튼 — 세션 줄의 더블 클릭 판별을 기다리지 않고 누르는 즉시 반응한다
        Button { configuration.isOn.toggle() } label: {
        ZStack(alignment: on ? .trailing : .leading) {
            Capsule()
                .fill(fill)
                .overlay(Capsule().stroke(Color.white.opacity(locked ? 0.35 : on ? 0 : 0.18),
                                          style: StrokeStyle(lineWidth: 0.5, dash: locked ? [2, 2] : [])))
            // 손잡이 안의 종 아이콘으로 알림 스위치임을 알린다 — 켜짐 종, 꺼짐 빗금 종
            Circle()
                .fill(Color.white.opacity(locked ? 0.45 : on ? 1 : 0.75))
                .shadow(color: .black.opacity(locked ? 0 : 0.3), radius: 1, y: 0.5)
                .overlay(
                    Image(systemName: locked || !on ? "bell.slash.fill" : "bell.fill")
                        .font(.system(size: 8, weight: .bold))
                        .foregroundStyle(locked ? Color.black.opacity(0.6) : on ? tint : Color.black.opacity(0.55))
                        .contentTransition(.symbolEffect(.replace))
                )
                .padding(2)
        }
        .frame(width: 36, height: 20)
        .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .animation(.spring(response: 0.22, dampingFraction: 0.8), value: on)
    }
}

// 윗줄 전체 알림 단추 — 꺼지면 붉은 캡슐 "알림 꺼짐"(좁으면 아이콘만)으로 눈에 띄게
struct AllMuteButton: View {
    let muted: Bool
    let toggle: () -> Void
    @State private var hover = false

    var body: some View {
        let c = Color(red: 1.00, green: 0.45, blue: 0.45)
        Button(action: toggle) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 4) {
                    Image(systemName: muted ? "bell.slash.fill" : "bell.fill")
                    if muted { Text("알림 꺼짐").fixedSize() }
                }
                Image(systemName: muted ? "bell.slash.fill" : "bell.fill")
            }
            .boardFont(11, weight: .semibold)
            .foregroundStyle(muted ? c : hover ? .primary : .secondary)
            .contentTransition(.symbolEffect(.replace))
            .padding(.horizontal, 8)
            .frame(minWidth: 28, minHeight: 24)
            .background(Capsule().fill(muted ? c.opacity(0.16) : Color.white.opacity(hover ? 0.12 : 0.07)))
            .overlay(Capsule().stroke(muted ? c.opacity(0.4) : .clear, lineWidth: 0.5))
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint(muted ? "전체 알림 꺼짐: 누르면 다시 켜고, 세션마다 정해 둔 알림 설정으로 돌아갑니다"
              : "전체 알림 끄기: 모든 세션의 작업 완료·승인 대기 알림을 띄우지 않습니다")
    }
}

// 작업경로 카드 안 구역 머리글 — 접기 화살표·종류 아이콘·이름, 상태별 색 점·개수(있는 상태만)와 가는 선. 누르면 구역을 접고 편다
struct KindSection: View {
    let headless: Bool
    let sessions: [Session]
    let collapsed: Bool
    let toggle: () -> Void
    @State private var hover = false
    var body: some View {
        let c = headless ? Color(red: 0.72, green: 0.55, blue: 1.00) : Color(red: 0.32, green: 0.78, blue: 0.86)
        HStack(spacing: 6) {
            Image(systemName: "chevron.right")
                .font(.system(size: 8, weight: .bold))
                .rotationEffect(.degrees(collapsed ? 0 : 90))
                .frame(width: 8)
            Image(systemName: headless ? "moon.stars.fill" : "terminal.fill").font(.system(size: 9, weight: .semibold))
            Text(headless ? "백그라운드" : "터미널").boardFont(10.5, weight: .semibold)
            ForEach(["waiting", "working", "paused", "idle"], id: \.self) { st in
                let n = sessions.filter { $0.state == st }.count
                if n > 0 {
                    HStack(spacing: 3) {
                        Circle().fill(Palette.color(st)).frame(width: 6, height: 6)
                        Text("\(n)").boardFont(10, weight: .semibold).monospacedDigit()
                            .foregroundStyle(.secondary)
                            .contentTransition(.numericText())
                    }
                    .hint("\(Palette.name(st)) \(n)개")
                }
            }
            Rectangle().fill(c.opacity(0.22)).frame(height: 0.5)
        }
        .foregroundStyle(c.opacity(hover ? 1 : 0.85))
        .padding(.horizontal, 10)
        .padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(hover ? 0.05 : 0)))
        .padding(.top, 1)
        .contentShape(Rectangle())
        .clickable($hover)
        .onTapGesture { withAnimation(foldSpring) { toggle() } }
        .hint((headless ? "터미널 없이 도는 세션이라 이동할 터미널이 없습니다" : "터미널에서 도는 세션입니다. 보드 탭 세션은 눌러서 그 탭으로, 다른 터미널 세션은 눌러서 그 앱, 두 번 눌러서 그 세션으로 이동합니다")
              + (collapsed ? " (누르면 펼치기)" : " (누르면 접기)"))
    }
}

struct SubToggle: View {
    let count: Int
    let running: Int
    let open: Bool
    let toggle: () -> Void
    var compact = false     // 줄이 좁으면 아이콘 + 개수만 보인다
    @State private var hover = false

    var body: some View {
        let c = Color(red: 0.95, green: 0.55, blue: 0.75)
        Button(action: toggle) {
            HStack(spacing: 4) {
                Image(systemName: "person.2.fill")
                    .font(.system(size: 9))
                let n = running > 0 && running < count ? "\(running)/\(count)" : "\(count)"
                Text(compact ? n : "하위 작업 \(n)")
                    .monospacedDigit()
                if !compact {
                    Text(open ? "· 접기" : "· 펼치기").foregroundStyle(.primary.opacity(hover ? 0.9 : 0.6))
                }
            }
            .fixedSize()
            .boardFont(10, weight: .semibold)
            .foregroundStyle(c)
            .padding(.horizontal, 6)
            .padding(.vertical, 1.5)
            .background(Capsule().fill(c.opacity(hover ? 0.28 : 0.15)))
            .overlay(Capsule().stroke(c.opacity(0.25), lineWidth: 0.5))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .clickable($hover)
        .hint("하위 작업 \(count)개 · " + (open ? "서브에이전트 목록 접기" : "서브에이전트 목록 펼치기"))
    }
}

// labelStyle 을 조건에 따라 고르기 위한 형식 지우개
struct AnyLabelStyle: LabelStyle {
    private let make: (Configuration) -> AnyView
    init<S: LabelStyle>(_ style: S) { make = { AnyView(style.makeBody(configuration: $0)) } }
    func makeBody(configuration: Configuration) -> some View { make(configuration) }
}

struct SubagentRow: View {
    let sub: Sub
    let last: Bool

    var body: some View {
        let done = sub.state == "done"
        HStack(spacing: 8) {
            TreeBranch(last: last)
                .stroke(Color.white.opacity(0.22), style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
                .frame(width: 16)
            // 도는 서브에이전트는 회전 대신 멈춘 아이콘 — 줄마다 회전 표시를 두면 서브에이전트 수만큼 시계가 돌 때마다 다시 그린다
            if sub.boardState == "working" {
                Image(systemName: "ellipsis.circle.fill")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Palette.color("working"))
                    .frame(width: 18, height: 18)
                    .scaleEffect(0.8)
            } else {
                StateIcon(state: sub.boardState).scaleEffect(0.8)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(sub.desc.isEmpty ? sub.type : sub.desc)
                    .boardFont(12, weight: .medium)
                    .foregroundStyle(done ? .secondary : .primary)
                    .lineLimit(1)
                // 좁으면 라벨을 아이콘만, 더 좁으면 에이전트 종류·도구 이름을 뺀다 — 글자가 잘게 끊기지 않게. 그래도 안 들어가면 다음 줄로
                ViewThatFits(in: .horizontal) {
                    meta(level: 0)
                    meta(level: 1)
                    meta(level: 2)
                    meta(level: 2, wrap: true)
                }
                .boardFont(10.5)
                .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(.leading, 36)          // 부모 줄의 상태 아이콘 아래에 연결선이 오도록
        .padding(.trailing, 10)
        .frame(minHeight: 38)
        .opacity(done ? 0.6 : 1)
        .hint(sub.background ? "백그라운드로 실행된 서브에이전트" : "세션이 결과를 기다리는 서브에이전트")
 
    }

    private func meta(level: Int, wrap: Bool = false) -> some View {
        let done = sub.state == "done"
        return (wrap ? AnyLayout(WrapRow(spacing: 5)) : AnyLayout(HStackLayout(spacing: 5))) {
            KindLabel(subagent: true, compact: level > 0)
            if level < 2, !sub.type.isEmpty { Text(sub.type).fixedSize() }
            if level < 2 { Text("·") }
            TickingText { now in
                done ? "\(elapsed(sub.started, sub.ended ?? now)) 걸려 완료"
                    : sub.state == "waiting" ? "승인 대기" : elapsed(sub.started, now)
            }
            .monospacedDigit()
            .fixedSize()
            if let b = sub.brain { ModelLabel(brain: b, level: level) }
            if !done, !sub.tool.isEmpty {
                Text("·")
                Label(level < 2 ? sub.tool : "", systemImage: "wrench.and.screwdriver.fill")
                    .labelStyle(level < 2 ? AnyLabelStyle(.titleAndIcon) : AnyLabelStyle(.iconOnly))
                    .fixedSize()
                    .hint(sub.tool)
            }
        }
    }
}

// 경로 카드 폴더 모서리 표지 — 펼친 터미널이 지금 이 경로를 보이고 있다(아이콘은 제목 막대의 터미널 단추와 같은 쪽).
// 글자는 붙이지 않는다 — 좁은 보드에서 카드 이름이 잘리고 바로 아래 "터미널" 구역 머리글과 겹쳐 읽힌다. 설명은 폴더 아이콘 힌트에
struct TerminalHereMark: View {
    let boardOnLeft: Bool
    var body: some View {
        Image(systemName: boardOnLeft ? "rectangle.trailinghalf.inset.filled" : "rectangle.leadinghalf.inset.filled")
            .font(.system(size: 8, weight: .bold))
            .foregroundStyle(.white)
            .padding(.horizontal, 4).padding(.vertical, 2.5)
            .background(Capsule().fill(Color(nsColor: terminalBarColor)))
            .overlay(Capsule().stroke(Color.white.opacity(0.7), lineWidth: 1))
            .fixedSize()
    }
}

// 카드 테두리 — 승인 대기가 있으면 테두리와 빛이 천천히 숨 쉰다
struct CardBorder: View {
    let color: Color
    let waiting: Bool
    var body: some View {
        if waiting {
            Ambient { now in
                let k = swing(now, 2.4)
                RoundedRectangle(cornerRadius: 16)
                    .stroke(color.opacity(0.25 + 0.5 * k), lineWidth: 1)
                    .shadow(color: color.opacity(0.35 * k), radius: 12, y: 3)
            }
        } else {
            RoundedRectangle(cornerRadius: 16).stroke(color.opacity(0.22), lineWidth: 1)
        }
    }
}

struct GroupCard: View {
    let g: Group
    let collapsed: Bool
    let toggle: () -> Void
    let endedOpen: Bool
    let toggleEnded: () -> Void
    let pinned: Bool
    let togglePin: () -> Void
    let remove: () -> Void
    let clearEnded: () -> Void
    @ObservedObject private var hub = TerminalHub.shared
    // 이 경로를 쓰는 자율주행 실행 — 실행 줄(기본 보기에서는 숨김)과 원래 작업 트리 표지(늘)
    @ObservedObject private var autopilot = AutopilotStore.shared
    @AppStorage("autopilotView") private var autopilotView = "all"
    @State private var hover = false
    @State private var endedHover = false
    // 접어 둔 구역 "<작업경로>|term" / "<작업경로>|bg" (줄바꿈으로 이음) — 앱을 다시 켜도 유지한다
    @AppStorage("kindCollapsed") private var kindCollapsed = ""

    private func kindFolded(_ kind: String) -> Bool {
        kindCollapsed.split(separator: "\n").contains(Substring("\(g.id)|\(kind)"))
    }

    private func toggleKind(_ kind: String) {
        let key = "\(g.id)|\(kind)"
        var keys = Set(kindCollapsed.split(separator: "\n").map(String.init))
        if keys.contains(key) { keys.remove(key) } else { keys.insert(key) }
        kindCollapsed = keys.sorted().joined(separator: "\n")
    }

    var body: some View {
        let c = Palette.color(g.urgent)
        let here = hub.expanded && hub.workspace == workspaceKey(g.id)   // 펼친 터미널이 이 경로를 보이는 중
        let apRuns = autopilot.runs(at: g.id)
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.secondary.opacity(hover ? 1 : 0.6))
                    .rotationEffect(.degrees(collapsed ? 0 : 90))
                    .frame(width: 10)
                    .hint(collapsed ? "경로 카드 펼치기" : "경로 카드 접기")
                // 접혀 있어도 작업 중(도는 링)·승인 대기(퍼지는 파문)가 보이게 폴더 자리에 상태 표시를 그린다
                ZStack {
                    if collapsed, let top = g.active.first {
                        StateIcon(state: top.state)
                    } else {
                        Image(systemName: "folder.fill")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(c)
                    }
                }
                .frame(width: 30, height: 30)
                .background(RoundedRectangle(cornerRadius: 8).fill(c.opacity(0.16)))
                // 펼친 터미널이 지금 보이는 경로 — 상태 색과 헷갈리지 않게 흰 표지를 폴더 모서리에(카드 이름 폭을 줄이지 않게)
                .overlay(alignment: .bottomTrailing) { if here { TerminalHereMark(boardOnLeft: hub.boardOnLeft).offset(x: 6, y: 5) } }
                .hint((here ? "터미널에서 지금 보는 경로 · " : "")
                      + (collapsed ? g.active.first.map { "\(Palette.name($0.state)) · 누르면 펼치기" } ?? "작업 경로 · 누르면 펼치기"
                         : "작업 경로 · 누르면 접기"))
                VStack(alignment: .leading, spacing: 1) {
                    Text(g.name).boardFont(15, weight: .semibold).lineLimit(1)
                    // 접혀 있으면 경로 대신 지금 하는 일을 상태 색으로 — 여럿이면 나머지 개수를 붙인다
                    if collapsed, let top = g.active.first {
                        HStack(spacing: 4) {
                            Text("\(Palette.name(top.state)) · \(doing(top))").lineLimit(1)
                            if g.active.count > 1 { Text("+\(g.active.count - 1)").fixedSize() }
                        }
                        .boardFont(11, weight: .medium)
                        .foregroundStyle(Palette.color(top.state))
                    } else {
                        Text(g.path).boardFont(11).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                    }
                }
                Spacer()
                // 이 경로에서 바로 새 세션 — 기본 명령으로 보드 탭에서 띄운다
                HeaderButton(icon: "plus", tint: nil, help: "이 경로에서 새 세션 (\(defaultSessionCommand))") {
                    TerminalHub.shared.startSession(defaultSessionCommand, in: g.id)
                }
                HeaderButton(icon: pinned ? "star.fill" : "star", tint: pinned ? Color(red: 1.0, green: 0.78, blue: 0.28) : nil,
                             help: pinned ? "상단 고정 풀기" : "상단에 고정", action: togglePin)
                HeaderButton(icon: "xmark", tint: nil, help: "목록에서 지우기 (새 활동이 생기면 다시 나타납니다)", action: remove)
                HStack(spacing: 6) {
                    ForEach(["waiting", "working", "idle"], id: \.self) { st in
                        let n = g.sessions.filter { $0.state == st }.count
                        if n > 0 {
                            HStack(spacing: 3) {
                                Circle().fill(Palette.color(st)).frame(width: 6, height: 6)
                                Text("\(n)").boardFont(11, weight: .semibold).monospacedDigit()
                                    .contentTransition(.numericText())
                            }
                            .hint("\(Palette.name(st)) \(n)개")
                        }
                    }
                }
                .foregroundStyle(.secondary)
            }
            .padding(6)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hover ? 0.06 : 0)))
            .modifier(Shimmer(active: collapsed && g.urgent == "working", color: c))   // 접힌 채 작업 중이면 세션 줄처럼 빛이 지나간다
            .padding(-6)
            .padding(.bottom, collapsed ? 0 : 2)
            .contentShape(Rectangle())
            .clickable($hover)
            .onTapGesture {
                withAnimation(foldSpring) { toggle() }
                hub.showWorkspace(g.id)   // 그 경로에 보드 탭이 있으면 터미널도 그 경로로(접힌 터미널은 펼치지 않는다)
            }
            .contextMenu {
                Button(pinned ? "상단 고정 풀기" : "상단에 고정", action: togglePin)
                Button("목록에서 지우기", role: .destructive, action: remove)
            }

            // 원래 작업 트리로 자율주행이 도는 경로의 표지 — 기본 보기에서도 남긴다. 좁은 카드에서 이름이 밀리지 않게 머리 아래 줄에 둔다
            if let r = apRuns.first(where: \.inPlace) { APInPlaceRow(run: r) }
            // 자율주행 실행 줄 — 그 경로의 세션 카드 무리 안(와이어프레임 보드 본체 ③). 접힌 카드에서는 한 줄로.
            // 단위 시계(분)는 1초 시계(EverySecond)로 셈한다 — 카드는 매초 다시 그리지 않고 이 줄만 분이 바뀔 때 다시 그린다
            if autopilotView != "basic" {
                ForEach(apRuns) { r in
                    EverySecond { now in APRunLine(run: r, minutes: r.unitMinutes(now), compact: collapsed).equatable() }
                        .transition(.opacity)
                }
            }

            if !collapsed {
                let ended = g.sessions.filter { $0.state == "ended" }
                VStack(alignment: .leading, spacing: 6) {
                    // 터미널 세션과 백그라운드 세션을 구역으로 나눈다 — 백그라운드는 더블 클릭으로 이동할 터미널이 없다.
                    // 세션이 있는 종류마다 머리글을 붙인다 — 한 종류만 있어도 터미널인지 백그라운드인지 보인다
                    let live = g.sessions.filter { $0.state != "ended" }
                    // 보드 탭 안에서 시작된 claude -p 는 탭과 함께 끝나므로 터미널 구역(탭 쪽)에 둔다. 탭 claude 가 넘긴 대화를 이어 받은 작업도 그 탭 쪽
                    let terminal = live.filter { hub.shownPlace($0) != .background }
                    let background = live.filter { hub.shownPlace($0) == .background }
                    if !terminal.isEmpty {
                        KindSection(headless: false, sessions: terminal,
                                    collapsed: kindFolded("term"), toggle: { toggleKind("term") })
                    }
                    if !kindFolded("term") {
                        ForEach(Array(terminal.enumerated()), id: \.element.id) { i, s in
                            SessionRow(s: s)
                                .transition(foldIn(i))
                        }
                    }
                    if !background.isEmpty {
                        KindSection(headless: true, sessions: background,
                                    collapsed: kindFolded("bg"), toggle: { toggleKind("bg") })
                    }
                    if !kindFolded("bg") {
                        ForEach(Array(background.enumerated()), id: \.element.id) { i, s in
                            SessionRow(s: s)
                                .transition(foldIn(terminal.count + i))
                        }
                    }
                    if !ended.isEmpty {
                        // 화살표 크기·칸 폭·간격을 구역 머리글(KindSection)과 같게 — 화살표가 같은 세로줄에 선다
                        HStack(spacing: 6) {
                            Image(systemName: "chevron.right")
                                .font(.system(size: 8, weight: .bold))
                                .rotationEffect(.degrees(endedOpen ? 90 : 0))
                                .frame(width: 8)
                            Text("종료된 세션 \(ended.count)개")
                                .boardFont(11, weight: .medium)
                                .contentTransition(.numericText())
                            Spacer()
                            if endedHover {
                                ClearEndedButton(count: ended.count, action: clearEnded)
                            }
                        }
                        .foregroundStyle(.secondary.opacity(endedHover ? 1 : 0.7))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(endedHover ? 0.06 : 0)))
                        .contentShape(Rectangle())
                        .clickable($endedHover)
                        .hint(endedOpen ? "종료된 세션 접기" : "종료된 세션 펼치기")
                        .onTapGesture { withAnimation(foldSpring) { toggleEnded() } }
                        .transition(.opacity)
                        if endedOpen {
                            VStack(alignment: .leading, spacing: 6) {
                                ForEach(Array(ended.enumerated()), id: \.element.id) { i, s in
                                    SessionRow(s: s)
                                        .transition(foldIn(i))
                                }
                            }
                            .transition(.opacity)
                        }
                    }
                }
                .transition(.opacity.combined(with: .scale(scale: 0.97, anchor: .top)))
            }
        }
        .padding(12)
        .clipShape(RoundedRectangle(cornerRadius: 16))   // 접고 펼치는 동안 줄이 카드 밖으로 비치지 않게
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(LinearGradient(colors: [c.opacity(0.10), Color.white.opacity(0.03)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing))
        )
        .overlay(CardBorder(color: c, waiting: g.urgent == "waiting"))
        .shadow(color: g.urgent == "waiting" ? .clear : .black.opacity(0.25), radius: 8, y: 3)
        .animation(.easeInOut(duration: 0.4), value: g.urgent)
    }
}
