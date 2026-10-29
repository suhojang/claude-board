// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/restoreGuards.test.ts
// restore.ts 의 안전 규칙 — 작업 폴더가 없을 때(H1)·이어 열 순간의 생사(M6)·옛 pid 의 생성 시각(L2)·저장 목록 덮어쓰기와 순서(M5·L1).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  claudeDirs,
  heldLaunch,
  liveCheckTarget,
  mayOverwriteSaved,
  missingDir,
  restorePlan,
  savedClaudeDirs,
  serialSender,
  waitOldPids,
} from "./restore.ts";

const SID = "11111111-2222-3333-4444-555555555555";
const shellLeaf = (cwd = "C:\\work\\a") => ({ leaf: { cwd, command: null, sessionId: null, mdRenderOff: false } });
const claudeLeaf = (over = {}) => ({ leaf: { cwd: "C:\\work\\a", command: "claude", sessionId: SID, mdRenderOff: false, ...over } });
const dirsOf = (...dirs) => new Set(dirs);
const planOf = (entries, dirs, alive = new Set()) => restorePlan(entries, alive, dirs);
const held = (over = {}) => ({ cwd: "C:\\work\\a", command: "claude", session: SID, ...over });
const launchOf = (heldValues, context) => heldLaunch(heldValues, { workspace: "C:\\work\\a", existingDirs: dirsOf("C:\\work\\a"), tabName: "a", ...context });
const target = (pid, ctime = null) => ({ pid, ctime });

// ── H1: 작업 폴더가 없으면 claude 를 되살리지도 이어 열지도 않는다 ─────

test("폴더: 홈(~)·빈 경로는 요구하지 않고 탭 작업 경로와 저장 경로를 요구한다", () => {
  assert.deepEqual(claudeDirs("C:\\w", "C:\\w\\sub"), ["C:\\w", "C:\\w\\sub"]);
  assert.deepEqual(claudeDirs("~", "C:\\w"), ["C:\\w"]);
  assert.deepEqual(claudeDirs("~", ""), []);
});

test("폴더: 없는 첫 폴더를 돌려주고 다 있으면 null", () => {
  assert.equal(missingDir("C:\\w", "C:\\w\\sub", dirsOf("C:\\w", "C:\\w\\sub")), null);
  assert.equal(missingDir("C:\\w", "C:\\w\\sub", dirsOf("C:\\w")), "C:\\w\\sub");
  assert.equal(missingDir("C:\\w", "C:\\w\\sub", dirsOf()), "C:\\w");
  assert.equal(missingDir("~", "", dirsOf()), null);
});

test("폴더: 저장 항목에서 claude 화면이 요구하는 폴더만 중복 없이 모은다", () => {
  const entries = [
    { workspace: "C:\\a", name: null, selected: false, root: claudeLeaf({ cwd: "C:\\a\\x" }) },
    { workspace: "C:\\b", name: null, selected: false, root: { split: "down", ratio: 0.5, first: claudeLeaf({ cwd: "C:\\a\\x" }), second: shellLeaf("C:\\only-shell") } },
    { workspace: "~", name: null, selected: false, root: claudeLeaf({ cwd: "C:\\h" }) },
  ];
  assert.deepEqual(savedClaudeDirs(entries), ["C:\\a", "C:\\a\\x", "C:\\b", "C:\\h"]);
});

test("되살리기: 작업 폴더가 없는 claude 화면은 되살리지 않고 없는 폴더를 알려 준다(홈 폴더로 대신 열지 않는다)", () => {
  const entries = [{ workspace: "C:\\gone", name: null, selected: true, root: claudeLeaf({ cwd: "C:\\gone" }) }];
  const plan = planOf(entries, dirsOf());
  assert.deepEqual(plan.tabs, []);
  assert.deepEqual(plan.surfaces, []);
  assert.equal(plan.resumeNow, null);
  assert.deepEqual(plan.missingDirs, ["C:\\gone"]);
});

test("되살리기: 탭 작업 경로는 있어도 저장한 화면 경로가 없으면 되살리지 않는다", () => {
  const entries = [{ workspace: "C:\\a", name: null, selected: true, root: claudeLeaf({ cwd: "C:\\a\\deleted" }) }];
  const plan = planOf(entries, dirsOf("C:\\a"));
  assert.equal(plan.surfaces.length, 0);
  assert.deepEqual(plan.missingDirs, ["C:\\a\\deleted"]);
});

test("되살리기: 폴더가 없는 셸 화면은 mac 처럼 그대로 되살린다(홈에서 셸로 뜬다)", () => {
  const plan = planOf([{ workspace: "C:\\gone", name: null, selected: true, root: shellLeaf("C:\\gone") }], dirsOf());
  assert.equal(plan.tabs.length, 1);
  assert.equal(plan.surfaces[0].held, null);
  assert.deepEqual(plan.missingDirs, []);
});

test("되살리기: 나눈 탭에서 폴더 없는 claude 화면만 빠지고 셸 형제가 남는다", () => {
  const entries = [{ workspace: "C:\\a", name: null, selected: true, root: { split: "right", ratio: 0.5, first: claudeLeaf({ cwd: "C:\\gone" }), second: shellLeaf("C:\\a") } }];
  const plan = planOf(entries, dirsOf("C:\\a"));
  assert.equal(plan.surfaces.length, 1);
  assert.equal(plan.surfaces[0].held, null);
  assert.deepEqual(plan.missingDirs, ["C:\\gone"]);
});

test("되살리기: 홈(~) 탭의 claude 화면은 홈이 늘 있으므로 되살린다", () => {
  const entries = [{ workspace: "~", name: null, selected: true, root: claudeLeaf({ cwd: "C:\\h" }) }];
  assert.equal(planOf(entries, dirsOf("C:\\h")).surfaces.length, 1);
});

test("되살리기: 있는 폴더의 claude 화면은 그대로 되살린다", () => {
  const entries = [{ workspace: "C:\\work\\a", name: null, selected: true, root: claudeLeaf() }];
  const plan = planOf(entries, dirsOf("C:\\work\\a"));
  assert.equal(plan.surfaces.length, 1);
  assert.deepEqual(plan.missingDirs, []);
  assert.equal(plan.resumeNow, plan.tabs[0].focused);
});

test("이어 열기: 탭 작업 경로나 저장 경로가 없으면 명령을 만들지 않는다", () => {
  const ended = { sessions: [{ id: SID, state: "ended" }] };
  assert.equal(launchOf(held(), ended)?.sessionId, SID);
  assert.equal(launchOf(held(), { ...ended, existingDirs: dirsOf() }), null, "탭 작업 경로(C:\\work\\a)가 없다");
  assert.equal(launchOf(held({ cwd: "C:\\work\\a\\sub" }), ended), null, "저장 경로가 없다");
  assert.equal(launchOf(held({ session: undefined }), { sessions: [], existingDirs: dirsOf() }), null, "새 세션 명령도 마찬가지");
});

// ── M6: 이어 열 때의 생사 ───────────────────────────────────

test("이어 열기: 상태 묶음은 아직 끝나지 않았어도 지금 프로세스가 죽었으면 이어 연다", () => {
  const ctx = (alive) => ({ sessions: [{ id: SID, state: "working", alive }] });
  assert.equal(launchOf(held(), ctx(false))?.sessionId, SID, "방금 죽은 옛 보드의 claude");
  assert.equal(launchOf(held(), ctx(true)), null, "지금도 산다 = 보드 밖에서 도는 세션");
  assert.equal(launchOf(held(), ctx(undefined)), null, "물어보지 못했으면 상태를 따른다");
});

test("생사 확인 대상: 끝나지 않은 세션만 pid 와 생성 시각을 보낸다", () => {
  const s = (over) => ({ id: SID, state: "working", pid: 7, pidCtime: 12.5, ...over });
  assert.deepEqual(liveCheckTarget(SID, [s({})]), { pid: 7, ctime: 12.5 });
  assert.deepEqual(liveCheckTarget(SID, [s({ pidCtime: undefined })]), { pid: 7, ctime: null });
  assert.equal(liveCheckTarget(SID, [s({ state: "ended" })]), null);
  assert.equal(liveCheckTarget(SID, [s({ pid: 0 })]), null, "pid 를 모르면 물을 수 없다");
  assert.equal(liveCheckTarget(SID, []), null);
  assert.equal(liveCheckTarget(SID, null), null);
  assert.equal(liveCheckTarget(undefined, [s({})]), null);
});

// ── L2: 옛 pid 는 생성 시각과 함께 묻는다 ───────────────────

function fakeClock() {
  let now = 0;
  return { now: () => now, sleep: async (ms) => void (now += ms) };
}

test("옛 pid 기다리기: 생성 시각을 그대로 실어 묻는다", async () => {
  const asked = [];
  await waitOldPids([target(7, 1.5), target(9, null)], async (list) => (asked.push(list), []), fakeClock());
  assert.deepEqual(asked, [[target(7, 1.5), target(9, null)]]);
});

test("옛 pid 기다리기: 다시 물을 때는 아직 산 것만 묻는다", async () => {
  const asked = [];
  let calls = 0;
  await waitOldPids([target(7, 1.5), target(9, 2.5)], async (list) => (asked.push(list.map((t) => t.pid)), ++calls < 2 ? [9] : []), fakeClock());
  assert.deepEqual(asked, [[7, 9], [9]]);
});

// ── M5·L1: 저장 목록 덮어쓰기와 저장 순서 ─────────────────

test("저장 허용: 다시 세웠거나 처음 켠 때만 덮어쓴다 — 이어받은 맨 셸이나 못 읽은 목록으로는 덮지 않는다", () => {
  assert.equal(mayOverwriteSaved("restored"), true);
  assert.equal(mayOverwriteSaved("fresh"), true);
  assert.equal(mayOverwriteSaved("adopted"), false);
  assert.equal(mayOverwriteSaved("unreadable"), false);
});

test("저장 순서: 앞의 저장이 끝난 뒤 다음을 보내 순서가 뒤집히지 않는다", async () => {
  const log = [];
  const gates = [];
  const send = serialSender(
    (item) =>
      new Promise((resolve) => {
        log.push(`start ${item}`);
        gates.push(() => (log.push(`end ${item}`), resolve()));
      }),
    () => assert.fail("오류가 없어야 한다"),
  );
  const first = send("a");
  const second = send("b");
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(log, ["start a"], "둘째는 첫째가 끝나기 전에 시작하지 않는다");
  gates[0]();
  await first;
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(log, ["start a", "end a", "start b"]);
  gates[1]();
  await second;
  assert.deepEqual(log, ["start a", "end a", "start b", "end b"]);
});

test("저장 순서: 하나가 실패해도 알리고 다음 저장을 막지 않으며, 돌려준 약속은 끝나면 풀린다", async () => {
  const seen = [];
  const errors = [];
  const send = serialSender(
    async (item) => {
      if (item === "bad") throw new Error("x");
      seen.push(item);
    },
    (e) => errors.push(e.message),
  );
  await send("bad");
  await send("good");
  assert.deepEqual(seen, ["good"]);
  assert.deepEqual(errors, ["x"]);
});
