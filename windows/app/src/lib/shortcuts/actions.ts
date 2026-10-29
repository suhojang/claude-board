// 바꿀 수 있는 동작의 표(mac `Shortcuts.actions`) — id 는 설정 shortcuts 의 열쇠라 바꾸면 사용자가 바꾼 키가 사라진다.
// 아무것도 실행하지 않는 순수 표라 node:test 로 시험한다. 동작 실행은 run.ts 가 한다.
//
// 기본 키는 Windows 에서 지금 쓰는 키를 그대로 둔 것이다(Ctrl+T·Ctrl+Shift+R·Ctrl+Alt+1~9·좌우·위아래·Ctrl+Alt+B/T) —
// 거기에 Windows Terminal 의 탭 이동 키(Ctrl+Tab·Ctrl+Shift+Tab)를 보조로 더했다.
// 3단계(P3.0)가 화면 나누기·글자 크기 행을 더한다(Windows Terminal 키: Alt+Shift+(+/-) 나누기, Alt+방향키 화면 고르기, Alt+Shift+방향키 경계 옮기기).
import { MOD, keyOf, type KeyCombo } from "./keyCombo.ts";

export type ShortcutActionId = string;

export interface ShortcutAction {
  id: ShortcutActionId;
  /** 설정 > 단축키에서 묶는 이름 */
  group: string;
  title: string;
  /** 기본 키, 없으면 null(비어 있는 동작) */
  initial: KeyCombo | null;
}

/** 표에 없고 고를 수도 없는 동작 — 설정 화면 열고 닫기(Ctrl+,). 키는 `SETTINGS_KEY` 로 고정이다 */
export const SETTINGS_TOGGLE = "settings.toggle";
/** 설정 화면이 떠 있을 때 Esc */
export const SETTINGS_ESCAPE = "settings.escape";
/** 설정 > 단축키가 새 키를 받는 중 — 눌린 키는 받는 쪽(recording.ts)이 처리한다 */
export const SHORTCUT_RECORD = "shortcuts.record";

export const SETTINGS_KEY: KeyCombo = keyOf("Comma", MOD.ctrl);

const CA = MOD.ctrl | MOD.alt;
const TAB_DIGITS = [1, 2, 3, 4, 5, 6, 7, 8, 9] as const;

export const SHORTCUT_ACTIONS: readonly ShortcutAction[] = [
  { id: "snap", group: "보드", title: "처음 크기로 붙이기 (어느 앱에서든)", initial: keyOf("KeyB", CA) },
  { id: "terminal", group: "보드", title: "터미널 펼치기·접기 (어느 앱에서든)", initial: keyOf("KeyT", CA) },
  { id: "pin", group: "보드", title: "항상 위에 고정", initial: null },
  { id: "newSession", group: "터미널", title: "새 세션…", initial: keyOf("KeyT", MOD.ctrl) },
  { id: "rename", group: "터미널", title: "탭 이름 바꾸기", initial: keyOf("KeyR", MOD.ctrl | MOD.shift) },
  ...TAB_DIGITS.map((n) => ({ id: `tab${n}`, group: "탭으로 가기", title: `탭 ${n}`, initial: keyOf(`Digit${n}`, CA) })),
  { id: "prevTab", group: "탭으로 가기", title: "이전 탭", initial: keyOf("ArrowLeft", CA) },
  { id: "nextTab", group: "탭으로 가기", title: "다음 탭", initial: keyOf("ArrowRight", CA) },
  { id: "prevTab2", group: "탭으로 가기", title: "이전 탭 (보조)", initial: keyOf("Tab", MOD.ctrl | MOD.shift) },
  { id: "nextTab2", group: "탭으로 가기", title: "다음 탭 (보조)", initial: keyOf("Tab", MOD.ctrl) },
  { id: "prevWorkspace", group: "탭으로 가기", title: "이전 경로", initial: keyOf("ArrowUp", CA) },
  { id: "nextWorkspace", group: "탭으로 가기", title: "다음 경로", initial: keyOf("ArrowDown", CA) },
];

/** id 로 찾은 동작 — 모르는 id 는 undefined */
export function actionOf(id: ShortcutActionId): ShortcutAction | undefined {
  return SHORTCUT_ACTIONS.find((a) => a.id === id);
}
