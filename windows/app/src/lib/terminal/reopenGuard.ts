// 이어 열기 중 표지 — 같은 끝난 세션을 연타·재요청으로 두 번 열지 않게 한다. 순수 함수만 둔다(sessionFlow 가 쓴다).

/** 표지를 가장 오래 쥐는 시간(ms) — 세션이 끝남을 못 벗어나도 이만큼 뒤에는 다시 열 수 있다 */
export const REOPEN_TTL_MS = 10_000;

export interface Reopening {
  /** 이어 열기를 시작한 시각(ms) */
  since: number;
  /** 명령을 친 화면 */
  surfaceId: string;
}

export interface ReopenFacts {
  now: number;
  /** 그 세션이 보드 상태에서 아직 끝남인가 */
  sessionEnded: boolean;
  /** 그 화면의 터미널에서 claude 가 도는가(runsClaude 에 들었다) */
  termRunsClaude: boolean;
}

/** 표지가 아직 유효한가 — 세션이 끝남을 벗어났거나 claude 가 떴거나 시간이 지나면 끝난 것이다 */
export function reopeningActive(r: Reopening | undefined, f: ReopenFacts): boolean {
  if (!r) return false;
  if (f.now - r.since >= REOPEN_TTL_MS) return false;
  return f.sessionEnded && !f.termRunsClaude;
}

/** 이어 열기 중인 세션에 묶인 화면은 빈 셸 자리(idle)로 다시 쓰지 않는다 */
export function canTakeAsIdle(surfaceSessionId: string | null | undefined, reopeningIds: ReadonlySet<string>): boolean {
  return !surfaceSessionId || !reopeningIds.has(surfaceSessionId);
}
