// CLI 시험: main() 에 가짜 판정자(FakeJudge)와 가짜 하위 프로세스(스텁)를 꽂는다. 실제 Jev 호출은 없다.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import cp from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { main } from './cli.mjs';
import { FakeJudge } from './adapters/fakeJudge.mjs';
import { FAMILIES } from './questions.mjs';
import { decode } from './domain/png.mjs';
import { tempRepo, writePng } from './testkit/helpers.mjs';

function run(argv, deps = {}) {
  const out = [];
  const err = [];
  return main(argv, { out: (l) => out.push(l), err: (l) => err.push(l), ...deps }).then((code) => ({ code, out, err }));
}

test('check: 가족 문구와 기대값이 그대로 판정자에게 간다 — PASS 는 종료코드 0', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'board.png');
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'jev pass', noul: 0.97 } });
    const r = await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge: fake });
    assert.equal(r.code, 0);
    assert.deepEqual(r.out, ['PASS|jev pass']);
    assert.equal(fake.calls[0].item.statement, FAMILIES.chip.statement);
    assert.equal(fake.calls[0].item.expect, 'yes');
  } finally {
    t.cleanup();
  }
});

test('check: 판정 -> 종료코드 표 (PASS 0, FAIL 1, UNSURE 3, UNVERIFIED 6)', async () => {
  const t = tempRepo();
  try {
    for (const [status, code] of [['PASS', 0], ['FAIL', 1], ['UNSURE', 3], ['UNVERIFIED', 6]]) {
      const image = writePng(t.out, `board-${status}.png`, 300 + code); // 이미지마다 달라야 판정 캐시에 걸리지 않는다
      const r = await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge: new FakeJudge({ check: { status, reason: 'x' } }) });
      assert.equal(r.code, code, status);
      assert.equal(r.out[0], `${status}|x`);
    }
  } finally {
    t.cleanup();
  }
});

test('check: 키가 없으면 첫 줄이 UNVERIFIED|no Jev key 이고 종료코드 6 (통과로 읽을 수 없다)', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'board.png');
    const r = await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge: new FakeJudge({ check: { status: 'UNVERIFIED', reason: 'no Jev key' } }) });
    assert.deepEqual(r.out, ['UNVERIFIED|no Jev key']);
    assert.equal(r.code, 6);
    assert.ok(r.err.some((l) => l.includes('not calibrated')), '보정 안 됨 안내는 stderr');
  } finally {
    t.cleanup();
  }
});

test('check: --require-calibrated 면 호출 없이 UNVERIFIED|family not calibrated', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'board.png');
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'x' } });
    const r = await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo, '--require-calibrated'], { judge: fake });
    assert.deepEqual([r.code, r.out[0], fake.calls.length], [6, 'UNVERIFIED|family not calibrated', 0]);
  } finally {
    t.cleanup();
  }
});

test('가드: e2e 밖 이미지는 GUARD| 와 종료코드 5, 판정자는 불리지 않는다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.repo, 'outside.png');
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'x' } });
    const r = await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge: fake });
    assert.equal(r.code, 5);
    assert.match(r.out[0], /^GUARD\|/);
    assert.equal(fake.calls.length, 0);
  } finally {
    t.cleanup();
  }
});

test('가드: PNG 가 아닌 파일·너무 큰 파일은 GUARD', async () => {
  const t = tempRepo();
  try {
    const text = path.join(t.out, 'text.png');
    fs.writeFileSync(text, 'plain text pretending to be a screenshot, long enough for a header');
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'x' } });
    const r = await run(['check', '--image', text, '--family', 'chip', '--repo', t.repo], { judge: fake });
    assert.equal(r.code, 5);
    const big = path.join(t.out, 'big.png');
    fs.writeFileSync(big, Buffer.concat([fs.readFileSync(writePng(t.out, 'ok.png')), Buffer.alloc(16 * 1024 * 1024 + 1)]));
    assert.equal((await run(['check', '--image', big, '--family', 'chip', '--repo', t.repo], { judge: fake })).code, 5);
    assert.equal(fake.calls.length, 0);
  } finally {
    t.cleanup();
  }
});

test('사용법 오류: 종료코드 2 와 USAGE|', async () => {
  for (const argv of [[], ['nope'], ['check'], ['check', '--image', 'x'], ['check', '--image', 'x', '--family', 'nonexistent'], ['check', '--bogus'], ['screen', '--image', 'x'], ['crop'], ['report'], ['measure']]) {
    const r = await run(argv, { judge: new FakeJudge() });
    assert.equal(r.code, 2, JSON.stringify(argv));
    assert.match(r.out[0], /^USAGE\|/);
  }
});

test('screen: 고른 화면을 CHOICE| 로 낸다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'board.png');
    const fake = new FakeJudge({ screen: { status: 'PASS', reason: 'screen=settings', choice: 'settings' } });
    const r = await run(['screen', '--image', image, '--choices', 'settings,board-expanded', '--expect', 'settings', '--repo', t.repo], { judge: fake });
    assert.deepEqual([r.code, r.out], [0, ['PASS|screen=settings', 'CHOICE|settings']]);
    assert.deepEqual(fake.calls[0].choices, ['settings', 'board-expanded']);
  } finally {
    t.cleanup();
  }
});

test('판정 캐시: 같은 화면을 두 번 물어도 판정자는 한 번 불린다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'board.png');
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'jev pass' } });
    await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge: fake });
    await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge: fake });
    assert.equal(fake.calls.length, 1);
    assert.ok(fs.readdirSync(path.join(t.out, 'jev-cache')).some((n) => n.startsWith('v-')));
  } finally {
    t.cleanup();
  }
});

test('crop: 사각형으로 잘라 e2e/out 에 쓴다. 밖에는 쓰지 않는다', async () => {
  const t = tempRepo();
  try {
    const source = writePng(t.root, 'fullscreen.png', 400, 300);
    const out = path.join(t.out, 'cropped.png');
    const r = await run(['crop', '--in', source, '--rect', '20,10,300,280', '--out', out, '--repo', t.repo]);
    assert.equal(r.code, 0);
    assert.match(r.out[0], /^CROP\|.*cropped\.png\|300x280$/);
    const image = decode(fs.readFileSync(out));
    assert.deepEqual([image.width, image.height], [300, 280]);
    const bad = await run(['crop', '--in', source, '--rect', '20,10,300,280', '--out', path.join(t.repo, 'bad.png'), '--repo', t.repo]);
    assert.equal(bad.code, 5);
    assert.equal(fs.existsSync(path.join(t.repo, 'bad.png')), false);
    const outside = await run(['crop', '--in', source, '--rect', '300,10,300,280', '--out', out, '--repo', t.repo]);
    assert.equal(outside.code, 5, '이미지 밖 사각형');
    const notNumbers = await run(['crop', '--in', source, '--rect', '1,2,three,4', '--out', out, '--repo', t.repo]);
    assert.equal(notNumbers.code, 5);
  } finally {
    t.cleanup();
  }
});

test('crop --window-title 과 check --allowed-window-title: 제목이 같을 때만 판정자에게 간다(다르면 GUARD 5)', async () => {
  const t = tempRepo();
  try {
    const source = writePng(t.root, 'fullscreen.png', 400, 300);
    const out = path.join(t.out, 'board.png');
    assert.equal((await run(['crop', '--in', source, '--rect', '20,10,300,280', '--out', out, '--window-title', 'Claude Board', '--repo', t.repo])).code, 0);
    assert.equal(JSON.parse(fs.readFileSync(`${out}.manifest.json`, 'utf8')).windowTitle, 'Claude Board');
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'jev pass' } });
    const good = await run(['check', '--image', out, '--family', 'chip', '--allowed-window-title', 'Claude Board', '--repo', t.repo], { judge: fake });
    assert.deepEqual([good.code, fake.calls.length], [0, 1]);
    const bad = await run(['check', '--image', out, '--family', 'chip', '--allowed-window-title', 'Notepad', '--repo', t.repo], { judge: fake });
    assert.equal(bad.code, 5);
    assert.match(bad.out[0], /^GUARD\|/);
    assert.equal(fake.calls.length, 1);
  } finally {
    t.cleanup();
  }
});

test('개인정보 가드: 창 제목에 사용자 이름이 있으면 GUARD 5, 판정자는 불리지 않는다', async () => {
  const t = tempRepo();
  try {
    const source = writePng(t.root, 'fullscreen.png', 400, 300);
    const out = path.join(t.out, 'board.png');
    await run(['crop', '--in', source, '--rect', '20,10,300,280', '--out', out, '--window-title', 'Board of Someone', '--repo', t.repo]);
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'x' } });
    const r = await run(['check', '--image', out, '--family', 'chip', '--allowed-window-title', 'Board of Someone', '--repo', t.repo], { judge: fake, userName: 'someone' });
    assert.equal(r.code, 5);
    assert.match(r.out[0], /^GUARD\|privacy guard: user name/);
    assert.equal(fake.calls.length, 0);
  } finally {
    t.cleanup();
  }
});

test('report: rows JSON 을 표로 낸다', async () => {
  const t = tempRepo();
  try {
    const rows = path.join(t.root, 'rows.json');
    fs.writeFileSync(rows, JSON.stringify([{ name: 'chip', deterministic: 'PASS', jev: { status: 'UNVERIFIED', reason: 'no Jev key' } }]));
    const r = await run(['report', '--rows', rows]);
    assert.equal(r.code, 0);
    assert.ok(r.out.some((l) => l.includes('UNVERIFIED: no Jev key')));
  } finally {
    t.cleanup();
  }
});

test('종단: 가짜 하위 프로세스(스텁)로 check 가 UNVERIFIED|no Jev key / 6 을 낸다', async () => {
  const t = tempRepo();
  try {
    const image = writePng(t.out, 'board.png');
    const stub = path.join(path.dirname(fileURLToPath(import.meta.url)), 'testkit', 'stub-cli.mjs');
    const { JevLookJudge } = await import('./adapters/jevlook.mjs');
    const judge = new JevLookJudge({
      repo: t.repo,
      cacheDir: path.join(t.out, 'jev-cache', 'harness'),
      command: { file: process.execPath, prefixArgs: [stub] },
      env: { ...process.env, JEV_STUB: 'nokey' },
    });
    const r = await run(['check', '--image', image, '--family', 'chip', '--repo', t.repo], { judge });
    assert.deepEqual([r.code, r.out], [6, ['UNVERIFIED|no Jev key']]);
  } finally {
    t.cleanup();
  }
});

test('jev.mjs 를 실제 프로세스로: 없는 이미지는 GUARD 와 종료코드 5 (파이썬·Jev 를 부르기 전)', () => {
  const entry = path.join(path.dirname(fileURLToPath(import.meta.url)), 'jev.mjs');
  const done = cp.spawnSync(process.execPath, [entry, 'check', '--image', path.join(process.cwd(), 'nope.png'), '--family', 'chip'], { encoding: 'utf8' });
  assert.equal(done.status, 5);
  assert.match(done.stdout, /^GUARD\|/);
});
