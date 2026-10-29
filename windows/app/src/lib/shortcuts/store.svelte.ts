// 단축키 화면 상태(mac `ShortcutSettings`) — 키 받는 중인 동작, 받지 않은 키 안내, 전역 단축키 등록 실패, 지금 키 글자.
// 받기 흐름은 recording.ts(순수)가 쥐고, 여기는 화면이 보는 반응형 객체에 이어 붙이기만 한다.
import { onHotkeysFailed, resetShortcuts, setShortcut, syncHotkeys } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import type { ShortcutOverrides } from "$lib/types";
import type { ShortcutActionId } from "./actions";
import { label } from "./keyCombo";
import { createRecorder, type RecordKey } from "./recording";
import { combo } from "./rules";

export const ShortcutSettings = $state({
  /** 새 키를 받는 중인 동작 */
  recording: null as ShortcutActionId | null,
  /** 받지 않은 키 안내 */
  refused: null as string | null,
  /** 전역 단축키로 등록하지 못한 동작 */
  hotKeyFailed: [] as ShortcutActionId[],
});

/** 사용자가 바꾼 키 목록 — 화면이 이것을 읽으면 상태 묶음이 바뀔 때 따라 바뀐다 */
export function currentOverrides(): ShortcutOverrides {
  return board.snap?.settings.shortcuts ?? {};
}

/** 그 동작의 지금 키 글자("Ctrl+T"), 비어 있으면 빈 문자열 — 메뉴·탭 줄·제목 막대의 안내 글자가 이것을 따른다 */
export function keyLabel(id: ShortcutActionId): string {
  const c = combo(id, currentOverrides());
  return c ? label(c) : "";
}

const recorder = createRecorder(ShortcutSettings, {
  overrides: currentOverrides,
  write: setShortcut,
  syncHotkeys: (paused) => void syncHotkeys(paused).then((failed) => (ShortcutSettings.hotKeyFailed = failed)),
});

/** 줄의 키 칸을 눌렀다 — 그 동작의 새 키를 받기 시작한다(전역 단축키는 풀어 둔다) */
export const startRecording = recorder.start;

/** 키 받기를 그만둔다(Esc·다른 곳 누름·설정 화면 닫힘) — 전역 단축키를 다시 건다 */
export const stopRecording = recorder.stop;

/** 받는 중에 눌린 키(dispatch 가 SHORTCUT_RECORD 로 넘겨 준다) */
export function recordKey(e: RecordKey): void {
  recorder.key(e);
}

/** 한 줄을 기본 키로 되돌린다 */
export const resetShortcut = recorder.reset;

/** 겹친 상대 동작의 키를 비운다 */
export const clearShortcut = recorder.clear;

/** 바꾼 키를 모두 지운다 */
export function resetAllShortcuts(): void {
  resetShortcuts();
}

/** 설정 > 단축키 화면이 마운트될 때 — 등록 실패 알림을 듣고 지금 상태를 한 번 읽는다. 듣기를 끄고 받기를 그만두는 함수를 돌려준다 */
export function connectShortcutSettings(): () => void {
  ShortcutSettings.refused = null;
  const off = onHotkeysFailed((failed) => (ShortcutSettings.hotKeyFailed = failed));
  void syncHotkeys(false).then((failed) => (ShortcutSettings.hotKeyFailed = failed));
  return () => {
    off();
    stopRecording();
  };
}
