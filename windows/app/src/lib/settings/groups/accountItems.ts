// 설정 > 계정 묶음의 항목 행(mac SettingsItem accountItems) — items.ts 가 여덟 묶음을 합친다(청크마다 파일이 달라 겹치지 않는다).
// 한 항목이 한 카드가 아니다: `accounts` 는 저장한 계정 줄들과 계정 추가·로그인 단계를 함께 가리키는 묶음 항목이다.
import type { SettingsItem } from "../items";

export const ACCOUNT_ITEMS: readonly SettingsItem[] = [
  {
    id: "accounts",
    section: "account",
    title: "저장한 계정",
    keywords: ["로그인", "전환", "추가", "지우기", "이메일", "계정 추가", "다시 로그인", "login", "account"],
  },
  {
    id: "autoSwitch",
    section: "account",
    title: "한도가 차면 다른 계정으로 자동 전환",
    keywords: ["한도", "자동", "전환", "limit", "switch"],
  },
];
