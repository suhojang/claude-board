// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/webglBlink.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { disposeWebgl } from "./webglBlink.ts";

function fixture(blink) {
  const log = [];
  const term = { options: { cursorBlink: blink }, reset: () => log.push("reset") };
  const addon = { dispose: () => log.push(`dispose(blink=${term.options.cursorBlink})`) };
  return { term, addon, log };
}

test("마지막 폐기: 리셋 뒤 깜빡임을 끈 채로 애드온을 폐기하고 되돌리지 않는다", () => {
  const { term, addon, log } = fixture(true);
  disposeWebgl(term, addon, true);
  assert.deepEqual(log, ["reset", "dispose(blink=false)"]);
  assert.equal(term.options.cursorBlink, false);
});

test("컨텍스트를 잃었을 때: 끈 채로 폐기하고 깜빡임을 되돌리며 화면은 리셋하지 않는다", () => {
  const { term, addon, log } = fixture(true);
  disposeWebgl(term, addon, false);
  assert.deepEqual(log, ["dispose(blink=false)"]);
  assert.equal(term.options.cursorBlink, true);
});

test("깜빡임이 원래 꺼져 있으면 옵션도 리셋도 건드리지 않는다", () => {
  for (const final of [true, false]) {
    const { term, addon, log } = fixture(false);
    disposeWebgl(term, addon, final);
    assert.deepEqual(log, ["dispose(blink=false)"]);
    assert.equal(term.options.cursorBlink, false);
  }
});

test("폐기가 던져도 깜빡임을 되돌린다(컨텍스트를 잃은 경우)", () => {
  const { term } = fixture(true);
  const addon = { dispose: () => { throw new Error("boom"); } };
  assert.throws(() => disposeWebgl(term, addon, false), /boom/);
  assert.equal(term.options.cursorBlink, true);
});

test("터미널이 없어도 애드온은 폐기한다", () => {
  let n = 0;
  disposeWebgl(null, { dispose: () => n++ }, true);
  assert.equal(n, 1);
});

test("업스트림 감시 — @xterm/addon-webgl 이 0.19.0 이 아니면 덮개를 다시 본다", () => {
  const pkg = JSON.parse(readFileSync(new URL("../../../node_modules/@xterm/addon-webgl/package.json", import.meta.url), "utf8"));
  assert.equal(
    pkg.version,
    "0.19.0",
    "re-check webglBlink.ts: delete the shim if upstream WebglRenderer _register()s _cursorBlinkStateManager (WebglRenderer.ts:32)",
  );
});
