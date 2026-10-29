// 시험용 claude 바꿔치기(D5) — 환경변수 CLAUDE_BOARD_CLAUDE 가 있는 시험 실행에서만 보드가 치는 명령의 맨 앞 `claude` 를 가짜로 바꾼다. 순수 함수다.
// 바꾸는 것은 명령 맨 앞 낱말 하나뿐이다 — `claude`·`claude.exe`·`claude.cmd`, 그 앞에 경로가 붙은 것, 큰따옴표로 묶은 것. 인자는 건드리지 않는다.
// 바꿀 값(prefix)은 절대 경로 조각(프로그램 + 앞쪽 인자)이고, 조각마다 셸의 따옴표 규칙으로 묶는다.
//   pwsh·powershell  `& '<조각>' '<조각>'`   작은따옴표 안은 글자 그대로라 $·`·공백·한글이 안전하다(작은따옴표는 두 번 쓴다)
//   cmd              `"<조각>" "<조각>"`     큰따옴표로 묶는다(경로에 큰따옴표는 못 든다)
import type { TermInfo } from "../types.ts";

// 맨 앞 claude 낱말: (공백)(큰따옴표로 묶은 것 | 묶지 않은 것) 바로 뒤가 공백이나 끝이어야 한다(claude2·claude-board 는 아니다)
const LEADING = /^(\s*)(?:"(?:[^"]*[\\/])?claude(?:\.exe|\.cmd)?"|(?:[^\s"]*[\\/])?claude(?:\.exe|\.cmd)?)(?=\s|$)/i;
// PowerShell 은 곡선 작은따옴표도 작은따옴표로 읽는다
const PS_QUOTES = /['‘’‚‛]/g;

function quotePart(part: string, shell: TermInfo["shell"]): string {
  return shell === "cmd" ? `"${part}"` : `'${part.replace(PS_QUOTES, (q) => q + q)}'`;
}

/** prefix = 프로그램과 앞쪽 인자(절대 경로). null·빈 배열이면 시험 모드가 아니라 그대로 */
export function applyTestClaude(command: string, prefix: string[] | null, shell: TermInfo["shell"]): string {
  if (!prefix || prefix.length === 0) return command;
  const start = LEADING.exec(command);
  if (!start) return command;
  const quoted = prefix.map((part) => quotePart(part, shell)).join(" ");
  const replacement = shell === "cmd" ? quoted : `& ${quoted}`;
  return `${start[1]}${replacement}${command.slice(start[0].length)}`;
}

/** 시험 모드 — 꺼짐 / 켜짐 / 어긋남(켜려 했으나 거절됐거나 물어보지 못함) / 아직 안 받음. 어긋남·안 받음에서는 아무 명령도 치지 않는다 */
export type TestClaudeMode =
  | { kind: "unloaded" }
  | { kind: "off" }
  | { kind: "on"; prefix: string[] }
  | { kind: "invalid"; reason: string };

let mode: TestClaudeMode = { kind: "unloaded" };

export function currentTestClaudeMode(): TestClaudeMode {
  return mode;
}

/** 바꿔칠 값 — 켜짐일 때만 있다 */
export function currentTestClaudePrefix(): string[] | null {
  return mode.kind === "on" ? mode.prefix : null;
}

/** 명령을 쳐 넣으면 안 되는 까닭 — 쳐도 되면 null. 시험 모드인지 모르거나 어긋난 채로 치면 진짜 claude 가 뜰 수 있다 */
export function typingBlockedReason(): string | null {
  if (mode.kind === "unloaded") return "시험 claude 여부를 아직 받지 못했다";
  if (mode.kind === "invalid") return mode.reason;
  return null;
}

/** 시험용 — 받기 전 상태로 되돌린다 */
export function resetTestClaude(): void {
  mode = { kind: "unloaded" };
}

/**
 * 백엔드의 답을 받아 둔다. null·없음 = 꺼짐, 문자열 조각 배열 = 켜짐, `{invalid: 이유}` 와 그 밖의 모든 모양 = 어긋남(닫힌 채로 실패한다).
 */
export function setTestClaudePrefix(value: unknown): void {
  if (value === null || value === undefined) {
    mode = { kind: "off" };
  } else if (Array.isArray(value) && value.length > 0 && value.every((part) => typeof part === "string" && part !== "")) {
    mode = { kind: "on", prefix: value as string[] };
  } else if (typeof value === "object" && !Array.isArray(value) && typeof (value as { invalid?: unknown }).invalid === "string") {
    const why = (value as { invalid: string }).invalid;
    mode = { kind: "invalid", reason: why || "시험 claude 설정이 어긋났다" };
  } else {
    mode = { kind: "invalid", reason: "시험 claude 답의 모양이 맞지 않는다" };
  }
}

/** 백엔드에 한 번 묻는다(startHub 가 첫 명령 전에 기다린다) — 실패하면 어긋남으로 둔다. 시험 하니스는 첫 키 전에 `testClaudePrefix()` 값을 직접 확인한다 */
export async function loadTestClaudePrefix(fetchPrefix: () => Promise<unknown>): Promise<void> {
  try {
    setTestClaudePrefix(await fetchPrefix());
  } catch (e) {
    mode = { kind: "invalid", reason: `시험 claude 여부를 묻지 못했다: ${e instanceof Error ? e.message : String(e)}` };
  }
}
