// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/resizeBarrier.fix.test.ts
// xterm 처럼 쓰기 콜백이 비동기로, 순서대로 오는 가짜 port 로 D1 리뷰 수정(M1·M2·D1~D3·L1~L3)을 시험한다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ResizeBarrier } from "./resizeBarrier.ts";

const E = "\x1b";
const enc = (s) => Uint8Array.from(Buffer.from(s, "latin1"));
const vis = (s) => s.replaceAll(E, "~");
const flushPromises = () => new Promise((resolve) => setImmediate(resolve));
const label = (s) => `${s.cols}x${s.rows}`;
const only = (events, prefix) => events.filter((e) => e.startsWith(prefix));
const SHRINK = { cols: 60, rows: 24 };
const PRE = `${E}[?25l${E}[8;24;60t\n`;

/** xterm 처럼: write·flush 는 대기열에 들어가고 pump 가 한 건씩 해석하며 콜백을 부른다 */
function harness({ cur = { cols: 80, rows: 24 }, options = {}, lazyFlush = false, asyncParse = true } = {}) {
  const events = [];
  let size = { ...cur };
  let clock = 0;
  let nextTimer = 1;
  const timers = new Map();
  const sends = [];
  const xterm = [];
  const flushes = [];
  const fail = { write: 0, apply: 0 };
  const port = {
    sendResize(s) {
      events.push(`send:${label(s)}`);
      return new Promise((resolve, reject) => sends.push({ size: s, resolve, reject }));
    },
    applyLocal(s) {
      if (fail.apply > 0) {
        fail.apply--;
        throw new Error("apply boom");
      }
      size = { ...s };
      events.push(`apply:${label(s)}`);
    },
    write(bytes, done) {
      if (fail.write > 0) {
        fail.write--;
        throw new Error("write boom");
      }
      events.push(`write:${vis(Buffer.from(bytes).toString("latin1"))}`);
      const item = { text: vis(Buffer.from(bytes).toString("latin1")), done };
      if (asyncParse) xterm.push(item);
      else {
        events.push(`parsed:${item.text}`);
        done();
      }
    },
    flush(done) {
      events.push("flush");
      if (lazyFlush) flushes.push(done);
      else if (asyncParse) xterm.push({ text: null, done });
      else done();
    },
    current: () => ({ ...size }),
    altScreen: () => false,
    bound: () => true,
    setTimer(fn, ms) {
      const id = nextTimer++;
      timers.set(id, { at: clock + ms, fn });
      return id;
    },
    clearTimer: (id) => timers.delete(id),
    log: (m) => events.push(`log:${m}`),
  };
  let seq = 0;
  let frameId = 0;
  const h = {
    events,
    sends,
    timers,
    fail,
    port,
    barrier: new ResizeBarrier(port, options),
    frame(text) {
      const id = ++frameId;
      const bytes = enc(text);
      h.barrier.output(seq, bytes, () => events.push(`done:${id}`));
      seq += bytes.length;
      return id;
    },
    get seq() {
      return seq;
    },
    tick(ms) {
      clock += ms;
      for (const [id, t] of [...timers].sort((a, b) => a[1].at - b[1].at)) {
        if (t.at <= clock && timers.has(id)) {
          timers.delete(id);
          t.fn();
        }
      }
    },
    async resolveSend(index, anchor) {
      sends[index].resolve(anchor);
      await flushPromises();
    },
    async rejectSend(index) {
      sends[index].reject(new Error("boom"));
      await flushPromises();
    },
    /** xterm 이 대기열에서 n 건을 해석한다 */
    pump(n = Infinity) {
      while (n-- > 0 && xterm.length > 0) {
        const item = xterm.shift();
        if (item.text !== null) events.push(`parsed:${item.text}`);
        item.done();
      }
    },
    completeFlush: () => flushes.shift()(),
    size: () => size,
  };
  return h;
}

const indexOfEvent = (events, name) => events.indexOf(name);
const lastParsed = (events) => events.map((e, i) => (e.startsWith("parsed:") ? i : -1)).filter((i) => i >= 0).at(-1);

test("M1 cut A 가 프레임 맨 앞이어도 앞서 쓴 바이트가 해석된 뒤에 적용한다", async () => {
  const h = harness();
  h.barrier.request(SHRINK);
  h.pump(); // 계획 flush
  await h.resolveSend(0, 0);
  h.frame(`abc${PRE}`); // 머리말까지 안전하게 써진다
  h.frame(`${E}[Hrepaint`); // cut 은 이 프레임 맨 앞: cut 아래에 남은 조각이 없다
  assert.equal(only(h.events, "apply").length, 0, "해석 전에는 바꾸지 않는다");
  h.pump();
  const applyAt = indexOfEvent(h.events, "apply:60x24");
  assert.ok(applyAt > lastParsed(h.events.slice(0, applyAt)), "apply 앞에 parsed 가 있다");
  const before = h.events.slice(0, applyAt);
  assert.ok(before.includes(`parsed:abc${vis(PRE)}`), "앞서 쓴 바이트가 먼저 해석됐다");
  assert.ok(!before.includes(`parsed:${vis(`${E}[Hrepaint`)}`), "cut 뒤는 apply 뒤에 쓰인다");
});

test("M1 폴백 유휴 컷도 앞서 쓴 바이트가 해석된 뒤에 적용한다", async () => {
  const h = harness();
  h.barrier.request(SHRINK);
  h.pump();
  await h.resolveSend(0, 0);
  h.tick(1000);
  h.frame("abc");
  h.tick(50);
  assert.equal(only(h.events, "apply").length, 0, "abc 가 아직 해석 전");
  h.pump();
  const applyAt = indexOfEvent(h.events, "apply:60x24");
  assert.ok(applyAt > h.events.indexOf("parsed:abc"));
});

test("M1 쓰기가 하나도 밀려 있지 않으면 바로 적용한다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.resolveSend(0, 0);
  h.tick(1000);
  h.tick(50);
  assert.ok(h.events.includes("apply:60x24"));
});

test("M2 force 요청은 같은 크기여도 백엔드에 한 번 알린다", async () => {
  const h = harness();
  h.barrier.request({ cols: 80, rows: 24 });
  h.pump();
  assert.equal(h.sends.length, 0, "force 없이는 같은 크기를 보내지 않는다");
  h.barrier.request({ cols: 80, rows: 24 }, { force: true });
  h.pump();
  assert.deepEqual(only(h.events, "send"), ["send:80x24"]);
  await h.resolveSend(0, null);
  h.pump();
  assert.deepEqual(h.size(), { cols: 80, rows: 24 });
  h.frame("next");
  assert.equal(h.events.at(-1), "write:next", "끝난 뒤에는 Idle");
});

test("M2 force 요청에 백엔드 크기가 달랐으면 머리말을 찾아 평소처럼 적용한다", async () => {
  const h = harness();
  h.barrier.request({ cols: 80, rows: 24 }, { force: true });
  h.pump();
  await h.resolveSend(0, 0);
  h.frame(`${PRE.replace("60t", "80t")}${E}[Hx`);
  h.pump();
  assert.ok(h.events.includes("apply:80x24"));
});

test("D1 계획 중 gap 이면 붙잡은 프레임이 RIS 보다 먼저 쓰인다", () => {
  const h = harness({ lazyFlush: true });
  h.barrier.request(SHRINK);
  h.frame("held");
  assert.equal(only(h.events, "write").length, 0);
  h.barrier.interrupt("gap");
  h.port.write(enc(`${E}c`), () => {}); // term.ts 가 gap 직후 쓰는 RIS
  assert.deepEqual(only(h.events, "write"), ["write:held", "write:~c"]);
});

test("D1 스캔 중 gap 이면 붙잡은 꼬리, RIS, 그 뒤 크기 적용 순서다", async () => {
  const h = harness();
  h.barrier.request(SHRINK);
  h.pump();
  await h.resolveSend(0, 0);
  h.frame(`a${E}`);
  h.barrier.interrupt("gap");
  h.port.write(enc(`${E}c`), () => {});
  h.pump();
  assert.deepEqual(only(h.events, "write"), ["write:a", "write:~", "write:~c"]);
  assert.equal(only(h.events, "apply").length, 1);
  assert.equal(h.timers.size, 0);
});

test("D2 CR 만 흐르는 스트림도 폴백 1500ms 안에 자르고 적용한다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.resolveSend(0, 0);
  h.tick(1000); // 기한: 줄 폴백
  for (let t = 0; t < 1450; t += 40) {
    h.frame("xx\r");
    h.tick(40);
  }
  assert.equal(only(h.events, "apply").length, 0, "상한 전");
  for (let t = 0; t < 120; t += 40) {
    h.frame("xx\r");
    h.tick(40);
  }
  assert.equal(only(h.events, "apply").length, 1, "1500ms 상한에서 잘렸다");
  assert.ok(only(h.events, "log").some((l) => l.includes("1500ms")));
});

test("D2 폴백 바이트가 상한을 넘으면 프레임 끝에서 자른다", async () => {
  const h = harness({ asyncParse: false, options: { fallbackMaxBytes: 100 } });
  h.barrier.request(SHRINK);
  await h.resolveSend(0, 0);
  h.tick(1000);
  h.frame("x".repeat(60));
  assert.equal(only(h.events, "apply").length, 0);
  h.frame("y".repeat(60));
  assert.ok(h.events.includes("apply:60x24"));
  assert.ok(only(h.events, "log").some((l) => l.includes("바이트 상한")));
});

test("D3 termResize 가 거부되면 적용하지 않고 500ms 뒤 한 번 다시 보낸다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  h.frame("held");
  await h.rejectSend(0);
  assert.equal(only(h.events, "apply").length, 0, "확인 없이 바꾸지 않는다");
  assert.ok(h.events.includes("write:held"), "붙잡은 프레임은 푼다");
  h.tick(499);
  assert.equal(h.sends.length, 1);
  h.tick(1);
  assert.equal(h.sends.length, 2, "정확히 한 번 다시 보냈다");
  await h.resolveSend(1, null);
  assert.ok(h.events.includes("apply:60x24"), "재시도가 확인되면 적용한다");
});

test("D3 연달아 3번 거부되면 포기하고 타이머를 남기지 않는다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.rejectSend(0);
  h.tick(500);
  await h.rejectSend(1);
  h.tick(500);
  await h.rejectSend(2);
  assert.equal(h.sends.length, 3);
  assert.equal(h.timers.size, 0);
  h.tick(5000);
  assert.equal(h.sends.length, 3, "더 보내지 않는다");
  assert.equal(only(h.events, "apply").length, 0);
  assert.ok(only(h.events, "log").some((l) => l.includes("포기")));
});

test("D3 재시도 전에 새 요청이 오면 그 요청만 계획한다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.rejectSend(0);
  h.barrier.request({ cols: 50, rows: 24 });
  assert.deepEqual(only(h.events, "send"), ["send:60x24", "send:50x24"]);
  h.tick(5000);
  assert.equal(h.sends.length, 2, "낡은 재시도는 취소됐다");
});

test("L1 계획 중 exit 이면 붙잡은 프레임이 onExit 쓰기보다 먼저 쓰인다", () => {
  const h = harness({ lazyFlush: true });
  h.barrier.request(SHRINK);
  h.frame("held");
  h.barrier.interrupt("exit");
  h.port.write(enc(""), () => {}); // term.ts 의 onExit 빈 쓰기
  assert.deepEqual(only(h.events, "write"), ["write:held", "write:"]);
});

test("L2 cut 앞을 쓰다 port 가 던져도 Idle 로 돌아가 붙잡은 프레임을 푼다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.resolveSend(0, 0);
  h.fail.write = 1;
  h.frame(`${PRE}${E}[Hx`);
  assert.ok(only(h.events, "log").some((l) => l.includes("내부 오류")));
  h.frame("after");
  assert.ok(h.events.includes("write:after"), "Idle 이라 바로 쓴다");
  h.barrier.request({ cols: 50, rows: 24 });
  assert.equal(h.sends.length, 2, "다시 요청을 받는다");
  assert.equal(h.timers.size, 0);
});

test("L2 applyLocal 이 던져도 붙잡은 프레임을 풀고 Idle 이 된다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.resolveSend(0, 0);
  h.fail.apply = 1;
  h.frame(`${PRE}${E}[Hx`);
  assert.ok(h.events.includes(`write:${vis(`${E}[Hx`)}`), "나머지가 풀렸다");
  h.frame("next");
  assert.ok(h.events.includes("write:next"));
});

test("L3 reset 은 진행 중인 단계를 버리고 붙잡은 프레임을 풀며 늦은 응답을 무시한다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.resolveSend(0, 0);
  h.frame("a");
  h.frame(`b${E}`);
  h.barrier.reset();
  assert.equal(h.timers.size, 0);
  assert.ok(h.events.includes("write:~"), "붙잡은 꼬리가 풀렸다");
  const count = h.events.length;
  h.frame("fresh");
  assert.ok(h.events.includes("write:fresh"));
  assert.ok(h.events.length > count);
  assert.equal(only(h.events, "apply").length, 0, "낡은 단계는 크기를 적용하지 않는다");
  h.barrier.request({ cols: 50, rows: 24 });
  assert.deepEqual(only(h.events, "send"), ["send:60x24", "send:50x24"]);
});

test("L3 reset 은 대기 중인 재시도를 취소한다", async () => {
  const h = harness({ asyncParse: false });
  h.barrier.request(SHRINK);
  await h.rejectSend(0);
  assert.equal(h.timers.size, 1);
  h.barrier.reset();
  assert.equal(h.timers.size, 0);
});
