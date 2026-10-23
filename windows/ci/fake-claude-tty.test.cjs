'use strict';
// fake-claude-tty.cjs 자체 시험 — 파이프로 띄운 자식에 키를 써 넣고 키 기록(JSON 줄)을 확인한다. GUI·ConPTY 는 쓰지 않는다.
// 실행: node --test windows/ci/fake-claude-tty.test.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const SCRIPT = path.join(__dirname, 'fake-claude-tty.cjs');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitFor(predicate, what, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = predicate();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`시간 초과: ${what}`);
    await sleep(20);
  }
}

// 자식 하나와 그 기록 — dir 은 시험마다 새 임시 폴더(홈·키 기록·hook 기록이 모두 이 안에 있다)
function start({ args = [], env = {}, hooks = false } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-claude-tty-'));
  const keylog = path.join(dir, 'keys.log');
  const child = cp.spawn(process.execPath, [SCRIPT, ...args], {
    cwd: dir,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: Object.assign({}, process.env, { CLAUDE_BOARD_HOME: dir, FAKE_CLAUDE_KEYLOG: keylog, FAKE_CLAUDE_MAX_MS: '20000' }, hooks ? {} : { FAKE_CLAUDE_HOOKS: '0' }, env),
  });
  let stdout = '';
  child.stdout.on('data', (d) => (stdout += d.toString('utf8')));
  const exited = new Promise((resolve) => child.on('exit', (code, signal) => resolve({ code, signal })));
  const lines = () => (fs.existsSync(keylog) ? fs.readFileSync(keylog, 'utf8').split('\n').filter(Boolean).map((row) => JSON.parse(row)) : []);
  const chunks = () => lines().filter((l) => l.hex !== undefined);
  const events = (name) => lines().filter((l) => l.event === name);
  // 조각 하나를 쓰고 기록에 그 조각이 나타날 때까지 기다린다(파이프에서 조각이 합쳐지지 않게)
  async function send(bytes) {
    const before = chunks().length;
    child.stdin.write(Buffer.from(bytes));
    await waitFor(() => chunks().length > before, `조각 ${Buffer.from(bytes).toString('hex')}`);
    await sleep(30);
  }
  // 자식이 작업 폴더를 쥐고 있으면 Windows 에서 지워지지 않으므로 끝나기를 기다린 뒤 지운다
  const cleanup = async () => {
    child.kill();
    await exited;
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 50 });
  };
  return { dir, child, keylog, lines, chunks, events, send, exited, stdout: () => stdout, cleanup };
}

async function ready(t) {
  await waitFor(() => t.events('start').length === 1, '시작 기록');
  await waitFor(() => t.stdout().includes('fake-claude-tty ready'), '준비 문구');
}

test('홀로 온 Esc — 1b 한 조각이 esc 로 기록되고 esc 사건이 남는다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send([0x1b]);
    await waitFor(() => t.events('esc').length === 1, 'esc 사건');
    assert.deepEqual(t.chunks().map((c) => [c.hex, c.len, c.kind]), [['1b', 1, 'esc']]);
    assert.equal(t.lines().filter((l) => l.event === 'line').length, 0);
  } finally {
    await t.cleanup();
  }
});

test('Enter 한 바이트 — 0d 조각, 빈 줄 사건, 어떤 hook 도 안 부른다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send([0x0d]);
    await waitFor(() => t.events('line').length === 1, '줄 사건');
    assert.deepEqual(t.chunks().map((c) => [c.hex, c.kind]), [['0d', 'enter']]);
    assert.equal(t.events('line')[0].text, '');
    assert.equal(t.events('hook').length, 0);
  } finally {
    await t.cleanup();
  }
});

test('글 — 16진수로 기록하고 Enter 가 오면 줄 사건과 되돌려 쓰기를 남긴다(한글 포함)', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send(Buffer.from('hello'));
    await t.send(Buffer.from('안녕'));
    assert.deepEqual(
      t.chunks().map((c) => [c.hex, c.kind]),
      [[Buffer.from('hello').toString('hex'), 'text'], [Buffer.from('안녕').toString('hex'), 'text']],
    );
    assert.equal(t.events('line').length, 0, 'Enter 전에는 줄이 없다');
    await t.send([0x0d]);
    await waitFor(() => t.events('line').length === 1, '줄 사건');
    assert.equal(t.events('line')[0].text, 'hello안녕');
    await waitFor(() => t.stdout().includes('received: hello안녕'), '되돌려 쓰기');
  } finally {
    await t.cleanup();
  }
});

test('글과 Enter 가 한 조각으로 와도 text+enter 로 기록하고 줄을 낸다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send(Buffer.from('abc\r'));
    await waitFor(() => t.events('line').length === 1, '줄 사건');
    assert.deepEqual(t.chunks().map((c) => [c.hex, c.kind]), [['6162630d', 'text+enter']]);
    assert.equal(t.events('line')[0].text, 'abc');
  } finally {
    await t.cleanup();
  }
});

test('방향키 같은 이스케이프 시퀀스(1b5b41)는 seq 이고 esc 사건도 글도 아니다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send([0x1b, 0x5b, 0x41]);
    await t.send([0x0d]);
    await waitFor(() => t.events('line').length === 1, '줄 사건');
    assert.equal(t.chunks()[0].kind, 'seq');
    assert.equal(t.events('esc').length, 0);
    assert.equal(t.events('line')[0].text, '');
  } finally {
    await t.cleanup();
  }
});

test('백스페이스는 줄에서 한 글자를 지운다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send(Buffer.from('abx'));
    await t.send([0x7f]);
    await t.send(Buffer.from('c'));
    await t.send([0x0d]);
    await waitFor(() => t.events('line').length === 1, '줄 사건');
    assert.equal(t.events('line')[0].text, 'abc');
  } finally {
    await t.cleanup();
  }
});

test('/exit + Enter — 종료 코드 0 으로 끝나고 exit 사건이 남는다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send(Buffer.from('/exit'));
    await t.send([0x0d]);
    const result = await t.exited;
    assert.deepEqual(result, { code: 0, signal: null });
    assert.equal(t.events('line')[0].text, '/exit');
    assert.equal(t.events('exit')[0].code, 0);
    assert.match(t.stdout(), /bye/);
  } finally {
    await t.cleanup();
  }
});

test('중단 순서 — Esc, 쉼, /exit, 쉼, Enter 의 바이트와 간격이 기록에 그대로 남는다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send([0x1b]);
    await sleep(400);
    await t.send(Buffer.from('/exit'));
    await sleep(200);
    await t.send([0x0d]);
    await t.exited;
    const chunks = t.chunks();
    assert.deepEqual(chunks.map((c) => c.hex), ['1b', Buffer.from('/exit').toString('hex'), '0d']);
    assert.ok(chunks[1].gapMs >= 350, `Esc 와 /exit 사이 ${chunks[1].gapMs} ms`);
    assert.ok(chunks[2].gapMs >= 150, `/exit 와 Enter 사이 ${chunks[2].gapMs} ms`);
    assert.equal(t.events('esc').length, 1);
  } finally {
    await t.cleanup();
  }
});

test('/rename 이름 + Enter — 이름 사건, 세션 파일의 name, 되돌려 쓰기, 프로세스는 계속 산다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send(Buffer.from('/rename 작업A'));
    await t.send([0x0d]);
    await waitFor(() => t.events('rename').length === 1, '이름 사건');
    assert.equal(t.events('rename')[0].name, '작업A');
    const record = JSON.parse(fs.readFileSync(path.join(t.dir, '.claude', 'sessions', `${t.child.pid}.json`), 'utf8'));
    assert.equal(record.name, '작업A');
    assert.equal(record.pid, t.child.pid);
    await waitFor(() => t.stdout().includes('renamed: 작업A'), '되돌려 쓰기');
    assert.equal(t.child.exitCode, null);
  } finally {
    await t.cleanup();
  }
});

test('/rename 만 치면 이름을 바꾸지 않는다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send(Buffer.from('/rename'));
    await t.send([0x0d]);
    await waitFor(() => t.events('line').length === 1, '줄 사건');
    assert.equal(t.events('rename').length, 0);
  } finally {
    await t.cleanup();
  }
});

test('Ctrl+C(03) 는 기록하고 130 으로 끝난다', async () => {
  const t = start();
  try {
    await ready(t);
    t.child.stdin.write(Buffer.from([0x03]));
    const result = await t.exited;
    assert.equal(result.code, 130);
    assert.equal(t.chunks()[0].hex, '03');
    assert.equal(t.events('ctrl-c').length, 1);
  } finally {
    await t.cleanup();
  }
});

test('표준입력이 닫히면 0 으로 끝난다', async () => {
  const t = start();
  try {
    await ready(t);
    t.child.stdin.end();
    assert.deepEqual(await t.exited, { code: 0, signal: null });
  } finally {
    await t.cleanup();
  }
});

test('인자 — argv 를 시작 사건과 FAKE_CLAUDE_ARGVLOG 에 남기고 --session-id·--name 을 쓴다', async () => {
  const argvlog = path.join(os.tmpdir(), `fake-claude-tty-argv-${process.pid}-${Date.now()}.log`);
  const t = start({ args: ['--resume', 'sid-1234-abcd', '--name', '내 작업'], env: { FAKE_CLAUDE_ARGVLOG: argvlog } });
  try {
    await ready(t);
    const start0 = t.events('start')[0];
    assert.deepEqual(start0.argv, ['--resume', 'sid-1234-abcd', '--name', '내 작업']);
    assert.equal(start0.sessionId, 'sid-1234-abcd');
    assert.deepEqual(JSON.parse(fs.readFileSync(argvlog, 'utf8').trim()), ['--resume', 'sid-1234-abcd', '--name', '내 작업']);
    const record = JSON.parse(fs.readFileSync(path.join(t.dir, '.claude', 'sessions', `${t.child.pid}.json`), 'utf8'));
    assert.equal(record.name, '내 작업');
    assert.equal(record.sessionId, 'sid-1234-abcd');
    assert.ok(fs.existsSync(path.join(t.dir, '.claude', 'projects', 'fake-claude-tty', 'sid-1234-abcd.jsonl')), '대화 기록 파일');
  } finally {
    await t.cleanup();
    fs.rmSync(argvlog, { force: true });
  }
});

test('키 기록 파일이 없어도(환경 변수 없음) 오류 없이 돈다', async () => {
  const t = start({ env: { FAKE_CLAUDE_KEYLOG: '' } });
  try {
    await waitFor(() => t.stdout().includes('fake-claude-tty ready'), '준비 문구');
    t.child.stdin.write(Buffer.from([0x1b]));
    await sleep(100);
    assert.equal(t.child.exitCode, null);
    assert.equal(fs.existsSync(t.keylog), false);
  } finally {
    await t.cleanup();
  }
});

test('조각 기록은 시각이 오름차순이고 줄마다 JSON 하나다', async () => {
  const t = start();
  try {
    await ready(t);
    await t.send([0x1b]);
    await t.send(Buffer.from('x'));
    await t.send([0x0d]);
    const raw = fs.readFileSync(t.keylog, 'utf8');
    assert.ok(raw.endsWith('\n'));
    const times = t.lines().map((l) => l.t);
    assert.deepEqual([...times].sort((a, b) => a - b), times);
    for (const l of t.lines()) assert.ok(Number.isInteger(l.ms) && l.ms >= 0 && l.gapMs >= 0);
  } finally {
    await t.cleanup();
  }
});

// ── hook ────────────────────────────────────────────────────────────────────

// settings.json 에 hook 둘(우리 것 + 남의 것)을 등록하고, 우리 것만 stdin 을 파일에 덧붙이게 한다
function hookSetup() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-claude-tty-hooks-'));
  const recorder = path.join(dir, 'record-hook.cjs');
  const sink = path.join(dir, 'hook-calls.jsonl');
  fs.writeFileSync(
    recorder,
    "const fs=require('fs');let s='';process.stdin.on('data',d=>s+=d);process.stdin.on('end',()=>fs.appendFileSync(process.argv[2],JSON.stringify({label:process.argv[3],cwd:process.cwd(),project:process.env.CLAUDE_PROJECT_DIR,input:JSON.parse(s)})+'\\n'));",
  );
  const mine = (label) => ({ type: 'command', command: process.execPath, args: [recorder, sink, label], timeout: 10 });
  const other = { type: 'command', command: process.execPath, args: ['-e', "require('fs').appendFileSync(process.argv[1],'LEAK\\n')", sink] };
  const settings = {
    hooks: {
      UserPromptSubmit: [{ hooks: [mine('state working'), other] }],
      SessionEnd: [{ hooks: [mine('state ended')] }],
    },
  };
  const settingsPath = path.join(dir, 'settings.json');
  fs.writeFileSync(settingsPath, '﻿' + JSON.stringify(settings));
  const calls = () => (fs.existsSync(sink) ? fs.readFileSync(sink, 'utf8').split('\n').filter(Boolean).map((r) => (r === 'LEAK' ? { leak: true } : JSON.parse(r))) : []);
  return { dir, settingsPath, calls, marker: 'record-hook.cjs' };
}

test('hook — 시작·글 입력·/exit 에서 등록된 우리 hook 만 stdin JSON 과 함께 불린다', async () => {
  const h = hookSetup();
  const t = start({ hooks: true, env: { FAKE_CLAUDE_SETTINGS: h.settingsPath, FAKE_CLAUDE_HOOK_MARKER: h.marker, FAKE_CLAUDE_SESSION_ID: 'sess-hook-0001' } });
  try {
    await ready(t);
    await waitFor(() => h.calls().length === 1, '시작 hook');
    await t.send(Buffer.from('일 시작'));
    await t.send([0x0d]);
    await waitFor(() => h.calls().length === 2, '글 hook');
    await t.send(Buffer.from('/exit'));
    await t.send([0x0d]);
    await t.exited;
    const calls = h.calls();
    assert.deepEqual(calls.map((c) => c.label), ['state working', 'state working', 'state ended']);
    assert.ok(!calls.some((c) => c.leak), '남의 hook 은 불리지 않는다');
    assert.deepEqual(calls.map((c) => c.input.hook_event_name), ['UserPromptSubmit', 'UserPromptSubmit', 'SessionEnd']);
    assert.equal(calls[1].input.prompt, '일 시작');
    for (const c of calls) {
      assert.equal(c.input.session_id, 'sess-hook-0001');
      assert.equal(fs.realpathSync(c.input.cwd), fs.realpathSync(t.dir));
      assert.equal(fs.realpathSync(c.project), fs.realpathSync(t.dir));
      assert.ok(fs.existsSync(c.input.transcript_path), '대화 기록 파일이 있어야 이름 바꾸기가 통과한다');
    }
    assert.equal(t.events('hook').length, 3);
    assert.ok(t.events('hook').every((e) => e.status === 0));
  } finally {
    await t.cleanup();
    fs.rmSync(h.dir, { recursive: true, force: true });
  }
});

test('hook — FAKE_CLAUDE_HOOKS=0 이면 등록돼 있어도 부르지 않는다', async () => {
  const h = hookSetup();
  const t = start({ hooks: false, env: { FAKE_CLAUDE_SETTINGS: h.settingsPath, FAKE_CLAUDE_HOOK_MARKER: h.marker } });
  try {
    await ready(t);
    await t.send(Buffer.from('x'));
    await t.send([0x0d]);
    await sleep(200);
    assert.deepEqual(h.calls(), []);
  } finally {
    await t.cleanup();
    fs.rmSync(h.dir, { recursive: true, force: true });
  }
});

test('hook — settings.json 이 없거나 깨져 있어도 조용히 hook 없이 돈다', async () => {
  for (const content of [null, '{ not json']) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-claude-tty-bad-'));
    const settingsPath = path.join(dir, 'settings.json');
    if (content !== null) fs.writeFileSync(settingsPath, content);
    const t = start({ hooks: true, env: { FAKE_CLAUDE_SETTINGS: settingsPath } });
    try {
      await ready(t);
      await t.send(Buffer.from('x'));
      await t.send([0x0d]);
      await waitFor(() => t.events('line').length === 1, '줄 사건');
      assert.equal(t.events('hook').length, 0);
      assert.equal(t.child.exitCode, null);
    } finally {
      await t.cleanup();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }
});
