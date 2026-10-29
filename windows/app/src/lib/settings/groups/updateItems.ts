// 설정 > 업데이트 묶음의 항목 행(mac SettingsItem) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
import type { SettingsItem } from "../items";

export const UPDATE_ITEMS: readonly SettingsItem[] = [
  { id: "version", section: "update", title: "Claude Board 버전", keywords: ["업데이트 확인", "새 버전", "설치", "바뀐 점"] },
  { id: "autoUpdate", section: "update", title: "자동으로 업데이트 확인", keywords: ["자동 업데이트"] },
];
