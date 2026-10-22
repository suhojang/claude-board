// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/replayGate.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { ReplayGate, USER_INPUT_WINDOW_MS } from "./replayGate.ts";

test("되감기 중이 아니면 모두 통과", () => {
  assert.equal(new ReplayGate().allows(1000), true);
});

test("되감기 중 xterm 자동 응답은 삼킨다", () => {
  const gate = new ReplayGate();
  gate.replayWriteStarted();
  assert.equal(gate.allows(1000), false);
});

test("되감기 중이어도 사용자 입력 직후 데이터는 통과", () => {
  const gate = new ReplayGate();
  gate.replayWriteStarted();
  gate.noteUserInput(1000);
  assert.equal(gate.allows(1000 + USER_INPUT_WINDOW_MS - 1), true);
  assert.equal(gate.allows(1000 + USER_INPUT_WINDOW_MS), false);
});

test("쓰기가 모두 끝나면 다시 통과, 카운터는 음수가 되지 않는다", () => {
  const gate = new ReplayGate();
  gate.replayWriteStarted();
  gate.replayWriteStarted();
  gate.replayWriteFinished();
  assert.equal(gate.allows(5), false);
  gate.replayWriteFinished();
  gate.replayWriteFinished();
  assert.equal(gate.allows(5), true);
  gate.replayWriteStarted();
  assert.equal(gate.allows(5), false);
});
