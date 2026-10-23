// 셸이 줄 편집을 시작한 뒤에 명령 한 줄을 쳐 넣는다(ClaudeBoard.swift BoardSurface.type) — hub 를 모르게 상태 접근은 인자로 받는다.
import { termWrite } from "$lib/backend";
import type { TermId } from "$lib/types";
import type { TermController } from "./term";

/** 셸이 첫 출력을 낸 뒤 300ms 조용해지면(5초 상한) 명령을 쳐 넣고 100ms 뒤 Enter(\r) */
const QUIET_MS = 300;
const QUIET_CAP_MS = 5000;
const ENTER_DELAY_MS = 100;
export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export interface TypeHooks {
  /** 기다리는 사이 탭이 닫혔거나 같은 칸에 다른 셸이 붙었으면 거짓 — 거짓이면 치지 않는다 */
  alive: () => boolean;
  /** 친다는 표시(화면 typedAt) — 기다리는 동안에도 부른다. 프로세스 스냅샷이 따라오기 전의 찰나를 claude 가 도는 것으로 메운다 */
  touch: () => void;
}

/** 이미 셸 프롬프트에 있는 화면에 명령을 바로 친다 */
export async function typeAtPrompt(termId: TermId, command: string, hooks: TypeHooks) {
  hooks.touch();
  termWrite(termId, command);
  await sleep(ENTER_DELAY_MS);
  if (!hooks.alive()) return;
  hooks.touch();
  termWrite(termId, "\r");
}

/** 새로 띄운 셸이 조용해질 때까지(5초 상한) 기다렸다가 친다 */
export async function typeCommand(ctl: TermController, termId: TermId, command: string, hooks: TypeHooks) {
  const startedAt = Date.now();
  // 첫 출력(lastOutputAt > 0)이 있고 그 뒤 300ms 조용하면 준비된 것. 5초가 지나면 그냥 친다
  const quiet = () => ctl.lastOutputAt > 0 && Date.now() - ctl.lastOutputAt >= QUIET_MS;
  while (!quiet() && Date.now() - startedAt < QUIET_CAP_MS) {
    hooks.touch();
    await sleep(50);
  }
  if (hooks.alive()) await typeAtPrompt(termId, command, hooks);
}
