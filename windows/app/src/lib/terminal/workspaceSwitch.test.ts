// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/workspaceSwitch.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { isTerminalHere, switchForHeaderClick } from "./workspaceSwitch.ts";
import { shownTabOf } from "./tabLogic.ts";

const HOME = "C:\\Users\\demo";
const tabs = [
  { id: "t1", workspace: "C:\\Users\\demo\\work\\shop-api" },
  { id: "t2", workspace: "C:\\Users\\demo\\work\\shop-api" },
  { id: "t3", workspace: "C:\\Users\\demo\\work\\web" },
];

test("탭이 있고 터미널이 펼쳐 있으면 그 경로의 탭을 고르고 포커스한다", () => {
  const id = shownTabOf(tabs, "C:\\Users\\demo\\work\\web", {}, HOME);
  assert.deepEqual(switchForHeaderClick(id, true), { kind: "select", tabId: "t3", focus: true });
});

test("탭이 있고 터미널이 접혀 있으면 고르기만 하고 포커스(펼침)는 하지 않는다", () => {
  const id = shownTabOf(tabs, "C:\\Users\\demo\\work\\web", {}, HOME);
  assert.deepEqual(switchForHeaderClick(id, false), { kind: "select", tabId: "t3", focus: false });
});

test("그 경로에 탭이 없으면 펼침 여부와 상관없이 아무것도 하지 않는다", () => {
  const id = shownTabOf(tabs, "C:\\Users\\demo\\work\\infra", {}, HOME);
  assert.equal(id, undefined);
  assert.deepEqual(switchForHeaderClick(id, true), { kind: "none" });
  assert.deepEqual(switchForHeaderClick(id, false), { kind: "none" });
});

test("마지막에 고른 탭이 있으면 그 탭으로, 대소문자·슬래시 차이는 같은 경로로 본다", () => {
  const last = { "c:\\users\\demo\\work\\shop-api": "t2" };
  const id = shownTabOf(tabs, "c:/users/demo/work/shop-api/", last, HOME);
  assert.deepEqual(switchForHeaderClick(id, true), { kind: "select", tabId: "t2", focus: true });
});

test("표지: 펼친 터미널이 이 경로를 보일 때만", () => {
  const ws = "C:\\Users\\demo\\work\\web";
  assert.equal(isTerminalHere(true, ws, "C:/Users/demo/work/web/", HOME), true);
  assert.equal(isTerminalHere(false, ws, ws, HOME), false);
  assert.equal(isTerminalHere(true, ws, "C:\\Users\\demo\\work\\shop-api", HOME), false);
  assert.equal(isTerminalHere(true, "", ws, HOME), false);
});

test("표지: 홈 경로는 ~ 와 같은 키로 맞춘다", () => {
  assert.equal(isTerminalHere(true, "~", HOME, HOME), true);
  assert.equal(isTerminalHere(true, HOME, "~", HOME), true);
});
