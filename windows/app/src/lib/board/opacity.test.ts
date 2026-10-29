// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/board/opacity.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import { test } from "node:test";
import assert from "node:assert/strict";
import { boardOpaque, coversWorkArea } from "./opacity.ts";

// 3.1.9 진리표. 입력은 (covering, previewing, pointerInside). 옛 규칙(3.1.8 까지)은
//   expanded || (!previewing && (inside || focused))
// 였다. 바뀐 행:
//   covering previewing inside | 새      | 옛 규칙과 다른 이유
//   F        F          F      | 투명    | 옛: 보드가 활성이면 불투명. 새: 마우스가 밖이면 활성이어도 투명
//   F(펼침)  F          F/T    | 투명/불투명 | 옛: 펼치면 늘 불투명. 새: 덮지 않는 펼친 창(60%)은 마우스를 따른다
//   T        F          *      | 불투명  | 같음 — 단 covering 은 "펼쳤다"가 아니라 "작업 영역을 덮는다"
//   *        T          *      | 투명    | 미리 보기가 먼저다(mac `!previewing && (inWindow || expandedToFullScreen() || menuOpen)`) — 덮은 창에서도 슬라이더 값이 보인다
// 나머지 행(F F T 불투명, F T * 투명)은 옛 규칙과 같다.
const TABLE = [
  [false, false, false, false],
  [false, false, true, true],
  [false, true, false, false],
  [false, true, true, false],
  [true, false, false, true],
  [true, false, true, true],
  [true, true, false, false],
  [true, true, true, false],
];

test("진리표 여덟 행", () => {
  for (const [covering, previewing, pointerInside, want] of TABLE)
    assert.equal(boardOpaque({ covering, previewing, pointerInside }), want, JSON.stringify({ covering, previewing, pointerInside }));
});

test("바뀐 행 — 보드가 활성이어도 마우스가 창 밖이면 투명, 옛 규칙은 불투명이었다", () => {
  const oldRule = (expanded, previewing, inside, focused) => expanded || (!previewing && (inside || focused));
  assert.equal(oldRule(false, false, false, true), true); // 옛: 활성 = 불투명
  assert.equal(boardOpaque({ covering: false, previewing: false, pointerInside: false }), false); // 새: 투명
});

test("바뀐 행 — 60% 로 줄인 펼친 창(덮지 않음)도 마우스가 밖이면 투명, 옛 규칙은 펼치면 늘 불투명이었다", () => {
  const oldRule = (expanded, previewing, inside, focused) => expanded || (!previewing && (inside || focused));
  assert.equal(oldRule(true, false, false, false), true);
  const covering = coversWorkArea(1152, 648, 1920, 1080);
  assert.equal(boardOpaque({ covering, previewing: false, pointerInside: false }), false);
  assert.equal(boardOpaque({ covering, previewing: false, pointerInside: true }), true);
});

test("작업 영역 전체를 덮은 창은 마우스가 밖이어도 불투명, 미리 보는 중에는 고른 투명도(미리 보기가 먼저)", () => {
  const covering = coversWorkArea(1920, 1080, 1920, 1080);
  assert.equal(boardOpaque({ covering, previewing: false, pointerInside: false }), true);
  assert.equal(boardOpaque({ covering, previewing: true, pointerInside: false }), false);
  assert.equal(boardOpaque({ covering, previewing: true, pointerInside: true }), false);
});

test("coversWorkArea — 허용 오차 2px 안은 덮는다, 3px 모자라면 덮지 않는다", () => {
  assert.equal(coversWorkArea(1920, 1080, 1920, 1080), true);
  assert.equal(coversWorkArea(1918, 1078, 1920, 1080), true);
  assert.equal(coversWorkArea(1917, 1080, 1920, 1080), false);
  assert.equal(coversWorkArea(1920, 1077, 1920, 1080), false);
  assert.equal(coversWorkArea(2000, 1200, 1920, 1080), true); // 넘쳐도 덮는다
});

test("coversWorkArea — 접힌 보드 칸(454 폭)과 60% 창은 덮지 않는다", () => {
  assert.equal(coversWorkArea(454, 1080, 1920, 1080), false);
  assert.equal(coversWorkArea(1152, 648, 1920, 1080), false);
});

test("coversWorkArea — 값을 못 잰 때(0·NaN)는 덮는 것으로 본다", () => {
  assert.equal(coversWorkArea(NaN, 100, 1920, 1080), true);
  assert.equal(coversWorkArea(1000, 800, 0, 0), true);
});
