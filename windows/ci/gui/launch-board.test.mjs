// launch-board.ps1 시험 — 진짜 보드·GUI 없이, node 사본을 ClaudeBoard.exe 로 둔 "보드 대역"(testkit/standin-board.mjs)으로 확인 절차를 돌린다.
// 실제 사용자 홈·settings.json·진짜 claude 는 건드리지 않는다. 시험 폴더는 임시 폴더다.
// 실행: node --test windows/ci/gui/launch-board.test.mjs   (pwsh 필요, Windows 전용)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import cp from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const LAUNCH = path.join(HERE, 'launch-board.ps1');
const STANDIN = path.join(HERE, 'testkit', 'standin-board.mjs');
const FAKE_SCRIPT = path.join(HERE, '..', 'fake-claude-tty.cjs');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const has = (exe) => cp.spawnSync(exe, ['-NoProfile', '-Command', '1'], { encoding: 'utf8' }).status === 0;
const SKIP = process.platform !== 'win32' || !has('pwsh');

const alive = (pid) => cp.spawnSync('tasklist', ['/FI', `PID eq ${pid}`, '/NH'], { encoding: 'utf8' }).stdout.includes(String(pid));
const killTree = (pid) => cp.spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { encoding: 'utf8' });

// node 사본(하드 링크가 되면 링크) — 이름을 ClaudeBoard.exe 로 둔다
function boardExe(dir) {
  const target = path.join(dir, 'bin', 'ClaudeBoard.exe');
  fs.mkdirSync(path.dirname(target), { recursive: true });
  try {
    fs.linkSync(process.execPath, target);
  } catch {
    fs.copyFileSync(process.execPath, target);
  }
  return target;
}

function workspace() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'launch-board-test-'));
  const root = path.join(dir, 'run');
  const started = [];
  return {
    dir,
    root,
    exe: boardExe(dir),
    envOut: path.join(dir, 'env.json'),
    started,
    cleanup: async () => {
      for (const pid of started) killTree(pid);
      await sleep(300);
      fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 50 });
    },
  };
}

// 출력은 파일로 받는다 — 파이프를 쓰면 보드(대역)가 핸들을 쥔 채 살아 있을 때 기다리게 되는지까지 이 시험이 본다
function launch(ws, extra = [], env = {}, shell = 'pwsh') {
  const outFile = path.join(ws.dir, `out-${Math.random().toString(16).slice(2)}.txt`);
  const fd = fs.openSync(outFile, 'w');
  const args = ['-NoProfile', '-File', LAUNCH, '-Root', ws.root, '-Exe', ws.exe, '-ExeArgs', STANDIN, '-HiddenWindow', '-TimeoutSec', '25', ...extra];
  const result = cp.spawnSync(shell, args, { stdio: ['ignore', fd, fd], env: Object.assign({}, process.env, { STANDIN_ENV_OUT: ws.envOut }, env), timeout: 120000 });
  fs.closeSync(fd);
  const text = fs.readFileSync(outFile, 'utf8');
  const started = /started pid=(\d+)/.exec(text)?.[1]; // 실패한 실행이 남긴 보드(대역)도 치우려고 단계 줄에서도 읽는다
  if (started) ws.started.push(Number(started));
  const pid = /BOARD_PID=(\d+)/.exec(text)?.[1];
  if (pid) ws.started.push(Number(pid));
  return { code: result.status, text, pid: pid ? Number(pid) : null };
}

test('성공 — 새 프로세스 pid·포트를 내고, 격리 환경과 가짜 claude 파일을 만든다. -Stop 은 기록한 보드만 끈다', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    const r = launch(ws, ['-AllowOtherBoards'], { CLAUDE_BOARD_NOTIFIER_DIR: 'C:\\inherited\\notifier', CLAUDE_CONFIG_DIR: 'C:\\inherited\\cfg' });
    assert.equal(r.code, 0, r.text);
    assert.match(r.text, /LAUNCH\|OK\|board=\d+\|port=\d+/);
    assert.ok(r.pid && alive(r.pid), '새 보드 프로세스가 살아 있다');

    const record = JSON.parse(fs.readFileSync(path.join(ws.root, 'board.json'), 'utf8'));
    assert.equal(record.pid, r.pid);
    assert.equal(record.exe.toLowerCase(), ws.exe.toLowerCase());
    const prefix = `${path.join(ws.root, 'fake', 'fake-bin', 'claude.exe')}|${path.join(ws.root, 'fake', 'fake-claude-tty.cjs')}`;
    assert.equal(record.prefix, prefix);
    assert.match(r.text, new RegExp(`CDP_PORT=${record.port}`));

    // 보드 프로세스가 받은 환경 — 격리 값만, 물려받은 것은 지워진다
    const env = JSON.parse(fs.readFileSync(ws.envOut, 'utf8'));
    assert.equal(env.CLAUDE_BOARD_HOME, path.join(ws.root, 'home'));
    assert.equal(env.WEBVIEW2_USER_DATA_FOLDER, path.join(ws.root, 'wv2'));
    assert.equal(env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, `--remote-debugging-port=${record.port}`);
    assert.equal(env.CLAUDE_BOARD_CLAUDE, prefix);
    assert.equal(env.CLAUDE_BOARD_NOTIFIER_DIR, null);
    assert.equal(env.CLAUDE_CONFIG_DIR, null);

    // D6 환경 파일 + 가짜 claude 사본
    const fakeEnv = JSON.parse(fs.readFileSync(path.join(ws.root, 'fake', 'fake-claude.env.json'), 'utf8'));
    assert.equal(fakeEnv.CLAUDE_BOARD_HOME, path.join(ws.root, 'home'));
    assert.equal(fakeEnv.FAKE_CLAUDE_KEYLOG, path.join(ws.root, 'logs', 'keys.log'));
    assert.equal(fakeEnv.FAKE_CLAUDE_ARGVLOG, path.join(ws.root, 'logs', 'argv.log'));
    assert.deepEqual(fs.readFileSync(path.join(ws.root, 'fake', 'fake-claude-tty.cjs')), fs.readFileSync(FAKE_SCRIPT));
    assert.equal(fs.statSync(path.join(ws.root, 'fake', 'fake-bin', 'claude.exe')).size, fs.statSync(process.execPath).size);
    assert.ok(fs.existsSync(path.join(ws.root, 'home', '.claude')));

    // 다른 프로세스는 건드리지 않고 기록한 보드만 끈다
    const bystander = cp.spawn(process.execPath, ['-e', 'setTimeout(()=>{}, 60000)'], { stdio: 'ignore' });
    ws.started.push(bystander.pid);
    const stopped = launch(ws, ['-Stop']);
    assert.equal(stopped.code, 0, stopped.text);
    await sleep(500);
    assert.equal(alive(r.pid), false);
    assert.equal(alive(bystander.pid), true);
  } finally {
    await ws.cleanup();
  }
});

test('-FakeEnv 로 더한 값이 가짜 claude 환경 파일에 들어간다', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    // -FakeEnv 는 해시 테이블이라 -Command 로 부른다
    const outFile = path.join(ws.dir, 'out-fakeenv.txt');
    const fd = fs.openSync(outFile, 'w');
    const script = `& '${LAUNCH}' -Root '${ws.root}' -Exe '${ws.exe}' -ExeArgs '${STANDIN}' -HiddenWindow -AllowOtherBoards -TimeoutSec 25 -FakeEnv @{ FAKE_CLAUDE_MAX_MS = '5000'; FAKE_CLAUDE_HOOKS = '0' }; exit $LASTEXITCODE`;
    const result = cp.spawnSync('pwsh', ['-NoProfile', '-Command', script], { stdio: ['ignore', fd, fd], timeout: 120000 });
    fs.closeSync(fd);
    const text = fs.readFileSync(outFile, 'utf8');
    const pid = /BOARD_PID=(\d+)/.exec(text)?.[1];
    if (pid) ws.started.push(Number(pid));
    assert.equal(result.status, 0, text);
    const fakeEnv = JSON.parse(fs.readFileSync(path.join(ws.root, 'fake', 'fake-claude.env.json'), 'utf8'));
    assert.equal(fakeEnv.FAKE_CLAUDE_MAX_MS, '5000');
    assert.equal(fakeEnv.FAKE_CLAUDE_HOOKS, '0');
    assert.equal(fakeEnv.CLAUDE_BOARD_HOME, path.join(ws.root, 'home'));
  } finally {
    await ws.cleanup();
  }
});

test('Windows PowerShell 5.1 에서도 같은 결과', { skip: SKIP || !has('powershell') }, async () => {
  const ws = workspace();
  try {
    const r = launch(ws, ['-AllowOtherBoards'], {}, 'powershell');
    assert.equal(r.code, 0, r.text);
    assert.match(r.text, /LAUNCH\|OK\|/);
    const stopped = launch(ws, ['-Stop'], {}, 'powershell');
    assert.equal(stopped.code, 0, stopped.text);
  } finally {
    await ws.cleanup();
  }
});

test('삼켜짐 — 새 프로세스가 바로 끝나면 4 와 SWALLOWED, 남의 보드는 건드리지 않는다', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    const other = cp.spawn(ws.exe, [STANDIN], { stdio: 'ignore', env: Object.assign({}, process.env, { STANDIN_MODE: 'silent' }) });
    ws.started.push(other.pid);
    await sleep(500);
    const r = launch(ws, ['-AllowOtherBoards'], { STANDIN_MODE: 'swallow' });
    assert.equal(r.code, 4, r.text);
    assert.match(r.text, /LAUNCH\|SWALLOWED\|/);
    assert.equal(r.pid, null);
    assert.equal(alive(other.pid), true, '이미 떠 있던 보드는 그대로 산다');
  } finally {
    await ws.cleanup();
  }
});

test('다른 ClaudeBoard.exe 가 떠 있으면 -AllowOtherBoards 없이는 3 으로 거절하고 아무것도 띄우지·끄지 않는다', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    const other = cp.spawn(ws.exe, [STANDIN], { stdio: 'ignore', env: Object.assign({}, process.env, { STANDIN_MODE: 'silent' }) });
    ws.started.push(other.pid);
    await sleep(500);
    const r = launch(ws);
    assert.equal(r.code, 3, r.text);
    assert.match(r.text, /LAUNCH\|RUNNING\|\d+\|/);
    assert.match(r.text, /LAUNCH\|REFUSED\|/);
    assert.equal(alive(other.pid), true);
    assert.equal(fs.existsSync(path.join(ws.root, 'board.json')), false);
    assert.equal(fs.existsSync(path.join(ws.root, 'fake')), false, '거절하면 아무것도 만들지 않는다');
  } finally {
    await ws.cleanup();
  }
});

test('포트를 쥔 것이 우리 자손이 아니면 4(NOT-OURS)이고 우리가 띄운 것만 끈다', { skip: SKIP }, async () => {
  const ws = workspace();
  // 남의 프로세스가 포트를 쥐고 /json 에 그럴듯하게 답한다 — 별도 프로세스여야 한다(spawnSync 가 이 시험의 이벤트 루프를 막는다)
  const probe = http.createServer();
  await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
  const port = probe.address().port;
  await new Promise((resolve) => probe.close(resolve));
  const squatterCode = `require('http').createServer((q, s) => { s.setHeader('content-type', 'application/json'); s.end(JSON.stringify([{ type: 'page', title: 'Claude Board', url: 'x', webSocketDebuggerUrl: 'ws://127.0.0.1:1/x' }])); }).listen(${port}, '127.0.0.1')`;
  const squatter = cp.spawn(process.execPath, ['-e', squatterCode], { stdio: 'ignore' });
  ws.started.push(squatter.pid);
  try {
    const up = Date.now() + 5000;
    for (;;) {
      try {
        await fetch(`http://127.0.0.1:${port}/json`);
        break;
      } catch {
        if (Date.now() > up) throw new Error('포트 점유 프로세스가 안 떴다');
        await sleep(50);
      }
    }
    const r = launch(ws, ['-AllowOtherBoards', '-Port', String(port)], { STANDIN_MODE: 'silent' });
    assert.equal(r.code, 4, r.text);
    assert.match(r.text, /LAUNCH\|NOT-OURS\|/);
    assert.equal(alive(squatter.pid), true, '포트 주인은 우리 것이 아니므로 끄지 않는다');
    const started = Number(/started pid=(\d+)/.exec(r.text)[1]);
    await sleep(500);
    assert.equal(alive(started), false, '우리가 띄운 보드(대역)는 껐다');
  } finally {
    await ws.cleanup();
  }
});

test('test_claude_prefix 가 기대와 다르면 5 이고 우리가 띄운 보드를 끈다', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    const r = launch(ws, ['-AllowOtherBoards'], { STANDIN_MODE: 'wrongprefix' });
    assert.equal(r.code, 5, r.text);
    assert.match(r.text, /LAUNCH\|PREFIX-MISMATCH\|/);
    const record = JSON.parse(fs.readFileSync(path.join(ws.root, 'board.json'), 'utf8'));
    await sleep(800);
    assert.equal(alive(record.pid), false, '보드를 껐다');
  } finally {
    await ws.cleanup();
  }
});

test('-Stop 은 기록한 보드가 아니면(pid 가 다른 프로세스로 바뀜) 3 으로 거절한다', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    fs.mkdirSync(ws.root, { recursive: true });
    const bystander = cp.spawn(process.execPath, ['-e', 'setTimeout(()=>{}, 60000)'], { stdio: 'ignore' });
    ws.started.push(bystander.pid);
    await sleep(300);
    fs.writeFileSync(path.join(ws.root, 'board.json'), JSON.stringify({ pid: bystander.pid, startTime: '2000-01-01T00:00:00.0000000Z', exe: ws.exe }));
    const r = launch(ws, ['-Stop']);
    assert.equal(r.code, 3, r.text);
    assert.match(r.text, /LAUNCH\|REFUSED\|/);
    assert.equal(alive(bystander.pid), true);
    // 기록이 없으면 역시 3
    fs.rmSync(path.join(ws.root, 'board.json'));
    assert.equal(launch(ws, ['-Stop']).code, 3);
  } finally {
    await ws.cleanup();
  }
});

test('Root 검사 — 상대 경로·사용자 홈·~\\.claude·문서\\PowerShell 안·회사 경로는 2', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    const home = process.env.USERPROFILE;
    const docs = cp.spawnSync('pwsh', ['-NoProfile', '-Command', "[Environment]::GetFolderPath('MyDocuments')"], { encoding: 'utf8' }).stdout.trim();
    const attempts = ['run', 'C:run', home, path.join(home, '.claude'), path.join(home, '.claude', 'x'), path.join(docs, 'PowerShell'), path.join(docs, 'PowerShell', 'x'), path.join(docs, 'WindowsPowerShell', 'x'), path.join(ws.dir, 'aijinet-project', 'run')];
    for (const root of attempts) {
      // 없는 exe 를 준다 — 검사가 깨져 통과해도 exe 확인(6)에서 멈춰 사용자 폴더에 아무것도 만들지 않는다
      const copy = { ...ws, root, exe: path.join(ws.dir, 'none', 'ClaudeBoard.exe') };
      const r = launch(copy, ['-AllowOtherBoards']);
      assert.equal(r.code, 2, `${root}: ${r.text}`);
      assert.match(r.text, /LAUNCH\|USAGE\|/);
    }
    // 거절한 자리에는 아무것도 만들지 않았다
    assert.equal(fs.existsSync(path.join(home, '.claude', 'x')), false);
    assert.equal(fs.existsSync(path.join(docs, 'PowerShell', 'x')), false);
  } finally {
    await ws.cleanup();
  }
});

test('없는 exe·가짜 스크립트·node 는 6', { skip: SKIP }, async () => {
  const ws = workspace();
  try {
    const noExe = launch({ ...ws, exe: path.join(ws.dir, 'none', 'ClaudeBoard.exe') }, ['-AllowOtherBoards']);
    assert.equal(noExe.code, 6, noExe.text);
    const noScript = launch(ws, ['-AllowOtherBoards', '-FakeScript', path.join(ws.dir, 'none.cjs')]);
    assert.equal(noScript.code, 6, noScript.text);
    const noNode = launch(ws, ['-AllowOtherBoards', '-Node', path.join(ws.dir, 'none-node.exe')]);
    assert.equal(noNode.code, 6, noNode.text);
  } finally {
    await ws.cleanup();
  }
});
