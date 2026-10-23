// 탭 이름 바꾸기(⇧Ctrl+R)를 그 탭 claude 세션들의 /rename 으로 옮기는 순수 계획 — mac ClaudeBoard.swift 의
// renameSessions·pickName·applyRenames 의 Windows 판. Svelte 도 입출력도 모르고 입력을 고치지 않는다 — hub 가 결과대로 키를 넣는다.
import type { BoardSurface, Session } from "../types.ts";

/** 이름 바꾸기를 어디까지 쳤나 — 없음(아직 안 침) · 글을 치는 중 · 글은 붙였고 Enter 만 남음 · Enter 를 친 시각(ms) */
export type RenameStep = { kind: "typing" } | { kind: "typed" } | { kind: "sent"; at: number };

export interface RenameTarget {
  surfaceId: string;
  target: string; // 이 화면의 claude 에 붙일 새 세션 이름
}

/** Enter 를 친 뒤 이름이 안 바뀌어도 기다리는 시간 — 이만큼 지나면 그만둔다(mac 5초) */
export const RENAME_SENT_TIMEOUT_MS = 5000;

/** 대소문자를 가리지 않고 안 쓰는 이름을 고른다(mac pickName) — 쓰는 이름이면 base-2, base-3 … */
function pickName(base: string, used: Set<string>): string {
  let name = base;
  let serial = 2;
  while (used.has(name.toLowerCase())) name = `${base}-${serial++}`;
  return name;
}

/**
 * 탭의 화면마다 붙일 세션 이름을 정한다(mac renameSessions). 화면의 주 세션 = 사는(ended 아닌) 세션 가운데 tty 가 화면 termId 와 같은 것.
 * 주 세션이 없는 셸 화면·끝난 세션·mode(-p·--bg)가 있는 세션·headless 는 뺀다. 이미 그 이름이면 목록에서만 빠진다(자리는 차지한다).
 * 쓰는 이름 = 다른 세션의 살아 있는 이름(자기 화면 것은 뺀다) + reserved(방금 붙여 아직 파일에 없는 이름 등) + 앞 화면이 가져간 이름.
 */
export function renameTargets(
  tabName: string,
  surfaces: BoardSurface[],
  sessions: Session[],
  reserved: string[],
): RenameTarget[] {
  const alive = sessions.filter((session) => session.state !== "ended" && session.tty);
  const mainOf = (surface: BoardSurface): Session | undefined =>
    surface.termId === null ? undefined : alive.find((session) => session.tty === surface.termId);

  const taken = new Set<string>(reserved.map((name) => name.toLowerCase()));
  const result: RenameTarget[] = [];
  for (const surface of surfaces) {
    const main = mainOf(surface);
    if (!main || main.mode !== null || main.headless) continue;
    const used = new Set(taken);
    for (const other of sessions) {
      if (other.id !== main.id && other.state !== "ended" && other.name) used.add(other.name.toLowerCase());
    }
    const target = pickName(tabName, used);
    taken.add(target.toLowerCase());
    if (main.name !== target) result.push({ surfaceId: surface.id, target });
  }
  return result;
}

/** 이름 바꾸기를 쳐도 되는가 — 보드 상태가 작업 중·대기(idle)이고 Claude Code 상태가 승인 창·질문 창(waiting)이 아니다(mac renameSafe) */
export function renameSafe(state: Session["state"], claudeStatus: string | null): boolean {
  return (state === "working" || state === "idle") && claudeStatus !== "waiting";
}

export interface RenameStepResult {
  step: RenameStep | undefined; // undefined = 이 화면의 이름 바꾸기가 끝났다(상태를 지운다)
  emit: "text" | "enter" | "none"; // text = `/rename <이름>` 글을 붙인다 · enter = Enter 를 친다
}

/**
 * 화면 하나의 다음 걸음(mac applyRenames). 세션이 끝났거나 이미 그 이름이면 끝낸다. 글을 치는 중(typing)에는 기다린다.
 * sent 는 5초가 지나면 끝낸다. 시작하기(st 없음)·Enter 만 남은 때(typed)는 check.safe 와 check.transcript 가 모두 참일 때만 나아간다 —
 * 시작이면 emit text + step typing, typed 면 emit enter + step sent(now).
 * hub 는 글 붙이기가 끝난 뒤 step 을 typed 로 바꾸고 이 함수를 다시 불러 Enter 직전에 안전을 한 번 더 확인한다(그 사이 창이 떴으면 typed 로 남는다).
 */
export function nextRenameStep(
  st: RenameStep | undefined,
  s: Session,
  target: string,
  now: number,
  check: { safe: boolean; transcript: boolean },
): RenameStepResult {
  if (s.state === "ended" || s.name === target) return { step: undefined, emit: "none" };
  if (st?.kind === "typing") return { step: st, emit: "none" };
  if (st?.kind === "sent") {
    return now - st.at > RENAME_SENT_TIMEOUT_MS ? { step: undefined, emit: "none" } : { step: st, emit: "none" };
  }
  if (!check.safe || !check.transcript) return { step: st, emit: "none" };
  if (st?.kind === "typed") return { step: { kind: "sent", at: now }, emit: "enter" };
  return { step: { kind: "typing" }, emit: "text" };
}
