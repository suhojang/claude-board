// 터미널 저장소(TerminalHub) — 화면이 보는 터미널 목록·탭·경로·고른 탭과 칸별 컨트롤러.
// macOS 보드의 TerminalHub(ClaudeBoard.swift TerminalHub)와 같은 규칙이다: 탭은 작업 경로(workspace)로 묶여 경로 줄에 서고,
// 탭 줄은 지금 경로의 탭만 보인다. 셸이 끝나면 그 탭이 닫히고 선택은 같은 경로의 이웃 탭 → 이웃 경로 → 빈 상태로 간다.
// 터미널 출력은 보드의 1초 상태 묶음에 싣지 않고, 여기서 term_list 와 채널로 채운다(이벤트 기반).
import { tick } from "svelte";
import { inTauri, rememberMdRenderOff, setTerminalExpanded, termKill, termList, termSpawn, testClaudePrefix } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import { mockTabs } from "$lib/mock";
import type { BoardSurface, BoardTab, LeafPhase, Session, State, TermId, TermInfo } from "$lib/types";
import {
  folderNameOf,
  homeFromGroups,
  insertIndexFor,
  mostUrgent,
  needsCloseConfirm,
  nthTab,
  pickAfterClose,
  renamedTo,
  shownTabOf,
  tabsInWorkspace,
  workspaceHintOf,
  workspaceKeyOf,
  workspaceLabelOf,
  workspacesOf,
  wrapStep,
} from "./tabLogic";
import { pairSessions, placeOf, type SessionPlace, type SurfaceChange } from "./place";
import { startLifecycle } from "./lifecycle.svelte";
import { loadPluginDir } from "./mdRenderLoad";
import { loadTestClaudePrefix } from "./testClaude";
import { hasLeaf, leaves } from "./split";
import { startTabRenames } from "./renameFlow.svelte";
import { startSessionFlow } from "./sessionFlow.svelte";
import { TermController } from "./term";
import { typeCommand } from "./typeCommand";

export interface LeafState {
  phase: LeafPhase;
  exitCode: number | null;
  error: string;
}

/** 탭을 닫기 전에 묻는 창 — TerminalAreaView 가 그린다 */
export interface Confirm {
  title: string;
  text: string;
  run: () => void;
  /** 확인 단추 문구 — 없으면 "닫기" */
  okLabel?: string;
  /** 취소(Esc·취소 단추)했을 때 — 없으면 창만 닫는다 */
  cancel?: () => void;
}

export const TerminalHub = $state({
  terms: {} as Record<TermId, TermInfo>,
  tabs: [] as BoardTab[],
  /** 모든 화면(mac BoardSurface). 탭의 root 트리가 id 로 가리킨다 */
  surfaces: [] as BoardSurface[],
  /** 고른 탭 id. 탭이 없으면 빈 문자열 */
  selected: "",
  /** 지금 경로 줄에서 고른 경로(탭 줄이 보여 주는 경로). 탭이 없으면 빈 문자열 */
  workspace: "",
  /** 경로마다 마지막에 고른 탭(경로 키 → 탭 id) */
  lastTab: {} as Record<string, string>,
  /** 수정키(Ctrl+Alt)를 누르고 있는 동안 true — 탭의 번호 배지와 이름 바꾸기 안내를 보인다 */
  commandHeld: false,
  confirm: null as Confirm | null,
  /** 새 세션 창이 떠 있는가 */
  sheet: false,
  leaves: {} as Record<string, LeafState>,
  /** 세션·화면 짝짓기(SPEC §5.12, W2) — 사는 세션 id → 그 세션이 달린 화면 id. pair() 가 상태 묶음마다 다시 쥔다 */
  paired: {} as Record<string, string>,
  /** 화면 안에서 시작한 headless 세션만(mac TerminalHub.tabOwnedHeadless) — 칩 "CB -p" 의 함께 끝남 근거 */
  tabOwnedHeadless: {} as Record<string, true>,
  /** 보드 안 알림 한 줄(mac TerminalHub.notice) — 6초 뒤 사라진다. TerminalNotice 가 그린다 */
  notice: null as string | null,
});

// 컨트롤러는 xterm 을 쥔 일반 객체라 반응형 상태에 넣지 않는다
const controllers = new Map<string, TermController>();
let started = false;

export function controllerOf(leafId: string): TermController {
  let ctl = controllers.get(leafId);
  if (!ctl) {
    ctl = new TermController(leafId, {
      onExit: (code) => terminalDidClose(leafId, code),
      onTitle: (title) => {
        const surface = surfaceOf(leafId);
        // 셸이 처음에 보내는 제목은 실행 파일 경로(C:\Program Files\...\pwsh.exe)라 보이지 않게 한다
        if (surface) surface.title = /\.exe"?$/i.test(title.trim()) ? "" : title;
      },
    });
    controllers.set(leafId, ctl);
  }
  return ctl;
}

// ── 경로 ──────────────────────────────────────────────────────

// 홈 폴더 — 상태 묶음의 그룹이 홈을 "~" 로 줄여 보내므로 id(전체 경로)와 path(줄인 경로)의 차이에서 알아낸다
function homeDir(): string | null {
  return homeFromGroups(board.snap?.groups ?? []);
}

/** 경로를 견주는 꼴 — 대소문자·슬래시 방향·끝 \ 차이로 같은 폴더가 어긋나지 않게, 홈은 "~" */
export function workspaceKey(path: string): string {
  return workspaceKeyOf(path, homeDir());
}

/** 폴더 이름(홈은 ~) */
export function folderName(workspace: string) {
  return folderNameOf(workspace, homeDir());
}

/** 경로 줄 이름 — 폴더 이름(홈은 ~). 같은 이름이 둘 이상이면 부모 폴더를 붙여 가른다 */
export function workspaceLabel(ws: string, among: string[]): string {
  return workspaceLabelOf(ws, among, homeDir());
}

/** 경로 줄 항목의 힌트 — 전체 경로(홈은 ~) */
export function workspaceHint(ws: string): string {
  return workspaceHintOf(ws, homeDir());
}

/** 경로 줄의 경로들 — 탭 목록에 처음 나타난 순서 */
export function workspaces(): string[] {
  return workspacesOf(TerminalHub.tabs, homeDir());
}

export function tabsIn(ws: string): BoardTab[] {
  return tabsInWorkspace(TerminalHub.tabs, ws, homeDir());
}

/** 탭 줄에 보이는 이름 — 사용자가 바꾼 이름, 없으면 폴더 이름 */
export function shownName(tab: BoardTab) {
  return tab.customName ?? folderName(tab.workspace);
}

// ── 탭 상태 ───────────────────────────────────────────────────

/** 탭에서 고른 화면 */
function focusedSurface(tab: BoardTab): BoardSurface | undefined {
  return surfaceOf(tab.focused);
}

function sessionOf(tab: BoardTab): Session | null {
  const sessionId = focusedSurface(tab)?.sessionId;
  if (!sessionId) return null;
  for (const g of board.snap?.groups ?? []) {
    const s = g.sessions.find((x) => x.id === sessionId);
    if (s) return s;
  }
  return null;
}

/** 탭 상태 — 그 탭 화면들의 세션 가운데 가장 급한 것(세션이 없으면 idle). 지금은 탭마다 세션 하나다 */
export function tabState(tab: BoardTab): State {
  return sessionOf(tab)?.state ?? "idle";
}

/** 경로 상태 — 그 경로 탭들 가운데 가장 급한 상태 */
export function workspaceState(ws: string): State {
  return mostUrgent(tabsIn(ws).map(tabState));
}

/** 탭이 지금 하는 일 — 승인 대기이고 message 가 있으면 message, 아니면 세션 제목. 셸 탭은 터미널 제목 */
export function doing(tab: BoardTab): string {
  const s = sessionOf(tab);
  if (!s) return focusedSurface(tab)?.title ?? "";
  return s.state === "waiting" && s.message ? s.message : s.title;
}

/** 탭에서 도는 프로그램이 있는가 — 짝 세션이 아직 끝나지 않았을 때(셸만 있는 탭은 바로 닫는다) */
function isBusy(tab: BoardTab): boolean {
  return needsCloseConfirm(sessionOf(tab)?.state);
}

// ── 탭 만들기·찾기 ────────────────────────────────────────────

function newTab(termId: TermId | null, workspace: string): BoardTab {
  const surface: BoardSurface = {
    id: crypto.randomUUID(),
    termId,
    sessionId: null,
    command: null,
    held: null,
    mdRenderOff: false,
    title: "",
    typedAt: null,
  };
  TerminalHub.surfaces.push(surface);
  TerminalHub.leaves[surface.id] = { phase: "idle", exitCode: null, error: "" };
  return {
    id: crypto.randomUUID(),
    workspace,
    customName: null,
    renaming: false,
    root: { leaf: surface.id },
    focused: surface.id,
    zoomed: false,
  };
}

/** 화면 id 로 화면을 찾는다 */
export function surfaceOf(id: string): BoardSurface | undefined {
  return TerminalHub.surfaces.find((s) => s.id === id);
}

function tabOfLeaf(leafId: string): BoardTab | undefined {
  return TerminalHub.tabs.find((t) => hasLeaf(t.root, leafId));
}

export function selectedTab(): BoardTab | undefined {
  return TerminalHub.tabs.find((t) => t.id === TerminalHub.selected);
}

export function focusActive() {
  const tab = selectedTab();
  if (tab && !tab.renaming) controllers.get(tab.focused)?.focus();
}

/** 고른 탭에서 조합 중인 글자가 있으면 확정한다 */
export function commitComposition() {
  const tab = selectedTab();
  if (tab && !tab.renaming) controllers.get(tab.focused)?.commitComposition();
}

// 고른 탭이 그려진(보이게 된) 뒤에 터미널로 포커스를 돌려준다
function focusSelected() {
  void tick().then(() => requestAnimationFrame(focusActive));
}

const expanded = () => board.snap?.settings.terminalExpanded ?? false;

// ── 세션·화면 짝짓기(SPEC §5.12, W2) ──────────────────────────

/**
 * 화면 하나에 짝짓기 고침을 새긴다(place.ts 의 changes 는 hub 만 화면을 고친다).
 * 화면이 이미 닫혔으면 건너뛴다 — 스냅샷이 고침보다 뒤에 도는 것은 닫힘과의 경합이라 숱하게 일어나는 일이고,
 * 닫힌 화면에 새길 고침은 더는 의미가 없다(다음 스냅샷이 여는 화면에 새 고침을 준다)
 */
function applyChange(change: SurfaceChange) {
  const surface = surfaceOf(change.surfaceId);
  if (!surface) return;
  if (change.kind === "sessionId") {
    surface.sessionId = change.sessionId;
    if (surface.mdRenderOff) rememberMdRenderOff(change.sessionId); // 끈 화면에서 세션 id 가 바뀌어도(/clear 등) 끈 채로 이어 열게
  } else if (change.kind === "command") surface.command = change.command;
  else {
    // claude 가 끝나 셸 화면으로 돌아간다 — 명령을 비우고 꾸미기 끔을 푼다(place.test 의 surfaceAfter 가 같은 몸을 쓴다)
    surface.command = null;
    surface.mdRenderOff = false;
  }
}

/** 상태 묶음 세션들로 짝짓기를 다시 쥔다 — 화면을 먼저 고치고 짝·소유 표를 갈아낸다(mac TerminalHub.pair) */
export function pair(sessions: Session[], runsClaude: TermId[] | null) {
  const plan = pairSessions({ sessions, surfaces: TerminalHub.surfaces, runsClaude });
  for (const change of plan.changes) applyChange(change);
  TerminalHub.paired = { ...plan.paired };
  TerminalHub.tabOwnedHeadless = { ...plan.owned };
}

/** 세션이 어디서 도는지 화면 고침 뒤의 상태로 가른다(mac TerminalHub.place) — 세션 줄의 위치 칩이 부른다 */
export function place(s: Session): SessionPlace {
  return placeOf(s, { paired: TerminalHub.paired, owned: TerminalHub.tabOwnedHeadless }, TerminalHub.surfaces, board.snap?.runsClaude ?? null);
}

// ── 시작 ──────────────────────────────────────────────────────

/** 페이지를 새로 고친 뒤 아직 도는 터미널을 탭으로 이어받는다. 빈 탭은 만들지 않는다 — 탭이 생길 때까지 빈 상태. 여러 번 불러도 한 번만 한다 */
export async function startHub() {
  if (started) return;
  started = true;
  await loadPluginDir(); // 답변 꾸미기 폴더 — 첫 명령을 치기 전에 알아야 한다
  await loadTestClaudePrefix(testClaudePrefix); // 시험 모드(진짜 claude 대신 가짜) 여부 — 첫 명령을 치기 전에 정해 둔다
  let list: TermInfo[] = [];
  try {
    list = await termList();
  } catch (e) {
    console.error("터미널 목록을 읽지 못함", e);
  }
  for (const info of list) TerminalHub.terms[info.id] = info;
  for (const info of list.filter((t) => t.status === "running")) {
    const tab = newTab(info.id, info.cwd);
    TerminalHub.tabs.push(tab);
  }
  if (TerminalHub.tabs.length) select(TerminalHub.tabs[TerminalHub.tabs.length - 1].id, false);
  // 브라우저 개발 화면에서만 — 탭 여러 개를 미리 연다(?tabs=N)
  if (!inTauri && TerminalHub.tabs.length === 0) {
    for (const t of mockTabs()) {
      const tab = openShell(t.dir, { sessionId: t.sessionId });
      if (tab && t.name) tab.customName = t.name;
    }
  }
  startLifecycle(); // 탭 저장·되살리기·끌 때 확인(P1.1)
  startSessionFlow(); // 상태 묶음마다 짝짓기(pair)를 다시 쥐고, 세션 열기 요청·이름 바꾸기를 받는다
}

// ── 고르기 ────────────────────────────────────────────────────

/** 탭을 고른다. 터미널 포커스는 펼쳐 있을 때만 */
export function select(id: string, focus = true) {
  const tab = TerminalHub.tabs.find((t) => t.id === id);
  if (!tab) return;
  TerminalHub.selected = id;
  TerminalHub.workspace = tab.workspace;
  TerminalHub.lastTab[workspaceKey(tab.workspace)] = id;
  if (focus && expanded()) focusSelected();
}

// 경로에서 보일 탭 — 그 경로에서 마지막에 고른 탭(닫혔으면 첫 탭). 탭이 없으면 undefined
function shownTab(ws: string): string | undefined {
  return shownTabOf(TerminalHub.tabs, ws, TerminalHub.lastTab, homeDir());
}

/** 경로 줄 항목을 눌렀을 때 — 그 경로의 마지막에 고른 탭으로 */
export function showWorkspace(path: string) {
  const id = shownTab(path);
  if (id) select(id);
}

/** 지금 경로 안에서 왼쪽(-1)·오른쪽(+1) 탭으로(끝에서는 반대쪽 끝으로) */
export function stepTab(step: number) {
  const list = tabsIn(TerminalHub.workspace);
  if (!list.length) return;
  select(list[wrapStep(list.length, list.findIndex((t) => t.id === TerminalHub.selected), step)].id);
}

/** 탭 줄 순서의 n 번째(1부터) 탭으로 */
export function selectNumber(n: number) {
  const tab = nthTab(tabsIn(TerminalHub.workspace), n);
  if (tab) select(tab.id);
}

/** 경로 줄에서 앞(-1)·뒤(+1) 경로로(끝에서는 반대쪽 끝으로) */
export function stepWorkspace(step: number) {
  const list = workspaces();
  if (!list.length) return;
  const key = workspaceKey(TerminalHub.workspace);
  const id = shownTab(list[wrapStep(list.length, list.findIndex((w) => workspaceKey(w) === key), step)]);
  if (id) select(id);
}

// ── 열기 ──────────────────────────────────────────────────────

/** 그 폴더(없으면 홈)의 셸 탭을 같은 경로 탭들의 끝에(새 경로면 맨 끝에) 연다. command 는 셸이 뜬 뒤 한 번 칠 명령 */
export function openShell(dir: string | null = null, opts: { command?: string; sessionId?: string; mdRenderOff?: boolean } = {}): BoardTab | undefined {
  const tab = newTab(null, dir ?? "~");
  const surface = focusedSurface(tab);
  if (surface) {
    surface.command = opts.command ?? null;
    surface.sessionId = opts.sessionId ?? null;
    surface.mdRenderOff = opts.mdRenderOff ?? false;
    // 명령을 넣을 화면은 막 claude 가 뜨는 것으로 본다 — 셸이 뜨기 전 스냅샷이 명령을 비우지 않게
    surface.typedAt = opts.command ? Date.now() : null;
  }
  TerminalHub.tabs.splice(insertIndexFor(TerminalHub.tabs, tab.workspace, homeDir()), 0, tab);
  select(tab.id);
  return TerminalHub.tabs.find((t) => t.id === tab.id);
}

/** 새 세션 창 — 터미널이 접혀 있으면 펼치고 연다. 이미 떠 있으면 겹쳐 띄우지 않는다(Ctrl+T 를 또 누름) */
export function newSessionSheet() {
  if (TerminalHub.sheet) return;
  if (!expanded()) setTerminalExpanded(true);
  TerminalHub.sheet = true;
}

/** 칸의 셸을 띄우거나(없으면 새로, 있으면 이어서) 붙는다. 칸이 처음 보일 때 부른다 */
export async function startLeaf(leafId: string) {
  const leaf = surfaceOf(leafId);
  const tab = tabOfLeaf(leafId);
  const state = TerminalHub.leaves[leafId];
  if (!leaf || !tab || !state || state.phase !== "idle") return;
  state.phase = "starting";
  state.error = "";
  const ctl = controllerOf(leafId);
  // 기다리는 사이 탭이 닫혔는가 — remove() 가 칸 상태를 지운다
  const closed = () => !TerminalHub.leaves[leafId];
  let spawnedId: TermId | null = null;
  try {
    await ctl.open();
    if (closed()) return ctl.dispose();
    let info = leaf.termId ? TerminalHub.terms[leaf.termId] : undefined;
    if (!info) {
      const dims = ctl.dims();
      if (!dims) throw new Error("터미널 크기를 잴 수 없음");
      const cwd = workspaceKey(tab.workspace) === "~" ? undefined : tab.workspace;
      info = await termSpawn({ cwd, cols: dims.cols, rows: dims.rows });
      spawnedId = info.id;
      if (closed()) {
        termKill(info.id); // 닫힌 탭의 셸이 남지 않게
        return ctl.dispose();
      }
      TerminalHub.terms[info.id] = info;
      leaf.termId = info.id;
    }
    await ctl.bind(info);
    if (closed()) {
      if (spawnedId) termKill(spawnedId);
      return ctl.dispose();
    }
    if (state.phase === "starting") state.phase = "running";
    // 새로 띄운 셸에만 명령을 한 번 친다(셸 준비를 기다린 뒤). 기다리는 동안 칸은 이미 running
    if (spawnedId && leaf.command) {
      const command = leaf.command;
      leaf.command = null;
      const termId = spawnedId;
      void typeCommand(ctl, termId, command, {
        alive: () => !!TerminalHub.leaves[leafId] && surfaceOf(leafId)?.termId === termId && !!TerminalHub.terms[termId],
        touch: () => {
          leaf.typedAt = Date.now();
        },
        shell: info.shell,
        mdRenderOff: leaf.mdRenderOff,
      });
    }
  } catch (e) {
    if (closed()) {
      if (spawnedId) termKill(spawnedId);
      return;
    }
    state.phase = "failed";
    state.error = String(e);
    console.error("터미널을 시작하지 못함", e);
  }
}

/** 시작에 실패한 칸에 새 셸을 다시 띄운다. 칸은 그대로 두고 터미널만 바꾼다 */
export function retry(leafId: string) {
  const leaf = surfaceOf(leafId);
  const state = TerminalHub.leaves[leafId];
  if (!leaf || !state || state.phase !== "failed") return;
  controllerOf(leafId).reset();
  leaf.termId = null;
  state.phase = "idle";
  void startLeaf(leafId).then(() => focusActive());
}

// ── 닫기 ──────────────────────────────────────────────────────

/** 확인 창을 세운다 — 떠 있던 확인 창이 있으면 그 취소를 불러 준다. 밀려난 창의 답을 기다리던 쪽(보드로 옮기기 등)이 영원히 기다리지 않게 */
export function showConfirm(next: Confirm) {
  const previous = TerminalHub.confirm;
  TerminalHub.confirm = next;
  previous?.cancel?.();
}

/** 탭의 ×. 그 탭의 짝 세션이 아직 끝나지 않았을 때만 묻고, 아니면 바로 닫는다 */
export function requestClose(tab: BoardTab) {
  if (!isBusy(tab)) return remove(tab);
  showConfirm({
    title: "탭을 닫을까요?",
    text: `${shownName(tab)} 탭에서 도는 프로그램이 함께 끝납니다.`,
    run: () => remove(tab),
  });
}

/** 탭을 닫고 셸을 끝낸다. 고른 탭이었으면 같은 경로의 이웃 탭 → 이웃 경로 → 빈 상태로 */
export function remove(tab: BoardTab) {
  const i = TerminalHub.tabs.findIndex((t) => t.id === tab.id);
  if (i < 0) return;
  const key = workspaceKey(tab.workspace);
  const next = pickAfterClose(TerminalHub.tabs, tab.id, TerminalHub.lastTab, homeDir());
  const gone = TerminalHub.tabs[i];
  const wasSelected = TerminalHub.selected === gone.id;
  TerminalHub.tabs.splice(i, 1);
  for (const id of leaves(gone.root)) {
    const termId = surfaceOf(id)?.termId;
    if (termId && TerminalHub.terms[termId]?.status !== "exited") termKill(termId);
    if (termId) delete TerminalHub.terms[termId];
    controllers.get(id)?.dispose();
    controllers.delete(id);
    delete TerminalHub.leaves[id];
    const at = TerminalHub.surfaces.findIndex((x) => x.id === id);
    if (at >= 0) TerminalHub.surfaces.splice(at, 1);
  }
  if (TerminalHub.lastTab[key] === gone.id) delete TerminalHub.lastTab[key];
  if (!wasSelected) return;
  if (next) select(next);
  else {
    TerminalHub.selected = "";
    TerminalHub.workspace = "";
  }
}

// 셸이 끝났다 — 그 탭을 닫는다(마지막 화면이므로 탭째). claude 가 끝난 것은 셸 프롬프트가 남아 여기 오지 않는다
function terminalDidClose(leafId: string, code: number | null) {
  const state = TerminalHub.leaves[leafId];
  const tab = tabOfLeaf(leafId);
  if (!state || !tab) return;
  state.phase = "exited";
  state.exitCode = code;
  remove(tab);
}

// ── 이름 바꾸기 ───────────────────────────────────────────────

/** 고른 탭의 이름 자리를 입력 칸으로 바꾼다 */
export function beginRename() {
  const tab = selectedTab();
  if (tab) tab.renaming = true;
}

/** 이름 바꾸기 끝 — name 이 null 이면 취소. 비우거나 폴더 이름과 같게 저장하면 사용자 이름을 지운다. 끝나면 포커스는 늘 터미널로 */
export function endRename(tab: BoardTab, name: string | null) {
  if (!tab.renaming) return;
  tab.renaming = false;
  if (name !== null) {
    tab.customName = renamedTo(tab.customName, folderName(tab.workspace), name);
    startTabRenames(tab);
  }
  focusSelected();
}

/** 경계를 끄는 동안 칸 크기를 맞추지 않고, 놓을 때 한 번 맞춘다 */
export function setDragging(dragging: boolean) {
  for (const ctl of controllers.values()) ctl.setDragging(dragging);
}
