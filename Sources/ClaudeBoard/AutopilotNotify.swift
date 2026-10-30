// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import AutopilotRules
import BoardRules
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI
import WebKit

// MARK: - 자율주행 · 끝 알림

// 4단계 ⑩-3(2026-10-30, 설계 5장 6절 "끝 알림 흐름"): 실행이 끝났다는 알림(실행마다 한 번)을 보드가 화면 오른쪽 위에 작은 떠 있는 판으로 띄운다.
// 누르면 기다리는 올릴 글이 있으면 올릴 글 검수(칸 위 실행 줄의 "합치기·승격 →"으로 잇는다), 없고 합치기가 남았으면 합치기·승격, 둘 다 없으면 지난 실행.
// "나중에" 뒤 하루 지나 다시 오는 합치기 알림도 같은 판으로 한 번 띄우고 합치기·승격을 연다. 판단(띄울 때·여는 곳)은 규칙 AutopilotRules/Notify.swift.
// macOS 알림 센터 대신 보드 안의 판을 쓰는 까닭(설계 판단): 권한 묻기가 없고, 누르면 보드 시트를 바로 열며, 설치판·시험 빌드가 저절로 갈리고, AX 로 시험된다.
// 대화방(webhook) 알림은 엔진이 보내고(보드 창이 꺼져 있어도), 이 판은 보드가 떠 있을 때(꺼져 있던 동안 끝난 실행은 다음에 뜰 때 한 번) 보인다.
// 명령줄 --action autopilot-open|autopilot-merge <실행 id> 도 같은 함수로 연다(떠 있는 보드에 분산 알림으로 넘김)

struct APNoticeItem: Equatable, Identifiable {
    enum Kind: String { case end, merge }
    let run: String
    let kind: Kind
    var title = "", line = "", detail = "", action = ""
    var id: String { kind.rawValue + "-" + run }
}

@MainActor final class APNotices: ObservableObject {
    static let shared = APNotices()
    private let d = UserDefaults.standard
    private let seeded: Double                // 이 판(끝 알림)의 보드가 처음 뜬 때 — 그 앞에 끝난 실행은 띄우지 않는다
    private var notified: Set<String>
    private var reminded: Set<String>
    @Published private(set) var items: [APNoticeItem] = []
    private var panels: [String: NSPanel] = [:]
    private var closeWatch: [String: NSObjectProtocol] = [:]   // 제목 막대의 닫기 단추로 닫아도 목록에서 빼게(남으면 다음 판이 보이지 않는 판 아래 자리를 잡음)

    private init() {
        let s = d.double(forKey: "autopilotEndSeeded")
        if s > 0 { seeded = s } else { seeded = Date().timeIntervalSince1970; d.set(seeded, forKey: "autopilotEndSeeded") }
        notified = Set(d.stringArray(forKey: "autopilotEndNotified") ?? [])
        reminded = Set(d.stringArray(forKey: "autopilotMergeReminded") ?? [])
    }
    var remindedIDs: Set<String> { reminded }

    // 자율주행 읽기(1초)가 부른다 — 끝 정리를 마친 실행의 끝 알림, 나중에 뒤 하루 지난 합치기 알림(규칙 apEndNoticeDue·apMergeReminderDue)
    func check(_ runs: [APRun], later: [String: Double]) {
        let now = Date().timeIntervalSince1970
        var changed = false
        for r in runs where !r.active {
            if apEndNoticeDue(ended: r.ended, wrapped: r.wrapped, notified: notified.contains(r.id), seeded: seeded) {
                notified.insert(r.id); changed = true
                show(item(.end, r))
                boardLog("자율주행 \(r.id): 끝 알림을 띄웠습니다")
            }
            if apMergeReminderDue(later: later[r.id], reminded: reminded.contains(r.id), mergeWaiting: r.mergeWaiting, now: now) {
                reminded.insert(r.id); changed = true
                show(item(.merge, r))
                boardLog("자율주행 \(r.id): 나중에 뒤 하루가 지나 합치기 알림을 띄웠습니다")
            }
        }
        if changed {
            let ids = Set(runs.map(\.id))   // 지운 실행의 표지는 버린다(목록이 끝없이 늘지 않게)
            notified = notified.filter(ids.contains); reminded = reminded.filter(ids.contains)
            d.set(Array(notified).sorted(), forKey: "autopilotEndNotified")
            d.set(Array(reminded).sorted(), forKey: "autopilotMergeReminded")
        }
    }

    private func item(_ k: APNoticeItem.Kind, _ r: APRun) -> APNoticeItem {
        let w = apOutboxWaiting(apRunOutbox(r.dir))
        var it = APNoticeItem(run: r.id, kind: k)
        let failed = r.units.filter { ["failed", "capped"].contains($0.status) }.count
        let result = "\(r.units.count)개 중 \(r.passed)개 통과" + (failed > 0 ? " · 미통과·상한 \(failed)" : "")
        switch k {
        case .end:
            it.title = r.state == "done" ? "자율주행이 끝났습니다" : (r.state == "error" ? "자율주행이 멈췄습니다(엔진 오류)" : "자율주행이 멈췄습니다(중단)")
            it.line = "\(r.name) · \(result)"
            it.detail = [w > 0 ? "검수를 기다리는 올릴 글 \(w)" : nil, r.mergeWaiting ? "합치기 기다림" : nil].compactMap { $0 }.joined(separator: " · ")
            switch apEndOpen(waiting: w, mergeWaiting: r.mergeWaiting) {
            case .review: it.action = "올릴 글 검수 →"
            case .merge: it.action = "합치기·승격 →"
            case .history: it.action = "지난 실행 →"
            }
        case .merge:
            it.title = "합치기를 기다리는 실행이 있습니다"
            it.line = "\(r.name) · \(result)"
            it.detail = "'나중에'를 누른 지 하루가 지났습니다. 바뀐 줄을 보고 합치거나 버립니다"
            it.action = "합치기·승격 →"
        }
        return it
    }

    // 떠 있는 판 — 화면 오른쪽 위에 차례로 쌓는다. 앞으로 꺼내지만 보드를 활성화하지 않는다(치던 글자가 보드로 새지 않게)
    private func show(_ it: APNoticeItem) {
        if let old = panels[it.id] { old.orderOut(nil); panels[it.id] = nil }
        items.removeAll { $0.id == it.id }
        items.append(it)
        let p = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 340, height: 120), styleMask: [.titled, .closable, .nonactivatingPanel, .utilityWindow, .fullSizeContentView],
                        backing: .buffered, defer: false)
        p.title = "자율주행 알림"
        p.titlebarAppearsTransparent = true
        p.isFloatingPanel = true
        p.level = .floating
        p.hidesOnDeactivate = false
        p.becomesKeyOnlyIfNeeded = true
        p.isReleasedWhenClosed = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        let host = NSHostingView(rootView: APNoticeView(item: it))
        host.setFrameSize(host.fittingSize)
        p.contentView = host
        p.setContentSize(NSSize(width: 340, height: max(host.fittingSize.height, 100)))
        panels[it.id] = p
        let id = it.id
        closeWatch[id] = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: p, queue: .main) { _ in
            MainActor.assumeIsolated { APNotices.shared.dismiss(id) }
        }
        place()
        p.orderFrontRegardless()
    }
    private func place() {
        guard let screen = (window?.screen ?? NSScreen.main)?.visibleFrame else { return }
        var y = screen.maxY - 12
        for it in items {
            guard let p = panels[it.id] else { continue }
            let h = p.frame.height
            p.setFrameOrigin(NSPoint(x: screen.maxX - p.frame.width - 12, y: y - h))
            y -= h + 8
        }
    }
    func dismiss(_ id: String) {
        if let w = closeWatch.removeValue(forKey: id) { NotificationCenter.default.removeObserver(w) }
        panels[id]?.orderOut(nil)
        panels[id] = nil
        items.removeAll { $0.id == id }
        place()
    }

    // 끝 알림을 누름 — 여는 곳은 규칙 apEndOpen(기다리는 올릴 글 → 검수, 합치기 → 승격, 그 밖 → 지난 실행)
    func open(_ runId: String) {
        dismiss(APNoticeItem(run: runId, kind: .end).id)
        guard let r = AutopilotStore.shared.anyRun(runId) else { boardLog("자율주행 \(runId): 알림을 눌렀지만 실행 폴더를 찾지 못했습니다"); return }
        front()
        switch apEndOpen(waiting: apOutboxWaiting(apRunOutbox(r.dir)), mergeWaiting: r.mergeWaiting) {
        case .review: APOutboxReview.shared.open(run: runId)
        case .merge: APMerge.shared.open(r.dir)
        case .history: AutopilotStore.shared.screen = .history(runId)
        }
    }
    // 나중에 뒤 다시 온 합치기 알림을 누름 — 합치기·승격
    func openMerge(_ runId: String) {
        dismiss(APNoticeItem(run: runId, kind: .merge).id)
        guard let r = AutopilotStore.shared.anyRun(runId) else { return }
        front()
        APMerge.shared.open(r.dir)
    }
    private func front() {
        if NSApp.isHidden { NSApp.unhideWithoutActivation() }
        if window?.isMiniaturized == true { window?.deminiaturize(nil) }
        window?.orderFrontRegardless()
    }
}

// 실행 id 꼴(엔진과 같은 기준: 영문·숫자·.-_, 점으로 시작하지 않음) — 명령줄 --action 으로 받은 id 를 경로에 붙이기 전에
func apRunIDOK(_ s: String) -> Bool {
    !s.isEmpty && s.count <= 120 && !s.hasPrefix(".") && s.unicodeScalars.allSatisfy { $0.isASCII && (CharacterSet.alphanumerics.contains($0) || "._-".unicodeScalars.contains($0)) }
}

// 실행 폴더의 올릴 글(엔진이 쓴 outbox/<id>.json, 보통 파일·상한 아래일 때만) — 끝 알림이 무엇을 열지 볼 때 그 자리에서 읽는다
func apRunOutbox(_ dir: String) -> [APOutboxItem] {
    let box = dir + "/outbox"
    return ((try? FileManager.default.contentsOfDirectory(atPath: box)) ?? []).compactMap { n -> APOutboxItem? in
        guard n.hasSuffix(".json"), apOutboxIDOK(String(n.dropLast(5))), let d = APFolderReportSource.plainFile(box + "/" + n) else { return nil }
        return apOutboxParse(d)
    }
}

struct APNoticeView: View {
    let item: APNoticeItem
    @State private var hover = false
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: item.kind == .end ? "steeringwheel" : "arrow.triangle.merge").font(.system(size: 13, weight: .bold)).foregroundStyle(Palette.accent)
                Text(item.title).font(.system(size: 13, weight: .bold)).lineLimit(1)
                Spacer(minLength: 4)
                Button { APNotices.shared.dismiss(item.id) } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("닫기(이 실행의 끝 알림은 다시 오지 않습니다. 보드의 끝난 실행 줄은 남습니다)")
                    .accessibilityLabel("닫기").accessibilityIdentifier("ap-notice-close-\(item.id)")
            }
            Text(item.line).font(.system(size: 12)).lineLimit(2).accessibilityIdentifier("ap-notice-line-\(item.id)")
            if !item.detail.isEmpty { Text(item.detail).font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
            HStack {
                Spacer(minLength: 0)
                Button {
                    if item.kind == .end { APNotices.shared.open(item.run) } else { APNotices.shared.openMerge(item.run) }
                } label: {
                    Text(item.action).font(.system(size: 12, weight: .semibold)).padding(.horizontal, 10).padding(.vertical, 4)
                        .background(RoundedRectangle(cornerRadius: 7).fill(Palette.accent.opacity(hover ? 0.32 : 0.22)))
                }
                .buttonStyle(.plain)
                .onHover { hover = $0 }
                .accessibilityIdentifier("ap-notice-open-\(item.id)")
            }
        }
        .padding(.horizontal, 14).padding(.top, 26).padding(.bottom, 12)
        .frame(width: 340, alignment: .leading)
        .contentShape(Rectangle())
        .onTapGesture { if item.kind == .end { APNotices.shared.open(item.run) } else { APNotices.shared.openMerge(item.run) } }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ap-notice-\(item.id)")
    }
}
