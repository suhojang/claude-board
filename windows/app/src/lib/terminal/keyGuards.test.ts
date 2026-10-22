// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/keyGuards.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { confirmKeyAction, createRefireGuard } from "./keyGuards.ts";

const key = (code, extra = {}) => ({ code, keyCode: 229, isComposing: true, ctrlKey: true, altKey: false, shiftKey: false, timeStamp: 1000, ...extra });

test("조합 중에 처리한 단축키를 입력기가 다시 쏘면 한 번만 알아본다 (Ctrl+Alt+→ 가 두 칸 가던 것)", () => {
  const guard = createRefireGuard();
  const first = key("ArrowRight", { altKey: true });
  assert.equal(guard.isRefire(first), false);
  guard.handled(first);
  const again = key("ArrowRight", { altKey: true, keyCode: 39, timeStamp: 1002 });
  assert.equal(guard.isRefire(again), true);
  assert.equal(guard.isRefire(again), false, "한 번 알아보면 기억을 지운다");
});

test("다시 쏜 키는 isComposing 이 꺼져 있어도 알아본다 (Ctrl+T 가 셸로 새던 것)", () => {
  const guard = createRefireGuard();
  guard.handled(key("KeyT"));
  assert.equal(guard.isRefire(key("KeyT", { keyCode: 84, isComposing: false, timeStamp: 1003 })), true);
});

test("조합 중이 아니던 단축키 뒤에는 아무것도 기억하지 않는다", () => {
  const guard = createRefireGuard();
  guard.handled(key("ArrowRight", { altKey: true, keyCode: 39, isComposing: false }));
  assert.equal(guard.isRefire(key("ArrowRight", { altKey: true, keyCode: 39, isComposing: false, timeStamp: 1001 })), false);
});

test("다른 키·다른 수식키·시간이 지난 같은 키는 다시 쏜 것이 아니다", () => {
  const guard = createRefireGuard();
  guard.handled(key("ArrowRight", { altKey: true }));
  assert.equal(guard.isRefire(key("ArrowLeft", { altKey: true, timeStamp: 1001 })), false);
  guard.handled(key("ArrowRight", { altKey: true }));
  assert.equal(guard.isRefire(key("ArrowRight", { altKey: false, timeStamp: 1001 })), false);
  guard.handled(key("ArrowRight", { altKey: true }));
  assert.equal(guard.isRefire(key("ArrowRight", { altKey: true, timeStamp: 1500 })), false);
});

test("닫기 확인 창: Esc 는 어디서든 취소, 창 밖 키는 삼키고 창 안 키는 그대로", () => {
  assert.equal(confirmKeyAction("Escape", false), "cancel");
  assert.equal(confirmKeyAction("Escape", true), "cancel");
  assert.equal(confirmKeyAction("Enter", false), "swallow");
  assert.equal(confirmKeyAction("a", false), "swallow");
  assert.equal(confirmKeyAction("Enter", true), "pass");
  assert.equal(confirmKeyAction("Tab", true), "pass");
});
