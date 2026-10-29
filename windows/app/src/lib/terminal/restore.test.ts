// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/restore.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import type { BoardSurface, BoardTab } from "../types.ts";
import { heldLaunch, parseEntries, restorePlan, savedSessionIds, savedTabs, waitOldPids } from "./restore.ts";

// 폴더가 다 있다고 답하는 집합 — 폴더 존재를 보지 않는 시험에 쓴다
const ALL = { has: () => true };
const planOf = (entries, alive = new Set(), dirs = ALL) => restorePlan(entries, alive, dirs);
const target = (pid, ctime = null) => ({ pid, ctime });
const launchOf = (held, context) => heldLaunch(held, { workspace: "C:\\work\\a", existingDirs: ALL, ...context });

let serial = 0;
const surface = (over: Partial<BoardSurface> = {}): BoardSurface => ({
  id: `s${++serial}`,
  termId: null,
  sessionId: null,
  command: null,
  held: null,
  mdRenderOff: false,
  title: "",
  ...over,
});
const tab = (root, over: Partial<BoardTab> = {}): BoardTab => ({
  id: `t${++serial}`,
  workspace: "C:\\work\\a",
  customName: null,
  renaming: false,
  root,
  focused: "",
  zoomed: false,
  ...over,
});
const leafTab = (s: BoardSurface, over = {}) => tab({ leaf: s.id }, { focused: s.id, ...over });
const view = (surfaces, terms = {}, selected = "") => ({ surfaces, terms, selected });
const term = (id, cwd) => ({ id, cwd, pid: 1, shell: "pwsh", cols: 80, rows: 24, status: "running", exitCode: null, windowsBuild: null, oldestSeq: 0, nextSeq: 0 });

const SID = "11111111-2222-3333-4444-555555555555";
const shellLeaf = (cwd = "C:\\work\\a") => ({ leaf: { cwd, command: null, sessionId: null, mdRenderOff: false } });
const claudeLeaf = (over = {}) => ({ leaf: { cwd: "C:\\work\\a", command: "claude", sessionId: SID, mdRenderOff: false, ...over } });

// ── savedTabs ──────────────────────────────────────────────────

test("저장: 셸만 있는 탭은 터미널이 알려 준 경로(cwd)를 남긴다", () => {
  const s = surface({ termId: "t1" });
  const t = leafTab(s);
  const [entry] = savedTabs([t], view([s], { t1: term("t1", "C:\\work\\a\\sub") }, t.id));
  assert.deepEqual(entry.root, shellLeaf("C:\\work\\a\\sub"));
  assert.equal(entry.selected, true);
});

test("저장: 터미널 정보가 없으면 탭 작업 경로가 cwd 다", () => {
  const s = surface();
  const [entry] = savedTabs([leafTab(s, { workspace: "~" })], view([s]));
  assert.deepEqual(entry.root, shellLeaf("~"));
  assert.equal(entry.selected, false);
});

test("저장: claude 화면은 명령·세션 id·꾸미기 끔까지 남긴다", () => {
  const s = surface({ termId: "t1", command: "claude", sessionId: SID, mdRenderOff: true });
  const [entry] = savedTabs([leafTab(s)], view([s], { t1: term("t1", "C:\\work\\a") }));
  assert.deepEqual(entry.root, claudeLeaf({ mdRenderOff: true }));
});

test("저장: 명령이 끝난 화면(command null)은 세션 id 가 남아 있어도 셸로 저장한다", () => {
  const s = surface({ termId: "t1", command: null, sessionId: SID, mdRenderOff: false });
  const [entry] = savedTabs([leafTab(s)], view([s], { t1: term("t1", "C:\\work\\a") }));
  assert.deepEqual(entry.root, shellLeaf());
});

test("저장: 아직 이어 열지 않은 화면은 남겨 둔 값 그대로", () => {
  const s = surface({ held: { cwd: "C:\\work\\b", command: "claude", session: SID, mdoff: "1" }, sessionId: SID });
  const [entry] = savedTabs([leafTab(s)], view([s]));
  assert.deepEqual(entry.root, claudeLeaf({ cwd: "C:\\work\\b", mdRenderOff: true }));
});

test("저장: 탭 이름·작업 경로·고른 탭", () => {
  const a = surface();
  const b = surface();
  const ta = leafTab(a, { customName: "하나", workspace: "C:\\x" });
  const tb = leafTab(b, { workspace: "C:\\y" });
  const entries = savedTabs([ta, tb], view([a, b], {}, tb.id));
  assert.deepEqual(entries.map((e) => [e.workspace, e.name, e.selected]), [["C:\\x", "하나", false], ["C:\\y", null, true]]);
});

test("저장: 나눈 트리는 그대로 저장한다(왕복)", () => {
  const a = surface({ command: "claude", sessionId: SID });
  const b = surface();
  const t = tab({ split: "right", ratio: 0.3, first: { leaf: a.id }, second: { leaf: b.id } }, { focused: a.id });
  const [entry] = savedTabs([t], view([a, b]));
  assert.equal(entry.root.split, "right");
  assert.equal(entry.root.ratio, 0.3);
  const plan = planOf([entry]);
  assert.equal(plan.surfaces.length, 2);
  const root = plan.tabs[0].root;
  assert.equal(root.split, "right");
  assert.equal(root.ratio, 0.3);
  assert.deepEqual(savedTabs(plan.tabs, view(plan.surfaces, {}, plan.selectedTab)).map((e) => e.root.first.leaf.command), ["claude"]);
});

// ── parseEntries ───────────────────────────────────────────────

test("읽기: 깨진 항목은 건너뛰고 맞는 것만 남긴다", () => {
  const good = { workspace: "C:\\a", name: "n", selected: true, root: claudeLeaf() };
  const entries = parseEntries([good, null, 3, { workspace: 1, root: shellLeaf() }, { workspace: "C:\\b" }, { workspace: "C:\\c", root: { leaf: { command: "x" } } }]);
  assert.equal(entries.length, 1);
  assert.deepEqual(entries[0], good);
});

test("읽기: 배열이 아니면 빈 목록", () => {
  assert.deepEqual(parseEntries(undefined), []);
  assert.deepEqual(parseEntries({ workspace: "x" }), []);
});

test("읽기: 몫은 0.1~0.9 로 맞추고 이상한 값은 0.5", () => {
  const split = (ratio) => parseEntries([{ workspace: "w", root: { split: "down", ratio, first: shellLeaf(), second: shellLeaf() } }])[0].root.ratio;
  assert.equal(split(0.01), 0.1);
  assert.equal(split(5), 0.9);
  assert.equal(split("x"), 0.5);
});

test("읽기: 셸 화면의 세션 id·꾸미기 끔은 버린다", () => {
  const [entry] = parseEntries([{ workspace: "w", root: { leaf: { cwd: "c", command: null, sessionId: SID, mdRenderOff: true } } }]);
  assert.deepEqual(entry.root, shellLeaf("c"));
});

// ── restorePlan ────────────────────────────────────────────────

test("되살리기: 모든 화면은 셸로 돌아오고 claude 화면은 held 만 쥔다", () => {
  const plan = planOf(parseEntries([{ workspace: "C:\\a", name: null, selected: true, root: claudeLeaf({ mdRenderOff: true }) }]));
  const [s] = plan.surfaces;
  assert.equal(s.command, null);
  assert.equal(s.termId, null);
  assert.deepEqual(s.held, { cwd: "C:\\work\\a", command: "claude", session: SID, mdoff: "1" });
  assert.equal(s.sessionId, SID);
  assert.equal(s.mdRenderOff, true);
});

test("되살리기: 셸 탭은 held 없이 작업 경로를 지킨다", () => {
  const plan = planOf([{ workspace: "C:\\proj", name: "내 탭", selected: true, root: shellLeaf("C:\\proj") }]);
  assert.equal(plan.tabs[0].workspace, "C:\\proj");
  assert.equal(plan.tabs[0].customName, "내 탭");
  assert.equal(plan.surfaces[0].held, null);
  assert.equal(plan.resumeNow, null);
});

test("되살리기: 고른 탭의 화면만 바로 이어 열고 나머지는 held 로 기다린다", () => {
  const entries = [
    { workspace: "C:\\a", name: null, selected: false, root: claudeLeaf({ sessionId: "aaaaaaaa-1" }) },
    { workspace: "C:\\b", name: null, selected: true, root: claudeLeaf({ sessionId: "bbbbbbbb-2" }) },
    { workspace: "C:\\c", name: null, selected: false, root: claudeLeaf({ sessionId: "cccccccc-3" }) },
  ];
  const plan = planOf(entries);
  assert.equal(plan.tabs.length, 3);
  assert.equal(plan.selectedTab, plan.tabs[1].id);
  assert.equal(plan.resumeNow, plan.tabs[1].focused);
  const heldCount = plan.surfaces.filter((s) => s.held).length;
  assert.equal(heldCount, 3); // 호출한 쪽이 resumeNow 의 held 를 비워 바로 이어 연다
});

test("되살리기: 표시가 없으면 마지막 탭을 고른다", () => {
  const plan = planOf([
    { workspace: "C:\\a", name: null, selected: false, root: shellLeaf() },
    { workspace: "C:\\b", name: null, selected: false, root: shellLeaf() },
  ]);
  assert.equal(plan.selectedTab, plan.tabs[1].id);
});

test("되살리기: 탭이 없으면 고를 것도 없다", () => {
  assert.deepEqual(planOf([]), { tabs: [], surfaces: [], selectedTab: null, resumeNow: null, missingDirs: [] });
});

test("되살리기: 보드 밖에서 아직 도는 세션의 화면은 버리고 탭에 화면이 없으면 탭도 버린다", () => {
  const entries = [{ workspace: "C:\\a", name: null, selected: true, root: claudeLeaf() }];
  const plan = planOf(entries, new Set([SID]));
  assert.deepEqual(plan.tabs, []);
  assert.deepEqual(plan.surfaces, []);
  assert.equal(plan.selectedTab, null);
});

test("되살리기: 나눈 탭에서 한 화면을 버리면 형제가 그 자리를 차지한다", () => {
  const entries = [{
    workspace: "C:\\a", name: null, selected: true,
    root: { split: "right", ratio: 0.5, first: claudeLeaf({ sessionId: "aaaaaaaa-1" }), second: shellLeaf() },
  }];
  const plan = planOf(entries, new Set(["aaaaaaaa-1"]));
  assert.equal(plan.surfaces.length, 1);
  assert.ok("leaf" in plan.tabs[0].root);
  assert.equal(plan.tabs[0].focused, plan.surfaces[0].id);
});

test("되살리기: 셸 화면은 세션 id 가 살아 있어도 버리지 않는다", () => {
  const plan = planOf([{ workspace: "w", name: null, selected: true, root: shellLeaf() }], new Set([SID]));
  assert.equal(plan.tabs.length, 1);
});

test("되살리기: 화면·탭 id 는 서로 다르다", () => {
  const plan = planOf([
    { workspace: "a", name: null, selected: false, root: shellLeaf() },
    { workspace: "a", name: null, selected: true, root: shellLeaf() },
  ]);
  assert.equal(new Set([...plan.tabs.map((t) => t.id), ...plan.surfaces.map((s) => s.id)]).size, 4);
});

test("savedSessionIds: claude 화면의 세션 id 만 중복 없이", () => {
  const entries = [
    { workspace: "a", name: null, selected: false, root: claudeLeaf() },
    { workspace: "b", name: null, selected: false, root: { split: "down", ratio: 0.5, first: claudeLeaf(), second: shellLeaf() } },
    { workspace: "c", name: null, selected: false, root: claudeLeaf({ sessionId: null }) },
  ];
  assert.deepEqual(savedSessionIds(entries), [SID]);
});

// ── waitOldPids ────────────────────────────────────────────────

/** 가짜 시계 — sleep 이 시간을 그만큼 앞으로 보낸다 */
function fakeClock() {
  let t = 0;
  const sleeps: number[] = [];
  return { clock: { now: () => t, sleep: async (ms) => { sleeps.push(ms); t += ms; } }, sleeps };
}

test("옛 pid 기다리기: 비었으면 묻지 않는다", async () => {
  const { clock } = fakeClock();
  let asked = 0;
  const left = await waitOldPids([], async () => { asked++; return []; }, clock);
  assert.equal(left.size, 0);
  assert.equal(asked, 0);
});

test("옛 pid 기다리기: 바로 끝나 있으면 기다리지 않는다", async () => {
  const { clock, sleeps } = fakeClock();
  const left = await waitOldPids([target(10), target(20)], async () => [], clock);
  assert.equal(left.size, 0);
  assert.deepEqual(sleeps, []);
});

test("옛 pid 기다리기: 도중에 끝나면 거기서 멈춘다", async () => {
  const { clock, sleeps } = fakeClock();
  let calls = 0;
  const left = await waitOldPids([target(10)], async (list) => (++calls < 3 ? list.map((t) => t.pid) : []), clock);
  assert.equal(left.size, 0);
  assert.deepEqual(sleeps, [200, 200]);
});

test("옛 pid 기다리기: 5초 뒤에도 살아 있으면 그 pid 를 돌려준다", async () => {
  const { clock, sleeps } = fakeClock();
  const left = await waitOldPids([target(10), target(20)], async (list) => list.map((t) => t.pid).filter((pid) => pid === 10), clock);
  assert.deepEqual([...left], [10]);
  assert.equal(sleeps.reduce((a, b) => a + b, 0), 5000);
});

test("옛 pid 기다리기: pid 0 은 묻지 않는다", async () => {
  const { clock } = fakeClock();
  const asked: number[][] = [];
  await waitOldPids([target(0), target(7)], async (list) => { asked.push(list.map((t) => t.pid)); return []; }, clock);
  assert.deepEqual(asked, [[7]]);
});

// ── heldLaunch ─────────────────────────────────────────────────

const held = (over = {}) => ({ cwd: "C:\\work\\a", command: "claude", session: SID, ...over });

test("이어 열기: 끝난 세션은 --resume 으로, 탭 이름을 --name 으로 붙인다", () => {
  const launch = launchOf(held(), { sessions: [{ id: SID, state: "ended" }], tabName: "내 탭" });
  assert.deepEqual(launch, { command: `claude --name 내 탭 --resume ${SID}`.replace("내 탭", '"내 탭"'), sessionId: SID });
});

test("이어 열기: 세션이 아직 끝나지 않았으면 보드 밖에서 도는 것이라 이어 열지 않는다", () => {
  for (const state of ["idle", "working", "waiting"]) {
    assert.equal(launchOf(held(), { sessions: [{ id: SID, state }], tabName: "a" }), null);
  }
});

test("이어 열기: 세션의 이어 열기 인자가 있으면 --resume <id> 뒤에 붙인다", () => {
  const launch = launchOf(held(), { sessions: [{ id: SID, state: "ended", resumeArgs: ["--dangerously-skip-permissions", "--resume", SID] }], tabName: "a" });
  assert.equal(launch.command, `claude --name a --resume ${SID} --dangerously-skip-permissions`);
});

test("이어 열기: 보드 상태에 없는 세션(대화 없이 끝남)은 처음 명령을 새 세션 id 로", () => {
  const launch = launchOf(held(), { sessions: [], tabName: "a" });
  assert.match(launch.command, /^claude --name a --session-id [0-9a-f-]{36}$/);
  assert.equal(launch.sessionId, launch.command.split(" ").at(-1));
  assert.notEqual(launch.sessionId, SID);
});

test("이어 열기: 보드 상태를 아직 못 받았으면 이어 연다", () => {
  const launch = launchOf(held(), { sessions: null, tabName: "a" });
  assert.equal(launch.command, `claude --name a --resume ${SID}`);
});

test("이어 열기: 세션 id 가 없으면 새 세션", () => {
  const launch = launchOf(held({ session: undefined }), { sessions: [], tabName: "a" });
  assert.match(launch.command, /^claude --name a --session-id /);
});

test("이어 열기: 모양이 맞지 않는 세션 id 는 명령에 넣지 않는다", () => {
  assert.equal(launchOf(held({ session: "x; rm -rf" }), { sessions: null, tabName: "a" }), null);
});

test("이어 열기: 명령이 비면 이어 열 것이 없다", () => {
  assert.equal(launchOf(held({ command: "" }), { sessions: [], tabName: "a" }), null);
});

test("이어 열기: claude 가 아닌 명령은 그대로", () => {
  const launch = launchOf({ cwd: "c", command: "npm run dev" }, { sessions: [], tabName: "a" });
  assert.deepEqual(launch, { command: "npm run dev", sessionId: null });
});
