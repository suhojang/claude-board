// 터미널 크기 바꾸기 단계 계획의 순수 규칙. 외부를 import 하지 않아 node:test 로 바로 시험한다.
//
// ConPTY 는 크기를 한 번에 바꾸면 다시 그리기를 깨뜨린다. D1 게이트 실측(build 26300 의 수신 conhost,
// 도구: app/src-tauri/src/pty/capture.rs 캡처 + capture/loads.rs 부하 5종 = ascii·ko·ko-odd·tui·ink)에서
// 모양(방향)마다 캡처 5개를 돌려 "깨진 캡처 수/5" 가 0 인 방법을 골랐다. 표 주석의 실측 수는
// 모두 이 게이트의 깨진 캡처 수/5(괄호 안은 깨진 행 수 합계)다.
//
// 한 단계는 { 목표 크기, 컷 } 이고 컷은 RepaintScanner(repaintScanner.ts)가 찾는 자리다:
//   A = 다시 그리기 머리말 뒤 ESC[H 바로 앞
//   B = 머리말 뒤 첫 ESC[?25h 바로 뒤(예산을 넘기거나 없으면 다음 LF 뒤 폴백)
// 목표 크기는 열 2·행 1 아래로 내려가지 않게 못박힌다. 크기가 같으면 바꿀 단계가 없다(빈 배열).

export interface Size {
  cols: number;
  rows: number;
}
export type Cut = "A" | "B";

export interface Step {
  to: Size;
  cut: Cut;
}

/** 크기 못박음의 하한 — 게이트 실측에서도 ConPTY 는 이보다 작은 크기를 의미 있게 다루지 않는다 */
const MIN_COLS = 2;
const MIN_ROWS = 1;

/** 크기가 같은가 */
function sameSize(a: Size, b: Size): boolean {
  return a.cols === b.cols && a.rows === b.rows;
}

/** 목표를 열 2 × 행 1 아래로 찍지 못하게 못박는다(목표만 못박는다, 지금 크기는 실제 값인 그대로다) */
function clampedTarget(to: Size): Size {
  return { cols: Math.max(MIN_COLS, to.cols), rows: Math.max(MIN_ROWS, to.rows) };
}

/** 크기 음수·소수·NaN 은 지금 터뜨린다 — 조용히 말도 안 되는 단계를 계획하지 않는다 */
function requireInteger(size: Size, name: string): void {
  for (const field of ["cols", "rows"] as const) {
    if (!Number.isInteger(size[field])) throw new Error(`${name}.${field} 는 정수여야 한다: ${size[field]}`);
  }
}

/**
 * 지금 크기 `from` 을 `to` 로 크게 만드는 단계들을 고른다. 마지막 단계는 늘 못박힌 목표에서 끝난다.
 * `alt` 는 대체 화면(TUI)에서의 바꾸기다. 각 방향의 방법은 행 주석의 게이트 실측 근거를 따른다.
 */
export function planSteps(from: Size, to: Size, alt: boolean): Step[] {
  requireInteger(from, "from");
  requireInteger(to, "to");
  if (from.cols < 1 || from.rows < 1) throw new Error(`from 는 1x1 이상이어야 한다: ${from.cols}x${from.rows}`);
  if (sameSize(from, to)) return [];
  const target = clampedTarget(to);
  if (sameSize(from, target)) return [];
  const colsSign = Math.sign(target.cols - from.cols); // -1 줄임 · 0 같음 · +1 늘임
  const rowsSign = Math.sign(target.rows - from.rows);

  // 대체 화면 — 어떤 방향이든 한 번, A. 실측: tui 는 모든 모양·모든 컷에서 0/5, ConPTY 는 커서 숨김 뒤
  // ESC[?25h 를 늘 내지 않아(70/70 캡처) cut B 는 늘 예산 폴백(LF 자리)으로 간다 — A 하나로 충분하다.
  if (alt) return [{ to: target, cut: "A" }];

  // 열만 줄임 — 한 번, A. 실측 120x30→80x30: A 0/5(ascii·ko·ko-odd·tui·ink 모두 0), 반면 B 5/5(155),
  // C ascii 2/5(4), 오늘 방식 D(xterm 을 100ms 먼저 바꿈) ascii 4/5(4). 120x30→79x30(넓은 글자가 새 폭에 걸친 모양)
  // 에서도 A ko 0·ko-odd 0/5 — 열 줄임은 A 가 무조건이다.
  if (colsSign < 0 && rowsSign === 0) return [{ to: target, cut: "A" }];

  // 열만 늘임 — 한 번, B. 실측 80x30→120x30: B ko 0·ko-odd 0·tui 0·ink 0, ascii 1/5(1행) —
  // A 5/5(80), C 5/5(71-80), D 5/5(80-85): 늘임에서 머리말 ESC[H 앞 A 는 늘 깨진다.
  if (colsSign > 0 && rowsSign === 0) return [{ to: target, cut: "B" }];

  // 행만 바꿈(늘림·줄임 모두) — 한 번, A. 실측 120x30→120x40: A ascii 0·tui 0·ink 0(B ink 부하 5/5(85),
  // D ascii 5/5(50)) — 처음 설계 가설 B 는 틀렸다, A 가 맞다. 실측 120x40→120x30: 완전한 컷은 없다
  // (A ascii 3/5(30행, 깨진 캡처당 10행)·tui 0·ink 0 — B ascii 5/5, C ascii 5/5, D ascii 0/5 이지만 ink 5/5(135)).
  // 가장 덜 깨는 A 를 고른다(행 줄임만 최선 노력).
  if (colsSign === 0) return [{ to: target, cut: "A" }];

  // 열 줄고 행 늘림 — 두 단계, 열 먼저: (목표 열, 지금 행) A → (목표 열, 목표 행) A.
  // 실측 120x30→60x30→60x40(AA): 0/5 다섯 부하 모두(ascii·ko·ko-odd·tui·ink). 한 번에 바꾸면 완전한 컷이 없다
  // (A 5/5(25), B 5/5(100), C 5/5(29), D 4/5(4)). 둘째 단계(행만)는 크기 알림이 안 온다(실측 20/20) —
  // 스캐너가 ESC[H 로 찾아 주는 A 다.
  if (colsSign < 0 && rowsSign > 0) {
    return [
      { to: { cols: target.cols, rows: from.rows }, cut: "A" },
      { to: target, cut: "A" },
    ];
  }

  // 열 줄고 행 줄어 — 한 번, A(최선 노력). 실측 120x40→60x30: A ascii 5/5(50)·tui 0·ink 0(D ascii 5/5·ink 5/5(135)).
  // 두 단계도 더 낫지 않다(부하는 ascii 만): 열 먼저 AA 4/5(20), 행 먼저 AA 5/5(50) — 가장 잘 아는 것은 A 한 번.
  if (colsSign < 0) return [{ to: target, cut: "A" }];

  // 열 늘고 행 바뀜(늘림·줄림 모두) — 두 단계, 행 먼저: (지금 열, 목표 행) A → (목표 열, 목표 행) B.
  // 근거(G0 실측, 셀마다 캡처 5개): 60x30→120x40 은 행 먼저 AB 가 ascii·ko·tui·ink 모두 0/5 이고 한 단계 A 는 ascii·ko 5/5(80),
  // 한 단계 B 는 ink 5/5(85) 로 깨진다. 80x40→120x30 은 깨끗한 계획이 없어 같은 순서가 가장 덜 깨진다 — ascii 5/5(25), ko 4/5(20),
  // tui·ink 0/5 (최선 노력).
  if (colsSign > 0) {
    return [
      { to: { cols: from.cols, rows: target.rows }, cut: "A" },
      { to: target, cut: "B" },
    ];
  }

  throw new Error(`도달하지 않는 결정 표: colsSign ${colsSign} rowsSign ${rowsSign}`); // 방어 — 방향 표가 다 채워져 있으므로 여기에 오면 안 된다
}