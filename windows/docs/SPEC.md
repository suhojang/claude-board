# Claude Board for Windows — 역분석 명세와 재구현 의무 대응표

macOS 판(저장소 최상위의 Swift·Python 소스, 커밋 `68ffed2`)을 역분석해 Windows 판이 지켜야 할 동작을 정리한 문서다.
Windows 판 개발자(사람·에이전트)는 이 문서를 단일 기준으로 삼는다. macOS 소스의 줄 번호는 `68ffed2` 기준이다.

## 0. 결정 사항

| 항목 | 결정 |
|---|---|
| 위치 | 저장소의 `windows/` 아래에만 둔다. 최상위의 macOS 파일은 고치지 않는다(문서 링크·`.gitignore`·`CLAUDE.md` 제외) |
| 기술 | Rust. 보드·알림 패널은 Tauri 2(프론트엔드 Svelte 5 + TypeScript + Vite), hook·명령줄 도구는 순수 Rust 실행 파일 |
| 버전 | macOS 와 같은 번호를 쓴다. 첫 판은 `2.12`. 버전은 `windows/VERSION` 한 곳에 두고 빌드가 읽는다 |
| 릴리스 | 태그 `windows-v<버전>`, GitHub 릴리스는 **절대 Latest 로 표시하지 않는다**(`--latest=false`, API `make_latest: "false"`) |
| 범위 | 보드·알림 패널·hook·사용 한도·업데이트·트레이 + 세션 터미널 제어 + 계정 전환·자동 전환 + 캐릭터 8종 전체 + TUI |
| 문서 | 문서·코드 주석·커밋 메시지는 한국어(저장소 관례) |

### 0.1 Latest 를 쓰지 않는 이유 (어기면 기존 macOS 사용자가 깨진다)

설치된 모든 macOS 앱이 `https://github.com/suhojang/claude-board/releases/latest/download/VERSION`·`ClaudeBoard.zip`·`install.sh` 를 읽는다
(`ClaudeBoard.swift:4101-4132`, `packaging/install.sh`). Windows 릴리스가 한 번이라도 Latest 가 되면 macOS 업데이트 확인·설치가 404 나 엉뚱한 버전을 받는다.
그래서 Windows 판은 `/releases/latest/` 를 쓰지 않고 **릴리스 목록 API 에서 `windows-v` 로 시작하는 태그를 골라** 쓴다.
macOS 의 `packaging/release.sh` 는 최상위 `CHANGELOG.md` 를 `## <버전>` 으로 찾으므로, Windows 변경 내역은 `windows/CHANGELOG.md` 에 따로 쓴다.

## 1. 디렉터리와 산출물

```
windows/
  VERSION                     2.12 — 버전의 단일 출처
  CHANGELOG.md                Windows 판 변경 내역. "## <버전> — <날짜>" 항목이 릴리스 설명이 된다
  README.md                   Windows 판 소개·설치·사용법(사용자용)
  docs/SPEC.md                이 문서
  docs/설치 가이드.md          packaging/README.txt 에 해당 — 모든 기능의 자세한 설명
  Cargo.toml                  워크스페이스: crates/core, crates/hook (Cargo.lock 은 windows/ 에)
  crates/core/                claude-board-core — 파일 약속·hook 상태 규칙·hook 등록·경로·Windows 프로세스 도구(라이브러리)
  crates/hook/                claude-board-hook — hook·설치·TUI 명령줄 실행 파일
  app/                        Tauri 앱(프론트엔드 package.json·src/ 는 여기)
  app/src-tauri/              Tauri 백엔드(별도 Cargo.lock) — 보드 상태 읽기·사용 한도·계정·업데이트·터미널 제어·트레이·창
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
| `%USERPROFILE%\.claude\notifier\` | macOS 와 같은 폴더. `claude-board-hook.exe`(hook 이 실행하는 사본), `app-path`(ClaudeBoard.exe 전체 경로 한 줄), `sessions\`, `mute-all`, `notify-position`, `accounts.json`, `accounts\`, `update.log`, `auto-switch.log`, `board.log`, `board-theme`, `context\`(사용자 상태줄이 씀), `context-windows.json` |
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
| `since` | state 가 마지막으로 바뀐 시각(초, 소수) | 같음 |
| `updated` | 마지막 hook 시각 | 같음 |
| `started` | 프로세스 정보를 새로 잡은 시각 | 같음 |
| `pid` | 조상 가운데 Claude Code 프로세스 pid | `claude.exe`(또는 명령줄에 `claude` 가 든 `node.exe`/`bun.exe`) |
| `pid_ctime` | — | **Windows 전용 추가**: 그 프로세스의 생성 시각(FILETIME 을 유닉스 초로). pid 재사용을 가려낸다. 없으면 pid 만 본다 |
| `tty` | 터미널 장치 이름 | Windows 에는 없다 → 늘 `null` |
| `headless` | 터미널 없이 도는 세션 | §3.4 의 판별 규칙 |
| `args` | 다시 열 때 넘길 실행 인자(`carry_args`) | 명령줄을 `NtQueryInformationProcess(ProcessCommandLineInformation)` 로 읽어 `CommandLineToArgvW` 로 나눈 뒤 같은 규칙 |
| `transcript_path`, `permission_mode`, `model` | hook 입력에서 | 같음(서브에이전트 이벤트는 permission_mode·model 을 바꾸지 않는다) |
| `topic` | 마지막 요청 문구 — 공백을 한 칸으로 줄이고 앞 **40 코드포인트**, `<` 로 시작하는 요청은 무시 | 같음 |
| `last_message` | idle 이면 `last_assistant_message`(공백 정리, 200 코드포인트), waiting 이면 `message` 앞 200(공백 정리 없음) | 같음 |
| `subagents` | `{agent_id: {type, description?, background?, started, state(running\|waiting\|done), tool?, updated, ended?, last_message?}}` | 같음 |
| `state_before_sub_wait` | 서브에이전트 승인 대기 전 상태 | 같음 |
| `limit_hit` | `{at, message}` — 한도에 걸려 끊김 | 같음 |
| `resume_cmd` | 다시 여는 명령 문자열 | **표시용**. Windows 명령줄 규칙으로 인용(예: `claude --model claude-opus-5-5 --resume <id>`). 이 문자열을 셸(cmd·PowerShell)로 실행하지 않는다 |
| `resume_args` | — | **Windows 전용 추가**: `claude` 뒤에 붙일 인자 배열(`[..., "--resume", "<id>"]`). 보드는 이 배열로 `claude.exe` 를 직접 실행한다 — 모델 이름 같은 값은 저장소의 설정 파일에서 올 수 있어 셸 해석을 거치면 명령 주입이 된다 |

쓰기: 같은 폴더에 `<sid>.<임의>.tmp` 로 쓰고 바꿔 넣는다(Windows 는 `MoveFileExW(REPLACE_EXISTING)`, 공유 위반이면 50ms 간격 5번 다시). 읽고 고쳐 쓰는 동안 `.<sid>.lock` 을 `LockFileEx` 배타 잠금(macOS `flock`).

### 2.2 표지 파일

| 파일 | 쓰는 쪽 → 읽는 쪽 | 뜻 |
|---|---|---|
| `sessions/<sid>.mute` | 보드 → notify | 그 세션 알림 끔 |
| `mute-all` | 보드 → notify | 전체 알림 끔(세션별 표지보다 먼저 본다) |
| `notify-position` | 보드 → notify | `top-left`·`top-right`·`bottom-left`·`bottom-right`. 없으면 가운데(`center` 를 고르면 파일을 지운다) |
| `sessions/<sid>.paused` | 보드 → state hook | 보드에서 일시중지. 내용 `{"paused_at":…, "topic":…}`. `UserPromptSubmit` 이 오면 hook 이 지운다 |
| `app-path` | setup → notify | **Windows 전용**: ClaudeBoard.exe 전체 경로 |
| `context/<sid>.json` | 사용자 상태줄 → 보드 | `{context_window:{context_window_size, current_usage:{input_tokens,cache_creation_input_tokens,cache_read_input_tokens}, used_percentage}, model, effort, at}` |
| `%TEMP%\.claude-usage-cache.json` | 보드(·상태줄) | 사용량 API 응답 캐시(macOS 는 `/tmp/.claude-usage-cache.json`) |

### 2.3 자율주행 실행 폴더 (macOS 먼저, Windows 는 같은 약속으로 뒤에)

자리는 `~/.claude/notifier/` 가 아니다 — macOS 는 `~/Library/Application Support/Claude Board/autopilot/runs/<실행 id>/`(시험 빌드 `Claude Board Dev`), 엔진 전용 폴더는 같은 뿌리의 `private/<실행 id>/`. 모델 쪽 읽기 금지는 엔진 전용 뿌리 전체(실제·시험 판), 모든 실행의 `engine/`, 옛 뿌리까지다(다른 실행의 백업을 읽지 못하게). Claude Code 가 `~/.claude` 를 보호 경로로 두어 모델의 Write·Edit 도구가 단위 폴더에 쓰지 못해서 옮겼다(2026-10-20). Windows 는 `%LOCALAPPDATA%\Claude Board\autopilot\` 같은 사용자 앱 데이터 자리에 둔다.

실행을 넘어 쌓이는 기록(지난 실행, 단위별 추정·실제, 답한 카드, 프로젝트 대기열, 실행마다 쓴 자율주행 세트와 하네스 지문)은 같은 뿌리의 `private/records.sqlite` 하나에 두기로 했다(2026-10-21. 2026-10-22 에 macOS 엔진 고침에서 만들기로 당겼고, 표는 `docs/자율주행/설계.md` 5장 7절). 엔진들이 쓰고 보드는 읽기 전용으로 열며 모델은 읽지 못한다. 바깥 서비스 연결(macOS 4단계, `docs/자율주행/설계.md` 5장 6절)도 같은 약속이다 — 쓰기는 보드 내장 REST 만, 읽기는 내장과 사용자 MCP 읽기 세션이고 결과는 메타 형식 파일로 근거 묶음에 넣는다.
macOS 는 1단계(엔진과 상태 파일, `ClaudeAutopilot.swift` → `claude-autopilot`)와 엔진 고침(지시 얹기·하네스 지문·기록 저장소, 2차에 가지·`finish`·`prerun`·Jira 가지, 셋째에 따로 작업 트리·보안 검토 둘째·Jira 가지 꼴 `{type}`, 2026-10-22)까지 있다. 필드·상태 값·규칙의 기준은 `docs/자율주행/설계.md` 4장(엔진)·5장(파일 약속)이고, 바꾸면 그 문서와 이 절을 함께 고친다. Windows 판은 아직 만들지 않는다.

| 파일 | 쓰는 쪽 → 읽는 쪽 | 뜻 |
|---|---|---|
| `run.json` | 출발 화면(설정) → 엔진(상태) → 보드·TUI | 설정(`model`·`effort`·`stopline`·`caps`·`check_timeout_min`·`check_retries`·`precheck_retry_min`·`background_quiet_sec`·`allowed_tools`·`disallowed_tools`·`write_roots`·`network_allow`·`local_binding`·`env_pass`·`unix_sockets`·`read_deny`·`sandbox_exclude`·`project`(자율주행 세트 자리와 기록 저장소의 프로젝트 이름)·`project_root`·`base_branch`(출발 가지: 이름 하나 또는 {저장소 경로: 가지}, 없으면 지금 가지)·`worktree_mode`(worktree·in_place 강제)·`worktree_files`(따로 작업 트리에 함께 둘 git 무시 파일))과 상태(`state`: `starting`·`running`·`waiting_limit`·`waiting_stopline`·`waiting_precheck`·`paused`·`stopped`·`done`·`error`, `reason`, `unit`, `unit_caps`, `wait_until`, `engine_pid`, `sessions`, `usage`, `harness`, 얹은 세트 요약 `set`, 끝 정리 시각 `wrapped`, 기록 저장소 쓰기 실패 `records_error`, 어디서 `worktree`, 가지 `branches`(저장소마다 `base`·`work`·`original`·`restored`·`restore_note`·`on`·`made`, `start`·`end`·`moved`·`created`, Jira `stories`, 끝난 뒤 `promoted`·`discarded`), 시각들). 설정 칸은 출발 때 `pinned/frozen.json` 으로 고정되고 엔진은 그것만 읽는다 |
| `queue.json` | 엔진(출발 때 동결) → 보드 | 단위 배열: `id`·`title`·`prompt`·`check`(종료 코드 0 이 통과)·`precheck`(선택)·`action`(선택, 엔진이 모델 전에 돌리는 고정 명령)·`check_timeout_min`·`action_timeout_min`(선택)·`context`(선택, 근거 묶음 폴더 — 출발 때 `pinned/context/<n>/` 로 사본, 단위 시작 때 `units/<n>/context/` 로 읽기 전용)·`cwd`·`caps{minutes,usd,weekly_pp}`·`review`·`tickets`(배열)·`story`·`branch`(Jira 연결 프로젝트의 스토리 키와 작업 가지, 2026-10-22)·`depends`(선택, 기대는 앞선 단위 id 배열)·`prerun`(선택, 기본 true, 판정 미리 돌리기에 넣을지)·`source_key`·`kind`·`est_min`·`est_basis`(선택, 기록 저장소에 남길 원천 항목 키·종류·출발 때 예상)·`status`(`pending`·`running`·`done`·`failed`·`capped`·`stopped`·`skipped`) |
| `cards.json`, `answers.json` | 출발 화면 → 엔진·모델 | 출발 카드와 고른 답 |
| `prerun.json`, `prerun/<n>/` | 엔진(`prerun`, 출발 전) → 가져오기·보드 | 판정 미리 돌리기 결과: 단위마다 돌렸는지·종료 코드·통과, 일하기 전부터 통과한 단위 `already_passing` 과 출발 카드 감 `cards`(빼기·판정 고치기·그대로), 판정 출력과 증거 파일 |
| `units/<n>/result.json` | 모델 → 엔진·보드 | 단위 결과 기록(모델이 쓰는 유일한 결과물) |
| `units/<n>/engine.json`, `stream.jsonl`, `stderr.log`, `evidence/check-<k>/` | 엔진 → 보드 | 단위 상태·시계·비용·판정·상한·세션 id·프로세스(엔진 비공개 `engine/unit-<n>.json` 의 사본), claude stream 원문, 판정 출력과 증거 파일 |
| `events.jsonl`, `context.jsonl` | 엔진 → 보드·TUI | 쉬운 말 진행 기록 `{ts, who: board\|model\|engine, text}`, 도구 호출 원문 요약 |
| `control.json` | 보드 → 엔진 | `{action: pause\|resume\|stop\|takeover\|giveback\|restart_from_intake, ts}`. 엔진이 받으면 지운다 |
| `inbox/`, `outbox/` | 보드 ↔ 엔진 | 사람 지시, 올리지 않은 바깥 글(2·4단계) |
| `pinned/` | 엔진(출발 때) | 고정한 엔진·플러그인 사본과 고정 설정 `frozen.json`(해시는 다시 띄우는 쪽의 인자로만), 프로젝트 자율주행 세트 사본 `set/`(지시 `autopilot.md`, 주제로 시작이면 명세 `spec.md`·기술 스택 `stack.json`), 판정 스크립트·판정 시나리오 사본 `checks/`(판정은 이 고정본으로), 서브에이전트가 뜰 때 공통 규칙과 지시를 넣는 고정 플러그인 `plugin/`(`SubagentStart` hook, macOS 엔진 고침 1차) — 다시 띄우는 쪽은 이 사본을 실행하고, 해시가 다르면 `error` |
| `engine/` | 엔진 | 엔진 비공개 상태·샌드박스 설정(모델의 쓰기 범위 밖) |
| (실행 폴더 밖) `private/<실행 id>/` | 엔진이 돌리는 명령 | 비밀이 든 것(복원 전 백업 등), 출발 때 뜬 사용자 하네스 사본 `harness/`. 모델은 읽기도 금지 |
| (실행 폴더 밖) `private/projects/<프로젝트>/set/` | 보드·가져오기 세션·초안 세션 → 엔진(출발 때 사본) | 프로젝트의 지금 자율주행 세트: 지시 `autopilot.md`, 판정 스크립트·판정 시나리오 `checks/`, 기준 하네스 지문 `harness.json`, 작업 트리에 함께 둘 파일 `worktree_files.json`, 주제로 시작이면 명세 `spec.md`(요구사항·수용 기준)와 기술 스택 `stack.json`(`stack` 칸: 프론트·백엔드·DB 칸마다 이름·버전·빌드 도구와 어디서 왔는지) |
| (실행 폴더 밖) `projects/<프로젝트>.json` | 엔진·보드 → 보드·엔진 | 프로젝트 설정: 정책 카드 답, 보고서 받을 곳, 승격 차례 `promotion`(단계 목록 `[{from, into, after}]`, `from` 이 `autopilot` 이면 그 실행의 자율주행 가지), 지난 출발 가지 `last_base`(`{<저장소>: <가지>}`), Jira 연결 프로젝트의 티켓·가지 규칙 `tickets`(`epic`·`task_type`·`branch_names{story, task}`·`merge`, 2026-10-22) |
| `starts.log`, `engine.log` | 엔진 | 뜬 시각(10분 안에 세 번 비정상 시작이면 `error`), 엔진 출력 |

엔진이 띄우는 claude(와 판정 명령)에는 `CLAUDE_BOARD_AUTOPILOT`(실행 id)·`CLAUDE_BOARD_AUTOPILOT_DIR`(실행 폴더)·`CLAUDE_BOARD_UNIT_DIR`(단위 폴더)를 붙인다. hook 은 이 변수가 있을 때만 자율주행 규칙을 쓴다(macOS 도 아직 hook 은 그대로). `sessions/<sid>.json` 의 필드는 바꾸지 않는다.
사용자 하네스(작업 저장소의 `CLAUDE.md`·`.claude/`·`.mcp.json`)는 출발 준비·실행·끝 어디서도 고치지 않는다(2026-10-21). 자율주행 지시는 보드의 프로젝트 자율주행 세트로 갖고 엔진이 시스템 프롬프트로 자율주행 세션에만 얹는다. 엔진은 출발 때 하네스 사본과 지문을 남기고 끝날 때 다시 재어 `run.json` 의 `harness`(`base`·`files`·`folders`, 끝에 `same`·`changed`·`checked`·`cleaned`)에 적으며, 출발 때 없던 빈 `.claude/.cc-writes` 를 치운다. 세트는 실행마다 기록 저장소에 남고 같은 경로에서 다시 출발하면 불러와 고쳐 쓴다(`docs/자율주행/설계.md` 2장 끝).
어디서 도는가(2026-10-21, 가지 전략은 같은 날 저녁에 바꿈): 사람이 출발 확인에서 저장소마다 출발 가지를 고르고, 두 모드 모두 그 가지에서 `autopilot/<실행 id>` 를 만들어 거기에만 커밋한다(기존 가지 ref·`packed-refs` 는 쓰기 금지, 단위 칸 `work_branches` 는 없앰). 단일 저장소이고 확인(한 저장소, submodule 없음, 형제 경로 없음, 작업 트리에서 환경 확인 통과)을 통과하면 그 가지의 따로 작업 트리(macOS `~/.claude-board/worktrees/<실행 id>/<저장소 이름>/`, 경로에 공백 없이)에서 돌고(macOS 엔진 고침 셋째에 구현), 아니면 원래 작업 트리를 그 가지로 바꿔(`git switch -c`) 돌며 보드가 "자율주행 중"을 표시하고, 끝나면 작업 트리가 깨끗할 때만 출발 때 걸려 있던 가지로 되돌린다. `worktree_mode`(run.json·프로젝트 설정)로 `worktree`·`in_place` 강제, `worktree_files` 로 작업 트리에 함께 둘 git 무시 파일. `run.json` 의 `worktree`(`mode`·`reason`·`repo`·`path`·`branch`·`base`)와 `branches`(`repos` 아래 저장소마다 `base{branch, head}`·`work`·`original`·`restored`, 따로 작업 트리면 `work_tree`·`work_git_dir`, 출발·끝 가지 머리 `start`·`end`, `moved` 는 0 이어야 함, `created`). 승격은 끝난 뒤 사람이 보드에서 프로젝트 설정의 승격 차례대로 한다(자율주행 가지 → dev, 자율주행 가지 → live(dev 뒤에만), live → master(live 뒤에만), 엔진 `finish <실행 폴더> merge --into <가지>`·`promote <가지> --into <가지>`·`discard`(합치기 전에만), 따로 작업 트리면 합친 뒤·버린 뒤 작업 트리를 `worktree remove`+`prune`. 보드의 합치기·승격 시트는 git 을 읽기만 하고(커밋 수·바뀐 줄·엔진과 같은 `merge-tree` 충돌 미리 보기) 누를 때만 이 명령을 부른다, macOS 2단계 5-3). 판정 미리 돌리기는 엔진 `prerun`, 단위 칸 `prerun`(기본 true). 모델 쪽 가지 보호(macOS 엔진 고침 2·셋째 실측): 두 모드 모두 `.git` 이 쓰기 범위(따로 작업 트리면 Claude Code 가 공통 `.git` 을 자동 쓰기 허용)라, 출발 때 가지·엔진이 만든 가지 가운데 지금 작업 가지를 뺀 ref 파일과 `.lock`, `HEAD`, `packed-refs`, `config`, 공통 `.git` 설정 자리(`hooks`·`config.worktree`·`info`·`commondir`·`objects/info/alternates`), 보안(`refs/replace`·`shallow`, 따로 작업 트리면 작업 트리 `.git` 파일·`commondir`·`gitdir`)을 쓰기 금지하고 `HEAD.lock`·`packed-refs.lock` 은 연다(커밋에 필요). 밖에서 도는 git 은 `commit.gpgsign=false`·`core.attributesFile=/dev/null`·`GIT_ATTR_NOSYSTEM=1`·`GIT_NO_REPLACE_OBJECTS=1` 로 돌리고 쓰기 전 git 폴더 inode 가 출발 때와 같은지 본다(git 폴더 바꿔치기 막기). Windows 의 격리도 같은 자리를 막아야 한다. 주제로 시작의 판정 시나리오는 출발 뒤 모델이 고칠 수 없고 엔진은 `pinned/checks/` 의 고정본으로 판정한다.
Jira 연결 프로젝트(2026-10-22): 에픽(사람이 번호를 줌) 아래 스토리, 스토리 아래 작업 티켓(단위 하나)을 보드가 출발할 때 한꺼번에 만들고, 가지는 출발 가지에서 스토리 가지, 스토리 가지에서 작업 가지를 엔진이 만든다(가지 이름 꼴 `tickets.branch_names` 의 자리표 `{type}`·`{story}`·`{task}`, 예 `features/parent/<스토리 키>`·`features/<작업 키>` 또는 `<종류>/_<스토리>/parent/<스토리>`·`<종류>/_<스토리>/<작업>`. `{type}` 은 `tickets.type_by_kind` 에서 단위 종류로 찾음, 2026-10-22 셋째). 판정을 통과한 작업 가지는 엔진이 바로 스토리 가지로 합치고(병합 커밋), 스토리 가지를 dev·live 로 합치는 것은 사람이 승격 화면에서 한다. 스토리가 live 에 합쳐진 뒤 "합친 가지 지우기"는 엔진 `finish … cleanup --story <키>`(live 에 다 들어간 이 실행의 가지만, `--unmerged` 면 합치지 못한 작업 가지도), `--story` 일은 지금 가지 머리가 그 실행 끝의 머리(`branches.end`)와 같은 가지만 움직인다(이어 가기가 같은 이름의 가지를 다시 만들므로, macOS 2단계 5-3). `run.json` 의 `branches.stories`. 작업 → 스토리 합치기는 체크아웃 없이(스토리 머리가 작업 머리의 조상이면 작업 tree 로 병합 커밋을 만들어 스토리 ref 만 올림, 아니면 사람 몫), 통과하지 않은 작업에 기대는(`depends`) 뒤 작업은 `skipped`. 자세히는 `docs/자율주행/설계.md` 2장 끝 "Jira 연결 프로젝트의 티켓과 가지".

Windows 로 옮길 때: 엔진은 Rust 실행 파일(같은 상태 기계), 다시 띄우기는 macOS LaunchAgent(KeepAlive) 대신 작업 스케줄러(실패하면 다시 시작), claude 와 그 자식은 Job Object 로 묶어 엔진이 죽거나 멈출 때 함께 끝낸다(macOS 는 process group). 실행 id 는 작업 이름에 들어가므로 영문·숫자·`.-_` 만.
격리: macOS 는 Claude Code 의 Bash 샌드박스(`--settings` 의 `sandbox`, Seatbelt)로 쓰기 범위·네트워크 허용 목록을 건다. Claude Code 문서상 네이티브 Windows 에서는 이 샌드박스가 없어 명령이 샌드박스 없이 돈다(WSL2 에서만 bubblewrap) — Windows 판은 WSL2 안에서 돌리거나 다른 격리(AppContainer·별도 계정 등)를 먼저 정해야 같은 약속을 지킬 수 있다. 환경 변수는 고른 목록만 넘기고(`docs/자율주행/설계.md` 4장 `passEnv`), 실행 폴더의 상태 파일은 모델의 쓰기 범위 밖에, 엔진 전용 폴더는 읽기도 금지로 둔다. 엔진이 밖에서 돌리는 git 에는 `core.fsmonitor=false`·`core.hooksPath` 비우기와 시스템·전역 설정 끄기를 건다. localhost 는 기본으로 막는다.

## 3. hook — `claude-board-hook.exe`

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

stdin JSON 하나를 읽는다. 어떤 오류도 Claude Code 를 막지 않게 **전체를 잡아 종료 코드 0** 으로 끝낸다. 출력은 쓰지 않는다.

1. `state == "waiting"` 이고 `notification_type == "idle_prompt"` 면 `idle` 로 바꾼다.
2. `session_id` 가 없으면 끝. `sessions/` 를 만든다. `idle` + `hook_event_name == "SessionStart"` 면 prune(§3.5).
3. `.<sid>.lock` 배타 잠금 아래에서 읽고 고쳐 쓴다:
   - 기록에 `pid` 가 없거나, 이벤트가 `SessionStart` 거나, 기록된 pid 가 살아 있지 않으면: Claude 프로세스를 다시 찾아 `pid`·`pid_ctime`·`tty`(null)·`headless`·`started=now`·`args` 를 새로 쓴다. 이미 있는데 `args` 만 없으면 args 만 채운다.
   - `transcript_path` 가 오면 갱신. 서브에이전트 이벤트가 아니면 `permission_mode`·`model`(빈 문자열이 아닌 문자열일 때만) 갱신.
   - `cwd = data.cwd or rec.cwd or 현재 폴더`, `project`, `session_id`, `updated=now`.
   - `project_dir = 환경 변수 CLAUDE_PROJECT_DIR(비어 있지 않으면) or rec.project_dir or cwd` — 서브에이전트 이벤트까지 모든 hook 에서, `updated` 바로 뒤 키로(state.py 와 같은 순서).
   - `prompt` 를 공백 정리 → 비어 있지 않고 `<` 로 시작하지 않고 앞 40자가 기존 topic 과 다르면 topic 교체(`topic_changed`).
   - 서브에이전트 이벤트(`agent_id` 있음): subagents 기록(`track_subagent` — `state.py:146-174` 그대로: SubagentStop 은 done+ended+last_message, 그 밖은 ended 를 지우고 waiting/running·tool 갱신, `subagents/agent-<id>.meta.json` 의 description·`requestShape=="background"` 를 처음 한 번 읽음, done 후 3600초 지난 것 삭제).
     세션 상태는 바꾸지 않는다. 단 승인 요청(waiting)이면 세션을 waiting 으로 올리고 이전 상태를 `state_before_sub_wait` 에 둔다. 그 뒤 서브에이전트가 다시 움직이면(세션이 waiting 이고 그 키가 있으면) 이전 상태로 돌린다.
   - `ended` 면 끝나지 않은 서브에이전트를 모두 done+ended.
   - `Notification` + `notification_type == "permission_prompt"` 이고 기록 상태가 `idle` 이면 `state_before_sub_wait = "idle"`. 쉬고 있는 세션은 스스로 승인을 묻지 않으므로 서브에이전트의 승인 요청이다.
   - 서브에이전트 이벤트도 `Notification` 도 아니면 `state_before_sub_wait` 를 지운다. `Notification` 은 서브에이전트의 승인 요청이어도 `agent_id` 없이 온다(Claude Code 2.1.304 확인).
   - `UserPromptSubmit` 이면 `<sid>.paused` 를 지운다.
   - `StopFailure` + `error == "rate_limit"` 이고 `last_assistant_message`(공백 정리)가 정규식 `hit your|usage limit`(대소문자 무시)에 맞으면 `limit_hit = {at: now, message: 앞 200}`.
     서브에이전트가 아닌 `UserPromptSubmit`·`PreToolUse`·`PostToolUse`·`Stop`·`SessionStart` 면 `limit_hit` 를 지운다.
   - 상태가 바뀌면 `state`·`since=now`. idle 이고 `last_assistant_message` 가 있으면 last_message(공백 정리, 200). waiting 이고 `message` 가 있으면 last_message = message 앞 200.
   - `resume_cmd` 를 만든다(§3.3). 원자적으로 쓴다.
   - 상태가 바뀌었거나 topic 이 바뀌었고 `headless == false` 면 터미널 제목을 `"<아이콘> <project> · <topic 앞 20자>…"` 로 바꾼다(아이콘 working 🔵, waiting 🟠, idle 🟢, ended ⚫; topic 이 20자 이하면 `…` 없이, 없으면 ` · ` 부분 없음).
     Windows: `FreeConsole` → `AttachConsole(pid)` → `SetConsoleTitleW` → `FreeConsole`. 실패는 무시. Windows Terminal(ConPTY) 탭 제목도 이것으로 바뀐다(실기 확인, §11.1).

근거: REA 프로세스 캡처 `ev_23674ebbd76cceea4252d166b60c1ad66648ee3f2a951ba5d4c669d1c07d3368` — `state.py` 에 이벤트 14개를 차례로 넣은 결과(테스트 기준 데이터, §12).

### 3.3 `carry_args`·`resume_cmd` — `state.py:57-97`

- 넘길 인자: 값 하나(`--model --permission-mode --agent --settings --plugin-dir`), 값 여럿(`--add-dir --mcp-config`, 다음 `-` 전까지), 값 없음(`--dangerously-skip-permissions --allow-dangerously-skip-permissions --chrome --ide --verbose`), `--x=값` 형태도 받는다. argv[0] 은 건너뛴다.
- resume: args 에서 `--permission-mode`·`--model` 을 빼고 기록된 값으로 다시 붙인 뒤 `claude <args…> --resume <sid>`. Windows 인용 규칙으로 잇는다.

### 3.4 Claude 프로세스·headless 판별 (Windows)

- 조상 따라가기: `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` 한 번으로 pid→(ppid, exe 이름) 표를 만들고, hook 의 부모부터 최대 8단계 올라간다. exe 이름이 `claude.exe` 면 그 프로세스. `node.exe`·`bun.exe` 는 명령줄에 `claude` 가 들어 있을 때만 인정.
- `alive(pid)`: `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetExitCodeProcess == STILL_ACTIVE`. `pid_ctime` 이 있으면 `GetProcessTimes` 생성 시각도 같아야 산 것으로 본다. 열 권한이 없으면(ACCESS_DENIED) 산 것으로 본다.
- `headless`(macOS 는 tty 가 없으면 headless): 다음 중 하나면 true — 실행 인자에 `-p`/`--print` 가 있다 / `AttachConsole(pid)` 실패(콘솔 없음) / 콘솔 창(`GetConsoleWindow`)도 그 창의 소유 창(`GetWindow(GW_OWNER)`, ConPTY 의 가짜 창은 터미널 창을 소유자로 둔다)도 보이지 않고, 조상 가운데 보이는 최상위 창을 가진 프로세스도 없다.
  실기 확인(§11.1): Windows Terminal 탭으로 열린 새 콘솔은 콘솔 창이 숨은 `PseudoConsoleWindow`(OpenConsole.exe)이고 그 소유 창이 보이는 WT 창(`CASCADIA_HOSTING_WINDOW_CLASS`)이라 false. 콘솔 없는 프로세스(`DETACHED_PROCESS`)·`-p` 는 true. 창 없는 콘솔(`CREATE_NO_WINDOW`)은 조상에 보이는 창이 없으면 true.

### 3.5 prune — `state.py:113-135`

종료된 지 2일 넘은(파일 수정 시각) 세션 파일 가운데 pid 가 없거나 죽은 것을 지운다. `.lock`·`.paused`·`.mute` 는 짝인 `.json` 이 없으면 지운다.

### 3.6 `notify <stop|notification>` — `notify.py`

1. stdin JSON(깨졌으면 `{}`). `mute-all` 이 있으면 끝. `sessions/<sid>.mute` 가 있으면 끝. `notification` 이고 `notification_type == "idle_prompt"` 면 끝.
2. notification: 제목 "승인 대기", kind `wait`, 소리 wait, 본문 `message` 또는 "Claude 가 입력을 기다리고 있습니다".
   stop: 제목 "작업 완료", kind `done`, 소리 done, 본문 `last_assistant_message` 또는 transcript 끝 400KB 에서 찾은 마지막 assistant 텍스트.
3. 본문 다듬기(`plain`·`first_sentence`): 코드 블록 제거, 인라인 코드·굵게 기호 제거, 줄머리 `#`·`-`·`*`·`1.`·`|` 제거, 공백 정리, 140자 넘으면 139자 + `…`. 그다음 첫 문장(`.!?。` 로 끝나는 가장 짧은 앞부분, 없으면 전체).
4. 요약 `waiting=<n>,working=<n>,idle=<n>`: 모든 세션 파일의 상태를 세되, 이 세션은 막 바뀐 상태(wait→waiting, done→idle)로 센다. 다른 세션은 pid 가 죽었으면 뺀다. 이 세션 파일이 아직 없으면 그 상태에 1 을 더한다.
5. 위치: `notify-position` 내용(없거나 비면 `center`).
6. `app-path` 의 ClaudeBoard.exe 를 `--notify --title <제목> --subtitle <project> --body <본문> --summary <요약> --sound <done|wait> --kind <done|wait> --duration 10 --position <위치>` 로 띄우고 **기다리지 않고** 끝낸다.
   `CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`(breakaway 가 거부되면 그 플래그 없이 다시), 표준 입출력은 닫는다. app-path 가 없거나 파일이 없으면 끝.

### 3.7 `setup --app <ClaudeBoard.exe>` / `uninstall` (macOS `user-setup.sh`·`Uninstall.command` 대응)

setup: `notifier\sessions\` 만들기 → 자기 자신을 `notifier\claude-board-hook.exe` 로 복사(§1.2 이름 바꾸기 규칙) → `app-path` 쓰기 → `install-hooks` → `%USERPROFILE%\.local\bin\claude-board.cmd`(`@start "" "<app>" %*`)·`claude-board-tui.cmd`(`@"%USERPROFILE%\.claude\notifier\claude-board-hook.exe" tui %*`) → Run 레지스트리 값.
uninstall: `uninstall-hooks` → Run 값 삭제 → 실행 중인 ClaudeBoard.exe 종료 → notifier 의 hook exe(들)·`app-path`·`notify-position`·`sessions\`·`accounts.json`·`accounts\`(DPAPI 금고 포함) 삭제, 빈 notifier 폴더 삭제 → `.local\bin` 의 cmd 두 개 삭제. 지금 Claude Code 로그인(`.credentials.json`)은 건드리지 않는다. 앱 폴더 삭제는 uninstall.ps1 이 한다(실행 중인 자기 exe 는 못 지우므로).

### 3.8 `tui` — `board-tui.py` 이식

crossterm 으로 같은 화면: 프로젝트 경로별 카드(`project_dir`, 없으면 `cwd`. 테두리 색 = 가장 급한 상태), 상태별 표시(작업 중 스피너 `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, 승인 대기 `◉/○` 깜빡임, 대기 `✓`, 종료 `✕`), 경과 시간, topic, 백그라운드 표시, 승인 대기 메시지 줄, 아래 상태별 개수, 0.12초 다시 그리기·1초 다시 읽기, `q` 종료, `t` 테마 순환(4종, `notifier\board-theme` 에 저장), 한글 폭 2칸. 종료 후 30분 넘은 ended 세션 파일은 지운다(`board-tui.py:107-129`).

## 4. 알림 패널 — `ClaudeBoard.exe --notify …` (`ClaudeNotifier.swift`)

- 인자: `--title --subtitle --body --kind done|wait --summary waiting=1,working=2,idle=3 --sound done|wait|none --duration 10 --position center|top-left|top-right|bottom-left|bottom-right`. 같은 키를 다시 주면 뒤의 값.
- 이 모드는 단일 실행 잠금을 잡지 않는 별도 프로세스다. 패널 하나를 보이고 닫히면 프로세스가 끝난다.
- 창: 테두리 없음, 투명 배경 위 둥근 카드(반경 14), 폭 440, 높이는 내용에 맞춤. **포커스를 가져가지 않는다** — `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`, 보일 때 `SW_SHOWNOACTIVATE`. 작업 표시줄에 나타나지 않는다. 실기 확인(§11.1): 띄우기 전후 포그라운드 창이 같고, 확장 스타일에 NOACTIVATE·TOOLWINDOW·TOPMOST 가 켜지고 APPWINDOW 는 꺼져 있다.
- 위치: 마우스가 있는 모니터의 작업 영역. 가운데: 슬롯 0~4 의 가로 이동 `[0, 0.5, -0.5, 1, -1] × 폭`. 모서리: 여백 12, 같은 모서리에 떠 있는 패널의 세로 범위를 피해 간격 8 로 쌓는다(위 모서리는 아래로, 아래 모서리는 위로). 슬롯 파일 `%TEMP%\claude-notifier-slots\<0..4>`(`CreateNew` 로 잡음, 120초 넘게 갱신 없는 것은 버림, 내용 `"<position> <top> <bottom>"`), 범위를 읽고 쓰는 동안 `lock` 파일 잠금, 1초마다 수정 시각 갱신, 닫을 때 지움(`ClaudeNotifier.swift:54-118, 223-250`).
- 내용(`ClaudeNotifier.swift:280-420`, 스크린샷 `docs/images/notify-wait.png`·`notify-done.png`): 1줄 = 색 배지(30×30, 아이콘 wait ✋ / done ✓) · `<subtitle>` 흰 굵게 + `<title>` 강조색 · 오른쪽 `HH:mm`. 2줄 = 본문(최대 2줄, 흰 82%). 3줄 = 상태별 개수(색 점 + "승인 대기 n"·"작업 중 n"·"대기 n", 0 은 숨김). 맨 아래 2px 진행 막대(남은 시간).
  강조색 wait `rgb(255,168,64)`, done `rgb(77,217,140)`. 바탕은 보드와 같은 남색 그라데이션 + 왼쪽 위 강조색 빛 + 강조색 35% 테두리.
- 동작: 0.28초 동안 14px 아래에서 올라오며 나타남, 소리(done → 시스템 알림음, wait → 시스템 경고음; `PlaySoundW` 별칭, 실패 무시), 마우스를 올리면 카운트다운 멈춤, 누르면 보드를 띄우고(`ClaudeBoard.exe --show`) 닫힘, 시간이 다 되면 0.25초 동안 흐려지며 10px 올라가고 끝.

## 5. 보드 백엔드 (`app/src-tauri`)

### 5.1 세션 읽기 — `Store.reload` (`ClaudeBoard.swift:941-1207`)

1초마다(그리고 표지 파일을 바꾼 직후 바로) `sessions/*.json` 을 읽는다. 파일 수정 시각이 같으면 이전에 푼 값을 쓴다.

- 상태 보정: ended 가 아닌데 pid 가 죽었으면 `ended`.
- `transcript = transcript_path` 또는 `~/.claude/projects/<cwd 의 영숫자 밖 문자를 - 로>/<sid>.jsonl`. ended 이고 (`updated` 가 2일 넘었거나 transcript 파일이 없으면) 세션 파일을 지우고 뺀다.
- 서브에이전트: ended 가 아니면 `subagents` 를 풀되 끝난 지 10초 넘은 것은 뺀다. 정렬: done 이 아닌 것 먼저, started 순. 각 `subagents/agent-<id>.jsonl` 에서 모델·effort(§5.2).
- 백그라운드 셸 수: idle 일 때만, 그 Claude 프로세스의 자식 가운데 명령줄에 `shell-snapshots` 가 든 프로세스 수(3초마다 백그라운드에서 셈). idle 인데 셸이 있거나 끝나지 않은 서브에이전트가 있으면 `working` 으로 보인다.
- `<sid>.paused` 가 있고 ended 가 아니면 `paused`. `<sid>.mute` 가 있으면 muted.
- ended 가 아니면 모델·effort: transcript 의 마지막 assistant 줄(§5.2). 상태줄 기록의 effort 가 신선하면 그것으로 덮는다.
- 컨텍스트: §5.3. topic·last_message 에서 `#*\`` 기호를 지운다.
- 정렬·묶기: `project_dir`(없거나 빈 문자열이면 `cwd`) 별 그룹 — 그룹 id·이름·경로가 이 값이다. 세션 하나의 `cwd`(와 그 이름)는 그대로 둔다. 그룹 안은 rank(waiting 0, working 1, paused·idle 2, ended 3) 오름차순, 같으면 since 최신 먼저.
  그룹 순서: 고정(pinned) 순서 → 가장 급한 rank → 가장 최근 updated.
- 지운 그룹(hiddenGroups: 그룹 id→지운 시각): 모든 세션 updated 가 그 시각 이하면 숨기고, 새 활동이 있으면 목록에서 빼고 다시 보인다.
- 조용한 카드 자동 접기: waiting·working 이 없고 마지막 updated 가 30분 넘은 그룹을 한 번 접는다(autoFolded 에 그때의 마지막 활동 시각을 적어, 사람이 다시 펴면 새 활동 전에는 또 접지 않는다).
- 새로 승인 대기가 된 세션의 그룹은 접혀 있으면 편다.
- `mute-all` 존재 여부.

### 5.2 모델·effort — `ModelReader` (`ClaudeBoard.swift:97-147`)

파일 끝 400KB 를 거꾸로 보며 `"assistant"` 가 든 줄 가운데 `type=="assistant"`, `message.model` 이 `<synthetic>` 이 아닌 첫 줄 → `{model, effort: 줄의 effort}`. 수정 시각이 같거나 3초 안이면 다시 읽지 않는다. 없으면 세션 기록의 `model` 로 대신(effort 없음).
계열 이름: opus/sonnet/haiku/fable 이 들어 있으면 첫 글자 대문자, 아니면 `claude-` 를 뺀 이름. 버전: `-(\d+)-(\d+)` → `5.5`. effort 단계: low 1 … max 5.

### 5.3 컨텍스트 — `ContextReader` (`ClaudeBoard.swift:149-246`)

1순위 `context/<sid>.json`: `context_window_size > 0` 이고 transcript 수정 시각이 기록의 `at` 보다 60초 넘게 늦지 않으면 used = 세 입력 토큰 합(0 이면 `used_percentage`). 그 모델의 크기를 `context-windows.json` 에 배워 둔다.
2순위 transcript 끝 1.5MB 에서 사이드체인이 아닌 마지막 assistant 의 usage 합. 크기 = 배운 값 또는 기본(`[1m]` 이나 `claude-opus-5`·`claude-sonnet-5`·`claude-haiku-5`·`claude-fable-5` 로 시작하면 1,000,000, 그 밖 200,000), used 가 크기보다 크면 1,000,000. pct = min(100, used×100/size).

### 5.4 사용 한도 — `UsageStore` (`ClaudeBoard.swift:262-354`)

30초마다. 캐시 파일이 120초 안에 쓰였으면 그것만 쓴다. 아니면 `~/.claude/.credentials.json` 의 `claudeAiOauth.accessToken`(없으면 조용히 실패 표시) — `expiresAt`(ms)이 지났으면 "로그인 토큰 갱신 전". `GET https://api.anthropic.com/api/oauth/usage` 헤더 `Authorization: Bearer <토큰>`, `anthropic-beta: oauth-2025-04-20`, 제한 10초.
응답: `five_hour.utilization`→"5시간", `seven_day.utilization`→"주간 · 전체", `limits[]` 가운데 `kind=="weekly_scoped"` 의 `percent` → "주간 · <scope.model.display_name>", 각 `resets_at`(ISO8601). 성공하면 캐시에 쓴다. 계정을 바꾸면 캐시를 지우고 5초 동안 그 전 캐시는 믿지 않으며 진행 중이던 응답은 버린다.
토큰은 메모리에만 두고 기록하지 않는다. **(`.credentials.json` 의 필드 이름은 문서에 없다 — 없으면 "사용량을 불러오지 못했습니다" 로 조용히 내려간다)**

### 5.5 계정 전환 — `AccountStore` (`ClaudeBoard.swift:369-789`)

macOS 키체인 대신:
- 지금 로그인: `~/.claude/.credentials.json`(JSON, `claudeAiOauth` 와 그 밖의 키 — `mcpOAuth` 등은 그대로 둔다). 계정 정보: `~/.claude.json` 의 `oauthAccount`(3초마다 수정 시각이 바뀌면 다시 읽음).
- 저장한 계정 토큰: `notifier\accounts\vault\<계정 id 의 sha256 앞 16자>.bin` = DPAPI(`CryptProtectData`, 현재 사용자) 로 암호화한 `{"claudeAiOauth": …}`. 표시 정보는 `notifier\accounts.json`(`{"accounts":[{oauthAccount, savedAt, refreshTokenExpiresAt?}]}`, macOS 와 같은 형식).
- 계정 id = `accountUuid|organizationUuid`. 이메일·조직·요금제(`organizationType` 의 `claude_` 를 빼고 첫 글자 대문자 + `organizationRateLimitTier` 의 `_<n>x` → " 5x"), `refreshTokenExpiresAt` 지나면 "다시 로그인 필요".
- 전환: `~/.claude/.storage-write.lock` 폴더 잠금(mkdir, 15초 넘은 잠금은 버림, 5초 기다림 — Claude Code 와 같은 방식) 안에서 지금 계정의 최신 토큰을 저장 → 대상 토큰을 `claudeAiOauth` 에 넣어 `.credentials.json` 을 원자적으로 씀. 그다음 `~/.claude.json.lock` 잠금 안에서 `oauthAccount` 를 바꾸고, 다른 계정이면 계정별 캐시 키(`ClaudeBoard.swift:535-539` 목록)를 지운다.
- 계정 추가: 새 터미널 창에서 `CLAUDE_CONFIG_DIR=<notifier\accounts\login-<8자>>` 로 `claude auth login --claudeai [--email <이메일>]` 실행 후 안내 문구. 3초마다 그 폴더의 `.claude.json` 의 `oauthAccount` 와 `.credentials.json` 의 `claudeAiOauth` 가 생기면 저장하고 지금 계정도 저장한 뒤 폴더를 지운다. 토큰만 있고 계정 정보가 30초 넘게 없으면 실패 안내. 15분 넘으면 그만둔다. 시작할 때 남은 `login-*` 폴더를 치운다.
- 작업 중·승인 대기 세션이 있으면 전환 전에 확인 대화상자(`ClaudeBoard.swift:1718-1740` 문구).
- 안내 문구(notice)는 7초 뒤 사라진다. 문구는 macOS 와 같게.

### 5.6 한도 자동 전환 — `AutoSwitcher` (`ClaudeBoard.swift:791-918`)

같은 규칙(3초 주기, settle 40초, recent 6시간, 찬 계정 기록과 풀리는 시각, 다음 계정 고르기, 이어 가기 요청 문구, `auto-switch.log`). 이어 가기는 Ghostty 대신 §5.7 의 콘솔 입력으로 `Esc → 0.5초 → 문구 → 0.2초 → Enter`. `canDrive` 가 아니면 직접 이어 달라고 안내.

### 5.7 터미널 제어 (Windows) — Ghostty AppleScript 대체

실기 확인(§11.1): 콘솔 붙기·제목·입력 넣기(줄 단위·raw 모드 모두, Windows Terminal 탭 포함), 세션의 터미널 창 찾기, 이어 열기·계정 추가의 새 콘솔 실행(인자·환경·작업 폴더가 그대로 도착). **Windows 미확인**: Windows Terminal 탭 고르기(UI Automation), 창 앞으로 가져오기, 실제 Claude Code 에서의 일시중지·재개·중단.

- 콘솔 입력: 보드 프로세스가 `FreeConsole` → `AttachConsole(pid)` → `CreateFileW("CONIN$")` → `WriteConsoleInputW` 로 KEY_EVENT(누름·뗌) 를 넣고 → `FreeConsole`. Esc = VK_ESCAPE/0x1B, Enter = VK_RETURN/`\r`, 글자는 UTF-16 단위마다 `wVirtualKeyCode=0` 에 문자. 붙어 있는 동안 `SetConsoleCtrlHandler(NULL, TRUE)`. 한 번에 한 세션씩(전역 잠금). `canDrive(s)` = headless 아님 + ended 아님 + 콘솔에 붙을 수 있음.
- 일시중지 = Esc 후 `<sid>.paused` 기록. 재개 = 살아 있으면 `carryOnPrompt` 입력 + Enter 후 표지 삭제, 아니면 이어 열기(carryOn). 중단 = 확인 대화상자 → 표지 삭제 → canDrive 면 `Esc → 0.4초 → /exit → 0.2초 → Enter`, 아니면 프로세스 종료(`TerminateProcess` 대신 먼저 `GenerateConsoleCtrlEvent(CTRL_BREAK)` 를 시도하지 말고 macOS SIGTERM 처럼 바로 종료해도 된다 — 대화 기록은 남는다).
  문구: `carryOnPrompt` = "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요." 자동 전환 문구 = `ClaudeBoard.swift:801`.
- 세션으로 이동(더블 클릭): 콘솔에 붙어 `GetConsoleWindow` → 보이면 그 창, 아니면 소유 창(`GW_OWNER`)·조상 프로세스의 보이는 최상위 창 → 최소화면 복원 후 앞으로(`AllowSetForegroundWindow`/`SetForegroundWindow`, 막히면 `AttachThreadInput` 방식). Windows Terminal 이면 탭까지: 그 콘솔 제목을 표지 `CBFOCUS-<8자>` 로 잠깐 바꾸고 UI Automation 으로 이름이 그 표지인 TabItem 을 찾아 `SelectionItemPattern.Select` 한 뒤 제목을 되돌린다(`ClaudeBoard.swift:4831-4875` 와 같은 생각).
- 한 번 클릭 = 그 세션의 터미널 앱 창만 앞으로(위 창 찾기의 결과), 모르면 Windows Terminal(없으면 아무것도 안 함).
- 이어 열기(종료 세션 더블 클릭·"세션 다시 열기"): `resume_args`(없으면 `["--resume", "<id>"]`), carryOn 이면 문구를 인자 하나로 뒤에 붙이고 `.paused` 삭제.
  **셸(cmd.exe·PowerShell·wt.exe 명령줄)을 거치지 않는다** — `claude.exe`(PATH 또는 `%USERPROFILE%\.local\bin\claude.exe`)를 인자 배열로 `CREATE_NEW_CONSOLE` 로 직접 띄우고 작업 폴더는 `project_dir`(없으면 cwd — 보드 카드의 경로). Windows 11 에서 기본 터미널이 Windows Terminal 이면 그 안에 열린다. claude 가 끝나면 창도 닫힌다(macOS 는 셸 pane 이 남는다 — 안전을 위해 다르게 한다).
- 명령을 새 터미널 창에서 실행(`runInTerminal`, 계정 추가): 같은 방식 — `claude.exe auth login --claudeai [--email <이메일>]` 를 환경변수 `CLAUDE_CONFIG_DIR=<로그인 폴더>` 만 더해 `CREATE_NEW_CONSOLE` 로 직접 띄운다. 홈 폴더에서 시작. 끝난 뒤 안내는 보드의 안내 문구가 맡는다.
- `--action pause|resume|stop|reopen|focus <id>` 로 실행되면 떠 있는 보드에 넘긴다(단일 실행 플러그인의 인자 전달) — stop 은 확인 없이.

### 5.8 업데이트 — `Updater` (`ClaudeBoard.swift:4097-4224`)

- 현재 버전 = 빌드 때 `windows/VERSION`. 개발 빌드(`VERSION` 을 못 읽었거나 환경변수로 dev 표시)는 확인하지 않고 제목 옆에 `dev`.
- 켤 때와 1시간마다 `GET https://api.github.com/repos/suhojang/claude-board/releases?per_page=30`(헤더 `User-Agent: ClaudeBoard-Windows`, `Accept: application/vnd.github+json`) → draft·prerelease 가 아니고 `tag_name` 이 `windows-v` 로 시작하는 첫 릴리스 → 버전 = 태그에서 `windows-v` 를 뺀 것(숫자 비교). 더 새로우면 알림 줄. **`/releases/latest` 를 쓰지 않는다.**
- 알림 줄: "새 버전 <v> 이 나왔습니다" · **변경 내용**(`https://github.com/suhojang/claude-board/blob/main/windows/CHANGELOG.md`) · **업데이트**. 손으로 확인했는데 없으면 "최신 버전입니다 (<v>)" 4초, 실패하면 "업데이트를 확인하지 못했습니다".
- 업데이트 실행: `https://raw.githubusercontent.com/suhojang/claude-board/windows-v<v>/windows/packaging/install.ps1`(태그 기준 원본 — 릴리스 자산과 달리 UTF-8 로 내려와 update.log 의 한글이 깨지지 않는다)로 `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "& { $s = irm '<url>'; & ([scriptblock]::Create($s)) -Version '<v>' -Update }"` 를 창 없이 분리 실행, 출력은 `notifier\update.log`. 이때 환경변수 `PSModulePath` 를 넘기지 않는다 — PowerShell 7 에서 띄운 보드는 PowerShell 7 의 모듈 경로를 물려받고, 그것을 받은 5.1 은 `Get-FileHash` 를 찾지 못해 install.ps1 이 멈춘다(실기 확인). 성공하면 스크립트가 보드를 끄고 새 보드를 띄우므로 돌아오지 않는다. 프로세스가 끝나 돌아오면 실패 — "업데이트하지 못했습니다 · ~/.claude/notifier/update.log" + 다시 시도.

### 5.9 보드 설정 저장 (macOS UserDefaults 대응)

`board-settings.json`(Tauri app config dir) 한 파일, 백엔드가 단일 출처. 키: `collapsed[]`, `endedOpen[]`, `pinnedGroups[]`, `hiddenGroups{그룹 id:시각}`, `autoFolded{그룹 id:시각}`, `kindCollapsed[]`("<그룹 id>|term"·"<그룹 id>|bg"), `subCollapsed[]`(세션 id), `usageCompact`, `pinned`(항상 위, 기본 true), `mascot`(기본 `tralalero`), `opacity`(0.3~1, 기본 1), `autoSwitch`(기본 false), `autoSwitchLast`, `autoSwitchFull{id:시각}`, `boardMini`, `boardRestoreFrame{x,y,w,h}`, `windowFrame`.

### 5.10 창·트레이·단축키

- 보드 창: 장식 없음(직접 그린 제목 막대: 끌기 영역, 오른쪽에 최소화·닫기, 왼쪽에 "처음 크기로 오른쪽에 붙이기" 단추, 오른쪽에 최대축소/⤢ 단추), 투명 배경 + Windows 11 Mica/아크릴(`window-vibrancy`, 실패 무시), 크기 조절 가능, 최소 300×144.
  실행할 때: 미니로 끄지 않았으면 폭 454, 그 모니터 작업 영역 높이 전체, 마지막 가로 위치. 닫기 = 숨기기(앱은 트레이에 남음).
- 항상 위(pinned, 기본 켜짐) = `set_always_on_top`.
- 투명도: 슬라이더 값(30~100%)을 보드가 활성이 아니고 마우스가 창 밖일 때 문서 전체 불투명도로 적용, 마우스가 들어오거나 활성이면 100%, 슬라이더를 움직이는 동안과 놓은 뒤 1.2초는 미리보기.
- 전역 단축키 Ctrl+Alt+B = 처음 크기로 오른쪽에 붙이고 앞으로(포커스는 가져가지 않음). 등록 실패는 `board.log` 에만.
- 트레이: 앱 아이콘. 왼쪽 클릭 = 보드 열고 닫기. 툴팁 "Claude Board — 승인 대기 n · 작업 중 n". 승인 대기가 있으면 주황 점, 작업 중만 있으면 파란 점이 붙은 아이콘으로 바꾼다(미리 만든 그림). 오른쪽 클릭 메뉴: 세션 보드 보기 · 항상 위에 고정(체크) · 캐릭터 ▸(8종, 체크) · 알림 위치 ▸(중앙(기본)·왼쪽 상단·오른쪽 상단·왼쪽 하단·오른쪽 하단, 체크) · 업데이트 확인… · 종료.
- 단일 실행: 두 번째 실행은 첫 보드를 앞으로(`--show` 와 같음), `--action` 이면 동작을 넘기고 끝. `--background` 면 창을 보이되 포커스를 가져가지 않는다.
- Windows Terminal(`WindowsTerminal.exe`)이 새로 실행되면 보드를 보이게(포커스는 그대로) — 3초 주기 프로세스 확인. (macOS 의 Ghostty 연동 대응)
- 설치된 Claude Code 버전: `%USERPROFILE%\.local\share\claude\versions\` 의 가장 높은 이름, 없으면 표시 안 함(1분 캐시).

### 5.11 터미널 독 (`EdgeDock`, `ClaudeBoard.swift:3391-3627`)

창 오른쪽 아래 끝 탭에 마우스를 대면 나오는 실행 단추 묶음. Windows 목록: Windows Terminal(`wt.exe`), PowerShell 7(`pwsh.exe`), Windows PowerShell(`powershell.exe`), 명령 프롬프트(`cmd.exe`), Git Bash(`git-bash.exe` — `%ProgramFiles%\Git`). 설치 여부(PATH·알려진 위치), 실행 중 점, 힌트 문구("<이름> 창으로 이동"/"<이름> 실행"/"<이름> · 설치되지 않음"). 아이콘은 직접 그린 SVG.

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
  terminals: { id: string; name: string; installed: boolean; running: boolean }[];
}
interface Group { id: string /* project_dir, 없으면 cwd */; name: string; path: string /* 홈을 ~ 로 */; sessions: Session[] }
interface Session {
  id: string; cwd: string; state: State; since: number; updated: number; headless: boolean;
  title: string /* topic 또는 message 또는 "요청 전" */; topic: string; message: string;
  ctx: { used: number; size: number; pct: number } | null;
  brain: { model: string; effort: string; family: string; version: string; effortLevel: number } | null;
  subagents: Sub[]; shells: number; paused: boolean; muted: boolean; limitAt: number | null; canDrive: boolean;
}
interface Sub { id: string; type: string; desc: string; state: "running" | "waiting" | "done"; started: number;
                ended: number | null; tool: string; background: boolean; brain: Session["brain"] }
interface Limit { id: string /* 5h | 7d | 7d-<이름> */; label: string; short: string; tiny: string; pct: number; resets: number | null }
interface Account { id: string; email: string; org: string; plan: string; expired: boolean }
```

### 6.2 명령 (프론트 → 백엔드, Tauri `invoke`)

`get_snapshot` · `toggle_collapsed(cwd)` · `set_all_collapsed(collapse)` · `toggle_ended_open(cwd)` · `toggle_pin_group(cwd)` · `hide_group(cwd)` · `remove_ended(cwd)` · `remove_session(id)` ·
`toggle_kind_collapsed(key)` · `toggle_sub_collapsed(id)` · `set_usage_compact(bool)` · `set_mute(id, muted)` · `set_all_muted(bool)` · `reveal(cwd)` ·
`session_action(action: "activate"|"focus"|"resume"|"pause"|"resume_paused"|"stop", id)` · `switch_account(id)` · `add_account(email?)` · `cancel_add_account` · `remove_account(id)` · `set_auto_switch(bool)` ·
`check_update(manual)` · `run_update` · `open_changelog` · `set_pinned(bool)` · `set_mascot(kind)` · `set_opacity(v)` · `set_notify_position(pos)` · `open_terminal(id)` ·
`enter_mini(rows)` · `leave_mini` · `snap_right` · `hide_board` · `minimize_board` · `log(text)`.
그룹을 가리키는 명령 인자 `cwd` 는 그룹 id(§5.1 의 묶음 열쇠)다.
확인 대화상자(계정 전환·세션 중단)는 백엔드가 네이티브 대화상자(`tauri-plugin-dialog`)로 띄운다.

### 6.3 알림 패널 — 프론트 경로 `/notify`, 백엔드가 인자를 `get_notify_options` 로 준다. 내용 높이 `notify_ready(height)`(백엔드가 그 높이로 자리를 잡고 보인다), 마우스 `notify_hover(inside)`, 클릭 `notify_clicked`(백엔드가 보드를 띄움), 닫힘 애니메이션이 끝나면 `notify_finish`(백엔드가 슬롯을 놓고 프로세스를 끝냄). 카운트다운은 프론트가 한다.

### 6.4 캐릭터 컴포넌트 계약 — `app/src/lib/mascot/Foreman.svelte`

props `{ groups: Group[]; limits: Limit[]; now: number; compact?: boolean; mascot: MascotKind; onPickMascot: (kind) => void }`. 말풍선 문구·시계·그림·오른쪽 클릭 캐릭터 메뉴를 모두 안에서 처리한다. 이름·유행어 목록은 `app/src/lib/mascot/mascots.ts` 의 `MASCOTS`.

## 7. 보드 화면 (프론트엔드) — `ClaudeBoard.swift:1247-2483, 2584-2930, 3772-4095, 4246-4460` 와 스크린샷 `docs/images/*.png`

- 어두운 테마 고정. 바탕(`Backdrop`, `ClaudeBoard.swift:2608-2622`), 상태 색(`Palette`, 923-939): working `rgb(115,168,255)`, waiting `rgb(255,168,92)`, idle `rgb(179,191,219)`, paused `rgb(242,204,89)`, ended 회색, accent `rgb(217,120,87)`.
- 상태 아이콘 애니메이션: 작업 중 도는 그라데이션 호(0.9초), 승인 대기 퍼지는 파문(1.1초), 대기 통통 튀며 나타나는 체크, 일시중지 ⏸, 종료 빨간 ⊖. 작업 중 줄에 2.4초 주기로 지나가는 빛(Shimmer). 승인 대기 카드 테두리가 2.4초 주기로 숨 쉼. 창이 가려지거나 숨으면 애니메이션 멈춤.
- 단계(tier, `ClaudeBoard.swift:4246-4283`): 폭 360 미만 = mini. 아니면 목록 칸이 120 이상 남으면 full, 56 이상이면 medium, 그 밖 mini. medium 은 사용 한도 한 줄, 카드는 접힌 한 줄(눌러서 그 카드만 펼침, 저장된 접기 상태는 그대로). mini 는 캐릭터 줄 + 경로마다 한 줄 + 아래 줄(한도·투명도), 제목 막대에 상태별 개수와 ⤢.
- 최대축소(↘↖): 오른쪽 위 모서리 기준 폭 300, 높이 = 144 + (min(max(그룹 수,2),6) − 2) × 26. ⤢ 는 기억한 위치(작업 영역과 겹치고 폭 ≥ 360)로, 없으면 처음 크기로 오른쪽에 붙이기.
- 윗줄: 앱 아이콘 + "Claude Board" + 버전 캡슐, 아래 "활성 세션 n개 · Claude Code <v>", 오른쪽 캐릭터(§7.1). 둘째 줄: 상태 칩(승인 대기·작업 중·대기, 누르면 그 상태만 보기/다시 누르면 모두), 전체 알림 단추(꺼지면 붉은 "알림 꺼짐"), 업데이트 확인 ⟳, 모두 접기/펼치기(full 에서만), 위에 고정.
- 사용 한도 카드(1479-1628): 계정 칩(메뉴: 계정 목록·체크·"다시 로그인 필요"·계정 추가…/추가 취소·저장한 계정 지우기 ▸·한도 자동 전환 체크), 간략/상세 단추, 한도 막대(색: <50 초록, <80 주황, 그 밖 빨강), 남은 시간 문구(`resetText`), 안내 문구.
- 경로 카드(2221-2395): 접기 화살표, 폴더(접혀 있으면 대표 세션 상태 아이콘), 이름·경로(접히면 "<상태> · <하는 일>" +n), 별(상단 고정)·X(목록에서 지우기), 상태별 개수, 오른쪽 클릭 메뉴. 안: 터미널/백그라운드 구역 머리글(접기), 세션 줄, "종료된 세션 n개"(접기, 마우스 올리면 휴지통 "모두 지우기").
- 세션 줄(1743-1913): 서브에이전트 접기 화살표, 상태 아이콘, 제목, 알림 스위치(전체 알림 꺼짐이면 잠김), … 메뉴(ended: 세션 다시 열기/재개, paused: 재개·중단, 그 밖: 일시중지(idle 이거나 canDrive 아니면 비활성)·중단), ended 면 ⓧ 지우기. 둘째 줄: 경과 시간(ended "HH:mm 종료", idle "<경과> 전"), 하위 작업 n(running/count), 셸 n개, 모델 라벨(계열 색 Opus `rgb(255,153,115)` Sonnet `rgb(122,179,255)` Haiku `rgb(140,224,204)` Fable `rgb(224,179,255)` + effort 막대 5칸), 컨텍스트 라벨(<30 초록, <70 노랑, 그 밖 빨강). 폭이 좁으면 3단계로 줄임. 승인 대기면 ✋ 메시지(2줄). 한 번 클릭 = activate, 더블 클릭 = ended 면 resume, 아니면 focus.
- 서브에이전트 줄(2144-2201): 트리 연결선, 작은 상태 아이콘, 설명(없으면 종류), 라벨 "서브에이전트"·종류·경과("<시간> 걸려 완료"/"승인 대기")·모델·🔧도구.
- 아래로 가려진 카드가 있으면 "아래에 경로 n개 더[ · 승인 대기 있음]"(누르면 그 카드로 스크롤). 세션이 없으면 "아직 기록된 세션이 없습니다 / Claude Code 세션에서 요청을 보내면 여기에 나타납니다".
- 힌트(설명 말풍선): 0.35초 뒤, 방금 떠 있었으면 바로. 누르거나 스크롤하면 사라짐. 모든 단추의 설명 문구는 macOS `.hint(…)` 문구를 그대로 쓴다.
- 업데이트 알림 줄(4186-4224), 오른쪽 아래 투명도 슬라이더, 오른쪽 아래 끝 터미널 독(§5.11).

### 7.1 캐릭터 (`ClaudeBoard.swift:2643-3388`)

말풍선 문구(`foremanQuips`, 2674-2726) 그대로. 3.2초마다 다음 말, 3.7초마다 눈 깜빡임, 말이 바뀌면 0.45초 폴짝. 기분 urgent/busy/idle/sleepy 별 색·동작.
8종(트랄랄레로 트랄랄라·봄바르디로 크로코딜로·퉁 퉁 퉁 사후르·리릴리 라릴라·브르르 브르르 파타핌·보네카 암발라부·카푸치노 아사시노·현장 감독)과 유행어(2949-2961), 그림·동작은 `BrainrotFigure`(3019-3388)와 감독 `figure`(2806-2927)를 SVG 로 옮긴다(46×44, mini 0.75배). 누르면 다음 말, 오른쪽 클릭 = 캐릭터 고르기.

## 8. 배포

### 8.1 릴리스 자산 (태그 `windows-v<버전>`, Latest 아님)

| 자산 | 내용 |
|---|---|
| `ClaudeBoard-windows-x64.zip` | `ClaudeBoard.exe`, `claude-board-hook.exe`, `LICENSE`, `README.md` |
| `ClaudeBoard-windows-x64.zip.sha256` | `<sha256 16진수>  ClaudeBoard-windows-x64.zip` |
| `ClaudeBoard-<버전>-setup.exe` | Tauri NSIS 설치 프로그램(사용자 단위 설치, 설치 후 `claude-board-hook.exe setup` 실행) |
| `install.ps1`, `uninstall.ps1` | 설치·업데이트·제거 스크립트 |
| `VERSION` | 버전 한 줄 |

setup.exe 메모(실기 확인): `/S` 로 같은 버전을 다시 설치하면 Tauri NSIS 는 옛 제거 프로그램을 부르지 않는다 — PREUNINSTALL 의 제자리 건너뛰기(`$EXEDIR == $INSTDIR`)는 GUI 업그레이드에서 쓰인다(제자리 제거 `uninstall.exe /S _?=<폴더>` 는 금고·hook·자동 실행 값을 남김을 확인). NSIS 제거 프로그램이 스스로 지우는 Run 값 이름은 `Claude Board` 이고 setup 이 쓰는 값은 `ClaudeBoard` 라, 자동 실행 값은 PREUNINSTALL 의 `uninstall` 이 지운다.

### 8.2 `install.ps1`

`irm https://raw.githubusercontent.com/suhojang/claude-board/main/windows/packaging/install.ps1 | iex` (처음 설치·업데이트 같은 명령). 인자 `-Version <v>`(없으면 API 로 최신 `windows-v*`), `-Update`(묻지 않음).
1) 처음 설치(settings.json 에 `.claude/notifier/` 가 없음)이고 `-Update` 가 아니면 hook 11개와 환경변수를 등록할지 묻는다(Enter = 예). 2) zip·sha256 받기 → 해시 확인(다르면 중단) → 임시 폴더에 풀기. 3) 실행 중인 `ClaudeBoard.exe` 종료 → `%LOCALAPPDATA%\Programs\Claude Board\` 에 바꿔 넣기(실패하면 되돌리기) → `Unblock-File`. 4) `claude-board-hook.exe setup --app <경로>`. 5) 보드 실행. 6) 처음 설치면 "이미 실행 중인 Claude Code 세션은 다시 시작해야 hook 이 적용됩니다". Windows PowerShell 5.1 에서도 돌게 쓴다.
`uninstall.ps1`: `claude-board-hook.exe uninstall` → 앱 폴더 삭제 → 안내.

### 8.3 CI·자동 릴리스 — `.github/workflows/windows.yml`

- 계기: `windows/**`·워크플로 파일이 바뀐 push(모든 브랜치)·pull_request, workflow_dispatch.
- build 작업(windows-latest): Rust stable, Node 22, `cargo test`(windows 워크스페이스), `cargo build --release -p claude-board-hook`, 앱 `npm ci` → `npx tauri build`(NSIS), 자산 묶기(§8.1), `actions/upload-artifact` 로 올리기.
- smoke 작업(windows-latest, build 뒤): 아티팩트로 실제 설치·화면을 확인한다. 러너는 화면 세션이 있는 Windows Server 2025 이고 기본 터미널이 Windows Terminal 이다.
  `windows/ci/install-checks.ps1`(설치 확인 — 실패하면 작업 실패): setup.exe `/S` 설치 → 등록된 hook 을 가짜 Claude Code(node.exe 를 `claude.exe` 로 복사) 아래에서 그대로 실행 → 같은 버전 재설치·제자리 제거 → `/S` 제거, install.ps1·uninstall.ps1 을 Windows PowerShell 5.1 에서 웹 호출을 같은 이름의 함수로 가로채 실행(처음 설치 거부·`-Update` 설치·틀린 sha256 거부).
  `windows/ci/screen-checks.ps1`(화면 확인 — 정보용, `continue-on-error`): 보드 창·단일 실행·알림 패널의 포커스·확장 스타일·스스로 닫힘, 스크린샷. 결과표·로그·스크린샷은 `smoke-results` 아티팩트.
- release 작업: main 의 push 일 때만, build·smoke 뒤(설치 확인이 실패하면 릴리스하지 않는다). `windows/VERSION` 을 읽어 태그 `windows-v<버전>` 이 아직 없고 `windows/CHANGELOG.md` 에 `## <버전>` 항목이 있으면 그 항목을 설명으로 `gh release create windows-v<버전> … --latest=false --title "Claude Board for Windows <버전>"`. 항목이 없으면 실패로 끝낸다(macOS release.sh 와 같은 규칙).
- 배포 순서(관리자): `windows/CHANGELOG.md` 맨 위에 항목 → `windows/VERSION` 올리기(`windows/packaging/release.sh <버전>` — README·설치 가이드의 setup.exe·릴리스 링크도 바꾼다) → main 에 push → CI 가 빌드·릴리스.

## 9. 문서 의무

- `windows/README.md`(사용자용: 컨셉·필요 환경·설치·사용법·업데이트·제거·배포·macOS 와 다른 점), `windows/docs/설치 가이드.md`(모든 기능 자세히), `windows/CHANGELOG.md`(`## 2.12 — 2026-10-16` 첫 항목).
- 최상위 `README.md` 에 Windows 판 안내 한 절(설치 한 줄 + `windows/README.md` 링크), `CLAUDE.md` 에 Windows 구조·명령·"Latest 금지" 규칙, `.gitignore` 에 `!/windows/`·`!/.github/`.
- 사용자에게 보이는 기능이 바뀌면 `windows/CHANGELOG.md`·`windows/README.md`·`windows/docs/설치 가이드.md` 를 함께 고친다.
- 자율주행(§2.3)은 macOS 의 `docs/자율주행/설계.md` 를 기준으로 하고, Windows 판을 만들 때 이 명세를 먼저 맞춘다.

## 10. 이 Mac 에서 확인할 수 있는 것과 없는 것

확인 가능: core·hook 단위 테스트(macOS 에서 `cargo test`), Windows 대상 `cargo check`(링크 전까지), 프론트엔드 빌드, Tauri 앱을 macOS 에서 띄워 가짜 세션 파일로 화면 확인.
Windows CI 러너에서 실기로 확인: `#[cfg(windows)]` 시험(콘솔 제목·입력·터미널 창 찾기·headless, 프로세스 표·생성 시각·명령줄·종료, hook 이 `claude.exe` 를 조상으로 찾기, DPAPI, 새 콘솔 실행의 인자 전달)과 smoke 작업(§8.3)의 설치·화면 확인.
사람이 Windows 에서 확인해야 함: Windows Terminal 탭 고르기, 세션 창 앞으로 가져오기, 트레이·전역 단축키·Mica, 소리, 실제 Claude Code 의 `.credentials.json` 형식·계정 전환, 실제 Claude Code 세션의 일시중지·재개·중단, 로그인 자동 실행, 배율이 큰 모니터의 패널 위치.

## 11. 재구현 의무 대응표

| ID | 의무 | macOS 근거 | Windows 위치 | 검증 |
|---|---|---|---|---|
| H1 | hook 상태 기록 규칙 | state.py:177-284 | crates/core `state` | 기준 데이터 테스트(REA 캡처 14단계) |
| H2 | 서브에이전트 기록 | state.py:146-174 | crates/core `state` | 기준 데이터 테스트 |
| H3 | carry_args·resume_cmd(Windows 인용) | state.py:57-97 | crates/core `state` | 단위 테스트 |
| H4 | prune | state.py:113-135 | crates/core | 단위 테스트 |
| H5 | 프로세스 찾기·alive·headless·제목 | state.py:24-110 | crates/core `proc`·`console` | Windows 실기 시험(CI) |
| N1 | 알림 거르기·본문 다듬기·요약 | notify.py | crates/core `notify` | 단위 테스트 |
| N2 | 패널 띄우기(분리 프로세스) | notify.py:126-136 | crates/hook | smoke 화면 확인 |
| N3 | 알림 패널 화면·위치·슬롯·소리·클릭 | ClaudeNotifier.swift | app `--notify` + notify.html | macOS 실행 화면, smoke 화면 확인(비활성·스타일) |
| I1 | hook 등록·제거(exec form, 표시, 백업) | hooks.py | crates/core `hooks` | 단위 테스트 |
| I2 | setup·uninstall | user-setup.sh, Uninstall.command | crates/hook | smoke 설치 확인 |
| I3 | install.ps1·uninstall.ps1·해시 확인 | install.sh | windows/packaging | smoke 설치 확인(Windows PowerShell 5.1) |
| B1 | 세션 읽기·정렬·보정 | ClaudeBoard.swift:941-1207 | src-tauri `store` | 단위 테스트(가짜 세션 폴더) |
| B2 | 모델·effort·컨텍스트 | 97-246 | src-tauri `readers` | 단위 테스트 |
| B3 | 사용 한도 | 262-354 | src-tauri `usage` | 응답 파싱 테스트 |
| B4 | 계정 전환·추가·지우기 | 369-789 | src-tauri `accounts` | 파일 조작 테스트(DPAPI 는 Windows) |
| B5 | 한도 자동 전환 | 791-918 | src-tauri `autoswitch` | 선택 규칙 테스트 |
| B6 | 터미널 제어 | 4525-4910 | src-tauri `terminal` | Windows 실기 시험(콘솔 입력·창 찾기·새 콘솔), 탭 고르기 미확인 |
| B7 | 업데이트(Latest 금지) | 4097-4184 | src-tauri `updater` | 릴리스 목록 파싱 테스트 |
| B8 | 설정 저장 | UserDefaults 키 | src-tauri `settings` | 단위 테스트 |
| B9 | 창·트레이·단축키·단일 실행·자동 실행 | 4226-5204 | src-tauri `main` | macOS 실행(트레이), smoke 화면 확인(창·단일 실행) |
| U1 | 보드 화면 전체(단계 3개) | 1247-4460 | app/src | macOS 실행 스크린샷 |
| U2 | 캐릭터 8종·말풍선 | 2643-3388 | app/src/lib/mascot | 스크린샷 |
| T1 | TUI | board-tui.py | crates/hook `tui` | macOS 실행 |
| R1 | CI·자동 릴리스(Latest 금지) | release.sh | .github/workflows/windows.yml | 브랜치 push 후 CI |
| D1 | 문서 | README.md, packaging/README.txt | windows/*.md, 최상위 README·CLAUDE.md | 검토 |
| A1 | 자율주행 엔진·실행 폴더 약속(§2.3) | ClaudeAutopilot.swift, docs/자율주행/설계.md | (아직 없음 — macOS 먼저, 같은 약속으로 뒤에) | 미구현 |

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
