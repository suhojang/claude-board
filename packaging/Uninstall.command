#!/bin/bash
# Claude Board 제거 — 설치 스크립트가 넣은 앱·파일·hook·로그인 항목·셸 설정 줄을 지운다
N="$HOME/.claude/notifier"
echo "== Claude Board 제거 =="

HOOKS="$N/hooks.py"
[ -f "$HOOKS" ] || HOOKS="/Applications/Claude Board.app/Contents/Resources/notifier/hooks.py"
echo "1) 전역 settings.json 에서 hook 제거"
python3 "$HOOKS" uninstall || echo "   (hook 제거 실패: ~/.claude/settings.json 에서 .claude/notifier/ 가 들어간 hook 을 직접 지우세요)"

echo "2) 앱·자동 실행 제거: /Applications 의 앱을 지우려면 관리자 비밀번호가 필요합니다"
pkill -x ClaudeBoard 2>/dev/null || true
launchctl bootout "gui/$(id -u)/com.claude.board" 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/com.claude.board.plist"
osascript -e 'tell application "System Events" to delete (every login item whose name is "Claude Board")' >/dev/null 2>&1 || true
rm -rf "$HOME/Applications/Claude Board.app"
sudo /bin/sh -c 'rm -rf "/Applications/Claude Board.app"; pkgutil --forget com.claude.board >/dev/null 2>&1; true' \
  || echo "   (/Applications/Claude Board.app 을 지우지 못했습니다. Finder 에서 휴지통으로 옮기세요)"

echo "3) 자율주행 따로 작업 트리 정리(~/.claude-board/worktrees)"
# 자율주행이 단일 저장소를 돌릴 때 만든 작업 트리. 커밋한 일은 원래 저장소의 autopilot/<실행 id> 가지에 그대로 남는다(여기서는 작업 트리만 지운다).
WT="$HOME/.claude-board/worktrees"
if [ -d "$WT" ]; then
  for d in "$WT"/*/*/; do
    [ -f "$d/.git" ] || continue
    # .git 파일의 gitdir 은 <원래 .git>/worktrees/<이름> — 그 두 단계 위가 원래 저장소의 .git 이다(그 안에서는 --show-toplevel 이 안 된다)
    gd=$(sed -n 's/^gitdir: //p' "$d/.git" 2>/dev/null)
    common=$(cd "$gd/../.." 2>/dev/null && pwd)
    rm -rf "$d"
    [ -n "$common" ] && git --git-dir="$common" worktree prune 2>/dev/null
  done
  rm -rf "$WT"
  rmdir "$HOME/.claude-board" 2>/dev/null || true
  echo "   정리했습니다(원래 저장소의 autopilot 가지는 그대로 둡니다)."
fi

echo "4) 알림 패널·hook 스크립트·저장한 계정 제거"
for f in claude-notifier notify.py state.py board-tui.py hooks.py notify-position login-launch-off; do rm -f "$N/$f"; done
rm -rf "$N/sessions" "$N/__pycache__" "$N/plugins"
# 계정 전환용으로 저장해 둔 로그인 토큰(키체인)과 계정 목록 — 지금 Claude Code 로그인("Claude Code-credentials")은 그대로 둔다
while security delete-generic-password -s "Claude Board-account" >/dev/null 2>&1; do :; done
# accounts.json.broken-*: 깨진 목록을 보드가 옆으로 치운 것(이메일이 들어 있다)
rm -rf "$N/accounts.json" "$N"/accounts.json.broken-* "$N/accounts"
rmdir "$N" 2>/dev/null || true
rm -f "$HOME/.local/bin/claude-board" "$HOME/.local/bin/claude-board-tui"

if grep -q ">>> claude-board >>>" "$HOME/.zshrc" 2>/dev/null; then
  echo "5) ~/.zshrc 에서 Claude Board 줄 제거"
  cp "$HOME/.zshrc" "$HOME/.zshrc.bak-claude-board"
  python3 - <<'PY'
import os, re
p = os.path.expanduser("~/.zshrc")
s = open(p).read()
s = re.sub(r"\n?# >>> claude-board >>>.*?# <<< claude-board <<<\n?", "\n", s, flags=re.S)
open(p, "w").write(s)
PY
fi
echo
echo "제거를 마쳤습니다. 실행 중인 Claude Code 세션은 다시 시작해야 hook 제거가 적용됩니다."
