// 설정 > 계정의 로그인 진행 규칙 — 백엔드 이벤트(board://account-login)를 화면이 그릴 값으로 접는 순수 함수(mac `loginURL`·`loginNote`).
// 로그인이 도는지(adding)는 상태 묶음이 말한다. 여기는 이벤트로만 오는 주소와 안내 한 줄만 쥔다.
import type { AccountLoginPayload } from "../../types";

export interface LoginProgress {
  /** 로그인 명령이 알려 준 주소 — 아직 못 받았으면 null */
  url: string | null;
  /** 마지막 안내 한 줄(코드를 보냈다·로그인을 마쳤다) */
  note: string | null;
}

export const NO_PROGRESS: LoginProgress = Object.freeze({ url: null, note: null });

/** 이벤트 하나를 반영한 새 진행. done 이면 모두 비운다(실패 사유는 상태 묶음의 notice 로도 온다) */
export function reduceLogin(progress: LoginProgress, event: AccountLoginPayload): LoginProgress {
  if (event.done) return NO_PROGRESS;
  return { url: event.url ?? progress.url, note: event.note ?? progress.note };
}

/** 붙여 넣은 코드를 보낼 수 있다 — 앞뒤 공백을 뺀 글이 비어 있지 않다 */
export function canSubmitCode(code: string): boolean {
  return code.trim().length > 0;
}

/** 로그인 단계 화면이 쓸 글 */
export interface LoginSteps {
  hasUrl: boolean;
  /** 1단계(로그인 창에서 로그인) 설명 */
  openDetail: string;
  note: string | null;
}

export function loginSteps(progress: LoginProgress): LoginSteps {
  const hasUrl = progress.url !== null;
  return {
    hasUrl,
    openDetail: hasUrl
      ? "로그인을 마치면 저절로 추가되고 창이 닫힙니다. 창을 닫으면 계정 추가를 취소합니다. Google 로그인이 막히면 다른 브라우저에서 열고 코드를 받아 아래에 넣습니다"
      : "로그인 주소를 받는 중…",
    note: progress.note,
  };
}
