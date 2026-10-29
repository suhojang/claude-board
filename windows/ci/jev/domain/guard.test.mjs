import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { GuardError, checkPng, ensureNotCompany, ensureUnderE2e, fitSizeFeasible, guardImage } from './guard.mjs';
import { makePng, tempRepo, writePng } from '../testkit/helpers.mjs';

test('e2e/out 아래 PNG 는 통과한다', () => {
  const t = tempRepo();
  try {
    const file = writePng(t.out, 'board.png');
    const guarded = guardImage(file, t.repo);
    assert.equal(guarded.header.width, 320);
    assert.equal(guarded.path, file);
  } finally {
    t.cleanup();
  }
});

test('e2e 밖·e2e 바로 아래·허용 하위 폴더가 아닌 곳은 거절한다', () => {
  const t = tempRepo();
  try {
    fs.mkdirSync(path.join(t.repo, 'e2e', 'other'));
    const cases = [
      writePng(t.repo, 'outside.png'),
      writePng(path.join(t.repo, 'e2e'), 'direct.png'),
      writePng(path.join(t.repo, 'e2e', 'other'), 'other.png'),
      writePng(t.root, 'root.png'),
    ];
    for (const file of cases) assert.throws(() => guardImage(file, t.repo), GuardError, file);
  } finally {
    t.cleanup();
  }
});

test('.. 로 e2e 를 벗어나는 경로는 거절한다', () => {
  const t = tempRepo();
  try {
    writePng(t.repo, 'escape.png');
    assert.throws(() => guardImage(path.join(t.out, '..', '..', 'escape.png'), t.repo), GuardError);
  } finally {
    t.cleanup();
  }
});

test('없는 파일은 GuardError 다(조용히 지나가지 않는다)', () => {
  const t = tempRepo();
  try {
    assert.throws(() => guardImage(path.join(t.out, 'missing.png'), t.repo), GuardError);
  } finally {
    t.cleanup();
  }
});

test('.png 이름이어도 내용이 PNG 가 아니면 거절한다', () => {
  const t = tempRepo();
  try {
    const file = path.join(t.out, 'fake.png');
    fs.writeFileSync(file, 'this is just text, not a picture; padding padding padding');
    assert.throws(() => guardImage(file, t.repo), /PNG 가 아니다/);
  } finally {
    t.cleanup();
  }
});

test('회사 경로 조각이 있으면 거절한다', () => {
  assert.throws(() => ensureNotCompany('C:/x/aijinet-project/e2e/out/a.png'), GuardError);
  assert.throws(() => ensureNotCompany('C:\\x\\aijinet-project\\a.png'), GuardError);
  assert.doesNotThrow(() => ensureNotCompany('C:/x/my-project/a.png'));
});

test('크기 한도: 너무 큰 파일·픽셀·극단 비율을 거절한다', () => {
  assert.doesNotThrow(() => checkPng(makePng(1568, 900)));
  assert.doesNotThrow(() => checkPng(makePng(300, 300)));
  const bigFile = Buffer.concat([makePng(300, 300), Buffer.alloc(16 * 1024 * 1024 + 1)]);
  assert.throws(() => checkPng(bigFile), /바이트/);
  const huge = Buffer.from(makePng(300, 300));
  huge.writeUInt32BE(8000, 16); // IHDR 너비만 바꾼다: 8000x300 = 240 만 픽셀이지만 비율이 극단이다
  assert.throws(() => checkPng(huge), /한도 안으로 맞출 수 없는/);
  const bomb = Buffer.from(makePng(300, 300));
  bomb.writeUInt32BE(30000, 16);
  bomb.writeUInt32BE(30000, 20);
  assert.throws(() => checkPng(bomb), /픽셀 수/);
});

test('fitSizeFeasible: 줄이기·키우기·불가능', () => {
  assert.deepEqual(fitSizeFeasible(3136, 1568), { width: 1568, height: 784 });
  assert.deepEqual(fitSizeFeasible(100, 400), { width: 280, height: 1120 });
  assert.equal(fitSizeFeasible(100, 2000), null);
  assert.deepEqual(fitSizeFeasible(1, 1), { width: 280, height: 280 });
});

test('ensureUnderE2e: 출력 경로(아직 없는 파일)도 따질 수 있다', () => {
  const t = tempRepo();
  try {
    assert.equal(ensureUnderE2e(path.join(t.out, 'new.png'), t.repo, { mustExist: false }), path.join(t.out, 'new.png'));
    assert.throws(() => ensureUnderE2e(path.join(t.repo, 'new.png'), t.repo, { mustExist: false }), GuardError);
  } finally {
    t.cleanup();
  }
});
