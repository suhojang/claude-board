// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/shutdownList.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import type { BoardSurface, BoardTab, Session } from "../types.ts";
import { shutdownList } from "./shutdownList.ts";

let serial = 0;
const session = (over: Partial<Session>): Session => ({
  id: `id${++serial}`,
  cwd: "C:\\work\\proj",
  pid: 100 + serial,
  state: "idle",
  since: 1,
  updated: 2,
  headless: false,
  title: "제목",
  topic: "",
  message: "",
  name: null,
  ctx: null,
  brain: null,
  subagents: [],
  shells: 0,
  paused: false,
  muted: false,
  limitAt: null,
  canDrive: true,
  tty: null,
  tabAncestor: null,
  mode: null,
  appName: null,
  ...over,
});
const surface = (termId: string | null, over: Partial<BoardSurface> = {}): BoardSurface => ({
  id: `f-${termId ?? ++serial}`,
  termId,
  sessionId: null,
  command: null,
  held: null,
  mdRenderOff: false,
  title: "",
  ...over,
});
const tabOf = (s: BoardSurface, over: Partial<BoardTab> = {}): BoardTab => ({
  id: `tab-${s.id}`,
  workspace: "C:\\work\\proj",
  customName: null,
  renaming: false,
  root: { leaf: s.id },
  focused: s.id,
  zoomed: false,
  ...over,
});
const sub = (state) => ({ id: "x", type: "t", desc: "", state, started: 1, ended: null, tool: "", background: false, brain: null });

const list = (tabs, sessions, surfaces, facts = {}) => shutdownList(tabs, sessions, surfaces, { now: 1_000_000, ...facts });

test("쉬는 주 세션뿐이면 묻지 않고 표지도 남기지 않는다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "idle" });
  assert.deepEqual(list([tabOf(s)], [main], [s]), { items: [], pause: [] });
});

test("탭이 없으면 빈 목록", () => {
  assert.deepEqual(list([], [], []), { items: [], pause: [] });
});

test("작업 중인 주 세션은 줄이 되고 표지도 남긴다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "working", title: "빌드" });
  const result = list([tabOf(s)], [main], [s]);
  assert.deepEqual(result.items, [{ sessionId: main.id, title: "proj · 빌드", kind: "working" }]);
  assert.deepEqual(result.pause, [main.id]);
});

test("승인 대기도 줄이 되고 표지를 남긴다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "waiting" });
  const result = list([tabOf(s)], [main], [s]);
  assert.equal(result.items[0].kind, "waiting");
  assert.deepEqual(result.pause, [main.id]);
});

test("쉬는 주 세션이라도 서브에이전트가 돌면 줄이 되고 표지를 남긴다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "idle", subagents: [sub("done"), sub("running")] });
  const result = list([tabOf(s)], [main], [s]);
  assert.equal(result.items[0].kind, "subagents");
  assert.deepEqual(result.pause, [main.id]);
});

test("끝난 서브에이전트만 있으면 줄이 아니다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "idle", subagents: [sub("done")] });
  assert.deepEqual(list([tabOf(s)], [main], [s]).items, []);
});

test("Bash 백그라운드 셸이 도는 쉬는 세션은 shell 줄이 되고 표지를 남긴다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "idle", shells: 2 });
  const result = list([tabOf(s)], [main], [s]);
  assert.equal(result.items[0].kind, "shell");
  assert.deepEqual(result.pause, [main.id]);
});

test("쉬는 주 세션 + 탭 안 claude -p: -p 만 줄이 되고 표지는 주 세션에만", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "idle" });
  const child = session({ headless: true, tabAncestor: "t1", mode: "-p", state: "working", title: "배치" });
  const result = list([tabOf(s)], [main, child], [s]);
  assert.deepEqual(result.items, [{ sessionId: child.id, title: "proj · 배치", kind: "headlessInTab" }]);
  assert.deepEqual(result.pause, [main.id]);
});

test("탭 안 claude -p 는 쉬는 중이어도 줄이 되고 표지는 남기지 않는다", () => {
  const s = surface("t1");
  const child = session({ headless: true, tabAncestor: "t1", mode: "-p", state: "idle" });
  const result = list([tabOf(s)], [child], [s]);
  assert.equal(result.items[0].kind, "headlessInTab");
  assert.deepEqual(result.pause, []);
});

test("셸 프롬프트에서 친 claude -p(tty 로 짝지음)는 줄이 되고 주 세션으로 치지 않는다", () => {
  const s = surface("t1");
  const typed = session({ tty: "t1", mode: "-p", state: "working" });
  const result = list([tabOf(s)], [typed], [s]);
  assert.equal(result.items[0].kind, "headlessInTab");
  assert.deepEqual(result.pause, []);
});

test("백그라운드 세션과 다른 터미널의 세션은 줄이 아니다", () => {
  const s = surface("t1");
  const bg = session({ headless: true, mode: "--bg", state: "working" });
  const other = session({ state: "working", appName: "Windows Terminal" });
  assert.deepEqual(list([tabOf(s)], [bg, other], [s]), { items: [], pause: [] });
});

test("끝난 세션은 줄이 아니다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "ended", subagents: [sub("running")] });
  assert.deepEqual(list([tabOf(s)], [main], [s]), { items: [], pause: [] });
});

test("claude 가 아닌 프로그램이 도는 셸은 줄이 되고 세션 id 는 없다", () => {
  const s = surface("t1");
  const t = tabOf(s, { customName: "서버" });
  const result = list([t], [], [s], { atPrompt: [], runsClaude: [] });
  assert.deepEqual(result.items, [{ sessionId: null, title: "서버", kind: "shell" }]);
  assert.deepEqual(result.pause, []);
});

test("프롬프트에 있는 셸은 줄이 아니다", () => {
  const s = surface("t1");
  assert.deepEqual(list([tabOf(s)], [], [s], { atPrompt: ["t1"], runsClaude: [] }).items, []);
});

test("claude 가 도는 터미널·방금 명령을 친 화면은 셸 줄이 아니다", () => {
  const a = surface("t1");
  const b = surface("t2", { typedAt: 999_000 });
  const result = list([tabOf(a), tabOf(b)], [], [a, b], { atPrompt: [], runsClaude: ["t1"] });
  assert.deepEqual(result.items, []);
});

test("셸 줄은 상태 묶음에서 atPrompt 를 못 받았으면 만들지 않는다", () => {
  const s = surface("t1");
  assert.deepEqual(list([tabOf(s)], [], [s]).items, []);
});

test("아직 셸을 띄우지 않은 화면은 줄이 아니다", () => {
  const s = surface(null);
  assert.deepEqual(list([tabOf(s)], [], [s], { atPrompt: [], runsClaude: [] }).items, []);
});

test("탭에 없는 화면(닫히는 중)은 보지 않는다", () => {
  const stray = surface("t9");
  const main = session({ tty: "t9", state: "working" });
  assert.deepEqual(list([], [main], [stray], { atPrompt: [] }), { items: [], pause: [] });
});

test("한 세션이 여러 까닭이면 줄은 하나, 위의 까닭이 이긴다", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "working", subagents: [sub("running")], shells: 1 });
  const result = list([tabOf(s)], [main], [s]);
  assert.equal(result.items.length, 1);
  assert.equal(result.items[0].kind, "working");
});

test("탭 둘: 일하는 탭의 주 세션에만 표지, 쉬는 탭은 그대로", () => {
  const a = surface("t1");
  const b = surface("t2");
  const busy = session({ tty: "t1", state: "working" });
  const idle = session({ tty: "t2", state: "idle" });
  const result = list([tabOf(a), tabOf(b)], [busy, idle], [a, b]);
  assert.deepEqual(result.items.map((i) => i.sessionId), [busy.id]);
  assert.deepEqual(result.pause, [busy.id]);
});

test("줄 제목은 폴더 이름 · 세션 제목", () => {
  const s = surface("t1");
  const main = session({ tty: "t1", state: "working", cwd: "C:\\a\\b\\폴더", title: "작업" });
  assert.equal(list([tabOf(s)], [main], [s]).items[0].title, "폴더 · 작업");
});
