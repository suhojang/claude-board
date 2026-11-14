// @ts-nocheck
// closeTab 의 시험 — 탭 화면 값 만들기(잎 하나·나눔·셸 안 뜬 화면 건너뛰기·--name 값·막 뜸)와 지문(panesKey)을 본다.
// 웹뷰 꼴 PaneView 는 백엔드 open_tab/close_handle.rs PaneView 의 필드와 같아야 한다.
import { test } from "node:test";
import assert from "node:assert/strict";

import { panesKey, paneViews, type PaneView } from "./closeTab.ts";
import { leaves, type SplitNode } from "./split.ts";
import type { BoardSurface, BoardTab, LeafPhase } from "../types.ts";
import { TYPED_WINDOW_MS } from "./place.ts";

/** 화면 하나 — paneViews 가 보는 값(id·termId·sessionId·command·typedAt)만 고치고 나머지는 기본값 */
const surface = (id: string, termId: string | null, over: Partial<BoardSurface> = {}): BoardSurface => ({
  id,
  termId,
  sessionId: null,
  command: null,
  held: null,
  mdRenderOff: false,
  title: "",
  ...over,
});

/** 탭 하나 — 잎 나누기 트리와 바깥 열기 표지만 받는다. 고른 화면은 첫 잎 */
const tab = (id: string, root: SplitNode, over: Partial<BoardTab> = {}): BoardTab => ({
  id,
  workspace: "workspace-" + id,
  customName: null,
  renaming: false,
  root,
  focused: leaves(root)[0],
  zoomed: false,
  ...over,
});

/** paneViews 의 tabName 인자 — 보이는 탭 이름(shownName 의 단순 판) */
const tabNameOf = (t: BoardTab) => t.customName ?? t.workspace;

/** 잎 id → 화면 지도로 surfaceOf 를 만든다. 지도에 없는 잎은 undefined */
const surfaces = (byLeaf: Record<string, BoardSurface>) => (leafId: string) => byLeaf[leafId];

/** 잎 id → 칸 상태 지도로 phaseOf 를 만든다. 지도에 없는 잎은 undefined */
const phases = (byLeaf: Record<string, LeafPhase>) => (leafId: string) => byLeaf[leafId];

/** 잎 둘 나눈 트리 — split 시험에서 곧장 쓴다 */
const splitTwo: SplitNode = { split: "right", ratio: 0.5, first: { leaf: "f1" }, second: { leaf: "f2" } };

// ── 잎 하나 탭 ────────────────────────────────────────────────

test("잎 하나 탭 — id 는 백엔드 터미널 id, 탭 이름을 names 에 싣는다", () => {
  const panes = paneViews(
    [tab("w-1", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1") }),
    phases({ f1: "running" }),
    tabNameOf,
    0,
  );
  assert.deepEqual(panes, [
    {
      id: "t1",
      names: ["workspace-w-1"],
      sessionId: null,
      tab: "workspace-w-1",
      external: false,
      takeover: false,
      split: false,
      starting: false,
    },
  ]);
});

test("잎 하나 탭 — 바깥 열기(--open-tab) 탭은 external 이고 takeover 은 늘 false다", () => {
  const pane = paneViews(
    [tab("w-e", { leaf: "f1" }, { external: true })],
    surfaces({ f1: surface("f1", "t7") }),
    phases({}),
    tabNameOf,
    0,
  )[0];
  assert.equal(pane.external, true);
  assert.equal(pane.takeover, false);
});

test("잎 하나 탭 — 화면이 기억하는 세션 id 를 sessionId 로 싣는다", () => {
  const pane = paneViews(
    [tab("w-s", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1", { sessionId: "uuid-1" }) }),
    phases({}),
    tabNameOf,
    0,
  )[0];
  assert.equal(pane.sessionId, "uuid-1");
});

// ── 나눈 탭(split) ────────────────────────────────────────────

test("나눈 탭 — 두 화면이 트리 순서로 나오고 split 이며 탭 이름은 names 에 넣지 않는다", () => {
  const tabSplit = tab("w-2", { split: "right", ratio: 0.5, first: { leaf: "f1" }, second: { leaf: "f2" } });
  const panes = paneViews(
    [tabSplit],
    surfaces({ f1: surface("f1", "t1"), f2: surface("f2", "t2") }),
    phases({ f1: "running", f2: "running" }),
    tabNameOf,
    0,
  );
  assert.deepEqual(
    panes.map((p) => [p.id, p.split, p.names]),
    [
      ["t1", true, []],
      ["t2", true, []],
    ],
  );
  assert.equal(panes.every((p) => p.tab === "workspace-w-2"), true);
});

// ── 셸이 안 뜬 화면(termId 없음) ─────────────────────────────

test("셸이 안 뜬 화면(termId 없음) — 목록에서 뺀다(잎 하나 탭)", () => {
  const panes = paneViews(
    [tab("w-3", { leaf: "f1" })],
    surfaces({ f1: surface("f1", null) }),
    phases({ f1: "idle" }),
    tabNameOf,
    0,
  );
  assert.deepEqual(panes, []);
});

test("나눈 탭의 잎 하나가 셸 안 뜸 — 남은 화면 하나만 나오고 split 은 탭 트리 기준", () => {
  const panes = paneViews(
    [tab("w-4", { split: "down", ratio: 0.5, first: { leaf: "f1" }, second: { leaf: "f2" } })],
    surfaces({ f1: surface("f1", "t1"), f2: surface("f2", null) }),
    phases({ f1: "running", f2: "idle" }),
    tabNameOf,
    0,
  );
  assert.equal(panes.length, 1);
  assert.equal(panes[0].id, "t1");
  assert.equal(panes[0].split, true);
});

test("탭이 없으면 — 화면 값도 없다", () => {
  assert.deepEqual(paneViews([], surfaces({}), phases({}), tabNameOf, 0), []);
});

// ── --name / -n 값 ────────────────────────────────────────────

test("--name — 명령의 --name 값을 벗겨 names 에 더한다(잎 하나 탭은 탭 이름 뒤에)", () => {
  const pane = paneViews(
    [tab("w-5", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1", { command: 'claude --name "my session" --dangerously-skip-permissions' }) }),
    phases({}),
    tabNameOf,
    0,
  )[0];
  assert.deepEqual(pane.names, ["workspace-w-5", "my session"]);
});

test("-n — 짧은 꼴도 읽는다(나눈 탭은 탭 이름 없이 값만)", () => {
  const pane = paneViews(
    [tab("w-6", { split: "right", ratio: 0.5, first: { leaf: "f1" }, second: { leaf: "f2" } })],
    surfaces({ f1: surface("f1", "t1", { command: "claude -n 짧은이름" }), f2: surface("f2", "t2") }),
    phases({}),
    tabNameOf,
    0,
  )[0];
  assert.deepEqual(pane.names, ["짧은이름"]);
  assert.equal(pane.split, true);
});

test("--name 이 없거나 값이 없으면 — 이름 없이 끝낸다", () => {
  const panes = paneViews(
    [
      tab("w-7", splitTwo),
    ],
    surfaces({ f1: surface("f1", "t1", { command: "claude --resume x" }), f2: surface("f2", "t2", { command: "claude --name" }) }),
    phases({}),
    tabNameOf,
    0,
  );
  assert.deepEqual(panes.map((p) => p.names), [[], []]);
});

test("명령을 아직 안 친 화면(command 없음) — 잎 하나 탭은 탭 이름만 names 에 있다", () => {
  const pane = paneViews(
    [tab("w-8", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1") }),
    phases({}),
    tabNameOf,
    0,
  )[0];
  assert.deepEqual(pane.names, ["workspace-w-8"]);
});

// ── 막 뜸(starting) ───────────────────────────────────────────

test("막 뜸 — 칸 상태가 starting 이면 막 뜬다", () => {
  const pane = paneViews(
    [tab("w-9", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1") }),
    phases({ f1: "starting" }),
    tabNameOf,
    0,
  )[0];
  assert.equal(pane.starting, true);
});

test("막 뜸 — claude 명령을 친 지 창 안이면 막 뜨는 것으로 본다(칸 상태는 running)", () => {
  const typedAt = 1_000;
  const pane = paneViews(
    [tab("w-10", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1", { command: "claude", typedAt }) }),
    phases({ f1: "running" }),
    tabNameOf,
    typedAt + TYPED_WINDOW_MS - 1,
  )[0];
  assert.equal(pane.starting, true);
});

test("막 뜸 — 창이 다 지나면 아니다", () => {
  const typedAt = 1_000;
  const pane = paneViews(
    [tab("w-11", { leaf: "f1" })],
    surfaces({ f1: surface("f1", "t1", { command: "claude", typedAt }) }),
    phases({ f1: "running" }),
    tabNameOf,
    typedAt + TYPED_WINDOW_MS,
  )[0];
  assert.equal(pane.starting, false);
});

test("막 뜸 — running 이고 언제 쳤는지 모르면(typedAt 없음) 아니다", () => {
  for (const typedAt of [null, undefined]) {
    const pane = paneViews(
      [tab("w-12", { leaf: "f1" })],
      surfaces({ f1: surface("f1", "t1", { typedAt }) }),
      phases({ f1: "running" }),
      tabNameOf,
      1_000,
    )[0];
    assert.equal(pane.starting, false);
  }
});

// ── 지문(panesKey) ────────────────────────────────────────────

/** 지문 견주기용 화면 값 하나 */
const aPane: PaneView = { id: "t1", names: ["x"], sessionId: null, tab: "x", external: false, takeover: false, split: false, starting: false };

test("지문 — 내용이 같으면 같은 문자열이다", () => {
  const same: PaneView[] = [{ ...aPane }];
  assert.equal(panesKey([aPane]), panesKey(same));
});

test("지문 — 값이 바뀌면 다른 문자열이다", () => {
  const changed: PaneView[] = [{ ...aPane, starting: true }];
  assert.notEqual(panesKey([aPane]), panesKey(changed));
});

test("지문 — 차례가 바뀌면 다른 문자열이다", () => {
  const another: PaneView = { ...aPane, id: "t2" };
  assert.notEqual(panesKey([aPane, another]), panesKey([another, aPane]));
});

