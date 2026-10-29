// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/board/clickGate.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import { createClickGate } from "./clickGate.ts";

// 가짜 시계 — advance 로만 시간이 흐른다
function harness(delayMs) {
  let now = 0;
  let nextHandle = 1;
  const timers = new Map();
  const log = [];
  const gate = createClickGate({
    delayMs,
    setTimer: (callback, ms) => {
      const handle = nextHandle++;
      timers.set(handle, { at: now + ms, callback });
      return handle;
    },
    clearTimer: (handle) => timers.delete(handle),
    onActivate: () => log.push("activate"),
    onFocus: () => log.push("focus"),
  });
  const advance = (ms) => {
    now += ms;
    for (const [handle, timer] of [...timers]) {
      if (timer.at <= now) {
        timers.delete(handle);
        timer.callback();
      }
    }
  };
  return { gate, log, advance, timers };
}

test("한 번 클릭 — T 전에는 아무것도 없고 T 에 activate 한 번", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  advance(499);
  assert.deepEqual(log, []);
  advance(1);
  assert.deepEqual(log, ["activate"]);
  advance(5000);
  assert.deepEqual(log, ["activate"]);
});

test("T 안의 두 번째 클릭 — focus 한 번, activate 없음", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  advance(200);
  gate.press(2);
  advance(5000);
  assert.deepEqual(log, ["focus"]);
});

test("세 번째 클릭은 focus 를 더 내지 않는다", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  gate.press(2);
  gate.press(3);
  advance(5000);
  assert.deepEqual(log, ["focus"]);
});

test("T 보다 떨어진 두 번의 한 번 클릭 — activate 두 번", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  advance(600);
  gate.press(1);
  advance(600);
  assert.deepEqual(log, ["activate", "activate"]);
});

test("T 전에 dispose 하면 아무것도 나가지 않는다", () => {
  const { gate, log, advance, timers } = harness(500);
  gate.press(1);
  gate.dispose();
  advance(5000);
  assert.deepEqual(log, []);
  assert.equal(timers.size, 0);
});

test("지연 0 — detail 1 에서 바로 activate, detail 2 에서 바로 focus, 타이머 없음", () => {
  const { gate, log, timers } = harness(0);
  gate.press(1);
  assert.deepEqual(log, ["activate"]);
  gate.press(2);
  assert.deepEqual(log, ["activate", "focus"]);
  assert.equal(timers.size, 0);
});

test("대기 중 또 detail 1 이 오면 앞 타이머는 취소되고 새 T 로 센다", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  advance(300);
  gate.press(1);
  advance(300);
  assert.deepEqual(log, []);
  advance(200);
  assert.deepEqual(log, ["activate"]);
});

test("세 번 연속(1, 2, 3) — 시간이 한참 흘러도 focus 하나뿐이고 타이머가 남지 않는다", () => {
  const { gate, log, advance, timers } = harness(500);
  gate.press(1);
  advance(100);
  gate.press(2);
  advance(100);
  gate.press(3);
  advance(10_000);
  assert.deepEqual(log, ["focus"]);
  assert.equal(timers.size, 0);
});

test("detail 3·0 이 홀로 와도 아무것도 내지 않고 대기 타이머도 만들지 않는다", () => {
  const { gate, log, advance, timers } = harness(500);
  gate.press(3);
  gate.press(0);
  advance(10_000);
  assert.deepEqual(log, []);
  assert.equal(timers.size, 0);
});

test("detail 2 가 홀로 와도(첫 눌림을 놓침) focus 한 번", () => {
  const { gate, log } = harness(500);
  gate.press(2);
  assert.deepEqual(log, ["focus"]);
});

test("두 번 클릭이 끝난 뒤 새 한 번 클릭은 다시 T 뒤에 activate 한다", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  gate.press(2);
  advance(2000);
  gate.press(1);
  advance(499);
  assert.deepEqual(log, ["focus"]);
  advance(1);
  assert.deepEqual(log, ["focus", "activate"]);
});

test("dispose 는 타이머가 이미 터진 뒤에는 아무 일도 안 하고, 두 번 불러도 안전하다", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  advance(500);
  gate.dispose();
  gate.dispose();
  assert.deepEqual(log, ["activate"]);
});

test("dispose 뒤에도 게이트는 다시 쓸 수 있다", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  gate.dispose();
  gate.press(1);
  advance(500);
  assert.deepEqual(log, ["activate"]);
});

test("타이머 중간에 dispose 한 뒤 늦게 온 두 번째 클릭은 focus 만 낸다", () => {
  const { gate, log, advance } = harness(500);
  gate.press(1);
  advance(200);
  gate.dispose();
  advance(1000);
  gate.press(2);
  assert.deepEqual(log, ["focus"]);
});

test("clearTimer 가 못 막는 환경에서 이미 취소된 타이머의 콜백이 뒤늦게 불려도 activate 하지 않는다", () => {
  const callbacks = [];
  const log = [];
  const gate = createClickGate({
    delayMs: 500,
    setTimer: (cb) => callbacks.push(cb),
    clearTimer: () => {}, // 일부러 아무것도 지우지 않는다
    onActivate: () => log.push("activate"),
    onFocus: () => log.push("focus"),
  });
  gate.press(1);
  gate.press(2); // 첫 타이머를 취소한다
  callbacks[0]();
  assert.deepEqual(log, ["focus"]);
  gate.press(1);
  gate.dispose(); // 둘째 타이머를 취소한다
  callbacks[1]();
  assert.deepEqual(log, ["focus"]);
});

test("지연 0 — 세 번 연속은 activate, focus 뒤 아무것도 없다", () => {
  const { gate, log, timers } = harness(0);
  gate.press(1);
  gate.press(2);
  gate.press(3);
  assert.deepEqual(log, ["activate", "focus"]);
  assert.equal(timers.size, 0);
});

test("음수 지연도 0 처럼 즉시 activate 한다", () => {
  const { gate, log, timers } = harness(-5);
  gate.press(1);
  assert.deepEqual(log, ["activate"]);
  assert.equal(timers.size, 0);
});

test("T 미만 간격의 한 번 클릭 셋 — 타이머는 하나만 살고 마지막 클릭 뒤 T 에 activate 한 번", () => {
  const { gate, log, advance, timers } = harness(500);
  gate.press(1);
  advance(100);
  gate.press(1);
  advance(100);
  gate.press(1);
  assert.equal(timers.size, 1);
  advance(499);
  assert.deepEqual(log, []);
  advance(1);
  assert.deepEqual(log, ["activate"]);
});
