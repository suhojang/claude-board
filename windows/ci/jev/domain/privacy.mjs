// Jev 로 보내는 글(진술·선택지·이름·자른 그림의 메타데이터)에 개인정보가 섞이지 않았는지 문자열로 막는다.
//
// 막는 것: Windows 사용자 이름, `\Users\` 경로, 이메일 주소, 길게 이어진 토큰 같은 글자(32자 이상의 영숫자·_-).
// 못 막는 것: 그림 픽셀 속 내용. 계정 칩의 이메일·사용량 패널·다른 창이 그림에 들어갔는지는 글자 검사로 알 수 없다.
//   그래서 호출하는 쪽이 보드 창 사각형으로 먼저 `crop` 하고(README), 자를 때 창 제목을 manifest 에 남겨
//   `allowedWindowTitle` 로 대조한다. 그래도 그 창 안에 실제 계정 이메일이 그려져 있으면 이 검사는 모른다.
// 순수 함수다 — 환경(사용자 이름)은 호출하는 쪽이 넘긴다.

const WINDOWS_USER_PATH = /[\\/]users[\\/]/i;
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+/;
const TOKEN_LIKE = /[A-Za-z0-9_-]{32,}/;

// 까닭에는 걸린 글자를 담지 않는다 — 토큰·이메일을 로그에 다시 적지 않으려는 것이다.
export function privacyBlock(text, userName = '') {
  const value = String(text ?? '');
  const name = String(userName ?? '').trim();
  if (name && value.toLowerCase().includes(name.toLowerCase())) return 'user name';
  if (WINDOWS_USER_PATH.test(value)) return '\\Users\\ path';
  if (EMAIL.test(value)) return 'e-mail address';
  if (TOKEN_LIKE.test(value)) return 'long token-like string';
  return null;
}

// 여러 글 중 처음 걸린 까닭과 그 글의 자리 이름. 걸린 것이 없으면 null.
export function privacyBlockAll(fields, userName = '') {
  for (const [where, text] of Object.entries(fields)) {
    const reason = privacyBlock(text, userName);
    if (reason) return `${reason} in ${where}`;
  }
  return null;
}
