// @ts-nocheck — node:test 와 .ts 경로 import 를 svelte-check 가 못 풀어서 건너뛴다(다른 시험 파일과 같다)
import test from "node:test";
import assert from "node:assert/strict";
import { createImeKeys } from "./ime.ts";

// 최소한의 textarea: 이벤트 리스너만 받는다
function fakeTextarea() {
  const handlers = {};
  return {
    addEventListener: (t, f) => (handlers[t] ||= []).push(f),
    removeEventListener: (t, f) => (handlers[t] = (handlers[t] || []).filter((x) => x !== f)),
    fire: (t) => (handlers[t] || []).forEach((f) => f()),
  };
}
const key = (code, extra = {}) => ({ type: "keydown", code, keyCode: 229, isComposing: false, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, preventDefault() {}, ...extra });
const tick = () => new Promise((r) => setTimeout(r, 5));

test("조합 중 Escape 는 한 번만 보낸다 (입력기가 compositionend 직후 다시 쏘는 Escape 는 버린다)", async () => {
  const ta = fakeTextarea();
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => false);
  ta.fire("compositionstart");
  assert.equal(ime.filter(key("Escape", { isComposing: true })), false); // 붙들림
  ta.fire("compositionend");
  assert.equal(ime.filter(key("Escape", { keyCode: 27 })), false); // 다시 쏘아진 것: 버림
  await tick();
  assert.deepEqual(sent, ["\x1b"]);
});

test("조합 중 Enter 도 한 번만 보낸다", async () => {
  const ta = fakeTextarea();
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => false);
  ta.fire("compositionstart");
  ime.filter(key("Enter", { isComposing: true }));
  ta.fire("compositionend");
  assert.equal(ime.filter(key("Enter", { keyCode: 13 })), false);
  await tick();
  assert.deepEqual(sent, ["\r"]);
});

test("조합이 끝난 뒤 따로 누른 Escape 는 그대로 xterm 으로 간다", async () => {
  const ta = fakeTextarea();
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => false);
  ta.fire("compositionstart");
  ime.filter(key("Escape", { isComposing: true }));
  ta.fire("compositionend");
  await tick(); // 붙들린 키가 나간 뒤
  assert.equal(ime.filter(key("Escape", { keyCode: 27 })), true);
  assert.deepEqual(sent, ["\x1b"]);
});

test("조합 중 방향키도 중복 없이 한 번만", async () => {
  const ta = fakeTextarea();
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => false);
  ta.fire("compositionstart");
  ime.filter(key("ArrowLeft", { isComposing: true }));
  ta.fire("compositionend");
  assert.equal(ime.filter(key("ArrowLeft", { keyCode: 37 })), false);
  await tick();
  assert.deepEqual(sent, ["\x1b[D"]);
});

test("조합 중 Alt+Enter 는 idle 때처럼 ESC CR 로 한 번만 (Alt 를 잃고 CR 만 가던 것)", async () => {
  const ta = fakeTextarea();
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => false);
  ta.fire("compositionstart");
  ime.filter(key("Enter", { isComposing: true, altKey: true }));
  ta.fire("compositionend");
  assert.equal(ime.filter(key("Enter", { keyCode: 13, altKey: true })), false);
  await tick();
  assert.deepEqual(sent, ["\x1b\r"]);
});
