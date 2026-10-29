// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/paneLayout.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  boardLaunchWidth,
  paneDividerWidth,
  boardMinWidth,
  terminalMinWidth,
  clampBoardWidth,
  maxBoardWidth,
  scrollTargetFor,
  tabStripLeast,
  tabStripWidth,
} from "./paneLayout.ts";

test("보드 칸 최대 폭 — 창 폭에서 경계와 터미널 최소 폭을 뺀다", () => {
  assert.equal(maxBoardWidth(1400), 1400 - paneDividerWidth - terminalMinWidth);
  assert.equal(maxBoardWidth(boardMinWidth + paneDividerWidth + terminalMinWidth), boardMinWidth);
});

test("창이 아주 좁아도 보드 칸 최대 폭은 최소 폭 아래로 내려가지 않는다", () => {
  assert.equal(maxBoardWidth(700), boardMinWidth);
  assert.equal(maxBoardWidth(0), boardMinWidth);
});

test("보드 칸 폭 — 끄는 값 > 저장값 > 기본값", () => {
  assert.equal(clampBoardWidth(500, 400, 1600), 500);
  assert.equal(clampBoardWidth(null, 400, 1600), 400);
  assert.equal(clampBoardWidth(null, null, 1600), boardLaunchWidth);
  assert.equal(clampBoardWidth(null, undefined, 1600), boardLaunchWidth);
  assert.equal(clampBoardWidth(0, 400, 1600), boardMinWidth); // 0 도 끄는 값이다(?? 로 고르므로 저장값으로 넘어가지 않는다)
});

test("보드 칸 폭 — 최소 360 아래·최대 위는 잘린다", () => {
  assert.equal(clampBoardWidth(100, null, 1600), boardMinWidth);
  assert.equal(clampBoardWidth(null, -5, 1600), boardMinWidth);
  assert.equal(clampBoardWidth(5000, null, 1600), 1600 - paneDividerWidth - terminalMinWidth);
  assert.equal(clampBoardWidth(null, 5000, 1000), 1000 - paneDividerWidth - terminalMinWidth);
});

test("보드 칸 폭 — 좁은 창에서는 저장값이 얼마든 최소 폭", () => {
  assert.equal(clampBoardWidth(null, 454, 700), boardMinWidth);
  assert.equal(clampBoardWidth(900, 454, 600), boardMinWidth);
});

test("보드 칸 폭 — 창을 넓히면 저장된 폭이 다시 살아난다(저장값은 건드리지 않고 보이는 값만 자른다)", () => {
  assert.equal(clampBoardWidth(null, 700, 1000), 1000 - paneDividerWidth - terminalMinWidth); // 512
  assert.equal(clampBoardWidth(null, 700, 1600), 700);
});

test("탭 칸 폭 — 탭이 모두 들어가면 탭마다 250 + 간격 2", () => {
  assert.equal(tabStripWidth(2000, 100, 3), 3 * 252 - 2);
  assert.equal(tabStripWidth(2000, 100, 1), 250);
});

test("탭 칸 폭 — 모자라면 + 단추를 뺀 남는 폭 전부", () => {
  assert.equal(tabStripWidth(600, 100, 10), 600 - 16 - 100);
});

test("탭 칸 폭 — 탭이 없거나 줄이 아직 안 재졌으면(0) 음수가 되지 않는다", () => {
  assert.equal(tabStripWidth(0, 0, 3), 0);
  assert.equal(tabStripWidth(50, 100, 3), 0);
  assert.equal(tabStripWidth(2000, 100, 0), 0);
});

test("탭 최소 안쪽 폭 — 120 + 간격, 탭이 없으면 0", () => {
  assert.equal(tabStripLeast(0), 0);
  assert.equal(tabStripLeast(1), 120);
  assert.equal(tabStripLeast(5), 5 * 122 - 2);
});

test("탭이 120 씩도 안 들어가면 안쪽 폭이 칸보다 커서 밀린다, 들어가면 같다", () => {
  const overflow = (bar, n) => tabStripLeast(n) > tabStripWidth(bar, 100, n);
  assert.equal(overflow(500, 10), true);
  assert.equal(overflow(2000, 3), false);
});

test("고른 항목 보이게 밀기 — 이미 보이면 그대로", () => {
  assert.equal(scrollTargetFor(100, 200, 50, 300), 50);
  assert.equal(scrollTargetFor(50, 350, 50, 300), 50); // 양 끝이 칸 가장자리와 딱 맞음
});

test("고른 항목 보이게 밀기 — 왼쪽 밖이면 왼쪽 끝을, 오른쪽 밖이면 오른쪽 끝을 맞춘다(가장 가까운 만큼만)", () => {
  assert.equal(scrollTargetFor(20, 140, 100, 300), 20);
  assert.equal(scrollTargetFor(380, 500, 100, 300), 200); // 500 - 300
});
