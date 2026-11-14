// 카드 머리 클릭이 터미널을 그 경로로 바꾸는 규칙(mac TerminalHub.showWorkspace·GroupCard 머리 탭, 3.1.0) — 순수 계산.
// 허브(hub.svelte.ts)는 svelte 와 Tauri 를 import 해 node:test 로 못 읽으므로 결정만 여기에 둔다. 이 파일은 tabLogic 만 import 한다.
import { workspaceKeyOf } from "./tabLogic.ts";

/**
 * 머리를 눌렀을 때 터미널이 할 일.
 * - 그 경로에 탭이 있다: 그 경로로 바꾼다(마지막에 고른 탭). 포커스는 터미널이 펼쳐 있을 때만 — 접혀 있으면 펼치지 않고 바꿔만 둔다(다음에 펼치면 그 경로가 보인다)
 * - 탭이 없다: 아무것도 하지 않는다(새 경로를 열지 않는다)
 */
export type WorkspaceSwitch = { kind: "none" } | { kind: "select"; tabId: string; focus: boolean };

/** shownTab: 그 경로에서 보일 탭(shownTabOf 의 결과, 없으면 undefined), expanded: 터미널이 펼쳐 있는가 */
export function switchForHeaderClick(shownTab: string | undefined, expanded: boolean): WorkspaceSwitch {
  if (!shownTab) return { kind: "none" };
  return { kind: "select", tabId: shownTab, focus: expanded };
}

/** 펼친 터미널이 지금 이 경로를 보이는 중인가 — 카드 폴더 아이콘의 표지(mac `here`). 접힌 터미널은 보이는 것이 없어 표지가 없다 */
export function isTerminalHere(expanded: boolean, terminalWorkspace: string, groupId: string, home: string | null): boolean {
  return expanded && terminalWorkspace !== "" && workspaceKeyOf(terminalWorkspace, home) === workspaceKeyOf(groupId, home);
}
