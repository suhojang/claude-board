// @ts-nocheck — node:test 로만 돌고 svelte-check 대상이 아니다
// 실행: cd windows/app && node --test src/lib/settings/groups/accountLogin.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";

import { NO_PROGRESS, canSubmitCode, loginSteps, reduceLogin } from "./accountLogin.ts";

const URL = "https://login.fake.test/oauth/authorize?client_id=fake&state=abc";

test("주소가 오면 담고, 안내는 따로 쌓인다", () => {
  const withUrl = reduceLogin(NO_PROGRESS, { url: URL });
  assert.deepEqual(withUrl, { url: URL, note: null });
  const withNote = reduceLogin(withUrl, { note: "코드를 보냈습니다. 확인하는 중…" });
  assert.deepEqual(withNote, { url: URL, note: "코드를 보냈습니다. 확인하는 중…" });
  // 안내가 바뀌어도 주소는 그대로
  assert.equal(reduceLogin(withNote, { note: "로그인을 마쳤습니다. 계정 정보를 확인하는 중…" }).url, URL);
});

test("끝났다는 소식이면 주소·안내를 모두 비운다(오류가 함께 와도)", () => {
  const running = { url: URL, note: "n" };
  assert.deepEqual(reduceLogin(running, { done: true }), NO_PROGRESS);
  assert.deepEqual(reduceLogin(running, { done: true, error: "계정을 추가하지 못했습니다: x" }), NO_PROGRESS);
});

test("빈 소식은 아무것도 바꾸지 않고, 받은 상태를 고치지 않는다", () => {
  const running = Object.freeze({ url: URL, note: null });
  assert.deepEqual(reduceLogin(running, {}), running);
  assert.notEqual(reduceLogin(running, { note: "x" }), running);
  assert.deepEqual(running, { url: URL, note: null });
});

test("코드는 앞뒤 공백을 빼고 비어 있지 않아야 보낼 수 있다", () => {
  for (const [code, ok] of [["", false], ["   ", false], ["\t\n", false], ["abc", true], ["  abc  ", true]]) {
    assert.equal(canSubmitCode(code), ok, JSON.stringify(code));
  }
});

test("로그인 단계 안내: 주소가 오기 전·후와 안내가 있을 때", () => {
  const waiting = loginSteps(NO_PROGRESS);
  assert.equal(waiting.hasUrl, false);
  assert.match(waiting.openDetail, /받는 중/);
  const ready = loginSteps({ url: URL, note: null });
  assert.equal(ready.hasUrl, true);
  assert.doesNotMatch(ready.openDetail, /받는 중/);
  assert.equal(ready.note, null);
  assert.equal(loginSteps({ url: URL, note: "코드를 보냈습니다. 확인하는 중…" }).note, "코드를 보냈습니다. 확인하는 중…");
});
