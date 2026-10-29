import { test } from 'node:test';
import assert from 'node:assert/strict';
import { agreement, aggregate } from './report.mjs';

const v = (status, reason = '') => ({ status, reason });

test('일치 표: 결정적 판정이 기준이고 Jev UNVERIFIED 는 일치가 아니다', () => {
  const table = [
    ['PASS', 'PASS', 'agree'],
    ['FAIL', 'FAIL', 'agree'],
    ['PASS', 'FAIL', 'DISAGREE'],
    ['PASS', 'UNSURE', 'DISAGREE'],
    ['FAIL', 'PASS', 'DISAGREE'],
    ['FAIL', 'UNSURE', 'n/a'],
    ['PASS', 'UNVERIFIED', 'n/a'],
    ['FAIL', 'UNVERIFIED', 'n/a'],
    ['UNVERIFIED', 'PASS', 'n/a'],
    ['UNSURE', 'PASS', 'n/a'],
  ];
  for (const [det, jev, expected] of table) assert.equal(agreement(det, v(jev)), expected, `${det} / ${jev}`);
});

test('표: 세 칸과 합계 줄, UNVERIFIED 는 낱말 그대로 보인다', () => {
  const { lines } = aggregate([
    { name: 'chip', deterministic: 'PASS', jev: v('PASS', 'jev pass') },
    { name: 'divider', deterministic: 'PASS', jev: v('UNVERIFIED', 'no Jev key') },
    { name: 'dialog', deterministic: 'FAIL', jev: v('PASS', 'jev pass') },
  ]);
  assert.match(lines[0], /check\s+\| deterministic \| Jev\s+\| agreement/);
  assert.ok(lines.some((l) => l.includes('UNVERIFIED: no Jev key') && l.endsWith('n/a')));
  assert.ok(lines.some((l) => l.startsWith('dialog') && l.endsWith('DISAGREE')));
  assert.equal(lines.at(-1), 'deterministic PASS 2 FAIL 1 | Jev PASS 2 FAIL 0 UNSURE 0 UNVERIFIED 1 | disagreements 1');
});

test('종료코드는 결정적 판정만 본다: Jev PASS 는 결정적 FAIL 을 덮지 못한다', () => {
  assert.equal(aggregate([{ name: 'a', deterministic: 'FAIL', jev: v('PASS') }]).exitCode, 1);
  assert.equal(aggregate([{ name: 'a', deterministic: 'PASS', jev: v('FAIL') }]).exitCode, 0);
  assert.equal(aggregate([{ name: 'a', deterministic: 'PASS', jev: v('UNVERIFIED', 'no Jev key') }]).exitCode, 0);
});

test('종료코드: 결정적 칸이 전부 PASS 일 때만 0 — FAIL 1, UNVERIFIED 6, UNSURE 3, 빈 표 6', () => {
  const code = (...determinism) => aggregate(determinism.map((deterministic, i) => ({ name: `c${i}`, deterministic, jev: v('PASS') }))).exitCode;
  assert.equal(code('PASS', 'PASS'), 0);
  assert.equal(code('PASS', 'FAIL'), 1);
  assert.equal(code('UNVERIFIED'), 6, '결정적 칸이 UNVERIFIED 뿐이면 통과가 아니다');
  assert.equal(code('PASS', 'UNVERIFIED'), 6);
  assert.equal(code('UNSURE'), 3);
  assert.equal(code('PASS', 'UNSURE'), 3);
  assert.equal(code('UNVERIFIED', 'UNSURE'), 6, 'UNVERIFIED 가 UNSURE 보다 앞선다');
  assert.equal(code('FAIL', 'UNVERIFIED', 'UNSURE'), 1, 'FAIL 이 가장 앞선다');
  assert.equal(aggregate([]).exitCode, 6, '행이 없으면 아무것도 확인하지 못한 것이다');
});

test('Jev 칸이 없으면 UNVERIFIED(not asked)', () => {
  const { lines, counts } = aggregate([{ name: 'a', deterministic: 'PASS' }]);
  assert.ok(lines.some((l) => l.includes('UNVERIFIED: not asked')));
  assert.equal(counts.jev.PASS, 0);
});

test('결정적 판정이 낱말이 아니면 던진다', () => {
  assert.throws(() => aggregate([{ name: 'a', deterministic: 'ok', jev: v('PASS') }]), TypeError);
});
