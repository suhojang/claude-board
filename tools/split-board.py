#!/usr/bin/env python3
# 보드 앱 한 파일(ClaudeBoard.swift)을 Sources/ClaudeBoard/ 아래 여러 파일로 나눈다(이슈 #9).
# 코드는 옮기기만 한다 — 같은 입력에는 늘 바이트까지 같은 출력이 나온다.
#
# 쓰는 곳: main 의 나누기 커밋, 그리고 아직 나누기 전 ClaudeBoard.swift 를 고친 가지(자율주행 autopilot-stage2 등)에 main 을 합칠 때.
# 그 가지에서 나누기 커밋을 합치다 ClaudeBoard.swift 의 수정/삭제 충돌이 나면, 합친 ClaudeBoard.swift 로 이 스크립트를 돌려
# Sources/ClaudeBoard/ 를 다시 만들고 ClaudeBoard.swift 를 지운다(절차는 CLAUDE.md).
#
#   python3 -I tools/split-board.py                   # 저장소 맨 위 ClaudeBoard.swift → Sources/ClaudeBoard/*.swift (표에 없는 .swift 는 지움)
#   python3 -I tools/split-board.py --check           # ClaudeBoard.swift 를 나눈 결과가 Sources/ClaudeBoard/ 와 같은지만 본다
#   python3 -I tools/split-board.py --check --from C  # 커밋 C 의 ClaudeBoard.swift 를 나눈 결과가 지금 Sources/ClaudeBoard/ 와 같은지
#   --input <파일> 로 다른 입력을, --out <폴더> 로 다른 출력 자리를 줄 수 있다
#   나누기 커밋이 이 스크립트의 출력 그대로인지: 그 커밋을 꺼낸 작업 트리에서 --check --from <나누기 커밋>^
#
# 규칙
# - 나누는 단위: 맨 위(중괄호 밖) `// MARK: - <이름>` 절. 절 이름 → 파일 이름은 SECTION_FILES 표, 표에 없으면 fallback_name.
# - 첫 MARK 앞 머리(머리 주석·import)는 모든 파일 맨 위에 그대로 넣는다.
# - main.swift: 파일 끝의 시작 코드. 마지막 최상위 형식 선언(class·struct·enum·extension·protocol·actor)이 끝난 뒤부터
#   파일 끝까지를 옮긴다(앞의 빈 줄은 뺀다). 그 밖에 최상위 실행문(선언이 아닌 문장)이 있으면 실패한다
#   — 여러 파일 모듈에서 실행문은 main.swift 에만 둘 수 있다.
# - 전역 초기화 시점: 한 파일일 때는 최상위 let/var 가 모두 시작할 때 위에서부터 초기화됐다. main.swift 밖의 전역은
#   처음 쓸 때 초기화되므로, 부작용이 있는 전역(EAGER)은 main.swift 맨 앞에서 원래 차례대로 한 번 읽는다(TOUCH_COMMENT 묶음).
#   나머지는 LAZY 표에 까닭과 함께 둔다. 어느 표에도 없는 전역은 경고만 내고(처음 쓸 때 초기화) 표에 넣어 정하게 한다.
# - private·fileprivate: 파일이 나뉘면 다른 파일에서 안 보이므로, 컴파일에 필요한 선언만 ACCESS 표의 줄 머리로 찾아
#   private/fileprivate 를 뗀다(internal). 표 항목은 0번(그 가지에 없는 이름) 또는 1번 맞아야 하고, 2번 넘게 맞으면 실패한다.
#   맞은 선언도 그 이름을 다른 파일(주석·문자열 밖)이 쓸 때만 뗀다 — 같은 표로 가지마다 필요한 곳만 풀린다(main 출력은 그대로).
# - 각 파일 끝의 빈 줄은 뗀다(파일은 줄바꿈 하나로 끝난다).
# - 줄 보존 확인: 머리 복제·TOUCH 묶음·ACCESS 바꿈·끝 빈 줄을 되돌려 출력 파일을 절 차례로 이어 붙이면 입력과 바이트까지 같아야 한다.
#   어긋나면 아무것도 쓰지 않고 실패한다.
import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INPUT_NAME = "ClaudeBoard.swift"
OUT_REL = Path("Sources") / "ClaudeBoard"

# 절 이름 → 파일 이름(확장자 뺌). BoardRules 의 파일 이름(Accounts·AutoSwitch·Ports·SessionControl·Sessions·SessionScan·
# Shortcuts·TabPairing·Terminals·Updates·Usage)과 겹치거나 헷갈리지 않게 짓는다. main 에 없는 절(자율주행 가지)도 미리 둔다
SECTION_FILES = {
    "데이터": "BoardData",
    "계정 전환": "AccountSwitching",
    "사용 한도 자동 전환": "LimitSwitching",
    "상태 표시 (애니메이션)": "StatusAnimation",
    "화면": "BoardViews",
    "접기·펼치기": "Folding",
    "보드 글자 크기": "BoardFontScale",
    "힌트": "Hints",
    "브레인롯 캐릭터": "BrainrotMascots",
    "업데이트": "AppUpdate",
    "내장 터미널": "EmbeddedTerminal",
    "창": "BoardWindow",
    "창 크기 단계": "BoardTiers",
    "단축키": "KeyboardShortcuts",
    "설정 화면": "SettingsScreen",
    "자율주행": "Autopilot",   # autopilot-stage2 가지에만 있는 절
}
MAIN_FILE = "main"


def fallback_name(title, index):
    # 표에 없는 절: 이름이 영어(글자·숫자·공백)면 낱말을 이어 붙인 이름, 아니면 Section-<절 차례 두 자리>
    words = title.split()
    if words and all(re.fullmatch(r"[A-Za-z0-9]+", w) for w in words):
        return "".join(w[:1].upper() + w[1:] for w in words)
    return f"Section-{index:02d}"


# 시작할 때 바로 초기화해야 하는 전역(초기화식에 부작용이 있다). main.swift 맨 앞에서 입력 차례대로 `_ = 이름` 으로 읽는다
EAGER = {
    "unfocusedSplitOpacity": "사용자 Ghostty 설정 파일을 읽어 그 시점 값을 잡는다(시작할 때 읽던 값)",
    "mdRenderDir": "앱 안 md-render 를 ~/.claude/notifier/plugins/ 로 복사하고 실패를 board.log 에 남긴다"
                   "(--action·두 번째 실행도 잠금 확인 전에 했다)",
}

# 처음 쓸 때 초기화돼도 같은 전역 — 부작용 없는 상수·값, 바깥을 바꾸지 않는 객체
LAZY = {
    "appIcon": "앱 번들의 그림을 읽기만 한다(메인에서 처음 쓴다)",
    "hourMinute": "DateFormatter 만들기",
    "foldSpring": "애니메이션 값",
    "versionCache": "빈 캐시 값",
    "ink": "색 값",
    "terminalApps": "상수 표",
    "releaseRepo": "상수", "releaseBase": "상수", "changelogURL": "상수",
    "paneDividerWidth": "상수", "terminalMinWidth": "상수", "terminalTabBarHeight": "상수",
    "terminalWorkspaceBarHeight": "상수", "terminalAreaColor": "색 값", "terminalBarColor": "색 값",
    "workspaceKeys": "빈 사전",
    "ghosttyConfigFiles": "홈 폴더 경로 문자열",
    "mdRenderTokenPattern": "상수",
    "terminalExpandedMinSize": "상수 계산(뒤에 선언된 boardMinWidth 를 쓴다 — 릴리스 빌드에서는 그때도 정적 초기화된 값)",
    "builtinSessionCommand": "상수",
    "defaultSessionCommand": "계산 속성(저장 없음)",
    "terminalToggleWidth": "상수", "terminalToggleSink": "nil",
    "window": "nil(AppDelegate 가 넣는다)",
    "sizeGuard": "창 대리자 객체 만들기(상태 없음)",
    "boardMinWidth": "상수", "boardLaunchWidth": "상수",
    "miniWidth": "상수", "miniMinHeight": "상수", "miniRowHeight": "상수", "fullListMin": "상수", "mediumListMin": "상수",
    "notifyPositions": "상수 표",
    "ghosttyQueue": "DispatchQueue 만들기(작업 없음)",
    "systemTerminals": "상태 없는 어댑터", "boardTabs": "상태 없는 어댑터", "tabProbe": "상태 없는 어댑터",
    "terminalControl": "어댑터를 쥐는 객체 만들기(init 은 저장만)",
    "sessionControl": "어댑터를 쥐는 객체 만들기(init 은 저장만)",
    "sessionsDir": "홈 폴더 경로 문자열", "muteAllPath": "홈 폴더 경로 문자열", "notifyPositionPath": "홈 폴더 경로 문자열",
    # 자율주행 가지(autopilot-stage2)에만 있는 전역 — main 출력은 바꾸지 않는다
    "autopilotRoot": "계산 속성(저장 없음)", "apDraftsRoot": "계산 속성(저장 없음)",
    "apDayTime": "DateFormatter 만들기", "apSQLiteTransient": "상수(SQLITE_TRANSIENT)", "apDevBuild": "번들 id 비교",
}

TOUCH_COMMENT = ("// 다른 파일의 전역은 처음 쓸 때 초기화된다. 초기화식에 부작용이 있어 시작할 때 바로 초기화해야 하는 전역은 "
                 "여기서 먼저 읽는다(tools/split-board.py 의 EAGER 표)")

# private/fileprivate 를 떼는 선언 — 줄의 들여쓰기 뒤 머리. 다른 파일(절)에서 쓰여 컴파일에 필요한 것만 둔다
ACCESS = [
    "private let hourMinute = formatter(",   # LimitSwitching → BoardViews(끝난 시각 글자)
    "private func formatter(",               # LimitSwitching → 자율주행 가지의 Autopilot(apDayTime·날짜 글자). main 에서는 같은 파일만 써서 그대로
]

ACCESS_TOKENS = ("fileprivate(set) ", "private(set) ", "fileprivate ", "private ")
MODIFIERS = {"private", "fileprivate", "internal", "public", "open", "final", "nonisolated", "static",
             "indirect", "prefix", "postfix", "infix", "convenience", "required", "override", "dynamic", "lazy",
             "mutating", "nonmutating", "unowned", "weak", "package"}
DECL_KEYWORDS = {"import", "let", "var", "func", "class", "struct", "enum", "protocol", "extension", "typealias",
                 "actor", "precedencegroup", "operator", "macro", "init", "deinit", "subscript"}
TYPE_KEYWORDS = {"class", "struct", "enum", "extension", "protocol", "actor"}
MARK_PREFIX = "// MARK: - "


class SplitError(Exception):
    pass


# ---- Swift 를 줄 단위로 훑기: 각 줄이 시작할 때 중괄호·괄호 밖(최상위)인지, 주석·문자열 안인지, 코드만 남긴 글자 ----
def scan(text):
    lines = text.split("\n")[:-1]   # 입력은 줄바꿈으로 끝난다
    code_lines = []
    top_start = []
    joined = []   # 괄호 밖이지만 앞 줄에 이어진다고 본 줄
    stack = [["code", 0]]   # code / interp(괄호 수) / str(해시 수, 여러 줄) / block(겹침 수)
    depth = 0
    for line in lines:
        top_start.append(len(stack) == 1 and depth == 0)
        out = []
        i, n = 0, len(line)
        while i < n:
            ctx = stack[-1]
            c = line[i]
            if ctx[0] in ("code", "interp"):
                if line.startswith("//", i):
                    out.append(" " * (n - i))
                    break
                if line.startswith("/*", i):
                    stack.append(["block", 1]); out.append("  "); i += 2; continue
                m = re.match(r'(#*)("""|")', line[i:])
                if m:
                    hashes, q = len(m.group(1)), m.group(2)
                    stack.append(["str", hashes, q == '"""'])
                    out.append(m.group(0)); i += len(m.group(0)); continue
                if c in "([{":
                    if ctx[0] == "interp" and c == "(":
                        ctx[1] += 1
                    else:
                        depth += 1
                elif c in ")]}":
                    if ctx[0] == "interp" and c == ")":
                        if ctx[1] == 0:
                            stack.pop(); out.append(c); i += 1; continue
                        ctx[1] -= 1
                    else:
                        depth -= 1
                out.append(c); i += 1; continue
            if ctx[0] == "block":
                if line.startswith("/*", i):
                    ctx[1] += 1; out.append("  "); i += 2; continue
                if line.startswith("*/", i):
                    ctx[1] -= 1; out.append("  "); i += 2
                    if ctx[1] == 0:
                        stack.pop()
                    continue
                out.append(" "); i += 1; continue
            # 문자열
            hashes, multi = ctx[1], ctx[2]
            close = ('"""' if multi else '"') + "#" * hashes
            if line.startswith(close, i):
                stack.pop(); out.append(close); i += len(close); continue
            esc = "\\" + "#" * hashes
            if line.startswith(esc, i):
                j = i + len(esc)
                if j < n and line[j] == "(":
                    stack.append(["interp", 0]); out.append(" " * (j - i) + "("); i = j + 1; continue
                out.append(" " * min(len(esc) + 1, n - i)); i = j + 1; continue
            out.append(" "); i += 1
        if stack[-1][0] == "str" and not stack[-1][2]:
            raise SplitError(f"{len(code_lines) + 1}행: 한 줄 문자열이 줄 끝에서 닫히지 않았다")
        code_lines.append("".join(out))
    if len(stack) != 1 or depth != 0:
        raise SplitError("파일 끝에서 괄호·문자열·주석이 닫히지 않았다 — 훑기 규칙을 넓혀야 한다")
    # 괄호 밖이어도 앞 줄에 이어지는 줄(여러 줄 선언 머리 `class A: B,` ↵ `C {`, 줄 머리 `.`·`where` 등)은 최상위 시작이 아니다.
    # 앞 줄 끝은 좁게 본다 — `var x: T?` 처럼 끝나는 선언 다음 줄을 삼키지 않게. 삼킨 줄이 선언이면 split 이 실패한다
    prev = None
    for i, c in enumerate(code_lines):
        s = c.strip()
        if not s:
            continue
        if top_start[i] and prev is not None and (prev.endswith(CONTINUE_END) or s.startswith(CONTINUE_START)):
            top_start[i] = False
            joined.append(i)
        prev = s
    return lines, code_lines, top_start, joined


CONTINUE_END = (",", "=", "->", "&&", "||")
CONTINUE_START = (".", "where ", "&&", "||", "->", "?", ":", "+")


def classify(code):
    # 최상위에서 시작하는 줄의 종류: blank / comment / decl:<키워드> / attr / directive / stmt
    s = code.strip()
    if not s:
        return "blank"
    s = strip_modifiers(s)
    if not s:
        return "attr"
    if s.startswith("#"):
        return "directive"
    m = re.match(r"\w+", s)
    if m and m.group(0) in DECL_KEYWORDS:
        return "decl:" + m.group(0)
    return "stmt"


def strip_modifiers(s):
    # 줄 머리의 속성(@MainActor 등)과 수식어(private 등)를 뗀다
    while True:
        m = re.match(r"@\w+(\([^)]*\))?\s*", s)
        if m:
            s = s[m.end():]; continue
        m = re.match(r"(\w+)(\((set|unsafe)\))?\s+", s)
        if m and m.group(1) in MODIFIERS:
            s = s[m.end():]; continue
        return s


def decl_names(code):
    # 최상위 let/var 선언 줄에서 이름들(let a = …, b = … 꼴도) — 괄호 밖 쉼표로 나눈 각 묶음의 첫 이름
    m = re.match(r"(let|var)\s+(.*)", strip_modifiers(code.strip()))
    if not m:
        return []
    names, depth, part = [], 0, ""
    for ch in m.group(2):
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == "," and depth == 0:
            names.append(part)
            part = ""
        else:
            part += ch
    names.append(part)   # 마지막 묶음(줄이 괄호 안에서 끝나도)
    return [mm.group(1) for mm in (re.match(r"\s*(\w+)", p) for p in names) if mm]


def declared_names(code):
    # 선언 줄(최상위·멤버)에서 선언하는 이름들
    s = strip_modifiers(code.strip())
    if re.match(r"(let|var)\b", s):
        return decl_names(s)
    m = re.match(r"(func|struct|class|enum|typealias|protocol|actor)\s+(\w+)", s)
    return [m.group(2)] if m else []


def identifiers(code):
    # 코드 글자에서 앞이 '.' 가 아닌 이름 토큰
    return {m.group(1) for m in re.finditer(r"(?<![.\w])([A-Za-z_]\w*)", code)}


def split(text):
    if not text.endswith("\n") or "\r" in text:
        raise SplitError("입력은 LF 줄바꿈이고 줄바꿈으로 끝나야 한다")
    lines, code, top, joined = scan(text)
    n = len(lines)
    kinds = [classify(code[i]) if top[i] else None for i in range(n)]
    swallowed = [i + 1 for i in joined if classify(code[i]).startswith("decl:")]
    if swallowed:
        raise SplitError(f"앞 줄에 이어진다고 본 줄이 선언이다: {swallowed} — 훑기 규칙(CONTINUE_END·CONTINUE_START)을 고쳐야 한다")

    marks = [i for i in range(n) if top[i] and lines[i].startswith(MARK_PREFIX)]
    if not marks:
        raise SplitError("맨 위 `// MARK: - ` 절이 없다")
    header = lines[:marks[0]]
    if any(kinds[i] not in (None, "blank", "decl:import", "directive") for i in range(marks[0])):
        raise SplitError("첫 MARK 앞 머리에는 주석·import 만 둔다(모든 파일에 그대로 들어간다)")

    # 최상위 선언의 끝: 다음 최상위 시작 줄의 바로 앞
    def decl_end(i):
        j = i + 1
        while j < n and not top[j]:
            j += 1
        return j - 1

    type_decls = [i for i in range(marks[0], n) if kinds[i] and kinds[i].startswith("decl:")
                  and kinds[i][5:] in TYPE_KEYWORDS]
    if not type_decls:
        raise SplitError("최상위 형식 선언이 없다")
    tail = decl_end(type_decls[-1]) + 1
    while tail < n and not lines[tail].strip():
        tail += 1
    if tail >= n:
        raise SplitError("마지막 형식 선언 뒤에 시작 코드가 없다")
    stray = [i + 1 for i in range(marks[0], tail) if kinds[i] == "stmt"]
    if stray:
        raise SplitError(f"시작 코드(main.swift) 밖의 최상위 실행문: {stray} — main.swift 규칙을 넓혀야 한다")
    if not any(kinds[i] == "stmt" for i in range(tail, n)):
        raise SplitError("시작 코드에 실행문이 없다")
    if any(i >= tail for i in marks):
        raise SplitError("시작 코드 안에 MARK 절이 있다")

    # 절 → 파일. 시작 코드는 마지막 절 안에 있다(위에서 MARK 가 시작 코드 안에 없음을 봤다) — 그 절은 시작 코드 앞에서 끝난다
    sections = []
    used = {MAIN_FILE.lower()}
    for k, m in enumerate(marks):
        end = marks[k + 1] if k + 1 < len(marks) else tail
        title = lines[m][len(MARK_PREFIX):].strip()
        name = SECTION_FILES.get(title) or fallback_name(title, k + 1)
        base, d = name, 2
        while name.lower() in used:
            name, d = f"{base}-{d}", d + 1
        used.add(name.lower())
        sections.append({"title": title, "file": name, "start": m, "end": end})

    # 전역
    globals_found = []   # (줄, 이름들, main 인가)
    for i in range(marks[0], n):
        if kinds[i] in ("decl:let", "decl:var"):
            globals_found.append((i, decl_names(code[i]), i >= tail))
    eager = []
    unknown = []
    for i, names, in_main in globals_found:
        if in_main:
            continue
        for nm in names:
            if nm in EAGER:
                eager.append((i, nm))
        if not any(nm in EAGER or nm in LAZY for nm in names):
            unknown.append((i + 1, names))

    # private 떼기
    changed = {}   # 줄 번호 → 새 줄
    access_hits = []
    access_idle = []   # 맞았지만 다른 파일이 쓰지 않아 그대로 둔 것

    def file_of(i):
        if i >= tail:
            return MAIN_FILE
        return next(s["file"] for s in sections if s["start"] <= i < s["end"])
    for entry in ACCESS:
        tok = next((t for t in ACCESS_TOKENS if t in entry), None)
        if tok is None:
            raise SplitError(f"ACCESS 항목에 private/fileprivate 가 없다: {entry!r}")
        hits = [i for i in range(n) if lines[i].lstrip().startswith(entry)]
        if len(hits) > 1:
            raise SplitError(f"ACCESS 항목이 {len(hits)}번 맞는다(1번이어야 한다): {entry!r} → {[h + 1 for h in hits]}")
        for i in hits:
            names = declared_names(code[i])
            if not names:
                raise SplitError(f"ACCESS 항목의 선언 이름을 못 읽었다: {entry!r}")
            mine = file_of(i)
            top_level = lines[i] == lines[i].lstrip()
            used = sorted({file_of(j) for j in range(marks[0], n) if file_of(j) != mine
                           and any((nm in identifiers(code[j])) if top_level else re.search(rf"\b{nm}\b", code[j])
                                   for nm in names)})
            if not used:
                access_idle.append((i + 1, entry))
                continue
            ind = len(lines[i]) - len(lines[i].lstrip())
            pos = ind + entry.index(tok)
            changed[i] = lines[i][:pos] + lines[i][pos + len(tok):]
            access_hits.append((i + 1, entry))

    def body(a, b):
        out = [changed.get(i, lines[i]) for i in range(a, b)]
        while out and not out[-1].strip():
            out.pop()
        return out

    files = {}
    for s in sections:
        files[s["file"]] = header + body(s["start"], s["end"])
    touch = []
    if eager:   # EAGER 이름이 그 입력에 없으면(다른 가지) 0번 맞아도 된다
        touch = [TOUCH_COMMENT] + [f"_ = {nm}" for _, nm in eager] + [""]
    files[MAIN_FILE] = header + touch + body(tail, n)
    order = [s["file"] for s in sections] + [MAIN_FILE]
    rendered = {f + ".swift": "\n".join(files[f]) + "\n" for f in order}

    # main.swift 의 전역을 다른 파일이 쓰는 곳(그 파일의 코드는 시작 코드 뒤에 돌아야 한다)
    main_globals = [nm for i, names, in_main in globals_found if in_main for nm in names]
    main_refs = {}
    for i in range(marks[0], tail):
        ids = identifiers(code[i])
        for nm in main_globals:
            if nm in ids:
                main_refs.setdefault(nm, []).append(i + 1)
    body_code = "\n".join(code[marks[0]:tail])
    main_refs = {nm: (lns, bool(re.search(rf"\b(let|var|for|case)\s+{nm}\b|[{{,(]\s*{nm}\s*(in\b|,|:)", body_code)))
                 for nm, lns in main_refs.items()}

    info = {"sections": sections, "tail": tail + 1, "eager": [(i + 1, nm) for i, nm in eager], "unknown": unknown,
            "access": access_hits, "access_idle": access_idle,
            "unmatched_access": [e for e in ACCESS if not any(e == h[1] for h in access_hits + access_idle)],
            "main_refs": main_refs, "order": [f + ".swift" for f in order]}
    verify(text, rendered, header, touch, lines, changed, sections, tail)
    return rendered, info


def verify(text, rendered, header, touch, lines, changed, sections, tail):
    # 줄 보존: 출력만 가지고 입력을 다시 만든다 — 머리를 떼고, TOUCH 묶음을 떼고, ACCESS 바꿈과 끝 빈 줄을 되돌린다
    head = "\n".join(header) + "\n" if header else ""
    undo = {changed[i]: lines[i] for i in changed}   # 바뀐 줄 글자 → 원래 줄(같은 글자의 다른 줄이 있으면 아래 비교에서 실패한다)
    pieces = {}
    for fname, body_text in rendered.items():
        if not body_text.startswith(head):
            raise SplitError(f"{fname}: 머리가 입력의 머리와 다르다")
        rest = body_text[len(head):]
        if fname == MAIN_FILE + ".swift" and touch:
            t = "\n".join(touch) + "\n"
            if not rest.startswith(t):
                raise SplitError("main.swift: TOUCH 묶음이 어긋났다")
            rest = rest[len(t):]
        out = []
        for ln in rest.split("\n")[:-1]:
            if ln in undo:
                orig = undo[ln]
                tok = next(t for t in ACCESS_TOKENS if t in orig)
                if orig.replace(tok, "", 1) != ln:
                    raise SplitError(f"{fname}: ACCESS 바꿈이 private/fileprivate 떼기만이 아니다: {ln!r}")
                ln = orig
            out.append(ln)
        pieces[fname] = out
    rebuilt = list(header)
    segs = [(s["file"] + ".swift", s["start"], s["end"]) for s in sections] + [(MAIN_FILE + ".swift", tail, len(lines))]
    for f, a, b in segs:
        got = pieces[f]
        if len(got) > b - a:
            raise SplitError(f"{f}: 입력보다 줄이 많다")
        # 뗀 끝 빈 줄을 되돌린다 — 정말 빈 줄이었는지는 아래 비교가 본다
        rebuilt.extend(got + [""] * (b - a - len(got)))
    if len(rebuilt) != len(lines) or any(a != b for a, b in zip(rebuilt, lines)):
        first = next((k for k, (a, b) in enumerate(zip(rebuilt, lines)) if a != b), min(len(rebuilt), len(lines)))
        raise SplitError(f"줄 보존 확인 실패: 입력 {first + 1}행부터 어긋난다")
    if "\n".join(rebuilt) + "\n" != text:
        raise SplitError("줄 보존 확인 실패(바이트)")


def read_input(args):
    if args.from_commit:
        r = subprocess.run(["git", "-C", str(ROOT), "show", f"{args.from_commit}:{INPUT_NAME}"], capture_output=True)
        if r.returncode != 0:
            raise SplitError(f"git show {args.from_commit}:{INPUT_NAME} 실패: {r.stderr.decode(errors='replace').strip()}")
        return r.stdout.decode("utf-8"), f"{args.from_commit}:{INPUT_NAME}"
    path = Path(args.input) if args.input else ROOT / INPUT_NAME
    if not path.exists():
        raise SplitError(f"입력이 없다: {path}")
    return path.read_bytes().decode("utf-8"), str(path)


def report(info, rendered):
    print("파일:")
    for f in info["order"]:
        title = next((s["title"] for s in info["sections"] if s["file"] + ".swift" == f), "(시작 코드)")
        print(f"  {f:28} {rendered[f].count(chr(10)):6}줄  {title}")
    print(f"main.swift 시작 코드: 입력 {info['tail']}행부터 끝까지")
    print("시작할 때 바로 초기화(EAGER): " + (", ".join(f"{nm}({ln}행)" for ln, nm in info["eager"]) or "없음"))
    for ln, names in info["unknown"]:
        print(f"경고: 표(EAGER·LAZY)에 없는 전역 {', '.join(names)} ({ln}행) — 처음 쓸 때 초기화된다. 표에 넣어 정한다")
    print(f"private 뗌: {len(info['access'])}곳" + "".join(f"\n  {ln}행 {e}" for ln, e in info["access"]))
    for ln, e in info["access_idle"]:
        print(f"ACCESS 항목을 다른 파일이 쓰지 않아 그대로 둠: {ln}행 {e}")
    if info["unmatched_access"]:
        print("이 입력에 없는 ACCESS 항목: " + ", ".join(info["unmatched_access"]))
    for nm, (lns, local) in info["main_refs"].items():
        print(f"참고: main.swift 의 전역 {nm} 과 같은 이름이 다른 파일에 {len(lns)}번(입력 {lns[0]}행 등)"
              + (", 같은 이름의 지역 이름도 있다" if local else "")
              + " — 전역을 쓰는 곳은 한 파일일 때처럼 시작 코드가 그 선언을 지난 뒤에 돌아야 한다")


def main():
    ap = argparse.ArgumentParser(description="ClaudeBoard.swift 를 Sources/ClaudeBoard/ 로 나눈다")
    ap.add_argument("--check", action="store_true", help="쓰지 않고 출력 폴더와 같은지만 본다")
    ap.add_argument("--from", dest="from_commit", help="이 커밋의 ClaudeBoard.swift 를 입력으로")
    ap.add_argument("--input", help="입력 파일(기본: 저장소 맨 위 ClaudeBoard.swift)")
    ap.add_argument("--out", help="출력 폴더(기본: Sources/ClaudeBoard)")
    ap.add_argument("-q", "--quiet", action="store_true", help="요약을 찍지 않는다")
    args = ap.parse_args()
    out_dir = Path(args.out) if args.out else ROOT / OUT_REL
    try:
        text, src = read_input(args)
        rendered, info = split(text)
    except SplitError as e:
        print(f"split-board: {e}", file=sys.stderr)
        return 2
    existing = {p.name for p in out_dir.glob("*.swift")} if out_dir.is_dir() else set()
    if args.check:
        bad = []
        for f in sorted(set(rendered) | existing):
            if f not in rendered:
                bad.append(f"{f}: 나눈 결과에 없는 파일")
            elif f not in existing:
                bad.append(f"{f}: 출력 폴더에 없다")
            elif (out_dir / f).read_bytes() != rendered[f].encode("utf-8"):
                bad.append(f"{f}: 내용이 다르다")
        if bad:
            print(f"split-board: {src} 를 나눈 결과가 {out_dir} 와 다르다", file=sys.stderr)
            for b in bad:
                print("  " + b, file=sys.stderr)
            return 1
        if not args.quiet:
            print(f"split-board: {src} 를 나눈 결과가 {out_dir} 의 {len(rendered)}개 파일과 같다")
        return 0
    out_dir.mkdir(parents=True, exist_ok=True)
    for f in sorted(existing - set(rendered)):
        (out_dir / f).unlink()
        print(f"지움: {out_dir / f}")
    for f in sorted(rendered):
        (out_dir / f).write_bytes(rendered[f].encode("utf-8"))
    if not args.quiet:
        print(f"split-board: {src} → {out_dir} ({len(rendered)}개 파일)")
        report(info, rendered)
    return 0


if __name__ == "__main__":
    sys.exit(main())
