// 보드 글자 크기 단축키 넷(mac `Shortcuts.actions` 의 boardBigger·boardBigger2·boardSmaller·boardReset)과 눌렀을 때 바뀌는 배율·화면 글 — 순수 모듈이다.
// actions.ts 가 `BOARD_FONT_ACTIONS` 를 표에 펼쳐 넣고, run.ts 가 `boardFontScaleChange` 로 잇는다(plan.md B-05).
// import 하는 것은 같은 폴더의 fontScale.ts 와 shortcuts 의 순수 모듈뿐이라 node:test 로 시험한다.
//
// mac `BoardFontScale.change` 와 같은 규칙: 크게·작게는 한 단계(10%), 원래 크기는 100%.
// 80%·150% 끝에서도 키는 먹고 같은 비율 글을 다시 보인다(mac 은 끝에서도 `toast` 를 부른다) — 키가 죽은 것으로 보이지 않게 하는 쪽이다.
//
// 기본 키(결정 D6: Ctrl+Alt 계열, 겹치면 그 줄만 바꾼다)
// - mac 은 ⌘⌥ 로 작게(-)·원래(0), ⌘⌥⇧ 로 크게(=)다. Windows 에서 Ctrl+Alt+0 은 showBoard 가 쓰고 Ctrl+Alt+= 은 화면 고르게(equalize)가 쓸 자리라
//   보드 글자 셋은 모두 Shift 를 더한 Ctrl+Alt+Shift+= / - / 0 으로 둔다(D6 표와 같다). 같은 키가 둘이 되지 않음은 fontScaleKeys.test.ts 가 표 전체로 센다.
// - boardBigger2(보조): mac 의 ⌘⌥= 에 해당하는 Ctrl+Alt+= 는 equalize 와 겹친다. 숫자패드 + 는 어느 동작도 안 쓰므로 Ctrl+Alt+Num + 로 둔다.
import { boardFontScaleToast, normalizeBoardFontScale, stepBoardFontScale } from "./fontScale.ts";
import { MOD, keyOf, type KeyCombo } from "../shortcuts/keyCombo.ts";

/** actions.ts 의 표 행 모양(순환 import 를 피하려고 여기서 따로 쓴다 — showBoard.ts 와 같다) */
export interface BoardFontAction {
  id: string;
  group: string;
  title: string;
  initial: KeyCombo | null;
}

export const BOARD_BIGGER = "boardBigger";
export const BOARD_BIGGER2 = "boardBigger2";
export const BOARD_SMALLER = "boardSmaller";
export const BOARD_RESET = "boardReset";

const CAS = MOD.ctrl | MOD.alt | MOD.shift;
const GROUP = "글자 크기";

/** actions.ts 표에 그대로 펼쳐 넣는 행 — mac 표 순서(크게·크게 보조·작게·원래 크기), 이름도 mac 과 같다 */
export const BOARD_FONT_ACTIONS: readonly BoardFontAction[] = [
  { id: BOARD_BIGGER, group: GROUP, title: "보드 글자 크게", initial: keyOf("Equal", CAS) },
  { id: BOARD_BIGGER2, group: GROUP, title: "보드 글자 크게 (보조)", initial: keyOf("NumpadAdd", MOD.ctrl | MOD.alt) },
  { id: BOARD_SMALLER, group: GROUP, title: "보드 글자 작게", initial: keyOf("Minus", CAS) },
  { id: BOARD_RESET, group: GROUP, title: "보드 글자 원래 크기", initial: keyOf("Digit0", CAS) },
];

/** 동작 id → 걸음(+1 크게, -1 작게, null 원래 크기). stepBoardFontScale 의 두 번째 인자 그대로다 */
export const BOARD_FONT_STEP: Readonly<Record<string, 1 | -1 | null>> = {
  [BOARD_BIGGER]: 1,
  [BOARD_BIGGER2]: 1,
  [BOARD_SMALLER]: -1,
  [BOARD_RESET]: null,
};

/** 보드 글자 크기 동작인가 */
export function isBoardFontAction(id: string): boolean {
  return Object.hasOwn(BOARD_FONT_STEP, id);
}

export interface BoardFontScaleChange {
  /** 바뀐 뒤의 배율(0.8~1.5) */
  scale: number;
  /** 보드 아래에 잠깐 보일 글 — "보드 글자 110%" */
  toast: string;
  /** 배율이 실제로 달라졌는가. 끝(80%·150%)이나 이미 100% 면 false — 저장은 건너뛰어도 되지만 글은 그대로 보인다 */
  changed: boolean;
}

/** 동작 id 를 지금 배율에 적용한 결과. 보드 글자 동작이 아니면 null(run.ts 는 아무것도 하지 않는다).
 *  current 는 설정 값 그대로(없음·깨진 값 포함)여도 된다 — 기본 100% 로 본다 */
export function boardFontScaleChange(id: string, current: unknown): BoardFontScaleChange | null {
  if (!isBoardFontAction(id)) return null;
  const from = normalizeBoardFontScale(current);
  const scale = stepBoardFontScale(from, BOARD_FONT_STEP[id]);
  return { scale, toast: boardFontScaleToast(scale), changed: scale !== from };
}
