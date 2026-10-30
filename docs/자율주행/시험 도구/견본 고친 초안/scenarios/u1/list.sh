#!/bin/sh
# AC-2: GET /todos 가 있다(고친 판정: 글자를 찾지 않고 app/todos.py 를 불러 ROUTES 목록에 그 경로가 있는지 본다)
python3 - <<'PY'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("todos", "app/todos.py")
m = importlib.util.module_from_spec(spec)
try:
    spec.loader.exec_module(m)
except BaseException as e:
    print("app/todos.py 를 불러오지 못했습니다:", type(e).__name__); sys.exit(1)
r = getattr(m, "ROUTES", None)
sys.exit(0 if isinstance(r, list) and any(x.startswith("GET") and x.endswith("/todos") for x in r) else 1)
PY
