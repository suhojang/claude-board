import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarize, trustProblems, wilsonLower } from './stats.mjs';

const rep = (n, truth, status) => Array.from({ length: n }, () => ({ truth, status }));

test('wilsonLower: 알려진 값과 경계', () => {
  assert.equal(wilsonLower(0, 0), 0);
  assert.ok(Math.abs(wilsonLower(30, 30) - 0.8865) < 0.001); // 30/30 의 95% Wilson 하한
  assert.ok(Math.abs(wilsonLower(50, 100) - 0.4038) < 0.001);
  assert.ok(wilsonLower(60, 60) > wilsonLower(30, 30));
});

test('summarize: 맞힘·불확실·거짓 통과를 센다', () => {
  const s = summarize([...rep(8, 'yes', 'PASS'), ...rep(1, 'yes', 'FAIL'), ...rep(1, 'yes', 'UNSURE'), ...rep(7, 'no', 'FAIL'), ...rep(2, 'no', 'PASS'), ...rep(1, 'no', 'UNSURE')]);
  assert.equal(s.positives, 10);
  assert.equal(s.negatives, 10);
  assert.equal(s.decided, 18);
  assert.equal(s.correct, 15);
  assert.equal(s.falsePass, 2);
  assert.equal(s.falsePassRate, 0.2);
  assert.equal(s.abstainRate, 0.1);
  assert.equal(s.hitRate, 0.75);
  assert.ok(Math.abs(s.decidedAccuracy - 15 / 18) < 1e-9);
});

test('summarize: UNVERIFIED 나 잘못된 truth 는 세지 않고 던진다', () => {
  assert.throws(() => summarize([{ truth: 'yes', status: 'UNVERIFIED' }]), TypeError);
  assert.throws(() => summarize([{ truth: 'maybe', status: 'PASS' }]), TypeError);
});

test('trustProblems: 문턱을 모두 넘어야 비어 있다', () => {
  const good = summarize([...rep(30, 'yes', 'PASS'), ...rep(30, 'no', 'FAIL'), ...rep(10, 'yes', 'PASS')]);
  // 40 양성 + 30 음성, 전부 맞힘: Wilson 하한(70/70) 약 0.948
  assert.deepEqual(trustProblems(good), []);
  const small = summarize([...rep(29, 'yes', 'PASS'), ...rep(30, 'no', 'FAIL')]);
  assert.match(trustProblems(small).join(), /positives 29<30/);
  const leaky = summarize([...rep(40, 'yes', 'PASS'), ...rep(28, 'no', 'FAIL'), ...rep(2, 'no', 'PASS')]);
  assert.match(trustProblems(leaky).join(), /falsePassRate/);
  const shy = summarize([...rep(20, 'yes', 'PASS'), ...rep(30, 'yes', 'UNSURE'), ...rep(30, 'no', 'FAIL')]);
  assert.match(trustProblems(shy).join(), /abstainRate/);
});
