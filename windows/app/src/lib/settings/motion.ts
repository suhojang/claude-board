// 설정 화면이 올라오고 내려가는 움직임(mac `TerminalAreaView.present`) — 순수 함수만 둔다. SettingsView 가 이 값으로 전환을 그린다.
import type { SettingsTransition } from "$lib/types";

export interface Motion {
  /** 밀리초. 0 이면 효과 없음 */
  duration: number;
  /** 들어오는 가장자리 — -1 은 왼쪽 밖, 1 은 오른쪽 밖, 0 은 제자리(밀지 않음). 나갈 때는 같은 쪽으로 간다 */
  dx: -1 | 0 | 1;
  fade: boolean;
}

const NONE: Motion = { duration: 0, dx: 0, fade: false };

/** 모드와 보드 위치로 움직임을 정한다. 밀기는 보드 반대쪽 가장자리에서 들어온다. 알 수 없는 값은 밀기(기본), 움직임 줄이기 설정이면 효과 없음 */
export function motionFor(mode: SettingsTransition | string | undefined, context: { boardLeft: boolean; reduced: boolean }): Motion {
  if (context.reduced || mode === "none") return NONE;
  if (mode === "fade") return { duration: 200, dx: 0, fade: true };
  return { duration: 340, dx: context.boardLeft ? 1 : -1, fade: true };
}

/** 전환의 한 순간(t: 0 = 화면 밖, 1 = 제자리) — translatePct 는 설정 화면 폭에 대한 퍼센트 */
export function frameAt(motion: Motion, t: number): { translatePct: number; opacity: number } {
  return {
    translatePct: motion.dx * 100 * (1 - t) + 0, // + 0 은 -0 을 0 으로 만든다
    opacity: motion.fade ? t : 1,
  };
}
