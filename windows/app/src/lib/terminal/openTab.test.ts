// @ts-nocheck
// openTab 의 시험 — 요청 읽기·세션 id 읽기(plan.rs open_tab_prepare 과 같은 차례)·셸 한 줄 묶기(큰따옴표 규칙)·이어 열 화면 찾기를 본다
import { test } from "node:test";
import assert from "node:assert/strict";

import { findReuseSurface, openTabLine, openTabSession, parseOpenTabEvent } from "./openTab.ts";

const SID = "0f0e1d2c-3b4a-5968-7766-554433221100";
const NEW = "1234abcd-9876-5432-1100-ffeeddccbbaa";

// ── 요청 읽기 ─────────────────────────────────────────────────

test("요청 읽기 — snake_case 이벤트 꼴을 그대로 읽는다", () => {
  const request = parseOpenTabEvent({
    id: "req-1",
    cwd: "C:\\work\\claude-board-autopilot",
    title: "역할",
    command: ["claude", "--session-id", NEW],
    no_focus: false,
    reuse_tab: null,
  });
  assert.deepEqual(request, {
    id: "req-1",
    cwd: "C:\\work\\claude-board-autopilot",
    title: "역할",
    command: ["claude", "--session-id", NEW],
    noFocus: false,
    reuseTab: null,
  });
});

test("요청 읽기 — camelCase 꼴도 읽는다(wiring 이 어느 쪽으로 넘길지 아직 고정되지 않았다)", () => {
  const request = parseOpenTabEvent({ id: "req-2", cwd: "D:\\x", title: "t", command: ["claude"], noFocus: true, reuseTab: "s-1" });
  assert.equal(request?.noFocus, true);
  assert.equal(request?.reuseTab, "s-1");
});

test("요청 읽기 — Focus 가 보내는 최소 꼴(command 없음)도 읽는다", () => {
  const request = parseOpenTabEvent({ id: "req-3", cwd: "D:\\x", reuse_tab: "s-1", no_focus: false });
  assert.deepEqual(request, { id: "req-3", cwd: "D:\\x", title: null, command: [], noFocus: false, reuseTab: "s-1" });
});

test("요청 읽기 — 꼴이 맞지 않으면 없다", () => {
  assert.equal(parseOpenTabEvent(null), null);
  assert.equal(parseOpenTabEvent("no"), null);
  assert.equal(parseOpenTabEvent({}), null); // id 도 cwd 도 없다
  assert.equal(parseOpenTabEvent({ id: "req", title: "t" }), null); // cwd 가 없다
  assert.equal(parseOpenTabEvent({ id: "", cwd: "D:\\x", command: [] }), null); // 빈 id
  assert.equal(parseOpenTabEvent({ id: "req", cwd: "D:\\x", command: "claude" }), null); // command 가 배열이 아니다
  assert.equal(parseOpenTabEvent({ id: "req", cwd: "D:\\x", command: [42] }), null); // 인자가 문자열이 아니다
  assert.equal(parseOpenTabEvent({ id: "req", cwd: "D:\\x", command: [""] }), null); // 빈 인자
});

// ── 이어 열 화면 찾기(reuseTab — 이제 백엔드 터미널 id "t1" 꼴이고 화면 id 옛 꼴도 읽는다) ──

/** 이어 열 후보 화면 두 필드만 — findReuseSurface 가 보는 꼴(실제 꼴은 이보다 넓다) */
const screen = (id: string, termId: string | null) => ({ id, termId });

test("이어 열 화면 — 화면 id 로 찾는다(옛 꼴)", () => {
  const first = screen("f-1", "t1");
  const second = screen("f-2", "t2");
  assert.equal(findReuseSurface([first, second], "f-2"), second);
});

test("이어 열 화면 — 백엔드 터미널 id 로도 찾는다(이제 backend 가 보내는 꼴)", () => {
  const first = screen("f-1", "t1");
  const second = screen("f-2", "t2");
  assert.equal(findReuseSurface([first, second], "t2"), second);
});

test("이어 열 화면 — 화면 id 가 이긴다(다른 화면의 termId 와 겹쳐도, 차례와 상관없이)", () => {
  const byId = screen("t1", "t2"); // 화면 id 가 다른 화면의 터미널 id 와 우연히 같다
  const byTerm = screen("f-2", "t1");
  assert.equal(findReuseSurface([byTerm, byId], "t1"), byId); // termId 를 먼저 보면 byTerm 이 나온다
  const alone = findReuseSurface([byId, byTerm], "t1");
  assert.equal(alone, byId); // 화면 id 가 앞서도 같다
});

test("이어 열 화면 — 없는 id 면 없다(그 사이 닫힌 화면)", () => {
  const only = screen("f-1", "t1");
  assert.equal(findReuseSurface([only], "f-9"), undefined); // 화면 id 로 없다
  assert.equal(findReuseSurface([only], "t9"), undefined); // 터미널 id 로도 없다
  assert.equal(findReuseSurface([], "t1"), undefined); // 화면이 아예 없다
});

test("이어 열 화면 — 셸이 아직 안 뜬 화면(termId null)은 화면 id 로만 찾는다", () => {
  const idle = screen("f-1", null);
  assert.equal(findReuseSurface([idle], "f-1"), idle); // 화면 id 로는 찾는다
  assert.equal(findReuseSurface([idle], "null"), undefined); // 터미널 id 는 없어서 눌리지 않는다
});

// ── 세션 id 읽기(plan.rs open_tab_prepare 과 같은 차례) ──────

test("세션 id — 새 세션(plan 이 끼운 --session-id)을 읽고 소문자로 맞춘다", () => {
  assert.equal(openTabSession(["claude", "--session-id", NEW.toUpperCase()]), NEW);
  assert.equal(openTabSession(["claude", `--session-id=${NEW}`]), NEW);
});

test("세션 id — 이어 열기(--resume)를 읽는다, 짧은 꼴 -r 도", () => {
  assert.equal(openTabSession(["claude", "--resume", SID]), SID);
  assert.equal(openTabSession(["claude", "-r", SID]), SID);
  assert.equal(openTabSession(["claude", `--resume=${SID}`]), SID);
});

test("세션 id — --resume 이 있으면(모양이 맞지 않아도) --session-id 는 보지 않는다(plan 의 else-if)", () => {
  assert.equal(openTabSession(["claude", "--resume", "역할", "--session-id", NEW]), null);
});

test("세션 id — fork 는 이어 열지 않고, 새 세션이어도 plan 이 뭘 줬는지를 따른다", () => {
  assert.equal(openTabSession(["claude", "--resume", SID, "--fork-session"]), null);
  assert.equal(openTabSession(["claude", "---session-id", NEW]), null); // 오타 꼴은 읽지 않는다
});

test("세션 id — 값 인자가 - 로 시작하면 값이 아니다(plan.rs next_value)", () => {
  assert.equal(openTabSession(["claude", "--session-id", "--name", "x"]), null);
  assert.equal(openTabSession(["claude", "--resume"]), null); // 고르기(picker)
});

test("세션 id — 세션 인자가 아예 없으면 없다(요청이 아닌 낱말들)", () => {
  assert.equal(openTabSession(["claude", "--continue"]), null);
  assert.equal(openTabSession(["claude", "--dangerously-skip-permissions"]), null);
});

// ── 셸 한 줄 묶기 ────────────────────────────────────────────

test("셸 줄 — 낱말은 공백으로 잇고 안전한 것은 그대로 둔다", () => {
  assert.equal(openTabLine(["claude", "--version"]), "claude --version");
  assert.equal(openTabLine(["claude", "--session-id=abc"]), "claude --session-id=abc");
  assert.equal(openTabLine(["claude", "--add-dir", "C:\\a\\b"]), "claude --add-dir C:\\a\\b"); // Windows 경로는 그대로
});

test("셸 줄 — 공백·유니코드가 든 낱말은 큰따옴표로 묶는다(quoteSessionName 과 같은 규칙)", () => {
  assert.equal(openTabLine(["claude", "-p", "hello world"]), "claude -p \"hello world\"");
  assert.equal(openTabLine(["claude", "--name", "역할-테스트"]), 'claude --name "역할-테스트"');
});

test("셸 줄 — 묶을 수 없는 낱말은 없다로 거절한다", () => {
  assert.equal(openTabLine([]), null); // 빈 argv
  assert.equal(openTabLine(["claude", ""]), null); // 빈 낱말
  assert.equal(openTabLine(["claude", 'a"b']), null); // 큰따옴표
  assert.equal(openTabLine(["claude", "a`b"]), null); // 역따옴표
  assert.equal(openTabLine(["claude", "$x"]), null); // 환경 변수
  assert.equal(openTabLine(["claude", "C:\\a\\"]), null); // 끝이 \ — git-bash 가 묶음을 깬다
  assert.equal(openTabLine(["claude", "a\nb"]), null); // 개행 — 줄이 일찍 끝난다
  assert.equal(openTabLine(["claude", "a\tb"]), null); // 탭 — 탭 완성이 눌린다
  assert.equal(openTabLine(["claude", "a\rb"]), null); // CR 도 같은 제어
});
