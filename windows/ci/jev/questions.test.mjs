import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { FAMILIES, IMAGE_LIMITS, SCREENS, TRUST } from './questions.mjs';
import { assertJudge, assertVerdict, normalizeItem, UsageError } from './port.mjs';
import { FakeJudge } from './adapters/fakeJudge.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));

test('가족: 문구는 비어 있지 않고 겹치지 않으며 보정 전이다', () => {
  const names = Object.keys(FAMILIES);
  assert.deepEqual(names, ['chip', 'terminal-clean', 'confirm-dialog', 'dialog-two-sessions', 'vertical-divider', 'settings-screen']);
  assert.equal(new Set(Object.values(FAMILIES).map((f) => f.statement)).size, names.length);
  for (const [name, family] of Object.entries(FAMILIES)) {
    assert.ok(family.statement.length > 10, name);
    assert.match(family.expect, /^(yes|no)$/, name);
    assert.equal(family.calibration.status, 'not calibrated', `${name}: 보정 결과 없이 바꾸지 않는다`);
    assert.equal(family.calibration.wilsonLower, null);
  }
  assert.ok(FAMILIES.chip.statement.includes('"CB"'));
});

test('화면 목록과 신뢰 문턱', () => {
  assert.deepEqual(Object.keys(SCREENS), ['board-collapsed', 'board-expanded', 'settings', 'shutdown-dialog', 'empty-terminal']);
  assert.deepEqual({ ...TRUST }, { minPositives: 30, minNegatives: 30, wilsonLower: 0.9, falsePassMax: 1 / 30, abstainMax: 0.35 });
});

test('판단값에 judgment 표시가 붙어 있다 (문턱은 보정 전엔 판단값)', () => {
  const source = fs.readFileSync(path.join(HERE, 'questions.mjs'), 'utf8');
  assert.match(source, /신뢰 문턱 \(judgment/);
  assert.match(source, /judgment/);
});

test('이미지 한도 사본이 하네스 thresholds.py 와 같다 (하네스가 있는 머신에서만)', (t) => {
  const file = path.join(os.homedir(), '.claude', 'skills', 'ui-automation', 'scripts', 'jevvision', 'domain', 'thresholds.py');
  if (!fs.existsSync(file)) return t.skip('하네스 thresholds.py 가 없다');
  const text = fs.readFileSync(file, 'utf8');
  const read = (name) => Number(new RegExp(`^${name} = ([0-9_]+)`, 'm').exec(text)?.[1].replaceAll('_', ''));
  assert.equal(IMAGE_LIMITS.minSide, read('IMAGE_MIN_SIDE'));
  assert.equal(IMAGE_LIMITS.maxSide, read('IMAGE_MAX_SIDE'));
  assert.equal(IMAGE_LIMITS.minPixels, read('IMAGE_MIN_PIXELS'));
  assert.equal(IMAGE_LIMITS.maxPixels, read('IMAGE_MAX_PIXELS'));
  assert.equal(IMAGE_LIMITS.maxFileBytes, 16 * 1024 * 1024);
});

test('normalizeItem: 문자열·객체·잘못된 입력', () => {
  assert.deepEqual(normalizeItem('abc'), { statement: 'abc', expect: 'yes', name: 'q1' });
  assert.deepEqual(normalizeItem({ name: 'x', statement: 'abc', expect: 'no' }), { name: 'x', statement: 'abc', expect: 'no' });
  assert.equal(normalizeItem('abc', 'yes', 2).name, 'q3');
  assert.throws(() => normalizeItem(''), UsageError);
  assert.throws(() => normalizeItem('a', 'maybe'), UsageError);
  assert.throws(() => normalizeItem({ name: 'bad name', statement: 'a' }), UsageError);
});

test('포트: FakeJudge 는 Judge 모양이고, 정해 두지 않은 질문은 PASS 가 아니다', async () => {
  const judge = assertJudge(new FakeJudge());
  const verdict = assertVerdict(await judge.check('x.png', 'a'));
  assert.equal(verdict.status, 'UNVERIFIED');
  assert.throws(() => assertJudge({ check() {} }), TypeError);
  assert.throws(() => assertVerdict({ status: 'OK', reason: '' }), TypeError);
});

test('이 도구의 소스는 키를 읽지 않는다: TYPESAFE 를 환경에서 꺼내는 코드가 없다', () => {
  const sources = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(file);
      else if (entry.name.endsWith('.mjs') && !entry.name.endsWith('.test.mjs') && entry.name !== 'stub-cli.mjs') sources.push(file);
    }
  };
  walk(HERE);
  assert.ok(sources.length >= 12);
  for (const file of sources) {
    const text = fs.readFileSync(file, 'utf8');
    assert.doesNotMatch(text, /(process\.env|env)\s*(\.|\[)\s*['"]?TYPESAFE/, `${path.basename(file)} 가 키를 환경에서 읽는다`);
    assert.doesNotMatch(text, /Authorization|Bearer |cmdkey|CredRead/i, `${path.basename(file)} 가 키 취급 코드를 갖는다`);
    assert.doesNotMatch(text, /api\.typesafe\.ai|fetch\(|https?\.request/, `${path.basename(file)} 가 HTTP 를 안다`);
  }
});
