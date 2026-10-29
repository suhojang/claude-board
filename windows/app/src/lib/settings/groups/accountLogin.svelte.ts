// 계정 추가 로그인의 진행(주소·안내) — 이벤트로만 오므로 화면(설정 > 계정)이 닫혀 있는 동안에도 받아 둔다.
// 계정 칩(늘 떠 있는 보드 머리)과 설정 > 계정 화면이 `watchAccountLogin()` 으로 듣고, 규칙은 accountLogin.ts 가 쥔다.
import { onAccountLogin } from "$lib/backend";
import { NO_PROGRESS, reduceLogin, type LoginProgress } from "./accountLogin";

let progress = $state.raw<LoginProgress>(NO_PROGRESS);
let watchers = 0;
let stop: (() => void) | null = null;

export const accountLogin = {
  get url() {
    return progress.url;
  },
  get note() {
    return progress.note;
  },
  /** 새 로그인을 시작한다 — 앞 로그인의 주소·안내를 비운다 */
  reset() {
    progress = NO_PROGRESS;
  },
};

/** 이벤트 듣기를 시작하고 끄는 함수를 돌려준다. 여럿이 불러도 듣기는 하나이고, 마지막이 끄면 멈춘다 */
export function watchAccountLogin(): () => void {
  if (watchers++ === 0) stop = onAccountLogin((event) => (progress = reduceLogin(progress, event)));
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (--watchers === 0) {
      stop?.();
      stop = null;
    }
  };
}
