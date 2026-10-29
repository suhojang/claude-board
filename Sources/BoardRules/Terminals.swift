// 다른 터미널 조작 규칙 — 세션이 어느 터미널에 있는지(조상 앱 번들 id), 조작 길 고르기(보드 탭·Ghostty·iTerm2·Terminal·그 밖),
// AppleScript 글자 만들기, Ghostty 터미널 찾기(표지 제목)·키 보내기, iTerm2·Terminal 의 tty 포커스
// (ClaudeBoard.swift 의 ownerApp·ttyExists·focusByTty·canDrive·canDriveSession·driveSession·ghosttyTerminalId·ghosttySend·focusGhostty·focusSession
// 에 있던 판단을 옮김, 동작·포트 호출 차례·AppleScript 글자는 바이트까지 그대로). 바깥(다른 터미널 앱·프로세스·tty 제목·보드 탭)은 포트(Ports.swift)로만 부른다.
// 이슈 #9 3차에 고친 것: 세션을 띄운 앱 기억을 보이는 세션으로 정리(prune), Ghostty 포커스도 빈 터미널 id 를 못 찾은 것으로,
// AppleScript 글자에 넣는 tty·터미널 id 도 막는다(보통 값은 글자가 바이트까지 그대로), tty 이름 꼴이 아닌 tty 로는 찾지 않는다(isTtyName).
// 큐(ghosttyQueue·백그라운드·메인)와 타이머는 앱이 쥔다 — 여기 함수는 부른 큐에서 그대로 돈다
import Foundation

/// 지원하는 다른 터미널 앱의 번들 id — Ghostty 는 AppleScript 에 tty 가 없어 표지 제목으로 찾고, iTerm2·Terminal 은 tty 로 바로 찾는다
public enum TerminalBundle {
    public static let ghostty = "com.mitchellh.ghostty"
    public static let iterm = "com.googlecode.iterm2"
    public static let terminal = "com.apple.Terminal"
}

/// 세션이 도는 곳 — 칩·클릭 흐름·카드 구역이 쓴다. 정하는 규칙은 sessionPlace·pairTabs(TabPairing.swift)
public enum SessionPlace: Equatable, Sendable {
    case tab(UUID, headless: Bool)   // 보드 탭의 화면(headless = 그 화면 안에서 시작된 claude -p)
    case terminal                    // 다른 터미널 앱
    case background                  // claude -p·--bg 등 터미널 없이 도는 세션
    case ended
}

/// 세션을 띄운 다른 터미널 앱의 종류 — 조상 앱 번들 id 로 가른다
public enum TerminalKind: Equatable, Sendable {
    case ghostty
    case iterm
    case terminal
    case other(String)   // VS Code·Orca 등 — 앱만 앞으로 가져온다
    case unknown         // 조상으로도 tty 로도 못 찾음
}

public func terminalKind(_ bundleId: String?) -> TerminalKind {
    guard let bundleId else { return .unknown }
    switch bundleId {
    case TerminalBundle.ghostty: return .ghostty
    case TerminalBundle.iterm: return .iterm
    case TerminalBundle.terminal: return .terminal
    default: return .other(bundleId)
    }
}

/// 세션 터미널에 키·글자를 넣는 길 — 보드 탭이 먼저다(그 화면 앞에서 대화형 claude 가 돌 때). 다음이 다른 터미널의 Ghostty 세션
public enum DriveRoute: Equatable, Sendable {
    case tab(UUID)   // 보드 탭의 그 화면에 직접
    case ghostty     // Ghostty 세션 — AppleScript(앱이 ghosttyQueue 에서 ghosttyDrive)
    case unavailable // 넣을 수 없다(보드 탭의 claude -p·화면 앞이 아님, Ghostty 밖 터미널, 백그라운드, 끝남)
}

/// 두 번 눌렀을 때 세션의 터미널 창까지 가는 길(focusSession) — 앱이 백그라운드 큐에서 고른다
public enum FocusRoute: Equatable, Sendable {
    case tty(String)   // iTerm2·Terminal — tty 로 탭을 찾아 앞으로(못 찾으면 그 앱만 앞으로)
    case app(String)   // 그 밖의 앱 — 앱만 앞으로
    case ghostty       // Ghostty 이거나 못 찾음 — 표지 제목으로 찾아 포커스(ghosttyQueue, 못 찾으면 기본 터미널만)
}

// MARK: - tty 이름

/// /dev/ 아래 터미널 이름으로 쓸 수 있는 tty 인가 — 비지 않은 ASCII 영숫자만. 세션 기록의 tty 는 state.py 가 `ps -o tty=` 로 얻은 이름이고
/// (ttys003, 옛 BSD pty 의 ttyp1, 콘솔 로그인의 console. 터미널이 없는 "??" 는 null 로 남긴다), 보드 탭의 tty 는 엔진이 준 /dev/ttys012 에서
/// /dev/ 를 뗀 것이라 모두 이 꼴이다. 세션 파일에서 온 바깥 값이므로 /dev/<tty> 경로를 만드는 곳(창 제목 쓰기, iTerm2·Terminal 에서 tty 로
/// 찾기)은 이것을 먼저 거친다 — "../" 같은 값이면 /dev 밖의 파일에 창 제목(OSC 2)을 쓸 수 있다. 아니면 쓰지 않고 없는 것으로 본다
public func isTtyName(_ tty: String) -> Bool {
    !tty.isEmpty && tty.unicodeScalars.allSatisfy { ("a"..."z").contains($0) || ("A"..."Z").contains($0) || ("0"..."9").contains($0) }
}

// MARK: - AppleScript 글자 (보통 값은 바이트까지 예전 그대로 — 시험이 고정해 둔다)

/// AppleScript 글자 안에 넣을 값 — 역슬래시·큰따옴표만 막는다. 줄바꿈은 그대로 넣는다(지금 동작)
public func appleScriptEscaped(_ t: String) -> String {
    t.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
}

/// Ghostty 터미널 목록 — 줄마다 "id<탭 문자>제목". tell 안에서 tab 은 Ghostty 의 탭 객체 이름으로 읽혀 글자 "tab" 이 들어간다 —
/// 탭 문자는 character id 9 로 쓴다
public let ghosttyTerminalsScript = """
    tell application "Ghostty"
      set out to ""
      repeat with t in terminals
        set out to out & (id of t) & (character id 9) & (name of t) & linefeed
      end repeat
      return out
    end tell
    """

/// 그 표지 제목인 Ghostty 터미널의 id(없으면 빈 글자). 표지는 넣는 쪽이 만든 글자(titleMarker)라 막지 않는다
public func ghosttyFindScript(marker: String) -> String {
    """
    tell application "Ghostty"
      repeat with t in terminals
        if name of t is "\(marker)" then return id of t
      end repeat
      return ""
    end tell
    """
}

/// 그 표지 제목인 Ghostty 터미널로 포커스하고 앱을 앞으로 — 그 id(없으면 빈 글자)
public func ghosttyFocusScript(marker: String) -> String {
    """
    tell application "Ghostty"
      repeat with t in terminals
        if name of t is "\(marker)" then
          focus t
          activate
          return id of t
        end if
      end repeat
      return ""
    end tell
    """
}

/// Ghostty 터미널 id 에 단계를 보낸다 — 맞는 터미널이 있으면 "ok". key·text 값은 appleScriptEscaped, wait(과 그 밖의 종류)은 값 그대로 delay.
/// 터미널 id 는 바깥(Ghostty)이 준 글자라 같은 막기를 거친다(UUID 같은 보통 id 는 글자가 그대로다)
public func ghosttySendScript(terminal id: String, steps: TerminalSteps) -> String {
    let body = steps.map { kind, v in
        switch kind {
        case "key": return "send key \"\(appleScriptEscaped(v))\" to t"
        case "text": return "input text \"\(appleScriptEscaped(v))\" to t"
        default: return "delay \(v)"
        }
    }.joined(separator: "\n      ")
    return """
    tell application "Ghostty"
      repeat with t in terminals
        if (id of t as text) is "\(appleScriptEscaped(id))" then
          \(body)
          return "ok"
        end if
      end repeat
      return ""
    end tell
    """
}

/// tty 로 iTerm2(그 밖의 앱 id 는 Terminal)의 탭을 찾아 앞으로 가져온다 — 찾으면 "ok". tty 는 세션 기록(state.py 가 쓴 파일)에서 온 바깥 값이라
/// 같은 막기를 거친다(ttys003 같은 보통 이름은 글자가 그대로다)
public func ttyFocusScript(app bundleId: String, tty: String) -> String {
    let dev = appleScriptEscaped("/dev/" + tty)
    if bundleId == TerminalBundle.iterm {
        return """
        tell application id "\(TerminalBundle.iterm)"
          repeat with w in windows
            repeat with t in tabs of w
              repeat with s in sessions of t
                if tty of s is "\(dev)" then
                  select w
                  tell t to select
                  tell s to select
                  activate
                  return "ok"
                end if
              end repeat
            end repeat
          end repeat
          return ""
        end tell
        """
    }
    return """
    tell application id "\(TerminalBundle.terminal)"
      repeat with w in windows
        repeat with t in tabs of w
          if tty of t is "\(dev)" then
            set selected tab of w to t
            set index of w to 1
            activate
            return "ok"
          end if
        end repeat
      end repeat
      return ""
    end tell
    """
}

/// iTerm2(그 밖의 앱 id 는 Terminal)에서 그 tty 인 세션(탭) 수를 글자로. tty 는 ttyFocusScript 처럼 막는다
public func ttyCountScript(app bundleId: String, tty: String) -> String {
    let dev = appleScriptEscaped("/dev/" + tty)
    return bundleId == TerminalBundle.iterm
        ? "tell application id \"\(TerminalBundle.iterm)\" to return (count of (every session of every tab of every window whose tty is \"\(dev)\")) as text"
        : "tell application id \"\(TerminalBundle.terminal)\" to return (count of (every tab of every window whose tty is \"\(dev)\")) as text"
}

/// Ghostty 터미널 목록 글자 → id: 제목. 탭 문자가 없는 줄은 뺀다(같은 id 는 뒤의 것)
public func ghosttyTitles(_ list: String?) -> [String: String] {
    var titles: [String: String] = [:]
    for line in (list ?? "").split(separator: "\n") {
        let parts = line.split(separator: "\t", maxSplits: 1, omittingEmptySubsequences: false)
        if parts.count == 2 { titles[String(parts[0])] = String(parts[1]) }
    }
    return titles
}

/// 터미널을 찾을 표지 제목 — "<앞말>-<UUID 앞 8글자>"(키 보내기 CBFIND, 포커스 CBFOCUS)
public func titleMarker(_ prefix: String, _ uuid: UUID) -> String { "\(prefix)-\(uuid.uuidString.prefix(8))" }

/// Ghostty 에서 표지 제목을 찾는 횟수와 간격 — 쓴 제목을 Ghostty 가 읽을 때까지(최대 0.5초)
public enum GhosttyFind {
    public static let tries = 10
    public static let interval: TimeInterval = 0.05
}

// MARK: - 유스케이스

/// 다른 터미널 조작 — 세션을 띄운 앱 찾기(세션마다 기억), 조작 길 고르기, Ghostty·iTerm2·Terminal 조작.
/// 기억은 잠금으로 지킨다 — 메인(canDrive)·백그라운드(focusSession·activateOwner) 큐에서 함께 부른다.
/// 보드 탭 포트는 driveRoute 만 부른다(메인에서)
public final class TerminalControl: @unchecked Sendable {
    private let apps: TerminalApps
    private let processes: ProcessControl
    private let titles: TerminalTitles
    private let tabs: BoardTabs
    private var owners: [String: String] = [:]   // 세션 id → 찾은 앱 번들 id(찾은 것만). 보드 목록에서 빠진 세션 몫은 prune 이 지운다
    // ghosttyQueue 와 따로 잠근다 — 잠금은 기억을 읽고 쓸 때만 쥐고 포트 호출(AppleScript 포함)은 잠금 밖이다.
    // ghosttyQueue 안(중단 등)에서 owner 를 불러도 같은 큐에 sync 를 걸어 교착하지 않게
    private let lock = NSLock()

    public init(apps: TerminalApps, processes: ProcessControl, titles: TerminalTitles, tabs: BoardTabs) {
        self.apps = apps; self.processes = processes; self.titles = titles; self.tabs = tabs
    }

    /// 세션을 띄운 터미널 앱 — 조상으로 못 찾으면(pid 0 이면 조상은 보지 않는다) 떠 있는 iTerm2·Terminal 차례로 tty 를 찾는다.
    /// 찾은 것만 세션마다 기억한다(못 찾으면 다음에 다시 찾는다)
    public func owner(_ s: Session) -> String? {
        if let c = lock.withLock({ owners[s.id] }) { return c }
        var found = s.pid > 0 ? processes.ancestorBundleID(s.pid) : nil
        if found == nil, !s.tty.isEmpty {
            found = [TerminalBundle.iterm, TerminalBundle.terminal].first { apps.isRunning($0) && ttyExists($0, s.tty) }
        }
        if let f = found { lock.withLock { owners[s.id] = f } }
        return found
    }

    /// 기억을 남길 세션 id 몫만 남긴다 — 앱이 세션 판정(scanSessions) 뒤 목록에 남은 세션 id 로 부른다(ContextUsage.prune 과 같은 자리).
    /// 지우지 않으면 보드를 오래 띄워 둘 때 끝나 지워진 세션 몫이 쌓인다. 지운 세션이 다시 보이면 다음에 다시 찾는다
    public func prune(keep: Set<String>) {
        lock.withLock { owners = owners.filter { keep.contains($0.key) } }
    }

    /// 그 앱에 그 tty 인 세션(탭)이 있는가 — 결과가 숫자가 아니면 없다. tty 이름 꼴(isTtyName)이 아니면 묻지 않고 없다
    public func ttyExists(_ bundleId: String, _ tty: String) -> Bool {
        guard isTtyName(tty) else { return false }
        return (Int(apps.run(ttyCountScript(app: bundleId, tty: tty)) ?? "") ?? 0) > 0
    }

    /// tty 로 iTerm2·Terminal 의 탭을 찾아 앞으로 가져온다 — 찾으면 true. tty 이름 꼴(isTtyName)이 아니면 찾지 않고 false
    public func focusByTty(_ bundleId: String, _ tty: String) -> Bool {
        guard isTtyName(tty) else { return false }
        return apps.run(ttyFocusScript(app: bundleId, tty: tty)) == "ok"
    }

    /// Ghostty 에서 도는 터미널 세션인가 — AppleScript 로 키를 보낼 수 있다. 앞 조건이 아니면 앱을 찾지 않는다
    public func canDrive(_ s: Session) -> Bool {
        !s.headless && !s.tty.isEmpty && s.state != "ended" && owner(s) == TerminalBundle.ghostty
    }

    /// 세션 터미널에 키를 넣는 길(메인에서) — 보드 탭의 대화형 claude 가 그 화면 앞에 있으면 그 화면, 다른 터미널의 Ghostty 세션이면 Ghostty.
    /// 보드 탭 세션은 앞에 없어도 Ghostty 길로 가지 않는다(앱을 찾지 않는다). canDriveSession 은 이것이 unavailable 이 아닌가다
    public func driveRoute(_ s: Session) -> DriveRoute {
        let place = tabs.place(s)
        if case .tab(let id, false) = place, tabs.isForeground(id, pid: s.pid) { return .tab(id) }
        guard place == .terminal, canDrive(s) else { return .unavailable }
        return .ghostty
    }

    /// 두 번 눌러 터미널 창까지 갈 세션인가 — 아니면(백그라운드·tty 없음·끝남) 앱이 기본 터미널만 띄운다(메인에서 바로)
    public func canFocus(_ s: Session) -> Bool {
        !s.headless && !s.tty.isEmpty && s.state != "ended"
    }

    /// 두 번 눌렀을 때 갈 길(백그라운드 큐에서) — 세션을 띄운 앱으로 가른다. 못 찾으면 Ghostty 길(지금 동작)
    public func focusRoute(_ s: Session) -> FocusRoute {
        let app = owner(s)
        switch terminalKind(app) {
        case .iterm: return .tty(TerminalBundle.iterm)
        case .terminal: return .tty(TerminalBundle.terminal)
        case .other(let id): return .app(id)
        case .ghostty, .unknown: return .ghostty
        }
    }

    /// 한 번 눌렀을 때 앞으로 가져올 앱(백그라운드 큐에서) — 백그라운드·끝난 세션은 찾지 않고 nil. nil 이면 앱이 기본 터미널을 띄운다
    public func ownerToActivate(_ s: Session) -> String? {
        s.headless || s.state == "ended" ? nil : owner(s)
    }

    /// 세션이 도는 Ghostty 터미널의 id(ghosttyQueue 에서) — 그 tty 에 잠깐 표지 제목을 써서 찾고 원래 제목으로 되돌린다.
    /// 찾았어도 표지를 쓰기 전 목록에 없던 터미널이면 못 찾은 것으로 본다. 못 찾으면 표지를 폴더 이름으로 바꾸고 nil
    public func ghosttyTerminalId(_ s: Session, marker: String) -> String? {
        let before = ghosttyTitles(apps.run(ghosttyTerminalsScript))
        titles.write(tty: s.tty, title: marker)
        var found: String?
        for _ in 0..<GhosttyFind.tries {
            apps.wait(GhosttyFind.interval)
            found = apps.run(ghosttyFindScript(marker: marker))
            if let f = found, !f.isEmpty { break }
        }
        if let f = found, !f.isEmpty, let old = before[f] {
            titles.write(tty: s.tty, title: old)
            return f
        }
        titles.write(tty: s.tty, title: (s.cwd as NSString).lastPathComponent)
        return nil
    }

    /// Ghostty 터미널에 단계를 보낸다 — 그 터미널이 있으면 true
    public func ghosttySend(_ id: String, _ steps: TerminalSteps) -> Bool {
        apps.run(ghosttySendScript(terminal: id, steps: steps)) == "ok"
    }

    /// Ghostty 세션에 단계를 넣는다(ghosttyQueue 에서) — 터미널을 찾아 보낸다. 못 찾으면 보내지 않고 false
    public func ghosttyDrive(_ s: Session, _ steps: TerminalSteps, marker: String) -> Bool {
        ghosttyTerminalId(s, marker: marker).map { ghosttySend($0, steps) } ?? false
    }

    /// 표지 제목으로 Ghostty 터미널을 찾아 포커스한다(ghosttyQueue 에서). 찾아 원래 제목으로 되돌렸으면 true.
    /// 아니면 표지를 폴더 이름으로 바꾸고 false — Ghostty 밖(다른 터미널)에서 도는 세션이라 앱이 기본 터미널만 앞으로 가져온다.
    /// ghosttyTerminalId 와 같이 찾은 글자가 비었으면 못 찾은 것이다(목록에 id 가 빈 줄이 있어도 그 제목으로 되돌리지 않는다)
    public func focusGhostty(_ s: Session, marker: String) -> Bool {
        // 표지를 쓰기 전 제목을 기억해 둔다 — 터미널 id 와 제목을 줄 단위로 받는다
        let before = ghosttyTitles(apps.run(ghosttyTerminalsScript))
        titles.write(tty: s.tty, title: marker)
        var found: String?
        for _ in 0..<GhosttyFind.tries {
            apps.wait(GhosttyFind.interval)
            found = apps.run(ghosttyFocusScript(marker: marker))
            if let f = found, !f.isEmpty { break }
        }
        if let f = found, !f.isEmpty, let old = before[f] {
            titles.write(tty: s.tty, title: old)
            return true
        }
        titles.write(tty: s.tty, title: (s.cwd as NSString).lastPathComponent)
        return false
    }
}
