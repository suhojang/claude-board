// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/shortcuts/recording.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { createRecorder } from "./recording.ts";

const press = (code, o = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...o });

// 가짜 백엔드 — 호출을 순서대로 적는다. write 는 저장 목록도 고쳐서 다음 판단이 새 목록을 보게 한다
function setup(initialOverrides = {}) {
  const calls = [];
  let overrides = initialOverrides;
  const state = { recording: null, refused: null };
  const recorder = createRecorder(state, {
    overrides: () => overrides,
    write: (id, override) => {
      calls.push(["write", id, override]);
      overrides = override ? { ...overrides, [id]: override } : Object.fromEntries(Object.entries(overrides).filter(([k]) => k !== id));
    },
    syncHotkeys: (paused) => calls.push(["hotkeys", paused]),
  });
  return { state, recorder, calls, overrides: () => overrides };
}

test("받기 시작 — 전역 단축키를 풀고 그 동작이 받는 중이 된다", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  assert.equal(state.recording, "rename");
  assert.deepEqual(calls, [["hotkeys", true]]);
});

test("Esc 는 취소 — 아무것도 바꾸지 않고 전역 단축키를 다시 건다", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  recorder.key(press("Escape"));
  assert.equal(state.recording, null);
  assert.deepEqual(calls, [["hotkeys", true], ["hotkeys", false]]);
});

test("Backspace·Delete 는 비우기", () => {
  for (const code of ["Backspace", "Delete"]) {
    const { state, recorder, calls } = setup();
    recorder.start("rename");
    recorder.key(press(code));
    assert.equal(state.recording, null);
    assert.deepEqual(calls, [["hotkeys", true], ["hotkeys", false], ["write", "rename", []]], code);
  }
});

test("새 키 — 전역 단축키를 다시 건 뒤 저장하고 받기를 끝낸다", () => {
  const { state, recorder, calls } = setup();
  recorder.start("newSession");
  recorder.key(press("KeyN", { ctrlKey: true, altKey: true }));
  assert.equal(state.recording, null);
  assert.equal(state.refused, null);
  assert.deepEqual(calls, [["hotkeys", true], ["hotkeys", false], ["write", "newSession", ["KeyN", 3]]]);
});

test("기본 키로 되돌아오면 저장하지 않고 항목을 지운다(null)", () => {
  const { recorder, calls } = setup({ newSession: ["KeyN", 3] });
  recorder.start("newSession");
  recorder.key(press("KeyT", { ctrlKey: true }));
  assert.deepEqual(calls.at(-1), ["write", "newSession", null]);
});

test("수정키만 눌린 keydown 은 건너뛴다 — 계속 받는 중", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  recorder.key(press("ControlLeft", { ctrlKey: true }));
  recorder.key(press("ShiftLeft", { ctrlKey: true, shiftKey: true }));
  assert.equal(state.recording, "rename");
  assert.equal(state.refused, null);
  assert.deepEqual(calls, [["hotkeys", true]]);
});

test("Ctrl·Alt 가 없는 키는 받지 않고 안내 — 계속 받는 중", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  recorder.key(press("KeyA"));
  recorder.key(press("KeyA", { shiftKey: true }));
  assert.equal(state.recording, "rename");
  assert.match(state.refused, /Ctrl.*Alt/);
  assert.deepEqual(calls, [["hotkeys", true]]);
});

test("F1~F12 는 수정키 없이도 받는다", () => {
  const { recorder, calls } = setup();
  recorder.start("pin");
  recorder.key(press("F6"));
  assert.deepEqual(calls.at(-1), ["write", "pin", ["F6", 0]]);
});

test("고를 수 없는 키는 받지 않고 어떤 키인지 알린다", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  recorder.key(press("F4", { altKey: true }));
  assert.equal(state.recording, "rename");
  assert.match(state.refused, /Alt\+F4/);
  recorder.key(press("KeyD", { metaKey: true }));
  assert.match(state.refused, /Win\+D/);
  assert.deepEqual(calls, [["hotkeys", true]]);
});

test("안내가 뜬 뒤 고른 키가 받아지면 안내가 사라진다", () => {
  const { state, recorder } = setup();
  recorder.start("rename");
  recorder.key(press("KeyA"));
  assert.notEqual(state.refused, null);
  recorder.key(press("KeyN", { ctrlKey: true }));
  assert.equal(state.refused, null);
});

test("안내가 뜬 뒤 Esc 로 취소하면 안내도 사라진다", () => {
  const { state, recorder } = setup();
  recorder.start("rename");
  recorder.key(press("KeyA"));
  assert.notEqual(state.refused, null);
  recorder.key(press("Escape"));
  assert.equal(state.recording, null);
  assert.equal(state.refused, null);
});

test("안내가 뜬 뒤 Backspace 로 비우면 안내도 사라진다", () => {
  const { state, recorder } = setup();
  recorder.start("rename");
  recorder.key(press("KeyA"));
  recorder.key(press("Backspace"));
  assert.equal(state.recording, null);
  assert.equal(state.refused, null);
});

test("받는 중이 아니면 키를 건드리지 않는다", () => {
  const { recorder, calls } = setup();
  assert.equal(recorder.key(press("KeyN", { ctrlKey: true })), false);
  recorder.stop();
  assert.deepEqual(calls, []);
});

test("받는 동작을 바꿔 누르면 그 동작이 받고 전역 단축키는 한 번만 푼다", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  recorder.start("pin");
  assert.equal(state.recording, "pin");
  assert.deepEqual(calls, [["hotkeys", true]]);
});

test("받는 중에 그만두면(설정 화면 닫힘) 전역 단축키가 다시 걸린다", () => {
  const { state, recorder, calls } = setup();
  recorder.start("rename");
  recorder.stop();
  assert.equal(state.recording, null);
  assert.deepEqual(calls, [["hotkeys", true], ["hotkeys", false]]);
});

test("겹친 동작 비우기·한 줄 되돌리기", () => {
  const { recorder, calls } = setup({ rename: ["KeyT", 1] });
  recorder.clear("newSession");
  recorder.reset("rename");
  assert.deepEqual(calls, [["write", "newSession", []], ["write", "rename", null]]);
});

test("겹침을 만든 뒤 상대를 비우면 겹침이 풀린다 (전체 흐름)", async () => {
  const { conflicts } = await import("./rules.ts");
  const { recorder, overrides } = setup();
  recorder.start("rename");
  recorder.key(press("KeyT", { ctrlKey: true }));
  assert.deepEqual(conflicts("rename", overrides()), ["newSession"]);
  recorder.clear("newSession");
  assert.deepEqual(conflicts("rename", overrides()), []);
});
