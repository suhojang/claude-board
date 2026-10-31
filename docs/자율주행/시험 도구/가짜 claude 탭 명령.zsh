#!/bin/zsh
# 보드 탭 시험용 claude 명령(8단계 ⑫-8, 터미널로 이어받기): 가짜 claude 를 프로세스 이름(argv[0]) claude 로 띄운다.
# 보드는 탭의 앞 프로세스가 claude 인지를 argv[0] 의 이름으로 보고(isLiveClaude, 쥔 탭 목록·다시 뜰 때 되살리기), 엔진은 명령줄의 claude 조각과
# 세션 id 로 본다(apClaudeHolds). 파이썬 스크립트를 그대로 띄우면 argv[0] 이 파이썬이라 보드가 셸만 남은 화면으로 본다.
# 쓰는 법: 시험 자리(scratchpad)에 이 파일을 이름 claude 로, 가짜 claude.py 를 같은 폴더에 fake-claude.py 로 복사하고(옆에 fake-config.json 을 두면
# 장난감 대화 기록·대본을 씀), 시험 빌드 보드 설정 autopilotTabClaude 를 그 claude 의 절대 경로로(빈칸 없는 자리). 설치판 보드는 이 설정을 듣지 않는다.
# 프레임워크 파이썬은 bin/python3 이 Python.app 으로 다시 exec 하며 argv[0] 을 버려서 Python.app 안의 실행 파일을 바로 띄운다(없으면 실제 실행 파일)
py=$(/usr/bin/env python3 -I -c 'import sys, os; a = os.path.join(sys.base_prefix, "Resources/Python.app/Contents/MacOS/Python"); print(a if os.path.exists(a) else os.path.realpath(sys.executable))')
exec -a claude "$py" -I "${0:A:h}/fake-claude.py" "$@"
