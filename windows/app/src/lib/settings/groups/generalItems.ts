// 설정 > 일반 묶음의 항목 행(mac SettingsItem) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
import { MASCOTS } from "$lib/mascot/mascots";
import type { SettingsItem } from "../items";

export const GENERAL_ITEMS: readonly SettingsItem[] = [
  { id: "pin", section: "general", title: "항상 위에 고정", keywords: ["플로팅", "pin", "always on top"] },
  { id: "login", section: "general", title: "로그인할 때 열기", keywords: ["로그인 시 실행", "시작", "자동 실행", "시작 프로그램", "startup"] },
  { id: "mascot", section: "general", title: "제목 옆 캐릭터", keywords: MASCOTS.map((m) => m.name) },
];
