// 브라우저(npm run dev)용 가짜 백엔드 — docs/images/hero.png 와 같은 장면을 만들고, SPEC §6.2 명령마다 이 상태를 고쳐
// 보드를 Tauri 없이도 끝까지 눌러 볼 수 있게 한다. 주소에 ?update 를 붙이면 새 버전 알림 줄, ?empty 면 세션 없는 화면
// 화면 대조용 시나리오(?scenario=mac1) — 파일은 저장소에 올리지 않는다(없으면 건너뜀)
type Scenarios = { applyMac1?: (s: typeof S) => void; FROZEN_NOW?: number };
const scenarios: Scenarios = Object.values(import.meta.glob("./mock-scenarios.ts", { eager: true }))[0] as Scenarios ?? {};
import type { Account, Brain, Group, Session, Snapshot, State, Sub, TermId, TermInfo, TermSpec } from "./types";

const SEC = 1000;
const MIN = 60 * SEC;
const HOUR = 60 * MIN;
const HOME = "C:\\Users\\demo";

const q = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
const hasUpdate = q.has("update");
const empty = q.has("empty");
const scenario = q.get("scenario");

const t0 = Date.now();

function brain(model: string, family: string, version: string, effort: string): Brain {
  const levels: Record<string, number> = { low: 1, medium: 2, high: 3, xhigh: 4, max: 5 };
  return { model, family, version, effort, effortLevel: levels[effort] ?? 0 };
}
const opus = (e = "xhigh") => brain("claude-opus-5-5", "Opus", "5.5", e);
const sonnet = (e = "high") => brain("claude-sonnet-5-5", "Sonnet", "5.5", e);
const haiku = (e = "medium") => brain("claude-haiku-4-5", "Haiku", "4.5", e);

function ctx(used: number, size = 1_000_000) {
  return { used, size, pct: Math.min(100, Math.floor((used * 100) / size)) };
}

function session(cwd: string, p: Partial<Session> & { id: string; state: State }): Session {
  const topic = p.topic ?? "";
  const message = p.message ?? "";
  return {
    cwd,
    pid: 0, // 브라우저 개발 화면에는 프로세스가 없다
    since: t0,
    updated: t0,
    headless: false,
    title: topic || message || "요청 전",
    topic,
    message,
    name: null,
    ctx: null,
    brain: null,
    subagents: [],
    shells: 0,
    paused: false,
    muted: false,
    limitAt: null,
    canDrive: !p.headless,
    // 세션 짝짓기 입력(SPEC §5.12, W2) — 브라우저 개발 화면은 파일 pid·콘솔이 없어 전부 null 로 둔다.
    // 탭 화면의 sessionId(mockTabs)로 이어진 세션이 place 의 held 길로 탭 소속(CB 칩)이 된다
    tty: null,
    tabAncestor: null,
    mode: null,
    appName: null,
    ...p,
  };
}

function sub(p: Partial<Sub> & { id: string; type: string }): Sub {
  return { desc: "", state: "running", started: t0, ended: null, tool: "", background: false, brain: null, ...p };
}

function group(name: string, sessions: Session[]): Group {
  return { id: `${HOME}\\work\\${name}`, name, path: `~\\work\\${name}`, sessions };
}

const cwdOf = (name: string) => `${HOME}\\work\\${name}`;

function initialGroups(): Group[] {
  if (empty) return [];
  return [
    group("shop-api", [
      session(cwdOf("shop-api"), {
        id: "s-shop-1",
        state: "waiting",
        since: t0 - 47 * SEC,
        updated: t0 - 5 * SEC,
        topic: "주문 취소 시 재고를 복구하는 로직 추가해줘",
        message: "Claude needs your permission to use Bash",
        name: "shop-api-3c",
        brain: opus(),
        ctx: ctx(312_000),
      }),
      session(cwdOf("shop-api"), {
        id: "s-shop-2",
        state: "idle",
        since: t0 - (3 * MIN + 7 * SEC),
        updated: t0 - (3 * MIN + 7 * SEC),
        topic: "결제 웹훅 서명 검증 테스트 작성",
        brain: sonnet(),
        ctx: ctx(148_000),
      }),
    ]),
    group("web-dashboard", [
      session(cwdOf("web-dashboard"), {
        id: "s-web-1",
        state: "working",
        since: t0 - (4 * MIN + 27 * SEC),
        updated: t0 - 2 * SEC,
        topic: "매출 차트를 주간·월간으로 바꿀 수 있게 해줘",
        name: "frontend-dashboard-redesign-v2",
        brain: opus(),
        ctx: ctx(455_000),
        subagents: [
          sub({ id: "a-1", type: "Explore", desc: "차트 컴포넌트 위치 찾기", started: t0 - 57 * SEC, tool: "Grep", brain: haiku() }),
          sub({ id: "a-2", type: "code-reviewer", desc: "변경 사항 리뷰", started: t0 - 27 * SEC, tool: "Read", brain: sonnet() }),
        ],
      }),
    ]),
    group("mobile-app", [
      session(cwdOf("mobile-app"), {
        id: "s-mob-1",
        state: "working",
        headless: true,
        canDrive: false,
        since: t0 - (8 * MIN + 47 * SEC),
        updated: t0 - 9 * SEC,
        topic: "iOS 빌드 경고 정리",
        brain: sonnet(),
        ctx: ctx(96_000),
      }),
    ]),
    group("infra", [
      session(cwdOf("infra"), {
        id: "s-infra-1",
        state: "idle",
        since: t0 - (2 * HOUR + MIN),
        updated: t0 - (2 * HOUR + MIN),
        topic: "Terraform 상태 잠금 정리",
        brain: sonnet("medium"),
        ctx: ctx(41_000, 200_000),
      }),
      session(cwdOf("infra"), {
        id: "s-infra-2",
        state: "ended",
        since: t0 - 3 * HOUR,
        updated: t0 - 3 * HOUR,
        topic: "CI 캐시 키 정리",
        name: "ci-cache-9d",
      }),
    ]),
  ];
}

const demo: Account = { id: "acc-demo|org-demo", email: "demo@example.com", org: "Demo", plan: "Max 20x", expired: false };
const work: Account = { id: "acc-work|org-work", email: "work@example.com", org: "Example Inc.", plan: "Max 5x", expired: false };
const old: Account = { id: "acc-old|org-old", email: "old@example.com", org: "", plan: "Pro", expired: true };

const S: Snapshot = {
  now: t0,
  runsClaude: [], // 지금 claude 가 도는 보드 터미널 — 가짜 화면에서 실제로 도는 claude 는 없다
  groups: initialGroups(),
  allMuted: false,
  counts: { waiting: 0, working: 0, idle: 0, paused: 0, ended: 0 },
  usage: {
    limits: [
      { id: "5h", label: "5시간", short: "5시간", tiny: "5h", pct: 38, resets: t0 + 2 * HOUR + 14 * MIN + 30 * SEC },
      { id: "7d", label: "주간 · 전체", short: "주간", tiny: "7d", pct: 54, resets: t0 + 71 * HOUR + 30 * SEC },
      { id: "7d-Fable", label: "주간 · Fable", short: "Fable", tiny: "F", pct: 12, resets: t0 + 71 * HOUR + 30 * SEC },
    ],
    failed: false,
    tokenExpired: false,
  },
  accounts: { current: demo, saved: [demo, work, old], adding: false, switching: false, autoSwitch: false, notice: null },
  updater: {
    current: "2.12",
    dev: false,
    latest: hasUpdate ? "2.13" : null,
    running: false,
    failed: false,
    checking: false,
    notice: null,
  },
  settings: {
    collapsed: [cwdOf("infra")],
    endedOpen: [],
    pinnedGroups: [],
    kindCollapsed: [],
    subCollapsed: [],
    usageCompact: false,
    pinned: true,
    mascot: "tralalero",
    opacity: 1,
    autoSwitch: false,
    boardMini: false,
    notifyPosition: "center",
    // 주소에 ?collapsed 를 붙이면 터미널을 접은 채, ?left 면 보드를 왼쪽에 두고 연다
    terminalExpanded: !q.has("collapsed"),
    boardSide: q.has("left") ? "left" : "right",
    boardPaneWidth: 454,
    mdRenderOff: [],
    shortcuts: {},
    settingsTransition: "slide",
    newSessionCommand: "",
    newSessionMdRender: true,
    autoUpdateCheck: true,
  },
  claudeVersion: "2.1.301",
  terminals: [
    { id: "wt", name: "Windows Terminal", installed: true, running: true },
    { id: "pwsh", name: "PowerShell 7", installed: true, running: false },
    { id: "powershell", name: "Windows PowerShell", installed: true, running: false },
    { id: "cmd", name: "명령 프롬프트", installed: true, running: false },
    { id: "git-bash", name: "Git Bash", installed: false, running: false },
  ],
};

if (scenario === "mac1") scenarios.applyMac1?.(S);

const rank: Record<State, number> = { waiting: 0, working: 1, paused: 2, idle: 2, ended: 3 };

// §5.1 정렬 — 그룹 안은 급한 순·최근 순, 그룹은 고정 순서 → 가장 급한 상태 → 가장 최근 활동
function normalize() {
  for (const g of S.groups) {
    g.sessions.sort((a, b) => rank[a.state] - rank[b.state] || b.since - a.since);
    for (const s of g.sessions) s.title = s.topic || s.message || "요청 전";
  }
  const pins = S.settings.pinnedGroups;
  const urgent = (g: Group) => Math.min(...g.sessions.map((s) => rank[s.state]));
  const last = (g: Group) => Math.max(...g.sessions.map((s) => s.updated));
  S.groups.sort((a, b) => {
    const pa = pins.indexOf(a.id), pb = pins.indexOf(b.id);
    if (pa >= 0 || pb >= 0) return (pa < 0 ? 1e9 : pa) - (pb < 0 ? 1e9 : pb);
    return urgent(a) - urgent(b) || last(b) - last(a);
  });
  const c: Record<State, number> = { waiting: 0, working: 0, idle: 0, paused: 0, ended: 0 };
  for (const g of S.groups) for (const s of g.sessions) c[s.state]++;
  S.counts = c;
}

let publish: (s: Snapshot) => void = () => {};

function push() {
  normalize();
  S.now = scenario === "mac1" && scenarios.FROZEN_NOW ? scenarios.FROZEN_NOW : Date.now();
  publish(structuredClone(S));
}

export function startMock(fn: (s: Snapshot) => void) {
  publish = fn;
  push();
  setInterval(push, 1000);
}

function toggle(list: string[], item: string) {
  const i = list.indexOf(item);
  if (i >= 0) list.splice(i, 1);
  else list.push(item);
}

function find(id: string): Session | undefined {
  for (const g of S.groups) for (const s of g.sessions) if (s.id === id) return s;
}

const timers: Record<string, ReturnType<typeof setTimeout>> = {};
function later(key: string, ms: number, fn: () => void) {
  clearTimeout(timers[key]);
  timers[key] = setTimeout(() => {
    fn();
    push();
  }, ms);
}

function accountNotice(text: string, error = false) {
  S.accounts.notice = { text, error };
  later("accountNotice", 7000, () => (S.accounts.notice = null));
}

function updaterNotice(kind: "ok" | "warn" | "dev", text: string) {
  S.updater.notice = { kind, text };
  later("updaterNotice", 4000, () => (S.updater.notice = null));
}

export async function handle(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  const str = (k: string) => String(a[k] ?? "");
  const bool = (k: string) => a[k] === true;
  const st = S.settings;
  switch (cmd) {
    case "get_snapshot":
      normalize();
      return structuredClone(S);
    case "toggle_collapsed":
      toggle(st.collapsed, str("cwd"));
      break;
    case "set_all_collapsed":
      st.collapsed = bool("collapse") ? S.groups.map((g) => g.id) : [];
      break;
    case "toggle_ended_open":
      toggle(st.endedOpen, str("cwd"));
      break;
    case "toggle_pin_group":
      toggle(st.pinnedGroups, str("cwd"));
      break;
    case "hide_group":
      S.groups = S.groups.filter((g) => g.id !== str("cwd"));
      st.pinnedGroups = st.pinnedGroups.filter((c) => c !== str("cwd"));
      break;
    case "remove_ended":
      for (const g of S.groups) if (g.id === str("cwd")) g.sessions = g.sessions.filter((s) => s.state !== "ended");
      S.groups = S.groups.filter((g) => g.sessions.length);
      break;
    case "remove_session":
      for (const g of S.groups) g.sessions = g.sessions.filter((s) => s.id !== str("id"));
      S.groups = S.groups.filter((g) => g.sessions.length);
      break;
    case "toggle_kind_collapsed":
      toggle(st.kindCollapsed, str("key"));
      break;
    case "toggle_sub_collapsed":
      toggle(st.subCollapsed, str("id"));
      break;
    case "set_usage_compact":
      st.usageCompact = bool("on");
      break;
    case "set_mute": {
      const s = find(str("id"));
      if (s) s.muted = bool("muted");
      break;
    }
    case "set_all_muted":
      S.allMuted = bool("on");
      break;
    case "reveal":
      st.collapsed = st.collapsed.filter((c) => c !== str("cwd"));
      st.kindCollapsed = st.kindCollapsed.filter((k) => !k.startsWith(str("cwd") + "|"));
      break;
    case "session_action": {
      const s = find(str("id"));
      const action = str("action");
      console.info(`[mock] session_action ${action} ${str("id")}`);
      if (!s) break;
      const now = Date.now();
      if (action === "reopen" && s.state === "ended") Object.assign(s, { state: "working", since: now, updated: now, paused: false });
      if (action === "pause") Object.assign(s, { state: "paused", paused: true, updated: now });
      if (action === "resume") Object.assign(s, { state: "working", paused: false, since: now, updated: now });
      if (action === "stop") Object.assign(s, { state: "ended", paused: false, subagents: [], updated: now });
      break;
    }
    case "switch_account": {
      const target = S.accounts.saved.find((x) => x.id === str("id"));
      if (!target) break;
      S.accounts.switching = true;
      later("switch", 700, () => {
        S.accounts.switching = false;
        S.accounts.current = target;
        accountNotice(`${target.email} 계정으로 전환했습니다 — 실행 중인 세션도 다음 요청부터 이 계정을 씁니다`);
      });
      break;
    }
    case "begin_add_account":
      S.accounts.adding = true;
      accountNotice("터미널과 브라우저에서 추가할 계정으로 로그인해 주세요");
      later("add", 6000, () => {
        S.accounts.adding = false;
        const email = (a.email as string | null) || "new@example.com";
        const acc: Account = { id: `acc-${email}`, email, org: "", plan: "Pro", expired: false };
        S.accounts.saved = [...S.accounts.saved.filter((x) => x.email !== email), acc];
        accountNotice(`${email} 계정을 추가했습니다 — 계정 메뉴에서 골라 바로 전환할 수 있습니다`);
      });
      break;
    case "cancel_add_account":
      clearTimeout(timers.add);
      S.accounts.adding = false;
      break;
    case "delete_account": {
      const acc = S.accounts.saved.find((x) => x.id === str("id"));
      S.accounts.saved = S.accounts.saved.filter((x) => x.id !== str("id"));
      if (acc) accountNotice(`${acc.email} 계정을 목록에서 지웠습니다`);
      break;
    }
    case "set_auto_switch":
      S.accounts.autoSwitch = st.autoSwitch = bool("on");
      break;
    case "check_update": {
      const manual = bool("manual");
      if (manual) S.updater.checking = true;
      later("check", 800, () => {
        S.updater.checking = false;
        if (hasUpdate) S.updater.latest = "2.13";
        else if (manual) updaterNotice("ok", `최신 버전입니다 (${S.updater.current})`);
      });
      break;
    }
    case "run_update":
      S.updater.running = true;
      S.updater.failed = false;
      later("update", 3000, () => {
        S.updater.running = false;
        S.updater.failed = true;
      });
      break;
    case "open_changelog":
      window.open("https://github.com/suhojang/claude-board/blob/main/windows/CHANGELOG.md", "_blank");
      break;
    case "set_pinned":
      st.pinned = bool("on");
      break;
    case "set_mascot":
      st.mascot = a.kind as Snapshot["settings"]["mascot"];
      break;
    case "set_opacity":
      st.opacity = Number(a.v) || 1;
      break;
    case "set_notify_position":
      st.notifyPosition = a.pos as Snapshot["settings"]["notifyPosition"];
      break;
    case "set_board_mini":
      st.boardMini = bool("on");
      break;
    case "set_shortcut": {
      const combo = a.combo as Snapshot["settings"]["shortcuts"][string] | null;
      if (combo) st.shortcuts[str("id")] = combo;
      else delete st.shortcuts[str("id")];
      break;
    }
    case "reset_shortcuts":
      st.shortcuts = {};
      break;
    case "set_settings_transition":
      st.settingsTransition = a.value as Snapshot["settings"]["settingsTransition"];
      break;
    case "set_new_session_defaults":
      st.newSessionCommand = str("command").trim();
      st.newSessionMdRender = bool("mdRender");
      break;
    case "set_auto_update_check":
      st.autoUpdateCheck = bool("on");
      break;
    case "login_launch":
      return true;
    case "set_login_launch":
    case "submit_login_code":
    case "open_login_url":
      return;
    case "set_terminal_expanded":
      st.terminalExpanded = bool("on");
      break;
    case "set_board_side":
      st.boardSide = a.side === "left" ? "left" : "right";
      break;
    case "set_board_width":
      st.boardPaneWidth = Number(a.width) || 454;
      break;
    case "term_spawn":
      return spawnTerm(a.spec as TermSpec);
    case "term_list":
      return [...fakeTerms.values()].map((t) => t.info());
    case "term_ack":
      return;
    case "term_write":
      fakeTerms.get(str("id"))?.input(str("data"));
      return;
    case "term_write_bin":
      fakeTerms.get(str("id"))?.input(String.fromCharCode(...(a.data as number[])));
      return;
    case "take_open_session":
      return [];
    case "save_tabs":
    case "answer_shutdown":
    case "shutdown_dialog_shown":
    case "lifecycle_started":
      return;
    case "load_tabs":
      return [];
    case "existing_dirs":
      return a.paths; // 브라우저 개발 화면에는 폴더 검사가 없다 — 다 있다고 답한다
    case "prune_md_render_off":
      return;
    case "pids_alive":
      return [];
    case "wait_exit":
      return true;
    case "md_render_dir":
    case "test_claude_prefix":
      return null;
    case "remember_md_render_off":
      toggle(st.mdRenderOff, str("id"));
      break;
    case "reopen_done":
      return;
    case "drive_session":
      return false;
    case "rename_check":
      return { safe: false, transcript: false };
    case "show_board":
      return;
    case "term_resize":
      fakeTerms.get(str("id"))?.resize(Number(a.cols), Number(a.rows));
      return null; // 기준 seq 없음 — 장벽이 바로 적용한다
    case "term_kill":
      fakeTerms.get(str("id"))?.exit(1);
      return;
    case "open_terminal": {
      const t = S.terminals.find((x) => x.id === str("id"));
      if (t?.installed) t.running = true;
      break;
    }
    default:
      // enter_mini·leave_mini·snap_right·hide_board·minimize_board·log — 브라우저에서는 창을 다룰 수 없어 기록만 한다
      console.info(`[mock] ${cmd}`, a);
      return;
  }
  push();
}

// ── 가짜 터미널(브라우저 개발용) ─────────────────────────────────
// 셸 대신 한 줄을 쳐서 Enter 하면 그 줄을 되돌려 주는 에코 터미널. 실제 백엔드와 같은 프레임(8바이트 seq + 출력, JSON gap·exit)을
// 내보내 프런트의 해독·다시 받기·종료 처리까지 같은 길로 지나게 한다. 명령: exit [코드] 종료 · flood 많은 줄 쏟아내기 · clear 화면 지우기

// 터미널 칸을 미리 여는 길 — 주소에 ?tabs=N(1~12)을 붙이면 터미널을 펼칠 때 탭 N 개가 열린다(경로 줄·가로 밀림·상태 점·같은 이름 라벨을 볼 때)
const TAB_PRESETS: { dir: string; sessionId?: string; name?: string }[] = [
  { dir: cwdOf("shop-api"), sessionId: "s-shop-1" },
  { dir: cwdOf("shop-api"), sessionId: "s-shop-2" },
  { dir: cwdOf("web-dashboard"), sessionId: "s-web-1" },
  { dir: cwdOf("shop-api") },
  { dir: cwdOf("mobile-app"), sessionId: "s-mob-1" },
  { dir: cwdOf("infra"), sessionId: "s-infra-1" },
  { dir: "D:\\other\\shop-api" },
  { dir: HOME },
  { dir: cwdOf("infra"), sessionId: "s-infra-2" },
  { dir: cwdOf("web-dashboard"), name: "배포 확인용 터미널" },
  { dir: cwdOf("web-dashboard") },
  { dir: cwdOf("web-dashboard") },
];

export function mockTabs() {
  const n = Math.min(12, Math.max(0, Number(q.get("tabs")) || 0));
  return TAB_PRESETS.slice(0, n);
}

const PROMPT = "PS C:\\Users\\demo> ";
const encoder = new TextEncoder();
const fakeTerms = new Map<TermId, FakeTerm>();
let nextTermNumber = 1;

class FakeTerm {
  private log: Uint8Array[] = [];
  private nextSeq = 0;
  private line = "";
  private emit: ((message: unknown) => void) | null = null;
  private status: TermInfo["status"] = "running";
  private exitCode: number | null = null;

  constructor(
    readonly id: TermId,
    private cols: number,
    private rows: number,
    private readonly cwd: string,
  ) {
    // 제목(OSC 0) — 셸 탭의 "하는 일" 자리에 보이는 터미널 제목
    this.print(`\x1b]0;${cwd}\x07Mock PowerShell (echo) ${cols}x${rows}\r\n한 줄을 치고 Enter 를 누르면 되돌려 줍니다. exit, flood, clear\r\n\r\n${PROMPT}`);
  }

  info(): TermInfo {
    return {
      id: this.id,
      pid: 4000 + Number(this.id.slice(1)),
      shell: "pwsh",
      cwd: this.cwd,
      cols: this.cols,
      rows: this.rows,
      status: this.status,
      exitCode: this.exitCode,
      windowsBuild: 26200,
      oldestSeq: 0,
      nextSeq: this.nextSeq,
    };
  }

  // 채널을 이 연결로 바꾸고 fromSeq 부터 다시 보낸다
  attach(fromSeq: number, emit: (message: unknown) => void): TermInfo {
    this.emit = emit;
    let offset = 0;
    for (const chunk of this.log) {
      if (offset + chunk.length > fromSeq) {
        const skip = Math.max(0, fromSeq - offset);
        this.send(offset + skip, chunk.subarray(skip));
      }
      offset += chunk.length;
    }
    if (this.status === "exited") emit({ t: "exit", code: this.exitCode });
    return this.info();
  }

  resize(cols: number, rows: number) {
    if (cols < 2 || rows < 1 || (cols === this.cols && rows === this.rows)) return;
    this.cols = cols;
    this.rows = rows;
  }

  input(data: string) {
    if (this.status === "exited") return;
    for (const ch of data) {
      if (ch === "\r" || ch === "\n") this.submit();
      else if (ch === "\x7f") {
        if (this.line) {
          this.line = [...this.line].slice(0, -1).join("");
          this.print("\b \b");
        }
      } else if (ch === "\x03") {
        this.line = "";
        this.print(`^C\r\n${PROMPT}`);
      } else if (ch === "\x1b") this.print("^[");
      else if (ch >= " ") {
        this.line += ch;
        this.print(ch);
      }
    }
  }

  exit(code: number) {
    if (this.status === "exited") return;
    this.status = "exited";
    this.exitCode = code;
    this.emit?.({ t: "exit", code });
  }

  private submit() {
    const line = this.line.trim();
    this.line = "";
    const [command, arg] = line.split(/\s+/);
    if (command === "exit") {
      this.print("\r\n");
      this.exit(Number(arg) || 0);
    } else if (command === "clear") this.print("\x1b[2J\x1b[H" + PROMPT);
    else if (command === "flood") {
      let text = "\r\n";
      for (let i = 1; i <= 3000; i++) text += `line ${i}\r\n`;
      this.print(text + PROMPT);
    } else this.print(`\r\n${line ? `${line}\r\n` : ""}${PROMPT}`);
  }

  private print(text: string) {
    const bytes = encoder.encode(text);
    this.log.push(bytes);
    this.send(this.nextSeq, bytes);
    this.nextSeq += bytes.length;
  }

  private send(seq: number, bytes: Uint8Array) {
    if (!this.emit) return;
    const frame = new ArrayBuffer(8 + bytes.length);
    new DataView(frame).setBigUint64(0, BigInt(seq), true);
    new Uint8Array(frame, 8).set(bytes);
    this.emit(frame);
  }
}

function spawnTerm(spec: TermSpec): TermInfo {
  const id = `t${nextTermNumber++}`;
  const term = new FakeTerm(id, spec.cols, spec.rows, spec.cwd ?? HOME);
  fakeTerms.set(id, term);
  return term.info();
}

export function attachTerm(id: TermId, fromSeq: number, emit: (message: unknown) => void): TermInfo {
  const term = fakeTerms.get(id);
  if (!term) throw new Error(`터미널 ${id} 없음`);
  return term.attach(fromSeq, emit);
}

// 브라우저 개발 화면의 "다른 폴더 고르기…" — 대화상자가 없어 고정된 폴더를 돌려준다
export function mockPickFolder(): string {
  return `${HOME}\\picked-folder`;
}
