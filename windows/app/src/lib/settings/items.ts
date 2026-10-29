// 설정 화면의 항목 표(mac `SettingsItem`) — 검색(search.ts)이 이 표의 제목·키워드로 모든 묶음에서 찾고, 묶음 화면이 같은 표로 줄을 그린다.
//
// 항목 행은 묶음마다 groups/<이름>Items.ts 에 있다(청크가 달라 파일이 겹치지 않게). 여기는 합치기만 한다. 3단계(P3.0)가 글자 크기·테마·애니메이션 행을 터미널·보드 묶음 파일에 더한다.
import type { SettingsSectionId } from "$lib/types";
import { GENERAL_ITEMS } from "./groups/generalItems";
import { WINDOW_ITEMS } from "./groups/windowItems";
import { NOTIFY_ITEMS } from "./groups/notifyItems";
import { TERMINAL_ITEMS } from "./groups/terminalItems";
import { BOARD_ITEMS } from "./groups/boardItems";
import { ACCOUNT_ITEMS } from "./groups/accountItems";
import { SHORTCUTS_ITEMS } from "./groups/shortcutsItems";
import { UPDATE_ITEMS } from "./groups/updateItems";

export interface SettingsItem {
  /** 항목 하나의 안정 식별자 — 검색 결과 이동과 시험이 가리킨다 */
  id: string;
  section: SettingsSectionId;
  title: string;
  /** 제목에 없는 검색어(동의어·영문) */
  keywords: string[];
}

export const SETTINGS_ITEMS: readonly SettingsItem[] = [
  ...GENERAL_ITEMS,
  ...WINDOW_ITEMS,
  ...NOTIFY_ITEMS,
  ...TERMINAL_ITEMS,
  ...BOARD_ITEMS,
  ...ACCOUNT_ITEMS,
  ...SHORTCUTS_ITEMS,
  ...UPDATE_ITEMS,
];
