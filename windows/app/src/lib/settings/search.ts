// 설정 항목 검색 — 제목과 키워드에서 대소문자를 가리지 않고 찾는다(mac `SettingsItem` 검색).
// 항목 표는 묶음마다 groups/<이름>Items.ts 에 있고 items.ts 가 합친다 — 여기는 그 표를 가로질러 찾기만 한다.
import type { SettingsSectionId } from "$lib/types";
import type { SettingsItem } from "./items";

/** query 가 제목이나 키워드에 든 항목, items 의 순서 그대로. 앞뒤 공백은 무시하고 빈 검색어는 빈 목록 */
export function search<T extends Pick<SettingsItem, "title" | "keywords">>(items: readonly T[], query: string): T[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [];
  return items.filter((item) => [item.title, ...item.keywords].some((text) => text.toLowerCase().includes(needle)));
}

/** 맞는 항목이 든 묶음 — sectionOrder 의 순서대로 */
export function matchedSections(items: readonly Pick<SettingsItem, "title" | "keywords" | "section">[], query: string, sectionOrder: readonly SettingsSectionId[]): SettingsSectionId[] {
  const found = new Set(search(items, query).map((item) => item.section));
  return sectionOrder.filter((section) => found.has(section));
}
