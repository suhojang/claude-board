// 캐릭터 동작의 공유 시계 — 모든 캐릭터가 시계 하나를 함께 본다.
// 따로 돌면 캐릭터마다 화면을 따로 다시 그리고, 화면 주사율만큼 그리면 보드가 버벅인다.
// 보이는 세션 가운데 작업 중·승인 대기가 하나라도 있으면 초당 30번(requestAnimationFrame),
// 없으면 초당 5번(setTimeout)으로 늦춘다(mac AmbientTicker.busy). 보는 캐릭터가 없거나 창이 숨으면(document.hidden) 멈춘다.
// 늦춘 동안에도 호출한 쪽(clock.t 를 읽는 캐릭터)은 그대로 그려지고, 말풍선·눈 깜빡임 타이머는 이 시계와 상관없다.
// 세션이 다시 일하기 시작하면 늦춘 시계가 한 번 더 깨어나는 때(최대 0.2초) 바로 초당 30번으로 돌아간다.
import { createSubscriber } from "svelte/reactivity";
import { board } from "../board/store.svelte";
import type { State } from "../types";

export const ACTIVE_FPS = 30;
export const IDLE_FPS = 5;
// 한 프레임의 간격(ms). 주사율이 맞지 않아 프레임이 조금 일찍 와도 그 프레임을 쓰도록 ACTIVE 는 2ms 여유를 둔다
export const ACTIVE_MS = 1000 / ACTIVE_FPS;
export const IDLE_MS = 1000 / IDLE_FPS;
const SLACK_MS = 2;

type Counts = Partial<Record<State, number>> | null | undefined;

// 프레임 간격 결정(순수) — 개수를 모르면(보드 상태가 아직 없다: 알림 패널·첫 화면) 바쁜 것으로 본다(mac busy 의 처음 값 true)
export function frameInterval(counts: Counts): number {
  if (!counts) return ACTIVE_MS;
  return (counts.working ?? 0) > 0 || (counts.waiting ?? 0) > 0 ? ACTIVE_MS : IDLE_MS;
}

// 시계를 돌리는 쪽이 주는 것 — 시험은 가짜로 갈아 끼운다
export interface LoopHost {
  raf(cb: (ms: number) => void): number;
  caf(id: number): void;
  after(fn: () => void, ms: number): number;
  cancelAfter(id: number): void;
  now(): number;
  hidden(): boolean;
  counts(): Counts;
  tick(ms: number): void;
}

// 시계 한 개 — 바쁠 때는 rAF 로 초당 30번, 한가할 때는 타이머로 초당 5번만 깨어난다(그 사이엔 화면 갱신 호출도 없다).
// 깰 때마다 개수를 다시 읽어 다음 깨움 방식을 고른다
export function createLoop(host: LoopHost) {
  let running = false;
  let raf = 0;
  let timer = 0;
  let last = -Infinity;

  function pump() {
    if (!running) return;
    if (frameInterval(host.counts()) === IDLE_MS) timer = host.after(idleTick, IDLE_MS);
    else raf = host.raf(frame);
  }
  function frame(ms: number) {
    raf = 0;
    if (ms - last >= ACTIVE_MS - SLACK_MS) {
      last = ms;
      host.tick(ms);
    }
    pump();
  }
  function idleTick() {
    timer = 0;
    last = host.now();
    host.tick(last);
    pump();
  }
  function clear() {
    if (raf) host.caf(raf);
    if (timer) host.cancelAfter(timer);
    raf = 0;
    timer = 0;
  }

  return {
    start() {
      if (running || host.hidden()) return;
      running = true;
      pump();
    },
    stop() {
      running = false;
      clear();
    },
  };
}

let now = performance.now() / 1000;

const subscribe = createSubscriber((update) => {
  const loop = createLoop({
    raf: (cb) => requestAnimationFrame(cb),
    caf: (id) => cancelAnimationFrame(id),
    after: (fn, ms) => window.setTimeout(fn, ms),
    cancelAfter: (id) => clearTimeout(id),
    now: () => performance.now(),
    hidden: () => document.hidden,
    counts: () => board.snap?.counts,
    tick: (ms) => {
      now = ms / 1000;
      update();
    },
  });
  const visibility = () => (document.hidden ? loop.stop() : loop.start());
  document.addEventListener("visibilitychange", visibility);
  loop.start();
  return () => {
    loop.stop();
    document.removeEventListener("visibilitychange", visibility);
  };
});

export const clock = {
  // 지금 시각(초) — 읽는 쪽은 바쁠 때 초당 30번, 한가할 때 초당 5번 다시 그려진다
  get t() {
    subscribe();
    return now;
  },
  // 다시 그리기를 걸지 않고 지금 시각만 읽는다(말풍선이 바뀐 시각 기록용)
  peek: () => performance.now() / 1000,
};
