#!/usr/bin/env python3
# 장난감 사용량 파일 쓰기(6단계 ⑫-6, 시험 빌드만). 자율주행 엔진 시험의 계정 사용률 자리 — 여러 엔진이 함께 읽는다.
# 시험 실행의 run.json 에 "usage_path": "<이 파일이 쓴 JSON 절대 경로>"(--dev 출발만 받음, 설치판 엔진은 듣지 않음)를 넣으면
# 엔진이 사용률을 이 파일에서 먼저 읽고(규칙 apUsageSteps), 공용 캐시 /tmp/.claude-usage-cache.json 과 사용량 API 는 보지 않는다.
# 꼴은 공용 캐시와 같은 퍼센트 꼴({five_hour:{utilization, resets_at}, seven_day:{…}}). 바깥으로 아무것도 보내지 않는다.
#
# 쓰는 법: 장난감 사용량.py <파일> <주간 %> [--reset-sec <주간 초기화까지 초, 기본 3일>] [--five <5시간 %, 기본 10>]
#   예) 두 실행 모두 멈출 선 대기: 멈출 선(예 50%)보다 높게 쓰고(--reset-sec 150 이면 2분 반 뒤 창이 초기화돼 이어 감)
#       자리 시험: 주간 27% · 멈출 선 30% · 단위 주간 %p 상한 5 이면 한 실행이 자리를 잡는 동안 다른 실행은 기다린다
import argparse, datetime, json, os, time

p = argparse.ArgumentParser()
p.add_argument("path")
p.add_argument("week", type=float)
p.add_argument("--reset-sec", type=float, default=3 * 86400)
p.add_argument("--five", type=float, default=10)
a = p.parse_args()

iso = lambda t: datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")
now = time.time()
j = {"five_hour": {"utilization": a.five, "resets_at": iso(now + 4 * 3600)},
     "seven_day": {"utilization": a.week, "resets_at": iso(now + a.reset_sec)}}
path = os.path.abspath(a.path)
if os.path.realpath(path) == "/private/tmp/.claude-usage-cache.json":   # 설치된 보드·상태줄과 함께 쓰는 캐시에는 쓰지 않는다
    raise SystemExit("공용 사용량 캐시에는 쓰지 않습니다. 시험 폴더의 다른 파일을 주세요")
tmp = path + ".tmp-%d" % os.getpid()
with open(tmp, "w") as f:
    json.dump(j, f)
os.replace(tmp, path)   # 엔진이 반쯤 쓴 파일을 읽지 않게 바꿔 넣는다
print(json.dumps(j))
