// 셸에 치는 --name 값 — 허용 목록 방식이다. 글자·숫자(유니코드 가능)·공백·_ . - 만 남기고 나머지는 모두 '-' 로 바꾼다.
// 폴더 이름은 통제할 수 없는 입력이다(곧은 따옴표뿐 아니라 “ ” „ 같은 굽은 따옴표도 PowerShell 은 큰따옴표로 읽는다).
// 그래서 금지 글자를 빼는 방식이 아니라 허용한 글자만 통과시킨다. 이 파일은 외부를 import 하지 않아 node:test 로 바로 시험한다.

const FALLBACK = "session";

/** 셸 명령줄에 넣어도 안전한 세션 이름. 비면 "session" */
export function sanitizeSessionName(raw: string): string {
  const cleaned = raw
    .replace(/[^\p{L}\p{N} _.-]/gu, "-")
    .replace(/ +/g, " ")
    .replace(/-{2,}/g, "-")
    .replace(/^[ \-.]+|[ \-.]+$/g, "");
  return cleaned || FALLBACK;
}

/** 이미 정리된 이름인가 — 정리 함수의 고정점이다(허용 글자·앞뒤 규칙의 유일한 정의가 sanitizeSessionName 이다) */
const isSanitized = (name: string): boolean => name !== "" && sanitizeSessionName(name) === name;

/** 이미 sanitizeSessionName 을 거친 이름만 받는다. 공백이 있으면 큰따옴표로 묶는다(허용 글자뿐이라 묶음 안에서 풀리는 것이 없다) */
export function quoteSessionName(sanitized: string): string {
  if (!isSanitized(sanitized)) throw new Error("정리되지 않은 세션 이름");
  return sanitized.includes(" ") ? `"${sanitized}"` : sanitized;
}

/** 명령줄에 들어가도 되는 `--name` 값인가 — sessionNameArg 의 결과 꼴(정리된 이름, 공백이 있으면 큰따옴표로 묶임)만 참 */
export function isSessionNameArg(arg: string): boolean {
  const quoted = arg.length >= 2 && arg.startsWith('"') && arg.endsWith('"');
  const inner = quoted ? arg.slice(1, -1) : arg;
  return isSanitized(inner) && (quoted || !inner.includes(" "));
}

/** 폴더·탭 글자 → 셸 명령줄에 그대로 넣을 수 있는 `--name` 값(정리 + 필요하면 큰따옴표) */
export const sessionNameArg = (raw: string): string => quoteSessionName(sanitizeSessionName(raw));
