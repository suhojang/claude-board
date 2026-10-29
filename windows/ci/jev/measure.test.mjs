import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { loadDataset, measure, measureLines, sizeProblems } from './measure.mjs';
import { FakeJudge } from './adapters/fakeJudge.mjs';
import { main } from './cli.mjs';
import { tempRepo, writePng } from './testkit/helpers.mjs';

function dataset(t, { positives, negatives, family = 'chip' }) {
  const dir = path.join(t.out, 'calib');
  fs.mkdirSync(dir, { recursive: true });
  const truth = {};
  for (let i = 0; i < positives + negatives; i += 1) {
    const name = `s${i}.png`;
    writePng(dir, name, 300 + i, 300);
    truth[name] = { family, truth: i < positives ? 'yes' : 'no' };
  }
  fs.writeFileSync(path.join(dir, 'truth.json'), JSON.stringify(truth));
  return dir;
}

// 정답을 아는 가짜 판정자: truth 가 yes 인 그림에만 PASS, 나머지는 FAIL
const oracle = (dir) => {
  const truth = JSON.parse(fs.readFileSync(path.join(dir, 'truth.json'), 'utf8'));
  return (imagePath) => {
    const entry = truth[path.basename(imagePath)];
    return { status: entry.truth === 'yes' ? 'PASS' : 'FAIL', reason: 'jev' };
  };
};

test('loadDataset: truth.json 을 읽고 잘못된 가족·정답은 던진다', () => {
  const t = tempRepo();
  try {
    const dir = dataset(t, { positives: 2, negatives: 1 });
    assert.deepEqual(loadDataset(dir).map((i) => [path.basename(i.file), i.family, i.truth]), [['s0.png', 'chip', 'yes'], ['s1.png', 'chip', 'yes'], ['s2.png', 'chip', 'no']]);
    fs.writeFileSync(path.join(dir, 'truth.json'), JSON.stringify({ 'a.png': { family: 'nope', truth: 'yes' } }));
    assert.throws(() => loadDataset(dir), /알 수 없는 가족/);
    fs.writeFileSync(path.join(dir, 'truth.json'), JSON.stringify({ 'a.png': { family: 'chip', truth: 'maybe' } }));
    assert.throws(() => loadDataset(dir), /truth/);
    assert.throws(() => loadDataset(path.join(t.out, 'missing')), /truth.json/);
  } finally {
    t.cleanup();
  }
});

test('sizeProblems: 가족마다 양성·음성 30 장 미만이면 알린다', () => {
  const items = [...Array(30).fill({ family: 'chip', truth: 'yes' }), ...Array(29).fill({ family: 'chip', truth: 'no' })];
  assert.deepEqual(sizeProblems(items), ['chip pos 30/30 neg 29/30']);
  assert.deepEqual(sizeProblems([...items, { family: 'chip', truth: 'no' }]), []);
});

test('데이터셋이 작으면 호출 전에 UNVERIFIED 로 끝난다', async () => {
  const t = tempRepo();
  try {
    const fake = new FakeJudge({ check: { status: 'PASS', reason: 'x' } });
    const result = await measure({ judge: fake, items: loadDataset(dataset(t, { positives: 3, negatives: 3 })) });
    assert.equal(result.verdict.status, 'UNVERIFIED');
    assert.equal(result.verdict.reason, 'dataset too small');
    assert.equal(fake.calls.length, 0);
  } finally {
    t.cleanup();
  }
});

test('키가 없으면 첫 그림에서 중단하고 UNVERIFIED (일부만 센 숫자를 내지 않는다)', async () => {
  const t = tempRepo();
  try {
    const dir = dataset(t, { positives: 30, negatives: 30 });
    const fake = new FakeJudge({ check: { status: 'UNVERIFIED', reason: 'no Jev key' } });
    const result = await measure({ judge: fake, items: loadDataset(dir) });
    assert.deepEqual([result.verdict.status, result.verdict.reason, result.families], ['UNVERIFIED', 'no Jev key', undefined]);
    assert.equal(fake.calls.length, 1);
  } finally {
    t.cleanup();
  }
});

test('끝까지 돌면 가족별 요약과 MEASURE 줄을 낸다', async () => {
  const t = tempRepo();
  try {
    const dir = dataset(t, { positives: 30, negatives: 30 });
    const result = await measure({ judge: new FakeJudge({ check: oracle(dir) }), items: loadDataset(dir) });
    assert.deepEqual(result.families.chip.problems, []);
    const [line] = measureLines(result.families);
    assert.match(line, /^MEASURE\|family=chip\|pos=30\|neg=30\|hit=1\.00\|accuracy=1\.00\|wilsonLower=0\.9\d\|abstain=0\.00\|falsePass=0\.000\|trust=yes$/);
  } finally {
    t.cleanup();
  }
});

test('거짓 통과가 많으면 trust=no 와 사유', async () => {
  const t = tempRepo();
  try {
    const dir = dataset(t, { positives: 30, negatives: 30 });
    const truth = JSON.parse(fs.readFileSync(path.join(dir, 'truth.json'), 'utf8'));
    const yesMan = new FakeJudge({ check: (imagePath) => ({ status: truth[path.basename(imagePath)].truth === 'no' && Number(path.basename(imagePath).slice(1, -4)) < 35 ? 'PASS' : oracle(dir)(imagePath).status, reason: 'jev' }) });
    const result = await measure({ judge: yesMan, items: loadDataset(dir) });
    assert.match(measureLines(result.families)[0], /trust=no\(.*falsePassRate/);
  } finally {
    t.cleanup();
  }
});

test('CLI measure: 키가 없는 판정자 -> UNVERIFIED|no Jev key 와 종료코드 6', async () => {
  const t = tempRepo();
  try {
    const dir = dataset(t, { positives: 30, negatives: 30 });
    const out = [];
    const code = await main(['measure', '--dataset', dir, '--repo', t.repo], {
      out: (l) => out.push(l),
      err: () => {},
      judge: new FakeJudge({ check: { status: 'UNVERIFIED', reason: 'no Jev key' } }),
    });
    assert.deepEqual([code, out[0]], [6, 'UNVERIFIED|no Jev key']);
  } finally {
    t.cleanup();
  }
});
