// 다시 받는 출력(새로 고침·gap 뒤)을 xterm 이 파싱하는 동안 xterm 이 스스로 만드는 응답(DA·커서 위치 보고 등)은 셸로 보내지 않는다.
// 이미 한 번 응답한 질문을 되감아 읽는 것이기 때문이다. 그러나 사용자가 친 키는 언제나 셸로 간다.
//
// 규칙: 되감기 쓰기가 진행 중(pending > 0)이어도, 사용자 입력 이벤트(keydown·paste·compositionend·input)가
// USER_INPUT_WINDOW_MS 안에 있었으면 그 데이터는 통과시킨다. xterm 은 사용자 입력을 그 이벤트 처리 중에(조합은 직후에)
// 동기로 onData 로 내보내므로, 사용자 입력으로 생긴 데이터는 늘 최근 입력 이벤트 뒤에 온다.
// 한계: 키를 누른 직후 창 안에 같은 순간 파싱된 되감기 응답은 함께 통과할 수 있다(사용자 입력을 막는 것보다 낫다).
// 이 파일은 외부를 import 하지 않아 node:test 로 바로 시험한다.

/** 사용자 입력 이벤트 뒤 이 시간 안에 나온 데이터는 사용자 것으로 본다 */
export const USER_INPUT_WINDOW_MS = 300;

export class ReplayGate {
  private pending = 0;
  private lastUserInputAt = Number.NEGATIVE_INFINITY;

  /** 되감아 받은 조각을 xterm 에 쓰기 시작했다 */
  replayWriteStarted() {
    this.pending++;
  }

  /** 그 조각의 파싱이 끝났다(쓰기 콜백) */
  replayWriteFinished() {
    this.pending = Math.max(0, this.pending - 1);
  }

  /** 사용자 입력 이벤트(keydown·paste·compositionend·input)가 있었다 */
  noteUserInput(now: number) {
    this.lastUserInputAt = now;
  }

  /** xterm 이 낸 데이터를 셸로 보내도 되는가 */
  allows(now: number): boolean {
    return this.pending === 0 || now - this.lastUserInputAt < USER_INPUT_WINDOW_MS;
  }
}
