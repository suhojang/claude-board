# Jev 시험 협업 도구

Windows 판 보드 화면 시험에서, DOM 으로 읽을 수 없는 픽셀 사실(터미널은 WebGL 캔버스다)을 Jev(TypeSafe System One)에 묻는 도구다.
Jev 는 조언자다. 모든 GUI 시험 행은 결정적 증거(DOM 읽기, 키 기록, argv 기록, 파일 존재, `term.buffer` 검사)를 따로 갖고, Jev 는 별도 칸에만 들어간다.
Jev PASS 는 결정적 FAIL 을 PASS 로 바꾸지 못하고, Jev FAIL·UNSURE 는 자동 실패가 아니다. 어긋나면 사람이 그 스크린샷을 열어 읽고 기록한다.

HTTP·키 조회·캐시 형식은 하네스 `~/.claude/skills/ui-automation/scripts/jevvision/` 이 소유한다. 이 폴더는 그것을 하위 프로세스로 부를 뿐이고, 키를 읽지도 쓰지도 넘기지도 않는다.
의존 패키지가 없다(Node 24 ESM, 내장 모듈만).

## 명령

```sh
node windows/ci/jev/jev.mjs check   --image windows/e2e/out/board.png --family chip
node windows/ci/jev/jev.mjs screen  --image windows/e2e/out/board.png --choices board-expanded,settings --expect settings
node windows/ci/jev/jev.mjs crop    --in <전체 스크린샷.png> --rect x,y,너비,높이 --out windows/e2e/out/board.png --window-title "<창 제목>"
node windows/ci/jev/jev.mjs check   --image windows/e2e/out/board.png --family chip --allowed-window-title "<창 제목>"
node windows/ci/jev/jev.mjs report  --rows rows.json
node windows/ci/jev/jev.mjs measure --dataset windows/e2e/out/calib
node windows/ci/jev/jev.mjs selftest
node --test windows/ci/jev/
```

출력 첫 줄이 상태 낱말(`PASS|…`, `FAIL|…`, `UNSURE|…`, `UNVERIFIED|…`, `GUARD|…`, `USAGE|…`)이라 UNVERIFIED 를 통과로 읽을 수 없다.
종료코드는 0 PASS, 1 FAIL, 2 사용법, 3 UNSURE, 5 GUARD, 6 UNVERIFIED 다. 보정 안 됨 안내(`NOTE|…`)와 확신도는 stderr 로 나간다. `--verbose` 는 `DETAIL|` 한 줄(하네스 오류 줄, 긴 토큰은 가림)을 더한다.

| 파일 | 역할 |
|---|---|
| `questions.mjs` | 질문 가족(문구·기대값·보정 상태), 알려진 화면, 신뢰 문턱, 호출 상한, 이미지 한도. 사람이 검토하는 유일한 파일 |
| `port.mjs` | `Judge` 포트(`check`·`checkAll`·`screen`)와 판정 모양 `{status, reason}` |
| `adapters/jevlook.mjs` | 실제 어댑터. `jevvision.cli` 를 하위 프로세스로 부른다 |
| `adapters/fakeJudge.mjs` | 시험용. 정해 두지 않은 질문은 UNVERIFIED 다 |
| `adapters/guardedJudge.mjs` | 이미지·글을 먼저 검사하고 검사한 바이트의 임시 사본을 안쪽에 주는 데코레이터 |
| `cache.mjs` | 판정 해시 캐시 데코레이터 |
| `domain/verdict.mjs` | 종료코드와 출력 줄에서 판정으로. 순수 함수 |
| `domain/guard.mjs`, `domain/png.mjs` | 위치·PNG 검사·자른 그림 manifest 대조, PNG 읽기·자르기·쓰기 |
| `domain/privacy.mjs` | 보내는 글의 개인정보 검사(사용자 이름·`\Users\` 경로·이메일·긴 토큰). 순수 함수 |
| `report.mjs`, `measure.mjs`, `domain/stats.mjs` | 종합 표, 보정 측정, Wilson 하한 |

## 판정 규칙

- 어댑터는 종료코드와 결과 줄이 둘 다 맞을 때만 PASS·FAIL·UNSURE 를 낸다. 종료코드 0 인데 줄이 없거나, 종료코드 1 인데 `CHECK|FAIL` 줄이 없으면(파이썬 예외도 1 이다) UNVERIFIED 다.
- 하네스 종료코드 매핑: 0 PASS, 1 FAIL, 3 UNSURE, 6 키 없음이면 `UNVERIFIED|no Jev key`(오류 줄이 키 이름을 말할 때), 그 밖의 6 은 `jev precondition`, 7 은 `jev error or cache miss`, 2·4·5 는 각각 usage·budget·guard 사유의 UNVERIFIED.
- 시간 초과는 `jev timeout`, 파이썬을 못 띄우면 `jevvision not runnable` 이다. 둘 다 UNVERIFIED 다.
- 비-0 종료코드는 어떤 경로로도 PASS 가 되지 않는다(`domain/verdict.test.mjs` 가 rc 1..255 를 돌려 본다).
- 보정 전 가족은 모두 `not calibrated` 이고 조언으로만 쓴다. `--require-calibrated` 를 주면 호출 없이 `UNVERIFIED|family not calibrated` 로 끝난다.

## 이미지 가드

이미지는 `<repo>/e2e/out/` 또는 `<repo>/e2e/artifacts/` 아래의 PNG 파일이어야 한다. 기본 `<repo>` 는 `windows/` 이고 `--repo` 로 바꾼다.
하네스 가드(`guard.ensure_under_e2e`)가 `<--repo>/e2e/` 아래만 허용하므로 그 안쪽에서도 git 이 무시하는 두 하위 폴더(`windows/.gitignore` 의 `/e2e/out/`, `/e2e/artifacts/`)로 좁힌다.
다음은 호출 전에 `GUARD|…`, 종료코드 5 로 거절한다. 판정자(캐시·하위 프로세스)에는 닿지 않는다.

- 허용 폴더 밖 파일, `..` 나 링크로 벗어난 경로, 없는 파일
- 확장자가 `.png` 여도 PNG 헤더가 아닌 파일
- 16 MiB 를 넘는 파일, 하네스 한도(`questions.mjs` 의 `IMAGE_LIMITS`, 하네스 `thresholds.py` 의 사본) 안으로 줄이거나 키울 수 없는 비율, 3,600만 픽셀을 넘는 원본
- 경로에 `aijinet-project` 조각이 있는 것

이미지는 한 번만 읽는다. 그 바이트를 검사하고, 같은 바이트를 `<repo>/e2e/out/jev-in-*/image.png` 임시 사본으로 써서 캐시와 하위 프로세스에 그 사본을 준다. 검사한 뒤에 원본이 바뀌어도 안쪽은 검사한 바이트를 본다. 끝나면 사본을 지운다.

보내는 글은 `domain/privacy.mjs` 가 막는다. 진술·기대값·이름·선택지에 Windows 사용자 이름, `\Users\` 경로, 이메일 주소, 32자 이상 이어진 토큰 같은 글자가 있으면 `GUARD|privacy guard: …` 종료코드 5 이고 안쪽에 닿지 않는다. 까닭 줄에는 걸린 글자를 적지 않는다.
`crop --window-title` 은 그림 옆에 `<그림>.manifest.json`(창 제목·사각형·자른 그림의 sha256)을 쓴다. `check`·`screen` 에 `--allowed-window-title` 을 주면 manifest 가 있고, sha256 이 그림과 같고, 창 제목이 같을 때만 지나간다. 주지 않으면 manifest 는 보지 않는다.

가드가 막지 못하는 것이 있다. 그림 픽셀 안에 무엇이 들었는지는 읽지 않으므로 보드 창만 담겼는지, 계정 칩의 이메일이나 사용량 패널이 들었는지는 알 수 없다(창 제목과 manifest 는 호출하는 쪽이 정한 값이다). 로컬 이미지 경로는 검사하지 않는다(하네스는 이미지 바이트를 보낸다. unchecked: 하네스 ledger 에 경로가 남는지).
호출하는 쪽이 이 규율을 지킨다.

- 격리한 보드 프로세스 자신의 창 내용만 넘긴다. CDP `Page.captureScreenshot` 은 보드 콘텐츠만 담는다. 전체 화면 캡처는 먼저 `crop` 으로 보드 창 사각형만 자른다.
- 실제 계정 데이터가 보이는 화면(사용량 패널, 실제 이메일이 든 계정 칩, 실제 로그인 페이지), 사용자의 바탕화면, 격리 보드 밖 창, 설치된 보드는 보내지 않는다.
- `crop` 은 Node 내장 `zlib` 만 쓴다. 8비트 비인터레이스 PNG(회색·회색+알파·RGB·RGBA)만 푼다. 팔레트·16비트·인터레이스는 거절한다. 사각형이 이미지 밖이면 줄이지 않고 거절한다.

## 캐시

판정 캐시는 (판정자 id·version + 진술 + 기대값 + 이미지 sha256)을 키로 `windows/e2e/out/jev-cache/v-<sha256>.json` 에 판정 한 개를 둔다. 같은 화면과 질문은 하네스를 다시 부르지 않는다.
UNVERIFIED 는 저장하지 않는다. 키가 없어서 못 본 것이 키가 생긴 뒤에도 못 본 것으로 남지 않게 하려는 것이다.
하네스 자신의 응답 캐시(`jev-cache/harness/`)는 별개의 안쪽 층이다. `--replay` 는 그쪽을 읽기 전용으로 돌려 캐시에 없으면 UNVERIFIED 로 끝낸다.
`checkAll` 은 캐시에 없는 질문만 한 요청으로 묶어 묻는다(`jevlook` 은 실행당 라이브 호출 6 회가 기본 상한이다).

## 종합 보고

`report` 는 `[{name, deterministic, jev: {status, reason}}]` 를 읽어 `check | deterministic | Jev | agreement` 표와 합계 줄을 낸다.

| 결정적 | Jev | 일치 칸 |
|---|---|---|
| PASS | PASS | agree |
| FAIL | FAIL | agree |
| PASS | FAIL, UNSURE | DISAGREE (사람이 스크린샷을 읽는다) |
| FAIL | PASS | DISAGREE (Jev PASS 는 결정적 FAIL 을 덮지 못한다) |
| FAIL | UNSURE | n/a |
| 아무거나 | UNVERIFIED | n/a (일치로 세지 않는다) |

종료코드는 결정적 판정만으로 정한다. 전부 PASS 일 때만 0 이고, FAIL 이 있으면 1, 그렇지 않고 UNVERIFIED 가 있거나 행이 없으면 6, 그렇지 않고 UNSURE 가 있으면 3 이다. Jev 칸은 종료코드를 바꾸지 못한다.

## 보정 절차 (J2)

`measure` 는 키가 생긴 뒤에 실행한다. 절차 전문은 `measure.mjs` 머리말에 있고 요약은 아래와 같다.

1. 데이터셋: 보드 mock 화면과 D1 재생 렌더로 PNG 를 `windows/e2e/out/calib/` 에 모으고, `truth.json` 에 `{"<파일>.png": {"family": "chip", "truth": "yes"|"no"}}` 로 정답을 적는다. 정답은 캡처 시점의 DOM 에서 뽑는다. 음성은 DOM 변형(칩 글자 바꾸기, 요소 숨기기, 행 겹치기, 구분선 지우기)이나 다른 가족의 진술로 만든다.
2. 가족마다 양성 30장 이상, 음성 30장 이상. 모자라면 호출 전에 `UNVERIFIED|dataset too small` 로 끝난다.
3. `node windows/ci/jev/jev.mjs measure --dataset windows/e2e/out/calib` 가 가족별 `MEASURE|…` 줄을 낸다: 적중률(불확실은 못 맞힌 것으로 센다), 결정한 답의 정확도와 Wilson 하한, 불확실(UNSURE) 비율, 거짓 통과율(음성에서 PASS).
4. 신뢰 문턱은 `questions.mjs` 의 `TRUST` 다(전부 `judgment`): Wilson 하한 0.90 이상, 거짓 통과 30분의 1 이하, 불확실 0.35 이하. 넘은 가족만 사람이 `calibration.status` 를 바꾼다. 이 코드는 파일을 고치지 않는다.
5. 어느 그림이든 UNVERIFIED 가 나오면 그 자리에서 중단한다. 일부만 센 숫자로 신뢰를 판정하지 않는다.

키가 없으면 `measure` 는 `UNVERIFIED|no Jev key`(종료코드 6)로 끝난다.

## 소유자가 할 일

키는 소유자가 직접 넣는다. 이 저장소의 코드와 에이전트는 키 값을 다루지 않는다.

1. 제어판의 자격 증명 관리자에서 Windows 자격 증명에 일반 자격 증명을 추가한다. 인터넷 또는 네트워크 주소는 `typesafe-api-key`, 사용자 이름은 아무거나, 암호는 키 값이다. 또는 사용자 환경변수 `TYPESAFE_API_KEY` 를 정한다.
2. `cmdkey /pass:<값>` 은 쓰지 않는다. 키가 명령줄과 셸 기록에 남는다.
3. 하네스의 Windows 조회 코드(`key_source.py` 의 `windows_credential_manager_lookup`)와 `jevlook` 의 Windows venv 경로, venv 의 Pillow 는 하네스 쪽 작업이다. 끝난 뒤 `node windows/ci/jev/jev.mjs selftest` 가 `SELFTEST|PASS` 를 내면 연결된 것이다.

## 현재 상태와 한계

- 이 PC 에서 하네스 Windows 조회는 자리표시자다. 키 없이 실행하면 하네스가 `ERROR|6|PreconditionError|Windows 자격 증명 관리자 조회는 아직 구현되지 않았다(gamerig-win 몫). 환경변수 TYPESAFE_API_KEY 를 쓴다` 를 내고, 어댑터는 이를 `UNVERIFIED|no Jev key`, 종료코드 6 으로 옮긴다.
- 하네스 venv(`%USERPROFILE%\.local\share\ui-automation\venv`)가 없으면 `python -I` 를 쓴다. 그 파이썬에 Pillow 가 있어야 하네스가 이미지를 줄일 수 있다.
- untested: Jev 를 실제로 부른 적이 없다. 하위 프로세스 인자 전달(따옴표가 든 진술)은 Node 스텁으로만 확인했고 파이썬 쪽 argv 해석은 확인하지 않았다. `measure` 의 라이브 실행도 한 번도 하지 않았다.
- untested: Windows 에서의 호출 전체, 이 보드의 어두운 UI, 작은 크기의 WebGL 터미널 글자에 대한 Jev 정확도. 보정 전 모든 가족은 `not calibrated` 다.
- unchecked: 하네스 `thresholds.py` 와 `IMAGE_LIMITS` 의 일치는 하네스가 있는 머신에서만 시험이 대조한다.
