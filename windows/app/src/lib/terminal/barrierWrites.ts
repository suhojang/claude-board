// ResizeBarrier 의 쓰기 보조 — 아직 콜백이 안 온 쓰기 수를 세고, 붙잡은 프레임 큐를 seq 한계에서 자른다.
// 외부를 import 하지 않아 node:test 로 바로 시험한다.

export interface Held {
  seq: number;
  bytes: Uint8Array;
  done: () => void;
}

const noop = () => {};

/** 큐 앞에서 limit(절대 seq) 아래만 꺼낸다. 한계에 걸친 프레임은 나눠서 앞쪽만 꺼내고(done 은 뒤쪽에 남긴다) 큐에 뒤쪽을 남긴다 */
export function takePieces(queue: Held[], limit: number): Held[] {
  const pieces: Held[] = [];
  while (queue.length > 0) {
    const head = queue[0];
    const end = head.seq + head.bytes.length;
    if (head.seq >= limit) break;
    if (end <= limit) {
      pieces.push(head);
      queue.shift();
      continue;
    }
    const n = limit - head.seq;
    pieces.push({ seq: head.seq, bytes: head.bytes.subarray(0, n), done: noop });
    queue[0] = { seq: limit, bytes: head.bytes.subarray(n), done: head.done };
    break;
  }
  return pieces;
}

/** port.write 를 부르고 콜백이 오기 전까지를 센다. 앞서 쓴 바이트가 다 해석되기 전엔 크기를 바꾸면 안 된다 */
export class WriteLedger {
  private outstanding = 0;

  private readonly rawWrite: (bytes: Uint8Array, done: () => void) => void;
  private readonly rawFlush: (done: () => void) => void;

  constructor(rawWrite: (bytes: Uint8Array, done: () => void) => void, rawFlush: (done: () => void) => void) {
    this.rawWrite = rawWrite;
    this.rawFlush = rawFlush;
  }

  write(bytes: Uint8Array, done: () => void): void {
    this.outstanding++;
    try {
      this.rawWrite(bytes, () => {
        this.outstanding--;
        done();
      });
    } catch (error) {
      this.outstanding--;
      throw error;
    }
  }

  /** 앞서 쓴 바이트를 xterm 이 모두 해석한 뒤 fn 을 부른다. 기다릴 쓰기가 없으면 바로 부른다 */
  afterWrites(fn: () => void): void {
    if (this.outstanding > 0) this.rawFlush(fn);
    else fn();
  }
}
