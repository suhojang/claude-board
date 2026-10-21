// 브라우저(npm run dev)용 가짜 백엔드 — docs/images/hero.png 와 같은 장면을 만들고, SPEC §6.2 명령마다 이 상태를 고쳐
// 보드를 Tauri 없이도 끝까지 눌러 볼 수 있게 한다. 주소에 ?update 를 붙이면 새 버전 알림 줄, ?empty 면 세션 없는 화면
import type { Account, Brain, Group, Session, Snapshot, State, Sub } from "./types";

const SEC = 1000;
const MIN = 60 * SEC;
const HOUR = 60 * MIN;
const HOME = "C:\\Users\\demo";

const q = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
const hasUpdate = q.has("update");
const empty = q.has("empty");

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
  S.now = Date.now();
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
      if (action === "resume" && s.state === "ended") Object.assign(s, { state: "working", since: now, updated: now, paused: false });
      if (action === "pause") Object.assign(s, { state: "paused", paused: true, updated: now });
      if (action === "resume_paused") Object.assign(s, { state: "working", paused: false, since: now, updated: now });
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
    case "add_account":
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
    case "remove_account": {
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
