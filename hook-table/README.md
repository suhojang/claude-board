# hook 공유 시험 표

macOS hook(`state.py`·`notify.py`·`hooks.py`)과 Windows 판 Rust 코어(`windows/crates/core` 의 `state`·`notify`·`hooks`)는 같은 hook 규칙을 따로 구현한다.
이 폴더의 표는 Python 을 기준으로 hook 을 처음부터 끝까지 돌린 입력과 결과다. Rust 시험(`windows/crates/core/tests/hook_table.rs`)이 같은 표를 읽어 자기 결과와 견준다.
두 판이 함께 쓰는 계약이라 어느 한쪽 폴더가 아니라 저장소 맨 위에 둔다(`tests/` 는 macOS 파일 시스템에서 Swift 시험 폴더 `Tests/` 와 겹쳐 쓰지 않는다).

| 파일 | 내용 |
|---|---|
| `gen.py` | 생성기. 시나리오(입력)도 여기 있다 |
| `state.json` | `state.py <상태>` 시나리오. 단계마다 hook 인자·입력·가짜 claude 실행 인자·`CLAUDE_PROJECT_DIR`, 그 뒤 `sessions/` 폴더 전체 |
| `notify.json` | `notify.py <stop\|notification>` 시나리오. 심은 파일·입력, 그 뒤 띄울 패널의 인자(띄우지 않으면 `null`) |
| `hooks.json` | `hooks.py install` 이 settings.json 에 넣는 이벤트·matcher·하위 명령·type·timeout, `env` |

## 명령

```sh
python3 -I hook-table/gen.py                                  # 표 다시 만들기(저장소 맨 위에서, python3 3.9 이상, 표준 라이브러리만)
git diff --exit-code hook-table/                              # 커밋된 표와 같은지
cd windows && cargo test -p claude-board-core --test hook_table   # Rust 가 같은 표로 견준다(unix 만)
```

`gen.py` 는 실행할 때마다 임시 폴더에서 돌고 끝나면 지운다. 저장소 안에 `sessions/`·`__pycache__` 를 남기지 않는다(남기면 CI 가 실패한다).
같은 코드로 두 번 만들면 바이트까지 같다(키 정렬, LF, 끝 줄바꿈 하나). `--out <폴더>` 로 다른 곳에 쓸 수 있다.

## 돌리는 방법

`state.py`·`notify.py`·`hooks.py` 를 import 해서 `main()` 을 같은 프로세스에서 돌리고 바깥만 가짜로 바꾼다.

- 시계: `time.time` 은 시나리오 시작 시각 + 단계 번호(초)
- 프로세스: `find_claude` 는 가짜 claude(pid 4242, tty `hook-table-tty`), `proc_args` 는 그 단계의 실행 인자, `alive` 는 4242 면 산 것이고 그 밖은 진짜 생사, `set_title` 은 아무것도 하지 않는다
- 경로: `SESSIONS`·`BIN`·`hooks.PATH` 는 임시 폴더, `HOME` 도 임시 폴더(notify 가 읽는 `~/.claude/sessions/<pid>.json`)
- 패널: `subprocess.Popen` 은 실행하지 않고 인자만 잡는다
- 그 밖: stdin(JSON 문자열), `sys.argv`, `os.getcwd`(입력에 cwd 가 없을 때 `/hook-table/cwd`), 환경 변수 `CLAUDE_PROJECT_DIR`

notify 의 다른 세션 생사(`os.kill`), 대화 기록·Claude Code 세션 파일·서브에이전트 메타 파일 읽기는 진짜로 한다.
Rust 시험은 같은 가짜를 `state::Env` 로 넣고 `state::run`·`notify::prepare`·`hooks::add` 를 부른다. 시계는 진짜이고 단계마다 앞뒤 시각을 재어 그 사이 값을 `@<k>` 로 바꾼다.

## 표시 규칙

실행마다 바뀌는 값은 표에 표시로 적고, 표를 돌리는 쪽이 자기 값으로 채운 뒤 결과를 다시 표시로 바꿔 견준다.

| 표시 | 뜻 |
|---|---|
| `@DIR` | 표를 돌리는 쪽의 임시 폴더(경로 앞부분) |
| `@<k>` | k 번째 단계(0부터)에서 hook 이 쓴 시각 |
| `@seed` | 심어 둔 시각 = 시나리오 시작 시각 - 1000초. `@seed-<n>` 은 시작 시각 - n초, `@ms:seed-<n>` 은 그 밀리초(Claude Code 의 `statusUpdatedAt`) |
| `@LIVE_PID`·`@PARENT_PID` | 표를 돌리는 프로세스 자신과 그 부모의 pid(산 것) |
| `@DEAD_PID` | 띄웠다가 거둔 자식의 pid(죽은 것) |
| `4242` | 가짜 claude 의 pid. 가짜 생사 판정에서 늘 산 것이라 진짜 생사를 묻는 자리(prune 할 기록, notify 의 다른 세션)에는 쓰지 않는다 |
| `age` | 심은 파일의 수정 시각 = 시작 시각 - age초 |

`sessions/` 의 `.json` 파일은 내용을 적고, 그 밖의 파일과 JSON 이 아닌 파일은 있음(`true`)만 적는다.
`resume_cmd` 는 인용을 풀어 토큰 목록으로 적는다. 패널은 인자를 이름별 값으로 적는다.

## 뺀 것과 이유

두 판이 일부러 다르게 하는 것은 표에 넣지 않는다.

- 프로세스 찾기·headless 판정·터미널 제목 쓰기: 플랫폼마다 다르게 구현한다(SPEC §3.4). 표는 가짜 claude 를 넣고 그 pid·tty·실행 인자가 기록에 실리는 길만 본다
- `resume_cmd` 의 인용 방식: macOS 는 POSIX 셸(`shlex.join`), Windows 는 명령줄 규칙(SPEC §2.1). Rust 쪽은 `resume_args` 앞에 `claude` 를 붙인 토큰과, 그것을 이 플랫폼 규칙으로 이은 문자열이 `resume_cmd` 와 같은지를 본다
- Rust 에만 있는 `resume_args`·`pid_ctime`: Windows 판이 더한 필드다(SPEC §2.1)
- 패널 인자의 순서·`--sound` 이름·`--activate`·`--notify`·실행 파일 경로: 패널 프로그램이 다르다(SPEC §3.6, §4)
- 패널의 빈 `--session`: macOS 는 세션 id 를 모르면 빈 문자열로 넘기고 Windows 는 인자를 뺀다. 두 패널 모두 "세션 없음"으로 보므로 표에는 `null`
- 경로 구분자(`/`·`\`·`..`)가 든 `session_id`: Windows 판만 받지 않는다(sessions 밖에 쓰지 않게). 표에 넣지 않는다
- 세션 파일의 키 순서: 표는 키를 정렬해 적는다. 키 순서는 `windows/crates/core/tests/state_sequence.rs` 가 본다
- hook 입력 앞의 UTF-8 BOM·notify 의 깨진 입력: Windows 는 hook 실행 파일(`crates/hook` 의 `main.rs`)이 코어를 부르기 전에 다룬다(SPEC §3.2, §3.6). 표는 코어만 본다

## 표가 덮지 못하는 것

- 프로세스 찾기 층(`find_claude`·`proc_args`·tty·headless): 위에서 가짜로 바꾼 자리라 이 층의 고침(예: 백그라운드 작업의 pty 호스트를 세션으로 잡지 않기)은 표로 잡지 못한다. 플랫폼별 시험이 맡는다(macOS 는 아직 없음, Windows 는 `crates/core/src/proc.rs` 의 단위 시험과 `crates/core/tests/windows_proc.rs`)
- 패널을 실제로 띄우는 일, 동시에 도는 hook 의 파일 잠금, 터미널 제목

## 유지 규칙

- hook 규칙(`state.py`·`notify.py`·`hooks.py`)을 고치면 `python3 -I hook-table/gen.py` 로 표를 다시 만들어 **같은 커밋**에 넣는다. 표를 손으로 고치지 않는다. 새 규칙이면 그 규칙을 보여 주는 시나리오를 `gen.py` 에 더한다
- 표가 바뀌면 Rust 코어를 같은 PR 에서 맞춘다(`windows/docs/SPEC.md`·`windows/CHANGELOG.md` 도 함께). 일부러 다르게 둘 것이면 `gen.py` 의 `EXCLUDED` 와 이 README 의 "뺀 것"에 이유를 적고 두 쪽이 그 값을 빼게 한다
- CI(`.github/workflows/rules.yml`)가 push·PR 마다 표를 다시 만들어 커밋된 것과 다르면 실패하고, Rust 시험을 돌린다
