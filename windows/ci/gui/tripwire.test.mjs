// tripwire.ps1 시험 — 임시 폴더의 사본과 임시 프로세스만 쓴다. 실제 settings.json·profile.ps1·진짜 claude 는 건드리지도 읽지도 않는다.
// 실행: node --test windows/ci/gui/tripwire.test.mjs   (pwsh 가 필요하다. Windows PowerShell 5.1 이 있으면 selftest 를 거기서도 돈다)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import cp from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'tripwire.ps1');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const has = (exe) => cp.spawnSync(exe, ['-NoProfile', '-Command', '1'], { encoding: 'utf8' }).status === 0;
const HAS_PWSH = has('pwsh');
const HAS_PS51 = process.platform === 'win32' && has('powershell');

function run(shell, args) {
  const result = cp.spawnSync(shell, ['-NoProfile', '-File', SCRIPT, ...args], { encoding: 'utf8' });
  return { code: result.status, text: `${result.stdout}${result.stderr}` };
}

// 지킬 파일 사본과 상태 파일 자리
function fixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tripwire-test-'));
  const settings = path.join(dir, 'home', '.claude', 'settings.json');
  const profile = path.join(dir, 'docs', 'PowerShell', 'profile.ps1');
  fs.mkdirSync(path.dirname(settings), { recursive: true });
  fs.mkdirSync(path.dirname(profile), { recursive: true });
  fs.writeFileSync(settings, '{"secret-marker-settings":1}');
  fs.writeFileSync(profile, '# secret-marker-profile');
  const state = path.join(dir, 'state', 'state.json');
  const watch = ['-State', state, '-SettingsPath', settings, '-ProfilePaths', profile];
  return { dir, settings, profile, state, watch, cleanup: () => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 50 }) };
}

for (const [label, enabled, shell] of [['pwsh', HAS_PWSH, 'pwsh'], ['Windows PowerShell 5.1', HAS_PS51, 'powershell']]) {
  test(`selftest(${label}) — 임시 사본의 변화를 잡고 안 바뀐 것은 통과시킨다`, { skip: !enabled }, () => {
    const r = run(shell, ['-Phase', 'selftest']);
    assert.equal(r.code, 0, r.text);
    assert.match(r.text, /TRIPWIRE\|SELFTEST\|PASS/);
    assert.match(r.text, /TRIPWIRE\|CHANGED\|profile:PowerShell\|/);
    assert.doesNotMatch(r.text, /selftest profile/, '파일 내용을 출력하지 않는다');
  });
}

test('before → after: 그대로면 0, 설정 한 글자만 바뀌어도 1 과 CHANGED 줄(해시만 나온다)', { skip: !HAS_PWSH }, () => {
  const f = fixture();
  try {
    const before = runIn(f, ['-Phase', 'before', ...f.watch]);
    assert.equal(before.code, 0, before.text);
    assert.match(before.text, /TRIPWIRE\|RECORDED\|settings\.json\|[0-9a-f]{32}/);
    const same = runIn(f, ['-Phase', 'after', ...f.watch.slice(0, 2)]);
    assert.equal(same.code, 0, same.text);
    assert.match(same.text, /TRIPWIRE\|SAME\|settings\.json\|/);
    fs.appendFileSync(f.settings, ' ');
    const changed = runIn(f, ['-Phase', 'after', '-State', f.state]);
    assert.equal(changed.code, 1, changed.text);
    assert.match(changed.text, /TRIPWIRE\|CHANGED\|settings\.json\|[0-9a-f]{32}\|[0-9a-f]{32}/);
    for (const text of [before.text, same.text, changed.text]) {
      assert.doesNotMatch(text, /secret-marker/, '파일 내용을 출력하지 않는다');
    }
  } finally {
    f.cleanup();
  }
});

test('지운 파일·새로 생긴 파일도 바뀐 것이다(ABSENT)', { skip: !HAS_PWSH }, () => {
  const f = fixture();
  try {
    fs.rmSync(f.profile);
    assert.equal(runIn(f, ['-Phase', 'before', ...f.watch]).code, 0);
    assert.equal(runIn(f, ['-Phase', 'after', '-State', f.state]).code, 0, '없던 파일이 계속 없으면 같다');
    fs.writeFileSync(f.profile, '# 새로 생김');
    const created = runIn(f, ['-Phase', 'after', '-State', f.state]);
    assert.equal(created.code, 1, created.text);
    assert.match(created.text, /TRIPWIRE\|CHANGED\|profile:PowerShell\|ABSENT\|[0-9a-f]{32}/);
  } finally {
    f.cleanup();
  }
});

test('사용법 오류는 2 — 상태 파일 없음, -State 없음, 상태 파일이 지키는 폴더 안', { skip: !HAS_PWSH }, () => {
  const f = fixture();
  try {
    assert.equal(runIn(f, ['-Phase', 'after', '-State', path.join(f.dir, 'none.json')]).code, 2);
    assert.equal(runIn(f, ['-Phase', 'before']).code, 2);
    const inside = path.join(path.dirname(f.settings), 'state.json');
    const r = runIn(f, ['-Phase', 'before', '-State', inside, '-SettingsPath', f.settings, '-ProfilePaths', f.profile]);
    assert.equal(r.code, 2, r.text);
    assert.equal(fs.existsSync(inside), false);
  } finally {
    f.cleanup();
  }
});

// ── 프로세스: 기준선 비교 — before 뒤에 새로 뜬 claude 가 허용 폴더 밖이면 실패 ─────────────────────────────

// node.exe 사본(하드 링크가 되면 링크) — 이름을 claude.exe·board.exe 로 둔다
function nodeCopy(target) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  try {
    fs.linkSync(process.execPath, target);
  } catch {
    fs.copyFileSync(process.execPath, target);
  }
  return target;
}

const killTree = (pid) => cp.spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { encoding: 'utf8' });
const killOne = (pid) => cp.spawnSync('taskkill', ['/PID', String(pid), '/F'], { encoding: 'utf8' });
const WAIT = ['-e', 'setTimeout(()=>{}, 60000)'];

// 보드를 모르는 단독 프로세스 — 시험이 끝나면 killTree 로 끈다
async function startAlone(exe, args = WAIT) {
  const child = cp.spawn(exe, args, { stdio: 'ignore', windowsHide: true });
  await sleep(500);
  return child.pid;
}

// 보드 역할 프로세스가 claudeExe 를 자식으로 띄운다. 자식 pid 는 파일로 받는다
async function startBoardWith(dir, claudeExe) {
  const board = nodeCopy(path.join(dir, 'board', 'board.exe'));
  const childPidFile = path.join(dir, 'child.pid');
  const boardScript = path.join(dir, 'board.cjs');
  fs.writeFileSync(
    boardScript,
    `const cp = require('child_process'), fs = require('fs');
const c = cp.spawn(${JSON.stringify(claudeExe)}, ['-e', 'setTimeout(()=>{}, 60000)'], { stdio: 'ignore', detached: true }); // detached: 보드가 죽어도 자식이 따라 죽지 않게(libuv 작업 개체에서 빠진다)
fs.writeFileSync(${JSON.stringify(childPidFile)}, String(c.pid));
setTimeout(() => {}, 60000);`,
  );
  const boardProcess = cp.spawn(board, [boardScript], { stdio: 'ignore', windowsHide: true });
  const deadline = Date.now() + 8000;
  while (!fs.existsSync(childPidFile)) {
    if (Date.now() > deadline) throw new Error('자식이 안 떴다');
    await sleep(30);
  }
  await sleep(300);
  return { boardPid: boardProcess.pid, childPid: Number(fs.readFileSync(childPidFile, 'utf8')) };
}

const winOnly = { skip: !HAS_PWSH || process.platform !== 'win32' };
// 이 PC 에는 실제 claude 세션이 늘 새로 뜬다 — 시험 폴더 아래 실행 파일만 보게 좁힌다(-TestScopeUnder)
const runIn = (f, args) => run('pwsh', [...args, '-TestScopeUnder', f.dir]);
const realLine = (pid, tail) => new RegExp(`TRIPWIRE\\|REAL-CLAUDE\\|${pid}\\|${tail}`);

test('BoardPid=0 으로 before 를 돌려도(보드가 아직 없다) 그 뒤에 새로 뜬 허용 폴더 밖 claude.exe 는 1(REAL-CLAUDE)', winOnly, async () => {
  const f = fixture();
  const fakeDir = path.join(f.dir, 'fake');
  const started = [];
  try {
    assert.equal(runIn(f, ['-Phase', 'before', ...f.watch, '-FakeDir', fakeDir]).code, 0);
    const pid = await startAlone(nodeCopy(path.join(f.dir, 'realish', 'claude.exe')));
    started.push(pid);
    const bad = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(bad.code, 1, bad.text);
    assert.match(bad.text, realLine(pid, '[^|]*claude\\.exe\\|exe'));
    // after 도 같다 — 해시가 그대로여도 새 claude 가 있으면 실패
    assert.equal(runIn(f, ['-Phase', 'after', '-State', f.state]).code, 1);
  } finally {
    for (const pid of started) killTree(pid);
    await sleep(300);
    f.cleanup();
  }
});

test('허용 폴더 안(가짜) claude.exe 는 새로 떠도 0, 접두어만 같은 형제 폴더는 허용이 아니다', winOnly, async () => {
  const f = fixture();
  const fakeDir = path.join(f.dir, 'fake');
  const started = [];
  try {
    assert.equal(runIn(f, ['-Phase', 'before', ...f.watch, '-FakeDir', fakeDir]).code, 0);
    const pid = await startAlone(nodeCopy(path.join(fakeDir, 'fake-bin', 'claude.exe')));
    started.push(pid);
    const good = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(good.code, 0, good.text);
    assert.doesNotMatch(good.text, new RegExp(`\\|${pid}\\|`), '허용 폴더 안은 목록에도 없다');
    const sibling = runIn(f, ['-Phase', 'check', '-State', f.state, '-FakeDir', path.join(f.dir, 'fak')]);
    assert.equal(sibling.code, 1, sibling.text);
  } finally {
    for (const pid of started) killTree(pid);
    await sleep(300);
    f.cleanup();
  }
});

test('before 때 이미 돌던 claude 는 참고(OTHER-CLAUDE)일 뿐 실패가 아니다', winOnly, async () => {
  const f = fixture();
  const started = [];
  try {
    const pid = await startAlone(nodeCopy(path.join(f.dir, 'realish', 'claude.exe')));
    started.push(pid);
    const before = runIn(f, ['-Phase', 'before', ...f.watch]);
    assert.equal(before.code, 0, before.text);
    assert.match(before.text, /TRIPWIRE\|BASELINE\|[1-9]/);
    const check = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(check.code, 0, check.text);
    assert.match(check.text, new RegExp(`TRIPWIRE\\|OTHER-CLAUDE\\|${pid}\\|`));
    assert.doesNotMatch(check.text, /REAL-CLAUDE/);
  } finally {
    for (const pid of started) killTree(pid);
    await sleep(300);
    f.cleanup();
  }
});

test('node 로 도는 claude(npm 설치: 명령줄에 @anthropic-ai/claude-code)도 잡는다 — 다른 node 스크립트는 안 잡는다', winOnly, async () => {
  const f = fixture();
  const started = [];
  try {
    assert.equal(runIn(f, ['-Phase', 'before', ...f.watch]).code, 0);
    const npmScript = path.join(f.dir, 'npm', 'node_modules', '@anthropic-ai', 'claude-code', 'cli.js');
    fs.mkdirSync(path.dirname(npmScript), { recursive: true });
    fs.writeFileSync(npmScript, 'setTimeout(() => {}, 60000);');
    const plain = path.join(f.dir, 'plain.js');
    fs.writeFileSync(plain, 'setTimeout(() => {}, 60000);');
    const nodeHere = nodeCopy(path.join(f.dir, 'node-bin', 'node.exe')); // 시험 폴더 안의 node 사본 — 이름은 node.exe 그대로다
    const plainPid = await startAlone(nodeHere, [plain]);
    started.push(plainPid);
    const harmless = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(harmless.code, 0, harmless.text);
    const pid = await startAlone(nodeHere, [npmScript]);
    started.push(pid);
    const bad = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(bad.code, 1, bad.text);
    assert.match(bad.text, realLine(pid, '[^|]*node\\.exe\\|node'));
    assert.doesNotMatch(bad.text, /cli\.js/, '명령줄은 출력하지 않는다');
    assert.doesNotMatch(bad.text, realLine(plainPid, ''));
  } finally {
    for (const pid of started) killTree(pid);
    await sleep(300);
    f.cleanup();
  }
});

test('보드가 이미 끝났어도(자식 claude 만 남았다) 잡는다 — 보드가 살아 있으면 childOfBoard=True 표시만 붙는다', winOnly, async () => {
  const f = fixture();
  const fakeDir = path.join(f.dir, 'fake');
  const started = [];
  try {
    const record = path.join(f.dir, 'board.json');
    assert.equal(runIn(f, ['-Phase', 'before', ...f.watch, '-FakeDir', fakeDir, '-BoardFile', record]).code, 0);
    const real = await startBoardWith(f.dir, nodeCopy(path.join(f.dir, 'realish', 'claude.exe')));
    started.push(real.childPid, real.boardPid);
    fs.writeFileSync(record, JSON.stringify({ pid: real.boardPid, fakeDir }));
    const alive = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(alive.code, 1, alive.text);
    assert.match(alive.text, realLine(real.childPid, '.*childOfBoard=True'));
    killOne(real.boardPid); // 보드만 끈다 — 자식 claude 는 남는다
    await sleep(500);
    const gone = runIn(f, ['-Phase', 'after', '-State', f.state]);
    assert.equal(gone.code, 1, gone.text);
    assert.match(gone.text, realLine(real.childPid, '.*childOfBoard=False'));
  } finally {
    for (const pid of started) killTree(pid);
    await sleep(300);
    f.cleanup();
  }
});

test('보드를 먼저 알리고 before 를 돌렸을 때 그 보드 밑에 진짜 claude 가 이미 있으면 before 가 1', winOnly, async () => {
  const f = fixture();
  const started = [];
  try {
    const real = await startBoardWith(f.dir, nodeCopy(path.join(f.dir, 'realish', 'claude.exe')));
    started.push(real.boardPid);
    const before = runIn(f, ['-Phase', 'before', ...f.watch, '-BoardPid', String(real.boardPid)]);
    assert.equal(before.code, 1, before.text);
    assert.match(before.text, realLine(real.childPid, ''));
  } finally {
    for (const pid of started) killTree(pid);
    await sleep(300);
    f.cleanup();
  }
});

test('기준선 없는 옛 상태 파일은 2 — 닫힌 채로 실패한다', { skip: !HAS_PWSH }, () => {
  const f = fixture();
  try {
    assert.equal(runIn(f, ['-Phase', 'before', ...f.watch]).code, 0);
    const doc = JSON.parse(fs.readFileSync(f.state, 'utf8'));
    delete doc.claudeBaseline;
    fs.writeFileSync(f.state, JSON.stringify(doc));
    const r = runIn(f, ['-Phase', 'check', '-State', f.state]);
    assert.equal(r.code, 2, r.text);
    assert.match(r.text, /TRIPWIRE\|USAGE\|.*기준선/);
  } finally {
    f.cleanup();
  }
});
