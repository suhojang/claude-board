// 새 세션 띄우기 — 명령을 고쳐(--session-id, --name) 그 폴더의 셸 탭에 쳐 넣는다. 명령줄을 만드는 순수 함수는 sessionCommand.ts 에 있다.
import { rememberMdRenderOff, setTerminalExpanded } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import { folderName, TerminalHub, openShell, shownName } from "./hub.svelte";
import { DEFAULT_SESSION_COMMAND, freshSessionCommand, namedCommand } from "./sessionCommand";
import { quoteSessionName, sanitizeSessionName } from "./sessionName";

export { DEFAULT_SESSION_COMMAND };

/** 다른 탭이 쓰는 이름과 겹치지 않는 이름 — 겹치면 -2, -3 …(대소문자는 가리지 않는다) */
export function uniqueTabName(base: string): string {
  const used = new Set(TerminalHub.tabs.map((tab) => shownName(tab).toLowerCase()));
  let name = base;
  for (let n = 2; used.has(name.toLowerCase()); n++) name = `${base}-${n}`;
  return name;
}

/** 그 폴더에서 새 세션을 새 탭으로 띄운다. 터미널이 접혀 있으면 펼친다. mdRender 가 거짓이면 답변 꾸미기를 끄고 띄우고 그 세션 id 를 끈 세션으로 기억한다 */
export function startSession(command: string, dir: string, mdRender = true) {
  const trimmed = command.trim();
  if (!trimmed) return;
  if (!board.snap?.settings.terminalExpanded) setTerminalExpanded(true);
  const fresh = freshSessionCommand(trimmed);
  const finalCommand = namedCommand(fresh.command, quoteSessionName(uniqueTabName(sanitizeSessionName(folderName(dir)))));
  const mdRenderOff = !mdRender;
  if (mdRenderOff && fresh.sessionId) rememberMdRenderOff(fresh.sessionId);
  openShell(dir, { command: finalCommand, sessionId: fresh.sessionId ?? undefined, mdRenderOff });
}
