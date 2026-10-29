// 백엔드(src-tauri)가 보내는 보드 상태 묶음 — windows/docs/SPEC.md §6.1 과 같게 유지한다. 시각은 모두 유닉스 밀리초
export type State = "working" | "waiting" | "idle" | "paused" | "ended";
export type Mood = "urgent" | "busy" | "idle" | "sleepy";
export type MascotKind =
  | "tralalero" | "bombardiro" | "tungtung" | "lirili" | "patapim" | "boneca" | "cappuccino" | "foreman";
export type BoardSide = "left" | "right";
export type NotifyPosition = "center" | "top-left" | "top-right" | "bottom-left" | "bottom-right";

export interface Brain {
  model: string;        // claude-opus-5-5 등
  effort: string;       // low | medium | high | xhigh | max, 모르면 ""
  family: string;       // Opus · Sonnet · Haiku · Fable
  version: string;      // 5.5
  effortLevel: number;  // 0 = 모름, 1(low) ~ 5(max)
}

export interface Ctx {
  used: number;
  size: number;
  pct: number;
}

export interface Sub {
  id: string;
  type: string;
  desc: string;
  state: "running" | "waiting" | "done";
  started: number;
  ended: number | null;
  tool: string;
  background: boolean;
  brain: Brain | null;
}

export interface Session {
  id: string;
  cwd: string;
  pid: number;          // claude 프로세스 id — 보드 밖 터미널의 세션이 끝나기를 기다릴 때(waitExit) 쓴다
  pidCtime?: number | null; // 그 프로세스의 생성 시각(유닉스 초) — pid 가 다른 프로세스에 재사용됐는지 가린다. 모르면 null
  state: State;
  since: number;
  updated: number;
  headless: boolean;
  title: string;        // topic, 없으면 message, 둘 다 없으면 "요청 전"
  topic: string;
  message: string;
  name: string | null;  // Claude Code 세션 이름(~/.claude/sessions/<pid>.json). 없거나 종료된 세션이면 null
  ctx: Ctx | null;
  brain: Brain | null;
  subagents: Sub[];
  shells: number;
  paused: boolean;
  muted: boolean;
  limitAt: number | null;
  canDrive: boolean;    // 콘솔에 키를 넣을 수 있는 세션(일시중지 가능)
  resumeArgs?: string[]; // 이어 열 때 claude 뒤에 붙일 인자(끝은 --resume <id>) — 탭을 되살릴 때 원래 플래그를 살린다
  // ── 세션 짝짓기(SPEC §5.12, W2) — 백엔드가 보내는 값. 자리(tab·terminal·background·ended)를 가르는 조형은
  // terminal/place.ts 가 쥔다: 이 네 값은 순수 짝짓기 module(pairSessions·placeOf)의 입력일 뿐이다 ──
  tty: TermId | null;         // 이 세션의 콘솔이 보드의 어느 터미널(TermId)인지(mac Session.tty 대응), null = 보드 터미널이 아니다
  tabAncestor: TermId | null; // headless 세션이 어느 보드 터미널의 자손인지(mac tabAncestor), null = 탭 자손이 아니다
  mode: "-p" | "--bg" | null; // claude 를 띄운 방식 — -p · --bg, 대화형이면 null
  appName: string | null;     // 보드 밖 다른 터미널 앱 이름(Windows Terminal 등), 짝이 밝혀지기 전에는 null
}

export interface Group {
  id: string;           // project_dir(claude 를 띄운 폴더), 없으면 cwd
  name: string;
  path: string;         // 홈 폴더를 ~ 로
  sessions: Session[];  // 급한 순
}

export interface Limit {
  id: string;           // 5h | 7d | 7d-<모델 이름>
  label: string;        // 5시간 · 주간 · 전체 · 주간 · Fable
  short: string;        // 5시간 · 주간 · Fable
  tiny: string;         // 5h · 7d · F
  pct: number;
  resets: number | null;
}

export interface Account {
  id: string;
  email: string;
  org: string;
  plan: string;         // Max 5x 등
  expired: boolean;     // 다시 로그인 필요
}

export interface Notice {
  text: string;
  error: boolean;
}

export interface BoardSettings {
  collapsed: string[];
  endedOpen: string[];
  pinnedGroups: string[];
  kindCollapsed: string[];   // "<그룹 id>|term" · "<그룹 id>|bg"
  subCollapsed: string[];    // 서브에이전트 목록을 접은 세션 id
  usageCompact: boolean;
  pinned: boolean;           // 항상 위
  mascot: MascotKind;
  opacity: number;           // 0.3 ~ 1
  autoSwitch: boolean;
  boardMini: boolean;
  notifyPosition: NotifyPosition;
  terminalExpanded: boolean; // 내장 터미널을 펼쳤는지(보드 옆에 터미널 칸이 붙는다)
  boardSide: BoardSide;      // 접었을 때 보드가 붙는 화면 쪽, 펼쳤을 때 터미널 반대쪽
  boardPaneWidth: number;    // 보드 칸 폭(논리 px)
  mdRenderOff: string[];     // 답변 꾸미기(md-render)를 끄고 띄운 세션 id
  shortcuts: ShortcutOverrides;        // 사용자가 바꾼 단축키(기본값이면 항목 없음)
  settingsTransition: SettingsTransition; // 설정 화면이 열리고 닫히는 움직임
  newSessionCommand: string;           // 새 세션 기본 명령, 비면 claude
  newSessionMdRender: boolean;         // 새 세션의 답변 꾸미기 기본값
  autoUpdateCheck: boolean;            // 시작할 때와 하루마다 새 버전 확인
}

// ── 설정 화면·단축키·계정 로그인(2단계) — 규칙은 settings/·shortcuts/ ────────────────

// 설정 화면의 묶음(mac SettingsSection) — 백엔드 이벤트 board://settings 의 section 값이기도 하다
export type SettingsSectionId = "general" | "window" | "notify" | "terminal" | "board" | "account" | "shortcuts" | "update";

// 설정 화면이 열리고 닫히는 움직임(mac SettingsTransition)
export type SettingsTransition = "slide" | "fade" | "none";

// 저장한 단축키 하나 — [] 는 비움, [KeyboardEvent.code, 수정키 비트] 는 그 키(비트는 shortcuts/keyCombo.ts 가 정한다). 기본값이면 항목이 없다
export type ShortcutOverride = [] | [string, number];
export type ShortcutOverrides = Record<string, ShortcutOverride>;

// 이벤트 board://settings — 백엔드(트레이 "설정…")가 설정 화면을 열거나 닫으라고 한다. section 이 없으면 지금 묶음
export interface SettingsEventPayload {
  open: boolean;
  section?: SettingsSectionId;
}

// 이벤트 board://account-login — 계정 추가 로그인의 진행. url 은 열 주소, note 는 안내 한 줄, done 은 끝남, error 는 사유
export interface AccountLoginPayload {
  url?: string;
  note?: string;
  done?: boolean;
  error?: string;
}

export interface TerminalApp {
  id: string;
  name: string;
  installed: boolean;
  running: boolean;
}

export interface Snapshot {
  now: number;
  atPrompt?: string[];  // 셸이 프롬프트에 있는(자식 프로세스가 없는) 보드 터미널(TermId) — 끝난 세션을 그 탭에서 이어 열 때 쓴다 (W3)
  runsClaude?: TermId[];  // 지금 claude 가 도는 보드 터미널(TermId) — 서피스의 세션이 끝났는지 가르는 데 쓴다 (W2)
  groups: Group[];
  allMuted: boolean;
  counts: Record<State, number>;
  usage: { limits: Limit[]; failed: boolean; tokenExpired: boolean };
  accounts: {
    current: Account | null;
    saved: Account[];
    adding: boolean;
    switching: boolean;
    autoSwitch: boolean;
    notice: Notice | null;
  };
  updater: {
    current: string;
    dev: boolean;
    latest: string | null;
    running: boolean;
    failed: boolean;
    checking: boolean;
    notice: { kind: "ok" | "warn" | "dev"; text: string } | null;
  };
  settings: BoardSettings;
  claudeVersion: string | null;
  terminals: TerminalApp[];
}

// 세션 줄·카드에서 부르는 동작 — 백엔드 명령 session_action 의 action 값. mac performAction 의 어휘(pause·resume·stop·reopen·focus)와 Windows 전용 activate
export type SessionAction = "activate" | "focus" | "reopen" | "pause" | "resume" | "stop";

// 알림 패널(/notify) 인자 — 백엔드 명령 get_notify_options 가 준다
export interface NotifyOptions {
  title: string;
  subtitle: string;
  body: string;
  kind: "done" | "wait";
  summary: { waiting?: number; working?: number; idle?: number };
  duration: number;     // 초
  position: NotifyPosition;
}

// ── 내장 터미널 — 계약은 windows/docs/SPEC.md 터미널 절(백엔드 terms.rs)과 같게 유지한다 ──────────────

// 백엔드가 프로세스 수명 동안 유일하게 내는 터미널 번호("t1", "t2", …). 화면(BoardSurface)은 따로 id 를 갖는다
export type TermId = string;

export interface TermSpec {
  cwd?: string;
  cols: number;
  rows: number;
}

export interface TermInfo {
  id: TermId;
  pid: number;
  shell: "pwsh" | "powershell" | "cmd";
  cwd: string;
  cols: number;
  rows: number;
  status: "running" | "exited";
  exitCode: number | null;
  windowsBuild: number | null;
  oldestSeq: number;   // 링에 남은 가장 오래된 바이트의 오프셋
  nextSeq: number;     // 다음에 올 바이트의 오프셋
}

// 채널로 오는 프레임 한 개를 푼 것: 출력 조각(seq = 첫 바이트의 오프셋) · 링이 덮여 처음부터 다시 받음 · 셸 종료(마지막 출력 뒤)
export type TermFrame =
  | { kind: "data"; seq: number; bytes: Uint8Array }
  | { kind: "gap"; seq: number }
  | { kind: "exit"; code: number | null };

// 화면 나누기 트리(mac SplitNode). 잎은 화면(BoardSurface) 하나의 id, 가지는 둘로 나눈 것:
// right 면 왼쪽|오른쪽, down 이면 위/아래. ratio 는 앞쪽(first = 왼쪽·위)의 몫
export type SplitDirection = "right" | "down";
export type SplitNode =
  | { leaf: string }
  | { split: SplitDirection; ratio: number; first: SplitNode; second: SplitNode };

// 화면 하나(mac BoardSurface) — 터미널 하나와 그 셸에 붙은 세션 정보. id 는 TerminalHub.leaves·컨트롤러의 열쇠다
export interface BoardSurface {
  id: string;
  termId: TermId | null;          // 이 화면에 붙은 셸(백엔드 터미널 번호), 아직 안 띄웠으면 null
  sessionId: string | null;       // 이 화면의 claude 세션(보드 상태 묶음의 Session.id), 셸 화면은 null. 끝난 뒤에도 남는다
  command: string | null;         // 셸이 뜬 뒤 한 번 칠 claude 명령, 없으면 null(셸 화면)
  held: Record<string, string> | null; // 다시 뜰 때 셸만 띄우고 들어갈 때 이어 열 값(resumeHeld)
  mdRenderOff: boolean;           // 답변 꾸미기를 끄고 띄운 화면
  title: string;                  // 터미널이 알려 준 제목(xterm onTitleChange)
  typedAt?: number | null;        // 셸에 claude 명령을 친 시각(ms) — 3초 동안은 claude 가 도는 것으로 본다(mac typedAt, W3)
}

// 보드 터미널 탭 하나(mac BoardTab) — 화면 나누기 트리 하나와 그 가운데 고른 화면. 지금은 탭마다 화면 하나뿐이다
export interface BoardTab {
  id: string;
  workspace: string;          // 경로 줄 항목 — 이 탭을 연 작업 경로("~" = 홈)
  customName: string | null;  // 사용자가 바꾼 탭 이름, 없으면 폴더 이름
  renaming: boolean;          // 이름 바꾸기 입력 칸이 열려 있는지
  root: SplitNode;
  focused: string;            // 고른 화면의 id
  zoomed: boolean;            // 고른 화면 하나 크게 보기
}

// 칸의 셸 상태 — idle(아직 안 띄움) · starting · running · exited · failed
export type LeafPhase = "idle" | "starting" | "running" | "exited" | "failed";

// ── 세션 열기·구동(W3) ───────────────────────────────────────

// 백엔드가 보드로 보내는 "세션 열기" 요청(board://open-session) — 끝난 세션 이어 열기(reopen)·창으로 이동(focus)
export interface OpenSessionPayload {
  id: string;
  action: "reopen" | "focus";
  carryOn: boolean;
  projectDir: string;
  resumeArgs: string[];
}

// drive_session 이 보드 터미널이나 콘솔에 넣을 입력 한 단계(mac driveSession)
export type DriveStep = { key: "escape" | "enter" } | { text: string } | { wait: number };

export interface RenameCheck {
  safe: boolean;
  transcript: boolean;
}

// ── 탭 저장·끌 때 확인(P1.1) — 규칙은 terminal/restore.ts · terminal/shutdownList.ts ─────────

// 저장하는 화면 하나(mac `savedSurface`)
export interface SavedSurface {
  cwd: string;
  command: string | null;
  sessionId: string | null;
  mdRenderOff: boolean;
}

// 저장하는 화면 나누기 트리(mac `savedNode`) — 잎은 화면 하나, 가지는 둘로 나눈 것
export type SavedNode =
  | { leaf: SavedSurface }
  | { split: SplitDirection; ratio: number; first: SavedNode; second: SavedNode };

// 저장하는 탭 하나(mac `entry`) — 백엔드 설정 terminalTabs 의 항목이다
export interface TabEntry {
  workspace: string;
  name: string | null;
  selected: boolean;
  root: SavedNode;
}

// 생사를 물을 프로세스 — pid 와 그 생성 시각(세션 기록의 pid_ctime). 생성 시각이 있으면 재사용된 pid 를 산 것으로 보지 않는다
export interface PidTarget {
  pid: number;
  ctime: number | null;
}

// cancel 은 떠 있는 확인 창을 닫으라는 뜻이다(백엔드가 답을 기다리다 포기했다) — 그때 items 는 비어 있다
export type ShutdownReason = "quit" | "update" | "cancel";

// 끌 때 확인 창에 보이는 줄 하나. shell 줄은 세션이 없어 sessionId 가 null
export interface ShutdownItem {
  sessionId: string | null;
  title: string;
  kind: "working" | "waiting" | "subagents" | "shell" | "headlessInTab";
}

// 이벤트 board://confirm-shutdown — 목록(items)은 프론트가 shutdownList() 로 만든다
export interface ConfirmShutdownPayload {
  reason: ShutdownReason;
  items: ShutdownItem[];
}
