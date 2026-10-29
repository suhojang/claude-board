'use strict';
// fake-claude-login.cjs 자체 시험 — 파이프로 띄운 자식에 코드를 써 넣고 출력·종료 코드·로그인 자리 파일을 확인한다.
// 실행: node --test windows/ci/fake-claude-login.test.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const SCRIPT = path.join(__dirname, 'fake-claude-login.cjs');
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

// 맥 보드 loginOutput 과 같은 읽기 — 색 글자를 빼고, 주소는 `https://…oauth/authorize…` 뒤 공백까지
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007]*\u0007/g;
const URL_PATTERN = /https:\/\/[^\s]+oauth\/authorize[^\s]*(?=\s)/;
// 맥 보드 loginEnded 와 같은 읽기 — `prompted >` 뒤를 자르고 마지막 비지 않은 줄
function failureReason(text) {
  const lines = text
    .split(/\r?\n|\r/)
    .map((line) => line.split('prompted >').pop().trim())
    .filter(Boolean);
  return lines.length ? lines[lines.length - 1] : '';
}

function start({ args = ['auth', 'login', '--claudeai'], env = {}, slot = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-claude-login-'));
  const slotDir = path.join(dir, 'slot');
  const homeDir = path.join(dir, 'home');
  fs.mkdirSync(homeDir);
  const log = path.join(dir, 'login.log');
  const base = Object.assign({}, process.env, { CLAUDE_BOARD_HOME: homeDir, USERPROFILE: homeDir, HOME: homeDir, FAKE_LOGIN_LOG: log, FAKE_LOGIN_MAX_MS: '20000' });
  for (const key of ['CLAUDE_CONFIG_DIR', 'FAKE_LOGIN_CODE', 'FAKE_LOGIN_EMAIL', 'FAKE_LOGIN_AUTO_MS']) delete base[key];
  if (slot) base.CLAUDE_CONFIG_DIR = slotDir;
  const child = cp.spawn(process.execPath, [SCRIPT, ...args], { cwd: dir, stdio: ['pipe', 'pipe', 'pipe'], env: Object.assign(base, env) });
  let text = '';
  child.stdout.on('data', (d) => (text += d.toString('utf8')));
  child.stderr.on('data', (d) => (text += d.toString('utf8')));
  const exited = new Promise((resolve) => child.on('exit', (code) => resolve(code)));
  const rows = () => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(Boolean).map((row) => JSON.parse(row)) : []);
  const cleanup = async () => {
    child.kill();
    await exited;
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 50 });
  };
  return { dir, slotDir, homeDir, child, exited, rows, cleanup, text: () => text.replace(ANSI, ''), json: (name) => JSON.parse(fs.readFileSync(path.join(slotDir, name), 'utf8')) };
}

async function ready(t) {
  await waitFor(() => t.text().includes('Paste code here if prompted > '), '코드 입력 안내');
}

test('주소 줄 — 맥 파서가 읽는 oauth/authorize 주소가 나오고 코드 입력 안내가 뒤따른다', async () => {
  const t = start();
  try {
    await ready(t);
    const url = URL_PATTERN.exec(t.text());
    assert.ok(url, t.text());
    assert.match(url[0], /^https:\/\/login\.fake\.test\/oauth\/authorize\?/);
    assert.ok(t.text().indexOf(url[0]) < t.text().indexOf('Paste code here if prompted >'));
    assert.equal(t.rows().find((r) => r.event === 'url').url, url[0]);
    assert.equal(t.child.exitCode, null);
  } finally {
    await t.cleanup();
  }
});

test('맞는 코드 — 종료 코드 0, 로그인 자리에 계정·토큰 파일이 쓰인다(CRLF·앞뒤 공백도 받는다)', async () => {
  const t = start({ env: { FAKE_LOGIN_CODE: 'right-code' } });
  try {
    await ready(t);
    t.child.stdin.write('  right-code \r\n');
    assert.equal(await t.exited, 0);
    assert.match(t.text(), /Login successful\./);
    const account = t.json('.claude.json').oauthAccount;
    assert.equal(account.emailAddress, 'fake-login@example.test');
    assert.match(account.accountUuid, /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
    const oauth = t.json('.credentials.json').claudeAiOauth;
    assert.ok(oauth.accessToken.startsWith('fake-access-') && oauth.refreshToken.startsWith('fake-refresh-'));
    assert.ok(oauth.refreshTokenExpiresAt > Date.now());
    assert.deepEqual(t.rows().map((r) => r.event), ['start', 'url', 'code', 'write', 'exit']);
  } finally {
    await t.cleanup();
  }
});

test('--email 인자가 계정 이메일이 되고, 같은 이메일은 늘 같은 accountUuid 다', async () => {
  const uuids = [];
  for (let i = 0; i < 2; i += 1) {
    const t = start({ args: ['auth', 'login', '--claudeai', '--email', 'someone@example.test'] });
    try {
      await ready(t);
      t.child.stdin.write('fake-code-1234\n');
      assert.equal(await t.exited, 0);
      assert.equal(t.json('.claude.json').oauthAccount.emailAddress, 'someone@example.test');
      uuids.push(t.json('.claude.json').oauthAccount.accountUuid);
    } finally {
      await t.cleanup();
    }
  }
  assert.equal(uuids[0], uuids[1]);
});

test('틀린 코드 — 종료 코드 1, 같은 줄의 안내 뒤 마지막 줄이 실패 까닭, 자리에 아무것도 쓰지 않는다', async () => {
  const t = start();
  try {
    await ready(t);
    t.child.stdin.write('wrong\n');
    assert.equal(await t.exited, 1);
    assert.equal(failureReason(t.text()), 'Login failed: Invalid authorization code');
    assert.match(t.text(), /prompted > Login failed:/);
    assert.equal(fs.existsSync(t.slotDir), false);
    assert.equal(t.rows().some((r) => r.event === 'write'), false);
  } finally {
    await t.cleanup();
  }
});

test('빈 줄은 코드가 아니라 계속 기다린다 — 그 뒤 맞는 코드면 성공', async () => {
  const t = start();
  try {
    await ready(t);
    t.child.stdin.write('\r\n\n   \r');
    await sleep(150);
    assert.equal(t.child.exitCode, null);
    t.child.stdin.write('fake-code-1234\r');
    assert.equal(await t.exited, 0);
  } finally {
    await t.cleanup();
  }
});

test('코드를 한 글자씩 나눠 보내도 한 줄로 합쳐 읽고, 백스페이스는 한 글자를 지운다', async () => {
  const t = start({ env: { FAKE_LOGIN_CODE: 'abc' } });
  try {
    await ready(t);
    for (const ch of ['a', 'b', 'x', '\x7f', 'c', '\r']) {
      t.child.stdin.write(ch);
      await sleep(20);
    }
    assert.equal(await t.exited, 0);
  } finally {
    await t.cleanup();
  }
});

test('표준입력이 코드 없이 닫히면 종료 코드 1', async () => {
  const t = start();
  try {
    await ready(t);
    t.child.stdin.end();
    assert.equal(await t.exited, 1);
    assert.match(t.text(), /Login cancelled\./);
    assert.equal(fs.existsSync(t.slotDir), false);
  } finally {
    await t.cleanup();
  }
});

test('Ctrl+C(03) 는 130 으로 끝난다', async () => {
  const t = start();
  try {
    await ready(t);
    t.child.stdin.write('\x03');
    assert.equal(await t.exited, 130);
    assert.equal(fs.existsSync(t.slotDir), false);
  } finally {
    await t.cleanup();
  }
});

test('CLAUDE_CONFIG_DIR 가 없으면 아무것도 쓰지 않고 2 — 홈(.claude)도 만들지 않는다', async () => {
  const t = start({ slot: false });
  try {
    assert.equal(await t.exited, 2);
    assert.match(t.text(), /CLAUDE_CONFIG_DIR/);
    assert.equal(URL_PATTERN.test(t.text()), false);
    assert.deepEqual(fs.readdirSync(t.homeDir), []);
  } finally {
    await t.cleanup();
  }
});

test('CLAUDE_CONFIG_DIR 가 상대 경로여도 2', async () => {
  const t = start({ slot: false, env: { CLAUDE_CONFIG_DIR: 'relative\\slot' } });
  try {
    assert.equal(await t.exited, 2);
    assert.equal(fs.existsSync(path.join(t.dir, 'relative')), false);
  } finally {
    await t.cleanup();
  }
});

test('auth login 이 아니면 사용법을 쓰고 2', async () => {
  for (const args of [[], ['auth'], ['auth', 'logout'], ['--version']]) {
    const t = start({ args });
    try {
      assert.equal(await t.exited, 2, args.join(' '));
      assert.match(t.text(), /usage:/);
      assert.equal(fs.existsSync(t.slotDir), false);
    } finally {
      await t.cleanup();
    }
  }
});

test('FAKE_LOGIN_AUTO_MS — 코드 없이 스스로 성공한다(localhost 콜백 길)', async () => {
  const t = start({ env: { FAKE_LOGIN_AUTO_MS: '150' } });
  try {
    await ready(t);
    assert.equal(await t.exited, 0);
    assert.equal(t.json('.claude.json').oauthAccount.emailAddress, 'fake-login@example.test');
    assert.ok(t.rows().some((r) => r.event === 'auto'));
  } finally {
    await t.cleanup();
  }
});

test('시간 초과 — FAKE_LOGIN_MAX_MS 가 지나면 실패로 끝난다', async () => {
  const t = start({ env: { FAKE_LOGIN_MAX_MS: '200' } });
  try {
    await ready(t);
    assert.equal(await t.exited, 1);
    assert.equal(failureReason(t.text()), 'Login failed: timed out');
  } finally {
    await t.cleanup();
  }
});
