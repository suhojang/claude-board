// swift-tools-version: 6.2
// 보드 앱(ClaudeBoard.swift)만 SwiftPM 으로 빌드한다 — 내장 터미널 엔진(libghostty-spm)을 쓰기 위해서다.
// 알림 패널 등 다른 실행 파일은 지금처럼 swiftc 로 빌드한다. 모든 swift build/package 명령에 --disable-keychain 을 붙인다
// (빼면 바이너리 자산을 내려받을 때 키체인 조회에서 멈춘다)
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
                .product(name: "GhosttyTerminal", package: "libghostty-spm"),
                .product(name: "GhosttyTheme", package: "libghostty-spm"),   // 사용자 Ghostty 설정의 theme 이름을 색으로
            ],
            path: ".",
            exclude: [
                "windows", "packaging", "plugins", "docs", "dist",
                "ClaudeNotifier.swift", "ClaudeAutopilot.swift", "ClaudeStatus.swift", "AppIcon.swift", "AppIcon.icns",
                "notify.py", "state.py", "hooks.py", "board-tui.py", "build-board.sh",
                "README.md", "CHANGELOG.md", "CLAUDE.md", "LICENSE",
            ],
            sources: ["ClaudeBoard.swift"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
