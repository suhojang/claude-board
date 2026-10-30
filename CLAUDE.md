# CLAUDE.md

이 파일은 Claude Code(claude.ai/code)가 이 저장소의 코드를 다룰 때 따를 지침을 담는다.

## 개요

Claude Board 는 Claude Code 세션 여러 개의 상태(승인 대기·작업 중·대기)를 한 화면에 모아 보여 주는 macOS 14+ 앱이다.
Claude Code hook 이 Python 스크립트를 실행해 세션 상태를 파일로 남기고, Swift 앱(보드·알림 패널)과 TUI 가 그 파일을 읽는다.
보드 창 왼쪽에는 내장 터미널(libghostty)이 있어, 보드에서 띄우거나 이어 연 세션은 보드가 pty 를 직접 쥔 탭(보드 탭)에서 돈다.
문서·코드 주석·커밋 메시지는 모두 한국어로 쓴다.

## 명령

Xcode 프로젝트는 없다. 보드(`Sources/ClaudeBoard/`)는 내장 터미널 엔진(libghostty-spm) 때문에, 자율주행 엔진(`Sources/ClaudeAutopilot/`)은 규칙 라이브러리를 import 하려고 저장소 맨 위 `Package.swift` 의 SwiftPM 실행 타깃으로 빌드하고
(소스는 그 폴더들과 순수 규칙 라이브러리 `Sources/BoardRules/`·`Sources/AutopilotRules/`, Command Line Tools 만으로 된다), 나머지 Swift 파일은 하나가 실행 파일 하나이고 `swiftc` 로 바로 빌드한다.
시험은 두 규칙 라이브러리의 Swift Testing 시험(`Tests/BoardRulesTests/`·`Tests/AutopilotRulesTests/`, `swift test`)과 md-render 플러그인의 `claude plugin test` 가 있고, 린트 설정은 없다.
모든 `swift build`/`swift test`/`swift package` 에 `--disable-keychain`(빼면 엔진 내려받기에서 키체인 조회로 10분 넘게 멈춘다)과 `--triple`(배포 빌드와 같은 `.build` 를 쓰므로 생략한 빌드와 섞으면 오류)을 붙인다.

```sh
# 빠른 확인 — 보드는 import GhosttyTerminal, 자율주행 엔진은 import AutopilotRules 때문에 swiftc -typecheck 를 못 쓰고 빌드로 확인한다(한 번에 둘 다.
# 첫 빌드는 엔진 약 77 MB 를 내려받아 1분 남짓, 그 뒤 약 20초). 결과 실행 파일은 swift build … --show-bin-path 의 ClaudeBoard·ClaudeAutopilot
swift build -c release --disable-keychain --triple "$(uname -m)-apple-macosx14.0"
swift test --disable-keychain --triple "$(uname -m)-apple-macosx14.0" --toolset clt-testing.json   # BoardRules·AutopilotRules 시험. 끝 줄 "Test run with N tests" 의 N 이 0 보다 큰지 본다
#  → 자율주행 규칙 시험(Tests/AutopilotRulesTests) 이 돌았는지는 출력의 "자율주행 …" Suite 줄로 본다(--filter AutopilotRulesTests 로 그것만)
#  → Windows 판과 함께 쓰는 보드 규칙 표 rules-table/*.json 도 읽는다(RulesTableTests.swift, 꼴은 rules-table/README.md — 사람이 쓴 의도라 Swift 출력으로 만들지 않는다)
#  → --toolset 은 Command Line Tools 만 있는 Mac 용(SwiftPM 이 CLT 의 Testing.framework 를 못 찾아 빼면 no such module 'Testing'). Xcode 가 있는 Mac 에서는 뺀다.
#    같은 -F·-rpath 를 Package.swift 의 unsafeFlags 로 넣으면 시험이 0개 돌고 성공으로 끝나므로 toolset 파일로만 넣는다. 처음에는 앱·엔진까지 debug 로 빌드해 1분 남짓
swiftc -typecheck ClaudeNotifier.swift
python3 -m py_compile state.py notify.py hooks.py board-tui.py   # 만들어진 __pycache__ 는 지운다
python3 -I hook-table/gen.py && git diff --exit-code hook-table/   # hook 공유 시험 표 다시 만들기·차이 확인(Rust 쪽: cd windows && cargo test -p claude-board-core --test hook_table)
claude plugin validate plugins/md-render --strict && claude plugin test plugins/md-render   # Claude Code 가 저장소 사본에 만드는 tsconfig.json 은 올리지 않는다

# 시험 빌드 — 설치된 보드·hook 은 건드리지 않는다
./build-board.sh --dev
#  → dist/dev/Claude Board Dev.app (번들 id com.claude.board.dev — 잠금 파일·분산 알림 이름이 달라 설치된 보드와 함께 뜬다)
#  → 띄우기: open "dist/dev/Claude Board Dev.app"   끄기: pkill -f "Claude Board Dev.app/Contents/MacOS"

# 로컬 빌드·설치 — 실제 환경을 덮어쓴다
./build-board.sh
#  → /Applications/Claude Board.app 을 다시 빌드하고(서명 인증서가 없으면 ad-hoc 서명)
#  → ~/.claude/notifier/ 에 claude-notifier 와 Python 스크립트를 복사한다 (지금 도는 hook 이 바로 새 스크립트를 쓴다)
#  → 떠 있는 보드는 다시 띄우지 않는다. 바꾼 앱을 보려면: pkill -f "Claude Board.app/Contents/MacOS"; open -a "Claude Board"
#    (pkill -x ClaudeBoard 는 시험 빌드까지 끈다)

# 배포 (관리자) — CHANGELOG.md 맨 위에 "## <버전> (<날짜>)" 항목을 쓰고 commit·push 한 뒤
packaging/release.sh <버전>   # 버전은 시맨틱 버전 MAJOR.MINOR.PATCH(3.0.0 부터). make-dmg.sh 로 universal(arm64+x86_64) 빌드·서명 → gh release 생성
```

macOS CI `.github/workflows/rules.yml` 이 hook·보드 규칙 파일이 바뀐 push·PR 마다 표 차이·`hooks.py` 사본(`cmp`)·core `cargo test`·`swift test`(BoardRules·AutopilotRules 시험, 시험 수가 0 이면 실패. 보드·엔진도 debug 로 함께 빌드된다)를 돌린다.

`release.sh` 는 CHANGELOG 의 `## <버전>` 항목이 없으면 빌드 전에 멈추고, 그 항목을 릴리스 설명으로 쓴다. `gh auth login` 과 서명 인증서(`packaging/make-cert.sh` 로 만드는 "Claude Board Signing")가 필요하다.

## 구조

### 데이터 흐름

```
Claude Code hook ──> state.py  ──> ~/.claude/notifier/sessions/<session_id>.json ──> ClaudeBoard (1초마다 읽음), board-tui.py
                 └─> notify.py ──> claude-notifier 패널을 분리 프로세스로 띄움 (Stop·Notification 때)
```

- `hooks.py` 의 `HOOKS` 표가 이벤트 11개를 `state.py <working|waiting|idle|ended|subagent>` / `notify.py <stop|notification>` 에 연결해 `~/.claude/settings.json` 에 넣는다.
  자기 hook 은 명령 문자열에 `.claude/notifier/` 가 들어간 것으로 알아보고, `packaging/install.sh` 도 같은 표시로 처음 설치인지 판단한다.
- hook 스크립트는 Claude Code 를 막거나 실패시키면 안 된다 — `main()` 전체를 `except Exception: pass` 로 감싸고, 패널은 `start_new_session=True` 로 띄우고 바로 끝난다.
- `state.py` 는 서브에이전트가 동시에 hook 을 돌리므로 `.<sid>.lock` 을 `flock` 으로 잡고 읽고-고쳐-쓴다. 서브에이전트 이벤트(`agent_id` 있음)는 세션 상태 대신 `subagents` 에만 남긴다.
- 스크립트는 자기 파일 옆의 `sessions/` 를 쓴다 — 저장소에서 바로 실행하면 저장소 안에 `sessions/` 가 생긴다.
- `hook-table/`(맨 위) — hook 규칙의 공유 시험 표. `gen.py` 가 state.py·notify.py·hooks.py 의 `main()` 을 바깥만 가짜로 바꿔 끝까지 돌려 만들고, Windows 판 core 시험(`windows/crates/core/tests/hook_table.rs`)이 같은 표로 견준다. hook 규칙을 고치면 표를 다시 만들어 같은 커밋에 넣고 Rust 를 맞추거나 다른 점을 그 README 에 적는다.

### 파일로 주고받는 약속 (`~/.claude/notifier/`)

여러 프로그램이 파일만으로 상태를 나누므로, 파일 이름이나 JSON 필드를 바꾸면 읽는 쪽을 모두 고쳐야 한다.

| 파일 | 쓰는 쪽 | 읽는 쪽 |
|---|---|---|
| `sessions/<sid>.json` (`state`·`pid`·`tty`·`headless`·`cwd`·`project_dir`(claude 를 띄운 폴더 = hook 의 `CLAUDE_PROJECT_DIR`, cd·워크트리에도 그대로 — 보드·TUI 카드 묶음)·`topic`·`last_message`·`subagents`·`limit_hit`·`resume_cmd`·`transcript_path`·`model` …) | state.py | 보드, board-tui.py, notify.py(상태별 개수) |
| `sessions/<sid>.mute`, `mute-all`, `notify-position` | 보드 | notify.py |
| `login-launch-off` | 보드 (설정 > 일반 > 로그인할 때 열기를 끌 때, LaunchAgent plist 도 지움) | `user-setup.sh` (있으면 LaunchAgent 를 등록하지 않음), `Uninstall.command` 가 지움 |
| `sessions/<sid>.paused` | 보드 (일시중지 때, 업데이트·종료로 끊기는 보드 탭 세션에도) | state.py (새 요청이 오면 지움), 보드 (일시중지 카드·재개) |
| `plugins/md-render/` | 보드 (시작할 때 앱 안 사본에서 바뀐 파일만 옮김. Claude Code 가 안에 `tsconfig.json`·`.claude-plugin/types/` 를 더 씀, `Uninstall.command` 가 지움) | 보드 탭 claude (`--plugin-dir`) |
| `context/<sid>.json` | 사용자의 상태줄 스크립트 (이 저장소에 없음) | 보드 (컨텍스트 사용량·effort) |
| `accounts.json`(저장 계정 표시 정보, 토큰은 키체인)·`accounts.json.broken-<시각>` | 보드 (계정 저장·지우기. 깨진 목록은 쓰기 전에 broken 으로 옮김) | 보드 |
| `~/.claude/sessions/<pid>.json` (이 폴더 밖, `name`·`status` …) | Claude Code (도는 세션마다, 끝나면 지움) | 보드 (카드 세션 이름 라벨·탭 줄 이름, 이름 바꾸기 전 승인 창 확인 `status`, pid 없는 세션의 생사(sessionId 로 찾음)) |
| `/tmp/.claude-usage-cache.json` | 보드·상태줄 스크립트(·자율주행 엔진이 사용량 API 를 부를 때, 설치판만)가 함께 씀. 시험 빌드 엔진은 읽기만 하고(장난감 사용량 파일 `usage_path` 가 있으면 그것), 시험 빌드 보드는 설정 `usageOffline` 이면 읽기만 | 보드, 자율주행 엔진(멈출 선) |
| (이 폴더 밖) `~/Library/Application Support/Claude Board/autopilot/runs/<실행 id>/`(시험 빌드는 `Claude Board Dev`, 엔진 전용 `…/autopilot/private/<실행 id>/`, 실행 기록 저장소 `…/autopilot/private/records.sqlite`(판 3 부터 여러 실행의 같은 경로 자리 `path_claims`·멈출 선 자리 `stopline_slots`, 엔진들이 BEGIN IMMEDIATE 로 함께 씀)) — `run.json`·`queue.json`·`control.json`·`inbox/`·`events.jsonl`·`units/<n>/{result,engine}.json` … 자세히는 `docs/자율주행/설계.md` 5장. `~/.claude` 는 Claude Code 보호 경로라 옮겼다. 출발 전 폴더는 보드가 `…/autopilot/drafts/<실행 id>/` 에 쓰고 출발할 때 `runs/` 로 옮긴다 | 자율주행 엔진(출발 전 `run.json`·`queue.json`·`cards.json`·`answers.json` 과 `control.json`·`inbox/<시각>.md`(방향을 바꿀 말), `projects/<프로젝트>.json` 의 출발 가지·승격 차례는 보드, `result.json` 은 모델) | 보드(2단계), 엔진 |
| (이 폴더 밖) `~/Library/Application Support/Claude Board/autopilot/connections.json` — 설정 › 연결의 연결 목록(서비스·계정·범위·만료일·마지막 확인, 토큰 없음). 토큰·webhook 주소는 키체인 일반 암호(서비스 `Claude Board-connection`, 시험 빌드 `Claude Board Dev-connection`, 계정 이름은 연결 id `atlassian`·`notion`·`figma`·`webhook-<n>`, `/usr/bin/security` 표준 입력으로 씀)에만. 실행 폴더의 올릴 글은 `runs/<실행 id>/outbox/<글 id>.json`(엔진이 만들고 상태도 엔진만 씀 — 보드 단추는 앱 안 엔진의 `outbox` 명령을 부름, 글마다 `.<id>.lock` flock) | 보드(연결 목록·키체인 쓰기), 엔진(outbox) | 엔진(올릴 때마다 연결 목록·키체인 읽기), 보드(검수 화면) |
| (이 폴더 밖) 근거 묶음 폴더 — 출발 준비 `…/autopilot/drafts/<준비>/context/links/`(주제로 시작의 자료 링크, 5단계 ⑪-2 부터 출발하면 단위 `context` 로 `pinned/context/<n>/` 에 뜸. 같은 준비 폴더에 보드가 쓰는 `topic.json`(주제 글·작업 경로·새 폴더인지·스택·출발 가지), 엔진이 쓰는 `harness-points.json`·`draft-check.json`, 초안 세션이 쓰는 `draft/`(spec.md·units.json·cards.json·autopilot.md·scenarios/), `run.json` 의 `seed`(출발 전 자율주행 가지 기록).
5단계 ⑪-3 부터 같은 준비 폴더(두 원천 모두)에 엔진이 `prerun.json` 의 `probes`·`probe_cards`·`probe_history` 와 탐침 증거 `prerun/probes/<n>/`, 판정 비판 `critic.json`(`rounds[]`)과 `critic/r<차례>/`(`in/` 판정 사본·저장소 파일 목록, `out/` 비판 세션 결과 `findings.json`·`cases/<id>/`, `verify/` 증거, `pins/`), 판정 놓침 요약 `misses.json`, 세트 이어 쓰기 `revise.json`·`revise/`, 판정 고치기 `draft-fix/`·`draft-fix-in/`·`draft-r<n>/`(앞 초안)·`draft-fix-weak.json`·`draft-fix-check.json` 을 쓰고, 보드가 `run.json` 의 `misses_used` 를 쓴다), 단위 `runs/<실행 id>/units/<n>/context/`(출발 때 뜬 사본 + 단위 시작 전 티켓): 메타 형식 파일 `<n>-<출처>-<id>.json`(Figma 는 `.png` 하나 더, 0444, 절 글은 믿지 않는 자료 표시 ⟦ ⟧ 로 감쌈)과 `MANIFEST.json`(출처·읽은 시각·sha256·가린 비밀 수). MCP 읽기 세션 자리 `…/autopilot/private/read/<시각>-<id>/`(질문·답·세션 설정, 고정 MCP 설정은 끝나면 지움, 7일 뒤 지움) | 자율주행 엔진(`read` 명령·`beginUnit`) | 보드(준비 줄), 모델(단위 근거 묶음, 읽기만) |
| (이 폴더 밖) `~/.claude-board/worktrees/<실행 id>/<저장소>/` — 단일 저장소를 따로 작업 트리에서 돌릴 때 엔진이 만드는 작업 트리(경로에 공백 없게 실행 폴더 뿌리 밖). `start` 가 만들고 `finish`(합친 뒤·버린 뒤)·제거 도구가 지운다. 모델 쓰기 범위이므로 비밀을 두지 않는다 | 자율주행 엔진 | 모델(작업 트리 세션), 엔진 |

### 실행 파일

- `Sources/ClaudeBoard/` (21개 파일, 약 19,500줄 — 그 가운데 자율주행 `Autopilot.swift` 약 7,000줄·`AutopilotTopic.swift` 약 870줄·`AutopilotPublish.swift` 약 770줄·`AutopilotReport.swift` 약 260줄·`AutopilotNotify.swift` 약 210줄) — 보드 앱 전체. AppKit 창 위에 SwiftUI 화면을 얹었다. 한 파일 `ClaudeBoard.swift` 이던 것을 2026-10-29 에
  `tools/split-board.py` 로 맨 위 `// MARK: - ` 절마다 한 파일로 나눴다(코드는 옮기기만 했고 머리의 import 는 파일마다 같다 — 자율주행 `Autopilot.swift`·`AutopilotTopic.swift`·`AutopilotReport.swift`·`AutopilotPublish.swift`·`AutopilotNotify.swift` 만 `AutopilotRules` 를 더 import 한다. 자율주행 절은 가지 `autopilot-stage2` 에 main 을 합칠 때 같은 스크립트로 떨어졌고, `AutopilotTopic.swift` 는 2단계 5-2b, `AutopilotReport.swift` 는 3단계, `AutopilotPublish.swift` 는 4단계 ⑩-1, `AutopilotNotify.swift` 는 ⑩-3 에서 새로 만든 파일이다). 절 이름과 내용이 어긋난 곳이 있으니
  (`BoardView` 는 `BrainrotMascots.swift`, `Store` 는 `LimitSwitching.swift`, `AppDelegate` 는 `SettingsScreen.swift`) 파일 이름보다 아래 표나 grep 으로 찾는다.

  | 파일 | 절 | 들어 있는 것 |
  |---|---|---|
  | `BoardData.swift` | 데이터 | `Group` 별칭, 대화 기록·Claude Code 상태 읽기(`ModelReader`·`InterruptReader`·`ClaudeNames`·`ContextReader`), 세션 판정·사용량 진짜 어댑터, `UsageStore` |
  | `AccountSwitching.swift` | 계정 전환 | 키체인·설정·저장 계정 진짜 어댑터, 로그인 창 `LoginWebWindow`, `AccountStore` |
  | `LimitSwitching.swift` | 사용 한도 자동 전환 | `AutoSwitcher`, `appIcon`, `Palette`, `Store`, 시각 글자 `clock` |
  | `StatusAnimation.swift` | 상태 표시 (애니메이션) | `AmbientTicker`, `StateIcon` |
  | `BoardViews.swift` | 화면 | 사용량 줄(`UsagePanel`·`AccountChip`), 세션 줄(`SessionRow`·`SessionMenu`·`SubagentRow`), 카드 `GroupCard` |
  | `Folding.swift` | 접기·펼치기 | 머리 단추, `ClickableIf` |
  | `BoardFontScale.swift` | 보드 글자 크기 | `BoardFontScale` |
  | `Hints.swift` | 힌트 | `HintPanel`, 바탕 `Backdrop`, 현장 감독 `Foreman`·말풍선 |
  | `BrainrotMascots.swift` | 브레인롯 캐릭터 | `Mascot`·`BrainrotFigure`, 터미널 앱 `TerminalAppStore`, 터미널 독 `EdgeDock`, 창 투명도, 보드 화면 `BoardView` |
  | `AppUpdate.swift` | 업데이트 | `Updater`(`releaseRepo`), `UpdateBanner` |
  | `EmbeddedTerminal.swift` | 내장 터미널 | `TerminalHub`·`BoardTab`·`BoardSurface`·`BoardTerminalView`·`BoardRootView`·`PaneDivider`, `confirmShutdown`, `mdRenderDir`, `boardLog` |
  | `Autopilot.swift` | 자율주행 | 뿌리 `autopilotRoot`, 실행·단위 모델(`APRun`·`APUnit`), `AutopilotStore`(원문 지우기 `purge` 포함), 기록 저장소 읽기 `APRecords`, 보드 본체 줄(`APRunLine`·`APRunCard`·`APEndedLine`·`APBar`), 자율주행 중 `APRunningScreen`·지난 실행 `APHistoryScreen`, 출발 `APStart`·`APStartSheet`(출발 확인의 설정 줄·승격 차례·자율주행 지시 그 자리 바꾸기, 4단계 ⑩-2 링크 읽기 줄 `readLink`·에픽 읽기 `refreshEpic` 은 엔진 `read`), 합치기·승격 `APMerge`·`APMergeSheet`, 엔진·git 부르기(`apEngine`·`apGit`) |
  | `AutopilotTopic.swift` | 자율주행 · 주제로 시작 | 출발 준비 1단계의 주제로 시작 입력 판 `APTopic`·`APTopicStep1`·`APSlotEditor`(주제 글·자료 링크(줄 상태 `linkView`·MCP 서버 고르기, 엔진 `read servers`)·작업 경로·기술 스택), 5-2b 규칙의 진짜 어댑터(`APFolderFiles`·`APBoardGit`·`APRepoFiles`·`APHTTPFeeds`·`APCatalogCacheFile`·`APRawRunFiles`·`APSystemClock`) |
  | `AutopilotReport.swift` | 자율주행 · 보고서 뷰어 | 보고서 창 `APReportViewer`(WebKit, JavaScript 끔·처리기 없는 바탕 주소 `claude-board-report://report/`·문서 안 이동만·https 링크는 확인 뒤 기본 브라우저·콘텐츠 규칙으로 http(s)·ws(s)·ftp(s) 막음·파일로 저장), 보고서 규칙의 진짜 어댑터 `APFolderReportSource`(실행 폴더)·`APRecordsReportSource`(기록 저장소, 행은 `APRecords.reportSource` 가 읽음)·`apBoardReportHTML` |
  | `AutopilotPublish.swift` | 자율주행 · 연결과 올릴 글 | 설정 › 연결(`APConnections`: 연결 목록 쓰기·키체인 쓰기·엔진 `connection check`·`notify test` 부르기, `APConnectionCard`·`APWebhookCard`(webhook 글 꼴·시험 글), 설정 항목 `apConnectionItems`), 올릴 글 검수 시트(`APOutboxReview`: 3초마다 `runs/*/outbox` 시각만 보고 다시 읽기·엔진 `outbox post|skip|later` 부르기, `APOutboxSheet`, 본문 미리보기 `APDocView`(웹 보기 없이 글자 그대로), 티켓 글(실행 중에 새로 필요해진 티켓)) |
  | `AutopilotNotify.swift` | 자율주행 · 끝 알림 | 끝 알림 판 `APNotices`(실행마다 한 번·처음 뜬 때 앞에 끝난 실행은 아님·나중에 뒤 하루 다시 알림, 보드 설정 `autopilotEndSeeded`·`autopilotEndNotified`·`autopilotMergeReminded`, 누르면 규칙 `apEndOpen` 대로 검수·합치기·지난 실행)·`APNoticeView`, 실행 폴더 올릴 글 읽기 `apRunOutbox`, 실행 id 꼴 `apRunIDOK`(명령줄 `--action autopilot-open|autopilot-merge`) |
  | `BoardWindow.swift` | 창 | `window`, `SizeGuard` |
  | `BoardTiers.swift` | 창 크기 단계 | `BoardTier`·`BoardFit`, 미니 화면 `MiniBoard` |
  | `KeyboardShortcuts.swift` | 단축키 | `Shortcuts`·`ShortcutSettings` |
  | `SettingsScreen.swift` | 설정 화면 | `SettingsScreen`·`SettingsView`·`SettingsItem`·`LoginItem`, 다른 터미널·세션 조작 어댑터와 앱 함수(`driveSession` 등), `AppDelegate`(메뉴·메뉴 막대) |
  | `main.swift` | (시작 코드) | 부작용 있는 전역 먼저 읽기, `--action` 넘기기, 한 번만 뜨는 잠금, `resolveResourceBundleInApp`, 앱 시작 |

  한 모듈이라 다른 파일의 선언을 그대로 쓰지만 파일 범위 `private`·`fileprivate` 은 그 파일 안에서만 보인다. 최상위 실행문은 `main.swift` 에만 둘 수 있고,
  다른 파일의 전역 `let`/`var` 는 처음 쓸 때 초기화된다 — 시작할 때 바로 초기화해야 하는 부작용 있는 전역(파일 읽기·쓰기 등)은 `main.swift` 맨 앞에서 먼저 읽는다(지금 `unfocusedSplitOpacity`·`mdRenderDir`).
  주요 부분: `Store`(1초마다 `scanSessions` 로 세션 상태를 정해 경로별 `Group` 으로 묶음), `UsageStore`(30초마다 사용량 조회·계정 전환 세대 확인·화면 갱신, 사용량 규칙은 BoardRules),
  `AccountStore`(계정 전환의 @Published 상태·큐·3초 감시·안내와 계정 추가 로그인 흐름, 전환 규칙은 BoardRules — 키체인 `Claude Code-credentials` 와 `~/.claude.json` 의 `oauthAccount` 를 `/usr/bin/security` 로 바꿔 끼우고, 저장 계정은 키체인 서비스 `Claude Board-account`)·`AutoSwitcher`(3초 타이머·UserDefaults·로그·이어 가기 터미널 조작, 판단은 BoardRules),
  `Updater`(1시간 타이머·설정 읽기·알림 줄·`install.sh` 실행, 새 버전 판단은 BoardRules), 다른 터미널 포커스·조작과 세션 조작(`focusSession`·`activateOwner`·`driveSession`·`waitExit`·`pauseSession`·`stopSession`·`moveToBoard`·`resumeSession`·`reopenEnded`·`openSession` —
  확인 창·`ghosttyQueue`·백그라운드·메인 큐·0.2초 타이머만 쥐고 판단은 BoardRules 의 `terminalControl`·`sessionControl`).
  창 크기에 따라 `BoardTier`(full·medium·mini)로 그린다. `$TMPDIR/claude-board.lock` 을 `flock` 해 한 번만 뜨고, 두 번째 실행은 `claude-board.show` 분산 알림만 보내고 끝난다.
  `--action pause|resume|stop|reopen|focus <세션 id>`·`--action autopilot-open|autopilot-merge <자율주행 실행 id>`(끝 알림 판을 누른 것과 같음)·`--action quit -`(install.sh 가 청하는 정상 종료 — ⌘Q 와 같은 확인 흐름, 받으면 `$TMPDIR/<instanceName>.quit` 에 `asking`, 확인 창에서 취소하면 `cancel`) 는 떠 있는 보드에 `claude-board.action` 분산 알림으로 넘긴다(메뉴와 같은 함수 — reopen 은 `reopenEnded`: 끝난 세션만, 이어서 진행 요청은 일시중지했던 세션에만. pause 는 메뉴 단추와 같은 `isPausable`: 작업 중·승인 대기만).
  `EmbeddedTerminal.swift`(내장 터미널) — 창 내용은 `BoardRootView`([터미널 영역 │ `PaneDivider` │ 보드]). `TerminalHub` 가 탭(`BoardTab`)을 쥐고,
  탭(화면)마다 그 폴더의 로그인 셸을 띄우고, claude 명령은 셸의 줄 편집이 시작되면 그 셸에 붙여넣기 + Return 으로 넣는다(`BoardSurface.type`, claude 가 끝나면 셸 프롬프트로 남고 셸을 exit 하면 탭을 닫음.
  새 세션은 `--session-id`, 모든 길에 탭 이름을 `--name` 으로 붙이고 ⇧⌘R 은 `/rename` — `named`·`applyRenames`). `pair` 가 세션 파일의 `tty` = 탭 tty(headless 는 pid 조상이 탭)로 짝짓고
  `place` 가 보드 탭·다른 터미널·백그라운드·끝남을 가른다(규칙은 BoardRules 의 `pairTabs`·`sessionPlace`, 여기서는 화면 값을 떠 넘기고 결과를 화면·@Published 에 씀) — 세션 파일 약속은 그대로. 쥔 탭 목록은 UserDefaults `terminalTabs` 에 바뀔 때마다 남기고 다시 뜰 때 `restoreTabs` 가 resume_cmd 로 이어 연다.
  `BoardTerminalView`(한글 조합 중 Return·Esc 고침 — 엔진을 올리면 다시 시험),
  `confirmShutdown`(업데이트·종료 전 확인, 끊기는 탭의 주 세션에 `.paused`), `driveSession`(보드 탭은 탭에 직접, Ghostty 는 AppleScript — 길은 `TerminalControl.driveRoute`),
  `mdRenderDir`·`mdRenderCommand`(md-render 를 `~/.claude/notifier/plugins/` 로 옮기고 `--plugin-dir` 을 한 번만 붙임, 끈 세션은 `mdRenderOff`). 보드 쪽 문제는 `~/.claude/notifier/board.log`.
  설계·결정·실측은 `docs/내장 터미널/설계.md`(11장 단계별 메모) — 이 기능을 바꿀 때 먼저 본다.
  `SettingsScreen.swift`(설정 화면) — 앱 메뉴 > 설정…(⌘,)이 터미널 영역을 덮는 `SettingsView`(`SettingsHostingView` 에 얹음, 열고 닫기는 `SettingsScreen`). 항목은 `SettingsItem` 표 하나에서 그리고 검색도 이 표로 한다.
  `KeyboardShortcuts.swift`(단축키) — `Shortcuts`(사용자가 바꾼 키를 얹은 표를 @Published 로 쥐고 UserDefaults `shortcuts`·메뉴 키 달기·Carbon 전역 단축키·키 감시의 NSEvent 분기)·`ShortcutSettings`(키 받기),
  표와 판정은 BoardRules 의 `ShortcutTable`(메뉴·안내 글자도 여기서 읽음). 로그인할 때 열기는 `LoginItem`(LaunchAgent plist 를 쓰고 지우고, 끄면 `login-launch-off` 표지).
  `Autopilot.swift`(자율주행) — `AutopilotStore` 가 자율주행 뿌리(시험 빌드는 `Claude Board Dev`)의 `runs/` 와 `private/records.sqlite`(읽기 전용)를 읽어 실행 줄·자율주행 중·지난 실행 화면을 그리고, 일시정지·재개·중단은 `control.json` 으로 남긴다. 실행이 여럿이면 실행마다 줄이 따로이고(작은 크기의 줄에 실행 이름), 자율주행 중 화면의 `ap-switch-<실행 id>` 로 다른 실행 화면으로 바꾼다(6단계 ⑫-6). 출발 준비 시트(`APStart`, 원천은 지난 실행 이어 가기와 주제로 시작 — 주제로 시작은 5단계 ⑪-2 부터 출발까지: 링크 읽기·버전 목록·스택·새 폴더 → 앱 안 엔진 `seed base`(뼈대·출발 가지 보기·하네스 확인 지점) → `draft`(초안 세션·기계 검사) → `seed scenarios`(시나리오 커밋) → 이어 가기와 같은 확인·판정 미리 돌리기, 출발 확인의 명세·기술 스택 줄과 새 폴더 가지 꼴, 출발할 때 세트 spec.md·stack.json·autopilot.md 쓰기와 근거 묶음 자리 고치기, drafts 정리는 규칙 `apDraftSweepDecide` 와 엔진 `seed drop`)가 출발 전 실행 폴더를 쓰고 앱 안 엔진의 `status --json`·`prerun`·`start` 를 부른다. 경로 카드 오른쪽 클릭 "이 경로에서 자율주행…"이 그 경로로 연다. 출발 확인에서 모델·멈출 선·승격 차례(출발하면 `projects/<프로젝트>.json`)와 자율주행 지시(세트 `set/autopilot.md`, 고치기 전 글은 `set-backup/`)를 그 자리에서 바꾼다. 지난 실행의 "원문 지우기"는 실행 폴더만 지우고 `purged/<실행 id>.json` 에 남긴다. 합치기·승격 시트(`APMerge`)는 끝난 실행의 승격 차례 단계를 git 읽기로 미리 보이고(바뀐 줄·충돌) 사람이 누를 때만 엔진 `finish` 를 부른다. 원래 작업 트리로 도는 경로에 보드 탭 새 세션을 열면 `startSession` 이 경고한다. 5단계 ⑪-1: 출발 준비 2단계의 "시험 실행기 환경 확인" 줄(엔진 status --json 의 `env`)·판정 미리 돌리기 줄의 시나리오 결과, 이미 통과 시나리오 카드와 실행 파일이 없는 수용 기준 카드(`criterion-<단위>-<기준>`), 지난 실행 단위 보고서의 판정 증거(`APEvidence`, "증거 폴더 열기"). 5단계 ⑪-3: 준비 줄 "판정 놓침(이 프로젝트)"(앱 안 엔진 `misses`)·"세트 이어 쓰기"(이어 가기, 놓침이 있으면 `revise`)·"반대 탐침"(prerun.json)·"판정 비판 세션"(`critic`), 출발 카드 넷(반대 탐침·확인된 지적·확인 안 됨·해석 고르기)과 약해진 고침 카드, 출발 카드 상자의 "판정 고치기 n차 돌리기"(주제로 시작만, `draft --fix` → `seed scenarios` → 다시 확인), 출발 확인의 판정 차이 접기, 이어 가기 후보에 통과했지만 버린 단위(`extension APStart` 절). 판정 규칙은 `AutopilotRules`(아래)에서 import 한다(가지 기록·합치기 상태·남은 변경 보관·지난 실행 고르기). 설계는 `docs/자율주행/설계.md`.
- `Sources/BoardRules/` — 보드의 순수 규칙 라이브러리(Foundation 만): 세션·서브에이전트·카드 모델(`Session`·`Sub`·`Group`·`Ctx`·`Brain`), 세션 기록 해석(`Sub.parse`·`sessionID`), 카드에 보일 세션·묶음·정렬(`visibleSessions`·`groupSessions`·`sortGroups`), 화면 경로의 홈 줄이기(`displayPath`), 요금제 사용량(`Usage.swift`: `Limit`·응답 해석 `parseUsage`·토큰 상태 `loginToken`·캐시를 믿을지·받을지 `checkUsageCache`·받기 `fetchUsage`·받은 결과 처리 `settleUsage`, 앱도 쓰는 `isoDate`),
  계정 전환(`Accounts.swift`: `Account`(만료는 `isExpired(at:)`)·저장 계정 목록 해석과 고치기·전환 JSON 변환 `replacingAccountToken`·`switchConfig`·저장 `storeAccount`·`saveCurrentAccount`·전환 `switchAccount`·지우기 `removeAccount`),
  자동 전환 판단(`AutoSwitch.swift`: `decideAutoSwitch` — 한도 세션·전환 까닭·다음 계정과 한도 표시·이어 갈 세션을 값으로 돌려준다),
  다른 터미널 조작(`Terminals.swift`: 세션이 도는 곳 `SessionPlace`, 조상 앱 번들 id → `terminalKind`, AppleScript 글자 `ghosttySendScript`·`ghosttyFindScript`·`ghosttyFocusScript`·`ttyFocusScript`·`ttyCountScript`(바이트까지 시험이 고정),
  `TerminalControl` — 세션을 띄운 앱 `owner`(세션마다 기억)·`canDrive`·조작 길 `driveRoute`(보드 탭 먼저, 다음 Ghostty)·`focusRoute`·Ghostty 표지 제목 찾기 `ghosttyTerminalId`·`focusGhostty`·`focusByTty`),
  보드 탭 짝짓기·자리(`TabPairing.swift`: 화면 값 `TabScreen` → `pairTabs`(tty 짝·headless 조상·셸만 남은 화면·넘긴 작업 따라가기·경로 상태 점, 결과 `TabPairing`)·자리 `sessionPlace`·`parkedScreen`·`shownSessionPlace`),
  세션 조작 순서(`SessionControl.swift`: 단계 `SessionSteps`(살아 있는 세션 재개 `resume` 은 줄바꿈 + `resumePrompt` — 첫 출력 전에 멈춰 입력 칸에 되돌아온 요청 아래 줄에 붙는다)·꺼진 세션 이어 열기의 첫 요청 `carryOnPrompt`·일시중지할 응답이 있는가 `isPausable`·일시중지 표지 `pausedMarkData`·끝나기 기다리기 한 번의 판단 `exitWait`, `SessionControl` 의 `pause`·`resumePaused`·`stop`·`moveToBoard`·`reopen`·`reopenEnded`·`open`),
  업데이트 판단(`Updates.swift`: VERSION 본문 검증 `releaseVersion`·새 버전인지 `isNewerVersion`·확인하는 빌드 `updateEligible`·결과 `UpdateCheckResult`(손으로 확인한 안내 `notice`)·유스케이스 `checkForUpdate`),
  단축키 판정(`Shortcuts.swift`: 수정키 `KeyModifiers`(raw 값이 `NSEvent.ModifierFlags` 와 같고 보드 설정 `shortcuts` 에 그대로 남으므로 동작 id 처럼 바꾸지 않는다)·키 `KeyCombo`(`label`·`menuKey`)·
  동작 표와 사용자가 바꾼 키 `ShortcutTable`(`combo`·`set`·`conflicts`·`actionID`)·키 받기 판정 `recordShortcut`·시스템 단축키와 겹침 `systemShortcutTaken`).
  보드가 `import BoardRules` 로 쓰고, SwiftUI 와 겹치는 `Group` 은 보드 `BoardData.swift` 의 별칭이 이것을 가리킨다. 규칙을 바꾸면 `Tests/BoardRulesTests/` 의 입력·기대값 표와, 그 규칙이 맨 위 `rules-table/`(Windows 판과 함께 쓰는 표)에 있으면 그 줄도 함께 고친다(이슈 #9 — 한 판만의 차이는 줄에 `only` 와 까닭).
  포트·어댑터: 세션 상태 판정 `scanSessions`(`SessionScan.swift`)는 바깥을 포트(`Ports.swift`: `SessionRecords`·`ProcessProbe`·`ClaudeLive`·`Transcripts`·`ContextUsage`·`Clock`)로만 부른다 — 포트는 BoardRules, 진짜 어댑터는 보드 `BoardData.swift`(`SessionFiles`·`SystemProcesses`·`TranscriptFiles`·`ClaudeNames`·`ContextReader`·`SystemClock`, mtime 캐시도 어댑터에), 가짜 어댑터는 `Tests/` 에만 둔다.
  사용량 규칙도 같은 방식으로 `UsageCache`(`/tmp/.claude-usage-cache.json`, 상태줄과 함께 쓰는 약속)·`LoginCredentials`(키체인 로그인 정보)·`UsageAPI`·`Clock` 포트로만 부른다 — 진짜 어댑터는 `UsageCacheFile`·`KeychainCredentials`·`OAuthUsageAPI`(상태 없는 struct).
  계정 전환 규칙은 `KeychainItems`(키체인 일반 암호 항목, 자리는 `KeychainItem`)·`ClaudeConfig`(`~/.claude.json`)·`SavedAccountList`(`~/.claude/notifier/accounts.json`)·`CredentialsWriteLock`(`~/.claude/.storage-write`)·`Clock` 포트로만 부른다(묶음 `AccountPorts`) —
  진짜 어댑터는 `SystemKeychain`(`Keychain` enum 의 security 명령·16진수·ASCII JSON 그대로)·`ClaudeConfigFile`·`SavedAccountsFile`·`StorageWriteLock`, 로그인 정보 항목 자리는 앱의 `claudeLoginItem()`(서비스 해시·`$USER`).
  다른 터미널·세션 조작은 `TerminalApps`(떠 있는지·활성화·AppleScript 실행·잠깐 기다리기)·`ProcessControl`(조상 앱·부모 pid·살아 있는지 `kill(pid, 0) == 0`·SIGTERM — EPERM 뜻이 `ProcessProbe.isGone` 과 달라 따로)·`TerminalTitles`(tty 제목)·
  `BoardTabs`(보드 탭)·`SessionMarks`(`.paused`·`.mute` 쓰기·지우기)·`SessionTerminals`(단계 넣기·끝나기 기다리기·포커스 — 큐·타이머를 쥔 앱 함수)·`Clock` 포트로만 부른다 —
  진짜 어댑터는 `SystemTerminalApps`·`SystemProcessControl`·`TtyTitles`·`HubTabs`(`TerminalHub` 를 `MainActor.assumeIsolated` 로)·`SessionMarkFiles`·`AppSessionTerminals`.
  보드 탭 짝짓기·자리는 화면 값을 값으로 받고 `TabProbe`(화면의 claude·실행 방식 — 비싸서 그 갈래에서만, 진짜 어댑터 `HubProbe`)·`ProcessControl.parentPID`(headless 조상)·`ClaudeLive.parkedJob`(넘긴 작업 id) 포트로만 부른다.
  업데이트 판단은 `ReleaseVersions`(최신 VERSION, 진짜 어댑터 `GitHubReleaseVersions` — URLSession), 단축키 글자는 `KeyboardLayout`(지금 ASCII 자판, 진짜 어댑터 `SystemKeyboardLayout` — `asciiKeyCharacters`) 포트로만 부른다.
  사용자가 바꾼 키는 포트 없이 값으로 넘긴다(읽고 쓰기는 UserDefaults 한 줄씩이라). Carbon 수정키 `carbonMods`·NSEvent 변환·시스템 단축키 목록 읽기(`systemHotKeys`)는 앱에 둔다.
  규칙에 새 OS 호출(파일·프로세스·시각·홈 폴더)을 넣을 때는 포트를 거친다 — BoardRules 에서 FileManager·kill·`Date()` 를 직접 부르지 않는다(남은 예외: `Group.path` 의 `NSHomeDirectory()`).
  새 기능(자율주행)도 같은 꼴로 쓴다: 판정은 Foundation 만 쓰는 규칙 라이브러리(자율주행은 `Sources/AutopilotRules`, 아래)에, 바깥(git·프로세스·HTTP·SQLite 등)은 포트로, 가짜 어댑터 표 시험을 붙인다. 이미 있는 코드는 한꺼번에 옮기지 않고 손대는 부분부터 옮기며, 옮길 때는 옮기기 전 출력과 견주는 덤프·시험으로 동작이 그대로인지 확인한다.
- `Sources/AutopilotRules/` — 자율주행 판정 규칙 라이브러리(Foundation 만). 보드 `Autopilot.swift` 와 엔진 `Sources/ClaudeAutopilot/main.swift` 가 함께 `import AutopilotRules` 한다(보드의 다른 파일은 import 하지 않는다).
  `Branches.swift`: 끝난 실행의 가지 기록(run.json·기록 저장소 `runs.branches` 의 `branches`) 읽기 `APStep`·`APPromoted`·`APFailed`·`apStoryRepos`(엔진 `storyRepos` 도 대신함, 새 꼴 `repos`·옛 꼴 `repo`)·`apPromotedRows`·`apFailedRows`·`apUnresolved`·`apStepDone`,
  합칠 쪽 `apMergeSources`(가지를 만든 스토리만)·합치기 상태 `apMergeOpen`(보드 본체 끝난 실행 줄의 "합치기 기다림")·`apMergeMark`(지난 실행의 버림·…에 합침·…까지 올림·합치기 기다림, 합칠 쪽마다 다르면 섞인 수, 색은 보드가 고름),
  엔진 `finish discard` 의 기록 줄 `apDiscardRows`(지울 것이 없어도 합칠 쪽은 버린 줄)·`finish` 의 뿌리 `apFinishRoot`(`--dev` 는 run.json 과 맞는지만), 합치기·승격 시트의 "출발 가지에만 있던 커밋" 경고 `apCarriesBase`.
  `Leftovers.swift`: 가지를 바꾸기 전 남은 변경 `apLeftover`(그대로·커밋·보관·바꾸지 않음)·읽기 전용 자리에서 보관할 새 경로 `apStashPaths`·보관 메시지 `apStashMessage`·누구 몫 `apStashUnit`.
  `History.swift`: 지난 실행 상세에 보일 실행 `apHistoryPick`(거른 프로젝트 안에서). 이름의 AP 는 보드 자율주행 절의 이름 짓기를 그대로 따른다.
  2단계 5-2b(출발 화면 나머지): `Promotion.swift`(가지 자리 `apBranchRoles`·저장소 가지로 채운 승격 차례 `apDefaultPromotion`(보드에서 옮김)·바꾼 차례의 고칠 것 `apPromotionProblems`·고를 것 `apPromotionChoices`),
  `StartSettings.swift`(설정 줄 모델·멈출 선 고르기와 확인, 출발 준비가 쓸 값의 차례 `apStartSettings`: 프로젝트 설정 → 지난 실행), `Instruction.swift`(자율주행 지시 줄 차이 `apLineDiff`·머리 줄·저장 확인),
  `Stack.swift`(기존 저장소 빌드·설정 파일 감지 `apDetectStack`·주제 글 낱말 대조 `apTopicHits`·`apTopicStack`·다른 스택 경고 `apStackConflicts`), `Links.swift`(자료 링크 종류·읽기 상태 `apLinkInfo`, 읽지는 않음),
  `NewRepo.swift`(새 폴더 확인 `apNewRepoCheck`·git 차례·유스케이스 `apCreateRepo`), `Versions.swift`(공식 버전 목록: 고정 주소 `APFeed`·응답 풀기·목록과 추천 `apBuildCatalog`·유스케이스 `apLoadCatalog`, 하루 캐시 → 받기 → 지난 캐시 → 앱에 든 목록 `BuiltinVersions.swift`),
  `Purge.swift`(지난 실행 원문 지우기 `apPurgeCheck`·유스케이스 `apPurgeRaw`, 실행 폴더만), `Carry.swift`(이어 간 줄 밖의 열린 미룬 몫 `apOpenParked`).
  3단계(보고서): `Report.swift`(결과 기록 읽기 `apParseResult`·기록 → 값 `apReportRun`·한 파일로 완결된 HTML `apReportHTML`(CSP 로 바깥 요청 막음, 그림은 인라인 SVG, 밝은·어두운 화면, 시각·시간대·홈 폴더는 값)·유스케이스 `apDrawReports`),
  `Sequence.swift`(결과 기록 흐름 그림의 sequenceDiagram 작은 부분 읽기 `apParseSequence`·SVG `apSequenceSVG`, 읽지 못한 줄이 있으면 원문), `Prose.swift`(긴 줄표 `apPlain`(보드에서 옮김)·HTML 이스케이프 `apEscapeHTML`·링크 `apSafeLink`(https 만)).
  바깥은 포트(`Ports.swift`: `APClock`·`APFolders`·`APGitRunner`·`APStackFiles`·`APVersionFeeds`·`APVersionCache`·`APRawFiles`·`APReportSource`·`APReportSink`, BoardRules 의 `Clock` 과 이름이 겹치지 않게 AP)로만 부른다 — 진짜 어댑터는 보드 `AutopilotTopic.swift`·`AutopilotReport.swift`(보고서 읽을 곳)와 엔진(`EngineReportSource`·`EngineReportSink`), 가짜는 `Tests/AutopilotRulesTests` 에만.
  입력 승격 차례는 값(`[APStep]`)으로 받고 프로젝트 설정 파일 읽기(`apPromotion`)·기록 저장소 읽기(`APRecords`)는 보드에 남았다.
  시험(`Tests/AutopilotRulesTests/` 의 `BranchesTests`·`LeftoversTests`·`HistoryTests`·`PromotionTests`, 5-2b 새 규칙은 `TopicTests`·`VersionsTests`·`StartTests`(가짜 어댑터))은 옮기기 전 함수로 돌린 덤프의 입력·기대값 표다(시험 빌드 뿌리 장난감 기록 26개를 줄이고 경로를 `/w/…` 로 바꿈, 경계 입력). 규칙을 고치면 그 줄의 기대값과 note 를 함께 고친다(5-3b 에서 고친 줄은 note 에 "5-3b").
  보고서 시험 `ReportTests` 는 견본 기록(`Tests/AutopilotRulesTests/Report/sample-run.json`·악성 견본 `evil-run.json`, 장난감 값)과 골든 HTML 셋(같은 폴더, `Package.swift` 에서 exclude)을 #filePath 로 읽는다.
  틀을 고쳐 골든이 달라지면 화면으로 확인한 뒤 `AP_REPORT_GOLDEN=write swift test …` 로 다시 쓰고 바뀐 줄을 함께 커밋한다.
  4단계 ⑩-1(연결과 올릴 글): `Connections.swift`(연결 목록 `connections.json` 꼴 `apParseConnections`·사이트 주소 규칙 `apSiteCheck`(설치판 https 만, 시험 빌드만 http://127.0.0.1)·`apWebhookCheck`·바탕 주소 `apServiceBase`·만료일 `apExpiry`·키체인 항목 이름 `apSecretService(dev:)`·인증 머리 `apAuthHeader`(보내기 직전에만)·연결 확인 `apCheckConnection`),
  `Formats.swift`(작은 문서 꼴 `APDoc` → Jira 문서 형식 `apADF`·Confluence 저장 형식 `apStorage`(매크로 없이 이스케이프)·Notion 블록 `apNotionBlocks`(글 한 칸 2,000자 `apNotionRich`, 한 번에 100블록 `apBatches`)),
  `Outbox.swift`(게시 설정 `apPublishParse`·단위 댓글 `apUnitOutbox`·보고서 글 `apRunOutbox`·글 파일 꼴 `apOutboxParse`·올릴지 `apPostDecide`·429 다시 보내기 `apRetryWait`·요청 꼴·올리기 유스케이스 `apPostItem`(보내기 전에 posting, 단계마다 받은 곳 id, 죽은 뒤는 uncertain)·`apMarkItem`).
  포트 `APHTTP`·`APSecrets`·`APWaiter`·`APOutboxStore` 의 진짜 어댑터는 엔진 한 곳(`EngineHTTP`(리다이렉트 따르지 않음)·`EngineSecrets`(security 로 읽기만)·`EngineOutbox`(글마다 flock))이고, 보드는 연결 목록 쓰기·키체인 쓰기만 하고 연결 확인·올리기는 앱 안 엔진 명령을 부른다. 시험 `PublishTests`(가짜 어댑터 표·골든 `Tests/AutopilotRulesTests/Publish/`, `Package.swift` 에서 exclude).
  4단계 ⑩-2(읽기): `Meta.swift`(메타 형식 `APMeta`·형식 검사 `apMetaCheck`(형식 밖 칸·HTML 태그·상한, `wrapped:` 는 근거 묶음 파일의 믿지 않는 자료 표시까지)·내장 읽기 자르기 `apMetaClamp`·마크업 빼기·가리킨 절 `apChunked`·비밀 가리기 `apRedact`·표시 감싸기 `apMetaWrapUntrusted`),
  `Reading.swift`(링크 → 읽을 대상 `apReadTarget`·읽기 막는 까닭 `apReadBlocker`·자료 링크 줄 `apLinkState`·GET 요청 꼴·Jira·Confluence·Notion·Figma 응답 → 메타·Figma 그림 호스트 허용 `apImageURLOK`·유스케이스 `apReadLink`·에픽 `apReadEpic`),
  `Evidence.swift`(근거 묶음 파일 이름·`MANIFEST.json`·넣는 문 `apAddEvidence`(가리기·감싸기·검사가 모든 길에서 여기)), `MCPRead.swift`(사용자 MCP 설정의 서버 `apMCPServers`(저장소 `.mcp.json` 은 승인된 것만)·허용(읽기 동사로 시작)·거부(쓰기 꼴) 규칙·claude 인자 `apMCPReadArgs`(`--restricted`·`--strict-mcp-config`·`--tools ""`)·결과 `apMCPMeta`).
  포트는 ⑩-1 의 `APHTTP`·`APSecrets`·`APWaiter` 와 `APEvidenceStore`(진짜 어댑터 엔진 `EngineEvidence`). 시험 `ReadingTests`(가짜 어댑터 표·골든 `Tests/AutopilotRulesTests/Reading/`, `Package.swift` 에서 exclude, 토큰 꼴 견본은 시험 안에서 이어 붙여 만듦).
  4단계 ⑩-3(티켓 나무·알림): `Tickets.swift`(만들 티켓 계획 `tickets-plan.json` 꼴 `apTicketPlanParse`·상태 `tickets.json` `apTicketBookParse`·이슈 종류 고르기 `apPickIssueType`·만들기·부모 바꾸기 요청 꼴·만들기 유스케이스 `apMakeTickets`(남은 것만, 보내기 전에 creating, 죽은 뒤는 uncertain)·자리표 `apTicketPlaceholder`·출발하면 안 되는 키 `apUnmadeKeys`·실행 중에 새로 필요해진 티켓 글 `apNewTicketOutbox`(늘 검수 뒤)),
  `Notify.swift`(webhook 글 꼴 `APNotifyFormat`·다듬기 `apNotifyClean`(가리기·멘션 무력화)·이스케이프·본문 `apNotifyBody`·출발·대기·끝·3시간 요약 글·정한 때 `apStateNotice`·`apSummaryDue`·보내기 유스케이스 `apSendNotice`·끝 알림이 여는 곳 `apEndOpen`·띄울 때 `apEndNoticeDue`·`apMergeReminderDue`·요약의 합치기 기다림 `apRunMergeOpen`).
  포트는 ⑩-1 것과 `APTicketStore`(진짜 어댑터 엔진 `EngineTickets`). 시험 `TicketsTests`·`NotifyTests`(가짜 어댑터 표·골든 `Publish/ticket-*.json`·`notify-*.json`).
  5단계 ⑪-1(시험 실행기): `Scenarios.swift`(대기열 칸 `scenarios`·`criteria` 읽기 `apParseScenarios`·`apParseCriteria`·실행 파일이 없는 수용 기준 `apCriteriaGaps`·그 카드 글 `apCriterionCard`(범위에서 빼기 추천)·출발 때 답 `apCriterionAnswer`,
  5단계 ⑪-2(주제로 시작의 초안 세션·뼈대): `Draft.swift`(초안 세션 글 `apDraftPrompt`(첫 줄 고정·근거 묶음은 믿지 않는 자료)·지시 쓰는 법 `apInstructionGuide`·claude 인자 `apDraftArgs`(`--tools` 읽기·결과 쓰기 다섯, Bash·웹·MCP 없음, `--strict-mcp-config`)·세션 설정 `apDraftSettingsJSON`(hook 끔·쓰기는 결과 자리만·읽기 금지), 결과 기계 검사 `apDraftCheck`(허용 목록 칸 `apDraftUnitKeys`, 샌드박스 밖·범위를 넓히는 칸 `apDraftWideningKeys` 는 거절, 칸·상한·HTML·비밀 꼴·다른 가지/push/바깥 쓰기 줄 `apForbiddenLines`(부정은 맞은 꼴 바로 뒤 같은 마디만)·수용 기준마다 시나리오나 카드·명세 읽기 `apParseSpec`·지시 첫 줄), 하네스 확인 지점 `apHarnessPoints`·`apHarnessMissing`·`apHarnessCard`, 초안 단위 → 대기열 `apDraftQueueUnit`(작업 폴더는 저장소 뿌리, 시나리오는 `.autopilot/scenarios/<단위 id>/`), drafts 정리 `apDraftSweepDecide`),
  `Skeleton.swift`(스택 → 생성기 계획 `apSkeletonPlan`(FastAPI·compose 는 보드 틀, Spring 은 start.spring.io, Nest·Vite·Next·Vue 는 명령 글, 직접 입력은 첫 단위 "프로젝트 구성", 판 글자 `apVersionOK`·질의 값 `apQueryValue`)·도구 확인 `apSkeletonToolProblems`·유스케이스 `apMakeSkeleton`(바깥 요청 걸음 막기)·출발 전 자율주행 가지 `APSeed`·`apSeedProblem`). 포트 `APDraftFiles`·`APSkeletonWork`(진짜 어댑터 엔진 `EngineDraftFiles`·`EngineSkeletonWork`, 링크를 따라가지 않음). 시험 `DraftTests`(가짜 어댑터 표, 보안 검토 줄 포함).
  돌리는 법 `apScenarioRun`(첫 줄 #! 또는 확장자, 브라우저는 드라이버 도구)·환경 확인 `apEnvCheck`·고칠 것 글 `apEnvProblem`, 판정 차례 `apJudgeSteps`(시나리오 고정본들 → 판정 명령)·유스케이스 `apRunJudge`(고정본 지문 견주기, 멈춤이면 거기서)·증거 요약 `apJudgeSummary`·이미 통과 시나리오 카드 `apPrerunScenarioCard`),
  `JudgeSandbox.swift`(판정 샌드박스 Seatbelt 프로필 `apJudgeProfile`: 쓰기 모두 막기 → 연 곳 → 다시 막기 → 이번 증거 폴더, 읽기 금지, `(deny lsopen)`·`(deny appleevent-send)`·mach-lookup 기본 막고 목록만(브라우저 걸음만 조금 더), 네트워크 `apJudgeNet`(기본 없음, 실행 network_allow 의 localhost 포트·단위 check_ports·local_binding)),
  5단계 ⑪-3(판정 비판 일반판): `Critique.swift`(판정이 보는 자리 `apJudgePaths`·`apJudgeRunScripts`·`apProbeTargets`·`apJudgeLiterals`, 보호 자리 `apProtectedJudgePath`(경로 키 `apPathKey`: 조각마다 NFC 뒤 소문자, APFS 가 대소문자·정규화를 가리지 않아서), 판정 설정 파일 꼴 `apJudgeConfigPath`,
  반대 탐침 `apProbePlan`·`apRunProbes`·`apProbeCard`, 비판 세션 기준표 `apCriticRubric`·모델 `apCriticModel`·`apCriticModelPick`·글 `apCriticPrompt`·결과 읽기 `apCriticParse`(반례는 그 판정이 보는 자리만, `apCasePathProblem`)·확인 `apVerifyFindings`·카드 `apCriticCard`, 판정 고치기 `apFixNext`(최대 2번)·`apDraftFixPrompt`·약해짐 `apJudgeWeakening`·`apWeakCard`),
  `Misses.swift`(판정 놓침 신호 `APMissSignal`·`apUnitMisses`·`apPrepMisses`·`apDiscardedUnit`·요약 `apMissDigest`, 세트 이어 쓰기 조건 `apSetRewrite`·세션 글 `apRevisePrompt`·결과 검사 `apReviseCheck`, 판정 차이 `apJudgeChanges`·`apScenarioTextChanges`),
  `Records.swift`(기록 저장소 표 꼴 판 1 `apRecordsSchemaV1`(엔진에서 옮김)·판 2 `judge_misses`·판 올리기 `apRecordsSteps`). 포트 `APProbeWorkspace`(진짜 어댑터 엔진 `ProbeTrees`). 시험 `CritiqueTests`(가짜 작업 트리·결과 폴더 표, 옛 판 1 저장소를 SQLite 로 열어 판 2 로 올리는 시험, 대소문자·NFD·설정 파일 반례 줄).
  `Checks.swift` 에 출력 끝 `apOutputTail`·미리 돌리기 카드 `apPrerunCard`(엔진에서 옮김). 포트 `APToolProbe`·`APJudgeRunner`(진짜 어댑터 엔진 `EngineToolProbe`·`Engine.JudgeRunner`, `sandbox-exec -f` 로 감쌈). 시험 `ScenariosTests`(가짜 어댑터 표·프로필 골든)·`JudgeMoveTests`(옮기기 전 식 대조).
  6단계 ⑫-6(여러 실행 함께 돌리기): `Stopline.swift`(엔진 stoplineGate 에서 옮긴 멈출 선 판정 `apStoplineWait`·퍼센트 꼴 사용량 `apUsageFromPercent`·`apISODate`, 옮기기 덤프 같음),
  `Multirun.swift`(사용률 읽을 곳 `apUsageSteps`(설치판은 stream → 캐시 → API 그대로, 시험 빌드는 장난감 파일 `usage_path` → stream 이거나 stream → 캐시, API 없음), 멈출 선 자리 `APSlot`·`apSlotDecide`(지금 주간 사용률 + 다른 실행 자리의 쓰지 않은 %p 를 그 실행의 선과 견줌, 먼저 기다린 실행에 차례, 15분 지난 자리 지움)·`apTakeSlot`·`apBeatSlot`·`apReleaseSlot`·`apSlotSize`,
  같은 경로 자리 `APClaim`·`apClaimPaths`(한 transaction: 겹치고 살아 있으면 거부, 남은 자리는 치우고 잡음)·`apClaimAlive`·`apPathOverlap`(접은 경로, 같거나 안)·`apClaimKeys`·`apClaimBusyText`, 잠긴 저장소 다시 하기 `apBusyRetryDelay`),
  `Records.swift` 판 3(`path_claims`·`stopline_slots`). 포트 `APSlotBook`·`APClaimBook`(진짜 어댑터 엔진 `EngineSlotBook`·`EngineClaimBook`, 기록 저장소 BEGIN IMMEDIATE 한 transaction, 출발 전 확인은 읽기만 하는 `ReadOnlyClaimBook`)·`APClaimProbe`(엔진 `EngineClaimProbe`: run.json·엔진 pid 명령줄·LaunchAgent·출발 명령 pid).
  시험 `StoplineTests`(옮기기 덤프에서 고른 줄)·`MultirunTests`(설치판이 덮어쓰기를 듣지 않는 표, 자리 판정 표, 가짜 자리 표로 두 엔진이 같은 때 잡기·번갈아·같은 때 두 출발, 판 2 → 3 저장소).
  자율주행 규칙은 맨 위 `rules-table/`(Windows 공용 표)에 넣지 않는다.
- `Sources/ClaudeAutopilot/main.swift` — 자율주행 엔진 `claude-autopilot`(`start`·`run`·`control`·`status`·`records import`·`prerun`·`finish`(`--dev` 는 그 실행의 뿌리와 맞는지만 본다)·`report`(끝난 실행의 보고서 다시 그리기)·`outbox`(올릴 글 list·post·skip·later, 보드 검수 화면이 부름)·`connection check`(설정 › 연결의 연결 확인)·`read`(바깥 읽기: 내장 REST·`--mcp <서버>` 읽기 세션, `read epic <키>`·`read servers`, 시험 빌드만 `--mcp-from`·`--claude`)·`tickets <출발 준비 폴더> make|list`(출발 확인의 티켓 나무 만들기, 출발한 실행은 거부)·`seed <출발 준비 폴더> base|scenarios|drop`·`draft <출발 준비 폴더>`(주제로 시작, 5단계 ⑪-2, ⑪-3 부터 `--fix <차례> --cards <id,…>`·`--accept-fix <차례>`)·`critic <출발 준비 폴더> [--round <차례>]`·`misses <폴더>`·`revise <출발 준비 폴더>`(5단계 ⑪-3, 폴더가 첫 인자)·`notify test <webhook id>`·`notify summary`(시험 빌드만 `--at`), 사용법은 `usage()`).
  5단계 ⑪-2: `seed base` 가 출발 가지 머리에서 `autopilot/<실행 id>` 와 임시 작업 트리(`~/.claude-board/worktrees/.seed-<id>/`)를 만들고 새 폴더면 뼈대를 커밋(도구가 없으면 가지를 만들지 않고 실패와 까닭, 시험 빌드는 바깥 요청 생성기를 `external_generators` 없이는 막음), `draft` 가 초안 세션(고정 세션 설정 0600, 글은 표준 입력, 결과는 `drafts/<준비>/draft/`)과 기계 검사(`draft-check.json`), `seed scenarios` 가 시나리오를 파일 시스템을 거치지 않고 git 색인에만 넣어 커밋(링크 너머 거절). `run.json` 의 `seed` 가 있으면 출발 전 확인·`start`·`prerun` 이 그 가지를 그대로 이어받는다(기록과 같은 머리·출발 가지가 그대로일 때만, 시나리오 글은 그 머리의 git 객체에서, prerun 은 그 머리의 임시 작업 트리에서).
  6단계 ⑫-6: `start` 가 저장소 경로(저장소 밖 작업 폴더는 그 폴더, `claimPathsOf`)를 실제 경로로 풀어 기록 저장소 `path_claims` 에 잡고(같은 경로에서 살아 있는 실행이 있으면 "이 경로에서 자율주행이 돌고 있습니다(…)" 로 출발하지 않음, 잡은 뒤 실패하면 `failCleanup` 이 풂) 끝 정리를 마치면 푼다.
  출발 전 `status`(보드 출발 준비 2단계)도 같은 규칙으로 읽기만 해 고칠 것으로 낸다. 새 단위 앞 `stoplineGate(n)` 이 `stopline_slots` 에 자리를 잡고(다른 실행 때문이면 1분마다 다시 봄, 상태 그대로), `pollControl` 이 1분마다 다시 적고(`slotTick`), 단위 끝·일시정지·시작하지 못한 단위·실행 끝에 푼다.
  사용률은 `apUsageSteps`(시험 빌드만 실행 `usage_path` 장난감 사용량 파일, `docs/자율주행/시험 도구/장난감 사용량.py`). `RecordsDB` 는 열기의 journal_mode·판 읽기와 쓰기 transaction 을 잠기면 `apBusyRetryDelay` 대로 다시 한다. 출발 뒤 `status` 에 "멈출 선 자리"·"같은 경로 자리" 줄.
  5단계 ⑪-3: `prerun` 이 일하기 전 판정이 실패한 단위마다 반대 탐침(`--no-probes` 로 끔, `--keep-history` 는 앞 탐침 카드를 남김)을 버리는 작업 트리 `ProbeTrees`(규칙 포트 `APProbeWorkspace` 의 진짜 어댑터, `~/.claude-board/worktrees/.probe-<id>-<저장소>`, 걸음마다 reset --hard·clean -fdq, 쓰기는 safeDirIn·O_NOFOLLOW·연 뒤 F_GETPATH 실제 자리가 보호 자리면 거절)에서 돌린다. `critic` 은 작성과 다른 모델의 비판 세션(세 세션 공통 `readSession`, 인자는 엔진 전용 `private/critic/<id>-r<차례>/args.json`)과 반례 확인(그 판정이 보는 자리만), `draft --fix` 는 고칠 것(`fixItems`)과 함께 지금 초안을 고치게 하고 약해지지 않았을 때만 바꿔 끼움, `revise` 는 이어 가기의 세트 이어 쓰기. 기록 저장소는 판 2(`judge_misses`, 열 때 `apRecordsSteps` 로 올림): `syncRun` 이 단위 신호·준비 때 확인된 지적을 쓰고 `misses_used` 를 used 로, `misses` 가 열린 놓침을 `misses.json` 에.
  5단계 ⑪-1: 판정(시나리오·판정 명령)은 `sandbox-exec` 판정 샌드박스 안에서 돈다(단위·실행 `check_sandbox: false` 로 사람이 끈 것만 밖). `start` 가 시나리오를 `pinned/checks/scenarios/<단위>/<차례>/` 로 고정하고(sha256 은 고정 설정), 판정은 늘 고정본으로, 증거는 `units/<n>/evidence/check-<k>/`(시나리오마다 `scenario-<i>/`·`summary.json`·`sandbox.sb`, 파일 0444). `prerun` 도 같은 판정. 시험 빌드만 실행 `browser_path`(가짜 브라우저 `docs/자율주행/시험 도구/가짜 브라우저.py`).
  시험 도구 `docs/자율주행/시험 도구/`: 가짜 claude(`가짜 claude.py`, 단위 세션·MCP 읽기·초안·판정 고치기·비판·세트 이어 쓰기 세션 흉내, 모델 비용 0)·가짜 바깥 서버·가짜 브라우저·장난감 사용량 파일 쓰기(`장난감 사용량.py`, 6단계 ⑫-6, 공용 캐시에는 쓰지 않음)와 견본 넷(`견본 초안`·`견본 고친 초안`·`견본 비판`·`견본 이어 쓰기`, 장난감 값).
  4단계 ⑩-3: `start` 는 자리표·임시 번호가 남은 대기열로 출발하지 않는다. 상태 알림(출발·대기·끝)을 `setState`·끝 정리에서, 3시간 요약을 1분마다 뿌리의 `notify-summary.json`(잠금)으로 하나만 보낸다(보드 창이 꺼져 있어도). 단위 판정 뒤 결과 기록 `new_tickets` 를 검수 뒤 만들 티켓 글로 남긴다.
  4단계 ⑩-2: 단위 시작 전에 단위 `tickets` 의 첫 키를 Atlassian 연결(읽기 범위)로 읽어 `units/<n>/context/` 에 넣는다(`readTicket`, 못 읽어도 단위는 감).
  4단계 ⑩-1: 단위 판정 뒤 Jira 댓글 글, 끝 정리 때 보고서 글을 `outbox/` 에 만들고, 게시 방식이 자동으로 올림이면 바로 올린다(보드 창이 꺼져 있어도, 연결 없음·만료면 요청 0 으로 쌓음). 모델 세션 샌드박스는 `outbox/` 와 `~/Library/Keychains` 를 읽기·쓰기 금지에 둔다.
  도는 엔진은 단위 판정 뒤·건너뛴 뒤·끝 정리 때 실행 폴더에 보고서(`report.html`·`units/<n>/report.html`)를 AutopilotRules 틀로 직접 그린다(보드 창이 꺼져 있어도, 모델 0). SwiftPM 실행 타깃 `ClaudeAutopilot`(언어 모드 5, `AutopilotRules` 에 기댐)이고 한 파일이라 최상위 실행문이 있다.
  보드 앱 밖에서 LaunchAgent(KeepAlive)로 돌고, 출발 때 실행 폴더 `pinned/` 에 고정한 자기 사본과 설정(`frozen.json`, 해시는 LaunchAgent 인자)으로 돈다. 모델 세션은 Claude Code 샌드박스(`--settings`)·고른 환경 변수·`--setting-sources project,local` 로 띄운다.
  build-board.sh 는 보드와 같은 `swift build` 결과의 `ClaudeAutopilot` 을, make-dmg.sh 는 두 아키텍처 결과를 `lipo` 로 합쳐 앱 `Contents/MacOS/claude-autopilot` 에 싣는다(앱보다 먼저 서명). 설계·실측은 `docs/자율주행/설계.md`·`진행 상황.md`.
- `ClaudeNotifier.swift` — 알림 패널. 인자는 파일 머리 주석 참고. 여러 패널이 동시에 뜨면 슬롯 파일로 자리를 나눈다. `--session` 을 받으면 누를 때 보드에 `focus` 동작을 알린다.
- `board-tui.py` — 터미널판 보드(`claude-board-tui`).
- `AppIcon.swift` — 앱 아이콘 PNG(iconset 이름 규칙)를 인자로 준 폴더에 그린다. 결과 `AppIcon.icns` 는 저장소에 커밋되어 있다.
- `ClaudeStatus.swift` — 예전 메뉴 막대 앱. 어떤 빌드 스크립트도 빌드하지 않는다.

### 패키징과 배포

- Info.plist 는 `build-board.sh` 와 `packaging/make-dmg.sh` 안에 각각 heredoc 으로 있다. 키를 바꾸면 두 곳을 함께 고친다(버전은 make-dmg.sh 만 넣는다).
- 엔진 리소스 번들 `GhosttyKit_GhosttyTerminal.bundle`(terminfo·셸 통합)은 `.app` 맨 위에 두면 서명이 거부되어 `Contents/Resources/` 에 둔다. SwiftPM 의 `Bundle.module` 은 그 자리를 못 찾으므로 앱이 시작할 때 `resolveResourceBundleInApp()` 이 알려 준다.
  빌드 경로가 남은 개발 Mac 에서는 이게 깨져도 멀쩡해 보이고 사용자 Mac 에서만 첫 터미널을 열 때 죽는다 — 확인할 때는 `.build/<triple>/release/` 의 번들을 치우고 연다. 엔진 버전은 `exact:` 로 고정한다(2.2.2026101502 아래는 LGPL libintl 이 정적으로 들어 있다).
- `hooks.py` 와 `packaging/hooks.py` 는 같은 내용의 사본이다. build-board.sh 는 앞의 것, make-dmg.sh 는 뒤의 것을 앱에 넣으므로 둘을 함께 고친다.
- `plugins/md-render/` 는 들여온 md-render 다(들여온 파일은 원본 그대로 — 출처·커밋·바꾼 점은 그 README). 앱 `Contents/Resources/plugins/md-render/` 에는 `LICENSE`·`README.md`·`.claude-plugin/plugin.json`·`hooks/hooks.json`·`hooks/register.tsx`
  다섯 파일만 build-board.sh·make-dmg.sh 가 이름으로 골라 싣는다(두 곳을 함께 고친다). `tests/` 는 `claude plugin test` 용이라 싣지 않는다.
- `packaging/THIRD_PARTY_NOTICES.txt` — 앱에 정적으로 들어간 구성 요소(엔진과 그 안의 MIT 아닌 것까지)의 고지. 두 스크립트가 `Contents/Resources/` 에 복사한다. 엔진 버전을 올리면 다시 만든다.
- 설치 경로는 두 가지다: `packaging/install.sh`(curl 설치·업데이트, 보드의 업데이트 단추도 이것을 실행)와 dmg 안의 pkg(`pkg/scripts/postinstall` → `user-setup.sh`). 둘 다 사용자 설정은 앱 안의 `user-setup.sh` 가 맡는다.
  `install.sh` 차례: 받기·서명 확인 → 보드와의 관계 `board_relation`(조상의 설치된 보드 바로 아래가 탭의 `login` 이면 `tab`, 아니면 `updater`(보드 → `bash -c`), 조상에 없으면 `outside`) → `outside` 이고 보드가 떠 있으면 바꿔 끼우기 전에 지금 깔린 앱으로 `--action quit -` 을 보내 정상 종료를 청한다(`ask_board_quit`: 5초 안에 `asking` 이 없으면 요청을 모르는 옛 보드라 3단계에서 `pkill`, 있으면 꺼질 때까지 10분까지 기다리고 `cancel` 이면 아무것도 바꾸지 않고 멈춤) → 바꿔 끼우기 → `updater` 는 `pkill -a`(보드가 이미 확인 창을 거침), `outside` 는 남은 것 `pkill -x` → `user-setup.sh` 가 LaunchAgent 를 `bootout`·`bootstrap` 해 새 앱을 띄운다. `tab` 이면 보드를 끄지 않고 `CLAUDE_BOARD_KEEP_RUNNING` 으로 `user-setup.sh` 의 `launchctl`·`open` 도 건너뛴다(LaunchAgent 로 뜬 보드는 `bootout` 이 확인 없이 끄고, 그 안에서 돌던 설치도 함께 끊긴다).
- 저장소 `suhojang/claude-board` 와 릴리스 파일 이름 `ClaudeBoard.zip`·`install.sh`·`VERSION` 은 `release.sh`·`install.sh`·`Sources/ClaudeBoard/AppUpdate.swift`(`releaseRepo`) 가 함께 쓴다 — 이름을 바꾸면 이미 설치된 앱의 업데이트가 깨진다.
- `install.sh` 는 서명 인증서의 SHA1(`CERT_SHA1`)을 고정해 확인한다. 인증서를 바꾸면 기존 사용자의 업데이트가 거부되고 Ghostty 제어 권한을 다시 묻는다.

## 저장소 관례

- `.gitignore` 는 `/*` 로 모두 빼고 `!/<이름>` 으로 올릴 파일만 고르는 방식이다. 최상위에 새 파일을 만들면 여기에 추가해야 추적된다. `dist/` 의 빌드 결과는 올리지 않는다.
- 사용자에게 보이는 기능이 바뀌면 `CHANGELOG.md`(바뀜·추가·고침), `README.md`, `packaging/README.txt`(설치 가이드 — 모든 기능의 자세한 설명)를 함께 고친다.
- 릴리스 커밋 메시지는 `<버전> 릴리스 — <요약>` 형식이다.
- 보드가 한 파일(`ClaudeBoard.swift`)이던 때 갈라진 가지(자율주행 `autopilot-stage2` 등 — `autopilot-stage2` 는 2026-10-29 에 이 절차로 합쳤다)에 main 을 합칠 때는 나누기 커밋(`git log --diff-filter=D --format=%h -1 main -- ClaudeBoard.swift`)을 따로 합친다:
  ① 나누기 직전 커밋(`<나누기 커밋>^`)까지 보통으로 합친다. ② 나누기 커밋을 합친다 — `ClaudeBoard.swift` 수정/삭제 충돌이 나면 작업 트리에 남은 그 가지 쪽(①까지 합친) `ClaudeBoard.swift` 로
  `python3 -I tools/split-board.py` 를 돌려 `Sources/ClaudeBoard/` 를 다시 만들고 `git rm ClaudeBoard.swift`·`git add Sources/ClaudeBoard` 뒤 합치기를 마친다(`Package.swift` 등은 보통으로).
  스크립트가 표에 없는 전역·`main.swift` 밖 최상위 실행문을 알리거나 빌드가 `private` 로 막히면 그 가지에서 스크립트 표(`EAGER`·`LAZY`·`ACCESS`)를 넓혀 다시 돌린다. ③ 그 뒤 main 을 마저 합친다.
- 사용자에게 보이는 글자(앱 화면 문자열·README·CHANGELOG·설치 가이드)에는 긴 줄표(—)를 쓰지 않는다. 문장을 나누거나 쉼표·쌍점·괄호·가운뎃점으로 쓴다. 코드 주석·설계 문서는 그대로 써도 된다.

## Windows 판 (windows/)

Windows 판은 `windows/` 아래에만 있다(최상위 macOS 파일은 고치지 않는다). Rust 로 쓰고, 보드·알림 패널은 Tauri 2(Svelte 5 + TypeScript + Vite), hook·명령줄 도구는 순수 Rust 실행 파일이다.
`windows/docs/SPEC.md` 가 macOS 소스를 역분석한 단일 기준 명세다 — 동작을 바꾸거나 더할 때 먼저 여기에 맞춘다.

### 구조

- `crates/core`(라이브러리: 파일 약속·hook 상태 규칙·hook 등록·경로·Windows 프로세스 도구)와 `crates/hook`(`claude-board-hook.exe`: `state`·`notify`·`setup`·`uninstall`·`install-hooks`·`uninstall-hooks`·`tui`)은 `windows/Cargo.toml` 워크스페이스다.
- `app/` 은 프론트엔드(`package.json`·`src/`), `app/src-tauri/` 는 Tauri 백엔드(`ClaudeBoard.exe` — 인자 없음 = 보드, `--notify …` = 알림 패널 한 개)다. 백엔드는 워크스페이스 밖이라 **Cargo.lock 이 두 개**(`windows/Cargo.lock`, `windows/app/src-tauri/Cargo.lock`)다.
- `packaging/` — `install.ps1`(설치·업데이트, 보드의 업데이트 단추도 이것을 `-Version <v> -Update` 로 실행), `uninstall.ps1`, `nsis-hooks.nsh`(setup.exe 설치 후 `setup`, 제거 전 `uninstall`), `release.sh`(배포 준비 도우미).
- `ci/` — CI smoke 작업이 Windows 러너에서 돌리는 설치·화면 확인 스크립트(setup.exe·install.ps1 무인 설치·제거, 보드·알림 패널 스크린샷). Windows 실기 시험은 `crates/*/tests/windows_*.rs` 와 src-tauri 의 `#[cfg(windows)]` 시험이다.
  GUI 시험 도구도 여기 있다: `ci/gui`(격리 보드 실행기·철선·CDP 도구)와 `ci/jev`(Jev 시험 협업 도구)와 가짜 claude(`ci/fake-claude-tty.cjs`·`ci/fake-claude-login.cjs`). 시험은 `CLAUDE_BOARD_CLAUDE` 로 보드가 치는 `claude` 를 가짜로 바꿔 진짜 claude 를 띄우지 않는다(`CLAUDE_BOARD_HOME` 이 함께 있을 때만 켜지고, 어긋나면 명령을 치지 않고 실패한다). 명령과 규칙은 `windows/docs/SPEC.md` §5.13 이 소유한다.
- 파일 약속(`~/.claude/notifier/` 의 파일 이름·JSON 필드)은 macOS 와 같다. 한쪽에서 바꾸면 다른 쪽도 고친다.
- 실행 파일 이름 `ClaudeBoard.exe`·`claude-board-hook.exe`, 릴리스 자산 이름(`ClaudeBoard-windows-x64.zip`(+`.sha256`)·`install.ps1`·`VERSION`)을 바꾸면 이미 설치된 hook·업데이트가 깨진다.
- hook exe 는 Tauri sidecar(`externalBin`)로 setup.exe 에 묶인다 — 빌드 전에 `app/src-tauri/binaries/claude-board-hook-x86_64-pc-windows-msvc.exe` 로 복사해 두어야 한다(CI 가 한다).
- `install.ps1`·`uninstall.ps1` 은 Windows PowerShell 5.1 에서도 돌아야 한다(`??`·삼항·`&&` 금지). `irm | iex` 로 호출한 셸 안에서 돌므로 `exit` 를 쓰지 않고, BOM 없이 코드는 ASCII 로만 쓴다.

### 명령

```sh
cd windows && cargo test                                    # core·hook 단위 테스트 (macOS 에서도 돈다)
cd windows && cargo check --target x86_64-pc-windows-msvc   # Windows 전용 코드 컴파일 확인(링크 전까지). rustup target add x86_64-pc-windows-msvc 필요
cd windows/app && npm run check && npm test                 # 프론트엔드 타입 검사(svelte-check)와 순수 함수 시험(node --test src/lib/**/*.test.ts)
cd windows/app && npm ci && npm run tauri dev               # 보드를 띄워 화면 확인 (macOS 에서는 가짜 세션 파일로)
```

### 버전과 배포

- 버전은 `windows/VERSION` 한 곳에 둔다(macOS 와 같은 번호, 첫 판 2.12). Tauri 에는 CI 가 semver(2.12 → 2.12.0)로 넘긴다.
- 배포: `windows/CHANGELOG.md` 맨 위에 `## <버전> — <날짜>` 항목 → `windows/VERSION` 올리기(`windows/packaging/release.sh <버전>` 이 항목 확인 후 바꾸고, README·설치 가이드의 setup.exe·릴리스 링크도 새 버전으로 바꿔 준다) → main 에 push.
  `.github/workflows/windows.yml` 이 빌드·테스트하고 smoke 작업으로 실제 설치를 확인한 뒤(실패하면 릴리스하지 않음), 태그 `windows-v<버전>` 이 아직 없으면 그 항목을 설명으로 릴리스를 만든다(항목이 없으면 실패).
  push 전에 브랜치에서 `gh workflow run windows.yml --ref <브랜치> -f dry_run=true` 로 릴리스 작업의 모든 확인을 돌려 볼 수 있다(아무것도 만들지 않고 gh 명령만 보여 준다). `dry_run=false` 는 main 에서만 실제 릴리스를 만든다.
- **Windows 릴리스는 절대 Latest 로 표시하지 않는다**(`--latest=false`). 설치된 macOS 앱이 `releases/latest/download/...` 를 읽기 때문에 Windows 릴리스가 Latest 가 되면 macOS 업데이트가 깨진다.
  그래서 Windows 쪽 코드·스크립트는 `/releases/latest` 를 쓰지 않고 릴리스 목록에서 `windows-v` 태그를 고른다. macOS 의 `packaging/release.sh` 는 최상위 `CHANGELOG.md` 만 본다.
- 사용자에게 보이는 기능이 바뀌면 `windows/CHANGELOG.md`·`windows/README.md`·`windows/docs/설치 가이드.md` 를 함께 고친다.
