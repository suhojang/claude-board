// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/shortcuts/gate.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { SETTINGS_ESCAPE, SETTINGS_TOGGLE, SHORTCUT_ACTIONS, SHORTCUT_RECORD } from "./actions.ts";
import { actionAllowed } from "./gate.ts";

// [동작, 펼침, 받는가]
const TABLE = [
  [SETTINGS_TOGGLE, false, true],
  [SETTINGS_TOGGLE, true, true],
  [SETTINGS_ESCAPE, false, true],
  [SETTINGS_ESCAPE, true, true],
  ["newSession", false, false],
  ["newSession", true, true],
  ["tab1", false, false],
  ["nextTab2", true, true],
  ["rename", false, false],
  [SHORTCUT_RECORD, false, false],
  [SHORTCUT_RECORD, true, true],
  // 보드 글자 크기는 터미널과 무관해 접혀도 받는다
  ["boardBigger", false, true],
  ["boardBigger2", false, true],
  ["boardSmaller", false, true],
  ["boardReset", false, true],
];

for (const [id, expanded, want] of TABLE) {
  test(`actionAllowed: ${id} ${expanded ? "펼침" : "접힘"} -> ${want}`, () => assert.equal(actionAllowed(id, expanded), want));
}

test("접힌 동안 받는 동작은 설정 열고 닫기와 보드 글자 크기 넷뿐이다(나머지는 접히면 막힌다)", () => {
  for (const action of SHORTCUT_ACTIONS) {
    const alwaysOn = action.id.startsWith("board") && action.group === "글자 크기";
    assert.equal(actionAllowed(action.id, false), alwaysOn, action.id);
  }
});
