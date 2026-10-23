// 칸 크기 계산 — 보드 칸 폭 범위와 탭 줄의 탭 폭. 화면(+page.svelte·TerminalTabBar.svelte)이 쓰고, 외부를 import 하지 않아 node:test 로 시험한다.

// 칸 폭 상수(논리 px) — 정본은 Rust window.rs 이고(mac ClaudeBoard.swift 의 같은 이름), 여기는 값을 맞춰 둔 사본이다. 값을 바꾸면 둘을 함께 고친다.
/** 보드 칸이 남길 최소 폭 — window.rs BOARD_MIN_WIDTH */
export const boardMinWidth = 360;
/** 터미널 칸이 남길 최소 폭 — window.rs TERMINAL_MIN_WIDTH */
export const terminalMinWidth = 480;
/** 두 칸 사이 경계 폭 — window.rs PANE_DIVIDER_WIDTH */
export const paneDividerWidth = 8;
/** 저장된 폭이 없을 때 보드 칸의 처음 폭 — window.rs BOARD_LAUNCH_WIDTH */
export const boardLaunchWidth = 454;

/** 창 폭에서 보드 칸이 가질 수 있는 가장 큰 폭 — 창이 좁아도 boardMinWidth 아래로 내려가지 않는다 */
export function maxBoardWidth(windowWidth: number): number {
  return Math.max(boardMinWidth, windowWidth - paneDividerWidth - terminalMinWidth);
}

/** 보드 칸 폭 — 끄는 중의 값 → 저장된 값 → 기본값 순으로 고르고 [boardMinWidth, maxBoardWidth] 로 자른다 */
export function clampBoardWidth(dragging: number | null, saved: number | null | undefined, windowWidth: number): number {
  return Math.min(Math.max(dragging ?? saved ?? boardLaunchWidth, boardMinWidth), maxBoardWidth(windowWidth));
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
