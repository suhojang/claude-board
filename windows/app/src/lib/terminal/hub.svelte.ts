// 터미널 저장소(TerminalHub) — 화면이 보는 터미널 목록·탭·경로·고른 탭과 칸별 컨트롤러.
// macOS 보드의 TerminalHub(ClaudeBoard.swift:5388-5455, 5847, 6008-6115)와 같은 규칙이다: 탭은 작업 경로(workspace)로 묶여 경로 줄에 서고,
// 탭 줄은 지금 경로의 탭만 보인다. 셸이 끝나면 그 탭이 닫히고 선택은 같은 경로의 이웃 탭 → 이웃 경로 → 빈 상태로 간다.
// 터미널 출력은 보드의 1초 상태 묶음에 싣지 않고, 여기서 term_list 와 채널로 채운다(이벤트 기반).
import { tick } from "svelte";
import { inTauri, setTerminalExpanded, termKill, termList, termSpawn, termWrite } from "$lib/backend";
import { board } from "$lib/board/store.svelte";
import { mockTabs } from "$lib/mock";
import type { LeafPhase, PaneNode, Session, State, TermId, TermInfo, TerminalTab } from "$lib/types";
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
import { TermController } from "./term";

export interface LeafState {
  phase: LeafPhase;
  exitCode: number | null;
  error: string;
}

/** 탭을 닫기 전에 묻는 창 — TerminalArea 가 그린다 */
export interface Confirm {
  title: string;
  text: string;
  run: () => void;
}

export const hub = $state({
  terms: {} as Record<TermId, TermInfo>,
  tabs: [] as TerminalTab[],
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
});

// 컨트롤러는 xterm 을 쥔 일반 객체라 반응형 상태에 넣지 않는다
const controllers = new Map<string, TermController>();
let started = false;

export function controllerOf(leafId: string): TermController {
  let ctl = controllers.get(leafId);
  if (!ctl) {
    ctl = new TermController(leafId, {
      onExit: (code) => markExited(leafId, code),
      onTitle: (title) => {
        const tab = tabOfLeaf(leafId);
        // 셸이 처음에 보내는 제목은 실행 파일 경로(C:\Program Files\...\pwsh.exe)라 보이지 않게 한다
        if (tab) tab.title = /\.exe"?$/i.test(title.trim()) ? "" : title;
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
  return workspacesOf(hub.tabs, homeDir());
}

export function tabsIn(ws: string): TerminalTab[] {
  return tabsInWorkspace(hub.tabs, ws, homeDir());
}

/** 탭 줄에 보이는 이름 — 사용자가 바꾼 이름, 없으면 폴더 이름 */
export function shownName(tab: TerminalTab) {
  return tab.customName ?? folderName(tab.workspace);
}

// ── 탭 상태 ───────────────────────────────────────────────────

function sessionOf(tab: TerminalTab): Session | null {
  if (!tab.sessionId) return null;
  for (const g of board.snap?.groups ?? []) {
    const s = g.sessions.find((x) => x.id === tab.sessionId);
    if (s) return s;
  }
  return null;
}

/** 탭 상태 — 그 탭 화면들의 세션 가운데 가장 급한 것(세션이 없으면 idle). 지금은 탭마다 세션 하나다 */
export function tabState(tab: TerminalTab): State {
  return sessionOf(tab)?.state ?? "idle";
}

/** 경로 상태 — 그 경로 탭들 가운데 가장 급한 상태 */
export function workspaceState(ws: string): State {
  return mostUrgent(tabsIn(ws).map(tabState));
}

/** 탭이 지금 하는 일 — 승인 대기이고 message 가 있으면 message, 아니면 세션 제목. 셸 탭은 터미널 제목 */
export function doing(tab: TerminalTab): string {
  const s = sessionOf(tab);
  if (!s) return tab.title;
  return s.state === "waiting" && s.message ? s.message : s.title;
}

/** 탭에서 도는 프로그램이 있는가 — 짝 세션이 아직 끝나지 않았을 때(셸만 있는 탭은 바로 닫는다) */
function isBusy(tab: TerminalTab): boolean {
  return needsCloseConfirm(sessionOf(tab)?.state);
}

// ── 탭 만들기·찾기 ────────────────────────────────────────────

function newTab(termId: TermId | null, workspace: string): TerminalTab {
  const leafId = crypto.randomUUID();
  hub.leaves[leafId] = { phase: "idle", exitCode: null, error: "" };
  const root: PaneNode = { type: "leaf", leafId, termId };
  return {
    id: crypto.randomUUID(),
    root,
    activeLeaf: leafId,
    workspace,
    customName: null,
    renaming: false,
    sessionId: null,
    command: null,
    title: "",
  };
}

export function findLeaf(node: PaneNode, leafId: string): (PaneNode & { type: "leaf" }) | null {
  if (node.type === "leaf") return node.leafId === leafId ? node : null;
  return findLeaf(node.first, leafId) ?? findLeaf(node.second, leafId);
}

function leavesOf(node: PaneNode): Array<PaneNode & { type: "leaf" }> {
  return node.type === "leaf" ? [node] : [...leavesOf(node.first), ...leavesOf(node.second)];
}

function tabOfLeaf(leafId: string): TerminalTab | undefined {
  return hub.tabs.find((t) => findLeaf(t.root, leafId));
}

function leafOf(leafId: string) {
  const tab = tabOfLeaf(leafId);
  return tab ? findLeaf(tab.root, leafId) : null;
}

export function activeTab(): TerminalTab | undefined {
  return hub.tabs.find((t) => t.id === hub.selected);
}

export function focusActive() {
  const tab = activeTab();
  if (tab && !tab.renaming) controllers.get(tab.activeLeaf)?.focus();
}

/** 고른 탭에서 조합 중인 글자가 있으면 확정한다 */
export function commitComposition() {
  const tab = activeTab();
  if (tab && !tab.renaming) controllers.get(tab.activeLeaf)?.commitComposition();
}

// 고른 탭이 그려진(보이게 된) 뒤에 터미널로 포커스를 돌려준다
function focusSelected() {
  void tick().then(() => requestAnimationFrame(focusActive));
}

const expanded = () => board.snap?.settings.terminalExpanded ?? false;

// ── 시작 ──────────────────────────────────────────────────────

/** 페이지를 새로 고친 뒤 아직 도는 터미널을 탭으로 이어받는다. 빈 탭은 만들지 않는다 — 탭이 생길 때까지 빈 상태. 여러 번 불러도 한 번만 한다 */
export async function startHub() {
  if (started) return;
  started = true;
  let list: TermInfo[] = [];
  try {
    list = await termList();
  } catch (e) {
    console.error("터미널 목록을 읽지 못함", e);
  }
  for (const info of list) hub.terms[info.id] = info;
  for (const info of list.filter((t) => t.status === "running")) {
    const tab = newTab(info.id, info.cwd);
    hub.tabs.push(tab);
  }
  if (hub.tabs.length) select(hub.tabs[hub.tabs.length - 1].id, false);
  // 브라우저 개발 화면에서만 — 탭 여러 개를 미리 연다(?tabs=N)
  if (!inTauri && hub.tabs.length === 0) {
    for (const t of mockTabs()) {
      const tab = openShell(t.dir, { sessionId: t.sessionId });
      if (tab && t.name) tab.customName = t.name;
    }
  }
}

// ── 고르기 ────────────────────────────────────────────────────

/** 탭을 고른다. 터미널 포커스는 펼쳐 있을 때만 */
export function select(id: string, focus = true) {
  const tab = hub.tabs.find((t) => t.id === id);
  if (!tab) return;
  hub.selected = id;
  hub.workspace = tab.workspace;
  hub.lastTab[workspaceKey(tab.workspace)] = id;
  if (focus && expanded()) focusSelected();
}

// 경로에서 보일 탭 — 그 경로에서 마지막에 고른 탭(닫혔으면 첫 탭). 탭이 없으면 undefined
function shownTab(ws: string): string | undefined {
  return shownTabOf(hub.tabs, ws, hub.lastTab, homeDir());
}

/** 경로 줄 항목을 눌렀을 때 — 그 경로의 마지막에 고른 탭으로 */
export function showWorkspace(path: string) {
  const id = shownTab(path);
  if (id) select(id);
}

/** 지금 경로 안에서 왼쪽(-1)·오른쪽(+1) 탭으로(끝에서는 반대쪽 끝으로) */
export function stepTab(step: number) {
  const list = tabsIn(hub.workspace);
  if (!list.length) return;
  select(list[wrapStep(list.length, list.findIndex((t) => t.id === hub.selected), step)].id);
}

/** 탭 줄 순서의 n 번째(1부터) 탭으로 */
export function selectNumber(n: number) {
  const tab = nthTab(tabsIn(hub.workspace), n);
  if (tab) select(tab.id);
}

/** 경로 줄에서 앞(-1)·뒤(+1) 경로로(끝에서는 반대쪽 끝으로) */
export function stepWorkspace(step: number) {
  const list = workspaces();
  if (!list.length) return;
  const key = workspaceKey(hub.workspace);
  const id = shownTab(list[wrapStep(list.length, list.findIndex((w) => workspaceKey(w) === key), step)]);
  if (id) select(id);
}

// ── 열기 ──────────────────────────────────────────────────────

/** 그 폴더(없으면 홈)의 셸 탭을 같은 경로 탭들의 끝에(새 경로면 맨 끝에) 연다. command 는 셸이 뜬 뒤 한 번 칠 명령 */
export function openShell(dir: string | null = null, opts: { command?: string; sessionId?: string } = {}): TerminalTab | undefined {
  const tab = newTab(null, dir ?? "~");
  tab.command = opts.command ?? null;
  tab.sessionId = opts.sessionId ?? null;
  hub.tabs.splice(insertIndexFor(hub.tabs, tab.workspace, homeDir()), 0, tab);
  select(tab.id);
  return hub.tabs.find((t) => t.id === tab.id);
}

/** 새 세션 창 — 터미널이 접혀 있으면 펼치고 연다. 이미 떠 있으면 겹쳐 띄우지 않는다(Ctrl+T 를 또 누름) */
export function newSessionSheet() {
  if (hub.sheet) return;
  if (!expanded()) setTerminalExpanded(true);
  hub.sheet = true;
}

/** 셸이 첫 출력을 낸 뒤 300ms 조용해지면(5초 상한) 명령을 쳐 넣고 100ms 뒤 Enter(\r) — 셸의 줄 편집이 시작된 뒤에 넣는다(ClaudeBoard.swift:5088-5096) */
const QUIET_MS = 300;
const QUIET_CAP_MS = 5000;
const ENTER_DELAY_MS = 100;
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

async function typeCommand(ctl: TermController, leafId: string, termId: TermId, command: string) {
  const startedAt = Date.now();
  // 첫 출력(lastOutputAt > 0)이 있고 그 뒤 300ms 조용하면 준비된 것. 5초가 지나면 그냥 친다
  const quiet = () => ctl.lastOutputAt > 0 && Date.now() - ctl.lastOutputAt >= QUIET_MS;
  while (!quiet() && Date.now() - startedAt < QUIET_CAP_MS) await sleep(50);
  // 기다리는 사이 탭이 닫혔거나 같은 칸에 다른 셸이 붙었으면 치지 않는다
  const alive = () => hub.leaves[leafId] && leafOf(leafId)?.termId === termId && hub.terms[termId];
  if (!alive()) return;
  termWrite(termId, command);
  await sleep(ENTER_DELAY_MS);
  if (alive()) termWrite(termId, "\r");
}

/** 칸의 셸을 띄우거나(없으면 새로, 있으면 이어서) 붙는다. 칸이 처음 보일 때 부른다 */
export async function startLeaf(leafId: string) {
  const leaf = leafOf(leafId);
  const tab = tabOfLeaf(leafId);
  const state = hub.leaves[leafId];
  if (!leaf || !tab || !state || state.phase !== "idle") return;
  state.phase = "starting";
  state.error = "";
  const ctl = controllerOf(leafId);
  // 기다리는 사이 탭이 닫혔는가 — remove() 가 칸 상태를 지운다
  const closed = () => !hub.leaves[leafId];
  let spawnedId: TermId | null = null;
  try {
    await ctl.open();
    if (closed()) return ctl.dispose();
    let info = leaf.termId ? hub.terms[leaf.termId] : undefined;
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
      hub.terms[info.id] = info;
      leaf.termId = info.id;
    }
    await ctl.bind(info);
    if (closed()) {
      if (spawnedId) termKill(spawnedId);
      return ctl.dispose();
    }
    if (state.phase === "starting") state.phase = "running";
    // 새로 띄운 셸에만 명령을 한 번 친다(셸 준비를 기다린 뒤). 기다리는 동안 칸은 이미 running
    if (spawnedId && tab.command) {
      const command = tab.command;
      tab.command = null;
      void typeCommand(ctl, leafId, spawnedId, command);
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
  const leaf = leafOf(leafId);
  const state = hub.leaves[leafId];
  if (!leaf || !state || state.phase !== "failed") return;
  controllerOf(leafId).reset();
  leaf.termId = null;
  state.phase = "idle";
  void startLeaf(leafId).then(() => focusActive());
}

// ── 닫기 ──────────────────────────────────────────────────────

/** 탭의 ×. 그 탭의 짝 세션이 아직 끝나지 않았을 때만 묻고, 아니면 바로 닫는다 */
export function requestClose(tab: TerminalTab) {
  if (!isBusy(tab)) return remove(tab);
  hub.confirm = {
    title: "탭을 닫을까요?",
    text: `${shownName(tab)} 탭에서 도는 프로그램이 함께 끝납니다.`,
    run: () => remove(tab),
  };
}

/** 탭을 닫고 셸을 끝낸다. 고른 탭이었으면 같은 경로의 이웃 탭 → 이웃 경로 → 빈 상태로 */
export function remove(tab: TerminalTab) {
  const i = hub.tabs.findIndex((t) => t.id === tab.id);
  if (i < 0) return;
  const key = workspaceKey(tab.workspace);
  const next = pickAfterClose(hub.tabs, tab.id, hub.lastTab, homeDir());
  const gone = hub.tabs[i];
  const wasSelected = hub.selected === gone.id;
  hub.tabs.splice(i, 1);
  for (const leaf of leavesOf(gone.root)) {
    const termId = leaf.termId;
    if (termId && hub.terms[termId]?.status !== "exited") termKill(termId);
    if (termId) delete hub.terms[termId];
    controllers.get(leaf.leafId)?.dispose();
    controllers.delete(leaf.leafId);
    delete hub.leaves[leaf.leafId];
  }
  if (hub.lastTab[key] === gone.id) delete hub.lastTab[key];
  if (!wasSelected) return;
  if (next) select(next);
  else {
    hub.selected = "";
    hub.workspace = "";
  }
}

// 셸이 끝났다 — 그 탭을 닫는다(마지막 화면이므로 탭째). claude 가 끝난 것은 셸 프롬프트가 남아 여기 오지 않는다
function markExited(leafId: string, code: number | null) {
  const state = hub.leaves[leafId];
  const tab = tabOfLeaf(leafId);
  if (!state || !tab) return;
  state.phase = "exited";
  state.exitCode = code;
  remove(tab);
}

// ── 이름 바꾸기 ───────────────────────────────────────────────

/** 고른 탭의 이름 자리를 입력 칸으로 바꾼다 */
export function beginRename() {
  const tab = activeTab();
  if (tab) tab.renaming = true;
}

/** 이름 바꾸기 끝 — name 이 null 이면 취소. 비우거나 폴더 이름과 같게 저장하면 사용자 이름을 지운다. 끝나면 포커스는 늘 터미널로 */
export function endRename(tab: TerminalTab, name: string | null) {
  if (!tab.renaming) return;
  tab.renaming = false;
  if (name !== null) {
    tab.customName = renamedTo(tab.customName, folderName(tab.workspace), name);
  }
  focusSelected();
}

/** 경계를 끄는 동안 칸 크기를 맞추지 않고, 놓을 때 한 번 맞춘다 */
export function setDragging(dragging: boolean) {
  for (const ctl of controllers.values()) ctl.setDragging(dragging);
}
