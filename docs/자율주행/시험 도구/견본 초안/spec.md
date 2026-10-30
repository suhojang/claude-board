# 명세: 할 일 API (장난감 견본)

가짜 claude 의 초안 세션 흉내가 결과 폴더에 그대로 복사하는 견본입니다(5단계 ⑪-2 시험, 값은 장난감).

## 요구사항
- R-1: 할 일을 만들고 목록으로 본다. 근거: https://www.figma.com/design/zzToyFile1/todo?node-id=1-2
- R-2: 처음 쓰는 사람을 위한 안내가 있다. 근거: https://zz-toy.example.com/wiki/spaces/TOY/pages/48213

## 수용 기준
- AC-1 (R-1): Given 서버 코드가 있고, When 할 일 만들기 경로를 찾으면, Then POST /todos 가 있다.
- AC-2 (R-1): Given 서버 코드가 있고, When 목록 경로를 찾으면, Then GET /todos 가 있다.
- AC-3 (R-2): Given README, When 처음 쓰는 사람이 읽으면, Then 띄우는 법이 쉽게 읽힌다.
