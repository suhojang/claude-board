// 바깥 열기(--open-tab) 요청의 앞 처리 — mac OpenTab.swift 의 앞 처리를 Windows 판으로 옮긴 것이다.
// open-tab 이벤트 꼴을 읽고(plan 이 골라 보낸 꼴 — 새 세션의 --session-id 는 plan 이 이미 끼운다),
// argv 를 셸에 칠 한 줄로 묶는다. 이 파일은 외부를 import 하지 않아 node:test 로 바로 시험한다.
// 이벤트 꼴은 아직 wiring 이 고정하지 않아 snake_case·camelCase 를 둘 다 읽는다.

/** 바깥 열기 요청 하나 — plan.rs OpenTabAction 의 Focus·Reuse·Open 에 대응한다.
 * Focus(이미 도는 화면 가리키기)는 command 가 비고 reuseTab 만 든다 */
export interface OpenTabRequest {
  id: string;
  /** 작업 폴더(절대 경로) — 새 탭을 열 때 쓴다 */
  cwd: string;
  /** 탭 이름이자 세션 이름(plan 이 이미 정했다) — 없으면 탭 이름은 폴더 이름이 된다 */
  title: string | null;
  /** claude 명령 인자. Focus 는 비어 있다 */
  command: string[];
  /** true 면 탭을 만들어 두기만 하고 보드를 펼치지 않는다(mac openExternal 의 focus: false) */
  noFocus: boolean;
  /** 이어 열 화면 id — 없으면 새 탭(plan 의 Open) */
  reuseTab: string | null;
}

/** 이벤트 꼴을 요청으로 읽는다. 꼴이 맞지 않으면 없다 — 부른 쪽이 기다리던 요청을 조용히 버리지 않게, 부르는 쪽에 기록을 남기는 일은 듣는 쪽(hub)이 한다 */
export function parseOpenTabEvent(raw: unknown): OpenTabRequest | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const id = r["id"];
  const cwd = r["cwd"];
  if (typeof id !== "string" || id === "" || typeof cwd !== "string" || cwd === "") return null;
  const argv = r["command"];
  const command = argv === undefined || argv === null ? [] : argv; // Focus 는 command 가 없다
  if (!Array.isArray(command) || command.some((a) => typeof a !== "string" || a === "")) return null;
  const oneOf = (...keys: string[]): unknown => {
    for (const key of keys) if (key in r) return r[key];
    return undefined;
  };
  const title = oneOf("title");
  const reuse = oneOf("reuse_tab", "reuseTab");
  return {
    id,
    cwd,
    title: typeof title === "string" && title !== "" ? title : null,
    command,
    noFocus: oneOf("no_focus", "noFocus") === true,
    reuseTab: typeof reuse === "string" && reuse !== "" ? reuse : null,
  };
}

// 화면의 좁은 꼴 — 이어 열 찾기에 필요한 것만 본다(외부 import 없이 순수하게 시험하려고)
interface ReuseCandidate {
  id: string;
  termId: string | null;
}

/** 이어 열 화면(reuseTab) 찾기 — id 는 화면 id(옛 꼴)거나 백엔드 터미널 id("t1" 꼴)다.
 * 화면 id 가 이기고(id 로 찾은 것이 화면 id 와 termId 가 겹친 두 후보 가운데 이긴다),
 * 화면 id 로 없으면 termId 로 본다. 둘 다 없으면 없다 — 부른 쪽은 그 사이 닫힌 화면을 골랐다 */
export function findReuseSurface<S extends ReuseCandidate>(surfaces: readonly S[], id: string): S | undefined {
  return surfaces.find((s) => s.id === id) ?? surfaces.find((s) => s.termId === id);
}

// 세션 id 꼴 — plan.rs is_session_id 와 같다(8-4-4-4-12 자리의 16진수)
const SESSION_ID_SHAPE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;

/** `--flag 값`(`-f 값`도)이나 `--flag=값`의 값 — 값 인자는 - 로 시작하지 않을 때만 읽는다(plan.rs next_value 와 같다) */
function flagValue(argv: readonly string[], flag: string, short?: string): string | null {
  for (let i = 1; i < argv.length; i++) {
    const a = argv[i];
    const inline = a.startsWith(flag + "=") ? a.slice(flag.length + 1) : null;
    if (a !== flag && a !== short && inline === null) continue;
    if (inline !== null) return inline;
    const next = argv[i + 1];
    return next !== undefined && !next.startsWith("-") ? next : null;
  }
  return null;
}

/**
 * argv 에서 세션 id — plan.rs open_tab_prepare(199-207)과 같은 차례로 읽는다. --resume(uuid 꼴이고 --fork-session 이 아닐 때)이
 * 이기고, --resume 이 있으면 그 모양이 맞지 않더라도 --session-id 는 보지 않는다. 세션 파일·짝짓기는 소문자 id 를 쓰므로
 * 소문자로 맞춘다(mac run 의 lowercased). 새 세션의 --session-id 는 plan 이 이미 끼워 보내므로 여기서 만들 일은 없다
 */
export function openTabSession(argv: readonly string[]): string | null {
  const forked = argv.includes("--fork-session");
  const resume = flagValue(argv, "--resume", "-r");
  if (resume !== null) {
    if (!forked && SESSION_ID_SHAPE.test(resume)) return resume.toLowerCase();
    return null;
  }
  const flagged = flagValue(argv, "--session-id");
  return flagged !== null && SESSION_ID_SHAPE.test(flagged) ? flagged.toLowerCase() : null;
}

// 낱말 하나가 어느 셸(pwsh·powershell·cmd·git-bash)에서나 그대로 넘기는 글자 — shellCommand.ts 의 SHELL_SAFE_CHARS 에
// Windows 경로의 \ 를 더했다. WSL 은 명령을 치지 않는다(shellRules 의 canTypeClaude)
const BARE_SAFE = /^[A-Za-z0-9_@%+=:,./\\-]+$/u;

/**
 * 인자 목록을 셸에 칠 한 줄로 묶는다. 인용은 큰따옴표 하나뿐 — quoteSessionName·sessionCommand(이어 열기 끝말)이 쓰는 규칙이다.
 * cmd·PowerShell·git-bash 가 모두 인용으로 읽는 것은 큰따옴표뿐이라 작은따옴표(shellCommand 의 shlex 꼴)는 쓰지 않는다.
 * 묶은 뒤에도 안전하게 넘길 수 없는 낱말이면 없다로 끝낸다 — 셸이 다르게 읽을 정보를 칠 것보다 거절이 낫다(기록을 남겨 알린다):
 * · 개행·탭 (mac 은 bracketed paste 로 그대로 넣지만 Windows 는 낱글자로 쳐 넣어 줄이 일찍 끝나거나 탭 완성이 눌린다)
 * · 큰따옴표·`·$ (묶과 묶지 않을에서 늘어나거나 명령을 늘린다)
 * · 끝이 \ 인 낱말 (git-bash 가 묶음을 닫기 전에 \" 로 읽어 묶음을 깬다)
 * · 빈 낱말 (빈 인자는 요청이 이미 이상하다 — plan 이 걸러야 할 것)
 */
export function openTabLine(argv: readonly string[]): string | null {
  if (argv.length === 0) return null;
  const parts: string[] = [];
  for (const word of argv) {
    if (word === "" || /["`$]|\n|\r|\t/u.test(word) || word.endsWith("\\")) return null;
    parts.push(BARE_SAFE.test(word) ? word : `"${word}"`);
  }
  return parts.join(" ");
}
