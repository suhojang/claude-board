// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/board/openSession.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import { backgroundNotice, openSession } from "./openSession.ts";

const sessionOf = (over = {}) => ({ title: "빌드 고치기", mode: null, headless: false, paused: false, ...over });

test("tab — 한 번이든 두 번이든 그 화면을 reveal 한다", () => {
  for (const double of [false, true]) {
    assert.deepEqual(openSession(sessionOf(), { kind: "tab", surfaceId: "s1", headless: false }, double), { kind: "reveal", surfaceId: "s1" });
  }
});

test("tab 에 붙들린 끝난 세션도 reveal 이다", () => {
  const ended = sessionOf({ paused: true });
  assert.deepEqual(openSession(ended, { kind: "tab", surfaceId: "s9", headless: false }, true), { kind: "reveal", surfaceId: "s9" });
});

test("terminal — 한 번은 activate, 두 번은 focus", () => {
  assert.deepEqual(openSession(sessionOf(), { kind: "terminal" }, false), { kind: "action", action: "activate" });
  assert.deepEqual(openSession(sessionOf(), { kind: "terminal" }, true), { kind: "action", action: "focus" });
});

test("background — 한 번·두 번 모두 같은 알림", () => {
  const s = sessionOf({ mode: "--bg" });
  const once = openSession(s, { kind: "background" }, false);
  assert.deepEqual(once, { kind: "notice", text: backgroundNotice(s) });
  assert.deepEqual(openSession(s, { kind: "background" }, true), once);
});

test("알림 문구는 mode 가 없으면 -p 를 쓴다", () => {
  assert.equal(
    backgroundNotice(sessionOf()),
    "빌드 고치기: claude -p 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다.",
  );
  assert.ok(backgroundNotice(sessionOf({ mode: "--bg" })).includes("claude --bg 로"));
});

test("ended — 일시중지 표시가 없으면 carryOn false 로 reopen", () => {
  for (const double of [false, true]) {
    assert.deepEqual(openSession(sessionOf(), { kind: "ended" }, double), { kind: "action", action: "reopen", carryOn: false });
  }
});

test("ended + paused — carryOn true 로 reopen", () => {
  assert.deepEqual(openSession(sessionOf({ paused: true }), { kind: "ended" }, false), { kind: "action", action: "reopen", carryOn: true });
});

test("ended + headless — 아무것도 하지 않는다", () => {
  for (const double of [false, true]) {
    assert.deepEqual(openSession(sessionOf({ headless: true, mode: "-p" }), { kind: "ended" }, double), { kind: "none" });
  }
});

test("ended + headless + paused — 일시중지 표시가 있어도 아무것도 하지 않는다", () => {
  for (const double of [false, true]) {
    assert.deepEqual(openSession(sessionOf({ headless: true, paused: true }), { kind: "ended" }, double), { kind: "none" });
  }
});

test("tab — 화면 안 headless 소유(-p)여도 reveal 이고 알림을 내지 않는다", () => {
  const owned = sessionOf({ headless: true, mode: "-p" });
  for (const double of [false, true]) {
    assert.deepEqual(openSession(owned, { kind: "tab", surfaceId: "s2", headless: true }, double), { kind: "reveal", surfaceId: "s2" });
  }
});

test("terminal — paused·headless 표시는 동작을 바꾸지 않는다", () => {
  for (const over of [{ paused: true }, { headless: true }, { mode: "-p" }]) {
    assert.deepEqual(openSession(sessionOf(over), { kind: "terminal" }, false), { kind: "action", action: "activate" });
    assert.deepEqual(openSession(sessionOf(over), { kind: "terminal" }, true), { kind: "action", action: "focus" });
  }
});

test("background — 일시중지 표시와 무관하게 알림만, 제목이 문구 앞에 온다", () => {
  const s = sessionOf({ headless: true, paused: true, mode: "-p", title: "야간 배치" });
  const effect = openSession(s, { kind: "background" }, false);
  assert.equal(effect.kind, "notice");
  assert.ok(effect.text.startsWith("야간 배치: claude -p 로"));
});

test("모든 자리 × 한·두 번 — 효과는 정해진 네 종류뿐이고 입력 세션을 고치지 않는다", () => {
  const places = [{ kind: "tab", surfaceId: "s1", headless: false }, { kind: "terminal" }, { kind: "background" }, { kind: "ended" }];
  const s = sessionOf({ paused: true });
  const before = JSON.stringify(s);
  for (const place of places) {
    for (const double of [false, true]) {
      assert.ok(["reveal", "action", "notice", "none"].includes(openSession(s, place, double).kind));
    }
  }
  assert.equal(JSON.stringify(s), before);
});

test("reopen 의 carryOn 은 일시중지 표시를 그대로 따르고 클릭 횟수와 무관하다", () => {
  for (const paused of [true, false]) {
    for (const double of [false, true]) {
      assert.equal(openSession(sessionOf({ paused }), { kind: "ended" }, double).carryOn, paused);
    }
  }
});
