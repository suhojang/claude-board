// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 캐릭터 공유 시계의 프레임 간격 결정(frameInterval, 순수)과 시계 전환(createLoop, 가짜 rAF·타이머)을 시험한다.
// clock.svelte.ts 는 store.svelte.ts 를 가져오므로 startState.test.ts 와 같은 방식으로 읽는다:
// `module.registerHooks` 로 `.svelte.ts` 를 compileModule 로 바꾸고 `$lib/backend` 는 가짜 모듈로 돌린다.
// 실행: cd windows/app && node --test src/lib/mascot/clock.test.ts
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { registerHooks, stripTypeScriptTypes } from "node:module";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { compileModule } from "svelte/compiler";

const BACKEND_URL = "cb-test:backend";

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "$lib/backend") return { url: BACKEND_URL, shortCircuit: true };
    if (specifier === "../board/store.svelte") return nextResolve("../board/store.svelte.ts", context);
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url === BACKEND_URL) {
      return { format: "module", source: "export const onSnapshot = () => () => {};\n", shortCircuit: true };
    }
    if (url.endsWith(".svelte.ts")) {
      const ts = readFileSync(fileURLToPath(url), "utf8");
      const js = stripTypeScriptTypes(ts);
      const { js: out } = compileModule(js, { generate: "client", filename: fileURLToPath(url) });
      return { format: "module", source: out.code, shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});

const m = await import("./clock.svelte.ts");
const { frameInterval, createLoop, ACTIVE_MS, IDLE_MS, ACTIVE_FPS, IDLE_FPS } = m;

// ── frameInterval ────────────────────────────────────────────────

test("상수: 바쁠 때 초당 30번, 한가할 때 초당 5번", () => {
  assert.equal(ACTIVE_FPS, 30);
  assert.equal(IDLE_FPS, 5);
  assert.equal(ACTIVE_MS, 1000 / 30);
  assert.equal(IDLE_MS, 200);
});

test("작업 중이 하나만 있어도 초당 30번", () => {
  assert.equal(frameInterval({ working: 1, waiting: 0, idle: 0, paused: 0, ended: 0 }), ACTIVE_MS);
});

test("승인 대기가 하나만 있어도 초당 30번", () => {
  assert.equal(frameInterval({ working: 0, waiting: 1, idle: 0, paused: 0, ended: 0 }), ACTIVE_MS);
});

test("작업 중과 승인 대기가 모두 0 이면 초당 5번", () => {
  assert.equal(frameInterval({ working: 0, waiting: 0, idle: 3, paused: 1, ended: 2 }), IDLE_MS);
});

test("대기·일시중지·종료만 있으면 초당 5번", () => {
  assert.equal(frameInterval({ idle: 5 }), IDLE_MS);
  assert.equal(frameInterval({ paused: 5 }), IDLE_MS);
  assert.equal(frameInterval({ ended: 5 }), IDLE_MS);
});

test("세션이 하나도 없으면 초당 5번", () => {
  assert.equal(frameInterval({ working: 0, waiting: 0, idle: 0, paused: 0, ended: 0 }), IDLE_MS);
  assert.equal(frameInterval({}), IDLE_MS);
});

test("보드 상태가 아직 없으면(null·undefined) 바쁜 것으로 본다", () => {
  assert.equal(frameInterval(null), ACTIVE_MS);
  assert.equal(frameInterval(undefined), ACTIVE_MS);
});

test("작업 중과 승인 대기가 둘 다 있어도 초당 30번", () => {
  assert.equal(frameInterval({ working: 2, waiting: 4 }), ACTIVE_MS);
});

// ── createLoop: 가짜 rAF·타이머 ──────────────────────────────────

// 시각 now 를 직접 흘리는 가짜 환경. rAF 는 16ms(주사율 약 60Hz)마다, 타이머는 정한 시각에 부른다
function fakeHost(counts) {
  const env = {
    now: 1000,
    counts,
    hidden: false,
    ticks: [],
    rafCalls: 0,
    timerCalls: 0,
    cancelled: 0,
    rafQ: new Map(),
    timerQ: new Map(),
    id: 0,
  };
  env.host = {
    raf: (cb) => {
      env.rafCalls += 1;
      env.rafQ.set(++env.id, cb);
      return env.id;
    },
    caf: (id) => {
      env.cancelled += 1;
      env.rafQ.delete(id);
    },
    after: (fn, ms) => {
      env.timerCalls += 1;
      env.timerQ.set(++env.id, { fn, at: env.now + ms });
      return env.id;
    },
    cancelAfter: (id) => {
      env.cancelled += 1;
      env.timerQ.delete(id);
    },
    now: () => env.now,
    hidden: () => env.hidden,
    counts: () => env.counts,
    tick: (ms) => env.ticks.push(ms),
  };
  // 1ms 씩 흘리면서 16ms 마다 rAF 를, 정한 시각이 된 타이머를 부른다
  env.advance = (ms) => {
    for (let i = 0; i < ms; i++) {
      env.now += 1;
      if (env.now % 16 === 0) {
        const q = [...env.rafQ];
        env.rafQ.clear();
        for (const [, cb] of q) cb(env.now);
      }
      for (const [id, t] of [...env.timerQ]) {
        if (t.at <= env.now) {
          env.timerQ.delete(id);
          t.fn();
        }
      }
    }
  };
  return env;
}

const BUSY = { working: 1, waiting: 0, idle: 0, paused: 0, ended: 0 };
const QUIET = { working: 0, waiting: 0, idle: 2, paused: 0, ended: 0 };

test("바쁠 때 1초에 약 30번 그린다(타이머는 쓰지 않는다)", () => {
  const e = fakeHost(BUSY);
  const loop = createLoop(e.host);
  loop.start();
  e.advance(1000);
  // 16ms 간격 rAF 중 31.3ms 이상 벌어진 것만 쓰므로 두 프레임(32ms)마다 하나, 1초에 31번
  assert.ok(e.ticks.length >= 28 && e.ticks.length <= 32, `ticks=${e.ticks.length}`);
  assert.equal(e.timerCalls, 0);
  loop.stop();
});

test("한가하면 1초에 5번만 그린다(rAF 는 쓰지 않는다)", () => {
  const e = fakeHost(QUIET);
  const loop = createLoop(e.host);
  loop.start();
  e.advance(1000);
  assert.equal(e.ticks.length, 5);
  assert.equal(e.rafCalls, 0);
  loop.stop();
});

test("한가한 시계는 200ms 간격으로 깬다", () => {
  const e = fakeHost(QUIET);
  const loop = createLoop(e.host);
  loop.start();
  e.advance(1000);
  for (let i = 1; i < e.ticks.length; i++) assert.equal(e.ticks[i] - e.ticks[i - 1], 200);
  loop.stop();
});

test("한가하다가 일을 시작하면 다음 깨움(최대 0.2초)에 초당 30번 방식으로 돌아온다", () => {
  const e = fakeHost(QUIET);
  const loop = createLoop(e.host);
  loop.start();
  e.advance(500);
  const before = e.ticks.length;
  assert.equal(before, 2);
  e.counts = BUSY;
  e.advance(200);
  assert.ok(e.rafCalls >= 1, "rAF 로 바뀌었다");
  e.rafCalls = 0;
  e.timerCalls = 0;
  e.advance(1000);
  assert.equal(e.timerCalls, 0);
  assert.ok(e.ticks.length - before >= 28, `ticks=${e.ticks.length - before}`);
  loop.stop();
});

test("바쁘다가 모두 쉬면 다음 프레임에 초당 5번 방식으로 바뀐다", () => {
  const e = fakeHost(BUSY);
  const loop = createLoop(e.host);
  loop.start();
  e.advance(300);
  e.counts = QUIET;
  e.advance(100);
  e.rafCalls = 0;
  const before = e.ticks.length;
  e.advance(1000);
  assert.equal(e.rafCalls, 0);
  assert.equal(e.ticks.length - before, 5);
  loop.stop();
});

test("창이 숨은 채로는 시작하지 않는다", () => {
  const e = fakeHost(BUSY);
  e.hidden = true;
  const loop = createLoop(e.host);
  loop.start();
  e.advance(1000);
  assert.equal(e.ticks.length, 0);
  assert.equal(e.rafCalls + e.timerCalls, 0);
});

test("stop 하면 대기 중인 rAF·타이머를 취소하고 더는 그리지 않는다", () => {
  for (const counts of [BUSY, QUIET]) {
    const e = fakeHost(counts);
    const loop = createLoop(e.host);
    loop.start();
    e.advance(300);
    loop.stop();
    const n = e.ticks.length;
    e.advance(1000);
    assert.equal(e.ticks.length, n);
    assert.equal(e.rafQ.size + e.timerQ.size, 0);
  }
});

test("stop 뒤 다시 start 하면 이어서 돈다, start 를 두 번 불러도 겹치지 않는다", () => {
  const e = fakeHost(QUIET);
  const loop = createLoop(e.host);
  loop.start();
  loop.start();
  e.advance(1000);
  assert.equal(e.ticks.length, 5);
  loop.stop();
  loop.start();
  e.advance(1000);
  assert.equal(e.ticks.length, 10);
  loop.stop();
});
