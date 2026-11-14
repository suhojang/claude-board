// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/board/fontScale.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BOARD_FONT_SCALE_DEFAULT,
  BOARD_FONT_SCALE_MAX,
  BOARD_FONT_SCALE_MIN,
  BOARD_FONT_SCALE_STEP,
  boardFontScaleToast,
  canGrowBoardFontScale,
  canShrinkBoardFontScale,
  formatBoardFontScale,
  innerWidth,
  isDefaultBoardFontScale,
  miniHeightFactor,
  normalizeBoardFontScale as norm,
  screenHeight,
  stepBoardFontScale as step,
} from "./fontScale.ts";

test("상수: mac 과 같은 범위·단계·기본값", () => {
  assert.equal(BOARD_FONT_SCALE_MIN, 0.8);
  assert.equal(BOARD_FONT_SCALE_MAX, 1.5);
  assert.equal(BOARD_FONT_SCALE_DEFAULT, 1);
  assert.equal(BOARD_FONT_SCALE_STEP, 0.1);
});

test("normalize: 단계의 배수는 그대로, 부동소수점 오차 없이", () => {
  for (const t of [8, 9, 10, 11, 12, 13, 14, 15]) assert.equal(norm(t / 10), t / 10);
  assert.equal(norm(1.2000000000000002), 1.2);
  assert.equal(norm(0.1 * 3 + 0.8), 1.1); // 0.30000000000000004 + 0.8
});

test("normalize: 경계 — 0.8·1.5 는 그대로, 바로 밖은 끝값", () => {
  assert.equal(norm(0.8), 0.8);
  assert.equal(norm(1.5), 1.5);
  assert.equal(norm(0.79), 0.8);
  assert.equal(norm(1.51), 1.5);
  assert.equal(norm(0.7), 0.8);
  assert.equal(norm(1.6), 1.5);
});

test("normalize: 범위 밖은 끝값으로 자른다(음수·0·아주 큰 수·무한대 부호는 끝값이 아니라 기본값)", () => {
  assert.equal(norm(0), 0.8);
  assert.equal(norm(-3), 0.8);
  assert.equal(norm(100), 1.5);
  assert.equal(norm(Number.MAX_VALUE), 1.5);
  assert.equal(norm(Number.MIN_VALUE), 0.8);
  assert.equal(norm(Infinity), 1);
  assert.equal(norm(-Infinity), 1);
});

test("normalize: 단계가 아닌 값은 가장 가까운 단계로 맞춘다", () => {
  assert.equal(norm(1.04), 1);
  assert.equal(norm(1.06), 1.1);
  assert.equal(norm(1.26), 1.3);
  assert.equal(norm(0.84), 0.8);
  assert.equal(norm(0.86), 0.9);
  assert.equal(norm(1.44), 1.4);
  assert.equal(norm(1.46), 1.5);
});

test("normalize: 숫자 문자열은 받고 앞뒤 공백은 무시", () => {
  assert.equal(norm("1.2"), 1.2);
  assert.equal(norm(" 0.9 "), 0.9);
  assert.equal(norm("2"), 1.5);
  assert.equal(norm("0"), 0.8);
  assert.equal(norm("1e0"), 1);
});

test("normalize: 숫자가 아닌 값은 기본값", () => {
  for (const bad of [NaN, "", "   ", "abc", "1.2x", "NaN", "Infinity", null, undefined, {}, [], [1.2], true, false, () => 1, 10n]) {
    assert.equal(norm(bad), 1, `기본값이어야 한다: ${String(bad)}`);
  }
});

test("step: 한 단계씩, 끝에서 멈춘다", () => {
  assert.equal(step(1, 1), 1.1);
  assert.equal(step(1, -1), 0.9);
  assert.equal(step(1.4, 1), 1.5);
  assert.equal(step(1.5, 1), 1.5);
  assert.equal(step(0.9, -1), 0.8);
  assert.equal(step(0.8, -1), 0.8);
});

test("step: null 은 원래 크기 — 어느 값에서든 1", () => {
  for (const v of [0.8, 1.2, 1.5, 1, 99, NaN]) assert.equal(step(v, null), 1);
});

test("step: 열 번 누르면 끝에 닿고 되돌아온다(오차 누적 없음)", () => {
  let v = 1;
  for (let i = 0; i < 10; i++) v = step(v, 1);
  assert.equal(v, 1.5);
  for (let i = 0; i < 10; i++) v = step(v, -1);
  assert.equal(v, 0.8);
  for (let i = 0; i < 2; i++) v = step(v, 1);
  assert.equal(v, 1);
});

test("step: 현재 값이 깨졌으면 기본값에서 시작, step 이 0·NaN 이면 그대로", () => {
  assert.equal(step(NaN, 1), 1.1);
  assert.equal(step(Infinity, -1), 0.9);
  assert.equal(step(1.2, 0), 1.2);
  assert.equal(step(1.2, NaN), 1.2);
});

test("format·toast: 정수 퍼센트(mac `보드 글자 N%`)", () => {
  assert.equal(formatBoardFontScale(1), "100%");
  assert.equal(formatBoardFontScale(0.8), "80%");
  assert.equal(formatBoardFontScale(1.1), "110%");
  assert.equal(formatBoardFontScale(1.5), "150%");
  assert.equal(formatBoardFontScale(7), "150%");
  assert.equal(formatBoardFontScale(NaN), "100%");
  assert.equal(boardFontScaleToast(1.3), "보드 글자 130%");
});

test("isDefault·can*: 원래 크기와 끝 단추 상태", () => {
  assert.equal(isDefaultBoardFontScale(1), true);
  assert.equal(isDefaultBoardFontScale(1.1), false);
  assert.equal(isDefaultBoardFontScale("1"), true);
  assert.equal(isDefaultBoardFontScale(NaN), true);
  assert.equal(canGrowBoardFontScale(1.4), true);
  assert.equal(canGrowBoardFontScale(1.5), false);
  assert.equal(canShrinkBoardFontScale(0.9), true);
  assert.equal(canShrinkBoardFontScale(0.8), false);
});

test("화면 크기 변환: 안쪽 높이×배율 = 화면 높이, 화면 폭÷배율 = 안쪽 폭", () => {
  assert.equal(screenHeight(200, 1.5), 300);
  assert.equal(screenHeight(200, 1), 200);
  assert.equal(innerWidth(360, 1.5), 240);
  assert.equal(innerWidth(360, 0.8), 450);
  assert.equal(innerWidth(360, NaN), 360);
});

test("miniHeightFactor: 80% 는 줄이지 않고 150% 는 늘린다(mac max(1, scale))", () => {
  assert.equal(miniHeightFactor(0.8), 1);
  assert.equal(miniHeightFactor(1), 1);
  assert.equal(miniHeightFactor(1.5), 1.5);
});
