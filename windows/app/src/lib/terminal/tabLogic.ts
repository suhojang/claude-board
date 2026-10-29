// 탭 허브의 순수 규칙 — 경로 키·이름·경로 줄·닫은 뒤 고르기·이름 바꾸기·번호 배지.
// 허브(hub.svelte.ts)는 svelte 와 Tauri 를 import 해 node:test 로 못 읽으므로, 계산만 여기로 뺐다. 이 파일은 외부를 import 하지 않는다.
// 홈 폴더는 보드 상태 묶음에서 알아내므로 인자(home)로 받는다 — 허브가 homeDir() 을 넘긴다.

/** 탭에서 이 규칙이 보는 것 — 허브의 BoardTab 이 그대로 맞는다 */
export interface TabLike {
  id: string;
  workspace: string;
}

/** 경로를 견주는 꼴의 앞단 — 슬래시 방향·끝 \ ·대소문자를 맞춘다 */
export function normPath(path: string): string {
  return path.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
}

/** 경로를 견주는 꼴 — 대소문자·슬래시 방향·끝 \ 차이로 같은 폴더가 어긋나지 않게, 홈은 "~" */
export function workspaceKeyOf(path: string, home: string | null): string {
  const n = normPath(path);
  return n === "~" || (home !== null && n === normPath(home)) ? "~" : n;
}

/** 상태 묶음의 그룹(id = 전체 경로, path = 홈을 ~ 로 줄인 경로)에서 홈 폴더를 알아낸다 */
export function homeFromGroups(groups: ReadonlyArray<{ id: string; path: string }>): string | null {
  for (const g of groups) {
    if (g.path === "~") return g.id;
    if (g.path.startsWith("~") && g.id.endsWith(g.path.slice(1))) return g.id.slice(0, g.id.length - g.path.length + 1);
  }
  return null;
}

export function baseName(path: string): string {
  const parts = path.replace(/\//g, "\\").replace(/\\+$/, "").split("\\");
  return parts.at(-1) || path;
}

/** 폴더 이름(홈은 ~) */
export function folderNameOf(workspace: string, home: string | null): string {
  return workspaceKeyOf(workspace, home) === "~" ? "~" : baseName(workspace);
}

/** 경로의 마디들 — 슬래시·역슬래시 둘 다 구분자이고, 끝 구분자와 빈 마디(UNC 앞머리 포함)는 버린다 */
function segmentsOf(path: string): string[] {
  return path.split(/[\\/]+/).filter((part) => part !== "");
}

/** 뒤에서 depth 마디를 `/` 로 이은 이름. 마디가 모자라면 앞을 빈 마디로 채운다 */
function trailingLabel(segments: string[], depth: number): string {
  const tail = segments.slice(-depth);
  return [...Array<string>(depth - tail.length).fill(""), ...tail].join("/");
}

/**
 * 경로 줄 이름 — 폴더 이름(홈은 ~). 같은 이름이 둘 이상이면 부모 폴더를 붙이고,
 * 그래도 같은 이름이 남으면 겹치는 이름이 없어질 때까지 조부모 … 로 한 마디씩 더 붙인다.
 * 같은 경로(대소문자·구분자 차이만)는 한 항목이라 같은 이름을 받는다 — 구분은 탭 번호가 맡는다
 */
export function workspaceLabelOf(ws: string, among: string[], home: string | null): string {
  const name = folderNameOf(ws, home);
  if (name === "~") return name;
  const mine = normPath(ws);
  const rivals = new Map<string, string[]>();
  for (const other of among) {
    const key = normPath(other);
    if (key !== mine && folderNameOf(other, home) === name && !rivals.has(key)) rivals.set(key, segmentsOf(other));
  }
  const own = segmentsOf(ws);
  const sameNameElsewhere = [...rivals.values()];
  if (!sameNameElsewhere.length) return name;
  const deepest = Math.max(own.length, ...sameNameElsewhere.map((segments) => segments.length));
  let depth = 2;
  // 마디를 다 쓰고도 같으면(경로가 마디 기준으로 같음) 더 붙일 것이 없다
  while (depth < deepest && sameNameElsewhere.some((rival) => trailingLabel(rival, depth).toLowerCase() === trailingLabel(own, depth).toLowerCase())) depth++;
  return trailingLabel(own, depth);
}

/** 경로 줄 항목의 힌트 — 전체 경로(홈은 ~) */
export function workspaceHintOf(ws: string, home: string | null): string {
  if (workspaceKeyOf(ws, home) === "~") return "~";
  if (home && normPath(ws).startsWith(normPath(home) + "\\")) return "~" + ws.slice(home.length);
  return ws;
}

/** 경로 줄의 경로들 — 탭 목록에 처음 나타난 순서 */
export function workspacesOf(tabs: ReadonlyArray<TabLike>, home: string | null): string[] {
  const keys: string[] = [];
  const list: string[] = [];
  for (const t of tabs) {
    const k = workspaceKeyOf(t.workspace, home);
    if (!keys.includes(k)) {
      keys.push(k);
      list.push(t.workspace);
    }
  }
  return list;
}

export function tabsInWorkspace<T extends TabLike>(tabs: ReadonlyArray<T>, ws: string, home: string | null): T[] {
  const key = workspaceKeyOf(ws, home);
  return tabs.filter((t) => workspaceKeyOf(t.workspace, home) === key);
}

/** 경로에서 보일 탭 — 그 경로에서 마지막에 고른 탭(닫혔으면 첫 탭). 탭이 없으면 undefined */
export function shownTabOf(tabs: ReadonlyArray<TabLike>, ws: string, lastTab: Readonly<Record<string, string>>, home: string | null): string | undefined {
  const last = lastTab[workspaceKeyOf(ws, home)];
  if (last && tabs.some((t) => t.id === last)) return last;
  return tabsInWorkspace(tabs, ws, home)[0]?.id;
}

/** 새 탭이 들어갈 자리 — 같은 경로 탭들의 끝, 새 경로면 맨 끝 */
export function insertIndexFor(tabs: ReadonlyArray<TabLike>, workspace: string, home: string | null): number {
  const key = workspaceKeyOf(workspace, home);
  for (let i = tabs.length - 1; i >= 0; i--) {
    if (workspaceKeyOf(tabs[i].workspace, home) === key) return i + 1;
  }
  return tabs.length;
}

/** 목록 안에서 step 만큼 옮긴 자리(끝에서는 반대쪽 끝으로). 지금 자리가 없으면(-1) 0 에서 센다. 빈 목록은 -1 */
export function wrapStep(length: number, index: number, step: number): number {
  if (length <= 0) return -1;
  const i = Math.max(0, index);
  return (((i + step) % length) + length) % length;
}

/** 탭 줄 순서의 n 번째(1부터). 없으면 undefined — n 이 탭 수보다 크거나 1 보다 작을 때 */
export function nthTab<T>(list: ReadonlyArray<T>, n: number): T | undefined {
  if (!Number.isInteger(n) || n < 1) return undefined;
  return list[n - 1];
}

/** 탭 줄 i 번째(0부터)의 번호 배지 — 수정키를 누르는 동안만, 앞의 아홉 개(Ctrl+Alt+1..9)에만 */
export function badgeNumber(held: boolean, index: number): number | null {
  return held && index < 9 ? index + 1 : null;
}

/** 탭을 닫은 뒤 고를 탭 id — tabs 는 닫기 전 목록. 같은 경로의 이웃 탭(닫힌 자리, 끝이면 마지막) → 이웃 경로의 보일 탭 → 없으면 undefined */
export function pickAfterClose(
  tabs: ReadonlyArray<TabLike>,
  closingId: string,
  lastTab: Readonly<Record<string, string>>,
  home: string | null,
): string | undefined {
  const gone = tabs.find((t) => t.id === closingId);
  if (!gone) return undefined;
  const key = workspaceKeyOf(gone.workspace, home);
  const at = tabsInWorkspace(tabs, gone.workspace, home).findIndex((t) => t.id === closingId);
  const order = workspacesOf(tabs, home);
  const remaining = tabs.filter((t) => t.id !== closingId);
  const rest = tabsInWorkspace(remaining, gone.workspace, home);
  if (rest.length) return rest[Math.min(at, rest.length - 1)].id;
  const others = order.filter((w) => workspaceKeyOf(w, home) !== key);
  if (!others.length) return undefined;
  const slot = order.findIndex((w) => workspaceKeyOf(w, home) === key);
  return shownTabOf(remaining, others[Math.min(slot, others.length - 1)], lastTab, home);
}

/** 이름 바꾸기 결과 — 새 customName. 보이던 이름(shown)과 같게 저장하면 그대로, 비우거나 폴더 이름과 같게 저장하면 null(사용자 이름 지움) */
export function renamedTo(current: string | null, folder: string, input: string): string | null {
  const n = input.trim();
  const shown = current ?? folder;
  if (n === shown) return current;
  return n === "" || n === folder ? null : n;
}

/** 탭 닫기 확인이 필요한가 — 짝 세션이 있고 아직 끝나지 않았을 때만(셸만 있는 탭은 바로 닫는다) */
export function needsCloseConfirm(sessionState: string | null | undefined): boolean {
  return !!sessionState && sessionState !== "ended";
}

/** 경로 상태 점의 순위 — 낮을수록 급하다. 점은 일시중지를 대기보다 앞에 친다(카드 순위 board/format.ts 의 rank 와 다르다) */
export const DOT_RANK: Readonly<Record<string, number>> = { waiting: 0, working: 1, paused: 2, idle: 3, ended: 4 };

/** 상태들 가운데 가장 급한 것(경로 상태 점 순위). 비면 idle */
export function mostUrgent<S extends string>(states: ReadonlyArray<S>): S | "idle" {
  return states.length ? states.reduce((a, b) => (DOT_RANK[b] < DOT_RANK[a] ? b : a)) : "idle";
}
