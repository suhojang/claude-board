// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/shortcuts/showBoard.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { MOD, fromStored, keyOf, label, same, toStored } from "./keyCombo.ts";
import { SHORTCUT_ACTIONS } from "./actions.ts";
import { acceptable } from "./rules.ts";
import { BOARD_VIEW_ACTIONS, NEXT_TAB3, PREV_TAB3, SHOW_BOARD, TAB_STEP, isTerminalElement, showBoardPlan } from "./showBoard.ts";

const MINE = new Set(BOARD_VIEW_ACTIONS.map((a) => a.id));
// 통합 전(actions.ts 에 아직 없음)에도 통합 후(이미 있음)에도 같은 표가 되게, 내 행을 빼고 다시 더한다
const TABLE = [...SHORTCUT_ACTIONS.filter((a) => !MINE.has(a.id)), ...BOARD_VIEW_ACTIONS];
const byId = (id) => TABLE.find((a) => a.id === id);
const press = (code, mods) => keyOf(code, mods);

test("세 동작의 id·묶음·제목·기본 키 — mac 표(Shortcuts.swift:97·108·109)와 같은 이름", () => {
  const rows = Object.fromEntries(BOARD_VIEW_ACTIONS.map((a) => [a.id, [a.group, a.title, a.initial ? label(a.initial) : null]]));
  assert.deepEqual(rows, {
    showBoard: ["보드", "세션 보드 보기", "Ctrl+Alt+0"],
    prevTab3: ["탭으로 가기", "이전 탭 (보조 2)", null],
    nextTab3: ["탭으로 가기", "다음 탭 (보조 2)", null],
  });
  assert.equal(SHOW_BOARD, "showBoard");
  assert.equal(PREV_TAB3, "prevTab3");
  assert.equal(NEXT_TAB3, "nextTab3");
});

test("전체 표 — id 가 겹치지 않고 기본 키끼리 겹치지 않고 고를 수 있는 키다", () => {
  const ids = TABLE.map((a) => a.id);
  assert.equal(new Set(ids).size, ids.length);
  const keyed = TABLE.filter((a) => a.initial);
  for (const a of keyed) {
    assert.equal(acceptable(a.initial), "ok", `${a.id} 기본 키가 예약 키이거나 수정키 없음`);
    const same_ = keyed.filter((b) => b.id !== a.id && same(a.initial, b.initial)).map((b) => b.id);
    assert.deepEqual(same_, [], `${a.id} 기본 키 겹침`);
  }
});

test("showBoard 기본 키 Ctrl+Alt+0 는 tab1~9(Ctrl+Alt+1~9)·Ctrl+0 과 다르다", () => {
  const k = byId(SHOW_BOARD).initial;
  assert.ok(same(k, press("Digit0", MOD.ctrl | MOD.alt)));
  assert.ok(!same(k, press("Digit0", MOD.ctrl)), "Ctrl+0 은 터미널 글자 원래 크기용");
  for (let n = 1; n <= 9; n++) assert.ok(!same(k, byId(`tab${n}`).initial));
  assert.equal(acceptable(k), "ok");
  assert.equal(k.win, false);
});

test("prevTab3·nextTab3 는 기본 키가 없다 — mac 키가 prevTab2·nextTab2 와 같은 키라서", () => {
  assert.equal(byId(PREV_TAB3).initial, null);
  assert.equal(byId(NEXT_TAB3).initial, null);
  assert.ok(same(byId("prevTab2").initial, press("Tab", MOD.ctrl | MOD.shift)), "mac ⌃⇧⇥ 에 해당하는 키는 prevTab2");
  assert.ok(same(byId("nextTab2").initial, press("Tab", MOD.ctrl)), "mac ⌃⇥ 에 해당하는 키는 nextTab2");
});

test("눌린 키 → 동작 — Ctrl+Alt+0 은 showBoard, Ctrl+Tab 은 nextTab2 가 계속 받는다", () => {
  const find = (c) => TABLE.find((a) => same(a.initial, c))?.id ?? null;
  assert.equal(find(press("Digit0", MOD.ctrl | MOD.alt)), "showBoard");
  assert.equal(find(press("Tab", MOD.ctrl)), "nextTab2");
  assert.equal(find(press("Tab", MOD.ctrl | MOD.shift)), "prevTab2");
});

test("사용자가 prevTab3·nextTab3 에 키를 묶어도 겹치지 않는 키면 충돌이 없다", () => {
  const bound = [...TABLE.filter((a) => a.id !== PREV_TAB3), { id: PREV_TAB3, initial: press("BracketLeft", MOD.ctrl | MOD.alt) }];
  const k = bound.find((a) => a.id === PREV_TAB3).initial;
  assert.equal(acceptable(k), "ok");
  assert.deepEqual(bound.filter((a) => a.id !== PREV_TAB3 && same(a.initial, k)), []);
});

test("저장 왕복 — Ctrl+Alt+0 은 [Digit0, 3] 으로 저장되고 같은 키로 돌아온다", () => {
  const k = byId(SHOW_BOARD).initial;
  assert.deepEqual(toStored(k), ["Digit0", MOD.ctrl | MOD.alt]);
  assert.ok(same(fromStored(toStored(k)), k));
});

test("라벨 — 숫자 0 은 Digit0 이 아니라 0 으로 보인다", () => {
  assert.equal(label(byId(SHOW_BOARD).initial), "Ctrl+Alt+0");
});

test("TAB_STEP — 이전 셋은 -1, 다음 셋은 +1, 그 밖은 없다", () => {
  for (const id of ["prevTab", "prevTab2", "prevTab3"]) assert.equal(TAB_STEP[id], -1, id);
  for (const id of ["nextTab", "nextTab2", "nextTab3"]) assert.equal(TAB_STEP[id], 1, id);
  assert.equal(Object.keys(TAB_STEP).length, 6);
  assert.equal(TAB_STEP.showBoard, undefined);
  assert.equal(TAB_STEP.prevWorkspace, undefined);
});

test("showBoardPlan — 창은 항상 앞으로, 터미널에 포커스가 있을 때만 포커스를 뺀다", () => {
  assert.deepEqual(showBoardPlan({ terminalFocused: true }), { showWindow: true, blurTerminal: true });
  assert.deepEqual(showBoardPlan({ terminalFocused: false }), { showWindow: true, blurTerminal: false });
});

test("isTerminalElement — .xterm 안의 요소만 터미널이다", () => {
  assert.equal(isTerminalElement({ closest: (s) => (s === ".xterm" ? {} : null) }), true);
  assert.equal(isTerminalElement({ closest: () => null }), false);
  assert.equal(isTerminalElement({}), false);
  assert.equal(isTerminalElement(null), false);
  assert.equal(isTerminalElement(undefined), false);
});

test("통합 뒤 —actions.ts 에 이 행들이 있으면 여기와 똑같다", () => {
  for (const mine of BOARD_VIEW_ACTIONS) {
    const real = SHORTCUT_ACTIONS.find((a) => a.id === mine.id);
    if (real) assert.deepEqual(real, mine, `${mine.id} 행이 showBoard.ts 와 다르다`);
  }
});
