// 설정 > 창 묶음의 항목 행(mac SettingsItem) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
// 펼치기·접기 애니메이션 행은 3단계(P3.0)가 더한다.
import type { SettingsItem } from "../items";

export const WINDOW_ITEMS: readonly SettingsItem[] = [
  { id: "side", section: "window", title: "보드 위치", keywords: ["오른쪽", "왼쪽"] },
  { id: "opacity", section: "window", title: "투명도", keywords: ["투명", "불투명"] },
  { id: "transition", section: "window", title: "설정 화면 전환", keywords: ["밀기", "서서히", "효과", "애니메이션"] },
];
