// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/reopenGuard.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import { REOPEN_TTL_MS, canTakeAsIdle, reopeningActive } from "./reopenGuard.ts";

const r = { since: 1000, surfaceId: "s1" };
const facts = (over = {}) => ({ now: 2000, sessionEnded: true, termRunsClaude: false, ...over });

test("표지가 없으면 활성이 아니다", () => {
  assert.equal(reopeningActive(undefined, facts()), false);
});

test("세션이 아직 끝남이고 claude 가 안 떴으면 활성이다", () => {
  assert.equal(reopeningActive(r, facts()), true);
});

test("세션이 끝남을 벗어나거나 그 터미널에서 claude 가 뜨면 끝난다", () => {
  assert.equal(reopeningActive(r, facts({ sessionEnded: false })), false);
  assert.equal(reopeningActive(r, facts({ termRunsClaude: true })), false);
});

test("10초가 지나면 끝난다", () => {
  assert.equal(reopeningActive(r, facts({ now: r.since + REOPEN_TTL_MS - 1 })), true);
  assert.equal(reopeningActive(r, facts({ now: r.since + REOPEN_TTL_MS })), false);
});

test("이어 열기 중인 세션에 묶인 화면은 idle 자리로 쓰지 않는다", () => {
  const ids = new Set(["a"]);
  assert.equal(canTakeAsIdle("a", ids), false);
  assert.equal(canTakeAsIdle("b", ids), true);
  assert.equal(canTakeAsIdle(null, ids), true);
});
