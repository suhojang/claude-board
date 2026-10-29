// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/shortcuts/keyCombo.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { MOD, fromEvent, fromStored, isModifierCode, keyOf, label, same, toStored } from "./keyCombo.ts";

const ev = (code, o = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...o });

test("키 이벤트는 코드와 수정키 넷으로 읽는다 (Meta = Win)", () => {
  assert.deepEqual(fromEvent(ev("KeyB", { ctrlKey: true, altKey: true })), { code: "KeyB", ctrl: true, alt: true, shift: false, win: false });
  assert.deepEqual(fromEvent(ev("Digit1", { shiftKey: true, metaKey: true })), { code: "Digit1", ctrl: false, alt: false, shift: true, win: true });
});

test("저장 모양 [코드, 비트] — Ctrl=1 Alt=2 Shift=4 Win=8", () => {
  assert.deepEqual(toStored(keyOf("KeyN", MOD.ctrl | MOD.alt)), ["KeyN", 3]);
  assert.deepEqual(toStored(keyOf("F5", 0)), ["F5", 0]);
  assert.deepEqual(toStored(keyOf("Tab", MOD.ctrl | MOD.shift | MOD.win)), ["Tab", 13]);
});

test("저장 모양은 되읽으면 같은 키다", () => {
  for (const mods of [0, 1, 2, 3, 4, 5, 6, 7, 8, 15]) {
    const c = keyOf("KeyQ", mods);
    assert.ok(same(fromStored(toStored(c)), c), `mods ${mods}`);
  }
});

test("깨진 저장 값은 null — 비움([])·글자 아닌 코드·정수 아닌 비트·빈 코드", () => {
  assert.equal(fromStored([]), null);
  assert.equal(fromStored(["KeyA"]), null);
  assert.equal(fromStored([12, 3]), null);
  assert.equal(fromStored(["KeyA", "3"]), null);
  assert.equal(fromStored(["KeyA", 1.5]), null);
  assert.equal(fromStored(["KeyA", -1]), null);
  assert.equal(fromStored(["", 1]), null);
  assert.equal(fromStored(["KeyA", 1, 2]), null);
  assert.equal(fromStored(null), null);
});

test("같은 키 판정 — null 끼리는 같고 null 과 키는 다르다", () => {
  assert.equal(same(null, null), true);
  assert.equal(same(null, keyOf("KeyA", 1)), false);
  assert.equal(same(keyOf("KeyA", 1), keyOf("KeyA", 1)), true);
  assert.equal(same(keyOf("KeyA", 1), keyOf("KeyA", 3)), false);
  assert.equal(same(keyOf("KeyA", 1), keyOf("KeyB", 1)), false);
});

test("수정키만 눌린 keydown 은 수정키 코드다", () => {
  for (const c of ["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight"]) {
    assert.equal(isModifierCode(c), true, c);
  }
  for (const c of ["KeyA", "Tab", "Escape", "F4"]) assert.equal(isModifierCode(c), false, c);
});

test("화면 글자 — Ctrl Alt Shift Win 순서, 키 이름은 읽기 쉬운 글자", () => {
  assert.equal(label(keyOf("KeyB", MOD.ctrl | MOD.alt)), "Ctrl+Alt+B");
  assert.equal(label(keyOf("KeyR", MOD.ctrl | MOD.shift)), "Ctrl+Shift+R");
  assert.equal(label(keyOf("Digit1", MOD.ctrl | MOD.alt)), "Ctrl+Alt+1");
  assert.equal(label(keyOf("ArrowLeft", MOD.ctrl | MOD.alt)), "Ctrl+Alt+←");
  assert.equal(label(keyOf("ArrowUp", MOD.alt | MOD.shift)), "Alt+Shift+↑");
  assert.equal(label(keyOf("Tab", MOD.ctrl)), "Ctrl+Tab");
  assert.equal(label(keyOf("Comma", MOD.ctrl)), "Ctrl+,");
  assert.equal(label(keyOf("Minus", MOD.alt | MOD.shift)), "Alt+Shift+-");
  assert.equal(label(keyOf("Equal", MOD.alt | MOD.shift)), "Alt+Shift+=");
  assert.equal(label(keyOf("BracketLeft", MOD.ctrl | MOD.shift)), "Ctrl+Shift+[");
  assert.equal(label(keyOf("Escape", MOD.ctrl | MOD.shift)), "Ctrl+Shift+Esc");
  assert.equal(label(keyOf("PageUp", MOD.ctrl)), "Ctrl+PgUp");
  assert.equal(label(keyOf("F4", MOD.alt)), "Alt+F4");
  assert.equal(label(keyOf("F11", 0)), "F11");
  assert.equal(label(keyOf("Space", MOD.alt)), "Alt+Space");
  assert.equal(label(keyOf("KeyX", MOD.win)), "Win+X");
  assert.equal(label(keyOf("Numpad5", MOD.ctrl)), "Ctrl+Num 5");
});

test("모르는 코드는 코드 이름 그대로", () => {
  assert.equal(label(keyOf("IntlBackslash", MOD.ctrl)), "Ctrl+IntlBackslash");
});
