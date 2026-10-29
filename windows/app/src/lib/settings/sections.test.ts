// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/sections.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { SETTINGS_SECTIONS, sectionTitle } from "./sections.ts";

test("묶음은 여덟 개이고 mac 순서(일반·창·알림·터미널·보드·계정·단축키·업데이트)다", () => {
  assert.deepEqual(
    SETTINGS_SECTIONS.map((s) => s.id),
    ["general", "window", "notify", "terminal", "board", "account", "shortcuts", "update"],
  );
  assert.deepEqual(
    SETTINGS_SECTIONS.map((s) => s.title),
    ["일반", "창", "알림", "터미널", "보드", "계정", "단축키", "업데이트"],
  );
});

test("묶음마다 한 줄 설명이 있다", () => {
  for (const s of SETTINGS_SECTIONS) assert.ok(s.subtitle.length > 0, s.id);
});

test("sectionTitle — id 로 이름을 찾는다", () => {
  assert.equal(sectionTitle("shortcuts"), "단축키");
});
