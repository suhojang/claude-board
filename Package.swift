// swift-tools-version: 6.2
// 보드 앱(Sources/ClaudeBoard/)과 자율주행 엔진(Sources/ClaudeAutopilot/)을 SwiftPM 으로 빌드한다 — 보드는 내장 터미널 엔진(libghostty-spm)을 쓰기 위해서,
// 엔진은 규칙 라이브러리 AutopilotRules 를 import 하기 위해서다(swiftc 단독 빌드로는 라이브러리를 import 할 수 없다).
// 보드는 한 파일(ClaudeBoard.swift)이던 것을 tools/split-board.py 로 MARK 절마다 나눴다 — 최상위 실행문은 main.swift 에만 둔다.
// 알림 패널(ClaudeNotifier.swift)은 지금처럼 swiftc 로 빌드한다. 모든 swift build/test/package 명령에 --disable-keychain 을 붙인다
// (빼면 바이너리 자산을 내려받을 때 키체인 조회에서 멈춘다)
//
// 보드 규칙(세션·카드 모델과 묶음·정렬 같은 순수 함수)은 라이브러리 BoardRules(Sources/BoardRules)로 떼어 Swift Testing 으로 시험한다
// (Tests/BoardRulesTests). 자율주행 판정 규칙은 라이브러리 AutopilotRules(Sources/AutopilotRules)로 떼어 보드·엔진이 함께 import 하고
// 같은 꼴로 시험한다(Tests/AutopilotRulesTests). Command Line Tools 만 있으면 SwiftPM 이 Testing.framework 자리를 못 찾으므로
// `swift test … --toolset clt-testing.json` 으로 돌린다. 같은 -F·-rpath 를 여기 unsafeFlags 로 넣으면 시험이 0개 돌고 성공으로 끝나니
// 넣지 않는다. Xcode 가 있는 Mac 에서는 --toolset 을 뺀다
import PackageDescription

let package = Package(
    name: "ClaudeBoard",
    platforms: [.macOS(.v14)],
    dependencies: [
        // 2.2.2026101502 아래는 LGPL 인 libintl 이 정적으로 들어 있다 — 버전을 올릴 때도 exact 로 고정한다
        .package(url: "https://github.com/Lakr233/libghostty-spm.git", exact: "2.2.2026101502"),
    ],
    targets: [
        .executableTarget(
            name: "ClaudeBoard",
            dependencies: [
                "BoardRules",
                "AutopilotRules",
                .product(name: "GhosttyTerminal", package: "libghostty-spm"),
                .product(name: "GhosttyTheme", package: "libghostty-spm"),   // 사용자 Ghostty 설정의 theme 이름을 색으로
            ],
            path: "Sources/ClaudeBoard",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        // 자율주행 엔진 claude-autopilot — 보드 앱 밖에서 launchd 로 돈다. 실행 파일 이름은 ClaudeAutopilot 이고 build-board.sh·make-dmg.sh 가
        // 앱 Contents/MacOS/claude-autopilot 으로 싣는다. 언어 모드는 swiftc 단독 빌드 때의 기본(5)과 같게 둔다
        .executableTarget(
            name: "ClaudeAutopilot",
            dependencies: ["AutopilotRules"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        // Foundation 만 쓰는 순수 규칙 — 앱·엔진 없이 시험한다
        .target(name: "BoardRules", swiftSettings: [.swiftLanguageMode(.v5)]),
        .testTarget(name: "BoardRulesTests", dependencies: ["BoardRules"]),
        // 자율주행 판정 규칙 — 보드 자율주행 화면과 엔진이 함께 쓴다. 자율주행 규칙은 맨 위 rules-table/(Windows 판과 함께 쓰는 표)에 넣지 않는다
        .target(name: "AutopilotRules", swiftSettings: [.swiftLanguageMode(.v5)]),
        .testTarget(name: "AutopilotRulesTests", dependencies: ["AutopilotRules"]),
    ]
)
