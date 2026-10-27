// Claude Board 자율주행 엔진(실행 파일 claude-autopilot) — 보드 앱 밖에서 돈다
// 실행 상태의 주인은 엔진 하나다. 엔진이 죽으면 launchd 가 다시 띄우고(KeepAlive), 엔진은 상태 파일에서 이어 간다.
// 설계·파일 약속·실측: docs/자율주행/설계.md, docs/자율주행/진행 상황.md
//
// 사용법: claude-autopilot start <실행 폴더> [--dev]                 run.json·queue.json 확인 → 고정 사본 → LaunchAgent 올리기
//        claude-autopilot run <실행 폴더> <고정 설정 해시>            launchd 가 부르는 본체(해시는 start 가 LaunchAgent 인자로 넣는다)
//        claude-autopilot control <실행 폴더> pause|resume|stop      control.json 쓰기(보드도 같은 파일을 쓴다)
//        claude-autopilot status <실행 폴더> [--dev]                  사람이 읽는 한 화면 요약
//        claude-autopilot records import [--dev|--db <파일>] [--project <이름>] [--unit-key <칸 경로>] <원본…>
//                                                                   실행 기록 저장소(SQLite)에 실행 폴더·일반 꼴 대기열을 옮겨 담기
//        claude-autopilot prerun <실행 폴더> [--dev]                  출발 전 판정 미리 돌리기(prerun.json)
//        claude-autopilot finish <실행 폴더> merge --into <가지> [--story <키>] | promote <가지> --into <가지> | discard [--story <키>]
//                                                                   끝난 실행의 가지 승격(사람이 부른다. 엔진이 스스로 부르지 않는다)
import Foundation
import Darwin
import CryptoKit
import SQLite3

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
func runTool(_ path: String, _ args: [String], cwd: String? = nil, timeout: Double = 30, withErr: Bool = false) -> (code: Int32, out: String) {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: path)
    p.arguments = args
    if let cwd { p.currentDirectoryURL = URL(fileURLWithPath: cwd) }
    let pipe = Pipe()
    p.standardOutput = pipe
    p.standardError = withErr ? pipe : FileHandle.nullDevice
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
// 프로젝트 자율주행 세트(지시 autopilot.md, 주제로 시작이면 spec.md·stack.json)의 지금 판. 엔진 전용 뿌리 아래라 모델은 읽지도 쓰지도 못한다
func setDir(project: String, dev: Bool) -> String { privateRoot(dev: dev) + "/projects/\(project)/set" }
// 실행을 넘어 쌓이는 기록(설계 5장 7절). 엔진들이 함께 쓰고 보드는 읽기 전용으로 연다
func recordsPath(dev: Bool) -> String { privateRoot(dev: dev) + "/records.sqlite" }
func validName(_ s: String) -> Bool { s.range(of: "^[A-Za-z0-9._-]+$", options: .regularExpression) != nil }
func isLink(_ p: String) -> Bool { (try? fm.attributesOfItem(atPath: p)[.type] as? FileAttributeType) == .typeSymbolicLink }
func mkdirPrivate(_ p: String) {
    try? fm.createDirectory(atPath: p, withIntermediateDirectories: true)
    chmod(p, 0o700)
}

// 엔진이 샌드박스 밖에서 부르는 git 의 보호(보안 검토 2026-10-22 둘째): 저장소 설정의 fsmonitor·hooks·attributesFile·gpgsign 을 쓰지 않고,
// 시스템·전역 설정과 전역 attributes·replace 객체도 읽지 않는다 — 모델이 저장소 설정·attributes·replace 에 명령을 심어도 밖의 git 이 실행하지 않게
let gitHardenEnv = ["GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "GIT_ATTR_NOSYSTEM=1", "GIT_NO_REPLACE_OBJECTS=1"]
let gitHardenC = ["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "core.attributesFile=/dev/null"]
func safeGit(_ args: [String], cwd: String) -> String {
    runTool("/usr/bin/env", gitHardenEnv + ["/usr/bin/git", "--no-optional-locks"] + gitHardenC + args, cwd: cwd).out
}
// 엔진이 가지를 바꾸는 git(만들기·바꾸기·병합 커밋·ref 올리기): safeGit 과 같은 보호에 종료 코드와 오류 출력까지 돌려준다.
// 전역 설정을 읽지 않으므로 엔진이 만드는 커밋에는 사용자 신원을 명령줄로 붙인다(없으면 "Please tell me who you are")
func gitRun(_ args: [String], cwd: String, commit: Bool = false, withErr: Bool = true) -> (code: Int32, out: String) {
    var a = gitHardenEnv + ["/usr/bin/git"] + gitHardenC
    if commit { let i = gitIdentity(cwd); a += ["-c", "user.name=\(i.name)", "-c", "user.email=\(i.email)"] }
    let r = runTool("/usr/bin/env", a + args, cwd: cwd, timeout: 120, withErr: withErr)
    return (r.code, r.out.trimmingCharacters(in: .whitespacesAndNewlines))
}
// 사용자 git 신원(값만 읽는다 — git config --get 은 아무것도 실행하지 않는다). 없으면 엔진 이름
func gitIdentity(_ cwd: String) -> (name: String, email: String) {
    func get(_ k: String) -> String? {
        let v = runTool("/usr/bin/git", ["config", "--get", k], cwd: cwd).out.trimmingCharacters(in: .whitespacesAndNewlines)
        return v.isEmpty ? nil : v
    }
    return (get("user.name") ?? "Claude Board autopilot", get("user.email") ?? "autopilot@claude-board.invalid")
}
func gitOut(_ args: [String], cwd: String) -> String? {   // 성공했을 때의 출력(한 줄이면 그 줄)
    let r = gitRun(args, cwd: cwd)
    return r.code == 0 ? r.out : nil
}
func firstLine(_ s: String) -> String { short(s, 160) }
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
- 커밋은 엔진이 걸어 둔 지금 작업 가지에만 하고, 한 일은 턴을 끝내기 전에 그 가지에 커밋해 둔다. 다른 가지로 바꾸거나(switch·checkout·rebase) 다른 가지를 옮기거나 지우거나 맞추지(branch -f·update-ref·merge·reset) 않는다. 다른 가지 맞추기는 끝난 뒤 사람이 바뀐 줄을 보고 보드에서 하는 일이라서다(샌드박스도 그런 쓰기를 막는다). 프로젝트 규칙이 dev·live·master 같은 가지에 커밋·병합하거나 가지를 맞추라고 하면, 일은 지금 작업 가지에 커밋하고 맞추기는 하지 않은 채 result.json 의 next_start 에 무엇을 어느 가지로 맞추려 했는지 적는다.
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

// MARK: - 사용자 하네스 사본과 지문

// 저장소의 하네스(CLAUDE.md·CLAUDE.local.md·.mcp.json·.claude/)는 출발 준비·실행·끝 어디서도 고치지 않는다(설계 2장 끝).
// 출발 때 엔진 전용 폴더에 사본과 지문을 남기고, 끝에 다시 재어 같은지만 적는다. 다르면 사람이 고쳤을 수 있어 되돌리지 않는다
let harnessNames: Set<String> = ["CLAUDE.md", "CLAUDE.local.md", ".mcp.json"]
let harnessWalkSkip: Set<String> = [".git", "node_modules", "build", ".gradle", "target", "dist", "out", ".idea", ".build", "DerivedData", "Pods", ".venv"]

// 하네스 파일 하나 읽기(엔진은 샌드박스 밖에서 돈다). 모델이 하위 CLAUDE.md·.claude/ 안에 FIFO·장치 파일 링크(/dev/zero)를 두면
// 따라가 읽다가 멈추거나 메모리를 다 쓴다(보안 검토) — 링크를 따라가지 않고(O_NOFOLLOW) 보통 파일만, 상한 안에서만 읽는다
let harnessFileCap = 1 << 20        // 파일 하나 1 MiB
let harnessTotalCap = 32 << 20      // 한 번 훑을 때 합계 32 MiB
enum HarnessRead { case data(Data), large(Int), other(String) }   // other: 종류(link:<대상>·fifo·socket·device·dir·other)
func readHarnessFile(_ p: String, budget: Int) -> HarnessRead? {
    var st = stat()
    guard lstat(p, &st) == 0 else { return nil }
    func kind(_ m: mode_t) -> String {
        switch m & S_IFMT {
        case S_IFLNK: return "link:" + ((try? fm.destinationOfSymbolicLink(atPath: p)) ?? "")
        case S_IFIFO: return "fifo"
        case S_IFSOCK: return "socket"
        case S_IFCHR, S_IFBLK: return "device"
        case S_IFDIR: return "dir"
        default: return "other"
        }
    }
    guard st.st_mode & S_IFMT == S_IFREG else { return .other(kind(st.st_mode)) }
    // lstat 과 open 사이에 바꿔치기해도 FIFO 에서 멈추지 않게 O_NONBLOCK, 열고 나서 다시 본다
    let fd = open(p, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
    guard fd >= 0 else { return errno == ELOOP ? .other(kind(S_IFLNK)) : nil }
    defer { close(fd) }
    guard fstat(fd, &st) == 0 else { return nil }
    guard st.st_mode & S_IFMT == S_IFREG else { return .other(kind(st.st_mode)) }
    let size = Int(st.st_size)
    guard size <= min(harnessFileCap, budget) else { return .large(size) }
    var d = Data(), buf = [UInt8](repeating: 0, count: 65536)
    while d.count <= size {
        let n = read(fd, &buf, buf.count)
        if n < 0, errno == EINTR { continue }
        if n <= 0 { break }
        d.append(contentsOf: buf[0..<n])
    }
    return d.count > min(harnessFileCap, budget) ? .large(d.count) : .data(d)   // 읽는 동안 커졌으면 크기만
}

struct HarnessScan {
    var files: [[String: Any]] = []     // [{folder, path, sha256, size}] — path 는 폴더 안 상대 경로. 보통 파일이 아니면 sha256 자리에 종류
    var claudeDirs: [String] = []       // 있는 .claude 폴더(절대 경로)
    var ccWrites: [String] = []         // 있는 .claude/.cc-writes 폴더(절대 경로)
    var base: String {                  // 지문 전체의 sha256(경로·sha256·크기 줄을 차례로)
        sha256(Data(files.map { "\($0["folder"] ?? "")\t\($0["path"] ?? "")\t\($0["sha256"] ?? "")\t\($0["size"] ?? "")\n" }.joined().utf8))
    }
}

// 폴더마다 깊이 4 까지 내려가며 하네스 파일을 모은다. `.claude/` 는 안을 모두 담되 worktrees/(작업 트리)·.cc-writes/(claude 의 임시 자리)는 뺀다
func scanHarness(_ folders: [String]) -> HarnessScan {
    var s = HarnessScan()
    var budget = harnessTotalCap
    // 보통 파일은 1차와 같은 지문 줄(sha256·크기). 상한을 넘으면 크기만("large"), 링크·FIFO·소켓·장치는 내용 없이 종류만 — 끝 검사에서 "바뀜"으로 잡힌다
    func add(_ folder: String, _ rel: String) {
        switch readHarnessFile(folder + "/" + rel, budget: budget) {
        case .data(let d)?:
            budget -= d.count
            s.files.append(["folder": folder, "path": rel, "sha256": sha256(d), "size": d.count])
        case .large(let n)?: s.files.append(["folder": folder, "path": rel, "sha256": "large", "size": n])
        case .other(let k)? where k != "dir": s.files.append(["folder": folder, "path": rel, "sha256": k, "size": 0])
        default: break
        }
    }
    for folder in folders {
        var level = [""], depth = 0
        while !level.isEmpty, depth <= 4 {
            var next: [String] = []
            for rel in level {
                let d = rel.isEmpty ? folder : folder + "/" + rel
                for name in ((try? fm.contentsOfDirectory(atPath: d)) ?? []).sorted() {
                    let r = rel.isEmpty ? name : rel + "/" + name
                    let p = folder + "/" + r
                    if name == ".claude", isDir(p), !isLink(p) {
                        s.claudeDirs.append(p)
                        if isDir(p + "/.cc-writes"), !isLink(p + "/.cc-writes") { s.ccWrites.append(p + "/.cc-writes") }
                        // worktrees/ 안은 프로젝트 흐름이 만든 git 작업 트리(빌드 결과까지)일 수 있어 내려가지 않는다. enumerator 는 링크 폴더로 내려가지 않는다(실측)
                        var inner: [String] = []
                        if let en = fm.enumerator(atPath: p) {
                            while let x = en.nextObject() as? String {
                                if x == "worktrees" || x == ".cc-writes" { en.skipDescendants(); continue }
                                inner.append(x)
                            }
                        }
                        // 폴더만 빼고 모두 add 에 넘긴다(isDir 은 링크를 따라가므로 쓰지 않는다 — 폴더를 가리키는 링크도 종류로 남는다)
                        for f in inner.sorted() where !(isDir(p + "/" + f) && !isLink(p + "/" + f)) { add(folder, r + "/" + f) }
                    } else if harnessNames.contains(name), isLink(p) || !isDir(p) {
                        add(folder, r)
                    } else if isDir(p), !isLink(p), !harnessWalkSkip.contains(name) {
                        next.append(r)
                    }
                }
            }
            level = next; depth += 1
        }
    }
    return s
}

// 단위 작업 폴더들의 저장소 뿌리(저장소가 아니면 그 폴더). 같은 뿌리는 한 번만
func harnessFolders(_ cwds: [String]) -> [String] {
    var out: [String] = []
    for c in cwds {
        let top = safeGit(["rev-parse", "--show-toplevel"], cwd: c).trimmingCharacters(in: .whitespacesAndNewlines)
        let f = top.hasPrefix("/") && isDir(top) ? canonical(top) : c
        if !out.contains(f) { out.append(f) }
    }
    return out
}

// 출발 때: 사본(<엔진 전용 폴더>/harness/<폴더 번호>/<상대 경로>)과 지문(manifest.json)을 남기고 run.json 에 넣을 요약을 돌려준다
func harnessSnapshot(_ folders: [String], to dir: String) -> [String: Any] {
    try? fm.removeItem(atPath: dir)
    mkdirPrivate(dir)
    let s = scanHarness(folders)
    var budget = harnessTotalCap
    for f in s.files {
        guard let folder = f["folder"] as? String, let rel = f["path"] as? String, let i = folders.firstIndex(of: folder) else { continue }
        guard case .data(let d)? = readHarnessFile(folder + "/" + rel, budget: budget) else { continue }   // 보통 파일만 사본을 뜬다
        budget -= d.count
        let dst = dir + "/\(i + 1)/" + rel
        try? fm.createDirectory(atPath: (dst as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        writePrivate(dst, d)
    }
    let manifest: [String: Any] = ["folders": folders, "files": s.files, "claude_dirs": s.claudeDirs, "cc_writes": s.ccWrites,
                                   "base": s.base, "at": r3(now())]
    writePrivate(dir + "/manifest.json", Data(jsonText(manifest, pretty: true).utf8))
    return ["base": s.base, "files": s.files.count, "folders": folders, "copy": dir]
}

// 끝에: 출발 때 없던 빈 .claude/.cc-writes 와 그것만 들어 있던 .claude 를 치우고, 다시 재어 출발 때와 견준다(사본·지문은 manifest 만 읽는다 — 고정 설정 없이도 된다)
func harnessCheck(manifestDir dir: String) -> [String: Any]? {
    guard let m = readJSON(dir + "/manifest.json") as? [String: Any], let folders = m["folders"] as? [String] else { return nil }
    let hadClaude = Set(m["claude_dirs"] as? [String] ?? []), hadCC = Set(m["cc_writes"] as? [String] ?? [])
    var cleaned: [String] = []
    for cc in scanHarness(folders).ccWrites where !hadCC.contains(cc) && (try? fm.contentsOfDirectory(atPath: cc))?.isEmpty == true {
        guard rmdir(cc) == 0 else { continue }
        cleaned.append(cc)
        let parent = (cc as NSString).deletingLastPathComponent
        if !hadClaude.contains(parent), (try? fm.contentsOfDirectory(atPath: parent))?.isEmpty == true, rmdir(parent) == 0 { cleaned.append(parent) }
    }
    let s = scanHarness(folders)
    func key(_ f: [String: Any]) -> String { "\(f["folder"] ?? "")/\(f["path"] ?? "")" }
    var before: [String: String] = [:], after: [String: String] = [:]
    for f in m["files"] as? [[String: Any]] ?? [] { before[key(f)] = "\(f["sha256"] ?? "")" }
    for f in s.files { after[key(f)] = "\(f["sha256"] ?? "")" }
    var changed: [[String: Any]] = []
    for (k, v) in before.sorted(by: { $0.key < $1.key }) {
        if let a = after[k] { if a != v { changed.append(["path": k, "change": "changed"]) } } else { changed.append(["path": k, "change": "removed"]) }
    }
    for k in after.keys.sorted() where before[k] == nil { changed.append(["path": k, "change": "added"]) }
    return ["same": changed.isEmpty, "changed": changed, "checked": r3(now()), "cleaned": cleaned, "end_base": s.base]
}

// MARK: - 가지 (설계 2장 끝 "어디서 도는가"·"Jira 연결 프로젝트의 티켓과 가지")

// 원래 작업 트리 모드: 저장소마다 고른 출발 가지 머리에서 autopilot/<실행 id> 를 만들어 원래 작업 트리를 그 가지로 바꾸고(git switch -c),
// 끝에 깨끗하면 출발 때 가지로 되돌린다. 엔진이 스스로 움직이는 가지는 이 실행이 만든 autopilot/<실행 id> 와 Jira 스토리·작업 가지뿐이다.
// 따로 작업 트리(단일 저장소)는 다음 차례라 아직 없다

// 단위 작업 폴더·쓰기 범위가 든 git 저장소의 뿌리(같은 저장소는 한 번만). 저장소가 아닌 폴더(~/.gradle 등)는 뺀다
func gitRepos(_ paths: [String]) -> [String] {
    var out: [String] = []
    for p in paths {
        guard let top = gitOut(["rev-parse", "--show-toplevel"], cwd: p), top.hasPrefix("/") else { continue }
        let c = canonical(top)
        if !out.contains(c) { out.append(c) }
    }
    return out
}
// 가지 이름 → 머리 커밋(refname:short 는 같은 이름의 태그가 있으면 heads/ 를 붙이므로 전체 이름에서 뗀다)
func branchHeads(_ repo: String) -> [String: String] {
    var m: [String: String] = [:]
    for line in (gitOut(["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/heads/"], cwd: repo) ?? "").split(separator: "\n") {
        let f = line.split(separator: "\t")
        if f.count == 2, f[0].hasPrefix("refs/heads/") { m[String(f[0].dropFirst(11))] = String(f[1]) }
    }
    return m
}
func currentBranch(_ repo: String) -> String? { gitOut(["symbolic-ref", "-q", "--short", "HEAD"], cwd: repo).flatMap { $0.isEmpty ? nil : $0 } }
func headOf(_ repo: String, _ ref: String) -> String? { gitOut(["rev-parse", "-q", "--verify", ref + "^{commit}"], cwd: repo).flatMap { $0.isEmpty ? nil : $0 } }
func branchExists(_ repo: String, _ b: String) -> Bool { headOf(repo, "refs/heads/" + b) != nil }
func isAncestor(_ repo: String, _ a: String, _ b: String) -> Bool { gitRun(["merge-base", "--is-ancestor", a, b], cwd: repo).code == 0 }
// 작업 트리 상태 줄(추적 안 하는 파일 포함, 무시 파일 제외). 색인을 고쳐 쓰지 않게 --no-optional-locks
func porcelain(_ repo: String) -> [String] {
    (gitOut(["--no-optional-locks", "status", "--porcelain"], cwd: repo) ?? "").split(separator: "\n").map(String.init)
}
// 가지가 걸려 있는 작업 트리(가지 이름 → 작업 트리 경로)
func checkedOut(_ repo: String) -> [String: String] {
    var m: [String: String] = [:], wt = ""
    for line in (gitOut(["worktree", "list", "--porcelain"], cwd: repo) ?? "").split(separator: "\n") {
        if line.hasPrefix("worktree ") { wt = String(line.dropFirst(9)) }
        if line.hasPrefix("branch refs/heads/") { m[String(line.dropFirst(18))] = wt }
    }
    return m
}
func repoName(_ r: String) -> String { (r as NSString).lastPathComponent }
// Jira 가지 이름 꼴(프로젝트 설정 tickets.branch_names). 자리표 {type}·{story}·{task} 를 채운다.
// 예: story "{type}/_{story}/parent/{story}", task "{type}/_{story}/{task}". {type} 은 tickets.type_by_kind 에서 단위 종류로 찾는다
func renderBranch(_ pattern: String, type: String? = nil, story: String? = nil, task: String? = nil) -> String {
    var s = pattern
    if let t = type { s = s.replacingOccurrences(of: "{type}", with: t) }
    if let st = story { s = s.replacingOccurrences(of: "{story}", with: st) }
    if let tk = task { s = s.replacingOccurrences(of: "{task}", with: tk) }
    return s
}
func validBranch(_ b: String, cwd: String) -> Bool { gitRun(["check-ref-format", "--branch", b], cwd: cwd).code == 0 }
// 프로젝트 설정(설계 5장 5절 projects/<프로젝트>.json): 승격 차례 promotion, 티켓·가지 규칙 tickets
func projectSettings(_ name: String, dev: Bool) -> [String: Any]? { readJSON(autopilotRoot(dev: dev) + "/projects/\(name).json") as? [String: Any] }
let defaultStoryPattern = "features/parent/{story}", defaultTaskPattern = "features/{task}"

// 고정 설정의 출발 가지(run.json base_branch: 가지 이름 하나 또는 {저장소 경로: 가지}). 없으면 지금 걸린 가지
func baseBranch(_ f: [String: Any], _ repo: String) -> String? {
    if let s = f["base_branch"] as? String { return s }
    if let m = f["base_branch"] as? [String: Any], let s = m[repo] as? String { return s }
    return currentBranch(repo)
}

// git 폴더의 장치·inode(보안: 출발 때와 같은지 verifyGitDir 가 견준다)
func gitStat(_ p: String) -> String? {
    var st = stat(); guard lstat(p, &st) == 0 else { return nil }
    return "\(st.st_dev):\(st.st_ino)"
}
// replace ref·tag·shallow 묶음(보안: 실행 중 새 replace·shallow·tag 변화를 끝에 보고하려고)
func extraRefs(_ repo: String, _ common: String) -> [String: Any] {
    var tags: [String: String] = [:], replace: [String] = []
    for line in (gitOut(["for-each-ref", "--format=%(refname)\t%(objectname)", "refs/tags/", "refs/replace/"], cwd: repo) ?? "").split(separator: "\n") {
        let f = line.split(separator: "\t"); guard f.count == 2 else { continue }
        if f[0].hasPrefix("refs/tags/") { tags[String(f[0].dropFirst(10))] = String(f[1]) }
        else if f[0].hasPrefix("refs/replace/") { replace.append(String(f[0].dropFirst(13))) }
    }
    let shallow = fm.contents(atPath: common + "/shallow").map { sha256($0) }
    return ["tags": tags, "replace": replace.sorted(), "shallow": shallow ?? NSNull()]
}
// 경로의 실제 자리: 있는 가장 깊은 조상까지 realpath 로 풀고 나머지를 붙인다(없는 파일도 /tmp → /private/tmp·바로 가기 폴더를 푼 자리로).
// 없는 대상을 가리키는 바로 가기도 그 대상 자리로 푼다(모델이 그 대상을 만들 수 있다)
func physicalPath(_ p: String, hops: Int = 0) -> String {
    var head = p, tail: [String] = []
    while true {
        if let r = realpath(head, nil) {
            defer { free(r) }
            return URL(fileURLWithPath: ([String(cString: r)] + tail.reversed()).joined(separator: "/")).standardizedFileURL.path
        }
        if hops < 32, let dest = try? fm.destinationOfSymbolicLink(atPath: head) {
            let target = dest.hasPrefix("/") ? dest : (head as NSString).deletingLastPathComponent + "/" + dest
            return physicalPath(([target] + tail.reversed()).joined(separator: "/"), hops: hops + 1)
        }
        let up = (head as NSString).deletingLastPathComponent
        if up == head || up.isEmpty { return URL(fileURLWithPath: p).standardizedFileURL.path }
        tail.append((head as NSString).lastPathComponent); head = up
    }
}
// include 가 가리키면 위험한 자리(모델 쓰기 범위): 단위 작업 폴더·write_roots 에 더해 저장소 git 폴더(cwd 가 작업 트리 안이면 Claude Code 가
// 공통 .git 을 자동 허용, 실측), 따로 작업 트리 뿌리, 실행 폴더의 단위 폴더, 임시 폴더(샌드박스 기본 허용 /tmp·/var/tmp 와 사용자 TMPDIR)
func includeScope(_ paths: [String], gitDirs: [String], runDir: String) -> [String] {
    paths + gitDirs + [home + "/.claude-board/worktrees", runDir + "/units", "/tmp", "/private/tmp", "/var/tmp", "/private/var/tmp", "/var/folders", "/private/var/folders"]
}
// 저장소 config 의 include·includeIf·filter(보안): git 이 해석한 대로 읽는다(git config --file <파일> --no-includes --null --list).
// 줄 단위로 직접 읽으면 같은 줄 키·따옴표·줄 끝 주석·줄 잇기를 git 과 다르게 읽는다(보안 검토 2026-10-22 셋째). 읽는 파일은 공통 config·config.worktree
// 와 (작업 트리면) 그 git 폴더의 config.worktree. include 는 git 규칙대로(그 파일이 있는 폴더 기준 상대 경로, ~ 풀기) 중첩까지 따라간다(방문 집합,
// 깊이 10 = git 상한). scope 안을 가리키면 risky — 모델이 그 파일에 filter 를 심으면 엔진 밖 git 이 실행한다. 범위 밖 파일이 다시 범위 안을
// include 해도, git 이 읽지 못하는 파일·깊이를 넘는 중첩도 risky. snapshot 은 저장소 자리 파일의 include·filter 만(밖 파일은 사람이 고칠 수 있어 뺀다)
func configIncludes(_ common: String, gitDir: String, scope: [String]) -> (snapshot: [String], risky: [String]) {
    var snap: [String] = [], risky: [String] = [], seen = Set<String>()
    let sc = Set(scope.flatMap { [$0, physicalPath($0)] }.map { $0.lowercased() })   // 대소문자를 가리지 않는 볼륨이라 소문자로 견준다
    func inScope(_ p: String) -> Bool {
        [URL(fileURLWithPath: p).standardizedFileURL.path, physicalPath(p)].map { $0.lowercased() }
            .contains { f in sc.contains { f == $0 || f.hasPrefix($0 + "/") } }
    }
    func walk(_ file: String, _ depth: Int, root: Bool) {
        guard fm.fileExists(atPath: file), seen.insert(physicalPath(file)).inserted else { return }
        // 표준 오류는 섞지 않는다(경고 줄이 첫 키에 붙으면 include 를 놓친다). cwd 는 / — 저장소를 찾지 않게
        let r = gitRun(["config", "--file", file, "--no-includes", "--null", "--list"], cwd: "/", withErr: false)
        guard r.code == 0 else { risky.append("파일을 git 이 읽지 못해(\(file))"); return }
        let out = r.out
        let dir = (physicalPath(file) as NSString).deletingLastPathComponent
        for entry in out.split(separator: "\0") {
            let nl = entry.firstIndex(of: "\n")   // 키\n값, 값 없는 키(불리언)는 줄바꿈이 없다
            let k = String(entry[..<(nl ?? entry.endIndex)]).lowercased(), v = nl.map { String(entry[entry.index(after: $0)...]) }
            let inc = k == "include.path" || (k.hasPrefix("includeif.") && k.hasSuffix(".path"))
            if root, inc || k.hasPrefix("filter.") { snap.append(file + "\t" + k + (v.map { "=" + $0 } ?? "")) }
            guard inc, let v, !v.isEmpty else { continue }
            let p = v.hasPrefix("~") ? (v as NSString).expandingTildeInPath : (v.hasPrefix("/") ? v : dir + "/" + v)
            if inScope(p) { risky.append("include 가 모델 쓰기 범위 안(\(p))을 가리켜") }
            else if depth >= 10 { risky.append("include 가 10단 넘게 겹쳐(\(p))") }
            else { walk(p, depth + 1, root: false) }
        }
    }
    for f in [common + "/config", common + "/config.worktree"] + (gitDir != common ? [gitDir + "/config.worktree"] : []) { walk(f, 0, root: true) }
    return (snap, risky)
}
// config.worktree 지문(보안 2026-10-22 셋째): extensions.worktreeConfig 가 켜진 저장소면 git 이 읽는다. 모델이 filter·fsmonitor 를 심으면 엔진 밖
// git(남은 변경 커밋의 add 등)이 실행할 수 있어 거부 목록으로 막고, 끝 검사가 출발 때와 견준다(바로 가기·FIFO 는 따라가지 않고 1 MiB 상한)
func worktreeConfigPrints(_ common: String, gitDir: String) -> [String: String] {
    var d: [String: String] = [:]
    for p in Set([common, gitDir]).map({ $0 + "/config.worktree" }) {
        switch readHarnessFile(p, budget: harnessFileCap) {
        case .data(let x): d[p] = sha256(x)
        case .large(let n): d[p] = "large:\(n)"
        case .other(let k): d[p] = k
        case nil: d[p] = "none"
        }
    }
    return d
}
// 저장소 하나의 가지 기록 틀(두 모드 공통). workTree 가 있으면 따로 작업 트리 모드
func repoRecord(_ f: [String: Any], _ r: String, work: String, original: String?, workTree: String?, scope: [String]) -> [String: Any] {
    let wtCwd = workTree ?? r
    let gitDir = gitOut(["rev-parse", "--absolute-git-dir"], cwd: wtCwd) ?? r + "/.git"
    var common = gitOut(["rev-parse", "--git-common-dir"], cwd: r) ?? r + "/.git"
    if !common.hasPrefix("/") { common = canonical(r + "/" + common) }
    common = canonical(common)
    let base = baseBranch(f, r) ?? ""
    var one: [String: Any] = [
        "base": ["branch": base, "head": headOf(r, "refs/heads/" + base) ?? ""], "work": work,
        "original": original ?? NSNull(), "original_head": headOf(r, "HEAD") ?? "", "restored": NSNull(),
        "git_dir": canonical(gitDir), "common_dir": common, "common_ino": gitStat(common) ?? NSNull(),
        "dirty_start": porcelain(wtCwd), "on": work, "made": [work],
        "includes": configIncludes(common, gitDir: canonical(gitDir), scope: scope).snapshot,
        "config_worktree": worktreeConfigPrints(common, gitDir: canonical(gitDir)), "extra": extraRefs(r, common)]
    if let wt = workTree { one["work_tree"] = wt; one["work_git_dir"] = canonical(gitDir); one["work_git_ino"] = gitStat(canonical(gitDir)) ?? NSNull() }
    return one
}

// 원래 작업 트리 모드: 저장소마다 출발 가지에서 autopilot/<실행 id> 를 만들어 원래 작업 트리를 바꾼다(git switch -c). 하나라도 실패하면 되돌린다
func startBranches(_ f: [String: Any], id: String, scope: [String]) -> (branches: [String: Any], error: String?) {
    let work = "autopilot/" + id
    var info: [String: Any] = [:], start: [String: Any] = [:]
    for r in f["repos"] as? [String] ?? [] {
        let original = currentBranch(r)
        let base = baseBranch(f, r) ?? ""
        let one = repoRecord(f, r, work: work, original: original, workTree: nil, scope: scope)
        start[r] = branchHeads(r)
        let sw = gitRun(["switch", "-c", work, base], cwd: r)
        guard sw.code == 0 else {
            rollbackBranches(["repos": info])
            return (["repos": info], "\(r) 를 출발 가지 \(base) 에서 만든 \(work) 로 바꾸지 못했습니다: \(firstLine(sw.out))")
        }
        info[r] = one
    }
    return (["repos": info, "start": start, "stories": [:] as [String: Any]], nil)
}

// 따로 작업 트리 모드(단일 저장소): 출발 가지 머리에서 autopilot/<실행 id> 가지와 작업 트리(~/.claude-board/worktrees/<id>/<저장소>/)를 만든다.
// 원래 작업 트리·가지는 건드리지 않는다. 함께 둘 파일(git 이 무시하는 .env·데이터 등)을 복사하거나 읽기 전용으로 잇는다. 경로 바꾸기는 cmdStart 가 한다
func startWorktree(_ f: [String: Any], id: String, scope: [String]) -> (branches: [String: Any], worktree: [String: Any], wtPath: String, repo: String, error: String?) {
    let work = "autopilot/" + id
    guard let r = (f["repos"] as? [String])?.first else { return ([:], [:], "", "", "따로 작업 트리로 돌 저장소가 없습니다") }
    let base = baseBranch(f, r) ?? ""
    let wtBase = home + "/.claude-board/worktrees/" + id
    let wtPath = wtBase + "/" + repoName(r)
    if fm.fileExists(atPath: wtPath) { return ([:], [:], "", r, "작업 트리 자리가 이미 있습니다: \(wtPath)") }
    mkdirPrivate(home + "/.claude-board"); mkdirPrivate(home + "/.claude-board/worktrees"); mkdirPrivate(wtBase)
    _ = gitRun(["worktree", "prune"], cwd: r)
    let startHeads = branchHeads(r)   // 작업 가지를 만들기 전 머리(autopilot/<id> 가 "움직임" 아니라 "새로 생김"으로 잡히게)
    let add = gitRun(["worktree", "add", "-b", work, wtPath, base], cwd: r)
    guard add.code == 0 else {
        try? fm.removeItem(atPath: wtBase)
        return ([:], [:], "", r, "\(repoName(r)) 에 출발 가지 \(base) 에서 작업 트리를 만들지 못했습니다: \(firstLine(add.out))")
    }
    // 함께 둘 파일(git 이 무시하는 자리). 세트·run.json 의 worktree_files: ["경로"] 또는 [{path, link}] (복사 기본, link 는 읽기 전용 바로가기)
    for item in f["worktree_files"] as? [Any] ?? [] {
        let path = (item as? String) ?? ((item as? [String: Any])?["path"] as? String) ?? ""
        let link = (item as? [String: Any])?["link"] as? Bool ?? false
        guard !path.isEmpty, !path.hasPrefix("/"), !path.contains("..") else { continue }
        let src = r + "/" + path, dst = wtPath + "/" + path
        guard fm.fileExists(atPath: src) else { continue }
        try? fm.createDirectory(atPath: (dst as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        try? fm.removeItem(atPath: dst)
        if link { try? fm.createSymbolicLink(atPath: dst, withDestinationPath: src) }
        else { try? fm.copyItem(atPath: src, toPath: dst) }
    }
    let one = repoRecord(f, r, work: work, original: currentBranch(r), workTree: wtPath, scope: scope)
    // 작업 트리 git 폴더의 config.worktree(git 이 원래 것을 복사해 둠)는 상대 경로의 기준 폴더가 달라 출발 준비 확인과 다르게 풀릴 수 있어 다시 본다
    let gd = one["git_dir"] as? String ?? "", cd = one["common_dir"] as? String ?? ""
    if let p = configIncludes(cd, gitDir: gd, scope: scope + [wtPath, cd, gd]).risky.first {
        rollbackBranches(["repos": [r: one]])
        return ([:], [:], "", r, "작업 트리의 git config \(p) 작업 트리를 지웠습니다")
    }
    let branches: [String: Any] = ["repos": [r: one], "start": [r: startHeads], "stories": [:] as [String: Any]]
    let worktree: [String: Any] = ["mode": "worktree", "repo": r, "path": wtPath, "branch": work,
                                   "base": (one["base"] as? [String: Any])?["head"] ?? "",
                                   "reason": "단일 저장소라 따로 작업 트리에서 돕니다(원래 작업 트리는 사람이 그대로 씁니다)"]
    return (branches, worktree, wtPath, r, nil)
}

// 출발이 끝까지 가지 못했을 때: 바꾼 저장소를 출발 때 가지로 되돌리고 만든 가지·작업 트리를 지운다
func rollbackBranches(_ b: [String: Any]) {
    for (r, v) in b["repos"] as? [String: Any] ?? [:] {
        guard let i = v as? [String: Any], let work = i["work"] as? String else { continue }
        if let wt = i["work_tree"] as? String {   // 따로 작업 트리: 작업 트리를 지우고 prune 한 뒤 가지 삭제
            _ = gitRun(["worktree", "remove", "--force", wt], cwd: r)
            _ = gitRun(["worktree", "prune"], cwd: r)
            _ = gitRun(["branch", "-D", work], cwd: r)
            let base = (wt as NSString).deletingLastPathComponent
            if (try? fm.contentsOfDirectory(atPath: base))?.isEmpty == true { try? fm.removeItem(atPath: base) }
        } else {
            if let o = i["original"] as? String { _ = gitRun(["switch", o], cwd: r) }
            else if let h = i["original_head"] as? String, !h.isEmpty { _ = gitRun(["switch", "--detach", h], cwd: r) }
            _ = gitRun(["branch", "-D", work], cwd: r)
        }
    }
}

// 단위 경로를 작업 트리 쪽으로 바꾼다(cmdStart 가 작업 트리를 만든 뒤). cwd·write_roots 는 접두사 치환, 명령·프롬프트는 경계 치환, context 는 그대로
func remapToWorktree(_ f: inout [String: Any], from orig: String, to wt: String) {
    func prefix(_ s: String) -> String { s == orig ? wt : (s.hasPrefix(orig + "/") ? wt + String(s.dropFirst(orig.count)) : s) }
    func boundary(_ s: String) -> String {   // orig 뒤에 /·끝·따옴표·공백이 올 때만 바꾼다(toy vs toy2 가름)
        var out = "", i = s.startIndex
        while let rng = s.range(of: orig, range: i..<s.endIndex) {
            out += s[i..<rng.lowerBound]
            let after = rng.upperBound < s.endIndex ? s[rng.upperBound] : " "
            out += (after == "/" || after == " " || after == "\"" || after == "'" || after == "\n" || after == "`") ? wt : orig
            i = rng.upperBound
        }
        out += s[i...]
        return out
    }
    if let roots = f["write_roots"] as? [String] { f["write_roots"] = roots.map(prefix) }
    if var units = f["units"] as? [[String: Any]] {
        for j in units.indices {
            if let c = units[j]["cwd"] as? String { units[j]["cwd"] = prefix(c) }
            for k in ["check", "precheck", "action", "prompt"] { if let s = units[j][k] as? String { units[j][k] = boundary(s) } }
        }
        f["units"] = units
    }
}

// MARK: - 실행 기록 저장소 (SQLite, 설계 5장 7절)

// 실행을 넘어 쌓이는 기록. 엔진(여러 실행의 엔진이 함께)과 옮겨 담기가 쓰고 보드는 읽기 전용으로 연다. 자리는 엔진 전용 뿌리라 모델은 읽지 못한다.
// 쓰기는 모두 같은 키로 덮어쓰는 upsert 라 같은 실행을 몇 번 옮겨도 행이 늘지 않고, 한 번 실패해도 다음 쓰기가 실행 폴더에서 다시 옮긴다
struct DBError: Error, CustomStringConvertible {
    let description: String
    var busy = false
}

let SQLITE_TRANSIENT_ = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

final class RecordsDB {
    private var db: OpaquePointer?
    static let version: Int32 = 1
    static let tables = ["projects", "runs", "units", "queue_items", "cards", "carry", "sets", "tickets", "imports"]
    static let schemaV1 = """
    CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT UNIQUE, kind TEXT, repos TEXT, created REAL, updated REAL);
    CREATE TABLE runs (id TEXT PRIMARY KEY, project_id TEXT, title TEXT, source TEXT, continues TEXT, state TEXT, reason TEXT, model TEXT,
      effort TEXT, dev INTEGER, started REAL, ended REAL, units_total INTEGER, units_done INTEGER, units_failed INTEGER, units_capped INTEGER,
      units_stopped INTEGER, active_sec REAL, cost_usd REAL, weekly_pp REAL, worktree TEXT, branches TEXT, harness TEXT, set_id TEXT,
      epic TEXT, run_dir TEXT, origin TEXT, updated REAL);
    CREATE INDEX runs_project ON runs(project_id);
    CREATE TABLE units (run_id TEXT NOT NULL, n INTEGER NOT NULL, unit_id TEXT, title TEXT, kind TEXT, source_key TEXT, story_key TEXT,
      task_key TEXT, branch TEXT, status TEXT, merged TEXT, est_min REAL, est_basis TEXT, active_sec REAL, cost_usd REAL, weekly_pp REAL,
      turns INTEGER, tries INTEGER, check_exit INTEGER, denials TEXT, result TEXT, model TEXT, started REAL, ended REAL, PRIMARY KEY (run_id, n));
    CREATE INDEX units_source ON units(source_key);
    CREATE TABLE queue_items (project_id TEXT NOT NULL, source_key TEXT NOT NULL, title TEXT, kind TEXT, grp TEXT, est_min REAL, est_basis TEXT,
      depends TEXT, needs TEXT, state TEXT, done_run TEXT, done_unit INTEGER, origin TEXT, story_key TEXT, task_key TEXT, note TEXT,
      added REAL, updated REAL, PRIMARY KEY (project_id, source_key));
    CREATE TABLE cards (run_id TEXT NOT NULL, card_id TEXT NOT NULL, def TEXT, answer TEXT, policy INTEGER, answered REAL, PRIMARY KEY (run_id, card_id));
    CREATE TABLE carry (id TEXT PRIMARY KEY, run_id TEXT, unit_n INTEGER, kind TEXT, title TEXT, why TEXT, state TEXT, used_run TEXT);
    CREATE INDEX carry_run ON carry(run_id);
    CREATE TABLE sets (id TEXT PRIMARY KEY, project_id TEXT, digest TEXT UNIQUE, instruction TEXT, spec TEXT, stack TEXT, checks TEXT, harness TEXT,
      worktree_files TEXT, parent_id TEXT, origin TEXT, created REAL);
    CREATE TABLE tickets (project_id TEXT NOT NULL, key TEXT NOT NULL, kind TEXT, parent_key TEXT, title TEXT, url TEXT, source_key TEXT,
      created_run TEXT, created REAL, PRIMARY KEY (project_id, key));
    CREATE TABLE imports (path TEXT NOT NULL, sha256 TEXT NOT NULL, rows INTEGER, at REAL, PRIMARY KEY (path, sha256));
    """

    init(_ path: String) throws {
        let fresh = !fm.fileExists(atPath: path)
        try? fm.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        guard sqlite3_open_v2(path, &db, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX, nil) == SQLITE_OK else {
            let m = db.map { String(cString: sqlite3_errmsg($0)) } ?? "?"
            sqlite3_close_v2(db); db = nil
            throw DBError(description: "열지 못함(\(m))")
        }
        if fresh { chmod(path, 0o600) }
        sqlite3_busy_timeout(db, 5000)   // 다른 엔진이 쓰는 중이면 5초까지 기다린다(그 뒤는 transaction 이 다시 한다)
        try exec("PRAGMA journal_mode=WAL")
        let v = Int32(try rows("PRAGMA user_version").first.flatMap { num($0[0]) } ?? 0)
        if v > RecordsDB.version { throw DBError(description: "저장소 판(\(v))이 이 엔진이 아는 판(\(RecordsDB.version))보다 새롭습니다. 보드를 업데이트하세요") }
        if v < 1 {
            try transaction {
                // 다른 엔진이 먼저 만들었을 수 있다 — 잠금을 잡은 뒤 다시 본다
                if (try rows("PRAGMA user_version").first.flatMap { num($0[0]) } ?? 0) < 1 {
                    try exec(RecordsDB.schemaV1)
                    try exec("PRAGMA user_version=1")
                }
            }
        }
    }
    deinit { close() }
    // 마지막 연결이 닫힐 때 SQLite 가 WAL 을 본 파일로 접는다. exit 앞에서는 deinit 이 불리지 않아 직접 닫는다
    func close() { if db != nil { sqlite3_close_v2(db); db = nil } }

    private func err(_ rc: Int32) -> DBError {
        DBError(description: String(cString: sqlite3_errmsg(db)), busy: rc == SQLITE_BUSY || rc == SQLITE_LOCKED || (rc & 0xff) == SQLITE_BUSY)
    }

    func exec(_ sql: String) throws {
        let rc = sqlite3_exec(db, sql, nil, nil, nil)
        guard rc == SQLITE_OK else { throw err(rc) }
    }

    private func prepare(_ sql: String, _ args: [Any?]) throws -> OpaquePointer? {
        var st: OpaquePointer?
        let rc = sqlite3_prepare_v2(db, sql, -1, &st, nil)
        guard rc == SQLITE_OK else { throw err(rc) }
        for (i, a) in args.enumerated() {
            let k = Int32(i + 1)
            switch a {
            case nil: sqlite3_bind_null(st, k)
            case let s as String: sqlite3_bind_text(st, k, s, -1, SQLITE_TRANSIENT_)
            case let b as Bool: sqlite3_bind_int64(st, k, b ? 1 : 0)
            case let x as Int: sqlite3_bind_int64(st, k, Int64(x))
            case let x as Double: sqlite3_bind_double(st, k, x)
            default: sqlite3_bind_null(st, k)
            }
        }
        return st
    }

    @discardableResult
    func run(_ sql: String, _ args: [Any?] = []) throws -> Int {
        let st = try prepare(sql, args)
        defer { sqlite3_finalize(st) }
        var rc = sqlite3_step(st)
        while rc == SQLITE_ROW { rc = sqlite3_step(st) }
        guard rc == SQLITE_DONE else { throw err(rc) }
        return Int(sqlite3_changes(db))
    }

    func rows(_ sql: String, _ args: [Any?] = []) throws -> [[Any?]] {
        let st = try prepare(sql, args)
        defer { sqlite3_finalize(st) }
        var out: [[Any?]] = []
        var rc = sqlite3_step(st)
        while rc == SQLITE_ROW {
            out.append((0..<sqlite3_column_count(st)).map { c -> Any? in
                switch sqlite3_column_type(st, c) {
                case SQLITE_INTEGER: return Int(sqlite3_column_int64(st, c))
                case SQLITE_FLOAT: return sqlite3_column_double(st, c)
                case SQLITE_TEXT: return String(cString: sqlite3_column_text(st, c))
                default: return nil
                }
            })
            rc = sqlite3_step(st)
        }
        guard rc == SQLITE_DONE else { throw err(rc) }
        return out
    }

    // 쓰기 잠금을 먼저 잡고(BEGIN IMMEDIATE) 한 번에 쓴다. 다른 엔진과 겹쳐 잠겨 있으면 조금 쉬었다 다섯 번까지 다시 한다
    func transaction(_ body: () throws -> Void) throws {
        var tries = 0
        while true {
            do {
                try exec("BEGIN IMMEDIATE")
                do { try body(); try exec("COMMIT"); return } catch { _ = try? exec("ROLLBACK"); throw error }
            } catch let e as DBError where e.busy && tries < 5 {
                tries += 1
                usleep(useconds_t(300_000 * tries))
            }
        }
    }

    func count(_ table: String) -> Int { (try? rows("SELECT COUNT(*) FROM \(table)").first.flatMap { num($0[0]) }).flatMap { $0 }.map(Int.init) ?? -1 }
}

// 옮겨 담는 실행 하나
struct RunSource {
    var dir: String
    var origin: String              // engine(엔진이 실행 중에) | import(records import)
    var project: String?            // 옮겨 담기의 --project. 없으면 run.json·고정 설정의 project
    var unitKey: String?            // 단위의 원천 항목 키를 queue.json 단위의 어느 칸에서 읽을지(점으로 잇는 경로). 없으면 source_key
    var frozen: [String: Any]?      // 엔진은 자기가 읽은 고정 설정을 넘긴다. 없으면 pinned/frozen.json(없으면 queue.json)
}

func valueAt(_ path: String, _ o: [String: Any]) -> Any? {
    var cur: Any? = o
    for k in path.split(separator: ".") { cur = (cur as? [String: Any])?[String(k)] }
    return cur
}
func jtext(_ v: Any?) -> String? {
    guard let v, !(v is NSNull) else { return nil }
    return v as? String ?? jsonText(v)
}
func inum(_ v: Any?) -> Int? { num(v).map { Int($0) } }
func mtime(_ p: String) -> Double? { ((try? fm.attributesOfItem(atPath: p))?[.modificationDate] as? Date).map { r3($0.timeIntervalSince1970) } }

// 폴더 여럿의 공통 조상. 홈이나 그 위가 되면 쓰지 않는다(프로젝트 작업 경로로는 너무 넓다)
func commonRoot(_ paths: [String]) -> String? {
    guard var parts = paths.first?.split(separator: "/").map(String.init) else { return nil }
    for p in paths.dropFirst() {
        let q = p.split(separator: "/").map(String.init)
        var i = 0
        while i < min(parts.count, q.count), parts[i] == q[i] { i += 1 }
        parts = Array(parts.prefix(i))
    }
    let r = "/" + parts.joined(separator: "/")
    return r == "/" || r == home || home.hasPrefix(r + "/") ? nil : r
}

// 판정 스크립트 지문(출발 때 고정한 pinned/checks/): 경로·sha256·크기와 글
func checksFingerprint(_ dir: String) -> [[String: Any]] {
    (fm.enumerator(atPath: dir)?.allObjects as? [String] ?? []).sorted().compactMap { rel in
        let p = dir + "/" + rel
        guard !isDir(p), let d = fm.contents(atPath: p) else { return nil }
        var o: [String: Any] = ["path": rel, "sha256": sha256(d), "size": d.count]
        if d.count <= 200_000, let t = String(data: d, encoding: .utf8) { o["text"] = t }
        return o
    }
}

extension RecordsDB {
    // 프로젝트 행. 작업 경로(root)는 대기열 파일·run.json 이 적은 값이 이기고, 없으면 처음 정한 값을 둔다(처음이면 저장소들의 공통 조상)
    func upsertProject(_ id: String, explicitRoot: String?, repos: [String]) throws {
        let old = try rows("SELECT root, repos FROM projects WHERE id=?", [id]).first
        let oldRoot = old.flatMap { $0[0] as? String }
        var all = old.flatMap { $0[1] as? String }.flatMap { (try? JSONSerialization.jsonObject(with: Data($0.utf8))) as? [String] } ?? []
        for x in repos where !all.contains(x) { all.append(x) }
        var root = explicitRoot.map { canonical($0) } ?? oldRoot ?? commonRoot(all) ?? all.first
        if let r = root, try !rows("SELECT id FROM projects WHERE root=? AND id<>?", [r, id]).isEmpty { root = oldRoot }   // 다른 프로젝트의 자리
        let t = r3(now())
        try run("""
            INSERT INTO projects(id,name,root,kind,repos,created,updated) VALUES(?,?,?,?,?,?,?)
            ON CONFLICT(id) DO UPDATE SET root=excluded.root, kind=excluded.kind, repos=excluded.repos, updated=excluded.updated
            """, [id, id, root, all.count > 1 ? "bundle" : "single", jsonText(all), t, t])
    }

    // 실행 폴더 하나를 runs·units·cards·carry(와 projects·sets, 끝난 단위의 대기열 항목 상태)로 옮긴다. 쓴 행 수를 돌려준다
    func syncRun(_ src: RunSource) throws -> Int {
        let dir = src.dir
        guard let r = readJSON(dir + "/run.json") as? [String: Any], let q = readJSON(dir + "/queue.json") as? [[String: Any]] else {
            throw DBError(description: "run.json·queue.json 을 읽지 못했습니다(\(dir))")
        }
        let f = src.frozen ?? (readJSON(dir + "/pinned/frozen.json") as? [String: Any]) ?? [:]
        let defs = (f["units"] as? [[String: Any]]).flatMap { $0.count == q.count ? $0 : nil }
        func def(_ i: Int) -> [String: Any] { defs?[i] ?? q[i] }
        let runId = (dir as NSString).lastPathComponent
        let project = src.project ?? (f["project"] as? String) ?? (r["project"] as? String)
        let t = r3(now())
        var written = 0
        var eng: [Int: [String: Any]] = [:], resText: [Int: String] = [:]
        for i in q.indices {
            if let e = readJSON(dir + "/units/\(i + 1)/engine.json") as? [String: Any] { eng[i + 1] = e }
            if let d = fm.contents(atPath: dir + "/units/\(i + 1)/result.json") { resText[i + 1] = String(decoding: d, as: UTF8.self) }
        }
        if let project {
            let cwds = q.indices.compactMap { def($0)["cwd"] as? String }
            // 저장소 목록: 가지 기록의 저장소 키(원래 저장소 경로 — 따로 작업 트리면 작업 트리가 아니라 원래 저장소). 없으면 하네스 폴더,
            // 옮겨 담는 옛 실행은 단위 작업 폴더(저장소를 열어 보지 않는다). 작업 트리 경로가 기록 저장소의 projects.repos 에 들어가지 않게 한다
            let branchRepos = ((r["branches"] as? [String: Any])?["repos"] as? [String: Any])?.keys.map { String($0) }.sorted() ?? []
            var folders: [String]
            if !branchRepos.isEmpty {
                folders = branchRepos
            } else {
                folders = (r["harness"] as? [String: Any])?["folders"] as? [String] ?? []
                for c in cwds where !folders.contains(where: { c == $0 || c.hasPrefix($0 + "/") }) { folders.append(c) }
            }
            try upsertProject(project, explicitRoot: (f["project_root"] ?? r["project_root"]) as? String, repos: folders)
            written += 1
        }
        // 이 실행이 쓴 자율주행 세트(같은 판은 한 줄): 지시·명세·기술 스택·판정 스크립트로 판을 가른다. 하네스 지문은 그 판을 처음 쓴 실행의 것을
        // 기준으로 남긴다(실행마다의 하네스는 runs.harness — 저장소가 다르거나 사람이 설정을 고쳐도 같은 지시가 새 판이 되지 않게)
        var setId: String?
        if let set = f["set"] as? [String: Any], set["instruction"] != nil || set["spec"] != nil || set["stack"] != nil {
            let checks = checksFingerprint(dir + "/pinned/checks")
            let harness = r["harness"] as? [String: Any]
            let parts = [set["instruction"] as? String ?? "", set["spec"] as? String ?? "", jtext(set["stack"]) ?? "", jsonText(checks)]
            let digest = sha256(Data(parts.joined(separator: "\u{0}").utf8))
            let id = String(digest.prefix(16))
            let parent = try project.flatMap { try rows("SELECT id FROM sets WHERE project_id=? AND id<>? ORDER BY created DESC LIMIT 1", [$0, id]).first?[0] as? String }
            written += try run("""
                INSERT INTO sets(id,project_id,digest,instruction,spec,stack,checks,harness,worktree_files,parent_id,origin,created)
                VALUES(?,?,?,?,?,?,?,?,NULL,?,NULL,?) ON CONFLICT(id) DO NOTHING
                """, [id, project, digest, set["instruction"] as? String, set["spec"] as? String, jtext(set["stack"]),
                      checks.isEmpty ? nil : jsonText(checks), harness.map { jsonText($0) }, parent, t])
            setId = id
        }
        let statuses = q.map { $0["status"] as? String ?? "pending" }
        func cnt(_ s: String) -> Int { statuses.filter { $0 == s }.count }
        var act = 0.0, cost = 0.0, pp = 0.0
        for e in eng.values { act += num(e["active_sec"]) ?? 0; cost += num(e["cost_usd"]) ?? 0; pp += num(e["weekly_pp"]) ?? 0 }
        let model = f["model"] as? String ?? r["model"] as? String
        let runCols: [Any?] = [
            runId, project, (f["title"] ?? r["title"]) as? String, jtext(r["source"]), jtext(r["continues"]), r["state"] as? String, r["reason"] as? String,
            model, (f["effort"] ?? r["effort"]) as? String, (r["dev"] as? Bool).map { $0 ? 1 : 0 }, num(r["started"]), num(r["ended"]),
            q.count, cnt("done"), cnt("failed"), cnt("capped"), cnt("stopped"), r3(act), r3(cost), r3(pp),
            jtext(r["worktree"]), jtext(r["branches"]), jtext(r["harness"]), setId, jtext(r["epic"]), dir, src.origin, t]
        written += try run("""
            INSERT INTO runs(id,project_id,title,source,continues,state,reason,model,effort,dev,started,ended,units_total,units_done,units_failed,
              units_capped,units_stopped,active_sec,cost_usd,weekly_pp,worktree,branches,harness,set_id,epic,run_dir,origin,updated)
            VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id, title=excluded.title, source=excluded.source, continues=excluded.continues,
              state=excluded.state, reason=excluded.reason, model=excluded.model, effort=excluded.effort, dev=excluded.dev, started=excluded.started,
              ended=excluded.ended, units_total=excluded.units_total, units_done=excluded.units_done, units_failed=excluded.units_failed,
              units_capped=excluded.units_capped, units_stopped=excluded.units_stopped, active_sec=excluded.active_sec, cost_usd=excluded.cost_usd,
              weekly_pp=excluded.weekly_pp, worktree=excluded.worktree, branches=excluded.branches, harness=excluded.harness,
              set_id=COALESCE(excluded.set_id, runs.set_id), epic=excluded.epic, run_dir=excluded.run_dir, updated=excluded.updated
            """, runCols)
        for (i, qq) in q.enumerated() {
            let n = i + 1, d = def(i), e = eng[n] ?? [:]
            let result = resText[n].flatMap { (try? JSONSerialization.jsonObject(with: Data($0.utf8))) as? [String: Any] }
            var key = d["source_key"] as? String
            if key == nil, let p = src.unitKey { key = valueAt(p, qq) as? String ?? valueAt(p, d) as? String }
            var est = num(d["est_min"]), basis = d["est_basis"] as? String, kind = d["kind"] as? String
            if let project, let key, let row = try rows("SELECT est_min, est_basis, kind FROM queue_items WHERE project_id=? AND source_key=?", [project, key]).first {
                est = est ?? num(row[0]); basis = basis ?? row[1] as? String; kind = kind ?? row[2] as? String
            }
            kind = kind ?? (result?["unit"] as? [String: Any])?["kind"] as? String
            let checks = e["checks"] as? [[String: Any]] ?? []
            let status = qq["status"] as? String ?? "pending"
            written += try run("""
                INSERT INTO units(run_id,n,unit_id,title,kind,source_key,story_key,task_key,branch,status,merged,est_min,est_basis,active_sec,cost_usd,
                  weekly_pp,turns,tries,check_exit,denials,result,model,started,ended)
                VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
                ON CONFLICT(run_id,n) DO UPDATE SET unit_id=excluded.unit_id, title=excluded.title, kind=COALESCE(excluded.kind, units.kind),
                  source_key=COALESCE(excluded.source_key, units.source_key), story_key=excluded.story_key, task_key=excluded.task_key,
                  branch=excluded.branch, status=excluded.status, merged=excluded.merged, est_min=COALESCE(excluded.est_min, units.est_min),
                  est_basis=COALESCE(excluded.est_basis, units.est_basis), active_sec=excluded.active_sec, cost_usd=excluded.cost_usd,
                  weekly_pp=excluded.weekly_pp, turns=excluded.turns, tries=excluded.tries, check_exit=excluded.check_exit, denials=excluded.denials,
                  result=excluded.result, model=excluded.model, started=excluded.started, ended=excluded.ended
                """, [runId, n, d["id"] as? String ?? qq["id"] as? String, d["title"] as? String ?? qq["title"] as? String, kind, key,
                      // Jira 작업: 스토리 키, 작업 키(tickets 의 첫 번호), 작업 가지, 스토리로 합쳤는지(merged·nothing·not_merged)
                      d["story"] as? String, d["story"] == nil ? nil : (d["tickets"] as? [Any])?.first.map { "\($0)" },
                      e["branch"] as? String ?? d["branch"] as? String, status, (e["merge"] as? [String: Any])?["state"] as? String, est, basis,
                      num(e["active_sec"]), num(e["cost_usd"]), num(e["weekly_pp"]), inum(e["turns"]), inum(e["try"]),
                      inum(checks.last?["exit"]), (e["denials"] as? [Any]).map { jsonText($0) }, resText[n], model,
                      num(e["started"]), num(e["ended"])])
            // 결과 기록의 미룬 사람 몫·새로 찾은 일(다음 출발 카드·대기열 후보). 고쳐 쓴 결과 기록에서 빠진 것은 아직 열린 것만 지운다
            if let result {
                var keep: [String] = []
                for kindKey in ["next_start", "new_work"] {
                    for (j, item) in (result[kindKey] as? [Any] ?? []).enumerated() {
                        let o = item as? [String: Any] ?? ["title": "\(item)"]
                        let cid = "\(runId)/\(n)/\(kindKey)/\(j + 1)"
                        keep.append(cid)
                        written += try run("""
                            INSERT INTO carry(id,run_id,unit_n,kind,title,why,state,used_run) VALUES(?,?,?,?,?,?,'open',NULL)
                            ON CONFLICT(id) DO UPDATE SET title=excluded.title, why=excluded.why
                            """, [cid, runId, n, kindKey, jtext(o["title"]), jtext(o["why"])])
                    }
                }
                for row in try rows("SELECT id FROM carry WHERE run_id=? AND unit_n=? AND state='open'", [runId, n]) {
                    if let x = row[0] as? String, !keep.contains(x) { try run("DELETE FROM carry WHERE id=?", [x]) }
                }
            }
            // 판정을 통과한 단위의 대기열 항목은 끝남으로
            if let project, let key, status == "done" {
                try run("""
                    UPDATE queue_items SET state='done', done_run=?, done_unit=?, updated=?
                    WHERE project_id=? AND source_key=? AND (state IS NOT 'done' OR done_run IS NOT ? OR done_unit IS NOT ?)
                    """, [runId, n, t, project, key, runId, n])
            }
        }
        // 출발 카드와 고른 답
        let cardsAny = readJSON(dir + "/cards.json")
        if let cards = cardsAny as? [[String: Any]] ?? (cardsAny as? [String: Any])?["cards"] as? [[String: Any]] {
            let answers = readJSON(dir + "/answers.json") as? [String: Any] ?? [:]
            let at = mtime(dir + "/answers.json")
            for c in cards {
                guard let cid = c["id"] as? String else { continue }
                let a = jtext(answers[cid])
                written += try run("""
                    INSERT INTO cards(run_id,card_id,def,answer,policy,answered) VALUES(?,?,?,?,NULL,?)
                    ON CONFLICT(run_id,card_id) DO UPDATE SET def=excluded.def, answer=excluded.answer, answered=excluded.answered
                    """, [runId, cid, jsonText(c), a, a == nil ? nil : at])
            }
        }
        return written
    }

    // 일반 꼴 대기열 파일(설계 5장 7절)을 queue_items 로. 프로젝트 전용 꼴은 그 프로젝트의 변환 스크립트가 이 꼴로 바꿔 넘긴다
    static let queueStates: Set<String> = ["open", "done", "parked", "dropped", "board"]
    func importQueue(_ path: String, project override: String?) throws -> Int {
        guard let j = readJSON(path) as? [String: Any], j["format"] as? String == "claude-board-queue",
              let items = j["items"] as? [[String: Any]] else {
            throw DBError(description: "일반 꼴 대기열 파일이 아닙니다(format \"claude-board-queue\" 와 items 배열이 있어야 함)")
        }
        guard let project = override ?? j["project"] as? String, validName(project) else {
            throw DBError(description: "프로젝트 이름이 없거나 꼴이 맞지 않습니다(파일의 project 또는 --project, 영문·숫자·.-_)")
        }
        var seen = Set<String>(), bad: [String] = []
        for (i, it) in items.enumerated() {
            guard let k = it["source_key"] as? String, !k.isEmpty, (it["title"] as? String ?? "").isEmpty == false else { bad.append("\(i + 1)번째 항목에 source_key·title 이 없음"); continue }
            if !seen.insert(k).inserted { bad.append("source_key 가 겹침: \(k)") }
            if let s = it["state"] as? String, !RecordsDB.queueStates.contains(s) { bad.append("\(k) 의 state(\(s))는 open·done·parked·dropped·board 가운데 하나") }
        }
        guard bad.isEmpty else { throw DBError(description: "고칠 것 \(bad.count)개: " + bad.prefix(5).joined(separator: "; ")) }
        try upsertProject(project, explicitRoot: j["root"] as? String, repos: j["repos"] as? [String] ?? [])
        let t = r3(now())
        var written = 1
        for it in items {
            written += try run("""
                INSERT INTO queue_items(project_id,source_key,title,kind,grp,est_min,est_basis,depends,needs,state,done_run,done_unit,origin,
                  story_key,task_key,note,added,updated) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
                ON CONFLICT(project_id,source_key) DO UPDATE SET title=excluded.title, kind=excluded.kind, grp=excluded.grp, est_min=excluded.est_min,
                  est_basis=excluded.est_basis, depends=excluded.depends, needs=excluded.needs,
                  state=CASE WHEN queue_items.state='done' AND excluded.state<>'done' THEN queue_items.state ELSE excluded.state END,
                  done_run=CASE WHEN queue_items.state='done' AND excluded.state<>'done' THEN queue_items.done_run ELSE excluded.done_run END,
                  done_unit=CASE WHEN queue_items.state='done' AND excluded.state<>'done' THEN queue_items.done_unit ELSE excluded.done_unit END,
                  origin=excluded.origin, story_key=excluded.story_key, task_key=excluded.task_key, note=excluded.note, updated=excluded.updated
                """, [project, it["source_key"] as? String, it["title"] as? String, it["kind"] as? String, jtext(it["grp"]), num(it["est_min"]),
                      jtext(it["est_basis"]), (it["depends"] as? [Any]).map { jsonText($0) }, it["needs"].flatMap { $0 is NSNull ? nil : jsonText($0) },
                      it["state"] as? String ?? "open", jtext(it["done_run"]), inum(it["done_unit"]), it["origin"] as? String ?? "import",
                      jtext(it["story_key"]), jtext(it["task_key"]), jtext(it["note"]), num(it["added"]) ?? t, t])
        }
        // 대기열보다 먼저 옮긴 실행의 단위에도 예상·종류를 붙인다(이미 있는 값은 두고)
        try run("""
            UPDATE units SET
              est_min=COALESCE(est_min, (SELECT q.est_min FROM queue_items q WHERE q.project_id=?1 AND q.source_key=units.source_key)),
              est_basis=COALESCE(est_basis, (SELECT q.est_basis FROM queue_items q WHERE q.project_id=?1 AND q.source_key=units.source_key)),
              kind=COALESCE(kind, (SELECT q.kind FROM queue_items q WHERE q.project_id=?1 AND q.source_key=units.source_key))
            WHERE source_key IS NOT NULL AND run_id IN (SELECT id FROM runs WHERE project_id=?1)
            """, [project])
        return written
    }
}

// MARK: - 엔진

enum Stop { case pause, stop }
enum TurnEnd { case finished, limit(Double?), capped(String), paused, stopped, failed(String) }
enum CheckEnd { case passed, failed(Int32, String), capped(String), paused, stopped }
enum WaitEnd { case elapsed, paused, stopped }

let stateLabel = ["starting": "준비 중", "running": "진행 중", "waiting_limit": "한도 대기", "waiting_stopline": "멈출 선 대기",
                  "waiting_precheck": "시험 환경 대기",
                  "paused": "일시정지", "stopped": "중단됨", "done": "끝남", "error": "엔진 오류로 멈춤"]
let unitLabel = ["pending": "대기", "running": "진행 중", "done": "통과", "failed": "미통과", "capped": "상한에서 멈춤", "stopped": "중단", "skipped": "건너뜀"]
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
                              "read_deny", "sandbox_exclude", "claude_args", "claude_path", "project", "project_root", "base_branch"]
    static let unitKeys = ["id", "title", "prompt", "check", "precheck", "action", "context", "cwd", "caps", "check_timeout_min", "action_timeout_min",
                           "tickets", "review", "source_key", "kind", "est_min", "est_basis", "story", "branch", "depends", "prerun"]
    func syncFromFrozen() {
        for k in Engine.settingKeys { run[k] = cfg[k] }
        run["set"] = setSummary(cfg)
        for i in queue.indices { for k in Engine.unitKeys { queue[i][k] = defs[i][k] } }
        saveRun(); saveQueue()
    }
    var isDev: Bool { cfg["dev"] as? Bool ?? run["dev"] as? Bool ?? false }
    // 출발 때 고정한 프로젝트 자율주행 지시(pinned/set/autopilot.md 와 같은 글). 시스템 프롬프트의 공통 규칙 뒤에 붙는다
    var instruction: String? { (cfg["set"] as? [String: Any])?["instruction"] as? String }
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
    var enginePrivate: String { privateRoot(dev: isDev) + "/" + id }
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
            if run["wrapped"] == nil { wrapUp() }          // 하네스 끝 검사·기록 저장소 쓰기 전에 끊겼으면 그것부터
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
        if !setupEnv() { event("engine", "로그인 셸 환경을 읽지 못해 기본 환경으로 띄웁니다.") }
        // claude_path 는 시험용(한도 거절을 흉내 내는 가짜 claude 등) — --dev 로 출발한 실행만 고정 설정에 들어 있다
        guard let c = (cfg["claude_path"] as? String).flatMap({ access($0, X_OK) == 0 ? $0 : nil }) ?? findExecutable("claude", loginEnv) else {
            event("engine", "claude 를 찾지 못해 멈춥니다(로그인 셸의 PATH 에 없음).")
            finish("error", "claude 를 찾지 못했습니다.")
        }
        claude = c
        writePlugin()
        run["engine_pid"] = Int(getpid())
        if first {
            run["started"] = r3(t)
            event("engine", "자율주행을 시작합니다. 단위 \(total)개를 차례로 합니다(모델 \(model)).")
            if let ins = instruction {
                event("engine", "프로젝트 자율주행 지시(\(ins.count)자)를 시스템 프롬프트의 공통 규칙 뒤에 붙여 돕니다: \(short(ins, 80))")
            } else if let set = cfg["set"] as? [String: Any] {
                event("engine", "프로젝트 자율주행 세트에 지시(autopilot.md)가 없어 공통 규칙만으로 돕니다(세트 자리 \(set["dir"] ?? "?")).")
            } else {
                event("engine", "run.json 에 project 칸이 없어 프로젝트 자율주행 지시 없이 공통 규칙만으로 돕니다.")
            }
            if let h = run["harness"] as? [String: Any] {
                event("engine", "하네스 사본과 지문을 남겼습니다(저장소 \((h["folders"] as? [Any])?.count ?? 0)곳, 파일 \(h["files"] ?? 0)개). 끝에 다시 재어 같은지 봅니다.")
            }
            for r in repos {
                let i = repoInfo(r), base = (i["base"] as? [String: Any])?["branch"] as? String ?? "?"
                event("engine", "\(repoName(r)): 출발 가지 \(base) 에서 만든 \(i["work"] ?? "?") 로 원래 작업 트리를 바꿔 돕니다. 끝나면 깨끗할 때 \((i["original"] as? String) ?? "출발 때 자리") 로 되돌립니다.")
            }
            setState("running", "단위를 시작하는 중입니다.")
            record("출발")
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
                event("engine", "자율주행을 마쳤습니다. 통과 \(c("done")), 미통과 \(c("failed")), 상한에서 멈춤 \(c("capped"))\(c("skipped") > 0 ? ", 건너뜀 \(c("skipped"))" : "").")
                finish("done", "대기열을 모두 마쳤습니다.")
            }
            if queue[n - 1]["status"] as? String != "running" {
                guard stoplineGate(), unitBranchGate(n), precheckGate(n) else { continue }   // 건너뛸 단위는 시험 환경을 기다리지 않는다
                beginUnit(n)
            }
            runUnit(n)
        }
    }

    // 로그인 셸 환경을 한 번 읽는다(못 읽으면 기본 환경, false). 샌드박스 안에서는 /usr/bin/java 가 JDK 를 못 찾는다("Unable to locate a Java Runtime", 실측) —
    // JAVA_HOME 이 없으면 밖에서 찾아 넣는다. 엔진 본체와 판정 미리 돌리기가 같이 쓴다
    @discardableResult
    func setupEnv() -> Bool {
        shell = loginShell()
        let got = captureLoginEnv(shell)
        loginEnv = got ?? ["HOME": home, "SHELL": shell, "LANG": "ko_KR.UTF-8",
                           "USER": NSUserName(), "LOGNAME": NSUserName(), "TMPDIR": NSTemporaryDirectory(),
                           "PATH": "\(home)/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"]
        if loginEnv["JAVA_HOME"] == nil {
            let jh = runTool("/usr/libexec/java_home", [], timeout: 10)
            let p = jh.out.trimmingCharacters(in: .whitespacesAndNewlines)
            if jh.code == 0, isDir(p) { loginEnv["JAVA_HOME"] = p }
        }
        return got != nil
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
        wrapUp()
        cleanupAndExit()
    }

    // 끝(완료·중단·오류)의 정리: 하네스를 다시 재어 출발 때와 같은지 적고, 출발 때 없던 빈 .claude/.cc-writes 를 치우고, 기록 저장소에 끝을 쓴다.
    // 출발 때 남긴 지문(manifest.json)만 읽으므로 고정 설정을 못 읽은 오류 끝에서도 돈다
    // 차례(설계 4장 시작과 끝): 하네스 끝 검사·빈 자리 치우기 → 가지 끝 머리 → 기록 저장소 → 출발 때 가지로 되돌리기 → 한 번 더 기록 → wrapped.
    // 되돌리기가 맨 뒤인 것은 switch 가 하네스 파일을 바꿔 "하네스가 바뀜"으로 잡히지 않게. 정리 중 끊겨 다시 돌아도 이미 한 단계는 건너뛴다
    func wrapUp() {
        if var h = run["harness"] as? [String: Any], h["checked"] == nil {
            if let c = harnessCheck(manifestDir: enginePrivate + "/harness") {
                for (k, v) in c { h[k] = v }
                run["harness"] = h
                let changed = c["changed"] as? [[String: Any]] ?? []
                if changed.isEmpty {
                    event("engine", "하네스가 출발 때와 같습니다(파일 \(h["files"] ?? 0)개).")
                } else {
                    let k = { (x: String) in changed.filter { $0["change"] as? String == x }.count }
                    let some = changed.prefix(3).compactMap { $0["path"] as? String }.map { $0.hasPrefix(home + "/") ? "~/" + $0.dropFirst(home.count + 1) : $0 }
                    event("engine", "하네스가 출발 때와 다릅니다(바뀜 \(k("changed")), 더함 \(k("added")), 없어짐 \(k("removed")): \(some.joined(separator: ", "))\(changed.count > 3 ? " 등" : "")). 실행 중에 사람이 고쳤을 수 있어 되돌리지 않았습니다. 출발 때 사본은 엔진 전용 폴더에 있습니다.")
                }
                let cleaned = c["cleaned"] as? [String] ?? []
                if !cleaned.isEmpty {
                    event("engine", "출발 때 없던 빈 임시 자리(.claude/.cc-writes, 그것만 들어 있던 .claude) \(cleaned.count)곳을 치웠습니다.")
                }
            } else {
                event("engine", "출발 때 남긴 하네스 지문을 찾지 못해 끝 검사를 하지 못했습니다.")
            }
        }
        if !repos.isEmpty, br["end"] == nil { endBranches() }
        saveRun()
        record("끝")
        if restoreBranches() { record("되돌린 뒤") }
        run["wrapped"] = r3(now())
        saveRun()
    }

    // 서브에이전트에 주는 규칙. --append-system-prompt 는 Agent 도구의 서브에이전트에 닿지 않아(실측) 고정 플러그인의 SubagentStart hook 이
    // 서브에이전트가 뜰 때 같은 글을 넣는다(additionalContext). 결과 기록은 주 세션 몫이라 서브에이전트에게는 쓰지 말라고 한다
    var subagentRules: String {
        "이 서브에이전트는 Claude Board 자율주행 세션(사람이 지켜보지 않는 실행)이 띄웠다. 주 세션과 같은 아래 규칙을 따른다. "
            + "다만 결과 기록(result.json)은 주 세션이 쓰므로 서브에이전트는 쓰지 않고, 보류할 것이나 사람 몫은 답에 적어 주 세션에 넘긴다.\n\n"
            + systemPromptText + (instruction.map { "\n\n# 이 프로젝트의 자율주행 지시(보드가 출발 때 고정함)\n\n" + $0 } ?? "")
    }
    var pluginDir: String { dir + "/pinned/plugin" }
    // 엔진이 뜰 때마다 고정 설정에서 다시 쓴다(pinned/ 는 모델이 쓸 수 없지만, 글은 늘 고정 설정과 같게)
    func writePlugin() {
        mkdirPrivate(pluginDir); mkdirPrivate(pluginDir + "/.claude-plugin"); mkdirPrivate(pluginDir + "/hooks")
        writePrivate(pluginDir + "/.claude-plugin/plugin.json", Data(jsonText([
            "name": "claude-board-autopilot", "version": "1.0.0",
            "description": "Claude Board 자율주행: 서브에이전트가 뜰 때 자율주행 규칙과 프로젝트 지시를 넣는다"], pretty: true).utf8))
        writePrivate(pluginDir + "/hooks/hooks.json", Data(jsonText([
            "hooks": ["SubagentStart": [["hooks": [["type": "command", "command": "/bin/cat \"${CLAUDE_PLUGIN_ROOT}/subagent-start.json\""]]]]]],
            pretty: true).utf8))
        writePrivate(pluginDir + "/subagent-start.json", Data(jsonText([
            "hookSpecificOutput": ["hookEventName": "SubagentStart", "additionalContext": subagentRules]]).utf8))
    }

    // 기록 저장소에 이 실행을 옮겨 적는다(출발·단위 판정 뒤·끝). 실패해도 실행은 멈추지 않고, 다음 기록 때 실행 폴더에서 통째로 다시 옮긴다
    func record(_ when: String) {
        do {
            if !fm.fileExists(atPath: privateRoot(dev: isDev)) { mkdirPrivate(privateRoot(dev: isDev)) }
            let db = try RecordsDB(recordsPath(dev: isDev))
            // 고정 설정을 못 읽은 오류 끝(누가 바꿨다)이면 빈 값을 넘긴다 — 바뀐 pinned/frozen.json 을 기록에 옮기지 않게
            try db.transaction { _ = try db.syncRun(RunSource(dir: dir, origin: "engine", frozen: cfg)) }
            if run["records_error"] != nil {
                run["records_error"] = nil; saveRun()
                event("engine", "실행 기록 저장소에 다시 옮겨 적었습니다(\(when)).")
            }
        } catch {
            run["records_error"] = "\(error)"; saveRun()
            event("engine", "실행 기록 저장소에 쓰지 못했습니다(\(when): \(error)). 실행은 그대로 가고, 다음 기록 때 실행 폴더에서 다시 옮깁니다.")
        }
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

    // MARK: 가지

    var br: [String: Any] { get { run["branches"] as? [String: Any] ?? [:] } set { run["branches"] = newValue } }
    var repos: [String] { (br["repos"] as? [String: Any] ?? [:]).keys.sorted() }
    func repoInfo(_ r: String) -> [String: Any] { (br["repos"] as? [String: Any])?[r] as? [String: Any] ?? [:] }
    func setRepoInfo(_ r: String, _ i: [String: Any]) {
        var b = br, rs = b["repos"] as? [String: Any] ?? [:]
        rs[r] = i; b["repos"] = rs; br = b
    }
    // 저장소의 작업 트리 자리: 따로 작업 트리 모드면 그 작업 트리, 아니면 원래 저장소. 작업 트리 쪽 git(switch·status·커밋)은 이 자리에서 돈다
    func wt(_ r: String) -> String { repoInfo(r)["work_tree"] as? String ?? r }
    func repoOf(_ path: String) -> String? {
        repos.filter { r in let w = wt(r); return path == r || path.hasPrefix(r + "/") || path == w || path.hasPrefix(w + "/") }.max { $0.count < $1.count }
    }
    // 보안(2026-10-22 둘째): 엔진이 샌드박스 밖에서 쓰는 git 을 돌리기 전에 그 저장소의 git 폴더가 출발 때와 같은지 본다.
    // 모델이 .git 을 바꿔치기했으면(장치·inode·절대 경로가 다름) git 을 돌리지 않고 위반으로 알린다(샌드박스가 이미 막지만 받침)
    func gitDirOK(_ r: String) -> Bool {
        let i = repoInfo(r)
        guard let c = i["common_dir"] as? String else { return true }
        let live = gitOut(["rev-parse", "--git-common-dir"], cwd: wt(r)).map { $0.hasPrefix("/") ? canonical($0) : canonical(wt(r) + "/" + $0) }
        if live != c || (i["common_ino"] as? String).map({ $0 != gitStat(c) }) == true {
            event("engine", "보안: \(repoName(r)) 의 git 폴더가 출발 때와 달라져 git 을 돌리지 않습니다(바꿔치기 의심). 사람이 확인하세요.")
            return false
        }
        if let wg = i["work_git_dir"] as? String, (i["work_git_ino"] as? String).map({ $0 != gitStat(wg) }) == true {
            event("engine", "보안: \(repoName(r)) 의 작업 트리 git 폴더가 출발 때와 달라져 git 을 돌리지 않습니다. 사람이 확인하세요.")
            return false
        }
        return true
    }
    func story(_ k: String) -> [String: Any] { (br["stories"] as? [String: Any])?[k] as? [String: Any] ?? [:] }
    func setStory(_ k: String, _ s: [String: Any]) {
        var b = br, ss = b["stories"] as? [String: Any] ?? [:]
        ss[k] = s; b["stories"] = ss; br = b
    }
    func addMade(_ r: String, _ b: String) {
        var i = repoInfo(r)
        let m = i["made"] as? [String] ?? []
        if !m.contains(b) { i["made"] = m + [b]; setRepoInfo(r, i) }
    }

    // 모델 쪽 가지 보호(샌드박스 denyWrite, 2026-10-22 실측): 출발 때 있던 가지와 엔진이 만든 가지 가운데 지금 작업 가지를 뺀 ref 파일과 그 .lock,
    // HEAD(가지 바꾸기), packed-refs(압축된 가지 옮기기·지우기), config. HEAD.lock·packed-refs.lock 은 막지 않는다 — 막으면 커밋이 안 되거나(HEAD.lock)
    // 커밋마다 오류 줄이 찍힌다(packed-refs.lock). 거부한 폴더 안을 글자 그대로 다시 열 수는 없어(실측) 가지는 하나씩 막고, Jira 스토리 가지 꼴의
    // 앞 폴더(예 features/parent)는 통째로 막는다. 모델이 새로 만든 가지는 막지 않는다(끝의 가지 머리 비교로 created 에 남는다)
    func protectDeny() -> [String] {
        var out: [String] = []
        let start = br["start"] as? [String: Any] ?? [:]
        for r in repos {
            let i = repoInfo(r)
            guard let g = i["git_dir"] as? String, let c = i["common_dir"] as? String else { continue }
            let on = i["on"] as? String ?? ""
            var names = Set((start[r] as? [String: Any] ?? [:]).keys)
            names.formUnion(i["made"] as? [String] ?? [])
            names.remove(on)
            for b in names.sorted() { out += [c + "/refs/heads/" + b, c + "/refs/heads/" + b + ".lock"] }
            out += [c + "/HEAD", g + "/HEAD", c + "/packed-refs", c + "/config"]
            // 밖에서 도는 git 이 실행할 공통 .git 설정 자리(작업 트리 모드는 공통 .git 이 쓰기 범위라 harnessDeny 가 못 덮는다, 보안 2026-10-22 둘째)
            out += [c + "/hooks", c + "/config.worktree", c + "/info", c + "/commondir", c + "/objects/info/alternates"]
            // 작업 트리 git 폴더의 config.worktree(extensions.worktreeConfig 면 git 이 읽는다, 보안 2026-10-22 셋째). 따로 작업 트리와 원래 저장소가 작업 트리인 경우
            if g != c { out.append(g + "/config.worktree") }
            // 보안: replace ref(검토 속이기)와 shallow(얕은 복제로 이력 가리기)
            out += [c + "/refs/replace", c + "/shallow", g + "/shallow"]
            for d in storyDirs where !on.hasPrefix(d + "/") { out.append(c + "/refs/heads/" + d) }
            // 따로 작업 트리: 작업 트리 .git 파일과 그 작업 트리 git 폴더의 commondir·gitdir(git 폴더 바꿔치기 막기)
            if let w = i["work_tree"] as? String { out += [w + "/.git", g + "/commondir", g + "/gitdir"] }
        }
        return out
    }
    // 스토리 가지들의 앞 폴더(통째로 막아 모델이 스토리 꼴 가지를 만들거나 옮기지 못하게). 꼴이 {type}·{story} 로 스토리마다 달라
    // 패턴이 아니라 이 실행이 정한 실제 스토리 가지(단위 story_branch)에서 뽑는다. 예 "fix/_TOY-1/parent/TOY-1" → "fix/_TOY-1/parent"
    var storyDirs: [String] {
        var s = Set<String>()
        for d in defs {
            if let sb = d["story_branch"] as? String, let slash = sb.lastIndex(of: "/") {
                let p = String(sb[..<slash]); if !p.isEmpty { s.insert(p) }
            }
        }
        return s.sorted()
    }

    // 단위를 시작하기 전의 가지(설계 2장 끝): 기대는 앞 단위(depends)가 통과하지 않았으면(Jira 단위면 스토리 가지에 합쳐지지 않았으면) 건너뛴다.
    // Jira 단위(story 칸)는 스토리 가지(없으면 출발 가지 머리에서)와 작업 가지(스토리 가지 머리에서)를 만들고 그 저장소를 작업 가지로 바꾼다.
    // 다른 저장소와 Jira 가 아닌 단위는 autopilot/<실행 id> 에 둔다. 못 하면 그 단위를 건너뛰고 false
    func unitBranchGate(_ n: Int) -> Bool {
        for d in def(n)["depends"] as? [String] ?? [] {
            guard let i = defs.firstIndex(where: { $0["id"] as? String == d }) else { continue }
            let st = queue[i]["status"] as? String ?? "pending"
            let m = (loadUnit(i + 1)["merge"] as? [String: Any])?["state"] as? String
            guard st == "done", defs[i]["story"] == nil || m == "merged" || m == "nothing" else {
                skipUnit(n, "기대는 앞 단위 「\(title(i + 1))」\(st == "done" ? " 이 스토리 가지에 합쳐지지 않아" : " 가 통과하지 못해") 시작하지 않습니다.")
                return false
            }
        }
        let target = def(n)["story"] == nil ? nil : repoOf(cwd(n))
        for r in repos {
            var want = repoInfo(r)["work"] as? String ?? ""
            if r == target, let k = def(n)["story"] as? String, let sb = def(n)["story_branch"] as? String, let task = def(n)["branch"] as? String {
                if let why = ensureBranch(r, sb, from: (repoInfo(r)["base"] as? [String: Any])?["head"] as? String ?? "", story: k)
                    ?? ensureBranch(r, task, from: sb, story: nil) {
                    skipUnit(n, why); return false
                }
                want = task
            }
            if let why = switchTo(r, want) { skipUnit(n, why); return false }
        }
        return true
    }

    // 이 실행이 만드는 가지(스토리·작업): 없으면 만들고 made 에 적는다. 이미 있는데 이 실행이 만든 것이 아니면 움직이지 않는다(까닭을 돌려줌)
    func ensureBranch(_ r: String, _ b: String, from: String, story k: String?) -> String? {
        let made = repoInfo(r)["made"] as? [String] ?? []
        if branchExists(r, b) { return made.contains(b) ? nil : "\(repoName(r)) 에 \(b) 가지가 이미 있어(이 실행이 만든 것이 아님) 쓰지 않습니다" }
        addMade(r, b); saveRun()   // 만들기 전에 적는다 — 만든 뒤 끊겨 다시 떠도 이 실행의 가지로 안다
        let c = gitRun(["branch", b, from], cwd: r)
        guard c.code == 0 else { return "\(repoName(r)) 에 \(b) 가지를 만들지 못했습니다: \(firstLine(c.out))" }
        if let k {
            let h = headOf(r, "refs/heads/" + b) ?? ""
            setStory(k, ["repo": r, "branch": b, "base": h, "head": h, "merged": [] as [Any], "unmerged": [] as [Any]])
            event("board", "스토리 가지 \(b) 를 출발 가지 머리에서 만들었습니다(\(repoName(r))).")
        } else {
            event("board", "작업 가지 \(b) 를 스토리 가지 \(from) 머리에서 만들었습니다(\(repoName(r))).")
        }
        saveRun()
        return nil
    }

    // 원래 작업 트리를 want 가지로 바꾼다. 앞 단위가 커밋하지 않고 남긴 변경은 그 가지(이 실행이 만든 가지)에 엔진이 커밋해 남긴다 —
    // 다음 작업 가지에 섞이지 않게. 출발 때부터 더러웠거나 실행 중에 누가 가지를 바꿨으면 바꾸지 않는다(까닭을 돌려줌)
    func switchTo(_ r: String, _ want: String) -> String? {
        var i = repoInfo(r)
        let w = wt(r)   // 작업 트리 모드면 작업 트리에서, 아니면 원래 저장소에서 바꾼다
        let on = i["on"] as? String ?? "", cur = currentBranch(w) ?? ""
        if on == want, cur == want { return nil }
        guard gitDirOK(r) else { return "\(repoName(r)) 의 git 폴더가 바뀌어 \(want) 로 바꾸지 않았습니다" }
        if cur != on { return "\(repoName(r)) 의 가지가 실행 중에 바뀌어(지금 \(cur.isEmpty ? "가지 없음" : cur)) \(want) 로 바꾸지 않았습니다" }
        let dirty = porcelain(w), dirtyStart = i["dirty_start"] as? [String] ?? []
        if dirty != dirtyStart {
            guard dirtyStart.isEmpty, (i["made"] as? [String] ?? []).contains(on) else {
                return "\(repoName(r)) 에 커밋하지 않은 변경 \(dirty.count)개가 있어 \(want) 로 바꾸지 않았습니다"
            }
            let a = gitRun(["add", "-A"], cwd: w)
            let c = a.code == 0 ? gitRun(["commit", "-q", "--no-verify", "-m", "자율주행 \(id): \(on) 에 커밋하지 않고 남은 변경(다음 단위로 가지를 바꾸기 전에 엔진이 남김)"], cwd: w, commit: true) : a
            guard c.code == 0 else { return "\(repoName(r)) 에 남은 변경을 \(on) 에 커밋하지 못해 \(want) 로 바꾸지 않았습니다: \(firstLine(c.out))" }
            event("board", "앞 단위가 커밋하지 않고 남긴 변경 \(dirty.count)개를 그 가지(\(on))에 커밋해 두었습니다.")
        }
        let sw = gitRun(["switch", want], cwd: w)
        guard sw.code == 0 else { return "\(repoName(r)) 를 \(want) 로 바꾸지 못했습니다: \(firstLine(sw.out))" }
        i = repoInfo(r); i["on"] = want; setRepoInfo(r, i); saveRun()
        return nil
    }

    // 단위를 시작하지 않고 넘긴다(기대는 앞 단위가 통과하지 않음, 가지를 바꾸지 못함). 대기열 항목은 남아 다음 출발에서 다시 고른다
    func skipUnit(_ n: Int, _ why: String) {
        try? fm.createDirectory(atPath: unitDir(n), withIntermediateDirectories: true)
        let t = r3(now())
        var u: [String: Any] = ["unit": n, "id": def(n)["id"] as? String ?? "u\(n)", "title": title(n), "status": "skipped", "phase": "end",
                                "reason": why, "started": t, "ended": t, "active_sec": 0, "cost_usd": 0, "turns": 0, "checks": [] as [Any]]
        if let task = def(n)["branch"] as? String { u["branch"] = task }
        saveUnit(n, u)
        queue[n - 1]["status"] = "skipped"; saveQueue()
        noteUnmerged(n, "건너뜀: " + why)
        event("board", "단위 \(n)/\(total) 건너뜀: \(why)")
        record("단위 \(n) 건너뜀")
    }

    // 판정을 통과한 Jira 작업 가지를 스토리 가지로 합친다(사용자 결정: 판정 통과 때 엔진이 바로). 체크아웃 없이 한다 — 순차 실행이라 스토리 머리가
    // 작업 머리의 조상이면 작업 머리의 tree 로 병합 커밋(부모 = 스토리 머리 + 작업 머리)을 만들어 스토리 ref 만 old 값을 준 update-ref 로 올린다.
    // 조상이 아니면(그 사이 누가 스토리 가지를 움직임) 합치지 않고 사람 몫으로 남긴다. 작업 트리는 작업 가지에 그대로 둔다(같은 tree 라 흔들리지 않음)
    func mergeTask(_ n: Int) -> [String: Any] {
        var u = loadUnit(n)
        if let m = u["merge"] as? [String: Any], m["state"] as? String == "merged" { return m }   // 합친 뒤 끊겨 판정을 다시 했다
        guard let k = def(n)["story"] as? String, let task = def(n)["branch"] as? String, let sb = def(n)["story_branch"] as? String,
              let r = repoOf(cwd(n)) else { return [:] }
        guard gitDirOK(r) else { return ["story": k, "task": task, "branch": sb, "state": "not_merged", "why": "git 폴더가 바뀌어 합치지 않았습니다", "at": r3(now())] }
        var m: [String: Any] = ["story": k, "task": task, "branch": sb, "at": r3(now())]
        let th = headOf(r, "refs/heads/" + task), sh = headOf(r, "refs/heads/" + sb)
        func no(_ why: String) { m["state"] = "not_merged"; m["why"] = why }
        if th == nil || sh == nil { no("작업 가지나 스토리 가지를 찾지 못했습니다") }
        else if th == sh { m["state"] = "nothing"; m["why"] = "작업 가지에 새 커밋이 없어 합칠 것이 없습니다" }
        else if isAncestor(r, th!, sh!) { m["state"] = "merged"; m["head"] = sh! }   // 이미 들어 있음
        else if !isAncestor(r, sh!, th!) { no("스토리 가지 머리가 작업 가지의 조상이 아니라(그 사이 스토리 가지가 움직임) 합치지 않았습니다. 사람이 합칩니다") }
        else {
            var head = th!
            let ff = (cfg["tickets"] as? [String: Any])?["merge"] as? String == "ff"
            if !ff {
                let c = gitOut(["rev-parse", th! + "^{tree}"], cwd: r).map {
                    gitRun(["commit-tree", $0, "-p", sh!, "-p", th!, "-m", "Merge branch '\(task)' into \(sb)\n\n자율주행 \(id) 단위 \(n): \(title(n))"], cwd: r, commit: true)
                }
                if let c, c.code == 0 { head = c.out } else { head = "" ; no("병합 커밋을 만들지 못했습니다: \(firstLine(c?.out ?? ""))") }
            }
            if !head.isEmpty {
                let up = gitRun(["update-ref", "-m", "autopilot \(id): merge \(task)", "refs/heads/" + sb, head, sh!], cwd: r)
                if up.code == 0 { m["state"] = "merged"; m["head"] = head; m["how"] = ff ? "ff" : "merge" }
                else { no("스토리 가지를 올리지 못했습니다(그 사이 움직였을 수 있음): \(firstLine(up.out))") }
            }
        }
        var s = story(k)
        if let h = headOf(r, "refs/heads/" + sb) { s["head"] = h }
        if m["state"] as? String == "not_merged" {
            s["unmerged"] = (s["unmerged"] as? [Any] ?? []) + [["task": task, "why": m["why"] ?? ""]]
        } else if !(s["merged"] as? [String] ?? []).contains(task) {
            s["merged"] = (s["merged"] as? [String] ?? []) + [task]
        }
        setStory(k, s); saveRun()
        u["merge"] = m; saveUnit(n, u)
        switch m["state"] as? String {
        case "merged": event("board", "작업 가지 \(task) 를 스토리 가지 \(sb) 에 \(m["how"] as? String == "ff" ? "앞으로 감기로" : "병합 커밋으로") 합쳤습니다.")
        case "nothing": event("board", "작업 가지 \(task) 에 새 커밋이 없어 스토리 가지 \(sb) 는 그대로입니다.")
        default: event("board", "작업 가지 \(task) 를 스토리 가지에 합치지 않았습니다: \(m["why"] ?? "")")
        }
        return m
    }

    // 통과하지 못한 Jira 작업(미통과·상한·중단·건너뜀): 합치지 않고 작업 가지만 남긴다
    func noteUnmerged(_ n: Int, _ why: String) {
        guard let k = def(n)["story"] as? String, let task = def(n)["branch"] as? String else { return }
        var u = loadUnit(n)
        if u["merge"] == nil { u["merge"] = ["story": k, "task": task, "state": "not_merged", "why": why, "at": r3(now())]; saveUnit(n, u) }
        var s = story(k)
        if s.isEmpty { s = ["repo": repoOf(cwd(n)) ?? "", "branch": def(n)["story_branch"] ?? "", "merged": [] as [Any]] }
        s["unmerged"] = (s["unmerged"] as? [Any] ?? []) + [["task": task, "why": why]]
        setStory(k, s); saveRun()
    }

    // 끝: 저장소마다 가지 머리를 다시 재어 출발 때와 견준다. 움직인 기존 가지는 0 이어야 한다(되돌리지 않고 보여만 준다)
    func endBranches() {
        var b = br
        let start = b["start"] as? [String: Any] ?? [:]
        var end: [String: Any] = [:], moved: [[String: Any]] = [], created: [[String: Any]] = []
        for r in repos {
            let h = branchHeads(r), s = start[r] as? [String: Any] ?? [:]
            let made = repoInfo(r)["made"] as? [String] ?? []
            end[r] = h
            for (k, v) in s.sorted(by: { $0.key < $1.key }) where h[k] != v as? String {
                moved.append(["repo": r, "branch": k, "from": v, "to": h[k] ?? NSNull()])
            }
            for (k, v) in h.sorted(by: { $0.key < $1.key }) where s[k] == nil {
                created.append(["repo": r, "branch": k, "head": v, "by": made.contains(k) ? "engine" : "other"])
            }
        }
        b["end"] = end; b["moved"] = moved; b["created"] = created
        // 보안(2026-10-22 둘째): 실행 중에 새로 생긴 replace ref·바뀐 tag·shallow·include/filter 를 보고한다(되돌리지 않고 보여만 준다)
        var replaceNew: [[String: Any]] = [], tagsChanged: [[String: Any]] = [], shallowChanged: [String] = [], includeChanged: [String] = [], worktreeConfigChanged: [String] = []
        for r in repos {
            let i = repoInfo(r); let c = i["common_dir"] as? String ?? r + "/.git"
            let nowE = extraRefs(r, c), wasE = i["extra"] as? [String: Any] ?? [:]
            let wasT = wasE["tags"] as? [String: String] ?? [:], nowT = nowE["tags"] as? [String: String] ?? [:]
            for (k, v) in nowT where wasT[k] != v { tagsChanged.append(["repo": r, "tag": k]) }
            for k in wasT.keys where nowT[k] == nil { tagsChanged.append(["repo": r, "tag": k, "removed": true]) }
            let wasR = Set(wasE["replace"] as? [String] ?? []), nowR = Set(nowE["replace"] as? [String] ?? [])
            for x in nowR.subtracting(wasR).sorted() { replaceNew.append(["repo": r, "object": x]) }
            if jtext(wasE["shallow"]) != jtext(nowE["shallow"]) { shallowChanged.append(r) }
            let g = i["git_dir"] as? String ?? c
            if (i["includes"] as? [String] ?? []) != configIncludes(c, gitDir: g, scope: []).snapshot { includeChanged.append(r) }
            if let was = i["config_worktree"] as? [String: String], was != worktreeConfigPrints(c, gitDir: g) { worktreeConfigChanged.append(r) }
        }
        b["replace_new"] = replaceNew; b["tags_changed"] = tagsChanged; b["shallow_changed"] = shallowChanged; b["include_changed"] = includeChanged
        b["config_worktree_changed"] = worktreeConfigChanged
        br = b
        let other = created.filter { $0["by"] as? String == "other" }.compactMap { $0["branch"] as? String }
        if moved.isEmpty {
            event("engine", "출발 때 있던 가지는 하나도 움직이지 않았습니다. 이 실행이 만든 가지 \(created.count - other.count)개\(other.isEmpty ? "" : ", 그 밖에 새로 생긴 가지 \(other.count)개(\(other.prefix(3).joined(separator: ", ")))").")
        } else {
            let some = moved.prefix(3).map { "\(repoName($0["repo"] as? String ?? "")) \($0["branch"] ?? "")" }
            event("engine", "출발 때 있던 가지 \(moved.count)개가 움직였습니다(\(some.joined(separator: ", "))\(moved.count > 3 ? " 등" : "")). 되돌리지 않았습니다. 사람이 확인하세요.")
        }
        if !replaceNew.isEmpty || !tagsChanged.isEmpty || !shallowChanged.isEmpty || !includeChanged.isEmpty || !worktreeConfigChanged.isEmpty {
            event("engine", "보안: 실행 중에 replace 객체 \(replaceNew.count)개, 태그 변화 \(tagsChanged.count)개, shallow 변화 \(shallowChanged.count)곳, include/filter 변화 \(includeChanged.count)곳, config.worktree 변화 \(worktreeConfigChanged.count)곳이 생겼습니다. 사람이 push·검토 전에 확인하세요.")
        }
    }

    // 끝: 원래 작업 트리를 출발 때 걸려 있던 가지로 되돌린다. 엔진이 마지막에 걸어 둔 가지에 그대로 있고 작업 트리가 출발 때와 같은 상태(보통 깨끗함)일
    // 때만. 아니면 되돌리지 않고 까닭을 적는다. 이미 되돌렸으면(정리 중 끊겨 다시 돎) 건너뛴다. 바뀐 것이 있으면 true
    func restoreBranches() -> Bool {
        var changed = false
        for r in repos {
            var i = repoInfo(r)
            guard i["restored"] == nil || i["restored"] is NSNull else { continue }
            if i["work_tree"] != nil {   // 따로 작업 트리: 원래 작업 트리·가지를 건드리지 않았으니 되돌릴 것이 없다
                i["restored"] = NSNull(); i["restore_note"] = "따로 작업 트리라 원래 작업 트리·가지를 건드리지 않았습니다"; setRepoInfo(r, i)
                continue
            }
            let original = i["original"] as? String, on = i["on"] as? String ?? "", cur = currentBranch(r)
            var ok = false, note: String
            if let o = original, cur == o { ok = true; note = "이미 출발 때 가지(\(o))에 있습니다" }
            else if (cur ?? "") != on { note = "실행 중에 가지가 바뀌어(지금 \(cur ?? "가지 없음")) 출발 때 가지로 되돌리지 않았습니다" }
            else if porcelain(r) != (i["dirty_start"] as? [String] ?? []) {
                note = "커밋하지 않은 변경 \(porcelain(r).count)개가 있어 출발 때 가지로 되돌리지 않았습니다(작업 트리는 \(on) 에 있습니다)"
            } else {
                let sw = original.map { gitRun(["switch", $0], cwd: r) } ?? gitRun(["switch", "--detach", i["original_head"] as? String ?? "HEAD"], cwd: r)
                if sw.code == 0 { ok = true; note = "원래 작업 트리를 출발 때 가지(\(original ?? "가지 없는 자리"))로 되돌렸습니다" }
                else { note = "출발 때 가지로 되돌리지 못했습니다: \(firstLine(sw.out))" }
            }
            i["restored"] = ok; i["restore_note"] = note
            if ok { i["on"] = original ?? "" }
            setRepoInfo(r, i); changed = true
            event("engine", "\(repoName(r)): \(note).")
        }
        if changed { saveRun() }
        return changed
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
        if let set = cfg["set"] as? [String: Any] {   // 주제로 시작의 세트: 기술 스택은 글로, 명세는 고정한 사본 자리로
            if let st = set["stack"] { s += "- 기술 스택(출발 때 정함, 이 스택으로 구성합니다): \(jsonText(st))\n" }
            if set["spec"] is String { s += "- 명세(요구사항과 수용 기준, 읽기 전용): `\(dir)/pinned/set/spec.md`\n" }
        }
        s += "- 쓰기는 작업 폴더\(writeRoots.isEmpty ? "" : "와 " + writeRoots.joined(separator: ", ")), 단위 폴더, 임시 폴더만 됩니다(샌드박스). 네트워크는 허용한 곳만 나갑니다.\n"
        if !repos.isEmpty {
            let on = repos.map { "\(repoName($0)) `\(repoInfo($0)["on"] as? String ?? "?")`" }
            s += "- 작업 가지: \(on.joined(separator: ", ")). 커밋은 이 가지에만 합니다. 가지를 바꾸거나 다른 가지를 옮기지 않습니다(다른 가지 맞추기는 끝난 뒤 사람이 보드에서 합니다).\n"
        }
        if let k = def(n)["story"] as? String {
            s += "- 이 작업은 스토리 \(k) 의 작업입니다. 판정을 통과하면 엔진이 이 작업 가지를 스토리 가지(`\(def(n)["story_branch"] ?? "")`)에 합칩니다. 직접 합치지 않습니다.\n"
        }
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
        if let task = def(n)["branch"] as? String { u["branch"] = task }
        saveUnit(n, u)
        queue[n - 1]["status"] = status
        saveQueue()
        if status != "done" { noteUnmerged(n, unitLabel[status] ?? status) }   // Jira 작업: 합치지 않고 작업 가지만 남긴다
        let spent = "\(dur(num(u["active_sec"]) ?? 0)), \(usd(num(u["cost_usd"]) ?? 0)) USD"
        event("board", "단위 \(n)/\(total) \(unitLabel[status] ?? status): \(reason) (\(spent))")
        if !ok, status != "stopped" { event("board", "모델이 결과 기록(result.json)을 남기지 않았습니다.") }
        record("단위 \(n) 판정 뒤")
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
                    var why = "판정 명령을 통과했습니다."
                    if def(n)["story"] != nil {   // Jira 작업: 판정 통과 때 엔진이 바로 스토리 가지로 합친다
                        let m = mergeTask(n)
                        why += m["state"] as? String == "merged" ? " 스토리 가지에 합쳤습니다." : m["state"] as? String == "nothing" ? " 합칠 커밋이 없었습니다." : " 스토리 가지에는 합치지 않았습니다."
                    }
                    endUnit(n, "done", why); return
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
        // 가지 보호는 파일 도구에도(Write 로 ref 파일을 고쳐 쓰는 길). 폴더 자리(스토리 가지 꼴)는 안까지
        for p in protectDeny() { denied += ["Edit(/\(p))"] + (isDir(p) && !isLink(p) ? ["Edit(/\(p)/**)"] : []) }
        // 사용자 settings 는 읽지 않는다(--setting-sources project,local): 사용자 hook(알림 패널)·플러그인 hook 은 샌드박스 밖에서 돌고,
        // 사용자 허용 규칙의 WebFetch(domain:…) 는 샌드박스 네트워크 허용 목록에 합쳐져 고정한 목록을 넓힌다. ~/.claude/CLAUDE.md 도 함께 빠진다(실측)
        a += ["--setting-sources", "project,local"]
        a += ["--permission-mode", "dontAsk", "--permission-prompts", "none", "--strict-mcp-config", "--settings", sandboxSettings(n),
              "--allowedTools", allowed.joined(separator: " "), "--disallowedTools", denied.joined(separator: " ")]
        for d in [unitDir(n)] + writeRoots { a += ["--add-dir", d] }
        // 프로젝트 자율주행 지시는 공통 규칙 뒤에 붙인다. 저장소 CLAUDE.md 는 사용자 메시지로 들어가므로(공식 문서) 이 지시가 시스템 프롬프트 쪽에서 앞선다
        a += ["--append-system-prompt", systemPromptText + (instruction.map { "\n\n# 이 프로젝트의 자율주행 지시(보드가 출발 때 고정함)\n\n" + $0 } ?? "")]
        // 서브에이전트에는 위 글이 닿지 않는다(실측) — 고정 플러그인의 SubagentStart hook 이 같은 규칙을 넣는다
        if fm.fileExists(atPath: pluginDir + "/hooks/hooks.json") { a += ["--plugin-dir", pluginDir] }
        a += cfg["claude_args"] as? [String] ?? []    // 시험용(--dev 출발만, 예: --setting-sources project,local)
        return a
    }

    // 이번 단위의 샌드박스 설정 파일(엔진 비공개 폴더, 0600). Bash 와 그 자식 프로세스에 운영체제가 건다:
    // 쓰기는 작업 폴더·write_roots·단위 폴더·임시 폴더, 그 안에서도 하네스 자리는 쓰기 금지, 네트워크는 허용 목록만(git 원격 없음)
    func sandboxSettings(_ n: Int) -> String {
        var deny = harnessDeny([cwd(n)] + writeRoots)
        deny += ["engine.json", "stream.jsonl", "stderr.log", "evidence", "context"].map { unitDir(n) + "/" + $0 }
        deny += protectDeny()   // 지금 작업 가지 밖의 가지·HEAD·packed-refs(단위마다, 엔진이 가지를 바꾼 뒤 다시 계산)
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
                    for name in names where !skip.contains(name) && isDir(d + "/" + name) && !isLink(d + "/" + name) { next.append(d + "/" + name) }
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
        // fsmonitor 는 claudeEnv 에 이미 있다. 모델이 저장소 설정·attributes·replace 에 명령을 심어도 밖에서 도는 판정·고정 단계의 git 이 실행하지 않게
        for (key, value) in [("core.hooksPath", "/dev/null"), ("commit.gpgsign", "false"), ("core.attributesFile", "/dev/null")] {
            e["GIT_CONFIG_KEY_\(k)"] = key; e["GIT_CONFIG_VALUE_\(k)"] = value; k += 1
        }
        e["GIT_CONFIG_COUNT"] = String(k)
        e["GIT_CONFIG_NOSYSTEM"] = "1"
        e["GIT_CONFIG_GLOBAL"] = "/dev/null"
        e["GIT_ATTR_NOSYSTEM"] = "1"
        e["GIT_NO_REPLACE_OBJECTS"] = "1"
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

    // 판정 미리 돌리기(출발 전)용 고정 명령 하나: 단위 상태·control 없이, 로그인 셸·작업 폴더·고른 환경 + $CLAUDE_BOARD_EVIDENCE_DIR. 출력은 ev/output.log
    func plainCommand(_ n: Int, _ cmd: String, ev: String, limit: Double) -> (code: Int32, tail: String, sec: Double, timedOut: Bool) {
        try? fm.createDirectory(atPath: ev, withIntermediateDirectories: true)
        let log = ev + "/output.log"
        var env = commandEnv(n)
        env["CLAUDE_BOARD_EVIDENCE_DIR"] = ev
        guard let c = Child(shell, ["-l", "-c", cmd], env: env, cwd: cwd(n), outFile: log, newGroup: true) else { return (-1, "명령을 띄우지 못했습니다", 0, false) }
        let start = now()
        let finished = c.wait(limit)
        killGroup(c.pid)   // 끝났어도 뒤에 남긴 프로세스까지
        _ = c.wait(3)
        let tail = String(decoding: (fm.contents(atPath: log) ?? Data()).suffix(4000), as: UTF8.self)
            .split(separator: "\n", omittingEmptySubsequences: false).suffix(40).joined(separator: "\n")
        return (finished ? (c.code ?? -1) : -1, tail, now() - start, !finished)
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
           claude-autopilot status <실행 폴더> [--dev]
           claude-autopilot records import [--dev | --db <파일>] [--project <이름>] [--unit-key <칸 경로>] <원본…>
             원본: 실행 폴더(run.json·queue.json 이 있는 폴더) 또는 일반 꼴 대기열 파일(format "claude-board-queue")
           claude-autopilot prerun <실행 폴더> [--dev]
             출발 전: 판정을 한 번씩 미리 돌려 일하기 전부터 통과하는 단위를 prerun.json 에(출발 카드 감)
           claude-autopilot finish <실행 폴더> merge --into <가지> [--story <키>]
           claude-autopilot finish <실행 폴더> promote <가지> --into <가지>
           claude-autopilot finish <실행 폴더> discard [--story <키>]
             끝난 실행의 가지 승격(사람이 부름): 자율주행 가지(Jira 실행은 스토리 가지)를 합치기·기존 가지 올리기·합치기 전에 버리기
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
    // title 도 고정한다 — 엔진이 다시 뜰 때 run.json 설정 칸을 고정 값으로 되돌려 쓰므로, 빠지면 실행 제목이 지워졌다(엔진 고침 1차에서 찾음)
    for k in ["title", "effort", "stopline", "caps", "check_timeout_min", "check_retries", "precheck_retry_min", "background_quiet_sec"] where run[k] != nil { f[k] = run[k] }
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
    // 프로젝트와 자율주행 세트: 세트는 이름으로 엔진 전용 뿌리 아래에서만 찾는다(작업 폴더처럼 모델이 쓸 수 있는 자리를 가리키지 못하게).
    // 지시 글은 고정 설정에 넣어 해시로 지키고, 출발 때 pinned/set/ 에 사본을 둔다
    if let p = run["project"] {
        if let name = p as? String, validName(name) {
            f["project"] = name
            let sd = setDir(project: name, dev: dev)
            var set: [String: Any] = ["dir": sd]
            if fm.fileExists(atPath: sd + "/autopilot.md") {
                if let t = try? String(contentsOfFile: sd + "/autopilot.md", encoding: .utf8) {
                    let tt = t.trimmingCharacters(in: .whitespacesAndNewlines)
                    if !tt.isEmpty { set["instruction"] = tt }
                } else { problems.append("자율주행 지시(\(sd)/autopilot.md)를 UTF-8 글로 읽지 못했습니다") }
            }
            if fm.fileExists(atPath: sd + "/spec.md") {
                if let t = try? String(contentsOfFile: sd + "/spec.md", encoding: .utf8) { set["spec"] = t }
                else { problems.append("명세(\(sd)/spec.md)를 UTF-8 글로 읽지 못했습니다") }
            }
            if let d = fm.contents(atPath: sd + "/stack.json") {
                if let j = try? JSONSerialization.jsonObject(with: d) { set["stack"] = j } else { problems.append("기술 스택(\(sd)/stack.json)이 JSON 이 아닙니다") }
            }
            f["set"] = set
        } else { problems.append("run.json 의 project 는 영문·숫자·.-_ 로 쓴 프로젝트 이름입니다") }
    }
    if let pr = run["project_root"] {
        if let s = pr as? String, s.hasPrefix("/"), isDir(s) { f["project_root"] = canonical(s) }
        else { problems.append("run.json 의 project_root 는 있는 폴더의 절대 경로여야 합니다") }
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
        // 기록 저장소에 남길 칸(원천 항목 키·종류·출발 때 예상). 판정·도구와 상관없지만 출발 뒤 바뀌지 않게 함께 고정한다
        for k in ["source_key", "kind", "est_basis"] {
            if let v = q[k] { if v is String { d[k] = v } else { problems.append("단위 \(n) 의 \(k) 는 글자여야 합니다") } }
        }
        if let v = q["est_min"] { if num(v) != nil, !(v is String) { d["est_min"] = v } else { problems.append("단위 \(n) 의 est_min 은 수여야 합니다") } }
        // 가지 칸: Jira 단위의 스토리 키·작업 가지, 기대는 앞 단위(id), 판정 미리 돌리기에 넣을지(기본 true)
        for k in ["story", "branch"] {
            if let v = q[k] { if let s = v as? String, !s.isEmpty { d[k] = s } else { problems.append("단위 \(n) 의 \(k) 는 글자여야 합니다") } }
        }
        if let v = q["depends"] {
            if let a = v as? [String] {
                let earlier = units.compactMap { $0["id"] as? String }
                for x in a where !earlier.contains(x) { problems.append("단위 \(n) 의 depends 에 적은 \(x) 는 앞선 단위의 id 가 아닙니다") }
                // 기대는 단위는 한 스토리(설계 2장 끝, 2026-10-27): 스토리 가지는 출발 가지 머리에서 만들어 다른 스토리의 앞 단위 변경이 없다.
                // story 가 둘 다 없으면 같음(Jira 아닌 실행은 그대로), Jira 단위와 Jira 아닌 단위 사이도 거부
                for x in a {
                    guard let j = units.firstIndex(where: { $0["id"] as? String == x }) else { continue }
                    let mine = d["story"] as? String, theirs = units[j]["story"] as? String
                    if mine != theirs {
                        problems.append("단위 \(n) 의 depends 가 다른 스토리의 단위를 가리킵니다: 단위 \(n) 의 스토리 \(mine ?? "없음"), 기대는 단위 \(j + 1) 의 스토리 \(theirs ?? "없음"). 기대는 단위는 같은 스토리에 둡니다(스토리 가지는 출발 가지에서 따로 만들어 앞 단위의 변경이 들어오지 않습니다)")
                    }
                }
                d["depends"] = a
            } else { problems.append("단위 \(n) 의 depends 는 앞선 단위 id 의 배열이어야 합니다") }
        }
        if let v = q["prerun"] { if v is Bool { d["prerun"] = v } else { problems.append("단위 \(n) 의 prerun 은 true/false 입니다") } }
        if d["branch"] != nil, d["story"] == nil { problems.append("단위 \(n) 의 branch 는 story 와 함께 씁니다(Jira 연결 프로젝트의 작업 가지)") }
        units.append(d)
        queue[i]["id"] = d["id"]; queue[i]["tickets"] = d["tickets"]; queue[i]["status"] = "pending"
    }
    // 가지(설계 2장 끝): 단위 작업 폴더·쓰기 범위가 든 저장소마다 출발 가지에서 autopilot/<실행 id> 를 만든다. 출발 가지는 run.json base_branch
    // (가지 이름 하나 또는 {저장소 경로: 가지}), 없으면 지금 걸린 가지
    let repos = gitRepos(units.compactMap { $0["cwd"] as? String } + roots)
    f["repos"] = repos
    if let b = run["base_branch"] {
        if let s = b as? String, !s.isEmpty { f["base_branch"] = s }
        else if let m = b as? [String: Any], m.values.allSatisfy({ $0 is String }) {
            var mm: [String: Any] = [:]
            for (k, v) in m {
                let c = canonical(k)
                if repos.contains(c) { mm[c] = v } else { problems.append("base_branch 에 적은 \(k) 는 이 실행의 저장소가 아닙니다") }
            }
            f["base_branch"] = mm
        } else { problems.append("run.json 의 base_branch 는 가지 이름 하나 또는 {저장소 경로: 가지 이름} 입니다") }
    }
    let work = "autopilot/" + id
    for r in repos {
        guard let base = baseBranch(f, r) else {
            problems.append("\(r) 가 가지에 걸려 있지 않습니다(detached HEAD). run.json 의 base_branch 로 출발 가지를 고르세요"); continue
        }
        if !branchExists(r, base) { problems.append("\(r) 에 출발 가지 \(base) 가 없습니다") }
        if branchExists(r, work) { problems.append("\(r) 에 이미 \(work) 가지가 있습니다(같은 실행 id 를 다시 썼습니다)") }
    }
    // Jira 연결 프로젝트의 단위(story 칸): 스토리 가지·작업 가지 이름을 프로젝트 설정의 꼴로 정해 고정한다. 둘 다 이 실행이 새로 만들 가지여야 한다
    // (엔진이 스스로 움직이는 가지는 이 실행이 만든 것뿐 — 이미 있는 가지면 출발하지 않는다)
    if units.contains(where: { $0["story"] != nil }) {
        let t = (f["project"] as? String).flatMap { projectSettings($0, dev: dev)?["tickets"] as? [String: Any] } ?? [:]
        let names = t["branch_names"] as? [String: Any] ?? [:]
        let sp = names["story"] as? String ?? defaultStoryPattern, tp = names["task"] as? String ?? defaultTaskPattern
        let how = t["merge"] as? String ?? "merge"
        let typeByKind = t["type_by_kind"] as? [String: Any] ?? [:]   // 단위 종류(kind) → {type} 자리표 값(예 {"fix":"fix","dev":"feature"})
        let needsType = sp.contains("{type}") || tp.contains("{type}")
        if !sp.contains("{story}") { problems.append("프로젝트 설정 tickets.branch_names.story 꼴에 {story} 가 있어야 합니다") }
        if !tp.contains("{task}") { problems.append("프로젝트 설정 tickets.branch_names.task 꼴에 {task} 가 있어야 합니다") }
        if !["merge", "ff"].contains(how) { problems.append("프로젝트 설정 tickets.merge 는 merge(병합 커밋) 또는 ff(앞으로 감기)입니다") }
        f["tickets"] = ["branch_names": ["story": sp, "task": tp], "merge": how, "type_by_kind": typeByKind]
        // 한 스토리는 같은 종류만(2026-10-27 사용자 결정): {type} 을 단위 종류로 채우면 섞인 스토리의 스토리 가지가 갈린다.
        // 꼴에 {type} 이 없어도 같은 규칙으로 막는다. kind 가 없거나 빈 단위는 "없음" 한 값으로 센다
        var storyOrder: [String] = [], kindsOf: [String: [(kind: String, n: Int)]] = [:]
        for (i, u) in units.enumerated() {
            guard let s = u["story"] as? String else { continue }
            if kindsOf[s] == nil { storyOrder.append(s) }
            let k = u["kind"] as? String ?? ""
            kindsOf[s, default: []].append((k.isEmpty ? "없음" : k, i + 1))
        }
        for s in storyOrder {
            let ks = kindsOf[s] ?? []
            var kinds: [String] = []
            for k in ks where !kinds.contains(k.kind) { kinds.append(k.kind) }
            guard kinds.count > 1 else { continue }
            let detail = kinds.map { k in "\(k)(단위 " + ks.filter { $0.kind == k }.map { "\($0.n)" }.joined(separator: "·") + ")" }.joined(separator: ", ")
            problems.append("스토리 \(s) 에 단위 종류가 섞여 있습니다: \(detail). 한 스토리에는 같은 종류의 단위만 묶습니다")
        }
        var seen = Set<String>()
        for i in units.indices {
            guard let story = units[i]["story"] as? String else { continue }
            let n = i + 1, cwd = units[i]["cwd"] as? String ?? ""
            guard let r = repos.filter({ cwd == $0 || cwd.hasPrefix($0 + "/") }).max(by: { $0.count < $1.count }) else {
                problems.append("단위 \(n) 은 story 가 있는데 작업 폴더가 git 저장소 안이 아닙니다"); continue
            }
            var type: String? = nil
            if needsType {   // 가지 꼴에 {type} 이 있으면 단위 종류로 찾는다. 없으면 출발 거부(사용자 결정 2026-10-22)
                let kind = units[i]["kind"] as? String ?? ""
                guard let ty = typeByKind[kind] as? String, !ty.isEmpty else {
                    problems.append("단위 \(n) 의 가지 꼴에 {type} 이 있는데 종류(\(kind.isEmpty ? "없음" : kind))에 맞는 tickets.type_by_kind 가 없습니다"); continue
                }
                type = ty
            }
            let key = (units[i]["tickets"] as? [Any])?.first.map { "\($0)" }
            guard let task = units[i]["branch"] as? String ?? key.map({ renderBranch(tp, type: type, story: story, task: $0) }) else {
                problems.append("단위 \(n) 은 story 가 있는데 tickets 에 작업 키도 branch 도 없습니다"); continue
            }
            let sb = renderBranch(sp, type: type, story: story)
            units[i]["branch"] = task; units[i]["story_branch"] = sb
            queue[i]["branch"] = task
            for b in [sb, task] where !validBranch(b, cwd: r) { problems.append("단위 \(n) 의 가지 이름 \(b) 가 git 가지 이름 꼴이 아닙니다") }
            if branchExists(r, sb) { problems.append("\(repoName(r)) 에 스토리 가지 \(sb) 가 이미 있습니다(엔진은 이 실행이 만든 가지만 움직입니다)") }
            if branchExists(r, task) { problems.append("\(repoName(r)) 에 작업 가지 \(task) 가 이미 있습니다") }
            if !seen.insert(task).inserted { problems.append("작업 가지 \(task) 를 두 단위가 함께 씁니다") }
        }
    }
    // 어디서 도는가(설계 2장 끝 "어디서 도는가"): 단일 저장소면 따로 작업 트리, 묶음·확인 실패면 원래 작업 트리. worktree_mode 로 강제할 수 있다.
    // 함께 둘 파일 목록(git 이 무시하는 .env·데이터 등)은 run.json worktree_files 또는 세트 worktree_files.json
    if let wf = run["worktree_files"] {
        if let a = wf as? [Any] { f["worktree_files"] = a } else { problems.append("run.json 의 worktree_files 는 배열이어야 합니다") }
    } else if let sd = (f["set"] as? [String: Any])?["dir"] as? String, let a = readJSON(sd + "/worktree_files.json") as? [Any] {
        f["worktree_files"] = a
    }
    // 보안(2026-10-22 둘째·셋째): git config 의 include 가 모델 쓰기 범위 안을 가리키면 모델이 그 파일로 filter 를 심을 수 있어 출발을 막는다.
    // 범위는 원래 경로로 잰다(따로 작업 트리도 precheck 실패면 원래 작업 트리로 떨어진다). 저장소마다 공통 .git·git 폴더를 함께 범위에 넣는다
    var gitDirsOf: [String: (common: String, git: String)] = [:]
    for r in repos {
        var common = gitOut(["rev-parse", "--git-common-dir"], cwd: r) ?? r + "/.git"
        if !common.hasPrefix("/") { common = canonical(r + "/" + common) }
        gitDirsOf[r] = (canonical(common), canonical(gitOut(["rev-parse", "--absolute-git-dir"], cwd: r) ?? common))
    }
    let scope = includeScope(units.compactMap { $0["cwd"] as? String } + roots, gitDirs: gitDirsOf.values.flatMap { [$0.common, $0.git] }, runDir: dir)
    for r in repos {
        guard let g = gitDirsOf[r] else { continue }
        for p in configIncludes(g.common, gitDir: g.git, scope: scope).risky {
            problems.append("\(repoName(r)) 의 git config \(p) 출발하지 않습니다(모델이 고칠 수 있는 파일로 엔진 밖 git 에 명령을 심을 수 있습니다. include 를 범위 밖으로 옮기거나 고치세요)")
        }
    }
    let forced = run["worktree_mode"] as? String ?? (f["project"] as? String).flatMap { projectSettings($0, dev: dev)?["worktree_mode"] as? String }
    let (wtOK, wtReason) = worktreeChecks(repos, units, roots)
    switch forced {
    case "in_place": f["worktree_mode"] = "in_place"; f["worktree_reason"] = "worktree.mode 설정으로 원래 작업 트리에서 돕니다"
    case "worktree":
        if wtOK { f["worktree_mode"] = "worktree"; f["worktree_reason"] = wtReason }
        else { problems.append("worktree.mode 로 따로 작업 트리를 강제했지만 될 수 없습니다: \(wtReason)") }
    default:
        f["worktree_mode"] = wtOK ? "worktree" : "in_place"; f["worktree_reason"] = wtReason
    }
    f["units"] = units
    return (problems, f, queue)
}

// 단일 저장소 확인(설계 2장 끝): ① 한 저장소 ② submodule 없음 ③ 형제 경로(../) 없음. ④ 작업 트리 precheck 는 cmdStart 가 만든 뒤 본다
func worktreeChecks(_ repos: [String], _ units: [[String: Any]], _ roots: [String]) -> (ok: Bool, reason: String) {
    if repos.count != 1 { return (false, repos.isEmpty ? "git 저장소에서 돌지 않아 원래 자리에서 돕니다" : "저장소 묶음(\(repos.count)곳)이라 원래 작업 트리에서 돕니다") }
    let r = repos[0]
    if fm.fileExists(atPath: r + "/.gitmodules") { return (false, "submodule 이 있어(.gitmodules) 원래 작업 트리에서 돕니다") }
    for (i, u) in units.enumerated() {
        for k in ["prompt", "check", "precheck", "action"] {
            if let s = u[k] as? String, let hit = siblingHit(s) { return (false, "단위 \(i + 1) 의 \(k) 가 형제 경로(\(hit))를 가리켜 원래 작업 트리에서 돕니다") }
        }
    }
    for hf in ["CLAUDE.md", "CLAUDE.local.md", ".mcp.json", ".claude/settings.json", ".claude/settings.local.json"] {
        if let t = try? String(contentsOfFile: r + "/" + hf, encoding: .utf8), let hit = siblingHit(t) {
            return (false, "저장소 \(hf) 가 형제 경로(\(hit))를 가리켜 원래 작업 트리에서 돕니다")
        }
    }
    return (true, "단일 저장소라 따로 작업 트리에서 돕니다")
}
// 형제 저장소 상대 경로(../<다른 저장소>)로 보이는 토큰. 보수적으로 잡아 의심스러우면 원래 작업 트리로(안전한 쪽)
func siblingHit(_ s: String) -> String? {
    for tok in s.split(whereSeparator: { " \t\n\"'`()[]".contains($0) }) {
        if tok.hasPrefix("../") || tok.contains("/../") { return String(tok.prefix(60)) }
    }
    return nil
}
// ④ 작업 트리 precheck 를 돌릴 때 쓰는 환경(출발 전, Engine 없이): 로그인 환경에서 고른 것 + 밖에서 도는 git 보호
func worktreeCheckEnv(_ f: [String: Any]) -> [String: String] {
    var e = passEnv(captureLoginEnv(loginShell()) ?? ["PATH": "\(home)/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin", "HOME": home],
                    extra: f["env_pass"] as? [String] ?? [])
    if e["JAVA_HOME"] == nil { let jh = runTool("/usr/libexec/java_home", [], timeout: 10); let p = jh.out.trimmingCharacters(in: .whitespacesAndNewlines); if jh.code == 0, isDir(p) { e["JAVA_HOME"] = p } }
    for (i, kv) in [("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")].enumerated() { _ = i; e[kv.0] = kv.1 }
    return e
}
// 출발 전 명령 하나(precheck)를 로그인 셸로 cwd 에서 돌린다. 종료 코드(2분 상한, 못 돌리면 -1)
func runCheckAt(_ cmd: String, cwd: String, env: [String: String]) -> Int32 {
    guard let c = Child(loginShell(), ["-l", "-c", cmd], env: env, cwd: cwd, outFile: "/dev/null", newGroup: true) else { return -1 }
    let ok = c.wait(120)
    killGroup(c.pid); _ = c.wait(2)
    return ok ? (c.code ?? -1) : -1
}

// run.json 에 내놓는 세트 요약(보드가 그린다). 글 전문은 고정 설정과 pinned/set/ 에만 둔다
func setSummary(_ f: [String: Any]) -> Any {
    guard let set = f["set"] as? [String: Any] else { return NSNull() }
    var o: [String: Any] = ["project": f["project"] ?? NSNull(), "dir": set["dir"] ?? NSNull(),
                            "spec": set["spec"] != nil, "stack": set["stack"] != nil]
    if let t = set["instruction"] as? String {
        o["first_line"] = short(t, 200); o["chars"] = t.count; o["sha256"] = sha256(Data(t.utf8))
    }
    return o
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
    // 가지·작업 트리를 먼저 만들고 경로를 바꾼 뒤에 고정 설정을 쓴다(remap 이 해시에 들어가야 한다, 설계 4장 "시작과 끝").
    // 따로 작업 트리(단일 저장소)면 작업 트리를 만들고 ④ precheck 를 그 자리에서 보고, 실패하면 원래 작업 트리로 떨어진다
    var fz = frozen
    let scope = includeScope((fz["units"] as? [[String: Any]] ?? []).compactMap { $0["cwd"] as? String } + (fz["write_roots"] as? [String] ?? []), gitDirs: [], runDir: dir)
    var branches: [String: Any] = [:]
    func inPlace(_ reason: String) {
        let (b, err) = startBranches(fz, id: id, scope: scope)
        if let err { fail("출발하지 않았습니다: \(err)") }
        branches = b
        if !(b["repos"] as? [String: Any] ?? [:]).isEmpty {
            run["branches"] = b
            let rs = fz["repos"] as? [String] ?? []
            run["worktree"] = ["mode": "in_place", "repos": rs, "branch": "autopilot/" + id,
                               "base": (b["repos"] as? [String: Any] ?? [:]).mapValues { (($0 as? [String: Any])?["base"] as? [String: Any])?["head"] ?? "" },
                               "reason": reason + (rs.count > 1 ? " (저장소 \(rs.count)곳)" : "")]
        }
    }
    if fz["worktree_mode"] as? String == "worktree" {
        let (b, w, wtPath, repo, err) = startWorktree(fz, id: id, scope: scope)
        if let err { fail("출발하지 않았습니다: \(err)") }
        // ④ 작업 트리에서 precheck 가 통과하는지 본다. 실패하면 작업 트리를 지우고 원래 작업 트리로
        var test = fz; remapToWorktree(&test, from: repo, to: wtPath)
        var precheckFail: String?
        if (test["units"] as? [[String: Any]] ?? []).contains(where: { ($0["precheck"] as? String ?? "").isEmpty == false }) {
            let penv = worktreeCheckEnv(fz)
            for (i, u) in (test["units"] as? [[String: Any]] ?? []).enumerated() {
                guard let pc = u["precheck"] as? String, !pc.isEmpty, let cwd = u["cwd"] as? String else { continue }
                if runCheckAt(pc, cwd: cwd, env: penv) != 0 { precheckFail = "단위 \(i + 1) 의 시험 환경 확인이 작업 트리에서 실패"; break }
            }
        }
        if let pf = precheckFail {
            rollbackBranches(b)
            inPlace("따로 작업 트리의 \(pf)해 원래 작업 트리에서 돕니다")
        } else {
            fz = test; branches = b; run["branches"] = b; run["worktree"] = w
        }
    } else {
        inPlace(fz["worktree_reason"] as? String ?? "원래 작업 트리에서 돕니다")
    }
    let frozenData = Data(jsonText(fz, pretty: true).utf8)
    guard writePrivate(dir + "/pinned/frozen.json", frozenData) else { rollbackBranches(branches); fail("고정 설정을 쓰지 못했습니다") }
    // 근거 묶음도 출발 때 사본을 떠 둔다(출발 뒤에 원본이 바뀌어도 단위가 받는 묶음은 같다). 단위를 시작할 때 units/<n>/context/ 로 넘긴다. context 는 바꾸지 않는다
    try? fm.removeItem(atPath: dir + "/pinned/context")
    for (i, d) in (fz["units"] as? [[String: Any]] ?? []).enumerated() {
        guard let src = d["context"] as? String else { continue }
        try? fm.createDirectory(atPath: dir + "/pinned/context", withIntermediateDirectories: true)
        do { try fm.copyItem(atPath: src, toPath: dir + "/pinned/context/\(i + 1)") } catch { rollbackBranches(branches); fail("단위 \(i + 1) 의 근거 묶음을 복사하지 못했습니다: \(error.localizedDescription)") }
    }
    // 프로젝트 자율주행 세트의 사본: 고정 설정에 넣은 글 그대로(보드·사람이 보는 자리. 엔진은 고정 설정의 글을 쓴다)
    try? fm.removeItem(atPath: dir + "/pinned/set")
    if let set = fz["set"] as? [String: Any], set["instruction"] != nil || set["spec"] != nil || set["stack"] != nil {
        mkdirPrivate(dir + "/pinned/set")
        if let t = set["instruction"] as? String { writePrivate(dir + "/pinned/set/autopilot.md", Data((t + "\n").utf8)) }
        if let t = set["spec"] as? String { writePrivate(dir + "/pinned/set/spec.md", Data(t.utf8)) }
        if let st = set["stack"] { writePrivate(dir + "/pinned/set/stack.json", Data((jsonText(st, pretty: true) + "\n").utf8)) }
    }
    // 하네스 사본과 지문: 단위 작업 폴더들의 저장소 뿌리마다(따로 작업 트리면 작업 트리), 엔진 전용 폴더(모델은 읽지 못함)에 둔다. 끝에 다시 재어 같은지 적는다
    let ep = privateRoot(dev: dev) + "/" + id
    mkdirPrivate(privateRoot(dev: dev)); mkdirPrivate(ep)
    let cwds = (fz["units"] as? [[String: Any]] ?? []).compactMap { $0["cwd"] as? String }
    run["harness"] = harnessSnapshot(harnessFolders(cwds), to: ep + "/harness")
    run["set"] = setSummary(fz)
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
          (try? data.write(to: URL(fileURLWithPath: plist))) != nil else {
        rollbackBranches(branches)
        run["state"] = "error"; run["reason"] = "LaunchAgent 파일을 쓰지 못했습니다."; run["wrapped"] = r3(now())
        writeJSON(dir + "/run.json", run)
        fail("LaunchAgent 파일을 쓰지 못했습니다: \(plist) (가지는 출발 때로 되돌렸습니다)")
    }
    let r = runTool("/bin/launchctl", ["bootstrap", "gui/\(uid())", plist])
    guard r.code == 0 else {
        try? fm.removeItem(atPath: plist)
        rollbackBranches(branches)
        run["state"] = "error"; run["reason"] = "launchd 에 올리지 못했습니다."; run["wrapped"] = r3(now())
        writeJSON(dir + "/run.json", run)
        fail("launchctl bootstrap 이 실패했습니다(코드 \(r.code)). 가지는 출발 때로 되돌렸습니다")
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
func statusBeforeStart(_ dir: String, _ run: [String: Any], dev devFlag: Bool) -> Never {
    let dev = devFlag || run["claude_args"] != nil || run["claude_path"] != nil
    let (problems, f, _) = prepare(dir, dev: dev)
    var out = "실행 \((dir as NSString).lastPathComponent) · 출발 전\(run["title"].map { " · \($0)" } ?? "")\n"
    out += problems.isEmpty ? "출발 준비 확인: 이상 없음\n" : "출발 준비 확인: 고칠 것 \(problems.count)개\n" + problems.map { "  - \($0)\n" }.joined()
    let sl = f["stopline"] as? [String: Any] ?? [:]
    let pct = { (k: String, d: Double) in "\(Int(((num(sl[k]) ?? d) * 100).rounded()))%" }
    out += "모델 \(f["model"] ?? "?")\((f["effort"] as? String).map { " · effort \($0)" } ?? "") · 멈출 선 주간 \(pct("seven_day", 0.80)) · 5시간 \(pct("five_hour", 0.95))\n"
    out += "쓰기 범위: 단위마다 작업 폴더 + \((f["write_roots"] as? [String] ?? []).joined(separator: ", ").ifEmpty("(더 없음)")) + 단위 폴더 + 임시 폴더\n"
    out += "네트워크: \((f["network_allow"] as? [String] ?? defaultNetwork).joined(separator: ", ").ifEmpty("(없음)"))\((f["local_binding"] as? Bool ?? false) ? " · localhost 모든 포트 열림" : "")\n"
    out += "읽기 금지: \((f["read_deny"] as? [String] ?? []).count)곳 + 엔진 전용 뿌리(실제·시험 판), 모든 실행의 engine 폴더, 옛 뿌리(~/.claude/notifier/autopilot)\n"
    // 어디서와 가지: 단일 저장소면 따로 작업 트리, 묶음·확인 실패면 원래 작업 트리를 출발 가지에서 만든 autopilot/<실행 id> 로 바꿔 돈다
    let id = (dir as NSString).lastPathComponent
    let wtMode = f["worktree_mode"] as? String == "worktree"
    out += "어디서: \(wtMode ? "따로 작업 트리(~/.claude-board/worktrees/\(id)/<저장소>/)" : "원래 작업 트리")\(f["worktree_reason"].map { " — \($0)" } ?? "")\n"
    for r in f["repos"] as? [String] ?? [] {
        let cur = currentBranch(r), base = baseBranch(f, r) ?? "?"
        let dirty = porcelain(r)
        if wtMode {
            out += "가지: \(repoName(r)) · 출발 가지 \(base)\(headOf(r, "refs/heads/" + base).map { "(\($0.prefix(7)))" } ?? "") 에서 autopilot/\(id) 를 만들어 따로 작업 트리에서 돕니다. 원래 작업 트리(지금 \(cur ?? "가지 없음"))는 그대로 둡니다\n"
            if !dirty.isEmpty { out += "  원래 작업 트리의 커밋 안 한 변경 \(dirty.count)개: 이 변경은 자율주행에 들어가지 않습니다(작업 트리는 출발 가지의 마지막 커밋에서 시작)\n" }
        } else {
            out += "가지: \(repoName(r)) · 출발 가지 \(base)\(headOf(r, "refs/heads/" + base).map { "(\($0.prefix(7)))" } ?? "") 에서 autopilot/\(id) 를 만들어 원래 작업 트리를 바꿈 · 지금 \(cur ?? "가지 없음"), 끝나면 깨끗할 때 되돌림\n"
            if !dirty.isEmpty { out += "  커밋 안 한 변경 \(dirty.count)개: 가지가 바뀌어도 이 변경은 작업 트리에 남고, 모델이 함께 커밋할 수 있습니다\n" }
            if let b = baseBranch(f, r), let c = cur, b != c { out += "  지금 걸린 가지(\(c))가 출발 가지(\(b))와 다릅니다. 판정 미리 돌리기는 지금 자리에서 돌므로 결과가 다를 수 있습니다\n" }
        }
    }
    if let wf = f["worktree_files"] as? [Any], !wf.isEmpty { out += "작업 트리에 함께 둘 파일: \(wf.count)개\n" }
    if let t = f["tickets"] as? [String: Any], let names = t["branch_names"] as? [String: Any] {
        out += "Jira 가지: 스토리 \(names["story"] ?? "?") · 작업 \(names["task"] ?? "?") · 작업 → 스토리는 판정 통과 때 엔진이 \(t["merge"] as? String == "ff" ? "앞으로 감기로" : "병합 커밋으로")\n"
    }
    if let e = f["env_pass"] as? [String], !e.isEmpty { out += "더 넘길 환경 변수: \(e.joined(separator: ", "))\n" }
    if let x = f["sandbox_exclude"] as? [String], !x.isEmpty { out += "샌드박스 밖에서 돌 명령: \(x.joined(separator: ", "))\n" }
    if let set = f["set"] as? [String: Any] {
        if let t = set["instruction"] as? String {
            out += "자율주행 지시: 「\(short(t, 120))」 (\(t.count)자, 공통 규칙 뒤에 붙임. 세트 \(set["dir"] ?? "?"))\n"
        } else {
            out += "자율주행 지시: 없음(세트 자리에 autopilot.md 가 없음: \(set["dir"] ?? "?")). 공통 규칙만으로 돕니다\n"
        }
        if let t = set["spec"] as? String { out += "명세: spec.md (\(t.count)자)\n" }
        if let st = set["stack"] { out += "기술 스택: \(short(jsonText(st), 160))\n" }
    } else {
        out += "자율주행 지시: 없음(run.json 에 project 칸이 없음). 공통 규칙만으로 돕니다\n"
    }
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
        if let k = d["story"] as? String { out += "     스토리 \(k): 작업 가지 \(d["branch"] ?? "?") ← 스토리 가지 \(d["story_branch"] ?? "?")\n" }
        if let a = d["depends"] as? [String], !a.isEmpty { out += "     기대는 앞 단위: \(a.joined(separator: ", ")) (통과하지 않으면 건너뜀)\n" }
        if d["prerun"] as? Bool == false { out += "     판정 미리 돌리기: 넣지 않음(prerun: false)\n" }
    }
    if let p = readJSON(dir + "/prerun.json") as? [String: Any] {
        let pass = p["already_passing"] as? [Any] ?? []
        out += "판정 미리 돌리기(\(num(p["at"]).map { when($0) } ?? "?")): 일하기 전부터 통과 \(pass.count)개\(pass.isEmpty ? "" : "(단위 " + pass.map { "\($0)" }.joined(separator: ", ") + ")")\n"
    }
    out += "출발: \"\(selfPath())\" start \"\(dir)\"\(dev ? " --dev" : "")\n"
    print(out, terminator: "")
    exit(problems.isEmpty ? 0 : 1)
}

extension String { func ifEmpty(_ s: String) -> String { isEmpty ? s : self } }

func cmdStatus(_ args: [String]) -> Never {
    let dir = runDir(args.first)
    guard let run = readJSON(dir + "/run.json") as? [String: Any] else { fail("run.json 을 읽지 못했습니다") }
    if run["state"] == nil { statusBeforeStart(dir, run, dev: args.contains("--dev")) }
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
    if let b = run["branches"] as? [String: Any] {
        for (r, v) in (b["repos"] as? [String: Any] ?? [:]).sorted(by: { $0.key < $1.key }) {
            let i = v as? [String: Any] ?? [:]
            let restored = i["restored"] as? Bool
            out += "가지: \(repoName(r)) · 출발 \((i["base"] as? [String: Any])?["branch"] ?? "?") → \(i["work"] ?? "?") · 지금 \(i["on"] ?? "?")"
                + (restored == nil ? "" : restored! ? " · 출발 때 가지로 되돌림" : " · 되돌리지 않음(\(i["restore_note"] ?? ""))") + "\n"
        }
        for (k, v) in (b["stories"] as? [String: Any] ?? [:]).sorted(by: { $0.key < $1.key }) {
            let s = v as? [String: Any] ?? [:]
            out += "스토리 \(k): \(s["branch"] ?? "?") · 합침 \((s["merged"] as? [Any])?.count ?? 0) · 합치지 않음 \((s["unmerged"] as? [Any])?.count ?? 0)\n"
        }
        if let m = b["moved"] as? [Any] { out += "움직인 기존 가지: \(m.count)개\n" }
        for p in b["promoted"] as? [[String: Any]] ?? [] { out += "합침: \(p["branch"] ?? "?") → \(p["into"] ?? "?") (\(repoName(p["repo"] as? String ?? "")), \(p["how"] ?? ""))\n" }
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

// 옮겨 담을 원본의 지문: 파일이면 내용, 실행 폴더면 읽는 파일들(경로와 내용). 같은 경로·같은 지문은 다시 옮기지 않는다(imports 표)
func sourceDigest(_ p: String) -> String {
    guard isDir(p) else { return sha256(fm.contents(atPath: p) ?? Data()) }
    var names = ["run.json", "queue.json", "cards.json", "answers.json"]
    let units = ((try? fm.contentsOfDirectory(atPath: p + "/units")) ?? []).filter { Int($0) != nil }.sorted { Int($0)! < Int($1)! }
    for u in units { names += ["units/\(u)/engine.json", "units/\(u)/result.json"] }
    var d = Data()
    for n in names {
        guard let c = fm.contents(atPath: p + "/" + n) else { continue }
        d.append(Data((n + "\u{0}").utf8)); d.append(Data(sha256(c).utf8)); d.append(0x0a)
    }
    return sha256(d)
}

// 실행 기록 저장소에 지난 기록을 옮겨 담는다. 원본은 읽기만 한다. 일반 꼴 대기열을 먼저 옮기고(단위의 예상을 대기열 항목에서 찾아 붙이므로) 실행 폴더를 옮긴다
func cmdRecords(_ args: [String]) -> Never {
    guard args.first == "import" else { usage() }
    var dev = false, dbArg: String?, project: String?, unitKey: String?, sources: [String] = []
    var i = 1
    func next() -> String { i += 1; guard i < args.count else { usage() }; return args[i] }
    while i < args.count {
        switch args[i] {
        case "--dev": dev = true
        case "--db": dbArg = next()
        case "--project": project = next()
        case "--unit-key": unitKey = next()
        default: sources.append(args[i])
        }
        i += 1
    }
    guard !sources.isEmpty else { usage() }
    if let p = project, !validName(p) { fail("--project 는 영문·숫자·.-_ 로 쓴 이름입니다: \(p)") }
    let path = dbArg.map { canonical($0) } ?? recordsPath(dev: dev)
    if dbArg == nil { mkdirPrivate(privateRoot(dev: dev)) }
    let db: RecordsDB
    do { db = try RecordsDB(path) } catch { fail("실행 기록 저장소를 열지 못했습니다(\(path)): \(error)") }
    var files: [String] = [], dirs: [String] = []
    for s in sources {
        let p = canonical(s)
        if isDir(p) { dirs.append(p) } else if fm.fileExists(atPath: p) { files.append(p) } else { fail("원본이 없습니다: \(s)") }
    }
    print("실행 기록 저장소: \(path)")
    var bad = 0
    for p in files + dirs {
        let digest = sourceDigest(p)
        if (try? db.rows("SELECT rows FROM imports WHERE path=? AND sha256=?", [p, digest]))?.isEmpty == false {
            print("  그대로: 같은 내용을 이미 옮겨 담았습니다: \(p)")
            continue
        }
        do {
            var n = 0
            try db.transaction {
                if isDir(p) {
                    guard fm.fileExists(atPath: p + "/run.json") else { throw DBError(description: "실행 폴더가 아닙니다(run.json 없음)") }
                    let r = readJSON(p + "/run.json") as? [String: Any] ?? [:]
                    guard project != nil || r["project"] is String else { throw DBError(description: "프로젝트를 모릅니다(run.json 에 project 가 없으면 --project 로 줍니다)") }
                    n = try db.syncRun(RunSource(dir: p, origin: "import", project: project, unitKey: unitKey, frozen: nil))
                } else {
                    n = try db.importQueue(p, project: project)
                }
                try db.run("INSERT INTO imports(path,sha256,rows,at) VALUES(?,?,?,?) ON CONFLICT(path,sha256) DO NOTHING", [p, digest, n, r3(now())])
            }
            print("  옮겨 담음: \(p) (쓴 행 \(n)개)")
        } catch {
            bad += 1
            print("  옮겨 담지 못함: \(p): \(error)")
        }
    }
    print("표마다 행 수: " + RecordsDB.tables.map { "\($0) \(db.count($0))" }.joined(separator: " · "))
    db.close()
    exit(bad == 0 ? 0 : 1)
}

// 판정 미리 돌리기(설계 2장, 4장 prerun): 출발 전에 고정 단계가 없고 prerun 이 참인 단위의 판정을 한 번씩 돌려, 일하기 전부터 통과하는 단위를
// 출발 카드 감(빼기·판정 고치기·그대로)으로 prerun.json 에 남긴다. 원래 작업 트리 모드라 지금 자리에서 돈다(출발 가지와 다르면 알림만, 작업 트리를 바꾸지 않음)
func cmdPrerun(_ args: [String]) -> Never {
    let dir = runDir(args.first { !$0.hasPrefix("--") })
    guard let run = readJSON(dir + "/run.json") as? [String: Any] else { fail("run.json 을 읽지 못했습니다") }
    guard run["state"] == nil else { fail("이미 출발한 실행입니다. 판정 미리 돌리기는 출발 전에만 합니다.") }
    let dev = args.contains("--dev") || run["claude_args"] != nil || run["claude_path"] != nil
    let (problems, f, queue) = prepare(dir, dev: dev)
    guard problems.isEmpty else { fail("출발 준비 확인에 고칠 것이 있어 돌리지 않았습니다:\n" + problems.map { "  - " + $0 }.joined(separator: "\n")) }
    let e = Engine(dir)
    e.cfg = f; e.defs = f["units"] as? [[String: Any]] ?? []; e.queue = queue
    e.setupEnv()
    let id = (dir as NSString).lastPathComponent
    var whereNotes: [[String: Any]] = []
    // 따로 작업 트리 모드면 출발 기준(고른 출발 가지)의 임시 분리 작업 트리에서 돌린다(설계 4장 prerun). 끝에 지운다. 명령 글은 바꾸지 않는다(cwd 만)
    var tempWt: (repo: String, path: String)?
    if f["worktree_mode"] as? String == "worktree", let r = (f["repos"] as? [String])?.first, let base = baseBranch(f, r) {
        let wtp = home + "/.claude-board/worktrees/.prerun-" + id
        mkdirPrivate(home + "/.claude-board"); mkdirPrivate(home + "/.claude-board/worktrees")
        try? fm.removeItem(atPath: wtp); _ = gitRun(["worktree", "prune"], cwd: r)
        if gitRun(["worktree", "add", "--detach", wtp, base], cwd: r).code == 0 {
            tempWt = (r, wtp)
            for item in f["worktree_files"] as? [Any] ?? [] {
                let path = (item as? String) ?? ((item as? [String: Any])?["path"] as? String) ?? ""
                guard !path.isEmpty, !path.hasPrefix("/"), !path.contains(".."), fm.fileExists(atPath: r + "/" + path) else { continue }
                try? fm.createDirectory(atPath: ((wtp + "/" + path) as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
                try? fm.copyItem(atPath: r + "/" + path, toPath: wtp + "/" + path)
            }
            e.defs = e.defs.map { var d = $0; if let c = d["cwd"] as? String { d["cwd"] = c == r ? wtp : (c.hasPrefix(r + "/") ? wtp + String(c.dropFirst(r.count)) : c) }; return d }
            whereNotes.append(["repo": r, "base": base, "same": true, "note": "따로 작업 트리(출발 가지 \(base) 기준)의 임시 작업 트리에서 돌렸습니다"])
        } else {
            whereNotes.append(["repo": r, "base": base, "same": false, "note": "임시 작업 트리를 만들지 못해 지금 자리에서 돌렸습니다"])
        }
    } else {
        for r in f["repos"] as? [String] ?? [] {
            let cur = currentBranch(r), base = baseBranch(f, r)
            let same = base != nil && cur == base && headOf(r, "HEAD") == headOf(r, "refs/heads/" + (base ?? ""))
            var w: [String: Any] = ["repo": r, "on": cur ?? NSNull(), "base": base ?? NSNull(), "same": same]
            if !same { w["note"] = "지금 걸린 가지(\(cur ?? "가지 없음"))에서 돌렸습니다. 출발 가지(\(base ?? "?"))와 다르면 결과가 다를 수 있습니다" }
            whereNotes.append(w)
        }
    }
    let out = dir + "/prerun"
    try? fm.removeItem(atPath: out)
    var units: [[String: Any]] = [], passing: [Int] = [], cards: [[String: Any]] = []
    print("판정 미리 돌리기: 실행 \((dir as NSString).lastPathComponent), 단위 \(e.defs.count)개")
    for w in whereNotes where w["note"] != nil { print("  알림: \(repoName(w["repo"] as? String ?? "")): \(w["note"]!)") }
    for (i, d) in e.defs.enumerated() {
        let n = i + 1, t = d["title"] as? String ?? "단위 \(n)"
        var o: [String: Any] = ["n": n, "id": d["id"] ?? "u\(n)", "title": t, "ran": false]
        func say(_ s: String) { print("  \(n). \(t): \(s)") }
        if d["prerun"] as? Bool == false { o["skipped"] = "부작용이 있는 판정이라 미리 돌리지 않습니다(prerun: false)"; say(o["skipped"] as! String); units.append(o); continue }
        if !(d["action"] as? String ?? "").isEmpty { o["skipped"] = "고정 단계가 있는 단위라 미리 돌리지 않습니다"; say(o["skipped"] as! String); units.append(o); continue }
        let ev = out + "/\(n)"
        if let pc = d["precheck"] as? String, !pc.isEmpty {
            let r = e.plainCommand(n, pc, ev: ev + "/precheck", limit: 120)
            if r.code != 0 {
                o["skipped"] = "시험 환경 확인이 실패해(종료 코드 \(r.code)) 판정을 돌리지 않았습니다"; say(o["skipped"] as! String); units.append(o); continue
            }
        }
        let limit = (num(d["check_timeout_min"]) ?? num(f["check_timeout_min"]) ?? 10) * 60
        let r = e.plainCommand(n, d["check"] as? String ?? "false", ev: ev, limit: limit)
        o["ran"] = true; o["exit"] = Int(r.code); o["sec"] = r3(r.sec); o["tail"] = r.tail; o["passed"] = r.code == 0; o["evidence"] = "prerun/\(n)"
        if r.timedOut { o["timed_out"] = true }
        units.append(o)
        if r.code == 0 {
            passing.append(n)
            cards.append(["id": "prerun-\(d["id"] ?? "u\(n)")", "unit": n, "kind": "prerun",
                          "title": "「\(t)」 의 판정이 일하기 전부터 통과합니다",
                          "why": "이미 끝난 일이거나 판정이 약하다는 뜻입니다. 그대로 두면 모델이 일하지 않아도 통과로 기록됩니다.",
                          "options": [["key": "drop", "label": "빼기"], ["key": "fix_check", "label": "판정 고치기"], ["key": "keep", "label": "그대로"]],
                          "recommended": "drop"])
            say("일하기 전부터 통과합니다(\(dur(r.sec))). 출발 카드 감: 빼기(추천) · 판정 고치기 · 그대로")
        } else {
            say("실패(종료 코드 \(r.code)\(r.timedOut ? ", 시간 상한" : ""), \(dur(r.sec))). 일하기 전이라 맞습니다")
        }
    }
    if let tw = tempWt {   // 임시 작업 트리를 지운다
        _ = gitRun(["worktree", "remove", "--force", tw.path], cwd: tw.repo)
        _ = gitRun(["worktree", "prune"], cwd: tw.repo); try? fm.removeItem(atPath: tw.path)
    }
    writeJSON(dir + "/prerun.json", ["at": r3(now()), "where": whereNotes, "units": units, "already_passing": passing, "cards": cards])
    print("결과: \(dir)/prerun.json (일하기 전부터 통과 \(passing.count)개)")
    exit(0)
}

// 끝난 실행의 가지 승격(설계 2장 끝 "승격", 4장 finish). 사람이 명령으로만 부른다 — 엔진이 스스로 부르는 길은 없다.
//   merge --into <가지> [--story <키>]   자율주행 가지(Jira 실행이면 --story 의 스토리 가지)를 <가지>로
//   promote <가지> --into <가지>         기존 가지를 다음 가지로(live → master). 프로젝트 설정의 승격 차례가 있어야 한다
//   discard [--story <키>]               어느 단계도 합치기 전에만 자율주행 가지(또는 그 스토리의 스토리·작업 가지)를 지운다
// 합치기는 받는 가지가 그대로면 앞으로 감기, 앞서 나갔으면 병합 커밋(merge-tree 로 체크아웃 없이), 충돌이면 하지 않는다. 받는 가지가 어느 작업 트리에
// 걸려 있으면 그 작업 트리가 깨끗할 때만 그 자리에서 앞으로 감기(ff-only)로 올리고, 아니면 하지 않는다
// 따로 작업 트리 모드에서 엔진이 만든 작업 트리를 지우고 prune 한다(합친 뒤·버린 뒤). 커밋 안 한 변경이 있으면 force 없이는 지우지 않고 까닭을 돌려준다
func finishWorktreeRemove(_ r: String, _ info: [String: Any], force: Bool) -> String? {
    guard let i = info[r] as? [String: Any], let w = i["work_tree"] as? String, isDir(w) else { return nil }
    let rm = gitRun(["worktree", "remove"] + (force ? ["--force"] : []) + [w], cwd: r)
    guard rm.code == 0 else { return "작업 트리(\(w))를 지우지 못했습니다(커밋하지 않은 변경이 있을 수 있음): \(firstLine(rm.out))" }
    _ = gitRun(["worktree", "prune"], cwd: r)
    let base = (w as NSString).deletingLastPathComponent
    if (try? fm.contentsOfDirectory(atPath: base))?.isEmpty == true { try? fm.removeItem(atPath: base) }
    print("  작업 트리를 지웠습니다: \(w)")
    return nil
}

func cmdFinish(_ args: [String]) -> Never {
    guard args.count >= 2 else { usage() }
    let dir = runDir(args[0])
    var rest = Array(args.dropFirst())
    let op = rest.removeFirst()
    func opt(_ k: String) -> String? {
        guard let i = rest.firstIndex(of: k), i + 1 < rest.count else { return nil }
        let v = rest[i + 1]; rest.removeSubrange(i...(i + 1)); return v
    }
    let into = opt("--into"), storyKey = opt("--story")
    guard var run = readJSON(dir + "/run.json") as? [String: Any] else { fail("run.json 을 읽지 못했습니다") }
    let state = run["state"] as? String ?? ""
    guard ["done", "stopped", "error"].contains(state), run["wrapped"] != nil else {
        fail("실행이 아직 끝나지 않았습니다(\(stateLabel[state] ?? state)). 끝난 뒤에 합치거나 버립니다.")
    }
    var b = run["branches"] as? [String: Any] ?? [:]
    let info = b["repos"] as? [String: Any] ?? [:]
    guard !info.isEmpty else { fail("이 실행에는 엔진이 만든 가지가 없습니다(git 저장소에서 돌지 않았거나 가지 기능 전의 실행).") }
    var promoted = b["promoted"] as? [[String: Any]] ?? []
    // 합칠 쪽: 자율주행 가지(저장소마다) 또는 스토리 가지
    var label = "autopilot", refs: [(repo: String, branch: String)] = []
    if let k = storyKey {
        guard let s = (b["stories"] as? [String: Any])?[k] as? [String: Any], let r = s["repo"] as? String, let sb = s["branch"] as? String else {
            fail("이 실행에 스토리 \(k) 가 없습니다.")
        }
        label = "story:" + k; refs = [(r, sb)]
    } else {
        refs = info.keys.sorted().compactMap { r in ((info[r] as? [String: Any])?["work"] as? String).map { (r, $0) } }
    }
    let steps = (run["project"] as? String).flatMap { projectSettings($0, dev: run["dev"] as? Bool ?? false)?["promotion"] as? [[String: Any]] }
    func done(_ src: String?, _ into: String) -> Bool { promoted.contains { ($0["into"] as? String) == into && (src == nil || $0["source"] as? String == src) } }
    func step(_ from: String, _ into: String) -> [String: Any] {
        guard let steps else { return [:] }
        guard let s = steps.first(where: { $0["from"] as? String == from && $0["into"] as? String == into }) else {
            let list = steps.map { "\(($0["from"] as? String) == "autopilot" ? "자율주행 가지" : ($0["from"] as? String ?? "?")) → \($0["into"] ?? "?")" }
            fail("프로젝트 설정의 승격 차례에 \(from == "autopilot" ? "자율주행 가지" : from) → \(into) 단계가 없습니다(차례: \(list.joined(separator: ", "))).")
        }
        return s
    }
    let id = (dir as NSString).lastPathComponent
    switch op {
    case "merge":
        guard let into, rest.isEmpty else { usage() }
        if let after = step("autopilot", into)["after"] as? String, !done(label, after) {
            fail("\(after) 에 먼저 합쳐야 \(into) 에 합칠 수 있습니다(승격 차례).")
        }
        promoted += mergeRefs(label, refs, into, id: id)
        // 합친 뒤 따로 작업 트리를 지운다(깨끗할 때만, 커밋 안 한 변경이 있으면 지우지 않고 알린다). 가지는 남아 다음 승격에 쓰인다
        for r in Set(refs.map { $0.repo }) { if let note = finishWorktreeRemove(r, info, force: false) { print("  " + note) } }
    case "promote":
        guard let into, rest.count == 1 else { usage() }
        let from = rest[0]
        guard steps != nil else { fail("프로젝트 설정에 승격 차례(promotion)가 없어 기존 가지끼리는 합치지 않습니다. 보드에서 차례를 정한 뒤 다시 하세요.") }
        if let after = step(from, into)["after"] as? String, !done(nil, after) {
            fail("\(after) 에 먼저 합쳐야 \(from) 을 \(into) 에 합칠 수 있습니다(승격 차례).")
        }
        promoted += mergeRefs("branch:" + from, info.keys.sorted().map { ($0, from) }, into, id: id)
    case "discard":
        guard rest.isEmpty, into == nil else { usage() }
        if let p = promoted.first(where: { $0["source"] as? String == label }) { fail("이미 \(p["into"] ?? "?") 에 합친 단계가 있어 버리지 않습니다. 버리기는 합치기 전에만 합니다.") }
        var del = refs
        if let k = storyKey, let r = refs.first?.repo {   // 스토리 단위: 그 스토리의 작업 가지도 함께
            let defs = (readJSON(dir + "/pinned/frozen.json") as? [String: Any])?["units"] as? [[String: Any]] ?? []
            for d in defs where d["story"] as? String == k { if let t = d["branch"] as? String, branchExists(r, t) { del.append((r, t)) } }
        }
        // 엔진 자신의 작업 트리에 걸린 가지는 먼저 작업 트리를 지워(force) 떼어 내면 된다. 다른 작업 트리에 걸린 것만 거부
        var bad: [String] = []
        for (r, x) in del {
            if let wt = checkedOut(r)[x], wt != (info[r] as? [String: Any])?["work_tree"] as? String {
                bad.append("\(x) 가 작업 트리(\(wt))에 걸려 있습니다. 다른 가지로 바꾼 뒤 다시 하세요")
            }
        }
        if !bad.isEmpty { fail("버리지 않았습니다:\n" + bad.map { "  - " + $0 }.joined(separator: "\n")) }
        for r in Set(del.map { $0.repo }) { if let note = finishWorktreeRemove(r, info, force: true) { print("  " + note) } }
        var gone: [[String: Any]] = []
        if !del.contains(where: { branchExists($0.repo, $0.branch) }) { print("  지울 가지가 없습니다(이미 버렸거나 지워짐).") }
        for (r, x) in del where branchExists(r, x) {
            let h = headOf(r, "refs/heads/" + x) ?? ""
            let c = gitRun(["branch", "-D", x], cwd: r)
            print(c.code == 0 ? "  지움: \(repoName(r)) \(x) (\(h.prefix(7)))" : "  지우지 못함: \(repoName(r)) \(x): \(firstLine(c.out))")
            if c.code == 0 { gone.append(["source": label, "repo": r, "branch": x, "head": h, "at": r3(now())]) }
        }
        b["discarded"] = (b["discarded"] as? [Any] ?? []) + gone
    default: usage()
    }
    b["promoted"] = promoted
    run["branches"] = b; run["updated"] = r3(now())
    writeJSON(dir + "/run.json", run)
    // 기록 저장소의 runs.branches 도 새로(실패해도 run.json 은 이미 썼다)
    if let db = try? RecordsDB(recordsPath(dev: run["dev"] as? Bool ?? false)) {
        do { try db.transaction { _ = try db.syncRun(RunSource(dir: dir, origin: "engine", frozen: nil)) } }
        catch { print("  실행 기록 저장소에 쓰지 못했습니다: \(error)") }
        db.close()
    }
    exit(0)
}

// 합치기 하나(저장소마다): 모든 저장소를 먼저 계획하고, 하나라도 충돌·거부면 아무것도 하지 않는다. 한 것을 promoted 줄로 돌려준다
func mergeRefs(_ label: String, _ refs: [(repo: String, branch: String)], _ into: String, id: String) -> [[String: Any]] {
    struct Plan { var repo, src, srcHead, intoHead, how: String; var tree: String?; var wt: String? }
    var plans: [Plan] = [], bad: [String] = []
    for (r, s) in refs {
        guard let sh = headOf(r, "refs/heads/" + s) else { bad.append("\(repoName(r)) 에 \(s) 가지가 없습니다(버렸거나 지워짐)"); continue }
        guard let ih = headOf(r, "refs/heads/" + into) else { print("  건너뜀: \(repoName(r)) 에 \(into) 가지가 없습니다"); continue }
        var p = Plan(repo: r, src: s, srcHead: sh, intoHead: ih, how: "same", tree: nil, wt: checkedOut(r)[into])
        if isAncestor(r, sh, ih) { p.how = "same" }
        else if isAncestor(r, ih, sh) { p.how = "ff" }
        else {
            let m = gitRun(["merge-tree", "--write-tree", "--name-only", "--no-messages", ih, sh], cwd: r)
            let lines = m.out.split(separator: "\n").map(String.init)
            if m.code == 0, let t = lines.first { p.how = "merge"; p.tree = t }
            else if m.code == 1 { bad.append("\(repoName(r)): \(s) 를 \(into) 에 합치면 충돌합니다(\(lines.dropFirst().prefix(5).joined(separator: ", "))). 보드 탭에서 직접 합치세요") }
            else { bad.append("\(repoName(r)): 합칠 tree 를 만들지 못했습니다: \(firstLine(m.out))") }
        }
        if p.how != "same", let wt = p.wt, !(gitOut(["--no-optional-locks", "status", "--porcelain", "--untracked-files=no"], cwd: wt) ?? "x").isEmpty {
            bad.append("\(repoName(r)): \(into) 가 걸린 작업 트리(\(wt))에 커밋하지 않은 변경이 있어 합치지 않습니다")
        }
        plans.append(p)
    }
    if !bad.isEmpty { fail("합치지 않았습니다(아무것도 바꾸지 않음):\n" + bad.map { "  - " + $0 }.joined(separator: "\n")) }
    var out: [[String: Any]] = []
    for p in plans {
        var head = p.srcHead
        if p.how == "same" { print("  그대로: \(repoName(p.repo)) \(into) 에 \(p.src) 가 이미 들어 있습니다") }
        if p.how == "merge", let t = p.tree {
            let c = gitRun(["commit-tree", t, "-p", p.intoHead, "-p", p.srcHead, "-m", "Merge branch '\(p.src)' into \(into)\n\n자율주행 \(id) 승격(보드 finish)"], cwd: p.repo, commit: true)
            guard c.code == 0 else { print("  하지 못함: \(repoName(p.repo)) 병합 커밋: \(firstLine(c.out))"); continue }
            head = c.out
        }
        if p.how != "same" {
            let up = p.wt.map { gitRun(["merge", "--ff-only", "-q", head], cwd: $0) }
                ?? gitRun(["update-ref", "-m", "autopilot \(id): finish \(p.src) into \(into)", "refs/heads/" + into, head, p.intoHead], cwd: p.repo)
            guard up.code == 0 else { print("  하지 못함: \(repoName(p.repo)) \(into) 올리기: \(firstLine(up.out))"); continue }
            print("  합침: \(repoName(p.repo)) \(p.src) → \(into) (\(p.how == "ff" ? "앞으로 감기" : "병합 커밋") \(p.intoHead.prefix(7)) → \(head.prefix(7))\(p.wt == nil ? "" : ", 작업 트리도 함께"))")
        }
        out.append(["source": label, "repo": p.repo, "branch": p.src, "into": into, "how": p.how, "from": p.intoHead, "to": head, "at": r3(now())])
    }
    return out
}

let argv = Array(CommandLine.arguments.dropFirst())
switch argv.first {
case "start": cmdStart(Array(argv.dropFirst()))
case "run": cmdRun(Array(argv.dropFirst()))
case "control": cmdControl(Array(argv.dropFirst()))
case "status": cmdStatus(Array(argv.dropFirst()))
case "records": cmdRecords(Array(argv.dropFirst()))
case "prerun": cmdPrerun(Array(argv.dropFirst()))
case "finish": cmdFinish(Array(argv.dropFirst()))
default: usage()
}
