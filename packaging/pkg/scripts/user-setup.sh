#!/bin/bash
# Claude Board 사용자별 설정 — postinstall 이 설치한 사용자 권한으로 실행한다
# 알림 패널·hook 스크립트 복사, 전역 settings.json hook 등록, 명령·로그인 자동 실행·Ghostty 연동 설정
set -e
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
APP="$1"
RES="$APP/Contents/Resources/notifier"
N="$HOME/.claude/notifier"
BIN="$HOME/.local/bin"
LABEL="com.claude.board"
AGENT="$HOME/Library/LaunchAgents/$LABEL.plist"

if ! python3 -c 'import sys' >/dev/null 2>&1; then
  echo "Claude Board: python3 가 없습니다. hook 이 python3 로 실행되므로 먼저 설치해야 합니다(xcode-select --install)" >&2
  exit 1
fi

# 1) 알림 패널·hook 스크립트
mkdir -p "$N/sessions"
for f in claude-notifier notify.py state.py board-tui.py hooks.py; do
  cp "$RES/$f" "$N/$f"
done
chmod +x "$N/claude-notifier" "$N/notify.py" "$N/state.py" "$N/board-tui.py"
xattr -d com.apple.quarantine "$N/claude-notifier" 2>/dev/null || true

# 2) 전역 settings.json hook (백업 후 Claude Board hook 만 다시 넣는다)
python3 "$N/hooks.py" install

# 3) 명령
mkdir -p "$BIN"
printf '#!/bin/sh\n# Claude Board 앱을 연다 — 이미 떠 있으면 창을 앞으로 불러온다\nopen -a "%s"\n' "$APP" > "$BIN/claude-board"
chmod +x "$BIN/claude-board"
ln -sf "$N/board-tui.py" "$BIN/claude-board-tui"

# 4) 예전 판(~/Applications 설치)이 남아 있으면 지운다 — 같은 번들 ID 가 두 곳에 있으면 실행 대상이 섞인다
rm -rf "$HOME/Applications/Claude Board.app"

# 5) Ghostty 를 열 때 보드가 꺼져 있으면 함께 띄우는 줄 — 예전 블록은 새 경로로 바꾼다
if [ -d "/Applications/Ghostty.app" ] || [ -d "$HOME/Applications/Ghostty.app" ]; then
  touch "$HOME/.zshrc"
  APP="$APP" python3 - <<'PY'
import os, re
p = os.path.expanduser("~/.zshrc")
s = open(p).read()
block = (
    "# >>> claude-board >>>\n"
    "# Ghostty 에서 셸이 시작될 때 보드 앱이 꺼져 있으면 함께 띄운다 (포커스는 터미널 유지)\n"
    "# -a: 보드 안 터미널의 셸은 보드가 조상이라 -a 없이는 못 찾는다\n"
    'if [[ "$TERM_PROGRAM" == "ghostty" ]] && ! pgrep -axq ClaudeBoard; then\n'
    f'  open -g -a "{os.environ["APP"]}" --args --background\n'
    "fi\n"
    "# <<< claude-board <<<\n"
)
pat = re.compile(r"# >>> claude-board >>>.*?# <<< claude-board <<<\n?", re.S)
s = pat.sub(lambda _: block, s) if pat.search(s) else s.rstrip("\n") + "\n\n" + block
open(p, "w").write(s)
PY
fi

# 6) 로그인할 때 자동 실행 (LaunchAgent) — 등록하면서 지금 바로 한 번 띄운다. open -a 는 로그인 직후 LaunchServices 준비 전에 실패할 수 있어 실행 파일을 직접 띄운다.
#    보드 설정(설정 > 일반 > 로그인할 때 열기)에서 끈 사람(표지 파일 login-launch-off)은 등록하지 않고 지금 한 번만 띄운다
#    CLAUDE_BOARD_KEEP_RUNNING(install.sh 를 보드 안 터미널에서 돌림)이면 떠 있는 보드를 끄지도 새로 띄우지도 않는다 — 로그인 자동 실행으로 뜬 보드는
#    launchctl bootout 이 끄고(탭의 세션이 끊기고 그 안에서 돌던 설치도 함께 끊긴다), plist 만 맞춰 두면 다음 로그인·⌘Q 뒤 다시 열 때 새 앱이 뜬다
KEEP="${CLAUDE_BOARD_KEEP_RUNNING:-}"
if [ -e "$N/login-launch-off" ]; then
  [ -n "$KEEP" ] || launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
  rm -f "$AGENT"
  [ -n "$KEEP" ] || open -a "$APP"
  exit 0
fi
mkdir -p "$(dirname "$AGENT")"
cat > "$AGENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$APP/Contents/MacOS/ClaudeBoard</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
PLIST
[ -z "$KEEP" ] || exit 0
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$AGENT" 2>/dev/null || open -a "$APP"
