// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/shortcuts/rules.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { MOD, keyOf, label, same } from "./keyCombo.ts";
import { SHORTCUT_ACTIONS, SETTINGS_KEY, actionOf } from "./actions.ts";
import { RESERVED, acceptable, combo, conflicts, find, set } from "./rules.ts";

const CA = MOD.ctrl | MOD.alt;

test("기본 키 표 — 지금 Windows 에서 쓰는 키가 그대로다", () => {
  const table = Object.fromEntries(SHORTCUT_ACTIONS.map((a) => [a.id, a.initial ? label(a.initial) : null]));
  assert.equal(table.newSession, "Ctrl+T");
  assert.equal(table.rename, "Ctrl+Shift+R");
  assert.equal(table.snap, "Ctrl+Alt+B");
  assert.equal(table.terminal, "Ctrl+Alt+T");
  for (let n = 1; n <= 9; n++) assert.equal(table[`tab${n}`], `Ctrl+Alt+${n}`);
  assert.equal(table.prevTab, "Ctrl+Alt+←");
  assert.equal(table.nextTab, "Ctrl+Alt+→");
  assert.equal(table.prevWorkspace, "Ctrl+Alt+↑");
  assert.equal(table.nextWorkspace, "Ctrl+Alt+↓");
  assert.equal(table.prevTab2, "Ctrl+Shift+Tab");
  assert.equal(table.nextTab2, "Ctrl+Tab");
  assert.equal(table.pin, null);
});

test("표의 id 는 겹치지 않고, 기본 키끼리도 겹치지 않으며, 고를 수 있는 키다", () => {
  const ids = SHORTCUT_ACTIONS.map((a) => a.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const a of SHORTCUT_ACTIONS) {
    assert.deepEqual(conflicts(a.id, {}), [], `${a.id} 기본 키 겹침`);
    if (a.initial) assert.equal(acceptable(a.initial), "ok", `${a.id} 기본 키가 못 고르는 키`);
  }
});

test("설정 화면 키(Ctrl+,)는 표의 어느 동작에도 없다", () => {
  assert.equal(label(SETTINGS_KEY), "Ctrl+,");
  assert.equal(find(SETTINGS_KEY, {}), null);
});

test("combo — 바꾼 것이 있으면 그것, 비움([])이면 null, 없으면 기본값", () => {
  assert.ok(same(combo("newSession", {}), keyOf("KeyT", MOD.ctrl)));
  assert.ok(same(combo("newSession", { newSession: ["KeyN", 3] }), keyOf("KeyN", CA)));
  assert.equal(combo("newSession", { newSession: [] }), null);
  assert.equal(combo("pin", {}), null);
  assert.equal(combo("모르는동작", {}), null);
});

test("combo — 깨진 저장 값은 비움처럼 null", () => {
  assert.equal(combo("newSession", { newSession: ["KeyN"] }), null);
});

test("set — 기본값으로 돌아오면 항목을 지운다", () => {
  const changed = set({}, "newSession", keyOf("KeyN", CA));
  assert.deepEqual(changed, { newSession: ["KeyN", 3] });
  const back = set(changed, "newSession", actionOf("newSession").initial);
  assert.deepEqual(back, {});
});

test("set — null 은 비움([])으로, 원래 비어 있는 동작을 비우면 항목이 안 남는다", () => {
  assert.deepEqual(set({}, "rename", null), { rename: [] });
  assert.deepEqual(set({}, "pin", null), {});
  assert.deepEqual(set({ pin: ["KeyP", 3] }, "pin", null), {});
});

test("set — 받은 목록을 고치지 않고 새 객체를 돌려준다", () => {
  const before = { rename: [] };
  const after = set(before, "newSession", keyOf("KeyN", CA));
  assert.notEqual(after, before);
  assert.deepEqual(before, { rename: [] });
  assert.deepEqual(after, { rename: [], newSession: ["KeyN", 3] });
});

test("저장 모양 {id: [code, 비트]} 는 set → combo 로 되돌아온다", () => {
  const stored = set(set({}, "newSession", keyOf("KeyN", CA)), "rename", null);
  const roundTrip = JSON.parse(JSON.stringify(stored));
  assert.ok(same(combo("newSession", roundTrip), keyOf("KeyN", CA)));
  assert.equal(combo("rename", roundTrip), null);
});

test("겹침 — 양쪽에서 찾고 한쪽을 비우면 사라진다", () => {
  const overrides = set({}, "rename", keyOf("KeyT", MOD.ctrl)); // newSession 의 기본 키
  assert.deepEqual(conflicts("rename", overrides), ["newSession"]);
  assert.deepEqual(conflicts("newSession", overrides), ["rename"]);
  const cleared = set(overrides, "newSession", null);
  assert.deepEqual(conflicts("rename", cleared), []);
  assert.deepEqual(conflicts("newSession", cleared), []);
});

test("겹침 — 비운 동작은 아무와도 겹치지 않는다", () => {
  assert.deepEqual(conflicts("pin", {}), []);
  assert.deepEqual(conflicts("pin", { rename: [] }), []);
  assert.deepEqual(conflicts("rename", { rename: [], pin: [] }), []);
});

test("find — 표 순서에서 먼저 나오는 동작, 비운 동작은 건너뛴다", () => {
  assert.equal(find(keyOf("KeyT", MOD.ctrl), {}), "newSession");
  assert.equal(find(keyOf("KeyT", MOD.ctrl), { newSession: [] }), null);
  assert.equal(find(keyOf("KeyT", MOD.ctrl), { rename: ["KeyT", 1] }), "newSession", "겹치면 표 순서에서 앞선 동작");
  assert.equal(find(keyOf("KeyT", MOD.ctrl), { newSession: [], rename: ["KeyT", 1] }), "rename");
  assert.equal(find(keyOf("KeyZ", 0), {}), null);
});

test("고를 수 없는 키 — 판단 목록과 Win 조합", () => {
  const refused = [
    keyOf("F4", MOD.alt), keyOf("Tab", MOD.alt), keyOf("Space", MOD.alt), keyOf("Escape", MOD.alt),
    keyOf("Escape", MOD.ctrl), keyOf("Escape", MOD.ctrl | MOD.shift),
    keyOf("KeyC", MOD.ctrl), keyOf("KeyV", MOD.ctrl), keyOf("KeyX", MOD.ctrl), keyOf("KeyA", MOD.ctrl), keyOf("KeyZ", MOD.ctrl), keyOf("KeyY", MOD.ctrl),
    keyOf("Comma", MOD.ctrl),
    keyOf("KeyD", MOD.win), keyOf("KeyL", MOD.win | MOD.shift), keyOf("F5", MOD.win | MOD.ctrl),
  ];
  for (const c of refused) assert.equal(acceptable(c), "reserved", label(c));
  assert.ok(RESERVED.length >= 13);
});

test("고를 수 없는 키는 정확히 같은 조합만 — Ctrl+Shift+C 는 된다", () => {
  assert.equal(acceptable(keyOf("KeyC", MOD.ctrl | MOD.shift)), "ok");
  assert.equal(acceptable(keyOf("F4", MOD.ctrl)), "ok");
});

test("Shift 만 든 키·수정키 없는 키는 거절, F1~F12 는 수정키 없이도 된다", () => {
  assert.equal(acceptable(keyOf("KeyA", 0)), "needsModifier");
  assert.equal(acceptable(keyOf("KeyA", MOD.shift)), "needsModifier");
  assert.equal(acceptable(keyOf("Tab", MOD.shift)), "needsModifier");
  assert.equal(acceptable(keyOf("F1", 0)), "ok");
  assert.equal(acceptable(keyOf("F12", MOD.shift)), "ok");
  assert.equal(acceptable(keyOf("F13", 0)), "needsModifier");
  assert.equal(acceptable(keyOf("KeyA", MOD.alt)), "ok");
  assert.equal(acceptable(keyOf("KeyN", MOD.ctrl)), "ok");
});
