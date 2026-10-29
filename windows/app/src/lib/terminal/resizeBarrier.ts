// xterm 크기를 ConPTY 의 다시 그리기 머리말(cut)에 맞춰 바꾸는 상태 기계.
// Idle → Planning(xterm 쓰기 대기열을 비움) → Sending(termResize 의 기준 seq 를 기다림) → Scanning(스캐너에 출력을 먹임) → Holding(cut 앞을 쓰고 콜백에서 applyLocal)
// → 붙잡은 프레임을 풀고 → 다음 단계 또는 최신 요청으로 다시 계획 → Idle.
// 한 번에 단계 하나만 진행하고, 요청은 슬롯 하나(desired)에 합친다. 디바운스는 호출자(term.ts)의 몫이다.
// 붙잡은 프레임은 xterm 에 쓰기 전까지 done 을 부르지 않는다(생산자의 크레딧 창이 막아 준다).
// applyLocal 은 앞서 쓴 바이트가 모두 해석된 뒤에만 부른다(afterWrites). 콜백 안에서 예외가 나도 붙잡은 프레임을 풀고 Idle 로 돌아간다(run·recover).
// 이 파일은 resizeRules.ts·repaintScanner.ts 만 import 하고 타이머는 port 로 주입받아 node:test 로 바로 시험한다.

import { type Held, takePieces, WriteLedger } from "./barrierWrites.ts";
import { RepaintScanner } from "./repaintScanner.ts";
import { planSteps, type Size, type Step } from "./resizeRules.ts";

export interface BarrierPort {
  /** termResize — 적용했으면 ConPTY 크기 변경 직전의 스트림 위치, 적용 안 했으면 null */
  sendResize(size: Size): Promise<number | null>;
  /** xterm 크기 바꾸기 */
  applyLocal(size: Size): void;
  write(bytes: Uint8Array, done: () => void): void;
  /** 지금까지 write 한 바이트를 xterm 이 다 해석한 뒤 done 을 부른다. altScreen()·current() 는 그 뒤에만 믿을 수 있다 */
  flush(done: () => void): void;
  current(): Size;
  altScreen(): boolean;
  bound(): boolean;
  setTimer(fn: () => void, ms: number): unknown;
  clearTimer(handle: unknown): void;
  log(message: string): void;
}

export interface BarrierOptions {
  deadlineMs?: number;
  tailIdleMs?: number;
  scanBudget?: number;
  fallbackMaxMs?: number;
  fallbackMaxBytes?: number;
  retryMs?: number;
}

export const PREAMBLE_DEADLINE_MS = 1000;
export const TAIL_IDLE_MS = 50;
export const SCAN_BUDGET_BYTES = 64 * 1024;
export const FALLBACK_MAX_MS = 1500;
export const FALLBACK_MAX_BYTES = 256 * 1024;
export const RETRY_MS = 500;
const MAX_FAILURES = 3;
const CR_GRACE_FACTOR = 4;

type Phase = "idle" | "planning" | "sending" | "scanning" | "holding";

const NO_BYTES = new Uint8Array(0);

export class ResizeBarrier {
  private readonly deadlineMs: number;
  private readonly tailIdleMs: number;
  private readonly scanBudget: number;
  private readonly fallbackMaxMs: number;
  private readonly fallbackMaxBytes: number;
  private readonly retryMs: number;

  private phase: Phase = "idle";
  private desired: Size | null = null;
  private steps: Step[] = [];
  private stepIndex = 0;
  private minSeq: number | undefined;
  private token = 0; // 단계마다 올린다: 늦게 도착한 약속·콜백이 지난 단계를 건드리지 못하게
  private disposed = false;
  private readonly ledger: WriteLedger;
  private force = false; // 크기가 같아도 백엔드에 한 번 알린다(붙을 때)
  private requestGen = 0;
  private failures = 0;
  private retryTimer: unknown = null;
  private fallbackTimer: unknown = null;
  private fallbackBytes = 0;

  private scanner: RepaintScanner | null = null;
  private anchor = 0;
  private lastEnd: number | null = null; // 스캐너에 먹인 마지막 끝 seq
  private scannedBytes = 0;
  private fallback = false;
  private queue: Held[] = [];
  private deadlineTimer: unknown = null;
  private idleTimer: unknown = null;
  private crBlocked = false; // 줄 폴백이 CR 바로 뒤라 못 잘랐다: 새 바이트 없이 더 기다리면 강제로 자른다

  private readonly port: BarrierPort;

  constructor(port: BarrierPort, options: BarrierOptions = {}) {
    this.port = port;
    this.deadlineMs = options.deadlineMs ?? PREAMBLE_DEADLINE_MS;
    this.tailIdleMs = options.tailIdleMs ?? TAIL_IDLE_MS;
    this.ledger = new WriteLedger((bytes, done) => port.write(bytes, done), (done) => port.flush(done));
    this.scanBudget = options.scanBudget ?? SCAN_BUDGET_BYTES;
    this.fallbackMaxMs = options.fallbackMaxMs ?? FALLBACK_MAX_MS;
    this.fallbackMaxBytes = options.fallbackMaxBytes ?? FALLBACK_MAX_BYTES;
    this.retryMs = options.retryMs ?? RETRY_MS;
  }

  /** 원하는 크기. 진행 중이면 슬롯에 덮어쓰고, 끝난 뒤 한 번만 다시 계획한다 */
  request(desired: Size, options: { force?: boolean } = {}): void {
    if (this.disposed) return;
    this.clearRetry();
    if (!this.port.bound()) {
      // 붙기 전에는 보낼 곳이 없다: xterm 만 바꾸고 상태는 두지 않는다
      const now = this.port.current();
      if (now.cols !== desired.cols || now.rows !== desired.rows) this.port.applyLocal(desired);
      return;
    }
    this.requestGen++;
    this.desired = desired;
    if (options.force) this.force = true;
    if (this.phase === "idle") this.beginPlan();
  }

  /** 데이터 프레임 하나. 크기를 바꾸는 중이 아니면 그대로 쓴다 */
  output(seq: number, bytes: Uint8Array, done: () => void): void {
    if (this.disposed) return;
    if (bytes.length === 0) {
      done();
      return;
    }
    if (this.phase === "idle") {
      this.ledger.write(bytes, done);
      return;
    }
    this.queue.push({ seq, bytes, done });
    if (this.phase === "scanning") this.run(() => this.feedScanner(seq, bytes));
  }

  /** 스트림이 끊겼다(gap)·프로세스가 끝났다(exit): 기다림을 버리고 붙잡은 프레임을 지금 순서대로 쓴 뒤, 그 쓰기가 해석되면 계획한 크기를 적용한다.
   *  호출자는 이 호출 바로 뒤에 RIS·onExit 쓰기를 해도 된다: 붙잡은 프레임이 먼저 쓰인다 */
  interrupt(reason: "gap" | "exit"): void {
    if (this.disposed) return;
    if (reason === "exit") {
      this.desired = null;
      this.force = false;
      this.clearRetry();
    }
    if (this.phase === "idle") return;
    if (reason === "gap") this.minSeq = undefined;
    // 계획 중·cut 앞을 쓰는 중이면 콜백이 곧 적용하니 붙잡은 것만 먼저 쓴다
    if (this.phase === "planning" || this.phase === "holding") {
      this.run(() => this.drain(Infinity));
      return;
    }
    this.port.log(`resize barrier: ${reason} 로 기다림을 버리고 바로 적용`);
    const token = ++this.token;
    this.clearTimers();
    this.scanner = null;
    this.phase = "holding";
    this.run(() => {
      this.drain(Infinity);
      this.ledger.afterWrites(() => this.run(() => this.finishNow(token)));
    });
  }

  /** 새 셸로 바꿀 때: 진행 중인 단계를 버리고(늦은 응답은 무시) 붙잡은 프레임을 푼다 */
  reset(): void {
    if (this.disposed) return;
    this.token++;
    this.clearTimers();
    this.clearRetry();
    this.scanner = null;
    this.steps = [];
    this.desired = null;
    this.force = false;
    this.failures = 0;
    this.minSeq = undefined;
    this.run(() => this.drain(Infinity));
    this.phase = "idle";
  }

  /** 타이머를 걷고 이후 port 를 부르지 않는다 */
  dispose(): void {
    this.disposed = true;
    this.token++;
    this.clearTimers();
    this.clearRetry();
    this.scanner = null;
    this.queue = [];
    this.desired = null;
  }

  // ---- 계획 ----

  /** 계획 전에 xterm 쓰기 대기열을 비운다: altScreen()·current() 는 해석이 끝난 뒤의 값이어야 한다 */
  private beginPlan(): void {
    if (this.desired === null) return;
    const token = ++this.token;
    this.phase = "planning"; // 이 사이 들어온 프레임은 붙잡는다, 요청은 슬롯에 합친다
    this.port.flush(() => this.run(() => this.plan(token)));
  }

  private plan(token: number): void {
    if (token !== this.token || this.disposed) return;
    const want = this.desired;
    const force = this.force;
    this.desired = null;
    this.force = false;
    let steps = want === null || !this.port.bound() ? [] : planSteps(this.port.current(), want, this.port.altScreen());
    // 붙을 때: 같은 크기여도 백엔드가 한 번은 알게 한다(백엔드는 같은 크기면 null 을 돌려 장벽이 바로 끝낸다)
    if (force && want !== null && steps.length === 0 && this.port.bound()) {
      steps = [{ to: { cols: Math.max(2, want.cols), rows: Math.max(1, want.rows) }, cut: "A" }];
    }
    if (want !== null && !this.port.bound()) this.port.applyLocal(want);
    if (steps.length === 0) {
      this.drain(Infinity);
      this.phase = "idle";
      if (this.desired !== null) this.beginPlan();
      return;
    }
    this.steps = steps;
    this.stepIndex = 0;
    this.minSeq = undefined;
    this.sendStep();
  }

  private sendStep(): void {
    const step = this.steps[this.stepIndex];
    const token = ++this.token;
    if (!this.port.bound()) {
      this.phase = "holding";
      this.completeStep(token);
      return;
    }
    this.phase = "sending";
    this.scanner = null;
    this.lastEnd = null;
    this.scannedBytes = 0;
    this.fallback = false;
    this.crBlocked = false;
    let pending: Promise<number | null>;
    try {
      pending = this.port.sendResize(step.to);
    } catch (error) {
      pending = Promise.reject(error);
    }
    pending.then(
      (anchor) => this.run(() => this.onAnchor(token, anchor)),
      (error) => this.run(() => this.onRejected(token, error)),
    );
  }

  /** 백엔드가 크기를 못 바꿨다: xterm 은 옛 크기로 두고 단계를 버린다. 500ms 뒤 한 번 다시 시도하고 3번 연달아 실패하면 포기한다 */
  private onRejected(token: number, error: unknown): void {
    if (token !== this.token || this.disposed) return;
    const wanted = this.steps[this.steps.length - 1].to;
    this.failures++;
    this.port.log(`resize barrier: termResize 실패 ${this.failures}/${MAX_FAILURES}, 단계를 버린다 (${String(error)})`);
    this.token++;
    this.clearTimers();
    this.scanner = null;
    this.steps = [];
    this.drain(Infinity);
    this.phase = "idle";
    if (this.desired !== null) {
      this.beginPlan();
      return;
    }
    if (this.failures >= MAX_FAILURES) {
      this.port.log("resize barrier: termResize 3번 실패, 포기한다");
      this.failures = 0;
      return;
    }
    const gen = this.requestGen;
    this.retryTimer = this.port.setTimer(() => {
      this.retryTimer = null;
      this.run(() => {
        if (gen === this.requestGen && this.phase === "idle") this.request(wanted);
      });
    }, this.retryMs);
  }

  private onAnchor(token: number, anchor: number | null): void {
    if (token !== this.token || this.disposed) return;
    this.failures = 0;
    if (anchor === null) {
      this.applyNow(token); // 적용 안 됨(같은 크기·끝남): 기다릴 머리말이 없다
      return;
    }
    const step = this.steps[this.stepIndex];
    this.anchor = anchor;
    this.scanner = new RepaintScanner({
      target: step.to,
      cut: step.cut,
      alt: this.port.altScreen(),
      anchorSeq: anchor,
      minSeq: this.minSeq,
    });
    this.phase = "scanning";
    this.deadlineTimer = this.port.setTimer(() => this.run(() => this.onDeadline(token)), this.deadlineMs);
    for (const held of [...this.queue]) {
      if (this.phase !== "scanning" || token !== this.token) return;
      this.feedScanner(held.seq, held.bytes);
    }
  }

  private applyNow(token: number): void {
    this.phase = "holding";
    this.completeStep(token); // 기다릴 머리말이 없다: 바꾼 뒤 붙잡은 것을 푼다
  }

  // ---- 스캔 ----

  private feedScanner(seq: number, bytes: Uint8Array): void {
    const scanner = this.scanner as RepaintScanner;
    const end = seq + bytes.length;
    this.scannedBytes += Math.max(0, end - Math.max(seq, this.anchor));
    this.lastEnd = Math.max(this.lastEnd ?? end, end);
    if (this.fallback) this.fallbackBytes += bytes.length;
    const result = scanner.feed(seq, bytes);
    this.afterFeed(result);
  }

  private afterFeed(result: { safe: number; cut: number | null }): void {
    this.crBlocked = false;
    if (result.cut !== null) {
      this.holdAtCut(result.cut);
      return;
    }
    this.drain(result.safe);
    if (this.fallback && this.fallbackBytes >= this.fallbackMaxBytes) {
      // 줄바꿈 없이 계속 흐른다: 이 프레임 끝에서 자른다(CR 바로 뒤면 다음 프레임에서 LF 와 함께 자른다)
      const cut = (this.scanner as RepaintScanner).flushIdle(false);
      if (cut !== null) {
        this.port.log("resize barrier: 폴백 바이트 상한, 프레임 끝에서 자른다");
        this.holdAtCut(cut);
        return;
      }
    }
    if (!this.fallback && this.scannedBytes >= this.scanBudget) {
      this.enterFallback("64KiB 안에 머리말이 없다");
      return;
    }
    const heldTail = this.lastEnd !== null && result.safe < this.lastEnd;
    if (this.fallback || heldTail) this.armIdle();
    else this.clearIdle();
  }

  private onDeadline(token: number): void {
    this.deadlineTimer = null;
    if (token !== this.token || this.phase !== "scanning" || this.fallback) return;
    this.enterFallback(`머리말이 ${this.deadlineMs}ms 안에 안 왔다`);
  }

  /** 머리말을 포기하고 다음 LF 뒤에서 자른다 */
  private enterFallback(reason: string): void {
    this.port.log(`resize barrier: ${reason}, 줄 끝에서 자른다`);
    this.fallback = true;
    this.fallbackBytes = 0;
    const token = this.token;
    this.fallbackTimer = this.port.setTimer(() => this.run(() => this.onFallbackMax(token)), this.fallbackMaxMs);
    const scanner = this.scanner as RepaintScanner;
    scanner.lineFallback();
    if (this.lastEnd === null) {
      this.armIdle();
      return;
    }
    this.afterFeed(scanner.feed(this.lastEnd, NO_BYTES));
  }

  /** 폴백이 너무 오래 걸린다(CR 만 흐르는 진행 막대 등): 지금 끝에서 무조건 자른다 */
  private onFallbackMax(token: number): void {
    this.fallbackTimer = null;
    if (token !== this.token || this.phase !== "scanning") return;
    this.port.log(`resize barrier: 폴백이 ${this.fallbackMaxMs}ms 를 넘어 지금 끝에서 자른다`);
    this.holdAtCut((this.scanner as RepaintScanner).flushIdle(true) ?? Infinity);
  }

  private onTailIdle(token: number): void {
    this.idleTimer = null;
    if (token !== this.token || this.phase !== "scanning") return;
    const scanner = this.scanner as RepaintScanner;
    if (this.lastEnd === null) {
      if (this.fallback) this.holdAtCut(Infinity); // 아무것도 안 왔다: 바로 바꾼다
      return;
    }
    const cut = scanner.flushIdle(this.crBlocked);
    if (cut !== null) {
      this.holdAtCut(cut);
      return;
    }
    this.drain(scanner.safe);
    if (!this.fallback) return;
    // CR 바로 뒤라 못 잘랐다. 바이트가 계속 오는 동안은 CR-LF 를 가르지 않는다(afterFeed 가 crBlocked 를 푼다).
    // 새 바이트 없이 4 x tailIdleMs 가 더 지나면 CR 로 끝난 스트림이니 거기서 자르고 적용한다
    this.crBlocked = true;
    this.armIdle(this.tailIdleMs * CR_GRACE_FACTOR);
  }

  private armIdle(ms: number = this.tailIdleMs): void {
    this.clearIdle();
    const token = this.token;
    this.idleTimer = this.port.setTimer(() => this.run(() => this.onTailIdle(token)), ms);
  }

  private clearIdle(): void {
    if (this.idleTimer !== null) this.port.clearTimer(this.idleTimer);
    this.idleTimer = null;
  }

  private clearTimers(): void {
    this.clearIdle();
    if (this.deadlineTimer !== null) this.port.clearTimer(this.deadlineTimer);
    this.deadlineTimer = null;
    if (this.fallbackTimer !== null) this.port.clearTimer(this.fallbackTimer);
    this.fallbackTimer = null;
  }

  private clearRetry(): void {
    if (this.retryTimer !== null) this.port.clearTimer(this.retryTimer);
    this.retryTimer = null;
  }

  // ---- cut ----

  private holdAtCut(cut: number): void {
    const step = this.steps[this.stepIndex];
    const token = this.token;
    this.clearTimers();
    this.scanner = null;
    this.phase = "holding";
    if (Number.isFinite(cut)) this.minSeq = step.cut === "A" ? cut + 3 : cut;
    this.drain(cut, () => this.run(() => this.completeStep(token)));
  }

  /** cut 앞을 다 쓴 뒤: xterm 크기를 바꾸고, 붙잡은 프레임을 풀고, 다음 일로 넘어간다 */
  private completeStep(token: number): void {
    if (token !== this.token || this.disposed) return;
    this.port.applyLocal(this.steps[this.stepIndex].to);
    this.drain(Infinity);
    this.phase = "idle";
    this.stepIndex++;
    if (this.desired !== null) this.beginPlan();
    else if (this.stepIndex < this.steps.length) this.sendStep();
  }

  /** gap·exit 으로 기다림을 버렸다: 남은 단계는 접고 지금 단계의 크기만 적용한다 */
  private finishNow(token: number): void {
    if (token !== this.token || this.disposed) return;
    this.port.applyLocal(this.steps[this.stepIndex].to);
    this.drain(Infinity);
    this.phase = "idle";
    if (this.desired !== null) this.beginPlan();
  }

  /** 장벽 내부 콜백의 예외가 단계를 멈춰 세우지 못하게 한다 */
  private run(fn: () => void): void {
    try {
      fn();
    } catch (error) {
      this.recover(error);
    }
  }

  private recover(error: unknown): void {
    this.port.log(`resize barrier: 내부 오류, 붙잡은 프레임을 풀고 Idle 로 돌아간다 (${String(error)})`);
    this.token++;
    this.clearTimers();
    this.scanner = null;
    this.steps = [];
    try {
      this.drain(Infinity);
    } catch (second) {
      this.port.log(`resize barrier: 풀다가 또 실패 (${String(second)})`);
    }
    this.phase = "idle";
  }

  /** 큐 앞에서 limit(절대 seq) 아래만 순서대로 쓴다. 마지막 조각의 콜백에서 after 를 부른다.
   *  쓸 조각이 없어도 앞서 쓴 바이트가 다 해석된 뒤에 부른다 */
  private drain(limit: number, after?: () => void): void {
    const pieces = takePieces(this.queue, limit);
    pieces.forEach((piece, index) => {
      const isLast = index === pieces.length - 1;
      if (!(isLast && after)) {
        this.ledger.write(piece.bytes, piece.done);
        return;
      }
      this.ledger.write(piece.bytes, () => {
        piece.done();
        after();
      });
    });
    if (pieces.length === 0 && after) this.ledger.afterWrites(after);
  }
}
