#!/bin/sh
# Claude Board 배포용 dmg 를 만든다 — 설치 패키지(.pkg)·설치 가이드·제거 스크립트를 담고, 앱과 dmg 를 개인 인증서로 서명한다
# pkg 는 서명하지 않는다 — macOS 는 pkg 서명을 Apple 발급 인증서로만 허용한다(자체 서명은 "Certificate authority pinning mismatch")
# 받는 사람은 dmg 안의 pkg 를 열고 관리자 비밀번호만 입력하면 설치된다(/Applications + 사용자 설정)
# 사용법: make-dmg.sh <버전>   서명 인증서 이름은 CLAUDE_BOARD_SIGN_ID (기본 "Claude Board Signing" — make-cert.sh 로 만든다)
# 결과: <저장소>/dist/ClaudeBoard-<버전>.dmg, curl 설치·업데이트용 앱 zip ClaudeBoard-<버전>-app.zip (release.sh 가 올린다)
set -e
VERSION="${1:?사용법: make-dmg.sh <버전>}"
SIGN_ID="${CLAUDE_BOARD_SIGN_ID:-Claude Board Signing}"
N="$(cd "$(dirname "$0")/.." && pwd)"
P="$N/packaging"
OUT="$N/dist"
WORK="$(mktemp -d)"
ROOT="$WORK/root"
APP="$ROOT/Claude Board.app"
STAGE="$WORK/Claude Board"
trap 'rm -rf "$WORK"' EXIT

if ! security find-identity -v -p codesigning | grep -qF "\"$SIGN_ID\""; then
  echo "서명 인증서 '$SIGN_ID' 를 찾지 못했습니다. 먼저 $P/make-cert.sh 를 실행하세요." >&2
  exit 1
fi

build() {  # $1 소스, $2 결과 — 두 아키텍처로 빌드해 하나로 합친다
  swiftc -O -target arm64-apple-macos14 "$1" -o "$WORK/arm64"
  swiftc -O -target x86_64-apple-macos14 "$1" -o "$WORK/x86_64"
  lipo -create "$WORK/arm64" "$WORK/x86_64" -output "$2"
  rm -f "$WORK/arm64" "$WORK/x86_64"
}
# 보드는 터미널 엔진(libghostty-spm) 때문에 SwiftPM 으로 빌드한다 — $1 아키텍처. --arch 두 개(한 번에 universal)는 Xcode 가 필요해서
# 아키텍처마다 따로 빌드해 합친다. --disable-keychain 이 없으면 엔진 내려받기에서 키체인 조회로 멈춘다
spm() { a="$1"; shift; swift build -c release --disable-keychain --package-path "$N" --triple "$a-apple-macosx14.0" "$@"; }

echo "1) 앱 빌드"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/notifier" "$STAGE" "$OUT"
spm arm64
spm x86_64
lipo -create "$(spm arm64 --show-bin-path)/ClaudeBoard" "$(spm x86_64 --show-bin-path)/ClaudeBoard" \
  -output "$APP/Contents/MacOS/ClaudeBoard"
# 엔진 리소스(terminfo·셸 통합)는 아키텍처와 무관해 한 벌 — .app 맨 위는 서명이 거부하므로 Contents/Resources 에 둔다
cp -R "$(spm arm64 --show-bin-path)/GhosttyKit_GhosttyTerminal.bundle" "$APP/Contents/Resources/"
build "$N/ClaudeNotifier.swift" "$APP/Contents/Resources/notifier/claude-notifier"
# 자율주행 엔진 — build-board.sh 와 같은 자리. 출발할 때 실행 폴더에 자기 사본을 두고 launchd 가 그 사본을 띄운다(docs/자율주행/설계.md)
build "$N/ClaudeAutopilot.swift" "$APP/Contents/MacOS/claude-autopilot"
cp "$N/notify.py" "$N/state.py" "$N/board-tui.py" "$P/hooks.py" "$APP/Contents/Resources/notifier/"
cp "$N/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cp "$N/LICENSE" "$APP/Contents/Resources/LICENSE"
# 앱에 정적으로 들어간 구성 요소(터미널 엔진 등)의 라이선스 고지
cp "$P/THIRD_PARTY_NOTICES.txt" "$APP/Contents/Resources/"
# 답변 마크다운 꾸미기(md-render) — 실행에 필요한 파일과 LICENSE·README.md 만 이름으로 고른다(시험용 tests/, Claude Code 가
# 저장소 사본에 만드는 tsconfig.json·.claude-plugin/types/ 는 뺀다). 보드가 시작할 때 ~/.claude/notifier/plugins/md-render 로 옮겨 쓴다
for f in LICENSE README.md .claude-plugin/plugin.json hooks/hooks.json hooks/register.tsx; do
  mkdir -p "$(dirname "$APP/Contents/Resources/plugins/md-render/$f")"
  cp "$N/plugins/md-render/$f" "$APP/Contents/Resources/plugins/md-render/$f"
done
# install.sh(curl 설치·업데이트)가 앱을 바꿔 끼운 뒤 실행하는 사용자 설정 — 서명 전에 넣어야 앱 서명에 포함된다
cp "$P/pkg/scripts/user-setup.sh" "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Claude Board</string>
  <key>CFBundleDisplayName</key><string>Claude Board</string>
  <key>CFBundleIdentifier</key><string>com.claude.board</string>
  <key>CFBundleExecutable</key><string>ClaudeBoard</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>세션의 터미널(Ghostty·iTerm2·Terminal)로 포커스를 옮기고, 일시중지·재개·중단·보드로 옮기기 때 Ghostty 세션에 키를 입력합니다.</string>
</dict>
</plist>
PLIST

echo "2) 서명: $SIGN_ID"
# 안쪽 실행 파일 → 앱 순서. hardened runtime 은 공증할 때만 필요하다(Ghostty 제어용 entitlement 가 함께 필요)
codesign --force --timestamp=none --sign "$SIGN_ID" "$APP/Contents/Resources/notifier/claude-notifier"
codesign --force --timestamp=none --sign "$SIGN_ID" "$APP/Contents/MacOS/claude-autopilot"
codesign --force --timestamp=none --sign "$SIGN_ID" "$APP"
codesign --verify --strict "$APP"
# curl 설치·업데이트용 — 서명한 앱만 담는다
ditto -c -k --norsrc --noextattr --noacl --keepParent "$APP" "$OUT/ClaudeBoard-$VERSION-app.zip"

echo "3) 설치 패키지"
# 같은 번들 ID 의 앱이 다른 곳(예: 예전 ~/Applications 설치)에 있으면 Installer 가 그쪽을 덮어쓰지 않도록 위치 고정
pkgbuild --analyze --root "$ROOT" "$WORK/component.plist" >/dev/null
plutil -replace 0.BundleIsRelocatable -bool NO "$WORK/component.plist"
pkgbuild --root "$ROOT" --component-plist "$WORK/component.plist" --install-location /Applications \
  --scripts "$P/pkg/scripts" --identifier com.claude.board --version "$VERSION" \
  "$WORK/ClaudeBoard-component.pkg" >/dev/null
mkdir -p "$WORK/resources"
cp "$P/README.txt" "$P/pkg/conclusion.txt" "$WORK/resources/"
sed "s/@VERSION@/$VERSION/" "$P/pkg/Distribution.xml" > "$WORK/Distribution.xml"
PKG="$STAGE/Claude Board 설치.pkg"
productbuild --distribution "$WORK/Distribution.xml" --package-path "$WORK" --resources "$WORK/resources" \
  "$PKG" >/dev/null

echo "4) dmg"
cp "$P/Uninstall.command" "$STAGE/"
chmod +x "$STAGE/Uninstall.command"
cp "$P/README.txt" "$STAGE/설치 가이드.txt"
cp "$P/README.txt" "$OUT/ClaudeBoard-$VERSION-설치 가이드.txt"
[ -f "$OUT/$SIGN_ID.cer" ] && cp "$OUT/$SIGN_ID.cer" "$STAGE/서명 인증서.cer"
DMG="$OUT/ClaudeBoard-$VERSION.dmg"
rm -f "$DMG"
hdiutil create -volname "Claude Board" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
codesign --force --timestamp=none --sign "$SIGN_ID" "$DMG"
codesign --verify "$DMG"
echo "built: $DMG"
