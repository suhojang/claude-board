#!/usr/bin/env python3
# AC-1: POST /todos 가 있다(고친 판정: 글자가 아니라 app/todos.py 를 불러 ROUTES 값을 본다)
import importlib.util, sys
spec = importlib.util.spec_from_file_location("todos", "app/todos.py")
m = importlib.util.module_from_spec(spec)
try:
    spec.loader.exec_module(m)
except BaseException as e:
    print("app/todos.py 를 불러오지 못했습니다:", type(e).__name__); sys.exit(1)
r = getattr(m, "ROUTES", None)
sys.exit(0 if isinstance(r, list) and "POST /todos" in r else 1)
