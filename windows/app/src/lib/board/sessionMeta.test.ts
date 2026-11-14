// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 세션 줄 정보 라벨 줄임 단계가 mac SessionRow(3.1.0 가장 좁을 때 다음 줄로 넘김)와 같은지 시험한다.
// 실행: cd windows/app && node --test src/lib/board/sessionMeta.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { META_STAGES, metaStage } from "./sessionMeta.ts";

test("단계는 다섯이고 mac ViewThatFits 후보 순서와 같다", () => {
  assert.deepEqual(
    META_STAGES.map((s) => [s.level, s.name, s.wrap]),
    [
      [0, true, false],
      [1, true, false],
      [1, false, false],
      [2, false, false],
      [2, false, true],
    ],
  );
});

test("다음 줄 넘김은 마지막 단계에서만", () => {
  assert.deepEqual(
    META_STAGES.map((s) => s.wrap),
    [false, false, false, false, true],
  );
  assert.equal(metaStage(3).wrap, false);
  assert.equal(metaStage(4).wrap, true);
});

test("세션 이름은 단계 1 까지만 보이고 2 부터 빠진다", () => {
  assert.equal(metaStage(1).name, true);
  assert.equal(metaStage(2).name, false);
  assert.equal(metaStage(4).name, false);
});

test("줄이는 정도는 단계가 갈수록 줄지 않는다", () => {
  for (let i = 1; i < META_STAGES.length; i++) {
    assert.ok(META_STAGES[i].level >= META_STAGES[i - 1].level, `단계 ${i}`);
  }
});

test("범위 밖 단계 번호는 끝 쪽으로 붙는다", () => {
  assert.equal(metaStage(-1), META_STAGES[0]);
  assert.equal(metaStage(99), META_STAGES[4]);
  assert.equal(metaStage(Number.NaN), META_STAGES[4]);
});
