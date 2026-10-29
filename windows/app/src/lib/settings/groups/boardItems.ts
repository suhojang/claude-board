// 설정 > 보드 묶음의 항목 행(mac SettingsItem) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
// 보드 글자 크기 행은 3단계(P3.0)가 더한다.
import type { SettingsItem } from "../items";

export const BOARD_ITEMS: readonly SettingsItem[] = [
  { id: "usageCompact", section: "board", title: "사용 한도 간략히 보기", keywords: ["사용량", "한도"] },
];
