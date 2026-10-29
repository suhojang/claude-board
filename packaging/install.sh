#!/bin/bash
# Claude Board 설치·업데이트 — 처음 설치와 업데이트 모두 같은 명령이다
#   curl -fsSL https://github.com/suhojang/claude-board/releases/latest/download/install.sh | bash
# 최신 릴리스의 앱을 받아 서명을 확인하고 /Applications 에 바꿔 끼운 뒤, 앱 안의 user-setup.sh 로 사용자 설정을 하고 보드를 다시 띄운다.
# 전역 settings.json 에 Claude Board hook 이 없을 때(처음 설치)만 hook 등록을 묻는다. 보드의 업데이트 단추도 이 스크립트를 실행한다
set -e
REPO="suhojang/claude-board"
# 앱 서명 인증서(Claude Board Signing) — 인증서가 바뀌면 Ghostty 제어 권한을 다시 물어야 하므로 다른 인증서로 서명된 앱은 설치하지 않는다
CERT_SHA1="a4ad1a544ebf9e6662c18b2cec24a9fe3f828eff"
APP="/Applications/Claude Board.app"

# 설치된 보드와 이 스크립트의 관계 — tab(보드 안 터미널) · updater(보드의 업데이트 단추) · outside(보드 밖)를 찍는다.
# 조상을 거슬러 올라가 그 보드를 만나면, 보드 바로 아래가 탭의 login 이면 보드 탭이고 아니면 업데이트 단추다(보드 → bash -c → 이 스크립트).
# 조상에 보드가 없으면 보드 밖이다
board_relation() {
  local p=$$ child="" n=0
  while [ -n "$p" ] && [ "$p" -gt 1 ] && [ "$n" -lt 64 ]; do
    if [ "$(ps -o comm= -p "$p" 2>/dev/null)" = "$APP/Contents/MacOS/ClaudeBoard" ]; then
      if [ -n "$child" ] && [ "$(basename "$(ps -o comm= -p "$child" 2>/dev/null)")" = "login" ]; then echo tab; else echo updater; fi
      return
    fi
    child="$p"
    p="$(ps -o ppid= -p "$p" 2>/dev/null | tr -d ' ')"
    n=$((n + 1))
  done
  echo outside
}

# 보드 밖에서 실행했는데 설치된 보드가 떠 있으면 정상 종료를 청한다 — ⌘Q 와 같은 길이라 작업 중인 보드 탭 세션이 있으면 확인 창이 뜨고,
# 끄면 그 세션에 일시중지 표지가 남는다(pkill 은 확인·표지 없이 끊는다). 요청은 바꿔 끼우기 전에 지금 깔린 앱으로 보낸다(앱의 --action 은
# md-render 파일부터 옮기므로 새 앱으로 보내면 새 파일이 옛 보드 밑으로 먼저 간다). 요청을 받은 보드는 사용자 임시 폴더의 claude-board.quit 에
# asking 을 쓰고, 확인 창에서 취소하면 cancel 을 쓴다.
# 0 = 보드가 꺼졌거나 떠 있지 않음, 1 = 요청을 모르는 옛 보드(5초 안에 표지가 없음 — 3단계에서 전처럼 끈다), 2 = 취소했거나 10분 안에 답이 없음
ask_board_quit() {
  local ack until
  pgrep -qf "$APP/Contents/MacOS/ClaudeBoard" || return 0
  ack="$(getconf DARWIN_USER_TEMP_DIR)claude-board.quit"
  rm -f "$ack"
  "$APP/Contents/MacOS/ClaudeBoard" --action quit - >/dev/null 2>&1 || return 1
  # 시각은 SECONDS 로 잰다 — sleep 0.1 을 부르는 데도 시간이 들어 횟수로 세면 두 배 넘게 늘어난다
  until=$((SECONDS + 5))
  while [ ! -e "$ack" ] && [ "$SECONDS" -lt "$until" ]; do sleep 0.1; done
  [ -e "$ack" ] || return 1
  echo "   떠 있는 보드에 종료를 청했습니다. 작업 중인 세션이 있어 확인 창이 떴으면 답해 주세요(기다립니다)"
  until=$((SECONDS + 600))
  while [ "$SECONDS" -lt "$until" ]; do
    if ! pgrep -qf "$APP/Contents/MacOS/ClaudeBoard"; then rm -f "$ack"; return 0; fi
    if [ "$(cat "$ack" 2>/dev/null)" = "cancel" ]; then rm -f "$ack"; return 2; fi
    sleep 0.2
  done
  rm -f "$ack"
  return 2
}

# 받는 도중 끊겨도 아무것도 실행되지 않게 전체를 함수로 감싸 마지막 줄에서 부른다
main() {
  export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
  echo "== Claude Board 설치 =="
  if ! python3 -c 'import sys' >/dev/null 2>&1; then
    echo "python3 를 찾지 못했습니다. hook 이 python3 로 실행되므로 먼저 설치하세요(xcode-select --install 또는 Homebrew)." >&2
    exit 1
  fi

  # 처음 설치(hook 이 아직 없음)일 때만 묻는다 — hooks.py 와 같은 표시(.claude/notifier/)로 판단한다
  FIRST=""
  if ! grep -qF '.claude/notifier/' "$HOME/.claude/settings.json" 2>/dev/null; then
    FIRST=1
    echo "Claude Code 전역 설정 ~/.claude/settings.json 에 Claude Board hook 11개와"
    echo "환경변수 CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1 을 등록합니다."
    echo "원본은 settings.json.bak-claude-board-<날짜-시각> 으로 백업하고, 다른 hook·설정은 그대로 둡니다."
    echo "이후 업데이트에서는 다시 묻지 않습니다."
    printf "계속할까요? [Y/n] "
    # curl | bash 로 실행하면 표준 입력이 스크립트이므로 터미널에서 직접 읽는다
    if ! read -r ans </dev/tty; then
      echo; echo "터미널에서 설치 명령을 실행해야 hook 등록을 확인할 수 있습니다." >&2
      exit 1
    fi
    case "$ans" in ""|[Yy]*) ;; *) echo "설치를 취소했습니다."; exit 1 ;; esac
  fi

  W="$(mktemp -d)"
  trap 'rm -rf "$W"' EXIT

  echo "1) 최신 버전 받기"
  curl -fsSL "https://github.com/$REPO/releases/latest/download/ClaudeBoard.zip" -o "$W/app.zip"
  ditto -x -k "$W/app.zip" "$W/new"
  NEW="$W/new/Claude Board.app"
  if ! codesign --verify --deep --strict -R="certificate leaf = H\"$CERT_SHA1\"" "$NEW" 2>/dev/null; then
    echo "받은 앱의 서명이 Claude Board Signing 인증서와 맞지 않아 설치하지 않습니다." >&2
    exit 1
  fi
  echo "   Claude Board $(plutil -extract CFBundleShortVersionString raw "$NEW/Contents/Info.plist")"

  # 보드 밖에서 실행했으면 바꿔 끼우기 전에 떠 있는 보드에 정상 종료를 청한다 — 취소하면 아무것도 바꾸지 않고 멈춘다(받은 파일은 trap 이 지움)
  REL="$(board_relation)"
  if [ "$REL" = outside ]; then
    QUIT=0
    ask_board_quit || QUIT=$?
    if [ "$QUIT" = 2 ]; then
      echo "보드 종료를 취소했거나 10분 안에 답이 없어 설치하지 않았습니다. 바꾼 것은 없습니다. 다시 실행하면 처음부터 합니다." >&2
      exit 1
    fi
  fi

  echo "2) 앱 설치: $APP"
  if [ -e "$APP" ] || ! mv "$NEW" "$APP" 2>/dev/null; then
    # 이미 있는 앱은 Contents 만 바꿔 끼운다 — 앱 폴더만 내 소유면 되므로 다음 업데이트부터는 관리자 비밀번호가 필요 없다.
    # pkg 로 설치한 앱(root 소유)이거나 /Applications 에 쓸 수 없는 계정이면 이번 한 번만 관리자 비밀번호 창을 띄운다
    if [ ! -w "$APP" ] || { [ -e "$APP/Contents" ] && [ ! -w "$APP/Contents" ]; }; then
      echo "   앱 폴더를 이 계정 소유로 바꿉니다. 관리자 비밀번호 창이 뜹니다(처음 한 번만)"
      osascript -e "do shell script \"mkdir -p '$APP' && chown -R $(id -un):staff '$APP'\" with administrator privileges" >/dev/null
    fi
    if [ -e "$APP/Contents" ]; then mv "$APP/Contents" "$W/old"; fi
    mv "$NEW/Contents" "$APP/Contents" || { mv "$W/old" "$APP/Contents"; exit 1; }
  fi
  touch "$APP"

  echo "3) 사용자 설정(알림 패널·hook·명령·자동 실행)"
  # 실행 중인 보드를 끄고 user-setup.sh 가 자동 실행 등록과 함께 새 앱을 띄운다.
  # 보드의 업데이트 단추로 실행했어도 이 스크립트는 별도 프로세스 그룹이라 보드가 꺼져도 끝까지 돈다.
  # macOS pkill 은 자기 조상을 빼고 찾는다 — 업데이트 단추로 돌면 보드가 이 스크립트의 조상이라 -a 로 함께 끈다(보드가 이미 확인 창을 거쳤다).
  # 보드 밖이면 위에서 정상 종료를 청했으니 남은 것은 그 요청을 모르는 옛 보드(와 시험 빌드)뿐이다.
  # 보드 안 터미널(보드 탭)에서 손으로 돌렸으면 보드를 끄지 않는다 — 보드를 끄는 순간 탭의 세션이 확인·일시중지 표지 없이 끊기고
  # 이 스크립트도 함께 끊겨 새 보드를 띄우지 못한다. user-setup.sh 에도 알려(CLAUDE_BOARD_KEEP_RUNNING) 로그인 자동 실행으로 뜬 보드를
  # launchctl 로 내렸다 올리지 않게 한다(내리면 보드가 꺼진다). 출력이 파일이어도(에이전트가 돌림 등) 보드 탭이면 같다
  KEPT=""
  case "$REL" in
    tab) KEPT=1 ;;
    updater) pkill -a -x ClaudeBoard 2>/dev/null || true ;;
    *) pkill -x ClaudeBoard 2>/dev/null || true ;;
  esac
  if ! CLAUDE_BOARD_KEEP_RUNNING="$KEPT" bash "$APP/Contents/Resources/user-setup.sh" "$APP"; then
    open -a "$APP" || true
    echo "사용자 설정 중 실패했습니다. 위 메시지를 확인하고 다시 실행하세요." >&2
    exit 1
  fi

  echo
  echo "설치를 마쳤습니다."
  if [ -n "$KEPT" ]; then
    echo "보드 안 터미널에서 실행해 지금 보드는 끄지 않았습니다. 새 세션은 바로 새 hook 을 쓰고, 보드는 종료(⌘Q)하고 다시 열면 새 버전이 뜹니다."
  fi
  if [ -n "$FIRST" ]; then
    echo "이미 실행 중인 Claude Code 세션은 다시 시작해야 hook 이 적용됩니다."
    echo "처음 알림·더블 클릭 때 macOS 가 'Ghostty 제어' 권한을 물으면 허용하세요."
  fi
}

main "$@"
