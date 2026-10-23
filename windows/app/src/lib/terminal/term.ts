// 터미널 한 칸의 컨트롤러 — xterm 한 개와 백엔드 터미널(TermId) 한 개를 이어 준다.
// 자기 host div 를 가지고 있어 칸을 다른 부모로 appendChild 로 옮겨도 화면과 출력이 그대로다(나중에 칸을 나눌 때 쓴다).
// xterm 은 이 칸이 처음 보일 때 동적 import 로 읽는다(알림 패널 번들에 들어가지 않게).
import { log, termAck, termAttach, termList, termResize, termWrite, termWriteBin } from "$lib/backend";
import type { TermFrame, TermId, TermInfo } from "$lib/types";
import { createImeKeys, type ImeKeys } from "./ime";
import { ReplayGate } from "./replayGate";
import { ResizeBarrier } from "./resizeBarrier";
import { disposeWebgl } from "./webglBlink";
// 형식만 가져오므로 번들에 들어가지 않는다
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import type { WebglAddon } from "@xterm/addon-webgl";

/** 백엔드가 보내는 ack 간격(계약: 16ms 또는 64KiB 마다 최대 한 번) */
const ACK_INTERVAL_MS = 16;
const ACK_BYTES = 64 * 1024;
/** 장벽에 크기를 요청하기 전 뒤쪽 디바운스 */
const RESIZE_DEBOUNCE_MS = 200;
/** ConPTY 가 줄 바꿈을 직접 다시 계산하는 첫 빌드(xterm windowsPty 규칙) */
const CONPTY_REFLOW_BUILD = 21376;

/** 터미널 테마 배경 — 고른 탭·현재 경로 항목·칸 바탕이 같은 색을 쓰도록 이 한 곳에서 정하고 TerminalAreaView 가 --term-bg 로 내보낸다 */
export const TERM_BG = "#212121";

const THEME = {
  background: TERM_BG,
  foreground: "#ebebf5",
  cursor: "#d97857",
  cursorAccent: TERM_BG,
  selectionBackground: "rgba(217, 120, 87, 0.35)",
};

export interface TermHooks {
  /** 셸이 끝났다(마지막 출력까지 화면에 쓴 뒤) */
  onExit(code: number | null): void;
  /** 프로그램이 터미널 제목을 바꿨다(OSC 0·2) */
  onTitle(title: string): void;
}

export class TermController {
  /** xterm 이 그려질 상자. 칸을 옮길 때는 이 요소를 통째로 옮긴다 */
  readonly host: HTMLDivElement;
  termId: TermId | null = null;
  /** 마지막으로 출력 조각이 도착한 시각(ms). 한 번도 없으면 0 — 새 세션 명령을 칠 때를 가늠한다 */
  lastOutputAt = 0;

  private term: Terminal | null = null;
  private fitAddon: FitAddon | null = null;
  private webgl: WebglAddon | null = null;
  private ime: ImeKeys | null = null;
  private opening: Promise<void> | null = null;
  private visible = false;
  private dragging = false;
  private fitFrame = 0;
  private fitDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  // xterm 크기는 장벽이 ConPTY 의 다시 그리기 머리말에서만 바꾼다(fitNow 는 요청만 한다)
  private readonly barrier = new ResizeBarrier({
    sendResize: async ({ cols, rows }) => (this.termId ? await termResize(this.termId, cols, rows) : null),
    applyLocal: ({ cols, rows }) => this.term?.resize(cols, rows),
    write: (bytes, done) => this.safeWrite(bytes, done),
    // xterm 의 쓰기 대기열은 비동기로 해석된다: 빈 쓰기의 콜백은 앞서 쓴 바이트를 다 해석한 뒤에 온다
    flush: (done) => (this.term ? this.safeWrite(new Uint8Array(0), done) : done()),
    current: () => ({ cols: this.term?.cols ?? 80, rows: this.term?.rows ?? 24 }),
    altScreen: () => this.term?.buffer.active.type === "alternate",
    bound: () => this.termId !== null && this.term !== null,
    setTimer: (fn, ms) => setTimeout(fn, ms),
    clearTimer: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
    log: (message) => log(message),
  });
  private disposed = false;
  private focusWhenOpen = false;

  // 다시 받기: 이 오프셋 앞의 출력은 되감아 받은 것이라, 그 사이 xterm 이 만든 자동 응답(onData)은 셸로 보내지 않는다.
  // 단 사용자 입력은 늘 통과한다 — 규칙은 replayGate.ts 머리말이 소유한다
  private replayEnd = 0;
  private readonly gate = new ReplayGate();
  private inputListeners: AbortController | null = null;
  private expectedSeq = 0;
  private ackedSeq = 0;
  private sentAckSeq = 0;
  private ackTimer: ReturnType<typeof setTimeout> | undefined;
  private waitingForSize: (() => void) | null = null;

  constructor(
    readonly leafId: string,
    private readonly hooks: TermHooks,
  ) {
    this.host = document.createElement("div");
    this.host.className = "term-host";
  }

  // ── 상태 알림(칸이 부른다) ────────────────────────────────

  /** 보이는지. 접힌 동안은 fit 도 하지 않는다 */
  setVisible(visible: boolean) {
    this.visible = visible;
    if (visible) this.notifyBox();
  }

  /** 경계를 끄는 동안은 fit 을 멈추고, 놓으면 한 번 맞춘다 */
  setDragging(dragging: boolean) {
    this.dragging = dragging;
    if (!dragging) this.requestFit();
  }

  /** 조합 중인 글자를 확정한다(칸이 숨겨져 조합이 취소되기 전에) */
  commitComposition() {
    this.ime?.commit();
  }

  /** 칸 크기가 바뀌었다 */
  notifyBox() {
    this.waitingForSize?.();
    this.requestFit();
  }

  /** 포커스를 준다. xterm 이 아직 안 열렸으면(처음 보이는 탭) 열리는 즉시 준다 */
  focus() {
    if (this.term) this.term.focus();
    else this.focusWhenOpen = true;
  }

  // ── 열기·붙이기 ───────────────────────────────────────────

  /** 칸이 보이고 1셀 이상 크기가 되면 xterm 을 열고 글자 크기를 잰다. 여러 번 불러도 한 번만 연다 */
  open(): Promise<void> {
    this.opening ??= this.openOnce().catch((e) => {
      this.opening = null;
      throw e;
    });
    return this.opening;
  }

  private hasBox() {
    return this.visible && this.host.clientWidth > 0 && this.host.clientHeight > 0;
  }

  private async openOnce() {
    const [{ Terminal }, { FitAddon }, { Unicode11Addon }] = await Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit"),
      import("@xterm/addon-unicode11"),
      import("@xterm/xterm/css/xterm.css"),
    ]);
    // 상자가 잡힌 뒤에 연다 — xterm 은 글자 크기를 DOM 으로 재므로 보이는 상태여야 한다
    while (!this.hasBox()) {
      if (this.disposed) return;
      await new Promise<void>((resolve) => (this.waitingForSize = resolve));
    }
    this.waitingForSize = null;
    const term = new Terminal({
      allowProposedApi: true,
      allowTransparency: false,
      scrollback: 5000,
      cursorBlink: true,
      fontSize: 14,
      fontFamily: "Cascadia Mono, Consolas, D2Coding, monospace",
      theme: THEME,
      windowsPty: { backend: "conpty" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.open(this.host);
    this.term = term;
    this.fitAddon = fit;
    this.loadWebgl(term);

    // 사용자 입력 이벤트 시각 — 캡처 단계라 xterm 이 onData 를 내기 전에 기록된다(조합은 compositionend 직후 비동기로 나온다)
    this.inputListeners = new AbortController();
    const noteInput = () => this.gate.noteUserInput(Date.now());
    for (const type of ["keydown", "paste", "compositionend", "input", "beforeinput"]) {
      term.textarea!.addEventListener(type, noteInput, { capture: true, signal: this.inputListeners.signal });
    }
    term.onData((data) => {
      if (this.termId && this.gate.allows(Date.now())) termWrite(this.termId, data);
    });
    term.onBinary((data) => {
      if (this.termId && this.gate.allows(Date.now())) termWriteBin(this.termId, Array.from(data, (ch) => ch.charCodeAt(0)));
    });
    term.onTitleChange((title) => this.hooks.onTitle(title));
    this.ime = createImeKeys(
      term.textarea!,
      (data) => this.termId && termWrite(this.termId, data),
      () => term.modes.applicationCursorKeysMode,
    );
    term.attachCustomKeyEventHandler((ev) => this.handleKey(ev));
    this.fitNow();
    if (this.focusWhenOpen) term.focus();
  }

  // WebGL 이 안 되거나(원격 데스크톱 등) 도중에 컨텍스트를 잃으면 DOM 렌더러로 간다
  private loadWebgl(term: Terminal) {
    import("@xterm/addon-webgl")
      .then(({ WebglAddon }) => {
        if (this.disposed) return;
        const addon = new WebglAddon();
        addon.onContextLoss(() => {
          disposeWebgl(this.term, addon, false);
          if (this.webgl === addon) this.webgl = null;
        });
        term.loadAddon(addon);
        this.webgl = addon;
      })
      .catch((e) => console.warn("WebGL 렌더러를 쓸 수 없어 기본 렌더러로 그립니다", e));
  }

  private handleKey(ev: KeyboardEvent): boolean {
    if (this.ime && !this.ime.filter(ev)) return false;
    // Shift+Enter 는 줄 바꿈 문자 — Claude Code 의 여러 줄 입력
    if (ev.code === "Enter" && ev.shiftKey && !ev.ctrlKey && !ev.altKey) {
      if (ev.type === "keydown") {
        ev.preventDefault();
        if (this.termId) termWrite(this.termId, "\n");
      }
      return false;
    }
    // 선택이 있을 때만 Ctrl+C 는 복사, 아니면 셸로(인터럽트)
    if (ev.code === "KeyC" && ev.ctrlKey && !ev.altKey && !ev.shiftKey && this.term?.hasSelection()) {
      if (ev.type === "keydown") {
        void navigator.clipboard.writeText(this.term.getSelection()).catch((e) => console.error("복사 실패", e));
        this.term.clearSelection();
      }
      return false;
    }
    // Ctrl+V 는 브라우저 붙여넣기(paste 이벤트)에 맡긴다
    if (ev.code === "KeyV" && ev.ctrlKey && !ev.altKey) return false;
    return true;
  }

  /** 지금 xterm 의 칸 수. 열기 전이면 null */
  dims(): { cols: number; rows: number } | null {
    return this.term ? { cols: this.term.cols, rows: this.term.rows } : null;
  }

  /** 백엔드 터미널에 붙는다. 이어서 받기라 출력이 겹치지 않는다 */
  async bind(info: TermInfo): Promise<void> {
    const term = this.term;
    if (!term) throw new Error("터미널을 열기 전에 붙을 수 없음");
    this.termId = info.id;
    if (info.windowsBuild != null) {
      term.options.windowsPty =
        info.windowsBuild >= CONPTY_REFLOW_BUILD
          ? { backend: "conpty", buildNumber: info.windowsBuild }
          : { backend: "conpty" };
    }
    // 붙이는 동안 도착하는 출력은 모두 되감아 받은 것으로 본다. 붙은 뒤 실제 끝 오프셋으로 좁힌다
    this.replayEnd = Number.POSITIVE_INFINITY;
    const attached = await termAttach(info.id, this.expectedSeq, (frame) => this.onFrame(frame));
    this.replayEnd = attached.nextSeq;
    // 이어 붙은(이미 도는) 터미널의 ConPTY 는 이 칸의 크기를 모른다: 같은 크기여도 한 번 알린다(백엔드가 같으면 null 을 돌려 장벽이 바로 끝낸다)
    this.barrier.request({ cols: term.cols, rows: term.rows }, { force: true });
    this.requestFit();
  }

  /** 새 셸로 바꿀 때 — 화면을 비우고 출력 오프셋을 처음으로 돌린다 */
  reset() {
    this.barrier.reset(); // 붙잡은 프레임을 먼저 풀어 낡은 단계가 새 셸에 크기를 적용하지 못하게 한다
    this.termId = null;
    this.term?.reset();
    this.expectedSeq = 0;
    this.ackedSeq = 0;
    this.sentAckSeq = 0;
    this.replayEnd = 0;
    clearTimeout(this.ackTimer);
  }

  // ── 출력 ──────────────────────────────────────────────────

  private onFrame(frame: TermFrame) {
    const term = this.term;
    if (!term || this.disposed) return;
    if (frame.kind === "gap") {
      // 링이 덮여 처음부터 다시 온다 — 장벽이 붙잡은 프레임을 먼저 쓴 다음 순서로 화면을 비우고(RIS) 다시 받은 것으로 센다
      this.barrier.interrupt("gap");
      this.safeWrite("\x1bc", () => {});
      this.expectedSeq = frame.seq;
      this.ackedSeq = frame.seq;
      this.sentAckSeq = frame.seq;
      this.replayEnd = Number.POSITIVE_INFINITY;
      this.refreshReplayEnd();
      return;
    }
    if (frame.kind === "exit") {
      // 붙잡은 프레임을 먼저 쓰고(interrupt), 앞서 쓴 출력이 모두 그려진 뒤에 알린다
      this.barrier.interrupt("exit");
      this.safeWrite("", () => this.hooks.onExit(frame.code));
      return;
    }
    this.lastOutputAt = Date.now();
    // 이미 받은 앞부분이 겹쳐 오면 자른다
    const skip = Math.max(0, this.expectedSeq - frame.seq);
    if (skip >= frame.bytes.length) return;
    const bytes = skip ? frame.bytes.subarray(skip) : frame.bytes;
    const start = frame.seq + skip;
    const end = start + bytes.length;
    this.expectedSeq = end;
    const replay = start < this.replayEnd;
    if (replay) this.gate.replayWriteStarted();
    // 크기를 바꾸는 중이면 장벽이 붙잡고, 쓴 뒤에야 done 이 불려 ack 도 그때 나간다
    this.barrier.output(start, bytes, () => {
      if (replay) this.gate.replayWriteFinished();
      this.acknowledge(end);
    });
  }

  // 쓰기 콜백이 던지면 xterm 의 쓰기 대기열이 멈추므로 막는다
  private safeWrite(data: Uint8Array | string, done: () => void) {
    this.term?.write(data, () => {
      try {
        done();
      } catch (e) {
        console.error("터미널 쓰기 콜백 실패", e);
      }
    });
  }

  private refreshReplayEnd() {
    const id = this.termId;
    if (!id) return;
    termList()
      .then((list) => {
        const info = list.find((t) => t.id === id);
        this.replayEnd = info ? info.nextSeq : 0;
      })
      .catch(() => (this.replayEnd = 0));
  }

  // 파싱을 끝낸 끝 오프셋을 16ms 또는 64KiB 마다 최대 한 번 알린다
  private acknowledge(seq: number) {
    this.ackedSeq = Math.max(this.ackedSeq, seq);
    if (this.ackedSeq - this.sentAckSeq >= ACK_BYTES) this.sendAck();
    else this.ackTimer ??= setTimeout(() => this.sendAck(), ACK_INTERVAL_MS);
  }

  private sendAck() {
    clearTimeout(this.ackTimer);
    this.ackTimer = undefined;
    if (!this.termId || this.ackedSeq === this.sentAckSeq) return;
    this.sentAckSeq = this.ackedSeq;
    termAck(this.termId, this.ackedSeq);
  }

  // ── 크기 ──────────────────────────────────────────────────

  /** 보인 뒤 requestAnimationFrame 에서 한 번만 맞춘다 */
  requestFit() {
    if (this.fitFrame || !this.term || !this.hasBox() || this.dragging) return;
    this.fitFrame = requestAnimationFrame(() => {
      this.fitFrame = 0;
      if (this.hasBox() && !this.dragging) this.fitNow();
    });
  }

  private fitNow() {
    const dims = this.fitAddon?.proposeDimensions();
    if (!dims || !Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return;
    const term = this.term;
    if (!term) return;
    // 붙기 전(첫 출력 전)에는 디바운스 없이 바로 맞춘다 — 장벽이 xterm 만 바꾼다
    if (!this.termId) {
      this.barrier.request({ cols: dims.cols, rows: dims.rows });
      return;
    }
    clearTimeout(this.fitDebounceTimer);
    this.fitDebounceTimer = setTimeout(() => this.barrier.request({ cols: dims.cols, rows: dims.rows }), RESIZE_DEBOUNCE_MS);
  }

  dispose() {
    this.disposed = true;
    cancelAnimationFrame(this.fitFrame);
    clearTimeout(this.fitDebounceTimer);
    this.barrier.dispose();
    clearTimeout(this.ackTimer);
    this.waitingForSize?.();
    this.inputListeners?.abort();
    this.ime?.dispose();
    if (this.webgl) disposeWebgl(this.term, this.webgl, true);
    this.webgl = null;
    this.term?.dispose();
    this.host.remove();
  }
}
