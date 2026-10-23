// 세션 줄을 눌렀을 때 무엇을 할지 정하는 순수 함수 — mac ClaudeBoard.swift openSession 의 switch 와 같은 표다(w3 설계 §2).
// Svelte·타이머·IPC 를 모른다. 호출자가 돌려받은 효과를 실제 동작(reveal·session_action·toast)으로 옮긴다.
import type { Session } from "../types.ts";
import type { SessionPlace } from "../terminal/place.ts";

/** 호출자가 실행할 효과 하나 */
export type OpenEffect =
  | { kind: "reveal"; surfaceId: string } // 보드 탭을 펼쳐 그 화면을 앞으로
  | { kind: "action"; action: "activate" | "focus" | "reopen"; carryOn?: boolean } // Rust session_action
  | { kind: "notice"; text: string } // 보드 안 알림(터미널이 없는 세션)
  | { kind: "none" };

/** 터미널 없이 도는 세션을 눌렀을 때 알리는 말 — 모드가 없으면 -p 로 본다 */
export function backgroundNotice(s: Pick<Session, "title" | "mode">): string {
  return `${s.title}: claude ${s.mode ?? "-p"} 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다.`;
}

/**
 * 클릭 한 번(double=false)·두 번(double=true)의 효과.
 * 끝난 세션의 두 번째 클릭은 reopen 을 다시 내지 않는다 — 첫 클릭 직후 place 가 tab 으로 바뀌어 reveal 로 간다(place.ts typedAt).
 */
export function openSession(
  s: Pick<Session, "title" | "mode" | "headless" | "paused">,
  place: SessionPlace,
  double: boolean,
): OpenEffect {
  switch (place.kind) {
    case "tab":
      return { kind: "reveal", surfaceId: place.surfaceId };
    case "terminal":
      return { kind: "action", action: double ? "focus" : "activate" };
    case "background":
      return { kind: "notice", text: backgroundNotice(s) };
    case "ended":
      // 끝난 headless(-p·--bg) 세션은 누르면 아무것도 하지 않는다 — 메뉴의 다시 열기로만 연다
      return s.headless ? { kind: "none" } : { kind: "action", action: "reopen", carryOn: s.paused };
  }
}
