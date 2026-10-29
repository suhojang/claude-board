// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/groups/accountItems.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import { ACCOUNT_ITEMS } from "./accountItems.ts";

test("계정 묶음 항목은 모두 account 묶음이고 id 가 겹치지 않는다", () => {
  assert.ok(ACCOUNT_ITEMS.length >= 2);
  assert.ok(ACCOUNT_ITEMS.every((item) => item.section === "account"));
  const ids = ACCOUNT_ITEMS.map((item) => item.id);
  assert.equal(new Set(ids).size, ids.length);
  assert.ok(ids.includes("accounts") && ids.includes("autoSwitch"));
});

test("검색어에 로그인·전환·추가·지우기가 있어 어느 말로도 찾는다", () => {
  const accounts = ACCOUNT_ITEMS.find((item) => item.id === "accounts");
  for (const word of ["로그인", "전환", "추가", "지우기"]) assert.ok(accounts.keywords.includes(word), word);
  assert.ok(ACCOUNT_ITEMS.every((item) => item.title.length > 0));
});
