// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 서브에이전트 줄 아이콘 규칙이 mac SubagentRow(3.2.1)와 같은지 시험한다.
// 실행: cd windows/app && node --test src/lib/board/subIcon.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import { subIconKind } from "./subIcon.ts";

test("working 은 멈춘 진행 중 아이콘 — 도는 아이콘이 아니다", () => {
  assert.equal(subIconKind("working"), "still");
});

test("working 이 아닌 보드 상태는 그대로 StateIcon 에 넘긴다", () => {
  for (const s of ["waiting", "idle", "paused", "ended"]) assert.equal(subIconKind(s), s);
});

// 서브에이전트 state(running·waiting·done) → subState → 아이콘. 입력에 부모 세션 상태가 없다 —
// 부모가 working·waiting·idle·paused·ended 어느 쪽이든 같은 서브에이전트는 같은 아이콘이다.
test("서브에이전트 state 별 최종 아이콘: running 멈춘 것, waiting 파문, done 체크", () => {
  const subState = (s) => ({ running: "working", waiting: "waiting", done: "idle" })[s] ?? "working";
  assert.equal(subIconKind(subState("running")), "still");
  assert.equal(subIconKind(subState("waiting")), "waiting");
  assert.equal(subIconKind(subState("done")), "idle");
  assert.equal(subIconKind(subState("모르는 값")), "still");
});

test("서브에이전트 줄에는 working(도는 아이콘)이 나오지 않는다", () => {
  for (const s of ["working", "waiting", "idle", "paused", "ended"]) assert.notEqual(subIconKind(s), "working");
});
