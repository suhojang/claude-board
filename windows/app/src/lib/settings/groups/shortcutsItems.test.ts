// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/groups/shortcutsItems.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { SHORTCUT_ACTIONS } from "../../shortcuts/actions.ts";
import { SHORTCUTS_ITEMS } from "./shortcutsItems.ts";

test("동작마다 항목 한 줄 + 모두 기본값으로", () => {
  assert.equal(SHORTCUTS_ITEMS.length, SHORTCUT_ACTIONS.length + 1);
  assert.ok(SHORTCUTS_ITEMS.every((item) => item.section === "shortcuts"));
  assert.equal(new Set(SHORTCUTS_ITEMS.map((item) => item.id)).size, SHORTCUTS_ITEMS.length);
});

test("항목 제목은 동작 제목, 검색어에 기본 키와 묶음 이름이 든다", () => {
  const item = SHORTCUTS_ITEMS.find((i) => i.id === "key.newSession");
  assert.equal(item.title, "새 세션…");
  assert.ok(item.keywords.includes("Ctrl+T"));
  assert.ok(item.keywords.includes("터미널"));
  const empty = SHORTCUTS_ITEMS.find((i) => i.id === "key.pin");
  assert.ok(!empty.keywords.some((k) => k.includes("+")), "기본 키가 없는 동작은 키 글자가 없다");
});
