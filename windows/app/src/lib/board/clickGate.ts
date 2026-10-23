// 세션 줄 클릭 관문(w3 설계 D3) — 다른 터미널 줄의 한 번 클릭을 잠깐 미뤄 두 번째 클릭이 오면 activate 없이 focus 만 낸다.
// 타이머는 밖에서 주입받는다(setTimer·clearTimer) — 이 모듈은 시계를 모르므로 가짜 타이머로 시험한다.
export interface ClickGateOptions<Handle> {
  /** 0 이면 지연 없이 detail 1 에서 activate, detail 2 에서 focus. 0 보다 크면 그 시간 뒤에야 activate */
  delayMs: number;
  setTimer: (callback: () => void, ms: number) => Handle;
  clearTimer: (handle: Handle) => void;
  onActivate: () => void;
  onFocus: () => void;
}

export interface ClickGate {
  /** 눌림 하나 — detail 은 MouseEvent.detail(연속 클릭 횟수) */
  press(detail: number): void;
  /** 대기 중인 activate 를 취소한다(줄이 사라질 때) */
  dispose(): void;
}

export function createClickGate<Handle>(options: ClickGateOptions<Handle>): ClickGate {
  const { delayMs, setTimer, clearTimer, onActivate, onFocus } = options;
  let pending: { handle: Handle } | null = null;

  const cancelPending = (): void => {
    if (pending === null) return;
    clearTimer(pending.handle);
    pending = null;
  };

  return {
    press(detail: number): void {
      if (detail === 1) {
        if (delayMs <= 0) {
          onActivate();
          return;
        }
        cancelPending();
        const entry = { handle: undefined as unknown as Handle };
        entry.handle = setTimer(() => {
          if (pending !== entry) return;
          pending = null;
          onActivate();
        }, delayMs);
        pending = entry;
        return;
      }
      if (detail === 2) {
        cancelPending();
        onFocus();
      }
      // 세 번째 이후 눌림은 이미 focus 를 냈으므로 무시한다
    },
    dispose(): void {
      cancelPending();
    },
  };
}
