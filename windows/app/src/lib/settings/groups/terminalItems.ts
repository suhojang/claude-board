// 설정 > 터미널 묶음의 항목 행(mac SettingsItem) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
// 3단계(P3.0)가 글자 크기·테마·애니메이션 행을 여기에 더한다.
import type { SettingsItem } from "../items";

export const TERMINAL_ITEMS: readonly SettingsItem[] = [
  { id: "sessionCommand", section: "terminal", title: "기본 명령", keywords: ["새 세션", "claude", "실행", "명령어"] },
  { id: "sessionMd", section: "terminal", title: "답변 마크다운 꾸미기", keywords: ["새 세션", "md-render"] },
];
