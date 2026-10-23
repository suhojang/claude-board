// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/imeBlur.test.ts
// 칸이 포커스를 잃으며 끝난 조합 — 크롬은 compositionend 직후 blur 를 내고 xterm 은 blur 에서 칸을 비워 조합 글자를 잃는다(실측 GUI 시험)
import test from "node:test";
import assert from "node:assert/strict";
import { createImeKeys } from "./ime.ts";

function setup() {
  const handlers = {};
  const calls = [];
  const doc = { activeElement: null };
  const ta = {
    value: "",
    ownerDocument: doc,
    addEventListener: (t, f) => (handlers[t] ||= []).push(f),
    removeEventListener: (t, f) => (handlers[t] = (handlers[t] || []).filter((x) => x !== f)),
    fire: (t, ev) => (handlers[t] || []).slice().forEach((f) => f(ev)),
    blur: () => calls.push("blur"),
    focus: () => calls.push("focus"),
    count: (t) => (handlers[t] || []).length,
  };
  doc.activeElement = ta;
  const sent = [];
  const ime = createImeKeys(ta, (d) => sent.push(d), () => false);
  return { ta, doc, sent, ime, calls };
}
const tick = () => new Promise((r) => setTimeout(r, 5));

test("확정됐는데 칸이 비었으면(blur 가 xterm 보다 먼저 지움) 그 글자를 대신 보낸다", async () => {
  const { ta, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "한" });
  ta.value = ""; // blur 가 지운 칸
  ta.fire("compositionend", { data: "한" });
  await tick();
  assert.deepEqual(sent, ["한"]);
});

test("확정됐고 칸에 글자가 남아 있으면 xterm 이 보내므로 건드리지 않는다", async () => {
  const { ta, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "한" });
  ta.value = "한";
  ta.fire("compositionend", { data: "한" });
  await tick();
  assert.deepEqual(sent, []);
});

test("터미널 칸이 숨겨져 조합이 취소되면(\"\") 마지막 조합 글자를 확정해 보낸다", async () => {
  const { ta, doc, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "하" });
  ta.fire("compositionupdate", { data: "한" });
  doc.activeElement = { tagName: "BODY" };
  ta.fire("compositionend", { data: "" });
  await tick();
  assert.deepEqual(sent, ["한"]);
});

test("다른 입력 칸(탭 이름 칸 등)으로 포커스가 옮겨 갔으면 입력기가 거기에 쓰므로 보내지 않는다", async () => {
  const { ta, doc, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "한" });
  doc.activeElement = { tagName: "INPUT" };
  ta.fire("compositionend", { data: "" });
  await tick();
  assert.deepEqual(sent, []);
});

test("확정됐어도 포커스가 다른 입력 칸으로 옮겨 갔으면(Ctrl+Shift+R) 셸로 또 보내지 않는다", async () => {
  const { ta, doc, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "한" });
  ta.value = ""; // 포커스를 잃으며 xterm 이 비운 칸
  doc.activeElement = { tagName: "INPUT" };
  ta.fire("compositionend", { data: "한" });
  await tick();
  assert.deepEqual(sent, []);
});

test("Backspace 로 지워 비게 된 조합(포커스는 그대로)은 보낼 것이 없다", async () => {
  const { ta, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "ㅎ" });
  ta.fire("compositionupdate", { data: "" });
  ta.fire("compositionend", { data: "" });
  await tick();
  assert.deepEqual(sent, []);
});

test("조합이 이어지는 음절 경계(compositionend 직후 compositionstart)에서는 앞 글자를 다시 보내지 않는다", async () => {
  const { ta, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "한" });
  ta.value = "한";
  ta.fire("compositionend", { data: "한" });
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "ㄱ" });
  ta.value = "한ㄱ";
  await tick();
  assert.deepEqual(sent, []);
});

test("commit() 은 조합 중일 때만 blur 와 focus 로 확정한다", () => {
  const { ta, ime, calls } = setup();
  ime.commit();
  assert.deepEqual(calls, []);
  ta.fire("compositionstart");
  ime.commit();
  assert.deepEqual(calls, ["blur", "focus"]);
  ta.fire("compositionend", { data: "" });
  calls.length = 0;
  ime.commit();
  assert.deepEqual(calls, []);
});

test("dispose 는 새 리스너도 걷는다", () => {
  const { ta, ime } = setup();
  assert.equal(ta.count("compositionupdate"), 1);
  assert.equal(ta.count("compositionend"), 2);
  ime.dispose();
  assert.equal(ta.count("compositionupdate"), 0);
  assert.equal(ta.count("compositionend"), 0);
});

test("dispose 뒤에는 대기 중이던 복구 전송도 나가지 않는다", async () => {
  const { ta, ime, sent } = setup();
  ta.fire("compositionstart");
  ta.fire("compositionupdate", { data: "한" });
  ta.value = "";
  ta.fire("compositionend", { data: "한" });
  ime.dispose();
  await tick();
  assert.deepEqual(sent, []);
});
