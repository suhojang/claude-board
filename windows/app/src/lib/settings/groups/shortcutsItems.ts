// 설정 > 단축키 묶음의 항목 행(mac SettingsItem) — 동작 표(shortcuts/actions.ts)에서 그대로 만든다(mac `shortcutItems`). items.ts 가 여덟 묶음을 합친다.
// 동작이 늘면(3단계) 행도 따라 늘어난다. 검색어에는 기본 키 글자와 묶음 이름도 넣는다.
import { SHORTCUT_ACTIONS } from "../../shortcuts/actions.ts";
import { label } from "../../shortcuts/keyCombo.ts";
import type { SettingsItem } from "../items";

export const SHORTCUTS_RESET_ALL_ITEM_ID = "shortcuts.resetAll";

export const SHORTCUTS_ITEMS: readonly SettingsItem[] = [
  ...SHORTCUT_ACTIONS.map(
    (action): SettingsItem => ({
      id: `key.${action.id}`,
      section: "shortcuts",
      title: action.title,
      keywords: [...(action.initial ? [label(action.initial)] : []), action.group, "단축키", "키", "shortcut", "keyboard"],
    }),
  ),
  { id: SHORTCUTS_RESET_ALL_ITEM_ID, section: "shortcuts", title: "단축키 모두 기본값으로", keywords: ["초기화", "되돌리기", "reset", "단축키"] },
];
