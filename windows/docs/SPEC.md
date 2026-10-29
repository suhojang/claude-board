# Claude Board for Windows — 역분석 명세와 재구현 의무 대응표

macOS 판(저장소 최상위의 Swift·Python 소스, 커밋 `68ffed2`)을 역분석해 Windows 판이 지켜야 할 동작을 정리한 문서다.
Windows 판 개발자(사람·에이전트)는 이 문서를 단일 기준으로 삼는다. macOS 소스의 줄 번호는 `68ffed2` 기준이다.
2.13 에서 더한 규칙(Claude Code 실시간 상태 덮기·세션 이름·since·인자 중복 제거)은 그 뒤 macOS 3.0~3.1.8 의 `state.py`·`notify.py`·`board-tui.py`·`ClaudeBoard.swift` 에서 왔고, 줄 번호 대신 함수 이름으로 가리킨다.
다음 판(초안)이 더한 규칙(탭 저장·복원, 끌 때 확인, 앱 쪽 답변 꾸미기, 보드로 옮기기, 알림에서 세션으로 이동, 경로 줄 밀기, 맨 위 고정·투명도, 서브에이전트 시작 표지)은 macOS 3.1.8~3.2.0 의 같은 이름 함수(`saveTabs`·`restoreTabs`·`resumeHeld`·`confirmShutdown`·`mdRenderDir`·`mdRenderCommand`·`moveToBoard`·`Sub.parse`)에서 왔다.
2단계(설정 화면, 단축키 바꾸기, 새 세션 기본값, 로그인할 때 열기, 설정 안 계정 추가)는 macOS 3.2.0 의 `SettingsScreen`·`SettingsItem`·`Shortcuts`·`KeyCombo`·`ShortcutSettings`·`LoginItem`·`NotifyPositionPicker`·`LoginWebWindow`·`loginURL`·`loginNote`·`confirmRemove` 에서 왔고 §5.14 가 소유한다.

## 0. 결정 사항

| 항목 | 결정 |
|---|---|
| 위치 | 저장소의 `windows/` 아래에만 둔다. 최상위의 macOS 파일은 고치지 않는다(문서 링크·`.gitignore`·`CLAUDE.md` 제외) |
| 기술 | Rust. 보드·알림 패널은 Tauri 2(프론트엔드 Svelte 5 + TypeScript + Vite), hook·명령줄 도구는 순수 Rust 실행 파일 |
| 버전 | macOS 와 같은 번호를 쓴다. 첫 판은 `2.12`. 버전은 `windows/VERSION` 한 곳에 두고 빌드가 읽는다 |
| 릴리스 | 태그 `windows-v<버전>`, GitHub 릴리스는 **절대 Latest 로 표시하지 않는다**(`--latest=false`, API `make_latest: "false"`) |
| 범위 | 보드·알림 패널·hook·사용 한도·업데이트·트레이 + 세션 터미널 제어 + 계정 전환·자동 전환 + 캐릭터 8종 전체 + TUI + 내장 터미널(탭·탭 저장과 복원·끌 때 확인·세션 짝짓기·구동·보드로 옮기기·답변 꾸미기·크기 바꾸기 장벽, 화면 나누기는 구조만, §5.12) + 설정 화면·단축키 바꾸기·새 세션 기본값·로그인할 때 열기·설정 안 계정 추가(§5.14) |
| 터미널 화면 | xterm.js 6.0.0 + `@xterm/addon-webgl` 0.19.0 + `@xterm/addon-fit` 0.11.0 + `@xterm/addon-unicode11` 0.9.0. 네 개 모두 정확한 버전으로 고정한다(`package-lock.json` 포함). 소스를 패치하지 않고 6.1 베타로도 올리지 않는다 |
| 터미널 셸 연결 | 자체 ConPTY 래퍼(`windows-sys` 로 쓴 얇은 층). 지금은 시스템 ConPTY(kernel32)를 쓰고 sideload `conpty.dll` 은 쓰지 않는다. 나중에 갈아 끼울 수 있게 `ConptyApi { create, resize, close }` 함수 표 한 곳에서만 ConPTY 를 부른다 |
| 터미널 수명 | Job 객체를 쓰지 않는다. 보드가 꺼지면 콘솔 끊김에 맡긴다(보드 쪽 파이프 끝은 상속 불가, §5.12) |
| 문서 | 문서·코드 주석·커밋 메시지는 한국어(저장소 관례) |

### 0.1 Latest 를 쓰지 않는 이유 (어기면 기존 macOS 사용자가 깨진다)

설치된 모든 macOS 앱이 `https://github.com/suhojang/claude-board/releases/latest/download/VERSION`·`ClaudeBoard.zip`·`install.sh` 를 읽는다
(`ClaudeBoard.swift Updater`, `packaging/install.sh`). Windows 릴리스가 한 번이라도 Latest 가 되면 macOS 업데이트 확인·설치가 404 나 엉뚱한 버전을 받는다.
그래서 Windows 판은 `/releases/latest/` 를 쓰지 않고 **릴리스 목록 API 에서 `windows-v` 로 시작하는 태그를 골라** 쓴다.
macOS 의 `packaging/release.sh` 는 최상위 `CHANGELOG.md` 를 `## <버전>` 으로 찾으므로, Windows 변경 내역은 `windows/CHANGELOG.md` 에 따로 쓴다.

## 1. 디렉터리와 산출물

```
windows/
  VERSION                     버전의 단일 출처
  CHANGELOG.md                Windows 판 변경 내역. "## <버전> — <날짜>" 항목이 릴리스 설명이 된다
  README.md                   Windows 판 소개·설치·사용법(사용자용)
  docs/SPEC.md                이 문서
  docs/설치 가이드.md          packaging/README.txt 에 해당 — 모든 기능의 자세한 설명
  Cargo.toml                  워크스페이스: crates/core, crates/hook (Cargo.lock 은 windows/ 에)
  crates/core/                claude-board-core — 파일 약속·hook 상태 규칙·hook 등록·경로·Windows 프로세스 도구(라이브러리)
  crates/hook/                claude-board-hook — hook·설치·TUI 명령줄 실행 파일
  app/                        Tauri 앱(프론트엔드 package.json·src/ 는 여기)
  app/src-tauri/              Tauri 백엔드(별도 Cargo.lock) — 보드 상태 읽기·사용 한도·계정·업데이트·터미널 제어·트레이·창
                              내장 터미널: src/pty/(ConPTY 래퍼·환경 블록)·src/ring.rs(출력 링 버퍼)·src/terms.rs(명령·`TermHub`)·src/terms/pairing.rs(세션 짝짓기 어댑터)
                              세션 동작: src/session_action.rs(동작 이름)·src/drive.rs(구동)·src/open_session.rs(세션 열기 요청)·src/terminal.rs(`perform`)
                              수명 주기: src/shutdown.rs(끌 때 확인 우편함·`.paused` 표지·탭 목록 저장)·src/proc_wait.rs(프로세스 생사·끝나기 기다리기)
                              답변 꾸미기·시험: src/mdrender.rs(플러그인 옮겨 쓰기)·src/test_claude.rs(시험용 claude 바꿔치기)
  app/src/lib/terminal/       내장 터미널 프론트엔드(hub.svelte.ts·term.ts·resizeBarrier.ts·place.ts·sessionFlow.svelte.ts·TerminalAreaView·TerminalTabBar·TerminalPane·PaneDivider)
                              수명 주기: restore.ts(탭 저장·복원 규칙)·shutdownList.ts(끌 때 확인 목록 규칙)·lifecycle.svelte.ts(실행)·ShutdownList.svelte(확인 창)
                              답변 꾸미기·시험: mdRender.ts·mdRenderLoad.ts·testClaude.ts, 경로 줄·탭 줄 밀기 stripScroll.ts
  app/src/lib/board/          보드 화면. 세션 줄 클릭 표 `openSession.ts`·`clickGate.ts`, 위치 칩 `placeChip.ts`, 보드로 옮기기 `moveToBoard.ts`·`sessionActions.ts`, 투명 규칙 `opacity.ts`
  ci/gui/                     격리한 보드로 GUI 를 시험하는 도구(launch-board.ps1·tripwire.ps1·cdp.mjs, §5.13)
  ci/jev/                     Jev 시험 협업 도구(§5.13)
  ci/fake-claude-tty.cjs      키를 받아 적는 가짜 claude. fake-claude-login.cjs 는 가짜 로그인
  crates/core/src/place.rs    세션과 보드 터미널 짝짓기 순수 규칙(`Host`·`ProcessView`·`place`)
  packaging/install.ps1       설치·업데이트 스크립트(업데이트 단추도 이것을 실행)
  packaging/uninstall.ps1     제거 스크립트
.github/workflows/windows.yml Windows CI·릴리스
```

### 1.1 실행 파일 이름 (바꾸면 hook·설치·업데이트가 깨진다)

| 파일 | 역할 |
|---|---|
| `ClaudeBoard.exe` | Tauri 앱. 인자 없음 = 보드. `--notify …` = 알림 패널 하나만 띄우고 끝나는 별도 프로세스. `--background`·`--show`·`--action <동작> <세션 id>` |
| `claude-board-hook.exe` | 콘솔 프로그램. `state <working\|waiting\|idle\|ended\|subagent>`, `notify <stop\|notification>`, `setup --app <ClaudeBoard.exe 경로>`, `uninstall`, `install-hooks`, `uninstall-hooks`, `tui` |

### 1.2 설치 위치

| 경로 | 내용 |
|---|---|
| `%LOCALAPPDATA%\Programs\Claude Board\` | `ClaudeBoard.exe`, `claude-board-hook.exe`(원본), `LICENSE` — install.ps1 로 설치한 경우 |
| `%LOCALAPPDATA%\Claude Board\` | setup.exe(NSIS, 사용자 단위)로 설치한 경우 — 위 파일과 `uninstall.exe`, `HKCU\Software\claude\Claude Board`·설정 > 앱 항목. install.ps1 업데이트는 `app-path` 를 따라 이 자리를 바꿔 끼운다 |
| `%USERPROFILE%\.claude\notifier\` | macOS 와 같은 폴더. `claude-board-hook.exe`(hook 이 실행하는 사본), `app-path`(ClaudeBoard.exe 전체 경로 한 줄), `sessions\`, `mute-all`, `notify-position`, `accounts.json`, 깨진 목록을 옮겨 둔 `accounts.json.broken-<yyyyMMdd-HHmmss>` 파일들(이름이 있으면 뒤에 `-2`·`-3`, §5.5), `accounts\`, `update.log`, `auto-switch.log`, `board.log`, `board-theme`, `context\`(사용자 상태줄이 씀), `context-windows.json`, `plugins\md-render\`(보드가 시작할 때 설치 폴더에서 옮겨 쓴 사본, §5.12.11) |
| `%USERPROFILE%\.local\bin\` | `claude-board.cmd`(보드 열기), `claude-board-tui.cmd`(TUI) — Claude Code 네이티브 설치가 이 폴더를 PATH 에 넣는다 |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 값 `ClaudeBoard` | `"<설치 경로>\ClaudeBoard.exe" --background` — 로그인 자동 실행(macOS LaunchAgent 대응) |
| Tauri app config dir(`%APPDATA%\com.claude.board\`) | 보드 설정 `board-settings.json`(§5.9) |

hook 사본을 바꿀 때는 실행 중인 exe 를 덮어쓸 수 없으므로 `claude-board-hook.exe` → `claude-board-hook.old-<시각>.exe` 로 이름을 바꾼 뒤 새 파일을 복사하고, 다음 setup 때 지난 `.old-*.exe` 를 지운다.

## 2. 파일로 주고받는 약속 (macOS 와 같게)

경로 기준은 `%USERPROFILE%\.claude\notifier\`(= `~/.claude/notifier/`). 필드 이름·뜻은 macOS 와 **같다**. 다른 점만 따로 적는다.

### 2.1 `sessions/<session_id>.json` (hook 이 쓰고 보드·TUI·notify 가 읽는다)

| 필드 | 뜻 | Windows |
|---|---|---|
| `session_id`, `cwd`, `project`(= cwd 의 마지막 이름) | | `project` 는 `\`·`/` 모두 구분자로 본다 |
| `project_dir` | claude 를 띄운 프로젝트 폴더(hook 환경 변수 `CLAUDE_PROJECT_DIR`). `cwd` 는 Claude 가 하위 폴더로 cd 하거나 워크트리에 들어가면 바뀌지만 이 값은 그대로다 — Claude Code 2.1.304 실측: hook 입력 `cwd` 가 `repo` → `repo/sub` → `repo/.claude/worktrees/t1` 로 바뀌는 동안 `CLAUDE_PROJECT_DIR` 은 끝까지 `repo`. 보드·TUI 는 이것으로 카드를 묶고, 이 필드가 없는 예전 기록은 `cwd` 로 묶는다 | 같음 |
| `state` | `working`·`waiting`·`idle`·`ended` | 같음 |
| `since` | state 가 마지막으로 바뀐 시각 또는 새 요청(`UserPromptSubmit`)이 들어온 시각(초, 소수) | 같음 |
| `updated` | 마지막 hook 시각 | 같음 |
| `started` | 프로세스 정보를 새로 잡은 시각 | 같음 |
| `pid` | 조상 가운데 Claude Code 프로세스 pid | `claude.exe`(또는 명령줄에 `@anthropic-ai/claude-code` 가 든 `node.exe`/`bun.exe`, §3.4) |
| `pid_ctime` | — | **Windows 전용 추가**: 그 프로세스의 생성 시각(FILETIME 을 유닉스 초로). pid 재사용을 가려낸다. 없으면 pid 만 본다 |
| `tty` | 터미널 장치 이름 | Windows 에는 없다 → 늘 `null` |
| `headless` | 터미널 없이 도는 세션 | §3.4 의 판별 규칙 |
| `args` | 다시 열 때 넘길 실행 인자(`carry_args`) | 명령줄을 `NtQueryInformationProcess(ProcessCommandLineInformation)` 로 읽어 `CommandLineToArgvW` 로 나눈 뒤 같은 규칙 |
| `transcript_path`, `permission_mode`, `model` | hook 입력에서 | 같음(서브에이전트 이벤트는 permission_mode·model 을 바꾸지 않는다) |
| `topic` | 마지막 요청 문구 — 공백을 한 칸으로 줄이고 앞 **40 코드포인트**, `<` 로 시작하는 요청은 무시 | 같음 |
| `last_message` | idle 이면 `last_assistant_message`(공백 정리, 200 코드포인트), waiting 이면 `message` 앞 200(공백 정리 없음) | 같음 |
| `subagents` | `{agent_id: {type, description?, background?, started, started_seen, state(running\|waiting\|done), tool?, updated, ended?, last_message?}}` | 같음. `started_seen` 은 `SubagentStart` hook 을 본 기록만 참이다(아래 §3.2). 이 표지가 생기기 전에 쓰인 기록에는 키가 없다 |
| `state_before_sub_wait` | 서브에이전트 승인 대기 전 상태 | 같음 |
| `limit_hit` | `{at, message}` — 한도에 걸려 끊김 | 같음 |
| `resume_cmd` | 다시 여는 명령 문자열 | **표시용**. Windows 명령줄 규칙으로 인용(예: `claude --model claude-opus-5-5 --resume <id>`). 이 문자열을 셸(cmd·PowerShell)로 실행하지 않는다 |
| `resume_args` | — | **Windows 전용 추가**: `claude` 뒤에 붙일 인자 배열(`[..., "--resume", "<id>"]`). 보드는 이 배열의 허용 글자만 든 인자를 골라 보드 탭의 셸에 이어 열기 명령으로 친다(§5.7). 모델 이름 같은 값은 저장소의 설정 파일에서 올 수 있어 셸 해석을 거치면 명령 주입이 되므로, 허용 밖의 인자가 하나라도 있으면 모두 버리고 `claude --resume <id>` 만 친다 |

쓰기: 같은 폴더에 `<sid>.<임의>.tmp` 로 쓰고 바꿔 넣는다(Windows 는 `MoveFileExW(REPLACE_EXISTING)`, 공유 위반이면 50ms 간격 5번 다시). 읽고 고쳐 쓰는 동안 `.<sid>.lock` 을 `LockFileEx` 배타 잠금(macOS `flock`).

### 2.2 표지 파일

| 파일 | 쓰는 쪽 → 읽는 쪽 | 뜻 |
|---|---|---|
| `sessions/<sid>.mute` | 보드 → notify | 그 세션 알림 끔 |
| `mute-all` | 보드 → notify | 전체 알림 끔(세션별 표지보다 먼저 본다) |
| `notify-position` | 보드 → notify | `top-left`·`top-right`·`bottom-left`·`bottom-right`. 없으면 가운데(`center` 를 고르면 파일을 지운다) |
| `sessions/<sid>.paused` | 보드 → state hook | 보드에서 일시중지. 내용 `{"paused_at":…, "topic":…}`. `UserPromptSubmit` 이 오면 hook 이 지운다. 보드를 끌 때 확인에서 끝내기를 고르면 끊기는 세션에도 남기고(§5.12.10), 이어 열기가 끝나면(`reopen_done`) 보드가 지운다 |
| `login-launch-off` | 보드(설정 > 일반) → setup | 로그인할 때 열기를 끈 표지. 있으면 `setup` 이 Run 값을 다시 쓰지 않고 지운다. `uninstall` 이 이 파일을 지운다(§5.14.5) |
| `app-path` | setup → notify | **Windows 전용**: ClaudeBoard.exe 전체 경로 |
| `context/<sid>.json` | 사용자 상태줄 → 보드 | `{context_window:{context_window_size, current_usage:{input_tokens,cache_creation_input_tokens,cache_read_input_tokens}, used_percentage}, model, effort, at}` |
| `%TEMP%\.claude-usage-cache.json` | 보드(·상태줄) | 사용량 API 응답 캐시(macOS 는 `/tmp/.claude-usage-cache.json`) |

### 2.3 `~/.claude/sessions/<pid>.json` (Claude Code 가 쓰고, 우리는 읽기만 한다)

경로 `%USERPROFILE%\.claude\sessions\<pid>.json`, `<pid>` 는 세션 파일(§2.1)의 `pid`. 이 폴더는 `notifier\sessions\` 와 다른 폴더다.
같은 폴더에 `<pid>.<해시>.key` 같은 파일도 있으므로 정확히 `<pid>.json` 만 연다. Windows 의 Claude Code 2.1.296 이 아래 필드를 쓴다.

| 필드 | 뜻 | 쓰는 곳 |
|---|---|---|
| `sessionId` | 이 pid 가 지금 도는 세션 id | 세션 파일의 `session_id` 와 같을 때만 아래 값을 믿는다 |
| `status` | `idle`·`busy`·`waiting` | §3.9 의 상태 덮기 |
| `statusUpdatedAt` | `status` 가 바뀐 시각(유닉스 밀리초, 숫자) | §3.9. 읽을 때 1000 으로 나눠 초로 쓴다 |
| `name` | 세션 이름(`/rename`·`--name`). 비어 있을 수 있다 | 보드의 세션 이름 라벨(§7) |
| `pid` | Claude Code 프로세스 | macOS·Windows 보드: 세션 파일에 `pid` 가 없는 끝나지 않은 세션의 생사·상태 덮기(§3.9)·세션 이름을 판정할 때 `sessionId` 로 파일을 찾아 써 본다(§5.1) |

파일이 없거나, JSON 객체가 아니거나, pid 가 0 이면 "정보 없음"으로 본다. 읽기 실패는 오류로 올리지 않는다. 이 파일을 쓰거나 지우지 않는다.
hook 이 기록하는 `pid`(`proc::find_claude`, §3.4)는 이 파일 이름의 pid 와 같고, 기록의 `pid_ctime`(유닉스 초)은 `procStart`(FILETIME 100ns 틱, 1601년 기준)를 `(procStart − 116444736000000000) / 1e7` 로 바꾼 값과 같다. 실제 Claude Code 2.1.296 세션 안에서 확인했다(§11.1).

## 3. hook — `claude-board-hook.exe`

hook 규칙(§3.1·§3.2·§3.3·§3.5·§3.6)의 기준 시험은 저장소 맨 위 `hook-table/` 이다. macOS hook 을 끝까지 돌려 만든 표를 `crates/core/tests/hook_table.rs` 가 `state::run`·`notify::prepare`·`hooks::add` 로 견준다(표시·뺀 것·유지 규칙은 그 README). 이와 쌍을 이루는 `rules-table/` 은 disk·프로세스 없이 판단 단계만 순수하게 견주는 가벼운 표이다(`pairing.json`·`session-scan.json`·`autoswitch.json`·`usage.json`·`sessions-group.json`).

### 3.1 등록 형식 (exec form — 셸을 거치지 않는다)

Claude Code 문서(hooks): `args` 가 있으면 `command` 를 실행 파일로 보고 셸 없이 `args` 를 인자로 넘긴다. Windows 에서는 `command` 가 실제 `.exe` 여야 한다.
명령에는 macOS `hooks.py` 와 같은 표시 `.claude/notifier/` 가 들어가야 한다 — **경로를 슬래시(`/`)로 쓴다**.

```json
{"type": "command", "command": "C:/Users/<사용자>/.claude/notifier/claude-board-hook.exe", "args": ["state", "working"], "timeout": 5}
```

이벤트 표(macOS `hooks.py` 의 `HOOKS` 와 같은 순서·같은 묶음. 도구 이벤트만 `"matcher": "*"`):

| 이벤트 | matcher | hook(순서대로) |
|---|---|---|
| SessionStart | | state idle |
| UserPromptSubmit | | state working |
| PreToolUse | `*` | state working |
| PostToolUse | `*` | state working |
| PermissionRequest | `*` | state waiting |
| Notification | | notify notification, state waiting |
| Stop | | notify stop, state idle |
| StopFailure | | state idle |
| SessionEnd | | state ended |
| SubagentStart | | state subagent |
| SubagentStop | | state subagent |

`install-hooks`: `%USERPROFILE%\.claude\settings.json` 을 `settings.json.bak-claude-board-<YYYYmmdd-HHMMSS>` 로 백업 → 명령에 `.claude/notifier/` 또는 `.claude\notifier\` 가 든 hook 을 모두 빼고(빈 묶음·이벤트도 지움) → 위 표대로 넣고 → `env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE = "1"`. JSON 이 깨져 있으면 고치지 않고 오류로 끝낸다. 들여쓰기 2칸, 끝에 줄바꿈, 임시 파일 후 바꿔 넣기.
`uninstall-hooks`: 같은 백업 후 hook 만 빼고 env 키를 지운다(env 가 비면 env 도 지움). 다른 hook·설정은 건드리지 않는다.

### 3.2 `state <상태>` — `state.py` 와 같은 규칙

stdin JSON 하나를 읽는다. 앞의 UTF-8 BOM 은 `main.rs` 의 `read_input` 이 한 번만 떼어 `state`·`notify` 가 같은 입력을 본다. 이 관용은 Windows 에만 있다 — PowerShell 5.1 의 `|` 같은 도구가 BOM 을 붙이는데, `state.py`·`notify.py` 는 BOM 을 거부한다(`json.load` 가 `Unexpected UTF-8 BOM`). `state.py` 규칙이 아니다. 어떤 오류도 Claude Code 를 막지 않게 **전체를 잡아 종료 코드 0** 으로 끝낸다. 출력은 쓰지 않는다.

1. `state == "waiting"` 이고 `notification_type == "idle_prompt"` 면 `idle` 로 바꾼다. `notification_type` 이 그 밖의 비지 않은 값인데 승인 계열(`permission_prompt`·`worker_permission_prompt`·`elicitation_dialog`·`elicitation_url_dialog`)이 아니면 아무것도 쓰지 않고 끝 — 다른 백그라운드 작업이 끝남(`agent_completed`)·입력 필요(`agent_needs_input`)·로그인 성공 같은 알림도 같은 hook 으로 온다. 종류가 없으면 승인으로 본다.
2. `session_id` 가 없으면 끝. `sessions/` 를 만든다. `idle` + `hook_event_name == "SessionStart"` 면 prune(§3.5).
3. `.<sid>.lock` 배타 잠금 아래에서 읽고 고쳐 쓴다:
   - 기록에 `pid` 가 없거나, 이벤트가 `SessionStart` 거나, 기록된 pid 가 살아 있지 않으면: Claude 프로세스를 다시 찾아 `pid`·`pid_ctime`·`tty`(null)·`headless`·`started=now`·`args` 를 새로 쓴다. 이미 있는데 `args` 만 없으면 args 만 채운다.
   - `transcript_path` 가 오면 갱신. 서브에이전트 이벤트가 아니면 `permission_mode`·`model`(빈 문자열이 아닌 문자열일 때만) 갱신.
   - `cwd = data.cwd or rec.cwd or 현재 폴더`, `project`, `session_id`, `updated=now`.
   - `project_dir = 환경 변수 CLAUDE_PROJECT_DIR(비어 있지 않으면) or rec.project_dir or cwd` — 서브에이전트 이벤트까지 모든 hook 에서, `updated` 바로 뒤 키로(state.py 와 같은 순서).
   - `prompt` 를 공백 정리 → 비어 있지 않고 `<` 로 시작하지 않고 앞 40자가 기존 topic 과 다르면 topic 교체(`topic_changed`).
   - 서브에이전트 이벤트(`agent_id` 있음): subagents 기록(`track_subagent` — `state.py:146-174` 그대로: SubagentStop 은 done+ended+last_message, 그 밖은 ended 를 지우고 waiting/running·tool 갱신, `subagents/agent-<id>.meta.json` 의 description·`requestShape=="background"` 를 처음 한 번 읽음, done 후 3600초 지난 것 삭제).
     새 기록은 `started_seen = false` 로 만들고 `SubagentStart` 가 오면 `started` 를 그 시각으로 고쳐 `started_seen = true` 로 둔다. 시작 hook 없이 끝 hook 이나 도구 hook 만 온 기록은 `started` 가 믿을 수 없는 값이라 표지가 거짓으로 남는다(mac `state.py track_subagent`).
     세션 상태는 바꾸지 않는다. 단 승인 요청(waiting)이면 세션을 waiting 으로 올리고 이전 상태를 `state_before_sub_wait` 에 둔다. 그 뒤 서브에이전트가 다시 움직이면(세션이 waiting 이고 그 키가 있으면) 이전 상태로 돌린다.
   - `ended` 면 끝나지 않은 서브에이전트를 모두 done+ended.
   - `Notification` + `notification_type == "permission_prompt"` 이고 기록 상태가 `idle` 이면 `state_before_sub_wait = "idle"`. 쉬고 있는 세션은 스스로 승인을 묻지 않으므로 서브에이전트의 승인 요청이다.
   - 서브에이전트 이벤트도 `Notification` 도 아니면 `state_before_sub_wait` 를 지운다. `Notification` 은 서브에이전트의 승인 요청이어도 `agent_id` 없이 온다(Claude Code 2.1.304 확인).
   - `UserPromptSubmit` 이면 `<sid>.paused` 를 지운다.
   - `StopFailure` + `error == "rate_limit"` 이고 `last_assistant_message`(공백 정리)가 정규식 `hit your|usage limit`(대소문자 무시)에 맞으면 `limit_hit = {at: now, message: 앞 200}`.
     서브에이전트가 아닌 `UserPromptSubmit`·`PreToolUse`·`PostToolUse`·`Stop`·`SessionStart` 면 `limit_hit` 를 지운다.
   - 상태가 바뀌었거나 이벤트가 `UserPromptSubmit` 이면 `state`·`since=now`(`state.py:update`). Esc 로 끊으면 hook 이 오지 않아 다음 요청이 working → working 인데, 그때도 시작 시각을 새로 잡는다.
     "상태가 바뀌었는가"(`changed`)는 그대로 두며, 아래 터미널 제목 조건이 이 값을 쓴다. 새 요청이 아닌 working → working(`PreToolUse` 등)은 `since` 를 바꾸지 않는다.
   - idle 이고 `last_assistant_message` 가 있으면 last_message(공백 정리, 200). waiting 이고 `message` 가 있으면 last_message = message 앞 200.
   - `resume_cmd` 를 만든다(§3.3). 원자적으로 쓴다.
   - 상태가 바뀌었거나 topic 이 바뀌었고 `headless == false` 면 터미널 제목을 `"<아이콘> <project> · <topic 앞 20자>…"` 로 바꾼다(아이콘 working 🔵, waiting 🟠, idle 🟢, ended ⚫; topic 이 20자 이하면 `…` 없이, 없으면 ` · ` 부분 없음).
     Windows: `FreeConsole` → `AttachConsole(pid)` → `SetConsoleTitleW` → `FreeConsole`. 실패는 무시. Windows Terminal(ConPTY) 탭 제목도 이것으로 바뀐다(실기 확인, §11.1).

근거: REA 프로세스 캡처 `ev_23674ebbd76cceea4252d166b60c1ad66648ee3f2a951ba5d4c669d1c07d3368` — `state.py` 에 이벤트 14개를 차례로 넣은 결과(테스트 기준 데이터, §12).

### 3.3 `carry_args`·`resume_cmd` — `state.py:57-97`

- 넘길 인자: 값 하나(`--model --permission-mode --agent --settings --plugin-dir`), 값 여럿(`--add-dir --mcp-config`, 다음 `-` 전까지), 값 없음(`--dangerously-skip-permissions --allow-dangerously-skip-permissions --chrome --ide --verbose`), `--x=값` 형태도 받는다. argv[0] 은 건너뛴다.
- 중복 제거(`state.py:carry_args`): 같은 인자는 한 번만 넘긴다. Claude Code 는 bypass 권한 모드로 이어 열 때 `--allow-dangerously-skip-permissions` 를 스스로 하나 더 붙여 띄우므로, 그대로 넘기면 이어 열 때마다 하나씩 늘어난다.
  - 값 없는 플래그는 같은 이름이 또 오면 건너뛴다.
  - 값 하나 인자는 (이름, 값) 쌍이 같을 때만 건너뛴다. `--model a --model b` 는 둘 다 남는다.
  - `--x=값` 형태는 문자열 전체가 같을 때만 건너뛴다. `--model a` 와 `--model=a` 는 서로 다른 형태라 둘 다 남는다.
  - 값 여럿 인자(`--add-dir x`)는 중복을 제거하지 않는다. `--add-dir=x` 형태는 위 규칙대로 제거한다.
- resume: args 에서 `--permission-mode`·`--model` 을 빼고 기록된 값으로 다시 붙인 뒤 `claude <args…> --resume <sid>`. Windows 인용 규칙으로 잇는다.

### 3.4 Claude 프로세스·headless 판별 (Windows)

- 조상 따라가기: `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` 한 번으로 pid→(ppid, exe 이름) 표를 만들고, hook 의 부모부터 최대 8단계 올라간다. exe 이름이 `claude.exe` 면 그 프로세스. `node.exe`·`bun.exe` 는 명령줄에 Claude Code 패키지(`@anthropic-ai/claude-code`)가 들어 있을 때만 인정한다(`proc::is_claude_exe`). `claude-board` 같은 폴더 이름에 `claude` 가 든 것만으로는 세지 않는다.
- `alive(pid)`: `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetExitCodeProcess == STILL_ACTIVE`. `pid_ctime` 이 있으면 `GetProcessTimes` 생성 시각도 같아야 산 것으로 본다. 열 권한이 없으면(ACCESS_DENIED) 산 것으로 본다.
- `headless`(macOS 는 tty 가 없으면 headless): 다음 중 하나면 true — 실행 인자에 `-p`/`--print` 가 있다 / `AttachConsole(pid)` 실패(콘솔 없음) / 콘솔 창(`GetConsoleWindow`)도 그 창의 소유 창(`GetWindow(GW_OWNER)`, ConPTY 의 가짜 창은 터미널 창을 소유자로 둔다)도 보이지 않고, 조상 가운데 보이는 최상위 창을 가진 프로세스도 없다.
  실기 확인(§11.1): Windows Terminal 탭으로 열린 새 콘솔은 콘솔 창이 숨은 `PseudoConsoleWindow`(OpenConsole.exe)이고 그 소유 창이 보이는 WT 창(`CASCADIA_HOSTING_WINDOW_CLASS`)이라 false. 콘솔 없는 프로세스(`DETACHED_PROCESS`)·`-p` 는 true. 창 없는 콘솔(`CREATE_NO_WINDOW`)은 조상에 보이는 창이 없으면 true.

### 3.5 prune — `state.py:113-135`

종료된 지 2일 넘은(파일 수정 시각) 세션 파일 가운데 pid 가 없거나 죽은 것을 지운다. `.lock`·`.paused`·`.mute` 는 짝인 `.json` 이 없으면 지운다.

### 3.6 `notify <stop|notification>` — `notify.py`

1. stdin JSON(BOM 은 `read_input` 이 뗀다. 깨졌으면 `{}`). `mute-all` 이 있으면 끝. `sessions/<sid>.mute` 가 있으면 끝. `notification` 이고 `notification_type` 이 비지 않았는데 승인 계열(§3.2 1)이 아니면(`idle_prompt` 포함) 끝.
2. notification: 제목 "승인 대기", kind `wait`, 소리 wait, 본문 `message` 또는 "Claude 가 입력을 기다리고 있습니다".
   stop: 제목 "작업 완료", kind `done`, 소리 done, 본문 `last_assistant_message` 또는 transcript 끝 400KB 에서 찾은 마지막 assistant 텍스트.
3. 본문 다듬기(`plain`·`first_sentence`): 코드 블록 제거, 인라인 코드·굵게 기호 제거, 줄머리 `#`·`-`·`*`·`1.`·`|` 제거, 공백 정리, 140자 넘으면 139자 + `…`. 그다음 첫 문장(`.!?。` 로 끝나는 가장 짧은 앞부분, 없으면 전체).
4. 요약 `waiting=<n>,working=<n>,idle=<n>`: 모든 세션 파일의 상태를 세되, 이 세션은 막 바뀐 상태(wait→waiting, done→idle)로 센다. 다른 세션은 pid 가 죽었으면 뺀다. 이 세션 파일이 아직 없으면 그 상태에 1 을 더한다.
   살아 있는 **다른** 세션의 기록 상태가 working·waiting 이면 §3.9 의 규칙으로 덮은 상태를 센다(`notify.py:session_summary`). 지금 알림을 내는 세션은 덮지 않는다 — 막 바뀐 상태를 그대로 센다.
5. 위치: `notify-position` 내용(없거나 비면 `center`).
6. `app-path` 의 ClaudeBoard.exe 를 `--notify --title <제목> --subtitle <project> --body <본문> --summary <요약> --sound <done|wait> --kind <done|wait> --duration 10 --position <위치>` 로 띄우고 **기다리지 않고** 끝낸다.
   `CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`(breakaway 가 거부되면 그 플래그 없이 다시), 표준 입출력은 닫는다. app-path 가 없거나 파일이 없으면 끝.

### 3.7 `setup --app <ClaudeBoard.exe>` / `uninstall` (macOS `user-setup.sh`·`Uninstall.command` 대응)

setup: `notifier\sessions\` 만들기 → 자기 자신을 `notifier\claude-board-hook.exe` 로 복사(§1.2 이름 바꾸기 규칙) → `app-path` 쓰기 → `install-hooks` → `%USERPROFILE%\.local\bin\claude-board.cmd`(`@start "" "<app>" %*`)·`claude-board-tui.cmd`(`@"%USERPROFILE%\.claude\notifier\claude-board-hook.exe" tui %*`) → Run 레지스트리 값(설정 > 일반에서 끈 표지 `login-launch-off` 가 있으면 쓰지 않고 지운다, §5.14.5).
uninstall: `uninstall-hooks` → Run 값 삭제 → 실행 중인 ClaudeBoard.exe 종료 → notifier 의 hook exe(들)·`app-path`·`notify-position`·`login-launch-off`·`sessions\`·`accounts.json`·`accounts.json.broken-` 로 시작하는 파일들·`accounts\`(DPAPI 금고 포함)·`plugins\`(보드가 옮겨 쓴 md-render 사본과 Claude Code 가 그 안에 만든 `tsconfig.json`·`.claude-plugin\types\`) 삭제, 빈 notifier 폴더 삭제.
`CLAUDE_BOARD_HOME`·`CLAUDE_BOARD_NOTIFIER_DIR` 로 폴더를 바꿔 띄운 실행은 `setup` 이 Run 값을 쓰지 않고("자동 실행: 건너뜀") `uninstall` 이 Run 값을 지우지 않는다(실행 중인 ClaudeBoard.exe 를 끝내는 일은 격리 여부와 무관하게 한다, §5.14.5).
`plugins\` 가 링크(junction)여도 가리키는 곳은 지우지 않고 링크만 뗀다. 환경변수 `CLAUDE_BOARD_NOTIFIER_DIR` 로 notifier 를 옮겨 둔 실행(시험·격리)에서는 `plugins\` 를 지우지 않는다(그 폴더에 보드가 옮긴 것 밖의 것이 있을 수 있다). `packaging/uninstall.ps1` 이 같은 범위·같은 조건으로 지운다(§8.2). 그다음 `.local\bin` 의 cmd 두 개를 지운다. 지금 Claude Code 로그인(`.credentials.json`)은 건드리지 않는다. 앱 폴더 삭제는 uninstall.ps1 이 한다(실행 중인 자기 exe 는 못 지우므로).

### 3.8 `tui` — `board-tui.py` 이식

crossterm 으로 같은 화면: 프로젝트 경로별 카드(`project_dir`, 없으면 `cwd`. 테두리 색 = 가장 급한 상태), 상태별 표시(작업 중 스피너 `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, 승인 대기 `◉/○` 깜빡임, 대기 `✓`, 종료 `✕`), 경과 시간, topic, 백그라운드 표시, 승인 대기 메시지 줄, 아래 상태별 개수, 0.12초 다시 그리기·1초 다시 읽기, `q` 종료, `t` 테마 순환(4종, `notifier\board-theme` 에 저장), 한글 폭 2칸. 종료 후 30분 넘은 ended 세션 파일은 지운다(`board-tui.py:107-129`).
파일을 읽을 때(`board-tui.py:load`) pid 가 죽은 세션을 ended 로 바꾼 뒤, 30분 prune 보다 앞에서 §3.9 의 덮기를 적용한다. 덮은 세션은 `state` 와 `since` 가 바뀐 채로 그려진다.

### 3.9 Claude Code 실시간 상태 덮기 — `claude_live` (`notify.py`·`board-tui.py` 의 `interrupted_at`·`claude_status`, `ClaudeBoard.swift` 의 `InterruptReader`·`ClaudeNames`)

Claude Code 는 사용자가 Esc 로 응답을 끊으면 `Stop` hook 을 부르지 않는다. 세션 파일이 working·waiting 으로 남으므로 hook 이 오지 않는 두 경우를 Claude Code 가 남긴 기록으로 메운다.
규칙은 core 의 `live_state` 한 곳에 있고, 알림 요약(§3.6)·TUI(§3.8)·보드(§5.1)가 그 함수를 쓴다.

입력과 결과(순수 함수):
- 기록된 `state` 가 `working`·`waiting` 일 때만 본다. `idle`·`ended`·`paused` 면 아래 두 조회를 하지 않는다.
- **중단 시각**(`interrupted_at`): 세션의 `transcript_path` 가 있으면 대화 기록의 끝을 본다. `transcript_path` 가 비어 있으면 중단 없음(보드는 §5.1 의 transcript 경로로 대신한다).
- **status 변화**(`status_change`): §2.3 의 파일에서 `sessionId` 가 이 세션의 id(비어 있지 않음)와 같고, `status` 가 있고, `statusUpdatedAt/1000` 이 세션 `since`(없으면 0)보다 **클 때만** `(status, 바뀐 시각)`.
- 중단 시각이 있거나(없을 때 `status == "idle"` 이면 그 바뀐 시각) → `idle`, `since` = 그 시각. 중단이 busy 보다 먼저다.
- 위가 아니고 기록 상태가 `waiting` 이며 `status == "busy"` 면 → `working`, `since` = 바뀐 시각. 승인한 도구가 도는 동안은 `PostToolUse` 까지 hook 이 없어 승인 대기로 남기 때문이다.
- 그 밖에는 기록 그대로.
- 첫 응답이 나오기 전에 Esc 로 끊으면 대화 기록에 중단 표시가 남지 않는다. 이 경우를 `status == "idle"` 이 메운다.

`interrupted_at_in(tail, mtime)` — 대화 기록 끝 256,000 바이트(`INTERRUPT_TAIL`) 이하와 파일 수정 시각(유닉스 초)을 받아 끊은 시각(유닉스 초) 또는 없음:
1. 끝 조각에 바이트열 `[Request interrupted by user`(`INTERRUPT_MARK`, "… for tool use]" 변형 포함)가 없으면 JSON 을 풀지 않고 없음.
2. `\n` 으로 나눠 **뒤에서부터** 본다. 줄 끝 `\r` 와 줄 앞 BOM 을 떼고, JSON 으로 풀리지 않는 줄(256,000 경계에서 잘린 첫 줄·UTF-8 이 아닌 줄 포함)은 건너뛴다.
3. `type` 이 `user`·`assistant` 가 아니거나 `isSidechain`·`isMeta` 가 `true` 인 줄은 건너뛴다. 중단 뒤에 붙는 cost-state·last-prompt 줄이 여기에 든다.
4. 글(`text`): `message.content` 가 문자열이면 그것, 목록이면 첫 원소의 `text`. `message` 가 객체가 아니거나 글이 없으면 `""`.
5. `user` 이고 글이 `<` 로 시작하면 건너뛴다(`!` 명령·슬래시 명령 출력).
6. `user` 이고 글이 `[Request interrupted by user` 로 시작하면 `timestamp`(ISO 8601, `Z`)를 풀어 돌려준다. 없거나 못 풀면 파일 수정 시각.
7. 그 밖의 첫 대화 줄(assistant 줄·일반 user 줄·`tool_result` user 줄)이면 없음.
`interrupted_at(path)` 는 파일 끝 조각과 수정 시각을 읽어 위를 부르는 포장이다. 파일이 없거나 못 읽으면 없음.

## 4. 알림 패널 — `ClaudeBoard.exe --notify …` (`ClaudeNotifier.swift`)

- 인자: `--title --subtitle --body --kind done|wait --summary waiting=1,working=2,idle=3 --sound done|wait|none --duration 10 --position center|top-left|top-right|bottom-left|bottom-right [--session <세션 id>]`. 같은 키를 다시 주면 뒤의 값. `--session` 은 hook 이 세션 id 를 알 때만 붙는다(`crates/core/src/notify.rs panel_args`).
- 이 모드는 단일 실행 잠금을 잡지 않는 별도 프로세스다. 패널 하나를 보이고 닫히면 프로세스가 끝난다.
- 창: 테두리 없음, 투명 배경 위 둥근 카드(반경 14), 폭 440, 높이는 내용에 맞춤. **포커스를 가져가지 않는다** — `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`, 보일 때 `SW_SHOWNOACTIVATE`. 작업 표시줄에 나타나지 않는다. 실기 확인(§11.1): 띄우기 전후 포그라운드 창이 같고, 확장 스타일에 NOACTIVATE·TOOLWINDOW·TOPMOST 가 켜지고 APPWINDOW 는 꺼져 있다.
- 위치: 마우스가 있는 모니터의 작업 영역. 가운데: 슬롯 0~4 의 가로 이동 `[0, 0.5, -0.5, 1, -1] × 폭`. 모서리: 여백 12, 같은 모서리에 떠 있는 패널의 세로 범위를 피해 간격 8 로 쌓는다(위 모서리는 아래로, 아래 모서리는 위로). 슬롯 파일 `%TEMP%\claude-notifier-slots\<0..4>`(`CreateNew` 로 잡음, 120초 넘게 갱신 없는 것은 버림, 내용 `"<position> <top> <bottom>"`), 범위를 읽고 쓰는 동안 `lock` 파일 잠금, 1초마다 수정 시각 갱신, 닫을 때 지움(`ClaudeNotifier.swift:54-118, 223-250`).
- 내용(`ClaudeNotifier.swift:280-420`, 스크린샷 `docs/images/notify-wait.png`·`notify-done.png`): 1줄 = 색 배지(30×30, 아이콘 wait ✋ / done ✓) · `<subtitle>` 흰 굵게 + `<title>` 강조색 · 오른쪽 `HH:mm`. 2줄 = 본문(최대 2줄, 흰 82%). 3줄 = 상태별 개수(색 점 + "승인 대기 n"·"작업 중 n"·"대기 n", 0 은 숨김). 맨 아래 2px 진행 막대(남은 시간).
  강조색 wait `rgb(255,168,64)`, done `rgb(77,217,140)`. 바탕은 보드와 같은 남색 그라데이션 + 왼쪽 위 강조색 빛 + 강조색 35% 테두리.
- 동작: 0.28초 동안 14px 아래에서 올라오며 나타남, 소리(done → 시스템 알림음, wait → 시스템 경고음; `PlaySoundW` 별칭, 실패 무시), 마우스를 올리면 카운트다운 멈춤, 누르면 보드를 띄우고 닫힘(`--session` 이 없으면 `ClaudeBoard.exe --show`, 있으면 `ClaudeBoard.exe --action focus <세션 id>`: 보드 탭의 세션은 그 탭을 고르고 다른 터미널의 세션은 그 창으로 간다, §5.12.12), 시간이 다 되면 0.25초 동안 흐려지며 10px 올라가고 끝.

## 5. 보드 백엔드 (`app/src-tauri`)

### 5.1 세션 읽기 — `Store.reload` (`ClaudeBoard.swift Store.reload`)

1초마다(그리고 표지 파일을 바꾼 직후 바로) `sessions/*.json` 을 읽는다. 파일 수정 시각이 같으면 이전에 푼 값을 쓴다.

- 상태 보정: ended 가 아닌데 pid 가 죽었으면 `ended`.
  세션 파일에 `pid` 가 없으면(hook 이 Claude 프로세스를 못 찾음) 끝나지 않은 세션은 `~/.claude/sessions/*.json`(§2.3) 가운데 `sessionId` 가 세션 id 와 같은 것(둘 이상이면 가장 최근에 고쳐 쓴 것)의 pid 로 같은 판정을 한다. 그런 파일이 없으면 그대로 둔다(끝났다고 단정하지 않는다: `claude -p` 처럼 그 파일을 쓰지 않는 세션). 끝난 세션은 찾지 않는다(mac 규칙과 같다).
  찾은 pid 는 상태 덮기(§3.9)·세션 이름에도 쓰지만, 세션의 `pid` 값(구동·중단·끝나기 기다리기·탭 짝짓기·백그라운드 셸 수 대상)은 세션 파일 그대로 둔다(`BoardRules.scanSessions`). 그래서 SessionEnd 없이 죽은 세션도 목록에서 끝남으로 보인다. pid 가 없는 살아 있는 세션은 백그라운드 셸 수를 세지 않는다(macOS 는 찾은 pid 로 셈: 다른 한계).
- `transcript = transcript_path` 또는 `~/.claude/projects/<cwd 의 영숫자 밖 문자를 - 로>/<sid>.jsonl`. ended 이고 (`updated` 가 2일 넘었거나 transcript 파일이 없으면) 세션 파일을 지우고 뺀다.
- Claude Code 실시간 상태 덮기(§3.9): 위 종료 판정과 prune 뒤, 아래 셸·서브에이전트로 working 을 올리는 단계와 paused 판정 앞에서 적용한다(`Store.reload` 의 순서).
  기록 상태가 working·waiting 일 때만 `live_state` 를 부르고, 덮이면 `state` 와 `since` 가 바뀐다. 세션 id 는 파일 이름으로 대신해도 된다.
  중단 시각은 위 `transcript` 경로의 파일 끝에서 읽고, 파일 수정 시각과 크기가 같으면 이전 결과를 쓴다(`InterruptReader` 대응). Claude Code 의 `sessions\<pid>.json`(§2.3)도 pid 별로 수정 시각과 크기가 같으면 다시 읽지 않는다(`ClaudeNames` 대응). 두 캐시는 목록에서 사라진 항목을 버린다.
  덮은 결과가 idle 이어도 아래 셸·서브에이전트 규칙은 그대로 적용되므로, 끊은 뒤에도 서브에이전트가 돌고 있으면 working 으로 보인다. paused 는 덮은 결과보다 우선한다.
- 서브에이전트: ended 가 아니면 `subagents` 를 풀되 끝난 지 10초 넘은 것과 `started_seen` 이 참이 아닌 것(시작 hook 을 보지 못한 기록, 키가 없는 옛 기록 포함)은 뺀다. 뺀 기록은 세션 상태를 working 으로 올리는 근거로도 세지 않는다(`store.rs parse_subs`, mac `Sub.parse`). 정렬: done 이 아닌 것 먼저, started 순. 각 `subagents/agent-<id>.jsonl` 에서 모델·effort(§5.2).
- 백그라운드 셸 수: idle 일 때만, 그 Claude 프로세스의 자식 가운데 명령줄에 `shell-snapshots` 가 든 프로세스 수(3초마다 백그라운드에서 셈). idle 인데 셸이 있거나 끝나지 않은 서브에이전트가 있으면 `working` 으로 보인다.
- `<sid>.paused` 가 있고 ended 가 아니면 `paused`. `<sid>.mute` 가 있으면 muted.
- 세션 이름: ended 가 아니면 `sessions\<pid>.json` 의 `name`(§2.3)이 비어 있지 않을 때 그 값, 아니면 없음. ended 세션에는 이름을 붙이지 않는다.
- ended 가 아니면 모델·effort: transcript 의 마지막 assistant 줄(§5.2). 상태줄 기록의 effort 가 신선하면 그것으로 덮는다.
- 컨텍스트: §5.3. topic·last_message 에서 `#*\`` 기호를 지운다.
- 정렬·묶기: `project_dir`(없거나 빈 문자열이면 `cwd`) 별 그룹 — 그룹 id·이름·경로가 이 값이다. 세션 하나의 `cwd`(와 그 이름)는 그대로 둔다. 그룹 안은 rank(waiting 0, working 1, paused·idle 2, ended 3) 오름차순, 같으면 since 최신 먼저.
  그룹 순서: 고정(pinned) 순서 → 가장 급한 rank → 가장 최근 updated.
- 지운 그룹(hiddenGroups: 그룹 id→지운 시각): 모든 세션 updated 가 그 시각 이하면 숨기고, 새 활동이 있으면 목록에서 빼고 다시 보인다.
- 조용한 카드 자동 접기: waiting·working 이 없고 마지막 updated 가 30분 넘은 그룹을 한 번 접는다(autoFolded 에 그때의 마지막 활동 시각을 적어, 사람이 다시 펴면 새 활동 전에는 또 접지 않는다).
- 새로 승인 대기가 된 세션의 그룹은 접혀 있으면 편다.
- `mute-all` 존재 여부.

### 5.2 모델·effort — `ModelReader` (`ClaudeBoard.swift ModelReader`)

파일 끝 400KB 를 거꾸로 보며 `"assistant"` 가 든 줄 가운데 `type=="assistant"`, `message.model` 이 `<synthetic>` 이 아닌 첫 줄 → `{model, effort: 줄의 effort}`. 수정 시각이 같거나 3초 안이면 다시 읽지 않는다. 없으면 세션 기록의 `model` 로 대신(effort 없음).
계열 이름: opus/sonnet/haiku/fable 이 들어 있으면 첫 글자 대문자, 아니면 `claude-` 를 뺀 이름. 버전: `-(\d+)-(\d+)` → `5.5`. effort 단계: low 1 … max 5.

### 5.3 컨텍스트 — `ContextReader` (`ClaudeBoard.swift ContextReader`)

1순위 `context/<sid>.json`: `context_window_size > 0` 이고 transcript 수정 시각이 기록의 `at` 보다 60초 넘게 늦지 않으면 used = 세 입력 토큰 합(0 이면 `used_percentage`). 그 모델의 크기를 `context-windows.json` 에 배워 둔다.
2순위 transcript 끝 1.5MB 에서 사이드체인이 아닌 마지막 assistant 의 usage 합. 크기 = 배운 값 또는 기본(`[1m]` 이나 `claude-opus-5`·`claude-sonnet-5`·`claude-haiku-5`·`claude-fable-5` 로 시작하면 1,000,000, 그 밖 200,000), used 가 크기보다 크면 1,000,000. pct = min(100, used×100/size).
reload 가 끝나면 목록에 남은 세션(끝난 것 포함)의 세션 id·transcript 몫만 남기고 세션 하나하나의 `context/<sid>.json` 캐시를 정리한다. 끝난 세션 몫이 쌓이는 것을 막는다. 배운 모델별 크기(`context-windows.json`)는 그대로 둔다.

### 5.4 사용 한도 — `UsageStore` (`ClaudeBoard.swift UsageStore`)

30초마다. 캐시 파일을 먼저 본다(mac `checkUsageCache` 규칙): 푼 캐시만 나이를 세어, 나이가 음수(수정 시각이 미래: 시계 어긋남)거나 120초 넘으면 새로 받는다. 푼 캐시는 나이와 상관없이 보이는 데 쓴다. 푸지 못한 캐시(JSON 아님·푼 한도 없음)는 나이를 세지 않고 바로 새로 받는다. 받는 중에는 새로 받지 않는다.
새로 받을 때는 `~/.claude/.credentials.json` 의 `claudeAiOauth.accessToken`(없으면 조용히 실패 표시). `expiresAt`(ms)이 지났으면 "로그인 토큰 갱신 전". `GET https://api.anthropic.com/api/oauth/usage` 헤더 `Authorization: Bearer <토큰>`, `anthropic-beta: oauth-2025-04-20`, 제한 10초.
응답: `five_hour.utilization`→"5시간"(id `5h`), `seven_day.utilization`→"주간 · 전체"(id `7d`), `limits[]` 가운데 `kind=="weekly_scoped"` 의 `percent` → "주간 · <scope.model.display_name>"(id `7d-<모델 이름>`), 각 `resets_at`(ISO8601). 모델별 한도의 이름이 같아 id 가 겹치면 뒤에 온 것에 `-2`·`-3` 을 붙인다(라벨은 그대로. id 를 뺀 항목은 차지하지 않고, `5h`·`7d` 는 늘 그대로다). 푼 한도가 하나도 없는 응답은 빈 한도로 본다: 실패로 처리해 보이던 한도를 지우지 않고 캐시에도 쓰지 않는다(지금 보이는 한도가 없으면 "사용량을 불러오지 못했습니다"). 성공(푼 한도가 하나 이상)이면 캐시에 쓴다. 계정을 바꾸면 캐시를 지우고 5초 동안 그 전 캐시는 믿지 않으며 진행 중이던 응답은 버린다.
토큰은 메모리에만 두고 기록하지 않는다. **(`.credentials.json` 의 필드 이름은 문서에 없다 — 없으면 "사용량을 불러오지 못했습니다" 로 조용히 내려간다)**

### 5.5 계정 전환 — `AccountStore` (`ClaudeBoard.swift AccountStore`)

macOS 키체인 대신:
- 지금 로그인: `~/.claude/.credentials.json`(JSON, `claudeAiOauth` 와 그 밖의 키 — `mcpOAuth` 등은 그대로 둔다). 계정 정보: `~/.claude.json` 의 `oauthAccount`(3초마다 수정 시각이 바뀌면 다시 읽음).
- 저장한 계정 토큰: `notifier\accounts\vault\<계정 id 의 sha256 앞 16자>.bin` = DPAPI(`CryptProtectData`, 현재 사용자) 로 암호화한 `{"claudeAiOauth": …}`. 표시 정보는 `notifier\accounts.json`(`{"accounts":[{oauthAccount, savedAt, refreshTokenExpiresAt?}]}`, macOS 와 같은 형식).
- 계정 id = `accountUuid|organizationUuid`. 이메일·조직·요금제(`organizationType` 의 `claude_` 를 빼고 첫 글자 대문자 + `organizationRateLimitTier` 의 `_<n>x` → `5x`: 비지 않은 조각만 빈칸으로 이어 `5x`·`Max 5x`·`Pro` 의 꼴이 된다), `refreshTokenExpiresAt` 지나면 "다시 로그인 필요".
- 저장한 계정 목록이 깨져 있으면(쓰기용 해석 `writable_meta`: 객체 배열이 그대로, JSON 아님·최상위 객체 아님·`accounts` 배열 아님(null 포함)·배열에 객체 아닌 원소 섞임은 깨진 것) `accounts.json` 을 `accounts.json.broken-<지역 yyyyMMdd-HHmmss>`(같은 이름이면 `-2`·`-3`) 로 옆으로 옮긴 뒤 빈 목록에 새 항목을 더해 새로 쓴다. 전환은 막지 않는다. 옮기지 못하면 쓰지 않는다(오류 안내). 표시용 읽기는 깨진 목록을 조용히 빈 목록으로 본다.
- 전환: `~/.claude/.storage-write.lock` 폴더 잠금(mkdir, 15초 넘은 잠금은 버림, 5초 기다림 — Claude Code 와 같은 방식) 안에서 지금 계정의 최신 토큰을 저장 → 대상 토큰을 `claudeAiOauth` 에 넣어 `.credentials.json` 을 원자적으로 씀. 그다음 `~/.claude.json.lock` 잠금 안에서 `oauthAccount` 를 바꾸고, 다른 계정이면 계정별 캐시 키(`ClaudeBoard.swift AccountStore.accountCaches` 목록)를 지운다.
- 전환을 막는 규칙(save_current: 실패는 "전환하지 못했습니다: " 가 앞에 붙은 안내로 나온다): 지금 로그인 토큰이 있는데 `~/.claude.json` 의 `oauthAccount` 가 없거나 그 안의 `accountUuid` 가 없거나 정보가 글자 값이 아니면 지금 계정을 먼저 저장하지 않고 전환을 거절한다. 안내: "지금 로그인된 계정의 정보가 없어 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요". `accountUuid` 모자람은 "정보가 없어" 가 "정보가 모자라" 로 바뀐 문장이다. 단 바꾸려는 로그인의 `refreshToken` 이 지금 로그인과 같은 글자면 막지 않는다. 막으면 목록·금고·`.credentials.json` 을 모두 그대로 둔다. 로그인 후의 지금 계정 저장도 같은 규칙을 거친다.
- 계정 지우기(remove): 금고 파일을 지우고(파일이 없는 것은 지난 것으로 본다) 목록에서 뺀다. 금고 지우기가 파일 없음 밖의 오류로 실패하면 목록을 건드리지 않고 끝낸다. 돌려 주는 안내: 성공 "<이메일> 계정을 목록에서 지웠습니다", 목록이 깨져 있으면 "<이메일>: 로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 읽지 못해 목록은 그대로 두었습니다", 목록 쓰기 실패 "<이메일>: 로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 쓰지 못했습니다: <오류>", 금고 실패 "<이메일> 계정을 지우지 못했습니다: 저장한 로그인 정보를 지우지 못해 목록에 그대로 두었습니다. 잠시 뒤 다시 지워 주세요" 또는 "<이메일> 계정을 지우지 못했습니다: <오류>". 지우기는 `accounts.json.broken-*` 파일을 건드리지 않는다.
- 계정 추가: 설정 > 계정에서 한다. 보드가 쥔 ConPTY 에서 `claude auth login --claudeai [--email <이메일>]` 을 `CLAUDE_CONFIG_DIR=<notifier\accounts\login-<8자>>` 로 돌린다(주소·코드·로그인 창·접기·시험 게이트는 §5.14.6). 3초마다 그 폴더의 `.claude.json` 의 `oauthAccount` 와 `.credentials.json` 의 `claudeAiOauth` 가 생기면 저장하고 지금 계정도 저장한 뒤 폴더를 지운다. 토큰만 있고 계정 정보가 30초 넘게 없으면 실패 안내. 15분 넘으면 그만둔다. 시작할 때 남은 `login-*` 폴더를 치운다.
- 저장한 계정 지우기는 네이티브 확인 대화상자에서 **지우기** 를 골랐을 때만 한다(§5.14.6).
- 작업 중·승인 대기 세션이 있으면 전환 전에 확인 대화상자(`ClaudeBoard.swift AccountChip` 의 확인 창 문구).
- 안내 문구(notice)는 7초 뒤 사라진다. 문구는 macOS 와 같게.

### 5.6 한도 자동 전환 — `AutoSwitcher` (`ClaudeBoard.swift AutoSwitcher`)

같은 규칙(3초 주기, settle 40초, recent 6시간, 찬 계정 기록과 풀리는 시각, 다음 계정 고르기, 이어 가기 요청 문구, `auto-switch.log`). 이어 가기는 Ghostty 대신 §5.7 의 콘솔 입력으로 `Esc → 0.5초 → 문구 → 0.2초 → Enter`. `canDrive` 가 아니면 직접 이어 달라고 안내.

- 떠날 까닭과 찬 계정 기록(autoSwitchFull)은 5시간·주간(전체) 한도만으로 채운다: 모델별 주간 한도(`weekly_scoped`)는 그 모델을 쓰는 세션만 막으므로 계정을 떠나는 까닭·표시 재료에서 뺀다(mac 규칙과 같다). 가득 찼다고 보는 조건은 pct 100 이상인데 아직 풀리지 않은 것(풀리는 시각이 지금 뒤이거나 모름)이다. 풀리는 시각을 모르면 지금에서 1시간 뒤를 적어 둔다. 표시하는 퍼센트는 정수로 자른다.
- 실패 쪽 셋(mac `autoSwitchFailed`): 골라 들어가는(대상) 계정 쪽 실패는 그 계정에만 1시간 건너뛰는 표지를 적는다(다음 3초 판단이 다음 후보로 다시 시도한다). 지금 계정·시스템 쪽 실패는 지금 계정에 1시간 쉬는 표지를 적고 "사용 한도에 도달해 <이메일> 계정으로 자동 전환하려 했지만 하지 못했습니다: <까닭>. 계정이 바뀌거나 1시간이 지날 때까지 자동 전환을 쉽니다" 를 한 번 보인다. 쓰기 잠금 시간 초과라는 실패는 30초 쉬는 표지만 적고 알리지 않는다.
- 떠날 까닭이 있는데 지금 계정을 모르면(mac 규칙 unknown-current): 표시를 전혀 바꾸지 않고 "사용 한도에 도달했지만 지금 로그인된 계정의 정보를 읽지 못해 자동 전환을 하지 못했습니다. Claude Code 에서 /login 으로 다시 로그인해 주세요" 를 한 번만 보인다. 지금 계정이 다시 알려지거나 전환이 끝나면 이 표시를 푼다. 전환할 계정이 없을 때도 같은 방식으로 한 번만 알린다. 문구는 "사용 한도에 도달했지만 전환할 다른 계정이 없습니다" 뒤에 "저장한 계정이 모두 한도에 도달했거나 다시 로그인이 필요합니다" 가 뒤따른다.

### 5.7 터미널 제어 (Windows) — Ghostty AppleScript 대체

확인(§11.1, §11.3): 콘솔 붙기·제목·입력 넣기(줄 단위·raw 모드 모두, Windows Terminal 탭 포함), 세션의 터미널 창 찾기, 새 콘솔 실행(인자·환경·작업 폴더가 그대로 도착. 계정 추가는 이 길을 쓰지 않는다, §5.14.6), 클래식 콘솔 창으로 세션 줄 클릭이 그 창을 앞으로 가져오는 것(§11.3 conhost).
**Windows 미확인**: Windows Terminal 탭 고르기(UI Automation), 실제 Claude Code 에서의 일시중지·재개·중단, 실제 Claude Code 에서 Esc 한 글자(`1b`)가 닿는지.

- 콘솔 입력(보드 밖 콘솔): 보드 프로세스가 `FreeConsole` → `AttachConsole(pid)` → `CreateFileW("CONIN$")` → `WriteConsoleInputW` 로 KEY_EVENT(누름·뗌) 를 넣고 → `FreeConsole`. Esc = VK_ESCAPE/0x1B, Enter = VK_RETURN/`\r`, 글자는 UTF-16 단위마다 `wVirtualKeyCode=0` 에 문자. 붙어 있는 동안 `SetConsoleCtrlHandler(NULL, TRUE)`. 한 번에 한 세션씩(전역 잠금).
  보드가 연 ConPTY(보드 터미널 탭)에 붙은 pid 에는 `AttachConsole` 하지 않는다. 자기 ConPTY 에 붙은 채 탭이 닫히면 콘솔 닫힘 신호로 보드가 끝날 수 있기 때문이다.
- 구동 대상(`drive.rs drive_target`, 순수 함수 하나가 `Session.canDrive` 와 모든 구동의 목적지를 정한다). 처음 맞는 줄이 이긴다:

  | 조건 | 키 넣기(일시중지·재개·이름 바꾸기) | 중단 |
  |---|---|---|
  | 끝났거나 pid 0 | 없음 | 없음 |
  | `tty`(보드 터미널 짝)가 있고 `mode` 없는 대화형이며 그 터미널에서 claude 가 도는 중(`runsClaude`) | 그 보드 터미널에 원시 바이트(`TermHub::write`) | 같음 |
  | `tty` 가 있으나 위에 안 맞음(`-p`·claude 가 앞에서 안 돔) | 없음 | claude pid 종료 |
  | headless 이고 `tabAncestor` 가 있음(탭 안에서 시작된 `claude -p`) | 없음 | claude pid 종료 |
  | 그 밖 headless(`--bg`·`-p` 백그라운드) | 없음 | 없음(보드가 신호를 보내지 않는다) |
  | 콘솔에 붙을 수 있음(`drivable`, 3초 탐침) | 콘솔 입력 | 콘솔 입력, 실패하면 claude pid 종료 |
  | 그 밖 | 없음 | claude pid 종료 |

  원시 바이트는 Esc = `1b`, Enter = `0d`, 글자는 UTF-8 이다. 글에 제어 문자(`\r`·`\n`·Esc·`\x03` 같은 C0)가 있으면 아무것도 보내지 않고 거부한다. 단계 사이 기다림은 단계마다 5초, 한 요청 합계 10초가 상한이다.
  구동 직전에 세션을 다시 읽어 같은 pid·같은 생성 시각(`pid_ctime`)이 아직 살아 있을 때만 진행한다(`still_same_session`). 끝났거나 pid 가 바뀌었으면 아무것도 하지 않는다.
  claude pid 종료는 `proc::alive(pid, pid_ctime)` 로 생성 시각이 같음을 한 번 더 확인한 뒤 그 pid 하나만 끝낸다. 탭의 셸은 죽이지 않는다.
- 일시중지 = Esc 를 넣고, 넣는 데 성공한 뒤에만 `<sid>.paused`(`{paused_at, topic}`)를 쓴다. 메뉴에서는 `working`·`waiting` 이고 `canDrive` 일 때만 켜진다. 재개(판정은 같은 파일의 순수 함수 `resume_plan(state, pid, alive)`, 살아 있음 = 끝나지 않았고 pid 가 없거나 pid 가 살아 있음): 살아 있는 세션은 콘솔 입력으로 `CARRY_ON_PROMPT` 입력 → 200ms → Enter 후 표지를 지운다. pid 를 모르는 살아 있는 세션도 이 쪽으로 판정하지만 구동 대상표(위)에서 pid 0 이 대상이 없어 넣기가 실패한다. 키를 넣지 못하면 표지를 그대로 두고 "<폴더>: 세션이 아직 돌고 있지만 그 터미널에 키를 넣지 못했습니다. 그 터미널에서 이어 가 주세요" 를 알린다. 새 탭으로 이어 열지 않는다: 같은 세션이 중복으로 뜨는 것을 막는다(mac 규칙과 같다). 끝났거나 pid 가 죽었으면 이어 열기(carryOn: 열기가 끝나면 `reopen_done` 이 표지를 지운다).
  중단 = 확인 대화상자(`--action stop` 은 확인 없음) → 확인이 떠 있는 동안 바뀌었을 수 있어 세션을 다시 읽음 → 표지 삭제 → `Esc → 400ms → /exit → 200ms → Enter`. 보드 터미널에 쓰지 못하면 claude pid 종료로 잇는다(`run_with_fallback`).
  문구: `CARRY_ON_PROMPT` = "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요."(`drive.rs`·`sessionCommand.ts` 가 같은 문장을 쓴다). 자동 전환 문구 = `ClaudeBoard.swift AutoSwitcher`.
- 세션 줄 클릭(프론트 `board/openSession.ts` 의 순수 표 하나. 위치 `place` 는 §5.12.7):

  | 위치 | 한 번 클릭 | 더블 클릭 | … 메뉴 |
  |---|---|---|---|
  | `tab`(보드 탭) | 그 탭을 고르고 보드를 앞으로(`reveal`) | 같음 | 일시중지·재개·중단 |
  | `terminal`(다른 터미널) | `activate`: 그 터미널 앱 창만 앞으로 | `focus`: 그 창(Windows Terminal 이면 그 탭)으로 | 일시중지·재개·중단 |
  | `background`(살아 있는 headless) | 보드 안 알림 6초: "<제목>: claude <mode, 없으면 -p> 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다." | 같음 | 없음 |
  | `ended` | `reopen`(일시중지 표지가 있으면 carryOn). headless 였던 세션은 아무것도 안 함 | 첫 클릭이 이미 열고 있어 무시 | 세션 다시 열기·재개 |

  다른 터미널 줄만 한 번 클릭을 `GetDoubleClickTime`(명령 `double_click_time`)만큼 미룬다(`clickGate.ts`). 두 번째 클릭이 오면 `activate` 없이 `focus` 만 낸다. 세 번 눌러도 `focus` 는 한 번이다. 보드 탭 줄은 미루지 않는다.
- 세션으로 이동(`focus`): 콘솔에 붙어 `GetConsoleWindow` → 보이면 그 창, 아니면 소유 창(`GW_OWNER`)·조상 프로세스의 보이는 최상위 창 → 최소화면 복원 후 앞으로(`AllowSetForegroundWindow`/`SetForegroundWindow`, 막히면 `AttachThreadInput` 방식). Windows Terminal 이면 탭까지: 그 콘솔 제목을 표지 `CBFOCUS-<8자>` 로 잠깐 바꾸고 UI Automation 으로 이름이 그 표지인 TabItem 을 찾아 `SelectionItemPattern.Select` 한 뒤 제목을 되돌린다(`ClaudeBoard.swift focusGhostty` 와 같은 생각). 창을 못 찾으면 Windows Terminal(없으면 아무것도 안 함).
- 이어 열기(`reopen`): 새 콘솔 창을 띄우지 않고 보드 탭에서 연다. Rust 가 이벤트 `board://open-session`(`{id, action, carryOn, projectDir, resumeArgs}`)을 프론트 허브에 보내고, 허브가 연다.
  허브가 듣기를 시작하기 전의 요청은 줄에 쌓아 두었다가 `take_open_session` 이 통째로 돌려준다(보드가 막 떠서 `--action` 이 먼저 오는 경우). 첫 상태 묶음이 오기 전의 요청은 그때까지 미룬다.
  - 같은 폴더의 탭 가운데 셸이 프롬프트에 있고(`atPrompt`) claude 가 없고 명령이 없고 3초 안에 친 것이 없는 화면이 있으면 거기에 치고, 없으면 새 탭을 연다. 터미널이 접혀 있으면 펼친다.
  - 명령 = `claude --resume <id> <원래 인자>`(carryOn 이면 뒤에 `CARRY_ON_PROMPT` 를 큰따옴표로 묶어 붙임) 다음에 `--name <탭 이름>`. 원래 인자는 `resume_args` 에서 끝의 `--resume <id>` 쌍을 떼고 `-p`·`--print` 를 뺀 나머지다. 남은 인자가 허용 글자(`[A-Za-z0-9._:=/@+-]`, 맨 앞 `@` 제외) 밖이거나 `--resume`·`--continue`·`--session-id`·`--name` 따위와 부딪히면 인자를 모두 버리고 `claude --resume <id>` 로 돌아간다. 세션 id 는 `[0-9A-Za-z-]{8,64}` 이어야 하고 아니면 명령을 만들지 않는다. 셸(cmd.exe·PowerShell)에 해석될 글자는 명령줄에 넣지 않는다.
  - 이어 열기를 시작한 세션은 표지를 쥔다. 세션이 끝남을 벗어나거나 그 터미널에서 claude 가 뜨거나 10초가 지나면 푼다. 표지가 있는 동안 같은 세션의 이어 열기 요청은 그 탭을 앞으로 가져오기만 한다.
  - carryOn 이면 치고 난 뒤 프론트가 `reopen_done` 을 부르고 그때 `<sid>.paused` 를 지운다. 명령을 만들지 못하면 "세션을 이어 열 수 없습니다" 를 알리고 표지를 남긴다.
- 계정 추가는 새 터미널 창을 띄우지 않고 보드가 쥔 ConPTY 에서 돈다(§5.14.6).
- 이름 바꾸기: 탭 이름을 바꾸면(Ctrl+Shift+R) 그 탭에서 도는 대화형 세션에 `/rename <이름>` 을 친다(`renamePlan.ts`·`renameFlow.svelte.ts`, mac `renameSessions`·`applyRenames`). 구동은 `drive_session`, 지금 쳐도 되는지는 `rename_check`(상태 `working`·`idle` 이고 `~\.claude\sessions\<pid>.json` 의 status 가 `waiting` 이 아니고 대화 기록이 있음)가 가른다. 이름은 `sessionName.ts` 허용 목록을 거치고 겹치면 `-2`, `-3` 을 붙인다. 끝난 세션·`mode` 가 있는 세션·짝 없는 세션에는 치지 않는다. 친 뒤 이름이 반영되거나 5초가 지나면 그 세션의 이름 바꾸기를 끝낸다.
- `--action pause|resume|stop|reopen|focus|activate <id>` 로 실행되면 떠 있는 보드에 넘긴다(단일 실행 플러그인의 인자 전달). stop 은 확인 없이, reopen 은 carryOn 이 참으로, focus 는 보드 허브가 그 세션의 탭을 고르거나(보드 탭이면) 그 창으로 가게(`openSession(double)`) 한다.
- 동작 이름은 mac `performAction` 어휘와 같고 명령줄·`session_action` 이 같은 집합을 쓴다(`session_action.rs` 의 `SessionAction::parse`): `pause` · `resume` = 일시중지한 세션 재개(mac `resumePaused`) · `stop` · `reopen` = 꺼진 세션 이어 열기(mac `resumeSession(carryOn:)`; `session_action` 의 `carryOn` 을 안 주면 명령줄처럼 true) · `focus` · Windows 전용 `activate`(한 번 클릭). 옛 이름 `resume_paused` 는 받지 않는다.
- 다른 터미널의 세션을 보드 탭으로 옮기기(mac "보드로 옮기기")와 알림 패널을 눌렀을 때 그 세션으로 이동은 §5.12.12 가 소유한다.
- 이어 열 때의 명령은 `resumeCommand` 가 만든 줄에 `mdRenderCommand`(§5.12.11)가 `--plugin-dir` 을 하나 붙이고 `namedCommand` 가 `--name` 을 붙인 것이다. 세션 기록의 `resume_args` 에 보드가 앞서 붙인 md-render `--plugin-dir <...\plugins\md-render>` 쌍이 들어 있으면 허용 글자 검사 전에 그 쌍만 떼어, 나머지 인자(`--model`·`--dangerously-skip-permissions` 등)를 버리지 않는다(`sessionCommand.ts stripMdRenderPluginDir`).

### 5.8 업데이트 — `Updater` (`ClaudeBoard.swift Updater`)

- 현재 버전 = 빌드 때 `windows/VERSION`. 개발 빌드(`VERSION` 을 못 읽었거나 환경변수로 dev 표시)는 확인하지 않고 제목 옆에 `dev`.
- 켤 때와 1시간마다 `GET https://api.github.com/repos/suhojang/claude-board/releases?per_page=30`(헤더 `User-Agent: ClaudeBoard-Windows`, `Accept: application/vnd.github+json`) → draft·prerelease 가 아니고 `tag_name` 이 `windows-v` 로 시작하는 첫 릴리스 → 버전 = 태그에서 `windows-v` 를 뺀 것(숫자 비교: `cmp_version` 은 점 조각마다 앞의 0 을 떼서 길이로, 길이가 같으면 글자 차례로 견준다. 조각이 아무리 길어도(수 자리가 아무리 많아도) 옳게 견준다. 없는 조각은 0 이다). 더 새로우면 알림 줄. **`/releases/latest` 를 쓰지 않는다.**
- 알림 줄: "새 버전 <v> 이 나왔습니다" · **변경 내용**(`https://github.com/suhojang/claude-board/blob/main/windows/CHANGELOG.md`) · **업데이트**. 손으로 확인했는데 없으면 "최신 버전입니다 (<v>)" 4초, 실패하면 "업데이트를 확인하지 못했습니다".
- 업데이트 실행: `https://raw.githubusercontent.com/suhojang/claude-board/windows-v<v>/windows/packaging/install.ps1`(태그 기준 원본 — 릴리스 자산과 달리 UTF-8 로 내려와 update.log 의 한글이 깨지지 않는다)로 `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "& { $s = irm '<url>'; & ([scriptblock]::Create($s)) -Version '<v>' -Update }"` 를 창 없이 분리 실행, 출력은 `notifier\update.log`. 이때 환경변수 `PSModulePath` 를 넘기지 않는다 — PowerShell 7 에서 띄운 보드는 PowerShell 7 의 모듈 경로를 물려받고, 그것을 받은 5.1 은 `Get-FileHash` 를 찾지 못해 install.ps1 이 멈춘다(실기 확인). 성공하면 스크립트가 보드를 끄고 새 보드를 띄우므로 돌아오지 않는다. 프로세스가 끝나 돌아오면 실패 — "업데이트하지 못했습니다 · ~/.claude/notifier/update.log" + 다시 시도.
- 업데이트 전 확인: `run()` 은 업데이트할 것이 있고(`latest` 가 있다) 이미 도는 중이 아닐 때(`can_start`)만 끌 때 확인(§5.12.10, 이유 `update`)을 건다. 할 일이 없으면 창도 `.paused` 표지도 만들지 않는다. 확인에서 취소하면 업데이트를 시작하지 않는다. 확인을 기다리는 사이 다른 업데이트가 시작됐으면 이 호출이 남긴 표지를 지우고 물러난다. 스크립트가 보드를 끄지 못하고 돌아오면(실패) 확인에서 남긴 표지를 지운다(세션은 그대로 도는 중이므로 일시중지로 남지 않게 한다, mac `Updater.run`).

### 5.9 보드 설정 저장 (macOS UserDefaults 대응)

`board-settings.json`(Tauri app config dir) 한 파일, 백엔드가 단일 출처. 키: `collapsed[]`, `endedOpen[]`, `pinnedGroups[]`, `hiddenGroups{그룹 id:시각}`, `autoFolded{그룹 id:시각}`, `kindCollapsed[]`("<그룹 id>|term"·"<그룹 id>|bg"), `subCollapsed[]`(세션 id), `usageCompact`, `pinned`(항상 위, 기본 true), `mascot`(기본 `tralalero`), `opacity`(0.3~1, 기본 1), `autoSwitch`(기본 false), `autoSwitchLast`, `autoSwitchFull{id:시각}`, `boardMini`, `boardRestoreFrame{x,y,w,h}`, `windowFrame`.
내장 터미널 키(이름은 macOS UserDefaults 와 같다): `terminalExpanded`(bool, 기본 true), `boardPaneWidth`(f64, 논리 px, 기본 454), `boardSide`(`"right"`\|`"left"`, 기본 `"right"`), `miniFromExpanded`(bool, 펼친 채 미니로 들어갔는지의 표지),
`terminalTabs`(탭 목록, 항목 모양은 §5.12.9, 기본 빈 목록), `mdRenderOff`(답변 꾸미기를 끄고 띄운 세션 id 목록, §5.12.11, 기본 빈 목록).
설정 화면 키(§5.14): `shortcuts`(`{동작 id: [키 코드, 수정키 비트]}`, 비운 동작은 `[]`, 기본 키면 항목 없음, 기본 `{}`, §5.14.3), `settingsTransition`(`"slide"`\|`"fade"`\|`"none"`, 기본 `"slide"`, 모르는 값은 `"slide"` 로 읽는다), `newSessionCommand`(문자열, 기본 빈 값, §5.14.4), `newSessionMdRender`(bool, 기본 true), `autoUpdateCheck`(bool, 기본 true, §5.14.5).
`shortcuts` 는 읽을 때 항목마다 모양을 거른다(객체가 아니면 그 키만 버리고, 배열이 아니거나 모양이 틀린 항목은 그 항목만 버린다. 항목 하나 때문에 파일 전체를 기본값으로 읽지 않는다).
로그인할 때 열기는 이 파일이 아니라 Run 값 `ClaudeBoard` 와 표지 파일 `login-launch-off`(§2.2)가 소유한다.
`terminalTabs` 는 `save_tabs` 가 통째로 바꿔 넣고 `load_tabs` 가 읽는다. 값은 프론트가 쥔 모양이며 백엔드는 항목을 풀지 않는다.
`SettingsView` 에는 `terminalExpanded`·`boardSide`·`boardPaneWidth`·`mdRenderOff` 와 설정 화면 키 다섯이 있다(`terminalTabs` 는 스냅샷에 싣지 않고 `load_tabs` 로만 읽는다). 1초 스냅샷(§6.1)에는 터미널 목록을 넣지 않는다 — 터미널 상태는 이벤트 기반이고 프론트 `TerminalHub` 가 `term_list` 와 채널로 채운다(§5.12).

### 5.10 창·트레이·단축키

- 보드 창: 장식 없음(직접 그린 제목 막대: 끌기 영역, 오른쪽에 최소화·닫기, 왼쪽에 "처음 크기로 보드 쪽에 붙이기" 단추, 오른쪽에 최대축소/⤢ 단추, 터미널 펼치기/접기 단추), 투명 배경 + Windows 11 Mica/아크릴(`window-vibrancy`, 실패 무시), 크기 조절 가능, 최소 300×144.
  실행할 때: 미니로 끄지 않았으면 `terminalExpanded` 가 true 일 때 펼친 채(작업 영역 전체), false 일 때 접은 채 보드 쪽(`boardSide`)에 폭 `boardPaneWidth`(기본 454)·그 모니터 작업 영역 높이 전체로 붙여 연다(마지막 가로 위치는 쓰지 않는다. 모니터는 저장한 창 위치로 고르고 없으면 주 모니터). 미니로 껐으면 그 크기·위치 그대로. 닫기 = 숨기기(앱은 트레이에 남음).
- 항상 위(pinned, 기본 켜짐, mac 3.1.9): 창이 다른 창 위에 있는 조건은 `pinned && !(펼침 && 작업 영역을 덮음)` 이다(`window.rs on_top`). 접힌 창과 60% 로 줄인 펼친 창은 `pinned` 를 따르고, 그 모니터 작업 영역 전체를 덮은 펼친 창만 고정을 쉰다.
  덮는다 = 창 프레임이 작업 영역의 네 변 모두 허용 오차 2px 안으로 같거나 넘는다(`covers_work_area`, 2px 는 배율 반올림을 흡수하려는 판단값이고 실측이 아니다). 최소화됐거나(Windows 는 프레임을 -32000 으로 옮긴다) 작업 영역과 겹치지 않는 창은 덮는지 모르므로(`covering_state` 가 None) 지금 상태를 그대로 둔다.
  프레임이나 모니터를 못 쟀으면 덮는 것으로 본다. 다시 정하는 때(`refresh_on_top`): 고정을 바꿨을 때, 접고 펼칠 때, 창 크기·위치 변경이 0.7초 넘게 멎었을 때(`save_frame_if_settled`).
- 투명도: 슬라이더 값(30~100%)을 문서 전체 불투명도로 적용한다. 불투명한 때는 `!미리보기 && (창이 작업 영역을 덮음 || 마우스가 창 안)` 이다(`opacity.ts boardOpaque`). 포커스·활성은 보지 않는다 — 보드가 앞에 있어도 마우스가 창 밖이면 슬라이더 값으로 보인다.
  슬라이더를 움직이는 동안과 놓은 뒤 1.2초는 미리보기이며 이때는 덮고 있어도 슬라이더 값으로 보인다(mac 과 같은 순서). 덮는지는 `window.outerWidth/Height` 와 `screen.availWidth/Height` 로 재고(`coversWorkArea`, 허용 오차 2px) 값을 못 쟀으면 덮는 것으로 본다. 펼쳤는가가 아니라 덮는가를 본다 — 60% 로 줄인 펼친 창은 마우스가 밖이면 흐려진다.
- 전역 단축키 Ctrl+Alt+B = 터미널을 접고 처음 크기로 보드 쪽(`boardSide`, 기본 오른쪽)에 붙이고 앞으로(포커스는 가져가지 않음). 기본 키이며 설정 > 단축키에서 바꾸면 그 키로 다시 등록한다(§5.14.3). 등록 실패는 `board.log` 에 남고 설정 > 단축키의 그 줄에도 보인다.
- 전역 단축키 Ctrl+Alt+T = 터미널 펼침/접힘(기본 키, 바꿀 수 있다). Ctrl+Alt+B 와 같은 `with_handler` 에 등록하고 실패는 로그와 설정 > 단축키에 알린다. 한국어 키보드에서는 왼쪽 Ctrl+Alt 로 누른다. 동작:
  접힘이면 펼치고·표시·포커스를 주며 이벤트 `board://terminal-focus` 를 보낸다 / 펼침이고 앞에 있으면 접는다 / 펼침이지만 다른 앱 뒤에 있으면 표시·포커스·`board://terminal-focus`(프론트가 활성 터미널에 `focus()`).
- 터미널 펼침/접힘(`terminalExpanded`): 창 안은 `[터미널 │ 경계 │ 보드]`(`boardSide` 가 `"left"` 면 보드가 왼쪽) 세 칸이다. 크기는 모두 물리 px = 논리 px × 그 모니터 배율이다.
  - 펼침: 프레임 = 그 모니터 작업 영역 전체(`m.work`), 항상 위는 `on_top(pinned, 펼침, 덮음)` = 끔(작업 영역을 덮은 프레임이다), 최소 크기 논리 848×300(= 보드 360 + 경계 8 + 터미널 480). 보드 칸 폭 = `clamp(boardPaneWidth, 360, innerWidth − 8 − 480)`.
    펼친 동안 창은 자유 크기(끌기·크기 조절 가능)이고 `save_frame_if_settled` 는 프레임을 저장하지 않는다. 사용자가 작업 영역보다 작게 줄이면(예: 약 60%) 덮지 않으므로 `pinned` 이면 다시 맨 위에 둔다(이 규칙은 멎은 뒤 `refresh_on_top` 이 맞춘다).
  - 접힘: 보드 쪽 가장자리에서 `boardPaneWidth` × 배율 폭, 작업 영역 전체 높이, 항상 위 = `pinned`(`on_top(pinned, 접힘, _)`), 최소 300×144. 접을 때와 접힌 채 시작할 때는 늘 `boardPaneWidth` 로 보드 쪽에 붙인다(마지막 가로 위치는 없다). `launch_frame` 과 접힌 창 프레임이 고정 폭(`LAUNCH_WIDTH`)과 오른쪽 가장자리 대신 `boardPaneWidth`·`boardSide` 를 쓴다. `snap_right` 명령은 이름을 그대로 두고 `boardSide` 쪽에 붙인다(펼쳐 있으면 접는다).
  - 경계를 끌다 놓으면 `set_board_width(width)` 가 `boardPaneWidth`(논리 px)를 저장한다.
  - 접을 때 터미널은 언마운트하지 않고 숨긴다. 출력은 숨겨진 동안에도 백엔드 링 버퍼에 쌓이고 다시 펼치면 이어서 받는다(§5.12).
  - 미니: `enter_mini` 는 먼저 접고 `miniFromExpanded` 를 기록한다. 미니 프레임은 보드 쪽 모서리 기준이다. `leave_mini` 는 그 표지가 있으면 다시 펼친다.
- 트레이: 앱 아이콘. 왼쪽 클릭 = 보드 열고 닫기. 툴팁 "Claude Board — 승인 대기 n · 작업 중 n". 승인 대기가 있으면 주황 점, 작업 중만 있으면 파란 점이 붙은 아이콘으로 바꾼다(미리 만든 그림). 오른쪽 클릭 메뉴: 세션 보드 보기 · 터미널 펼치기 (<지금 키>)(체크) · 항상 위에 고정(체크) · 설정… · 업데이트 확인… · 종료. 캐릭터·알림 위치·보드 위치 항목은 없고 설정 화면에서 고른다(§5.14.5).
- 단일 실행: 두 번째 실행은 첫 보드를 앞으로(`--show` 와 같음), `--action` 이면 동작을 넘기고 끝. `--background` 면 창을 보이되 포커스를 가져가지 않는다.
- Windows Terminal(`WindowsTerminal.exe`)이 새로 실행되면 보드를 보이게(포커스는 그대로) — 3초 주기 프로세스 확인. (macOS 의 Ghostty 연동 대응)
- 설치된 Claude Code 버전: `%USERPROFILE%\.local\share\claude\versions\` 의 가장 높은 이름, 없으면 표시 안 함(1분 캐시).

### 5.11 터미널 독 (`EdgeDock`, `ClaudeBoard.swift EdgeDock`)

창 오른쪽 아래 끝 탭에 마우스를 대면 나오는 실행 단추 묶음. Windows 목록: Windows Terminal(`wt.exe`), PowerShell 7(`pwsh.exe`), Windows PowerShell(`powershell.exe`), 명령 프롬프트(`cmd.exe`), Git Bash(`git-bash.exe` — `%ProgramFiles%\Git`). 설치 여부(PATH·알려진 위치), 실행 중 점, 힌트 문구("<이름> 창으로 이동"/"<이름> 실행"/"<이름> · 설치되지 않음"). 아이콘은 직접 그린 SVG.

### 5.12 내장 터미널

보드 창 안에 터미널 영역을 두고 거기서 셸을 돌린다(macOS 판 `BoardRootView`·`TerminalHub` 대응). 이 절이 백엔드 ↔ 프론트엔드 터미널 계약을 소유한다. 확인된 동작은 §11.2~§11.5 의 표만 확인으로 읽는다. 나머지는 미확인이다.

범위. 있는 것: 펼침·접힘(제목 막대 단추·Ctrl+Alt+T·트레이), 경계 끌기와 폭 기억, 보드 위치(오른쪽 기본·왼쪽), 숨겨진 동안 출력 유지, 한글 IME,
경로 줄·탭 줄·빈 상태(둘 다 넘치면 가로로 밀기, §5.12.13), 새 세션 창(Ctrl+T·카드의 +, 키는 §5.14.3 에서 바꾼다), 탭마다 셸 하나와 그 셸에 쳐 넣는 claude(`--session-id`·`--name`), 탭 이름 바꾸기와 `/rename`, 닫기 확인, Ctrl+Alt+1~9 번호 배지,
세션과 탭의 짝짓기와 위치 칩(§5.12.7), 세션 줄 클릭 흐름과 일시중지·재개·중단·이어 열기(§5.7), 크기 바꾸기 장벽(§5.12.8),
탭 목록 저장과 복원(§5.12.9), 보드를 끌 때 확인 창(§5.12.10), 앱 쪽 답변 꾸미기(md-render, §5.12.11), 다른 터미널 세션을 보드 탭으로 옮기기와 알림 패널에서 세션으로 이동(§5.12.12).
미구현(각각 한 줄): 화면 나누기(자료구조 `SplitNode` 만 있고 탭마다 잎 하나만 그린다) · 터미널 글꼴 크기·테마·애니메이션 설정 · 보드 글자 크기 배율 ·
실제 Windows 10 확인 · 모니터가 여럿일 때의 배치 확인 · AltGr 배열 확인. 목록은 §11.7 이 소유한다.
이름은 macOS `ClaudeBoard.swift` 와 맞춘다: 탭 `BoardTab`, 화면 `BoardSurface`, 칸 배치 `SplitNode`, 저장소 `TerminalHub`(프론트 `hub.svelte.ts`), 탭 줄 `TerminalTabBar`, 경로 줄 `TerminalWorkspaceBar`, 영역 `TerminalAreaView`, 세션 동작 `performAction` 어휘.
탭·분할로 늘릴 수 있는 구조다. 모든 명령이 `TermId` 를 받고, 칸 배치는 분할 트리형이다. 지금은 탭마다 잎 하나다.

#### 5.12.1 식별자와 명령

백엔드가 프로세스 수명 동안 유일한 `TermId` 문자열(`"t1"`, `"t2"`, …)을 낸다. 프론트는 칸(pane)마다 `leafId`(UUID)를 따로 만든다. 셸이 다시 떠도 칸은 그대로다.
명령은 모두 `#[tauri::command(async)]` 이고 `terms.rs` 에 있다. JS 쪽 인자 이름은 camelCase. 상태 타입은 `terms.rs` 의 `pub struct TermHub`(`Default`)이고 `board.rs` 가 `manage` 한다.

```ts
interface TermInfo {
  id: string; pid: number; shell: "pwsh" | "powershell" | "cmd"; cwd: string; cols: number; rows: number;
  status: "running" | "exited"; exitCode: number | null; windowsBuild: number | null;
  oldestSeq: number; nextSeq: number;   // 링 버퍼가 아직 갖고 있는 출력 오프셋 범위
}
```

| 명령 | 인자 → 결과 | 동작 |
|---|---|---|
| `term_spawn` | `spec: { cwd?, cols, rows }` → `TermInfo` | 셸을 pwsh → powershell → `%ComSpec%` 순으로 고른다. pwsh·powershell 은 `-NoLogo`(프로필은 읽는다), cmd 는 인자 없음. 프론트가 맞춘 크기로 만든다. 시작 폴더는 `spec.cwd`, 없거나 폴더가 아니면 홈 폴더다. 프론트는 탭의 작업 경로가 홈(`~`)이 아닐 때만 `cwd` 를 보낸다 |
| `term_list` | → `TermInfo[]` | 지금 있는 터미널 목록 |
| `term_attach` | `id, fromSeq, onData: Channel<InvokeResponseBody>` → `TermInfo` | 그 id 의 이전 채널을 대체한다(보는 쪽은 하나, 크레딧 초기화). 링 버퍼를 `max(fromSeq, oldestSeq)` 부터 다시 보낸다 |
| `term_ack` | `id, seq` | xterm 이 파싱을 끝낸 끝 오프셋. 프론트가 16ms 또는 64KiB 마다 최대 한 번 보낸다 |
| `term_write` | `id, data: string` | xterm `onData`. ConPTY 입력 파이프에 UTF-8 로 통째로 쓴다 |
| `term_write_bin` | `id, data: number[]` | xterm `onBinary`. 같은 방식 |
| `term_resize` | `id, cols, rows` → `number \| null` | cols < 2, rows < 1, 같은 크기, 이미 닫힌 콘솔이면 null 을 돌려주고 아무것도 하지 않는다. 적용했으면 ConPTY 크기를 바꾸기 직전까지 리더가 읽은 출력 오프셋(`bytesReadBeforeCall`)을 돌려준다. 프론트 장벽(§5.12.8)이 이 값으로 다시 그리기 머리말을 찾는 기준을 잡는다 |
| `term_kill` | `id` | 별도 스레드에서 `ClosePseudoConsole`. 6초 안에 안 끝나면 프로세스를 강제 종료한다. 프로세스가 끝나면 터미널은 `term_list` 에서 빠지고 그 출력 보내기 스레드도 끝난다. 끝났지만 죽이지 않은 터미널은 보던 웹뷰가 닫힐 때까지 남는다 |

#### 5.12.2 채널 프레임

- 원시 데이터: 8바이트 little-endian `seq`(첫 페이로드 바이트의 오프셋) + 페이로드 최대 64KiB. JS 는 ArrayBuffer 를 `term.write(bytes, cb)` 로 쓰고 `cb` 에서 ack 한다. xterm 이 UTF-8 디코더 상태를 유지하므로 청크 경계가 글자 중간이어도 안전하다.
- JSON 제어: `{t:"gap", seq}` 는 보낼 앞부분을 링 버퍼가 덮어쓴 경우다. 프론트는 터미널을 리셋하고 `seq` 부터 다시 받는다. `{t:"exit", code}` 는 리더가 EOF 에 닿은 뒤에만 가므로 늘 마지막 출력 뒤에 온다.
- 1024바이트 미만 원시 메시지는 eval 로, 큰 것은 fetch 로 가고 JS `Channel` 은 index 순으로 전달한다고 본다. 이 순서 보장은 tauri 2.12.4 의 동작에 기댄 것이다(미확인).

#### 5.12.3 터미널당 스레드

| 스레드 | 하는 일 |
|---|---|
| 리더 | `ReadFile` 64KiB 씩을 4MiB 링 버퍼(뮤텍스)에 쓰고 condvar 로 알린다. 웹뷰를 기다리지 않는다 |
| 배처 | 8ms 또는 64KiB 마다 보낸다. ack 안 된 바이트가 1MiB 미만일 때만 보낸다. 링 버퍼가 아직 안 보낸 바이트를 덮으면 다음 전송은 `gap` 을 보낸 뒤 `oldestSeq` 부터 다시 보낸다 |
| 웨이터 | `WaitForSingleObject(hProcess)` → 종료 코드 읽기 → 가짜 콘솔을 별도 스레드에서 닫기 → 리더 EOF → `exit` 메시지. 셸이 끝나면 가짜 콘솔을 닫아야 한다. 자손 프로세스가 붙어 있으면 출력 파이프가 저절로 닫히지 않기 때문이다 |

#### 5.12.4 프로세스 생성

- 파이프는 `CreatePipe` 에 널 보안 속성으로 만든다(상속 불가). `updater::run` 이 핸들 상속으로 powershell 을 띄우므로 이 파이프가 새어 나가면 안 된다.
- `CreateProcessW(bInheritHandles = FALSE, EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT)` + `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`. ConPTY 플래그는 0이다.
- `STARTF_USESTDHANDLES` 를 켜고 표준 핸들은 널로 둔다. 콘솔에서 보드를 띄웠을 때 자식이 그 콘솔의 표준 핸들을 물려받지 않게 하려는 것이다(미확인).
- 환경 블록은 `CreateEnvironmentBlock(bInherit = FALSE)` 로 새로 만든다. 보드가 낡은 환경을 들고 있거나 부모 Claude 세션의 `CLAUDECODE`·`CLAUDE_CODE_CHILD_SESSION` 을 물려받는 것을 막는다.
- 첫 표시 때 셸을 띄운다(lazy spawn). 접힌 채 시작하면 띄우지 않는다.
- Windows 빌드 번호는 레지스트리 `CurrentBuildNumber` 에서 읽어 `TermInfo.windowsBuild` 로 준다(xterm `windowsPty` 용).
- `terminal.rs` 의 `locate("pwsh")` 가 Store 앱 실행 별칭(`WindowsApps\pwsh`)을 찾을 수 있다. `is_file()` 통과와 별칭에 대한 `CreateProcessW` + 가짜 콘솔 속성이 되는지는 미확인이다.
- `windows-sys` 0.61 기능 추가: `Win32_System_Console`, `Win32_System_Pipes`, `Win32_Storage_FileSystem`, `Win32_System_IO`, `Win32_Security`, `Win32_System_Environment`, `Win32_System_Registry`. 바뀌는 잠금 파일은 `app/src-tauri/Cargo.lock` 뿐이다.
- 비 Windows 빌드는 `Unsupported` 를 돌려주는 스텁으로 `cargo check` 가 되게 한다.

#### 5.12.5 프론트엔드 규칙 (`app/src/lib/terminal/`)

- 구조: `hub.svelte.ts` 의 `TerminalHub` 가 `terms`(TermId 키)와 `tabs: BoardTab[]`, `surfaces: BoardSurface[]`(`surfaceOf(id)`)를 쥔다. 탭(`BoardTab`)은 `workspace`, `customName`, `renaming`, `root: SplitNode`, `focused`(고른 서피스 id), `zoomed` 를 갖고, 서피스(`BoardSurface`)는 터미널 하나의 상태 `id`, `termId`, `sessionId`, `command`, `held`, `mdRenderOff`, `title`, `typedAt`(셸에 claude 명령을 친 시각, §5.12.7) 을 갖는다.
  `SplitNode = { leaf: 서피스 id } | { split: "right" | "down", ratio, first, second }`(`ratio` 는 `first`(왼쪽·위)의 몫). 나누기 트리 연산은 `terminal/split.ts` 의 순수 함수다. 지금은 탭마다 서피스 하나만 그린다. 안 고른 탭도 마운트한 채 숨겨 셸과 출력이 그대로 남는다.
  탭은 작업 경로(`workspace`)로 묶인다. 경로를 견주는 꼴(`workspaceKey`)은 소문자·`/`→`\`·끝 `\` 제거이고 홈은 `~` 다. 경로 줄은 탭 목록에 처음 나타난 순서다.
  페이지를 새로 고치면 `term_list` 에서 아직 도는 터미널을 탭으로 이어받는다(이름·세션 연결은 잃는다). 이 이어받음은 저장해 둔 탭 목록을 덮어쓰지 않는다(§5.12.9). 앱을 다시 켠 뒤의 탭 복원은 §5.12.9 가 소유한다.
  `term.ts` 는 터미널마다 컨트롤러를 두고, 컨트롤러가 자기 host div 를 가져 다른 부모로 `appendChild` 해서 옮길 수 있다(나중의 분할용).
- xterm 옵션: `allowProposedApi: true`, `unicode.activeVersion = "11"`, `scrollback: 5000`, `allowTransparency: false`(테마 배경은 불투명), 글꼴 `Cascadia Mono, Consolas, D2Coding, monospace`,
  `windowsPty: { backend: "conpty", buildNumber }`(빌드 21376 이상일 때만 `buildNumber`). WebGL 애드온은 `onContextLoss` 때 버리고 DOM 렌더러로 내려간다. 애드온을 못 올리면(예: WebGL2 없음) 로그 한 줄을 남기고 DOM 렌더러로 그린다.
  DOM 렌더러의 칸 맞춤: 앱 전체의 `letter-spacing: 0.025em` 이 xterm 의 글자 폭 재는 요소로 상속되면 행 간격이 칸 폭과 어긋난다. `TerminalPane.svelte` 가 `.term-host` 에 `letter-spacing: normal`, `.xterm-rows` 에 `letter-spacing: 0 !important` 를 둬 상속을 막는다. 재는 명령: `cd windows/app && npm run dom-metrics`(헤드리스 Chrome 에서 79번째 칸의 어긋남).
- WebGL 덮개: `webglBlink.ts` 는 애드온을 폐기할 때 커서 깜박임 관리자가 새는 것을 막는다. 업스트림 `WebglRenderer.ts:32` 가 `_cursorBlinkStateManager` 를 `_register()` 하면 덮개를 지운다. `webglBlink.test.ts` 가 애드온 버전이 0.19.0 인지 재서 버전이 바뀌면 실패한다.
- fit: 접을 때 터미널을 언마운트하지 않고 숨기며 fit 도 하지 않는다. fit 은 칸이 1셀 이상일 때만, 보인 뒤 `requestAnimationFrame` 에서 한 번 한다. 경계를 끄는 동안은 fit 을 멈추고 놓을 때 한 번 한다. 크기 요청은 200ms 디바운스(`RESIZE_DEBOUNCE_MS`) 뒤에 장벽으로 간다. xterm 의 `term.resize` 는 장벽의 `applyLocal` 한 곳에서만 부른다(§5.12.8).
- 다시 받기(replay): 받는 동안 xterm 의 자동 응답 `onData` 를 삼킨다(쓰기마다 카운터를 올리고 쓰기 콜백에서 내린다). 단 사용자 입력 이벤트(keydown·paste·compositionend·input) 뒤 300ms 안의 데이터는 통과한다 — 키 입력은 되감기 중에도 셸에 닿는다(`replayGate.ts`). 쓰기 콜백은 던지지 않는다.
- 키: 선택이 있을 때만 Ctrl+C 는 복사, 아니면 셸로 보낸다. Ctrl+V 는 브라우저 붙여넣기에 맡긴다. Shift+Enter 는 `\n` 을 보낸다.
- IME(`ime.ts`, xterm 이슈 #6065): xterm 소스를 패치하지 않고 키 처리기로만 고친다. 조합 중이 아닌데 keyCode 229 로 오는 키는 `ev.code` 로 바꿔 보낸다(Escape → `\x1b`, Ctrl+A..Z → 0x01..0x1A, Ctrl+Space → 0x00).
  조합 중의 Enter·Escape·방향키·Home/End·Ctrl 조합은 `compositionend` 까지 붙들었다가 다음 틱에 보낸다. 이미 하나 대기 중이면 다시 쏘아진 Enter 는 버린다. Shift 는 조합을 시작한 keydown 에서 가져온다.
- 자산: `@xterm/xterm/css/xterm.css` 를 가져오고 xterm 은 터미널 뷰에서만 동적 `import()` 로 읽는다(`/notify` 번들에 들어가지 않고 프리렌더에서 돌지 않는다). 워커·원격 자산은 없다.
  CSP 는 지금 `csp: null` 이다. 설정한다면 `style-src 'unsafe-inline'` 과 ipc 용 `connect-src` 가 필요하다.

#### 5.12.6 탭·새 세션 (`hub.svelte.ts`, `launch.ts`)

- 탭은 늘 셸이다(pwsh → powershell → cmd 순). claude 명령은 새로 띄운 셸에 붙은 뒤 `typeCommand` 가 친다: 셸의 첫 출력을 기다리고, 출력이 300ms 조용해지면(상한 5초, 컨트롤러의 `lastOutputAt`) `term_write` 로 명령을 쓰고 100ms 뒤 `
` 을 따로 쓴다. 기다리는 사이 탭이 닫히거나 칸의 셸이 바뀌면 치지 않는다. 셸 없이 바로 띄우지 않는다.
  시작 중(`ctl.open()`·`term_spawn`·`bind` 뒤)에 탭이 닫혔으면 새로 띄운 셸을 `term_kill` 로 끝내고 컨트롤러를 버린 채 명령을 치지 않는다.
  `/exit` 하면 같은 탭이 셸 프롬프트로 돌아오고, 셸이 끝나면(exit 이벤트) 그 탭이 닫힌다. 닫으면 선택은 같은 경로의 이웃 탭 → 이웃 경로 → 빈 상태 순으로 간다.
- 새 세션: 터미널이 접혀 있으면 펼치고, 명령을 `freshSessionCommand`(claude 로 시작하고 `--resume`·`-r`·`--continue`·`-c` 가 없으면 `--session-id <새 UUID>` 를 붙이고 기존 `--session-id` 는 새 값으로 바꿈) →
  `namedCommand`(`--name <탭 이름>` 을 claude 바로 뒤에 붙임, 사용자가 `-n`·`--name` 을 적었으면 그대로)를 거쳐 그 폴더의 새 탭에서 친다. 탭 이름은 폴더 이름이고 다른 탭과 겹치면 `-2`, `-3` 을 붙인다.
  `--name` 값은 허용 목록(`sessionName.ts`)만 통과한다: 글자·숫자(유니코드)·공백·`_`·`.`·`-` 외의 글자(굽은 따옴표 “ ” „ 포함)는 모두 `-` 로 바꾸고 이어진 `-` 는 하나로, 앞뒤의 공백·`-`·`.` 은 떼며, 비면 `session`. 공백이 있으면 큰따옴표로 묶는다. 폴더·탭 글자를 검증 없이 명령줄에 넣지 않고, 작업 폴더(cwd)는 `term_spawn` 인자로만 넘기며 쳐 넣지 않는다. 명령줄을 만드는 순수 함수(`freshSessionCommand`·`namedCommand`)는 `sessionCommand.ts` 에 있고, `namedCommand` 는 `sessionName.ts` 의 `sessionNameArg`(정리 + 인용)를 거친 값만 받으며 아니면 던진다. 이름 자리 밖의 글자는 사용자가 적은 명령 그대로다. 셸이 cmd.exe 여도 같은 줄이 한 명령으로 실행되고 `--name` 값은 인자 한 칸으로 간다(`&`·`|`·`^`·`%VAR%`·`!`·따옴표·괄호·끝의 `\`·한글·빈 이름·255자 이름 모두 — 허용 글자 밖은 `-` 로 바뀌어 cmd 의 특수문자가 이름 자리에 남지 않는다). 시험: `cd windows/app && node --test src/lib/terminal/*.test.ts` — `sessionCommand.test.ts` 의 cmd.exe 묶음은 Windows 에서만 돈다. 기본 명령은 `claude --dangerously-skip-permissions` 다.
- 탭 이름: Ctrl+Shift+R 로 이름 자리가 입력 칸이 된다(Enter 저장, Esc 취소, 포커스를 잃으면 저장, 비우거나 폴더 이름과 같으면 사용자 이름을 지움). 끝나면 포커스는 터미널로 간다. 셸이 보내는 제목이 `.exe` 로 끝나면(실행 파일 경로) 탭에 보이지 않게 버린다.
- 닫기: 탭의 ×는 그 탭의 짝 세션(`sessionId` 로 찾은 세션)이 있고 끝나지 않았을 때만 확인 창을 띄우고, 셸만 있는 탭은 바로 닫는다. 닫으면 `term_kill` 한다.
- 미구현은 §5.12 범위 절의 목록이 소유한다.
- 터미널에 명령을 칠 때(`typeCommand`·`typeAtPrompt`)는 먼저 `mdRenderCommand`(§5.12.11)로 `--plugin-dir` 을 정리하고, 그다음 `applyTestClaude`(§5.13)로 시험 모드의 맨 앞 `claude` 를 바꾼다. 시험 모드 여부를 아직 받지 못했거나 어긋났으면 아무것도 치지 않고 board.log 에 한 줄 남긴다.
- 탭 소속은 같은 콘솔 호스트 기준이다(§5.12.7). 보드는 자기 ConPTY 에 붙은 pid 에 `AttachConsole` 하지 않는다(탐침 `can_attach` 도 건너뛴다).
- md-render(앱 쪽 옮겨 쓰기와 `--plugin-dir`)는 §5.12.11 이 소유한다.
- 보드가 꺼질 때는 Job 객체 없이 콘솔 끊김에 맡긴다. 트레이 **종료**와 업데이트는 그 전에 §5.12.10 의 확인을 건다.

#### 5.12.7 세션 짝짓기와 위치 칩

보드는 세션 파일의 `pid` 가 보드 탭 어느 터미널의 claude 인지 가려 세션 줄에 위치 칩을 붙인다(mac `TerminalHub.pair`·`place`·`PlaceChip`). Windows 에는 tty 가 없어 "같은 콘솔 호스트"가 그 자리를 맡는다.
ConPTY 마다 자기 콘솔 호스트 프로세스(conhost/OpenConsole `--headless`)가 있기 때문이다.

- 순수 규칙은 `crates/core/src/place.rs`, 어댑터는 `src-tauri/src/terms/pairing.rs`(3초 탐침 스레드)다. 규칙(처음 맞는 것):
  - R0 끝난 세션·pid 0 이면 모두 없음(`/clear` 로 끝난 옛 id 는 같은 pid 를 쓰므로 묶지 않는다).
  - R1 `tty`(주 규칙): 세션 pid 의 콘솔 호스트(`NtQueryInformationProcess(ProcessConsoleHostProcess)`, 값의 낮은 두 비트는 플래그라 뗀다)가 어떤 보드 터미널 셸의 콘솔 호스트와 같으면 그 `TermId`. 같은 콘솔을 상속한 자손(`cd`·`cmd /c claude`·`claude.cmd` 중계)은 모두 같은 답이다. 분리 프로세스(`DETACHED_PROCESS`)·`start ""`(새 콘솔)·`CREATE_NO_WINDOW` 자식은 다른 호스트라 짝이 안 된다.
  - R2 `tty`(대체 규칙): 콘솔 호스트를 못 읽을 때(`Unknown`)만, 조상을 따라가 보드가 띄운 셸 pid 에 닿고 headless 가 아니면 `tty`.
  - R3 `tabAncestor`: `tty` 가 없고 headless 이고 `mode` 가 `--bg` 가 아니며 조상이 보드 셸에 닿으면(탭 안에서 시작된 `claude -p`).
  - 조상 따라가기: 최대 12단계, 단계마다 부모의 생성 시각이 자식보다 늦지 않아야 하고(pid 재사용 방지, 시각을 모르면 멈춤), 중간에 분리 프로세스나 `--bg` 가 있으면 멈춘다(감독자 경계).
  - `mode`: argv 에서 `--bg`·`--background`·`--bg-pty-host` 면 `--bg`, `-p`·`--print` 면 `-p`, 없으면 null.
  - `appName`(보드 밖 터미널 세션만): `WindowsTerminal.exe` → `Windows Terminal`, `Code.exe` → `VS Code`, `mintty.exe` → `Git Bash`, 그 밖 `터미널`.
  - `runsClaude`: 보드 터미널 셸의 자손 가운데 Claude Code 프로세스(`is_claude_exe`)의 콘솔 호스트가 그 터미널의 것인 `TermId` 들. `atPrompt`: 셸에 `conhost.exe`·`OpenConsole.exe` 밖의 살아 있는 자식이 없는 `TermId` 들.
- 스냅샷에 실리는 필드: `Session.tty`·`tabAncestor`·`mode`·`appName`·`headless`·`canDrive`, `Snapshot.runsClaude`·`atPrompt`(§6.1). `headless` 는 세션 파일 값이되 `tty` 가 있으면 false 다. 탐침은 3초마다 돌아 손으로 친 `claude` 는 3초 안에 짝이 지어지고, 그 전에는 `appName` 이 없어 잘못된 칩을 그리지 않는다.
- 프론트 `terminal/place.ts`(순수): 끝나지 않은 세션의 `tty` 가 화면의 `termId` 와 같으면 그 화면 소속이고 화면의 `sessionId` 를 그 세션으로 따라가게 한다(`/clear`·손으로 친 claude). 같은 `tty` 로 짝지어 온 세션이 여럿이면 `paired` 에는 모두 남지만 주 세션은 하나뿐이다: `(updated, since, 세션 id)` 사전식으로 큰 쪽(hook 기록이 가장 늦은 것, 그마저 같으면 화면에 온 지 가장 오래된 것, 같으면 세션 id 글자 차례가 뒤인 것)이 주인이고, 화면의 `sessionId` 고침은 주인만 낸다(세션 목록 차례와 관계없이 정한다, mac 규칙과 같다). 살아 있는 headless 세션의 `tabAncestor` 가 화면을 가리키면 소속(+ `CB -p` 근거). 명령 없는 셸 화면에 `mode` 없는 주 세션이 짝지어 오면 `command = "claude"`, claude 가 도는 흔적이 없는데 명령이 남아 있으면 비운다. 셸에 claude 명령을 친 지 3초(`TYPED_WINDOW_MS`) 안이면 돌고 있는 것으로 본다.
- 화면이 기억하는 세션 id로 붙는 길(`placeOf` 의 held): 화면의 `sessionId` 와 세션 id 가 같고 그 화면에서 claude 가 도는 중(`runsClaude`)이거나 3초 안에 명령을 쳤을 때만 그 화면 소속(`tab`)이다. 끝난 세션도 같은 조건으로 붙는다. 그 화면에서 claude 가 안 도는 셸 화면에 남은 옛 id 는 그 세션을 잡지 않으므로, 다른 터미널에서 이어 열어 온 세션이 보드 탭 세션으로 보이지 않는다(mac PR #25 규칙). 그 밖에 끝냈으면 `ended`, 살아 있는 headless 는 `background`, 나머지는 `terminal`.
- 경로 줄의 상태 점(§7.2)은 그 경로 탭들의 상태 가운데 가장 급한 것 하나다. 순위는 대기 0 · 작업 중 1 · 일시중지 2 · 대기(idle) 3 · 끝남 4 로, 일시중지가 대기보다 앞선다(카드 순위와는 다르다, mac PR #20 P2 규칙).
- 칩 문구(`board/placeChip.ts`, mac 과 같은 글자):

  | 위치 | 칩 | 힌트 |
  |---|---|---|
  | `tab` | `CB`(mode 가 있으면 `CB -p`·`CB --bg`) | 대화형: "보드 탭에서 도는 세션: 누르면 그 탭으로". headless·mode: "보드 탭 안에서 시작된 claude <mode, 없으면 -p> (탭이 닫히면 함께 끝납니다)" |
  | `terminal` | `<appName>` (이름을 알기 전에는 칩 없음) | "<appName> 에서 도는 세션" |
  | `background` | `$ claude <mode, 없으면 -p>` | "터미널 없이 도는 세션입니다. 보드는 상태만 보여 줍니다" |
  | `ended` | 대화형: "보드에서 이어 열기". headless: `$ claude -p` | 대화형: "누르면 보드 새 탭에서 이 대화를 이어 엽니다". headless: "끝난 백그라운드 세션입니다. 메뉴의 세션 다시 열기로 보드 탭에서 엽니다" |

  카드는 `background` 세션을 따로 묶는다(구역 머리글).
- 알려진 한계: 탭 안에서 분리해 띄운 `claude -p` 의 부모가 살아 있는 동안은 `tabAncestor` 로 짝이 지어져 칩이 `CB` 로 보이고, 부모가 끝나면 `$ claude -p` 로 바뀐다(§11.4 G4). `atPrompt` 는 최대 3초 묵은 값이라 방금 다른 프로그램을 띄운 화면이 잠깐 빈 셸로 보일 수 있다(`typedAt` 와 `runsClaude` 로 막지만 없애지는 못한다). 실제 Claude Code 의 `claude --bg` 프로세스 모양(숨은 콘솔인지 분리인지)은 확인하지 못했다.

#### 5.12.8 크기 바꾸기 장벽 (`ResizeBarrier`)

ConPTY 는 크기를 바꾸면 화면을 다시 그리는 바이트를 흘리는데, xterm 의 크기를 그 바이트의 어느 자리에서 바꾸느냐에 따라 줄이 깨진다. xterm 크기는 ConPTY 의 다시 그리기 머리말 자리(cut)에서만 바꾼다.

- 흐름(`resizeBarrier.ts`, 한 번에 단계 하나, 요청은 슬롯 하나에 합친다): 계획(쓰기 대기열을 비움) → `term_resize`(기준 오프셋을 받음) → 스캐너에 출력을 먹이며 머리말 찾기(`repaintScanner.ts`) → cut 앞까지 xterm 에 쓰고 쓰기가 모두 해석된 뒤 `applyLocal`(= `term.resize`) → 붙잡은 프레임을 풀고 다음 단계 또는 최신 요청으로 다시 계획. 프레임은 xterm 에 쓰기 전까지 `done` 을 부르지 않는다(생산자의 크레딧 창이 막는다).
- 머리말: `(ESC[?25l)? (ESC[8;R;Ct)? (ESC[n;1H)? LF? ESC[H`. 크기 알림이 있으면 목표 크기와 같아야 한다. 머리말 없는 맨 `ESC[H` 는 대체 화면에서 기준 오프셋 − 64 바이트로부터 4 KiB 안에서만 받는다. cut A = 그 `ESC[H` 바로 앞, cut B = 그 뒤 첫 `ESC[?25h` 바로 뒤(예산 `rows*(cols*4+16)+256` 바이트 안, 넘기거나 없으면 다음 LF 뒤). 크기 알림은 항상 오지 않으므로(행만 바꾸는 둘째 단계는 알림이 없다) 머리말은 알림이 아니라 `ESC[H` 로 찾는다.
- 모양별 단계표(`resizeRules.ts planSteps`, 근거는 §11.5 의 측정):

  | 모양 | 계획 | 측정 결과 |
  |---|---|---|
  | 대체 화면(TUI) 어떤 모양이든 | 한 단계, cut A | 합성 tui 부하에서 모든 모양 0/5 |
  | 열만 줄임 | 한 단계, A | 0/5 |
  | 열만 늘림 | 한 단계, B | 0/5, ascii 만 한 줄 깨짐 1/5 |
  | 행만 바꿈 | 한 단계, A | 늘림 0/5, 줄임은 ascii 3/5(30줄)로 최선 노력 |
  | 열 줄고 행 늘림 | 두 단계: 열(지금 행) A → 행 A | 0/5 |
  | 열 줄고 행 줄임 | 한 단계, A | ascii 5/5(50줄), 최선 노력 |
  | 열 늘고 행 바뀜 | 두 단계: 행(지금 열) A → 열 B | 행 늘림 0/5, 행 줄임은 ascii 5/5(25줄)·ko 4/5(20줄), 최선 노력 |

- 기한과 폴백(상수는 `resizeBarrier.ts`): 머리말을 1000ms(`PREAMBLE_DEADLINE_MS`) 안에 못 찾거나 기준 뒤 64 KiB(`SCAN_BUDGET_BYTES`)를 지나면 줄 끝(LF 뒤)에서 자르고 `board.log` 에 한 줄을 남긴다. 스트림이 50ms(`TAIL_IDLE_MS`) 조용하면 그 끝에서 자른다(CR 로 끝난 스트림은 4배 기다린다). 줄 끝 폴백도 1500ms·256 KiB(`FALLBACK_MAX_*`) 안에 끝내야 한다. `term_resize` 가 실패하면 500ms(`RETRY_MS`) 뒤 다시 하고 세 번 실패하면 단계를 버린다. `gap`·`exit`·터미널 다시 붙기(`bind`, 강제 요청)·`reset` 은 장벽을 거쳐 처리한다.
- 쓰기 장부(`WriteLedger`): 아직 콜백이 안 온 쓰기를 세어, cut 앞 바이트가 모두 해석되기 전에는 `term.resize` 를 부르지 않는다.
- 한계(정직한 잔여): 단계표 측정은 합성 부하 5종(ascii·ko·ko-odd·tui·ink)이고 실제 Claude Code 화면으로는 측정하지 않았다. 빌드 26300 한 대의 conhost 만 측정했고 다른 빌드·Windows 10·sideload `conpty.dll` 은 측정하지 않았다. 캡처는 셀마다 5개라 0/5 는 깨지지 않는다는 증명이 아니다. 행만 줄임, 열 줄고 행 줄임, 열 늘고 행 줄임은 깨진 줄이 남는 최선 노력이다. 보드 안 끝에서 끝까지 측정(§11.5 G2b)에서도 열 늘림 계열은 줄 하나에 빈칸이 생기고, 경계를 끌어 열을 줄이면 줄 하나가 잘렸다.

#### 5.12.9 탭 저장과 복원 (`restore.ts`·`lifecycle.svelte.ts`, 백엔드 `shutdown.rs`·`proc_wait.rs`)

보드를 끄고 다시 켜면 탭이 돌아온다(mac `saveTabs`·`restoreTabs`·`resumeHeld`). 규칙은 `restore.ts` 의 순수 함수이고 `lifecycle.svelte.ts` 가 그 결과를 허브와 백엔드에 실행한다.

- 저장 모양은 설정 `terminalTabs`(§5.9)의 `TabEntry` 목록이다: `{ workspace, name | null, selected, root }`. `root` 는 `SavedNode`(`{ leaf }` 또는 `{ split, ratio, first, second }`), 잎 `SavedSurface` 는 `{ cwd, command | null, sessionId | null, mdRenderOff }`.
  읽을 때 디스크 값은 믿지 않는다. 모양이 맞는 항목만 받고 깨진 항목은 건너뛰어 하나가 보드 시작을 막지 않는다(`parseEntries`). 비율은 0.1~0.9 로 자른다.
- 저장하는 값: 셸만 있는 화면은 경로(cwd)만. claude 화면은 명령·세션 id·꾸미기 끔까지. 명령이 끝나 셸만 남은 화면(`command` 가 null)은 셸로 저장한다. 끝난 세션은 다시 이어 열 대상이 아니다. 아직 들어가지 않아 이어 열지 않은 화면(`held`)은 쥔 값을 그대로 다시 저장한다.
- 저장 시점: 탭 목록·화면·터미널 경로·고른 탭이 바뀔 때마다 300ms 모았다가 `save_tabs` 로 통째로 바꿔 넣는다. 같은 내용이면 보내지 않는다. 보내는 일은 한 줄로 세워(`serialSender`) 앞의 것이 끝난 뒤 다음을 보내고, 하나가 실패해도 뒤의 것을 막지 않는다. 끌기·업데이트를 고른 뒤 10초 동안은 저장하지 않는다. 셸이 먼저 죽어 탭이 닫혀 가는 모습을 저장하면 목록이 줄어들기 때문이다.
- 시작 결과는 넷이고 `restored`·`fresh` 일 때만 저장 목록을 덮어쓸 수 있다.
  `restored` 저장 항목으로 탭을 다시 세웠다 · `fresh` 저장 항목이 없다 · `adopted` 화면을 새로 고쳐 도는 터미널을 이어받았다(명령·세션 id·탭 이름을 잃은 맨 셸이라 그 모양을 저장하면 되살릴 탭이 사라진다. board.log 에 "저장해 둔 탭 목록은 덮어쓰지 않습니다") · `unreadable` 저장 목록이나 작업 경로를 읽지 못했다.
- 되살리기(`restorePlan`): 모든 화면은 우선 셸로 돌아오고, claude 화면의 이어 열 값은 `held`(`cwd`·`command`·`session`·`mdoff`)에 담긴다. 저장 때 고른 탭(표시가 없으면 마지막 탭)의 고른 화면만 바로 이어 열고(`resumeNow`), 나머지는 그 탭을 고를 때 이어 연다(`resumeHeld`). 이어 열기 전의 화면은 셸도 아직 뜨지 않은 상태다(첫 표시 때 셸을 띄운다, §5.12.4).
- 앞 보드가 남긴 세션 기다리기: 저장된 세션 id 가운데 첫 상태 묶음(최대 3초 기다림)에서 끝나지 않은 세션의 pid 와 생성 시각(`pid_ctime`)을 `pids_alive` 로 200ms 마다 묻는다. 5초 뒤에도 살아 있으면 보드 밖에서 같은 세션이 도는 것이라 그 화면은 되살리지 않는다(업데이트 직후 옛 보드의 claude 가 아직 끝나는 중인 경우). 생성 시각이 있으면 재사용된 pid 를 산 것으로 보지 않고, pid 를 모르는 세션은 기다림 대상이 아니다.
- 작업 폴더 확인(`existing_dirs`): 백엔드가 절대 경로이면서 실제 폴더인 것만 있다고 답한다(상대 경로는 어느 폴더인지 모르므로 없는 것으로 본다). claude 화면의 탭 작업 경로나 저장한 cwd 가 없으면 그 화면은 되살리지도 이어 열지도 않고 board.log 에 `탭을 되살리지 않음: 작업 경로가 없습니다 <경로>` 를 남긴다. 없는 폴더에서는 셸이 홈 폴더로 떠서 권한 확인을 건너뛰는 claude 가 홈 폴더 전체를 열게 되기 때문이다(mac `restoreCommand` 도 같다).
  홈(`~`)과 빈 경로는 늘 있는 것으로 본다. 폴더를 확인하지 못해도 되살리지 않는다(`unreadable`). 되살리지 않은 화면은 트리에서 빠지고 형제가 그 자리를 차지하며, 한 탭의 화면이 모두 빠지면 그 탭도 없다. 셸만 있던 화면은 홈에서 셸로 돌아온다.
- 이어 열기(`heldLaunch`): 이어 열 순간에 정한다(상태 묶음은 1초까지 묵는다). 세션이 보드 상태에 있고 끝나지 않았으면 `pids_alive` 로 지금 생사를 다시 묻는다. 살아 있으면 보드 밖에서 이어 열린 것이라 셸로 둔다. 방금 죽은 옛 보드의 claude 는 끝난 것으로 본다.
  세션이 상태에 있고 끝났으면 `claude <이어 열기 인자> --resume <id>`(§5.7, `resumeArgs` 에서 md-render `--plugin-dir` 쌍을 떼고 허용 글자 인자만 남긴다), 상태에 없으면(대화 없이 끝나 지워졌다) 처음 명령을 새 세션 id 로, 상태 묶음을 아직 못 받았으면 인자 없이 `--resume`. 모두 `--name <탭 이름>` 을 붙이고 §5.12.11 의 `--plugin-dir` 규칙을 거친다.
  셸이 못 떴으면(`failed`·`exited`) 값을 쥐고 있다가 뜨면 이어 연다. 이어 열기를 물어보는 사이에도 `held` 를 쥐고 있어 저장 목록이 그 화면을 셸로 적지 않는다. 이어 열린 세션의 `.paused` 는 새 요청이 오면 hook 이 지운다.
- 알려진 한계(`untested:`/`unchecked:`): 웹뷰를 새로 고치면 아직 들어간 적 없는 복원 탭이 화면에서 사라진다(도는 터미널만 이어받기 때문이다). 저장 목록에는 남아 다음 보드 시작에 돌아온다. 작업 폴더가 지워진 탭은 화면에서 조용히 빠지고, 저장 목록이 `restored` 로 다시 쓰이면 목록에서도 빠져 폴더가 돌아와도 돌아오지 않으며, 그 세션의 `.paused` 는 정리되지 않고 남는다. 화면 나누기 트리의 저장은 단위 시험(`restore.test.ts`)만 거쳤고 GUI 에서는 탭마다 잎 하나만 있다. 실제 Claude Code 가 이어 열기를 처리하는 모양은 확인하지 못했다.

#### 5.12.10 보드를 끌 때 확인 (`shutdown.rs`·`shutdownList.ts`·`ShutdownList.svelte`)

트레이 **종료**와 업데이트(§5.8)가 보드를 끄기 전에, 보드 탭에서 도는 세션이 있으면 목록을 보이고 끌지 묻는다(mac `confirmShutdown`·`ShutdownList`). 확인 창은 보드 창 안의 Svelte 모달이다(네이티브 대화상자가 아니다). 그래야 DevTools 프로토콜로 읽고 찍을 수 있다.

- 흐름: 백엔드 `confirm` 이 이벤트 `board://confirm-shutdown {reason: "quit"|"update", items: []}` 를 보낸다(`items` 는 프론트가 채운다). 프론트는 먼저 탭 목록 저장을 시작하고 `shutdownList()` 로 목록을 만든다.
  목록이 비면 저장이 끝난 뒤 창 없이 `answer_shutdown(true, [])` 로 답한다. 있으면 보드가 숨어 있거나 접혀 있어도 보이게 하고(`showBoard`) 모달을 세운 뒤 `shutdown_dialog_shown` 으로 알린다. 사용자가 고르면 `answer_shutdown(ok, pause)` 를 마지막 저장이 끝난 뒤에 부른다. 답을 받으면 백엔드가 곧 끝내므로 먼저 답하면 마지막 변경이 사라진다.
  계속(ok)이면 고른 순간의 세션으로 `.paused` 대상을 다시 센다(창이 떠 있는 사이 세션이 끝났거나 새 일을 시작했을 수 있다).
- 창: 제목 `작업 중인 세션 <n>개가 꺼집니다`, 줄마다 점·`<폴더 이름> · <세션 제목>`·까닭(작업 중 · 승인 대기 · 서브에이전트 실행 중 · 셸에서 실행 중 · claude -p), 단추 **종료**(업데이트면 **업데이트**)와 **취소 (Esc)**. 처음 포커스는 취소 단추에 있어 Enter 가 뒤에서 도는 프로그램으로 가지 않는다. 창 밖 키는 삼키고 Esc 는 xterm 이 먹기 전에 캡처 단계에서 받는다.
- 기다림의 상태기계(`Handshake`, 우편함 한 칸): 이벤트를 보낸 뒤 프론트의 첫 반응(답 또는 "창을 띄움")을 `REACTION_TIMEOUT` = 10초 기다린다. 반응이 없으면 `TimedOut`: 프론트가 없거나 멎은 것이라 board.log 에 남기고 `.paused` 없이 그대로 진행한다.
  "창을 띄움" 뒤에는 사용자가 고를 때까지 기다리되 `ANSWER_TIMEOUT` = 15분을 넘기면 `AnswerTimedOut`: 끄지 않고 취소하며 이벤트 `reason: "cancel"` 로 창을 닫게 한다. 두 시간은 판단값이고 실측이 아니다.
  새 요청(트레이 **종료**를 또 누름)이 오면 기다리던 요청은 `Cancelled` 가 되고 새 요청이 이어받는다: 숨은 보드를 보이고 이벤트를 다시 보내 창을 다시 세운다. 프론트가 시작할 때(`lifecycle_started`) 기다리던 요청도 취소한다. 이벤트를 받을 화면이 바뀌었기 때문이다. 프론트가 `lifecycle_started` 를 한 번도 부르지 않았으면 묻지 않고 지나간다.
  끄기는 `Answered(ok)` 와 `TimedOut` 에서만 진행한다. 취소·`Cancelled`·`AnswerTimedOut` 에서는 보드가 그대로 남는다. 창이 떠 있는 동안 화면이 다시 뜨거나 답이 오래 없어도 사용자가 한 번 더 **종료**를 눌러 빠져나올 수 있다.
- 목록 규칙(`shutdownList.ts`): 보드 탭에서 도는 세션만 꺼진다. 백그라운드·다른 터미널의 세션은 영향이 없어 목록에 없다. 한 세션이 여러 까닭에 걸려도 줄은 하나이고 위에서부터 `headlessInTab`(탭 안 `claude -p`, 늘) > `waiting` > `working` > `subagents`(끝나지 않은 서브에이전트) > `shell`(Bash 백그라운드 셸) 순이다.
  쉬는 주 세션은 줄이 아니다. 더해서 claude 가 아닌 프로그램이 도는 셸 탭(프롬프트에 있지 않고, `runsClaude` 에도 없고, 짝 세션도 없고, 3초 안에 명령을 친 것도 아님)은 세션 없는 `shell` 줄이다. 줄이 하나도 없으면 묻지 않는다.
- `.paused` 표지: 줄이 있는 화면의 주 세션(그 화면 콘솔에서 도는 대화형 `mode` 없는 비 headless 세션)에 남긴다. 주 세션이 쉬어도(서브에이전트·셸·`-p` 만 돌아도) 남는다. 탭 안의 `claude -p` 는 입력 자리가 없어 남기지 않는다.
  백엔드 `write_marks` 가 한 번 더 거른다: 알려진 세션이 아니거나 끝났거나 headless 이거나 `mode` 가 있으면 건너뛰고, 이미 표지가 있으면(사용자가 일시중지해 둔 세션) 덮지 않는다(`create_new`). 표지 내용은 `{paused_at, topic}` 이고 경로는 `terminal.rs paused_mark` 다. 다시 뜬 보드는 이 표지로 "세션 재개"를 보인다(§5.7).
- 업데이트 경로: §5.8. 업데이트할 것이 있을 때만 묻고, 취소하면 시작하지 않으며, 스크립트가 돌아오면 표지를 지운다.

#### 5.12.11 답변 꾸미기 (md-render), 앱 쪽 (`mdrender.rs`·`mdRender.ts`·`NewSessionSheet.svelte`)

보드 탭에서 띄우는 claude 에 답변 꾸미기 플러그인(`plugins\md-render`)을 `--plugin-dir` 로 붙인다(mac `mdRenderDir`·`mdRenderCommand`·`mdRenderOff`).

- 옮겨 쓰기: 설치 폴더의 사본(`ClaudeBoard.exe` 옆 `plugins\md-render`, Tauri `bundle.resources`)을 `%USERPROFILE%\.claude\notifier\plugins\md-render\` 로 옮기고 그 경로를 넘긴다. 설치 폴더 안 경로를 바로 넘기지 않는 까닭: 대화형 claude 가 플러그인 폴더에 `tsconfig.json`·`.claude-plugin\types\` 를 쓰고, 업데이트가 설치 폴더를 바꿔 끼워도 경로가 유지되게 하려는 것이다.
- 옮기는 것은 다섯 파일(`PLUGIN_FILES`: `LICENSE`·`README.md`·`.claude-plugin/plugin.json`·`hooks/hooks.json`·`hooks/register.tsx`)뿐이고 `tests\` 는 옮기지 않는다. 내용(sha256)이 다른 것만 덮으며, 목적지의 그 밖 파일(claude 가 만든 것)은 건드리지 않는다. 같은 내용이면 둘째 시작은 아무것도 옮기지 않는다.
  파일은 옆 파일(`.cb-partial`)에 쓰고 바꿔 끼워 반쪽 파일이 남지 않는다. 원본 다섯 중 하나라도 없으면(깨진 설치) 아무것도 옮기지 않고 실패한다. 목적지·하위 폴더·옮길 파일이 심볼릭 링크나 정션이면 따라 쓰지 않고 거절한다(링크 너머의 다른 폴더를 덮지 않으려는 것이다).
- 결과는 처음 부를 때(`md_render_dir`) 옮기고 성공만 쥔다. 실패(파일이 잠김 등)는 쥐지 않아 다음 호출이 다시 시도하고, 이유는 board.log `답변 꾸미기 플러그인을 옮기지 못했습니다` 에 남는다. 프론트는 시작할 때 한 번만 부르므로 실제 재시도는 다음 보드 시작이나 화면 새로 고침 때다. 옮기지 못했으면 `--plugin-dir` 을 붙이지 않고 새 세션 창의 "답변 마크다운 꾸미기" 체크는 잠긴다.
- 명령 규칙(`mdRenderCommand(command, pluginDir, off)`): claude 명령이 아니면 그대로. 값이 `...\plugins\md-render` 로 끝나는 `--plugin-dir` 은 모두 걷어 내고(경로는 대소문자·`\`·`/` 를 가리지 않고, 따옴표 없음·큰따옴표·작은따옴표 꼴 모두), 켜져 있고 폴더를 알 때만 claude 낱말 바로 뒤에 하나 붙인다. 사용자가 적은 다른 `--plugin-dir` 은 둔다.
  폴더는 큰따옴표로 묶는다. 따옴표 안에서도 셸이 풀어 버리는 글자(큰따옴표·`%`·`$`·백틱·`^`·`!`·굽은 따옴표·제어 문자·끝의 역슬래시)가 있으면 그 폴더는 쓰지 않는다(`quotePluginDir`). 시작 때 폴더를 한 번 받아 두며(`loadPluginDir`) 첫 명령을 치기 전에 기다린다.
- 끄기: 새 세션 창에서 체크를 풀면 그 세션 id 를 설정 `mdRenderOff`(§5.9)에 넣고(`remember_md_render_off`) `--plugin-dir` 없이 시작한다. 같은 세션을 이어 열 때(다시 열기·되살리기·보드로 옮기기)도 끈 채로 연다. 저장한 탭 항목의 `mdRenderOff` 도 같은 뜻이다. 보드를 켜고 첫 상태 묶음을 받으면 세션 파일이 없어진 id 는 `prune_md_render_off` 로 목록에서 치운다.
- 설치 배치와 제거: §8.1, §3.7, §8.2.
- 확인한 것은 §11.2.1 의 G10 이다. `untested:` 실제 Claude Code 가 플러그인으로 답변을 다시 그리는 것, PowerShell 5.1 에서 `&` 가 든 경로를 npm 의 `claude.cmd` 중계가 어떻게 받는지, setup.exe 로 실제 설치한 폴더 배치(NSIS 스크립트를 읽어 확인했을 뿐 설치하지 않았다).

#### 5.12.12 보드로 옮기기와 알림에서 세션으로 (`moveToBoard.ts`·`sessionFlow.svelte.ts`·`notify.rs`)

다른 터미널에서 도는 세션을 끝내고 보드 새 탭에서 같은 대화를 이어 연다(mac `moveToBoard`).

- 메뉴: 세션 줄 … 메뉴의 **보드로 옮기기**("그쪽을 끝내고 보드 새 탭에서 이어 엽니다")는 위치가 `terminal` 이고 `canDrive` 인 세션에만 있다.
- 순서: 누른 순간의 세션을 다시 읽는다(메뉴를 연 뒤 바뀌었을 수 있다). 이미 끝났으면 알림 "세션이 이미 끝났습니다". 작업 중(`working`)이거나 승인 대기(`waiting`)이면 먼저 "보드로 옮길까요?"(옮기기 · 취소)를 터미널 영역의 확인 창으로 묻는다. 터미널이 접혀 있으면 펼쳐 보이고, 취소하면 도로 접는다. 쉬는 세션은 묻지 않는다.
  그다음 그 콘솔에 Esc → 400ms → `/exit` → 200ms → Enter(`drive_session`)를 넣고, 그 프로세스가 끝나기를 `wait_exit(pid, pid_ctime, 8000)` 으로 기다린 뒤 보통의 "세션 다시 열기"와 같은 길(`session_action reopen`, 이어서 진행 문구 없음)로 보드 탭에서 `--resume <id>` 로 연다.
- 실패는 알림 한 줄(6초)이다: 키를 넣을 곳을 못 찾으면 "<폴더>: 세션 터미널을 찾지 못해 보드로 옮기지 못했습니다", 8초 안에 안 끝나면 "<폴더>: 세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요". 같은 세션을 옮기는 중에 또 누르면 거른다. 키를 두 번 보내지 않는다.
  `wait_exit` 는 100ms 마다 `proc::alive(pid, pid_ctime)` 로 보고, pid 0 은 어떤 프로세스인지 모르므로 기다리지 않고 끝나지 않은 것으로 답한다.
- 알림 패널에서 세션으로: hook 이 세션 id 를 알면 패널을 `--session <id>` 와 함께 띄운다. 패널을 누르면 `ClaudeBoard.exe --action focus <id>` 가 실행 중인 보드로 간다(단일 실행). 보드 탭의 세션은 그 탭을 고르고 다른 터미널의 세션은 그 창으로 간다(§5.7 `focus`).
  `focus` 는 보드가 숨어 있거나 세션이 보드 밖(다른 터미널·백그라운드)이어도 보드를 보인다(포커스는 가져가지 않는다). 세션을 못 찾으면 board.log 에 `--action focus: 세션을 찾지 못했습니다 (<id>)` 만 남기고 탭은 그대로다. 세션 id 가 없는 패널은 전과 같이 `--show` 다. 보드가 막 켜지는 중에 온 동작은 첫 상태 묶음 뒤에 처리한다.
- `untested:` 실제 Claude Code 가 `/exit` 를 "Background work is running" 으로 막는 경우, 입력 칸에 쓰다 만 글이 있을 때(`/exit` 가 이어 붙어 질문으로 전송될 수 있다, mac 도 같다), 승인 대기 상태 세션의 확인 창, Windows Terminal 안의 세션(G10 은 클래식 콘솔만), 둘째 모니터의 알림 패널.

#### 5.12.13 경로 줄·탭 줄 가로 밀기 (`stripScroll.ts`·`TerminalWorkspaceBar.svelte`·`TerminalTabBar.svelte`)

mac 3.1.7(탭 줄)·3.1.8(경로 줄)의 가로 밀기다. 두 줄이 같은 규칙을 쓰는 순수 모듈 하나(`stripScroll.ts`)를 공유한다.

- 휠(`wheelStep`): 가로 값이 오거나(`deltaX ≠ 0`, 트랙패드 가로 밀기·브라우저가 Shift+휠을 가로 값으로 바꿔 준 경우) 값이 없으면 브라우저에 맡긴다. 세로 값만 오면 그만큼 우리가 밀고 기본 동작은 막는다. 안 막으면 Shift+휠이 두 번 밀린다.
- 보이게 밀기(`scrollTargetClamped`): 고른 항목이 양끝 여백 8px 까지 보이도록 가장 가까운 만큼만, 0 에서 끝 사이로 민다.
- 경로 줄: 이름을 줄이지 않고 전체를 보이며 넘치면 가로로 민다. 지금 경로가 바뀔 때, 경로 개수가 바뀔 때, 칸 폭이 바뀔 때 지금 경로를 보이게 맞춘다. 경로 목록 배열이 아니라 개수만 의존한다. 배열은 상태 묶음마다(약 1초) 새로 만들어지므로 그 자체를 보면 손으로 민 스크롤을 지금 경로로 되돌려 버린다.
- 탭 줄: 탭 폭 120~250. 다 안 들어가면 가로로 밀고 고른 탭이 보이게 따라간다. **+ Ctrl+T** 단추는 밀리는 칸 밖이라 늘 오른쪽 끝에 보인다.
- 확인 결과와 남은 한계는 §11.2.1 이다(경로 줄이 손으로 민 스크롤을 되돌리던 것을 고친 뒤의 GUI 재확인은 아직 없다).

### 5.13 GUI 시험 장치 (`windows/ci/gui`·`windows/ci/jev`·가짜 claude)

보드를 GUI 로 시험할 때 지키는 두 규칙: 진짜 claude 를 띄우지 않고, 사용자 환경(홈·`settings.json`·PowerShell 프로필·설치한 보드)을 바꾸지 않는다. 아래 장치가 그 규칙을 코드로 막는다.

- 시험용 claude 바꿔치기(`CLAUDE_BOARD_CLAUDE`, `test_claude.rs`·`testClaude.ts`): 값은 `|` 로 이은 절대 경로(프로그램과 앞쪽 인자)다. 켜지는 조건은 환경변수 `CLAUDE_BOARD_HOME` 도 있을 때뿐이고, 모든 조각이 비지 않은 절대 경로이고 큰따옴표가 없으며 실재해야 한다. 사용자 보드에 남은 변수 하나가 진짜 claude 를 바꿔치지 못하게 하려는 것이다.
  판정은 셋이다: 꺼짐(변수 없음) · 켜짐 · 어긋남(변수는 있으나 위 조건을 어김). `test_claude_prefix` 는 꺼짐이면 `null`, 켜짐이면 조각 배열, 어긋남이면 `{invalid: 이유}` 를 준다. 어긋남을 꺼짐으로 읽으면 진짜 claude 를 치게 되므로 가른다.
- 닫힌 채로 실패한다: 프론트는 시험 모드 여부를 아직 받지 못했거나(`unloaded`) 어긋났으면 어떤 명령도 치지 않고 board.log 에 `[시험 claude] 명령을 치지 않았다: <이유>` 를 남긴다. 묻는 데 실패해도 어긋남으로 둔다. 시작할 때 첫 명령 전에 이 답을 기다려 둔다(첫 명령에서 진짜 claude 가 뜨는 경쟁을 없앤다).
  보드가 직접 띄우는 계정 추가 로그인도 같은 판정을 따른다(`login_program`): 꺼짐이면 진짜 claude 를 찾고, 켜짐이면 가짜, 어긋남이면 진짜를 찾지도 띄우지도 않고 거절한다.
- 바꾸는 것은 명령 맨 앞의 `claude`·`claude.exe`·`claude.cmd` 한 낱말뿐이다(경로가 붙은 것, 큰따옴표로 묶은 것 포함). 인자는 건드리지 않는다. 조각은 셸에 맞게 묶는다: pwsh·powershell 은 `& '<조각>' '<조각>'`(작은따옴표 안은 글자 그대로), cmd 는 `"<조각>" "<조각>"`.
- 가짜 claude: `ci/fake-claude-tty.cjs` 는 키를 받아 적는 가짜 Claude Code 다(`FAKE_CLAUDE_KEYLOG` 에 받은 바이트, `FAKE_CLAUDE_ARGVLOG` 에 시작 인자). 등록된 Claude Board hook 을 실제로 불러 세션 파일과 `.paused` 처리는 진짜 코드 길을 지난다. 보드 탭의 셸은 보드의 환경변수를 물려받지 않으므로 가짜는 자기 옆 `fake-claude.env.json` 으로 빠진 변수를 채운다(실제 환경변수가 이긴다).
  `ci/fake-claude-login.cjs` 는 `claude auth login --claudeai` 를 흉내 낸다. `CLAUDE_CONFIG_DIR` 이 없으면 아무것도 쓰지 않고 종료 코드 2 로 끝나고, 로그인 주소는 예약된 `.test` 도메인이라 어디에도 닿지 않는다.
- `ci/gui/launch-board.ps1`: 빌드한 `ClaudeBoard.exe` 를 격리해 띄운다(`CLAUDE_BOARD_HOME`·`WEBVIEW2_USER_DATA_FOLDER`·CDP 포트·`CLAUDE_BOARD_CLAUDE` 를 보드 프로세스의 환경으로만 준다). 이미 `ClaudeBoard.exe` 가 떠 있으면(사용자 보드일 수 있다) 끄지 않고 거절한다(종료코드 3). 띄운 뒤 보드의 `test_claude_prefix` 가 기대한 값인지 확인하고 다르면 띄운 보드만 끈다(종료코드 5). `-Stop` 은 이 스크립트가 띄워 기록한 보드만 끈다. 종료코드는 파일 머리말이 소유한다.
- `ci/gui/tripwire.ps1`: 시험 앞뒤로 사용자 `settings.json` 과 PowerShell `profile.ps1` 의 md5(해시만 기록한다), 그리고 시험 중 진짜 claude 가 새로 뜨지 않았는지를 본다. 후자는 기준선 비교다: `before` 가 이미 도는 claude(이름이 `claude.exe` 인 프로세스, 명령줄에 `@anthropic-ai/claude-code` 가 든 `node.exe`·`bun.exe`)의 pid 와 생성 시각을 적고, `after`·`check` 는 기준선에 없는 새 것이 가짜 폴더 밖이면 실패(종료코드 1)한다. 보드가 이미 끝났어도 판정한다. 시험 전부터 돌던 다른 세션의 claude 는 `OTHER-CLAUDE` 참고 줄이고 실패가 아니다.
- `ci/gui/cdp.mjs`: 격리한 보드의 WebView2 에 붙는 최소 CDP 도구(`targets`·`eval`·`shot`). 127.0.0.1 밖으로 나가지 않고 캡처는 보드 콘텐츠만 담는다.
- Jev(`ci/jev`): 터미널은 WebGL 캔버스라 DOM 으로 못 읽는 픽셀 사실을 Jev 에 묻는 조언자다. 모든 시험 행은 결정적 증거(DOM 읽기·키 기록·argv 기록·파일 존재·`term.buffer` 검사)를 따로 갖고 Jev 는 별도 칸이다. Jev PASS 는 결정적 FAIL 을 PASS 로 바꾸지 못하고 Jev FAIL·UNSURE 는 자동 실패가 아니다. 어긋나면 사람이 그림을 열어 읽는다.
  판정 낱말은 PASS·FAIL·UNSURE·UNVERIFIED 이고 UNVERIFIED 는 통과가 아니다. 종료코드는 0 PASS · 1 FAIL · 2 사용법 · 3 UNSURE · 5 GUARD · 6 UNVERIFIED 다. 종합 표(`report`)의 종료코드는 결정적 판정만으로 정하며 전부 PASS 일 때만 0 이다.
  보내기 전 가드: 그림은 `windows/e2e/out/`·`windows/e2e/artifacts/` 아래의 PNG(헤더 확인, 크기 한도)여야 하고, 한 번만 읽어 검사한 바이트의 임시 사본만 안쪽에 준다. 보내는 글에 사용자 이름·`\Users\` 경로·이메일·32자 이상 토큰이 있으면 거절한다(`domain/privacy.mjs`, 까닭 줄에도 걸린 글자를 적지 않는다). 전체 화면 캡처는 `crop --window-title` 로 보드 창만 잘라 그림 옆 manifest(창 제목·사각형·sha256)에 묶고, `--allowed-window-title` 이 있으면 그 manifest 가 맞을 때만 지나간다. 판정 캐시 키에는 판정자 id·version 이 들어간다.
  키: 이 저장소의 코드와 에이전트는 키를 읽지도 쓰지도 넘기지도 않는다. 하네스 `jevvision` 이 Windows 자격 증명 관리자의 일반 자격 증명 `typesafe-api-key`(또는 환경변수 `TYPESAFE_API_KEY`)에서만 읽는다. 파일이나 명령줄 인자로 받지 않는다(`cmdkey /pass:<값>` 도 쓰지 않는다). 키가 없으면 `UNVERIFIED|no Jev key`(종료코드 6)다.
- `untested:` Jev 를 실제로 부른 적이 없다(이 PC 에서 하네스의 Windows 조회는 자리표시자이고 키가 없다). 모든 질문 가족이 `not calibrated` 다. 하위 프로세스 인자 전달(따옴표가 든 진술)은 Node 스텁으로만 확인했다. 시험 명령: 철선·launch-board·CDP 도구는 `node --test "windows/ci/gui/*.test.mjs"`(디렉터리 인자는 이 폴더에서 시험 파일을 찾지 못한다), Jev 는 `node --test windows/ci/jev/`, 가짜 claude 는 `node --test windows/ci/fake-claude-tty.test.cjs windows/ci/fake-claude-login.test.cjs` 다.
- 알려진 한계(`unchecked:`): `launch-board.ps1` 은 보드가 뜬 뒤에 시험 모드를 확인한다. 저장한 탭이 있는 격리 폴더를 다시 띄우면 그 확인이 끝나기 전에 보드가 이어 열기 명령을 칠 수 있다. 이 경우 보드 자신의 닫힌 채 실패(위)가 같은 환경으로 가짜를 쓰게 막지만, 순서는 스크립트가 증명하지 않는다.

### 5.14 설정 화면·단축키 바꾸기·설정 안 계정 추가

mac 3.2.0 의 설정 화면(`SettingsScreen`)을 옮긴 것이다. 이 절이 설정 화면의 열기·닫기·검색·전환, 단축키 모델, 새 세션 기본값, 로그인할 때 열기, 트레이 메뉴, 설정 안 계정 추가를 소유한다. 확인한 것은 §11.2.2 의 표만 확인으로 읽는다. 아직 없는 것(화면 나누기·글꼴 크기·테마·애니메이션·보드 글자 크기)은 §11.7 이 소유한다.
이름은 mac 과 맞춘다: `SettingsScreen`·`SettingsItem`·`SettingsSection`·`Shortcuts`·`KeyCombo`·`ShortcutSettings`·`LoginItem`·`NotifyPositionPicker`.

#### 5.14.1 열기·닫기·Esc·접힌 칸·전환 (`settings/screen.svelte.ts`·`screenRules.ts`·`motion.ts`·`SettingsView.svelte`)

- 설정 화면(`SettingsView`)은 터미널 영역(`TerminalAreaView`, 제목 띠 아래)을 덮는다. 보드 칸은 그대로다. 열려 있는 동안 탭의 셸과 claude 는 뒤에서 그대로 돈다(터미널 칸은 숨겨지지 않고 가려질 뿐이다). 열면 포커스가 검색 칸으로 간다.
- 여는 길: Ctrl+,(고정 키, 바꿀 수 없다. 터미널 칸이 접혀 있어도 받는다), 트레이 **설정…**(창을 먼저 보이고 이벤트 `board://settings` `{open: true, section?}` 를 보낸다), 계정 칩 메뉴의 **계정 추가…**(계정 묶음으로 열고 로그인을 시작한다, §5.14.6). 이미 떠 있으면 묶음만 바꾸고 검색 칸에 포커스를 준다.
- 닫는 길: Ctrl+, 한 번 더, 왼쪽 위 **터미널로 돌아가기**(Esc 표시), Esc. 닫힌 뒤 활성 터미널로 포커스를 돌려준다(새 세션 창·확인 창이 떠 있으면 그쪽이 포커스를 가진다). 키를 받는 중이면 닫으면서 받기를 그만둔다.
- 탭을 다루는 사용자 동작은 설정 화면을 닫고 한다(`leaveSettings`, `keep: true`): 탭 고르기, 경로 고르기·줄 안 이동(`showWorkspace`·`stepTab`·`selectNumber`·`stepWorkspace`), 새 탭 열기(`openShell`), 새 세션 창(`newSessionSheet`), 탭 닫기 요청(`requestClose`), 이름 바꾸기 시작(`beginRename`), 탭 단추 클릭, 세션 줄 클릭(`sessionFlow.svelte.ts`). 이때는 포커스를 터미널로 돌리지 않고 그 동작이 정한다.
- Esc 차례(`escapeStep`): 키 받는 중이면 받기 취소 → 검색어가 있으면 검색어 지우기(화면은 그대로 열려 있다) → 아니면 닫기. 스스로 Esc 를 처리하는 칸(`data-own-escape`, 지금은 고치던 글자가 있는 설정 > 터미널의 기본 명령 칸)에서 누른 Esc 는 설정 화면이 가져가지 않는다. 그 칸이 글자를 되돌리고 포커스를 풀며, 그다음 Esc 가 닫는다.
- 접힌 보드에서 열면 터미널 칸을 펼치고(`collapseOnOpen`: 상태 묶음을 받았고 접혀 있을 때만) 닫을 때 다시 접는다(`afterClose`: 우리가 펼쳤고 `keep` 이 아닐 때). 백엔드는 펼침을 `terminalExpanded` 로 저장하므로 설정 화면이 열려 있는 동안 파일에 `true` 가 적히고, 닫으면 `false` 로 돌아간다(mac 은 곧바로 false 로 되돌려 쓴다. 한계).
  열린 채 보드가 꺼질 때는 끌 때 확인(§5.12.10)의 길이 끄기 직전에 접고 저장이 끝나기를 기다린 뒤 답한다(`restoreSettingsBeforeQuit`): 목록이 비어 곧 꺼지는 길, 그리고 확인 창에서 **종료** 를 고른 뒤. 취소하면 접지 않는다(설정 화면이 그대로 열려 있다). 접지 못해도 끄기를 막지 않고 콘솔에만 남긴다. 탭을 다루는 동작으로 닫으면(`keep`) 펼친 채 둔다.
- 터미널 칸이 접히면(Ctrl+Alt+T·접기 단추) 설정 화면은 전환 없이 곧바로 사라지고 우리가 펼쳤다는 기억도 지운다(`dismissForCollapse`).
- 전환(`settingsTransition`: `slide`(기본)·`fade`·`none`, 모르는 값은 `slide`): `slide` 는 340ms, 보드 반대쪽 가장자리에서 들어오며 투명도가 함께 0에서 1로 간다(보드가 오른쪽이면 `translateX(-100%)` 에서 0, 보드가 왼쪽이면 `+100%` 에서 0). 나갈 때는 같은 쪽으로 간다. `fade` 는 200ms 투명도만. `none` 은 곧바로. 운영체제의 움직임 줄이기(`prefers-reduced-motion: reduce`)는 설정과 무관하게 `none` 이다. 설정값은 전환이 시작되는 순간 읽으므로 열어 둔 채 바꾼 값은 닫을 때부터 적용된다.
- 화면: 왼쪽 목록(돌아가기 · 검색 칸 · 묶음 여덟, `SECTION_ORDER` 순서)과 오른쪽 내용. 검색(`search`)은 모든 묶음의 항목 제목과 키워드에서 대소문자를 가리지 않고 부분 일치로 찾으며 앞뒤 공백은 무시하고 빈 검색어는 빈 결과다. 검색어가 있으면 오른쪽이 묶음별 결과 목록(`"<검색어>" 검색 결과 n개`)이 되고, 맞는 항목이 없는 묶음은 왼쪽 목록에서 흐려진다.
  결과를 누르면 그 묶음으로 가서 그 카드를 가운데로 스크롤하고 1200ms 동안 윤곽을 밝힌다(움직임 줄이기이면 스크롤·윤곽 없이). 카드는 카드 제목이 항목 제목과 같은 것으로 찾으므로, 제목이 계정 이메일인 **저장한 계정** 은 묶음 맨 위로만 간다. 묶음을 고르면 검색어를 지우고 키 받기를 그만둔다.

#### 5.14.2 묶음과 항목 (`settings/groups/*Items.ts`가 항목 표, 같은 폴더의 `*.svelte` 가 줄)

묶음 여덟(순서 그대로)과 항목(`SettingsItem` id, 저장 위치):

| 묶음 | 항목 | 저장 |
|---|---|---|
| 일반 | `pin` 항상 위에 고정 · `login` 로그인할 때 열기(§5.14.5) · `mascot` 제목 옆 캐릭터 | `pinned` · Run 값 `ClaudeBoard` · `mascot` |
| 창 | `side` 보드 위치 · `opacity` 투명도(30~100%) · `transition` 설정 화면 전환 | `boardSide` · `opacity` · `settingsTransition` |
| 알림 | `allMute` 모든 알림 끄기 · `position` 알림 위치 | `mute-all` 표지 · `notify-position` 파일 |
| 터미널 | `sessionCommand` 기본 명령 · `sessionMd` 답변 마크다운 꾸미기(§5.14.4) | `newSessionCommand` · `newSessionMdRender` |
| 보드 | `usageCompact` 사용 한도 간략히 보기 | `usageCompact` |
| 계정 | `accounts` 저장한 계정(저장 계정 줄들과 계정 추가·로그인 단계를 함께 가리키는 묶음 항목, §5.14.6) · `autoSwitch` 한도가 차면 다른 계정으로 자동 전환 | `autoSwitch` |
| 단축키 | 동작마다 `key.<동작 id>` 와 `shortcuts.resetAll`(§5.14.3) | `shortcuts` |
| 업데이트 | `version` 버전·바뀐 점·업데이트 확인/업데이트 · `autoUpdate` 자동으로 업데이트 확인(§5.14.5) | `autoUpdateCheck` |

- 알림 위치는 화면 그림에서 다섯 자리 가운데 눌린 점에서 가장 가까운 자리를 고른다(`pickPosition`, mac `NotifyPositionPicker` 의 132×82 그림 비율: 좌우 자리는 가운데에서 ±1/3 폭, 위·가운데·아래는 높이의 약 24%·54%·83%, 거리가 같으면 가운데가 이긴다).
- 이전에 트레이 메뉴에 있던 캐릭터·알림 위치·보드 위치는 이 화면에서 고른다(캐릭터 오른쪽 클릭, 윗줄 종 단추, 투명도 슬라이더는 그대로 있다).
- 설정 파일(§5.9)에 새 키가 다섯 있다(아래 §5.9).

#### 5.14.3 단축키 바꾸기 (`shortcuts/*`, `settings/groups/Shortcuts.svelte`, 백엔드 `hotkeys.rs`)

모델:
- 키 하나(`KeyCombo`)는 `KeyboardEvent.code`(자판 위치)와 Ctrl·Alt·Shift·Win 네 개다. 글자가 아니라 코드로 맞추므로 한글 입력기에서도 같은 키다.
- 저장은 설정의 `shortcuts` = `{동작 id: [코드, 수정키 비트]}`(비트 Ctrl=1 Alt=2 Shift=4 Win=8), 비운 동작은 `[]`, 기본 키와 같은 키로 바꾸면 항목을 지운다(`set`). 동작 id 는 저장의 열쇠라 바꾸면 사용자가 바꾼 키가 사라진다.
- 기본 키(`shortcuts/actions.ts`, 구현 그대로):

| id | 묶음 | 이름 | 기본 키 |
|---|---|---|---|
| `snap` | 보드 | 처음 크기로 붙이기 (어느 앱에서든) | Ctrl+Alt+B |
| `terminal` | 보드 | 터미널 펼치기·접기 (어느 앱에서든) | Ctrl+Alt+T |
| `pin` | 보드 | 항상 위에 고정 | 없음 |
| `newSession` | 터미널 | 새 세션… | Ctrl+T |
| `rename` | 터미널 | 탭 이름 바꾸기 | Ctrl+Shift+R |
| `tab1`~`tab9` | 탭으로 가기 | 탭 1~9 | Ctrl+Alt+1~9 |
| `prevTab` · `nextTab` | 탭으로 가기 | 이전 탭 · 다음 탭 | Ctrl+Alt+← · Ctrl+Alt+→ |
| `prevTab2` · `nextTab2` | 탭으로 가기 | 이전 탭 (보조) · 다음 탭 (보조) | Ctrl+Shift+Tab · Ctrl+Tab |
| `prevWorkspace` · `nextWorkspace` | 탭으로 가기 | 이전 경로 · 다음 경로 | Ctrl+Alt+↑ · Ctrl+Alt+↓ |

표 밖의 고정 키는 둘이다: 설정 열고 닫기 Ctrl+,(`SETTINGS_KEY`)와 설정 화면이 떠 있을 때의 Esc. 화면 나누기·글꼴 크기 동작은 3단계에서 표에 더한다.

받을 수 있는 키(`acceptable`):
- Win 이 든 키는 모두 거절(`reserved`). Ctrl 과 Alt 가 모두 없는 키는 거절(`needsModifier`, "Ctrl·Alt 가운데 하나를 함께 눌러 주세요", Shift 만 든 키도 같다). 예외로 F1~F12 는 수정키 없이도 받는다(mac 규칙).
- 거절 목록 `RESERVED`(**판단 목록**, 측정한 것이 아니라 Windows 가 먼저 가져가거나 편집·복사 표준으로 굳은 키를 적었다): Alt+F4 · Alt+Tab · Alt+Space · Alt+Esc · Ctrl+Esc · Ctrl+Shift+Esc · Ctrl+C·V·X·A·Z·Y · Ctrl+,. 거절하면 "<키>: Windows 기본 키라 쓸 수 없습니다" 를 단축키 묶음 위에 보이고 계속 받는다.

충돌: 같은 키를 쓰는 다른 동작이 있으면 양쪽 줄 모두에 `"<상대 이름>" 항목과 같은 키입니다` 와 **그쪽 비우기**(상대를 `[]` 로 비운다)가 뜬다. 충돌은 저장을 막지 않는다. 눌린 키가 겹친 두 동작에 걸리면 표 순서에서 앞선 동작이 실행된다(`find`).

받기 흐름(`recording.ts`): 줄의 키 칸을 누르면 "새 키를 누르세요…" 로 바뀌며 그 동작의 새 키를 받는다(한 번에 한 동작). 수정키만 눌린 keydown 은 다음 키를 기다린다. Esc 는 취소, Backspace·Delete 는 비움(`[]`), 그 밖의 받을 수 있는 키는 저장하고 끝낸다. 칸에서 포커스를 잃으면 받기를 그만둔다. 바꾼 줄에는 ↺(기본 키로 되돌리기), 윗줄에는 **모두 기본값으로**(바꾼 것이 없으면 비활성)가 있다.

앱 안 키 판단(`dispatch`, 순서 그대로): ① 입력기 조합 중이면 null(수정키가 든 키는 조합 글자가 아니라 단축키이므로 조합 중이어도 알아본다) ② AltGraph 가 눌려 있으면 null(Windows 가 Ctrl+Alt 로 보고하는 글자 키라 가로채지 않는다) ③ 키 받는 중이면 받는 쪽으로 ④ 새 세션 창·확인 창이 떠 있으면 null ⑤ 수정키만이면 null ⑥ Ctrl+, 는 설정 열고 닫기 ⑦ 설정 화면이 떠 있고 수정키 없는 Esc 이고 눌린 칸이 스스로 Esc 를 처리하지 않으면 설정 Esc ⑧ 표에서 찾기. 키 감시(`TerminalAreaView`)는 접혀 있어도 늘 걸려 있고, 동작마다 접힘에서 받는지 거른다(`gate.ts`): 접혀 있으면 Ctrl+, 와 설정 Esc 만 받고 나머지 앱 안 동작(탭·새 세션·이름 바꾸기 등)은 무시한다.

전역 단축키(`hotkeys.rs`): `snap`·`terminal` 둘만 어느 앱에서든 듣는다. 보드를 켤 때와 키 표가 바뀔 때마다(`set_shortcut`·`reset_shortcuts`) 걸린 것을 모두 풀고 지금 표대로 다시 건다. 설정 화면이 새 키를 받는 동안(`sync_hotkeys(paused = true)`)은 모두 풀어 둔다(누른 키가 동작하지 않고 화면으로 들어오게). 끝나면(`paused = false`) 다시 건다. 비운(`[]`) 동작이나 모양이 틀린 항목은 걸지 않는다. 걸지 못한 동작 id 는 `board.log` 에 남기고 이벤트 `board://hotkeys`(`["snap", …]`)와 `sync_hotkeys` 의 반환값으로 알려, 설정 > 단축키의 그 줄에 "전역 단축키로 등록하지 못했습니다 (다른 앱이 쓰는 키일 수 있습니다)" 가 뜬다.
키 받기가 `sync_hotkeys(false)` 와 `set_shortcut` 을 따로 보내 두 명령이 겹쳐 돌 수 있으므로, 키 표는 단축키 잠금 안에서 읽는다(`resync_locked`). 그래서 마지막에 걸리는 것은 언제나 그 시점의 최신 표다.

저장 모양과 깨진 값: `set_shortcut(id, combo)` 는 `[]` 또는 `[비지 않은 문자열, 0 이상 정수]` 만 받고 아니면 저장하지 않고 `board.log` 에 남긴다(`combo` 가 없으면 기본값으로 항목 삭제). 설정 파일을 읽을 때 `shortcuts` 가 객체가 아니면 그 키만 버리고, 배열이 아닌 항목이나 모양이 틀린 항목은 그 항목만 버린다. 항목 하나가 틀렸다고 설정 파일 전체를 기본값으로 읽지 않는다.

안내 글자가 바꾼 키를 따른다(`keyLabel`): 제목 막대 "처음 크기로 붙이기" 힌트(`snap`), 터미널 단추 캡슐(`terminal`), 탭 줄 **+** 의 키 글자(`newSession`, 비웠으면 글자 없음), 탭의 이름 바꾸기 안내(`rename`), 트레이 **터미널 펼치기 (<키>)**(`terminal_combo_display`, 코드 이름 `KeyT` 는 `T`, `Digit1` 은 `1`, 그 밖은 코드 이름 그대로).

#### 5.14.4 새 세션 기본값 (`terminal/newSessionDefaults.ts`, `settings/groups/Terminal.svelte`)

- 설정 키 `newSessionCommand`(기본 빈 값)와 `newSessionMdRender`(기본 true). 새 세션 창의 처음 명령과 답변 꾸미기 체크의 처음 값, 그리고 경로 카드 머리 **+**(창 없이 바로 새 탭)가 쓴다. 비었거나 설정이 없거나 검사에 걸리면 기본 명령 `claude --dangerously-skip-permissions` 다. 답변 꾸미기는 값이 `false` 일 때만 꺼진다. 옮겨 둔 md-render 폴더를 모르면 체크가 잠긴다(§5.12.11).
- 검사(`validateDefaultCommand`): 앞뒤 공백을 뗀 글에 대해, 빈 칸은 통과(기본 명령으로 돌아간다). 순서대로 ① 제어 문자(U+0000~001F, U+007F, 탭·줄바꿈 포함)가 있으면 "줄바꿈이나 제어 문자는 쓸 수 없습니다" ② 500자를 넘으면 "500자까지 쓸 수 있습니다" ③ `^\s*(\S*[\\/])?claude(\.exe)?(?=\s|$)`(대소문자 무시)에 안 맞으면 "claude 로 시작해야 합니다".
- 저장은 Enter 나 칸을 벗어날 때, 검사를 통과한 값만. 틀리면 칸 테두리가 붉어지고 사유가 보이며 저장하지 않는다(저장된 값은 그대로). 백엔드 `set_new_session_defaults(command, md_render)` 가 앞뒤 공백을 뗀다. 칸에서 고치던 글자가 있을 때 Esc 는 글자를 되돌리고 포커스를 풀며 설정 화면은 닫지 않는다(그다음 Esc 가 닫는다).
- 읽는 길도 같은 검사를 한다(`defaultNewSessionCommand`): 손으로 고친 설정 파일의 줄바꿈이나 `& claude …` 가 그대로 셸에 쳐지지 않게, 검사에 걸리면 기본 명령으로 쓴다.

#### 5.14.5 로그인할 때 열기·자동 업데이트 확인·트레이 (`login_item.rs`, `crates/hook/src/setup.rs`, `updater.rs`, `tray.rs`)

- 로그인할 때 열기: 레지스트리 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 의 값 `ClaudeBoard` = `"<ClaudeBoard.exe 경로>" --background`(`setup` 이 쓰는 것과 같은 모양, 값 이름 `RUN_VALUE` 도 같다). 켜 있는지는 값이 있는지 실제로 읽어 답한다(`login_launch`). 레지스트리를 쓰지 않는 실행(아래 격리 실행·Windows 가 아닌 개발 기기)이거나 읽지 못하면 표지 파일 `notifier\login-launch-off` 를 따른다.
  끄면 표지 파일을 먼저 쓰고(업데이트의 `setup` 이 다시 등록하지 않게) 값을 지운다(없으면 성공). 켜면 값을 먼저 쓰고 성공해야 표지를 지운다. 지금 도는 보드는 그대로 두고 다음 로그인부터 바뀐다. 화면은 바꾼 뒤 실제 값을 다시 읽어 스위치를 맞추고, 쓰기에 실패하면 이전 값으로 되돌리며 한 줄을 보인다("로그인할 때 열기를 바꾸지 못했습니다. 사유는 보드 기록(board.log)에 남습니다", 다시 읽기에 실패하면 "바뀐 값을 확인하지 못했습니다. 설정을 다시 열어 확인하세요"). 처리 중에는 다시 누를 수 없다.
- `setup` 5단계: 표지가 없으면 Run 값을 쓰고, 있으면 값을 지운다(끄기가 값을 못 지웠어도 업데이트 뒤에는 꺼진 채로 남는다). `uninstall` 은 Run 값과 표지 파일(`login-launch-off`)을 지운다(§3.7).
- 폴더를 바꿔 띄운 실행(`CLAUDE_BOARD_HOME` 또는 `CLAUDE_BOARD_NOTIFIER_DIR`, `paths::overridden()`)은 보드·`setup`·`uninstall` 모두 진짜 Run 값을 쓰지도 지우지도 않는다. 보드는 자기 notifier 폴더의 표지 파일만 바꾸고, `setup` 은 "자동 실행: 건너뜀" 한 줄만 남기고, `uninstall` 은 Run 값 삭제를 건너뛴다. 이 실행이 설치된 보드의 자동 실행을 건드리지 않게 하려는 것이다.
- 자동 업데이트 확인(`autoUpdateCheck`, 기본 true): 끄면 보드를 켤 때와 1시간마다의 확인을 하지 않는다. 손으로 누른 확인(설정 > 업데이트의 단추, 트레이 **업데이트 확인…**)은 그대로 된다. 켜는 순간 곧바로 한 번 확인한다(§5.8).
- 트레이 오른쪽 클릭 메뉴(지금): 세션 보드 보기 · 터미널 펼치기 (<지금 키>)(체크) · 항상 위에 고정(체크) · 설정… · 업데이트 확인… · 종료. 캐릭터·알림 위치·보드 위치 항목은 없다(설정 > 일반·알림·창). **설정…** 은 창을 먼저 보이고 설정 화면을 연다.

#### 5.14.6 설정 안 계정 추가 (`account_login.rs`·`account_login/`·`login_output.rs`·`accounts.rs`, 화면 `settings/groups/Account.svelte`·`accountLogin.ts`·`board/AccountChip.svelte`)

- 시작: 설정 > 계정의 **계정 추가…**, 계정 칩 메뉴의 **계정 추가… (설정 > 계정에서 로그인)**(설정을 계정 묶음으로 열고 바로 시작), 만료된 저장 계정의 **다시 로그인**(`--email <이메일>` 을 더한다). 진행 중이면 칩 메뉴에 **계정 추가 계속 (설정 > 계정)** 과 **계정 추가 취소** 가 뜬다. 명령 `begin_add_account(email?)`.
- 보드가 가짜 콘솔(ConPTY, 1000열 × 50줄)에서 `<claude> auth login --claudeai [--email <이메일>]` 을 돌린다. 홈 폴더에서 시작하고 환경변수 `CLAUDE_CONFIG_DIR=<notifier\accounts\login-<8자>>`(슬롯)와 `BROWSER=<슬롯>\open-browser.cmd` 를 더한다. 새 콘솔 창은 띄우지 않고 셸을 거치지 않는다.
  `open-browser.cmd` 는 받은 첫 인자(주소)를 같은 폴더의 `url.txt` 에 덧붙이기만 하고 브라우저를 열지 않는다(`set "address=%~1"` 와 지연 확장으로 주소의 `&`·`>`·`%` 가 명령으로 읽히지 않게, ASCII 만). 이유: 진짜 claude 가 지금 로그인된 계정이 이미 들어 있는 기본 브라우저를 열어 거기서 로그인을 마쳐 버리면 "다른 계정 추가"가 안 된다(mac 도 같은 방식).
- 주소 찾기(`login_output.rs`): 출력에서 제어열을 걷어 낸 글(`strip_ansi`: 색·창 제목 제어열은 지우고, 커서를 칸 사이로 옮기는 제어열 `C`·`G`·`H`·`f` 는 공백 하나로 바꾼다. 화면 글이 커서 이동으로 띄어 쓰이므로 지워 버리면 주소 뒤 글이 주소에 붙는다. 끝이 잘린 제어열은 버린다)에서 `https://` 로 시작하고 `oauth/authorize` 를 품은 공백 없는 덩어리를, 뒤에 공백이 올 때만(끝까지 읽혔을 때만) 주소로 친다. 읽기 경계에서 글자·제어열·주소가 잘려도 모았다가 읽는다(20,000바이트를 넘으면 뒤쪽 절반만 남긴다).
  출력에서 못 찾으면 보드가 0.3초마다 슬롯의 `url.txt` 를 읽어 같은 규칙으로 찾는다(두 번째 길). 주소는 처음 온전해질 때 한 번만 알린다(이벤트 `board://account-login` `{url?, note?, done?, error?}`).
- 코드 칸: 사용자가 붙여 넣은 코드(`submit_login_code`)는 제어 문자를 버리고 앞뒤 공백을 뗀 글을 `<코드>\r` 로 ConPTY 입력에 쓴다. 빈 코드, 끝난 명령, 끝난 로그인이면 쓰지 않는다. 코드는 로그에 남기지 않는다. 보낸 뒤 "코드를 보냈습니다. 확인하는 중…" 안내가 뜬다.
- 끝남: 명령이 종료 코드 0 으로 끝나면 "로그인을 마쳤습니다. 계정 정보를 확인하는 중…" 안내를 띄우고, 저장은 보드가 슬롯을 3초마다 지켜보는 `accounts::tick` 이 한다(§5.5: `.claude.json` 의 `oauthAccount` 와 `.credentials.json` 의 `claudeAiOauth` 가 생기면 저장하고 지금 계정도 저장한 뒤 슬롯 삭제). 0 이 아닌 코드로 끝나면(보드가 아직 이 로그인을 기다리는 중일 때만) 마지막 비어 있지 않은 줄에서 `prompted >` 뒤의 글(200자까지)로 "계정을 추가하지 못했습니다: <사유>" 를 띄우고 계정 추가를 접는다(글이 없으면 "계정을 추가하지 못했습니다"). 토큰만 있고 계정 정보가 30초 넘게 없으면 "로그인은 됐지만 계정 정보를 받지 못했습니다. 계정 추가를 다시 해 주세요", 15분이 넘으면 "로그인을 15분 동안 기다리다 그만뒀습니다" 로 접는다.
- 화면(설정 > 계정): 로그인 패널의 단계 둘과 취소 단추. 1) 열린 로그인 창에서 로그인(주소가 오면 **다른 브라우저에서 열기**(`open_login_url`: 로그인 명령 출력에서 온 https 주소만 연다)와 **링크 복사**), 2) 브라우저가 코드를 보여 주면 붙여넣는 칸과 **확인**. 저장한 계정 줄에는 **전환**(만료면 **다시 로그인**)과 **지우기** 가 있고 지금 계정은 **사용 중** 이다.
- 로그인 창(`login_window.rs`): 주소가 처음 오면 보드가 소유한 창 하나("Claude 계정 추가", 520×720, 가운데, 보드가 맨 위에 고정이면 같이)를 연다. 쿠키가 창마다 새것인 incognito 라 브라우저에 로그인된 계정이 이어지지 않는다. 열어도 되는 주소는 https 이고 호스트가 있는 것뿐이다(`login_url`). 이 창은 `capabilities` 에 없는 라벨이라 페이지가 보드 명령(`invoke`)을 부를 수 없다.
  로그인이 끝나기 전에 사용자가 이 창을 닫으면 계정 추가를 취소하고 "로그인 창을 닫아 계정 추가를 취소했습니다" 를 띄운다. 로그인이 끝나거나 취소되면 보드가 창을 닫는다(사용자가 닫은 것으로 치지 않는다). Google 은 앱 안 웹 창의 Google 로그인을 막을 수 있어(이메일 로그인은 된다) 그때는 **다른 브라우저에서 열기** 와 코드 칸을 쓴다. 시험 모드(가짜 claude)에서는 창을 열지 않는다.
- 접기(`cancel_add`): 로그인 프로세스를 먼저 죽이고 끝난 것을 확인한 뒤(최대 3초) 슬롯을 지운다. 죽기 전의 claude 가 지운 슬롯에 평문 토큰 파일을 다시 만들지 못하게 하려는 순서다. 3초 안에 안 끝나면 슬롯은 지우되 `board.log` 에 남긴다. 저장을 마친 뒤에도 같은 순서로 한 번 더 한다. 보드가 시작할 때 남은 `login-*` 슬롯은 치운다.
- 지우기: **지우기**(칩 메뉴의 **저장한 계정 지우기 ▸** 도 같다)는 `delete_account(id)` 가 네이티브 대화상자(`board.confirm`, 제목 "<이메일> 계정을 지울까요?", 본문 "보드에 저장한 이 계정의 로그인 정보를 지웁니다. 다시 쓰려면 계정을 다시 추가해야 합니다.", 버튼 **지우기**·**취소**)에서 확인했을 때만 지운다. 지금 로그인 계정과 저장 목록에 없는 id 는 지우지 않는다. 확인을 건너뛰던 옛 `add_account`·`remove_account` 명령은 없다.
- 시험 게이트(§5.13): 어느 프로그램을 띄울지는 `login_program` 이 정한다. 시험 claude(`CLAUDE_BOARD_CLAUDE`)가 켜짐이면 가짜, 어긋남이면 거절, 꺼짐이면 진짜 claude 를 찾는다. 단 홈이나 notifier 를 환경변수로 바꿔 띄운 실행에서 시험 claude 가 꺼져 있으면 진짜 claude 를 찾지도 띄우지도 않고 "…로 폴더를 바꾼 실행에서는 시험 claude(CLAUDE_BOARD_CLAUDE) 없이 계정 추가를 하지 않습니다" 로 거절한다. 가짜에는 보드 환경의 `FAKE_LOGIN_` 로 시작하는 변수만 넘긴다.
- `unchecked:` 진짜 Windows `claude auth login` 의 ConPTY 출력 모양과 Windows Claude Code 가 `BROWSER` 를 따르는지(따르지 않으면 기본 브라우저가 열리고 주소는 출력에서만 얻는다), 로그인 창 자체(GUI, Google 로그인 포함), 로그인이 도는 중에 보드를 끄면 로그인 프로세스가 끝나는지는 확인하지 못했다(§11.2.2).

## 6. 백엔드 ↔ 프론트엔드 계약

### 6.1 상태 묶음 — 이벤트 `board://snapshot` 와 명령 `get_snapshot`

백엔드가 1초 reload 뒤 내용이 바뀌면(또는 `now` 만 바뀌어도 1초마다) 보낸다. 시각은 모두 **유닉스 밀리초**.

```ts
type State = "working" | "waiting" | "idle" | "paused" | "ended";
interface Snapshot {
  now: number;
  groups: Group[];               // 정렬·숨김·걸러내기 전(필터는 프론트) — §5.1 순서
  allMuted: boolean;
  counts: { waiting: number; working: number; idle: number; paused: number; ended: number };
  usage: { limits: Limit[]; failed: boolean; tokenExpired: boolean };
  accounts: { current: Account | null; saved: Account[]; adding: boolean; switching: boolean;
              autoSwitch: boolean; notice: { text: string; error: boolean } | null };
  updater: { current: string; dev: boolean; latest: string | null; running: boolean; failed: boolean;
             checking: boolean; notice: { kind: "ok" | "warn" | "dev"; text: string } | null };
  settings: BoardSettings;       // §5.9 의 프론트가 쓰는 키 + notifyPosition(notify-position 파일 내용). 정확한 모양은 app/src/lib/types.ts
  claudeVersion: string | null;
  terminals: { id: string; name: string; installed: boolean; running: boolean }[];   // §5.11 터미널 독(외부 터미널 앱) 목록. 내장 터미널(§5.12)의 `TermInfo` 와 다르며 스냅샷에 내장 터미널은 없다
  runsClaude: string[];          // 지금 claude 가 도는 보드 터미널 TermId (§5.12.7)
  atPrompt: string[];            // 셸이 프롬프트에 있는 보드 터미널 TermId (§5.12.7)
}
interface Group { id: string /* project_dir, 없으면 cwd */; name: string; path: string /* 홈을 ~ 로 */; sessions: Session[] }
interface Session {
  id: string; cwd: string; state: State; since: number; updated: number; headless: boolean;
  pid: number; pidCtime: number | null;   // claude 프로세스와 그 생성 시각(유닉스 초). 끝나기 기다리기·생사 확인에 쓴다(§5.12.9, §5.12.12)
  title: string /* topic 또는 message 또는 "요청 전" */; topic: string; message: string;
  name: string | null /* Claude Code 세션 이름(§2.3). 없거나 ended 면 null */;
  ctx: { used: number; size: number; pct: number } | null;
  brain: { model: string; effort: string; family: string; version: string; effortLevel: number } | null;
  subagents: Sub[]; shells: number; paused: boolean; muted: boolean; limitAt: number | null;
  canDrive: boolean;             // §5.7 drive_target 이 키를 넣을 곳을 찾았는가
  tty: string | null;            // 이 pid 가 도는 보드 터미널 TermId (§5.12.7)
  tabAncestor: string | null;    // headless 인데 조상이 보드 터미널 셸인 경우의 TermId
  mode: "-p" | "--bg" | null;    // claude 를 띄운 방식. 대화형이면 null
  appName: string | null;        // 보드 밖 터미널 세션의 앱 이름(Windows Terminal 등)
}
interface Sub { id: string; type: string; desc: string; state: "running" | "waiting" | "done"; started: number;
                ended: number | null; tool: string; background: boolean; brain: Session["brain"] }
interface Limit { id: string /* 5h | 7d | 7d-<이름> */; label: string; short: string; tiny: string; pct: number; resets: number | null }
interface Account { id: string; email: string; org: string; plan: string; expired: boolean }
```

### 6.2 명령 (프론트 → 백엔드, Tauri `invoke`)

`get_snapshot` · `toggle_collapsed(cwd)` · `set_all_collapsed(collapse)` · `toggle_ended_open(cwd)` · `toggle_pin_group(cwd)` · `hide_group(cwd)` · `remove_ended(cwd)` · `remove_session(id)` ·
`toggle_kind_collapsed(key)` · `toggle_sub_collapsed(id)` · `set_usage_compact(bool)` · `set_mute(id, muted)` · `set_all_muted(bool)` · `reveal(cwd)` ·
`session_action(action: "activate"|"focus"|"reopen"|"pause"|"resume"|"stop", id, carryOn?)` · `drive_session(id, steps)`(`{key:"escape"|"enter"}`·`{text}`·`{wait:ms}`, 성공 여부) · `rename_check(id)`(`{safe, transcript}`) · `show_board` · `take_open_session`(쌓인 세션 열기 요청을 통째로 돌려주고 이후는 이벤트로 받겠다고 알림) · `reopen_done(id)`(이어 열기를 마쳤으니 일시중지 표지를 지움) · `double_click_time`(ms) · `switch_account(id)` · `begin_add_account(email?)` · `submit_login_code(code)` · `open_login_url` · `cancel_add_account` · `delete_account(id)`(네이티브 확인 뒤에만 지움) · `set_auto_switch(bool)` ·
`check_update(manual)` · `run_update` · `open_changelog` · `set_pinned(bool)` · `set_mascot(kind)` · `set_opacity(v)` · `set_notify_position(pos)` · `open_terminal(id)` ·
`enter_mini(rows)` · `leave_mini` · `snap_right`(이름은 그대로 두고 `boardSide` 쪽에 붙인다) · `hide_board` · `minimize_board` · `log(text)`.
내장 터미널 창·설정 명령(`board.rs`): `set_terminal_expanded(on)` · `set_board_side(side: "left"|"right")` · `set_board_width(width)`(논리 px, 경계를 놓을 때 저장). 이벤트 `board://terminal-focus`(프론트가 활성 터미널에 `focus()`).
설정 화면 명령(§5.14): `set_settings_transition(value)` · `set_new_session_defaults(command, md_render)` · `set_auto_update_check(on)` · `set_shortcut(id, combo?)`(`combo` 가 없으면 기본값으로, `[]` 는 비움) · `reset_shortcuts` · `sync_hotkeys(paused)`(걸지 못한 동작 id 를 돌려준다) · `login_launch`(Run 값을 읽어 답한다) · `set_login_launch(on)`. 이벤트 `board://settings` `{open, section?}`(트레이 **설정…**) · `board://hotkeys`(`[동작 id, …]`) · `board://account-login` `{url?, note?, done?, error?}`.
수명 주기·답변 꾸미기·시험 명령: `save_tabs(entries)`·`load_tabs`(§5.12.9) · `existing_dirs(paths)`(있는 폴더만 돌려줌) · `pids_alive(targets: {pid, ctime}[])`(산 pid 목록) · `wait_exit(pid, pidCtime, timeoutMs)`(끝났으면 true) · `lifecycle_started`(화면이 끌 때 확인 이벤트 듣기를 켬) · `answer_shutdown(ok, pause)` · `shutdown_dialog_shown`(§5.12.10) · `md_render_dir`(옮겨 둔 폴더, 못 옮겼으면 null) · `remember_md_render_off(id)` · `prune_md_render_off` · `test_claude_prefix`(§5.13).
이벤트 `board://confirm-shutdown` `{reason: "quit"|"update"|"cancel", items}`(`cancel` 이면 떠 있는 확인 창을 닫는다).
이벤트 `board://open-session` `{id, action: "reopen"|"focus", carryOn, projectDir, resumeArgs}`(Rust → 허브, §5.7). 알림 패널 클릭이 거는 `--action focus <id>` 도 이 이벤트로 온다(§5.12.12).
내장 터미널 명령(`terms.rs`, 모양은 §5.12.1): `term_spawn` · `term_list` · `term_attach` · `term_ack` · `term_write` · `term_write_bin` · `term_resize` · `term_kill`.
그룹을 가리키는 명령 인자 `cwd` 는 그룹 id(§5.1 의 묶음 열쇠)다.
확인 대화상자(계정 전환·세션 중단)는 백엔드가 네이티브 대화상자(`tauri-plugin-dialog`)로 띄운다.

### 6.3 알림 패널 — 프론트 경로 `/notify`, 백엔드가 인자를 `get_notify_options` 로 준다. 내용 높이 `notify_ready(height)`(백엔드가 그 높이로 자리를 잡고 보인다), 마우스 `notify_hover(inside)`, 클릭 `notify_clicked`(백엔드가 보드를 띄움. 패널에 `--session` 이 있으면 `--action focus <세션 id>`, 없으면 `--show`), 닫힘 애니메이션이 끝나면 `notify_finish`(백엔드가 슬롯을 놓고 프로세스를 끝냄). 카운트다운은 프론트가 한다.

### 6.4 캐릭터 컴포넌트 계약 — `app/src/lib/mascot/Foreman.svelte`

props `{ groups: Group[]; limits: Limit[]; now: number; compact?: boolean; mascot: MascotKind; onPickMascot: (kind) => void }`. 말풍선 문구·시계·그림·오른쪽 클릭 캐릭터 메뉴를 모두 안에서 처리한다. 이름·유행어 목록은 `app/src/lib/mascot/mascots.ts` 의 `MASCOTS`.

## 7. 보드 화면 (프론트엔드) — `ClaudeBoard.swift` 의 「화면」·「창 크기 단계」 구역(`BoardView`·`GroupCard`·`SessionRow`·`MiniBoard`)과 스크린샷 `docs/images/*.png`

- 어두운 테마 고정. 바탕(`Backdrop`, `ClaudeBoard.swift Backdrop`), 상태 색(`ClaudeBoard.swift Palette`): working `rgb(115,168,255)`, waiting `rgb(255,168,92)`, idle `rgb(179,191,219)`, paused `rgb(242,204,89)`, ended 회색, accent `rgb(217,120,87)`.
- 상태 아이콘 애니메이션: 작업 중 도는 그라데이션 호(0.9초), 승인 대기 퍼지는 파문(1.1초), 대기 통통 튀며 나타나는 체크, 일시중지 ⏸, 종료 빨간 ⊖. 작업 중 줄에 2.4초 주기로 지나가는 빛(Shimmer). 승인 대기 카드 테두리가 2.4초 주기로 숨 쉼. 창이 가려지거나 숨으면 애니메이션 멈춤.
- 단계(tier, `ClaudeBoard.swift BoardTier`): 폭은 창 폭이 아니라 보드 칸의 폭(ResizeObserver)이다(§7.2). 폭 360 미만 = mini. 아니면 목록 칸이 120 이상 남으면 full, 56 이상이면 medium, 그 밖 mini. medium 은 사용 한도 한 줄, 카드는 접힌 한 줄(눌러서 그 카드만 펼침, 저장된 접기 상태는 그대로). mini 는 캐릭터 줄 + 경로마다 한 줄 + 아래 줄(한도·투명도), 제목 막대에 상태별 개수와 ⤢.
- 최대축소(↘↖): 보드 쪽 위 모서리(오른쪽이 기본) 기준 폭 300, 높이 = 144 + (min(max(그룹 수,2),6) − 2) × 26. ⤢ 는 기억한 위치(작업 영역과 겹치고 폭 ≥ 360)로, 없으면 처음 크기로 보드 쪽에 붙이기. 터미널이 펼쳐진 채 미니로 들어갔으면 ⤢ 가 다시 펼친다(§5.10).
- 윗줄: 앱 아이콘 + "Claude Board" + 버전 캡슐, 아래 "활성 세션 n개 · Claude Code <v>", 오른쪽 캐릭터(§7.1). 둘째 줄: 상태 칩(승인 대기·작업 중·대기, 누르면 그 상태만 보기/다시 누르면 모두), 전체 알림 단추(꺼지면 붉은 "알림 꺼짐"), 업데이트 확인 ⟳, 모두 접기/펼치기(full 에서만), 위에 고정.
- 사용 한도 카드(`UsagePanel`): 계정 칩(메뉴: 계정 목록·체크·"다시 로그인 필요"·계정 추가… (설정 > 계정에서 로그인)/계정 추가 계속·계정 추가 취소·저장한 계정 지우기 ▸·한도 자동 전환 체크), 간략/상세 단추, 한도 막대(색: <50 초록, <80 주황, 그 밖 빨강), 남은 시간 문구(`resetText`), 안내 문구.
- 경로 카드(`GroupCard`): 접기 화살표, 폴더(접혀 있으면 대표 세션 상태 아이콘), 이름·경로(접히면 "<상태> · <하는 일>" +n), 별(상단 고정)·X(목록에서 지우기), 상태별 개수, 오른쪽 클릭 메뉴. 안: 터미널/백그라운드 구역 머리글(접기), 세션 줄, "종료된 세션 n개"(접기, 마우스 올리면 휴지통 "모두 지우기").
- 세션 줄(`SessionRow`): 서브에이전트 접기 화살표, 상태 아이콘, 제목, 알림 스위치(전체 알림 꺼짐이면 잠김), … 메뉴(ended: 세션 다시 열기/재개, paused: 재개·중단, 그 밖: 일시중지(`working`·`waiting` 이고 canDrive 일 때만 켜짐)·중단. 살아 있는 `background` 세션에는 … 단추가 없다), ended 면 ⓧ 지우기. 둘째 줄: 경과 시간(ended "HH:mm 종료", idle "<경과> 전"), 하위 작업 n(running/count), 셸 n개, 세션 이름 라벨(태그 아이콘 + 이름, 아래), 모델 라벨(계열 색 Opus `rgb(255,153,115)` Sonnet `rgb(122,179,255)` Haiku `rgb(140,224,204)` Fable `rgb(224,179,255)` + effort 막대 5칸), 컨텍스트 라벨(<30 초록, <70 노랑, 그 밖 빨강). 폭이 좁으면 3단계로 줄임(0·1단계에만 이름 라벨을 보이고 3번째 단계부터 뺀다).
  세션 이름 라벨: `name` 이 있을 때만. 24 코드포인트를 넘으면 앞 11자 + `…` + 뒤 11자로 줄이고(코드포인트 기준), 마우스를 올리면 힌트 "세션 이름 <name>" 에 전체 이름이 나온다. 승인 대기면 ✋ 메시지(2줄). 위치 칩(§5.12.7)이 이름 라벨 앞에 붙는다. 한 번·더블 클릭은 위치별 표(§5.7)를 따른다.
- 서브에이전트 줄(`SubagentRow`): 트리 연결선, 작은 상태 아이콘, 설명(없으면 종류), 라벨 "서브에이전트"·종류·경과("<시간> 걸려 완료"/"승인 대기")·모델·🔧도구.
- 아래로 가려진 카드가 있으면 "아래에 경로 n개 더[ · 승인 대기 있음]"(누르면 그 카드로 스크롤). 세션이 없으면 "아직 기록된 세션이 없습니다 / Claude Code 세션에서 요청을 보내면 여기에 나타납니다".
- 힌트(설명 말풍선): 0.35초 뒤, 방금 떠 있었으면 바로. 누르거나 스크롤하면 사라짐. 모든 단추의 설명 문구는 macOS `.hint(…)` 문구를 그대로 쓴다.
- 업데이트 알림 줄(`UpdateBanner`), 오른쪽 아래 투명도 슬라이더, 오른쪽 아래 끝 터미널 독(§5.11).

### 7.1 캐릭터 (`ClaudeBoard.swift Foreman`·`BrainrotFigure`)

말풍선 문구(`foremanQuips`) 그대로. 3.2초마다 다음 말, 3.7초마다 눈 깜빡임, 말이 바뀌면 0.45초 폴짝. 기분 urgent/busy/idle/sleepy 별 색·동작.
8종(트랄랄레로 트랄랄라·봄바르디로 크로코딜로·퉁 퉁 퉁 사후르·리릴리 라릴라·브르르 브르르 파타핌·보네카 암발라부·카푸치노 아사시노·현장 감독)과 유행어(`Mascot.catchphrase`), 그림·동작은 `BrainrotFigure` 와 감독 `Foreman.figure`를 SVG 로 옮긴다(46×44, mini 0.75배). 누르면 다음 말, 오른쪽 클릭 = 캐릭터 고르기.

### 7.2 내장 터미널이 들어간 레이아웃

- 페이지 루트는 `[터미널 │ 경계 │ 보드]` 세 칸의 flex 이고, 보드가 왼쪽(`boardSide` = `"left"`)이면 순서를 뒤집는다. 보드는 창 전체를 덮는 고정 위치 요소가 아니라 이 루트 안의 한 칸이다. 창의 둥근 모서리도 보드 칸이 아니라 루트에 걸린다.
- 단계(§7 첫 항목)는 창 폭이 아니라 보드 칸의 폭으로 고른다. 칸 폭이 바뀌면 단계가 다시 정해진다.
- 문서 전체 불투명도는 §5.10 의 규칙(`boardOpaque`)을 따른다. 작업 영역 전체를 덮은 펼친 창은 불투명하고, 줄여서 덮지 않는 펼친 창은 마우스가 밖이면 흐려진다.
- 제목 막대에 터미널 펼치기/접기 단추가 있다. 터미널 독(`EdgeDock`)과 마우스 가장자리 판정(`pointer.atEdge`)은 창이 아니라 보드 칸의 가장자리를 기준으로 하고, 보드가 왼쪽이면 좌우를 뒤집는다.
- 터미널 칸의 경계(PaneDivider)는 끄는 동안 fit 을 멈추고(§5.12.5), 놓으면 `set_board_width` 로 보드 칸 폭을 저장한다.
- 브라우저 개발용 목(`mock.ts`)에는 입력을 그대로 되돌려 주는 에코 터미널이 있다.
- 내장 터미널의 새 화면 문자열에는 긴 줄표(—)를 쓰지 않는다.

터미널 영역(`TerminalAreaView.svelte`)은 위에서 아래로 제목 띠 · 경로 줄(탭이 있을 때만) · 탭 줄 · 화면 칸이다. 값은 CSS 픽셀이다.

| 부분 | 값 |
|---|---|
| 제목 띠 | 높이 32, 왼쪽 여백 8, 바탕 `--term-bar`(#1a1a21). 터미널 단추(캡슐)가 앞쪽 가장자리에 있고 미니 단계에서는 없다. 나머지는 창 끌기 자리. 보드가 왼쪽이면 최소화·닫기(`WinButtons`)가 띠 오른쪽 끝에 있다 |
| 터미널 단추 | 눌리는 칸 높이 28, 캡슐 높이 20·좌우 여백 8·둥근 끝. 아이콘 · "터미널 펼침"/"터미널 접힘" · 키 표시 "Ctrl+Alt+T". 펼침은 작업 중 색, 접힘은 흰 반투명. 보드 쪽 제목 막대에도 접힘일 때 같은 단추가 있다 |
| 경로 줄 | 높이 32, 좌우 여백 8, 항목 높이 24·사이 4. 항목은 상태 점 7 · 폴더 이름(12, 굵게) · 탭 수(11). 같은 폴더 이름이 둘 이상이면 `부모/이름`, 홈은 `~`. 지금 경로는 고른 탭 바탕에 작업 중 색 테두리. 힌트는 전체 경로(홈 아래는 `~` 로 줄임). 넘치면 가로로 밀고(휠도 됨) 지금 경로가 늘 보이게 한다. 경로를 닫는 단추는 없다 |
| 탭 줄 | 높이 40, 좌우 여백 8, 아래 선 1. 지금 경로의 탭만 보인다. 탭 폭 120~250, 사이 2, 다 들어가면 가장 넓게. 120 씩도 안 들어가면 가로로 밀고 고른 탭이 보이게 따라간다 |
| 탭 | 상태 점 8 · 이름(13, 굵게) · 하는 일(12, 줄임표) · 닫기 ×(16). 고른 탭은 화면 바탕색 + 위쪽 2px 작업 중 색 줄. 하는 일은 짝 세션이 있으면 세션 제목(승인 대기면 메시지), 없으면 셸이 보낸 제목 |
| 새 세션 단추 | 탭 줄의 밀리는 칸 밖(늘 보임). 점선 테두리(1px, 3 그리고 2 간격) 상자 높이 26 · 아이콘 · "Ctrl+T"(새 세션의 지금 키, 비웠으면 글자 없음). 힌트 "새 세션" |
| 빈 상태 | 탭이 없을 때 화면 칸 가운데: "보드에서 띄운 세션이 없습니다"(14) · "보드에서 시작하거나 이어 연 세션이 여기 탭으로 열립니다"(12) · "새 세션" 캡슐 단추(높이 30) |
| 새 세션 창 | 창 맨 위 가운데에서 내려오는 모달, 폭 460, 아래 모서리만 둥글다. 작업 폴더 목록(보드의 경로들, 줄 높이 46, 최대 230, 라디오) · "다른 폴더 고르기…"(폴더 선택 대화상자) · 실행할 명령 입력(기본 `claude --dangerously-skip-permissions`) · "답변 마크다운 꾸미기"(기본 켬. 옮겨 둔 md-render 폴더를 모르면 잠김, §5.12.11) · [취소] [새 탭에서 시작]. Esc 취소, Enter 시작. 처음 고른 폴더는 지금 경로와 같은 보드 경로, 없으면 첫 경로, 보드에 경로가 없으면 홈 |
| 닫기 확인 창 | 짝 세션이 끝나지 않은 탭을 ×로 닫을 때만. 폭 320. "탭을 닫을까요?" [닫기] [취소 (Esc)] |
| 알림 줄 | 창 아래쪽 가운데에 한 줄(`TerminalNotice.svelte`, 폭 최대 480). 6초 뒤 사라지고 누르면 바로 닫히며 다시 띄우면 6초가 새로 시작한다. 터미널 없는 세션을 눌렀을 때의 안내와 "세션을 이어 열 수 없습니다" 가 여기에 뜬다 |

카드의 +(`GroupCard` 머리의 단추)는 기본 명령으로 그 카드 경로에서 바로 새 탭을 연다(새 세션 창을 거치지 않는다).

키(기본 키이며 설정 > 단축키에서 바꾼다, §5.14.3. 터미널이 펼쳐진 동안만 받는다(설정 열고 닫기 Ctrl+, 만 접혀 있어도 받는다). xterm 이 받기 전에 캡처 단계에서 가로챈다. 새 세션 창·확인 창이 떠 있으면 무시):

| 키 | 동작 |
|---|---|
| Ctrl+T | 새 세션 창(다른 길로 불렸을 때 접혀 있으면 펼친다). 이미 떠 있으면 무시. 셸 안의 Ctrl+T 는 셸에 닿지 않는다 |
| Ctrl+Shift+R | 고른 탭 이름 바꾸기 |
| Ctrl+Alt+1~9 | 지금 경로의 탭 순서 n 번째 탭 |
| Ctrl+Alt+←/→ | 지금 경로 안의 이전/다음 탭(끝에서 반대쪽 끝으로) |
| Ctrl+Alt+↑/↓ | 이전/다음 경로(끝에서 반대쪽 끝으로) |
| Ctrl+Alt+T | 펼치기/접기(전역, §5.10) |
| Ctrl+, | 설정 화면 열고 닫기(바꿀 수 없다, §5.14.1) |

AltGr(`getModifierState("AltGraph")` 가 참인 입력)은 Windows 가 Ctrl+Alt 로 보고하지만 글자를 치는 키라 위 Ctrl+Alt 키를 가로채지 않고 번호 배지도 띄우지 않는다.

Ctrl+Alt 를 누르는 동안(창이 포커스를 잃으면 해제) 탭의 × 자리에 번호 배지(1~9)가, 고른 탭의 하는 일 자리에 "Ctrl+Shift+R 이름 바꾸기" 안내가 보인다.

## 8. 배포

### 8.1 릴리스 자산 (태그 `windows-v<버전>`, Latest 아님)

| 자산 | 내용 |
|---|---|
| `ClaudeBoard-windows-x64.zip` | 최상위 `ClaudeBoard.exe`, `claude-board-hook.exe`, `LICENSE`, `README.md`, `THIRD_PARTY_NOTICES.txt` + `plugins\md-render\` 아래 다섯 파일(아래 「md-render 배치」) |
| `ClaudeBoard-windows-x64.zip.sha256` | `<sha256 16진수>  ClaudeBoard-windows-x64.zip` |
| `ClaudeBoard-<버전>-setup.exe` | Tauri NSIS 설치 프로그램(사용자 단위 설치, 설치 후 `claude-board-hook.exe setup` 실행). 설치 폴더에 zip 과 같은 배치로 풀린다 |
| `install.ps1`, `uninstall.ps1` | 설치·업데이트·제거 스크립트 |
| `VERSION` | 버전 한 줄 |

OS 빌드 검사: `ClaudeBoard.exe` 가 `CreatePseudoConsole` 을 정적으로 가져오므로 Windows 10 1809(빌드 17763) 미만에서는 실행 때 진입점 오류로 뜨지 않는다. 두 설치 경로 모두 시작 전에 빌드를 보고 17763 미만이면 멈춘다. 빌드는 레지스트리 `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\CurrentBuildNumber` 를 먼저 읽는다(호환 매니페스트가 없는 프로세스는 `OSVersion`·`GetVersionEx` 가 거짓 버전을 받는다). 못 읽을 때만 `OSVersion`(install.ps1)·`WinVer.nsh`(setup.exe)로 대신하고, 그것도 못 얻으면 검사를 건너뛴다. install.ps1 은 `throw`(exit 없음), setup.exe 는 `packaging/nsis-hooks.nsh` 의 숨은 첫 Section `-ClaudeBoardOsBuild` 가 MessageBox(`/SD IDOK` 라 `/S` 에서 멈추지 않음) 뒤 `Abort`(`/S` 종료 코드 2)한다. Tauri 템플릿의 `NSIS_HOOK_PREINSTALL` 은 `Section WebView2`(부트스트래퍼 내려받기) 뒤에 불리므로 쓰지 않는다. 시험: `windows/ci/os-build-check.ps1`(5.1·pwsh 단위 시험).

**md-render 배치.** macOS 앱이 `Contents/Resources/plugins/md-render/` 에 싣는 것과 같은 다섯 파일만 이름으로 골라(`tests\`·Claude Code 가 만드는 `tsconfig.json`·`.claude-plugin\types\` 는 싣지 않는다) 설치 폴더 아래 같은 상대 경로로 둔다. 릴리스 zip 과 setup.exe(Tauri `bundle.resources`)가 똑같다.

```
<설치 폴더>\ClaudeBoard.exe, claude-board-hook.exe, LICENSE, README.md, THIRD_PARTY_NOTICES.txt (+ setup.exe 는 uninstall.exe)
<설치 폴더>\plugins\md-render\LICENSE
<설치 폴더>\plugins\md-render\README.md
<설치 폴더>\plugins\md-render\.claude-plugin\plugin.json
<설치 폴더>\plugins\md-render\hooks\hooks.json
<설치 폴더>\plugins\md-render\hooks\register.tsx
```

설치 폴더는 setup.exe 가 `%LOCALAPPDATA%\Claude Board`, `install.ps1` 이 `%LOCALAPPDATA%\Programs\Claude Board`(setup.exe 로 깐 폴더가 있으면 그쪽)다. 보드가 시작할 때 이 폴더를 `%USERPROFILE%\.claude\notifier\plugins\md-render` 로 옮겨 쓰고 그 경로를 `--plugin-dir` 로 넘긴다(§5.12.11). 옮겨 쓰는 까닭은 Claude Code 가 플러그인 폴더에 `tsconfig.json` 등을 쓰기 때문에 설치 폴더 안 경로를 바로 넘기지 않으려는 것이다. 확인 명령: `grep -rn "md-render" windows/app/src-tauri/src` 가 `mdrender.rs`(옮겨 쓰기)와 `settings.rs`(`mdRenderOff` 필드)를 가리킨다.
setup.exe 의 이 배치는 Tauri `bundle.resources`(`windows/app/src-tauri/tauri.conf.json`)가 정한다. 빌드가 만든 `windows/app/src-tauri/target/release/nsis/x64/installer.nsi` 에서 `$INSTDIR` 아래 `plugins\md-render\` 다섯 파일과 `THIRD_PARTY_NOTICES.txt`·`claude-board-hook.exe` 를 푸는 줄과 제거 구역이 같은 여섯 개를 지우는 줄을 읽어 확인했다(§11.2.1 G10 단계 8). 설치 프로그램을 실제로 실행해 확인한 것은 아니다.
`THIRD_PARTY_NOTICES.txt` 의 원본은 저장소의 `windows/packaging/THIRD_PARTY_NOTICES.txt`(xterm.js·애드온 셋·md-render 의 라이선스 원문)다. zip 은 자산 묶기 단계가 최상위에 복사하고, setup.exe 는 `bundle.resources` 에 같은 파일을 넣어 설치 폴더 최상위에 둔다.

setup.exe 메모(실기 확인): `/S` 로 같은 버전을 다시 설치하면 Tauri NSIS 는 옛 제거 프로그램을 부르지 않는다 — PREUNINSTALL 의 제자리 건너뛰기(`$EXEDIR == $INSTDIR`)는 GUI 업그레이드에서 쓰인다(제자리 제거 `uninstall.exe /S _?=<폴더>` 는 금고·hook·자동 실행 값을 남김을 확인). NSIS 제거 프로그램이 스스로 지우는 Run 값 이름은 `Claude Board` 이고 setup 이 쓰는 값은 `ClaudeBoard` 라, 자동 실행 값은 PREUNINSTALL 의 `uninstall` 이 지운다.

### 8.2 `install.ps1`

`irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/install.ps1 | iex` (처음 설치·업데이트 같은 명령). 인자 `-Version <v>`(없으면 API 로 최신 `windows-v*`), `-Update`(묻지 않음).
1) 처음 설치(settings.json 에 `.claude/notifier/` 가 없음)이고 `-Update` 가 아니면 hook 11개와 환경변수를 등록할지 묻는다(Enter = 예). 2) zip·sha256 받기 → 해시 확인(다르면 중단) → 임시 폴더에 풀기. 3) 실행 중인 `ClaudeBoard.exe` 종료 → `%LOCALAPPDATA%\Programs\Claude Board\` 에 바꿔 넣기(실패하면 되돌리기) → `Unblock-File`. 바꿔 넣는 단위는 zip 최상위 파일마다 하나와 `plugins\md-render\` 폴더 하나다. 원래 것은 `.old-<시각>`(폴더는 `plugins\md-render.old-<시각>`)으로 비켜 두었다가 모두 끝나면 지우고, 하나라도 실패하면 거꾸로 되돌린다. 2)가 끝나면 3)에 들어가기 전에 zip 에 `ClaudeBoard.exe`·`claude-board-hook.exe` 와 `plugins\md-render\` 의 다섯 파일이 모두 있는지, `plugins\md-render\` 밖 하위 폴더에 모르는 파일이 없는지 확인해 어긋나면 아무것도 건드리지 않고 `받은 zip 에 <경로> 가 없습니다.` 로 멈춘다. 4) `claude-board-hook.exe setup --app <경로>`. 5) 보드 실행. 6) 처음 설치면 "이미 실행 중인 Claude Code 세션은 다시 시작해야 hook 이 적용됩니다". Windows PowerShell 5.1 에서도 돌게 쓴다.
`uninstall.ps1`: 보드가 옮겨 쓴 `%USERPROFILE%\.claude\notifier\plugins` 삭제(macOS `Uninstall.command` 가 `plugins` 를 지우는 것과 같고 `claude-board-hook.exe uninstall` 과 같은 범위다. 그 안에는 보드가 옮긴 다섯 파일과 Claude Code 가 플러그인 폴더에 만드는 `tsconfig.json`·`.claude-plugin\types\` 뿐이다. 사용자가 따로 설치한 플러그인은 Claude Code 가 자기 `%USERPROFILE%\.claude\plugins` 에 둔다. 링크(junction)면 가리키는 곳은 지우지 않고 링크만 뗀다. 환경변수 `CLAUDE_BOARD_NOTIFIER_DIR` 로 notifier 를 옮긴 실행에서는 지우지 않는다. `claude-board-hook.exe uninstall` 이 notifier 를 비었을 때만 지우므로 그보다 먼저 한다) → `claude-board-hook.exe uninstall` → 앱 폴더 삭제(`plugins\md-render\` 포함) → 안내. setup.exe 로 제거할 때의 사본 삭제는 `claude-board-hook.exe uninstall` 이 맡는다.

### 8.3 CI·자동 릴리스 — `.github/workflows/windows.yml`

- 계기: `windows/**`·워크플로 파일이 바뀐 push(모든 브랜치)·pull_request, workflow_dispatch.
- build 작업(windows-latest): Rust stable, Node 22, `claude plugin validate plugins/md-render --strict`·`claude plugin test plugins/md-render`(Claude Code `@anthropic-ai/claude-code@2.1.288` 고정 설치, 인증 불필요)와 같은 검사를 `C:\Temp\한글 폴더\md-render` 사본에서, `cargo test`(windows 워크스페이스), `cargo build --release -p claude-board-hook`, 앱 `npm ci` → `npx tauri build`(NSIS), 자산 묶기(§8.1), `actions/upload-artifact` 로 올리기.
- smoke 작업(windows-latest, build 뒤): 아티팩트로 실제 설치·화면을 확인한다. 러너는 화면 세션이 있는 Windows Server 2025 이고 기본 터미널이 Windows Terminal 이다.
  `windows/ci/install-checks.ps1`(설치 확인 — 실패하면 작업 실패. 모든 설치 시나리오가 `plugins\md-render\` 다섯 파일의 유무를 확인한다. 시나리오 5 는 zip 에 파일이 빠졌을 때의 거부, 폴더가 막혔을 때의 되돌리기, uninstall.ps1 의 notifier 사본 삭제도 확인한다): setup.exe `/S` 설치 → 등록된 hook 을 가짜 Claude Code(node.exe 를 `claude.exe` 로 복사) 아래에서 그대로 실행 → 같은 버전 재설치·제자리 제거 → `/S` 제거, install.ps1·uninstall.ps1 을 Windows PowerShell 5.1 에서 웹 호출을 같은 이름의 함수로 가로채 실행(처음 설치 거부·`-Update` 설치·틀린 sha256 거부).
  `windows/ci/screen-checks.ps1`(화면 확인 — 정보용, `continue-on-error`): 보드 창·단일 실행·알림 패널의 포커스·확장 스타일·스스로 닫힘, 스크린샷. 결과표·로그·스크린샷은 `smoke-results` 아티팩트.
- release 작업: main 의 push 일 때만, build·smoke 뒤(설치 확인이 실패하면 릴리스하지 않는다). `windows/VERSION` 을 읽어 태그 `windows-v<버전>` 이 아직 없고 `windows/CHANGELOG.md` 에 `## <버전>` 항목이 있으면 그 항목을 설명으로 `gh release create windows-v<버전> … --latest=false --title "Claude Board for Windows <버전>"`. 항목이 없으면 실패로 끝낸다(macOS release.sh 와 같은 규칙).
- 배포 순서(관리자): `windows/CHANGELOG.md` 맨 위에 항목 → `windows/VERSION` 올리기(`windows/packaging/release.sh <버전>` — README·설치 가이드의 setup.exe·릴리스 링크도 바꾼다) → main 에 push → CI 가 빌드·릴리스.

## 9. 문서 의무

- `windows/README.md`(사용자용: 컨셉·필요 환경·설치·사용법·업데이트·제거·배포·macOS 와 다른 점), `windows/docs/설치 가이드.md`(모든 기능 자세히), `windows/CHANGELOG.md`(`## 2.12 — 2026-10-16` 첫 항목).
- 최상위 `README.md` 에 Windows 판 안내 한 절(설치 한 줄 + `windows/README.md` 링크), `CLAUDE.md` 에 Windows 구조·명령·"Latest 금지" 규칙, `.gitignore` 에 `!/windows/`·`!/.github/`.
- 사용자에게 보이는 기능이 바뀌면 `windows/CHANGELOG.md`·`windows/README.md`·`windows/docs/설치 가이드.md` 를 함께 고친다. 사용자에게 보이는 글자(앱 문자열·README·CHANGELOG·설치 가이드)에는 긴 줄표(—)를 쓰지 않는다.
- 설정 화면·단축키 바꾸기·로그인할 때 열기·설정 안 계정 추가를 다루는 사용자용 문서는 단축키를 바꾸면 전역 단축키(Ctrl+Alt+B·T)도 따라 바뀌고 다른 앱이 쓰는 키면 등록하지 못한다는 점, 로그인 창이 Google 로그인을 막을 수 있어 다른 브라우저에서 여는 길이 있다는 점을 싣는다.
- 내장 터미널을 다루는 사용자용 문서(README·설치 가이드)에는 Ctrl+Alt+T 가 JetBrains IDE 의 같은 단축키를 가져가고 AltGr 배열에서는 그 글자를 칠 수 없다는 점, 한국어 키보드에서는 왼쪽 Ctrl+Alt 로 누른다는 점을 싣는다.

## 10. 이 Mac 에서 확인할 수 있는 것과 없는 것

확인 가능: core·hook 단위 테스트(macOS 에서 `cargo test` — 중단 시각·status 변화·상태 덮기·인자 중복 제거·since 규칙 포함), Windows 대상 `cargo check`(링크 전까지), 프론트엔드 빌드, Tauri 앱을 macOS 에서 띄워 가짜 세션 파일로 화면 확인.
Windows CI 러너에서 실기로 확인: `#[cfg(windows)]` 시험(콘솔 제목·입력·터미널 창 찾기·headless, 프로세스 표·생성 시각·명령줄·종료, hook 이 `claude.exe` 를 조상으로 찾기, DPAPI, 새 콘솔 실행의 인자 전달)과 smoke 작업(§8.3)의 설치·화면 확인.
내장 터미널(§5.12)은 §11.2 의 "확인" 행과 §11.3~§11.6 에 적은 측정만 실기로 확인했다. 나머지 계획: ring 단위 시험은 어디서나, ConPTY 시험(`#[cfg(windows)]`)과 창 프레임 시험은 Windows 에서, 화면·IME·경계 끌기는 dev 보드를 원격 디버깅 포트로 띄워 DevTools 프로토콜로 조작해 본다(§11.2).
사람이 Windows 에서 확인해야 함: 실제 Windows 10(§11.3 의 순서), 탐색기가 보드에 트레이 아이콘 이벤트를 넘기는 동작과 트레이 메뉴 항목(이 PC 는 쓸 수 있는 트레이 아이콘이 없다, §11.2.1), Windows Terminal 탭 고르기, 세션 창 앞으로 가져오기, 트레이·전역 단축키·Mica, 소리, 실제 Claude Code 의 `.credentials.json` 형식·계정 전환, 실제 Claude Code 세션의 일시중지·재개·중단, 실제 세션에서 Esc 로 끊은 뒤 보드·TUI 가 대기로 바꾸는지와 승인 뒤 바로 작업 중으로 바꾸는지, 세션 이름 라벨, 진짜 Run 값을 쓰고 지우는 로그인할 때 열기, 배율이 큰 모니터의 패널 위치, 진짜 `claude auth login` 의 출력 모양과 `BROWSER` 준수, 로그인 창(Google 로그인 포함). 2단계 목록은 §11.2.2.

## 11. 재구현 의무 대응표

| ID | 의무 | macOS 근거 | Windows 위치 | 검증 |
|---|---|---|---|---|
| H1 | hook 상태 기록 규칙 | state.py:177-284 | crates/core `state` | 기준 데이터 테스트(REA 캡처 14단계) |
| H2 | 서브에이전트 기록 | state.py:146-174 | crates/core `state` | 기준 데이터 테스트 |
| H3 | carry_args·resume_cmd(Windows 인용) | state.py:57-97 | crates/core `state` | 단위 테스트 |
| H4 | prune | state.py:113-135 | crates/core | 단위 테스트 |
| H5 | 프로세스 찾기·alive·headless·제목 | state.py:24-110 | crates/core `proc`·`console` | Windows 실기 시험(CI) |
| H6 | 이어 열기 인자 중복 제거·새 요청의 since 갱신 | state.py:carry_args, state.py:update | crates/core `state` | 단위 테스트 |
| N1 | 알림 거르기·본문 다듬기·요약 | notify.py | crates/core `notify` | 단위 테스트 |
| N2 | 패널 띄우기(분리 프로세스) | notify.py:126-136 | crates/hook | smoke 화면 확인 |
| N3 | 알림 패널 화면·위치·슬롯·소리·클릭 | ClaudeNotifier.swift | app `--notify` + notify.html | macOS 실행 화면, smoke 화면 확인(비활성·스타일) |
| I1 | hook 등록·제거(exec form, 표시, 백업) | hooks.py | crates/core `hooks` | 단위 테스트 |
| I2 | setup·uninstall | user-setup.sh, Uninstall.command | crates/hook | smoke 설치 확인 |
| I3 | install.ps1·uninstall.ps1·해시 확인 | install.sh | windows/packaging | smoke 설치 확인(Windows PowerShell 5.1) |
| B1 | 세션 읽기·정렬·보정 | ClaudeBoard.swift Store.reload | src-tauri `store` | 단위 테스트(가짜 세션 폴더) |
| B2 | 모델·effort·컨텍스트 | ClaudeBoard.swift ModelReader·InterruptReader·ClaudeNames·ContextReader | src-tauri `readers` | 단위 테스트 |
| B3 | 사용 한도 | ClaudeBoard.swift UsageStore | src-tauri `usage` | 응답 파싱 테스트 |
| B4 | 계정 전환·추가·지우기 | ClaudeBoard.swift AccountStore | src-tauri `accounts`·`account_login` | 파일 조작 테스트(DPAPI 는 Windows). 추가·지우기는 ST6 |
| B5 | 한도 자동 전환 | ClaudeBoard.swift AutoSwitcher | src-tauri `autoswitch` | 선택 규칙 테스트 |
| B6 | 터미널 제어 | ClaudeBoard.swift focusGhostty·driveSession·TerminalAppStore | src-tauri `terminal` | Windows 실기 시험(콘솔 입력·창 찾기·새 콘솔), 탭 고르기 미확인 |
| B7 | 업데이트(Latest 금지) | ClaudeBoard.swift Updater | src-tauri `updater` | 릴리스 목록 파싱 테스트 |
| B8 | 설정 저장 | UserDefaults 키 | src-tauri `settings` | 단위 테스트 |
| B9 | 창·트레이·단축키·단일 실행·자동 실행 | ClaudeBoard.swift AppDelegate·SizeGuard | src-tauri `main` | macOS 실행(트레이), smoke 화면 확인(창·단일 실행) |
| U1 | 보드 화면 전체(단계 3개) | ClaudeBoard.swift BoardView·MiniBoard | app/src | macOS 실행 스크린샷 |
| U2 | 캐릭터 8종·말풍선 | ClaudeBoard.swift Foreman·BrainrotFigure | app/src/lib/mascot | 스크린샷 |
| U3 | 세션 이름 라벨 | ClaudeBoard.swift ClaudeNames | src-tauri `readers`·`store`, app/src `SessionRow.svelte` | 단위 테스트(이름 읽기·ended 숨김), macOS 실행 스크린샷, Windows 보드 화면(합성 세션 파일: 이름 라벨, 24자 넘는 이름 줄임) |
| T1 | TUI | board-tui.py | crates/hook `tui` | macOS 실행 |
| L1 | Esc 중단·Claude Code status 로 상태 덮기(알림 요약·TUI·보드) | notify.py·board-tui.py:interrupted_at·claude_status, ClaudeBoard.swift:InterruptReader·ClaudeNames, Store.reload | crates/core `claude_live`, `notify`, hook `tui`, src-tauri `store`·`readers` | 단위 테스트(가짜 대화 기록·가짜 claude 폴더), 합성 세션 파일로 보드 화면 확인(실제 세션 미확인) |
| R1 | CI·자동 릴리스(Latest 금지) | release.sh | .github/workflows/windows.yml | 브랜치 push 후 CI |
| D1 | 문서 | README.md, packaging/README.txt | windows/*.md, 최상위 README·CLAUDE.md | 검토 |
| TM1 | 터미널 코어: ConPTY 래퍼·환경 블록·링 버퍼·스레드·채널 프레임(§5.12.1~§5.12.4) | `TerminalHub`·`BoardSurface` (설계.md 7·11장) | src-tauri `pty/`·`ring.rs`·`terms.rs` | ring 단위 시험, `#[cfg(windows)]` 시험 8개(§11.2) |
| TM2 | 창 규칙: 펼침·접힘·보드 쪽·미니·Ctrl+Alt+T·트레이(§5.10) | `BoardRootView`, 보드 위치 메뉴 (설계.md) | src-tauri `window`·`settings`·`board`·`tray`·`model` | 프레임 단위 시험(펼침/접힘 × 양쪽 × 모니터 둘 × 배율 1.0·1.5, 미니 모서리), 설정 왕복 시험 |
| TM3 | 터미널 화면: xterm.js·IME·fit·흐름 제어·칸 배치, 경로 줄·탭 줄·빈 상태(§5.12.5, §7.2) | `BoardTerminalView`·`PaneDivider` | app/src/lib/terminal, app/src/routes | `npm run check`·`npm run build`, 번들 청크 검사, 브라우저 목 확인, 실기 조작(§11.2) |
| TM5 | 탭·새 세션: 탭이 셸을 쥐고 claude 를 쳐 넣음, `--session-id`·`--name`, 이름 바꾸기, 닫기 확인, 키(§5.12.6, §7.2) | `freshSessionCommand`·`named`·`BoardTab`·새 세션 창 (ClaudeBoard.swift BoardSurface.type·TerminalHub.named·freshSessionCommand·NewSessionSheet) | app/src/lib/terminal/hub.svelte.ts·launch.ts·NewSessionSheet 외 | `npm run check`·`npm run build`, 실기 조작(§11.2) |
| TM4 | 문서 | | SPEC §0·§5.7·§5.9·§5.10·§5.12·§6·§7·§8·§11, CHANGELOG 초안, README, 설치 가이드 | 검토 |
| TM6 | 이름 맞춤: 탭·화면·분할 트리·구성요소·세션 동작 어휘를 mac 이름으로 | `BoardTab`·`BoardSurface`·`SplitNode`·`TerminalTabBar`·`performAction` | app/src/lib/terminal, src-tauri `session_action.rs` | `npm run check`·`npm test`, `cargo test`(`session_action::tests`) |
| TM7 | 세션 짝짓기·위치 칩(§5.12.7) | `TerminalHub.pair`·`place`·`PlaceChip` | crates/core `place`, src-tauri `terms/pairing.rs`, app `terminal/place.ts`·`board/placeChip.ts` | 단위 시험, 실제 프로세스 시험(`#[cfg(windows)]`), 가짜 claude GUI 확인(§11.4) |
| TM8 | 클릭 흐름·구동·이어 열기·`/rename`(§5.7) | `openSession`·`driveSession`·`resumeSession`·`renameSessions` | src-tauri `drive.rs`·`open_session.rs`·`terminal.rs`, app `board/openSession.ts`·`board/clickGate.ts`·`terminal/sessionFlow.svelte.ts`·`sessionCommand.ts`·`renamePlan.ts` | 단위 시험, 키를 받아 적는 가짜 claude(`ci/fake-claude-tty.cjs`), 실기 G7(§11.4: 결과 자리 비어 있음) |
| TM9 | 크기 바꾸기 장벽(§5.12.8) | | app `terminal/resizeRules.ts`·`repaintScanner.ts`·`resizeBarrier.ts`·`barrierWrites.ts`, src-tauri `term_resize` | 캡처 측정·재생·보드 안 끝에서 끝까지(§11.5) |
| TM10 | DOM 렌더러 칸 맞춤(§5.12.5) | | app `TerminalPane.svelte` | `npm run dom-metrics`(수정 뒤 실행 결과는 이 문서에 없다) |
| TM11 | md-render 패키징(§8.1~§8.3) | 앱 `Contents/Resources/plugins/md-render` | windows/packaging, `windows.yml`, `ci/install-checks.ps1` | 샌드박스 5.1 설치 확인(§11.6). 앱 쪽 옮겨 쓰기는 TM15 |
| TM12 | Windows 10 대용 확인(§11.3) | | | Windows 11 에서 흉내 낸 확인. 실제 Windows 10 은 미확인 |
| TM13 | 탭 저장과 복원(§5.12.9) | `saveTabs`·`restoreTabs`·`resumeHeld`·`restoreCommand` | app `terminal/restore.ts`·`lifecycle.svelte.ts`, src-tauri `shutdown.rs`(`save_tabs`·`load_tabs`·`existing_dirs`)·`proc_wait.rs` | 순수 함수 단위 시험(`restore.test.ts`·`restoreGuards.test.ts`), 가짜 claude GUI G9(§11.2.1). 새로 고침 뒤 사라지는 탭·지워진 폴더의 `.paused` 는 한계 |
| TM14 | 끌 때 확인(§5.12.10) | `confirmShutdown`·`ShutdownList`·`busyTabSessions` | src-tauri `shutdown.rs`·`tray.rs`·`updater.rs`, app `terminal/shutdownList.ts`·`ShutdownList.svelte` | 우편함 상태기계·목록 규칙 단위 시험, G9(§11.2.1). 트레이 아이콘 이벤트는 흉내, 업데이트 경로는 GUI 미실행 |
| TM15 | 앱 쪽 md-render(§5.12.11) | `mdRenderDir`·`mdRenderCommand`·`mdRenderOff` | src-tauri `mdrender.rs`, app `terminal/mdRender.ts`·`mdRenderLoad.ts`·`NewSessionSheet.svelte`, `tauri.conf.json` | 옮기기·명령 규칙 단위 시험, G10(§11.2.1). 실제 Claude Code 의 렌더링은 미확인 |
| TM16 | 보드로 옮기기·알림에서 세션으로(§5.12.12) | `moveToBoard`·`ClaudeNotifier` 의 session 클릭 | app `board/moveToBoard.ts`·`board/sessionActions.ts`·`terminal/sessionFlow.svelte.ts`, src-tauri `notify.rs`·`board.rs`, crates/core `notify.rs` | 단위 시험, G10(§11.2.1, 클래식 콘솔만) |
| TM17 | 경로 줄 밀기·맨 위 고정·투명 규칙(§5.12.13, §5.10) | mac 3.1.8·3.1.9 | app `terminal/stripScroll.ts`·`board/opacity.ts`, src-tauri `window.rs`·`board.rs` | 순수 함수·프레임 단위 시험, G10(§11.2.1), 경로 줄 되돌림 수정은 G11 행 10 에서 재확인(§11.2.2) |
| H7 | 서브에이전트 시작 표지 | state.py `track_subagent`, `Sub.parse` | crates/core `state`, src-tauri `store` | 단위 시험, G10 단계 7 |
| ST1 | 설정 화면 껍데기: 열기·닫기·Esc 차례·접힌 칸 펼침과 되접기·검색·전환(§5.14.1) | `SettingsScreen`·`SettingsItem`·`SettingsSection`·`TerminalAreaView.present` | app `settings/screen.svelte.ts`·`screenRules.ts`·`search.ts`·`motion.ts`·`sections.ts`·`items.ts`·`SettingsView.svelte`·`SettingsSidebar.svelte` | 순수 함수 단위 시험(`node --test`), G11(§11.2.2). 열린 채 끄는 길과 보드 왼쪽 전환 방향은 한계 |
| ST2 | 묶음 화면과 항목 표(§5.14.2), 알림 위치 그림 고르기 | `SettingsItem`·`NotifyPositionPicker` | app `settings/groups/*`·`ScreenPicker.svelte`·`screenPick.ts`, src-tauri `settings.rs`·`board.rs` | 단위 시험, G11 행 6 |
| ST3 | 새 세션 기본 명령·답변 꾸미기 기본값과 검사(§5.14.4) | `newSessionCommand`·`newSessionMdRender` | app `terminal/newSessionDefaults.ts`·`settings/groups/Terminal.svelte`·`NewSessionSheet.svelte`, src-tauri `settings.rs`·`board.rs` | 단위 시험, G11 행 7. 줄바꿈 분기는 GUI 로 닿지 않는다 |
| ST4 | 로그인할 때 열기·자동 업데이트 확인·트레이 메뉴(§5.14.5) | `LoginItem`·`autoUpdateCheck` | src-tauri `login_item.rs`·`updater.rs`·`tray.rs`, crates/hook `setup.rs`, crates/core `paths.rs` | 가짜 Run 키 단위 시험, G11 행 6(격리 표지 파일만). 진짜 Run 값 쓰기는 미확인 |
| SK1 | 단축키 바꾸기: 모델·규칙·받기·dispatch·전역 단축키(§5.14.3) | `Shortcuts`·`KeyCombo`·`ShortcutSettings`·`shortcutItems` | app `shortcuts/*`·`settings/groups/Shortcuts.svelte`, src-tauri `hotkeys.rs` | 단위 시험(규칙·받기·dispatch·게이트), 가짜 등록기 시험(`hotkeys::tests`), G11 행 8. `RESERVED` 는 판단 목록 |
| ST6 | 설정 안 계정 추가·지우기 확인(§5.14.6) | `loginOutput`·`loginEnded`·`LoginWebWindow`·`loginURL`·`loginNote`·`confirmRemove`·`AccountLoginPanel` | src-tauri `account_login.rs`·`account_login/`·`login_output.rs`·`accounts.rs`, app `settings/groups/Account.svelte`·`accountLogin.ts`·`board/AccountChip.svelte` | 단위 시험(출력 읽기·상태 기계·가짜 로그인 끝까지), G11 행 9(가짜). 진짜 `claude auth login`·로그인 창은 미확인 |
| ST5 | 문서 | | SPEC §5.14·§11.2.2·§11.7, CHANGELOG, README, 설치 가이드 | 검토 |
| TT1 | GUI 시험 장치(§5.13) | | `ci/gui`·`ci/jev`·`ci/fake-claude-*.cjs`, src-tauri `test_claude.rs` | `node --test` 시험(§5.13), G9·G10 에서 사용. Jev 는 키 없음 |

### 11.1 검증 결과 (2026-10-16, 첫 판 2.12)

Windows 실기 확인은 GitHub Actions windows-latest 러너(Windows Server 2025, 화면 세션 있음, 기본 터미널 Windows Terminal)에서 했다 — `#[cfg(windows)]` 시험은 실행 38913340492, 설치·화면 확인(smoke)은 실행 38916177014.

REA 의 `evaluate-reconstruction-coverage`·`build-reconstruction-obligation-ledger` 는 REA 가 배포 바이너리에서 뽑은 인증 근거와 원본·재구현 양쪽의 실행 사례를 입력으로 요구한다.
원본은 소스가 모두 있고(REA 지침상 소스 저장소 분석에는 REA 를 쓰지 않는다) 재구현은 Windows 에서만 돌므로 그 도구는 돌리지 않았다. 아래 표가 그 대신이다.
REA 를 쓴 곳: `import-reference-source`(원본 소스 목록), `capture-process`(state.py 실행 근거 `ev_23674ebbd76cceea4252d166b60c1ad66648ee3f2a951ba5d4c669d1c07d3368` — H1·H2 기준 데이터).
`observe-native-ui` 는 메뉴 막대 전용 앱의 실행 시각을 읽지 못해 REA 가 거부했다(`process-identity-unavailable`).

| ID | 상태 | 근거 |
|---|---|---|
| H1·H2 | 확인 | REA 캡처 14단계와 같은 결과(환경 의존 필드는 가림). macOS·Windows CI 에서 시험 통과 |
| H3·H4 | 확인 | 단위 시험(적대적 모델 이름이 인자 하나로 남는지 포함). `resume_args` 로 셸을 거치지 않음 |
| H5 | 확인 | Windows 실기 시험: 프로세스 표·부모·생성 시각·명령줄(공백·따옴표·한글)·종료, 콘솔 제목 왕복·붙기, 터미널 창 찾기·headless(§3.4). hook 이 새 콘솔의 가짜 `claude.exe` 를 부모로 찾아 pid·생성 시각·`resume_args`(적대적 모델 이름이 원소 하나)를 남기고 제목을 바꾸며, `-p` 는 headless·제목 그대로 |
| N1 | 확인 | notify.py 와 같은 출력 단위 시험 |
| N2·N3 | 확인(소리 제외) | 설치된 notify hook 이 패널을 기다리지 않고 30ms 안에 끝나고, 패널이 뜨되 포그라운드 창이 그대로, 확장 스타일 NOACTIVATE·TOOLWINDOW·TOPMOST·APPWINDOW 없음, 스스로 닫힘. 소리는 **Windows 미확인** |
| I1 | 확인 | 등록·제거 왕복·백업·깨진 JSON 거부 시험 |
| I2·I3 | 확인 | setup.exe `/S` 설치(설치 폴더·hook 13개 명령·env·백업·다른 설정 보존·hook 사본·app-path·Run 값·명령 두 개) → 등록된 hook 을 가짜 Claude Code 아래에서 실행 → 재설치·제자리 제거(금고 보존) → `/S` 제거(원래 settings.json 으로 돌아감, 로그인 파일 그대로). install.ps1 을 Windows PowerShell 5.1 에서: 처음 설치 리디렉션 거부, `-Update` 설치(macOS Latest·draft·prerelease 를 건너뛰고 windows-v 고름), 틀린 sha256 거부(기존 설치 그대로), uninstall.ps1. 여기서 업데이트 단추의 PSModulePath 버그를 찾아 고쳤다(§5.8) |
| B1~B5·B7·B8 | 확인 | src-tauri 단위 시험(Windows CI 에서도 통과). DPAPI 왕복·망가진 봉인 거부·금고 파일에 토큰 평문 없음을 Windows 실기 시험으로 확인. 실제 Claude Code 계정으로 전환은 **Windows 미확인** |
| B6·B9 | 부분 | 콘솔 입력(줄 단위·raw 모드, WT 탭 포함)·새 콘솔 실행(인자·환경·작업 폴더 그대로)·보드 창·단일 실행을 실기로 확인. 탭 고르기·창 앞으로·트레이·단축키·Mica 는 **Windows 미확인** |
| U1 | 확인 | macOS 에서 Tauri 창을 가짜 세션으로 띄워 스크린샷, 브라우저에서 세 단계·알림 패널을 원본 스크린샷과 비교 |
| U2 | 확인 | Swift 그림 코드를 macOS 에서 렌더링한 기준 그림과 픽셀 비교(가장자리 차이 0.7% 미만) |
| T1 | 확인 | board-tui.py 와 화면 바이트 비교 |
| R1 | 확인 | Windows CI build·release(dry run) 통과(실행 38903798305) — 자산 §8.1 과 같고, 만들 명령에 `--latest=false --target <커밋>`. main 병합 뒤 실행 38923609837 이 build·smoke·release 를 통과해 `windows-v2.12` 를 게시했다 — 자산 6개, zip SHA256 일치, Latest 는 macOS `v2.12` 그대로 |
| D1 | 확인 | 문서 작성·검토 |

2.13 추가분(H6·L1·U3)은 단위 시험과, Windows 11 에서 NSIS 로 빌드한 `ClaudeBoard.exe` 를 합성 세션 파일(`CLAUDE_BOARD_HOME` 격리 홈)로 띄운 화면 확인을 거쳤다. 실제 Claude Code 세션으로는 확인하지 않았다.

| ID | 상태 | 근거 |
|---|---|---|
| H6·L1·U3 | 부분 확인 | 합성 세션 파일로 보드 화면 확인(Esc 로 끊긴 기록 → 대기와 끊은 때부터의 경과 시간, 승인 대기 + status busy → 작업 중, 24자 넘는 이름 줄임, 같은 `project_dir` 한 카드). 단위 시험 대상: 인자 중복 제거·새 요청의 since·중단 시각 파싱(CRLF·BOM·256,000 경계·tool_result·sidechain)·status 변화·상태 덮기 진리표·알림 요약(다른 세션만 덮음)·TUI 읽기·보드 읽기(중단→대기, 승인 뒤 working, 서브에이전트가 돌면 working, paused 우선, ended 면 이름 숨김). 실제 세션 실측: hook 이 기록한 `pid` 10928 = 같은 세션 id 의 `~\.claude\sessions\10928.json`(`sessionId` 일치), `pid_ctime` 1792619617.90832 = `procStart` 134370932179083200 환산값(차 0). 조상 `claude.exe` 는 Git Bash 의 `bash.exe` 세 단계 위. **untested:** Esc 로 끊은 실제 세션에서의 동작 |

### 11.2 내장 터미널 검증 상태

`확인` 은 아래 두 번의 실기 확인(2026-10-21·2026-10-22)과 §11.2.1 의 1단계 GUI 확인이고 나머지는 `미확인` 이다. 상태를 바꿀 때는 근거(실행한 명령과 결과)를 같은 행에 적는다.

| ID | 상태 | 확인한 것 |
|---|---|---|
| TM3·TM5 | 확인 (2026-10-21, Windows 11, 빌드한 앱, 가짜 세션 파일) | 빈 상태와 탭 줄·경로 줄 그려짐 / + 단추로 탭 열기 / 새 세션 창에 보드 경로가 목록으로 뜸 / 새 세션이 pwsh 탭에 `claude --name alpha --dangerously-skip-permissions --session-id <uuid>` 를 쳐 넣어 시작되고 Claude Code 가 바르게 그려짐 / 접고 펼쳐도 출력 유지 / 셸 exit 뒤 PowerShell 프롬프트 / 셸이 창 크기를 99x44(열x줄)로 앎 |
| TM5 | 확인 (2026-10-22, Windows 11 10.0.26300, 실제 ConPTY 위 `%ComSpec%`, 보드와 같은 입력: 줄을 쓰고 100ms 뒤 CR) | `sessionCommand.ts` 가 만든 줄 43개(대조군 2개는 따로)를 cmd.exe 탭에 쳐서 PATH 앞의 인자 기록 프로그램(`claude.exe`, .NET Framework 4)이 받은 argv 를 쟀다. 폴더 이름 34종(보통·공백·한글·`&`·파이프·`^`·`%COMSPEC%`·`%PATH%`·`!COMSPEC!`·`"`·`'`·괄호·끝의 `\`·`\"`·`,;=`·굽은 따옴표·`<>`·줄바꿈·`--` 로 시작·빈·공백뿐·`con`·`...`·전각 글자·255자 한글·300자) 모두 호출 1회, argv 는 `--name <정리한 이름> --dangerously-skip-permissions --session-id <uuid>` 다섯 칸, 두 번째 명령 없음. 사용자가 적은 명령 9종(`--model` 등 실어 보내는 인자·`--resume <id>`·`-r`·`--resume=abc`·`--continue`·`--name` 이미 있음·옛 `--session-id` 교체·`claude.exe`·따옴표 인자)도 기대한 argv 그대로. 시작 폴더 이름의 `&`·`(` 는 영향 없음. 대조군: 정리하지 않은 `a & claude --name PWNED` 는 호출 2회, `a & echo x> "%CANARY%"` 는 카나리 파일이 생겨 이 측정이 주입을 잡는다. 같은 입력의 `node --test` 사본은 PATH 앞의 `claude.cmd` 중계로 돈다 |

아래 행은 `미확인` 이다. 위 확인과 겹치는 항목도 그 행의 나머지 조건을 채우기 전에는 바꾸지 않는다.

| ID | 상태 | 확인할 것 |
|---|---|---|
| TM1 | 미확인 | `ring.rs`: 순서 연속성, 넘칠 때 오래된 것 버림, gap 감지, seq 부터 다시 보내기. `cargo test --manifest-path windows/app/src-tauri/Cargo.toml -- --test-threads=1` 통과와 같은 매니페스트의 `cargo clippy --all-targets` 경고 없음 |
| TM1 | 미확인 | `#[cfg(windows)]` 시험 ① `cmd /c echo 한글 & exit 3` 이 UTF-8 출력과 종료 코드 3 ② raw 모드 node 에코가 `\x1b`·`\x03`·한글·문구를 바이트 그대로 받음 ③ 100×40 으로 resize 하면 자식이 `columns == 100` 으로 봄 ④ 뷰어 없이 2MiB 를 쏟아도 자식이 끝남 ⑤ kill 이 6초 안에 끝남 ⑥ 파이프 끝의 `HANDLE_FLAG_INHERIT == 0` ⑦ 기본 셸이 이 PC 에서 pwsh 로 풀리고 `exit\r` 가 종료 코드 0 ⑧ 환경 블록에 `PATH` 가 있고 시험이 `CLAUDECODE` 를 설정해도 없음 |
| TM1 | 미확인 | `WindowsApps\pwsh` 별칭에 대한 `CreateProcessW` + 가짜 콘솔 속성, 널 표준 핸들(`STARTF_USESTDHANDLES`)이 콘솔에서 띄운 보드에서 자식에게 콘솔 핸들을 물려주지 않는지, JS `Channel` index 순 전달 |
| TM2 | 미확인 | 프레임 시험(펼침/접힘 × 보드 쪽 양쪽 × 모니터 둘 × 배율 1.0·1.5, 미니가 양쪽에서 맞는 모서리)과 설정 왕복 시험(새 키 넷) 통과 |
| TM3 | 미확인 | `cd windows/app && npm ci && npm run check && npm run build` 통과, `build/notify/index.html` 의 청크에 xterm 이 없음, 브라우저 목(`npm run dev`)에서 칸·경계·에코가 보임 |
| TM3 | 미확인 | dev 보드를 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 로 띄워 조작: Ctrl+Alt+T 로 펼침(전체 작업 영역, 항상 위 해제, 불투명도 1) / 3초 안에 PowerShell 프롬프트 / 경계 끌기 뒤 `[Console]::WindowWidth` 와 보이는 열 수가 같음 / 접으면 끌어 둔 폭으로 보드 쪽에 붙음 / 왼쪽 보드도 같음 / `1..300000 \| % { "line $_" }` 직후 접고 20초 뒤 펼치면 마지막 줄 `line 300000` / `echo 한글 테스트 😀` 폭 / `cmd /c "exit 7"` / `exit` 하면 그 탭이 닫히고 마지막이면 빈 상태 / 페이지 새로고침(Ctrl+R)에도 같은 셸로 돌아옴 / `claude` 를 손으로 실행해 Esc·Shift+Enter·크기 바꾸기·`/exit` / 트레이에서 보드를 종료하고 5초 안에 pwsh·claude 가 사라짐 |

탭·새 세션에서 미확인(`untested:`): 실제 한글 IME 입력, Windows 10, 배율이 다른 모니터 여러 개, 폴더 선택 대화상자("다른 폴더 고르기…"), cmd.exe 의 `DelayedExpansion`·`AutoRun` 레지스트리를 켠 PC(위 측정은 기본 설정), 인자 기록 프로그램 대신 실제 Claude Code 의 인자 파서, 화면 나누기(미구현), Ctrl+Alt+1~9·Ctrl+T·Ctrl+Shift+R 의 실제 키 입력, 닫기 확인 창, 이름 바꾸기, 같은 폴더 이름의 `부모/이름` 표시. 경로 줄 밀기·탭 저장과 복원·끌 때 확인·답변 꾸미기는 §11.2.1 에서 가짜 claude 로 확인했고 그 표의 `untested:` 가 남은 것을 소유한다.
정직한 잔여: 커서 깜박임 누수를 막는 `app/src/lib/terminal/webglBlink.ts` 의 덮개는, 프로그램이 DEC 비공개 모드 `?12h` 를 켠 상태에서 WebGL 컨텍스트가 끊기는 길을 덮지 못한다. 업스트림 `WebglRenderer.ts:363` 이 `decPrivateModes.cursorBlink ?? options.cursorBlink` 를 읽는데, `reset()` 이 화면 상태를 지우므로 덮개가 부르지 않는다. 컨텍스트 끊김은 드물다.
미구현 목록은 §11.7 이 소유한다.

이 PC 로 확인할 수 없는 것(`untested:`): 보드를 강제 종료했을 때 시스템 conhost 의 동작(CI 로 본다), Windows 10, 배율이 다른 모니터 여러 개, 원격 데스크톱의 WebGL, AltGr 배열과 JetBrains 의 Ctrl+Alt+T 충돌, 실제 한글 IME(TSF) 입력 경로(DevTools 프로토콜로는 흉내만 낼 수 있다),
탭에서 손으로 띄운 실제 claude 를 hook 이 어떻게 분류하는지. 3초 탐침(`can_attach`)이 보드 터미널의 pid 에는 붙지 않게 건너뛰도록 짰으나(`terms/pairing.rs` 의 `no_attach`) 붙지 않았음을 로그로 직접 증명하지는 못했다(`board.log` 에 `can_attach` 의 흔적을 남기지 않는다).

#### 11.2.1 1단계(수명 주기·답변 꾸미기·옮기기·알림 이동·밀기·맨 위 고정) GUI 확인

Windows 11 10.0.26300, 화면 1280x720 배율 100% 한 대, 격리 홈과 가짜 claude(§5.13)로 실행했다. 근거는 작업 기록의 `evidence-g9.md`(G9, 수명 주기)·`evidence-g10.md`(G10, 나머지)·코드 검토 `review-p1-opus.md`(저장소 밖)다.
G9 의 exe 는 커밋 `122ef91` 에서 `npx tauri build --no-bundle` 로, G10 의 exe 는 커밋 `f18cb6f` 에서 `npx tauri build --bundles nsis` 로 빌드했고 둘 다 빌드 뒤 `windows/app/src`·`src-tauri/src`·`crates`·`ci` 에 더 새로운 파일이 없음을 `find … -newer` 로 확인했다. 실행 전후 철선(§5.13)은 둘 다 `TRIPWIRE|OK`(진짜 claude 줄 없음, `settings.json`·`profile.ps1` md5 같음)이고 실행 뒤 `git status --short` 가 비어 있다.
Jev 칸은 모든 행이 `UNVERIFIED|no Jev key` 다(`node windows/ci/jev/jev.mjs check --image windows/e2e/out/<그림>.png --family chip`). 질문 가족 `chip` 은 보정 전이라 기록만 하고 통과로 세지 않는다. 아래 판정은 결정적 증거(DOM 읽기·파일·argv 기록·키 기록·창 스타일)만으로 한 것이다.

G9 수명 주기(트레이 **종료**는 이 PC 에 쓸 수 있는 트레이 아이콘이 없어 아이콘 이벤트를 흉내 냈다. 아래 「트레이 흉내」):

| 단계 | 확인한 것 | 판정 |
|---|---|---|
| a1 | 탭 둘에서 트레이 종료: 확인 창 제목 `작업 중인 세션 2개가 꺼집니다`, 줄 둘(`<폴더> · fake-claude-tty start`, 작업 중), 탭 안 `-p` 가짜는 목록에 없음, board.log 에 폴백 줄 없음 | 통과 |
| a2 | 취소: 창이 닫히고 보드 프로세스가 그대로, `.paused` 없음 | 통과 |
| a3 | 탭 셋에서 다시 종료 후 확인: 보드 프로세스가 사라짐. `.paused` 는 탭 셋의 주 세션에만, 탭 안 `-p` 가짜와 시드 세션에는 없음. 탭 셸의 가짜는 죽고 `-p` 가짜는 계속 돎 | 통과 |
| a4 | 종료 뒤 `terminalTabs` 에 셋 모두(고른 탭 표시 포함), 항목마다 `command: "claude"`·세션 id·cwd | 통과 |
| b1 | 다시 켜면 고른 탭이 곧바로 이어 열림(argv `--resume <id>`, hook 이 `.paused` 를 지움) | 옛 빌드는 부분 통과: 이어 열기가 처음 인자(`--dangerously-skip-permissions` 등)를 잃었다. 수정(`f18cb6f`) 뒤는 G10 단계 2 가 통과 |
| b2 | 고르지 않은 claude 탭은 그 탭에 들어갈 때 이어 열림(경로 칩 클릭 뒤 argv 한 줄 늘고 `.paused` 지워짐). 들어가기 전의 화면은 셸도 아직 뜨지 않은 상태다 | 옛 빌드는 b1 과 같은 이유로 부분 통과 |
| b3 | 작업 폴더를 지운 탭: claude 줄이 argv 에 없음, board.log `탭을 되살리지 않음: 작업 경로가 없습니다 …`, 탭 자체가 화면에서 빠지고 `.paused` 는 남음 | 통과(설계보다 엄격: 탭이 통째로 빠진다) |
| c1 | 확인 창이 떠 있을 때 웹뷰 새로 고침: 창이 닫히고 board.log 에 이어받음·취소 줄. 다시 종료하면 같은 목록의 창이 다시 뜸 | 통과 |
| c2 | 창이 숨은 채 종료: 창이 보이고(`IsWindowVisible` 거짓에서 참) 줄 둘의 창이 뜸 | 통과 |
| c3 | 창이 이미 열린 채 종료를 또 누름: 앞 요청이 취소되고 새 요청이 이어받아 창이 그대로 | 통과 |
| c4 | 답하면 보드가 끝나고 `.paused` 가 남으며 `terminalTabs` 는 끝내기 전과 같음 | 통과 |
| d | 이어 열기만 하고 들어간 적 없는 탭이 있는 채 웹뷰 새로 고침: 저장 목록은 새로 고침 12초 뒤에도 바이트 단위로 같고(`cmp`), 다음 보드 시작에 두 탭이 다시 돌아옴 | 통과. 부작용: 그 탭이 화면에서는 다음 시작까지 사라짐(§5.12.9 한계) |
| e | 업데이트할 것이 없으면 확인 창이 없음 | 코드(`updater.rs` 의 `can_start`)와 스냅샷(`updater.latest` 가 null)과 단위 시험으로만 통과. GUI 에서 업데이트 항목을 누르지 않았다 |

트레이 흉내: 쉘이 보드의 트레이 창에 보내는 오른쪽 클릭 메시지(`WM_USER_TRAYICON` 6002 + `WM_RBUTTONUP`)를 보드 자신의 트레이 창에 직접 보내 네이티브 메뉴를 열고, 항목은 진짜 마우스 클릭으로 골랐다. 메뉴 → `on_menu("quit")` → `shutdown::quit` 길은 진짜이고, 탐색기가 보드에 아이콘 이벤트를 넘기는 부분만 흉내다. 시험 끝에 이 PC 의 알림 영역 설정에 임시로 쓴 값 하나를 지웠다.

G10 나머지 기능(보드는 세 번 띄웠다: 격리 폴더 A, A 를 끄고 다시 켠 것, 공백·한글이 든 폴더 B):

| 단계 | 확인한 것 | 판정 |
|---|---|---|
| 1a | 첫 시작에 `plugins\md-render` 가 정확히 다섯 파일(`LICENSE`·`README.md`·`plugin.json`·`hooks.json`·`register.tsx`) | 통과 |
| 1b | 보드를 끄고 다시 켠 둘째 시작은 아무것도 다시 옮기지 않음(파일 목록과 수정 시각이 같음) | 통과 |
| 1c | md 켠 새 탭의 claude argv 에 `--plugin-dir <notifier>\plugins\md-render` 가 정확히 하나(여러 탭에서 줄마다 1) | 통과 |
| 1d·1e | 체크를 푼 새 세션은 `--plugin-dir` 없이 시작하고 `mdRenderOff` 에 기록됨. 그 세션을 `/exit` 로 끝내고 "세션 다시 열기"로 이어 열어도 `--plugin-dir` 이 없음 | 통과 |
| 1f | 공백·한글이 든 홈 경로에서도 `--plugin-dir` 하나에 전체 경로가 온전함 | 통과 |
| 2 | 이어 열기가 처음 인자를 보존(`--model m`·`--dangerously-skip-permissions` 유지, md-render `--plugin-dir` 정확히 하나) — G9 b1 의 수정 확인. 선택 탭 재시작 이어 열기도 같은 인자 유지 | 통과 |
| 3a | 클래식 콘솔의 가짜: 세션 줄 칩 `터미널`, … 메뉴에 일시중지·보드로 옮기기·세션 중단 | 통과 |
| 3b | 작업 중이면 "보드로 옮길까요?"(옮기기·취소), 취소는 키를 보내지 않음. 쉬는 세션은 묻지 않고 바로 옮김 | 통과 |
| 3c | 키 기록: Esc(`1b`) → `/exit` → `0d` 사이가 세 번 재서 399/201ms·399/201ms·400/200ms, 그 뒤 SessionEnd hook, 종료 코드 0 | 통과 |
| 3d | 보드 탭이 열리고(칩 `CB`) argv `--resume <id>` 에 원래 인자 유지. 쉬는 세션도 같음 | 통과 |
| 3e | `/exit` 를 무시하는 가짜: 0.5초 뒤 같은 항목을 또 눌러도 둘째 창이 없고 키 기록에 키 묶음이 한 번뿐, 8초 뒤 알림 `…세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요` | 통과(둘째 누름의 첫 시도는 시험 쪽 선택자가 줄을 못 잡아 다시 함) |
| 4a | 보드를 X 로 숨기고 `--notify … --session <id>` 패널을 진짜 마우스로 누름: 보드가 보이고 저장된 선택 탭이 그 세션의 탭으로 바뀜 | 통과 |
| 4b | 모르는 세션 id 의 패널 클릭: 보드가 보이고 탭은 그대로, board.log `--action focus: 세션을 찾지 못했습니다 (…)` | 통과 |
| 5a | 경로 11개(긴 이름)에서 줄 `scrollWidth` 2507 / `clientWidth` 818, 줄임표로 잘린 이름 0, 전체 이름이 보임 | 통과 |
| 5b | Shift+휠: 휠 이벤트가 막히고(`defaultPrevented` 참) `scrollLeft` 1689 에서 1289 로 가지만 150~1000ms 뒤 지금 경로로 되돌아옴 | 실패. 원인은 경로 목록 배열을 의존한 효과(상태 묶음마다 다시 돎)였고 `77bacc3` 이 경로 개수만 의존하도록 고쳤다. 고친 뒤 GUI 재확인은 §11.2.2 행 10 에서 통과 |
| 5c | 칩 클릭·재시작·창 폭 줄이기 뒤에도 지금 경로가 보임(`scrollLeft` 0·817·171·1689) | 통과 |
| 5d | 탭 8개에서 탭 줄 `scrollWidth` 974 > 734, **+ Ctrl+T** 단추가 미는 칸 밖 오른쪽 끝, 손으로 민 탭 줄은 2초 동안 되돌아가지 않음 | 통과 |
| 6a | 접고 고정: 맨 위(`WS_EX_TOPMOST`) | 통과 |
| 6b | 작업 영역 전체로 펼침: 맨 위 아님(처음과 접고 다시 펼친 뒤 모두) | 통과 |
| 6c | 진짜 가장자리 끌기로 약 60%(848x465, 902x696)로 줄임: 0.5초 뒤와 2초 넘게 멎은 뒤 모두 맨 위 | 통과 |
| 6d | 고정 단추를 끄면 맨 위 아님, 켜면 맨 위 | 통과 |
| 6e | 줄였던 창을 끌어 작업 영역을 덮게 만듦: 0.3초 뒤 맨 위 아님이고 이후 읽기 4번 동안 변함 없음. 처음에 틀렸다가 스스로 고쳐지는 경우는 만들지 못했다 | 부분 통과 |
| 6f | 슬라이더 0.5, 마우스가 창 안이면 불투명, 848x465 창 밖(1200,100)이면 보드가 앞(`document.hasFocus` 참)이어도 문서 불투명도 0.5 | 통과 |
| 6g | 작업 영역 전체를 덮은 창은 마우스가 작업 표시줄 위(창 밖)여도 불투명 1. 접고 펼치기 4번 모두 1 | 통과. 단 처음 한 번 0.5 로 남은 것을 봤고 같은 조작 4번에서는 다시 나오지 않았다(아래 `untested:`) |
| 7 | 서브에이전트: 시작 기록 없는 기록만 있으면 세션 상태 `idle`·`subagents` 빈 목록, 시작 기록 있는 것이 더해지면 `working` 과 하위 작업 1(숨은 기록의 글자는 화면에 없음) | 통과 |
| 8 | 설치 배치: 빌드가 만든 `installer.nsi` 줄 646~667 이 `$INSTDIR` 아래 `plugins\md-render\` 다섯 파일과 `THIRD_PARTY_NOTICES.txt`·`claude-board-hook.exe` 를 풀고, 줄 787~808 의 제거 구역이 같은 여섯 개를 지움. `makensis` 가 성공했으니 원본 파일이 모두 있다 | 통과(스크립트를 읽은 것이고 설치 프로그램을 실행하지 않았다) |

G9·G10 이 확인하지 못한 것(`untested:`):
- 탐색기가 보드로 트레이 아이콘 이벤트를 넘기는 부분과 트레이의 다른 메뉴 항목. 이 PC 에는 승격할 수 있는 트레이 아이콘이 없다(작업 표시줄에 숨겨진 아이콘 단추가 없다).
- 실제 Windows 10, 모니터가 둘인 PC(알림 패널·창 배치), 원격 데스크톱의 WebGL, AltGr 배열, 터치패드 가로 밀기, 실제 Windows Terminal 에서 도는 세션의 보드로 옮기기(G10 은 클래식 콘솔만).
- 실제 Claude Code 의 키 처리: 이어 열기의 `--plugin-dir` 처리·`/exit` 의 실제 응답 시간·`resume_args` 보존.
- GUI 에서의 업데이트 경로(`confirm_update` 를 누르지 않았다), 승인 대기(`waiting`) 상태의 세션이 든 확인 창과 보드로 옮기기 확인, 응답이 흐르는 동안의 새로 고침, 화면 나누기 탭의 저장, 끝난 채로 종료한 탭(`.paused` 가 없어야 한다), 설치 프로그램을 실제로 실행한 배치.
- 맨 위 고정이 멎은 뒤 스스로 다시 정해지는 경우(멎은 뒤 변하지 않음만 확인), 투명도가 한 번 0.5 로 남았다가 재현되지 않은 경우(마우스 위치는 이벤트로만 읽으므로 창이 가만한 마우스 밑에서 움직이면 한 번 낡은 값이 남을 수 있다. 가능한 경쟁으로만 본다).
- 창 가장자리를 화면 밖으로 끌면 보드가 작업 영역보다 커질 수 있다(작업 영역 1280x696 에서 1281x714·1283x696 을 봤다). 다음 접고 펼치기에서 맞춰진다. 겉모양 문제로 본다.
- `launch-board.ps1` 의 시험 모드 확인 순서(§5.13), Jev 판정(키 없음).
검토(`review-p1-opus.md`)가 남긴 `unchecked:`: 비동기 Tauri 명령이 막힌 동안의 스레드(`wait_exit`·확인 대기), WebView2 의 `screen.availWidth` 가 모니터가 여럿일 때 맞는지, 로그오프 때 셸이 먼저 죽는 경우, 시험 빌드와 설치된 보드가 단일 실행 식별자를 나누는지.

#### 11.2.2 2단계(설정 화면·단축키 바꾸기·새 세션 기본값·로그인할 때 열기·설정 안 계정 추가) GUI 확인

Windows 11 10.0.26300, 화면 1280x720 배율 100% 한 대, 격리 홈과 가짜 claude(§5.13)로 실행했다. 근거는 작업 기록의 `evidence-g11.md`(G11)와 코드 검토 `review-p2-opus.md`(둘 다 저장소 밖)다.
exe 는 커밋 `f58df68` 에서 `npx tauri build --bundles nsis` 로 빌드했고(hook 실행 파일이 `crates` 소스보다 오래돼 `cargo build --release -p claude-board-hook` 로 먼저 다시 빌드해 사이드카로 복사했다), 빌드 뒤 `windows/app/src`·`src-tauri/src`·`crates`·`ci` 에 더 새로운 파일이 없음을 `find … -newer` 로 확인했다. NSIS 설치 프로그램은 만들기만 하고 실행하지 않았다.
시험용 claude 는 보드가 `auth …` 를 가짜 로그인(`ci/fake-claude-login.cjs`)에, 그 밖을 키를 받아 적는 가짜(`ci/fake-claude-tty.cjs`)에 나눠 주는 가짜 중계기로 정했다. 진짜 claude 는 한 번도 뜨지 않았다(철선 `TRIPWIRE|OK|before`·`TRIPWIRE|OK|after`, 보드 아래 claude 프로세스는 모두 가짜). 진짜 `HKCU\…\Run` 의 `ClaudeBoard` 값은 시험 전과 후 모두 없었고(다른 Run 값은 그대로, 로그인할 때 열기를 켜고 끌 때마다 다시 확인), 사용자 `settings.json`·`profile.ps1` 의 md5 도 같았다. 실행 뒤 `git status --short` 가 비어 있다.
Jev 칸은 모든 그림이 `UNVERIFIED|no Jev key` 이고 통과로 세지 않았다. 아래 판정은 결정적 증거(DOM 읽기·설정 파일·키 기록·창 스타일·프로세스 목록)만으로 한 것이다.

| 행 | 확인한 것 | 판정 |
|---|---|---|
| 1 | 진짜 Ctrl+, 로 `[data-testid=settings-view]` 가 터미널 영역(0,32 에서 818x664, 보드 칸은 오른쪽 454px)에 뜨고, 한 번 더 누르면 사라지며 포커스가 `xterm-helper-textarea` 로 돌아옴. 돌아가기 단추로도 닫힘. 가짜 claude 프로세스는 열기 전·중·후 모두 살아 있고 키 기록은 2줄 그대로 | 통과 |
| 2 | 접힌 보드(진짜 Ctrl+Alt+T 로 접음, `terminalExpanded=false`, 창 826..1280): Ctrl+, 로 칸이 펼쳐지고(창 0,0 에서 1280x696) 열려 있는 동안 파일에 `terminalExpanded=true`, 한 번 더 누르면 `false` 로 돌아가고 창이 454px 로 돌아옴. 접힌 채 열고 트레이 메뉴 **종료**: 확인 창 "작업 중인 세션 1개가 꺼집니다"에서 진짜 클릭으로 **종료**, 보드가 꺼지기 전에 파일이 `terminalExpanded=false` 로 돌아가 있음. 트레이 메뉴 항목은 UI Automation 으로 읽히지 않아 위쪽 방향키와 Enter 로 마지막 항목을 골랐다 | 통과 |
| 3 | Esc 차례: 검색 "투명" 뒤 Esc 첫 번째는 검색어만 지우고(화면 그대로), 두 번째는 닫음. 설정 > 터미널 칸에 진짜 키로 `claude --mod` 를 치고 Esc 첫 번째는 칸을 `""` 으로 되돌리고(화면 그대로, 포커스 BODY), 두 번째는 닫음. 그동안 파일의 `newSessionCommand` 는 `""` | 통과 |
| 4 | 검색: "투명" 은 창 묶음 1건, "단축키" 21건이 모두 단축키 묶음, "계정" 은 저장한 계정과 자동 전환, "고정" 은 일반·단축키 두 묶음. 결과를 진짜로 눌러 묶음 제목으로 이동하고 투명도·항상 위에 고정·한도가 차면 다른 계정으로 자동 전환 카드에서 윤곽 `animate`(1200ms)가 호출됨. "저장한 계정" 은 계정 묶음으로만 가고 윤곽은 없음(카드 제목이 이메일이라 설계대로), 페이지 오류 0 | 통과 |
| 5 | 전환: 설정 파일에 `slide`·`fade`·`none` 이 각각 써짐. 진짜 Ctrl+, 에서 프레임마다 읽어 `slide` 는 340ms, `translateX` -818 에서 0 과 투명도(첫 프레임 투명도 0, 중간 프레임 29개), `fade` 는 약 200ms 투명도만(중간 프레임 17개), `none` 은 첫 프레임에 투명도 1·변형 없음·중간 프레임 0개로 곧바로 나타남 | 통과 |
| 6 | 묶음 컨트롤: 항상 위에 고정을 끄면 접힌 창이 맨 위가 아니고 켜면 맨 위. 투명도 슬라이더를 진짜로 끌면 문서 불투명도가 따라가고(0.97, 0.75 …) 파일에 `opacity=0.49`(작업 영역을 덮은 창은 1.2초 뒤 1 로 돌아감). 캐릭터가 파일에 저장됨. 보드 위치 왼쪽은 `boardSide=left` 와 접힌 창 x=0(오른쪽으로 되돌림). 알림 그림에서 왼쪽 아래를 눌러 `notify-position` 이 `bottom-left`. 자동 업데이트 확인이 파일에서 true→false→true. 로그인할 때 열기: 끄면 격리 notifier 폴더에 `login-launch-off` 가 생기고 켜면 사라짐, 진짜 Run 값은 처음·끈 뒤·켠 뒤 모두 없음 | 통과 |
| 7 | 설정 > 터미널: 칸의 안내 글자가 `claude --dangerously-skip-permissions`. `claude --model opus` 와 Enter 는 저장됨. `rm -rf x` 는 `.bad` 와 사유 "claude 로 시작해야 합니다", 파일 그대로, 칸을 벗어나도 저장 안 됨. 탭 문자와 NUL(DOM 으로 넣음)은 "줄바꿈이나 제어 문자는 쓸 수 없습니다", 저장 안 됨. 답변 꾸미기 체크는 기본 켬이고 진짜 클릭으로 `newSessionMdRender=false`. 이어서 새 세션 창은 명령 `claude --model opus`·체크 꺼짐으로 열리고, 경로 카드 **+** 는 `--plugin-dir` 0개, 체크를 다시 켜면 새 세션 창도 켜지고 **+** 는 `--plugin-dir` 1개 | 통과. 줄바꿈 분기는 GUI 로 닿지 않는다(입력 칸이 줄바꿈을 스스로 버린다) |
| 8 | 단축키: 20줄과 키 목록. 새 세션을 진짜 키로 Ctrl+Alt+N 으로 바꾸면 파일에 `shortcuts={"newSession":["KeyN",3]}` 이고 탭 줄 **+** 의 안내가 Ctrl+T 에서 Ctrl+Alt+N 으로 바뀜. 옛 Ctrl+T 는 창이 안 뜨고 가짜 claude 가 `0x14` 를 받음, 새 키는 창을 엶. 이름 바꾸기를 Ctrl+Alt+N 으로 정하면 두 줄 모두 충돌 표시, **그쪽 비우기** 가 새 세션을 `[]`(없음, 안내 사라짐)로. Ctrl+C 는 "Ctrl+C: Windows 기본 키라 쓸 수 없습니다" 이고 계속 받는 중. Esc 는 취소(화면 그대로), Backspace 는 `[]`. 줄의 ↺ 로 두 줄이 기본으로 돌아와 `shortcuts={}`, **모두 기본값으로** 는 바꾼 2건을 지우고 그 뒤 비활성. 키를 받는 동안 진짜 Ctrl+Alt+T 는 터미널을 접지 않고 키로 받아짐(전역 단축키가 풀림). 받기를 끝낸 뒤 보드를 X 로 숨기고 다른 앱이 앞인 상태에서 진짜 Ctrl+Alt+T 가 보드를 보이고 펼쳤고 Ctrl+Alt+B 가 다시 보임 | 통과. 제목 막대·메뉴 안내 글자는 탭 줄 **+** 와 Ctrl+Alt+T 토글 글자만 읽었다 |
| 9 | 계정 추가(가짜): **계정 추가…** 로 로그인 패널이 뜨고 `data-url` 이 가짜 주소, **다른 브라우저에서 열기**·**링크 복사** 단추와 코드 칸. 슬롯 폴더 `notifier\accounts\login-xxxx` 에 `open-browser.cmd`, 프로세스는 가짜 중계기의 `auth login --claudeai`. 틀린 코드는 "계정을 추가하지 못했습니다: Login failed: Invalid authorization code" 와 슬롯 삭제, 맞는 코드는 "fake-login@example.test 계정을 추가했습니다"와 `accounts\vault\<id>.bin` 에만 저장(열어 보지 않음)되고 줄에 **전환**·**지우기**. 취소는 슬롯 폴더 삭제·자격 증명 파일 0개·로그인 프로세스 종료. 칩 메뉴 **계정 추가…** 는 설정을 계정 묶음으로 열고 로그인을 시작(취소해 마침). **지우기** 는 네이티브 모달(`#32770`)로 "fake-login@example.test 계정을 지울까요?" 와 **지우기**·**취소**(DOM 대화상자가 아님), 진짜 클릭으로 **취소** 하면 계정과 vault 파일이 남음. 브라우저 프로세스 수와 가장 새 시작 시각이 전후 같고(16:19:35) 로그인 창은 뜨지 않음(시험 모드는 창을 열지 않는다), 철선 OK. **링크** 단추는 한 번도 누르지 않았다 | 통과 |
| 10 | 회귀: 폴더 8개의 새 세션 창(답변 꾸미기 켬, 명령 `claude --model opus`)이 만든 탭마다 `--plugin-dir` 정확히 하나. 경로 줄 11칸(`scrollWidth` 2507, `clientWidth` 818, 고른 칸 08 이 보임)에서 진짜 Shift+휠로 `scrollLeft` 1689 에서 1289 로 가고 9초 동안 1289 에 머묾(G10 에서는 150~1000ms 뒤 되돌아왔다. 수정 `77bacc3` 이 유지됨). 맨 위 고정: 접고 고정이면 true, 작업 영역을 덮으면 false, 고정을 끄면 false | 통과(5b 의 GUI 재확인이 이 행에서 이뤄졌다) |

관찰(판정에 넣지 않았고 고치지 않았다):
- 설정 > 터미널의 기본 명령 칸에서 틀린 글자(예: `rm -rf x`)는 칸을 벗어난 뒤에도 사유와 함께 칸에 남는다. 저장되지 않으며 저장된 값은 Esc 나 다시 포커스할 때까지 돌아오지 않는다.
- 설정 > 단축키에서 거절된 키의 붉은 안내는 Esc 로 받기를 취소해도 남는다(받기를 시작하면 지워지고 끝낼 때는 지우지 않는다).
- 키를 받는 중에 웹뷰가 새로 고쳐지면 전역 단축키(Ctrl+Alt+B·T)가 풀린 채 남는다. 백엔드의 풀어 둠 표시가 새로 고침을 넘겨 살아 있고, 설정 > 단축키를 다시 열 때 `sync_hotkeys(false)` 가 다시 건다(검토 L2, 코드 읽기로만 확인하고 재현하지 않았다).
- 한 번도 들어가 본 적 없는 복원된 탭은 웹뷰 새로 고침 뒤 다음 보드 시작까지 화면에서 사라진다(저장 목록에는 남는다, §5.12.9).
- 폴더를 바꿔 띄운 `uninstall` 도 실행 중인 `ClaudeBoard.exe` 를 모두 끝낸다(`setup.rs` 가 프로세스 이름으로 고르며 격리 여부로 가르지 않는다). 설치된 보드가 같은 PC 에서 돌고 있으면 함께 꺼진다. 코드 읽기로만 확인했다.
- 새 세션을 "없음" 으로 비우면 탭 줄 **+** 의 키 글자가 아예 사라진다(`keyLabel` 이 빈 문자열, 설계대로).

확인하지 못한 것(`untested:`):
- 실제 Windows 10, 모니터가 둘인 PC(알림 패널 위치 포함), 원격 데스크톱의 WebGL, AltGr 배열.
- 실제 Claude Code 의 키 처리: 단축키를 바꾼 뒤 가로챈 키와 가로채지 않은 키가 실제 claude 화면에서 어떻게 닿는지.
- 진짜 `claude auth login` 의 ConPTY 출력 모양(`login_output.rs` 의 시험 입력은 mac 파서와 가짜 로그인에서 따왔다)과 Windows Claude Code 가 `BROWSER` 환경변수를 따르는지. 따르지 않으면 기본 브라우저가 열리고 주소는 출력에서만 얻는다.
- 로그인 창 자체(주소를 처음 받을 때 열리는 보드 소유 창, Google 로그인 포함). 시험 모드는 창을 열지 않아 한 번도 열려 보지 못했다.
- 진짜 `HKCU\…\Run` 값을 쓰고 지우는 것. GUI 시험은 격리 홈이라 표지 파일만 바꿨고 단위 시험은 가짜 Run 키를 썼다.
- 보드가 보이는데 포커스가 다른 창에 있는 상태의 전역 단축키(숨긴 보드만 확인했다. 다른 창에 포커스를 줄 방법이 없었다).
- 계정을 실제로 지우는 것(취소만 시험했다).
- 보드 위치가 왼쪽일 때의 설정 화면 전환 방향(`motion.ts` 단위 시험뿐이다).
- 단축키를 바꾼 뒤 제목 막대·메뉴·트레이의 안내 글자(탭 줄 **+** 와 터미널 토글 글자만 읽었다. 트레이 글자는 단위 시험과 코드 읽기뿐이다).
- 기본 명령 검사의 줄바꿈 분기를 GUI 로 닿게 하는 것(붙여넣기는 사용자 클립보드가 필요했다).
- 로그인이 도는 중에 보드를 끄면 로그인 프로세스가 끝나는지(검토 `unchecked:`).
- Jev 판정(키 없음). Rust 시험은 검토 셸에 링크 도구가 없어 못 돌렸고 통합 세션이 돌렸다(260 통과로 보고받았다. 명령 `cargo test --manifest-path windows/app/src-tauri/Cargo.toml -- --test-threads=1`, 이 문서를 쓴 세션에서는 다시 돌리지 않았다).

### 11.3 Windows 10 대용 확인 (Windows 11 에서 흉내 낸 것)

개발 PC 한 대(Windows 11 10.0.26300, 화면 1280x720 배율 100%, WebView2 155.0.4283.62)에서 격리 홈(`CLAUDE_BOARD_HOME`)과 격리 WebView2 폴더로 돌렸다. **실제 Windows 10 이 아니다.** 근거 파일은 작업 기록의 `evidence-win10-ps51.md`·`evidence-win10-screen.md`·`evidence-win10-conhost.md`·`evidence-w5b-ps51.md` 다(저장소 밖).

| 항목 | 흉내 낸 방법 | 결과 |
|---|---|---|
| Windows PowerShell 5.1 에서 설치·업데이트·제거 | `ci/install-checks.ps1` 시나리오 5 를 샌드박스 홈(USERPROFILE·LOCALAPPDATA·APPDATA·TEMP 를 샌드박스로)에서 실행. install.ps1·uninstall.ps1 은 5.1.26100.9711(`powershell.exe`), 도우미는 PowerShell 7.6.7 | 57 항목 실패 0(md-render 배치 전 레이아웃). md-render 배치 뒤는 §11.6. 실제 홈의 settings.json·.credentials.json·Run 키·notifier 는 전후 같음(MD5·`reg query` 비교) |
| pwsh 없는 PC | PATH 에서 pwsh 폴더를 빼고 `LOCALAPPDATA`·`ProgramFiles` 를 빈 폴더로 | 열린 탭 셸이 `powershell`(5.1) 로 열림. 실제 키보드·한글 IME 로 `echo 한글` → `한글`. `echo "한글테스트AB가나다\|"` 의 막대가 칸 18(넓은 글자 8칸). 창 가장자리를 끌어 크기를 바꾼 뒤 xterm 칸 수 = 셸이 아는 `WindowSize` = `mode con`(61x36, 86x30 등) 일치. 탭 × 로 닫으면 약 0.2초 안에 셸·conhost 가 사라지고 보드는 살아 있음 |
| 같은 확인에서 나온 결함 | 스트리밍 중 두 번 크기 바꾼 400줄의 내용 | 장벽(§5.12.8) 도입 전 빌드에서 2회 중 2회 줄이 깨졌다(한 번은 전송 쪽은 400줄 모두 도착했는데 xterm 버퍼에서 깨짐). 장벽 도입 뒤 측정은 §11.5 |
| GPU 없음 | WebView2 인자 `--disable-gpu` | WebGL 은 소프트웨어 D3D11(WARP) 위에서 계속 켜졌고 DOM 렌더러 경로는 쓰이지 않았다. 화면은 GPU 켠 때와 303548 픽셀 중 8픽셀 차이 |
| WebGL 없음 | `--disable-gpu --disable-webgl --disable-3d-apis` | 경고 1건 뒤 DOM 렌더러로 그림(`.xterm-rows` 36행, 캔버스 0). 한글 IME·막대 칸 18·크기 일치는 같다. **DOM 렌더러에서 앱 글자 간격(`letter-spacing`)이 상속돼 글자 pitch 7.87px 와 칸 8.2px 가 어긋나는 결함을 찾아 고쳤다**(§5.12.5) |
| 글꼴 대체 | `Cascadia Mono` 를 `local("Consolas")` 로, `Segoe UI Variable` 을 사용 불가로 주입 | 칸 8x16 → 7x17, 같은 영역 99x36 → 113x34, 셸 크기 일치. 한글+영문 막대(칸 18)·상자 선(칸 20) 위치 같음. 보드 쪽 글 27개 요소 잘림 0. 제목 `Claude Board` 폭 121.2 → 114.8px |
| Windows Terminal 없는 클래식 콘솔 | 레지스트리 `HKCU\Console\%%Startup` 의 `DelegationConsole`·`DelegationTerminal` 을 Windows 콘솔 호스트로 바꿔 둠(끝나고 원래 값으로 복원, 전후 `reg export` 같음) | 세션 줄 한 번 클릭·더블 클릭이 그 `ConsoleWindowClass` 창을 앞으로 가져옴(최소화된 창은 복원). 오류 로그 없음 |

관찰한 한계: 클래식 콘솔이 보드의 클릭 지점을 덮고 있으면 한 번 클릭이 곧바로 콘솔을 앞으로 가져와, 더블 클릭의 둘째 클릭이 콘솔에 닿고 QuickEdit 선택 모드로 들어가 출력이 멈춘다(Esc 로 풀림). 1회 관찰.

**실제 Windows 10 에서 확인하지 않은 것(`untested:`)**: Windows 10 의 Segoe UI·Cascadia Mono 없음 상태의 글꼴, 1809 와 22H2 의 ConPTY·conhost, Windows 10 에 들어 있는 Windows PowerShell 5.1, GPU 가 없는 VM·낡은 드라이버(여기서는 D3D11 소프트웨어 대체뿐), WebView2 런타임 버전 차이, 배율 125~175% 와 모니터 여럿, 스토어 `pwsh` 별칭이 처음 실행될 때, 조합 중인 IME 가 있는 채 크기 바꾸기, install.ps1 의 대화형 분기(`Read-Host`), 샌드박스 안의 setup.exe(시나리오 1~4).

1단계 기능도 실제 Windows 10 에서 확인하지 않았다(`untested:`): 트레이 **종료**의 확인 창과 `.paused` 표지, 보드를 다시 켰을 때의 탭 복원과 5초 대기, md-render 옮겨 쓰기와 setup.exe 의 `plugins\md-render\` 배치, 보드로 옮기기, 알림 패널 클릭의 `--action focus`, 맨 위 고정·투명도 규칙(§11.2.1 은 Windows 11 한 대의 결과다).

다른 PC(실제 Windows 10, 가능하면 일회용 VM)에서 확인하는 순서:
1. 빌드 번호 `reg query "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion" /v CurrentBuildNumber` 가 17763 이상인지 본다(미만이면 두 설치 경로가 시작 전에 멈추는 것이 맞다, §8.1).
2. 릴리스 아티팩트를 한 폴더에 받아 `pwsh windows/ci/install-checks.ps1 -Assets <폴더> -Results <폴더>` 를 돌린다. 실제 `%USERPROFILE%`·HKCU 를 쓰므로 일회용 VM 에서만 돌린다. 모든 항목이 통과여야 한다(실패 시 스크립트가 끝에 throw).
3. 설치한 보드를 띄워 pwsh 가 없는 상태에서 새 세션을 열어 탭 셸이 `powershell` 인지, `echo "한글테스트AB가나다|"` 의 막대가 칸 18 인지 본다.
4. 탭에서 `1..300000 | % { "line $_" }` 가 도는 동안 창 폭을 줄였다 늘려, 끝난 뒤 `line N` 번호가 이어지는지(빠진 번호가 0 인지) 본다.
5. Windows Terminal 이 없는 상태에서 다른 콘솔 창의 claude 세션을 더블 클릭해 그 창이 앞으로 오는지, 종료된 세션을 더블 클릭하면 보드 탭에서 `claude --resume` 으로 이어지는지 본다.
6. 위 `untested:` 의 나머지(GPU 없는 VM, 배율, 모니터 여럿)를 같은 PC 에서 가능한 만큼 채우고 이 절에 근거와 날짜를 더한다.
7. 1단계 기능: 트레이 **종료**로 확인 창이 뜨고 취소하면 보드가 남는지, 끝낸 뒤 다시 켜면 탭이 돌아오는지, `%USERPROFILE%\.claude\notifier\plugins\md-render\` 에 다섯 파일이 생기는지, 다른 콘솔 세션을 **보드로 옮기기** 하면 보드 탭에서 이어지는지를 본다.

### 11.4 세션 짝짓기·클릭 흐름 확인

가짜 claude(`node.exe` 를 `claude.exe` 로 복사한 것이 hook 을 직접 실행)로 격리 홈에서 보드를 띄워 확인했다(빌드 1e36ccf 의 exe, `evidence-w2-g4.md`). **실제 Claude Code 는 돌리지 않았다.**

| 단계 | 스냅샷 필드 → 칩과 힌트 | 결과 |
|---|---|---|
| 1 탭(pwsh)에서 가짜 claude | `tty` t1, `mode` null → `CB`, "보드 탭에서 도는 세션: 누르면 그 탭으로" | 통과 |
| 2a 클래식 콘솔 | `tty` null, `appName` `터미널` → `터미널` | 통과 |
| 2b Windows Terminal | `appName` `Windows Terminal` → `Windows Terminal` | 통과 |
| 3 탭 안 `-p` | `tty` t2, `mode` `-p` → `CB -p`, "보드 탭 안에서 시작된 claude -p (탭이 닫히면 함께 끝납니다)" | 통과 |
| 4a `--bg` 분리 | `mode` `--bg`, headless → `$ claude --bg`, 백그라운드 구역, 짝 없음 | 통과 |
| 4c 모드 없는 분리 프로세스, 부모 살아 있음 | `tabAncestor` t3, `mode` null → `CB`, 백그라운드 구역 | 관찰(§5.12.7 알려진 한계). 부모가 끝나면 `$ claude -p` 로 바뀜 |
| 5 탭의 가짜 claude 가 SessionEnd 없이 죽음 | `ended`(파일은 `working` 그대로) → "보드에서 이어 열기" | 통과 |
| 6b `/clear` 흉내(옛 id SessionEnd 뒤 새 id) | 새 id 는 같은 탭 `CB`, 옛 id 는 종료된 구역의 "보드에서 이어 열기" | 통과 |

보드로 옮기기와 알림 패널 클릭으로 그 세션으로 가는 흐름은 §11.2.1 의 G10 이 소유한다. 거기서 클래식 콘솔의 가짜 세션만 썼으므로 실제 Windows Terminal 의 세션, 승인 대기 상태 세션의 확인 창, 실제 Claude Code 의 `/exit` 응답은 `untested:` 다.

W3 설계가 요구한 11행 GUI 확인(G7: 탭에서 `claude` → 일시중지 시 `keys.log` 에 외톨이 `1b` → 재개 문구와 `0d` → 중단 `1b`·`/exit`·`0d` → 종료 세션 클릭 시 같은 탭 재사용과 `--resume` 인자 → 더블 클릭 시 이어 열기 한 번 → 숨은 `-p` 클릭 시 알림 6초 → Ctrl+Shift+R 의 `/rename` → `--action focus`·`--action reopen` → Windows Terminal 회귀):

G7 결과(Windows 11 빌드 26300, 격리한 보드와 키를 받아 적는 가짜 claude, 증거 evidence-w3-g7.md): 통과. 일시중지는 외톨이 `1b` 가 가짜에 닿고, 재개는 문구 뒤 195ms 에 `0d`, 중단은 `1b`·`/exit`(400ms 뒤)·`0d`(200ms 뒤)이며 가짜 pid 만 죽고 탭 셸은 남는다. 끝난 세션 클릭은 빈 탭을 재사용하고 없으면 새 탭을 열며, 더블클릭은 탭과 명령을 한 번만 만들고 `--model` 은 유지되며 잘못된 id 는 알림만 띄운다. 숨은 세션 행은 메뉴가 없고 6초 알림이 뜬다. 콘솔 행은 더블클릭 지연 뒤 창을 앞으로 가져오고 QuickEdit 선택이 생기지 않으며 Windows Terminal 도 앞으로 온다. `/rename` 글과 Enter 가 들어가고 CLI `focus`·`pause`·`resume`·`stop`·`reopen` 이 동작한다. 시작 직후 보낸 `--action reopen` 두 건이 모두 열렸다. 스냅샷 150회를 150초 동안 최대 20ms 로 받아 멈춤이 없었다. `untested:` 실제 Claude Code 가 글과 Enter 를 받는 방식(rename 글과 Enter 사이는 시험 당시 5ms 였고 이제 200ms 다), 탭 pid 에 AttachConsole 을 안 부른다는 직접 증명(로그 없음, 탭 pid 를 25회 넘게 구동해도 보드가 죽지 않았다는 간접 근거뿐). 설계 §2 표대로 중단은 `.paused` 를 지워 일시중지 뒤 중단한 세션은 문구 없이 열리고, CLI `--action reopen` 은 항상 이어서 진행 문구를 붙인다.

코드 검토(Opus, `review-w3-opus.md`)가 짚은 치명 1건(스냅샷이 탐침 잠금을 두 번 잡아 보드가 멈춤)·중간 3건(이어 열기가 원래 인자를 모두 버림, 중단이 재사용된 pid 를 끝낼 수 있음, 이어 열기 중복 방지가 3초 뒤 풀림)과 낮은 위험 8건은 같은 브랜치에서 고쳤다(고친 뒤 다시 검토하지는 않았다). 고친 뒤의 동작이 §5.7·§5.12.7 이다. 고치지 않고 남긴 것: 일시중지 문구에 외톨이 Esc 한 글자가 실제 Claude Code 에 닿는지, `/exit` 가 "Background work is running" 에 막히는지, 실제 Windows Claude Code 의 `/rename`, Claude Code 가 한 번에 오는 여러 글자 입력을 어떻게 읽는지, cmd.exe 탭에서 `resumeCommand` 줄(순수 시험과 pwsh GUI 만 거침)은 `untested:` 다. `atPrompt` 는 최대 3초 묵은 값이다.

### 11.5 크기 바꾸기 장벽 측정

장벽은 측정으로 정했다. 모든 값은 빌드 26300 의 conhost 한 대에서, 합성 부하(ascii·ko·ko-odd·tui·ink)로 쟀다. 셀은 "깨진 캡처 수/5(깨진 줄 합계)" 다.

**① 캡처 게이트**(`evidence-d1-gate.md`, `evidence-d1-g0.md`): 크기 바꾸는 순간 ConPTY 출력을 원시 바이트로 남기는 캡처(`pty/capture.rs`, `#[ignore]` 시험)를 모양마다 5개씩 떠 xterm 6.0.0 에 재생해 cut 위치별로 깨짐을 셌다. cut D 는 장벽 이전 방식(ConPTY 호출 100ms 전에 xterm 크기를 바꿈)이다.

| 모양 | 고른 방법 | 고른 방법의 깨짐 | 이전 방식 D 의 깨짐 |
|---|---|---|---|
| 120x30 → 80x30(열 줄임) | A | 모든 부하 0/5 | ascii 4/5(4) |
| 80x30 → 120x30(열 늘림) | B | ascii 1/5(1), 나머지 0/5 | ascii 5/5(85) |
| 120x30 → 120x40(행 늘림) | A | 0/5 | ascii 5/5(50) |
| 120x40 → 120x30(행 줄임) | A | ascii 3/5(30), 나머지 0/5 | ink 5/5(135) |
| 120x30 → 60x40(열 줄고 행 늘림) | 두 단계 A, A | 모든 부하 0/5 | ascii 4/5(4) |
| 120x40 → 60x30(열 줄고 행 줄임) | A | ascii 5/5(50) | ascii 5/5(5), ink 5/5(135) |
| 60x30 → 120x40(열 늘고 행 늘림) | 행 A 뒤 열 B | ascii·ko·tui·ink 0/5 | ascii 5/5(1576) |
| 80x40 → 120x30(열 늘고 행 줄임) | 행 A 뒤 열 B | ascii 5/5(25), ko 4/5(20) | ascii 5/5(3289) |

발견 둘: ConPTY 는 크기 알림(`ESC[8;r;ct`)을 항상 내지 않는다(행만 바꾸는 둘째 단계 20/20 캡처에서 알림 없음). 대체 화면에서는 `ESC[?25h` 가 오지 않아 cut B 는 70/70 캡처에서 폴백(다음 LF)으로 갔다.

**② 실제 모듈 재생**(`evidence-d1-w6.md`, `evidence-d1-w6b.md`): 장벽·스캐너·규칙표의 실제 TypeScript 를 캡처 위에 재생했다(185 캡처 × 시드 5 = 925회, 프레임을 1 B~64 KiB 로 무작위 재분할). 장벽이 낸 깨진 수는 모든 캡처에서 게이트 수 이하였고(최선 노력 칸은 게이트 수와 같다), 쓴 바이트는 925/925 순서대로 한 번씩 xterm 에 닿았고 최종 크기는 925/925 요청과 같았다. 장벽이 낸 1150개의 cut 위치는 게이트의 찾기와 모두 같았고 CR 과 LF 사이를 자른 것은 0이었다. 이 재생이 드러낸 결함 둘(xterm 이 쓰기를 해석하기 전에 대체 화면 여부를 읽음 8/20회 오판, CR 로 끝난 스트림에서 크기가 안 적용됨 3/18회)은 같은 브랜치에서 고쳤고 다시 재생해 각각 0/20, 0/18 이었다.

**③ 보드 안 끝에서 끝까지**(`evidence-d1-g2.md`, `evidence-d1-g2b.md`, 빌드 1e36ccf, 3~5회): 실제 보드 탭에서 `line N` + 96개 `x` 를 30만 줄 흘리는 동안 `SetWindowPos`·실제 마우스로 창 크기를 바꾸고, 끝난 뒤 번호가 이어지는 논리 줄을 셌다(줄 번호 중복 0, 끝난 뒤 깨진 줄 0).

| 모양 | 깨진 줄 5회(G2b) | 판정 |
|---|---|---|
| 창 폭 줄임 1100 → 800 | 0 0 0 0 0 | 예상(0)과 같음 |
| 창 폭 늘림 800 → 1100 | 1 1 0 1 1 | 예상 이하(한 줄에 빈칸) |
| 행 늘림 | 0 0 0 0 0 | 같음 |
| 행 줄임 | 0 0 0 0 0, 매번 번호 6개 건너뜀 | 최선 노력 범위 안 |
| 열 줄고 행 늘림 | 0 0 0 0 0 | 같음 |
| 열 늘고 행 늘림(800x440 → 1280x696) | 1 1 1 1 1 | 늘림 규칙(1 이하) 안 |
| 최대화 뒤 복원 | 최대화 1줄 5/5, 복원은 번호 6개 건너뜀 | 안 |
| 모서리 끌기 14회 혼합 크기 변경 | 5회 모두 2줄 | 한 규칙으로 평가하지 않음 |
| 경계(divider) 끌기로 열 97 → 86 | 1 1 1 1 1(한 줄이 97자에서 잘림) | **예상(0)과 다름, 원인 미확인** |

**한계(정직한 잔여)**: 실제 Claude Code 화면은 돌리지 않았다(합성 평문·tui·ink 흉내만). 셀마다 캡처 5개, 보드 안 측정은 3~5회라 0 은 없음의 증명이 아니다. 빌드 26300 한 대뿐이다. 장벽이 폴백 로그에 남기는 줄 수는 있지만 "ConPTY 호출에서 cut 까지의 지연"은 기록하지 않아 재지 못했다. 보드를 새로고침하고 곧바로 크기를 바꾸면(쉬는 셸) 머리말을 1000ms 안에 못 찾아 줄 끝 폴백이 1~3번 났다(최종 폭은 셸·xterm 모두 6/6 맞음, 시각적 깨짐은 재지 않았다). 마우스를 계속 움직이는 끌기는 200ms 디바운스 때문에 끝날 때 한 번만 크기가 바뀐다.

### 11.6 배포 확인 (md-render 패키징)

`evidence-w5b-ps51.md`: 릴리스 zip 을 워크플로의 자산 묶기 단계 그대로 만들어(10개 항목: 최상위 5개 + `plugins/md-render/` 5개), 시나리오 5 를 샌드박스 홈에서 Windows PowerShell 5.1 로 돌렸다.
- 설치 폴더 교체 방식을 고치기 전 install.ps1: 75 항목 중 74 통과·실패 1. 실패는 열린 파일 하나가 폴더 이름 바꾸기를 막는 상황에서 `Move-Item` 이 폴더를 반쯤 옮기고 던져, "원래대로 되돌렸습니다" 라고 말하면서 `plugins\md-render` 가 반쪽으로 남은 것.
- 폴더를 `[IO.Directory]::Move` 로 한 번에 옮기는 수정(커밋 bbe644c)을 얹은 사본: 75 항목 실패 0.
- 확인한 것: zip 에 md-render 다섯 파일이 빠졌을 때 아무것도 건드리지 않고 거부, 파일 하나가 열려 있어 바꿔 넣지 못할 때 최상위 파일과 `plugins\md-render` 가 처음 상태로 돌아가고 `.old-*` 가 없음, uninstall.ps1 이 앱 폴더와 notifier 의 md-render 사본을 지움. 실제 홈·Run 키·notifier 는 전후 같음.
- **확인하지 못한 것(`untested:`)**: setup.exe 가 같은 `plugins\md-render\` 배치로 설치하는지(시나리오 1~4 는 샌드박스에서 돌리지 않았다. CI smoke 작업이 확인하며 그 결과는 이 브랜치를 push 해 실행한 뒤에야 나온다), 앱이 시작할 때 md-render 를 옮겨 쓰는 동작(§5.12.11 로 구현했고 G10 이 가짜 claude 로 확인했다, §11.2.1. 이 절의 샌드박스 설치 확인에는 들어 있지 않다), 실제 릴리스 자산과 네트워크 경로(웹 호출은 가로챈 가짜), `windows.yml` 이 실제 러너에서 같은 zip 을 만드는지, `claude plugin validate`·`claude plugin test` 단계(CI 에서만 돈다).

### 11.7 미구현

이 목록은 아직 없는 것만 싣는다. 구현한 1단계 항목의 내용은 §5.12.9~§5.12.13, 2단계 항목의 내용은 §5.14, 확인 결과와 남은 한계는 §11.2.1·§11.2.2 가 소유한다.

| 항목 | 상태 한 줄 |
|---|---|
| 화면 나누기 | `SplitNode` 와 `split.ts` 순수 함수만 있고 탭마다 잎 하나만 그린다 |
| 터미널 글꼴 크기·테마·애니메이션 설정 | 고정(글꼴 `Cascadia Mono, Consolas, D2Coding, monospace`) |
| 보드 글자 크기 배율 | 없음 |
| 실제 Windows 10 확인 | §11.3 의 순서가 남아 있다 |
| 모니터가 여럿일 때의 확인 | 프레임 시험은 모니터 둘을 가정한 단위 시험뿐이고 실기 확인이 없다 |
| AltGr 배열 확인 | AltGr 입력은 Ctrl+Alt 키를 가로채지 않게 코드로 막았으나 실제 배열에서 확인하지 않았다 |
| 탐색기가 보드에 트레이 아이콘 이벤트를 넘기는 동작 | 이 PC 에는 쓸 수 있는 트레이 아이콘이 없어 확인하지 못했다(§11.2.1) |
| 진짜 `claude auth login` 과 `BROWSER` | 출력 모양과 Windows Claude Code 가 `BROWSER` 를 따르는지 확인하지 못했다. 가짜 로그인으로만 끝까지 돌려 봤다(§5.14.6, §11.2.2) |
| 로그인 창 | 시험 모드는 창을 열지 않아 보드 소유 로그인 창(Google 로그인 포함)을 한 번도 열어 보지 못했다 |
| 진짜 Run 값 | 로그인할 때 열기가 진짜 `HKCU\…\Run` 값을 쓰고 지우는 것을 확인하지 못했다(격리 표지 파일과 가짜 Run 키만) |
| 전역 단축키 | 보드가 보이는데 포커스가 다른 창에 있는 상태에서는 확인하지 못했다(숨긴 보드만) |
| 계정 지우기 | 확인 대화상자의 취소만 시험했고 실제로 지우는 것은 시험하지 않았다 |
| 보드 왼쪽의 설정 화면 전환 | 단위 시험뿐이고 GUI 로 보지 않았다 |
| 단축키를 바꾼 뒤 안내 글자 | 탭 줄 **+** 와 터미널 토글 글자만 읽었다. 제목 막대·메뉴·트레이는 코드로만 확인했다 |
| 실제 Claude Code 의 키 처리 | 바꾼 단축키가 실제 claude 화면에서 어떻게 닿는지 확인하지 못했다 |

구현했지만 남은 한계(`untested:`/`unchecked:`):

| 항목 | 한계 |
|---|---|
| 웹뷰 새로 고침 | 들어간 적 없는 복원 탭이 다음 보드 시작까지 화면에서 사라진다(저장 목록에는 남는다) |
| 작업 폴더가 지워진 탭 | 화면에서 조용히 빠지고 저장 목록에서도 빠지며 그 세션의 `.paused` 가 남는다 |
| 투명도 | 마우스 위치를 이벤트로만 읽어, 창이 가만한 마우스 밑에서 움직이면 낡은 값이 남을 수 있다. 처음 한 번 0.5 로 남은 것을 봤고 재현하지 못했다 |
| 시험 모드 확인 순서 | `launch-board.ps1` 이 보드가 뜬 뒤에 확인한다(§5.13) |
| 키 받는 중 웹뷰 새로 고침 | 전역 단축키가 풀린 채 남고 설정 > 단축키를 다시 열면 걸린다(코드 읽기, 재현하지 않았다) |
| 폴더를 바꿔 띄운 `uninstall` | 격리 여부와 무관하게 실행 중인 `ClaudeBoard.exe` 를 모두 끝낸다(설치된 보드 포함, 코드 읽기) |
| 로그인 중 보드 끄기 | 도는 로그인 프로세스가 끝나는지 확인하지 못했다 |
| Jev | 키가 없어 모든 판정이 `UNVERIFIED` 이고 질문 가족이 보정 전이다 |
