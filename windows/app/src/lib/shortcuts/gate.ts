// 터미널 칸이 접혀 있을 때 어느 동작을 받는가 — 키 감시(TerminalAreaView)는 접혀 있어도 늘 걸려 있고, 여기서 동작마다 거른다.
// 설정 화면 열고 닫기(Ctrl+,·Esc)는 mac 처럼 앱 어디서나 듣는다(접힌 보드에서 열면 터미널 칸을 펼쳐 설정 화면이 된다).
// 탭·새 세션·이름 바꾸기 같은 터미널 동작은 칸이 펼쳐진 동안만 한다(접힌 칸 뒤의 탭을 건드리지 않는다).
// 순수 함수라 node:test 로 시험한다.
import { SETTINGS_ESCAPE, SETTINGS_TOGGLE, type ShortcutActionId } from "./actions.ts";

const ALWAYS: readonly ShortcutActionId[] = [SETTINGS_TOGGLE, SETTINGS_ESCAPE];

/** 터미널 칸이 펼쳐져 있는가(expanded)에 따라 이 동작을 받는가 */
export function actionAllowed(actionId: ShortcutActionId, expanded: boolean): boolean {
  return expanded || ALWAYS.includes(actionId);
}
