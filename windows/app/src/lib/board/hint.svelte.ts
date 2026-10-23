// 마우스를 올리면 곧 뜨는 짧은 설명(macOS HintPanel, ClaudeBoard.swift HintPanel).
// 시스템 툴팁은 오래 기다려야 뜨고 1초마다 다시 그리면 기다림이 처음부터 다시 시작돼 직접 띄운다.
// 0.35초 뒤에 뜨고, 방금(0.4초 안) 떠 있었으면 옆 단추로 옮겨도 바로 뜬다. 누르거나 스크롤하면 사라진다
const DELAY = 350;

type Entry = { id: number; text: string };

// 마우스가 들어가 있는 힌트 영역 — 안쪽 영역은 바깥 영역보다 나중에 들어가므로 마지막 것을 보인다
let stack: Entry[] = [];
let seq = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let lastShown = 0;
const mouse = { x: 0, y: 0 };

export const hintState = $state({ visible: false, text: "", x: 0, y: 0 });

function hide() {
  clearTimeout(timer);
  timer = undefined;
  if (hintState.visible) {
    hintState.visible = false;
    lastShown = performance.now();
  }
}

function schedule() {
  hide();
  const top = stack.at(-1);
  if (!top) return;
  const warm = performance.now() - lastShown < 400;
  timer = setTimeout(() => {
    hintState.text = top.text;
    hintState.x = mouse.x;
    hintState.y = mouse.y;
    hintState.visible = true;
  }, warm ? 0 : DELAY);
}

function remove(id: number) {
  const before = stack.length;
  stack = stack.filter((e) => e.id !== id);
  return stack.length !== before;
}

// 마우스가 창 밖으로 나갔다 — 빠르게 나가면 영역마다의 나감 신호를 놓칠 수 있다
export function leaveAll() {
  stack = [];
  hide();
}

let installed = false;
function install() {
  if (installed) return;
  installed = true;
  window.addEventListener("mousemove", (e) => {
    mouse.x = e.clientX;
    mouse.y = e.clientY;
  }, { capture: true, passive: true });
  for (const ev of ["mousedown", "wheel", "scroll", "contextmenu"]) window.addEventListener(ev, hide, { capture: true, passive: true });
  document.documentElement.addEventListener("mouseleave", leaveAll);
  window.addEventListener("blur", leaveAll);
}

// use:hint={"설명"} — 빈 문자열이면 아무것도 띄우지 않는다
export function hint(node: HTMLElement, text: string) {
  install();
  const id = ++seq;
  let current = text;
  const enter = (e: MouseEvent) => {
    mouse.x = e.clientX;
    mouse.y = e.clientY;
    remove(id);
    if (current) stack.push({ id, text: current });
    schedule();
  };
  // 마우스가 없던 영역이 사라질 때(폭에 맞춰 라벨을 바꾸는 등)는 떠 있는 힌트를 건드리지 않는다
  const leave = () => {
    if (remove(id)) schedule();
  };
  node.addEventListener("mouseenter", enter);
  node.addEventListener("mouseleave", leave);
  return {
    update(next: string) {
      current = next;
      const e = stack.find((x) => x.id === id);
      if (!e) return;
      e.text = next;
      if (hintState.visible && stack.at(-1) === e) hintState.text = next;
    },
    destroy() {
      node.removeEventListener("mouseenter", enter);
      node.removeEventListener("mouseleave", leave);
      leave();
    },
  };
}
