// 백엔드(src-tauri)가 보내는 보드 상태 묶음 — windows/docs/SPEC.md §6.1 과 같게 유지한다. 시각은 모두 유닉스 밀리초
export type State = "working" | "waiting" | "idle" | "paused" | "ended";
export type Mood = "urgent" | "busy" | "idle" | "sleepy";
export type MascotKind =
  | "tralalero" | "bombardiro" | "tungtung" | "lirili" | "patapim" | "boneca" | "cappuccino" | "foreman";
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
