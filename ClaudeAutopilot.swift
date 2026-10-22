// Claude Board 자율주행 엔진(실행 파일 claude-autopilot) — 보드 앱 밖에서 돈다
// 실행 상태의 주인은 엔진 하나다. 엔진이 죽으면 launchd 가 다시 띄우고(KeepAlive), 엔진은 상태 파일에서 이어 간다.
// 설계·파일 약속·실측: docs/자율주행/설계.md, docs/자율주행/진행 상황.md
//
// 사용법: claude-autopilot start <실행 폴더> [--dev]                 run.json·queue.json 확인 → 고정 사본 → LaunchAgent 올리기
//        claude-autopilot run <실행 폴더> <고정 설정 해시>            launchd 가 부르는 본체(해시는 start 가 LaunchAgent 인자로 넣는다)
//        claude-autopilot control <실행 폴더> pause|resume|stop      control.json 쓰기(보드도 같은 파일을 쓴다)
//        claude-autopilot status <실행 폴더>                        사람이 읽는 한 화면 요약
import Foundation
import Darwin
import CryptoKit

// MARK: - 공통 도구

let fm = FileManager.default
let home = NSHomeDirectory()
var gotTerm: sig_atomic_t = 0       // SIGTERM 을 받음 — 돌던 claude 를 끝내고 나간다
var finishing: sig_atomic_t = 0     // 정리 중 — 스스로 부른 launchctl bootout 의 SIGTERM 이면 바로 끝낸다

func now() -> Double { Date().timeIntervalSince1970 }
func r3(_ x: Double) -> Double { (x * 1000).rounded() / 1000 }
func num(_ v: Any?) -> Double? { (v as? NSNumber)?.doubleValue ?? (v as? String).flatMap { Double($0) } }

func readJSON(_ path: String) -> Any? {
    guard let d = fm.contents(atPath: path) else { return nil }
    return try? JSONSerialization.jsonObject(with: d)
}

func jsonText(_ obj: Any, pretty: Bool = false) -> String {
    var o: JSONSerialization.WritingOptions = [.withoutEscapingSlashes, .sortedKeys]
    if pretty { o.insert(.prettyPrinted) }
    guard let d = try? JSONSerialization.data(withJSONObject: obj, options: o) else { return "{}" }
    return String(decoding: d, as: UTF8.self)
}

// 읽는 쪽(보드)이 반쯤 쓴 파일을 보지 않게 임시 파일에 쓰고 바꿔 넣는다
func writeJSON(_ path: String, _ obj: Any) {
    let tmp = path + ".tmp-\(getpid())"
    guard (try? Data((jsonText(obj, pretty: true) + "\n").utf8).write(to: URL(fileURLWithPath: tmp))) != nil else { return }
    rename(tmp, path)
}

// 엔진만 읽는 파일(고정 설정·샌드박스 설정)은 처음부터 0600 으로 만들어 바꿔 넣는다
@discardableResult
func writePrivate(_ path: String, _ data: Data) -> Bool {
    let tmp = path + ".tmp-\(getpid())"
    let fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0o600)
    guard fd >= 0 else { return false }
    let ok = data.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) } == data.count
    close(fd)
    guard ok, rename(tmp, path) == 0 else { unlink(tmp); return false }
    return true
}

func sha256(_ d: Data) -> String { SHA256.hash(data: d).map { String(format: "%02x", $0) }.joined() }

func appendLine(_ path: String, _ line: String) {
    let fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0o644)
    guard fd >= 0 else { return }
    let bytes = Array((line.hasSuffix("\n") ? line : line + "\n").utf8)
    _ = bytes.withUnsafeBufferPointer { write(fd, $0.baseAddress, $0.count) }
    close(fd)
}

func fail(_ msg: String) -> Never {
    FileHandle.standardError.write(Data((msg + "\n").utf8))
    exit(2)
}

let stampFmt: DateFormatter = { let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd HH:mm:ss"; return f }()
let whenFmt: DateFormatter = { let f = DateFormatter(); f.locale = Locale(identifier: "ko_KR"); f.dateFormat = "M월 d일 HH:mm"; return f }()
let clockFmt: DateFormatter = { let f = DateFormatter(); f.dateFormat = "MM-dd HH:mm:ss"; return f }()
func when(_ t: Double) -> String { whenFmt.string(from: Date(timeIntervalSince1970: t)) }

// 엔진 기록(engine.log — launchd 가 표준 출력·오류를 여기로 보낸다)
func elog(_ s: String) {
    FileHandle.standardError.write(Data("[\(stampFmt.string(from: Date()))] \(s)\n".utf8))
}

func dur(_ sec: Double) -> String {
    let s = Int(sec.rounded())
    if s < 60 { return "\(s)초" }
    if s < 3600 { return s % 60 == 0 ? "\(s / 60)분" : "\(s / 60)분 \(s % 60)초" }
    return "\(s / 3600)시간 \((s % 3600) / 60)분"
}

func usd(_ x: Double) -> String { String(format: x < 1 ? "%.3f" : "%.2f", x) }

// 사람이 읽는 한 줄: 첫 줄만, 길면 자른다
func short(_ s: String, _ n: Int = 140) -> String {
    let line = s.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.first { !$0.isEmpty } ?? ""
    return line.count > n ? String(line.prefix(n)) + "…" : line
}

func isoDate(_ s: String?) -> Double? {
    guard var s else { return nil }
    let f = ISO8601DateFormatter()
    if let d = f.date(from: s) { return d.timeIntervalSince1970 }
    // 소수 초 자릿수가 많으면(…00.318890+00:00) 떼고 읽는다
    if let r = s.range(of: #"\.\d+"#, options: .regularExpression) { s.removeSubrange(r) }
    return f.date(from: s)?.timeIntervalSince1970
}

func selfPath() -> String {
    var size: UInt32 = 0
    _NSGetExecutablePath(nil, &size)
    var buf = [CChar](repeating: 0, count: Int(size) + 1)
    _NSGetExecutablePath(&buf, &size)
    let p = String(cString: buf)
    guard let real = realpath(p, nil) else { return p }
    defer { free(real) }
    return String(cString: real)
}

func canonical(_ path: String) -> String {
    let p = (path as NSString).expandingTildeInPath
    guard let real = realpath(p, nil) else { return URL(fileURLWithPath: p).standardizedFileURL.path }
    defer { free(real) }
    return String(cString: real)
}

func isDir(_ p: String) -> Bool { var d: ObjCBool = false; return fm.fileExists(atPath: p, isDirectory: &d) && d.boolValue }

// 짧은 명령(ps·git·launchctl·security)을 돌려 표준 출력을 받는다
@discardableResult
func runTool(_ path: String, _ args: [String], cwd: String? = nil, timeout: Double = 30) -> (code: Int32, out: String) {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: path)
    p.arguments = args
    if let cwd { p.currentDirectoryURL = URL(fileURLWithPath: cwd) }
    let pipe = Pipe()
    p.standardOutput = pipe
    p.standardError = FileHandle.nullDevice
    p.standardInput = FileHandle.nullDevice
    guard (try? p.run()) != nil else { return (-1, "") }
    var data = Data()
    let done = DispatchSemaphore(value: 0)
    DispatchQueue.global().async { data = pipe.fileHandleForReading.readDataToEndOfFile(); done.signal() }
    if done.wait(timeout: .now() + timeout) == .timedOut { p.terminate(); _ = done.wait(timeout: .now() + 2) }
    p.waitUntilExit()
    return (p.terminationStatus, String(decoding: data, as: UTF8.self))
}

func uid() -> String { String(getuid()) }

// 자율주행 폴더 뿌리: ~/.claude 는 Claude Code 의 보호 경로라 Write·Edit 도구가 단위 폴더에 못 쓴다(실측) — Application Support 아래에 둔다
func autopilotRoot(dev: Bool) -> String { home + "/Library/Application Support/" + (dev ? "Claude Board Dev" : "Claude Board") + "/autopilot" }
func privateRoot(dev: Bool) -> String { autopilotRoot(dev: dev) + "/private" }

// 엔진 자신이 부르는 git: 저장소 설정의 fsmonitor·hooks 를 쓰지 않고 시스템·전역 설정도 읽지 않는다
func safeGit(_ args: [String], cwd: String) -> String {
    runTool("/usr/bin/env", ["GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "/usr/bin/git", "--no-optional-locks",
                             "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"] + args, cwd: cwd).out
}
func plistPath(_ label: String) -> String { home + "/Library/LaunchAgents/\(label).plist" }
func launchdLoaded(_ label: String) -> Bool { runTool("/bin/launchctl", ["print", "gui/\(uid())/\(label)"]).code == 0 }
func commandOf(_ pid: pid_t) -> String { runTool("/bin/ps", ["-o", "command=", "-p", String(pid)]).out.trimmingCharacters(in: .whitespacesAndNewlines) }

// MARK: - 자식 프로세스

// posix_spawn 으로 띄운다 — process group 을 직접 정하려고(Foundation Process 는 정할 수 없다).
// claude 는 엔진과 같은 group 에 둔다: 엔진이 죽으면 launchd 가 같은 group 을 함께 끝낸다(실측). 판정 명령·셸 환경 읽기는 따로 group 을 만들어 통째로 끝낼 수 있게 한다
final class Child {
    let pid: pid_t
    private(set) var code: Int32?
    private var inFD: Int32 = -1
    private let lock = NSLock()
    private var lines: [String] = []
    private var raw = Data()          // rawOut 일 때 줄로 나누지 않고 모은 출력
    private var eof = true

    init?(_ path: String, _ args: [String], env: [String: String], cwd: String, stdinPipe: Bool = false,
          stdoutPipe: Bool = false, rawOut: Bool = false, outFile: String? = nil, errFile: String? = nil, newGroup: Bool = false) {
        var inP: [Int32] = [-1, -1], outP: [Int32] = [-1, -1]
        if stdinPipe, pipe(&inP) != 0 { return nil }
        if stdoutPipe, pipe(&outP) != 0 { return nil }
        var fa: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&fa)
        defer { posix_spawn_file_actions_destroy(&fa) }
        if stdinPipe { posix_spawn_file_actions_adddup2(&fa, inP[0], 0) }
        else { posix_spawn_file_actions_addopen(&fa, 0, "/dev/null", O_RDONLY, 0) }
        if stdoutPipe { posix_spawn_file_actions_adddup2(&fa, outP[1], 1) }
        else { posix_spawn_file_actions_addopen(&fa, 1, outFile ?? "/dev/null", O_WRONLY | O_CREAT | O_APPEND, 0o644) }
        posix_spawn_file_actions_addopen(&fa, 2, errFile ?? outFile ?? "/dev/null", O_WRONLY | O_CREAT | O_APPEND, 0o644)
        var attr: posix_spawnattr_t?
        posix_spawnattr_init(&attr)
        defer { posix_spawnattr_destroy(&attr) }
        // 엔진이 무시하는 SIGPIPE 등은 기본값으로 돌리고, 파일 설명자는 위에서 고른 0·1·2 만 넘긴다
        var flags = Int32(POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_CLOEXEC_DEFAULT)
        if newGroup { flags |= Int32(POSIX_SPAWN_SETPGROUP); posix_spawnattr_setpgroup(&attr, 0) }
        posix_spawnattr_setflags(&attr, Int16(flags))
        var all = sigset_t(); sigfillset(&all); posix_spawnattr_setsigdefault(&attr, &all)
        var none = sigset_t(); sigemptyset(&none); posix_spawnattr_setsigmask(&attr, &none)
        let argv: [UnsafeMutablePointer<CChar>?] = ([path] + args).map { strdup($0) } + [nil]
        let envp: [UnsafeMutablePointer<CChar>?] = env.map { strdup("\($0.key)=\($0.value)") } + [nil]
        defer { argv.forEach { free($0) }; envp.forEach { free($0) } }
        // 작업 폴더는 띄우는 동안만 바꾼다(posix_spawn 의 chdir 동작은 macOS 26 부터라 14 에서 못 쓴다). 엔진은 띄우기를 한 스레드에서만 한다
        let back = fm.currentDirectoryPath
        var p: pid_t = 0
        let rc = chdir(cwd) == 0 ? posix_spawn(&p, path, &fa, &attr, argv, envp) : errno
        chdir(back)
        for fd in [inP[0], outP[1]] where fd >= 0 { close(fd) }
        guard rc == 0 else {
            for fd in [inP[1], outP[0]] where fd >= 0 { close(fd) }
            return nil
        }
        pid = p
        if stdinPipe { inFD = inP[1]; _ = fcntl(inFD, F_SETFD, FD_CLOEXEC) }
        if stdoutPipe {
            let fd = outP[0]
            _ = fcntl(fd, F_SETFD, FD_CLOEXEC)
            eof = false
            Thread.detachNewThread { [self] in readLoop(fd, rawOut) }
        }
    }

    private func readLoop(_ fd: Int32, _ rawOut: Bool) {
        var buf = [UInt8](repeating: 0, count: 65536)
        var pending = [UInt8]()
        while true {
            let n = read(fd, &buf, buf.count)
            if n < 0, errno == EINTR { continue }
            if n <= 0 { break }
            if rawOut { lock.lock(); raw.append(contentsOf: buf[0..<n]); lock.unlock(); continue }
            pending.append(contentsOf: buf[0..<n])
            guard let last = pending.lastIndex(of: 0x0a) else { continue }
            let got = pending[..<last].split(separator: 0x0a, omittingEmptySubsequences: false).map { String(decoding: $0, as: UTF8.self) }
            pending.removeSubrange(...last)
            lock.lock(); lines.append(contentsOf: got); lock.unlock()
        }
        close(fd)
        lock.lock()
        if !pending.isEmpty { lines.append(String(decoding: pending, as: UTF8.self)) }
        eof = true
        lock.unlock()
    }

    func drain() -> [String] { lock.lock(); defer { lock.unlock() }; let l = lines; lines = []; return l }
    var rawData: Data { lock.lock(); defer { lock.unlock() }; return raw }
    var atEOF: Bool { lock.lock(); defer { lock.unlock() }; return eof && lines.isEmpty }

    @discardableResult
    func send(_ s: String) -> Bool {
        guard inFD >= 0 else { return false }
        var bytes = Array(s.utf8)[...]
        while !bytes.isEmpty {
            let n = bytes.withUnsafeBufferPointer { write(inFD, $0.baseAddress, $0.count) }
            if n < 0, errno == EINTR { continue }
            if n <= 0 { return false }
            bytes = bytes.dropFirst(n)
        }
        return true
    }

    func closeInput() { if inFD >= 0 { close(inFD); inFD = -1 } }

    // 끝났으면 종료 코드(신호로 끝나면 128+신호), 아직이면 nil
    @discardableResult
    func poll() -> Int32? {
        if let code { return code }
        var st: Int32 = 0
        let r = waitpid(pid, &st, WNOHANG)
        if r == pid { code = (st & 0x7f) == 0 ? (st >> 8) & 0xff : 128 + (st & 0x7f) }
        else if r < 0, errno != EINTR { code = -1 }
        return code
    }

    func signal(_ s: Int32) { if poll() == nil { kill(pid, s) } }

    // 끝날 때까지 기다린다(그동안 tick 을 부른다). 시간 안에 안 끝나면 false
    func wait(_ sec: Double, tick: () -> Void = {}) -> Bool {
        let end = now() + sec
        while poll() == nil {
            if now() >= end { return false }
            tick()
            usleep(100_000)
        }
        return true
    }
}

// 따로 group 을 만든 자식(판정 명령, claude 의 도구 명령)을 통째로 끝낸다
func killGroup(_ pgid: pid_t) {
    guard pgid > 1, pgid != getpgrp(), killpg(pgid, 0) == 0 else { return }
    killpg(pgid, SIGTERM)
    for _ in 0..<20 where killpg(pgid, 0) == 0 { usleep(100_000) }
    if killpg(pgid, 0) == 0 { killpg(pgid, SIGKILL) }
}

// 프로세스 표(pid, ppid, pgid, 명령)
func processTable() -> [(pid: pid_t, ppid: pid_t, pgid: pid_t, cmd: String)] {
    runTool("/bin/ps", ["-A", "-o", "pid=,ppid=,pgid=,command="]).out.split(separator: "\n").compactMap { line in
        let f = line.split(separator: " ", maxSplits: 3, omittingEmptySubsequences: true)
        guard f.count >= 3, let a = pid_t(f[0]), let b = pid_t(f[1]), let c = pid_t(f[2]) else { return nil }
        return (a, b, c, f.count > 3 ? String(f[3]) : "")
    }
}

// MARK: - 로그인 셸 환경

// 엔진이 뜰 때 한 번 사용자의 로그인 + 대화형 셸 환경을 읽는다(`$SHELL -i -l`, 보드 3.0 의 LoginEnvironment 와 같은 방식).
// `-l` 만으로는 이 Mac 에서 ~/.local/bin(claude)이 .zshrc 에만 있어 못 찾았고(실측), claude 를 셸로 매번 띄우면 .zshrc 의 출력이 stream 에 섞인다.
// 셸은 깨끗한 환경에서 띄워 엔진이 받은 환경(launchd, 손으로 띄운 터미널의 세션 변수)이 새지 않게 한다.
// 읽은 환경에는 사용자 셸의 비밀 변수(토큰 등)가 들어 있으므로 파일에 쓰지 않고 파이프로만 받으며, claude 에는 passEnv 로 고른 것만 넘긴다
func loginShell() -> String {
    if let s = ProcessInfo.processInfo.environment["SHELL"], !s.isEmpty { return s }
    if let pw = getpwuid(getuid()), let sh = pw.pointee.pw_shell { return String(cString: sh) }
    return "/bin/zsh"
}

func captureLoginEnv(_ shell: String) -> [String: String]? {
    let pe = ProcessInfo.processInfo.environment
    var base = ["HOME": home, "SHELL": shell, "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                "USER": pe["USER"] ?? NSUserName(), "LOGNAME": pe["LOGNAME"] ?? NSUserName(),
                "TMPDIR": pe["TMPDIR"] ?? NSTemporaryDirectory()]
    if let l = pe["LANG"] { base["LANG"] = l }
    let marker = "__CLAUDE_BOARD_LOGIN_ENV__"
    guard let c = Child(shell, ["-i", "-l", "-c", "printf '\\n\(marker)\\n'; /usr/bin/env -0"], env: base, cwd: home,
                        stdoutPipe: true, rawOut: true, errFile: "/dev/null", newGroup: true) else { return nil }
    if !c.wait(15) { killGroup(c.pid); _ = c.wait(2); return nil }
    for _ in 0..<20 where !c.atEOF { usleep(50_000) }   // 셸이 끝난 뒤 파이프에 남은 출력까지
    let data = c.rawData
    guard c.code == 0, let r = data.range(of: Data("\n\(marker)\n".utf8)) else { return nil }
    var env: [String: String] = [:]
    for item in data[r.upperBound...].split(separator: 0) {
        let s = String(decoding: item, as: UTF8.self)
        guard let eq = s.firstIndex(of: "="), eq != s.startIndex else { continue }
        env[String(s[..<eq])] = String(s[s.index(after: eq)...])
    }
    for k in ["SHLVL", "PWD", "OLDPWD", "_"] { env.removeValue(forKey: k) }
    if env["LANG"] == nil, env["LC_ALL"] == nil, env["LC_CTYPE"] == nil { env["LANG"] = "ko_KR.UTF-8" }
    return env["PATH"] == nil ? nil : env
}

func findExecutable(_ name: String, _ env: [String: String]) -> String? {
    let dirs = (env["PATH"] ?? "").split(separator: ":").map(String.init) + [home + "/.local/bin"]
    for d in dirs {
        let p = d + "/" + name
        if access(p, X_OK) == 0, !isDir(p) { return p }
    }
    return nil
}

// claude·판정 명령에 넘길 환경 변수만 고른다. 이름에 비밀 낱말이 든 변수는 고른 목록에 있어도 env_pass 에 이름이 적힌 것만 넘긴다.
// claude 자신의 로그인은 키체인(OAuth)이라 환경 변수가 필요 없다(init 의 apiKeySource "none", 실측). API 키로 쓰는 사람은 env_pass 에 ANTHROPIC_API_KEY 를 적는다
let envKeep: Set<String> = ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "TMPDIR", "TERM", "JAVA_HOME",
                            "DOCKER_HOST", "DOCKER_CONFIG", "DOCKER_CONTEXT", "DOCKER_CERT_PATH", "DOCKER_TLS_VERIFY",
                            "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "no_proxy",
                            "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "CLAUDE_CONFIG_DIR"]
let envKeepPrefixes = ["LC_", "GRADLE_", "SDKMAN_"]
let secretName = try! NSRegularExpression(pattern: "TOKEN|SECRET|PASSW|CREDENTIAL|PRIVATE|API_?KEY|(^|_)KEY($|_)|AUTH", options: [.caseInsensitive])

func passEnv(_ env: [String: String], extra: [String]) -> [String: String] {
    env.filter { k, _ in
        if extra.contains(k) { return true }
        let r = NSRange(k.startIndex..., in: k)
        if secretName.firstMatch(in: k, range: r) != nil { return false }
        return envKeep.contains(k) || envKeepPrefixes.contains { k.hasPrefix($0) }
    }
}

// MARK: - 사용률

// 계정 사용률(0~1)과 초기화 시각. stream 의 rate_limit_event 를 주로 쓰고, 오래됐으면 보드·상태줄이 함께 쓰는 캐시, 그다음 사용량 API
let usageCache = "/tmp/.claude-usage-cache.json"

func usageFrom(apiJSON j: [String: Any], from: String, at: Double) -> [String: Any]? {
    guard let five = j["five_hour"] as? [String: Any] else { return nil }
    var u: [String: Any] = ["at": r3(at), "from": from]
    // 캐시·API 는 퍼센트(84.0), stream 은 비율(0.84) — 비율로 맞춘다
    if let v = num(five["utilization"]) { u["five_hour"] = v / 100 }
    if let r = isoDate(five["resets_at"] as? String) { u["five_hour_resets"] = r }
    if let seven = j["seven_day"] as? [String: Any] {
        if let v = num(seven["utilization"]) { u["seven_day"] = v / 100 }
        if let r = isoDate(seven["resets_at"] as? String) { u["seven_day_resets"] = r }
    }
    return u
}

func usageFromCache() -> [String: Any]? {
    guard let m = (try? fm.attributesOfItem(atPath: usageCache))?[.modificationDate] as? Date,
          now() - m.timeIntervalSince1970 < 900,
          let j = readJSON(usageCache) as? [String: Any] else { return nil }
    return usageFrom(apiJSON: j, from: "cache", at: m.timeIntervalSince1970)
}

// 키체인의 Claude Code 로그인 토큰으로 사용량 API 를 부른다(보드 UsageStore 와 같은 방식). 토큰은 메모리에만 둔다
func usageFromAPI() -> [String: Any]? {
    let user = ProcessInfo.processInfo.environment["USER"] ?? NSUserName()
    let account = user.range(of: "^[a-zA-Z0-9._-]+$", options: .regularExpression) != nil ? user : "claude-code-user"
    let r = runTool("/usr/bin/security", ["find-generic-password", "-a", account, "-s", "Claude Code-credentials", "-w"], timeout: 10)
    guard r.code == 0 else { return nil }
    var raw = r.out.trimmingCharacters(in: .whitespacesAndNewlines)
    if !raw.hasPrefix("{"), raw.count % 2 == 0, raw.allSatisfy(\.isHexDigit) {   // ASCII 밖 바이트가 섞이면 16진수로 나온다
        let b = Array(raw.utf8)
        raw = String(decoding: stride(from: 0, to: b.count, by: 2).compactMap { UInt8(String(decoding: b[$0..<$0 + 2], as: UTF8.self), radix: 16) }, as: UTF8.self)
    }
    guard let j = try? JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
          let oauth = j["claudeAiOauth"] as? [String: Any], let token = oauth["accessToken"] as? String else { return nil }
    if let exp = num(oauth["expiresAt"]), exp / 1000 < now() { return nil }
    var req = URLRequest(url: URL(string: "https://api.anthropic.com/api/oauth/usage")!, timeoutInterval: 10)
    req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    req.setValue("oauth-2025-04-20", forHTTPHeaderField: "anthropic-beta")
    let sem = DispatchSemaphore(value: 0)
    var body: Data?
    URLSession.shared.dataTask(with: req) { d, resp, _ in
        if (resp as? HTTPURLResponse)?.statusCode == 200 { body = d }
        sem.signal()
    }.resume()
    _ = sem.wait(timeout: .now() + 12)
    guard let body, let j2 = try? JSONSerialization.jsonObject(with: body) as? [String: Any],
          let u = usageFrom(apiJSON: j2, from: "api", at: now()) else { return nil }
    try? body.write(to: URL(fileURLWithPath: usageCache), options: .atomic)   // 보드·상태줄과 함께 쓰는 캐시
    return u
}

// MARK: - 모델에게 주는 글

let systemPromptText = """
이 세션은 Claude Board 자율주행 모드로 돈다.
- 사람에게 묻지 않는다. 사람 몫(되돌리기 어려운 바깥 반영, 보안과 책임, 운영 값과 용량, 자율주행 자신의 규칙·시험·종료 조건, 사람이 손댄 영역)에 걸리는 부분은 하지 않고 result.json 의 next_start 에 적는다. 걸리지 않으면 근거 문서와 대조한 권고안을 채택하고 decisions_applied 에 적는다.
- 허용 범위 밖의 도구는 묻지 않고 거부된다. 거부되면 다른 길로 하고, 길이 없으면 그 부분만 보류해 next_start 에 적는다.
- CLAUDE.md 와 .claude/ 아래 파일을 고치지 않는다. git push 와 Jira·Confluence 쓰기를 하지 않는다.
- 판정은 엔진이 판정 명령의 종료 코드로 한다. 판정 명령이나 시험 기준을 낮추거나 지우지 않는다.
- 사람이 읽는 글(답, Bash 도구의 description, result.json)은 모두 쉬운 한국어로 쓴다.
- 홈 폴더 전체를 뒤지는 검색(grep -r ~, find ~ 등)은 하지 않는다. 근거는 단위 글이 적은 경로와 작업 폴더에서 찾는다.
- 백그라운드로 띄운 명령은 턴을 끝내기 전에 끝낸다. 결과를 보낸 뒤 백그라운드 작업만 남아 오래 조용하면 엔진이 그 작업을 끝내고 넘어간다.
- 일을 마치면 $CLAUDE_BOARD_UNIT_DIR/result.json 을 쓰고 턴을 끝낸다.
"""

let resultFormat = """
{
  "unit": {"title": "쉬운 말 제목", "kind": "story|dev|fix|env", "ref": ""},
  "outcome": "done|failed|parked|capped",
  "headline": "한 문장 결론",
  "risks": ["알아 두실 위험(확인 요청이 아님)"],
  "summary": ["한 일 한 줄씩"],
  "tests": [{"name": "시험 이름(쉬운 말)", "result": "pass|fail|skip", "note": ""}],
  "changes": [{"what": "바꾼 것", "where": "저장소 경로"}],
  "decisions_applied": [{"what": "따른 결정", "basis": "근거(출발 카드, 근거 문서 절 등)"}],
  "sequence": [{"title": "흐름 그림 제목", "mermaid": "sequenceDiagram ..."}],
  "next_start": [{"title": "사람 몫이라 보류한 것", "why": "이유"}],
  "new_work": [{"title": "새로 찾은 일(다음 출발 대기열 후보)", "why": "이유"}],
  "next": "다음에 할 일 한 문장",
  "refs": {"jira": "", "ssot": "", "commit": ""}
}
"""

let msgPaused = "일시정지됐던 작업을 이어서 하세요. 이미 끝낸 일은 다시 하지 말고, 멈춘 자리의 상태(파일, git)를 확인한 뒤 계속하세요."
let msgLimit = "사용량 한도로 끊겼던 작업을 이어서 하세요. 이미 끝낸 일은 다시 하지 말고, 멈춘 자리의 상태(파일, git)를 확인한 뒤 계속하세요."
let msgRestart = "엔진이 다시 떠서 이어 갑니다. 하던 일이 중간에 끊겼을 수 있으니, 이미 끝낸 일은 다시 하지 말고 멈춘 자리의 상태(파일, git, 돌던 명령)를 확인한 뒤 이어서 하세요."
let msgError = "오류로 끊겼던 작업을 이어서 하세요. 이미 끝낸 일은 다시 하지 말고, 멈춘 자리의 상태(파일, git)를 확인한 뒤 계속하세요."

// 허용 목록 기본값(run.json allowed_tools 로 바꿀 수 있다). 목록 밖은 dontAsk 로 묻지 않고 거부된다.
// 실행 단위에는 웹 도구를 주지 않는다 — Bash 와 웹 읽기가 함께 있으면 읽은 글의 지시로 비밀이 새어 나갈 수 있다(웹은 주제로 시작의 초안 세션만).
// 파일 쓰기 도구는 여기 두지 않고 엔진이 쓰기 범위마다 `Edit(/<경로>/**)` 로 붙인다 — 경로 없는 Write 는 홈 아무 곳에나 쓸 수 있었다(실측)
let defaultAllowed = ["Read", "Glob", "Grep", "Bash", "Task", "TaskStop", "SendMessage", "Skill", "ToolSearch", "Monitor", "TodoWrite"]
let fileWriteTools: Set<String> = ["Write", "Edit", "MultiEdit", "NotebookEdit"]
// 늘 붙이는 거부 목록 — 명령줄로 넘겨 세션이 지울 수 없다. Edit 규칙은 Write 도구도 막는다(실측).
// 이 규칙은 파일 도구만 막는다. Bash 는 샌드박스 denyWrite(sandboxSettings)와 네트워크 허용 목록이 막는다
let alwaysDenied = ["Edit(**/CLAUDE.md)", "Edit(~/.claude/CLAUDE.md)", "Edit(.claude/**)", "Edit(**/.claude/rules/**)",
                    "Edit(~/.claude/settings.json)", "Edit(~/.claude/settings.local.json)", "Edit(~/Library/LaunchAgents/**)",
                    "Bash(git push)", "Bash(git push *)", "Bash(git -C * push *)",
                    "mcp__atlassian", "mcp__plugin_atlassian_atlassian", "mcp__claude_ai_Atlassian"]
// 네트워크 허용 목록 기본값(run.json network_allow 로 바꿈): Maven·Gradle 저장소와 localhost. git 원격 호스트는 넣지 않아 push 가 나갈 길이 없다
// localhost 는 넣지 않는다 — 넣고 allowLocalBinding 을 켜면 로컬 DB(5432)·Redis 에도 붙는다. 단위가 정말 필요하면 network_allow 에
// "localhost:<포트>" 로 연다(프록시를 거치는 HTTP 만, NO_PROXY 를 비워 부를 때 — 실측). DB 같은 비 HTTP 는 local_binding(모든 포트)뿐
let defaultNetwork = ["repo.maven.apache.org", "repo1.maven.org", "plugins.gradle.org", "services.gradle.org", "downloads.gradle.org"]
// 쓰기 범위 안에서도 Bash 가 고치지 못하게 할 하네스 자리(샌드박스 denyWrite). `.claude` 를 통째로 막으면 `.claude/worktrees/` 의 git 작업 트리를 못 만든다
let harnessEntries = ["CLAUDE.md", ".mcp.json", ".claude/CLAUDE.md", ".claude/settings.json", ".claude/settings.local.json", ".claude/rules",
                      ".claude/skills", ".claude/agents", ".claude/commands", ".claude/hooks", ".claude/workflows", ".claude/output-styles",
                      // git 이 commit 에 쓰지 않는 설정 자리 — 여기 core.fsmonitor·hooksPath 를 심으면 엔진이 밖에서 도는 git 이 실행할 수 있다.
                      // objects·refs·index·HEAD·logs 와 .git/worktrees/<이름>/ 의 commondir·gitdir 은 commit·worktree add 에 필요해 남긴다
                      ".git/hooks", ".git/config", ".git/config.worktree", ".git/info", ".git/commondir", ".git/objects/info/alternates",
                      ".git/worktrees/*/config.worktree",
                      ".git/modules/*/hooks", ".git/modules/*/config", ".git/modules/*/config.worktree", ".git/modules/*/info",
                      ".git/modules/*/objects/info/alternates"]

// MARK: - 엔진

enum Stop { case pause, stop }
enum TurnEnd { case finished, limit(Double?), capped(String), paused, stopped, failed(String) }
enum CheckEnd { case passed, failed(Int32, String), capped(String), paused, stopped }
enum WaitEnd { case elapsed, paused, stopped }

let stateLabel = ["starting": "준비 중", "running": "진행 중", "waiting_limit": "한도 대기", "waiting_stopline": "멈출 선 대기",
                  "waiting_precheck": "시험 환경 대기",
                  "paused": "일시정지", "stopped": "중단됨", "done": "끝남", "error": "엔진 오류로 멈춤"]
let unitLabel = ["pending": "대기", "running": "진행 중", "done": "통과", "failed": "미통과", "capped": "상한에서 멈춤", "stopped": "중단"]
let capLabel = ["minutes": "시간", "usd": "비용", "weekly": "주간 사용률"]

final class Engine {
    let dir: String
    let id: String
    var run: [String: Any] = [:]        // 상태(엔진만 씀). 설정 값은 여기서 읽지 않는다
    var queue: [[String: Any]] = []     // 단위 상태(엔진만 씀). 단위 정의는 defs 에서 읽는다
    var cfg: [String: Any] = [:]        // 출발 때 고정한 실행 설정(pinned/frozen.json)
    var defs: [[String: Any]] = []      // 출발 때 고정한 단위 정의(prompt·check·precheck·cwd·caps…)
    var loginEnv: [String: String] = [:]
    var shell = "/bin/zsh"
    var claude = ""
    var crashResume = false     // 비정상 종료 뒤 다시 떴다 — 하던 단위의 claude 는 "다시 떠서 이어 갑니다"로 잇는다
    var lastActivitySave = 0.0

    init(_ dir: String) { self.dir = dir; id = (dir as NSString).lastPathComponent }

    var runPath: String { dir + "/run.json" }
    var queuePath: String { dir + "/queue.json" }
    var controlPath: String { dir + "/control.json" }
    var privateDir: String { dir + "/engine" }     // 엔진만 쓰고 읽는 상태(모델의 쓰기 범위 밖)
    func unitDir(_ n: Int) -> String { dir + "/units/\(n)" }
    func unitPath(_ n: Int) -> String { privateDir + "/unit-\(n).json" }
    var state: String { run["state"] as? String ?? "" }

    func load() -> Bool {
        guard let r = readJSON(runPath) as? [String: Any], let q = readJSON(queuePath) as? [[String: Any]] else { return false }
        run = r; queue = q
        return true
    }

    // 고정한 설정을 읽고 출발 때 해시(LaunchAgent 인자)와 견준다. 다르면 nil — 누가 실행 중에 바꿨다
    func loadFrozen(_ sha: String) -> Bool {
        guard let data = fm.contents(atPath: dir + "/pinned/frozen.json"), sha256(data) == sha,
              let f = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let u = f["units"] as? [[String: Any]], u.count == queue.count else { return false }
        cfg = f; defs = u
        return true
    }
    // run.json·queue.json 의 설정 칸을 고정 값으로 되돌려 쓴다 — 누가 바꿔 놓았어도 보드가 그리는 값이 엔진이 쓰는 값과 같게
    static let settingKeys = ["title", "model", "effort", "stopline", "caps", "check_timeout_min", "check_retries", "precheck_retry_min", "background_quiet_sec",
                              "allowed_tools", "disallowed_tools", "write_roots", "network_allow", "local_binding", "env_pass", "unix_sockets",
                              "read_deny", "sandbox_exclude", "claude_args", "claude_path"]
    static let unitKeys = ["id", "title", "prompt", "check", "precheck", "action", "context", "cwd", "caps", "check_timeout_min", "action_timeout_min",
                           "tickets", "review"]
    func syncFromFrozen() {
        for k in Engine.settingKeys { run[k] = cfg[k] }
        for i in queue.indices { for k in Engine.unitKeys { queue[i][k] = defs[i][k] } }
        saveRun(); saveQueue()
    }
    func saveRun() { run["updated"] = r3(now()); writeJSON(runPath, run) }
    func saveQueue() { writeJSON(queuePath, queue) }
    func loadUnit(_ n: Int) -> [String: Any] { readJSON(unitPath(n)) as? [String: Any] ?? [:] }
    // 엔진은 비공개 사본만 읽는다. 단위 폴더의 engine.json 은 보드가 그리라고 내놓는 사본이다
    func saveUnit(_ n: Int, _ u: [String: Any]) {
        var u = u; u["updated"] = r3(now())
        writeJSON(unitPath(n), u)
        if isDir(unitDir(n)) { writeJSON(unitDir(n) + "/engine.json", u) }
    }

    func event(_ who: String, _ text: String) {
        appendLine(dir + "/events.jsonl", jsonText(["ts": r3(now()), "who": who, "text": text]))
        elog("\(who): \(text)")
        run["last_activity"] = r3(now())
    }
    func context(_ n: Int, _ o: [String: Any]) {
        var o = o; o["ts"] = r3(now()); o["unit"] = n
        appendLine(dir + "/context.jsonl", jsonText(o))
    }
    func setState(_ s: String, _ reason: String, until: Double? = nil) {
        run["state"] = s; run["reason"] = reason
        run["wait_until"] = until.map { r3($0) } ?? NSNull()
        saveRun()
    }

    var model: String { cfg["model"] as? String ?? "" }
    var total: Int { queue.count }
    func def(_ n: Int) -> [String: Any] { defs[n - 1] }
    func title(_ n: Int) -> String { def(n)["title"] as? String ?? "단위 \(n)" }
    func cwd(_ n: Int) -> String { def(n)["cwd"] as? String ?? home }
    var writeRoots: [String] { cfg["write_roots"] as? [String] ?? [] }
    var readDenyPaths: [String] { cfg["read_deny"] as? [String] ?? [] }
    // 모델이 읽지 못하게 하는 엔진 쪽 자리: 엔진 전용 뿌리 전체(실제 판·시험 판 — 다른 실행의 복원 전 백업도), 모든 실행의 engine 폴더,
    // 옛 뿌리 ~/.claude/notifier/autopilot(Claude Code 는 ~/.claude 쓰기만 막고 읽기는 막지 않는다, 실측). 폴더 자리 글롭은 샌드박스·Read 규칙 모두 된다(실측)
    var engineDeny: [String] {
        var a = [privateRoot(dev: false), privateRoot(dev: true)] + [false, true].map { autopilotRoot(dev: $0) + "/runs/*/engine" }
        let old = home + "/.claude/notifier/autopilot"
        a.append(dir.hasPrefix(old + "/") ? old + "/runs/*/engine" : old)   // 옛 뿌리에서 도는 실행이면 자기 폴더는 읽어야 한다
        return a
    }
    // 엔진 전용 폴더: 실행 폴더 밖, 0700, 샌드박스·파일 도구 모두 읽기 금지. 복원 전 백업처럼 비밀이 든 것을 둔다
    var enginePrivate: String { privateRoot(dev: cfg["dev"] as? Bool ?? false) + "/" + id }
    var envPass: [String] { cfg["env_pass"] as? [String] ?? [] }

    func caps(_ n: Int) -> [String: Double] {
        let d = cfg["caps"] as? [String: Any] ?? [:]
        let c = def(n)["caps"] as? [String: Any] ?? [:]
        func v(_ k: String, _ def: Double) -> Double { num(c[k]) ?? num(d[k]) ?? def }
        return ["minutes": v("minutes", 120), "usd": v("usd", 100), "weekly_pp": v("weekly_pp", 5)]
    }

    // MARK: control.json

    // 새 요청이면 action 을 돌려준다. 받은 것은 run.json 에 남기고 파일은 지운다 — 다시 뜬 엔진이 같은 요청을 또 하지 않게
    func readControl() -> String? {
        guard fm.fileExists(atPath: controlPath) else { return nil }
        let c = readJSON(controlPath) as? [String: Any]
        let m = ((try? fm.attributesOfItem(atPath: controlPath))?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? now()
        try? fm.removeItem(atPath: controlPath)
        guard let a = c?["action"] as? String else { return nil }
        let ts = num(c?["ts"]) ?? m
        guard ts > num(run["control_seen"]) ?? 0 else { return nil }
        run["control_seen"] = ts; run["control_last"] = a
        saveRun()
        return a
    }

    // 도는 동안 1초마다 부른다: 일시정지·중단이면 그 값을, 아니면 nil
    func pollControl() -> Stop? {
        if gotTerm != 0 { terminated() }
        guard let a = readControl() else { return nil }
        switch a {
        case "pause": event("board", "일시정지 요청을 받았습니다."); return .pause
        case "stop": event("board", "중단 요청을 받았습니다."); return .stop
        case "resume": event("board", "재개 요청을 받았지만 일시정지 상태가 아니라 그대로 갑니다.")
        default: event("board", "아직 엔진이 하지 못하는 요청(\(a))이라 넘깁니다.")
        }
        return nil
    }

    // 누가 엔진을 끄면(SIGTERM) claude 는 같은 group 이라 함께 끝난다. 상태는 그대로 두어 다시 뜨면 잇는다
    func terminated() -> Never {
        elog("SIGTERM 을 받아 끝냅니다")
        saveRun()
        exit(1)
    }

    // MARK: 시작과 끝

    func main() -> Never {
        let prev = state
        if ["stopped", "done", "error"].contains(prev) {   // 끝난 실행인데 다시 떴다(정리 도중 끊김) — 정리만 다시 한다
            cleanupAndExit()
        }
        let first = prev == "starting" || prev.isEmpty
        let abnormal = !first
        try? fm.createDirectory(atPath: privateDir, withIntermediateDirectories: true)
        chmod(privateDir, 0o700)
        try? fm.createDirectory(atPath: enginePrivate, withIntermediateDirectories: true)
        chmod((enginePrivate as NSString).deletingLastPathComponent, 0o700); chmod(enginePrivate, 0o700)
        let t = now()
        appendLine(dir + "/starts.log", [String(Int(t)), stampFmt.string(from: Date()), abnormal ? "abnormal" : "normal",
                                         "pid=\(getpid())", "prev=\(prev)", "exe=\(selfPath())"].joined(separator: "\t"))
        if abnormal {
            let recent = ((try? String(contentsOfFile: dir + "/starts.log", encoding: .utf8)) ?? "").split(separator: "\n").filter {
                let f = $0.split(separator: "\t")
                return f.count > 2 && f[2] == "abnormal" && (Double(f[0]) ?? 0) >= t - 600
            }.count
            let n = num(run["unit"]).map(Int.init)
            if let n, n >= 1, n <= total { killLeftovers(n) }
            if recent >= 3 {
                event("engine", "엔진 오류로 멈춥니다: 10분 안에 세 번 비정상으로 다시 떴습니다.")
                if let n, n >= 1, n <= total, queue[n - 1]["status"] as? String == "running" {
                    endUnit(n, "stopped", "엔진 오류로 멈췄습니다.")
                }
                finish("error", "엔진 오류로 멈춤: 10분 안에 세 번 비정상으로 다시 떴습니다.")
            }
        }
        shell = loginShell()
        if let e = captureLoginEnv(shell) { loginEnv = e } else {
            loginEnv = ["HOME": home, "SHELL": shell, "LANG": "ko_KR.UTF-8",
                        "USER": NSUserName(), "LOGNAME": NSUserName(), "TMPDIR": NSTemporaryDirectory(),
                        "PATH": "\(home)/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"]
            event("engine", "로그인 셸 환경을 읽지 못해 기본 환경으로 띄웁니다.")
        }
        // claude_path 는 시험용(한도 거절을 흉내 내는 가짜 claude 등) — --dev 로 출발한 실행만 고정 설정에 들어 있다
        // 샌드박스 안에서는 /usr/bin/java 가 JDK 를 못 찾는다("Unable to locate a Java Runtime", 실측) — JAVA_HOME 이 없으면 밖에서 찾아 넣는다
        if loginEnv["JAVA_HOME"] == nil {
            let jh = runTool("/usr/libexec/java_home", [], timeout: 10)
            let p = jh.out.trimmingCharacters(in: .whitespacesAndNewlines)
            if jh.code == 0, isDir(p) { loginEnv["JAVA_HOME"] = p }
        }
        guard let c = (cfg["claude_path"] as? String).flatMap({ access($0, X_OK) == 0 ? $0 : nil }) ?? findExecutable("claude", loginEnv) else {
            event("engine", "claude 를 찾지 못해 멈춥니다(로그인 셸의 PATH 에 없음).")
            finish("error", "claude 를 찾지 못했습니다.")
        }
        claude = c
        run["engine_pid"] = Int(getpid())
        if first {
            run["started"] = r3(t)
            event("engine", "자율주행을 시작합니다. 단위 \(total)개를 차례로 합니다(모델 \(model)).")
            setState("running", "단위를 시작하는 중입니다.")
        } else if prev == "paused" {
            event("engine", "엔진이 다시 떴습니다. 사람이 일시정지한 실행이라 그대로 쉽니다.")
            saveRun()
        } else {
            crashResume = true
            event("engine", "엔진이 다시 떴습니다. 직전에 비정상으로 끝나 하던 자리에서 잇습니다.")
            saveRun()
        }
        while true {
            if state == "paused" { pausedLoop(); continue }
            guard let n = queue.firstIndex(where: { $0["status"] as? String == "running" }).map({ $0 + 1 })
                    ?? queue.firstIndex(where: { ($0["status"] as? String ?? "pending") == "pending" }).map({ $0 + 1 }) else {
                let c = { (s: String) in self.queue.filter { $0["status"] as? String == s }.count }
                event("engine", "자율주행을 마쳤습니다. 통과 \(c("done")), 미통과 \(c("failed")), 상한에서 멈춤 \(c("capped")).")
                finish("done", "대기열을 모두 마쳤습니다.")
            }
            if queue[n - 1]["status"] as? String != "running" {
                guard stoplineGate(), precheckGate(n) else { continue }
                beginUnit(n)
            }
            runUnit(n)
        }
    }

    // 고정 설정이 출발 때와 다르다(해시가 다르거나 파일이 없음): 실행 중에 누가 바꿨다. 설정을 믿을 수 없으니 아무것도 띄우지 않고 멈춘다
    func frozenChanged() -> Never {
        event("engine", "고정한 실행 설정이 출발 때와 달라 멈춥니다. 실행 중에 설정 파일이 바뀌었을 수 있습니다.")
        if let i = queue.firstIndex(where: { $0["status"] as? String == "running" }) {
            killLeftovers(i + 1)   // 직전 엔진이 남긴 claude·도구 명령(설정 없이도 비공개 상태로 안다)
            queue[i]["status"] = "stopped"; saveQueue()
        }
        finish("error", "고정한 실행 설정이 출발 때와 달라 멈췄습니다.")
    }

    func finish(_ s: String, _ reason: String) -> Never {
        run["ended"] = r3(now())
        setState(s, reason)
        cleanupAndExit()
    }

    // plist 를 지우고 마지막에 launchctl bootout — launchd 가 이 엔진에 SIGTERM 을 보내므로 기록은 모두 끝낸 뒤에 한다
    func cleanupAndExit() -> Never {
        guard let label = run["launchd_label"] as? String ?? launchdLabelFromEnv() else { exit(0) }
        try? fm.removeItem(atPath: plistPath(label))
        finishing = 1
        if let c = Child("/bin/launchctl", ["bootout", "gui/\(uid())/\(label)"], env: ["PATH": "/usr/bin:/bin"], cwd: "/", newGroup: true) {
            _ = c.wait(30)
        }
        exit(0)   // launchd 에서 돌지 않았거나(손으로 run) 이미 내려간 경우
    }

    func launchdLabelFromEnv() -> String? {
        guard let l = ProcessInfo.processInfo.environment["XPC_SERVICE_NAME"], l.hasPrefix("com.claude.board"), l.contains(".autopilot.") else { return nil }
        return l
    }

    func pausedLoop() {
        while true {
            if gotTerm != 0 { terminated() }
            if let a = readControl() {
                switch a {
                case "resume":
                    event("board", "재개합니다. 멈춘 자리에서 같은 세션으로 이어 갑니다.")
                    setState("running", "재개했습니다.")
                    return
                case "stop": stopRun()
                case "pause": event("board", "이미 일시정지 상태입니다.")
                default: event("board", "아직 엔진이 하지 못하는 요청(\(a))이라 넘깁니다.")
                }
            }
            sleep(1)
        }
    }

    func pause() {
        event("board", "일시정지했습니다. 재개하면 같은 세션으로 이어 갑니다.")
        setState("paused", "사람이 일시정지했습니다. 재개하면 이어 갑니다.")
    }

    func stopRun() -> Never {
        if let n = queue.firstIndex(where: { $0["status"] as? String == "running" }).map({ $0 + 1 }) {
            endUnit(n, "stopped", "사람이 중단했습니다.")
        }
        event("board", "사람이 중단해 실행을 마칩니다. 다시 띄우지 않습니다.")
        finish("stopped", "사람이 중단했습니다.")
    }

    // 지정한 시각까지 쉰다(일시정지·중단은 받는다)
    func waitUntil(_ t: Double) -> WaitEnd {
        while now() < t {
            switch pollControl() {
            case .pause: return .paused
            case .stop: return .stopped
            case nil: break
            }
            sleep(1)
        }
        return .elapsed
    }

    // 직전 엔진이 남긴 claude·도구 명령·판정 명령을 끝낸다 — 한 세션을 두 프로세스가 함께 쓰지 않게
    func killLeftovers(_ n: Int) {
        var u = loadUnit(n)
        guard let procs = u["procs"] as? [String: Any] else { return }
        var notes: [String] = []
        if let pid = num(procs["claude_pid"]).map({ pid_t($0) }), pid > 1, let sid = u["session_id"] as? String {
            if kill(pid, 0) == 0, commandOf(pid).contains(sid) {
                kill(pid, SIGTERM)
                for _ in 0..<50 where kill(pid, 0) == 0 { usleep(100_000) }
                if kill(pid, 0) == 0 { kill(pid, SIGKILL) }
                notes.append("claude(pid \(pid)) 살아 있어 끝냄")
            } else {
                notes.append("claude(pid \(pid)) 이미 끝나 있음")
            }
        }
        let table = processTable()
        for g in procs["groups"] as? [[String: Any]] ?? [] {
            guard let pg = num(g["pgid"]).map({ pid_t($0) }), let cmd = g["cmd"] as? String else { continue }
            let members = table.filter { $0.pgid == pg }
            if members.contains(where: { $0.cmd.hasPrefix(String(cmd.prefix(60))) }) {
                killGroup(pg)
                notes.append("도구·판정 명령 group \(pg) 살아 있어 끝냄")
            } else {
                notes.append("group \(pg) 이미 끝나 있음")
            }
        }
        u["restart_cleanup"] = (u["restart_cleanup"] as? [String] ?? []) + ["\(stampFmt.string(from: Date())) " + notes.joined(separator: ", ")]
        u["procs"] = [:] as [String: Any]
        saveUnit(n, u)
        elog("다시 뜬 엔진의 정리: " + notes.joined(separator: ", "))
    }

    // MARK: 멈출 선

    // 지금 계정 사용률(15분 안에 읽은 값). 없으면 캐시·API 로 새로 읽는다
    func freshUsage() -> [String: Any]? {
        if let u = run["usage"] as? [String: Any], let at = num(u["at"]), now() - at < 900 { return u }
        if let u = usageFromCache() ?? usageFromAPI() { run["usage"] = u; saveRun(); return u }
        return nil
    }

    // 새 단위를 시작해도 되는가. 계정 사용률이 멈출 선 이상이면 초기화 시각까지 기다리고 false(다시 확인하게)
    func stoplineGate() -> Bool {
        let line = cfg["stopline"] as? [String: Any] ?? [:]
        let l7 = num(line["seven_day"]) ?? 0.80, l5 = num(line["five_hour"]) ?? 0.95
        var until: Double?, reason = ""
        if let u = freshUsage() {
            let t = now()
            let r7 = num(u["seven_day_resets"]) ?? 0, r5 = num(u["five_hour_resets"]) ?? 0
            let v7 = r7 > 0 && r7 <= t ? 0 : num(u["seven_day"]) ?? 0     // 초기화 시각이 지났으면 그 창은 0 으로 본다
            let v5 = r5 > 0 && r5 <= t ? 0 : num(u["five_hour"]) ?? 0
            if v7 >= l7 {
                until = (r7 > t ? r7 : t + 600) + 120
                reason = "계정 주간 사용률 \(Int((v7 * 100).rounded()))% 가 멈출 선 \(Int((l7 * 100).rounded()))% 에 닿아 새 단위를 시작하지 않고 \(when(until!)) 까지 기다립니다."
            } else if v5 >= l5 {
                until = (r5 > t ? r5 : t + 600) + 120
                reason = "계정 5시간 사용률 \(Int((v5 * 100).rounded()))% 가 멈출 선 \(Int((l5 * 100).rounded()))% 에 닿아 새 단위를 시작하지 않고 \(when(until!)) 까지 기다립니다."
            }
        } else {
            until = now() + 600
            reason = "계정 사용률을 알 수 없어 새 단위를 시작하지 않고 10분 뒤 다시 확인합니다."
        }
        guard let until else { return true }
        if run["reason"] as? String != reason { event("board", reason) }
        setState("waiting_stopline", reason, until: until)
        switch waitUntil(until) {
        case .elapsed: setState("running", "멈출 선 대기를 마치고 다시 확인합니다.")
        case .paused: pause()
        case .stopped: stopRun()
        }
        return false
    }

    // 단위의 시험 환경이 살아 있는가(precheck, 선택). 실패하면 단위를 시작하지 않고 precheck_retry_min(기본 5분) 뒤 다시 확인한다(false)
    func precheckGate(_ n: Int) -> Bool {
        guard let cmd = def(n)["precheck"] as? String, !cmd.isEmpty else { return true }
        let log = privateDir + "/precheck-\(n).log"
        try? fm.removeItem(atPath: log)
        var code: Int32 = -1
        if let c = Child(shell, ["-l", "-c", cmd], env: commandEnv(n), cwd: cwd(n), outFile: log, newGroup: true) {
            if c.wait(120) { code = c.code ?? -1 }
            killGroup(c.pid)
            _ = c.wait(3)
        }
        if code == 0 {
            if state == "waiting_precheck" { event("board", "시험 환경 확인을 통과해 단위를 시작합니다.") }
            return true
        }
        let last = String(decoding: (fm.contents(atPath: log) ?? Data()).suffix(800), as: UTF8.self)
            .split(whereSeparator: \.isNewline).last.map { short(String($0), 120) } ?? ""
        let until = now() + (num(cfg["precheck_retry_min"]) ?? 5) * 60
        let reason = "단위 \(n)/\(total) 의 시험 환경 확인이 실패해(종료 코드 \(code)) 시작하지 않고 \(when(until)) 에 다시 확인합니다\(last.isEmpty ? "" : ": " + last)"
        if state != "waiting_precheck" { event("board", reason) }
        setState("waiting_precheck", reason, until: until)
        switch waitUntil(until) {
        case .elapsed: break
        case .paused: pause()
        case .stopped: stopRun()
        }
        return false
    }

    // MARK: 단위

    func firstMessage(_ n: Int) -> String {
        let q = def(n)
        let tickets = (q["tickets"] as? [Any] ?? []).map { "\($0)" }
        var s = (q["prompt"] as? String ?? "") + "\n\n---\n"
        s += "이 일은 Claude Board 자율주행의 작업 단위 하나입니다(「\(title(n))」).\n"
        s += "- 작업 폴더: \(cwd(n))\n"
        s += "- 판정 명령: `\(q["check"] as? String ?? "")` (일을 마치면 엔진이 작업 폴더에서 돌려 종료 코드 0 이면 통과로 봅니다. 마치기 전에 직접 돌려 보세요.)\n"
        s += tickets.isEmpty
            ? "- 이 단위에 쓸 Jira 티켓 번호는 없습니다. 새 티켓이 있어야 하는 부분은 하지 말고 next_start 에 적으세요.\n"
            : "- 이 단위에 쓸 Jira 티켓: \(tickets.joined(separator: ", ")) (보드가 만들어 둔 번호입니다. 새로 만들지 않습니다.)\n"
        if def(n)["context"] is String {
            s += "- 이 단위의 근거 묶음(읽기 전용, 보드가 넘긴 사본): `\(unitDir(n))/context/`\n"
        }
        if fm.fileExists(atPath: dir + "/answers.json") {
            s += "- 출발 때 고른 답: \(dir)/answers.json (카드: \(dir)/cards.json). 따를 답이 있으면 따르고 decisions_applied 에 적으세요.\n"
        }
        s += "- 쓰기는 작업 폴더\(writeRoots.isEmpty ? "" : "와 " + writeRoots.joined(separator: ", ")), 단위 폴더, 임시 폴더만 됩니다(샌드박스). 네트워크는 허용한 곳만 나갑니다.\n"
        if unitDir(n).hasPrefix(home + "/.claude/") {
            // ~/.claude 아래는 Claude Code 의 보호 경로라 Write·Edit 도구가 묻지 않는 모드에서 거부된다. 샌드박스 Bash 는 쓸 수 있다(실측)
            s += "- 단위 폴더(`\(unitDir(n))`)는 Write·Edit 도구로 쓸 수 없습니다. result.json 과 작업 파일은 Bash 로 쓰세요(예: `cat > \"$CLAUDE_BOARD_UNIT_DIR/result.json\" <<'EOF'` … `EOF`).\n"
        }
        s += "- 일을 마치면 결과 기록을 `\(unitDir(n))/result.json` 에 아래 형식의 JSON 하나로 쓰세요. 사람이 읽는 글은 모두 쉬운 한국어로 쓰고, 내부 번호(티켓·결정 번호 등)는 문장에 쓰지 말고 refs 에만 둡니다. 비용은 엔진이 채우므로 쓰지 않습니다. 해당 없는 칸은 빈 배열이나 빈 글자로 둡니다.\n\n"
        s += "```json\n\(resultFormat)\n```\n"
        return s
    }

    func beginUnit(_ n: Int) {
        try? fm.createDirectory(atPath: unitDir(n), withIntermediateDirectories: true)
        if def(n)["context"] is String {   // 근거 묶음: 출발 때 뜬 사본을 단위 폴더로 넘긴다. 모델 쪽은 쓰기 금지(권한 규칙·샌드박스), 파일은 읽기 전용
            let dst = unitDir(n) + "/context"
            try? fm.removeItem(atPath: dst)
            try? fm.copyItem(atPath: dir + "/pinned/context/\(n)", toPath: dst)
            let files = (fm.enumerator(atPath: dst)?.allObjects as? [String] ?? []).filter { !isDir(dst + "/" + $0) }
            for f in files { chmod(dst + "/" + f, 0o444) }
            event("board", "근거 묶음 파일 \(files.count)개를 단위 폴더(context)로 넘겼습니다.")
        }
        let u: [String: Any] = [
            "unit": n, "id": def(n)["id"] as? String ?? "u\(n)", "title": title(n), "status": "running",
            "phase": (def(n)["action"] as? String ?? "").isEmpty ? "model" : "action",
            "started": r3(now()), "active_sec": 0, "cost_usd": 0, "cost_done": 0, "turns": 0, "caps": caps(n),
            "try": 0, "errors": 0, "checks": [] as [Any], "sessions": [] as [Any], "denials": [] as [Any],
            "first_message": firstMessage(n), "next_message": firstMessage(n),
            "weekly_base": (run["usage"] as? [String: Any])?["seven_day"] ?? NSNull()]
        saveUnit(n, u)
        queue[n - 1]["status"] = "running"
        saveQueue()
        run["unit"] = n
        run["unit_caps"] = caps(n)   // run.json caps 는 출발 화면이 정한 기본값이라 엔진이 덮지 않는다
        event("board", "단위 \(n)/\(total) 시작: \(title(n))")
        setState("running", "단위 \(n)/\(total) 하는 중: \(title(n))")
    }

    func endUnit(_ n: Int, _ status: String, _ reason: String) {
        var u = loadUnit(n)
        u["status"] = status; u["phase"] = "end"; u["ended"] = r3(now()); u["reason"] = reason
        if status == "capped" {
            u["git"] = ["status": safeGit(["status", "--porcelain"], cwd: cwd(n)), "diff_stat": safeGit(["diff", "--stat"], cwd: cwd(n))]
        }
        let ok = readJSON(unitDir(n) + "/result.json") is [String: Any]
        u["result_ok"] = ok
        saveUnit(n, u)
        queue[n - 1]["status"] = status
        saveQueue()
        let spent = "\(dur(num(u["active_sec"]) ?? 0)), \(usd(num(u["cost_usd"]) ?? 0)) USD"
        event("board", "단위 \(n)/\(total) \(unitLabel[status] ?? status): \(reason) (\(spent))")
        if !ok, status != "stopped" { event("board", "모델이 결과 기록(result.json)을 남기지 않았습니다.") }
    }

    func runUnit(_ n: Int) {
        var u = loadUnit(n)
        if crashResume {
            crashResume = false
            if u["phase"] as? String == "model" { u["next_message"] = msgRestart; saveUnit(n, u) }
        }
        let retries = Int(num(cfg["check_retries"]) ?? 2)
        while true {
            switch u["phase"] as? String ?? "model" {
            case "model":
                let end = turn(n, &u, message: u["next_message"] as? String ?? msgRestart)
                switch end {
                case .finished:
                    u["phase"] = "check"; saveUnit(n, u)
                case .limit(let resets):
                    let t = now()
                    let until = (resets.map { $0 > t ? $0 : t + 1800 } ?? t + 1800) + 120   // 초기화 2분 뒤. 모르거나 이미 지났으면 30분 뒤
                    u["phase"] = "limit_wait"; u["wait_until"] = r3(until); u["next_message"] = msgLimit
                    u["limits"] = (num(u["limits"]) ?? 0) + 1
                    saveUnit(n, u)
                    event("board", "사용량 한도에 막혔습니다. \(when(until)) 에 같은 세션으로 다시 시작합니다\(resets == nil ? "(초기화 시각을 몰라 30분 뒤)" : "").")
                case .capped(let kind):
                    endUnit(n, "capped", "\(capLabel[kind] ?? kind) 상한에 닿아 멈췄습니다. 바뀐 파일 목록을 남기고 다음 단위로 갑니다.")
                    return
                case .paused:
                    u["next_message"] = msgPaused; saveUnit(n, u); pause(); return
                case .stopped:
                    stopRun()
                case .failed(let msg):
                    let e = Int(num(u["errors"]) ?? 0) + 1
                    u["errors"] = e; u["next_message"] = msgError; saveUnit(n, u)
                    if e >= 3 { endUnit(n, "failed", "claude 가 세 번 오류로 끝나 미통과로 기록합니다: \(msg)"); return }
                    event("board", "claude 가 오류로 끝났습니다(\(msg)). 1분 뒤 같은 세션으로 다시 엽니다(\(e)/3).")
                    switch waitUntil(now() + 60) {
                    case .elapsed: break
                    case .paused: pause(); return
                    case .stopped: stopRun()
                    }
                }
            case "action":
                switch action(n, &u) {
                case nil: u["phase"] = "model"; saveUnit(n, u)
                case .paused?: pause(); return              // 재개하면 고정 단계를 처음부터 다시 돌린다
                case .stopped?: stopRun()
                case .capped(let kind)?:
                    endUnit(n, "capped", "\(capLabel[kind] ?? kind) 상한에 닿아 멈췄습니다."); return
                default: return
                }
            case "limit_wait":
                let until = num(u["wait_until"]) ?? now()
                setState("waiting_limit", "사용량 한도로 쉬는 중입니다. \(when(until)) 에 같은 자리에서 다시 시작합니다.", until: until)
                switch waitUntil(until) {
                case .elapsed:
                    u["phase"] = "model"; saveUnit(n, u)
                    event("board", "한도 대기를 마치고 같은 세션으로 이어 갑니다.")
                    setState("running", "단위 \(n)/\(total) 하는 중: \(title(n))")
                case .paused: pause(); return
                case .stopped: stopRun()
                }
            case "check":
                switch check(n, &u) {
                case .passed:
                    endUnit(n, "done", "판정 명령을 통과했습니다."); return
                case .failed(let code, let tail):
                    let k = Int(num(u["try"]) ?? 0)
                    if k >= retries {
                        endUnit(n, "failed", "\(k == 0 ? "" : "\(k)번 다시 해도 ")판정 명령을 통과하지 못해 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다.")
                        return
                    }
                    u["try"] = k + 1; u["phase"] = "model"
                    u["next_message"] = """
                    판정 명령이 실패했습니다(종료 코드 \(code)). 판정 명령: `\(def(n)["check"] as? String ?? "")`
                    아래는 출력 끝부분입니다. 원인을 고쳐 판정을 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.

                    ```
                    \(tail)
                    ```
                    """
                    saveUnit(n, u)
                    event("board", "판정 실패(종료 코드 \(code)). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(\(k + 1)/\(retries)).")
                case .capped(let kind):
                    endUnit(n, "capped", "\(capLabel[kind] ?? kind) 상한에 닿아 멈췄습니다. 바뀐 파일 목록을 남기고 다음 단위로 갑니다.")
                    return
                case .paused: pause(); return
                case .stopped: stopRun()
                }
            default:
                return
            }
        }
    }

    // MARK: claude 한 턴

    func claudeArgs(_ n: Int, _ u: inout [String: Any]) -> [String] {
        var a = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--model", model]
        if let e = cfg["effort"] as? String, !e.isEmpty { a += ["--effort", e] }
        var sid = u["session_id"] as? String ?? ""
        if u["session_started"] as? Bool == true {
            a += ["--resume", sid]
        } else {
            // 처음이거나, 앞서 띄운 claude 가 세션을 만들기 전에 끊겼다 — 새 id 로 연다
            if sid.isEmpty || u["session_tried"] as? Bool == true { sid = UUID().uuidString.lowercased() }
            u["session_id"] = sid; u["session_tried"] = true
            u["sessions"] = (u["sessions"] as? [String] ?? []) + [sid]
            run["sessions"] = (run["sessions"] as? [String] ?? []) + [sid]
            saveRun()
            a += ["--session-id", sid]
        }
        let cap = caps(n)["usd"] ?? 100
        a += ["--max-budget-usd", String(format: "%.2f", max(cap - (num(u["cost_done"]) ?? 0), 0.01))]
        // 파일 쓰기 도구는 쓰기 범위마다 경로를 붙여서만 허용한다. 실행 폴더의 상태 파일·고정 사본·다른 단위는 거부 규칙으로도 막는다
        let writable = [cwd(n)] + writeRoots + [unitDir(n)]
        let allowed = ((cfg["allowed_tools"] as? [String]) ?? defaultAllowed).filter { !fileWriteTools.contains($0) }
            + writable.map { "Edit(/\($0)/**)" }
        var denied = alwaysDenied + ((cfg["disallowed_tools"] as? [String]) ?? [])
        denied += ["run.json", "queue.json", "control.json", "events.jsonl", "context.jsonl", "starts.log", "engine.log",
                   "cards.json", "answers.json", ".engine.lock", "pinned/**", "engine/**", "inbox/**", "outbox/**"].map { "Edit(/\(dir)/\($0))" }
        denied += ["engine.json", "stream.jsonl", "stderr.log", "evidence/**", "context/**"].map { "Edit(/\(unitDir(n))/\($0))" }
        denied += (1...total).filter { $0 != n }.map { "Edit(/\(unitDir($0))/**)" }
        // 엔진 전용 폴더(복원 전 백업 등)와 read_deny 는 파일 도구로도 읽지 못하게(샌드박스 denyRead 는 Read 도구를 막지 않는다)
        for p in engineDeny + readDenyPaths { denied += ["Read(/\(p)/**)", "Read(/\(p))", "Edit(/\(p)/**)"] }
        // 사용자 settings 는 읽지 않는다(--setting-sources project,local): 사용자 hook(알림 패널)·플러그인 hook 은 샌드박스 밖에서 돌고,
        // 사용자 허용 규칙의 WebFetch(domain:…) 는 샌드박스 네트워크 허용 목록에 합쳐져 고정한 목록을 넓힌다. ~/.claude/CLAUDE.md 도 함께 빠진다(실측)
        a += ["--setting-sources", "project,local"]
        a += ["--permission-mode", "dontAsk", "--permission-prompts", "none", "--strict-mcp-config", "--settings", sandboxSettings(n),
              "--allowedTools", allowed.joined(separator: " "), "--disallowedTools", denied.joined(separator: " ")]
        for d in [unitDir(n)] + writeRoots { a += ["--add-dir", d] }
        a += ["--append-system-prompt", systemPromptText]
        a += cfg["claude_args"] as? [String] ?? []    // 시험용(--dev 출발만, 예: --setting-sources project,local)
        return a
    }

    // 이번 단위의 샌드박스 설정 파일(엔진 비공개 폴더, 0600). Bash 와 그 자식 프로세스에 운영체제가 건다:
    // 쓰기는 작업 폴더·write_roots·단위 폴더·임시 폴더, 그 안에서도 하네스 자리는 쓰기 금지, 네트워크는 허용 목록만(git 원격 없음)
    func sandboxSettings(_ n: Int) -> String {
        var deny = harnessDeny([cwd(n)] + writeRoots)
        deny += ["engine.json", "stream.jsonl", "stderr.log", "evidence", "context"].map { unitDir(n) + "/" + $0 }
        let readDeny = engineDeny + readDenyPaths
        let s: [String: Any] = ["sandbox": [
            "enabled": true, "failIfUnavailable": true, "allowUnsandboxedCommands": false, "autoAllowBashIfSandboxed": true,
            "excludedCommands": cfg["sandbox_exclude"] as? [String] ?? [],
            "filesystem": ["allowWrite": writeRoots, "denyWrite": deny + readDeny, "denyRead": readDeny],
            "network": ["allowedDomains": cfg["network_allow"] as? [String] ?? defaultNetwork, "strictAllowlist": true,
                        "allowLocalBinding": cfg["local_binding"] as? Bool ?? false, "allowUnixSockets": cfg["unix_sockets"] as? [String] ?? []]]]
        let path = privateDir + "/settings-\(n).json"
        writePrivate(path, Data(jsonText(s, pretty: true).utf8))
        return path
    }

    // 쓰기 범위 안의 하네스 자리: 각 범위의 맨 위와, 깊이 4 까지 CLAUDE.md·.claude·.git 이 있는 폴더(하위 저장소·submodule).
    // `.claude/worktrees/` 안은 내려가지 않는다 — 그 안의 사본까지 막으면 git worktree 를 못 만든다(남는 위험은 설계.md 4장)
    func harnessDeny(_ roots: [String]) -> [String] {
        let skip: Set<String> = [".git", ".claude", "node_modules", "build", ".gradle", "target", "dist", "out", ".idea"]
        var found: [String] = [], extra: [String] = []
        for root in roots {
            // 도구 캐시(~/.gradle·~/.m2)는 저장소가 아니라 훑지 않고, 다음 빌드에서 코드로 실행되는 설정만 막는다(판정 명령은 샌드박스 밖에서 돈다)
            switch (root as NSString).lastPathComponent {
            case ".gradle": extra += ["init.d", "init.gradle", "init.gradle.kts", "gradle.properties"].map { root + "/" + $0 }; continue
            case ".m2": extra += ["settings.xml", "settings-security.xml", "extensions.xml"].map { root + "/" + $0 }; continue
            default: break
            }
            let top = (try? fm.contentsOfDirectory(atPath: root)) ?? []
            guard top.contains(".git") || top.contains("CLAUDE.md") || top.contains(".claude") else { found.append(root); continue }
            var level = [root], depth = 0
            while !level.isEmpty, depth <= 4 {
                var next: [String] = []
                for d in level {
                    let names = (try? fm.contentsOfDirectory(atPath: d)) ?? []
                    if d == root || names.contains("CLAUDE.md") || names.contains(".claude") || names.contains(".git") { found.append(d) }
                    for name in names where !skip.contains(name) && isDir(d + "/" + name) { next.append(d + "/" + name) }
                }
                level = next; depth += 1
            }
        }
        return found.flatMap { d in harnessEntries.map { d + "/" + $0 } } + extra
    }

    // claude 가 띄운 도구 명령의 process group(claude·엔진과 다른 group) — pgid → 명령 앞부분
    func toolGroups(_ claudePid: pid_t) -> [pid_t: String] {
        let table = processTable(), me = getpgrp()
        var kids = Set([claudePid]), groups: [pid_t: String] = [:], grew = true
        while grew {
            grew = false
            for p in table where kids.contains(p.ppid) && !kids.contains(p.pid) { kids.insert(p.pid); grew = true }
        }
        for p in table where kids.contains(p.pid) && p.pid != claudePid && p.pgid != me {
            if groups[p.pgid] == nil || p.pid == p.pgid { groups[p.pgid] = String(p.cmd.prefix(120)) }
        }
        return groups
    }

    // claude 는 Bash 의 작업 폴더가 된 곳마다 빈 `.claude/.cc-writes`(파일 바꿔 쓰기용 임시 자리)를 만든다(실측) — 단위 폴더 안의 빈 것은 치운다
    func cleanStaging(_ n: Int) {
        guard let all = fm.enumerator(atPath: unitDir(n))?.allObjects as? [String] else { return }
        for rel in all where rel.hasSuffix(".claude/.cc-writes") {
            let p = unitDir(n) + "/" + rel
            if (try? fm.contentsOfDirectory(atPath: p))?.isEmpty == true {
                rmdir(p)
                let parent = (p as NSString).deletingLastPathComponent
                if (try? fm.contentsOfDirectory(atPath: parent))?.isEmpty == true { rmdir(parent) }
            }
        }
    }

    var bgQuiet: Double { num(cfg["background_quiet_sec"]) ?? 120 }

    func claudeEnv(_ n: Int) -> [String: String] {
        var e = passEnv(loginEnv, extra: envPass)
        e["CLAUDE_BOARD_AUTOPILOT"] = id
        e["CLAUDE_BOARD_AUTOPILOT_DIR"] = dir
        e["CLAUDE_BOARD_UNIT_DIR"] = unitDir(n)
        // git push 막기: 모든 push 주소를 없는 주소로 바꾼다(fetch 는 그대로). Bash 거부 규칙을 비켜 가는 `git -C . push` 도 막힌다(실측)
        let k = Int(e["GIT_CONFIG_COUNT"] ?? "") ?? 0
        e["GIT_CONFIG_COUNT"] = String(k + 1)
        e["GIT_CONFIG_KEY_\(k)"] = "url.autopilot-push-blocked:.pushInsteadOf"
        e["GIT_CONFIG_VALUE_\(k)"] = ""
        // claude 프로세스 자신도 세션을 열 때 샌드박스 밖에서 git status 를 부른다(저장소 fsmonitor 를 실행함, 실측) — fsmonitor 는 끈다.
        // hooksPath 는 claude 쪽에 넣지 않는다(모델이 샌드박스 안에서 하는 commit 의 저장소 훅까지 꺼진다)
        e["GIT_CONFIG_COUNT"] = String(k + 2)
        e["GIT_CONFIG_KEY_\(k + 1)"] = "core.fsmonitor"
        e["GIT_CONFIG_VALUE_\(k + 1)"] = "false"
        return e
    }

    // 엔진이 샌드박스 밖에서 돌리는 명령(precheck·action·판정)의 환경: claude 와 같은 고른 변수에 git 보호를 더한다.
    // 모델이 저장소 git 설정에 core.fsmonitor·hooksPath 를 심어도 밖의 git 이 실행하지 않게 명령줄 설정(-c 와 같은 무게)으로 덮고,
    // 시스템·전역 git 설정은 읽지 않는다. 엔진 전용 폴더(CLAUDE_BOARD_PRIVATE_DIR)는 이 명령들만 안다
    func commandEnv(_ n: Int) -> [String: String] {
        var e = claudeEnv(n)
        var k = Int(e["GIT_CONFIG_COUNT"] ?? "") ?? 0
        for (key, value) in [("core.hooksPath", "/dev/null")] {   // fsmonitor 는 claudeEnv 에 이미 있다
            e["GIT_CONFIG_KEY_\(k)"] = key; e["GIT_CONFIG_VALUE_\(k)"] = value; k += 1
        }
        e["GIT_CONFIG_COUNT"] = String(k)
        e["GIT_CONFIG_NOSYSTEM"] = "1"
        e["GIT_CONFIG_GLOBAL"] = "/dev/null"
        e["CLAUDE_BOARD_PRIVATE_DIR"] = enginePrivate
        return e
    }

    // 한 턴 동안의 셈
    struct Turn {
        var resultSeen = false      // 진짜 result 를 받았다(num_turns 0 인 빈 result 는 세지 않는다)
        var open = false            // result 뒤 새 턴이 시작됐다(백그라운드 작업 알림 등)
        var lastError = false
        var bg = 0                  // 돌고 있는 백그라운드 작업 수
        var limit = false
        var limitResets: Double?
        var budget = false
        var cost = 0.0              // 이 프로세스의 누적 비용(result 의 total_cost_usd — 프로세스마다 0 부터, 실측)
        var lastLine = now()
        var lastText = ""
        var looked = 0              // 모아 쓰는 "살펴봄"(읽기·찾기) 수
        var errText = ""
    }

    func turn(_ n: Int, _ u: inout [String: Any], message: String) -> TurnEnd {
        let args = claudeArgs(n, &u)
        let sid = u["session_id"] as? String ?? ""
        let resumed = args.contains("--resume")
        // 새 세션이면(처음이거나, 세션이 만들어지기 전에 끊겼다) 이어 가는 말이 아니라 단위의 첫 입력을 넘긴다
        let message = resumed ? message : (u["first_message"] as? String ?? firstMessage(n))
        saveUnit(n, u)
        guard let c = Child(claude, args, env: claudeEnv(n), cwd: cwd(n), stdinPipe: true, stdoutPipe: true,
                            errFile: unitDir(n) + "/stderr.log") else {
            return .failed("claude 를 띄우지 못했습니다")
        }
        u["procs"] = ["claude_pid": Int(c.pid), "groups": [] as [Any]]
        u["claude_runs"] = (num(u["claude_runs"]) ?? 0) + 1
        saveUnit(n, u)
        if !resumed { event("board", "모델에게 일을 넘겼습니다(새 세션).") }
        c.send(jsonText(["type": "user", "message": ["role": "user", "content": message]]) + "\n")
        var t = Turn()
        let cap = caps(n)
        var tick = now(), lastCtl = 0.0, lastSave = now(), lastScan = 0.0
        func handleAll() { for line in c.drain() { handle(line, n, &u, &t, sid) } }
        func active() {
            let x = now()
            u["active_sec"] = r3((num(u["active_sec"]) ?? 0) + min(max(x - tick, 0), 5))   // 잠자기 등으로 시계가 튀면 5초까지만 센다
            tick = x
            u["cost_usd"] = r3((num(u["cost_done"]) ?? 0) + t.cost)
        }
        func stopClaude() -> Void {
            event("board", "claude 를 멈춥니다(SIGINT).")
            c.signal(SIGINT)
            if !c.wait(20, tick: handleAll) { c.signal(SIGTERM); if !c.wait(5, tick: handleAll) { c.signal(SIGKILL); _ = c.wait(2) } }
            c.closeInput()
            handleAll()
            for g in (u["procs"] as? [String: Any])?["groups"] as? [[String: Any]] ?? [] {
                if let pg = num(g["pgid"]) { killGroup(pid_t(pg)) }
            }
        }
        func endProcess() {
            u["cost_done"] = r3((num(u["cost_done"]) ?? 0) + t.cost); t.cost = 0
            u["cost_usd"] = u["cost_done"]
            if t.looked > 0 { event("model", "파일 \(t.looked)곳을 살펴봤습니다."); t.looked = 0 }
            u["procs"] = [:] as [String: Any]
            saveUnit(n, u); saveRun()
        }
        var stopped: TurnEnd?
        var bgCleared: Double?   // 결과 뒤 남은 백그라운드 작업을 치운 시각
        while true {
            handleAll()
            active()
            let x = now()
            if t.resultSeen, !t.open, t.bg == 0 || t.lastError, x - t.lastLine >= 2 { break }   // 턴이 끝났다
            // result 를 보냈는데 백그라운드 작업만 남아 오래 조용하다(실제 실행: 모델이 홈 폴더 전체 검색을 띄워 두고 끝내 20분 넘게 멈춤) —
            // 그 작업들을 치우고 넘어간다. 단위 시간 상한 안에서만 기다린다(아래 상한 확인이 같이 돈다)
            if t.resultSeen, !t.open, t.bg > 0, x - t.lastLine >= bgQuiet, bgCleared == nil {
                event("board", "결과를 보낸 뒤 백그라운드 작업 \(t.bg)개만 남아 \(dur(bgQuiet)) 동안 새 출력이 없어, 그 작업을 정리하고 판정으로 넘어갑니다.")
                for (pg, _) in toolGroups(c.pid) { killGroup(pg) }
                bgCleared = x
            }
            // 치우면 claude 가 작업 끝 알림을 받고 짧은 턴을 한 번 더 돈다(실측) — 그 턴이 끝나면 위 첫 줄로 넘어가고, 조용하면 10초 뒤 넘어간다
            if let k = bgCleared, t.resultSeen, !t.open, x - max(k, t.lastLine) >= 10 { break }
            if c.poll() != nil, c.atEOF || x - t.lastLine >= 3 { break }
            if x - lastCtl >= 1 {
                lastCtl = x
                if let s = pollControl() { stopped = s == .pause ? .paused : .stopped; break }
                // 단위 주간 %p = 단위를 시작할 때부터 오른 계정 주간 사용률. 초기화 시각이 지난 값은 낡아서 쓰지 않는다
                let usage = run["usage"] as? [String: Any] ?? [:]
                if num(usage["seven_day_resets"]).map({ $0 > x }) ?? true, let cur = num(usage["seven_day"]) {
                    var base = num(u["weekly_base"]) ?? cur   // 단위를 시작할 때 몰랐으면 이 단위의 첫 값부터 잰다
                    let last = num(u["weekly_last"]) ?? base
                    if cur < last {   // 주간 창이 초기화됐다 — 앞 창에서 쓴 만큼을 쌓고 새 창에서 다시 잰다
                        u["weekly_pp_done"] = r3((num(u["weekly_pp_done"]) ?? 0) + max(last - base, 0) * 100)
                        base = cur
                    }
                    u["weekly_base"] = base; u["weekly_last"] = cur
                    u["weekly_pp"] = r3((num(u["weekly_pp_done"]) ?? 0) + max(cur - base, 0) * 100)
                }
                if (num(u["active_sec"]) ?? 0) >= (cap["minutes"] ?? 120) * 60 { stopped = .capped("minutes"); break }
                // 한도 거절을 받았으면 사용률이 100% 로 튀어 있다 — 상한이 아니라 한도 대기로 간다
                if !t.limit, (num(u["weekly_pp"]) ?? 0) >= cap["weekly_pp"] ?? 5 { stopped = .capped("weekly"); break }
                if (num(u["cost_usd"]) ?? 0) >= cap["usd"] ?? 100 { stopped = .capped("usd"); break }
            }
            if x - lastScan >= 5 {   // claude 가 띄운 도구 명령의 group 을 적어 둔다(엔진이 죽으면 다음 엔진이 치운다)
                lastScan = x
                u["procs"] = ["claude_pid": Int(c.pid), "groups": toolGroups(c.pid).map { ["pgid": Int($0.key), "cmd": $0.value] }]
            }
            if x - lastSave >= 5 {
                lastSave = x; saveUnit(n, u)
                if x - lastActivitySave >= 5 { lastActivitySave = x; saveRun() }
            }
            usleep(200_000)
        }
        if let stopped {
            stopClaude()
            cleanStaging(n)
            active()
            endProcess()
            return stopped
        }
        // 턴이 끝났다 — 입력을 닫으면 claude 가 스스로 끝난다(0.6초, 실측)
        c.closeInput()
        if !c.wait(10, tick: handleAll) { c.signal(SIGTERM); if !c.wait(5, tick: handleAll) { c.signal(SIGKILL); _ = c.wait(2) } }
        handleAll()
        cleanStaging(n)
        active()
        endProcess()
        if t.limit { return .limit(t.limitResets) }
        if t.budget { return .capped("usd") }
        if !t.resultSeen {
            let err = (try? String(contentsOfFile: unitDir(n) + "/stderr.log", encoding: .utf8)) ?? ""
            return .failed("결과 없이 끝남, 종료 코드 \(c.code ?? -1)\(err.isEmpty ? "" : ": " + short(String(err.suffix(300)), 120))")
        }
        if t.lastError { return .failed(t.errText.isEmpty ? "claude 오류" : t.errText) }
        return .finished
    }

    // stream-json 한 줄: 기록하고, 사람이 읽는 줄과 원문 요약을 남기고, 한도·비용·끝을 센다
    func handle(_ line: String, _ n: Int, _ u: inout [String: Any], _ t: inout Turn, _ sid: String) {
        appendLine(unitDir(n) + "/stream.jsonl", line)
        guard let o = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any] else { return }
        let type = o["type"] as? String ?? ""
        let sub = o["subtype"] as? String ?? ""
        t.lastLine = now()
        // result 뒤에 모델이 다시 움직이면(백그라운드 작업이 끝나 claude 가 새 턴을 연 경우) 그 턴이 끝날 때까지 기다린다
        if type == "assistant" || type == "user" || (type == "system" && sub == "init") { t.open = true }
        switch type {
        case "system":
            if sub == "init", o["session_id"] as? String == sid, u["session_started"] as? Bool != true {
                u["session_started"] = true; saveUnit(n, u)
            }
            if sub == "background_tasks_changed" { t.bg = (o["tasks"] as? [Any])?.count ?? 0 }
            if sub == "api_retry" { context(n, ["kind": "note", "text": "API 다시 시도 \(num(o["attempt"]).map { String(Int($0)) } ?? "?")번째"]) }
        case "assistant":
            let msg = o["message"] as? [String: Any] ?? [:]
            let main = o["parent_tool_use_id"] == nil || o["parent_tool_use_id"] is NSNull
            if o["error"] as? String == "rate_limit" || num(o["api_error_status"]) == 429 {
                t.limit = true
                let info = (o["api_error_params"] as? [String: Any])?["rate_limit_info"] as? [String: Any]
                if let r = resetsOf(info) { t.limitResets = r }
            }
            for b in msg["content"] as? [[String: Any]] ?? [] {
                switch b["type"] as? String {
                case "text":
                    let s = short(b["text"] as? String ?? "")
                    if main, !s.isEmpty, s != t.lastText { flushLooked(&t); event("model", s); t.lastText = s }
                case "tool_use":
                    let name = b["name"] as? String ?? "?"
                    let input = b["input"] as? [String: Any] ?? [:]
                    context(n, ["kind": "tool_use", "tool": name, "id": b["id"] ?? "", "agent": !main,
                                "input": String(jsonText(input).prefix(600))])
                    if main { toolEvent(name, input, n, &t) }
                default: break
                }
            }
        case "user":
            let msg = o["message"] as? [String: Any] ?? [:]
            for b in msg["content"] as? [[String: Any]] ?? [] where b["type"] as? String == "tool_result" {
                var text = ""
                if let s = b["content"] as? String { text = s }
                else if let arr = b["content"] as? [[String: Any]] { text = arr.compactMap { $0["text"] as? String }.joined(separator: "\n") }
                context(n, ["kind": "tool_result", "id": b["tool_use_id"] ?? "", "is_error": b["is_error"] as? Bool ?? false,
                            "output": String(text.prefix(400))])
            }
        case "rate_limit_event":
            let info = o["rate_limit_info"] as? [String: Any] ?? [:]
            noteRate(info)
            if info["status"] as? String == "rejected" { t.limit = true; t.limitResets = resetsOf(info) ?? t.limitResets }
        case "result":
            let turns = Int(num(o["num_turns"]) ?? 0)
            let text = o["result"] as? String ?? ""
            let isErr = o["is_error"] as? Bool ?? false
            if turns == 0, text.isEmpty, !isErr {
                // 이어 열기 직후 등에 오는 빈 result — 끝으로 세지 않는다(옛 엔진의 결함)
                t.open = false
                context(n, ["kind": "note", "text": "빈 result(num_turns 0)를 받아 끝으로 세지 않았습니다"])
                return
            }
            t.resultSeen = true; t.open = false; t.lastError = isErr
            if let c = num(o["total_cost_usd"]) { t.cost = max(t.cost, c) }
            u["turns"] = Int(num(u["turns"]) ?? 0) + turns
            if sub == "error_max_budget_usd" { t.budget = true }
            // 한도 거절은 subtype 이 success 인데 is_error true 로 온다(실측)
            if isErr, num(o["api_error_status"]) == 429 || o["api_error"] as? String == "usage_limit_reached" { t.limit = true }
            if isErr { t.errText = short(text.isEmpty ? sub : text, 120) }
            flushLooked(&t)
            for d in o["permission_denials"] as? [[String: Any]] ?? [] { denial(d, n, &u) }
        default: break
        }
    }

    func resetsOf(_ info: [String: Any]?) -> Double? {
        guard let info else { return nil }
        if let k = info["rateLimitType"] as? String, let w = (info["unifiedWindows"] as? [String: Any])?[k] as? [String: Any],
           let r = num(w["resetsAt"]) { return r }
        return num(info["resetsAt"])
    }

    // rate_limit_event 의 계정 사용률(0~1, 0.01 단위)을 run.json 에 남긴다
    func noteRate(_ info: [String: Any]) {
        var u = run["usage"] as? [String: Any] ?? [:]
        let w = info["unifiedWindows"] as? [String: Any] ?? [:]
        if let f = w["five_hour"] as? [String: Any] { u["five_hour"] = num(f["utilization"]); u["five_hour_resets"] = num(f["resetsAt"]) }
        if let s = w["seven_day"] as? [String: Any] { u["seven_day"] = num(s["utilization"]); u["seven_day_resets"] = num(s["resetsAt"]) }
        if w.isEmpty, let v = num(info["utilization"]), let k = info["rateLimitType"] as? String, k == "five_hour" || k == "seven_day" {
            u[k] = v; u[k + "_resets"] = num(info["resetsAt"])
        }
        u["at"] = r3(now()); u["from"] = "stream"
        run["usage"] = u
    }

    func rel(_ p: String, _ n: Int) -> String {
        let c = cwd(n) + "/"
        if p.hasPrefix(unitDir(n) + "/") { return "단위 기록 폴더의 " + String(p.dropFirst(unitDir(n).count + 1)) }
        if p.hasPrefix(c) { return String(p.dropFirst(c.count)) }
        return p.hasPrefix(home + "/") ? "~/" + p.dropFirst(home.count + 1) : p
    }

    func flushLooked(_ t: inout Turn) {
        if t.looked > 0 { event("model", "파일 \(t.looked)곳을 살펴봤습니다."); t.looked = 0 }
    }

    // 도구 호출 하나를 쉬운 말 한 줄로. 읽기·찾기는 모아서 한 줄로 쓴다
    func toolEvent(_ name: String, _ i: [String: Any], _ n: Int, _ t: inout Turn) {
        let path = rel(i["file_path"] as? String ?? i["notebook_path"] as? String ?? "", n)
        let text: String
        switch name {
        case "Read", "Glob", "Grep", "LS": t.looked += 1; return
        case "Write": text = path.hasSuffix("result.json") && path.hasPrefix("단위 기록 폴더") ? "결과 기록을 썼습니다." : "파일을 썼습니다: \(path)"
        case "Edit", "MultiEdit", "NotebookEdit": text = "파일을 고쳤습니다: \(path)"
        case "Bash":
            let d = (i["description"] as? String).map { short($0, 100) } ?? ""
            text = "명령을 돌립니다: " + (d.isEmpty ? short(i["command"] as? String ?? "", 80) : d)
        case "Task", "Agent": text = "보조 에이전트에게 맡깁니다: " + short(i["description"] as? String ?? "", 80)
        case "TodoWrite": text = "할 일 목록을 정리했습니다."
        case "WebFetch": text = "웹 페이지를 읽습니다: " + short(i["url"] as? String ?? "", 80)
        case "WebSearch": text = "웹에서 찾아봅니다: " + short(i["query"] as? String ?? "", 80)
        case "Skill": text = "스킬을 씁니다: " + short(i["skill"] as? String ?? i["command"] as? String ?? "", 60)
        default: text = "도구를 씁니다: \(name)"
        }
        flushLooked(&t)
        event("model", text)
    }

    func denial(_ d: [String: Any], _ n: Int, _ u: inout [String: Any]) {
        let name = d["tool_name"] as? String ?? "?"
        let i = d["tool_input"] as? [String: Any] ?? [:]
        let what: String
        switch name {
        case "Write", "Edit", "MultiEdit", "NotebookEdit": what = "파일 고치기(\(rel(i["file_path"] as? String ?? i["notebook_path"] as? String ?? "", n)))"
        case "Bash": what = "명령(\(short(i["command"] as? String ?? "", 80)))"
        default: what = name
        }
        u["denials"] = (u["denials"] as? [Any] ?? []) + [["tool": name, "what": what, "ts": r3(now())]]
        event("board", "허용 범위 밖이라 거부했습니다: \(what)")
    }

    // MARK: 판정 명령

    // 단위의 고정 단계(action): 엔진이 모델보다 먼저 샌드박스 밖에서 돌린다. 정해진 일(DB 원복 등)은 모델이 아니라 엔진이 하고,
    // 모델은 그 결과 파일을 읽어 판단·기록만 한다. 결과는 성공·실패와 상관없이 모델에게 넘긴다(기준은 뒤의 판정 명령이 본다)
    func action(_ n: Int, _ u: inout [String: Any]) -> CheckEnd? {
        let cmd = def(n)["action"] as? String ?? "true"
        let limit = (num(def(n)["action_timeout_min"]) ?? 10) * 60
        event("board", "고정 단계를 돌립니다(모델 전에 엔진이).")
        // 다시 돌 때(일시정지·엔진 다시 뜨기 뒤) 앞서 남긴 파일(쓰기 전 백업 등)을 지우지 않는다 — 출력만 새로 쓴다
        let r = fixedCommand(n, &u, cmd, ev: unitDir(n) + "/evidence/action", limit: limit, keep: true)
        if let stop = r.interrupt { return stop }
        u["action_result"] = ["exit": Int(r.code), "sec": r3(r.sec), "evidence": "evidence/action", "files": r.files, "tail": r.tail]
        event("board", "고정 단계 \(r.code == 0 ? "끝" : "실패(종료 코드 \(r.code))")(\(dur(r.sec))). 결과를 모델에게 넘깁니다.")
        u["first_message"] = firstMessage(n) + """

        ---
        엔진이 이 단위를 시작하기 전에 고정 단계(action)를 돌렸습니다: 종료 코드 \(r.code)\(r.timedOut ? "(시간 상한에 닿아 멈춤)" : ""). 출력과 남긴 파일은 `\(unitDir(n))/evidence/action/` 에 있습니다(\(r.files.joined(separator: ", "))).
        이 결과를 읽고 판단·기록만 하세요. 고정 단계를 다시 돌리거나 같은 일을 직접 하지 마세요.
        """
        return nil
    }

    // 엔진이 샌드박스 밖에서 돌리는 고정 명령(판정·고정 단계): 로그인 셸, 작업 폴더, 고른 환경 변수 + $CLAUDE_BOARD_EVIDENCE_DIR.
    // 출력은 ev/output.log, 명령이 ev 에 남긴 파일 목록을 돌려준다. 일시정지·중단·단위 시간 상한을 받는다
    func fixedCommand(_ n: Int, _ u: inout [String: Any], _ cmd: String, ev: String, limit: Double, keep: Bool = false)
        -> (code: Int32, tail: String, interrupt: CheckEnd?, timedOut: Bool, files: [String], sec: Double) {
        if !keep { try? fm.removeItem(atPath: ev) }
        try? fm.createDirectory(atPath: ev, withIntermediateDirectories: true)
        let log = ev + "/output.log"
        try? fm.removeItem(atPath: log)
        var env = commandEnv(n)
        env["CLAUDE_BOARD_EVIDENCE_DIR"] = ev
        guard let c = Child(shell, ["-l", "-c", cmd], env: env, cwd: cwd(n), outFile: log, newGroup: true) else {
            return (-1, "명령을 띄우지 못했습니다", nil, false, [], 0)
        }
        u["procs"] = ["groups": [["pgid": Int(c.pid), "cmd": "\(shell) -l -c \(cmd)"]]]
        saveUnit(n, u)
        let start = now()
        let cap = (caps(n)["minutes"] ?? 120) * 60
        var tick = start
        var interrupt: CheckEnd?, timedOut = false
        while c.poll() == nil {
            let x = now()
            u["active_sec"] = r3((num(u["active_sec"]) ?? 0) + min(max(x - tick, 0), 5)); tick = x
            if let s = pollControl() { interrupt = s == .pause ? .paused : .stopped; break }
            if (num(u["active_sec"]) ?? 0) >= cap { interrupt = .capped("minutes"); break }
            if x - start >= limit { timedOut = true; break }
            usleep(500_000)
        }
        killGroup(c.pid)   // 끝났어도 명령이 뒤에 남긴 프로세스(띄운 서버 등)까지 치운다
        _ = c.wait(3)
        u["procs"] = [:] as [String: Any]
        var tail = String(decoding: (fm.contents(atPath: log) ?? Data()).suffix(4000), as: UTF8.self)
        tail = tail.split(separator: "\n", omittingEmptySubsequences: false).suffix(40).joined(separator: "\n")
        if timedOut { tail += "\n(시간 상한 \(dur(limit))에 닿아 엔진이 멈췄습니다)" }
        let code: Int32 = interrupt != nil || timedOut ? -1 : (c.code ?? -1)
        let files = ((fm.enumerator(atPath: ev)?.allObjects as? [String]) ?? []).filter { !isDir(ev + "/" + $0) }.sorted()
        saveUnit(n, u)
        return (code, tail, interrupt, timedOut, files, now() - start)
    }

    func check(_ n: Int, _ u: inout [String: Any]) -> CheckEnd {
        let k = Int(num(u["try"]) ?? 0)
        let cmd = def(n)["check"] as? String ?? "false"
        // 증거 폴더: 판정 출력(output.log)과, 판정 명령이 $CLAUDE_BOARD_EVIDENCE_DIR 에 남긴 파일. 엔진만 쓴다(모델은 샌드박스·거부 규칙으로 못 씀)
        let ev = unitDir(n) + "/evidence/check-\(k)"
        event("board", "판정 명령을 돌립니다\(k == 0 ? "" : "(다시 \(k)번째)").")
        let limit = (num(def(n)["check_timeout_min"]) ?? num(cfg["check_timeout_min"]) ?? 10) * 60
        let r = fixedCommand(n, &u, cmd, ev: ev, limit: limit)
        u["checks"] = (u["checks"] as? [Any] ?? []) + [["try": k, "exit": Int(r.code), "sec": r3(r.sec), "tail": r.tail, "at": r3(now()),
                                                        "interrupted": r.interrupt != nil, "evidence": "evidence/check-\(k)", "files": r.files]]
        saveUnit(n, u)
        if let interrupt = r.interrupt { return interrupt }
        if r.code == 0 { event("board", "판정 통과(\(dur(r.sec)))."); return .passed }
        return .failed(r.code, r.tail)
    }
}

// MARK: - 하위 명령

func runDir(_ arg: String?) -> String {
    guard let arg else { usage() }
    let d = canonical(arg)
    guard isDir(d) else { fail("실행 폴더가 없습니다: \(d)") }
    return d
}

func usage() -> Never {
    fail("""
    사용법: claude-autopilot start <실행 폴더> [--dev]
           claude-autopilot run <실행 폴더> <고정 설정 해시>
           claude-autopilot control <실행 폴더> pause|resume|stop
           claude-autopilot status <실행 폴더>
    """)
}

// 쓰기 범위·작업 폴더로 쓰면 안 되는 자리: 홈 전체와 그 위, ~/.claude, ~/Library, 실행 폴더
func badRoot(_ p: String, runDir: String) -> String? {
    if p == "/" || p == home || home.hasPrefix(p + "/") { return "홈 폴더 전체나 그 위" }
    if p == home + "/.claude" || p.hasPrefix(home + "/.claude/") { return "~/.claude 아래" }
    if p.hasPrefix(home + "/Library/") || p == home + "/Library" { return "~/Library 아래" }
    if p == runDir || p.hasPrefix(runDir + "/") || runDir.hasPrefix(p + "/") { return "실행 폴더와 겹침" }
    return nil
}

// 출발 전 확인과 고정: run.json·queue.json 에서 보안에 걸리는 값(모델, 도구 목록, 쓰기 범위, 네트워크, 환경 변수,
// 단위의 prompt·check·precheck·cwd·상한)을 골라 고정 설정으로 만든다. 문제가 있으면 problems 에 쉬운 말로
func prepare(_ dir: String, dev: Bool) -> (problems: [String], frozen: [String: Any], queue: [[String: Any]]) {
    var problems: [String] = []
    let id = (dir as NSString).lastPathComponent
    if id.range(of: "^[A-Za-z0-9._-]+$", options: .regularExpression) == nil {
        problems.append("실행 폴더 이름은 영문·숫자·.-_ 로만 씁니다(launchd 이름에 들어갑니다): \(id)")
    }
    guard let run = readJSON(dir + "/run.json") as? [String: Any] else { return (["run.json 을 읽지 못했습니다"], [:], []) }
    guard var queue = readJSON(dir + "/queue.json") as? [[String: Any]], !queue.isEmpty else {
        return (["queue.json 에 단위가 없습니다(JSON 배열)"], [:], [])
    }
    var f: [String: Any] = ["version": 1, "id": id, "dev": dev, "frozen_at": r3(now())]
    if let m = run["model"] as? String, !m.isEmpty { f["model"] = m } else { problems.append("run.json 에 model 이 없습니다") }
    for k in ["effort", "stopline", "caps", "check_timeout_min", "check_retries", "precheck_retry_min", "background_quiet_sec"] where run[k] != nil { f[k] = run[k] }
    func strings(_ k: String) -> [String]? {
        guard let v = run[k] else { return nil }
        guard let a = v as? [String] else { problems.append("run.json 의 \(k) 는 글자 배열이어야 합니다"); return nil }
        return a
    }
    if let a = strings("allowed_tools") {
        if a.contains(where: { fileWriteTools.contains($0) }) {
            problems.append("allowed_tools 에 경로 없는 파일 쓰기 도구(Write·Edit 등)를 넣지 않습니다. 엔진이 쓰기 범위마다 붙입니다")
        }
        f["allowed_tools"] = a
    }
    for k in ["disallowed_tools", "network_allow", "env_pass", "sandbox_exclude"] { if let a = strings(k) { f[k] = a } }
    if let v = run["local_binding"] { if v is Bool { f["local_binding"] = v } else { problems.append("run.json 의 local_binding 은 true/false 입니다") } }
    if let a = strings("read_deny") {
        if a.contains(where: { !$0.hasPrefix("/") }) { problems.append("read_deny 는 절대 경로로 씁니다") }
        f["read_deny"] = a.map { canonical($0) }
    }
    if let a = strings("unix_sockets") {
        if a.contains(where: { !$0.hasPrefix("/") }) { problems.append("unix_sockets 는 절대 경로로 씁니다") }
        f["unix_sockets"] = a
    }
    var roots: [String] = []
    for r in strings("write_roots") ?? [] {
        let c = canonical(r)
        if !r.hasPrefix("/") || !isDir(c) { problems.append("write_roots 의 \(r) 가 없는 폴더입니다"); continue }
        if let why = badRoot(c, runDir: dir) { problems.append("write_roots 에 \(r) 를 넣을 수 없습니다(\(why))"); continue }
        roots.append(c)
    }
    f["write_roots"] = roots
    for k in ["claude_args", "claude_path"] where run[k] != nil {
        if dev { f[k] = run[k] } else { problems.append("run.json 의 \(k) 는 시험용이라 --dev 로 출발할 때만 받습니다") }
    }
    var units: [[String: Any]] = []
    for (i, q) in queue.enumerated() {
        let n = i + 1
        for k in ["title", "prompt", "check", "cwd"] where (q[k] as? String ?? "").isEmpty { problems.append("단위 \(n) 에 \(k) 가 없습니다") }
        var d: [String: Any] = ["id": q["id"] as? String ?? "u\(n)", "title": q["title"] ?? "", "prompt": q["prompt"] ?? "", "check": q["check"] ?? "",
                                "tickets": q["tickets"] ?? [] as [Any], "review": q["review"] ?? false]
        if let c = q["cwd"] as? String, c.hasPrefix("/"), isDir(c) {
            let cc = canonical(c)
            if let why = badRoot(cc, runDir: dir) { problems.append("단위 \(n) 의 cwd 로 \(c) 를 쓸 수 없습니다(\(why))") }
            d["cwd"] = cc
        } else if q["cwd"] != nil { problems.append("단위 \(n) 의 cwd 가 없는 폴더입니다") }
        for k in ["precheck", "action"] {
            if let p = q[k] { if p is String { d[k] = p } else { problems.append("단위 \(n) 의 \(k) 는 글자여야 합니다") } }
        }
        if let c = q["context"] {   // 근거 묶음 폴더: 단위를 시작할 때 units/<n>/context/ 로 복사해 읽기 전용으로 넘긴다
            if let p = c as? String, p.hasPrefix("/"), isDir(p) {
                d["context"] = canonical(p)
                let links = (fm.enumerator(atPath: p)?.allObjects as? [String] ?? []).filter {
                    (try? fm.attributesOfItem(atPath: p + "/" + $0)[.type] as? FileAttributeType) == .typeSymbolicLink
                }
                if !links.isEmpty { problems.append("단위 \(n) 의 context 안에 바로 가기(심볼릭 링크)가 있습니다: \(links.prefix(3).joined(separator: ", "))") }
            } else { problems.append("단위 \(n) 의 context 는 있는 폴더의 절대 경로여야 합니다") }
        }
        for k in ["check_timeout_min", "action_timeout_min"] {
            if let t = q[k] { if (num(t) ?? 0) > 0 { d[k] = t } else { problems.append("단위 \(n) 의 \(k) 는 0 보다 큰 수여야 합니다") } }
        }
        if let c = q["caps"] { if c is [String: Any] { d["caps"] = c } else { problems.append("단위 \(n) 의 caps 는 객체여야 합니다") } }
        if q["tickets"] != nil, !(q["tickets"] is [Any]) { problems.append("단위 \(n) 의 tickets 는 배열이어야 합니다") }
        units.append(d)
        queue[i]["id"] = d["id"]; queue[i]["tickets"] = d["tickets"]; queue[i]["status"] = "pending"
    }
    f["units"] = units
    return (problems, f, queue)
}

func cmdStart(_ args: [String]) -> Never {
    let dir = runDir(args.first { !$0.hasPrefix("--") })
    let dev = args.contains("--dev")
    let id = (dir as NSString).lastPathComponent
    guard var run = readJSON(dir + "/run.json") as? [String: Any] else { fail("run.json 을 읽지 못했습니다") }
    guard run["state"] == nil else { fail("이미 출발한 실행입니다(상태 \(run["state"]!)). 새 실행 폴더로 출발하세요.") }
    let (problems, frozen, queue) = prepare(dir, dev: dev)
    guard problems.isEmpty else { fail("출발하지 않았습니다:\n" + problems.map { "  - " + $0 }.joined(separator: "\n")) }
    let label = (dev ? "com.claude.board.dev" : "com.claude.board") + ".autopilot." + id
    let plist = plistPath(label)
    guard !fm.fileExists(atPath: plist), !launchdLoaded(label) else { fail("같은 이름의 LaunchAgent 가 이미 있습니다: \(label)") }
    // 자기 사본과 고정 설정을 실행 폴더에 둔다(엔진만 쓰는 0700 폴더). 실행 중에 앱을 다시 깔거나 업데이트해도 이 실행의 엔진은 바뀌지 않고,
    // 엔진은 실행 중에 run.json·queue.json 의 설정 칸을 읽지 않는다. 고정 설정의 해시는 LaunchAgent 인자로만 넘겨 다시 뜰 때 견준다
    let pinned = dir + "/pinned/claude-autopilot"
    for d in ["/pinned", "/engine", "/units"] {
        try? fm.createDirectory(atPath: dir + d, withIntermediateDirectories: true)
    }
    chmod(dir + "/pinned", 0o700); chmod(dir + "/engine", 0o700)
    try? fm.removeItem(atPath: pinned)
    do { try fm.copyItem(atPath: selfPath(), toPath: pinned) } catch { fail("엔진 사본을 만들지 못했습니다: \(error.localizedDescription)") }
    let frozenData = Data(jsonText(frozen, pretty: true).utf8)
    guard writePrivate(dir + "/pinned/frozen.json", frozenData) else { fail("고정 설정을 쓰지 못했습니다") }
    // 근거 묶음도 출발 때 사본을 떠 둔다(출발 뒤에 원본이 바뀌어도 단위가 받는 묶음은 같다). 단위를 시작할 때 units/<n>/context/ 로 넘긴다
    try? fm.removeItem(atPath: dir + "/pinned/context")
    for (i, d) in (frozen["units"] as? [[String: Any]] ?? []).enumerated() {
        guard let src = d["context"] as? String else { continue }
        try? fm.createDirectory(atPath: dir + "/pinned/context", withIntermediateDirectories: true)
        do { try fm.copyItem(atPath: src, toPath: dir + "/pinned/context/\(i + 1)") } catch { fail("단위 \(i + 1) 의 근거 묶음을 복사하지 못했습니다: \(error.localizedDescription)") }
    }
    let sha = sha256(frozenData)
    writeJSON(dir + "/queue.json", queue)
    run["state"] = "starting"; run["reason"] = "엔진을 띄우는 중입니다."
    run["launchd_label"] = label; run["dev"] = dev; run["created"] = r3(now())
    run["sessions"] = [] as [Any]; run["unit"] = NSNull(); run["wait_until"] = NSNull(); run["updated"] = r3(now())
    writeJSON(dir + "/run.json", run)
    let job: [String: Any] = [
        "Label": label, "ProgramArguments": [pinned, "run", dir, sha], "WorkingDirectory": dir,
        "RunAtLoad": true, "KeepAlive": ["SuccessfulExit": false], "ThrottleInterval": 10,
        // 빌드·시험을 터미널에서처럼 제 속도로 돌리려고(지정하지 않으면 가벼운 자원 제한이 걸린다)
        "ProcessType": "Interactive",
        "StandardOutPath": dir + "/engine.log", "StandardErrorPath": dir + "/engine.log"]
    try? fm.createDirectory(atPath: home + "/Library/LaunchAgents", withIntermediateDirectories: true)
    guard let data = try? PropertyListSerialization.data(fromPropertyList: job, format: .xml, options: 0),
          (try? data.write(to: URL(fileURLWithPath: plist))) != nil else { fail("LaunchAgent 파일을 쓰지 못했습니다: \(plist)") }
    let r = runTool("/bin/launchctl", ["bootstrap", "gui/\(uid())", plist])
    guard r.code == 0 else {
        try? fm.removeItem(atPath: plist)
        run["state"] = "error"; run["reason"] = "launchd 에 올리지 못했습니다."
        writeJSON(dir + "/run.json", run)
        fail("launchctl bootstrap 이 실패했습니다(코드 \(r.code))")
    }
    print("출발했습니다: \(label)\n상태 보기: \(pinned) status \(dir)")
    exit(0)
}

func cmdRun(_ args: [String]) -> Never {
    let dir = runDir(args.first)
    chdir(dir)
    signal(SIGPIPE, SIG_IGN)
    signal(SIGTERM) { _ in
        if finishing != 0 { _exit(0) }
        gotTerm = 1
    }
    // 한 실행 폴더에 엔진 하나 — 손으로 띄운 것과 launchd 가 띄운 것이 겹치지 않게
    let lock = open(dir + "/.engine.lock", O_RDWR | O_CREAT, 0o644)
    guard lock >= 0, flock(lock, LOCK_EX | LOCK_NB) == 0 else { elog("이미 이 실행 폴더의 엔진이 돌고 있습니다"); exit(0) }
    let e = Engine(dir)
    guard e.load() else {
        elog("run.json·queue.json 을 읽지 못했습니다")
        e.cleanupAndExit()
    }
    guard args.count >= 2, e.loadFrozen(args[1]) else { e.frozenChanged() }
    e.syncFromFrozen()
    e.main()
}

func cmdControl(_ args: [String]) -> Never {
    guard args.count >= 2, ["pause", "resume", "stop"].contains(args[1]) else { usage() }
    let dir = runDir(args[0])
    guard let run = readJSON(dir + "/run.json") as? [String: Any] else { fail("run.json 을 읽지 못했습니다") }
    let s = run["state"] as? String ?? ""
    if ["stopped", "done", "error"].contains(s) { print("이미 끝난 실행입니다(\(stateLabel[s] ?? s))."); exit(0) }
    writeJSON(dir + "/control.json", ["action": args[1], "ts": r3(now())])
    print("보냈습니다: \(["pause": "일시정지", "resume": "재개", "stop": "중단"][args[1]]!). 엔진이 1초 안에 받습니다.")
    exit(0)
}

// 출발 전 실행 폴더: 확인 결과와 고정될 값, 출발 명령 한 줄
func statusBeforeStart(_ dir: String, _ run: [String: Any]) -> Never {
    let dev = (run["claude_args"] != nil || run["claude_path"] != nil)
    let (problems, f, _) = prepare(dir, dev: dev)
    var out = "실행 \((dir as NSString).lastPathComponent) · 출발 전\(run["title"].map { " · \($0)" } ?? "")\n"
    out += problems.isEmpty ? "출발 준비 확인: 이상 없음\n" : "출발 준비 확인: 고칠 것 \(problems.count)개\n" + problems.map { "  - \($0)\n" }.joined()
    let sl = f["stopline"] as? [String: Any] ?? [:]
    let pct = { (k: String, d: Double) in "\(Int(((num(sl[k]) ?? d) * 100).rounded()))%" }
    out += "모델 \(f["model"] ?? "?")\((f["effort"] as? String).map { " · effort \($0)" } ?? "") · 멈출 선 주간 \(pct("seven_day", 0.80)) · 5시간 \(pct("five_hour", 0.95))\n"
    out += "쓰기 범위: 단위마다 작업 폴더 + \((f["write_roots"] as? [String] ?? []).joined(separator: ", ").ifEmpty("(더 없음)")) + 단위 폴더 + 임시 폴더\n"
    out += "네트워크: \((f["network_allow"] as? [String] ?? defaultNetwork).joined(separator: ", ").ifEmpty("(없음)"))\((f["local_binding"] as? Bool ?? false) ? " · localhost 모든 포트 열림" : "")\n"
    out += "읽기 금지: \((f["read_deny"] as? [String] ?? []).count)곳 + 엔진 전용 뿌리(실제·시험 판), 모든 실행의 engine 폴더, 옛 뿌리(~/.claude/notifier/autopilot)\n"
    if let e = f["env_pass"] as? [String], !e.isEmpty { out += "더 넘길 환경 변수: \(e.joined(separator: ", "))\n" }
    if let x = f["sandbox_exclude"] as? [String], !x.isEmpty { out += "샌드박스 밖에서 돌 명령: \(x.joined(separator: ", "))\n" }
    let dc = f["caps"] as? [String: Any] ?? [:]
    for (i, d) in (f["units"] as? [[String: Any]] ?? []).enumerated() {
        let c = d["caps"] as? [String: Any] ?? [:]
        func cap(_ k: String, _ def: Double) -> String { let v = num(c[k]) ?? num(dc[k]) ?? def; return v == v.rounded() ? String(Int(v)) : String(v) }
        out += "  \(i + 1). \(d["title"] ?? "")  [상한 \(cap("minutes", 120))분 · \(cap("usd", 100)) USD · 주간 \(cap("weekly_pp", 5))%p]\n"
        out += "     작업 폴더: \(d["cwd"] ?? "?")\n"
        if let p = d["precheck"] as? String { out += "     시험 환경 확인: \(short(p, 160))\n" }
        if let p = d["action"] as? String { out += "     고정 단계(엔진, 모델 전): \(short(p, 160))\n" }
        if let p = d["context"] as? String {
            let k = (fm.enumerator(atPath: p)?.allObjects as? [String] ?? []).filter { !isDir(p + "/" + $0) }.count
            out += "     근거 묶음(읽기 전용으로 넘김): \(p) (파일 \(k)개)\n"
        }
        out += "     판정: \(short(d["check"] as? String ?? "", 160))" + ((d["check_timeout_min"]).map { " (\(num($0).map { String(Int($0)) } ?? "?")분 상한)" } ?? "") + "\n"
    }
    out += "출발: \"\(selfPath())\" start \"\(dir)\"\(dev ? " --dev" : "")\n"
    print(out, terminator: "")
    exit(problems.isEmpty ? 0 : 1)
}

extension String { func ifEmpty(_ s: String) -> String { isEmpty ? s : self } }

func cmdStatus(_ args: [String]) -> Never {
    let dir = runDir(args.first)
    guard let run = readJSON(dir + "/run.json") as? [String: Any] else { fail("run.json 을 읽지 못했습니다") }
    if run["state"] == nil { statusBeforeStart(dir, run) }
    let queue = readJSON(dir + "/queue.json") as? [[String: Any]] ?? []
    let s = run["state"] as? String ?? "-"
    var out = "실행 \((dir as NSString).lastPathComponent) · \(stateLabel[s] ?? s)"
    if let r = run["reason"] as? String, !r.isEmpty { out += ": \(r)" }
    out += "\n"
    if let pid = num(run["engine_pid"]).map({ pid_t($0) }), !["stopped", "done", "error"].contains(s) {
        out += "엔진 pid \(pid) (\(kill(pid, 0) == 0 ? "살아 있음" : "없음"))"
        if let l = run["launchd_label"] as? String { out += " · launchd \(launchdLoaded(l) ? "올라 있음" : "없음")" }
        out += "\n"
    }
    if let u = run["usage"] as? [String: Any] {
        let p = { (k: String) in num(u[k]).map { "\(Int(($0 * 100).rounded()))%" } ?? "?" }
        out += "계정 사용률: 5시간 \(p("five_hour")) · 주간 \(p("seven_day"))\n"
    }
    for (i, q) in queue.enumerated() {
        let n = i + 1
        let st = q["status"] as? String ?? "pending"
        let u = readJSON(dir + "/units/\(n)/engine.json") as? [String: Any] ?? [:]
        var line = "  \(n)/\(queue.count) \(unitLabel[st] ?? st)  \(q["title"] as? String ?? "")"
        if !u.isEmpty {
            let caps = u["caps"] as? [String: Any] ?? [:]
            line += "  · \(dur(num(u["active_sec"]) ?? 0))"
            if st == "running", let m = num(caps["minutes"]) { line += " / \(dur(m * 60))" }
            line += " · \(usd(num(u["cost_usd"]) ?? 0)) USD"
            if let k = num(u["try"]), k > 0 { line += " · 다시 \(Int(k))번" }
        }
        out += line + "\n"
    }
    let events = ((try? String(contentsOfFile: dir + "/events.jsonl", encoding: .utf8)) ?? "").split(separator: "\n").suffix(8)
    if !events.isEmpty { out += "최근 기록:\n" }
    let who = ["board": "보드", "model": "모델", "engine": "엔진"]
    for l in events {
        guard let o = (try? JSONSerialization.jsonObject(with: Data(l.utf8))) as? [String: Any] else { continue }
        let t = num(o["ts"]).map { clockFmt.string(from: Date(timeIntervalSince1970: $0)) } ?? ""
        out += "  \(t) \(who[o["who"] as? String ?? ""] ?? "?"): \(o["text"] as? String ?? "")\n"
    }
    print(out, terminator: "")
    exit(0)
}

let argv = Array(CommandLine.arguments.dropFirst())
switch argv.first {
case "start": cmdStart(Array(argv.dropFirst()))
case "run": cmdRun(Array(argv.dropFirst()))
case "control": cmdControl(Array(argv.dropFirst()))
case "status": cmdStatus(Array(argv.dropFirst()))
default: usage()
}
