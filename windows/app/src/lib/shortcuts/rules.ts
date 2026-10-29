// 단축키 규칙(mac `Shortcuts.combo/set/conflicts`, `ShortcutSettings.record` 의 받기 조건) — 순수 함수만 둔다.
import type { ShortcutOverrides } from "../types.ts";
import { SHORTCUT_ACTIONS, SETTINGS_KEY, actionOf, type ShortcutActionId } from "./actions.ts";
import { MOD, fromStored, keyOf, same, toStored, type KeyCombo } from "./keyCombo.ts";

/**
 * 고를 수 없는 키 — 판단 목록(`judgment`, 측정한 것이 아니라 Windows 가 먼저 가져가거나 편집·복사 표준으로 굳은 키를 적었다).
 * Win 이 든 모든 키는 이 목록과 별개로 `acceptable` 이 거절한다.
 */
export const RESERVED: readonly KeyCombo[] = [
  keyOf("F4", MOD.alt),
  keyOf("Tab", MOD.alt),
  keyOf("Space", MOD.alt),
  keyOf("Escape", MOD.alt),
  keyOf("Escape", MOD.ctrl),
  keyOf("Escape", MOD.ctrl | MOD.shift),
  ...["KeyC", "KeyV", "KeyX", "KeyA", "KeyZ", "KeyY"].map((code) => keyOf(code, MOD.ctrl)),
  SETTINGS_KEY, // 이 화면을 여는 Ctrl+,
];

const FUNCTION_KEY = /^F([1-9]|1[0-2])$/;

/** 그 동작의 지금 키 — 바꾼 것이 있으면 그것(비움이거나 깨진 값이면 null), 없으면 기본값 */
export function combo(id: ShortcutActionId, overrides: ShortcutOverrides): KeyCombo | null {
  if (Object.hasOwn(overrides, id)) return fromStored(overrides[id]);
  return actionOf(id)?.initial ?? null;
}

/** 키를 바꾼 뒤의 저장 목록(새 객체). c 가 null 이면 비움, 기본값과 같은 키면 항목을 지운다 */
export function set(overrides: ShortcutOverrides, id: ShortcutActionId, c: KeyCombo | null): ShortcutOverrides {
  const next = { ...overrides };
  if (same(c, actionOf(id)?.initial ?? null)) delete next[id];
  else next[id] = c ? toStored(c) : [];
  return next;
}

/** 같은 키를 쓰는 다른 동작의 id */
export function conflicts(id: ShortcutActionId, overrides: ShortcutOverrides): ShortcutActionId[] {
  const mine = combo(id, overrides);
  if (!mine) return [];
  return SHORTCUT_ACTIONS.filter((a) => a.id !== id && same(combo(a.id, overrides), mine)).map((a) => a.id);
}

/** 눌린 키가 가리키는 동작 — 겹치면 표 순서에서 앞선 것(mac 메뉴가 앞의 것만 받는 것과 같다). 없으면 null */
export function find(pressed: KeyCombo, overrides: ShortcutOverrides): ShortcutActionId | null {
  return SHORTCUT_ACTIONS.find((a) => same(combo(a.id, overrides), pressed))?.id ?? null;
}

/** 받아도 되는 키인가 — Win 조합과 RESERVED 는 reserved, Ctrl·Alt 가 없는 키는 needsModifier(F1~F12 는 수정키 없이도 된다) */
export function acceptable(c: KeyCombo): "ok" | "reserved" | "needsModifier" {
  if (c.win) return "reserved";
  if (!c.ctrl && !c.alt && !FUNCTION_KEY.test(c.code)) return "needsModifier";
  return RESERVED.some((r) => same(r, c)) ? "reserved" : "ok";
}
