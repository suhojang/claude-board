// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/groups/loginLaunch.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { changeLoginLaunch, LOGIN_LAUNCH_FAILED, LOGIN_LAUNCH_UNKNOWN } from "./loginLaunch.ts";

const resolves = (value) => async () => value;
const rejects = async () => {
  throw new Error("실패");
};

// [이름, 누른 값, 누르기 전 값, set, read, 기대]
const TABLE = [
  ["켰고 실제로 켜졌다", true, false, async () => {}, resolves(true), { value: true, note: null }],
  ["껐고 실제로 꺼졌다", false, true, async () => {}, resolves(false), { value: false, note: null }],
  ["set 은 끝났지만 실제 값이 그대로 — 읽은 값을 보이고 안내한다", true, false, async () => {}, resolves(false), { value: false, note: LOGIN_LAUNCH_FAILED }],
  ["set 이 실패 — 누르기 전 값으로 되돌리고 안내한다", true, false, rejects, resolves(true), { value: false, note: LOGIN_LAUNCH_FAILED }],
  ["set 이 실패 — 끄려던 것도 되돌린다", false, true, rejects, resolves(false), { value: true, note: LOGIN_LAUNCH_FAILED }],
  ["set 은 됐는데 읽기가 실패 — 되돌리고 확인 못 했다고 안내한다", true, false, async () => {}, rejects, { value: false, note: LOGIN_LAUNCH_UNKNOWN }],
];

for (const [name, on, previous, set, read, want] of TABLE) {
  test(`changeLoginLaunch: ${name}`, async () => assert.deepEqual(await changeLoginLaunch(on, previous, { set, read }), want));
}

test("changeLoginLaunch: set 이 끝난 뒤에 읽는다", async () => {
  const order = [];
  await changeLoginLaunch(true, false, {
    set: async () => {
      await Promise.resolve();
      order.push("set");
    },
    read: async () => {
      order.push("read");
      return true;
    },
  });
  assert.deepEqual(order, ["set", "read"]);
});
