import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { VerdictCache, cachedJudge, judgeIdentity, sha256 } from './cache.mjs';
import { FakeJudge } from './adapters/fakeJudge.mjs';
import { tempRepo, writePng } from './testkit/helpers.mjs';

const PASS = { status: 'PASS', reason: 'jev pass', noul: 0.97 };

test('같은 진술과 같은 이미지는 안쪽을 한 번만 부른다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const fake = new FakeJudge({ check: PASS });
    const judge = cachedJudge(fake, new VerdictCache(path.join(t.out, 'jev-cache')));
    const first = await judge.check(image, 'the chip is visible');
    const second = await judge.check(image, 'the chip is visible');
    assert.equal(first.status, 'PASS');
    assert.equal(second.cached, true);
    assert.equal(fake.calls.length, 1);
  } finally {
    t.cleanup();
  }
});

test('진술·기대값·이미지가 다르면 따로 묻는다', async () => {
  const t = tempRepo();
  try {
    const a = writePng(t.out, 'a.png', 320, 300);
    const b = writePng(t.out, 'b.png', 330, 300);
    const fake = new FakeJudge({ check: PASS });
    const judge = cachedJudge(fake, new VerdictCache(path.join(t.out, 'jev-cache')));
    await judge.check(a, 'one');
    await judge.check(a, 'two');
    await judge.check(a, 'one', 'no');
    await judge.check(b, 'one');
    assert.equal(fake.calls.length, 4);
  } finally {
    t.cleanup();
  }
});

test('판정자 id 나 version 이 다르면 같은 질문도 캐시를 물려받지 않는다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const cache = new VerdictCache(path.join(t.out, 'jev-cache'));
    const first = new FakeJudge({ check: PASS });
    await cachedJudge(first, cache).check(image, 's');
    assert.equal(first.calls.length, 1);

    const sameAgain = new FakeJudge({ check: PASS });
    assert.equal((await cachedJudge(sameAgain, cache).check(image, 's')).cached, true, '같은 id·version 은 적중');
    assert.equal(sameAgain.calls.length, 0);

    const otherId = Object.assign(new FakeJudge({ check: PASS }), { id: 'real-jev' });
    await cachedJudge(otherId, cache).check(image, 's');
    assert.equal(otherId.calls.length, 1, '다른 id 는 다시 묻는다');

    const newVersion = Object.assign(new FakeJudge({ check: PASS }), { version: 2 });
    await cachedJudge(newVersion, cache).check(image, 's');
    assert.equal(newVersion.calls.length, 1, '다른 version 은 다시 묻는다');
    await cachedJudge(newVersion, cache).checkAll(image, [{ name: 'a', statement: 's' }]);
    assert.equal(newVersion.calls.filter((c) => c.verb === 'checkAll').length, 0, 'checkAll 도 같은 키를 쓴다');
    const screens = new FakeJudge({ screen: { status: 'PASS', reason: 'screen=x', choice: 'x' } });
    await cachedJudge(screens, cache).screen(image, ['x'], 'x');
    const otherScreens = Object.assign(new FakeJudge({ screen: { status: 'PASS', reason: 'screen=x', choice: 'x' } }), { id: 'other' });
    await cachedJudge(otherScreens, cache).screen(image, ['x'], 'x');
    assert.deepEqual([screens.calls.length, otherScreens.calls.length], [1, 1], 'screen 도 판정자별로 따로 묻는다');
  } finally {
    t.cleanup();
  }
});

test('id 를 밝히지 않은 판정자는 클래스 이름과 version 0 으로 가른다', () => {
  assert.deepEqual(judgeIdentity({ id: 'a', version: 3 }), ['a', 3]);
  class Mystery {}
  assert.deepEqual(judgeIdentity(new Mystery()), ['Mystery', 0]);
});

test('UNVERIFIED 는 저장하지 않아 키가 생기면 다시 묻는다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const dir = path.join(t.out, 'jev-cache');
    const fake = new FakeJudge({ check: [{ status: 'UNVERIFIED', reason: 'no Jev key' }, PASS] });
    const judge = cachedJudge(fake, new VerdictCache(dir));
    assert.equal((await judge.check(image, 's')).status, 'UNVERIFIED');
    assert.equal(fs.existsSync(dir) ? fs.readdirSync(dir).length : 0, 0);
    assert.equal((await judge.check(image, 's')).status, 'PASS');
    assert.equal(fake.calls.length, 2);
  } finally {
    t.cleanup();
  }
});

test('FAIL 과 UNSURE 는 저장한다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const cache = new VerdictCache(path.join(t.out, 'jev-cache'));
    const fake = new FakeJudge({ check: [{ status: 'FAIL', reason: 'jev fail' }, { status: 'UNSURE', reason: 'jev unsure' }] });
    const judge = cachedJudge(fake, cache);
    await judge.check(image, 'x');
    await judge.check(image, 'y');
    assert.equal((await judge.check(image, 'x')).status, 'FAIL');
    assert.equal((await judge.check(image, 'y')).status, 'UNSURE');
    assert.equal(fake.calls.length, 2);
  } finally {
    t.cleanup();
  }
});

test('checkAll: 적중하지 못한 질문만 한 요청으로 묶어 묻는다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const fake = new FakeJudge({ check: PASS });
    const judge = cachedJudge(fake, new VerdictCache(path.join(t.out, 'jev-cache')));
    await judge.check(image, 'one');
    const all = await judge.checkAll(image, [{ name: 'a', statement: 'one' }, { name: 'b', statement: 'two' }, { name: 'c', statement: 'three' }]);
    assert.deepEqual(all.map((v) => Boolean(v.cached)), [true, false, false]);
    const batch = fake.calls.find((c) => c.verb === 'checkAll');
    assert.deepEqual(batch.items.map((i) => i.statement), ['two', 'three']);
  } finally {
    t.cleanup();
  }
});

test('screen 도 캐시한다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const fake = new FakeJudge({ screen: { status: 'PASS', reason: 'screen=settings', choice: 'settings' } });
    const judge = cachedJudge(fake, new VerdictCache(path.join(t.out, 'jev-cache')));
    await judge.screen(image, ['settings', 'board-expanded'], 'settings');
    assert.equal((await judge.screen(image, ['settings', 'board-expanded'], 'settings')).cached, true);
    await judge.screen(image, ['settings', 'board-expanded'], 'board-expanded');
    assert.equal(fake.calls.length, 2);
  } finally {
    t.cleanup();
  }
});

test('깨진 캐시 파일은 삼키지 않고 던진다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'a.png');
    const dir = path.join(t.out, 'jev-cache');
    const cache = new VerdictCache(dir);
    const key = VerdictCache.key([...judgeIdentity(new FakeJudge()), 'check', 's', 'yes', sha256(fs.readFileSync(image))]);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, `v-${key}.json`), '{not json');
    const judge = cachedJudge(new FakeJudge({ check: PASS }), cache);
    await assert.rejects(() => judge.check(image, 's'), /깨졌다/);
  } finally {
    t.cleanup();
  }
});

test('캐시 키는 sha256 hex 만 받는다(경로 주입 방지)', () => {
  const cache = new VerdictCache('x');
  assert.throws(() => cache.get('../evil'), TypeError);
  assert.throws(() => cache.put('../evil', PASS), TypeError);
});
