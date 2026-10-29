// 설정 화면의 묶음 여덟 개(mac `SettingsSection`) — 순서가 왼쪽 목록의 순서다. 이름·한 줄 설명만 두고, 줄(항목)은 groups/ 가 쥔다.
import type { SettingsSectionId } from "$lib/types";

export interface SettingsSectionInfo {
  id: SettingsSectionId;
  title: string;
  subtitle: string;
}

export const SETTINGS_SECTIONS: readonly SettingsSectionInfo[] = [
  { id: "general", title: "일반", subtitle: "보드를 띄우는 방식과 제목 옆 캐릭터" },
  { id: "window", title: "창", subtitle: "보드 자리, 투명도, 펼치기·접기와 설정 화면의 움직임" },
  { id: "notify", title: "알림", subtitle: "작업 완료·승인 대기 알림 패널" },
  { id: "terminal", title: "터미널", subtitle: "내장 터미널의 글자·테마와 새 세션 기본값" },
  { id: "board", title: "보드", subtitle: "세션 보드의 글자 크기와 사용 한도 칸" },
  { id: "account", title: "계정", subtitle: "Claude 계정 전환과 한도 자동 전환" },
  { id: "shortcuts", title: "단축키", subtitle: "키 칸을 누르고 새 키를 누르면 바로 바뀝니다" },
  { id: "update", title: "업데이트", subtitle: "새 버전 확인과 설치" },
];

export const SECTION_ORDER: readonly SettingsSectionId[] = SETTINGS_SECTIONS.map((section) => section.id);

export function sectionTitle(id: SettingsSectionId): string {
  return SETTINGS_SECTIONS.find((section) => section.id === id)?.title ?? id;
}
