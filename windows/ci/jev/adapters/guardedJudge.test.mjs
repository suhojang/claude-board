import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { guardedJudge } from './guardedJudge.mjs';
import { FakeJudge } from './fakeJudge.mjs';
import { cachedJudge, VerdictCache } from '../cache.mjs';
import { cropFile } from '../crop.mjs';
import { GuardError, manifestPathOf } from '../domain/guard.mjs';
import { makePng, tempRepo, writePng } from '../testkit/helpers.mjs';

const PASS = { status: 'PASS', reason: 'jev pass' };
const USER = 'someone';

test('개인정보가 든 진술·이름·선택지는 GuardError 이고 안쪽은 불리지 않는다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const fake = new FakeJudge({ check: PASS, screen: PASS });
    const judge = guardedJudge(fake, { repo: t.repo, userName: USER });
    const cases = [
      () => judge.check(image, 'the chip of Someone is visible'),
      () => judge.check(image, 'the file C:\\Users\\x\\a.png is shown'),
      () => judge.check(image, 'the e-mail owner@example.com is shown'),
      () => judge.check(image, `token ${'a'.repeat(40)} is shown`),
      () => judge.checkAll(image, [{ name: 'a', statement: 'fine' }, { name: 'b', statement: 'x@y.cc is shown' }]),
      () => judge.screen(image, ['settings', 'x'.repeat(33)], 'settings'),
    ];
    for (const run of cases) {
      await assert.rejects(run, (error) => error instanceof GuardError && /^privacy guard: /.test(error.message));
    }
    assert.equal(fake.calls.length, 0, '막힌 것은 안쪽에 닿지 않는다');
  } finally {
    t.cleanup();
  }
});

test('깨끗한 진술은 지나가고, 에러 메시지에는 걸린 글자가 없다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const fake = new FakeJudge({ check: PASS });
    const judge = guardedJudge(fake, { repo: t.repo, userName: USER });
    assert.equal((await judge.check(image, 'the chip "CB" is visible in the highlighted session row')).status, 'PASS');
    const secret = 'sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ012345';
    await assert.rejects(() => judge.check(image, `key ${secret}`), (error) => !error.message.includes('sk-ant'));
  } finally {
    t.cleanup();
  }
});

test('이미지는 한 번만 읽어 같은 바이트의 임시 사본을 안쪽에 준다 — 검사 뒤에 원본이 바뀌어도 안쪽은 검사한 바이트를 본다', async () => {
  const t = tempRepo();
  try {
    const original = path.join(t.out, 'a.png');
    const checkedBytes = makePng(320, 300);
    fs.writeFileSync(original, checkedBytes);
    const seen = [];
    let reads = 0;
    const fake = new FakeJudge({
      check: (copy) => {
        seen.push({ copy, bytes: fs.readFileSync(copy), inE2eOut: path.relative(t.out, copy).split(path.sep)[0].startsWith('jev-in-') });
        return PASS;
      },
    });
    const readFile = (file) => {
      reads += 1;
      const bytes = fs.readFileSync(file);
      fs.writeFileSync(file, makePng(321, 300)); // 검사 직후 원본을 바꿔치기한다
      return bytes;
    };
    const judge = guardedJudge(cachedJudge(fake, new VerdictCache(path.join(t.out, 'jev-cache'))), { repo: t.repo, userName: USER, readFile });
    assert.equal((await judge.check(original, 'the chip is visible')).status, 'PASS');
    assert.equal(reads, 1, '원본은 한 번만 읽는다');
    assert.equal(seen.length, 1);
    assert.deepEqual(seen[0].bytes, checkedBytes, '안쪽이 본 바이트 = 검사한 바이트');
    assert.notDeepEqual(fs.readFileSync(original), checkedBytes, '원본은 실제로 바뀌었다');
    assert.notEqual(seen[0].copy, original);
    assert.equal(seen[0].inE2eOut, true, '사본은 e2e/out/jev-in-* 아래');
    assert.equal(fs.existsSync(seen[0].copy), false, '끝나면 사본을 지운다');
    assert.deepEqual(fs.readdirSync(t.out).filter((n) => n.startsWith('jev-in-')), [], '사본 폴더도 남지 않는다');
  } finally {
    t.cleanup();
  }
});

test('안쪽이 던져도 임시 사본은 지운다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const fake = new FakeJudge({
      check: () => {
        throw new Error('boom');
      },
    });
    await assert.rejects(() => guardedJudge(fake, { repo: t.repo, userName: USER }).check(image, 'x'), /boom/);
    assert.deepEqual(fs.readdirSync(t.out).filter((n) => n.startsWith('jev-in-')), []);
  } finally {
    t.cleanup();
  }
});

function cropped(t, windowTitle) {
  const source = path.join(t.root, 'full.png');
  fs.writeFileSync(source, makePng(400, 300));
  const output = path.join(t.out, 'board.png');
  cropFile({ input: source, rect: { x: 10, y: 10, width: 300, height: 280 }, output, repo: t.repo, windowTitle });
  return output;
}

test('allowedWindowTitle: manifest 의 창 제목이 같을 때만 지나간다', async () => {
  const t = tempRepo();
  try {
    const board = cropped(t, 'Claude Board');
    const fake = new FakeJudge({ check: PASS });
    const ok = guardedJudge(fake, { repo: t.repo, userName: USER, allowedWindowTitle: 'Claude Board' });
    assert.equal((await ok.check(board, 'the chip is visible')).status, 'PASS');
    const wrong = guardedJudge(fake, { repo: t.repo, userName: USER, allowedWindowTitle: 'Notepad' });
    await assert.rejects(() => wrong.check(board, 'the chip is visible'), /창 제목/);
    assert.equal(fake.calls.length, 1);
  } finally {
    t.cleanup();
  }
});

test('allowedWindowTitle: manifest 가 없거나, 제목 없이 잘랐거나, 자른 뒤 그림이 바뀌었으면 막는다', async () => {
  const t = tempRepo();
  try {
    const fake = new FakeJudge({ check: PASS });
    const judge = guardedJudge(fake, { repo: t.repo, userName: USER, allowedWindowTitle: 'Claude Board' });
    const plain = writePng(t.out, 'plain.png'); // crop 을 거치지 않은 그림
    await assert.rejects(() => judge.check(plain, 'x'), /manifest/);
    const untitled = cropped(t, undefined);
    await assert.rejects(() => judge.check(untitled, 'x'), /창 제목/);
    const board = cropped(t, 'Claude Board');
    fs.writeFileSync(board, makePng(310, 280)); // 자른 뒤에 바꿔치기
    await assert.rejects(() => judge.check(board, 'x'), /sha256/);
    const broken = cropped(t, 'Claude Board');
    fs.writeFileSync(manifestPathOf(broken), '{not json');
    await assert.rejects(() => judge.check(broken, 'x'), /manifest/);
    assert.equal(fake.calls.length, 0);
  } finally {
    t.cleanup();
  }
});

test('allowedWindowTitle 이 없으면 manifest 를 보지 않는다(기존 동작)', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'plain.png');
    const fake = new FakeJudge({ check: PASS });
    assert.equal((await guardedJudge(fake, { repo: t.repo, userName: USER }).check(image, 'x')).status, 'PASS');
  } finally {
    t.cleanup();
  }
});

test('창 제목이나 manifest 글에 개인정보가 있으면 막는다', async () => {
  const t = tempRepo();
  try {
    const board = cropped(t, 'Board of Someone');
    const fake = new FakeJudge({ check: PASS });
    const judge = guardedJudge(fake, { repo: t.repo, userName: USER, allowedWindowTitle: 'Board of Someone' });
    await assert.rejects(() => judge.check(board, 'x'), /privacy guard: user name in windowTitle/);
    assert.equal(fake.calls.length, 0);
  } finally {
    t.cleanup();
  }
});
