#!/bin/sh
# Claude Board.app 을 빌드해 /Applications 에 설치한다 (배포판 pkg 와 같은 위치)
# 알림 패널·hook 스크립트는 hook 이 실행하는 ~/.claude/notifier 에 설치한다
# --dev: 시험 빌드 — 설치된 보드·hook 은 건드리지 않고 <저장소>/dist/dev/Claude Board Dev.app 만 만든다.
#        번들 id 가 com.claude.board.dev 라 잠금 파일·분산 알림 이름이 달라 설치된 보드와 함께 띄울 수 있다
set -e
N="$(cd "$(dirname "$0")" && pwd)"
APP="/Applications/Claude Board.app"
NAME="Claude Board"
BUNDLE_ID="com.claude.board"
if [ "$1" = "--dev" ]; then
  DEV=1
  APP="$N/dist/dev/Claude Board Dev.app"
  NAME="Claude Board Dev"
  BUNDLE_ID="com.claude.board.dev"
fi
SIGN_ID="${CLAUDE_BOARD_SIGN_ID:-Claude Board Signing}"
# pkg 로 설치한 앱은 root 소유라 그대로는 덮어쓸 수 없다 — 처음 한 번 관리자 비밀번호 창을 띄워 소유자를 나로 바꾼다
if [ -e "$APP" ] && [ ! -w "$APP/Contents" ]; then
  osascript -e "do shell script \"chown -R $(id -un):staff '$APP'\" with administrator privileges"
fi
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# 보드는 터미널 엔진(libghostty-spm) 때문에, 자율주행 엔진은 규칙 라이브러리(AutopilotRules) 때문에 SwiftPM 으로 빌드한다(한 번에 둘 다) —
# 첫 빌드는 엔진(약 77 MB)을 내려받아 1분 남짓 걸린다.
# --disable-keychain 이 없으면 내려받기에서 키체인 조회로 멈춘다. 배포 빌드(make-dmg.sh)와 같은 .build 를 쓰므로 --triple 을 늘 적는다
spm() { swift build -c release --disable-keychain --package-path "$N" --triple "$(uname -m)-apple-macosx14.0" "$@"; }
spm
BIN="$(spm --show-bin-path)"
# 떠 있는 보드가 있어도 깨지지 않게 새 파일로 복사해 바꿔 끼운다
cp "$BIN/ClaudeBoard" "$APP/Contents/MacOS/ClaudeBoard.new"
mv "$APP/Contents/MacOS/ClaudeBoard.new" "$APP/Contents/MacOS/ClaudeBoard"
# 자율주행 엔진 — 보드 앱 밖에서 launchd 로 도는 실행 파일. 출발할 때 실행 폴더에 자기 사본을 두고 그 사본으로 돌아서,
# 여기서 바꿔 끼워도 도는 실행은 그대로다(docs/자율주행/설계.md). ~/.claude/notifier 로는 옮기지 않는다(보드가 앱 안의 것을 부른다).
# SwiftPM 실행 타깃 ClaudeAutopilot(Sources/ClaudeAutopilot/) — 위 spm 이 보드와 함께 빌드했다
cp "$BIN/ClaudeAutopilot" "$APP/Contents/MacOS/claude-autopilot.new"
mv "$APP/Contents/MacOS/claude-autopilot.new" "$APP/Contents/MacOS/claude-autopilot"
# 엔진 리소스(terminfo·셸 통합) — .app 맨 위에 두면 서명이 거부되므로 Contents/Resources 에 두고 앱이 시작할 때 그 자리를 알려 준다
rm -rf "$APP/Contents/Resources/GhosttyKit_GhosttyTerminal.bundle"
cp -R "$BIN/GhosttyKit_GhosttyTerminal.bundle" "$APP/Contents/Resources/"
cp "$N/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
# 답변 마크다운 꾸미기(md-render) — 실행에 필요한 파일과 LICENSE·README.md 만 이름으로 고른다(시험용 tests/, Claude Code 가
# 저장소 사본에 만드는 tsconfig.json·.claude-plugin/types/ 는 뺀다). 보드가 시작할 때 ~/.claude/notifier/plugins/md-render 로 옮겨 쓴다
rm -rf "$APP/Contents/Resources/plugins"
for f in LICENSE README.md .claude-plugin/plugin.json hooks/hooks.json hooks/register.tsx; do
  mkdir -p "$(dirname "$APP/Contents/Resources/plugins/md-render/$f")"
  cp "$N/plugins/md-render/$f" "$APP/Contents/Resources/plugins/md-render/$f"
done
# 앱에 정적으로 들어간 구성 요소(터미널 엔진 등)의 라이선스 고지
cp "$N/packaging/THIRD_PARTY_NOTICES.txt" "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>ClaudeBoard</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>세션의 터미널(Ghostty·iTerm2·Terminal)로 포커스를 옮기고, 일시중지·재개·중단·보드로 옮기기 때 Ghostty 세션에 키를 입력합니다.</string>
</dict>
</plist>
PLIST
# make-cert.sh 로 만든 개인 인증서가 있으면 그것으로, 없으면 ad-hoc 서명 — 안쪽 실행 파일(엔진) → 앱 순서
if security find-identity -v -p codesigning | grep -qF "\"$SIGN_ID\""; then
  codesign --force --timestamp=none --sign "$SIGN_ID" "$APP/Contents/MacOS/claude-autopilot"
  codesign --force --timestamp=none --sign "$SIGN_ID" "$APP"
else
  codesign --force --sign - "$APP/Contents/MacOS/claude-autopilot" >/dev/null 2>&1 || true
  codesign --force --sign - "$APP" >/dev/null 2>&1 || true
fi
touch "$APP"
echo "built: $APP"
[ -n "$DEV" ] && exit 0
# sessions 등 실행 상태는 그대로 둔다. 알림 패널은 떠 있는 패널이 있어도 깨지지 않게 새 파일로 빌드해 바꿔 끼운다
R="$HOME/.claude/notifier"
mkdir -p "$R/sessions"
swiftc -O "$N/ClaudeNotifier.swift" -o "$R/claude-notifier.new"
mv "$R/claude-notifier.new" "$R/claude-notifier"
cp "$N/notify.py" "$N/state.py" "$N/board-tui.py" "$N/hooks.py" "$R/"
echo "installed: $R"
