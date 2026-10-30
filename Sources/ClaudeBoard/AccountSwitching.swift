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

// MARK: - 계정 전환

// Claude Code 로그인 정보는 키체인 일반 암호 항목(서비스 "Claude Code-credentials", 계정 $USER)에 JSON 으로 있다.
// /usr/bin/security 로 읽고 쓴다 — Claude Code 도 security 로 만들고 고치므로 허용 창이 뜨지 않는다.
// 쓸 값은 명령줄 대신 표준 입력(security -i)으로 넘겨 ps 에 드러나지 않게 한다
enum Keychain {
    // Claude Code 와 같은 계정 이름 규칙
    static let user: String = {
        let u = ProcessInfo.processInfo.environment["USER"] ?? NSUserName()
        return u.range(of: "^[a-zA-Z0-9._-]+$", options: .regularExpression) != nil ? u : "claude-code-user"
    }()

    @discardableResult
    private static func security(_ args: [String], input: String? = nil) -> (status: Int32, out: Data) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        p.arguments = args
        let out = Pipe(), inp = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        if input != nil { p.standardInput = inp }
        guard (try? p.run()) != nil else { return (-1, Data()) }
        if let input {
            inp.fileHandleForWriting.write(Data(input.utf8))
            try? inp.fileHandleForWriting.close()
        }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return (p.terminationStatus, data)
    }

    static func read(_ service: String, account: String = user) -> Data? {
        let r = security(["find-generic-password", "-a", account, "-s", service, "-w"])
        guard r.status == 0 else { return nil }
        var d = r.out
        while d.last == 0x0a { d.removeLast() }
        // ASCII 가 아닌 바이트가 섞인 값은 16진수로 나온다 — JSON 은 { 로 시작하므로 16진수 글자만 있으면 풀어서 돌려준다
        let hexDigits = Set("0123456789abcdef".utf8)
        if d.first != UInt8(ascii: "{"), d.count % 2 == 0, d.allSatisfy(hexDigits.contains) {
            let s = Array(d)
            d = Data(stride(from: 0, to: s.count, by: 2).map { UInt8(String(decoding: s[$0..<$0 + 2], as: UTF8.self), radix: 16)! })
        }
        return d.isEmpty ? nil : d
    }

    static func exists(_ service: String, account: String = user) -> Bool {
        security(["find-generic-password", "-a", account, "-s", service]).status == 0
    }

    static func write(_ service: String, account: String = user, _ data: Data) -> Bool {
        let hex = data.map { String(format: "%02x", $0) }.joined()
        let line = "add-generic-password -U -a \"\(account)\" -s \"\(service)\" -X \"\(hex)\"\n"
        // security -i 는 한 줄 4KB 까지 받는다 — 넘으면 Claude Code 처럼 명령줄로 넘긴다
        if line.utf8.count <= 4032 { return security(["-i"], input: line).status == 0 }
        return security(["add-generic-password", "-U", "-a", account, "-s", service, "-X", hex]).status == 0
    }

    static func delete(_ service: String, account: String = user) {
        security(["delete-generic-password", "-a", account, "-s", service])
    }

    // Claude Code 는 security -w 출력을 그대로 JSON 으로 읽는다 — 16진수로 나오지 않게 ASCII 밖 글자는 \uXXXX 로 쓴다
    static func asciiJSON(_ obj: Any) throws -> Data {
        let s = String(decoding: try JSONSerialization.data(withJSONObject: obj, options: [.withoutEscapingSlashes]), as: UTF8.self)
        var out = ""
        for u in s.utf16 {
            if u < 0x80 { out.unicodeScalars.append(Unicode.Scalar(UInt8(u))) } else { out += String(format: "\\u%04x", u) }
        }
        return Data(out.utf8)
    }

    static func json(_ service: String, account: String = user) -> [String: Any]? {
        read(service, account: account).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }
}

// Claude Code 의 로그인 정보 키체인 서비스 이름 — CLAUDE_CONFIG_DIR 을 정해 띄우면 "-<그 경로(NFC) sha256 앞 8자>" 가 붙는다 (2.1.300 실측)
func credentialsService(_ configDir: String? = nil) -> String {
    guard let dir = configDir else { return "Claude Code-credentials" }
    let hex = SHA256.hash(data: Data(dir.precomposedStringWithCanonicalMapping.utf8)).map { String(format: "%02x", $0) }.joined()
    return "Claude Code-credentials-\(hex.prefix(8))"
}

// Claude Code 로그인 정보 항목 — 서비스는 위 이름, 계정 이름은 $USER(Keychain.user). configDir 이 없으면 CLAUDE_CONFIG_DIR 없이 띄운 세션들이 쓰는 주 항목
func claudeLoginItem(_ configDir: String? = nil) -> KeychainItem {
    KeychainItem(service: credentialsService(configDir), account: Keychain.user)
}

// 계정 전환 오류(AccountError)는 BoardRules(Accounts.swift)에 있다 — 아래 잠금·설정 고치기도 그것으로 던진다

// Claude Code 와 같은 잠금 — proper-lockfile 처럼 "<파일>.lock" 폴더를 만들어 잡는다. 15초 넘게 갱신되지 않은 잠금은 버려진 것으로 본다
func withFileLock<T>(_ file: String, _ body: () throws -> T) throws -> T {
    let lock = file + ".lock"
    let deadline = Date().addingTimeInterval(5)
    while mkdir(lock, 0o755) != 0 {
        guard errno == EEXIST else { throw AccountError("잠금을 만들지 못했습니다: \(lock)") }
        if let m = mtime(lock), Date().timeIntervalSince(m) > 15 { rmdir(lock); continue }
        guard Date() < deadline else { throw AccountError("Claude Code 가 로그인 정보를 쓰는 중입니다. 잠시 뒤 다시 시도해 주세요", side: .busy) }
        Thread.sleep(forTimeInterval: 0.1)
    }
    defer { rmdir(lock) }
    return try body()
}

// JSON 설정 파일의 몇 키만 바꿔 쓴다 — Claude Code 와 같은 잠금 아래에서 읽고 고친 뒤 임시 파일을 거쳐 바꿔 넣는다. 권한(0600)은 그대로 둔다
func patchJSON(_ path: String, _ change: (inout [String: Any]) -> Void) throws {
    try withFileLock(path) {
        guard let data = FileManager.default.contents(atPath: path),
              var j = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AccountError("\((path as NSString).lastPathComponent) 를 읽지 못했습니다")
        }
        change(&j)
        let out = try JSONSerialization.data(withJSONObject: j, options: [.prettyPrinted, .withoutEscapingSlashes])
        let perms = (try? FileManager.default.attributesOfItem(atPath: path)[.posixPermissions]) ?? 0o600
        try out.write(to: URL(fileURLWithPath: path), options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: perms], ofItemAtPath: path)
    }
}

// MARK: 계정 전환의 진짜 어댑터 — 규칙(계정 모델·저장 계정 목록·전환 JSON 변환·저장·전환·지우기·자동 전환 판단)은
// BoardRules/Accounts.swift·AutoSwitch.swift, 포트는 BoardRules/Ports.swift(가짜 어댑터는 Tests 에만). 넷 다 상태 없는 struct 라 그대로 Sendable 이다

// 키체인 일반 암호 항목 — security 명령·16진수 풀기·ASCII JSON·4KB 넘으면 명령줄은 Keychain 에 그대로 있다
struct SystemKeychain: KeychainItems {
    func read(_ item: KeychainItem) -> Data? { Keychain.read(item.service, account: item.account) }
    func exists(_ item: KeychainItem) -> Bool { Keychain.exists(item.service, account: item.account) }
    func write(_ item: KeychainItem, json: [String: Any]) throws -> Bool {
        Keychain.write(item.service, account: item.account, try Keychain.asciiJSON(json))
    }
    func delete(_ item: KeychainItem) { Keychain.delete(item.service, account: item.account) }
}

// Claude Code 설정(~/.claude.json, 계정 추가 중에는 따로 마련한 설정 폴더의 것)
struct ClaudeConfigFile: ClaudeConfig {
    let path: String
    init(_ path: String = NSHomeDirectory() + "/.claude.json") { self.path = path }
    // 수정 시각 — AccountStore 가 3초마다 보고 바뀔 때만 다시 읽는다(규칙은 쓰지 않는다)
    func modifiedAt() -> Date? { mtime(path) }
    func oauthAccount() -> [String: Any]? {
        guard let data = FileManager.default.contents(atPath: path),
              let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return j["oauthAccount"] as? [String: Any]
    }
    func patch(_ change: (inout [String: Any]) -> Void) throws { try patchJSON(path, change) }
}

// 보드의 저장 계정 목록 — 이메일·조직 이름이 들어 있어 0600
struct SavedAccountsFile: SavedAccountList {
    let path = NSHomeDirectory() + "/.claude/notifier/accounts.json"
    func read() -> Data? { FileManager.default.contents(atPath: path) }
    func write(_ data: Data) throws {
        try data.write(to: URL(fileURLWithPath: path), options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path)
    }
    // 깨진 목록을 accounts.json.broken-<지역 시각 yyyyMMdd-HHmmss> 로 옮긴다(파일 이름에 쓰려고 쌍점 없이). 같은 이름이 있으면 -2, -3 … 을 붙인다.
    // 다른 계정의 로그인 정보는 키체인에 그대로 있으므로 사용자가 이 파일에서 항목을 되살리면 된다
    func setAside(at date: Date) throws {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyyMMdd-HHmmss"
        let base = path + ".broken-" + f.string(from: date)
        var target = base, n = 2
        while FileManager.default.fileExists(atPath: target) { target = "\(base)-\(n)"; n += 1 }
        do {
            try FileManager.default.moveItem(atPath: path, toPath: target)
        } catch {
            boardLog("저장 계정 목록(accounts.json)을 읽지 못해 옆으로 옮기려 했지만 옮기지 못함: \(error.localizedDescription)")
            throw error
        }
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: target)
        boardLog("저장 계정 목록(accounts.json)을 읽지 못해 \((target as NSString).lastPathComponent) 로 옮김")
    }
}

// Claude Code 가 토큰을 갱신해 쓸 때 잡는 잠금
struct StorageWriteLock: CredentialsWriteLock {
    let path = NSHomeDirectory() + "/.claude/.storage-write"
    func withLock<T>(_ body: () throws -> T) throws -> T { try withFileLock(path, body) }
}

// 계정 추가 로그인 창 — 로그인 명령이 열려던 주소(localhost 콜백)를 보드가 띄운 웹 창에서 연다. 외부 브라우저 탭은 닫아도 보드가
// 알 길이 없고, macOS 로그인 창(ASWebAuthenticationSession)은 기본 브라우저가 Safari 가 아니면 Safari 앱을 띄웠다(사용자 결정).
// 쿠키는 창마다 새로(nonPersistent) — 브라우저에 로그인된 계정이 저절로 이어져 다른 계정을 못 넣는 일이 없게. 완료는 AccountStore 가
// close() 로 닫고, 사용자가 닫으면 closed 를 부른다. 한계: Google 은 앱 안 웹 창의 Google 로그인을 막을 수 있다(이메일 로그인은 된다)
func boardWindow() -> NSWindow { window }

final class LoginWebWindow: NSObject, NSWindowDelegate, WKUIDelegate {
    private var win: NSWindow?
    private var ours = false   // 우리가 닫았다 — 사용자가 닫은 것으로 치지 않는다
    private let closed: () -> Void

    init(_ url: URL, closed: @escaping () -> Void) {
        self.closed = closed
        super.init()
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 520, height: 720), configuration: config)
        web.uiDelegate = self
        let w = NSWindow(contentRect: web.frame, styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        w.title = "Claude 계정 추가"
        w.contentView = web
        w.isReleasedWhenClosed = false
        let board = boardWindow()   // NSWindowDelegate 안에서는 window 가 delegate 메서드 이름으로 읽힌다
        w.level = board.level   // 위에 고정한 보드에 가리지 않게
        w.delegate = self
        let f = board.frame
        w.setFrameOrigin(NSPoint(x: f.midX - w.frame.width / 2, y: f.midY - w.frame.height / 2))
        win = w
        web.load(URLRequest(url: url))
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func close() {
        ours = true
        win?.close()
    }

    func windowWillClose(_ notification: Notification) {
        win = nil
        if !ours { closed() }
    }

    // 새 창으로 여는 링크(target=_blank·window.open)는 같은 창에서
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if navigationAction.targetFrame == nil { webView.load(navigationAction.request) }
        return nil
    }
}

// 계정 전환: 계정마다 브라우저 로그인을 한 번 받아 토큰을 저장해 두고, 고르면 Claude Code 로그인 항목의 토큰만 바꿔 넣는다.
// 실행 중인 세션은 요청을 보낼 때마다 키체인을 다시 읽으므로(최대 30초 캐시) 모든 세션이 다음 요청부터 새 계정으로 이어 간다.
// 계정 모델·저장 계정 목록·전환할 때의 JSON 변환·저장·전환·지우기는 BoardRules(Accounts.swift)에 있다 — 여기는 @Published 상태·큐·3초 감시·안내,
// 계정 추가 로그인 흐름(pty·로그인 창·코드 넣기·끝났는지 보기)을 쥔다
final class AccountStore: ObservableObject {
    struct Notice: Equatable { let text: String; let error: Bool }

    @Published private(set) var current: Account?          // 지금 로그인된 계정 — ~/.claude.json 의 oauthAccount
    @Published private(set) var saved: [Account] = []      // 전환할 수 있게 저장해 둔 계정
    @Published private(set) var adding = false              // 계정 추가 — 브라우저 로그인을 기다리는 중
    @Published private(set) var loginURL: URL?              // 로그인 명령이 알려 준 브라우저 로그인 주소(설정 > 계정에 보인다)
    @Published private(set) var loginNote: String?          // 로그인 명령의 마지막 안내(코드를 보냈다·실패 까닭 등)
    @Published private(set) var switching = false
    @Published private(set) var notice: Notice?
    // 사용 한도가 차면 다른 계정으로 자동 전환(AutoSwitcher) — 계정 메뉴에서 켜고 끈다. 기본은 꺼짐
    @Published var autoSwitch = UserDefaults.standard.bool(forKey: "autoSwitch") {
        didSet { UserDefaults.standard.set(autoSwitch, forKey: "autoSwitch") }
    }
    var onSwitch: (() -> Void)?

    // 지금 계정 — 3초마다 수정 시각을 보고 바뀔 때만 다시 읽는다
    private let config = ClaudeConfigFile()
    // 저장·전환·지우기 유스케이스에 넘기는 진짜 어댑터. 로그인 정보는 CLAUDE_CONFIG_DIR 없이 띄운 세션들이 쓰는 주 항목만 바꾼다
    private let ports = AccountPorts(keychain: SystemKeychain(), login: claudeLoginItem(), config: ClaudeConfigFile(),
                                     saved: SavedAccountsFile(), writeLock: StorageWriteLock(), clock: SystemClock())
    private let loginRoot = NSHomeDirectory() + "/.claude/notifier/accounts"
    private let queue = DispatchQueue(label: "com.claude.board.accounts")
    private var currentMtime: Date?
    private var timer: Timer?
    private var login: (slot: String, started: Date)?
    private var loginProcess: Process?                      // 계정 추가 로그인 명령(claude auth login) — pty 로 띄운다
    private var loginPty: FileHandle?                       // 그 명령의 터미널 쪽 — 코드를 써 넣는다
    private var loginText = ""                              // 그 명령의 출력(색 글자를 뺀 것)
    private var browserLogin: LoginWebWindow?               // 그 명령이 열려던 주소를 연 로그인 창 — 닫으면 계정 추가를 취소한다
    private var browserWait: Timer?
    private var tokenOnlySince: Date?       // 로그인 토큰은 생겼는데 계정 정보가 아직 없는 때
    private var polling = false
    private var noticeWork: DispatchWorkItem?

    init() {
        loadSaved()
        loadCurrent()
        // 터미널에서 /login 으로 바꿔도 따라가도록 계정 정보 파일을 지켜본다 — 바뀔 때만 다시 읽는다
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            self?.loadCurrent()
            self?.pollLogin()
        }
        // 지난번에 끝나지 않은 로그인 자리를 치운다
        queue.async { [loginRoot] in
            for name in (try? FileManager.default.contentsOfDirectory(atPath: loginRoot)) ?? [] where name.hasPrefix("login-") {
                Self.dropSlot(loginRoot + "/" + name)
            }
        }
    }

    func loadCurrent(force: Bool = false) {
        let m = config.modifiedAt()
        guard force || m != currentMtime else { return }
        currentMtime = m
        let a = currentAccount(config)
        if a != current { current = a }
    }

    private func loadSaved() {
        let list = loadSavedAccounts(ports.saved)
        if list != saved { saved = list }
    }

    // 전환 요청의 결과 — 자동 전환이 받는다. 실패는 던진 오류 그대로(쪽은 BoardRules 의 switchFailureSide 가 가른다)
    enum SwitchOutcome {
        case switched
        case busy               // 다른 전환이 도는 중이라 하지 않음 — 자동 전환은 표시도 쉼도 없이 다음 판단에서 다시 본다
        case failed(Error)
    }

    // note: 전환했을 때 띄울 안내(자동 전환), done: 결과 — 자동 전환이 받는다. 실패 안내는 여기서 띄운다(손으로 전환하는 길과 같다)
    func switchTo(_ a: Account, note: String? = nil, done: ((SwitchOutcome) -> Void)? = nil) {
        guard !switching else { done?(.busy); return }
        switching = true
        queue.async { [weak self] in
            guard let self else { return }
            let r = Result { try switchAccount(to: a, ports: self.ports) }
            DispatchQueue.main.async {
                self.switching = false
                self.loadCurrent(force: true)
                self.loadSaved()
                switch r {
                case .success(let warning):
                    self.onSwitch?()
                    self.flash(warning ?? note ?? "\(a.email) 계정으로 전환했습니다. 실행 중인 세션도 다음 요청부터 이 계정을 씁니다",
                               error: warning != nil)
                    done?(.switched)
                case .failure(let e):
                    self.flash("전환하지 못했습니다: \(e.localizedDescription)", error: true)
                    done?(.failed(e))
                }
            }
        }
    }

    // 계정 추가: 따로 정한 설정 폴더(CLAUDE_CONFIG_DIR)로 claude auth login 을 터미널에서 띄운다 — 지금 로그인은 건드리지 않는다.
    // 로그인이 끝나면 그 폴더 몫 키체인 항목에서 토큰을 가져와 저장하고 자리를 치운다
    // 계정 추가(설정 > 계정, 계정 칩 메뉴는 설정 화면을 열고 여기로) — 따로 마련한 설정 폴더로 claude auth login 을 사용자 로그인 셸에서
    // pty 로 띄운다. 명령이 브라우저를 열고, 로그인이 끝나면 localhost 콜백으로 저절로 마친다. 브라우저가 코드를 보여 주는 경우는
    // 설정 화면의 칸에서 받아 명령에 써 넣는다(submitLoginCode). 끝났는지는 그 폴더를 지켜보는 pollLogin 이 판단한다
    func beginAdd(email: String? = nil) {
        guard login == nil else { return }
        guard let claude = findCommand("claude") else { return flash("claude 명령을 찾지 못했습니다", error: true) }
        let slot = loginRoot + "/login-" + UUID().uuidString.prefix(8).lowercased()
        do { try FileManager.default.createDirectory(atPath: slot, withIntermediateDirectories: true) } catch {
            return flash("로그인 자리를 만들지 못했습니다: \(error.localizedDescription)", error: true)
        }
        // 명령이 브라우저를 직접 열지 않고(BROWSER) 주소를 파일로 남기게 한다 — 보드가 그 주소를 제 로그인 창(LoginWebWindow)으로
        // 열어, 사용자가 창을 닫으면 알아채고 취소한다(외부 브라우저 탭은 닫아도 알 길이 없다)
        let opener = slot + "/open-browser", urlFile = slot + "/open-url"
        try? "#!/bin/sh\nprintf '%s' \"$1\" > \(shellQuote(urlFile))\n".write(toFile: opener, atomically: true, encoding: .utf8)
        try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: opener)
        var cmd = "CLAUDE_CONFIG_DIR=\(shellQuote(slot)) BROWSER=\(shellQuote(opener)) exec \(shellQuote(claude.path)) auth login --claudeai"
        if let email { cmd += " --email \(shellQuote(email))" }
        // 줄이 접히지 않게 넓은 터미널로 — 로그인 주소가 한 줄로 나와야 읽는다
        var size = winsize(ws_row: 50, ws_col: 1000, ws_xpixel: 0, ws_ypixel: 0)
        var master: Int32 = -1, slave: Int32 = -1
        guard openpty(&master, &slave, nil, nil, &size) == 0 else {
            try? FileManager.default.removeItem(atPath: slot)
            return flash("로그인 명령을 띄우지 못했습니다", error: true)
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: userShell())
        p.arguments = ["-l", "-c", cmd]   // 로그인 셸 — 터미널에서 친 것과 같은 PATH·환경
        let tty = FileHandle(fileDescriptor: slave, closeOnDealloc: true)
        p.standardInput = tty
        p.standardOutput = tty
        p.standardError = tty
        let pty = FileHandle(fileDescriptor: master, closeOnDealloc: true)
        // read(2) 로 읽는다 — availableData 는 읽기 오류(명령이 끝난 뒤의 EIO)에 예외를 던진다. 글자 하나가 두 번에 나뉘어 와도
        // 멈추지 않게 깨진 바이트는 대체 글자로 둔다(찾는 것은 ASCII 주소·안내뿐)
        pty.readabilityHandler = { [weak self] h in
            var buf = [UInt8](repeating: 0, count: 8192)
            let n = read(h.fileDescriptor, &buf, buf.count)
            guard n > 0 else { h.readabilityHandler = nil; return }
            let t = String(decoding: buf[0..<n], as: UTF8.self)
            DispatchQueue.main.async { self?.loginOutput(t, slot: slot) }
        }
        p.terminationHandler = { [weak self] p in
            DispatchQueue.main.async { self?.loginEnded(p.terminationStatus, slot: slot) }
        }
        do { try p.run() } catch {
            try? FileManager.default.removeItem(atPath: slot)
            return flash("로그인 명령을 띄우지 못했습니다: \(error.localizedDescription)", error: true)
        }
        try? tty.close()   // 자식이 쥐었다 — 우리 쪽 사본은 닫아야 자식이 끝날 때 읽기가 끝난다
        loginProcess = p
        loginPty = pty
        loginText = ""
        loginURL = nil
        loginNote = nil
        login = (slot, Date())
        adding = true
        // 명령이 BROWSER 를 부르면 주소가 생긴다 — 받으면 로그인 창을 연다(10초 넘게 없으면 그만 본다: 주소 복사·코드 칸은 그대로)
        let started = Date()
        browserWait = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] t in
            guard let self, self.login?.slot == slot, Date().timeIntervalSince(started) < 10 else { t.invalidate(); return }
            guard let s = try? String(contentsOfFile: urlFile, encoding: .utf8), let url = URL(string: s) else { return }
            t.invalidate()
            self.openBrowserLogin(url, slot: slot)
        }
    }

    // 로그인 창 — 닫으면(완료 전) 계정 추가를 취소한다
    func openBrowserLogin(_ url: URL, slot: String) {
        browserLogin?.close()
        browserLogin = LoginWebWindow(url) { [weak self] in
            guard let self, self.login?.slot == slot else { return }
            self.cancelAdd()
            self.flash("로그인 창을 닫아 계정 추가를 취소했습니다")
        }
    }

    // 로그인 명령의 출력 — 색 글자를 빼고 모아 두고, 로그인 주소를 찾는다
    private func loginOutput(_ chunk: String, slot: String) {
        guard login?.slot == slot else { return }
        loginText += chunk.replacingOccurrences(of: "\u{1B}\\[[0-9;?]*[ -/]*[@-~]|\u{1B}\\][^\u{07}]*\u{07}", with: "", options: .regularExpression)
        if loginText.count > 20_000 { loginText = String(loginText.suffix(10_000)) }
        if loginURL == nil, let r = loginText.range(of: "https://[^\\s]+oauth/authorize[^\\s]*(?=\\s)", options: .regularExpression) {
            loginURL = URL(string: String(loginText[r]))
        }
    }

    // 로그인 명령이 끝났다 — 성공이면 pollLogin 이 곧 계정 정보를 찾는다. 실패면 마지막 줄을 까닭으로 보이고 그만둔다
    private func loginEnded(_ status: Int32, slot: String) {
        guard login?.slot == slot else { return }
        loginProcess = nil
        loginPty?.readabilityHandler = nil
        loginPty = nil
        guard status != 0 else { loginNote = "로그인을 마쳤습니다. 계정 정보를 확인하는 중…"; return }
        // 실패 까닭은 코드 입력 안내와 같은 줄에 붙어 나온다("Paste code here if prompted > Login failed: …") — 안내를 떼고 마지막 줄
        let last = loginText.split(whereSeparator: \.isNewline)
            .map { line -> String in
                let t = line.components(separatedBy: "prompted >").last ?? String(line)
                return t.trimmingCharacters(in: .whitespaces)
            }
            .last { !$0.isEmpty } ?? ""
        cancelAdd()
        flash("계정을 추가하지 못했습니다" + (last.isEmpty ? "" : ": \(last.prefix(200))"), error: true)
    }

    // 브라우저가 보여 준 코드를 로그인 명령에 써 넣는다
    func submitLoginCode(_ code: String) {
        let c = code.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !c.isEmpty, let pty = loginPty else { return }
        pty.write(Data((c + "\r").utf8))
        loginNote = "코드를 보냈습니다. 확인하는 중…"
    }

    func cancelAdd() {
        guard let l = login else { return }
        login = nil
        tokenOnlySince = nil
        adding = false
        loginURL = nil
        loginNote = nil
        loginPty?.readabilityHandler = nil
        loginPty = nil
        loginProcess?.terminate()
        loginProcess = nil
        browserWait?.invalidate()
        browserLogin?.close()
        browserLogin = nil
        queue.async { Self.dropSlot(l.slot) }
    }

    private static func dropSlot(_ slot: String) {
        Keychain.delete(credentialsService(slot))
        try? FileManager.default.removeItem(atPath: slot)
    }

    // 로그인이 끝났는지 본다 — 그 설정 폴더에 계정 정보가, 키체인에 그 폴더 몫 토큰이 생기면 끝난 것
    private func pollLogin() {
        guard let l = login, !polling else { return }
        if Date().timeIntervalSince(l.started) > 15 * 60 {
            cancelAdd()
            return flash("로그인을 15분 동안 기다리다 그만뒀습니다", error: true)
        }
        polling = true
        queue.async { [weak self] in
            guard let self else { return }
            var result: Result<Account, Error>?
            var tokenOnly = false
            if let oa = ClaudeConfigFile(l.slot + "/.claude.json").oauthAccount(), let a = Account(oauth: oa),
               let oauth = Keychain.json(credentialsService(l.slot))?["claudeAiOauth"] as? [String: Any] {
                result = Result {
                    try storeAccount(oa, credentials: oauth, ports: self.ports)
                    // 지금 계정도 함께 저장해 둔다 — 목록에 나란히 보이고 언제든 돌아올 수 있게
                    try? self.ports.writeLock.withLock { try saveCurrentAccount(except: a.id, ports: self.ports) }
                    Self.dropSlot(l.slot)
                    return a
                }
            } else {
                tokenOnly = Keychain.exists(credentialsService(l.slot))
            }
            DispatchQueue.main.async {
                self.polling = false
                // 프로필을 받지 못하면 Claude Code 가 계정 정보를 남기지 않는다 — 30초 넘게 그러면 다시 해 보라고 알린다
                if tokenOnly, self.login?.slot == l.slot {
                    let since = self.tokenOnlySince ?? Date()
                    self.tokenOnlySince = since
                    if Date().timeIntervalSince(since) > 30 {
                        self.cancelAdd()
                        self.flash("로그인은 됐지만 계정 정보를 받지 못했습니다. 계정 추가를 다시 해 주세요", error: true)
                    }
                    return
                }
                guard let result, self.login?.slot == l.slot else { return }
                self.login = nil
                self.tokenOnlySince = nil
                self.adding = false
                self.loginURL = nil
                self.loginNote = nil
                self.loginPty = nil
                self.loginProcess?.terminate()   // 보통은 이미 끝났다
                self.loginProcess = nil
                self.browserWait?.invalidate()
                self.browserLogin?.close()       // 로그인 창은 완료 화면에 머물러 있다 — 닫는다
                self.browserLogin = nil
                self.loadSaved()
                switch result {
                case .success(let a): self.flash("\(a.email) 계정을 추가했습니다. 전환을 누르면 바로 바꿉니다")
                case .failure(let e): self.flash("계정을 추가하지 못했습니다: \(e.localizedDescription)", error: true)
                }
            }
        }
    }

    func remove(_ a: Account) {
        queue.async { [weak self] in
            guard let self else { return }
            // 다 했으면 nil, 토큰은 지웠지만 목록을 고치지 못했으면 그 안내, 키체인에서 못 지웠으면 던진다(목록은 그대로)
            let r = Result { try removeAccount(a.id, ports: self.ports) }
            DispatchQueue.main.async {
                self.loadSaved()
                switch r {
                case .success(nil): self.flash("\(a.email) 계정을 목록에서 지웠습니다")
                case .success(let warning?): self.flash("\(a.email): \(warning)", error: true)
                case .failure(let e): self.flash("\(a.email) 계정을 지우지 못했습니다: \(e.localizedDescription)", error: true)
                }
            }
        }
    }

    func flash(_ text: String, error: Bool = false) {
        withAnimation(.easeOut(duration: 0.2)) { notice = Notice(text: text, error: error) }
        noticeWork?.cancel()
        let w = DispatchWorkItem { [weak self] in withAnimation(.easeOut(duration: 0.3)) { self?.notice = nil } }
        noticeWork = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 7, execute: w)
    }
}
