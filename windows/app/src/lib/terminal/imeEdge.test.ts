// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/imeEdge.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
// ime.test.ts 가 다루지 않는 가장자리 — 조합 밖 229 키, 수식키 조합, 붙들지 않는 키, 방향키 변형, 정리(dispose).
import test from "node:test";
import assert from "node:assert/strict";
import { createImeKeys } from "./ime.ts";

function fakeTextarea() {
  const handlers = {};
  return {
    handlers,
    addEventListener: (t, f) => (handlers[t] ||= []).push(f),
    removeEventListener: (t, f) => (handlers[t] = (handlers[t] || []).filter((x) => x !== f)),
    fire: (t) => (handlers[t] || []).slice().forEach((f) => f()),
  };
}
const key = (code, extra = {}) => ({ type: "keydown", code, keyCode: 229, isComposing: false, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, preventDefault() {}, ...extra });
const tick = () => new Promise((r) => setTimeout(r, 5));
function setup(appCursor = false) {
  const ta = fakeTextarea();
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => appCursor);
  return { ta, sent, ime };
}

test("조합 밖 229 — Escape 는 바로 보내고 xterm 은 건너뛴다", () => {
  const { sent, ime } = setup();
  assert.equal(ime.filter(key("Escape")), false);
  assert.deepEqual(sent, ["\x1b"]);
});

test("조합 밖 229 — Ctrl+A..Z 는 제어 문자, Ctrl+Space 는 NUL", () => {
  const { sent, ime } = setup();
  assert.equal(ime.filter(key("KeyC", { ctrlKey: true })), false);
  assert.equal(ime.filter(key("KeyA", { ctrlKey: true })), false);
  assert.equal(ime.filter(key("KeyZ", { ctrlKey: true })), false);
  assert.equal(ime.filter(key("Space", { ctrlKey: true })), false);
  assert.deepEqual(sent, ["\x03", "\x01", "\x1a", "\x00"]);
});

test("조합 밖 229 — Ctrl+Alt·Ctrl+Meta 조합(AltGr 글자 등)은 건드리지 않는다", () => {
  const { sent, ime } = setup();
  assert.equal(ime.filter(key("KeyQ", { ctrlKey: true, altKey: true })), true);
  assert.equal(ime.filter(key("KeyQ", { ctrlKey: true, metaKey: true })), true);
  assert.deepEqual(sent, []);
});

test("조합 밖 229 — 글자·숫자·Enter 는 xterm 에 맡긴다", () => {
  const { sent, ime } = setup();
  for (const code of ["KeyA", "Digit1", "Enter", "ArrowLeft"]) assert.equal(ime.filter(key(code)), true, code);
  assert.deepEqual(sent, []);
});

test("229 가 아닌 평범한 키는 그대로 통과 — Escape 도 xterm 이 읽는다", () => {
  const { sent, ime } = setup();
  assert.equal(ime.filter(key("Escape", { keyCode: 27 })), true);
  assert.equal(ime.filter(key("KeyA", { keyCode: 65 })), true);
  assert.deepEqual(sent, []);
});

test("keydown 이 아닌 이벤트(keyup·keypress)는 건드리지 않는다", () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  assert.equal(ime.filter(key("Enter", { type: "keyup" })), true);
  assert.equal(ime.filter(key("Escape", { type: "keypress" })), true);
  assert.deepEqual(sent, []);
});

test("조합 중 — 글자 키·수식키만 누른 것은 붙들지 않고 입력기에 맡긴다", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  for (const code of ["KeyA", "Space", "ShiftLeft", "ControlRight", "AltLeft", "MetaLeft", "Backspace", "Tab"]) {
    assert.equal(ime.filter(key(code, { isComposing: true })), true, code);
  }
  ta.fire("compositionend");
  await tick();
  assert.deepEqual(sent, []);
});

test("조합 중 Shift+Enter 는 줄바꿈 \\n, 숫자패드 Enter 는 \\r", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  ime.filter(key("Enter", { shiftKey: true }));
  ta.fire("compositionend");
  await tick();
  ta.fire("compositionstart");
  ime.filter(key("NumpadEnter"));
  ta.fire("compositionend");
  await tick();
  assert.deepEqual(sent, ["\n", "\r"]);
});

test("조합 중 Enter 를 두 번 눌러도 한 번만 붙든다", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  assert.equal(ime.filter(key("Enter")), false);
  assert.equal(ime.filter(key("Enter")), false);
  ta.fire("compositionend");
  await tick();
  assert.deepEqual(sent, ["\r"]);
});

test("조합 중 Ctrl+C 도 붙들었다가 끝난 뒤 한 번 보낸다", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  assert.equal(ime.filter(key("KeyC", { ctrlKey: true })), false);
  ta.fire("compositionend");
  assert.equal(ime.filter(key("KeyC", { ctrlKey: true })), false); // 다시 쏘아진 같은 키는 버린다
  await tick();
  assert.deepEqual(sent, ["\x03"]);
});

test("조합 중 수식키 방향키 — modifier 숫자를 붙인다(Shift 2 · Alt 3 · Ctrl 5 · Ctrl+Shift 6)", async () => {
  const { ta, sent, ime } = setup();
  const press = async (extra, code) => {
    ta.fire("compositionstart");
    ime.filter(key(code, extra));
    ta.fire("compositionend");
    await tick();
  };
  await press({ shiftKey: true }, "ArrowRight");
  await press({ altKey: true }, "ArrowUp");
  await press({ ctrlKey: true }, "ArrowLeft");
  await press({ ctrlKey: true, shiftKey: true }, "ArrowDown");
  assert.deepEqual(sent, ["\x1b[1;2C", "\x1b[1;3A", "\x1b[1;5D", "\x1b[1;6B"]);
});

test("조합 중 방향키는 애플리케이션 커서 모드에서 \\x1bO 로 간다, Home·End 도", async () => {
  const { ta, sent, ime } = setup(true);
  ta.fire("compositionstart");
  ime.filter(key("ArrowUp"));
  ime.filter(key("Home"));
  ime.filter(key("End"));
  ta.fire("compositionend");
  await tick();
  assert.deepEqual(sent, ["\x1bOA", "\x1bOH", "\x1bOF"]);
});

test("조합 중 서로 다른 키는 누른 순서대로 모두 보낸다", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  ime.filter(key("ArrowLeft"));
  ime.filter(key("Enter"));
  ime.filter(key("Escape"));
  ta.fire("compositionend");
  await tick();
  assert.deepEqual(sent, ["\x1b[D", "\r", "\x1b"]);
});

test("붙든 키가 있는 채 다음 조합이 시작되면 앞서 붙든 키부터 보낸다", () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  ime.filter(key("Enter"));
  ta.fire("compositionend");
  ta.fire("compositionstart"); // end 의 지연 전송이 돌기 전에 곧바로 다시 시작
  assert.deepEqual(sent, ["\r"]);
});

test("compositionend 직후 다시 쏘아진 키가 아닌 다른 키는 버리지 않는다", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  ime.filter(key("Enter"));
  ta.fire("compositionend");
  assert.equal(ime.filter(key("KeyC", { ctrlKey: true })), false); // 229 + Ctrl+C: 직접 보낸다
  await tick();
  assert.deepEqual(sent, ["\x03", "\r"]);
});

test("isComposing 이 참이면 compositionstart 를 못 받았어도 조합 중으로 본다", async () => {
  const { sent, ime } = setup();
  assert.equal(ime.filter(key("Enter", { isComposing: true })), false);
  assert.deepEqual(sent, []); // 아직 붙들고 있다
  assert.equal(ime.filter(key("ArrowLeft", { isComposing: true })), false);
});

test("dispose 뒤에는 리스너가 떨어지고 대기 중이던 전송도 나가지 않는다", async () => {
  const { ta, sent, ime } = setup();
  ta.fire("compositionstart");
  ime.filter(key("Enter"));
  ta.fire("compositionend");
  ime.dispose();
  await tick();
  assert.deepEqual(sent, []);
  assert.equal((ta.handlers.compositionstart || []).length, 0);
  assert.equal((ta.handlers.compositionend || []).length, 0);
});
