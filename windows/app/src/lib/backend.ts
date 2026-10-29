// 백엔드(src-tauri)와 주고받는 곳 — windows/docs/SPEC.md §6.
// Tauri 안에서는 invoke·이벤트를 쓰고, 브라우저(npm run dev)에서는 mock.ts 의 가짜 상태로 화면을 끝까지 눌러 볼 수 있게 한다.
// 명령 인자 이름은 Rust 명령의 매개변수 이름과 같아야 한다(Tauri 가 camelCase ↔ snake_case 로 맞춘다)
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AccountLoginPayload, BoardSide, ConfirmShutdownPayload, DriveStep, MascotKind, NotifyOptions, NotifyPosition, OpenSessionPayload, PidTarget, RenameCheck, SessionAction,
  SettingsEventPayload, SettingsTransition, ShortcutOverride, Snapshot, State, TabEntry, TermFrame, TermId, TermInfo, TermSpec,
} from "./types";

export const inTauri = isTauri();

type Patch = (s: Snapshot) => void;

let current: Snapshot | null = null;
const listeners = new Set<(s: Snapshot) => void>();
// 누른 즉시 화면에 보이도록 먼저 고친 내용 — 백엔드의 다음 상태 묶음이 아직 반영하지 못했을 수 있어 잠깐 다시 덮는다.
// 그래서 패치는 "뒤집기" 가 아니라 "이 값으로" 처럼 몇 번 덮어도 같은 결과여야 한다
let pending: { until: number; patch: Patch }[] = [];
let started = false;

function emit(s: Snapshot) {
  current = s;
  for (const l of listeners) l(s);
}

function publish(s: Snapshot) {
  const now = Date.now();
  pending = pending.filter((p) => p.until > now);
  for (const p of pending) p.patch(s);
  emit(s);
}

async function start() {
  if (started) return;
  started = true;
  if (inTauri) {
    await listen<Snapshot>("board://snapshot", (e) => publish(e.payload));
    publish(await invoke<Snapshot>("get_snapshot"));
  } else {
    const mock = await import("./mock");
    mock.startMock(publish);
  }
}

// 상태 묶음을 받는다 — 처음 부를 때 백엔드(또는 가짜 상태)를 연결한다
export function onSnapshot(fn: (s: Snapshot) => void): () => void {
  listeners.add(fn);
  if (current) fn(current);
  void start();
  return () => listeners.delete(fn);
}

function optimistic(patch: Patch) {
  pending.push({ until: Date.now() + 1500, patch });
  if (!current) return;
  const s = structuredClone(current);
  patch(s);
  emit(s);
}

async function call<T = void>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    if (inTauri) return await invoke<T>(cmd, args);
    const mock = await import("./mock");
    return (await mock.handle(cmd, args ?? {})) as T;
  } catch (e) {
    console.error(`${cmd} 실패`, e);
    throw e;
  }
}

// 실패해도 화면은 다음 상태 묶음으로 바로잡히므로 누른 쪽에서는 기다리지 않는다
function fire(cmd: string, args?: Record<string, unknown>) {
  call(cmd, args).catch(() => {});
}

function setIn(list: string[], item: string, on: boolean) {
  const i = list.indexOf(item);
  if (on && i < 0) list.push(item);
  if (!on && i >= 0) list.splice(i, 1);
}

// 세션을 뺀 뒤 상태별 개수를 다시 센다
function recount(s: Snapshot) {
  const c: Record<State, number> = { waiting: 0, working: 0, idle: 0, paused: 0, ended: 0 };
  for (const g of s.groups) for (const x of g.sessions) c[x.state]++;
  s.counts = c;
}

const has = (list: string[] | undefined, item: string) => !!list?.includes(item);

// ── 보드 설정·표지 ─────────────────────────────────────────────

export function toggleCollapsed(cwd: string) {
  const on = !has(current?.settings.collapsed, cwd);
  optimistic((s) => setIn(s.settings.collapsed, cwd, on));
  fire("toggle_collapsed", { cwd });
}

export function setAllCollapsed(collapse: boolean) {
  optimistic((s) => {
    s.settings.collapsed = collapse ? s.groups.map((g) => g.id) : [];
  });
  fire("set_all_collapsed", { collapse });
}

export function toggleEndedOpen(cwd: string) {
  const on = !has(current?.settings.endedOpen, cwd);
  optimistic((s) => setIn(s.settings.endedOpen, cwd, on));
  fire("toggle_ended_open", { cwd });
}

export function togglePinGroup(cwd: string) {
  const on = !has(current?.settings.pinnedGroups, cwd);
  optimistic((s) => setIn(s.settings.pinnedGroups, cwd, on));
  fire("toggle_pin_group", { cwd });
}

export function hideGroup(cwd: string) {
  optimistic((s) => {
    s.groups = s.groups.filter((g) => g.id !== cwd);
    setIn(s.settings.pinnedGroups, cwd, false);
    recount(s);
  });
  fire("hide_group", { cwd });
}

export function removeEnded(cwd: string) {
  optimistic((s) => {
    for (const g of s.groups) if (g.id === cwd) g.sessions = g.sessions.filter((x) => x.state !== "ended");
    s.groups = s.groups.filter((g) => g.sessions.length > 0);
    recount(s);
  });
  fire("remove_ended", { cwd });
}

export function removeSession(id: string) {
  optimistic((s) => {
    for (const g of s.groups) g.sessions = g.sessions.filter((x) => x.id !== id);
    s.groups = s.groups.filter((g) => g.sessions.length > 0);
    recount(s);
  });
  fire("remove_session", { id });
}

export function toggleKindCollapsed(key: string) {
  const on = !has(current?.settings.kindCollapsed, key);
  optimistic((s) => setIn(s.settings.kindCollapsed, key, on));
  fire("toggle_kind_collapsed", { key });
}

export function toggleSubCollapsed(id: string) {
  const on = !has(current?.settings.subCollapsed, id);
  optimistic((s) => setIn(s.settings.subCollapsed, id, on));
  fire("toggle_sub_collapsed", { id });
}

export function setUsageCompact(on: boolean) {
  optimistic((s) => {
    s.settings.usageCompact = on;
  });
  fire("set_usage_compact", { on });
}

export function setMute(id: string, muted: boolean) {
  optimistic((s) => {
    for (const g of s.groups) for (const x of g.sessions) if (x.id === id) x.muted = muted;
  });
  fire("set_mute", { id, muted });
}

export function setAllMuted(on: boolean) {
  optimistic((s) => {
    s.allMuted = on;
  });
  fire("set_all_muted", { on });
}

// 요약 칩으로 찾아갈 때 — 카드와 그 안의 터미널·백그라운드 구역을 편다
export function reveal(cwd: string) {
  optimistic((s) => {
    setIn(s.settings.collapsed, cwd, false);
    s.settings.kindCollapsed = s.settings.kindCollapsed.filter((k) => !k.startsWith(cwd + "|"));
  });
  fire("reveal", { cwd });
}

export function setPinned(on: boolean) {
  optimistic((s) => {
    s.settings.pinned = on;
  });
  fire("set_pinned", { on });
}

export function setMascot(kind: MascotKind) {
  optimistic((s) => {
    s.settings.mascot = kind;
  });
  fire("set_mascot", { kind });
}

export function setOpacity(v: number) {
  optimistic((s) => {
    s.settings.opacity = v;
  });
  fire("set_opacity", { v });
}

export function setNotifyPosition(pos: NotifyPosition) {
  optimistic((s) => {
    s.settings.notifyPosition = pos;
  });
  fire("set_notify_position", { pos });
}

// 창 크기 단계가 미니인지 — 미니로 둔 채 끄면 다음에도 그 크기로 연다(§5.9 boardMini).
// SPEC §6.2 목록에 아직 없는 명령이라 백엔드에 없으면 조용히 넘어간다
export function setBoardMini(on: boolean) {
  call("set_board_mini", { on }).catch(() => {});
}

// ── 세션·계정·업데이트 ─────────────────────────────────────────

// activate = 그 세션의 터미널 앱만 앞으로, focus = 그 세션 창(탭)으로 이동, reopen = 꺼진 세션 이어 열기(carryOn 이면 "이어서 진행" 문구 포함),
// pause·resume·stop = 일시중지·일시중지한 세션 재개·중단(중단 확인 대화상자는 백엔드가 띄운다)
export function sessionAction(action: SessionAction, id: string, carryOn?: boolean) {
  fire("session_action", { action, id, carryOn });
}

// 보드가 안 떠 있던 동안 쌓인 "세션 열기" 요청을 모두 꺼낸다(오래된 것부터) — 처음 부르면 프런트엔드가 준비됐다는 표시도 된다.
// onOpenSession 의 듣기가 끝난 뒤에 불러야 그 사이에 오는 요청을 놓치지 않는다 (W3)
export function takeOpenSession(): Promise<OpenSessionPayload[]> {
  return call<OpenSessionPayload[]>("take_open_session");
}

// 이어 열기를 마쳤다고 알린다 — 백엔드가 그제서야 일시중지 표시를 지운다
export function reopenDone(id: string): Promise<void> {
  return call<void>("reopen_done", { id });
}

// 백엔드가 보내는 "세션 열기" 요청(끝난 세션 이어 열기·창으로 이동). 듣기가 시작되면 풀린다
export async function onOpenSession(fn: (p: OpenSessionPayload) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen<OpenSessionPayload>("board://open-session", (e) => fn(e.payload));
}

// 보드 탭이나 콘솔에 단계별 입력을 넣는다(일시중지 Esc, 재개 문구, /rename). 넣을 곳이 없으면 false
export function driveSession(id: string, steps: DriveStep[]): Promise<boolean> {
  return call<boolean>("drive_session", { id, steps });
}

// /rename 을 보내도 되는 세션인가 — 신선한 세션 상태와 대화 기록 유무 (mac renameSafe·hasTranscript)
export function renameCheck(id: string): Promise<RenameCheck> {
  return call<RenameCheck>("rename_check", { id });
}

// 시스템 더블 클릭 시간(ms) — 다른 터미널 줄의 한 번 클릭을 이만큼 미룬다. 읽지 못하면(브라우저 개발 화면 포함) 500
export async function doubleClickTime(): Promise<number> {
  try {
    const ms = await call<number>("double_click_time");
    return typeof ms === "number" && ms > 0 ? ms : 500;
  } catch {
    return 500;
  }
}

// 보드 창을 펼쳐 앞으로 가져온다 (mac showBoard)
export function showBoard() {
  fire("show_board");
}

export function switchAccount(id: string) {
  fire("switch_account", { id });
}

export function cancelAddAccount() {
  fire("cancel_add_account");
}

export function setAutoSwitch(on: boolean) {
  optimistic((s) => {
    s.accounts.autoSwitch = on;
    s.settings.autoSwitch = on;
  });
  fire("set_auto_switch", { on });
}

export function checkUpdate(manual: boolean) {
  if (manual) {
    optimistic((s) => {
      s.updater.checking = true;
    });
  }
  fire("check_update", { manual });
}

export function runUpdate() {
  fire("run_update");
}

export function openChangelog() {
  fire("open_changelog");
}

// ── 창·터미널 ──────────────────────────────────────────────────

export function openTerminal(id: string) {
  fire("open_terminal", { id });
}

export function enterMini(rows: number) {
  fire("enter_mini", { rows });
}

export function leaveMini() {
  fire("leave_mini");
}

export function snapRight() {
  fire("snap_right");
}

export function hideBoard() {
  fire("hide_board");
}

export function minimizeBoard() {
  fire("minimize_board");
}

export function log(text: string) {
  fire("log", { text });
}

// ── 터미널 칸·보드 칸 배치 ─────────────────────────────────────

// 터미널을 펼치거나 접는다. 창 크기·위치는 백엔드가 바꾸고, 화면은 누른 즉시 따라간다
export function setTerminalExpanded(on: boolean) {
  optimistic((s) => {
    s.settings.terminalExpanded = on;
  });
  fire("set_terminal_expanded", { on });
}

/** setTerminalExpanded 와 같되 백엔드가 저장을 마칠 때까지 기다린다 — 끄기 직전처럼 곧 프로세스가 끝나는 자리용. 실패하면 던진다 */
export async function setTerminalExpandedAndWait(on: boolean): Promise<void> {
  optimistic((s) => {
    s.settings.terminalExpanded = on;
  });
  await call("set_terminal_expanded", { on });
}

export function setBoardSide(side: BoardSide) {
  optimistic((s) => {
    s.settings.boardSide = side;
  });
  fire("set_board_side", { side });
}

// 경계를 놓을 때 한 번 저장한다(논리 px)
export function setBoardWidth(width: number) {
  optimistic((s) => {
    s.settings.boardPaneWidth = width;
  });
  fire("set_board_width", { width });
}

// 백엔드가 Ctrl+Alt+T 등으로 터미널을 앞에 불렀을 때 — 활성 터미널에 포커스를 준다
export function onTerminalFocus(fn: () => void): () => void {
  if (!inTauri) return () => {};
  let off: (() => void) | undefined;
  let closed = false;
  listen("board://terminal-focus", fn).then((unlisten) => (closed ? unlisten() : (off = unlisten)));
  return () => {
    closed = true;
    off?.();
  };
}

// ── 내장 터미널 명령 — 계약은 SPEC 터미널 절 ─────────────────────

export function termSpawn(spec: TermSpec): Promise<TermInfo> {
  return call<TermInfo>("term_spawn", { spec });
}

export function termList(): Promise<TermInfo[]> {
  return call<TermInfo[]>("term_list");
}

export function termAck(id: TermId, seq: number) {
  fire("term_ack", { id, seq });
}

/** 적용했으면 ConPTY 크기 변경 직전의 스트림 위치, 적용 안 했으면(같은 크기·끝난 셸) null */
export function termResize(id: TermId, cols: number, rows: number): Promise<number | null> {
  return call<number | null>("term_resize", { id, cols, rows });
}

export function termKill(id: TermId) {
  fire("term_kill", { id });
}

// 채널 메시지 하나를 푼다. 원시 데이터는 8바이트 little-endian seq + 출력, JSON 제어는 {t:"gap"|"exit"}.
// 작은 원시 메시지는 숫자 배열로, 큰 것은 ArrayBuffer 로 올 수 있어 둘 다 받는다
export function decodeFrame(message: unknown): TermFrame {
  let bytes: Uint8Array | null = null;
  if (message instanceof ArrayBuffer) bytes = new Uint8Array(message);
  else if (message instanceof Uint8Array) bytes = message;
  else if (Array.isArray(message)) bytes = Uint8Array.from(message as number[]);
  if (bytes) {
    if (bytes.length < 8) throw new Error(`터미널 프레임이 8바이트보다 짧음: ${bytes.length}`);
    const seq = Number(new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true));
    return { kind: "data", seq, bytes: bytes.subarray(8) };
  }
  const control = message as { t?: string; seq?: number; code?: number | null } | null;
  if (control?.t === "gap") return { kind: "gap", seq: Number(control.seq) };
  if (control?.t === "exit") return { kind: "exit", code: control.code ?? null };
  throw new Error(`알 수 없는 터미널 프레임: ${JSON.stringify(message)}`);
}

// fromSeq 부터 다시 받기 시작하고, 이후 프레임을 onFrame 으로 보낸다. 같은 id 의 이전 채널은 백엔드가 대체한다
export async function termAttach(id: TermId, fromSeq: number, onFrame: (frame: TermFrame) => void): Promise<TermInfo> {
  const onMessage = (message: unknown) => onFrame(decodeFrame(message));
  if (inTauri) {
    const channel = new Channel<unknown>();
    channel.onmessage = onMessage;
    return invoke<TermInfo>("term_attach", { id, fromSeq, onData: channel });
  }
  const mock = await import("./mock");
  return mock.attachTerm(id, fromSeq, onMessage);
}

// 입력은 터미널마다 줄을 세워 앞 명령이 끝난 뒤에 보낸다 — 비동기 명령은 도착 순서가 보장되지 않아 빠르게 치면 글자가 뒤섞인다.
// 기다리는 동안 쌓인 같은 종류의 입력은 한 번에 보낸다
type Input = { text: string } | { bin: number[] };
const inputs = new Map<TermId, { queue: Input[]; busy: boolean }>();

function enqueueInput(id: TermId, item: Input) {
  let state = inputs.get(id);
  if (!state) inputs.set(id, (state = { queue: [], busy: false }));
  const last = state.queue.at(-1);
  if (last && "text" in last && "text" in item) last.text += item.text;
  else if (last && "bin" in last && "bin" in item) last.bin.push(...item.bin);
  else state.queue.push(item);
  if (!state.busy) void drainInput(id, state);
}

async function drainInput(id: TermId, state: { queue: Input[]; busy: boolean }) {
  state.busy = true;
  try {
    for (let item = state.queue.shift(); item; item = state.queue.shift()) {
      await ("text" in item ? call("term_write", { id, data: item.text }) : call("term_write_bin", { id, data: item.bin })).catch(() => {});
    }
  } finally {
    state.busy = false;
  }
}

export function termWrite(id: TermId, data: string) {
  enqueueInput(id, { text: data });
}

export function termWriteBin(id: TermId, data: number[]) {
  enqueueInput(id, { bin: data });
}

// ── 수명 주기·md-render·시험 장치(1단계) — 규칙은 terminal/{restore,shutdownList,mdRender,testClaude}.ts ──

// 다시 뜰 때 이어 열 탭 목록을 통째로 바꿔 저장한다 / 읽는다 (mac saveTabs·restoreTabs)
// 저장이 끝나면 풀린다 — 끄기 직전에는 이것을 기다린 뒤 답해야 마지막 저장이 사라지지 않는다
export function saveTabs(entries: TabEntry[]): Promise<void> {
  return call<void>("save_tabs", { entries });
}

export function loadTabs(): Promise<TabEntry[]> {
  return call<TabEntry[]>("load_tabs");
}

// 주어진 프로세스 가운데 아직 산 것의 pid — 앞 보드가 남긴 터미널이 끝났는지 볼 때. 생성 시각(ctime)이 다르면 재사용된 pid 라 산 것으로 치지 않는다
export function pidsAlive(targets: PidTarget[]): Promise<number[]> {
  return call<number[]>("pids_alive", { targets });
}

// 그 프로세스가 timeoutMs 안에 끝나기를 기다린다 — 끝났으면 true (보드 밖 터미널의 세션을 /exit 로 끝낸 뒤). pid 0(모름)은 늘 false
export function waitExit(pid: number, pidCtime: number | null, timeoutMs: number): Promise<boolean> {
  return call<boolean>("wait_exit", { pid, pidCtime, timeoutMs });
}

// 끌 때 확인 창의 답 — pause 는 .paused 를 남길 세션 id. 백엔드가 받은 뒤 풀린다
export function answerShutdown(ok: boolean, pause: string[]): Promise<void> {
  return call<void>("answer_shutdown", { ok, pause });
}

// 화면이 (다시) 시작했다 — 그 전에 보낸 끌 때 확인 요청은 화면이 사라졌으므로 백엔드가 기다리던 요청을 취소한다
export function lifecycleStarted(): Promise<void> {
  return call<void>("lifecycle_started");
}

// 주어진 경로 가운데 실제로 있는 폴더 — 탭을 되살릴 때 작업 폴더가 아직 있는지(없는 폴더에서는 셸이 홈 폴더로 뜬다)
export function existingDirs(paths: string[]): Promise<string[]> {
  return call<string[]>("existing_dirs", { paths });
}

// 답변 꾸미기를 끈 세션 id 가운데 세션 파일이 없어진 것을 치운다 — 보드를 켤 때 한 번(mac pruneMdRenderOff)
export function pruneMdRenderOff(): Promise<void> {
  return call<void>("prune_md_render_off");
}

// 끌 때 확인 창을 띄웠다고 알린다 — 이 뒤로 백엔드는 사용자가 고를 때까지 기다린다(안 알리면 10초 뒤 그대로 진행한다)
export function shutdownDialogShown() {
  fire("shutdown_dialog_shown");
}

// 백엔드가 끌 때 확인을 요청했을 때(트레이 종료·업데이트) — 듣기가 시작된 뒤에 풀리고, 듣기를 끄는 함수를 돌려준다
export async function onConfirmShutdown(fn: (p: ConfirmShutdownPayload) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen<ConfirmShutdownPayload>("board://confirm-shutdown", (e) => fn(e.payload));
}

// 옮겨 둔 md-render 플러그인 폴더, 못 옮겼으면 null(그러면 --plugin-dir 을 붙이지 않는다)
export function mdRenderDir(): Promise<string | null> {
  return call<string | null>("md_render_dir");
}

// 답변 꾸미기를 끄고 띄운 세션 id 를 기억한다
export function rememberMdRenderOff(id: string) {
  fire("remember_md_render_off", { id });
}

// 시험 모드에서 맨 앞 claude 를 바꿔칠 프로그램·앞쪽 인자, 시험 모드가 아니면 null
export function testClaudePrefix(): Promise<string[] | null> {
  return call<string[] | null>("test_claude_prefix");
}

// ── 설정 화면·단축키·계정 로그인(2단계) — 규칙은 settings/·shortcuts/ ──────

// 단축키 하나를 바꾼다 — override 는 저장 모양 그대로: null 은 기본값으로(항목 삭제), [] 는 비움, [코드, 수정키] 는 그 키
export function setShortcut(id: string, override: ShortcutOverride | null) {
  optimistic((s) => {
    if (override) s.settings.shortcuts[id] = override;
    else delete s.settings.shortcuts[id];
  });
  fire("set_shortcut", { id, combo: override });
}

export function resetShortcuts() {
  optimistic((s) => {
    s.settings.shortcuts = {};
  });
  fire("reset_shortcuts");
}

// 전역 단축키(Ctrl+Alt+B/T)를 지금 표대로 다시 건다 — paused 면 모두 풀어 둔다(설정 화면이 새 키를 받는 동안).
// 돌려주는 것은 등록하지 못한 동작 id. 표를 바꾸는 set_shortcut·reset_shortcuts 도 다시 걸고 board://hotkeys 로 같은 목록을 알린다
export async function syncHotkeys(paused: boolean): Promise<string[]> {
  return (await call<string[] | undefined>("sync_hotkeys", { paused })) ?? [];
}

export function onHotkeysFailed(fn: (ids: string[]) => void): () => void {
  return listenEvent<string[]>("board://hotkeys", fn);
}

// 로그인할 때 열기가 켜져 있는가(표지 파일이 없으면 켠 것)
export function loginLaunch(): Promise<boolean> {
  return call<boolean>("login_launch");
}

/** 로그인할 때 열기를 바꾼다 — 끝날 때까지 기다리고 실패하면 던진다(바꾼 뒤 실제 값은 loginLaunch 로 다시 읽는다) */
export function setLoginLaunch(on: boolean): Promise<void> {
  return call("set_login_launch", { on });
}

export function setSettingsTransition(value: SettingsTransition) {
  optimistic((s) => {
    s.settings.settingsTransition = value;
  });
  fire("set_settings_transition", { value });
}

// 새 세션 기본 명령(비면 claude)과 답변 꾸미기 기본값
export function setNewSessionDefaults(command: string, mdRender: boolean) {
  optimistic((s) => {
    s.settings.newSessionCommand = command.trim();
    s.settings.newSessionMdRender = mdRender;
  });
  fire("set_new_session_defaults", { command, mdRender });
}

export function setAutoUpdateCheck(on: boolean) {
  optimistic((s) => {
    s.settings.autoUpdateCheck = on;
  });
  fire("set_auto_update_check", { on });
}

// 설정 화면 안 계정 추가 — 로그인 진행은 board://account-login 으로 온다. 취소는 cancelAddAccount
export function beginAddAccount(email?: string) {
  fire("begin_add_account", { email: email ?? null });
}

export function submitLoginCode(code: string) {
  fire("submit_login_code", { code });
}

// 지우기 전에 백엔드가 확인 창을 띄운다(취소하면 아무 일도 없다)
export function deleteAccount(id: string) {
  fire("delete_account", { id });
}

// 지금 로그인 주소를 기본 브라우저로 연다 — 주소는 백엔드가 로그인 명령의 출력에서 읽은 것만 연다
export function openLoginUrl() {
  fire("open_login_url");
}

// 백엔드가 설정 화면을 열거나 닫으라고 할 때(트레이 "설정…"). 듣기를 끄는 함수를 돌려준다
export function onSettingsEvent(fn: (p: SettingsEventPayload) => void): () => void {
  return listenEvent<SettingsEventPayload>("board://settings", fn);
}

// 계정 추가 로그인의 진행(주소·안내·끝남·사유). 듣기를 끄는 함수를 돌려준다
export function onAccountLogin(fn: (p: AccountLoginPayload) => void): () => void {
  return listenEvent<AccountLoginPayload>("board://account-login", fn);
}

function listenEvent<T>(name: string, fn: (p: T) => void): () => void {
  if (!inTauri) return () => {};
  let off: (() => void) | undefined;
  let closed = false;
  listen<T>(name, (e) => fn(e.payload)).then((unlisten) => (closed ? unlisten() : (off = unlisten)));
  return () => {
    closed = true;
    off?.();
  };
}

// ── 알림 패널(/notify) — §6.3 ──────────────────────────────────

// 브라우저에서는 주소의 쿼리(title·subtitle·body·kind·summary·duration·position)로 받는다
export async function getNotifyOptions(): Promise<NotifyOptions> {
  if (inTauri) return invoke<NotifyOptions>("get_notify_options");
  const q = new URLSearchParams(location.search);
  const kind = q.get("kind") === "wait" ? "wait" : "done";
  const summary: NotifyOptions["summary"] = {};
  for (const pair of (q.get("summary") ?? "").split(",")) {
    const [k, v] = pair.split("=");
    const n = Number(v);
    if ((k === "waiting" || k === "working" || k === "idle") && Number.isFinite(n)) summary[k] = n;
  }
  return {
    title: q.get("title") ?? (kind === "wait" ? "승인 대기" : "작업 완료"),
    subtitle: q.get("subtitle") ?? "",
    body: q.get("body") ?? "",
    kind,
    summary,
    duration: Number(q.get("duration")) || 10,
    position: (q.get("position") as NotifyPosition) || "center",
  };
}

// 백엔드가 이 높이로 자리를 잡고 창을 보인 뒤에 돌아온다 — 그다음에 나타나는 애니메이션을 시작한다
export async function notifyReady(height: number) {
  if (inTauri) await invoke("notify_ready", { height }).catch((e) => console.error(e));
}

export function notifyHover(inside: boolean) {
  if (inTauri) invoke("notify_hover", { inside }).catch(() => {});
}

export function notifyClicked() {
  if (inTauri) invoke("notify_clicked").catch(() => {});
}

export function notifyFinish() {
  if (inTauri) invoke("notify_finish").catch(() => {});
}
