// 보드 글자 크기 배율 규칙 — 순수 함수다. node:test 로 바로 시험한다(fontScale.test.ts).
// mac Sources/ClaudeBoard/BoardFontScale.swift 를 옮겼다: 기본 1, 80%~150%, 10%씩. 크게·작게는 한 단계, 원래 크기는 1.
// 설정 키는 `boardFontScale`(Rust settings.rs 의 `board_font_scale`). 원래 크기(1)는 mac 처럼 설정을 남기지 않는 값으로 본다.

/** 가장 작은 배율(80%) */
export const BOARD_FONT_SCALE_MIN = 0.8;
/** 가장 큰 배율(150%) */
export const BOARD_FONT_SCALE_MAX = 1.5;
/** 기본 배율(원래 크기) */
export const BOARD_FONT_SCALE_DEFAULT = 1;
/** 한 단계(10%). 배율은 늘 이 단계의 배수이므로 내부는 10배 정수(tenths)로 센다 */
export const BOARD_FONT_SCALE_STEP = 0.1;

const MIN_TENTHS = 8;
const MAX_TENTHS = 15;
const DEFAULT_TENTHS = 10;

/** 10배 정수(8~15) → 배율. 부동소수점 오차(0.1 * 3)가 안 생기게 나눗셈으로 만든다 */
function fromTenths(tenths: number): number {
  return Math.min(MAX_TENTHS, Math.max(MIN_TENTHS, tenths)) / 10;
}

/** 저장된 값 → 배율. 숫자와 숫자 문자열만 받고(앞뒤 공백 허용) 그 밖(null·NaN·Infinity·빈 문자열·객체)은 기본값.
 *  범위 밖은 끝값으로 자르고 10% 단위가 아니면 가장 가까운 단계로 맞춘다(mac 은 자르기만 하지만 mac 이 쓰는 값은 늘 단계의 배수다) */
export function normalizeBoardFontScale(raw: unknown): number {
  let n: number;
  if (typeof raw === "number") n = raw;
  else if (typeof raw === "string" && raw.trim() !== "") n = Number(raw);
  else return BOARD_FONT_SCALE_DEFAULT;
  if (!Number.isFinite(n)) return BOARD_FONT_SCALE_DEFAULT;
  return fromTenths(Math.round(n * 10));
}

/** 한 단계 바꾼 배율 — step: +1 크게, -1 작게, null 원래 크기(mac `change`). 끝에서는 더 가지 않는다 */
export function stepBoardFontScale(current: number, step: number | null): number {
  if (step === null) return BOARD_FONT_SCALE_DEFAULT;
  const base = Math.round(normalizeBoardFontScale(current) * 10);
  const delta = Number.isFinite(step) ? Math.trunc(step) : 0;
  return fromTenths(base + delta);
}

/** 화면에 보이는 비율 글자 — 1.1 → "110%" */
export function formatBoardFontScale(scale: number): string {
  return `${Math.round(normalizeBoardFontScale(scale) * 100)}%`;
}

/** 바뀐 비율을 보드 아래에 잠깐 알리는 글(mac `toast("보드 글자 \(tenths * 10)%")`) */
export function boardFontScaleToast(scale: number): string {
  return `보드 글자 ${formatBoardFontScale(scale)}`;
}

/** 원래 크기(1)인가 — 설정 화면이 「원래대로」 단추를 쓸 수 있는지, 저장 값을 비울지 가르는 데 쓴다 */
export function isDefaultBoardFontScale(scale: number): boolean {
  return normalizeBoardFontScale(scale) === BOARD_FONT_SCALE_DEFAULT;
}

/** 더 키울 수 있는가 / 더 줄일 수 있는가 — 설정 화면 +/- 단추의 눌림 가능 여부 */
export function canGrowBoardFontScale(scale: number): boolean {
  return normalizeBoardFontScale(scale) < BOARD_FONT_SCALE_MAX;
}
export function canShrinkBoardFontScale(scale: number): boolean {
  return normalizeBoardFontScale(scale) > BOARD_FONT_SCALE_MIN;
}

/** 칸 크기 문턱과 보드 칸 크기는 화면 픽셀이다. 배율 안쪽 칸(zoom)은 배율만큼 크게 그려지므로,
 *  안쪽 내용이 쓰는 높이를 화면 높이로 바꾸려면 배율을 곱한다 — 단계 문턱(paneLayout.ts·+page.svelte 의 fixed)은 화면 높이로 비교한다. */
export function screenHeight(innerHeight: number, scale: number): number {
  return innerHeight * normalizeBoardFontScale(scale);
}

/** 안쪽 칸이 쓸 수 있는 폭(안쪽 좌표) — 화면 폭을 배율로 나눈다. 150% 에서 360px 칸은 안쪽으로 240px 이다 */
export function innerWidth(screenWidth: number, scale: number): number {
  return screenWidth / normalizeBoardFontScale(scale);
}

/** 미니 보드 높이에 곱할 수(mac BoardTiers `max(1, scale)`) — 줄어든 80% 에서는 줄이지 않고 150% 에서만 늘린다 */
export function miniHeightFactor(scale: number): number {
  return Math.max(1, normalizeBoardFontScale(scale));
}
