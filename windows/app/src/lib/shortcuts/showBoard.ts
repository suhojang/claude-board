// 보드 보기·앞뒤 탭 셋째 키(mac `Shortcuts.actions` 의 showBoard·prevTab3·nextTab3) — 표 행과 실행 판단만 둔 순수 모듈이다.
// actions.ts 가 `BOARD_VIEW_ACTIONS` 를 표에 펼쳐 넣고, run.ts 가 `TAB_STEP`·`showBoardPlan` 으로 잇는다.
//
// 기본 키
// - showBoard: mac ⌘0 → Ctrl+Alt+0. Ctrl+0 은 터미널 글자 원래 크기(D6)와 xterm·셸이 쓸 수 있어 피했고, tab1~9 가 Ctrl+Alt+1~9 라 0 이 비어 있다.
// - prevTab3·nextTab3: mac ⌃⇧⇥·⌃⇥ 는 Windows 에서 prevTab2·nextTab2 의 기본 키(Ctrl+Shift+Tab·Ctrl+Tab)와 같은 키라 기본 키를 두지 않는다
//   (같은 키 두 동작은 충돌 표시만 만든다, 결정 D6·C-261·C-262). 동작은 둘째 키와 같고, 사용자가 다른 키를 묶을 수 있게 id 만 있다.
//   mac 도 세 키가 각각 다른 동작 id 이고 효과가 같다(Shortcuts.swift:106-109) — 한 동작에 키 셋을 두는 모양이 아니라 id 셋을 두는 모양이 mac 표와 같다.
// import 하는 것은 keyCombo.ts 뿐이다(actions.ts 와 순환을 만들지 않으려고 표 행 모양을 여기서 따로 쓴다).
import { MOD, keyOf, type KeyCombo } from "./keyCombo.ts";

export interface BoardViewAction {
  id: string;
  group: string;
  title: string;
  initial: KeyCombo | null;
}

export const SHOW_BOARD = "showBoard";
export const PREV_TAB3 = "prevTab3";
export const NEXT_TAB3 = "nextTab3";

/** actions.ts 표에 그대로 펼쳐 넣는 행 — showBoard 는 mac 표처럼 「보드」 묶음의 맨 앞, 탭 둘은 nextTab2 뒤 */
export const BOARD_VIEW_ACTIONS: readonly BoardViewAction[] = [
  { id: SHOW_BOARD, group: "보드", title: "세션 보드 보기", initial: keyOf("Digit0", MOD.ctrl | MOD.alt) },
  { id: PREV_TAB3, group: "탭으로 가기", title: "이전 탭 (보조 2)", initial: null },
  { id: NEXT_TAB3, group: "탭으로 가기", title: "다음 탭 (보조 2)", initial: null },
];

/** 이전·다음 탭 동작 id → 걸음(-1·+1). run.ts 가 `stepTab(step)` 에 넘긴다. 보조 키 셋 모두 같은 효과다 */
export const TAB_STEP: Readonly<Record<string, -1 | 1>> = {
  prevTab: -1,
  prevTab2: -1,
  [PREV_TAB3]: -1,
  nextTab: 1,
  nextTab2: 1,
  [NEXT_TAB3]: 1,
};

export interface ShowBoardState {
  /** 지금 키 입력이 터미널(xterm) 칸에 가 있다 */
  terminalFocused: boolean;
}

export interface ShowBoardPlan {
  /** 보드 창을 펼쳐 앞으로 가져온다(백엔드 `show_board`) — 접혀 있어도 숨겨져 있어도 항상 */
  showWindow: true;
  /** 터미널 칸에서 포커스를 빼 보드 칸으로 옮긴다(mac ⌘0 이 터미널에서도 Ghostty 글자 크기가 아니라 보드 보기, M-537) */
  blurTerminal: boolean;
}

/** 키 입력이 가 있는 요소가 터미널(xterm) 칸 안인가 — run.ts 가 `document.activeElement` 를 넘긴다 */
export function isTerminalElement(el: { closest?: (selector: string) => unknown } | null | undefined): boolean {
  return !!el && typeof el.closest === "function" && !!el.closest(".xterm");
}

/** showBoard 를 눌렀을 때 할 일 — 창은 항상 앞으로, 터미널에 포커스가 있으면 그것도 뺀다 */
export function showBoardPlan(state: ShowBoardState): ShowBoardPlan {
  return { showWindow: true, blurTerminal: state.terminalFocused };
}
