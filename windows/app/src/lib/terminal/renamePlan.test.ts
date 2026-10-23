// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/renamePlan.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { nextRenameStep, renameSafe, renameTargets } from "./renamePlan.ts";

let serial = 0;
const session = (over = {}) => ({
  id: `s${++serial}`, cwd: ".", state: "working", since: 1, updated: 2, headless: false, title: "", topic: "", message: "",
  name: null, ctx: null, brain: null, subagents: [], shells: 0, paused: false, muted: false, limitAt: null, canDrive: true,
  tty: null, tabAncestor: null, mode: null, appName: null, ...over,
});
const surface = (termId) => ({ id: `f-${termId}`, termId, sessionId: null, command: null, held: null, mdRenderOff: false, title: "" });

const idle = { safe: true, transcript: true };

test("renameTargets — 주 세션이 있는 화면마다 탭 이름이 목표가 된다", () => {
  const out = renameTargets("작업A", [surface("t1")], [session({ tty: "t1" })], []);
  assert.deepEqual(out, [{ surfaceId: "f-t1", target: "작업A" }]);
});

test("renameTargets — 끝난 세션·-p·--bg·짝 없는 세션·셸 화면은 뺀다", () => {
  const sessions = [
    session({ tty: "t1", state: "ended" }),
    session({ tty: "t2", mode: "-p" }),
    session({ tty: "t3", mode: "--bg" }),
    session({ tty: "other" }),
  ];
  const surfaces = [surface("t1"), surface("t2"), surface("t3"), surface("t4"), surface(null)];
  assert.deepEqual(renameTargets("a", surfaces, sessions, []), []);
});

test("renameTargets — 같은 이름은 -2, -3 … 으로 나눈다", () => {
  const sessions = [session({ tty: "t1" }), session({ tty: "t2" }), session({ tty: "t3" })];
  const out = renameTargets("a", [surface("t1"), surface("t2"), surface("t3")], sessions, []);
  assert.deepEqual(out.map((entry) => entry.target), ["a", "a-2", "a-3"]);
});

test("renameTargets — 대소문자를 가리지 않고, 다른 세션이 쓰는 이름과 reserved 를 피한다", () => {
  const sessions = [session({ tty: "t1" }), session({ tty: "elsewhere", name: "Proj" })];
  assert.deepEqual(renameTargets("proj", [surface("t1")], sessions, []).map((e) => e.target), ["proj-2"]);
  assert.deepEqual(renameTargets("x", [surface("t1")], sessions, ["X"]).map((e) => e.target), ["x-2"]);
});

test("renameTargets — 이미 그 이름이면 목록에서 빠지지만 자리는 차지한다, 자기 이름은 충돌이 아니다", () => {
  const sessions = [session({ tty: "t1", name: "a" }), session({ tty: "t2" })];
  const out = renameTargets("a", [surface("t1"), surface("t2")], sessions, []);
  assert.deepEqual(out, [{ surfaceId: "f-t2", target: "a-2" }]);
});

test("renameSafe — 작업 중·대기(idle)이고 Claude 상태가 waiting 이 아닐 때만", () => {
  assert.equal(renameSafe("working", "busy"), true);
  assert.equal(renameSafe("idle", null), true);
  assert.equal(renameSafe("working", "waiting"), false);
  assert.equal(renameSafe("waiting", null), false);
  assert.equal(renameSafe("ended", null), false);
});

test("nextRenameStep — 시작: 안전하면 글을 붙이고 typing", () => {
  assert.deepEqual(nextRenameStep(undefined, session(), "t", 100, idle), { step: { kind: "typing" }, emit: "text" });
});

test("nextRenameStep — 승인 창(waiting)이면 아무것도 넣지 않는다", () => {
  const r = nextRenameStep(undefined, session(), "t", 100, { safe: false, transcript: true });
  assert.deepEqual(r, { step: undefined, emit: "none" });
});

test("nextRenameStep — 대화 기록이 없으면 넣지 않는다", () => {
  const r = nextRenameStep(undefined, session(), "t", 100, { safe: true, transcript: false });
  assert.deepEqual(r, { step: undefined, emit: "none" });
});

test("nextRenameStep — 글을 붙인 뒤 다시 보니 위험하면 typed 로 Enter 를 미룬다, 안전해지면 Enter", () => {
  const typed = { kind: "typed" };
  assert.deepEqual(nextRenameStep(typed, session(), "t", 100, { safe: false, transcript: true }), { step: typed, emit: "none" });
  assert.deepEqual(nextRenameStep(typed, session(), "t", 200, idle), { step: { kind: "sent", at: 200 }, emit: "enter" });
});

test("nextRenameStep — typing 중에는 기다린다", () => {
  const typing = { kind: "typing" };
  assert.deepEqual(nextRenameStep(typing, session(), "t", 100, idle), { step: typing, emit: "none" });
});

test("nextRenameStep — sent 는 이름이 맞으면 바로, 아니면 5초가 지나야 지운다", () => {
  const sent = { kind: "sent", at: 1000 };
  assert.deepEqual(nextRenameStep(sent, session({ name: "t" }), "t", 1100, idle), { step: undefined, emit: "none" });
  assert.deepEqual(nextRenameStep(sent, session(), "t", 6000, idle), { step: sent, emit: "none" });
  assert.deepEqual(nextRenameStep(sent, session(), "t", 6001, idle), { step: undefined, emit: "none" });
});

test("nextRenameStep — 세션이 끝났으면 어느 단계든 지운다", () => {
  for (const st of [undefined, { kind: "typing" }, { kind: "typed" }, { kind: "sent", at: 1 }]) {
    assert.deepEqual(nextRenameStep(st, session({ state: "ended" }), "t", 2, idle), { step: undefined, emit: "none" });
  }
});

test("renameTargets — mode 가 없어도 headless 인 세션(화면 안 소유 -p 등)은 뺀다", () => {
  const sessions = [session({ tty: "t1", headless: true, mode: null })];
  assert.deepEqual(renameTargets("a", [surface("t1")], sessions, []), []);
});

test("renameTargets — tty 는 있으나 termId 가 없는 화면(null)과 짝 없는 화면은 건너뛰고 나머지는 계속 짝짓는다", () => {
  const sessions = [session({ tty: "t2" })];
  const out = renameTargets("a", [surface(null), surface("t1"), surface("t2")], sessions, []);
  assert.deepEqual(out, [{ surfaceId: "f-t2", target: "a" }]);
});

test("renameTargets — 끝난 세션의 이름은 충돌로 세지 않는다", () => {
  const sessions = [session({ tty: "t1" }), session({ tty: "gone", state: "ended", name: "proj" })];
  assert.deepEqual(renameTargets("proj", [surface("t1")], sessions, []).map((e) => e.target), ["proj"]);
});

test("renameTargets — 다른 세션이 이미 -2 를 쓰면 -3 으로 건너뛴다", () => {
  const sessions = [session({ tty: "t1" }), session({ tty: "x", name: "A" }), session({ tty: "y", name: "A-2" })];
  assert.deepEqual(renameTargets("a", [surface("t1")], sessions, []).map((e) => e.target), ["a-3"]);
});

test("renameTargets — reserved 의 대소문자도 같은 이름으로 본다, 빈 입력은 빈 결과", () => {
  assert.deepEqual(renameTargets("a", [], [], []), []);
  assert.deepEqual(renameTargets("a", [surface("t1")], [], []), []);
  const sessions = [session({ tty: "t1" })];
  assert.deepEqual(renameTargets("Proj", [surface("t1")], sessions, ["PROJ", "proj-2"]).map((e) => e.target), ["Proj-3"]);
});

test("renameTargets — 같은 tty 로 사는 세션이 둘이면 앞의 것이 주 세션이다(find)", () => {
  const first = session({ tty: "t1", name: "a" });
  const second = session({ tty: "t1" });
  assert.deepEqual(renameTargets("a", [surface("t1")], [first, second], []), []);
});

test("renameTargets — 입력 배열과 세션을 고치지 않는다", () => {
  const sessions = [session({ tty: "t1", name: "x" })];
  const surfaces = [surface("t1")];
  const reserved = ["r"];
  const snapshot = JSON.stringify({ sessions, surfaces, reserved });
  renameTargets("a", surfaces, sessions, reserved);
  assert.equal(JSON.stringify({ sessions, surfaces, reserved }), snapshot);
});

test("renameSafe — 일시중지·끝남은 안전하지 않고 Claude 상태가 busy·idle·null 이면 막지 않는다", () => {
  assert.equal(renameSafe("paused", null), false);
  assert.equal(renameSafe("working", "idle"), true);
  assert.equal(renameSafe("idle", "busy"), true);
  assert.equal(renameSafe("idle", "waiting"), false);
  assert.equal(renameSafe("working", ""), true);
});

test("nextRenameStep — 대화 기록이 없으면 typed 단계에서도 Enter 를 미룬다", () => {
  const typed = { kind: "typed" };
  assert.deepEqual(nextRenameStep(typed, session(), "t", 100, { safe: true, transcript: false }), { step: typed, emit: "none" });
});

test("nextRenameStep — sent 에서는 안전 검사 결과와 상관없이 기다린다(Enter 를 두 번 치지 않는다)", () => {
  const sent = { kind: "sent", at: 1000 };
  for (const check of [idle, { safe: false, transcript: false }]) {
    assert.deepEqual(nextRenameStep(sent, session(), "t", 1500, check), { step: sent, emit: "none" });
  }
});

test("nextRenameStep — typing 중에 세션이 그 이름이 되면 끝낸다", () => {
  assert.deepEqual(nextRenameStep({ kind: "typing" }, session({ name: "t" }), "t", 1, idle), { step: undefined, emit: "none" });
});

test("nextRenameStep — 시작 단계에서 안전하지 않으면 step 은 undefined 로 남는다(typing 으로 넘어가지 않는다)", () => {
  assert.deepEqual(nextRenameStep(undefined, session(), "t", 1, { safe: false, transcript: false }), { step: undefined, emit: "none" });
});

test("nextRenameStep — 이름은 대소문자까지 같아야 끝난 것으로 본다", () => {
  assert.deepEqual(nextRenameStep(undefined, session({ name: "T" }), "t", 1, idle), { step: { kind: "typing" }, emit: "text" });
});

test("nextRenameStep — 입력 step 객체를 고치지 않는다", () => {
  const typed = Object.freeze({ kind: "typed" });
  const sent = Object.freeze({ kind: "sent", at: 5 });
  nextRenameStep(typed, session(), "t", 100, idle);
  nextRenameStep(sent, session(), "t", 100, idle);
  nextRenameStep(sent, session(), "t", 99_999, idle);
});

test("nextRenameStep — 한 걸음씩 이어 가면 text, enter 뒤 이름이 붙으면 끝난다(전체 흐름)", () => {
  let r = nextRenameStep(undefined, session(), "t", 0, idle);
  assert.equal(r.emit, "text");
  r = nextRenameStep({ kind: "typed" }, session(), "t", 10, idle);
  assert.deepEqual(r, { step: { kind: "sent", at: 10 }, emit: "enter" });
  r = nextRenameStep(r.step, session({ name: "t" }), "t", 20, idle);
  assert.deepEqual(r, { step: undefined, emit: "none" });
});
