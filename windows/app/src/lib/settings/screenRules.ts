// 설정 화면의 판단(mac `SettingsScreen.escape/open/close`) — 순수 함수만 둔다. screen.svelte.ts 가 이 결과대로 상태를 바꾼다.

export type EscapeStep = "cancelRecording" | "clearQuery" | "close";

/** Esc 한 번이 하는 일 — 키 받는 중이면 취소, 검색어가 있으면 지우고, 아니면 닫는다 */
export function escapeStep(state: { recording: boolean; query: string }): EscapeStep {
  if (state.recording) return "cancelRecording";
  if (state.query.length > 0) return "clearQuery";
  return "close";
}

/** 열 때 터미널 칸을 잠깐 펼쳐야 하나 — 접혀 있다고 알 때만(상태를 아직 못 받았으면 건드리지 않는다) */
export function collapseOnOpen(board: { snapKnown: boolean; expanded: boolean }): boolean {
  return board.snapKnown && !board.expanded;
}

/** 닫은 뒤 터미널 칸 — 잠깐 펼친 것이면 다시 접는다. 탭을 다루는 동작으로 닫았으면(keep) 펼친 채 둔다 */
export function afterClose(close: { collapsedByUs: boolean; keep: boolean }): "recollapse" | "stay" {
  return close.collapsedByUs && !close.keep ? "recollapse" : "stay";
}

/** 터미널 칸이 접혔다(⌃⌥T·접기 단추) — 떠 있는 설정 화면은 효과 없이 내려간다 */
export function shouldDismissForCollapse(board: { shown: boolean; snapKnown: boolean; expanded: boolean }): boolean {
  return board.shown && board.snapKnown && !board.expanded;
}
