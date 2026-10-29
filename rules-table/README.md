# 공유 규칙 표(`rules-table/`)

이 폴더는 macOS 보드와 Windows 판이 **같은 JSON 줄을 읽고 같은 판정을 하는지** 견주는 공유 시험 표다.
줄 하나가 "입력(input) 이 이렇게 들어오면 기대(expect) 가 이렇게 돼야 한다" 는 하나의 규칙이다.
새 표 파일을 넣지 않는 동안에도 이 문서가 꼴의 소유자고, 양쪽 돌리는 쪽(`windows/app/src-tauri/src/rules_table.rs`,
`windows/app/src/lib/rulesTable.ts`, macOS `Tests/BoardRulesTests/RulesTableTests.swift`)이 로더 시험으로 이 꼴을 지킨다. 같은 성격의 앞 예는 `hook-table/` 이다.

## 파일과 rule

| 파일 | rule |
|---|---|
| `README.md` | 꼴·표시·유지 규칙 (이 문서) |
| `sessions-group.json` | `group` |
| `session-scan.json` | `scan` |
| `usage.json` | `parse` · `cache` · `settle` |
| `autoswitch.json` | `decide` · `failed` |
| `pairing.json` | `screen-session` · `place` · `workspace-dot` |

## 꼴 (schema 1)

```json
{
  "family": "usage",
  "schema": 1,
  "cases": [
    {
      "id": "usage.parse.same-model-name-three",
      "rule": "parse",
      "why": "모델별 주간 한도의 이름이 같으면 id 가 겹친다. 처음 것은 그대로, 뒤의 것은 -2·-3 을 붙인다. 라벨은 그대로",
      "source": "PR #20 U1",
      "input": {"body": {"five_hour": {"utilization": 10}}},
      "expect": {"limits": [{"id": "5h", "label": "5시간", "pct": 10}]}
    }
  ]
}
```

- `id`: `<family>.<rule>.<영문 slug>`. 파일 안에서 유일. **한 번 쓴 id 는 바꾸거나 다른 뜻으로 다시 쓰지 않는다.** 줄을 없애면 그 id 는 다시 쓰지 않는다.
- `why`: 사람이 쓴 의도한 동작(한국어). 필수. Swift·Rust 출력으로 만들지 않는다.
- `source`: 근거(PR·주인 문장). 선택.
- `input`·`expect`: 키는 rule 마다 아래 표가 고정한다. **모르는 키가 있으면 돌리는 쪽이 실패한다.**
- `only`: `"mac"` 또는 `"windows"`. 한 판에만 해당하는 줄에만 붙인다. 붙인 까닭은 같은 줄의 `why` 에 함께 적는다. 없으면 둘 다 돌리는 줄이다.
- 시각: **지금 = 0 기준 초**(음수 = 과거). 한도 `resets` 도 초(Windows 는 ms 로 바꿔 넣는다).
  단 `parse` 의 expect `resets` 는 응답의 `resets_at` 을 푼 **1970 기준 ms**(지금과 관계없는 절대 시각)다.
- 계정: 이름 `"A"` 는 id `"A|org"`, 이메일은 `"a@example.com"` 처럼 쓴다.
- JSON 이 아닌 몸체는 `"body_raw": "<글자>"` 로 준다.

## 돌리는 쪽 의무

1. 파일의 모든 줄을 돌린다.
2. 모르는 rule 은 실패다.
3. 실패한 id 를 모두 모아 한 번에 보인다(첫 실패에서 끊지 않는다).
4. 돌린 줄이 0 이면 실패다.
5. `only` 가 자기 판이 아닌 줄(`"mac"` 은 Windows 쪽, `"windows"` 는 mac 쪽)은 건너뛴다.

## rule 마다 입력·기대

- `group`: input `records`(`[{file, record}]`, record 는 세션 파일 JSON 그대로, `since`·`updated` 는 지금 기준 초), `pinned`(경로 목록) → expect `groups`(`[{id, sessions:[세션 id]}]`, 화면 차례).
- `scan`: input `file`, `record`, `transcript`(bool), `claude_sessions`(`[{pid, sessionId, age}]`, age = 그 파일 mtime 이 지금보다 몇 초 전), `procs`(`{"<pid>": "alive"/"gone"/"denied"}`, 없는 pid = gone), `paused`(bool) → expect `kept`(bool), `state`(kept 일 때).
- `parse`: input `body` 또는 `body_raw` → expect `limits`(`[{id, label, pct, resets?}]`) 또는 `null`(실패).
- `cache`: input `modified`(초, null = 파일 없음), `trust_after`(초, 없으면 아주 먼 과거), `body`/`body_raw`, `fetching` → expect `cached`(한도 id 목록 또는 null), `fetch`(bool).
- `settle`: input `body`/`body_raw`/없음, `current_empty`(bool) → expect `applied`, `cache_written`, `failed`(true/false/null = 그대로).
- `decide`: mac `AutoSwitchTests.swift` 의 DecideRow·PauseSpec 과 같은 이름(`sessions[{id, state, limit_at, cwd, message}]`, `limits[{id, pct, resets}]`, `current`, `saved`, `expired`, `full`, `resumed`, `last_switch`, `pending`, `switching`, `pause{account, until}`) → expect `move`(`"stay"`, `"switch A→B [<까닭>]"`(+`" stopped"`), `"none [<까닭>]"`, `"unknown [<까닭>]"`), `marks`(null = 그대로), `resume`(세션 id 목록).
- `failed`: input `side`(`target`/`other`/`busy`), `target`, `current`, `full` → expect `marks`, `pause`(null 또는 `{account, until}`).
- `screen-session`: input `screens[{id, tty, session_id}]`, `sessions[{id, state, tty, updated, since, headless}]` → expect `screen_session`(`{화면 id: 세션 id 또는 null}`), `paired`(`{세션 id: 화면 id}`).
- `place`: 위와 같고 + `runs_claude`(화면 id 목록), `owned`(화면 안 headless 세션 id 목록) → expect `place`(`{세션 id: "tab:<화면>" / "terminal" / "background" / "ended"}`).
- `workspace-dot`: input `states`(화면 차례의 세션 상태, 빈 목록 가능) → expect `state`.

Windows 대응: `tty` = TS `Session.tty`(TermId) = `surface.termId`, `runs_claude` = `runsClaude`, `typedAt` = null.
mac 대응: `tty` = `TabScreen.tty`, `runs_claude` = 가짜 `TabProbe.runsClaude`.