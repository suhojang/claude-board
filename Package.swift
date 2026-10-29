// swift-tools-version: 6.2
// 보드 앱(Sources/ClaudeBoard/)만 SwiftPM 으로 빌드한다 — 내장 터미널 엔진(libghostty-spm)을 쓰기 위해서다.
// 보드는 한 파일(ClaudeBoard.swift)이던 것을 tools/split-board.py 로 MARK 절마다 나눴다 — 최상위 실행문은 main.swift 에만 둔다.
// 알림 패널 등 다른 실행 파일은 지금처럼 swiftc 로 빌드한다. 모든 swift build/test/package 명령에 --disable-keychain 을 붙인다
// (빼면 바이너리 자산을 내려받을 때 키체인 조회에서 멈춘다)
//
// 보드 규칙(세션·카드 모델과 묶음·정렬 같은 순수 함수)은 라이브러리 BoardRules(Sources/BoardRules)로 떼어 Swift Testing 으로 시험한다
// (Tests/BoardRulesTests). Command Line Tools 만 있으면 SwiftPM 이 Testing.framework 자리를 못 찾으므로
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
                .product(name: "GhosttyTerminal", package: "libghostty-spm"),
                .product(name: "GhosttyTheme", package: "libghostty-spm"),   // 사용자 Ghostty 설정의 theme 이름을 색으로
            ],
            path: "Sources/ClaudeBoard",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        // Foundation 만 쓰는 순수 규칙 — 앱·엔진 없이 시험한다
        .target(name: "BoardRules", swiftSettings: [.swiftLanguageMode(.v5)]),
        .testTarget(name: "BoardRulesTests", dependencies: ["BoardRules"]),
    ]
)
