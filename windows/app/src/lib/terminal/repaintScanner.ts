// 크기를 바꾼 뒤 ConPTY 가 내는 "다시 그리기 머리말"을 출력 스트림에서 찾아 xterm 크기를 바꿀 자리(cut)를 정한다.
// 머리말: (ESC[?25l)? (ESC[8;R;Ct)? (ESC[<n>;1H)? LF? ESC[H  — 전체 길이가 3 초과여야 후보다.
// 크기 알림이 있으면 (R,C)가 목표 크기와 같아야 하고, 다르면 그 후보를 버리고 계속 찾는다.
// 머리말 없는 맨 ESC[H 는 대체 화면에서만, 기준(anchorSeq-64)에서 4 KiB 안일 때만 받는다.
// cut A = 받아들인 ESC[H 바로 앞. cut B = A 뒤 첫 ESC[?25h 바로 뒤(예산 안), 예산을 넘기면 그 뒤 첫 LF 다음.
// 스트리밍: 이미 내보낸 바이트 ≤ 40 개만 되돌아 읽는다. 붙잡는 것은 ESC[H(A) 또는 ESC[?25h(B)의 진짜 앞부분뿐이다.
// 기준 이전(seq < anchorSeq-64) 바이트는 어떤 후보에도 쓰지 않는다. 프레임은 seq 순서대로 이어서 들어온다고 본다.
// 이 파일은 외부를 import 하지 않아 node:test 로 바로 시험한다.

export interface Size {
  cols: number;
  rows: number;
}
export type Cut = "A" | "B";

/** 되돌아 읽는 최대 바이트: ?25l(6) + 알림(≤16) + CUP(≤10) + LF(1) + ESC[H(3) 보다 넉넉히 */
const LOOKBEHIND = 40;
/** 맨 ESC[H 를 받는 거리(기준 시작에서) */
const BARE_WINDOW = 4096;
const ANCHOR_BACK = 64;

const ESC = 0x1b;
const LBRACKET = 0x5b;
const H = 0x48;
const LF = 0x0a;
const CR = 0x0d;
const SHOW_CURSOR = [0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x68]; // ESC[?25h

// 머리말 뒤쪽 끝(ESC[H 바로 앞)에 맞는 꼴. 가장 왼쪽에서 시작하는 일치 = 가장 긴 머리말.
const PREAMBLE = /(?:\x1b\[\?25l)?(\x1b\[8;(\d+);(\d+)t)?(?:\x1b\[\d{1,5};1H)?\n?$/;

type Mode = "scan" | "b" | "line" | "done";

export class RepaintScanner {
  private readonly target: Size;
  private readonly cutKind: Cut;
  private readonly alt: boolean;
  private readonly lower: number;
  private readonly bareFrom: number; // 맨 ESC[H 거리의 시작(minSeq 와 무관하게 anchorSeq-64)
  private readonly budget: number;

  private mode: Mode = "scan";
  private buf = new Uint8Array(0);
  private base = 0; // buf[0] 의 절대 seq
  private end = -1; // 지금까지 받은 끝(다음에 올 seq). -1 = 아직 없음
  private scanned: number; // 다음에 ESC[H 를 찾을 시작 위치
  private anchorA = -1;
  private bScan = 0; // ESC[?25h 를 찾을 시작 위치
  private released = 0; // 지금까지 써도 된다고 알린 위치(safe 의 최댓값)
  private lineScan = 0;
  private cutAt: number | null = null;
  private held = 0;

  /** minSeq: 이 seq 아래 바이트는 후보로 쓰지 않는다(두 단계 계획에서 앞 단계의 ESC[H 를 건너뛰는 데 쓴다: 앞 cut + 3) */
  constructor(o: { target: Size; cut: Cut; alt: boolean; anchorSeq: number; minSeq?: number }) {
    this.target = o.target;
    this.cutKind = o.cut;
    this.alt = o.alt;
    this.bareFrom = o.anchorSeq - ANCHOR_BACK;
    this.lower = Math.max(this.bareFrom, o.minSeq ?? this.bareFrom);
    this.scanned = this.lower;
    this.budget = o.target.rows * (o.target.cols * 4 + 16) + 256;
  }

  /** 지금 써도 되는 위치(잘림이 정해졌으면 cut). 현재까지의 반환값과 같다 */
  get safe(): number {
    return this.cutAt ?? this.end - this.held;
  }

  /** 받은 프레임을 먹이고, 지금 xterm 에 써도 되는 절대 seq 와 찾은 cut 을 돌려준다 */
  feed(seq: number, bytes: Uint8Array): { safe: number; cut: number | null } {
    if (this.mode === "done") return { safe: this.cutAt as number, cut: this.cutAt };
    if (this.end < 0) {
      this.end = seq;
      this.base = seq;
      this.released = seq;
    }
    this.append(seq, bytes);
    this.step();
    return this.result();
  }

  /** 프레임이 한동안 안 온다. 붙잡은 앞부분이 아무것도 못 이루면 풀거나, 지금 끝에서 자를 자리를 돌려준다.
   *  force 면 CR 바로 뒤여도 자른다(스트림이 CR 에서 끝난 경우) */
  flushIdle(force = false): number | null {
    if (this.mode === "done") return this.cutAt;
    if (this.end < 0) return null;
    if (this.mode === "line") return this.cutHere(force);
    if (this.mode === "b") return this.held > 0 ? this.cutHere(force) : null;
    this.held = 0; // A 를 찾는 중: 붙잡은 ESC / ESC[ 는 그냥 데이터로 푼다
    this.released = Math.max(this.released, this.end);
    return null;
  }

  /** 머리말을 포기하고 "다음 LF 바로 뒤"에서 자른다 */
  lineFallback(): void {
    if (this.mode === "done" || this.mode === "line") return;
    this.enterLine(Math.max(this.released, this.anchorA >= 0 ? this.anchorA + 3 : 0));
    if (this.end >= 0) this.step();
  }

  // ---- 내부 ----

  private result(): { safe: number; cut: number | null } {
    const safe = this.safe;
    if (this.cutAt === null) this.released = Math.max(this.released, safe);
    return { safe, cut: this.cutAt };
  }

  private cutHere(force: boolean): number | null {
    // 바이트가 계속 흐르는 동안은 CR 과 LF 사이를 자르지 않는다. force 는 호출자(ResizeBarrier)가
    // CR 뒤로 오래 아무것도 안 온 뒤에만 준다: 그만큼 조용하면 LF 는 같은 출력 덩어리에 이어지지 않는다
    if (!force && this.end > this.base && this.buf[this.end - 1 - this.base] === CR) return null;
    this.finish(this.end);
    return this.cutAt;
  }

  private finish(at: number) {
    this.cutAt = at;
    this.mode = "done";
    this.buf = new Uint8Array(0);
    this.held = 0;
  }

  private enterLine(from: number) {
    this.mode = "line";
    this.lineScan = from;
    this.held = 0;
  }

  private append(seq: number, bytes: Uint8Array) {
    if (seq > this.end) {
      // 빈 구간(정상이면 없다). 되돌아 읽은 바이트를 믿을 수 없으니 버린다
      this.buf = new Uint8Array(0);
      this.base = seq;
      this.end = seq;
      this.scanned = Math.max(this.scanned, seq);
    }
    const skip = this.end - seq;
    if (skip >= bytes.length) return;
    const fresh = skip > 0 ? bytes.subarray(skip) : bytes;
    const start = this.end;
    this.end = start + fresh.length;
    // 기준 이전 바이트는 보관하지 않는다
    const keepFrom = Math.max(start, this.lower);
    if (keepFrom >= this.end) {
      this.dropBefore(this.end);
      return;
    }
    const part = fresh.subarray(keepFrom - start);
    const merged = new Uint8Array(this.buf.length + part.length);
    const oldKeep = this.buf;
    merged.set(oldKeep, 0);
    merged.set(part, oldKeep.length);
    if (oldKeep.length === 0) this.base = keepFrom;
    this.buf = merged;
  }

  private dropBefore(seq: number) {
    if (seq <= this.base) return;
    const n = Math.min(seq - this.base, this.buf.length);
    this.buf = this.buf.subarray(n);
    this.base += n;
    if (this.buf.length === 0) this.base = Math.max(this.base, seq);
  }

  private at(seq: number): number {
    return this.buf[seq - this.base];
  }

  private step() {
    this.held = 0;
    if (this.mode === "scan") this.findA();
    if (this.mode === "b") this.findB();
    if (this.mode === "line") this.findLine();
    if (this.mode === "scan") this.trimScan();
    else if (this.mode === "b") this.dropBefore(Math.max(this.base, this.end - SHOW_CURSOR.length));
    else if (this.mode === "line") this.dropBefore(this.end - 1); // 마지막 바이트(CR 여부)는 남긴다
  }

  private trimScan() {
    this.dropBefore(Math.max(this.lower, this.scanned - LOOKBEHIND));
  }

  private findA() {
    for (let k = Math.max(this.scanned, this.base); k + 3 <= this.end; k++) {
      if (this.at(k) !== ESC || this.at(k + 1) !== LBRACKET || this.at(k + 2) !== H) continue;
      const from = Math.max(this.lower, k - LOOKBEHIND, this.base);
      let behind = "";
      for (let i = from; i < k; i++) behind += String.fromCharCode(this.at(i));
      const m = PREAMBLE.exec(behind) as RegExpExecArray;
      if (m[0].length > 0) {
        if (m[1] && (Number(m[2]) !== this.target.rows || Number(m[3]) !== this.target.cols)) continue;
        this.accept(k);
        return;
      }
      if (this.alt && k - this.bareFrom < BARE_WINDOW) {
        this.accept(k);
        return;
      }
    }
    this.scanned = Math.max(this.scanned, this.end - 2);
    // ESC[H 의 진짜 앞부분(ESC, ESC[)이 꼬리에 있으면 다음 프레임까지 붙잡는다
    const n = this.end - this.base;
    if (n >= 1 && this.end - 1 >= this.lower && this.at(this.end - 1) === ESC) this.held = 1;
    else if (n >= 2 && this.end - 2 >= this.lower && this.at(this.end - 2) === ESC && this.at(this.end - 1) === LBRACKET) this.held = 2;
  }

  private accept(k: number) {
    if (this.cutKind === "A") {
      this.finish(k);
      return;
    }
    this.anchorA = k;
    this.mode = "b";
    this.bScan = k + 3;
  }

  private findB() {
    const a = this.anchorA;
    const n = SHOW_CURSOR.length;
    for (let k = Math.max(this.bScan, this.base); k + n <= this.end; k++) {
      let ok = true;
      for (let i = 0; i < n; i++) {
        if (this.at(k + i) !== SHOW_CURSOR[i]) {
          ok = false;
          break;
        }
      }
      if (!ok) continue;
      if (k + n - a <= this.budget) {
        this.finish(k + n);
        return;
      }
      this.fallbackToLine();
      return;
    }
    this.bScan = Math.max(this.bScan, this.end - (n - 1));
    if (this.end - a > this.budget) {
      this.fallbackToLine();
      return;
    }
    // ESC[?25h 의 진짜 앞부분이 꼬리에 있으면 붙잡는다
    for (let m = Math.min(n - 1, this.end - (a + 3)); m >= 1; m--) {
      let ok = true;
      for (let i = 0; i < m; i++) {
        if (this.at(this.end - m + i) !== SHOW_CURSOR[i]) {
          ok = false;
          break;
        }
      }
      if (ok) {
        this.held = m;
        break;
      }
    }
  }

  /** 예산을 넘겼다: 아직 내보내지 않은 바이트에서 다음 LF 뒤로 */
  private fallbackToLine() {
    this.enterLine(Math.max(this.released, this.anchorA + 3));
    this.findLine();
  }

  private findLine() {
    for (let k = Math.max(this.lineScan, this.base); k < this.end; k++) {
      if (this.at(k) === LF) {
        this.finish(k + 1);
        return;
      }
    }
    this.lineScan = Math.max(this.lineScan, this.end);
  }
}
