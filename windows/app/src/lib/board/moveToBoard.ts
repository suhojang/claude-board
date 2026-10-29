// 다른 터미널에서 도는 세션을 보드 탭으로 옮기기(mac moveToBoard) — 순서·확인·실패 알림 규칙. 효과는 deps 로 받는다.
// 일하는·기다리는 세션은 먼저 묻고, 콘솔에 Esc, 0.4초, `/exit`, 0.2초, Enter 를 넣어 끝낸 뒤 끝나기를 8초 기다려 보드 탭에서 이어 연다.
// 실패하면 알림 한 줄. 외부를 import 하지 않아(타입만) node:test 로 시험한다.
import type { DriveStep, Session } from "../types.ts";

/** 세션을 끝내는 키 순서(mac driveSession 인자 — wait 는 ms) */
export const MOVE_STEPS: DriveStep[] = [{ key: "escape" }, { wait: 400 }, { text: "/exit" }, { wait: 200 }, { key: "enter" }];

/** /exit 를 넣은 뒤 그 프로세스가 끝나기를 기다리는 최대 시간 */
export const EXIT_TIMEOUT_MS = 8000;

const CONFIRM_TITLE = "보드로 옮길까요?";
const CONFIRM_WARNING = "지금 터미널의 세션을 끝내고 보드 새 탭에서 같은 대화를 이어 엽니다. 진행 중인 응답은 끊깁니다.";

/** 옮기기 전에 확인을 받아야 하는 세션인가 — 응답 중이거나 승인을 기다리는 세션은 끊기므로 */
export function needsConfirm(s: Pick<Session, "state">): boolean {
  return s.state === "working" || s.state === "waiting";
}

export interface MoveDeps {
  confirm(title: string, text: string): Promise<boolean>;
  drive(id: string, steps: DriveStep[]): Promise<boolean>;
  waitExit(pid: number, pidCtime: number | null, timeoutMs: number): Promise<boolean>;
  resumeSession(s: Session): void;
  toast(text: string): void;
}

// 알림 앞에 붙이는 폴더 이름(mac lastPathComponent) — 끝 구분자는 무시한다
function lastFolder(cwd: string): string {
  return cwd.split(/[\\/]/).filter(Boolean).at(-1) ?? cwd;
}

/**
 * 누른 순간의 세션 — 메뉴를 연 뒤 상태가 바뀌었을 수 있어 확인 여부는 지금 상태로 정한다. 세션이 사라졌거나 이미 끝났으면 null(옮길 것이 없다).
 */
export function sessionAtClick<T extends Pick<Session, "state">>(current: T | undefined): T | null {
  return current && current.state !== "ended" ? current : null;
}

/** 같은 세션을 옮기는 중에 또 누른 것을 거른다 — 키(Esc·/exit)를 두 번 보내지 않게. 돌려주는 함수는 이미 도는 중이면 false */
export function createMoveGuard(): (id: string, run: () => Promise<void>) => Promise<boolean> {
  const moving = new Set<string>();
  return async (id, run) => {
    if (moving.has(id)) return false;
    moving.add(id);
    try {
      await run();
      return true;
    } finally {
      moving.delete(id);
    }
  };
}

/**
 * 순서: 확인(해당할 때) → 콘솔에 키 → 끝나기 대기 → 이어 열기. 어느 단계든 실패하면 알림 한 줄을 내고 멈춘다.
 * deps 가 던진 오류는 삼키지 않고 그대로 던진다(호출자가 알린다).
 */
export async function moveToBoard(s: Session, deps: MoveDeps): Promise<void> {
  if (needsConfirm(s) && !(await deps.confirm(CONFIRM_TITLE, `${s.title}\n\n${CONFIRM_WARNING}`))) return;
  const name = lastFolder(s.cwd);
  if (!(await deps.drive(s.id, MOVE_STEPS))) {
    return deps.toast(`${name}: 세션 터미널을 찾지 못해 보드로 옮기지 못했습니다`);
  }
  if (!(await deps.waitExit(s.pid, s.pidCtime ?? null, EXIT_TIMEOUT_MS))) {
    return deps.toast(`${name}: 세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요`);
  }
  deps.resumeSession(s);
}
