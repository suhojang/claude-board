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

// MARK: - 업데이트

// GitHub 최신 릴리스의 VERSION 과 이 앱 버전을 비교해 새 버전이 있으면 보드 위에 알린다.
// 업데이트 단추는 설치와 같은 install.sh 를 curl 로 받아 실행한다 — 스크립트가 앱을 바꿔 끼우고 보드를 다시 띄운다.
// 판단(본문 검증·새 버전인지·확인하는 빌드인지·확인 결과와 안내)은 BoardRules/Updates.swift, 포트는 ReleaseVersions(가짜 어댑터는 Tests 에만).
// 여기는 어댑터와 타이머·설정 읽기·알림 줄·애니메이션·업데이트 실행이다
let releaseRepo = "suhojang/claude-board"   // packaging/install.sh·release.sh 의 REPO 와 같게
let releaseBase = "https://github.com/\(releaseRepo)/releases/latest/download"
let changelogURL = URL(string: "https://github.com/\(releaseRepo)/blob/main/CHANGELOG.md")!

// 최신 릴리스 VERSION 받기 — 캐시 무시, 15초. done 은 URLSession 의 큐에서
struct GitHubReleaseVersions: ReleaseVersions {
    func fetchLatest(_ done: @escaping (_ status: Int?, _ body: Data?) -> Void) {
        let req = URLRequest(url: URL(string: releaseBase + "/VERSION")!, cachePolicy: .reloadIgnoringLocalCacheData,
                             timeoutInterval: 15)
        URLSession.shared.dataTask(with: req) { data, resp, _ in done((resp as? HTTPURLResponse)?.statusCode, data) }.resume()
    }
}

final class Updater: ObservableObject {
    static let shared = Updater()
    @Published var latest: String?   // 이 앱보다 새 버전이 나왔으면 그 버전
    @Published var running = false
    @Published var failed = false
    @Published var checking = false   // 손으로 누른 확인이 도는 중
    @Published var notice: (icon: String, text: String)?   // 손으로 확인한 결과 — 새 버전이 없을 때 알림 줄에 잠깐 뜬다
    let current = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""   // 제목 옆 버전 표시도 쓴다
    private let logPath = NSHomeDirectory() + "/.claude/notifier/update.log"
    private var timer: Timer?
    private var noticeHide: DispatchWorkItem?
    private let releases: ReleaseVersions = GitHubReleaseVersions()
    // build-board.sh 로 만든 개발용 빌드(1.0)는 배포판으로 덮어쓰지 않게 확인하지 않는다
    private var eligible: Bool { updateEligible(current) }

    private init() {
        guard eligible else { return }
        if UserDefaults.standard.bool(forKey: "autoUpdateCheck") { check() }
        timer = Timer.scheduledTimer(withTimeInterval: 3600, repeats: true) { [weak self] _ in
            if UserDefaults.standard.bool(forKey: "autoUpdateCheck") { self?.check() }   // 설정 > 업데이트
        }
    }

    // manual — 업데이트 확인 단추·메뉴에서 불렀을 때. 새 버전이 없거나 확인하지 못해도 결과를 알림 줄에 보인다
    func check(manual: Bool = false) {
        if manual {
            guard !checking else { return }
            // 개발용 빌드는 받지 않고 바로 알린다 — 도는 중(checking)으로 바꾸지 않는다
            guard eligible else { if let n = UpdateCheckResult.devBuild.notice(current: current) { show(n.icon, n.text) }; return }
            checking = true
        }
        checkForUpdate(current: current, releases: releases) { [weak self] result in
            guard let self else { return }
            DispatchQueue.main.async {
                // 받지 못했으면 알던 새 버전을 그대로 둔다
                switch result {
                case .newer(let v): withAnimation(.easeOut(duration: 0.2)) { self.latest = v }
                case .upToDate: withAnimation(.easeOut(duration: 0.2)) { self.latest = nil }
                case .failed, .devBuild: break
                }
                guard manual else { return }
                self.checking = false
                if let n = result.notice(current: self.current) { self.show(n.icon, n.text) }
            }
        }
    }

    private func show(_ icon: String, _ text: String) {
        withAnimation(.easeOut(duration: 0.2)) { notice = (icon, text) }
        noticeHide?.cancel()
        let hide = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.2)) { self?.notice = nil } }
        noticeHide = hide
        DispatchQueue.main.asyncAfter(deadline: .now() + 4, execute: hide)
    }

    func run() {
        guard !running else { return }
        // 보드 탭 세션은 보드와 함께 꺼진다 — 일하는 중인 것이 있으면 먼저 묻는다. 스크립트가 보드를 pkill 로 끄므로(종료 확인을 거치지 않음)
        // 목록·일시중지 표지는 여기서 남기고, 보드를 끄기 전에 스크립트가 멈추면(아래로 돌아오면) 남긴 표지를 지운다 — 세션은 그대로 도니까
        guard let marks = MainActor.assumeIsolated({ confirmShutdown(update: true) }) else { return }
        // 설정 화면은 닫아 둔다 — 스크립트가 pkill 로 끄므로 applicationWillTerminate 를 거치지 않고, 잠깐 펼친 창이면 접혀야 다시 뜰 자리가 맞다
        MainActor.assumeIsolated { SettingsScreen.shared.close(); TerminalHub.shared.slide?.finish() }
        running = true
        failed = false
        FileManager.default.createFile(atPath: logPath, contents: nil)
        let out = FileHandle(forWritingAtPath: logPath) ?? .nullDevice
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/bash")
        // pipefail — 받기에 실패하면 빈 스크립트가 성공으로 끝나지 않게 한다.
        // 출력은 파일로 받는다 — 스크립트가 도중에 이 앱을 끄므로 파이프로 받으면 쓰다가 끊겨 스크립트도 멈춘다
        p.arguments = ["-c", "set -o pipefail; /usr/bin/curl -fsSL '\(releaseBase)/install.sh' | /bin/bash"]
        p.standardInput = FileHandle.nullDevice
        p.standardOutput = out
        p.standardError = out
        // 성공하면 스크립트가 이 앱을 끄고 새 앱을 띄우므로 여기로 돌아오지 않는다 — 돌아오면 끄기 전에 멈춘 것이다
        p.terminationHandler = { [weak self] p in
            DispatchQueue.main.async {
                for m in marks { try? FileManager.default.removeItem(atPath: m) }
                self?.running = false
                self?.failed = p.terminationStatus != 0
            }
        }
        do { try p.run() } catch {
            for m in marks { try? FileManager.default.removeItem(atPath: m) }
            running = false
            failed = true
        }
    }
}

// 새 버전 알림 줄 — 전체·중간 화면의 윗줄 아래에 뜬다
struct UpdateBanner: View {
    @ObservedObject var updater: Updater
    @State private var hover = false
    @State private var notesHover = false

    var body: some View {
        let tint = updater.failed ? Palette.color("waiting") : Palette.accent
        // 새 버전이 없으면 손으로 확인한 결과(최신·확인 실패)만 단추 없이 보인다
        let notice = updater.latest == nil ? updater.notice : nil
        HStack(spacing: 6) {
            Image(systemName: notice?.icon ?? (updater.failed ? "exclamationmark.triangle.fill" : "arrow.down.circle.fill"))
                .font(.system(size: 12)).foregroundStyle(tint)
            Text(notice?.text ?? (updater.failed ? "업데이트하지 못했습니다 · ~/.claude/notifier/update.log"
                 : updater.running ? "업데이트 중… 끝나면 보드가 다시 열립니다"
                 : "새 버전 \(updater.latest ?? "") 이 나왔습니다"))
                .boardFont(12, weight: .semibold).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 6)
            if updater.running {
                ProgressView().controlSize(.small)
            } else if notice == nil {
                // 무엇이 바뀌었는지 — 브라우저에서 GitHub 의 CHANGELOG.md 를 연다
                Button("변경 내용") { NSWorkspace.shared.open(changelogURL) }
                    .buttonStyle(.plain).boardFont(12, weight: .semibold)
                    .foregroundStyle(notesHover ? .primary : .secondary)
                    .clickable($notesHover)
                    .hint("GitHub 에서 CHANGELOG.md 를 엽니다")
                Rectangle().fill(Color.white.opacity(0.14)).frame(width: 0.5, height: 10).padding(.horizontal, 2)
                Button(updater.failed ? "다시 시도" : "업데이트") { updater.run() }
                    .buttonStyle(.plain).boardFont(12, weight: .semibold)
                    .foregroundStyle(tint.opacity(hover ? 1 : 0.85))
                    .clickable($hover)
                    .hint("새 버전을 받아 바꿔 끼웁니다. 끝나면 보드가 다시 열립니다")
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 8).fill(tint.opacity(0.12)))
    }
}
