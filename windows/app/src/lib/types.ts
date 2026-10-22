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
  // 세션이 어디서 도는지(위치 칩) — 프런트엔드 전용 선택 필드. 백엔드는 아직 보내지 않고 이후 단계(세션·탭 짝짓기)가 채운다.
  // 없으면 칩을 그리지 않는다. macOS PlaceChip(ClaudeBoard.swift) 대응:
  //   tab = 보드 탭(CB), terminal = 다른 터미널(appName), background = 터미널 없이(mode: -p·--bg), ended = 끝난 세션
  place?: SessionPlace;
}

export interface SessionPlace {
  kind: "tab" | "terminal" | "background" | "ended";
  appName?: string;     // kind=terminal 일 때 앱 이름
  mode?: string;        // -p · --bg — 대화형이면 없음
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
}

export interface TerminalApp {
  id: string;
  name: string;
  installed: boolean;
  running: boolean;
}

export interface Snapshot {
  now: number;
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

// 세션 줄·카드에서 부르는 동작 — 백엔드 명령 session_action 의 action 값
export type SessionAction = "activate" | "focus" | "resume" | "pause" | "resume_paused" | "stop";

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

// 백엔드가 프로세스 수명 동안 유일하게 내는 터미널 번호("t1", "t2", …). 칸(leaf)은 따로 leafId 를 갖는다
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

// 터미널 칸 배치. 1단계는 잎 하나만 그리지만 나누기를 넣을 수 있는 트리 모양이다
export type PaneNode =
  | { type: "leaf"; leafId: string; termId: TermId | null }
  | { type: "split"; dir: "row" | "col"; ratio: number; first: PaneNode; second: PaneNode };

export interface TerminalTab {
  id: string;
  root: PaneNode;
  activeLeaf: string;
  workspace: string;          // 경로 줄 항목 — 이 탭을 연 작업 경로("~" = 홈)
  customName: string | null;  // 사용자가 바꾼 탭 이름, 없으면 폴더 이름
  renaming: boolean;          // 이름 바꾸기 입력 칸이 열려 있는지
  sessionId: string | null;   // 이 탭의 claude 세션(보드 상태 묶음의 Session.id), 셸 탭은 null
  command: string | null;     // 셸이 뜬 뒤 한 번 칠 명령(claude …), 없으면 null
  title: string;              // 터미널이 알려 준 제목(xterm onTitleChange)
}

// 칸의 셸 상태 — idle(아직 안 띄움) · starting · running · exited · failed
export type LeafPhase = "idle" | "starting" | "running" | "exited" | "failed";
