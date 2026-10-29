// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/search.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { search, matchedSections } from "./search.ts";

const items = [
  { id: "pin", section: "general", title: "항상 위에 고정", keywords: ["플로팅", "Pin"] },
  { id: "login", section: "general", title: "로그인할 때 열기", keywords: ["startup", "자동 실행"] },
  { id: "opacity", section: "window", title: "투명도", keywords: ["투명", "Opacity"] },
  { id: "pos", section: "notify", title: "알림 위치", keywords: ["Position"] },
  { id: "theme", section: "terminal", title: "테마", keywords: [] },
];
const ids = (list) => list.map((i) => i.id);

test("제목에서 찾는다", () => {
  assert.deepEqual(ids(search(items, "투명도")), ["opacity"]);
  assert.deepEqual(ids(search(items, "위")), ["pin", "pos"]);
});

test("키워드에서 찾는다(제목에 없는 낱말)", () => {
  assert.deepEqual(ids(search(items, "플로팅")), ["pin"]);
  assert.deepEqual(ids(search(items, "자동")), ["login"]);
});

test("대소문자를 가리지 않는다", () => {
  assert.deepEqual(ids(search(items, "pin")), ["pin"]);
  assert.deepEqual(ids(search(items, "OPACITY")), ["opacity"]);
  assert.deepEqual(ids(search(items, "StartUp")), ["login"]);
});

test("여러 묶음에 걸쳐 찾고 items 의 순서를 지킨다", () => {
  const found = search(items, "p"); // Pin · startup · Opacity · Position
  assert.deepEqual(ids(found), ["pin", "login", "opacity", "pos"]);
  assert.deepEqual([...new Set(found.map((i) => i.section))], ["general", "window", "notify"]);
});

test("앞뒤 공백은 무시하고 빈 검색어는 빈 목록이다", () => {
  assert.deepEqual(ids(search(items, "  투명도  ")), ["opacity"]);
  assert.deepEqual(search(items, ""), []);
  assert.deepEqual(search(items, "   "), []);
});

test("맞는 것이 없으면 빈 목록이고, 원본 배열을 바꾸지 않는다", () => {
  const before = items.length;
  assert.deepEqual(search(items, "zzz"), []);
  assert.equal(items.length, before);
});

test("matchedSections — 맞는 항목이 든 묶음만, 묶음 순서 그대로", () => {
  const order = ["general", "window", "notify", "terminal", "board", "account", "shortcuts", "update"];
  assert.deepEqual(matchedSections(items, "p", order), ["general", "window", "notify"]);
  assert.deepEqual(matchedSections(items, "", order), []);
  assert.deepEqual(matchedSections(items, "zzz", order), []);
});
