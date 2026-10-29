// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/screenPick.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import { test } from "node:test";
import assert from "node:assert/strict";
import { pickPosition, SPOTS, SPOT_ORDER } from "./screenPick.ts";

const PIC = { width: 132, height: 82 };
const at = (p, size = PIC) => ({ x: SPOTS[p].x * size.width, y: SPOTS[p].y * size.height });

test("다섯 자리의 가운데를 누르면 그 자리", () => {
  for (const p of SPOT_ORDER) assert.equal(pickPosition(PIC, at(p)), p, p);
});

test("그림의 네 모서리와 한가운데", () => {
  assert.equal(pickPosition(PIC, { x: 0, y: 0 }), "top-left");
  assert.equal(pickPosition(PIC, { x: 132, y: 0 }), "top-right");
  assert.equal(pickPosition(PIC, { x: 0, y: 82 }), "bottom-left");
  assert.equal(pickPosition(PIC, { x: 132, y: 82 }), "bottom-right");
  assert.equal(pickPosition(PIC, { x: 66, y: 41 }), "center");
});

test("위·아래 가운데 줄은 가운데로 간다(가운데 자리가 가장 가깝다)", () => {
  assert.equal(pickPosition(PIC, { x: 66, y: 0 }), "center");
  assert.equal(pickPosition(PIC, { x: 66, y: 82 }), "center");
});

test("그림 크기를 키워도 같은 자리를 고른다", () => {
  const big = { width: 264, height: 164 };
  for (const p of SPOT_ORDER) assert.equal(pickPosition(big, at(p, big)), p, p);
  assert.equal(pickPosition(big, { x: 10, y: 10 }), "top-left");
  assert.equal(pickPosition(big, { x: 250, y: 150 }), "bottom-right");
});

test("그림 밖을 눌러도 가장 가까운 자리", () => {
  assert.equal(pickPosition(PIC, { x: -50, y: -50 }), "top-left");
  assert.equal(pickPosition(PIC, { x: 500, y: 500 }), "bottom-right");
  assert.equal(pickPosition(PIC, { x: 500, y: -20 }), "top-right");
});

test("크기가 0이거나 숫자가 아니면 가운데(기본)", () => {
  assert.equal(pickPosition({ width: 0, height: 0 }, { x: 5, y: 5 }), "center");
  assert.equal(pickPosition({ width: 132, height: 0 }, { x: 5, y: 5 }), "center");
  assert.equal(pickPosition(PIC, { x: Number.NaN, y: 3 }), "center");
  assert.equal(pickPosition({ width: Number.NaN, height: 82 }, { x: 5, y: 5 }), "center");
});

test("자리 좌표는 그림 안의 비율이다", () => {
  for (const p of SPOT_ORDER) {
    assert.ok(SPOTS[p].x > 0 && SPOTS[p].x < 1 && SPOTS[p].y > 0 && SPOTS[p].y < 1, p);
  }
  assert.equal(SPOT_ORDER.length, 5);
  assert.equal(new Set(SPOT_ORDER).size, 5);
});
