// 알림 위치를 화면 그림에서 고르는 순수 규칙 — 그림 안의 누른 점을 알림 위치 다섯 자리 가운데 하나로(mac `NotifyPositionPicker`).
// 자리 모양은 mac 의 132×82 그림과 같은 비율이다: 좌우 자리는 가운데에서 ±1/3 폭, 위·가운데·아래 자리는 그림 높이의 약 24%·54%·83% 에 있다.
import type { NotifyPosition } from "$lib/types";

/** 자리를 그리고 겨눌 때의 순서 — 가운데가 앞이라 같은 거리에서는 가운데가 이긴다 */
export const SPOT_ORDER: readonly NotifyPosition[] = ["center", "top-left", "top-right", "bottom-left", "bottom-right"];

/** 그림 안의 자리 한가운데(0~1 비율) */
export const SPOTS: Readonly<Record<NotifyPosition, { x: number; y: number }>> = {
  "top-left": { x: 1 / 6, y: 20 / 82 },
  "top-right": { x: 5 / 6, y: 20 / 82 },
  center: { x: 1 / 2, y: 44 / 82 },
  "bottom-left": { x: 1 / 6, y: 68 / 82 },
  "bottom-right": { x: 5 / 6, y: 68 / 82 },
};

/** 자리 이름 — 설명 글·접근성 이름·검색어(mac `notifyPositions`) */
export const POSITION_NAMES: Readonly<Record<NotifyPosition, string>> = {
  "top-left": "왼쪽 위",
  "top-right": "오른쪽 위",
  center: "가운데",
  "bottom-left": "왼쪽 아래",
  "bottom-right": "오른쪽 아래",
};

/** 그림 크기 안에서 누른 점이 가장 가까운 알림 위치. 크기가 0 이거나 숫자가 아니면 가운데 */
export function pickPosition(size: { width: number; height: number }, point: { x: number; y: number }): NotifyPosition {
  const { width, height } = size;
  if (![width, height, point.x, point.y].every(Number.isFinite) || width <= 0 || height <= 0) return "center";
  let best: NotifyPosition = "center";
  let bestDistance = Infinity;
  for (const p of SPOT_ORDER) {
    const dx = SPOTS[p].x * width - point.x;
    const dy = SPOTS[p].y * height - point.y;
    const d = dx * dx + dy * dy;
    if (d < bestDistance) {
      best = p;
      bestDistance = d;
    }
  }
  return best;
}
