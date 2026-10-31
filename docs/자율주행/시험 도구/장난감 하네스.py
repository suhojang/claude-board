#!/usr/bin/env python3
# 장난감 하네스 저장소 만들기(7단계 ⑫-7b 하네스 읽기 시험) — 실제 프로젝트 하네스는 읽지 않고 이것으로만 시험한다.
# 쓰는 법: python3 -I 장난감 하네스.py <새 저장소 경로> [--edges]      저장소를 새로 만든다(main·dev·live·master, 출발 가지 live)
#          python3 -I 장난감 하네스.py <저장소 경로> --change           live 에 하네스 파일 하나(.claude/rules/release.md)를 고쳐 커밋한다
#          python3 -I 장난감 하네스.py <저장소 경로> --scenario         live 에 판정 시나리오(.autopilot/scenarios/u1/check.sh)를 커밋한다(이어 가기 시나리오 고치기 시험)
# 확인 지점(기계로 찾아야 하는 자리)은 규칙 파일·스킬·hook·설정·CLAUDE.md 에 섞어 둔다. 지나쳐야 하는 줄(대기열·자주 묻는 질문·부정·결제 승인 API)도 함께.
# --edges 는 읽지 말아야 할 자리(바로 가기 파일·큰 파일·node_modules·.claude/worktrees·깊이 5 의 CLAUDE.md)를 더한다.
# 값은 모두 가짜(zz-toy·example.com)이고 토큰 꼴 글자는 쓰지 않는다
import os, subprocess, sys

def git(repo, *a):
    subprocess.run(["git", "-C", repo, *a], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def write(repo, rel, text, mode=None):
    p = os.path.join(repo, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", encoding="utf-8") as f:
        f.write(text)
    if mode:
        os.chmod(p, mode)

FILES = {
    "CLAUDE.md": """# 장난감 프로젝트 규칙

이 저장소는 자율주행 하네스 읽기 시험용 장난감(zz-toy)입니다.
배포 전에는 사람에게 확인받고 진행한다.
모호하면 사용자에게 묻는다.
대기열 파일(queue.txt)을 먼저 읽는다.
자주 묻는 질문은 docs/faq.md 에 있다.
사람에게 묻지 않고 끝까지 진행한다.
결제 승인 API 는 src/pay.py 에 있다.
""",
    ".claude/rules/release.md": """# 릴리스 규칙
- 릴리스 노트는 미리보기로 사람에게 보여 준 뒤 올린다.
- 승인 없이 main 에 합치지 않는다.
- PR 은 리뷰어의 승인을 받은 뒤 머지한다.
""",
    ".claude/skills/ticket/SKILL.md": """---
name: ticket
description: 장난감 티켓 만들기
---
1. mcp__atlassian__createJiraIssue 로 티켓을 만든다.
2. 만든 뒤 AskUserQuestion 으로 담당자를 고른다.
3. 사용자의 답을 기다린 뒤 다음으로 간다.
""",
    ".claude/settings.json": """{
  "permissions": {
    "ask": ["Bash(git push:*)"]
  },
  "hooks": {
    "PreToolUse": [
      {"matcher": "Bash", "hooks": [{"type": "command", "command": ".claude/hooks/guard.sh"}]}
    ]
  }
}
""",
    ".claude/hooks/guard.sh": """#!/bin/sh
# 배포 명령은 막는다(장난감)
if grep -q deploy; then
  echo "배포는 사람이 합니다" >&2
  exit 2
fi
exit 0
""",
    "docs/faq.md": "# 자주 묻는 질문\n- 이 파일은 하네스가 아니다. 사람에게 묻는다.\n",
    "a/b/c/d/CLAUDE.md": "# 깊은 폴더 규칙\n여기 파일을 지우기 전에는 확인을 받는다.\n",
    "Claude.local.md": "# 개인 규칙(대소문자가 다른 이름)\n머지 전에 사람의 승인을 받는다.\n",
    "README.md": "# zz-toy\n장난감 저장소입니다.\n",
    "TODO.md": "# 할 일\n## 이번 주\n- [ ] 인사 파일 hello.txt 에 안녕 한 줄을 쓴다\n- [ ] 작별 파일 bye.txt 에 잘 가 한 줄을 쓴다\n",
}

EDGES = {
    "node_modules/x/CLAUDE.md": "묻는다\n",
    ".claude/worktrees/w1/CLAUDE.md": "사람에게 묻는다\n",
    "a/b/c/d/e/CLAUDE.md": "깊이 5 라 읽지 않는다. 사람에게 묻는다.\n",
    "build/CLAUDE.md": "빌드 결과. 사람에게 묻는다.\n",
}

def make(repo, edges):
    if os.path.exists(repo):
        sys.exit("이미 있는 경로입니다: " + repo)
    os.makedirs(repo)
    git(repo, "init", "-q", "-b", "main")
    git(repo, "config", "user.name", "zz-toy")
    git(repo, "config", "user.email", "zz-toy@example.com")
    for rel, text in FILES.items():
        write(repo, rel, text, 0o755 if rel.endswith(".sh") else None)
    if edges:
        for rel, text in EDGES.items():
            write(repo, rel, text)
        write(repo, ".claude/rules/big.md", "사람에게 묻는다\n" + "가" * (300 * 1024))
        os.symlink("/etc/hosts", os.path.join(repo, ".claude/rules/link.md"))
        write(repo, ".gitignore", "node_modules/\nbuild/\n.claude/worktrees/\n")
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", "장난감 하네스")
    for b in ("dev", "live", "master"):
        git(repo, "branch", b)
    git(repo, "checkout", "-q", "live")

def change(repo):
    p = os.path.join(repo, ".claude/rules/release.md")
    with open(p, "a", encoding="utf-8") as f:
        f.write("- 운영 DB 마이그레이션은 사람의 확인을 받은 뒤 돌린다.\n")
    git(repo, "add", ".claude/rules/release.md")
    git(repo, "commit", "-q", "-m", "하네스 고침: 마이그레이션 확인 줄")

def scenario(repo):
    write(repo, ".autopilot/scenarios/u1/check.sh", "#!/bin/sh\n# 인사 파일이 있는지(형식만 보는 약한 판정)\ntest -f hello.txt\n", 0o755)
    git(repo, "add", ".autopilot/scenarios/u1/check.sh")
    git(repo, "commit", "-q", "-m", "판정 시나리오 u1")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__ or "쓰는 법: 장난감 하네스.py <경로> [--edges|--change|--scenario]")
    repo = os.path.abspath(sys.argv[1])
    if "--change" in sys.argv:
        change(repo)
    elif "--scenario" in sys.argv:
        scenario(repo)
    else:
        make(repo, "--edges" in sys.argv)
    print(repo)
