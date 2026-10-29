// 투명 모드 규칙 — 보드가 불투명해야 하는가. 순수 함수다. node:test 로 바로 시험한다(opacity.test.ts).
// 3.1.9: 마우스가 창 밖이면 보드가 활성이어도 고른 투명도로 보이고, 화면(작업 영역)을 덮은 창은 마우스가 안에 있는 것과 같이 불투명하다.
// 슬라이더로 투명도를 미리 보는 동안은 어느 쪽이든 고른 투명도가 보인다(mac `!previewing && (inWindow || expandedToFullScreen() || menuOpen)`).

export interface OpacityInput {
  /** 창이 그 모니터 작업 영역 전체를 덮는가(`coversWorkArea`) — 펼쳤는가가 아니다. 60% 로 줄인 펼친 창은 덮지 않는다 */
  covering: boolean;
  /** 슬라이더를 끄는 중이거나 놓은 뒤 1.2초 — 고른 투명도를 미리 보여 준다 */
  previewing: boolean;
  /** 마우스 포인터가 창 안에 있다 — 포커스·활성과 무관하다 */
  pointerInside: boolean;
}

/** 미리 보는 중이 아닐 때만, 덮고 있거나 마우스가 안에 있으면 불투명. 미리 보는 중에는 덮고 있어도 고른 투명도로 보인다 */
export function boardOpaque({ covering, previewing, pointerInside }: OpacityInput): boolean {
  return !previewing && (covering || pointerInside);
}

/** 창이 작업 영역 전체를 덮는가 — 폭·높이 모두 작업 영역에서 허용 오차(기본 2px) 이상 모자라지 않다.
 *  보드 창은 테두리·그림자가 없어 바깥 크기 = 안쪽 크기다. 값을 못 쟀거나 유한하지 않으면 덮는 것으로 본다(펼치면 불투명이던 옛 쪽으로 기운다). */
export function coversWorkArea(outerW: number, outerH: number, availW: number, availH: number, tolerance = 2): boolean {
  if (![outerW, outerH, availW, availH].every(Number.isFinite) || availW <= 0 || availH <= 0) return true;
  return outerW >= availW - tolerance && outerH >= availH - tolerance;
}
