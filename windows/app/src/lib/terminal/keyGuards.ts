// 터미널 영역의 키 처리에서 쓰는 순수 판단 두 가지 — TerminalArea.svelte 가 부른다. 아무것도 import 하지 않아 node:test 로 바로 시험한다.

/** 입력기가 같은 키를 다시 쏘는 데 걸리는 시간의 상한(ms). 실측은 2ms 안쪽이다 */
const REFIRE_WINDOW_MS = 100;

export interface ChordKey {
  code: string;
  keyCode: number;
  isComposing: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  timeStamp: number;
}

const chordOf = (ev: ChordKey) => `${ev.code}|${ev.ctrlKey ? "c" : ""}${ev.altKey ? "a" : ""}${ev.shiftKey ? "s" : ""}`;

/**
 * 조합 중에 누른 단축키는 입력기가 조합을 끝내면서 같은 키를 한 번 더 쏜다(keyCode 229 한 번, 실제 keyCode 한 번).
 * 두 번 처리하면 탭이 두 칸 넘어가고(Ctrl+Alt+←/→), 새 세션 창이 뜬 뒤 다시 온 Ctrl+T 가 셸로 샌다.
 * 처리한 단축키가 조합 중이었으면 기억해 두었다가, 바로 이어 같은 키가 오면 한 번만 알아본다.
 */
export function createRefireGuard() {
  let last: { chord: string; at: number } | null = null;
  return {
    /** 단축키로 처리한 keydown 을 알린다 */
    handled(ev: ChordKey) {
      last = ev.keyCode === 229 || ev.isComposing ? { chord: chordOf(ev), at: ev.timeStamp } : null;
    },
    /** 이 keydown 이 방금 처리한 조합 중 단축키를 입력기가 다시 쏜 것인가. 한 번 알아보면 기억을 지운다 */
    isRefire(ev: ChordKey): boolean {
      const hit = last !== null && last.chord === chordOf(ev) && ev.timeStamp - last.at < REFIRE_WINDOW_MS;
      if (hit) last = null;
      return hit;
    },
  };
}

export type ConfirmKeyAction = "cancel" | "swallow" | "pass";

/**
 * 닫기 확인 창이 떠 있는 동안의 키. 포커스가 터미널에 남아 있어 그대로 두면 키가 뒤에서 도는 프로그램으로 간다
 * (취소하려고 누른 Esc 가 Claude 를 멈춘다). Esc 는 취소하고, 창 안(단추)의 키는 그대로 두고, 나머지는 삼킨다.
 */
export function confirmKeyAction(key: string, insideDialog: boolean): ConfirmKeyAction {
  if (key === "Escape") return "cancel";
  return insideDialog ? "pass" : "swallow";
}
