// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/terminal/sessionName.test.ts   (Node 22.18+/24 는 .ts 를 바로 읽는다)
import { test } from "node:test";
import assert from "node:assert/strict";
import { quoteSessionName, sanitizeSessionName } from "./sessionName.ts";

const SAFE = /^[\p{L}\p{N} _.-]+$/u;
const hostile = [
  "x“;calc;“", "x”;calc;”", "x„;calc;„", "a`b", "$(calc)", "%USERPROFILE%",
  "a&b", "a|b", "a\nb", "a\r\ncalc", 'a"b', "a'b", "a;b", "a<b>c", "x“;calc;“ y",
];

test("공격 입력은 허용 글자만 남는다", () => {
  for (const input of hostile) {
    const out = sanitizeSessionName(input);
    assert.match(out, SAFE, input);
    const quoted = quoteSessionName(out);
    assert.doesNotMatch(quoted.replace(/^"|"$/g, ""), /["“”„`$%&|;<>()\r\n']/, input);
  }
});

test("굽은 따옴표 주입", () => {
  assert.equal(sanitizeSessionName("x“;calc;“"), "x-calc");
});

test("한글·유니코드 글자는 그대로", () => {
  assert.equal(sanitizeSessionName("내 프로젝트"), "내 프로젝트");
  assert.equal(quoteSessionName("내 프로젝트"), '"내 프로젝트"');
  assert.equal(quoteSessionName(sanitizeSessionName("shop-api_2.0")), "shop-api_2.0");
});

test("비거나 전부 금지 글자면 session", () => {
  assert.equal(sanitizeSessionName(""), "session");
  assert.equal(sanitizeSessionName("   "), "session");
  assert.equal(sanitizeSessionName('"`$%'), "session");
  assert.equal(sanitizeSessionName("--"), "session");
});

test("앞뒤 -·점·공백을 떼어 플래그로 읽히지 않게", () => {
  assert.equal(sanitizeSessionName("--dangerous"), "dangerous");
  assert.equal(sanitizeSessionName(" a  b "), "a b");
});

test("정리 안 된 이름은 quote 가 거부", () => {
  assert.throws(() => quoteSessionName('x"y'));
  assert.throws(() => quoteSessionName("x“y"));
  assert.throws(() => quoteSessionName("-x"));
});
