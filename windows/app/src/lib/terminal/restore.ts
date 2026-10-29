// 탭 저장·복원 규칙(mac saveTabs·restoreTabs·resumeHeld) — hub 를 모르는 순수 함수다. node:test 로 바로 시험한다.
// 저장 모양은 types.ts 의 TabEntry·SavedNode·SavedSurface(mac `entry`·`savedNode`). 규칙:
// - 셸 화면은 경로만 남기고, claude 화면은 명령·세션 id·꾸미기 끔까지 남긴다. 명령이 끝나 셸만 남은 화면(command 가 null)은 셸로 저장한다.
// - 다시 뜰 때 모든 화면이 우선 셸로 돌아오고, claude 화면의 이어 열 값은 held 에 담긴다. 고른 탭의 화면만 바로 이어 열고(resumeNow),
//   나머지는 그 탭에 들어갈 때(lifecycle 의 resumeHeld) 이어 연다.
// - 앞 보드가 남긴 세션이 보드 밖에서 아직 도는 중이면(옛 pid 가 살아 있음) 5초까지 기다리고, 그래도 살아 있으면 그 화면은 되살리지 않는다.
// - 저장해 둔 작업 폴더가 지금 없으면 claude 화면은 되살리지도 이어 열지도 않는다(mac restoreCommand). 폴더가 없으면 셸이 홈 폴더로 떠서
//   권한 확인을 건너뛰는 claude 가 홈 폴더 전체를 열게 되기 때문이다. 셸만 있던 화면은 mac 처럼 홈에서 셸로 돌아온다.
import type { BoardSurface, BoardTab, PidTarget, Session, SplitDirection, SplitNode, TabEntry, TermId, TermInfo } from "../types.ts";
import type { SavedNode, SavedSurface } from "../types.ts";
import { freshSessionCommand, namedCommand, resumeCommand } from "./sessionCommand.ts";
import { sessionNameArg } from "./sessionName.ts";
import { leaves } from "./split.ts";

export type { SavedNode, SavedSurface, TabEntry } from "../types.ts";

/** 저장할 때 보는 화면 — 탭이 id 로만 가리키는 화면들·터미널 정보(cwd)·고른 탭 */
export interface RestoreView {
  surfaces: BoardSurface[];
  terms: Record<TermId, TermInfo>;
  selected: string;
}

// ── 저장 ──────────────────────────────────────────────────────

// 아직 들어가지 않아 이어 열지 않은 화면은 남겨 둔 값(held) 그대로 다시 저장한다(mac entry 의 `if let held`)
function savedFromHeld(held: Record<string, string>): SavedSurface {
  return {
    cwd: held.cwd ?? "",
    command: held.command ?? null,
    sessionId: held.session ?? null,
    mdRenderOff: held.mdoff !== undefined,
  };
}

function savedSurface(surface: BoardSurface | undefined, tab: BoardTab, view: RestoreView): SavedSurface {
  if (surface?.held) return savedFromHeld(surface.held);
  const cwd = (surface?.termId ? view.terms[surface.termId]?.cwd : undefined) || tab.workspace;
  if (!surface || surface.command === null) return { cwd, command: null, sessionId: null, mdRenderOff: false };
  return { cwd, command: surface.command, sessionId: surface.sessionId, mdRenderOff: surface.mdRenderOff };
}

function savedNode(node: SplitNode, tab: BoardTab, view: RestoreView): SavedNode {
  if ("leaf" in node) return { leaf: savedSurface(view.surfaces.find((s) => s.id === node.leaf), tab, view) };
  return {
    split: node.split,
    ratio: node.ratio,
    first: savedNode(node.first, tab, view),
    second: savedNode(node.second, tab, view),
  };
}

/** 지금 탭들을 저장 항목으로 — 끝난 명령은 셸로, 아직 이어 열지 않은 화면은 남겨 둔 값 그대로 */
export function savedTabs(tabs: BoardTab[], view: RestoreView): TabEntry[] {
  return tabs.map((tab) => ({
    workspace: tab.workspace,
    name: tab.customName,
    selected: tab.id === view.selected,
    root: savedNode(tab.root, tab, view),
  }));
}

// ── 읽기(디스크의 값은 믿지 않는다) ─────────────────────────

const isObject = (x: unknown): x is Record<string, unknown> => typeof x === "object" && x !== null && !Array.isArray(x);

function parseSurface(x: unknown): SavedSurface | null {
  if (!isObject(x) || typeof x.cwd !== "string") return null;
  const command = typeof x.command === "string" ? x.command : null;
  return {
    cwd: x.cwd,
    command,
    sessionId: command !== null && typeof x.sessionId === "string" && x.sessionId !== "" ? x.sessionId : null,
    mdRenderOff: command !== null && x.mdRenderOff === true,
  };
}

function parseNode(x: unknown): SavedNode | null {
  if (!isObject(x)) return null;
  if ("leaf" in x) {
    const leaf = parseSurface(x.leaf);
    return leaf && { leaf };
  }
  if (x.split !== "right" && x.split !== "down") return null;
  const first = parseNode(x.first);
  const second = parseNode(x.second);
  if (!first || !second) return null;
  const ratio = typeof x.ratio === "number" && Number.isFinite(x.ratio) ? Math.min(Math.max(x.ratio, 0.1), 0.9) : 0.5;
  return { split: x.split as SplitDirection, ratio, first, second };
}

/** 설정에서 읽은 값 가운데 모양이 맞는 탭 항목만 — 깨진 항목 하나가 보드 시작을 막지 않게 건너뛴다 */
export function parseEntries(raw: unknown): TabEntry[] {
  if (!Array.isArray(raw)) return [];
  const entries: TabEntry[] = [];
  for (const x of raw) {
    if (!isObject(x) || typeof x.workspace !== "string") continue;
    const root = parseNode(x.root);
    if (!root) continue;
    entries.push({ workspace: x.workspace, name: typeof x.name === "string" ? x.name : null, selected: x.selected === true, root });
  }
  return entries;
}

// ── 되살리기 ──────────────────────────────────────────────────

export interface RestorePlan {
  tabs: BoardTab[];
  /** tabs 가 가리키는 화면들 */
  surfaces: BoardSurface[];
  /** 고를 탭 id — 저장 때 고른 탭, 표시가 없으면 마지막 탭. 탭이 없으면 null */
  selectedTab: string | null;
  /** 지금 이어 열 화면 id — 고른 탭의 고른 화면. 나머지는 held 에 담아 들어갈 때 이어 연다. 없으면 null */
  resumeNow: string | null;
  /** 작업 폴더가 없어 되살리지 않은 claude 화면의 없는 폴더들(중복 없이, 처음 나온 순서) — 호출한 쪽이 board.log 에 남긴다 */
  missingDirs: string[];
}

/** 홈("~")은 늘 있는 것으로 본다 — 탭 작업 경로 "~" 는 보드가 홈 폴더를 뜻하게 쓴다. 빈 문자열은 폴더가 아니다 */
const isHomeOrEmpty = (dir: string) => dir === "~" || dir === "";

/** claude 화면을 되살리려면 있어야 하는 폴더들 — 셸이 뜨는 탭 작업 경로와 저장해 둔 화면의 작업 경로 */
export function claudeDirs(workspace: string, cwd: string): string[] {
  return [workspace, cwd].filter((dir) => !isHomeOrEmpty(dir));
}

/** 있어야 하는 폴더 가운데 지금 없는 첫 폴더, 다 있으면 null. `existing` 은 백엔드가 있다고 답한 폴더들 */
export function missingDir(workspace: string, cwd: string, existing: ReadonlySet<string>): string | null {
  return claudeDirs(workspace, cwd).find((dir) => !existing.has(dir)) ?? null;
}

/** 저장 항목들의 claude 화면이 요구하는 폴더들(중복 없이) — 백엔드에 있는지 물을 후보 */
export function savedClaudeDirs(entries: TabEntry[]): string[] {
  const dirs = new Set<string>();
  const walk = (workspace: string, node: SavedNode) => {
    if ("leaf" in node) {
      if (node.leaf.command !== null) for (const dir of claudeDirs(workspace, node.leaf.cwd)) dirs.add(dir);
    } else {
      walk(workspace, node.first);
      walk(workspace, node.second);
    }
  };
  for (const entry of entries) walk(entry.workspace, entry.root);
  return [...dirs];
}

/** claude 화면이 다시 뜰 때 이어 열 값 — held 에 그대로 담는다(mac `held`: cwd·command·session·mdoff) */
function heldOf(leaf: SavedSurface): Record<string, string> {
  return {
    cwd: leaf.cwd,
    command: leaf.command ?? "",
    ...(leaf.sessionId ? { session: leaf.sessionId } : {}),
    ...(leaf.mdRenderOff ? { mdoff: "1" } : {}),
  };
}

/**
 * 저장 항목들로 다시 세울 탭·화면과 지금 이어 열 화면을 정한다. `aliveOutside` 는 보드 밖에서 아직 도는 세션 id — 그 화면은 되살리지 않는다.
 * `existingDirs` 는 백엔드가 있다고 답한 폴더들(`savedClaudeDirs` 로 물은 것) — 작업 폴더가 없는 claude 화면은 되살리지 않는다.
 */
export function restorePlan(entries: TabEntry[], aliveOutside: ReadonlySet<string>, existingDirs: ReadonlySet<string>): RestorePlan {
  const surfaces: BoardSurface[] = [];
  const missingDirs: string[] = [];

  // 화면 하나를 만든다. 저장 때 claude 화면이었으면 셸로 띄우고 이어 열 값만 쥔다
  function surfaceOf(leaf: SavedSurface, workspace: string): BoardSurface | null {
    const isClaude = leaf.command !== null;
    if (isClaude && leaf.sessionId && aliveOutside.has(leaf.sessionId)) return null;
    const missing = isClaude ? missingDir(workspace, leaf.cwd, existingDirs) : null;
    if (missing !== null) {
      if (!missingDirs.includes(missing)) missingDirs.push(missing);
      return null;
    }
    const surface: BoardSurface = {
      id: crypto.randomUUID(),
      termId: null,
      sessionId: isClaude ? leaf.sessionId : null,
      command: null,
      held: isClaude ? heldOf(leaf) : null,
      mdRenderOff: isClaude && leaf.mdRenderOff,
      title: "",
      typedAt: null,
    };
    surfaces.push(surface);
    return surface;
  }

  // 되살리지 않는 화면은 빼고 형제가 그 자리를 차지한다
  function nodeOf(node: SavedNode, workspace: string): SplitNode | null {
    if ("leaf" in node) {
      const surface = surfaceOf(node.leaf, workspace);
      return surface && { leaf: surface.id };
    }
    const first = nodeOf(node.first, workspace);
    const second = nodeOf(node.second, workspace);
    if (!first || !second) return first ?? second;
    return { split: node.split, ratio: node.ratio, first, second };
  }

  const tabs: BoardTab[] = [];
  let selectedTab: string | null = null;
  for (const entry of entries) {
    const root = nodeOf(entry.root, entry.workspace);
    if (!root) continue;
    const tab: BoardTab = {
      id: crypto.randomUUID(),
      workspace: entry.workspace,
      customName: entry.name,
      renaming: false,
      root,
      focused: leaves(root)[0],
      zoomed: false,
    };
    tabs.push(tab);
    if (entry.selected) selectedTab = tab.id;
  }
  selectedTab ??= tabs.at(-1)?.id ?? null;
  const picked = tabs.find((t) => t.id === selectedTab);
  const focusedSurface = picked && surfaces.find((s) => s.id === picked.focused);
  return { tabs, surfaces, selectedTab, resumeNow: focusedSurface?.held ? focusedSurface.id : null, missingDirs };
}

/** 저장 항목들이 가리키는 세션 id — 보드 밖에서 아직 도는지 볼 후보 */
export function savedSessionIds(entries: TabEntry[]): string[] {
  const ids = new Set<string>();
  const walk = (node: SavedNode) => {
    if ("leaf" in node) {
      if (node.leaf.command !== null && node.leaf.sessionId) ids.add(node.leaf.sessionId);
    } else {
      walk(node.first);
      walk(node.second);
    }
  };
  for (const entry of entries) walk(entry.root);
  return [...ids];
}

// ── 앞 보드가 남긴 프로세스 기다리기 ──────────────────────────

export interface Clock {
  now(): number;
  sleep(ms: number): Promise<void>;
}

/**
 * 앞 보드가 남긴 프로세스 가운데 `timeoutMs` 뒤에도 살아 있는 것의 pid — `pollMs` 마다 `alive` 로 다시 묻는다.
 * `alive` 는 산 프로세스의 pid 를 돌려준다. 생성 시각(ctime)을 같이 보내므로 재사용된 pid 는 산 것으로 치지 않는다.
 */
export async function waitOldPids(
  targets: PidTarget[],
  alive: (targets: PidTarget[]) => Promise<number[]>,
  clock: Clock,
  timeoutMs = 5000,
  pollMs = 200,
): Promise<Set<number>> {
  const startedAt = clock.now();
  const known = targets.filter((target) => target.pid > 0);
  if (known.length === 0) return new Set();
  const stillAlive = async (list: PidTarget[]) => new Set(await alive(list));
  let remaining = await stillAlive(known);
  while (remaining.size > 0 && clock.now() - startedAt < timeoutMs) {
    await clock.sleep(pollMs);
    remaining = await stillAlive(known.filter((target) => remaining.has(target.pid)));
  }
  return remaining;
}

// ── 저장 목록을 덮어써도 되는가 ───────────────────────────────

/**
 * 시작할 때 저장 목록을 어떻게 맞이했는가.
 * - restored: 저장 항목으로 탭을 다시 세웠다
 * - fresh: 저장 항목이 없다(처음 켬)
 * - adopted: 화면을 새로 고쳐 도는 터미널을 이어받았다 — 이어받은 터미널은 명령·세션 id·탭 이름을 잃은 맨 셸이다
 * - unreadable: 저장 목록을 읽지 못했다
 */
export type RestoreOutcome = "restored" | "fresh" | "adopted" | "unreadable";

/** 지금 탭 모양으로 저장 목록을 덮어써도 되는가 — 이어받은 맨 셸이나 읽지 못한 목록으로 덮으면 되살릴 탭이 사라진다 */
export function mayOverwriteSaved(outcome: RestoreOutcome): boolean {
  return outcome === "restored" || outcome === "fresh";
}

/**
 * 보내는 일을 한 줄로 세운다 — 앞의 것이 끝난 뒤에 다음을 보내 순서가 뒤집히지 않고, 하나가 실패해도 뒤의 것을 막지 않는다.
 * 돌려주는 함수는 그 항목까지 보내는 일이 끝나면(실패해도) 풀리는 약속을 준다. 실패는 `onError` 로 알린다.
 */
export function serialSender<T>(send: (item: T) => Promise<void>, onError: (error: unknown) => void): (item: T) => Promise<void> {
  let tail: Promise<void> = Promise.resolve();
  return (item) => {
    tail = tail.then(() => send(item)).catch(onError);
    return tail;
  };
}

// ── 이어 열기(resumeHeld) ─────────────────────────────────────

/**
 * 이어 열 때 아는 것 — sessions 는 보드 상태의 세션들(아직 못 받았으면 null), tabName 은 탭 줄 이름.
 * `alive` 는 이어 열 순간에 백엔드에 물은 생사다(상태 묶음의 state 는 1초까지 묵을 수 있다) — 그 세션 id 의 프로세스가 지금 살아 있는가.
 * `existingDirs` 는 백엔드가 있다고 답한 폴더들, `workspace` 는 지금 탭의 작업 경로(셸이 뜬 폴더).
 */
export interface HeldContext {
  sessions: ReadonlyArray<Pick<Session, "id" | "state"> & { resumeArgs?: string[]; alive?: boolean }> | null;
  tabName: string;
  workspace: string;
  existingDirs: ReadonlySet<string>;
}

/** 그 세션이 끝나지 않은 것으로 보여 프로세스 생사를 물어봐야 하는가 — 물을 pid·생성 시각, 물을 필요가 없으면 null */
export function liveCheckTarget(
  sessionId: string | undefined,
  sessions: ReadonlyArray<Pick<Session, "id" | "state" | "pid" | "pidCtime">> | null,
): PidTarget | null {
  const known = sessionId ? sessions?.find((s) => s.id === sessionId) : undefined;
  if (!known || known.state === "ended" || known.pid <= 0) return null;
  return { pid: known.pid, ctime: known.pidCtime ?? null };
}

export interface HeldLaunch {
  command: string;
  /** 화면이 쥘 세션 id — 새 세션이면 새로 붙인 id, claude 가 아닌 명령이면 null */
  sessionId: string | null;
}

/**
 * held 에 담아 둔 값으로 칠 명령을 정한다(mac restoreCommand). 돌려주는 null 은 이어 열지 않고 셸로 둔다는 뜻이다.
 * - 작업 폴더(탭 작업 경로·저장해 둔 경로)가 지금 없다 → null (셸이 홈으로 떠 있다. 거기서 claude 를 띄우지 않는다)
 * - 세션이 보드 상태에 있고 끝나지 않았고 프로세스가 지금도 산다 = 보드 밖에서(다른 터미널·백그라운드) 이어 열렸다 → null.
 *   상태 묶음이 아직 "끝나지 않음"이어도 `alive` 가 false 면(방금 죽은 옛 보드의 claude) 끝난 것으로 본다
 * - 세션이 보드 상태에 있다(끝남) → `claude [그 세션의 이어 열기 인자] --resume <id>`
 * - 세션이 보드 상태에 없다 = 대화 없이 끝나 지워졌다 → 처음 명령을 새 세션 id 로
 * - 보드 상태를 아직 못 받았다 → 이어 열기(아는 인자가 없다)
 */
export function heldLaunch(held: Record<string, string>, context: HeldContext): HeldLaunch | null {
  const command = held.command;
  if (!command) return null;
  if (missingDir(context.workspace, held.cwd ?? "", context.existingDirs) !== null) return null;
  const name = sessionNameArg(context.tabName);
  const sessionId = held.session;
  if (sessionId) {
    const known = context.sessions?.find((s) => s.id === sessionId);
    if (known && known.state !== "ended" && known.alive !== false) return null;
    if (known || context.sessions === null) {
      try {
        return { command: namedCommand(resumeCommand(known?.resumeArgs ?? [], sessionId, false), name), sessionId };
      } catch {
        return null; // 모양이 맞지 않는 세션 id — 명령에 넣지 않는다
      }
    }
  }
  const fresh = freshSessionCommand(command);
  return { command: namedCommand(fresh.command, name), sessionId: fresh.sessionId };
}
