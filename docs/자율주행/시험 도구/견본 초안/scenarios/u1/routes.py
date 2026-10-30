#!/usr/bin/env python3
# AC-1: POST /todos 가 있다(장난감: app/todos.py 의 ROUTES 를 본다)
import pathlib, sys
p = pathlib.Path("app/todos.py")
if not p.exists():
    print("app/todos.py 가 없습니다"); sys.exit(1)
sys.exit(0 if "POST /todos" in p.read_text() else 1)
