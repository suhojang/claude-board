// 칸 크기 계산 — 보드 칸 폭 범위와 탭 줄의 탭 폭. 화면(+page.svelte·TabBar.svelte)이 쓰고, 외부를 import 하지 않아 node:test 로 시험한다.

/** 보드 칸은 360 이상, 터미널 칸은 480 이상 남긴다. 경계는 8 (논리 px) */
export const MIN_BOARD = 360;
export const MIN_TERMINAL = 480;
export const DIVIDER = 8;
export const DEFAULT_BOARD_WIDTH = 454;

/** 창 폭에서 보드 칸이 가질 수 있는 가장 큰 폭 — 창이 좁아도 MIN_BOARD 아래로 내려가지 않는다 */
export function maxBoardWidth(windowWidth: number): number {
  return Math.max(MIN_BOARD, windowWidth - DIVIDER - MIN_TERMINAL);
}

/** 보드 칸 폭 — 끄는 중의 값 → 저장된 값 → 기본값 순으로 고르고 [MIN_BOARD, maxBoardWidth] 로 자른다 */
export function clampBoardWidth(dragging: number | null, saved: number | null | undefined, windowWidth: number): number {
  return Math.min(Math.max(dragging ?? saved ?? DEFAULT_BOARD_WIDTH, MIN_BOARD), maxBoardWidth(windowWidth));
}

export const TAB_MIN_WIDTH = 120;
export const TAB_MAX_WIDTH = 250;
export const TAB_GAP = 2;
/** 탭 줄 좌우 안쪽 여백 합(8 + 8) */
export const TAB_BAR_PADDING = 16;

/** 탭 칸에 줄 폭 — 탭이 모두 들어가면 탭마다 가장 넓게, 아니면 + 단추를 뺀 남는 폭 전부 */
export function tabStripWidth(barWidth: number, newButtonWidth: number, count: number): number {
  return Math.max(0, Math.min(barWidth - TAB_BAR_PADDING - newButtonWidth, count * (TAB_MAX_WIDTH + TAB_GAP) - TAB_GAP));
}

/** 탭을 가장 좁게 놓았을 때의 안쪽 폭 — 칸(tabStripWidth)보다 크면 가로로 밀린다 */
export function tabStripLeast(count: number): number {
  return Math.max(0, count * (TAB_MIN_WIDTH + TAB_GAP) - TAB_GAP);
}

/** 가로로 밀 때 고른 항목이 칸 안에 보이도록 옮길 scrollLeft (가장 가까운 만큼만). 이미 보이면 그대로 */
export function scrollTargetFor(left: number, right: number, scrollLeft: number, clientWidth: number): number {
  if (left < scrollLeft) return left;
  if (right > scrollLeft + clientWidth) return right - clientWidth;
  return scrollLeft;
}
