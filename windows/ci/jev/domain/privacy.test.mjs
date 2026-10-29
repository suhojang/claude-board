import { test } from 'node:test';
import assert from 'node:assert/strict';
import { privacyBlock, privacyBlockAll } from './privacy.mjs';

test('사용자 이름은 대소문자와 상관없이 막는다 — 이름을 모르면(빈 값) 이 검사는 건너뛴다', () => {
  assert.equal(privacyBlock('the chip of SkVuD is visible', 'skvud'), 'user name');
  assert.equal(privacyBlock('the chip is visible', 'skvud'), null);
  assert.equal(privacyBlock('the chip is visible', ''), null);
  assert.equal(privacyBlock('the chip is visible'), null);
});

test('\\Users\\ 경로는 슬래시 방향·드라이브와 상관없이 막는다', () => {
  for (const text of ['C:\\Users\\someone\\x.png', 'D:/Users/someone/x.png', 'see \\USERS\\a', '/c/Users/a/b']) {
    assert.equal(privacyBlock(text), '\\Users\\ path', text);
  }
  assert.equal(privacyBlock('C:\\cbtest\\board.png'), null);
  assert.equal(privacyBlock('Users are listed in the sheet'), null);
});

test('이메일 주소를 막는다 — 골뱅이만 있는 글은 막지 않는다', () => {
  for (const text of ['a@b.co', 'owner.name+tag@example.com', 'mail: x_y@sub.domain.org shown']) {
    assert.equal(privacyBlock(text), 'e-mail address', text);
  }
  for (const text of ['@anthropic-ai/claude-code', 'chip "CB" at 10@2x', 'a @ b']) assert.equal(privacyBlock(text), null, text);
});

test('32자 이상 이어진 토큰 같은 글자를 막는다 — 31자까지와 긴 낱말 사이 공백은 통과', () => {
  assert.equal(privacyBlock('a'.repeat(32)), 'long token-like string');
  assert.equal(privacyBlock(`key ${'Ab1-_'.repeat(8)} end`), 'long token-like string');
  assert.equal(privacyBlock('a'.repeat(31)), null);
  assert.equal(privacyBlock(`${'a'.repeat(20)} ${'b'.repeat(20)}`), null);
});

test('까닭에는 걸린 글자를 담지 않는다', () => {
  const secret = 'sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ012345';
  const reason = privacyBlock(`the token is ${secret}`);
  assert.equal(reason, 'long token-like string');
  assert.ok(!reason.includes('sk-ant'));
  const mail = privacyBlockAll({ statement: 'owner@example.com is shown' });
  assert.equal(mail, 'e-mail address in statement');
});

test('privacyBlockAll: 처음 걸린 자리와 까닭, 다 깨끗하면 null', () => {
  assert.equal(privacyBlockAll({ a: 'fine', b: 'C:\\Users\\x', c: 'x@y.zz' }), '\\Users\\ path in b');
  assert.equal(privacyBlockAll({ a: 'fine', b: 'also fine' }, 'someone'), null);
  assert.equal(privacyBlockAll({ a: 'Someone was here' }, 'someone'), 'user name in a');
});
