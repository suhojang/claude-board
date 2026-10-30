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

// MARK: - 자율주행 · 보고서 뷰어

// 3단계(2026-10-30, 설계 6장): 지난 실행·합치기 시트의 "보고서 보기"가 여는 창. 보고서 글은 규칙 AutopilotRules 의 틀(apReportHTML)로
// 보드가 그 자리에서 그린다 — 실행 폴더가 있으면 그 기록(엔진이 report.html 을 그린 것과 같은 바탕), 원문을 지웠으면 기록 저장소(결과 기록 전문).
// 엔진이 쓴 report.html 파일을 열지 않는 것은 설계 판단: 단위 폴더는 모델이 쓸 수 있는 자리이고, 화면은 늘 지금 기록으로 그리며, 원문을 지운 실행도 같은 길로 보인다.
// 웹 보기는 가둔다: JavaScript 끔, 처음 글(about:blank)과 같은 문서 안 이동(#unit-n)만 허용, https 링크는 확인 뒤 기본 브라우저로,
// 그 밖 이동·새 창은 막음, http(s)·ws(s)·ftp 요청은 콘텐츠 규칙으로도 막음(보고서 글의 CSP 와 겹쳐), 저장소는 nonPersistent,
// 글은 loadHTMLString(baseURL: nil) 로 넣어 로컬 파일을 하나도 읽지 않는다. "파일로 저장…"은 화면에 보이는 글 그대로 한 파일로 쓴다

// 보드 쪽 읽을 곳(실행 폴더) — 모델이 쓸 수 있는 단위 폴더의 파일(result.json·engine.json)은 링크가 아닌 보통 파일이고 상한 아래일 때만 읽는다
struct APFolderReportSource: APReportSource {
    let dir: String
    var origin: APReportOrigin { .folder }
    static let maxBytes = 4 << 20
    static func plainFile(_ p: String) -> Data? {
        var st = stat()
        guard lstat(p, &st) == 0, (st.st_mode & S_IFMT) == S_IFREG, st.st_size <= off_t(maxBytes) else { return nil }
        return FileManager.default.contents(atPath: p)
    }
    func run() -> [String: Any]? { Self.plainFile(dir + "/run.json").flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any] }
    func queue() -> [[String: Any]] { Self.plainFile(dir + "/queue.json").flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [[String: Any]] ?? [] }
    func engine(_ n: Int) -> [String: Any]? { Self.plainFile(dir + "/units/\(n)/engine.json").flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any] }
    func result(_ n: Int) -> String? { Self.plainFile(dir + "/units/\(n)/result.json").map { String(decoding: $0, as: UTF8.self) } }
}

// 보드 쪽 읽을 곳(기록 저장소) — 행을 run.json·queue.json·engine.json 꼴 사전으로 바꾼 값(APRecords.reportSource 가 읽음)
struct APRecordsReportSource: APReportSource {
    let runValue: [String: Any]
    let queueValue: [[String: Any]]
    let engines: [Int: [String: Any]]
    let results: [Int: String]
    var origin: APReportOrigin { .records }
    func run() -> [String: Any]? { runValue }
    func queue() -> [[String: Any]] { queueValue }
    func engine(_ n: Int) -> [String: Any]? { engines[n] }
    func result(_ n: Int) -> String? { results[n] }
}

// 실행 하나의 보고서 글 — 실행 폴더의 run.json 이 있으면 그것, 없으면 기록 저장소. 둘 다 없으면 nil. 메인 스레드에서 부르지 않는다
func apBoardReportHTML(run id: String, runDir: String) -> String? {
    let src: APReportSource?
    if !runDir.isEmpty, APFolderReportSource.plainFile(runDir + "/run.json") != nil {
        src = APFolderReportSource(dir: runDir)
    } else {
        src = APRecords.reportSource(autopilotRoot + "/private/records.sqlite", run: id)
    }
    guard let s = src, let r = apReportRun(id: id, source: s) else { return nil }
    return apReportHTML(r, at: Date().timeIntervalSince1970, tz: .current, home: NSHomeDirectory())
}

// 보고서 창 — 실행마다 하나(같은 실행을 다시 누르면 앞으로 꺼내고 다시 그림)
final class APReportViewer: NSObject, NSWindowDelegate, WKNavigationDelegate, WKUIDelegate {
    private static var open: [String: APReportViewer] = [:]
    private static var blockList: WKContentRuleList?
    private static var blockTried = false
    // 바깥 요청 막기(콘텐츠 규칙): 주소 꼴이 http(s)·ws(s)·ftp 면 어떤 자원이든 막는다. about:blank 와 같은 문서 안 이동은 걸리지 않는다
    private static let blockRules = #"[{"trigger":{"url-filter":"^(https?|wss?|ftps?)://"},"action":{"type":"block"}}]"#

    let id: String
    private var html = ""
    private var title = ""
    private var win: NSWindow?
    private var web: WKWebView?
    private var note: NSTextField?
    private var loaded = false

    // 지난 실행·합치기 시트에서 부른다. 기록을 뒤에서 읽고 그린 뒤 창을 연다
    static func show(run id: String, runDir: String, title: String) {
        if let v = open[id] { v.win?.makeKeyAndOrderFront(nil) }
        DispatchQueue.global(qos: .userInitiated).async {
            let text = apBoardReportHTML(run: id, runDir: runDir)
            DispatchQueue.main.async {
                guard let text else {
                    let a = NSAlert()
                    a.messageText = "보고서를 그릴 기록이 없습니다."
                    a.informativeText = "실행 폴더의 run.json 도, 기록 저장소의 이 실행 행도 읽지 못했습니다."
                    a.runModal()
                    return
                }
                let v = open[id] ?? APReportViewer(id: id)
                open[id] = v
                v.present(text, title: title)
            }
        }
    }

    private init(id: String) { self.id = id; super.init() }

    private func present(_ text: String, title t: String) {
        html = text
        title = t.isEmpty ? id : t
        if win == nil { build() }
        win?.title = "자율주행 보고서: " + title
        note?.stringValue = ""
        withBlockList { [weak self] in
            guard let self, let web = self.web else { return }
            self.loaded = false
            web.loadHTMLString(self.html, baseURL: nil)
        }
        win?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func withBlockList(_ then: @escaping () -> Void) {
        if Self.blockTried { attachBlock(); then(); return }
        Self.blockTried = true
        WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "claude-board-report-block", encodedContentRuleList: Self.blockRules) { list, err in
            DispatchQueue.main.async {
                Self.blockList = list
                if list == nil { boardLog("자율주행 보고서: 콘텐츠 규칙을 만들지 못했습니다(CSP·이동 막기는 그대로): \(err?.localizedDescription ?? "?")") }
                self.attachBlock()
                then()
            }
        }
    }
    private func attachBlock() {
        guard let l = Self.blockList, let ucc = web?.configuration.userContentController else { return }
        ucc.removeAllContentRuleLists()
        ucc.add(l)
    }

    private func build() {
        let cfg = WKWebViewConfiguration()
        cfg.websiteDataStore = .nonPersistent()
        cfg.defaultWebpagePreferences.allowsContentJavaScript = false
        cfg.preferences.javaScriptCanOpenWindowsAutomatically = false
        cfg.preferences.isElementFullscreenEnabled = false
        cfg.suppressesIncrementalRendering = true
        let web = WKWebView(frame: .zero, configuration: cfg)
        web.navigationDelegate = self
        web.uiDelegate = self
        web.allowsBackForwardNavigationGestures = false
        web.allowsLinkPreview = false
        web.allowsMagnification = true
        web.setAccessibilityIdentifier("ap-report-web")
        web.translatesAutoresizingMaskIntoConstraints = false

        let save = NSButton(title: "파일로 저장…", target: self, action: #selector(saveFile))
        save.bezelStyle = .rounded
        save.setAccessibilityIdentifier("ap-report-save")
        save.toolTip = "보이는 보고서를 HTML 파일 하나로 저장합니다. 바깥 글꼴·스크립트 없이 이 파일만으로 열립니다"
        let close = NSButton(title: "닫기", target: self, action: #selector(closeWindow))
        close.bezelStyle = .rounded
        close.setAccessibilityIdentifier("ap-report-close")
        let note = NSTextField(labelWithString: "")
        note.textColor = .secondaryLabelColor
        note.font = .systemFont(ofSize: 11.5)
        note.lineBreakMode = .byTruncatingMiddle
        note.setAccessibilityIdentifier("ap-report-note")
        let bar = NSStackView(views: [note, NSView(), save, close])
        bar.orientation = .horizontal
        bar.spacing = 8
        bar.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        bar.translatesAutoresizingMaskIntoConstraints = false
        note.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        let root = NSView()
        root.addSubview(bar)
        root.addSubview(web)
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: root.topAnchor), bar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            web.topAnchor.constraint(equalTo: bar.bottomAnchor), web.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            web.trailingAnchor.constraint(equalTo: root.trailingAnchor), web.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 820), styleMask: [.titled, .closable, .resizable, .miniaturizable],
                         backing: .buffered, defer: false)
        w.contentView = root
        w.isReleasedWhenClosed = false
        w.minSize = NSSize(width: 360, height: 300)
        w.setAccessibilityIdentifier("ap-report-window")
        let board = boardWindow()
        w.level = board.level   // 위에 고정한 보드에 가리지 않게
        w.delegate = self
        let f = board.frame
        w.setFrameOrigin(NSPoint(x: max(f.minX - 920, 20), y: max(f.midY - 410, 20)))
        if let screen = board.screen?.visibleFrame, !screen.intersects(w.frame) { w.center() }
        self.web = web; self.note = note; self.win = w
    }

    @objc private func saveFile() {
        guard let w = win else { return }
        let p = NSSavePanel()
        p.allowedContentTypes = [.html]
        p.nameFieldStringValue = "\(id)-report.html"
        p.canCreateDirectories = true
        p.beginSheetModal(for: w) { [weak self] resp in
            guard let self, resp == .OK, let url = p.url else { return }
            do {
                try Data(self.html.utf8).write(to: url, options: .atomic)
                self.note?.stringValue = "저장했습니다: " + url.path.replacingOccurrences(of: NSHomeDirectory(), with: "~")
            } catch {
                self.note?.stringValue = "저장하지 못했습니다: " + error.localizedDescription
            }
        }
    }

    @objc private func closeWindow() { win?.close() }

    func windowWillClose(_ notification: Notification) {
        web?.navigationDelegate = nil
        web?.uiDelegate = nil
        Self.open[id] = nil
    }

    // 이동 막기 — 처음 글과 같은 문서 안 이동(#unit-n)만. https 링크는 확인 뒤 기본 브라우저, 그 밖은 아무 일도 하지 않는다
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let url = action.request.url
        let main = action.targetFrame?.isMainFrame ?? false
        if let u = url, u.scheme == "about", u.absoluteString.hasPrefix("about:blank"), main, !loaded || u.fragment != nil {
            decisionHandler(.allow)
            return
        }
        decisionHandler(.cancel)
        if action.navigationType == .linkActivated, let u = url, let link = apSafeLink(u.absoluteString) { askOpen(link) }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { loaded = true }

    // 새 창(target=_blank·window.open)은 만들지 않는다
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction,
                             windowFeatures: WKWindowFeatures) -> WKWebView? { nil }

    private func askOpen(_ link: String) {
        guard let w = win, let url = URL(string: link) else { return }
        let a = NSAlert()
        a.messageText = "기본 브라우저에서 열까요?"
        a.informativeText = "보고서의 링크는 모델이 결과 기록에 적은 주소입니다. 주소를 확인한 뒤 엽니다.\n\n" + link
        a.addButton(withTitle: "열기")
        a.addButton(withTitle: "그만두기")
        a.beginSheetModal(for: w) { resp in
            if resp == .alertFirstButtonReturn { NSWorkspace.shared.open(url) }
        }
    }
}
