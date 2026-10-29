// 끌 때 확인 창의 목록 규칙(mac busyTabSessions·confirmShutdown) — 누구를 보이고 누구에게 `.paused` 를 남기나. 순수 함수다.
// - 보드 탭에서 도는 세션만 꺼진다. 백그라운드·다른 터미널의 세션은 영향이 없어 목록에 없다.
// - 줄이 되는 때: 탭 안의 `claude -p`(늘) · 승인 대기 · 작업 중 · 서브에이전트가 도는 세션 · Bash 백그라운드 셸이 도는 세션 ·
//   claude 가 아닌 프로그램이 도는 셸 탭. 쉬는 주 세션은 줄이 되지 않는다.
// - `.paused` 는 줄이 있는 탭의 주 세션(사람이 입력하는 대화형 claude)에 남긴다 — 주 세션이 쉬어도(서브에이전트·셸·-p 만 돌아도) 남긴다.
//   탭 안의 `claude -p` 는 입력 자리가 없어 남기지 않는다. 줄이 하나도 없으면 묻지 않고 아무 표지도 남기지 않는다.
import type { BoardSurface, BoardTab, Session, ShutdownItem, TermId } from "../types.ts";
import { pairSessions, TYPED_WINDOW_MS } from "./place.ts";
import { leaves } from "./split.ts";
import { baseName } from "./tabLogic.ts";

export type { ShutdownItem } from "../types.ts";

export interface ShutdownList {
  /** 확인 창에 보일 줄. 비면 확인 없이 끈다 */
  items: ShutdownItem[];
  /** `.paused` 를 남길 세션 id */
  pause: string[];
}

/** 상태 묶음에서 오는 사실 — 없으면(옛 백엔드·가짜 상태) 셸 탭 줄은 만들지 않는다 */
export interface ShutdownFacts {
  /** 셸이 프롬프트에 있는(자식 프로세스가 없는) 보드 터미널 */
  atPrompt?: TermId[];
  /** 지금 claude 가 도는 보드 터미널 */
  runsClaude?: TermId[];
  now?: number;
}

const isActive = (state: string) => state !== "done";

/** 줄이 되는 까닭 — 한 세션에 여럿이어도 가장 위의 하나만(headlessInTab > waiting > working > subagents > shell) */
function kindOf(session: Session, headless: boolean): ShutdownItem["kind"] | null {
  if (headless) return "headlessInTab";
  if (session.state === "waiting") return "waiting";
  if (session.state === "working") return "working";
  if (session.subagents.some((sub) => isActive(sub.state))) return "subagents";
  if (session.shells > 0) return "shell";
  return null;
}

/** `surfaces` 는 지금 열린 모든 화면 — 탭 root 의 잎 id 가 가리키는 것만 본다. 탭에서 세션·셸을 찾는 데 쓴다 */
export function shutdownList(tabs: BoardTab[], sessions: Session[], surfaces: BoardSurface[], facts: ShutdownFacts = {}): ShutdownList {
  const now = facts.now ?? Date.now();
  const tabOfSurface = new Map<string, BoardTab>();
  for (const tab of tabs) for (const id of leaves(tab.root)) tabOfSurface.set(id, tab);
  const tabSurfaces = surfaces.filter((surface) => tabOfSurface.has(surface.id));
  const plan = pairSessions({ sessions, surfaces: tabSurfaces, runsClaude: facts.runsClaude ?? null, now });

  const items: ShutdownItem[] = [];
  const busySurfaces = new Set<string>();
  const pause = new Set<string>();

  // 화면의 주 세션 — 그 화면의 콘솔(tty)에서 도는 사람이 입력하는 대화형 세션(mac hub.surface(id).session, hub.mode(main) == nil)
  const mainOf = (surface: BoardSurface): Session | undefined =>
    sessions.findLast((s) => s.state !== "ended" && !s.headless && (s.mode ?? null) === null && surface.termId !== null && s.tty === surface.termId);

  for (const session of sessions) {
    const surfaceId = plan.paired[session.id];
    if (session.state === "ended" || surfaceId === undefined) continue;
    const headless = plan.owned[session.id] === true || (session.mode ?? null) !== null;
    const kind = kindOf(session, headless);
    if (kind === null) continue;
    items.push({ sessionId: session.id, title: `${baseName(session.cwd)} · ${session.title}`, kind });
    busySurfaces.add(surfaceId);
  }

  // claude 가 아닌 프로그램이 도는 셸 탭 — 프롬프트에 있지 않고, claude 도 아니고, 짝 세션도 없고, 방금 명령을 친 것도 아니다
  const atPrompt = facts.atPrompt ? new Set(facts.atPrompt) : null;
  const runsClaude = new Set(facts.runsClaude ?? []);
  const pairedSurfaces = new Set(Object.values(plan.paired));
  for (const surface of tabSurfaces) {
    const typing = surface.typedAt != null && now - surface.typedAt < TYPED_WINDOW_MS;
    if (!atPrompt || surface.termId === null || atPrompt.has(surface.termId) || runsClaude.has(surface.termId)) continue;
    if (pairedSurfaces.has(surface.id) || typing) continue;
    const tab = tabOfSurface.get(surface.id)!;
    items.push({ sessionId: null, title: tab.customName ?? baseName(tab.workspace), kind: "shell" });
  }

  for (const surface of tabSurfaces) {
    if (!busySurfaces.has(surface.id)) continue;
    const main = mainOf(surface);
    if (main) pause.add(main.id);
  }
  return { items, pause: [...pause] };
}
