<p align="center"><img src="../docs/images/icon.png" width="128" alt="Claude Board 아이콘 — 상태 점이 붙은 세션 카드가 쌓인 보드"></p>

<h1 align="center">Claude Board for Windows</h1>

<p align="center">Claude Code 세션 여러 개를 한 화면에서 지켜보는 Windows 보드 앱</p>

macOS 판([README](../README.md))과 같은 보드 · 알림 패널 · 사용 한도 · 계정 전환을 Windows 10 · 11 에서 씁니다.
버전별 변경은 [CHANGELOG.md](CHANGELOG.md), 모든 기능의 자세한 설명은 [설치 가이드](docs/설치%20가이드.md) 를 보세요.

> 🧪 **첫 Windows 판입니다.** 보드 · 알림 · hook · 사용 한도 · 계정 전환은 macOS 판과 같은 규칙으로 동작합니다.
> 다만 세션의 터미널 창을 직접 다루는 기능(창 · 탭으로 이동, 일시중지 · 재개 · 중단, 자동 전환 뒤 이어 가기, 터미널 제목 바꾸기)은
> Windows 콘솔 · Windows Terminal 의 내부 동작에 기대므로 환경에 따라 동작하지 않을 수 있습니다(최선 노력).

## 💡 컨셉

Claude Code 를 여러 프로젝트 · 터미널 탭에서 함께 돌리면, 어느 세션이 승인을 기다리는지, 어느 세션이 응답을 끝냈는지 알려면 탭을 하나씩 돌아봐야 합니다.
Claude Board 는 그 확인을 보드 한 장으로 옮깁니다.

- 🗂️ **한눈에** — 프로젝트 폴더(claude 를 띄운 폴더)마다 카드 하나에 세션을 묶고, 급한 순서(승인 대기 → 작업 중 → 대기)로 보여 줍니다. Claude 가 하위 폴더로 `cd` 하거나 워크트리에 들어가도 세션은 같은 카드에 남습니다.
- 🔔 **놓치지 않게** — 승인이 필요하거나 응답이 끝나면 화면 가운데(트레이 메뉴에서 네 모서리로 바꿀 수 있음)에 알림 패널이 뜹니다. 키보드 입력은 가져가지 않습니다.
- 🎯 **바로 그 자리로** — 세션을 더블 클릭하면 그 세션의 터미널 창으로 이동하고(Windows Terminal 이면 그 탭까지), 종료된 세션은 `claude --resume` 으로 이어 엽니다.
- 📊 **한도까지 함께** — 5시간 · 주간 사용 한도와 계정 전환을 보드에서 바로 다룹니다.

설치하면 Claude Code hook 이 세션마다 상태를 `%USERPROFILE%\.claude\notifier\sessions\` 에 기록하고, 보드와 알림 패널이 그 기록을 읽어 보여 줍니다.
기록 형식은 macOS 판과 같습니다.

## 🧰 필요한 환경

- 🪟 Windows 10 1809(빌드 17763) 이상 또는 Windows 11 — x64 (ARM64 Windows 에서는 확인하지 않았습니다). 더 낮으면 설치가 시작 전에 멈춥니다
- 🌐 Microsoft Edge WebView2 런타임 — Windows 11 에는 기본으로 들어 있습니다. 없으면 setup.exe 는 설치 중에 받아 설치하고, PowerShell 설치는 없다고 알려 줍니다.
- 🤖 Claude Code — 네이티브 설치(`irm https://claude.ai/install.ps1 | iex`)를 권장합니다. `claude-board` · `claude-board-tui` 명령이 들어가는 `%USERPROFILE%\.local\bin` 을 PATH 에 넣어 줍니다.
- 🐍 python 은 필요 없습니다. hook 은 실행 파일 하나(`claude-board-hook.exe`)입니다.
- 🖥️ 세션 창 이동 · 이어 열기는 Windows Terminal 을 권장합니다. 다른 콘솔에서도 보드와 알림은 동작합니다.

## 📦 설치

PowerShell 에서 한 줄로 설치합니다(Windows PowerShell 5.1 · PowerShell 7 모두 됩니다).

```powershell
irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/install.ps1 | iex
```

- ⚙️ 처음 설치할 때 `%USERPROFILE%\.claude\settings.json` 에 hook 11개와 환경변수 `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1` 을 등록할지 묻습니다(Enter = 예).
  원본은 `settings.json.bak-claude-board-<날짜-시각>` 으로 백업하고, 다른 hook · 설정은 그대로 둡니다.
- 🔐 관리자 권한은 필요 없습니다. 모든 파일이 사용자 폴더에만 설치됩니다.
- ✅ 받은 zip 은 릴리스에 함께 올린 SHA256 값과 맞춰 본 뒤에만 설치합니다.
- 🎉 설치가 끝나면 보드가 열립니다. 이미 실행 중인 Claude Code 세션은 다시 시작해야 hook 이 적용됩니다.

### 💿 설치 프로그램(setup.exe)으로 설치

설치 프로그램 [ClaudeBoard-2.12-setup.exe](https://github.com/suhojang/claude-board/releases/download/windows-v2.12/ClaudeBoard-2.12-setup.exe) 를 받아 실행해도 됩니다([windows-v2.12 릴리스](https://github.com/suhojang/claude-board/releases/tag/windows-v2.12)).
다른 버전은 [릴리스 목록](https://github.com/suhojang/claude-board/releases)에서 `windows-v` 로 시작하는 릴리스를 고르세요. **Latest** 표시가 붙은 것은 macOS 판입니다.

- 🛡️ 코드 서명이 없어 SmartScreen 이 "Windows 의 PC 보호" 창을 띄웁니다. **추가 정보 → 실행** 을 누르면 설치됩니다.
- 사용자 단위로 설치되어 관리자 권한을 묻지 않습니다. 설치가 끝나면 hook 등록 · 명령 · 자동 실행 설정까지 함께 합니다(따로 묻지 않습니다).

설치하면 `%LOCALAPPDATA%\Programs\Claude Board\` 의 앱(setup.exe 로 설치하면 `%LOCALAPPDATA%\Claude Board\`), `%USERPROFILE%\.claude\notifier\` 의 hook 실행 파일,
로그인 자동 실행(레지스트리 `HKCU\...\Run` 의 `ClaudeBoard` 값), `%USERPROFILE%\.local\bin` 의 `claude-board` · `claude-board-tui` 명령이 생깁니다.

## 🕹️ 사용법

### 🪟 보드 열기

- 🔁 로그인할 때 저절로 실행됩니다. 보드가 숨어 있을 때 Windows Terminal 을 새로 열면 보드가 다시 보입니다(포커스는 터미널에 그대로).
- 🖱️ 작업 표시줄 알림 영역(트레이)의 Claude Board 아이콘을 누르면 보드를 열고 닫습니다. 마우스를 올리면 승인 대기 · 작업 중 세션 수가 보이고,
  승인 대기가 있으면 주황 점, 작업 중만 있으면 파란 점이 아이콘에 붙습니다.
- ⌨️ 어느 앱에서든 **Ctrl+Alt+B** 를 누르면 터미널을 접고 보드가 처음 크기로 화면 가장자리(기본은 오른쪽)에 붙어 앞으로 나옵니다.
- ⌨️ 어느 앱에서든 **Ctrl+Alt+T** 를 누르면 보드 안의 터미널이 펼쳐지거나 접힙니다(아래 내장 터미널 절).
- 💻 터미널에서 `claude-board` 는 보드 창을, `claude-board-tui` 는 터미널판 보드를 엽니다.
- ❎ 보드의 닫기 단추는 창만 숨깁니다. 완전히 끄려면 트레이 아이콘 오른쪽 클릭 > **종료**.

### ⚡ 자주 쓰는 조작

| 하고 싶은 것 | 방법 |
|---|---|
| 🧭 그 세션의 터미널로 가기 | 세션 줄을 더블 클릭 — 그 세션의 터미널 창을 앞으로 가져오고, Windows Terminal 이면 그 탭을 고릅니다(최선 노력) |
| ⏯️ 끝난 세션 이어 열기 | 종료된 세션을 더블 클릭 — 새 콘솔 창(Windows 의 기본 터미널 — Windows 11 은 보통 Windows Terminal)에서 `claude --resume` 으로 이어 엽니다. claude 가 끝나면 창도 닫힙니다 |
| ⏸️ 일시중지 · 재개 · 중단 | 세션 줄의 … 메뉴 — 그 세션의 콘솔에 Esc · 이어 가기 문구 · `/exit` 를 넣습니다(최선 노력, 터미널 없이 도는 세션은 안 됨) |
| ⎋ Esc 로 끊은 세션 | 따로 할 일이 없습니다. 세션을 Esc 로 끊으면 보드와 알림 패널이 바로 대기로 보여 줍니다. 승인을 처리하면 바로 작업 중으로 바뀝니다 |
| 🏷️ 세션 이름 보기 | 세션 줄의 태그 아이콘 옆에 Claude Code 세션 이름(`/rename` · `--name`)이 보입니다. 24자를 넘으면 가운데를 줄이고, 마우스를 올리면 전체 이름이 나옵니다. 보드 칸이 좁으면 빠집니다 |
| 🔍 한 상태만 보기 | 윗줄의 상태 칩(승인 대기 · 작업 중 · 대기)을 누르기, 다시 누르면 모두 보기 |
| 👤 계정 바꾸기 | 사용 한도 카드 왼쪽 위 계정을 누르고 저장한 계정 고르기 (**계정 추가…** 로 미리 저장) |
| 🔀 한도가 차면 계정 자동 전환 | 계정 메뉴의 **한도가 차면 다른 계정으로 자동 전환** 을 켜기 (기본 꺼짐) — 한도로 멈춘 세션은 전환 40초 뒤 이어서 진행합니다(콘솔에 입력을 넣을 수 있는 세션만) |
| 🔕 알림 끄기 | 세션 줄의 종 토글은 그 세션만, 윗줄의 종 단추는 전체 |
| 📍 알림 위치 바꾸기 | 트레이 아이콘 오른쪽 클릭 > **알림 위치** 에서 중앙(기본) · 왼쪽 상단 · 오른쪽 상단 · 왼쪽 하단 · 오른쪽 하단 |
| 🤏 창을 작게 두기 | 오른쪽 위 최대축소 단추(↘↖)로 미니 화면, 같은 자리의 ⤢ 로 원래 크기 |
| 🌫️ 다른 앱을 쓰는 동안 흐리게 | 오른쪽 아래 투명도 슬라이더 |
| 🎭 캐릭터 바꾸기 | 캐릭터를 오른쪽 클릭하거나 트레이 아이콘 오른쪽 클릭 > **캐릭터** |
| 🖥️ 터미널 열기 | 창 오른쪽 아래 끝의 탭에 마우스를 대면 Windows Terminal · PowerShell 7 · Windows PowerShell · 명령 프롬프트 · Git Bash 단추가 나옵니다 |

제목 옆에는 Claude Board 버전이, 그 아래 활성 세션 수 옆에는 설치된 Claude Code 버전이 보입니다.
화면 구성(보드 · 알림 패널 · 중간 화면 · 미니 화면 · 새 버전 알림)은 macOS 판과 같습니다 — 그림은 [macOS 판 README](../README.md) 를 보세요.

### 🖥️ 내장 터미널

보드 창 안에 터미널 영역이 있습니다. 위에서 아래로 제목 띠 · 경로 줄 · 탭 줄 · 화면입니다.
세션을 시작하면 그 폴더의 새 탭이 열리고, 탭마다 셸(PowerShell 7 · Windows PowerShell · 명령 프롬프트 중 이 PC 에 있는 것, 이 순서) 하나가 돕니다.
`claude` 는 그 셸에 보드가 대신 쳐 넣어 실행하므로, Claude Code 를 끝내면(`/exit`) 같은 탭이 셸 프롬프트로 돌아옵니다. 셸을 끝내면 탭이 닫힙니다.
탭이 하나도 없으면 "보드에서 띄운 세션이 없습니다" 와 **새 세션** 단추가 보입니다.
화면 나누기 · 탭 저장 · 세션 연결 칩 · 일시중지 · 재개 · 중단 · 글꼴 크기 · 테마 · 답변 마크다운 꾸미기 · 애니메이션 · ⋯ 패널은 아직 없습니다.
2026-10-21 Windows 11 에서 만든 앱으로 확인한 것은 빈 화면 · 탭 줄 · 경로 줄이 그려지는 것, + 단추로 탭 열기, 새 세션 창에 보드 경로가 뜨는 것, pwsh 탭에서 `claude` 가 시작되어 바르게 그려지는 것, 접고 펼쳐도 출력이 남는 것, 셸 종료 뒤 PowerShell 프롬프트, 셸이 창 크기를 아는 것(99x44)뿐입니다.
나머지(실제 한글 입력, Windows 10, 배율이 다른 모니터, 폴더 선택 창, cmd.exe 탭의 이름 처리 등)는 아직 확인하지 않았습니다(미확인).

| 하고 싶은 것 | 방법 |
|---|---|
| 터미널 펼치기 · 접기 | 제목 막대의 터미널 단추, 트레이 아이콘 오른쪽 클릭 > **터미널 펼치기 (Ctrl+Alt+T)**, 또는 어느 앱에서든 **Ctrl+Alt+T** |
| 새 세션 시작 | 탭 줄의 **+ Ctrl+T**, 빈 화면의 **새 세션**, 또는 터미널이 펼쳐진 동안 **Ctrl+T**. 폴더(보드에 있는 경로 또는 다른 폴더)와 명령을 골라 **새 탭에서 시작** |
| 경로 카드에서 바로 시작 | 보드 카드 머리의 **+** : 그 경로에서 기본 명령(`claude --dangerously-skip-permissions`)으로 새 탭을 엽니다 |
| 탭 · 경로 고르기 | 탭이나 경로 줄 항목을 누르기. 키는 **Ctrl+Alt+1~9**(지금 경로의 n 번째 탭), **Ctrl+Alt+← →**(이전/다음 탭), **Ctrl+Alt+↑ ↓**(이전/다음 경로) |
| 탭 이름 바꾸기 | **Ctrl+Shift+R** (Enter 저장, Esc 취소) |
| 탭 닫기 | 탭의 × . Claude Code 가 아직 도는 탭이면 먼저 묻습니다 |
| 보드 폭 바꾸기 | 터미널과 보드 사이의 경계를 끌기. 놓으면 그 폭을 기억합니다 |
| 보드를 왼쪽에 두기 | 트레이 아이콘 오른쪽 클릭 > **보드 위치** > 왼쪽 (기본은 오른쪽) |
| 보드만 오른쪽에 붙이기 | **Ctrl+Alt+B** 로 터미널을 접고 보드를 고른 쪽에 붙입니다 |

- 펼치면 창이 화면 작업 영역 전체로 넓어지고 항상 위 고정과 투명도는 꺼집니다. 접으면 끌어 둔 폭으로 보드 쪽 가장자리에 붙고 항상 위 고정이 돌아옵니다.
- 경로 줄은 탭이 있는 폴더를 보여 주고(상태 점 · 폴더 이름 · 탭 수), 탭 줄은 지금 경로의 탭만 보여 줍니다. 넘치면 가로로 밀립니다. Ctrl+Alt 를 누르고 있으면 탭에 번호가 나타납니다.
- 새 세션이 `claude` 면 보드가 `--session-id` 와 `--name <폴더 이름>` 을 붙입니다(같은 이름이 있으면 `-2`, `-3`). `--resume` · `--continue` 명령에는 `--session-id` 를 붙이지 않고, 직접 `--name` 을 적었으면 그대로 둡니다.
- 접어 둔 동안에도 터미널 출력은 쌓이고, 다시 펼치면 이어서 보입니다. 보드를 끄면 터미널의 셸도 끊깁니다. 보드를 다시 켜면 탭은 복원되지 않습니다.
- Ctrl+T 와 Ctrl+Shift+R 은 터미널이 펼쳐진 동안 보드가 가져가므로 셸 안에서는 쓸 수 없습니다.
- ⚠️ **Ctrl+Alt+T 는 JetBrains IDE 등 다른 앱의 같은 단축키와 겹칩니다.** 보드가 켜져 있는 동안은 보드가 가져가고, 다른 앱이 먼저 쓰고 있으면 보드 쪽 등록이 실패합니다(`board.log` 에 기록, 단추와 트레이 메뉴는 그대로 됩니다).
  AltGr 로 문자를 치는 키보드 배열에서는 Ctrl+Alt 조합이 그 글자와 겹쳐 입력이 막힐 수 있습니다.
- 한국어 키보드에서는 **왼쪽** Ctrl+Alt 로 누르세요.

## 🔄 업데이트

새 버전이 나오면 보드 윗줄 아래에 알림이 뜹니다. **변경 내용** 은 [CHANGELOG.md](CHANGELOG.md) 를 열고,
**업데이트** 를 누르면 새 버전으로 바꿔 끼우고 보드가 다시 열립니다. 실패하면 보드는 그대로 두고 다시 시도 단추를 보이며, 기록은 `%USERPROFILE%\.claude\notifier\update.log` 에 남습니다.
위 설치 명령을 다시 실행해도 같습니다. hook 등록은 다시 묻지 않습니다.

새 버전은 보드를 켤 때와 그 뒤 1시간마다 확인합니다. 바로 확인하려면 윗줄의 업데이트 확인 단추(⟳)나 트레이 아이콘 오른쪽 클릭 > **업데이트 확인…** 을 누릅니다.

특정 버전을 설치하려면:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/install.ps1))) -Version 2.12
```

## 🗑️ 제거

```powershell
irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/uninstall.ps1 | iex
```

앱, `%USERPROFILE%\.claude\notifier\` 의 hook 실행 파일 · 세션 기록, settings.json 의 hook 과 환경변수(백업 후), 로그인 자동 실행,
`claude-board` · `claude-board-tui` 명령, 계정 전환용으로 저장한 로그인 정보를 지웁니다. 지금 Claude Code 로그인은 그대로 둡니다.
setup.exe 로 설치했다면 **설정 > 앱 > 설치된 앱** 에서 Claude Board 를 제거해도 같습니다.

## 🍎 macOS 판과 다른 점

| | macOS | Windows |
|---|---|---|
| hook | python3 스크립트 | 실행 파일 하나(`claude-board-hook.exe`) — 셸을 거치지 않는 형식(command + args)으로 등록 |
| 상태 아이콘 | 메뉴 막대 | 작업 표시줄 알림 영역(트레이) |
| 보드 단축키 | ⌃⌥B | Ctrl+Alt+B (터미널 펼치기 · 접기는 Ctrl+Alt+T) |
| 내장 터미널 | 보드 탭 여러 개 · 화면 나누기 · 세션 연결 · 탭 복원 | 탭 · 경로 줄 · 새 세션 창 · 이름 바꾸기는 있음. 화면 나누기 · 세션 연결 칩 · 탭 복원 · 일시중지/재개/중단 · 글꼴 크기 · 테마 · 마크다운 꾸미기는 아직 없음 |
| 세션으로 이동 · 이어 열기 | Ghostty 1.3 이상 | 콘솔 창 + Windows Terminal 탭(최선 노력), 이어 열기는 기본 터미널의 새 창 — 셸 없이 claude 를 바로 띄우므로 끝나면 창이 닫힙니다 |
| 일시중지 · 재개 · 중단 · 자동 전환 뒤 이어 가기 | Ghostty 에 키 입력 | 세션 콘솔에 키 입력 넣기(최선 노력) |
| 저장한 계정 토큰 | 키체인 | DPAPI 로 암호화한 파일 — 이 PC 의 이 Windows 사용자만 풀 수 있습니다 |
| 지금 로그인 | 키체인 | `%USERPROFILE%\.claude\.credentials.json` |
| 로그인 자동 실행 | LaunchAgent | 레지스트리 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| 터미널과 함께 띄우기 | Ghostty 를 열 때(`~/.zshrc` 줄) | Windows Terminal 이 새로 뜨면 숨은 보드를 다시 보이기 |
| 앱 확인 | 개인 인증서 서명 확인 | 코드 서명 없음 — 설치 스크립트가 SHA256 으로 확인, setup.exe 는 SmartScreen 경고 |
| 새 버전 확인 | 최신(Latest) 릴리스 | 릴리스 목록에서 `windows-v` 로 시작하는 태그 |
| 터미널 독 | Ghostty · iTerm2 · 터미널 · Orca | Windows Terminal · PowerShell 7 · Windows PowerShell · 명령 프롬프트 · Git Bash |

## 🛠️ 배포 (관리자)

1. [CHANGELOG.md](CHANGELOG.md) 맨 위에 `## <버전> — <날짜>` 항목을 씁니다.
2. `windows/packaging/release.sh <버전>` — 그 항목이 있는지 확인하고 `windows/VERSION` 을 그 버전으로 바꿉니다.
   README(최상위 · 이 문서) · 설치 가이드의 setup.exe · 릴리스 링크(`windows-v<버전>`, `ClaudeBoard-<버전>-setup.exe`)도 새 버전으로 바꿉니다.
3. (권장) 브랜치에서 릴리스 과정을 미리 확인합니다 — 아무것도 만들지 않는 dry run 입니다.

   ```sh
   gh workflow run windows.yml --ref <브랜치> -f dry_run=true
   ```

   빌드 뒤 릴리스 작업이 버전 · 태그 존재 · CHANGELOG 항목 · 자산 목록(zip 안의 파일과 SHA256 포함)을 모두 확인하고, 실행했을 `gh release create … --latest=false --target <커밋>` 명령만 보여 줍니다.
   push 할 때마다 빌드 작업도 그 버전의 CHANGELOG 항목을 찾아 보여 주고, 없으면 실패합니다.
4. commit 해 main 에 push 하면 `.github/workflows/windows.yml` 이 빌드 · 테스트한 뒤 `windows-v<버전>` 릴리스를 만듭니다.
   릴리스 설명은 CHANGELOG 의 그 버전 항목이고, 항목이 없으면 실패합니다. 태그가 이미 있으면 건너뜁니다.
   main 에서 `-f dry_run=false` 로 손으로 돌려도 실제 릴리스를 만듭니다(다른 브랜치에서는 거부합니다).

> ⚠️ Windows 릴리스는 **절대 Latest 로 표시하지 않습니다.** 설치된 macOS 앱이 `releases/latest/download/...` 로 업데이트를 받기 때문에,
> Windows 릴리스가 Latest 가 되면 macOS 업데이트가 깨집니다. GitHub 화면에서 릴리스를 고칠 때도 "Set as the latest release" 를 켜지 마세요.

릴리스 자산은 `ClaudeBoard-windows-x64.zip`(+ `.sha256`), `ClaudeBoard-<버전>-setup.exe`, `install.ps1`, `uninstall.ps1`, `VERSION` 입니다.
구조와 macOS 판과의 대응은 [docs/SPEC.md](docs/SPEC.md) 에 있습니다.

## 📄 라이선스

[Apache License 2.0](../LICENSE) · Copyright 2026 suhojang

Claude Board 는 개인이 만든 비공식 도구로, Anthropic 이 만들거나 보증하지 않습니다. Claude 와 Claude Code 는 Anthropic 의 상표입니다.
