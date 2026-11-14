// 경로 카드 안의 자율주행 줄이 보이는 글과 짝짓기 규칙 — 판단은 여기, 그리는 일은 AutopilotLine.svelte.
// DOM·Svelte 를 부르지 않아(타입만 import) node:test 로 시험한다.
// 사용자에게 보이는 글에는 긴 줄표를 쓰지 않는다: 구분은 가운뎃점과 쌍점으로 한다.
import type { AutopilotAction, AutopilotRun } from "../types.ts";

/** 경로 견주기: 대소문자 무시, `/`·`\` 같게, 끝 구분자 무시 */
export function samePath(a: string, b: string): boolean {
  return normalizePath(a) === normalizePath(b);
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** 그 카드(Group.path)의 실행들 — 들어온 차례 그대로 */
export function runsForGroup(runs: AutopilotRun[], groupPath: string): AutopilotRun[] {
  return runs.filter((run) => samePath(run.path, groupPath));
}

/** 어느 카드에도 짝이 없는 실행 */
export function orphanRuns(runs: AutopilotRun[], groupPaths: string[]): AutopilotRun[] {
  return runs.filter((run) => !groupPaths.some((groupPath) => samePath(run.path, groupPath)));
}

export interface LineText {
  state: string;
  env: string;
  noSandbox: boolean;
  unit: string | null;
  clock: string | null;
  cost: string | null;
  tally: string;
  wait: string | null;
}

const STATE_LABELS: Record<string, string> = {
  running: "도는 중",
  waiting_limit: "사용량 한도 대기",
  waiting_stopline: "멈출 선 대기",
  waiting_precheck: "시험 환경 대기",
  paused: "일시정지",
  taken_over: "터미널로 이어받음",
  starting: "출발 중",
  done: "끝남",
  stopped: "중단됨",
  error: "오류로 멈춤",
};

export function stateLabel(state: string): string {
  return STATE_LABELS[state] ?? state;
}

/** 단위가 쓴 시간 — 45초 / 12분 / 1시간 3분 */
export function clockText(activeSec: number): string {
  const sec = Math.max(0, Math.floor(activeSec));
  if (sec < 60) return `${sec}초`;
  const minutes = Math.floor(sec / 60);
  if (minutes < 60) return `${minutes}분`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}시간` : `${hours}시간 ${rest}분`;
}

/** 1 미만은 소수 셋째까지, 이상은 둘째까지 */
export function costText(costUsd: number): string {
  const digits = Math.abs(costUsd) < 1 ? 3 : 2;
  return `이번 단위 ${costUsd.toFixed(digits)} USD`;
}

export function tallyText(run: AutopilotRun): string {
  const parts = [`통과 ${run.done}`, `미통과 ${run.failed}`, `상한 ${run.capped}`];
  if (run.skipped > 0) parts.push(`건너뜀 ${run.skipped}`);
  if (run.stopped > 0) parts.push(`중단 ${run.stopped}`);
  return parts.join(" · ");
}

const pad2 = (n: number) => String(n).padStart(2, "0");

/** 풀리는 시각 — 지금과 같은 날이면 "14:05 까지", 다른 날이면 "11월 6일 14:05 까지"(지역 시각) */
export function waitText(waitUntil: number | null, now: number): string | null {
  if (waitUntil == null) return null;
  const at = new Date(waitUntil);
  const today = new Date(now);
  const time = `${pad2(at.getHours())}:${pad2(at.getMinutes())}`;
  const sameDay = at.getFullYear() === today.getFullYear() && at.getMonth() === today.getMonth() && at.getDate() === today.getDate();
  return sameDay ? `${time} 까지` : `${at.getMonth() + 1}월 ${at.getDate()}일 ${time} 까지`;
}

export function lineText(run: AutopilotRun, now: number): LineText {
  const unit = run.unit;
  return {
    state: stateLabel(run.state),
    env: run.env.label || "알 수 없음",
    noSandbox: !run.env.sandbox,
    unit: unit ? `단위 ${unit.n}/${run.total}: ${unit.title}` : null,
    clock: unit ? clockText(unit.activeSec) : null,
    cost: unit ? costText(unit.costUsd) : null,
    tally: tallyText(run),
    wait: waitText(run.waitUntil, now),
  };
}

const BUTTON_ORDER: { action: AutopilotAction; label: string }[] = [
  { action: "pause", label: "일시정지" },
  { action: "resume", label: "재개" },
  { action: "stop", label: "중단" },
];

/** 단추: can 에 있는 것만, 차례 pause → resume → stop. 끝난 실행은 없다 */
export function buttons(run: AutopilotRun): { action: AutopilotAction; label: string }[] {
  if (run.ended) return [];
  return BUTTON_ORDER.filter((button) => run.can.includes(button.action));
}
