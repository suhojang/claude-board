// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/shortcuts/dispatch.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { SETTINGS_ESCAPE, SETTINGS_TOGGLE, SHORTCUT_RECORD } from "./actions.ts";
import { dispatch, ownsEscape } from "./dispatch.ts";

const press = (code, o = {}) => {
  const { altGraph = false, ...mods } = o;
  return { code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, getModifierState: (m) => m === "AltGraph" && altGraph, ...mods };
};
const idle = { recording: null, composing: false, modalOpen: false, settingsShown: false };
const ctrl = { ctrlKey: true };
const ctrlAlt = { ctrlKey: true, altKey: true };

test("기본 키 — 지금 Windows 에서 쓰는 키가 같은 동작으로 간다", () => {
  const table = [
    ["KeyT", ctrl, "newSession"],
    ["KeyR", { ctrlKey: true, shiftKey: true }, "rename"],
    ["Digit1", ctrlAlt, "tab1"],
    ["Digit9", ctrlAlt, "tab9"],
    ["ArrowLeft", ctrlAlt, "prevTab"],
    ["ArrowRight", ctrlAlt, "nextTab"],
    ["ArrowUp", ctrlAlt, "prevWorkspace"],
    ["ArrowDown", ctrlAlt, "nextWorkspace"],
    ["KeyB", ctrlAlt, "snap"],
    ["KeyT", ctrlAlt, "terminal"],
    ["Tab", ctrl, "nextTab2"],
    ["Tab", { ctrlKey: true, shiftKey: true }, "prevTab2"],
  ];
  for (const [code, mods, id] of table) assert.equal(dispatch(press(code, mods), idle), id, `${code} ${JSON.stringify(mods)}`);
});

test("표에 없는 키는 null — 키를 그대로 둔다", () => {
  assert.equal(dispatch(press("KeyA"), idle), null);
  assert.equal(dispatch(press("KeyC", ctrl), idle), null, "Ctrl+C 는 셸로");
  assert.equal(dispatch(press("KeyT", { ctrlKey: true, shiftKey: true }), idle), null);
  assert.equal(dispatch(press("ControlLeft", ctrl), idle), null, "수정키만 눌린 keydown");
});

test("바꾼 키 — 새 키가 동작하고 옛 키는 풀린다", () => {
  const overrides = { newSession: ["KeyN", 3] };
  assert.equal(dispatch(press("KeyN", ctrlAlt), { ...idle, overrides }), "newSession");
  assert.equal(dispatch(press("KeyT", ctrl), { ...idle, overrides }), null);
});

test("비운 동작은 어느 키에도 안 걸린다", () => {
  assert.equal(dispatch(press("KeyT", ctrl), { ...idle, overrides: { newSession: [] } }), null);
});

test("입력기 조합 중이면 null", () => {
  assert.equal(dispatch(press("KeyT", ctrl), { ...idle, composing: true }), null);
  assert.equal(dispatch(press("KeyA"), { ...idle, composing: true, recording: "rename" }), null);
});

test("AltGraph(AltGr 이 Ctrl+Alt 로 오는 것)면 null", () => {
  assert.equal(dispatch(press("Digit2", { ...ctrlAlt, altGraph: true }), idle), null);
  assert.equal(dispatch(press("KeyB", { ...ctrlAlt, altGraph: true }), idle), null);
  assert.equal(dispatch(press("KeyQ", { ...ctrlAlt, altGraph: true }), { ...idle, recording: "rename" }), null);
});

test("키를 받는 중이면 어떤 키든 받는 쪽으로 — 수정키만 눌린 것도 (받는 쪽이 건너뛴다)", () => {
  const recording = { ...idle, recording: "rename" };
  assert.equal(dispatch(press("KeyT", ctrl), recording), SHORTCUT_RECORD);
  assert.equal(dispatch(press("Escape"), recording), SHORTCUT_RECORD);
  assert.equal(dispatch(press("Escape"), { ...recording, settingsShown: true }), SHORTCUT_RECORD, "받기 취소가 설정 닫기보다 먼저");
  assert.equal(dispatch(press("Comma", ctrl), recording), SHORTCUT_RECORD);
  assert.equal(dispatch(press("ControlLeft", ctrl), recording), SHORTCUT_RECORD);
});

test("확인 창·새 세션 창이 떠 있으면 null", () => {
  assert.equal(dispatch(press("KeyT", ctrl), { ...idle, modalOpen: true }), null);
  assert.equal(dispatch(press("Comma", ctrl), { ...idle, modalOpen: true }), null);
  assert.equal(dispatch(press("Escape"), { ...idle, modalOpen: true, settingsShown: true }), null);
});

test("설정 화면이 떠 있을 때 Esc 는 settings.escape, 아니면 null", () => {
  assert.equal(dispatch(press("Escape"), { ...idle, settingsShown: true }), SETTINGS_ESCAPE);
  assert.equal(dispatch(press("Escape"), idle), null);
  assert.equal(dispatch(press("Escape", { shiftKey: true }), { ...idle, settingsShown: true }), null, "수정키가 붙은 Esc 는 아니다");
});

test("Ctrl+, 는 설정 화면 열고 닫기 — 떠 있든 아니든", () => {
  assert.equal(dispatch(press("Comma", ctrl), idle), SETTINGS_TOGGLE);
  assert.equal(dispatch(press("Comma", ctrl), { ...idle, settingsShown: true }), SETTINGS_TOGGLE);
  assert.equal(dispatch(press("Comma", ctrlAlt), idle), null);
});

test("설정 키는 바꾼 표로 옮겨도 그대로다", () => {
  assert.equal(dispatch(press("Comma", ctrl), { ...idle, overrides: { newSession: ["Comma", 1] } }), SETTINGS_TOGGLE);
});

test("Esc 가 눌린 칸이 스스로 Esc 를 처리하면(escapeOwned) settings.escape 로 가져가지 않는다", () => {
  assert.equal(dispatch(press("Escape"), { ...idle, settingsShown: true, escapeOwned: true }), null);
  assert.equal(dispatch(press("Escape"), { ...idle, settingsShown: true, escapeOwned: false }), SETTINGS_ESCAPE);
  assert.equal(dispatch(press("Comma", ctrl), { ...idle, settingsShown: true, escapeOwned: true }), SETTINGS_TOGGLE, "Esc 만 해당한다");
});

test("ownsEscape — data-own-escape 칸 안에서 눌린 키만 true", () => {
  const inside = { closest: (selector) => (selector === "[data-own-escape]" ? {} : null) };
  const outside = { closest: () => null };
  assert.equal(ownsEscape(inside), true);
  assert.equal(ownsEscape(outside), false);
  assert.equal(ownsEscape(null), false);
  assert.equal(ownsEscape(undefined), false);
  assert.equal(ownsEscape({}), false, "closest 가 없는 대상(window·document)");
});
