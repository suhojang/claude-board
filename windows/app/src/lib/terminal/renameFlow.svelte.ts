// 탭 이름 바꾸기를 그 탭 claude 세션들의 /rename 으로 옮기는 흐름(mac renameSessions·applyRenames) — 계획은 renamePlan.ts 가 짜고,
// 여기는 걸음마다 안전을 확인(rename_check)하고 키를 넣는(drive_session) 일만 한다. 이름 바꾸기를 끝낸 순간과 상태 묶음마다 부른다.
import { driveSession, renameCheck } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import type { BoardSurface, BoardTab, Session } from "$lib/types";
import { shownName, surfaceOf } from "./hub.svelte";
import { nextRenameStep, renameTargets, type RenameStep } from "./renamePlan";
import { sanitizeSessionName } from "./sessionName";
import { leaves } from "./split";

const wanted = new Map<string, string>(); // 화면 id -> 붙이려는 세션 이름
const steps = new Map<string, RenameStep>(); // 화면 id -> 어디까지 쳤나
const busy = new Set<string>(); // 키를 넣는 중인 화면 — 같은 화면의 걸음이 겹치지 않게

const NOT_SAFE = { safe: false, transcript: false };

const allSessions = (): Session[] => board.snap?.groups.flatMap((g) => g.sessions) ?? [];

/** 화면의 주 세션 — 사는 대화형 세션 가운데 tty 가 화면 터미널과 같은 것 */
function mainSessionOf(surface: BoardSurface, sessions: Session[]): Session | undefined {
  if (surface.termId === null) return undefined;
  return sessions.find((s) => s.state !== "ended" && !s.headless && s.mode === null && s.tty === surface.termId);
}

function forget(surfaceId: string) {
  wanted.delete(surfaceId);
  steps.delete(surfaceId);
}

/** 탭 이름이 정해졌다 — 그 탭 화면마다 붙일 세션 이름을 정해 두고 첫 걸음을 내딛는다 */
export function startTabRenames(tab: BoardTab) {
  const ids = leaves(tab.root);
  const surfaces = ids.map(surfaceOf).filter((s): s is BoardSurface => s !== undefined);
  const reserved = [...wanted].filter(([id]) => !ids.includes(id)).map(([, target]) => target);
  for (const id of ids) forget(id);
  for (const { surfaceId, target } of renameTargets(sanitizeSessionName(shownName(tab)), surfaces, allSessions(), reserved)) {
    wanted.set(surfaceId, target);
  }
  void applyRenames();
}

async function checkOf(sessionId: string) {
  try {
    return await renameCheck(sessionId);
  } catch {
    return NOT_SAFE;
  }
}

/** 글을 넣은 뒤 Enter 까지 기다리는 시간(ms) — 재개 문구의 200ms 와 같다 */
const TYPE_TO_ENTER_MS = 200;

// 화면 하나의 걸음 — 글을 붙인 직후에는 한 번 더 확인하고 Enter 까지 간다(그 사이 창이 떴으면 typed 로 남는다)
async function advance(surfaceId: string) {
  for (let round = 0; round < 2; round++) {
    const target = wanted.get(surfaceId);
    const surface = surfaceOf(surfaceId);
    const main = surface && mainSessionOf(surface, allSessions());
    if (!target || !main) return forget(surfaceId);
    const step = steps.get(surfaceId);
    const asks = main.name !== target && (step === undefined || step.kind === "typed");
    const check = asks ? await checkOf(main.id) : NOT_SAFE;
    if (wanted.get(surfaceId) !== target) return;
    const next = nextRenameStep(step, main, target, Date.now(), check);
    if (!next.step) return forget(surfaceId);
    steps.set(surfaceId, next.step);
    if (next.emit === "none") return;
    const ok = await driveSession(main.id, next.emit === "text" ? [{ text: `/rename ${target}` }] : [{ key: "enter" }]);
    if (wanted.get(surfaceId) !== target) return;
    if (!ok) return forget(surfaceId);
    if (next.emit === "enter") return;
    steps.set(surfaceId, { kind: "typed" });
    // 글과 Enter 를 한 덩어리로 보내면 Claude Code 가 붙여넣기로 볼 수 있어 재개 흐름처럼 사이를 띄운다
    await new Promise<void>((resolve) => setTimeout(resolve, TYPE_TO_ENTER_MS));
  }
}

/** 기다리는 이름 바꾸기를 한 걸음씩 나아가게 한다 — 상태 묶음마다 부른다 */
export async function applyRenames() {
  for (const surfaceId of [...wanted.keys()]) {
    if (busy.has(surfaceId)) continue;
    busy.add(surfaceId);
    try {
      await advance(surfaceId);
    } finally {
      busy.delete(surfaceId);
    }
  }
}
