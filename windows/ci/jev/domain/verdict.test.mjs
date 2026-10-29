import { test } from 'node:test';
import assert from 'node:assert/strict';
import { toScreenVerdict, toSelftestVerdict, toVerdict, toVerdicts } from './verdict.mjs';

const NOKEY = 'ERROR|6|PreconditionError|Windows 자격 증명 관리자 조회는 아직 구현되지 않았다(gamerig-win 몫). 환경변수 TYPESAFE_API_KEY 를 쓴다';
const pass = 'CHECK|PASS|q1|expect=yes|noul=0.97';
const fail = 'CHECK|FAIL|q1|expect=yes|noul=0.04';
const unsure = 'CHECK|UNSURE|q1|expect=yes|noul=0.50';

test('종료코드 표: rc 와 결과 줄이 맞아야 판정이 나온다', () => {
  const table = [
    [0, [pass], 'PASS'],
    [1, [fail], 'FAIL'],
    [3, [unsure], 'UNSURE'],
    [6, [NOKEY], 'UNVERIFIED', 'no Jev key'],
    [6, ['ERROR|6|PreconditionError|venv 가 없다'], 'UNVERIFIED', 'jev precondition'],
    [7, ['ERROR|7|JevError|jev 500'], 'UNVERIFIED', 'jev error or cache miss'],
    [2, ['ERROR|2|UsageError|x'], 'UNVERIFIED', 'jev usage error'],
    [4, ['ERROR|4|BudgetExceeded|x'], 'UNVERIFIED', 'jev budget used up'],
    [5, ['ERROR|5|GuardViolation|x'], 'UNVERIFIED', 'jev guard refused'],
    [99, [], 'UNVERIFIED', 'jevvision exit 99'],
  ];
  for (const [rc, lines, status, reason] of table) {
    const v = toVerdict(rc, lines);
    assert.equal(v.status, status, `rc ${rc}`);
    if (reason) assert.equal(v.reason, reason, `rc ${rc}`);
  }
});

test('비-0 종료코드는 어떤 줄이 와도 PASS 가 되지 않는다', () => {
  for (let rc = 1; rc <= 255; rc += 1) {
    assert.notEqual(toVerdict(rc, [pass]).status, 'PASS', `rc ${rc} with PASS line`);
  }
});

test('종료코드 0 이어도 PASS 줄이 없으면 UNVERIFIED', () => {
  assert.equal(toVerdict(0, []).status, 'UNVERIFIED');
  assert.equal(toVerdict(0, [fail]).status, 'UNVERIFIED');
  assert.equal(toVerdict(0, [unsure]).status, 'UNVERIFIED');
  assert.equal(toVerdict(0, ['CHECK|PASS|other|expect=yes|noul=0.97']).status, 'UNVERIFIED', '다른 이름의 줄');
});

test('종료코드 1 은 FAIL 줄이 있을 때만 FAIL (파이썬 예외도 1 이다)', () => {
  assert.equal(toVerdict(1, []).status, 'UNVERIFIED');
  assert.equal(toVerdict(1, ['Traceback (most recent call last):']).status, 'UNVERIFIED');
  assert.equal(toVerdict(1, [pass]).status, 'UNVERIFIED');
});

test('종료코드 3 은 UNSURE 줄 또는 ASK 줄이 있을 때만 UNSURE', () => {
  assert.equal(toVerdict(3, []).status, 'UNVERIFIED');
  const ask = toVerdict(3, ['ASK|check|사람에게 넘긴다']);
  assert.equal(ask.status, 'UNSURE');
  assert.equal(ask.detail, '사람에게 넘긴다');
});

test('noul 값을 읽는다', () => {
  assert.equal(toVerdict(0, [pass]).noul, 0.97);
  assert.equal(toVerdict(1, [fail]).noul, 0.04);
});

test('여러 질문: 줄마다 판정, 종료코드는 합산과 맞아야 한다', () => {
  const lines = ['CHECK|PASS|a|expect=yes|noul=0.97', 'CHECK|FAIL|b|expect=yes|noul=0.02', 'CHECK|UNSURE|c|expect=no|noul=0.5'];
  assert.deepEqual(toVerdicts(1, lines, ['a', 'b', 'c']).map((v) => v.status), ['PASS', 'FAIL', 'UNSURE']);
  assert.ok(toVerdicts(0, lines, ['a', 'b', 'c']).every((v) => v.status === 'UNVERIFIED'), 'rc 0 인데 FAIL 줄');
  assert.ok(toVerdicts(3, lines, ['a', 'b', 'c']).every((v) => v.status === 'UNVERIFIED'), 'rc 3 인데 FAIL 줄');
  assert.equal(toVerdicts(1, lines.slice(0, 2), ['a', 'b', 'c'])[2].status, 'UNVERIFIED', '줄이 없는 질문');
});

test('필드 속 파이프와 CR 은 줄을 깨지 않는다', () => {
  assert.equal(toVerdict(0, ['CHECK|PASS|q1|expect=yes|noul=0.97\r']).status, 'PASS');
});

test('결과 줄이 아닌 줄(트레이스백)은 버리고 상세의 긴 토큰은 가린다', () => {
  const v = toVerdict(7, ['sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'ERROR|7|JevError|bearer bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb']);
  assert.equal(v.status, 'UNVERIFIED');
  assert.ok(!v.detail.includes('bbbbbbbbbbbbbbbbbbbbbbbbbbbb'));
  assert.ok(v.detail.includes('[redacted]'));
});

test('screen: expect 가 같으면 PASS, 다르면 FAIL, other 는 UNSURE', () => {
  const line = (name) => [`SCREEN|${name}|conf=0.95|popup=0.01|system_dialog=0.00|loading=0.00`];
  assert.deepEqual(toScreenVerdict(0, line('settings'), 'settings'), { status: 'PASS', reason: 'screen=settings', choice: 'settings' });
  assert.equal(toScreenVerdict(0, line('settings'), 'board-expanded').status, 'FAIL');
  assert.equal(toScreenVerdict(0, line('settings')).status, 'PASS');
  assert.equal(toScreenVerdict(0, line('other'), 'settings').status, 'UNSURE');
  assert.equal(toScreenVerdict(0, []).status, 'UNVERIFIED');
  assert.equal(toScreenVerdict(3, ['ASK|screen|확신이 낮다']).status, 'UNSURE');
  assert.equal(toScreenVerdict(3, []).status, 'UNVERIFIED');
  assert.equal(toScreenVerdict(6, [NOKEY]).reason, 'no Jev key');
});

test('selftest: SELFTEST|PASS 와 종료코드 0 만 PASS', () => {
  assert.equal(toSelftestVerdict(0, ['SELFTEST|PASS|box=1']).status, 'PASS');
  assert.equal(toSelftestVerdict(0, []).status, 'UNVERIFIED');
  assert.equal(toSelftestVerdict(7, ['SELFTEST|FAIL|kind=absent']).status, 'FAIL');
  assert.equal(toSelftestVerdict(6, [NOKEY]).reason, 'no Jev key');
});
