// 동작 하나를 실행한다 — dispatch.ts 가 고른 id 를 허브·백엔드·설정 화면의 함수에 잇는다. 표(actions.ts)와 판단(dispatch.ts)은 순수라 따로 시험하고, 여기는 잇기만 한다.
import { setPinned, setTerminalExpanded, snapRight } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import { escapeSettings, toggleSettings } from "$lib/settings/screen.svelte";
import { beginRename, newSessionSheet, selectNumber, stepTab, stepWorkspace } from "$lib/terminal/hub.svelte";
import { SETTINGS_ESCAPE, SETTINGS_TOGGLE, SHORTCUT_RECORD, type ShortcutActionId } from "./actions";
import type { RecordKey } from "./recording";
import { recordKey } from "./store.svelte";

const settings = () => board.snap?.settings;

// 어느 앱에서든 듣는 둘(snap·terminal)은 전역 단축키가 먼저 가져가지만, 등록하지 못했을 때 보드가 앞에 있으면 여기서도 같은 일을 한다
const ACTIONS: Record<ShortcutActionId, () => void> = {
  snap: snapRight,
  terminal: () => setTerminalExpanded(!(settings()?.terminalExpanded ?? false)),
  pin: () => setPinned(!(settings()?.pinned ?? false)),
  newSession: newSessionSheet,
  rename: beginRename,
  prevTab: () => stepTab(-1),
  nextTab: () => stepTab(1),
  prevTab2: () => stepTab(-1),
  nextTab2: () => stepTab(1),
  prevWorkspace: () => stepWorkspace(-1),
  nextWorkspace: () => stepWorkspace(1),
  [SETTINGS_TOGGLE]: toggleSettings,
  [SETTINGS_ESCAPE]: escapeSettings,
};
for (let n = 1; n <= 9; n++) ACTIONS[`tab${n}`] = () => selectNumber(n);

/** 동작 하나를 실행한다. 모르는 id 는 아무것도 하지 않는다. 키 받는 중(SHORTCUT_RECORD)이면 눌린 키 e 를 받는 쪽에 넘긴다 */
export function runShortcutAction(id: ShortcutActionId, e?: RecordKey): void {
  if (id === SHORTCUT_RECORD) {
    if (e) recordKey(e);
    return;
  }
  ACTIONS[id]?.();
}
