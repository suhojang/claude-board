# 보드 내장 터미널 — Windows 판 계획과 점검

2026-10-17. 두 문서를 합쳤다. **1부(점검)가 2부(첫 계획)보다 나중이고, 둘이 다르면 1부를 따른다.** 사용자 결정은 [설계.md](설계.md) 가 기준이다.
파일 경로·줄 번호는 2026-10-17 기준 저장소 코드다. `check/`·`research/` 같은 조사 자료 경로는 당시 임시 폴더라 남아 있지 않을 수 있다.

---

# 1부. 사용자 결정 이행 점검


기준: 설계 메모 `scratchpad/embedded-terminal-design.md`(정해진 것 1~3, 와이어프레임 검토 후 결정 1~9와 하위 항목, 답변 마크다운 꾸미기, claude 를 띄우는 환경),
앞선 계획 `scratchpad/windows-plan/PLAN.md`, 저장소 `windows/`(커밋 `21358bf`). 저장소 파일은 고치지 않았다.
이 점검에서 만든 것: `scratchpad/windows-plan/check/`(내려받은 문서·소스 발췌, `api-check/` 컴파일 확인 프로젝트).

판정: **됨** = 지금 코드·문서 근거로 그대로 구현 가능 / **됨(조정 필요)** = 가능하지만 PLAN·코드에 바꿀 점이 있음 /
**위험** = 가능해 보이나 Windows 실기 전에는 동작을 장담할 수 없거나 정책과 어긋날 여지가 큼 / **막힘** = 지금 알려진 방법으로는 안 됨.
"확인" = 소스·문서로 직접 확인, "확인 안 됨" = 추론 또는 실기에서만 알 수 있음.

---

## 0. 한눈에

- **막힘은 없다.** 모든 결정이 Windows 에서 구현 가능하다. 다만 아래 다섯 가지는 실제 Windows PC 확인 전에는 장담할 수 없다.
- 위험 상위 5:
  1. **한글 조합 중 Enter·Esc·Shift+Enter** — macOS 에서 고친 "조합 중 키가 사라짐"이 xterm.js(WebView2)에서도 생기는지 알려진 이슈·문서가 없다. 한글 모드의 Esc·Ctrl+글자 누락(#6065)은 확인된 문제라 키 처리기로 직접 고쳐야 한다.
  2. **PLAN 의 Job 설정(KILL_ON_JOB_CLOSE + BREAKAWAY_OK)은 백그라운드 정책을 어긴다.** libuv(Node·Bun 의 프로세스 띄우기)는 분리 실행에 breakaway 를 요청하지 않으므로, 보드 탭 안에서 시작된 Claude Code supervisor(`claude --bg`·`←`·`/bg`)와 그 아래 모든 백그라운드 세션이 보드와 함께 죽는다. → Job 을 쓰지 않거나, 쓰면 `SILENT_BREAKAWAY_OK` 로 탭의 claude.exe 하나만 담는다.
  3. **"보드 탭 소속" 판정** — tty 가 없어서 pid 조상만으로 판정하면, 탭에서 시작된 supervisor 아래의 백그라운드 세션까지 탭 세션(`CB`)으로 잘못 잡히고 headless 도 false 로 기록된다. 콘솔 동일성(같은 ConPTY 콘솔에 붙어 있는가)으로 판정해야 한다.
  4. **실제 Claude Code 의 키·종료 동작** — ConPTY 에 넣은 단독 Esc, Shift+Enter 바이트, `/exit` 가 "Background work is running" 대화상자에 막히는 경우, CTRL_CLOSE_EVENT 를 받았을 때의 정리(5초 제한).
  5. **런처 구조와 pid** — `~/.local/bin/claude.exe` 가 복사본인지 다른 exe 를 띄우는 런처인지 문서에 없다(macOS·Linux 는 심볼릭 링크라고만 적혀 있음). 조상 비교로 두 경우 모두 짝지을 수 있지만 CI 에서 확인해 둔다.
- 새로 필요한 사용자 결정 6개(§4): Job 방식 확인, Windows 단축키, 새 환경을 어디서 읽을지, 탭 안 `claude -p` 표시, 보드 탭에서 Claude Code 의 "백그라운드로 보내기"를 허용할지, Shift+Enter 바이트.

---

## 1. 결정별 점검표

### 1. 화면

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| 창을 작업 영역 전체로 펼치기·접기 | 됨(조정 필요) | `window.rs:68 monitors()` 가 모니터별 작업 영역(물리 픽셀)·배율을 이미 준다. 펼침 = `set_frame(m.work)`(최대화가 아님 — 창이 `maximizable: false`, `tauri.conf.json`). 펼친 동안 항상 위(`board.rs:218-224 set_pinned`, 기본 켜짐)와 투명도(`+page.svelte:33-37`, 문서 전체 opacity)는 꺼야 한다 — PLAN §6.2 와 같음. 펼친 틀은 `save_frame_if_settled`(`board.rs:316-337`)가 "⤢ 로 돌아갈 자리"로 저장하지 않게 한다 |
| 터미널은 탭으로 하나씩 | 됨 | 프론트 구성 문제. 안 보이는 탭은 xterm `Terminal` 객체만 살려 두고 DOM 에 붙이지 않는 PLAN §3.4 1안 그대로. WebGL 맥락은 보이는 탭 하나만 |
| 경계를 끌어 두 영역 폭 조절(보드 폭 고정 아님) | 됨(조정 필요) | **PLAN §0·§6.1·§6.2 가 보드 폭을 454 고정으로 적었다 — 결정과 어긋남.** 보드 칸 폭을 설정(예: `boardPaneWidth`, 논리 px)으로 저장하고, 단계 계산(`+page.svelte:49-50 pickTier` 는 지금 `window.innerWidth`)을 보드 칸 폭(ResizeObserver)으로 바꾼다. 터미널 칸은 `addon-fit` → `ResizePseudoConsole`(100ms 늦춤). 끄는 동안 연속 크기 바꾸기는 Claude Code 다시 그리기 폭주가 되므로 놓을 때 한 번 보내는 쪽이 안전 |
| 미니 크기에서 눌러도 펼침 | 됨(조정 필요) | **PLAN §6.2 의 "`enter_mini`: 펼쳐 있으면 먼저 접는다"는 그대로 두되, 반대 방향(미니에서 보드 탭 세션을 누름 → 펼침)을 더해야 한다.** 미니(`window.rs:56-60 mini_frame`, 폭 300) 상태에서 activate 가 오면 `board_mini=false` → 펼친 틀 → 보드 칸은 저장 폭(≥360 이면 미니 아님). 펼친 상태에서 사람이 보드 칸을 360 미만으로 끌면 보드 칸만 미니 화면이 되는 것은 허용(단계 계산이 칸 폭 기준이므로 자연히 그렇게 된다) |
| 세션 칩: `CB` / 다른 터미널 앱 이름 / `$ claude -p` | 됨(조정 필요) | `CB` = 보드 탭 소속(§1-5·§1-6 의 판정). 앱 이름 = `console::terminal_window(pid)`(`console.rs:207-236`) → `win::window_pid` → `ProcessTable::name` 으로 얻는다(이미 `terminal.rs:238-241 focus()` 가 같은 길로 Windows Terminal 을 알아본다). **지금 3초 확인(`board.rs:273-301 probe_in_background`)은 `drivable` 만 싣는다 → 호스트 exe 이름을 `Probe` 에 더 실어야 한다**(AttachConsole 을 한 번 더 하지 않게 같은 붙기에서 함께). 이름 표: `WindowsTerminal.exe`→Windows Terminal, `Code.exe`→VS Code, `conhost.exe`/`OpenConsole.exe`·모름→"콘솔". **함정**: 조상 창을 따라가다 `ClaudeBoard.exe` 창에 닿는 세션(탭에서 시작된 supervisor 아래 세션)은 `CB` 가 아니라 백그라운드로 보여야 한다(§2-3) |

### 2. 전역 단축키 (macOS ⌃⌥T 대응)

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| 펼치기·접기 = Ctrl+Alt+T | 됨(조정 필요) | 지금 Ctrl+Alt+B 는 `tauri-plugin-global-shortcut`(Windows 는 `RegisterHotKey`)로 등록하고 실패는 `board.log` 에만 남긴다(`board.rs:682-705`, SPEC §5.10). 같은 `with_handler` 에 `Code::KeyT` 하나를 더하면 된다 |
| AltGr 충돌 | 위험(낮음) | Windows 는 Ctrl+Alt 를 AltGr 로 취급한다 — AltGr 로 글자를 치는 배열(독일어 `@`=AltGr+Q, 폴란드어 `Ż`, 유로 기호 등)에서는 등록한 Ctrl+Alt+<키> 의 글자를 못 친다(Raymond Chen, "Why Ctrl+Alt shouldn't be used as a shortcut modifier" 2004, 2012 후속 글). AltGr+T 에 글자가 있는 배열은 드물지만 **지금 Ctrl+Alt+B 도 같은 노출**(헝가리어 배열 AltGr+B = `{` 로 알고 있음 — 확인 안 됨). **한국어 배열은 오른쪽 Alt = 한/영, 오른쪽 Ctrl = 한자라 AltGr 가 없다 → 왼쪽 Ctrl+왼쪽 Alt 로만 눌린다**는 것을 설치 가이드에 적는다 |
| 다른 앱과의 충돌 | 위험(낮음) | Windows 시스템 단축키는 아니다. 앱 안 단축키로는 **JetBrains IDE 의 "Surround With" 가 Ctrl+Alt+T**(Windows·Linux 기본 배치로 알고 있음 — 확인 안 됨) — 전역 등록하면 IDE 에서 그 단축키를 빼앗는다. 다른 앱이 먼저 전역으로 잡았으면 등록 실패(로그만) — 지금 Ctrl+Alt+B 와 같은 처리. 사용자 결정 §4-2 |
| 보드 안 터미널에서의 영향 | 됨 | 전역 등록 키는 xterm.js 에도 오지 않는다 — Claude Code 가 쓰는 키가 아니므로 문제없음 |

### 3. 새 세션

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| 기본 명령 `claude --dangerously-skip-permissions` | 됨(조정 필요) | 실행 파일 찾기는 `terminal.rs:28-38 find_claude`(PATH, 없으면 `~/.local/bin/claude.exe`, `claude.cmd` 는 일부러 고르지 않음 — npm 설치는 cmd.exe 가 인자를 다시 해석하므로). **새 세션 창에서 명령을 고칠 수 있게 하면 셸 없이 인자로 나눠야 한다** — Windows 는 `CommandLineToArgvW` 규칙으로 나누고 첫 낱말은 `claude` 로만 받는다(다른 실행 파일·`-p`·`--bg`·`--resume`·사용자가 쓴 `--session-id` 는 거르거나 경고 — `--bg` 면 탭이 곧바로 끝나고, `-p` 면 대화형이 아님). 처음 한 번 Claude Code 의 bypass 경고·폴더 신뢰 질문이 탭에 뜬다(사용자가 탭에서 답함, macOS 와 같음) |
| `--session-id <uuid>` | 됨 | 문서: "must be a valid UUID"(`code.claude.com/docs/en/cli-reference` `--session-id`). UUID 는 `CoCreateGuid`(windows-sys `Win32_System_Com`) 또는 `uuid` crate. carry_args 가 `--session-id` 를 넘기지 않으므로(`state.rs:63-71` 목록에 없음) 이어 열기에서 `--session-id X --resume X` 가 겹치지 않는다 |
| 셸 없이 claude.exe 바로 실행 | 됨 | ConPTY 자식으로 `CreateProcessW(EXTENDED_STARTUPINFO_PRESENT, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE)` + 인자 배열을 `quote::windows` 로 잇기(`terminal.rs:61-69 console_command_line`, 공백·한글·따옴표 왕복은 이미 Windows 실기 시험 `terminal.rs:583-656`). **보드 쪽 파이프 끝은 상속 불가로 만든다** — 상속되면 보드가 죽어도 파이프가 안 끊겨 ConPTY 가 탭 프로세스를 닫지 못한다(§2-2) |
| 끝나면 [다시 열기][셸 열기] — 어떤 셸? | 됨 | `pwsh.exe`(PowerShell 7) → `powershell.exe` → `%ComSpec%`(cmd.exe) 순서로 찾아 cwd 에서 인자 `-NoLogo` 만 주고 ConPTY 로 띄운다(PLAN §6.1 과 같음, 위치 찾기는 `terminal.rs:280-305 locate` 재사용). claude 종료 감지는 자식 프로세스 핸들 `WaitForSingleObject` + `GetExitCodeProcess`(macOS 의 "0.25초 안 0 아닌 코드 = failed to launch" 와 같은 규칙 가능). 셸 탭 안에서 사람이 `claude` 를 띄우면 세션 id 를 미리 모르므로 짝짓기는 pid·콘솔로만 한다(§1-5). 셸 기본값을 고를지는 사용자 결정 §4-7(작음) |
| 보드 탭 세션 `--plugin-dir <앱에 실은 md-render>` | 됨(조정 필요) | §1-9 참고. 경로는 `current_exe()` 폴더 기준 `plugins\md-render`. 인자 배열이라 공백(`Claude Board`)·한글 사용자 이름이 문제없다. **설치 쪽 고칠 것이 있다**(§1-9) |
| **claude 를 띄우는 환경(PATH 낡음)** | 됨(조정 필요) | 문제는 실제로 있다: 보드는 로그인 때 Run 값으로 떠서(SPEC §1.2) 그때의 환경을 평생 쓴다. 나중에 node·Git 을 설치해 PATH 가 바뀌면 보드가 띄운 claude 와 사용자 hook(node)이 못 찾는다 — macOS 의 `node: command not found` 와 같은 증상. `find_claude` 는 `~/.local/bin` 을 따로 보므로 claude 자체는 찾는다. **고침**: claude·셸을 띄울 때마다 `CreateEnvironmentBlock(자기 토큰, bInherit=FALSE)`(userenv.dll — 레지스트리의 시스템·사용자 변수와 프로필 변수로 새로 만든 블록)을 쓴다. 이 Mac 에서 `x86_64-pc-windows-msvc` 로 컴파일 확인(`check/api-check`, windows-sys 0.61.2 `Win32_System_Environment`). 선례: Windows Terminal 1.18 부터 새 탭마다 환경을 새로 만든다(`compatibility.reloadEnvironmentVariables`, 기본 true). 지금 `spawn_console` 은 `std::env::vars_os()` 를 넘긴다(`terminal.rs:97-107`). **대가**: 보드 프로세스에만 있던 변수(개발 셸에서 띄운 보드의 `CLAUDE_CONFIG_DIR`, PowerShell 7 의 `PSModulePath` 등)는 빠진다 — `PSModulePath` 는 오히려 빠지는 게 맞고(SPEC §5.8 의 업데이트 버그와 같은 원인), 나머지는 사용자 결정 §4-3. PowerShell 프로필에서 PATH 를 바꾸는 도구(fnm 등)는 레지스트리에 없어 이 방법으로도 안 잡힌다 |

### 4. 이어 열기

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| 보드가 다시 뜰 때 resume_cmd 로 자동 이어 열기 | 됨(조정 필요) | Windows 는 표시용 `resume_cmd` 대신 `resume_args` 배열로 실행(SPEC §2.1, `store.rs:268-273` 없으면 `--resume <id>`). 쥔 세션 목록은 `board-settings.json`(PLAN §4.1, 파일 약속 아님). **조정 셋**: ① 목록에 탭 자식 pid·생성 시각을 함께 남긴다 — 다시 뜰 때 세션 파일의 pid 가 "옛 탭 자식"이면 CTRL_CLOSE 처리(최대 5초, §2-2)가 끝나기를 6~10초 기다렸다 연다. "다른 pid 가 살아 있음"이면 이어 열지 않고 목록에서 뺀다(탭에서 백그라운드로 옮겨 간 세션 — §2-4). PLAN §4.2-2 는 둘을 구분하지 않는다. ② `resume_args` 는 세션 기록에 권한 모드·모델 값이 있으면 `--permission-mode`·`--model` 을 붙인다(`state.rs:108-117`, 값이 없으면 건너뜀 `state.rs:109`) — 보드 탭 세션은 hook 입력으로 둘 다 기록되므로 사실상 늘 붙는다. Claude Code 문서상 그 대화가 **아직 도는 백그라운드 세션이면 이런 설정 플래그가 있을 때 attach 하지 않고 종료 코드 1 로 끝난다**(`code.claude.com/docs/en/sessions` "Resume a running background session") → ①의 "다른 pid 살아 있음 → 열지 않음"이 이것을 막는다. 그래도 빨리 끝나면 [다시 열기][셸 열기] 막대와 탭에 남은 Claude Code 안내문을 그대로 보여 준다. ③ 로그인 자동 실행(`--background`)에서도 창을 먼저 보인 뒤 연다(PLAN §4.2-1) |
| 끝난 세션 클릭 → 보드 새 탭 | 됨 | 지금 `resume()`(`terminal.rs:197-212`)이 `CREATE_NEW_CONSOLE` 로 여는 자리를 ConPTY 탭으로 바꾸면 된다. 새 콘솔로 여는 옛 동작은 결정 3(보드 새 탭)으로 정리됨 — PLAN §12-3 은 닫힌 질문 |
| core carry_args 가 `--plugin-dir`·`--dangerously-skip-permissions` 를 넘기는가 | **됨(확인)** | `state.rs:63` `CARRY_ONE` 에 `--plugin-dir`, `state.rs:65-71` `CARRY_FLAG` 에 `--dangerously-skip-permissions`·`--allow-dangerously-skip-permissions`. `=` 꼴도 받는다(`state.rs:81`). macOS `state.py` 와 같은 목록(SPEC §3.3). Claude Code 문서도 `--plugin-dir` 는 resume 때 복원되지 않으니 다시 넘기라고 한다(`sessions` "What a resumed session restores") → carry 가 필요하다는 근거. **고칠 것은 core 가 아니라 보드 쪽**: `resume_args` 안의 `--plugin-dir` 가 옛 설치 경로(setup.exe `%LOCALAPPDATA%\Claude Board\` ↔ install.ps1 `%LOCALAPPDATA%\Programs\Claude Board\`, SPEC §1.2)이거나 사용자가 꾸미기를 껐을 수 있다 → 보드는 "끝이 `plugins\md-render` 이거나 보드 설치 폴더 아래"인 `--plugin-dir` 만 빼고, 설정이 켜져 있으면 지금 경로로 한 번만 넣는다(사용자의 다른 `--plugin-dir` 는 그대로) |

### 5. 짝짓기 (tty 없음)

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| `--session-id` + pid 조상으로 세션 파일과 탭 맞추기 | 됨(조정 필요) | hook 은 부모부터 8단계 안 첫 `claude.exe`(또는 `claude` 가 든 node/bun)를 pid 로, 생성 시각을 `pid_ctime` 로 남긴다(`proc.rs:123-132`, SPEC §3.4). 보드는 `CreateProcessW` 에서 자식 pid·핸들을 받으므로 "세션 pid == 탭 자식" 또는 "탭 자식 ∈ `ProcessTable::ancestors(세션 pid)`"(생성 시각도 비교 — Windows 는 부모가 죽은 뒤 ppid 가 재사용될 수 있다)로 확인한다. **조정 셋**: ① 탭 안에서 `/clear`·`/resume` 하면 같은 프로세스가 **다른 세션 id** 로 hook 을 부른다 → 탭↔세션 id 는 고정하지 말고 pid 로 다시 맞춘다(그래야 [다시 열기]가 마지막 세션을 연다, macOS 도 같음). ② [셸 열기] 탭 안의 claude 는 id 를 미리 모르므로 pid 조상·콘솔로만. ③ "탭 소속"의 최종 판정은 조상이 아니라 **같은 콘솔에 붙어 있는가**로 한다(§2-3) |
| claude.exe 런처 구조 | 위험(낮음) | 네이티브 설치 스크립트(`claude.ai/install.ps1`)는 `claude.exe` 를 받아 `claude.exe install` 로 "launcher" 를 놓는다고만 하고 Windows 구조는 문서에 없다(`setup` 문서: macOS·Linux 는 `~/.local/bin/claude` 가 `versions/` 로의 심볼릭 링크). 두 경우 모두 짝짓기는 된다 — 복사본이면 세션 pid = 탭 자식, 런처가 `versions\<v>.exe` 를 띄우면 hook 이 이름 `claude.exe` 를 찾아 올라가므로 런처(= 탭 자식)가 pid 가 되거나, 버전 exe 이름도 `claude.exe` 면 탭 자식이 그 조상이 된다. CI 의 가짜 Claude Code(node.exe 복사)는 이를 드러내지 못한다 → **CI 에서 실제 네이티브 설치(인증 불필요) 후 두 파일 해시 비교 + ConPTY 안에서 `claude.exe` 를 띄워 자식 프로세스 표 확인**으로 정한다(§5) |

### 6. 백그라운드 정책 (가장 주의)

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| 백그라운드 세션은 모니터링만(펼치기·이동·보드로 옮기기·세션 중단·⋯ 메뉴 없음) | 됨 | 프론트(`SessionRow.svelte:30-52` 클릭·메뉴, `GroupCard.svelte:33-34` 구역 나눔은 `headless` 기준)와 `terminal.rs::perform` 분기만 바꾸면 된다. 지금도 3초 확인은 headless 세션에 `AttachConsole` 하지 않는다(`board.rs:281`, `!s.headless` 만) → 보드가 백그라운드 세션에 신호를 보내는 경로는 `stop()`(`terminal.rs:180-194`)·`pause()` 뿐이고 그 메뉴를 없애면 끝 |
| 끝난 백그라운드는 메뉴 '세션 다시 열기'로만 보드 탭에서 | 됨 | `carry_args` 는 `-p`/`--print` 를 넘기지 않는다(`state.rs:63-71` 목록에 없음, `state.rs:526-539` 시험이 `--print` 를 빼는 것을 확인) → 대화형으로 열린다. 열린 뒤 SessionStart hook 이 pid 를 탭 자식으로 다시 잡는다(`state.rs:218-233`). headless 는 보드 창이 보이면 false(조상 = `ClaudeBoard.exe` 의 보이는 창, `console.rs:224-234`), 숨어 있으면 true 로 기록될 수 있으므로 보드가 짝지은 세션은 false 로 덮는다(PLAN §5.3) |
| 보드 업데이트·종료가 백그라운드 세션에 절대 영향 없음 | **위험 → 조정하면 됨** | 보드 바깥 백그라운드 세션: install.ps1 은 `Get-Process -Name ClaudeBoard | Stop-Process -Force` 만 한다(`install.ps1:111-115`), uninstall 도 `ClaudeBoard.exe` 만 끝낸다(`setup.rs:219-220`) → 영향 없음. **문제는 보드 탭에서 시작된 것**: Claude Code supervisor 는 "처음 세션을 백그라운드로 보내거나 agent view 를 열 때" 자동으로 뜨고 그 아래에 모든 백그라운드 세션이 각자 프로세스로 돈다(`agent-view` "The supervisor process"). 보드 탭에서 `←`(빈 입력에서 한 번 — v2.1.196 부터 한 번 누름)·`/bg`·`claude --bg` 를 쓰면 supervisor 가 탭 claude.exe 의 자손으로 뜬다. PLAN §4.3 의 Job(`KILL_ON_JOB_CLOSE | BREAKAWAY_OK`)이면 supervisor 는 `CREATE_BREAKAWAY_FROM_JOB` 을 요청해야만 빠지는데 **libuv 는 분리 실행에 그 플래그를 일부러 쓰지 않는다**(libuv `src/win/process.c` 주석: "we're not setting the CREATE_BREAKAWAY_FROM_JOB flag … CREATE_BREAKAWAY_FROM_JOB makes the CreateProcess call fail if we're under job control that doesn't allow breakaway", `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` 만). → supervisor 와 **그 뒤 다른 터미널에서 띄운 백그라운드 세션까지** 보드가 죽을 때 함께 죽는다. **고침**: §2-2 |
| Job 을 쓸지·breakaway | 됨(조정 필요) | 선택지 둘. **(가) Job 없음(권장)** — 보드가 죽으면(강제 종료 포함) 보드가 쥔 ConPTY 파이프가 닫히고, OpenConsole/conhost 는 신호 파이프·입력 파이프가 끊기면 `VtIo::SendCloseEvent` → `CloseConsoleProcessState` 로 **붙어 있는 모든 클라이언트에 CTRL_CLOSE_EVENT** 를 보낸다(microsoft/terminal `src/host/PtySignalInputThread.cpp:322-326`, `VtInputThread.cpp:53-56`, `VtIo.cpp:300-321`). CTRL_CLOSE_EVENT 는 처리 함수가 돌아오거나 5초(`SPI_GETHUNGAPPTIMEOUT`)가 지나면 시스템이 프로세스를 끝낸다(`HandlerRoutine` 문서 "Timeouts"). `ClosePseudoConsole` 문서도 "will send CTRL_CLOSE_EVENT to each client application that is still connected". 콘솔이 없는(분리) supervisor 는 이 신호를 받지 않는다 — agent view 문서의 "close your shell … your dispatched work keeps going" 과도 맞는다. **(나) Job 을 쓰면 `KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK`** — "Allows any process associated with the job to create child processes that are not associated with the job"(`JOBOBJECT_BASIC_LIMIT_INFORMATION` 문서) → Job 에는 보드가 직접 넣은 탭 claude.exe 만 남고 supervisor·도구 셸·MCP·알림 패널은 저절로 빠진다. 얻는 것은 "탭 claude.exe 를 즉시 끝냄" 하나뿐이고, 잃는 것은 Claude Code 가 CTRL_CLOSE 에서 할 정리(SessionEnd hook, 백그라운드 작업 넘기기)다. 컴파일 확인: `check/api-check`(`JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK` 는 windows-sys 0.61.2 `JobObjects/mod.rs:429`). 결정 9 의 문구("Job 은 탭 자식만, 분리 실행은 빠져나가게")는 (나)로 정확히 이룰 수 있지만 (가)도 같은 결과 → 사용자 확인 §4-1 |
| ConPTY 를 닫으면 붙은 콘솔 클라이언트가 끝나는가 | **됨(확인)** / 강제 종료 경로는 위험(낮음) | 보드가 `ClosePseudoConsole` 을 부르는 경우는 Microsoft 문서로 확인. **보드가 강제 종료되어 파이프만 끊기는 경우의 근거는 microsoft/terminal `main` 소스(= sideload 할 OpenConsole 과 같은 코드)다 — 시스템 conhost(Windows 10 1809 ~ 11 각 판)가 같은 길을 타는지는 확인하지 않았다.** CI 시험 ⑧(§3 표)로 시스템 conhost 경로를 확인하고, 안 되면 sideload ConPTY(PLAN §12-6) 또는 Job (나)(§4-1)가 답이 된다. 단 "끝난다"는 "CTRL_CLOSE_EVENT → 최대 5초 뒤 강제 종료"다. 보드가 다시 떠서 이어 열 때 이 5초를 기다려야 한다(§1-4 ①). 탭 안에서 `CREATE_NO_WINDOW`(새 숨은 콘솔)나 분리 실행으로 뜬 자손은 이 콘솔의 클라이언트가 아니므로 이 신호를 받지 않는다 — Claude Code 가 정상 종료하며 자기 도구 프로세스를 정리하는지는 확인 안 됨 |
| 업데이트(`install.ps1 -Update` 가 보드를 끔)와 확인 창·이어 열기 맞물림 | 됨(조정 필요) | 지금 `updater::run`(`updater.rs:149-168`)은 확인 없이 스크립트를 띄우고, 트레이 "종료"는 메인 스레드에서 바로 `app.exit(0)`(`tray.rs:103`). `Board::confirm` 은 "UI 스레드에서 부르지 않는다"(`board.rs:248-262`) → 트레이 종료는 스레드로 옮겨 확인 후 `exit`. 흐름: 확인(탭 세션만) → 탭마다 pty 로 `Esc → /exit → Enter` → 탭 자식 종료를 몇 초 기다림 → 남은 탭은 `ClosePseudoConsole`(읽기 스레드와 다른 스레드에서 — 24H2 이전은 출력을 비우지 않으면 무한 대기, `ClosePseudoConsole` 문서 경고) → 스크립트 실행. 스크립트가 보드를 강제 종료해도 탭은 이미 닫혔다. **함정**: 백그라운드 작업(서브에이전트·백그라운드 셸·모니터)이 도는 세션에서 `/exit` 는 곧바로 끝나지 않고 "Background work is running" 대화상자를 띄운다(`agent-view` "From inside a session") → 기다림이 시간 초과로 끝나고 ConPTY 닫기(CTRL_CLOSE)로 넘어간다. 확인 창에 "서브에이전트·백그라운드 셸이 도는 세션"을 작업 중으로 보여 주는 결정이 바로 이 경우를 알린다. 셋째 경로: 사람이 새 setup.exe 를 직접 돌리면 NSIS 가 보드를 끝낸다(확인 없음, 사람이 고른 일 — PLAN §4.4 와 같음) |
| 확인 창에는 보드 탭 세션만, 서브에이전트가 도는 탭 세션은 작업 중으로 | **됨(확인)** | `store.rs:236-239`: idle 인데 끝나지 않은 서브에이전트나 백그라운드 셸이 있으면 이미 `working` 으로 바꾼다 → "탭 소속 ∧ state ∈ {working, waiting}" 필터가 그대로 맞다. 다른 터미널 세션은 탭 소속이 아니므로 빠진다 |
| `-p` 만으로 headless(SPEC §3.4) — 탭 안 `claude -p` 가 "백그라운드"로 보이지만 탭과 함께 끊김 | 위험 → 조정하면 됨 | 판별은 `console.rs:49-56`(`print_mode(argv) || terminal_window 없음`). 탭 안에서 사람·Claude 가 띄운 `claude -p` 는 탭 콘솔에 붙어 있으면 CTRL_CLOSE 로 함께 끝난다 → 정책("백그라운드는 영향 없음")과 어긋난다. **맞추는 법**: 보드가 세션마다 "탭 소속"을 따로 계산한다(파일 약속은 그대로) — ① 같은 콘솔(§2-3)이면 그 탭의 작업, ② 콘솔은 다르지만 조상에 탭 자식이 있고 그 사이에 supervisor 가 없으면 "탭과 함께 끊길 수 있음"으로 본다. 둘 다 확인 창에 그 탭의 작업으로 넣는다(결정 9 "경계" 문단과 같음). headless 표시·칩은 그대로 두되 표시 방식은 사용자 결정 §4-4 |
| PLAN 의 Job 과 탭 안 `claude --bg` | 위험 → 조정하면 됨 | 위 "Job 을 쓸지" 줄. 추가로 `←` 로 탭 세션 자체를 백그라운드로 보내면 그 대화가 supervisor 아래로 옮겨 가고 탭은 agent view 화면이 된다(`agent-view`). 같은 세션 id 로 worker 가 hook 을 부르는지(SessionStart 가 오는지)는 확인 안 됨 — 온다면 세션 파일의 pid 가 worker 로 바뀌고 조상에 탭 자식이 있어 조상 판정으론 `CB` 로 잘못 잡힌다(§2-3·§2-4) |

### 7. 보드로 옮기기 (다른 터미널 → 보드 탭)

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| Windows Terminal 등의 세션에 `/exit` 보내기 | 됨(조정 필요) | 지금 콘솔 입력(`console.rs:258-303 send_input`: `AttachConsole` → `CONIN$` → `WriteConsoleInputW`)으로 Esc·글자·Enter 를 넣는다. 실기 확인: 줄 단위·raw 모드·**Windows Terminal 탭 포함**(SPEC §5.7·§11.1 B6). **실제 Claude Code 에서의 일시중지·재개·중단은 미확인**(SPEC §10). `canDrive` = headless 아님 ∧ 붙을 수 있음(`store.rs:274`) — 관리자 권한 터미널처럼 붙지 못하면 메뉴를 끈다 |
| 못 보내면 대안 / 두 프로세스 동시 실행 막기 | 됨 | 보내기 → pid 가 죽을 때까지(최대 10초) `proc::alive(pid, pid_ctime)` 확인 → **죽은 뒤에만** 보드 탭에서 `--resume`. 안 죽으면 옮기지 않고 안내(예: "Background work is running" 대화상자, 또는 그 세션이 `claude attach` 로 붙은 백그라운드 세션이라 `/exit` 가 분리만 하고 계속 도는 경우 — `agent-view` "Detaching never stops a background session"). 붙지 못하는 세션은 비활성(PLAN §7)이 기본, 원하면 `proc::terminate` 후 이어 열기(대화 기록은 남음, 진행 중 응답만 잃음)를 확인 창과 함께 둘 수 있다. **공통 위험(macOS 도 같음)**: 입력 칸에 사람이 쓰던 글이 있으면 `/exit` 가 그 뒤에 붙어 요청으로 보내질 수 있다 — 상태가 idle·working 일 때만 허용하는 정도로 줄인다 |

### 8. 한글 입력

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| 조합 중 Return·Esc 가 사라지는 문제를 보드에서 고침(Windows 대응) | **위험(실기 1순위)** | xterm.js 6.0.0 `CompositionHelper.keydown`(`src/browser/input/CompositionHelper.ts:94-118`, 6.0.0 태그): 조합 중 `keyCode === 229` 키는 삼키고(`return false`) 다른 키코드가 오면 `_finalizeComposition(false)` 후 통과("This is mainly here for the case where enter is pressed"). 즉 **IME 가 Enter 를 keyCode 13 으로 다시 내주면 정상, 229 로만 주고 compositionend 로 끝내면 macOS 와 같은 "키 사라짐"**. Microsoft 한국어 IME + Chromium 에서 어느 쪽인지 적은 이슈·문서를 찾지 못했다(xterm.js·VS Code 이슈 검색: 조합 중 Enter 누락 보고 없음 — VS Code 터미널을 매일 쓰는 한국어 사용자가 많다는 점은 "정상일 가능성"의 약한 근거일 뿐). **고칠 자리는 있다(확인)**: `CoreBrowserTerminal._keyDown`(6.0.0 `src/browser/CoreBrowserTerminal.ts:1021-1037`)은 `attachCustomKeyEventHandler` 처리기를 `compositionHelper.keydown` **보다 먼저** 부른다. → 처리기에서 `ev.isComposing`(또는 keyCode 229) ∧ `ev.code ∈ {Enter, NumpadEnter, Escape}` 이면 "보류 키"로 적고, `term.textarea` 의 `compositionend` 뒤(xterm 이 확정 글자를 `setTimeout(0)` 으로 보내므로 그다음 틱) 그 사이 진짜 keydown 13/27 이 오지 않았을 때만 `\r`/`\x1b`(수정키 Enter 는 §4-6 의 바이트)를 보낸다 — macOS 의 "입력기가 확정과 함께 명령을 보냈을 때만 다시 보냄"과 같은 생각. 수정키 Return 은 처리기에서 xterm 기본 처리를 막고 확정 뒤에 보낸다 |
| 알려진 이슈 #6065 | **확인 — 직접 고쳐야 함** | 열림(2026-07-24). Windows 11 Chromium + **Microsoft 한국어 IME** 한글 모드에서 조합이 없어도 `Ctrl+<글자>`·`Ctrl+Space`·단독 `Esc` 가 keyCode 229 로 와서 `onData` 에 아무것도 안 감 — Claude Code 의 Esc(중단)·Ctrl+C·Ctrl+J 가 한글 모드에서 안 먹는다. 고치는 PR #6066 은 미병합이고 **Esc·Ctrl+글자만 다루며 Enter 는 범위 밖**. 우리 처리기에서 같은 복구(`ev.code` 로 진짜 키 찾기: Escape→`\x1b`, Ctrl+KeyA..Z→0x01..0x1A, Ctrl+Space→0x00)를 하고 false |
| #6049 | 확인 안 됨(해당 여부) | 열림. TSF IME 가 helper textarea 전체를 바꿔 쓰면 확정 앞부분이 잘림 — Microsoft Pinyin 으로 확인, 한국어 IME 해당 여부는 모름. 재현되면 확정마다 textarea 비우기 패치 |
| #6089 | 위험 | 열림(2026-08-05). 빠르게 칠 때 확정된 조합 하나가 통째로 사라짐(공유 불리언 `_isSendingComposition`). 재현은 Linux fcitx5-hangul, 한글처럼 음절마다 확정이 잦으면 잘 걸린다고 적혀 있음 — Claude Code 응답이 쏟아지는 동안(렌더 부하) 생길 수 있다. PR #6090 성격의 덧대기 검토 |
| 그 밖 | 참고 | Claude Code 자체가 Windows Terminal 에서 한글 조합을 망가뜨린 보고들(claude-code #65519·#73064·#77401) — xterm.js 에서는 조합이 웹뷰 textarea 에서 끝나고 확정 글자만 ConPTY 로 가므로 이 계열과는 길이 다르다(확인 안 됨). ConPTY 는 시작할 때 터미널에 `ESC[?9001h`(win32-input-mode 요청)·`ESC[?1004h` 를 늘 보낸다(`VtIo.cpp:197-203`) — xterm.js 6.0.0 은 win32-input-mode 를 모르므로 무시(좋음). **6.1 로 올리면 그 요청을 받아들여 비 US 배열 가상 키 오류(#6169)를 탈 수 있으니 끄는 옵션을 확인**한다 |
| 실기 확인 방법 — GitHub Windows 러너에서 한국어 IME + SendInput | 위험(가능성만) | 원리상 가능: 러너는 화면 세션이 있는 Windows Server 2025(SPEC §8.3, 스크린샷 확인됨) → `Set-WinUserLanguageList` 로 ko-KR 입력기 추가, 보드 창을 앞에 두고 `SendInput` 으로 VK_HANGUL(0x15)·자모 키 입력. 제약(모두 확인 안 됨): 서버 판에 Microsoft 한국어 IME 가 기본으로 있는지(없으면 기능 추가 다운로드 필요), TSF(ctfmon)가 그 세션에서 도는지, 입력기 목록 바꾸기가 이미 뜬 창에 바로 먹는지, "새/이전 버전 Microsoft IME" 중 어느 쪽인지, SendInput 이 WebView2 의 TSF 경로까지 똑같이 타는지. 참고로 설계 메모의 macOS 실측도 "다른 프로세스에서 입력기를 바꾸면 앞 앱 입력 문맥엔 적용되지 않는다"였다. **결론: 실제 Windows 11 PC 가 필요하다.** CI 에서 할 수 있는 것: 실기에서 한 번 이벤트 순서(keydown·composition*·input, 시각 포함)를 기록 → 같은 순서를 Playwright(Chromium) 또는 WebView2 CDP(`Input.imeSetComposition`·`Input.insertText`·`Input.dispatchKeyEvent`, `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port`)로 재생해 우리 고침이 회귀하지 않는지 본다(#6065 의 재현도 playwright-core 로 keyCode 229 를 강제한 방식) |

### 9. 답변 마크다운 꾸미기 (md-render)

| 결정 | 판정 | 근거·조정 |
|---|---|---|
| Windows Claude Code 에서 mod(`hooks/register.tsx`, `ui.render`)가 도는가 | 됨(문서 근거) | mods 문서 "Where mods run": "`claude` in a terminal, including an editor's integrated terminal … Hooks run: Yes / draws: Yes" — OS 제외 없음. 제외는 Desktop 앱의 WSL 세션뿐. "Mods are on by default. In the terminal, use Claude Code v2.1.287 or later". `CLAUDE_CODE_PLUGIN_DIRS` 는 "`;` on Windows" 로 구분한다고 적혀 있어 Windows 를 고려한 기능(`mods/reference`). `register.tsx` 는 `import type` 하나뿐이고 파일·경로·프로세스 접근이 없다(`mdmod/repo/hooks/register.tsx:1`). 실제 렌더링 확인은 계정이 있는 실기에서(§5) |
| 공백 있는 설치 경로의 `--plugin-dir` | 됨 | 셸 없이 인자 배열로 넘기므로 `C:\Users\홍 길동\AppData\Local\Programs\Claude Board\plugins\md-render` 도 인자 하나로 간다(인자 왕복은 Windows 실기 시험됨, `terminal.rs:583-656`). 플러그인 안 경로는 `./register.tsx`(슬래시)라 "백슬래시 경로는 Windows 에서만 읽힘" 주의(`plugins/loading`)와 무관. 한글·공백 경로에서 Bun 이 모듈을 읽는지는 CI 에서 `claude plugin validate`/`test` 를 그런 경로에 복사해 돌려 확인 가능 |
| 앱에 싣기·업데이트 | **됨(조정 필요 — PLAN 이 틀림)** | **install.ps1 은 zip 의 최상위 파일만 바꿔 넣는다**(`install.ps1:121` `Get-ChildItem -LiteralPath $stage -File`) → `plugins\md-render\`(그리고 PLAN 의 `conpty\`) 하위 폴더가 업데이트에서 빠진다. PLAN §4.4 의 "install.ps1 은 고치지 않아도 된다"는 틀렸다 — 하위 폴더까지 "이름 바꿔 비키기" 하도록 고친다(업데이트는 새 태그의 install.ps1 을 받아 돌리므로 옛 앱과 호환 문제 없음, `updater.rs:88-99`). 함께: 필수 파일 목록(`install.ps1:92`), CI zip 목록 비교(`.github/workflows/windows.yml:292`), SPEC §8.1, Tauri `bundle.resources`(NSIS 설치 폴더 아래 경로가 그대로 되는지 확인 안 됨 → `install-checks.ps1` 에 확인 추가), 제3자 고지(MIT) |
| CI 에서 `claude plugin validate`/`claude plugin test` | 됨(인증 불필요) | 문서: 테스트는 "without a session, a sign-in, or a network", 실패하면 종료 코드 1(`mods/test`). 원 저장소 CI 도 ubuntu-latest 에서 비밀값 없이 `npm install -g @anthropic-ai/claude-code@2.1.288` → `claude plugin validate . --strict` → `claude plugin test .`(`mdmod/repo/.github/workflows/ci.yml`). windows-latest 에서 같은 명령(정확한 버전 고정, ≥ 2.1.287)을 돌리면 된다. "hooks modules are turned off" 가 나오면 관리 설정 문제 |
| `--plugin-dir` 세션의 부수 효과 | 참고 | `--plugin-dir` 로 띄운 대화형 세션은 저장할 때마다 다시 읽고 그 줄을 대화에 흐리게 남긴다(`mods/troubleshoot`) — 설치 폴더를 바꾸는 순간(업데이트)에만 해당하고 그때 탭은 이미 닫혀 있다. 탭에서 백그라운드로 옮겨 간 세션이 그 폴더를 계속 지켜볼 수 있는데, 파일 교체가 이름 바꾸기 방식이라 대체로 문제없을 것(확인 안 됨) |

### 10. 서브에이전트 표시

| 결정 | 판정 | 근거 |
|---|---|---|
| 지금처럼 부모 세션 아래 | **됨(확인)** | hook 은 `agent_id` 이벤트를 세션 상태 대신 부모 기록의 `subagents` 에만 남기고(`state.rs:160-209`, SPEC §3.2, macOS `state.py:146-174` 와 같은 규칙, `requestShape=="background"` 도 같은 자리), 보드는 부모 아래 줄로 그린다(`store.rs:226-234`). 서브에이전트는 부모 프로세스 안에서 돌므로 탭과 함께 끊긴다 — 확인 창 반영은 `store.rs:236-239` 가 이미 함 |

---

## 2. 막히거나 위험한 것과 대안 (자세히)

### 2-1. 한글 IME (위험, 실기 1순위)

- 확인된 문제: 한글 모드에서 Esc·Ctrl+글자 누락(#6065) → 처리기로 복구(위 표). 이것 없이는 Claude Code 중단(Esc)·Ctrl+C·Ctrl+J 가 한글 모드에서 안 된다.
- 모르는 것: 조합 중 Enter·Esc·Shift+Enter 의 이벤트 순서(229 만 오는지, 229 뒤 13 이 다시 오는지). 둘 다 대비하는 "보류 키 + 중복 방지" 처리기를 쓰면 어느 쪽이든 맞게 동작하도록 만들 수 있다. 다만 xterm.js 가 확정을 `setTimeout(0)` 으로 보내고(#6089 가 그 경쟁을 지적) 우리 다시 보내기도 그 뒤 틱에 해야 하므로, 순서가 꼬이지 않는지는 실기에서만 안다.
- 내려가는 순서(PLAN §9 단계 0 과 같음): DOM 렌더러(WebGL 조합 위치 #6161 회피) → #6066·#6060 이 들어간 6.1 베타를 정확한 버전으로(그때 win32-input-mode 끄기 확인) → ghostty-web 한국어 IME 수정(#120) 병합 여부 재확인.

### 2-2. Job 과 백그라운드 정책 (위험 → 설계로 해결)

- PLAN §4.3 그대로면 정책 위반(§1-6). 권장: **Job 없음**. 보드가 죽으면 ConPTY 가 CTRL_CLOSE 로 탭 콘솔 클라이언트를 끝낸다(OpenConsole `main` 소스 확인 — 시스템 conhost 의 같은 동작은 CI 시험 ⑧로 확인할 것, 안 되면 sideload 또는 Job (나)). 분리 실행된 supervisor 는 남는다.
- 꼭 지킬 구현 조건: 보드가 쥔 ConPTY 파이프 끝은 상속 불가(`CreatePipe` 의 `bInheritHandle = FALSE`), 탭 자식은 `bInheritHandles = FALSE` 로 띄운다(node-pty 와 같음). 보드가 띄우는 다른 프로세스(업데이트 powershell 등)가 파이프를 물려받으면 보드가 죽어도 파이프가 살아 탭이 안 끝난다.
- Job 을 쓰더라도 `SILENT_BREAKAWAY_OK` 여야 한다. 일반 `BREAKAWAY_OK` 는 libuv 때문에 소용없다.
- 확인 창·이어 열기에서 "CTRL_CLOSE 5초"를 감안한다(§1-4).

### 2-3. 보드 탭 소속 판정 (위험 → 콘솔 동일성으로 해결)

- 조상만 보면 틀리는 경우: 탭에서 시작된 supervisor 아래 worker(백그라운드 세션)는 조상에 탭 claude.exe → `ClaudeBoard.exe` 가 있다. 그러면 ① 보드는 `CB` 로 잘못 잡고, ② hook 의 headless 판별(`console.rs:224-234` 조상의 보이는 창)도 보드 창을 찾아 false 로 기록해 "터미널" 구역에 들어간다.
- 권장 판정: **세션 프로세스가 탭과 같은 콘솔에 붙어 있는가.** 방법 둘 — (a) 지금 core 가 쓰는 `AttachConsole(pid)` + `GetConsoleWindow` 로 콘솔 창(ConPTY 마다 하나인 숨은 `PseudoConsoleWindow`)을 얻어 탭 자식의 것과 비교(전역 잠금·표준 핸들 되돌리기가 이미 있음, `console.rs:143-160`), (b) `NtQueryInformationProcess(ProcessConsoleHostProcess=49)` 로 콘솔 호스트(OpenConsole/conhost) pid 를 비교 — 붙지 않아도 되지만 문서에 없는 값이다(windows-sys 에 상수가 없어 49 를 직접 씀, `check/api-check` 컴파일 확인). PLAN §5.3 은 "보드가 자기 ConPTY 콘솔에 AttachConsole 하는 것을 피한다"고 했으므로 (b)를 먼저 실기 시험하고 안 되면 (a).
- 보정 규칙: 탭 콘솔 소속 → `CB`, headless=false. 콘솔은 다른데 `terminal_window` 가 보드 자신의 창으로 풀리는 세션 → headless=true 로 덮고 백그라운드 칩. 확인 창 포함은 §1-6 의 ①②.
- supervisor pid 를 알면 ②의 "사이에 supervisor 가 없음"을 정확히 볼 수 있다: `claude daemon status` 가 supervisor pid 를 보고하고(`agent-view` "Where state is stored"), Windows 는 `~/.claude/daemon.lock` 에 pid 를 적는다(v2.1.257 변경 기록) — 파일 형식은 확인 안 됨.

### 2-4. Claude Code 자체의 "백그라운드로 보내기"와 보드 탭 (위험, macOS 도 같음)

- 보드 탭에서 빈 입력에 `←` 한 번이면 대화가 supervisor 아래로 옮겨 가고 탭은 agent view 가 된다. `/exit` 의 "Background work is running" 대화상자에도 "Move to background and exit" 가 있다.
- 결과: 보드 탭 세션이 "보드 밖 백그라운드 세션"이 되고(정책상 보드는 손대지 않아야 함), 다음 보드 시작 때 이어 열면 실패(종료 코드 1)하거나 attach 가 된다. §1-4 ①의 "다른 pid 가 살아 있으면 열지 않고 목록에서 뺌"으로 막는다.
- 아예 막으려면 탭 세션 환경에 `CLAUDE_CODE_DISABLE_AGENT_VIEW=1`(또는 설정 `disableAgentView`) — 문서: "turn off background agents and agent view entirely", 그러면 exit 대화상자의 "Move to background" 도 안 보인다. 대신 보드 탭에서 Claude Code 의 백그라운드 기능을 못 쓴다 → 사용자 결정 §4-5.

### 2-5. 실제 Claude Code 와 ConPTY 키 (위험)

- 단독 Esc(`\x1b`): VS Code(xterm.js + ConPTY)에서 Claude Code 의 Esc 가 동작하므로 될 것으로 보지만 확인 안 됨(PLAN §7 과 같음).
- Shift+Enter: Claude Code 문서상 VS Code 는 `/terminal-setup` 이 키 설정을 넣어야 되고, Windows Terminal 은 설정 없이 된다(win32-input-mode 로 수정키를 그대로 보내므로). 우리(xterm.js 6.0, win32-input-mode 없음)는 VS Code 쪽 → 보드가 바이트를 정해 보내야 한다. 후보 `\x1b\r`(macOS Ghostty 사용자 설정과 같음)은 ConPTY 입력 해석기가 ESC 를 따로 떼어 Esc 키(중단)로 넘길 위험이 있다(확인 안 됨). **Ctrl+J(`\n`)는 "every terminal with no setup" 에서 줄바꿈**(`terminal-config` 문서) → Windows 기본은 `\n` 권장, 실기에서 둘 다 확인. 사용자 결정 §4-6.
- Backspace: Claude Code 는 Windows 에서 `^H` 를 Ctrl+Backspace(단어 지우기)로 읽는다(`terminal-config`). xterm.js 기본 Backspace 는 `\x7f` 라 문제없을 것 — 아니면 탭 환경에 `CLAUDE_CODE_BS_AS_CTRL_BACKSPACE=0`.
- 다시 맞추기(PLAN §3.4) 때 고리 버퍼를 새 xterm 에 다시 쓰면, 버퍼 안의 터미널 질의(DA1 `ESC[c` 등 — ConPTY 가 시작할 때 보냄, `VtIo.cpp:197-203`)에 xterm 이 **다시 답해 그 답이 사용자 입력으로 들어간다** → 다시 쓰는 동안 `onData` 를 버린다.
- PLAN §3.1 의 "INHERIT_CURSOR 의 커서 질의를 얼마나 기다리는지 확인 안 됨"은 이제 확인: `INHERIT_CURSOR` 일 때만 DA1 응답을 최대 1초 기다린다(`VtIo.cpp:208-217 WaitUntilDA1(1000)`). 플래그 0 이면 기다리지 않는다 — PLAN 의 "기본 0" 이 맞다.

### 2-6. 환경 (조정)

- `CreateEnvironmentBlock` 으로 해결(§1-3). PowerShell 프로필로만 PATH 를 바꾸는 도구는 못 잡는다 — macOS 처럼 셸을 한 번 띄워 환경을 받을지는 사용자 결정 §4-3.

---

## 3. PLAN.md 에서 바꿀 부분

| PLAN 위치 | 지금 내용 | 바꿀 내용 |
|---|---|---|
| §0 "실행 방식 … 결정 필요", §11 실행 행, §12-1 | 셸 없이 직접 실행할지 결정 필요 | 결정됨(설계 메모 결정 8). §12-1 삭제. macOS 도 같은 방식이므로 §11 의 "macOS 는 셸 + 첫 입력" 행을 고친다 |
| §0 창 행, §6.1, §6.2 `+page.svelte` | 오른쪽 보드 폭 454 고정, `.board` 폭 고정 | 경계 끌기로 보드 칸 폭 가변(설정에 저장), 단계 계산을 보드 칸 폭으로(결정 2) |
| §6.2 `enter_mini` | 펼쳐 있으면 먼저 접는다 | 그대로 두고 "미니에서 보드 탭 세션 클릭 → 미니 해제 + 펼침"을 더한다(결정 4) |
| §6.2 전역 단축키 | Ctrl+Alt+B 동작만 | Ctrl+Alt+T(펼치기·접기) 등록 추가, AltGr·JetBrains 충돌을 문서에(§1-2) |
| §2.3 IME 표 | #6065 Esc·Ctrl 우회만 | 조합 중 Enter·Esc·수정키 Enter 의 "보류 키 + 확정 뒤 보내기 + 중복 방지"(macOS 고침의 대응), 처리기가 CompositionHelper 보다 먼저 불린다는 확인(6.0.0 `CoreBrowserTerminal.ts:1025`), #6066 은 Enter 범위 밖, 실기 이벤트 기록 → CI 재생 시험 |
| §2.3 Shift+Enter | 바이트 확인 안 됨 | Windows 기본 `\n`(Ctrl+J) 권장, `\x1b\r` 은 실기 비교 후(§2-5) |
| §3.1 이유 1 | 커서 질의 대기 시간 확인 안 됨 | `INHERIT_CURSOR` 일 때만 DA1 1초 대기(소스 확인). 시작할 때 ConPTY 가 `ESC[c ESC[?1004h ESC[?9001h` 를 늘 보냄 — 6.1 로 올릴 때 win32-input-mode 끄기 |
| §3.4 다시 맞추기 | 버퍼를 다시 씀 | 다시 쓰는 동안 xterm `onData`(질의 응답)를 버린다 |
| §3.1·§8.2 파이프 | — | 보드 쪽 파이프 끝 상속 불가, 탭 자식 `bInheritHandles=FALSE` 를 명시(ConPTY 끊김이 탭 종료를 보장하는 전제) |
| §4.1 `boardTerminals` | `{sessionId, cwd, args, openedAt}` | 탭 자식 `pid`·`ctime` 추가. 탭↔세션 id 는 `/clear`·`/resume` 때 pid 로 다시 맞춘다 |
| §4.2-2 | pid 가 살아 있으면 "다른 곳에서 돌고 있음" | "옛 탭 자식이면 CTRL_CLOSE(최대 5초)를 6~10초 기다림" ↔ "다른 pid 면 이어 열지 않고 목록에서 뺌(백그라운드로 옮겨 간 세션)"을 나눈다. resume_args 의 설정 플래그 때문에 도는 백그라운드 세션을 열면 종료 코드 1 이 된다는 문서 근거를 적는다 |
| §4.3 Job | `KILL_ON_JOB_CLOSE | BREAKAWAY_OK`, 결정 필요 | **Job 없음(권장)** 또는 `KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK`. 일반 BREAKAWAY_OK 는 libuv 가 breakaway 를 요청하지 않아 supervisor 가 Job 에 남는다. ConPTY 끊김 → CTRL_CLOSE 로 탭 클라이언트가 끝난다는 근거(소스·문서)를 "확인"으로 |
| §4.4 확인 흐름 | `/exit` 기다림 3초 | `/exit` 가 "Background work is running" 대화상자에 막힐 수 있음 → 시간 초과 시 `ClosePseudoConsole`(읽기 스레드와 다른 스레드). 트레이 종료 확인은 메인 스레드 밖에서. 확인 창 목록 = 탭 소속(콘솔 동일 + 탭 자손 `-p`) ∧ working/waiting |
| §4.4 install.ps1 | "고치지 않아도 된다" | **고쳐야 한다** — 최상위 파일만 바꿔 넣으므로(`install.ps1:121`) `conpty\`·`plugins\md-render\` 하위 폴더 처리를 더한다. `$need` 목록, `windows.yml:292`, SPEC §8.1 함께 |
| §5.2 짝짓기 | 세션 id + pid 조상 | + 콘솔 동일성(§2-3)을 최종 판정으로, supervisor 아래는 제외, [셸 열기] 탭은 id 없이 pid·콘솔로 |
| §5.3 보정 | 짝지은 세션 headless=false | + 콘솔은 다르지만 조상 창이 보드인 세션은 headless=true 로 덮음 |
| §6.1 [셸 열기] | pwsh → powershell → cmd | 그대로(확인). 띄우는 환경은 새 환경 블록으로 |
| §7 보드로 옮기기 | 최대 10초 기다림 → resume | 안 죽으면 옮기지 않음(대화상자·attach 된 백그라운드 세션), 입력 칸에 글이 있을 때의 위험 표기 |
| §7·§8.2 claude 띄우는 환경 | 언급 없음 | `CreateEnvironmentBlock` 으로 새 환경(Cargo 기능 `Win32_System_Environment` 추가). claude·셸·이어 열기 모두 |
| §8 md-render | 언급 없음(설계 메모의 결정 뒤에 나옴) | `bundle.resources` 로 `plugins/md-render/` 4파일, zip·install.ps1·CI 목록, 제3자 고지(MIT), 보드가 `--plugin-dir` 를 빼고 다시 넣는 규칙(§1-4), CI 에 windows-latest `claude plugin validate --strict`·`claude plugin test`(한글·공백 경로 사본 포함) |
| §8.5 CI 실기 시험 | ①~⑦ | + ⑧ 시험 프로세스가 ConPTY 를 쥔 채 강제 종료되면 ConPTY 자식이 5초 안에 끝나는지(Job 없이), ⑨ 그 자식이 `DETACHED_PROCESS` 로 띄운 손자는 살아남는지(Job 없음·SILENT 각각), ⑩ 콘솔 동일성 판정(같은 콘솔 손자 / `CREATE_NO_WINDOW` 손자 / 분리 손자), ⑪ `HKCU\Environment` 의 PATH 를 바꾼 뒤 새 환경 블록으로 띄운 자식이 새 PATH 를 보는지, ⑫ 실제 Claude Code 네이티브 설치 후 런처 구조(해시 비교·자식 프로세스 표, 인증 불필요) |
| §12 결정 목록 | 1~6 | 1(셸 없이)·3(이어 열기 자리)·4(새 세션) 은 결정됨. 2(Job)는 §4-1 로 좁혀짐. 5(`.paused` 재개 표지)·6(sideload ConPTY)은 여전히 열림 |

---

## 4. Windows 에서 새로 필요한 사용자 결정

1. **Job 방식 확인** — (가) Job 을 쓰지 않는다(권장: ConPTY 끊김으로 탭이 끝나고 Claude Code 가 정리할 시간 5초를 받는다), (나) 탭 claude.exe 하나만 담는 Job(`SILENT_BREAKAWAY_OK`, 즉시 끝냄 대신 정리 없음). 어느 쪽이든 결정 9 의 "분리 실행은 빠져나간다"는 지켜진다.
2. **Windows 펼치기·접기 단축키** — Ctrl+Alt+T(macOS ⌃⌥T 와 같은 꼴, JetBrains "Surround With" 와 충돌, AltGr 배열 주의) 로 갈지, 다른 키로 할지.
3. **claude 를 띄우는 환경** — 레지스트리 기준 새 환경(`CreateEnvironmentBlock`, 보드 프로세스에만 있던 변수는 버림)만 쓸지, macOS 의 로그인 셸처럼 PowerShell 을 프로필과 함께 한 번 띄워 환경을 받을지(fnm 같은 프로필 기반 도구까지 잡지만 느리고 사용자 프로필 스크립트가 돈다).
4. **보드 탭 안에서 띄운 `claude -p` 의 표시** — 지금 규칙대로 "백그라운드 · `$ claude -p`" 칩으로 둘지, 그 탭 세션 아래에 붙여 보일지(어느 쪽이든 업데이트·종료 확인 창에는 그 탭의 작업으로 넣는다 — 결정 9 경계 문단). Windows 는 `-p` 만으로 headless 가 되므로 이 경우가 macOS 보다 자주 보인다.
5. **보드 탭에서 Claude Code 의 "백그라운드로 보내기"(`←`·`/bg`·exit 대화상자의 Move)를 허용할지** — 허용하면 탭 세션이 보드 밖 백그라운드 세션으로 바뀌고(보드는 손대지 않음), 막으면 탭 세션 환경에 `CLAUDE_CODE_DISABLE_AGENT_VIEW=1` 을 넣어 그 기능이 사라진다. macOS 판에도 같은 질문이다.
6. **Shift+Enter 로 보낼 바이트** — Windows 기본 `\n`(Ctrl+J, 문서상 모든 터미널에서 줄바꿈)을 권장. macOS 의 `\x1b\r` 과 맞출지는 실기 비교 뒤.
7. (작음) **[셸 열기] 의 셸** — pwsh → powershell → cmd 고정 순서로 둘지, 설정으로 고르게 할지(Git Bash 포함 여부).
- 그대로 열려 있는 PLAN 결정: 업데이트·종료로 끊긴 작업 세션에 `.paused` 를 남겨 "재개" 단추로 이어 가게 할지(PLAN §12-5), sideload ConPTY 를 실을지(PLAN §12-6).

---

## 5. 확인 장소별 목록

### 5-1. 실제 Windows PC(사람)로만 확인 가능

1. 한국어 IME(Windows 11 + Microsoft 한국어 IME, 새/이전 버전 둘 다): 조합 중 Enter·Esc·Shift+Enter·Ctrl+C, 한글 모드에서 조합 없이 Esc·Ctrl+C·Ctrl+J(#6065), 빠른 입력·응답이 쏟아지는 중 입력(#6089), 공백 뒤 조합(#6049), 한/영·한자 키, 조합 글자 위치(WebGL #6161). 이때 이벤트 순서를 기록해 CI 재생 자료로 남긴다.
2. 실제 Claude Code(로그인 필요)를 보드 탭에서: 단독 Esc 중단, Shift+Enter(`\n`·`\x1b\r`), Backspace, 붙여넣기·이미지 붙여넣기(Alt+V), 창 크기 바꾸기 뒤 입력 상자, 한글·이모지 폭, 동기화 출력 깜빡임.
3. md-render 꾸미기가 xterm.js 탭에서 실제로 그려지는지(한글 사용자 이름·공백 설치 경로 포함), 폴더 신뢰·bypass 경고가 탭에서 답해지는지.
4. 탭에서 `←`·`/bg`·`claude --bg` 로 supervisor 를 띄운 뒤 보드를 강제 종료·업데이트 → supervisor 와 백그라운드 세션이 살아 있는지, 옮겨 간 세션의 hook 이 같은 세션 id·worker pid 로 오는지, 그 세션 파일의 headless 값.
5. CTRL_CLOSE_EVENT 를 받은 Claude Code 가 5초 안에 SessionEnd hook 을 돌리고 끝나는지, 백그라운드 작업을 넘기는지.
6. 업데이트 흐름 전체: 확인 창(서브에이전트 도는 탭 포함) → `/exit`(대화상자 막힘 포함) → install.ps1 → 새 보드가 탭을 이어 엶.
7. Windows Terminal·conhost 에서 도는 실제 Claude Code 에 "보드로 옮기기"(`/exit` 주입) — 기존 일시중지·재개·중단 미확인 항목과 같음(SPEC §10).
8. Ctrl+Alt+T 등록, 한국어·독일어 등 배열에서의 동작, JetBrains 와의 충돌.
9. 로그인 뒤 node·Git 을 설치하고 보드를 다시 띄우지 않은 채 새 탭 → 사용자 hook(node)이 도는지.
10. 펼치기(여러 모니터·배율 125/150%·작업 표시줄 위치), 투명 창 위 WebGL·Mica, 항상 위 해제, 미니에서 펼치기.

### 5-2. GitHub Windows 러너(CI)로 확인 가능 (인증 불필요)

- ConPTY 래퍼 시험(PLAN §8.5 ①~⑦) + §3 표의 ⑧~⑫(강제 종료 시 CTRL_CLOSE, 분리 손자 생존, 콘솔 동일성, 새 환경 블록, 네이티브 설치 런처 구조).
- `claude plugin validate --strict`·`claude plugin test` 를 windows-latest 에서, 한글·공백 경로 사본으로.
- 설치 확인: setup.exe·install.ps1 업데이트가 `plugins\md-render\`·`conpty\` 를 만들고 바꾸고 지우는지.
- IME 회귀: 실기에서 기록한 이벤트 순서를 Playwright/CDP 로 재생해 처리기 결과 바이트 비교.
- (정보용 시도) 러너에 ko-KR 입력기를 넣고 `SendInput` 으로 실제 IME 를 태워 보기 — 될지 모른다(§1-8). 실패해도 위 재생 시험으로 대신한다.

---

## 6. 출처 (조회일 2026-10-17)

- 저장소(커밋 `21358bf`): `windows/crates/core/src/state.rs:63-122`, `console.rs:49-56·143-160·207-236·258-303`, `proc.rs:63-77·123-132`; `windows/app/src-tauri/src/board.rs:218-224·248-262·273-301·316-337·682-705·849`, `store.rs:226-239·265-274`, `terminal.rs:28-38·47-135·180-212·238-241·280-305·583-656`, `updater.rs:88-99·149-203`, `tray.rs:103`, `window.rs:9-14·33-36·56-60`; `windows/app/src/routes/+page.svelte:33-37·49-50`, `src/lib/board/SessionRow.svelte:30-52`, `GroupCard.svelte:33-34`; `windows/crates/hook/src/spawn.rs:8-19`, `setup.rs:219-220`; `windows/packaging/install.ps1:92·111-115·121`; `.github/workflows/windows.yml:292`; `windows/docs/SPEC.md` §2.1·§3.3·§3.4·§5.7·§5.8·§5.10·§10·§11.1.
- Microsoft 문서: `learn.microsoft.com/windows/console/closepseudoconsole`(CTRL_CLOSE_EVENT, 24H2 즉시 반환, 출력 비우기 경고), `…/console/handlerroutine`(CTRL_CLOSE 5000ms), `…/win32/api/winnt/ns-winnt-jobobject_basic_limit_information`(BREAKAWAY_OK·SILENT_BREAKAWAY_OK·KILL_ON_JOB_CLOSE), `…/win32/api/userenv/nf-userenv-createenvironmentblock`.
- microsoft/terminal 소스(main, gh api): `src/host/VtIo.cpp`(시작 시 `ESC[c ESC[?1004h ESC[?9001h`, `WaitUntilDA1(1000)` 은 cursor 상속일 때만, `SendCloseEvent` → `CloseConsoleProcessState`), `src/host/PtySignalInputThread.cpp`(`_Shutdown`), `src/host/VtInputThread.cpp`(입력 파이프 끊김 시 `SendCloseEvent`). Windows Terminal 1.18 `compatibility.reloadEnvironmentVariables`(github.com/microsoft/terminal/releases/tag/v1.18.1421.0).
- libuv `src/win/process.c`(v1.x): 분리 실행에 `CREATE_BREAKAWAY_FROM_JOB` 을 쓰지 않는 주석, `CREATE_NO_WINDOW` 조건.
- xterm.js(gh api): 6.0.0 태그 `src/browser/CoreBrowserTerminal.ts:1021-1043`, `src/browser/input/CompositionHelper.ts:94-118`; 이슈·PR #6065(열림)·#6066(열림, Esc·Ctrl 만)·#6049(열림)·#6089(열림)·#6060·#6090·#6169·#6161·#6112.
- 관련 이슈: anthropics/claude-code #65519·#73064·#77401(Windows Terminal 의 한글 조합), microsoft/vscode #331903·#331754·#301552(VS Code 터미널 IME).
- Claude Code 문서(code.claude.com/docs/en/…): `agent-view`(supervisor, `←`, Background work 대화상자, `disableAgentView`/`CLAUDE_CODE_DISABLE_AGENT_VIEW`, Windows 메모, 변경 기록), `sessions`(도는 백그라운드 세션 이어 열기 — 설정 플래그면 종료 코드 1, resume 때 `--plugin-dir` 미복원), `cli-reference`(`--session-id` UUID, `--plugin-dir`, `--bg`), `setup`(Windows 설치·Git Bash, 런처는 macOS·Linux 만 설명), `terminal-config`(Shift+Enter 표, Ctrl+J, Windows `^H`), `plugins/mods/overview`(Where mods run, v2.1.287), `plugins/mods/test`(sign-in 불필요), `plugins/mods/reference`(`CLAUDE_CODE_PLUGIN_DIRS` `;` on Windows), `plugins/mods/troubleshoot`, `plugins/loading`. Claude Code 설치 스크립트 `claude.ai/install.ps1`(내려받아 읽기만, `check/dl-install/`).
- md-render 저장소 사본: `scratchpad/mdmod/repo`(`.github/workflows/ci.yml`, `hooks/hooks.json`, `hooks/register.tsx`).
- 키보드: Raymond Chen, "Why Ctrl+Alt shouldn't be used as a shortcut modifier"(devblogs.microsoft.com/oldnewthing/20040329-00/?p=40003)와 2012 후속(…/20120423-00/?p=7803).
- 컴파일 확인: `scratchpad/windows-plan/check/api-check`(windows-sys 0.61.2 — `CreateEnvironmentBlock`·`OpenProcessToken`·`SetInformationJobObject(KILL_ON_JOB_CLOSE|SILENT_BREAKAWAY_OK)`·`NtQueryInformationProcess(49)`) `cargo check --offline --target x86_64-pc-windows-msvc` 통과(실행 안 함).
- 웹 조회의 User-Agent 는 `claude-board-research`, gh 는 읽기 API 만 썼다. GitHub Actions 는 실행하지 않았다.


---

# 2부. 첫 계획 (점검 전)


기준: 설계 메모 `scratchpad/embedded-terminal-design.md`(macOS·Windows 공통), `windows/docs/SPEC.md`(커밋 `21358bf` 시점), `windows/` 소스.
저장소 파일은 고치지 않았다. 조사·시험 흔적은 이 폴더의 `research/`(내려받은 소스·문서 발췌), `pty-check/`(컴파일 확인용 cargo 시험 프로젝트), `download-conpty/`(NuGet ConPTY 패키지 — 실행하지 않고 목록·내보내기 표만 읽음).

표기: **확인** = 저장소·레지스트리·소스로 직접 확인. **확인 안 됨** = 기억이나 추론, 또는 Windows 실기에서만 알 수 있는 것.

---

## 0. 한눈에

| 항목 | 권장안 |
|---|---|
| 터미널 화면 | **xterm.js 6.0.0 + `@xterm/addon-webgl` 0.19.0**(+ fit 0.11.0, unicode11 0.9.0 또는 unicode-graphemes 0.4.0) — 정확한 버전 고정. 한국어 IME 의 Esc·Ctrl 누락(#6065)은 키 처리기로 직접 보낸다 |
| pty | **windows-sys 로 쓴 얇은 ConPTY 래퍼**(node-pty 와 같은 방식) + **Microsoft.Windows.Console.ConPTY 1.25.260930003**(conpty.dll·OpenConsole.exe, MIT)을 앱 옆에 싣고 절대 경로로 불러온다. 없으면 kernel32 의 ConPTY. macOS 개발 빌드만 portable-pty 0.9.0 |
| 바이트 전달 | 백엔드가 pty 출력을 **늘 비우며** 터미널마다 고리 버퍼에 쌓고, 8~16ms 단위로 묶어 Tauri `Channel`(`InvokeResponseBody::Raw`)로 보낸다. 입력은 `invoke` |
| 실행 방식 | **셸을 거치지 않고 `claude.exe` 를 인자 배열로 ConPTY 안에 직접 띄운다**(SPEC §5.7 의 명령 주입 방지 규칙). 설계 메모의 "셸 + 첫 입력으로 명령"과 다르다 — 결정 필요 |
| 짝짓기 | 보드가 띄울 때 **세션 id 를 미리 안다**(`--resume <id>` 는 같은 id 를 쓰고, 새 세션은 `--session-id <uuid>`). 세션 파일의 pid 조상에 보드 자식 pid 가 있는지로 확인. **파일 약속은 바꾸지 않는다** |
| 세션 유지 | 보드가 ConPTY 를 쥐고, 쥔 세션 목록을 `board-settings.json` 에 남긴다. 다시 뜨면 `resume_args` 로 이어 연다. 업데이트·종료 전 작업 중 세션이 있으면 확인 |
| 창 | "터미널 펼치기"(기본 켜짐) = 그 모니터 작업 영역 전체, 왼쪽 터미널·오른쪽 보드 폭 454. 펼친 동안 항상 위·투명도 효과는 끈다 |

큰 위험 세 가지: ① 한국어 IME(Windows 실기 전에는 알 수 없음), ② ConPTY 플래그·글자 폭·시작 때 커서 질의, ③ 실제 Claude Code 에서 hook 이 기록하는 pid 가 보드 자식과 어떤 관계인지(실행 파일이 런처인지).

---

## 1. 지금 코드에서 확인한 사실

- **세션 파일의 `tty` 는 Windows 에서 늘 `null`**(SPEC §2.1, `crates/core/src/state.rs` 의 `ClaudeProcess.tty` 주석 "Windows 는 늘 None"). `pid` 는 hook 의 부모부터 최대 8단계 올라가며 처음 만나는 `claude.exe`(또는 명령줄에 `claude` 가 든 `node.exe`/`bun.exe`)이고, `pid_ctime`(생성 시각)이 함께 남는다(`proc::find_claude`). 기록된 pid 가 살아 있으면 다시 찾지 않는다(`state::apply`).
- **headless 판별**(`console::is_headless`): `-p`/`--print` 이거나, 그 pid 의 콘솔에 붙지 못하거나, 콘솔 창의 소유 창·콘솔 창·조상 프로세스의 보이는 최상위 창이 모두 없으면 참. 보드가 쥔 ConPTY 에서는 콘솔 창이 숨은 가짜 창이고, 소유 창은 누가 정해 주지 않으면 없다 → 조상(= `ClaudeBoard.exe`)의 보이는 창으로 판정된다. **보드 창이 숨어 있을 때 세션이 시작되면 headless 가 참이 된다**(백그라운드 구역으로 분류, 제목 바꾸기 생략).
- **터미널 제어**(`app/src-tauri/src/terminal.rs`): 일시중지·재개·중단·자동 전환은 `console::send_input`(`AttachConsole` → `WriteConsoleInputW`), 이어 열기는 `CreateProcessW(CREATE_NEW_CONSOLE)` 로 `claude.exe` 를 인자 배열 그대로 띄운다. 3초마다 `probe_in_background` 가 모든 살아 있는 세션에 `console::can_attach` 를 해 `can_drive` 를 정한다.
- **창**(`window.rs`·`board.rs`): 장식 없음·투명·`maximizable: false`, 실행 폭 454·작업 영역 높이 전체, `pinned`(항상 위) 기본 켜짐, 창이 움직임을 멈추면 `window_frame`·`board_restore_frame` 저장, 폭 360 미만이면 미니. 화면 단계는 `+page.svelte` 의 `pickTier()` 가 `window.innerWidth` 로 고른다. 투명도는 `document.documentElement.style.opacity` 로 문서 전체에 건다.
- **알림 패널 클릭**(`notify.rs::notify_clicked`)은 `ClaudeBoard.exe --show` 만 띄운다 — 세션 id 가 없다(hook 이 패널에 넘기는 인자에도 없다, SPEC §3.6).
- **업데이트**(`updater.rs`): 확인 없이 `install.ps1 -Update` 를 띄우고, 스크립트가 `Get-Process -Name ClaudeBoard | Stop-Process -Force` 로 보드를 끈다(`install.ps1` 111~113행). 트레이 "종료"는 `app.exit(0)`(`tray.rs` 103행) — `ExitRequested` 는 `code: None` 일 때만 막는다.
- **스냅숏 필드 이름 충돌**: `Snapshot.terminals` 는 이미 터미널 독 목록(Windows Terminal·PowerShell 등)이다. 새 필드는 다른 이름을 쓴다.
- **릴리스 작업이 zip 안의 파일 목록을 정확히 비교한다**(`.github/workflows/windows.yml` 292행: `ClaudeBoard.exe LICENSE README.md claude-board-hook.exe`). 파일을 더하면 이 확인·SPEC §8.1·install.ps1(92행 `foreach ($need …)`)을 함께 고쳐야 한다.
- 이 Mac 에 `x86_64-pc-windows-msvc` target 이 있다(`rustup target list --installed`). cargo 1.95.0. 저장소의 tauri 는 2.12.4(`app/src-tauri/Cargo.lock`).

---

## 2. 터미널 화면 (웹뷰 안 엔진)

### 2.1 후보

| 후보 | 버전·갱신 | 라이선스 | 렌더러 | 한국어 IME | 판단 |
|---|---|---|---|---|---|
| `@xterm/xterm` | 6.0.0 (2025-12-22), 저장소 활동 2026-10-13, ★21k | MIT | DOM(기본), WebGL 애드온 0.19.0. **6.0 에서 canvas 렌더러 삭제**(릴리스 노트 #5105) | 알려진 이슈 다수(아래) — 그래도 VS Code 터미널(Chromium + xterm.js + node-pty + ConPTY)이 매일 쓰이는 기준선 | **권장** |
| `ghostty-web` (coder) | 0.4.0 (2025-12-09, 그 뒤 정식판 없음, `next` 태그만), 저장소 활동 2026-10-15, ★3k | MIT | canvas 2D(WebGL 은 요청 이슈 #155 열림) | **작동 안 함**: #119 "Korean (Hangul) IME input not working"(2026-01-25, 열림), 고치는 PR #120 미병합, 조합 위치 PR #190·#202 미병합 | 지금은 제외 |
| `@wterm/ghostty`(vercel-labs/wterm) | 0.5.4 (2026-09-29) | Apache-2.0 | 확인 안 됨 | 확인 안 됨 | 조사만 |
| `ghostty-webgpu` | 0.1.2 (2026-09-28) | MIT | WebGPU | 확인 안 됨 | 아직 이르다 |
| `@aurigax-ai/ghostty-web`(ghostty-web 갈래) | 0.3.5 (2026-10-16) | MIT | 확인 안 됨 | 확인 안 됨 | 조사만 |
| Rust `ghostty-vt`(libghostty-vt 바인딩, Zig 로 빌드) | 0.7.2 (2026-10-14), 내려받기 117 | Apache-2.0 | 화면 없음(백엔드 쪽 VT 상태) | — | 나중 "백엔드가 화면 상태를 쥐는" 단계의 후보로만 |

ghostty 계열은 "Ghostty 의 VT 해석기를 wasm 으로" 라는 장점이 있지만(복잡한 문자·XTPUSHSGR), 이 기능의 1순위 사용자가 한국어 입력을 하므로 IME 가 안 되는 지금은 쓸 수 없다. macOS 판 엔진(libghostty-spm, AppKit 뷰)과 엔진이 달라지는 것은 Tauri 웹뷰라 피할 수 없다.

### 2.2 권장 구성 (정확한 버전 고정 — `^` 금지, package-lock 에 그대로)

- `@xterm/xterm` 6.0.0, `@xterm/addon-webgl` 0.19.0, `@xterm/addon-fit` 0.11.0, 글자 폭: `@xterm/addon-unicode11` 0.9.0 **또는** `@xterm/addon-unicode-graphemes` 0.4.0(ConPTY 의 폭 계산과 맞춰 고른다 — §3.3). 모두 MIT, 2025-12-22 배포.
- 옵션: `windowsPty: { backend: 'conpty', buildNumber }`(6.0 타입 정의에 있음 — `research/xterm-6.0.0.d.ts` 303행), `scrollback` 5,000줄 안팎, `allowTransparency: false`(창이 투명해도 터미널은 불투명 바탕), 글꼴은 `Cascadia Mono, Consolas, D2Coding, monospace` 처럼 한글 폭이 맞는 고정폭을 앞에.
- WebGL 맥락을 잃으면(`onContextLoss`) 애드온을 버리고 DOM 렌더러로 내려간다.
- 출력은 문자열이 아니라 `term.write(Uint8Array)` 로 넣는다 — UTF-8 이 덩어리 경계에서 잘려도 xterm.js 가 이어 푼다.
- 동기화 출력(DEC 2026)이 6.0 에 들어왔다(릴리스 노트 #5453). Claude Code 의 깜빡임 줄이기에 쓰이지만 **ConPTY 가 이 순서열을 그대로 넘기는지는 확인 안 됨**.

### 2.3 한국어 IME — 알려진 이슈와 대응

| 이슈 | 내용 | 우리 환경과의 관계 | 대응 |
|---|---|---|---|
| [#6065](https://github.com/xtermjs/xterm.js/issues/6065) (열림) | **Windows 11 Chromium + Microsoft 한국어 IME** 에서 `keyCode 229` 인 Esc·Ctrl+글자·Ctrl+Space 가 `onData` 에 오지 않음. 고치는 PR [#6066](https://github.com/xtermjs/xterm.js/pull/6066) 미병합 | 정확히 이 환경(WebView2 = Chromium). Claude Code 의 Esc(중단)·Ctrl+C·Ctrl+J 가 한글 모드에서 안 먹는다 | `attachCustomKeyEventHandler` 에서 `keyCode === 229` 이면서 `key === 'Escape'` 이거나 `ctrlKey` 인 keydown 을 잡아 해당 바이트(`\x1b`, `\x03` 등)를 직접 보내고 false. 조합 중이 아닌지 확인 |
| [#6049](https://github.com/xtermjs/xterm.js/issues/6049) (열림) | Windows TSF IME 가 조합 갱신을 textarea 전체 교체로 하면 확정 문자열 앞부분이 잘림(Microsoft Pinyin 으로 확인) | Microsoft 한국어 IME 도 TSF — **해당 여부 확인 안 됨** | 우회가 깔끔하지 않다. #6051·#6060 을 지켜본다. 실기에서 재현되면 helper textarea 를 확정마다 비우는 패치 검토 |
| [#6089](https://github.com/xtermjs/xterm.js/issues/6089) (열림) | 빠르게 칠 때 확정된 조합이 통째로 사라짐(보낼 일정 하나를 공유 불리언으로 막음) | 한글은 음절마다 확정이 잦아 영향이 클 수 있음 — 확인 안 됨 | 실기 시험 항목. 재현되면 PR #6090 성격의 패치를 우리 쪽에 덧대는 것 검토 |
| [#6045](https://github.com/xtermjs/xterm.js/issues/6045) (열림) | 키 겹쳐 누름 + `keyCode 229` 에서 글자 중복·누락 | 확인 안 됨 | 실기 시험 항목 |
| [#6112](https://github.com/xtermjs/xterm.js/issues/6112) (열림) | 앱이 kitty 키보드 프로토콜을 켜면 IME 한 글자 확정이 사라짐 | Claude Code 가 그 프로토콜을 켜는지·ConPTY 가 그 요청을 넘기는지 확인 안 됨 | 실기 시험 항목 |
| [#6161](https://github.com/xtermjs/xterm.js/issues/6161) (열림) | WebGL 렌더러에서 조합 중 글자(preedit)가 칸에 안 맞음 | 표시만 어긋남 | 감수. 심하면 DOM 렌더러 |
| win32-input-mode | xterm.js 구현([#5603](https://github.com/xtermjs/xterm.js/issues/5603))은 2026-01 병합 — **6.0.0 에는 없고 6.1 베타에만**, 그마저 비 US 배열에서 가상 키가 틀림([#6169](https://github.com/xtermjs/xterm.js/issues/6169) 열림) | ConPTY 에 win32-input-mode 를 켜면 안 된다는 근거 | ConPTY 플래그에서 뺀다(§3.3) |

IME 는 CI 러너에서 시험할 수 없다(IME 없음). **구현 전에 실제 Windows 11 + Microsoft 한국어 IME 에서 확인하는 시제품 단계(§9 단계 0)를 둔다.**

그 밖의 키: xterm.js 는 Ctrl+C/V 를 복사·붙여넣기로 쓰지 않는다 — 선택이 있으면 Ctrl+C = 복사, Ctrl+V = 텍스트 붙여넣기로 키 처리기에서 정한다(클립보드는 Tauri 쪽에서 읽는 것이 안전 — 플러그인 필요 여부 확인 안 됨). Claude Code 의 이미지 붙여넣기는 Alt+V 로 Claude Code 가 직접 클립보드를 읽는다고 알고 있다(확인 안 됨). Shift+Enter 줄바꿈은 xterm.js 기본이 Enter 와 같으므로 Claude Code 가 받는 줄바꿈 순서열로 바꿔 보내야 한다 — 어떤 바이트가 맞는지 확인 안 됨(`/terminal-setup` 이 VS Code 에 넣는 값 참고).

### 2.4 성능

- 렌더: WebGL 애드온이 VS Code 의 기본 경로. 보이는 터미널은 하나(세션을 누를 때 바꿈)이므로 WebGL 맥락도 하나로 둘 수 있다 — 안 보이는 터미널은 `Terminal` 객체만 살려 두고 파싱만 하거나(DOM 붙이지 않음), 아예 객체를 만들지 않고 백엔드 고리 버퍼에서 다시 그린다(§3.4). 브라우저의 WebGL 맥락 수 제한(Chromium 은 16개 안팎으로 기억 — 확인 안 됨) 때문에라도 보이는 것 하나만 WebGL 로 둔다.
- 파싱: xterm.js 는 쓰기를 쪼개 처리한다. Claude Code 의 다시 그리기 폭주는 초당 수백 KB 수준으로 본다(확인 안 됨) — 문제는 처리량보다 **숨은 창의 타이머 감속**(§10 위험).

---

## 3. pty (ConPTY)

### 3.1 후보

| 후보 | 버전·갱신 | 라이선스 | Windows | 판단 |
|---|---|---|---|---|
| `portable-pty`(wezterm) | 0.9.0 (2025-02-11), 내려받기 2,150만 | MIT | ConPTY. **플래그를 `INHERIT_CURSOR \| RESIZE_QUIRK \| WIN32_INPUT_MODE` 로 고정**(`src/win/psuedocon.rs` 87~89행), `conpty.dll` 을 **이름만으로** 먼저 찾고 없으면 kernel32(55행), HPCON 을 밖으로 내주지 않음. 의존: winapi 0.3·winreg 0.10·shared_library·lazy_static·anyhow·filedescriptor·serial2 | Windows 에는 쓰지 않음. **macOS 개발 빌드(`cfg(not(windows))`)에만** |
| `tauri-plugin-pty`(Tnze) | 0.3.1 (2026-07-08), ★22, JS 쪽 npm `tauri-pty` 0.3.1 | MIT | portable-pty 를 감쌈. **JS 가 아무 실행 파일이나 띄운다**(`spawn(file, args, …)`), 출력은 4KB 씩 `read` invoke 를 반복(async 명령 안에서 막히는 read) | 제외 — pty 는 백엔드가 쥐어야 한다(짝짓기·일시중지·확인 대화상자·세션 유지) |
| `conpty`(zhiburt) | 0.7.0 (2024-09-23), ★24 | MIT | ConPTY 전용 | 갱신이 멎음. 제외 |
| `winpty-rs`(andfoy) | 1.0.6 (2026-06-10) | MIT OR Apache-2.0 | ConPTY + winpty | 쓸 이유 없음(winpty 불필요). 제외 |
| `alacritty_terminal` | 0.26.0 (2026-04-06) | Apache-2.0 | 자체 ConPTY + 에뮬레이터 + 이벤트 루프 | 에뮬레이터를 백엔드에 두는 설계일 때만 의미. 지금은 과함 |
| **자체 래퍼**(windows-sys 0.61.2) | — | — | node-pty 의 `src/win/conpty.cc` 와 같은 방식 | **권장** |

자체 래퍼를 고르는 이유:
1. **플래그를 정할 수 있다.** VS Code 기준선인 node-pty 는 `inheritCursor ? PSEUDOCONSOLE_INHERIT_CURSOR : 0` 만 넘긴다(`research/node-pty-conpty.cc` 289행). portable-pty 가 늘 켜는 `INHERIT_CURSOR` 는 conhost 가 시작할 때 커서 위치(`ESC[6n`)를 묻고 답을 기다리게 한다 — 보드가 다시 뜰 때 웹뷰 터미널이 생기기 전에 이어 열면 답할 쪽이 없다(얼마나 기다리는지 확인 안 됨). `WIN32_INPUT_MODE` 는 xterm.js 6.0.0 이 그 입력 방식을 못 쓴다(§2.3). `RESIZE_QUIRK`(0x2)는 새 conpty.h 에 없다.
2. **DLL 을 절대 경로로 부른다.** 이름만으로 부르면 기본 검색 순서(PATH 포함)를 탄다.
3. **HPCON 을 쥔다** → sideload conpty.dll 의 `ConptyReparentPseudoConsole(hpc, 보드 창)` 로 가짜 콘솔 창의 소유자를 보드 창으로 정할 수 있다 → hook 의 headless 판별(소유 창이 보이면 false)과 기존 `console::terminal_window` 가 보드 창을 가리킨다.
4. **일시중지 상태로 만들어 Job 에 넣은 뒤 시작**할 수 있다(§4.3).
5. 저장소가 이미 windows-sys 0.61 을 쓰고(`terminal.rs` 의 `CreateProcessW` 직접 호출, `core::console`) Windows 실기 시험 틀이 있다. portable-pty 를 Windows 에 넣으면 winapi 0.3 계열을 따로 끌어온다.

비용: 300줄 안팎의 Win32 코드와 Windows 실기 시험. 알려진 함정 — `ClosePseudoConsole` 은 출력 파이프를 누군가 비우는 동안에만 끝나므로(오래된 Windows) 출력 읽기 스레드를 살려 둔 채 다른 스레드에서 닫는다(확인 안 됨 — node-pty 가 같은 이유로 스레드를 나눈다).

### 3.2 sideload ConPTY (`Microsoft.Windows.Console.ConPTY`)

- NuGet `Microsoft.Windows.Console.ConPTY` **1.25.260930003**(2026-10-02, MIT, `github.com/microsoft/terminal`, "Windows 10.0.17763.0 이상"). 그 위에 1.26 미리보기도 있다. 정식판 하나로 고정한다.
- 패키지 안: `runtimes/win-x64/native/conpty.dll`(119,608바이트), `build/native/runtimes/x64/OpenConsole.exe`(1,089,376바이트), `inc/conpty.h`. x64 판만 싣는다(+약 1.2MB).
- conpty.dll 내보내기(PE 내보내기 표로 확인): `CreatePseudoConsole`·`ResizePseudoConsole`·`ClosePseudoConsole`·`ReleasePseudoConsole`·`ClearPseudoConsole` 과 `Conpty*` 이름들(`ConptyReparentPseudoConsole`·`ConptyShowHidePseudoConsole`·`ConptyClearPseudoConsole`·`ConptyReleasePseudoConsole`·`ConptyCreatePseudoConsoleAsUser`·`ConptyPackPseudoConsole`).
- conpty.dll 은 **자기 파일 옆의 `OpenConsole.exe`**, 없으면 `<x64|arm64|x86>\OpenConsole.exe`, 그것도 없으면 시스템 `conhost.exe` 를 쓴다(`research/winconpty.cpp` 49~95행).
- 새 판이 해석하는 플래그(`winconpty.cpp` 169~187행): `INHERIT_CURSOR`(0x1) → `--inheritcursor`, `AMBIGUOUS_IS_WIDE`(0x20), 글자 폭 `GLYPH_WIDTH_GRAPHEMES`(0x08)·`WCSWIDTH`(0x10)·`CONSOLE`(0x18) → `--textMeasurement …`. 0x2·0x4 는 해석하지 않는다.
- 놓는 자리: node-pty 처럼 `<설치 폴더>\conpty\conpty.dll` + `conpty\OpenConsole.exe`. 불러오기: node-pty 는 `PathCombineW(…, L"conpty\\conpty.dll")` 로 만든 **전체 경로**를 `LoadLibraryW` 로 부르고 `GetProcAddress("ConptyCreatePseudoConsole")` 를 쓴다(`node-pty-conpty.cc` 179~207행). 우리도 `<current_exe 폴더>\conpty\conpty.dll` 전체 경로로 부르고 같은 이름을 쓴다(conpty.dll 이 다른 DLL 을 찾는 순서까지 그 폴더로 하려면 `LoadLibraryExW(…, LOAD_WITH_ALTERED_SEARCH_PATH)` 를 쓸 수 있다 — 이것은 이 계획의 추가 안이고 node-pty 는 하지 않는다). 파일이 없거나 실패하면 `kernel32.dll` 의 `CreatePseudoConsole`, 플래그 0, 다시 붙이기(reparent) 없음.
- 왜 싣는가: 시스템 conhost 는 Windows 판마다 다르다(특히 Windows 10 의 넓은 글자·이모지 처리). 싣는 판은 폭 계산을 고를 수 있어 xterm.js 와 맞출 수 있다. 서명은 Microsoft 서명 바이너리 그대로(확인 안 됨 — 서명 확인을 CI 에 넣는다).

### 3.3 ConPTY 플래그

- 기본: `0`(INHERIT_CURSOR 없음 — 커서 질의 대기를 없앤다. node-pty 기본과 같음).
- sideload 일 때만 글자 폭 플래그를 하나 더한다: **`GLYPH_WIDTH_GRAPHEMES` ↔ `addon-unicode-graphemes`**, 또는 **`GLYPH_WIDTH_WCSWIDTH` ↔ `addon-unicode11`**. 어느 쌍이 한글·이모지·상자 문자에서 Claude Code 입력 상자 커서가 밀리지 않는지 단계 1 의 실기 시험으로 정한다(확인 안 됨). kernel32 로 내려갔을 때는 폭을 고를 수 없으므로 unicode11 을 쓴다.
- `AMBIGUOUS_IS_WIDE` 는 켜지 않는다(xterm.js 기본도 모호 폭 = 1).

### 3.4 바이트 흐름 · 처리량 · 크기 바꾸기

```
claude.exe ──ConPTY── OpenConsole.exe ──출력 파이프──> [읽기 스레드: 늘 읽음]
   → 터미널별 고리 버퍼(예: 4MB, 바이트 순번 붙임)
   → 묶음 스레드: 8~16ms 또는 64KB 마다 Channel<InvokeResponseBody>::send(Raw(덩어리))
   → 웹뷰: term.write(Uint8Array) → 다 처리하면 ack(순번)
입력: xterm onData(string) → invoke("term_write", {id, data}) → 입력 파이프에 UTF-8 로 씀
```

- **읽기 스레드는 웹뷰를 기다리며 막히지 않는다.** 파이프를 비우지 않으면 `claude.exe` 가 출력 쓰기에서 멈춰 멎은 것처럼 보인다. 웹뷰가 늦으면 고리 버퍼에서 오래된 바이트가 밀려난다.
- 웹뷰가 받은 순번과 백엔드의 가장 오래된 순번 사이에 틈이 생기면(밀려남·웹뷰 다시 읽기·창 숨김 뒤) "다시 맞추기": 터미널 초기화(`reset`) → 고리 버퍼 전체를 다시 씀 → `ResizePseudoConsole` 로 같은 크기를 한 번 더 보내 Claude Code 가 화면 전체를 다시 그리게 한다(Ink 는 크기 변경에 다시 그린다 — 확인 안 됨).
- Tauri Channel 의 실제 경로(2.12.4 소스 `crates/tauri/src/ipc/channel.rs` 36~38·293~318행, `research/tauri-channel.rs`): Raw 덩어리가 **1,024바이트 미만이면 eval 로 Uint8Array 리터럴**, 그 이상이면 데이터를 맡겨 두고 웹뷰가 `plugin:__TAURI_CHANNEL__|fetch` 로 가져간다(ArrayBuffer). 메시지마다 순번(index)이 붙는다(`channel.rs` 284행 — JS 쪽이 그 순번으로 순서를 맞추는지는 읽지 않았다, 확인 안 됨. 우리 덩어리에도 바이트 순번을 붙이므로 순서가 뒤바뀌면 다시 맞추기로 잡힌다). 그래서 백엔드에서 묶어 큰 덩어리로 보낸다(초당 수십 번).
- 보이지 않는 터미널: 1안 — 프론트에 `Terminal` 객체를 두되 DOM 에 붙이지 않고 계속 write(상태 유지, 메모리 = 스크롤백). 2안 — 보일 때만 만들고 고리 버퍼로 다시 그림(메모리 적음, 처음 보일 때 다시 그리기 비용). **1안으로 시작**하고 세션 수가 많으면 2안.
- 크기 바꾸기: `addon-fit` 이 칸 수를 계산 → 프론트가 100ms 늦춰(debounce) `invoke("term_resize", {id, cols, rows})` → `ResizePseudoConsole`. 창 펼치기·접기·모니터 배율 변경 때도 같은 경로.
- 대안(쓰지 않음): 로컬 WebSocket 서버 — 토큰·포트 관리가 늘고 Tauri Channel 로 충분하다.

### 3.5 컴파일 확인

`pty-check/`: portable-pty 0.9.0 으로 ConPTY 열기·쓰기·크기 바꾸기·자식 pid, 그리고 windows-sys 0.61.2 의 `CreatePseudoConsole`·`ResizePseudoConsole`·`ClosePseudoConsole`·`CreatePipe`·`InitializeProcThreadAttributeList`·`UpdateProcThreadAttribute`·`PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`·`LoadLibraryExW`·`GetProcAddress` 를 쓰는 코드를 `cargo check --target x86_64-pc-windows-msvc` 로 통과(경고만, 링크·실행은 안 함). 저장소의 Cargo.lock 은 건드리지 않았다.
자체 래퍼에 필요한 windows-sys 기능(src-tauri `Cargo.toml` 에 더할 것): `Win32_System_Console`, `Win32_System_Pipes`, `Win32_System_JobObjects`, `Win32_System_LibraryLoader`, `Win32_Storage_FileSystem`, `Win32_System_IO`, `Win32_Security`(지금 `Win32_Foundation`·`Win32_System_Threading` 등은 이미 있음). `Win32_System_JobObjects` 는 시험 프로젝트에 넣지 않았다 — 단계 1 에서 확인.

---

## 4. 세션 유지

### 4.1 보드가 쥔 세션 목록

`board-settings.json`(SPEC §5.9)에 키 하나를 더한다: `boardTerminals: [{ sessionId, cwd, args, openedAt }]`(args = 마지막으로 띄운 `claude` 뒤 인자 — 표시·진단용, 이어 열 때는 세션 파일의 `resume_args` 를 쓴다). 보드가 터미널을 열 때·세션이 끝나 사람이 터미널을 닫을 때 고쳐 쓴다. 다른 프로그램은 읽지 않으므로 **파일 약속 변경이 아니다**.

### 4.2 다시 뜰 때 이어 열기

1. 보드 시작 → 창을 띄운 **뒤**(웹뷰가 읽혀야 터미널 객체가 생기고, 창이 보여야 hook 의 headless 판별이 맞다) 목록의 세션마다:
2. 세션 파일의 pid 가 아직 살아 있으면(`proc::alive(pid, pid_ctime)`) 이어 열지 않고 "다른 곳에서 돌고 있음"으로 둔다 — 같은 세션을 두 프로세스가 쓰지 않게.
3. 아니면 `claude.exe <resume_args>`(없으면 `--resume <id>`)를 cwd 에서 ConPTY 로 띄운다. **진행 중이던 응답은 이어 하지 않는다**(설계 메모의 결정). 작업 중에 끊긴 세션은 끊기기 전에 `.paused` 표지를 남겨 두면(§4.4) 기존 "재개" 단추로 `carryOnPrompt` 를 넣을 수 있다 — 넣을지 결정 필요.
4. 질문 없이 끝나 대화 기록(jsonl)이 없는 세션은 store 가 세션 파일을 지운다(`store.rs` "질문 없이 끝난 세션은 … 목록에서 뺀다") → 이어 열 수 없으니 목록에서 뺀다.

### 4.3 끝날 때 — Job 객체

- 보드가 띄우는 ConPTY 자식은 `CREATE_SUSPENDED` 로 만들고 보드의 Job 에 넣은 뒤 `ResumeThread`. Job 설정: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK`.
  - KILL_ON_JOB_CLOSE: 보드가 강제 종료되면(업데이트의 `Stop-Process -Force`) `claude.exe` 와 그 자식(도구 셸·MCP 서버)이 확실히 끝난다 → 새 보드의 `--resume` 이 옛 프로세스와 겹치지 않는다. ConPTY 만으로도 conhost 가 끝나며 자식이 끝나는지는 **확인 안 됨**(기억으로는 ClosePseudoConsole 때 붙은 클라이언트도 끝난다고 문서에 있다 — 문서 재확인과, 보드가 강제 종료될 때도 같은지 실기 확인 필요).
  - BREAKAWAY_OK: notify hook 이 알림 패널을 `CREATE_BREAKAWAY_FROM_JOB` 로 먼저 띄운다(`crates/hook/src/spawn.rs`). 허용하지 않으면 패널이 Job 안에 들어가 보드와 함께 죽는다.
  - **주의**: 보드가 쥔 세션 안에서 사람이 `claude --bg` 로 Claude Code 백그라운드 세션(supervisor)을 띄우면 그것도 Job 안이라 보드와 함께 죽는다(breakaway 를 쓰지 않는 한 — 확인 안 됨). 문서는 백그라운드 세션이 터미널보다 오래 산다고 한다. **Job 을 쓸지 결정 필요**(대안: Job 없이 ConPTY 종료에만 맡기고, 새 보드가 이어 열기 전에 옛 pid 가 죽었는지 확인).

### 4.4 업데이트 · 종료 흐름

- 확인을 받는 곳: `updater::run`(업데이트 단추), 트레이 "종료"(`tray.rs` 의 `quit` → 지금은 바로 `app.exit(0)`), `setup`/`uninstall` 이 보드를 끄는 경로는 사람이 고른 일이라 확인 없이.
- 조건: 보드가 쥔 세션 가운데 `working`·`waiting`(일시중지 제외)이 있으면 네이티브 대화상자 — "보드에서 작업 중인 세션 n개가 끊깁니다. 다시 뜨면 이어 엽니다(진행 중이던 응답은 다시 요청해야 합니다)." [업데이트/종료] [취소]. 문구는 계정 전환 확인(`board.rs::switch_account`)과 같은 꼴.
- 확인 뒤: 작업 중 세션에 `.paused` 표지를 남기고(선택, §4.2-3) → 쥔 세션마다 `Esc → /exit → Enter` 를 넣고 최대 3초 동안 세션 상태가 `ended` 가 되기를 기다린다(SessionEnd hook 이 돌고 대화 기록이 닫히게) → 남은 것은 ConPTY 를 닫는다 → 업데이트 스크립트 실행 또는 종료.
- install.ps1 은 고치지 않아도 된다(보드를 끄는 방식 그대로). 단 sideload 파일이 늘었으므로 바꿔 끼울 파일 목록에 `conpty\conpty.dll`·`conpty\OpenConsole.exe` 를 더한다. 실행 중인 OpenConsole.exe 는 보드를 끈 뒤 곧 끝나지만, 남아 있으면 기존 "이름 바꿔 비키기(.old-<시각>)" 방식으로 처리된다.
- Windows 로그오프·종료: 보드가 그냥 죽는다 → 다음 로그인의 `--background` 실행에서 이어 연다.
- 나중 선택지(지금 범위 밖): Claude Code 자체 백그라운드 세션(`claude --bg`/`claude attach <id>`, supervisor 가 터미널과 무관하게 세션을 유지 — `code.claude.com/docs/en/agent-view`, 연구 미리보기, Windows 언급 있음)이 설계 메모의 "자체 pty 서버" 단계를 대신할 수 있다. 백엔드의 "pty 를 직접 연다 / 바이트를 넣어 준다" 바꿔 끼우기 자리에 "attach 클라이언트를 띄운다" 를 하나 더 두면 된다.

---

## 5. 세션 ↔ 터미널 짝짓기

### 5.1 Windows 세션 파일의 pid·tty

- `tty`: 늘 `null` → 쓸 수 없다.
- `pid`/`pid_ctime`: hook 의 조상 8단계 안 첫 `claude.exe` 와 그 생성 시각. 보드가 `claude.exe` 를 ConPTY 에 직접 띄우면 **보통 hook 의 부모가 곧 보드의 자식**이다(hook 은 exec form 이라 셸이 끼지 않는다, SPEC §3.1).
- 그러나 `~/.local/bin/claude.exe` 가 `~/.local/share/claude/versions/<버전>.exe` 를 띄우는 런처라면(보드가 버전을 거기서 읽는다, SPEC §5.10) 세션의 pid 는 자식의 자식이 된다 — **확인 안 됨**. CI 의 가짜 Claude Code 는 node.exe 를 `claude.exe` 로 복사한 것이라 이 경우를 드러내지 못한다 → 사람이 Windows 에서 확인.

### 5.2 방식 (파일 약속 변경 없음)

1. **세션 id 를 미리 안다.** 이어 열기는 `--resume <id>`(Claude Code 문서: `--fork-session` 을 줄 때만 새 id — 즉 기본은 같은 id), 새 세션은 보드가 UUID 를 만들어 `--session-id <uuid>` 로 띄운다. 터미널 기록에 그 id 를 처음부터 붙인다.
2. **pid 로 확인한다.** 그 id 의 세션 파일이 생기거나 바뀌면, 세션 pid 가 보드 자식 pid 와 같거나 `ProcessTable::ancestors(session.pid, 8)` 에 보드 자식 pid 가 있는지(자식의 생성 시각도 일치) 본다. 맞으면 "보드가 쥔 세션". 안 맞으면(같은 id 를 다른 터미널에서 이어 연 경우) 짝짓지 않고 그 터미널은 "다른 곳에서 이 세션이 열림"으로 표시.
3. **세션 파일이 아직 없을 때**(Claude Code 가 막 뜨는 중): 터미널은 왼쪽에 보이되 목록 줄은 SessionStart hook 이 파일을 쓴 다음부터 나온다.
4. **마지막 수단(필요할 때만)**: ConPTY 자식 환경에 `CLAUDE_BOARD_TERMINAL=<터미널 id>` 를 넣어 두는 것은 처음부터 한다(해가 없다, 진단용). hook 이 이 값을 세션 파일에 남기는 것은 **파일 약속 변경**이라(macOS `state.py`·보드 `Sources/ClaudeBoard/`·`board-tui.py`, Windows SPEC §2.1·core 역직렬화·store 모두 고침) 위 1·2 가 실기에서 실패할 때만 한다.

### 5.3 짝지은 세션의 보정 (보드 쪽)

- `headless` 는 세션 파일 값 대신 **false**, `canDrive` 는 "pty 가 살아 있고 세션이 끝나지 않음"으로 정한다(`store.rs::session` 의 계산을 짝 정보로 덮음).
- `probe_in_background` 는 짝지은 세션에 `console::can_attach` 를 하지 않는다 — 보드가 자기 ConPTY 의 콘솔에 `AttachConsole` 하는 것을 피한다(그 사이 보드의 표준 핸들을 바꿨다 되돌리는 코드가 돈다).
- sideload 로 `ConptyReparentPseudoConsole(보드 창)` 을 했다면 hook 쪽 판별도 headless=false 가 되고 콘솔 제목(🔵 아이콘 + 프로젝트 · topic)도 바뀐다 → xterm.js `onTitleChange` 로 받아 터미널 머리줄에 쓸 수 있다(ConPTY 가 제목을 OSC 로 내보낸다 — 확인 안 됨).

---

## 6. 창 배치

### 6.1 동작

- 설정 키 `terminalExpanded`(기본 true). 켜면 창을 **그 창이 있는 모니터의 작업 영역 전체**로 바꾼다: 왼쪽 = 터미널 칸(폭 = 작업 영역 − 454 논리 px), 오른쪽 = 지금 보드(폭 454 = `LAUNCH_WIDTH`). 끄면 `snap_frame`(오른쪽에 폭 454, 높이 전체).
- 보드가 쥔 세션 줄을 한 번 누르면(activate) — 접혀 있으면 펼치고 그 세션의 터미널로 바꾸고 터미널에 키보드 포커스. 두 번 누르기(focus)도 같다. 알림 패널 클릭도 같다(§6.3).
- 쥔 세션이 하나도 없는데 펼쳐 있으면 터미널 칸에 빈 안내("보드에서 연 세션이 여기에 나타납니다" + 세션 다시 열기/새 세션 단추).
- claude 가 끝난 터미널: 그 자리에 "세션이 끝났습니다" 막대 + [다시 열기] [셸 열기] [닫기]. 셸 열기는 cwd 에서 `pwsh.exe`(없으면 `powershell.exe`·`cmd.exe`)를 **아무 명령도 입력하지 않고** 띄운다(설계 메모의 "셸로 남는다"를 셸 해석 없이 맞춘 꼴 — §12 결정).

### 6.2 지금 창 코드에 맞춘 구현 방향

- `window.rs`: `expanded_frame(m: &Mon) -> Frame`(= `m.work`) 추가. `launch_frame` 에 펼침 여부를 받아 펼쳤으면 저장한 위치가 있는 모니터의 작업 영역 전체. 단위 시험은 지금의 `launch_and_snap` 꼴로.
- `board.rs`:
  - `set_terminal_expanded(on)` 명령: 켜면 `set_always_on_top(false)`·`set_frame(expanded)`·(선택) `set_resizable(false)`; 끄면 `pinned` 값으로 항상 위를 되돌리고 `snap_frame`.
  - `save_frame_if_settled`: 펼친 동안은 `window_frame`·`board_restore_frame` 을 저장하지 않는다(펼친 큰 틀이 "⤢ 로 돌아갈 자리"가 되지 않게). 접은 틀만 기억.
  - `enter_mini`: 펼쳐 있으면 먼저 접는다(미니 = 보드만).
  - 전역 단축키 Ctrl+Alt+B: 지금은 "처음 크기로 오른쪽에 붙이고 보이기" — 펼침이 켜져 있으면 펼친 틀로 보이기, 아니면 지금대로.
  - `set_pinned`: 펼친 동안에는 값만 저장하고 창에는 적용하지 않음.
- `+page.svelte`:
  - 바깥을 `flex` 가로 둘로: `<TerminalPane>`(펼쳤을 때만) + `.board`(폭 고정 454px, 접었을 땐 100%).
  - `pickTier()` 는 `window.innerWidth` 대신 **보드 칸 폭**(ResizeObserver)으로.
  - 투명도 효과(`documentElement.style.opacity`)는 펼친 동안 끈다(터미널이 반투명해지지 않게). 접은 상태는 지금대로.
- `TitleBar.svelte`: "터미널 펼치기/접기" 단추(힌트 문구 macOS 판과 맞춤 — macOS 쪽 문구가 정해지면 그대로). 펼친 동안 제목 막대 끌기 영역을 끈다(작업 영역 크기 창을 끌면 화면 밖으로 나감). "처음 크기로 오른쪽에 붙이기" 단추 = 접기.
- 여러 모니터: 창이 있는 모니터 기준(`monitor_of`). 펼친 채 모니터 배율이 바뀌면 `ScaleFactorChanged` 이벤트에서 다시 맞춤.
- 장식 없음·투명 창 + Mica: 터미널 칸은 불투명 바탕을 직접 칠한다. WebView2 의 투명 배경 위 WebGL 캔버스 합성은 문제없을 것으로 보지만 확인 안 됨.

### 6.3 알림 패널 "세션으로 이동"

- hook → 패널 인자에 `--session <sid>` 를 더한다(`crates/core/src/notify.rs` 의 인자 만들기, `crates/hook` 의 notify, SPEC §3.6·§4).
- 패널 클릭(`notify.rs::notify_clicked`): `--session` 이 있으면 `ClaudeBoard.exe --action focus <sid>`(이미 `action_arg` 가 `focus` 를 받는다), 없으면 지금처럼 `--show`.
- 업데이트 사이 판이 섞일 수 있다: 옛 hook 사본 + 새 앱 = `--session` 없음 → `--show`. 새 hook + 옛 앱 = 지금 패널 인자 해석(`notify.rs::parse`, 93행 `_ => {}`)이 **모르는 키를 값과 함께 건너뛰므로 안전**(확인). 그래서 hook 과 앱을 바꾸는 순서에 제약이 없다.
- `--action focus` 의 동작: 보드가 쥔 세션이면 펼치고 그 터미널로, 아니면 지금처럼 그 세션의 터미널 창으로.

---

## 7. 터미널 제어를 pty 쓰기로 바꾸는 범위

`terminal.rs::perform` 에서 세션이 보드가 쥔 것이면 아래로 보내고, 아니면 지금 경로(콘솔 입력·창 찾기·새 콘솔)를 그대로 쓴다.

| 동작 | 지금(SPEC §5.7) | 보드가 쥔 세션 |
|---|---|---|
| 한 번 클릭(activate) | 그 세션 터미널 창 앞으로 | 펼치고 그 터미널로 바꿈(창 포커스는 보드) |
| 두 번 클릭(focus) | 창 + Windows Terminal 탭 고르기(UI Automation) | 같음 + 터미널에 키보드 포커스 |
| 일시중지 | `WriteConsoleInputW` Esc + `.paused` | pty 에 `\x1b` + `.paused` |
| 재개 | 콘솔에 `carryOnPrompt` + Enter, 죽었으면 새 콘솔에서 이어 열기 | pty 에 문구 + `\r`, 죽었으면 **보드 터미널에서** 이어 열기 |
| 중단 | 확인 → Esc → 0.4초 → `/exit` → 0.2초 → Enter, 못 붙으면 프로세스 종료 | 같은 순서를 pty 에. 3초 안에 안 끝나면 자식 종료(`TerminateProcess` 또는 Job 의 그 프로세스) |
| 이어 열기(종료 세션 더블 클릭·"세션 다시 열기") | `CREATE_NEW_CONSOLE` 로 `claude.exe` | **보드 터미널로**(펼침이 꺼져 있으면 켬). 새 콘솔로 여는 옛 동작을 메뉴에 남길지 결정 필요 |
| 자동 전환 이어 가기(`autoswitch.rs`) | 콘솔 입력 Esc → 0.5초 → 문구 → 0.2초 → Enter | 같은 순서를 pty 에 |
| `--action pause\|resume\|stop\|reopen\|focus` | 떠 있는 보드에 넘김 | 같음(위 표대로) |
| 계정 추가(`claude auth login`) | 새 콘솔 | 그대로 둔다(범위 밖) |
| 보드가 쥐지 않은 세션 | — | 지금 그대로. 메뉴에 **"보드로 옮기기"**: canDrive 면 `Esc → /exit → Enter` → pid 가 죽을 때까지(최대 10초) 기다림 → 보드 터미널에서 `--resume`. canDrive 가 아니면 비활성 |

- pty 에 쓰는 Esc 는 단독 `\x1b`. VS Code(xterm.js + node-pty, win32-input-mode 없음)에서 Claude Code 의 Esc 가 동작하므로 conhost 가 단독 Esc 를 Esc 키로 바꾼다고 본다 — **확인 안 됨**, 단계 1 실기 시험. 안 되면 win32-input-mode 순서열(`ESC[27;1;27;1;0;1_` 누름 + 뗌)을 그 키에만 쓴다.
- 문구는 키 하나씩이 아니라 UTF-8 한 번에 쓴다. 괄호 붙여넣기(bracketed paste)는 쓰지 않는다(지금 콘솔 입력도 글자 입력).
- `console::send_input` 등 기존 코드는 그대로 둔다(보드가 쥐지 않은 세션용).

---

## 8. 고칠 파일 목록

### 8.1 명세 `windows/docs/SPEC.md` (먼저 고친다)

- §0 결정 사항: 내장 터미널 엔진·pty·sideload ConPTY.
- §1 디렉터리·§1.2 설치 위치: `conpty\conpty.dll`·`conpty\OpenConsole.exe`.
- §2.1: 파일 약속은 바뀌지 않음을 명시(짝짓기는 세션 id + pid 조상). 마지막 수단을 쓰게 되면 그때 `terminal` 필드 추가.
- §3.6·§4: 패널 인자 `--session <sid>`, 클릭 동작.
- §5.7: 보드가 쥔 세션의 제어 표(§7), "보드로 옮기기", 이어 열기가 보드 터미널로.
- **§5.12(새)**: 내장 터미널 — ConPTY 래퍼(플래그·DLL·Job·reparent), 바이트 흐름·고리 버퍼·다시 맞추기, 쥔 세션 목록·이어 열기·업데이트/종료 확인, 끝난 터미널 막대.
- §5.8: 업데이트 전 확인.
- §5.9: `terminalExpanded`, `boardTerminals`.
- §5.10: 펼침 동작·항상 위·투명도·단축키·틀 저장.
- §6.1: `Snapshot` 에 `terms: Term[]`(`{id, sessionId, cwd, title, state: "starting"|"running"|"exited", exitCode}`)·`expanded`·`activeTerm`, `Session` 에 `held: boolean`. §6.2: 명령 `term_open(cwd, sessionId?, mode: "resume"|"new"|"shell")`·`term_attach(id, channel, fromSeq)`·`term_ack(id, seq)`·`term_write(id, data)`·`term_resize(id, cols, rows)`·`term_close(id)`·`term_select(id)`·`set_terminal_expanded(on)`·`move_to_board(sessionId)`.
- §7: 화면(터미널 칸·끝난 막대·펼침 단추).
- §8.1·§8.3: zip 내용·CI 확인, 제3자 고지.
- §10·§11: 대응표에 새 의무(터미널 엔진·ConPTY·짝짓기·창 펼침·세션 유지) 행, §11.1 검증 결과.

### 8.2 백엔드 `windows/app/src-tauri/`

- `Cargo.toml`: windows-sys 기능 추가(§3.5), `[target.'cfg(not(windows))'.dependencies] portable-pty = "=0.9.0"`(개발 빌드 전용).
- `src/pty/mod.rs`(새): 터미널 뒤쪽을 바꿔 끼우는 자리 — `spawn(exe, args, cwd, env, size) -> PtyHandle`, `write`, `resize`, `close`, `child_pid`·`child_ctime`, 출력 콜백. 설계 메모의 "pty 를 직접 연다 / 바이트를 넣어 준다(나중 pty 서버)" 를 여기서 바꾼다.
- `src/pty/windows.rs`(새): ConPTY 래퍼 + sideload + Job + reparent. `#[cfg(windows)]` 실기 시험(아래 §8.5).
- `src/pty/unix.rs`(새): portable-pty 로 개발용.
- `src/terms.rs`(새): 터미널 목록·고리 버퍼·묶음 보내기·다시 맞추기·짝짓기·쥔 세션 목록 저장·이어 열기·끝남 처리.
- `src/board.rs`: `Board` 에 `terms`, 명령 등록, 업데이트·종료 확인, 펼침, 틀 저장 규칙, `probe_in_background` 에서 쥔 세션 제외, 시작 때 이어 열기(창 띄운 뒤).
- `src/terminal.rs`: `perform` 분기(§7), "보드로 옮기기".
- `src/autoswitch.rs`: 이어 가기 입력을 pty 로(쥔 세션일 때).
- `src/store.rs`·`src/model.rs`: 짝 정보로 `headless`·`can_drive` 보정, `Session.held`, `Snapshot.terms`·`expanded`·`active_term`.
- `src/settings.rs`: `terminal_expanded`(기본 true), `board_terminals`.
- `src/window.rs`: `expanded_frame`, `launch_frame` 펼침 인자, 시험.
- `src/updater.rs`: 실행 전 확인(보드 쪽에서 부르게).
- `src/tray.rs`: "종료" 전 확인(메인 스레드에서 대화상자를 막고 띄우지 않게 — 지금 `confirm` 은 "UI 스레드에서 부르지 않는다").
- `src/notify.rs`: `--session` 받기, 클릭 때 `--action focus`.
- `tauri.windows.conf.json`: `bundle.resources` 로 `conpty/conpty.dll`·`conpty/OpenConsole.exe` 를 설치 폴더에(NSIS 의 자원 배치 경로가 `<설치 폴더>\conpty\` 가 되는지 확인 안 됨 — 안 되면 `externalBin` 방식이나 NSIS 훅에서 복사). 웹뷰 배경 감속을 끌 브라우저 인자(§10) 검토.
- `capabilities/default.json`: 새 앱 명령은 지금 꼴로 열린다. 알림 패널 창(`notify`)에서도 부를 수 있으므로 위험하면 명령 안에서 창 라벨을 확인.
- (빌드 준비물) `binaries/` 와 같은 식으로 CI 가 NuGet 패키지를 받아 `conpty/` 에 풀어 둔다 — 저장소에는 바이너리를 올리지 않는다(`.gitignore` 의 `/*` + `!` 방식 확인).

### 8.3 프론트 `windows/app/`

- `package.json`·`package-lock.json`: `@xterm/xterm` 6.0.0, `@xterm/addon-webgl` 0.19.0, `@xterm/addon-fit` 0.11.0, `@xterm/addon-unicode11` 0.9.0 또는 `@xterm/addon-unicode-graphemes` 0.4.0 — 정확한 버전.
- `src/lib/terminal/TerminalPane.svelte`(새): 칸·빈 안내·끝난 막대·머리줄(제목).
- `src/lib/terminal/term.ts`(새): xterm 만들기·WebGL 과 맥락 잃음 처리·fit·IME 우회 키 처리기·복사/붙여넣기·Shift+Enter·Channel 받기·ack·다시 맞추기.
- `src/lib/types.ts`·`src/lib/backend.ts`·`src/lib/mock.ts`(브라우저 개발용 가짜 터미널 — 받은 글자를 되울림).
- `src/routes/+page.svelte`: 두 칸 배치, 단계 계산을 보드 칸 폭으로, 펼친 동안 투명도 끔.
- `src/lib/board/TitleBar.svelte`: 펼침 단추, 끌기 영역 조건.
- `src/lib/board/SessionRow.svelte`: 쥔 세션 표시(작은 터미널 표지), 메뉴에 "보드로 옮기기"·"보드 터미널 닫기".
- `src/lib/board/GroupCard.svelte`: (결정되면) "이 경로에서 새 세션".
- `src/routes/notify/+page.svelte`: 변경 없음(클릭은 백엔드가 처리).

### 8.4 core·hook `windows/crates/`

- `core/src/notify.rs`: 패널 인자에 `--session <sid>`(+ 단위 시험).
- `hook`: notify 쪽 인자 전달(+ `tests/windows_hook.rs` 의 패널 인자 기대값).
- 짝짓기가 마지막 수단까지 가면 그때만 `core/src/state.rs`(환경 변수 기록)·`tests/fixtures`·macOS `state.py` 등.
- `core/API.md`: 바뀐 공개 API.

### 8.5 CI `windows/ci/`·`.github/workflows/windows.yml`

- 빌드: NuGet ConPTY 1.25.260930003 을 받아 해시 확인 → `app/src-tauri/conpty/` 에 x64 두 파일, zip 묶기에 `conpty\` 폴더, 릴리스 작업의 zip 목록 확인(292행) 고침, Authenticode 서명이 Microsoft 인지 확인.
- `#[cfg(windows)]` 실기 시험(src-tauri): ① ConPTY 로 node.exe 스크립트(가짜 Claude Code — raw 모드로 키를 받아 기록)를 띄워 입력 `\x1b`·`\x03`·한글 UTF-8·문구가 그대로 도착하는지, ② 출력 1MB 를 쏟을 때 읽기 스레드가 막히지 않는지(자식이 끝까지 쓰는지), ③ 크기 바꾸기 뒤 자식이 새 크기를 보는지, ④ sideload 와 kernel32 두 경로, ⑤ Job: 보드 대신 시험 프로세스를 강제 종료하면 자식이 끝나는지, 알림 패널 흉내 프로세스가 breakaway 되는지, ⑥ 등록된 hook 을 ConPTY 안 가짜 `claude.exe` 아래에서 돌려 세션 파일의 pid 가 자식 pid 인지·reparent 뒤 headless 가 false 인지, ⑦ 글자 폭: 한글·이모지·상자 문자를 쓰는 자식의 출력을 xterm 헤드리스로 돌려 커서 위치 비교(가능하면).
- `screen-checks.ps1`(정보용): 보드를 띄워 펼침 상태 스크린샷, 가짜 세션 하나를 보드 터미널로 연 화면.
- `install-checks.ps1`: 설치 폴더에 `conpty\` 두 파일이 생기는지, 재설치·제거 때 지워지는지, install.ps1 업데이트(실행 중 OpenConsole.exe 가 있어도) 성공하는지.

### 8.6 문서 3종

- `windows/CHANGELOG.md`(추가), `windows/README.md`(사용법·macOS 와 다른 점), `windows/docs/설치 가이드.md`(내장 터미널·펼침·보드로 옮기기·업데이트 때 세션이 끊기고 이어 열림·IME 알려진 문제·제3자 구성 요소).
- 제3자 고지: xterm.js(MIT), Microsoft ConPTY/OpenConsole(MIT) — `LICENSE` 옆 고지 파일을 zip·setup.exe 에 넣는다.

---

## 9. 단계별 순서

각 단계는 "확인 방법"이 통과해야 다음으로 간다.

0. **실기 시제품(가장 먼저, 사람)** — 이 Mac 이 아닌 Windows 11 + Microsoft 한국어 IME 에서, 별도 작은 Tauri 시제품(저장소 밖)에 xterm.js 6.0.0 + 최소 ConPTY 로 진짜 Claude Code 를 띄운다.
   → 확인: 한글 입력(빠르게·길게), 한글 모드에서 Esc·Ctrl+C·Ctrl+J, Shift+Enter, 붙여넣기, 창 크기 바꾸기 뒤 입력 상자 커서, hook 세션 파일의 pid 가 ConPTY 자식 pid 인지(런처 여부).
   이 단계를 돌릴지는 사용자가 정한다(이 계획은 구현하지 않는다). IME 가 쓸 수 없는 수준이면 내려가는 순서: ① 같은 xterm.js 6.0.0 에서 DOM 렌더러(조합 위치 #6161 이 빠짐) + 키 처리기 우회 보강 → ② #6066·#6060 이 병합된 6.1 베타를 정확한 버전으로 고정 → ③ 그래도 안 되면 ghostty-web 의 한국어 IME 수정(#120)이 병합됐는지 다시 본다.
1. **명세** — 이 계획 §8.1 의 목록대로 SPEC 을 고치고 대응표에 새 의무 행. → 확인: 검토.
2. **pty 래퍼 + CI 실기 시험** — `pty/windows.rs`·`unix.rs`, CI 의 NuGet 받기. → 확인: §8.5 의 ①~⑥ Windows 러너 통과, macOS 에서 `cargo test`.
3. **터미널 관리 + 바이트 흐름** — `terms.rs`, 명령, Channel, 고리 버퍼·다시 맞추기. → 확인: 단위 시험(고리 버퍼 순번·틈 감지·묶음), macOS `npm run tauri dev` 에서 진짜 셸/claude 로 출력 폭주·창 숨겼다 보이기.
4. **화면** — `TerminalPane`·`term.ts`·두 칸 배치·단계 계산·투명도. → 확인: macOS 실행 스크린샷(세 단계 × 펼침/접힘), 브라우저 mock.
5. **창 펼치기** — `window.rs`·`board.rs` 의 펼침·항상 위·틀 저장·미니·단축키. → 확인: `window.rs` 단위 시험(여러 모니터·배율), Windows 실기(사람).
6. **짝짓기·보정** — 세션 id + pid 조상, headless·canDrive 덮기, probe 제외. → 확인: 가짜 세션 폴더 단위 시험, §8.5 ⑥.
7. **제어 전환** — `perform` 분기, 자동 전환, 보드로 옮기기, 이어 열기를 보드 터미널로. → 확인: §8.5 ① 의 가짜 Claude Code 로 일시중지·재개·중단 키 순서 도착.
8. **세션 유지** — 목록 저장, 시작 때 이어 열기(창 뒤), 업데이트·종료 확인과 `/exit` 기다림, Job. → 확인: CI 에서 보드 강제 종료 → 다시 실행 → 가짜 세션이 `--resume <id>` 로 다시 뜸.
9. **알림 패널 이동** — core·hook·패널. → 확인: 단위 시험 + smoke 의 패널 인자.
10. **배포** — zip·setup.exe·install.ps1·CI 확인·제3자 고지. → 확인: CI build·smoke·release dry run.
11. **문서 3종 + 사람 확인 목록** — SPEC §10 "사람이 Windows 에서 확인해야 함"에 IME·실제 Claude Code·펼침·업데이트 뒤 이어 열기 추가.

---

## 10. 위험 요소

| 위험 | 영향 | 줄이는 방법 |
|---|---|---|
| **한국어 IME**(#6065 Esc·Ctrl 누락, #6049·#6089 확정 글자 잘림·누락) | 기능의 핵심 사용감. CI 로 못 잡음 | 단계 0 실기 시제품, 키 처리기 우회, 이슈 추적, 심하면 6.1 베타의 수정 반영 검토 |
| **ConPTY 플래그·글자 폭** | 한글·이모지에서 커서 밀림(Claude Code 입력 상자 깨짐), 시작 때 커서 질의 대기 | INHERIT_CURSOR·WIN32_INPUT_MODE 빼기, sideload 로 폭 계산을 xterm 애드온과 맞추기, CI 비교 시험 |
| **pid 짝짓기**(런처 실행 파일이면 pid 가 자식의 자식) | 보드가 쥔 세션을 못 알아봄 | 세션 id 를 미리 정하고 pid 는 조상으로 확인, 최후 수단 환경 변수 + hook 기록 |
| 숨은 창의 웹뷰 감속(Chromium 은 숨은 페이지 타이머를 늦춤) | 터미널 화면이 늦게 따라옴. 백엔드가 늘 비우므로 Claude Code 가 멈추지는 않음 | 고리 버퍼 + 보일 때 다시 맞추기. 필요하면 WebView2 브라우저 인자로 감속 끄기(Tauri `additional_browser_args` — 기본 인자를 함께 넣어야 하고 효과는 확인 안 됨) |
| 업데이트·종료 때 진행 중 응답이 끊김 | 사용자가 받아들인 결정 | 확인 대화상자, `/exit` 기다림, `.paused` 로 재개 단추 |
| Job 이 `claude --bg` 백그라운드 세션까지 죽임 | 사람이 보드 안에서 띄운 백그라운드 세션이 보드와 함께 끝남 | Job 사용 여부 결정, 쓰면 문서에 적음 |
| 항상 위 + 작업 영역 크기 창 | 다른 앱을 모두 가림 | 펼친 동안 항상 위 끔 |
| WebGL 맥락 잃음(GPU 드라이버·원격 데스크톱) | 터미널이 비어 보임 | DOM 렌더러로 내려감 |
| sideload 파일 배포 | 설치·업데이트·제거 경로가 하나 늘고 실행 중 OpenConsole.exe 가 파일을 잡음 | install.ps1 의 이름 바꿔 비키기, CI 설치 확인 |
| 새 명령이 알림 패널 웹뷰에서도 불림 | 로컬 화면뿐이라 낮음 | 명령에서 창 라벨 확인 |
| 터미널 여러 개의 메모리 | 스크롤백 × 세션 수 | 스크롤백 상한, 안 보이는 터미널은 나중에 버퍼만 두는 방식 |
| 옛 hook + 새 앱 / 새 hook + 옛 앱 | 패널 인자 해석 | `--session` 은 없어도 되는 인자. 옛 앱은 모르는 키를 건너뜀(`notify.rs::parse` 확인) |

---

## 11. macOS 판과 달라지는 점

| 항목 | macOS(설계 메모) | Windows |
|---|---|---|
| 엔진 | libghostty-spm(GhosttyKit xcframework + AppKit 뷰·입력·IME), 네이티브 | xterm.js 6.0.0 + WebGL, 웹뷰 안 |
| pty | 엔진이 직접 열 수 있음(설계 메모의 첫 방식) | 백엔드가 ConPTY 를 열고 바이트를 웹뷰에 넣어 준다(두 번째 방식만) |
| 실행 | 셸을 띄우고 첫 입력으로 명령 → claude 가 끝나도 셸로 남음 | 셸 없이 `claude.exe` 를 인자 배열로 직접(SPEC §5.7 명령 주입 방지). 끝나면 [다시 열기][셸 열기] 막대 |
| 짝짓기 | 1순위 엔진이 알려 주는 자식 pid·tty ↔ 세션 파일 pid·tty | tty 없음 → 세션 id 를 미리 정하고 pid 조상으로 확인 |
| 끝날 때 | 엔진 종료 | Job 객체(결정 필요)와 ConPTY 닫기 |
| headless | tty 유무 | 콘솔 창 소유자(reparent)·조상 창 — 보드 쪽 보정 필요 |
| 창 | AppKit 창 | 장식 없는 투명 Tauri 창 — 항상 위·투명도·끌기를 펼친 동안 끔 |
| 배포 | 앱 번들 안 xcframework | `conpty\conpty.dll`·`conpty\OpenConsole.exe` 를 설치 폴더에 |
| IME | 엔진(GhosttyTerminal)이 처리 | 웹뷰 + xterm.js 의 IME 처리(알려진 이슈) |

---

## 12. 결정이 필요한 것 (사용자)

1. **셸을 거치지 않는 실행**: 설계 메모의 "셸 + 첫 입력으로 명령" 대신 `claude.exe` 직접 실행으로 할지(권장). 끝난 뒤 막대에서 [셸 열기] 를 누르면 cwd 에서 셸을 띄우는 것으로 "셸로 남는다"를 대신.
2. **Job 객체**: 보드가 강제 종료될 때 자식을 확실히 끝내는 대신, 보드 안에서 띄운 `claude --bg` 백그라운드 세션도 함께 끝나는 것을 받아들일지.
3. **이어 열기의 기본 자리**: 종료 세션 더블 클릭·"세션 다시 열기"를 늘 보드 터미널로 할지, 새 콘솔로 여는 옛 동작을 메뉴에 남길지.
4. **새 세션 열기**: 설계 메모에 직접 없는 "이 경로에서 새 세션"(그룹 카드 메뉴, `--session-id <uuid>`)을 넣을지 — 없으면 보드 터미널은 이어 열기·보드로 옮기기로만 생긴다.
5. **끊긴 작업 세션의 재개 표지**: 업데이트·종료로 끊긴 working 세션에 `.paused` 를 남겨 다시 뜬 뒤 "재개" 단추로 이어 가게 할지.
6. **sideload ConPTY 를 실을지**: 싣지 않으면 파일·배포가 단순해지지만 글자 폭을 고를 수 없고 reparent 도 못 한다(kernel32 판은 Windows 판마다 다름).

---

## 13. 출처 (조회일 2026-10-17)

- 레지스트리(crates.io API·npm registry·NuGet API): portable-pty 0.9.0 / tauri-plugin-pty 0.3.1 / conpty 0.7.0 / winpty-rs 1.0.6 / alacritty_terminal 0.26.0 / ghostty-vt 0.7.2 / tauri 2.12.4 / windows-sys 0.61.2 / @xterm/xterm 6.0.0 / @xterm/addon-webgl 0.19.0 / @xterm/addon-fit 0.11.0 / @xterm/addon-unicode11 0.9.0 / @xterm/addon-unicode-graphemes 0.4.0 / @xterm/addon-canvas 0.7.0(2024, 6.0 에서 삭제) / ghostty-web 0.4.0 / @wterm/ghostty 0.5.4 / ghostty-webgpu 0.1.2 / @aurigax-ai/ghostty-web 0.3.5 / tauri-pty 0.3.1 / node-pty 1.1.0 / Microsoft.Windows.Console.ConPTY 1.25.260930003.
- GitHub(gh api): xtermjs/xterm.js(★21,274, 2026-10-13), coder/ghostty-web(★2,976, 2026-10-15), Tnze/tauri-plugin-pty(★22), wezterm/wezterm, microsoft/terminal `src/winconpty/winconpty.cpp`, microsoft/node-pty `src/win/conpty.cc`, tauri-apps/tauri `crates/tauri/src/ipc/channel.rs`(태그 tauri-v2.12.4), xterm.js 6.0.0 릴리스 노트·`typings/xterm.d.ts`, 이슈 xterm.js #6065 #6066 #6049 #6089 #6045 #6112 #6161 #5603 #6169, ghostty-web #119 #120 #155 #190 #202.
- 문서: `code.claude.com/docs/en/cli-reference`(`--resume`·`--fork-session`·`--session-id`), `code.claude.com/docs/en/agent-view`(백그라운드 세션·supervisor).
- 로컬: portable-pty 0.9.0 crate 소스(`~/.cargo/registry/src/…/portable-pty-0.9.0/src/win/psuedocon.rs`), NuGet 패키지의 `inc/conpty.h`·PE 내보내기 표(llvm-objdump), `pty-check/` cargo check 결과.
