// 수명 주기 — 보드를 켤 때 탭 되살리기(restore.ts), 탭이 바뀔 때 저장, 끌 때 확인 창(board://confirm-shutdown)을 hub 에 이어 준다.
// 무엇을 저장·복원하고 누구를 확인 창에 보이는지는 restore.ts·shutdownList.ts 가 정한다. 여기는 그 결과를 hub 상태와 백엔드에 실행한다.
import { untrack } from "svelte";
import {
  answerShutdown,
  existingDirs,
  lifecycleStarted,
  loadTabs,
  log,
  onConfirmShutdown,
  onSnapshot,
  pidsAlive,
  pruneMdRenderOff,
  saveTabs,
  showBoard,
  shutdownDialogShown,
} from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import type { BoardSurface, ConfirmShutdownPayload, PidTarget, Session, Snapshot, TabEntry } from "$lib/types";
import { restoreSettingsBeforeQuit } from "$lib/settings/screen.svelte";
import { TerminalHub, controllerOf, focusActive, select, shownName, surfaceOf } from "./hub.svelte";
import {
  claudeDirs,
  heldLaunch,
  liveCheckTarget,
  mayOverwriteSaved,
  parseEntries,
  restorePlan,
  savedClaudeDirs,
  savedSessionIds,
  savedTabs,
  serialSender,
  waitOldPids,
  type Clock,
  type RestoreOutcome,
} from "./restore";
import { shutdownList, type ShutdownList } from "./shutdownList";
import { hasLeaf, leaves } from "./split";
import { sleep, typeCommand } from "./typeCommand";

/** 확인 창 상태 — ShutdownList.svelte 가 읽는다. 떠 있지 않으면 null */
export const Lifecycle = $state({
  confirm: null as ConfirmShutdownPayload | null,
});

/** 저장한 탭 목록을 쓰기 전에 기다리는 시간(ms) — 짧은 사이에 여러 번 바뀌는 것을 한 번으로 묶는다 */
const SAVE_DEBOUNCE_MS = 300;
/** 첫 상태 묶음을 기다리는 시간(ms) — 넘으면 세션을 모르는 채 되살린다(이어 열기는 세션 id 로 한다) */
const FIRST_SNAPSHOT_WAIT_MS = 3000;
/** 끄기·업데이트를 고른 뒤 이 시간(ms) 동안은 저장하지 않는다 — 터미널이 먼저 죽어 탭이 닫혀 가는 모습을 저장하면 목록이 줄어든다.
 * 끄기가 끝내 일어나지 않았으면(업데이트 실패 등) 이 시간 뒤의 지금 모습을 저장한다 */
const FROZEN_AFTER_SHUTDOWN_MS = 10_000;

const realClock: Clock = { now: () => Date.now(), sleep };

const sessionsOf = (snap: Snapshot | null | undefined): Session[] => snap?.groups.flatMap((g) => g.sessions) ?? [];

// ── 저장 ──────────────────────────────────────────────────────

let lastSent: string | null = null;
let pendingSave: { json: string; entries: TabEntry[] } | null = null;
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let frozenUntil = 0;
/** 시작할 때 저장 목록을 어떻게 맞이했는가 — 정해지기 전(null)과 이어받은 맨 셸·못 읽은 목록일 때는 저장 목록을 덮어쓰지 않는다 */
let restoreOutcome: RestoreOutcome | null = null;
/** 보낸 저장이 모두 끝나면 풀리는 약속 — 끄기 직전에 기다린다 */
let lastSend: Promise<void> = Promise.resolve();

// 저장은 한 줄로 세워 보낸다 — 순서가 뒤집히면 오래된 목록이 새 목록을 덮는다. 실패하면 같은 내용을 다시 보낼 수 있게 한다
const sendEntries = serialSender<TabEntry[]>(saveTabs, (e) => {
  lastSent = null;
  console.error("탭 목록을 저장하지 못함", e);
});

function currentEntries() {
  return savedTabs(TerminalHub.tabs, { surfaces: TerminalHub.surfaces, terms: TerminalHub.terms, selected: TerminalHub.selected });
}

const mayOverwrite = () => restoreOutcome !== null && mayOverwriteSaved(restoreOutcome);

function sendPendingSave(force = false): Promise<void> {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  const frozenFor = frozenUntil - Date.now();
  if (!force && frozenFor > 0) {
    saveTimer = setTimeout(() => void sendPendingSave(), frozenFor);
    return lastSend;
  }
  const next = pendingSave;
  pendingSave = null;
  if (!next || next.json === lastSent || !mayOverwrite()) return lastSend;
  lastSent = next.json;
  lastSend = sendEntries(next.entries);
  return lastSend;
}

/** 바뀐 탭 목록을 잠깐 모았다가 저장한다(mac saveTabs — 바뀔 때마다. 같으면 쓰지 않는다) */
function scheduleSave(entries: TabEntry[] = currentEntries()) {
  pendingSave = { json: JSON.stringify(entries), entries };
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void sendPendingSave(), SAVE_DEBOUNCE_MS);
}

/** 지금 탭 목록을 기다리지 않고 저장한다 — 끄기 직전·되살리기를 마친 직후. 돌려주는 약속은 보낸 저장이 모두 끝나면 풀린다 */
function flushSave(): Promise<void> {
  scheduleSave();
  return sendPendingSave(true);
}

// ── 이어 열기(resumeHeld) ─────────────────────────────────────

/** 되살린 뒤 아직 들어가지 않은 화면의 claude 를 이어 연다 — 그 탭을 고를 때. 명령은 그때 다시 정한다(그사이 다른 곳에서 이어 열렸으면 셸로 둔다) */
function resumeHeld(surface: BoardSurface) {
  const held = surface.held;
  const tab = TerminalHub.tabs.find((t) => hasLeaf(t.root, surface.id));
  const state = TerminalHub.leaves[surface.id];
  if (!held || !tab || !state) return;
  // 셸이 못 떴으면 값을 쥐고 있는다 — 다시 시도해 뜨면 그때 이어 연다
  if (state.phase === "failed" || state.phase === "exited") return;
  if (resuming.has(surface.id)) return;
  resuming.add(surface.id);
  void launchHeld(surface, held, tab.workspace)
    .catch((e) => console.error("탭을 이어 열지 못함", e))
    .finally(() => resuming.delete(surface.id));
}

/** 이어 열 값을 백엔드에 묻는 중인 화면 — 묻는 사이에도 held 를 쥐고 있어 저장 목록이 그 화면을 셸로 적지 않는다 */
const resuming = new Set<string>();

/** 이어 열 순간의 생사·폴더를 백엔드에 묻고 칠 명령을 정해 친다 — 상태 묶음은 1초까지 묵을 수 있어 그 값으로 판정하지 않는다(mac isLiveClaude) */
async function launchHeld(surface: BoardSurface, held: Record<string, string>, workspace: string) {
  const sessions = board.snap ? sessionsOf(board.snap) : null;
  const target = liveCheckTarget(held.session, sessions);
  let alive: boolean | undefined;
  if (target) {
    try {
      alive = (await pidsAlive([target])).includes(target.pid);
    } catch (e) {
      console.error("세션 프로세스 생사를 묻지 못함 — 상태 묶음을 따른다", e);
    }
  }
  // 폴더를 확인하지 못하면 없는 것으로 본다 — 확인 없이 claude 를 띄우지 않는다
  let existing = new Set<string>();
  let checked = true;
  try {
    existing = new Set(await existingDirs(claudeDirs(workspace, held.cwd ?? "")));
  } catch (e) {
    checked = false;
    console.error("작업 경로를 확인하지 못함", e);
  }
  const tab = TerminalHub.tabs.find((t) => hasLeaf(t.root, surface.id));
  const state = TerminalHub.leaves[surface.id];
  if (!tab || !state || surfaceOf(surface.id) !== surface) return; // 기다리는 사이 탭이 닫혔다
  surface.held = null;
  const launch = heldLaunch(held, {
    sessions: sessions?.map((s) => ({ id: s.id, state: s.state, resumeArgs: s.resumeArgs, alive: s.id === held.session ? alive : undefined })) ?? null,
    tabName: shownName(tab),
    workspace: tab.workspace,
    existingDirs: existing,
  });
  if (!launch) {
    const missing = held.command ? claudeDirs(tab.workspace, held.cwd ?? "").find((dir) => !existing.has(dir)) : undefined;
    if (missing !== undefined) log(checked ? `탭을 이어 열지 않음: 작업 경로가 없습니다 ${missing}` : `탭을 이어 열지 않음: 작업 경로를 확인하지 못했습니다 ${missing}`);
    return;
  }
  if (launch.sessionId) surface.sessionId = launch.sessionId;
  if (state.phase === "idle" || state.phase === "starting" || !surface.termId) {
    // 셸이 뜨는 중이거나 아직 안 떴다 — startLeaf 가 셸이 뜬 뒤 한 번 친다
    surface.command = launch.command;
    surface.typedAt = Date.now();
    return;
  }
  const termId = surface.termId;
  surface.command = launch.command;
  void typeCommand(controllerOf(surface.id), termId, launch.command, {
    alive: () => surfaceOf(surface.id)?.termId === termId && !!TerminalHub.terms[termId],
    touch: () => {
      const live = surfaceOf(surface.id);
      if (live) live.typedAt = Date.now();
    },
    shell: TerminalHub.terms[termId]?.shell ?? "pwsh",
    mdRenderOff: surface.mdRenderOff,
  });
}

// ── 되살리기 ──────────────────────────────────────────────────

function firstSessions(): Promise<Session[] | null> {
  if (board.snap) return Promise.resolve(sessionsOf(board.snap));
  return new Promise((resolve) => {
    let settled = false;
    let off = () => {};
    const finish = (value: Session[] | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      off();
      resolve(value);
    };
    const timer = setTimeout(() => finish(null), FIRST_SNAPSHOT_WAIT_MS);
    off = onSnapshot((snap) => finish(sessionsOf(snap)));
    if (settled) off();
  });
}

/**
 * 저장해 둔 탭을 되살린다. 이미 탭이 있으면(새로 고침으로 터미널을 이어받음) 건드리지 않고 저장 목록도 덮어쓰지 않는다 —
 * 이어받은 터미널은 명령·세션 id·탭 이름을 잃은 맨 셸이라 그 모양을 저장하면 되살릴 탭이 사라진다.
 */
async function restoreSavedTabs(): Promise<RestoreOutcome> {
  if (TerminalHub.tabs.length > 0) {
    log("화면을 새로 고쳐 도는 터미널을 이어받았습니다 — 저장해 둔 탭 목록은 덮어쓰지 않습니다");
    return "adopted";
  }
  let entries: TabEntry[] = [];
  try {
    entries = parseEntries(await loadTabs());
  } catch (e) {
    console.error("저장해 둔 탭 목록을 읽지 못함", e);
    log("저장해 둔 탭 목록을 읽지 못해 덮어쓰지 않습니다");
    return "unreadable";
  }
  if (entries.length === 0) return "fresh";
  const sessions = await firstSessions();
  // 앞 보드가 남긴 claude 가 보드 밖에서 아직 도는 중이면(업데이트 직후 등) 같은 세션을 둘이 쥐지 않게 최대 5초 기다린다
  const candidates = new Map<number, { sessionId: string; target: PidTarget }>();
  for (const id of savedSessionIds(entries)) {
    const target = liveCheckTarget(id, sessions);
    if (target) candidates.set(target.pid, { sessionId: id, target });
  }
  const stillAlive = await waitOldPids([...candidates.values()].map((c) => c.target), pidsAlive, realClock);
  const aliveOutside = new Set([...stillAlive].map((pid) => candidates.get(pid)!.sessionId));
  // 저장해 둔 작업 폴더가 지금 있는지 — 없으면 셸이 홈으로 떠 claude 를 거기서 띄우게 된다. 확인하지 못하면 되살리지 않는다
  let existing: Set<string>;
  try {
    existing = new Set(await existingDirs(savedClaudeDirs(entries)));
  } catch (e) {
    console.error("저장해 둔 작업 경로를 확인하지 못함", e);
    log("저장해 둔 작업 경로를 확인하지 못해 탭을 되살리지 않고 목록을 덮어쓰지 않습니다");
    return "unreadable";
  }
  // 기다리는 사이 사용자가 탭을 열었으면 그 탭 뒤에 이어 붙인다
  const plan = restorePlan(entries, aliveOutside, existing);
  for (const dir of plan.missingDirs) log(`탭을 되살리지 않음: 작업 경로가 없습니다 ${dir}`);
  for (const surface of plan.surfaces) {
    TerminalHub.surfaces.push(surface);
    TerminalHub.leaves[surface.id] = { phase: "idle", exitCode: null, error: "" };
  }
  TerminalHub.tabs.push(...plan.tabs);
  if (TerminalHub.selected === "" && plan.selectedTab) {
    select(plan.selectedTab, false);
    const resumeNow = plan.resumeNow ? surfaceOf(plan.resumeNow) : undefined;
    if (resumeNow) resumeHeld(resumeNow);
  }
  return "restored";
}

// ── 끌 때 확인 ────────────────────────────────────────────────

function currentShutdownList(): ShutdownList {
  return shutdownList(TerminalHub.tabs, sessionsOf(board.snap), TerminalHub.surfaces, {
    atPrompt: board.snap?.atPrompt,
    runsClaude: board.snap?.runsClaude,
  });
}

/** 저장이 끝나기를 기다린다 — 저장 실패가 끄기 답을 막지 않게 오류는 로그만 남긴다 */
async function saved(pendingSaved: Promise<void>) {
  try {
    await pendingSaved;
  } catch (e) {
    console.error("끄기 전 저장을 마치지 못함", e);
  }
}

async function answer(ok: boolean, pause: string[]) {
  try {
    await answerShutdown(ok, pause);
  } catch (e) {
    console.error("끌 때 확인의 답을 보내지 못함", e);
  }
}

async function onConfirmRequest(request: ConfirmShutdownPayload) {
  if (request.reason === "cancel") {
    // 백엔드가 답을 기다리다 포기했다 — 떠 있는 확인 창을 닫는다
    if (Lifecycle.confirm) {
      Lifecycle.confirm = null;
      focusActive();
    }
    return;
  }
  const { items } = currentShutdownList();
  // 물어볼 것이 없으면 곧 꺼진다 — 설정 화면이 접힌 보드를 잠깐 펼쳐 둔 것이면 접는 저장을 마친 뒤에 답한다. 확인 창을 띄우는 길은 사용자가 취소할 수 있어 answerConfirm 이 한다
  if (items.length === 0) await restoreSettingsBeforeQuit();
  const pendingSaved = flushSave();
  if (items.length === 0) {
    frozenUntil = Date.now() + FROZEN_AFTER_SHUTDOWN_MS;
    await saved(pendingSaved);
    return answer(true, []);
  }
  // 보드가 숨어 있거나 접혀 있어도 보이게 한다 — 창을 띄웠다고 알려야 백엔드가 사용자가 고를 때까지 기다린다.
  // 같은 요청이 다시 오면(트레이 "종료"를 또 누름) 창을 다시 세워 보인다
  Lifecycle.confirm = { reason: request.reason, items };
  showBoard();
  shutdownDialogShown();
}

/** 확인 창의 답 — 계속(ok)이면 고른 순간의 세션으로 다시 세어 `.paused` 대상을 넘긴다(창을 띄워 둔 사이 끝났거나 새로 일을 시작한 세션이 있을 수 있다).
 * 마지막 저장이 끝난 뒤에 답한다 — 답을 받으면 백엔드가 곧 끝내므로 저장이 끝나기 전에 답하면 마지막 변경이 사라진다 */
export async function answerConfirm(ok: boolean) {
  if (!Lifecycle.confirm) return;
  const pause = ok ? currentShutdownList().pause : [];
  Lifecycle.confirm = null;
  if (ok) await restoreSettingsBeforeQuit();
  const pendingSaved = flushSave();
  if (ok) frozenUntil = Date.now() + FROZEN_AFTER_SHUTDOWN_MS;
  await saved(pendingSaved);
  await answer(ok, pause);
  if (!ok) focusActive();
}

// ── 시작 ──────────────────────────────────────────────────────

let started = false;

/** 보드를 켤 때 세션 파일이 없어진 세션의 id 를 꾸미기 끈 목록에서 치운다 — 첫 상태 묶음이 와서 세션을 읽은 뒤에 */
function pruneStaleMdRenderOff() {
  void firstSessions()
    .then((sessions) => (sessions ? pruneMdRenderOff() : undefined))
    .catch((e) => console.error("꾸미기 끈 세션 목록을 정리하지 못함", e));
}

/** 시작 때 한 번 — hub 의 startHub 가 부른다. 여러 번 불러도 한 번만 한다 */
export function startLifecycle(): void {
  if (started) return;
  started = true;
  // 듣기가 켜진 뒤 백엔드에 알린다 — 그 전에 보낸 확인 요청은 이 화면이 받지 못했으므로 백엔드가 기다리던 요청을 취소한다
  void onConfirmShutdown((request) => void onConfirmRequest(request).catch((e) => console.error("끌 때 확인 요청을 처리하지 못함", e)))
    .then(() => lifecycleStarted())
    .catch((e) => console.error("끌 때 확인 듣기를 켜지 못함", e));
  pruneStaleMdRenderOff();
  void restoreSavedTabs()
    .catch((e): RestoreOutcome => {
      console.error("탭을 되살리지 못함", e);
      return "unreadable";
    })
    .then((outcome) => {
      restoreOutcome = outcome;
      // 되살리기를 마친 뒤부터 바뀔 때마다 저장한다 — 마치기 전에 저장하면 지난 목록을 덮는다. 첫 저장은 백엔드에 수명 주기가 켜졌다고 알린다
      void flushSave();
      $effect.root(() => {
        // 탭 목록·화면·터미널 경로·고른 탭을 읽는 것만으로 의존이 잡힌다
        $effect(() => {
          const entries = currentEntries();
          untrack(() => scheduleSave(entries));
        });
        // 탭에 들어갈 때 그 탭의 아직 안 이어 연 화면을 이어 연다
        $effect(() => {
          const tab = TerminalHub.tabs.find((t) => t.id === TerminalHub.selected);
          if (!tab) return;
          untrack(() => {
            for (const id of leaves(tab.root)) {
              const surface = surfaceOf(id);
              if (surface?.held) resumeHeld(surface);
            }
          });
        });
      });
    });
}
