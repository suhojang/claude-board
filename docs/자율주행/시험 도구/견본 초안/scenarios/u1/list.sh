#!/bin/sh
# AC-2: GET /todos 가 있다
test -f app/todos.py || { echo "app/todos.py 가 없습니다"; exit 1; }
grep -q "GET /todos" app/todos.py
