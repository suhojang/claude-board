// WebGL 애드온(0.19.0)은 폐기할 때 커서 깜빡임 관리자를 풀지 않는다. 그 관리자의 setInterval 이 렌더러를 붙들고
// 렌더러가 xterm 전체(버퍼·DOM)를 붙들어, 탭을 열고 닫을 때마다 Terminal 하나가 샌다.
// 렌더러는 옵션이 바뀌면 깜빡임을 다시 정해 꺼져 있으면 관리자를 풀므로, 폐기 앞에서 깜빡임을 끈다.
// 이 파일은 외부를 import 하지 않아 node:test 로 바로 시험한다.

export interface BlinkTerm {
  options: { cursorBlink?: boolean };
  reset(): void;
}

/**
 * 애드온을 폐기하되 깜빡임 관리자가 남지 않게 한다.
 * `final` 은 터미널도 곧 폐기할 때다: 프로그램이 켠 DEC 모드 12(`ESC[?12h`)도 지운다 — 렌더러는 그것이 있으면 옵션을 무시한다.
 * 아니면(컨텍스트를 잃어 DOM 렌더러로 내려갈 때) 폐기 뒤 깜빡임 옵션을 되돌린다.
 */
export function disposeWebgl(term: BlinkTerm | null, addon: { dispose(): void }, final: boolean) {
  const blink = term?.options.cursorBlink;
  if (term && blink) {
    if (final) term.reset();
    term.options.cursorBlink = false;
  }
  try {
    addon.dispose();
  } finally {
    if (term && blink && !final) term.options.cursorBlink = true;
  }
}
