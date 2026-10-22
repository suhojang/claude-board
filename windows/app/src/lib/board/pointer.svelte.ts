// 창 안 마우스 위치와 창 활성 여부 — 투명 모드(마우스가 올라오거나 활성이면 불투명)와 터미널 독(보드 칸 바깥쪽 끝에 대면 열림)이 함께 본다.
// 단추·줄마다 hover 를 따로 보면 경계를 지날 때 끊기므로 문서 전체에서 한 번만 추적한다
import { inTauri } from "$lib/backend";

// 터미널 독 크기(ClaudeBoard.swift:3503-3511) — 보드 칸 바깥쪽 끝에서 이 폭 안, 아래에서 독 높이 조금 위까지 마우스가 오면 연다
export const DOCK = { size: 36, gap: 8, pad: 8, trigger: 16, bottomInset: 34 };
export const dockHeight = (n: number) => n * (DOCK.size + DOCK.gap) - DOCK.gap + DOCK.pad * 2;

export const pointer = $state({ inside: false, atEdge: false });
export const win = $state({
  focused: true,
  // 투명도 슬라이더를 끄는 동안의 값과 미리보기(놓은 뒤 1.2초까지) — 그동안은 고른 투명도를 그대로 보여 준다
  dragging: false,
  dragLevel: 1,
  previewing: false,
});

// 보드 칸(터미널 칸을 펼치면 창의 한쪽일 뿐이다)과 그 바깥쪽 가장자리가 있는 쪽을 돌려준다
export type BoardEdge = () => { el: HTMLElement | undefined; side: "left" | "right" };

export function trackWindow(terminalCount: () => number, boardEdge: BoardEdge): () => void {
  win.focused = document.hasFocus();
  const move = (e: MouseEvent) => {
    pointer.inside = true;
    const triggerHeight = DOCK.bottomInset + dockHeight(terminalCount()) + 12;
    const { el, side } = boardEdge();
    if (!el) {
      pointer.atEdge = false;
      return;
    }
    const box = el.getBoundingClientRect();
    const fromEdge = side === "right" ? box.right - e.clientX : e.clientX - box.left;
    pointer.atEdge = fromEdge >= 0 && fromEdge <= DOCK.trigger && box.bottom - e.clientY <= triggerHeight;
  };
  const leave = () => {
    pointer.inside = false;
    pointer.atEdge = false;
  };
  const enter = () => (pointer.inside = true);
  const focus = () => (win.focused = true);
  const blur = () => (win.focused = false);
  window.addEventListener("mousemove", move, { passive: true });
  document.documentElement.addEventListener("mouseleave", leave);
  document.documentElement.addEventListener("mouseenter", enter);
  window.addEventListener("focus", focus);
  window.addEventListener("blur", blur);
  // Tauri 창은 웹뷰 밖에서 활성이 바뀔 수 있어 창 이벤트도 함께 본다
  let unlisten: (() => void) | undefined;
  if (inTauri) {
    import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
      unlisten = await getCurrentWindow().onFocusChanged((e) => (win.focused = e.payload));
    });
  }
  return () => {
    window.removeEventListener("mousemove", move);
    document.documentElement.removeEventListener("mouseleave", leave);
    document.documentElement.removeEventListener("mouseenter", enter);
    window.removeEventListener("focus", focus);
    window.removeEventListener("blur", blur);
    unlisten?.();
  };
}
