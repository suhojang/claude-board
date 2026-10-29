// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/motion.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { motionFor, frameAt } from "./motion.ts";

test("옆으로 밀기 — 보드 반대쪽 가장자리에서 들어오고 그쪽으로 나간다", () => {
  assert.deepEqual(motionFor("slide", { boardLeft: false, reduced: false }), { duration: 340, dx: -1, fade: true });
  assert.deepEqual(motionFor("slide", { boardLeft: true, reduced: false }), { duration: 340, dx: 1, fade: true });
});

test("서서히 바뀌기 — 자리는 그대로 두고 투명도만", () => {
  assert.deepEqual(motionFor("fade", { boardLeft: false, reduced: false }), { duration: 200, dx: 0, fade: true });
});

test("효과 없음은 길이 0", () => {
  assert.deepEqual(motionFor("none", { boardLeft: false, reduced: false }), { duration: 0, dx: 0, fade: false });
});

test("움직임 줄이기 설정이면 어느 모드든 효과 없음", () => {
  for (const mode of ["slide", "fade", "none"]) {
    assert.deepEqual(motionFor(mode, { boardLeft: true, reduced: true }), { duration: 0, dx: 0, fade: false });
  }
});

test("알 수 없는 값은 옆으로 밀기(기본)로 본다", () => {
  assert.equal(motionFor("zzz", { boardLeft: false, reduced: false }).duration, 340);
  assert.equal(motionFor(undefined, { boardLeft: false, reduced: false }).duration, 340);
});

test("frameAt — t=0 은 가장자리 밖·투명, t=1 은 제자리·불투명", () => {
  const m = { duration: 340, dx: -1, fade: true };
  assert.deepEqual(frameAt(m, 0), { translatePct: -100, opacity: 0 });
  assert.deepEqual(frameAt(m, 1), { translatePct: 0, opacity: 1 });
  assert.deepEqual(frameAt({ duration: 200, dx: 0, fade: true }, 0.5), { translatePct: 0, opacity: 0.5 });
  assert.deepEqual(frameAt({ duration: 0, dx: 0, fade: false }, 0), { translatePct: 0, opacity: 1 });
});
