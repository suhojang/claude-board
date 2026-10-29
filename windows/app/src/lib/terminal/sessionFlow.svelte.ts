// 세션 열기 흐름(mac openSession·reveal·resumeSession·toast) — 세션 줄을 눌렀을 때와 백엔드의 "세션 열기" 요청(reopen·focus)을
// 보드 탭으로 옮긴다. 무엇을 할지는 board/openSession.ts 의 순수 표가 정하고, 여기는 그 효과를 hub 에 실행한다.
import {
  doubleClickTime,
  onOpenSession,
  onSnapshot,
  reopenDone,
  sessionAction,
  setTerminalExpanded,
  showBoard,
  takeOpenSession,
  driveSession,
  waitExit,
} from "$lib/backend";
import { createMoveGuard, moveToBoard, sessionAtClick } from "$lib/board/moveToBoard";
import { openSession, type OpenEffect } from "$lib/board/openSession";
import { createClickGate, type ClickGate } from "$lib/board/clickGate";
import { board } from "$lib/board/store.svelte";
import type { BoardSurface, OpenSessionPayload, Session } from "$lib/types";
import { TerminalHub, folderName, leaveSettings, openShell, pair, place, select, showConfirm, surfaceOf, workspaceKey } from "./hub.svelte";
import { uniqueTabName } from "./launch";
import { isMdRenderOff } from "./mdRender";
import { applyRenames } from "./renameFlow.svelte";
import { namedCommand, resumeCommand } from "./sessionCommand";
import { canTakeAsIdle, reopeningActive, type Reopening } from "./reopenGuard";
import { quoteSessionName, sanitizeSessionName } from "./sessionName";
import { hasLeaf } from "./split";
import { typeAtPrompt } from "./typeCommand";

const NOTICE_MS = 6000;
let noticeTimer: ReturnType<typeof setTimeout> | undefined;

/** 보드 안 알림 한 줄을 6초 동안 보인다. 다시 부르면 시간이 새로 시작한다 */
export function toast(text: string) {
  clearTimeout(noticeTimer);
  TerminalHub.notice = text;
  noticeTimer = setTimeout(dismissNotice, NOTICE_MS);
}

export function dismissNotice() {
  clearTimeout(noticeTimer);
  TerminalHub.notice = null;
}

const allSessions = (sessions = board.snap?.groups ?? []): Session[] => sessions.flatMap((g) => g.sessions);
const findSession = (id: string): Session | undefined => allSessions().find((s) => s.id === id);
const tabOfSurface = (surfaceId: string) => TerminalHub.tabs.find((t) => hasLeaf(t.root, surfaceId));

/** 그 화면의 탭을 펼쳐 고르고 보드 창을 앞으로(mac reveal) */
export function reveal(surfaceId: string) {
  leaveSettings();
  const tab = tabOfSurface(surfaceId);
  if (!tab) return;
  if (!board.snap?.settings.terminalExpanded) setTerminalExpanded(true);
  tab.focused = surfaceId;
  select(tab.id);
  showBoard();
}

// 이어 열기를 시작한 끝난 세션의 표지 — 세션이 끝남을 벗어나거나 그 터미널에 claude 가 뜨거나 10초가 지날 때까지 같은 세션을 다시 열지 않는다
const reopening = new Map<string, Reopening>();

function isReopening(id: string): boolean {
  const r = reopening.get(id);
  const surface = r && surfaceOf(r.surfaceId);
  const termRunsClaude = !!surface?.termId && (board.snap?.runsClaude ?? []).includes(surface.termId);
  const active = reopeningActive(r, { now: Date.now(), sessionEnded: findSession(id)?.state === "ended", termRunsClaude });
  if (!active) reopening.delete(id);
  return active;
}

// 셸 프롬프트에 있고 claude 가 없는 같은 경로의 화면 — 끝난 세션을 새 탭 대신 이어 열 자리(mac launch 의 idle 탭)
function idleSurface(dir: string): BoardSurface | undefined {
  const reopeningIds = new Set([...reopening.keys()].filter(isReopening));
  const atPrompt = new Set(board.snap?.atPrompt ?? []);
  const key = workspaceKey(dir);
  return TerminalHub.surfaces.find((x) => {
    const tab = tabOfSurface(x.id);
    const typing = x.typedAt != null && Date.now() - x.typedAt < 3000;
    return canTakeAsIdle(x.sessionId, reopeningIds) && x.termId !== null && atPrompt.has(x.termId) && x.command === null && !typing && tab && workspaceKey(tab.workspace) === key;
  });
}

/** 끝난 세션을 보드 탭에서 이어 연다(mac resumeSession) — 이미 이어 여는 중이거나 탭에 있으면 그 탭을 앞으로 */
function resumeSession(p: OpenSessionPayload) {
  const s = findSession(p.id);
  const where = s && place(s);
  if (where?.kind === "tab") return reveal(where.surfaceId);
  const pending = TerminalHub.surfaces.find((x) => x.sessionId === p.id && x.command !== null);
  if (pending) return reveal(pending.id);
  if (isReopening(p.id)) return reveal(reopening.get(p.id)!.surfaceId);
  let command: string;
  try {
    const name = quoteSessionName(uniqueTabName(sanitizeSessionName(folderName(p.projectDir || "~"))));
    command = namedCommand(resumeCommand(p.resumeArgs, p.id, p.carryOn), name);
  } catch (e) {
    console.error("세션 이어 열기 명령을 만들지 못했습니다", e);
    return toast("세션을 이어 열 수 없습니다");
  }
  if (!board.snap?.settings.terminalExpanded) setTerminalExpanded(true);
  const mdRenderOff = isMdRenderOff(p.id, board.snap?.settings.mdRenderOff ?? []); // 새 세션 창에서 끄고 띄운 세션은 이어 열 때도 끈 채로
  const idle = idleSurface(p.projectDir);
  if (idle?.termId) {
    const termId = idle.termId;
    idle.sessionId = p.id;
    idle.command = command;
    idle.mdRenderOff = mdRenderOff;
    reopening.set(p.id, { since: Date.now(), surfaceId: idle.id });
    void typeAtPrompt(termId, command, {
      alive: () => surfaceOf(idle.id)?.termId === termId && !!TerminalHub.terms[termId],
      touch: () => {
        idle.typedAt = Date.now();
      },
      shell: TerminalHub.terms[termId]?.shell ?? "pwsh", // 터미널 정보가 아직 없으면 보드 기본 셸
      mdRenderOff,
    }).then(() => confirmReopened(p));
    return reveal(idle.id);
  }
  const tab = openShell(p.projectDir || null, { command, sessionId: p.id, mdRenderOff });
  if (tab) {
    reopening.set(p.id, { since: Date.now(), surfaceId: tab.focused });
    reveal(tab.focused);
    confirmReopened(p);
  }
}

// 보드로 옮기기 확인 — 터미널 영역 확인 창(hub confirm)으로 묻는다. 터미널이 접혀 있으면 창이 안 보이므로 펼쳐 보이고, 취소하면 도로 접는다
function confirmMove(title: string, text: string): Promise<boolean> {
  const expandedByUs = !board.snap?.settings.terminalExpanded;
  if (expandedByUs) setTerminalExpanded(true);
  showBoard();
  // 다른 확인 창(탭 닫기 등)이 이 창을 밀어내면 showConfirm 이 이 창의 취소를 불러 주므로 약속이 늘 끝난다
  return new Promise((resolve) => {
    showConfirm({
      title,
      text,
      okLabel: "옮기기",
      run: () => resolve(true),
      cancel: () => {
        if (expandedByUs) setTerminalExpanded(false);
        resolve(false);
      },
    });
  });
}

const onceMoving = createMoveGuard();

/**
 * 다른 터미널에서 도는 세션을 보드 탭으로 옮긴다(mac moveToBoard). 규칙은 board/moveToBoard.ts, 여기는 효과를 이어 준다.
 * 메뉴를 연 뒤 상태가 바뀌었을 수 있어 누른 순간의 세션으로 확인 여부를 정하고, 옮기는 중에 또 누르면 거른다.
 */
export function moveSessionToBoard(opened: Session): Promise<void> {
  const s = sessionAtClick(findSession(opened.id));
  if (!s) {
    toast("세션이 이미 끝났습니다");
    return Promise.resolve();
  }
  return onceMoving(s.id, () =>
    moveToBoard(s, {
      confirm: confirmMove,
      drive: driveSession,
      waitExit,
      // 이어 열기 인자(resumeArgs)는 백엔드 세션 기록이 쥐고 있다 — 보통의 "세션 다시 열기"와 같은 길(reopen 요청)로 보낸다
      resumeSession: (moved) => sessionAction("reopen", moved.id, false),
      toast,
    }),
  )
    .then(() => {})
    .catch((e) => {
      console.error("보드로 옮기지 못했습니다", e);
      toast("보드로 옮기지 못했습니다");
    });
}

// 이어 열기를 마쳤다고 백엔드에 알린다 — 일시중지 표시는 그때 지워진다(실패하면 남아 다시 시도할 수 있다)
function confirmReopened(p: OpenSessionPayload) {
  if (p.carryOn) void reopenDone(p.id).catch(() => {});
}

// 첫 상태 묶음이 오기 전의 요청은 세션 모양을 모르니 미뤄 둔다
const deferred: OpenSessionPayload[] = [];

function handleOpenRequest(p: OpenSessionPayload) {
  if (!board.snap) return void deferred.push(p);
  if (p.action === "reopen") return resumeSession(p);
  const s = findSession(p.id);
  if (s) runEffect(s, openSession(s, place(s), true));
}

function runEffect(s: Session, effect: OpenEffect) {
  if (effect.kind === "reveal") reveal(effect.surfaceId);
  else if (effect.kind === "action") sessionAction(effect.action, s.id, effect.carryOn);
  else if (effect.kind === "notice") toast(effect.text);
}

let doubleClickMs = 500;

/** 보드 시작 때 한 번 — 상태 묶음마다 짝짓기·이름 바꾸기, 세션 열기 요청 받기, 더블 클릭 시간 읽기 */
export function startSessionFlow() {
  onSnapshot((snap) => {
    pair(allSessions(snap.groups), snap.runsClaude ?? null);
    void applyRenames();
    // 화면 상태(board.snap)가 이 묶음으로 갱신된 뒤에 흘린다
    if (deferred.length) queueMicrotask(() => deferred.splice(0).forEach(handleOpenRequest));
  });
  // 듣기가 시작된 뒤에 쌓인 요청을 꺼낸다 — 그 사이의 요청은 이벤트로 오거나 줄에 있다
  void onOpenSession(handleOpenRequest)
    .then(() => takeOpenSession())
    .then((queued) => queued.forEach(handleOpenRequest))
    .catch((e) => console.error("세션 열기 요청을 받지 못했습니다", e));
  void doubleClickTime().then((ms) => (doubleClickMs = ms));
}

export interface RowClick {
  click(e: MouseEvent): void;
  dispose(): void;
}

/**
 * 세션 줄(또는 미니 보드의 경로 줄)의 클릭 처리. 다른 터미널의 줄만 한 번 클릭을 더블 클릭 시간만큼 미룬다(clickGate) —
 * 두 번째 클릭이 오면 activate 없이 focus 만 낸다. 그 밖의 자리는 바로 openSession 표대로 간다.
 * 끝난 세션의 두 번째 클릭은 첫 클릭이 이미 열고 있으므로 무시한다.
 */
export function createRowClick(getSession: () => Session | undefined): RowClick {
  let gate: ClickGate | undefined;
  // 지연된 activate 가 눌린 세션을 가리키도록 클릭 때 세션 id 를 쥔다(그사이 줄의 대표 세션이 바뀌어도 눌린 세션에 간다)
  let clickedId: string | undefined;
  const act = (action: "activate" | "focus") => {
    if (clickedId) sessionAction(action, clickedId);
  };
  const gateOf = () =>
    (gate ??= createClickGate({
      delayMs: doubleClickMs,
      setTimer: (callback, ms) => setTimeout(callback, ms),
      clearTimer: (handle) => clearTimeout(handle),
      onActivate: () => act("activate"),
      onFocus: () => act("focus"),
    }));
  return {
    click(e) {
      const s = getSession();
      if (!s) return;
      leaveSettings(); // 세션 줄을 누르면 설정 화면을 닫고 그 세션으로 간다
      const where = place(s);
      if (where.kind === "terminal") {
        if (e.detail <= 1) clickedId = s.id;
        return gateOf().press(e.detail);
      }
      gate?.dispose();
      if (where.kind === "ended" && e.detail >= 2) return;
      runEffect(s, openSession(s, where, e.detail >= 2));
    },
    dispose: () => gate?.dispose(),
  };
}
