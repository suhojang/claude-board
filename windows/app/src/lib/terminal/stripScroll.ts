// 가로로 미는 줄(탭 줄·경로 줄)의 순수 규칙 — 휠 처리와 고른 항목을 보이게 밀 위치. DOM 은 두 Svelte 가 만진다.
import { scrollTargetFor } from "./paneLayout.ts";

/** 휠 한 번이 줄을 얼마나 밀어야 하는가.
 *  가로 값(트랙패드 가로 밀기, 브라우저가 Shift+휠을 가로 값으로 바꿔 준 경우)은 브라우저에 맡긴다.
 *  세로 값만 오면(보통 휠, Shift+휠이 세로 값으로 온 경우) 우리가 그만큼 밀고 기본 동작은 막는다 — 안 막으면 Shift+휠이 두 번 밀린다. */
export function wheelStep(deltaX: number, deltaY: number, _shiftKey: boolean): { add: number; prevent: boolean } {
  if (deltaX !== 0 || deltaY === 0) return { add: 0, prevent: false };
  return { add: deltaY, prevent: true };
}

/** 고른 항목(`left`..`right`, 안쪽 좌표)이 칸 안에 여백까지 보이도록 옮길 scrollLeft(가장 가까운 만큼만, 0..끝 사이). */
export function scrollTargetClamped(left: number, right: number, scrollLeft: number, clientWidth: number, scrollWidth: number, margin: number): number {
  const max = Math.max(0, scrollWidth - clientWidth);
  const target = scrollTargetFor(left - margin, right + margin, scrollLeft, clientWidth);
  return Math.min(max, Math.max(0, target));
}
