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

// MARK: - 사용 한도 자동 전환

// 지금 계정의 5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 한도로 멈췄던 세션에 이어서 진행하라는 요청을 넣는다.
// 계정 메뉴에서 켜고 끄며 기본은 꺼짐. 다른 계정의 남은 한도는 미리 알 수 없어(저장한 토큰은 그 계정으로 전환해야 갱신된다)
// 그 계정이 지금 계정이었을 때 한도가 찼던 기록으로 고른다. 결정마다 ~/.claude/notifier/auto-switch.log 에 한 줄씩 남긴다.
// 판단(한도에 걸린 세션·전환할지와 까닭·다음 계정과 한도 표시·이어 갈 세션)은 BoardRules(AutoSwitch.swift)의 decideAutoSwitch 가,
// 전환 실패 뒤의 표시와 쉼은 autoSwitchFailed 가 하고,
// 여기는 3초 타이머·UserDefaults·로그·안내·전환 요청·터미널 조작(driveSession)을 쥔다
final class AutoSwitcher {
    static let prompt = "사용 한도로 멈췄던 세션을 다른 계정으로 전환해 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요."
    private let store: Store
    private let usage: UsageStore
    private let accounts: AccountStore
    private var timer: Timer?
    // 마지막으로 계정을 바꾼 시각(손으로 바꾼 것 포함) — 보드를 다시 켜도 이어지게 저장한다
    private var lastSwitch = Date(timeIntervalSince1970: UserDefaults.standard.double(forKey: "autoSwitchLast")) {
        didSet { UserDefaults.standard.set(lastSwitch.timeIntervalSince1970, forKey: "autoSwitchLast") }
    }
    private var pending = false              // 자동 전환을 요청하고 결과를 기다리는 중
    // 이어 가기를 넣은 "세션 id@한도 시각" — 한 번만 넣는다. 메모리에만 두는 것은 의도다: 요청이 들어가면 state.py 가 UserPromptSubmit 에서
    // limit_hit 을 지워 그 세션은 다시 고르지 않으므로, 보드를 다시 켠 뒤 다시 넣는 것은 넣기에 실패해 limit_hit 이 남은 세션뿐이고 그때는 다시 해 보는 편이 낫다
    private var resumed = Set<String>()
    private var noCandidate = false          // 전환할 계정이 없다고 이미 알렸다 — 계정이 바뀌기 전에는 다시 알리지 않는다
    private var unknownCurrent = false       // 지금 계정을 몰라 전환하지 못했다고 이미 알렸다 — 지금 계정이 알려지거나 계정이 바뀌기 전에는 다시 알리지 않는다
    // 지금 계정·시스템 쪽 전환 실패로 자동 전환을 쉬는 중 — 지금 계정이 바뀌거나 1시간이 지날 때까지(판단은 decideAutoSwitch 가 본다).
    // 메모리에만 두는 것은 의도다: 보드를 다시 켜면 한 번 다시 해 보고, 같은 까닭이면 다시 한 번 알리고 쉰다(/login 등으로 고친 뒤에는 그쪽이 낫다)
    private var pause: AutoSwitchPause?
    // 한도가 찬 계정 id 와 풀리는 시각 — 그 전에는 고르지 않는다
    private var full = UserDefaults.standard.dictionary(forKey: "autoSwitchFull") as? [String: Double] ?? [:] {
        didSet { UserDefaults.standard.set(full, forKey: "autoSwitchFull") }
    }
    private let logPath = NSHomeDirectory() + "/.claude/notifier/auto-switch.log"

    init(store: Store, usage: UsageStore, accounts: AccountStore) {
        self.store = store
        self.usage = usage
        self.accounts = accounts
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.tick() }
    }

    // 계정이 바뀌었다 — 이때부터 settle 동안 난 한도 오류는 이전 계정 토큰으로 나간 요청이다
    func switched() {
        lastSwitch = Date()
        noCandidate = false
        unknownCurrent = false
        pause = nil
    }

    private func tick() {
        guard accounts.autoSwitch else { return }
        if accounts.current != nil { unknownCurrent = false }   // 지금 계정이 알려졌다 — 다시 모르게 되면 다시 알린다
        let now = Date()
        let d = decideAutoSwitch(sessions: store.groups.flatMap(\.sessions), limits: usage.limits,
                                 current: accounts.current, saved: accounts.saved, full: full, resumed: resumed,
                                 lastSwitch: lastSwitch, pending: pending, switching: accounts.switching, pause: pause, now: now)
        // 1) 전환 — 지금 계정은 한도가 풀릴 때까지 고르지 않게 적어 두고(후보가 없어도), 저장한 계정 가운데 지금 계정 다음부터 쓸 수 있는 계정으로 바꾼다
        if let marks = d.full { full = marks }
        switch d.move {
        case .stay:
            break
        case .unknownCurrent(let reason):
            if !unknownCurrent {
                unknownCurrent = true
                log("\(reason): 지금 계정을 몰라 전환하지 못함")
                accounts.flash("사용 한도에 도달했지만 지금 로그인된 계정의 정보를 읽지 못해 자동 전환을 하지 못했습니다. Claude Code 에서 /login 으로 다시 로그인해 주세요", error: true)
            }
        case .noCandidate(let reason):
            if !noCandidate {
                noCandidate = true
                log("\(reason): 전환할 계정 없음")
                accounts.flash("사용 한도에 도달했지만 전환할 다른 계정이 없습니다. 저장한 계정이 모두 한도에 도달했거나 다시 로그인이 필요합니다", error: true)
            }
        case .switchTo(let next, let cur, let reason, let stopped):
            pending = true
            log("\(reason): \(cur.email) 에서 \(next.email) 로 전환")
            let note = "사용 한도에 도달해 \(next.email) 계정으로 자동 전환했습니다"
                + (stopped ? ". 한도로 멈춘 세션은 \(Int(AutoSwitchRule.settle))초 뒤 이어서 진행합니다" : "")
            // 실패하면 까닭의 쪽으로 가른다(autoSwitchFailed) — 대상 계정 쪽이면 그 계정만 1시간 고르지 않고 다음 판단이 다음 후보로,
            // 그 밖(지금 계정·시스템 쪽)이면 다른 계정으로 해도 같으므로 후보를 쓰지 않고 쉬며 한 번만 알린다(switchTo 의 실패 안내를 바꿔 띄움)
            accounts.switchTo(next, note: note) { [weak self] r in
                guard let self else { return }
                self.pending = false
                guard case .failed(let e) = r else { return }
                let side = switchFailureSide(e)
                let after = autoSwitchFailed(side, target: next, from: cur, full: self.full, now: now)
                self.full = after.full
                self.pause = after.pause
                switch side {
                case .target:
                    self.log("\(next.email) 로 전환하지 못함 (1시간 동안 고르지 않음): \(e.localizedDescription)")
                case .busy:
                    // 쓰기 잠금은 곧 풀린다 — 알리지 않고 잠깐 쉰 뒤 같은 후보로 다시 해 본다
                    self.log("\(next.email) 로 전환하지 못함, \(Int(AutoSwitchRule.busyRetry))초 뒤 다시: \(e.localizedDescription)")
                case .other:
                    self.log("\(next.email) 로 전환하지 못함, 지금 계정이 바뀌거나 1시간이 지날 때까지 자동 전환을 쉼: \(e.localizedDescription)")
                    self.accounts.flash("사용 한도에 도달해 \(next.email) 계정으로 자동 전환하려 했지만 하지 못했습니다: \(e.localizedDescription). "
                                        + "계정이 바뀌거나 1시간이 지날 때까지 자동 전환을 쉽니다", error: true)
                }
            }
        }
        // 2) 이어 가기 — 전환하고 settle 이 지났으면, 그 전(이전 계정)에 한도로 멈춘 세션에 이어서 진행하라는 요청을 넣는다
        guard !d.resume.isEmpty else { return }
        var manual: [String] = []
        for r in d.resume {
            resumed.insert(r.key)
            let s = r.session
            let name = (s.cwd as NSString).lastPathComponent
            // 1초 타이머(메인 스레드)에서 부른다
            guard MainActor.assumeIsolated({ canDriveSession(s) }) else {
                manual.append(name)
                log("\(name): 보드 탭·Ghostty 세션이 아니라 이어 가지 못함")
                continue
            }
            // Esc 를 먼저 — Claude Code 가 띄운 한도 선택 창을 닫고 "한도가 풀리면 저절로 이어 가기" 예약을 취소한다.
            // 선택 창이 열린 채 Enter 를 보내면 그 창의 항목이 골라진다
            MainActor.assumeIsolated {
                driveSession(s, [("key", "escape"), ("wait", "0.5"), ("text", Self.prompt), ("wait", "0.2"), ("key", "enter")]) { [weak self] ok in
                    self?.log("\(name): \(ok ? "이어 가기 요청을 넣음" : "터미널을 찾지 못해 이어 가지 못함")")
                }
            }
        }
        let n = d.resume.count - manual.count
        accounts.flash((n > 0 ? "한도로 멈췄던 세션 \(n)개를 이어서 진행합니다" : "")
                       + (manual.isEmpty ? "" : (n > 0 ? ". " : "") + "\(manual.joined(separator: ", ")) 세션은 보드 탭·Ghostty 밖이라 직접 이어 주세요"),
                       error: !manual.isEmpty)
    }

    private func log(_ text: String) {
        let line = "\(Date()) \(text)\n"
        if let h = FileHandle(forWritingAtPath: logPath) {
            h.seekToEndOfFile()
            h.write(Data(line.utf8))
            try? h.close()
        } else {
            FileManager.default.createFile(atPath: logPath, contents: Data(line.utf8))
        }
    }
}

// 앱 아이콘(AppIcon.icns) — 메뉴 막대와 보드 제목 옆에도 같은 그림을 쓴다. 크기별 그림 중 화면 배율에 맞는 것이 골라진다
let appIcon = NSImage(named: "AppIcon") ?? NSImage()

enum Palette {
    static func color(_ state: String) -> Color {
        switch state {
        case "working": return Color(red: 0.45, green: 0.66, blue: 1.00)
        case "waiting": return Color(red: 1.00, green: 0.66, blue: 0.36)
        case "idle": return Color(red: 0.70, green: 0.75, blue: 0.86)   // 유휴 — 완료로 읽히지 않게 초록 대신 차분한 회청색
        case "paused": return Color(red: 0.95, green: 0.80, blue: 0.35)
        default: return Color(white: 0.55)
        }
    }

    static func name(_ state: String) -> String {
        ["working": "작업 중", "waiting": "승인 대기", "idle": "대기", "paused": "일시중지", "ended": "종료"][state] ?? state
    }

    static let accent = Color(red: 0.85, green: 0.47, blue: 0.34)
}

final class Store: ObservableObject {
    @Published var groups: [Group] = []
    // 정렬·접기 판단 같은 계산에만 쓴다 — 화면의 경과 시간은 SecondTicker 를 본다(publish 하면 매초 보드 전체를 다시 계산한다)
    private(set) var now = Date()
    // 접힌 그룹의 경로(Group.id) — 앱을 다시 켜도 유지한다
    @Published var collapsed = Set(UserDefaults.standard.stringArray(forKey: "collapsed") ?? [])
    // 전체 알림 끄기 — ~/.claude/notifier/mute-all 이 있으면 notify.py 가 모든 세션의 알림을 띄우지 않는다
    @Published var allMuted = FileManager.default.fileExists(atPath: muteAllPath)
    private var timer: Timer?
    // 세션 상태 판정(BoardRules.scanSessions)에 넘기는 진짜 어댑터 — Claude Code 상태는 ClaudeNames.shared
    private let sessionFiles = SessionFiles()
    private let processes = SystemProcesses()
    private let transcripts = TranscriptFiles()
    private let contexts = ContextReader()
    private let clock = SystemClock()
    // 백그라운드 셸 수 — ps 를 메인 스레드에서 돌리면 클릭 처리가 밀리므로 뒤에서 세고 다음 reload 가 쓴다(processes 가 들고 있다)
    private var shellsBusy = false
    private var shellsAt = Date.distantPast

    // ps 는 프로세스 수백 개를 훑으므로 3초에 한 번만 센다
    private func refreshShells() {
        guard !shellsBusy, now.timeIntervalSince(shellsAt) >= 3 else { return }
        shellsBusy = true
        shellsAt = now
        DispatchQueue.global(qos: .utility).async {
            let counts = backgroundShells()
            DispatchQueue.main.async { [weak self] in
                self?.processes.shellCounts = counts
                self?.shellsBusy = false
            }
        }
    }

    init() {
        reload()
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.reload() }
        // 세션 줄에서 표지 파일을 바꾸면 1초 주기를 기다리지 않고 바로 다시 읽는다
        NotificationCenter.default.addObserver(forName: .boardReload, object: nil, queue: .main) { [weak self] _ in
            self?.reload(force: true)
        }
    }

    func count(_ state: String) -> Int { groups.reduce(0) { $0 + $1.sessions.filter { $0.state == state }.count } }

    // 직전 reload 때 승인 대기였던 세션 — 새로 승인 대기가 된 세션만 골라 그 그룹을 펼친다
    private var waitingIds: Set<String> = []

    func toggle(_ cwd: String) {
        if collapsed.contains(cwd) { collapsed.remove(cwd) } else { collapsed.insert(cwd) }
        UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
    }

    // 종료 세션 목록을 펼쳐 둔 그룹의 경로(Group.id) — 기본은 접힘
    @Published var endedOpen = Set(UserDefaults.standard.stringArray(forKey: "endedOpen") ?? [])

    func toggleEnded(_ cwd: String) {
        if endedOpen.contains(cwd) { endedOpen.remove(cwd) } else { endedOpen.insert(cwd) }
        UserDefaults.standard.set(Array(endedOpen), forKey: "endedOpen")
    }

    // 상단에 고정한 작업경로 — 고정한 순서대로 맨 위에 둔다
    @Published var pinnedGroups = UserDefaults.standard.stringArray(forKey: "pinnedGroups") ?? []

    func togglePinGroup(_ cwd: String) {
        if let i = pinnedGroups.firstIndex(of: cwd) { pinnedGroups.remove(at: i) } else { pinnedGroups.append(cwd) }
        UserDefaults.standard.set(pinnedGroups, forKey: "pinnedGroups")
        reload(force: true)
    }

    // 목록에서 지운 작업경로와 지운 시각 — 그 뒤에 새 활동(세션 기록 갱신)이 생기면 다시 나타난다
    private var hiddenGroups = UserDefaults.standard.dictionary(forKey: "hiddenGroups") as? [String: Double] ?? [:]

    func hideGroup(_ g: Group) {
        // 종료된 세션 기록은 함께 지운다
        for s in g.sessions where s.state == "ended" { try? FileManager.default.removeItem(atPath: s.file) }
        hiddenGroups[g.id] = Date().timeIntervalSince1970
        UserDefaults.standard.set(hiddenGroups, forKey: "hiddenGroups")
        pinnedGroups.removeAll { $0 == g.id }
        UserDefaults.standard.set(pinnedGroups, forKey: "pinnedGroups")
        reload(force: true)
    }

    // 그 작업경로의 종료된 세션 기록을 한 번에 지운다 — 대화 기록(jsonl)은 남는다
    func removeEnded(_ g: Group) {
        for s in g.sessions where s.state == "ended" { try? FileManager.default.removeItem(atPath: s.file) }
        reload(force: true)
    }

    // 자동으로 접은 작업경로와 그때의 마지막 활동 시각 — 사람이 다시 펼치면 새 활동이 생기기 전까지 또 접지 않는다
    private var autoFolded = UserDefaults.standard.dictionary(forKey: "autoFolded") as? [String: Double] ?? [:]
    static let quietFoldAfter: TimeInterval = 30 * 60

    // 승인 대기·작업 중 세션이 없고 마지막 활동이 30분 넘게 지난 카드를 한 번 접는다
    private func foldQuiet(_ groups: [Group]) {
        var changed = false
        for g in groups {
            let last = (g.sessions.map(\.updated).max() ?? .distantPast).timeIntervalSince1970
            let quiet = !g.sessions.contains { $0.state == "waiting" || $0.state == "working" }
            guard quiet, now.timeIntervalSince1970 - last > Store.quietFoldAfter else { continue }
            if autoFolded[g.id] == last { continue }
            autoFolded[g.id] = last
            changed = true
            if !collapsed.contains(g.id) {
                withAnimation(foldSpring) { _ = collapsed.insert(g.id) }
                UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
            }
        }
        let ids = Set(groups.map(\.id))
        if autoFolded.keys.contains(where: { !ids.contains($0) }) {
            autoFolded = autoFolded.filter { ids.contains($0.key) }
            changed = true
        }
        if changed { UserDefaults.standard.set(autoFolded, forKey: "autoFolded") }
    }

    // 요약 칩으로 찾아갈 때: 카드와 그 안의 터미널·백그라운드 구역을 펼친다
    func reveal(_ cwd: String) {
        if collapsed.contains(cwd) {
            collapsed.remove(cwd)
            UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
        }
        let d = UserDefaults.standard
        let keys = (d.string(forKey: "kindCollapsed") ?? "").split(separator: "\n").map(String.init)
        let kept = keys.filter { !$0.hasPrefix(cwd + "|") }
        if kept.count != keys.count { d.set(kept.joined(separator: "\n"), forKey: "kindCollapsed") }
    }

    func setAll(collapse: Bool) {
        collapsed = collapse ? Set(groups.map(\.id)) : []
        UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
    }

    private func expandNewlyWaiting(_ sessions: [Session]) {
        let waiting = sessions.filter { $0.state == "waiting" }
        let opened = Set(waiting.filter { !waitingIds.contains($0.id) }.map(\.projectDir)).intersection(collapsed)
        waitingIds = Set(waiting.map(\.id))
        guard !opened.isEmpty else { return }
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { collapsed.subtract(opened) }
        UserDefaults.standard.set(Array(collapsed), forKey: "collapsed")
    }

    // 세션별 .mute 표지는 건드리지 않으므로 다시 켜면 세션마다 정해 둔 상태가 그대로 돌아온다
    func setAllMuted(_ on: Bool) {
        if on {
            FileManager.default.createFile(atPath: muteAllPath, contents: Data())
        } else {
            try? FileManager.default.removeItem(atPath: muteAllPath)
        }
        withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { allMuted = on }
    }

    func reload(force: Bool = false) {
        // 세션 상태 판정은 BoardRules.scanSessions 가 포트로 한다 — 시계는 거기서 한 번만 읽고 여기서도 그 시각을 쓴다.
        // 셸 수는 직전까지 뒤에서 센 것을 쓰고(refreshShells 의 결과는 이 reload 가 끝난 뒤에 들어온다), 낡은 세션 기록 지우기·reader 캐시 정리도 거기서 한다
        let scan = scanSessions(records: sessionFiles, processes: processes, live: ClaudeNames.shared,
                                transcripts: transcripts, contexts: contexts, clock: clock)
        now = scan.now
        SecondTicker.shared.now = now
        refreshShells()
        let sessions = scan.sessions
        // 세션을 띄운 앱 기억도 판정의 캐시 정리(contexts.prune)처럼 목록에 남은 세션 몫만 남긴다
        terminalControl.prune(keep: Set(sessions.map(\.id)))
        // 탭 짝짓기는 넘긴 세션까지 모두 본다 — 보드 탭에서 넘긴 원래 claude 가 그 탭에 남아 있을 수 있고(탭 줄 이름·쥔 목록),
        // 카드 목록에서만 뺀다
        // 1초 타이머·알림 관찰자 모두 메인 스레드. 탭 claude 가 넘긴 작업을 붙여 보는 중이면 탭은 그 작업 세션을 따라간다(parkedFrom = 넘긴 쪽)
        let parkedFrom = MainActor.assumeIsolated { TerminalHub.shared.pair(sessions); return TerminalHub.shared.parkedFrom }
        // 넘긴 세션은 이어 받은 세션의 카드가 있을 때만 뺀다 — 그 세션 파일이 없으면(아직 hook 이 안 왔거나 지워짐) 지금처럼 보인다.
        // 탭이 이어 받은 작업을 따라가는 동안의 탭 claude 세션도 뺀다(그 탭의 카드는 작업 쪽 하나).
        // groups 가 이 목록으로 만들어지므로 상태별 개수·메뉴 막대·감독 말·종료 확인도 함께 빠진다
        let shown = visibleSessions(sessions, handedTo: scan.handedTo, parkedFrom: parkedFrom)
        expandNewlyWaiting(shown)
        let muted = sessionFiles.allMuted()
        if muted != allMuted { allMuted = muted }
        let grouped = groupSessions(shown)
        // 지운 뒤 새 활동이 없는 작업경로는 빼고, 새 활동이 생긴 작업경로는 지운 목록에서 꺼낸다
        var visible: [Group] = []
        for g in grouped {
            if let at = hiddenGroups[g.id] {
                if g.sessions.allSatisfy({ $0.updated.timeIntervalSince1970 <= at }) { continue }
                hiddenGroups[g.id] = nil
                UserDefaults.standard.set(hiddenGroups, forKey: "hiddenGroups")
            }
            visible.append(g)
        }
        foldQuiet(visible)
        let sorted = sortGroups(visible, pinned: pinnedGroups)
        AmbientTicker.shared.busy = sorted.contains { $0.sessions.contains { $0.state == "working" || $0.state == "waiting" } }
        if force || sorted != groups {
            withAnimation(.spring(response: 0.45, dampingFraction: 0.82)) { groups = sorted }
        }
    }
}

// claude 프로세스별 백그라운드 셸 수 — Claude Code 는 Bash 도구를 shell-snapshots 를 읽는 zsh/bash 자식으로 띄운다.
// 응답이 끝난 뒤에도 남아 있는 이 자식은 run_in_background 로 띄운 셸이다
func backgroundShells() -> [Int32: Int] {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/bin/ps")
    p.arguments = ["-axo", "ppid=,command="]
    let pipe = Pipe()
    p.standardOutput = pipe
    p.standardError = FileHandle.nullDevice
    guard (try? p.run()) != nil else { return [:] }
    let out = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    p.waitUntilExit()
    var counts: [Int32: Int] = [:]
    for line in out.split(separator: "\n") where line.contains("/.claude/shell-snapshots/snapshot-") {
        let t = line.trimmingCharacters(in: .whitespaces)
        if let ppid = Int32(t.prefix { $0.isNumber }) { counts[ppid, default: 0] += 1 }
    }
    return counts
}

func formatter(_ format: String) -> DateFormatter {
    let f = DateFormatter()
    f.locale = Locale(identifier: "ko_KR")
    f.dateFormat = format
    return f
}
let hourMinute = formatter("HH:mm"), dayHourMinute = formatter("M/d(E) HH:mm")

// 종료된 세션은 흐르는 시간 대신 끝난 시각을 고정해서 보여 준다
func clock(_ date: Date) -> String { hourMinute.string(from: date) }

// 경과 시간 글자(elapsed)는 BoardRules 에 있다

// 매초 바뀌는 글자(경과 시간·남은 시간·감독 말)만 보는 1초 시계 — 줄·카드에 시각을 넘기면 매초 모든 줄의 입력이 바뀐다.
// 시계를 따로 돌리지 않고 Store.reload 가 함께 바꾼다(한 번에 다시 그리게)
final class SecondTicker: ObservableObject {
    static let shared = SecondTicker()
    @Published fileprivate(set) var now = Date()
}
struct EverySecond<Content: View>: View {
    @ObservedObject private var ticker = SecondTicker.shared
    @ViewBuilder let content: (Date) -> Content
    var body: some View { content(ticker.now) }
}

// 줄마다 매초 바뀌는 경과 시간 글자 — 자리는 숫자를 모두 0 으로 바꾼 글자가 잡고(숫자 폭이 같아 크기가 같다) 실제 글자는 그 위에 겹쳐 그린다.
// 글자가 바뀔 때마다 크기를 다시 재면 줄마다 ViewThatFits 가 후보를 모두 다시 재어 서브에이전트 수만큼 CPU 가 늘었다(32개일 때 보드 CPU 30% → 22%, 2026-10-26 실측).
// 이렇게 두면 자릿수가 바뀔 때만 다시 잰다.
// monospacedDigit·fixedSize 는 바깥에서 붙인다
struct TickingText: View {
    let text: (Date) -> String
    var body: some View {
        EverySecond { now in
            let s = text(now)
            Text(String(s.map { $0.isWholeNumber ? "0" : $0 })).hidden()
                .overlay(alignment: .leading) { Text(s).fixedSize().contentTransition(.numericText()) }
        }
    }
}
