// 단축키 받기(mac `ShortcutSettings`)의 흐름 — 줄의 키 칸을 누르면 새 키를 받는다(Esc 취소, Backspace·Delete 비우기).
// 받는 동안은 전역 단축키를 풀어 둔다(누른 키가 동작하지 않고 여기로 들어오게). 바깥 세계(백엔드 호출)는 deps 로 받아 node:test 로 시험한다.
import type { ShortcutOverride, ShortcutOverrides } from "../types.ts";
import { actionOf, type ShortcutActionId } from "./actions.ts";
import { fromEvent, isModifierCode, label } from "./keyCombo.ts";
import { acceptable, set } from "./rules.ts";
import type { KeyCombo } from "./keyCombo.ts";

export interface RecorderState {
  /** 새 키를 받는 중인 동작 */
  recording: ShortcutActionId | null;
  /** 받지 않은 키 안내 */
  refused: string | null;
}

export interface RecorderDeps {
  /** 지금 저장된 바꾼 키 목록 */
  overrides(): ShortcutOverrides;
  /** 저장 — null 은 기본값으로(항목 삭제), [] 는 비움 */
  write(id: ShortcutActionId, override: ShortcutOverride | null): void;
  /** 전역 단축키를 다시 건다(paused 면 모두 풀어 둔다) */
  syncHotkeys(paused: boolean): void;
}

export type RecordKey = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

const NEEDS_MODIFIER = "Ctrl·Alt 가운데 하나를 함께 눌러 주세요";

/** state 는 화면이 보는 객체(반응형이어도 된다) — 이 함수가 고친다 */
export function createRecorder(state: RecorderState, deps: RecorderDeps) {
  // 동작 하나의 키를 c 로 바꾼 저장 값을 쓴다 — 기본값과 같으면 항목을 지운다(null)
  function apply(id: ShortcutActionId, c: KeyCombo | null) {
    deps.write(id, set(deps.overrides(), id, c)[id] ?? null);
  }

  function start(id: ShortcutActionId) {
    if (state.recording === null) deps.syncHotkeys(true);
    state.recording = id;
    state.refused = null;
  }

  function stop() {
    if (state.recording === null) return;
    state.recording = null;
    deps.syncHotkeys(false);
  }

  /** 받는 중에 누른 키. 받는 중이 아니면 false */
  function key(e: RecordKey): boolean {
    const id = state.recording;
    if (id === null) return false;
    if (isModifierCode(e.code)) return true; // 수정키만 눌렸다 — 다음 키를 기다린다
    const c = fromEvent(e);
    const bare = !c.ctrl && !c.alt && !c.shift && !c.win;
    if (bare && c.code === "Escape") {
      state.refused = null;
      stop();
      return true;
    }
    if (bare && (c.code === "Backspace" || c.code === "Delete")) {
      state.refused = null;
      stop();
      apply(id, null);
      return true;
    }
    const verdict = acceptable(c);
    if (verdict === "needsModifier") {
      state.refused = NEEDS_MODIFIER;
      return true;
    }
    if (verdict === "reserved") {
      state.refused = `${label(c)}: Windows 기본 키라 쓸 수 없습니다`;
      return true;
    }
    state.refused = null;
    stop(); // 전역 단축키를 다시 걸게 한 뒤 저장한다. 순서는 보장되지 않는다 — 백엔드가 락 안에서 키 표를 읽고 걸기 때문에 어느 쪽이 먼저 닿아도 된다
    apply(id, c);
    return true;
  }

  return {
    start,
    stop,
    key,
    /** 한 줄을 기본 키로 되돌린다 */
    reset: (id: ShortcutActionId) => apply(id, actionOf(id)?.initial ?? null),
    /** 겹친 상대 동작의 키를 비운다 */
    clear: (id: ShortcutActionId) => apply(id, null),
  };
}
