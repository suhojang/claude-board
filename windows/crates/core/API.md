# claude-board-core 공개 API

보드 백엔드(`app/src-tauri`)와 hook 실행 파일(`crates/hook`)이 함께 쓰는 라이브러리다. 규칙의 출처는 `windows/docs/SPEC.md`.
Tauri 쪽에서는 `claude-board-core = { path = "../../crates/core" }` 로 넣고 `claude_board_core::…` 로 쓴다.
세션 기록처럼 모양이 느슨한 JSON 은 `Record`(= `serde_json::Map`, 키 순서 유지)로 다룬다.

## 경로 — `paths`

| 함수 | 경로 |
|---|---|
| `home()` | `CLAUDE_BOARD_HOME` > `%USERPROFILE%`(그 밖 `$HOME`) |
| `claude_dir()`, `settings_file()`, `credentials_file()` | `~/.claude`, `~/.claude/settings.json`, `~/.claude/.credentials.json` |
| `claude_json()` | `~/.claude.json` |
| `notifier_dir()` | `CLAUDE_BOARD_NOTIFIER_DIR` > `~/.claude/notifier` |
| `sessions_dir()`, `hook_exe()`, `app_path_file()` | `notifier/sessions`, `notifier/claude-board-hook.exe`, `notifier/app-path` |
| `mute_all_file()`, `notify_position_file()`, `board_theme_file()` | `notifier/mute-all`, `notifier/notify-position`, `notifier/board-theme` |
| `local_bin()` | `~/.local/bin` |
| `basename(&str)` | 마지막 이름(Windows 는 `\`·`/` 모두 구분자) — 세션의 `project` 규칙 |
| `claude_exe() -> Option<PathBuf>` | Claude Code 실행 파일 — PATH 의 `claude.exe`(그 밖 `claude`), 없으면 `~/.local/bin/claude.exe` |

환경변수로 가짜 폴더를 쓰면 macOS 에서도 보드를 띄워 볼 수 있다. hook 등록까지 시험할 때는 `CLAUDE_BOARD_HOME` 을 쓴다(명령 경로에 `.claude/notifier/` 표시가 들어가야 한다).

## 프로세스 — `proc`

- `ProcessTable::capture()` — 스냅숏 한 번. `parent(pid)`, `name(pid)`(예 `claude.exe`), `ancestors(pid, max)`, `children(pid)`, `pids_named("WindowsTerminal.exe")`.
- `alive(pid, ctime: Option<f64>) -> bool` — `ctime` 이 있으면 생성 시각도 맞아야 산 것. 권한이 없으면 산 것.
- `record_alive(&Record) -> bool` — 세션 기록의 `pid`·`pid_ctime` 으로 판정(pid 가 없거나 null 이면 참). **보드의 "ended 가 아닌데 pid 가 죽었으면 ended" 보정에 이것을 쓴다.**
- `creation_time(pid)`, `command_line(pid) -> Option<Vec<String>>`(argv[0] 포함), `terminate(pid)`.
- `shell_count(&table, pid)` — 자식 가운데 명령줄에 `shell-snapshots` 가 든 수(SPEC §5.1 백그라운드 셸, 3초마다 백그라운드에서 부를 것 — 자식마다 명령줄을 읽는다).
- `find_claude()` — hook 전용(자기 조상에서 Claude 프로세스 찾기).

## 콘솔 — `console` (Windows 실기 미확인, Windows 밖에서는 false/None)

모든 함수가 프로세스 전역 잠금 아래 `FreeConsole → AttachConsole(pid) → … → FreeConsole` 하므로 한 번에 한 세션씩 돈다.
UI 스레드에서 부르지 말고 백그라운드에서 부른다(`Pause` 동안 잠금을 쥐고 있다).

- `send_input(pid, &[Input]) -> bool` — `Input::Escape`, `Input::Enter`, `Input::Text(&str)`, `Input::Pause(Duration)`.
  - 일시중지: `[Escape]` / 재개: `[Text(carryOnPrompt), Enter]` / 중단: `[Escape, Pause(400ms), Text("/exit"), Pause(200ms), Enter]`
  - 자동 전환 이어 가기: `[Escape, Pause(500ms), Text(문구), Pause(200ms), Enter]`
- `can_attach(pid)` — `canDrive(s) = !s.headless && s.state != ended && can_attach(pid)`.
- `terminal_window(pid) -> Option<isize>` — 그 세션의 터미널 창 HWND 값(포커스·앞으로 가져오기에 쓴다). 콘솔 창 → 소유 창 → 조상의 보이는 최상위 창.
- `title(pid)`, `set_title(pid, &str)` — Windows Terminal 탭 찾기에서 제목을 `CBFOCUS-…` 로 잠깐 바꿨다 되돌릴 때.
- `is_headless(pid, argv)`, `print_mode(argv)` — hook 이 세션 기록의 `headless` 를 정할 때 쓰는 규칙(SPEC §3.4).

## 세션 기록 — `state`

- `run(sessions, state_arg, stdin_bytes, &SystemEnv)` — `state <상태>` hook 본체(잠금·읽기·쓰기·prune·`.paused` 삭제·제목).
- `apply(rec, state_arg, input, now, &dyn Env) -> Outcome { record, title, remove_paused }` — 순수 함수(시험용 `Env` 를 넣는다).
- **세션 다시 열기는 `resume_args` 로만 한다.** 기록의 `resume_args`(또는 `resume_args(&Record)`)는 `claude` 뒤에 붙일 인자 배열이고
  `"--resume", "<sid>"` 로 끝난다(기록에 없으면 `["--resume", "<sid>"]`). 보드는 `paths::claude_exe()` 를
  `Command::new(exe).args(resume_args)` + `CREATE_NEW_CONSOLE` + `current_dir(cwd)` 로 직접 띄운다 — cmd.exe·PowerShell·wt.exe 명령줄을 거치지 않는다.
  모델 이름 같은 값은 저장소의 `.claude/settings.json` 에서 올 수 있어(예: `x" & calc & "`) 셸 문자열로 실행하면 명령 주입이 된다.
- `resume_cmd` 는 **표시용** 문자열(플랫폼 인용)이다 — 실행하지 않는다.
- `resume_cmd(&Record)`(플랫폼 인용), `resume_cmd_with(&Record, quote::join_windows)`, `carry_args(&argv)`(같은 플래그·`(이름, 값)` 쌍·`--x=v` 는 한 번만, `--add-dir x` 꼴은 거르지 않음), `title_text(state, &Record)`, `prune(sessions, now, alive)`, `lock_path(sessions, sid)`.
- hook 은 세션 파일을 `.<sid>.lock` 배타 잠금 아래에서 읽고 고쳐 쓴다(Windows `LockFileEx`, 그 밖 `flock` — std `File::lock`).

## 알림 — `notify`

`plain`, `first_sentence`, `last_assistant_text(path)`, `session_summary(sessions, current_id, current_state, alive, live)`(`live` 는 다른 세션의 working·waiting 을 겹쳐 보는 클로저, 보통 `claude_live::live_override`), `prepare(kind, input, notifier, cwd) -> Option<Panel { app, args }>`, `panel_args(…)`.
패널 인자 모양: `--notify --title … --subtitle … --body … --summary waiting=n,working=n,idle=n --sound done|wait --kind done|wait --duration 10 --position …`.

## Claude Code 신호 겹쳐 보기 — `claude_live`

hook 이 오지 않는 때(Esc 중단, 승인한 도구가 도는 동안)를 메운다. 규칙은 `live_state` 한 곳이다.

- `interrupted_at_in(tail, mtime) -> Option<f64>` — 대화 기록 끝 256_000 바이트(`INTERRUPT_TAIL`)의 마지막 대화 줄이 `[Request interrupted by user…` 이면 그 `timestamp`(없으면 `mtime`). 순수 함수. `interrupted_at(path)` 는 파일을 읽는 포장(없으면 None).
- `ClaudeSession { session_id, name, status, status_at }` — `~/.claude/sessions/<pid>.json`(Claude Code 가 씀, 읽기만)에서 읽는 값. `parse_claude_session(bytes)`, `claude_session_path(claude_dir, pid)`, `read_claude_session(claude_dir, pid)`(pid 0 이면 None).
- `status_change(&cs, sid, since) -> Option<(status, at)>` — sid 가 비어 있지 않고 파일의 sessionId 와 같으며 `status_at > since` 일 때. `session_name(&cs)` — 비어 있지 않은 name.
- `live_state(state, interrupted, status) -> Option<(&'static str, f64)>` — working·waiting 일 때만 클로저를 부른다. 중단 또는 status idle → `("idle", at)`, waiting 인데 busy → `("working", at)`. 중단이 먼저다.
- `live_override(&Record, claude_dir)` — 세션 기록의 state·pid·session_id·since·transcript_path 로 `live_state` 를 돌리는 편의 함수(hook 알림 요약과 `tui` 가 쓴다).

## hook 등록 — `hooks`

`install(settings, command, stamp)`, `uninstall(settings, stamp)`, `strip`, `add`, `command_path(exe)`(슬래시 경로), `MARKERS`, `ENV_KEY`, `HOOKS`.
깨진 JSON·예상과 다른 `hooks`/`env` 모양이면 `Err` 를 돌려주고 파일을 건드리지 않는다.

## 그 밖

- `fileio::write_atomic(dst, prefix, bytes)` — 같은 폴더 임시 파일 후 바꿔 넣기(Windows 공유 위반이면 50ms×5 다시). `fileio::read_record(path)`, `fileio::replace(tmp, dst)`.
- `quote::{windows, posix, join, join_windows, join_posix}` — 명령줄 인용(이어 열기 명령을 만들 때 Windows 규칙).
- `clock::now()`(유닉스 초), `clock::local(ts)` → `LocalTime { … }.hm()/.hms()/.stamp()`, `clock::iso_date(&str) -> Option<f64>`(ISO8601 → 유닉스 초, 시간대 필수).
- `text::{collapse_ws, take_chars, truthy, is_py_space}` — Python 문자열 규칙.

## 실행 파일 `claude-board-hook`

`state <s>`·`notify <k>`(출력 없음, 늘 0) / `setup --app <ClaudeBoard.exe>` / `uninstall` / `install-hooks` / `uninstall-hooks` / `tui` / `--version`(출력 `claude-board-hook <버전>`).
Windows 가 아닌 곳에서 `setup`·`uninstall`·`install-hooks`·`uninstall-hooks` 는 `CLAUDE_BOARD_HOME`(또는 `CLAUDE_BOARD_NOTIFIER_DIR`)이 없으면 거부한다 — macOS 판의 `~/.claude` 를 지키기 위해서다.
