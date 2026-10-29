// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/place.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import type { BoardSurface, Session } from "../types.ts";
import { pairSessions, placeOf } from "./place.ts";
import type { SurfaceChange } from "./place.ts";

// ── 도우미 ────────────────────────────────────────────────────────────────────

let serial = 0;
/** 시험마다 없어도 되는 필드를 생략할 수 있게 Session 의 모든 필수 값을 채우는 도우미 */
const session = (over: Partial<Session>): Session => ({
  id: `id${++serial}`,
  cwd: ".",
  state: "working",
  since: 1,
  updated: 2,
  headless: false,
  title: "",
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
  ...over,
});

/** 아직 termId 가 없는 셸 화면(null)까지 만들 수 있는 BoardSurface 도우미 */
const surface = (over: Partial<BoardSurface>): BoardSurface => ({
  id: `f${++serial}`,
  termId: null,
  sessionId: null,
  command: null,
  held: null,
  mdRenderOff: false,
  title: "",
  ...over,
});

/**
 * hub.svelte.ts 가 적용할 고침을 복사본 화면에 옮기는 도우미 — mac place() 는 pair() 뒤에 화면 고침이 끝난
 * 상태에서 부르므로, placeOf 시험도 이 어긋나는 순서를 따라 본다. 모르는 화면 id 는 착시로 넘기지 않고 터뜨린다
 */
function surfaceAfter(surfaces: BoardSurface[], changes: SurfaceChange[]): BoardSurface[] {
  const after = surfaces.map((surface) => ({ ...surface }));
  for (const change of changes) {
    const target = after.find((surface) => surface.id === change.surfaceId);
    if (!target) throw new Error(`모르는 화면 id 의 고침: ${JSON.stringify(change)}`);
    if (change.kind === "sessionId") target.sessionId = change.sessionId;
    else if (change.kind === "command") target.command = change.command;
    else if (change.kind === "reset") {
      target.command = null;
      target.mdRenderOff = false;
    }
  }
  return after;
}

// ── 규칙 1 — tty 짝 ──────────────────────────────────────────────────────────

test("tty 짝 — 사는 세션 tty 의 화면에 짝지고 화면 sessionId 를 그 세션 id 로 대신한다", () => {
  const f1 = surface({ id: "f1", termId: "t1" });
  // -p 로 해 둔다 — 대화형(mode 없음)이면 화면 고침 3(command=claude)도 겹치므로, 이 시험은 짝짓기에만 본다
  const a = session({ id: "a", tty: "t1", mode: "-p" });
  const plan = pairSessions({ sessions: [a], surfaces: [f1] });
  assert.deepEqual(plan.paired, { a: "f1" });
  assert.deepEqual(plan.owned, {});
  // 화면 sessionId 가 아직 없으므로(null) 붙여 준다
  assert.deepEqual(plan.changes, [{ kind: "sessionId", surfaceId: "f1", sessionId: "a" }]);
});

test("/clear — 새 id 가 화면 sessionId 를 대신하고, 끝난 옛 id 는 ended 로 떨어진다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude" });
  // /clear — 같은 pid, 같은 콘솔(tty)이고 id 만 바뀌었다. 옛 세션은 끝났다
  const fresh = session({ id: "new", tty: "t1" });
  const old = session({ id: "old", state: "ended", tty: "t1" });
  const plan = pairSessions({ sessions: [fresh, old], surfaces: [f1] });
  // 끝난 옛 id 는 짝짓지 않는다 — 표에는 새 id 만 있다
  assert.deepEqual(plan.paired, { new: "f1" });
  assert.deepEqual(plan.changes, [{ kind: "sessionId", surfaceId: "f1", sessionId: "new" }]);
  // hub 가 고침을 적용한 뒤 화면에서 두 세션의 자리를 본다
  const after = surfaceAfter([f1], plan.changes);
  assert.deepEqual(placeOf(fresh, plan, after), { kind: "tab", surfaceId: "f1", headless: false });
  assert.deepEqual(placeOf(old, plan, after), { kind: "ended" });
});

test("끝난 세션은 sessionId 가 같은 화면이 있어도, runsClaude 가 그 termId 를 가르킬 때만 탭 소속이다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "a", command: "claude" });
  const a = session({ id: "a", state: "ended" });
  const plan = pairSessions({ sessions: [a], surfaces: [f1] }); // 끝났으니 짝짓지 않는다
  assert.deepEqual(plan.paired, {});
  assert.deepEqual(placeOf(a, plan, [f1]), { kind: "ended" }); // runsClaude 없음 — 끝남(칩 "보드에서 이어 열기")
  // 이어 열기를 막 둔 직후 — 화면에서 claude 가 막 뜨는 중이면 다시 탭 소속이다
  assert.deepEqual(placeOf(a, plan, [f1], ["t1"]), { kind: "tab", surfaceId: "f1", headless: false });
});

// ── 규칙 2 — 탭 자손 headless 세션 ───────────────────────────────────────────

test("headless + tabAncestor — 탭 화면 소속(headless=true), 칩 글자 CB -p 는 PlaceChip 이 place·mode 로 조립한다", () => {
  const f1 = surface({ id: "f1", termId: "t1" });
  const a = session({ id: "a", headless: true, tabAncestor: "t1", mode: "-p" });
  const plan = pairSessions({ sessions: [a], surfaces: [f1] });
  assert.deepEqual(plan.paired, { a: "f1" });
  assert.deepEqual(plan.owned, { a: true });
  assert.deepEqual(plan.changes, []); // 탭 자손 짝지기는 화면 sessionId 를 바꾸지 않는다(mac 도 이렇다)
  assert.deepEqual(placeOf(a, plan, [f1]), { kind: "tab", surfaceId: "f1", headless: true });
});

test("-p 세션을 tty 로 짝지면 탭 소속(headless=false)이고 command 를 새기지 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1" });
  const a = session({ id: "a", tty: "t1", mode: "-p" });
  const plan = pairSessions({ sessions: [a], surfaces: [f1] });
  assert.deepEqual(plan.paired, { a: "f1" });
  assert.deepEqual(plan.owned, {}); // tty 로 짝진 세션은 탭의 주인이 아니다
  // 사람이 친 대화형만 command 를 새긴다 — -p 는 mode 에 남아 칩(CB -p)의 근거다
  assert.deepEqual(
    plan.changes.filter((change) => change.kind === "command"),
    [],
  );
  assert.deepEqual(placeOf(a, plan, [f1]), { kind: "tab", surfaceId: "f1", headless: false });
});

// ── 규칙 3 — 화면의 claude 명령 새기기·비우기 ────────────────────────────────

test("셸 프롬프트에서 사람이 친 대화형 claude 는 command=claude 를 새긴다, 화면의 -p 는 새기지 않는다", () => {
  // mode 가 빠진(값이 없거나 null) 대화형 — 스냅샷이 mode 를 아직 못 채웠어도 대화형으로 본다
  const f1 = surface({ id: "f1", termId: "t1" });
  const talk = session({ id: "talk", tty: "t1", mode: null });
  const plan = pairSessions({ sessions: [talk], surfaces: [f1] });
  assert.deepEqual(plan.changes, [
    { kind: "sessionId", surfaceId: "f1", sessionId: "talk" },
    { kind: "command", surfaceId: "f1", command: "claude" },
  ]);

  // -p 세션이 짝지어 온 화면은 명령을 새기지 않는다
  const f2 = surface({ id: "f2", termId: "t2" });
  const printed = session({ id: "printed", tty: "t2", mode: "-p" });
  const plan2 = pairSessions({ sessions: [printed], surfaces: [f2] });
  assert.deepEqual(
    plan2.changes.filter((change) => change.kind === "command"),
    [],
  );
});

test("claude 가 사라지면 명령을 비우고 꾸미기 끔을 푼다 — runsClaude 가 아직 가르키면 비우지 않는다", () => {
  // 보드가 넣은 claude 화면 — 세션이 목록에서 사라져도 화면에서 claude 가 막 뜨는 중이면 그대로 둔다
  const f1 = surface({ id: "f1", termId: "t1", command: "claude --dangerously-skip-permissions", mdRenderOff: true });
  const holding = pairSessions({ sessions: [], surfaces: [f1], runsClaude: ["t1"] });
  assert.deepEqual(holding.changes, []);

  // runsClaude 가 값 자체가 없거나 null 이면(스냅샷에 아직 못 채웠다) 빈 것으로 본다 — 셸 화면으로 돌아간다
  const gone = pairSessions({ sessions: [], surfaces: [f1] });
  assert.deepEqual(gone.changes, [{ kind: "reset", surfaceId: "f1" }]);
  const nullList = pairSessions({ sessions: [], surfaces: [f1], runsClaude: null });
  assert.deepEqual(nullList.changes, [{ kind: "reset", surfaceId: "f1" }]);

  // 명령이 이미 없는 셸 화면은 아무 고침이 없다
  const plan = pairSessions({ sessions: [], surfaces: [surface({ id: "f2", termId: "t2" })] });
  assert.deepEqual(plan.changes, []);
});

// ── 규칙 4 — placeOf 의 나머지 자리 ─────────────────────────────────────────

test("짝지지 못한 headless 세션은 background($ claude -p · --bg)다", () => {
  const a = session({ id: "a", state: "idle", headless: true, mode: "--bg" });
  assert.deepEqual(placeOf(a, { paired: {}, owned: {} }, []), { kind: "background" });
});

test("짝지지 못한 대화형 세션은 다른 터미널(terminal)이다 — 앱 이름은 Session.appName 이 전한다", () => {
  const a = session({ id: "a", state: "working", tty: null, appName: "Windows Terminal" });
  assert.deepEqual(placeOf(a, { paired: {}, owned: {} }, []), { kind: "terminal" });
});

test("tty 가 모르는 termId 를 가르키면 짝짓지 않고 어떤 화면도 붙게 하지 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1" });
  const a = session({ id: "a", tty: "tx" }); // 이런 termId 의 화면은 없다
  const plan = pairSessions({ sessions: [a], surfaces: [f1] });
  assert.deepEqual(plan.paired, {});
  assert.deepEqual(plan.changes, []); // 화면을 건드리는 고침도 없다
  assert.deepEqual(placeOf(a, plan, [f1]), { kind: "terminal" });
});

test("화면 둘 — 세션끼리 서로의 화면으로 새지 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1" });
  const f2 = surface({ id: "f2", termId: "t2" });
  const a = session({ id: "a", tty: "t1" });
  const b = session({ id: "b", tty: "t2" });
  const c = session({ id: "c", headless: true, tabAncestor: "t2" }); // 탭 자손도 스스로 가르킨 화면으로만 간다
  const plan = pairSessions({ sessions: [a, b, c], surfaces: [f1, f2] });
  assert.deepEqual(plan.paired, { a: "f1", b: "f2", c: "f2" });
  assert.deepEqual(plan.owned, { c: true });
  // tty 짝 두 세션이 모두 대화형이므로 화면마다 명령이 새겨진다 — 각각 나의 화면에만
  assert.deepEqual(plan.changes, [
    { kind: "sessionId", surfaceId: "f1", sessionId: "a" },
    { kind: "sessionId", surfaceId: "f2", sessionId: "b" },
    { kind: "command", surfaceId: "f1", command: "claude" },
    { kind: "command", surfaceId: "f2", command: "claude" },
  ]);
  assert.deepEqual(placeOf(a, plan, [f1, f2]), { kind: "tab", surfaceId: "f1", headless: false });
  assert.deepEqual(placeOf(b, plan, [f1, f2]), { kind: "tab", surfaceId: "f2", headless: false });
  assert.deepEqual(placeOf(c, plan, [f1, f2]), { kind: "tab", surfaceId: "f2", headless: true });
});

// ── 순수성 — 입력을 고치지 않는다 ───────────────────────────────────────────

test("pairSessions 는 입력을 고치지 않는다 — 고침은 계획(changes)으로만 돌려준다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", mdRenderOff: true });
  const beforeSurface = { ...f1 };
  const a = session({ id: "a", tty: "t1" });
  const beforeSession = { ...a };
  pairSessions({ sessions: [a], surfaces: [f1] });
  assert.deepEqual(f1, beforeSurface); // 세션 id 대신을 hub 의 mutator 가 하도록 화면은 그대로다
  assert.deepEqual(a, beforeSession);
});

// ── typedAt — 명령을 친 직후 3초는 claude 가 도는 것으로 본다 ─────────────────

test("typedAt 3초 안 — 끝난 id 를 쥔 화면은 tab 으로 남고 reset 이 나오지 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: 10_000 });
  const ended = session({ id: "old", state: "ended" });
  const plan = pairSessions({ sessions: [ended], surfaces: [f1], now: 12_999 });
  assert.deepEqual(plan.changes, []);
  assert.deepEqual(placeOf(ended, plan, [f1], [], 12_999), { kind: "tab", surfaceId: "f1", headless: false });
});

test("typedAt 3초가 지나고 runsClaude 도 없으면 ended 가 되고 reset 이 나온다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: 10_000 });
  const ended = session({ id: "old", state: "ended" });
  const plan = pairSessions({ sessions: [ended], surfaces: [f1], now: 13_000 });
  assert.deepEqual(plan.changes, [{ kind: "reset", surfaceId: "f1" }]);
  assert.deepEqual(placeOf(ended, plan, [f1], [], 13_000), { kind: "ended" });
});

test("typedAt 가 null 이면 기존 동작 그대로, now 를 비우면 Date.now() 를 쓴다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: null });
  const ended = session({ id: "old", state: "ended" });
  assert.deepEqual(pairSessions({ sessions: [ended], surfaces: [f1] }).changes, [{ kind: "reset", surfaceId: "f1" }]);
  const fresh = surface({ id: "f2", termId: "t2", sessionId: "old2", command: "claude", typedAt: Date.now() });
  assert.deepEqual(pairSessions({ sessions: [], surfaces: [fresh] }).changes, []);
  assert.deepEqual(placeOf(session({ id: "old2", state: "ended" }), { paired: {}, owned: {} }, [fresh]), {
    kind: "tab", surfaceId: "f2", headless: false,
  });
});
test("typedAt 경계 — 2999ms 는 아직 돈다, 3000ms 는 아니다(< 3초)", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: 0 });
  const ended = session({ id: "old", state: "ended" });
  assert.deepEqual(placeOf(ended, { paired: {}, owned: {} }, [f1], [], 2999), { kind: "tab", surfaceId: "f1", headless: false });
  assert.deepEqual(placeOf(ended, { paired: {}, owned: {} }, [f1], [], 3000), { kind: "ended" });
});

test("typedAt 는 그 화면이 쥔 세션 id 에만 걸린다 — 다른 화면의 typedAt 이 끝난 세션을 붙들지 않는다", () => {
  const mine = surface({ id: "f1", termId: "t1", sessionId: "old", typedAt: null });
  const other = surface({ id: "f2", termId: "t2", sessionId: "else", typedAt: 10_000 });
  const ended = session({ id: "old", state: "ended" });
  assert.deepEqual(placeOf(ended, { paired: {}, owned: {} }, [mine, other], [], 10_500), { kind: "ended" });
});

test("typedAt 는 살아 있는 세션에는 영향이 없다 — terminal·background 그대로", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "x", typedAt: 10_000 });
  const live = session({ id: "live", state: "working" });
  const bg = session({ id: "bg", state: "working", headless: true });
  assert.deepEqual(placeOf(live, { paired: {}, owned: {} }, [f1], [], 10_100), { kind: "terminal" });
  assert.deepEqual(placeOf(bg, { paired: {}, owned: {} }, [f1], [], 10_100), { kind: "background" });
});

test("typedAt 창 안에서 두 번째 클릭이 와도 두 번 다 tab 이라 reopen 이 다시 나가지 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: 5_000 });
  const ended = session({ id: "old", state: "ended" });
  for (const now of [5_001, 6_500, 7_999]) {
    const plan = pairSessions({ sessions: [ended], surfaces: [f1], now });
    assert.equal(placeOf(ended, plan, [f1], [], now).kind, "tab", String(now));
    assert.deepEqual(plan.changes, [], String(now));
  }
});

test("미래의 typedAt(시계가 뒤로 감김)도 창 안으로 본다 — 3초 안이라 tab 으로 남는다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", typedAt: 20_000 });
  const ended = session({ id: "old", state: "ended" });
  assert.equal(placeOf(ended, { paired: {}, owned: {} }, [f1], [], 10_000).kind, "tab");
});

test("typedAt 가 undefined(필드 없음)여도 null 과 같다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude" });
  delete f1.typedAt;
  const ended = session({ id: "old", state: "ended" });
  assert.equal(placeOf(ended, { paired: {}, owned: {} }, [f1], [], 1).kind, "ended");
  assert.deepEqual(pairSessions({ sessions: [ended], surfaces: [f1], now: 1 }).changes, [{ kind: "reset", surfaceId: "f1" }]);
});

test("runsClaude 가 null·undefined 여도 typedAt 창은 그대로 동작한다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: 1_000 });
  const ended = session({ id: "old", state: "ended" });
  for (const runsClaude of [null, undefined]) {
    assert.equal(placeOf(ended, { paired: {}, owned: {} }, [f1], runsClaude, 1_500).kind, "tab");
    assert.deepEqual(pairSessions({ sessions: [ended], surfaces: [f1], runsClaude, now: 1_500 }).changes, []);
  }
});

test("termId 가 null 인 화면은 runsClaude 로는 못 붙들어도 typedAt 으로는 붙든다(셸이 아직 안 뜬 찰나)", () => {
  const f1 = surface({ id: "f1", termId: null, sessionId: "old", command: "claude", typedAt: 100 });
  const ended = session({ id: "old", state: "ended" });
  assert.equal(placeOf(ended, { paired: {}, owned: {} }, [f1], ["t1"], 200).kind, "tab");
  assert.equal(placeOf(ended, { paired: {}, owned: {} }, [f1], ["t1"], 5_000).kind, "ended");
});

test("typedAt 창이 지나면 reset 은 나오지만 runsClaude 가 그 termId 를 가르키는 동안은 나오지 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "old", command: "claude", typedAt: 0 });
  const ended = session({ id: "old", state: "ended" });
  assert.deepEqual(pairSessions({ sessions: [ended], surfaces: [f1], runsClaude: ["t1"], now: 60_000 }).changes, []);
  assert.deepEqual(pairSessions({ sessions: [ended], surfaces: [f1], runsClaude: ["t2"], now: 60_000 }).changes, [{ kind: "reset", surfaceId: "f1" }]);
});

test("끝난 세션을 같은 id 로 화면이 둘 쥐면 먼저 나온 화면이 이긴다(find) — 결정적이다", () => {
  const a = surface({ id: "fa", termId: "t1", sessionId: "old", typedAt: 1_000 });
  const b = surface({ id: "fb", termId: "t2", sessionId: "old", typedAt: 1_000 });
  const ended = session({ id: "old", state: "ended" });
  assert.equal(placeOf(ended, { paired: {}, owned: {} }, [a, b], [], 1_100).surfaceId, "fa");
  assert.equal(placeOf(ended, { paired: {}, owned: {} }, [b, a], [], 1_100).surfaceId, "fb");
});

// ── 화면 기억 세션 id — 살아 있는 세션도 runsClaude·typedAt 문지기를 지난다 ────

test("claude 를 안 도는 화면에 옛 세션 id 만 남아 있으면 살아 있는 세션도 붙지 않는다 — 다른 터미널·백그라운드다", () => {
  // 세션 x 는 이 보드에 없는 콘솔(ttys007)에서 도는 중 — 화면 f1 에는 옛 id 만 남아 있다
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "x" });
  for (const state of ["idle", "working"] as const) {
    const x = session({ id: "x", state, tty: "ttys007" });
    const plan = pairSessions({ sessions: [x], surfaces: [f1] });
    const after = surfaceAfter([f1], plan.changes);
    assert.deepEqual(placeOf(x, plan, after), { kind: "terminal" }, state);
  }
  const bg = session({ id: "x", state: "working", tty: "ttys007", headless: true, mode: "-p" });
  const bgPlan = pairSessions({ sessions: [bg], surfaces: [f1] });
  assert.deepEqual(placeOf(bg, bgPlan, surfaceAfter([f1], bgPlan.changes)), { kind: "background" });
});

test("옛 id 를 쥔 화면에서 claude 가 도는 중이면(runsClaude) 살아 있는 세션도 그 화면이다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "x", command: "claude" });
  const x = session({ id: "x", state: "working", tty: "ttys007" });
  const plan = pairSessions({ sessions: [x], surfaces: [f1], runsClaude: ["t1"] });
  assert.deepEqual(plan.changes, []); // claude 가 도는 중이므로 고침이 없다
  assert.deepEqual(placeOf(x, plan, [f1], ["t1"]), { kind: "tab", surfaceId: "f1", headless: false });
});

test("3초 안에 claude 명령을 친 화면도 살아 있는 세션을 붙든다 — 창이 지나면 다른 터미널로 떨어진다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "x", command: "claude", typedAt: 10_000 });
  const x = session({ id: "x", state: "working", tty: "ttys007" });
  const plan = pairSessions({ sessions: [x], surfaces: [f1], now: 12_999 }); // 창 안
  assert.deepEqual(plan.changes, []);
  assert.deepEqual(placeOf(x, plan, [f1], [], 12_999), { kind: "tab", surfaceId: "f1", headless: false });
  const expired = pairSessions({ sessions: [x], surfaces: [f1], now: 13_000 }); // 창 밖
  assert.deepEqual(expired.changes, [{ kind: "reset", surfaceId: "f1" }]);
  assert.deepEqual(placeOf(x, expired, surfaceAfter([f1], expired.changes), [], 13_000), { kind: "terminal" });
});

// ── 같은 tty 로 짝지어 온 세션이 여럿 — 화면 주 세션은 (updated, since, id) 차례로 고른다 ──

/**
 * 같은 tty 로 짝지어 온 세션 쌍을 목록 차례와 그 반대로 두 번 짝짓는다 — 주 세션과 화면 고침은 세션 목록
 * 차례와 관계없어야 한다(mac TabPairing.ttyWinner: updated, since, id 사전식). 주인은 sessionId 고침의
 * 대상으로, paired 표의 멤버로 각각 드러난다
 */
function assertSameTtyWinner(
  sessions: Session[],
  expectedPaired: Record<string, string>,
  expectedChanges: SurfaceChange[],
): void {
  const f1 = surface({ id: "f1", termId: "t1" });
  for (const order of [sessions, [...sessions].reverse()] as Session[][]) {
    const label = order.map((item) => item.id).join("+");
    const plan = pairSessions({ sessions: order, surfaces: [f1], now: 1_000 });
    assert.deepEqual(plan.paired, expectedPaired, label);
    assert.deepEqual(plan.changes, expectedChanges, label);
  }
}

test("같은 tty 에 짝진 세션 둘 — updated 가 늦은 쪽이 화면 주인이고 sessionId 는 주인을 따라간다", () => {
  // id 글자 차례(b < a)와 updated 차례가 어긋나 있어야 목록 차례·id 차례 양쪽에서 다른 쪽을 거를 수 있다
  const a = session({ id: "a", tty: "t1", mode: null, updated: 10 });
  const b = session({ id: "b", tty: "t1", mode: null, updated: 0, since: 20 });
  assertSameTtyWinner([a, b], { a: "f1", b: "f1" }, [
    { kind: "sessionId", surfaceId: "f1", sessionId: "a" },
    { kind: "command", surfaceId: "f1", command: "claude" },
  ]);
});

test("updated 도 같으면 화면에 온 지 더 오래된(since 가 늦은) 쪽이 화면 주인이다 — 목록 차례와 무관", () => {
  const a = session({ id: "a", tty: "t1", mode: null, updated: 5, since: 3 });
  const b = session({ id: "b", tty: "t1", mode: null, updated: 5, since: 1 });
  assertSameTtyWinner([a, b], { a: "f1", b: "f1" }, [
    { kind: "sessionId", surfaceId: "f1", sessionId: "a" },
    { kind: "command", surfaceId: "f1", command: "claude" },
  ]);
});

test("updated·since 까지 같으면 세션 id 글자 차례의 뒤 쪽이 화면 주인이다(mac ttyWinner) — 목록 차례와 무관", () => {
  const a = session({ id: "a", tty: "t1", mode: null, updated: 5, since: 1 });
  const b = session({ id: "b", tty: "t1", mode: null, updated: 5, since: 1 });
  assertSameTtyWinner([a, b], { a: "f1", b: "f1" }, [
    { kind: "sessionId", surfaceId: "f1", sessionId: "b" },
    { kind: "command", surfaceId: "f1", command: "claude" },
  ]);
});

test("끝난 세션은 주인 견주기에 들지 않는다 — updated 가 아무리 늦어도 paired 에서 빠진다", () => {
  const a = session({ id: "a", tty: "t1", mode: null, updated: 2 });
  const z = session({ id: "z", state: "ended", tty: "t1", mode: null, updated: 100 });
  assertSameTtyWinner([a, z], { a: "f1" }, [
    { kind: "sessionId", surfaceId: "f1", sessionId: "a" },
    { kind: "command", surfaceId: "f1", command: "claude" },
  ]);
});

test("주인의 id 가 이미 화면에 남아 있으면 sessionId 고침이 없다 — 패자 id 로 되바꾸지도 않는다", () => {
  const f1 = surface({ id: "f1", termId: "t1", sessionId: "b" }); // 화면이 이미 주인 b 의 id 를 쥔다
  const a = session({ id: "a", tty: "t1", mode: "-p", updated: 0 }); // 패자(-p 면 command 새기기도 없어 고침 0)
  const b = session({ id: "b", tty: "t1", mode: "-p", updated: 10 }); // 주인
  for (const order of [[a, b], [b, a]] as Session[][]) {
    const label = order.map((item) => item.id).join("+");
    const plan = pairSessions({ sessions: order, surfaces: [f1], now: 1_000 });
    assert.deepEqual(plan.paired, { a: "f1", b: "f1" }, label);
    assert.deepEqual(plan.changes, [], label);
  }
});
