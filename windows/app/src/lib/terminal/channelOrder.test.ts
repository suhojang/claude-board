// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/channelOrder.test.ts
// 백엔드(tauri 2.12.4 ipc/channel.rs)는 보낸 순서대로 index 를 달고, 1024바이트 미만 원시 메시지는 eval 로 곧바로,
// 큰 것은 fetch 를 거쳐 늦게 도착한다. 이 시험은 @tauri-apps/api 의 진짜 `Channel` 에 index 가 뒤섞인 채 도착시켜
// onmessage 가 보낸 순서대로 한 번씩만 불리는지 본다.
import { test } from "node:test";
import assert from "node:assert/strict";

const callbacks = new Map();
let nextId = 1;
globalThis.window = {
  __TAURI_INTERNALS__: {
    transformCallback(cb) {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
  },
};
const { Channel } = await import("@tauri-apps/api/core");

function bytes(n, tag) {
  const b = new Uint8Array(n);
  b[0] = tag & 255;
  b[1] = (tag >> 8) & 255;
  return b.buffer;
}

/** 백엔드가 보내는 모양({ message, index })을 도착 순서(`arrival`)대로 채널 콜백에 넣는다. */
function run(sizes, arrival) {
  const got = [];
  const ch = new Channel((m) => got.push(new Uint8Array(m)[0] | (new Uint8Array(m)[1] << 8)));
  const cb = callbacks.get(ch.id);
  for (const i of arrival) cb({ message: bytes(sizes[i], i), index: i });
  return got;
}

test("1024바이트 미만과 이상이 섞여 보내진 순서와 다르게 도착해도 보낸 순서로 전달된다", () => {
  // 작은 것(eval)은 곧바로, 큰 것(fetch)은 뒤늦게 — 도착 순서를 흉내 낸다
  const sizes = [];
  for (let i = 0; i < 400; i++) sizes.push(i % 3 === 0 ? 65536 : i % 3 === 1 ? 1023 : 1024);
  const small = [...sizes.keys()].filter((i) => sizes[i] < 1024);
  const big = [...sizes.keys()].filter((i) => sizes[i] >= 1024);
  const got = run(sizes, [...small, ...big]);
  assert.deepEqual(got, [...sizes.keys()]);
});

test("임의로 섞어 도착해도 빠짐·중복 없이 순서대로", () => {
  const n = 2000;
  const sizes = Array.from({ length: n }, (_, i) => (i % 2 ? 100 : 5000));
  let seed = 12345;
  const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
  const order = [...sizes.keys()];
  for (let i = n - 1; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }
  assert.deepEqual(run(sizes, order), [...sizes.keys()]);
});

test("먼저 온 index 가 빈 동안에는 아무것도 전달하지 않는다", () => {
  const got = [];
  const ch = new Channel((m) => got.push(new Uint8Array(m)[0]));
  const cb = callbacks.get(ch.id);
  cb({ message: bytes(2000, 1), index: 1 });
  cb({ message: bytes(10, 2), index: 2 });
  assert.deepEqual(got, []);
  cb({ message: bytes(10, 0), index: 0 });
  assert.deepEqual(got, [0, 1, 2]);
});
