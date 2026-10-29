// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/split.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import type { SplitNode } from "../types.ts";
import { clampRatio, hasLeaf, leaves, removeLeaf, replaceLeaf } from "./split.ts";

const a: SplitNode = { leaf: "a" };
const b: SplitNode = { leaf: "b" };
const c: SplitNode = { leaf: "c" };
const ab: SplitNode = { split: "right", ratio: 0.3, first: a, second: b };
const abc: SplitNode = { split: "down", ratio: 0.5, first: ab, second: c };

test("leaves: 트리 순서(앞에서 뒤)", () => {
  assert.deepEqual(leaves(a), ["a"]);
  assert.deepEqual(leaves(abc), ["a", "b", "c"]);
  assert.equal(hasLeaf(abc, "b"), true);
  assert.equal(hasLeaf(abc, "z"), false);
});

test("removeLeaf: 마지막 잎을 빼면 null, 없는 id 는 그대로", () => {
  assert.equal(removeLeaf(a, "a"), null);
  assert.deepEqual(removeLeaf(a, "x"), a);
});

test("removeLeaf: 둘 중 하나를 빼면 부모가 형제로 접힌다", () => {
  assert.deepEqual(removeLeaf(ab, "a"), b);
  assert.deepEqual(removeLeaf(ab, "b"), a);
});

test("removeLeaf: 깊은 잎을 빼도 나머지 몫·방향은 그대로", () => {
  assert.deepEqual(removeLeaf(abc, "a"), { split: "down", ratio: 0.5, first: b, second: c });
  assert.deepEqual(removeLeaf(abc, "c"), ab);
});

test("replaceLeaf: 잎을 가지로 바꾸고 원본은 그대로", () => {
  assert.deepEqual(replaceLeaf(a, "a", ab), ab);
  const grown = replaceLeaf(abc, "c", { split: "right", ratio: 0.5, first: c, second: { leaf: "d" } });
  assert.deepEqual(leaves(grown), ["a", "b", "c", "d"]);
  assert.deepEqual(replaceLeaf(a, "x", b), a);
  assert.deepEqual(leaves(abc), ["a", "b", "c"]);
});

test("clampRatio: 양쪽 80px 보장, 짧으면 0.5 로 모인다", () => {
  assert.equal(clampRatio(0.5, 800), 0.5);
  assert.equal(clampRatio(0, 800), 0.1);
  assert.equal(clampRatio(1, 800), 0.9);
  assert.equal(clampRatio(0.05, 100), 0.5);
  assert.equal(clampRatio(0.9, 100), 0.5);
});
