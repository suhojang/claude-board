// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/stripScroll.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { wheelStep, scrollTargetClamped } from "./stripScroll.ts";

test("휠 — 세로로만 돌면 그만큼 가로로 직접 민다(기본 동작은 막는다)", () => {
  assert.deepEqual(wheelStep(0, 120, false), { add: 120, prevent: true });
  assert.deepEqual(wheelStep(0, -60, false), { add: -60, prevent: true });
});

test("Shift+휠 — 세로 값이 오면 우리가 밀고 막아서 기본 동작과 겹쳐 두 배로 밀리지 않는다", () => {
  assert.deepEqual(wheelStep(0, 120, true), { add: 120, prevent: true });
});

test("트랙패드 가로 밀기·Shift+휠이 가로 값으로 오면 브라우저에 맡긴다", () => {
  assert.deepEqual(wheelStep(40, 0, false), { add: 0, prevent: false });
  assert.deepEqual(wheelStep(40, 5, true), { add: 0, prevent: false });
});

test("휠 값이 0 이면 아무것도 하지 않는다", () => {
  assert.deepEqual(wheelStep(0, 0, false), { add: 0, prevent: false });
});

test("고른 경로 보이게 밀기 — 이미 보이면 그대로(여백 포함)", () => {
  assert.equal(scrollTargetClamped(108, 208, 50, 300, 1000, 8), 50);
});

test("고른 경로 보이게 밀기 — 왼쪽 밖이면 여백까지 보이게, 0 아래로는 가지 않는다", () => {
  assert.equal(scrollTargetClamped(8, 100, 200, 300, 1000, 8), 0); // 맨 앞 경로
  assert.equal(scrollTargetClamped(120, 220, 200, 300, 1000, 8), 112);
});

test("고른 경로 보이게 밀기 — 오른쪽 밖이면 여백까지 보이게, 끝을 넘지 않는다", () => {
  assert.equal(scrollTargetClamped(500, 600, 0, 300, 1000, 8), 308);
  assert.equal(scrollTargetClamped(900, 992, 0, 300, 1000, 8), 700); // 끝 = 1000 - 300
});

test("칸이 안쪽보다 넓으면(안 넘침) 0", () => {
  assert.equal(scrollTargetClamped(8, 100, 0, 300, 250, 8), 0);
});
