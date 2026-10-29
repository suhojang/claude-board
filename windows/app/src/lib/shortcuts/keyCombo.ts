// 키 하나 — `KeyboardEvent.code`(자판 위치)와 수정키 넷. 글자가 아니라 코드로 맞추므로 한글 입력기에서도 같은 키다(mac `KeyCombo`).
// 아무것도 import 하지 않아 node:test 로 바로 시험한다.
//
// 저장 모양은 `[code, 수정키 비트]` — 비트는 Ctrl=1 · Alt=2 · Shift=4 · Win=8(백엔드는 모양만 본다).

export interface KeyCombo {
  code: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
}

/** 저장할 때 쓰는 수정키 비트 */
export const MOD = { ctrl: 1, alt: 2, shift: 4, win: 8 } as const;

/** 코드와 수정키 비트로 키를 만든다 — 표를 짧게 쓰려고 */
export function keyOf(code: string, mods = 0): KeyCombo {
  return { code, ctrl: (mods & MOD.ctrl) !== 0, alt: (mods & MOD.alt) !== 0, shift: (mods & MOD.shift) !== 0, win: (mods & MOD.win) !== 0 };
}

/** 키 이벤트에서 읽은 키 */
export function fromEvent(e: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): KeyCombo {
  return { code: e.code, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, win: e.metaKey };
}

/** 같은 키인가 — 둘 다 null(없음)이어도 같다 */
export function same(a: KeyCombo | null, b: KeyCombo | null): boolean {
  if (a === null || b === null) return a === b;
  return a.code === b.code && a.ctrl === b.ctrl && a.alt === b.alt && a.shift === b.shift && a.win === b.win;
}

const bitsOf = (c: KeyCombo) => (c.ctrl ? MOD.ctrl : 0) | (c.alt ? MOD.alt : 0) | (c.shift ? MOD.shift : 0) | (c.win ? MOD.win : 0);

/** 저장 모양 `[코드, 수정키 비트]` */
export function toStored(c: KeyCombo): [string, number] {
  return [c.code, bitsOf(c)];
}

/** 저장 모양에서 키를. 비움([])이나 깨진 값은 null */
export function fromStored(v: readonly unknown[] | null | undefined): KeyCombo | null {
  if (!Array.isArray(v) || v.length !== 2) return null;
  const [code, mods] = v;
  if (typeof code !== "string" || code === "" || typeof mods !== "number" || !Number.isInteger(mods) || mods < 0) return null;
  return keyOf(code, mods & 15);
}

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight"]);

/** 수정키 자체의 코드 — 이 키만 눌린 keydown 은 아직 키가 정해진 것이 아니다 */
export function isModifierCode(code: string): boolean {
  return MODIFIER_CODES.has(code);
}

const NAMED: Record<string, string> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Escape: "Esc",
  Backspace: "Backspace",
  Delete: "Del",
  Insert: "Ins",
  PageUp: "PgUp",
  PageDown: "PgDn",
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
  NumpadAdd: "Num +",
  NumpadSubtract: "Num -",
  NumpadMultiply: "Num *",
  NumpadDivide: "Num /",
  NumpadDecimal: "Num .",
  NumpadEnter: "Num Enter",
};

function keyName(code: string): string {
  const named = NAMED[code];
  if (named) return named;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^Numpad[0-9]$/.test(code)) return `Num ${code.slice(6)}`;
  return code; // Tab·Enter·Space·Home·End·F1~F12 는 코드 이름이 곧 화면 글자
}

/** 화면에 보이는 키 — "Ctrl+Alt+B" */
export function label(c: KeyCombo): string {
  const parts: string[] = [];
  if (c.ctrl) parts.push("Ctrl");
  if (c.alt) parts.push("Alt");
  if (c.shift) parts.push("Shift");
  if (c.win) parts.push("Win");
  parts.push(keyName(c.code));
  return parts.join("+");
}
