// 설정 > 알림 묶음의 항목 행(mac SettingsItem) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
import type { SettingsItem } from "../items";
import { POSITION_NAMES, SPOT_ORDER } from "../screenPick";

export const NOTIFY_ITEMS: readonly SettingsItem[] = [
  { id: "allMute", section: "notify", title: "모든 알림 끄기", keywords: ["음소거", "종"] },
  { id: "position", section: "notify", title: "알림 위치", keywords: SPOT_ORDER.map((p) => POSITION_NAMES[p]) },
];
