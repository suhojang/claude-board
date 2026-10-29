// 어댑터 시험: 하위 프로세스는 testkit/stub-cli.mjs(jevvision.cli 대역)다. 실제 Jev 호출은 없다.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { JevLookJudge, resolveCommand } from './jevlook.mjs';
import { tempRepo, writePng } from '../testkit/helpers.mjs';

const STUB = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'testkit', 'stub-cli.mjs');

function setup(scenario, extra = {}) {
  const t = tempRepo();
  const argvLog = path.join(t.root, 'argv.log');
  const judge = new JevLookJudge({
    repo: t.repo,
    cacheDir: path.join(t.out, 'jev-cache', 'harness'),
    command: { file: process.execPath, prefixArgs: [STUB] },
    env: { ...process.env, JEV_STUB: scenario, JEV_STUB_ARGV_LOG: argvLog, ...extra.env },
    ...extra.options,
  });
  return { t, judge, image: writePng(t.out, 'board.png'), argvLog };
}

const table = [
  ['pass', 'PASS', undefined],
  ['fail', 'FAIL', undefined],
  ['unsure', 'UNSURE', undefined],
  ['ask', 'UNSURE', undefined],
  ['nokey', 'UNVERIFIED', 'no Jev key'],
  ['precondition', 'UNVERIFIED', 'jev precondition'],
  ['error7', 'UNVERIFIED', 'jev error or cache miss'],
  ['budget4', 'UNVERIFIED', 'jev budget used up'],
  ['guard5', 'UNVERIFIED', 'jev guard refused'],
  ['usage2', 'UNVERIFIED', 'jev usage error'],
  ['crash', 'UNVERIFIED', 'exit code and result lines disagree'],
  ['silent0', 'UNVERIFIED', 'exit code and result lines disagree'],
  ['pass-but-fail-line', 'UNVERIFIED', 'exit code and result lines disagree'],
];
for (const [scenario, status, reason] of table) {
  test(`스텁 ${scenario} -> ${status}${reason ? ` (${reason})` : ''}`, async () => {
    const { t, judge, image } = setup(scenario);
    try {
      const verdict = await judge.check(image, 'the chip text "CB" is visible');
      assert.equal(verdict.status, status);
      if (reason) assert.equal(verdict.reason, reason);
    } finally {
      t.cleanup();
    }
  });
}

test('하위 프로세스가 받은 인자: 동사·이미지·--check 셋·--repo·--jev-cache·--record·--budget', async () => {
  const { t, judge, image, argvLog } = setup('pass');
  try {
    await judge.check(image, 'the chip text "CB" is visible', 'yes');
    const call = JSON.parse(fs.readFileSync(argvLog, 'utf8').trim());
    assert.deepEqual(call.args.slice(0, 6), ['check', image, '--check', 'q1', 'the chip text "CB" is visible', 'yes']);
    assert.ok(call.args.includes('--record'));
    assert.equal(call.args[call.args.indexOf('--repo') + 1], t.repo);
    assert.equal(call.args[call.args.indexOf('--budget') + 1], '6');
    assert.equal(call.cwd, t.repo);
    assert.ok(!call.args.some((a) => /key|bearer/i.test(a)), '인자에 키 관련 낱말이 없다');
  } finally {
    t.cleanup();
  }
});

test('--replay 모드: --record 를 붙이지 않는다', async () => {
  const { t, judge, image, argvLog } = setup('pass', { options: { record: false } });
  try {
    await judge.check(image, 's');
    assert.ok(!JSON.parse(fs.readFileSync(argvLog, 'utf8').trim()).args.includes('--record'));
  } finally {
    t.cleanup();
  }
});

test('환경은 그대로 물려준다: 어댑터가 키를 넣지도 빼지도 않는다', async () => {
  const inherited = setup('pass', { env: { TYPESAFE_API_KEY: undefined } });
  try {
    delete inherited.judge.env.TYPESAFE_API_KEY;
    await inherited.judge.check(inherited.image, 's');
    assert.equal(JSON.parse(fs.readFileSync(inherited.argvLog, 'utf8').trim()).keyInEnv, false);
  } finally {
    inherited.t.cleanup();
  }
  const present = setup('pass');
  try {
    present.judge.env.TYPESAFE_API_KEY = 'placeholder-not-a-real-key';
    await present.judge.check(present.image, 's');
    assert.equal(JSON.parse(fs.readFileSync(present.argvLog, 'utf8').trim()).keyInEnv, true, '물려준다(하네스 KeySource 가 읽는다)');
    assert.ok(!fs.readFileSync(present.argvLog, 'utf8').includes('placeholder-not-a-real-key'), '값은 어디에도 기록되지 않는다');
  } finally {
    present.t.cleanup();
  }
});

test('여러 질문은 한 번의 호출로 묻고 줄마다 판정을 받는다', async () => {
  const { t, judge, image, argvLog } = setup('fail');
  try {
    const verdicts = await judge.checkAll(image, [{ name: 'a', statement: 'one' }, { name: 'b', statement: 'two', expect: 'no' }]);
    assert.deepEqual(verdicts.map((v) => v.status), ['FAIL', 'PASS']);
    assert.equal(fs.readFileSync(argvLog, 'utf8').trim().split('\n').length, 1);
  } finally {
    t.cleanup();
  }
});

test('긴 토큰 모양과 추적 줄은 판정에 새지 않는다', async () => {
  const { t, judge, image } = setup('long-token');
  try {
    const verdict = await judge.check(image, 's');
    const text = JSON.stringify(verdict);
    assert.ok(!text.includes('aaaaaaaaaaaaaaaa'));
    assert.ok(!text.includes('sk-'));
  } finally {
    t.cleanup();
  }
});

test('시간 초과는 UNVERIFIED(jev timeout)', async () => {
  const { t, judge, image } = setup('hang', { options: { timeoutMs: 300 } });
  try {
    const verdict = await judge.check(image, 's');
    assert.deepEqual([verdict.status, verdict.reason], ['UNVERIFIED', 'jev timeout']);
  } finally {
    t.cleanup();
  }
});

test('파이썬이 없으면 UNVERIFIED(jevvision not runnable)', async () => {
  const { t, judge, image } = setup('pass', { options: { command: { file: path.join(process.cwd(), 'no-such-python.exe'), prefixArgs: [] } } });
  try {
    const verdict = await judge.check(image, 's');
    assert.deepEqual([verdict.status, verdict.reason], ['UNVERIFIED', 'jevvision not runnable']);
  } finally {
    t.cleanup();
  }
});

test('screen: 고른 화면과 expect 를 비교한다', async () => {
  const same = setup('screen', { env: { JEV_STUB_SCREEN: 'settings' } });
  try {
    const verdict = await same.judge.screen(same.image, ['settings', 'board-expanded'], 'settings');
    assert.deepEqual([verdict.status, verdict.choice], ['PASS', 'settings']);
    const call = JSON.parse(fs.readFileSync(same.argvLog, 'utf8').trim());
    assert.equal(call.args[0], 'screen');
    assert.equal(fs.readdirSync(path.dirname(call.args[call.args.indexOf('--screens') + 1])).filter((n) => n.startsWith('screens-')).length, 0, '임시 화면 목록은 지운다');
  } finally {
    same.t.cleanup();
  }
  const other = setup('screen', { env: { JEV_STUB_SCREEN: 'board-expanded' } });
  try {
    assert.equal((await other.judge.screen(other.image, ['settings', 'board-expanded'], 'settings')).status, 'FAIL');
  } finally {
    other.t.cleanup();
  }
});

test('screen: 잘못된 화면 이름은 호출 전에 던진다', async () => {
  const { t, judge, image, argvLog } = setup('screen');
  try {
    await assert.rejects(() => judge.screen(image, ['has space']), /화면 이름/);
    await assert.rejects(() => judge.screen(image, ['other']), /화면 이름/);
    await assert.rejects(() => judge.screen(image, []), /비었다/);
    await assert.rejects(() => judge.screen(image, ['a'], 'b'), /choices 에 없다/);
    assert.equal(fs.existsSync(argvLog), false, '하위 프로세스를 부르지 않았다');
  } finally {
    t.cleanup();
  }
});

test('selftest: SELFTEST|PASS 면 PASS, 키가 없으면 UNVERIFIED', async () => {
  const ok = setup('selftest-pass');
  try {
    assert.equal((await ok.judge.selftest()).status, 'PASS');
  } finally {
    ok.t.cleanup();
  }
  const none = setup('nokey');
  try {
    assert.equal((await none.judge.selftest()).reason, 'no Jev key');
  } finally {
    none.t.cleanup();
  }
});

test('resolveCommand: 하네스 venv 가 있으면 그것, 없으면 python -I', () => {
  const env = { USERPROFILE: 'C:\\Users\\someone' };
  const withVenv = resolveCommand({ platform: 'win32', env, exists: () => true });
  assert.equal(withVenv.file, path.join('C:\\Users\\someone', '.local', 'share', 'ui-automation', 'venv', 'Scripts', 'python.exe'));
  const without = resolveCommand({ platform: 'win32', env, exists: () => false });
  assert.equal(without.file, 'python');
  assert.deepEqual(without.prefixArgs.slice(0, 4), ['-I', '-B', '-X', 'utf8']);
  assert.equal(without.prefixArgs.at(-1), path.join('C:\\Users\\someone', '.claude', 'skills', 'ui-automation', 'scripts'));
  assert.equal(resolveCommand({ platform: 'win32', env: { ...env, UI_AUTOMATION_HOME: 'D:\\ua' }, exists: () => true }).file, path.join('D:\\ua', 'venv', 'Scripts', 'python.exe'));
  assert.equal(resolveCommand({ platform: 'linux', env: { HOME: '/h' }, exists: () => false }).file, 'python3');
});
