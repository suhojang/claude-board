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

// 다른 파일의 전역은 처음 쓸 때 초기화된다. 초기화식에 부작용이 있어 시작할 때 바로 초기화해야 하는 전역은 여기서 먼저 읽는다(tools/split-board.py 의 EAGER 표)
_ = unfocusedSplitOpacity
_ = mdRenderDir

// 시험 빌드(build-board.sh --dev, 번들 id com.claude.board.dev)는 설치된 보드와 함께 떠야 하므로 잠금 파일·분산 알림 이름을 따로 쓴다
let instanceName = Bundle.main.bundleIdentifier == "com.claude.board.dev" ? "claude-board-dev" : "claude-board"

// 명령줄 동작: claude-board --action pause|resume|stop|reopen|focus <세션 id>, --action quit -(설치 스크립트가 청하는 정상 종료)
// 떠 있는 보드에 알림으로 넘긴다 — Ghostty 제어 권한을 가진 그 보드가 실행한다
if let i = CommandLine.arguments.firstIndex(of: "--action"), i + 2 < CommandLine.arguments.count {
    DistributedNotificationCenter.default().postNotificationName(
        .init("\(instanceName).action"), object: nil,
        userInfo: ["action": CommandLine.arguments[i + 1], "id": CommandLine.arguments[i + 2]], deliverImmediately: true)
    exit(0)
}

@MainActor func performAction(_ action: String, _ id: String) {
    // 보드 밖에서 업데이트하는 install.sh 가 청하는 정상 종료 — 세션 id 는 보지 않는다. 이 동작을 모르는 옛 보드는 아래 세션 찾기에서 그냥 돌아간다
    if action == "quit" { (NSApp.delegate as? AppDelegate)?.requestQuit(); return }
    // 떠 있는 보드의 Store 를 쓴다 — 새로 만들면 매번 1초 타이머와 알림 관찰자가 남아 계속 돈다
    guard let s = (NSApp.delegate as? AppDelegate)?.store.groups.flatMap(\.sessions).first(where: { $0.id == id }) else { return }
    switch action {
    case "pause": pauseSession(s)
    case "resume": resumePaused(s)
    case "stop": stopSession(s, confirm: false)
    case "reopen": reopenEnded(s)
    case "focus": openSession(s, double: true)   // 보드 탭 세션은 펼치고 그 탭으로, 다른 터미널은 그 세션의 터미널 창으로
    default: break
    }
}

// 이미 떠 있으면 새로 띄우지 않는다
let lockPath = NSTemporaryDirectory() + "\(instanceName).lock"
let lockFd = open(lockPath, O_CREAT | O_RDWR, 0o600)
if lockFd >= 0, flock(lockFd, LOCK_EX | LOCK_NB) != 0 {
    // 이미 떠 있는 창을 앞으로 불러오고 끝낸다
    DistributedNotificationCenter.default().postNotificationName(.init("\(instanceName).show"), object: nil,
                                                                 userInfo: nil, deliverImmediately: true)
    exit(0)
}

// 터미널 엔진의 리소스 번들(terminfo·셸 통합)은 .app 의 Contents/Resources 에 있다 — .app 맨 위는 서명이 거부한다.
// SwiftPM 이 만든 Bundle.module 은 .app 맨 위와 빌드한 컴퓨터의 빌드 경로만 찾으므로(못 찾으면 첫 터미널에서 죽는다,
// 빌드 경로가 남은 개발 Mac 에서는 멀쩡해 보인다), Bundle.module 이 한 번 정해지는 동안만 main 번들의 bundleURL 을
// Contents/Resources 로 돌려준다. 한 번 정해지면 바뀌지 않으므로 첫 TerminalController 전에 한 번 부른다
func resolveResourceBundleInApp() {
    guard Bundle.main.bundlePath.hasSuffix(".app"), let res = Bundle.main.resourceURL else { return }
    let sel = #selector(getter: Bundle.bundleURL)
    guard let m = class_getInstanceMethod(Bundle.self, sel) else { return }
    typealias Fn = @convention(c) (AnyObject, Selector) -> NSURL
    let original = method_getImplementation(m)
    let origFn = unsafeBitCast(original, to: Fn.self)
    let main = Bundle.main
    let block: @convention(block) (AnyObject) -> NSURL = { obj in obj === main ? res as NSURL : origFn(obj, sel) }
    method_setImplementation(m, imp_implementationWithBlock(block))
    _ = GhosttyRuntimeResources.directoryURL   // 여기서 Bundle.module 이 한 번 정해진다
    method_setImplementation(m, original)
}
resolveResourceBundleInApp()

// Claude Code 세션 안에서 보드를 띄우면(open -a·업데이트 단추) 그 세션의 표지 변수(CLAUDECODE·CLAUDE_CODE_CHILD_SESSION·
// CLAUDE_CODE_SESSION_ID 등)가 보드 환경에 그대로 들어온다(open 은 띄운 쪽 환경을 넘긴다, 실측). 보드 탭의 claude·셸이 그것을
// 물려받으면 claude 가 자식 세션으로 여겨 대화 기록을 저장하지 않는다 — 시작할 때 지운다.
// 사용자가 셸 설정에 둔 CLAUDE_* 값은 탭의 셸이 뜰 때 다시 들어온다
func inheritedSessionVariable(_ key: String) -> Bool {
    key.hasPrefix("CLAUDE") || key == "AI_AGENT" || key.hasPrefix("CODEX_COMPANION_")
}
for key in ProcessInfo.processInfo.environment.keys where inheritedSessionVariable(key) { unsetenv(key) }

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = AppDelegate()
app.delegate = delegate
app.run()
